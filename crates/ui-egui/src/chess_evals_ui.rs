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
    /// The positions kept lately, and what was kept of each.
    kept: VecDeque<(String, Eval)>,
}

/// The line of `snap` to note: its best line, or, when that is only a bound (a search stopped
/// mid-way), the last exact one of the search, one depth short (plan A9-N17a3's
/// [`Snapshot::exact`]); the bound itself when the search has no exact line.
fn line_to_note(snap: &Snapshot) -> Option<&pdfcraft_engine::chess::analysis::Line> {
    use pdfcraft_engine::chess::pdfcraft_chess::uci::Bound;
    let best = snap.lines.iter().find(|l| l.multipv == 1).or(snap.lines.first())?;
    if best.bound == Bound::Exact {
        return Some(best);
    }
    Some(snap.exact.iter().find(|l| l.multipv == 1 && l.bound == Bound::Exact).unwrap_or(best))
}

/// Whether `new` is to be kept over `old` by the store's one rule ([`Eval::outranks`]: an exact
/// score over a bound, then deeper), the time aside: the same line shown again is not newer.
fn outranks(new: &Eval, old: &Eval) -> bool {
    Eval { at: old.at, ..new.clone() }.outranks(old)
}

impl Finding {
    /// What the engine says now (`snap`) of the position `sent` to it, the game of document `doc`:
    /// its best line ([`line_to_note`]), when it outranks the one noted ([`outranks`], plan
    /// A9-N17b: an exact line of the same depth replaces a bound).
    pub(crate) fn note(&mut self, doc: Option<DocId>, sent: Option<&str>, snap: &Snapshot) {
        let (Some(doc), Some(fen), Some(line)) = (doc, snap.fen.as_deref(), line_to_note(snap)) else { return };
        if sent != Some(fen) || line.depth < MIN_DEPTH {
            return;
        }
        let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let Some(eval) = Eval::new(line.depth, line.score, line.bound, &line.uci, snap.name.as_deref(), at) else { return };
        if self.best.as_ref().is_some_and(|(_, f, e)| f == fen && !outranks(&eval, e))
            || self.kept.iter().any(|(f, e)| f == fen && !outranks(&eval, e))
        {
            return;
        }
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
        self.kept.push_back((found.1.clone(), found.2.clone()));
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

    /// The folder they are kept in, when a folder (the engine's batch writes there off the
    /// interface thread, plan A9-N17b); `None` for memory.
    pub(crate) fn folder(&self) -> Option<std::path::PathBuf> {
        match &self.store.place {
            crate::chess_store_ui::Place::Folder(s) => Some(s.dir().to_path_buf()),
            crate::chess_store_ui::Place::Memory(_) => None,
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use pdfcraft_engine::chess::analysis::Line;
    use pdfcraft_engine::chess::pdfcraft_chess::uci::{Bound, Score};

    const FEN: &str = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1";

    fn line(depth: u32, cp: i32, bound: Bound) -> Line {
        Line { multipv: 1, depth, score: Score::Centipawns(cp), bound, uci: vec!["e7e5".into()], san: vec!["e5".into()] }
    }

    fn snap(lines: Vec<Line>, exact: Vec<Line>, searching: bool) -> Snapshot {
        Snapshot { fen: Some(FEN.into()), lines, exact, searching, ready: true, ..Snapshot::default() }
    }

    /// Plan A9-N17b (lane A's note on A9-N17a3): a best line that is only a bound (a search
    /// stopped mid-way) is not the last word: the board notes the last exact line of the search,
    /// one depth short, as the batch does.
    #[test]
    fn a_bound_best_line_notes_the_last_exact_one() {
        let mut f = Finding::default();
        f.note(Some(DocId(1)), Some(FEN), &snap(vec![line(20, 90, Bound::Lower)], vec![line(19, 30, Bound::Exact)], false));
        let kept = f.take();
        let noted: Vec<(u32, Option<i32>, Option<&str>)> = kept.iter().map(|(_, _, e)| (e.depth, e.cp, e.bound.as_deref())).collect();
        assert_eq!(noted, [(19, Some(30), None)]);
    }

    /// An exact line replaces a bound noted before at the same depth ([`Eval::outranks`]); the
    /// same line shown again is not kept twice.
    #[test]
    fn an_exact_line_replaces_a_bound_of_the_same_depth() {
        let mut f = Finding::default();
        // No exact line yet: the bound is what there is.
        f.note(Some(DocId(1)), Some(FEN), &snap(vec![line(20, 90, Bound::Lower)], Vec::new(), true));
        f.note(Some(DocId(1)), Some(FEN), &snap(vec![line(20, 40, Bound::Exact)], Vec::new(), true));
        f.finish();
        let kept = f.take();
        let noted: Vec<(u32, Option<i32>, Option<&str>)> = kept.iter().map(|(_, _, e)| (e.depth, e.cp, e.bound.as_deref())).collect();
        assert_eq!(noted, [(20, Some(40), None)]);
        f.note(Some(DocId(1)), Some(FEN), &snap(vec![line(20, 40, Bound::Exact)], Vec::new(), false));
        assert!(f.take().is_empty(), "kept once");
    }
}
