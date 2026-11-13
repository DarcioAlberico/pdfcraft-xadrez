//! Chess ▸ the book's own PGN, in the app (plan "Carregar PGN do livro", R6a).
//!
//! **Where.** As the diagram corrections: the desktop keeps a copy of each document's PGN in the
//! `chess/reference` folder of the recovery folder (`DirStore`); the web build keeps them in
//! eframe's `Storage` under [`STORAGE_KEY`], within [`WEB_BUDGET`] bytes of their own
//! (`MemStore`). Until either is set (tests, a session without recovery) they live in memory for
//! the session. The format and the rules are the engine's document store
//! (`pdfcraft_engine::chess::reference_store`, plan A9-G3a): one file per document,
//! `<hash>.reference.json`, found by its keys. Reading it never renames a file: one that can't be
//! read is said, and set aside only by the next change, which says so too. The wait for another
//! writer's lock (at most three seconds) stays on the interface thread until step I3b's writer.
//! Where they are kept is one type for every kind of store ([`crate::chess_store_ui::UiStore`],
//! plan A9-I1b).
//!
//! **Who holds it** (plan A9-I1b, §2.7). The engine's session holds each open document's PGN,
//! indexed, with the user's decisions ([`pdfcraft_engine::Session::chess_book_pgn`]): the one copy
//! in memory, which the Text tab, its jobs and the Diagrams list read. The app keeps only the keys
//! it was loaded under, and in each document's view ([`crate::chess_ui::DocChess`]) the PGN beside
//! the PDF it offers and what Link the whole book found.
//!
//! **What uses it.** Chess ▸ Load the book's PGN… (`chess.reference.load`) keeps the PGN chosen
//! for the active document, and Remove the book's PGN (`chess.reference.clear`) removes it. Each
//! page the Text tab reads is linked to it (`reference::link`, off the interface thread): a move
//! the reader could not play and the PGN placed is shown as read from the PGN, a move the PGN has
//! elsewhere as in conflict. Saving records it under the saved file's keys
//! ([`PdfCraftApp::rekey_book_pgn`]).

use std::sync::Arc;

use pdfcraft_engine::DocId;
use pdfcraft_engine::chess::BookPgn;
use pdfcraft_engine::chess::DocKeys;
use pdfcraft_engine::chess::pdfcraft_chessvision::doc::reference::{MAX_REFERENCE_BYTES, ReferenceIndex};
use pdfcraft_engine::chess::reference_store::{Choice, Decision, Kept, KeptPgn, Loaded, Reference, ReferenceStoreError};
use pdfcraft_engine::chess::span::MoveAt;

use crate::PdfCraftApp;
use crate::chess_store_ui::{UiStore, in_store};

/// The web build's `Storage` key for the books' PGNs.
pub const STORAGE_KEY: &str = "pdfcraft-chess-reference-v1";

/// The bytes the web build keeps the books' PGNs in, apart from the games and the corrections.
pub const WEB_BUDGET: usize = 2 << 20;

/// Most documents remembered as having declined the PGN beside them; the oldest go first.
pub const MAX_DECLINED: usize = 256;

/// Where the books' PGNs are kept.
type Store = UiStore<Reference>;

impl UiStore<Reference> {
    fn load(&self, keys: &DocKeys, pages: usize) -> Result<Loaded, ReferenceStoreError> {
        in_store!(&self.place, s => s.load(keys, pages))
    }

    /// The writes say why not in words that follow "was not kept as the book's PGN:".
    fn set(&mut self, keys: &DocKeys, pages: usize, pgn: KeptPgn, has_move: impl Fn(&str) -> bool) -> Result<(Kept, Loaded), String> {
        self.writable()?;
        let out = in_store!(&self.place, s => s.set(keys, pages, pgn, has_move)).map_err(|e| e.to_string())?;
        self.wrote();
        Ok(out)
    }

    fn clear(&mut self, keys: &DocKeys, pages: usize) -> Result<bool, String> {
        self.writable()?;
        let cleared = in_store!(&self.place, s => s.clear(keys, pages, now())).map_err(|e| e.to_string())?;
        if cleared {
            self.wrote();
        }
        Ok(cleared)
    }

    fn decide(&mut self, keys: &DocKeys, pages: usize, decision: Decision) -> Result<Loaded, String> {
        self.writable()?;
        let out = in_store!(&self.place, s => s.decide(keys, pages, decision)).map_err(|e| e.to_string())?;
        self.wrote();
        Ok(out)
    }

    fn rekey(&mut self, old: &DocKeys, new: &DocKeys, pages: usize) -> Result<bool, String> {
        self.writable()?;
        let moved = in_store!(&self.place, s => s.rekey(old, new, pages)).map_err(|e| e.to_string())?;
        if moved {
            self.wrote();
        }
        Ok(moved)
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
    /// The PGN beside the PDF, by its file name, when the panel offers it (R6f): the document
    /// keeps none and the user did not say Not now to it.
    pub offer: Option<String>,
}

impl BookPgnView {
    /// The user's choice for the move at `at` the walk wrote `san` and the reader read `san_read`,
    /// when they made one ([`Decision::is_for`]).
    pub fn choice(&self, at: MoveAt, san: &str, san_read: &str) -> Option<Choice> {
        self.decisions.iter().find(|d| d.is_for(at, san, san_read)).map(|d| d.choice)
    }
}

/// One open document's PGN, as loaded: the keys it was loaded under, so a save that changes them
/// moves it along. The session holds the PGN itself ([`pdfcraft_engine::Session::chess_book_pgn`]).
struct DocPgn {
    doc: DocId,
    keys: DocKeys,
}

/// The books' PGNs in the app.
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
    pub(crate) uploads: Arc<std::sync::Mutex<Vec<(DocId, String, Result<Vec<u8>, String>)>>>,
}

impl Default for BookPgns {
    fn default() -> Self {
        BookPgns {
            store: UiStore::memory(WEB_BUDGET),
            docs: Vec::new(),
            progress: None,
            declined: Vec::new(),
            told: Vec::new(),
            #[cfg(target_arch = "wasm32")]
            uploads: Default::default(),
        }
    }
}

impl BookPgns {
    /// Keep the books' PGNs in `dir` (the desktop; tests pass their own folder).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn use_folder(&mut self, dir: std::path::PathBuf) {
        self.store = UiStore::folder(dir, WEB_BUDGET);
        self.docs.clear();
    }

    /// Keep the books' PGNs in the web's `Storage` value `text` (the files by name, as
    /// [`Self::storage_values`] wrote them). A value that can't be read is kept aside at the next
    /// save, never written over, and nothing is kept until then (plan A9-I1b).
    pub fn use_storage(&mut self, text: Option<&str>) {
        self.store = UiStore::storage(STORAGE_KEY, WEB_BUDGET, text);
        self.docs.clear();
    }

    /// The web's `Storage` values to save ([`UiStore::storage_values`]): `current` is what the
    /// `Storage` holds under [`STORAGE_KEY`] now, merged in; `taken` says which keys are.
    pub fn storage_values(&mut self, current: Option<&str>, taken: impl Fn(&str) -> bool) -> Vec<(String, String)> {
        self.store.storage_values(current, taken)
    }

    /// Why the PGNs the web's `Storage` held could not be read, while they are not kept aside yet.
    pub fn unreadable(&self) -> Option<&str> {
        self.store.unreadable()
    }

    fn doc(&self, id: DocId) -> Option<&DocPgn> {
        self.docs.iter().find(|d| d.doc == id)
    }
}

/// Seconds since 1970, now.
fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl PdfCraftApp {
    /// Load document `id`'s PGN from the store into the session the first time it is asked for,
    /// indexed; from then on the session holds it. A PGN that can't be read is said once, and the
    /// document reads without it. Whether the document is open.
    fn book_pgn_for(&mut self, id: DocId) -> bool {
        let open: Vec<DocId> = self.session.docs().iter().map(|d| d.id).collect();
        self.chess.book_pgns.docs.retain(|d| open.contains(&d.doc));
        if self.chess.book_pgns.doc(id).is_some() {
            return true;
        }
        let Some(doc) = self.session.get(id) else { return false };
        let (keys, pages) = (doc.doc_keys(), doc.info.pages.len());
        let mut kept = None;
        match self.chess.book_pgns.store.load(&keys, pages) {
            Ok(loaded) => {
                // Reading never renames (plan A9-G3a): a file that can't be read is said, and
                // the next change sets it aside.
                if let Some((_, why)) = &loaded.unreadable {
                    self.notify(crate::i18n::fmt(tl!("The book's PGN couldn't be read: {e}"), &[("e", why)]));
                }
                if let Some(pgn) = loaded.pgn() {
                    match ReferenceIndex::from_pgn(&pgn.text) {
                        Ok(i) => kept = Some(BookPgn { name: pgn.name.clone(), index: Arc::new(i), decisions: loaded.file.decisions.clone() }),
                        Err(e) => self.notify(crate::i18n::fmt(tl!("The book's PGN couldn't be read: {e}"), &[("e", &e.to_string())])),
                    }
                }
            }
            // No keys: nothing is kept for it.
            Err(ReferenceStoreError::NoKeys) => {}
            Err(e) => self.notify(crate::i18n::fmt(tl!("The book's PGN couldn't be read: {e}"), &[("e", &e.to_string())])),
        }
        // The folder is looked through only when there is something to offer.
        let sibling = if kept.is_none() { find_sibling_pgn(&keys) } else { None };
        self.chess.doc_mut(id).pgn_sibling = sibling;
        self.session.set_chess_book_pgn(id, kept);
        self.chess.book_pgns.docs.push(DocPgn { doc: id, keys });
        true
    }

    /// Document `id`'s PGN, indexed, when it keeps one.
    pub(crate) fn book_pgn_index(&mut self, id: DocId) -> Option<Arc<ReferenceIndex>> {
        self.book_pgn_for(id);
        self.session.chess_book_pgn(id).map(|p| p.index.clone())
    }

    /// What the Text tab says about document `id`'s PGN, when it keeps one.
    pub fn book_pgn_summary(&mut self, id: DocId) -> Option<BookPgnSummary> {
        self.book_pgn_for(id);
        self.kept_book_pgn(id)
    }

    /// What document `id` keeps, as the session holds it (loaded when the Text tab reads a page).
    fn kept_book_pgn(&self, id: DocId) -> Option<BookPgnSummary> {
        let p = self.session.chess_book_pgn(id)?;
        Some(BookPgnSummary { name: p.name.clone(), games: p.index.games().len(), moves: p.index.moves() })
    }

    /// What the Chess panel and its Text tab show of document `id`'s book's PGN, as loaded.
    pub(crate) fn book_pgn_view(&self, id: DocId) -> BookPgnView {
        let pgns = &self.chess.book_pgns;
        let progress = pgns.progress.as_ref().map(|(done, of)| (done.load(std::sync::atomic::Ordering::Relaxed), *of));
        let kept = self.session.chess_book_pgn(id);
        let view = self.chess.doc(id);
        let sibling = view
            .filter(|_| kept.is_none())
            .and_then(|v| v.pgn_sibling.as_ref())
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned());
        let declined =
            pgns.doc(id).and_then(|d| pdfcraft_engine::chess::corrections::file_name(&d.keys)).is_none_or(|key| pgns.declined.contains(&key));
        BookPgnView {
            summary: self.kept_book_pgn(id),
            offer: sibling.clone().filter(|_| !declined),
            sibling,
            link: view.and_then(|v| v.book_link.clone()),
            progress,
            decisions: kept.map(|p| p.decisions.clone()).unwrap_or_default(),
        }
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
                // At most the cap, read as the models' picker reads (plan A9-I1b, R2-05).
                let bytes = crate::chess_models::read_picked(&h, MAX_REFERENCE_BYTES).await;
                uploads.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push((id, h.file_name(), bytes));
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
            match bytes {
                Ok(bytes) => self.set_book_pgn(id, &name, &bytes),
                Err(e) => self.notify_fmt("Couldn't read {name}: {e}", &[("name", &name), ("e", &e)]),
            }
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
        let (text, encoding) = pdfcraft_engine::chess::pdfcraft_chess::pgn::decode(bytes);
        let index = match ReferenceIndex::from_pgn(&text) {
            Ok(i) if !i.games().is_empty() => i,
            Ok(_) => return self.notify(refused(&tl!("no game in it"))),
            Err(e) => return self.notify(refused(&e)),
        };
        let summary = BookPgnSummary { name: Some(name.to_owned()), games: index.games().len(), moves: index.moves() };
        let kept = KeptPgn::new(Some(name), text, summary.games, summary.moves, now())
            .map_err(|e| e.to_string())
            .and_then(|pgn| self.chess.book_pgns.store.set(&keys, pages, pgn.read_from(bytes), |san| index.has_move(san)));
        // The decisions stay, with the same PGN (plan A9-F1) and with another (decision D25, plan
        // A9-G4); those naming a move the new one doesn't have are said.
        let (decisions, unmatched) = match kept {
            Ok((kept, loaded)) => {
                self.say_book_pgn_set_aside(&loaded);
                (loaded.file.decisions, kept.unmatched)
            }
            Err(e) => return self.notify(refused(&e)),
        };
        self.chess.book_pgns.docs.retain(|d| d.doc != id);
        self.chess.book_pgns.docs.push(DocPgn { doc: id, keys });
        self.session.set_chess_book_pgn(id, Some(BookPgn { name: Some(name.to_owned()), index: Arc::new(index), decisions }));
        // It keeps one now: nothing beside it to offer (Remove the book's PGN looks again), and the
        // whole book is linked to another PGN no more.
        let view = self.chess.doc_mut(id);
        view.pgn_sibling = None;
        view.book_link = None;
        // The Text tab reads its page again, linked to it, and the Diagrams list takes it.
        self.chess.doc_mut(id).text = None;
        self.refresh_chess_kinds(id);
        let mut note = crate::i18n::fmt(
            tl!("Book's PGN {name}: {games} games, {moves} moves. The pages the Text tab reads are linked to it."),
            &[("name", name), ("games", &summary.games.to_string()), ("moves", &summary.moves.to_string())],
        );
        if let Some(latin) = crate::chess_collection::encoding_note(encoding) {
            note.push_str(" · ");
            note.push_str(&latin);
        }
        if unmatched > 0 {
            note.push_str(" · ");
            note.push_str(&crate::i18n::fmt(
                tl!("Choices on moves in conflict naming a move this PGN doesn't have: {n}. They are kept for a PGN that has it."),
                &[("n", &unmatched.to_string())],
            ));
        }
        self.notify(note);
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
        let Some(path) = self.chess.doc(id).and_then(|d| d.pgn_sibling.clone()) else { return };
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
    pub(crate) fn decide_book_move(&mut self, at: MoveAt, san: String, san_read: String, choice: Choice) {
        let Some((_, id)) = self.active_ids() else { return };
        let Some(doc) = self.session.get(id) else { return };
        let (keys, pages) = (doc.doc_keys(), doc.info.pages.len());
        // The PGN's move it is in conflict with, as the PGN writes it (plan A9-G4, R6-10).
        let pgn_san = self.conflict_pgn_move(id, at).unwrap_or_default();
        let decision = Decision::new(at, san, san_read, pgn_san, choice);
        match self.chess.book_pgns.store.decide(&keys, pages, decision) {
            Ok(loaded) => {
                self.say_book_pgn_set_aside(&loaded);
                if let Some(kept) = self.session.chess_book_pgn(id) {
                    let pgn = BookPgn { decisions: loaded.file.decisions, ..BookPgn::clone(&kept) };
                    self.session.set_chess_book_pgn(id, Some(pgn));
                }
            }
            Err(e) => self.notify(crate::i18n::fmt(tl!("The choice was not kept: {e}"), &[("e", &e)])),
        }
    }

    /// The book's PGN's move that the move at `at` of document `id`, as the Text tab read its page,
    /// is linked to (in conflict, the same move from another position), as the PGN writes it.
    fn conflict_pgn_move(&self, id: DocId, at: MoveAt) -> Option<String> {
        use pdfcraft_engine::chess::pdfcraft_chessvision::doc::Inline;
        use pdfcraft_engine::chess::pdfcraft_chessvision::doc::reference::RefNode;
        let text = self.chess.text_of(id)?;
        let page = text.chess.pages.iter().find(|p| p.index == at.page)?;
        let block = page.blocks.get(usize::try_from(at.block).ok()?)?;
        let Inline::Move(m) = block.inlines().get(usize::try_from(at.inline).ok()?)? else { return None };
        let r = m.reference.as_ref()?;
        let kept = self.session.chess_book_pgn(id)?;
        kept.index.san(RefNode { game: r.game?, node: r.node? }).map(str::to_owned)
    }

    /// Chess ▸ Remove the book's PGN: remove the PGN kept for the active document.
    pub fn clear_book_pgn(&mut self) {
        let Some((_, id)) = self.active_ids() else { return };
        let Some(doc) = self.session.get(id) else { return };
        let (keys, pages) = (doc.doc_keys(), doc.info.pages.len());
        match self.chess.book_pgns.store.clear(&keys, pages) {
            Ok(cleared) => {
                self.chess.book_pgns.docs.retain(|d| d.doc != id);
                let sibling = find_sibling_pgn(&keys);
                self.chess.book_pgns.docs.push(DocPgn { doc: id, keys });
                self.session.set_chess_book_pgn(id, None);
                let view = self.chess.doc_mut(id);
                view.pgn_sibling = sibling;
                view.book_link = None;
                view.text = None;
                self.refresh_chess_kinds(id);
                let said = if cleared { tl!("The book's PGN was removed from this document.") } else { tl!("This document keeps no book's PGN.") };
                self.notify(said.to_string());
            }
            Err(e) => self.notify(crate::i18n::fmt(tl!("The book's PGN couldn't be removed: {e}"), &[("e", &e)])),
        }
    }

    /// Say that a change set the document's file aside before writing (one that could not be
    /// read, or whose unreadable decisions it dropped; plan A9-G3a).
    fn say_book_pgn_set_aside(&mut self, loaded: &Loaded) {
        if let Some((name, why)) = &loaded.set_aside {
            self.notify(crate::i18n::fmt(
                tl!("The book's PGN kept for this document couldn't be read ({why}); it was kept aside as {name}."),
                &[("why", why), ("name", name)],
            ));
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
            Err(e) => self.notify(crate::i18n::fmt(tl!("The book's PGN was not kept for the saved file: {e}"), &[("e", &e)])),
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
        if self.book_pgn_for(id)
            && !self.chess.book_pgns.told.contains(&id)
            && let Some(name) = self.book_pgn_view(id).offer
        {
            self.chess.book_pgns.told.push(id);
            self.notify(crate::i18n::fmt(
                tl!("{name} sits beside this PDF: the Chess panel offers to use it as the book's PGN."),
                &[("name", &name)],
            ));
        }
        let Some(page) = self.chess.editor.as_ref().filter(|e| e.doc == id).map(|e| e.page) else { return };
        if self.views.get(vi).map(|v| v.current) != Some(page) || self.chess.job.is_some() || self.session.chess_book_pgn(id).is_none() {
            return;
        }
        let stamp = self.session.chess_stamp(id);
        if self.chess.text_of(id).is_some_and(|t| t.page == page && Some(t.stamp) == stamp) {
            return;
        }
        self.read_chess_text(page, false);
    }
}

/// Most entries of the PDF's folder looked at for its PGN: the folder may be Downloads (plan
/// A9-I7a). The PGN named exactly as the PDF is found whatever the folder holds.
#[cfg(not(target_arch = "wasm32"))]
const MAX_FOLDER_ENTRIES: usize = 4096;

/// The PGN beside the PDF at `keys`' path that goes with it ([`sibling_pgn`]: `book.pdf` →
/// `book.pgn`, and `📚Book.pdf` → `Book.pgn`), when there is one. Never on the web, which has no
/// paths.
fn find_sibling_pgn(keys: &DocKeys) -> Option<std::path::PathBuf> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let pdf = std::path::Path::new(keys.path.as_deref()?);
        if let Some(same) = ["pgn", "PGN"].iter().map(|ext| pdf.with_extension(ext)).find(|p| p.is_file()) {
            return Some(same);
        }
        let dir = pdf.parent()?;
        let names: Vec<String> = std::fs::read_dir(dir)
            .ok()?
            .take(MAX_FOLDER_ENTRIES)
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .filter(|n| is_pgn_name(n) && dir.join(n).is_file())
            .collect();
        let name = sibling_pgn(pdf.file_name()?.to_str()?, names.iter().map(String::as_str))?;
        Some(dir.join(name))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = keys;
        None
    }
}

/// Whether `name` is a PGN's file name (`.pgn`, in any case).
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn is_pgn_name(name: &str) -> bool {
    std::path::Path::new(name).extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("pgn"))
}

/// What a file name's stem is matched by (plan A9-I7a, finding A6-03): its letters and digits in
/// lower case, every run of anything else (spaces, punctuation, symbols, emoji) one space, none
/// at either end. `📚 Livro_3.Nc3` gives `livro 3 nc3`.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn name_key(stem: &str) -> String {
    let mut key = String::with_capacity(stem.len());
    let mut gap = false;
    for c in stem.chars() {
        if !c.is_alphanumeric() {
            gap = true;
            continue;
        }
        if gap && !key.is_empty() {
            key.push(' ');
        }
        gap = false;
        key.extend(c.to_lowercase());
    }
    key
}

/// The PGN among `names` (file names in the folder of the PDF named `pdf`) that goes with the
/// PDF: the one named exactly as it is, else the first, in name order, whose stem is the PDF's
/// once symbols, emoji, spaces and case are set aside ([`name_key`]). A PDF whose name has no
/// letter or digit has none. Only an offer: the user says whether to use it.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn sibling_pgn<'a>(pdf: &str, names: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    fn stem(name: &str) -> Option<&str> {
        std::path::Path::new(name).file_stem().and_then(|s| s.to_str())
    }
    let pdf_stem = stem(pdf)?;
    let key = name_key(pdf_stem);
    if key.is_empty() {
        return None;
    }
    let pgns: Vec<&'a str> = names.into_iter().filter(|n| is_pgn_name(n)).collect();
    if let Some(same) = pgns.iter().copied().find(|n| stem(n) == Some(pdf_stem)) {
        return Some(same);
    }
    pgns.into_iter().filter(|n| stem(n).is_some_and(|s| name_key(s) == key)).min()
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The PGN beside the PDF is the one named as it is, or, failing that, one whose name is the
    /// same once emoji, symbols, spaces and case are set aside (plan A9-I7a, finding A6-03: the
    /// user's books start with an emoji their PGNs don't have).
    #[test]
    fn the_pgn_beside_is_found_by_its_name_set_apart_from_symbols_and_case() {
        assert_eq!(sibling_pgn("📚Livro.pdf", ["Livro.pgn"]), Some("Livro.pgn"));
        let user = "📚Harikrisna Pentala. Beat the French Defense with 3. Nc3.pdf";
        assert_eq!(
            sibling_pgn(user, ["notes.txt", "Harikrisna Pentala. Beat the French Defense with 3. Nc3.pgn"]),
            Some("Harikrisna Pentala. Beat the French Defense with 3. Nc3.pgn")
        );
        // Spaces collapsed, symbols and case set aside, the extension in any case.
        assert_eq!(sibling_pgn("My  Book (2nd ed.).pdf", ["my book 2nd ed.PGN"]), Some("my book 2nd ed.PGN"));
        assert_eq!(sibling_pgn("Livro.pdf", ["LIVRO.Pgn"]), Some("LIVRO.Pgn"));
        // The one named exactly as the PDF wins over one that only matches; else the first by name.
        assert_eq!(sibling_pgn("Livro.pdf", ["livro!.pgn", "Livro.pgn"]), Some("Livro.pgn"));
        assert_eq!(sibling_pgn("📚Livro.pdf", ["livro!.pgn", "Livro.pgn"]), Some("Livro.pgn"));
        // Another name, another file kind, or a PDF named by symbols alone: nothing offered.
        assert_eq!(sibling_pgn("Livro 2.pdf", ["Livro.pgn"]), None);
        assert_eq!(sibling_pgn("Livro.pdf", ["Livro.txt", "Livro.pgn.bak"]), None);
        assert_eq!(sibling_pgn("📚.pdf", ["!!.pgn", ".pgn"]), None);
        assert_eq!(sibling_pgn("", ["Livro.pgn"]), None);
        assert_eq!(name_key("📚 Livro_3.Nc3  "), "livro 3 nc3");
        assert_eq!(name_key("Éé Ñ"), "éé ñ");
    }
}
