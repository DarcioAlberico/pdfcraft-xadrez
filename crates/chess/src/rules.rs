//! Legality checks that do not need the side to move.
//!
//! A book diagram rarely says who is to move, so a violation that depends on the side to move
//! (the side not to move is in check) is not necessarily a misreading: it may only mean that
//! Black is to move. [`check_position`] therefore separates three outcomes:
//!
//! - **legal** whichever side is to move;
//! - **legal only with one side to move**, which is how the side to move is inferred later;
//! - **illegal** whoever moves: a king missing, a pawn on the back rank, more pieces than
//!   promotions could have made. That is a recognition error, not a question of turn.
//!
//! A whole position (with its side to move, castling rights and en-passant square) can break two
//! rules more: [`position_violations`] names them too, for saying why a FEN is not legal.
//!
//! The rules come from the laws of chess, not from another implementation.

use std::fmt;

use crate::fen::Position;
use crate::placement::{Color, Piece, PieceKind, Placement, Square};

/// One reason a position cannot occur in a game.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Violation {
    /// No piece on the board at all.
    Empty,
    /// `Color` has no king.
    NoKing(Color),
    /// `Color` has more than one king.
    TooManyKings(Color),
    /// `Color` has more than 8 pawns.
    TooManyPawns(Color),
    /// `Color` has more than 16 pieces.
    TooManyPieces(Color),
    /// A pawn stands on the first or the eighth rank.
    PawnOnBackRank,
    /// `Color` has more extra pieces (a second queen, a third rook, two bishops on squares of
    /// one colour…) than its missing pawns could have promoted to.
    ImpossiblePromotions(Color),
    /// The two kings stand next to each other.
    AdjacentKings,
    /// `Color` is in check although it is not its turn.
    NotToMoveInCheck(Color),
    /// `Color` is in check from more than two pieces.
    TooManyCheckers(Color),
    /// `Color` is in double check by pieces that no single move can uncover together: a double
    /// check always involves a line piece whose line was opened.
    ImpossibleDoubleCheck(Color),
    /// `Color` keeps a castling right its king or that rook no longer allows: the king off its
    /// first square, or no rook in that corner. Only a whole position has rights.
    BadCastling(Color),
    /// The en-passant square is not one the last move's double step passed: no pawn of the side
    /// that moved stands in front of it, or the square or the one behind it is taken. Only a
    /// whole position has one.
    BadEnPassant,
}

impl Violation {
    /// Whether the violation holds whichever side is to move.
    pub fn is_fatal(self) -> bool {
        !matches!(
            self,
            Violation::NotToMoveInCheck(_) | Violation::TooManyCheckers(_) | Violation::ImpossibleDoubleCheck(_) | Violation::BadEnPassant
        )
    }
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let side = |c: &Color| if *c == Color::White { "White" } else { "Black" };
        match self {
            Violation::Empty => write!(f, "the board is empty"),
            Violation::NoKing(c) => write!(f, "{}'s king is missing", side(c)),
            Violation::TooManyKings(c) => write!(f, "{} has more than one king", side(c)),
            Violation::TooManyPawns(c) => write!(f, "{} has more than 8 pawns", side(c)),
            Violation::TooManyPieces(c) => write!(f, "{} has more than 16 pieces", side(c)),
            Violation::PawnOnBackRank => write!(f, "a pawn stands on the first or the eighth rank"),
            Violation::ImpossiblePromotions(c) => write!(f, "{} has more extra pieces than missing pawns to promote", side(c)),
            Violation::AdjacentKings => write!(f, "the kings stand next to each other"),
            Violation::NotToMoveInCheck(c) => write!(f, "{} is in check, but it is the other side's turn", side(c)),
            Violation::TooManyCheckers(c) => write!(f, "{}'s king is in check from more than two pieces", side(c)),
            Violation::ImpossibleDoubleCheck(c) => write!(f, "{}'s king is in a double check no move could give", side(c)),
            Violation::BadCastling(c) => write!(f, "{} may castle, but its king or rook has left its square", side(c)),
            Violation::BadEnPassant => write!(f, "the en-passant square does not fit the pawns"),
        }
    }
}

/// What [`check_position`] concluded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Legality {
    /// Legal whichever side is to move.
    Legal,
    /// Legal only with `side` to move. `other` says why the other side cannot be.
    OnlyWithSideToMove { side: Color, other: Vec<Violation> },
    /// Illegal whoever is to move.
    Illegal(Vec<Violation>),
}

impl Legality {
    /// Whether some side to move makes the position legal.
    pub fn is_legal_for_some_side(&self) -> bool {
        !matches!(self, Legality::Illegal(_))
    }

    /// Whether the position is legal with `side` to move.
    pub fn allows(&self, side: Color) -> bool {
        match self {
            Legality::Legal => true,
            Legality::OnlyWithSideToMove { side: s, .. } => *s == side,
            Legality::Illegal(_) => false,
        }
    }
}

/// Classify a placement without knowing who is to move.
pub fn check_position(placement: &Placement) -> Legality {
    let fatal = fatal_violations(placement);
    if !fatal.is_empty() {
        return Legality::Illegal(fatal);
    }
    let white = turn_violations(placement, Color::White);
    let black = turn_violations(placement, Color::Black);
    match (white.is_empty(), black.is_empty()) {
        (true, true) => Legality::Legal,
        (true, false) => Legality::OnlyWithSideToMove { side: Color::White, other: black },
        (false, true) => Legality::OnlyWithSideToMove { side: Color::Black, other: white },
        (false, false) => {
            let mut all = white;
            for v in black {
                if !all.contains(&v) {
                    all.push(v);
                }
            }
            Legality::Illegal(all)
        }
    }
}

/// Every violation with `side` to move: the fatal ones and those that depend on the turn.
pub fn violations_with_side(placement: &Placement, side: Color) -> Vec<Violation> {
    let mut v = fatal_violations(placement);
    if v.is_empty() {
        v.extend(turn_violations(placement, side));
    }
    v
}

/// Every violation of a whole position, with `side` to move when it does not say: its
/// placement's ([`violations_with_side`]), then castling rights the pieces no longer allow and an
/// en-passant square no double step could have left. Empty for a legal position; a position
/// whose board the game refuses for a rule not named here gets none either.
pub fn position_violations(position: &Position, side: Color) -> Vec<Violation> {
    let side = position.side_to_move.unwrap_or(side);
    let mut v = violations_with_side(&position.placement, side);
    let fitted = position.castling.fitted(&position.placement);
    let c = position.castling;
    if (c.white_king_side, c.white_queen_side) != (fitted.white_king_side, fitted.white_queen_side) {
        v.push(Violation::BadCastling(Color::White));
    }
    if (c.black_king_side, c.black_queen_side) != (fitted.black_king_side, fitted.black_queen_side) {
        v.push(Violation::BadCastling(Color::Black));
    }
    if let Some(ep) = position.en_passant
        && !en_passant_fits(&position.placement, ep, side)
    {
        v.push(Violation::BadEnPassant);
    }
    v
}

/// Whether `ep` is the square a pawn of the side not to move (`side` moves) just passed with a
/// double step: on the third rank from its side, the pawn right in front of it, and the square
/// and the one it came from empty.
fn en_passant_fits(placement: &Placement, ep: Square, side: Color) -> bool {
    let mover = side.opponent();
    // Ranks from 0: White's double step passes the third rank (2) to the fourth (3); Black's
    // passes the sixth (5) to the fifth (4).
    let (passed, landed, left) = if mover == Color::White { (2, 3, 1) } else { (5, 4, 6) };
    let at = |rank: u8| Square::new(ep.file(), rank);
    ep.rank() == passed
        && at(landed).is_some_and(|sq| placement.get(sq) == Some(Piece::new(mover, PieceKind::Pawn)))
        && placement.get(ep).is_none()
        && at(left).is_some_and(|sq| placement.get(sq).is_none())
}

/// The violations that hold whoever is to move.
pub fn fatal_violations(placement: &Placement) -> Vec<Violation> {
    if placement.is_empty() {
        return vec![Violation::Empty];
    }
    let mut v = Vec::new();
    for color in [Color::White, Color::Black] {
        let kings = placement.count(Piece::new(color, PieceKind::King));
        if kings == 0 {
            v.push(Violation::NoKing(color));
        } else if kings > 1 {
            v.push(Violation::TooManyKings(color));
        }
        if placement.count(Piece::new(color, PieceKind::Pawn)) > 8 {
            v.push(Violation::TooManyPawns(color));
        }
        if placement.count_color(color) > 16 {
            v.push(Violation::TooManyPieces(color));
        }
        if promotions_needed(placement, color) > missing_pawns(placement, color) {
            v.push(Violation::ImpossiblePromotions(color));
        }
    }
    if placement.pieces().any(|(sq, p)| p.kind == PieceKind::Pawn && (sq.rank() == 0 || sq.rank() == 7)) {
        v.push(Violation::PawnOnBackRank);
    }
    if let (Some(w), Some(b)) = (placement.king(Color::White), placement.king(Color::Black))
        && w.file().abs_diff(b.file()) <= 1
        && w.rank().abs_diff(b.rank()) <= 1
    {
        v.push(Violation::AdjacentKings);
    }
    v
}

/// Pawns `color` no longer has, each of which may have promoted.
fn missing_pawns(placement: &Placement, color: Color) -> usize {
    8usize.saturating_sub(placement.count(Piece::new(color, PieceKind::Pawn)))
}

/// Pieces of `color` beyond what the initial position holds, each needing a promotion.
/// Bishops count per square colour: one light-squared and one dark-squared bishop to start.
fn promotions_needed(placement: &Placement, color: Color) -> usize {
    let count = |kind| placement.count(Piece::new(color, kind));
    let bishops = |light: bool| placement.pieces().filter(|(sq, p)| *p == Piece::new(color, PieceKind::Bishop) && sq.is_light() == light).count();
    count(PieceKind::Queen).saturating_sub(1)
        + count(PieceKind::Rook).saturating_sub(2)
        + count(PieceKind::Knight).saturating_sub(2)
        + bishops(true).saturating_sub(1)
        + bishops(false).saturating_sub(1)
}

/// The violations that depend on `side` being the side to move. Assumes one king per side;
/// without it there is nothing to say about checks.
fn turn_violations(placement: &Placement, side: Color) -> Vec<Violation> {
    let mut v = Vec::new();
    let (Some(own), Some(theirs)) = (placement.king(side), placement.king(side.opponent())) else {
        return v;
    };
    if !attackers(placement, theirs, side).is_empty() {
        v.push(Violation::NotToMoveInCheck(side.opponent()));
    }
    let checkers = attackers(placement, own, side.opponent());
    if checkers.len() > 2 {
        v.push(Violation::TooManyCheckers(side));
    } else if checkers.len() == 2 && !checkers.iter().any(|(_, kind)| kind.is_slider()) {
        v.push(Violation::ImpossibleDoubleCheck(side));
    }
    v
}

const KNIGHT_STEPS: [(i8, i8); 8] = [(1, 2), (2, 1), (2, -1), (1, -2), (-1, -2), (-2, -1), (-2, 1), (-1, 2)];
const KING_STEPS: [(i8, i8); 8] = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)];
const ROOK_DIRS: [(i8, i8); 4] = [(1, 0), (0, 1), (-1, 0), (0, -1)];
const BISHOP_DIRS: [(i8, i8); 4] = [(1, 1), (-1, 1), (-1, -1), (1, -1)];

/// The pieces of `by` that attack `target`, with their squares.
pub fn attackers(placement: &Placement, target: Square, by: Color) -> Vec<(Square, PieceKind)> {
    let mut found = Vec::new();
    let is = |sq: Square, kind: PieceKind| placement.get(sq) == Some(Piece::new(by, kind));

    // A white pawn attacks diagonally upwards, so it stands one rank below its target.
    let pawn_rank = match by {
        Color::White => -1,
        Color::Black => 1,
    };
    for df in [-1, 1] {
        if let Some(sq) = target.offset(df, pawn_rank).filter(|&sq| is(sq, PieceKind::Pawn)) {
            found.push((sq, PieceKind::Pawn));
        }
    }
    for (steps, kind) in [(&KNIGHT_STEPS, PieceKind::Knight), (&KING_STEPS, PieceKind::King)] {
        for &(df, dr) in steps {
            if let Some(sq) = target.offset(df, dr).filter(|&sq| is(sq, kind)) {
                found.push((sq, kind));
            }
        }
    }
    for (dirs, line_kind) in [(&ROOK_DIRS, PieceKind::Rook), (&BISHOP_DIRS, PieceKind::Bishop)] {
        for &(df, dr) in dirs {
            let mut sq = target;
            // At most 7 steps reach the edge of the board.
            for _ in 0..7 {
                let Some(next) = sq.offset(df, dr) else { break };
                sq = next;
                if let Some(piece) = placement.get(sq) {
                    if piece.color == by && (piece.kind == line_kind || piece.kind == PieceKind::Queen) {
                        found.push((sq, piece.kind));
                    }
                    break;
                }
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fen::parse_placement;

    fn check(fen: &str) -> Legality {
        check_position(&parse_placement(fen).unwrap())
    }

    #[test]
    fn the_start_position_is_legal_for_both_sides() {
        assert_eq!(check_position(&Placement::standard()), Legality::Legal);
    }

    #[test]
    fn a_king_in_check_decides_the_side_to_move() {
        // The black king on e8 is attacked by the rook on e1: only Black can be to move.
        let l = check("4k3/8/8/8/8/8/8/K3R3");
        assert_eq!(l, Legality::OnlyWithSideToMove { side: Color::Black, other: vec![Violation::NotToMoveInCheck(Color::Black)] });
        assert!(l.allows(Color::Black));
        assert!(!l.allows(Color::White));
    }

    #[test]
    fn a_blocked_line_is_not_a_check() {
        assert_eq!(check("4k3/8/8/8/4p3/8/8/K3R3"), Legality::Legal);
    }

    #[test]
    fn pawns_attack_forwards_only() {
        // White pawn d7 attacks e8; a white pawn on e7 does not.
        assert!(matches!(check("4k3/3P4/8/8/8/8/8/K7"), Legality::OnlyWithSideToMove { side: Color::Black, .. }));
        assert_eq!(check("4k3/4P3/8/8/8/8/8/K7"), Legality::Legal);
        // A black pawn on d2 attacks e1, not e3.
        assert!(matches!(check("k7/8/8/8/8/8/3p4/4K3"), Legality::OnlyWithSideToMove { side: Color::White, .. }));
    }

    #[test]
    fn missing_and_extra_kings_are_fatal() {
        assert_eq!(check("8/8/8/8/8/8/8/K7"), Legality::Illegal(vec![Violation::NoKing(Color::Black)]));
        assert_eq!(check("k7/8/8/8/8/8/8/K6K"), Legality::Illegal(vec![Violation::TooManyKings(Color::White)]));
        assert_eq!(check("8/8/8/8/8/8/8/8"), Legality::Illegal(vec![Violation::Empty]));
    }

    #[test]
    fn pawns_on_the_back_rank_are_fatal() {
        assert_eq!(check("P3k3/8/8/8/8/8/8/K7"), Legality::Illegal(vec![Violation::PawnOnBackRank]));
    }

    #[test]
    fn adjacent_kings_are_fatal() {
        assert_eq!(check("8/8/8/3kK3/8/8/8/8"), Legality::Illegal(vec![Violation::AdjacentKings]));
    }

    #[test]
    fn too_many_pawns_and_pieces_are_fatal() {
        let l = check("4k3/8/8/8/8/P7/PPPPPPPP/4K3");
        assert!(matches!(&l, Legality::Illegal(v) if v.contains(&Violation::TooManyPawns(Color::White))));
    }

    #[test]
    fn extra_pieces_need_missing_pawns() {
        // Two white queens with all eight pawns: impossible. With seven pawns: a promotion.
        let l = check("3qk3/pppppppp/8/8/8/8/PPPPPPPP/2QQK3");
        assert!(matches!(&l, Legality::Illegal(v) if v.contains(&Violation::ImpossiblePromotions(Color::White))));
        assert!(check("3qk3/pppppppp/8/8/8/8/1PPPPPPP/2QQK3").is_legal_for_some_side());
    }

    #[test]
    fn two_bishops_on_one_square_colour_need_a_promotion() {
        // c1 and e3 are both dark squares; all eight pawns are still on the board.
        let l = check("4k3/pppppppp/8/8/8/4B3/PPPPPPPP/2B1K3");
        assert!(matches!(&l, Legality::Illegal(v) if v.contains(&Violation::ImpossiblePromotions(Color::White))));
        // c1 dark and f1 light: the normal pair.
        assert!(check("4k3/pppppppp/8/8/8/8/PPPPPPPP/2B1KB2").is_legal_for_some_side());
    }

    #[test]
    fn double_check_needs_a_line_piece() {
        // Knight f6 and pawn d7 both check the black king on e8: no move uncovers that.
        let l = check("4k3/3P4/5N2/8/8/8/8/K7");
        assert!(matches!(&l, Legality::Illegal(v) if v.contains(&Violation::ImpossibleDoubleCheck(Color::Black))));
        // Rook e1 and knight f6: a discovered double check, legal with Black to move.
        assert!(matches!(check("4k3/8/5N2/8/8/8/8/K3R3"), Legality::OnlyWithSideToMove { side: Color::Black, .. }));
    }

    #[test]
    fn both_kings_in_check_is_illegal() {
        // Rook h1 checks the white king on e1; rook e2 checks the black king on e8.
        let l = check("4k3/8/8/8/8/8/4R3/4K2r");
        assert!(matches!(&l, Legality::Illegal(v)
            if v.contains(&Violation::NotToMoveInCheck(Color::White)) && v.contains(&Violation::NotToMoveInCheck(Color::Black))));
    }

    #[test]
    fn legal_positions_convert_to_cozy_chess() {
        for fen in ["4k3/8/8/8/8/8/8/K3R3", "r3k2r/8/8/8/8/8/8/R3K2R"] {
            let pos = crate::fen::parse(fen).unwrap();
            let legality = check_position(&pos.placement);
            for side in [Color::White, Color::Black] {
                assert_eq!(legality.allows(side), pos.to_board(side).is_ok(), "{fen} {side:?}");
            }
        }
    }

    /// A whole FEN's reasons (plan A8, P3b1): castling rights the pieces no longer allow and an
    /// en-passant square no double step left are named, besides the placement's violations.
    #[test]
    fn whole_positions_say_why_they_are_not_legal() {
        let why = |fen: &str| position_violations(&crate::fen::parse(fen).unwrap(), Color::White);
        assert_eq!(why("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1"), vec![]);
        assert_eq!(why("r3k2r/8/8/8/8/8/8/R4K1R w KQkq - 0 1"), vec![Violation::BadCastling(Color::White)], "the king left e1");
        assert_eq!(why("r3k3/8/8/8/8/8/8/R3K2R w KQkq - 0 1"), vec![Violation::BadCastling(Color::Black)], "no rook on h8");
        assert_eq!(why("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 2"), vec![], "d7-d5 just played");
        assert_eq!(why("4k3/8/8/4P3/8/8/8/4K3 w - d6 0 2"), vec![Violation::BadEnPassant], "no black pawn on d5");
        assert_eq!(why("4k3/8/8/8/3P4/8/8/4K3 b - d3 0 1"), vec![], "d2-d4 just played");
        assert_eq!(why("4k3/8/8/8/3P4/8/3P4/4K3 b - d3 0 1"), vec![Violation::BadEnPassant], "d2 is taken");
        assert_eq!(why("8/8/8/8/8/8/8/4K3 w - - 0 1"), vec![Violation::NoKing(Color::Black)]);
        assert_eq!(why("4k3/4Q3/8/8/8/8/8/4K3 w - - 0 1"), vec![Violation::NotToMoveInCheck(Color::Black)]);
        // The side from the FEN wins over the one given.
        assert_eq!(why("4k3/4Q3/8/8/8/8/8/4K3 b - - 0 1"), vec![]);
        assert_eq!(Violation::NoKing(Color::Black).to_string(), "Black's king is missing");
    }

    /// The reasons agree with the game's own verdict: a position a game can't start from has one,
    /// and one it can has none, over positions breaking each named rule.
    #[test]
    fn the_reasons_agree_with_the_board() {
        for fen in [
            "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1",
            "r3k2r/8/8/8/8/8/8/R4K1R w KQkq - 0 1",
            "4k3/8/8/4P3/8/8/8/4K3 w - d6 0 2",
            "4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 2",
            "8/8/8/8/8/8/8/4K3 w - - 0 1",
            "4k3/4Q3/8/8/8/8/8/4K3 w - - 0 1",
            "4k3/8/8/8/8/8/8/4KK2 w - - 0 1",
            "P3k3/8/8/8/8/8/8/4K3 w - - 0 1",
            "8/8/8/8/8/8/8/8 w - - 0 1",
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        ] {
            let pos = crate::fen::parse(fen).unwrap();
            let refused = crate::game::Game::from_fen(fen).is_err();
            assert_eq!(!position_violations(&pos, Color::White).is_empty(), refused, "{fen}");
        }
    }
}
