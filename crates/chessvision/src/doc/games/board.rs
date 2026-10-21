//! A game of the document on the chess board: [`to_game`] makes a walker's [`GameScore`] the
//! board's [`pdfcraft_chess::game::Game`], and [`ChessDoc::board_game`] does it with each move's
//! place on its page.
//!
//! The game is built move by move from the score, not through PGN text: the start position,
//! the tags, every move with its glyphs, its comment and its comment before, the variations in
//! their order (the line a move continues in first). So each node of the score has exactly one
//! move on the board ([`BoardGame::paths`]), and a comment before a move stays on that move
//! wherever PGN would have to write it. Every move of a score was played when the score was
//! read, so all of them play again; a move the board refuses anyway (a game nested deeper or
//! longer than the board holds, a score read from JSON that was tampered with) ends its line
//! there, and the moves left out are counted ([`BoardGame::dropped`]), never silently lost.

use std::collections::HashSet;

use pdfcraft_chess::game::{Game as ChessGame, NodeId, NodePath, NodeSource};

use super::{GameResult, GameScore};
use crate::doc::{Block, ChessDoc, Inline};

/// A score as the chess board's game.
#[derive(Clone, Debug)]
pub struct BoardGame {
    /// The game, shown at its start.
    pub game: ChessGame,
    /// Each node of the score that is on the board (its index in [`GameScore::nodes`]) and the
    /// path to its move there, in the score's node order.
    pub paths: Vec<(usize, NodePath)>,
    /// Moves of the score left off the board: one the board refused, with every move after it.
    pub dropped: usize,
}

impl BoardGame {
    /// The move of score node `node` on the board, when it is there.
    pub fn node(&self, node: usize) -> Option<NodeId> {
        let (_, path) = self.paths.iter().find(|(n, _)| *n == node)?;
        self.game.node_at(path)
    }
}

/// The roster's placeholders, which the board reads as no value (as `Game::from_pgn` does).
fn placeholder(value: &str) -> bool {
    matches!(value.trim(), "" | "?" | "????.??.??")
}

/// The board's game of `score` and where each score node went (its board move, `None` when it
/// was left out), with the count left out. `None` when the start position is not legal.
fn build(score: &GameScore) -> Option<(ChessGame, Vec<Option<NodeId>>, usize)> {
    let mut game = match &score.start_fen {
        None => ChessGame::standard(),
        Some(fen) => ChessGame::from_fen(fen).ok()?,
    };
    let r = &score.roster;
    for (name, value) in [("Event", &r.event), ("Site", &r.site), ("Date", &r.date), ("Round", &r.round), ("White", &r.white), ("Black", &r.black)] {
        if !placeholder(value) {
            // A value the board can't keep (a bad name can't happen here) is only a tag lost.
            let _ = game.set_tag(name, value);
        }
    }
    if score.result != GameResult::Unknown {
        let _ = game.set_tag("Result", score.result.as_pgn());
    }
    for t in &score.extra_tags {
        if !placeholder(&t.value) {
            let _ = game.set_tag(&t.name, &t.value);
        }
    }
    game.set_comment(0, &score.comment);

    let count = score.nodes.len();
    let mut placed: Vec<Option<NodeId>> = vec![None; count];
    // Every score node is taken once: a tree read from JSON may point twice at one node or loop.
    let mut seen = vec![false; count];
    let mut on_board: HashSet<NodeId> = HashSet::from([0]);
    let mut dropped = 0usize;
    // Lines still to play: the score node they go on from (`None`: the start) and its move.
    let mut todo: Vec<(Option<usize>, NodeId)> = vec![(None, 0)];
    while let Some((from, at)) = todo.pop() {
        // A move's replies are played in their order, so the board keeps it: the first goes on
        // with the line, the others are its variations.
        for &child in score.children(from) {
            match seen.get_mut(child) {
                Some(s) if !*s => *s = true,
                _ => continue,
            }
            let Some(node) = score.nodes.get(child) else { continue };
            game.go_to_node(at);
            let played = game.play_san(&node.san).ok().map(|()| game.current()).filter(|&m| m != at && on_board.insert(m));
            let Some(mv) = played else {
                dropped = dropped.saturating_add(left_out(score, child, &mut seen));
                continue;
            };
            if let Some(slot) = placed.get_mut(child) {
                *slot = Some(mv);
            }
            for &code in &node.nags {
                // A glyph the board can't hold (out of range, or too many) is only a glyph lost.
                game.set_nag(mv, code);
            }
            game.set_comment(mv, &node.comment);
            game.set_comment_before(mv, &node.comment_before);
            todo.push((Some(child), mv));
        }
    }
    game.go_to_node(0);
    Some((game, placed, dropped))
}

/// How many moves are left out with score node `n`: it and every move after it not taken yet
/// (each counted once, marked as taken).
fn left_out(score: &GameScore, n: usize, seen: &mut [bool]) -> usize {
    let mut count = 1usize;
    let mut todo: Vec<usize> = score.children(Some(n)).to_vec();
    while let Some(c) = todo.pop() {
        match seen.get_mut(c) {
            Some(s) if !*s => *s = true,
            _ => continue,
        }
        count = count.saturating_add(1);
        todo.extend_from_slice(score.children(Some(c)));
    }
    count
}

/// The chess board's game of `score`: its start position, tags, comments, glyphs and every
/// variation in its order, with the path to each node's move. `None` when the start position is
/// not legal.
pub fn to_game(score: &GameScore) -> Option<BoardGame> {
    let (game, placed, dropped) = build(score)?;
    let paths = placed.iter().enumerate().filter_map(|(n, mv)| Some((n, game.path_of((*mv)?)?))).collect();
    Some(BoardGame { game, paths, dropped })
}

impl ChessDoc {
    /// Game `index` of the document as the chess board's game ([`to_game`]), each move keeping
    /// where it was printed (its page and its box in user space) when the pages say. `None`
    /// when there is no such game or its start position is not legal.
    pub fn board_game(&self, index: usize) -> Option<BoardGame> {
        let score = &self.games.get(index)?.score;
        let (mut game, placed, dropped) = build(score)?;
        for page in &self.pages {
            for inline in page.blocks.iter().flat_map(Block::inlines) {
                let Inline::Move(m) = inline else { continue };
                let (Some(r), Some(rect)) = (m.game, m.source_rect) else { continue };
                if r.game != index {
                    continue;
                }
                let Some(mv) = placed.get(r.node).copied().flatten() else { continue };
                // The first place a move was printed is its place.
                if game.source(mv).is_none() {
                    game.set_source(mv, Some(NodeSource { page: page.index, rect_user: [rect.x0, rect.y0, rect.x1, rect.y1] }));
                }
            }
        }
        // Setting the sources moved nothing: the game is still shown at its start.
        let paths = placed.iter().enumerate().filter_map(|(n, mv)| Some((n, game.path_of((*mv)?)?))).collect();
        Some(BoardGame { game, paths, dropped })
    }
}
