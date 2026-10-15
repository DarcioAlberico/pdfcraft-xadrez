//! FEN parsing and formatting, tolerant enough for diagrams that carry only the piece placement.
//!
//! A book diagram says where the pieces stand and nothing else, so only the placement field is
//! required. The other five fields are read when present and otherwise left unknown. Fields may
//! be separated by spaces or by `_`, the form lichess URLs use.

use std::fmt;

use crate::placement::{Color, Piece, Placement, Square};

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
        cozy_chess::Board::from_fen(&fen, false)
    }
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

    proptest! {
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
