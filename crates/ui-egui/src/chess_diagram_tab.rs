//! Chess ▸ Diagram (plan A8, P3b2): the selected diagram's position to check and confirm, a tab
//! of its own beside the Inspector.
//!
//! The header says which diagram it is ("Page 17 · diagram 1 of 2", a link that shows it on the
//! page), opens the position on the analysis board and walks the diagrams the Diagrams list's
//! chip shows, or with none the ones to review ("12 of 40 to review", "3 of 12 · Corrected", Page
//! Up / Page Down; plan A8, P4), saying so when it goes round from one end to the other and when
//! the last one to review is confirmed ("Review done: 40 of 40 checked"). Under it the board (as large as the tab allows, 200 to 420 pt),
//! the palette under the board, who moves, the move number, and the details (castling, the FEN,
//! the view from Black's side, Restore the automatic reading) with the model's second readings. The footer
//! stays put: whether the position is legal (and why not), the other actions, and the main one,
//! Confirm and next ▾ Only confirm (Ctrl+Enter), which keeps the position for the diagram
//! (Verified when it is the reading, else Corrected) without changing the PDF. The last choice is
//! remembered. Preferences ▸ Classic diagram editor keeps the editor in the Diagrams panel instead.
//!
//! All of it works by the keyboard (plan A8, P3c; [`crate::chess_diagram_keys`]): the board has
//! the keyboard when a diagram opens and after a confirmation, Tab goes header → board → palette →
//! who moves → move number → details → second readings → ⋯ → Confirm → ▾, and the tab tells
//! screen readers which diagram opened.

use egui::{Align, Layout, RichText, UiBuilder};
use pdfcraft_engine::chess::pdfcraft_chess::placement::Color;

use crate::chess_diagram_keys::TabKey;
use crate::chess_editor::{DiagramEditor, EditorAction};
use crate::chess_ui::{ChessResults, diagram_title, shown, walk_place};
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

/// The book's PGN's suggestion (plan "Carregar PGN do livro", R6b): the squares it would change,
/// and a button that puts its position on the board as an unconfirmed change, which the user
/// confirms as any other. Nothing is kept until then.
fn suggestion_row(ui: &mut egui::Ui, t: &Tokens, ed: &mut crate::chess_editor::DiagramEditor, s: &crate::chess_reference_ui::Suggestion) {
    ui.add_space(4.0);
    ui.label(
        RichText::new(crate::i18n::fmt(tl!("The book's PGN shows: {changes}"), &[("changes", &s.changes.join(", "))])).small().color(t.text_muted),
    );
    let shown = ed.fen().split(' ').next() == s.fen.split(' ').next();
    if ui
        .add_enabled(!shown, egui::Button::new(tl!("Use the PGN's position")))
        .on_hover_text(tl!("Put the PGN's position on the board. It is kept only when you confirm it."))
        .clicked()
    {
        let flipped = ed.flipped;
        ed.show_draft(&s.fen, flipped);
    }
}

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
    // What the arrows walk: the list's chip, or the ones to review (plan A8, P4).
    let filter = app.chess.list.filter;
    let reviewing = matches!(filter, None | Some(DiagramState::Review));
    let queue = results.walk(filter);
    let review_done = app.chess.review_done;
    let Some(ed) = app.chess.editor.as_ref().filter(|e| e.doc == id) else {
        empty(ui, &t);
        return;
    };
    let (page, index) = (ed.page, ed.index);
    let on_page = results.pages.iter().find(|p| p.page == page).map_or(0, |p| p.diagrams.len());
    let state = DiagramState::of(
        results.pages.iter().find(|p| p.page == page).and_then(|p| ChessResults::reading(p, index)),
        results.kind(page, index),
        results.confirmed(page, index),
    );
    let kept = results.kind(page, index).is_some();
    let place = queue.iter().position(|q| *q == (page, index));
    let label = app.chess_page_label(page);
    let pieces = app.chess.role_pieces(pdfcraft_engine::chess::pdfcraft_chessvision::pieces::choice::Role::Diagrams);
    let confirm_only = app.chess.confirm_only;
    let mut hint = !app.chess.diagram_hint_seen;
    let tab_keys = std::mem::take(&mut app.chess.diagram_keys.tab_keys);
    let board_keys = app.chess.diagram_keys.board.take();
    let suggestion = crate::chess_reference_ui::suggestion(app.chess.text.as_ref(), id, current, page, index);
    let Some(ed) = app.chess.editor.as_mut() else { return };

    // The tab's keys (plan A8, P3c), taken in `raw_input_hook`.
    let mut action = None;
    let mut back_to_list = false;
    for (key, m) in tab_keys {
        match crate::chess_diagram_keys::tab_action(key, m) {
            Some(TabKey::ConfirmNext) => action = Some(EditorAction::Confirm { next: true }),
            Some(TabKey::Previous) if !queue.is_empty() => action = Some(EditorAction::Previous),
            Some(TabKey::Next) if !queue.is_empty() => action = Some(EditorAction::Next),
            Some(TabKey::Undo) => ed.undo(),
            Some(TabKey::Redo) => ed.redo(),
            Some(TabKey::BackToList) => back_to_list = true,
            _ => {}
        }
    }
    let dirty = ed.dirty();
    let whole = ui.max_rect();
    let header_rect = egui::Rect::from_min_size(whole.min, egui::vec2(whole.width(), HEADER));
    let footer_rect = egui::Rect::from_min_max(egui::pos2(whole.min.x, (whole.max.y - FOOTER).max(whole.min.y + HEADER)), whole.max);
    let body_rect = egui::Rect::from_min_max(egui::pos2(whole.min.x, header_rect.max.y), egui::pos2(whole.max.x, footer_rect.min.y));

    ui.scope_builder(UiBuilder::new().max_rect(header_rect.shrink2(egui::vec2(8.0, 4.0))), |ui| {
        // The buttons first, from the right; the title takes what is left, shortened if need be.
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            {
                ui.add_enabled_ui(!queue.is_empty(), |ui| {
                    let next = if reviewing { tl!("Next to review (Page Down)") } else { tl!("Next diagram (Page Down)") };
                    if crate::icons::button(ui, "chevron-right", 24.0, false, next).clicked() {
                        action = Some(EditorAction::Next);
                    }
                    ui.label(RichText::new(walk_place(filter, place, queue.len())).small().color(t.text_muted));
                    let previous = if reviewing { tl!("Previous to review (Page Up)") } else { tl!("Previous diagram (Page Up)") };
                    if crate::icons::button(ui, "chevron-left", 24.0, false, previous).clicked() {
                        action = Some(EditorAction::Previous);
                    }
                });
                if crate::icons::button(ui, "gauge", 24.0, false, tl!("Analyze on the board")).clicked() {
                    action = Some(EditorAction::Analyze);
                }
            }
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                state_icon(ui, &t, state, 16.0, dirty);
                let title = diagram_title(&label, index, on_page);
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
            // The review just finished (plan A8, P4).
            if let Some((checked, of)) = review_done {
                ui.horizontal(|ui| {
                    ui.add(crate::icons::image("badge-check", 14.0, t.success_text));
                    let done = crate::i18n::fmt(tl!("Review done: {d} of {n} checked"), &[("d", &checked.to_string()), ("n", &of.to_string())]);
                    ui.label(RichText::new(done).small().strong().color(t.success_text));
                });
            }
            // The first time: what a click does, and that Confirm leaves the PDF alone (P3b3).
            if hint && first_time_hint(ui, &t) {
                hint = false;
            }
            // Changes not confirmed yet (they are kept as a draft when another diagram opens).
            if dirty {
                ui.horizontal(|ui| {
                    ui.add(crate::icons::image("triangle-alert", 14.0, t.warning_text));
                    ui.label(RichText::new(tl!("Unconfirmed changes")).small().color(t.warning_text));
                });
            }
            let side = (ui.available_width() - 24.0).min(body_rect.height() - AROUND_BOARD).clamp(MIN_BOARD, MAX_BOARD);
            let sprites = crate::chess_editor::palette_sprites(ui, ed, &pieces);
            ui.vertical_centered(|ui| crate::chess_editor::board_view(ui, &t, ed, &pieces, side, true, board_keys.as_ref()));
            ui.add_space(6.0);
            crate::chess_editor::palette(ui, sprites.as_ref(), &mut ed.brush, false);
            ui.add_space(6.0);
            fields(ui, ed);
            crate::chess_editor::second_readings(ui, ed);
            if let Some(s) = &suggestion {
                suggestion_row(ui, &t, ed, s);
            }
        });
    });

    ui.scope_builder(UiBuilder::new().max_rect(footer_rect), |ui| {
        ui.painter().hline(footer_rect.x_range(), footer_rect.min.y, egui::Stroke::new(1.0, t.border));
        let inner = footer_rect.shrink2(egui::vec2(8.0, 10.0));
        ui.scope_builder(UiBuilder::new().max_rect(inner), |ui| {
            // Whether the position is legal on the left, in what the buttons leave; the buttons
            // made left to right, so the keyboard goes ⋯ → Confirm → ▾ (plan A8, P3c).
            let illegal = ed.illegal_text();
            let only = confirm_only || queue.is_empty();
            let gap = ui.spacing().item_spacing.x;
            let buttons = MENU_BUTTON + gap + widgets::icon_pill_width(ui, confirm_label(only)) + gap + MENU_BUTTON;
            let split = (inner.max.x - buttons).max(inner.min.x);
            let status = egui::Rect::from_min_max(inner.min, egui::pos2((split - gap).max(inner.min.x), inner.max.y));
            ui.scope_builder(UiBuilder::new().max_rect(status).layout(Layout::left_to_right(Align::Center)), |ui| {
                let (icon, text, colour) = match &illegal {
                    None => ("circle-check", tl!("Legal position").to_string(), t.success_text),
                    Some(why) => ("circle-x", why.clone(), t.danger_text),
                };
                ui.add(crate::icons::image(icon, 14.0, colour));
                ui.add(egui::Label::new(RichText::new(text).small().color(colour)).truncate()).on_hover_text(illegal.as_deref().unwrap_or_default());
            });
            let right = egui::Rect::from_min_max(egui::pos2(split, inner.min.y), inner.max);
            ui.scope_builder(UiBuilder::new().max_rect(right).layout(Layout::left_to_right(Align::Center)), |ui| {
                if let Some(a) = more_menu(ui, ed, illegal.is_none(), kept) {
                    action = Some(a);
                }
                if let Some(a) = confirm_button(ui, only, !queue.is_empty()) {
                    action = Some(a);
                }
            });
        });
    });
    // Changes since the last pass, for Undo.
    ed.track();
    let editor_board = crate::chess_diagram_keys::board_id(true);

    // What the tab says to screen readers, polite: the diagram opened.
    let said = &app.chess.diagram_keys.announce;
    if !said.is_empty() {
        let said = said.clone();
        ui.ctx().accesskit_node_builder(egui::Id::new("chess-diagram-announce"), |node| {
            node.set_role(egui::accesskit::Role::Status);
            node.set_live(egui::accesskit::Live::Polite);
            node.set_label(said);
        });
    }
    // For the next frame's keys: where the tab is, and whether the keyboard is in it.
    let focused = ui.ctx().memory(|m| m.focused());
    let focus_in = focused.and_then(|f| ui.ctx().read_response(f)).is_some_and(|r| whole.contains(r.rect.center()));
    app.chess.diagram_keys.tab = Some(whole);
    app.chess.diagram_keys.focus_in_tab = focus_in && !back_to_list;
    if back_to_list {
        // Esc: the keyboard goes back to the diagram's row in the list; nothing is discarded.
        app.chess.diagram_keys.focus_list = true;
        ui.memory_mut(|m| m.surrender_focus(editor_board));
        app.reveal_chess_diagrams();
    }

    if !hint {
        app.chess.diagram_hint_seen = true;
    }
    if let Some(a) = action {
        app.chess_editor_action(a);
    }
}

/// The first-time hint over the board, with a button to put it away. Whether it was put away.
fn first_time_hint(ui: &mut egui::Ui, t: &Tokens) -> bool {
    let mut away = false;
    egui::Frame::new().fill(t.accent_soft).corner_radius(6).inner_margin(8).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.add(
                egui::Label::new(
                    RichText::new(tl!("Click a square to put the piece there. Confirm keeps the correction without changing the PDF.")).small(),
                )
                .wrap(),
            );
            if crate::icons::button(ui, "x", 20.0, false, tl!("Dismiss")).clicked() {
                away = true;
            }
        });
    });
    ui.add_space(4.0);
    away
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

/// Who moves, the move number and the details.
fn fields(ui: &mut egui::Ui, ed: &mut DiagramEditor) {
    // Each label centred on its row's controls.
    ui.horizontal(|ui| {
        let mut side = ed.position.side_to_move.unwrap_or(Color::White);
        row_label(ui, tl!("To move:"));
        widgets::segmented(ui, &mut side, &[(Color::White, tl!("White")), (Color::Black, tl_ctx!("chess side", "Black"))]);
        ed.position.side_to_move = Some(side);
    });
    ui.horizontal(|ui| {
        row_label(ui, tl!("Move number:"));
        ui.add(egui::DragValue::new(&mut ed.position.fullmove_number).range(1..=crate::chess_editor::MAX_MOVE_NUMBER).speed(0.2))
            .on_hover_text(tl!("The move the diagram stands at: 35 with Black to move is 35…"));
    });
    egui::CollapsingHeader::new(tl!("Details")).id_salt("chess-diagram-details").show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            let c = &mut ed.position.castling;
            crate::chess_editor::castling_fields(ui, c);
            // A right the pieces do not allow is taken off again (plan A8, P3b1).
            *c = c.fitted(&ed.position.placement);
        });
        ui.add(egui::Label::new(RichText::new(ed.fen()).monospace().small()).wrap());
        ui.horizontal_wrapped(|ui| {
            if ui.small_button(tl!("Flip board")).clicked() {
                ed.flipped = !ed.flipped;
            }
            if ed.original.is_some() && ui.small_button(tl!("Restore the automatic reading")).clicked() {
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

/// The side of the footer's ⋯ and ▾ buttons, in points.
const MENU_BUTTON: f32 = 26.0;

/// The main button's words: Only confirm, or Confirm and next.
fn confirm_label(only: bool) -> &'static str {
    if only { tl!("Only confirm") } else { tl!("Confirm and next") }
}

/// The main button and its ▾: Confirm and next (Ctrl+Enter) or Only confirm, the last one chosen
/// in front. Without a review queue there is no next, so Only confirm is in front.
fn confirm_button(ui: &mut egui::Ui, confirm_only: bool, queue: bool) -> Option<EditorAction> {
    let only = confirm_only || !queue;
    let mut action = None;
    let hint = if only {
        tl!("Keep this position for the diagram, without changing the PDF.")
    } else {
        tl!("Keep this position for the diagram, without changing the PDF, and open the next one to review (Ctrl+Enter).")
    };
    if widgets::icon_pill(ui, "badge-check", confirm_label(only), true).on_hover_text(hint).clicked() {
        action = Some(EditorAction::Confirm { next: !only });
    }
    let more = crate::icons::button(ui, "chevron-down", MENU_BUTTON, false, tl!("More ways to confirm"));
    egui::Popup::menu(&more).show(|ui| {
        if ui.add_enabled(queue, egui::Button::new(tl!("Confirm and next"))).clicked() {
            action = Some(EditorAction::Confirm { next: true });
        }
        if ui.button(tl!("Only confirm")).clicked() {
            action = Some(EditorAction::Confirm { next: false });
        }
    });
    action
}

/// The other actions, behind ⋯.
fn more_menu(ui: &mut egui::Ui, ed: &DiagramEditor, legal: bool, kept: bool) -> Option<EditorAction> {
    let mut action = None;
    let button = crate::icons::button(ui, "ellipsis", MENU_BUTTON, false, tl!("More actions"));
    egui::Popup::menu(&button).show(|ui| {
        let fen = ed.fen();
        if ui
            .button(tl!("Redraw in the PDF…"))
            .on_hover_text(tl!("Draw this position over the original diagram in the PDF (one undo step)."))
            .clicked()
        {
            action = Some(EditorAction::Redraw);
        }
        if ui.add_enabled(legal, egui::Button::new(tl!("Add lichess link"))).clicked() {
            action = Some(EditorAction::Link);
        }
        if ui.button(tl!("Copy FEN")).on_hover_text(tl!("Position code other chess programs accept.")).clicked() {
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
