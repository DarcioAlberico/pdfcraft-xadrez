//! Decoding a board subject to the rules of chess.
//!
//! The per-square argmax decides 64 times independently, so nothing stops it from producing two
//! white kings or a pawn on the eighth rank. In a German figurine font the king and queen are
//! confused systematically, and the argmax comes out with no white king.
//!
//! The information to repair that is already in the matrix; the argmax throws it away. Decoding
//! is therefore a search: the most probable assignment among those that obey the rules. Since
//! the argmax is the unconstrained optimum, the answer is "the argmax with the fewest, cheapest
//! changes". It is a uniform-cost search over square changes, where the cost of an assignment is
//! its loss of log-probability against the argmax. That cost is never negative and does not
//! depend on the path, so the first legal assignment taken from the queue is the most probable
//! one in the space searched.
//!
//! The rules are those checkable without the side to move, which a book diagram does not carry:
//! one king per side, at most 8 pawns and 16 pieces per side, no pawn on a back rank, no more
//! promoted pieces (bishops counted per square colour) than missing pawns, no adjacent kings.
//!
//! [`Limits::max_changes`] caps how many squares a repair may rewrite. A reading that needs more
//! is too wrong: repairing it would invent a legal position, which is worse than admitting the
//! failure. The search is bounded by a count of expansions and of states, not by a clock, so it
//! gives the same answer on every machine and in WebAssembly.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashSet};

use super::{CLASS_COUNT, EMPTY, Probabilities, class_of, piece_of};
use crate::placement::{Color, Piece, PieceKind, Square};

/// Which optional rules the decoder applies, so their effect can be measured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rules {
    /// Two bishops of one side on squares of one colour count as a promotion.
    pub bishop_colors: bool,
    /// Kings that touch are a violation.
    pub adjacent_kings: bool,
}

impl Default for Rules {
    fn default() -> Self {
        Rules { bishop_colors: true, adjacent_kings: true }
    }
}

/// Bounds on the search.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Most squares a repair may rewrite.
    pub max_changes: usize,
    /// Most states taken from the queue.
    pub max_expansions: usize,
    /// Most states ever queued, which bounds memory (64 bytes each, plus the set).
    pub max_states: usize,
    /// How many alternative classes are tried per square, most probable first. Empty is always
    /// tried too: it is the right repair when a square is noise from the frame or the caption.
    pub alternatives_per_square: usize,
    pub rules: Rules,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { max_changes: 6, max_expansions: 5_000, max_states: 100_000, alternatives_per_square: 4, rules: Rules::default() }
    }
}

/// A rule an assignment breaks, for reporting.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Problem {
    NoKing(Color),
    TooManyKings(Color),
    PawnOnBackRank,
    TooManyPawns(Color),
    TooManyPieces(Color),
    TooManyPromotions(Color),
    AdjacentKings,
}

/// One square change the decoder made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Change {
    pub square: Square,
    pub from: Option<Piece>,
    pub to: Option<Piece>,
}

/// What [`decode`] returns.
#[derive(Clone, Debug, PartialEq)]
pub struct Decoded {
    /// The chosen class of every square, in reading order.
    pub cells: [u8; 64],
    /// Its log-probability; never above the argmax's.
    pub log_prob: f64,
    /// The squares changed from the argmax.
    pub changes: Vec<Change>,
    /// `false` when the search ran out of budget or changes without a legal assignment. `cells`
    /// is then the best partial state: fewest problems, then cheapest.
    pub satisfied: bool,
    /// What is still broken when `satisfied` is false.
    pub remaining: Vec<Problem>,
    /// States taken from the queue.
    pub expansions: usize,
}

/// A broken rule and the squares a fix must change.
struct Violation {
    problem: Problem,
    /// Branching over these is complete: every legal assignment changes at least one of them.
    squares: Vec<usize>,
    /// Fixed target class, or `None` for "the square's other plausible classes".
    target: Option<u8>,
}

fn is_light(reading_index: usize) -> bool {
    // a8 (index 0) is light, and the colour alternates.
    (reading_index / 8 + reading_index % 8).is_multiple_of(2)
}

fn violations(state: &[u8; 64], rules: Rules) -> Vec<Violation> {
    let mut v = Vec::new();
    let class = |color, kind| class_of(Some(Piece::new(color, kind)));
    let squares_of = |c: u8| (0..64).filter(|&i| state.get(i) == Some(&c)).collect::<Vec<_>>();
    let mut counts = [0usize; CLASS_COUNT];
    for &c in state {
        if let Some(n) = counts.get_mut(usize::from(c)) {
            *n += 1;
        }
    }
    let count = |c: u8| counts.get(usize::from(c)).copied().unwrap_or(0);

    for color in [Color::White, Color::Black] {
        let king = class(color, PieceKind::King);
        match count(king) {
            0 => v.push(Violation {
                problem: Problem::NoKing(color),
                squares: (0..64).filter(|&i| state.get(i) != Some(&king)).collect(),
                target: Some(king),
            }),
            1 => {}
            _ => v.push(Violation { problem: Problem::TooManyKings(color), squares: squares_of(king), target: None }),
        }
    }

    let white_pawn = class(Color::White, PieceKind::Pawn);
    let black_pawn = class(Color::Black, PieceKind::Pawn);
    let back_rank: Vec<usize> = (0..8).chain(56..64).filter(|&i| state.get(i).is_some_and(|c| *c == white_pawn || *c == black_pawn)).collect();
    if !back_rank.is_empty() {
        v.push(Violation { problem: Problem::PawnOnBackRank, squares: back_rank, target: None });
    }

    for color in [Color::White, Color::Black] {
        let pawn = class(color, PieceKind::Pawn);
        if count(pawn) > 8 {
            v.push(Violation { problem: Problem::TooManyPawns(color), squares: squares_of(pawn), target: None });
        }
    }

    for color in [Color::White, Color::Black] {
        let own: Vec<usize> = (0..64).filter(|&i| state.get(i).and_then(|c| piece_of(*c)).is_some_and(|p| p.color == color)).collect();
        if own.len() > 16 {
            v.push(Violation { problem: Problem::TooManyPieces(color), squares: own, target: None });
            continue;
        }
        let extra = |kind, initial: usize| count(class(color, kind)).saturating_sub(initial);
        let mut promoted = extra(PieceKind::Knight, 2) + extra(PieceKind::Bishop, 2) + extra(PieceKind::Rook, 2) + extra(PieceKind::Queen, 1);
        if rules.bishop_colors {
            // Bishops of one square colour beyond the first were promoted too; count only what
            // the plain bishop count above has not already counted.
            let bishop = class(color, PieceKind::Bishop);
            let light = (0..64).filter(|&i| state.get(i) == Some(&bishop) && is_light(i)).count();
            let dark = (0..64).filter(|&i| state.get(i) == Some(&bishop) && !is_light(i)).count();
            let by_colour = light.saturating_sub(1) + dark.saturating_sub(1);
            let by_count = (light + dark).saturating_sub(2);
            promoted += by_colour.saturating_sub(by_count);
        }
        if count(class(color, PieceKind::Pawn)) + promoted > 8 {
            v.push(Violation { problem: Problem::TooManyPromotions(color), squares: own, target: None });
        }
    }

    if rules.adjacent_kings {
        let whites = squares_of(class(Color::White, PieceKind::King));
        let blacks = squares_of(class(Color::Black, PieceKind::King));
        let mut touching: Vec<usize> = Vec::new();
        for &w in &whites {
            for &b in &blacks {
                if (w / 8).abs_diff(b / 8) <= 1 && (w % 8).abs_diff(b % 8) <= 1 {
                    touching.extend([w, b]);
                }
            }
        }
        touching.sort_unstable();
        touching.dedup();
        if !touching.is_empty() {
            v.push(Violation { problem: Problem::AdjacentKings, squares: touching, target: None });
        }
    }
    v
}

/// Log-probabilities, floored so an impossible class costs a lot instead of infinity.
fn log_probs(probs: &Probabilities) -> [[f64; CLASS_COUNT]; 64] {
    let mut out = [[0.0f64; CLASS_COUNT]; 64];
    for (square, row) in out.iter_mut().enumerate() {
        for (class, cell) in (0u8..).zip(row.iter_mut()) {
            *cell = f64::from(probs.get(square, class)).max(1e-12).ln();
        }
    }
    out
}

fn logp(lp: &[[f64; CLASS_COUNT]; 64], square: usize, class: u8) -> f64 {
    lp.get(square).and_then(|r| r.get(usize::from(class))).copied().unwrap_or(f64::NEG_INFINITY)
}

/// The classes worth trying on `square` instead of `current`, most probable first.
fn alternatives(lp: &[[f64; CLASS_COUNT]; 64], square: usize, current: u8, limit: usize) -> Vec<u8> {
    let mut order: Vec<u8> = (0..CLASS_COUNT as u8).filter(|&c| c != current).collect();
    order.sort_by(|&a, &b| logp(lp, square, b).total_cmp(&logp(lp, square, a)));
    order.truncate(limit);
    if current != EMPTY && !order.contains(&EMPTY) {
        order.push(EMPTY);
    }
    order
}

/// A queued state. Ordered so that `BinaryHeap` pops the cheapest, oldest first.
struct Node {
    cost: f64,
    seq: u64,
    state: [u8; 64],
}

impl PartialEq for Node {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for Node {}
impl PartialOrd for Node {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Node {
    fn cmp(&self, other: &Self) -> Ordering {
        other.cost.total_cmp(&self.cost).then_with(|| other.seq.cmp(&self.seq))
    }
}

/// The most probable assignment that obeys the rules, within `limits`.
pub fn decode(probs: &Probabilities, limits: &Limits) -> Decoded {
    let lp = log_probs(probs);
    let base = probs.argmax();
    let finish = |state: [u8; 64], satisfied: bool, expansions: usize| {
        let changes = (0..64)
            .filter_map(|i| {
                let (from, to) = (*base.get(i)?, *state.get(i)?);
                if from == to {
                    return None;
                }
                Some(Change { square: Square::from_reading_index(i)?, from: piece_of(from), to: piece_of(to) })
            })
            .collect();
        let remaining = if satisfied { Vec::new() } else { violations(&state, limits.rules).into_iter().map(|v| v.problem).collect() };
        let log_prob = (0..64).map(|i| logp(&lp, i, state.get(i).copied().unwrap_or(EMPTY))).sum();
        Decoded { cells: state, log_prob, changes, satisfied, remaining, expansions }
    };

    let first = violations(&base, limits.rules);
    if first.is_empty() {
        return finish(base, true, 0);
    }

    let mut heap = BinaryHeap::new();
    let mut seen: HashSet<[u8; 64]> = HashSet::new();
    heap.push(Node { cost: 0.0, seq: 0, state: base });
    seen.insert(base);
    let mut seq = 0u64;
    let mut expansions = 0usize;
    let mut best = (first.len(), 0.0f64, base);

    while let Some(Node { cost, state, .. }) = heap.pop() {
        if expansions >= limits.max_expansions {
            break;
        }
        expansions += 1;
        let found = violations(&state, limits.rules);
        if found.is_empty() {
            return finish(state, true, expansions);
        }
        if (found.len(), cost) < (best.0, best.1) {
            best = (found.len(), cost, state);
        }
        let changed = state.iter().zip(&base).filter(|(a, b)| a != b).count();
        if changed >= limits.max_changes {
            continue;
        }
        // Branch on the most constrained violation: every solution must fix all of them, so
        // fixing the one with fewest squares first keeps the branching small without losing any.
        let Some(target) = found.iter().min_by_key(|v| v.squares.len()) else { continue };
        for &square in &target.squares {
            let Some(&current) = state.get(square) else { continue };
            let choices = match target.target {
                Some(t) if t != current => vec![t],
                Some(_) => Vec::new(),
                None => alternatives(&lp, square, current, limits.alternatives_per_square),
            };
            for class in choices {
                if seen.len() >= limits.max_states {
                    break;
                }
                let mut next = state;
                if let Some(cell) = next.get_mut(square) {
                    *cell = class;
                }
                if !seen.insert(next) {
                    continue;
                }
                seq += 1;
                let delta = logp(&lp, square, current) - logp(&lp, square, class);
                heap.push(Node { cost: cost + delta, seq, state: next });
            }
        }
    }
    finish(best.2, false, expansions)
}

#[cfg(test)]
mod tests {
    use super::super::testing::*;
    use super::*;
    use crate::fen::format_placement;
    use crate::recognize::placement_of;

    fn run(v: &[f32]) -> Decoded {
        decode(&Probabilities::new(v).unwrap(), &Limits::default())
    }

    fn fen(d: &Decoded) -> String {
        format_placement(&placement_of(&d.cells))
    }

    #[test]
    fn a_legal_argmax_is_returned_untouched() {
        let d = run(&matrix("r3k3/8/8/8/8/8/8/4K2R", 0.95));
        assert!(d.satisfied);
        assert!(d.changes.is_empty());
        assert_eq!(d.expansions, 0);
        assert_eq!(fen(&d), "r3k3/8/8/8/8/8/8/4K2R");
    }

    #[test]
    fn a_king_read_as_a_queen_is_restored() {
        let mut v = matrix("8/8/8/4k3/8/8/8/4K3", 0.99);
        set(&mut v, "e1", piece('Q'), 0.6, piece('K'), 0.35);
        let d = run(&v);
        assert!(d.satisfied);
        assert_eq!(fen(&d), "8/8/8/4k3/8/8/8/4K3");
        assert_eq!(d.changes, vec![Change { square: Square::parse("e1").unwrap(), from: piece('Q'), to: piece('K') }]);
    }

    #[test]
    fn the_cheaper_of_two_kings_is_changed() {
        let mut v = matrix("8/8/8/4k3/8/8/8/4K3", 0.99);
        // A second white king on a4 that is likely an empty square.
        set(&mut v, "a4", piece('K'), 0.5, None, 0.45);
        let d = run(&v);
        assert!(d.satisfied);
        assert_eq!(fen(&d), "8/8/8/4k3/8/8/8/4K3");
    }

    #[test]
    fn a_pawn_on_the_back_rank_goes() {
        let mut v = matrix("8/8/8/4k3/8/8/8/4K3", 0.99);
        set(&mut v, "a8", piece('P'), 0.6, piece('B'), 0.3);
        let d = run(&v);
        assert!(d.satisfied);
        assert_eq!(fen(&d), "B7/8/8/4k3/8/8/8/4K3");
    }

    #[test]
    fn bishop_colours_count_as_promotions() {
        // Eight white pawns and two white bishops on dark squares (c1, e3): one must go. The
        // e3 bishop is the less certain one and its second reading is empty.
        let mut v = matrix("4k3/pppppppp/8/8/8/4B3/PPPPPPPP/2B1K3", 0.99);
        set(&mut v, "e3", piece('B'), 0.55, None, 0.4);
        let d = run(&v);
        assert!(d.satisfied);
        assert_eq!(fen(&d), "4k3/pppppppp/8/8/8/8/PPPPPPPP/2B1K3");
        let classic =
            decode(&Probabilities::new(&v).unwrap(), &Limits { rules: Rules { bishop_colors: false, adjacent_kings: true }, ..Limits::default() });
        assert!(classic.changes.is_empty(), "without the rule the reading is left alone");
    }

    #[test]
    fn adjacent_kings_are_separated() {
        let mut v = matrix("8/8/8/3kK3/8/8/8/8", 0.99);
        // The black king on d5 is likely empty, and c6 likely holds it instead.
        set(&mut v, "d5", piece('k'), 0.6, None, 0.39);
        set(&mut v, "c6", None, 0.55, piece('k'), 0.44);
        let d = run(&v);
        assert!(d.satisfied, "{:?}", d.remaining);
        assert_eq!(fen(&d), "8/8/2k5/4K3/8/8/8/8");
    }

    #[test]
    fn a_hopeless_reading_is_not_invented() {
        // Ten white pawns on the back ranks: more changes than allowed.
        let v = matrix("PPPPP3/8/8/4k3/8/8/8/PPPPPK2", 0.99);
        let d = run(&v);
        assert!(!d.satisfied);
        assert!(d.changes.len() <= Limits::default().max_changes);
        assert!(d.remaining.contains(&Problem::PawnOnBackRank));
    }

    #[test]
    fn the_budget_bounds_the_work() {
        let v = matrix("PPPPP3/8/8/4k3/8/8/8/PPPPPK2", 0.99);
        let tight = Limits { max_expansions: 10, ..Limits::default() };
        let d = decode(&Probabilities::new(&v).unwrap(), &tight);
        assert!(d.expansions <= 10);
        let tiny = Limits { max_states: 5, ..Limits::default() };
        assert!(!decode(&Probabilities::new(&v).unwrap(), &tiny).satisfied);
    }

    #[test]
    fn decoding_is_deterministic() {
        let mut v = matrix("8/8/8/4k3/8/8/8/4K3", 0.99);
        set(&mut v, "e1", piece('Q'), 0.5, piece('K'), 0.45);
        set(&mut v, "a4", piece('K'), 0.5, None, 0.45);
        assert_eq!(run(&v), run(&v));
    }
}
