//! The book's PGN searched by material and pawn structure (plan A9-N16a, finding A8-09): each game
//! the index holds played again once, from the text it keeps ([`ReferenceIndex::game`]), every
//! line of it, and each position after a move held to a [`Query`]. The index keeps only a short
//! digest of each position, which does not say its material, so the moves are played again.
//!
//! **Stretches, not positions.** The positions in a row of one line that match are one
//! [`Stretch`], its first and its last move: a rook ending of thirty moves is one find. A variation
//! is a line of its own, from its first move.

use std::collections::VecDeque;

use pdfcraft_chess::fen::Fen;
use pdfcraft_chess::game::{Game as PgnGame, NodeId};
use pdfcraft_chess::pattern::Query;
use pdfcraft_chess::placement::Color;

use super::ReferenceIndex;

/// A move as a book numbers it: `34.Rd1`, `34...Kf7`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NumberedMove {
    pub number: u32,
    pub black: bool,
    pub san: String,
}

impl std::fmt::Display for NumberedMove {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.black { write!(f, "{}...{}", self.number, self.san) } else { write!(f, "{}.{}", self.number, self.san) }
    }
}

/// Positions in a row of one line that a query found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stretch {
    /// The game, from 0.
    pub game: usize,
    /// The move that reached its first position, and the one that reached its last.
    pub first: NumberedMove,
    pub last: NumberedMove,
    /// On the game's main line, or in a variation.
    pub main_line: bool,
    /// How many positions it holds.
    pub positions: usize,
}

/// What [`ReferenceIndex::stretches`] found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stretches {
    /// The first ones, game by game and each game's main line first, at most the `max` asked for.
    pub found: Vec<Stretch>,
    /// How many there are, all of them.
    pub total: usize,
    /// Each game with any, from 0, and how many it has.
    pub per_game: Vec<(usize, usize)>,
    /// The games played again: each indexed game once (plan A9-N16a's count).
    pub played: usize,
    /// Indexed games whose text could not be played again.
    pub unreadable: usize,
}

/// The move that reached `node`, numbered from the position after it.
fn numbered(game: &PgnGame, node: NodeId, fen: &Fen) -> NumberedMove {
    let full = fen.fullmove_number();
    // Black to move after it: White played it, in move `full`; else Black, in the move before.
    let (number, black) = match fen.side() {
        Some(Color::Black) => (full, false),
        _ => (full.saturating_sub(1).max(1), true),
    };
    NumberedMove { number, black, san: game.san(node).to_owned() }
}

/// The run of matching positions a line is in, while it lasts.
struct Run {
    first: NumberedMove,
    last: NumberedMove,
    positions: usize,
}

impl ReferenceIndex {
    /// The stretches of the book's PGN whose positions `query` finds, at most `max` listed (plan
    /// A9-N16a). Each indexed game is played again once (a game past the caps, with no moves
    /// indexed, is not); the positions after its moves, every line, are held to the query.
    pub fn stretches(&self, query: &Query, max: usize) -> Stretches {
        let mut out = Stretches::default();
        for (k, g) in self.games().iter().enumerate() {
            if g.moves == 0 {
                continue;
            }
            let Some(game) = self.game(k) else {
                out.unreadable = out.unreadable.saturating_add(1);
                continue;
            };
            out.played = out.played.saturating_add(1);
            let before = out.total;
            game_stretches(&game, k, query, max, &mut out);
            let found = out.total.saturating_sub(before);
            if found > 0 {
                out.per_game.push((k, found));
            }
        }
        out
    }
}

/// The stretches of `game` (game `k`), pushed to `out`: its main line first, then its variations
/// in the order they branch off.
fn game_stretches(game: &PgnGame, k: usize, query: &Query, max: usize, out: &mut Stretches) {
    let mut lines: VecDeque<(NodeId, bool)> = VecDeque::new();
    let roots = game.replies(0);
    if let Some(first) = roots.first() {
        lines.push_back((*first, true));
    }
    lines.extend(roots.iter().skip(1).map(|n| (*n, false)));
    // A game is a tree: no node is reached twice. The count bounds it anyway.
    let mut budget = game.move_count().saturating_add(1);
    while let Some((start, main_line)) = lines.pop_front() {
        let mut run: Option<Run> = None;
        let mut node = start;
        loop {
            if budget == 0 {
                return;
            }
            budget -= 1;
            let found = game.fen_at(node).filter(|fen| query.matches(fen.placement()));
            match (found, run.as_mut()) {
                (Some(fen), Some(r)) => {
                    r.last = numbered(game, node, &fen);
                    r.positions = r.positions.saturating_add(1);
                }
                (Some(fen), None) => {
                    let m = numbered(game, node, &fen);
                    run = Some(Run { first: m.clone(), last: m, positions: 1 });
                }
                (None, _) => flush(&mut run, k, main_line, max, out),
            }
            let replies = game.replies(node);
            lines.extend(replies.iter().skip(1).map(|n| (*n, false)));
            match replies.first() {
                Some(next) => node = *next,
                None => break,
            }
        }
        flush(&mut run, k, main_line, max, out);
    }
}

/// Count the run that ended, and list it while fewer than `max` are.
fn flush(run: &mut Option<Run>, game: usize, main_line: bool, max: usize, out: &mut Stretches) {
    let Some(r) = run.take() else { return };
    out.total = out.total.saturating_add(1);
    if out.found.len() < max {
        out.found.push(Stretch { game, first: r.first, last: r.last, main_line, positions: r.positions });
    }
}

#[cfg(test)]
mod tests;
