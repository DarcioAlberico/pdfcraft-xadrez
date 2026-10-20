//! Chess ▸ Board: an analysis board in its own panel, beside the Inspector.
//!
//! A diagram goes onto it from the page (double-click the diagram); Open on the board, in the
//! Chess diagrams panel, puts it on that panel's Study board tab, the same board. Moves are played by clicking a piece and then where it goes, or by
//! dragging it there; the arrows (or the arrow keys, with the pointer over the panel) walk the
//! game back and forth. A move made at an earlier point starts a variation. The move list shows
//! the variations, comments and annotation glyphs; a move's menu makes its line the main one or
//! deletes it. The move shown takes a glyph and a comment, and the game its details (the PGN
//! tags). The position is also a FEN field and the game a PGN field: both can be copied, or
//! edited and loaded back. The right button draws on the board, as on lichess: a click circles a
//! square and a drag draws an arrow (Shift red, Alt blue, Ctrl yellow, green otherwise); the
//! drawings belong to the position, and a left click on the board clears them.
//!
//! The Chess panel's Study board tab (`chess_study_ui`) shows this same board and game, built
//! from the parts below, and adds position set-up and engine analysis.
//!
//! The rules are `pdfcraft_chess::game` (L0); this file only shows them. A diagram read from a
//! book can be illegal (a misread square): it is shown as it is, and no move can be played on it
//! until it is corrected in the diagram editor.

use std::collections::HashMap;

use egui::{Color32, Key, PointerButton, Pos2, Rect, Stroke, TextureHandle, vec2};
use pdfcraft_engine::DocId;
use pdfcraft_engine::chess::pdfcraft_chess::draw::{BoardStyle, PieceSet, Rgb};
use pdfcraft_engine::chess::pdfcraft_chess::game::{Game, GameError, NodeId, Outcome, Token};
use pdfcraft_engine::chess::pdfcraft_chess::placement::{Color, Piece, PieceKind, Placement, Square};

use crate::chess_history::{ChessEdit, GameHistory, Snapshot};
use crate::theme::{self, Tokens};

/// The board image's side, in pixels (it is scaled to the panel).
const BOARD_PX: u32 = 640;

/// Largest board on screen, in points.
const MAX_BOARD_PT: f32 = 480.0;

/// Most positions that keep drawings; the oldest go first.
const MAX_MARKED_POSITIONS: usize = 512;

/// Most drawings on one position.
const MAX_MARKS: usize = 64;

/// The colours drawings come in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkColor {
    Green,
    Red,
    Blue,
    Yellow,
    /// The engine's best move (not drawn by hand).
    Engine,
}

impl MarkColor {
    /// The colour a right-button drawing gets from the keys held: Shift red, Alt blue, Ctrl (or
    /// Command) yellow, green otherwise.
    pub fn from_modifiers(m: egui::Modifiers) -> MarkColor {
        if m.shift {
            MarkColor::Red
        } else if m.alt {
            MarkColor::Blue
        } else if m.command || m.ctrl {
            MarkColor::Yellow
        } else {
            MarkColor::Green
        }
    }

    fn color32(self) -> Color32 {
        match self {
            MarkColor::Green => Color32::from_rgba_unmultiplied(21, 120, 27, 200),
            MarkColor::Red => Color32::from_rgba_unmultiplied(136, 32, 32, 200),
            MarkColor::Blue => Color32::from_rgba_unmultiplied(0, 48, 136, 200),
            MarkColor::Yellow => Color32::from_rgba_unmultiplied(230, 143, 0, 200),
            MarkColor::Engine => Color32::from_rgba_unmultiplied(0, 92, 175, 140),
        }
    }
}

/// An arrow from one square to another, or a circle when the two are the same.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mark {
    pub from: Square,
    pub to: Square,
    pub color: MarkColor,
}

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
    /// The pieces cut out of their squares, for the one following the pointer in a drag.
    sprites: Option<TextureHandle>,
    /// The comment or game-detail field being typed in. The game keeps comments and tags cleaned
    /// (spaces collapsed, ends trimmed), so a field re-read from it every frame would eat a space
    /// typed at the end.
    draft: Option<Draft>,
    /// The drawings, by position (its FEN), and the order the positions got their first one.
    marks: HashMap<String, Vec<Mark>>,
    marked_order: Vec<String>,
    /// Where a right-button drag started.
    drawing: Option<Square>,
    /// The game's undo and redo steps.
    pub(crate) history: GameHistory,
}

/// A comment or game-detail field while it has the keyboard: its key, its text as typed, and
/// whether this editing session is an undo step yet (a session is one step, not one per key).
struct Draft {
    key: String,
    text: String,
    recorded: bool,
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
            sprites: None,
            draft: None,
            marks: HashMap::new(),
            marked_order: Vec::new(),
            drawing: None,
            history: GameHistory::default(),
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
    /// One undo step: "Load diagram" with a `source`, "Load FEN" without.
    pub fn load_fen(&mut self, fen: &str, flipped: bool, source: Option<Source>) {
        let edit = if source.is_some() { ChessEdit::LoadDiagram } else { ChessEdit::LoadFen };
        self.load_fen_as(edit, fen, flipped, source);
    }

    /// [`Self::load_fen`] as the undo step `edit`.
    pub(crate) fn load_fen_as(&mut self, edit: ChessEdit, fen: &str, flipped: bool, source: Option<Source>) {
        self.edit(|s| s.put_fen(fen, flipped, source).then_some(edit));
    }

    /// The change [`Self::load_fen`] makes; whether the FEN could be read.
    fn put_fen(&mut self, fen: &str, flipped: bool, source: Option<Source>) -> bool {
        let (game, illegal, message) = match Game::from_fen(fen) {
            Ok(game) => (game, None, None),
            Err(GameError::BadFen(e)) => {
                self.message = Some(crate::i18n::fmt(tl!("The FEN couldn't be read: {e}"), &[("e", &e)]));
                return false;
            }
            Err(_) => match pdfcraft_engine::chess::pdfcraft_chess::fen::parse_placement(fen) {
                Ok(placement) => (
                    Game::standard(),
                    Some((placement, fen.trim().to_owned())),
                    Some(tl!("Not a legal position: correct it in the diagram editor to play moves on it.").to_string()),
                ),
                Err(e) => {
                    self.message = Some(crate::i18n::fmt(tl!("The FEN couldn't be read: {e}"), &[("e", &e.to_string())]));
                    return false;
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
        true
    }

    /// Read the PGN field into the board (one undo step).
    fn load_pgn(&mut self) {
        self.edit(|s| match Game::from_pgn(&s.pgn_text) {
            Ok((game, stopped)) => {
                s.game = game;
                s.illegal = None;
                s.selected = None;
                s.promotion = None;
                s.message = stopped.map(|m| crate::i18n::fmt(tl!("Reading stopped at {move}: it is not a legal move there."), &[("move", &m)]));
                s.refresh_fields(true);
                Some(ChessEdit::LoadPgn)
            }
            Err(e) => {
                s.message = Some(crate::i18n::fmt(tl!("The PGN couldn't be read: {e}"), &[("e", &e.to_string())]));
                None
            }
        });
    }

    /// Back to the standard start position, as a new game (one undo step).
    fn standard_start(&mut self) {
        self.edit(|s| {
            s.game = Game::standard();
            s.illegal = None;
            s.source = None;
            s.selected = None;
            s.promotion = None;
            s.message = None;
            s.refresh_fields(true);
            Some(ChessEdit::StandardStart)
        });
    }

    /// The board as an undo step keeps it.
    fn snapshot(&self) -> Snapshot {
        Snapshot { game: self.game.clone(), illegal: self.illegal.clone(), source: self.source, flipped: self.flipped }
    }

    /// Make a change to the game as one undo step. `change` says what it did, or `None` when it
    /// did nothing; a change that left the game's revision as it was is not a step either (a
    /// move already in the game was only followed), unless it put a new game in place. Returns
    /// whether a step was recorded.
    pub(crate) fn edit(&mut self, change: impl FnOnce(&mut BoardState) -> Option<ChessEdit>) -> bool {
        let before = self.snapshot();
        let revision = self.game.revision();
        self.history.see(revision);
        let Some(edit) = change(self) else { return false };
        let replaced = edit.replaces_game();
        if !replaced && self.game.revision() == revision {
            return false;
        }
        if replaced {
            // A new game starts at revision 0: keep revisions from repeating across games.
            self.game.bump_revision(self.history.max_revision());
        }
        self.history.see(self.game.revision());
        self.history.record(edit, before);
        true
    }

    /// Undo the last change to the game. Whether there was one.
    pub fn undo(&mut self) -> bool {
        self.restore(true)
    }

    /// Redo the last change undone. Whether there was one.
    pub fn redo(&mut self) -> bool {
        self.restore(false)
    }

    /// What Undo would undo ("Play Nd5"), in the current language.
    pub fn undo_label(&self) -> Option<String> {
        self.history.undo_edit().map(ChessEdit::text)
    }

    /// What Redo would redo, in the current language.
    pub fn redo_label(&self) -> Option<String> {
        self.history.redo_edit().map(ChessEdit::text)
    }

    /// "Undo Play Nd5" or "Redo Play Nd5" when there is a step to undo (`undo`) or redo.
    pub(crate) fn command_text(&self, undo: bool) -> Option<String> {
        let edit = if undo { self.history.undo_edit() } else { self.history.redo_edit() };
        edit.map(|e| crate::chess_history::command_text(undo, e))
    }

    /// Put back the board a step kept. The game put back gets a revision newer than any seen, so
    /// the FEN and PGN fields (refreshed on the revision) and anything else keyed on it can't
    /// take it for the game they last saw.
    fn restore(&mut self, undo: bool) -> bool {
        let current = self.snapshot();
        let seen = self.history.max_revision().max(self.game.revision());
        let back = if undo { self.history.undo(current) } else { self.history.redo(current) };
        let Some((edit, snapshot)) = back else { return false };
        self.game = snapshot.game;
        self.game.bump_revision(seen);
        self.history.see(self.game.revision());
        self.illegal = snapshot.illegal;
        self.source = snapshot.source;
        if edit.sets_side() {
            self.flipped = snapshot.flipped;
        }
        self.selected = None;
        self.promotion = None;
        self.message = None;
        self.drawing = None;
        // A field being typed in shows the game's text again, and its next key starts a new step.
        self.draft = None;
        true
    }

    /// Fill the FEN and PGN fields from the board, when it changed (`force`: always).
    pub(crate) fn refresh_fields(&mut self, force: bool) {
        let key = match &self.illegal {
            Some((_, fen)) => format!("illegal|{fen}"),
            None => format!("{}|{}|{}", self.game.start_fen(), self.game.revision(), self.game.current()),
        };
        if force || key != self.shown {
            self.fen_text = self.current_fen();
            self.pgn_text = if self.illegal.is_some() { String::new() } else { self.game.pgn() };
            self.shown = key;
        }
    }

    /// The FEN of the board shown (an illegal position's as it was given).
    pub(crate) fn current_fen(&self) -> String {
        self.illegal.as_ref().map_or_else(|| self.game.fen(), |(_, fen)| fen.clone())
    }

    /// The pieces shown.
    pub(crate) fn placement(&self) -> Placement {
        self.illegal.as_ref().map_or_else(|| self.game.placement(), |(p, _)| *p)
    }

    /// The square at `(x, y)` in the board's own units (0..1 from its top left), as shown.
    pub(crate) fn square_at(&self, x: f32, y: f32) -> Option<Square> {
        if !(x.is_finite() && y.is_finite() && (0.0..1.0).contains(&x) && (0.0..1.0).contains(&y)) {
            return None;
        }
        // In 0..8 after the checks above, so the casts cannot wrap.
        let (col, row) = ((x * 8.0) as u8, (y * 8.0) as u8);
        if self.flipped { Square::new(7 - col, row) } else { Square::new(col, 7 - row) }
    }

    /// The rectangle of `square` on a board drawn in `board`.
    pub(crate) fn square_rect(&self, board: Rect, square: Square) -> Rect {
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

    /// Play a move (one undo step, unless it was in the game already and is only followed).
    fn play(&mut self, from: Square, to: Square, promotion: Option<PieceKind>) {
        self.selected = None;
        self.edit(|s| match s.game.play(from, to, promotion) {
            Ok(()) => {
                s.promotion = None;
                s.message = None;
                Some(ChessEdit::Play(s.game.san(s.game.current()).to_owned()))
            }
            Err(GameError::NeedsPromotion) => {
                s.promotion = Some((from, to));
                None
            }
            Err(e) => {
                s.message = Some(e.to_string());
                None
            }
        });
    }

    /// Play `moves` (UCI) from the move shown, as far as they can be played: a line the engine
    /// suggested. Moves already in the game are followed; the others continue the line or start
    /// a variation. One undo step, when a move was added.
    pub fn play_uci_line(&mut self, moves: &[String]) {
        if self.illegal.is_some() {
            return;
        }
        self.selected = None;
        self.promotion = None;
        self.message = None;
        self.edit(|s| {
            for mv in moves {
                if let Err(e) = s.game.play_uci(mv) {
                    s.message = Some(e.to_string());
                    break;
                }
            }
            Some(ChessEdit::EngineLine)
        });
    }

    /// Step through the line: `Some(true)` forward, `Some(false)` back.
    pub(crate) fn step(&mut self, forward: bool) {
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
        let fen = fields.join(" ");
        self.edit(|s| match Game::from_fen(&fen) {
            Ok(game) => {
                s.game = game;
                s.message = None;
                s.selected = None;
                Some(ChessEdit::SideToMove)
            }
            Err(_) => {
                s.message = Some(tl!("That side can't be to move: the other king is in check.").to_string());
                None
            }
        });
    }

    /// The drawings on the position shown.
    pub fn marks(&self) -> &[Mark] {
        self.marks.get(&self.current_fen()).map_or(&[], Vec::as_slice)
    }

    /// Draw `mark` on the position shown, or take it off when it is there already in that
    /// colour; the same squares in another colour are recoloured.
    pub fn toggle_mark(&mut self, mark: Mark) {
        let key = self.current_fen();
        if !self.marks.contains_key(&key) {
            if self.marked_order.len() >= MAX_MARKED_POSITIONS && !self.marked_order.is_empty() {
                let oldest = self.marked_order.remove(0);
                self.marks.remove(&oldest);
            }
            self.marked_order.push(key.clone());
        }
        let list = self.marks.entry(key).or_default();
        match list.iter().position(|m| m.from == mark.from && m.to == mark.to) {
            Some(i) if list.get(i).is_some_and(|m| m.color == mark.color) => {
                list.remove(i);
            }
            Some(i) => {
                if let Some(m) = list.get_mut(i) {
                    m.color = mark.color;
                }
            }
            None if list.len() < MAX_MARKS => list.push(mark),
            None => {}
        }
    }

    /// Take the drawings off the position shown.
    pub fn clear_marks(&mut self) {
        let key = self.current_fen();
        if self.marks.remove(&key).is_some() {
            self.marked_order.retain(|k| *k != key);
        }
    }

    /// The twelve pieces cut out of their squares, in one image: White's king, queen, rook,
    /// bishop, knight and pawn on the top row, Black's on the second (see [`sprite_uv`]). Each
    /// is drawn on white and on black; where the two differ the piece is see-through, so the
    /// difference is its alpha and the drawing on black its premultiplied colour.
    pub(crate) fn sprites(&mut self, ctx: &egui::Context, pieces: &PieceSet) -> Option<TextureHandle> {
        if let Some(tex) = &self.sprites {
            return Some(tex.clone());
        }
        let mut placement = Placement::empty();
        for (row, color) in [(7u8, Color::White), (6u8, Color::Black)] {
            for (file, kind) in (0u8..).zip(KINDS) {
                if let Some(sq) = Square::new(file, row) {
                    placement.set(sq, Some(Piece { color, kind }));
                }
            }
        }
        let style = |c: Rgb| BoardStyle { border: 0.0, coordinates: false, flipped: false, light: c, dark: c, ..BoardStyle::default() };
        let draw = |c: Rgb| pdfcraft_engine::chess::office::diagram_rgba(&placement, pieces, &style(c), BOARD_PX).ok();
        let (w, h, on_white) = draw(Rgb(255, 255, 255))?;
        let (w2, h2, on_black) = draw(Rgb(0, 0, 0))?;
        let (w, h) = (usize::try_from(w).ok()?, usize::try_from(h).ok()?);
        if (w, h) != (usize::try_from(w2).ok()?, usize::try_from(h2).ok()?)
            || on_white.len() != w.checked_mul(h)?.checked_mul(4)?
            || on_black.len() != on_white.len()
        {
            return None;
        }
        let mut rgba = Vec::with_capacity(on_white.len());
        for (wp, bp) in on_white.as_chunks::<4>().0.iter().zip(on_black.as_chunks::<4>().0) {
            let diff: u16 = wp.iter().zip(bp).take(3).map(|(a, b)| u16::from(a.saturating_sub(*b))).sum::<u16>() / 3;
            let alpha = 255u8.saturating_sub(u8::try_from(diff).unwrap_or(255));
            // Premultiplied: the colour on black is colour × alpha, never above alpha.
            rgba.extend(bp.iter().take(3).map(|c| (*c).min(alpha)));
            rgba.push(alpha);
        }
        let tex = ctx.load_texture("chess-analysis-pieces", egui::ColorImage::from_rgba_premultiplied([w, h], &rgba), egui::TextureOptions::LINEAR);
        self.sprites = Some(tex.clone());
        Some(tex)
    }

    /// Drop the board's pictures (they are drawn again when it is shown): a game not shown keeps
    /// none.
    pub(crate) fn release_textures(&mut self) {
        self.texture = None;
        self.sprites = None;
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

/// The pieces in the order of the sprite image's columns.
const KINDS: [PieceKind; 6] = [PieceKind::King, PieceKind::Queen, PieceKind::Rook, PieceKind::Bishop, PieceKind::Knight, PieceKind::Pawn];

/// Where `piece` is in the sprite image (texture coordinates).
pub(crate) fn sprite_uv(piece: Piece) -> Rect {
    let col = KINDS.iter().position(|k| *k == piece.kind).unwrap_or(0) as f32;
    let row = if piece.color == Color::White { 0.0 } else { 1.0 };
    Rect::from_min_size(egui::pos2(col / 8.0, row / 8.0), vec2(1.0 / 8.0, 1.0 / 8.0))
}

/// The colours drawn over the board.
const LAST_MOVE: Color32 = Color32::from_rgba_premultiplied(155, 199, 0, 105);
const PICKED: Color32 = Color32::from_rgba_premultiplied(20, 85, 30, 110);
const TARGET: Color32 = Color32::from_rgba_premultiplied(20, 85, 30, 120);
const CHECK: Color32 = Color32::from_rgb(0xE0, 0x3E, 0x3E);

/// An arrow from the centre of `mark.from` to the centre of `mark.to` (a ring when they are the
/// same square).
fn paint_mark(painter: &egui::Painter, state: &BoardState, board: Rect, mark: Mark) {
    let (a, b) = (state.square_rect(board, mark.from), state.square_rect(board, mark.to));
    let cell = a.width();
    let color = mark.color.color32();
    if mark.from == mark.to {
        painter.circle_stroke(a.center(), cell * 0.44, Stroke::new(cell * 0.07, color));
        return;
    }
    let (start, end) = (a.center(), b.center());
    let dir = (end - start).normalized();
    if !(dir.x.is_finite() && dir.y.is_finite()) {
        return;
    }
    let head = cell * 0.42;
    // The head ends a little short of the centre; the shaft stops where the head starts.
    let tip = end - dir * cell * 0.12;
    let base = tip - dir * head;
    let normal = vec2(-dir.y, dir.x);
    painter.line_segment([start + dir * cell * 0.18, base], Stroke::new(cell * 0.17, color));
    painter.add(egui::Shape::convex_polygon(vec![tip, base + normal * head * 0.55, base - normal * head * 0.55], color, Stroke::NONE));
}

/// The board itself, with its marks, taking clicks and drags. `overlay` is drawn under the
/// hand-made drawings (the engine's best move).
pub(crate) fn board_view(ui: &mut egui::Ui, t: &Tokens, state: &mut BoardState, pieces: &PieceSet, overlay: &[Mark]) {
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
    if response.drag_started_by(PointerButton::Primary)
        && let Some(sq) = response.interact_pointer_pos().and_then(|p| at(state, p))
    {
        // Dragging picks the piece up; dropping it on its own square leaves it picked.
        state.clear_marks();
        if state.selected != Some(sq) {
            state.click(sq);
        }
    }
    if response.drag_stopped_by(PointerButton::Primary)
        && let (Some(from), Some(to)) = (state.selected, response.interact_pointer_pos().and_then(|p| at(state, p)))
        && from != to
    {
        state.click(to);
    } else if response.clicked()
        && let Some(sq) = response.interact_pointer_pos().and_then(|p| at(state, p))
    {
        state.clear_marks();
        state.click(sq);
    }
    // The right button draws: a click circles a square, a drag draws an arrow.
    let modifiers = ui.input(|i| i.modifiers);
    if response.drag_started_by(PointerButton::Secondary) {
        state.drawing = response.interact_pointer_pos().and_then(|p| at(state, p));
    }
    if response.drag_stopped_by(PointerButton::Secondary) {
        if let (Some(from), Some(to)) = (state.drawing.take(), response.interact_pointer_pos().and_then(|p| at(state, p))) {
            state.toggle_mark(Mark { from, to, color: MarkColor::from_modifiers(modifiers) });
        }
    } else if response.secondary_clicked()
        && let Some(sq) = response.interact_pointer_pos().and_then(|p| at(state, p))
    {
        state.toggle_mark(Mark { from: sq, to: sq, color: MarkColor::from_modifiers(modifiers) });
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
        // The square a dragged piece would land on, and the piece itself under the pointer: its
        // square shows empty meanwhile.
        if response.dragged_by(PointerButton::Primary)
            && let Some(from) = state.selected
            && let Some(pointer) = response.interact_pointer_pos()
        {
            if let Some(sq) = at(state, pointer) {
                painter.rect_stroke(state.square_rect(rect, sq), 0.0, Stroke::new(2.0, Color32::WHITE), egui::StrokeKind::Inside);
            }
            if let (Some(piece), Some(sprites)) = (state.game.piece_on(from), state.sprites(ui.ctx(), pieces)) {
                let cell = state.square_rect(rect, from);
                let Rgb(r, g, b) = if from.is_light() { BoardStyle::default().light } else { BoardStyle::default().dark };
                painter.rect_filled(cell, 0.0, Color32::from_rgb(r, g, b));
                let ghost = Rect::from_center_size(pointer, cell.size() * 1.1);
                ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("chess-board-drag"))).image(
                    sprites.id(),
                    ghost,
                    sprite_uv(piece),
                    Color32::WHITE,
                );
            }
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
    // The drawings: the overlay, the position's own, and the arrow being drawn.
    for mark in overlay.iter().chain(state.marks()) {
        paint_mark(&painter, state, rect, *mark);
    }
    if response.dragged_by(PointerButton::Secondary)
        && let (Some(from), Some(to)) = (state.drawing, response.interact_pointer_pos().and_then(|p| at(state, p)))
    {
        paint_mark(&painter, state, rect, Mark { from, to, color: MarkColor::from_modifiers(modifiers) });
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

/// What a move's menu asked for.
enum MoveEdit {
    Go(NodeId),
    Promote(NodeId),
    Delete(NodeId),
}

/// The move list: numbered, the move shown highlighted, each one a click away; variations in
/// brackets and smaller, comments in italics. A move's menu (right-click) makes its line the main
/// one or deletes it.
fn move_list(ui: &mut egui::Ui, t: &Tokens, state: &mut BoardState) {
    if state.illegal.is_some() {
        return;
    }
    let tokens = state.game.tokens();
    if tokens.iter().all(|t| !matches!(t, Token::Move { .. })) {
        ui.label(egui::RichText::new(tl!("Play a move: click a piece, then where it goes, or drag it there.")).small().color(t.text_muted));
        return;
    }
    let mut edit = None;
    let current = state.game.current();
    egui::ScrollArea::vertical().id_salt("chess-board-moves").max_height(180.0).auto_shrink([false, true]).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for token in &tokens {
                match token {
                    Token::Move { node, number, color, numbered, san, nags, depth } => {
                        let depth = *depth;
                        if *numbered {
                            let n = if *color == Color::White { format!("{number}.") } else { format!("{number}...") };
                            ui.label(egui::RichText::new(n).small().color(t.text_faint));
                        }
                        let text = format!("{san}{}", glyphs(nags));
                        let mut rich = egui::RichText::new(text).monospace();
                        if depth > 0 {
                            rich = rich.small().color(t.text_muted);
                        }
                        let r = ui.selectable_label(*node == current, rich);
                        if r.clicked() {
                            edit = Some(MoveEdit::Go(*node));
                        }
                        r.context_menu(|ui| {
                            if depth > 0 && ui.button(tl!("Make this the main line")).clicked() {
                                edit = Some(MoveEdit::Promote(*node));
                                ui.close();
                            }
                            if ui.button(tl!("Delete from this move")).clicked() {
                                edit = Some(MoveEdit::Delete(*node));
                                ui.close();
                            }
                        });
                    }
                    Token::Comment { text, .. } => {
                        ui.label(egui::RichText::new(text).italics().small().color(t.text_muted));
                    }
                    Token::VariationStart => {
                        ui.label(egui::RichText::new("(").small().color(t.text_faint));
                    }
                    Token::VariationEnd => {
                        ui.label(egui::RichText::new(")").small().color(t.text_faint));
                    }
                }
            }
        });
    });
    if let Some(e) = edit {
        state.selected = None;
        state.promotion = None;
        match e {
            MoveEdit::Go(node) => state.game.go_to_node(node),
            MoveEdit::Promote(node) => {
                state.edit(|s| {
                    s.game.promote(node);
                    Some(ChessEdit::MakeMainLine)
                });
            }
            MoveEdit::Delete(node) => {
                state.edit(|s| {
                    s.game.delete(node);
                    Some(ChessEdit::DeleteMoves)
                });
            }
        }
    }
}

/// How glyph `code` is shown. The slight advantages (⩲ ⩱) are written `+=` and `=+`, as books
/// set them in text: the interface fonts lack the two signs.
fn nag_text(code: u16) -> String {
    match code {
        14 => "+=".to_owned(),
        15 => "=+".to_owned(),
        _ => pdfcraft_engine::chess::pdfcraft_chess::nag::by_code(code).map_or_else(|| format!("${code}"), |nag| nag.glyph.to_owned()),
    }
}

/// How a move's glyphs read after it: `!`, `?` and the like glued on, the others after a space.
fn glyphs(nags: &[u16]) -> String {
    let mut out = String::new();
    for &code in nags {
        if !(1..=6).contains(&code) {
            out.push(' ');
        }
        out.push_str(&nag_text(code));
    }
    out
}

/// The glyphs the panel offers for the move shown: its quality, then the evaluation.
const OFFERED_NAGS: [u16; 13] = [1, 2, 3, 4, 5, 6, 10, 14, 15, 16, 17, 18, 19];

/// The glyph buttons and the comment of the move shown.
fn annotation(ui: &mut egui::Ui, state: &mut BoardState) {
    let node = state.game.current();
    if state.illegal.is_some() || node == 0 {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for code in OFFERED_NAGS {
            let on = state.game.nags(node).contains(&code);
            if ui.selectable_label(on, egui::RichText::new(nag_text(code)).monospace()).clicked() {
                state.edit(|s| {
                    s.game.toggle_nag(node, code);
                    Some(ChessEdit::Glyph(nag_text(code)))
                });
            }
        }
    });
    let key = format!("comment {node}");
    let (mut comment, mut recorded) = drafted(&state.draft, &key, state.game.comment(node));
    let r = ui.add(egui::TextEdit::multiline(&mut comment).hint_text(tl!("Comment on this move")).desired_rows(1).desired_width(f32::INFINITY));
    if r.changed() {
        recorded = state.type_in(recorded, ChessEdit::Comment, |game| game.set_comment(node, &comment));
    }
    keep_draft(&mut state.draft, &key, &r, comment, recorded);
}

impl BoardState {
    /// A key typed in a comment or detail field: the first change of the field's editing session
    /// (`recorded` false) is an undo step, the next ones join it. Whether the session is a step.
    fn type_in(&mut self, recorded: bool, edit: ChessEdit, change: impl FnOnce(&mut Game)) -> bool {
        if recorded {
            change(&mut self.game);
            self.history.see(self.game.revision());
            true
        } else {
            self.edit(|s| {
                change(&mut s.game);
                Some(edit)
            })
        }
    }
}

/// The text a cleaned field shows: as typed while it is being typed in (`draft` holds its
/// `key`), else as the game keeps it; and whether its editing session is an undo step yet.
fn drafted(draft: &Option<Draft>, key: &str, stored: &str) -> (String, bool) {
    match draft {
        Some(d) if d.key == key => (d.text.clone(), d.recorded),
        _ => (stored.to_owned(), false),
    }
}

/// Keep a field's text as typed while it has the keyboard; once it loses it, show the game's
/// (and the next change to it is a new undo step).
fn keep_draft(draft: &mut Option<Draft>, key: &str, response: &egui::Response, text: String, recorded: bool) {
    if response.has_focus() {
        *draft = Some(Draft { key: key.to_owned(), text, recorded });
    } else if draft.as_ref().is_some_and(|d| d.key == key) {
        *draft = None;
    }
}

/// The game's details: the PGN tags people fill in.
fn details(ui: &mut egui::Ui, state: &mut BoardState) {
    if state.illegal.is_some() {
        return;
    }
    egui::CollapsingHeader::new(tl!("Game details")).id_salt("chess-board-details").show(ui, |ui| {
        egui::Grid::new("chess-board-tags").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            for (tag, label) in [
                ("White", tl!("White")),
                ("Black", tl_ctx!("chess side", "Black")),
                ("Event", tl!("Event")),
                ("Site", tl!("Site")),
                ("Date", tl!("Date")),
                ("Round", tl!("Round")),
                ("Result", tl!("Result")),
            ] {
                ui.label(label);
                let key = format!("tag {tag}");
                let (mut value, mut recorded) = drafted(&state.draft, &key, state.game.tag(tag).unwrap_or_default());
                let r = ui.add(egui::TextEdit::singleline(&mut value).desired_width(f32::INFINITY));
                if r.changed() {
                    recorded = state.type_in(recorded, ChessEdit::GameDetails, |game| {
                        // The names above are valid tag names: this cannot fail.
                        let _ = game.set_tag(tag, &value);
                    });
                }
                keep_draft(&mut state.draft, &key, &r, value, recorded);
                ui.end_row();
            }
        });
    });
}

/// The arrow keys walk the line while the pointer is over the panel and no field has the keyboard.
pub(crate) fn keys(ui: &mut egui::Ui, state: &mut BoardState) {
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
}

/// Flip, and whose move it is (before the first move).
pub(crate) fn view_controls(ui: &mut egui::Ui, state: &mut BoardState) {
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
}

/// The pieces a pawn reaching the last rank can become, while one waits.
pub(crate) fn promotion_choice(ui: &mut egui::Ui, state: &mut BoardState) {
    let Some((from, to)) = state.promotion else { return };
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

/// Start, back, forward and end; then the status line and the last message.
pub(crate) fn navigation(ui: &mut egui::Ui, t: &Tokens, state: &mut BoardState) {
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
        // Undo and Redo for the game (provisional here: they move to the PGN toolbar in T5a).
        // Each names the step it would take back ("Undo Play Nd5").
        ui.separator();
        for undo in [true, false] {
            let name = state.command_text(undo).unwrap_or_else(|| if undo { tl!("Undo") } else { tl!("Redo") }.to_string());
            let (icon, enabled) =
                if undo { ("undo-2", state.history.undo_edit().is_some()) } else { ("redo-2", state.history.redo_edit().is_some()) };
            if ui.add_enabled_ui(enabled, |ui| crate::icons::button(ui, icon, 28.0, false, &name)).inner.clicked() {
                if undo {
                    state.undo();
                } else {
                    state.redo();
                }
            }
        }
    });
    let (text, colour) = status(state);
    ui.label(egui::RichText::new(text).small().color(colour));
    if let Some(m) = &state.message {
        ui.label(egui::RichText::new(m).small().color(t.text_muted));
    }
}

/// The move list, the glyphs and comment of the move shown, and the game's details.
pub(crate) fn moves_and_notes(ui: &mut egui::Ui, t: &Tokens, state: &mut BoardState) {
    move_list(ui, t, state);
    if state.illegal.is_none() && state.game.ply() < state.game.len() && ui.small_button(tl!("Delete the moves after this one")).clicked() {
        state.edit(|s| {
            s.game.truncate();
            Some(ChessEdit::DeleteMoves)
        });
    }
    annotation(ui, state);
    details(ui, state);
}

/// The FEN and PGN fields, with what can be done with them.
pub(crate) fn fen_and_pgn(ui: &mut egui::Ui, state: &mut BoardState) -> Option<BoardAction> {
    let mut action = None;
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
            state.standard_start();
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

/// The Chess board panel. `source_label` names the diagram the board was loaded from.
pub fn panel(ui: &mut egui::Ui, t: &Tokens, state: &mut BoardState, pieces: &PieceSet, source_label: Option<String>) -> Option<BoardAction> {
    let mut action = None;
    keys(ui, state);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tl!("Chess board")).font(theme::semibold(15.5)));
        if let (Some(label), Some(source)) = (source_label, state.source)
            && ui.link(egui::RichText::new(label).small()).on_hover_text(tl!("Show this diagram on the page")).clicked()
        {
            action = Some(BoardAction::ShowSource(source));
        }
    });
    ui.add_space(4.0);
    view_controls(ui, state);
    ui.add_space(4.0);
    board_view(ui, t, state, pieces, &[]);
    promotion_choice(ui, state);
    navigation(ui, t, state);
    ui.add_space(4.0);
    moves_and_notes(ui, t, state);
    ui.add_space(6.0);
    let fields = fen_and_pgn(ui, state);
    action.or(fields)
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
    fn glyphs_read_as_books_print_them() {
        assert_eq!(glyphs(&[1]), "!");
        assert_eq!(glyphs(&[5, 14]), "!? +=");
        assert_eq!(glyphs(&[15, 18]), " =+ +-");
        assert_eq!(glyphs(&[999]), " $999");
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
