//! Chess ▸ Diagram (plan A8, P3b2): the selected diagram's position to check and confirm, in the
//! Chess column's Diagrams tab in the list's place (plan A9-I0a, master-detail).
//!
//! The header goes back to the list ("‹ Diagrams", or Esc), says which diagram it is ("Page 17 · diagram 1 of 2", a link that shows it on the
//! page), opens the position on the analysis board and walks the diagrams the Diagrams list's
//! chip shows, or with none the ones to review ("12 of 40 to review", "3 of 12 · Corrected", Page
//! Up / Page Down; plan A8, P4), saying so when it goes round from one end to the other and when
//! the last one to review is confirmed ("Review done: 40 of 40 checked"). Under it **Read |
//! Original** (Alt+O; plan A9-I0, finding A6-14): the board as read, or in its square the page's
//! own picture of the diagram, drawn from the page view's picture of the page (its UV), never one
//! beside the other. Then the board (as large as the tab allows, 200 to 380 pt),
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
use pdfcraft_engine::chess::pdfcraft_chess::mate::MateReport;
use pdfcraft_engine::chess::pdfcraft_chess::placement::Color;
use pdfcraft_engine::chess::pdfcraft_chessvision::board::{BoardReading, MateFinding};

use crate::chess_diagram_keys::TabKey;
use crate::chess_editor::{DiagramEditor, EditorAction};
use crate::chess_ui::{ChessResults, diagram_title, shown, walk_place};
use crate::chess_widgets::{DiagramState, state_icon};
use crate::theme::Tokens;
use crate::{PdfCraftApp, widgets};

/// The header's and the footer's heights, in points.
const HEADER: f32 = 36.0;
const FOOTER: f32 = 52.0;

/// The board's side: the room the tab leaves beside it, and its height less what goes around the
/// board, from [`MIN_BOARD`] to [`MAX_BOARD`] points, and never more than the room ([`board_side`]).
const MIN_BOARD: f32 = 200.0;
/// Up to 380 pt, which a 420 pt column from 1,920 pt leaves (plan A9-I0c, finding R4-13).
const MAX_BOARD: f32 = 380.0;
const AROUND_BOARD: f32 = 204.0 + SWITCH_ROW;

/// The row of the Read / Original switch over the board (plan A9-I0, sketch (b)).
const SWITCH_ROW: f32 = 28.0;

/// The margin the body keeps on each side of the tab.
const BODY_MARGIN: f32 = 8.0;

/// The width the board may take in a tab `width` points wide: the body's margins and the scroll
/// bar's room come off, so the board never runs under the bar or over the tab's edge. Never
/// negative, whatever the layout gives (a width of `NaN` gives 0).
fn board_room(width: f32, bar: f32) -> f32 {
    let room = width - 2.0 * BODY_MARGIN - bar.max(0.0);
    if room.is_finite() && room > 0.0 { room } else { 0.0 }
}

/// The board's side in a tab of `room` points across and `height` points tall: as large as both
/// allow, between [`MIN_BOARD`] and [`MAX_BOARD`], and never wider than the room — a tab too
/// narrow for [`MIN_BOARD`] gets a smaller board, not one over its edge.
fn board_side(room: f32, height: f32) -> f32 {
    let tall = height - AROUND_BOARD;
    let side = if tall.is_finite() { room.min(tall) } else { room };
    side.clamp(MIN_BOARD, MAX_BOARD).min(room).max(0.0)
}

/// The book's PGN's suggestion (plan "Carregar PGN do livro", R6b): the squares it would change,
/// and a button that puts its position on the board as an unconfirmed change, which the user
/// confirms as any other. Nothing is kept until then.
fn suggestion_row(ui: &mut egui::Ui, t: &Tokens, ed: &mut crate::chess_editor::DiagramEditor, s: &crate::chess_reference_ui::Suggestion) {
    ui.add_space(4.0);
    ui.label(
        RichText::new(crate::i18n::fmt(tl!("The book's PGN shows: {changes}"), &[("changes", &s.changes.join(", "))])).small().color(t.text_muted),
    );
    // The same pieces on the same squares (plan A9-T1b: a key, not a slice of the FEN text).
    let shown = crate::chess_ui::same_pieces(&ed.fen(), &s.fen);
    if ui
        .add_enabled(!shown, egui::Button::new(tl!("Use the PGN's position")))
        .on_hover_text(tl!("Put the PGN's position on the board. It is kept only when you confirm it."))
        .clicked()
    {
        let flipped = ed.flipped;
        ed.show_draft(&s.fen, flipped);
    }
}

/// A move of a problem's solution as the solution writes it: "1.Qf7" for White, "1...Kb2" for
/// Black; the move in SAN as the solver gives it.
fn first_move(side: Color, san: &str) -> String {
    if side == Color::White { format!("1.{san}") } else { format!("1...{san}") }
}

/// What the solver found for the mate in `n` the page asks of a diagram (plan A9-N4), in one line:
/// "Mate in 2: 1.Qf7! (unique)", the keys of a cook, or no mate with the best try and the defence
/// that refutes it, so the user can tell a misread diagram from the book's mistake.
pub(crate) fn mate_line(finding: &MateFinding) -> String {
    let report: &MateReport = match finding {
        MateFinding::BothSides { n } => {
            return crate::i18n::fmt(tl!("Mate in {n}: both sides mate, so who moves stays assumed"), &[("n", &n.to_string())]);
        }
        MateFinding::Solved(report) => report,
    };
    let n = report.n.to_string();
    let mut line = match report.keys.as_slice() {
        [key] if report.unique() => crate::i18n::fmt(tl!("Mate in {n}: {key}! (unique)"), &[("n", &n), ("key", &first_move(report.side, key))]),
        // Found before the budget ran out: there may be another.
        [key] => crate::i18n::fmt(tl!("Mate in {n}: {key}! (not proved the only key)"), &[("n", &n), ("key", &first_move(report.side, key))]),
        [] if report.exhausted => crate::i18n::fmt(tl!("Mate in {n}: no key found within the search budget"), &[("n", &n)]),
        [] => match (&report.best_try, &report.refutation) {
            (Some(best), Some(reply)) => crate::i18n::fmt(
                tl!("Mate in {n}: none found; best try {best}, refuted by {reply}"),
                &[("n", &n), ("best", &first_move(report.side, best)), ("reply", &first_move(report.side.opponent(), reply))],
            ),
            _ => crate::i18n::fmt(tl!("Mate in {n}: none found"), &[("n", &n)]),
        },
        keys => {
            let written: Vec<String> = keys.iter().map(|k| format!("{}!", first_move(report.side, k))).collect();
            crate::i18n::fmt(tl!("Mate in {n}: {keys} ({k} keys)"), &[("n", &n), ("keys", &written.join(", ")), ("k", &keys.len().to_string())])
        }
    };
    if let Some(m) = report.shortest.filter(|m| *m < report.n && !report.keys.is_empty()) {
        line.push_str(" · ");
        line.push_str(&crate::i18n::fmt(tl!("mates in {m}"), &[("m", &m.to_string())]));
    }
    if report.castles {
        line.push_str(" · ");
        line.push_str(tl!("supposes castling is allowed"));
    }
    line
}

/// The tab: the editor of the diagram selected in the Diagrams panel or on the page, else what to
/// do to get one.
pub(crate) fn panel(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    // The tab has its own keys, which follow its focus (plan A8, P3c): a click in it gives the
    // board's keys and Undo back, as a click on the page does (plan A9-I18).
    if crate::chess_history::pressed_in(ui) {
        app.chess.leave_scope();
    }
    // Ctrl+Z and Ctrl+Shift+Z first, so what is drawn is what they leave (plan A9-G10).
    let tab_keys = std::mem::take(&mut app.chess.diagram_keys.tab_keys);
    for &(key, m) in &tab_keys {
        match crate::chess_diagram_keys::tab_action(key, m) {
            Some(TabKey::Undo) => app.diagram_tab_step(true),
            Some(TabKey::Redo) => app.diagram_tab_step(false),
            _ => {}
        }
    }
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
    let filter = app.chess.filter(id);
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
    let place = queue.iter().position(|q| *q == pdfcraft_engine::chess::DiagramAt::new(page, index));
    let label = app.chess_page_label(page);
    // What the solver found for the mate the page asks of the diagram, with the position it was
    // found for (plan A9-N4).
    let mate = results
        .pages
        .iter()
        .find(|p| p.page == page)
        .and_then(|p| ChessResults::reading(p, index))
        .and_then(BoardReading::position)
        .and_then(|pos| pos.mate.as_ref().map(|m| (pos.fen.clone(), mate_line(m))));
    let pieces = app.chess.role_pieces(pdfcraft_engine::chess::pdfcraft_chessvision::pieces::choice::Role::Diagrams);
    let confirm_only = app.chess.confirm_only;
    let mut hint = !app.chess.diagram_hint_seen;
    let board_keys = app.chess.diagram_keys.board.take();
    let suggestion = crate::chess_reference_ui::suggestion(app.chess.text_of(id), id, current, page, index);
    // The page's own picture of the diagram, for Original (plan A9-I0, finding A6-14).
    let picture = original_picture(app, id, page, index);
    let mut original = app.chess.diagram_original;
    let Some(ed) = app.chess.editor.as_mut() else { return };

    // The tab's keys (plan A8, P3c), taken in `raw_input_hook`.
    let mut action = None;
    let mut back_to_list = false;
    for (key, m) in tab_keys {
        match crate::chess_diagram_keys::tab_action(key, m) {
            Some(TabKey::ConfirmNext) => action = Some(EditorAction::Confirm { next: true }),
            Some(TabKey::Previous) if !queue.is_empty() => action = Some(EditorAction::Previous),
            Some(TabKey::Next) if !queue.is_empty() => action = Some(EditorAction::Next),
            Some(TabKey::BackToList) => back_to_list = true,
            Some(TabKey::Original) => original = !original,
            _ => {}
        }
    }
    let dirty = ed.dirty();
    let whole = ui.max_rect();
    let header_rect = egui::Rect::from_min_size(whole.min, egui::vec2(whole.width(), HEADER));
    let footer_rect = egui::Rect::from_min_max(egui::pos2(whole.min.x, (whole.max.y - FOOTER).max(whole.min.y + HEADER)), whole.max);
    let body_rect = egui::Rect::from_min_max(egui::pos2(whole.min.x, header_rect.max.y), egui::pos2(whole.max.x, footer_rect.min.y));

    let header_inner = header_rect.shrink2(egui::vec2(8.0, 4.0));
    let title = diagram_title(&label, index, on_page);
    ui.scope_builder(UiBuilder::new().max_rect(header_inner), |ui| {
        // Measured first (plan A9-I8, §2.8.1): the place in the walk as "3/12" (whole in its
        // tooltip and name), and "‹ Diagrams" as its arrow alone when its word would leave the
        // diagram's title cut short (at 330–360 pt it showed only "…").
        let place_said = walk_place(filter, place, queue.len());
        let place_short = match place {
            Some(k) => format!("{}/{}", k.saturating_add(1), queue.len()),
            None => queue.len().to_string(),
        };
        let small = egui::TextStyle::Small.resolve(ui.style());
        let body = egui::TextStyle::Body.resolve(ui.style());
        let gap = ui.spacing().item_spacing.x;
        let buttons = 3.0 * (24.0 + gap) + widgets::text_width(ui, &place_short, &small) + gap;
        let back_word =
            14.0 + ui.spacing().icon_spacing + widgets::text_width(ui, tl!("Diagrams"), &body) + 2.0 * ui.spacing().button_padding.x + gap;
        let left_fixed = 16.0 + gap + widgets::MIN_TARGET + gap;
        let title_width = widgets::text_width(ui, &title, &body) + 1.0;
        let back_with_word = header_inner.width() - buttons - left_fixed - back_word >= title_width;
        // The buttons first, from the right; the title takes what is left, shortened if need be.
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            {
                ui.add_enabled_ui(!queue.is_empty(), |ui| {
                    let next = if reviewing { tl!("Next to review (Page Down)") } else { tl!("Next diagram (Page Down)") };
                    if crate::icons::button(ui, "chevron-right", 24.0, false, next).clicked() {
                        action = Some(EditorAction::Next);
                    }
                    let r = ui.label(RichText::new(&place_short).small().color(t.text_muted)).on_hover_text(&place_said);
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &place_said));
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
                // Back to the list, the diagram's row taking the keyboard (plan A9-I0a).
                let back = if back_with_word {
                    egui::Button::image_and_text(crate::icons::image("chevron-left", 14.0, t.icon), tl!("Diagrams"))
                } else {
                    egui::Button::image(crate::icons::image("chevron-left", 14.0, t.icon))
                }
                .min_size(egui::vec2(widgets::MIN_TARGET, widgets::MIN_TARGET));
                let back = ui.add(back);
                back.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tl!("Back to Diagrams")));
                if back.on_hover_text(tl!("Back to Diagrams")).clicked() {
                    back_to_list = true;
                }
                state_icon(ui, &t, state, 16.0, dirty);
                // Its text's width (no more than the room left) and 24 points tall, the least a
                // control the pointer clicks may be (plan A9-I15, finding R4-06).
                let font = egui::TextStyle::Body.resolve(ui.style());
                let width = (widgets::text_width(ui, &title, &font) + 1.0).min(ui.available_width());
                let text = egui::Label::new(RichText::new(&title).strong().color(t.accent_text)).truncate().sense(egui::Sense::click());
                let link = ui.add_sized(egui::vec2(width, widgets::MIN_TARGET), text);
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
            if let Some(crate::chess_ui::Reviewed { checked, of }) = review_done {
                ui.horizontal(|ui| {
                    ui.add(crate::icons::image("badge-check", 14.0, t.success_text));
                    let done = crate::i18n::fmt(tl!("Review done: {d} of {n} checked"), &[("d", &checked.to_string()), ("n", &of.to_string())]);
                    ui.label(RichText::new(done).small().strong().color(t.success_text));
                });
            }
            // The first time: what a click does, and that Confirm leaves the PDF alone (P3b3).
            if hint && first_time_hint(ui, &t, board_room(body_rect.width(), ui.spacing().scroll.bar_width)) {
                hint = false;
            }
            // The board is as large as the tab can show it. The room is taken from the tab's own
            // width, never from `ui.available_width()`, which inside the scroll area reports more
            // than the body has (it leaves out the body's side margins): sized by it, the board
            // ran over the tab's right edge and under the scroll bar.
            let room = board_room(body_rect.width(), ui.spacing().scroll.bar_width);
            let side = board_side(room, body_rect.height());
            let sprites = crate::chess_editor::palette_sprites(ui, ed, &pieces);
            // Read or Original, in one row of its own (plan A9-I0, sketch (b)).
            // Its own id: the segments' ids come from their parent's, and the To move switch
            // under the board sits in a row whose id is the same as this one's. Changes not
            // confirmed yet (kept as a draft when another diagram opens) are said at the row's
            // end, its room kept, so the board never moves (plan A9-I8, finding A6-05).
            ui.allocate_ui_with_layout(egui::vec2(room, SWITCH_ROW), Layout::left_to_right(Align::Center), |ui| {
                ui.push_id("chess-diagram-shown", |ui| {
                    widgets::segmented(ui, tl!("Diagram shown"), &mut original, &[(false, tl_ctx!("diagram state", "Read")), (true, tl!("Original"))])
                });
                if dirty {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add(egui::Label::new(RichText::new(tl!("Unconfirmed changes")).small().color(t.warning_text)).truncate());
                        ui.add(crate::icons::image("triangle-alert", 14.0, t.warning_text));
                    });
                }
            })
            .response
            .on_hover_text(tl!("Alt+O switches between the board as read and the page's own picture."));
            ui.horizontal(|ui| {
                ui.add_space(((room - side) / 2.0).max(0.0));
                if original {
                    // The page's picture in the board's square, never beside or under it.
                    if original_view(ui, &t, picture, side, &original_name(&label, index)) {
                        action = Some(EditorAction::ShowOnPage);
                    }
                } else {
                    crate::chess_editor::board_view(
                        ui,
                        &t,
                        ed,
                        &pieces,
                        side,
                        true,
                        &crate::chess_editor::board_name(&label, index),
                        board_keys.as_ref(),
                    );
                }
            });
            ui.add_space(6.0);
            crate::chess_editor::palette(ui, sprites.as_ref(), &mut ed.brush, false);
            ui.add_space(6.0);
            fields(ui, ed);
            // The mate the page asks, while the board shows the position it was solved for.
            if let Some((_, line)) = mate.as_ref().filter(|(fen, _)| crate::chess_ui::same_pieces(&ed.fen(), fen)) {
                ui.add_space(4.0);
                ui.add(egui::Label::new(RichText::new(line).small()).wrap());
            }
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
            let confirm = Confirm::of(kept && !dirty, !queue.is_empty(), confirm_only);
            let gap = ui.spacing().item_spacing.x;
            let more_ways = if confirm.has_menu() { gap + MENU_BUTTON } else { 0.0 };
            let buttons = MENU_BUTTON + gap + widgets::icon_pill_width(ui, confirm.label()) + more_ways;
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
                if let Some(a) = confirm_button(ui, confirm) {
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
        // "‹ Diagrams" or Esc: the list again, the keyboard on the diagram's row; nothing is
        // discarded.
        app.close_chess_detail();
        ui.memory_mut(|m| m.surrender_focus(editor_board));
        app.reveal_chess_diagrams();
    }

    if !hint {
        app.chess.diagram_hint_seen = true;
    }
    app.chess.diagram_original = original;
    if let Some(a) = action {
        app.chess_editor_action(a);
    }
}

/// The first-time hint over the board, `width` points wide (the tab's body, measured before it is
/// drawn: plan A9-I8, finding A6-05), its words wrapping beside the button that puts it away, at
/// the top right corner. Whether it was put away.
fn first_time_hint(ui: &mut egui::Ui, t: &Tokens, width: f32) -> bool {
    const PAD: i8 = 8;
    let mut away = false;
    let inner = (width - 2.0 * f32::from(PAD)).max(0.0);
    let words = (inner - widgets::MIN_TARGET - ui.spacing().item_spacing.x).max(0.0);
    egui::Frame::new().fill(t.accent_soft).corner_radius(6).inner_margin(PAD).show(ui, |ui| {
        ui.set_width(inner);
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(egui::vec2(words, 0.0), Layout::top_down(Align::Min), |ui| {
                ui.set_max_width(words);
                ui.add(
                    egui::Label::new(
                        RichText::new(tl!("Click a square to put the piece there. Confirm keeps the correction without changing the PDF.")).small(),
                    )
                    .wrap(),
                );
            });
            if crate::icons::button(ui, "x", widgets::MIN_TARGET, false, tl!("Dismiss")).clicked() {
                away = true;
            }
        });
    });
    ui.add_space(4.0);
    away
}

/// The page's own picture of diagram `index` of `page` (plan A9-I0, finding A6-14): the texture
/// the page view drew the page with (its whole raster, or the backdrop under its tiles; else its
/// thumbnail) and the diagram's place in it, with its width over its height. Nothing is rendered
/// for it: a page never drawn has no picture yet.
fn original_picture(app: &PdfCraftApp, id: pdfcraft_engine::DocId, page: usize, index: usize) -> Option<(egui::TextureId, egui::Rect, f32)> {
    let view = app.views.iter().find(|v| v.id == id)?;
    let (bbox, _) = app.chess.results_of(id)?.diagram(page, index)?;
    let info = app.session.get(id)?.info.pages.get(page)?;
    let (uv, aspect) = diagram_uv(info.crop, info.rotation, bbox)?;
    let tex = view.page_raster(page).or_else(|| view.thumb_id(page))?;
    Some((tex, uv, aspect))
}

/// Where the user-space box `bbox` of a page with crop box `crop` and `rotation` is on the page as
/// drawn, from 0 to 1 each way from its top left (the picture's UV), and its width over its
/// height as drawn. `None` for an empty or non-finite box, or one off the page.
pub(crate) fn diagram_uv(crop: [f32; 4], rotation: u16, bbox: [f32; 4]) -> Option<(egui::Rect, f32)> {
    let [cx0, cy0, cx1, cy1] = crop;
    let (cw, ch) = (cx1 - cx0, cy1 - cy0);
    if !(cw.is_finite() && ch.is_finite() && cw > 0.0 && ch > 0.0 && bbox.iter().all(|v| v.is_finite())) {
        return None;
    }
    let norm = |x: f32, y: f32| -> egui::Pos2 {
        let (u, v) = ((x - cx0) / cw, (cy1 - y) / ch);
        let (a, b) = match rotation {
            90 => (1.0 - v, u),
            180 => (1.0 - u, 1.0 - v),
            270 => (v, 1.0 - u),
            _ => (u, v),
        };
        egui::pos2(a.clamp(0.0, 1.0), b.clamp(0.0, 1.0))
    };
    let uv = egui::Rect::from_two_pos(norm(bbox[0], bbox[1]), norm(bbox[2], bbox[3]));
    if uv.width() <= 0.0 || uv.height() <= 0.0 {
        return None;
    }
    // In points as drawn: the page's sides swap at a quarter turn.
    let (w, h) = if rotation % 180 == 90 { (uv.width() * ch, uv.height() * cw) } else { (uv.width() * cw, uv.height() * ch) };
    Some((uv, w / h))
}

/// The Original view's name: "Original diagram, page 13, diagram 1".
fn original_name(page: &str, index: usize) -> String {
    crate::i18n::fmt(tl!("Original diagram, page {p}, diagram {n}"), &[("p", page), ("n", &index.saturating_add(1).to_string())])
}

/// The Original view: the page's picture of the diagram in a square `side` points wide, as large
/// as its shape allows, named `name`; without a picture, why, and Show it on the page. Whether
/// that was clicked.
fn original_view(ui: &mut egui::Ui, t: &Tokens, picture: Option<(egui::TextureId, egui::Rect, f32)>, side: f32, name: &str) -> bool {
    let (square, response) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, name));
    let mut show = false;
    match picture {
        Some((tex, uv, aspect)) => {
            let aspect = if aspect.is_finite() && aspect > 0.0 { aspect } else { 1.0 };
            let size = if aspect >= 1.0 { egui::vec2(side, side / aspect) } else { egui::vec2(side * aspect, side) };
            let rect = egui::Rect::from_center_size(square.center(), size);
            ui.painter().image(tex, rect, uv, egui::Color32::WHITE);
            ui.painter().rect_stroke(rect, 0.0, egui::Stroke::new(1.0, t.border), egui::StrokeKind::Outside);
        }
        None => {
            ui.painter().rect_filled(square, 6.0, t.card);
            let inner = square.shrink(16.0);
            ui.scope_builder(UiBuilder::new().max_rect(inner).layout(Layout::top_down(Align::Center)), |ui| {
                ui.add_space((inner.height() / 2.0 - 30.0).max(0.0));
                ui.add(
                    egui::Label::new(
                        RichText::new(tl!("The page's picture of this diagram shows once its page is drawn.")).small().color(t.text_muted),
                    )
                    .wrap(),
                );
                show = ui.button(tl!("Show it on the page")).clicked();
            });
        }
    }
    show
}

/// Nothing to edit: how to get a diagram here.
fn empty(ui: &mut egui::Ui, t: &Tokens) {
    ui.add_space(16.0);
    ui.vertical_centered(|ui| {
        ui.add(crate::icons::image("square-dashed-mouse-pointer", 28.0, t.text_faint));
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
        widgets::segmented(ui, tl!("To move"), &mut side, &[(Color::White, tl!("White")), (Color::Black, tl_ctx!("chess side", "Black"))]);
        ed.position.side_to_move = Some(side);
    });
    ui.horizontal(|ui| {
        let named = row_label(ui, tl!("Move number:"));
        ui.add(egui::DragValue::new(&mut ed.position.fullmove_number).range(1..=crate::chess_editor::MAX_MOVE_NUMBER).speed(0.2))
            .labelled_by(named.id)
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
fn row_label(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let width = widgets::text_width(ui, text, &font);
    ui.add_sized(egui::vec2(width, ROW), egui::Label::new(text))
}

/// The height of a row of buttons in the tab, in points.
const ROW: f32 = 26.0;

/// The side of the footer's ⋯ and ▾ buttons, in points.
const MENU_BUTTON: f32 = 26.0;

/// What the main button does (plan A9-I15, finding R4-08; the words are plan A9 §2.8.2's).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Confirm {
    /// The diagram keeps this very position: Confirmed, greyed out until the position changes.
    Done,
    /// No review queue to go on to: Confirm, with no ▾.
    Alone,
    /// Confirm and next (Ctrl+Enter) or, when `only`, Only confirm in front, with ▾ for the other.
    Queue { only: bool },
}

impl Confirm {
    /// The button for a diagram whose kept position is the one shown (`done`), with diagrams to
    /// review (`queue`), the user having last chosen Only confirm (`only`).
    pub(crate) fn of(done: bool, queue: bool, only: bool) -> Self {
        match (done, queue) {
            (true, _) => Self::Done,
            (false, false) => Self::Alone,
            (false, true) => Self::Queue { only },
        }
    }

    /// The main button's words.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Done => tl!("Confirmed"),
            Self::Alone => tl!("Confirm"),
            Self::Queue { only: true } => tl!("Only confirm"),
            Self::Queue { only: false } => tl!("Confirm and next"),
        }
    }

    /// Whether a ▾ beside it offers the other way to confirm.
    pub(crate) fn has_menu(self) -> bool {
        matches!(self, Self::Queue { .. })
    }
}

/// The main button, and with a review queue its ▾: Confirm and next (Ctrl+Enter) or Only
/// confirm, the last one chosen in front ([`Confirm`]).
pub(crate) fn confirm_button(ui: &mut egui::Ui, confirm: Confirm) -> Option<EditorAction> {
    let mut action = None;
    let next = confirm == Confirm::Queue { only: false };
    let hint = match confirm {
        Confirm::Done => tl!("This position is kept for the diagram. Change it to confirm again."),
        Confirm::Queue { only: false } => {
            tl!("Keep this position for the diagram, without changing the PDF, and open the next one to review (Ctrl+Enter).")
        }
        Confirm::Alone | Confirm::Queue { only: true } => tl!("Keep this position for the diagram, without changing the PDF."),
    };
    let done = confirm == Confirm::Done;
    let main = ui.add_enabled_ui(!done, |ui| widgets::icon_pill(ui, "badge-check", confirm.label(), !done)).inner;
    if main.on_hover_text(hint).on_disabled_hover_text(hint).clicked() {
        action = Some(EditorAction::Confirm { next });
    }
    if confirm.has_menu() {
        let more = crate::icons::button(ui, "chevron-down", MENU_BUTTON, false, tl!("More ways to confirm"));
        egui::Popup::menu(&more).show(|ui| {
            if widgets::menu_entry(ui, true, egui::Button::new(tl!("Confirm and next"))).clicked() {
                action = Some(EditorAction::Confirm { next: true });
            }
            if widgets::menu_entry(ui, true, egui::Button::new(tl!("Only confirm"))).clicked() {
                action = Some(EditorAction::Confirm { next: false });
            }
        });
    }
    action
}

/// The other actions, behind ⋯.
fn more_menu(ui: &mut egui::Ui, ed: &DiagramEditor, legal: bool, kept: bool) -> Option<EditorAction> {
    let mut action = None;
    let button = crate::icons::button(ui, "ellipsis", MENU_BUTTON, false, tl!("More actions"));
    egui::Popup::menu(&button).show(|ui| {
        let fen = ed.fen();
        // Each item closes the menu, whatever clicked it (plan A9-I15, finding A6-18).
        let item = |ui: &mut egui::Ui, enabled: bool, text: &str| widgets::menu_entry(ui, enabled, egui::Button::new(text));
        if item(ui, true, tl!("Redraw in the PDF…"))
            .on_hover_text(tl!("Draw this position over the original diagram in the PDF (one undo step)."))
            .clicked()
        {
            action = Some(EditorAction::Redraw);
        }
        if item(ui, legal, tl!("Add lichess link")).clicked() {
            action = Some(EditorAction::Link);
        }
        if item(ui, true, tl!("Copy FEN")).on_hover_text(tl!("Position code other chess programs accept.")).clicked() {
            action = Some(EditorAction::CopyFen(fen.clone()));
        }
        if legal {
            let url =
                pdfcraft_engine::chess::pdfcraft_chess::lichess::analysis_url(&ed.position, if ed.flipped { Color::Black } else { Color::White });
            if item(ui, true, tl!("Open in lichess")).on_hover_text(&url).clicked() {
                action = Some(EditorAction::OpenLichess(url));
            }
        }
        if kept {
            // Last, apart, in the danger colour: it drops what the user kept (plan A9 §2.8.1).
            ui.separator();
            let danger = Tokens::get(ui.ctx()).danger_text;
            let forget = widgets::menu_entry(ui, true, egui::Button::new(RichText::new(tl!("Forget the correction")).color(danger)));
            if forget.on_hover_text(tl!("Show what was read again; the PDF is not changed.")).clicked() {
                action = Some(EditorAction::Forget);
            }
        }
    });
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plan A9-N4: the mate the page asks, in one line. "(unique)" only for one key proved; the
    /// keys of a cook; no mate with the best try and the defence that refutes it, Black's moves
    /// written "1...".
    #[test]
    fn the_mate_line_says_what_the_solver_found() {
        let report = |side, keys: &[&str], shortest, exhausted, best: Option<&str>, reply: Option<&str>, castles| {
            MateFinding::Solved(MateReport {
                n: 2,
                side,
                keys: keys.iter().map(|k| (*k).to_owned()).collect(),
                shortest,
                nodes: 1,
                exhausted,
                best_try: best.map(str::to_owned),
                refutation: reply.map(str::to_owned),
                castles,
            })
        };
        assert_eq!(mate_line(&report(Color::White, &["Qf7"], Some(2), false, None, None, false)), "Mate in 2: 1.Qf7! (unique)");
        assert_eq!(
            mate_line(&report(Color::White, &["O-O"], Some(1), false, None, None, true)),
            "Mate in 2: 1.O-O! (unique) · mates in 1 · supposes castling is allowed"
        );
        assert_eq!(mate_line(&report(Color::Black, &["Qf2", "Qg1"], Some(2), false, None, None, false)), "Mate in 2: 1...Qf2!, 1...Qg1! (2 keys)");
        assert_eq!(
            mate_line(&report(Color::White, &[], None, false, Some("Qc6"), Some("Ka3"), false)),
            "Mate in 2: none found; best try 1.Qc6, refuted by 1...Ka3"
        );
        assert_eq!(mate_line(&report(Color::White, &[], None, true, None, None, false)), "Mate in 2: no key found within the search budget");
        // A key found before the budget ran out is not proved the only one.
        assert_eq!(mate_line(&report(Color::White, &["Qf7"], Some(2), true, None, None, false)), "Mate in 2: 1.Qf7! (not proved the only key)");
        assert_eq!(mate_line(&MateFinding::BothSides { n: 1 }), "Mate in 1: both sides mate, so who moves stays assumed");
    }

    /// Plan A9-I0 (finding A6-14): the diagram's place on the page as drawn (the picture's UV),
    /// with the page turned as `/Rotate` says, and its shape; nothing for a box that isn't one.
    #[test]
    fn the_original_is_the_diagrams_place_on_the_page_as_drawn() {
        let crop = [0.0, 0.0, 600.0, 800.0];
        let bbox = [60.0, 580.0, 220.0, 740.0];
        let near = |a: egui::Rect, b: egui::Rect| (a.min - b.min).length() < 1e-4 && (a.max - b.max).length() < 1e-4;
        let (uv, aspect) = diagram_uv(crop, 0, bbox).unwrap();
        assert!(near(uv, egui::Rect::from_min_max(egui::pos2(0.1, 0.075), egui::pos2(220.0 / 600.0, 0.275))), "{uv:?}");
        assert!((aspect - 1.0).abs() < 1e-4);
        // A quarter turn: the page drawn landscape, the box where the turn puts it, still square.
        let (uv, aspect) = diagram_uv(crop, 90, bbox).unwrap();
        assert!(near(uv, egui::Rect::from_min_max(egui::pos2(0.725, 0.1), egui::pos2(0.925, 220.0 / 600.0))), "{uv:?}");
        assert!((aspect - 1.0).abs() < 1e-4);
        // A wide box keeps its shape.
        assert!((diagram_uv(crop, 0, [0.0, 0.0, 300.0, 100.0]).unwrap().1 - 3.0).abs() < 1e-4);
        assert!((diagram_uv(crop, 270, [0.0, 0.0, 300.0, 100.0]).unwrap().1 - 1.0 / 3.0).abs() < 1e-4);
        for bad in [[0.0, 0.0, 0.0, 0.0], [f32::NAN, 0.0, 10.0, 10.0], [700.0, 900.0, 800.0, 1000.0]] {
            assert!(diagram_uv(crop, 0, bad).is_none(), "{bad:?}");
        }
        assert!(diagram_uv([0.0, 0.0, 0.0, 800.0], 0, bbox).is_none(), "an empty page");
    }

    /// The board never takes more than the tab leaves it: the margins and the scroll bar come
    /// off, and a tab too narrow for [`MIN_BOARD`] gets a smaller board, never one over its edge.
    #[test]
    fn the_board_never_takes_more_than_the_tab_leaves_it() {
        // The tab as it opens (320 pt): the body's two margins and the bar come off.
        assert_eq!(board_room(320.0, 8.0), 296.0);
        assert_eq!(board_side(296.0, 700.0), 296.0);
        // A tab narrower than the least board: the board shrinks with it.
        let narrow = board_room(220.0, 8.0);
        assert_eq!(narrow, 196.0);
        assert_eq!(board_side(narrow, 700.0), 196.0);
        // A short tab: the height decides, and the least board still fits the room.
        assert_eq!(board_side(296.0, 380.0), 200.0);
        assert!(board_side(140.0, 380.0) <= 140.0);
        // A wide tab stops at the largest board.
        assert_eq!(board_side(900.0, 2000.0), MAX_BOARD);
        // Nothing the layout can give makes a negative or unreal side.
        for odd in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -100.0, 0.0, 10.0] {
            let room = board_room(odd, 8.0);
            assert!(room >= 0.0 && room.is_finite(), "room of {odd}: {room}");
            for height in [f32::NAN, f32::INFINITY, -50.0, 0.0, 240.0, 5000.0] {
                let side = board_side(room, height);
                assert!(side >= 0.0 && side.is_finite() && side <= room.max(0.0), "{odd} by {height}: {side} in {room}");
            }
        }
    }
}
