//! Chess ▸ Board ▸ Analysis: the engine's evaluations kept per document (plan A9-N8, finding
//! A8-11), in the engine's `evals` store ([`EvalsKind`]) in the app's data folder's `chess/evals`,
//! beside the corrections and the books' PGNs, where `chess_evals` reads them.
//!
//! - **What is kept:** the best line of each position the board's engine analysed, once it is
//!   finished with (the board went to another position, the analysis stopped or the board was
//!   hidden, or the search reached the depth asked for), when it is at least [`MIN_DEPTH`] deep:
//!   one stepping through a game keeps nothing of the moves it passed in a blink. The store keeps
//!   the deepest per position ([`pdfcraft_engine::chess::evals`]).
//! - **Whose:** the document whose game the board shows ([`crate::chess_collection::ChessGames`]),
//!   as it was when the evaluation was found. A game of no document keeps none.
//! - **When it is written:** at most every [`WRITE_EVERY`], and at once when the app saves or
//!   quits ([`PdfCraftApp::keep_chess_evals`]). An evaluation is found again by searching, never
//!   the user's work: it is not kept for a document whose chess may not be written
//!   ([`crate::chess_keep`]), and that is never asked about for it.
//! - **Desktop only:** the web build has no engine.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use pdfcraft_engine::DocId;
use pdfcraft_engine::chess::analysis::Snapshot;
use pdfcraft_engine::chess::evals::{Eval, Evals as EvalsKind};

use crate::PdfCraftApp;
use crate::chess_keep::Keeping;
use crate::chess_store_ui::{UiStore, in_store};

/// The least depth an evaluation is kept at.
pub(crate) const MIN_DEPTH: u32 = 10;

/// How often the evaluations found are written, at most.
pub(crate) const WRITE_EVERY: Duration = Duration::from_secs(5);

/// The evaluations found and not written yet, at most: past them the oldest goes.
const MAX_PENDING: usize = 512;

/// The positions remembered as kept, so that the lines the engine keeps for the session, shown
/// again on coming back to a position, are not kept again: as many as it keeps.
const MAX_REMEMBERED: usize = pdfcraft_engine::chess::analysis::MAX_KEPT;

/// The bytes the evaluations take in memory when they are kept nowhere else (a test).
const BUDGET: usize = 8 << 20;

/// An evaluation found: the document whose game it is, the position's FEN, the evaluation.
pub(crate) type Found = (DocId, String, Eval);

/// What the board's engine found (in [`crate::chess_study_ui::StudyState`]).
#[derive(Default)]
pub(crate) struct Finding {
    /// The deepest evaluation of the position being analysed.
    best: Option<Found>,
    /// The ones finished with, to keep.
    done: Vec<Found>,
    /// The positions kept lately, and how deep.
    kept: VecDeque<(String, u32)>,
}

impl Finding {
    /// What the engine says now (`snap`) of the position `sent` to it, the game of document `doc`:
    /// its best line, when deeper than the one noted.
    pub(crate) fn note(&mut self, doc: Option<DocId>, sent: Option<&str>, snap: &Snapshot) {
        let (Some(doc), Some(fen), Some(line)) = (doc, snap.fen.as_deref(), snap.lines.first()) else { return };
        if sent != Some(fen) || line.depth < MIN_DEPTH {
            return;
        }
        let deeper = |depth: u32| line.depth > depth;
        if self.best.as_ref().is_some_and(|(_, f, e)| f == fen && !deeper(e.depth)) || self.kept.iter().any(|(f, depth)| f == fen && !deeper(*depth))
        {
            return;
        }
        let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let Some(eval) = Eval::new(line.depth, line.score, line.bound, &line.uci, snap.name.as_deref(), at) else { return };
        if self.best.as_ref().is_some_and(|(_, f, _)| f != fen) {
            self.finish();
        }
        self.best = Some((doc, fen.to_owned(), eval));
        // Searched to the depth asked: finished with, though the board stays on it.
        if !snap.searching {
            self.finish();
        }
    }

    /// The position being analysed is finished with: its best evaluation is to keep.
    pub(crate) fn finish(&mut self) {
        let Some(found) = self.best.take() else { return };
        self.kept.retain(|(f, _)| *f != found.1);
        if self.kept.len() >= MAX_REMEMBERED {
            self.kept.pop_front();
        }
        self.kept.push_back((found.1.clone(), found.2.depth));
        if self.done.len() >= MAX_PENDING {
            self.done.remove(0);
        }
        self.done.push(found);
    }

    /// The evaluations finished with since the last time.
    pub(crate) fn take(&mut self) -> Vec<Found> {
        std::mem::take(&mut self.done)
    }
}

/// The evaluations in the app: where they are kept, and the ones waiting to be written.
pub struct Evals {
    pub(crate) store: UiStore<EvalsKind>,
    pending: Vec<Found>,
    /// When they were last written.
    written: Option<Instant>,
}

impl Default for Evals {
    fn default() -> Self {
        Evals { store: UiStore::memory(BUDGET), pending: Vec::new(), written: None }
    }
}

impl Evals {
    /// Keep the evaluations in `dir` (the app's data folder's `chess/evals`; tests pass their own).
    pub fn use_folder(&mut self, dir: std::path::PathBuf) {
        self.store = UiStore::folder(dir, BUDGET);
    }
}

impl PdfCraftApp {
    /// Keep the evaluations the board's engine finished with: written when [`WRITE_EVERY`] has
    /// passed since the last write, or `now` (the app saves or quits). How long until the next
    /// write is due, when some wait.
    pub(crate) fn keep_chess_evals(&mut self, now: bool) -> Option<Duration> {
        let found = self.chess.study.take_evals();
        let evals = &mut self.chess.evals;
        evals.pending.extend(found);
        if let Some(over) = evals.pending.len().checked_sub(MAX_PENDING) {
            evals.pending.drain(..over);
        }
        if evals.pending.is_empty() {
            return None;
        }
        let since = evals.written.map(|t| t.elapsed());
        if let Some(since) = since.filter(|s| !now && *s < WRITE_EVERY) {
            return Some(WRITE_EVERY.saturating_sub(since));
        }
        evals.written = Some(Instant::now());
        let pending = std::mem::take(&mut evals.pending);
        let mut docs: Vec<DocId> = Vec::new();
        for (doc, _, _) in &pending {
            if !docs.contains(doc) {
                docs.push(*doc);
            }
        }
        for doc in docs {
            // Never asked about for an evaluation: kept only where the document's chess may be.
            if self.chess_keeping(doc) != Keeping::Yes {
                continue;
            }
            let Some(d) = self.session.get(doc) else { continue };
            let (keys, pages) = (d.doc_keys(), d.info.pages.len());
            let of_doc: Vec<(String, Eval)> = pending.iter().filter(|(d, _, _)| *d == doc).map(|(_, fen, e)| (fen.clone(), e.clone())).collect();
            let kept = self
                .chess
                .evals
                .store
                .change(true, &keys, pages, |place| in_store!(place, s => s.keep(&keys, pages, &of_doc)).map_err(|e| e.to_string()));
            if let Err(e) = kept {
                log::warn!("the engine's evaluations could not be kept: {e}");
            }
        }
        None
    }
}
