//! Chess ▸ Game collections: the autosave of each document's chess games (PGN toolbar spec §4.7,
//! D10, T4b).
//!
//! **Where.** On the desktop, one file per document in the `chess` folder of the recovery folder
//! ([`crate::RecoveryStore::default_dir`]: the user's own data folder, owner-only on Unix), named
//! `<id>.games` after an id the collection gets when it is made. On the web, one value in eframe's
//! `Storage` under [`STORAGE_KEY`], capped at [`WEB_MAX_BYTES`] for all documents together.
//!
//! **Format** (version [`VERSION`]). A file is two lines of JSON: a short [`Header`] (format,
//! version, the document's keys, the instance that wrote it, how many games), so finding a
//! document's file reads only the headers; then the [`Body`]: the active game and, per game, its
//! PGN text and what PGN can't hold: the move shown (its [`NodePath`]), the board's side, an
//! illegal position as its FEN, the diagram it came from, where each move was read on its page
//! (`NodeSource`, by path) and the comments PGN can't place exactly (a comment before a first
//! reply with no variation before it is written into the comment after the move before it, and
//! read back as part of that one): every move whose comments read back differently is stored
//! with both of its comments, so a reload puts them back where they were. The installed builds
//! put comments back only through these, so they are written as long as the files are `*.games`
//! of version 1 (plan A9 §2.10, R9-01), and also for every comment before a first reply and the
//! move before it, which a build before plan A9-F2b reads differently from what this build
//! writes; a sample of the format is frozen in `tests/compat/games_v1.games`.
//!
//! **Versions** (plan A9-G12). A file a newer PdfCraft wrote (a higher version) is never renamed,
//! set aside or written over, whichever document it is for: its own document's games are read as
//! far as this version can make them out, and writing them is refused ([`Blocked::Newer`]). The
//! same holds for the web's value. A newer format takes a new file name (`<hash>.games.json`),
//! which this version doesn't look at.
//!
//! **Safety.** Files are written atomically ([`crate::editing::write_atomically`]) and only by the
//! instance that holds a lock in the recovery folder; a file another running instance wrote is
//! left alone. On the desktop the interface thread only encodes a document's games (what changed,
//! plan A9-I3a); the file is checked and written by the store's writer thread (plan A9-I3b,
//! [`crate::chess_writer`]), the last request of each file winning, and what became of the write
//! comes back as [`Written::Queued`]'s generation with its [`Blocked`], if any. Reading reads the first line within a cap, then checks the size, and parses
//! leniently; a file that can't be read as this version's or a newer one's, or is too big, is set
//! aside as `….bad` (never deleted) and the user is told. Nothing is evicted: a document above
//! [`MAX_GAMES`] games or [`MAX_BODY_BYTES`] is not autosaved (the user is told) until it is back
//! under.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, Read};
use std::path::{Path, PathBuf};

use pdfcraft_engine::DocId;
use pdfcraft_engine::chess::DocKeys;
use pdfcraft_engine::chess::pdfcraft_chess::game::{Game, MAX_PGN_LEN, NodeId, NodePath, NodeSource, Token};
use serde::{Deserialize, Serialize};

use crate::chess_board_ui::{BoardState, Source};

/// The format's name, in every header.
const FORMAT: &str = "pdfcraft-chess-games";

/// The format's version. A newer version is only read; an older or unknown one is set aside.
pub(crate) const VERSION: u32 = 1;

/// Most games autosaved for one document.
pub(crate) const MAX_GAMES: usize = pdfcraft_engine::chess::collection::MAX_GAMES;

/// Largest body autosaved for one document.
pub(crate) const MAX_BODY_BYTES: usize = 32 << 20;

/// Longest header read.
const MAX_HEADER_BYTES: usize = 64 << 10;

/// Largest file read: a header and a body at their caps.
const MAX_FILE_BYTES: u64 = (MAX_BODY_BYTES + MAX_HEADER_BYTES + 1) as u64;

/// Most files looked at in the folder when a document opens.
const MAX_FILES: usize = 4096;

/// Longest path kept as a key (a longer one names the document by its other keys).
const MAX_PATH_LEN: usize = 4096;

/// The web build's `Storage` key, and its cap for all documents together.
pub const STORAGE_KEY: &str = "pdfcraft-chess-games-v1";
pub(crate) const WEB_MAX_BYTES: usize = 4 << 20;

/// The `Storage` key a web value that couldn't be read is kept under.
pub const STORAGE_BAD_KEY: &str = "pdfcraft-chess-games-v1.bad";

/// The first line of a file: enough to tell whose games these are.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Header {
    format: String,
    version: u32,
    #[serde(default)]
    keys: StoredKeys,
    /// The lock file name of the instance that wrote the file.
    #[serde(default)]
    instance: Option<String>,
    #[serde(default)]
    games: usize,
}

/// [`DocKeys`] as stored (the identifier in hex).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct StoredKeys {
    #[serde(default)]
    id0: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    size: Option<u64>,
}

impl StoredKeys {
    fn of(keys: &DocKeys) -> StoredKeys {
        StoredKeys {
            id0: keys.id0.map(|id| id.iter().map(|b| format!("{b:02x}")).collect()),
            path: keys.path.clone().filter(|p| p.len() <= MAX_PATH_LEN),
            name: keys.name.clone().filter(|n| n.len() <= MAX_PATH_LEN),
            size: keys.size,
        }
    }

    /// The keys read back; an identifier that isn't 32 hex digits is left out.
    fn keys(&self) -> DocKeys {
        let id0 = self.id0.as_deref().filter(|h| h.len() == 32 && h.is_ascii()).and_then(|h| {
            let mut id = [0u8; 16];
            for (i, b) in id.iter_mut().enumerate() {
                *b = u8::from_str_radix(h.get(i * 2..i * 2 + 2)?, 16).ok()?;
            }
            Some(id)
        });
        DocKeys { id0, path: self.path.clone(), name: self.name.clone(), size: self.size }
    }

    /// The keys of a newer version's header, each one taken when it has the type this version
    /// gives it (a newer version may give another key another type).
    fn lenient(value: &serde_json::Value) -> StoredKeys {
        let text = |key: &str| value.get(key).and_then(serde_json::Value::as_str).map(str::to_owned);
        StoredKeys { id0: text("id0"), path: text("path"), name: text("name"), size: value.get("size").and_then(serde_json::Value::as_u64) }
    }
}

/// The second line: the games.
#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct Body {
    #[serde(default)]
    pub active: usize,
    #[serde(default)]
    pub games: Vec<GameRecord>,
    /// The games as they were before moves left them (plan A8 §7, V5); older files have none.
    #[serde(default)]
    pub trash: Vec<crate::chess_trash::Trashed>,
}

/// One game as stored.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct GameRecord {
    #[serde(default)]
    pgn: String,
    /// The move shown.
    #[serde(default)]
    current: Vec<u16>,
    #[serde(default)]
    flipped: bool,
    /// A position that can't be played, as given (the game is then empty).
    #[serde(default)]
    illegal_fen: Option<String>,
    /// The diagram the game was opened from.
    #[serde(default)]
    origin: Option<StoredOrigin>,
    #[serde(default)]
    sources: Vec<StoredSource>,
    #[serde(default)]
    comments: Vec<StoredComments>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
struct StoredOrigin {
    page: usize,
    index: usize,
}

/// Where move `path` was read on a page.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredSource {
    path: Vec<u16>,
    page: u32,
    rect: [f32; 4],
}

/// Move `path`'s two comments, where PGN would put them elsewhere.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredComments {
    path: Vec<u16>,
    #[serde(default)]
    after: String,
    #[serde(default)]
    before: String,
}

/// Why a document's games were not autosaved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Blocked {
    /// More games than [`MAX_GAMES`], or a body over [`MAX_BODY_BYTES`] (or the web's cap), or a
    /// game too long to read back.
    TooBig,
    /// This instance holds no lock in the recovery folder.
    NoLock,
    /// Another running instance wrote the file.
    OtherInstance,
    /// A newer PdfCraft wrote the file (or the web's value), in this format version: this one
    /// only reads it, and never writes over it.
    Newer(u32),
    /// The file couldn't be written.
    Io(String),
}

/// What [`Store::write`] did with a document's games.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Written {
    /// They are in the store now (memory, the web's value).
    Now,
    /// The store's writer thread writes them (plan A9-I3b): what became of the write comes back
    /// from [`Store::take_written`] with this generation.
    Queued(u64),
}

/// What the writer thread writes: a document's file, whole.
pub(crate) struct FileJob {
    path: PathBuf,
    bytes: Vec<u8>,
    /// The instance writing it (its lock's name), which a file another live instance wrote is
    /// never written over by.
    instance: String,
}

/// The writer of a folder of games files.
type GamesWriter = crate::chess_writer::StoreWriter<FileJob, Result<(), Blocked>>;

/// Every move of `game`, the start first.
fn nodes(game: &Game) -> Vec<NodeId> {
    std::iter::once(0)
        .chain(game.tokens().into_iter().filter_map(|t| match t {
            Token::Move { node, .. } => Some(node),
            _ => None,
        }))
        .collect()
}

/// The longest comment the readers of the builds before plan A9-F2b read whole (R9-01).
const OLD_READER_COMMENT_BYTES: usize = 4096;

/// Whether the comment `text` comes back as it is from the PGN this build writes, read by this
/// build or an installed one: empty, or no longer than the old readers' cap, its words one space
/// apart (the writer joins them so) and no `[%` command (read back as a drawing). Decided from the
/// text alone, never by reading the PGN back (plan A9-I3a).
fn comes_back(text: &str) -> bool {
    text.is_empty() || (text.len() <= OLD_READER_COMMENT_BYTES && !text.contains("[%") && text.split(' ').eq(text.split_whitespace()))
}

/// `board`'s game as stored. `None` when its PGN is too long to be read back. The stores take it
/// from a [`RecordCache`].
#[cfg(test)]
pub(crate) fn record(board: &BoardState) -> Option<GameRecord> {
    let part = if board.illegal.is_some() { GameRecord::default() } else { game_part(&board.game)? };
    Some(with_board(part, board))
}

/// `part` (what [`game_part`] stores of the board's game) with what the board adds: the move
/// shown, the side the board is seen from, an illegal position and the diagram it came from.
fn with_board(mut part: GameRecord, board: &BoardState) -> GameRecord {
    part.origin = board.source.map(|s| StoredOrigin { page: s.page, index: s.index });
    part.flipped = board.flipped;
    if let Some((_, fen)) = &board.illegal {
        return GameRecord { illegal_fen: Some(fen.clone()), flipped: board.flipped, origin: part.origin, ..GameRecord::default() };
    }
    part.current = board.game.path_of(board.game.current()).map(|p| p.indices().to_vec()).unwrap_or_default();
    part
}

/// What a game's moves, comments, glyphs, tags and sources store: the PGN, where moves were read
/// and the comments PGN would not give back as they are. The same for every board showing the
/// game at one revision ([`Game::revision`], a value no other game has). `None` when its PGN is
/// too long to be read back.
fn game_part(game: &Game) -> Option<GameRecord> {
    let pgn = game.pgn();
    if pgn.len() > MAX_PGN_LEN {
        return None;
    }
    let mut sources = Vec::new();
    let mut comments = Vec::new();
    for node in nodes(game) {
        let Some(path) = game.path_of(node) else { continue };
        if let Some(s) = game.source(node) {
            sources.push(StoredSource { path: path.indices().to_vec(), page: s.page, rect: s.rect_user });
        }
        // Stored whenever the PGN may not give them back as they are, to this build or to the
        // installed ones (plan A9-I3a, R9-01): every comment before a move, and a comment the
        // writer changes or an old reader cuts. Decided from the text: reading every game's PGN
        // back at every save cost the interface thread a whole reading per game (A5-02).
        let (after, before) = (game.comment(node), game.comment_before(node));
        if !before.is_empty() || !comes_back(after) || old_readers_move(game, node) {
            comments.push(StoredComments { path: path.indices().to_vec(), after: after.to_owned(), before: before.to_owned() });
        }
    }
    Some(GameRecord { pgn, sources, comments, ..GameRecord::default() })
}

/// Most collections whose records [`RecordCache`] keeps.
const MAX_CACHED_COLLECTIONS: usize = 64;

/// The games' records as the last write of each collection encoded them, by the game's revision
/// (plan A9-I3a, finding A5-02): a write encodes again only the games that changed since, the
/// others taken as they were.
#[derive(Default)]
pub(crate) struct RecordCache {
    by_cid: HashMap<String, HashMap<u64, Option<GameRecord>>>,
    /// Games encoded, not taken from the cache, so far (for tests).
    pub(crate) encoded: usize,
}

impl RecordCache {
    /// The records of collection `cid`'s `boards`; `None` when one is too long to be read back.
    /// Only those boards' games are kept for the collection's next write.
    fn records(&mut self, cid: &str, boards: &[&BoardState]) -> Option<Vec<GameRecord>> {
        let before = self.by_cid.remove(cid).unwrap_or_default();
        let mut now: HashMap<u64, Option<GameRecord>> = HashMap::with_capacity(boards.len());
        let mut out = Vec::with_capacity(boards.len());
        let mut whole = true;
        for board in boards {
            if board.illegal.is_some() {
                out.push(with_board(GameRecord::default(), board));
                continue;
            }
            let revision = board.game.revision();
            let part = match now.get(&revision).or_else(|| before.get(&revision)) {
                Some(part) => part.clone(),
                None => {
                    self.encoded = self.encoded.saturating_add(1);
                    game_part(&board.game)
                }
            };
            now.insert(revision, part.clone());
            match part {
                Some(part) => out.push(with_board(part, board)),
                None => whole = false,
            }
        }
        if self.by_cid.len() >= MAX_CACHED_COLLECTIONS {
            self.by_cid.clear();
        }
        self.by_cid.insert(cid.to_owned(), now);
        whole.then_some(out)
    }
}

/// Whether a build before plan A9-F2b reads move `node`'s comments elsewhere from what this
/// build writes, so they are stored whatever this build's reader makes of the PGN (R9-01): a
/// comment before a first reply, which this build writes after the variations of the move before
/// it (plan A9-F4), and the comment of that move before, into which those builds read it. The
/// installed builds put comments back in place only from what is stored.
fn old_readers_move(game: &Game, node: NodeId) -> bool {
    let before_first_reply = |n: NodeId| !game.comment_before(n).is_empty() && game.path_of(n).is_some_and(|p| p.indices().last() == Some(&0));
    before_first_reply(node) || game.replies(node).first().is_some_and(|&first| before_first_reply(first))
}

/// The board a stored game puts back, its diagram in document `doc`. `None` when it can't be
/// read.
pub(crate) fn restore(record: &GameRecord, doc: Option<DocId>) -> Option<BoardState> {
    let source = doc.zip(record.origin).map(|(doc, o)| Source { doc, page: o.page, index: o.index });
    let mut board = BoardState::default();
    if let Some(fen) = &record.illegal_fen {
        board.load_fen(fen, record.flipped, source);
        // Loading is an undo step (none when the FEN couldn't be read); a restored game starts
        // without history.
        let loaded = board.history.undo_edit().is_some();
        board.history = Default::default();
        return loaded.then_some(board);
    }
    let (mut game, _) = Game::from_pgn(&record.pgn).ok()?;
    // A stored comment past the cap (a file written by hand, or by a build with a larger cap) is
    // cut, and said on the board (plan A9-F3).
    let mut cut = false;
    for c in &record.comments {
        if let Some(node) = game.node_at(&NodePath::new(c.path.clone())) {
            cut |= game.set_comment(node, &c.after).is_err();
            cut |= game.set_comment_before(node, &c.before).is_err();
        }
    }
    if cut {
        board.message = Some(tl!("Only the first 65,536 bytes of a comment are kept.").to_owned());
    }
    for s in &record.sources {
        if let Some(node) = game.node_at(&NodePath::new(s.path.clone())) {
            game.set_source(node, Some(NodeSource { page: s.page, rect_user: s.rect }));
        }
    }
    if let Some(node) = game.node_at(&NodePath::new(record.current.clone())) {
        game.go_to_node(node);
    }
    board.game = game;
    board.flipped = record.flipped;
    board.source = source;
    board.refresh_fields(true);
    Some(board)
}

/// The first line and the second of a document's file. `Err(TooBig)` above the caps.
#[cfg(test)]
pub(crate) fn encode(
    keys: &DocKeys,
    active: usize,
    boards: &[&BoardState],
    trash: &[crate::chess_trash::Trashed],
    instance: Option<String>,
) -> Result<(Header, String), Blocked> {
    encode_cached(&mut RecordCache::default(), "", keys, active, boards, trash, instance)
}

/// [`encode`] for collection `cid`, its games' records taken from `cache` where they did not
/// change since its last write (plan A9-I3a).
fn encode_cached(
    cache: &mut RecordCache,
    cid: &str,
    keys: &DocKeys,
    active: usize,
    boards: &[&BoardState],
    trash: &[crate::chess_trash::Trashed],
    instance: Option<String>,
) -> Result<(Header, String), Blocked> {
    if boards.len() > MAX_GAMES {
        return Err(Blocked::TooBig);
    }
    let games = cache.records(cid, boards).ok_or(Blocked::TooBig)?;
    let body = serde_json::to_string(&Body { active, games, trash: trash.to_vec() }).map_err(|e| Blocked::Io(e.to_string()))?;
    if body.len() > MAX_BODY_BYTES {
        return Err(Blocked::TooBig);
    }
    Ok((Header { format: FORMAT.into(), version: VERSION, keys: StoredKeys::of(keys), instance, games: boards.len() }, body))
}

/// The games of a body, each with the document's diagrams; and how many couldn't be read.
pub(crate) fn decode(body: &Body, doc: Option<DocId>) -> (Vec<BoardState>, usize) {
    let mut boards = Vec::new();
    let mut lost = 0usize;
    for g in body.games.iter().take(MAX_GAMES) {
        match restore(g, doc) {
            Some(b) => boards.push(b),
            None => lost = lost.saturating_add(1),
        }
    }
    (boards, lost.saturating_add(body.games.len().saturating_sub(MAX_GAMES)))
}

/// The version a JSON value of this format says it has. `None` when it is not this format, or its
/// version is not one a `u32` holds.
fn version_of(value: &serde_json::Value) -> Option<u32> {
    if value.get("format").and_then(serde_json::Value::as_str) != Some(FORMAT) {
        return None;
    }
    value.get("version").and_then(serde_json::Value::as_u64).and_then(|v| u32::try_from(v).ok())
}

/// What a file's first line (or a web entry's header) is.
#[derive(Debug)]
enum Head {
    /// A header of this version.
    This(Header),
    /// A newer PdfCraft's, of that version: never set aside or written over, whosever it is. Its
    /// document's keys as far as this version can make them out.
    Newer(u32, DocKeys),
    /// Neither: set aside, never deleted.
    Bad,
}

impl Head {
    /// What the JSON `value` of a header is.
    fn of(value: serde_json::Value) -> Head {
        match version_of(&value) {
            Some(VERSION) => serde_json::from_value::<Header>(value).map_or(Head::Bad, Head::This),
            Some(v) if v > VERSION => Head::Newer(v, value.get("keys").map(StoredKeys::lenient).unwrap_or_default().keys()),
            _ => Head::Bad,
        }
    }

    /// What `header` (a web entry's, read already) is.
    fn of_header(header: &Header) -> Head {
        if header.format != FORMAT || header.version < VERSION {
            Head::Bad
        } else if header.version == VERSION {
            Head::This(header.clone())
        } else {
            Head::Newer(header.version, header.keys.keys())
        }
    }
}

/// What looking for a document's games found.
#[derive(Default)]
pub(crate) struct Found {
    /// The collection's id and its games.
    pub games: Option<(String, Body)>,
    /// Files (or web values) that couldn't be read and were set aside, by the name they were
    /// kept under.
    pub set_aside: Vec<String>,
    /// The games were written by a newer PdfCraft, in this format version: they are only read
    /// (writing them is refused with [`Blocked::Newer`]).
    pub newer: Option<u32>,
}

/// Where autosaved games live.
#[derive(Default)]
pub(crate) enum Store {
    /// Nowhere (no recovery folder, or a test without one).
    #[default]
    Off,
    Files(FileStore),
    Memory(MemoryStore),
}

impl Store {
    /// The games stored for the document named by `keys`, unless their id is `taken` (another
    /// open document has them already).
    pub(crate) fn find(&mut self, keys: &DocKeys, taken: &HashSet<String>) -> Found {
        match self {
            Store::Off => Found::default(),
            Store::Files(f) => f.find(keys, taken),
            Store::Memory(m) => m.find(keys, taken),
        }
    }

    /// Store a document's games under `cid`: in memory now, or, in a folder, by the store's
    /// writer thread ([`Written::Queued`]; plan A9-I3b). What can be told without the file (too
    /// many games, no lock) is said now.
    pub(crate) fn write(
        &mut self,
        cid: &str,
        keys: &DocKeys,
        active: usize,
        boards: &[&BoardState],
        trash: &[crate::chess_trash::Trashed],
    ) -> Result<Written, Blocked> {
        match self {
            Store::Off => Ok(Written::Now),
            Store::Files(f) => f.submit(cid, keys, active, boards, trash).map(Written::Queued),
            Store::Memory(m) => m.write(cid, keys, active, boards, trash).map(|()| Written::Now),
        }
    }

    /// What became of the writes [`Store::write`] queued, since the last call: each one's
    /// collection id, generation and result.
    pub(crate) fn take_written(&mut self) -> Vec<crate::chess_writer::Done<Result<(), Blocked>>> {
        match self {
            Store::Files(f) => f.writer.as_ref().map(GamesWriter::take_done).unwrap_or_default(),
            Store::Off | Store::Memory(_) => Vec::new(),
        }
    }

    /// Wait until the queued writes have ended, at most `timeout`. Whether they all have.
    pub(crate) fn settle(&self, timeout: std::time::Duration) -> bool {
        match self {
            Store::Files(f) => f.writer.as_ref().is_none_or(|w| w.settle(timeout)),
            Store::Off | Store::Memory(_) => true,
        }
    }

    /// Whether games are autosaved at all.
    pub(crate) fn is_on(&self) -> bool {
        !matches!(self, Store::Off)
    }
}

/// Whether `cid` is one of the ids this module makes: safe as a file name.
fn plain_cid(cid: &str) -> bool {
    !cid.is_empty() && cid.len() <= 64 && cid.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// The desktop's store: a folder of `<id>.games` files.
pub(crate) struct FileStore {
    dir: PathBuf,
    recovery: crate::RecoveryStore,
    /// Files that couldn't be read nor set aside: not looked at again this session.
    ignored: HashSet<PathBuf>,
    /// The games' records as last written (plan A9-I3a).
    records: RecordCache,
    /// The writer thread, made with the first write ([`FileStore::submit`]; plan A9-I3b).
    writer: Option<GamesWriter>,
}

impl FileStore {
    pub(crate) fn new(dir: PathBuf, recovery: crate::RecoveryStore) -> FileStore {
        FileStore { dir, recovery, ignored: HashSet::new(), records: RecordCache::default(), writer: None }
    }

    fn path(&self, cid: &str) -> PathBuf {
        self.dir.join(format!("{cid}.games"))
    }

    /// What the first line of the file at `path` is, read within the cap; `None` when there is no
    /// file. The size is checked after the version: a file of this version over the cap is
    /// [`Head::Bad`], a newer one is left alone whatever its size (a newer build may keep more).
    fn head(path: &Path) -> Option<Head> {
        let meta = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
            Err(_) => return Some(Head::Bad),
        };
        if !meta.is_file() {
            return Some(Head::Bad);
        }
        let Ok(file) = std::fs::File::open(path) else { return Some(Head::Bad) };
        let mut line = Vec::new();
        if std::io::BufReader::new(file.take(MAX_HEADER_BYTES as u64)).read_until(b'\n', &mut line).is_err() {
            return Some(Head::Bad);
        }
        let Some(value) = line.strip_suffix(b"\n").and_then(|l| serde_json::from_slice::<serde_json::Value>(l).ok()) else {
            return Some(Head::Bad);
        };
        Some(match Head::of(value) {
            Head::This(_) if meta.len() > MAX_FILE_BYTES => Head::Bad,
            head => head,
        })
    }

    /// The body of the file at `path`, read within the cap.
    fn body(path: &Path) -> Option<Body> {
        if std::fs::metadata(path).ok()?.len() > MAX_FILE_BYTES {
            return None;
        }
        let mut bytes = Vec::new();
        std::fs::File::open(path).ok()?.take(MAX_FILE_BYTES).read_to_end(&mut bytes).ok()?;
        let at = bytes.iter().position(|&b| b == b'\n')?;
        serde_json::from_slice(bytes.get(at.checked_add(1)?..)?).ok()
    }

    /// [`set_aside`], and a file that couldn't be renamed is not looked at again.
    fn set_aside(&mut self, path: &Path) -> Option<String> {
        let kept = set_aside(path);
        if kept.is_none() {
            self.ignored.insert(path.to_path_buf());
        }
        kept
    }

    fn find(&mut self, keys: &DocKeys, taken: &HashSet<String>) -> Found {
        let mut found = Found::default();
        if keys.is_empty() {
            return found;
        }
        let Ok(entries) = std::fs::read_dir(&self.dir) else { return found };
        let mut paths: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "games") && !self.ignored.contains(p))
            .take(MAX_FILES)
            .collect();
        // The same file first on every run.
        paths.sort();
        for path in paths {
            let Some(cid) = path.file_stem().and_then(|s| s.to_str()).filter(|s| plain_cid(s)).map(str::to_owned) else { continue };
            match FileStore::head(&path) {
                None => {}
                Some(Head::Bad) => found.set_aside.extend(self.set_aside(&path)),
                Some(Head::This(header)) => {
                    if taken.contains(&cid) || !header.keys.keys().matches(keys) {
                        continue;
                    }
                    match FileStore::body(&path) {
                        Some(body) => {
                            found.games = Some((cid, body));
                            return found;
                        }
                        None => found.set_aside.extend(self.set_aside(&path)),
                    }
                }
                // A newer PdfCraft's file is never touched; this document's is read, for reading
                // only (what this version can't make out of its body is left out, never lost).
                Some(Head::Newer(version, owner)) => {
                    if taken.contains(&cid) || !owner.matches(keys) {
                        continue;
                    }
                    found.games = Some((cid, FileStore::body(&path).unwrap_or_default()));
                    found.newer = Some(version);
                    return found;
                }
            }
        }
        found
    }

    /// The file a write of `cid`'s games makes: encoded here, from what changed (plan A9-I3a);
    /// what needs the file itself is [`commit`]'s.
    fn prepare(
        &mut self,
        cid: &str,
        keys: &DocKeys,
        active: usize,
        boards: &[&BoardState],
        trash: &[crate::chess_trash::Trashed],
    ) -> Result<FileJob, Blocked> {
        if !plain_cid(cid) {
            return Err(Blocked::Io(format!("bad collection id {cid:?}")));
        }
        let Some(instance) = self.recovery.instance() else { return Err(Blocked::NoLock) };
        let (header, body) = encode_cached(&mut self.records, cid, keys, active, boards, trash, Some(instance.clone()))?;
        let line = serde_json::to_string(&header).map_err(|e| Blocked::Io(e.to_string()))?;
        let mut bytes = Vec::with_capacity(line.len().saturating_add(body.len()).saturating_add(2));
        bytes.extend_from_slice(line.as_bytes());
        bytes.push(b'\n');
        bytes.extend_from_slice(body.as_bytes());
        bytes.push(b'\n');
        Ok(FileJob { path: self.path(cid), bytes, instance })
    }

    /// Write `cid`'s games now, on this thread (the tests; the writer thread is [`Self::submit`]).
    #[cfg(test)]
    fn write(
        &mut self,
        cid: &str,
        keys: &DocKeys,
        active: usize,
        boards: &[&BoardState],
        trash: &[crate::chess_trash::Trashed],
    ) -> Result<(), Blocked> {
        let job = self.prepare(cid, keys, active, boards, trash)?;
        commit(&self.recovery, &self.dir, job)
    }

    /// Have `cid`'s games written by the store's writer thread, which is made with the first
    /// write: the request's generation ([`Written::Queued`]). What can be told here (a bad id, no
    /// lock, too many games) is said now.
    fn submit(
        &mut self,
        cid: &str,
        keys: &DocKeys,
        active: usize,
        boards: &[&BoardState],
        trash: &[crate::chess_trash::Trashed],
    ) -> Result<u64, Blocked> {
        let job = self.prepare(cid, keys, active, boards, trash)?;
        let writer = self.writer.get_or_insert_with(|| {
            let (recovery, dir) = (self.recovery.clone(), self.dir.clone());
            crate::chess_writer::StoreWriter::new(
                "chess-games-writer",
                move |_cid: &str, job| commit(&recovery, &dir, job),
                |panic| Err(Blocked::Io(format!("the writer stopped: {panic}"))),
            )
        });
        Ok(writer.submit(cid, job))
    }
}

/// Write `job` into the folder `dir`, unless the file there is a newer PdfCraft's (only read here,
/// never written over) or another live instance's; a file that can't be read is set aside first,
/// never written over.
fn commit(recovery: &crate::RecoveryStore, dir: &Path, job: FileJob) -> Result<(), Blocked> {
    match FileStore::head(&job.path) {
        Some(Head::Newer(version, _)) => return Err(Blocked::Newer(version)),
        Some(Head::This(h)) => {
            if let Some(other) = h.instance.as_deref().filter(|w| *w != job.instance)
                && recovery.held_by_another_instance(other)
            {
                return Err(Blocked::OtherInstance);
            }
        }
        // What is there can't be read: kept aside, never written over.
        Some(Head::Bad) if set_aside(&job.path).is_none() => {
            return Err(Blocked::Io("the file there couldn't be set aside".into()));
        }
        None | Some(Head::Bad) => {}
    }
    std::fs::create_dir_all(dir).map_err(|e| Blocked::Io(e.to_string()))?;
    crate::editing::write_atomically(&job.path.to_string_lossy(), &job.bytes).map_err(|e| Blocked::Io(e.to_string()))
}

/// Set the file at `path` aside as `….bad` (or `….bad.1`, …), never over another one. The name it
/// was kept under, or `None` when it couldn't be renamed.
fn set_aside(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    for n in 0..100u32 {
        let bad = if n == 0 { format!("{name}.bad") } else { format!("{name}.bad.{n}") };
        let to = path.with_file_name(&bad);
        if to.exists() {
            continue;
        }
        if std::fs::rename(path, &to).is_ok() {
            log::warn!("chess games file {} couldn't be read; kept as {bad}", path.display());
            return Some(bad);
        }
        break;
    }
    log::warn!("chess games file {} couldn't be read nor set aside", path.display());
    None
}

/// One document's games in the web's value.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct MemEntry {
    cid: String,
    header: Header,
    body: String,
}

/// The web's whole value.
#[derive(Debug, Serialize, Deserialize)]
struct MemFile {
    format: String,
    version: u32,
    #[serde(default)]
    entries: Vec<MemEntry>,
}

/// The web's store: every document's games in one `Storage` value, written when eframe saves.
#[derive(Default)]
pub(crate) struct MemoryStore {
    entries: Vec<MemEntry>,
    /// Changed since the value was last taken.
    dirty: bool,
    /// What couldn't be read, to keep under [`STORAGE_BAD_KEY`] (the last one only: the browser's
    /// storage is small).
    bad: Option<String>,
    /// The value was written by a newer PdfCraft, in this format version: it is only read, and
    /// never written back (plan A9-G12).
    newer: Option<u32>,
    /// The games' records as last written (plan A9-I3a).
    pub(crate) records: RecordCache,
}

impl MemoryStore {
    /// The store a `Storage` value holds. A value that can't be read is kept aside (see
    /// [`Self::take_bad`]) and the store starts empty; whether that happened. A value a newer
    /// PdfCraft wrote is read as far as this version can make it out, and never written back.
    pub(crate) fn load(text: Option<&str>) -> (MemoryStore, bool) {
        let Some(text) = text.filter(|t| !t.is_empty()) else { return (MemoryStore::default(), false) };
        let mut value = (text.len() <= WEB_MAX_BYTES.saturating_mul(2)).then(|| serde_json::from_str::<serde_json::Value>(text).ok()).flatten();
        let version = value.as_ref().and_then(version_of);
        let store = match (value.as_mut(), version) {
            (Some(v), Some(VERSION)) => serde_json::from_value::<MemFile>(v.take())
                .ok()
                .map(|f| MemoryStore { entries: f.entries.into_iter().filter(|e| plain_cid(&e.cid)).collect(), ..MemoryStore::default() }),
            (Some(v), Some(newer)) if newer > VERSION => {
                // Each entry this version can make out; the others are left out, never lost, since
                // nothing is written back.
                let entries = match v.get_mut("entries").map(serde_json::Value::take) {
                    Some(serde_json::Value::Array(items)) => {
                        items.into_iter().filter_map(|e| serde_json::from_value::<MemEntry>(e).ok()).filter(|e| plain_cid(&e.cid)).collect()
                    }
                    _ => Vec::new(),
                };
                Some(MemoryStore { entries, newer: Some(newer), ..MemoryStore::default() })
            }
            _ => None,
        };
        match store {
            Some(store) => (store, false),
            None => (MemoryStore { bad: Some(text.chars().take(WEB_MAX_BYTES).collect()), ..MemoryStore::default() }, true),
        }
    }

    fn find(&mut self, keys: &DocKeys, taken: &HashSet<String>) -> Found {
        let mut found = Found::default();
        if keys.is_empty() {
            return found;
        }
        let mut i = 0;
        while let Some(e) = self.entries.get(i) {
            let mine = !taken.contains(&e.cid) && e.header.keys.keys().matches(keys);
            let head = match self.newer {
                // In a newer value, every entry is a newer PdfCraft's.
                Some(v) => Head::Newer(v, e.header.keys.keys()),
                None => Head::of_header(&e.header),
            };
            if let Head::Newer(version, _) = head {
                // Never set aside; this document's games are read, for reading only.
                if mine {
                    found.games = Some((e.cid.clone(), serde_json::from_str::<Body>(&e.body).unwrap_or_default()));
                    found.newer = Some(version);
                    return found;
                }
                i = i.saturating_add(1);
                continue;
            }
            let this = matches!(head, Head::This(_));
            if !this || mine {
                let entry = self.entries.remove(i);
                if this && let Ok(body) = serde_json::from_str::<Body>(&entry.body) {
                    // Kept (it is written back with the document's next change).
                    self.entries.insert(i, entry.clone());
                    found.games = Some((entry.cid, body));
                    return found;
                }
                self.bad = serde_json::to_string(&entry).ok();
                self.dirty = true;
                found.set_aside.push(STORAGE_BAD_KEY.to_owned());
                continue;
            }
            i = i.saturating_add(1);
        }
        found
    }

    fn write(
        &mut self,
        cid: &str,
        keys: &DocKeys,
        active: usize,
        boards: &[&BoardState],
        trash: &[crate::chess_trash::Trashed],
    ) -> Result<(), Blocked> {
        // A newer PdfCraft's value, or its entry for this collection, is only read here.
        let entry_newer = self.entries.iter().find(|e| e.cid == cid).and_then(|e| match Head::of_header(&e.header) {
            Head::Newer(v, _) => Some(v),
            _ => None,
        });
        if let Some(v) = self.newer.or(entry_newer) {
            return Err(Blocked::Newer(v));
        }
        let (header, body) = encode_cached(&mut self.records, cid, keys, active, boards, trash, None)?;
        let others: usize = self.entries.iter().filter(|e| e.cid != cid).map(|e| e.body.len().saturating_add(512)).sum();
        if others.saturating_add(body.len()) > WEB_MAX_BYTES {
            return Err(Blocked::TooBig);
        }
        let entry = MemEntry { cid: cid.to_owned(), header, body };
        match self.entries.iter_mut().find(|e| e.cid == cid) {
            Some(e) => *e = entry,
            None => self.entries.push(entry),
        }
        self.dirty = true;
        Ok(())
    }

    /// The value to store, when it changed since it was last taken.
    pub(crate) fn take_value(&mut self) -> Option<String> {
        if !std::mem::take(&mut self.dirty) || self.newer.is_some() {
            return None;
        }
        serde_json::to_string(&MemFile { format: FORMAT.into(), version: VERSION, entries: self.entries.clone() }).ok()
    }

    /// What couldn't be read, to keep under [`STORAGE_BAD_KEY`].
    pub(crate) fn take_bad(&mut self) -> Option<String> {
        self.bad.take()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::{Value, json};

    /// xorshift64*: deterministic choices for the fuzz-like tests of untrusted settings and files
    /// (here and in `chess_window`).
    pub(crate) struct Rng(pub u64);

    impl Rng {
        pub(crate) fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }

        pub(crate) fn below(&mut self, n: usize) -> usize {
            if n == 0 { 0 } else { (self.next() % n as u64) as usize }
        }
    }

    /// A random JSON value of at most `depth` levels: the numbers and strings that break parsers
    /// and casts (negative, huge, fractional, `u64::MAX`), and arrays and objects of them.
    pub(crate) fn random_value(rng: &mut Rng, depth: usize) -> Value {
        match rng.below(if depth == 0 { 6 } else { 10 }) {
            0 => Value::Null,
            1 => json!(rng.below(2) == 0),
            2 | 3 => random_number(rng),
            4 => json!(["", "board", "text", "diagrams", "Board", "1. e4 ((", "é", "x"][rng.below(8)]),
            5 => json!("9".repeat(rng.below(400))),
            // A pair of numbers, as sizes and positions are written.
            6 => json!([random_number(rng), random_number(rng)]),
            7 => Value::Array((0..rng.below(5)).map(|_| random_value(rng, depth - 1)).collect()),
            _ => {
                let names = ["detached", "size", "position", "tab", "pgn", "current", "comments", "path", "page", "rect", "active", "games"];
                Value::Object((0..rng.below(5)).map(|_| (names[rng.below(names.len())].to_owned(), random_value(rng, depth - 1))).collect())
            }
        }
    }

    /// A number that breaks casts and bounds: negative, huge, fractional, at a limit or just past it.
    fn random_number(rng: &mut Rng) -> Value {
        match rng.below(5) {
            0 => json!(rng.below(70_000)),
            1 => json!(-i64::try_from(rng.below(70_000)).unwrap_or(0)),
            2 => json!([0.0, -0.5, 1e-300, 1e300, f64::MAX, -f64::MAX, 16_384.0, 16_384.5, -16_385.0, 320.0, 319.9][rng.below(11)]),
            3 => json!(u64::MAX),
            _ => json!(i64::MIN),
        }
    }

    /// `v` with some of its values, at any depth, replaced by random ones: of any type, or (with
    /// `same_type`) of the type there, so the file still parses and its games are restored from
    /// values no autosave wrote (a path of 65,535s, a PGN that is not one, a page past the end).
    fn mutate_value(rng: &mut Rng, v: &mut Value, same_type: bool) {
        // A body has dozens of values: one in 24 keeps most files of the same types readable.
        if rng.below(if same_type { 24 } else { 8 }) == 0 {
            *v = match (&*v, same_type) {
                (Value::Number(_), true) => json!([0, 1, 2, 7, 255, 65_535][rng.below(6)]),
                (Value::String(_), true) => {
                    let pgn = [
                        "",
                        "1. e4 e5 (",
                        "{",
                        "*",
                        "1. e4 e5 2. Nf3 Nc6 3. Bb5 (3. Bc4 {x} Bc5) a6 $1 $220 --",
                        "K7/8/8/8/8/8/8/K6k w - - 0 1",
                        "é",
                    ];
                    match rng.below(9) {
                        7 => json!("x".repeat(5000)),
                        8 => json!("(".repeat(500)),
                        i => json!(pgn[i]),
                    }
                }
                (Value::Bool(_), true) => json!(rng.below(2) == 0),
                (Value::Array(items), true) if rng.below(2) == 0 => Value::Array(items.iter().cycle().take(rng.below(70)).cloned().collect()),
                (Value::Array(_), true) => json!(vec![65_535; rng.below(40)]),
                _ => random_value(rng, 2),
            };
            return;
        }
        match v {
            Value::Array(items) => items.iter_mut().for_each(|item| mutate_value(rng, item, same_type)),
            Value::Object(map) => map.values_mut().for_each(|item| mutate_value(rng, item, same_type)),
            _ => {}
        }
    }

    fn keys() -> DocKeys {
        DocKeys { id0: Some([9; 16]), path: Some("/b/book.pdf".into()), name: Some("book.pdf".into()), size: Some(1234) }
    }

    /// The keys of the frozen version 1 sample ([`GAMES_V1`]).
    pub(crate) fn v1_keys() -> DocKeys {
        keys()
    }

    /// The games of the frozen version 1 sample: an annotated game (a variation, a comment PGN
    /// would move, a move read on a page, the board turned), an illegal position opened from a
    /// diagram, and a game whose comment after a move and the one before the reply add up to
    /// more than the PGN reader keeps of one comment (R9-01).
    pub(crate) fn v1_sample_boards() -> Vec<BoardState> {
        let mut illegal = BoardState::default();
        illegal.load_fen("K7/8/8/8/8/8/8/K6k w - - 0 1", false, Some(Source { doc: DocId(7), page: 3, index: 1 }));
        illegal.history = Default::default();
        let mut long = BoardState::default();
        long.game.play_uci("e2e4").unwrap();
        let e4 = long.game.current();
        long.game.play_uci("e7e5").unwrap();
        let e5 = long.game.current();
        long.game.set_comment(e4, &"a".repeat(3000)).unwrap();
        long.game.set_comment_before(e5, &"b".repeat(2500)).unwrap();
        vec![annotated(), illegal, long]
    }

    /// The deleted moves of the frozen version 1 sample.
    pub(crate) fn v1_sample_trash() -> Vec<crate::chess_trash::Trashed> {
        vec![crate::chess_trash::Trashed { pgn: "1. e4 e5 2. Nf3 *".into(), name: "Game 1".into(), moves: 3, deleted: 1, at: 7 }]
    }

    /// A game with a variation, a comment before a first reply (which PGN wrote with the comment
    /// after the move before it, and writes after that move's variations since plan A9-F4), a
    /// comment before a variation's first move, glyphs, tags, sources and a move shown inside the
    /// variation.
    fn annotated() -> BoardState {
        let mut b = BoardState::default();
        let g = &mut b.game;
        for m in ["e2e4", "e7e5", "g1f3"] {
            g.play_uci(m).unwrap();
        }
        let nf3 = g.current();
        g.go_to(2);
        let e5 = g.current();
        g.go_to(1);
        g.play_uci("c7c5").unwrap();
        let c5 = g.current();
        g.set_comment(e5, "after e5").unwrap();
        // Nf3 is e5's first reply, after e5's variation: PGN writes it after the `)`.
        g.set_comment_before(nf3, "before Nf3").unwrap();
        // c5 starts a variation: PGN has room for this one, after the "(".
        g.set_comment_before(c5, "before c5").unwrap();
        g.toggle_nag(nf3, 1);
        g.set_tag("White", "Werle").unwrap();
        g.set_tag("Black", "Jianu").unwrap();
        g.set_source(nf3, Some(NodeSource { page: 4, rect_user: [10.0, 20.0, 30.0, 40.0] }));
        b.flipped = true;
        b
    }

    fn same(a: &BoardState, b: &BoardState) {
        assert_eq!(a.game.pgn(), b.game.pgn());
        assert_eq!(a.game.path_of(a.game.current()), b.game.path_of(b.game.current()));
        assert_eq!(a.flipped, b.flipped);
        for node in nodes(&a.game) {
            let path = a.game.path_of(node).unwrap();
            let other = b.game.node_at(&path).unwrap();
            assert_eq!(a.game.comment(node), b.game.comment(other), "{path:?}");
            assert_eq!(a.game.comment_before(node), b.game.comment_before(other), "{path:?}");
            assert_eq!(a.game.source(node), b.game.source(other), "{path:?}");
            assert_eq!(a.game.nags(node), b.game.nags(other), "{path:?}");
        }
    }

    #[test]
    fn a_game_comes_back_as_it_was_with_what_pgn_cannot_hold() {
        let board = annotated();
        let rec = record(&board).unwrap();
        // PGN keeps the comment before Nf3 in its place since plan A9-F4; it is stored all the same,
        // with e5's, into which a build before plan A9-F2b reads it (R9-01). Every comment before a
        // move is stored, decided without reading the PGN back (plan A9-I3a): c5's too.
        let (plain, _) = Game::from_pgn(&rec.pgn).unwrap();
        let nf3 = plain.node_at(&NodePath::new(vec![0, 0, 0])).unwrap();
        assert_eq!(plain.comment_before(nf3), "before Nf3");
        let stored: Vec<(&[u16], &str, &str)> = rec.comments.iter().map(|c| (c.path.as_slice(), c.after.as_str(), c.before.as_str())).collect();
        assert_eq!(stored, [(&[0u16, 0][..], "after e5", ""), (&[0u16, 1][..], "", "before c5"), (&[0u16, 0, 0][..], "", "before Nf3")]);
        let text = serde_json::to_string(&rec).unwrap();
        let back = restore(&serde_json::from_str(&text).unwrap(), None).unwrap();
        same(&board, &back);
        assert_eq!(back.game.comment_before(back.game.node_at(&NodePath::new(vec![0, 0, 0])).unwrap()), "before Nf3");
        assert!(back.history.undo_edit().is_none(), "a restored game has no history");
    }

    #[test]
    fn an_illegal_position_and_the_diagram_come_back() {
        let mut b = BoardState::default();
        let doc = DocId(7);
        b.load_fen("K7/8/8/8/8/8/8/K6k w - - 0 1", false, Some(Source { doc, page: 3, index: 1 }));
        let back = restore(&record(&b).unwrap(), Some(DocId(9))).unwrap();
        assert_eq!(back.illegal.as_ref().map(|(_, f)| f.as_str()), Some("K7/8/8/8/8/8/8/K6k w - - 0 1"));
        assert_eq!(back.source, Some(Source { doc: DocId(9), page: 3, index: 1 }), "the diagram, in the document open now");
        assert!(back.history.undo_edit().is_none());
    }

    #[test]
    fn caps_hold_and_bad_records_are_counted() {
        let one = BoardState::default();
        let many: Vec<&BoardState> = std::iter::repeat_n(&one, MAX_GAMES + 1).collect();
        assert_eq!(encode(&keys(), 0, &many, &[], None).err(), Some(Blocked::TooBig));
        assert!(encode(&keys(), 0, &many[..MAX_GAMES], &[], None).is_ok());
        let body = Body {
            active: 0,
            games: vec![
                GameRecord { pgn: "1. e4 e5".into(), ..GameRecord::default() },
                GameRecord { illegal_fen: Some("junk".into()), ..GameRecord::default() },
            ],
            trash: Vec::new(),
        };
        let (boards, lost) = decode(&body, None);
        assert_eq!((boards.len(), lost), (1, 1));
    }

    #[test]
    fn stored_keys_round_trip_and_bad_ids_are_dropped() {
        let k = keys();
        assert_eq!(StoredKeys::of(&k).keys(), k);
        let bad = StoredKeys { id0: Some("zz".repeat(16)), ..StoredKeys::default() };
        assert_eq!(bad.keys().id0, None);
        let short = StoredKeys { id0: Some("ab".into()), ..StoredKeys::default() };
        assert_eq!(short.keys().id0, None);
        // Multi-byte text of the right length is not hex either, and does not panic.
        let wide = StoredKeys { id0: Some("é".repeat(16)), ..StoredKeys::default() };
        assert_eq!(wide.keys().id0, None);
    }

    #[test]
    fn the_web_store_round_trips_caps_and_sets_bad_values_aside() {
        let (mut m, bad) = MemoryStore::load(None);
        assert!(!bad);
        let board = annotated();
        let trash = vec![crate::chess_trash::Trashed { pgn: "1. e4 e5 *".into(), name: "Game 1".into(), moves: 2, deleted: 1, at: 7 }];
        m.write("abc-1", &keys(), 0, &[&board], &trash).unwrap();
        let value = m.take_value().unwrap();
        assert_eq!(m.take_value(), None, "taken once per change");
        let (mut m, bad) = MemoryStore::load(Some(&value));
        assert!(!bad);
        let found = m.find(&keys(), &HashSet::new());
        let (cid, body) = found.games.unwrap();
        assert_eq!(cid, "abc-1");
        same(&board, &decode(&body, None).0[0]);
        assert_eq!(body.trash, trash, "the deleted moves kept come back too");
        // Taken by another open document: not found again.
        assert!(m.find(&keys(), &HashSet::from(["abc-1".to_owned()])).games.is_none());
        // Over the web's cap: refused, nothing evicted.
        // 256 games of about 20 KB each: 5 MB, over the web's 4 MiB (and under the desktop's cap).
        let mut big = BoardState::default();
        for m in ["e2e4", "e7e5", "g1f3", "b8c6", "f1b5"] {
            big.game.play_uci(m).unwrap();
            big.game.set_comment(big.game.current(), &"x".repeat(4000)).unwrap();
        }
        let mut boards = Vec::new();
        for _ in 0..MAX_GAMES {
            boards.push(&big);
        }
        assert_eq!(m.write("abc-2", &keys(), 0, &boards, &[]), Err(Blocked::TooBig));
        assert!(m.find(&keys(), &HashSet::new()).games.is_some(), "the first document's games are still there");
        // A value that can't be read is kept aside and the store starts empty (a newer version's
        // is only read: see the next test).
        for junk in ["{not json", "{\"format\":\"pdfcraft-chess-games\",\"version\":0,\"entries\":[]}"] {
            let (mut m, bad) = MemoryStore::load(Some(junk));
            assert!(bad);
            assert_eq!(m.take_bad().as_deref(), Some(junk));
        }
    }

    /// A store in a fresh folder of its own, under a claimed recovery lock, holding a valid v1
    /// file of two games; the file's path and bytes.
    fn store_with_a_file(test: &str) -> (PathBuf, FileStore, PathBuf, Vec<u8>) {
        let dir = std::env::temp_dir().join(format!("pdfcraft-chess-games-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let recovery = crate::RecoveryStore::new(dir.join("recovery"));
        recovery.claim();
        let mut store = FileStore::new(dir.join("chess"), recovery);
        store.write("abc-1", &keys(), 1, &[&annotated(), &BoardState::default()], &[]).unwrap();
        let path = store.path("abc-1");
        let valid = std::fs::read(&path).unwrap();
        (dir, store, path, valid)
    }

    /// Put `bytes` in place of the games file and look for the document's games: whatever they
    /// are, nothing panics, and a file that is not read is set aside as `.bad` (renamed, never
    /// deleted) unless it names another document. The folder is left with no file.
    fn find_after_writing(store: &mut FileStore, path: &Path, bytes: &[u8]) -> Found {
        std::fs::write(path, bytes).unwrap();
        let found = store.find(&keys(), &HashSet::new());
        let bad = path.with_file_name("abc-1.games.bad");
        if let Some((_, body)) = &found.games {
            let _ = decode(body, Some(DocId(3)));
            assert!(found.set_aside.is_empty());
        } else if found.set_aside.is_empty() {
            // Read and not this document's: left where it was.
            assert!(path.exists() && !bad.exists());
        } else {
            assert_eq!(found.set_aside, ["abc-1.games.bad"]);
            assert_eq!(std::fs::read(&bad).unwrap(), bytes, "kept as it was");
            assert!(!path.exists());
        }
        assert!(store.ignored.is_empty(), "every file could be set aside");
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(&bad);
        found
    }

    /// PGN toolbar T16: every truncation of a valid file is set aside as `.bad` and said, except
    /// the one that only loses the last line break, which reads as before.
    #[test]
    fn every_truncation_of_a_games_file_is_set_aside() {
        let (dir, mut store, path, valid) = store_with_a_file("truncated");
        let whole = find_after_writing(&mut store, &path, &valid);
        assert_eq!(whole.games.map(|(cid, body)| (cid, body.games.len(), body.active)), Some(("abc-1".to_owned(), 2, 1)));
        let header_end = valid.iter().position(|&b| b == b'\n').unwrap();
        let step = (valid.len() / 300).max(1);
        let cuts = (0..valid.len() - 1).step_by(step).chain([header_end, header_end + 1, valid.len() - 2]);
        for cut in cuts {
            let found = find_after_writing(&mut store, &path, &valid[..cut]);
            assert!(found.games.is_none() && !found.set_aside.is_empty(), "a file cut at {cut} of {} was not set aside", valid.len());
        }
        assert!(find_after_writing(&mut store, &path, &valid[..valid.len() - 1]).games.is_some(), "only the last line break gone");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// PGN toolbar T16: random bytes, bytes of a valid file flipped, spliced or dropped, and valid
    /// JSON with random values in place of any of the stored ones (paths of 65,535, huge pages,
    /// a PGN that is not one) never panic, in the file store and in the web's; what is not read
    /// is set aside.
    #[test]
    fn mutated_games_files_never_panic() {
        let (dir, mut store, path, valid) = store_with_a_file("mutated");
        let mut rng = Rng(0x7E16_A5A5);
        let header_end = valid.iter().position(|&b| b == b'\n').unwrap();
        let header: Value = serde_json::from_slice(&valid[..header_end]).unwrap();
        let body: Value = serde_json::from_slice(&valid[header_end + 1..]).unwrap();
        let (mut read, mut set_aside) = (0usize, 0usize);
        for round in 0..400 {
            let bytes: Vec<u8> = match round % 4 {
                // Random bytes, sometimes with a line break.
                0 => (0..rng.below(300)).map(|_| if rng.below(40) == 0 { b'\n' } else { rng.next() as u8 }).collect(),
                // The valid file with bytes changed.
                1 => {
                    let mut b = valid.clone();
                    for _ in 0..1 + rng.below(6) {
                        let at = rng.below(b.len());
                        match rng.below(3) {
                            0 => b[at] = rng.next() as u8,
                            1 => {
                                b.remove(at);
                            }
                            _ => b.insert(at, b"{}[]\",:0\n\\"[rng.below(10)]),
                        }
                    }
                    b
                }
                // The body's values changed to others of their type, the header kept: the games
                // are read and restored.
                2 => {
                    let mut body = body.clone();
                    mutate_value(&mut rng, &mut body, true);
                    format!("{header}\n{body}\n").into_bytes()
                }
                // Values of any type, in the header too.
                _ => {
                    let (mut body, mut header) = (body.clone(), header.clone());
                    mutate_value(&mut rng, &mut body, false);
                    mutate_value(&mut rng, &mut header, false);
                    format!("{header}\n{body}\n").into_bytes()
                }
            };
            let found = find_after_writing(&mut store, &path, &bytes);
            read += usize::from(found.games.is_some());
            set_aside += usize::from(!found.set_aside.is_empty());
            // The web's store takes the same bytes as its whole value, and their second line as a
            // document's games.
            let text = String::from_utf8_lossy(&bytes);
            let (mut m, _) = MemoryStore::load(Some(&text));
            let _ = m.find(&keys(), &HashSet::new());
            let games = text.split_once('\n').map_or("", |(_, games)| games);
            let wrapped =
                json!({ "format": FORMAT, "version": VERSION, "entries": [{ "cid": "abc-1", "header": header, "body": games }] }).to_string();
            let (mut m, bad) = MemoryStore::load(Some(&wrapped));
            assert!(!bad);
            if let Some((_, body)) = m.find(&keys(), &HashSet::new()).games {
                let _ = decode(&body, None);
            }
        }
        assert!(read > 50 && set_aside > 50, "both ways were taken: {read} read, {set_aside} set aside");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The version 1 format as the installed builds (≤ cc2d484c) write it: written by this module
    /// before plan A9-G12 changed anything (the sample games, keys and deleted moves above, the
    /// lock's name made fixed) and frozen since.
    const GAMES_V1: &[u8] = include_bytes!("../tests/compat/games_v1.games");

    /// A file's two lines as JSON, the lock name of the instance that wrote it left out.
    fn v1_lines(bytes: &[u8]) -> (Value, Value) {
        let at = bytes.iter().position(|&b| b == b'\n').unwrap();
        assert_eq!(bytes.iter().filter(|&&b| b == b'\n').count(), 2, "two lines, each ending in a line break");
        let mut header: Value = serde_json::from_slice(&bytes[..at]).unwrap();
        header["instance"] = Value::Null;
        (header, serde_json::from_slice(&bytes[at + 1..]).unwrap())
    }

    /// Every file in `dir`, by name, with its bytes.
    fn files_in(dir: &Path) -> Vec<(String, Vec<u8>)> {
        let mut files: Vec<(String, Vec<u8>)> = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| (e.file_name().to_string_lossy().into_owned(), std::fs::read(e.path()).unwrap()))
            .collect();
        files.sort();
        files
    }

    /// Plan A9 §2.10 and G12: the version 1 sample reads back whole (the comments PGN can't place
    /// included), and what this build writes for the same games is the same JSON. The installed
    /// builds read this format, and put comments back in place only through its `comments`
    /// (`StoredComments`, R9-01), so a change here needs a new version and a new file name.
    #[test]
    fn the_version_1_sample_reads_back_and_is_what_this_build_writes() {
        let dir = std::env::temp_dir().join(format!("pdfcraft-chess-games-v1-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let recovery = crate::RecoveryStore::new(dir.join("recovery"));
        recovery.claim();
        let folder = dir.join("chess");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("abc-1.games"), GAMES_V1).unwrap();
        let mut store = FileStore::new(folder.clone(), recovery);
        let found = store.find(&v1_keys(), &HashSet::new());
        assert!(found.set_aside.is_empty());
        let (cid, body) = found.games.expect("the sample is read");
        assert_eq!((cid.as_str(), body.active), ("abc-1", 1));
        assert_eq!(body.trash, v1_sample_trash());
        let (boards, lost) = decode(&body, Some(DocId(7)));
        assert_eq!(lost, 0);
        let sample = v1_sample_boards();
        assert_eq!(boards.len(), sample.len());
        for (a, b) in sample.iter().zip(&boards) {
            same(a, b);
            assert_eq!(a.illegal.as_ref().map(|(_, f)| f), b.illegal.as_ref().map(|(_, f)| f));
            assert_eq!(a.source, b.source);
        }
        // The comments PGN merges come back apart, the long pair whole.
        let long = &boards[2].game;
        let e5 = long.node_at(&NodePath::new(vec![0, 0])).unwrap();
        assert_eq!(long.comment_before(e5), "b".repeat(2500));
        // What this build writes for the same games.
        let refs: Vec<&BoardState> = sample.iter().collect();
        store.write("abc-2", &v1_keys(), 1, &refs, &v1_sample_trash()).unwrap();
        let written = std::fs::read(store.path("abc-2")).unwrap();
        // The version 1 format, unchanged; the PGN in it is the one writer's since plan A9-F4
        // (wrapped, a comment before a move after the variations of the move before), and reads
        // back, with the comments stored apart, to the same games. Every comment the sample stores
        // apart is stored still; since plan A9-I3a, decided without reading the PGN back, every
        // comment before a move is too.
        let without_pgn = |(header, mut body): (Value, Value)| {
            for g in body["games"].as_array_mut().into_iter().flatten() {
                g["pgn"] = Value::Null;
                g["comments"] = Value::Null;
            }
            (header, body)
        };
        assert_eq!(without_pgn(v1_lines(&written)), without_pgn(v1_lines(GAMES_V1)), "the version 1 format, unchanged");
        let comments = |(_, body): (Value, Value)| -> Vec<Value> {
            body["games"].as_array().into_iter().flatten().flat_map(|g| g["comments"].as_array().cloned().unwrap_or_default()).collect()
        };
        let now = comments(v1_lines(&written));
        for c in comments(v1_lines(GAMES_V1)) {
            assert!(now.contains(&c), "still stored: {c}");
        }
        let (_, written_body) = v1_lines(&written);
        let (again, lost) = decode(&serde_json::from_value(written_body).unwrap(), Some(DocId(7)));
        assert_eq!((again.len(), lost), (sample.len(), 0));
        for (a, b) in sample.iter().zip(&again) {
            same(a, b);
        }
        let (_, body) = v1_lines(GAMES_V1);
        let stored: Vec<&Value> = body["games"].as_array().unwrap().iter().flat_map(|g| g["comments"].as_array().unwrap()).collect();
        assert_eq!(stored.len(), 4, "every comment PGN would move is stored apart (R9-01)");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The first line of a games file a newer PdfCraft might write for the document `keys`, and
    /// a body with fields this version doesn't know.
    fn newer_file(keys: &DocKeys, pgn: &str) -> String {
        let keys = serde_json::to_value(StoredKeys::of(keys)).unwrap();
        let header = json!({ "format": FORMAT, "version": VERSION + 1, "keys": keys, "instance": "instance-1-0.lock", "games": 1, "uids": true });
        let body = json!({ "active": 0, "games": [{ "pgn": pgn, "uid": 7, "rev": 3 }], "tombstones": [] });
        format!("{header}\n{body}\n")
    }

    /// Plan A9-G12 (R3-05): a folder with a version 1 file of another book, the name a later
    /// format takes (`<hash>.games.json`), and `.games` files a newer PdfCraft wrote, one for
    /// another book and one for this one. Looking for this book's games renames, sets aside and
    /// writes nothing; this book's newer file is read.
    #[test]
    fn a_newer_games_file_is_only_read_and_no_other_books_file_is_touched() {
        let dir = std::env::temp_dir().join(format!("pdfcraft-chess-games-newer-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let recovery = crate::RecoveryStore::new(dir.join("recovery"));
        recovery.claim();
        let folder = dir.join("chess");
        let mut store = FileStore::new(folder.clone(), recovery);
        let other = DocKeys { id0: Some([5; 16]), path: Some("/b/other.pdf".into()), name: Some("other.pdf".into()), size: Some(99) };
        store.write("bb-1", &other, 0, &[&annotated()], &[]).unwrap();
        std::fs::write(folder.join("cc-2.games"), newer_file(&other, "1. d4 d5 *")).unwrap();
        std::fs::write(folder.join("dd-3.games"), newer_file(&keys(), "1. c4 e5 *")).unwrap();
        std::fs::write(folder.join("0123456789abcdef.games.json"), r#"{"format":"pdfcraft-chess-games","version":2,"games":[]}"#).unwrap();
        let before = files_in(&folder);
        let found = store.find(&keys(), &HashSet::new());
        assert!(found.set_aside.is_empty(), "nothing set aside: {:?}", found.set_aside);
        assert_eq!(files_in(&folder), before, "every file as it was, under its own name");
        let (cid, body) = found.games.expect("this book's games, read from the newer file");
        assert_eq!(cid, "dd-3");
        assert_eq!(decode(&body, None).0.iter().map(|b| b.game.movetext()).collect::<Vec<_>>(), ["1. c4 e5"]);
        assert_eq!(found.newer, Some(VERSION + 1), "for reading only");
        // Writing them is refused, and the file stays as it was.
        assert_eq!(store.write("dd-3", &keys(), 0, &[&annotated()], &[]), Err(Blocked::Newer(VERSION + 1)));
        // Another book looked for: the same, and its own version 1 file is found.
        let found = store.find(&other, &HashSet::new());
        assert_eq!(found.games.map(|(cid, _)| cid).as_deref(), Some("bb-1"));
        assert!(found.set_aside.is_empty() && found.newer.is_none());
        assert_eq!(files_in(&folder), before, "nothing renamed or written over");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Plan A9-G12 on the web: a value a newer PdfCraft wrote is read and never written back, and
    /// in a value of this version a newer entry of another document is left as it is.
    #[test]
    fn the_web_store_reads_a_newer_value_and_never_writes_it() {
        let newer_entry = |keys: &DocKeys, cid: &str| {
            let text = newer_file(keys, "1. c4 e5 *");
            let (header, body) = text.trim_end().split_once('\n').unwrap();
            json!({ "cid": cid, "header": serde_json::from_str::<Value>(header).unwrap(), "body": body, "more": 1 })
        };
        let value =
            json!({ "format": FORMAT, "version": VERSION + 1, "entries": [newer_entry(&keys(), "dd-3"), { "cid": "x", "odd": true }] }).to_string();
        let (mut m, bad) = MemoryStore::load(Some(&value));
        assert!(!bad, "a newer value is not set aside");
        let found = m.find(&keys(), &HashSet::new());
        assert_eq!(found.newer, Some(VERSION + 1));
        let (cid, body) = found.games.unwrap();
        assert_eq!((cid.as_str(), decode(&body, None).0[0].game.movetext().as_str()), ("dd-3", "1. c4 e5"));
        assert_eq!(m.write("dd-3", &keys(), 0, &[&annotated()], &[]), Err(Blocked::Newer(VERSION + 1)));
        assert_eq!(m.write("new-1", &keys(), 0, &[&annotated()], &[]), Err(Blocked::Newer(VERSION + 1)), "nothing is added either");
        assert_eq!((m.take_value(), m.take_bad()), (None, None), "never written back");
        // A value of this version with a newer entry of another document, and one of this one's.
        let other = DocKeys { id0: Some([5; 16]), path: Some("/b/other.pdf".into()), name: Some("other.pdf".into()), size: Some(99) };
        let (mut m, _) = MemoryStore::load(None);
        m.write("abc-1", &keys(), 0, &[&annotated()], &[]).unwrap();
        let mut value: Value = serde_json::from_str(&m.take_value().unwrap()).unwrap();
        value["entries"].as_array_mut().unwrap().insert(0, newer_entry(&other, "cc-2"));
        let (mut m, bad) = MemoryStore::load(Some(&value.to_string()));
        assert!(!bad);
        let found = m.find(&keys(), &HashSet::new());
        assert!(found.set_aside.is_empty() && found.newer.is_none());
        assert_eq!(found.games.map(|(cid, _)| cid).as_deref(), Some("abc-1"));
        assert_eq!(m.take_bad(), None);
        assert!(m.find(&other, &HashSet::new()).newer.is_some(), "the other document's newer games are still there");
        assert_eq!(m.write("cc-2", &other, 0, &[&annotated()], &[]), Err(Blocked::Newer(VERSION + 1)));
        m.write("abc-1", &keys(), 0, &[&BoardState::default()], &[]).unwrap();
        let written: Value = serde_json::from_str(&m.take_value().unwrap()).unwrap();
        assert_eq!(written["entries"][0]["header"]["version"], json!(VERSION + 1), "kept as it was when the value is written");
    }

    /// 120 games of 80 plies, with a comment every eighth move and a variation every twentieth,
    /// as a book's games are (plan A9-I3a).
    fn book_games() -> Vec<BoardState> {
        (0..120usize)
            .map(|g| {
                let mut b = BoardState::default();
                for ply in 0..80usize {
                    let legal = b.game.legal_moves();
                    let Some(pick) = legal.get((ply * 7 + g) % legal.len().max(1)).cloned() else { break };
                    let here = b.game.current();
                    let _ = b.game.play_san(&pick);
                    let next = b.game.current();
                    // A variation beside the move, the main line going on from the move.
                    if ply % 20 == 10
                        && let Some(other) = legal.get((ply * 7 + g + 1) % legal.len()).filter(|o| **o != pick).cloned()
                    {
                        b.game.go_to_node(here);
                        let _ = b.game.play_san(&other);
                        b.game.go_to_node(next);
                    }
                    if ply % 8 == 3 {
                        let n = b.game.current();
                        let _ = b.game.set_comment(n, "A quiet move that keeps the balance.");
                    }
                }
                b
            })
            .collect()
    }

    /// Plan A9-I3a (A5-02): the autosave of a document's 120 games takes at most 5 ms of the
    /// interface thread (the best of five, in the tests' optimised build). It read every game's
    /// PGN back at every save. Timing: `cargo test -p pdfcraft-ui-egui --lib -- --ignored perf_`.
    #[test]
    #[ignore = "timing (plan A9-I3a): run with --ignored perf_"]
    fn perf_the_autosave_of_120_games_takes_at_most_5_ms() {
        let boards = book_games();
        let moves: usize = boards.iter().map(|b| b.game.main_line().len()).sum();
        assert!(moves > 120 * 60, "long games: {moves} moves");
        let mut boards = boards;
        // The web's store (no disk): what a save costs the frame is the encoding. The first write
        // encodes every game; then one game changes before each write.
        let (mut m, _) = MemoryStore::load(None);
        let refs: Vec<&BoardState> = boards.iter().collect();
        m.write("perf-1", &keys(), 0, &refs, &[]).unwrap();
        let mut best = f64::MAX;
        for round in 0..5usize {
            let b = &mut boards[round * 7];
            let n = b.game.current();
            let _ = b.game.set_comment(n, &format!("Changed {round}."));
            let refs: Vec<&BoardState> = boards.iter().collect();
            let started = std::time::Instant::now();
            m.write("perf-1", &keys(), 0, &refs, &[]).unwrap();
            best = best.min(started.elapsed().as_secs_f64() * 1000.0);
        }
        eprintln!("autosave of 120 games ({moves} moves), one changed: {best:.2} ms");
        assert!(best <= 5.0, "{best:.2} ms");
    }

    /// Plan A9-I3a (A5-02): a write encodes again only the games that changed since the
    /// collection's last write; the others' records are taken as they were, and the file says the
    /// same as when every game is encoded.
    #[test]
    fn a_write_encodes_only_the_games_that_changed() {
        let mut boards = book_games();
        let (mut m, _) = MemoryStore::load(None);
        let refs: Vec<&BoardState> = boards.iter().collect();
        m.write("c-1", &keys(), 0, &refs, &[]).unwrap();
        assert_eq!(m.records.encoded, 120);
        let b = &mut boards[5];
        let n = b.game.current();
        b.game.set_comment(n, "A new comment.").unwrap();
        let refs: Vec<&BoardState> = boards.iter().collect();
        m.write("c-1", &keys(), 0, &refs, &[]).unwrap();
        assert_eq!(m.records.encoded, 121, "one of 120 encoded again");
        // Moving through a game or flipping its board is no new encoding, and is written.
        boards[9].flipped = true;
        let refs: Vec<&BoardState> = boards.iter().collect();
        m.write("c-1", &keys(), 0, &refs, &[]).unwrap();
        assert_eq!(m.records.encoded, 121);
        let cached = m.find(&keys(), &HashSet::new()).games.unwrap().1;
        let fresh = encode(&keys(), 0, &boards.iter().collect::<Vec<_>>(), &[], None).unwrap().1;
        assert_eq!(serde_json::to_value(&cached).unwrap(), serde_json::from_str::<Value>(&fresh).unwrap());
    }

    /// Plan A9-I3a: without reading the PGN back, every comment that PGN would not give back as it
    /// is, to this build or an installed one, is stored beside it: one with a line break or double
    /// spaces, one longer than the old readers' 4 KiB, a `[%clk]` command, every comment before a
    /// move. A plain comment is not.
    #[test]
    fn comments_the_pgn_changes_come_back_without_reading_it_back() {
        let mut b = BoardState::default();
        let long = "word ".repeat(1000).trim_end().to_owned();
        let said = ["a plain comment", "two  spaces", long.as_str(), "time [%clk 0:01:00]", "a line\nbreak"];
        for (m, text) in ["e2e4", "e7e5", "g1f3", "b8c6", "f1b5"].iter().zip(said) {
            b.game.play_uci(m).unwrap();
            let n = b.game.current();
            b.game.set_comment(n, text).unwrap();
        }
        b.game.play_uci("a7a6").unwrap();
        let a6 = b.game.current();
        b.game.set_comment_before(a6, "the Morphy").unwrap();
        let rec = record(&b).unwrap();
        // The plain comment of 1.e4 is not stored: it comes back from the PGN. The one longer than
        // an old reader's 4 KiB is, and so is the comment before 3...a6.
        let stored = |path: &[u16]| rec.comments.iter().find(|c| c.path == path);
        assert!(stored(&[0]).is_none(), "{:?}", rec.comments.iter().map(|c| &c.path).collect::<Vec<_>>());
        assert_eq!(stored(&[0, 0, 0]).map(|c| c.after.as_str()), Some(long.as_str()));
        assert_eq!(stored(&[0, 0, 0, 0, 0, 0]).map(|c| c.before.as_str()), Some("the Morphy"));
        let back = restore(&rec, None).unwrap();
        for node in nodes(&b.game) {
            let path = b.game.path_of(node).unwrap();
            let n = back.game.node_at(&path).unwrap();
            assert_eq!((back.game.comment(n), back.game.comment_before(n)), (b.game.comment(node), b.game.comment_before(node)), "{path:?}");
        }
    }
}
