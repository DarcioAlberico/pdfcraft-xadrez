//! A book's index in the Library (plan A9-N15a): every position the reading of the book reached,
//! one fixed-size record each, and what the search answers with besides (the book's title, how it
//! numbers its pages, its games and its exercises).
//!
//! **What is indexed.** Each diagram with a position (the one it shows: as the book's PGN
//! confirmed it, as the text settled it, else as read; [`DiagramBlock::shown`]); each move of the
//! text with a position after it (played by the reader, or recovered from the book's PGN); and the
//! moves of the book's PGN no move of the text was linked to, every line, each game played again
//! once. A record holds the position's key ([`BookKey`]: the placement and the side to move, and
//! whether the side was assumed), where it is (the page, and the diagram's place or the move's
//! address [`MoveAt`] or the PGN's game and node), the move as numbered and its SAN, its game,
//! and, for the search by material and pawns (plan A9-N16a), its material signature and its pawn
//! marks.
//!
//! **The file**, `<hash>.lib`, is in the mould of a page's reading kept on disk (plan A9-N5a): the
//! magic line, [`VERSION`], the key it was made under, the SHA-256 and the length of what it
//! holds, then that deflated: the length of a JSON part and the JSON ([`IndexMeta`]), the number
//! of records and the records, [`RECORD`] bytes each. It is untrusted input like any file: read
//! within [`MAX_FILE_BYTES`], every length checked against the others and against its cap, and
//! inflated as it is read, so nothing is allocated from what a length says.
//!
//! [`DiagramBlock::shown`]: pdfcraft_chessvision::doc::DiagramBlock::shown
//! [`MoveAt`]: pdfcraft_chessvision::doc::MoveAt

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Read as _;
use std::path::Path;

use pdfcraft_chess::fen::{BookKey, Fen, PlacementKey};
use pdfcraft_chess::game::Game as PgnGame;
use pdfcraft_chess::pattern::{Material, PawnTraits};
use pdfcraft_chess::placement::{Color, PieceKind, Placement};
use pdfcraft_chessvision::doc::reference::{LinkKind, ReferenceIndex};
use pdfcraft_chessvision::doc::{Block, ChessDoc, GameOrigin, Inline, MoveStatus};
use serde::{Deserialize, Serialize};
use sha2::Digest as _;

/// The file format's version.
pub const VERSION: u32 = 1;
/// Bytes of one position's record.
pub const RECORD: usize = 68;
/// Largest index file read.
pub const MAX_FILE_BYTES: usize = 64 << 20;
/// Most positions one book's index holds; the rest of the book is said to be left out.
pub const MAX_POSITIONS: usize = 4_000_000;
/// Largest JSON part.
pub const MAX_META_BYTES: usize = 32 << 20;
/// Most games and exercises listed in the JSON part, each.
pub const MAX_LISTED: usize = 100_000;

/// A file starts with it.
const MAGIC: &[u8] = b"pdfcraft-chess-library-index\n";
/// The magic line, the version, the key, the digest and the length of what it holds.
const HEADER: usize = MAGIC.len() + 4 + 32 + 32 + 4;
/// Most bytes it holds, inflated.
const MAX_PAYLOAD: usize = 8 + MAX_META_BYTES + MAX_POSITIONS * RECORD;
/// Records inflated at a time.
const CHUNK: usize = 1024;

/// The order a record's material lists each side's pieces in (that of a signature).
const PIECES: [PieceKind; 6] = [PieceKind::King, PieceKind::Queen, PieceKind::Rook, PieceKind::Bishop, PieceKind::Knight, PieceKind::Pawn];

/// What an index was made under: its book's content and everything its reading depends on
/// ([`super::index_key`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct IndexKey(pub(crate) [u8; 32]);

/// Where a position is in the book.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// A diagram: its place among the page's diagrams in reading order, from 0.
    Diagram { index: u32 },
    /// A move of the text: its block on the page and its inline in the block, from 0 (`MoveAt`).
    Move { block: u32, inline: u32 },
    /// A move of the book's PGN: its game in the PGN and its node, from 0.
    Pgn { game: u32, node: u32 },
}

/// One position of the book.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IndexedPosition {
    pub placement: PlacementKey,
    /// The side to move (White when the reading did not say: then `side_assumed`).
    pub side: Color,
    pub side_assumed: bool,
    /// The page, from 0; `None` for a move of the book's PGN that says none.
    pub page: Option<u32>,
    pub place: Place,
    /// The move that reached it, as numbered (`12.`, `12...`): 0 for a diagram.
    pub number: u16,
    pub black: bool,
    /// The move's SAN, in [`IndexMeta::sans`]; `None` for a diagram.
    pub san: Option<u32>,
    /// Its game, in [`IndexMeta::games`]: a move's, or the game a diagram starts.
    pub game: Option<u32>,
    /// On its game's main line.
    pub main_line: bool,
    /// A diagram's reading passed the gate, a person set it or the book's PGN confirmed it; a
    /// move's game starts from such a position.
    pub accepted: bool,
    /// How many of each piece each side has: White's then Black's, K Q R B N P.
    pub material: [u8; 12],
    /// The pawn marks: bits 0–2 White's doubled, isolated and passed pawns, bits 3–5 Black's.
    pub pawns: u8,
}

impl IndexedPosition {
    /// Its key.
    pub fn key(&self) -> BookKey {
        BookKey { placement: self.placement, side: self.side }
    }

    /// Whether it holds `material`, or with `either_side` that material with the colours swapped.
    pub fn has_material(&self, material: &Material, either_side: bool) -> bool {
        let wanted = material_counts(|c, k| material.count(c, k));
        let swapped = material_counts(|c, k| material.count(c.opponent(), k));
        self.material == wanted || (either_side && self.material == swapped)
    }

    /// The record's bytes.
    fn to_bytes(self) -> [u8; RECORD] {
        let mut b = [0u8; RECORD];
        let mut at = 0usize;
        let mut put = |bytes: &[u8]| {
            if let Some(slot) = b.get_mut(at..at.saturating_add(bytes.len())) {
                slot.copy_from_slice(bytes);
            }
            at = at.saturating_add(bytes.len());
        };
        for w in self.placement.words() {
            put(&w.to_le_bytes());
        }
        let (kind, a, c) = match self.place {
            Place::Diagram { index } => (0u8, index, 0),
            Place::Move { block, inline } => (1, block, inline),
            Place::Pgn { game, node } => (2, game, node),
        };
        let flags = u8::from(self.side == Color::Black)
            | u8::from(self.side_assumed) << 1
            | u8::from(self.main_line) << 2
            | u8::from(self.black) << 3
            | u8::from(self.accepted) << 4
            | kind << 5;
        put(&[flags, self.pawns]);
        put(&self.material);
        put(&self.number.to_le_bytes());
        put(&self.page.unwrap_or(u32::MAX).to_le_bytes());
        put(&a.to_le_bytes());
        put(&c.to_le_bytes());
        put(&self.san.unwrap_or(u32::MAX).to_le_bytes());
        put(&self.game.unwrap_or(u32::MAX).to_le_bytes());
        b
    }

    /// The position a record holds; `None` for one no index writes (an unknown kind).
    fn from_bytes(b: &[u8; RECORD]) -> Option<IndexedPosition> {
        let u32_at = |at: usize| b.get(at..at.checked_add(4)?).and_then(|s| s.try_into().ok()).map(u32::from_le_bytes);
        let u64_at = |at: usize| b.get(at..at.checked_add(8)?).and_then(|s| s.try_into().ok()).map(u64::from_le_bytes);
        let some = |v: u32| (v != u32::MAX).then_some(v);
        let placement = PlacementKey::from_words([u64_at(0)?, u64_at(8)?, u64_at(16)?, u64_at(24)?]);
        let flags = *b.get(32)?;
        let pawns = *b.get(33)?;
        let material: [u8; 12] = b.get(34..46)?.try_into().ok()?;
        let number = u16::from_le_bytes(b.get(46..48)?.try_into().ok()?);
        let (page, a, c, san, game) = (u32_at(48)?, u32_at(52)?, u32_at(56)?, u32_at(60)?, u32_at(64)?);
        let place = match flags >> 5 {
            0 => Place::Diagram { index: a },
            1 => Place::Move { block: a, inline: c },
            2 => Place::Pgn { game: a, node: c },
            _ => return None,
        };
        Some(IndexedPosition {
            placement,
            side: if flags & 1 != 0 { Color::Black } else { Color::White },
            side_assumed: flags & 2 != 0,
            page: some(page),
            place,
            number,
            black: flags & 8 != 0,
            san: some(san),
            game: some(game),
            main_line: flags & 4 != 0,
            accepted: flags & 16 != 0,
            material,
            pawns,
        })
    }
}

/// Twelve counts in a record's order, from `count(color, kind)`.
fn material_counts(count: impl Fn(Color, PieceKind) -> u8) -> [u8; 12] {
    let mut out = [0u8; 12];
    for (slot, (color, kind)) in out.iter_mut().zip([Color::White, Color::Black].into_iter().flat_map(|c| PIECES.map(|k| (c, k)))) {
        *slot = count(color, kind);
    }
    out
}

/// The material of a placement, as a record keeps it.
pub fn material_of(placement: &Placement) -> [u8; 12] {
    let m = Material::of(placement);
    material_counts(|c, k| m.count(c, k))
}

/// The pawn marks of a placement, as a record keeps them.
pub fn pawn_marks(placement: &Placement) -> u8 {
    let mut out = 0u8;
    for (shift, color) in [(0u8, Color::White), (3, Color::Black)] {
        let t = PawnTraits::of(placement, color);
        out |= (u8::from(t.has_doubled()) | u8::from(t.has_isolated()) << 1 | u8::from(t.has_passed()) << 2) << shift;
    }
    out
}

/// A game of the book, for the search's answer.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexedGame {
    /// Of the book's PGN, not read from the text.
    #[serde(default)]
    pub pgn: bool,
    pub white: String,
    pub black: String,
    pub event: String,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub eco: Option<String>,
    /// The page its first move is on, from 0 (a PGN's game: its `PdfPage`, when it has one).
    #[serde(default)]
    pub page: Option<u32>,
    /// Its main line's moves.
    #[serde(default)]
    pub moves: u32,
}

/// An exercise of the book (plan A9-N10's rule).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexedExercise {
    /// Its diagram: the page from 0, and its place on it from 0.
    pub page: u32,
    pub diagram: u32,
    /// The book's number for it.
    pub number: u32,
    /// Its solution's main line's moves.
    pub moves: u32,
}

/// What a book's reading held, counted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexCounts {
    /// Pages read.
    pub pages_read: u32,
    /// Diagrams found, those read into a position, and those whose reading is to review.
    pub diagrams: u32,
    pub read: u32,
    pub review: u32,
    /// Moves of the text with a position, and moves of the book's PGN indexed besides.
    pub moves: u32,
    pub pgn_moves: u32,
    pub games: u32,
    pub exercises: u32,
    /// Positions indexed, and whether [`MAX_POSITIONS`] left some out.
    pub positions: u32,
    #[serde(default)]
    pub capped: bool,
    /// Pages whose text is poor and that chess OCR did not read (no glyph model).
    #[serde(default)]
    pub needs_ocr: u32,
}

/// The JSON part of an index.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexMeta {
    pub title: String,
    pub pages: u32,
    /// The page labels as the book numbers its pages, by page from 0 (its `/PageLabels`, else the
    /// numbers it prints: `PageLabel::with_folios`); a page with none, or the list empty: its own
    /// number from 1.
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub games: Vec<IndexedGame>,
    #[serde(default)]
    pub exercises: Vec<IndexedExercise>,
    /// The SANs the records name, each once.
    #[serde(default)]
    pub sans: Vec<String>,
    /// The book's PGN that was kept for it, by its file name.
    #[serde(default)]
    pub pgn: Option<String>,
    #[serde(default)]
    pub counts: IndexCounts,
}

impl IndexMeta {
    /// Page `page` (from 0) as the book numbers it.
    pub fn label(&self, page: u32) -> String {
        let label = usize::try_from(page).ok().and_then(|p| self.labels.get(p)).map(String::as_str);
        pdfcraft_chessvision::doc::PageLabel::new(label, page).to_string()
    }
}

/// A book's index, whole: as it is made, before it is written.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BookIndex {
    pub meta: IndexMeta,
    pub positions: Vec<IndexedPosition>,
}

/// The SANs a book's index names, each once.
#[derive(Default)]
struct Sans {
    list: Vec<String>,
    at: HashMap<String, u32>,
}

impl Sans {
    fn of(&mut self, san: &str) -> Option<u32> {
        if let Some(i) = self.at.get(san) {
            return Some(*i);
        }
        let i = u32::try_from(self.list.len()).ok().filter(|i| *i != u32::MAX)?;
        self.list.push(san.to_owned());
        self.at.insert(san.to_owned(), i);
        Some(i)
    }
}

/// A move numbered from the position after it: Black to move after it, White played it.
fn numbered_from(fen: &Fen) -> (u16, bool) {
    let full = fen.fullmove_number();
    let (n, black) = match fen.side() {
        Some(Color::Black) => (full, false),
        _ => (full.saturating_sub(1).max(1), true),
    };
    (u16::try_from(n).unwrap_or(u16::MAX), black)
}

/// The book's index made from its reading `doc` (linked to its PGN `pgn`, when one is kept),
/// titled `title`, its pages labelled `labels` (`PageLabel::with_folios`).
pub fn index_of(doc: &ChessDoc, pgn: Option<(&str, &ReferenceIndex)>, title: &str, pages: u32, labels: Vec<String>) -> BookIndex {
    let mut sans = Sans::default();
    let mut positions: Vec<IndexedPosition> = Vec::new();
    let mut counts = IndexCounts::default();
    let mut capped = false;
    let mut push = |positions: &mut Vec<IndexedPosition>, p: IndexedPosition| {
        if positions.len() < MAX_POSITIONS {
            positions.push(p);
        } else {
            capped = true;
        }
    };
    let text_games = doc.games.len();
    // The game each diagram starts, and each game's main line.
    let mut starts: HashMap<(u32, usize), u32> = HashMap::new();
    let mut main: Vec<HashSet<usize>> = Vec::with_capacity(text_games);
    for (k, g) in doc.games.iter().enumerate() {
        if let GameOrigin::Diagram { page, diagram, .. } = g.origin
            && let Ok(k) = u32::try_from(k)
        {
            starts.entry((page, diagram)).or_insert(k);
        }
        main.push(g.score.mainline().into_iter().collect());
    }
    // The PGN's nodes a move of the text was linked to: indexed with the text's move.
    let mut linked: HashSet<(usize, usize)> = HashSet::new();
    for page in &doc.pages {
        counts.pages_read = counts.pages_read.saturating_add(1);
        for (b, block) in page.blocks.iter().enumerate() {
            if let Block::Diagram(d) = block {
                counts.diagrams = counts.diagrams.saturating_add(1);
                if d.open_review().next().is_some() || d.reading.as_ref().is_some_and(|r| !r.accepted) {
                    counts.review = counts.review.saturating_add(1);
                }
                let Some(shown) = d.shown() else { continue };
                let Ok(fen) = Fen::parse(shown.fen) else { continue };
                counts.read = counts.read.saturating_add(1);
                let placement = *fen.placement();
                let accepted =
                    shown.confirmed || d.source == pdfcraft_chessvision::doc::DiagramSource::User || d.reading.as_ref().is_some_and(|r| r.accepted);
                push(
                    &mut positions,
                    IndexedPosition {
                        placement: fen.placement_key(),
                        side: fen.book_key().side,
                        side_assumed: !shown.side_known,
                        page: Some(page.index),
                        place: Place::Diagram { index: u32::try_from(d.number).unwrap_or(u32::MAX) },
                        number: 0,
                        black: false,
                        san: None,
                        game: starts.get(&(page.index, d.number)).copied(),
                        main_line: false,
                        accepted,
                        material: material_of(&placement),
                        pawns: pawn_marks(&placement),
                    },
                );
                continue;
            }
            for (i, inline) in block.inlines().iter().enumerate() {
                let Inline::Move(m) = inline else { continue };
                let recovered = m.reference.as_ref().filter(|r| r.kind == LinkKind::Recovered).and_then(|r| r.fen.as_deref());
                let fen_text = if m.status == MoveStatus::Valid { m.fen.as_deref() } else { recovered };
                let Some(fen) = fen_text.and_then(|f| Fen::parse(f).ok()) else { continue };
                if let Some(r) = m.reference.as_ref().filter(|r| r.kind != LinkKind::Absent)
                    && let (Some(g), Some(n)) = (r.game, r.node)
                {
                    linked.insert((g, n));
                }
                let placement = *fen.placement();
                let (number, black) = match m.number {
                    Some(n) => (n.number, n.black),
                    None => numbered_from(&fen),
                };
                let game = m.game.filter(|_| m.status == MoveStatus::Valid);
                let trusted = game.and_then(|g| doc.games.get(g.game)).is_some_and(|g| g.trusted);
                counts.moves = counts.moves.saturating_add(1);
                push(
                    &mut positions,
                    IndexedPosition {
                        placement: fen.placement_key(),
                        side: fen.book_key().side,
                        side_assumed: false,
                        page: Some(page.index),
                        place: Place::Move { block: u32::try_from(b).unwrap_or(u32::MAX), inline: u32::try_from(i).unwrap_or(u32::MAX) },
                        number,
                        black,
                        san: sans.of(&m.san),
                        game: game.and_then(|g| u32::try_from(g.game).ok()),
                        main_line: game.is_some_and(|g| main.get(g.game).is_some_and(|line| line.contains(&g.node))),
                        accepted: trusted,
                        material: material_of(&placement),
                        pawns: pawn_marks(&placement),
                    },
                );
            }
        }
    }
    let mut games: Vec<IndexedGame> = doc
        .games
        .iter()
        .take(MAX_LISTED)
        .map(|g| {
            let r = &g.score.roster;
            IndexedGame {
                pgn: false,
                white: r.white.clone(),
                black: r.black.clone(),
                event: r.event.clone(),
                date: r.date.clone(),
                eco: g.score.extra_tags.iter().find(|t| t.name == "ECO").map(|t| t.value.clone()),
                page: Some(g.page),
                moves: u32::try_from(g.score.mainline().len()).unwrap_or(u32::MAX),
            }
        })
        .collect();
    // The book's PGN: its games, and the positions of its moves no move of the text reached.
    let mut pgn_name = None;
    if let Some((name, index)) = pgn {
        pgn_name = Some(name.to_owned());
        for (k, g) in index.games().iter().enumerate() {
            let game = (g.moves > 0).then(|| index.game(k)).flatten();
            let tag = |name: &str| game.as_ref().and_then(|p| p.tag(name)).unwrap_or("?").to_owned();
            let page = game.as_ref().and_then(|p| p.tag("PdfPage")).and_then(|v| v.trim().parse::<u32>().ok()).and_then(|p| p.checked_sub(1));
            let at = u32::try_from(text_games.saturating_add(k)).ok();
            if games.len() < MAX_LISTED {
                games.push(IndexedGame {
                    pgn: true,
                    white: tag("White"),
                    black: tag("Black"),
                    event: tag("Event"),
                    date: tag("Date"),
                    eco: game.as_ref().and_then(|p| p.tag("ECO")).map(str::to_owned),
                    page,
                    moves: u32::try_from(game.as_ref().map_or(0, main_len)).unwrap_or(u32::MAX),
                });
            }
            let Some(game) = game else { continue };
            pgn_positions(&game, k, at, page, &linked, &mut sans, &mut |p| {
                counts.pgn_moves = counts.pgn_moves.saturating_add(1);
                push(&mut positions, p);
            });
        }
    }
    let (exercises, _) = pdfcraft_chessvision::doc::exercises::list(doc);
    let exercises: Vec<IndexedExercise> = exercises
        .iter()
        .take(MAX_LISTED)
        .map(|e| IndexedExercise {
            page: e.at.page_index(),
            diagram: u32::try_from(e.at.index).unwrap_or(u32::MAX),
            number: e.number,
            moves: u32::try_from(e.moves).unwrap_or(u32::MAX),
        })
        .collect();
    counts.games = u32::try_from(games.len()).unwrap_or(u32::MAX);
    counts.exercises = u32::try_from(exercises.len()).unwrap_or(u32::MAX);
    counts.positions = u32::try_from(positions.len()).unwrap_or(u32::MAX);
    counts.capped = capped;
    let meta = IndexMeta { title: title.to_owned(), pages, labels, games, exercises, sans: sans.list, pgn: pgn_name, counts };
    BookIndex { meta, positions }
}

/// The moves of `game`'s main line.
fn main_len(game: &PgnGame) -> usize {
    let mut n = 0usize;
    let mut node = 0;
    while let Some(next) = game.replies(node).first() {
        // A game is a tree: its node count bounds the walk.
        if n > game.move_count() {
            break;
        }
        n = n.saturating_add(1);
        node = *next;
    }
    n
}

/// The positions after the moves of the PGN game `game` (game `k` of the PGN, `at` in the
/// index's games) that no move of the text was linked to (`linked`), given to `each`: its main
/// line first, then its variations in the order they branch off, every node once.
fn pgn_positions(
    game: &PgnGame,
    k: usize,
    at: Option<u32>,
    page: Option<u32>,
    linked: &HashSet<(usize, usize)>,
    sans: &mut Sans,
    each: &mut dyn FnMut(IndexedPosition),
) {
    let mut lines: VecDeque<(usize, bool)> = VecDeque::new();
    let roots = game.replies(0);
    if let Some(first) = roots.first() {
        lines.push_back((*first, true));
    }
    lines.extend(roots.iter().skip(1).map(|n| (*n, false)));
    // A game is a tree: its node count bounds the walk.
    let mut budget = game.move_count().saturating_add(1);
    while let Some((start, main_line)) = lines.pop_front() {
        let mut node = start;
        loop {
            if budget == 0 {
                return;
            }
            budget -= 1;
            if !linked.contains(&(k, node))
                && let Some(fen) = game.fen_at(node)
            {
                let placement = *fen.placement();
                let (number, black) = numbered_from(&fen);
                each(IndexedPosition {
                    placement: fen.placement_key(),
                    side: fen.book_key().side,
                    side_assumed: false,
                    page,
                    place: Place::Pgn { game: u32::try_from(k).unwrap_or(u32::MAX), node: u32::try_from(node).unwrap_or(u32::MAX) },
                    number,
                    black,
                    san: sans.of(game.san(node)),
                    game: at,
                    main_line,
                    accepted: true,
                    material: material_of(&placement),
                    pawns: pawn_marks(&placement),
                });
            }
            let replies = game.replies(node);
            lines.extend(replies.iter().skip(1).map(|n| (*n, false)));
            match replies.first() {
                Some(next) => node = *next,
                None => break,
            }
        }
    }
}

/// `index` as a file under `key`: `None` when it is larger than a file may be.
pub fn encode(index: &BookIndex, key: &IndexKey) -> Option<Vec<u8>> {
    use std::io::Write as _;
    let meta = serde_json::to_vec(&index.meta).ok()?;
    if meta.len() > MAX_META_BYTES || index.positions.len() > MAX_POSITIONS {
        return None;
    }
    let mut payload = Vec::with_capacity(8usize.saturating_add(meta.len()).saturating_add(index.positions.len().saturating_mul(RECORD)));
    payload.extend_from_slice(&u32::try_from(meta.len()).ok()?.to_le_bytes());
    payload.extend_from_slice(&meta);
    payload.extend_from_slice(&u32::try_from(index.positions.len()).ok()?.to_le_bytes());
    for p in &index.positions {
        payload.extend_from_slice(&p.to_bytes());
    }
    let mut out = Vec::with_capacity(HEADER.saturating_add(payload.len() / 4));
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&key.0);
    out.extend_from_slice(&sha2::Sha256::digest(&payload));
    out.extend_from_slice(&u32::try_from(payload.len()).ok()?.to_le_bytes());
    let mut z = flate2::write::DeflateEncoder::new(out, flate2::Compression::default());
    z.write_all(&payload).ok()?;
    let out = z.finish().ok()?;
    (out.len() <= MAX_FILE_BYTES).then_some(out)
}

/// Why an index could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadError {
    /// There is none.
    Missing,
    /// Made by another version of the format, or under another key: out of date, not damaged.
    Stale,
    /// It can't be trusted, and why: a check failed.
    Damaged(String),
    /// The file could not be read now (another program holding it, say), and why: nothing is
    /// known of it, so it is neither trusted nor set aside.
    Unreadable(String),
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::Missing => f.write_str("there is no index"),
            ReadError::Stale => f.write_str("the index is out of date"),
            ReadError::Damaged(why) => write!(f, "the index is damaged: {why}"),
            ReadError::Unreadable(why) => write!(f, "the index can't be read now: {why}"),
        }
    }
}

/// The key an index file was made under, from its header alone; `None` when it has none to give.
pub fn key_of(path: &Path) -> Option<IndexKey> {
    let mut head = [0u8; HEADER];
    std::fs::File::open(path).ok()?.read_exact(&mut head).ok()?;
    let rest = head.strip_prefix(MAGIC)?;
    let (version, rest) = rest.split_at_checked(4)?;
    (u32::from_le_bytes(version.try_into().ok()?) == VERSION).then_some(())?;
    Some(IndexKey(rest.get(..32)?.try_into().ok()?))
}

/// Read the index at `path`, made under `key` (`None`: any key): its JSON part, read first, and
/// each position given to `each` with it as it is inflated. What `each` is given is checked only
/// once the whole file is: on an error, drop it.
pub fn read(path: &Path, key: Option<&IndexKey>, each: &mut dyn FnMut(&IndexMeta, &IndexedPosition)) -> Result<IndexMeta, ReadError> {
    let bytes = match super::super::corrections::read_capped(path, MAX_FILE_BYTES) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(ReadError::Missing),
        Err(e) => return Err(ReadError::Unreadable(e.to_string())),
    };
    if bytes.len() > MAX_FILE_BYTES {
        return Err(ReadError::Damaged(format!("larger than {} MB", MAX_FILE_BYTES >> 20)));
    }
    let short = || ReadError::Damaged("too short to be an index".into());
    let rest = bytes.strip_prefix(MAGIC).ok_or_else(|| ReadError::Damaged("not a book's index".into()))?;
    let (version, rest) = rest.split_at_checked(4).ok_or_else(short)?;
    let (stored, rest) = rest.split_at_checked(32).ok_or_else(short)?;
    let (digest, rest) = rest.split_at_checked(32).ok_or_else(short)?;
    let (len, body) = rest.split_at_checked(4).ok_or_else(short)?;
    let version = u32::from_le_bytes(version.try_into().map_err(|_| short())?);
    if version != VERSION || key.is_some_and(|k| k.0 != stored) {
        return Err(ReadError::Stale);
    }
    let len = usize::try_from(u32::from_le_bytes(len.try_into().map_err(|_| short())?)).map_err(|_| short())?;
    if len > MAX_PAYLOAD {
        return Err(ReadError::Damaged("it says it holds more than an index may".into()));
    }
    let damaged = |e: std::io::Error| ReadError::Damaged(format!("its data is damaged: {e}"));
    // One byte more than said: a longer stream is told by what is left.
    let mut z = flate2::read::DeflateDecoder::new(body).take(u64::try_from(len).unwrap_or(u64::MAX).saturating_add(1));
    let mut hash = sha2::Sha256::new();
    let mut word = [0u8; 4];
    z.read_exact(&mut word).map_err(damaged)?;
    hash.update(word);
    let meta_len = usize::try_from(u32::from_le_bytes(word)).map_err(|_| short())?;
    if meta_len > MAX_META_BYTES || meta_len.checked_add(8).is_none_or(|n| n > len) {
        return Err(ReadError::Damaged("its parts do not add up".into()));
    }
    // Grown as it inflates: the length said is not trusted for an allocation.
    let mut meta = Vec::new();
    (&mut z).take(u64::try_from(meta_len).unwrap_or(u64::MAX)).read_to_end(&mut meta).map_err(damaged)?;
    if meta.len() != meta_len {
        return Err(ReadError::Damaged("its data is cut short".into()));
    }
    hash.update(&meta);
    let meta = serde_json::from_slice::<IndexMeta>(&meta).map_err(|e| ReadError::Damaged(format!("its list can't be read: {e}")))?;
    z.read_exact(&mut word).map_err(damaged)?;
    hash.update(word);
    let count = usize::try_from(u32::from_le_bytes(word)).map_err(|_| short())?;
    let records = count.checked_mul(RECORD).and_then(|r| r.checked_add(meta_len)?.checked_add(8));
    if count > MAX_POSITIONS || records != Some(len) {
        return Err(ReadError::Damaged("its parts do not add up".into()));
    }
    let mut chunk = vec![0u8; CHUNK.saturating_mul(RECORD)];
    let mut left = count;
    while left > 0 {
        let n = left.min(CHUNK);
        let buf = chunk.get_mut(..n.saturating_mul(RECORD)).ok_or_else(short)?;
        z.read_exact(buf).map_err(damaged)?;
        hash.update(&*buf);
        for record in buf.as_chunks::<RECORD>().0 {
            each(&meta, &IndexedPosition::from_bytes(record).ok_or_else(|| ReadError::Damaged("a record of an unknown kind".into()))?);
        }
        left -= n;
    }
    let mut more = [0u8; 1];
    if z.read(&mut more).map_err(damaged)? != 0 {
        return Err(ReadError::Damaged("it holds more than it says".into()));
    }
    if hash.finalize().as_slice() != digest {
        return Err(ReadError::Damaged("its data is damaged".into()));
    }
    Ok(meta)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn position(i: u32) -> IndexedPosition {
        let placement = *Fen::parse("8/8/8/4k3/8/8/4P3/4K2R w - - 0 1").unwrap().placement();
        IndexedPosition {
            placement: PlacementKey::of(&placement),
            side: if i.is_multiple_of(2) { Color::White } else { Color::Black },
            side_assumed: i.is_multiple_of(3),
            page: (!i.is_multiple_of(5)).then_some(i),
            place: match i % 3 {
                0 => Place::Diagram { index: i },
                1 => Place::Move { block: i, inline: i + 1 },
                _ => Place::Pgn { game: i, node: i + 2 },
            },
            number: u16::try_from(i % 300).unwrap(),
            black: i % 2 == 1,
            san: (!i.is_multiple_of(4)).then_some(i / 4),
            game: (!i.is_multiple_of(7)).then_some(i / 7),
            main_line: i.is_multiple_of(2),
            accepted: i % 3 != 1,
            material: material_of(&placement),
            pawns: pawn_marks(&placement),
        }
    }

    fn dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("pdfcraft-library-index-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A record reads back as written, every field.
    #[test]
    fn a_record_reads_back_as_written() {
        for i in 0..64 {
            let p = position(i);
            assert_eq!(IndexedPosition::from_bytes(&p.to_bytes()), Some(p));
        }
        // The material and the pawns as the search reads them: KRP-K, the pawn isolated and passed.
        let p = position(0);
        assert!(p.has_material(&Material::parse("KRP-K").unwrap(), false));
        assert!(!p.has_material(&Material::parse("K-KRP").unwrap(), false));
        assert!(p.has_material(&Material::parse("K-KRP").unwrap(), true));
        assert_eq!(p.pawns, 0b110, "White's pawn isolated and passed, not doubled");
    }

    /// An index file reads back whole; another key or version is out of date; a byte changed
    /// anywhere or the file cut short is damaged, never served.
    #[test]
    fn an_index_reads_back_and_a_damaged_one_is_refused() {
        let dir = dir("roundtrip");
        let index = BookIndex {
            meta: IndexMeta { title: "Book".into(), pages: 3, sans: vec!["e4".into()], ..Default::default() },
            positions: (0..2500).map(position).collect(),
        };
        let key = IndexKey([7; 32]);
        let bytes = encode(&index, &key).unwrap();
        let path = dir.join("a.lib");
        std::fs::write(&path, &bytes).unwrap();
        let mut got = Vec::new();
        let meta = read(&path, Some(&key), &mut |_, p| got.push(*p)).unwrap();
        assert_eq!(BookIndex { meta, positions: got }, index);
        assert_eq!(key_of(&path), Some(key));
        assert_eq!(read(&path, Some(&IndexKey([8; 32])), &mut |_, _| {}), Err(ReadError::Stale));
        assert!(read(&path, None, &mut |_, _| {}).is_ok());
        assert_eq!(read(&dir.join("none.lib"), None, &mut |_, _| {}), Err(ReadError::Missing));
        for at in [HEADER - 1, HEADER + 3, bytes.len() / 2, bytes.len() - 1] {
            let mut b = bytes.clone();
            b[at] ^= 0x41;
            std::fs::write(&path, &b).unwrap();
            assert!(matches!(read(&path, Some(&key), &mut |_, _| {}), Err(ReadError::Damaged(_))), "byte {at}");
        }
        for b in [bytes[..bytes.len() - 10].to_vec(), bytes[..HEADER + 20].to_vec(), bytes[..HEADER - 5].to_vec()] {
            std::fs::write(&path, &b).unwrap();
            assert!(matches!(read(&path, Some(&key), &mut |_, _| {}), Err(ReadError::Damaged(_))), "{} bytes", b.len());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
