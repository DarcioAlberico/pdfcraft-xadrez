//! A stretch of a line as books print it in running text, and the diagram marker a move carries.
//!
//! [`Game::line_text`] writes the moves from the start of the variation a move is in (from the
//! start of the game on the main line) up to that move, numbered as running text numbers them:
//! `18.♘d5! ♕xe1 19.♘xe7+`, or `18…♕xe1` when the stretch starts with Black's move. The pieces
//! are the figurines (`♘`, one set for both sides, as books print them) or the English letters,
//! and the glyphs are their printed signs.
//!
//! The diagram marker ([`crate::nag::DIAGRAM`]) is a glyph code like the others, so it goes
//! wherever the PGN goes and comes back as it was; it is left out of the running text, where a
//! book prints the diagram itself.

use std::sync::{Arc, Mutex};

use super::{Game, NodeId, Token};
use crate::nag;

/// The figurine for an English piece letter (white figurines for both sides, as books print
/// them).
fn figurine(letter: char) -> Option<char> {
    Some(match letter {
        'K' => '♔',
        'Q' => '♕',
        'R' => '♖',
        'B' => '♗',
        'N' => '♘',
        _ => return None,
    })
}

/// English SAN with its piece letters as figurines: the moving piece (`Nxe7+` → `♘xe7+`) and
/// the piece a pawn becomes (`e8=Q` → `e8=♕`). Castling, pawns and the null move are unchanged.
fn with_figurines(san: &str) -> String {
    let mut out = String::with_capacity(san.len().saturating_add(8));
    let mut after_equals = false;
    for (i, c) in san.chars().enumerate() {
        let piece = (i == 0 || after_equals).then(|| figurine(c)).flatten();
        out.push(piece.unwrap_or(c));
        after_equals = c == '=';
    }
    out
}

impl Game {
    /// The first move of the stretch of line move `node` is in: the first move of the variation
    /// it is in (the nearest move, `node` itself included, that is not its parent's first reply),
    /// or on the main line the game's first move. `None` for the start and a move not in the
    /// game.
    pub fn line_start(&self, node: NodeId) -> Option<NodeId> {
        let up = self.ancestry(node)?;
        let mut first = None;
        for &at in &up {
            first = Some(at);
            if self.reply_index(at).is_some_and(|(_, index)| index != 0) {
                break;
            }
        }
        first
    }

    /// The moves from [`Game::line_start`] of move `node` to `node`, as running text: each
    /// White move after its number (`19.`), the first move after its number too when Black plays
    /// it (`18…`), the move-quality glyphs glued on (`!`, `?!`) and the others after a space.
    /// With `figurines`, the pieces are figurines (`♘d5`), else English letters (`Nd5`). Empty
    /// for the start and a move not in the game.
    pub fn line_text(&self, node: NodeId, figurines: bool) -> String {
        let Some(up) = self.ancestry(node) else { return String::new() };
        let Some(first) = self.line_start(node) else { return String::new() };
        let count = up.iter().position(|&at| at == first).map_or(0, |i| i.saturating_add(1));
        let mut out = String::new();
        for (i, &id) in up.iter().take(count).rev().enumerate() {
            let n = self.node(id);
            let before = &self.node(n.parent).board;
            let white = before.side_to_move() == cozy_chess::Color::White;
            if i > 0 {
                out.push(' ');
            }
            if white {
                out.push_str(&format!("{}.", before.fullmove_number()));
            } else if i == 0 {
                out.push_str(&format!("{}…", before.fullmove_number()));
            }
            if figurines {
                out.push_str(&with_figurines(&n.san));
            } else {
                out.push_str(&n.san);
            }
            for &code in n.nags.iter().filter(|&&c| !nag::is_diagram(c)) {
                if !(1..=6).contains(&code) {
                    out.push(' ');
                }
                out.push_str(&nag::shown(code));
            }
        }
        out
    }

    /// Whether move `node` is marked for a diagram ([`crate::nag::DIAGRAM`], or the one from
    /// Black's side).
    pub fn has_diagram(&self, node: NodeId) -> bool {
        self.nodes.get(node).is_some_and(|n| n.nags.iter().any(|&c| nag::is_diagram(c)))
    }

    /// Mark move `node` for a diagram, or take the mark off (both diagram codes). Returns whether
    /// the game changed: not for the start, a move not in the game, or a move that has as many
    /// glyphs as it may.
    pub fn toggle_diagram(&mut self, node: NodeId) -> bool {
        if node == 0 || node >= self.nodes.len() {
            return false;
        }
        if self.has_diagram(node) {
            for code in [nag::DIAGRAM, nag::DIAGRAM_FROM_BLACK] {
                if self.nags(node).contains(&code) {
                    self.toggle_nag(node, code);
                }
            }
            true
        } else {
            self.set_nag(node, nag::DIAGRAM)
        }
    }
}

/// The moves in the order the notation reads them and which of them start a variation, read
/// from the tokens once for each revision of the game (plan A9-F5, A1-23): stepping from
/// variation to variation made the tokens twice and searched a list for each move, 4 ms a step
/// with 2,000 moves. A clone of the game starts with an empty cache.
#[derive(Debug, Default)]
pub(super) struct ReadingCache(Mutex<Option<(u64, Arc<Reading>)>>);

impl Clone for ReadingCache {
    fn clone(&self) -> ReadingCache {
        ReadingCache::default()
    }
}

/// See [`ReadingCache`].
#[derive(Debug)]
pub(super) struct Reading {
    /// Every move, in reading order.
    order: Vec<NodeId>,
    /// By node: its place in `order`.
    place: Vec<Option<usize>>,
    /// By place in `order`: whether the move starts a variation.
    starts: Vec<bool>,
}

impl Reading {
    fn of(game: &Game, nodes: usize) -> Reading {
        let mut reading = Reading { order: Vec::new(), place: vec![None; nodes], starts: Vec::new() };
        let mut opened = false;
        for token in game.tokens() {
            match token {
                Token::VariationStart => opened = true,
                Token::Move { node, .. } => {
                    if let Some(slot) = reading.place.get_mut(node) {
                        *slot = Some(reading.order.len());
                    }
                    reading.order.push(node);
                    reading.starts.push(opened);
                    opened = false;
                }
                _ => {}
            }
        }
        reading
    }

    /// The move at place `i` when it starts a variation.
    fn start_at(&self, i: usize) -> Option<NodeId> {
        self.starts.get(i).copied().filter(|&s| s).and(self.order.get(i).copied())
    }
}

impl Game {
    /// The reading order for this revision: the cached one, or read now and kept.
    fn reading(&self) -> Arc<Reading> {
        let mut cached = match self.reading.0.lock() {
            Ok(guard) => guard,
            // A panic elsewhere while it was held left a reading or none: either is whole.
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some((revision, reading)) = cached.as_ref()
            && *revision == self.revision
        {
            return Arc::clone(reading);
        }
        let reading = Arc::new(Reading::of(self, self.nodes.len()));
        *cached = Some((self.revision, Arc::clone(&reading)));
        reading
    }

    /// The first move of every variation, in reading order: the order [`Game::tokens`] writes
    /// them, a variation's own variations right after the move they branch from.
    pub fn variation_starts(&self) -> Vec<NodeId> {
        let reading = self.reading();
        (0..reading.order.len()).filter_map(|i| reading.start_at(i)).collect()
    }

    /// The first move of the next variation after `node` in reading order (plan HC11): into a
    /// variation, out of one into the next, across the main line. `None` after the last.
    pub fn next_variation(&self, node: NodeId) -> Option<NodeId> {
        let reading = self.reading();
        let from = reading.place.get(node).copied().flatten().map_or(0, |at| at.saturating_add(1));
        (from..reading.order.len()).find_map(|i| reading.start_at(i))
    }

    /// The first move of the variation before `node` in reading order: the one `node` is in when
    /// `node` is past its first move, else the one before. `None` before the first.
    pub fn previous_variation(&self, node: NodeId) -> Option<NodeId> {
        let reading = self.reading();
        let at = reading.place.get(node).copied().flatten()?;
        (0..at).rev().find_map(|i| reading.start_at(i))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::NodePath;

    fn game(pgn: &str) -> Game {
        let (g, stopped) = Game::from_pgn(pgn).unwrap();
        assert_eq!(stopped, None);
        g
    }

    fn at(g: &Game, path: &[u16]) -> NodeId {
        g.node_at(&NodePath::new(path.to_vec())).unwrap()
    }

    #[test]
    fn a_line_reads_as_a_book_prints_it() {
        let mut g = Game::from_fen("r1b2rk1/pp2bppp/2n1pn2/q7/2BP4/2N1BN2/PP1Q1PPP/R4RK1 w - - 0 18").unwrap();
        for m in ["d5", "exd5", "Nxd5", "Nxd5"] {
            g.play_san(m).unwrap();
        }
        let last = g.current();
        g.toggle_nag(last, 2);
        let d5 = at(&g, &[0]);
        g.toggle_nag(d5, 1);
        g.toggle_nag(d5, 16);
        assert_eq!(g.line_text(last, true), "18.d5! ± exd5 19.♘xd5 ♘xd5?");
        assert_eq!(g.line_text(last, false), "18.d5! ± exd5 19.Nxd5 Nxd5?");
        assert_eq!(g.line_text(0, true), "");
        assert_eq!(g.line_text(999, true), "");
    }

    #[test]
    fn a_variation_is_copied_from_its_first_move_and_black_starts_with_an_ellipsis() {
        let g = game("1. e4 e5 (1... c5 2. Nf3 (2. c3 d5) d6) 2. Nf3 *");
        assert_eq!(g.line_text(at(&g, &[0, 1, 0, 0]), true), "1…c5 2.♘f3 d6");
        assert_eq!(g.line_text(at(&g, &[0, 1, 1, 0]), false), "2.c3 d5");
        assert_eq!(g.line_text(at(&g, &[0, 0, 0]), true), "1.e4 e5 2.♘f3");
        assert_eq!(g.line_start(at(&g, &[0, 1, 0, 0])), Some(at(&g, &[0, 1])));
        assert_eq!(g.line_start(0), None);
    }

    #[test]
    fn figurines_replace_the_moving_piece_and_the_promotion_only() {
        assert_eq!(with_figurines("Nxe7+"), "♘xe7+");
        assert_eq!(with_figurines("exd8=Q+"), "exd8=♕+");
        assert_eq!(with_figurines("O-O-O"), "O-O-O");
        assert_eq!(with_figurines("Kb1"), "♔b1");
        assert_eq!(with_figurines("Bxb5"), "♗xb5");
        assert_eq!(with_figurines("--"), "--");
    }

    #[test]
    fn the_diagram_marker_is_a_glyph_kept_by_pgn_and_left_out_of_the_text() {
        let mut g = game("1. e4 e5 2. Nf3 *");
        let nf3 = at(&g, &[0, 0, 0]);
        assert!(!g.has_diagram(nf3));
        assert!(g.toggle_diagram(nf3));
        assert!(g.has_diagram(nf3));
        assert_eq!(g.movetext(), "1. e4 e5 2. Nf3 $220");
        assert_eq!(g.line_text(nf3, true), "1.e4 e5 2.♘f3");
        let (back, _) = Game::from_pgn(&g.pgn()).unwrap();
        assert!(back.has_diagram(at(&back, &[0, 0, 0])));
        assert_eq!(back.pgn(), g.pgn());
        assert!(g.toggle_diagram(nf3));
        assert_eq!(g.movetext(), "1. e4 e5 2. Nf3");
        // From Black's side, as another program wrote it: a diagram too, and taken off with it.
        let mut g = game("1. e4 $221 *");
        let e4 = at(&g, &[0]);
        assert!(g.has_diagram(e4));
        assert!(g.toggle_diagram(e4));
        assert_eq!(g.movetext(), "1. e4");
        assert!(!g.toggle_diagram(0), "the start has no move to mark");
        assert!(!g.toggle_diagram(99));
    }

    /// Plan HC11: next and previous variation walk the variations in reading order, four levels
    /// deep, into and out of them and across the main line.
    #[test]
    fn variations_are_walked_in_reading_order() {
        let pgn = "1. e4 e5 (1... c5 2. Nf3 (2. c3 d5 (2... Nf6 3. e5 (3. d3 d5))) d6) (1... e6) 2. Nf3 (2. Bc4) Nc6 *";
        let (g, stopped) = Game::from_pgn(pgn).unwrap();
        assert_eq!(stopped, None);
        let name = |n: NodeId| g.node(n).san.clone();
        let starts: Vec<String> = g.variation_starts().into_iter().map(name).collect();
        assert_eq!(starts, ["c5", "c3", "Nf6", "d3", "e6", "Bc4"]);
        // Forward from the start, then back from the end.
        let mut at = 0;
        let mut seen = Vec::new();
        while let Some(n) = g.next_variation(at) {
            seen.push(name(n));
            at = n;
        }
        assert_eq!(seen, starts);
        let end = g.reading().order.last().copied().unwrap();
        let mut at = end;
        let mut back = Vec::new();
        while let Some(n) = g.previous_variation(at) {
            back.push(name(n));
            at = n;
        }
        assert_eq!(back, ["Bc4", "e6", "d3", "Nf6", "c3", "c5"]);
        // Inside a variation, past its first move: previous goes to that first move.
        let d5 = g.reading().order.iter().copied().find(|&n| name(n) == "d5").unwrap();
        assert_eq!(g.previous_variation(d5).map(name).as_deref(), Some("c3"));
        // A game with no variation has none to go to.
        let (plain, _) = Game::from_pgn("1. e4 e5 *").unwrap();
        assert_eq!((plain.next_variation(0), plain.previous_variation(0)), (None, None));
    }

    /// The moves in reading order and the variation starts as the tokens give them, made afresh:
    /// what the cache must agree with (the two-pass search it replaced, plan A9-F5, A1-23).
    fn fresh(g: &Game) -> (Vec<NodeId>, Vec<NodeId>) {
        let mut order = Vec::new();
        let mut starts = Vec::new();
        let mut opened = false;
        for token in g.tokens() {
            match token {
                Token::VariationStart => opened = true,
                Token::Move { node, .. } => {
                    if opened {
                        starts.push(node);
                    }
                    order.push(node);
                    opened = false;
                }
                _ => {}
            }
        }
        (order, starts)
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(64))]

        /// Plan A9-F5 (A1-23): the reading order is read once per revision, and every change to
        /// the tree (a move, a variation made the main line, moved, deleted) gives the next step
        /// the new order, never the cached one; a clone keeps its own.
        #[test]
        fn the_cached_reading_order_follows_every_change(ops in proptest::collection::vec((0u8..6, 0usize..400), 1..60)) {
            let mut g = Game::standard();
            for (op, pick) in ops {
                let nodes = g.nodes.len();
                let node = pick % nodes;
                match op {
                    0 | 1 => {
                        g.go_to_node(node);
                        let moves = g.legal_moves();
                        if let Some(m) = moves.get(pick % moves.len().max(1)) {
                            let _ = g.play_san(m);
                        }
                    }
                    2 => {
                        let _ = g.promote(node);
                    }
                    3 => {
                        let _ = g.move_variation(node, super::super::Shift::Up);
                    }
                    4 => {
                        let _ = g.delete_variation(node);
                    }
                    _ => {
                        let copy = g.clone();
                        proptest::prop_assert_eq!(copy.variation_starts(), g.variation_starts());
                    }
                }
                let (order, starts) = fresh(&g);
                proptest::prop_assert_eq!(g.variation_starts(), starts.clone());
                for (i, &n) in order.iter().enumerate() {
                    let next = order.iter().skip(i + 1).find(|m| starts.contains(m)).copied();
                    let previous = order.iter().take(i).rev().find(|m| starts.contains(m)).copied();
                    proptest::prop_assert_eq!(g.next_variation(n), next);
                    proptest::prop_assert_eq!(g.previous_variation(n), previous);
                }
                proptest::prop_assert_eq!(g.next_variation(0), order.iter().find(|m| starts.contains(m)).copied());
            }
        }
    }
}
