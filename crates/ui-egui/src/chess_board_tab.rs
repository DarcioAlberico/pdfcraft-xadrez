//! Chess ▸ the Board tab in fixed regions (plan A9-I0b, §2.8.0 sketch (d); findings A6-12, A6-24,
//! R8-07): the board never scrolls. Top to bottom, each region measured before anything is drawn
//! ([`Regions`]):
//!
//! - a header of [`HEADER`] points: the move shown and whose move it is, then Analyze, the Move
//!   list beside the page, the board's theme and the board's menu (⋯: the engine's lines and
//!   program, clearing the drawings, the diagram the board came from);
//! - the board, as large as the room the other regions leave allows (with the evaluation bar
//!   beside it while the engine analyses);
//! - the navigation row ([`NAV`]): start, back, forward, end, the variations before and after, the
//!   board turned round, and at its end the engine's state or the material;
//! - the engine's lines while it analyses, a row of [`ENGINE_LINE`] each (up to
//!   [`MAX_ENGINE_LINES`]), cut short with "…" and whole in their tooltips;
//! - the trail ([`TRAIL`]): where in the tree the move shown is, the continuations at a fork, or
//!   the diagram the board came from;
//! - the moves, filling the rest (at least [`ROWS`] rows of them, [`ROWS_WITH_ENGINE`] with the
//!   engine's lines), their own scroll area the only one;
//! - a bar of [`BAR`] points at the foot: **Game ▾ · Comment · FEN/PGN ▾**, which took the place
//!   of the sections under the list. The first move's number is in Game ▾ (§2.8.1); the game's
//!   details, the document's games and the FEN and PGN fields take the moves' place
//!   ([`MovesView`]: "‹ Moves" and Esc go back); Comment opens the comment fields under the moves.
//!
//! Captures from the clipboard and the layer of captured text went to the Capture tool's popup
//! on the rail (`chess_capture::popup_settings`). The chess window draws the same regions, in
//! one column or the board's beside the moves' ([`Regions::beside`]).

use egui::{Rect, pos2, vec2};
use pdfcraft_engine::chess::pdfcraft_chess::draw::PieceSet;
use pdfcraft_engine::chess::pdfcraft_chess::placement::Color;

use crate::chess_board_ui::{self, BoardAction, Sizing};
use crate::chess_collection::GamesAction;
use crate::chess_tools::PgnTool;
use crate::chess_ui::ChessState;
use crate::theme::Tokens;

/// The header's height, in points.
pub(crate) const HEADER: f32 = 28.0;
/// The navigation row's height.
pub(crate) const NAV: f32 = 28.0;
/// One engine line's height.
pub(crate) const ENGINE_LINE: f32 = 20.0;
/// Most engine lines shown (the engine is asked for 1 to 3).
pub(crate) const MAX_ENGINE_LINES: usize = 3;
/// The trail's height.
pub(crate) const TRAIL: f32 = 20.0;
/// The bar's height.
pub(crate) const BAR: f32 = 24.0;
/// Least rows of moves without the engine's lines, and with them (§2.8.0 sketch (d)).
pub(crate) const ROWS: usize = 10;
pub(crate) const ROWS_WITH_ENGINE: usize = 7;
/// The smallest board drawn while the column holds the other regions at all, in points.
const SMALLEST_BOARD: f32 = 96.0;
/// What the moves' region keeps besides its rows: the game's header line and the gaps.
const MOVES_SLACK: f32 = 8.0;
/// The moves' rows in the Board tab are compact (plan A9-I0b): a move's button has this much
/// room above and below its text and is at least this tall, so ten rows take the 160–230 pt the
/// sketch gives them (the panel's 24 pt buttons made them 29 pt each, and the board 96 pt at
/// 1366 × 768). The moves are notation, set like text.
const ROW_PAD: f32 = 2.0;
const ROW_LEAST: f32 = 18.0;

/// What the moves' region shows (plan A9-I0b): the moves, or in their place the document's
/// games, the game's details, or the FEN and PGN fields.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MovesView {
    #[default]
    Moves,
    Games,
    Details,
    Fields,
}

/// The Board tab's regions, measured before anything is drawn (plan A9-I0b, §2.8.1 "Medir antes
/// de desenhar"). Every rectangle is finite; none is taller than the room it was cut from, except
/// that a column too short for the fixed regions lets them overlap at its foot rather than run
/// off it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Regions {
    pub(crate) header: Rect,
    /// The board's square.
    pub(crate) board: Rect,
    /// The evaluation bar's room left of the board, while the engine analyses.
    pub(crate) eval: Option<Rect>,
    pub(crate) nav: Rect,
    /// The engine's rows (empty while it does not analyse).
    pub(crate) engine: Rect,
    pub(crate) trail: Rect,
    pub(crate) moves: Rect,
    pub(crate) bar: Rect,
    /// Where the set-up view goes while the rail's Set up tool is armed: all under the header in
    /// one column, the board's side of two.
    pub(crate) setup: Rect,
}

/// `v`, or 0 when it is not a number.
fn finite(v: f32) -> f32 {
    if v.is_finite() { v } else { 0.0 }
}

/// A band `h` points tall and `w` wide at `(x, *y)`; `*y` moves under it.
fn band(x: f32, y: &mut f32, w: f32, h: f32) -> Rect {
    let r = Rect::from_min_size(pos2(x, *y), vec2(w.max(0.0), h.max(0.0)));
    *y += h.max(0.0);
    r
}

/// The board's band at `(x, *y)`: the evaluation bar's `eval` points, then the board `side`
/// points square; `*y` moves under it.
fn board_band(x: f32, y: &mut f32, side: f32, eval: f32) -> (Rect, Option<Rect>) {
    let board = Rect::from_min_size(pos2(x + eval, *y), vec2(side, side));
    let bar = (eval > 0.0).then(|| Rect::from_min_size(pos2(x, *y), vec2(eval, side)));
    *y += side;
    (board, bar)
}

impl Regions {
    /// The engine rows' height for `lines` lines (0 while it does not analyse).
    pub(crate) fn engine_height(lines: usize) -> f32 {
        lines.min(MAX_ENGINE_LINES) as f32 * ENGINE_LINE
    }

    /// The least height of the moves, `row` points a row: [`ROWS`], or [`ROWS_WITH_ENGINE`] with
    /// the engine's lines, and the region's own gaps.
    pub(crate) fn least_moves(lines: usize, row: f32) -> f32 {
        let rows = if lines > 0 { ROWS_WITH_ENGINE } else { ROWS };
        rows as f32 * finite(row).clamp(8.0, 200.0) + MOVES_SLACK
    }

    /// The board's side: as wide as `across` allows and as tall as `down`, at most `limit`, never
    /// below the smallest board unless `across` or the `room` for the board and the moves is less.
    fn side(across: f32, down: f32, room: f32, limit: f32) -> f32 {
        let (across, down, room, limit) = (finite(across).max(0.0), finite(down), finite(room).max(0.0), finite(limit).max(0.0));
        let smallest = SMALLEST_BOARD.min(across).min(room);
        across.min(down).min(limit).max(smallest).max(0.0)
    }

    /// The regions of one column `rect`, with `lines` engine lines (0–3), moves `row` points a row,
    /// the evaluation bar's `eval` points beside the board, and the board at most `limit` points.
    pub(crate) fn column(rect: Rect, lines: usize, row: f32, eval: f32, limit: f32) -> Regions {
        let (x, top) = (finite(rect.min.x), finite(rect.min.y));
        let (w, h) = (finite(rect.width()).max(0.0), finite(rect.height()).max(0.0));
        let engine = Self::engine_height(lines);
        let room = (h - HEADER - NAV - engine - TRAIL - BAR).max(0.0);
        let eval = finite(eval).clamp(0.0, w);
        let side = Self::side(w - eval, room - Self::least_moves(lines, row), room, limit);
        let mut y = top;
        let header = band(x, &mut y, w, HEADER);
        let setup = Rect::from_min_max(pos2(x, y), pos2(x + w, (top + h).max(y)));
        let (board, eval) = board_band(x, &mut y, side, eval);
        let nav = band(x, &mut y, w, NAV);
        let engine = band(x, &mut y, w, engine);
        let trail = band(x, &mut y, w, TRAIL);
        let bar_top = (top + h - BAR).max(y);
        let moves = Rect::from_min_max(pos2(x, y), pos2(x + w, bar_top));
        let bar = Rect::from_min_size(pos2(x, bar_top), vec2(w, BAR));
        Regions { header, board, eval, nav, engine, trail, moves, bar, setup }
    }

    /// The regions of two columns: the header, the board, the navigation and the engine's lines in
    /// `left`, the trail, the moves and the bar in `right` (the chess window when wide; while its
    /// column shows another tab, `right` is empty and so are they).
    pub(crate) fn beside(left: Rect, right: Rect, lines: usize, eval: f32, limit: f32) -> Regions {
        let (x, top) = (finite(left.min.x), finite(left.min.y));
        let (w, h) = (finite(left.width()).max(0.0), finite(left.height()).max(0.0));
        let engine = Self::engine_height(lines);
        let room = (h - HEADER - NAV - engine).max(0.0);
        let eval = finite(eval).clamp(0.0, w);
        let side = Self::side(w - eval, room, room, limit);
        let mut y = top;
        let header = band(x, &mut y, w, HEADER);
        let setup = Rect::from_min_max(pos2(x, y), pos2(x + w, (top + h).max(y)));
        let (board, eval) = board_band(x, &mut y, side, eval);
        let nav = band(x, &mut y, w, NAV);
        let engine = band(x, &mut y, w, engine);
        let (rx, rtop) = (finite(right.min.x), finite(right.min.y));
        let (rw, rh) = (finite(right.width()).max(0.0), finite(right.height()).max(0.0));
        let mut ry = rtop;
        let trail = band(rx, &mut ry, rw, TRAIL);
        let bar_top = (rtop + rh - BAR).max(ry);
        let moves = Rect::from_min_max(pos2(rx, ry), pos2(rx + rw, bar_top));
        let bar = Rect::from_min_size(pos2(rx, bar_top), vec2(rw, BAR));
        Regions { header, board, eval, nav, engine, trail, moves, bar, setup }
    }
}

/// One row of the moves as the list draws it at its text size (plan HC9), in points: the
/// buttons' height and the gap under them.
pub(crate) fn row_height(ui: &egui::Ui) -> f32 {
    let look = crate::chess_figurines::list_look(ui.ctx());
    let k = f32::from(look.size.percent()) / 100.0;
    let text = ui.text_style_height(&egui::TextStyle::Monospace) * k;
    (text + 2.0 * ROW_PAD).max(ROW_LEAST) + 2.0
}

/// Draw into `rect` with a left-to-right layout centred across it, clipped to it.
pub(crate) fn in_row<R>(ui: &mut egui::Ui, rect: Rect, salt: impl std::hash::Hash + std::fmt::Debug, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let mut child = ui.new_child(egui::UiBuilder::new().id_salt(salt).max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    add(&mut child)
}

/// Draw into `rect` top to bottom, clipped to it.
fn in_column<R>(ui: &mut egui::Ui, rect: Rect, salt: impl std::hash::Hash + std::fmt::Debug, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let mut child = ui.new_child(egui::UiBuilder::new().id_salt(salt).max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)));
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    add(&mut child)
}

/// What the Board tab asked the app for.
#[derive(Default)]
pub(crate) struct Asked {
    pub(crate) board: Option<BoardAction>,
    pub(crate) games: Option<GamesAction>,
}

/// The engine lines the regions make room for: the lines asked for while the engine analyses
/// (whether or not it has answered yet, so its rows never push the moves about), else none.
fn engine_lines(chess: &ChessState) -> usize {
    if chess.study.analysis { usize::try_from(chess.study.lines).unwrap_or(1).clamp(1, MAX_ENGINE_LINES) } else { 0 }
}

/// The Board tab in one column, `rect` (the Chess column's Board tab, a narrow chess window).
pub(crate) fn column(
    ui: &mut egui::Ui,
    t: &Tokens,
    chess: &mut ChessState,
    pieces: &PieceSet,
    source_label: Option<&str>,
    sizing: Sizing,
    rect: Rect,
) -> Asked {
    let eval = crate::chess_study_ui::eval_room(ui, &chess.study);
    let regions = Regions::column(rect, engine_lines(chess), row_height(ui), eval, sizing.side_limit());
    let asked = draw(ui, t, chess, pieces, source_label, sizing, &regions, Arrangement::Column);
    ui.advance_cursor_after_rect(rect);
    asked
}

/// The Board tab in two columns, the board's `left` and the moves' `right` (a wide chess window,
/// plan A9-I0c); with no `right` (the window's column shows its Diagrams or Text tab), the board's
/// regions alone.
#[allow(clippy::too_many_arguments)]
pub(crate) fn beside(
    ui: &mut egui::Ui,
    t: &Tokens,
    chess: &mut ChessState,
    pieces: &PieceSet,
    source_label: Option<&str>,
    sizing: Sizing,
    left: Rect,
    right: Option<Rect>,
) -> Asked {
    let eval = crate::chess_study_ui::eval_room(ui, &chess.study);
    let empty = Rect::from_min_size(left.left_bottom(), vec2(0.0, 0.0));
    let regions = Regions::beside(left, right.unwrap_or(empty), engine_lines(chess), eval, sizing.side_limit());
    let asked =
        draw(ui, t, chess, pieces, source_label, sizing, &regions, if right.is_some() { Arrangement::Beside } else { Arrangement::BoardOnly });
    ui.advance_cursor_after_rect(right.map_or(left, |r| left.union(r)));
    asked
}

/// How the regions are laid out: in one column, the moves beside the board, or the board alone.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Arrangement {
    Column,
    Beside,
    BoardOnly,
}

/// Where the Board tab's regions are, for the layout's tests: header, board, navigation, engine,
/// trail, moves, bar.
pub const REGIONS_ID: &str = "pdfcraft-chess-board-regions";

#[allow(clippy::too_many_arguments)]
fn draw(
    ui: &mut egui::Ui,
    t: &Tokens,
    chess: &mut ChessState,
    pieces: &PieceSet,
    source_label: Option<&str>,
    sizing: Sizing,
    r: &Regions,
    layout: Arrangement,
) -> Asked {
    let mut asked = Asked::default();
    ui.data_mut(|d| d.insert_temp(egui::Id::new(REGIONS_ID), [r.header, r.board, r.nav, r.engine, r.trail, r.moves, r.bar]));
    asked.board = in_row(ui, r.header, "chess-tab-header", |ui| header(ui, t, chess, source_label));
    // The fields are filled from the game whatever shows (the PGN only when the game changed),
    // and before any field is drawn, which one has the keyboard, for the glyph palette and the
    // rail's glyph buttons (whether or not a field of this tab shows: a capture's correction
    // field is one).
    let state = chess.board_mut();
    state.refresh_fields(false);
    state.typing.track(ui);
    // The set-up view (the rail's Set up tool) in the place of the board, and in one column of
    // everything under the header: it is taller than the board, so it scrolls.
    let setting_up = if chess.tools.tool == PgnTool::Setup {
        in_column(ui, r.setup, "chess-tab-setup", |ui| {
            egui::ScrollArea::vertical()
                .id_salt("chess-tab-setup-scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| crate::chess_study_ui::setup_in_place(ui, t, chess, pieces))
                .inner
        })
    } else {
        // Nothing armed: it draws nothing, and notes that the board is drawn.
        crate::chess_study_ui::setup_in_place(ui, t, chess, pieces)
    };
    if !setting_up {
        let (study, games) = (&chess.study, &mut chess.games);
        crate::chess_study_ui::board_at(ui, t, study, games.board_mut(), pieces, sizing, r.board, r.eval);
        in_row(ui, r.nav, "chess-tab-nav", |ui| nav_row(ui, t, chess, pieces));
        if r.engine.height() > 0.0 {
            engine_rows(ui, t, chess, r.engine);
        }
    }
    if (setting_up && layout == Arrangement::Column) || layout == Arrangement::BoardOnly {
        return asked;
    }
    if let Some(a) = in_row(ui, r.trail, "chess-tab-trail", |ui| trail_row(ui, t, chess, source_label)) {
        asked.board = asked.board.or(Some(a));
    }
    let (board, games) = in_column(ui, r.moves, "chess-tab-moves", |ui| moves_region(ui, t, chess, r.moves));
    asked.board = asked.board.or(board);
    asked.games = games;
    if let Some(a) = in_row(ui, r.bar, "chess-tab-bar", |ui| bar(ui, t, chess)) {
        asked.board = asked.board.or(Some(a));
    }
    asked
}

/// The header: the move shown and whose move it is (cut short with "…", whole in its tooltip and
/// accessible name), and from the right the board's menu, its theme, the Move list beside the
/// page and Analyze.
fn header(ui: &mut egui::Ui, t: &Tokens, chess: &mut ChessState, source_label: Option<&str>) -> Option<BoardAction> {
    let mut action = None;
    ui.spacing_mut().item_spacing.x = 4.0;
    ui.spacing_mut().button_padding.y = 2.0;
    let (text, colour, dot) = {
        let state = chess.board();
        let (status, colour) = chess_board_ui::status(state, t);
        let current = state.game.current();
        let text = if state.illegal.is_none() && current != 0 {
            format!("{} · {status}", crate::chess_tools::move_name(&state.game, current))
        } else {
            status
        };
        let dot = state.illegal.is_none().then(|| state.game.side_to_move() == Color::White);
        (text, colour, dot)
    };
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let name = tl!("More board actions");
        let menu = egui::containers::menu::MenuButton::from_button(
            egui::Button::image(crate::icons::image("ellipsis", 16.0, t.icon)).min_size(vec2(HEADER, HEADER - 4.0)),
        );
        let (r, picked) = menu.ui(ui, |ui| board_menu(ui, t, chess, source_label));
        r.on_hover_text(name).widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
        if let Some(a) = picked.and_then(|p| p.inner) {
            action = Some(a);
        }
        let theme_name = tl!("Board theme");
        // Open until a click outside it: the look's choices are tried in place, and the last
        // move's list (a combo box, not a submenu) would close it on opening. A theme picked
        // closes it.
        let config = egui::containers::menu::MenuConfig::new().close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
        let menu = egui::containers::menu::MenuButton::from_button(
            egui::Button::image(crate::icons::image("palette", 14.0, t.icon)).min_size(vec2(HEADER, HEADER - 4.0)),
        )
        .config(config);
        let (r, picked) = menu.ui(ui, crate::chess_theme::menu_items);
        let r = r.on_hover_text(theme_name);
        r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, theme_name));
        match picked.and_then(|p| p.inner) {
            Some(crate::chess_theme::MenuPick::Theme(theme)) => action = Some(BoardAction::BoardTheme(theme)),
            Some(crate::chess_theme::MenuPick::Look(look)) => action = Some(BoardAction::BoardLook(look)),
            None => {}
        }
        let beside = chess.moves_beside;
        let name = tl!("Move list beside the page");
        let r = ui
            .add(egui::Button::image(crate::icons::image("list-ordered", 14.0, t.icon)).selected(beside).min_size(vec2(HEADER, HEADER - 4.0)))
            .on_hover_text(name);
        r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, beside, name));
        if r.clicked() {
            action = Some(BoardAction::ToggleMoveList);
        }
        let analysing = chess.study.analysis;
        let name = if analysing { tl!("Stop the analysis") } else { tl!("Analyze") };
        let r = ui
            .add(egui::Button::image(crate::icons::image("gauge", 14.0, t.icon)).selected(analysing).min_size(vec2(HEADER, HEADER - 4.0)))
            .on_hover_text(name);
        r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, analysing, name));
        if r.clicked() {
            action = Some(BoardAction::Analyze);
        }
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            // The side to move's dot, as on the board's edge: white or black, ringed in the other,
            // so it shows on either theme.
            if let Some(white) = dot {
                let (spot, _) = ui.allocate_exact_size(vec2(10.0, 10.0), egui::Sense::hover());
                let (fill, ring) = if white { (egui::Color32::WHITE, egui::Color32::BLACK) } else { (egui::Color32::BLACK, egui::Color32::WHITE) };
                ui.painter().circle(spot.center(), 4.0, fill, egui::Stroke::new(1.0, ring));
            }
            ui.add(egui::Label::new(egui::RichText::new(&text).color(colour)).truncate());
        });
    });
    action
}

/// The board's menu (⋯): the engine's lines and program, the drawings cleared (and how they are
/// made), and the diagram the board came from.
fn board_menu(ui: &mut egui::Ui, t: &Tokens, chess: &mut ChessState, source_label: Option<&str>) -> Option<BoardAction> {
    let mut action = None;
    ui.set_max_width(300.0);
    #[cfg(not(target_arch = "wasm32"))]
    {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(tl!("Lines:")).small());
            for n in 1..=3u32 {
                if ui.selectable_label(chess.study.lines == n, n.to_string()).clicked() {
                    chess.study.lines = n;
                }
            }
        });
        let pick = egui::Button::new(tl!("Choose engine…"));
        if crate::widgets::menu_entry(ui, true, pick).on_hover_text(tl!("Pick a UCI chess engine program, such as Stockfish.")).clicked() {
            action = Some(BoardAction::ChooseEngine);
        }
        ui.separator();
    }
    let board = chess.board_mut();
    if crate::widgets::menu_entry(ui, !board.marks().is_empty(), egui::Button::new(tl!("Clear drawings"))).clicked() {
        board.clear_marks();
    }
    ui.label(egui::RichText::new(crate::chess_study_ui::drawing_help()).small().color(t.text_faint));
    if let (Some(label), Some(source)) = (source_label, board.source) {
        ui.separator();
        if crate::widgets::menu_entry(ui, true, egui::Button::new(label)).on_hover_text(tl!("Show this diagram on the page")).clicked() {
            action = Some(BoardAction::ShowSource(source));
        }
    }
    action
}

/// The navigation row: the pawn's promotion while one waits, else the moves' buttons, and at the
/// row's end the engine's state while it analyses, else the material.
fn nav_row(ui: &mut egui::Ui, t: &Tokens, chess: &mut ChessState, pieces: &PieceSet) {
    ui.spacing_mut().item_spacing.x = 2.0;
    if chess.board().promotion.is_some() {
        chess_board_ui::promotion_choice(ui, chess.board_mut());
        return;
    }
    chess_board_ui::nav_buttons(ui, t, chess.board_mut());
    let rest = ui.available_rect_before_wrap();
    if rest.width() < 16.0 {
        return;
    }
    let mut end =
        ui.new_child(egui::UiBuilder::new().id_salt("chess-nav-end").max_rect(rest).layout(egui::Layout::right_to_left(egui::Align::Center)));
    end.set_clip_rect(rest.intersect(ui.clip_rect()));
    #[cfg(not(target_arch = "wasm32"))]
    if let Some((status, searching)) = crate::chess_study_ui::engine_status(&chess.study, chess.board()) {
        end.add(egui::Label::new(egui::RichText::new(status).small().color(t.text_muted)).truncate());
        if searching {
            crate::chess_study_ui::searching(&mut end);
        }
        return;
    }
    crate::chess_study_ui::material_strip(&mut end, t, chess.board_mut(), pieces, rest.width());
}

/// The engine's rows while it analyses (or why it can't).
fn engine_rows(ui: &mut egui::Ui, t: &Tokens, chess: &mut ChessState, rect: Rect) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let (study, games) = (&chess.study, &mut chess.games);
        crate::chess_study_ui::engine_rows(ui, t, study, games.board_mut(), rect);
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = chess;
        let row = Rect::from_min_size(rect.min, vec2(rect.width(), ENGINE_LINE));
        in_row(ui, row, "chess-engine-web", |ui| {
            ui.add(egui::Label::new(egui::RichText::new(tl!("Engine analysis needs the desktop app.")).small().color(t.text_muted)).truncate());
        });
    }
}

/// The trail's row: the tree's menu while the game has variations, then the continuations at a
/// fork, else the trail to the move shown in a variation, else the diagram the board came from
/// (cut short with "…", whole in its tooltip and accessible name).
fn trail_row(ui: &mut egui::Ui, t: &Tokens, chess: &mut ChessState, source_label: Option<&str>) -> Option<BoardAction> {
    use crate::chess_moves_tree as tree;
    ui.spacing_mut().item_spacing.x = 4.0;
    let state = chess.board_mut();
    let mut action = None;
    let link = |ui: &mut egui::Ui, room: f32, label: &str| {
        let font = egui::TextStyle::Small.resolve(ui.style());
        let shown = crate::widgets::fit_line(label, room, |s| crate::widgets::text_width(ui, s, &font));
        let r = ui.link(egui::RichText::new(&shown).small()).on_hover_text(tl!("Show this diagram on the page"));
        r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Link, true, label));
        if shown != label {
            r.ctx.accesskit_node_builder(r.id, |node| node.set_description(label));
        }
        r.clicked()
    };
    // A position that is not legal has no moves: the diagram it came from, to correct it.
    if state.illegal.is_some() {
        if let (Some(label), Some(source)) = (source_label, state.source)
            && link(ui, ui.available_width(), label)
        {
            action = Some(BoardAction::ShowSource(source));
        }
        return action;
    }
    let tokens = state.tokens();
    let mut asked = None;
    if tree::has_variations(tokens.as_slice()) {
        asked = tree::tree_menu(ui, t, state);
    }
    let room = ui.available_width();
    if tree::continuations(ui, t, state, room) {
        // The fork's replies say where a step forward goes.
    } else if !tree::variations_holding(&state.game, state.game.current()).is_empty() {
        if let Some(a) = tree::trail(ui, t, state, room) {
            asked = Some(a);
        }
    } else if let (Some(label), Some(source)) = (source_label, state.source)
        && link(ui, room, label)
    {
        action = Some(BoardAction::ShowSource(source));
    }
    tree::apply(state, asked);
    action
}

/// The moves' region: what [`MovesView`] says, the moves by default (and the document's games
/// while the moves show in the Move list beside the page).
fn moves_region(ui: &mut egui::Ui, t: &Tokens, chess: &mut ChessState, rect: Rect) -> (Option<BoardAction>, Option<GamesAction>) {
    // A click on the game's header (plan HC10) opens its details in the moves' place.
    if std::mem::take(&mut chess.board_mut().open_details) {
        chess.board_view = MovesView::Details;
    }
    let view = chess.board_view;
    if view == MovesView::Moves && !chess.moves_beside {
        return (moves(ui, t, chess, rect), None);
    }
    let mut action = None;
    let mut games = None;
    if view != MovesView::Moves {
        let back = tl!("Back to the moves");
        let r = ui
            .add(egui::Button::image_and_text(crate::icons::image("chevron-left", 14.0, t.icon), tl!("Moves")).min_size(vec2(24.0, 24.0)))
            .on_hover_text(tl!("Back to the moves (Esc)"));
        r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, back));
        if r.clicked() {
            chess.board_view = MovesView::Moves;
            return (None, None);
        }
    }
    egui::ScrollArea::vertical().id_salt(("chess-tab-view", view as u8)).auto_shrink([false, false]).show(ui, |ui| {
        // Content as wide as the region less the scroll bar's room (it floats over the content).
        ui.set_max_width((ui.available_width() - ui.spacing().scroll.allocated_width()).max(48.0));
        match view {
            MovesView::Moves | MovesView::Games => {
                ui.label(egui::RichText::new(crate::chess_collection::games_title(&chess.games)).strong());
                games = crate::chess_collection::games_list(ui, t, &mut chess.games);
            }
            MovesView::Details => {
                ui.label(egui::RichText::new(tl!("Game details")).strong());
                chess_board_ui::details(ui, chess.board_mut());
            }
            MovesView::Fields => {
                let state = chess.board_mut();
                heading(ui, t, "FEN");
                let fen = chess_board_ui::fen_field(ui, state);
                ui.add_space(6.0);
                heading(ui, t, "PGN");
                // The PGN field takes the height left under it, less its buttons' row.
                let rows = ((ui.available_height() - 40.0) / ui.text_style_height(&egui::TextStyle::Monospace).max(1.0)).floor();
                let rows = if rows.is_finite() { (rows as usize).clamp(3, 200) } else { 8 };
                action = chess_board_ui::pgn_field(ui, state, rows).or(fen);
            }
        }
    });
    (action, games)
}

/// A field's heading, painted (the field itself carries the name for assistive technology).
fn heading(ui: &mut egui::Ui, t: &Tokens, text: &str) {
    let font = egui::FontId::proportional(12.0);
    let h = ui.fonts_mut(|f| f.row_height(&font));
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), egui::Sense::hover());
    ui.painter().text(rect.left_center(), egui::Align2::LEFT_CENTER, text, font, t.text_muted);
}

/// The moves: a notice of what was just done, the glyph brush's bar while it is armed, the move
/// shown's comment large (a preference, plan HC12), the notation in the height left, and the
/// comment fields under it while Comment is on. What goes under the notation is measured each
/// frame and set aside the next.
fn moves(ui: &mut egui::Ui, t: &Tokens, chess: &mut ChessState, rect: Rect) -> Option<BoardAction> {
    let below_id = ui.id().with("chess-tab-below");
    let below = ui.ctx().data(|d| d.get_temp::<f32>(below_id)).unwrap_or(0.0);
    let brush = chess.tools.brush();
    let focus = chess.comment_focus;
    {
        let state = chess.board_mut();
        crate::chess_moves_tree::created_notice(ui, t, state);
        // While a step forward asks which reply (the trail has the replies), the keys to press.
        if state.choosing.is_some() {
            let keys = tl!("↑ ↓ choose, → or Enter goes in, Esc takes the main line.");
            ui.add(egui::Label::new(egui::RichText::new(keys).small().color(t.text_faint)).truncate());
        }
        if let Some(m) = state.message.clone() {
            ui.add(egui::Label::new(egui::RichText::new(m).small().color(t.text_muted)).truncate());
        }
    }
    if let Some(code) = brush {
        chess_board_ui::brush_bar(ui, t, chess, code);
    }
    if focus {
        comment_large(ui, t, chess.board());
    }
    let state = chess.board_mut();
    let height = (rect.bottom() - ui.cursor().top() - below - 2.0).max(0.0);
    let action = ui
        .scope(|ui| {
            // Compact rows ([`row_height`]).
            ui.spacing_mut().button_padding.y = ROW_PAD;
            ui.spacing_mut().interact_size.y = ROW_LEAST;
            chess_board_ui::notation(ui, t, state, Some(height), brush, true)
        })
        .inner;
    let below_top = ui.cursor().top();
    if state.comment_open {
        ui.add_space(4.0);
        chess_board_ui::notes(ui, t, state);
    }
    let measured = if state.comment_open { ui.cursor().top() - below_top } else { 0.0 };
    if measured.is_finite() && (measured - below).abs() > 0.5 {
        ui.ctx().data_mut(|d| d.insert_temp(below_id, measured));
        ui.ctx().request_repaint();
    }
    action
}

/// The move shown's comment, large (plan HC12), in at most three lines: a longer one ends in "…"
/// and is whole in its tooltip and accessible name.
fn comment_large(ui: &mut egui::Ui, t: &Tokens, state: &chess_board_ui::BoardState) {
    if state.illegal.is_some() {
        return;
    }
    let comment = state.game.comment(state.game.current());
    if comment.trim().is_empty() {
        return;
    }
    egui::Frame::new().fill(t.hover).corner_radius(egui::CornerRadius::same(6)).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let mut job = egui::text::LayoutJob::simple(comment.to_owned(), egui::FontId::proportional(15.0), t.text, ui.available_width());
        job.wrap.max_rows = 3;
        job.wrap.overflow_character = Some('…');
        let galley = ui.fonts_mut(|f| f.layout_job(job));
        let elided = galley.elided;
        let r = ui.add(egui::Label::new(galley));
        let r = if elided { r.on_hover_text(comment) } else { r };
        r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, comment));
    });
}

/// The bar: Game ▾ (the first move's number, the game's details, the document's games), Comment
/// (the comment fields under the moves) and FEN/PGN ▾ (copies, lichess, the start position, and
/// the two fields in the moves' place).
fn bar(ui: &mut egui::Ui, t: &Tokens, chess: &mut ChessState) -> Option<BoardAction> {
    let mut action = None;
    ui.spacing_mut().item_spacing.x = 6.0;
    // Buttons as tall as the bar, never taller.
    ui.spacing_mut().button_padding.y = 1.0;
    ui.spacing_mut().interact_size.y = BAR - 2.0;
    let open_menu =
        |label: &str| egui::Button::new((label.to_owned(), crate::icons::image("chevron-down", 12.0, t.icon))).min_size(vec2(24.0, BAR - 2.0));
    // Game ▾: open until a click outside it, so its number can be typed and dragged.
    let config = egui::containers::menu::MenuConfig::new().close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    let games_title = crate::chess_collection::games_title(&chess.games);
    let (_, picked) = egui::containers::menu::MenuButton::from_button(open_menu(tl!("Game"))).config(config).ui(ui, |ui| {
        ui.set_max_width(320.0);
        chess_board_ui::move_number(ui, chess.board_mut(), tl!("The game starts at move:"));
        ui.separator();
        if crate::widgets::menu_entry(ui, true, egui::Button::new(tl!("Game details"))).clicked() {
            return Some(MovesView::Details);
        }
        if crate::widgets::menu_entry(ui, true, egui::Button::new(games_title.as_str())).clicked() {
            return Some(MovesView::Games);
        }
        None
    });
    if let Some(view) = picked.and_then(|p| p.inner) {
        chess.board_view = view;
    }
    // Comment: the comment fields of the move shown, under the moves.
    let open = chess.board().comment_open;
    let r = ui
        .add(
            egui::Button::image_and_text(crate::icons::image("message-square", 14.0, t.icon), tl!("Comment"))
                .selected(open)
                .min_size(vec2(24.0, BAR - 2.0)),
        )
        .on_hover_text(tl!("The comments of the move shown, under the moves"));
    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, chess.board().illegal.is_none(), open, tl!("Comment")));
    if r.clicked() && chess.board().illegal.is_none() {
        chess.board_view = MovesView::Moves;
        chess.board_mut().toggle_comment_editor();
    }
    // FEN/PGN ▾.
    let (_, picked) = egui::containers::menu::MenuButton::from_button(open_menu("FEN/PGN")).ui(ui, |ui| {
        let asked = chess_board_ui::fen_pgn_items(ui, chess.board_mut());
        ui.separator();
        let edit = crate::widgets::menu_entry(ui, true, egui::Button::new(tl!("Edit the FEN and the PGN"))).clicked();
        (asked, edit)
    });
    if let Some((asked, edit)) = picked.map(|p| p.inner) {
        action = asked;
        if edit {
            chess.board_view = MovesView::Fields;
        }
    }
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tall(r: Rect) -> f32 {
        r.height()
    }

    /// Plan A9-I0b (§2.8.0 sketch (d)): in one column the regions stack without a gap or an
    /// overlap, as wide as the column, the bar at its foot; the moves keep their least rows and
    /// the board takes the rest, square, within its limit.
    #[test]
    fn the_regions_stack_and_the_board_takes_what_the_moves_leave() {
        let rect = Rect::from_min_size(pos2(10.0, 50.0), vec2(330.0, 620.0));
        for lines in 0..=3 {
            let r = Regions::column(rect, lines, 20.0, if lines > 0 { 18.0 } else { 0.0 }, 1_200.0);
            let order = [r.header, Rect::from_min_max(pos2(rect.min.x, r.board.min.y), r.board.max), r.nav, r.engine, r.trail, r.moves, r.bar];
            for (a, b) in order.iter().zip(order.iter().skip(1)) {
                assert!((a.max.y - b.min.y).abs() < 0.01, "{lines}: {a:?} then {b:?}");
            }
            assert_eq!(r.header.min.y, rect.min.y);
            assert!((r.bar.max.y - rect.max.y).abs() < 0.01, "the bar at the foot");
            assert_eq!((tall(r.header), tall(r.nav), tall(r.trail), tall(r.bar)), (HEADER, NAV, TRAIL, BAR));
            assert_eq!(tall(r.engine), lines as f32 * ENGINE_LINE);
            assert!((r.board.width() - r.board.height()).abs() < 0.01 && r.board.max.x <= rect.max.x + 0.01, "{:?}", r.board);
            assert!(tall(r.moves) >= Regions::least_moves(lines, 20.0) - 0.01, "{lines}: {:?}", r.moves);
            // The board as wide as the column allows, or as tall as the moves leave.
            let across = rect.width() - r.eval.map_or(0.0, |e| e.width());
            assert!((r.board.width() - across).abs() < 0.01 || (tall(r.moves) - Regions::least_moves(lines, 20.0)).abs() < 0.01);
        }
        // Wide: the board stops at its limit and the moves take the rest.
        let r = Regions::column(Rect::from_min_size(pos2(0.0, 0.0), vec2(2_000.0, 3_000.0)), 0, 20.0, 0.0, 380.0);
        assert_eq!(r.board.width(), 380.0);
        assert!(tall(r.moves) > 2_000.0);
    }

    /// A column too short for all of them: the bar stays inside it, the board keeps its least
    /// side while there is room for one, and nothing is NaN or negative, whatever the input.
    #[test]
    fn short_or_bad_columns_give_finite_regions() {
        let r = Regions::column(Rect::from_min_size(pos2(0.0, 0.0), vec2(330.0, 300.0)), 3, 20.0, 18.0, 1_200.0);
        assert!(r.board.width() >= 0.0 && r.bar.max.y <= 300.0 + 0.01, "{r:?}");
        // 380 − 100 fixed − 200 for ten rows leaves 80: the board keeps its least side.
        let r = Regions::column(Rect::from_min_size(pos2(0.0, 0.0), vec2(330.0, 380.0)), 0, 20.0, 0.0, 1_200.0);
        assert!((r.board.width() - SMALLEST_BOARD).abs() < 0.01, "the smallest board before the moves' least rows: {r:?}");
        for (w, h, row, eval, limit) in [
            (f32::NAN, 500.0, 20.0, 0.0, 1_200.0),
            (330.0, f32::INFINITY, f32::NAN, 0.0, 1_200.0),
            (-5.0, -5.0, 20.0, 99.0, f32::NAN),
            (0.0, 0.0, 0.0, 0.0, 0.0),
            (330.0, 620.0, 1e30, f32::INFINITY, -1.0),
        ] {
            let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(w, h));
            for r in [Regions::column(rect, 9, row, eval, limit), Regions::beside(rect, rect, 9, eval, limit)] {
                for part in [r.header, r.board, r.nav, r.engine, r.trail, r.moves, r.bar, r.setup] {
                    assert!(part.is_finite() && part.width() >= 0.0 && part.height() >= 0.0, "{w} {h} {row} {eval} {limit}: {part:?}");
                }
            }
        }
    }

    /// Two columns (a wide chess window): the board's regions in the left one, the moves' in the
    /// right one, the bar at its foot.
    #[test]
    fn beside_the_board_the_moves_fill_their_column() {
        let left = Rect::from_min_size(pos2(0.0, 0.0), vec2(500.0, 700.0));
        let right = Rect::from_min_size(pos2(510.0, 0.0), vec2(400.0, 700.0));
        let r = Regions::beside(left, right, 3, 18.0, 1_200.0);
        assert!(left.contains_rect(r.board) && left.contains_rect(r.nav) && left.contains_rect(r.engine));
        assert!(right.contains_rect(r.trail) && right.contains_rect(r.moves) && right.contains_rect(r.bar));
        assert!((r.bar.max.y - right.max.y).abs() < 0.01 && (r.trail.min.y - right.min.y).abs() < 0.01);
        assert!(r.board.width() <= 500.0 - 18.0 + 0.01 && r.board.max.y <= r.nav.min.y + 0.01);
    }
}
