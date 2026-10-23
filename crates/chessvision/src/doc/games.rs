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
//! - **"Position after" captions.** Books caption a diagram with the move that led to it
//!   (`Position after: 11...Ba6`, `Posição após 23.Nf5`). After a diagram whose next words say so,
//!   a numbered move that cannot be played from it and whose piece already stands on its square
//!   is that caption: it is not played again but gives the diagram its side to move and number
//!   (White to move at 12 after `11...Ba6`), and is [`MoveStatus::Caption`]. A reading whose side
//!   was not assumed and says otherwise keeps its side, and the move is read as any other; so is
//!   one the open game can play (a diagram reprinting the game's position names its next move).
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
//! - **Weight.** Annotated books print the main line in bold and their analysis in regular type.
//!   In a game with moves of both weights, a bold move after regular ones goes on from the last
//!   bold move when it can be played there (an unnumbered one only when that bold move was
//!   played), and a bold move played on the bold line makes it the main line, so analysis printed
//!   first or going on from the main line's last move stays a variation. A bold move that only
//!   fits inside the analysis changes nothing. Moves all of one weight (OCR text, documents read
//!   before the weight was kept) are read as above.
//! - **Comments and NAGs.** Prose inside a game is the comment of the move before it. An annotation
//!   glyph belongs to the move it follows.
//! - **Puzzles.** A diagram whose caption gives a number (`№185.`, `Exercício 12`) is remembered
//!   by it. A solution that names it (`№185. Teichmann – Lasker 1...Qh4`, a heading `Ex. 6–12`),
//!   however many pages on, closes the open game and starts its moves from that diagram (tagged
//!   `PdfPuzzle`); headings between the name and the first move (the players, the event) keep
//!   it. When several chapters number their exercises from 1, the solution's chapter is found
//!   from the structure: the chapter its label gives, a heading over it that numbers or names
//!   one chapter, or else the chapters' order ([`puzzles`]).
//! - **Where a game ends.** At the next diagram, at a heading, at a result (`1-0`, `0-1`, `½-½`,
//!   `1/2-1/2`) outside variations, and at a new `1.` when the game started from the initial
//!   position. Paragraphs and page breaks do not end a game. A diagram does not end it when it
//!   shows the position the main line reached (a book reprinting the game's position, often on
//!   the next page), or when the next move continues the main line and is not a first move from
//!   the diagram (flagged [`ReviewFlag::ContinuedPastDiagram`]); text between the diagram and that
//!   move goes to neither game.
//!
//! - **Rules for selections.** [`WalkRules`] adds rules first written for a selection of the page
//!   (an assumed lost bracket, moves in prose, sibling separators, more results, a lone
//!   ellipsis); a whole book reads with [`WalkRules::BOOK`], and [`Walker::fragment`] reads one
//!   game from a given position with all of them ([`super::fragment`]).
//! - **On the chess board.** [`to_game`] makes a score the chess board's game, and
//!   [`super::ChessDoc::board_game`] does it with each move's place on its page.
//!
//! Everything is bounded: [`MAX_GAME_NODES`] moves a game, [`MAX_VARIATION_DEPTH`] nested
//! variations, [`MAX_COMMENT_CHARS`] a comment, [`MAX_GAMES`] games a document.

use cozy_chess::{Board, Color, Move};
use pdfcraft_chess::repair::GameContext;
use pdfcraft_chess::san::{San, SanBody, SanError};
use serde::{Deserialize, Serialize};

use super::{MoveNumber, MoveStatus, ReviewFlag, UserRect};

mod board;
mod puzzles;

pub use board::{BoardGame, to_game};
pub(crate) use puzzles::{Key, Name};

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
/// Headings between a solution's label and its first move that keep the diagram it named: the
/// players' line and the event's, in books that set them as headings.
pub const MAX_NAMED_HEADINGS: u8 = 2;

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

    /// Whether the line to node `to` goes through node `at` (`None`: the starting position, which
    /// every line goes through).
    fn leads_to(&self, at: Option<usize>, to: Option<usize>) -> bool {
        let Some(at) = at else { return true };
        let mut cur = to;
        for _ in 0..=self.nodes.len() {
            let Some(c) = cur else { return false };
            if c == at {
                return true;
            }
            cur = self.nodes.get(c).and_then(|node| node.parent);
        }
        false
    }

    /// Make the line through node `n` the main line: `n` and each move before it go first.
    fn put_line_first(&mut self, n: usize) {
        let mut cur = Some(n);
        for _ in 0..=self.nodes.len() {
            let Some(c) = cur else { break };
            self.put_first(c);
            cur = self.nodes.get(c).and_then(|node| node.parent);
        }
    }

    /// Put text that came before node `n`: its own `comment_before` when it starts a variation,
    /// else the comment of the move before it (or of the game).
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
    /// The position an illegal or ambiguous move was tried in, as a FEN (PGN toolbar T10: where
    /// the legal moves near it are looked for).
    pub tried: Option<String>,
}

impl MoveOutcome {
    fn status(status: MoveStatus) -> MoveOutcome {
        MoveOutcome { status, node: None, san: None, fen: None, tried: None }
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
    /// The last move a bold move put on the main level (`None`: none yet, the starting position).
    /// Books print their main line in bold and their analysis in regular type.
    bold_tip: Option<usize>,
    /// A regular move was read on the main level since the last bold one (or since the start):
    /// the reading may have left the bold line.
    strayed: bool,
    /// A regular move was read in this game: with bold ones, the book marks its main line by
    /// weight.
    regular_seen: bool,
    /// The last bold move read on the main level could not be played.
    bold_failed: bool,
}

impl Open {
    fn new(game: usize, seed: Option<Seed>) -> Open {
        Open {
            game,
            lines: vec![Line::default()],
            overflow: 0,
            truncated: false,
            too_large: false,
            replies: Vec::new(),
            seed,
            bold_tip: None,
            strayed: false,
            regular_seen: false,
            bold_failed: false,
        }
    }
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
    /// Numbered puzzle diagrams, by chapter, for the solutions that name them; at most
    /// [`MAX_PUZZLES`].
    puzzles: puzzles::Puzzles,
    blocked: Vec<(u32, usize)>,
    /// The reading rules beyond the walker's own.
    rules: WalkRules,
    /// Fragment mode ([`Walker::fragment`]): one game from a given position, never split.
    fragment: bool,
    /// Prose words were read since the last move or bracket ([`WalkRules::prose_moves`]).
    prose_seen: bool,
    /// The text since the last move ends in an ellipsis standing alone ([`WalkRules::ellipsis`]).
    ellipsis: bool,
    /// A separator was read inside brackets ([`WalkRules::separators`]).
    separator: Separator,
    /// In fragment mode, the text after the result that ended the game.
    rest: Option<String>,
    /// The words just before the move about to be read, after a diagram no move has settled yet,
    /// end in a "position after" cue ([`after_cue`]).
    caption: bool,
    /// The diagram (page, index) a solution named ([`Walker::puzzle`]) while it waits for its
    /// first move, and how many headings kept it since: the players and the event before that
    /// move keep it, up to [`MAX_NAMED_HEADINGS`].
    named: Option<(u32, usize, u8)>,
}

/// A separator between sibling variations, waiting for the move after it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Separator {
    #[default]
    None,
    /// `;`: taken by a move that can be played as a sibling, numbered or with no prose before it.
    Weak,
    /// `a)`, `b)`: taken by the next move.
    Strong,
}

/// Reading rules the walker can apply beyond its own (PGN toolbar spec, step T2a). Fragment mode
/// ([`Walker::fragment`]) applies them all; reading a whole book applies [`WalkRules::BOOK`],
/// those `examples/walk_bench.rs` showed do not make it worse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WalkRules {
    /// A numbered move that cannot be played in the variation being read but can on a line
    /// outside it closes the variations in between: their bracket was lost (OCR drops them).
    /// Flagged [`ReviewFlag::ImpliedClose`].
    pub implied_close: bool,
    /// A move without a number after prose words (since the last move or bracket) is prose and
    /// stays in the comment (`the break …f5`), unless it is set in bold.
    pub prose_moves: bool,
    /// Inside brackets, `;` before a move that can start a sibling variation (numbered, or
    /// right after it with no prose between), and a label `a)`, `(b)` before any move, close the
    /// variation and open its sibling. A label is never a bracket.
    pub separators: bool,
    /// `1:0`, `0:1`, `½:½`, `1/2:1/2` and a result in brackets (`(1-0)`) end the game too.
    pub results: bool,
    /// A move without a number after an ellipsis standing alone (`… Rc7`; the fragment's text
    /// reader splits `…Rc7` so) is Black's.
    pub ellipsis: bool,
}

impl WalkRules {
    /// None of the rules: the walker's own reading.
    pub const NONE: WalkRules = WalkRules { implied_close: false, prose_moves: false, separators: false, results: false, ellipsis: false };
    /// Every rule: fragment mode.
    pub const ALL: WalkRules = WalkRules { implied_close: true, prose_moves: true, separators: true, results: true, ellipsis: true };
    /// The rules a whole book is read with: those that made no move of ten earlier documents
    /// worse (`walk_bench --rules=`, 2,304 -> 2,327 valid moves, 23 better and none worse, all
    /// from `ellipsis`; the other two changed nothing there). `prose_moves` made 88 worse (books go
    /// on after prose without numbers), and `results` 3, all played into the wrong game before a
    /// result it now reads (`0:1`) ended the right one; both stay for fragments only.
    pub const BOOK: WalkRules = WalkRules { implied_close: true, prose_moves: false, separators: true, results: false, ellipsis: true };
}

impl Default for WalkRules {
    fn default() -> WalkRules {
        WalkRules::BOOK
    }
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

/// A variation opened in `parent`: an alternative to its last move.
fn variation_line(score: &GameScore, parent: &Line) -> Line {
    let (cursor, skipped, after) = match (parent.moved, parent.skipped) {
        // An alternative to the last move: from the position before it.
        (true, 0) => match parent.cursor.and_then(|c| score.nodes.get(c)) {
            Some(last) => (last.parent, 0, parent.cursor),
            None => (parent.cursor, 2, None),
        },
        // An alternative to the one printed move that could not be played.
        (_, 1) => (parent.cursor, 0, None),
        // Nothing to be an alternative to, or the position is lost.
        _ => (parent.cursor, 2, None),
    };
    Line { cursor, moved: false, skipped, after, ..Line::default() }
}

/// Whether `san`, numbered `number`, can be played as the first move of `line`, a variation just
/// opened: as a reply to the move it was opened after (see [`Walker::play`]) or in it.
fn opens_with(score: &GameScore, line: &Line, number: Option<MoveNumber>, san: &San) -> bool {
    let reply = number.is_some_and(|n| {
        line.after.is_some_and(|a| score.nodes.get(a).is_some_and(|m| ply_of(n) == m.ply.saturating_add(1)))
            && playable(score, &Line { cursor: line.after, ..Line::default() }, number, san)
    });
    reply || playable(score, line, number, san)
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

/// A word that is a result: `1-0`, `0-1`, `½-½`, `1/2-1/2`, before a full stop or comma. With
/// `extended` ([`WalkRules::results`]) also with a colon (`1:0`) or a dash (`1–0`).
fn result_word(word: &str, extended: bool) -> Option<GameResult> {
    let core = word.trim_end_matches(['.', ',', ';', ':']);
    GameResult::from_text(core).or_else(|| {
        if !extended {
            return None;
        }
        let plain: String = core.chars().map(|c| if matches!(c, ':' | '–' | '—') { '-' } else { c }).collect();
        GameResult::from_text(&plain)
    })
}

/// A result closed by a bracket at the start of `text` (the text after an opening bracket):
/// the result and the bytes up to and including the bracket.
fn bracketed_result(text: &str) -> Option<(GameResult, usize)> {
    let end = text.find([')', ']'])?;
    // The longest result, `1/2-1/2`, with a space on each side.
    if end > 12 {
        return None;
    }
    let result = result_word(text.get(..end)?.trim(), true)?;
    Some((result, end.saturating_add(1)))
}

/// Whether `text` (the text before a `)`) ends in a label: one letter `a` to `h` alone.
fn is_label(text: &str) -> bool {
    let mut rev = text.chars().rev();
    matches!(rev.next(), Some('a'..='h')) && rev.next().is_none_or(char::is_whitespace)
}

/// The words a caption puts before the move that led to its diagram, lower case, in the languages
/// books are read in. `after` alone is enough: the move must also be the one the diagram shows
/// played ([`led_to`]), which a move still to come never is.
const AFTER_CUES: [&str; 12] = ["after", "após", "apos", "depois de", "nach", "après", "apres", "después de", "despues de", "tras", "dopo", "na"];

/// Whether `text` (the words before a move) ends in a "position after" cue: `Position after:`,
/// `Posição após`, `Stellung nach`, with or without a colon.
fn after_cue(text: &str) -> bool {
    let lower = text.trim().trim_end_matches([':', '\u{a0}']).trim_end().to_lowercase();
    AFTER_CUES.iter().any(|cue| {
        lower.strip_suffix(cue).is_some_and(|before| before.is_empty() || before.ends_with(|c: char| c.is_whitespace() || c == '(' || c == '['))
    })
}

/// Whether `san`, played by Black when `black`, is the move that led to the position `fen`: the
/// piece it moves stands on its square (after castling, the king and the rook on theirs). A move
/// still to be played never is: no move ends on a square its own side holds.
fn led_to(fen: &str, san: &San, black: bool) -> bool {
    use pdfcraft_chess::placement::{Color as Side, Piece, PieceKind, Square};
    use pdfcraft_chess::san::Castle;
    let Ok(placement) = pdfcraft_chess::fen::parse_placement(placement_of(fen)) else { return false };
    let color = if black { Side::Black } else { Side::White };
    let holds = |file: u8, rank: u8, kind: PieceKind| Square::new(file, rank).and_then(|sq| placement.get(sq)) == Some(Piece { color, kind });
    let home = if black { 7 } else { 0 };
    match &san.body {
        SanBody::Castle(Castle::Short) => holds(6, home, PieceKind::King) && holds(5, home, PieceKind::Rook),
        SanBody::Castle(Castle::Long) => holds(2, home, PieceKind::King) && holds(3, home, PieceKind::Rook),
        SanBody::Move { piece, to, promotion, .. } => holds(to.file(), to.rank(), promotion.or(*piece).unwrap_or(PieceKind::Pawn)),
        SanBody::Null => false,
    }
}

/// The castling field `castling` without the rights the caption move `san` (Black's when `black`)
/// took away: castling or a king move takes both of its side's, a rook arriving on its own corner
/// that corner's (it was away from it). `-` when none is left.
fn rights_after(castling: &str, san: &San, black: bool) -> String {
    use pdfcraft_chess::placement::PieceKind;
    let (short, long) = if black { ('k', 'q') } else { ('K', 'Q') };
    let home = if black { 7 } else { 0 };
    let lost: &[char] = match &san.body {
        SanBody::Castle(_) | SanBody::Move { piece: Some(PieceKind::King), .. } => &[short, long],
        SanBody::Move { piece: Some(PieceKind::Rook), to, .. } if to.rank() == home && to.file() == 7 => &[short],
        SanBody::Move { piece: Some(PieceKind::Rook), to, .. } if to.rank() == home && to.file() == 0 => &[long],
        _ => &[],
    };
    let kept: String = castling.chars().filter(|c| *c != '-' && !lost.contains(c)).collect();
    if kept.is_empty() { "-".to_owned() } else { kept }
}

/// The en passant square the caption move `san` (Black's when `black`) leaves in `placement`, when
/// it is a pawn's step to its fourth rank that could have been a two-square step (the square it
/// passed and the one it left are empty). A one-square step looks the same; the square only
/// matters to a capture en passant printed next, which a one-square step would not allow.
fn passed_square(placement: &str, san: &San, black: bool) -> Option<String> {
    use pdfcraft_chess::placement::Square;
    let SanBody::Move { piece: None, capture: false, to, promotion: None, .. } = &san.body else { return None };
    let (fourth, passed, start) = if black { (4, 5, 6) } else { (3, 2, 1) };
    let board = pdfcraft_chess::fen::parse_placement(placement).ok()?;
    let empty = |rank: u8| Square::new(to.file(), rank).is_some_and(|sq| board.get(sq).is_none());
    (to.rank() == fourth && empty(passed) && empty(start)).then(|| Square::new(to.file(), passed).map(|sq| sq.to_string())).flatten()
}

/// `seed` as it stands after its caption `san`, numbered `number` ([`led_to`]): the other side to
/// move, at the move after it, without the castling rights the move took away and with the en
/// passant square a two-square pawn step leaves. `None` when the caption does not fit: the move is
/// not the one the diagram shows played, the reading says the side and it is not the one after the
/// caption, or the position is not legal with that side to move.
fn after_caption(seed: &Seed, number: MoveNumber, san: &San) -> Option<Seed> {
    let fen = seed.fen.as_deref()?;
    let fields: Vec<&str> = fen.split_whitespace().collect();
    let [placement, side, castling, _, halfmove, _] = fields.as_slice() else { return None };
    let next = number_of(ply_of(number).checked_add(1)?);
    if !led_to(fen, san, number.black) || (!seed.side_assumed && (*side == "b") != next.black) {
        return None;
    }
    let number = next.number.to_string();
    let rights = rights_after(castling, san, !next.black);
    let ep = passed_square(placement, san, !next.black).unwrap_or_else(|| "-".to_owned());
    // Rights the reading guessed from the placement can still be wrong for the other side, and
    // an en passant square needs a pawn to take it: the most that makes a legal FEN is kept.
    let fen = [(rights.as_str(), ep.as_str()), (rights.as_str(), "-"), ("-", "-")]
        .into_iter()
        .map(|(c, e)| compose(placement, next.black, c, e, halfmove, &number))
        .find(|f| Board::from_fen(f, false).is_ok())?;
    Some(Seed { fen: Some(fen), side_assumed: false, ..seed.clone() })
}

impl Walker {
    /// A walker for a whole document that reads with `rules`.
    pub fn with_rules(rules: WalkRules) -> Walker {
        Walker { rules, ..Walker::default() }
    }

    /// A walker in fragment mode: one game from `start_fen`, with every [`WalkRules`] rule. A
    /// numbered `1.` does not start another game, and nothing starts one after its result.
    /// `None` when the FEN is not a legal position.
    pub fn fragment(start_fen: &str) -> Option<Walker> {
        let score = GameScore::new(Some(start_fen.trim().to_owned()));
        score.start_board()?;
        let mut walker = Walker { rules: WalkRules::ALL, fragment: true, ..Walker::default() };
        walker.open = Some(Open::new(0, None));
        walker.games.push(Game { origin: GameOrigin::Initial, trusted: true, page: 0, score, flags: Vec::new() });
        walker.remaps.push(Vec::new());
        Some(walker)
    }

    /// Whether the open game can play `san`, numbered `number`, on the line being read: a
    /// diagram that reprints the game's position names the game's next move as its caption, and
    /// that move is the game's.
    fn open_plays(&self, number: Option<MoveNumber>, san: &San) -> bool {
        self.open.as_ref().is_some_and(|open| {
            self.games.get(open.game).is_some_and(|game| open.lines.last().is_some_and(|line| playable(&game.score, line, number, san)))
        })
    }

    /// Whether a game is being read.
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// In fragment mode, the text after the result that ended the game.
    pub fn take_rest(&mut self) -> Option<String> {
        self.rest.take()
    }

    /// A diagram: the next moves start from it, unless they continue the open game (see
    /// [`Walker::resolve_held`]). A diagram before it that no game started from is kept spare.
    pub fn diagram(&mut self, seed: Seed) {
        self.puzzles.keep(&seed);
        if let Anchor::Seed(old) = std::mem::take(&mut self.anchor) {
            self.keep_spare(old);
        }
        self.held = self.open.is_some();
        self.anchor = Anchor::Seed(seed);
        self.caption = false;
    }

    /// A solution names a puzzle: the open game ends and the next moves start from that
    /// puzzle's diagram, however far back it was, in the chapter the structure gives
    /// ([`puzzles`]). Nothing happens for a number no diagram gave, or when that diagram's moves
    /// are being read already (a caption under it).
    pub fn puzzle(&mut self, name: Name) {
        let Some(seed) = self.puzzles.find(name) else { return };
        let same = |s: &Seed| s.page == seed.page && s.diagram == seed.diagram;
        let waiting = matches!(&self.anchor, Anchor::Seed(s) if same(s));
        let playing = self.open.as_ref().is_some_and(|o| o.seed.as_ref().is_some_and(same));
        if waiting || playing {
            return;
        }
        self.close();
        self.named = Some((seed.page, seed.diagram, 0));
        self.anchor = Anchor::Seed(seed);
    }

    /// A solution whose number stands alone before its first move (`1675 1.Rd3+`): as
    /// [`Walker::puzzle`], and for a number no diagram gave, the open game still ends and the
    /// moves stay unchecked, rather than being read into whatever game was open.
    pub fn solution(&mut self, name: Name) {
        if self.puzzles.has(name.number) {
            self.puzzle(name);
        } else {
            self.close();
            self.anchor = Anchor::Blocked;
        }
    }

    /// A heading, with what it says about its chapter ([`Key`]): the open game ends, and no
    /// diagram waits, except the one a solution named while no move has been read from it yet
    /// (`Ex. 6–12`, then `A. Yusupov — D. Shapiro` set as a heading), for [`MAX_NAMED_HEADINGS`]
    /// headings: a third is a new section's.
    pub fn heading(&mut self, keys: Vec<Key>) {
        self.puzzles.heading(keys);
        if self.open.is_none()
            && let Anchor::Seed(seed) = &self.anchor
            && let Some((page, diagram, kept)) = self.named
            && (page, diagram) == (seed.page, seed.diagram)
            && kept < MAX_NAMED_HEADINGS
        {
            self.named = Some((page, diagram, kept.saturating_add(1)));
            return;
        }
        self.named = None;
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
        self.open = Some(Open::new(self.games.len(), started_from));
        self.games.push(Game { origin, trusted, page: loc.page, score, flags });
        self.remaps.push(Vec::new());
        Some(honour)
    }

    /// Where the move about to be read ends the open game and starts another (whole-book reading
    /// only; see [`Walker::play`]).
    fn split(&mut self, number: Option<MoveNumber>, san: &San, loc: Loc) {
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
    }

    /// A printed move; `bold` when it is set in bold (the book's main line).
    pub fn play(&mut self, number: Option<MoveNumber>, san: &San, text: &str, bold: bool, loc: Loc) -> MoveOutcome {
        let printed = format!("{}{}", number.map(|n| n.text()).unwrap_or_default(), text);
        let mut number = number;
        // The solution's moves have begun: a heading after them ends it. When its first move does
        // not start from the exercise it named (the game from move 1, printed before it reaches
        // the exercise's position), it is read as the heading left it: from no diagram.
        if let Some((page, diagram, _)) = self.named.take()
            && self.open.is_none()
            && let Anchor::Seed(seed) = &self.anchor
            && (seed.page, seed.diagram) == (page, diagram)
            && !seed_fits(seed, number, san)
        {
            self.anchor = Anchor::None;
            self.spare.clear();
        }
        // The move a "position after" caption names: not played, it settles the diagram's side
        // and number.
        if std::mem::take(&mut self.caption)
            && !self.fragment
            && !self.open_plays(number, san)
            && let (Anchor::Seed(seed), Some(n)) = (&self.anchor, number)
            && !seed_fits(seed, number, san)
            && let Some(settled) = after_caption(seed, n, san)
        {
            self.anchor = Anchor::Seed(settled);
            return MoveOutcome::status(MoveStatus::Caption);
        }
        if !self.fragment {
            self.split(number, san, loc);
        }
        if self.open.is_none() {
            // A fragment is one game: after its result nothing starts another.
            if self.fragment {
                return MoveOutcome::status(MoveStatus::Unchecked);
            }
            match self.start(number, san, loc) {
                Some(true) => {}
                Some(false) => number = None,
                None => return MoveOutcome::status(MoveStatus::Unchecked),
            }
        }
        let rules = self.rules;
        let prose_seen = std::mem::take(&mut self.prose_seen);
        let ellipsis = std::mem::take(&mut self.ellipsis);
        let separator = std::mem::take(&mut self.separator);
        let Some(open) = self.open.as_mut() else { return MoveOutcome::status(MoveStatus::Unchecked) };
        let gi = open.game;
        let Some(game) = self.games.get_mut(gi) else { return MoveOutcome::status(MoveStatus::Unchecked) };
        // A move in prose stays in the comment: one without a number after prose words
        // (`the break f5`), and in a fragment one numbered before the fragment's start, which no
        // line of it can hold (`as after 1.e4`).
        let citation = self.fragment && number.is_some_and(|n| game.score.start_ply().is_some_and(|s| ply_of(n) < s));
        if citation || (rules.prose_moves && prose_seen && number.is_none() && !bold) {
            comment(game, open, &printed);
            self.prose_seen = true;
            return MoveOutcome::status(MoveStatus::Unchecked);
        }
        // A move after a lone ellipsis is Black's: the next Black move of the line, or the reply
        // to the move a fresh variation was opened after, or else an alternative to the line's
        // last Black move.
        if rules.ellipsis
            && ellipsis
            && number.is_none()
            && let Some(line) = open.lines.last()
            && let Some(next) = game.score.next_ply(line.cursor)
        {
            let ply = if next % 2 == 1 {
                Some(next)
            } else if !line.moved && line.after.is_some() {
                next.checked_add(1)
            } else {
                next.checked_sub(1)
            };
            number = ply.map(number_of);
        }
        // A separator inside brackets: the variation ends and its sibling starts, when the move
        // can start it (`;`, before a numbered move or one right after it) or whatever the move
        // (`b)`).
        if rules.separators
            && (separator == Separator::Strong || (separator == Separator::Weak && (number.is_some() || !prose_seen)))
            && let Some(parent) = open.lines.len().checked_sub(2).and_then(|k| open.lines.get(k))
        {
            let sibling = variation_line(&game.score, parent);
            if separator == Separator::Strong || opens_with(&game.score, &sibling, number, san) {
                pop_line(game, open);
                open.lines.push(sibling);
            }
        }
        open.regular_seen |= !bold;
        // A bold move after regular ones goes on the bold line, when it can be played there: the
        // regular moves were analysis (`1...Be6+ 2.Kg6`, then the game's `1...Bxg7?`), even when
        // they came first or went on from the bold line's last move. An unnumbered one only goes
        // on from a bold move that was played: after one that failed (`20.exd5`, then `e5`), it
        // would be read a ply early.
        let mut placed = false;
        if bold
            && open.strayed
            && (number.is_some() || (open.bold_tip.is_some() && !open.bold_failed))
            && let [line] = open.lines.as_mut_slice()
        {
            let base = Line { cursor: open.bold_tip, moved: open.bold_tip.is_some(), ..Line::default() };
            if playable(&game.score, &base, number, san) {
                placed = true;
                *line = base;
            }
        }
        // A numbered move that the main line takes (see `main_takes`): the text went back to the
        // main line after an alternative written as prose (`2.Kg7 ... The premature 2.h4+ loses to
        // 2...Kxh4 ... 2...c5`).
        if !placed
            && main_takes(&game.score, &open.lines, number, san)
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
        // A numbered move that only a line outside the variation being read can take: the
        // variation's closing bracket was lost.
        if rules.implied_close
            && number.is_some()
            && let Some(current) = open.lines.last()
            && !playable(&game.score, current, number, san)
            && let Some(k) =
                (0..open.lines.len().saturating_sub(1)).rev().find(|&k| open.lines.get(k).is_some_and(|l| playable(&game.score, l, number, san)))
        {
            while open.lines.len() > k.saturating_add(1) {
                pop_line(game, open);
            }
            game.flags.push(GameFlag { page: loc.page, source_rect: loc.rect, flag: ReviewFlag::ImpliedClose { text: printed.clone() } });
        }
        let Some(line) = open.lines.last() else { return MoveOutcome::status(MoveStatus::Unchecked) };
        // The position an illegal or ambiguous move was tried in.
        let mut tried = None;
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
                    if open.lines.len() == 1 {
                        // In a book that marks its main line by weight, the bold line is the main
                        // line: a bold move played on it (after its last move, or as an
                        // alternative to one of its moves) puts its own line first. One that could
                        // only be read inside a line of analysis does not, and a book that sets
                        // everything in one weight says nothing: its first line stays the main one.
                        let parent = game.score.nodes.get(node).and_then(|n| n.parent);
                        if bold && open.regular_seen && game.score.leads_to(parent, open.bold_tip) {
                            game.score.put_line_first(node);
                        }
                        if bold {
                            open.bold_tip = Some(node);
                            open.bold_failed = false;
                        }
                        open.strayed = !bold;
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
                        tried: None,
                    };
                }
                Err(PlayError::Illegal(near)) => {
                    tried = game.score.board_after(at).map(|b| b.to_string());
                    (MoveStatus::Illegal, Some(ReviewFlag::IllegalMove { text: printed.clone(), near }))
                }
                Err(PlayError::Ambiguous(candidates)) => {
                    tried = game.score.board_after(at).map(|b| b.to_string());
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
        if open.lines.len() == 1 {
            open.strayed |= !bold;
            open.bold_failed |= bold;
        }
        comment(game, open, &printed);
        MoveOutcome { tried, ..MoveOutcome::status(status) }
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
        let glyph = pdfcraft_chess::nag::shown(code);
        comment(game, open, &glyph);
    }

    /// Prose, punctuation and brackets between moves.
    pub fn text(&mut self, text: &str) {
        // Words between a diagram and the move that settles it: a "position after" cue names the
        // move that led to the diagram.
        if matches!(self.anchor, Anchor::Seed(_)) && !text.trim().is_empty() {
            self.caption = after_cue(text);
        }
        // Captions and prose between a diagram and the move that settles it belong to neither.
        if self.held {
            return;
        }
        let mut start = 0usize;
        // The end of a label in brackets (`(a)`) read whole.
        let mut skip_to = 0usize;
        for (i, ch) in text.char_indices() {
            if self.open.is_none() {
                return;
            }
            if i < skip_to {
                continue;
            }
            let after = i.saturating_add(ch.len_utf8());
            match ch {
                '(' | '[' => {
                    if self.prose_until(text, start, i) {
                        return;
                    }
                    // A label in brackets (`(a)`, `(b)`) opens no variation.
                    if ch == '('
                        && self.rules.separators
                        && let Some(rest) = text.get(after..)
                        && let [Some('a'..='h'), Some(')'), next] = [rest.chars().next(), rest.chars().nth(1), rest.chars().nth(2)]
                        && next.is_none_or(char::is_whitespace)
                    {
                        // `(`, a letter `a`-`h` and `)` are one byte each.
                        skip_to = after.saturating_add(2);
                        if self.prose_until(text, i, skip_to) {
                            return;
                        }
                        self.prose_seen = false;
                        self.mark_separator(Separator::Strong);
                        start = skip_to;
                        continue;
                    }
                    // A result in brackets ends the game as a bare one does (`(1-0)`).
                    if self.rules.results
                        && self.main_level()
                        && let Some((result, len)) = text.get(after..).and_then(bracketed_result)
                    {
                        self.end(result, text.get(after.saturating_add(len)..).unwrap_or_default());
                        return;
                    }
                    self.open_variation();
                    (self.prose_seen, self.ellipsis, self.separator) = (false, false, Separator::None);
                    start = after;
                }
                ')' | ']' => {
                    // A label (`a)`, `b)`) is not a bracket: it separates sibling variations.
                    let label = ch == ')'
                        && self.rules.separators
                        && is_label(text.get(start..i).unwrap_or_default())
                        && text.get(after..).is_some_and(|t| t.trim().is_empty());
                    if label {
                        if self.prose_until(text, start, after) {
                            return;
                        }
                        self.prose_seen = false;
                        self.mark_separator(Separator::Strong);
                    } else {
                        if self.prose_until(text, start, i) {
                            return;
                        }
                        self.close_variation();
                        (self.prose_seen, self.ellipsis, self.separator) = (false, false, Separator::None);
                    }
                    start = after;
                }
                ';' if self.rules.separators && !self.main_level() => {
                    if self.prose_until(text, start, after) {
                        return;
                    }
                    self.prose_seen = false;
                    self.mark_separator(Separator::Weak);
                    start = after;
                }
                _ => {}
            }
        }
        self.prose_until(text, start, text.len());
    }

    /// Whether the reading is on the game's main level (no variation open).
    fn main_level(&self) -> bool {
        self.open.as_ref().is_some_and(|o| o.lines.len() == 1)
    }

    /// Remember a separator, when the reading is in a variation that has had a move.
    fn mark_separator(&mut self, kind: Separator) {
        if self.open.as_ref().is_some_and(|o| o.lines.len() > 1 && o.lines.last().is_some_and(|l| l.moved || l.skipped > 0)) {
            self.separator = kind;
        }
    }

    /// The prose `text[start..end]`. Returns whether a result in it ended the game; in fragment
    /// mode the text after the result is kept ([`Walker::take_rest`]).
    fn prose_until(&mut self, text: &str, start: usize, end: usize) -> bool {
        match self.prose(text.get(start..end).unwrap_or_default()) {
            Some(off) => {
                if self.fragment {
                    self.rest = Some(text.get(start.saturating_add(off)..).unwrap_or_default().to_owned());
                }
                true
            }
            None => false,
        }
    }

    /// The game ends with `result`; in fragment mode, `rest` is the text after it.
    fn end(&mut self, result: GameResult, rest: &str) {
        if let Some(game) = self.open.as_ref().and_then(|o| self.games.get_mut(o.game)) {
            game.score.result = result;
        }
        self.close();
        self.anchor = Anchor::None;
        if self.fragment {
            self.rest = Some(rest.to_owned());
        }
    }

    /// Prose; returns where in it a result that ended the game stops (a byte offset), if one did.
    fn prose(&mut self, text: &str) -> Option<usize> {
        if text.trim().is_empty() {
            return None;
        }
        // Words, not numbers or signs: `64.` before a move is no prose.
        if text.chars().any(char::is_alphabetic) {
            (self.prose_seen, self.ellipsis) = (true, false);
        } else if text.trim_end().ends_with('…') || text.trim_end().ends_with("...") {
            self.ellipsis = true;
        }
        let extended = self.rules.results;
        let open = self.open.as_mut()?;
        let game = self.games.get_mut(open.game)?;
        if open.lines.len() == 1 {
            let mut before = String::new();
            let mut pos = 0usize;
            for word in text.split_whitespace() {
                // Words are found in order, so each is in the text after the one before.
                let at = text.get(pos..).and_then(|t| t.find(word)).map_or(pos, |k| pos.saturating_add(k));
                pos = at.saturating_add(word.len());
                if let Some(result) = result_word(word, extended) {
                    comment(game, open, &before);
                    game.score.result = result;
                    self.close();
                    self.anchor = Anchor::None;
                    return Some(pos);
                }
                before.push_str(word);
                before.push(' ');
            }
        }
        comment(game, open, text);
        None
    }

    fn open_variation(&mut self) {
        let Some(open) = self.open.as_mut() else { return };
        if open.lines.len() > MAX_VARIATION_DEPTH {
            open.overflow = open.overflow.saturating_add(1);
            return;
        }
        let Some(game) = self.games.get(open.game) else { return };
        let Some(parent) = open.lines.last() else { return };
        let line = variation_line(&game.score, parent);
        open.lines.push(line);
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
