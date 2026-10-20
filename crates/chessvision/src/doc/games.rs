//! Games found in a document's text: move trees replayed with `cozy-chess`.
//!
//! **The tree.** A [`GameScore`] holds what PGN holds: the tag roster, the starting position (a FEN,
//! or the initial position), a comment, the moves and the result. Moves are [`Node`]s in an arena:
//! each knows its parent and its children, the first child continuing the line and the others its
//! alternatives (variations). Every node was played on a board, so its SAN is canonical and its
//! FEN is the position after it. Nodes are kept in PGN order ([`GameScore::canonical_order`]), so a
//! game written as PGN and read back is equal to itself.
//!
//! **The walk** ([`Walker`]) turns a document's blocks, in order, into games:
//!
//! - **Where a game starts.** At the first move after a diagram, from the diagram's FEN; or, with
//!   no diagram before it, at a move numbered `1.`, from the initial position. Moves anywhere else
//!   (prose such as "after 23.Nf5") have no position and stay [`MoveStatus::Unchecked`].
//! - **Which diagram.** Books print two diagrams side by side and their texts after both, so the
//!   diagram just before a first move is not always its own. When the first move is not legal
//!   there but is from a diagram nearby (this page or the one before) that no game started from,
//!   the game starts from that one, flagged [`ReviewFlag::DiagramMatchedByMove`]. A `1.` the open
//!   game cannot play, or would take as another first move, starts a new game the same way. A move
//!   of the other side one ply before the game's start (`1.f5+` after a game from `1...f5`) that
//!   is a first move from the game's own diagram starts a second game from it: books give the
//!   solution for each side to move.
//! - **The diagram's FEN.** The first move's number fixes the full-move number, and the side to
//!   move when the reading only assumed it (`1...Kd7` under a diagram read as White to move, if the
//!   position allows Black to move; flagged [`ReviewFlag::SideFromMoveNumber`]). A reading that
//!   disagrees otherwise is kept and flagged [`ReviewFlag::SideConflict`].
//! - **Trust.** Only an accepted reading is trusted (blueprint §2). A legal reading that did not
//!   pass the gate (including one the decoder repaired) still seeds its game, so its moves are
//!   checked against the best position there is, but the game is marked untrusted and flagged
//!   [`ReviewFlag::UntrustedStart`]: replaying cleanly is evidence for the reading, not proof. An
//!   unreadable or illegal reading seeds nothing: the moves after it stay unchecked and the diagram
//!   is flagged [`ReviewFlag::NoStartPosition`].
//! - **Lines.** A move continues its line. A numbered move that points back into the line (`1...Ke8`
//!   after `1.Qh5 Kd7 2.Qf7+`) starts an alternative to the move with that number, as books write
//!   second solutions. `(` and `[` open a variation, an alternative to the last move, and `)` and `]`
//!   close it (PGN's rule). A variation whose first move is numbered as the move after the last
//!   one holds replies to it instead (`1.Kb1 (1...b3 2.Kb2; 1...a3 2.Ka2)`); the main line's own
//!   reply, when it comes, still goes first.
//! - **Failures.** A move that is illegal, ambiguous or numbered out of sequence is flagged, and its
//!   line stops: every later move of that line is [`MoveStatus::NotReached`] until a numbered move
//!   finds its place again. A numbered move that cannot be played where the reading is, outside
//!   brackets, is tried on the main line: books go back to it after an alternative written as
//!   prose. So is one that could only be an alternative to a move of the digression it follows
//!   while it is exactly the main line's next move (`2...Kd7? 3.Kf4 … 9.Kf5` and then
//!   `3.Kd4 Kf6!`): the book is back on its main line. Nothing is dropped: the printed text of a
//!   move that was not played goes into the game's comments, so the PGN still carries it.
//! - **Comments and NAGs.** Prose inside a game is the comment of the move before it. An annotation
//!   glyph belongs to the move it follows.
//! - **Puzzles.** A diagram whose caption gives a number (`№185.`) is remembered by it. A
//!   solution that names it (`№185. Teichmann – Lasker 1...Qh4`), however many pages on, closes
//!   the open game and starts its moves from that diagram (tagged `PdfPuzzle`).
//! - **Where a game ends.** At the next diagram, at a heading, at a result (`1-0`, `0-1`, `½-½`,
//!   `1/2-1/2`) outside variations, and at a new `1.` when the game started from the initial
//!   position. Paragraphs and page breaks do not end a game. A diagram does not end it when it
//!   shows the position the main line reached (a book reprinting the game's position, often on
//!   the next page), or when the next move continues the main line and is not a first move from
//!   the diagram (flagged [`ReviewFlag::ContinuedPastDiagram`]); text between the diagram and that
//!   move goes to neither game.
//!
//! Everything is bounded: [`MAX_GAME_NODES`] moves a game, [`MAX_VARIATION_DEPTH`] nested
//! variations, [`MAX_COMMENT_CHARS`] a comment, [`MAX_GAMES`] games a document.

use cozy_chess::{Board, Color, Move};
use pdfcraft_chess::repair::GameContext;
use pdfcraft_chess::san::{San, SanBody, SanError};
use serde::{Deserialize, Serialize};

use super::{MoveNumber, MoveStatus, ReviewFlag, UserRect};

/// Most moves in one game, variations included.
pub const MAX_GAME_NODES: usize = 4096;
/// Most nested variations.
pub const MAX_VARIATION_DEPTH: usize = 32;
/// Longest comment, in characters.
pub const MAX_COMMENT_CHARS: usize = 4096;
/// Most NAGs on one move.
pub const MAX_NAGS: usize = 8;
/// Most games in one document.
pub const MAX_GAMES: usize = 4096;
/// Most legal moves listed for a reviewer.
pub const MAX_LISTED_MOVES: usize = 8;
/// Most diagrams kept that no game started from, to be matched by a later first move.
pub const MAX_SPARE_DIAGRAMS: usize = 6;
/// Most numbered puzzle diagrams remembered for their solutions.
pub const MAX_PUZZLES: usize = 20_000;

/// How a game ended.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum GameResult {
    #[serde(rename = "1-0")]
    WhiteWins,
    #[serde(rename = "0-1")]
    BlackWins,
    #[serde(rename = "1/2-1/2")]
    Draw,
    #[default]
    #[serde(rename = "*")]
    Unknown,
}

impl GameResult {
    /// The PGN termination marker.
    pub fn as_pgn(self) -> &'static str {
        match self {
            GameResult::WhiteWins => "1-0",
            GameResult::BlackWins => "0-1",
            GameResult::Draw => "1/2-1/2",
            GameResult::Unknown => "*",
        }
    }

    /// A result as books and PGN print it (`*` only as PGN's own token, see [`GameResult::from_pgn`]).
    pub fn from_text(token: &str) -> Option<GameResult> {
        match token {
            "1-0" => Some(GameResult::WhiteWins),
            "0-1" => Some(GameResult::BlackWins),
            "1/2-1/2" | "½-½" => Some(GameResult::Draw),
            _ => None,
        }
    }

    pub fn from_pgn(token: &str) -> Option<GameResult> {
        if token == "*" { Some(GameResult::Unknown) } else { GameResult::from_text(token) }
    }
}

/// PGN's seven-tag roster, less the result (kept in [`GameScore::result`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Roster {
    pub event: String,
    pub site: String,
    pub date: String,
    pub round: String,
    pub white: String,
    pub black: String,
}

impl Default for Roster {
    fn default() -> Roster {
        let q = || "?".to_owned();
        Roster { event: q(), site: q(), date: "????.??.??".to_owned(), round: q(), white: q(), black: q() }
    }
}

/// A PGN tag outside the roster.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tag {
    pub name: String,
    pub value: String,
}

/// One move of a game.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    /// Canonical English SAN, with the check or mate mark the position gives it.
    pub san: String,
    /// Half-moves from White's first move: 0 is `1.`, 1 is `1...`.
    pub ply: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nags: Vec<u16>,
    /// A comment printed before the move. Only a variation's first move has one; text before a
    /// main-line move is the comment of the move before it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment_before: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,
    /// The position after the move.
    pub fen: String,
    /// The move before it (`None` for a first move).
    pub parent: Option<usize>,
    /// The moves after it: the first continues the line, the others are its alternatives.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<usize>,
}

/// What PGN carries of a game.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameScore {
    pub roster: Roster,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_tags: Vec<Tag>,
    /// The starting position; `None` for the initial position.
    pub start_fen: Option<String>,
    /// The comment before the first move.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,
    pub nodes: Vec<Node>,
    /// The first moves: the main one first, then its alternatives.
    pub roots: Vec<usize>,
    pub result: GameResult,
}

/// Why a move could not be added.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PlayError {
    /// No legal move matches; the nearest legal moves, for the reviewer.
    Illegal(Vec<String>),
    /// Several legal moves match.
    Ambiguous(Vec<String>),
    /// [`MAX_GAME_NODES`] or [`MAX_VARIATION_DEPTH`] reached.
    TooLarge,
    /// The position to play from is missing or not legal.
    NoPosition,
}

/// The ply of a move number: `1.` is 0, `1...` is 1.
pub(crate) fn ply_of(number: MoveNumber) -> u32 {
    (u32::from(number.number).saturating_sub(1)).saturating_mul(2).saturating_add(u32::from(number.black))
}

/// The move number a ply is printed with.
pub(crate) fn number_of(ply: u32) -> MoveNumber {
    MoveNumber { number: u16::try_from(ply / 2 + 1).unwrap_or(u16::MAX), black: ply % 2 == 1 }
}

fn ply_of_board(board: &Board) -> u32 {
    (u32::from(board.fullmove_number()).saturating_sub(1)).saturating_mul(2).saturating_add(u32::from(board.side_to_move() == Color::Black))
}

fn legal_moves(board: &Board) -> Vec<Move> {
    let mut v = Vec::new();
    board.generate_moves(|moves| {
        v.extend(moves);
        false
    });
    v
}

/// The legal moves `san` could mean when its disambiguation does not single one out.
fn fitting(board: &Board, san: &San) -> Vec<String> {
    let SanBody::Move { piece, from_file, from_rank, to, promotion, .. } = san.body else { return Vec::new() };
    legal_moves(board)
        .into_iter()
        .filter(|mv| from_file.is_none_or(|f| f == mv.from.file() as u8) && from_rank.is_none_or(|r| r == mv.from.rank() as u8))
        .filter_map(|mv| San::from_move(board, mv).ok())
        .filter(|s| matches!(s.body, SanBody::Move { piece: p, to: t, promotion: pr, .. } if p == piece && t == to && (promotion.is_none() || pr == promotion)))
        .map(|s| s.to_english())
        .take(MAX_LISTED_MOVES)
        .collect()
}

/// Legal moves to the square `san` names, for a reviewer looking at an illegal move.
fn near(board: &Board, san: &San) -> Vec<String> {
    let SanBody::Move { to, .. } = san.body else { return Vec::new() };
    legal_moves(board)
        .into_iter()
        .filter_map(|mv| San::from_move(board, mv).ok())
        .filter(|s| matches!(s.body, SanBody::Move { to: t, .. } if t == to))
        .map(|s| s.to_english())
        .take(MAX_LISTED_MOVES)
        .collect()
}

/// Append `text` to a comment: whitespace collapsed to single spaces, `}` (which would end a PGN
/// comment) written `)`, at most [`MAX_COMMENT_CHARS`] characters, cut between words. Returns
/// whether anything was cut. Idempotent, so the builder and the PGN reader agree.
pub(crate) fn append_comment(target: &mut String, text: &str) -> bool {
    let mut count = target.chars().count();
    for word in text.split_whitespace() {
        let need = word.chars().count().saturating_add(usize::from(!target.is_empty()));
        if count.saturating_add(need) > MAX_COMMENT_CHARS {
            return true;
        }
        if !target.is_empty() {
            target.push(' ');
        }
        target.extend(word.chars().map(|c| if c == '}' { ')' } else { c }));
        count = count.saturating_add(need);
    }
    false
}

impl GameScore {
    /// An empty game from `start_fen` (`None`: the initial position). The FEN is not checked here;
    /// moves cannot be added to a game whose FEN is not a legal position.
    pub fn new(start_fen: Option<String>) -> GameScore {
        GameScore {
            roster: Roster::default(),
            extra_tags: Vec::new(),
            start_fen,
            comment: String::new(),
            nodes: Vec::new(),
            roots: Vec::new(),
            result: GameResult::Unknown,
        }
    }

    /// The starting position, when it is legal.
    pub fn start_board(&self) -> Option<Board> {
        match &self.start_fen {
            None => Some(Board::default()),
            Some(fen) => Board::from_fen(fen.trim(), false).ok(),
        }
    }

    /// The position after node `at` (`None`: the starting position).
    pub fn board_after(&self, at: Option<usize>) -> Option<Board> {
        match at {
            None => self.start_board(),
            Some(n) => Board::from_fen(&self.nodes.get(n)?.fen, false).ok(),
        }
    }

    /// The ply of the first move.
    pub fn start_ply(&self) -> Option<u32> {
        self.start_board().map(|b| ply_of_board(&b))
    }

    /// The ply of the move after node `at`.
    fn next_ply(&self, at: Option<usize>) -> Option<u32> {
        match at {
            None => self.start_ply(),
            Some(n) => Some(self.nodes.get(n)?.ply.saturating_add(1)),
        }
    }

    /// The moves after node `at` (`None`: the first moves).
    pub fn children(&self, at: Option<usize>) -> &[usize] {
        match at {
            None => &self.roots,
            Some(n) => self.nodes.get(n).map_or(&[], |node| &node.children),
        }
    }

    /// The main line's nodes.
    pub fn mainline(&self) -> Vec<usize> {
        let mut out = Vec::new();
        let mut next = self.roots.first().copied();
        while let Some(n) = next {
            if out.len() >= self.nodes.len() {
                break;
            }
            out.push(n);
            next = self.nodes.get(n).and_then(|node| node.children.first().copied());
        }
        out
    }

    /// Whether node `n` starts a variation: it is not the first of its siblings.
    pub fn starts_variation(&self, n: usize) -> bool {
        self.nodes.get(n).is_some_and(|node| self.children(node.parent).first() != Some(&n))
    }

    /// How many variations deep node `n` is.
    fn depth(&self, n: usize) -> usize {
        let mut depth = 0usize;
        let mut cur = Some(n);
        for _ in 0..=self.nodes.len() {
            let Some(c) = cur else { break };
            let Some(node) = self.nodes.get(c) else { break };
            depth = depth.saturating_add(usize::from(self.starts_variation(c)));
            cur = node.parent;
        }
        depth
    }

    /// Where the move numbered `ply` is on the path from node `at` back to the start: the node
    /// before it (`Some(None)` for a first move). `None` when the path has no such move.
    fn before_ply(&self, at: Option<usize>, ply: u32) -> Option<Option<usize>> {
        let mut cur = at;
        for _ in 0..=self.nodes.len() {
            let node = self.nodes.get(cur?)?;
            if node.ply == ply {
                return Some(node.parent);
            }
            cur = node.parent;
        }
        None
    }

    /// Play `san` after node `at`. With `reuse`, a move already there is returned instead of
    /// added twice. Returns the move's node.
    pub(crate) fn play(&mut self, at: Option<usize>, san: &San, reuse: bool) -> Result<usize, PlayError> {
        let board = self.board_after(at).ok_or(PlayError::NoPosition)?;
        let ply = self.next_ply(at).ok_or(PlayError::NoPosition)?;
        let mv = match san.to_move(&board) {
            Ok(mv) => mv,
            Err(SanError::Ambiguous(_)) => return Err(PlayError::Ambiguous(fitting(&board, san))),
            Err(_) => return Err(PlayError::Illegal(near(&board, san))),
        };
        let canonical = San::from_move(&board, mv).map_err(|_| PlayError::Illegal(Vec::new()))?.to_english();
        if reuse && let Some(&n) = self.children(at).iter().find(|&&c| self.nodes.get(c).is_some_and(|node| node.san == canonical)) {
            return Ok(n);
        }
        let alternative = !self.children(at).is_empty();
        let depth = at.map_or(0, |a| self.depth(a)).saturating_add(usize::from(alternative));
        if self.nodes.len() >= MAX_GAME_NODES || depth > MAX_VARIATION_DEPTH {
            return Err(PlayError::TooLarge);
        }
        let mut after = board;
        after.try_play(mv).map_err(|_| PlayError::Illegal(Vec::new()))?;
        let id = self.nodes.len();
        self.nodes.push(Node {
            san: canonical,
            ply,
            nags: Vec::new(),
            comment_before: String::new(),
            comment: String::new(),
            fen: after.to_string(),
            parent: at,
            children: Vec::new(),
        });
        match at.and_then(|a| self.nodes.get_mut(a)) {
            Some(parent) => parent.children.push(id),
            None => self.roots.push(id),
        }
        Ok(id)
    }

    /// The nodes in PGN order: a move, then each of its alternatives with their lines, then the
    /// move after it. Nodes the tree does not reach (or reaches deeper than
    /// [`MAX_VARIATION_DEPTH`], or a second time) are left out.
    pub fn canonical_order(&self) -> Vec<usize> {
        let mut out = Vec::with_capacity(self.nodes.len());
        let mut seen = vec![false; self.nodes.len()];
        if let Some((&first, alternatives)) = self.roots.split_first() {
            self.order_line(first, alternatives, 0, &mut out, &mut seen);
        }
        out
    }

    fn order_line(&self, start: usize, alternatives: &[usize], depth: usize, out: &mut Vec<usize>, seen: &mut [bool]) {
        let (mut current, mut alternatives) = (start, alternatives);
        loop {
            match seen.get_mut(current) {
                Some(s) if !*s => *s = true,
                _ => return,
            }
            out.push(current);
            if depth < MAX_VARIATION_DEPTH {
                for &alt in alternatives {
                    self.order_line(alt, &[], depth + 1, out, seen);
                }
            }
            let Some((&next, rest)) = self.nodes.get(current).and_then(|n| n.children.split_first()) else { return };
            (current, alternatives) = (next, rest);
        }
    }

    /// Put the nodes in [`GameScore::canonical_order`]. Returns where each old node went.
    pub(crate) fn reorder_canonical(&mut self) -> Vec<Option<usize>> {
        let order = self.canonical_order();
        let mut map = vec![None; self.nodes.len()];
        for (new, &old) in order.iter().enumerate() {
            if let Some(m) = map.get_mut(old) {
                *m = Some(new);
            }
        }
        let remap = |i: usize| map.get(i).copied().flatten();
        let nodes = order
            .iter()
            .filter_map(|&old| self.nodes.get(old))
            .map(|n| Node { parent: n.parent.and_then(remap), children: n.children.iter().filter_map(|&c| remap(c)).collect(), ..n.clone() })
            .collect();
        self.roots = self.roots.iter().filter_map(|&r| remap(r)).collect();
        self.nodes = nodes;
        map
    }

    /// Put text that came before node `n`: its own `comment_before` when it starts a variation,
    /// else the comment of the move before it (or of the game).
    /// Make node `n` the first of its siblings: the line its parent goes on with.
    pub(crate) fn put_first(&mut self, n: usize) {
        let parent = self.nodes.get(n).and_then(|node| node.parent);
        let siblings = match parent {
            None => &mut self.roots,
            Some(p) => match self.nodes.get_mut(p) {
                Some(node) => &mut node.children,
                None => return,
            },
        };
        if let Some(i) = siblings.iter().position(|&c| c == n) {
            let node = siblings.remove(i);
            siblings.insert(0, node);
        }
    }

    pub(crate) fn comment_before(&mut self, n: usize, text: &str) -> bool {
        let starts = self.starts_variation(n);
        let parent = self.nodes.get(n).and_then(|node| node.parent);
        let target = if starts {
            self.nodes.get_mut(n).map(|node| &mut node.comment_before)
        } else {
            match parent {
                Some(p) => self.nodes.get_mut(p).map(|node| &mut node.comment),
                None => Some(&mut self.comment),
            }
        };
        target.is_some_and(|t| append_comment(t, text))
    }
}

/// A move's node: [`super::ChessDoc::games`]`[game].score.nodes[node]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeRef {
    pub game: usize,
    pub node: usize,
}

/// Where a game's starting position came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GameOrigin {
    /// The moves started at `1.` with no diagram before them.
    Initial,
    /// The diagram before the moves: page index and the diagram's number on it.
    Diagram { page: u32, diagram: usize, accepted: bool },
}

/// A review flag of a game, and where on the page it was raised.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GameFlag {
    pub page: u32,
    pub source_rect: Option<UserRect>,
    pub flag: ReviewFlag,
}

/// A game found in the document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Game {
    pub origin: GameOrigin,
    /// Starts from the initial position or from an accepted reading.
    pub trusted: bool,
    /// The page its first move is on.
    pub page: u32,
    pub score: GameScore,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<GameFlag>,
}

impl Game {
    /// The starting position as a repair context (`pdfcraft_chess::repair`), for repairing this
    /// game's text against it. `None` when it is not a legal position.
    pub fn context(&self) -> Option<GameContext> {
        match &self.score.start_fen {
            None => Some(GameContext::game_start()),
            Some(fen) => GameContext::from_fen(fen),
        }
    }
}

/// A diagram as the start of the moves after it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Seed {
    pub page: u32,
    pub diagram: usize,
    /// The puzzle number its caption gives.
    pub puzzle: Option<u32>,
    /// The reading's FEN, when it is legal with its side to move.
    pub fen: Option<String>,
    pub accepted: bool,
    pub side_assumed: bool,
}

/// Where an event is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Loc {
    pub page: u32,
    pub rect: Option<UserRect>,
}

/// What became of a move.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MoveOutcome {
    pub status: MoveStatus,
    pub node: Option<NodeRef>,
    pub san: Option<String>,
    pub fen: Option<String>,
}

impl MoveOutcome {
    fn status(status: MoveStatus) -> MoveOutcome {
        MoveOutcome { status, node: None, san: None, fen: None }
    }
}

#[derive(Clone, Debug, Default)]
enum Anchor {
    /// No diagram waits for its moves.
    #[default]
    None,
    Seed(Seed),
    /// The last diagram could not seed its moves: they stay unchecked until the next diagram or
    /// heading.
    Blocked,
}

/// One line being read: the main line, or a variation.
#[derive(Clone, Debug, Default)]
struct Line {
    /// The last move played in it (`None`: its starting position).
    cursor: Option<usize>,
    /// A move was played in it.
    moved: bool,
    /// Printed moves since the last one played (0: the line is going). Starts at 2 for a variation
    /// whose starting position is not known.
    skipped: u32,
    /// The last printed move was played, so an annotation after it is its own.
    last_ok: bool,
    /// Text before the line's first move.
    pending: String,
    /// A variation not yet moved in: the move it was opened after. Its first move may be a reply
    /// to that move instead of an alternative to it.
    after: Option<usize>,
}

#[derive(Clone, Debug)]
struct Open {
    game: usize,
    /// `[0]` is the main line; the rest are the open variations, innermost last.
    lines: Vec<Line>,
    /// Brackets opened beyond [`MAX_VARIATION_DEPTH`], only counted.
    overflow: usize,
    truncated: bool,
    too_large: bool,
    /// Moves whose first reply was read inside brackets: the main line's own reply goes first.
    replies: Vec<usize>,
    /// The diagram the game started from.
    seed: Option<Seed>,
}

/// The finished walk.
#[derive(Debug, Default)]
pub(crate) struct Done {
    pub games: Vec<Game>,
    remaps: Vec<Vec<Option<usize>>>,
    /// Diagrams `(page, number)` that had moves after them but could not seed a game.
    pub blocked: Vec<(u32, usize)>,
}

impl Done {
    /// A node reference made while its game was open, as it is in the finished game.
    pub fn remap(&self, r: NodeRef) -> Option<NodeRef> {
        let node = (*self.remaps.get(r.game)?.get(r.node)?)?;
        Some(NodeRef { game: r.game, node })
    }
}

/// Finds games in a document's events, in reading order.
#[derive(Debug, Default)]
pub(crate) struct Walker {
    games: Vec<Game>,
    remaps: Vec<Vec<Option<usize>>>,
    anchor: Anchor,
    open: Option<Open>,
    /// A diagram came while a game was open: whether the game goes on past it is decided by the
    /// next move. Only set while `open` is.
    held: bool,
    /// Diagrams no game started from yet, oldest first, at most [`MAX_SPARE_DIAGRAMS`].
    spare: Vec<Seed>,
    /// The next game starts from a spare diagram its first move matched.
    matched: bool,
    /// Numbered puzzle diagrams, for the solutions that name them; at most [`MAX_PUZZLES`].
    puzzles: Vec<Seed>,
    blocked: Vec<(u32, usize)>,
}

/// Append text to the comment the current line is at.
fn comment(game: &mut Game, open: &mut Open, text: &str) {
    let depth = open.lines.len().saturating_sub(1);
    let Some(line) = open.lines.last_mut() else { return };
    let cut = match (line.moved, line.cursor) {
        (true, Some(c)) => game.score.nodes.get_mut(c).is_some_and(|node| append_comment(&mut node.comment, text)),
        _ if depth == 0 => append_comment(&mut game.score.comment, text),
        _ => append_comment(&mut line.pending, text),
    };
    open.truncated |= cut;
}

/// Close the innermost variation. Text it held before any move went in goes to the line around
/// it, in brackets.
fn pop_line(game: &mut Game, open: &mut Open) {
    if open.lines.len() <= 1 {
        return;
    }
    if let Some(line) = open.lines.pop()
        && !line.pending.is_empty()
    {
        comment(game, open, &format!("({})", line.pending));
    }
}

enum Decision {
    Play(Option<usize>),
    NotReached,
    OutOfSequence(MoveNumber),
}

fn decide(score: &GameScore, line: &Line, number: Option<MoveNumber>) -> Decision {
    let Some(next) = score.next_ply(line.cursor) else { return Decision::NotReached };
    let lost = || if line.skipped > 0 { Decision::NotReached } else { Decision::OutOfSequence(number_of(next)) };
    match number {
        None if line.skipped > 0 => Decision::NotReached,
        None => Decision::Play(line.cursor),
        Some(n) => {
            let q = ply_of(n);
            if q == next {
                Decision::Play(line.cursor)
            } else if q < next {
                score.before_ply(line.cursor, q).map_or_else(lost, Decision::Play)
            } else {
                lost()
            }
        }
    }
}

/// Whether `san`, numbered `number`, can be played in `line`: it has its place there and is legal.
fn playable(score: &GameScore, line: &Line, number: Option<MoveNumber>, san: &San) -> bool {
    match decide(score, line, number) {
        Decision::Play(at) => score.board_after(at).is_some_and(|b| san.to_move(&b).is_ok()),
        _ => false,
    }
}

/// Whether the main line takes `san`, numbered `number`, while the reading is elsewhere in the
/// game (one line open, not at the main line's last move): the move cannot be played where the
/// reading is, or only as an alternative to a move of that line (its number is earlier than the
/// line's next), while it is the main line's very next move. Books go back to the main line after
/// an alternative written as prose.
fn main_takes(score: &GameScore, lines: &[Line], number: Option<MoveNumber>, san: &San) -> bool {
    let ([line], Some(n)) = (lines, number) else { return false };
    let Some(tip) = score.mainline().last().copied() else { return false };
    if line.cursor == Some(tip) || !playable(score, &Line { cursor: Some(tip), moved: true, ..Line::default() }, number, san) {
        return false;
    }
    let q = ply_of(n);
    !playable(score, line, number, san) || (score.next_ply(Some(tip)) == Some(q) && score.next_ply(line.cursor).is_some_and(|next| q < next))
}

/// Whether `san` is the first move of a game from `seed`, numbered `number`: the number is
/// believed and the move is legal there.
fn seed_fits(seed: &Seed, number: Option<MoveNumber>, san: &San) -> bool {
    seed_score(seed, number).is_some_and(|(score, _, honour)| honour && score.start_board().is_some_and(|b| san.to_move(&b).is_ok()))
}

/// The piece placement of a FEN, its first field.
fn placement_of(fen: &str) -> &str {
    fen.split_whitespace().next().unwrap_or_default()
}

/// A FEN from its six fields.
fn compose(placement: &str, black: bool, castling: &str, ep: &str, halfmove: &str, fullmove: &str) -> String {
    format!("{placement} {} {castling} {ep} {halfmove} {fullmove}", if black { 'b' } else { 'w' })
}

/// The starting position for a diagram's moves, the flags it raised, and whether the first
/// move's number is to be believed. `None` when the reading has no legal position.
fn seed_score(seed: &Seed, number: Option<MoveNumber>) -> Option<(GameScore, Vec<ReviewFlag>, bool)> {
    let fen = seed.fen.as_deref()?;
    let original = Board::from_fen(fen.trim(), false).ok()?;
    let fields: Vec<&str> = fen.split_whitespace().collect();
    let mut flags = Vec::new();
    let mut honour = true;
    let mut board = original.clone();
    if let (Some(n), [placement, side, castling, ep, halfmove, _]) = (number, fields.as_slice()) {
        let mut black = *side == "b";
        let mut ep = *ep;
        if n.black != black {
            let flipped = compose(placement, n.black, castling, "-", halfmove, "1");
            if seed.side_assumed && Board::from_fen(&flipped, false).is_ok() {
                black = n.black;
                ep = "-";
                flags.push(ReviewFlag::SideFromMoveNumber);
            } else {
                flags.push(ReviewFlag::SideConflict);
                honour = false;
            }
        }
        if let Ok(b) = Board::from_fen(&compose(placement, black, castling, ep, halfmove, &n.number.to_string()), false) {
            board = b;
        }
    }
    let mut score = GameScore::new(Some(board.to_string()));
    score.extra_tags.push(Tag { name: "PdfPage".to_owned(), value: seed.page.saturating_add(1).to_string() });
    score.extra_tags.push(Tag { name: "PdfDiagram".to_owned(), value: seed.diagram.saturating_add(1).to_string() });
    if let Some(n) = seed.puzzle {
        score.extra_tags.push(Tag { name: "PdfPuzzle".to_owned(), value: n.to_string() });
    }
    Some((score, flags, honour))
}

impl Walker {
    /// A diagram: the next moves start from it, unless they continue the open game (see
    /// [`Walker::resolve_held`]). A diagram before it that no game started from is kept spare.
    pub fn diagram(&mut self, seed: Seed) {
        if let Some(n) = seed.puzzle {
            match self.puzzles.iter().position(|s| s.puzzle == Some(n)) {
                Some(i) => {
                    if let Some(old) = self.puzzles.get_mut(i) {
                        *old = seed.clone();
                    }
                }
                None if self.puzzles.len() < MAX_PUZZLES => self.puzzles.push(seed.clone()),
                None => {}
            }
        }
        if let Anchor::Seed(old) = std::mem::take(&mut self.anchor) {
            self.keep_spare(old);
        }
        self.held = self.open.is_some();
        self.anchor = Anchor::Seed(seed);
    }

    /// A solution names puzzle `n`: the open game ends and the next moves start from that
    /// puzzle's diagram, however far back it was. Nothing happens for a number no diagram
    /// gave, or when that diagram's moves are being read already (a caption under it).
    pub fn puzzle(&mut self, n: u32) {
        let Some(seed) = self.puzzles.iter().find(|s| s.puzzle == Some(n)).cloned() else { return };
        let same = |s: &Seed| s.page == seed.page && s.diagram == seed.diagram;
        let waiting = matches!(&self.anchor, Anchor::Seed(s) if same(s));
        let playing = self.open.as_ref().is_some_and(|o| o.seed.as_ref().is_some_and(same));
        if waiting || playing {
            return;
        }
        self.close();
        self.anchor = Anchor::Seed(seed);
    }

    /// A solution whose number stands alone before its first move (`1675 1.Rd3+`): as
    /// [`Walker::puzzle`], and for a number no diagram gave, the open game still ends and the
    /// moves stay unchecked, rather than being read into whatever game was open.
    pub fn solution(&mut self, n: u32) {
        if self.puzzles.iter().any(|s| s.puzzle == Some(n)) {
            self.puzzle(n);
        } else {
            self.close();
            self.anchor = Anchor::Blocked;
        }
    }

    /// A heading: the open game ends, and no diagram waits.
    pub fn heading(&mut self) {
        self.close();
        self.anchor = Anchor::None;
        self.spare.clear();
    }

    fn keep_spare(&mut self, seed: Seed) {
        if seed.fen.is_none() {
            return;
        }
        if self.spare.len() >= MAX_SPARE_DIAGRAMS {
            self.spare.remove(0);
        }
        self.spare.push(seed);
    }

    /// The most recent spare diagram, on this page or the one before, that `san` is a first move
    /// from; taken out of the spares.
    fn take_spare(&mut self, number: Option<MoveNumber>, san: &San, page: u32) -> Option<Seed> {
        let i = self.spare.iter().rposition(|s| s.page <= page && s.page.saturating_add(1) >= page && seed_fits(s, number, san))?;
        Some(self.spare.remove(i))
    }

    /// The first move after a diagram that came while a game was open. The game goes on when the
    /// diagram shows the position its main line reached, or when the move continues the main line
    /// (its number is the next one, and it is legal there) and is not a first move from the
    /// diagram. Otherwise the game ends and the move starts from the diagram.
    fn resolve_held(&mut self, number: Option<MoveNumber>, san: &San) {
        self.held = false;
        let seed = match &self.anchor {
            Anchor::Seed(s) => Some(s),
            _ => None,
        };
        let (same, fits) = match self.open.as_ref().and_then(|o| Some((o.lines.first()?, self.games.get(o.game)?))) {
            Some((main, game)) => {
                let board = game.score.board_after(main.cursor);
                let same =
                    seed.and_then(|s| s.fen.as_deref()).zip(board.as_ref()).is_some_and(|(f, b)| placement_of(f) == placement_of(&b.to_string()));
                let next = number.is_some_and(|n| game.score.next_ply(main.cursor) == Some(ply_of(n)));
                let fits = main.moved && main.skipped == 0 && next && board.is_some_and(|b| san.to_move(&b).is_ok());
                (same, fits && !seed.is_some_and(|s| seed_fits(s, number, san)))
            }
            None => (false, false),
        };
        if !same && !fits {
            self.close();
            return;
        }
        let anchor = std::mem::take(&mut self.anchor);
        if let Some(open) = self.open.as_mut()
            && let Some(game) = self.games.get_mut(open.game)
        {
            while open.lines.len() > 1 {
                pop_line(game, open);
            }
            if !same && let Anchor::Seed(s) = &anchor {
                game.flags.push(GameFlag {
                    page: s.page,
                    source_rect: None,
                    flag: ReviewFlag::ContinuedPastDiagram { page: s.page, diagram: s.diagram },
                });
            }
        }
        if !same && let Anchor::Seed(s) = anchor {
            self.keep_spare(s);
        }
    }

    fn close(&mut self) {
        self.held = false;
        let Some(mut open) = self.open.take() else { return };
        let Some(game) = self.games.get_mut(open.game) else { return };
        while open.lines.len() > 1 {
            pop_line(game, &mut open);
        }
        if open.truncated {
            game.flags.push(GameFlag { page: game.page, source_rect: None, flag: ReviewFlag::CommentTruncated });
        }
        let map = game.score.reorder_canonical();
        if let Some(slot) = self.remaps.get_mut(open.game) {
            *slot = map;
        }
    }

    /// Open a game for the move about to be read. Returns whether one is open, and whether the
    /// move's number is to be believed.
    fn start(&mut self, number: Option<MoveNumber>, san: &San, loc: Loc) -> Option<bool> {
        if self.games.len() >= MAX_GAMES {
            return None;
        }
        // A first move that does not fit where it stands may fit a spare diagram nearby.
        let mut matched = std::mem::take(&mut self.matched);
        let first = MoveNumber { number: 1, black: false };
        let anchor = match std::mem::take(&mut self.anchor) {
            Anchor::Seed(seed) if !seed_fits(&seed, number, san) => match self.take_spare(number, san, loc.page) {
                Some(spare) => {
                    matched = true;
                    self.keep_spare(seed);
                    Anchor::Seed(spare)
                }
                None => Anchor::Seed(seed),
            },
            Anchor::Blocked => match self.take_spare(number, san, loc.page) {
                Some(spare) => {
                    matched = true;
                    Anchor::Seed(spare)
                }
                None => Anchor::Blocked,
            },
            Anchor::None if number.is_some_and(|n| n.number == 1) && !(number == Some(first) && san.to_move(&Board::default()).is_ok()) => {
                match self.take_spare(number, san, loc.page) {
                    Some(spare) => {
                        matched = true;
                        Anchor::Seed(spare)
                    }
                    None => Anchor::None,
                }
            }
            other => other,
        };
        let mut started_from = None;
        let (origin, score, mut flags, honour) = match anchor {
            Anchor::Seed(seed) => match seed_score(&seed, number) {
                Some((score, flags, honour)) => {
                    let origin = GameOrigin::Diagram { page: seed.page, diagram: seed.diagram, accepted: seed.accepted };
                    started_from = Some(seed);
                    (origin, score, flags, honour)
                }
                None => {
                    self.blocked.push((seed.page, seed.diagram));
                    self.anchor = Anchor::Blocked;
                    return None;
                }
            },
            Anchor::Blocked => {
                self.anchor = Anchor::Blocked;
                return None;
            }
            Anchor::None if number == Some(MoveNumber { number: 1, black: false }) => {
                let mut score = GameScore::new(None);
                score.extra_tags.push(Tag { name: "PdfPage".to_owned(), value: loc.page.saturating_add(1).to_string() });
                (GameOrigin::Initial, score, Vec::new(), true)
            }
            Anchor::None => return None,
        };
        if matched {
            flags.push(ReviewFlag::DiagramMatchedByMove);
        }
        let trusted = match origin {
            GameOrigin::Initial => true,
            GameOrigin::Diagram { page, diagram, accepted } => {
                if !accepted {
                    flags.push(ReviewFlag::UntrustedStart { page, diagram });
                }
                accepted
            }
        };
        let flags = flags.into_iter().map(|flag| GameFlag { page: loc.page, source_rect: loc.rect, flag }).collect();
        self.open = Some(Open {
            game: self.games.len(),
            lines: vec![Line::default()],
            overflow: 0,
            truncated: false,
            too_large: false,
            replies: Vec::new(),
            seed: started_from,
        });
        self.games.push(Game { origin, trusted, page: loc.page, score, flags });
        self.remaps.push(Vec::new());
        Some(honour)
    }

    /// A printed move.
    pub fn play(&mut self, number: Option<MoveNumber>, san: &San, text: &str, loc: Loc) -> MoveOutcome {
        let printed = format!("{}{}", number.map(|n| n.text()).unwrap_or_default(), text);
        let mut number = number;
        if self.held {
            self.resolve_held(number, san);
        }
        // A `1.` that a spare diagram nearby starts, when the open game cannot play it or would
        // take it as another first move: a new example. A diagram with no text yet is likelier
        // its own than a second solution is.
        if let Some(n) = number
            && n.number == 1
            && let Some(open) = &self.open
            && let [main] = open.lines.as_slice()
            && main.moved
            && let Some(game) = self.games.get(open.game)
            && (game.score.start_ply() == Some(ply_of(n)) || !playable(&game.score, main, number, san))
            && let Some(spare) = self.take_spare(number, san, loc.page)
        {
            self.close();
            self.anchor = Anchor::Seed(spare);
            self.matched = true;
        }
        // The other side's move of the game's first move number, a first move from the game's own
        // diagram: the text gave one side's solution, then the other's (`Black, if on move, holds
        // by 1...f5`, then `1.f5+`). It starts a second game from the diagram.
        if let Some(n) = number
            && let Some(open) = &self.open
            && open.lines.len() == 1
            && let Some(seed) = &open.seed
            && let Some(game) = self.games.get(open.game)
            && game.score.start_ply() == Some(ply_of(n).saturating_add(1))
            && seed_fits(seed, number, san)
        {
            let seed = seed.clone();
            self.close();
            self.anchor = Anchor::Seed(seed);
        }
        // A new `1.` after a game from the initial position has moved on is a new game. So is a
        // `1.` that a game from a later move's diagram cannot play and the initial position can:
        // a game collection's next game, after the diagrams of the last one.
        let first = MoveNumber { number: 1, black: false };
        if number == Some(first)
            && let Some(open) = &self.open
            && let [main] = open.lines.as_slice()
            && main.moved
            && let Some(game) = self.games.get(open.game)
            && (game.origin == GameOrigin::Initial
                || (game.score.start_ply().is_some_and(|p| p > ply_of(first).saturating_add(1))
                    && !playable(&game.score, main, number, san)
                    && san.to_move(&Board::default()).is_ok()))
        {
            self.close();
            self.anchor = Anchor::None;
        }
        if self.open.is_none() {
            match self.start(number, san, loc) {
                Some(true) => {}
                Some(false) => number = None,
                None => return MoveOutcome::status(MoveStatus::Unchecked),
            }
        }
        let Some(open) = self.open.as_mut() else { return MoveOutcome::status(MoveStatus::Unchecked) };
        let gi = open.game;
        let Some(game) = self.games.get_mut(gi) else { return MoveOutcome::status(MoveStatus::Unchecked) };
        // A numbered move that the main line takes (see `main_takes`): the text went back to the
        // main line after an alternative written as prose (`2.Kg7 ... The premature 2.h4+ loses to
        // 2...Kxh4 ... 2...c5`).
        if main_takes(&game.score, &open.lines, number, san)
            && let [line] = open.lines.as_mut_slice()
            && let Some(tip) = game.score.mainline().last().copied()
        {
            *line = Line { cursor: Some(tip), moved: true, ..Line::default() };
        }
        // A bracket opened after a move can hold the replies to it (`1.Kb1 (1...b3 2.Kb2; 1...a3)`):
        // a first move numbered as the move after it is played after it.
        if let Some(n) = number
            && let Some(line) = open.lines.last_mut()
            && !line.moved
            && let Some(after) = line.after
            && game.score.nodes.get(after).is_some_and(|m| ply_of(n) == m.ply.saturating_add(1))
            && playable(&game.score, &Line { cursor: Some(after), ..Line::default() }, number, san)
        {
            line.cursor = Some(after);
            line.after = None;
            if game.score.children(Some(after)).is_empty() && !open.replies.contains(&after) {
                open.replies.push(after);
            }
        }
        let Some(line) = open.lines.last() else { return MoveOutcome::status(MoveStatus::Unchecked) };
        let (status, flag) = match decide(&game.score, line, number) {
            Decision::NotReached => (MoveStatus::NotReached, None),
            Decision::OutOfSequence(expected) => {
                (MoveStatus::OutOfSequence, Some(ReviewFlag::OutOfSequence { text: printed.clone(), expected: expected.text() }))
            }
            Decision::Play(at) => match game.score.play(at, san, true) {
                Ok(node) => {
                    let pending = open.lines.last_mut().map(|l| std::mem::take(&mut l.pending)).unwrap_or_default();
                    if !pending.is_empty() {
                        open.truncated |= game.score.comment_before(node, &pending);
                    }
                    if let Some(line) = open.lines.last_mut() {
                        *line = Line { cursor: Some(node), moved: true, skipped: 0, last_ok: true, ..Line::default() };
                    }
                    // The main line's reply to a move whose replies came first in brackets.
                    if open.lines.len() == 1
                        && let Some(parent) = game.score.nodes.get(node).and_then(|n| n.parent)
                        && let Some(i) = open.replies.iter().position(|&r| r == parent)
                    {
                        open.replies.swap_remove(i);
                        game.score.put_first(node);
                    }
                    let n = game.score.nodes.get(node);
                    return MoveOutcome {
                        status: MoveStatus::Valid,
                        node: Some(NodeRef { game: gi, node }),
                        san: n.map(|n| n.san.clone()),
                        fen: n.map(|n| n.fen.clone()),
                    };
                }
                Err(PlayError::Illegal(near)) => (MoveStatus::Illegal, Some(ReviewFlag::IllegalMove { text: printed.clone(), near })),
                Err(PlayError::Ambiguous(candidates)) => {
                    (MoveStatus::Ambiguous, Some(ReviewFlag::AmbiguousMove { text: printed.clone(), candidates }))
                }
                Err(PlayError::NoPosition) => (MoveStatus::NotReached, None),
                Err(PlayError::TooLarge) => {
                    let first = !open.too_large;
                    open.too_large = true;
                    (MoveStatus::NotReached, first.then_some(ReviewFlag::GameTooLarge))
                }
            },
        };
        if let Some(flag) = flag {
            game.flags.push(GameFlag { page: loc.page, source_rect: loc.rect, flag });
        }
        if let Some(line) = open.lines.last_mut() {
            line.skipped = line.skipped.saturating_add(1);
            line.last_ok = false;
        }
        comment(game, open, &printed);
        MoveOutcome::status(status)
    }

    /// An annotation glyph: the last move's, when it was played; else part of the comment.
    pub fn nag(&mut self, code: u16) {
        if self.held {
            return;
        }
        let Some(open) = self.open.as_mut() else { return };
        let Some(game) = self.games.get_mut(open.game) else { return };
        let Some(line) = open.lines.last() else { return };
        if line.last_ok
            && let Some(node) = line.cursor.and_then(|c| game.score.nodes.get_mut(c))
            && (node.nags.contains(&code) || node.nags.len() < MAX_NAGS)
        {
            if !node.nags.contains(&code) {
                node.nags.push(code);
            }
            return;
        }
        let glyph = pdfcraft_chess::nag::by_code(code).map_or_else(|| format!("${code}"), |n| n.glyph.to_owned());
        comment(game, open, &glyph);
    }

    /// Prose, punctuation and brackets between moves.
    pub fn text(&mut self, text: &str) {
        // Captions and prose between a diagram and the move that settles it belong to neither.
        if self.held {
            return;
        }
        let mut segment = String::new();
        for ch in text.chars() {
            if self.open.is_none() {
                return;
            }
            match ch {
                '(' | '[' => {
                    self.prose(&std::mem::take(&mut segment));
                    self.open_variation();
                }
                ')' | ']' => {
                    self.prose(&std::mem::take(&mut segment));
                    self.close_variation();
                }
                _ => segment.push(ch),
            }
        }
        self.prose(&segment);
    }

    fn prose(&mut self, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        let Some(open) = self.open.as_mut() else { return };
        let Some(game) = self.games.get_mut(open.game) else { return };
        if open.lines.len() == 1 {
            let mut before = String::new();
            for word in text.split_whitespace() {
                if let Some(result) = GameResult::from_text(word.trim_end_matches(['.', ',', ';', ':'])) {
                    comment(game, open, &before);
                    game.score.result = result;
                    self.close();
                    self.anchor = Anchor::None;
                    return;
                }
                before.push_str(word);
                before.push(' ');
            }
        }
        comment(game, open, text);
    }

    fn open_variation(&mut self) {
        let Some(open) = self.open.as_mut() else { return };
        if open.lines.len() > MAX_VARIATION_DEPTH {
            open.overflow = open.overflow.saturating_add(1);
            return;
        }
        let Some(game) = self.games.get(open.game) else { return };
        let Some(parent) = open.lines.last() else { return };
        let (cursor, skipped, after) = match (parent.moved, parent.skipped) {
            // An alternative to the last move: from the position before it.
            (true, 0) => match parent.cursor.and_then(|c| game.score.nodes.get(c)) {
                Some(last) => (last.parent, 0, parent.cursor),
                None => (parent.cursor, 2, None),
            },
            // An alternative to the one printed move that could not be played.
            (_, 1) => (parent.cursor, 0, None),
            // Nothing to be an alternative to, or the position is lost.
            _ => (parent.cursor, 2, None),
        };
        open.lines.push(Line { cursor, moved: false, skipped, after, ..Line::default() });
    }

    fn close_variation(&mut self) {
        let Some(open) = self.open.as_mut() else { return };
        if open.overflow > 0 {
            open.overflow -= 1;
            return;
        }
        if let Some(game) = self.games.get_mut(open.game) {
            pop_line(game, open);
        }
    }

    /// Close the open game and hand the games over.
    pub fn finish(mut self) -> Done {
        self.close();
        Done { games: self.games, remaps: self.remaps, blocked: self.blocked }
    }
}
