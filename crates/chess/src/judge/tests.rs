//! The card's hand-made cases (plan A9-N17a), each with its verdict, and the limits of D29.

use super::*;
use proptest::prelude::*;

const GOOD: u16 = 1;
const BRILLIANT: u16 = 3;
const POOR: u16 = 2;
const EQUAL: u16 = 10;
const WHITE_WINS: u16 = 18;
const BLACK_WINS: u16 = 19;

fn cp(cp: i32, depth: u32) -> Eval {
    Eval::new(Value::Cp(cp), depth)
}

/// A move `played` by `mover` with `nags`, the best move `best` keeping `before` and the move
/// played leaving `after` (both for the mover).
fn judged(nags: &[u16], mover: Color, played: &str, best: &str, before: Eval, after: Eval) -> Outcome {
    judge(&Judged { nags, mover, played, before: Some((before, best)), after: Some(after) }, &Limits::default())
}

#[test]
fn a_good_move_that_loses_two_pawns_is_refuted() {
    let out = judged(&[GOOD], Color::White, "g1f3", "f1d3", cp(50, 22), cp(-150, 22));
    assert_eq!(out, Outcome::Judged(vec![Verdict::Refuted { nag: GOOD, loss: Some(200) }]));
    // Up to the limit it stands: 1.5 pawns is not more than 1.5.
    assert_eq!(judged(&[GOOD], Color::White, "g1f3", "f1d3", cp(50, 22), cp(-100, 22)), Outcome::Judged(vec![]));
}

#[test]
fn a_good_move_that_is_the_best_stands() {
    // The best move loses nothing, whatever the two searches said of the positions.
    assert_eq!(judged(&[BRILLIANT], Color::Black, "e7e5", "e7e5", cp(30, 20), cp(-400, 20)), Outcome::Judged(vec![]));
}

#[test]
fn a_winning_sign_with_the_engine_at_minus_three_is_contradicted() {
    let out = judged(&[WHITE_WINS], Color::White, "a2a3", "a2a3", cp(-300, 25), cp(-300, 25));
    assert_eq!(out, Outcome::Judged(vec![Verdict::Contradicted { nag: WHITE_WINS, found: Band::BlackWins }]));
}

#[test]
fn an_equal_sign_at_a_fifth_of_a_pawn_stands() {
    assert_eq!(judged(&[EQUAL], Color::White, "a2a3", "a2a3", cp(20, 25), cp(20, 25)), Outcome::Judged(vec![]));
}

#[test]
fn a_poor_move_that_is_the_best_is_questioned() {
    let out = judged(&[POOR], Color::White, "a2a3", "a2a3", cp(10, 25), cp(10, 25));
    assert_eq!(out, Outcome::Judged(vec![Verdict::Questioned { nag: POOR, loss: 0 }]));
    // Losing a little (under 0.3) is questioned too; losing more is not.
    let out = judged(&[POOR], Color::White, "a2a3", "b2b3", cp(10, 25), cp(-15, 25));
    assert_eq!(out, Outcome::Judged(vec![Verdict::Questioned { nag: POOR, loss: 25 }]));
    assert_eq!(judged(&[POOR], Color::White, "a2a3", "b2b3", cp(10, 25), cp(-40, 25)), Outcome::Judged(vec![]));
}

#[test]
fn a_move_without_a_sign_is_not_judged() {
    assert_eq!(judged(&[], Color::White, "a2a3", "b2b3", cp(10, 25), cp(-900, 25)), Outcome::Unsigned);
    // Nor is an opinion (`!?`, `?!`), the unclear sign or compensation.
    assert_eq!(judged(&[5, 6, 13, 44], Color::White, "a2a3", "b2b3", cp(10, 25), cp(-900, 25)), Outcome::Unsigned);
}

#[test]
fn a_mate_against_an_equal_sign_contradicts_it() {
    let out = judged(&[EQUAL], Color::White, "d1h5", "d1h5", Eval::new(Value::Mates(2), 30), Eval::new(Value::Mates(2), 30));
    assert_eq!(out, Outcome::Judged(vec![Verdict::Contradicted { nag: EQUAL, found: Band::WhiteWins }]));
    // Black mated: Black wins for White's bands.
    let out = judged(&[EQUAL], Color::Black, "d8h4", "d8h4", Eval::new(Value::Mates(1), 30), Eval::new(Value::Mates(1), 30));
    assert_eq!(out, Outcome::Judged(vec![Verdict::Contradicted { nag: EQUAL, found: Band::BlackWins }]));
}

#[test]
fn a_shallow_evaluation_judges_nothing() {
    assert_eq!(judged(&[GOOD], Color::White, "g1f3", "f1d3", cp(50, 17), cp(-150, 22)), Outcome::Shallow { depth: 17 });
    assert_eq!(judged(&[EQUAL], Color::White, "g1f3", "f1d3", cp(50, 30), cp(500, 12)), Outcome::Shallow { depth: 12 });
    // An evaluation it needs is missing.
    let m = Judged { nags: &[GOOD], mover: Color::White, played: "g1f3", before: None, after: Some(cp(0, 30)) };
    assert_eq!(judge(&m, &Limits::default()), Outcome::NoEval);
    // The best move needs no position after it; an evaluation sign does.
    let m = Judged { nags: &[GOOD], mover: Color::White, played: "g1f3", before: Some((cp(0, 30), "g1f3")), after: None };
    assert_eq!(judge(&m, &Limits::default()), Outcome::Judged(vec![]));
    let m = Judged { nags: &[GOOD, EQUAL], mover: Color::White, played: "g1f3", before: Some((cp(0, 30), "g1f3")), after: None };
    assert_eq!(judge(&m, &Limits::default()), Outcome::NoEval);
    // A position with no move is known exactly.
    assert_eq!(judged(&[EQUAL], Color::White, "a2a3", "a2a3", cp(0, 30), Eval::new(Value::Cp(0), Eval::EXACT)), Outcome::Judged(vec![]));
}

#[test]
fn a_mate_is_beyond_every_limit() {
    // A `!` that lets a mate in 2 go, keeping a rook, is refuted.
    let out = judged(&[GOOD], Color::White, "a1a8", "d1h5", Eval::new(Value::Mates(2), 30), cp(500, 30));
    assert_eq!(out, Outcome::Judged(vec![Verdict::Refuted { nag: GOOD, loss: None }]));
    // One that walks into a mate, too.
    let out = judged(&[GOOD], Color::White, "a1a8", "d1h5", cp(0, 30), Eval::new(Value::Mated(3), 30));
    assert_eq!(out, Outcome::Judged(vec![Verdict::Refuted { nag: GOOD, loss: None }]));
    // A slower mate still mates: it loses no pawns.
    assert_eq!(
        judged(&[GOOD], Color::White, "a1a8", "d1h5", Eval::new(Value::Mates(2), 30), Eval::new(Value::Mates(5), 30)),
        Outcome::Judged(vec![])
    );
}

/// Plan A9-N17a2: mates are ordered by distance. A `??` that is mated sooner than the best move
/// allows is not the engine's best move; one as quick is.
#[test]
fn a_poor_move_that_hastens_a_mate_is_not_the_best() {
    let mated = |n| Eval::new(Value::Mated(n), 30);
    assert_eq!(judged(&[4], Color::White, "a2a3", "h2h3", mated(5), mated(4)), Outcome::Judged(vec![]));
    assert_eq!(judged(&[4], Color::White, "a2a3", "h2h3", mated(5), mated(5)), Outcome::Judged(vec![Verdict::Questioned { nag: 4, loss: 0 }]));
    // A `?` that mates later than the best move: not the best either; a `!` that does still mates.
    let mates = |n| Eval::new(Value::Mates(n), 30);
    assert_eq!(judged(&[POOR], Color::White, "a2a3", "h2h3", mates(2), mates(6)), Outcome::Judged(vec![]));
    assert_eq!(judged(&[GOOD], Color::White, "a2a3", "h2h3", mates(2), mates(6)), Outcome::Judged(vec![]));
}

#[test]
fn signs_are_for_white_whoever_moved() {
    // Black moved and stands at +2 for itself: −2 for White, which `−+` says and `+−` does not.
    let after = cp(200, 25);
    assert_eq!(judged(&[BLACK_WINS], Color::Black, "e7e5", "e7e5", after, after), Outcome::Judged(vec![]));
    let out = judged(&[WHITE_WINS], Color::Black, "e7e5", "e7e5", after, after);
    assert_eq!(out, Outcome::Judged(vec![Verdict::Contradicted { nag: WHITE_WINS, found: Band::BlackWins }]));
}

#[test]
fn a_move_with_two_signs_has_a_verdict_for_each() {
    let out = judged(&[GOOD, WHITE_WINS], Color::White, "g1f3", "f1d3", cp(150, 22), cp(-50, 22));
    assert_eq!(
        out,
        Outcome::Judged(vec![Verdict::Refuted { nag: GOOD, loss: Some(200) }, Verdict::Contradicted { nag: WHITE_WINS, found: Band::BlackSlightly }])
    );
}

/// Decision D29: a `+−` is contradicted below +0.70, and stands from it.
#[test]
fn a_winning_sign_stands_from_seventy_centipawns() {
    let at = |c: i32| judged(&[WHITE_WINS], Color::White, "a2a3", "a2a3", cp(c, 18), cp(c, 18));
    assert_eq!(at(69), Outcome::Judged(vec![Verdict::Contradicted { nag: WHITE_WINS, found: Band::WhiteSlightly }]));
    assert_eq!(at(70), Outcome::Judged(vec![]));
}

#[test]
fn the_limits_are_the_callers() {
    let strict = Limits { refuted: 50, ..Limits::default() };
    let m = Judged { nags: &[GOOD], mover: Color::White, played: "g1f3", before: Some((cp(50, 22), "f1d3")), after: Some(cp(-50, 22)) };
    assert_eq!(judge(&m, &Limits::default()), Outcome::Judged(vec![]));
    assert_eq!(judge(&m, &strict), Outcome::Judged(vec![Verdict::Refuted { nag: GOOD, loss: Some(100) }]));
    assert!(Limits::default().check().is_ok());
    assert!(Limits { bands: [70, 30, 150], ..Limits::default() }.check().is_err());
    assert!(Limits { refuted: -1, ..Limits::default() }.check().is_err());
}

#[test]
fn a_goal_is_held_to_its_sign() {
    let limits = Limits::default();
    // "White to play and win" with the engine at −3: contradicted.
    let out = judge_goal(Goal::Win, Color::White, Some(cp(-300, 20)), &limits);
    assert_eq!(out, Outcome::Judged(vec![Verdict::Goal { goal: Goal::Win, found: Band::BlackWins }]));
    assert_eq!(judge_goal(Goal::Win, Color::White, Some(cp(90, 20)), &limits), Outcome::Judged(vec![]));
    // Black to play and win, Black at +3 for itself.
    assert_eq!(judge_goal(Goal::Win, Color::Black, Some(cp(300, 20)), &limits), Outcome::Judged(vec![]));
    // A draw at +2 is contradicted; at +0.5 it stands.
    assert!(matches!(judge_goal(Goal::Draw, Color::White, Some(cp(200, 20)), &limits), Outcome::Judged(v) if v.len() == 1));
    assert_eq!(judge_goal(Goal::Draw, Color::White, Some(cp(50, 20)), &limits), Outcome::Judged(vec![]));
    // A mate in 5 needs a mate found within 5.
    assert_eq!(judge_goal(Goal::Mate(5), Color::White, Some(Eval::new(Value::Mates(4), 30)), &limits), Outcome::Judged(vec![]));
    assert!(matches!(judge_goal(Goal::Mate(5), Color::White, Some(cp(900, 30)), &limits), Outcome::Judged(v) if v.len() == 1));
    assert!(matches!(judge_goal(Goal::Mate(2), Color::White, Some(Eval::new(Value::Mates(3), 30)), &limits), Outcome::Judged(v) if v.len() == 1));
    // Help- and selfmates are not the engine's to judge; a shallow or missing evaluation says so.
    assert_eq!(judge_goal(Goal::HelpMate(2), Color::White, Some(cp(-900, 30)), &limits), Outcome::Unsigned);
    assert_eq!(judge_goal(Goal::Win, Color::White, Some(cp(-300, 10)), &limits), Outcome::Shallow { depth: 10 });
    assert_eq!(judge_goal(Goal::Win, Color::White, None, &limits), Outcome::NoEval);
}

#[test]
fn another_winning_line_is_another_solution() {
    let limits = Limits::default();
    assert!(other_solution(Value::Cp(500), Value::Cp(300), &limits));
    assert!(other_solution(Value::Mates(3), Value::Cp(160), &limits));
    assert!(!other_solution(Value::Mates(3), Value::Cp(40), &limits));
    assert!(!other_solution(Value::Cp(100), Value::Cp(100), &limits), "the best line does not win");
}

proptest! {
    /// The bands are the same for both sides: a score and its opposite fall in opposite bands.
    #[test]
    fn the_bands_mirror(c in any::<i32>()) {
        let limits = Limits::default();
        prop_assert_eq!(limits.band(Value::Cp(c)).step(), -limits.band(Value::Cp(c).other()).step());
    }

    /// Any score, any limits: judging never panics, and a refutation's loss is beyond the limit.
    #[test]
    fn judging_is_total(b in any::<i32>(), a in any::<i32>(), r in any::<i32>(), q in any::<i32>(), d in any::<u32>()) {
        let limits = Limits { refuted: r, questioned: q, bands: [r, q, d as i32], min_depth: d };
        let m = Judged { nags: &[1, 2, 10, 18], mover: Color::Black, played: "a7a6", before: Some((cp(b, d), "b7b6")), after: Some(cp(a, d)) };
        if let Outcome::Judged(v) = judge(&m, &limits) {
            for verdict in v {
                if let Verdict::Refuted { loss: Some(l), .. } = verdict {
                    prop_assert!(l > r);
                }
            }
        }
    }
}
