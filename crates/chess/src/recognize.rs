//! Recognition post-processing: from per-square probabilities to a legal, explained reading.
//!
//! A diagram classifier looks at each of the 64 squares and returns a probability for each of
//! 13 classes (empty, six white pieces, six black ones). This module turns that matrix into a
//! position. It knows nothing about pixels or models, so any classifier can feed it, and it is
//! tested with synthetic matrices.
//!
//! - [`decode`] finds the most probable assignment that obeys the rules a diagram can be checked
//!   against without knowing whose move it is.
//! - [`orient`] decides between the reading of the image as printed and the reading turned half
//!   a circle.
//! - [`side`] decides who is to move, and always says how it knows.
//!
//! **Confidence is the minimum, not the mean.** About 77% of squares are empty and trivially
//! right, so the mean stays near 0.97 on a board with a wrong square. A square below
//! [`UNCERTAIN_SQUARE_THRESHOLD`] is flagged for review, and a reading whose weakest square is
//! below [`ACCEPT_MIN_CONFIDENCE`] goes to review instead of export. A square the decoder
//! repaired carries a class that was not the most probable, whose probability can be at most
//! 0.5, so a repaired reading never passes that gate. That is arithmetic, not a defect: the
//! repair makes the reading legal, and a person confirms it.

pub mod decode;
pub mod orient;
pub mod side;

use std::fmt;

use crate::placement::{Color, Piece, PieceKind, Placement, Square};
use crate::rules::{Legality, check_position};

/// The classes, in the order a classifier's 13 outputs use: empty, then White's pawn, knight,
/// bishop, rook, queen and king, then Black's.
pub const CLASSES: [Option<Piece>; 13] = [
    None,
    Some(Piece::new(Color::White, PieceKind::Pawn)),
    Some(Piece::new(Color::White, PieceKind::Knight)),
    Some(Piece::new(Color::White, PieceKind::Bishop)),
    Some(Piece::new(Color::White, PieceKind::Rook)),
    Some(Piece::new(Color::White, PieceKind::Queen)),
    Some(Piece::new(Color::White, PieceKind::King)),
    Some(Piece::new(Color::Black, PieceKind::Pawn)),
    Some(Piece::new(Color::Black, PieceKind::Knight)),
    Some(Piece::new(Color::Black, PieceKind::Bishop)),
    Some(Piece::new(Color::Black, PieceKind::Rook)),
    Some(Piece::new(Color::Black, PieceKind::Queen)),
    Some(Piece::new(Color::Black, PieceKind::King)),
];

/// Number of classes per square.
pub const CLASS_COUNT: usize = 13;

/// The class index of the empty square.
pub const EMPTY: u8 = 0;

/// The class index of a square's content.
pub fn class_of(piece: Option<Piece>) -> u8 {
    // CLASSES has 13 entries, so the position always fits in a u8.
    CLASSES.iter().position(|c| *c == piece).map_or(EMPTY, |i| i as u8)
}

/// The content a class index stands for (`None` for empty or an index out of range).
pub fn piece_of(class: u8) -> Option<Piece> {
    CLASSES.get(usize::from(class)).copied().flatten()
}

/// A square whose confidence is at or below this goes into [`Reading::uncertain`]. High on
/// purpose: the classifier is confident even when wrong (about 0.80 on wrong squares against
/// 0.999 on right ones), so a low threshold would separate nothing.
pub const UNCERTAIN_SQUARE_THRESHOLD: f32 = 0.90;

/// Below this minimum confidence a reading goes to review instead of export.
pub const ACCEPT_MIN_CONFIDENCE: f32 = 0.80;

/// Why a probability matrix was rejected.
#[derive(Clone, Debug, PartialEq)]
pub enum ProbabilityError {
    /// Not 64 × 13 values.
    Shape(usize),
    /// A value is NaN, infinite or negative.
    BadValue { square: usize, class: usize },
    /// A square's values sum to zero.
    ZeroRow(usize),
}

impl fmt::Display for ProbabilityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProbabilityError::Shape(n) => write!(f, "expected 64 × 13 = 832 probabilities, got {n}"),
            ProbabilityError::BadValue { square, class } => {
                write!(f, "probability for square {square}, class {class} is not a finite non-negative number")
            }
            ProbabilityError::ZeroRow(s) => write!(f, "the probabilities for square {s} sum to zero"),
        }
    }
}

impl std::error::Error for ProbabilityError {}

/// Per-square class probabilities, in reading order (a8, b8 … h1), each row summing to 1.
#[derive(Clone, Debug, PartialEq)]
pub struct Probabilities {
    rows: [[f32; CLASS_COUNT]; 64],
}

impl Probabilities {
    /// Validate a classifier's output: 64 rows of 13 values, row-major, finite and
    /// non-negative. Rows are normalized to sum to 1, so raw scores that are already
    /// non-negative are accepted too.
    pub fn new(values: &[f32]) -> Result<Probabilities, ProbabilityError> {
        if values.len() != 64 * CLASS_COUNT {
            return Err(ProbabilityError::Shape(values.len()));
        }
        let mut rows = [[0.0f32; CLASS_COUNT]; 64];
        // The length was checked above, so there is no remainder.
        let (chunks, _) = values.as_chunks::<CLASS_COUNT>();
        for (square, (row, chunk)) in rows.iter_mut().zip(chunks).enumerate() {
            let mut sum = 0.0f32;
            for (class, (out, &v)) in row.iter_mut().zip(chunk).enumerate() {
                if !v.is_finite() || v < 0.0 {
                    return Err(ProbabilityError::BadValue { square, class });
                }
                *out = v;
                sum += v;
            }
            // Every value is finite and non-negative, so the sum is never NaN; it can still
            // overflow to infinity or be zero.
            if sum <= 0.0 || !sum.is_finite() {
                return Err(ProbabilityError::ZeroRow(square));
            }
            for out in row.iter_mut() {
                *out /= sum;
            }
        }
        Ok(Probabilities { rows })
    }

    /// The probability of `class` on the square at reading position `square`.
    pub fn get(&self, square: usize, class: u8) -> f32 {
        self.rows.get(square).and_then(|r| r.get(usize::from(class))).copied().unwrap_or(0.0)
    }

    /// The most probable class of every square.
    pub fn argmax(&self) -> [u8; 64] {
        let mut out = [EMPTY; 64];
        for (cell, row) in out.iter_mut().zip(&self.rows) {
            *cell = best_class(row, None).unwrap_or(EMPTY);
        }
        out
    }

    fn row(&self, square: usize) -> Option<&[f32; CLASS_COUNT]> {
        self.rows.get(square)
    }
}

/// The most probable class of a row, skipping `except`.
fn best_class(row: &[f32; CLASS_COUNT], except: Option<u8>) -> Option<u8> {
    (0u8..).zip(row).filter(|(c, _)| Some(*c) != except).max_by(|a, b| a.1.total_cmp(b.1)).map(|(c, _)| c)
}

/// The placement a reading-order class assignment describes.
pub fn placement_of(cells: &[u8; 64]) -> Placement {
    let mut pieces = [None; 64];
    for (out, &c) in pieces.iter_mut().zip(cells) {
        *out = piece_of(c);
    }
    Placement::from_reading_order(&pieces)
}

/// A square the classifier was not sure about.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Uncertain {
    pub square: Square,
    /// The probability of what the reading put there.
    pub confidence: f32,
    /// The next most probable content, and its probability: the second reading of the square.
    pub alternative: Option<Piece>,
    pub alternative_confidence: f32,
}

/// One reading of a diagram, from one image orientation.
#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    /// The chosen class of every square, in reading order.
    pub cells: [u8; 64],
    pub placement: Placement,
    /// The probability of the chosen class of every square, in reading order.
    pub confidences: [f32; 64],
    /// The lowest of `confidences`.
    pub min_confidence: f32,
    /// Squares at or below [`UNCERTAIN_SQUARE_THRESHOLD`], in reading order.
    pub uncertain: Vec<Uncertain>,
    /// What the decoder did.
    pub decoded: decode::Decoded,
    pub legality: Legality,
}

impl Reading {
    /// Decode `probs` and describe the result.
    pub fn new(probs: &Probabilities, limits: &decode::Limits) -> Reading {
        let decoded = decode::decode(probs, limits);
        let cells = decoded.cells;
        let mut confidences = [0.0f32; 64];
        let mut uncertain = Vec::new();
        for (i, (conf, &class)) in confidences.iter_mut().zip(&cells).enumerate() {
            *conf = probs.get(i, class);
            if *conf <= UNCERTAIN_SQUARE_THRESHOLD
                && let (Some(square), Some(row)) = (Square::from_reading_index(i), probs.row(i))
            {
                let alt = best_class(row, Some(class));
                uncertain.push(Uncertain {
                    square,
                    confidence: *conf,
                    alternative: alt.and_then(piece_of),
                    alternative_confidence: alt.map_or(0.0, |a| probs.get(i, a)),
                });
            }
        }
        let min_confidence = confidences.iter().copied().fold(1.0f32, f32::min);
        let placement = placement_of(&cells);
        let legality = check_position(&placement);
        Reading { cells, placement, confidences, min_confidence, uncertain, decoded, legality }
    }

    /// Whether the reading can be exported without review: legal for some side to move and
    /// with every square at or above [`ACCEPT_MIN_CONFIDENCE`].
    pub fn accepted(&self) -> bool {
        self.legality.is_legal_for_some_side() && self.min_confidence >= ACCEPT_MIN_CONFIDENCE
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use crate::fen::parse_placement;

    /// A matrix whose argmax is `fen` with probability `p` on every square, the rest spread
    /// evenly over the other classes.
    pub fn matrix(fen: &str, p: f32) -> Vec<f32> {
        let placement = parse_placement(fen).unwrap();
        let mut v = Vec::with_capacity(832);
        for cell in placement.reading_order() {
            let c = class_of(cell);
            for k in 0..13u8 {
                v.push(if k == c { p } else { (1.0 - p) / 12.0 });
            }
        }
        v
    }

    /// Set the row of the square `name` to put `first` at `p1` and `second` at `p2`.
    pub fn set(v: &mut [f32], name: &str, first: Option<Piece>, p1: f32, second: Option<Piece>, p2: f32) {
        let i = Square::parse(name).unwrap().reading_index();
        let rest = (1.0 - p1 - p2) / 11.0;
        for k in 0..13u8 {
            v[i * 13 + usize::from(k)] = if k == class_of(first) {
                p1
            } else if k == class_of(second) {
                p2
            } else {
                rest
            };
        }
    }

    pub fn piece(c: char) -> Option<Piece> {
        Piece::from_fen_char(c)
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;

    #[test]
    fn classes_round_trip() {
        for (i, c) in CLASSES.iter().enumerate() {
            assert_eq!(usize::from(class_of(*c)), i);
            assert_eq!(piece_of(i as u8), *c);
        }
        assert_eq!(piece_of(13), None);
    }

    #[test]
    fn bad_matrices_are_rejected() {
        assert_eq!(Probabilities::new(&[0.5; 10]), Err(ProbabilityError::Shape(10)));
        let mut v = matrix("8/8/8/4k3/8/8/8/4K3", 0.99);
        v[5] = f32::NAN;
        assert_eq!(Probabilities::new(&v), Err(ProbabilityError::BadValue { square: 0, class: 5 }));
        v[5] = -0.1;
        assert!(Probabilities::new(&v).is_err());
        let mut z = matrix("8/8/8/4k3/8/8/8/4K3", 0.99);
        for x in z.iter_mut().take(13) {
            *x = 0.0;
        }
        assert_eq!(Probabilities::new(&z), Err(ProbabilityError::ZeroRow(0)));
    }

    #[test]
    fn rows_are_normalized() {
        let raw: Vec<f32> = matrix("8/8/8/4k3/8/8/8/4K3", 0.99).iter().map(|x| x * 3.0).collect();
        let p = Probabilities::new(&raw).unwrap();
        assert!((p.get(0, EMPTY) - 0.99).abs() < 1e-5);
    }

    #[test]
    fn a_clean_reading_is_accepted() {
        let p = Probabilities::new(&matrix("8/8/8/4k3/8/8/8/4K3", 0.99)).unwrap();
        let r = Reading::new(&p, &decode::Limits::default());
        assert_eq!(crate::fen::format_placement(&r.placement), "8/8/8/4k3/8/8/8/4K3");
        assert!(r.uncertain.is_empty());
        assert!(r.accepted());
    }

    #[test]
    fn uncertain_squares_carry_their_second_reading() {
        let mut v = matrix("8/8/8/4k3/8/8/8/4K3", 0.99);
        set(&mut v, "d4", None, 0.7, piece('N'), 0.25);
        let r = Reading::new(&Probabilities::new(&v).unwrap(), &decode::Limits::default());
        assert_eq!(r.uncertain.len(), 1);
        let u = r.uncertain[0];
        assert_eq!(u.square, Square::parse("d4").unwrap());
        assert_eq!(u.alternative, piece('N'));
        assert!((u.alternative_confidence - 0.25).abs() < 1e-5);
        assert!(!r.accepted(), "0.7 is below the export gate");
    }

    #[test]
    fn a_repaired_reading_never_passes_the_gate() {
        // German figurine case: the white king read as a queen, the king a close second.
        let mut v = matrix("8/8/8/4k3/8/8/8/4K3", 0.99);
        set(&mut v, "e1", piece('Q'), 0.55, piece('K'), 0.44);
        let r = Reading::new(&Probabilities::new(&v).unwrap(), &decode::Limits::default());
        assert_eq!(crate::fen::format_placement(&r.placement), "8/8/8/4k3/8/8/8/4K3");
        assert_eq!(r.decoded.changes.len(), 1);
        assert!(r.min_confidence <= 0.5);
        assert!(!r.accepted());
    }
}
