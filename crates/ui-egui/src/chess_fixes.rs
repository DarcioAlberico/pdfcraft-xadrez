//! Chess ▸ Diagram ▸ the corrections as steps Undo and Redo take (plan A9-G10, findings A6-04,
//! A5-06).
//!
//! Confirm (in the Diagram tab, or Confirm correction asked about a draft), Forget the correction
//! and Mark the page as checked are each one step of the document's corrections, kept per document
//! for the session ([`crate::chess_ui::DocChess::fixes`]). A step keeps each correction it changed
//! as it was on its far side. Undoing or redoing it writes them back through the store, under its
//! lock and over what is there now, as their **next** revision
//! ([`pdfcraft_engine::chess::corrections::CorrectionFile::restore`]): a forgotten correction put
//! back is a new revision of it with `deleted` false, so a merge with any copy that saw it
//! forgotten keeps it. A step moves to the other side only once the store kept it; a store that
//! refuses (read only, held by another writer) says why, each time, and the step stays.
//!
//! **Where Undo goes** (D12): to the corrections right after one was kept or forgotten, which a
//! notice says with its Undo button ("Diagram 2, page 13: Corrected · Undo", [`crate::notice`]),
//! until a click on the page or in the chess board's panels, a change to the document or F6
//! ([`crate::chess_ui::ChessState::undo_target`]). In the Diagram tab, Ctrl+Z and Ctrl+Shift+Z
//! decide in one place ([`PdfCraftApp::diagram_tab_step`]): the editor's change or the
//! correction's step, whichever is the newer, else the document's change — never the chess game's,
//! which need not even be in sight (A5-06).
//!
//! **Forget with unconfirmed changes asks first** ([`forget_prompt`]): undoing the forgetting
//! brings the correction back, not the changes made over it.

use pdfcraft_engine::DocId;
use pdfcraft_engine::chess::corrections::Correction;

use crate::PdfCraftApp;
use crate::chess_history::{ChessEdit, Estimate};

/// Bytes a correction kept in a step takes, besides its texts (as the history's caps count it).
const ENTRY_BYTES: usize = 256;

/// One side of a step of the document's corrections.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FixState {
    /// Each correction the step changed, by uid, as it is on this side: `None` when no
    /// correction of that uid is live here.
    pub(crate) entries: Vec<(String, Option<Correction>)>,
    /// The diagram it was about, (page, index) as the scan numbered it then, when one.
    pub(crate) diagram: Option<(usize, usize)>,
}

impl Estimate for FixState {
    fn bytes(&self) -> usize {
        self.entries.iter().fold(0usize, |sum, (uid, e)| {
            let texts = e.as_ref().map_or(0, |e| e.fen.len().saturating_add(e.page_fp.len()));
            sum.saturating_add(ENTRY_BYTES).saturating_add(uid.len()).saturating_add(texts)
        })
    }
}

impl PdfCraftApp {
    /// Keep the step `edit` of document `id`'s corrections, `before` being how they were: Undo
    /// and Redo go to the corrections now. When it was put on the undo side, if it was kept.
    pub(crate) fn record_chess_fix(&mut self, id: DocId, edit: ChessEdit, before: FixState) -> Option<u64> {
        if before.entries.is_empty() {
            return None;
        }
        let fixes = &mut self.chess.doc_mut(id).fixes;
        fixes.record(edit, before);
        self.chess.fix_scope = true;
        self.chess.doc(id).and_then(|d| d.fixes.seq(true))
    }

    /// Undo (`undo`) or redo the step of document `id`'s corrections Undo or Redo would take:
    /// its corrections are written back, then the step moves to the other side and the marks and
    /// the editor follow. Whether it was taken; a store that refused says why.
    pub(crate) fn chess_fix_step(&mut self, id: DocId, undo: bool) -> bool {
        let Some(d) = self.chess.doc(id) else { return false };
        let (Some(target), Some(edit)) = (d.fixes.peek(undo).cloned(), if undo { d.fixes.undo_edit() } else { d.fixes.redo_edit() }.cloned()) else {
            return false;
        };
        let now = match self.write_chess_fix(id, &target) {
            Ok(now) => now,
            Err(why) => {
                self.say_corrections_not_kept(&why);
                return false;
            }
        };
        // Kept: the step goes to the other side, with how the corrections were before it.
        if let Some(d) = self.chess.docs.get_mut(&id) {
            if undo {
                d.fixes.undo(now);
            } else {
                d.fixes.redo(now);
            }
        }
        self.chess.fix_scope = true;
        if self.notice.as_ref().is_some_and(|n| matches!(n.action, Some((_, crate::notice::NoticeAction::UndoCorrection { .. })))) {
            self.notice = None;
        }
        self.chess_fix_shown(id, &edit, &target);
        let label = edit.text();
        if undo {
            self.notify_fmt("Undid {label}", &[("label", &label)]);
        } else {
            self.notify_fmt("Redid {label}", &[("label", &label)]);
        }
        true
    }

    /// The notice's Undo: the step of document `id`'s corrections put on the undo side at `seq`,
    /// while it is still the one Undo would take there.
    pub(crate) fn undo_chess_fix_at(&mut self, id: DocId, seq: u64) {
        if self.chess.doc(id).and_then(|d| d.fixes.seq(true)) == Some(seq) {
            self.chess_fix_step(id, true);
        }
    }

    /// Write `target`'s corrections into document `id`'s store, as their next revisions; the
    /// session holds what was kept. How they were in the store just before (the step's other
    /// side), or why not, when the store refused.
    fn write_chess_fix(&mut self, id: DocId, target: &FixState) -> Result<FixState, String> {
        if !self.load_chess_corrections(id) {
            return Err(tl!("the document is closed").to_string());
        }
        let Some(keys) = self.session.get(id).map(|d| d.doc_keys()) else { return Err(tl!("the document is closed").to_string()) };
        let fingerprints = self.session.chess_fingerprints(id).map(|f| f.pages().to_vec()).unwrap_or_default();
        let at = crate::chess_corrections_ui::now();
        let written = self.chess.corrections.store.update(&keys, &fingerprints, |f| {
            let mut entries = Vec::with_capacity(target.entries.len());
            for (uid, entry) in &target.entries {
                entries.push((uid.clone(), f.live().find(|e| e.uid == *uid).cloned()));
                f.restore(uid, entry.as_ref(), at)?;
            }
            Ok(FixState { entries, diagram: target.diagram })
        });
        let (before, loaded) = written.map_err(|e| e.to_string())?;
        self.chess.corrections.said = false;
        self.say_corrections_set_aside(&loaded);
        self.chess_corrections_kept(id, keys, loaded.file);
        Ok(before)
    }

    /// The editor of the step's diagram follows what is kept for it now: its starting position is
    /// the kept one (or the reading). A forgotten correction undone shows on the board again; one
    /// forgotten again shows the reading; a confirmation undone leaves the position on the board as
    /// changes not confirmed, nothing of the user's lost.
    fn chess_fix_shown(&mut self, id: DocId, edit: &ChessEdit, target: &FixState) {
        let Some((page, index)) = target.diagram else { return };
        let kept = self.chess.results_of(id).and_then(|r| r.kinds.get(&(page, index))).map(|k| k.fen.clone());
        let Some(ed) = self.chess.editor.as_mut().filter(|e| (e.doc, e.page, e.index) == (id, page, index)) else { return };
        match (kept, edit) {
            (Some(fen), ChessEdit::ForgetCorrection) => ed.show_kept(&fen),
            (Some(fen), _) => {
                if let Ok(p) = pdfcraft_engine::chess::pdfcraft_chess::fen::parse(&fen) {
                    ed.start = p.fen();
                }
            }
            (None, ChessEdit::ForgetCorrection) => ed.show_reading(),
            (None, _) => ed.start = ed.reading_fen(),
        }
        ed.settle();
    }

    /// Undo (`undo`) or Redo in the Diagram tab, its Ctrl+Z and Ctrl+Shift+Z (plan A9-G10,
    /// finding A5-06): the editor's change or the step of the document's corrections, whichever
    /// is the newer (the corrections' only while they are in scope: right after a Redraw in the
    /// PDF, the drawing is what Ctrl+Z takes back), else the document's change. Never the chess
    /// game's, whatever its scope.
    pub(crate) fn diagram_tab_step(&mut self, undo: bool) {
        let Some(ed) = self.chess.editor.as_ref() else { return };
        let id = ed.doc;
        let editor = ed.seq(undo);
        let fix = if self.chess.fix_scope { self.chess.doc(id).and_then(|d| d.fixes.seq(undo)) } else { None };
        match (editor, fix) {
            (Some(e), f) if f.is_none_or(|f| e > f) => {
                if let Some(ed) = self.chess.editor.as_mut() {
                    if undo {
                        ed.undo();
                    } else {
                        ed.redo();
                    }
                }
            }
            (_, Some(_)) => {
                self.chess_fix_step(id, undo);
            }
            _ => self.history_step(undo),
        }
    }

    /// Say that a change the user asked for could not be kept, and why: each time, not once.
    pub(crate) fn say_corrections_not_kept(&mut self, why: &str) {
        self.notify(crate::i18n::fmt(tl!("The diagram corrections couldn't be saved: {e}"), &[("e", why)]));
    }
}

/// The question before forgetting a correction with changes over it not confirmed yet, while it
/// is asked: Forget the correction drops both, Cancel (which has the keyboard) neither.
pub(crate) fn forget_prompt(app: &mut PdfCraftApp, ctx: &egui::Context) {
    if !app.chess.forget_prompt {
        return;
    }
    let mut answer = None;
    let shown = crate::widgets::show_titled_modal(ctx, "chess-forget-prompt", tl!("Forget the correction?"), |ui| {
        ui.set_max_width(420.0);
        ui.label(tl!("The changes not confirmed yet are discarded too. The PDF is not changed."));
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if crate::widgets::pill_button(ui, tl!("Forget the correction"), false).clicked() {
                answer = Some(true);
            }
            let cancel = crate::widgets::pill_button(ui, tl!("Cancel"), true);
            crate::widgets::dialog_focus(&cancel);
            if cancel.clicked() {
                answer = Some(false);
            }
        });
    });
    // Esc, or a click outside, cancels: nothing is forgotten.
    if answer.is_none() && shown.should_close() {
        answer = Some(false);
    }
    if let Some(forget) = answer {
        app.chess.forget_prompt = false;
        if forget {
            app.forget_chess_correction_now();
        }
    }
}
