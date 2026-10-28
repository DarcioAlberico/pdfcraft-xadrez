//! Chess ▸ Diagram ▸ Redraw in the PDF (plan A8, P3b4): the editor's position drawn over its
//! diagram in the PDF, as one undo step.
//!
//! The first time, a question says what it does ("Redraw in the PDF draws the corrected position
//! over the original…") with Redraw, Cancel and Don't show again. The position drawn is kept as
//! the diagram's correction, so the list says Corrected (or Verified) whatever the page reads
//! now, and the correction follows the page as it is after the redraw. Only that page is read
//! again: the list keeps every other diagram ([`PdfCraftApp::carry_chess_results`]).

use crate::PdfCraftApp;

/// The question asked before the first redraw.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RedrawPrompt {
    /// Don't show again is ticked.
    pub dont_ask: bool,
}

impl PdfCraftApp {
    /// Diagram ▸ Redraw in the PDF…: ask first, unless the question was put away; then
    /// [`Self::redraw_chess_now`].
    pub fn redraw_chess_diagram(&mut self) {
        if self.chess.editor.is_none() {
            return;
        }
        if self.chess.redraw_asked {
            self.redraw_chess_now();
        } else {
            self.chess.redraw_prompt = Some(RedrawPrompt::default());
        }
    }

    /// Answer the question: `redraw` or cancel, with Don't show again as ticked (kept only for
    /// Redraw: a Cancel never stops the question).
    pub fn answer_chess_redraw(&mut self, redraw: bool) {
        let Some(prompt) = self.chess.redraw_prompt.take() else { return };
        if redraw {
            self.chess.redraw_asked |= prompt.dont_ask;
            self.redraw_chess_now();
        }
    }

    /// Draw the editor's position over its diagram in the PDF (one undo step), keep it as the
    /// diagram's correction and read that page again. Whether it was drawn.
    pub fn redraw_chess_now(&mut self) -> bool {
        let Some(ed) = self.chess.editor.as_ref() else { return false };
        let (id, page, index, fen) = (ed.doc, ed.page, ed.index, ed.fen());
        let Some(generation) = self.session.get(id).map(|d| d.edit_generation()) else { return false };
        // What a correction of it needs, taken before the page changes.
        let target = self.chess_diagram_target(id, page, index);
        let (placement, flipped) = (ed.position.placement, ed.flipped);
        let pieces = crate::chess_editor::piece_set(&mut self.chess.pieces, &self.chess.diagram_piece_choice, &mut self.chess.diagram_piece_error);
        let replacement = pdfcraft_engine::chess::DiagramReplacement {
            drawings: Vec::new(),
            page,
            index,
            generation,
            placement,
            style: pdfcraft_engine::chess::pdfcraft_chess::draw::BoardStyle { flipped, ..Default::default() },
            pieces,
            scan: self.chess_scan_settings(),
        };
        if !self.apply_edit(pdfcraft_engine::Edit::ReplaceDiagram(Box::new(replacement))) {
            return false;
        }
        // Kept with the page as it is now, so the correction follows it. A store that fails says
        // so; the drawing stays.
        if let Some(target) = target
            && self.store_chess_correction(&target, fen.clone()).is_some()
            && let Some(ed) = self.chess.editor.as_mut()
        {
            // The position drawn is the one kept: nothing left to confirm.
            ed.start = fen;
        }
        // Ctrl+Z in the tab is the document's again: it takes the drawing back.
        if let Some(ed) = self.chess.editor.as_mut() {
            ed.forget_history();
        }
        self.notify(tl!("Diagram redrawn in the PDF").to_string());
        self.carry_chess_results();
        true
    }
}

/// The question before the first redraw, while it is asked.
pub(crate) fn prompt(app: &mut PdfCraftApp, ctx: &egui::Context) {
    let Some(p) = app.chess.redraw_prompt.as_mut() else { return };
    let mut answer = None;
    let shown = crate::widgets::show_modal(ctx, "chess-redraw-prompt", |ui| {
        ui.set_max_width(420.0);
        ui.label(tl!(
            "Redraw in the PDF draws the corrected position over the original. This changes the file; you can undo it with Ctrl+Z until you save."
        ));
        ui.add_space(8.0);
        ui.add(crate::widgets::checkbox(&mut p.dont_ask, tl!("Don't show again")));
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if crate::widgets::pill_button(ui, tl!("Redraw"), true).clicked() {
                answer = Some(true);
            }
            if crate::widgets::pill_button(ui, tl!("Cancel"), false).clicked() {
                answer = Some(false);
            }
        });
    });
    // Esc, or a click outside, cancels: the PDF is not changed.
    if answer.is_none() && shown.should_close() {
        answer = Some(false);
    }
    if let Some(redraw) = answer {
        app.answer_chess_redraw(redraw);
    }
}
