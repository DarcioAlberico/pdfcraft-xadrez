//! Chess ▸ the book's bookmarks as a tool (plan A9-N6, A8-13): `chess_bookmarks` reads the pages
//! as `chess_document` does and makes the book's outline (chapters, games and exercises,
//! `pdfcraft_chessvision::doc::bookmarks`) under one bookmark of its own, in one undoable step;
//! `dry_run` only shows it. It also gives the index of the book's games as CSV or Markdown, inline
//! or to a file confined to `--root`.

use pdfcraft_engine::Edit;
use pdfcraft_engine::chess::pdfcraft_chess::pgn;
use pdfcraft_engine::chess::pdfcraft_chess::san::Lang;
use pdfcraft_engine::chess::pdfcraft_chessvision::doc::PageLabel;
use pdfcraft_engine::chess::pdfcraft_chessvision::doc::bookmarks::{self, EntryKind, Include};
use serde_json::{Value, json};

use crate::{Args, Automation, Result, failed};

/// The book's own bookmark, when the call names none.
pub const DEFAULT_TITLE: &str = "Chess";

/// What an exercise's bookmark is called before its number, when the call says nothing.
pub const DEFAULT_EXERCISE: &str = "Exercise";

impl Automation {
    /// `chess_bookmarks`: the book's outline, shown (`dry_run`) or made, and its index of games.
    pub(crate) fn chess_bookmarks(&mut self, a: &Args) -> Result<Value> {
        let settings = self.chess_settings(a)?;
        let pages = if a.opt_ints("pages")?.is_some() { self.pages(a, "pages")? } else { Vec::new() };
        let lang = match a.opt_str("lang")? {
            None => None,
            Some(tag) => Some(Lang::from_tag(tag).ok_or_else(|| Args::wrong("lang", "a piece-letter language such as en, de, pt or fr"))?),
        };
        let include = match a.get("include") {
            None => Include::default(),
            Some(kinds) => {
                let kinds = kinds.as_array().ok_or_else(|| Args::wrong("include", "an array of \"chapters\", \"games\" and \"exercises\""))?;
                let mut include = Include { chapters: false, games: false, exercises: false };
                for kind in kinds {
                    match kind.as_str().unwrap_or_default() {
                        "chapters" => include.chapters = true,
                        "games" => include.games = true,
                        "exercises" => include.exercises = true,
                        _ => return Err(Args::wrong("include", "\"chapters\", \"games\" or \"exercises\"")),
                    }
                }
                include
            }
        };
        let title = a.opt_str("title")?.map(str::trim).unwrap_or(DEFAULT_TITLE).to_owned();
        if title.is_empty() || title.chars().count() > bookmarks::MAX_TITLE_CHARS {
            return Err(Args::wrong("title", "a bookmark title of 1 to 120 characters"));
        }
        let exercise = a.opt_str("exercise_label")?.map(str::trim).unwrap_or(DEFAULT_EXERCISE).to_owned();
        let dry_run = a.opt_bool("dry_run")?.unwrap_or(false);
        let index = match a.opt_str("index")? {
            None => None,
            Some("csv") => Some(false),
            Some("markdown") => Some(true),
            Some(_) => return Err(Args::wrong("index", "\"csv\" or \"markdown\"")),
        };
        // Resolved before reading, so a path outside the root costs nothing.
        let out = a.opt_str("out")?.map(|p| self.resolve(p, true).map(|r| (p.to_owned(), r))).transpose()?;
        if out.is_some() && index.is_none() {
            return Err(Args::wrong("out", "a file for the index: give index too"));
        }
        let id = self.doc(a)?.id;
        let (seed, _) = self.cipher_args(a)?;
        if let Some((_, path)) = &out {
            let inputs = self.input_paths(a)?;
            self.check_outputs(a, &[("out", path.as_path())], &inputs.iter().map(|p| p.as_path()).collect::<Vec<_>>())?;
        }
        let ocr = crate::chess::ocr_options(a)?.with_book(seed.as_ref());
        let loaded = self.use_store_corrections(a, id)?;
        let read = self.session.chess_document_with(id, &pages, &settings, lang, &ocr).map_err(failed)?;
        let entries = bookmarks::outline(&read.doc, include, &exercise);
        let labels: Vec<String> = self.doc(a)?.info.pages.iter().map(|p| p.label.clone()).collect();
        let labels = PageLabel::with_folios(&labels, &read.doc);
        let count = |k: EntryKind| entries.iter().filter(|e| e.kind == k).count();
        let tree: Vec<Value> = entries
            .iter()
            .map(|e| {
                json!({
                    "level": e.level,
                    "kind": e.kind.name(),
                    "title": e.title,
                    "page": u64::from(e.page).saturating_add(1),
                    "page_label": PageLabel::of(&labels, e.page).to_string(),
                })
            })
            .collect();
        let mut result = json!({
            "title": title,
            "dry_run": dry_run,
            "entries": entries.len(),
            "chapters": count(EntryKind::Chapter),
            "games": count(EntryKind::Game),
            "exercises": count(EntryKind::Exercise),
            "tree": tree,
            "pages": read.doc.pages.len(),
            "needs_ocr": crate::chess::needs_ocr_reply(&read.needs_ocr),
            "notes": crate::chess::notes_reply(&read.notes),
            "corrections": crate::chess_corrections::corrections_reply(loaded, &read),
        });
        if !dry_run {
            if entries.is_empty() {
                result["applied"] = json!(false);
                result["why"] = json!("the pages read have no chapter, game or exercise to bookmark");
            } else {
                let entries = entries
                    .iter()
                    .filter_map(|e| {
                        Some(pdfcraft_engine::OutlineEntry {
                            level: e.level,
                            title: e.title.clone(),
                            page: usize::try_from(e.page).ok()?,
                            element: None,
                        })
                    })
                    .collect();
                let applied = self.apply(a, Edit::AddBookmarkTree { title: title.clone(), entries })?;
                result["applied"] = json!(true);
                result["document"] = applied;
            }
        }
        if let Some(markdown) = index {
            let rows = bookmarks::game_index(&read.doc);
            let name = self.doc(a)?.name.trim_end_matches(".pdf").to_owned();
            let text = if markdown { bookmarks::index_markdown(&rows, &labels, &name) } else { bookmarks::index_csv(&rows, &labels) };
            result["index_games"] = json!(rows.len());
            match out {
                Some((shown, path)) => {
                    let bom = !markdown && a.opt_bool("bom")?.unwrap_or(false);
                    let bytes = if bom { [pgn::BOM, text.as_bytes()].concat() } else { text.into_bytes() };
                    crate::write_atomic(&path, &bytes)?;
                    result["out"] = json!(shown);
                    result["bytes"] = json!(bytes.len());
                }
                None => result["index"] = json!(text),
            }
        }
        Ok(result)
    }
}
