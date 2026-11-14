//! Mate in n, solved as chess problems are (plan A9-N4; A8-04, R6 O1).
//!
//! A problem book prints a diagram and a stipulation, `#2` or `Mate in 2`: the side to play
//! mates in at most that many moves against any defence. The first move that does so is the
//! **key**. [`solve`] finds every key by an exact search (no evaluation: a line is a mate or it is
//! not), which tells three things a reading alone cannot:
//!
//! - the reading is probably right when the position has exactly one key, as a sound problem
//!   does;
//! - more than one key (a *cook*) or none at all most often means a square was misread, and
//!   otherwise that the book erred; [`MateReport::best_try`] and [`MateReport::refutation`] (the
//!   first move that comes closest and the defence that refutes it) let a person tell which;
//! - who is to play, when the book does not say: [`mating_side`] answers only when exactly one
//!   side mates in n. When both do (every back-rank combination), it does not guess.
//!
//! **The problem conventions.** Castling is allowed unless the position proves otherwise: the
//! FEN's castling rights are taken as given, and a reading infers them from where the kings and
//! rooks stand ([`crate::recognize::side::infer_castling`]); a key that castles is said
//! ([`MateReport::castles`]). En passant only when the FEN gives its square, which a reading does
//! only when the caption proves the double step. A mate shorter than asked still solves the
//! problem ([`MateReport::shortest`] says it); a "dual" (two ways to go on after the key) is not
//! looked for.
//!
//! **The budget** is counted in positions played (nodes), never in time, so the answer is the
//! same on every machine and in WebAssembly, and a caller runs it in a job and bounds it. Out of
//! budget, the report says so ([`MateReport::exhausted`]) and claims nothing it has not proved.
//! Only up to [`MAX_MATE_MOVES`] moves.

use cozy_chess::{Board, GameStatus, Move};

use crate::fen::Fen;
use crate::placement::Color;
use crate::san::San;

/// Most moves a mate may be asked in: mate in 3 costs up to 40 times mate in 2.
pub const MAX_MATE_MOVES: u8 = 3;

/// The budget a reading spends on one side of one diagram: about 10 ms in a release build.
pub const DEFAULT_MATE_BUDGET: u64 = 200_000;

/// Why a position could not be solved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MateError {
    /// Not a FEN.
    Fen(String),
    /// The position is not legal with this side to move.
    Illegal(Color),
    /// Mate in this many moves is not solved here.
    Moves(u8),
}

impl std::fmt::Display for MateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MateError::Fen(why) => write!(f, "not a position: {why}"),
            MateError::Illegal(side) => write!(f, "the position is not legal with {} to move", if *side == Color::White { "White" } else { "Black" }),
            MateError::Moves(n) => write!(f, "mate in {n} is not solved here: 1 to {MAX_MATE_MOVES} moves"),
        }
    }
}

impl std::error::Error for MateError {}

/// What [`solve`] found.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MateReport {
    /// The moves asked for.
    pub n: u8,
    /// The side that plays the keys.
    pub side: Color,
    /// Every first move that mates in at most `n` against any defence, in SAN, in the order the
    /// moves were generated (none when out of budget before one was proved).
    pub keys: Vec<String>,
    /// The fewest moves a key mates in, when there is a key.
    pub shortest: Option<u8>,
    /// Positions played.
    pub nodes: u64,
    /// The budget ran out: there may be keys not found, and "no key" is not proved.
    pub exhausted: bool,
    /// When no key was found and the search was complete: the first move with the fewest
    /// defences that refute it, in SAN.
    pub best_try: Option<String>,
    /// The first defence that refutes [`MateReport::best_try`], in SAN.
    pub refutation: Option<String>,
    /// A key castles: it holds only if castling is allowed, which the problem convention
    /// supposes.
    pub castles: bool,
}

impl MateReport {
    /// Exactly one key, proved: a sound problem.
    pub fn unique(&self) -> bool {
        self.keys.len() == 1 && !self.exhausted
    }

    /// More than one key: a cook, most often a misread square.
    pub fn cooked(&self) -> bool {
        self.keys.len() > 1
    }

    /// No key, and the search was complete: no mate in `n`.
    pub fn no_mate(&self) -> bool {
        self.keys.is_empty() && !self.exhausted
    }
}

/// Every key that mates in at most `n` (1 to [`MAX_MATE_MOVES`]) for the side to move in `fen`
/// (White when the FEN does not say), within `budget` positions played.
pub fn solve(fen: &str, n: u8, budget: u64) -> Result<MateReport, MateError> {
    let fen = Fen::parse(fen).map_err(|e| MateError::Fen(e.to_string()))?;
    let side = fen.side().unwrap_or(Color::White);
    solve_side(&fen, side, n, budget)
}

/// [`solve`] with `side` to play, whatever the FEN says.
pub fn solve_side(fen: &Fen, side: Color, n: u8, budget: u64) -> Result<MateReport, MateError> {
    if !(1..=MAX_MATE_MOVES).contains(&n) {
        return Err(MateError::Moves(n));
    }
    let mut position = fen.position().clone();
    position.side_to_move = Some(side);
    let board = position.to_board(side).map_err(|_| MateError::Illegal(side))?;
    let mut search = Search { nodes: 0, budget };
    let mut report =
        MateReport { n, side, keys: Vec::new(), shortest: None, nodes: 0, exhausted: false, best_try: None, refutation: None, castles: false };
    for mv in moves(&board) {
        let Some(after) = search.play(&board, mv) else {
            report.exhausted = true;
            break;
        };
        match search.forced(&after, n - 1) {
            Some(true) => {
                if is_castle(&board, mv) {
                    report.castles = true;
                }
                // The fewest moves this key mates in.
                let mut fewest = n;
                for k in 1..n {
                    if search.forced(&after, k - 1) == Some(true) {
                        fewest = k;
                        break;
                    }
                }
                report.shortest = Some(report.shortest.map_or(fewest, |s| s.min(fewest)));
                report.keys.push(san(&board, mv));
            }
            Some(false) => {}
            None => {
                report.exhausted = true;
                break;
            }
        }
    }
    if report.keys.is_empty() && !report.exhausted {
        match search.best_try(&board, n) {
            Some(Some((try_move, refutation))) => {
                report.best_try = Some(san(&board, try_move));
                if let Some(after) = playing(&board, try_move) {
                    report.refutation = Some(san(&after, refutation));
                }
            }
            Some(None) => {}
            None => report.exhausted = true,
        }
    }
    report.nodes = search.nodes;
    Ok(report)
}

/// Who mates in `n`, when the book did not say who is to play.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MatingSide {
    /// Only this side mates in `n` (and the position is legal with it to move): its report.
    One(MateReport),
    /// Both do: the side stays as it was, assumed.
    Both,
    /// Neither does (with the side the position allows, or White, as the report's).
    Neither,
    /// The budget ran out for a side before it was known.
    Unknown,
}

/// [`solve_side`] for each side the position is legal with, within `budget` each.
pub fn mating_side(fen: &Fen, n: u8, budget: u64) -> Result<MatingSide, MateError> {
    let mut mates = Vec::new();
    for side in [Color::White, Color::Black] {
        match solve_side(fen, side, n, budget) {
            Ok(report) if report.exhausted && report.keys.is_empty() => return Ok(MatingSide::Unknown),
            Ok(report) if !report.keys.is_empty() => mates.push(report),
            Ok(_) | Err(MateError::Illegal(_)) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(match mates.len() {
        0 => MatingSide::Neither,
        1 => mates.pop().map_or(MatingSide::Neither, MatingSide::One),
        _ => MatingSide::Both,
    })
}

/// The search's count of positions played, against its budget.
struct Search {
    nodes: u64,
    budget: u64,
}

impl Search {
    /// `board` after `mv`, one node spent; `None` when the budget is spent.
    fn play(&mut self, board: &Board, mv: Move) -> Option<Board> {
        if self.nodes >= self.budget {
            return None;
        }
        self.nodes = self.nodes.saturating_add(1);
        // `mv` comes from the board's own legal moves.
        playing(board, mv).or_else(|| Some(board.clone()))
    }

    /// Whether the side to move on `board` mates in at most `n` moves (n ≥ 1); `None` when the
    /// budget ran out first.
    fn mates(&mut self, board: &Board, n: u8) -> Option<bool> {
        for mv in moves(board) {
            if n == 1 && !gives_check(board, mv) {
                continue;
            }
            let after = self.play(board, mv)?;
            if self.forced(&after, n.saturating_sub(1))? {
                return Some(true);
            }
        }
        Some(false)
    }

    /// After a move of the mating side: whether every defence on `board` lets it mate in at most
    /// `left` more moves (0: the defender must be mated already).
    fn forced(&mut self, board: &Board, left: u8) -> Option<bool> {
        match board.status() {
            GameStatus::Won => return Some(true),
            GameStatus::Drawn => return Some(false),
            GameStatus::Ongoing => {}
        }
        if left == 0 {
            return Some(false);
        }
        for reply in moves(board) {
            let after = self.play(board, reply)?;
            if !self.mates(&after, left)? {
                return Some(false);
            }
        }
        Some(true)
    }

    /// When no move mates in `n`: the move with the fewest refuting defences (a stalemate is no
    /// try) and its first refutation; `Some(None)` when there is no try; `None` out of budget.
    fn best_try(&mut self, board: &Board, n: u8) -> Option<Option<(Move, Move)>> {
        let mut best: Option<(usize, Move, Move)> = None;
        for mv in moves(board) {
            let after = self.play(board, mv)?;
            if after.status() != GameStatus::Ongoing {
                continue;
            }
            let mut refuting = 0usize;
            let mut first = None;
            for reply in moves(&after) {
                let next = self.play(&after, reply)?;
                let holds = n == 1 || !self.mates(&next, n - 1)?;
                if holds {
                    refuting = refuting.saturating_add(1);
                    first.get_or_insert(reply);
                }
                if best.as_ref().is_some_and(|(fewest, ..)| refuting >= *fewest) {
                    break;
                }
            }
            if let Some(refutation) = first
                && best.as_ref().is_none_or(|(fewest, ..)| refuting < *fewest)
            {
                best = Some((refuting, mv, refutation));
            }
        }
        Some(best.map(|(_, mv, refutation)| (mv, refutation)))
    }
}

/// The legal moves on `board`.
fn moves(board: &Board) -> Vec<Move> {
    let mut out = Vec::new();
    board.generate_moves(|list| {
        out.extend(list);
        false
    });
    out
}

/// `board` after the legal move `mv`.
fn playing(board: &Board, mv: Move) -> Option<Board> {
    let mut after = board.clone();
    after.try_play(mv).ok()?;
    Some(after)
}

fn gives_check(board: &Board, mv: Move) -> bool {
    playing(board, mv).is_some_and(|after| !after.checkers().is_empty())
}

/// cozy-chess writes castling as the king taking its own rook.
fn is_castle(board: &Board, mv: Move) -> bool {
    board.piece_on(mv.from) == Some(cozy_chess::Piece::King) && board.color_on(mv.to) == Some(board.side_to_move())
}

fn san(board: &Board, mv: Move) -> String {
    San::from_move(board, mv).map_or_else(|_| format!("{mv}"), |s| s.to_english())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUDGET: u64 = DEFAULT_MATE_BUDGET * 10;

    fn keys(fen: &str, n: u8) -> MateReport {
        solve(fen, n, BUDGET).unwrap()
    }

    /// The card's back-rank position: each side mates in one (Rd8# and Rd1#), so neither is the
    /// side to play by the solution.
    #[test]
    fn both_sides_mate_on_the_back_rank() {
        let fen = Fen::parse("3r2k1/5ppp/8/8/8/8/5PPP/3R2K1").unwrap();
        assert_eq!(mating_side(&fen, 1, BUDGET), Ok(MatingSide::Both));
        let white = keys("3r2k1/5ppp/8/8/8/8/5PPP/3R2K1 w - - 0 1", 1);
        assert_eq!((white.keys.clone(), white.shortest), (vec!["Rxd8#".to_owned()], Some(1)));
        // Without Black's rook only White mates.
        let fen = Fen::parse("6k1/5ppp/8/8/8/8/5PPP/3R2K1").unwrap();
        assert!(matches!(mating_side(&fen, 1, BUDGET), Ok(MatingSide::One(r)) if r.side == Color::White && r.keys == ["Rd8#"]));
    }

    /// Castling is allowed unless the position proves otherwise: with the right, O-O is the one
    /// key (and said); without it, there is no mate in two, and the best try and its refutation
    /// are given.
    #[test]
    fn castling_follows_the_problem_convention() {
        let with = keys("8/8/8/8/4R3/6k1/8/4K2R w K - 0 1", 2);
        assert_eq!(with.keys, ["O-O"], "{with:?}");
        assert!(with.unique() && with.castles && with.shortest == Some(2));
        let without = keys("8/8/8/8/4R3/6k1/8/4K2R w - - 0 1", 2);
        assert!(without.no_mate(), "{without:?}");
        assert!(without.best_try.is_some() && without.refutation.is_some(), "{without:?}");
    }

    /// A mate in one asked as a mate in two: the problem is solved, shorter than asked.
    #[test]
    fn a_shorter_mate_solves_the_problem() {
        let r = keys("6k1/5ppp/8/8/8/8/5PPP/3R2K1 w - - 0 1", 2);
        assert!(r.keys.contains(&"Rd8#".to_owned()), "{r:?}");
        assert_eq!(r.shortest, Some(1));
    }

    /// Stalemate is no mate: the only move that would "end" the game draws it.
    #[test]
    fn stalemate_is_no_mate() {
        // White to play: Qb6 or Qc7 stalemate; Qb7# mates. A mate in 1 has one key.
        let r = keys("k7/8/1K6/8/8/8/8/2Q5 w - - 0 1", 1);
        assert!(r.keys.iter().all(|k| k.ends_with('#')), "{r:?}");
        let stalemate = keys("k7/2Q5/1K6/8/8/8/8/8 b - - 0 1", 1);
        assert!(stalemate.no_mate() && stalemate.best_try.is_none(), "{stalemate:?}");
    }

    /// The budget is counted in positions: spent, the report says so and claims no "no mate".
    #[test]
    fn the_budget_bounds_the_search() {
        let r = solve("8/8/8/8/4R3/6k1/8/4K2R w K - 0 1", 2, 10).unwrap();
        assert!(r.exhausted && r.nodes <= 10 && !r.no_mate(), "{r:?}");
        assert_eq!(solve("8/8/8/8/8/8/8/K6k w - - 0 1", 4, BUDGET), Err(MateError::Moves(4)));
        assert_eq!(solve("8/8/8/8/8/8/8/K6k w - - 0 1", 0, BUDGET), Err(MateError::Moves(0)));
        assert!(matches!(solve("not a fen", 2, BUDGET), Err(MateError::Fen(_))));
        // Two white kings: not a position to play.
        assert!(matches!(solve("8/8/8/8/8/8/8/KK5k w - - 0 1", 2, BUDGET), Err(MateError::Illegal(Color::White))));
    }

    /// Kings next to each other are no position (the proptest's find: `cozy-chess` takes it, and its
    /// move generation then lets one king take the other and panics looking for it): refused, never
    /// searched.
    #[test]
    fn kings_side_by_side_are_refused_not_searched() {
        assert!(matches!(solve("4k3/3K4/8/8/8/8/8/4N3 b - - 0 1", 1, 1), Err(MateError::Illegal(Color::Black))));
        assert!(matches!(solve("8/8/8/8/8/8/8/Kk6 w - - 0 1", 2, BUDGET), Err(MateError::Illegal(Color::White))));
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(64))]

        /// Any placement, any side, any n and budget: a report or an error, never a panic, and
        /// never more positions played than the budget.
        #[test]
        fn any_position_is_solved_or_refused(
            squares in proptest::collection::vec((0usize..64, 0usize..13), 0..14),
            n in 0u8..5,
            budget in 0u64..5_000,
            black in proptest::prelude::any::<bool>(),
        ) {
            let mut cells = ['1'; 64];
            cells[4] = 'k';
            cells[60] = 'K';
            for (sq, piece) in squares {
                cells[sq] = ['1', 'P', 'N', 'B', 'R', 'Q', 'K', 'p', 'n', 'b', 'r', 'q', 'k'][piece];
            }
            let rows: Vec<String> = cells.chunks(8).map(|r| r.iter().collect()).collect();
            let fen = format!("{} {} - - 0 1", rows.join("/"), if black { 'b' } else { 'w' });
            if let Ok(r) = solve(&fen, n, budget) {
                proptest::prop_assert!(r.nodes <= budget);
                proptest::prop_assert!(r.keys.is_empty() || r.shortest.is_some_and(|s| s <= n));
            }
        }
    }
}
