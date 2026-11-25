//! Chess ▸ the Board tab in fixed regions (plan A9-I0b, §2.8.0 sketch (d); findings A6-12, A6-24,
//! R8-07): the board never scrolls. Top to bottom, each region measured before anything is drawn
//! ([`Regions`]):
//!
//! - a header of [`HEADER`] points: the move shown and whose move it is, then Analyze, the Move
//!   list beside the page, the board's theme and the board's menu (⋯: the engine's lines and
//!   program, clearing the drawings, the diagram the board came from);
//! - the board, as large as the room the other regions leave allows (with the evaluation bar
//!   beside it while the engine analyses), or the side the user chose ([`BoardSide`]) by dragging
//!   the grip under it or with the board's menu, kept between sessions (in a panel wider than
//!   tall, with the moves beside it where that gives a larger board: [`Regions::tab`]);
//! - the navigation row ([`NAV`]): start, back, forward, end, the variations before and after, the
//!   board turned round, and at its end the engine's state or the material;
//! - the engine's lines while it analyses, a row of [`ENGINE_LINE`] each (up to
//!   [`MAX_ENGINE_LINES`]), cut short with "…" and whole in their tooltips;
//! - the trail ([`TRAIL`]): where in the tree the move shown is, the continuations at a fork, or
//!   the diagram the board came from;
//! - the moves, filling the rest (at least [`ROWS`] rows of them, [`ROWS_WITH_ENGINE`] with the
//!   engine's lines), their own scroll area the only one;
//! - a bar of [`BAR`] points at the foot: **Game ▾ · Comment · In the book (n) · FEN/PGN ▾**,
//!   which took the place of the sections under the list. The first move's number is in Game ▾
//!   (§2.8.1); the game's details, the document's games, where the position is in the book (plan
//!   A9-N3, [`crate::chess_in_book`]) and the FEN and PGN fields take the moves' place
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
/// The rows an automatic board leaves the moves in a wide panel, and the widths (points) from
/// which they fall from those above and at which they are these (2026-11-25: the moves' room is
/// wide there, and in a class the board is what is looked at).
const ROWS_WIDE: usize = 4;
const WIDE: std::ops::RangeInclusive<f32> = 420.0..=600.0;
/// Least rows of moves under a board whose side the user chose, while something shows in the
/// moves' place (the games, the fields, the comment being written: [`rows_kept`]). While only the
/// moves would, it may take all their room: the board as large as the panel allows.
pub(crate) const ROWS_CHOSEN: usize = 3;
/// The board with the moves beside it, where the panel is wide ([`Regions::tab`]): the least width
/// of the board's side (its header and navigation), of the moves', and the gap between them.
const BESIDE_BOARD: f32 = 300.0;
const BESIDE_MOVES: f32 = 260.0;
const BESIDE_GAP: f32 = 10.0;
/// The smallest board drawn while the column holds the other regions at all, in points.
const SMALLEST_BOARD: f32 = 96.0;
/// The largest side the user may choose, in points: the panels' largest board.
const LARGEST_CHOSEN: f32 = Sizing::DOCKED.max_side;
// `BoardSide::chosen` clamps to these: the least below the most, or `clamp` would panic.
const _: () = assert!(SMALLEST_BOARD < LARGEST_CHOSEN);
/// What Larger board and Smaller board in the board's menu change the side by: about 10 %.
const STEP: f32 = 1.1;
/// The grip under the board: the navigation row's top this many points, as wide as the board.
const GRIP: f32 = 4.0;
/// What the moves' region keeps besides its rows: the game's header line and the gaps.
const MOVES_SLACK: f32 = 8.0;
/// The moves' rows in the Board tab are compact (plan A9-I0b): a move's button has this much
/// room above and below its text and is at least this tall, so ten rows take the 160–230 pt the
/// sketch gives them (the panel's 24 pt buttons made them 29 pt each, and the board 96 pt at
/// 1366 × 768). The moves are notation, set like text.
const ROW_PAD: f32 = 2.0;
const ROW_LEAST: f32 = 18.0;

/// What the moves' region shows (plan A9-I0b): the moves, or in their place the document's
/// games, the game's details, the FEN and PGN fields, or where the position is in the book (plan
/// A9-N3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MovesView {
    #[default]
    Moves,
    Games,
    Details,
    Fields,
    Book,
}

/// The board's side in the Chess column's Board tab: automatic, as large as the other regions
/// and [`ROWS`] rows of moves leave it (or the moves beside it, [`Regions::tab`]); the side the
/// user chose, in points, by dragging the grip under the board or with the board's menu; or the
/// largest the panel allows whatever its size (the grip dragged all the way down, or the menu).
/// Kept between sessions as the settings' `chess_board_size`: null (or absent) for automatic.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) enum BoardSide {
    #[default]
    Auto,
    /// Within [`SMALLEST_BOARD`]..=[`LARGEST_CHOSEN`] when made by [`BoardSide::chosen`].
    Chosen(f32),
    /// As large as the panel allows, growing and shrinking with it.
    Largest,
}

impl BoardSide {
    /// The settings' word for [`BoardSide::Largest`].
    const LARGEST: &str = "largest";

    /// A side the user chose, `side` points, kept within [`SMALLEST_BOARD`]..=[`LARGEST_CHOSEN`];
    /// a side that is not a number is automatic.
    pub(crate) fn chosen(side: f32) -> BoardSide {
        // `clamp` can't panic: its bounds are numbers, the least below the most (checked when
        // compiling, below the constants), and `side` is one.
        if side.is_nan() { BoardSide::Auto } else { BoardSide::Chosen(side.clamp(SMALLEST_BOARD, LARGEST_CHOSEN)) }
    }

    /// The side asked for, if the user chose one (one that is not a number counts as none): more
    /// than any panel allows for the largest.
    fn side(self) -> Option<f32> {
        match self {
            BoardSide::Chosen(side) if !side.is_nan() => Some(side),
            BoardSide::Largest => Some(f32::INFINITY),
            _ => None,
        }
    }

    /// The settings' `chess_board_size`: null for automatic, `"largest"`, else the side in points,
    /// to a tenth.
    pub(crate) fn to_json(self) -> serde_json::Value {
        if self == BoardSide::Largest {
            return serde_json::Value::from(Self::LARGEST);
        }
        // `from` gives null for a number JSON can't hold.
        self.side().map_or(serde_json::Value::Null, |side| serde_json::Value::from((f64::from(side) * 10.0).round() / 10.0))
    }

    /// What the settings say: a number is a side, kept within the bounds, so a hostile file makes
    /// neither a NaN nor a board of 10⁹ points; anything else (absent, null, other text) is
    /// automatic.
    pub(crate) fn from_json(v: &serde_json::Value) -> BoardSide {
        if v.as_str() == Some(Self::LARGEST) {
            return BoardSide::Largest;
        }
        // `as` saturates: 1e300 becomes infinity, which `chosen` brings down to the largest side.
        v.as_f64().map_or(BoardSide::Auto, |side| BoardSide::chosen(side as f32))
    }

    /// The control channel's `board-side`: `auto`, `largest`, or a side in points (kept within the
    /// bounds).
    pub(crate) fn parse(value: &str) -> Option<BoardSide> {
        match value.trim() {
            "auto" => Some(BoardSide::Auto),
            Self::LARGEST => Some(BoardSide::Largest),
            value => value.parse::<f32>().ok().filter(|side| side.is_finite()).map(BoardSide::chosen),
        }
    }
}

/// What the Board tab's regions are measured from, besides the rectangle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Measures {
    /// The engine's lines (0–3).
    pub(crate) lines: usize,
    /// A row of moves, in points.
    pub(crate) row: f32,
    /// The evaluation bar's room beside the board.
    pub(crate) eval: f32,
    /// The largest board, in points.
    pub(crate) limit: f32,
    /// The rows of moves kept under a board whose side the user chose ([`rows_kept`]).
    pub(crate) kept: usize,
}

/// The rows of moves kept under a board whose side the user chose: none while only the moves
/// would show there (the board may take all the panel), [`ROWS_CHOSEN`] while something the user
/// opened shows in their place.
fn rows_kept(chess: &ChessState) -> usize {
    let board = chess.board();
    let open = chess.board_view != MovesView::Moves || board.comment_open || board.open_details || chess.tools.brush().is_some();
    if open { ROWS_CHOSEN } else { 0 }
}

/// The board's sizes in the Board tab, for the grip under it and the board's menu: its side as
/// drawn, the least and the most a side the user chooses gets there (the smallest board; what the
/// panel allows, [`Regions::tab`]), and what the user chose.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Sizes {
    pub(crate) side: f32,
    pub(crate) least: f32,
    pub(crate) most: f32,
    pub(crate) asked: BoardSide,
}

impl Sizes {
    /// The sizes in the tab [`Regions::tab`] measures with the same arguments.
    pub(crate) fn tab(rect: Rect, m: Measures, side: BoardSide) -> Sizes {
        let drawn = |side| Regions::tab(rect, m, side).0.board.width();
        // A side of 0 gets the smallest board the tab gives, the largest its most.
        Sizes { side: drawn(side), least: drawn(BoardSide::Chosen(0.0)), most: drawn(BoardSide::Largest), asked: side }
    }

    /// What a side picked by the user is kept as: the most the panel gives is the largest, so the
    /// board grows with the panel from there.
    pub(crate) fn pick(self, side: f32) -> BoardSide {
        if side + 0.5 >= self.most { BoardSide::Largest } else { BoardSide::chosen(side) }
    }

    /// Whether Larger board has anything to do (half a point is no step).
    pub(crate) fn can_grow(self) -> bool {
        self.side + 0.5 < self.most
    }

    /// Whether Smaller board has anything to do.
    pub(crate) fn can_shrink(self) -> bool {
        self.side > self.least + 0.5
    }

    /// The side a step of [`STEP`] larger or smaller than the board's gives, within the least and
    /// the most.
    pub(crate) fn step(self, larger: bool) -> f32 {
        let side = if larger { self.side * STEP } else { self.side / STEP };
        // `max` then `min`, not `clamp`: no panic whatever the bounds are.
        side.max(self.least).min(self.most)
    }

    /// The side a drag of the grip gives: the board's side `from` when the drag began, and the
    /// pointer `dy` points lower than where it was pressed (the board's foot follows the pointer),
    /// within the least and the most.
    pub(crate) fn dragged(self, from: f32, dy: f32) -> f32 {
        (finite(from) + finite(dy)).max(self.least).min(self.most)
    }
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

    /// The least height of the moves under an automatic board in a column `w` wide, `row` points a
    /// row: [`ROWS`], or [`ROWS_WITH_ENGINE`] with the engine's lines, falling to [`ROWS_WIDE`] in a
    /// wide one ([`WIDE`]), and the region's own gaps.
    pub(crate) fn least_moves(w: f32, lines: usize, row: f32) -> f32 {
        let narrow = Self::least_rows(if lines > 0 { ROWS_WITH_ENGINE } else { ROWS }, row);
        let wide = Self::least_rows(ROWS_WIDE, row);
        let share = ((finite(w) - WIDE.start()) / (WIDE.end() - WIDE.start())).clamp(0.0, 1.0);
        narrow + (wide - narrow) * share
    }

    /// The height of `rows` rows of moves, `row` points each, and the region's own gaps.
    pub(crate) fn least_rows(rows: usize, row: f32) -> f32 {
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
    /// The board's side is `chosen`'s: automatic, as large as [`ROWS`] rows of moves leave it
    /// ([`ROWS_WITH_ENGINE`] with the engine's lines); or the side the user chose, as far as the
    /// column's width and the rows of moves `kept` allow. Either way the smallest board while the
    /// column has room for it.
    pub(crate) fn column(rect: Rect, m: Measures, chosen: BoardSide) -> Regions {
        let Measures { lines, row, eval, limit, kept } = m;
        let (x, top) = (finite(rect.min.x), finite(rect.min.y));
        let (w, h) = (finite(rect.width()).max(0.0), finite(rect.height()).max(0.0));
        let engine = Self::engine_height(lines);
        let room = (h - HEADER - NAV - engine - TRAIL - BAR).max(0.0);
        let eval = finite(eval).clamp(0.0, w);
        // `min` with a finite room: a chosen infinity is the most the column gives, and minus
        // infinity becomes 0 in `side`, the smallest board.
        let down = match chosen.side() {
            None => room - Self::least_moves(w, lines, row),
            Some(side) => side.min(if kept == 0 { room } else { room - Self::least_rows(kept, row) }),
        };
        let side = Self::side(w - eval, down, room, limit);
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

    /// The docked Board tab's regions in `rect`, and how they are laid out: in one column, or the
    /// moves beside the board where that gives the larger board (a panel wider than tall). A side
    /// both give takes the layout the automatic board has there.
    pub(crate) fn tab(rect: Rect, m: Measures, chosen: BoardSide) -> (Regions, Arrangement) {
        let column = Self::column(rect, m, chosen);
        let (w, h) = (finite(rect.width()).max(0.0), finite(rect.height()).max(0.0));
        if w < BESIDE_BOARD + BESIDE_GAP + BESIDE_MOVES {
            return (column, Arrangement::Column);
        }
        let eval = finite(m.eval).clamp(0.0, w);
        // The most the board gets with the moves beside it.
        let room = (h - HEADER - NAV - Self::engine_height(m.lines)).max(0.0);
        let most = Self::side(w - BESIDE_GAP - BESIDE_MOVES - eval, room, room, m.limit);
        let side = chosen.side().map_or(most, |side| side.max(SMALLEST_BOARD).min(most));
        let in_column = column.board.width();
        let beside = side > in_column + 0.5
            || (side + 0.5 >= in_column
                && matches!(chosen, BoardSide::Chosen(_))
                && most > Self::column(rect, m, BoardSide::Auto).board.width() + 0.5);
        if !beside {
            return (column, Arrangement::Column);
        }
        let (x, top) = (finite(rect.min.x), finite(rect.min.y));
        let left = Rect::from_min_size(pos2(x, top), vec2((side + eval).max(BESIDE_BOARD), h));
        let right = Rect::from_min_max(pos2(left.max.x + BESIDE_GAP, top), pos2((x + w).max(left.max.x + BESIDE_GAP), top + h));
        (Self::beside(left, right, m.lines, eval, side), Arrangement::Beside)
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

/// The Board tab in `rect` (the Chess column's Board tab, a narrow chess window), in one column.
/// `sizable` (the Chess column's): the board has the side the user chose ([`BoardSide`]), a grip
/// under it and its sizes in the board's menu, and the moves stand beside it where that gives a
/// larger board ([`Regions::tab`]); the chess window keeps its own sizing.
#[allow(clippy::too_many_arguments)]
pub(crate) fn column(
    ui: &mut egui::Ui,
    t: &Tokens,
    chess: &mut ChessState,
    pieces: &PieceSet,
    source_label: Option<&str>,
    sizing: Sizing,
    rect: Rect,
    sizable: bool,
) -> Asked {
    let m = Measures {
        lines: engine_lines(chess),
        row: row_height(ui),
        eval: crate::chess_study_ui::eval_room(ui, &chess.study),
        limit: sizing.side_limit(),
        kept: rows_kept(chess),
    };
    let (regions, layout, sizes) = if sizable {
        let chosen = chess.board_side;
        let (regions, layout) = Regions::tab(rect, m, chosen);
        (regions, layout, Some(Sizes::tab(rect, m, chosen)))
    } else {
        (Regions::column(rect, m, BoardSide::Auto), Arrangement::Column, None)
    };
    if layout == Arrangement::Beside {
        // Between the board's side and the moves', as in the chess window.
        ui.painter().vline(regions.moves.min.x - BESIDE_GAP / 2.0, rect.y_range(), egui::Stroke::new(1.0, t.divider));
    }
    let asked = draw(ui, t, chess, pieces, source_label, sizing, &regions, layout, sizes);
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
    let layout = if right.is_some() { Arrangement::Beside } else { Arrangement::BoardOnly };
    let asked = draw(ui, t, chess, pieces, source_label, sizing, &regions, layout, None);
    ui.advance_cursor_after_rect(right.map_or(left, |r| left.union(r)));
    asked
}

/// How the regions are laid out: in one column, the moves beside the board, or the board alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Arrangement {
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
    sizes: Option<Sizes>,
) -> Asked {
    let mut asked = Asked::default();
    ui.data_mut(|d| d.insert_temp(egui::Id::new(REGIONS_ID), [r.header, r.board, r.nav, r.engine, r.trail, r.moves, r.bar]));
    asked.board = in_row(ui, r.header, "chess-tab-header", |ui| header(ui, t, chess, source_label, sizes));
    // The fields are filled from the game whatever shows (the PGN only when the game changed),
    // and before any field is drawn, which one has the keyboard, for the glyph palette and the
    // rail's glyph buttons (whether or not a field of this tab shows: a capture's correction
    // field is one).
    let state = chess.board_mut();
    state.refresh_fields(false);
    state.announce_move(ui.ctx());
    state.typing.track(ui);
    // The set-up view (the rail's Set up tool) in the place of the board, and in one column of
    // everything under the header: it is taller than the board, so it scrolls.
    let setting_up = if chess.tools.tool == PgnTool::Setup {
        in_column(ui, r.setup, "chess-tab-setup", |ui| {
            egui::ScrollArea::vertical()
                .id_salt("chess-tab-setup-scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let asked = crate::chess_study_ui::setup_in_place(ui, t, chess, pieces);
                    crate::widgets::follow_focus(ui);
                    asked
                })
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
        // After the navigation row, so that the grip is over its top edge.
        if let Some(sizes) = sizes {
            grip(ui, t, chess, r.board, r.nav, sizes);
        }
        if r.engine.height() > 0.0 {
            engine_rows(ui, t, chess, r.engine);
        }
    }
    if (setting_up && layout == Arrangement::Column) || layout == Arrangement::BoardOnly {
        return asked;
    }
    // The largest board takes the moves' room too ([`rows_kept`]): what they say of a move that
    // could not be played is said in the trail's row then.
    let moves_shown = r.moves.height() >= ROW_LEAST;
    let trail = in_row(ui, r.trail, "chess-tab-trail", |ui| {
        if !moves_shown && let Some(m) = chess.board().message.clone() {
            ui.add(egui::Label::new(egui::RichText::new(m).small().color(t.text_muted)).truncate());
            return None;
        }
        trail_row(ui, t, chess, source_label)
    });
    if let Some(a) = trail {
        asked.board = asked.board.or(Some(a));
    }
    if moves_shown {
        let (board, games) = in_column(ui, r.moves, "chess-tab-moves", |ui| moves_region(ui, t, chess, r.moves));
        asked.board = asked.board.or(board);
        asked.games = games;
    }
    if let Some(a) = in_row(ui, r.bar, "chess-tab-bar", |ui| bar(ui, t, chess)) {
        asked.board = asked.board.or(Some(a));
    }
    asked
}

/// The header: the move shown and whose move it is (cut short with "…", whole in its tooltip and
/// accessible name), and from the right the board's menu, its theme, the Move list beside the
/// page and Analyze.
fn header(ui: &mut egui::Ui, t: &Tokens, chess: &mut ChessState, source_label: Option<&str>, sizes: Option<Sizes>) -> Option<BoardAction> {
    let mut action = None;
    ui.spacing_mut().item_spacing.x = 4.0;
    ui.spacing_mut().button_padding.y = 2.0;
    let (text, colour, dot) = {
        let state = chess.board();
        let (status, colour) = chess_board_ui::status(state, t);
        let current = state.game().current();
        let text = if !state.is_illegal() && current != 0 {
            format!("{} · {status}", crate::chess_tools::move_name(state.game(), current))
        } else {
            status
        };
        let dot = (!state.is_illegal()).then(|| state.game().side_to_move() == Color::White);
        (text, colour, dot)
    };
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let name = tl!("More board actions");
        let menu = egui::containers::menu::MenuButton::from_button(
            egui::Button::image(crate::icons::image("ellipsis", 16.0, t.icon)).min_size(vec2(HEADER, HEADER - 4.0)),
        );
        let (r, picked) = menu.ui(ui, |ui| board_menu(ui, t, chess, source_label, sizes));
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
/// made), the board's size where it has `sizes` (the Chess column's Board tab), and the diagram
/// the board came from.
fn board_menu(ui: &mut egui::Ui, t: &Tokens, chess: &mut ChessState, source_label: Option<&str>, sizes: Option<Sizes>) -> Option<BoardAction> {
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
        // Which program, and where (plan A9-I13): of two in the engine folder, the one taken.
        if let Some(program) = chess.study.program() {
            let named = crate::chess_study_ui::engine_program(program, pdfcraft_platform::app_dirs::AppDirs::current().install());
            ui.add(egui::Label::new(egui::RichText::new(named).small().color(t.text_muted)).wrap()).on_hover_text(program.display().to_string());
        }
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
    let source = board.source;
    // The board's size from the keyboard, and for screen readers: what the grip under it does.
    if let Some(sizes) = sizes {
        ui.separator();
        if crate::widgets::menu_entry(ui, sizes.can_grow(), egui::Button::new(tl!("Larger board"))).clicked() {
            chess.board_side = sizes.pick(sizes.step(true));
        }
        if crate::widgets::menu_entry(ui, sizes.can_shrink(), egui::Button::new(tl!("Smaller board"))).clicked() {
            chess.board_side = BoardSide::chosen(sizes.step(false));
        }
        let largest = sizes.asked == BoardSide::Largest;
        if crate::widgets::menu_entry(ui, !largest, egui::Button::new(tl!("Board size: the largest"))).clicked() {
            chess.board_side = BoardSide::Largest;
        }
        if crate::widgets::menu_entry(ui, sizes.asked != BoardSide::Auto, egui::Button::new(tl!("Board size: automatic"))).clicked() {
            chess.board_side = BoardSide::Auto;
        }
    }
    if let (Some(label), Some(source)) = (source_label, source) {
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
    // The Move field (plan A9-N7a; its place, the navigation row, chosen by the coordinator
    // 2026-11-16): after the arrows, which never give up room. The material at the row's end gives
    // its room up first, then the field shrinks to its narrowest. While the engine analyses, its
    // state keeps the row's end and the field waits behind a button (it, or M, opens the field,
    // which then has the row while it has the keyboard).
    let room = ui.available_rect_before_wrap().width();
    #[cfg(not(target_arch = "wasm32"))]
    let analysing = crate::chess_study_ui::engine_status(&chess.study, chess.board()).is_some();
    #[cfg(target_arch = "wasm32")]
    let analysing = false;
    let typing = chess.move_field.focus || ui.memory(|m| m.has_focus(crate::chess_move_field::field_id()));
    if analysing && !typing {
        if crate::chess_move_field::open_button(ui).clicked() {
            chess.move_field.focus = true;
        }
    } else if let Some(width) = crate::chess_move_field::width_in(room, 0.0) {
        ui.add_space(crate::chess_move_field::GAP);
        let (board, field) = (chess.games.board_mut(), &mut chess.move_field);
        crate::chess_move_field::field(ui, board, field, width);
        ui.add_space(crate::chess_move_field::GAP);
    }
    if analysing && typing {
        return;
    }
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

/// The grip under the board, as wide as it: the navigation row's top [`GRIP`] points, over the row
/// and never over the board, which keeps every click and drag of its own (its squares end where
/// the row begins). Dragged, the board's foot follows the pointer within what the column allows
/// ([`Sizes`]); a double-click gives the board its automatic side back. It takes no keyboard focus
/// (the chess keys take the arrows): the board's menu has the same as Larger board, Smaller board
/// and Board size: automatic.
fn grip(ui: &mut egui::Ui, t: &Tokens, chess: &mut ChessState, board: Rect, nav: Rect, sizes: Sizes) {
    let rect = Rect::from_min_max(pos2(board.min.x, nav.min.y), pos2(board.max.x, nav.min.y + GRIP.min(nav.height())));
    if !(rect.is_finite() && rect.width() > 0.0 && rect.height() > 0.0) {
        return;
    }
    let id = ui.id().with("chess-board-grip");
    let name = tl!("Drag to change the board's size");
    let r = ui.interact(rect, id, egui::Sense::CLICK | egui::Sense::DRAG);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, name));
    ui.ctx().accesskit_node_builder(r.id, |node| {
        node.set_role(egui::accesskit::Role::Splitter);
        node.set_orientation(egui::accesskit::Orientation::Horizontal);
    });
    if r.double_clicked() {
        chess.board_side = BoardSide::Auto;
    }
    // The side when the drag began, and the pointer's way down since it was pressed.
    if r.drag_started() {
        ui.data_mut(|d| d.insert_temp(id, sizes.side));
    }
    if r.dragged() {
        let from = ui.data(|d| d.get_temp::<f32>(id)).unwrap_or(sizes.side);
        let dy = ui.input(|i| i.pointer.press_origin().zip(i.pointer.interact_pos()).map_or(0.0, |(pressed, now)| now.y - pressed.y));
        chess.board_side = sizes.pick(sizes.dragged(from, dy));
    }
    let active = r.hovered() || r.dragged();
    if active {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
        ui.painter().rect_filled(rect, 0.0, t.accent);
    } else {
        // A short bar in the middle, as a grip is drawn.
        let bar = Rect::from_center_size(rect.center(), vec2(rect.width().min(32.0), rect.height().min(3.0)));
        ui.painter().rect_filled(bar, 1.5, t.border_strong);
    }
    r.on_hover_text(name);
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
    if state.is_illegal() {
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
    } else if !tree::variations_holding(state.game(), state.game().current()).is_empty() {
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
    crate::widgets::solid_scroll_bar(ui);
    egui::ScrollArea::vertical().id_salt(("chess-tab-view", view as u8)).auto_shrink([false, false]).show(ui, |ui| {
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
            MovesView::Book => crate::chess_in_book::list(ui, t, chess),
        }
        crate::widgets::follow_focus(ui);
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
/// and is whole in its tooltip and accessible name. Its prose only: a comment of `[%clk]` and
/// `[%eval]` commands, as every move of a lichess game has, shows nothing (plan A9-F7, R6-01).
fn comment_large(ui: &mut egui::Ui, t: &Tokens, state: &chess_board_ui::BoardState) {
    if state.is_illegal() {
        return;
    }
    let comment = pdfcraft_engine::chess::pdfcraft_chess::pgn::prose(state.game().comment(state.game().current()));
    let comment = comment.as_ref();
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
/// (the comment fields under the moves), In the book (n) (where the position is in the book, in
/// the moves' place; plan A9-N3) and FEN/PGN ▾ (copies, lichess, the start position, and the two
/// fields in the moves' place).
fn bar(ui: &mut egui::Ui, t: &Tokens, chess: &mut ChessState) -> Option<BoardAction> {
    let mut action = None;
    ui.spacing_mut().item_spacing.x = 4.0;
    // Buttons as tall as the bar, never taller, and narrow: four share a 360 pt column.
    ui.spacing_mut().button_padding = vec2(4.0, 1.0);
    ui.spacing_mut().interact_size.y = BAR - 2.0;
    // Measured before anything is drawn (§2.8.1): where the four do not fit with their words (the
    // reference screen's 360 pt column), Comment keeps its icon alone, named as before; In the
    // book (n) is cut with "…" in the rest.
    let book = crate::chess_in_book::bar_label(&chess.in_book.status());
    let (compact, book_room) = {
        let font = egui::TextStyle::Button.resolve(ui.style());
        let text = |s: &str| crate::widgets::text_width(ui, s, &font);
        let (pad, gap, icon) = (2.0 * ui.spacing().button_padding.x, ui.spacing().item_spacing.x, ui.spacing().icon_spacing);
        let menus = text(tl!("Game")) + text("FEN/PGN") + 2.0 * (12.0 + icon + pad);
        let comment = 14.0 + icon + text(tl!("Comment")) + pad;
        let room = ui.available_width();
        let compact = menus + comment + text(&book) + pad + 3.0 * gap > room;
        let comment = if compact { 24.0_f32.max(14.0 + pad) } else { comment };
        (compact, (room - menus - comment - 3.0 * gap - pad).max(0.0))
    };
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
    let icon = crate::icons::image("message-square", 14.0, t.icon);
    let button = if compact { egui::Button::image(icon) } else { egui::Button::image_and_text(icon, tl!("Comment")) };
    let r = ui.add(button.selected(open).min_size(vec2(24.0, BAR - 2.0))).on_hover_text(tl!("The comments of the move shown, under the moves"));
    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, !chess.board().is_illegal(), open, tl!("Comment")));
    if r.clicked() && !chess.board().is_illegal() {
        chess.board_view = MovesView::Moves;
        chess.board_mut().toggle_comment_editor();
    }
    // In the book (n): its list in the moves' place, or the moves again.
    if crate::chess_in_book::bar_item(ui, chess, &book, book_room, BAR - 2.0) {
        chess.board_view = if chess.board_view == MovesView::Book { MovesView::Moves } else { MovesView::Book };
    }
    // FEN/PGN ▾; its lichess link from the side the board's diagram is printed from (plan A9-G9).
    let printed = chess.board().source.and_then(|s| chess.results_of(s.doc).map(|r| r.printed_from(s.page, s.index)));
    let (_, picked) = egui::containers::menu::MenuButton::from_button(open_menu("FEN/PGN")).ui(ui, |ui| {
        let asked = chess_board_ui::fen_pgn_items(ui, chess.board_mut(), printed);
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

    /// Measures with three rows of moves kept under a chosen side.
    fn of(lines: usize, row: f32, eval: f32, limit: f32) -> Measures {
        Measures { lines, row, eval, limit, kept: ROWS_CHOSEN }
    }

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
            let r = Regions::column(rect, of(lines, 20.0, if lines > 0 { 18.0 } else { 0.0 }, 1_200.0), BoardSide::Auto);
            let order = [r.header, Rect::from_min_max(pos2(rect.min.x, r.board.min.y), r.board.max), r.nav, r.engine, r.trail, r.moves, r.bar];
            for (a, b) in order.iter().zip(order.iter().skip(1)) {
                assert!((a.max.y - b.min.y).abs() < 0.01, "{lines}: {a:?} then {b:?}");
            }
            assert_eq!(r.header.min.y, rect.min.y);
            assert!((r.bar.max.y - rect.max.y).abs() < 0.01, "the bar at the foot");
            assert_eq!((tall(r.header), tall(r.nav), tall(r.trail), tall(r.bar)), (HEADER, NAV, TRAIL, BAR));
            assert_eq!(tall(r.engine), lines as f32 * ENGINE_LINE);
            assert!((r.board.width() - r.board.height()).abs() < 0.01 && r.board.max.x <= rect.max.x + 0.01, "{:?}", r.board);
            assert!(tall(r.moves) >= Regions::least_moves(330.0, lines, 20.0) - 0.01, "{lines}: {:?}", r.moves);
            // The board as wide as the column allows, or as tall as the moves leave.
            let across = rect.width() - r.eval.map_or(0.0, |e| e.width());
            assert!((r.board.width() - across).abs() < 0.01 || (tall(r.moves) - Regions::least_moves(330.0, lines, 20.0)).abs() < 0.01);
        }
        // Wide: the board stops at its limit and the moves take the rest.
        let r = Regions::column(Rect::from_min_size(pos2(0.0, 0.0), vec2(2_000.0, 3_000.0)), of(0, 20.0, 0.0, 380.0), BoardSide::Auto);
        assert_eq!(r.board.width(), 380.0);
        assert!(tall(r.moves) > 2_000.0);
    }

    /// A column too short for all of them: the bar stays inside it, the board keeps its least
    /// side while there is room for one, and nothing is NaN or negative, whatever the input.
    #[test]
    fn short_or_bad_columns_give_finite_regions() {
        let r = Regions::column(Rect::from_min_size(pos2(0.0, 0.0), vec2(330.0, 300.0)), of(3, 20.0, 18.0, 1_200.0), BoardSide::Auto);
        assert!(r.board.width() >= 0.0 && r.bar.max.y <= 300.0 + 0.01, "{r:?}");
        // 380 − 100 fixed − 200 for ten rows leaves 80: the board keeps its least side.
        let r = Regions::column(Rect::from_min_size(pos2(0.0, 0.0), vec2(330.0, 380.0)), of(0, 20.0, 0.0, 1_200.0), BoardSide::Auto);
        assert!((r.board.width() - SMALLEST_BOARD).abs() < 0.01, "the smallest board before the moves' least rows: {r:?}");
        for (w, h, row, eval, limit) in [
            (f32::NAN, 500.0, 20.0, 0.0, 1_200.0),
            (330.0, f32::INFINITY, f32::NAN, 0.0, 1_200.0),
            (-5.0, -5.0, 20.0, 99.0, f32::NAN),
            (0.0, 0.0, 0.0, 0.0, 0.0),
            (330.0, 620.0, 1e30, f32::INFINITY, -1.0),
        ] {
            let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(w, h));
            for chosen in CHOSEN {
                for r in [Regions::column(rect, of(9, row, eval, limit), chosen), Regions::beside(rect, rect, 9, eval, limit)] {
                    for part in [r.header, r.board, r.nav, r.engine, r.trail, r.moves, r.bar, r.setup] {
                        assert!(part.is_finite() && part.width() >= 0.0 && part.height() >= 0.0, "{w} {h} {row} {eval} {limit} {chosen:?}: {part:?}");
                    }
                }
                let (r, _) = Regions::tab(rect, of(9, row, eval, limit), chosen);
                for part in [r.header, r.board, r.nav, r.engine, r.trail, r.moves, r.bar, r.setup] {
                    assert!(part.is_finite() && part.width() >= 0.0 && part.height() >= 0.0, "{w} {h} {row} {eval} {limit} {chosen:?}: {part:?}");
                }
                let sizes = Sizes::tab(rect, of(9, row, eval, limit), chosen);
                assert!([sizes.side, sizes.least, sizes.most].iter().all(|v| v.is_finite() && *v >= 0.0), "{sizes:?}");
                assert!(sizes.least <= sizes.most, "{sizes:?}");
            }
        }
    }

    /// Sides a user, a settings file or a bug could ask for, the bad ones made directly (not
    /// through [`BoardSide::chosen`], which would set them right).
    const CHOSEN: [BoardSide; 11] = [
        BoardSide::Largest,
        BoardSide::Auto,
        BoardSide::Chosen(f32::NAN),
        BoardSide::Chosen(f32::INFINITY),
        BoardSide::Chosen(f32::NEG_INFINITY),
        BoardSide::Chosen(0.0),
        BoardSide::Chosen(-5.0),
        BoardSide::Chosen(1e30),
        BoardSide::Chosen(f32::MAX),
        BoardSide::Chosen(f32::MIN_POSITIVE),
        BoardSide::Chosen(250.0),
    ];

    /// The board's side in one column `w` × `h` points.
    fn side_in(w: f32, h: f32, lines: usize, row: f32, eval: f32, limit: f32, chosen: BoardSide) -> f32 {
        Regions::column(Rect::from_min_size(pos2(12.0, 40.0), vec2(w, h)), of(lines, row, eval, limit), chosen).board.width()
    }

    /// The board's size (2026-11-19): automatic is the rule of before, side for side. The user's
    /// short window (a Board tab about 314 × 480 points) gives 152, a narrow column its width, a
    /// short one the smallest board.
    #[test]
    fn an_automatic_board_keeps_the_sides_it_had() {
        let auto = BoardSide::Auto;
        for (w, h, lines, row, eval, limit, side) in [
            // 620 − 100 fixed − 208 for ten rows of 20.
            (330.0, 620.0, 0, 20.0, 0.0, 1_200.0, 312.0),
            // 620 − 100 − 60 for three engine lines − 148 for seven rows; 330 − 18 across.
            (330.0, 620.0, 3, 20.0, 18.0, 1_200.0, 312.0),
            // The user's window: 480 − 100 − 228 for ten rows of 22.
            (314.0, 480.0, 0, 22.0, 0.0, 1_200.0, 152.0),
            // As wide as the column.
            (250.0, 700.0, 0, 20.0, 0.0, 1_200.0, 250.0),
            // Short: the smallest board; narrow: the column's width.
            (330.0, 380.0, 0, 20.0, 0.0, 1_200.0, 96.0),
            (80.0, 600.0, 0, 20.0, 0.0, 1_200.0, 80.0),
            // The limit; and rows that are not a number count as 8 points.
            (2_000.0, 3_000.0, 0, 20.0, 0.0, 380.0, 380.0),
            (330.0, 620.0, 0, f32::NAN, 0.0, 1_200.0, 330.0),
        ] {
            let got = side_in(w, h, lines, row, eval, limit, auto);
            assert!((got - side).abs() < 0.01, "{w} × {h}, {lines} lines, rows of {row}: {got}, not {side}");
        }
    }

    /// A side the user chose is the board's while the column has room for it and three rows of
    /// moves, larger or smaller than the automatic one; beyond, the column's width, the three rows
    /// or the limit stop it; never below the smallest board.
    #[test]
    fn a_chosen_side_is_honoured_and_capped_by_the_width_and_three_rows() {
        let chosen = BoardSide::Chosen;
        // The user's window, 340 wide: automatic 152, three rows of 22 leave 306.
        assert_eq!(side_in(340.0, 480.0, 0, 22.0, 0.0, 1_200.0, chosen(280.0)), 280.0);
        assert_eq!(side_in(340.0, 480.0, 0, 22.0, 0.0, 1_200.0, chosen(120.0)), 120.0);
        let r = Regions::column(Rect::from_min_size(pos2(0.0, 0.0), vec2(340.0, 480.0)), of(0, 22.0, 0.0, 1_200.0), chosen(1_000.0));
        assert_eq!(r.board.width(), 306.0, "three rows of moves stop it");
        assert!((tall(r.moves) - Regions::least_rows(ROWS_CHOSEN, 22.0)).abs() < 0.01, "three rows left: {:?}", r.moves);
        assert!((r.bar.max.y - 480.0).abs() < 0.01, "the bar at the foot: {r:?}");
        // With the engine's three lines and its bar: 480 − 160 − 74.
        assert_eq!(side_in(340.0, 480.0, 3, 22.0, 18.0, 1_200.0, chosen(1_000.0)), 246.0);
        // The width stops it, and the limit.
        assert_eq!(side_in(250.0, 900.0, 0, 22.0, 0.0, 1_200.0, chosen(600.0)), 250.0);
        assert_eq!(side_in(250.0, 900.0, 0, 22.0, 30.0, 1_200.0, chosen(600.0)), 220.0);
        assert_eq!(side_in(2_000.0, 3_000.0, 0, 20.0, 0.0, 380.0, chosen(900.0)), 380.0);
        // Never below the smallest board while there is room for one, whatever is asked.
        assert_eq!(side_in(340.0, 480.0, 0, 22.0, 0.0, 1_200.0, chosen(10.0)), SMALLEST_BOARD);
        assert_eq!(side_in(340.0, 480.0, 0, 22.0, 0.0, 1_200.0, BoardSide::chosen(10.0)), SMALLEST_BOARD);
        // Not a number is automatic; infinity the most the column gives.
        assert_eq!(side_in(340.0, 480.0, 0, 22.0, 0.0, 1_200.0, chosen(f32::NAN)), 152.0);
        assert_eq!(side_in(340.0, 480.0, 0, 22.0, 0.0, 1_200.0, chosen(f32::INFINITY)), 306.0);
        // Over many columns: square, inside the column, and three rows of moves unless the board
        // is at its smallest.
        for w in [0.0, 50.0, 96.0, 200.0, 340.0, 1_000.0] {
            for h in [0.0, 100.0, 250.0, 480.0, 615.0, 2_000.0] {
                for c in [0.0, 96.0, 150.0, 300.0, 700.0, 1_200.0, f32::INFINITY] {
                    let rect = Rect::from_min_size(pos2(5.0, 7.0), vec2(w, h));
                    let r = Regions::column(rect, of(0, 22.0, 0.0, 1_200.0), chosen(c));
                    assert!(r.board.max.x <= rect.max.x + 0.01 && (r.board.width() - r.board.height()).abs() < 0.01, "{w} {h} {c}: {r:?}");
                    let three = Regions::least_rows(ROWS_CHOSEN, 22.0);
                    assert!(tall(r.moves) >= three - 0.01 || r.board.width() <= SMALLEST_BOARD + 0.01, "{w} {h} {c}: {r:?}");
                }
            }
        }
    }

    /// What the grip and the board's menu work with: the least and the most a chosen side gets,
    /// steps of about 10 % within them, and a drag that follows the pointer within them.
    #[test]
    fn the_sizes_step_and_drag_within_the_column() {
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(340.0, 480.0));
        let sizes = Sizes::tab(rect, of(0, 22.0, 0.0, 1_200.0), BoardSide::Auto);
        assert_eq!(sizes, Sizes { side: 152.0, least: SMALLEST_BOARD, most: 306.0, asked: BoardSide::Auto });
        assert!(sizes.can_grow() && sizes.can_shrink());
        assert!((sizes.step(true) - 167.2).abs() < 0.01 && (sizes.step(false) - 152.0 / 1.1).abs() < 0.01);
        assert_eq!(sizes.dragged(152.0, 60.0), 212.0);
        assert_eq!((sizes.dragged(152.0, 500.0), sizes.dragged(152.0, -500.0)), (306.0, SMALLEST_BOARD));
        assert_eq!(sizes.dragged(f32::NAN, f32::INFINITY), SMALLEST_BOARD);
        // At the most, Larger board has nothing to do; at the least, Smaller board.
        let most = Sizes::tab(rect, of(0, 22.0, 0.0, 1_200.0), BoardSide::Chosen(f32::INFINITY));
        assert!(!most.can_grow() && most.can_shrink() && most.step(true) == 306.0);
        let least = Sizes::tab(rect, of(0, 22.0, 0.0, 1_200.0), BoardSide::chosen(0.0));
        assert!(!least.can_shrink() && least.step(false) == SMALLEST_BOARD);
        // A column too narrow for the smallest board: one size, nothing to step.
        let narrow = Sizes::tab(Rect::from_min_size(pos2(0.0, 0.0), vec2(80.0, 600.0)), of(0, 22.0, 0.0, 1_200.0), BoardSide::Auto);
        assert!(narrow.least == 80.0 && narrow.most == 80.0 && !narrow.can_grow() && !narrow.can_shrink());
    }

    /// The settings' `chess_board_size` and the control channel's `board-side`: null, absent or
    /// not a number is automatic; a number is kept within 96..=2,400 points, so a hostile settings
    /// file makes neither a NaN nor a board of 10⁹ points; what is written reads back the same.
    #[test]
    fn the_chosen_side_is_read_within_its_bounds() {
        use serde_json::json;
        let read = |v: serde_json::Value| BoardSide::from_json(&v);
        assert_eq!(read(serde_json::Value::Null), BoardSide::Auto);
        assert_eq!(read(json!({})["chess_board_size"].clone()), BoardSide::Auto);
        assert_eq!(read(json!("300")), BoardSide::Auto);
        assert_eq!(read(json!(true)), BoardSide::Auto);
        assert_eq!(read(json!(250)), BoardSide::Chosen(250.0));
        assert_eq!(read(json!(1e300)), BoardSide::Chosen(LARGEST_CHOSEN));
        assert_eq!(read(json!(1e9)), BoardSide::Chosen(LARGEST_CHOSEN));
        assert_eq!(read(json!(-1e300)), BoardSide::Chosen(SMALLEST_BOARD));
        assert_eq!(read(json!(1e-30)), BoardSide::Chosen(SMALLEST_BOARD));
        for side in [BoardSide::Auto, BoardSide::Chosen(96.0), BoardSide::Chosen(233.4), BoardSide::Chosen(1_200.0), BoardSide::Largest] {
            assert_eq!(read(side.to_json()), side);
        }
        assert_eq!(BoardSide::Chosen(f32::NAN).to_json(), serde_json::Value::Null);
        assert_eq!(BoardSide::chosen(f32::NAN), BoardSide::Auto);
        assert_eq!(BoardSide::parse("auto"), Some(BoardSide::Auto));
        assert_eq!(BoardSide::parse(" largest "), Some(BoardSide::Largest));
        assert_eq!(read(json!("largest")), BoardSide::Largest);
        assert_eq!(BoardSide::parse(" 300 "), Some(BoardSide::Chosen(300.0)));
        assert_eq!(BoardSide::parse("1e9"), Some(BoardSide::Chosen(LARGEST_CHOSEN)));
        for bad in ["", "big", "NaN", "inf", "-inf"] {
            assert_eq!(BoardSide::parse(bad), None, "{bad:?}");
        }
    }

    /// The largest board (2026-11-25): while only the moves would show under it, it takes their
    /// room too, and it grows with the panel; with something open there, three rows stay.
    #[test]
    fn the_largest_board_takes_all_the_panel_allows() {
        let none = Measures { kept: 0, ..of(0, 20.0, 0.0, 2_400.0) };
        let side = |w, h, m| Regions::tab(Rect::from_min_size(pos2(3.0, 4.0), vec2(w, h)), m, BoardSide::Largest);
        // As wide as the panel while it is tall.
        assert_eq!(side(500.0, 900.0, none).0.board.width(), 500.0);
        // As tall as its header, navigation, trail and bar leave: 600 − 100.
        let (r, layout) = side(550.0, 600.0, none);
        assert_eq!((r.board.width(), layout), (500.0, Arrangement::Column));
        assert!(tall(r.moves) < ROW_LEAST && (r.bar.max.y - 604.0).abs() < 0.01, "{r:?}");
        // Something open in the moves' place: three rows of it.
        assert_eq!(side(550.0, 600.0, of(0, 20.0, 0.0, 2_400.0)).0.board.width(), 432.0);
        // A panel wider than tall: the moves beside it, the board to the panel's foot.
        let (r, layout) = side(1_400.0, 800.0, none);
        assert_eq!((r.board.width(), layout), (800.0 - HEADER - NAV, Arrangement::Beside));
        assert!(r.moves.width() >= BESIDE_MOVES && r.moves.min.x >= r.board.max.x && r.bar.max.x <= 1_403.01, "{r:?}");
        // A 4K screen's height.
        assert_eq!(side(3_800.0, 2_100.0, none).0.board.width(), 2_100.0 - HEADER - NAV);
        // A side dragged as far as the panel goes is kept as the largest, one short of it is not.
        let sizes = Sizes::tab(Rect::from_min_size(pos2(0.0, 0.0), vec2(500.0, 900.0)), none, BoardSide::Auto);
        assert_eq!((sizes.pick(sizes.dragged(300.0, 900.0)), sizes.pick(320.0)), (BoardSide::Largest, BoardSide::Chosen(320.0)));
    }

    /// Automatic in a panel wider than tall: the moves beside the board, which is larger so; a
    /// narrow panel keeps its one column whatever its height.
    #[test]
    fn a_wide_panel_puts_the_moves_beside_the_board() {
        let tab = |w, h, side| Regions::tab(Rect::from_min_size(pos2(0.0, 0.0), vec2(w, h)), of(0, 20.0, 0.0, 2_400.0), side);
        let (r, layout) = tab(1_000.0, 600.0, BoardSide::Auto);
        assert_eq!((r.board.width(), layout), (544.0, Arrangement::Beside));
        // A smaller side chosen there stays beside the moves, and its header keeps its width.
        let (r, layout) = tab(1_000.0, 600.0, BoardSide::Chosen(150.0));
        assert_eq!((r.board.width(), r.header.width(), layout), (150.0, BESIDE_BOARD, Arrangement::Beside));
        assert_eq!(tab(340.0, 480.0, BoardSide::Auto).1, Arrangement::Column);
        assert_eq!(tab(565.0, 300.0, BoardSide::Largest).1, Arrangement::Column);
        // Tall: one column, as before.
        let (r, layout) = tab(700.0, 1_000.0, BoardSide::Auto);
        assert_eq!((r.board.width(), layout), (700.0, Arrangement::Column));
        // Wide and about as tall (the user's Inspector): four rows of moves under the board, where
        // ten would leave 552.
        let (r, layout) = tab(838.0, 860.0, BoardSide::Auto);
        assert_eq!((r.board.width(), layout), (672.0, Arrangement::Column));
        assert_eq!(tall(r.moves), Regions::least_rows(ROWS_WIDE, 20.0));
        // Every size: inside the panel, square, never smaller than one column gives.
        for w in [0.0, 300.0, 571.0, 800.0, 1_500.0] {
            for h in [0.0, 200.0, 450.0, 900.0] {
                for side in CHOSEN {
                    let (r, _) = tab(w, h, side);
                    let column = Regions::column(Rect::from_min_size(pos2(0.0, 0.0), vec2(w, h)), of(0, 20.0, 0.0, 2_400.0), side);
                    assert!(r.board.max.x <= w + 0.01 && r.moves.max.x <= w + 0.01, "{w} {h} {side:?}: {r:?}");
                    assert!(r.board.width() + 0.51 >= column.board.width(), "{w} {h} {side:?}: {r:?}");
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
