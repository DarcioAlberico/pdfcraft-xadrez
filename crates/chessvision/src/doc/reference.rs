//! A book's own PGN as a reference for its text (plan "Carregar PGN do livro", R1).
//!
//! Some books come with their games as a PGN (a Chessable course, a Forward Chess book). The
//! reader still reads the book first ([`super::games`]); [`align`] then runs over the whole
//! [`ChessDoc`] and links each move of the text to a move of the PGN ([`LinkKind`]):
//!
//! - **Confirmed:** the reader played it and its position is one of the PGN's, or it names the
//!   position of the diagram just before it (a "Position after" caption).
//! - **Recovered:** the reader could not play it and the PGN could.
//! - **Conflict:** the reader played it to a position the PGN does not have, and the PGN plays
//!   the same move from a position just before it.
//! - **Absent:** the PGN does not have it.
//!
//! [`align`] changes nothing in the document: the alignment is a list of links beside it, and a
//! book with no PGN is never aligned. [`link`] (R2) keeps the links on the moves
//! ([`MoveInline::reference`]) and marks the moves the PGN recovered for review, the reader's own
//! status kept; it is the one function the tool and the app's reading use.
//!
//! **How a move is found.** Positions anchor the walk, as the reader reads: the diagrams, the
//! moves linked so far and the initial position. A position is its pieces and the side to move;
//! castling rights are left out, because a diagram does not show them. For each move of the text,
//! the moves the PGN plays from the last [`RECENT_ANCHORS`] anchors are searched for its SAN (check
//! marks and annotation glyphs aside), on the side its printed number says. Among those that fit,
//! the one whose printed number matches wins, then one in the game the page discusses, then the
//! most recent anchor's. One anchor gives one position at most, so two positions never tie; when
//! several games pass through the position found (a transposition), the move of the anchor's own
//! game is taken, else the first in the PGN.
//!
//! **Which game a page discusses** is voted by the positions of the page that only one game of the
//! PGN has. The first vote counts the reader's own positions (diagrams and valid moves); each
//! alignment is voted again and run again until the map stops changing, at most [`MAX_ROUNDS`].
//!
//! Moves in headings are not aligned: a heading names a line rather than plays it.

use std::collections::{BTreeMap, HashMap};

use pdfcraft_chess::game::{Game as PgnGame, split_pgn};
use serde::{Deserialize, Serialize};

use super::games::DiagramAt;
use super::{Block, ChessDoc, Inline, MoveInline, MoveStatus, ReviewFlag};

/// Largest PGN text [`ReferenceIndex::from_pgn`] reads.
pub const MAX_REFERENCE_BYTES: usize = 32 << 20;

/// Most games a reference keeps. Courses run past the Board's own 256.
pub const MAX_REFERENCE_GAMES: usize = 4096;

/// Most moves a reference keeps, all games together.
pub const MAX_REFERENCE_NODES: usize = 1_000_000;

/// How many recent positions a move is searched from.
pub const RECENT_ANCHORS: usize = 60;

/// Most alignment runs, each with the page map the one before voted.
pub const MAX_ROUNDS: usize = 4;

/// How many pages, from the first read, [`link`] judges the PGN by.
pub const JUDGED_PAGES: usize = 20;

/// The share of those pages' moves, in percent, below which the PGN does not look like the book's.
pub const MIN_LINKED_PERCENT: usize = 30;

/// The initial position, as a [`ReferenceIndex`] keys it.
const START_KEY: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w";

/// Why a PGN could not be used as a reference.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ReferenceError {
    #[error("the PGN is {bytes} bytes; at most {max} are read")]
    TooLarge { bytes: usize, max: usize },
    #[error("the PGN has no moves")]
    NoMoves,
}

/// A move of the reference: its game, and its node in that game (0 is the game's start).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RefNode {
    pub game: usize,
    pub node: usize,
}

/// One game of the reference, as read.
#[derive(Clone, Debug)]
pub struct ReferenceGame {
    /// Its Event, Site, White and Black tags, those that say something, joined.
    pub name: String,
    /// The game, `None` when it could not be read at all ([`ReferenceGame::error`]).
    pub game: Option<PgnGame>,
    /// Its moves, all lines.
    pub moves: usize,
    /// The first move that could not be played: its line ends there.
    pub stopped: Option<String>,
    /// Why the game could not be read.
    pub error: Option<String>,
}

/// A position of the reference and the moves of the reference that reach it.
#[derive(Clone, Debug, Default)]
struct Position {
    /// The pieces and the side to move: the first two fields of its FEN.
    key: String,
    black: bool,
    /// The nodes standing on it ([`ReferenceIndex::nodes`]).
    nodes: Vec<usize>,
    /// The moves played from it, in every game.
    children: Vec<usize>,
    /// The one game it is in; `None` when several are.
    game: Option<usize>,
}

/// A node of the reference.
#[derive(Clone, Debug)]
struct IndexNode {
    at: RefNode,
    /// The position after it ([`ReferenceIndex::positions`]).
    position: usize,
    /// The node before it, `None` for a game's start.
    parent: Option<usize>,
    /// Its SAN without check marks, empty for a game's start.
    san: String,
    /// The number of the move.
    number: u32,
}

/// A book's PGN, indexed by position.
#[derive(Clone, Debug, Default)]
pub struct ReferenceIndex {
    games: Vec<ReferenceGame>,
    nodes: Vec<IndexNode>,
    positions: Vec<Position>,
    by_key: HashMap<String, usize>,
    /// The positions with these pieces, either side to move.
    by_placement: HashMap<String, Vec<usize>>,
    start: Option<usize>,
    truncated: bool,
}

impl ReferenceIndex {
    /// The games of a PGN file and their positions. A game that cannot be read is kept, with
    /// its error and no moves; past [`MAX_REFERENCE_GAMES`] games or [`MAX_REFERENCE_NODES`]
    /// moves, the rest is left out and [`ReferenceIndex::truncated`] says so.
    pub fn from_pgn(text: &str) -> Result<ReferenceIndex, ReferenceError> {
        if text.len() > MAX_REFERENCE_BYTES {
            return Err(ReferenceError::TooLarge { bytes: text.len(), max: MAX_REFERENCE_BYTES });
        }
        let chunks = split_pgn(text, MAX_REFERENCE_GAMES);
        let mut index = ReferenceIndex { truncated: chunks.len() > MAX_REFERENCE_GAMES, ..ReferenceIndex::default() };
        for chunk in chunks.into_iter().take(MAX_REFERENCE_GAMES) {
            let number = index.games.len();
            match PgnGame::from_pgn_report(chunk) {
                Ok((game, report)) => {
                    let moves = index.add_game(number, &game);
                    index.games.push(ReferenceGame { name: game_name(&game), game: Some(game), moves, stopped: report.stopped, error: None });
                }
                Err(e) => index.games.push(ReferenceGame { name: String::new(), game: None, moves: 0, stopped: None, error: Some(e.to_string()) }),
            }
        }
        if index.games.iter().all(|g| g.moves == 0) {
            return Err(ReferenceError::NoMoves);
        }
        index.start = index.by_key.get(START_KEY).copied();
        Ok(index)
    }

    /// Index game `game_at`'s nodes, parents before children; returns how many moves went in.
    fn add_game(&mut self, game_at: usize, game: &PgnGame) -> usize {
        let mut moves = 0;
        // (node in the game, its index node's parent, the full-move number before it)
        let mut stack = vec![(0usize, None::<usize>, 0u32)];
        while let Some((node, parent, number)) = stack.pop() {
            if self.nodes.len() >= MAX_REFERENCE_NODES {
                self.truncated = true;
                break;
            }
            let Some(fen) = game.fen_at(node) else { continue };
            let Some(key) = position_key(&fen) else { continue };
            let fullmove = fen.split_whitespace().nth(5).and_then(|n| n.parse().ok()).unwrap_or(0);
            let position = self.position_of(key, game_at);
            let id = self.nodes.len();
            self.nodes.push(IndexNode { at: RefNode { game: game_at, node }, position, parent, san: plain_san(game.san(node)), number });
            if let Some(p) = self.positions.get_mut(position) {
                p.nodes.push(id);
            }
            if let Some(parent) = parent {
                moves += 1;
                let from = self.nodes.get(parent).map(|p| p.position);
                if let Some(from) = from.and_then(|f| self.positions.get_mut(f)) {
                    from.children.push(id);
                }
            }
            // Reversed, so the first reply (the line) is indexed first.
            for &reply in game.replies(node).iter().rev() {
                stack.push((reply, Some(id), fullmove));
            }
        }
        moves
    }

    /// The position `key`, added when new; game `game` stands on it.
    fn position_of(&mut self, key: String, game: usize) -> usize {
        if let Some(&id) = self.by_key.get(&key) {
            if let Some(p) = self.positions.get_mut(id).filter(|p| p.game != Some(game)) {
                p.game = None;
            }
            return id;
        }
        let id = self.positions.len();
        let placement = key.split(' ').next().unwrap_or_default().to_string();
        self.by_placement.entry(placement).or_default().push(id);
        self.by_key.insert(key.clone(), id);
        let black = key.ends_with(" b");
        self.positions.push(Position { key, black, nodes: Vec::new(), children: Vec::new(), game: Some(game) });
        id
    }

    /// The games, in the order of the PGN.
    pub fn games(&self) -> &[ReferenceGame] {
        &self.games
    }

    /// Moves indexed, all games together.
    pub fn moves(&self) -> usize {
        self.games.iter().map(|g| g.moves).sum()
    }

    /// Different positions indexed (pieces and side to move).
    pub fn positions(&self) -> usize {
        self.positions.len()
    }

    /// Games or moves were left out ([`MAX_REFERENCE_GAMES`], [`MAX_REFERENCE_NODES`]).
    pub fn truncated(&self) -> bool {
        self.truncated
    }

    /// The FEN after move `at`.
    pub fn fen(&self, at: RefNode) -> Option<String> {
        self.games.get(at.game)?.game.as_ref()?.fen_at(at.node)
    }

    fn node(&self, id: usize) -> Option<&IndexNode> {
        self.nodes.get(id)
    }

    fn position(&self, id: usize) -> Option<&Position> {
        self.positions.get(id)
    }

    fn placement(&self, position: usize) -> Option<&str> {
        self.position(position).and_then(|p| p.key.split(' ').next())
    }
}

/// The name of a game: its Event, Site, White and Black tags that say something.
fn game_name(game: &PgnGame) -> String {
    ["Event", "Site", "White", "Black"]
        .iter()
        .filter_map(|tag| game.tag(tag))
        .map(str::trim)
        .filter(|v| !v.is_empty() && !v.chars().all(|c| c == '?'))
        .collect::<Vec<_>>()
        .join(" · ")
}

/// The pieces and the side to move of a FEN: its first two fields.
fn position_key(fen: &str) -> Option<String> {
    let mut fields = fen.split_whitespace();
    let placement = fields.next()?;
    let side = match fields.next() {
        Some("b") => "b",
        _ => "w",
    };
    Some(format!("{placement} {side}"))
}

/// A SAN without check marks and annotation glyphs, castling with letters.
fn plain_san(san: &str) -> String {
    let san = san.trim().trim_end_matches(['+', '#', '!', '?']);
    match san {
        "0-0" | "o-o" => "O-O".to_string(),
        "0-0-0" | "o-o-o" => "O-O-O".to_string(),
        _ => san.to_string(),
    }
}

/// How a move of the text stands against the reference.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkKind {
    /// The reader played it and the reference has its position, or it names the diagram before it.
    Confirmed,
    /// The reader could not play it; the reference did.
    Recovered,
    /// The reader played it to a position the reference does not have; the reference plays it
    /// from a position just before.
    Conflict,
    /// The reference does not have it.
    Absent,
}

/// An inline of the document: its page index, block and place in the block, from 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InlineAt {
    pub page: u32,
    pub block: usize,
    pub inline: usize,
}

/// A move of the text and its move in the reference.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoveLink {
    pub at: InlineAt,
    pub kind: LinkKind,
    /// `None` when [`LinkKind::Absent`].
    pub node: Option<RefNode>,
}

/// A diagram of the document and the move of the reference whose position it shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagramLink {
    pub at: DiagramAt,
    /// `None` when no move of the reference could be told to be the one.
    pub node: Option<RefNode>,
}

/// The game of the reference a page discusses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageGame {
    pub page: u32,
    pub game: usize,
}

/// How many moves of the text came out of each kind.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkCounts {
    pub confirmed: usize,
    pub recovered: usize,
    pub conflict: usize,
    pub absent: usize,
}

/// A document aligned with its reference.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Alignment {
    /// Every move of the document's paragraphs, in reading order.
    pub moves: Vec<MoveLink>,
    /// Every diagram that was read, in reading order.
    pub diagrams: Vec<DiagramLink>,
    /// The pages whose game the vote settled, in page order.
    pub pages: Vec<PageGame>,
    pub counts: LinkCounts,
    /// Alignment runs made (at most [`MAX_ROUNDS`]).
    pub rounds: usize,
}

/// A move of the text and where the reference has it, kept on the move
/// ([`MoveInline::reference`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoveReference {
    pub kind: LinkKind,
    /// The game of the reference, from 0; `None` when [`LinkKind::Absent`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game: Option<usize>,
    /// The node in that game (0 is its start).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<usize>,
}

/// A document linked to its reference ([`link`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Linked {
    pub alignment: Alignment,
    /// What a person should know about the PGN: that it does not look like the book's, or that
    /// it was cut short.
    pub warnings: Vec<String>,
}

/// Align `doc` with `index` ([`align`]) and keep the links on its moves: each move of a paragraph
/// gets its [`MoveInline::reference`], and a recovered move keeps the reader's status and gains
/// [`ReviewFlag::FromReference`] with the reference's move. The links of an earlier call are
/// replaced, so linking twice is linking once.
pub fn link(doc: &mut ChessDoc, index: &ReferenceIndex) -> Linked {
    for page in &mut doc.pages {
        for block in &mut page.blocks {
            if let Block::Paragraph { inlines, .. } | Block::Heading { inlines, .. } = block {
                for inline in inlines {
                    if let Inline::Move(m) = inline {
                        m.reference = None;
                        m.review.retain(|f| !matches!(f, ReviewFlag::FromReference { .. }));
                    }
                }
            }
        }
    }
    let alignment = align(doc, index);
    let at_page: HashMap<u32, usize> = doc.pages.iter().enumerate().map(|(k, p)| (p.index, k)).collect();
    for l in &alignment.moves {
        let page = at_page.get(&l.at.page).and_then(|&k| doc.pages.get_mut(k));
        let Some(Block::Paragraph { inlines, .. }) = page.and_then(|p| p.blocks.get_mut(l.at.block)) else { continue };
        let Some(Inline::Move(m)) = inlines.get_mut(l.at.inline) else { continue };
        m.reference = Some(MoveReference { kind: l.kind, game: l.node.map(|n| n.game), node: l.node.map(|n| n.node) });
        if l.kind == LinkKind::Recovered {
            let san = l.node.and_then(|n| {
                let game = index.games.get(n.game)?.game.as_ref()?;
                game.fen_at(n.node).map(|_| game.san(n.node).to_string())
            });
            if let Some(san) = san {
                m.review.push(ReviewFlag::FromReference { san });
            }
        }
    }
    let warnings = warnings(doc, index, &alignment);
    Linked { alignment, warnings }
}

/// The warnings of [`Linked`]: few of the moves on the first [`JUDGED_PAGES`] pages read linked,
/// and a PGN past the caps.
fn warnings(doc: &ChessDoc, index: &ReferenceIndex, alignment: &Alignment) -> Vec<String> {
    let mut out = Vec::new();
    let judged: Vec<u32> = doc.pages.iter().map(|p| p.index).take(JUDGED_PAGES).collect();
    let on_them = alignment.moves.iter().filter(|l| judged.contains(&l.at.page));
    let (linked, total) = on_them
        .fold((0usize, 0usize), |(linked, total), l| (linked.saturating_add(usize::from(l.kind != LinkKind::Absent)), total.saturating_add(1)));
    if total > 0 && linked.saturating_mul(100) < total.saturating_mul(MIN_LINKED_PERCENT) {
        out.push(format!("the PGN does not look like this book's: {linked} of the {total} moves on the first {} pages read are in it", judged.len()));
    }
    if index.truncated() {
        out.push(format!("the PGN is longer than {MAX_REFERENCE_GAMES} games or {MAX_REFERENCE_NODES} moves; the rest of it was left out"));
    }
    out
}

/// Something read in order.
enum Token<'a> {
    Diagram { at: DiagramAt, placement: &'a str },
    Move { at: InlineAt, mv: &'a MoveInline },
}

/// The document's diagrams and paragraph moves, in reading order.
fn tokens(doc: &ChessDoc) -> Vec<Token<'_>> {
    let mut out = Vec::new();
    for page in &doc.pages {
        for (b, block) in page.blocks.iter().enumerate() {
            match block {
                Block::Diagram(d) => {
                    if let Some(placement) = d.reading.as_ref().and_then(|r| r.fen.split_whitespace().next()) {
                        out.push(Token::Diagram { at: DiagramAt { page: page.index, diagram: d.number }, placement });
                    }
                }
                Block::Paragraph { inlines, .. } => {
                    for (i, inline) in inlines.iter().enumerate() {
                        if let Inline::Move(mv) = inline {
                            out.push(Token::Move { at: InlineAt { page: page.index, block: b, inline: i }, mv });
                        }
                    }
                }
                Block::Heading { .. } => {}
            }
        }
    }
    out
}

/// Link `doc`'s moves and diagrams to `index` (see the module documentation).
pub fn align(doc: &ChessDoc, index: &ReferenceIndex) -> Alignment {
    let tokens = tokens(doc);
    let mut map = first_vote(index, &tokens);
    let mut rounds = 0;
    loop {
        rounds += 1;
        let pass = Pass::run(index, &tokens, &map);
        let voters = pass.moves.iter().map(|(at, _, n)| (at.page, *n)).chain(pass.diagrams.iter().map(|(at, n)| (at.page, *n)));
        let next = vote(index, voters.filter_map(|(page, n)| Some((page, n?))));
        if next == map || rounds >= MAX_ROUNDS {
            let node_at = |n: Option<usize>| n.and_then(|n| index.node(n)).map(|n| n.at);
            let mut counts = LinkCounts::default();
            for (_, kind, _) in &pass.moves {
                match kind {
                    LinkKind::Confirmed => counts.confirmed += 1,
                    LinkKind::Recovered => counts.recovered += 1,
                    LinkKind::Conflict => counts.conflict += 1,
                    LinkKind::Absent => counts.absent += 1,
                }
            }
            let moves = pass.moves.iter().map(|&(at, kind, n)| MoveLink { at, kind, node: node_at(n) }).collect();
            let diagrams = pass.diagrams.iter().map(|&(at, n)| DiagramLink { at, node: node_at(n) }).collect();
            let pages = map.iter().map(|(&page, &game)| PageGame { page, game }).collect();
            return Alignment { moves, diagrams, pages, counts, rounds };
        }
        map = next;
    }
}

/// The first page map: the reader's own diagrams and valid moves.
fn first_vote(index: &ReferenceIndex, tokens: &[Token]) -> BTreeMap<u32, usize> {
    let mut votes: BTreeMap<u32, BTreeMap<usize, usize>> = BTreeMap::new();
    for token in tokens {
        let (page, positions): (u32, Vec<usize>) = match token {
            Token::Diagram { at, placement } => (at.page, index.by_placement.get(*placement).cloned().unwrap_or_default()),
            Token::Move { at, mv } if mv.status == MoveStatus::Valid => {
                (at.page, mv.fen.as_deref().and_then(position_key).and_then(|k| index.by_key.get(&k).copied()).into_iter().collect())
            }
            Token::Move { .. } => continue,
        };
        // A diagram's two sides count once: only when both are the one game's.
        let games: Vec<Option<usize>> = positions.iter().map(|&p| index.position(p).and_then(|p| p.game)).collect();
        if let Some(Some(game)) = games.first().copied().filter(|g| games.iter().all(|x| x == g)) {
            *votes.entry(page).or_default().entry(game).or_default() += 1;
        }
    }
    winners(votes)
}

/// A page map from links: each page votes for the games of its positions that only one game has.
fn vote(index: &ReferenceIndex, links: impl Iterator<Item = (u32, usize)>) -> BTreeMap<u32, usize> {
    let mut votes: BTreeMap<u32, BTreeMap<usize, usize>> = BTreeMap::new();
    for (page, id) in links {
        if let Some(game) = index.node(id).and_then(|n| index.position(n.position)).and_then(|p| p.game) {
            *votes.entry(page).or_default().entry(game).or_default() += 1;
        }
    }
    winners(votes)
}

/// Each page's game with the most votes; a page whose top two tie has none.
fn winners(votes: BTreeMap<u32, BTreeMap<usize, usize>>) -> BTreeMap<u32, usize> {
    let mut map = BTreeMap::new();
    for (page, games) in votes {
        let mut best: Option<(usize, usize)> = None;
        let mut tied = false;
        for (game, n) in games {
            match best {
                Some((_, m)) if n == m => tied = true,
                Some((_, m)) if n < m => {}
                _ => {
                    best = Some((game, n));
                    tied = false;
                }
            }
        }
        if let (Some((game, _)), false) = (best, tied) {
            map.insert(page, game);
        }
    }
    map
}

/// A position the walk passed through, and the node it came by when one did.
#[derive(Clone, Copy, Debug)]
struct Anchor {
    position: usize,
    node: Option<usize>,
}

/// One run of the alignment over the document; nodes as [`ReferenceIndex::nodes`] indexes them.
struct Pass {
    moves: Vec<(InlineAt, LinkKind, Option<usize>)>,
    diagrams: Vec<(DiagramAt, Option<usize>)>,
}

/// A diagram waiting to learn its move: by a caption naming it, or by the move played from it.
struct Pending {
    link: usize,
    placement: String,
}

impl Pass {
    fn run(index: &ReferenceIndex, tokens: &[Token], map: &BTreeMap<u32, usize>) -> Pass {
        let mut pass = Pass { moves: Vec::new(), diagrams: Vec::new() };
        let mut recent: Vec<Anchor> = Vec::new();
        let mut pending: Option<Pending> = None;
        // The node linked last, for a diagram that reprints the position just reached.
        let mut last: Option<usize> = None;
        // The pieces of the diagram just read, until the move after it: a caption may name it.
        let mut caption: Option<&str> = None;
        for token in tokens {
            match token {
                Token::Diagram { at, placement } => {
                    pass.settle(index, pending.take(), map);
                    let link = pass.diagrams.len();
                    let reached = last.filter(|&n| index.node(n).is_some_and(|n| index.placement(n.position) == Some(*placement)));
                    pass.diagrams.push((*at, reached));
                    caption = Some(*placement);
                    if reached.is_none() {
                        pending = Some(Pending { link, placement: (*placement).to_string() });
                    }
                    for &p in index.by_placement.get(*placement).map(Vec::as_slice).unwrap_or_default() {
                        push(&mut recent, Anchor { position: p, node: None });
                    }
                }
                Token::Move { at, mv } => {
                    let page_game = map.get(&at.page).copied();
                    let (kind, node) = link_move(index, mv, &recent, page_game, caption.take());
                    pass.moves.push((*at, kind, node));
                    let Some(node) = node.and_then(|id| index.node(id).map(|n| (id, n))) else { continue };
                    if let Some(p) = pending.take() {
                        // A caption names the diagram's own position; a move played from it says the
                        // diagram is the position before it.
                        let shows = |id: usize| index.node(id).is_some_and(|n| index.placement(n.position) == Some(p.placement.as_str()));
                        let shown = if shows(node.0) { Some(node.0) } else { node.1.parent.filter(|&q| shows(q)) };
                        match (shown, pass.diagrams.get_mut(p.link)) {
                            (Some(id), Some(d)) => d.1 = Some(id),
                            _ => pass.settle(index, Some(p), map),
                        }
                    }
                    push(&mut recent, Anchor { position: node.1.position, node: Some(node.0) });
                    last = Some(node.0);
                }
            }
        }
        pass.settle(index, pending, map);
        pass
    }

    /// A diagram no move told about: the node of the page's game with its pieces, else the first
    /// node with them when they are all one game's.
    fn settle(&mut self, index: &ReferenceIndex, pending: Option<Pending>, map: &BTreeMap<u32, usize>) {
        let Some(p) = pending else { return };
        let Some(link) = self.diagrams.get_mut(p.link) else { return };
        let nodes: Vec<(usize, usize)> = index
            .by_placement
            .get(&p.placement)
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter_map(|&pos| index.position(pos))
            .flat_map(|pos| pos.nodes.iter().filter_map(|&n| index.node(n).map(|node| (n, node.at.game))))
            .collect();
        let page_game = map.get(&link.0.page).copied();
        link.1 = match nodes.iter().find(|(_, game)| Some(*game) == page_game) {
            Some(&(n, _)) => Some(n),
            None => nodes.first().filter(|(_, first)| nodes.iter().all(|(_, g)| g == first)).map(|&(n, _)| n),
        };
    }
}

/// Put `anchor` first, dropping an older anchor on the same position, and keep at most
/// [`RECENT_ANCHORS`].
fn push(recent: &mut Vec<Anchor>, anchor: Anchor) {
    recent.retain(|a| a.position != anchor.position);
    recent.insert(0, anchor);
    recent.truncate(RECENT_ANCHORS);
}

/// The kind and the node (in [`ReferenceIndex::nodes`]) of one move of the text.
fn link_move(
    index: &ReferenceIndex,
    mv: &MoveInline,
    recent: &[Anchor],
    page_game: Option<usize>,
    caption: Option<&str>,
) -> (LinkKind, Option<usize>) {
    if mv.status == MoveStatus::Valid {
        let at = mv.fen.as_deref().and_then(position_key).and_then(|k| index.by_key.get(&k).copied());
        if let Some(position) = at {
            return (LinkKind::Confirmed, reached(index, position, recent, page_game));
        }
        return match search(index, mv, recent, page_game) {
            Some(node) => (LinkKind::Conflict, Some(node)),
            None => (LinkKind::Absent, None),
        };
    }
    match search(index, mv, recent, page_game) {
        Some(node) => {
            let names_diagram = caption.is_some_and(|c| index.node(node).and_then(|n| index.placement(n.position)) == Some(c));
            (if names_diagram { LinkKind::Confirmed } else { LinkKind::Recovered }, Some(node))
        }
        None => (LinkKind::Absent, None),
    }
}

/// The node by which the reader reached `position`: one played from the most recent anchor it
/// can be played from, else one of the page's game, else the first.
fn reached(index: &ReferenceIndex, position: usize, recent: &[Anchor], page_game: Option<usize>) -> Option<usize> {
    let nodes = &index.position(position)?.nodes;
    nodes.iter().copied().min_by_key(|&n| {
        let node = index.node(n);
        let parent = node.and_then(|n| n.parent);
        let from = parent.and_then(|p| index.node(p)).map(|p| p.position);
        let r = from.and_then(|f| recent.iter().position(|a| a.position == f)).unwrap_or(usize::MAX);
        let by_anchor = parent.is_some_and(|p| recent.get(r).is_some_and(|a| a.node == Some(p)));
        (r, !by_anchor, node.map(|n| n.at.game) != page_game, n)
    })
}

/// How a candidate of [`search`] ranks, least first: its number differs from the printed one, it
/// is not in the page's game, its anchor's recency, it is not a reply to the anchor's own node,
/// its index.
type Rank = (bool, bool, usize, bool, usize);

/// The move of the reference that the text's `mv` is, played from a recent anchor or the initial
/// position (see the module documentation for the order).
fn search(index: &ReferenceIndex, mv: &MoveInline, recent: &[Anchor], page_game: Option<usize>) -> Option<usize> {
    let san = plain_san(&mv.san);
    if san.is_empty() {
        return None;
    }
    let start = index.start.map(|position| Anchor { position, node: None });
    let mut best: Option<(Rank, usize)> = None;
    for (r, anchor) in recent.iter().copied().chain(start).enumerate() {
        let Some(from) = index.position(anchor.position) else { continue };
        if mv.number.is_some_and(|n| n.black != from.black) {
            continue;
        }
        for &c in &from.children {
            let Some(node) = index.node(c) else { continue };
            if node.san != san {
                continue;
            }
            let rank = (
                mv.number.is_some_and(|n| u32::from(n.number) != node.number),
                node.at.game != page_game.unwrap_or(usize::MAX),
                r,
                node.parent != anchor.node,
                c,
            );
            if best.as_ref().is_none_or(|(b, _)| rank < *b) {
                best = Some((rank, c));
            }
        }
    }
    best.map(|(_, c)| c)
}

#[cfg(test)]
mod tests;
