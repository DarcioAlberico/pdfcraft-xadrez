//! Chess ▸ Diagram ▸ drafts (plan A8, P3b3): changes made in the Diagram tab and not confirmed
//! yet, kept per diagram for the session.
//!
//! Leaving a diagram with changes keeps them as its draft; opening it again shows them. A draft
//! is marked on the diagram's state icon (a warning dot), in the list, on the page and in the tab.
//! Closing the document, quitting, exporting the book or detecting the diagrams again with drafts
//! asks about each one first ("Keep the correction of diagram 1 (page 17)?"): Confirm correction
//! keeps it as Confirm does, Discard changes drops it, Keep editing stops there and opens it. The
//! PDF is never changed. A correction the store refuses (read only, held by another writer)
//! leaves the question open, saying why in it and in a notice, each time (plan A9-G10, finding
//! A5-09): nothing waiting goes on, and nothing goes on in silence. A closed document's drafts go
//! with it ([`PdfCraftApp::sync_chess_docs`]).

use pdfcraft_engine::DocId;
use pdfcraft_engine::chess::DiagramAt;
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
    /// Why Confirm correction could not keep it, the last time it was tried (plan A9-G10).
    pub failed: Option<String>,
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
                self.chess.draft_prompt = Some(DraftPrompt { draft, then, failed: None });
                true
            }
            None => false,
        }
    }

    /// Answer the question about the draft asked about: then the next draft is asked about, and
    /// once none is left, what waited runs. Keep editing stops there and opens the diagram.
    pub fn answer_chess_draft(&mut self, answer: DraftAnswer) {
        let Some(DraftPrompt { draft, then, .. }) = self.chess.draft_prompt.take() else { return };
        let key = (draft.target.doc, draft.target.page, draft.target.index);
        let editing = self.chess.editor.as_ref().is_some_and(|e| (e.doc, e.page, e.index) == key);
        match answer {
            DraftAnswer::Confirm => {
                if let Err(why) = self.keep_chess_correction(&draft.target, draft.fen.clone()) {
                    // Not kept (and said): the draft stays and the question with it, saying why;
                    // nothing waiting goes on (finding A5-09).
                    self.chess.draft_prompt = Some(DraftPrompt { draft, then, failed: Some(why) });
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
            // Its document comes to the front, and its page is scanned first when its list doesn't
            // hold it (plan A9-I4, finding A5-09).
            DraftAnswer::KeepEditing => {
                self.pick_after_scan(draft.target.doc, pdfcraft_engine::chess::DiagramAt::new(draft.target.page, draft.target.index));
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
    /// The diagrams of `doc` with unconfirmed changes: the drafts, and the one in the editor
    /// when it has changes.
    pub(crate) fn draft_keys(&self, doc: DocId) -> Vec<DiagramAt> {
        let mut keys: Vec<DiagramAt> =
            self.drafts.iter().filter(|d| d.target.doc == doc).map(|d| DiagramAt::new(d.target.page, d.target.index)).collect();
        if let Some(ed) = self.editor.as_ref().filter(|e| e.doc == doc && e.dirty())
            && !keys.contains(&DiagramAt::new(ed.page, ed.index))
        {
            keys.push(DiagramAt::new(ed.page, ed.index));
        }
        keys
    }
}

/// The question about the draft waiting, when one is.
pub(crate) fn prompt(app: &mut PdfCraftApp, ctx: &egui::Context) {
    let Some(p) = app.chess.draft_prompt.as_ref() else { return };
    let (n, page) = (p.draft.target.index + 1, p.draft.target.page);
    let squares = p.draft.squares;
    let label = app
        .session
        .get(p.draft.target.doc)
        .map_or_else(|| page.saturating_add(1).to_string(), |d| pdfcraft_engine::chess::export::page_label_of(&d.info, page).to_string());
    let mut answer = None;
    let title = crate::i18n::fmt(tl!("Keep the correction of diagram {n} (page {p})?"), &[("n", &n.to_string()), ("p", &label)]);
    let shown = crate::widgets::show_titled_modal(ctx, "chess-draft-prompt", &title, |ui| {
        ui.set_max_width(420.0);
        let what = if squares > 0 {
            crate::i18n::fmt(tl!("Squares changed: {n}. Not confirmed yet."), &[("n", &squares.to_string())])
        } else {
            tl!("The position changed. Not confirmed yet.").to_string()
        };
        ui.label(what);
        // Why Confirm correction could not keep it, the last time (plan A9-G10).
        if let Some(why) = &p.failed {
            let t = crate::theme::Tokens::get(ui.ctx());
            ui.add_space(6.0);
            egui::Frame::NONE.fill(t.danger_soft).corner_radius(egui::CornerRadius::same(t.radius)).inner_margin(6).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.add(crate::icons::image("circle-x", 14.0, t.danger_text));
                    let said = crate::i18n::fmt(tl!("The diagram corrections couldn't be saved: {e}"), &[("e", why)]);
                    ui.add(egui::Label::new(egui::RichText::new(said).color(t.danger_text)).wrap());
                });
            });
        }
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let confirm = crate::widgets::pill_button(ui, tl!("Confirm correction"), true);
            crate::widgets::dialog_focus(&confirm);
            if confirm.clicked() {
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
