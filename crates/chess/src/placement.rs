//! Piece placement that may be illegal, as read from a book diagram.
//!
//! A misread square can put two white kings or a pawn on the eighth rank on the board, and the
//! recognizer has to hold that position to repair it. `cozy_chess::Board` refuses such positions,
//! so this crate keeps its own placement and converts to `cozy-chess` only once a position is
//! legal ([`crate::fen::Position::to_board`]).

use std::fmt;

/// A side.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Color {
    White,
    Black,
}

impl Color {
    /// The other side.
    pub fn opponent(self) -> Color {
        match self {
            Color::White => Color::Black,
            Color::Black => Color::White,
        }
    }

    /// `w` or `b`, as in FEN.
    pub fn fen_char(self) -> char {
        match self {
            Color::White => 'w',
            Color::Black => 'b',
        }
    }
}

/// A kind of piece, without its colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PieceKind {
    Pawn,
    Knight,
    Bishop,
    Rook,
    Queen,
    King,
}

impl PieceKind {
    /// Every kind, pawn first.
    pub const ALL: [PieceKind; 6] = [PieceKind::Pawn, PieceKind::Knight, PieceKind::Bishop, PieceKind::Rook, PieceKind::Queen, PieceKind::King];

    /// The English FEN letter, upper case.
    pub fn letter(self) -> char {
        match self {
            PieceKind::Pawn => 'P',
            PieceKind::Knight => 'N',
            PieceKind::Bishop => 'B',
            PieceKind::Rook => 'R',
            PieceKind::Queen => 'Q',
            PieceKind::King => 'K',
        }
    }

    /// Whether the piece attacks along lines that another piece can block.
    pub fn is_slider(self) -> bool {
        matches!(self, PieceKind::Bishop | PieceKind::Rook | PieceKind::Queen)
    }
}

/// A coloured piece.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Piece {
    pub color: Color,
    pub kind: PieceKind,
}

impl Piece {
    pub const fn new(color: Color, kind: PieceKind) -> Piece {
        Piece { color, kind }
    }

    /// The FEN letter: upper case for White, lower case for Black.
    pub fn fen_char(self) -> char {
        let c = self.kind.letter();
        match self.color {
            Color::White => c,
            Color::Black => c.to_ascii_lowercase(),
        }
    }

    /// The piece for a FEN letter, or `None` for any other character.
    pub fn from_fen_char(c: char) -> Option<Piece> {
        let kind = match c.to_ascii_uppercase() {
            'P' => PieceKind::Pawn,
            'N' => PieceKind::Knight,
            'B' => PieceKind::Bishop,
            'R' => PieceKind::Rook,
            'Q' => PieceKind::Queen,
            'K' => PieceKind::King,
            _ => return None,
        };
        let color = if c.is_ascii_uppercase() { Color::White } else { Color::Black };
        Some(Piece { color, kind })
    }
}

/// A square, a1 = 0 … h8 = 63 (`index = rank * 8 + file`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Square(u8);

impl Square {
    /// The corners a diagram's top left is: a8 from White's side, h1 from Black's.
    pub const A8: Square = Square(56);
    pub const H1: Square = Square(7);

    /// The square at `file` (0 = a) and `rank` (0 = first rank), or `None` off the board.
    pub fn new(file: u8, rank: u8) -> Option<Square> {
        (file < 8 && rank < 8).then_some(Square(rank * 8 + file))
    }

    /// The square at `file` and `rank` taken modulo 8. For coordinates that are on the board by
    /// construction (another chess library's squares), where `new` could only ever succeed.
    pub(crate) fn wrapping(file: u8, rank: u8) -> Square {
        Square((rank % 8) * 8 + file % 8)
    }

    /// The square with this index (a1 = 0), or `None` above 63.
    pub fn from_index(index: usize) -> Option<Square> {
        u8::try_from(index).ok().filter(|&i| i < 64).map(Square)
    }

    /// The square at position `i` of the reading order a diagram is scanned in: a8, b8 … h8,
    /// a7 … h1. This is the order of the 64 rows of a recognizer's probability matrix.
    pub fn from_reading_index(i: usize) -> Option<Square> {
        if i >= 64 {
            return None;
        }
        // `i < 64`, so both casts fit and `7 - i / 8` cannot underflow.
        Square::new((i % 8) as u8, 7 - (i / 8) as u8)
    }

    /// This square's position in reading order (a8 = 0 … h1 = 63).
    pub fn reading_index(self) -> usize {
        usize::from(7 - self.rank()) * 8 + usize::from(self.file())
    }

    pub fn index(self) -> usize {
        usize::from(self.0)
    }

    /// 0 = a … 7 = h.
    pub fn file(self) -> u8 {
        self.0 % 8
    }

    /// 0 = first rank … 7 = eighth rank.
    pub fn rank(self) -> u8 {
        self.0 / 8
    }

    /// Whether the square is light. a1 is dark.
    pub fn is_light(self) -> bool {
        (self.file() + self.rank()) % 2 == 1
    }

    /// The square `df` files and `dr` ranks away, or `None` off the board.
    pub fn offset(self, df: i8, dr: i8) -> Option<Square> {
        let f = i16::from(self.file()) + i16::from(df);
        let r = i16::from(self.rank()) + i16::from(dr);
        let f = u8::try_from(f).ok()?;
        let r = u8::try_from(r).ok()?;
        Square::new(f, r)
    }

    /// The square seen from the other side of the board (a1 ↔ h8).
    pub fn rotated(self) -> Square {
        Square(63 - self.0)
    }

    /// Parse a square name such as `e4`.
    pub fn parse(name: &str) -> Option<Square> {
        let mut chars = name.chars();
        let file = chars.next()?;
        let rank = chars.next()?;
        if chars.next().is_some() || !('a'..='h').contains(&file) || !('1'..='8').contains(&rank) {
            return None;
        }
        Square::new(file as u8 - b'a', rank as u8 - b'1')
    }
}

impl fmt::Display for Square {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", char::from(b'a' + self.file()), char::from(b'1' + self.rank()))
    }
}

/// What stands on each of the 64 squares. May be illegal.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Placement {
    squares: [Option<Piece>; 64],
}

impl Default for Placement {
    fn default() -> Self {
        Placement { squares: [None; 64] }
    }
}

impl Placement {
    /// An empty board.
    pub fn empty() -> Placement {
        Placement::default()
    }

    /// The initial position.
    pub fn standard() -> Placement {
        let mut p = Placement::empty();
        let back = [
            PieceKind::Rook,
            PieceKind::Knight,
            PieceKind::Bishop,
            PieceKind::Queen,
            PieceKind::King,
            PieceKind::Bishop,
            PieceKind::Knight,
            PieceKind::Rook,
        ];
        for (file, kind) in (0u8..).zip(back) {
            p.put(file, 0, Piece::new(Color::White, kind));
            p.put(file, 1, Piece::new(Color::White, PieceKind::Pawn));
            p.put(file, 6, Piece::new(Color::Black, PieceKind::Pawn));
            p.put(file, 7, Piece::new(Color::Black, kind));
        }
        p
    }

    fn put(&mut self, file: u8, rank: u8, piece: Piece) {
        if let Some(sq) = Square::new(file, rank) {
            self.set(sq, Some(piece));
        }
    }

    /// The pieces in reading order (a8 … h1): the inverse of [`Placement::reading_order`].
    pub fn from_reading_order(cells: &[Option<Piece>; 64]) -> Placement {
        let mut p = Placement::empty();
        for (i, cell) in cells.iter().enumerate() {
            if let Some(sq) = Square::from_reading_index(i) {
                p.set(sq, *cell);
            }
        }
        p
    }

    /// The 64 squares in reading order (a8 … h1).
    pub fn reading_order(&self) -> [Option<Piece>; 64] {
        let mut out = [None; 64];
        for (sq, piece) in self.pieces() {
            if let Some(cell) = out.get_mut(sq.reading_index()) {
                *cell = Some(piece);
            }
        }
        out
    }

    pub fn get(&self, sq: Square) -> Option<Piece> {
        // A `Square` is always below 64, so this never misses; `get` keeps it total anyway.
        self.squares.get(sq.index()).copied().flatten()
    }

    pub fn set(&mut self, sq: Square, piece: Option<Piece>) {
        if let Some(cell) = self.squares.get_mut(sq.index()) {
            *cell = piece;
        }
    }

    /// Every occupied square with its piece, a1 first.
    pub fn pieces(&self) -> impl Iterator<Item = (Square, Piece)> + '_ {
        self.squares.iter().enumerate().filter_map(|(i, cell)| Some((Square::from_index(i)?, (*cell)?)))
    }

    /// How many of `piece` stand on the board.
    pub fn count(&self, piece: Piece) -> usize {
        self.squares.iter().filter(|c| **c == Some(piece)).count()
    }

    /// How many pieces of `color` stand on the board.
    pub fn count_color(&self, color: Color) -> usize {
        self.squares.iter().flatten().filter(|p| p.color == color).count()
    }

    pub fn is_empty(&self) -> bool {
        self.squares.iter().all(Option::is_none)
    }

    /// The square of `color`'s king, if there is exactly one.
    pub fn king(&self, color: Color) -> Option<Square> {
        let mut kings = self.pieces().filter(|(_, p)| *p == Piece::new(color, PieceKind::King));
        let first = kings.next()?;
        kings.next().is_none().then_some(first.0)
    }

    /// The board turned half a circle: what a diagram printed upside down shows.
    pub fn rotated(&self) -> Placement {
        let mut p = Placement::empty();
        for (sq, piece) in self.pieces() {
            p.set(sq.rotated(), Some(piece));
        }
        p
    }
}

impl fmt::Debug for Placement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Placement({})", crate::fen::format_placement(self))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reading_order_starts_at_a8_and_ends_at_h1() {
        assert_eq!(Square::from_reading_index(0), Square::parse("a8"));
        assert_eq!(Square::from_reading_index(7), Square::parse("h8"));
        assert_eq!(Square::from_reading_index(8), Square::parse("a7"));
        assert_eq!(Square::from_reading_index(63), Square::parse("h1"));
        assert_eq!(Square::from_reading_index(64), None);
        for i in 0..64 {
            let sq = Square::from_reading_index(i).unwrap();
            assert_eq!(sq.reading_index(), i);
        }
    }

    #[test]
    fn square_names_round_trip() {
        for i in 0..64 {
            let sq = Square::from_index(i).unwrap();
            assert_eq!(Square::parse(&sq.to_string()), Some(sq));
        }
        for bad in ["", "e", "e9", "i1", "e44", "E4", "é4"] {
            assert_eq!(Square::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn a1_is_dark_and_h1_light() {
        assert!(!Square::parse("a1").unwrap().is_light());
        assert!(Square::parse("h1").unwrap().is_light());
        assert!(Square::parse("d1").unwrap().is_light());
    }

    #[test]
    fn offsets_stop_at_the_edge() {
        let a1 = Square::parse("a1").unwrap();
        assert_eq!(a1.offset(-1, 0), None);
        assert_eq!(a1.offset(0, -1), None);
        assert_eq!(a1.offset(7, 7), Square::parse("h8"));
        assert_eq!(a1.offset(8, 0), None);
        assert_eq!(a1.offset(i8::MIN, i8::MAX), None);
    }

    #[test]
    fn rotation_is_an_involution() {
        let p = Placement::standard();
        assert_ne!(p.rotated(), p);
        assert_eq!(p.rotated().rotated(), p);
        // Half a turn takes d1 to e8 and e1 to d8.
        assert_eq!(p.rotated().get(Square::parse("e8").unwrap()), Some(Piece::new(Color::White, PieceKind::Queen)));
        assert_eq!(p.rotated().get(Square::parse("d8").unwrap()), Some(Piece::new(Color::White, PieceKind::King)));
    }

    #[test]
    fn king_needs_exactly_one() {
        let mut p = Placement::standard();
        assert_eq!(p.king(Color::White), Square::parse("e1"));
        p.set(Square::parse("e4").unwrap(), Some(Piece::new(Color::White, PieceKind::King)));
        assert_eq!(p.king(Color::White), None);
        assert_eq!(Placement::empty().king(Color::Black), None);
    }
}
