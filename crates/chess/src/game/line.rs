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

use super::{Game, NodeId};
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
}
