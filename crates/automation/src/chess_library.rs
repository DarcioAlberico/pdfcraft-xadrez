//! Chess: the Library of books (plan A9-N15a, decisions D22 and D31), without the interface (the
//! Library panel is plan A9-N15c's): `chess_library_build` walks the folders given and indexes
//! every book whose index is not current, `chess_library_status` says what the Library holds and
//! `chess_library_clear` takes it out. Desktop only: on the web the tools say so.
//!
//! **Where.** The Library is the app's data folder's `chess/library` by default, as the readings'
//! folder of `chess_cache_status` is `chess/reading`: with `--root` or the automation roots it must
//! be inside them, else the status says `outside_root` and building or clearing is refused until
//! `library` names a folder inside them. The folders walked are confined like every path read.
//!
//! **What a book is read with**, as the app's Text tab reads it: the models (`models_dir`, else
//! the folder the tools find), the corrections and the PGN kept for it in the app's stores, and
//! the app's own readings looked in and never changed (decision 1 of 2026-11-18); each of them
//! outside the roots is left out, and said.

#[cfg(target_arch = "wasm32")]
use serde_json::Value;

#[cfg(target_arch = "wasm32")]
use crate::{Args, Automation, Result, failed};

/// The Library: the `library` argument, the app's `chess/library`, or `.pdfcraft`'s.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) const LIBRARY: crate::chess_store_migrate::Kind =
    crate::chess_store_migrate::Kind { arg: "library", dir: "library", legacy: ".pdfcraft/chess-library" };

/// Most folders one build walks.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
const MAX_DIRS: usize = 64;

#[cfg(target_arch = "wasm32")]
impl Automation {
    pub(crate) fn chess_library_build(&mut self, _a: &Args) -> Result<Value> {
        Err(failed("the Library runs on the desktop only: the web has no folders to walk"))
    }

    pub(crate) fn chess_library_status(&mut self, _a: &Args) -> Result<Value> {
        Err(failed("the Library runs on the desktop only"))
    }

    pub(crate) fn chess_library_clear(&mut self, _a: &Args) -> Result<Value> {
        Err(failed("the Library runs on the desktop only"))
    }

    pub(crate) fn chess_library_find(&mut self, _a: &Args) -> Result<Value> {
        Err(failed("the Library runs on the desktop only"))
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod desktop {
    use std::path::{Path, PathBuf};

    use pdfcraft_engine::chess::library::find::{FindQuery, Hit, MAX_RESULTS, MAX_TEXT_CHARS};
    use pdfcraft_engine::chess::library::index::Place;
    use pdfcraft_engine::chess::library::{BookRun, BuildOptions, CatalogBook, Library, State, WalkNote, is_hash};
    use pdfcraft_engine::chess::{ChessScanJob, MAX_SCAN_WORKERS};
    use serde_json::{Value, json};

    use super::{LIBRARY, MAX_DIRS};
    use crate::chess_store_migrate::{CORRECTIONS, REFERENCE, StoreAt};
    use crate::{Args, Automation, Result, ToolError, failed};

    /// Hits listed when `max` is not given.
    const DEFAULT_MAX: usize = 100;

    /// A position found, for a reply: its page from 1 and as the book numbers it, the diagram
    /// (from 1), the move of the text (as numbered, with its address) or the move of the book's
    /// PGN, its SAN and its game (from 1).
    fn hit_json(h: &Hit) -> Value {
        let numbered = h.san.as_ref().map(|san| format!("{}{}{san}", h.number, if h.black { "..." } else { "." }));
        let mut out = json!({
            "page": h.page.map(|p| p.saturating_add(1)),
            "page_label": h.label,
            "san": h.san,
            "game": h.game.map(|g| g.saturating_add(1)),
            "side_assumed": h.side_assumed,
            "main_line": h.main_line,
        });
        match h.place {
            Place::Diagram { index } => out["diagram"] = json!(index.saturating_add(1)),
            Place::Move { block, inline } => {
                out["move"] = json!(numbered);
                out["at"] = json!({ "block": block, "inline": inline });
            }
            Place::Pgn { game, node } => {
                out["move"] = json!(numbered);
                out["pgn"] = json!({ "game": game.saturating_add(1), "node": node });
            }
        }
        out
    }

    impl Automation {
        /// The Library `library` names, or the default, to change (`write`) or only to read;
        /// `None`: the app's, outside the roots.
        fn library(&self, a: &Args, write: bool) -> Result<Option<(Library, String)>> {
            Ok(match self.chess_store_at(a, LIBRARY, write)? {
                StoreAt::Folder(dir) => {
                    let shown = self.shown_path(&dir).0;
                    Some((Library::new(dir), shown))
                }
                StoreAt::OutsideRoot => None,
            })
        }

        /// A store of the app's the build reads a book with (`kind`'s default, only read), and
        /// whether it is outside the roots.
        fn app_store(&self, kind: crate::chess_store_migrate::Kind) -> (Option<PathBuf>, bool) {
            match self.chess_store_at(&Args(&Value::Null), kind, false) {
                Ok(StoreAt::Folder(dir)) => (Some(dir), false),
                Ok(StoreAt::OutsideRoot) | Err(_) => (None, true),
            }
        }

        /// A path of the catalog's for a reply: relative to the root when confined.
        fn shown(&self, path: &str) -> String {
            self.shown_path(Path::new(path)).0
        }

        fn book_json(&self, b: &CatalogBook, run: Option<&BookRun>, index: Option<bool>) -> Value {
            let mut out = json!({
                "paths": b.paths.iter().map(|p| self.shown(p)).collect::<Vec<_>>(),
                "hash": b.hash,
                "title": b.title,
                "pages": b.pages,
                "size": b.size,
                "modified": b.modified,
                "state": b.state.name(),
                "reason": b.reason.map(|r| r.name()),
                "error": b.error.as_deref().map(|e| self.shown_text(e)),
                "counts": b.counts,
                "no_piece_model": b.no_piece_model,
                "index_bytes": b.index_bytes,
            });
            if let Some(r) = run {
                out["run"] = json!({
                    "current": r.current,
                    "scanned": r.scanned,
                    "from_library": r.from_library,
                    "from_app": r.from_app,
                    "kept": r.kept,
                    "set_aside": r.set_aside,
                    "index_set_aside": r.index_set_aside,
                    "seconds": (r.seconds * 10.0).round() / 10.0,
                });
            }
            if let Some(i) = index {
                out["index"] = json!(i);
            }
            out
        }

        fn walk_json(&self, w: &WalkNote) -> Value {
            json!({
                "entries": w.entries,
                "capped": w.capped.iter().map(|c| c.name()).collect::<Vec<_>>(),
                "links": w.links.iter().map(|p| self.shown(p)).collect::<Vec<_>>(),
                "cloud_folders": w.cloud_folders.iter().map(|p| self.shown(p)).collect::<Vec<_>>(),
                "unlisted": w.unlisted.iter().map(|(p, e)| json!({ "path": self.shown(p), "error": self.shown_text(e) })).collect::<Vec<_>>(),
            })
        }

        pub(crate) fn chess_library_build(&mut self, a: &Args) -> Result<Value> {
            // The paths first: one that leaves the root is refused before anything is read.
            let given = a.strs("dirs")?;
            if given.is_empty() || given.len() > MAX_DIRS {
                return Err(Args::wrong("dirs", &format!("from 1 to {MAX_DIRS} folders")));
            }
            let mut dirs = Vec::with_capacity(given.len());
            for d in &given {
                let dir = self.resolve_to_read(d)?;
                if !dir.is_dir() {
                    return Err(failed(format!("{d} is not a folder")));
                }
                dirs.push(dir);
            }
            let models_dir = self.models_dir(a)?;
            let Some((library, shown)) = self.library(a, true)? else { return Err(failed("no Library folder to build in")) };
            let max_books = match a.opt_int("max_books")? {
                None => None,
                Some(n) => Some(usize::try_from(n).ok().filter(|n| *n >= 1).ok_or_else(|| Args::wrong("max_books", "1 or more"))?),
            };
            let workers = match a.opt_int("workers")? {
                None => ChessScanJob::default_workers(),
                Some(n) => usize::try_from(n)
                    .ok()
                    .filter(|n| (1..=MAX_SCAN_WORKERS).contains(n))
                    .ok_or_else(|| Args::wrong("workers", &format!("from 1 to {MAX_SCAN_WORKERS}")))?,
            };
            let (corrections, corrections_out) = self.app_store(CORRECTIONS);
            let (reference, reference_out) = self.app_store(REFERENCE);
            let (app_reading, reading_out) = self.app_store(crate::chess_cache::READING);
            let opts = BuildOptions { models_dir, workers, max_books, corrections, reference, app_reading, ..BuildOptions::default() };
            let built = library.build(&dirs, &opts, &mut |_| true).map_err(failed)?;
            let runs = built.books.iter().filter_map(|b| b.run);
            let (scanned, from_library, from_app) = runs.fold((0usize, 0usize, 0usize), |(s, l, p), r| {
                (s.saturating_add(r.scanned), l.saturating_add(r.from_library), p.saturating_add(r.from_app))
            });
            Ok(json!({
                "library": shown,
                "dirs": dirs.iter().map(|d| self.shown_path(d).0).collect::<Vec<_>>(),
                "books": built.books.iter().map(|b| self.book_json(&b.book, b.run.as_ref(), None)).collect::<Vec<_>>(),
                "read": built.read,
                "left": built.left,
                "cloud": built.cloud,
                "pages": { "scanned": scanned, "from_library": from_library, "from_app": from_app },
                "walk": self.walk_json(&built.walk),
                "bytes": built.bytes,
                "orphans": built.orphans,
                "orphan_bytes": built.orphan_bytes,
                "listing_truncated": built.listing_truncated,
                "max_bytes": pdfcraft_engine::chess::library::MAX_BYTES,
                "workers": workers,
                "outside_root": { "corrections": corrections_out, "reference": reference_out, "app_reading": reading_out },
            }))
        }

        pub(crate) fn chess_library_status(&mut self, a: &Args) -> Result<Value> {
            let Some((library, shown)) = self.library(a, false)? else {
                return Ok(json!({
                    "library": Value::Null,
                    "outside_root": true,
                    "why": "the Library is kept in the app's data folder, outside the folders the tools are confined to: give library to read one",
                    "books": [], "bytes": 0,
                }));
            };
            let catalog = library.catalog();
            let files = library.files();
            let has = |b: &CatalogBook| b.hash.as_deref().map(|h| files.indexes.iter().any(|(i, _)| i.eq_ignore_ascii_case(h)));
            let count = |s: State| catalog.books.iter().filter(|b| b.state == s).count();
            Ok(json!({
                "library": shown,
                "dirs": catalog.dirs.iter().map(|d| self.shown(d)).collect::<Vec<_>>(),
                "books": catalog.books.iter().map(|b| self.book_json(b, None, has(b))).collect::<Vec<_>>(),
                "states": {
                    "indexed": count(State::Indexed), "partial": count(State::Partial), "pending": count(State::Pending),
                    "skipped": count(State::Skipped), "cloud": count(State::Cloud), "full": count(State::Full),
                },
                "walk": self.walk_json(&catalog.walk),
                "indexes": files.indexes.len(),
                "set_aside": files.set_aside.len(),
                "staging": files.staging.len(),
                "bytes": files.bytes(),
                "max_bytes": pdfcraft_engine::chess::library::MAX_BYTES,
                "listing_truncated": files.truncated,
                "updated": catalog.updated,
            }))
        }

        pub(crate) fn chess_library_find(&mut self, a: &Args) -> Result<Value> {
            let max = match a.opt_int("max")? {
                None => DEFAULT_MAX,
                Some(n) => usize::try_from(n)
                    .ok()
                    .filter(|n| (1..=MAX_RESULTS).contains(n))
                    .ok_or_else(|| Args::wrong("max", &format!("from 1 to {MAX_RESULTS}")))?,
            };
            let position = if a.get("fen").is_some_and(|v| !v.is_null()) || a.get("moves").is_some_and(|v| !v.is_null()) {
                Some(crate::chess_position::position_arg(a)?)
            } else {
                None
            };
            let text = |name: &str| -> Result<Option<String>> {
                match a.opt_str(name)? {
                    None => Ok(None),
                    Some(t) if t.trim().is_empty() => Err(Args::wrong(name, "some text")),
                    Some(t) if t.chars().count() > MAX_TEXT_CHARS => Err(Args::wrong(name, &format!("at most {MAX_TEXT_CHARS} characters"))),
                    Some(t) => Ok(Some(t.trim().to_owned())),
                }
            };
            let query = FindQuery {
                position: position.as_ref().map(|f| f.book_key()),
                any_side: a.opt_bool("any_side")?.unwrap_or(false),
                san: text("san")?,
                player: text("player")?,
                event: text("event")?,
                eco: text("eco")?,
                pattern: crate::chess_pattern::pattern_arg(a)?,
            };
            if query.is_empty() {
                return Err(ToolError::InvalidArgs("give fen or moves, san, player, event, eco, material or pawns: what to look for".into()));
            }
            let Some((library, shown)) = self.library(a, false)? else {
                return Ok(json!({
                    "library": Value::Null,
                    "outside_root": true,
                    "why": "the Library is kept in the app's data folder, outside the folders the tools are confined to: give library to look in one",
                    "books": [], "total": 0, "listed": 0, "indexes_read": 0,
                }));
            };
            let found = library.find(&query, max);
            let books: Vec<Value> = found
                .books
                .iter()
                .map(|b| {
                    let games: Vec<Value> = b
                        .games
                        .iter()
                        .map(|(i, g)| {
                            json!({
                                "game": i.saturating_add(1),
                                "pgn": g.pgn,
                                "white": g.white,
                                "black": g.black,
                                "event": g.event,
                                "date": g.date,
                                "eco": g.eco,
                                "page": g.page.map(|p| p.saturating_add(1)),
                                "moves": g.moves,
                            })
                        })
                        .collect();
                    json!({
                        "hash": b.hash,
                        "title": b.title,
                        "paths": b.paths.iter().map(|p| self.shown(p)).collect::<Vec<_>>(),
                        "total": b.total,
                        "found": b.hits.iter().map(hit_json).collect::<Vec<_>>(),
                        "games": games,
                        "games_total": b.games_total,
                    })
                })
                .collect();
            Ok(json!({
                "library": shown,
                "fen": position.map(|f| f.to_string()),
                "asked": if query.asks_positions() { "positions" } else { "games" },
                "books": books,
                "total": found.total,
                "listed": found.listed,
                "indexes_read": found.indexes_read,
                "unread": found.unread.iter().map(|(h, why)| json!({ "hash": h, "why": why })).collect::<Vec<_>>(),
                "not_indexed": found.not_indexed,
                "nothing": found.indexes_read == 0,
                "why": (found.indexes_read == 0).then_some("no book of the Library is indexed yet: build it (chess_library_build) to look in its books"),
            }))
        }

        pub(crate) fn chess_library_clear(&mut self, a: &Args) -> Result<Value> {
            let Some((library, shown)) = self.library(a, true)? else { return Err(failed("no Library folder to clear")) };
            let book = match a.opt_str("book")? {
                None => None,
                Some(b) if is_hash(b) => Some(b.to_ascii_lowercase()),
                Some(b) => {
                    // A path of a book in the catalog.
                    let path = self.resolve_to_read(b)?;
                    let wanted = path.to_string_lossy().into_owned();
                    let hash = library.catalog().books.into_iter().find(|c| c.paths.contains(&wanted)).and_then(|c| c.hash);
                    Some(hash.ok_or_else(|| Args::wrong("book", "a book's hash, or the path of a book the Library read"))?)
                }
            };
            let cleared = library.clear(book.as_deref());
            Ok(json!({
                "library": shown,
                "book": book,
                "indexes": cleared.indexes,
                "files": cleared.files,
                "bytes": cleared.bytes,
                "catalog_error": cleared.catalog_error.as_deref().map(|e| self.shown_text(e)),
                "listing_truncated": cleared.listing_truncated,
            }))
        }
    }
}
