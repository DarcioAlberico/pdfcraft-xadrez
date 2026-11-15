//! Chess: the books' readings kept on disk (plan A9-N5b, decision D21): `chess_cache_status`
//! says what is kept, `chess_cache_clear` takes it out.
//!
//! The reading tools take and keep pages there when given `cache` (a folder, confined to the
//! root: [`Automation::chess_settings`]); without it they read every page, as before. The app
//! keeps its own in its data folder's `chess/reading`, which these two tools name by default, as
//! the stores' tools do (`chess_store_migrate`): with `--root` or the automation roots it must be
//! inside them, else the status says `outside_root` and clearing is refused until `cache` names a
//! folder inside them.

use pdfcraft_engine::chess::reading::{BookStatus, ReadingStore};
use serde_json::{Value, json};

use crate::chess_store_migrate::{Kind, StoreAt};
use crate::{Args, Automation, Result, failed};

/// The books' readings: the `cache` argument, the app's `chess/reading`, or `.pdfcraft`'s.
pub(crate) const READING: Kind = Kind { arg: "cache", dir: pdfcraft_engine::chess::reading::KIND, legacy: ".pdfcraft/chess-reading" };

/// Seconds since 1970 of a time (0 before it, or when the system can't say).
fn secs(t: std::time::SystemTime) -> u64 {
    t.duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// One book's folder for a reply: no path, its document as its record says (the file name, the
/// page count, and the identifier when it has one).
fn book_json(b: &BookStatus) -> Value {
    json!({
        "folder": b.folder,
        "name": b.doc.as_ref().and_then(|d| d.name.clone()),
        "id0": b.doc.as_ref().and_then(|d| d.id0.clone()),
        "pages": b.doc.as_ref().map(|d| d.pages),
        "entries": b.entries,
        "set_aside": b.set_aside,
        "bytes": b.bytes,
        "used": secs(b.used),
    })
}

impl Automation {
    /// The readings' folder `cache` names, or the default, to change (`write`) or only to read.
    pub(crate) fn reading_store(&self, a: &Args, write: bool) -> Result<Option<(ReadingStore, String)>> {
        match self.chess_store_at(a, READING, write)? {
            StoreAt::Folder(dir) => {
                let shown = self.shown_path(&dir).0;
                Ok(Some((ReadingStore::new(dir), shown)))
            }
            StoreAt::OutsideRoot => Ok(None),
        }
    }

    /// `chess_cache_status`: the books whose readings are kept, used most recently first, or only
    /// `doc`'s.
    pub(crate) fn chess_cache_status(&mut self, a: &Args) -> Result<Value> {
        let only = if a.get("doc").is_some_and(|v| !v.is_null()) { Some(self.doc(a)?.doc_keys()) } else { None };
        let Some((store, shown)) = self.reading_store(a, false)? else {
            return Ok(json!({
                "cache": Value::Null,
                "outside_root": true,
                "why": "the readings are kept in the app's data folder, outside the folders the tools are confined to: give cache to read some",
                "books": [], "entries": 0, "bytes": 0,
            }));
        };
        let wanted = only.as_ref().and_then(pdfcraft_engine::chess::reading::book_folder);
        let books: Vec<BookStatus> = store.status().into_iter().filter(|b| only.is_none() || wanted.as_deref() == Some(b.folder.as_str())).collect();
        Ok(json!({
            "cache": shown,
            "books": books.iter().map(book_json).collect::<Vec<_>>(),
            "entries": books.iter().fold(0usize, |n, b| n.saturating_add(b.entries)),
            "set_aside": books.iter().fold(0usize, |n, b| n.saturating_add(b.set_aside)),
            "bytes": books.iter().fold(0u64, |n, b| n.saturating_add(b.bytes)),
            "max_bytes": store.max_bytes(),
        }))
    }

    /// `chess_cache_clear`: take out the books' readings kept (only `doc`'s, when given): the
    /// store's own files, never another.
    pub(crate) fn chess_cache_clear(&mut self, a: &Args) -> Result<Value> {
        let only = if a.get("doc").is_some_and(|v| !v.is_null()) { Some(self.doc(a)?.doc_keys()) } else { None };
        let Some((store, shown)) = self.reading_store(a, true)? else { return Err(failed("no readings' folder to clear")) };
        let cleared = store.clear(only.as_ref());
        Ok(json!({ "cache": shown, "books": cleared.books, "files": cleared.files, "bytes": cleared.bytes }))
    }
}
