//! Chess: the book's own PGN kept with its document, without the interface (plan "Carregar PGN do
//! livro", R5): `chess_reference_set`, `chess_reference_status` and `chess_reference_clear`, and
//! the PGN the reading tools take when they are not given `reference_pgn`.
//!
//! The PGNs live in a folder confined to `--root` (`reference_store`, [`DEFAULT_STORE`] by
//! default), one file per document, in the format of `pdfcraft_engine::chess::reference_store`; a
//! tool reads and changes the PGN of the document it is given, never another's.

use pdfcraft_engine::DocId;
use pdfcraft_engine::chess::DocKeys;
use pdfcraft_engine::chess::pdfcraft_chessvision::doc::reference::ReferenceIndex;
#[cfg(not(target_arch = "wasm32"))]
use pdfcraft_engine::chess::reference_store::DirStore;
use pdfcraft_engine::chess::reference_store::{KeptPgn, Loaded, ReferenceStore, ReferenceStoreError};
use serde_json::{Value, json};

use crate::{Args, Automation, Result, failed};

/// The folder of the books' PGNs when `reference_store` is not given (relative to `--root` when
/// set).
pub(crate) const DEFAULT_STORE: &str = ".pdfcraft/chess-reference";

/// Where the tools keep the books' PGNs: a folder. The web build has no folders: there the store
/// is empty and keeps nothing (a set is refused), so the reading tools still read.
#[cfg(not(target_arch = "wasm32"))]
type FolderStore = DirStore;
#[cfg(target_arch = "wasm32")]
type FolderStore = pdfcraft_engine::chess::reference_store::MemStore;

/// The book's PGN a reading tool uses.
pub(crate) struct BookPgn {
    /// The path as given, or the kept PGN's file name.
    pub shown: String,
    pub index: ReferenceIndex,
}

/// Seconds since 1970, now.
fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// What a store found, for a reply.
fn loaded_reply(loaded: &Loaded) -> Value {
    let pgn = loaded.pgn();
    json!({
        "file": loaded.name,
        "kept": pgn.is_some(),
        "name": pgn.and_then(|p| p.name.clone()),
        "bytes": pgn.map(|p| p.bytes),
        "hash": pgn.map(|p| p.hash.clone()),
        "at": pgn.map(|p| p.at),
        "games": pgn.map(|p| p.games),
        "moves": pgn.map(|p| p.moves),
        // The user's choices on moves in conflict with it, made in the app (R6e).
        "decisions": loaded.file.as_ref().map(|f| f.decisions.len()),
        "read_only": loaded.read_only.is_some(),
        "set_aside": loaded.set_aside.as_ref().map(|(name, why)| json!({ "file": name, "why": why })),
    })
}

impl Automation {
    /// The folder `reference_store` names, confined to the root.
    fn reference_store(&self, a: &Args) -> Result<FolderStore> {
        let store = a.opt_str("reference_store")?.unwrap_or(DEFAULT_STORE);
        let dir = self.resolve(store, true)?;
        Ok(Self::folder_store(dir))
    }

    fn folder_store(dir: std::path::PathBuf) -> FolderStore {
        #[cfg(not(target_arch = "wasm32"))]
        let store = DirStore::new(dir);
        #[cfg(target_arch = "wasm32")]
        let store = {
            let _ = dir;
            FolderStore::new(0)
        };
        store
    }

    /// The document's keys and pages.
    fn reference_keys(&self, id: DocId) -> Result<(DocKeys, usize)> {
        let doc = self.session.get(id).ok_or_else(|| failed("no such document"))?;
        Ok((doc.doc_keys(), doc.info.pages.len()))
    }

    /// The book's PGN a reading tool of document `id` uses: `reference_pgn` when given, else the
    /// one kept for the document in `reference_store`, unless `reference` is false. With it, what
    /// the store had for the reply's `reference_kept` (`null` when the store was not looked at).
    /// Indexed before the pages are read, so a PGN that cannot be used costs nothing.
    pub(crate) fn book_pgn(&self, a: &Args, id: DocId) -> Result<(Option<BookPgn>, Value)> {
        if let Some(p) = a.opt_str("reference_pgn")? {
            let index = crate::chess::read_reference(&self.resolve(p, false)?, p)?;
            return Ok((Some(BookPgn { shown: p.to_owned(), index }), Value::Null));
        }
        if !a.opt_bool("reference")?.unwrap_or(true) {
            return Ok((None, Value::Null));
        }
        let store = self.reference_store(a)?;
        let (keys, pages) = self.reference_keys(id)?;
        let loaded = match store.load(&keys, pages) {
            Ok(l) => l,
            // A document with no name, path or identifier has nothing kept under it.
            Err(ReferenceStoreError::NoKeys) => return Ok((None, Value::Null)),
            Err(e) => return Err(failed(format!("{e} (pass reference: false to read without the book's PGN)"))),
        };
        let reply = loaded_reply(&loaded);
        let Some(pgn) = loaded.pgn() else { return Ok((None, reply)) };
        let shown = pgn.name.clone().unwrap_or_else(|| "the kept PGN".into());
        let index = ReferenceIndex::from_pgn(&pgn.text).map_err(|e| failed(format!("{shown} (kept for the document): {e}")))?;
        Ok((Some(BookPgn { shown, index }), reply))
    }

    /// After document `id` was saved: its PGN in the default folder, found under the keys it had
    /// `before`, recorded under its keys now. Whether there was one.
    pub(crate) fn rekey_chess_reference(&self, id: DocId, before: &DocKeys) -> std::result::Result<bool, String> {
        let Ok(dir) = self.resolve(DEFAULT_STORE, true) else { return Ok(false) };
        let doc = self.session.get(id).ok_or("no such document")?;
        Self::folder_store(dir).rekey(before, &doc.doc_keys(), doc.info.pages.len()).map_err(|e| e.to_string())
    }

    pub(crate) fn chess_reference_set(&mut self, a: &Args) -> Result<Value> {
        let store = self.reference_store(a)?;
        let id = self.doc(a)?.id;
        let shown = a.str("path")?;
        let path = self.resolve(shown, false)?;
        let text = crate::chess::read_reference_text(&path, shown)?;
        // Indexed first: a PGN with no game in it is refused, and the counts are kept with it.
        let index = ReferenceIndex::from_pgn(&text).map_err(|e| failed(format!("{shown}: {e}")))?;
        if index.games().is_empty() {
            return Err(failed(format!("{shown}: no game in it")));
        }
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
        let stopped = index.games().iter().filter(|g| g.stopped.is_some() || g.error.is_some()).count();
        let pgn = KeptPgn::new(name.as_deref(), text, index.games().len(), index.moves(), now()).map_err(failed)?;
        let (keys, pages) = self.reference_keys(id)?;
        let (replaced, loaded) = store.set(&keys, pages, pgn).map_err(failed)?;
        let mut reply = loaded_reply(&loaded);
        reply["replaced"] = json!(replaced);
        reply["stopped_games"] = json!(stopped);
        Ok(reply)
    }

    pub(crate) fn chess_reference_status(&self, a: &Args) -> Result<Value> {
        let store = self.reference_store(a)?;
        let id = self.doc(a)?.id;
        let (keys, pages) = self.reference_keys(id)?;
        let loaded = store.load(&keys, pages).map_err(failed)?;
        Ok(loaded_reply(&loaded))
    }

    pub(crate) fn chess_reference_clear(&mut self, a: &Args) -> Result<Value> {
        let store = self.reference_store(a)?;
        let id = self.doc(a)?.id;
        let (keys, pages) = self.reference_keys(id)?;
        let cleared = store.clear(&keys, pages).map_err(failed)?;
        Ok(json!({ "cleared": cleared }))
    }
}
