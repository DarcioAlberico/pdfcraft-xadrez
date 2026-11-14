//! Chess ▸ the book's figurine cipher kept per document (plan A9-G3c, V4b; §2.7): the session
//! holds the one copy in memory ([`pdfcraft_engine::Session::chess_cipher`]); here is where the
//! app keeps it, the engine's `cipher` store ([`Cipher`]) in its data folder's `chess/cipher`,
//! beside the corrections and the books' PGNs, so a book opened again reads its scanned pages
//! with what its earlier readings proved.
//!
//! - **Loaded** into the session the first time a reading of the document asks for it
//!   ([`PdfCraftApp::chess_cipher`]), merged with what the session learned meanwhile.
//! - **Kept** after every reading that taught it something ([`PdfCraftApp::keep_chess_cipher`]),
//!   merged under the store's lock with what another writer kept ([`BookCipher::merge`]: by page).
//!   The interface never waits for another writer: the store's writer thread merges it in
//!   ([`crate::chess_store_ui::UiStore::change`]).
//! - **What may be kept** is the one rule of [`crate::chess_keep`]: an encrypted book's only after
//!   the user's yes, which also keeps what its readings taught before; a book that does not allow
//!   copying, never. The session keeps it in memory meanwhile.
//! - **The web** keeps it in memory for the session only (not in the browser's storage).

use pdfcraft_engine::DocId;
use pdfcraft_engine::chess::cipher_store::Cipher;
use pdfcraft_engine::chess::pdfcraft_chessvision::cipher::BookCipher;

use crate::PdfCraftApp;
use crate::chess_store_ui::{UiStore, in_store};

/// The bytes the ciphers take in memory, when they are kept nowhere else: a book's table is at
/// most a megabyte.
const BUDGET: usize = 4 << 20;

/// The books' ciphers in the app.
pub struct Ciphers {
    pub(crate) store: UiStore<Cipher>,
    /// The documents whose kept cipher was loaded into the session.
    loaded: Vec<DocId>,
}

impl Default for Ciphers {
    fn default() -> Self {
        Ciphers { store: UiStore::memory(BUDGET), loaded: Vec::new() }
    }
}

impl Ciphers {
    /// Keep the books' ciphers in `dir` (the desktop; tests pass their own folder).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn use_folder(&mut self, dir: std::path::PathBuf) {
        self.store = UiStore::folder(dir, BUDGET);
        self.loaded.clear();
    }
}

impl PdfCraftApp {
    /// The book cipher document `id`'s reads have taught, if any: the one the session holds, with
    /// what the store kept for the document taken in the first time it is asked for (plan
    /// A9-G3c). Reading another book never takes it (plan A9-I1a).
    pub fn chess_cipher(&mut self, id: DocId) -> Option<BookCipher> {
        let open: Vec<DocId> = self.views.iter().map(|v| v.id).collect();
        self.chess.ciphers.loaded.retain(|d| open.contains(d));
        if !self.chess.ciphers.loaded.contains(&id)
            && let Some(doc) = self.session.get(id)
        {
            let (keys, pages) = (doc.doc_keys(), doc.info.pages.len());
            self.chess.ciphers.loaded.push(id);
            match in_store!(self.chess.ciphers.store.reading(&keys), s => s.load(&keys, pages)) {
                Ok(loaded) if !loaded.file.cipher.is_empty() => {
                    let mut cipher = self.session.chess_cipher(id).unwrap_or_default();
                    cipher.merge(&loaded.file.cipher);
                    self.session.set_chess_cipher(id, cipher);
                }
                Ok(_) => {}
                // A document without keys keeps none; a folder that can't be read leaves the
                // session's alone.
                Err(e) => log::warn!("the book's cipher could not be read: {e}"),
            }
        }
        self.session.chess_cipher(id)
    }

    /// `cipher`, what a reading of document `id` taught, is the document's now: in the session,
    /// and kept in the store when the document's chess may be written ([`crate::chess_keep`]).
    pub(crate) fn keep_chess_cipher(&mut self, id: DocId, cipher: BookCipher) {
        self.session.set_chess_cipher(id, cipher.clone());
        if cipher.is_empty() || !self.chess_may_write(id) {
            return;
        }
        let Some(doc) = self.session.get(id) else { return };
        let (keys, pages) = (doc.doc_keys(), doc.info.pages.len());
        let kept = self
            .chess
            .ciphers
            .store
            .change(true, &keys, pages, |place| in_store!(place, s => s.keep(&keys, pages, &cipher)).map_err(|e| e.to_string()));
        match kept {
            // What another writer kept is taken in too.
            Ok(merged) => self.session.set_chess_cipher(id, merged),
            Err(e) => log::warn!("the book's cipher could not be kept: {e}"),
        }
    }
}
