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

use crate::fen::{Fen, Position};
use crate::pgn_style::PgnStyle;
use crate::placement::{Color, Piece, PieceKind, Placement, Square};
use crate::san::{Lang, San, SanBody, color_from_cozy, square_from_cozy, square_to_cozy, to_cozy_kind};

mod drawing;
mod graft;
mod header;
mod line;
mod pgn_read;
pub mod pgn_scan;
mod split;
mod structure;

pub use drawing::{DrawColor, Drawing, MAX_DRAWINGS, drawing_commands, split_drawings};
pub use graft::{GraftMode, GraftReport, GraftStop, GraftStopReason, MAX_LEFTOVER_LEN, already_says};
pub use header::GameHeader;
pub use split::split_pgn;
pub use structure::{NodePath, NodeSource, Shift, StructureError};

/// Most moves a game holds, variations included. Real annotated games are under 3,000.
pub const MAX_NODES: usize = 10_000;

/// The highest number a game's first move can be given ([`Game::set_first_move_number`]).
pub const MAX_MOVE_NUMBER: u32 = 999;

/// Deepest nesting of variations (a variation inside a variation…).
pub const MAX_DEPTH: usize = 32;

/// Longest PGN read, in bytes.
pub const MAX_PGN_LEN: usize = 1 << 20;

/// Longest comment, in bytes, read from a PGN or set (one cap for both, plan A9-F3: a lower one
/// for editing would cut a comment read whole the first time it is edited). Cut at a character
/// boundary, and said ([`Cut`], [`PgnWarningKind::CommentCut`]).
pub const MAX_COMMENT_LEN: usize = 1 << 16;

/// Most tags kept, and the longest tag name and value.
pub const MAX_TAGS: usize = 64;
pub const MAX_TAG_LEN: usize = 256;

/// Most annotation glyphs on one move.
const MAX_NAGS: usize = 8;

/// Most warnings [`Game::from_pgn_report`] gives.
pub const MAX_PGN_WARNINGS: usize = 64;

/// What reading a PGN noted ([`Game::from_pgn_report`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PgnReport {
    /// The first move that could not be played: its line ends there.
    pub stopped: Option<String>,
    /// Why it could not be played (plan A9-F2b, A1-15: a limit is not an illegal move).
    pub stop_reason: Option<PgnStopReason>,
    /// What the reading noted, in the order read, at most [`MAX_PGN_WARNINGS`].
    pub warnings: Vec<PgnWarning>,
}

/// Why reading a PGN line stopped at a move ([`PgnReport::stopped`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PgnStopReason {
    /// No legal move there is written so (or the word is no move at all).
    Illegal,
    /// More than one legal move there is written so.
    Ambiguous,
    /// The game holds [`MAX_NODES`] moves already.
    TooLong,
    /// The move would nest a variation deeper than [`MAX_DEPTH`].
    TooDeep,
}

impl PgnStopReason {
    /// The reason's name in the tools' replies (`pgn_stop_reason`).
    pub fn name(self) -> &'static str {
        match self {
            PgnStopReason::Illegal => "illegal",
            PgnStopReason::Ambiguous => "ambiguous",
            PgnStopReason::TooLong => "too_long",
            PgnStopReason::TooDeep => "too_deep",
        }
    }

    /// The reason a graft stop stands for.
    fn from_graft(reason: GraftStopReason) -> PgnStopReason {
        match reason {
            GraftStopReason::Ambiguous => PgnStopReason::Ambiguous,
            GraftStopReason::TooLong => PgnStopReason::TooLong,
            GraftStopReason::TooDeep => PgnStopReason::TooDeep,
            GraftStopReason::Illegal | GraftStopReason::Unreadable | GraftStopReason::NullMove | GraftStopReason::NoAnchor => PgnStopReason::Illegal,
        }
    }
}

/// What reading a PGN noted about a piece of it, or writing a document's games left out (plan A9-F4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PgnWarningKind {
    /// A variation kept as a comment: no move stood before it for it to replace.
    OrphanVariation,
    /// A variation kept as a comment: it was nested deeper than [`MAX_DEPTH`] (the levels above
    /// it were read).
    TooDeep,
    /// A move or glyph written as books print it rather than as PGN writes it, read so: `e8Q`,
    /// `a8=q`, `Nf3±`, `+-` alone, `e.p.` after a capture (A1-09).
    NonStandard,
    /// A comment longer than [`MAX_COMMENT_LEN`] bytes, cut there (plan A9-F3, A1-02).
    CommentCut,
    /// A tag name or value longer than [`MAX_TAG_LEN`] bytes, cut there.
    TagCut,
    /// A tag dropped: past [`MAX_TAGS`], with no quoted value, or with a name PGN does not allow.
    TagsDropped,
    /// A glyph dropped: with no move before it, with a code PGN has none for (`$0`, past `$255`),
    /// or past the most a move holds.
    NagDropped,
    /// A result ignored: a `Result` tag that is no result, or a result after the moves that the
    /// tag disagrees with (the tag is kept).
    ResultIgnored,
    /// The `FEN` tag made legal: castling rights the pieces do not allow taken off, an
    /// en-passant square no double step left dropped, the clock kept to 100, the move number to
    /// 1..=[`MAX_MOVE_NUMBER`] (A1-08, A1-20).
    FenRepaired,
    /// Writing a document's game (plan A9-F4): a move the board refused, with every move after
    /// it, left out of what was written; `at` is the move it was to follow.
    MovesLeftOut,
    /// Writing a document's game: its start position is not one a game can start from, so the
    /// game was left out of what was written.
    GameLeftOut,
    /// The `Result` tag says one result and the board at the end of the main line another (a
    /// mate, a stalemate): the tag is kept, never written over (plan A9-F5, R6-04); `at` is the
    /// main line's last move.
    ResultMismatch,
}

impl PgnWarningKind {
    /// The kind's name in the tools' replies.
    pub fn name(self) -> &'static str {
        match self {
            PgnWarningKind::OrphanVariation => "orphan_variation",
            PgnWarningKind::TooDeep => "too_deep",
            PgnWarningKind::NonStandard => "non_standard",
            PgnWarningKind::CommentCut => "comment_cut",
            PgnWarningKind::TagCut => "tag_cut",
            PgnWarningKind::TagsDropped => "tags_dropped",
            PgnWarningKind::NagDropped => "nag_dropped",
            PgnWarningKind::ResultIgnored => "result_ignored",
            PgnWarningKind::FenRepaired => "fen_repaired",
            PgnWarningKind::MovesLeftOut => "moves_left_out",
            PgnWarningKind::GameLeftOut => "game_left_out",
            PgnWarningKind::ResultMismatch => "result_mismatch",
        }
    }

    /// Whether it says something of the game was changed to be read (cut, dropped, repaired or
    /// ignored), rather than a variation kept as text or a spelling read.
    pub fn is_change(self) -> bool {
        matches!(
            self,
            PgnWarningKind::CommentCut
                | PgnWarningKind::TagCut
                | PgnWarningKind::TagsDropped
                | PgnWarningKind::NagDropped
                | PgnWarningKind::ResultIgnored
                | PgnWarningKind::FenRepaired
                | PgnWarningKind::MovesLeftOut
                | PgnWarningKind::GameLeftOut
        )
    }
}

/// What a reading noted, and the move it is on (0: the start): for a variation kept as a comment,
/// the move whose comment (or comment-before) holds its text; for a move or glyph written as
/// books print it, that move.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PgnWarning {
    pub kind: PgnWarningKind,
    pub at: NodeId,
}

/// How a null move is written, in the game and in PGN.
pub const NULL_SAN: &str = "--";

/// The Seven Tag Roster, in the order PGN writes it.
pub const ROSTER: [&str; 7] = ["Event", "Site", "Date", "Round", "White", "Black", "Result"];

/// A move in the game (the start position is node 0).
pub type NodeId = usize;

/// A comment set was longer than [`MAX_COMMENT_LEN`] bytes and was cut: `kept` bytes of it are
/// the comment now ([`Game::set_comment`], [`Game::set_comment_before`]). The caller says so or
/// refuses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cut {
    pub kept: usize,
}

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
    /// The game is played in a variant this reader does not read (its `Variant` tag): refused
    /// whole, never read as chess and cut at its first move of another game (plan A9-F5, R6-05).
    UnsupportedVariant(String),
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
            GameError::UnsupportedVariant(name) => write!(f, "the game is {name}, a variant PdfCraft does not read"),
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
    /// Fifty moves without a capture or a pawn move: a draw a player may claim, never a result
    /// (A1-04).
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
    /// comment before move `node`, which comes right before it: after the variations of the move
    /// before (plan A9-F4), or right after the `(` of a variation it starts.
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

/// The next revision any game takes ([`fresh_revision`]).
static NEXT_REVISION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// A revision no game has had yet: one counter for all of them, so a cache keyed by it (the
/// notation's tokens, plan A8 V3) can't take one game for another. Saturates at `u64::MAX`,
/// which 2^64 changes never reach.
fn fresh_revision() -> u64 {
    fresh_revision_past(0)
}

/// [`fresh_revision`], and newer than `seen`: the counter first moves past it, so no revision
/// handed out later can equal the one returned.
fn fresh_revision_past(seen: u64) -> u64 {
    use std::sync::atomic::Ordering::Relaxed;
    if seen == u64::MAX {
        return u64::MAX;
    }
    NEXT_REVISION.fetch_max(seen.saturating_add(1), Relaxed);
    let r = NEXT_REVISION.fetch_add(1, Relaxed);
    // At the very top the counter would wrap; it stays at the top instead.
    if r == u64::MAX {
        NEXT_REVISION.store(u64::MAX, Relaxed);
    }
    r
}

/// A game from a start position.
#[derive(Clone, Debug)]
pub struct Game {
    /// Node 0 is the start; it is never removed.
    nodes: Vec<Node>,
    current: NodeId,
    tags: Vec<(String, String)>,
    /// Changes with every change to the moves, comments, glyphs and tags (not to the move shown),
    /// to a value no game has had ([`fresh_revision`]): a game put in another's place is told
    /// apart by it too.
    revision: u64,
    /// The furthest move of the line last stepped back along: while it comes after the move
    /// shown, a step forward goes towards it rather than along the first reply, so stepping back
    /// out of a variation and forward again returns into it. Not part of the PGN.
    tip: NodeId,
    /// The tags the moves decide, as they stood when read or set (plan A9-F4, R6-08): the
    /// main line's length when the `PlyCount` tag was, and its first [`crate::pgn::ECO_PLIES`]
    /// moves when the `ECO` tag was. The writer keeps a tag while the moves still say what it
    /// was given for ([`crate::pgn::write`]); `None` when the moves never did.
    ply_count_for: Option<usize>,
    eco_for: Option<Vec<Option<Move>>>,
    /// The moves in reading order, read once for each revision ([`line::ReadingCache`]).
    reading: line::ReadingCache,
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
        Game {
            nodes: vec![Node::start(board)],
            current: 0,
            tags: Vec::new(),
            revision: fresh_revision(),
            tip: 0,
            ply_count_for: None,
            eco_for: None,
            reading: line::ReadingCache::default(),
        }
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

    /// Changes with every change to the moves, comments, glyphs or tags, to a value no other game
    /// has (a clone keeps it until either changes).
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
        crate::fen::placement_of_board(self.board())
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
        self.revision = fresh_revision();
        true
    }

    /// The position after move `node` (the start's for node 0), whatever move is shown, read
    /// from the board ([`Fen::from_board`]: no FEN text in between); `None` when the game has no
    /// such move. Its text is the FEN [`Game::fen`] writes.
    pub fn fen_at(&self, node: NodeId) -> Option<Fen> {
        self.nodes.get(node).map(|n| Fen::from_board(&n.board))
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
    /// [`Game::play_null`] plays it. Castling is taken in any spelling [`San::read`] takes
    /// (`0-0-0`, as many programs write it in PGN, or `o-o`); other moves must be canonical.
    pub fn play_san(&mut self, text: &str) -> Result<(), GameError> {
        let read = San::read(text, Lang::En);
        if read.as_ref().is_ok_and(|san| san.body == SanBody::Null) {
            return self.play_null();
        }
        let san = match read {
            Ok(san) if matches!(san.body, SanBody::Castle(_)) => san,
            _ => San::parse(text).map_err(|_| GameError::IllegalMove)?,
        };
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
        self.revision = fresh_revision();
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
    /// no longer be written out), the game is left as it is and [`StructureError::TooDeep`] says
    /// so (plan A9-F5, A1-19: the refusal was silent); [`StructureError::NoSuchMove`] for a move
    /// the game does not have.
    pub fn promote(&mut self, node: NodeId) -> Result<(), StructureError> {
        if node >= self.nodes.len() {
            return Err(StructureError::NoSuchMove);
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
        if promoted.nodes.iter().any(|n| n.depth > MAX_DEPTH) {
            return Err(StructureError::TooDeep);
        }
        *self = promoted;
        Ok(())
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
        self.revision = fresh_revision();
    }

    /// The comment after move `node` (on node 0: before the first move).
    pub fn comment(&self, node: NodeId) -> &str {
        &self.node(node).comment
    }

    /// Set the comment after move `node`; past [`MAX_COMMENT_LEN`] bytes it is cut, and
    /// [`Cut`] says so (the caller tells the user, or refuses). Braces, which end a PGN
    /// comment, become parentheses. Drawing commands in it (`[%csl …]`, `[%cal …]`) are taken
    /// out and drawn on the move's position, as reading them from a PGN does
    /// ([`split_drawings`]): the comment's text never holds a drawing the move could hold, so
    /// the PGN written is read back the same.
    pub fn set_comment(&mut self, node: NodeId, text: &str) -> Result<(), Cut> {
        let Some(n) = self.nodes.get_mut(node) else { return Ok(()) };
        let mut drawings = n.drawings.clone();
        let (clean, cut) = clean_comment(&split_drawings(text, &mut drawings));
        if n.comment != clean || n.drawings != drawings {
            n.comment = clean;
            n.drawings = drawings;
            self.revision = fresh_revision();
        }
        cut.map_or(Ok(()), Err)
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
        self.revision = fresh_revision();
    }

    /// The SAN of move `node` (empty for the start).
    pub fn san(&self, node: NodeId) -> &str {
        &self.node(node).san
    }

    /// The move that led to the board shown, as (from, to) clicked.
    pub fn last_move(&self) -> Option<(Square, Square)> {
        self.move_at(self.current)
    }

    /// The move that led to the board after `node`, as (from, to) clicked; `None` for the start
    /// and for a node the game does not have.
    pub fn move_at(&self, node: NodeId) -> Option<(Square, Square)> {
        let n = self.nodes.get(node)?;
        let mv = n.mv?;
        Some(click_of(&self.nodes.get(n.parent)?.board, mv))
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
        self.emit_line(0, true, &mut out, 0);
        out
    }

    /// Write the line that goes on from `parent`, with the variations at each move. A move's
    /// comment-before comes right before it: after the variations of the move before, where
    /// PGN writes it (`1. e4 {A} (1. d4 d5) {B} 1... e5`, plan A9-F4, A1-06).
    fn emit_line(&self, mut parent: NodeId, mut numbered: bool, out: &mut Vec<Token>, nesting: usize) {
        if nesting > MAX_DEPTH {
            return;
        }
        loop {
            let children = &self.node(parent).children;
            let Some(&main) = children.first() else { break };
            numbered = self.emit_before(main, out) || numbered;
            numbered = self.emit_move(main, numbered, out);
            for &alt in children.iter().skip(1) {
                out.push(Token::VariationStart);
                self.emit_before(alt, out);
                let after = self.emit_move(alt, true, out);
                self.emit_line(alt, after, out, nesting.saturating_add(1));
                out.push(Token::VariationEnd);
                numbered = true;
            }
            parent = main;
        }
    }

    /// The comment before move `id`, when it has one; whether it had.
    fn emit_before(&self, id: NodeId, out: &mut Vec<Token>) -> bool {
        let text = &self.node(id).comment_before;
        if text.is_empty() {
            return false;
        }
        out.push(Token::Comment { node: id, text: text.clone(), before: true });
        true
    }

    /// One move and its comment. Returns whether the next move needs its number.
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
        commented
    }

    /// The moves as PGN movetext on one line: `1. e4 e5 (1... c5 {Sicilian}) 2. Nf3 $1`, as the
    /// one PGN writer writes them ([`crate::pgn::movetext`]). Comments next to each other (a
    /// move's own and the one before the next move, when no variation comes between) are written
    /// as one, the way PGN reads them back.
    pub fn movetext(&self) -> String {
        self.movetext_with(PgnStyle::FULL)
    }

    /// [`Game::movetext`] without what `style` leaves out (plan A8 V4): the variations past its
    /// depth, the comments (with the drawings written in them), the glyphs. A Black move is
    /// numbered where what is written just before it is not a move, so leaving a comment or a
    /// variation out leaves no stray `12...`.
    pub fn movetext_with(&self, style: PgnStyle) -> String {
        crate::pgn::movetext(self, crate::pgn::WriteOptions { style, ..crate::pgn::WriteOptions::default() })
    }

    /// What a comment token writes between its braces: for the comment after move `node`, its
    /// drawings' commands first (so the text after them is left as it was read), then its text.
    pub(crate) fn comment_body(&self, node: NodeId, text: &str, before: bool) -> String {
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
    /// cut to [`MAX_TAG_LEN`], and their line breaks and other control characters are spaces (as
    /// the writer writes them, so a game written and read back keeps its tags). A `PlyCount` or
    /// `ECO` set is taken to describe the moves as they are now ([`Game::settle_move_tags`]).
    pub fn set_tag(&mut self, name: &str, value: &str) -> Result<(), GameError> {
        let name = name.trim();
        if name.is_empty() || name.len() > MAX_TAG_LEN || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(GameError::BadTag);
        }
        if matches!(name, "FEN" | "SetUp") {
            return Ok(());
        }
        let spaced: String = value.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
        let value: String = cut(&spaced, MAX_TAG_LEN).trim().to_owned();
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
        match name {
            "PlyCount" => self.ply_count_for = Some(self.main_plies()),
            "ECO" => self.eco_for = Some(self.opening()),
            _ => {}
        }
        self.revision = fresh_revision();
        Ok(())
    }

    /// Take the tags the moves decide (`PlyCount`, `ECO`) to describe the moves as they are
    /// now: a game read (from PGN, or a document's score) whose tags came before its moves. The
    /// writer then keeps them while the main line keeps its length (`PlyCount`) and its first
    /// [`crate::pgn::ECO_PLIES`] moves (`ECO`); with `complete` false (a reading that stopped
    /// in the main line, so its moves are not all the text's), `PlyCount` is written from the
    /// moves, and `ECO` kept only when the opening was read whole.
    pub fn settle_move_tags(&mut self, complete: bool) {
        let opening = self.opening();
        self.ply_count_for = complete.then(|| self.main_plies());
        self.eco_for = (complete || opening.len() >= crate::pgn::ECO_PLIES).then_some(opening);
    }

    /// The `PlyCount` the writer writes: the tag as it was while the main line has the length it
    /// had then, else the main line's length; `None` without the tag.
    pub(crate) fn written_ply_count(&self) -> Option<String> {
        let tag = self.tag("PlyCount")?;
        let plies = self.main_plies();
        Some(if self.ply_count_for == Some(plies) { tag.to_owned() } else { plies.to_string() })
    }

    /// Whether the writer keeps the `ECO` tag: the main line still opens with the moves it had
    /// when the tag was read or set.
    pub(crate) fn eco_holds(&self) -> bool {
        self.eco_for.as_ref().is_some_and(|was| *was == self.opening())
    }

    /// Moves in the main line.
    fn main_plies(&self) -> usize {
        self.main_line().len().saturating_sub(1)
    }

    /// The main line's first [`crate::pgn::ECO_PLIES`] moves (`None` for a null move).
    fn opening(&self) -> Vec<Option<Move>> {
        self.main_line().into_iter().skip(1).take(crate::pgn::ECO_PLIES).map(|id| self.node(id).mv).collect()
    }

    /// The result as read or set: the `Result` tag, when it is one PGN writes (`1-0`, `0-1`,
    /// `1/2-1/2`, `*`). The board never writes over it (plan A9-F5, R6-04: a reading where the
    /// two disagree says so, [`PgnWarningKind::ResultMismatch`]); only a game with no tag (made on
    /// the board) takes what its board says at the end of the main line ([`Game::board_result`]),
    /// else `*`.
    pub fn result(&self) -> String {
        match self.tag("Result") {
            Some(r @ ("1-0" | "0-1" | "1/2-1/2" | "*")) => r.to_owned(),
            _ => self.board_result().unwrap_or("*").to_owned(),
        }
    }

    /// What the board says at the end of the main line: `1-0` or `0-1` for a mate, `1/2-1/2` for
    /// a stalemate, `None` while the game goes on. Fifty moves without a capture or a pawn move
    /// end nothing: that draw is claimed, not given (A1-04).
    pub fn board_result(&self) -> Option<&'static str> {
        let mut end = 0;
        while let Some(&next) = self.node(end).children.first() {
            end = next;
        }
        let board = &self.node(end).board;
        match outcome_of(board) {
            Outcome::Checkmate if board.side_to_move() == cozy_chess::Color::White => Some("0-1"),
            Outcome::Checkmate => Some("1-0"),
            Outcome::Stalemate => Some("1/2-1/2"),
            Outcome::Ongoing | Outcome::FiftyMoves => None,
        }
    }

    /// The game as PGN, as the one PGN writer writes it ([`crate::pgn::write`]): the seven
    /// tags, the start position when it is not the standard one, the others, and the moves
    /// wrapped at [`crate::pgn::LINE_WIDTH`] columns (copied to the clipboard too: decision D19).
    pub fn pgn(&self) -> String {
        self.pgn_with(PgnStyle::FULL)
    }

    /// [`Game::pgn`] with [`Game::movetext_with`]'s moves: the tags are all written whatever
    /// `style` leaves out.
    pub fn pgn_with(&self, style: PgnStyle) -> String {
        crate::pgn::write(self, crate::pgn::WriteOptions { style, ..crate::pgn::WriteOptions::default() })
    }

    /// Whether the game starts from the standard position (no `FEN` tag is written).
    pub(crate) fn starts_standard(&self) -> bool {
        self.start_fen() == Board::default().to_string()
    }

    /// Read a PGN's first game: its tags (the `FEN` tag's position, the standard start without
    /// one), its moves with their variations, comments and annotation glyphs. Move numbers are
    /// skipped. A move that can't be played ends its line (the variation it is in, or the main
    /// line); the first such move is returned with the game. The game is shown at the end of its
    /// main line. [`Game::from_pgn_report`] also says why it stopped and what was noted.
    pub fn from_pgn(text: &str) -> Result<(Game, Option<String>), GameError> {
        Game::from_pgn_report(text).map(|(game, report)| (game, report.stopped))
    }

    /// [`Game::from_pgn`], and what the reading noted ([`PgnReport`]), in three steps
    /// (`pgn_read`: scan, parse, build; plan A9-F2b). A variation with no move before it for it
    /// to replace (`((…))`, or one before the first move), or nested deeper than [`MAX_DEPTH`],
    /// is not dropped: its text becomes a comment where it stood, and a warning says so (plan A8
    /// V4). A variation never takes the place of the line it is a variation of (A1-03), and a
    /// comment between `)` and the next move is that move's comment-before (A1-06).
    pub fn from_pgn_report(text: &str) -> Result<(Game, PgnReport), GameError> {
        pgn_read::read(text)
    }
}

/// `tokens` without what `style` leaves out ([`Game::movetext_with`]). A Black move is numbered
/// when what is written just before it is not a move (the start, a comment, a bracket), as PGN
/// numbers it.
pub(crate) fn kept_tokens(tokens: Vec<Token>, style: PgnStyle) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::with_capacity(tokens.len());
    let mut level = 0usize;
    // The nesting of the variation being left out, with everything in it.
    let mut skipping: Option<usize> = None;
    for token in tokens {
        match token {
            Token::VariationStart => {
                level = level.saturating_add(1);
                if skipping.is_none() && !style.keeps(level) {
                    skipping = Some(level);
                }
                if skipping.is_none() {
                    out.push(Token::VariationStart);
                }
            }
            Token::VariationEnd => {
                let closed = level;
                level = level.saturating_sub(1);
                match skipping {
                    Some(at) if at == closed => skipping = None,
                    Some(_) => {}
                    None => out.push(Token::VariationEnd),
                }
            }
            _ if skipping.is_some() => {}
            Token::Comment { .. } if !style.comments => {}
            Token::Move { node, number, color, san, mut nags, depth, .. } => {
                if !style.nags {
                    nags.clear();
                }
                let numbered = color == Color::White || !matches!(out.last(), Some(Token::Move { .. }));
                out.push(Token::Move { node, number, color, numbered, san, nags, depth });
            }
            other => out.push(other),
        }
    }
    out
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

/// A comment as kept: braces made parentheses, whitespace runs made one space, capped at
/// [`MAX_COMMENT_LEN`] bytes; with what was cut, when it was.
fn clean_comment(text: &str) -> (String, Option<Cut>) {
    let tidy = tidy_comment(text);
    let kept = cut(&tidy, MAX_COMMENT_LEN);
    let cut_off = (kept.len() < tidy.len()).then_some(Cut { kept: kept.len() });
    (kept.to_owned(), cut_off)
}

/// A comment with braces made parentheses and whitespace runs made one space, not capped (for a
/// caller that must not cut it silently).
fn tidy_comment(text: &str) -> String {
    let swapped = text.replace('{', "(").replace('}', ")");
    swapped.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sq(name: &str) -> Square {
        Square::parse(name).unwrap()
    }

    /// Plan A9-F1 (A1-01): text that starts with a byte order mark (pasted, or passed through
    /// JSON) keeps its first tag and its moves.
    #[test]
    fn a_byte_order_mark_does_not_hide_the_first_tag() {
        let (game, stopped) = Game::from_pgn("\u{feff}[FEN \"4k3/8/8/8/8/8/8/4K3 w - - 0 1\"]\n[SetUp \"1\"]\n\n1. Ke2 Kd7 *").unwrap();
        assert_eq!(stopped, None);
        assert_eq!(game.start_fen(), "4k3/8/8/8/8/8/8/4K3 w - - 0 1");
        assert_eq!(game.pgn().matches("Ke2").count(), 1);
    }

    /// Plan A9-F2a (A1-16, sonda p08): two tags on one line, an indented tag, an indented FEN and
    /// a backslash that escapes nothing are read as written, the game with them.
    #[test]
    fn tags_on_one_line_indented_or_with_a_backslash_are_read() {
        let (g, stopped) = Game::from_pgn("[Event \"A\"] [Site \"B\"]\n\n1. e4 *").unwrap();
        assert_eq!((g.tag("Event"), g.tag("Site"), g.movetext().as_str(), stopped), (Some("A"), Some("B"), "1. e4", None));
        let (g, stopped) = Game::from_pgn("[Event \"A\"]\n  [Site \"B\"]\n\n1. e4 *").unwrap();
        assert_eq!((g.tag("Site"), g.movetext().as_str(), stopped), (Some("B"), "1. e4", None));
        let (g, stopped) = Game::from_pgn("[Event \"A\"]\n  [FEN \"4k3/8/8/8/8/8/8/4K3 w - - 0 1\"]\n\n1. Ke2 *").unwrap();
        assert_eq!((g.start_fen().as_str(), g.movetext().as_str(), stopped), ("4k3/8/8/8/8/8/8/4K3 w - - 0 1", "1. Ke2", None));
        let (g, _) = Game::from_pgn("[Annotator \"C:\\games \\\\ \\\"x\\\"\"]\n\n1. e4 *").unwrap();
        assert_eq!(g.tag("Annotator"), Some("C:\\games \\ \"x\""));
        // Written back with the escapes PGN needs, and read back the same.
        let (back, _) = Game::from_pgn(&g.pgn()).unwrap();
        assert_eq!(back.tag("Annotator"), g.tag("Annotator"));
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
        assert_eq!(g.fen_at(after_e4).map(|f| f.to_string()).as_deref(), Some("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1"));
        assert_eq!(g.fen_at(0).map(|f| f.to_string()), Some(g.start_fen()));
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
        g.promote(nf3).unwrap();
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
        g.set_comment(e5, "  the {classical}\nreply ").unwrap();
        play(&mut g, &[("g1", "f3")]);
        g.set_comment(0, "A test").unwrap();
        assert_eq!(g.movetext(), "{A test} 1. e4 e5 $2 $14 {the (classical) reply} 2. Nf3");
        // After a comment, Black's move gets its number again.
        g.set_comment(e5, "").unwrap();
        let nf3 = g.current();
        g.set_comment(nf3, "develops").unwrap();
        play(&mut g, &[("b8", "c6")]);
        assert_eq!(g.movetext(), "{A test} 1. e4 e5 $2 $14 2. Nf3 {develops} 2... Nc6");
        g.toggle_nag(e5, 14);
        assert_eq!(g.nags(e5), &[2]);
        assert!(g.set_comment(e5, &"é".repeat(MAX_COMMENT_LEN)).is_err(), "past the cap: cut, and said");
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
    fn castling_written_with_zeros_is_read() {
        // As a book's companion PGN writes it (a Chessable export): 83 of its 120 games stopped
        // at the first `0-0`.
        let text = "[FEN \"r3k2r/pppppppp/8/8/8/8/PPPPPPPP/R3K2R w KQkq - 0 1\"]\n\n1.0-0 0-0-0 ( 1...o-o 2.a3 ) 2.a3 *";
        let (g, stopped) = Game::from_pgn(text).unwrap();
        assert_eq!(stopped, None);
        assert_eq!(g.movetext(), "1. O-O O-O-O (1... O-O 2. a3) 2. a3");
        let mut g = Game::from_fen("3k4/8/8/8/8/8/8/R3K3 w Q - 0 1").unwrap();
        g.play_san("0-0-0+").unwrap();
        assert_eq!(g.movetext(), "1. O-O-O+");
        let mut g = Game::standard();
        assert_eq!(g.play_san("0-0"), Err(GameError::IllegalMove), "read, but not legal here");
        assert_eq!(g.play_san("nf3"), Err(GameError::IllegalMove), "other moves stay canonical");
    }

    #[test]
    fn an_illegal_move_ends_only_its_own_line() {
        let (g, stopped) = Game::from_pgn("1. e4 e5 (1... Ke3 c5) 2. Nf3 Nc6 3. Ke3 Bc5").unwrap();
        // The rest of each line is kept as a comment where it stopped (plan A9-F3, A4-05).
        assert_eq!(g.movetext(), "1. e4 e5 {Ke3 c5} 2. Nf3 Nc6 {Ke3 Bc5}");
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

    /// A1-11 (plan A9-F4): the PGN is wrapped at 79 columns, as the document's always was (it was
    /// one line of up to 13,501 characters); the move text alone stays on one line.
    #[test]
    fn long_move_text_is_wrapped_at_79_columns() {
        let mut g = Game::standard();
        for _ in 0..10 {
            play(&mut g, &[("g1", "f3"), ("g8", "f6"), ("f3", "g1"), ("f6", "g8")]);
        }
        let node = g.current();
        g.set_comment(node, "A comment long enough to have been cut at the eightieth column\nof the line").unwrap();
        let pgn = g.pgn();
        let moves = pgn.split("\n\n").nth(1).unwrap_or_default();
        assert!(moves.lines().count() > 1 && moves.lines().all(|l| l.chars().count() <= crate::pgn::LINE_WIDTH), "{pgn}");
        assert!(moves.split_whitespace().collect::<Vec<_>>().join(" ").contains("eightieth column of the line} *"), "{pgn}");
        assert_eq!(g.movetext().lines().count(), 1);
    }

    /// Plan A9-F5 (R6-04, A1-04): the `Result` read is the game's; the board says its own beside
    /// it, and a reading where the two disagree says so. Fifty moves decide nothing.
    #[test]
    fn a_result_read_is_never_written_over_by_the_board() {
        let kinds = |r: &PgnReport| r.warnings.iter().map(|w| w.kind).collect::<Vec<_>>();
        // The clock reaches 100 with the move: a draw one may claim, not the result (it was
        // written 1/2-1/2).
        let (g, report) = Game::from_pgn_report("[Result \"1-0\"]\n[FEN \"4k3/8/8/8/8/8/8/R3K3 w - - 99 80\"]\n\n80. Ra7 1-0").unwrap();
        assert_eq!((g.result(), g.board_result(), kinds(&report)), ("1-0".to_owned(), None, vec![]));
        assert!(g.pgn().contains("[Result \"1-0\"]") && g.pgn().trim_end().ends_with("80. Ra7 1-0"), "{}", g.pgn());
        // A mate with a draw tag: the tag stays, said (it was written 0-1).
        let (g, report) = Game::from_pgn_report("[Result \"1/2-1/2\"]\n\n1. f3 e5 2. g4 Qh4# 1/2-1/2").unwrap();
        assert_eq!((g.result(), g.board_result()), ("1/2-1/2".to_owned(), Some("0-1")));
        let qh4 = *g.main_line().last().unwrap();
        assert_eq!(report.warnings, [PgnWarning { kind: PgnWarningKind::ResultMismatch, at: qh4 }]);
        assert!(g.pgn().trim_end().ends_with("2. g4 Qh4# 1/2-1/2"), "{}", g.pgn());
        // A stalemate with a win tag.
        let (g, report) = Game::from_pgn_report("[Result \"1-0\"]\n[FEN \"7k/5Q2/6K1/8/8/8/8/8 b - - 0 1\"]\n\n1-0").unwrap();
        assert_eq!((g.result(), g.board_result(), kinds(&report)), ("1-0".to_owned(), Some("1/2-1/2"), vec![PgnWarningKind::ResultMismatch]));
        // An exercise kept unsolved: `*` is the result read, mate or not.
        let (g, report) = Game::from_pgn_report("[Result \"*\"]\n[FEN \"6k1/5ppp/8/8/8/8/5PPP/3R2K1 w - - 0 1\"]\n\n1. Rd8# *").unwrap();
        assert_eq!((g.result(), kinds(&report)), ("*".to_owned(), vec![]));
        // `*` with a result after the moves: the moves' result.
        let (g, _) = Game::from_pgn_report("[Result \"*\"]\n\n1. e4 1-0").unwrap();
        assert_eq!(g.result(), "1-0");
        // A game made on the board, with no tag, takes the board's result; fifty moves are none.
        let mut g = Game::standard();
        for m in ["f3", "e5", "g4", "Qh4#"] {
            g.play_san(m).unwrap();
        }
        assert_eq!(g.result(), "0-1");
        let mut g = Game::from_fen("4k3/8/8/8/8/8/8/R3K3 w - - 99 80").unwrap();
        g.play_san("Ra7").unwrap();
        assert_eq!((g.outcome(), g.result()), (Outcome::FiftyMoves, "*".to_owned()));
    }

    /// Plan A9-F5 (R6-05): the `Variant` tag decides how a game is read; another game than
    /// chess is refused whole, with its name, never read as chess and cut.
    #[test]
    fn a_game_of_another_variant_is_refused_whole() {
        let crazyhouse = "[Variant \"Crazyhouse\"]\n\n1. e4 d5 2. exd5 Qxd5 3. N@e4 *";
        assert_eq!(Game::from_pgn(crazyhouse).err(), Some(GameError::UnsupportedVariant("Crazyhouse".to_owned())));
        let chess960 = "[Variant \"Chess960\"]\n[FEN \"bqnb1rkr/pp3ppp/3ppn2/2p5/5P2/P2P4/NPP1P1PP/BQ1BNRKR w HFhf - 2 9\"]\n\n9. g3 *";
        let refused = Game::from_pgn(chess960).err();
        assert_eq!(refused, Some(GameError::UnsupportedVariant("Chess960".to_owned())));
        assert_eq!(refused.map(|e| e.to_string()).as_deref(), Some("the game is Chess960, a variant PdfCraft does not read"));
        for standard in ["Standard", "From Position", "standard", ""] {
            let (g, stopped) = Game::from_pgn(&format!("[Variant \"{standard}\"]\n\n1. e4 e5 *")).unwrap();
            assert_eq!((g.movetext(), stopped), ("1. e4 e5".to_owned(), None), "{standard}");
        }
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

    /// The corpus of plan A8 §7.4: variations three deep, one on Black's first move, a comment
    /// before a variation and after a move, glyphs inside variations.
    const CORPUS: &str =
        "1. e4 e5 (1... c5 {Sicilian} 2. Nf3 (2. c3 d5 (2... Nf6 3. e5 $1)) d6) 2. Nf3 {A comment} ({Before} 2. Bc4 $5 Nf6) Nc6 3. Bb5 *";

    /// Every move of the tree in PGN order, with its depth, glyphs and comments: what a round
    /// trip must keep.
    fn tree(g: &Game) -> Vec<String> {
        fn walk(g: &Game, id: NodeId, depth: usize, out: &mut Vec<String>) {
            for (i, &child) in g.node(id).children.iter().enumerate() {
                let d = if i == 0 { depth } else { depth + 1 };
                out.push(format!("{d} {} {:?} [{}] [{}]", g.node(child).san, g.nags(child), g.comment_before(child), g.comment(child)));
                walk(g, child, d, out);
            }
        }
        let mut out = vec![format!("start [{}]", g.comment(0))];
        walk(g, 0, 0, &mut out);
        out
    }

    #[test]
    fn the_corpus_reads_back_the_same_tree_and_writes_its_golden() {
        let (g, report) = Game::from_pgn_report(CORPUS).unwrap();
        assert_eq!(report, PgnReport::default());
        let full =
            "1. e4 e5 (1... c5 {Sicilian} 2. Nf3 (2. c3 d5 (2... Nf6 3. e5 $1)) 2... d6) 2. Nf3 {A comment} ({Before} 2. Bc4 $5 Nf6) 2... Nc6 3. Bb5";
        assert_eq!(g.movetext(), full);
        let (back, report) = Game::from_pgn_report(&g.pgn()).unwrap();
        assert_eq!(report, PgnReport::default());
        assert_eq!(tree(&back), tree(&g));
        assert_eq!(back.pgn(), g.pgn());
    }

    #[test]
    fn the_writer_leaves_out_what_the_style_says() {
        use crate::pgn_style::Variations;
        let (g, _) = Game::from_pgn(CORPUS).unwrap();
        let style = |variations, comments, nags| PgnStyle { variations, comments, nags };
        assert_eq!(g.movetext_with(PgnStyle::FULL), g.movetext());
        // The main line alone: no stray number where a variation was.
        assert_eq!(g.movetext_with(style(Variations::Depth(0), true, true)), "1. e4 e5 2. Nf3 {A comment} 2... Nc6 3. Bb5");
        assert_eq!(g.movetext_with(style(Variations::Depth(0), false, true)), "1. e4 e5 2. Nf3 Nc6 3. Bb5");
        // One level: the main line's variations, without theirs.
        assert_eq!(
            g.movetext_with(style(Variations::Depth(1), true, true)),
            "1. e4 e5 (1... c5 {Sicilian} 2. Nf3 d6) 2. Nf3 {A comment} ({Before} 2. Bc4 $5 Nf6) 2... Nc6 3. Bb5"
        );
        assert_eq!(
            g.movetext_with(style(Variations::All, false, false)),
            "1. e4 e5 (1... c5 2. Nf3 (2. c3 d5 (2... Nf6 3. e5)) 2... d6) 2. Nf3 (2. Bc4 Nf6) 2... Nc6 3. Bb5"
        );
        // The tags are all there whatever is left out, and what is left reads back.
        let pgn = g.pgn_with(style(Variations::Depth(0), false, false));
        assert!(pgn.starts_with("[Event \"?\"]") && pgn.ends_with("\n\n1. e4 e5 2. Nf3 Nc6 3. Bb5 *\n"), "{pgn}");
        let (back, report) = Game::from_pgn_report(&pgn).unwrap();
        assert_eq!((back.movetext(), report), ("1. e4 e5 2. Nf3 Nc6 3. Bb5".to_owned(), PgnReport::default()));
    }

    #[test]
    fn a_variation_with_no_move_before_it_is_kept_as_a_comment_and_said() {
        // Before the first move: a comment on the start.
        let (g, report) = Game::from_pgn_report("(1. d4 d5) 1. e4 e5 *").unwrap();
        assert_eq!(g.comment(0), "(d4 d5)");
        assert_eq!(g.movetext(), "{(d4 d5)} 1. e4 e5");
        assert_eq!(report.warnings, [PgnWarning { kind: PgnWarningKind::OrphanVariation, at: 0 }]);
        // `((…))`: the inner one has no move before it inside the outer one.
        let (g, report) = Game::from_pgn_report("1. e4 ((1. d4 {Queen's pawn} $1)) e5 2. Nf3 *").unwrap();
        assert_eq!(report.stopped, None);
        assert_eq!(report.warnings.len(), 1, "{report:?}");
        assert_eq!(report.warnings[0].kind, PgnWarningKind::OrphanVariation);
        // A variation holding only that text leaves it on the move it was an alternative to
        // (plan A9-F2b, A1-06; it used to go to the start).
        assert_eq!(g.comment(g.main_line()[1]), "(d4 Queen's pawn $1)", "{}", g.movetext());
        assert_eq!(g.movetext_with(PgnStyle { comments: false, ..PgnStyle::FULL }), "1. e4 e5 2. Nf3", "the main line goes on");
        // Never closed: kept to the end of the text.
        let (g, report) = Game::from_pgn_report("1. e4 e5 ((2. Nf3").unwrap();
        assert_eq!(report.warnings.len(), 1);
        assert!(g.movetext().contains("(Nf3"), "{}", g.movetext());
        assert_eq!(g.movetext_with(PgnStyle { comments: false, ..PgnStyle::FULL }), "1. e4 e5");
    }

    #[test]
    fn a_variation_nested_past_the_cap_is_kept_as_a_comment_and_said() {
        // Exactly MAX_DEPTH deep reads as it was written.
        let mut g = Game::standard();
        assert!(nest(&mut g));
        let deepest = g.movetext();
        let (back, report) = Game::from_pgn_report(&g.pgn()).unwrap();
        assert_eq!((back.movetext(), report), (deepest.clone(), PgnReport::default()));
        // One more level: the levels above it read, the last kept as text, said.
        let at = deepest.find(')').unwrap();
        let text = format!("{} (Zz9 {{too deep}}){}", &deepest[..at], &deepest[at..]);
        let (back, report) = Game::from_pgn_report(&format!("{text} *")).unwrap();
        assert_eq!(report.warnings.len(), 1, "{report:?}");
        assert_eq!(report.warnings[0].kind, PgnWarningKind::TooDeep);
        let written = back.movetext();
        assert_eq!(written.matches('(').count(), MAX_DEPTH + 1, "{written}");
        assert!(written.contains("{(Zz9 too deep)}"), "{written}");
        assert_eq!(back.node(report.warnings[0].at).depth, MAX_DEPTH);
    }

    #[test]
    fn python_chess_reads_the_corpus_as_the_same_tree() {
        // python-chess is GPL: an outside process only, and the test skips without it.
        let (g, _) = Game::from_pgn(CORPUS).unwrap();
        let script = "import sys, chess.pgn\n\
            g = chess.pgn.read_game(sys.stdin)\n\
            def walk(n, d):\n\
            \x20   for i, c in enumerate(n.variations):\n\
            \x20       e = d if i == 0 else d + 1\n\
            \x20       print(e, c.san(), sorted(c.nags))\n\
            \x20       walk(c, e)\n\
            walk(g, 0)\n\
            print(len(g.errors))";
        let child = std::process::Command::new("python")
            .args(["-I", "-c", script])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn();
        let Ok(mut child) = child else { return };
        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write as _;
            let _ = stdin.write_all(g.pgn().as_bytes());
        }
        let Ok(out) = child.wait_with_output() else { return };
        if !out.status.success() {
            return; // no python-chess
        }
        let text = String::from_utf8_lossy(&out.stdout).replace('\r', "");
        let ours: Vec<String> = tree(&g)
            .iter()
            .skip(1)
            .map(|line| {
                let mut parts = line.splitn(3, ' ');
                let (d, san) = (parts.next().unwrap_or_default(), parts.next().unwrap_or_default());
                let nags = parts.next().unwrap_or_default().split(" [").next().unwrap_or_default();
                format!("{d} {san} {nags}")
            })
            .chain(std::iter::once("0".to_owned()))
            .collect();
        assert_eq!(text.lines().collect::<Vec<_>>(), ours, "{}", g.pgn());
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
        // Refused, and said (plan A9-F5, A1-19: it was silent).
        assert_eq!(g.promote(d4), Err(StructureError::TooDeep));
        assert_eq!(g.movetext(), before);
        assert!(g.movetext().starts_with("1. e4"), "{}", g.movetext());
        assert_eq!(g.promote(9999), Err(StructureError::NoSuchMove));
        // A shallow promotion still works.
        let mut h = Game::standard();
        h.play_san("e4").unwrap();
        h.go_to_node(0);
        h.play_san("d4").unwrap();
        let d4 = h.current();
        assert_eq!(h.promote(d4), Ok(()));
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
                    2 => {
                        let _ = g.promote(g.current());
                    }
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
