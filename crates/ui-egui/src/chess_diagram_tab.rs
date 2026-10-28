//! Chess ▸ Diagram (plan A8, P3b2): the selected diagram's position to check and confirm, a tab
//! of its own beside the Inspector.
//!
//! The header says which diagram it is ("Page 17 · diagram 1 of 2", a link that shows it on the
//! page), opens the position on the analysis board and walks the review queue ("12 of 40 to
//! review", Page Up / Page Down). Under it the board (as large as the tab allows, 200 to 420 pt),
//! the palette under the board, who moves, the move number, and the details (castling, the FEN,
//! the view from Black's side, back to the reading) with the model's second readings. The footer
//! stays put: whether the position is legal (and why not), the other actions, and the main one,
//! Confirm and next ▾ Only confirm (Ctrl+Enter), which keeps the position for the diagram
//! (Verified when it is the reading, else Corrected) without changing the PDF. The last choice is
//! remembered. Preferences ▸ Classic diagram editor keeps the editor in the Diagrams panel instead.

use egui::{Align, Layout, RichText, UiBuilder};
use pdfcraft_engine::chess::pdfcraft_chess::placement::Color;

use crate::chess_editor::{DiagramEditor, EditorAction};
use crate::chess_ui::{ChessResults, shown};
use crate::chess_widgets::{DiagramState, state_icon};
use crate::theme::Tokens;
use crate::{PdfCraftApp, widgets};

/// The header's and the footer's heights, in points.
const HEADER: f32 = 36.0;
const FOOTER: f32 = 52.0;

/// The board's side: the tab's width less a margin, and its height less what goes around the
/// board, from [`MIN_BOARD`] to [`MAX_BOARD`] points.
const MIN_BOARD: f32 = 200.0;
const MAX_BOARD: f32 = 420.0;
const AROUND_BOARD: f32 = 204.0;

/// The tab: the editor of the diagram selected in the Diagrams panel or on the page, else what to
/// do to get one.
pub(crate) fn panel(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let Some((_, id)) = app.active_ids() else {
        empty(ui, &t);
        return;
    };
    let current = app.session.chess_stamp(id);
    let Some(results) = shown(&app.chess, id, current) else {
        empty(ui, &t);
        return;
    };
    let queue = results.review_queue();
    let Some(ed) = app.chess.editor.as_ref().filter(|e| e.doc == id) else {
        empty(ui, &t);
        return;
    };
    let (page, index) = (ed.page, ed.index);
    let on_page = results.pages.iter().find(|p| p.page == page).map_or(0, |p| p.diagrams.len());
    let state =
        DiagramState::of(results.pages.iter().find(|p| p.page == page).and_then(|p| ChessResults::reading(p, index)), results.kind(page, index));
    let kept = results.kind(page, index).is_some();
    let place = queue.iter().position(|q| *q == (page, index));
    let label = app.chess_page_label(page);
    let pieces = app.chess.role_pieces(pdfcraft_engine::chess::pdfcraft_chessvision::pieces::choice::Role::Diagrams);
    let confirm_only = app.chess.confirm_only;
    let Some(ed) = app.chess.editor.as_mut() else { return };

    let mut action = keys(ui, queue.len());
    let whole = ui.max_rect();
    let header_rect = egui::Rect::from_min_size(whole.min, egui::vec2(whole.width(), HEADER));
    let footer_rect = egui::Rect::from_min_max(egui::pos2(whole.min.x, (whole.max.y - FOOTER).max(whole.min.y + HEADER)), whole.max);
    let body_rect = egui::Rect::from_min_max(egui::pos2(whole.min.x, header_rect.max.y), egui::pos2(whole.max.x, footer_rect.min.y));

    ui.scope_builder(UiBuilder::new().max_rect(header_rect.shrink2(egui::vec2(8.0, 4.0))), |ui| {
        // The buttons first, from the right; the title takes what is left, shortened if need be.
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            {
                ui.add_enabled_ui(!queue.is_empty(), |ui| {
                    if crate::icons::button(ui, "chevron-right", 24.0, false, tl!("Next to review (Page Down)")).clicked() {
                        action = Some(EditorAction::Next);
                    }
                    let left = match place {
                        Some(k) => crate::i18n::fmt(tl!("{i} of {n} to review"), &[("i", &(k + 1).to_string()), ("n", &queue.len().to_string())]),
                        None => crate::i18n::fmt(tl!("{n} to review"), &[("n", &queue.len().to_string())]),
                    };
                    ui.label(RichText::new(left).small().color(t.text_muted));
                    if crate::icons::button(ui, "chevron-left", 24.0, false, tl!("Previous to review (Page Up)")).clicked() {
                        action = Some(EditorAction::Previous);
                    }
                });
                if crate::icons::button(ui, "gauge", 24.0, false, tl!("Analyze on the board")).clicked() {
                    action = Some(EditorAction::OpenBoard);
                }
            }
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                state_icon(ui, &t, state, 16.0);
                let title = crate::i18n::fmt(
                    tl!("Page {p} · diagram {n} of {m}"),
                    &[("p", &label), ("n", &(index + 1).to_string()), ("m", &on_page.max(index + 1).to_string())],
                );
                let link = ui.add(egui::Label::new(RichText::new(&title).strong().color(t.accent_text)).truncate().sense(egui::Sense::click()));
                link.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Link, true, &title));
                if crate::theme::hand(link).on_hover_text(tl!("Show it on the page")).clicked() {
                    action = Some(EditorAction::ShowOnPage);
                }
            });
        });
    });

    ui.scope_builder(UiBuilder::new().max_rect(body_rect.shrink2(egui::vec2(8.0, 0.0))), |ui| {
        egui::ScrollArea::vertical().id_salt("chess-diagram-tab").auto_shrink([false, false]).show(ui, |ui| {
            let side = (ui.available_width() - 24.0).min(body_rect.height() - AROUND_BOARD).clamp(MIN_BOARD, MAX_BOARD);
            let sprites = crate::chess_editor::palette_sprites(ui, ed, &pieces);
            ui.vertical_centered(|ui| crate::chess_editor::board_view(ui, &t, ed, &pieces, side, true));
            ui.add_space(6.0);
            crate::chess_editor::palette(ui, sprites.as_ref(), &mut ed.brush, false);
            ui.add_space(6.0);
            fields(ui, ed);
            crate::chess_editor::second_readings(ui, ed);
        });
    });

    ui.scope_builder(UiBuilder::new().max_rect(footer_rect), |ui| {
        ui.painter().hline(footer_rect.x_range(), footer_rect.min.y, egui::Stroke::new(1.0, t.border));
        let inner = footer_rect.shrink2(egui::vec2(8.0, 10.0));
        ui.scope_builder(UiBuilder::new().max_rect(inner), |ui| {
            // The buttons first, from the right; whether the position is legal in what is left.
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let illegal = ed.illegal_text();
                if let Some(a) = confirm_button(ui, confirm_only, !queue.is_empty()) {
                    action = Some(a);
                }
                if let Some(a) = more_menu(ui, ed, illegal.is_none(), kept) {
                    action = Some(a);
                }
                let (icon, text, colour) = match &illegal {
                    None => ("circle-check", tl!("Legal position").to_string(), t.success_text),
                    Some(why) => ("circle-x", why.clone(), t.danger_text),
                };
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.add(crate::icons::image(icon, 14.0, colour));
                    ui.add(egui::Label::new(RichText::new(text).small().color(colour)).truncate())
                        .on_hover_text(illegal.as_deref().unwrap_or_default());
                });
            });
        });
    });

    if let Some(a) = action {
        app.chess_editor_action(a);
    }
}

/// Nothing to edit: how to get a diagram here.
fn empty(ui: &mut egui::Ui, t: &Tokens) {
    ui.add_space(16.0);
    ui.vertical_centered(|ui| {
        ui.add(crate::icons::image("circle-dashed", 28.0, t.text_faint));
        ui.add_space(6.0);
        ui.label(RichText::new(tl!("Select a diagram in the Diagrams panel, or click one on the page.")).color(t.text_muted));
    });
}

/// The keys the tab takes while the pointer is over it: Ctrl+Enter confirms and goes on, Page Up
/// and Page Down walk the review queue (when there is one).
fn keys(ui: &egui::Ui, queue: usize) -> Option<EditorAction> {
    if !ui.rect_contains_pointer(ui.max_rect()) || ui.ctx().egui_wants_keyboard_input() {
        return None;
    }
    ui.input_mut(|i| {
        if i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter) {
            return Some(EditorAction::Confirm { next: true });
        }
        if queue == 0 {
            return None;
        }
        if i.consume_key(egui::Modifiers::NONE, egui::Key::PageDown) {
            return Some(EditorAction::Next);
        }
        i.consume_key(egui::Modifiers::NONE, egui::Key::PageUp).then_some(EditorAction::Previous)
    })
}

/// Who moves, the move number and the details.
fn fields(ui: &mut egui::Ui, ed: &mut DiagramEditor) {
    // Each label centred on its row's controls.
    ui.horizontal(|ui| {
        let mut side = ed.position.side_to_move.unwrap_or(Color::White);
        row_label(ui, tl!("To move:"));
        ui.selectable_value(&mut side, Color::White, tl!("White"));
        ui.selectable_value(&mut side, Color::Black, tl_ctx!("chess side", "Black"));
        ed.position.side_to_move = Some(side);
    });
    ui.horizontal(|ui| {
        row_label(ui, tl!("Move number:"));
        ui.add(egui::DragValue::new(&mut ed.position.fullmove_number).range(1..=crate::chess_editor::MAX_MOVE_NUMBER).speed(0.2))
            .on_hover_text(tl!("The move the diagram stands at: 35 with Black to move is 35…"));
    });
    egui::CollapsingHeader::new(tl!("Details")).id_salt("chess-diagram-details").show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(tl!("Castling:"));
            let c = &mut ed.position.castling;
            ui.add(widgets::checkbox(&mut c.white_king_side, "K"));
            ui.add(widgets::checkbox(&mut c.white_queen_side, "Q"));
            ui.add(widgets::checkbox(&mut c.black_king_side, "k"));
            ui.add(widgets::checkbox(&mut c.black_queen_side, "q"));
            // A right the pieces do not allow is taken off again (plan A8, P3b1).
            *c = c.fitted(&ed.position.placement);
        });
        ui.add(egui::Label::new(RichText::new(ed.fen()).monospace().small()).wrap());
        ui.horizontal_wrapped(|ui| {
            if ui.small_button(tl!("Flip board")).clicked() {
                ed.flipped = !ed.flipped;
            }
            if ed.original.is_some() && ui.small_button(tl!("Back to the reading")).clicked() {
                ed.reset();
            }
        });
    });
}

/// A row's label, as tall as the row's buttons, so its text is centred on theirs.
fn row_label(ui: &mut egui::Ui, text: &str) {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let width = widgets::text_width(ui, text, &font);
    ui.add_sized(egui::vec2(width, ROW), egui::Label::new(text));
}

/// The height of a row of buttons in the tab, in points.
const ROW: f32 = 26.0;

/// The main button and its ▾: Confirm and next (Ctrl+Enter) or Only confirm, the last one chosen
/// in front. Without a review queue there is no next, so Only confirm is in front.
fn confirm_button(ui: &mut egui::Ui, confirm_only: bool, queue: bool) -> Option<EditorAction> {
    let only = confirm_only || !queue;
    let mut action = None;
    let more = crate::icons::button(ui, "chevron-down", 26.0, false, tl!("More ways to confirm"));
    egui::Popup::menu(&more).show(|ui| {
        if ui.add_enabled(queue, egui::Button::new(tl!("Confirm and next"))).clicked() {
            action = Some(EditorAction::Confirm { next: true });
        }
        if ui.button(tl!("Only confirm")).clicked() {
            action = Some(EditorAction::Confirm { next: false });
        }
    });
    let label = if only { tl!("Only confirm") } else { tl!("Confirm and next") };
    let hint = if only {
        tl!("Keep this position for the diagram, without changing the PDF.")
    } else {
        tl!("Keep this position for the diagram, without changing the PDF, and open the next one to review (Ctrl+Enter).")
    };
    if widgets::icon_pill(ui, "badge-check", label, true).on_hover_text(hint).clicked() {
        action = Some(EditorAction::Confirm { next: !only });
    }
    action
}

/// The other actions, behind ⋯.
fn more_menu(ui: &mut egui::Ui, ed: &DiagramEditor, legal: bool, kept: bool) -> Option<EditorAction> {
    let mut action = None;
    let button = crate::icons::button(ui, "ellipsis", 26.0, false, tl!("More actions"));
    egui::Popup::menu(&button).show(|ui| {
        let fen = ed.fen();
        if ui.button(tl!("Replace on the page")).on_hover_text(tl!("Draw this position over the diagram on the page (one undo step).")).clicked() {
            action = Some(EditorAction::Replace);
        }
        if ui.add_enabled(legal, egui::Button::new(tl!("Add lichess link"))).clicked() {
            action = Some(EditorAction::Link);
        }
        if ui.button(tl!("Copy FEN")).clicked() {
            action = Some(EditorAction::CopyFen(fen.clone()));
        }
        if legal && let Ok(p) = pdfcraft_engine::chess::pdfcraft_chess::fen::parse(&fen) {
            let url = pdfcraft_engine::chess::pdfcraft_chess::lichess::analysis_url(&p, if ed.flipped { Color::Black } else { Color::White });
            if ui.button(tl!("Open in lichess")).on_hover_text(&url).clicked() {
                action = Some(EditorAction::OpenLichess(url));
            }
        }
        if kept && ui.button(tl!("Forget the correction")).on_hover_text(tl!("Show what was read again; the PDF is not changed.")).clicked() {
            action = Some(EditorAction::Forget);
        }
    });
    action
}
