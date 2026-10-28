//! Chess ▸ Diagrams ▸ the corrections the user confirms, kept in the app (plan A8, P3b2).
//!
//! **Where.** The desktop keeps them in the `chess/corrections` folder of the recovery folder,
//! beside the autosaved games and under the same instance lock (`DirStore`); the web build keeps
//! them in eframe's `Storage` under [`STORAGE_KEY`], within [`WEB_BUDGET`] bytes of their own
//! (`MemStore`). Until either is set (tests, a session without recovery) they live in memory for
//! the session. The format and the rules are the engine's (`pdfcraft_engine::chess::corrections`):
//! one file per document, found by its keys, each correction by its page's fingerprint and the
//! diagram's box.
//!
//! **What uses them.** Each document's corrections are given to the session as soon as its scan
//! results are kept ([`PdfCraftApp::refresh_chess_kinds`]), so the Text tab, the exports and the
//! lichess links read the corrected positions (P3a3); the Diagrams list and the marks on the page
//! show Verified and Corrected, and a confirmed diagram leaves the review queue. Saving records
//! them under the saved file's keys ([`PdfCraftApp::rekey_chess_corrections`]).

use std::collections::HashMap;

use pdfcraft_engine::DocId;
use pdfcraft_engine::chess::DocKeys;
use pdfcraft_engine::chess::corrections::{
    CorrectionError, CorrectionFile, CorrectionStore, Kind, Loaded, MemStore, NewCorrection, Original, StoreError, normalize,
};

use crate::PdfCraftApp;

/// The web build's `Storage` key for the corrections.
pub const STORAGE_KEY: &str = "pdfcraft-chess-corrections-v1";

/// The bytes the web build keeps corrections in, apart from the games' autosave.
pub const WEB_BUDGET: usize = 1 << 20;

/// Where the corrections are kept.
pub(crate) enum Store {
    #[cfg(not(target_arch = "wasm32"))]
    Folder(pdfcraft_engine::chess::corrections::DirStore),
    /// The web build's `Storage` value (`saved`: whether it changed since eframe last took it),
    /// or the session's memory until a store is set.
    Memory { store: MemStore, changed: bool },
}

impl Default for Store {
    fn default() -> Self {
        Store::Memory { store: MemStore::new(WEB_BUDGET), changed: false }
    }
}

impl Store {
    fn load(&self, keys: &DocKeys, pages: usize) -> Result<Loaded, StoreError> {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Store::Folder(s) => s.load(keys, pages),
            Store::Memory { store, .. } => store.load(keys, pages),
        }
    }

    fn update<R>(
        &mut self,
        keys: &DocKeys,
        pages: usize,
        change: impl FnOnce(&mut CorrectionFile) -> Result<R, CorrectionError>,
    ) -> Result<(R, Loaded), StoreError> {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Store::Folder(s) => s.update(keys, pages, change),
            Store::Memory { store, changed } => {
                let out = store.update(keys, pages, change)?;
                *changed = true;
                Ok(out)
            }
        }
    }

    fn rekey(&mut self, old: &DocKeys, new: &DocKeys, pages: usize) -> Result<bool, StoreError> {
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

/// One open document's corrections.
pub(crate) struct DocCorrections {
    pub doc: DocId,
    /// The keys they were loaded under: a save that changes them moves the corrections along.
    pub keys: DocKeys,
    pub file: CorrectionFile,
    /// The document's page fingerprints, for the document state they were made in.
    fingerprints: Option<(pdfcraft_engine::chess::ScanStamp, Vec<String>)>,
}

/// The corrections in the app.
#[derive(Default)]
pub struct Corrections {
    pub(crate) store: Store,
    pub(crate) docs: Vec<DocCorrections>,
    /// Why the store could not be read or written, said once until it works again.
    said: bool,
}

impl Corrections {
    /// Keep the corrections in `dir` (the desktop; tests pass their own folder). Corrections
    /// loaded from memory before are read again from there.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn use_folder(&mut self, dir: std::path::PathBuf) {
        self.store = Store::Folder(pdfcraft_engine::chess::corrections::DirStore::new(dir));
        self.docs.clear();
    }

    /// Keep the corrections in the web's `Storage` value `text` (what it holds now: the files by
    /// name, as [`Self::storage_value`] wrote them). A value that can't be read starts empty.
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

    /// The web's `Storage` value, when the corrections changed since it was last taken.
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

    fn doc(&self, id: DocId) -> Option<&DocCorrections> {
        self.docs.iter().find(|d| d.doc == id)
    }
}

impl DocCorrections {
    /// The correction over diagram `rect` (its user-space box) on `page`, given the page's
    /// crop box, when there is one.
    fn correction_at(&self, page: usize, rect: [f32; 4], crop: [f32; 4]) -> Option<&pdfcraft_engine::chess::corrections::Correction> {
        let fps = &self.fingerprints.as_ref()?.1;
        self.file.find(fps, page, normalize(rect, crop)?)
    }
}

/// Seconds since 1970, now.
fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl PdfCraftApp {
    /// Document `id`'s corrections, loaded from the store the first time and given to the
    /// session; their fingerprints made again when the document changed.
    fn chess_corrections_for(&mut self, id: DocId) -> Option<&DocCorrections> {
        let doc = self.session.get(id)?;
        let (keys, pages, stamp) = (doc.doc_keys(), doc.info.pages.len(), self.session.chess_stamp(id)?);
        if self.chess.corrections.doc(id).is_none() {
            let file = match self.chess.corrections.store.load(&keys, pages) {
                Ok(loaded) => {
                    if let Some((name, why)) = &loaded.set_aside {
                        self.notify(crate::i18n::fmt(
                            tl!("Saved diagram corrections couldn't be read ({why}); they were kept aside as {name}."),
                            &[("why", why), ("name", name)],
                        ));
                    }
                    loaded.file
                }
                // No keys: nothing is kept for it yet, and it starts empty.
                Err(StoreError::NoKeys) => CorrectionFile::new(Default::default()),
                Err(e) => {
                    self.say_corrections_failed(&e.to_string());
                    CorrectionFile::new(Default::default())
                }
            };
            self.session.set_chess_corrections(id, Some(file.clone()));
            self.chess.corrections.docs.push(DocCorrections { doc: id, keys, file, fingerprints: None });
        }
        let stale = self.chess.corrections.doc(id).is_none_or(|d| d.fingerprints.as_ref().is_none_or(|(s, _)| *s != stamp));
        if stale {
            let fps = self.session.get(id).map(|d| d.page_fingerprints()).unwrap_or_default();
            if let Some(d) = self.chess.corrections.docs.iter_mut().find(|d| d.doc == id) {
                d.fingerprints = Some((stamp, fps));
            }
        }
        self.chess.corrections.doc(id)
    }

    /// Document `id`'s corrections, for a reading made in a session of its own (the Text tab's
    /// and the exports' jobs, P3a3): the same bytes give the same page fingerprints there.
    pub(crate) fn chess_corrections_file(&mut self, id: DocId) -> Option<CorrectionFile> {
        self.chess_corrections_for(id).map(|c| c.file.clone()).filter(|f| f.live().next().is_some())
    }

    fn say_corrections_failed(&mut self, why: &str) {
        if !std::mem::replace(&mut self.chess.corrections.said, true) {
            self.notify(crate::i18n::fmt(tl!("The diagram corrections couldn't be saved: {e}"), &[("e", why)]));
        }
    }

    /// Mark each scanned diagram with what the user did with it (Verified, Corrected): the list,
    /// the marks on the page and the review queue follow. Closed documents' corrections go.
    pub(crate) fn refresh_chess_kinds(&mut self) {
        let open: Vec<DocId> = self.chess.corrections.docs.iter().map(|d| d.doc).filter(|d| self.session.get(*d).is_none()).collect();
        self.chess.corrections.docs.retain(|d| !open.contains(&d.doc));
        let Some(id) = self.chess.results.as_ref().map(|r| r.doc) else { return };
        let mut kinds = HashMap::new();
        self.chess_corrections_for(id);
        if let Some(c) = self.chess.corrections.doc(id)
            && let Some(results) = self.chess.results.as_ref()
        {
            for page in &results.pages {
                let Some(crop) = page.geometry.map(|g| g.crop_box()) else { continue };
                for (i, d) in page.diagrams.iter().enumerate() {
                    let b = d.candidate.bbox_user;
                    if let Some(e) = c.correction_at(page.page, [b.x0, b.y0, b.x1, b.y1], [crop.x0, crop.y0, crop.x1, crop.y1]) {
                        kinds.insert((page.page, i), (e.kind, e.fen.clone()));
                    }
                }
            }
        }
        if let Some(results) = self.chess.results.as_mut() {
            results.kinds = kinds;
        }
    }

    /// Diagram `index` of `page` of `id` as the scan found it: what a correction of it needs.
    pub(crate) fn chess_diagram_target(&self, id: DocId, page: usize, index: usize) -> Option<DiagramTarget> {
        let results = self.chess.results.as_ref().filter(|r| r.doc == id)?;
        let p = results.pages.iter().find(|p| p.page == page)?;
        let (d, crop) = (p.diagrams.get(index)?, p.geometry?.crop_box());
        let b = d.candidate.bbox_user;
        let rect = normalize([b.x0, b.y0, b.x1, b.y1], [crop.x0, crop.y0, crop.x1, crop.y1])?;
        let read = crate::chess_ui::ChessResults::reading(p, index).and_then(|r| r.position()).map(|p| p.fen.clone());
        Some(DiagramTarget { doc: id, page, index, rect, read })
    }

    /// Keep `fen` for diagram `target` without changing the PDF: Verified when it is the
    /// reading, else Corrected, and say so. The kind kept, or `None` when it could not be.
    pub(crate) fn keep_chess_correction(&mut self, target: &DiagramTarget, fen: String) -> Option<Kind> {
        let DiagramTarget { doc: id, page, index, rect, .. } = *target;
        let kind = if target.read.as_deref() == Some(fen.as_str()) { Kind::Verified } else { Kind::Corrected };
        let page_fp = self.chess_corrections_for(id).and_then(|c| c.fingerprints.as_ref()?.1.get(page).cloned())?;
        let doc = self.session.get(id)?;
        let (keys, pages) = (doc.doc_keys(), doc.info.pages.len());
        let new = NewCorrection { page, page_fp, rect, orig: Some(Original { fen: target.read.clone(), model: None }), fen, kind };
        let at = now();
        match self.chess.corrections.store.update(&keys, pages, |f| f.set(new, at)) {
            Ok((_, loaded)) => {
                self.chess.corrections.said = false;
                self.chess_corrections_kept(id, keys, loaded.file);
            }
            Err(e) => {
                self.say_corrections_failed(&e.to_string());
                return None;
            }
        }
        // A draft of it is confirmed too.
        self.chess.drafts.retain(|d| (d.target.doc, d.target.page, d.target.index) != (id, page, index));
        let state = crate::chess_widgets::DiagramState::of(None, Some(kind)).name();
        let label = self.chess_page_label(page);
        self.notify(crate::i18n::fmt(tl!("Diagram {n}, page {p}: {state}"), &[("n", &(index + 1).to_string()), ("p", &label), ("state", state)]));
        Some(kind)
    }

    /// Diagram Confirm: keep the editor's position for its diagram ([`Self::keep_chess_correction`]);
    /// then (`next`) open the next diagram to review. Whether it was kept.
    pub fn confirm_chess_diagram(&mut self, next: bool) -> bool {
        let Some(ed) = self.chess.editor.as_ref() else { return false };
        let (id, page, index, fen) = (ed.doc, ed.page, ed.index, ed.fen());
        let Some(target) = self.chess_diagram_target(id, page, index) else { return false };
        if self.keep_chess_correction(&target, fen.clone()).is_none() {
            return false;
        }
        // The editor's position is the one kept now: nothing left to confirm.
        if let Some(ed) = self.chess.editor.as_mut() {
            ed.start = fen;
        }
        self.chess.diagram_hint_seen = true;
        if next {
            self.chess_editor_action(crate::chess_editor::EditorAction::Next);
        }
        true
    }

    /// Forget the correction of the editor's diagram: it shows what was read again. Whether there
    /// was one.
    pub fn forget_chess_correction(&mut self) -> bool {
        let Some(ed) = self.chess.editor.as_ref() else { return false };
        let (id, page, index) = (ed.doc, ed.page, ed.index);
        let Some(uid) = self.chess_correction_uid(id, page, index) else { return false };
        let Some(doc) = self.session.get(id) else { return false };
        let (keys, pages) = (doc.doc_keys(), doc.info.pages.len());
        let at = now();
        let gone = self
            .chess
            .corrections
            .store
            .update(&keys, pages, |f| if f.delete(&uid, at) { Ok(()) } else { Err(CorrectionError::NoSuchCorrection(uid.clone())) });
        match gone {
            Ok((_, loaded)) => self.chess_corrections_kept(id, keys, loaded.file),
            Err(e) => {
                self.say_corrections_failed(&e.to_string());
                return false;
            }
        }
        // The editor shows the reading again, with nothing left to confirm.
        if let Some(ed) = self.chess.editor.as_mut() {
            ed.reset();
            ed.start = ed.fen();
        }
        self.chess.drafts.retain(|d| (d.target.doc, d.target.page, d.target.index) != (id, page, index));
        true
    }

    /// The uid of the correction over diagram `index` of `page` of `id`, when there is one.
    fn chess_correction_uid(&mut self, id: DocId, page: usize, index: usize) -> Option<String> {
        let (b, crop) = {
            let results = self.chess.results.as_ref().filter(|r| r.doc == id)?;
            let p = results.pages.iter().find(|p| p.page == page)?;
            (p.diagrams.get(index)?.candidate.bbox_user, p.geometry?.crop_box())
        };
        let c = self.chess_corrections_for(id)?;
        c.correction_at(page, [b.x0, b.y0, b.x1, b.y1], [crop.x0, crop.y0, crop.x1, crop.y1]).map(|e| e.uid.clone())
    }

    /// The store kept `file` for `id` under `keys`: the session and the marks take it.
    fn chess_corrections_kept(&mut self, id: DocId, keys: DocKeys, file: CorrectionFile) {
        self.session.set_chess_corrections(id, Some(file.clone()));
        match self.chess.corrections.docs.iter_mut().find(|d| d.doc == id) {
            Some(d) => {
                d.keys = keys;
                d.file = file;
            }
            None => self.chess.corrections.docs.push(DocCorrections { doc: id, keys, file, fingerprints: None }),
        }
        self.refresh_chess_kinds();
    }

    /// Document `id` was saved: its corrections are recorded under the keys it has now, so they
    /// are found when the saved file is opened again (as the game collection follows it).
    pub(crate) fn rekey_chess_corrections(&mut self, id: DocId) {
        let Some(doc) = self.session.get(id) else { return };
        let (keys, pages) = (doc.doc_keys(), doc.info.pages.len());
        let Some(old) = self.chess.corrections.doc(id).map(|d| d.keys.clone()).filter(|k| *k != keys) else { return };
        match self.chess.corrections.store.rekey(&old, &keys, pages) {
            Ok(_) => {
                if let Some(d) = self.chess.corrections.docs.iter_mut().find(|d| d.doc == id) {
                    d.keys = keys;
                }
            }
            Err(e) => self.say_corrections_failed(&e.to_string()),
        }
    }

    /// The printed label of page `page` (0-based) of the active document, else its number.
    pub(crate) fn chess_page_label(&self, page: usize) -> String {
        self.active_ids()
            .and_then(|(_, id)| self.session.get(id))
            .and_then(|d| d.info.pages.get(page))
            .map_or_else(|| (page + 1).to_string(), |p| p.label.clone())
    }
}
