//! Chess ▸ Export (plan A8, P9c): the menu, the notice about diagrams not checked yet, and what
//! the export said, with Open folder.
//!
//! The menu (in the Diagrams panel's header and on the Text tab) names each format by what it
//! makes ("EPUB — e-book") under "Export chess book", in the order PGN, EPUB, HTML, Word, RTF,
//! Markdown. It also holds which diagrams the PGN gives a game of their own: those not in a game
//! (the default), all of them or none. The choice lasts for the session.
//!
//! Exporting while diagrams detected in the document are still to review asks first ("Diagrams
//! not checked yet: 40"). Review first opens the first of them; Export anyway goes on. Once the
//! file is written, a window says where, with what the export noted (a diagram not read, a font
//! only named) and Open folder.

use pdfcraft_engine::DocId;
use pdfcraft_engine::chess::export::ChessFormat;
use pdfcraft_engine::chess::pdfcraft_chessvision::doc::markdown::PgnPositions;

use crate::PdfCraftApp;
use crate::chess_text_ui::TextAction;

/// The formats, in the menu's order.
pub const MENU: [ChessFormat; 6] =
    [ChessFormat::Pgn, ChessFormat::Epub, ChessFormat::Html, ChessFormat::Docx, ChessFormat::Rtf, ChessFormat::Markdown];

/// A format as the menu names it: what it makes.
pub fn label(format: ChessFormat) -> &'static str {
    match format {
        ChessFormat::Pgn => tl!("PGN — games"),
        ChessFormat::Epub => tl!("EPUB — e-book"),
        ChessFormat::Html => tl!("HTML — web page"),
        // A program's and formats' own names, the same in every language.
        ChessFormat::Docx => "Word (.docx)",
        ChessFormat::Rtf => "RTF",
        ChessFormat::Markdown => "Markdown",
    }
}

/// The export menu's items (in a menu that `ui` is): the formats, then which diagrams the PGN
/// gives a game of their own (`positions`, the choice now).
pub(crate) fn items(ui: &mut egui::Ui, positions: PgnPositions) -> Option<TextAction> {
    let mut action = None;
    ui.label(egui::RichText::new(tl!("Export chess book")).strong());
    for f in MENU {
        if ui.button(label(f)).clicked() {
            action = Some(TextAction::Export(f));
            ui.close();
        }
    }
    ui.separator();
    let choices =
        [(PgnPositions::Unused, tl!("Only those not in a game")), (PgnPositions::All, tl!("All of them")), (PgnPositions::None, tl!("None"))];
    ui.menu_button(tl!("Diagrams as PGN games"), |ui| {
        for (p, name) in choices {
            if ui.add(crate::widgets::radio(positions == p, name)).clicked() {
                action = Some(TextAction::PgnPositions(p));
                ui.close();
            }
        }
    })
    .response
    .on_hover_text(tl!(
        "A diagram becomes a game that starts from its position. A diagram a game starts from or prints again is already in that game."
    ));
    action
}

/// The question asked before exporting while diagrams are still to review.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingPrompt {
    pub doc: DocId,
    pub format: ChessFormat,
    /// How many diagrams are still to review.
    pub to_review: usize,
}

/// An answer to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PendingAnswer {
    /// Open the first diagram to review; nothing is exported.
    ReviewFirst,
    ExportAnyway,
    /// Esc, or a click outside: nothing happens.
    Cancel,
}

/// What a written export said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Done {
    /// Where it was written; none on the web, where it was downloaded.
    pub path: Option<std::path::PathBuf>,
    /// "Exported to …".
    pub said: String,
    /// What the export noted, as the engine wrote it.
    pub notes: Vec<String>,
}

impl PdfCraftApp {
    /// Before exporting document `id` as `format`: when diagrams detected in it are still to
    /// review, ask first. Whether the export must wait for the answer.
    pub(crate) fn ask_chess_pending(&mut self, id: DocId, format: ChessFormat) -> bool {
        let to_review = self.chess.results.as_ref().filter(|r| r.doc == id).map_or(0, |r| r.review_queue().len());
        if to_review == 0 {
            return false;
        }
        self.chess.export_prompt = Some(PendingPrompt { doc: id, format, to_review });
        true
    }

    /// Answer the question about the diagrams still to review.
    pub fn answer_chess_pending(&mut self, answer: PendingAnswer) {
        let Some(p) = self.chess.export_prompt.take() else { return };
        match answer {
            PendingAnswer::ReviewFirst => self.review_chess_now(),
            PendingAnswer::ExportAnyway => {
                // The document must still be the one asked about.
                if self.active_ids().is_some_and(|(_, id)| id == p.doc) {
                    self.export_chess_save(p.doc, p.format);
                }
            }
            PendingAnswer::Cancel => {}
        }
    }

    /// The export of `made` was written (to `path`; none on the web): say where, with its notes.
    pub(crate) fn chess_export_done(&mut self, made: &pdfcraft_engine::chess::export::ChessExport, path: Option<std::path::PathBuf>, said: String) {
        let mut notes = Vec::new();
        let (diagrams, read) = made.diagram_counts();
        let unread = diagrams.saturating_sub(read);
        if unread > 0 {
            notes.push(crate::i18n::fmt(tl!("Diagrams not read: {n}"), &[("n", &unread.to_string())]));
        }
        notes.extend(made.notes.iter().cloned());
        self.chess.export_done = Some(Done { path, said, notes });
    }

    /// Open folder: the system's file manager shows the file written.
    pub fn reveal_chess_export(&mut self) {
        let Some(path) = self.chess.export_done.as_ref().and_then(|d| d.path.clone()) else { return };
        self.chess.export_done = None;
        self.chess.last_revealed = Some(path.clone());
        #[cfg(not(target_arch = "wasm32"))]
        if self.ctx.is_some()
            && !self.run_inline
            && let Err(e) = crate::tab_context::reveal(&path)
        {
            self.notify_error(e);
        }
    }
}

/// The question before exporting, or what the export said, while either is shown.
pub(crate) fn prompt(app: &mut PdfCraftApp, ctx: &egui::Context) {
    if let Some(p) = app.chess.export_prompt.as_ref() {
        let n = p.to_review;
        let mut answer = None;
        let shown = crate::widgets::show_modal(ctx, "chess-export-pending", |ui| {
            ui.set_max_width(420.0);
            ui.heading(crate::i18n::fmt(tl!("Diagrams not checked yet: {n}"), &[("n", &n.to_string())]));
            ui.add_space(6.0);
            ui.label(tl!("They are exported as they were read, and a reading may be wrong. Review first opens the first of them."));
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if crate::widgets::pill_button(ui, tl!("Review first"), true).clicked() {
                    answer = Some(PendingAnswer::ReviewFirst);
                }
                if crate::widgets::pill_button(ui, tl!("Export anyway"), false).clicked() {
                    answer = Some(PendingAnswer::ExportAnyway);
                }
            });
        });
        if answer.is_none() && shown.should_close() {
            answer = Some(PendingAnswer::Cancel);
        }
        if let Some(a) = answer {
            app.answer_chess_pending(a);
        }
        return;
    }
    let Some(d) = app.chess.export_done.as_ref() else { return };
    let (mut reveal, mut close) = (false, false);
    let shown = crate::widgets::show_modal(ctx, "chess-export-done", |ui| {
        ui.set_max_width(460.0);
        ui.label(egui::RichText::new(&d.said).strong());
        if !d.notes.is_empty() {
            ui.add_space(6.0);
            egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                for note in &d.notes {
                    ui.label(note);
                }
            });
        }
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if d.path.is_some() && crate::widgets::pill_button(ui, tl!("Open folder"), true).clicked() {
                reveal = true;
            }
            if crate::widgets::pill_button(ui, tl!("Close"), d.path.is_none()).clicked() {
                close = true;
            }
        });
    });
    if reveal {
        app.reveal_chess_export();
    } else if close || shown.should_close() {
        app.chess.export_done = None;
    }
}
