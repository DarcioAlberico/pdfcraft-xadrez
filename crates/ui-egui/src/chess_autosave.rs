//! Chess ▸ Game collections: the autosave of each document's chess games (PGN toolbar spec §4.7,
//! D10, T4b), on the engine's document store since plan A9-G3b.
//!
//! **What and where.** A document's games are the engine's kind of store
//! [`pdfcraft_engine::chess::games_store`]: one `<hash>.games.json` per document, the format the
//! command line and the automation tools read and change too (`chess_games {doc}`). On the
//! desktop the files are in the app's data folder (`chess/games`, beside the corrections and the
//! books' PGNs, plan A9-G5a, D17), under the recovery folder's instance lock; on the web, in
//! eframe's `Storage` under [`STORAGE_KEY`], within [`WEB_MAX_BYTES`].
//!
//! **Merging, never overwriting** (§2.3). Each game has a uid and a revision a change raises;
//! a write is merged under the store's lock with what the file has then (the higher revision
//! winning, a tie keeping this window's), so the games a tool imported or another tab added are
//! kept and come into the list ([`Synced::added`]); a game deleted here is written as a tombstone.
//! A file another running PdfCraft autosaves is never written ([`Blocked::OtherInstance`]), nor
//! one a newer PdfCraft wrote ([`Blocked::Newer`]); one that can't be read is set aside by the
//! write (`….bad`, never deleted), and said.
//!
//! **Before plan A9-G3b** the app kept `<id>.games` files (two lines of JSON, version 1) in the
//! recovery folder's `chess`. They are brought in at every start, by the engine's rule
//! ([`games_store::DirStore::migrate_v1`]: merged, and again only when an earlier build changed
//! them since), and never written, renamed or deleted, so an installed build goes on working with
//! them. The web's value of before (`pdfcraft-chess-games-v1`) is brought in once, and left as it
//! is.
//!
//! **Off the frame** (plan A9-I3a, I3b). The interface thread encodes only the games that changed
//! ([`RecordCache`]); on the desktop the store's writer thread ([`crate::chess_writer`]) merges and
//! writes, the last request of each collection winning, and what became of the write comes back
//! as [`Written::Queued`]'s generation. Nothing is evicted: a document above [`MAX_GAMES`] games or
//! [`MAX_BODY_BYTES`] is not autosaved (the user is told) until it is back under.

use std::collections::HashMap;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

use pdfcraft_engine::DocId;
use pdfcraft_engine::chess::DocKeys;
use pdfcraft_engine::chess::games_store::{self, GameRecord, Games, GamesFile, GamesStoreError, Owner};
#[cfg(test)]
use pdfcraft_engine::chess::pdfcraft_chess::game::Game;

use crate::chess_board_ui::{BoardState, Source};
use crate::chess_store_ui::{UiStore, in_store};

/// Most games autosaved for one document.
pub(crate) const MAX_GAMES: usize = games_store::MAX_GAMES;

/// Largest file autosaved for one document.
pub(crate) const MAX_BODY_BYTES: usize = games_store::MAX_FILE_BYTES;

/// The web build's `Storage` key since plan A9-G3b, and its cap for all documents together.
pub const STORAGE_KEY: &str = "pdfcraft-chess-games";
pub(crate) const WEB_MAX_BYTES: usize = 4 << 20;

/// The `Storage` key of the web's value of before plan A9-G3b: read once, never written.
pub const STORAGE_KEY_V1: &str = "pdfcraft-chess-games-v1";

/// Why a document's games were not autosaved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Blocked {
    /// More games than [`MAX_GAMES`], or a file over [`MAX_BODY_BYTES`] (or the web's cap), or a
    /// game too long to read back.
    TooBig,
    /// This instance holds no lock in the recovery folder.
    NoLock,
    /// Another running instance autosaves the games.
    OtherInstance,
    /// A newer PdfCraft wrote the file, in this format version: this one only reads it, and never
    /// writes over it.
    Newer(u32),
    /// The file couldn't be written.
    Io(String),
}

impl From<GamesStoreError> for Blocked {
    fn from(e: GamesStoreError) -> Blocked {
        match e {
            GamesStoreError::TooBig | GamesStoreError::OverBudget(_) => Blocked::TooBig,
            GamesStoreError::OtherInstance => Blocked::OtherInstance,
            GamesStoreError::Newer(v) => Blocked::Newer(v),
            other => Blocked::Io(other.to_string()),
        }
    }
}

/// What a write brought back from the store, beside the games written.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Synced {
    /// The games the store had and the list did not (a tool imported them, another tab added
    /// them): taken into the list.
    pub added: Vec<GameRecord>,
    /// A file that couldn't be read was set aside under this name first.
    pub set_aside: Option<String>,
}

/// What [`Store::write`] did with a document's games.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Written {
    /// They are in the store now (memory, the web's value).
    Now(Synced),
    /// The store's writer thread writes them (plan A9-I3b): what became of the write comes back
    /// from [`Store::take_written`] with this generation.
    Queued(u64),
}

/// What the writer thread writes: a document's games, merged with the file's.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct FileJob {
    keys: DocKeys,
    file: GamesFile,
    /// The instance writing it (its lock's name), which a file another live instance keeps is
    /// never written over by.
    instance: String,
}

/// The result of one write.
pub(crate) type WriteResult = Result<Synced, Blocked>;

/// The writer of a folder of games files.
#[cfg(not(target_arch = "wasm32"))]
type GamesWriter = crate::chess_writer::StoreWriter<FileJob, WriteResult>;

/// What a game stored keeps track of (plan A9-G3b): its uid in the store, its revision, and what
/// it was when last written or read, so a change raises the revision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Stored {
    pub uid: String,
    pub rev: u64,
    /// [`board_stamp`] when last written or read.
    pub stamp: u64,
}

/// What of a board the store keeps, summed up: the game's revision, the move shown, the side and
/// an illegal position. Changes when one of them does.
pub(crate) fn board_stamp(board: &BoardState) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    board.game.revision().hash(&mut h);
    board.game.current().hash(&mut h);
    board.flipped.hash(&mut h);
    board.illegal.as_ref().map(|(_, f)| f.as_str()).hash(&mut h);
    board.source.map(|s| (s.page, s.index)).hash(&mut h);
    h.finish()
}

/// `part` (what [`GameRecord::of_game`] stores of the board's game) as game `uid` at `rev`, with
/// what the board adds: the move shown, the side the board is seen from, an illegal position and
/// the diagram it came from.
fn with_board(mut part: GameRecord, board: &BoardState, uid: &str, rev: u64) -> GameRecord {
    part.uid = uid.to_owned();
    part.rev = rev;
    part.origin = board.source.map(|s| pdfcraft_engine::chess::DiagramAt::new(s.page, s.index));
    part.flipped = board.flipped;
    if let Some((_, fen)) = &board.illegal {
        // An empty game: only the position, as given.
        part.pgn.clear();
        part.current.clear();
        part.sources.clear();
        part.comments.clear();
        part.illegal_fen = Some(fen.clone());
        return part;
    }
    part.current = board.game.path_of(board.game.current()).map(|p| p.indices().to_vec()).unwrap_or_default();
    part
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
    /// The game part of `board`'s record for collection `cid` ([`GameRecord::of_game`]), from the
    /// cache when its game did not change; `None` when its PGN is too long to be read back.
    fn part(
        &mut self,
        before: &HashMap<u64, Option<GameRecord>>,
        now: &mut HashMap<u64, Option<GameRecord>>,
        board: &BoardState,
    ) -> Option<GameRecord> {
        let revision = board.game.revision();
        let part = match now.get(&revision).or_else(|| before.get(&revision)) {
            Some(part) => part.clone(),
            None => {
                self.encoded = self.encoded.saturating_add(1);
                GameRecord::of_game(String::new(), 0, &board.game)
            }
        };
        now.insert(revision, part.clone());
        part
    }
}

/// A collection's games as the autosave writes them (plan A9-G3b): each kept game with its
/// [`Stored`] (a new uid for a game not stored yet; a higher revision for one changed since),
/// the game shown, the tombstones of the games deleted, the trash and what was taken out of it.
pub(crate) struct Collected<'a> {
    pub cid: &'a str,
    pub keys: &'a DocKeys,
    /// The games kept, in list order, with what each knows of its stored self.
    pub games: Vec<(&'a mut Option<Stored>, &'a BoardState)>,
    /// The place of the game shown among them.
    pub active: usize,
    /// Every uid the collection has had in the store, with its revision and whether it is
    /// deleted there; games no longer in the list are written deleted.
    pub known: &'a mut HashMap<String, (u64, bool)>,
    pub trash: Vec<games_store::Trashed>,
    pub taken: Vec<String>,
}

/// Seconds since 1970, now.
#[cfg(not(target_arch = "wasm32"))]
fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Seconds since 1970, now: milliseconds, finite and non-negative (`as` saturates anything else
/// to a bound).
#[cfg(target_arch = "wasm32")]
fn now() -> u64 {
    (js_sys::Date::now() / 1000.0) as u64
}

/// The file a write of `c` makes, encoded from what changed ([`RecordCache`]). `Err(TooBig)` above
/// [`MAX_GAMES`] or when a game is too long to be read back; a file past [`MAX_BODY_BYTES`] is
/// refused by the store.
fn encode(cache: &mut RecordCache, c: Collected<'_>) -> Result<GamesFile, Blocked> {
    if c.games.len() > MAX_GAMES {
        return Err(Blocked::TooBig);
    }
    let at = now();
    let before = cache.by_cid.remove(c.cid).unwrap_or_default();
    let mut parts = HashMap::with_capacity(c.games.len());
    let mut file = GamesFile::empty(pdfcraft_engine::chess::corrections::DocRecord::new(c.keys, 0));
    let mut whole = true;
    for (i, (stored, board)) in c.games.into_iter().enumerate() {
        let stamp = board_stamp(board);
        let s = match stored.take() {
            None => Stored { uid: games_store::new_uid(at, c.cid.as_bytes()), rev: games_store::next_rev(0, at), stamp },
            Some(mut s) => {
                // Deleted in the store after this copy was read (an undo brings it back): above
                // the tombstone.
                let deleted_at = c.known.get(&s.uid).filter(|(_, deleted)| *deleted).map(|(rev, _)| *rev);
                if s.stamp != stamp || deleted_at.is_some() {
                    s.rev = games_store::next_rev(s.rev.max(deleted_at.unwrap_or(0)), at);
                    s.stamp = stamp;
                }
                s
            }
        };
        let part = if board.illegal.is_some() { Some(blank_record()) } else { cache.part(&before, &mut parts, board) };
        match part {
            Some(part) => {
                let record = with_board(part, board, &s.uid, s.rev);
                if i == c.active {
                    file.active = Some(s.uid.clone());
                }
                c.known.insert(s.uid.clone(), (s.rev, false));
                file.games.push(record);
            }
            None => whole = false,
        }
        *stored = Some(s);
    }
    if cache.by_cid.len() >= MAX_CACHED_COLLECTIONS {
        cache.by_cid.clear();
    }
    cache.by_cid.insert(c.cid.to_owned(), parts);
    if !whole {
        return Err(Blocked::TooBig);
    }
    let listed: std::collections::HashSet<String> = file.games.iter().map(|g| g.uid.clone()).collect();
    for (uid, (rev, deleted)) in c.known.iter_mut() {
        if listed.contains(uid) {
            continue;
        }
        if !*deleted {
            *rev = games_store::next_rev(*rev, at);
            *deleted = true;
        }
        file.games.push(GameRecord::tombstone(uid.clone(), *rev));
    }
    file.trash = c.trash;
    file.taken = c.taken;
    // Past the file's cap the store refuses it when it writes ([`Blocked::TooBig`]).
    Ok(file)
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
    let (game, cut) = record.game()?;
    // A stored comment past the cap (a file written by hand, or by a build with a larger cap) is
    // cut, and said on the board (plan A9-F3).
    if cut {
        board.message = Some(tl!("Only the first 65,536 bytes of a comment are kept.").to_owned());
    }
    board.game = game;
    board.flipped = record.flipped;
    board.source = source;
    board.refresh_fields(true);
    Some(board)
}

/// The games of a file, each with the document's diagrams and its [`Stored`]; and how many
/// couldn't be read.
pub(crate) fn decode(file: &GamesFile, doc: Option<DocId>) -> (Vec<(BoardState, Stored)>, usize) {
    let mut boards = Vec::new();
    let mut lost = 0usize;
    for (i, g) in file.live().enumerate() {
        if i >= MAX_GAMES {
            lost = lost.saturating_add(1);
            continue;
        }
        match restore(g, doc) {
            Some(b) => {
                let stamp = board_stamp(&b);
                boards.push((b, Stored { uid: g.uid.clone(), rev: g.rev, stamp }));
            }
            None => lost = lost.saturating_add(1),
        }
    }
    (boards, lost)
}

/// What looking for a document's games found.
#[derive(Default)]
pub(crate) struct Found {
    /// The document's file in the store.
    pub name: String,
    /// The document's games, when the store has any.
    pub games: Option<GamesFile>,
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
    #[cfg(not(target_arch = "wasm32"))]
    Files(FileStore),
    Memory(MemoryStore),
}

impl Store {
    /// The games stored for the document named by `keys`.
    pub(crate) fn find(&mut self, keys: &DocKeys) -> Found {
        if keys.is_empty() {
            return Found::default();
        }
        match self {
            Store::Off => Found::default(),
            #[cfg(not(target_arch = "wasm32"))]
            Store::Files(f) => found(f.store.load(keys, 0)),
            Store::Memory(m) => found(in_store!(&m.store.place, s => s.load(keys, 0))),
        }
    }

    /// Store a document's games: in memory now, or, in a folder, by the store's writer thread
    /// ([`Written::Queued`]; plan A9-I3b). What can be told without the file (too many games, no
    /// lock) is said now.
    pub(crate) fn write(&mut self, c: Collected<'_>) -> Result<Written, Blocked> {
        match self {
            Store::Off => Ok(Written::Now(Synced::default())),
            #[cfg(not(target_arch = "wasm32"))]
            Store::Files(f) => f.submit(c).map(Written::Queued),
            Store::Memory(m) => m.write(c).map(Written::Now),
        }
    }

    /// What became of the writes [`Store::write`] queued, since the last call: each one's
    /// collection id, generation and result.
    pub(crate) fn take_written(&mut self) -> Vec<crate::chess_writer::Done<WriteResult>> {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Store::Files(f) => f.writer.as_ref().map(GamesWriter::take_done).unwrap_or_default(),
            Store::Off | Store::Memory(_) => Vec::new(),
        }
    }

    /// Wait until the queued writes have ended, at most `timeout`. Whether they all have.
    pub(crate) fn settle(&self, timeout: std::time::Duration) -> bool {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Store::Files(f) => f.writer.as_ref().is_none_or(|w| w.settle(timeout)),
            Store::Off | Store::Memory(_) => true,
        }
    }

    /// Whether games are autosaved at all.
    pub(crate) fn is_on(&self) -> bool {
        !matches!(self, Store::Off)
    }
}

/// What a load found, for the collection.
fn found(loaded: Result<pdfcraft_engine::chess::store::Loaded<GamesFile>, GamesStoreError>) -> Found {
    match loaded {
        Err(e) => {
            log::warn!("chess games couldn't be read: {e}");
            Found::default()
        }
        Ok(l) => {
            // A file that can't be read is set aside by the next write, and said then.
            let has = !l.file.games.is_empty() || !l.file.trash.is_empty();
            Found { name: l.name, newer: l.newer, games: has.then_some(l.file) }
        }
    }
}

/// The desktop's store: a folder of `<hash>.games.json` files, written by its own thread.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct FileStore {
    store: games_store::DirStore,
    recovery: crate::RecoveryStore,
    /// The games' records as last written (plan A9-I3a).
    records: RecordCache,
    /// The writer thread, made with the first write ([`FileStore::submit`]; plan A9-I3b).
    writer: Option<GamesWriter>,
}

#[cfg(not(target_arch = "wasm32"))]
impl FileStore {
    /// The store in the folder `dir`, written under `recovery`'s instance lock.
    pub(crate) fn new(dir: PathBuf, recovery: crate::RecoveryStore) -> FileStore {
        FileStore { store: games_store::DirStore::new(dir), recovery, records: RecordCache::default(), writer: None }
    }

    /// The instance's lock, the recovery folder's.
    pub(crate) fn recovery(&self) -> &crate::RecoveryStore {
        &self.recovery
    }

    /// Have collection `c`'s games merged into its file by the store's writer thread, which is
    /// made with the first write: the request's generation ([`Written::Queued`]). What can be told
    /// here (no lock, too many games) is said now.
    fn submit(&mut self, c: Collected<'_>) -> Result<u64, Blocked> {
        let Some(instance) = self.recovery.instance() else { return Err(Blocked::NoLock) };
        let cid = c.cid.to_owned();
        let keys = c.keys.clone();
        let file = encode(&mut self.records, c)?;
        let job = FileJob { keys, file, instance };
        let writer = self.writer.get_or_insert_with(|| {
            let (recovery, dir) = (self.recovery.clone(), self.store.dir().to_path_buf());
            crate::chess_writer::StoreWriter::new(
                "chess-games-writer",
                move |_cid: &str, job| commit(&games_store::DirStore::new(&dir), &recovery, job),
                |panic| Err(Blocked::Io(format!("the writer stopped: {panic}"))),
            )
        });
        Ok(writer.submit(&cid, job))
    }
}

/// Merge `job` into its file in `store`, unless another live instance keeps it or a newer
/// PdfCraft wrote it; what the file had that the job did not comes back.
#[cfg(not(target_arch = "wasm32"))]
fn commit(store: &games_store::DirStore, recovery: &crate::RecoveryStore, job: FileJob) -> WriteResult {
    let live = |name: &str| recovery.held_by_another_instance(name);
    let owner = Owner { me: Some(job.instance.as_str()), live: &live, merge: false };
    let mine: std::collections::HashSet<String> = job.file.games.iter().map(|g| g.uid.clone()).collect();
    let (kept, loaded) = store.keep(&job.keys, 0, job.file, &owner)?;
    Ok(synced(&kept, &mine, loaded.set_aside.map(|(name, _)| name)))
}

/// What a write brings back: the live games the file had that the job (`mine`) did not.
fn synced(kept: &games_store::Kept, mine: &std::collections::HashSet<String>, set_aside: Option<String>) -> Synced {
    let added = kept.file.live().filter(|g| !mine.contains(&g.uid)).cloned().collect();
    Synced { added, set_aside }
}

/// The web's store: every document's games in one `Storage` value, written when eframe saves
/// ([`UiStore`]: another tab's changes merged in first, an unreadable value kept aside).
pub(crate) struct MemoryStore {
    pub(crate) store: UiStore<Games>,
    /// The games' records as last written (plan A9-I3a).
    pub(crate) records: RecordCache,
}

impl MemoryStore {
    /// The store a `Storage` value holds (`text`; the web's value of before plan A9-G3b, `v1`,
    /// brought in when there is none yet). Whether a value couldn't be read.
    pub(crate) fn load(text: Option<&str>, v1: Option<&str>) -> (MemoryStore, bool) {
        let mut store = UiStore::<Games>::storage(STORAGE_KEY, WEB_MAX_BYTES, text);
        let bad = store.unreadable().is_some();
        if text.is_none()
            && let Some(v1) = v1
        {
            bring_in_v1(&mut store, v1);
        }
        (MemoryStore { store, records: RecordCache::default() }, bad)
    }

    fn write(&mut self, c: Collected<'_>) -> WriteResult {
        self.store.writable().map_err(Blocked::Io)?;
        let keys = c.keys.clone();
        let file = encode(&mut self.records, c)?;
        let mine: std::collections::HashSet<String> = file.games.iter().map(|g| g.uid.clone()).collect();
        let nobody = |_: &str| false;
        let owner = Owner { me: None, live: &nobody, merge: false };
        let (kept, loaded) = in_store!(&self.store.place, s => s.keep(&keys, 0, file, &owner))?;
        self.store.wrote();
        Ok(synced(&kept, &mine, loaded.set_aside.map(|(name, _)| name)))
    }

    /// The `Storage` values to save, `(key, value)`, with what `current` (the value the `Storage`
    /// holds now) holds merged in ([`UiStore::storage_values`]).
    pub(crate) fn storage_values(&mut self, current: Option<&str>, taken: impl Fn(&str) -> bool) -> Vec<(String, String)> {
        self.store.storage_values(current, taken)
    }
}

/// The web's value of before plan A9-G3b, `{format, version: 1, entries: [{cid, header, body}]}`,
/// each entry's games brought into `store` ([`games_store::v1::read`]); what can't be read is
/// left out. The value itself is never written again.
fn bring_in_v1(store: &mut UiStore<Games>, text: &str) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else { return };
    let Some(entries) = value.get("entries").and_then(serde_json::Value::as_array) else { return };
    let mut brought = false;
    for e in entries.iter().take(4 * MAX_GAMES) {
        let (Some(cid), Some(header), Some(body)) = (e.get("cid").and_then(|c| c.as_str()), e.get("header"), e.get("body").and_then(|b| b.as_str()))
        else {
            continue;
        };
        let bytes = format!("{header}\n{body}\n");
        let Some((keys, theirs, _)) = games_store::v1::read(bytes.as_bytes(), cid, 0) else { continue };
        let merged = in_store!(&store.place, s => s.update(&keys, 0, |f| {
            f.merge(&theirs);
            Ok::<(), GamesStoreError>(())
        }));
        brought |= merged.is_ok();
    }
    if brought {
        store.wrote();
    }
}

/// A record of nothing: a position that can't be played starts from it ([`with_board`]).
fn blank_record() -> GameRecord {
    let mut r = GameRecord::tombstone(String::new(), 0);
    r.deleted = false;
    r
}

/// The record of a stored game, as the engine keeps it: for the tests.
#[cfg(test)]
pub(crate) fn record(board: &BoardState) -> Option<GameRecord> {
    let part = if board.illegal.is_some() { blank_record() } else { GameRecord::of_game(String::new(), 0, &board.game)? };
    Some(with_board(part, board, "test", 1))
}

/// Every move of `game`, the start first (for the tests).
#[cfg(test)]
fn nodes(game: &Game) -> Vec<pdfcraft_engine::chess::pdfcraft_chess::game::NodeId> {
    use pdfcraft_engine::chess::pdfcraft_chess::game::Token;
    std::iter::once(0)
        .chain(game.tokens().into_iter().filter_map(|t| match t {
            Token::Move { node, .. } => Some(node),
            _ => None,
        }))
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use pdfcraft_engine::chess::pdfcraft_chess::game::{NodePath, NodeSource};
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

    fn other_keys() -> DocKeys {
        DocKeys { id0: Some([5; 16]), path: Some("/b/other.pdf".into()), name: Some("other.pdf".into()), size: Some(99) }
    }

    /// The keys of the frozen version 1 sample ([`GAMES_V1`]).
    pub(crate) fn v1_keys() -> DocKeys {
        keys()
    }

    /// The version 1 format as the installed builds (≤ cc2d484c) write it (plan A9-G12), frozen.
    pub(crate) const GAMES_V1: &[u8] = include_bytes!("../tests/compat/games_v1.games");

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
        g.set_comment_before(nf3, "before Nf3").unwrap();
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

    /// A collection of `boards` to write, its games' [`Stored`] in `stored` and the uids it had in
    /// the store in `known`.
    fn collected<'a>(
        cid: &'a str,
        keys: &'a DocKeys,
        boards: &'a [BoardState],
        stored: &'a mut Vec<Option<Stored>>,
        known: &'a mut HashMap<String, (u64, bool)>,
    ) -> Collected<'a> {
        stored.resize(boards.len(), None);
        let games = stored.iter_mut().zip(boards).collect();
        Collected { cid, keys, games, active: 0, known, trash: Vec::new(), taken: Vec::new() }
    }

    #[test]
    fn a_game_comes_back_as_it_was_with_what_pgn_cannot_hold() {
        let board = annotated();
        let rec = record(&board).unwrap();
        let text = serde_json::to_string(&rec).unwrap();
        let back = restore(&serde_json::from_str(&text).unwrap(), None).unwrap();
        same(&board, &back);
        assert_eq!(back.game.comment_before(back.game.node_at(&NodePath::new(vec![0, 0, 0])).unwrap()), "before Nf3");
        assert!(back.history.undo_edit().is_none(), "a restored game has no history");
    }

    #[test]
    fn an_illegal_position_and_the_diagram_come_back() {
        let mut b = BoardState::default();
        b.load_fen("K7/8/8/8/8/8/8/K6k w - - 0 1", false, Some(Source { doc: DocId(7), page: 3, index: 1 }));
        let back = restore(&record(&b).unwrap(), Some(DocId(9))).unwrap();
        assert_eq!(back.illegal.as_ref().map(|(_, f)| f.as_str()), Some("K7/8/8/8/8/8/8/K6k w - - 0 1"));
        assert_eq!(back.source, Some(Source { doc: DocId(9), page: 3, index: 1 }), "the diagram, in the document open now");
        assert!(back.history.undo_edit().is_none());
    }

    /// Plan A9-G3b: a write gives each game a uid once, raises its revision only when it changed,
    /// and writes a game no longer in the list as deleted; past the cap it is refused.
    #[test]
    fn uids_revisions_tombstones_and_the_cap() {
        let (mut m, _) = MemoryStore::load(None, None);
        let mut boards = vec![annotated(), BoardState::default()];
        let (mut stored, mut known) = (Vec::new(), HashMap::new());
        m.write(collected("c-1", &keys(), &boards, &mut stored, &mut known)).unwrap();
        let first: Vec<Stored> = stored.iter().flatten().cloned().collect();
        assert_eq!(first.len(), 2);
        m.write(collected("c-1", &keys(), &boards, &mut stored, &mut known)).unwrap();
        assert_eq!(stored.iter().flatten().cloned().collect::<Vec<_>>(), first, "unchanged: the same uids and revisions");
        boards[1].game.play_uci("d2d4").unwrap();
        m.write(collected("c-1", &keys(), &boards, &mut stored, &mut known)).unwrap();
        assert!(stored[1].as_ref().unwrap().rev > first[1].rev && stored[0] == Some(first[0].clone()));
        // The second game deleted: a tombstone.
        let mut one = vec![stored[0].clone()];
        m.write(collected("c-1", &keys(), &boards[..1], &mut one, &mut known)).unwrap();
        let file = in_store!(&m.store.place, s => s.load(&keys(), 0)).unwrap().file;
        assert_eq!(file.live().count(), 1);
        assert!(file.get(&first[1].uid).is_some_and(|g| g.deleted));
        let many: Vec<BoardState> = (0..=MAX_GAMES).map(|_| BoardState::default()).collect();
        let (mut s, mut k) = (Vec::new(), HashMap::new());
        assert_eq!(m.write(collected("c-2", &keys(), &many, &mut s, &mut k)).err(), Some(Blocked::TooBig));
    }

    /// Plan A9-G3b (§2.3): two tabs of the web build each keep a game, one in another book, one in
    /// the same book: saved one after the other, every game stays (the second tab's value merges
    /// the first's in before it is written).
    #[test]
    fn two_web_tabs_keep_each_others_games() {
        let (mut first, _) = MemoryStore::load(None, None);
        let (mut second, _) = MemoryStore::load(None, None);
        let (a, b, c) = (vec![annotated()], vec![BoardState::default()], vec![annotated()]);
        let (mut s1, mut k1) = (Vec::new(), HashMap::new());
        first.write(collected("t1", &keys(), &a, &mut s1, &mut k1)).unwrap();
        let (mut s2, mut k2) = (Vec::new(), HashMap::new());
        second.write(collected("t2", &other_keys(), &b, &mut s2, &mut k2)).unwrap();
        let (mut s3, mut k3) = (Vec::new(), HashMap::new());
        second.write(collected("t3", &keys(), &c, &mut s3, &mut k3)).unwrap();
        let (_, saved) = first.storage_values(None, |_| false).pop().unwrap();
        let (_, both) = second.storage_values(Some(&saved), |_| false).pop().unwrap();
        let (mut again, bad) = MemoryStore::load(Some(&both), None);
        assert!(!bad);
        assert_eq!(again.store.unreadable(), None);
        let book = again.find_for_test(&keys());
        assert_eq!(book.live().count(), 2, "this book's game of each tab");
        assert_eq!(again.find_for_test(&other_keys()).live().count(), 1, "the other book's");
    }

    /// Plan A9-G3b: the web's value of before (`pdfcraft-chess-games-v1`) is brought in when there
    /// is no value of this version yet, and then never again.
    #[test]
    fn the_web_value_of_before_is_brought_in_once() {
        let text = String::from_utf8(GAMES_V1.to_vec()).unwrap();
        let (header, body) = text.trim_end().split_once('\n').unwrap();
        let v1 = json!({ "format": "pdfcraft-chess-games", "version": 1, "entries": [{ "cid": "abc-1", "header": serde_json::from_str::<Value>(header).unwrap(), "body": body }] }).to_string();
        let (mut m, bad) = MemoryStore::load(None, Some(&v1));
        assert!(!bad);
        let file = m.find_for_test(&v1_keys());
        let (boards, lost) = decode(&file, Some(DocId(7)));
        assert_eq!((boards.len(), lost), (3, 0));
        let sample = v1_sample_boards();
        for ((b, _), a) in boards.iter().zip(&sample) {
            assert_eq!(a.game.movetext(), b.game.movetext());
            assert_eq!(a.source, b.source);
        }
        let (_, saved) = m.storage_values(None, |_| false).pop().expect("saved under the new key");
        assert!(!saved.contains("pdfcraft-chess-games-v1"));
        // With a value of this version, the old one is not read again.
        let (mut again, _) = MemoryStore::load(Some("[]"), Some(&v1));
        assert_eq!(again.find_for_test(&v1_keys()).live().count(), 0);
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

    /// Plan A9-I3a (A5-02): the encoding of a document's 120 games takes at most 5 ms of the
    /// interface thread (the best of five, in the tests' optimised build). Timing: `cargo test -p
    /// pdfcraft-ui-egui --lib -- --ignored perf_`.
    #[test]
    #[ignore = "timing (plan A9-I3a): run with --ignored perf_"]
    fn perf_the_autosave_of_120_games_takes_at_most_5_ms() {
        let mut boards = book_games();
        let moves: usize = boards.iter().map(|b| b.game.main_line().len()).sum();
        assert!(moves > 120 * 60, "long games: {moves} moves");
        let mut cache = RecordCache::default();
        let (mut stored, mut known) = (Vec::new(), HashMap::new());
        encode(&mut cache, collected("perf-1", &keys(), &boards, &mut stored, &mut known)).unwrap();
        let mut best = f64::MAX;
        for round in 0..5usize {
            let b = &mut boards[round * 7];
            let n = b.game.current();
            let _ = b.game.set_comment(n, &format!("Changed {round}."));
            let started = std::time::Instant::now();
            encode(&mut cache, collected("perf-1", &keys(), &boards, &mut stored, &mut known)).unwrap();
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
        let mut cache = RecordCache::default();
        let (mut stored, mut known) = (Vec::new(), HashMap::new());
        encode(&mut cache, collected("c-1", &keys(), &boards, &mut stored, &mut known)).unwrap();
        assert_eq!(cache.encoded, 120);
        let b = &mut boards[5];
        let n = b.game.current();
        b.game.set_comment(n, "A new comment.").unwrap();
        encode(&mut cache, collected("c-1", &keys(), &boards, &mut stored, &mut known)).unwrap();
        assert_eq!(cache.encoded, 121, "one of 120 encoded again");
        // Moving through a game or flipping its board is no new encoding, and is written.
        boards[9].flipped = true;
        let cached = encode(&mut cache, collected("c-1", &keys(), &boards, &mut stored, &mut known)).unwrap();
        assert_eq!(cache.encoded, 121);
        let fresh = encode(&mut RecordCache::default(), collected("c-1", &keys(), &boards, &mut stored, &mut known)).unwrap();
        assert_eq!(cached.games, fresh.games);
        assert!(cached.games[9].flipped);
    }

    /// Plan A9-G3b, as T16 did for version 1: random values in place of a written file's (paths
    /// of 65,535, huge pages, a PGN that is not one), random bytes and cut files never panic
    /// when the document's games are looked for and restored.
    #[test]
    fn mutated_games_files_never_panic() {
        let (mut m, _) = MemoryStore::load(None, None);
        let boards = vec![annotated(), BoardState::default()];
        let (mut stored, mut known) = (Vec::new(), HashMap::new());
        m.write(collected("c-1", &keys(), &boards, &mut stored, &mut known)).unwrap();
        let (_, value) = m.storage_values(None, |_| false).pop().unwrap();
        let files: Vec<(String, String)> = serde_json::from_str(&value).unwrap();
        let (name, valid) = files[0].clone();
        let parsed: Value = serde_json::from_str(&valid).unwrap();
        let mut rng = Rng(0x7E16_A5A5);
        let mut read = 0usize;
        for round in 0..400 {
            let text = match round % 3 {
                0 => valid.chars().take(rng.below(valid.len())).collect::<String>(),
                1 => {
                    let mut v = parsed.clone();
                    mutate_value(&mut rng, &mut v, true);
                    v.to_string()
                }
                _ => {
                    let mut v = parsed.clone();
                    mutate_value(&mut rng, &mut v, false);
                    v.to_string()
                }
            };
            let value = serde_json::to_string(&vec![(name.clone(), text)]).unwrap();
            let (mut m, _) = MemoryStore::load(Some(&value), None);
            let found = m.find_for_test(&keys());
            read += usize::from(found.live().count() > 0);
            let _ = decode(&found, Some(DocId(3)));
        }
        assert!(read > 50, "the games were read back {read} times");
    }

    impl MemoryStore {
        /// The document's games as the store has them now.
        fn find_for_test(&mut self, keys: &DocKeys) -> GamesFile {
            in_store!(&self.store.place, s => s.load(keys, 0))
                .map(|l| l.file)
                .unwrap_or_else(|_| GamesFile::empty(pdfcraft_engine::chess::corrections::DocRecord::new(keys, 0)))
        }
    }
}
