//! Chess ▸ Diagram ▸ drafts (plan A8, P3b3): changes made in the Diagram tab and not confirmed
//! yet, kept per diagram for the session.
//!
//! Leaving a diagram with changes keeps them as its draft; opening it again shows them. A draft
//! is marked on the diagram's state icon (a warning dot), in the list, on the page and in the tab.
//! Closing the document, quitting, exporting the book or detecting the diagrams again with drafts
//! asks about each one first ("Keep the correction of diagram 1 (page 17)?"): Confirm correction
//! keeps it as Confirm does, Discard changes drops it, Keep editing stops there and opens it. The
//! PDF is never changed.

use pdfcraft_engine::DocId;
use pdfcraft_engine::chess::export::ChessFormat;

use crate::PdfCraftApp;
use crate::chess_corrections_ui::DiagramTarget;

/// One diagram's unconfirmed changes.
#[derive(Clone, Debug, PartialEq)]
pub struct Draft {
    pub(crate) target: DiagramTarget,
    /// The position as changed, and the view it was changed in.
    pub fen: String,
    pub flipped: bool,
    /// The position it was changed from.
    pub start: String,
    /// How many squares were changed.
    pub squares: usize,
}

/// What waits for the drafts to be answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AfterDrafts {
    CloseTab(DocId),
    CloseAll,
    Quit,
    Export(ChessFormat),
    /// Detect the diagrams again (`true`: in the whole book).
    Scan(bool),
}

/// The question asked about one draft, and what then waits.
#[derive(Clone, Debug, PartialEq)]
pub struct DraftPrompt {
    pub(crate) draft: Draft,
    pub then: AfterDrafts,
}

/// An answer to the question.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DraftAnswer {
    Confirm,
    Discard,
    KeepEditing,
}

impl PdfCraftApp {
    /// The editor's changes become its diagram's draft (or its draft goes, when it has none).
    pub(crate) fn park_chess_editor(&mut self) {
        let Some(ed) = self.chess.editor.as_ref() else { return };
        let key = (ed.doc, ed.page, ed.index);
        let draft = if ed.dirty() {
            let (fen, flipped, start, squares) = (ed.fen(), ed.flipped, ed.start.clone(), ed.changed_squares());
            self.chess_diagram_target(key.0, key.1, key.2).map(|target| Draft { target, fen, flipped, start, squares })
        } else {
            None
        };
        self.chess.drafts.retain(|d| (d.target.doc, d.target.page, d.target.index) != key);
        if let Some(d) = draft {
            self.chess.drafts.push(d);
        }
    }

    /// The editor just opened: show its diagram's draft, when there is one.
    pub(crate) fn restore_chess_draft(&mut self) {
        let Some(ed) = self.chess.editor.as_mut() else { return };
        let key = (ed.doc, ed.page, ed.index);
        if let Some(d) = self.chess.drafts.iter().find(|d| (d.target.doc, d.target.page, d.target.index) == key) {
            ed.show_draft(&d.fen, d.flipped);
        }
    }

    /// Before `then` (for document `doc`, or every document): park the editor and, when a draft
    /// is waiting, ask about it. Whether `then` must wait for the answer.
    pub(crate) fn ask_chess_drafts(&mut self, doc: Option<DocId>, then: AfterDrafts) -> bool {
        self.park_chess_editor();
        let first = self.chess.drafts.iter().find(|d| doc.is_none_or(|id| d.target.doc == id)).cloned();
        match first {
            Some(draft) => {
                self.chess.draft_prompt = Some(DraftPrompt { draft, then });
                true
            }
            None => false,
        }
    }

    /// Answer the question about the draft asked about: then the next draft is asked about, and
    /// once none is left, what waited runs. Keep editing stops there and opens the diagram.
    pub fn answer_chess_draft(&mut self, answer: DraftAnswer) {
        let Some(DraftPrompt { draft, then }) = self.chess.draft_prompt.take() else { return };
        let key = (draft.target.doc, draft.target.page, draft.target.index);
        let editing = self.chess.editor.as_ref().is_some_and(|e| (e.doc, e.page, e.index) == key);
        match answer {
            DraftAnswer::Confirm => {
                if self.keep_chess_correction(&draft.target, draft.fen.clone()).is_none() {
                    // Not kept (the store failed, and said why): the draft stays, nothing goes on.
                    return;
                }
                if editing && let Some(ed) = self.chess.editor.as_mut() {
                    ed.start = draft.fen.clone();
                }
            }
            DraftAnswer::Discard => {
                self.chess.drafts.retain(|d| (d.target.doc, d.target.page, d.target.index) != key);
                if editing && let Some(ed) = self.chess.editor.as_mut() {
                    ed.show_kept(&draft.start);
                }
            }
            DraftAnswer::KeepEditing => {
                if let Some(i) = self.views.iter().position(|v| v.id == draft.target.doc) {
                    self.active = Some(i);
                }
                self.edit_chess_diagram(draft.target.page, draft.target.index, true);
                return;
            }
        }
        let doc = match then {
            AfterDrafts::CloseTab(id) => Some(id),
            AfterDrafts::Export(_) | AfterDrafts::Scan(_) => Some(draft.target.doc),
            AfterDrafts::CloseAll | AfterDrafts::Quit => None,
        };
        if self.ask_chess_drafts(doc, then) {
            return;
        }
        match then {
            AfterDrafts::CloseTab(id) => {
                if let Some(i) = self.views.iter().position(|v| v.id == id) {
                    self.request_close_tab(i);
                }
            }
            AfterDrafts::CloseAll => self.close_all(),
            AfterDrafts::Quit => {
                if let Some(ctx) = self.ctx.clone() {
                    self.request_quit(&ctx);
                }
            }
            AfterDrafts::Export(format) => self.export_chess_dialog(format),
            AfterDrafts::Scan(all) => self.start_chess_scan(all),
        }
    }
}

impl crate::chess_ui::ChessState {
    /// The diagrams of `doc` with unconfirmed changes (page, index): the drafts, and the one in
    /// the editor when it has changes.
    pub(crate) fn draft_keys(&self, doc: DocId) -> Vec<(usize, usize)> {
        let mut keys: Vec<(usize, usize)> = self.drafts.iter().filter(|d| d.target.doc == doc).map(|d| (d.target.page, d.target.index)).collect();
        if let Some(ed) = self.editor.as_ref().filter(|e| e.doc == doc && e.dirty())
            && !keys.contains(&(ed.page, ed.index))
        {
            keys.push((ed.page, ed.index));
        }
        keys
    }
}

/// The question about the draft waiting, when one is.
pub(crate) fn prompt(app: &mut PdfCraftApp, ctx: &egui::Context) {
    let Some(p) = app.chess.draft_prompt.as_ref() else { return };
    let (n, page) = (p.draft.target.index + 1, p.draft.target.page);
    let squares = p.draft.squares;
    let label =
        app.session.get(p.draft.target.doc).and_then(|d| d.info.pages.get(page)).map_or_else(|| (page + 1).to_string(), |pg| pg.label.clone());
    let mut answer = None;
    let shown = crate::widgets::show_modal(ctx, "chess-draft-prompt", |ui| {
        ui.set_max_width(420.0);
        ui.heading(crate::i18n::fmt(tl!("Keep the correction of diagram {n} (page {p})?"), &[("n", &n.to_string()), ("p", &label)]));
        ui.add_space(6.0);
        let what = if squares > 0 {
            crate::i18n::fmt(tl!("Squares changed: {n}. Not confirmed yet."), &[("n", &squares.to_string())])
        } else {
            tl!("The position changed. Not confirmed yet.").to_string()
        };
        ui.label(what);
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if crate::widgets::pill_button(ui, tl!("Confirm correction"), true).clicked() {
                answer = Some(DraftAnswer::Confirm);
            }
            if crate::widgets::pill_button(ui, tl!("Discard changes"), false).clicked() {
                answer = Some(DraftAnswer::Discard);
            }
            if crate::widgets::pill_button(ui, tl!("Keep editing"), false).clicked() {
                answer = Some(DraftAnswer::KeepEditing);
            }
        });
    });
    // Esc, or a click outside, keeps editing: nothing is lost.
    if answer.is_none() && shown.should_close() {
        answer = Some(DraftAnswer::KeepEditing);
    }
    if let Some(a) = answer {
        app.answer_chess_draft(a);
    }
}
