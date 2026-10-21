//! Drawings on a move's position: coloured squares and arrows, as an analysis board draws them,
//! kept on the move and written into its PGN comment.
//!
//! PGN has no place of its own for them, so they travel as commands inside the comment after the
//! move, in the convention most chess programs read and write (ChessBase's, which lichess follows
//! too): `[%csl Ga4,Rb5]` colours squares a4 green and b5 red, `[%cal Ge2e4,Rd1h5]` draws a green
//! arrow from e2 to e4 and a red one from d1 to h5. The colour letters are `G`reen, `R`ed,
//! `Y`ellow and `B`lue; a square's name follows (two for an arrow), comma-separated.
//!
//! Reading takes those commands out of the comment ([`split_drawings`]) and keeps the rest of it,
//! other `[%…]` commands (`[%clk]`, `[%eval]`) included, in place. Nothing read is dropped: an
//! entry that is not a drawing this model holds (an unknown colour letter, a square that is not
//! one, an arrow from a square to itself, a second colour for the same squares, an entry past
//! [`MAX_DRAWINGS`]) stays in the comment's text, in a command of its own name at the place the
//! command was. Writing puts the drawings first ([`drawing_commands`]: squares, then arrows), so
//! a comment read and written again is the same text.

use super::{Game, NodeId};
use crate::placement::Square;

/// Most drawings one move's position keeps (an analysis board keeps as many).
pub const MAX_DRAWINGS: usize = 64;

/// A drawing's colour, as the four letters of the PGN commands name them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DrawColor {
    Green,
    Red,
    Yellow,
    Blue,
}

impl DrawColor {
    /// The command's letter for the colour.
    pub fn letter(self) -> char {
        match self {
            DrawColor::Green => 'G',
            DrawColor::Red => 'R',
            DrawColor::Yellow => 'Y',
            DrawColor::Blue => 'B',
        }
    }

    /// The colour a command's letter names (upper case only, as the convention writes them).
    pub fn from_letter(letter: char) -> Option<DrawColor> {
        Some(match letter {
            'G' => DrawColor::Green,
            'R' => DrawColor::Red,
            'Y' => DrawColor::Yellow,
            'B' => DrawColor::Blue,
            _ => return None,
        })
    }
}

/// A coloured square (`from == to`) or an arrow from `from` to `to`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Drawing {
    pub from: Square,
    pub to: Square,
    pub color: DrawColor,
}

impl Drawing {
    /// Whether it is an arrow (else a square).
    pub fn is_arrow(&self) -> bool {
        self.from != self.to
    }
}

/// The two command names, for squares and for arrows.
const SQUARES: &str = "csl";
const ARROWS: &str = "cal";

/// `drawings` as PGN comment commands: `[%csl …]` for the squares, then `[%cal …]` for the
/// arrows, each in the order given; empty when there are none.
pub fn drawing_commands(drawings: &[Drawing]) -> String {
    let mut out = String::new();
    for (name, arrows) in [(SQUARES, false), (ARROWS, true)] {
        let entries: Vec<String> = drawings
            .iter()
            .filter(|d| d.is_arrow() == arrows)
            .map(|d| if arrows { format!("{}{}{}", d.color.letter(), d.from, d.to) } else { format!("{}{}", d.color.letter(), d.from) })
            .collect();
        if !entries.is_empty() {
            out.push_str(&format!("[%{name} {}]", entries.join(",")));
        }
    }
    out
}

/// Take the `[%csl …]` and `[%cal …]` commands out of a comment: their drawings join `into`
/// (squares before arrows, as [`Game::drawings`] keeps them), and the rest of the text is
/// returned with each command's place left as a space (the caller tidies the spaces). Entries
/// this model does not hold stay in the text, in a command of their own name where the command
/// was; a command none of whose entries are taken is left exactly as it was. An entry the same as
/// one already there (the same squares and colour) is taken without adding it twice.
pub fn split_drawings(text: &str, into: &mut Vec<Drawing>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("[%") {
        // `[` and `%` are ASCII, so `at` and `at + 2` are character boundaries.
        let (before, from_open) = rest.split_at(at);
        out.push_str(before);
        let body_and_after = from_open.get(2..).unwrap_or_default();
        let Some(close) = body_and_after.find(']') else {
            out.push_str(from_open);
            return out;
        };
        let body = body_and_after.get(..close).unwrap_or_default();
        let whole = from_open.get(..close.saturating_add(3)).unwrap_or(from_open);
        rest = body_and_after.get(close.saturating_add(1)..).unwrap_or_default();
        let (name, args) = body.split_once(char::is_whitespace).unwrap_or((body, ""));
        let arrows = match name {
            SQUARES => false,
            ARROWS => true,
            _ => {
                out.push_str(whole);
                continue;
            }
        };
        let mut kept: Vec<&str> = Vec::new();
        let mut took = false;
        for entry in args.split(',').map(str::trim).filter(|e| !e.is_empty()) {
            if take(entry, arrows, into) {
                took = true;
            } else {
                kept.push(entry);
            }
        }
        if !took {
            out.push_str(whole);
        } else if kept.is_empty() {
            out.push(' ');
        } else {
            out.push_str(&format!(" [%{name} {}] ", kept.join(",")));
        }
    }
    out.push_str(rest);
    out
}

/// Add the drawing `entry` names (`Ga4` for a square, `Ge2e4` for an arrow) to `into`. Whether it
/// was taken: an entry that is not one, an arrow from a square to itself, a second colour for the
/// same squares and an entry past [`MAX_DRAWINGS`] are not.
fn take(entry: &str, arrow: bool, into: &mut Vec<Drawing>) -> bool {
    let mut chars = entry.chars();
    let Some(color) = chars.next().and_then(DrawColor::from_letter) else { return false };
    let squares = chars.as_str();
    let (from, to) = if arrow {
        match (squares.get(..2).and_then(Square::parse), squares.get(2..).and_then(Square::parse)) {
            (Some(from), Some(to)) if from != to => (from, to),
            _ => return false,
        }
    } else {
        match Square::parse(squares) {
            Some(sq) => (sq, sq),
            None => return false,
        }
    };
    let drawing = Drawing { from, to, color };
    match into.iter().find(|d| d.from == from && d.to == to) {
        Some(d) => *d == drawing,
        None if into.len() < MAX_DRAWINGS => {
            insert(into, drawing);
            true
        }
        None => false,
    }
}

/// Put `drawing` in `list` after the drawings of its kind: squares stay before arrows.
fn insert(list: &mut Vec<Drawing>, drawing: Drawing) {
    let at = if drawing.is_arrow() { list.len() } else { list.iter().take_while(|d| !d.is_arrow()).count() };
    list.insert(at.min(list.len()), drawing);
}

impl Game {
    /// The drawings on the position after move `node` (on node 0: the start), squares first, then
    /// arrows.
    pub fn drawings(&self, node: NodeId) -> &[Drawing] {
        &self.node(node).drawings
    }

    /// Draw `drawing` on the position after move `node`, or take it off when it is there already
    /// in that colour; the same squares in another colour are recoloured. Returns whether the game
    /// changed: not for a move not in the game, nor a new drawing past [`MAX_DRAWINGS`].
    pub fn toggle_drawing(&mut self, node: NodeId, drawing: Drawing) -> bool {
        let Some(n) = self.nodes.get_mut(node) else { return false };
        let list = &mut n.drawings;
        match list.iter().position(|d| d.from == drawing.from && d.to == drawing.to) {
            Some(i) => {
                if list.get(i).is_some_and(|d| d.color == drawing.color) {
                    list.remove(i);
                } else if let Some(d) = list.get_mut(i) {
                    d.color = drawing.color;
                }
            }
            None if list.len() < MAX_DRAWINGS => insert(list, drawing),
            None => return false,
        }
        self.revision = self.revision.wrapping_add(1);
        true
    }

    /// Take every drawing off the position after move `node`. Returns whether there were any.
    pub fn clear_drawings(&mut self, node: NodeId) -> bool {
        let Some(n) = self.nodes.get_mut(node) else { return false };
        if n.drawings.is_empty() {
            return false;
        }
        n.drawings.clear();
        self.revision = self.revision.wrapping_add(1);
        true
    }
}

#[cfg(test)]
mod tests;
