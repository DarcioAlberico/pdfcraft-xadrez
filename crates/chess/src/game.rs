//! A game played out from a position: the moves made on an analysis board with their variations,
//! comments and annotation glyphs, the move being looked at, and the game as PGN.
//!
//! The moves are a tree. Each move's first reply is the line it continues in; the others are
//! variations. A move made where a reply already exists is a new variation, unless it is one of
//! the replies already there, which is then just followed. Moves are named by their squares, the
//! way a board is clicked: castling is the king moving two files (or onto its own rook), and a
//! pawn reaching the last rank needs the piece it becomes.
//!
//! Everything is bounded: the moves in a game ([`MAX_NODES`]), how deep variations nest
//! ([`MAX_DEPTH`]), the PGN read ([`MAX_PGN_LEN`]), comments and tags.

use std::fmt;

use cozy_chess::{Board, GameStatus, Move};

use crate::fen::Position;
use crate::placement::{Color, Piece, PieceKind, Placement, Square};
use crate::san::{Lang, San, SanBody, color_from_cozy, kind_from_cozy, square_from_cozy, square_to_cozy, to_cozy_kind};

mod drawing;
mod graft;
mod line;
mod structure;

pub use drawing::{DrawColor, Drawing, MAX_DRAWINGS, drawing_commands, split_drawings};
pub use graft::{GraftMode, GraftReport, GraftStop, GraftStopReason, MAX_LEFTOVER_LEN};
pub use structure::{NodePath, NodeSource, Shift, StructureError};

/// Most moves a game holds, variations included. Real annotated games are under 3,000.
pub const MAX_NODES: usize = 10_000;

/// The highest number a game's first move can be given ([`Game::set_first_move_number`]).
pub const MAX_MOVE_NUMBER: u32 = 999;

/// Deepest nesting of variations (a variation inside a variation…).
pub const MAX_DEPTH: usize = 32;

/// Longest PGN read, in bytes.
pub const MAX_PGN_LEN: usize = 1 << 20;

/// Longest comment kept, in bytes (cut at a character boundary).
pub const MAX_COMMENT_LEN: usize = 4096;

/// Most tags kept, and the longest tag name and value.
pub const MAX_TAGS: usize = 64;
pub const MAX_TAG_LEN: usize = 256;

/// Most annotation glyphs on one move.
const MAX_NAGS: usize = 8;

/// How a null move is written, in the game and in PGN.
pub const NULL_SAN: &str = "--";

/// The Seven Tag Roster, in the order PGN writes it.
pub const ROSTER: [&str; 7] = ["Event", "Site", "Date", "Round", "White", "Black", "Result"];

/// A move in the game (the start position is node 0).
pub type NodeId = usize;

/// Why a game could not be set up or a move made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GameError {
    /// The position is not legal (two kings, the side not to move in check…), so no move can be
    /// played from it. Why, as [`crate::rules::position_violations`] names it (empty when the
    /// rule broken has no name there).
    IllegalPosition(Vec<crate::rules::Violation>),
    /// The FEN could not be read.
    BadFen(String),
    /// That move is not legal here.
    IllegalMove,
    /// A pawn reaches the last rank: say what it becomes.
    NeedsPromotion,
    /// The game holds as many moves as a game may.
    TooLong,
    /// The move would start a variation nested deeper than [`MAX_DEPTH`].
    TooDeep,
    /// The PGN is longer than [`MAX_PGN_LEN`].
    PgnTooLong,
    /// A tag name that PGN does not allow (letters, digits and `_` only), or one too long.
    BadTag,
}

impl fmt::Display for GameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GameError::IllegalPosition(why) => {
                write!(f, "the position is not legal")?;
                for (i, v) in why.iter().enumerate() {
                    write!(f, "{}{v}", if i == 0 { ": " } else { "; " })?;
                }
                Ok(())
            }
            GameError::BadFen(e) => write!(f, "the FEN could not be read: {e}"),
            GameError::IllegalMove => write!(f, "that move is not legal"),
            GameError::NeedsPromotion => write!(f, "the pawn needs a piece to become"),
            GameError::TooLong => write!(f, "the game is too long"),
            GameError::TooDeep => write!(f, "variations would nest too deep (at most {MAX_DEPTH})"),
            GameError::PgnTooLong => write!(f, "the PGN is too long"),
            GameError::BadTag => write!(f, "a tag name has letters, digits and _ only"),
        }
    }
}

impl std::error::Error for GameError {}

/// How the game stands at the move shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Ongoing,
    /// The side to move is mated.
    Checkmate,
    Stalemate,
    /// Fifty moves without a capture or a pawn move.
    FiftyMoves,
}

/// One piece of the move text, in PGN order: what a move list shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Token {
    /// A move. `number` is shown before it when `numbered` (always for White; for Black at the
    /// start of a line and after a comment or a variation). `depth` is how deep in variations it
    /// is (0: the main line).
    Move {
        node: NodeId,
        number: u32,
        color: Color,
        numbered: bool,
        san: String,
        nags: Vec<u16>,
        depth: usize,
    },
    /// A comment after move `node` (on node 0: before the first move), or, with `before`, the
    /// comment before move `node`. A comment before a first reply comes where PGN has room for
    /// it, right after the comment on the move before (so before that move's variations); one
    /// before a variation's first move comes right after its `(`.
    Comment {
        node: NodeId,
        text: String,
        before: bool,
    },
    VariationStart,
    VariationEnd,
}

#[derive(Clone, Debug)]
struct Node {
    /// The move that led here (`None` for the start).
    mv: Option<Move>,
    san: String,
    /// The board after the move.
    board: Board,
    parent: NodeId,
    /// The replies: the first continues the line, the others are variations.
    children: Vec<NodeId>,
    nags: Vec<u16>,
    comment: String,
    /// The comment before the move (none on the start, whose own comment already comes before
    /// the first move).
    comment_before: String,
    /// The drawings on the position after the move (squares first, then arrows), written into
    /// the comment after it.
    drawings: Vec<Drawing>,
    /// Where on a page the move was read, when it was.
    source: Option<NodeSource>,
    /// Moves from the start.
    ply: usize,
    /// How deep in variations (0: the main line).
    depth: usize,
}

impl Node {
    fn start(board: Board) -> Node {
        Node {
            mv: None,
            san: String::new(),
            board,
            parent: 0,
            children: Vec::new(),
            nags: Vec::new(),
            comment: String::new(),
            comment_before: String::new(),
            drawings: Vec::new(),
            source: None,
            ply: 0,
            depth: 0,
        }
    }
}

/// A game from a start position.
#[derive(Clone, Debug)]
pub struct Game {
    /// Node 0 is the start; it is never removed.
    nodes: Vec<Node>,
    current: NodeId,
    tags: Vec<(String, String)>,
    /// Counts the changes to the moves, comments, glyphs and tags (not to the move shown).
    revision: u64,
    /// The furthest move of the line last stepped back along: while it comes after the move
    /// shown, a step forward goes towards it rather than along the first reply, so stepping back
    /// out of a variation and forward again returns into it. Not part of the PGN.
    tip: NodeId,
}

impl Default for Game {
    fn default() -> Game {
        Game::standard()
    }
}

impl Game {
    /// A game from the standard start position.
    pub fn standard() -> Game {
        Game::from_board(Board::default())
    }

    fn from_board(board: Board) -> Game {
        Game { nodes: vec![Node::start(board)], current: 0, tags: Vec::new(), revision: 0, tip: 0 }
    }

    /// A game from `position`, `side` to move when the position does not say.
    pub fn new(position: &Position, side: Color) -> Result<Game, GameError> {
        use crate::rules::{Violation, position_violations};
        let board = position.to_board(side).map_err(|_| GameError::IllegalPosition(position_violations(position, side)))?;
        // The board takes castling rights a moved king or rook no longer allows (it reads them as
        // Chess960's); standard chess does not.
        let castling: Vec<Violation> = position_violations(position, side).into_iter().filter(|v| matches!(v, Violation::BadCastling(_))).collect();
        if !castling.is_empty() {
            return Err(GameError::IllegalPosition(castling));
        }
        Ok(Game::from_board(board))
    }

    /// A game from a FEN (only the placement is required; White moves when it does not say).
    pub fn from_fen(fen: &str) -> Result<Game, GameError> {
        let position = crate::fen::parse(fen.trim()).map_err(|e| GameError::BadFen(e.to_string()))?;
        Game::new(&position, Color::White)
    }

    /// Node `id`, the start when there is none (ids come from this game, so never).
    fn node(&self, id: NodeId) -> &Node {
        match self.nodes.get(id).or_else(|| self.nodes.first()) {
            Some(node) => node,
            None => unreachable_node(),
        }
    }

    fn board(&self) -> &Board {
        &self.node(self.current).board
    }

    /// The move shown (0: the start).
    pub fn current(&self) -> NodeId {
        self.current
    }

    /// Changes with every change to the moves, comments, glyphs or tags.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Show move `node` (ignored when the game has no such move).
    pub fn go_to_node(&mut self, node: NodeId) {
        if node < self.nodes.len() {
            self.current = node;
        }
    }

    /// The line shown: the moves from the start to the move shown, then on to the line's
    /// remembered move (see [`Game::forward`]) and along each first reply after it.
    fn line(&self) -> Vec<NodeId> {
        self.line_of(if self.remembers(self.current) { self.tip } else { self.current })
    }

    /// Whether the remembered move comes after move `node` (not `node` itself).
    fn remembers(&self, node: NodeId) -> bool {
        self.tip != node && self.tip < self.nodes.len() && self.is_after(self.tip, node)
    }

    /// Moves from the start to the move shown.
    pub fn ply(&self) -> usize {
        self.node(self.current).ply
    }

    /// How many moves the line shown has.
    pub fn len(&self) -> usize {
        self.line().len().saturating_sub(1)
    }

    /// Whether no move was made at all.
    pub fn is_empty(&self) -> bool {
        self.nodes.len() <= 1
    }

    /// Show the move `ply` moves into the line shown (clamped to it).
    pub fn go_to(&mut self, ply: usize) {
        let line = self.line();
        if let Some(&node) = line.get(ply).or(line.last()) {
            self.current = node;
        }
    }

    /// One move back; `false` at the start. The line stepped back along is remembered, for
    /// [`Game::forward`].
    pub fn back(&mut self) -> bool {
        if self.current == 0 {
            return false;
        }
        if !self.is_after(self.tip, self.current) {
            self.tip = self.current;
        }
        self.current = self.node(self.current).parent;
        true
    }

    /// One move forward; `false` at the end of the line. After stepping back, forward retraces
    /// the moves stepped back over (into a variation, when that is where they were); otherwise
    /// it takes the first reply.
    pub fn forward(&mut self) -> bool {
        match self.next_reply(self.current) {
            Some(next) => {
                self.current = next;
                true
            }
            None => false,
        }
    }

    /// The reply a step forward from move `node` takes: the one towards the remembered move
    /// when that comes after `node`, else the first. `None` at the end of a line.
    pub fn next_reply(&self, node: NodeId) -> Option<NodeId> {
        if self.remembers(node) {
            let mut at = self.tip;
            // `is_after` walked this way already, so it reaches `node`.
            while self.node(at).parent != node && at != 0 {
                at = self.node(at).parent;
            }
            return Some(at);
        }
        self.nodes.get(node)?.children.first().copied()
    }

    /// The replies to move `node`, in order: the first continues its line, the others are
    /// variations. Empty at the end of a line and for a move not in the game.
    pub fn replies(&self, node: NodeId) -> &[NodeId] {
        self.nodes.get(node).map_or(&[], |n| n.children.as_slice())
    }

    /// Step forward into reply `index` of the move shown (0: the line, 1: its first variation…)
    /// and remember that line; `false` when there is no such reply.
    pub fn forward_to(&mut self, index: usize) -> bool {
        let Some(&next) = self.node(self.current).children.get(index) else { return false };
        if !self.is_after(self.tip, next) {
            self.tip = next;
        }
        self.current = next;
        true
    }

    /// Show the last move of the main line.
    pub fn go_to_main_end(&mut self) {
        if let Some(&last) = self.main_line().last() {
            self.current = last;
        }
    }

    /// The pieces shown.
    pub fn placement(&self) -> Placement {
        placement_of(self.board())
    }

    /// The FEN of the board shown.
    pub fn fen(&self) -> String {
        self.board().to_string()
    }

    /// The FEN of the start.
    pub fn start_fen(&self) -> String {
        self.node(0).board.to_string()
    }

    /// The number of the move the start stands at (its FEN's move number): 35 for a game that
    /// starts at 35. or 35…
    pub fn first_move_number(&self) -> u32 {
        u32::from(self.node(0).board.fullmove_number())
    }

    /// Number the game from `number` (the move the start stands at, as the book prints it), when
    /// its reading missed or misread it: every move keeps its place and is renumbered with the
    /// start. `number` is kept within 1..=[`MAX_MOVE_NUMBER`]. Returns whether anything changed.
    pub fn set_first_move_number(&mut self, number: u32) -> bool {
        let number = number.clamp(1, MAX_MOVE_NUMBER);
        let old = self.first_move_number();
        if number == old {
            return false;
        }
        for n in &mut self.nodes {
            // A move's number only grows from the start's; checked anyway, never below 1.
            let after = u32::from(n.board.fullmove_number()).saturating_sub(old);
            let renumbered = u16::try_from(number.saturating_add(after)).unwrap_or(u16::MAX).max(1);
            n.board.set_fullmove_number(renumbered);
        }
        self.revision = self.revision.wrapping_add(1);
        true
    }

    /// The FEN of the position after move `node` (the start's for node 0), whatever move is
    /// shown; `None` when the game has no such move.
    pub fn fen_at(&self, node: NodeId) -> Option<String> {
        self.nodes.get(node).map(|n| n.board.to_string())
    }

    /// Whose move it is on the board shown.
    pub fn side_to_move(&self) -> Color {
        color_from_cozy(self.board().side_to_move())
    }

    /// Whether the side to move is in check.
    pub fn in_check(&self) -> bool {
        !self.board().checkers().is_empty()
    }

    /// Whether the side to move after move `node` (at the start for node 0) is in check, whatever
    /// move is shown; `false` for a move not in the game.
    pub fn in_check_at(&self, node: NodeId) -> bool {
        self.nodes.get(node).is_some_and(|n| !n.board.checkers().is_empty())
    }

    /// How the game stands on the board shown.
    pub fn outcome(&self) -> Outcome {
        outcome_of(self.board())
    }

    /// The legal moves on the board shown, as (from, to) the way they are clicked: castling is the
    /// king's two-file step.
    fn clicks(&self) -> Vec<(Square, Square, Move)> {
        let board = self.board();
        let mut out = Vec::new();
        board.generate_moves(|piece_moves| {
            for mv in piece_moves {
                let (from, to) = click_of(board, mv);
                out.push((from, to, mv));
            }
            false
        });
        out
    }

    /// The squares the piece on `from` can go to.
    pub fn targets(&self, from: Square) -> Vec<Square> {
        let mut out: Vec<Square> = self.clicks().into_iter().filter(|(f, _, _)| *f == from).map(|(_, t, _)| t).collect();
        out.sort();
        out.dedup();
        out
    }

    /// The legal moves on the board shown, in SAN, sorted.
    pub fn legal_moves(&self) -> Vec<String> {
        let board = self.board();
        let mut out: Vec<String> =
            self.clicks().into_iter().filter_map(|(_, _, mv)| San::from_move(board, mv).ok().map(|san| san.to_english())).collect();
        out.sort();
        out
    }

    /// Whether moving `from` → `to` is a promotion (and so needs a piece).
    pub fn is_promotion(&self, from: Square, to: Square) -> bool {
        self.clicks().iter().any(|(f, t, mv)| *f == from && *t == to && mv.promotion.is_some())
    }

    /// The piece on `square` of the board shown.
    pub fn piece_on(&self, square: Square) -> Option<Piece> {
        self.placement().get(square)
    }

    /// Play `from` → `to` at the move shown (`promotion` for a pawn reaching the last rank). A
    /// reply already there is followed; a new one continues the line when there was none, else it
    /// is a new variation.
    pub fn play(&mut self, from: Square, to: Square, promotion: Option<PieceKind>) -> Result<(), GameError> {
        let candidates: Vec<Move> = self.clicks().into_iter().filter(|(f, t, _)| *f == from && *t == to).map(|(_, _, mv)| mv).collect();
        if candidates.is_empty() {
            // The king onto its own rook is castling too (cozy-chess's own notation).
            let (Some(f), Some(t)) = (square_to_cozy(from), square_to_cozy(to)) else { return Err(GameError::IllegalMove) };
            let mv = Move { from: f, to: t, promotion: None };
            return if self.board().is_legal(mv) { self.push(mv) } else { Err(GameError::IllegalMove) };
        }
        // A piece given for a move that is not a promotion is ignored.
        let mv = match (candidates.as_slice(), promotion) {
            ([mv], _) if mv.promotion.is_none() => *mv,
            (_, Some(kind)) => {
                let want = to_cozy_kind(kind);
                candidates.into_iter().find(|mv| mv.promotion == Some(want)).ok_or(GameError::IllegalMove)?
            }
            (_, None) => return Err(GameError::NeedsPromotion),
        };
        self.push(mv)
    }

    /// Play a move written in SAN (English letters, or as books print it) at the move shown. A
    /// null move (`--`, or `Z0` and `0000` as some programs write it) is played as
    /// [`Game::play_null`] plays it.
    pub fn play_san(&mut self, text: &str) -> Result<(), GameError> {
        if San::read(text, Lang::En).is_ok_and(|san| san.body == SanBody::Null) {
            return self.play_null();
        }
        let san = San::parse(text).map_err(|_| GameError::IllegalMove)?;
        let mv = san.to_move(self.board()).map_err(|_| GameError::IllegalMove)?;
        self.push(mv)
    }

    /// Play a move in UCI notation (`e2e4`, `e7e8q`; castling as `e1g1` or the king onto its rook).
    pub fn play_uci(&mut self, text: &str) -> Result<(), GameError> {
        let text = text.trim();
        let (from, to) = (text.get(0..2).and_then(Square::parse), text.get(2..4).and_then(Square::parse));
        let (Some(from), Some(to)) = (from, to) else { return Err(GameError::IllegalMove) };
        let promotion = match text.get(4..) {
            None | Some("") => None,
            Some(p) => Some(match p.to_ascii_lowercase().as_str() {
                "q" => PieceKind::Queen,
                "r" => PieceKind::Rook,
                "b" => PieceKind::Bishop,
                "n" => PieceKind::Knight,
                _ => return Err(GameError::IllegalMove),
            }),
        };
        self.play(from, to, promotion)
    }

    fn push(&mut self, mv: Move) -> Result<(), GameError> {
        self.push_step(Some(mv))
    }

    /// Play a null move at the move shown: the side to move passes, as annotators do to show
    /// what the other side threatens (`12... --`). It is written `--` in PGN, as most PGN tools
    /// read and write it. Refused when the side to move is in check (it cannot pass), and at the
    /// limits [`Game::play`] has. A null move already there is followed.
    pub fn play_null(&mut self) -> Result<(), GameError> {
        self.push_step(None)
    }

    /// Whether move `node` is a null move (a pass). The start is not.
    pub fn is_null(&self, node: NodeId) -> bool {
        node != 0 && self.nodes.get(node).is_some_and(|n| n.mv.is_none())
    }

    /// Play `step` at the move shown: a move, or with `None` a null move. A reply already there
    /// with the same step is followed.
    fn push_step(&mut self, step: Option<Move>) -> Result<(), GameError> {
        let here = self.current;
        // Every node but the start has a parent, so a reply without a move is a null move.
        if let Some(&existing) = self.node(here).children.iter().find(|&&c| self.node(c).mv == step) {
            self.current = existing;
            return Ok(());
        }
        let parent = self.node(here);
        let depth = parent.depth.saturating_add(usize::from(!parent.children.is_empty()));
        if depth > MAX_DEPTH {
            return Err(GameError::TooDeep);
        }
        if self.nodes.len() >= MAX_NODES {
            return Err(GameError::TooLong);
        }
        let board = &parent.board;
        let (san, after) = match step {
            Some(mv) => {
                let san = San::from_move(board, mv).map_err(|_| GameError::IllegalMove)?.to_english();
                let mut after = board.clone();
                after.try_play(mv).map_err(|_| GameError::IllegalMove)?;
                (san, after)
            }
            // `null_move` is `None` when the side to move is in check.
            None => (NULL_SAN.to_owned(), board.null_move().ok_or(GameError::IllegalMove)?),
        };
        let id = self.nodes.len();
        let ply = parent.ply.saturating_add(1);
        self.nodes.push(Node {
            mv: step,
            san,
            board: after,
            parent: here,
            children: Vec::new(),
            nags: Vec::new(),
            comment: String::new(),
            comment_before: String::new(),
            drawings: Vec::new(),
            source: None,
            ply,
            depth,
        });
        if let Some(p) = self.nodes.get_mut(here) {
            p.children.push(id);
        }
        self.current = id;
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }

    /// Delete the replies to the move shown (and everything after them).
    pub fn truncate(&mut self) {
        if let Some(n) = self.nodes.get_mut(self.current)
            && !n.children.is_empty()
        {
            n.children.clear();
            self.compact();
        }
    }

    /// Delete move `node` and everything after it (not the start). The move shown goes back to
    /// `node`'s parent when it was among them.
    pub fn delete(&mut self, node: NodeId) {
        if node == 0 || node >= self.nodes.len() {
            return;
        }
        let parent = self.node(node).parent;
        if self.is_after(self.current, node) {
            self.current = parent;
        }
        if let Some(p) = self.nodes.get_mut(parent) {
            p.children.retain(|&c| c != node);
        }
        self.compact();
    }

    /// Make the line through `node` the main line: at each move on the way from the start it
    /// becomes the first reply. The old main line becomes a variation, one level deeper with
    /// everything under it; when that would nest a move deeper than [`MAX_DEPTH`] (where it could
    /// no longer be written out), the game is left as it is.
    pub fn promote(&mut self, node: NodeId) {
        if node >= self.nodes.len() {
            return;
        }
        let mut promoted = self.clone();
        let mut at = node;
        while at != 0 {
            let parent = promoted.node(at).parent;
            if let Some(p) = promoted.nodes.get_mut(parent)
                && let Some(i) = p.children.iter().position(|&c| c == at)
            {
                let child = p.children.remove(i);
                p.children.insert(0, child);
            }
            at = parent;
        }
        promoted.compact();
        if promoted.nodes.iter().all(|n| n.depth <= MAX_DEPTH) {
            *self = promoted;
        }
    }

    /// Whether `node` is `ancestor` or comes after it.
    fn is_after(&self, mut node: NodeId, ancestor: NodeId) -> bool {
        loop {
            if node == ancestor {
                return true;
            }
            if node == 0 {
                return false;
            }
            node = self.node(node).parent;
        }
    }

    /// Drop the moves no longer reachable from the start, renumber the rest depth first (first
    /// replies first), and recompute how deep each one is in variations.
    fn compact(&mut self) {
        let mut order: Vec<NodeId> = Vec::with_capacity(self.nodes.len());
        let mut stack = vec![0];
        while let Some(id) = stack.pop() {
            if id >= self.nodes.len() || order.len() >= MAX_NODES {
                continue;
            }
            order.push(id);
            // Reversed, so the first reply is visited first.
            stack.extend(self.node(id).children.iter().rev().copied());
        }
        let mut new_id = vec![usize::MAX; self.nodes.len()];
        for (new, &old) in order.iter().enumerate() {
            if let Some(slot) = new_id.get_mut(old) {
                *slot = new;
            }
        }
        let map = |id: NodeId| new_id.get(id).copied().filter(|&n| n != usize::MAX);
        let mut nodes: Vec<Node> = order.iter().filter_map(|&old| self.nodes.get(old).cloned()).collect();
        for n in &mut nodes {
            n.parent = map(n.parent).unwrap_or(0);
            n.children = n.children.iter().filter_map(|&c| map(c)).collect();
        }
        // Depths: the parent comes before its children in `order`.
        for i in 1..nodes.len() {
            let Some(parent) = nodes.get(i).map(|n| n.parent) else { continue };
            let (pdepth, first) = nodes.get(parent).map_or((0, true), |p| (p.depth, p.children.first() == Some(&i)));
            if let Some(n) = nodes.get_mut(i) {
                n.depth = pdepth.saturating_add(usize::from(!first));
            }
        }
        self.current = map(self.current).unwrap_or(0);
        self.tip = map(self.tip).unwrap_or(self.current);
        self.nodes = nodes;
        self.revision = self.revision.wrapping_add(1);
    }

    /// The comment after move `node` (on node 0: before the first move).
    pub fn comment(&self, node: NodeId) -> &str {
        &self.node(node).comment
    }

    /// Set the comment after move `node`, cut to [`MAX_COMMENT_LEN`]. Braces, which end a PGN
    /// comment, become parentheses. Drawing commands in it (`[%csl …]`, `[%cal …]`) are taken
    /// out and drawn on the move's position, as reading them from a PGN does
    /// ([`split_drawings`]): the comment's text never holds a drawing the move could hold, so
    /// the PGN written is read back the same.
    pub fn set_comment(&mut self, node: NodeId, text: &str) {
        let Some(n) = self.nodes.get_mut(node) else { return };
        let mut drawings = n.drawings.clone();
        let clean = clean_comment(&split_drawings(text, &mut drawings));
        if n.comment != clean || n.drawings != drawings {
            n.comment = clean;
            n.drawings = drawings;
            self.revision = self.revision.wrapping_add(1);
        }
    }

    /// The annotation glyphs on move `node` (PGN codes).
    pub fn nags(&self, node: NodeId) -> &[u16] {
        &self.node(node).nags
    }

    /// Put glyph `code` on move `node`, or take it off. A move-quality glyph (`!`, `?`, …: codes
    /// 1–6) replaces the one there, as does an evaluation (codes 10–21).
    pub fn toggle_nag(&mut self, node: NodeId, code: u16) {
        if node == 0 || code == 0 || code > 255 {
            return;
        }
        let Some(n) = self.nodes.get_mut(node) else { return };
        if let Some(i) = n.nags.iter().position(|&c| c == code) {
            n.nags.remove(i);
        } else {
            let g = nag_group(code);
            if g != 0 {
                n.nags.retain(|&c| nag_group(c) != g);
            }
            if n.nags.len() < MAX_NAGS {
                n.nags.push(code);
                n.nags.sort_unstable();
            }
        }
        self.revision = self.revision.wrapping_add(1);
    }

    /// The SAN of move `node` (empty for the start).
    pub fn san(&self, node: NodeId) -> &str {
        &self.node(node).san
    }

    /// The move that led to the board shown, as (from, to) clicked.
    pub fn last_move(&self) -> Option<(Square, Square)> {
        let node = self.node(self.current);
        let mv = node.mv?;
        Some(click_of(&self.node(node.parent).board, mv))
    }

    /// The moves of the line shown in SAN, each with its number and whose it was: `(ply, number,
    /// color, san)`, `ply` being the board after the move.
    pub fn moves(&self) -> Vec<(usize, u32, Color, &str)> {
        self.line()
            .into_iter()
            .skip(1)
            .map(|id| {
                let n = self.node(id);
                let before = &self.node(n.parent).board;
                (n.ply, u32::from(before.fullmove_number()), color_from_cozy(before.side_to_move()), n.san.as_str())
            })
            .collect()
    }

    /// The move text in PGN order: moves, comments and variations.
    pub fn tokens(&self) -> Vec<Token> {
        let mut out = Vec::new();
        let root = self.node(0);
        if !root.comment.is_empty() || !root.drawings.is_empty() {
            out.push(Token::Comment { node: 0, text: root.comment.clone(), before: false });
        }
        self.emit_first_reply_before(0, &mut out);
        self.emit_line(0, true, &mut out, 0);
        out
    }

    /// Write the line that goes on from `parent`, with the variations at each move.
    fn emit_line(&self, mut parent: NodeId, mut numbered: bool, out: &mut Vec<Token>, nesting: usize) {
        if nesting > MAX_DEPTH {
            return;
        }
        loop {
            let children = &self.node(parent).children;
            let Some(&main) = children.first() else { break };
            numbered = self.emit_move(main, numbered, out);
            for &alt in children.iter().skip(1) {
                out.push(Token::VariationStart);
                let alt_node = self.node(alt);
                if !alt_node.comment_before.is_empty() {
                    out.push(Token::Comment { node: alt, text: alt_node.comment_before.clone(), before: true });
                }
                let after = self.emit_move(alt, true, out);
                self.emit_line(alt, after, out, nesting.saturating_add(1));
                out.push(Token::VariationEnd);
                numbered = true;
            }
            parent = main;
        }
    }

    /// One move, its comment and the comment before its first reply. Returns whether the next
    /// move needs its number.
    fn emit_move(&self, id: NodeId, numbered: bool, out: &mut Vec<Token>) -> bool {
        let n = self.node(id);
        let before = &self.node(n.parent).board;
        let color = color_from_cozy(before.side_to_move());
        out.push(Token::Move {
            node: id,
            number: u32::from(before.fullmove_number()),
            color,
            numbered: numbered || color == Color::White,
            san: n.san.clone(),
            nags: n.nags.clone(),
            depth: n.depth,
        });
        // Drawings are written in the comment, so a move with drawings has one.
        let commented = !n.comment.is_empty() || !n.drawings.is_empty();
        if commented {
            out.push(Token::Comment { node: id, text: n.comment.clone(), before: false });
        }
        self.emit_first_reply_before(id, out) || commented
    }

    /// The comment before `id`'s first reply, which PGN can only hold right after `id`'s own
    /// comment. Returns whether there was one.
    fn emit_first_reply_before(&self, id: NodeId, out: &mut Vec<Token>) -> bool {
        let Some(&first) = self.node(id).children.first() else { return false };
        let text = &self.node(first).comment_before;
        if text.is_empty() {
            return false;
        }
        out.push(Token::Comment { node: first, text: text.clone(), before: true });
        true
    }

    /// The moves as PGN movetext: `1. e4 e5 (1... c5 {Sicilian}) 2. Nf3 $1`. Comments next to
    /// each other (a move's own and the one before its first reply) are written as one, the way
    /// PGN reads them back, so the text read and written again is the same (while the two
    /// together fit in [`MAX_COMMENT_LEN`], which is all of a comment the reader keeps).
    pub fn movetext(&self) -> String {
        let mut out = String::new();
        let mut glue = true; // no space before the next piece
        let mut after_comment = false;
        for token in self.tokens() {
            if let Token::Comment { node, text, before } = &token
                && after_comment
                && out.ends_with('}')
            {
                out.pop();
                out.push(' ');
                out.push_str(&self.comment_body(*node, text, *before));
                out.push('}');
                continue;
            }
            after_comment = matches!(token, Token::Comment { .. });
            let piece = match &token {
                Token::Move { number, color, numbered, san, nags, .. } => {
                    let mut s = String::new();
                    if *numbered {
                        s.push_str(&match color {
                            Color::White => format!("{number}. "),
                            Color::Black => format!("{number}... "),
                        });
                    }
                    s.push_str(san);
                    for code in nags {
                        s.push_str(&format!(" ${code}"));
                    }
                    s
                }
                Token::Comment { node, text, before } => format!("{{{}}}", self.comment_body(*node, text, *before)),
                Token::VariationStart => "(".to_owned(),
                Token::VariationEnd => ")".to_owned(),
            };
            if !(glue || matches!(token, Token::VariationEnd)) {
                out.push(' ');
            }
            out.push_str(&piece);
            glue = matches!(token, Token::VariationStart);
        }
        out
    }

    /// What a comment token writes between its braces: for the comment after move `node`, its
    /// drawings' commands first (so the text after them is left as it was read), then its text.
    fn comment_body(&self, node: NodeId, text: &str, before: bool) -> String {
        let commands = if before { String::new() } else { drawing_commands(self.drawings(node)) };
        match (commands.is_empty(), text.is_empty()) {
            (true, _) => text.to_owned(),
            (false, true) => commands,
            (false, false) => format!("{commands} {text}"),
        }
    }

    /// The tags read or set, in their order (the start position's `SetUp` and `FEN` are not
    /// among them: they are written from the game).
    pub fn tags(&self) -> &[(String, String)] {
        &self.tags
    }

    /// A tag's value.
    pub fn tag(&self, name: &str) -> Option<&str> {
        self.tags.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }

    /// Set tag `name` (an empty value removes it). Names are letters, digits and `_`; values are
    /// cut to [`MAX_TAG_LEN`] and lose their line breaks.
    pub fn set_tag(&mut self, name: &str, value: &str) -> Result<(), GameError> {
        let name = name.trim();
        if name.is_empty() || name.len() > MAX_TAG_LEN || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(GameError::BadTag);
        }
        if matches!(name, "FEN" | "SetUp") {
            return Ok(());
        }
        let value: String = cut(&value.replace(['\r', '\n', '\t'], " "), MAX_TAG_LEN).trim().to_owned();
        let at = self.tags.iter().position(|(n, _)| n == name);
        match (at, value.is_empty()) {
            (Some(i), true) => {
                self.tags.remove(i);
            }
            (Some(i), false) => {
                if let Some(t) = self.tags.get_mut(i) {
                    t.1 = value;
                }
            }
            (None, true) => return Ok(()),
            (None, false) if self.tags.len() < MAX_TAGS => self.tags.push((name.to_owned(), value)),
            (None, false) => return Err(GameError::TooLong),
        }
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }

    /// The result: decided by a mate or a draw at the end of the main line, else the Result tag,
    /// else `*`.
    pub fn result(&self) -> String {
        let mut end = 0;
        while let Some(&next) = self.node(end).children.first() {
            end = next;
        }
        let board = &self.node(end).board;
        match board.status() {
            GameStatus::Won if board.side_to_move() == cozy_chess::Color::White => "0-1".to_owned(),
            GameStatus::Won => "1-0".to_owned(),
            GameStatus::Drawn => "1/2-1/2".to_owned(),
            GameStatus::Ongoing => match self.tag("Result") {
                Some(r @ ("1-0" | "0-1" | "1/2-1/2")) => r.to_owned(),
                _ => "*".to_owned(),
            },
        }
    }

    /// The game as PGN: the seven tags, the others, the start position when it is not the
    /// standard one, and the moves wrapped under 80 columns.
    pub fn pgn(&self) -> String {
        let result = self.result();
        let mut out = String::new();
        let quote = |v: &str| v.replace('\\', "\\\\").replace('"', "\\\"");
        for tag in ROSTER {
            let value = if tag == "Result" {
                result.clone()
            } else {
                self.tag(tag).map_or_else(|| if tag == "Date" { "????.??.??".to_owned() } else { "?".to_owned() }, str::to_owned)
            };
            out.push_str(&format!("[{tag} \"{}\"]\n", quote(&value)));
        }
        for (name, value) in self.tags.iter().filter(|(n, _)| !ROSTER.contains(&n.as_str())) {
            out.push_str(&format!("[{name} \"{}\"]\n", quote(value)));
        }
        let start = self.start_fen();
        if start != Board::default().to_string() {
            out.push_str("[SetUp \"1\"]\n");
            out.push_str(&format!("[FEN \"{start}\"]\n"));
        }
        out.push('\n');
        let moves = self.movetext();
        let text = if moves.is_empty() { result } else { format!("{moves} {result}") };
        out.push_str(&wrap(&text, 79));
        out.push('\n');
        out
    }

    /// Read a PGN's first game: its tags (the `FEN` tag's position, the standard start without
    /// one), its moves with their variations, comments and annotation glyphs. Move numbers and
    /// the result are skipped. A move that is not legal ends its line (the variation it is in, or
    /// the main line); the first such move is returned with the game. The game is shown at the
    /// end of its main line.
    pub fn from_pgn(text: &str) -> Result<(Game, Option<String>), GameError> {
        if text.len() > MAX_PGN_LEN {
            return Err(GameError::PgnTooLong);
        }
        let lexed = lex(text);
        let mut tags = Vec::new();
        for t in &lexed {
            match t {
                Lexed::Tag(name, value) if tags.len() < MAX_TAGS => tags.push((name.clone(), value.clone())),
                Lexed::Tag(..) => {}
                _ => break,
            }
        }
        let fen = tags.iter().find(|(n, _)| n == "FEN").map(|(_, v): &(String, String)| v.clone());
        let mut game = match fen {
            Some(fen) => Game::from_fen(&fen)?,
            None => Game::standard(),
        };
        for (name, value) in &tags {
            // The placeholders PGN writes for unknown values are no values. Bad names are
            // skipped, not fatal: the moves are what matters.
            if !matches!(value.as_str(), "?" | "????.??.??" | "*") {
                let _ = game.set_tag(name, value);
            }
        }
        let mut stopped: Option<String> = None;
        // The move the next one is played after, and the last move played (for comments and
        // glyphs), saved at each `(`.
        let (mut cursor, mut last): (NodeId, Option<NodeId>) = (0, None);
        let mut stack: Vec<(NodeId, Option<NodeId>)> = Vec::new();
        // The nesting at which a line was cut by an illegal move: skip until it closes.
        let mut dead: Option<usize> = None;
        // A comment that opens a variation (`({an idea} 1. d4)`): it is the comment before the
        // variation's first move, so it waits for that move rather than going to the line's.
        let mut pending: Option<String> = None;
        let mut started = false;
        for t in lexed.into_iter().skip(tags.len()) {
            match t {
                Lexed::Tag(..) if started => break, // the next game
                Lexed::Tag(..) => {}
                Lexed::Result(r) if stack.is_empty() && started => {
                    if game.tag("Result").is_none() && r != "*" {
                        let _ = game.set_tag("Result", &r);
                    }
                    break;
                }
                Lexed::Result(_) => {}
                Lexed::Open => {
                    stack.push((cursor, last));
                    if dead.is_none() {
                        match last {
                            // A variation replaces the move before it.
                            Some(l) if stack.len() <= MAX_DEPTH => cursor = game.node(l).parent,
                            _ => dead = Some(stack.len()),
                        }
                    }
                    last = None;
                }
                Lexed::Close => {
                    // A variation with a comment and no move: the comment goes where it used to.
                    if let Some(text) = pending.take()
                        && dead.is_none()
                    {
                        append_comment(&mut game, cursor, text);
                    }
                    if let Some((c, l)) = stack.pop() {
                        (cursor, last) = (c, l);
                    }
                    if dead.is_some_and(|d| stack.len() < d) {
                        dead = None;
                    }
                }
                Lexed::Comment(text) => {
                    if dead.is_some() {
                        continue;
                    }
                    match last {
                        None if !stack.is_empty() => {
                            pending = Some(match pending.take() {
                                Some(before) => format!("{before} {text}"),
                                None => text,
                            });
                        }
                        _ => append_comment(&mut game, last.unwrap_or(cursor), text),
                    }
                }
                Lexed::Nag(code) => {
                    if dead.is_none()
                        && let Some(l) = last
                        && !game.nags(l).contains(&code)
                    {
                        game.toggle_nag(l, code);
                    }
                }
                Lexed::Move(word) => {
                    started = true;
                    if dead.is_some() {
                        continue;
                    }
                    let (san, glyphs) = split_glyphs(&word);
                    game.current = cursor;
                    let white = game.side_to_move() == Color::White;
                    if game.play_san(san).is_ok() {
                        cursor = game.current;
                        last = Some(cursor);
                        for code in crate::nag::nags_from_suffix(glyphs, white) {
                            if !game.nags(cursor).contains(&code) {
                                game.toggle_nag(cursor, code);
                            }
                        }
                        if let Some(text) = pending.take() {
                            let joined = match game.comment_before(cursor) {
                                "" => text,
                                before => format!("{before} {text}"),
                            };
                            game.set_comment_before(cursor, &joined);
                        }
                    } else {
                        stopped.get_or_insert(word);
                        dead = Some(stack.len());
                        pending = None;
                    }
                }
            }
        }
        game.current = 0;
        while game.forward() {}
        Ok((game, stopped))
    }
}

/// Add `text` to the comment after `node`, after what it already says.
fn append_comment(game: &mut Game, node: NodeId, text: String) {
    let joined = match game.comment(node) {
        "" => text,
        before => format!("{before} {text}"),
    };
    game.set_comment(node, &joined);
}

/// Which glyphs exclude each other on one move: 1 for the move-quality glyphs (codes 1–6), 2 for
/// the evaluations (10–21), 0 for the others (which don't).
fn nag_group(code: u16) -> u8 {
    match code {
        1..=6 => 1,
        10..=21 => 2,
        _ => 0,
    }
}

/// Never called: node 0 always exists. A node to return instead of panicking.
#[cold]
fn unreachable_node() -> &'static Node {
    static START: std::sync::OnceLock<Node> = std::sync::OnceLock::new();
    START.get_or_init(|| Node::start(Board::default()))
}

fn outcome_of(board: &Board) -> Outcome {
    match board.status() {
        GameStatus::Ongoing => Outcome::Ongoing,
        GameStatus::Won => Outcome::Checkmate,
        GameStatus::Drawn if board.halfmove_clock() >= 100 && board.generate_moves(|_| true) => Outcome::FiftyMoves,
        GameStatus::Drawn => Outcome::Stalemate,
    }
}

/// A move as it is clicked: castling (the king onto its own rook in cozy-chess) is the king's
/// two-file step.
fn click_of(board: &Board, mv: Move) -> (Square, Square) {
    let from = square_from_cozy(mv.from);
    let to = square_from_cozy(mv.to);
    let castles = board.piece_on(mv.from) == Some(cozy_chess::Piece::King) && board.color_on(mv.to) == Some(board.side_to_move());
    if castles {
        let file = if to.file() > from.file() { 6 } else { 2 };
        return (from, Square::new(file, from.rank()).unwrap_or(to));
    }
    (from, to)
}

/// The pieces on a cozy-chess board.
fn placement_of(board: &Board) -> Placement {
    let mut placement = Placement::empty();
    for sq in board.occupied() {
        if let (Some(kind), Some(color)) = (board.piece_on(sq), board.color_on(sq)) {
            placement.set(square_from_cozy(sq), Some(Piece { color: color_from_cozy(color), kind: kind_from_cozy(kind) }));
        }
    }
    placement
}

/// `text` cut to at most `max` bytes, at a character boundary.
fn cut(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    text.get(..end).unwrap_or_default()
}

/// A comment as kept: braces made parentheses, whitespace runs made one space, capped.
fn clean_comment(text: &str) -> String {
    cut(&tidy_comment(text), MAX_COMMENT_LEN).to_owned()
}

/// A comment with braces made parentheses and whitespace runs made one space, not capped (for a
/// caller that must not cut it silently).
fn tidy_comment(text: &str) -> String {
    let swapped = text.replace('{', "(").replace('}', ")");
    swapped.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A move word split into the move and the glyphs glued to its end (`e4!?` → `e4`, `!?`).
fn split_glyphs(word: &str) -> (&str, &str) {
    let end = word.trim_end_matches(['!', '?']).len();
    (word.get(..end).unwrap_or(word), word.get(end..).unwrap_or_default())
}

/// Wrap `text` at spaces, lines at most `width` characters (a longer word stays whole).
fn wrap(text: &str, width: usize) -> String {
    let mut out = String::new();
    let mut col = 0;
    for word in text.split(' ').filter(|w| !w.is_empty()) {
        let n = word.chars().count();
        if col > 0 && col + 1 + n > width {
            out.push('\n');
            col = 0;
        } else if col > 0 {
            out.push(' ');
            col += 1;
        }
        out.push_str(word);
        col += n;
    }
    out
}

/// A piece of PGN.
#[derive(Debug, PartialEq)]
enum Lexed {
    Tag(String, String),
    Move(String),
    Comment(String),
    Nag(u16),
    Open,
    Close,
    Result(String),
}

/// Split PGN into its pieces. Move numbers are dropped; `{…}` and `;…` are comments; a line
/// starting with `%` is skipped.
fn lex(text: &str) -> Vec<Lexed> {
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    let mut word = String::new();
    let mut line_start = true;
    let flush = |word: &mut String, out: &mut Vec<Lexed>| {
        let w = std::mem::take(word);
        if w.is_empty() {
            return;
        }
        if let Some(n) = w.strip_prefix('$') {
            if let Ok(code) = n.parse::<u16>() {
                out.push(Lexed::Nag(code));
            }
        } else if matches!(w.as_str(), "*" | "1-0" | "0-1" | "1/2-1/2" | "½-½") {
            out.push(Lexed::Result(if w == "½-½" { "1/2-1/2".to_owned() } else { w }));
        } else if let Some(code) = crate::nag::nags_from_suffix(&w, true).first().filter(|_| w.chars().all(|c| matches!(c, '!' | '?'))) {
            // A glyph standing alone (`e4 !?`).
            out.push(Lexed::Nag(*code));
        } else if !w.chars().all(|c| c.is_ascii_digit()) {
            out.push(Lexed::Move(w));
        }
    };
    while let Some(c) = chars.next() {
        let at_line_start = std::mem::replace(&mut line_start, c == '\n');
        match c {
            '[' if at_line_start || (out.is_empty() && word.is_empty()) => {
                flush(&mut word, &mut out);
                let mut body = String::new();
                let mut in_quotes = false;
                let mut escaped = false;
                for c in chars.by_ref() {
                    if c == '\n' {
                        line_start = true;
                        break;
                    }
                    if in_quotes {
                        if escaped {
                            escaped = false;
                        } else if c == '\\' {
                            escaped = true;
                            continue;
                        } else if c == '"' {
                            in_quotes = false;
                            continue;
                        }
                        body.push(c);
                        continue;
                    }
                    match c {
                        '"' => {
                            in_quotes = true;
                            body.push('\u{1}');
                        }
                        ']' => break,
                        _ => body.push(c),
                    }
                }
                if let Some((name, value)) = body.split_once('\u{1}') {
                    out.push(Lexed::Tag(cut(name.trim(), MAX_TAG_LEN).to_owned(), cut(value, MAX_TAG_LEN).to_owned()));
                }
            }
            '%' if at_line_start => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        line_start = true;
                        break;
                    }
                }
            }
            '{' => {
                flush(&mut word, &mut out);
                let mut body = String::new();
                for c in chars.by_ref() {
                    if c == '}' {
                        break;
                    }
                    if body.len() < MAX_COMMENT_LEN.saturating_mul(2) {
                        body.push(c);
                    }
                }
                out.push(Lexed::Comment(body));
            }
            ';' => {
                flush(&mut word, &mut out);
                let mut body = String::new();
                for c in chars.by_ref() {
                    if c == '\n' {
                        line_start = true;
                        break;
                    }
                    if body.len() < MAX_COMMENT_LEN {
                        body.push(c);
                    }
                }
                out.push(Lexed::Comment(body));
            }
            '(' => {
                flush(&mut word, &mut out);
                out.push(Lexed::Open);
            }
            ')' => {
                flush(&mut word, &mut out);
                out.push(Lexed::Close);
            }
            // A move number glued to its move: `1.e4`, `12...Nf6`.
            '.' if word.chars().all(|c| c.is_ascii_digit()) => word.clear(),
            '.' => {}
            c if c.is_whitespace() => flush(&mut word, &mut out),
            c => {
                if word.len() < 64 {
                    word.push(c);
                }
            }
        }
    }
    flush(&mut word, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sq(name: &str) -> Square {
        Square::parse(name).unwrap()
    }

    fn play(g: &mut Game, moves: &[(&str, &str)]) {
        for (f, t) in moves {
            g.play(sq(f), sq(t), None).unwrap();
        }
    }

    #[test]
    fn plays_moves_by_squares_and_names_them() {
        let mut g = Game::standard();
        play(&mut g, &[("e2", "e4"), ("e7", "e5"), ("g1", "f3")]);
        assert_eq!(g.movetext(), "1. e4 e5 2. Nf3");
        assert_eq!(g.last_move(), Some((sq("g1"), sq("f3"))));
        assert_eq!(g.side_to_move(), Color::Black);
        assert_eq!(g.play(sq("e5"), sq("e3"), None), Err(GameError::IllegalMove));
    }

    #[test]
    fn the_first_move_number_renumbers_every_move_and_variation() {
        // A diagram read without its move number: Black to move, numbered 1.
        let mut g = Game::from_fen("4k3/8/8/8/8/8/4P3/4K3 b - - 0 1").unwrap();
        g.play_san("Kd7").unwrap();
        g.play_san("e4").unwrap();
        g.go_to(1);
        g.play_san("Ke1").unwrap_err();
        g.play_san("Kf2").unwrap();
        assert_eq!(g.movetext(), "1... Kd7 2. e4 (2. Kf2)");
        assert!(g.set_first_move_number(35));
        assert_eq!(g.first_move_number(), 35);
        assert_eq!(g.movetext(), "35... Kd7 36. e4 (36. Kf2)");
        assert_eq!(g.start_fen(), "4k3/8/8/8/8/8/4P3/4K3 b - - 0 35");
        assert!(g.pgn().contains("[FEN \"4k3/8/8/8/8/8/4P3/4K3 b - - 0 35\"]"), "{}", g.pgn());
        // Moves played after it are numbered on from the new number.
        g.go_to_main_end();
        g.play_san("Ke6").unwrap();
        assert_eq!(g.movetext(), "35... Kd7 36. e4 (36. Kf2) 36... Ke6");
        // The same number changes nothing; out of range is kept within 1..=999.
        assert!(!g.set_first_move_number(35));
        assert!(g.set_first_move_number(0));
        assert_eq!(g.first_move_number(), 1);
        g.set_first_move_number(u32::MAX);
        assert_eq!(g.first_move_number(), MAX_MOVE_NUMBER);
        assert_eq!(g.movetext(), "999... Kd7 1000. e4 (1000. Kf2) 1000... Ke6");
        // The game reads back from its PGN with the number.
        let (back, stopped) = Game::from_pgn(&g.pgn()).unwrap();
        assert_eq!((back.movetext(), stopped), (g.movetext(), None));
    }

    #[test]
    fn the_standard_start_renumbered_writes_its_fen() {
        let mut g = Game::standard();
        let revision = g.revision();
        assert!(g.set_first_move_number(12));
        assert!(g.revision() > revision);
        g.play_san("e4").unwrap();
        assert_eq!(g.movetext(), "12. e4");
        assert!(g.pgn().contains("[SetUp \"1\"]"));
    }

    #[test]
    fn fen_at_is_the_position_after_a_move_whatever_is_shown() {
        let mut g = Game::standard();
        play(&mut g, &[("e2", "e4"), ("e7", "e5")]);
        let after_e4 = g.main_line()[1];
        g.go_to_node(0);
        assert_eq!(g.fen_at(after_e4).as_deref(), Some("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1"));
        assert_eq!(g.fen_at(0), Some(g.start_fen()));
        assert_eq!(g.fen(), g.start_fen(), "the move shown is unchanged");
        assert_eq!(g.fen_at(99), None);
    }

    #[test]
    fn castling_is_the_kings_two_file_step() {
        let mut g = Game::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").unwrap();
        assert!(g.targets(sq("e1")).contains(&sq("g1")));
        assert!(g.targets(sq("e1")).contains(&sq("c1")));
        g.play(sq("e1"), sq("g1"), None).unwrap();
        assert_eq!(g.movetext(), "1. O-O");
        assert_eq!(g.last_move(), Some((sq("e1"), sq("g1"))));
        assert_eq!(g.piece_on(sq("f1")).map(|p| p.kind), Some(PieceKind::Rook));
        // The king onto its own rook is castling too.
        g.play(sq("e8"), sq("a8"), None).unwrap();
        assert_eq!(g.movetext(), "1. O-O O-O-O");
    }

    #[test]
    fn promotion_needs_its_piece() {
        let mut g = Game::from_fen("8/P6k/8/8/8/8/8/K7 w - - 0 1").unwrap();
        assert!(g.is_promotion(sq("a7"), sq("a8")));
        assert_eq!(g.play(sq("a7"), sq("a8"), None), Err(GameError::NeedsPromotion));
        g.play(sq("a7"), sq("a8"), Some(PieceKind::Knight)).unwrap();
        assert_eq!(g.movetext(), "1. a8=N");
    }

    #[test]
    fn legal_moves_are_listed_in_san() {
        let g = Game::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").unwrap();
        let moves = g.legal_moves();
        assert!(moves.contains(&"O-O".to_owned()) && moves.contains(&"O-O-O".to_owned()) && moves.contains(&"Rxa8+".to_owned()), "{moves:?}");
        assert_eq!(Game::standard().legal_moves().len(), 20);
    }

    #[test]
    fn uci_moves_are_played_too() {
        let mut g = Game::from_fen("8/P6k/8/8/8/8/8/K7 w - - 0 1").unwrap();
        g.play_uci("a7a8q").unwrap();
        assert_eq!(g.movetext(), "1. a8=Q");
        assert_eq!(g.play_uci("h7"), Err(GameError::IllegalMove));
        assert_eq!(g.play_uci("h7h6x"), Err(GameError::IllegalMove));
    }

    #[test]
    fn a_move_at_an_earlier_point_is_a_variation_unless_it_is_already_there() {
        let mut g = Game::standard();
        play(&mut g, &[("e2", "e4"), ("e7", "e5"), ("g1", "f3")]);
        g.go_to(1);
        g.play(sq("e7"), sq("e5"), None).unwrap();
        assert_eq!((g.ply(), g.len()), (2, 3), "the same move is followed");
        g.go_to(1);
        play(&mut g, &[("c7", "c5"), ("g1", "f3")]);
        assert_eq!(g.movetext(), "1. e4 e5 (1... c5 2. Nf3) 2. Nf3");
        assert_eq!(g.ply(), 3);
        assert_eq!(g.len(), 3, "the line shown is the variation's");
        // Forward retraces the moves stepped back over, into the variation, until another
        // line is chosen.
        g.back();
        g.back();
        g.forward();
        assert_eq!(g.san(g.current()), "c5");
        g.back();
        g.forward_to(0);
        g.back();
        g.forward();
        assert_eq!(g.san(g.current()), "e5");
    }

    #[test]
    fn variations_are_promoted_and_deleted() {
        let mut g = Game::standard();
        play(&mut g, &[("e2", "e4"), ("e7", "e5")]);
        g.go_to(1);
        play(&mut g, &[("c7", "c5"), ("g1", "f3")]);
        let nf3 = g.current();
        g.promote(nf3);
        assert_eq!(g.movetext(), "1. e4 c5 (1... e5) 2. Nf3");
        assert_eq!(g.san(g.current()), "Nf3", "the move shown follows its renumbering");
        // Delete the e5 variation.
        g.go_to(0);
        g.forward();
        let e5 = match g.tokens().into_iter().find(|t| matches!(t, Token::Move { san, .. } if san == "e5")) {
            Some(Token::Move { node, .. }) => node,
            _ => panic!("e5 is listed"),
        };
        g.delete(e5);
        assert_eq!(g.movetext(), "1. e4 c5 2. Nf3");
        // Deleting the move shown goes back to its parent.
        g.go_to(3);
        let shown = g.current();
        g.delete(shown);
        assert_eq!(g.movetext(), "1. e4 c5");
        assert_eq!(g.ply(), 2);
        g.truncate();
        g.go_to(1);
        g.truncate();
        assert_eq!(g.movetext(), "1. e4");
    }

    #[test]
    fn comments_and_glyphs_are_written_where_they_belong() {
        let mut g = Game::standard();
        play(&mut g, &[("e2", "e4"), ("e7", "e5")]);
        let e5 = g.current();
        g.toggle_nag(e5, 1);
        g.toggle_nag(e5, 2);
        assert_eq!(g.nags(e5), &[2], "one move-quality glyph at a time");
        g.toggle_nag(e5, 14);
        g.set_comment(e5, "  the {classical}\nreply ");
        play(&mut g, &[("g1", "f3")]);
        g.set_comment(0, "A test");
        assert_eq!(g.movetext(), "{A test} 1. e4 e5 $2 $14 {the (classical) reply} 2. Nf3");
        // After a comment, Black's move gets its number again.
        g.set_comment(e5, "");
        let nf3 = g.current();
        g.set_comment(nf3, "develops");
        play(&mut g, &[("b8", "c6")]);
        assert_eq!(g.movetext(), "{A test} 1. e4 e5 $2 $14 2. Nf3 {develops} 2... Nc6");
        g.toggle_nag(e5, 14);
        assert_eq!(g.nags(e5), &[2]);
        g.set_comment(e5, &"é".repeat(MAX_COMMENT_LEN));
        assert!(g.comment(e5).len() <= MAX_COMMENT_LEN);
    }

    #[test]
    fn navigation_stays_in_range() {
        let mut g = Game::standard();
        assert!(!g.back());
        assert!(!g.forward());
        g.play(sq("d2"), sq("d4"), None).unwrap();
        g.go_to(99);
        assert_eq!(g.ply(), 1);
        assert!(g.back());
        assert_eq!(g.fen(), Board::default().to_string());
        assert_eq!(g.last_move(), None);
        g.go_to_node(12345);
        assert_eq!(g.current(), 0);
    }

    #[test]
    fn numbering_starts_from_the_position() {
        let mut g = Game::from_fen("6k1/5ppp/8/8/8/8/5PPP/3R2K1 b - - 0 35").unwrap();
        play(&mut g, &[("g8", "h8"), ("d1", "d8")]);
        assert_eq!(g.movetext(), "35... Kh8 36. Rd8#");
        assert_eq!(g.outcome(), Outcome::Checkmate);
        let pgn = g.pgn();
        assert!(pgn.contains("[FEN \"6k1/5ppp/8/8/8/8/5PPP/3R2K1 b - - 0 35\"]"), "{pgn}");
        assert!(pgn.trim_end().ends_with("35... Kh8 36. Rd8# 1-0"), "{pgn}");
    }

    #[test]
    fn illegal_positions_are_refused() {
        use crate::rules::Violation;
        assert_eq!(Game::from_fen("8/8/8/8/8/8/8/8 w - - 0 1").err(), Some(GameError::IllegalPosition(vec![Violation::Empty])));
        assert!(matches!(Game::from_fen("not a fen"), Err(GameError::BadFen(_))));
        // White to move with Black in check, said so.
        let e = Game::from_fen("4k3/4Q3/8/8/8/8/8/4K3 w - - 0 1").err();
        assert_eq!(e, Some(GameError::IllegalPosition(vec![Violation::NotToMoveInCheck(Color::Black)])));
        assert_eq!(e.unwrap().to_string(), "the position is not legal: Black is in check, but it is the other side's turn");
    }

    #[test]
    fn pgn_round_trips_variations_comments_glyphs_and_tags() {
        let text = "[Event \"Club \\\"open\\\"\"]\n[White \"Ana\"]\n[ECO \"C20\"]\n[FEN \"r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1\"]\n\n\
                    {Start} 1.O-O! {short} (1. O-O-O O-O (1... Rb8) 2. Rd7) 1...O-O-O $14 ; note\n2. Ra7 *";
        let (g, stopped) = Game::from_pgn(text).unwrap();
        assert_eq!(stopped, None);
        assert_eq!(g.movetext(), "{Start} 1. O-O $1 {short} (1. O-O-O O-O (1... Rb8) 2. Rd7) 1... O-O-O $14 {note} 2. Ra7");
        assert_eq!(g.ply(), 3, "shown at the end of the main line");
        assert_eq!(g.tag("Event"), Some("Club \"open\""));
        assert_eq!(g.tag("ECO"), Some("C20"));
        let pgn = g.pgn();
        assert!(pgn.starts_with("[Event \"Club \\\"open\\\"\"]\n[Site \"?\"]"), "{pgn}");
        assert!(pgn.contains("[White \"Ana\"]") && pgn.contains("[ECO \"C20\"]"), "{pgn}");
        let (again, stopped) = Game::from_pgn(&pgn).unwrap();
        assert_eq!(stopped, None);
        assert_eq!(again.movetext(), g.movetext());
        assert_eq!(again.start_fen(), g.start_fen());
        assert_eq!(again.tags(), g.tags());
    }

    #[test]
    fn an_illegal_move_ends_only_its_own_line() {
        let (g, stopped) = Game::from_pgn("1. e4 e5 (1... Ke3 c5) 2. Nf3 Nc6 3. Ke3 Bc5").unwrap();
        assert_eq!(g.movetext(), "1. e4 e5 2. Nf3 Nc6");
        assert_eq!(stopped.as_deref(), Some("Ke3"));
        assert_eq!(Game::from_pgn(&"e".repeat(MAX_PGN_LEN + 1)).err(), Some(GameError::PgnTooLong));
    }

    #[test]
    fn only_the_first_game_is_read() {
        let (g, _) = Game::from_pgn("[Event \"a\"]\n\n1. e4 e5 1-0\n\n[Event \"b\"]\n\n1. d4 d5 0-1\n").unwrap();
        assert_eq!(g.movetext(), "1. e4 e5");
        assert_eq!(g.tag("Event"), Some("a"));
        assert!(g.pgn().contains("1. e4 e5 1-0"), "the result tag is kept when the moves don't decide it");
    }

    #[test]
    fn tags_are_checked() {
        let mut g = Game::standard();
        assert_eq!(g.set_tag("Bad name", "x"), Err(GameError::BadTag));
        g.set_tag("White", "Line\nbreak").unwrap();
        assert_eq!(g.tag("White"), Some("Line break"));
        g.set_tag("White", "").unwrap();
        assert_eq!(g.tag("White"), None);
        g.set_tag("FEN", "8/8/8/8/8/8/8/8 w - - 0 1").unwrap();
        assert_eq!(g.tag("FEN"), None, "the start position is written from the game");
    }

    #[test]
    fn standard_game_pgn_has_no_fen_tag() {
        let mut g = Game::standard();
        g.play(sq("e2"), sq("e4"), None).unwrap();
        let pgn = g.pgn();
        assert!(!pgn.contains("FEN"), "{pgn}");
        assert!(pgn.ends_with("1. e4 *\n"), "{pgn}");
    }

    #[test]
    fn long_move_text_is_wrapped() {
        let mut g = Game::standard();
        for _ in 0..10 {
            play(&mut g, &[("g1", "f3"), ("g8", "f6"), ("f3", "g1"), ("f6", "g8")]);
        }
        assert!(g.pgn().lines().all(|l| l.chars().count() <= 79));
    }

    #[test]
    fn stalemate_is_told_apart_from_mate() {
        let g = Game::from_fen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1").unwrap();
        assert_eq!(g.outcome(), Outcome::Stalemate);
        assert!(!g.in_check());
    }

    #[test]
    fn nesting_is_capped() {
        // Each level a variation of the last one: as deep as variations may nest, and no deeper.
        let mut g = Game::standard();
        assert!(nest(&mut g), "the cap was never reached");
        let text = g.movetext();
        assert_eq!(text.matches('(').count(), MAX_DEPTH, "{text}");
        // The PGN reader keeps the same depth when it reads the game back.
        let (back, stopped) = Game::from_pgn(&g.pgn()).unwrap();
        assert_eq!((back.movetext(), stopped), (text, None));
    }

    /// Under the current move, nest variations one inside the other (each the second reply to
    /// the move before) until the depth cap refuses one. Says whether it did.
    fn nest(g: &mut Game) -> bool {
        for _ in 0..(MAX_DEPTH + 10) {
            let mut moves = g.legal_moves();
            moves.dedup();
            let here = g.current();
            g.play_san(&moves[0]).unwrap();
            g.go_to_node(here);
            match g.play_san(&moves[1]) {
                Ok(()) => {}
                Err(GameError::TooDeep) => return true,
                Err(e) => panic!("{e:?}"),
            }
        }
        false
    }

    #[test]
    fn a_comment_opening_a_variation_goes_with_its_first_move() {
        let (g, stopped) = Game::from_pgn("{Intro} 1. e4 ({An idea} 1. d4 d5) 1... e5 ({Sharper} {still} 1... c5) *").unwrap();
        assert_eq!(stopped, None);
        // The game's own comment stays before the first move; nothing lands on e4 or e5.
        assert_eq!(g.comment(0), "Intro");
        let text = g.movetext();
        assert!(text.contains("1. e4 ({An idea} 1. d4 d5) 1... e5 ({Sharper still} 1... c5)"), "{text}");
        // It is the comment before the variation's first move, not one after it.
        let d4 = g.node_at(&NodePath::new(vec![1])).unwrap();
        assert_eq!((g.comment_before(d4), g.comment(d4)), ("An idea", ""));
        // What it writes reads back the same.
        let pgn = g.pgn();
        let (back, _) = Game::from_pgn(&pgn).unwrap();
        assert_eq!(back.movetext(), text);
        assert_eq!(back.pgn(), pgn);
    }

    #[test]
    fn a_promotion_that_would_nest_past_the_cap_is_refused() {
        // The main line's first move carries variations nested to the cap; promoting another
        // first move would push them all one deeper.
        let mut g = Game::standard();
        g.play_san("e4").unwrap();
        assert!(nest(&mut g));
        g.go_to_node(0);
        g.play_san("d4").unwrap();
        let d4 = g.current();
        let before = g.movetext();
        g.promote(d4);
        assert_eq!(g.movetext(), before);
        assert!(g.movetext().starts_with("1. e4"), "{}", g.movetext());
        // A shallow promotion still works.
        let mut h = Game::standard();
        h.play_san("e4").unwrap();
        h.go_to_node(0);
        h.play_san("d4").unwrap();
        let d4 = h.current();
        h.promote(d4);
        assert!(h.movetext().starts_with("1. d4 (1. e4)"), "{}", h.movetext());
    }

    /// 1. e4 e5 2. Nf3 Nc6 (2... Nf6 3. Bc4) 3. Bb5, shown at 3. Bc4 in the variation.
    fn variation_shown() -> Game {
        let mut g = Game::standard();
        play(&mut g, &[("e2", "e4"), ("e7", "e5"), ("g1", "f3"), ("b8", "c6"), ("f1", "b5")]);
        g.back();
        g.back();
        play(&mut g, &[("g8", "f6"), ("f1", "c4")]);
        g
    }

    #[test]
    fn stepping_back_out_of_a_variation_and_forward_again_returns_into_it() {
        let mut g = variation_shown();
        assert_eq!(g.movetext(), "1. e4 e5 2. Nf3 Nc6 (2... Nf6 3. Bc4) 3. Bb5");
        let bc4 = g.current();
        for _ in 0..3 {
            assert!(g.back());
        }
        assert_eq!(g.san(g.current()), "e5");
        // The line shown is the one stepped back along, so its length and its end are too.
        assert_eq!(g.len(), 5);
        for _ in 0..3 {
            assert!(g.forward());
        }
        assert_eq!(g.current(), bc4);
        assert!(!g.forward());
        // Nothing was lost on the way.
        assert_eq!(g.movetext(), "1. e4 e5 2. Nf3 Nc6 (2... Nf6 3. Bc4) 3. Bb5");
        // The end of the line is the remembered one's; the main line's end is a step of its own.
        g.go_to(0);
        g.go_to(usize::MAX);
        assert_eq!(g.current(), bc4);
        g.go_to_main_end();
        assert_eq!(g.san(g.current()), "Bb5");
    }

    #[test]
    fn a_move_shown_off_the_remembered_line_steps_forward_along_its_first_reply() {
        let mut g = variation_shown();
        g.back();
        g.back();
        // Nc6 is not on the way to the remembered 3. Bc4: forward follows the main line.
        let nf3 = g.current();
        let nc6 = g.replies(nf3)[0];
        g.go_to_node(nc6);
        assert!(g.forward());
        assert_eq!(g.san(g.current()), "Bb5");
        // Stepping back from Bb5 remembers the main line instead.
        g.back();
        g.back();
        assert_eq!(g.next_reply(nf3), Some(nc6));
        assert!(g.forward());
        assert_eq!(g.san(g.current()), "Nc6");
    }

    #[test]
    fn a_reply_is_chosen_by_its_index_and_remembered() {
        let mut g = variation_shown();
        for _ in 0..3 {
            g.back();
        }
        let nf3 = g.replies(g.current())[0];
        g.go_to_node(nf3);
        assert_eq!(g.replies(nf3).len(), 2);
        assert!(!g.forward_to(2));
        assert!(g.forward_to(0));
        assert_eq!(g.san(g.current()), "Nc6");
        g.back();
        assert!(g.forward_to(1));
        assert_eq!(g.san(g.current()), "Nf6");
        g.back();
        assert!(g.forward());
        assert_eq!(g.san(g.current()), "Nf6");
        assert!(g.forward());
        assert_eq!(g.san(g.current()), "Bc4");
        assert_eq!(g.replies(g.current()), &[] as &[NodeId]);
        assert_eq!(g.replies(9999), &[] as &[NodeId]);
    }

    #[test]
    fn the_remembered_line_survives_a_change_to_the_tree() {
        let mut g = variation_shown();
        for _ in 0..3 {
            g.back();
        }
        // A comment and a new variation elsewhere renumber nothing that matters: forward still
        // goes back into 2... Nf6.
        let e5 = g.current();
        g.go_to_node(0);
        g.play_san("d4").unwrap();
        g.delete(g.current());
        g.go_to_node(e5);
        g.forward();
        g.forward();
        assert_eq!(g.san(g.current()), "Nf6");
    }

    #[test]
    fn nesting_too_deep_is_told_apart_from_a_game_too_long() {
        let mut g = Game::standard();
        assert!(nest(&mut g));
        let mut moves = g.legal_moves();
        moves.dedup();
        let here = g.current();
        g.play_san(&moves[0]).unwrap();
        g.go_to_node(here);
        assert_eq!(g.play_san(&moves[1]), Err(GameError::TooDeep));
        assert!(GameError::TooDeep.to_string().contains("32"));
    }

    proptest::proptest! {
        /// Only a command that deletes takes moves away: stepping through the game, choosing a
        /// reply, going to a move and playing never lose one.
        #[test]
        fn moving_around_and_playing_never_lose_a_move(ops in proptest::collection::vec((0usize..7, 0usize..64), 0..120)) {
            let mut g = Game::standard();
            for (op, n) in ops {
                let before = (g.nodes.len(), g.movetext().len());
                match op {
                    0 => { g.back(); }
                    1 => { g.forward(); }
                    2 => { g.forward_to(n % 3); }
                    3 => g.go_to_node(n % g.nodes.len()),
                    4 => g.go_to(n),
                    5 => g.go_to_main_end(),
                    _ => {
                        let mut moves = g.legal_moves();
                        moves.dedup();
                        if !moves.is_empty() {
                            let _ = g.play_san(&moves[n % moves.len()]);
                        }
                    }
                }
                proptest::prop_assert!(g.nodes.len() >= before.0);
                if op < 6 {
                    proptest::prop_assert_eq!(g.movetext().len(), before.1);
                }
                proptest::prop_assert!(g.ply() <= g.len());
                if let Some(next) = g.next_reply(g.current()) {
                    proptest::prop_assert!(g.replies(g.current()).contains(&next));
                }
            }
        }
    }

    #[test]
    fn every_glyph_code_round_trips_through_pgn() {
        let mut checked = 0;
        for n in crate::nag::NAGS {
            for code in [Some(n.code), n.black_code].into_iter().flatten() {
                let mut g = Game::standard();
                play(&mut g, &[("e2", "e4")]);
                g.toggle_nag(g.current(), code);
                let pgn = g.pgn();
                assert!(pgn.contains(&format!("e4 ${code}")), "{pgn}");
                let (back, _) = Game::from_pgn(&pgn).unwrap();
                assert_eq!(back.nags(back.main_line()[1]), &[code], "{pgn}");
                checked += 1;
            }
        }
        assert_eq!(checked, 164, "codes 1–146 and 238–255");
    }

    proptest::proptest! {
        #[test]
        fn pgn_reader_never_panics(text in "\\PC{0,300}") {
            let _ = Game::from_pgn(&text);
        }

        #[test]
        fn pgn_reader_never_panics_on_pgn_like_text(parts in proptest::collection::vec(
            proptest::sample::select(vec!["e4", "e5", "Nf3", "(", ")", "{c}", "$1", "1.", "1...", "*", "[Event \"x\"]\n", "O-O", "d4", "c5", "!?", ";x\n"]), 0..80)) {
            let text = parts.join(" ");
            if let Ok((g, _)) = Game::from_pgn(&text) {
                let (again, _) = Game::from_pgn(&g.pgn()).unwrap();
                proptest::prop_assert_eq!(again.movetext(), g.movetext());
            }
        }

        #[test]
        fn random_clicks_never_panic(clicks in proptest::collection::vec((0usize..64, 0usize..64, 0usize..9), 0..80)) {
            let mut g = Game::standard();
            for (f, t, op) in clicks {
                let (Some(f), Some(t)) = (Square::from_index(f), Square::from_index(t)) else { continue };
                let _ = g.play(f, t, Some(PieceKind::Queen));
                match op {
                    0 => { g.back(); }
                    1 => g.delete(g.current()),
                    2 => g.promote(g.current()),
                    3 => g.toggle_nag(g.current(), 1),
                    4 => g.go_to(t.index() % 5),
                    _ => {}
                }
                proptest::prop_assert!(g.ply() <= g.len());
                let _ = g.last_move();
                let _ = g.pgn();
            }
        }
    }
}
