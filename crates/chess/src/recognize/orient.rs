//! Which way up a diagram is: the reading of the image as printed, or turned half a circle.
//!
//! The diagram is read both ways and a cascade of rules decides. Each rule answers or stays
//! silent, and the order is the decision. The order comes from a measurement on 320 test
//! boards:
//!
//! | signal              | right | wrong | silent |
//! |---------------------|-------|-------|--------|
//! | legality            |    52 |     0 |    268 |
//! | minimum confidence  |   320 |     0 |      0 |
//! | pawn ranks          |   264 |     9 |     47 |
//!
//! Confidence decides well when the reading is good and stops deciding when it is poor: on a
//! badly printed book both orientations came out near 0.04 with margins of 0.001 and 0.019, which
//! is noise. The pawn prior looks at the structure of the position rather than the look of the
//! pieces and stays informative there, so it comes after the margin rule, not before.
//!
//! A diagram printed from Black's point of view is not upside down: the pieces stand upright and
//! what changes is which square is which. The printed coordinates reveal it ([`Coordinates`]);
//! without them, the pieces can ([`looks_printed_from_black`]): pawns on the other side's
//! starting rank and a king on the far back rank. The reading as printed is then right, and the
//! placement is turned. Measured on 5,457 labelled boards printed from White's side, the pieces
//! say Black's side for 3 (endgames with pawns about to promote); in 38 books, for 39 diagrams,
//! all in three books that print Black-to-move exercises from Black's side.

use super::Reading;
use crate::placement::{Color, Piece, PieceKind, Placement};
use crate::rules::fatal_violations;

/// A minimum-confidence margin at or above this decides. Above 0.20 confidence was right 320
/// times out of 320; below it, it becomes noise.
pub const DECISIVE_MARGIN: f32 = 0.20;

/// The pawn prior decides when the two readings differ by at least this many ranks.
pub const PAWN_PRIOR_MARGIN: f32 = 1.0;

/// The pieces say a board was printed from Black's side when at least this many more pawns stand
/// on the other side's starting rank than on their own ([`looks_printed_from_black`]).
pub const BLACK_VIEW_PAWN_GAP: usize = 2;

/// Coordinates printed around a diagram, when they were read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Coordinates {
    /// The rank labels down the side, top to bottom: `8, 7, … 1` is White's view.
    pub ranks_top_to_bottom: Vec<u8>,
    /// The file labels along the bottom or top, left to right: `a … h` is White's view.
    pub files_left_to_right: Vec<char>,
}

impl Coordinates {
    /// `Some(true)` for White's view, `Some(false)` for Black's, `None` when the labels say
    /// nothing or the ranks and files disagree.
    pub fn white_point_of_view(&self) -> Option<bool> {
        // Ranks descending top to bottom (8 … 1) and files ascending left to right (a … h) are
        // both White's view.
        let by_ranks = direction(&self.ranks_top_to_bottom);
        let by_files = direction(&self.files_left_to_right).map(|descending| !descending);
        match (by_ranks, by_files) {
            (Some(r), Some(f)) if r != f => None,
            (Some(r), _) => Some(r),
            (None, f) => f,
        }
    }
}

/// `Some(true)` for a strictly descending run, `Some(false)` ascending, `None` otherwise.
fn direction<T: Ord>(run: &[T]) -> Option<bool> {
    if run.len() < 2 {
        return None;
    }
    if run.windows(2).all(|w| w.first() > w.get(1)) {
        Some(true)
    } else if run.windows(2).all(|w| w.first() < w.get(1)) {
        Some(false)
    } else {
        None
    }
}

/// The rule that decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Rule {
    /// The printed coordinates.
    Coordinates,
    /// Only one orientation is legal.
    SingleLegal,
    /// The minimum confidences differ by at least [`DECISIVE_MARGIN`].
    ConfidenceMargin,
    /// The pawns point one way by at least [`PAWN_PRIOR_MARGIN`] ranks.
    PawnPrior,
    /// Nothing decided: the more confident reading, marked ambiguous.
    Fallback,
    /// A diagram set in a chess font: its squares are exact and it is upright, so only the pieces
    /// can say it was printed from Black's side ([`font_verdict`]). Not part of the cascade.
    Font,
}

/// The cascade, in order.
pub const CASCADE: [Rule; 5] = [Rule::Coordinates, Rule::SingleLegal, Rule::ConfidenceMargin, Rule::PawnPrior, Rule::Fallback];

/// What a rule concluded.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Verdict {
    pub rule: Rule,
    /// `true`: the reading as printed; `false`: the reading turned half a circle.
    pub upright: bool,
    /// The choice was close, or the signals disagreed: a person should look.
    pub ambiguous: bool,
    /// Black's point of view, said by the coordinates or else by the pieces: the reading chosen
    /// is right as printed and its placement must be turned.
    pub black_point_of_view: bool,
    /// It was the pieces that said Black's point of view ([`looks_printed_from_black`]), not
    /// printed coordinates: a guess a person may want to check.
    pub black_view_from_pieces: bool,
    /// Upright minus turned minimum confidence.
    pub margin: f32,
    /// How many ranks the pawn prior favours the upright reading by, when it applies.
    pub pawn_gap: Option<f32>,
}

/// How upright a position looks from its pawns: the mean rank of Black's pawns minus that of
/// White's. About +2 to +4 in a real position and the same with the sign flipped upside down.
/// `None` when a side has no pawn.
pub fn pawn_direction_score(placement: &Placement) -> Option<f32> {
    let mean_rank = |color| {
        let ranks: Vec<f32> =
            placement.pieces().filter(|(_, p)| *p == Piece::new(color, PieceKind::Pawn)).map(|(sq, _)| f32::from(sq.rank())).collect();
        (!ranks.is_empty()).then(|| ranks.iter().sum::<f32>() / ranks.len() as f32)
    };
    Some(mean_rank(Color::Black)? - mean_rank(Color::White)?)
}

/// What one rule says about the two readings, or `None` when it has nothing to say.
pub fn apply(rule: Rule, upright: &Reading, turned: &Reading, coordinates: Option<&Coordinates>) -> Option<Verdict> {
    let margin = upright.min_confidence - turned.min_confidence;
    let pawn_gap = match (pawn_direction_score(&upright.placement), pawn_direction_score(&turned.placement)) {
        (None, None) => None,
        // A reading without pawns of both colours does not score; the other one is the one
        // saying something about the structure.
        (a, b) => Some(a.unwrap_or(0.0) - b.unwrap_or(0.0)),
    };
    let verdict = |upright: bool, ambiguous: bool| Verdict {
        rule,
        upright,
        ambiguous,
        black_point_of_view: false,
        black_view_from_pieces: false,
        margin,
        pawn_gap,
    };
    match rule {
        Rule::Coordinates => {
            let white = coordinates?.white_point_of_view()?;
            Some(Verdict { black_point_of_view: !white, ..verdict(true, false) })
        }
        Rule::SingleLegal => {
            let legal_up = fatal_violations(&upright.placement).is_empty();
            let legal_turned = fatal_violations(&turned.placement).is_empty();
            // Disagreement with confidence never happened in measurement; if it does, that is
            // what "ambiguous" means, not something to settle silently.
            (legal_up != legal_turned).then(|| verdict(legal_up, (margin > 0.0) != legal_up))
        }
        Rule::ConfidenceMargin => (margin.abs() >= DECISIVE_MARGIN).then(|| verdict(margin > 0.0, false)),
        Rule::PawnPrior => {
            let gap = pawn_gap?;
            (gap.abs() >= PAWN_PRIOR_MARGIN).then(|| verdict(gap > 0.0, false))
        }
        Rule::Fallback => Some(verdict(margin >= 0.0, true)),
        Rule::Font => None,
    }
}

/// Whether `placement`, the reading as printed, has the pieces of a board printed from Black's
/// point of view: at least [`BLACK_VIEW_PAWN_GAP`] more pawns on the other side's starting rank
/// (White's on the seventh, Black's on the second) than on their own, a king on the far back rank
/// (White's on the eighth or Black's on the first), and a legal position once turned. A real
/// position seen from White's side has its unmoved pawns at home and its kings behind them.
pub fn looks_printed_from_black(placement: &Placement) -> bool {
    let count = |color, kind, rank: u8| placement.pieces().filter(|(sq, p)| *p == Piece::new(color, kind) && sq.rank() == rank).count();
    let away = count(Color::White, PieceKind::Pawn, 6).saturating_add(count(Color::Black, PieceKind::Pawn, 1));
    let home = count(Color::White, PieceKind::Pawn, 1).saturating_add(count(Color::Black, PieceKind::Pawn, 6));
    let king_far = count(Color::White, PieceKind::King, 7).saturating_add(count(Color::Black, PieceKind::King, 0)) > 0;
    away >= home.saturating_add(BLACK_VIEW_PAWN_GAP) && king_far && fatal_violations(&placement.rotated()).is_empty()
}

/// For a diagram set in a chess font, read exactly as printed: a verdict only when the pieces say
/// it was printed from Black's side ([`looks_printed_from_black`]), its placement then to be turned.
pub fn font_verdict(placement: &Placement) -> Option<Verdict> {
    looks_printed_from_black(placement).then_some(Verdict {
        rule: Rule::Font,
        upright: true,
        ambiguous: false,
        black_point_of_view: true,
        black_view_from_pieces: true,
        margin: 0.0,
        pawn_gap: None,
    })
}

/// The first rule of the cascade that answers. Without coordinates, the pieces of the reading it
/// chose then say whether the board was printed from Black's side ([`looks_printed_from_black`]).
pub fn orient(upright: &Reading, turned: &Reading, coordinates: Option<&Coordinates>) -> Verdict {
    let verdict = CASCADE.into_iter().find_map(|rule| apply(rule, upright, turned, coordinates)).unwrap_or(Verdict {
        // The fallback always answers, so this is never reached; it keeps the function total.
        rule: Rule::Fallback,
        upright: true,
        ambiguous: true,
        black_point_of_view: false,
        black_view_from_pieces: false,
        margin: upright.min_confidence - turned.min_confidence,
        pawn_gap: None,
    });
    if verdict.rule == Rule::Coordinates {
        return verdict;
    }
    let chosen = if verdict.upright { upright } else { turned };
    let from_pieces = looks_printed_from_black(&chosen.placement);
    Verdict { black_point_of_view: from_pieces, black_view_from_pieces: from_pieces, ..verdict }
}

/// What every rule says, in order, including the silent ones: for a diagnostic panel and for
/// measuring one rule alone.
pub fn explain(upright: &Reading, turned: &Reading, coordinates: Option<&Coordinates>) -> Vec<(Rule, Option<Verdict>)> {
    CASCADE.into_iter().map(|rule| (rule, apply(rule, upright, turned, coordinates))).collect()
}

#[cfg(test)]
mod tests {
    use super::super::testing::*;
    use super::super::{Probabilities, decode::Limits};
    use super::*;

    fn reading(fen: &str, p: f32) -> Reading {
        Reading::new(&Probabilities::new(&matrix(fen, p)).unwrap(), &Limits::default())
    }

    fn reading_with_weak_square(fen: &str, p: f32, weak: f32) -> Reading {
        let mut v = matrix(fen, p);
        set(&mut v, "a5", None, weak, piece('N'), 1.0 - weak - 0.01);
        Reading::new(&Probabilities::new(&v).unwrap(), &Limits::default())
    }

    // A middlegame with White's pawns low and Black's high, and the same turned half a circle.
    const UP: &str = "r1bqk2r/pp3ppp/2n2n2/8/8/2N2N2/PP3PPP/R1BQK2R";
    const TURNED: &str = "R2KQB1R/PPP3PP/2N2N2/8/8/2n2n2/ppp3pp/r2kqb1r";

    #[test]
    fn coordinates_decide_first() {
        let up = reading(UP, 0.99);
        let turned = reading(TURNED, 0.99);
        let black_view = Coordinates { ranks_top_to_bottom: (1..=8).collect(), files_left_to_right: vec![] };
        let v = orient(&up, &turned, Some(&black_view));
        assert_eq!(v.rule, Rule::Coordinates);
        assert!(v.upright && v.black_point_of_view);
        let white_view = Coordinates { ranks_top_to_bottom: vec![], files_left_to_right: "abcdefgh".chars().collect() };
        assert!(!orient(&up, &turned, Some(&white_view)).black_point_of_view);
    }

    #[test]
    fn the_pieces_say_a_board_printed_from_blacks_side() {
        // Hansen, Chess Tactics 2, exercise 24.1 (Black to move), as printed: White's pawns on
        // the seventh, Black's on the second, White's king on g8.
        let printed = "RQ2R1K1/2BN1PPP/4B3/P2P2p1/pP1p1N1p/3b1n2/1ppbnq2/1k3r1r";
        let p = crate::fen::parse_placement(printed).unwrap();
        assert!(looks_printed_from_black(&p));
        assert!(!looks_printed_from_black(&p.rotated()), "seen from White's side it is a plain middlegame");
        let v = orient(&reading(printed, 0.99), &reading(&crate::fen::format_placement(&p.rotated()), 0.5), None);
        assert!(v.upright && v.black_point_of_view && v.black_view_from_pieces);
        // A middlegame from White's side, and endgames with one pawn about to promote, are not.
        for fen in [UP, "8/3kP3/8/8/8/8/1p3K2/8", "6K1/8/8/8/2Q5/7p/p7/1k6", "8/8/8/8/8/8/8/K6k"] {
            assert!(!looks_printed_from_black(&crate::fen::parse_placement(fen).unwrap()), "{fen}");
        }
        // Coordinates still win: White's view printed says White's view.
        let white_view = Coordinates { ranks_top_to_bottom: vec![], files_left_to_right: "abcdefgh".chars().collect() };
        assert!(!orient(&reading(printed, 0.99), &reading(UP, 0.5), Some(&white_view)).black_point_of_view);
    }

    #[test]
    fn disagreeing_coordinates_say_nothing() {
        let c = Coordinates { ranks_top_to_bottom: (1..=8).collect(), files_left_to_right: "abcdefgh".chars().collect() };
        assert_eq!(c.white_point_of_view(), None);
        let c = Coordinates { ranks_top_to_bottom: vec![8, 1, 7], files_left_to_right: vec![] };
        assert_eq!(c.white_point_of_view(), None);
        assert_eq!(Coordinates::default().white_point_of_view(), None);
    }

    #[test]
    fn a_single_legal_orientation_wins() {
        // Upright legal; turned has a pawn on the back rank.
        let up = reading("4k3/8/8/8/8/8/P7/4K3", 0.95);
        let turned = reading("4K3/P7/8/8/8/8/8/4k3", 0.99);
        let turned = Reading { placement: crate::fen::parse_placement("P3K3/8/8/8/8/8/8/4k3").unwrap(), ..turned };
        let v = orient(&up, &turned, None);
        assert_eq!(v.rule, Rule::SingleLegal);
        assert!(v.upright);
        assert!(v.ambiguous, "confidence pointed the other way");
    }

    #[test]
    fn a_clear_margin_decides() {
        let up = reading(UP, 0.99);
        let turned = reading_with_weak_square(TURNED, 0.99, 0.5);
        let v = orient(&up, &turned, None);
        assert_eq!(v.rule, Rule::ConfidenceMargin);
        assert!(v.upright && !v.ambiguous);
    }

    #[test]
    fn pawns_decide_when_confidence_ties_low() {
        let up = reading_with_weak_square(UP, 0.99, 0.30);
        let turned = reading_with_weak_square(TURNED, 0.99, 0.31);
        let v = orient(&up, &turned, None);
        assert_eq!(v.rule, Rule::PawnPrior);
        assert!(v.upright);
        assert!(v.pawn_gap.unwrap() >= PAWN_PRIOR_MARGIN);
    }

    #[test]
    fn nothing_decides_and_it_says_so() {
        let up = reading("4k3/8/8/8/8/8/8/4K3", 0.97);
        let turned = reading("3K4/8/8/8/8/8/8/3k4", 0.98);
        let v = orient(&up, &turned, None);
        assert_eq!(v.rule, Rule::Fallback);
        assert!(!v.upright && v.ambiguous);
        let all = explain(&up, &turned, None);
        assert_eq!(all.len(), 5);
        assert!(all.iter().take(4).all(|(_, v)| v.is_none()));
    }

    #[test]
    fn pawn_scores() {
        let p = crate::fen::parse_placement(UP).unwrap();
        assert!(pawn_direction_score(&p).unwrap() > 2.0);
        assert!(pawn_direction_score(&p.rotated()).unwrap() < -2.0);
        assert_eq!(pawn_direction_score(&crate::fen::parse_placement("4k3/8/8/8/8/8/P7/4K3").unwrap()), None);
    }
}
