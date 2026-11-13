//! Chess ▸ Diagrams ▸ the corrections the user confirms, kept in the app (plan A8, P3b2).
//!
//! **Where.** The desktop keeps them in the `chess/corrections` folder of the app's data folder
//! (plan A9-G5a, decision D17: the one the command line uses too; [`PdfCraftApp::use_chess_data`]
//! merges in what earlier builds kept in the recovery folder) (`DirStore`); the web build keeps
//! them in eframe's `Storage` under [`STORAGE_KEY`], within [`WEB_BUDGET`] bytes of their own
//! (`MemStore`). Until either is set (tests, a session without recovery) they live in memory for
//! the session. The format and the rules are the engine's (`pdfcraft_engine::chess::corrections`):
//! one file per document, found by its keys, each correction by its page's fingerprint and the
//! diagram's box. Where they are kept is one type for every kind of store
//! ([`crate::chess_store_ui::UiStore`], plan A9-I1b): on the web a value that can't be read is
//! kept aside, never written over, and another tab's changes are merged in before writing.
//!
//! **A protected document** (plan A9-I1c, [`crate::chess_keep`]): an encrypted document's
//! corrections are written only once the user says so, asked once; until then, after a no, and
//! always for a document that doesn't allow copying, they are changed in memory only
//! ([`PdfCraftApp::chess_corrections_place`]) and the session keeps working with them.
//!
//! **What uses them.** Each document's corrections are given to the session as soon as its scan
//! results are kept ([`PdfCraftApp::refresh_chess_kinds`]), so the Text tab, the exports and the
//! lichess links read the corrected positions (P3a3); the Diagrams list and the marks on the page
//! show Verified and Corrected, and a confirmed diagram leaves the review queue. Saving records
//! them under the saved file's keys ([`PdfCraftApp::rekey_chess_corrections`]).
//!
//! **One copy, one rule** (plan A9-G2b, R3-06, R8-04). The session holds the corrections in memory
//! ([`pdfcraft_engine::Session::chess_corrections`]); the app keeps only the keys they are stored
//! under. The marks are the engine's [`assign`]: every diagram and every correction placed once,
//! with the session's page fingerprints indexed once per document state, the same rule the exports
//! apply. Never `find` per diagram, which cost O(D × C) on the interface thread and could mark two
//! diagrams with one correction the exports give to one.

use std::collections::HashMap;

use pdfcraft_engine::DocId;
use pdfcraft_engine::chess::corrections::{
    Correction, CorrectionError, CorrectionFile, Corrections as CorrectionsKind, Kind, Loaded, NewCorrection, Original, PageBoxes, Status,
    StoreError, assign, normalize,
};
use pdfcraft_engine::chess::{DiagramAt, DocKeys};

use crate::chess_fixes::FixState;
use crate::chess_history::ChessEdit;
use crate::chess_store_ui::{UiStore, in_store};
use crate::chess_ui::Kept;

use crate::PdfCraftApp;

/// The web build's `Storage` key for the corrections.
pub const STORAGE_KEY: &str = "pdfcraft-chess-corrections-v1";

/// The bytes the web build keeps corrections in, apart from the games' autosave.
pub const WEB_BUDGET: usize = 1 << 20;

/// Where the corrections are kept.
pub(crate) type Store = UiStore<CorrectionsKind>;

impl UiStore<CorrectionsKind> {
    /// The document's corrections, with its own entries of a file several books shared (plan
    /// A9-G1b; `fingerprints`: its pages').
    fn load(&self, keys: &DocKeys, fingerprints: &[String]) -> Result<Loaded, StoreError> {
        in_store!(&self.place, s => s.load_with(keys, fingerprints).map(|(l, _)| l))
    }

    /// A change, after the document's entries are taken out of a file several books shared (plan
    /// A9-G1b): written when `write`, else made in memory only (plan A9-I1c,
    /// [`PdfCraftApp::chess_corrections_place`]). Why not, in words that follow "couldn't be
    /// saved:".
    pub(crate) fn update<R>(
        &mut self,
        write: bool,
        keys: &DocKeys,
        fingerprints: &[String],
        change: impl FnOnce(&mut CorrectionFile) -> Result<R, CorrectionError>,
    ) -> Result<(R, Loaded), String> {
        if write {
            self.writable()?;
        }
        let out = in_store!(self.at(write), s => s.update_with(keys, fingerprints, change).map(|(r, l, _)| (r, l))).map_err(|e| e.to_string())?;
        if write {
            self.wrote();
        }
        Ok(out)
    }

    fn rekey(&mut self, write: bool, old: &DocKeys, new: &DocKeys, pages: usize) -> Result<bool, String> {
        if write {
            self.writable()?;
        }
        let moved = in_store!(self.at(write), s => s.rekey(old, new, pages)).map_err(|e| e.to_string())?;
        if moved && write {
            self.wrote();
        }
        Ok(moved)
    }

    /// The document's corrections changed in memory while they could not be written, merged into
    /// the store now that they may be (plan A9-I1c): what the store keeps then.
    fn release(&mut self, keys: &DocKeys, fingerprints: &[String]) -> Result<Loaded, String> {
        let (held, _) = in_store!(self.at(false), s => s.load_with(keys, fingerprints)).map_err(|e| e.to_string())?;
        self.update(true, keys, fingerprints, |f| {
            f.merge(&held.file);
            Ok(())
        })
        .map(|(_, loaded)| loaded)
    }
}

/// A diagram to keep a correction for: its document, page and index in the scan, its box as
/// fractions of the crop box, and the position read (`None`: not read).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DiagramTarget {
    pub doc: DocId,
    pub page: usize,
    pub index: usize,
    pub rect: [f32; 4],
    pub read: Option<String>,
}

/// One open document's corrections as the app loaded them. The session holds the file, the one
/// copy in memory ([`pdfcraft_engine::Session::chess_corrections`], plan A9-G2b).
pub(crate) struct DocCorrections {
    pub doc: DocId,
    /// The keys they were loaded under: a save that changes them moves the corrections along.
    pub keys: DocKeys,
    /// They are changed in the store's memory, not written (plan A9-I1c).
    pub held: bool,
}

/// The corrections in the app.
pub struct Corrections {
    pub(crate) store: Store,
    pub(crate) docs: Vec<DocCorrections>,
    /// Why the store could not be read or written, said once until it works again.
    pub(crate) said: bool,
}

impl Default for Corrections {
    fn default() -> Self {
        Corrections { store: UiStore::memory(WEB_BUDGET), docs: Vec::new(), said: false }
    }
}

impl Corrections {
    /// Keep the corrections in `dir` (the desktop; tests pass their own folder). Corrections
    /// loaded from memory before are read again from there.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn use_folder(&mut self, dir: std::path::PathBuf) {
        self.store = UiStore::folder(dir, WEB_BUDGET);
        self.docs.clear();
    }

    /// Keep the corrections in the web's `Storage` value `text` (what it holds now: the files by
    /// name, as [`Self::storage_values`] wrote them). A value that can't be read is kept aside at
    /// the next save, never written over, and nothing is kept until then (plan A9-I1b).
    pub fn use_storage(&mut self, text: Option<&str>) {
        self.store = UiStore::storage(STORAGE_KEY, WEB_BUDGET, text);
        self.docs.clear();
    }

    /// The web's `Storage` values to save ([`UiStore::storage_values`]): `current` is what the
    /// `Storage` holds under [`STORAGE_KEY`] now, merged in; `taken` says which keys are.
    pub fn storage_values(&mut self, current: Option<&str>, taken: impl Fn(&str) -> bool) -> Vec<(String, String)> {
        self.store.storage_values(current, taken)
    }

    /// Why the corrections the web's `Storage` held could not be read, while they are not kept
    /// aside yet.
    pub fn unreadable(&self) -> Option<&str> {
        self.store.unreadable()
    }

    fn doc(&self, id: DocId) -> Option<&DocCorrections> {
        self.docs.iter().find(|d| d.doc == id)
    }
}

/// A correction kept for a diagram ([`PdfCraftApp::store_chess_correction`]).
pub(crate) struct Stored {
    pub kind: Kind,
    pub uid: String,
    /// The live correction it replaced, as it was (`None`: a new one): what Undo puts back.
    pub before: Option<Correction>,
}

/// Seconds since 1970, now.
pub(crate) fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl PdfCraftApp {
    /// Load document `id`'s corrections from the store into the session the first time they are
    /// asked for; from then on the session holds them. Whether the document is open. With the
    /// page fingerprints, a file several books shared gives this book its own entries (plan
    /// A9-G1b).
    pub(crate) fn load_chess_corrections(&mut self, id: DocId) -> bool {
        if self.session.get(id).is_none() {
            return false;
        }
        if self.chess.corrections.doc(id).is_some() {
            return true;
        }
        let fingerprints = self.session.chess_fingerprints(id).map(|f| f.pages().to_vec()).unwrap_or_default();
        let Some(keys) = self.session.get(id).map(|d| d.doc_keys()) else { return false };
        let file = match self.chess.corrections.store.load(&keys, &fingerprints) {
            Ok(loaded) => {
                self.say_unread_corrections(&loaded);
                loaded.file
            }
            // No keys: nothing is kept for it yet, and it starts empty.
            Err(StoreError::NoKeys) => CorrectionFile::new(Default::default()),
            Err(e) => {
                self.say_corrections_failed(&e.to_string());
                CorrectionFile::new(Default::default())
            }
        };
        self.session.set_chess_corrections(id, Some(file));
        self.chess.corrections.docs.push(DocCorrections { doc: id, keys, held: false });
        true
    }

    /// Where document `id`'s corrections are changed (plan A9-I1c, [`crate::chess_keep`]): in the
    /// store while they may be written; else in the store's memory, never saved, which starts from
    /// what the store holds of them. Corrections held in memory are merged into the store once they
    /// may be written, and stay held while that fails (said). Whether a change goes to the store;
    /// why not when even the memory refused.
    pub(crate) fn chess_corrections_place(&mut self, id: DocId) -> Result<bool, String> {
        let write = self.chess_may_write(id);
        let Some(d) = self.chess.corrections.docs.iter().find(|d| d.doc == id) else { return Ok(write) };
        let (keys, held) = (d.keys.clone(), d.held);
        let fingerprints = self.session.chess_fingerprints(id).map(|f| f.pages().to_vec()).unwrap_or_default();
        let set_held = |app: &mut Self, held: bool| {
            if let Some(d) = app.chess.corrections.docs.iter_mut().find(|d| d.doc == id) {
                d.held = held;
            }
        };
        match (write, held) {
            (true, true) => match self.chess.corrections.store.release(&keys, &fingerprints) {
                Ok(loaded) => {
                    set_held(self, false);
                    self.chess.corrections.said = false;
                    self.say_corrections_set_aside(&loaded);
                    self.chess_corrections_kept(id, keys, loaded.file);
                    Ok(true)
                }
                Err(why) => {
                    self.say_corrections_not_kept(&why);
                    Ok(false)
                }
            },
            (false, false) => {
                // What the store holds now, else what the session holds.
                let file = match self.chess.corrections.store.load(&keys, &fingerprints) {
                    Ok(loaded) => loaded.file,
                    Err(_) => {
                        self.session.chess_corrections(id).map_or_else(|| CorrectionFile::new(Default::default()), |f| CorrectionFile::clone(&f))
                    }
                };
                self.chess.corrections.store.hold(&keys, fingerprints.len(), file)?;
                set_held(self, true);
                Ok(false)
            }
            _ => Ok(write),
        }
    }

    /// Document `id`'s corrections, for a reading made in a session of its own (the Text tab's
    /// and the exports' jobs, P3a3): the same bytes give the same page fingerprints there.
    pub(crate) fn chess_corrections_file(&mut self, id: DocId) -> Option<CorrectionFile> {
        self.load_chess_corrections(id);
        self.session.chess_corrections(id).map(|f| CorrectionFile::clone(&f))
    }

    /// Say what the store could not read (plan A9-G3a): a file, or entries of one, left out of
    /// what is shown; reading never renames a file, so it is said instead.
    fn say_unread_corrections(&mut self, loaded: &Loaded) {
        let unread = loaded.unreadable.clone().or_else(|| (loaded.skipped > 0).then(|| (loaded.name.clone(), format!("{} entries", loaded.skipped))));
        if let Some((name, why)) = unread {
            self.notify(crate::i18n::fmt(
                tl!("Some saved diagram corrections couldn't be read ({why}); the others are used, and {name} is kept, never deleted."),
                &[("why", &why), ("name", &name)],
            ));
        }
    }

    /// Say that a change set a file aside before writing (one that could not be read, or whose
    /// unreadable entries it dropped).
    pub(crate) fn say_corrections_set_aside(&mut self, loaded: &Loaded) {
        if let Some((name, why)) = &loaded.set_aside {
            self.notify(crate::i18n::fmt(
                tl!("Saved diagram corrections couldn't be read ({why}); they were kept aside as {name}."),
                &[("why", why), ("name", name)],
            ));
        }
    }

    fn say_corrections_failed(&mut self, why: &str) {
        if !std::mem::replace(&mut self.chess.corrections.said, true) {
            self.notify(crate::i18n::fmt(tl!("The diagram corrections couldn't be saved: {e}"), &[("e", why)]));
        }
    }

    /// Mark each diagram scanned in document `id` with what the user did with it (Verified,
    /// Corrected): the list, the marks on the page and the review queue follow. Closed documents'
    /// corrections go.
    pub(crate) fn refresh_chess_kinds(&mut self, id: DocId) {
        let open: Vec<DocId> = self.chess.corrections.docs.iter().map(|d| d.doc).filter(|d| self.session.get(*d).is_none()).collect();
        self.chess.corrections.docs.retain(|d| !open.contains(&d.doc));
        if self.chess.results_of(id).is_none() {
            return;
        }
        self.load_chess_corrections(id);
        let kinds = self.chess_marks(id);
        // The readings the book's PGN has, unless the user confirmed them (plan "Carregar PGN do
        // livro", R6b): the engine's rule, `Status::with_reference`.
        let index = self.book_pgn_index(id);
        let mut confirmed = std::collections::HashSet::new();
        if let (Some(index), Some(results)) = (index.as_deref(), self.chess.results_of(id)) {
            for page in &results.pages {
                for (i, _) in page.diagrams.iter().enumerate() {
                    let reading = crate::chess_ui::ChessResults::reading(page, i);
                    let kind = kinds.get(&DiagramAt::new(page.page, i)).map(|k: &Kept| k.kind);
                    if Status::with_reference(reading, kind, Some(index)) == Status::Reference {
                        confirmed.insert(DiagramAt::new(page.page, i));
                    }
                }
            }
        }
        if let Some(results) = self.chess.results_of_mut(id) {
            results.kinds = kinds;
            results.confirmed = confirmed;
        }
    }

    /// The correction over each diagram of the results of document `id`, by the engine's one rule
    /// ([`assign`]) with the session's corrections and its page fingerprints, indexed once per
    /// document state: every diagram and correction placed once, O(D + C) (plan A9-G2b, R8-04).
    /// The boxes are the scan's, those the exports' readings carry, of every page with a visible
    /// area: a diagram the page's raster could not read is marked too.
    fn chess_marks(&mut self, id: DocId) -> HashMap<DiagramAt, Kept> {
        let Some(file) = self.session.chess_corrections(id) else { return HashMap::new() };
        let Some(fingerprints) = self.session.chess_fingerprints(id) else { return HashMap::new() };
        let Some(results) = self.chess.results_of(id) else { return HashMap::new() };
        let pages: Vec<PageBoxes> = results
            .pages
            .iter()
            .filter_map(|p| {
                let crop = p.geometry?.crop_box();
                let crop = [crop.x0, crop.y0, crop.x1, crop.y1];
                let boxes = p.diagrams.iter().map(|d| d.candidate.bbox_user).map(|b| normalize([b.x0, b.y0, b.x1, b.y1], crop)).collect();
                Some(PageBoxes { page: p.page, boxes })
            })
            .collect();
        let located = file.locate(fingerprints.as_ref());
        let (assigned, _) = assign(&pages, &located);
        assigned
            .into_iter()
            .map(|a| {
                (DiagramAt::new(a.page, a.index), Kept { uid: a.correction.uid.clone(), kind: a.correction.kind, fen: a.correction.fen.clone() })
            })
            .collect()
    }

    /// Diagram `index` of `page` of `id` as the scan found it: what a correction of it needs.
    pub(crate) fn chess_diagram_target(&self, id: DocId, page: usize, index: usize) -> Option<DiagramTarget> {
        let results = self.chess.results_of(id)?;
        let p = results.pages.iter().find(|p| p.page == page)?;
        let (d, crop) = (p.diagrams.get(index)?, p.geometry?.crop_box());
        let b = d.candidate.bbox_user;
        let rect = normalize([b.x0, b.y0, b.x1, b.y1], [crop.x0, crop.y0, crop.x1, crop.y1])?;
        let read = crate::chess_ui::ChessResults::reading(p, index).and_then(|r| r.position()).map(|p| p.fen.clone());
        Some(DiagramTarget { doc: id, page, index, rect, read })
    }

    /// Keep `fen` for diagram `target` without changing the PDF: Verified when it is the
    /// reading, else Corrected. It is one step of the document's corrections, which a notice says
    /// with its Undo button (plan A9-G10). The kind kept, or why it could not be, said each time.
    pub(crate) fn keep_chess_correction(&mut self, target: &DiagramTarget, fen: String) -> Result<Kind, String> {
        let stored = match self.store_chess_correction(target, fen) {
            Ok(stored) => stored,
            Err(why) => {
                self.say_corrections_not_kept(&why);
                return Err(why);
            }
        };
        let before = FixState { entries: vec![(stored.uid, stored.before)], diagram: Some(DiagramAt::new(target.page, target.index)) };
        let seq = self.record_chess_fix(target.doc, ChessEdit::ConfirmCorrection, before);
        let state = crate::chess_widgets::DiagramState::of(None, Some(stored.kind), false).name();
        let text = crate::i18n::fmt(
            tl!("Diagram {n}, page {p}: {state}"),
            &[("n", &(target.index + 1).to_string()), ("p", &self.chess_page_label(target.page)), ("state", state)],
        );
        let undo = seq.map(|seq| (tl!("Undo").to_string(), crate::notice::NoticeAction::UndoCorrection { doc: target.doc, seq }));
        self.post_notice(text, undo);
        Ok(stored.kind)
    }

    /// Keep `fen` for diagram `target`, without a word and without a step of its own: what was
    /// kept, or why it could not be (the caller says it).
    pub(crate) fn store_chess_correction(&mut self, target: &DiagramTarget, fen: String) -> Result<Stored, String> {
        let DiagramTarget { doc: id, page, index, rect, .. } = *target;
        let kind = if target.read.as_deref() == Some(fen.as_str()) { Kind::Verified } else { Kind::Corrected };
        let closed = || tl!("the document is closed").to_string();
        if !self.load_chess_corrections(id) {
            return Err(closed());
        }
        let fingerprints = self.session.chess_fingerprints(id).ok_or_else(closed)?;
        let keys = self.session.get(id).ok_or_else(closed)?.doc_keys();
        let new = NewCorrection { page, rect, orig: Some(Original { fen: target.read.clone(), model: None }), fen, kind };
        let at = now();
        let write = self.chess_corrections_place(id)?;
        let ((uid, before), loaded) =
            self.chess.corrections.store.update(write, &keys, fingerprints.pages(), |f| f.replace(new, at, fingerprints.as_ref()))?;
        self.chess.corrections.said = false;
        self.say_corrections_set_aside(&loaded);
        self.chess_corrections_kept(id, keys, loaded.file);
        // A draft of it is confirmed too.
        self.chess.drafts.retain(|d| (d.target.doc, d.target.page, d.target.index) != (id, page, index));
        Ok(Stored { kind, uid, before })
    }

    /// Correction `uid` of document `id`, drawn over its diagram on `page` (Redraw in the PDF),
    /// follows the page as it is now ([`CorrectionFile::follow_page`]). Why not, when the store
    /// refused.
    pub(crate) fn follow_chess_correction(&mut self, id: DocId, uid: &str, page: usize) -> Result<(), String> {
        let closed = || tl!("the document is closed").to_string();
        let fingerprints = self.session.chess_fingerprints(id).ok_or_else(closed)?;
        let keys = self.session.get(id).ok_or_else(closed)?.doc_keys();
        let at = now();
        let write = self.chess_corrections_place(id)?;
        let (_, loaded) =
            self.chess.corrections.store.update(write, &keys, fingerprints.pages(), |f| f.follow_page(uid, page, at, fingerprints.as_ref()))?;
        self.say_corrections_set_aside(&loaded);
        self.chess_corrections_kept(id, keys, loaded.file);
        Ok(())
    }

    /// Chess ▸ Mark the page as checked (plan A8, P2b): every diagram of `page` read with
    /// confidence and not confirmed yet is kept as Verified, without changing the PDF. Those to
    /// review, not read or with an error are left for the person to check one by one (the plan
    /// rejects accepting them in bulk). Says what was done; how many were checked.
    pub fn verify_chess_page(&mut self, page: usize) -> usize {
        let Some((_, id)) = self.active_ids() else { return 0 };
        let label = self.chess_page_label(page);
        let Some(p) = self.chess.results_of(id).and_then(|r| r.pages.iter().find(|p| p.page == page)).cloned() else {
            self.notify(tl!("Detect the diagrams on this page first.").to_string());
            return 0;
        };
        let (mut done, mut left) = (0usize, 0usize);
        // One step of the document's corrections for the whole page (plan A9-G10).
        let mut step = FixState { entries: Vec::new(), diagram: None };
        for index in 0..p.diagrams.len() {
            let kind = self.chess.results_of(id).and_then(|r| r.kind(page, index));
            let state = Status::of(crate::chess_ui::ChessResults::reading(&p, index), kind);
            if state.to_review() {
                left = left.saturating_add(1);
            }
            if !state.checked_with_the_page() {
                continue;
            }
            let Some(target) = self.chess_diagram_target(id, page, index) else { continue };
            let Some(fen) = target.read.clone() else { continue };
            match self.store_chess_correction(&target, fen) {
                Ok(stored) => step.entries.push((stored.uid, stored.before)),
                Err(why) => {
                    // Said; nothing more is tried, and what was kept is one step still.
                    self.say_corrections_not_kept(&why);
                    self.record_chess_fix(id, ChessEdit::CheckPage, step);
                    return done;
                }
            }
            done = done.saturating_add(1);
        }
        self.record_chess_fix(id, ChessEdit::CheckPage, step);
        let mut said = if done == 0 {
            crate::i18n::fmt(tl!("Nothing on page {p} is left that was read with confidence."), &[("p", &label)])
        } else {
            let n = u64::try_from(done).unwrap_or(u64::MAX);
            let checked = crate::i18n::trn(crate::i18n::current(), n, "{n} diagram checked", "{n} diagrams checked");
            crate::i18n::fmt(tl!("Page {p}: {done}"), &[("p", &label), ("done", &checked)])
        };
        if left > 0 {
            said.push_str(" · ");
            said.push_str(&crate::i18n::fmt(tl!("{n} to review"), &[("n", &left.to_string())]));
        }
        self.notify(said);
        done
    }

    /// Diagram Confirm: keep the editor's position for its diagram ([`Self::keep_chess_correction`]);
    /// then (`next`) open the next diagram to review. Whether it was kept.
    pub fn confirm_chess_diagram(&mut self, next: bool) -> bool {
        let Some(ed) = self.chess.editor.as_ref() else { return false };
        let (id, page, index, fen) = (ed.doc, ed.page, ed.index, ed.fen());
        let Some(target) = self.chess_diagram_target(id, page, index) else { return false };
        if self.keep_chess_correction(&target, fen.clone()).is_err() {
            return false;
        }
        // The editor's position is the one kept now: nothing left to confirm.
        if let Some(ed) = self.chess.editor.as_mut() {
            ed.start = fen;
        }
        self.chess.diagram_hint_seen = true;
        if next {
            // The last one to review: the review is done, and the tab says so instead of moving
            // on (plan A8, P4).
            let reviewing = matches!(self.chess.filter(id), None | Some(crate::chess_widgets::DiagramState::Review));
            let finished = self.chess.results_of(id).filter(|r| reviewing && r.review_queue().is_empty()).map(|r| r.reviewed());
            match finished {
                Some(done) => {
                    let (d, n) = (done.checked.to_string(), done.of.to_string());
                    let said = crate::i18n::fmt(tl!("Review done: {d} of {n} checked"), &[("d", &d), ("n", &n)]);
                    self.chess.diagram_keys.announce = said.clone();
                    self.chess.review_done = Some(done);
                    self.notify(said);
                }
                None => self.chess_editor_action(crate::chess_editor::EditorAction::Next),
            }
        }
        true
    }

    /// Forget the correction of the editor's diagram: it shows what was read again. With changes
    /// over it not confirmed yet, asks first ([`crate::chess_fixes::forget_prompt`]), since they go
    /// too. Whether it was forgotten.
    pub fn forget_chess_correction(&mut self) -> bool {
        if self.chess.editor.as_ref().is_some_and(crate::chess_editor::DiagramEditor::dirty) {
            self.chess.forget_prompt = true;
            return false;
        }
        self.forget_chess_correction_now()
    }

    /// [`Self::forget_chess_correction`] without asking: one step of the document's corrections,
    /// which a notice says with its Undo button (plan A9-G10). Whether there was one to forget; a
    /// store that refused says why.
    pub(crate) fn forget_chess_correction_now(&mut self) -> bool {
        let Some(ed) = self.chess.editor.as_ref() else { return false };
        let (id, page, index) = (ed.doc, ed.page, ed.index);
        let Some(uid) = self.chess_correction_uid(id, page, index) else { return false };
        let Some(keys) = self.session.get(id).map(|d| d.doc_keys()) else { return false };
        let fingerprints = self.session.chess_fingerprints(id).map(|f| f.pages().to_vec()).unwrap_or_default();
        let at = now();
        let gone = self.chess_corrections_place(id).and_then(|write| {
            self.chess.corrections.store.update(write, &keys, &fingerprints, |f| {
                let before = f.live().find(|e| e.uid == uid).cloned();
                if f.delete(&uid, at) { Ok(before) } else { Err(CorrectionError::NoSuchCorrection(uid.clone())) }
            })
        });
        let before = match gone {
            Ok((before, loaded)) => {
                self.chess.corrections.said = false;
                self.say_corrections_set_aside(&loaded);
                self.chess_corrections_kept(id, keys, loaded.file);
                before
            }
            Err(e) => {
                self.say_corrections_not_kept(&e);
                return false;
            }
        };
        // The editor shows the reading again, with nothing left to confirm; that is no change of
        // its own to undo.
        if let Some(ed) = self.chess.editor.as_mut() {
            ed.show_reading();
            ed.settle();
        }
        self.chess.drafts.retain(|d| (d.target.doc, d.target.page, d.target.index) != (id, page, index));
        let seq = self.record_chess_fix(
            id,
            ChessEdit::ForgetCorrection,
            FixState { entries: vec![(uid, before)], diagram: Some(DiagramAt::new(page, index)) },
        );
        let text = crate::i18n::fmt(
            tl!("Diagram {n}, page {p}: correction forgotten"),
            &[("n", &(index + 1).to_string()), ("p", &self.chess_page_label(page))],
        );
        let undo = seq.map(|seq| (tl!("Undo").to_string(), crate::notice::NoticeAction::UndoCorrection { doc: id, seq }));
        self.post_notice(text, undo);
        true
    }

    /// The uid of the correction over diagram `index` of `page` of `id`, when there is one: the
    /// one the marks show ([`Self::chess_marks`]).
    fn chess_correction_uid(&mut self, id: DocId, page: usize, index: usize) -> Option<String> {
        self.refresh_chess_kinds(id);
        self.chess.results_of(id)?.kinds.get(&DiagramAt::new(page, index)).map(|k| k.uid.clone())
    }

    /// The store kept `file` for `id` under `keys`: the session holds it, and the marks follow.
    pub(crate) fn chess_corrections_kept(&mut self, id: DocId, keys: DocKeys, file: CorrectionFile) {
        self.session.set_chess_corrections(id, Some(file));
        match self.chess.corrections.docs.iter_mut().find(|d| d.doc == id) {
            Some(d) => d.keys = keys,
            None => self.chess.corrections.docs.push(DocCorrections { doc: id, keys, held: false }),
        }
        self.refresh_chess_kinds(id);
    }

    /// Document `id` was saved: its corrections are recorded under the keys it has now, so they
    /// are found when the saved file is opened again (as the game collection follows it).
    pub(crate) fn rekey_chess_corrections(&mut self, id: DocId) {
        let Some(doc) = self.session.get(id) else { return };
        let (keys, pages) = (doc.doc_keys(), doc.info.pages.len());
        let Some((old, held)) = self.chess.corrections.doc(id).map(|d| (d.keys.clone(), d.held)).filter(|(k, _)| *k != keys) else { return };
        // Held in memory: they move there, still not written (plan A9-I1c).
        match self.chess.corrections.store.rekey(!held, &old, &keys, pages) {
            Ok(_) => {
                if let Some(d) = self.chess.corrections.docs.iter_mut().find(|d| d.doc == id) {
                    d.keys = keys;
                }
            }
            Err(e) => self.say_corrections_failed(&e),
        }
    }

    /// Page `page` (0-based) of document `id` as the book numbers it ([`Self::chess_page_label`]).
    pub(crate) fn page_label_in(&self, id: DocId, page: usize) -> String {
        match self.session.get(id) {
            Some(doc) => pdfcraft_engine::chess::export::page_label_of(&doc.info, page).to_string(),
            None => page.saturating_add(1).to_string(),
        }
    }

    /// Page `page` (0-based) of the active document as the book numbers it: its printed label,
    /// else its number from 1, by the one rule the exports follow (plan A9-T2b, R3-08).
    pub(crate) fn chess_page_label(&self, page: usize) -> String {
        match self.active_ids().and_then(|(_, id)| self.session.get(id)) {
            Some(doc) => pdfcraft_engine::chess::export::page_label_of(&doc.info, page).to_string(),
            None => page.saturating_add(1).to_string(),
        }
    }
}
