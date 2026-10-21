//! Playing a fragment read from a page into a game: the graft.
//!
//! [`Game::graft`] takes the items of a fragment ([`FragmentItem`]) and the move they go after,
//! and returns a changed copy of the game with a report of what went where. It never changes the
//! game it is called on, so a preview can graft, show the report and throw the copy away.
//!
//! What each item does:
//!
//! - **A move** is played after the last one. A reply already there with the same move is
//!   followed, never duplicated. A new one is a new reply after the others ([`GraftMode::Follow`]),
//!   or, in [`GraftMode::Line`] on the fragment's own line, the first reply, so the reply that
//!   was first becomes a variation (the report says `demoted`). Glyphs in its SAN suffix
//!   (`Nd5!?`) are set on it, and its rectangle becomes its [`NodeSource`] when the page is given
//!   and the move has no source yet.
//! - **A glyph** is set (not toggled) on the last move, by the code for the side it speaks of: a
//!   fragment carries White's code, and `↑` after a Black move is `$37`. Zugzwang speaks of the
//!   side to move, the others of the side that moved.
//! - **A comment** after a move is added to that move's comment, unless the comment already says
//!   it. A comment marked `before`, and any comment with no move before it in its line (at the
//!   start of the fragment or right after `(`), waits for the next move and becomes its
//!   comment-before.
//! - **`(`** starts an alternative to the last move: the moves that follow are played from that
//!   move's parent. With no move before it in its line, it starts an alternative to `at` (the
//!   move the fragment goes after) at the start of the fragment, and an alternative to the
//!   move that will follow otherwise (after another `(`, or when `at` is the start position,
//!   which has nothing to be an alternative to).
//! - **`)`** goes back to where its `(` was. A `)` without a `(`, and a `(` never closed, are in
//!   the report.
//! - **A result** ends the fragment.
//!
//! A null move (`--`, a threat shown by passing) is played as a move is, as [`Game::play_null`]
//! plays it.
//!
//! The graft stops at the first move that cannot be played (illegal, ambiguous, a null move by a
//! side in check, or past [`MAX_NODES`] or [`MAX_DEPTH`]), and after [`MAX_FRAGMENT_ITEMS`] items. What was not
//! used, a glyph or comment with no place included, is in the report as indices and as text: it
//! never vanishes. Grafting the same items twice changes nothing the second time.

use cozy_chess::Color as CozyColor;

use super::{Game, MAX_COMMENT_LEN, MAX_DEPTH, MAX_NODES, NodeId, NodePath, NodeSource, cut, tidy_comment};
use crate::fragment::{FragmentItem, MAX_FRAGMENT_ITEMS, items_text};
use crate::nag;
use crate::san::{SanBody, SanError, square_to_cozy};

/// Longest leftover text a report keeps, in bytes (cut at a character boundary). The leftover
/// items themselves are named by index.
pub const MAX_LEFTOVER_LEN: usize = 1 << 16;

/// How a fragment's moves go into the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GraftMode {
    /// As a variation: a reply already there is followed, a new one goes after the replies there.
    Follow,
    /// As the line: like [`GraftMode::Follow`], except that where the fragment's own line (not a
    /// parenthesized part of it) brings a new move to a move that has replies, the new move
    /// becomes the first reply and the old first reply a variation.
    Line,
    /// As text: the items are written out (moves in English SAN, with their numbers) and added
    /// to the comment of `at`, or to its comment-before when `at` starts a variation. No move is
    /// played.
    Comment,
}

/// Why the graft stopped before the end of the fragment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GraftStopReason {
    /// The move it was to go after is not in the game.
    NoAnchor,
    /// No legal move matches.
    Illegal,
    /// More than one legal move matches.
    Ambiguous,
    /// The move could not be read.
    Unreadable,
    /// The game would hold more than [`MAX_NODES`] moves, the fragment has more than
    /// [`MAX_FRAGMENT_ITEMS`] items, or a comment would pass [`MAX_COMMENT_LEN`]. A limit, not an
    /// illegal move.
    TooLong,
    /// Variations would nest deeper than [`MAX_DEPTH`].
    TooDeep,
    /// A null move (`--`) by the side to move in check, which cannot pass.
    NullMove,
}

/// Where and why the graft stopped.
#[derive(Clone, Debug, PartialEq)]
pub struct GraftStop {
    /// The item it stopped at; it and every item after it are left over.
    pub index: usize,
    pub reason: GraftStopReason,
    /// The item as printed.
    pub text: String,
    /// The move reads as a pawn taking en passant right after the start position. A position set
    /// up from a diagram never allows that (its FEN has no en passant square), which is the
    /// likely reason it is illegal.
    pub maybe_en_passant: bool,
}

/// What a graft did. Paths are in the grafted game.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GraftReport {
    /// Moves played, followed or new.
    pub played: usize,
    /// Why it stopped early; `None` when every item up to the end or a result was read.
    pub stop: Option<GraftStop>,
    /// The move the fragment went after.
    pub anchor: NodePath,
    /// The moves added, in the order they were played.
    pub new_nodes: Vec<NodePath>,
    /// A first reply became a variation ([`GraftMode::Line`]).
    pub demoted: bool,
    /// The fragment's first new move on its own line went after a move with no reply: it
    /// extended the tip of a line rather than branch from it.
    pub extended_tip: bool,
    /// Indices of the `)` without a `(` and of the `(` never closed.
    pub unbalanced: Vec<usize>,
    /// Indices of the moves whose printed number is not the half-move they were played at.
    pub ply_mismatches: Vec<usize>,
    /// Indices of the glyphs and comments that found no place (and so are left over).
    pub unplaced: Vec<usize>,
    /// The first item not read when reading ended early (at a stop, or after a result).
    pub rest_from: Option<usize>,
    /// The text of what is left over: the unplaced items, then the rest. At most
    /// [`MAX_LEFTOVER_LEN`] bytes.
    pub leftover_text: String,
    /// The last move played on the fragment's own line (the move the grafted game shows), or
    /// the last one played at all when the line had none.
    pub last: Option<NodePath>,
}

/// One `(` being read: where the line it left was.
struct Frame {
    cur: NodeId,
    last: Option<NodeId>,
    open: usize,
}

/// A comment waiting for the next move, with the items it came from.
#[derive(Default)]
struct Pending {
    text: String,
    items: Vec<usize>,
}

impl Game {
    /// Graft `items` after move `at` in `mode`, on a copy; returns the copy and what went where
    /// (see the [module](self) documentation). `page` is the page the items were read on: with
    /// it, a move's rectangle becomes its source. The copy shows the last move grafted on the
    /// fragment's own line.
    pub fn graft(&self, at: NodeId, mode: GraftMode, items: &[FragmentItem], page: Option<u32>) -> (Game, GraftReport) {
        let mut g = self.clone();
        let mut report = GraftReport::default();
        if at >= g.nodes.len() {
            report.stop = Some(GraftStop {
                index: 0,
                reason: GraftStopReason::NoAnchor,
                text: items.first().map(FragmentItem::text).unwrap_or_default(),
                maybe_en_passant: false,
            });
            report.rest_from = (!items.is_empty()).then_some(0);
            report.leftover_text = leftover_text(items, &[], report.rest_from);
            return (g, report);
        }
        if mode == GraftMode::Comment {
            g.graft_as_comment(at, items, &mut report);
        } else {
            g.graft_moves(at, mode == GraftMode::Line, items, page, &mut report);
        }
        report.anchor = g.path_of(at).unwrap_or_default();
        report.leftover_text = leftover_text(items, &report.unplaced, report.rest_from);
        (g, report)
    }

    /// The items as one comment on `at` (or before it, when it starts a variation).
    fn graft_as_comment(&mut self, at: NodeId, items: &[FragmentItem], report: &mut GraftReport) {
        // All or nothing: a fragment that does not fit is left over whole, never cut.
        let refuse = |report: &mut GraftReport| {
            if let Some(first) = items.first() {
                report.stop = Some(stop_at(0, first, GraftStopReason::TooLong, false));
                report.rest_from = Some(0);
            }
        };
        if items.len() > MAX_FRAGMENT_ITEMS {
            refuse(report);
            return;
        }
        let text = tidy_comment(&comment_text(items));
        if text.is_empty() {
            return;
        }
        let before = at != 0 && self.node(self.node(at).parent).children.first() != Some(&at);
        let existing = if before { self.comment_before(at) } else { self.comment(at) };
        if existing.contains(&text) {
            return;
        }
        let joined = if existing.is_empty() { text } else { format!("{existing} {text}") };
        if joined.len() > MAX_COMMENT_LEN {
            refuse(report);
            return;
        }
        if before {
            self.set_comment_before(at, &joined);
        } else {
            self.set_comment(at, &joined);
        }
    }

    /// Play the items after `at` (see [`Game::graft`]); `line`: [`GraftMode::Line`].
    fn graft_moves(&mut self, at: NodeId, line: bool, items: &[FragmentItem], page: Option<u32>, report: &mut GraftReport) {
        // The move the next one goes after, and the last move played in the line being read.
        let mut cur = at;
        let mut last: Option<NodeId> = None;
        let mut stack: Vec<Frame> = Vec::new();
        let mut pending = Pending::default();
        let mut line_last: Option<NodeId> = None;
        let mut any_last: Option<NodeId> = None;
        let mut new_ids: Vec<NodeId> = Vec::new();
        let mut diverged = false;
        for (index, item) in items.iter().enumerate() {
            if index >= MAX_FRAGMENT_ITEMS {
                report.stop = Some(stop_at(index, item, GraftStopReason::TooLong, false));
                report.rest_from = Some(index);
                break;
            }
            match item {
                FragmentItem::Result => {
                    if index.saturating_add(1) < items.len() {
                        report.rest_from = Some(index.saturating_add(1));
                    }
                    break;
                }
                FragmentItem::Open => {
                    let from = match last {
                        Some(m) => self.node(m).parent,
                        // The fragment opens with `(`: an alternative to `at`.
                        None if stack.is_empty() && at != 0 => self.node(at).parent,
                        None => cur,
                    };
                    stack.push(Frame { cur, last, open: index });
                    (cur, last) = (from, None);
                }
                FragmentItem::Close => {
                    // A comment after `(` with no move: its place is gone with the variation.
                    report.unplaced.append(&mut std::mem::take(&mut pending).items);
                    match stack.pop() {
                        Some(frame) => (cur, last) = (frame.cur, frame.last),
                        None => report.unbalanced.push(index),
                    }
                }
                FragmentItem::Nag(code) => {
                    let placed = last.is_some_and(|m| {
                        let white_moved = self.node(m).board.side_to_move() == CozyColor::Black;
                        self.set_nag(m, side_code(*code, white_moved))
                    });
                    if !placed {
                        report.unplaced.push(index);
                    }
                }
                FragmentItem::Comment { text, before } => {
                    let clean = tidy_comment(text);
                    if clean.is_empty() {
                        continue;
                    }
                    match last {
                        Some(m) if !before => {
                            if !self.add_comment(m, &clean, false) {
                                report.unplaced.push(index);
                            }
                        }
                        _ => {
                            if !pending.text.is_empty() {
                                pending.text.push(' ');
                            }
                            pending.text.push_str(&clean);
                            pending.items.push(index);
                        }
                    }
                }
                FragmentItem::Move { ply, san, rect, .. } => {
                    if ply.is_some_and(|p| self.abs_ply(cur) != Some(p)) {
                        report.ply_mismatches.push(index);
                    }
                    let parent = self.node(cur);
                    let white = parent.board.side_to_move() == CozyColor::White;
                    // The step to play: a move, or `None` for a null move.
                    let mv = match san.body {
                        SanBody::Null if parent.board.checkers().is_empty() => Ok(None),
                        SanBody::Null => Err(GraftStopReason::NullMove),
                        _ => san.to_move(&parent.board).map(Some).map_err(|e| match e {
                            SanError::Ambiguous(_) => GraftStopReason::Ambiguous,
                            SanError::NoSuchMove(_) | SanError::IllegalMove => GraftStopReason::Illegal,
                            _ => GraftStopReason::Unreadable,
                        }),
                    };
                    let existing = mv.ok().and_then(|mv| parent.children.iter().copied().find(|&c| self.node(c).mv == mv));
                    let node = match (mv, existing) {
                        (_, Some(node)) => node,
                        (Err(reason), None) => {
                            let ep = reason == GraftStopReason::Illegal && cur == 0 && reads_as_en_passant(self, san);
                            report.stop = Some(stop_at(index, item, reason, ep));
                            report.rest_from = Some(index);
                            break;
                        }
                        (Ok(mv), None) => {
                            let had_replies = !parent.children.is_empty();
                            let first = line && stack.is_empty() && had_replies;
                            match self.add_reply(cur, mv, first) {
                                Ok(node) => {
                                    report.demoted |= first;
                                    if stack.is_empty() && !diverged {
                                        diverged = true;
                                        report.extended_tip = !had_replies;
                                    }
                                    new_ids.push(node);
                                    node
                                }
                                Err(reason) => {
                                    report.stop = Some(stop_at(index, item, reason, false));
                                    report.rest_from = Some(index);
                                    break;
                                }
                            }
                        }
                    };
                    report.played = report.played.saturating_add(1);
                    for code in nag::nags_from_suffix(&san.suffix, white) {
                        self.set_nag(node, code);
                    }
                    let waiting = std::mem::take(&mut pending);
                    if !waiting.text.is_empty() && !self.add_comment(node, &waiting.text, true) {
                        report.unplaced.extend(waiting.items);
                    }
                    if let (Some(page), Some(rect_user)) = (page, rect)
                        && self.source(node).is_none()
                    {
                        // A rectangle that is not finite is not recorded; the move still is.
                        self.set_source(node, Some(NodeSource { page, rect_user: *rect_user }));
                    }
                    (cur, last) = (node, Some(node));
                    any_last = Some(node);
                    if stack.is_empty() {
                        line_last = Some(node);
                    }
                }
            }
        }
        report.unplaced.extend(pending.items);
        report.unplaced.sort_unstable();
        report.unbalanced.extend(stack.iter().map(|f| f.open));
        report.unbalanced.sort_unstable();
        report.new_nodes = new_ids.iter().filter_map(|&id| self.path_of(id)).collect();
        let shown = line_last.or(any_last);
        report.last = shown.and_then(|id| self.path_of(id));
        self.current = shown.unwrap_or(at);
    }

    /// Add `mv` (`None`: a null move) as a reply to `parent`: the last reply, or with `first` the
    /// first one (the reply that was first becomes the second, a variation, one level deeper with
    /// all its moves). Returns the new move, or why it could not be added (the game is then
    /// unchanged).
    fn add_reply(&mut self, parent: NodeId, mv: Option<cozy_chess::Move>, first: bool) -> Result<NodeId, GraftStopReason> {
        let p = self.node(parent);
        let old_first = p.children.first().copied();
        let depth = p.depth.saturating_add(usize::from(old_first.is_some() && !first));
        if self.nodes.len() >= MAX_NODES {
            return Err(GraftStopReason::TooLong);
        }
        let deepest = match (first, old_first) {
            (true, Some(old)) => self.subtree(old).iter().map(|&n| self.node(n).depth).max().unwrap_or(0).saturating_add(1),
            _ => depth,
        };
        if deepest > MAX_DEPTH || depth > MAX_DEPTH {
            return Err(GraftStopReason::TooDeep);
        }
        self.current = parent;
        // The limits were just checked; `push_step` checks them again and the move is legal.
        self.push_step(mv).map_err(|_| GraftStopReason::TooLong)?;
        let node = self.current;
        if first && let Some(old) = old_first {
            let moved = self.subtree(old);
            if let Some(p) = self.nodes.get_mut(parent)
                && let Some(new) = p.children.pop()
            {
                p.children.insert(0, new);
            }
            for n in moved {
                if let Some(n) = self.nodes.get_mut(n) {
                    n.depth = n.depth.saturating_add(1);
                }
            }
            if let Some(n) = self.nodes.get_mut(node) {
                n.depth = depth;
            }
        }
        Ok(node)
    }

    /// Move `node` and every move after it.
    fn subtree(&self, node: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack = vec![node];
        while let Some(at) = stack.pop() {
            // A tree has no cycles; the cap only keeps a broken one from looping.
            if at >= self.nodes.len() || out.len() >= self.nodes.len() {
                break;
            }
            out.push(at);
            stack.extend(self.node(at).children.iter().copied());
        }
        out
    }

    /// Add `text` (cleaned) to the comment after `node`, or with `before` to the one before it,
    /// unless it already says it. Returns whether the comment says it now: not when the two would
    /// not fit in [`MAX_COMMENT_LEN`] (nothing is cut).
    fn add_comment(&mut self, node: NodeId, text: &str, before: bool) -> bool {
        let existing = if before { self.comment_before(node) } else { self.comment(node) };
        if existing.contains(text) {
            return true;
        }
        let joined = if existing.is_empty() { text.to_owned() } else { format!("{existing} {text}") };
        if joined.len() > MAX_COMMENT_LEN {
            return false;
        }
        if before {
            self.set_comment_before(node, &joined);
        } else {
            self.set_comment(node, &joined);
        }
        true
    }
}

/// The code glyph `code` (White's, or any code PGN has) is written as on a move: by the side
/// that made it, or for zugzwang by the side to move after it. A code not in the table is kept.
fn side_code(code: u16, white_moved: bool) -> u16 {
    match nag::by_code(code) {
        Some(n) => n.code_for(if n.side_is_mover { white_moved } else { !white_moved }),
        None => code,
    }
}

fn stop_at(index: usize, item: &FragmentItem, reason: GraftStopReason, maybe_en_passant: bool) -> GraftStop {
    GraftStop { index, reason, text: item.text(), maybe_en_passant }
}

/// Whether `san` reads as a pawn taking en passant from the start position: a pawn capture onto
/// an empty square of the rank an en passant capture lands on.
fn reads_as_en_passant(game: &Game, san: &crate::san::San) -> bool {
    let SanBody::Move { piece: None, from_file: Some(file), to, promotion: None, .. } = san.body else { return false };
    let board = &game.node(0).board;
    let rank = if board.side_to_move() == CozyColor::White { 5 } else { 2 };
    file != to.file() && to.rank() == rank && square_to_cozy(to).is_some_and(|sq| board.piece_on(sq).is_none())
}

/// The items as a comment: moves in English SAN with their numbers when printed with one.
fn comment_text(items: &[FragmentItem]) -> String {
    let words: Vec<FragmentItem> = items
        .iter()
        .map(|item| match item {
            FragmentItem::Move { ply, san, .. } => {
                let move_text = san.to_english();
                let text = match ply {
                    Some(p) if p % 2 == 0 => format!("{}.{move_text}", p / 2 + 1),
                    Some(p) => format!("{}...{move_text}", p / 2 + 1),
                    None => move_text,
                };
                FragmentItem::Move { ply: *ply, san: san.clone(), text, bold: None, rect: None }
            }
            other => other.clone(),
        })
        .collect();
    items_text(&words)
}

/// The unplaced items, then the items from `rest_from` on, as text, cut to [`MAX_LEFTOVER_LEN`].
fn leftover_text(items: &[FragmentItem], unplaced: &[usize], rest_from: Option<usize>) -> String {
    let unplaced_items = unplaced.iter().filter_map(|&i| items.get(i));
    let rest = rest_from.and_then(|from| items.get(from..)).unwrap_or_default();
    // Each item adds at least one byte; stop reading once the cap is passed.
    let rest = rest.get(..MAX_LEFTOVER_LEN.min(rest.len())).unwrap_or_default();
    let text = items_text(unplaced_items.chain(rest.iter()));
    cut(&text, MAX_LEFTOVER_LEN).to_owned()
}

#[cfg(test)]
mod tests;
