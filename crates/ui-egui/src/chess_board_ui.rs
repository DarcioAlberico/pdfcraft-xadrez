//! Chess ▸ Board: an analysis board in its own panel, beside the Inspector.
//!
//! A diagram goes onto it from the page (double-click the diagram), or from Open on the board in
//! the Chess diagrams panel and its editor, which bring this panel forward. Moves are
//! played by clicking a piece and then where it goes, or by dragging it there; the arrows (or
//! the arrow keys, once the panel was clicked: see `chess_tools`) walk the game back and forth.
//! A move made at an earlier point starts a variation. The notation under the board takes the
//! panel's height left (spec §4.1) and shows each variation on a row of its own, indented by its
//! depth, with the comments and annotation glyphs; a move's menu has the rail's structure tools
//! (promote, demote, delete a variation, cut) and deletes the move. The move shown takes glyphs
//! (from the rail, or typed) and comments after and before it (`chess_annotate`), and the game
//! its details (the PGN tags). The position is also a FEN field and
//! the game a PGN field: both can be copied, or edited and loaded back; the two and the details
//! are sections, closed until opened. The right button draws on the board, as on lichess: a
//! click circles a square and a drag draws an arrow (Shift red, Alt blue, Ctrl yellow, green
//! otherwise). The drawings belong to the move shown (U7): they are the game's, each one an undo
//! step, and go into its PGN as `[%csl]`/`[%cal]` commands in the move's comment
//! (`pdfcraft_chess::game::Drawing`). A left click no longer clears them, since that would take
//! a book's drawings off as soon as a piece is moved; the Analysis section's Clear drawings does.
//! An illegal position, which has no game, keeps its drawings beside it, by its FEN.
//!
//! It is the study board too (T5c, D11 b): `chess_study_ui` adds position set-up (a rail tool),
//! engine analysis with an evaluation bar beside the board, and the material, in an Analysis
//! section under the board.
//!
//! The rules are `pdfcraft_chess::game` (L0); this file only shows them. A diagram read from a
//! book can be illegal (a misread square): it is shown as it is, and no move can be played on it
//! until it is corrected in the diagram editor.

use std::collections::HashMap;

use egui::{Color32, Key, PointerButton, Pos2, Rect, Stroke, TextureHandle, vec2};
use pdfcraft_engine::DocId;
use pdfcraft_engine::chess::pdfcraft_chess::draw::{BoardStyle, PieceSet, Rgb};
use pdfcraft_engine::chess::pdfcraft_chess::game::{DrawColor, Drawing, Game, GameError, MAX_MOVE_NUMBER, NodeId, Outcome, Token};
use pdfcraft_engine::chess::pdfcraft_chess::placement::{Color, Piece, PieceKind, Placement, Square};

use crate::chess_history::{ChessEdit, GameHistory, Snapshot};
use crate::theme::{self, Tokens};

/// The board image's side, in pixels (it is scaled to the panel).
const BOARD_PX: u32 = 640;

/// Largest board on screen, in points.
const MAX_BOARD_PT: f32 = 480.0;

/// How large the board may be drawn, how sharp its picture is and how the panel is laid out:
/// [`Sizing::DOCKED`] in the panels, larger in the chess board's own window
/// (`chess_window`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Sizing {
    /// Largest board side, in points (at least the smallest board, 160 pt).
    pub(crate) max_side: f32,
    /// Largest side of the board's picture, in pixels; at [`BOARD_PX`] or below it is drawn at
    /// [`BOARD_PX`] whatever the screen, as in the panels.
    pub(crate) max_px: u32,
    /// Two columns: the board on the left, the moves, notes and fields on the right.
    pub(crate) wide: bool,
}

impl Sizing {
    /// The panels: at most 480 pt, a 640 px picture, one column.
    pub(crate) const DOCKED: Sizing = Sizing { max_side: MAX_BOARD_PT, max_px: BOARD_PX, wide: false };

    /// The largest board side, never below the smallest board (160 pt) nor NaN, so `clamp`
    /// with it can't panic.
    pub(crate) fn side_limit(self) -> f32 {
        if self.max_side.is_finite() { self.max_side.max(160.0) } else { 160.0 }
    }

    /// The picture's side for a board `side` points wide at `pixels_per_point`: the screen's
    /// pixels rounded up to a multiple of 128 (so resizing a window redraws it now and then, not
    /// every frame), within [`BOARD_PX`]..=`max_px`.
    fn picture_px(self, side: f32, pixels_per_point: f32) -> u32 {
        if self.max_px <= BOARD_PX {
            return BOARD_PX;
        }
        // `as` saturates, and NaN becomes 0: any input stays within the bounds below.
        let px = (side * pixels_per_point / 128.0).ceil() as u32;
        px.saturating_mul(128).clamp(BOARD_PX, self.max_px)
    }
}

/// Most illegal positions that keep drawings; the oldest go first.
const MAX_MARKED_POSITIONS: usize = 512;

/// Most drawings on one position (as many as a move of the game keeps).
const MAX_MARKS: usize = pdfcraft_engine::chess::pdfcraft_chess::game::MAX_DRAWINGS;

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

    /// The game's colour for a hand-made drawing (`None` for the engine's arrow, which is not
    /// the game's).
    fn to_draw(self) -> Option<DrawColor> {
        match self {
            MarkColor::Green => Some(DrawColor::Green),
            MarkColor::Red => Some(DrawColor::Red),
            MarkColor::Blue => Some(DrawColor::Blue),
            MarkColor::Yellow => Some(DrawColor::Yellow),
            MarkColor::Engine => None,
        }
    }

    fn from_draw(color: DrawColor) -> MarkColor {
        match color {
            DrawColor::Green => MarkColor::Green,
            DrawColor::Red => MarkColor::Red,
            DrawColor::Blue => MarkColor::Blue,
            DrawColor::Yellow => MarkColor::Yellow,
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
    /// The PGN field was typed in since it was last filled from the game: a step on the board
    /// leaves it as typed until it is loaded or the edit discarded (plan A8, W6).
    pub(crate) pgn_edited: bool,
    /// A FEN typed while the game has moves, waiting for New game or Replace (plan A8, W3), and
    /// whether New game takes the keyboard when it is drawn next.
    pub(crate) fen_choice: Option<String>,
    focus_fen_choice: bool,
    /// Why the last load or move did not work.
    pub message: Option<String>,
    /// The board picture, the position and view it shows, and its side in pixels.
    texture: Option<(String, u32, TextureHandle)>,
    draw_error: Option<String>,
    /// The pieces cut out of their squares, for the one following the pointer in a drag.
    sprites: Option<TextureHandle>,
    /// The comment or game-detail field being typed in. The game keeps comments and tags cleaned
    /// (spaces collapsed, ends trimmed), so a field re-read from it every frame would eat a space
    /// typed at the end.
    draft: Option<Draft>,
    /// The drawings on illegal positions (a legal one's are its game's), by FEN, and the order
    /// the positions got their first one.
    marks: HashMap<String, Vec<Mark>>,
    marked_order: Vec<String>,
    /// Where a right-button drag started.
    drawing: Option<Square>,
    /// The game's undo and redo steps.
    pub(crate) history: GameHistory,
    /// The move shown (and the game's revision) when the notation last scrolled to it.
    revealed: Option<(NodeId, u64)>,
    /// The comment editor (the comment before the move too) is open.
    pub(crate) comment_open: bool,
    /// The comment field takes the keyboard when it is drawn next.
    pub(crate) focus_comment: bool,
    /// Where the glyph palette types.
    pub(crate) typing: crate::chess_annotate::Typing,
    /// The moves the last capture added, tinted in the move list (T7b).
    pub fresh: crate::chess_capture::Fresh,
    /// A move whose place on the page was asked for (Show in PDF, T8), until the app shows it.
    pub(crate) show_source: Option<NodeId>,
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
            pgn_edited: false,
            fen_choice: None,
            focus_fen_choice: false,
            message: None,
            texture: None,
            draw_error: None,
            sprites: None,
            draft: None,
            marks: HashMap::new(),
            marked_order: Vec::new(),
            drawing: None,
            history: GameHistory::default(),
            revealed: None,
            comment_open: false,
            focus_comment: false,
            typing: Default::default(),
            fresh: Default::default(),
            show_source: None,
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
    /// A stretch of a line as text (T15: "Copy line with figurines", "Copy line (letters)").
    CopyLine(String),
    /// Pick the engine program for analysis (desktop only).
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    ChooseEngine,
    /// The PGN field's games after the first, which went on the board: add them to the
    /// document's games; `more`: the field had more than the cap.
    AddGames {
        texts: Vec<String>,
        more: bool,
    },
    /// Show the Move list beside the page, or close it while it shows.
    ToggleMoveList,
    /// Open a new game from position `fen` after the one shown, which keeps its moves (plan A8,
    /// W3 and W4: a FEN typed, or the standard start, while the game has moves).
    NewGameAt(String),
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
            Err(e) => match pdfcraft_engine::chess::pdfcraft_chess::fen::parse_placement(fen) {
                Ok(placement) => (Game::standard(), Some((placement, fen.trim().to_owned())), Some(board_illegal_text(&e))),
                Err(e) => {
                    self.message = Some(crate::i18n::fmt(tl!("The FEN couldn't be read: {e}"), &[("e", &e.to_string())]));
                    return false;
                }
            },
        };
        self.game = game;
        self.illegal = illegal;
        self.message = message;
        self.fen_choice = None;
        self.selected = None;
        self.promotion = None;
        self.flipped = flipped;
        self.source = source;
        self.refresh_fields(true);
        true
    }

    /// Read the PGN field into the board (one undo step): its first game, when it holds several
    /// (split at tag lines, as Open PGN… splits a file). The same game read again (an edited
    /// comment or variation) keeps the move shown and where its moves are on the page
    /// ([`carry_place`]). Returns the games after it as texts, at
    /// most [`crate::chess_autosave::MAX_GAMES`] games in all, and whether the field had more;
    /// none when the first could not be read.
    pub(crate) fn load_pgn(&mut self) -> (Vec<String>, bool) {
        let max = crate::chess_autosave::MAX_GAMES;
        let (first, rest, more) = {
            let chunks = pdfcraft_engine::chess::collection::split_pgn(&self.pgn_text, max);
            let first = chunks.first().map_or_else(|| self.pgn_text.clone(), |c| (*c).to_owned());
            let rest: Vec<String> = chunks.iter().skip(1).take(max.saturating_sub(1)).map(|c| (*c).to_owned()).collect();
            (first, rest, chunks.len() > max)
        };
        let loaded = self.edit(|s| match Game::from_pgn(&first) {
            Ok((mut game, stopped)) => {
                carry_place(&s.game, &mut game);
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
        if loaded { (rest, more) } else { (Vec::new(), false) }
    }

    /// Back to the standard start position in the game's place (one undo step). With moves on
    /// the board the panel opens a new game instead ([`BoardAction::NewGameAt`]).
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

    /// The memory the board's game and its history take, as the history's caps count it (a game
    /// deleted from the list is kept for Undo with its history).
    pub(crate) fn bytes(&self) -> usize {
        crate::chess_history::Estimate::bytes(&self.snapshot()).saturating_add(self.history.bytes())
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
            // Text typed in the PGN field stays until it is loaded or discarded; a new game put on
            // the board (`force`) replaces it.
            if force || !self.pgn_edited {
                self.pgn_text = if self.illegal.is_some() { String::new() } else { self.game.pgn() };
                self.pgn_edited = false;
            }
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

    /// The drawings on the position shown: the move shown's, or an illegal position's.
    pub fn marks(&self) -> Vec<Mark> {
        if self.illegal.is_some() {
            return self.marks.get(&self.current_fen()).cloned().unwrap_or_default();
        }
        let drawings = self.game.drawings(self.game.current());
        drawings.iter().map(|d| Mark { from: d.from, to: d.to, color: MarkColor::from_draw(d.color) }).collect()
    }

    /// Draw `mark` on the position shown, or take it off when it is there already in that
    /// colour; the same squares in another colour are recoloured. On a legal position it is a
    /// change to the move shown, one undo step; the engine's colour is never drawn.
    pub fn toggle_mark(&mut self, mark: Mark) {
        let Some(color) = mark.color.to_draw() else { return };
        if self.illegal.is_none() {
            let node = self.game.current();
            let drawing = Drawing { from: mark.from, to: mark.to, color };
            self.edit(|s| s.game.toggle_drawing(node, drawing).then_some(ChessEdit::Draw));
            return;
        }
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

    /// Take the drawings off the position shown (on a legal position, one undo step).
    pub fn clear_marks(&mut self) {
        if self.illegal.is_none() {
            let node = self.game.current();
            self.edit(|s| s.game.clear_drawings(node).then_some(ChessEdit::ClearDrawings));
            return;
        }
        let key = self.current_fen();
        if self.marks.remove(&key).is_some() {
            self.marked_order.retain(|k| *k != key);
        }
    }

    /// The twelve pieces cut out of their squares ([`cut_sprites`]), once.
    pub(crate) fn sprites(&mut self, ctx: &egui::Context, pieces: &PieceSet) -> Option<TextureHandle> {
        if let Some(tex) = &self.sprites {
            return Some(tex.clone());
        }
        let tex = cut_sprites(ctx, pieces, "chess-analysis-pieces")?;
        self.sprites = Some(tex.clone());
        Some(tex)
    }

    /// Drop the board's pictures (they are drawn again when it is shown): a game not shown keeps
    /// none.
    pub(crate) fn release_textures(&mut self) {
        self.texture = None;
        self.sprites = None;
    }

    /// The board image, at least `px` pixels wide, redrawn when the pieces or the view change
    /// or a larger one is wanted (a sharper one drawn for the chess window serves the panels).
    fn texture(&mut self, ctx: &egui::Context, pieces: &PieceSet, px: u32) -> Option<TextureHandle> {
        let placement = self.placement();
        let key = format!("{}|{}", pdfcraft_engine::chess::pdfcraft_chess::fen::format_placement(&placement), self.flipped);
        if let Some((k, drawn, tex)) = &self.texture
            && *k == key
            && *drawn >= px
        {
            return Some(tex.clone());
        }
        let style = BoardStyle { border: 0.0, coordinates: false, flipped: self.flipped, ..BoardStyle::default() };
        match pdfcraft_engine::chess::office::diagram_rgba(&placement, pieces, &style, px) {
            Ok((w, h, rgba)) => {
                let (w, h) = (usize::try_from(w).ok()?, usize::try_from(h).ok()?);
                if rgba.len() != w.checked_mul(h)?.checked_mul(4)? {
                    return None;
                }
                let image = egui::ColorImage::from_rgba_premultiplied([w, h], &rgba);
                let tex = ctx.load_texture("chess-analysis-board", image, egui::TextureOptions::LINEAR);
                self.texture = Some((key, px, tex.clone()));
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

/// The board's message for a position it can't play: why, when the rule broken has a name
/// (plan A8, P3b1), and where to correct it.
fn board_illegal_text(e: &GameError) -> String {
    match e {
        GameError::IllegalPosition(why) if let Some(v) = why.first() => crate::i18n::fmt(
            tl!("Not a legal position ({reason}): correct it in the diagram editor to play moves on it."),
            &[("reason", crate::chess_editor::violation_text(*v))],
        ),
        _ => tl!("Not a legal position: correct it in the diagram editor to play moves on it.").to_string(),
    }
}

/// When `new` is the game `old` was, read again (the same start, tags and main line), the move
/// shown and where its moves are printed on the page carry over to it by their paths (plan A8,
/// W6): loading the PGN field after editing a comment or a variation keeps the board where it
/// was. A different game is left as read.
fn carry_place(old: &Game, new: &mut Game) {
    let main = |g: &Game| g.main_line().into_iter().map(|n| g.san(n).to_owned()).collect::<Vec<_>>();
    if old.start_fen() != new.start_fen() || old.tags() != new.tags() || main(old) != main(new) {
        return;
    }
    // Every move of the old game, from the start; a tree, so each is seen once.
    let mut stack = vec![0];
    while let Some(node) = stack.pop() {
        stack.extend_from_slice(old.replies(node));
        if let Some(source) = old.source(node)
            && let Some(at) = old.path_of(node).and_then(|p| new.node_at(&p))
            && new.san(at) == old.san(node)
        {
            new.set_source(at, Some(source));
        }
    }
    if let Some(at) = old.path_of(old.current()).and_then(|p| new.node_at(&p)) {
        new.go_to_node(at);
    }
}

/// Where `piece` is in the sprite image (texture coordinates).
pub(crate) fn sprite_uv(piece: Piece) -> Rect {
    let col = KINDS.iter().position(|k| *k == piece.kind).unwrap_or(0) as f32;
    let row = if piece.color == Color::White { 0.0 } else { 1.0 };
    Rect::from_min_size(egui::pos2(col / 8.0, row / 8.0), vec2(1.0 / 8.0, 1.0 / 8.0))
}

/// The twelve pieces cut out of their squares, in one image (`name` is the texture's): White's
/// king, queen, rook, bishop, knight and pawn on the top row, Black's on the second (see
/// [`sprite_uv`]). Each is drawn on white and on black; where the two differ the piece is
/// see-through, so the difference is its alpha and the drawing on black its premultiplied colour.
/// The pieces' own fills are the same on both, so they stay opaque.
pub(crate) fn cut_sprites(ctx: &egui::Context, pieces: &PieceSet, name: &str) -> Option<TextureHandle> {
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
    Some(ctx.load_texture(name, egui::ColorImage::from_rgba_premultiplied([w, h], &rgba), egui::TextureOptions::LINEAR))
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

/// The board itself within `sizing`'s limits, with its marks, taking clicks and drags. `overlay`
/// is drawn under the hand-made drawings (the engine's best move).
pub(crate) fn board_view_sized(ui: &mut egui::Ui, t: &Tokens, state: &mut BoardState, pieces: &PieceSet, overlay: &[Mark], sizing: Sizing) {
    let side = ui.available_width().clamp(160.0, sizing.side_limit());
    let px = sizing.picture_px(side, ui.ctx().pixels_per_point());
    let Some(tex) = state.texture(ui.ctx(), pieces, px) else {
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
    for mark in overlay.iter().copied().chain(state.marks()) {
        paint_mark(&painter, state, rect, mark);
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
    /// The glyph brush (T15) clicked on a move, or a glyph chosen in its menu (U1), with its glyph
    /// (White's code): on the move, or off it.
    Brush(NodeId, u16),
    /// "Remove all glyphs" in a move's menu (U1).
    ClearGlyphs(NodeId),
    /// The move's diagram mark, put on or taken off (T15).
    Diagram(NodeId),
    /// A rail tool, on what it acts on for the move.
    Tool(crate::chess_tools::Run, crate::chess_tools::Target),
    Delete(NodeId),
}

/// Least height of the notation in the Board panel, in points (spec §4.1).
pub(crate) const MIN_NOTATION: f32 = 120.0;

/// Tallest the notation grows to where it is not told how tall to be.
const SHRINK_NOTATION: f32 = 180.0;

/// How far a variation is indented per level, in points; deeper levels than [`MAX_INDENT`] are
/// indented as that one, so a deep tree still leaves room for its moves.
const INDENT: usize = 12;
const MAX_INDENT: usize = 8;

/// One row of the notation: a stretch of a line, indented by how deep in variations it is.
struct Row<'a> {
    depth: usize,
    tokens: Vec<&'a Token>,
}

/// The notation's rows: each variation starts a row of its own, one level in, and the line it
/// branched from goes on in a new row after it. A `)` with nothing before it in its row closes
/// the row before instead (`(2... Nf6 3. e5))`), rather than standing alone.
fn rows(tokens: &[Token]) -> Vec<Row<'_>> {
    let mut rows = vec![Row { depth: 0, tokens: Vec::new() }];
    let mut depth = 0usize;
    for token in tokens {
        match token {
            Token::VariationStart => {
                depth = depth.saturating_add(1);
                rows.push(Row { depth, tokens: vec![token] });
            }
            Token::VariationEnd => {
                if rows.last().is_some_and(|r| r.tokens.is_empty()) && rows.len() > 1 {
                    rows.pop();
                }
                if let Some(row) = rows.last_mut() {
                    row.tokens.push(token);
                }
                depth = depth.saturating_sub(1);
                rows.push(Row { depth, tokens: Vec::new() });
            }
            _ => {
                if let Some(row) = rows.last_mut() {
                    row.tokens.push(token);
                }
            }
        }
    }
    rows.retain(|r| !r.tokens.is_empty());
    rows
}

/// The notation: numbered moves, the move shown highlighted (and scrolled into view when it
/// changes), each one a click away; each variation on its own row, indented by its depth, smaller
/// and muted; comments in italics, a comment before a move written before it. A move's menu
/// (right-click) has the rail's structure tools, acting on that move's variation, and deletes the
/// move with what follows. `height`: exactly this tall (the Board panel, where it takes the
/// height left), or `None` to shrink to the moves up to [`SHRINK_NOTATION`]. With `brush` (the
/// glyph brush armed, T15), a click on a move puts that glyph on it or takes it off instead of
/// going to it. A move marked for a diagram has a small board after it; its menu marks it or takes
/// the mark off, and copies its line as text. Returns what the menu asked of the app.
pub(crate) fn notation(ui: &mut egui::Ui, t: &Tokens, state: &mut BoardState, height: Option<f32>, brush: Option<u16>) -> Option<BoardAction> {
    if state.illegal.is_some() {
        return None;
    }
    let tokens = state.game.tokens();
    let width = ui.available_width();
    // The area as one named region, for assistive technology (and the tests, which measure it).
    if let Some(h) = height {
        let region = Rect::from_min_size(ui.cursor().min, vec2(width, h));
        let r = ui.interact(region, ui.id().with("chess-notation"), egui::Sense::hover());
        r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, tl!("Moves")));
    }
    let mut area = egui::ScrollArea::vertical().id_salt("chess-board-moves");
    area = match height {
        Some(h) => area.min_scrolled_height(h).max_height(h).auto_shrink([false, false]),
        None => area.max_height(SHRINK_NOTATION).auto_shrink([false, true]),
    };
    let mut edit = None;
    let mut action = None;
    let current = state.game.current();
    let reveal = state.revealed != Some((current, state.game.revision()));
    area.show(ui, |ui| {
        if tokens.iter().all(|t| !matches!(t, Token::Move { .. })) {
            ui.label(egui::RichText::new(tl!("Play a move: click a piece, then where it goes, or drag it there.")).small().color(t.text_muted));
            return;
        }
        ui.spacing_mut().item_spacing.y = 2.0;
        for row in rows(&tokens) {
            let indent = i8::try_from(row.depth.min(MAX_INDENT).saturating_mul(INDENT)).unwrap_or(i8::MAX);
            egui::Frame::NONE.inner_margin(egui::Margin { left: indent, ..egui::Margin::ZERO }).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let small = row.depth > 0;
                    for token in &row.tokens {
                        match token {
                            Token::Move { node, number, color, numbered, san, nags, .. } => {
                                if *numbered {
                                    let n = if *color == Color::White { format!("{number}.") } else { format!("{number}...") };
                                    ui.label(egui::RichText::new(n).small().color(t.text_faint));
                                }
                                let ctx = ui.ctx().clone();
                                let mut rich =
                                    egui::RichText::new(format!("{san}{}", glyphs(nags, |s| crate::chess_annotate::drawable(&ctx, s)))).monospace();
                                if small {
                                    rich = rich.small().color(t.text_muted);
                                }
                                if let Some(tint) = state.fresh.tint(&state.game, *node, ui.ctx()) {
                                    rich = rich.background_color(tint);
                                }
                                let r = ui.selectable_label(*node == current, rich);
                                if *node == current && reveal {
                                    r.scroll_to_me_animation(None, egui::style::ScrollAnimation::none());
                                }
                                glyphs_after(ui, t, nags, small);
                                if state.game.has_diagram(*node) {
                                    let name = crate::i18n::fmt(
                                        tl!("Diagram after {move}"),
                                        &[("move", &crate::chess_tools::move_name(&state.game, *node))],
                                    );
                                    let d =
                                        ui.add(crate::icons::image("grid-3x3", 12.0, t.text_muted).sense(egui::Sense::hover())).on_hover_text(&name);
                                    d.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, &name));
                                }
                                if r.clicked() {
                                    edit = Some(match brush {
                                        Some(code) => MoveEdit::Brush(*node, code),
                                        None => MoveEdit::Go(*node),
                                    });
                                }
                                r.context_menu(|ui| {
                                    if let Some((run, target)) = crate::chess_tools::move_menu(ui, state, *node) {
                                        edit = Some(MoveEdit::Tool(run, target));
                                        ui.close();
                                    }
                                    ui.separator();
                                    // The glyphs, on this move (U1).
                                    if let Some(code) = crate::chess_annotate::glyph_menu(ui, state, *node) {
                                        edit = Some(if code == crate::chess_annotate::CLEAR {
                                            MoveEdit::ClearGlyphs(*node)
                                        } else {
                                            MoveEdit::Brush(*node, code)
                                        });
                                        ui.close();
                                    }
                                    ui.separator();
                                    let marked = state.game.has_diagram(*node);
                                    if ui.add(egui::Button::new(tl!("Diagram here")).selected(marked)).clicked() {
                                        edit = Some(MoveEdit::Diagram(*node));
                                        ui.close();
                                    }
                                    if ui.button(tl!("Copy line with figurines")).clicked() {
                                        action = Some(BoardAction::CopyLine(state.game.line_text(*node, true)));
                                        ui.close();
                                    }
                                    if ui.button(tl!("Copy line (letters)")).clicked() {
                                        action = Some(BoardAction::CopyLine(state.game.line_text(*node, false)));
                                        ui.close();
                                    }
                                    ui.separator();
                                    if ui.button(tl!("Delete from this move")).clicked() {
                                        edit = Some(MoveEdit::Delete(*node));
                                        ui.close();
                                    }
                                    if ui.button(tl!("New game from this position")).clicked() {
                                        crate::chess_collection::ask_new_game_from(ui.ctx(), *node);
                                        ui.close();
                                    }
                                });
                            }
                            // A comment with only drawings has no text to show.
                            Token::Comment { text, .. } if text.is_empty() => {}
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
        }
    });
    state.revealed = Some((current, state.game.revision()));
    if let Some(e) = edit {
        state.selected = None;
        state.promotion = None;
        match e {
            MoveEdit::Go(node) => state.game.go_to_node(node),
            MoveEdit::Brush(node, code) => crate::chess_annotate::brush_glyph(state, node, code),
            MoveEdit::ClearGlyphs(node) => crate::chess_annotate::clear_glyphs_at(state, node),
            MoveEdit::Diagram(node) => {
                let mut full = false;
                state.edit(|s| {
                    full = !s.game.toggle_diagram(node);
                    (!full).then_some(ChessEdit::Diagram)
                });
                state.message = full.then(|| tl!("Not marked: this move has as many glyphs as it can take.").to_owned());
            }
            MoveEdit::Tool(run, target) => run(state, target),
            MoveEdit::Delete(node) => {
                state.edit(|s| {
                    s.game.delete(node);
                    Some(ChessEdit::DeleteMoves)
                });
            }
        }
    }
    action
}

/// How glyph `code` is named in an undo step or a capture's chips (whatever the fonts). The
/// slight advantages (⩲ ⩱) are written `+=` and `=+`, as books set them in text: the interface
/// fonts lack the two signs. A painted sign, and an assessment without one, by its name.
pub(crate) fn nag_text(code: u16) -> String {
    match code {
        14 => "+=".to_owned(),
        15 => "=+".to_owned(),
        _ => match crate::chess_annotate::seen(code, |_| true) {
            crate::chess_annotate::Seen::Text(text) => text,
            crate::chess_annotate::Seen::Icon(_) | crate::chess_annotate::Seen::Name(_) => {
                crate::chess_annotate::nag_name(code).map_or_else(|| format!("${code}"), str::to_owned)
            }
        },
    }
}

/// How a move's glyphs read after it: `!`, `?` and the like glued on, the others after a space;
/// each as [`crate::chess_annotate::face`] shows it with the fonts `drawable` describes. The
/// diagram mark is not among them (the notation draws a small board for it), nor the glyphs
/// painted or named after the move ([`glyphs_after`]).
fn glyphs(nags: &[u16], mut drawable: impl FnMut(&str) -> bool) -> String {
    let mut out = String::new();
    for &code in nags.iter().filter(|&&c| !pdfcraft_engine::chess::pdfcraft_chess::nag::is_diagram(c)) {
        let crate::chess_annotate::Seen::Text(text) = crate::chess_annotate::seen(code, &mut drawable) else { continue };
        if !(1..=6).contains(&code) {
            out.push(' ');
        }
        out.push_str(&text);
    }
    out
}

/// After a move's label in the notation: its painted signs, then its assessments without a sign
/// by name, small and muted, each with its `$n` as the tooltip (see `chess_annotate`'s notes).
fn glyphs_after(ui: &mut egui::Ui, t: &Tokens, nags: &[u16], small: bool) {
    let color = if small { t.text_muted } else { t.text };
    for &code in nags {
        match crate::chess_annotate::seen(code, |_| true) {
            crate::chess_annotate::Seen::Icon(icon) => {
                let name = crate::chess_annotate::nag_name(code).unwrap_or_default();
                crate::chess_annotate::icon_label(ui, icon, if small { 11.0 } else { 13.0 }, color, name);
            }
            crate::chess_annotate::Seen::Name(name) => {
                let r = ui.add(egui::Label::new(egui::RichText::new(name).small().color(t.text_muted)).selectable(false));
                r.on_hover_text(format!("${code}"));
            }
            crate::chess_annotate::Seen::Text(_) => {}
        }
    }
}

impl BoardState {
    /// The text of field `key` (a comment or a game detail) as typed while it has the keyboard,
    /// else `stored`; and whether its editing session is an undo step yet.
    pub(crate) fn drafted(&self, key: &str, stored: &str) -> (String, bool) {
        drafted(&self.draft, key, stored)
    }

    /// Keep field `key`'s text as typed while it has the keyboard (see [`keep_draft`]).
    pub(crate) fn keep(&mut self, key: &str, response: &egui::Response, text: String, recorded: bool) {
        keep_draft(&mut self.draft, key, response, text, recorded);
    }

    /// Whether the comment editor (the comment before the move too) is open.
    pub fn comment_editor_open(&self) -> bool {
        self.comment_open
    }

    /// Open or close the comment editor; opened, its comment field takes the keyboard.
    pub(crate) fn toggle_comment_editor(&mut self) {
        self.comment_open = !self.comment_open;
        self.focus_comment = self.comment_open;
    }

    /// A key typed in a comment or detail field: the first change of the field's editing session
    /// (`recorded` false) is an undo step, the next ones join it. Whether the session is a step.
    pub(crate) fn type_in(&mut self, recorded: bool, edit: ChessEdit, change: impl FnOnce(&mut Game)) -> bool {
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
    // A date PGN can't read is kept as typed, and said once the field lets go (the games list's
    // editor refuses one).
    let typing = state.draft.as_ref().is_some_and(|d| d.key == "tag Date");
    if !typing && !pdfcraft_engine::chess::collection::pgn_date_ok(state.game.tag("Date").unwrap_or_default()) {
        ui.label(
            egui::RichText::new(tl!("A date is written YYYY.MM.DD, with ? for each digit not known.")).small().color(ui.visuals().error_fg_color),
        );
    }
}

/// Flip, whose move it is (before the first move) and the number of the move the game starts at.
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
        // The move number, when the reading missed or misread it: at any time, every move is
        // renumbered with it. Beside "To move:" when the room left on the row holds the label and
        // its field whole, else at the start of the next row. Measured here because they sit in a
        // scope (`add_enabled_ui`), which is as wide as the room left and breaks the label into
        // letters in a narrow one. The room left is `available_size_before_wrap`: in a wrapping
        // row `available_width` is the whole row's.
        let label =
            egui::WidgetText::from(tl!("Move number:")).into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::TextStyle::Body);
        let needed = label.size().x + 2.0 * ui.spacing().item_spacing.x + ui.spacing().interact_size.x;
        let room = ui.available_size_before_wrap().x;
        if room < needed || room.is_nan() {
            ui.end_row();
        }
        ui.add_enabled_ui(state.illegal.is_none(), |ui| move_number(ui, state, label));
    });
}

/// The number of the move the game starts at. Typing or dragging it is one undo step until the
/// field lets go.
fn move_number(ui: &mut egui::Ui, state: &mut BoardState, label: std::sync::Arc<egui::Galley>) {
    const KEY: &str = "move number";
    let mut recorded = state.draft.as_ref().is_some_and(|d| d.key == KEY && d.recorded);
    let mut number = state.game.first_move_number();
    ui.label(label);
    let r = ui
        .add(egui::DragValue::new(&mut number).range(1..=MAX_MOVE_NUMBER).speed(0.2))
        .on_hover_text(tl!("The move the game starts at, to correct it when it was not read: 35 with Black to move is 35…"));
    if number != state.game.first_move_number() {
        recorded = state.type_in(recorded, ChessEdit::MoveNumber, |game| {
            game.set_first_move_number(number);
        });
    }
    if r.has_focus() || r.dragged() {
        state.draft = Some(Draft { key: KEY.to_owned(), text: String::new(), recorded });
    } else if state.draft.as_ref().is_some_and(|d| d.key == KEY) {
        state.draft = None;
    }
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
    });
    let (text, colour) = status(state);
    ui.label(egui::RichText::new(text).small().color(colour));
    if let Some(m) = &state.message {
        ui.label(egui::RichText::new(m).small().color(t.text_muted));
    }
}

/// Under the notation: the comments of the move shown (the one before it while the comment
/// editor is open). Deleting moves is a command of the move's menu, not a button beside the list
/// (plan A8, W1). The glyph palette floats (`chess_annotate`, U1).
pub(crate) fn notes(ui: &mut egui::Ui, t: &Tokens, state: &mut BoardState) {
    if state.illegal.is_some() {
        return;
    }
    // Before any field is drawn: which one had the keyboard, for the palette.
    state.typing.track(ui);
    let (editor, focus) = (state.comment_open, std::mem::take(&mut state.focus_comment));
    crate::chess_annotate::comments(ui, t, state, editor, focus);
}

/// The FEN, the PGN and the game's details, each a section of its own, closed until opened
/// (egui keeps which are open, for the session and the next). The fields are refreshed from the
/// game whether or not their section is open.
pub(crate) fn sections(ui: &mut egui::Ui, state: &mut BoardState) -> Option<BoardAction> {
    let mut action = None;
    state.refresh_fields(false);
    egui::CollapsingHeader::new("FEN").id_salt("chess-board-fen").default_open(false).show(ui, |ui| {
        let id = ui.make_persistent_id("chess-fen-field");
        crate::chess_annotate::insert_pending(ui, &mut state.typing, crate::chess_annotate::Field::Fen, id, &mut state.fen_text);
        let r = ui.add(egui::TextEdit::singleline(&mut state.fen_text).id(id).font(egui::TextStyle::Monospace).desired_width(f32::INFINITY));
        // Enter puts a changed FEN on the board; with moves in the game it asks first, New game
        // taking the keyboard (plan A8, W3).
        if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) && state.fen_text.trim() != state.current_fen() {
            let fen = state.fen_text.trim().to_owned();
            if state.game.is_empty() {
                let flipped = state.flipped;
                state.load_fen(&fen, flipped, None);
            } else {
                state.fen_choice = Some(fen);
                state.focus_fen_choice = true;
            }
        }
        if let Some(fen) = state.fen_choice.clone() {
            ui.label(egui::RichText::new(tl!("The game on the board has moves. Put this position:")).small());
            ui.horizontal_wrapped(|ui| {
                let new = ui.button(tl!("New game with this position")).on_hover_text(tl!("The game on the board keeps its moves."));
                if std::mem::take(&mut state.focus_fen_choice) {
                    new.request_focus();
                }
                if new.clicked() {
                    action = Some(BoardAction::NewGameAt(fen.clone()));
                    state.fen_choice = None;
                }
                let tip = tl!("Its moves are deleted (Undo brings them back).");
                if ui.small_button(tl!("Replace this game")).on_hover_text(tip).clicked() {
                    let flipped = state.flipped;
                    state.load_fen(&fen, flipped, None);
                    state.fen_choice = None;
                }
                if ui.small_button(tl!("Cancel")).clicked() {
                    state.fen_choice = None;
                    state.fen_text = state.current_fen();
                }
            });
        }
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
            // With moves on the board the start position is a new game (plan A8, W4).
            let tip = if state.game.is_empty() {
                tl!("Start again from the standard position.")
            } else {
                tl!("A new game from the standard position; the game on the board keeps its moves.")
            };
            if ui.small_button(tl!("Standard start position")).on_hover_text(tip).clicked() {
                if state.game.is_empty() {
                    state.standard_start();
                } else {
                    action = Some(BoardAction::NewGameAt(Game::standard().fen()));
                }
            }
        });
    });
    egui::CollapsingHeader::new("PGN").id_salt("chess-board-pgn").default_open(false).show(ui, |ui| {
        let id = ui.make_persistent_id("chess-pgn-field");
        crate::chess_annotate::insert_pending(ui, &mut state.typing, crate::chess_annotate::Field::Pgn, id, &mut state.pgn_text);
        let r = ui
            .add(egui::TextEdit::multiline(&mut state.pgn_text).id(id).font(egui::TextStyle::Monospace).desired_rows(5).desired_width(f32::INFINITY));
        if r.changed() {
            state.pgn_edited = true;
        }
        let mut load = false;
        // Typed and not loaded yet: said, with what can be done with it (plan A8, W6).
        if state.pgn_edited {
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(tl!("PGN edited")).small().strong());
                load = ui.small_button(tl!("Load")).clicked();
                if ui.small_button(tl!("Discard")).on_hover_text(tl!("Show the game on the board again.")).clicked() {
                    state.pgn_edited = false;
                    state.refresh_fields(true);
                }
            });
        }
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(state.illegal.is_none(), egui::Button::new(tl!("Copy PGN")).small()).clicked() {
                action = Some(BoardAction::CopyPgn(state.game.pgn()));
            }
            let tip = tl!("Load the PGN above on the board, with its variations and comments.");
            if ui.small_button(tl!("Load PGN")).on_hover_text(tip).clicked() {
                load = true;
            }
        });
        if load {
            let (texts, more) = state.load_pgn();
            if !texts.is_empty() || more {
                action = Some(BoardAction::AddGames { texts, more });
            }
        }
    });
    if state.illegal.is_none() {
        egui::CollapsingHeader::new(tl!("Game details")).id_salt("chess-board-details").default_open(false).show(ui, |ui| details(ui, state));
    }
    action
}

/// The panel's title, the diagram the board was loaded from as a link to it, and the Move list
/// beside the page (`beside`: it shows) as a toggle.
pub(crate) fn title(ui: &mut egui::Ui, t: &Tokens, state: &BoardState, source_label: Option<String>, beside: bool) -> Option<BoardAction> {
    let mut action = None;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tl!("Chess board")).font(theme::semibold(15.5)));
        if let (Some(label), Some(source)) = (source_label, state.source)
            && ui.link(egui::RichText::new(label).small()).on_hover_text(tl!("Show this diagram on the page")).clicked()
        {
            action = Some(BoardAction::ShowSource(source));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let name = tl!("Move list beside the page");
            let r = ui.add(egui::Button::image(crate::icons::image("list-ordered", 14.0, t.icon)).selected(beside)).on_hover_text(name);
            r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, beside, name));
            if r.clicked() {
                action = Some(BoardAction::ToggleMoveList);
            }
        });
    });
    action
}

/// The Chess board panel below its keys, in one column (spec §4.1), in a scroll area whose
/// visible part is `viewport` points tall: the board and its controls within `sizing`'s limits
/// (with the evaluation bar while the engine analyses), the Analysis section, then
/// [`moves_column`] in the height left. While the rail's Set up tool is armed the set-up view
/// takes the place of all but the title. `source_label` names the diagram the board was loaded
/// from.
pub(crate) fn panel_body(
    ui: &mut egui::Ui,
    t: &Tokens,
    chess: &mut crate::chess_ui::ChessState,
    pieces: &PieceSet,
    source_label: Option<String>,
    sizing: Sizing,
    viewport: f32,
) -> (Option<BoardAction>, Option<crate::chess_collection::GamesAction>) {
    let top = ui.cursor().top();
    let action = title(ui, t, chess.board(), source_label, chess.moves_beside);
    ui.add_space(4.0);
    if crate::chess_study_ui::setup_in_place(ui, t, chess, pieces) {
        return (action, None);
    }
    let (study, state) = (&mut chess.study, chess.games.board_mut());
    view_controls(ui, state);
    ui.add_space(4.0);
    crate::chess_study_ui::board_with_bar(ui, t, study, state, pieces, sizing);
    promotion_choice(ui, state);
    navigation(ui, t, state);
    let analysis = crate::chess_study_ui::analysis_section(ui, t, study, state, pieces);
    ui.add_space(4.0);
    let (fields, games) = moves_column(ui, t, chess, viewport - (ui.cursor().top() - top));
    (action.or(analysis).or(fields), games)
}

/// The notation taking `room` points (at least [`MIN_NOTATION`]) less what goes under it, then
/// the glyphs and comment of the move shown, the document's games and the closed FEN, PGN and
/// details sections: under the board in one column, or the right-hand column of the chess
/// window's wide layout (`chess_window`). What goes under the notation is measured each frame
/// and set aside the next, so opening a section shrinks the notation (down to its least height,
/// then the column scrolls). While the Move list shows beside the page, the notation and the
/// move's notes are there ([`moves_list`]), and only the games and sections are here.
pub(crate) fn moves_column(
    ui: &mut egui::Ui,
    t: &Tokens,
    chess: &mut crate::chess_ui::ChessState,
    room: f32,
) -> (Option<BoardAction>, Option<crate::chess_collection::GamesAction>) {
    if chess.moves_beside {
        let games = crate::chess_collection::games_section(ui, t, &mut chess.games);
        return (sections(ui, chess.board_mut()), games);
    }
    let below_id = ui.id().with("chess-board-below");
    let below = ui.ctx().data(|d| d.get_temp::<f32>(below_id)).unwrap_or(0.0);
    let height = notation_height(room, 0.0, below);
    let brush = chess.tools.brush();
    if let Some(code) = brush {
        brush_bar(ui, t, chess, code);
    }
    let state = chess.board_mut();
    let copied = notation(ui, t, state, Some(height), brush);
    let below_top = ui.cursor().top();
    notes(ui, t, state);
    ui.add_space(4.0);
    let games = crate::chess_collection::games_section(ui, t, &mut chess.games);
    let fields = sections(ui, chess.board_mut());
    set_aside(ui, below_id, ui.cursor().top() - below_top, below);
    (copied.or(fields), games)
}

/// The Move list beside the page (`chess_ui::moves_panel`): the glyph brush's bar while it is
/// armed, the notation in the `room` points left, and the glyphs and comment of the move shown
/// under it, measured as in [`moves_column`].
pub(crate) fn moves_list(ui: &mut egui::Ui, t: &Tokens, chess: &mut crate::chess_ui::ChessState, room: f32) -> Option<BoardAction> {
    let below_id = ui.id().with("chess-moves-below");
    let below = ui.ctx().data(|d| d.get_temp::<f32>(below_id)).unwrap_or(0.0);
    let top = ui.cursor().top();
    let brush = chess.tools.brush();
    if let Some(code) = brush {
        brush_bar(ui, t, chess, code);
    }
    let height = notation_height(room, ui.cursor().top() - top, below);
    let state = chess.board_mut();
    let action = notation(ui, t, state, Some(height), brush);
    let below_top = ui.cursor().top();
    notes(ui, t, state);
    set_aside(ui, below_id, ui.cursor().top() - below_top, below);
    action
}

/// Keep `measured` points (what went under the notation) for the next frame under `id`, and draw
/// that frame when it differs from the `kept` ones.
fn set_aside(ui: &egui::Ui, id: egui::Id, measured: f32, kept: f32) {
    if measured.is_finite() && (measured - kept).abs() > 0.5 {
        ui.ctx().data_mut(|d| d.insert_temp(id, measured));
        ui.ctx().request_repaint();
    }
}

/// The armed glyph brush (T15), over the notation: its glyph, what a click does, and how to stop.
fn brush_bar(ui: &mut egui::Ui, t: &Tokens, chess: &mut crate::chess_ui::ChessState, code: u16) {
    let ctx = ui.ctx().clone();
    let seen = crate::chess_annotate::seen(code, |s| crate::chess_annotate::drawable(&ctx, s));
    let name = crate::chess_annotate::nag_name(code).unwrap_or_default();
    // A painted sign is drawn beside the brush, and the text names it only.
    let face = match &seen {
        crate::chess_annotate::Seen::Text(text) => text.clone(),
        crate::chess_annotate::Seen::Icon(_) | crate::chess_annotate::Seen::Name(_) => String::new(),
    };
    ui.horizontal_wrapped(|ui| {
        ui.add(crate::icons::image("brush", 14.0, t.icon));
        if let crate::chess_annotate::Seen::Icon(icon) = seen {
            crate::chess_annotate::icon_label(ui, icon, 14.0, t.icon, name);
        }
        let text =
            crate::i18n::fmt(tl!("Glyph brush {glyph} ({name}): click moves to put it on or take it off."), &[("glyph", &face), ("name", name)])
                .replace("  ", " ");
        ui.label(egui::RichText::new(text).small());
        if ui.small_button(tl!("Stop (Esc)")).clicked() {
            chess.tools.tool = crate::chess_tools::PgnTool::Navigate;
        }
    });
}

/// The notation's height in a panel `viewport` points tall, with `above` points drawn above it
/// and `below` to go under it: what is left, and at least [`MIN_NOTATION`].
fn notation_height(viewport: f32, above: f32, below: f32) -> f32 {
    // A little to spare, so rounding never makes the panel scroll by a point.
    let left = viewport - above - below - 4.0;
    if left.is_finite() { left.max(MIN_NOTATION) } else { MIN_NOTATION }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sq(name: &str) -> Square {
        Square::parse(name).unwrap()
    }

    /// The PGN field loaded with the same game (a comment and a variation added) keeps the move
    /// shown and where the moves are on the page; a different game is left as read (plan A8, W6).
    #[test]
    fn loading_the_same_game_keeps_the_move_shown_and_the_sources() {
        use pdfcraft_engine::chess::pdfcraft_chess::game::{NodePath, NodeSource};
        let mut s = BoardState { pgn_text: "1. e4 e5 2. Nf3 Nc6 *".into(), ..BoardState::default() };
        s.load_pgn();
        let e5 = s.game.node_at(&NodePath::new(vec![0, 0])).unwrap();
        let printed = NodeSource { page: 4, rect_user: [10.0, 20.0, 30.0, 40.0] };
        assert!(s.game.set_source(e5, Some(printed)));
        s.game.go_to_node(e5);
        s.pgn_text = "1. e4 e5 {Open game} (1... c5) 2. Nf3 Nc6 *".into();
        s.pgn_edited = true;
        s.load_pgn();
        assert_eq!(s.game.movetext(), "1. e4 e5 {Open game} (1... c5) 2. Nf3 Nc6");
        let e5 = s.game.node_at(&NodePath::new(vec![0, 0])).unwrap();
        assert_eq!(s.game.current(), e5, "the move shown is kept");
        assert_eq!(s.game.source(e5), Some(printed), "and where it is printed");
        assert!(!s.pgn_edited, "loaded");
        // Another main line is another game: read from its end, without the old sources.
        s.pgn_text = "1. d4 d5 *".into();
        s.load_pgn();
        assert_eq!(s.game.ply(), 2);
        assert_eq!(s.game.source(s.game.node_at(&NodePath::new(vec![0, 0])).unwrap()), None);
    }

    /// Typing in the PGN field survives a step on the board until the field is loaded or the
    /// edit discarded (plan A8, W6).
    #[test]
    fn typed_pgn_is_kept_while_the_board_moves() {
        let mut s = BoardState { pgn_text: "1. e4 e5 *".into(), ..BoardState::default() };
        s.load_pgn();
        s.refresh_fields(false);
        s.pgn_text = "1. e4 e5 2. Nf3 *".into();
        s.pgn_edited = true;
        s.game.go_to(1);
        s.refresh_fields(false);
        assert_eq!(s.pgn_text, "1. e4 e5 2. Nf3 *", "a step leaves the typed text");
        assert_ne!(s.fen_text, "", "the FEN follows the board");
        s.pgn_edited = false;
        s.refresh_fields(true);
        assert!(s.pgn_text.ends_with("1. e4 e5 *\n"), "discarded: the game again: {}", s.pgn_text);
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
    fn variations_get_rows_of_their_own_indented_by_depth() {
        let (g, _) = Game::from_pgn("1. e4 {Open} e5 (1... c5 2. Nf3 (2. c3 d5 (2... Nf6 3. e5)) d6) 2. Nf3 *").unwrap();
        let tokens = g.tokens();
        let rows: Vec<(usize, String)> = rows(&tokens)
            .iter()
            .map(|r| {
                let words: Vec<String> = r
                    .tokens
                    .iter()
                    .map(|t| match t {
                        Token::Move { san, .. } => san.clone(),
                        Token::Comment { text, .. } => format!("{{{text}}}"),
                        Token::VariationStart => "(".to_owned(),
                        Token::VariationEnd => ")".to_owned(),
                    })
                    .collect();
                (r.depth, words.join(" "))
            })
            .collect();
        let expected = [(0, "e4 {Open} e5"), (1, "( c5 Nf3"), (2, "( c3 d5"), (3, "( Nf6 e5 ) )"), (1, "d6 )"), (0, "Nf3")];
        assert_eq!(rows, expected.map(|(d, s)| (d, s.to_owned())));
        assert!(super::rows(&[]).is_empty());
        assert_eq!(notation_height(800.0, 500.0, 100.0), 196.0);
        assert_eq!(notation_height(400.0, 500.0, 100.0), MIN_NOTATION);
        assert_eq!(notation_height(f32::INFINITY, 500.0, f32::INFINITY), MIN_NOTATION, "NaN is not a height");
    }

    #[test]
    fn glyphs_read_as_books_print_them() {
        let previous = crate::i18n::current();
        crate::i18n::set_current(crate::i18n::Lang::EN);
        // Fonts without the slight-advantage signs, as the interface's own.
        let fonts = |s: &str| !s.contains(['⩲', '⩱']);
        assert_eq!(glyphs(&[1], fonts), "!");
        assert_eq!(glyphs(&[5, 14], fonts), "!? +=");
        assert_eq!(glyphs(&[15, 18], fonts), " =+ +-");
        assert_eq!(glyphs(&[999], fonts), " $999");
        assert_eq!(glyphs(&[14], |_| true), " ⩲", "a font with the sign shows it");
        // Painted signs and assessments without one come after the label, not in it.
        assert_eq!(glyphs(&[1, 142, 24, 26], |_| true), "! ○");
        assert_eq!(nag_text(142), "Better is");
        assert_eq!(nag_text(25), "Slight space advantage");
        assert_eq!(nag_text(27), "○");
        assert_eq!(nag_text(14), "+=");
        crate::i18n::set_current(previous);
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

    /// Several games in the field: the first goes on the board, the others come back as texts
    /// for the list, capped as Open PGN… caps a file.
    #[test]
    fn the_pgn_field_loads_its_first_game_and_hands_back_the_others() {
        let three = "[Event \"A\"]\n\n1. e4 *\n\n[Event \"B\"]\n\n1. d4 *\n\n[Event \"C\"]\n\n1. c4 *\n";
        let mut s = BoardState { pgn_text: three.into(), ..BoardState::default() };
        let (rest, more) = s.load_pgn();
        assert_eq!(s.game.movetext(), "1. e4");
        assert_eq!(s.game.tag("Event"), Some("A"));
        assert_eq!(rest.len(), 2);
        assert!(rest[0].contains("1. d4") && rest[1].contains("1. c4") && !more);
        assert_eq!(s.undo_label().as_deref(), Some("Load the PGN"), "one step of the game");
        // One game: nothing more.
        s.pgn_text = "1. d4 d5 *".into();
        assert_eq!(s.load_pgn(), (Vec::new(), false));
        // Past the cap: as many as Open PGN… reads, and said.
        let max = crate::chess_autosave::MAX_GAMES;
        s.pgn_text = (0..max + 5).map(|i| format!("[Round \"{i}\"]\n\n1. e4 *\n\n")).collect();
        let (rest, more) = s.load_pgn();
        assert_eq!((rest.len(), more), (max - 1, true));
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
