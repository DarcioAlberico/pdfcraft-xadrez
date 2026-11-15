//! `chess_export_worksheet`: the book's exercises as a sheet to print, a new PDF (plan A9-N12a,
//! finding A8-14). The format in the app's Export dialog is lane C's (N12b, with X2).
//!
//! The pages are read as every export reads them (the book's PGN, the user's games, the
//! corrections, the cache), the look is the exports' (`pieces`, `board_theme`), and the output
//! follows their rule: confined to `--root`, never the open book's file nor a file this call reads
//! unless `overwrite`.

use pdfcraft_engine::chess::pdfcraft_chess::san::Lang;
use pdfcraft_engine::chess::pdfcraft_chessvision::doc::worksheet::{Layout, PerPage};
use pdfcraft_engine::chess::worksheet::{Paper, SheetOptions};
use serde_json::{Value, json};

use crate::chess::{lang_of, needs_ocr_reply, notes_reply, ocr_options};
use crate::{Args, Automation, Result, failed};

impl Automation {
    pub(crate) fn chess_export_worksheet(&mut self, a: &Args) -> Result<Value> {
        let lang = a.opt_str("lang")?.map(|t| lang_of("lang", t)).transpose()?;
        let per_page = match a.opt_int("per_page")? {
            None => PerPage::Four,
            Some(n) => u64::try_from(n).ok().and_then(PerPage::of).ok_or_else(|| Args::wrong("per_page", "4, 6 or 9"))?,
        };
        let paper = match a.opt_str("paper")? {
            None => Paper::A4,
            Some(p) => Paper::of(p).ok_or_else(|| Args::wrong("paper", "\"a4\" or \"letter\""))?,
        };
        let caption_lang = a.opt_str("caption_lang")?.map(|t| lang_of("caption_lang", t)).transpose()?;
        let layout = Layout { per_page, solutions: a.opt_bool("solutions")?.unwrap_or(true), lang: caption_lang };
        let ocr = ocr_options(a)?.mode;
        let (book, _) = self.cipher_args(a)?;
        let scan = self.chess_settings(a)?;
        let (cache, disk_before) = (scan.cache.clone(), self.session.chess_disk());
        let pages = if a.opt_ints("pages")?.is_some() { self.pages(a, "pages")? } else { Vec::new() };
        let shown = a.str("out")?.to_owned();
        // Resolved before reading, so a path outside the root costs nothing.
        let path = self.resolve(&shown, true)?;
        let id = self.doc(a)?.id;
        let (book_pgn, reference_kept) = self.book_pgn(a, id)?;
        let games = match a.opt_str("games_pgn")? {
            Some(p) => Some(self.read_reference(&self.resolve(p, false)?, p)?),
            None => None,
        };
        let title = match a.opt_str("title")? {
            Some(t) => t.to_owned(),
            None => self.doc(a)?.name.trim_end_matches(".pdf").to_owned(),
        };
        let (mut options, look) = self.export_look(a, &title, lang.unwrap_or(Lang::En))?;
        self.check_outputs(a, &[("out", path.as_path())], &self.input_paths(a)?.iter().map(|p| p.as_path()).collect::<Vec<_>>())?;
        options.pages = pages;
        options.scan = scan;
        options.lang = lang;
        options.ocr = ocr;
        options.book = book;
        options.reference = book_pgn.as_ref().map(|r| r.index.clone());
        options.games = games;
        let loaded = self.use_store_corrections(a, id)?;
        let made = self.session.chess_worksheet(id, &options, &SheetOptions { layout, paper }).map_err(failed)?;
        crate::write_atomic(&path, &made.bytes)?;
        let read = &made.read;
        let mut notes: Vec<Value> = notes_reply(&read.notes);
        notes.extend(made.notes.iter().map(|n| json!({ "note": n })));
        Ok(json!({
            "out": shown,
            "bytes": made.bytes.len(),
            "pages": made.diagram_pages.saturating_add(made.key_pages),
            "diagram_pages": made.diagram_pages,
            "key_pages": made.key_pages,
            "per_page": per_page.count(),
            "paper": if paper == Paper::A4 { "a4" } else { "letter" },
            "exercises": made.sheet.items().map(|i| json!({
                "page": i.at.page.saturating_add(1),
                "diagram": i.at.index.saturating_add(1),
                "number": i.number,
                "caption": i.caption,
                "solution": i.solution,
                "side_assumed": i.side_assumed,
                "from_black": i.orientation == pdfcraft_engine::chess::pdfcraft_chess::placement::Color::Black,
            })).collect::<Vec<_>>(),
            "exercises_left_out": made.sheet.left_out.iter().map(|x| json!({
                "page": x.at.page.saturating_add(1),
                "diagram": x.at.index.saturating_add(1),
                "number": x.number,
                "reason": x.reason.name(),
            })).collect::<Vec<_>>(),
            "over": made.sheet.over,
            "in_english": made.in_english,
            "pieces": look.pieces,
            "needs_ocr": needs_ocr_reply(&read.needs_ocr),
            "notes": notes,
            "corrections": crate::chess_corrections::corrections_reply(loaded, read),
            "reference_kept": reference_kept,
            "cache": self.cache_reply(cache.as_ref(), disk_before),
        }))
    }
}
