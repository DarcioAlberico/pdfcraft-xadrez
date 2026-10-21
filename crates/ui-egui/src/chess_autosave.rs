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
//! reply is written into the comment after the move before it, and read back as part of that
//! one): every move whose comments read back differently is stored with both of its comments, so
//! a reload puts them back where they were.
//!
//! **Safety.** Files are written atomically ([`crate::editing::write_atomically`]) and only by the
//! instance that holds a lock in the recovery folder; a file another running instance wrote is
//! left alone. Reading checks the size first and parses leniently; a file that can't be read,
//! is too big or has an unknown version is set aside as `….bad` (never deleted) and the user is
//! told. Nothing is evicted: a document above [`MAX_GAMES`] games or [`MAX_BODY_BYTES`] is not
//! autosaved (the user is told) until it is back under.

use std::collections::HashSet;
use std::io::{BufRead, Read};
use std::path::{Path, PathBuf};

use pdfcraft_engine::DocId;
use pdfcraft_engine::chess::DocKeys;
use pdfcraft_engine::chess::pdfcraft_chess::game::{Game, MAX_PGN_LEN, NodeId, NodePath, NodeSource, Token};
use serde::{Deserialize, Serialize};

use crate::chess_board_ui::{BoardState, Source};

/// The format's name, in every header.
const FORMAT: &str = "pdfcraft-chess-games";

/// The format's version. Another version is set aside, not read.
pub(crate) const VERSION: u32 = 1;

/// Most games autosaved for one document.
pub(crate) const MAX_GAMES: usize = 256;

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
}

/// The second line: the games.
#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct Body {
    #[serde(default)]
    pub active: usize,
    #[serde(default)]
    pub games: Vec<GameRecord>,
}

/// One game as stored.
#[derive(Debug, Default, Serialize, Deserialize)]
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
#[derive(Debug, Serialize, Deserialize)]
struct StoredSource {
    path: Vec<u16>,
    page: u32,
    rect: [f32; 4],
}

/// Move `path`'s two comments, where PGN would put them elsewhere.
#[derive(Debug, Serialize, Deserialize)]
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
    /// The file couldn't be written.
    Io(String),
}

/// Every move of `game`, the start first.
fn nodes(game: &Game) -> Vec<NodeId> {
    std::iter::once(0)
        .chain(game.tokens().into_iter().filter_map(|t| match t {
            Token::Move { node, .. } => Some(node),
            _ => None,
        }))
        .collect()
}

/// `board`'s game as stored. `None` when its PGN is too long to be read back.
pub(crate) fn record(board: &BoardState) -> Option<GameRecord> {
    let origin = board.source.map(|s| StoredOrigin { page: s.page, index: s.index });
    if let Some((_, fen)) = &board.illegal {
        return Some(GameRecord { illegal_fen: Some(fen.clone()), flipped: board.flipped, origin, ..GameRecord::default() });
    }
    let game = &board.game;
    let pgn = game.pgn();
    if pgn.len() > MAX_PGN_LEN {
        return None;
    }
    let reloaded = Game::from_pgn(&pgn).ok().map(|(g, _)| g);
    let mut sources = Vec::new();
    let mut comments = Vec::new();
    for node in nodes(game) {
        let Some(path) = game.path_of(node) else { continue };
        if let Some(s) = game.source(node) {
            sources.push(StoredSource { path: path.indices().to_vec(), page: s.page, rect: s.rect_user });
        }
        let (after, before) = (game.comment(node), game.comment_before(node));
        let back = reloaded.as_ref().and_then(|r| r.node_at(&path).map(|n| (r.comment(n), r.comment_before(n))));
        if back != Some((after, before)) {
            comments.push(StoredComments { path: path.indices().to_vec(), after: after.to_owned(), before: before.to_owned() });
        }
    }
    let current = game.path_of(game.current()).map(|p| p.indices().to_vec()).unwrap_or_default();
    Some(GameRecord { pgn, current, flipped: board.flipped, illegal_fen: None, origin, sources, comments })
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
    for c in &record.comments {
        if let Some(node) = game.node_at(&NodePath::new(c.path.clone())) {
            game.set_comment(node, &c.after);
            game.set_comment_before(node, &c.before);
        }
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
pub(crate) fn encode(keys: &DocKeys, active: usize, boards: &[&BoardState], instance: Option<String>) -> Result<(Header, String), Blocked> {
    if boards.len() > MAX_GAMES {
        return Err(Blocked::TooBig);
    }
    let games = boards.iter().map(|b| record(b)).collect::<Option<Vec<_>>>().ok_or(Blocked::TooBig)?;
    let body = serde_json::to_string(&Body { active, games }).map_err(|e| Blocked::Io(e.to_string()))?;
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

/// Whether `header` is one this version reads.
fn readable(header: &Header) -> bool {
    header.format == FORMAT && header.version == VERSION
}

/// What looking for a document's games found.
#[derive(Default)]
pub(crate) struct Found {
    /// The collection's id and its games.
    pub games: Option<(String, Body)>,
    /// Files (or web values) that couldn't be read and were set aside, by the name they were
    /// kept under.
    pub set_aside: Vec<String>,
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

    /// Store a document's games under `cid`.
    pub(crate) fn write(&mut self, cid: &str, keys: &DocKeys, active: usize, boards: &[&BoardState]) -> Result<(), Blocked> {
        match self {
            Store::Off => Ok(()),
            Store::Files(f) => f.write(cid, keys, active, boards),
            Store::Memory(m) => m.write(cid, keys, active, boards),
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
}

impl FileStore {
    pub(crate) fn new(dir: PathBuf, recovery: crate::RecoveryStore) -> FileStore {
        FileStore { dir, recovery, ignored: HashSet::new() }
    }

    fn path(&self, cid: &str) -> PathBuf {
        self.dir.join(format!("{cid}.games"))
    }

    /// The header of the file at `path`, read within the cap. `Err` when the file is there and
    /// can't be read as one this version reads; `Ok(None)` when there is no file.
    fn header(path: &Path) -> Result<Option<Header>, ()> {
        let meta = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(()),
        };
        if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
            return Err(());
        }
        let file = std::fs::File::open(path).map_err(|_| ())?;
        let mut line = Vec::new();
        std::io::BufReader::new(file.take(MAX_HEADER_BYTES as u64)).read_until(b'\n', &mut line).map_err(|_| ())?;
        let header: Header = serde_json::from_slice(line.strip_suffix(b"\n").ok_or(())?).map_err(|_| ())?;
        if readable(&header) { Ok(Some(header)) } else { Err(()) }
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

    /// Set the file at `path` aside as `….bad` (or `….bad.1`, …), never over another one. The
    /// name it was kept under, or `None` when it couldn't be renamed (it is then ignored).
    fn set_aside(&mut self, path: &Path) -> Option<String> {
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
        self.ignored.insert(path.to_path_buf());
        None
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
            let header = match FileStore::header(&path) {
                Ok(Some(h)) => h,
                Ok(None) => continue,
                Err(()) => {
                    found.set_aside.extend(self.set_aside(&path));
                    continue;
                }
            };
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
        found
    }

    fn write(&mut self, cid: &str, keys: &DocKeys, active: usize, boards: &[&BoardState]) -> Result<(), Blocked> {
        if !plain_cid(cid) {
            return Err(Blocked::Io(format!("bad collection id {cid:?}")));
        }
        let Some(instance) = self.recovery.instance() else { return Err(Blocked::NoLock) };
        let (header, body) = encode(keys, active, boards, Some(instance.clone()))?;
        let path = self.path(cid);
        match FileStore::header(&path) {
            Ok(Some(h)) => {
                if let Some(other) = h.instance.as_deref().filter(|w| *w != instance)
                    && self.recovery.held_by_another_instance(other)
                {
                    return Err(Blocked::OtherInstance);
                }
            }
            Ok(None) => {}
            // What is there can't be read: keep it, never write over it.
            Err(()) => {
                if self.set_aside(&path).is_none() {
                    return Err(Blocked::Io("the file there couldn't be set aside".into()));
                }
            }
        }
        let line = serde_json::to_string(&header).map_err(|e| Blocked::Io(e.to_string()))?;
        let mut bytes = Vec::with_capacity(line.len().saturating_add(body.len()).saturating_add(2));
        bytes.extend_from_slice(line.as_bytes());
        bytes.push(b'\n');
        bytes.extend_from_slice(body.as_bytes());
        bytes.push(b'\n');
        std::fs::create_dir_all(&self.dir).map_err(|e| Blocked::Io(e.to_string()))?;
        crate::editing::write_atomically(&path.to_string_lossy(), &bytes).map_err(|e| Blocked::Io(e.to_string()))
    }
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
}

impl MemoryStore {
    /// The store a `Storage` value holds. A value that can't be read is kept aside (see
    /// [`Self::take_bad`]) and the store starts empty; whether that happened.
    pub(crate) fn load(text: Option<&str>) -> (MemoryStore, bool) {
        let Some(text) = text.filter(|t| !t.is_empty()) else { return (MemoryStore::default(), false) };
        let file = (text.len() <= WEB_MAX_BYTES.saturating_mul(2))
            .then(|| serde_json::from_str::<MemFile>(text).ok())
            .flatten()
            .filter(|f| f.format == FORMAT && f.version == VERSION);
        match file {
            Some(f) => (MemoryStore { entries: f.entries.into_iter().filter(|e| plain_cid(&e.cid)).collect(), dirty: false, bad: None }, false),
            None => (MemoryStore { entries: Vec::new(), dirty: false, bad: Some(text.chars().take(WEB_MAX_BYTES).collect()) }, true),
        }
    }

    fn find(&mut self, keys: &DocKeys, taken: &HashSet<String>) -> Found {
        let mut found = Found::default();
        if keys.is_empty() {
            return found;
        }
        let mut i = 0;
        while let Some(e) = self.entries.get(i) {
            if !readable(&e.header) || (!taken.contains(&e.cid) && e.header.keys.keys().matches(keys)) {
                let entry = self.entries.remove(i);
                if readable(&entry.header)
                    && let Ok(body) = serde_json::from_str::<Body>(&entry.body)
                {
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

    fn write(&mut self, cid: &str, keys: &DocKeys, active: usize, boards: &[&BoardState]) -> Result<(), Blocked> {
        let (header, body) = encode(keys, active, boards, None)?;
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
        if !std::mem::take(&mut self.dirty) {
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

    /// A game with a variation, a comment before a first reply (which PGN merges into the comment
    /// after the move before it), a comment before a variation's first move, glyphs, tags,
    /// sources and a move shown inside the variation.
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
        g.set_comment(e5, "after e5");
        // Nf3 is e5's first reply: PGN would write this one inside "after e5".
        g.set_comment_before(nf3, "before Nf3");
        // c5 starts a variation: PGN has room for this one, after the "(".
        g.set_comment_before(c5, "before c5");
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
        // PGN alone loses where the comment before Nf3 was.
        let (plain, _) = Game::from_pgn(&rec.pgn).unwrap();
        let nf3 = plain.node_at(&NodePath::new(vec![0, 0, 0])).unwrap();
        assert_eq!(plain.comment_before(nf3), "");
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
        assert_eq!(encode(&keys(), 0, &many, None).err(), Some(Blocked::TooBig));
        assert!(encode(&keys(), 0, &many[..MAX_GAMES], None).is_ok());
        let body = Body {
            active: 0,
            games: vec![
                GameRecord { pgn: "1. e4 e5".into(), ..GameRecord::default() },
                GameRecord { illegal_fen: Some("junk".into()), ..GameRecord::default() },
            ],
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
        m.write("abc-1", &keys(), 0, &[&board]).unwrap();
        let value = m.take_value().unwrap();
        assert_eq!(m.take_value(), None, "taken once per change");
        let (mut m, bad) = MemoryStore::load(Some(&value));
        assert!(!bad);
        let found = m.find(&keys(), &HashSet::new());
        let (cid, body) = found.games.unwrap();
        assert_eq!(cid, "abc-1");
        same(&board, &decode(&body, None).0[0]);
        // Taken by another open document: not found again.
        assert!(m.find(&keys(), &HashSet::from(["abc-1".to_owned()])).games.is_none());
        // Over the web's cap: refused, nothing evicted.
        // 256 games of about 20 KB each: 5 MB, over the web's 4 MiB (and under the desktop's cap).
        let mut big = BoardState::default();
        for m in ["e2e4", "e7e5", "g1f3", "b8c6", "f1b5"] {
            big.game.play_uci(m).unwrap();
            big.game.set_comment(big.game.current(), &"x".repeat(4000));
        }
        let mut boards = Vec::new();
        for _ in 0..MAX_GAMES {
            boards.push(&big);
        }
        assert_eq!(m.write("abc-2", &keys(), 0, &boards), Err(Blocked::TooBig));
        assert!(m.find(&keys(), &HashSet::new()).games.is_some(), "the first document's games are still there");
        // A value that can't be read is kept aside and the store starts empty.
        for junk in ["{not json", "{\"format\":\"pdfcraft-chess-games\",\"version\":99,\"entries\":[]}"] {
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
        store.write("abc-1", &keys(), 1, &[&annotated(), &BoardState::default()]).unwrap();
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
}
