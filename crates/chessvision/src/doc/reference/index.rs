//! The book's PGN indexed by position, compact (plan A9-I3b, finding R8-06: a PGN at the caps took
//! 1.18 GB, three strings a position and every game kept with a board a move).
//!
//! **What is kept, a move:** its game and its node there, the position after it, the move before
//! it, its number, its SAN (each SAN once, shared by every move written so) and the move itself in
//! two bytes. A position is its pieces and side to move as a 128-bit digest ([`PositionKey`]) in
//! one map, with the one game it is in; the moves standing on each position and the moves played
//! from it are two tables of ids. No game is kept: the full position after a move
//! ([`ReferenceIndex::fen`]) is played again from its game's start when asked, and a game is read
//! again from its text ([`ReferenceIndex::game`]), which is kept (at most
//! [`super::MAX_REFERENCE_BYTES`] for the whole PGN).
//!
//! **Caps.** At most [`super::MAX_REFERENCE_GAMES`] games, [`super::MAX_REFERENCE_NODES`] moves and
//! [`super::MAX_REFERENCE_MEMORY`] bytes of tables, estimated as the moves go in: past one, the
//! rest is left out, [`ReferenceIndex::cut`] says which, and [`super::link`]'s warnings say so. The
//! games past it are still listed, by the names their tags give, with no moves.
//!
//! **Once.** [`IndexMemo`] keeps the last indexes made, by the SHA-256 of their text, for every
//! thread that asks: the reading tools index a kept PGN once, not at every call, and an index
//! made on a worker is the one the interface finds.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use cozy_chess::{Board, Move, Piece as CozyPiece, Square as CozySquare};
use pdfcraft_chess::fen::{BookKey, Fen, PlacementKey};
use pdfcraft_chess::game::pgn_scan::{ScanKind, scan, tag_parts, tag_value};
use pdfcraft_chess::game::{Game as PgnGame, MAX_NODES, MAX_TAG_LEN, PgnStopReason, split_pgn};
use pdfcraft_chess::placement::Color;
use sha2::Digest as _;

use super::{MAX_REFERENCE_BYTES, MAX_REFERENCE_GAMES, MAX_REFERENCE_MEMORY, MAX_REFERENCE_NODES, RefNode, ReferenceError, plain_san, start_key};

/// No id: a game's start has no move before it and no SAN.
const NONE: u32 = u32::MAX;

/// The step of a game's start (no move led there).
const START_STEP: u16 = u16::MAX;

/// The step of a null move.
const NULL_STEP: u16 = u16::MAX - 1;

/// Most indexes [`IndexMemo`] keeps.
pub const MEMO_INDEXES: usize = 4;

/// A position's pieces and side to move in 128 bits (plan A9-I3: the book's key, [`BookKey`],
/// compacted for a map of a million positions): the SHA-256 of its [`PlacementKey`] cut to 128
/// bits, its lowest bit the side to move. Two placements share a digest with a chance of about
/// 2⁻¹²⁷ a pair; the same pieces with either side to move differ only in that bit, so the one map
/// finds both.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct PositionKey([u64; 2]);

impl PositionKey {
    /// The key of `placement` with White to move: what [`PositionKey::same_pieces`] compares.
    pub(crate) fn pieces(placement: PlacementKey) -> PositionKey {
        let mut sha = sha2::Sha256::new();
        for word in placement.words() {
            sha.update(word.to_le_bytes());
        }
        let digest: [u8; 32] = sha.finalize().into();
        let word = |at: usize| digest.get(at..at.saturating_add(8)).and_then(|b| <[u8; 8]>::try_from(b).ok()).map_or(0, u64::from_le_bytes);
        PositionKey([word(0), word(8) & !1])
    }

    pub(crate) fn of(key: BookKey) -> PositionKey {
        PositionKey::pieces(key.placement).with_side(key.side)
    }

    fn with_side(self, side: Color) -> PositionKey {
        let [high, low] = self.0;
        PositionKey([high, (low & !1) | u64::from(side == Color::Black)])
    }

    fn side(self) -> Color {
        if self.0[1] & 1 == 1 { Color::Black } else { Color::White }
    }

    /// Whether the two positions have the same pieces, whoever is to move.
    pub(crate) fn same_pieces(self, other: PositionKey) -> bool {
        self.with_side(Color::White) == other.with_side(Color::White)
    }
}

/// Why the index left out the rest of a PGN.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cut {
    /// It has more than [`super::MAX_REFERENCE_GAMES`] games.
    Games,
    /// It has more than [`super::MAX_REFERENCE_NODES`] moves.
    Moves,
    /// Its tables would pass the bytes the index was given ([`super::MAX_REFERENCE_MEMORY`]).
    Memory,
}

/// One game of the reference, as read.
#[derive(Clone, Debug)]
pub struct ReferenceGame {
    /// Its Event, Site, White and Black tags, those that say something, joined.
    pub name: String,
    /// Its moves indexed, all lines (none for a game past the caps).
    pub moves: usize,
    /// The first move that could not be played: its line ends there.
    pub stopped: Option<String>,
    /// Why it could not be played (plan A9-F2b).
    pub stop_reason: Option<PgnStopReason>,
    /// Why the game could not be read.
    pub error: Option<String>,
}

/// A game's text and where its moves are in the index.
#[derive(Clone, Debug)]
struct GameText {
    text: Box<str>,
    /// The board it starts from, which its moves are played again on ([`ReferenceIndex::fen`]).
    start: Board,
    /// Its nodes' ids in [`ReferenceIndex::node_of`]: from `first`, `len` of them.
    first: usize,
    len: usize,
}

/// A move of the reference (or a game's start).
#[derive(Clone, Copy, Debug)]
pub(crate) struct IndexNode {
    game: u32,
    node: u32,
    position: u32,
    parent: u32,
    /// Its SAN as written ([`Sans::written`]); [`NONE`] for a game's start.
    san: u32,
    /// The full-move number of the move.
    number: u16,
    /// The move from the node before, as [`step`] writes it.
    step: u16,
}

impl IndexNode {
    pub(crate) fn at(&self) -> RefNode {
        RefNode { game: self.game as usize, node: self.node as usize }
    }

    /// The position after it.
    pub(crate) fn position(&self) -> usize {
        self.position as usize
    }

    /// The node before it, `None` for a game's start.
    pub(crate) fn parent(&self) -> Option<usize> {
        (self.parent != NONE).then_some(self.parent as usize)
    }

    pub(crate) fn number(&self) -> u32 {
        u32::from(self.number)
    }
}

/// A table of ids by row: row `k`'s are `ids[start[k]..start[k + 1]]`, in the order they came.
#[derive(Clone, Debug, Default)]
struct Table {
    start: Vec<u32>,
    ids: Vec<u32>,
}

impl Table {
    /// The ids of `items` (row, id) by row, `rows` rows, each row's in the order given.
    fn of(rows: usize, items: impl Iterator<Item = (usize, u32)> + Clone) -> Table {
        let mut start = vec![0u32; rows.saturating_add(1)];
        for (row, _) in items.clone() {
            if let Some(n) = row.checked_add(1).and_then(|r| start.get_mut(r)) {
                *n = n.saturating_add(1);
            }
        }
        let mut sum = 0u32;
        for n in &mut start {
            sum = sum.saturating_add(*n);
            *n = sum;
        }
        let mut ids = vec![0u32; start.last().map_or(0, |&n| n as usize)];
        let mut next = start.clone();
        for (row, id) in items {
            if let Some(at) = next.get_mut(row) {
                if let Some(slot) = ids.get_mut(*at as usize) {
                    *slot = id;
                }
                *at = at.saturating_add(1);
            }
        }
        Table { start, ids }
    }

    fn row(&self, row: usize) -> &[u32] {
        match (self.start.get(row), row.checked_add(1).and_then(|r| self.start.get(r))) {
            (Some(&a), Some(&b)) => self.ids.get(a as usize..b as usize).unwrap_or_default(),
            _ => &[],
        }
    }

    fn bytes(&self) -> usize {
        self.start.capacity().saturating_add(self.ids.capacity()).saturating_mul(size_of::<u32>())
    }
}

/// The SANs of the reference's moves, each once.
#[derive(Clone, Debug, Default)]
struct Sans {
    /// As the games write them (check marks and all).
    written: Vec<Box<str>>,
    /// Each one's plain SAN ([`plain_san`]), an id of `plain`.
    plain_of: Vec<u32>,
    written_ids: HashMap<Box<str>, u32>,
    plain: HashMap<Box<str>, u32>,
    /// The text they hold, both maps and the list.
    bytes: usize,
}

impl Sans {
    fn intern(&mut self, san: &str) -> u32 {
        if let Some(&id) = self.written_ids.get(san) {
            return id;
        }
        let plain = plain_san(san);
        let next = u32::try_from(self.plain.len()).unwrap_or(NONE);
        let plain_len = plain.len();
        let p = *self.plain.entry(plain.into_boxed_str()).or_insert_with(|| next);
        let id = u32::try_from(self.written.len()).unwrap_or(NONE);
        self.written.push(san.into());
        self.plain_of.push(p);
        self.written_ids.insert(san.into(), id);
        self.bytes = self.bytes.saturating_add(san.len().saturating_mul(2)).saturating_add(plain_len).saturating_add(SAN_BYTES);
        id
    }
}

/// The bytes of a SAN's slots beside its text: the list, the plain id and both maps' entries.
const SAN_BYTES: usize = 3 * size_of::<Box<str>>() + 3 * size_of::<u32>() + 2;

/// The bytes a move takes, estimated while indexing: its node, its slot in its game's map and in
/// the two tables.
const NODE_BYTES: usize = size_of::<IndexNode>() + 3 * size_of::<u32>();

/// The bytes a position takes: its key, its game, its two rows' starts and its entry in the map
/// (an entry and a control byte, with the map at most seven eighths full).
const POSITION_BYTES: usize = size_of::<PositionKey>() + 3 * size_of::<u32>() + MAP_ENTRY_BYTES;

const MAP_ENTRY_BYTES: usize = (size_of::<(PositionKey, u32)>() + 1) * 8 / 7;

/// A book's PGN, indexed by position.
#[derive(Clone, Debug, Default)]
pub struct ReferenceIndex {
    games: Vec<ReferenceGame>,
    texts: Vec<GameText>,
    nodes: Vec<IndexNode>,
    /// Each game's nodes' ids in [`ReferenceIndex::nodes`], by their node in the game
    /// ([`GameText::first`]); [`NONE`] for one left out.
    node_of: Vec<u32>,
    keys: Vec<PositionKey>,
    /// The one game each position is in; [`NONE`] when several are.
    game_of: Vec<u32>,
    /// The nodes standing on each position.
    standing: Table,
    /// The moves played from each position, in every game.
    children: Table,
    by_key: HashMap<PositionKey, u32>,
    sans: Sans,
    /// The bytes the games' names hold, for the estimate.
    names: usize,
    start: Option<usize>,
    /// More games than [`super::MAX_REFERENCE_GAMES`].
    more_games: bool,
    /// Indexing stopped at the moves' or the memory's cap.
    stopped: Option<Cut>,
}

impl ReferenceIndex {
    /// The games of a PGN file and their positions. A game that cannot be read is kept, with
    /// its error and no moves; past [`super::MAX_REFERENCE_GAMES`] games,
    /// [`super::MAX_REFERENCE_NODES`] moves or [`super::MAX_REFERENCE_MEMORY`] bytes, the rest is
    /// left out and [`ReferenceIndex::cut`] says so.
    pub fn from_pgn(text: &str) -> Result<ReferenceIndex, ReferenceError> {
        ReferenceIndex::from_pgn_within(text, MAX_REFERENCE_MEMORY)
    }

    /// [`ReferenceIndex::from_pgn`] with at most `memory` bytes of tables.
    pub fn from_pgn_within(text: &str, memory: usize) -> Result<ReferenceIndex, ReferenceError> {
        if text.len() > MAX_REFERENCE_BYTES {
            return Err(ReferenceError::TooLarge { bytes: text.len(), max: MAX_REFERENCE_BYTES });
        }
        let chunks = split_pgn(text, MAX_REFERENCE_GAMES);
        let mut index = ReferenceIndex { more_games: chunks.len() > MAX_REFERENCE_GAMES, ..ReferenceIndex::default() };
        for chunk in chunks.into_iter().take(MAX_REFERENCE_GAMES) {
            let number = index.games.len();
            let first = index.node_of.len();
            let mut kept = GameText { text: chunk.into(), start: Board::default(), first, len: 0 };
            let game = if index.stopped.is_some() {
                // Past the caps: listed by its name, never read.
                ReferenceGame { name: chunk_name(chunk), moves: 0, stopped: None, stop_reason: None, error: None }
            } else {
                match PgnGame::from_pgn_report(chunk) {
                    Ok((game, report)) => {
                        let moves = index.add_game(number, &game, memory);
                        kept.start = game.start_board().clone();
                        kept.len = index.node_of.len().saturating_sub(first);
                        ReferenceGame { name: game_name(&game), moves, stopped: report.stopped, stop_reason: report.stop_reason, error: None }
                    }
                    Err(e) => ReferenceGame { name: String::new(), moves: 0, stopped: None, stop_reason: None, error: Some(e.to_string()) },
                }
            };
            index.names = index.names.saturating_add(game.name.len());
            index.games.push(game);
            index.texts.push(kept);
        }
        if index.games.iter().all(|g| g.moves == 0) {
            return Err(ReferenceError::NoMoves);
        }
        index.finish();
        Ok(index)
    }

    /// The bytes the tables would take with what is in them now ([`NODE_BYTES`],
    /// [`POSITION_BYTES`]).
    fn estimate(&self) -> usize {
        self.nodes
            .len()
            .saturating_mul(NODE_BYTES)
            .saturating_add(self.keys.len().saturating_mul(POSITION_BYTES))
            .saturating_add(self.sans.bytes)
            .saturating_add(self.names)
            .saturating_add(self.games.len().saturating_mul(size_of::<ReferenceGame>() + size_of::<GameText>()))
    }

    /// Index game `game_at`'s nodes, parents before children, each line before its variations
    /// (the order the alignment breaks its ties by), until a cap; returns how many moves went in.
    fn add_game(&mut self, game_at: usize, game: &PgnGame, memory: usize) -> usize {
        let first = self.node_of.len();
        let game_id = u32::try_from(game_at).unwrap_or(NONE);
        let mut moves = 0;
        // (node in the game, its index node's parent, the full-move number before it)
        let mut stack = vec![(0usize, NONE, 0u16)];
        while let Some((node, parent, number)) = stack.pop() {
            if self.nodes.len() >= MAX_REFERENCE_NODES {
                self.stopped = Some(Cut::Moves);
                break;
            }
            if self.estimate() >= memory {
                self.stopped = Some(Cut::Memory);
                break;
            }
            // Read from the board, no FEN text (plan A9-T1a): this runs for every move indexed.
            let Some(fen) = game.fen_at(node) else { continue };
            let (Ok(id), Ok(in_game), Some(slot)) = (u32::try_from(self.nodes.len()), u32::try_from(node), first.checked_add(node)) else { break };
            let fullmove = u16::try_from(fen.fullmove_number()).unwrap_or(u16::MAX);
            let position = self.position_of(PositionKey::of(fen.book_key()), game_id);
            let (san, step) = if parent == NONE { (NONE, START_STEP) } else { (self.sans.intern(game.san(node)), step(game.step_at(node))) };
            self.nodes.push(IndexNode { game: game_id, node: in_game, position, parent, san, number, step });
            if self.node_of.len() <= slot {
                self.node_of.resize(slot.saturating_add(1), NONE);
            }
            if let Some(s) = self.node_of.get_mut(slot) {
                *s = id;
            }
            if parent != NONE {
                moves += 1;
            }
            // Reversed, so the first reply (the line) is indexed first.
            for &reply in game.replies(node).iter().rev() {
                stack.push((reply, id, fullmove));
            }
        }
        moves
    }

    /// The position `key`, added when new; game `game` stands on it.
    fn position_of(&mut self, key: PositionKey, game: u32) -> u32 {
        if let Some(&id) = self.by_key.get(&key) {
            if let Some(g) = self.game_of.get_mut(id as usize).filter(|g| **g != game) {
                *g = NONE;
            }
            return id;
        }
        let id = u32::try_from(self.keys.len()).unwrap_or(NONE);
        self.by_key.insert(key, id);
        self.keys.push(key);
        self.game_of.push(game);
        id
    }

    /// Every game is in: the two tables, and the initial position.
    fn finish(&mut self) {
        let rows = self.keys.len();
        let ids = |id: usize| u32::try_from(id).unwrap_or(NONE);
        let standing = Table::of(rows, self.nodes.iter().enumerate().map(|(id, n)| (n.position(), ids(id))));
        let nodes = &self.nodes;
        let children = Table::of(rows, nodes.iter().enumerate().filter_map(|(id, n)| Some((nodes.get(n.parent()?)?.position(), ids(id)))));
        self.standing = standing;
        self.children = children;
        self.start = self.position_id(start_key());
        self.nodes.shrink_to_fit();
        self.node_of.shrink_to_fit();
        self.keys.shrink_to_fit();
        self.game_of.shrink_to_fit();
    }

    /// The games, in the order of the PGN.
    pub fn games(&self) -> &[ReferenceGame] {
        &self.games
    }

    /// Game `k` (from 0) read again from its text, all of it (also what is past the caps), as the
    /// index read it; `None` when it could not be read or there is no such game.
    pub fn game(&self, k: usize) -> Option<PgnGame> {
        PgnGame::from_pgn_report(&self.texts.get(k)?.text).ok().map(|(game, _)| game)
    }

    /// Moves indexed, all games together.
    pub fn moves(&self) -> usize {
        self.games.iter().map(|g| g.moves).sum()
    }

    /// Different positions indexed (pieces and side to move).
    pub fn positions(&self) -> usize {
        self.keys.len()
    }

    /// Games or moves were left out ([`ReferenceIndex::cut`]).
    pub fn truncated(&self) -> bool {
        self.cut().is_some()
    }

    /// Why the rest of the PGN was left out: the moves' or the memory's cap before the games'.
    pub fn cut(&self) -> Option<Cut> {
        self.stopped.or(self.more_games.then_some(Cut::Games))
    }

    /// The bytes the index holds: its tables and the games' texts.
    pub fn memory(&self) -> usize {
        let vec = |len: usize, each: usize| len.saturating_mul(each);
        let texts: usize = self.texts.iter().map(|t| t.text.len()).sum();
        vec(self.nodes.capacity(), size_of::<IndexNode>())
            .saturating_add(vec(self.node_of.capacity(), size_of::<u32>()))
            .saturating_add(vec(self.keys.capacity(), size_of::<PositionKey>()))
            .saturating_add(vec(self.game_of.capacity(), size_of::<u32>()))
            .saturating_add(self.standing.bytes())
            .saturating_add(self.children.bytes())
            .saturating_add(vec(self.by_key.capacity(), MAP_ENTRY_BYTES))
            .saturating_add(self.sans.bytes)
            .saturating_add(vec(self.games.capacity(), size_of::<ReferenceGame>()))
            .saturating_add(self.names)
            .saturating_add(vec(self.texts.capacity(), size_of::<GameText>()))
            .saturating_add(texts)
    }

    /// Whether a position of the reference has `fen`'s pieces, and its side to move unless
    /// `side_assumed` ([`BookKey::matches`]); `false` for a FEN that cannot be read.
    pub fn has_position(&self, fen: &str, side_assumed: bool) -> bool {
        let Ok(fen) = Fen::parse(fen) else { return false };
        if side_assumed {
            return !self.with_pieces(PositionKey::pieces(fen.placement_key())).is_empty();
        }
        self.position_id(fen.book_key()).is_some()
    }

    /// Whether some game of the reference plays a move written `san` (check marks, annotation
    /// marks and a castling written with zeros set aside), from any position (plan A9-G4: whether
    /// a decision made against another PGN can still apply).
    pub fn has_move(&self, san: &str) -> bool {
        self.plain_id(san).is_some()
    }

    /// Move `at` as the reference writes it; `None` for a node it does not have or a game's start.
    pub fn san(&self, at: RefNode) -> Option<&str> {
        let node = self.nodes.get(self.node_id(at)?)?;
        self.sans.written.get(node.san as usize).map(|s| &**s).filter(|s| !s.is_empty())
    }

    /// The position after move `at`, played again from its game's start.
    pub fn fen(&self, at: RefNode) -> Option<Fen> {
        let game = self.texts.get(at.game)?;
        let mut node = self.nodes.get(self.node_id(at)?)?;
        let mut steps = Vec::new();
        while let Some(parent) = node.parent() {
            // A game has at most MAX_NODES moves, so a longer way back is not one.
            if steps.len() > MAX_NODES {
                return None;
            }
            steps.push(node.step);
            node = self.nodes.get(parent)?;
        }
        let mut board = game.start.clone();
        for &s in steps.iter().rev() {
            play(&mut board, s)?;
        }
        Some(Fen::from_board(&board))
    }

    /// The id of move `at` in [`ReferenceIndex::nodes`], when it was indexed.
    fn node_id(&self, at: RefNode) -> Option<usize> {
        let game = self.texts.get(at.game)?;
        if at.node >= game.len {
            return None;
        }
        let id = *self.node_of.get(game.first.checked_add(at.node)?)?;
        (id != NONE).then_some(id as usize)
    }

    pub(crate) fn node(&self, id: usize) -> Option<&IndexNode> {
        self.nodes.get(id)
    }

    /// Every node, in its id's order.
    pub(crate) fn nodes(&self) -> impl Iterator<Item = &IndexNode> {
        self.nodes.iter()
    }

    /// The position with these pieces and side to move.
    pub(crate) fn position_id(&self, key: BookKey) -> Option<usize> {
        self.by_key.get(&PositionKey::of(key)).map(|&p| p as usize)
    }

    /// The positions with the pieces of `pieces` ([`PositionKey::pieces`]), either side to move,
    /// in the order they were indexed.
    pub(crate) fn with_pieces(&self, pieces: PositionKey) -> Vec<usize> {
        let mut found: Vec<usize> =
            [Color::White, Color::Black].into_iter().filter_map(|side| self.by_key.get(&pieces.with_side(side))).map(|&p| p as usize).collect();
        found.sort_unstable();
        found
    }

    pub(crate) fn key(&self, position: usize) -> Option<PositionKey> {
        self.keys.get(position).copied()
    }

    /// The side to move at `position`.
    pub(crate) fn side(&self, position: usize) -> Option<Color> {
        self.key(position).map(PositionKey::side)
    }

    /// The one game `position` is in; `None` when several are.
    pub(crate) fn position_game(&self, position: usize) -> Option<usize> {
        self.game_of.get(position).filter(|&&g| g != NONE).map(|&g| g as usize)
    }

    /// The nodes standing on `position`.
    pub(crate) fn standing(&self, position: usize) -> &[u32] {
        self.standing.row(position)
    }

    /// The moves played from `position`, in every game.
    pub(crate) fn children(&self, position: usize) -> &[u32] {
        self.children.row(position)
    }

    /// The initial position, when the reference has it.
    pub(crate) fn start(&self) -> Option<usize> {
        self.start
    }

    /// The id of the plain SAN of `san` as a move of the text writes it ([`plain_san`]), when a
    /// move of the reference is written so.
    pub(crate) fn plain_id(&self, san: &str) -> Option<u32> {
        self.sans.plain.get(plain_san(san).as_str()).copied()
    }

    /// The id of node `node`'s plain SAN; `None` for a game's start.
    pub(crate) fn node_plain(&self, node: &IndexNode) -> Option<u32> {
        self.sans.plain_of.get(node.san as usize).copied()
    }
}

/// A move as two bytes: from (6 bits), to (6 bits), the promotion's piece and one (3 bits);
/// [`NULL_STEP`] for a null move.
fn step(mv: Option<Move>) -> u16 {
    let Some(mv) = mv else { return NULL_STEP };
    let promotion = mv.promotion.map_or(0, |p| p as u16 + 1);
    (mv.from as u16) | ((mv.to as u16) << 6) | (promotion << 12)
}

/// Play `step` on `board` as the game did ([`PgnGame::start_board`]); `None` when it cannot be.
fn play(board: &mut Board, step: u16) -> Option<()> {
    if step == NULL_STEP {
        *board = board.null_move()?;
        return Some(());
    }
    let from = CozySquare::try_index(usize::from(step & 63))?;
    let to = CozySquare::try_index(usize::from((step >> 6) & 63))?;
    let promotion = match (step >> 12) & 7 {
        0 => None,
        p => Some(CozyPiece::try_index(usize::from(p - 1))?),
    };
    board.try_play(Move { from, to, promotion }).ok()
}

/// The name of a game: its Event, Site, White and Black tags that say something, each once (a
/// course's export can repeat its Event as White and its Site as Black).
fn game_name(game: &PgnGame) -> String {
    name_from(|tag| game.tag(tag))
}

fn name_from<'a>(tag: impl Fn(&str) -> Option<&'a str>) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for value in ["Event", "Site", "White", "Black"].iter().filter_map(|t| tag(t)).map(str::trim) {
        if !value.is_empty() && !value.chars().all(|c| c == '?') && !parts.contains(&value) {
            parts.push(value);
        }
    }
    parts.join(" · ")
}

/// The name of a game past the caps, from its tags as the reader takes them (a control character
/// a space, at most [`MAX_TAG_LEN`] characters, the last of a name), its moves never read.
fn chunk_name(chunk: &str) -> String {
    let mut tags: Vec<(&str, String)> = Vec::new();
    for token in scan(chunk) {
        if token.kind.is_movetext() {
            break;
        }
        let Some((name, raw)) = chunk.get(token.range).filter(|_| token.kind == ScanKind::Tag).and_then(tag_parts) else { continue };
        let value: String = tag_value(raw).chars().map(|c| if c.is_control() { ' ' } else { c }).take(MAX_TAG_LEN).collect();
        tags.retain(|(n, _)| *n != name);
        tags.push((name, value.trim().to_owned()));
    }
    name_from(|t| tags.iter().find(|(n, _)| *n == t).map(|(_, v)| v.as_str()))
}

/// The last indexes made, by the SHA-256 of their PGN's text (plan A9-I3b): a PGN is indexed once,
/// whoever asks again, from whichever thread. At most [`MEMO_INDEXES`], the least recently asked
/// for going first; one that could not be made is not kept (it is made again, and fails again).
#[derive(Debug, Default)]
pub struct IndexMemo {
    /// The most recently asked for last.
    kept: Mutex<Vec<([u8; 32], Arc<ReferenceIndex>)>>,
    made: AtomicUsize,
}

impl IndexMemo {
    /// `text` indexed ([`ReferenceIndex::from_pgn`]): the index made for the same text before,
    /// else one made now, on the calling thread (the memo is not locked meanwhile).
    pub fn index(&self, text: &str) -> Result<Arc<ReferenceIndex>, ReferenceError> {
        if text.len() > MAX_REFERENCE_BYTES {
            return Err(ReferenceError::TooLarge { bytes: text.len(), max: MAX_REFERENCE_BYTES });
        }
        let digest: [u8; 32] = sha2::Sha256::digest(text.as_bytes()).into();
        if let Some(found) = self.take(&digest) {
            return Ok(found);
        }
        let index = Arc::new(ReferenceIndex::from_pgn(text)?);
        self.made.fetch_add(1, Ordering::Relaxed);
        let mut kept = self.kept.lock().unwrap_or_else(PoisonError::into_inner);
        // Made meanwhile on another thread too: the one kept first stays the one.
        if let Some((_, first)) = kept.iter().find(|(d, _)| *d == digest) {
            return Ok(first.clone());
        }
        kept.push((digest, index.clone()));
        let over = kept.len().saturating_sub(MEMO_INDEXES);
        kept.drain(..over);
        Ok(index)
    }

    /// The index kept for `digest`, now the most recently asked for.
    fn take(&self, digest: &[u8; 32]) -> Option<Arc<ReferenceIndex>> {
        let mut kept = self.kept.lock().unwrap_or_else(PoisonError::into_inner);
        let at = kept.iter().position(|(d, _)| d == digest)?;
        let entry = kept.remove(at);
        let index = entry.1.clone();
        kept.push(entry);
        Some(index)
    }

    /// How many indexes this memo made (not found kept), for the tests and the measure.
    pub fn made(&self) -> usize {
        self.made.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests;
