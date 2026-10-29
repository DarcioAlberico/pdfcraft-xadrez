//! The shape of the move tree, and what a move carries besides its move.
//!
//! The tree is reshaped by explicit *heads*. A head is a move whose parent has two replies or
//! more: the first move of a stretch of line, which can be promoted, demoted or deleted as a
//! variation. The operations take the head they work on and never guess it from the move shown;
//! [`Game::heads_on_path`] lists the heads a move sits under, nearest first, for a caller to
//! choose from.
//!
//! Node ids change whenever the tree is compacted (after every structural change). What has to
//! name a move across edits (a preview, an automation call, an autosave) uses its [`NodePath`],
//! the reply indices from the start.
//!
//! Every operation either changes the game or says why it did not ([`StructureError`]), keeps
//! the comments, glyphs, comments-before, sources and the move shown (when it survives), and
//! never nests a move deeper than [`MAX_DEPTH`].

use std::fmt;

use super::{Game, MAX_DEPTH, MAX_NAGS, NodeId, clean_comment, nag_group};

/// Where on a PDF page a move was read: the page index and the rectangle `[x0, y0, x1, y1]` in
/// the page's user space. It is kept with the move but is not part of the PGN.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NodeSource {
    pub page: u32,
    pub rect_user: [f32; 4],
}

/// A move named by the reply indices that lead to it from the start: `[0, 2]` is the start's
/// first reply, then that move's third reply. The empty path is the start. Unlike a
/// [`NodeId`], a path stays valid while the moves before it are not reordered.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct NodePath(Vec<u16>);

impl NodePath {
    /// The path through the reply indices `indices`, from the start.
    pub fn new(indices: Vec<u16>) -> NodePath {
        NodePath(indices)
    }

    /// The reply indices, from the start.
    pub fn indices(&self) -> &[u16] {
        &self.0
    }

    /// Whether this is the start's path (the empty one).
    pub fn is_start(&self) -> bool {
        self.0.is_empty()
    }
}

impl From<Vec<u16>> for NodePath {
    fn from(indices: Vec<u16>) -> NodePath {
        NodePath(indices)
    }
}

/// Why a structural change was refused. The game is left as it was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StructureError {
    /// The game has no such move, or it is the start, which has no line to move.
    NoSuchMove,
    /// The move is its parent's first reply already: it continues its line rather than start a
    /// variation (nothing to promote, and on the main line nothing to delete as a variation).
    AlreadyFirst,
    /// The move is a variation, not its parent's first reply: only a first reply is demoted.
    NotFirst,
    /// The move is its parent's only reply: there is no variation to take its place.
    NoAlternative,
    /// The change would nest a move deeper than [`MAX_DEPTH`].
    TooDeep,
    /// The variation is the first or the last one at its fork already: there is no variation on
    /// that side to change places with (the line itself is not one).
    NoSibling,
}

/// Which way [`Game::move_variation`] moves a variation among the others at its fork.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Shift {
    /// Before the variation before it.
    Up,
    /// After the variation after it.
    Down,
}

impl fmt::Display for StructureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StructureError::NoSuchMove => write!(f, "there is no such move"),
            StructureError::AlreadyFirst => write!(f, "the move already continues its line"),
            StructureError::NotFirst => write!(f, "the move is a variation, not the line's continuation"),
            StructureError::NoAlternative => write!(f, "there is no variation to take its place"),
            StructureError::TooDeep => write!(f, "variations would nest too deep"),
            StructureError::NoSibling => write!(f, "there is no variation on that side to change places with"),
        }
    }
}

impl std::error::Error for StructureError {}

impl Game {
    /// The moves from `node` up to the start, `node` first and the start left out (empty for
    /// the start). `None` when the game has no such move.
    pub(super) fn ancestry(&self, node: NodeId) -> Option<Vec<NodeId>> {
        if node >= self.nodes.len() {
            return None;
        }
        let mut up = Vec::new();
        let mut at = node;
        while at != 0 {
            // A tree has no cycles; the cap only keeps a broken one from looping.
            if up.len() >= self.nodes.len() {
                return None;
            }
            up.push(at);
            at = self.nodes.get(at)?.parent;
        }
        Some(up)
    }

    /// `node`'s parent and its index among the parent's replies; `None` for the start or a move
    /// not in the game.
    pub(super) fn reply_index(&self, node: NodeId) -> Option<(NodeId, usize)> {
        if node == 0 {
            return None;
        }
        let parent = self.nodes.get(node)?.parent;
        let index = self.nodes.get(parent)?.children.iter().position(|&c| c == node)?;
        Some((parent, index))
    }

    /// The path to move `node` from the start; `None` when the game has no such move.
    pub fn path_of(&self, node: NodeId) -> Option<NodePath> {
        let up = self.ancestry(node)?;
        let mut indices = Vec::with_capacity(up.len());
        for &at in up.iter().rev() {
            let (_, index) = self.reply_index(at)?;
            indices.push(u16::try_from(index).ok()?);
        }
        Some(NodePath(indices))
    }

    /// The move at the end of `path`; `None` when there is none.
    pub fn node_at(&self, path: &NodePath) -> Option<NodeId> {
        let mut at = 0;
        for &index in &path.0 {
            at = *self.nodes.get(at)?.children.get(usize::from(index))?;
        }
        Some(at)
    }

    /// The absolute half-move of the position after move `node` (the start's for node 0): the
    /// half-move the next move from it is, counted from move 1 whatever the start position. The
    /// move numbered `n` is half-move `2·(n−1)` when White plays it and `2·(n−1) + 1` when Black
    /// does, as [`Game::moves`] numbers them, so a move's own half-move is its parent's
    /// `abs_ply`. From `… b - - 0 18` the start's is 35, the half-move of `18… Qd8`. `None` when
    /// the game has no such move.
    pub fn abs_ply(&self, node: NodeId) -> Option<u32> {
        let board = &self.nodes.get(node)?.board;
        let black = u32::from(board.side_to_move() == cozy_chess::Color::Black);
        Some(u32::from(board.fullmove_number()).saturating_sub(1).saturating_mul(2).saturating_add(black))
    }

    /// The line through move `node`: the moves from the start (included) to `node`, then on
    /// along each first reply. Empty when the game has no such move.
    pub fn line_of(&self, node: NodeId) -> Vec<NodeId> {
        let Some(up) = self.ancestry(node) else { return Vec::new() };
        let mut line = Vec::with_capacity(up.len().saturating_add(1));
        line.push(0);
        line.extend(up.iter().rev());
        let mut at = node;
        while let Some(&next) = self.nodes.get(at).and_then(|n| n.children.first()) {
            // A tree has no cycles; the cap only keeps a broken one from looping.
            if line.len() >= self.nodes.len() {
                break;
            }
            line.push(next);
            at = next;
        }
        line
    }

    /// The main line: the start, then each first reply.
    pub fn main_line(&self) -> Vec<NodeId> {
        self.line_of(0)
    }

    /// The heads on the way from the start to move `node` (`node` included): the moves whose
    /// parent has two replies or more, nearest to `node` first. Empty for the start, a move not in
    /// the game, or a line without variations.
    pub fn heads_on_path(&self, node: NodeId) -> Vec<NodeId> {
        let Some(up) = self.ancestry(node) else { return Vec::new() };
        up.into_iter().filter(|&at| self.nodes.get(at).and_then(|n| self.nodes.get(n.parent)).is_some_and(|p| p.children.len() >= 2)).collect()
    }

    /// Promote variation `head` one level: it becomes its parent's first reply, the line it
    /// replaced the second, and the other replies keep their order. Refused when `head` already
    /// is a first reply, and when a move would nest deeper than [`MAX_DEPTH`].
    pub fn promote_one(&mut self, head: NodeId) -> Result<(), StructureError> {
        let (parent, index) = self.reply_index(head).ok_or(StructureError::NoSuchMove)?;
        if index == 0 {
            return Err(StructureError::AlreadyFirst);
        }
        self.move_reply(parent, index, 0)
    }

    /// Demote `head`, a first reply, to second: the variation that was second continues the line
    /// instead. Refused when `head` is not a first reply, when it is its parent's only reply, and
    /// when a move would nest deeper than [`MAX_DEPTH`]. It undoes [`Game::promote_one`] of a
    /// move that was second.
    pub fn demote(&mut self, head: NodeId) -> Result<(), StructureError> {
        let (parent, index) = self.reply_index(head).ok_or(StructureError::NoSuchMove)?;
        if index != 0 {
            return Err(StructureError::NotFirst);
        }
        if self.nodes.get(parent).map_or(0, |p| p.children.len()) < 2 {
            return Err(StructureError::NoAlternative);
        }
        self.move_reply(parent, 0, 1)
    }

    /// Move variation `head` one place among the variations at its fork: before the one before it
    /// ([`Shift::Up`]) or after the one after it ([`Shift::Down`]). The line the fork continues
    /// in (the first reply) never moves, and nothing else changes: the depths stay as they are, so
    /// no limit can be passed. Refused when `head` is the first reply ([`StructureError::AlreadyFirst`])
    /// and when there is no variation on that side ([`StructureError::NoSibling`]). Moving a
    /// variation up undoes moving it down, and the other way round.
    pub fn move_variation(&mut self, head: NodeId, shift: Shift) -> Result<(), StructureError> {
        let (parent, index) = self.reply_index(head).ok_or(StructureError::NoSuchMove)?;
        if index == 0 {
            return Err(StructureError::AlreadyFirst);
        }
        let replies = self.nodes.get(parent).map_or(0, |p| p.children.len());
        let to = match shift {
            // Index 1 would change places with the line, index 0.
            Shift::Up if index >= 2 => index - 1,
            Shift::Down if index.saturating_add(1) < replies => index + 1,
            _ => return Err(StructureError::NoSibling),
        };
        self.move_reply(parent, index, to)
    }

    /// Delete variation `head` and every move after it. Refused when `head` is a first reply
    /// (the line it continues is not a variation). The move shown goes back to `head`'s parent
    /// when it was among the moves deleted.
    pub fn delete_variation(&mut self, head: NodeId) -> Result<(), StructureError> {
        let (_, index) = self.reply_index(head).ok_or(StructureError::NoSuchMove)?;
        if index == 0 {
            return Err(StructureError::AlreadyFirst);
        }
        self.delete(head);
        Ok(())
    }

    /// Move `parent`'s reply at index `from` to index `to`, then compact. The game is left as it
    /// was when an index is out of range or a move would nest deeper than [`MAX_DEPTH`].
    fn move_reply(&mut self, parent: NodeId, from: usize, to: usize) -> Result<(), StructureError> {
        let mut changed = self.clone();
        let replies = &mut changed.nodes.get_mut(parent).ok_or(StructureError::NoSuchMove)?.children;
        if from >= replies.len() || to >= replies.len() {
            return Err(StructureError::NoSuchMove);
        }
        // Both indices were just checked against the length, which `remove` lowers by one: `to`
        // is at most that new length, as `insert` needs.
        let reply = replies.remove(from);
        replies.insert(to, reply);
        changed.compact();
        if changed.nodes.iter().any(|n| n.depth > MAX_DEPTH) {
            return Err(StructureError::TooDeep);
        }
        *self = changed;
        Ok(())
    }

    /// Make the revision newer than both its own and `seen`, and than any other game's so far
    /// (revisions are one counter for all games). After a snapshot is restored, its older
    /// revision would otherwise match a plan made since; this keeps revisions from repeating.
    /// Saturates at `u64::MAX`.
    pub fn bump_revision(&mut self, seen: u64) {
        self.revision = super::fresh_revision_past(self.revision.max(seen));
    }

    /// The comment before move `node` (always empty for the start, whose own comment comes before
    /// the first move).
    pub fn comment_before(&self, node: NodeId) -> &str {
        &self.node(node).comment_before
    }

    /// Set the comment before move `node`, cleaned as [`Game::set_comment`] does. It stays on the
    /// move whatever is promoted or demoted around it. PGN has room for it only after the `(` of
    /// a variation, so before a first reply it is written in one comment with the one after the
    /// move before, and read back as that one. Ignored for the start.
    pub fn set_comment_before(&mut self, node: NodeId, text: &str) {
        if node == 0 {
            return;
        }
        let clean = clean_comment(text);
        if let Some(n) = self.nodes.get_mut(node)
            && n.comment_before != clean
        {
            n.comment_before = clean;
            self.revision = super::fresh_revision();
        }
    }

    /// Where on a page move `node` was read, when it was.
    pub fn source(&self, node: NodeId) -> Option<NodeSource> {
        self.nodes.get(node).and_then(|n| n.source)
    }

    /// Record where on a page move `node` was read (`None` forgets it). Returns whether it was
    /// recorded: not for the start, a move not in the game, or a rectangle that is not finite.
    pub fn set_source(&mut self, node: NodeId, source: Option<NodeSource>) -> bool {
        if node == 0 || source.is_some_and(|s| !s.rect_user.iter().all(|v| v.is_finite())) {
            return false;
        }
        let Some(n) = self.nodes.get_mut(node) else { return false };
        if n.source != source {
            n.source = source;
            self.revision = super::fresh_revision();
        }
        true
    }

    /// Put glyph `code` on move `node` (unlike [`Game::toggle_nag`], never take it off). A
    /// move-quality glyph (codes 1–6) replaces the one there, as does an evaluation (10–21).
    /// Returns whether the move carries the glyph: not for the start, a move not in the game, a
    /// code outside 1–255, or a move that has as many glyphs as it may.
    pub fn set_nag(&mut self, node: NodeId, code: u16) -> bool {
        if node == 0 || code == 0 || code > 255 {
            return false;
        }
        let Some(n) = self.nodes.get_mut(node) else { return false };
        if n.nags.contains(&code) {
            return true;
        }
        let group = nag_group(code);
        let mut nags: Vec<u16> = n.nags.iter().copied().filter(|&c| group == 0 || nag_group(c) != group).collect();
        if nags.len() >= MAX_NAGS {
            return false;
        }
        nags.push(code);
        nags.sort_unstable();
        n.nags = nags;
        self.revision = super::fresh_revision();
        true
    }
}

#[cfg(test)]
mod tests;
