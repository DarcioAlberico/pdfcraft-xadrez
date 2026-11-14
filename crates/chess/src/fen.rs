//! FEN parsing and formatting, tolerant enough for diagrams that carry only the piece placement.
//!
//! A book diagram says where the pieces stand and nothing else, so only the placement field is
//! required. The other five fields are read when present and otherwise left unknown. Fields may
//! be separated by spaces or by `_`, the form lichess URLs use.

use std::fmt;

use crate::placement::{Color, Piece, PieceKind, Placement, Square};
use crate::san::{color_from_cozy, kind_from_cozy, square_from_cozy};

/// Longest FEN accepted. A full FEN is under 100 bytes; anything far longer is not one.
pub const MAX_FEN_LEN: usize = 512;

/// Why a FEN was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FenError {
    /// The text is empty or has no placement field.
    Empty,
    /// Longer than [`MAX_FEN_LEN`] bytes.
    TooLong,
    /// The placement does not have 8 ranks.
    RankCount(usize),
    /// Rank `rank` (8 = the first one written) does not describe 8 squares.
    RankLength { rank: u8 },
    /// A character that is neither a piece letter nor a digit 1-8.
    BadChar(char),
    /// The side-to-move field is not `w` or `b`.
    BadSide(String),
    /// The castling field is not `-` or a subset of `KQkq`.
    BadCastling(String),
    /// The en passant field is not `-` or a square on the third or sixth rank.
    BadEnPassant(String),
    /// A move counter is not a number.
    BadCounter(String),
}

impl fmt::Display for FenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FenError::Empty => write!(f, "the FEN is empty"),
            FenError::TooLong => write!(f, "the FEN is longer than {MAX_FEN_LEN} bytes"),
            FenError::RankCount(n) => write!(f, "the placement has {n} ranks, not 8"),
            FenError::RankLength { rank } => write!(f, "rank {rank} does not describe 8 squares"),
            FenError::BadChar(c) => write!(f, "'{c}' is not a piece letter or a digit 1-8"),
            FenError::BadSide(s) => write!(f, "side to move '{s}' is not 'w' or 'b'"),
            FenError::BadCastling(s) => write!(f, "castling field '{s}' is not '-' or a subset of 'KQkq'"),
            FenError::BadEnPassant(s) => write!(f, "en passant field '{s}' is not '-' or a square on rank 3 or 6"),
            FenError::BadCounter(s) => write!(f, "move counter '{s}' is not a number"),
        }
    }
}

impl std::error::Error for FenError {}

/// Castling rights.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Castling {
    pub white_king_side: bool,
    pub white_queen_side: bool,
    pub black_king_side: bool,
    pub black_queen_side: bool,
}

impl Castling {
    /// `KQkq`, a subset of it, or `-`.
    pub fn fen(self) -> String {
        let mut s = String::new();
        for (on, c) in [(self.white_king_side, 'K'), (self.white_queen_side, 'Q'), (self.black_king_side, 'k'), (self.black_queen_side, 'q')] {
            if on {
                s.push(c);
            }
        }
        if s.is_empty() {
            s.push('-');
        }
        s
    }

    /// The rights `placement` still allows: each needs its side's king on its first square
    /// (e1, e8) and the rook in that corner.
    pub fn fitted(self, placement: &Placement) -> Castling {
        let on = |file: u8, rank: u8, color: Color, kind: PieceKind| {
            Square::new(file, rank).is_some_and(|sq| placement.get(sq) == Some(Piece::new(color, kind)))
        };
        let white_king = on(4, 0, Color::White, PieceKind::King);
        let black_king = on(4, 7, Color::Black, PieceKind::King);
        Castling {
            white_king_side: self.white_king_side && white_king && on(7, 0, Color::White, PieceKind::Rook),
            white_queen_side: self.white_queen_side && white_king && on(0, 0, Color::White, PieceKind::Rook),
            black_king_side: self.black_king_side && black_king && on(7, 7, Color::Black, PieceKind::Rook),
            black_queen_side: self.black_queen_side && black_king && on(0, 7, Color::Black, PieceKind::Rook),
        }
    }

    fn parse(field: &str) -> Result<Castling, FenError> {
        let mut c = Castling::default();
        if field == "-" {
            return Ok(c);
        }
        for ch in field.chars() {
            let flag = match ch {
                'K' => &mut c.white_king_side,
                'Q' => &mut c.white_queen_side,
                'k' => &mut c.black_king_side,
                'q' => &mut c.black_queen_side,
                _ => return Err(FenError::BadCastling(field.to_owned())),
            };
            if *flag {
                return Err(FenError::BadCastling(field.to_owned()));
            }
            *flag = true;
        }
        Ok(c)
    }
}

/// A position as far as it is known: the placement always, the rest when the source says so.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Position {
    pub placement: Placement,
    /// `None` when the source did not say (a book diagram usually does not).
    pub side_to_move: Option<Color>,
    pub castling: Castling,
    pub en_passant: Option<Square>,
    pub halfmove_clock: u32,
    pub fullmove_number: u32,
}

impl Position {
    /// A position known only by its placement.
    pub fn from_placement(placement: Placement) -> Position {
        Position { placement, side_to_move: None, castling: Castling::default(), en_passant: None, halfmove_clock: 0, fullmove_number: 1 }
    }

    /// The full FEN. An unknown side to move is written as `w`, the convention for diagrams;
    /// use [`Position::fen_tail`] to know whether the side was actually known.
    pub fn fen(&self) -> String {
        let tail = self.fen_tail().unwrap_or_else(|| self.tail_with(Color::White));
        format!("{} {}", format_placement(&self.placement), tail)
    }

    /// The five fields after the placement, or `None` when the side to move is unknown.
    pub fn fen_tail(&self) -> Option<String> {
        self.side_to_move.map(|side| self.tail_with(side))
    }

    fn tail_with(&self, side: Color) -> String {
        let ep = self.en_passant.map_or_else(|| "-".to_owned(), |sq| sq.to_string());
        format!("{} {} {} {} {}", side.fen_char(), self.castling.fen(), ep, self.halfmove_clock, self.fullmove_number)
    }

    /// The `cozy-chess` board for this position, with `side` to move when the position does
    /// not say. Fails when the position is not legal: check it with
    /// [`crate::rules::check_position`] first to learn why.
    pub fn to_board(&self, side: Color) -> Result<cozy_chess::Board, cozy_chess::FenParseError> {
        let side = self.side_to_move.unwrap_or(side);
        let fen = format!("{} {}", format_placement(&self.placement), self.tail_with(side));
        board_from_fen(&fen)
    }
}

/// A `cozy-chess` board from FEN text (Shredder castling not read), the one way a board is made
/// from text: `cozy-chess` takes two kings standing next to each other, and its move generation
/// then lets one king take the other and panics looking for it on the next move (plan A9-N4's
/// proptest found it in the mate solver; any FEN from a book, a PGN or a tool is untrusted input),
/// so that position is refused here, as no position.
pub fn board_from_fen(text: &str) -> Result<cozy_chess::Board, cozy_chess::FenParseError> {
    let board = cozy_chess::Board::from_fen(text, false)?;
    if kings_touch(&board) {
        return Err(cozy_chess::FenParseError::InvalidBoard);
    }
    Ok(board)
}

/// Whether the two kings stand next to each other (or one is missing, which `cozy-chess` refuses
/// before this is asked).
fn kings_touch(board: &cozy_chess::Board) -> bool {
    use cozy_chess::{Color as C, Piece};
    let king = |c: C| (board.colors(c) & board.pieces(Piece::King)).next_square();
    match (king(C::White), king(C::Black)) {
        (Some(white), Some(black)) => cozy_chess::get_king_moves(white).has(black),
        _ => true,
    }
}

/// A position read once from its FEN (plan A9-T1a, R3-03): parsed where it comes in, then
/// trusted. "The same position" is never decided by slicing FEN text, but by one of three named
/// equivalences, each a key:
///
/// - [`Fen::placement_key`]: the same pieces on the same squares.
/// - [`Fen::book_key`]: the same pieces and the same side to move, "same placement and turn",
///   the book's key (its diagrams, the moves of its text and the positions of its PGN). When the
///   side was assumed, only the placement counts ([`BookKey::matches`]).
/// - [`Fen::full_key`]: FIDE's same position (art. 9.2.3): the pieces, the side to move, the
///   castling rights and the en passant square. For repetition and the engine's cache.
///
/// The move counters never count in a key. Two FENs written differently for one position (`44`
/// for `8`, fields joined by `_` as in lichess URLs) are the same in all three. `==` is the same
/// FEN: every field, the counters too.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fen {
    position: Position,
}

impl Fen {
    /// Read a FEN ([`parse`]: only the placement is required).
    pub fn parse(text: &str) -> Result<Fen, FenError> {
        parse(text).map(Fen::from)
    }

    /// The position of a `cozy-chess` board, read from the board itself (no FEN text is written
    /// or read): what [`crate::game::Game::fen_at`] gives for every move, cheap enough for an
    /// index of a million moves. Its text is the board's own FEN.
    pub fn from_board(board: &cozy_chess::Board) -> Fen {
        use cozy_chess::Color as C;
        let side = color_from_cozy(board.side_to_move());
        let rights = |c: C| board.castle_rights(c);
        let castling = Castling {
            white_king_side: rights(C::White).short.is_some(),
            white_queen_side: rights(C::White).long.is_some(),
            black_king_side: rights(C::Black).short.is_some(),
            black_queen_side: rights(C::Black).long.is_some(),
        };
        // The square the pawn passed: the sixth rank when White is to move, the third otherwise.
        let ep_rank = if side == Color::White { 5 } else { 2 };
        let en_passant = board.en_passant().and_then(|file| Square::new(file as u8, ep_rank));
        Fen {
            position: Position {
                placement: placement_of_board(board),
                side_to_move: Some(side),
                castling,
                en_passant,
                halfmove_clock: u32::from(board.halfmove_clock()),
                fullmove_number: u32::from(board.fullmove_number()),
            },
        }
    }

    /// The position, field by field.
    pub fn position(&self) -> &Position {
        &self.position
    }

    /// The pieces.
    pub fn placement(&self) -> &Placement {
        &self.position.placement
    }

    /// The side to move, `None` when the FEN did not say (a book diagram's often does not).
    pub fn side(&self) -> Option<Color> {
        self.position.side_to_move
    }

    pub fn castling(&self) -> Castling {
        self.position.castling
    }

    pub fn en_passant(&self) -> Option<Square> {
        self.position.en_passant
    }

    pub fn halfmove_clock(&self) -> u32 {
        self.position.halfmove_clock
    }

    pub fn fullmove_number(&self) -> u32 {
        self.position.fullmove_number
    }

    /// The same pieces on the same squares.
    pub fn placement_key(&self) -> PlacementKey {
        PlacementKey::of(&self.position.placement)
    }

    /// "Same placement and turn". A side the FEN did not say is White, the convention FEN text
    /// writes it with ([`Position::fen`]); [`BookKey::matches`] leaves the side out when it was
    /// assumed.
    pub fn book_key(&self) -> BookKey {
        BookKey { placement: self.placement_key(), side: self.position.side_to_move.unwrap_or(Color::White) }
    }

    /// FIDE's same position: pieces, side to move (White when the FEN did not say), castling
    /// rights and the en passant square.
    pub fn full_key(&self) -> FullKey {
        let p = &self.position;
        FullKey { book: self.book_key(), castling: p.castling, en_passant: p.en_passant }
    }

    /// The `cozy-chess` board, with `side` to move when the FEN did not say
    /// ([`Position::to_board`]).
    pub fn to_board(&self, side: Color) -> Result<cozy_chess::Board, cozy_chess::FenParseError> {
        self.position.to_board(side)
    }
}

impl From<Position> for Fen {
    fn from(position: Position) -> Fen {
        Fen { position }
    }
}

impl std::str::FromStr for Fen {
    type Err = FenError;

    fn from_str(text: &str) -> Result<Fen, FenError> {
        Fen::parse(text)
    }
}

/// The full FEN ([`Position::fen`]: an unknown side to move is written `w`).
impl fmt::Display for Fen {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.position.fen())
    }
}

/// The pieces of a position, four bits a square: exact (two keys are equal exactly when the
/// pieces are, no hash to collide), 32 bytes against the ~70 of the placement's FEN text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PlacementKey([u64; 4]);

impl PlacementKey {
    pub fn of(placement: &Placement) -> PlacementKey {
        let mut words = [0u64; 4];
        for (sq, piece) in placement.pieces() {
            let i = sq.index();
            // A square's index is below 64, so its word is one of the four.
            if let Some(word) = words.get_mut(i / 16) {
                *word |= piece_code(piece) << ((i % 16) * 4);
            }
        }
        PlacementKey(words)
    }
}

/// A piece as four bits, never 0 (an empty square): 1–6 White's, 9–14 Black's.
fn piece_code(piece: Piece) -> u64 {
    let kind = match piece.kind {
        PieceKind::Pawn => 1,
        PieceKind::Knight => 2,
        PieceKind::Bishop => 3,
        PieceKind::Rook => 4,
        PieceKind::Queen => 5,
        PieceKind::King => 6,
    };
    if piece.color == Color::Black { kind | 8 } else { kind }
}

/// "Same placement and turn" (plan A9 glossary §2.1): the book's key. A diagram, a move of the
/// text and a position of the book's PGN show the same position when their keys
/// [match](BookKey::matches). Not FIDE's same position ([`FullKey`]): a diagram shows neither
/// castling rights nor an en passant square.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BookKey {
    pub placement: PlacementKey,
    pub side: Color,
}

impl BookKey {
    /// Whether a position keyed `self` is `other`: the same pieces, and the same side to move
    /// unless `self`'s side was assumed (a diagram that does not print it).
    pub fn matches(self, other: BookKey, side_assumed: bool) -> bool {
        self.placement == other.placement && (side_assumed || self.side == other.side)
    }
}

/// FIDE's same position (art. 9.2.3): the book's key, the castling rights and the en passant
/// square.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FullKey {
    pub book: BookKey,
    pub castling: Castling,
    pub en_passant: Option<Square>,
}

/// The pieces on a `cozy-chess` board.
pub(crate) fn placement_of_board(board: &cozy_chess::Board) -> Placement {
    let mut placement = Placement::empty();
    for sq in board.occupied() {
        if let (Some(kind), Some(color)) = (board.piece_on(sq), board.color_on(sq)) {
            placement.set(square_from_cozy(sq), Some(Piece { color: color_from_cozy(color), kind: kind_from_cozy(kind) }));
        }
    }
    placement
}

/// Parse a FEN. Only the placement field is required.
pub fn parse(fen: &str) -> Result<Position, FenError> {
    if fen.len() > MAX_FEN_LEN {
        return Err(FenError::TooLong);
    }
    let mut fields = fen.split(|c: char| c.is_whitespace() || c == '_').filter(|f| !f.is_empty());
    let placement = parse_placement_field(fields.next().ok_or(FenError::Empty)?)?;
    let mut pos = Position::from_placement(placement);
    if let Some(side) = fields.next() {
        pos.side_to_move = Some(match side {
            "w" | "W" => Color::White,
            "b" | "B" => Color::Black,
            _ => return Err(FenError::BadSide(side.to_owned())),
        });
    }
    if let Some(castling) = fields.next() {
        pos.castling = Castling::parse(castling)?;
    }
    if let Some(ep) = fields.next() {
        pos.en_passant = match ep {
            "-" => None,
            _ => Some(Square::parse(ep).filter(|sq| sq.rank() == 2 || sq.rank() == 5).ok_or_else(|| FenError::BadEnPassant(ep.to_owned()))?),
        };
    }
    if let Some(n) = fields.next() {
        pos.halfmove_clock = n.parse().map_err(|_| FenError::BadCounter(n.to_owned()))?;
    }
    if let Some(n) = fields.next() {
        pos.fullmove_number = n.parse().map_err(|_| FenError::BadCounter(n.to_owned()))?;
    }
    Ok(pos)
}

/// Parse just a placement, from a placement field or from a whole FEN.
pub fn parse_placement(fen: &str) -> Result<Placement, FenError> {
    parse(fen).map(|p| p.placement)
}

fn parse_placement_field(field: &str) -> Result<Placement, FenError> {
    let ranks: Vec<&str> = field.split('/').collect();
    if ranks.len() != 8 {
        return Err(FenError::RankCount(ranks.len()));
    }
    let mut placement = Placement::empty();
    // FEN writes the eighth rank first.
    for (rank, text) in (0u8..8).rev().zip(ranks) {
        let mut file: u8 = 0;
        for ch in text.chars() {
            if let Some(n) = ch.to_digit(10) {
                if !(1..=8).contains(&n) {
                    return Err(FenError::BadChar(ch));
                }
                // `n <= 8` and `file` is checked below before it can exceed 8, so no overflow.
                file = file.saturating_add(n as u8);
            } else {
                let piece = Piece::from_fen_char(ch).ok_or(FenError::BadChar(ch))?;
                let sq = Square::new(file, rank).ok_or(FenError::RankLength { rank: rank + 1 })?;
                placement.set(sq, Some(piece));
                file = file.saturating_add(1);
            }
            if file > 8 {
                return Err(FenError::RankLength { rank: rank + 1 });
            }
        }
        if file != 8 {
            return Err(FenError::RankLength { rank: rank + 1 });
        }
    }
    Ok(placement)
}

/// The placement field of a FEN.
pub fn format_placement(placement: &Placement) -> String {
    let mut s = String::with_capacity(71);
    for rank in (0u8..8).rev() {
        let mut empty = 0u8;
        for file in 0u8..8 {
            match Square::new(file, rank).and_then(|sq| placement.get(sq)) {
                Some(piece) => {
                    if empty > 0 {
                        s.push(char::from(b'0' + empty));
                        empty = 0;
                    }
                    s.push(piece.fen_char());
                }
                None => empty += 1,
            }
        }
        if empty > 0 {
            s.push(char::from(b'0' + empty));
        }
        if rank > 0 {
            s.push('/');
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::placement::PieceKind;
    use proptest::prelude::*;

    /// A board from FEN text refuses kings standing next to each other (`cozy-chess` accepts them,
    /// and its move generation then panics), and takes a legal position (plan A9-N4's proptest).
    #[test]
    fn a_board_with_kings_side_by_side_is_refused() {
        assert!(super::board_from_fen("8/8/8/8/8/8/8/Kk6 w - - 0 1").is_err());
        assert!(super::board_from_fen("4k3/3K4/8/8/8/8/8/4N3 b - - 0 1").is_err());
        assert!(super::board_from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1").is_ok());
        let position = super::parse("8/8/8/8/8/8/8/Kk6 w - - 0 1").expect("a placement");
        assert!(position.to_board(super::Color::White).is_err(), "to_board goes through it");
    }

    const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

    #[test]
    fn start_position_round_trips() {
        let pos = parse(START).unwrap();
        assert_eq!(pos.placement, Placement::standard());
        assert_eq!(pos.side_to_move, Some(Color::White));
        assert_eq!(pos.fen(), START);
    }

    #[test]
    fn placement_alone_is_enough() {
        let pos = parse("8/8/8/4k3/8/8/8/4K3").unwrap();
        assert_eq!(pos.side_to_move, None);
        assert_eq!(pos.fen_tail(), None);
        assert_eq!(pos.fen(), "8/8/8/4k3/8/8/8/4K3 w - - 0 1");
    }

    #[test]
    fn underscores_separate_fields_like_lichess_urls() {
        let pos = parse("8/8/8/4k3/8/8/8/4K3_b_-_-_0_1").unwrap();
        assert_eq!(pos.side_to_move, Some(Color::Black));
    }

    #[test]
    fn bad_placements_are_rejected_with_the_reason() {
        assert_eq!(parse(""), Err(FenError::Empty));
        assert_eq!(parse("   "), Err(FenError::Empty));
        assert_eq!(parse("8/8/8/8/8/8/8"), Err(FenError::RankCount(7)));
        assert_eq!(parse("8/8/8/8/8/8/8/7"), Err(FenError::RankLength { rank: 1 }));
        assert_eq!(parse("9/8/8/8/8/8/8/8"), Err(FenError::BadChar('9')));
        assert_eq!(parse("0/8/8/8/8/8/8/8"), Err(FenError::BadChar('0')));
        assert_eq!(parse("8/8/8/8/8/8/8/K8"), Err(FenError::RankLength { rank: 1 }));
        assert_eq!(parse("8/8/8/8/8/8/8/KKKKKKKKK"), Err(FenError::RankLength { rank: 1 }));
        assert_eq!(parse("8/8/8/8/8/8/8/x7"), Err(FenError::BadChar('x')));
        assert_eq!(parse("8/8/8/8/8/8/8/♔7"), Err(FenError::BadChar('♔')));
        assert_eq!(parse(&"8/".repeat(400)), Err(FenError::TooLong));
    }

    #[test]
    fn bad_tails_are_rejected_with_the_reason() {
        let p = "8/8/8/4k3/8/8/8/4K3";
        assert_eq!(parse(&format!("{p} x")), Err(FenError::BadSide("x".into())));
        assert_eq!(parse(&format!("{p} w KK")), Err(FenError::BadCastling("KK".into())));
        assert_eq!(parse(&format!("{p} w X")), Err(FenError::BadCastling("X".into())));
        assert_eq!(parse(&format!("{p} w - e4")), Err(FenError::BadEnPassant("e4".into())));
        assert_eq!(parse(&format!("{p} w - - x")), Err(FenError::BadCounter("x".into())));
        assert_eq!(parse(&format!("{p} w - - 0 -1")), Err(FenError::BadCounter("-1".into())));
        assert!(parse(&format!("{p} w - e3 0 1")).is_ok());
    }

    #[test]
    fn adjacent_digits_are_summed() {
        assert_eq!(parse_placement("44/8/8/8/8/8/8/8").unwrap(), Placement::empty());
    }

    #[test]
    fn illegal_positions_still_parse() {
        // Two white kings and a pawn on the eighth rank: a misread diagram must survive parsing.
        let p = parse_placement("P7/8/8/8/8/8/8/K6K").unwrap();
        assert_eq!(p.count(Piece::new(Color::White, PieceKind::King)), 2);
        assert_eq!(format_placement(&p), "P7/8/8/8/8/8/8/K6K");
    }

    #[test]
    fn to_board_uses_the_given_side_when_unknown() {
        let pos = parse("8/8/8/4k3/8/8/8/4K3").unwrap();
        let board = pos.to_board(Color::Black).unwrap();
        assert_eq!(board.side_to_move(), cozy_chess::Color::Black);
        assert!(parse_placement("8/8/8/8/8/8/8/K6K").map(Position::from_placement).unwrap().to_board(Color::White).is_err());
    }

    fn arb_cell() -> impl Strategy<Value = Option<Piece>> {
        prop_oneof![
            3 => Just(None),
            1 => (any::<bool>(), 0usize..6).prop_map(|(white, k)| {
                let color = if white { Color::White } else { Color::Black };
                Some(Piece::new(color, PieceKind::ALL[k]))
            }),
        ]
    }

    /// Plan A9-T1a: one position written three ways is one position for all three keys.
    #[test]
    fn a_position_written_differently_is_the_same_position() {
        let canonical = Fen::parse("r3k2r/8/8/8/4P3/8/8/R3K2R b KQkq e3 0 20").unwrap();
        for text in [
            "r3k2r/8/8/8/4P3/8/8/R3K2R_b_KQkq_e3_0_20",
            "r21k11r/8/44/8/4P3/8/8/R3K2R b KQkq e3 3 1",
            "  r3k2r/8/8/8/4P3/8/8/R3K2R   b KQkq e3 0 20 ",
        ] {
            let other = Fen::parse(text).unwrap();
            assert_eq!(other.placement_key(), canonical.placement_key(), "{text}");
            assert_eq!(other.book_key(), canonical.book_key(), "{text}");
            assert_eq!(other.full_key(), canonical.full_key(), "{text}: the move counters never count");
        }
        // A FEN that does not say the side is written with White's, and keyed so.
        let unsaid = Fen::parse("r3k2r/8/8/8/4P3/8/8/R3K2R").unwrap();
        assert_eq!(unsaid.side(), None);
        assert_eq!(unsaid.book_key(), Fen::parse("r3k2r/8/8/8/4P3/8/8/R3K2R w").unwrap().book_key());
        assert!(unsaid.book_key().matches(canonical.book_key(), true), "assumed: only the pieces count");
        assert!(!unsaid.book_key().matches(canonical.book_key(), false));
        // Castling and en passant are FIDE's, not the book's.
        let no_rights = Fen::parse("r3k2r/8/8/8/4P3/8/8/R3K2R b - - 0 20").unwrap();
        assert_eq!(no_rights.book_key(), canonical.book_key());
        assert_ne!(no_rights.full_key(), canonical.full_key());
    }

    #[test]
    fn a_fen_reads_and_writes_like_its_position() {
        let fen: Fen = START.parse().unwrap();
        assert_eq!(fen.to_string(), START);
        assert_eq!(fen.placement(), &Placement::standard());
        assert_eq!(
            (fen.side(), fen.castling().fen(), fen.en_passant(), fen.halfmove_clock(), fen.fullmove_number()),
            (Some(Color::White), "KQkq".to_owned(), None, 0, 1)
        );
        assert_eq!(Fen::parse("8/8/8/8"), Err(FenError::RankCount(4)));
        assert_eq!(fen.to_board(Color::Black).unwrap(), cozy_chess::Board::default());
    }

    /// A position that can differ from another in each field the keys look at.
    fn arb_position() -> impl Strategy<Value = Position> {
        let cells = proptest::collection::vec(
            prop_oneof![6 => Just(None), 1 => (any::<bool>(), 0usize..6).prop_map(|(w, k)| Some(Piece::new(if w { Color::White } else { Color::Black }, PieceKind::ALL[k])))],
            64,
        );
        let side = prop_oneof![Just(None), Just(Some(Color::White)), Just(Some(Color::Black))];
        let castling = (any::<bool>(), any::<bool>(), any::<bool>(), any::<bool>()).prop_map(|(a, b, c, d)| Castling {
            white_king_side: a,
            white_queen_side: b,
            black_king_side: c,
            black_queen_side: d,
        });
        let ep = prop_oneof![Just(None), (0u8..8, prop_oneof![Just(2u8), Just(5u8)]).prop_map(|(f, r)| Square::new(f, r))];
        (cells, side, castling, ep, 0u32..100, 1u32..300).prop_map(|(cells, side_to_move, castling, en_passant, halfmove_clock, fullmove_number)| {
            let mut arr = [None; 64];
            arr.copy_from_slice(&cells);
            Position { placement: Placement::from_reading_order(&arr), side_to_move, castling, en_passant, halfmove_clock, fullmove_number }
        })
    }

    /// `a` with each field taken from `b` or kept, as `pick` says: pairs that differ in one field
    /// or none as often as in many.
    fn mix(a: &Position, b: &Position, pick: [bool; 6]) -> Position {
        Position {
            placement: if pick[0] { b.placement } else { a.placement },
            side_to_move: if pick[1] { b.side_to_move } else { a.side_to_move },
            castling: if pick[2] { b.castling } else { a.castling },
            en_passant: if pick[3] { b.en_passant } else { a.en_passant },
            halfmove_clock: if pick[4] { b.halfmove_clock } else { a.halfmove_clock },
            fullmove_number: if pick[5] { b.fullmove_number } else { a.fullmove_number },
        }
    }

    /// A random game's boards, each move picked among the legal ones by `picks`.
    fn boards(picks: &[usize]) -> Vec<cozy_chess::Board> {
        let mut board = cozy_chess::Board::default();
        let mut out = vec![board.clone()];
        for &pick in picks {
            let mut moves = Vec::new();
            board.generate_moves(|m| {
                moves.extend(m);
                false
            });
            let Some(&mv) = moves.get(pick % moves.len().max(1)) else { break };
            board.play_unchecked(mv);
            out.push(board.clone());
        }
        out
    }

    proptest! {
        /// Plan A9-T1a: the three equivalences, each exactly the fields it names.
        #[test]
        fn the_keys_are_the_equivalences_they_name(a in arb_position(), b in arb_position(), pick in any::<[bool; 6]>()) {
            let b = mix(&a, &b, pick);
            let (fa, fb) = (Fen::from(a.clone()), Fen::from(b.clone()));
            let white = |p: &Position| p.side_to_move.unwrap_or(Color::White);
            let pieces = a.placement == b.placement;
            let turn = white(&a) == white(&b);
            prop_assert_eq!(fa.placement_key() == fb.placement_key(), pieces);
            prop_assert_eq!(fa.book_key() == fb.book_key(), pieces && turn);
            prop_assert_eq!(fa.book_key().matches(fb.book_key(), false), pieces && turn);
            prop_assert_eq!(fa.book_key().matches(fb.book_key(), true), pieces);
            prop_assert_eq!(fa.full_key() == fb.full_key(), pieces && turn && a.castling == b.castling && a.en_passant == b.en_passant);
            // The text is the position: read back, it keys alike.
            let back = Fen::parse(&fa.to_string()).unwrap();
            prop_assert_eq!(back.full_key(), fa.full_key());
        }

        /// Plan A9-T1a: a board's position, read from the board, is the one its FEN text says.
        #[test]
        fn a_boards_position_is_its_fen(picks in proptest::collection::vec(any::<usize>(), 0..60)) {
            for board in boards(&picks) {
                let fen = Fen::from_board(&board);
                prop_assert_eq!(fen.to_string(), board.to_string());
                prop_assert_eq!(Fen::parse(&board.to_string()).unwrap(), fen);
            }
        }

        #[test]
        fn any_placement_round_trips(cells in proptest::collection::vec(arb_cell(), 64)) {
            let mut arr = [None; 64];
            arr.copy_from_slice(&cells);
            let placement = Placement::from_reading_order(&arr);
            let text = format_placement(&placement);
            prop_assert_eq!(parse_placement(&text).unwrap(), placement);
            prop_assert_eq!(placement.reading_order(), arr);
        }

        #[test]
        fn parse_never_panics(s in "\\PC{0,120}") {
            let _ = parse(&s);
        }
    }
}
