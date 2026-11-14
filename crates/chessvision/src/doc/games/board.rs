//! A game of the document on the chess board: [`to_game`] makes a walker's [`GameScore`] the
//! board's [`pdfcraft_chess::game::Game`], and [`ChessDoc::board_game`] does it with each move's
//! place on its page.
//!
//! The game is built move by move from the score, not through PGN text: the start position,
//! the tags, every move with its glyphs, its comment and its comment before, the variations in
//! their order (the line a move continues in first). So each node of the score has exactly one
//! move on the board ([`BoardGame::paths`]), and a comment before a move stays on that move
//! wherever PGN would have to write it. Every move of a score was played when the score was
//! read, so all of them play again (a move whose SAN was edited into something no move is
//! written is found again from the positions before and after it); a move the board refuses
//! anyway (a game nested deeper or longer than the board holds, a score read from JSON that was
//! tampered with) ends its line there, and the moves left out are counted
//! ([`BoardGame::dropped`]) and said ([`BoardGame::warnings`]), never silently lost.
//!
//! It is also how the document's games are written as PGN (plan A9-F4, `super::super::pgn`):
//! the board's game, by the one PGN writer.

use std::collections::HashSet;

use cozy_chess::Board;
use pdfcraft_chess::game::{Game as ChessGame, MAX_TAG_LEN, NodeId, NodePath, NodeSource, PgnWarning, PgnWarningKind};
use pdfcraft_chess::san::San;

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
    /// Comments of the score the board had to cut (past `MAX_COMMENT_LEN` bytes): none from a
    /// score the walker read, whose cap is the same; one read from tampered JSON can have some.
    pub comments_cut: usize,
    /// What was left out or cut, each on the move it is at: [`PgnWarningKind::MovesLeftOut`] on
    /// the move a refused one was to follow, [`PgnWarningKind::CommentCut`] on the move whose
    /// comment was cut, [`PgnWarningKind::TagsDropped`] and [`PgnWarningKind::TagCut`] on the start
    /// for a tag the board can't keep or cuts.
    pub warnings: Vec<PgnWarning>,
}

impl BoardGame {
    /// The move of score node `node` on the board, when it is there.
    pub fn node(&self, node: usize) -> Option<NodeId> {
        let (_, path) = self.paths.iter().find(|(n, _)| *n == node)?;
        self.game.node_at(path)
    }
}

/// The placeholders PGN writes for unknown values, which the board reads as no value (as
/// `Game::from_pgn` reads a tag, so a game written and read back keeps its tags).
fn placeholder(value: &str) -> bool {
    matches!(value.trim(), "" | "?" | "????.??.??" | "*")
}

/// What [`build`] makes of a score.
struct Built {
    game: ChessGame,
    /// Each score node's board move, `None` when it was left out.
    placed: Vec<Option<NodeId>>,
    dropped: usize,
    comments_cut: usize,
    warnings: Vec<PgnWarning>,
}

impl Built {
    fn into_board_game(self) -> BoardGame {
        let Built { game, placed, dropped, comments_cut, warnings } = self;
        let paths = placed.iter().enumerate().filter_map(|(n, mv)| Some((n, game.path_of((*mv)?)?))).collect();
        BoardGame { game, paths, dropped, comments_cut, warnings }
    }
}

/// The SAN of the legal move after board move `at` that leads to the position `fen` (castling
/// rights, en passant and the side to move included, the move counters aside): how a move whose
/// SAN was edited into something no move is written is named again, never written `--` (PGN's
/// null move, another move altogether, plan A8 P8a).
fn san_by_position(game: &ChessGame, at: NodeId, fen: &str) -> Option<String> {
    let before = pdfcraft_chess::fen::board_from_fen(&game.fen_at(at)?.to_string()).ok()?;
    let after = pdfcraft_chess::fen::board_from_fen(fen.trim()).ok()?;
    let key = |b: &Board| pdfcraft_chess::fen::Fen::from_board(b).full_key();
    let want = key(&after);
    let mut found = None;
    before.generate_moves(|moves| {
        for mv in moves {
            let mut b = before.clone();
            b.play_unchecked(mv);
            if key(&b) == want {
                found = Some(mv);
                return true;
            }
        }
        false
    });
    San::from_move(&before, found?).ok().map(|s| s.to_english())
}

/// The board's game of `score` and where each score node went. `None` when the start position is
/// not legal.
fn build(score: &GameScore) -> Option<Built> {
    let mut game = match &score.start_fen {
        None => ChessGame::standard(),
        Some(fen) => ChessGame::from_fen(fen).ok()?,
    };
    let r = &score.roster;
    let roster = [("Event", &r.event), ("Site", &r.site), ("Date", &r.date), ("Round", &r.round), ("White", &r.white), ("Black", &r.black)];
    let result = (score.result != GameResult::Unknown).then(|| ("Result", score.result.as_pgn()));
    let tags =
        roster.into_iter().map(|(n, v)| (n, v.as_str())).chain(result).chain(score.extra_tags.iter().map(|t| (t.name.as_str(), t.value.as_str())));
    // A tag the board can't keep (a name PGN does not allow, past the most a game holds: only in
    // a score read from tampered JSON) is left out, and one longer than it keeps is cut: said.
    let mut warnings = Vec::new();
    for (name, value) in tags.filter(|(_, v)| !placeholder(v)) {
        if game.set_tag(name, value).is_err() {
            warnings.push(PgnWarning { kind: PgnWarningKind::TagsDropped, at: 0 });
        } else if value.trim().len() > MAX_TAG_LEN {
            warnings.push(PgnWarning { kind: PgnWarningKind::TagCut, at: 0 });
        }
    }
    let mut b = Built { game, placed: vec![None; score.nodes.len()], dropped: 0, comments_cut: 0, warnings };
    let cut = b.game.set_comment(0, &score.comment).is_err();
    b.cut(0, cut);
    // Every score node is taken once: a tree read from JSON may point twice at one node or loop.
    let mut seen = vec![false; score.nodes.len()];
    let mut on_board: HashSet<NodeId> = HashSet::from([0]);
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
            b.game.go_to_node(at);
            let named =
                b.game.play_san(&node.san).is_ok() || san_by_position(&b.game, at, &node.fen).is_some_and(|san| b.game.play_san(&san).is_ok());
            let played = named.then(|| b.game.current()).filter(|&m| m != at && on_board.insert(m));
            let Some(mv) = played else {
                b.dropped = b.dropped.saturating_add(left_out(score, child, &mut seen));
                b.warnings.push(PgnWarning { kind: PgnWarningKind::MovesLeftOut, at });
                continue;
            };
            if let Some(slot) = b.placed.get_mut(child) {
                *slot = Some(mv);
            }
            for &code in &node.nags {
                // A glyph the board can't hold (out of range, or too many) is only a glyph lost.
                b.game.set_nag(mv, code);
            }
            let cut = b.game.set_comment(mv, &node.comment).is_err();
            b.cut(mv, cut);
            let cut = b.game.set_comment_before(mv, &node.comment_before).is_err();
            b.cut(mv, cut);
            todo.push((Some(child), mv));
        }
    }
    b.game.go_to_node(0);
    // The tags came before the moves: the ones the moves decide describe them as they are.
    b.game.settle_move_tags(true);
    Some(b)
}

impl Built {
    /// Count and say a comment of board move `node` that was cut.
    fn cut(&mut self, node: NodeId, cut: bool) {
        if cut {
            self.comments_cut = self.comments_cut.saturating_add(1);
            self.warnings.push(PgnWarning { kind: PgnWarningKind::CommentCut, at: node });
        }
    }
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
    build(score).map(Built::into_board_game)
}

impl ChessDoc {
    /// Game `index` of the document as the chess board's game ([`to_game`]), each move keeping
    /// where it was printed (its page and its box in user space) when the pages say. `None`
    /// when there is no such game or its start position is not legal.
    pub fn board_game(&self, index: usize) -> Option<BoardGame> {
        let score = &self.games.get(index)?.score;
        let mut built = build(score)?;
        for page in &self.pages {
            for inline in page.blocks.iter().flat_map(Block::inlines) {
                let Inline::Move(m) = inline else { continue };
                let (Some(r), Some(rect)) = (m.game, m.source_rect) else { continue };
                if r.game != index {
                    continue;
                }
                let Some(mv) = built.placed.get(r.node).copied().flatten() else { continue };
                // The first place a move was printed is its place.
                if built.game.source(mv).is_none() {
                    built.game.set_source(mv, Some(NodeSource { page: page.index, rect_user: [rect.x0, rect.y0, rect.x1, rect.y1] }));
                }
            }
        }
        // Setting the sources moved nothing: the game is still shown at its start.
        Some(built.into_board_game())
    }
}
