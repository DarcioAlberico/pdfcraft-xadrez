//! A game played out from a position: the moves made on an analysis board, the ply being looked
//! at, and the game as PGN.
//!
//! The board is a line, not a tree. A move made at an earlier ply replaces everything after it,
//! unless it is the move already there, which just steps forward. Moves are named by their
//! squares, the way a board is clicked: castling is the king moving two files (or onto its own
//! rook), and a pawn reaching the last rank needs the piece it becomes.

use std::fmt;

use cozy_chess::{Board, GameStatus, Move};

use crate::fen::Position;
use crate::placement::{Color, Piece, PieceKind, Placement, Square};
use crate::san::{San, color_from_cozy, kind_from_cozy, square_from_cozy, square_to_cozy, to_cozy_kind};

/// Most plies a game holds. Real games are under 600.
pub const MAX_PLIES: usize = 2000;

/// Longest PGN read, in bytes.
pub const MAX_PGN_LEN: usize = 1 << 20;

/// Why a game could not be set up or a move made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GameError {
    /// The position is not legal (two kings, the side not to move in check…), so no move can be
    /// played from it.
    IllegalPosition,
    /// The FEN could not be read.
    BadFen(String),
    /// That move is not legal here.
    IllegalMove,
    /// A pawn reaches the last rank: say what it becomes.
    NeedsPromotion,
    /// The game is as long as a game may be.
    TooLong,
    /// The PGN is longer than [`MAX_PGN_LEN`].
    PgnTooLong,
}

impl fmt::Display for GameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GameError::IllegalPosition => write!(f, "the position is not legal"),
            GameError::BadFen(e) => write!(f, "the FEN could not be read: {e}"),
            GameError::IllegalMove => write!(f, "that move is not legal"),
            GameError::NeedsPromotion => write!(f, "the pawn needs a piece to become"),
            GameError::TooLong => write!(f, "the game is too long"),
            GameError::PgnTooLong => write!(f, "the PGN is too long"),
        }
    }
}

impl std::error::Error for GameError {}

/// How the game stands at the ply shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Ongoing,
    /// The side to move is mated.
    Checkmate,
    Stalemate,
    /// Fifty moves without a capture or a pawn move.
    FiftyMoves,
}

/// A game from a start position.
#[derive(Clone, Debug)]
pub struct Game {
    /// The board before each ply: `boards[0]` is the start, `boards[k]` the board after `k` plies.
    boards: Vec<Board>,
    moves: Vec<Move>,
    sans: Vec<String>,
    /// The ply shown (0 = the start).
    ply: usize,
}

impl Default for Game {
    fn default() -> Game {
        Game::standard()
    }
}

impl Game {
    /// A game from the standard start position.
    pub fn standard() -> Game {
        Game::from_board(Board::default())
    }

    fn from_board(board: Board) -> Game {
        Game { boards: vec![board], moves: Vec::new(), sans: Vec::new(), ply: 0 }
    }

    /// A game from `position`, `side` to move when the position does not say.
    pub fn new(position: &Position, side: Color) -> Result<Game, GameError> {
        let board = position.to_board(side).map_err(|_| GameError::IllegalPosition)?;
        Ok(Game::from_board(board))
    }

    /// A game from a FEN (only the placement is required; White moves when it does not say).
    pub fn from_fen(fen: &str) -> Result<Game, GameError> {
        let position = crate::fen::parse(fen.trim()).map_err(|e| GameError::BadFen(e.to_string()))?;
        Game::new(&position, Color::White)
    }

    /// The board at `ply`, the start when out of range (never: `ply` is kept in range).
    fn board_at(&self, ply: usize) -> &Board {
        // `boards` is never empty: every constructor puts the start in it, and `truncate` keeps it.
        match self.boards.get(ply).or_else(|| self.boards.first()) {
            Some(board) => board,
            None => unreachable_board(),
        }
    }

    /// The board shown.
    fn board(&self) -> &Board {
        self.board_at(self.ply)
    }

    /// The ply shown (0 = the start).
    pub fn ply(&self) -> usize {
        self.ply
    }

    /// How many plies the game has.
    pub fn len(&self) -> usize {
        self.moves.len()
    }

    /// Whether no move was made.
    pub fn is_empty(&self) -> bool {
        self.moves.is_empty()
    }

    /// Show ply `ply` (clamped to the game).
    pub fn go_to(&mut self, ply: usize) {
        self.ply = ply.min(self.moves.len());
    }

    /// One ply back; `false` at the start.
    pub fn back(&mut self) -> bool {
        let moved = self.ply > 0;
        self.ply = self.ply.saturating_sub(1);
        moved
    }

    /// One ply forward; `false` at the end.
    pub fn forward(&mut self) -> bool {
        let moved = self.ply < self.moves.len();
        self.go_to(self.ply.saturating_add(1));
        moved
    }

    /// The pieces shown.
    pub fn placement(&self) -> Placement {
        placement_of(self.board())
    }

    /// The FEN of the board shown.
    pub fn fen(&self) -> String {
        self.board().to_string()
    }

    /// The FEN of the start.
    pub fn start_fen(&self) -> String {
        self.board_at(0).to_string()
    }

    /// Whose move it is on the board shown.
    pub fn side_to_move(&self) -> Color {
        color_from_cozy(self.board().side_to_move())
    }

    /// Whether the side to move is in check.
    pub fn in_check(&self) -> bool {
        !self.board().checkers().is_empty()
    }

    /// How the game stands on the board shown.
    pub fn outcome(&self) -> Outcome {
        let board = self.board();
        match board.status() {
            GameStatus::Ongoing => Outcome::Ongoing,
            GameStatus::Won => Outcome::Checkmate,
            GameStatus::Drawn if board.halfmove_clock() >= 100 && has_moves(board) => Outcome::FiftyMoves,
            GameStatus::Drawn => Outcome::Stalemate,
        }
    }

    /// The legal moves on the board shown, as (from, to) the way they are clicked: castling is the
    /// king's two-file step.
    fn clicks(&self) -> Vec<(Square, Square, Move)> {
        let board = self.board();
        let mut out = Vec::new();
        board.generate_moves(|piece_moves| {
            for mv in piece_moves {
                let (from, to) = click_of(board, mv);
                out.push((from, to, mv));
            }
            false
        });
        out
    }

    /// The squares the piece on `from` can go to.
    pub fn targets(&self, from: Square) -> Vec<Square> {
        let mut out: Vec<Square> = self.clicks().into_iter().filter(|(f, _, _)| *f == from).map(|(_, t, _)| t).collect();
        out.sort();
        out.dedup();
        out
    }

    /// Whether moving `from` → `to` is a promotion (and so needs a piece).
    pub fn is_promotion(&self, from: Square, to: Square) -> bool {
        self.clicks().iter().any(|(f, t, mv)| *f == from && *t == to && mv.promotion.is_some())
    }

    /// The piece on `square` of the board shown.
    pub fn piece_on(&self, square: Square) -> Option<Piece> {
        self.placement().get(square)
    }

    /// Play `from` → `to` at the ply shown (`promotion` for a pawn reaching the last rank). The
    /// moves after the ply shown are replaced, unless this is the next one already.
    pub fn play(&mut self, from: Square, to: Square, promotion: Option<PieceKind>) -> Result<(), GameError> {
        let candidates: Vec<Move> = self.clicks().into_iter().filter(|(f, t, _)| *f == from && *t == to).map(|(_, _, mv)| mv).collect();
        if candidates.is_empty() {
            // The king onto its own rook is castling too (cozy-chess's own notation).
            let (Some(f), Some(t)) = (square_to_cozy(from), square_to_cozy(to)) else { return Err(GameError::IllegalMove) };
            let mv = Move { from: f, to: t, promotion: None };
            return if self.board().is_legal(mv) { self.push(mv) } else { Err(GameError::IllegalMove) };
        }
        // A piece given for a move that is not a promotion is ignored.
        let mv = match (candidates.as_slice(), promotion) {
            ([mv], _) if mv.promotion.is_none() => *mv,
            (_, Some(kind)) => {
                let want = to_cozy_kind(kind);
                candidates.into_iter().find(|mv| mv.promotion == Some(want)).ok_or(GameError::IllegalMove)?
            }
            (_, None) => return Err(GameError::NeedsPromotion),
        };
        self.push(mv)
    }

    /// Play a move written in SAN (English letters, or as books print it) at the ply shown.
    pub fn play_san(&mut self, text: &str) -> Result<(), GameError> {
        let san = San::parse(text).map_err(|_| GameError::IllegalMove)?;
        let mv = san.to_move(self.board()).map_err(|_| GameError::IllegalMove)?;
        self.push(mv)
    }

    fn push(&mut self, mv: Move) -> Result<(), GameError> {
        if self.moves.get(self.ply) == Some(&mv) {
            self.ply = self.ply.saturating_add(1);
            return Ok(());
        }
        if self.ply >= MAX_PLIES {
            return Err(GameError::TooLong);
        }
        let board = self.board().clone();
        let san = San::from_move(&board, mv).map_err(|_| GameError::IllegalMove)?;
        let mut after = board;
        after.try_play(mv).map_err(|_| GameError::IllegalMove)?;
        self.moves.truncate(self.ply);
        self.sans.truncate(self.ply);
        self.boards.truncate(self.ply.saturating_add(1));
        self.moves.push(mv);
        self.sans.push(san.to_english());
        self.boards.push(after);
        self.ply = self.moves.len();
        Ok(())
    }

    /// Drop the moves after the ply shown.
    pub fn truncate(&mut self) {
        self.moves.truncate(self.ply);
        self.sans.truncate(self.ply);
        self.boards.truncate(self.ply.saturating_add(1));
    }

    /// The move that led to the board shown, as (from, to) clicked.
    pub fn last_move(&self) -> Option<(Square, Square)> {
        let k = self.ply.checked_sub(1)?;
        let mv = *self.moves.get(k)?;
        Some(click_of(self.board_at(k), mv))
    }

    /// The moves in SAN, each with its number and whose it was: `(ply, number, color, san)`, `ply`
    /// being the board after the move.
    pub fn moves(&self) -> Vec<(usize, u32, Color, &str)> {
        self.sans
            .iter()
            .enumerate()
            .map(|(k, san)| {
                let before = self.board_at(k);
                (k.saturating_add(1), u32::from(before.fullmove_number()), color_from_cozy(before.side_to_move()), san.as_str())
            })
            .collect()
    }

    /// The moves as PGN movetext: `1. e4 e5 2. Nf3`, or `35... Kg7 36. Rd1` from Black's move.
    pub fn movetext(&self) -> String {
        let mut out = String::new();
        for (k, (_, number, color, san)) in self.moves().into_iter().enumerate() {
            if !out.is_empty() {
                out.push(' ');
            }
            match color {
                Color::White => out.push_str(&format!("{number}. ")),
                Color::Black if k == 0 => out.push_str(&format!("{number}... ")),
                Color::Black => {}
            }
            out.push_str(san);
        }
        out
    }

    /// The result tag: decided only by a mate or a draw at the end of the game.
    fn result(&self) -> &'static str {
        let end = self.board_at(self.moves.len());
        match end.status() {
            GameStatus::Won if end.side_to_move() == cozy_chess::Color::White => "0-1",
            GameStatus::Won => "1-0",
            GameStatus::Drawn => "1/2-1/2",
            GameStatus::Ongoing => "*",
        }
    }

    /// The game as PGN: the seven tags, the start position when it is not the standard one, and
    /// the moves.
    pub fn pgn(&self) -> String {
        let result = self.result();
        let mut out = String::new();
        for (tag, value) in
            [("Event", "?"), ("Site", "?"), ("Date", "????.??.??"), ("Round", "?"), ("White", "?"), ("Black", "?"), ("Result", result)]
        {
            out.push_str(&format!("[{tag} \"{value}\"]\n"));
        }
        let start = self.start_fen();
        if start != Board::default().to_string() {
            out.push_str("[SetUp \"1\"]\n");
            out.push_str(&format!("[FEN \"{start}\"]\n"));
        }
        out.push('\n');
        let moves = self.movetext();
        if moves.is_empty() {
            out.push_str(result);
        } else {
            out.push_str(&format!("{moves} {result}"));
        }
        out.push('\n');
        out
    }

    /// Read a PGN's main line: its `FEN` tag (the standard start without one) and its moves, with
    /// comments, variations, move numbers, NAGs and the result skipped. Reading stops at the first
    /// move that is not legal; that move is returned with the game read so far. The game is shown
    /// at its end.
    pub fn from_pgn(text: &str) -> Result<(Game, Option<String>), GameError> {
        if text.len() > MAX_PGN_LEN {
            return Err(GameError::PgnTooLong);
        }
        let mut game = match fen_tag(text) {
            Some(fen) => Game::from_fen(&fen)?,
            None => Game::standard(),
        };
        for token in movetext_tokens(text) {
            if game.len() >= MAX_PLIES {
                return Ok((game, Some(token)));
            }
            if game.play_san(&token).is_err() {
                return Ok((game, Some(token)));
            }
        }
        Ok((game, None))
    }
}

/// Never called: `boards` always holds the start. A board to return instead of panicking.
#[cold]
fn unreachable_board() -> &'static Board {
    static START: std::sync::OnceLock<Board> = std::sync::OnceLock::new();
    START.get_or_init(Board::default)
}

fn has_moves(board: &Board) -> bool {
    board.generate_moves(|_| true)
}

/// A move as it is clicked: castling (the king onto its own rook in cozy-chess) is the king's
/// two-file step.
fn click_of(board: &Board, mv: Move) -> (Square, Square) {
    let from = square_from_cozy(mv.from);
    let to = square_from_cozy(mv.to);
    let castles = board.piece_on(mv.from) == Some(cozy_chess::Piece::King) && board.color_on(mv.to) == Some(board.side_to_move());
    if castles {
        let file = if to.file() > from.file() { 6 } else { 2 };
        return (from, Square::new(file, from.rank()).unwrap_or(to));
    }
    (from, to)
}

/// The pieces on a cozy-chess board.
fn placement_of(board: &Board) -> Placement {
    let mut placement = Placement::empty();
    for sq in board.occupied() {
        if let (Some(kind), Some(color)) = (board.piece_on(sq), board.color_on(sq)) {
            placement.set(square_from_cozy(sq), Some(Piece { color: color_from_cozy(color), kind: kind_from_cozy(kind) }));
        }
    }
    placement
}

/// The value of the `FEN` tag, when there is one.
fn fen_tag(text: &str) -> Option<String> {
    text.lines().map(str::trim).find_map(|line| {
        let inner = line.strip_prefix('[')?.strip_suffix(']')?.trim();
        let rest = inner.strip_prefix("FEN")?.trim();
        Some(rest.strip_prefix('"')?.strip_suffix('"')?.to_owned())
    })
}

/// The move tokens of the main line, in order.
fn movetext_tokens(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut depth: usize = 0; // inside variations
    let mut in_brace = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if !in_brace && depth == 0 && trimmed.starts_with('[') {
            continue; // a tag
        }
        if !in_brace && trimmed.starts_with('%') {
            continue; // an escaped line
        }
        let mut word = String::new();
        let mut flush = |word: &mut String, depth: usize| {
            if depth == 0 && !word.is_empty() && is_move(word) {
                tokens.push(std::mem::take(word));
            }
            word.clear();
        };

        for c in line.chars() {
            if in_brace {
                in_brace = c != '}';
                continue;
            }
            match c {
                '{' => {
                    flush(&mut word, depth);
                    in_brace = true;
                }
                ';' => {
                    flush(&mut word, depth);
                    break; // a comment to the end of the line
                }
                '(' => {
                    flush(&mut word, depth);
                    depth = depth.saturating_add(1);
                }
                ')' => {
                    flush(&mut word, depth);
                    depth = depth.saturating_sub(1);
                }
                c if c.is_whitespace() => flush(&mut word, depth),
                // A move number glued to its move: `1.e4`, `12...Nf6`.
                '.' => {
                    if word.chars().all(|c| c.is_ascii_digit()) {
                        word.clear();
                    } else {
                        word.push(c);
                    }
                }
                c => word.push(c),
            }
        }
        flush(&mut word, depth);
    }
    tokens
}

/// Whether a word of movetext is a move (not a number, NAG or result).
fn is_move(word: &str) -> bool {
    !(word.starts_with('$') || word.chars().all(|c| c.is_ascii_digit()) || matches!(word, "*" | "1-0" | "0-1" | "1/2-1/2" | "½-½"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sq(name: &str) -> Square {
        Square::parse(name).unwrap()
    }

    #[test]
    fn plays_moves_by_squares_and_names_them() {
        let mut g = Game::standard();
        g.play(sq("e2"), sq("e4"), None).unwrap();
        g.play(sq("e7"), sq("e5"), None).unwrap();
        g.play(sq("g1"), sq("f3"), None).unwrap();
        assert_eq!(g.movetext(), "1. e4 e5 2. Nf3");
        assert_eq!(g.last_move(), Some((sq("g1"), sq("f3"))));
        assert_eq!(g.side_to_move(), Color::Black);
        assert_eq!(g.play(sq("e5"), sq("e3"), None), Err(GameError::IllegalMove));
    }

    #[test]
    fn castling_is_the_kings_two_file_step() {
        let mut g = Game::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").unwrap();
        assert!(g.targets(sq("e1")).contains(&sq("g1")));
        assert!(g.targets(sq("e1")).contains(&sq("c1")));
        g.play(sq("e1"), sq("g1"), None).unwrap();
        assert_eq!(g.movetext(), "1. O-O");
        assert_eq!(g.last_move(), Some((sq("e1"), sq("g1"))));
        assert_eq!(g.piece_on(sq("f1")).map(|p| p.kind), Some(PieceKind::Rook));
        // The king onto its own rook is castling too.
        g.play(sq("e8"), sq("a8"), None).unwrap();
        assert_eq!(g.movetext(), "1. O-O O-O-O");
    }

    #[test]
    fn promotion_needs_its_piece() {
        let mut g = Game::from_fen("8/P6k/8/8/8/8/8/K7 w - - 0 1").unwrap();
        assert!(g.is_promotion(sq("a7"), sq("a8")));
        assert_eq!(g.play(sq("a7"), sq("a8"), None), Err(GameError::NeedsPromotion));
        g.play(sq("a7"), sq("a8"), Some(PieceKind::Knight)).unwrap();
        assert_eq!(g.movetext(), "1. a8=N");
    }

    #[test]
    fn a_move_at_an_earlier_ply_replaces_the_rest_unless_it_is_the_same() {
        let mut g = Game::standard();
        for (f, t) in [("e2", "e4"), ("e7", "e5"), ("g1", "f3")] {
            g.play(sq(f), sq(t), None).unwrap();
        }
        g.go_to(1);
        g.play(sq("e7"), sq("e5"), None).unwrap();
        assert_eq!((g.ply(), g.len()), (2, 3), "the same move steps forward");
        g.go_to(1);
        g.play(sq("c7"), sq("c5"), None).unwrap();
        assert_eq!(g.movetext(), "1. e4 c5");
        assert_eq!(g.ply(), 2);
    }

    #[test]
    fn navigation_stays_in_range() {
        let mut g = Game::standard();
        assert!(!g.back());
        assert!(!g.forward());
        g.play(sq("d2"), sq("d4"), None).unwrap();
        g.go_to(99);
        assert_eq!(g.ply(), 1);
        assert!(g.back());
        assert_eq!(g.fen(), Board::default().to_string());
        assert_eq!(g.last_move(), None);
    }

    #[test]
    fn numbering_starts_from_the_position() {
        let mut g = Game::from_fen("6k1/5ppp/8/8/8/8/5PPP/3R2K1 b - - 0 35").unwrap();
        g.play(sq("g8"), sq("h8"), None).unwrap();
        g.play(sq("d1"), sq("d8"), None).unwrap();
        assert_eq!(g.movetext(), "35... Kh8 36. Rd8#");
        assert_eq!(g.outcome(), Outcome::Checkmate);
        let pgn = g.pgn();
        assert!(pgn.contains("[FEN \"6k1/5ppp/8/8/8/8/5PPP/3R2K1 b - - 0 35\"]"), "{pgn}");
        assert!(pgn.trim_end().ends_with("35... Kh8 36. Rd8# 1-0"), "{pgn}");
    }

    #[test]
    fn illegal_positions_are_refused() {
        assert_eq!(Game::from_fen("8/8/8/8/8/8/8/8 w - - 0 1").err(), Some(GameError::IllegalPosition));
        assert!(matches!(Game::from_fen("not a fen"), Err(GameError::BadFen(_))));
        // White to move with Black in check.
        assert_eq!(Game::from_fen("4k3/4Q3/8/8/8/8/8/4K3 w - - 0 1").err(), Some(GameError::IllegalPosition));
    }

    #[test]
    fn pgn_round_trips_and_skips_what_is_not_the_main_line() {
        let text = "[Event \"x\"]\n[FEN \"r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1\"]\n\n1.O-O {short} (1. O-O-O O-O) 1...O-O-O $1 ; note\n2. Ra7 *";
        let (g, stopped) = Game::from_pgn(text).unwrap();
        assert_eq!(stopped, None);
        assert_eq!(g.movetext(), "1. O-O O-O-O 2. Ra7");
        assert_eq!(g.ply(), 3);
        let (again, stopped) = Game::from_pgn(&g.pgn()).unwrap();
        assert_eq!(stopped, None);
        assert_eq!(again.movetext(), g.movetext());
        assert_eq!(again.start_fen(), g.start_fen());
    }

    #[test]
    fn pgn_reading_stops_at_the_first_illegal_move() {
        let (g, stopped) = Game::from_pgn("1. e4 e5 2. Ke3 Nc6").unwrap();
        assert_eq!(g.movetext(), "1. e4 e5");
        assert_eq!(stopped.as_deref(), Some("Ke3"));
        assert_eq!(Game::from_pgn(&"e".repeat(MAX_PGN_LEN + 1)).err(), Some(GameError::PgnTooLong));
    }

    #[test]
    fn standard_game_pgn_has_no_fen_tag() {
        let mut g = Game::standard();
        g.play(sq("e2"), sq("e4"), None).unwrap();
        let pgn = g.pgn();
        assert!(!pgn.contains("FEN"), "{pgn}");
        assert!(pgn.ends_with("1. e4 *\n"), "{pgn}");
    }

    #[test]
    fn stalemate_is_told_apart_from_mate() {
        let g = Game::from_fen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1").unwrap();
        assert_eq!(g.outcome(), Outcome::Stalemate);
        assert!(!g.in_check());
    }

    proptest::proptest! {
        #[test]
        fn pgn_reader_never_panics(text in "\\PC{0,200}") {
            let _ = Game::from_pgn(&text);
        }

        #[test]
        fn random_clicks_never_panic(clicks in proptest::collection::vec((0usize..64, 0usize..64, 0usize..7), 0..60)) {
            let mut g = Game::standard();
            for (f, t, back) in clicks {
                let (Some(f), Some(t)) = (Square::from_index(f), Square::from_index(t)) else { continue };
                let _ = g.play(f, t, Some(PieceKind::Queen));
                if back == 0 { g.back(); }
                proptest::prop_assert!(g.ply() <= g.len());
                let _ = g.last_move();
            }
        }
    }
}
