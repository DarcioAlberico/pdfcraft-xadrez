//! The book says × the engine says (plan A9-N17a, finding A8-10): a printed sign set against an
//! engine's evaluation, by limits the caller gives.
//!
//! Three things are judged, each only by what is exact about it:
//!
//! - **A move sign.** A `!` or `!!` that loses more than [`Limits::refuted`] to the engine's best
//!   move is *refuted*; a `?` or `??` that is the best move, or loses less than
//!   [`Limits::questioned`], is *questioned*. `!?`, `?!` and the others say an opinion, not a loss,
//!   and are not judged.
//! - **An evaluation sign** (`=`, `⩲`, `±`, `+−` and Black's): the evaluation after the move falls
//!   in one of seven bands ([`Band`], edges [`Limits::bands`]); the sign is *contradicted* when its
//!   band is two or more away from the evaluation's. One band is a matter of taste (a `±` the
//!   engine calls `+−`); two are not: with the default edges a `+−` is contradicted below +0.70,
//!   as decision D29 asks. `∞` and compensation are not judged.
//! - **A goal a caption prints** (`White to play and win`, `and draw`, `mate in 5`): a win as a
//!   `+−` for the side to play, a draw as a `=`, a mate as a mate found within the moves asked.
//!   Help- and selfmates are not judged.
//!
//! A mate is beyond every limit: a `!` that lets a mate go is refuted however much it keeps, and a
//! mate against a `=` contradicts it. An evaluation shallower than [`Limits::min_depth`] judges
//! nothing and is said ([`Outcome::Shallow`]). The limits are data, never hidden constants; the
//! defaults ([`Limits::default`]) are decision D29's.

use crate::placement::Color;
use crate::stipulation::Goal;

/// A score far above any centipawn score: a mate's rank is measured from it.
const MATE: i64 = 1_000_000;

/// Largest centipawn score taken as it is; beyond it, a score is held at this (a mate is more).
const MAX_CP: i64 = MATE / 2;

/// An engine's score for one side.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Value {
    /// Hundredths of a pawn, for the side.
    Cp(i32),
    /// The side mates in this many moves (0: it has mated, the other side is mated now).
    Mates(u32),
    /// The side is mated in this many moves (0: it is mated now).
    Mated(u32),
}

impl Value {
    /// The same score for the other side.
    pub fn other(self) -> Value {
        match self {
            Value::Cp(cp) => Value::Cp(cp.saturating_neg()),
            Value::Mates(n) => Value::Mated(n),
            Value::Mated(n) => Value::Mates(n),
        }
    }

    /// The score as a number that orders every score: a quicker mate is more, a later mate
    /// against is less bad.
    fn rank(self) -> i64 {
        match self {
            Value::Cp(cp) => i64::from(cp).clamp(-MAX_CP, MAX_CP),
            Value::Mates(n) => MATE - i64::from(n),
            Value::Mated(n) => -MATE + i64::from(n),
        }
    }

    /// Whether it is a mate, for or against.
    pub fn is_mate(self) -> bool {
        !matches!(self, Value::Cp(_))
    }
}

/// An evaluation: a score and how deep the search went for it. A position with no move (mated,
/// stalemated) is known exactly: give it [`Eval::EXACT`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Eval {
    pub value: Value,
    pub depth: u32,
}

impl Eval {
    /// The depth of a score known without a search (a position with no legal move).
    pub const EXACT: u32 = u32::MAX;

    pub fn new(value: Value, depth: u32) -> Eval {
        Eval { value, depth }
    }

    /// The same evaluation for the other side.
    pub fn other(self) -> Eval {
        Eval { value: self.value.other(), depth: self.depth }
    }
}

/// What [`judge`] holds a sign to (plan A9-N17a). Every field may be changed by the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// A `!`/`!!` that loses more than this many centipawns to the best move is refuted.
    pub refuted: i32,
    /// A `?`/`??` that loses less than this many centipawns is questioned (the best move loses 0).
    pub questioned: i32,
    /// The bands' edges, in centipawns, ascending: within the first either way is equal, up to the
    /// second slightly better, up to the third clearly better, from it winning.
    pub bands: [i32; 3],
    /// An evaluation shallower than this judges nothing.
    pub min_depth: u32,
}

impl Default for Limits {
    /// Decision D29 (2026-11-15): a `!` refuted past 1.5 pawns lost to the best move, a `?`
    /// questioned below 0.3, a `+−` contradicted below +0.70, depth at least 18. The bands are
    /// 0.30, 0.70 and 1.50: the `+−`'s two bands away start at 0.70.
    fn default() -> Limits {
        Limits { refuted: 150, questioned: 30, bands: [30, 70, 150], min_depth: 18 }
    }
}

impl Limits {
    /// Whether the limits can judge: non-negative losses and ascending, non-negative edges.
    pub fn check(&self) -> Result<(), &'static str> {
        if self.refuted < 0 || self.questioned < 0 {
            return Err("a loss limit is negative");
        }
        let [a, b, c] = self.bands;
        if a < 0 || a > b || b > c {
            return Err("the bands' edges are not ascending from 0");
        }
        Ok(())
    }

    /// The band `value` falls in, `value` being for White. A mate is a win, whatever the edges.
    pub fn band(&self, value: Value) -> Band {
        let r = match value {
            Value::Cp(cp) => i64::from(cp),
            Value::Mates(_) => return Band::WhiteWins,
            Value::Mated(_) => return Band::BlackWins,
        };
        let size = r.unsigned_abs();
        let [a, b, c] = self.bands.map(|e| u64::from(e.unsigned_abs()));
        let step: i8 = if size < a {
            0
        } else if size < b {
            1
        } else if size < c {
            2
        } else {
            3
        };
        Band::from_step(if r < 0 { -step } else { step })
    }
}

/// Who stands better, in seven steps (for White first).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Band {
    BlackWins,
    BlackClearly,
    BlackSlightly,
    Equal,
    WhiteSlightly,
    WhiteClearly,
    WhiteWins,
}

impl Band {
    /// −3 (Black wins) to 3 (White wins).
    pub fn step(self) -> i8 {
        match self {
            Band::BlackWins => -3,
            Band::BlackClearly => -2,
            Band::BlackSlightly => -1,
            Band::Equal => 0,
            Band::WhiteSlightly => 1,
            Band::WhiteClearly => 2,
            Band::WhiteWins => 3,
        }
    }

    fn from_step(step: i8) -> Band {
        match step {
            i8::MIN..=-3 => Band::BlackWins,
            -2 => Band::BlackClearly,
            -1 => Band::BlackSlightly,
            0 => Band::Equal,
            1 => Band::WhiteSlightly,
            2 => Band::WhiteClearly,
            _ => Band::WhiteWins,
        }
    }

    /// The band's name in the tools' replies.
    pub fn name(self) -> &'static str {
        match self {
            Band::BlackWins => "black_wins",
            Band::BlackClearly => "black_clearly_better",
            Band::BlackSlightly => "black_slightly_better",
            Band::Equal => "equal",
            Band::WhiteSlightly => "white_slightly_better",
            Band::WhiteClearly => "white_clearly_better",
            Band::WhiteWins => "white_wins",
        }
    }

    /// The band as the sign that says it (`+−`, `=`).
    pub fn sign(self) -> &'static str {
        match self {
            Band::BlackWins => "−+",
            Band::BlackClearly => "∓",
            Band::BlackSlightly => "⩱",
            Band::Equal => "=",
            Band::WhiteSlightly => "⩲",
            Band::WhiteClearly => "±",
            Band::WhiteWins => "+−",
        }
    }
}

/// What a move sign claims.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Claim {
    /// `!`, `!!`.
    Good,
    /// `?`, `??`.
    Bad,
}

fn move_claim(nag: u16) -> Option<Claim> {
    match nag {
        1 | 3 => Some(Claim::Good),
        2 | 4 => Some(Claim::Bad),
        _ => None,
    }
}

/// The band an evaluation sign claims; `None` for a sign that is no evaluation, or one that is not
/// judged (`∞`, compensation).
pub fn sign_band(nag: u16) -> Option<Band> {
    match nag {
        10..=12 => Some(Band::Equal),
        14 => Some(Band::WhiteSlightly),
        15 => Some(Band::BlackSlightly),
        16 => Some(Band::WhiteClearly),
        17 => Some(Band::BlackClearly),
        18 | 20 => Some(Band::WhiteWins),
        19 | 21 => Some(Band::BlackWins),
        _ => None,
    }
}

/// Whether [`judge`] judges `nag`: `!`, `!!`, `?`, `??` and the evaluation signs of [`sign_band`].
pub fn judged_sign(nag: u16) -> bool {
    move_claim(nag).is_some() || sign_band(nag).is_some()
}

/// A move to judge: its signs, who played it, and the engine's word on the positions around it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Judged<'a> {
    /// The signs printed on the move, as PGN codes.
    pub nags: &'a [u16],
    /// The side that played it.
    pub mover: Color,
    /// The move, in UCI.
    pub played: &'a str,
    /// The position before it: its evaluation for the mover (that of the engine's best move) and
    /// that best move, in UCI.
    pub before: Option<(Eval, &'a str)>,
    /// The position after it, for the mover.
    pub after: Option<Eval>,
}

/// What the engine says against a sign.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// A `!` or `!!` (`nag`) that loses `loss` centipawns to the best move; `None`: it lets a mate
    /// go, or walks into one.
    Refuted { nag: u16, loss: Option<i32> },
    /// A `?` or `??` (`nag`) that loses only `loss` centipawns (0: the best move).
    Questioned { nag: u16, loss: i32 },
    /// An evaluation sign (`nag`) two bands or more from `found`, the band after the move.
    Contradicted { nag: u16, found: Band },
    /// A goal printed for the side to play that the evaluation does not bear out: `found` is the
    /// band for White.
    Goal { goal: Goal, found: Band },
}

/// What [`judge`] made of a move or a goal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing judged was printed.
    Unsigned,
    /// A sign is printed, and an evaluation it needs is missing.
    NoEval,
    /// An evaluation it needs is shallower than [`Limits::min_depth`]: the shallowest depth.
    Shallow { depth: u32 },
    /// Judged: what the engine says against the signs (none: it agrees).
    Judged(Vec<Verdict>),
}

/// What a move loses for the mover against the best move.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Loss {
    /// Centipawns (0: as good as the best, or better).
    Cp(i32),
    /// Both mate, or both are mated, and the move is this many moves worse: a slower mate, or a
    /// quicker one against (0: as quick).
    MateMoves(u32),
    /// A mate let go, or one walked into: beyond every limit.
    MateLost,
}

/// What a move leaving `after` loses against the best move, which keeps `best`.
fn loss(best: Value, after: Value) -> Loss {
    match (best, after) {
        (Value::Mates(_), Value::Cp(_) | Value::Mated(_)) | (Value::Cp(_), Value::Mated(_)) => Loss::MateLost,
        // Mates ordered by distance: mating later, or being mated sooner, is worse.
        (Value::Mates(b), Value::Mates(a)) => Loss::MateMoves(a.saturating_sub(b)),
        (Value::Mated(b), Value::Mated(a)) => Loss::MateMoves(b.saturating_sub(a)),
        // Centipawns both ways, or the move did better than the best (the searches differ).
        _ => Loss::Cp(i32::try_from(best.rank().saturating_sub(after.rank()).max(0)).unwrap_or(i32::MAX)),
    }
}

/// The engine's word against the signs printed on a move ([`Judged`]), by `limits`.
pub fn judge(m: &Judged<'_>, limits: &Limits) -> Outcome {
    let moves: Vec<(u16, Claim)> = m.nags.iter().filter_map(|&n| move_claim(n).map(|c| (n, c))).collect();
    let signs: Vec<(u16, Band)> = m.nags.iter().filter_map(|&n| sign_band(n).map(|b| (n, b))).collect();
    if moves.is_empty() && signs.is_empty() {
        return Outcome::Unsigned;
    }
    let best_played = m.before.is_some_and(|(_, best)| best == m.played);
    // What each part needs: the move signs the position before (and after, unless the move is the
    // best), the evaluation signs the position after.
    let mut needed: Vec<Option<Eval>> = Vec::new();
    if !moves.is_empty() {
        needed.push(m.before.map(|(e, _)| e));
        if !best_played {
            needed.push(m.after);
        }
    }
    if !signs.is_empty() {
        needed.push(m.after);
    }
    let mut depth = u32::MAX;
    for e in &needed {
        match e {
            None => return Outcome::NoEval,
            Some(e) => depth = depth.min(e.depth),
        }
    }
    if depth < limits.min_depth {
        return Outcome::Shallow { depth };
    }
    let mut verdicts = Vec::new();
    if let Some((before, _)) = m.before {
        // With no position after a move that is not the best, nothing (the evaluation is needed).
        let lost = if best_played { Some(Loss::Cp(0)) } else { m.after.map(|a| loss(before.value, a.value)) };
        for (nag, claim) in &moves {
            match (claim, lost) {
                (Claim::Good, Some(Loss::MateLost)) => verdicts.push(Verdict::Refuted { nag: *nag, loss: None }),
                (Claim::Good, Some(Loss::Cp(l))) if l > limits.refuted => verdicts.push(Verdict::Refuted { nag: *nag, loss: Some(l) }),
                // A slower mate still mates: no refutation, whatever the distance.
                (Claim::Bad, Some(Loss::Cp(l))) if l < limits.questioned => verdicts.push(Verdict::Questioned { nag: *nag, loss: l }),
                (Claim::Bad, Some(Loss::MateMoves(0))) => verdicts.push(Verdict::Questioned { nag: *nag, loss: 0 }),
                _ => {}
            }
        }
    }
    if let Some(after) = m.after {
        let white = if m.mover == Color::White { after.value } else { after.value.other() };
        let found = limits.band(white);
        for (nag, claimed) in &signs {
            if (i16::from(found.step()) - i16::from(claimed.step())).abs() >= 2 {
                verdicts.push(Verdict::Contradicted { nag: *nag, found });
            }
        }
    }
    Outcome::Judged(verdicts)
}

/// The engine's word against the goal a caption prints for the side to play (`side`), `eval` being
/// the position's for that side: a win is held to a `+−`, a draw to a `=`, a mate in `n` to a mate
/// found in `n` moves or fewer. Help- and selfmates are [`Outcome::Unsigned`]: not judged.
pub fn judge_goal(goal: Goal, side: Color, eval: Option<Eval>, limits: &Limits) -> Outcome {
    if matches!(goal, Goal::HelpMate(_) | Goal::SelfMate(_)) {
        return Outcome::Unsigned;
    }
    let Some(eval) = eval else { return Outcome::NoEval };
    if eval.depth < limits.min_depth {
        return Outcome::Shallow { depth: eval.depth };
    }
    let white = if side == Color::White { eval.value } else { eval.value.other() };
    let found = limits.band(white);
    let ours = if side == Color::White { found.step() } else { found.step().saturating_neg() };
    let holds = match goal {
        // As a `+−` for the side: two bands away is below the third band's start.
        Goal::Win => ours >= 2,
        Goal::Draw => ours.abs() < 2,
        Goal::Mate(n) => matches!(eval.value, Value::Mates(m) if m <= u32::from(n)),
        Goal::HelpMate(_) | Goal::SelfMate(_) => true,
    };
    Outcome::Judged(if holds { Vec::new() } else { vec![Verdict::Goal { goal, found }] })
}

/// Whether an exercise has another solution: its second line (`second`, for the side to play) wins
/// too, as a `+−` or a mate, besides the best one (`best`). Plan A9-N17a ("outra solução", A8-10).
pub fn other_solution(best: Value, second: Value, limits: &Limits) -> bool {
    let wins = |v: Value| limits.band(v).step() >= 3;
    wins(best) && wins(second)
}

#[cfg(test)]
mod tests;
