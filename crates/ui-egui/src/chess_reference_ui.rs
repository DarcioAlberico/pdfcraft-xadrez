//! Chess ▸ the book's own PGN, in the app (plan "Carregar PGN do livro", R6a).
//!
//! **Where.** As the diagram corrections: the desktop keeps a copy of each document's PGN in the
//! `chess/reference` folder of the recovery folder (`DirStore`); the web build keeps them in
//! eframe's `Storage` under [`STORAGE_KEY`], within [`WEB_BUDGET`] bytes of their own
//! (`MemStore`). Until either is set (tests, a session without recovery) they live in memory for
//! the session. The format and the rules are the engine's
//! (`pdfcraft_engine::chess::reference_store`): one file per document, found by its keys.
//!
//! **What uses it.** Chess ▸ Load the book's PGN… (`chess.reference.load`) keeps the PGN chosen
//! for the active document, and Remove the book's PGN (`chess.reference.clear`) removes it. Each
//! page the Text tab reads is linked to it (`reference::link`, off the interface thread): a move
//! the reader could not play and the PGN placed is shown as read from the PGN, a move the PGN has
//! elsewhere as in conflict. Saving records it under the saved file's keys
//! ([`PdfCraftApp::rekey_book_pgn`]).

use std::sync::Arc;

use pdfcraft_engine::DocId;
use pdfcraft_engine::chess::DocKeys;
use pdfcraft_engine::chess::pdfcraft_chessvision::doc::reference::{MAX_REFERENCE_BYTES, ReferenceIndex};
use pdfcraft_engine::chess::reference_store::{Choice, Decision, KeptPgn, Loaded, MemStore, ReferenceFile, ReferenceStore, ReferenceStoreError};
use pdfcraft_engine::chess::span::MoveAt;

use crate::PdfCraftApp;

/// The web build's `Storage` key for the books' PGNs.
pub const STORAGE_KEY: &str = "pdfcraft-chess-reference-v1";

/// The bytes the web build keeps the books' PGNs in, apart from the games and the corrections.
pub const WEB_BUDGET: usize = 2 << 20;

/// Most documents remembered as having declined the PGN beside them; the oldest go first.
pub const MAX_DECLINED: usize = 256;

/// Where the books' PGNs are kept.
enum Store {
    #[cfg(not(target_arch = "wasm32"))]
    Folder(pdfcraft_engine::chess::reference_store::DirStore),
    /// The web build's `Storage` value (`changed`: since eframe last took it), or the session's
    /// memory until a store is set.
    Memory { store: MemStore, changed: bool },
}

impl Default for Store {
    fn default() -> Self {
        Store::Memory { store: MemStore::new(WEB_BUDGET), changed: false }
    }
}

impl Store {
    fn load(&self, keys: &DocKeys, pages: usize) -> Result<Loaded, ReferenceStoreError> {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Store::Folder(s) => s.load(keys, pages),
            Store::Memory { store, .. } => store.load(keys, pages),
        }
    }

    fn set(&mut self, keys: &DocKeys, pages: usize, pgn: KeptPgn) -> Result<(bool, Loaded), ReferenceStoreError> {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Store::Folder(s) => s.set(keys, pages, pgn),
            Store::Memory { store, changed } => {
                let out = store.set(keys, pages, pgn)?;
                *changed = true;
                Ok(out)
            }
        }
    }

    fn clear(&mut self, keys: &DocKeys, pages: usize) -> Result<bool, ReferenceStoreError> {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Store::Folder(s) => s.clear(keys, pages),
            Store::Memory { store, changed } => {
                let cleared = store.clear(keys, pages)?;
                *changed |= cleared;
                Ok(cleared)
            }
        }
    }

    fn decide(&mut self, keys: &DocKeys, pages: usize, decision: Decision) -> Result<ReferenceFile, ReferenceStoreError> {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Store::Folder(s) => s.decide(keys, pages, decision),
            Store::Memory { store, changed } => {
                let out = store.decide(keys, pages, decision)?;
                *changed = true;
                Ok(out)
            }
        }
    }

    fn rekey(&mut self, old: &DocKeys, new: &DocKeys, pages: usize) -> Result<bool, ReferenceStoreError> {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Store::Folder(s) => s.rekey(old, new, pages),
            Store::Memory { store, changed } => {
                let moved = store.rekey(old, new, pages)?;
                *changed |= moved;
                Ok(moved)
            }
        }
    }
}

/// What the Text tab says about a document's PGN.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BookPgnSummary {
    /// The PGN's file name, when it was kept.
    pub name: Option<String>,
    pub games: usize,
    pub moves: usize,
}

/// The whole book linked to its PGN (plan "Carregar PGN do livro", R6d): what Link the whole
/// book found, for the session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BookLink {
    pub counts: pdfcraft_engine::chess::pdfcraft_chessvision::doc::reference::LinkCounts,
    /// The book's pages, and how many of them discuss a game of the PGN.
    pub pages: usize,
    pub pages_linked: usize,
    /// Each game of the PGN, in its order.
    pub games: Vec<LinkedGame>,
}

/// A game of the PGN and the first page of the book that discusses it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkedGame {
    pub game: usize,
    pub name: String,
    /// 0-based; `None` when no page discusses it.
    pub first_page: Option<usize>,
}

impl BookLink {
    /// What the alignment `a` of a book of `pages` pages with `index` found.
    pub(crate) fn new(a: &pdfcraft_engine::chess::pdfcraft_chessvision::doc::reference::Alignment, index: &ReferenceIndex, pages: usize) -> BookLink {
        let mut first: Vec<Option<usize>> = vec![None; index.games().len()];
        for p in &a.pages {
            if let (Some(slot), Ok(page)) = (first.get_mut(p.game), usize::try_from(p.page)) {
                *slot = Some(slot.map_or(page, |q| q.min(page)));
            }
        }
        let games = index
            .games()
            .iter()
            .enumerate()
            .map(|(game, g)| LinkedGame { game, name: g.name.clone(), first_page: first.get(game).copied().flatten() })
            .collect();
        BookLink { counts: a.counts, pages, pages_linked: a.pages.len(), games }
    }

    /// The moves of the text linked to the PGN.
    pub fn linked(&self) -> usize {
        self.counts.confirmed.saturating_add(self.counts.recovered).saturating_add(self.counts.conflict)
    }

    /// The moves of the text.
    pub fn moves(&self) -> usize {
        self.linked().saturating_add(self.counts.absent)
    }
}

/// What the Text tab shows of a document's book's PGN.
#[derive(Clone, Debug, Default)]
pub struct BookPgnView {
    pub summary: Option<BookPgnSummary>,
    /// The file name of the PGN beside the PDF, offered while none is kept.
    pub sibling: Option<String>,
    pub link: Option<BookLink>,
    /// Link the whole book is running: pages read, of how many.
    pub progress: Option<(usize, usize)>,
    /// The user's decisions on moves in conflict with the PGN (R6e).
    pub decisions: Vec<Decision>,
}

impl BookPgnView {
    /// The user's choice for the move at `at` printed `san`, when they made one.
    pub fn choice(&self, at: MoveAt, san: &str) -> Option<Choice> {
        self.decisions.iter().find(|d| d.is_for(at.page, at.block, at.inline, san)).map(|d| d.choice)
    }
}

/// One open document's PGN, as loaded.
struct DocPgn {
    doc: DocId,
    /// The keys it was loaded under: a save that changes them moves it along.
    keys: DocKeys,
    /// The PGN indexed, when the document keeps one that could be read.
    index: Option<Arc<ReferenceIndex>>,
    summary: Option<BookPgnSummary>,
    /// A PGN beside the PDF with its name (`book.pdf` → `book.pgn`), offered when none is kept
    /// (R6c; desktop only).
    sibling: Option<std::path::PathBuf>,
    /// The whole book linked to it, when Link the whole book ran (R6d).
    link: Option<BookLink>,
    /// The user's decisions on moves in conflict with it (R6e).
    decisions: Vec<Decision>,
}

/// The books' PGNs in the app.
#[derive(Default)]
pub struct BookPgns {
    store: Store,
    docs: Vec<DocPgn>,
    /// Link the whole book's pages read so far, and of how many (R6d).
    pub(crate) progress: Option<(Arc<std::sync::atomic::AtomicUsize>, usize)>,
    /// The documents whose PGN beside them the user said Not now to, by the name their keys
    /// give their files (`corrections::file_name`): not offered again. Kept in the settings, at
    /// most [`MAX_DECLINED`] (R6f).
    pub declined: Vec<String>,
    /// The documents told this session that a PGN sits beside them (R6f).
    told: Vec<DocId>,
    /// PGN files the browser read for a document, picked up on the next frame.
    #[cfg(target_arch = "wasm32")]
    pub(crate) uploads: Arc<std::sync::Mutex<Vec<(DocId, String, Vec<u8>)>>>,
}

impl BookPgns {
    /// Keep the books' PGNs in `dir` (the desktop; tests pass their own folder).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn use_folder(&mut self, dir: std::path::PathBuf) {
        self.store = Store::Folder(pdfcraft_engine::chess::reference_store::DirStore::new(dir));
        self.docs.clear();
    }

    /// Keep the books' PGNs in the web's `Storage` value `text` (the files by name, as
    /// [`Self::storage_value`] wrote them). A value that can't be read starts empty.
    pub fn use_storage(&mut self, text: Option<&str>) {
        let files: Vec<(String, Vec<u8>)> = text
            .and_then(|t| serde_json::from_str::<Vec<(String, String)>>(t).ok())
            .unwrap_or_default()
            .into_iter()
            .map(|(name, body)| (name, body.into_bytes()))
            .collect();
        self.store = Store::Memory { store: MemStore::with_files(files, WEB_BUDGET), changed: false };
        self.docs.clear();
    }

    /// The web's `Storage` value, when the PGNs changed since it was last taken.
    pub fn storage_value(&mut self) -> Option<String> {
        let (store, changed) = match &mut self.store {
            Store::Memory { store, changed } => (store, changed),
            #[cfg(not(target_arch = "wasm32"))]
            Store::Folder(_) => return None,
        };
        if !std::mem::take(changed) {
            return None;
        }
        let files: Vec<(String, String)> =
            store.files().into_iter().filter_map(|(name, bytes)| String::from_utf8(bytes).ok().map(|b| (name, b))).collect();
        serde_json::to_string(&files).ok()
    }

    fn doc(&self, id: DocId) -> Option<&DocPgn> {
        self.docs.iter().find(|d| d.doc == id)
    }

    /// What document `id` keeps, as last loaded (the Text tab loads it when it reads a page).
    pub(crate) fn summary(&self, id: DocId) -> Option<&BookPgnSummary> {
        self.doc(id)?.summary.as_ref()
    }

    /// What the Text tab shows of document `id`'s book's PGN.
    pub(crate) fn view(&self, id: DocId) -> BookPgnView {
        let progress = self.progress.as_ref().map(|(done, of)| (done.load(std::sync::atomic::Ordering::Relaxed), *of));
        let decisions = self.doc(id).map(|d| d.decisions.clone()).unwrap_or_default();
        BookPgnView {
            summary: self.summary(id).cloned(),
            sibling: self.sibling_name(id),
            link: self.doc(id).and_then(|d| d.link.clone()),
            progress,
            decisions,
        }
    }

    /// Keep what Link the whole book found for document `id`.
    pub(crate) fn keep_link(&mut self, id: DocId, link: BookLink) {
        if let Some(d) = self.docs.iter_mut().find(|d| d.doc == id) {
            d.link = Some(link);
        }
    }

    /// The PGN beside document `id`'s PDF, by its file name, when the panel offers it (R6f): the
    /// document keeps none and the user did not say Not now to it.
    pub(crate) fn offer(&self, id: DocId) -> Option<String> {
        let d = self.doc(id)?;
        let key = pdfcraft_engine::chess::corrections::file_name(&d.keys)?;
        if self.declined.contains(&key) {
            return None;
        }
        self.sibling_name(id)
    }

    /// The file name of the PGN beside document `id`'s PDF, when it keeps none.
    pub(crate) fn sibling_name(&self, id: DocId) -> Option<String> {
        let d = self.doc(id).filter(|d| d.summary.is_none())?;
        d.sibling.as_ref()?.file_name().map(|n| n.to_string_lossy().into_owned())
    }
}

/// Seconds since 1970, now.
fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl PdfCraftApp {
    /// Document `id`'s PGN, loaded from the store the first time and indexed. A PGN that can't
    /// be read is said once, and the document reads without it.
    fn book_pgn_for(&mut self, id: DocId) -> Option<&DocPgn> {
        let open: Vec<DocId> = self.session.docs().iter().map(|d| d.id).collect();
        self.chess.book_pgns.docs.retain(|d| open.contains(&d.doc));
        if self.chess.book_pgns.doc(id).is_none() {
            let doc = self.session.get(id)?;
            let (keys, pages) = (doc.doc_keys(), doc.info.pages.len());
            let (mut index, mut summary, mut decisions) = (None, None, Vec::new());
            match self.chess.book_pgns.store.load(&keys, pages) {
                Ok(loaded) => {
                    if let Some((name, why)) = &loaded.set_aside {
                        self.notify(crate::i18n::fmt(
                            tl!("The book's PGN kept for this document couldn't be read ({why}); it was kept aside as {name}."),
                            &[("why", why), ("name", name)],
                        ));
                    }
                    decisions = loaded.file.as_ref().map(|f| f.decisions.clone()).unwrap_or_default();
                    if let Some(pgn) = loaded.pgn() {
                        match ReferenceIndex::from_pgn(&pgn.text) {
                            Ok(i) => {
                                summary = Some(BookPgnSummary { name: pgn.name.clone(), games: i.games().len(), moves: i.moves() });
                                index = Some(Arc::new(i));
                            }
                            Err(e) => self.notify(crate::i18n::fmt(tl!("The book's PGN couldn't be read: {e}"), &[("e", &e.to_string())])),
                        }
                    }
                }
                // No keys: nothing is kept for it.
                Err(ReferenceStoreError::NoKeys) => {}
                Err(e) => self.notify(crate::i18n::fmt(tl!("The book's PGN couldn't be read: {e}"), &[("e", &e.to_string())])),
            }
            let sibling = sibling_pgn(&keys);
            self.chess.book_pgns.docs.push(DocPgn { doc: id, keys, index, summary, sibling, link: None, decisions });
        }
        self.chess.book_pgns.doc(id)
    }

    /// Document `id`'s PGN, indexed, when it keeps one.
    pub(crate) fn book_pgn_index(&mut self, id: DocId) -> Option<Arc<ReferenceIndex>> {
        self.book_pgn_for(id)?.index.clone()
    }

    /// What the Text tab says about document `id`'s PGN, when it keeps one.
    pub fn book_pgn_summary(&mut self, id: DocId) -> Option<BookPgnSummary> {
        self.book_pgn_for(id)?.summary.clone()
    }

    /// Chess ▸ Load the book's PGN…: choose a PGN file and keep it for the active document.
    pub fn load_book_pgn(&mut self) {
        let Some((_, id)) = self.active_ids() else { return };
        #[cfg(not(target_arch = "wasm32"))]
        {
            let dialog = rfd::AsyncFileDialog::new().add_filter("PGN", &["pgn", "txt"]);
            self.ask(crate::pickers::Ask::File(dialog), None, move |app, paths| {
                let Some(path) = paths.into_iter().next() else { return };
                let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
                match read_pgn(&path) {
                    Ok(bytes) => app.set_book_pgn(id, &name, &bytes),
                    Err(e) => app.notify_fmt("Couldn't read {name}: {e}", &[("name", &name), ("e", &e)]),
                }
            });
        }
        #[cfg(target_arch = "wasm32")]
        {
            let uploads = self.chess.book_pgns.uploads.clone();
            let ctx = self.ctx.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let Some(h) = rfd::AsyncFileDialog::new().add_filter("PGN", &["pgn", "txt"]).pick_file().await else { return };
                let bytes = h.read().await;
                match uploads.lock() {
                    Ok(mut q) => q.push((id, h.file_name(), bytes)),
                    Err(poisoned) => poisoned.into_inner().push((id, h.file_name(), bytes)),
                }
                if let Some(ctx) = ctx {
                    ctx.request_repaint();
                }
            });
        }
    }

    /// The PGN files the browser has read since the last frame.
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn take_book_pgn_uploads(&mut self) {
        let files = match self.chess.book_pgns.uploads.lock() {
            Ok(mut q) => std::mem::take(&mut *q),
            Err(poisoned) => std::mem::take(&mut *poisoned.into_inner()),
        };
        for (id, name, bytes) in files {
            self.set_book_pgn(id, &name, &bytes);
        }
    }

    /// Keep the PGN `bytes`, read from a file named `name`, for document `id`, in place of the
    /// one it kept, and say what it holds. A PGN with no game in it is refused.
    pub fn set_book_pgn(&mut self, id: DocId, name: &str, bytes: &[u8]) {
        let Some(doc) = self.session.get(id) else { return };
        let (keys, pages) = (doc.doc_keys(), doc.info.pages.len());
        let refused =
            |e: &dyn std::fmt::Display| crate::i18n::fmt(tl!("{name} was not kept as the book's PGN: {e}"), &[("name", name), ("e", &e.to_string())]);
        if bytes.len() > MAX_REFERENCE_BYTES {
            return self.notify(refused(&tl!("the file is too large")));
        }
        let text = String::from_utf8_lossy(bytes).into_owned();
        let index = match ReferenceIndex::from_pgn(&text) {
            Ok(i) if !i.games().is_empty() => i,
            Ok(_) => return self.notify(refused(&tl!("no game in it"))),
            Err(e) => return self.notify(refused(&e)),
        };
        let summary = BookPgnSummary { name: Some(name.to_owned()), games: index.games().len(), moves: index.moves() };
        let kept =
            KeptPgn::new(Some(name), text, summary.games, summary.moves, now()).and_then(|pgn| self.chess.book_pgns.store.set(&keys, pages, pgn));
        if let Err(e) = kept {
            return self.notify(refused(&e));
        }
        self.chess.book_pgns.docs.retain(|d| d.doc != id);
        let sibling = sibling_pgn(&keys);
        let decisions = Vec::new();
        self.chess.book_pgns.docs.push(DocPgn {
            doc: id,
            keys,
            index: Some(Arc::new(index)),
            summary: Some(summary.clone()),
            sibling,
            link: None,
            decisions,
        });
        // The Text tab reads its page again, linked to it, and the Diagrams list takes it.
        self.chess.text = None;
        self.refresh_chess_kinds();
        self.notify(crate::i18n::fmt(
            tl!("Book's PGN {name}: {games} games, {moves} moves. The pages the Text tab reads are linked to it."),
            &[("name", name), ("games", &summary.games.to_string()), ("moves", &summary.moves.to_string())],
        ));
    }

    /// The answer to the offer of the PGN beside the active document's PDF (R6f): use it, or not
    /// now, which is remembered so it is not offered again.
    pub(crate) fn answer_pgn_offer(&mut self, use_it: bool) {
        if use_it {
            return self.use_sibling_pgn();
        }
        let Some((_, id)) = self.active_ids() else { return };
        let Some(key) = self.chess.book_pgns.doc(id).and_then(|d| pdfcraft_engine::chess::corrections::file_name(&d.keys)) else { return };
        let declined = &mut self.chess.book_pgns.declined;
        declined.retain(|k| *k != key);
        declined.push(key);
        let over = declined.len().saturating_sub(MAX_DECLINED);
        declined.drain(..over);
    }

    /// Keep the PGN beside the active document's PDF as its book's PGN (R6c).
    pub(crate) fn use_sibling_pgn(&mut self) {
        let Some((_, id)) = self.active_ids() else { return };
        let Some(path) = self.chess.book_pgns.doc(id).and_then(|d| d.sibling.clone()) else { return };
        let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
        #[cfg(not(target_arch = "wasm32"))]
        match read_pgn(&path) {
            Ok(bytes) => self.set_book_pgn(id, &name, &bytes),
            Err(e) => self.notify_fmt("Couldn't read {name}: {e}", &[("name", &name), ("e", &e)]),
        }
        #[cfg(target_arch = "wasm32")]
        let _ = name;
    }

    /// Keep the user's choice for the move at `at` printed `san`, in conflict with the active
    /// document's book's PGN (R6e): the book's move stands, or the PGN's is the one to follow.
    pub(crate) fn decide_book_move(&mut self, at: MoveAt, san: String, choice: Choice) {
        let Some((_, id)) = self.active_ids() else { return };
        let Some(doc) = self.session.get(id) else { return };
        let (keys, pages) = (doc.doc_keys(), doc.info.pages.len());
        let decision = Decision { page: at.page, block: at.block, inline: at.inline, san, choice };
        match self.chess.book_pgns.store.decide(&keys, pages, decision) {
            Ok(file) => {
                if let Some(d) = self.chess.book_pgns.docs.iter_mut().find(|d| d.doc == id) {
                    d.decisions = file.decisions;
                }
            }
            Err(e) => self.notify(crate::i18n::fmt(tl!("The choice was not kept: {e}"), &[("e", &e.to_string())])),
        }
    }

    /// Chess ▸ Remove the book's PGN: remove the PGN kept for the active document.
    pub fn clear_book_pgn(&mut self) {
        let Some((_, id)) = self.active_ids() else { return };
        let Some(doc) = self.session.get(id) else { return };
        let (keys, pages) = (doc.doc_keys(), doc.info.pages.len());
        match self.chess.book_pgns.store.clear(&keys, pages) {
            Ok(cleared) => {
                self.chess.book_pgns.docs.retain(|d| d.doc != id);
                let sibling = sibling_pgn(&keys);
                self.chess.book_pgns.docs.push(DocPgn { doc: id, keys, index: None, summary: None, sibling, link: None, decisions: Vec::new() });
                self.chess.text = None;
                self.refresh_chess_kinds();
                let said = if cleared { tl!("The book's PGN was removed from this document.") } else { tl!("This document keeps no book's PGN.") };
                self.notify(said.to_string());
            }
            Err(e) => self.notify(crate::i18n::fmt(tl!("The book's PGN couldn't be removed: {e}"), &[("e", &e.to_string())])),
        }
    }

    /// Document `id` was saved: its PGN, found under the keys it was loaded under, is recorded
    /// under the keys it has now, so it is found when the saved file is opened.
    pub(crate) fn rekey_book_pgn(&mut self, id: DocId) {
        let Some(doc) = self.session.get(id) else { return };
        let (keys, pages) = (doc.doc_keys(), doc.info.pages.len());
        let Some(old) = self.chess.book_pgns.doc(id).map(|d| d.keys.clone()).filter(|k| *k != keys) else { return };
        match self.chess.book_pgns.store.rekey(&old, &keys, pages) {
            Ok(_) => {
                if let Some(d) = self.chess.book_pgns.docs.iter_mut().find(|d| d.doc == id) {
                    d.keys = keys;
                }
            }
            Err(e) => self.notify(crate::i18n::fmt(tl!("The book's PGN was not kept for the saved file: {e}"), &[("e", &e.to_string())])),
        }
    }
}

/// The book's PGN's suggestion for a diagram: a position 1 or 2 squares from its reading, linked
/// on its page or the pages beside it (`reference::link`, R3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Suggestion {
    pub fen: String,
    /// The squares that differ: "e4: N → ·" (as read → as the PGN has it).
    pub changes: Vec<String>,
}

/// The suggestion for diagram `index` of `page` of document `id` in state `stamp`, when the Text
/// tab's reading of that page (`text`) has one.
pub(crate) fn suggestion(
    text: Option<&crate::chess_text_ui::PageText>,
    id: DocId,
    stamp: Option<pdfcraft_engine::chess::ScanStamp>,
    page: usize,
    index: usize,
) -> Option<Suggestion> {
    use pdfcraft_engine::chess::pdfcraft_chessvision::doc::Block;
    use pdfcraft_engine::chess::pdfcraft_chessvision::doc::reference::DiagramKind;
    let text = text.filter(|t| t.doc == id && t.page == page && Some(t.stamp) == stamp)?;
    let d = text
        .chess
        .pages
        .iter()
        .flat_map(|p| &p.blocks)
        .find_map(|b| if let Block::Diagram(d) = b { Some(d).filter(|d| d.number == index) } else { None })?;
    let r = d.reference.as_ref().filter(|r| r.kind == DiagramKind::Suggested)?;
    let piece = |p: Option<char>| p.map_or_else(|| "·".to_owned(), |c| c.to_string());
    let changes = r.changes.iter().map(|c| format!("{}: {} → {}", c.square, piece(c.from), piece(c.to))).collect();
    Some(Suggestion { fen: r.fen.clone(), changes })
}

impl PdfCraftApp {
    /// Every frame: when the diagram open in the Diagram tab is on the page shown and the
    /// document keeps a book's PGN, the page is read for the Text tab, linked to it, so the
    /// tab can offer the PGN's suggestion ([`suggestion`]).
    pub(crate) fn book_pgn_tick(&mut self) {
        let Some((vi, id)) = self.active_ids() else { return };
        // The PGN beside a document opened is offered (R6f): looked for once per document, and
        // said once per session.
        if self.book_pgn_for(id).is_some()
            && !self.chess.book_pgns.told.contains(&id)
            && let Some(name) = self.chess.book_pgns.offer(id)
        {
            self.chess.book_pgns.told.push(id);
            self.notify(crate::i18n::fmt(
                tl!("{name} sits beside this PDF: the Chess panel offers to use it as the book's PGN."),
                &[("name", &name)],
            ));
        }
        let Some(page) = self.chess.editor.as_ref().filter(|e| e.doc == id).map(|e| e.page) else { return };
        if self.views.get(vi).map(|v| v.current) != Some(page) || self.chess.job.is_some() || self.chess.book_pgns.summary(id).is_none() {
            return;
        }
        let stamp = self.session.chess_stamp(id);
        if self.chess.text.as_ref().is_some_and(|t| t.doc == id && t.page == page && Some(t.stamp) == stamp) {
            return;
        }
        self.read_chess_text(page, false);
    }
}

/// The PGN beside the PDF at `keys`' path with its name (`book.pdf` → `book.pgn` or `book.PGN`),
/// when there is one. Never on the web, which has no paths.
fn sibling_pgn(keys: &DocKeys) -> Option<std::path::PathBuf> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let pdf = std::path::Path::new(keys.path.as_deref()?);
        ["pgn", "PGN"].iter().map(|ext| pdf.with_extension(ext)).find(|p| p.is_file())
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = keys;
        None
    }
}

/// A PGN file of at most [`MAX_REFERENCE_BYTES`].
#[cfg(not(target_arch = "wasm32"))]
fn read_pgn(path: &std::path::Path) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let cap = u64::try_from(MAX_REFERENCE_BYTES).unwrap_or(u64::MAX);
    if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > cap {
        return Err(tl!("the file is too large").to_string());
    }
    let mut bytes = Vec::new();
    // One byte past the cap, so a file that grew since is still refused.
    std::fs::File::open(path).and_then(|f| f.take(cap.saturating_add(1)).read_to_end(&mut bytes)).map_err(|e| e.to_string())?;
    if bytes.len() > MAX_REFERENCE_BYTES {
        return Err(tl!("the file is too large").to_string());
    }
    Ok(bytes)
}
