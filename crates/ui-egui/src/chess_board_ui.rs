//! Chess ▸ Board: an analysis board in its own panel, beside the Inspector.
//!
//! A diagram goes onto it from the page (double-click the diagram) or from the Chess diagrams
//! panel (Open on the board). Moves are played by clicking a piece and then where it goes, or by
//! dragging it there; the board keeps the line played, which the arrows (or the arrow keys, with
//! the pointer over the panel) walk back and forth. A move made at an earlier point replaces the
//! rest of the line. The position is also a FEN field and the game a PGN field: both can be
//! copied, or edited and loaded back.
//!
//! The rules are `pdfcraft_chess::game` (L0); this file only shows them. A diagram read from a
//! book can be illegal (a misread square): it is shown as it is, and no move can be played on it
//! until it is corrected in the diagram editor.

use egui::{Color32, Key, Pos2, Rect, Stroke, TextureHandle, vec2};
use pdfcraft_engine::DocId;
use pdfcraft_engine::chess::pdfcraft_chess::draw::{BoardStyle, PieceSet};
use pdfcraft_engine::chess::pdfcraft_chess::game::{Game, GameError, Outcome};
use pdfcraft_engine::chess::pdfcraft_chess::placement::{Color, Piece, PieceKind, Placement, Square};

use crate::theme::{self, Tokens};

/// The board image's side, in pixels (it is scaled to the panel).
const BOARD_PX: u32 = 640;

/// Largest board on screen, in points.
const MAX_BOARD_PT: f32 = 480.0;

/// Where the position on the board came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Source {
    pub doc: DocId,
    /// Page (0-based) and the diagram's index on it.
    pub page: usize,
    pub index: usize,
}

/// The board panel's state.
pub struct BoardState {
    pub game: Game,
    /// A position that cannot be played (not legal): shown as it is, with its FEN.
    pub illegal: Option<(Placement, String)>,
    pub source: Option<Source>,
    pub flipped: bool,
    /// The piece picked up (by a click or a drag).
    pub selected: Option<Square>,
    /// A pawn move to the last rank waiting for its piece.
    pub promotion: Option<(Square, Square)>,
    /// The FEN and PGN fields, as typed.
    pub fen_text: String,
    pub pgn_text: String,
    /// What the fields were last filled from, so typing in them is not overwritten.
    shown: String,
    /// Why the last load or move did not work.
    pub message: Option<String>,
    texture: Option<(String, TextureHandle)>,
    draw_error: Option<String>,
}

impl Default for BoardState {
    fn default() -> BoardState {
        BoardState {
            game: Game::standard(),
            illegal: None,
            source: None,
            flipped: false,
            selected: None,
            promotion: None,
            fen_text: String::new(),
            pgn_text: String::new(),
            shown: String::new(),
            message: None,
            texture: None,
            draw_error: None,
        }
    }
}

/// What the board panel asks the app for.
#[derive(Clone, Debug, PartialEq)]
pub enum BoardAction {
    CopyFen(String),
    CopyPgn(String),
    OpenLichess(String),
    /// Go to the diagram the board was loaded from.
    ShowSource(Source),
}

impl BoardState {
    /// Put the position `fen` on the board, from Black's side when `flipped`. A position that is
    /// not legal is shown but cannot be played.
    /// A FEN that cannot be read changes nothing but the message (the field keeps what was typed).
    pub fn load_fen(&mut self, fen: &str, flipped: bool, source: Option<Source>) {
        let (game, illegal, message) = match Game::from_fen(fen) {
            Ok(game) => (game, None, None),
            Err(GameError::BadFen(e)) => {
                self.message = Some(crate::i18n::fmt(tl!("The FEN couldn't be read: {e}"), &[("e", &e)]));
                return;
            }
            Err(_) => match pdfcraft_engine::chess::pdfcraft_chess::fen::parse_placement(fen) {
                Ok(placement) => (
                    Game::standard(),
                    Some((placement, fen.trim().to_owned())),
                    Some(tl!("Not a legal position: correct it in the diagram editor to play moves on it.").to_string()),
                ),
                Err(e) => {
                    self.message = Some(crate::i18n::fmt(tl!("The FEN couldn't be read: {e}"), &[("e", &e.to_string())]));
                    return;
                }
            },
        };
        self.game = game;
        self.illegal = illegal;
        self.message = message;
        self.selected = None;
        self.promotion = None;
        self.flipped = flipped;
        self.source = source;
        self.refresh_fields(true);
    }

    /// Read the PGN field into the board.
    fn load_pgn(&mut self) {
        match Game::from_pgn(&self.pgn_text) {
            Ok((game, stopped)) => {
                self.game = game;
                self.illegal = None;
                self.selected = None;
                self.promotion = None;
                self.message = stopped.map(|m| crate::i18n::fmt(tl!("Reading stopped at {move}: it is not a legal move there."), &[("move", &m)]));
                self.refresh_fields(true);
            }
            Err(e) => self.message = Some(crate::i18n::fmt(tl!("The PGN couldn't be read: {e}"), &[("e", &e.to_string())])),
        }
    }

    /// Fill the FEN and PGN fields from the board, when it changed (`force`: always).
    fn refresh_fields(&mut self, force: bool) {
        let key = match &self.illegal {
            Some((_, fen)) => format!("illegal|{fen}"),
            None => format!("{}|{}|{}", self.game.start_fen(), self.game.movetext(), self.game.ply()),
        };
        if force || key != self.shown {
            self.fen_text = self.current_fen();
            self.pgn_text = if self.illegal.is_some() { String::new() } else { self.game.pgn() };
            self.shown = key;
        }
    }

    /// The FEN of the board shown (an illegal position's as it was given).
    fn current_fen(&self) -> String {
        self.illegal.as_ref().map_or_else(|| self.game.fen(), |(_, fen)| fen.clone())
    }

    /// The pieces shown.
    fn placement(&self) -> Placement {
        self.illegal.as_ref().map_or_else(|| self.game.placement(), |(p, _)| *p)
    }

    /// The square at `(x, y)` in the board's own units (0..1 from its top left), as shown.
    fn square_at(&self, x: f32, y: f32) -> Option<Square> {
        if !(x.is_finite() && y.is_finite() && (0.0..1.0).contains(&x) && (0.0..1.0).contains(&y)) {
            return None;
        }
        // In 0..8 after the checks above, so the casts cannot wrap.
        let (col, row) = ((x * 8.0) as u8, (y * 8.0) as u8);
        if self.flipped { Square::new(7 - col, row) } else { Square::new(col, 7 - row) }
    }

    /// The rectangle of `square` on a board drawn in `board`.
    fn square_rect(&self, board: Rect, square: Square) -> Rect {
        let (file, rank) = (f32::from(square.file()), f32::from(square.rank()));
        let (col, row) = if self.flipped { (7.0 - file, rank) } else { (file, 7.0 - rank) };
        let cell = board.width() / 8.0;
        Rect::from_min_size(board.min + vec2(col * cell, row * cell), vec2(cell, cell))
    }

    /// A click (or a drop) on `square`: pick a piece of the side to move up, or move the piece
    /// picked up there.
    fn click(&mut self, square: Square) {
        if self.illegal.is_some() {
            return;
        }
        let own = self.game.piece_on(square).is_some_and(|p| p.color == self.game.side_to_move());
        match self.selected {
            Some(from) if from == square => self.selected = None,
            Some(from) if self.game.targets(from).contains(&square) => self.play(from, square, None),
            _ => self.selected = own.then_some(square),
        }
    }

    fn play(&mut self, from: Square, to: Square, promotion: Option<PieceKind>) {
        self.selected = None;
        match self.game.play(from, to, promotion) {
            Ok(()) => {
                self.promotion = None;
                self.message = None;
            }
            Err(GameError::NeedsPromotion) => self.promotion = Some((from, to)),
            Err(e) => self.message = Some(e.to_string()),
        }
    }

    /// Step through the line: `Some(true)` forward, `Some(false)` back.
    fn step(&mut self, forward: bool) {
        self.selected = None;
        self.promotion = None;
        if forward {
            self.game.forward();
        } else {
            self.game.back();
        }
    }

    /// Whose move it is at the start, set before any move is played. Fails (and changes nothing)
    /// when that side cannot move first: the other king would be in check.
    fn set_first_side(&mut self, side: Color) {
        let start = self.game.start_fen();
        let mut fields: Vec<&str> = start.split_whitespace().collect();
        let Some(slot) = fields.get_mut(1) else { return };
        *slot = if side == Color::White { "w" } else { "b" };
        // The en passant square belongs to the side that was to move.
        if let Some(ep) = fields.get_mut(3) {
            *ep = "-";
        }
        match Game::from_fen(&fields.join(" ")) {
            Ok(game) => {
                self.game = game;
                self.message = None;
                self.selected = None;
            }
            Err(_) => self.message = Some(tl!("That side can't be to move: the other king is in check.").to_string()),
        }
    }

    /// The board image, redrawn when the pieces or the view change.
    fn texture(&mut self, ctx: &egui::Context, pieces: &PieceSet) -> Option<TextureHandle> {
        let placement = self.placement();
        let key = format!("{}|{}", pdfcraft_engine::chess::pdfcraft_chess::fen::format_placement(&placement), self.flipped);
        if let Some((k, tex)) = &self.texture
            && *k == key
        {
            return Some(tex.clone());
        }
        let style = BoardStyle { border: 0.0, coordinates: false, flipped: self.flipped, ..BoardStyle::default() };
        match pdfcraft_engine::chess::office::diagram_rgba(&placement, pieces, &style, BOARD_PX) {
            Ok((w, h, rgba)) => {
                let (w, h) = (usize::try_from(w).ok()?, usize::try_from(h).ok()?);
                if rgba.len() != w.checked_mul(h)?.checked_mul(4)? {
                    return None;
                }
                let image = egui::ColorImage::from_rgba_premultiplied([w, h], &rgba);
                let tex = ctx.load_texture("chess-analysis-board", image, egui::TextureOptions::LINEAR);
                self.texture = Some((key, tex.clone()));
                self.draw_error = None;
                Some(tex)
            }
            Err(e) => {
                self.draw_error = Some(e);
                None
            }
        }
    }
}

/// The colours drawn over the board.
const LAST_MOVE: Color32 = Color32::from_rgba_premultiplied(155, 199, 0, 105);
const PICKED: Color32 = Color32::from_rgba_premultiplied(20, 85, 30, 110);
const TARGET: Color32 = Color32::from_rgba_premultiplied(20, 85, 30, 120);
const CHECK: Color32 = Color32::from_rgb(0xE0, 0x3E, 0x3E);

/// The board itself, with its marks, taking clicks and drags.
fn board_view(ui: &mut egui::Ui, t: &Tokens, state: &mut BoardState, pieces: &PieceSet) {
    let side = ui.available_width().clamp(160.0, MAX_BOARD_PT);
    let Some(tex) = state.texture(ui.ctx(), pieces) else {
        if let Some(e) = &state.draw_error {
            ui.label(egui::RichText::new(crate::i18n::fmt(tl!("The board couldn't be drawn: {e}"), &[("e", e)])).small().color(t.text_muted));
        }
        return;
    };
    let response = ui.add(egui::Image::new(&tex).fit_to_exact_size(vec2(side, side)).sense(egui::Sense::click_and_drag()));
    let board_name = crate::i18n::fmt(tl!("Board: {fen}"), &[("fen", &state.current_fen())]);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, &board_name));
    let rect = response.rect;
    let at = |state: &BoardState, p: Pos2| state.square_at((p.x - rect.min.x) / rect.width().max(1.0), (p.y - rect.min.y) / rect.height().max(1.0));
    if response.drag_started()
        && let Some(sq) = response.interact_pointer_pos().and_then(|p| at(state, p))
    {
        // Dragging picks the piece up; dropping it on its own square leaves it picked.
        if state.selected != Some(sq) {
            state.click(sq);
        }
    }
    if response.drag_stopped()
        && let (Some(from), Some(to)) = (state.selected, response.interact_pointer_pos().and_then(|p| at(state, p)))
        && from != to
    {
        state.click(to);
    } else if response.clicked()
        && let Some(sq) = response.interact_pointer_pos().and_then(|p| at(state, p))
    {
        state.click(sq);
    }
    let painter = ui.painter_at(rect);
    if state.illegal.is_none() {
        if let Some((from, to)) = state.game.last_move() {
            painter.rect_filled(state.square_rect(rect, from), 0.0, LAST_MOVE);
            painter.rect_filled(state.square_rect(rect, to), 0.0, LAST_MOVE);
        }
        if state.game.in_check()
            && let Some(king) = state.game.placement().king(state.game.side_to_move())
        {
            let r = state.square_rect(rect, king);
            painter.circle_stroke(r.center(), r.width() * 0.45, Stroke::new(3.0, CHECK));
        }
        if let Some(from) = state.selected {
            painter.rect_filled(state.square_rect(rect, from), 0.0, PICKED);
            for to in state.game.targets(from) {
                let r = state.square_rect(rect, to);
                if state.game.piece_on(to).is_some() {
                    painter.circle_stroke(r.center(), r.width() * 0.44, Stroke::new(r.width() * 0.08, TARGET));
                } else {
                    painter.circle_filled(r.center(), r.width() * 0.15, TARGET);
                }
            }
        }
        // The square a dragged piece would land on.
        if response.dragged()
            && let Some(sq) = response.interact_pointer_pos().and_then(|p| at(state, p))
        {
            painter.rect_stroke(state.square_rect(rect, sq), 0.0, Stroke::new(2.0, Color32::WHITE), egui::StrokeKind::Inside);
        }
    }
    // Coordinates in the corner squares, as on an analysis board.
    let font = theme::semibold((side / 40.0).clamp(8.0, 12.0));
    for i in 0u8..8 {
        let (file_sq, rank_sq) =
            if state.flipped { (Square::new(7 - i, 7), Square::new(0, 7 - i)) } else { (Square::new(i, 0), Square::new(7, 7 - i)) };
        let ink = |sq: Square| if sq.is_light() { Color32::from_rgb(181, 136, 99) } else { Color32::from_rgb(240, 217, 181) };
        if let Some(sq) = file_sq {
            let r = state.square_rect(rect, sq);
            let letter = char::from(b'a'.saturating_add(sq.file())).to_string();
            painter.text(r.left_bottom() + vec2(2.0, -1.0), egui::Align2::LEFT_BOTTOM, letter, font.clone(), ink(sq));
        }
        if let Some(sq) = rank_sq {
            let r = state.square_rect(rect, sq);
            painter.text(r.right_top() + vec2(-2.0, 1.0), egui::Align2::RIGHT_TOP, (sq.rank() + 1).to_string(), font.clone(), ink(sq));
        }
    }
    if state.illegal.is_some() {
        painter.rect_stroke(rect, 0.0, Stroke::new(2.0, CHECK), egui::StrokeKind::Inside);
    }
}

/// The game's status line: whose move it is, check, mate.
fn status(state: &BoardState) -> (String, Color32) {
    if state.illegal.is_some() {
        return (tl!("Not a legal position").to_string(), CHECK);
    }
    let side = state.game.side_to_move();
    let fine = Color32::from_rgb(0x2D, 0x9D, 0x5B);
    match state.game.outcome() {
        Outcome::Checkmate => (if side == Color::White { tl!("Checkmate: Black wins") } else { tl!("Checkmate: White wins") }.to_string(), CHECK),
        Outcome::Stalemate => (tl!("Stalemate").to_string(), fine),
        Outcome::FiftyMoves => (tl!("Draw by the fifty-move rule").to_string(), fine),
        Outcome::Ongoing if state.game.in_check() => {
            (if side == Color::White { tl!("White to move, in check") } else { tl!("Black to move, in check") }.to_string(), CHECK)
        }
        Outcome::Ongoing => (if side == Color::White { tl!("White to move") } else { tl!("Black to move") }.to_string(), fine),
    }
}

/// The move list: numbered, the move shown highlighted, each one a click away.
fn move_list(ui: &mut egui::Ui, t: &Tokens, state: &mut BoardState) {
    if state.illegal.is_some() {
        return;
    }
    let moves = state.game.moves();
    if moves.is_empty() {
        ui.label(egui::RichText::new(tl!("Play a move: click a piece, then where it goes, or drag it there.")).small().color(t.text_muted));
        return;
    }
    let mut go = None;
    egui::ScrollArea::vertical().id_salt("chess-board-moves").max_height(150.0).auto_shrink([false, true]).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (k, (ply, number, color, san)) in moves.iter().enumerate() {
                match color {
                    Color::White => {
                        ui.label(egui::RichText::new(format!("{number}.")).small().color(t.text_faint));
                    }
                    Color::Black if k == 0 => {
                        ui.label(egui::RichText::new(format!("{number}...")).small().color(t.text_faint));
                    }
                    Color::Black => {}
                }
                let shown = *ply == state.game.ply();
                if ui.selectable_label(shown, egui::RichText::new(*san).monospace()).clicked() {
                    go = Some(*ply);
                }
            }
        });
    });
    if let Some(ply) = go {
        state.selected = None;
        state.promotion = None;
        state.game.go_to(ply);
    }
}

/// The Chess board panel. `source_label` names the diagram the board was loaded from.
pub fn panel(ui: &mut egui::Ui, t: &Tokens, state: &mut BoardState, pieces: &PieceSet, source_label: Option<String>) -> Option<BoardAction> {
    let mut action = None;
    // The arrow keys walk the line while the pointer is over the panel and no field has the keyboard.
    if ui.rect_contains_pointer(ui.max_rect()) && !ui.ctx().egui_wants_keyboard_input() {
        let keys = ui.input_mut(|i| [Key::ArrowLeft, Key::ArrowRight, Key::Home, Key::End].map(|k| i.consume_key(egui::Modifiers::NONE, k)));
        match keys {
            [true, ..] => state.step(false),
            [_, true, ..] => state.step(true),
            [_, _, true, _] => state.game.go_to(0),
            [.., true] => state.game.go_to(usize::MAX),
            _ => {}
        }
    }
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tl!("Chess board")).font(theme::semibold(15.5)));
        if let (Some(label), Some(source)) = (source_label, state.source)
            && ui.link(egui::RichText::new(label).small()).on_hover_text(tl!("Show this diagram on the page")).clicked()
        {
            action = Some(BoardAction::ShowSource(source));
        }
    });
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        if ui.small_button(tl!("Flip board")).clicked() {
            state.flipped = !state.flipped;
        }
        // Whose move it is can be changed before the first move.
        ui.add_enabled_ui(state.illegal.is_none() && state.game.is_empty(), |ui| {
            let mut side = state.game.side_to_move();
            ui.label(tl!("To move:"));
            crate::widgets::radio_value(ui, &mut side, Color::White, tl!("White"));
            crate::widgets::radio_value(ui, &mut side, Color::Black, tl_ctx!("chess side", "Black"));
            if side != state.game.side_to_move() {
                state.set_first_side(side);
            }
        });
    });
    ui.add_space(4.0);
    board_view(ui, t, state, pieces);
    if let Some((from, to)) = state.promotion {
        ui.horizontal(|ui| {
            ui.label(tl!("Promote to:"));
            let color = state.game.side_to_move();
            for kind in [PieceKind::Queen, PieceKind::Rook, PieceKind::Bishop, PieceKind::Knight] {
                let name = crate::chess_editor::piece_name(Piece { color, kind });
                if ui.small_button(name).clicked() {
                    state.play(from, to, Some(kind));
                }
            }
            if ui.small_button("✕").on_hover_text(tl!("Cancel")).clicked() {
                state.promotion = None;
            }
        });
    }
    // Navigation.
    ui.horizontal(|ui| {
        let (ply, len) = (state.game.ply(), state.game.len());
        let nav = |ui: &mut egui::Ui, on: bool, text: &str, tip: &str| {
            ui.add_enabled(on, egui::Button::new(text).min_size(vec2(44.0, 24.0))).on_hover_text(tip).clicked()
        };
        if nav(ui, ply > 0, "⏮", tl!("Start (Home)")) {
            state.game.go_to(0);
            state.selected = None;
        }
        if nav(ui, ply > 0, "◀", tl!("Previous move (Left arrow)")) {
            state.step(false);
        }
        if nav(ui, ply < len, "▶", tl!("Next move (Right arrow)")) {
            state.step(true);
        }
        if nav(ui, ply < len, "⏭", tl!("End (End)")) {
            state.game.go_to(usize::MAX);
            state.selected = None;
        }
    });
    let (text, colour) = status(state);
    ui.label(egui::RichText::new(text).small().color(colour));
    if let Some(m) = &state.message {
        ui.label(egui::RichText::new(m).small().color(t.text_muted));
    }
    ui.add_space(4.0);
    move_list(ui, t, state);
    if state.illegal.is_none() && state.game.ply() < state.game.len() && ui.small_button(tl!("Delete the moves after this one")).clicked() {
        state.game.truncate();
    }
    ui.add_space(6.0);
    state.refresh_fields(false);
    // FEN.
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("FEN").small().strong());
        let r = ui.add(egui::TextEdit::singleline(&mut state.fen_text).font(egui::TextStyle::Monospace).desired_width(f32::INFINITY));
        if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            let fen = state.fen_text.clone();
            let flipped = state.flipped;
            state.load_fen(&fen, flipped, None);
        }
    });
    ui.horizontal_wrapped(|ui| {
        if ui.small_button(tl!("Copy FEN")).clicked() {
            action = Some(BoardAction::CopyFen(state.current_fen()));
        }
        if state.illegal.is_none()
            && let Ok(p) = pdfcraft_engine::chess::pdfcraft_chess::fen::parse(&state.game.fen())
        {
            let url = pdfcraft_engine::chess::pdfcraft_chess::lichess::analysis_url(&p, if state.flipped { Color::Black } else { Color::White });
            if ui.small_button(tl!("Open in lichess")).on_hover_text(&url).clicked() {
                action = Some(BoardAction::OpenLichess(url));
            }
        }
        if ui.small_button(tl!("Standard start position")).clicked() {
            state.game = Game::standard();
            state.illegal = None;
            state.source = None;
            state.selected = None;
            state.promotion = None;
            state.message = None;
        }
    });
    ui.add_space(4.0);
    // PGN.
    ui.label(egui::RichText::new("PGN").small().strong());
    ui.add(egui::TextEdit::multiline(&mut state.pgn_text).font(egui::TextStyle::Monospace).desired_rows(5).desired_width(f32::INFINITY));
    ui.horizontal_wrapped(|ui| {
        if ui.add_enabled(state.illegal.is_none(), egui::Button::new(tl!("Copy PGN")).small()).clicked() {
            action = Some(BoardAction::CopyPgn(state.game.pgn()));
        }
        if ui.small_button(tl!("Load PGN")).on_hover_text(tl!("Play the main line of the PGN above on the board.")).clicked() {
            state.load_pgn();
        }
    });
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sq(name: &str) -> Square {
        Square::parse(name).unwrap()
    }

    #[test]
    fn clicks_pick_up_and_move_only_the_side_to_move() {
        let mut s = BoardState::default();
        s.click(sq("e7"));
        assert_eq!(s.selected, None, "Black's pawn is not White's to move");
        s.click(sq("e2"));
        assert_eq!(s.selected, Some(sq("e2")));
        s.click(sq("e5"));
        assert_eq!(s.game.len(), 0, "e2-e5 is not a move: the click on an empty square drops the piece");
        assert_eq!(s.selected, None);
        s.click(sq("e2"));
        s.click(sq("e4"));
        assert_eq!(s.game.movetext(), "1. e4");
        // Picking up another piece of the side to move switches to it.
        s.click(sq("g8"));
        s.click(sq("b8"));
        assert_eq!(s.selected, Some(sq("b8")));
    }

    #[test]
    fn promotion_waits_for_its_piece() {
        let mut s = BoardState::default();
        s.load_fen("8/P6k/8/8/8/8/8/K7 w - - 0 1", false, None);
        s.click(sq("a7"));
        s.click(sq("a8"));
        assert_eq!(s.promotion, Some((sq("a7"), sq("a8"))));
        s.play(sq("a7"), sq("a8"), Some(PieceKind::Rook));
        assert_eq!(s.game.movetext(), "1. a8=R");
        assert_eq!(s.promotion, None);
    }

    #[test]
    fn an_illegal_diagram_is_shown_but_not_played() {
        let mut s = BoardState::default();
        s.load_fen("K7/8/8/8/8/8/8/K6k w - - 0 1", false, None);
        assert!(s.illegal.is_some());
        assert_eq!(s.placement().count_color(Color::White), 2);
        s.click(sq("a8"));
        assert_eq!(s.selected, None);
        assert_eq!(s.fen_text, "K7/8/8/8/8/8/8/K6k w - - 0 1");
        // A good FEN afterwards takes over.
        s.load_fen("4k3/8/8/8/8/8/8/4K3 b - - 0 1", true, None);
        assert!(s.illegal.is_none());
        assert_eq!(s.game.side_to_move(), Color::Black);
    }

    #[test]
    fn a_bad_fen_keeps_the_board() {
        let mut s = BoardState::default();
        s.click(sq("d2"));
        s.click(sq("d4"));
        s.load_fen("this is not a fen", false, None);
        assert!(s.message.is_some());
        assert_eq!(s.game.movetext(), "1. d4");
    }

    #[test]
    fn the_first_side_can_be_switched_only_when_it_may_move() {
        let mut s = BoardState::default();
        s.load_fen("4k3/8/8/8/8/8/8/R3K3 w - - 0 1", false, None);
        s.set_first_side(Color::Black);
        assert_eq!(s.game.side_to_move(), Color::Black);
        // Black in check: White cannot be to move.
        s.load_fen("4k3/8/8/8/8/8/8/4R1K1 b - - 0 1", false, None);
        s.set_first_side(Color::White);
        assert_eq!(s.game.side_to_move(), Color::Black);
        assert!(s.message.is_some());
    }

    #[test]
    fn squares_follow_the_view() {
        let mut s = BoardState::default();
        assert_eq!(s.square_at(0.01, 0.99), Some(sq("a1")));
        s.flipped = true;
        assert_eq!(s.square_at(0.01, 0.99), Some(sq("h8")));
        assert_eq!(s.square_at(1.0, 0.5), None);
        assert_eq!(s.square_at(f32::NAN, 0.5), None);
        let r = s.square_rect(Rect::from_min_size(Pos2::ZERO, vec2(80.0, 80.0)), sq("h8"));
        assert_eq!(r.min, Pos2::new(0.0, 70.0));
    }

    #[test]
    fn pgn_field_loads_and_reports_where_it_stopped() {
        let mut s = BoardState { pgn_text: "1. e4 e5 2. Qh5 Nc6 3. Bc4 Nf6 4. Qxf7# 1-0".into(), ..BoardState::default() };
        s.load_pgn();
        assert_eq!(s.game.outcome(), Outcome::Checkmate);
        assert!(s.message.is_none());
        s.pgn_text = "1. e4 e5 2. Ke3".into();
        s.load_pgn();
        assert_eq!(s.game.movetext(), "1. e4 e5");
        assert!(s.message.as_deref().is_some_and(|m| m.contains("Ke3")));
    }
}
