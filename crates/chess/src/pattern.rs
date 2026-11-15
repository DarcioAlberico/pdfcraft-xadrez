//! Positions by material and pawn structure (plan A9-N16a, finding A8-09): only what is exact.
//!
//! - **Material** ([`Material`]): how many of each piece each side has, written as a signature,
//!   `KRP-KR`: White's pieces, then Black's, each in the order K Q R B N P, a letter a piece.
//!   [`Material::matches`] also takes the colours swapped (`KRP-KR` is `KR-KRP` with Black up a
//!   pawn) when asked.
//! - **Pawns** ([`PawnTraits`]): for a side, the files with doubled pawns, the isolated pawns (no
//!   pawn of its own on a file beside) and the passed ones (no pawn of the other side ahead of it on
//!   its file or a file beside).
//!
//! Everything reads a [`Placement`], which holds a diagram's illegal reading as well as a game's
//! position (the `cozy-chess` board refuses the first). Opposite castling and the game's phase are
//! left out until they have an exact definition (decision D32).

use std::fmt;

use crate::placement::{Color, Piece, PieceKind, Placement, Square};

/// Most pieces a side's half of a signature takes: as many as a side starts with.
pub const MAX_PIECES_A_SIDE: usize = 16;

/// Longest signature [`Material::parse`] reads, in characters (two full sides, a dash, spaces).
const MAX_SIGNATURE_CHARS: usize = 48;

/// The order a signature lists a side's pieces in.
const ORDER: [PieceKind; 6] = [PieceKind::King, PieceKind::Queen, PieceKind::Rook, PieceKind::Bishop, PieceKind::Knight, PieceKind::Pawn];

fn slot(kind: PieceKind) -> usize {
    match kind {
        PieceKind::King => 0,
        PieceKind::Queen => 1,
        PieceKind::Rook => 2,
        PieceKind::Bishop => 3,
        PieceKind::Knight => 4,
        PieceKind::Pawn => 5,
    }
}

fn side(color: Color) -> usize {
    match color {
        Color::White => 0,
        Color::Black => 1,
    }
}

/// A material signature: how many of each piece each side has ([`Material::of`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Material {
    /// White's then Black's counts, in [`ORDER`].
    counts: [[u8; 6]; 2],
}

/// Why a text is not a material signature.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaterialError {
    /// No dash between the sides.
    NoDash,
    /// Longer than a signature can be.
    TooLong,
    /// A character that is not one of K, Q, R, B, N and P.
    NotAPiece(char),
    /// A piece out of the order K Q R B N P.
    OutOfOrder(char),
    /// More than [`MAX_PIECES_A_SIDE`] pieces on a side.
    TooMany,
    /// A side with no piece.
    EmptySide,
    /// A side without its king (plan A9-N16a2): a position always has both, so it would match
    /// none.
    NoKing,
}

impl fmt::Display for MaterialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MaterialError::NoDash => write!(f, "a signature is White's pieces, a dash and Black's, as KRP-KR"),
            MaterialError::TooLong => write!(f, "a signature is at most {MAX_SIGNATURE_CHARS} characters"),
            MaterialError::NotAPiece(c) => write!(f, "'{c}' is not a piece: a signature has K, Q, R, B, N and P"),
            MaterialError::OutOfOrder(c) => write!(f, "a side's pieces go in the order K Q R B N P: '{c}' comes too late"),
            MaterialError::TooMany => write!(f, "a side has at most {MAX_PIECES_A_SIDE} pieces"),
            MaterialError::EmptySide => write!(f, "a side has no piece: each has its king at least, as K-K"),
            MaterialError::NoKing => write!(f, "each side has its king first, as KR-K: a position always has both"),
        }
    }
}

impl std::error::Error for MaterialError {}

impl Material {
    /// The pieces of `placement`, by side (more than 255 of a kind count as 255).
    pub fn of(placement: &Placement) -> Material {
        let mut counts = [[0u8; 6]; 2];
        for (_, piece) in placement.pieces() {
            if let Some(n) = counts.get_mut(side(piece.color)).and_then(|s| s.get_mut(slot(piece.kind))) {
                *n = n.saturating_add(1);
            }
        }
        Material { counts }
    }

    /// The signature `text` (`KRP-KR`; letters in either case, spaces left out, an en dash for the
    /// dash too), or why not.
    pub fn parse(text: &str) -> Result<Material, MaterialError> {
        if text.chars().count() > MAX_SIGNATURE_CHARS {
            return Err(MaterialError::TooLong);
        }
        let text: String = text.chars().filter(|c| !c.is_whitespace()).map(|c| if c == '\u{2013}' { '-' } else { c }).collect();
        let (white, black) = text.split_once('-').ok_or(MaterialError::NoDash)?;
        let mut counts = [[0u8; 6]; 2];
        for (half, letters) in [white, black].into_iter().enumerate() {
            if letters.is_empty() {
                return Err(MaterialError::EmptySide);
            }
            let mut last = 0usize;
            let mut total = 0usize;
            for c in letters.chars() {
                let kind = Piece::from_fen_char(c.to_ascii_uppercase()).map(|p| p.kind).ok_or(MaterialError::NotAPiece(c))?;
                let at = slot(kind);
                if at < last {
                    return Err(MaterialError::OutOfOrder(c));
                }
                last = at;
                total = total.saturating_add(1);
                if total > MAX_PIECES_A_SIDE {
                    return Err(MaterialError::TooMany);
                }
                if let Some(n) = counts.get_mut(half).and_then(|s| s.get_mut(at)) {
                    *n = n.saturating_add(1);
                }
            }
            if counts.get(half).and_then(|s| s.first()).is_none_or(|k| *k == 0) {
                return Err(MaterialError::NoKing);
            }
        }
        Ok(Material { counts })
    }

    /// How many `kind` `color` has.
    pub fn count(&self, color: Color, kind: PieceKind) -> u8 {
        self.counts.get(side(color)).and_then(|s| s.get(slot(kind))).copied().unwrap_or(0)
    }

    /// The same material with the colours swapped.
    pub fn mirrored(&self) -> Material {
        let [white, black] = self.counts;
        Material { counts: [black, white] }
    }

    /// Whether `self` is `other`, or, with `either_side`, `other` with the colours swapped.
    pub fn matches(&self, other: &Material, either_side: bool) -> bool {
        self == other || (either_side && self.mirrored() == *other)
    }
}

impl fmt::Display for Material {
    /// `KRP-KR`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (half, counts) in self.counts.iter().enumerate() {
            if half == 1 {
                f.write_str("-")?;
            }
            for (kind, n) in ORDER.iter().zip(counts) {
                for _ in 0..*n {
                    write!(f, "{}", kind.letter())?;
                }
            }
        }
        Ok(())
    }
}

/// Bits of files (bit 0 is the a-file) and of squares (bit 0 is a1).
fn file_bit(file: u8) -> u8 {
    1u8.checked_shl(u32::from(file)).unwrap_or(0)
}

fn square_bit(sq: Square) -> u64 {
    1u64.checked_shl(u32::try_from(sq.index()).unwrap_or(u32::MAX)).unwrap_or(0)
}

/// A side's pawn structure ([`PawnTraits::of`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PawnTraits {
    /// The files holding two pawns of the side or more (bit 0: the a-file).
    doubled: u8,
    /// Its pawns with no pawn of its own on a file beside (bit 0: a1).
    isolated: u64,
    /// Its pawns with no pawn of the other side ahead of them on their file or a file beside.
    passed: u64,
}

impl PawnTraits {
    /// `color`'s pawns in `placement`, by the files and ranks they stand on (a pawn on its first
    /// or last rank, which a misread diagram may hold, counts as any other).
    pub fn of(placement: &Placement, color: Color) -> PawnTraits {
        let pawns = |c: Color| placement.pieces().filter(move |(_, p)| *p == Piece::new(c, PieceKind::Pawn)).map(|(sq, _)| sq);
        let mut per_file = [0u8; 8];
        for sq in pawns(color) {
            if let Some(n) = per_file.get_mut(usize::from(sq.file())) {
                *n = n.saturating_add(1);
            }
        }
        let has = |file: i16| usize::try_from(file).ok().and_then(|f| per_file.get(f)).is_some_and(|n| *n > 0);
        // The other side's pawns, file by file, a bit for each rank (bit 0: the first).
        let mut theirs = [0u8; 8];
        for sq in pawns(color.opponent()) {
            if let Some(ranks) = theirs.get_mut(usize::from(sq.file())) {
                *ranks |= 1u8.checked_shl(u32::from(sq.rank())).unwrap_or(0);
            }
        }
        // Whether a pawn of theirs stands on `file` ahead of a pawn of ours on `rank`.
        let ahead = |file: i16, rank: u8| {
            let Some(ranks) = usize::try_from(file).ok().and_then(|f| theirs.get(f)) else { return false };
            let beyond = match color {
                Color::White => ranks.checked_shr(u32::from(rank) + 1).unwrap_or(0),
                Color::Black => ranks & 1u8.checked_shl(u32::from(rank)).map_or(u8::MAX, |b| b.wrapping_sub(1)),
            };
            beyond != 0
        };
        let mut out = PawnTraits::default();
        for (file, n) in (0u8..).zip(per_file) {
            if n >= 2 {
                out.doubled |= file_bit(file);
            }
        }
        for sq in pawns(color) {
            let file = i16::from(sq.file());
            if !has(file - 1) && !has(file + 1) {
                out.isolated |= square_bit(sq);
            }
            if !(ahead(file - 1, sq.rank()) || ahead(file, sq.rank()) || ahead(file + 1, sq.rank())) {
                out.passed |= square_bit(sq);
            }
        }
        out
    }

    /// The files with doubled pawns, `a` to `h` (0 to 7).
    pub fn doubled_files(&self) -> impl Iterator<Item = u8> + '_ {
        (0u8..8).filter(|f| self.doubled & file_bit(*f) != 0)
    }

    /// The isolated pawns, a1 first.
    pub fn isolated(&self) -> impl Iterator<Item = Square> + '_ {
        squares_of(self.isolated)
    }

    /// The passed pawns, a1 first.
    pub fn passed(&self) -> impl Iterator<Item = Square> + '_ {
        squares_of(self.passed)
    }

    pub fn has_doubled(&self) -> bool {
        self.doubled != 0
    }

    pub fn has_isolated(&self) -> bool {
        self.isolated != 0
    }

    pub fn has_passed(&self) -> bool {
        self.passed != 0
    }
}

fn squares_of(bits: u64) -> impl Iterator<Item = Square> {
    (0usize..64).filter(move |i| bits & 1u64.checked_shl(u32::try_from(*i).unwrap_or(64)).unwrap_or(0) != 0).filter_map(Square::from_index)
}

/// The pawn structure a [`Query`] asks for: each trait asked for, held by one side.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PawnQuery {
    pub doubled: bool,
    pub isolated: bool,
    pub passed: bool,
    /// The side that holds them; `None`: either, one side holding all of them.
    pub side: Option<Color>,
}

impl PawnQuery {
    /// Whether it asks for nothing.
    pub fn is_empty(&self) -> bool {
        !(self.doubled || self.isolated || self.passed)
    }

    fn held_by(&self, traits: &PawnTraits) -> bool {
        (!self.doubled || traits.has_doubled()) && (!self.isolated || traits.has_isolated()) && (!self.passed || traits.has_passed())
    }

    /// Whether `placement` has the traits asked for (true when none is).
    pub fn matches(&self, placement: &Placement) -> bool {
        if self.is_empty() {
            return true;
        }
        let sides: &[Color] = match self.side {
            Some(Color::White) => &[Color::White],
            Some(Color::Black) => &[Color::Black],
            None => &[Color::White, Color::Black],
        };
        sides.iter().any(|c| self.held_by(&PawnTraits::of(placement, *c)))
    }
}

/// What a position is looked for by (plan A9-N16a).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Query {
    pub material: Option<Material>,
    /// The material with the colours swapped matches too.
    pub either_side: bool,
    pub pawns: PawnQuery,
}

impl Query {
    /// Whether it asks for anything.
    pub fn is_empty(&self) -> bool {
        self.material.is_none() && self.pawns.is_empty()
    }

    /// Whether `placement` is a position it looks for (every position, when it asks for nothing).
    ///
    /// With [`Query::either_side`], a position whose material is the signature with the colours
    /// swapped is held to the pawns with their side swapped too (plan A9-N16a2): the side is the
    /// one holding the material, as [`Query::mirrored`] has it.
    pub fn matches(&self, placement: &Placement) -> bool {
        let Some(wanted) = self.material else { return self.pawns.matches(placement) };
        let found = Material::of(placement);
        let swapped = PawnQuery { side: self.pawns.side.map(Color::opponent), ..self.pawns };
        (found == wanted && self.pawns.matches(placement)) || (self.either_side && found == wanted.mirrored() && swapped.matches(placement))
    }

    /// The same query for the board turned to the other side ([`mirrored`]): the material's and the
    /// pawns' colours swapped.
    pub fn mirrored(&self) -> Query {
        Query {
            material: self.material.map(|m| m.mirrored()),
            either_side: self.either_side,
            pawns: PawnQuery { side: self.pawns.side.map(Color::opponent), ..self.pawns },
        }
    }
}

/// `placement` with the colours swapped and the ranks turned over (a1 ↔ a8): the same position for
/// the other side.
pub fn mirrored(placement: &Placement) -> Placement {
    let mut out = Placement::empty();
    for (sq, piece) in placement.pieces() {
        if let Some(to) = Square::new(sq.file(), 7u8.saturating_sub(sq.rank())) {
            out.set(to, Some(Piece::new(piece.color.opponent(), piece.kind)));
        }
    }
    out
}

#[cfg(test)]
mod tests;
