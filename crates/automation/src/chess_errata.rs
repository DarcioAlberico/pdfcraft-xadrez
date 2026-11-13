//! Chess ▸ notation errata as a tool (plan A9-N19): `chess_errata` reads the pages as
//! `chess_document` does, links them to the book's PGN when there is one, and lists what the book
//! printed that its moves do not bear out (`pdfcraft_chessvision::doc::errata`), as CSV or
//! Markdown, inline or to a file confined to `--root`.

use pdfcraft_engine::chess::pdfcraft_chess::pgn;
use pdfcraft_engine::chess::pdfcraft_chess::san::Lang;
use pdfcraft_engine::chess::pdfcraft_chessvision::doc::errata::{self, Issue};
use pdfcraft_engine::chess::pdfcraft_chessvision::doc::reference::{self, LinkKind};
use pdfcraft_engine::chess::pdfcraft_chessvision::doc::{Inline, MoveAt, PageLabel};
use pdfcraft_engine::chess::reference_store::Choice;
use serde_json::{Value, json};

use crate::{Args, Automation, Result, failed};

/// What `chess_errata` writes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    Csv,
    Markdown,
}

impl Automation {
    /// `chess_errata`: the notation errata of the pages read, as CSV or Markdown.
    pub(crate) fn chess_errata(&mut self, a: &Args) -> Result<Value> {
        let settings = self.chess_settings(a)?;
        let pages = if a.opt_ints("pages")?.is_some() { self.pages(a, "pages")? } else { Vec::new() };
        let lang = match a.opt_str("lang")? {
            None => None,
            Some(tag) => Some(Lang::from_tag(tag).ok_or_else(|| Args::wrong("lang", "a piece-letter language such as en, de, pt or fr"))?),
        };
        let format = match a.opt_str("format")? {
            None | Some("csv") => Format::Csv,
            Some("markdown") => Format::Markdown,
            Some(_) => return Err(Args::wrong("format", "\"csv\" or \"markdown\"")),
        };
        // Resolved before reading, so a path outside the root costs nothing.
        let out = a.opt_str("out")?.map(|p| self.resolve(p, true).map(|r| (p.to_owned(), r))).transpose()?;
        let id = self.doc(a)?.id;
        let (reference, reference_kept) = self.book_pgn(a, id)?;
        let (seed, _) = self.cipher_args(a)?;
        if let Some((_, path)) = &out {
            let inputs = self.input_paths(a)?;
            self.check_outputs(a, &[("out", path.as_path())], &inputs.iter().map(|p| p.as_path()).collect::<Vec<_>>())?;
        }
        let title = match a.opt_str("title")? {
            Some(t) => t.to_owned(),
            None => self.doc(a)?.name.trim_end_matches(".pdf").to_owned(),
        };
        let ocr = crate::chess::ocr_options(a)?.with_book(seed.as_ref());
        let loaded = self.use_store_corrections(a, id)?;
        let mut read = self.session.chess_document_with(id, &pages, &settings, lang, &ocr).map_err(failed)?;
        let mut reference_reply = Value::Null;
        // The moves the user kept as the book printed them against the PGN (Choice::Book): the
        // PGN was the one wrong there, so they are no erratum of the book.
        let mut kept_as_printed: Vec<MoveAt> = Vec::new();
        if let Some(r) = &reference {
            let linked = reference::link(&mut read.doc, &r.index);
            for page in &read.doc.pages {
                let on = r.decisions.on_page(page);
                kept_as_printed.extend(on.found.iter().filter(|(_, c)| *c == Choice::Book).map(|(at, _)| *at));
            }
            reference_reply = crate::chess::reference_reply(&r.shown, &r.index, &linked);
        }
        let all = errata::errata(&read.doc, reference.as_ref().map(|r| &r.index));
        // The moves in conflict with the PGN that are no row: read by OCR, or a word repaired.
        let conflicts = read
            .doc
            .pages
            .iter()
            .flat_map(|p| p.blocks.iter().flat_map(|b| b.inlines()))
            .filter(|i| matches!(i, Inline::Move(m) if m.reference.as_ref().is_some_and(|r| r.kind == LinkKind::Conflict)))
            .count();
        let not_judged = conflicts.saturating_sub(all.iter().filter(|e| e.issue == Issue::BookPgn).count());
        let (rows, set_aside): (Vec<_>, Vec<_>) = all.into_iter().partition(|e| !(e.issue == Issue::BookPgn && kept_as_printed.contains(&e.at)));
        if let Some(o) = reference_reply.as_object_mut() {
            o.insert("kept_as_printed".into(), json!(set_aside.len()));
            o.insert("not_judged".into(), json!(not_judged));
        }
        // As the book numbers its pages: its /PageLabels, else the numbers it prints (plan A9-L1b).
        let labels: Vec<String> = self.doc(a)?.info.pages.iter().map(|p| p.label.clone()).collect();
        let labels = PageLabel::with_folios(&labels, &read.doc);
        let text = match format {
            Format::Csv => errata::to_csv(&rows, &labels),
            Format::Markdown => errata::to_markdown(&rows, &labels, &title),
        };
        let issues: serde_json::Map<String, Value> =
            Issue::ALL.iter().map(|i| (i.name().to_owned(), json!(rows.iter().filter(|e| e.issue == *i).count()))).collect();
        let doc = &read.doc;
        let mut result = json!({
            "format": if format == Format::Csv { "csv" } else { "markdown" },
            "rows": rows.len(),
            "issues": issues,
            "pages": doc.pages.len(),
            "born_digital_pages": doc.pages.iter().filter(|p| errata::born_digital(p)).count(),
            "ocr_pages": doc.pages.iter().filter(|p| p.ocr).count(),
            "needs_ocr": read.needs_ocr.iter().map(|(p, why)| json!({ "page": p + 1, "reason": why })).collect::<Vec<_>>(),
            "notes": read.notes.iter().map(|(p, n)| json!({ "page": p + 1, "note": n })).collect::<Vec<_>>(),
            "corrections": crate::chess_corrections::corrections_reply(loaded, &read),
            "reference": reference_reply,
            "reference_kept": reference_kept,
        });
        match out {
            Some((shown, path)) => {
                // A CSV starts with a byte order mark only when asked, as a PGN does (Excel needs one
                // to read the figurines and accents).
                let bom = format == Format::Csv && a.opt_bool("bom")?.unwrap_or(false);
                let bytes = if bom { [pgn::BOM, text.as_bytes()].concat() } else { text.into_bytes() };
                crate::write_atomic(&path, &bytes)?;
                result["out"] = json!(shown);
                result["bytes"] = json!(bytes.len());
            }
            None => result["text"] = json!(text),
        }
        Ok(result)
    }
}
