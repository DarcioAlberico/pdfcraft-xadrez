//! Chess ▸ Game collections kept with their document, below the interface (plan A9-G3b, §2.3): a
//! document's games as a kind of the engine's document store ([`Games`]), so the app, the command
//! line and the automation tools read and change the same games.
//!
//! **Format** (version [`VERSION`]): one JSON file per document, `<hash>.games.json` (never
//! `*.games`, which the builds installed before it look through and set aside when its version
//! is not theirs, whichever book it is for: plan A9 §2.10, R3-05):
//! `{"format":"pdfcraft-chess-games","version":2,"doc":{…},"instance":…,"active":…,"games":[…],
//! "trash":[…],"taken":[…]}`.
//! - The document is recorded as every store records it ([`DocRecord`]: its identifier, the hash
//!   of its path, never the path, its name and size; R7-03), and `instance` names the running
//!   PdfCraft that autosaves the games (its lock file's name), which another one never writes over.
//! - Each game ([`GameRecord`]) has a `uid` and a revision `rev` that every change raises
//!   ([`next_rev`]); deleting leaves a tombstone (`deleted`), so a merge never brings the game back.
//!   It keeps its PGN and what PGN can't hold: the move shown, the side the board is seen from, a
//!   position that can't be played, the diagram it was opened from, where each move was read
//!   (`sources`, by the move's path) and the comments this build's PGN would not give back as they
//!   are (`comments`: every comment before a move, with the comment of the move before it, which
//!   this build's reader may take them into; a comment with several spaces, or a `[%` that is no
//!   drawing). The 4 KiB the readers of the builds before plan A9-F2b cut a comment at is no
//!   reason any more: those builds never read this file (R9-01).
//! - `active` is the uid of the game shown; `trash` the games as they were before moves left them
//!   (plan A8 §7), and `taken` the trash items taken out of it, by [`Trashed::id`], so a merge never
//!   puts one back.
//! - Fields a later build of this version writes, in the file, in a game or in a move's source,
//!   are kept as they are.
//!
//! **Merging** ([`GamesFile::merge`]): game by game, by uid, the higher revision winning (a tie
//! keeps this file's); the games only the other side has come after this file's, in its order;
//! the trash is the union of both, without what either took out. So the app's autosave, a tool's
//! change and an import into the same document never lose each other's games.
//!
//! **Version 1** (`<cid>.games`, two lines of JSON, kept by the builds before plan A9-G3b in the
//! recovery folder's `chess`) is only read, to bring its games in ([`v1`],
//! [`DocStore::migrate_v1`]); it is never written, renamed or deleted, so a build from before goes
//! on working on it, and what it changes there is brought in again at the next start.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::corrections::{DocRecord, hash_hex};
use super::store::{DocStore, Loaded, Parsed, StoreBackend, StoreFailure, StoreKind};
use crate::chess::pdfcraft_chess::game::{Game, MAX_PGN_LEN, NodeId, NodePath, NodeSource, Token};
use crate::chess::pdfcraft_chessvision::doc::DiagramAt;

/// The format's name, in every file (and in the headers of version 1).
pub const FORMAT: &str = "pdfcraft-chess-games";

/// The version this build writes: `<hash>.games.json`. A newer one is read only.
pub const VERSION: u32 = 2;

/// Most games of one document's collection: what a PGN import reads, the tools accept and the app
/// autosaves.
pub const MAX_GAMES: usize = super::collection::MAX_GAMES;

/// Most games one file keeps, tombstones included; past it the oldest tombstones go.
pub const MAX_RECORDS: usize = 8 * MAX_GAMES;

/// Largest file read or written.
pub const MAX_FILE_BYTES: usize = (32 << 20) + (64 << 10);

/// Most games kept in one document's trash; the oldest goes first.
pub const MAX_TRASHED: usize = 30;

/// Longest PGN kept in the trash.
pub const MAX_TRASHED_PGN: usize = 256 << 10;

/// Most trash items taken out that a file remembers.
const MAX_TAKEN: usize = 8 * MAX_TRASHED;

/// Longest uid, and of a FEN or an instance's name.
const MAX_TEXT: usize = 512;

/// Most pages a diagram's address may name (the corrections' own cap, R2-03).
const MAX_PAGES: usize = super::corrections::MAX_PAGES;

/// Where a move was read on a page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MoveSource {
    /// The move's path in its game ([`NodePath`]).
    pub path: Vec<u16>,
    /// The page (0-based).
    pub page: u32,
    /// Its box in user space.
    pub rect: [f32; 4],
    /// Fields a later build of this version writes, kept as they are.
    #[serde(flatten)]
    pub other: Map<String, Value>,
}

/// A move's two comments, where this build's PGN would not give them back as they are.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoveComments {
    pub path: Vec<u16>,
    #[serde(default)]
    pub after: String,
    #[serde(default)]
    pub before: String,
}

/// One game as stored.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GameRecord {
    /// The game's identity, kept through every change and every copy of the file.
    pub uid: String,
    /// Raised by every change ([`next_rev`]): of two copies, the higher one is the game.
    #[serde(default)]
    pub rev: u64,
    /// Deleted: a tombstone, so a merge never brings the game back.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub deleted: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub pgn: String,
    /// The move shown, by its path.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub current: Vec<u16>,
    /// The board is seen from Black's side.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub flipped: bool,
    /// A position that can't be played, as given (the game is then empty).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub illegal_fen: Option<String>,
    /// The diagram the game was opened from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<DiagramAt>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<MoveSource>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comments: Vec<MoveComments>,
    /// Fields a later build of this version writes, kept as they are.
    #[serde(flatten)]
    pub other: Map<String, Value>,
}

/// A game as it was before moves left it (plan A8 §7, V5).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trashed {
    /// The game before the deletion, as PGN.
    #[serde(default)]
    pub pgn: String,
    /// The game's name in the list then.
    #[serde(default)]
    pub name: String,
    /// Its moves then, variations included.
    #[serde(default)]
    pub moves: usize,
    /// How many of them went.
    #[serde(default)]
    pub deleted: usize,
    /// When, in seconds since 1970 (0 when unknown).
    #[serde(default)]
    pub at: u64,
}

impl Trashed {
    /// What names the item in every copy of the file: its PGN and when it was kept.
    pub fn id(&self) -> String {
        hash_hex(&[self.pgn.as_bytes(), &self.at.to_be_bytes()])
    }
}

/// A document's games.
#[derive(Clone, Debug, PartialEq)]
pub struct GamesFile {
    /// [`VERSION`], or the newer version of a file read only.
    pub version: u32,
    pub doc: DocRecord,
    /// The lock file name of the running PdfCraft that autosaves these games.
    pub instance: Option<String>,
    /// The uid of the game shown.
    pub active: Option<String>,
    /// The games in list order, tombstones among them.
    pub games: Vec<GameRecord>,
    /// The trash, oldest first.
    pub trash: Vec<Trashed>,
    /// The trash items taken out, by [`Trashed::id`].
    pub taken: Vec<String>,
    /// Fields a later build of this version wrote, kept as they are.
    pub other: Map<String, Value>,
}

/// The file as written: the header first, so finding a document's file parses little.
#[derive(Serialize)]
struct Written<'a> {
    format: &'static str,
    version: u32,
    doc: &'a DocRecord,
    #[serde(skip_serializing_if = "Option::is_none")]
    instance: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    active: Option<&'a str>,
    games: &'a [GameRecord],
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    trash: &'a [Trashed],
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    taken: &'a [String],
    #[serde(flatten)]
    other: &'a Map<String, Value>,
}

/// Why a store of games failed.
#[derive(Debug, thiserror::Error)]
pub enum GamesStoreError {
    #[error("the chess games folder: {0}")]
    Io(#[from] std::io::Error),
    #[error("the document has no name, path or identifier to keep games under")]
    NoKeys,
    #[error("another PdfCraft is writing the chess games; try again")]
    Busy,
    #[error("the chess games were saved by a newer PdfCraft (format {0}); this one only reads them")]
    Newer(u32),
    #[error("the chess games kept here would take more than their {0} bytes")]
    OverBudget(usize),
    #[error("too many chess games to keep for one document (at most {MAX_GAMES}, {} MB)", MAX_FILE_BYTES >> 20)]
    TooBig,
    #[error("a running PdfCraft autosaves these games: pass merge: true to merge into them")]
    OtherInstance,
}

impl StoreFailure for GamesStoreError {
    fn no_keys() -> Self {
        GamesStoreError::NoKeys
    }

    fn busy() -> Self {
        GamesStoreError::Busy
    }

    fn newer(version: u32) -> Self {
        GamesStoreError::Newer(version)
    }

    fn over_budget(budget: usize) -> Self {
        GamesStoreError::OverBudget(budget)
    }
}

/// The revision after `previous` when a change is made at `now` (seconds since 1970): the later of
/// the two, so a change made later wins over one made earlier in another copy, whichever build or
/// process made it, and one made in the same second still raises the revision.
pub fn next_rev(previous: u64, now: u64) -> u64 {
    previous.saturating_add(1).max(now)
}

/// A new game's uid: 32 hexadecimal digits from `now`, `salt` (what the caller knows of the game
/// and of itself: its process, its document) and a counter, unique in the process.
pub fn new_uid(now: u64, salt: &[u8]) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    hash_hex(&[b"game", &now.to_be_bytes(), &n.to_be_bytes(), salt])
}

/// Whether `uid` is one a file may hold: 1 to 64 letters, digits and `-`.
fn plain_uid(uid: &str) -> bool {
    !uid.is_empty() && uid.len() <= 64 && uid.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
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

/// Whether the comment `text` comes back as it is from the PGN this build writes and reads: its
/// words one space apart (the writer joins them so) and no `[%` command (read back as a drawing).
/// Decided from the text alone, never by reading the PGN back (plan A9-I3a).
fn comes_back(text: &str) -> bool {
    text.is_empty() || (!text.contains("[%") && text.split(' ').eq(text.split_whitespace()))
}

impl GameRecord {
    /// What `game` stores of itself, as game `uid` at revision `rev`: its PGN, the move shown,
    /// where its moves were read and the comments its PGN would not give back as they are. The
    /// board's side, a position that can't be played and the diagram it came from are the
    /// caller's to add. `None` when its PGN is too long to be read back.
    pub fn of_game(uid: String, rev: u64, game: &Game) -> Option<GameRecord> {
        let pgn = game.pgn();
        if pgn.len() > MAX_PGN_LEN {
            return None;
        }
        let mut sources = Vec::new();
        let mut comments = Vec::new();
        for node in nodes(game) {
            let Some(path) = game.path_of(node) else { continue };
            if let Some(s) = game.source(node).filter(|s| s.rect_user.iter().all(|v| v.is_finite())) {
                sources.push(MoveSource { path: path.indices().to_vec(), page: s.page, rect: s.rect_user, other: Map::new() });
            }
            // A comment before a move this build's reader may take for the comment after the move
            // before it (a first reply's, once a variation came between), so both are kept.
            let (after, before) = (game.comment(node), game.comment_before(node));
            let before_reply = game.replies(node).first().is_some_and(|&r| !game.comment_before(r).is_empty());
            if !before.is_empty() || before_reply || !comes_back(after) {
                comments.push(MoveComments { path: path.indices().to_vec(), after: after.to_owned(), before: before.to_owned() });
            }
        }
        let current = game.path_of(game.current()).map(|p| p.indices().to_vec()).unwrap_or_default();
        Some(GameRecord {
            uid,
            rev,
            deleted: false,
            pgn,
            current,
            flipped: false,
            illegal_fen: None,
            origin: None,
            sources,
            comments,
            other: Map::new(),
        })
    }

    /// A tombstone of game `uid` at revision `rev`.
    pub fn tombstone(uid: String, rev: u64) -> GameRecord {
        GameRecord {
            uid,
            rev,
            deleted: true,
            pgn: String::new(),
            current: Vec::new(),
            flipped: false,
            illegal_fen: None,
            origin: None,
            sources: Vec::new(),
            comments: Vec::new(),
            other: Map::new(),
        }
    }

    /// The game kept, at the move shown, and whether a comment kept was cut to the cap (a file
    /// written by hand, or by a build with a larger cap; plan A9-F3). `None` for a position that
    /// can't be played (`illegal_fen`), a tombstone, or a PGN that can't be read.
    pub fn game(&self) -> Option<(Game, bool)> {
        if self.deleted || self.illegal_fen.is_some() {
            return None;
        }
        let (mut game, _) = Game::from_pgn(&self.pgn).ok()?;
        let mut cut = false;
        for c in &self.comments {
            if let Some(node) = game.node_at(&NodePath::new(c.path.clone())) {
                cut |= game.set_comment(node, &c.after).is_err();
                cut |= game.set_comment_before(node, &c.before).is_err();
            }
        }
        for s in &self.sources {
            if let Some(node) = game.node_at(&NodePath::new(s.path.clone())) {
                game.set_source(node, Some(NodeSource { page: s.page, rect_user: s.rect }));
            }
        }
        if let Some(node) = game.node_at(&NodePath::new(self.current.clone())) {
            game.go_to_node(node);
        }
        Some((game, cut))
    }

    /// Why the record can't be kept: a uid that isn't one, a PGN or FEN past its cap, a diagram
    /// past the last page a document has.
    fn problem(&self) -> Option<&'static str> {
        if !plain_uid(&self.uid) {
            Some("a game without a uid")
        } else if self.pgn.len() > MAX_PGN_LEN {
            Some("a PGN too long")
        } else if self.illegal_fen.as_ref().is_some_and(|f| f.len() > MAX_TEXT) {
            Some("a position too long")
        } else if self.origin.is_some_and(|o| o.page > MAX_PAGES) {
            Some("a diagram past the last page")
        } else {
            None
        }
    }
}

impl GamesFile {
    /// An empty file for the document `doc`.
    pub fn empty(doc: DocRecord) -> GamesFile {
        GamesFile { version: VERSION, doc, instance: None, active: None, games: Vec::new(), trash: Vec::new(), taken: Vec::new(), other: Map::new() }
    }

    /// The games not deleted, in list order.
    pub fn live(&self) -> impl Iterator<Item = &GameRecord> {
        self.games.iter().filter(|g| !g.deleted)
    }

    /// The game `uid`, deleted or not.
    pub fn get(&self, uid: &str) -> Option<&GameRecord> {
        self.games.iter().find(|g| g.uid == uid)
    }

    /// The place of the game shown among the live ones (the first when it is none of them).
    pub fn active_index(&self) -> usize {
        self.active.as_deref().and_then(|a| self.live().position(|g| g.uid == a)).unwrap_or(0)
    }

    /// Take `theirs` in (another copy of the document's games): each game by its uid, the higher
    /// revision winning (a tie keeps this file's), a deletion as any change; the games only
    /// `theirs` has after this file's, in its order. The trash is the union of both, without the
    /// items either took out; the game shown is this file's while it is live. The uids of the
    /// games taken from `theirs` (added, or in place of this file's).
    pub fn merge(&mut self, theirs: &GamesFile) -> Vec<String> {
        let mut taken = Vec::new();
        for game in &theirs.games {
            match self.games.iter_mut().find(|g| g.uid == game.uid) {
                Some(mine) => {
                    if game.rev > mine.rev {
                        *mine = game.clone();
                        taken.push(game.uid.clone());
                    }
                }
                None => {
                    self.games.push(game.clone());
                    taken.push(game.uid.clone());
                }
            }
        }
        self.compact();
        for id in &theirs.taken {
            if !self.taken.contains(id) {
                self.taken.push(id.clone());
            }
        }
        let over = self.taken.len().saturating_sub(MAX_TAKEN);
        self.taken.drain(..over);
        for item in &theirs.trash {
            let id = item.id();
            if !self.trash.iter().any(|t| t.id() == id) {
                self.trash.push(item.clone());
            }
        }
        let gone: std::collections::HashSet<&String> = self.taken.iter().collect();
        self.trash.retain(|t| !gone.contains(&t.id()));
        self.trash.sort_by_key(|t| t.at);
        let over = self.trash.len().saturating_sub(MAX_TRASHED);
        self.trash.drain(..over);
        if !self.active.as_deref().is_some_and(|a| self.live().any(|g| g.uid == a)) {
            self.active.clone_from(&theirs.active);
        }
        taken
    }

    /// At most [`MAX_RECORDS`] games: past it, the tombstones of the lowest revisions go, then
    /// (a file of more live games than any build writes) nothing more.
    fn compact(&mut self) {
        let mut over = self.games.len().saturating_sub(MAX_RECORDS);
        if over == 0 {
            return;
        }
        let mut dead: Vec<(u64, usize)> = self.games.iter().enumerate().filter(|(_, g)| g.deleted).map(|(i, g)| (g.rev, i)).collect();
        dead.sort_unstable();
        let mut drop = vec![false; self.games.len()];
        for (_, i) in dead {
            if over == 0 {
                break;
            }
            if let Some(d) = drop.get_mut(i) {
                *d = true;
                over -= 1;
            }
        }
        let mut i = 0usize;
        self.games.retain(|_| {
            let keep = !drop.get(i).copied().unwrap_or(false);
            i = i.saturating_add(1);
            keep
        });
    }

    /// What `bytes` are: a file of this version, with the games and trash items that can't be
    /// read left out and counted; a newer version's, read as far as this build can, for reading
    /// only; or why they are no games file.
    pub fn parse(bytes: &[u8]) -> Parsed<GamesFile> {
        if bytes.len() > MAX_FILE_BYTES {
            return Parsed::Bad(format!("larger than {} MB", MAX_FILE_BYTES >> 20));
        }
        let value: Value = match serde_json::from_slice(bytes) {
            Ok(v) => v,
            Err(e) => return Parsed::Bad(format!("not JSON: {e}")),
        };
        let Value::Object(fields) = value else { return Parsed::Bad("not a chess games file".into()) };
        if fields.get("format").and_then(Value::as_str) != Some(FORMAT) {
            return Parsed::Bad("not a chess games file".into());
        }
        match fields.get("version").and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok()) {
            Some(VERSION) => match GamesFile::take(fields) {
                Ok((file, skipped)) => Parsed::File { file, skipped },
                Err(why) => Parsed::Bad(format!("not a chess games file of its version: {why}")),
            },
            Some(v) if v > VERSION => Parsed::Newer {
                version: v,
                file: GamesFile::take(fields).ok().map(|(mut f, _)| {
                    f.version = v;
                    f
                }),
            },
            _ => Parsed::Bad("no version this build reads in a .json file".into()),
        }
    }

    /// The file `fields` hold, and how many of its games and trash items were left out.
    fn take(mut fields: Map<String, Value>) -> Result<(GamesFile, usize), String> {
        fields.remove("format");
        fields.remove("version");
        let doc: DocRecord = serde_json::from_value(fields.remove("doc").ok_or("no document record")?).map_err(|e| e.to_string())?;
        let text = |v: Option<Value>| v.and_then(|v| v.as_str().map(str::to_owned)).filter(|t| t.len() <= MAX_TEXT);
        let instance = text(fields.remove("instance"));
        let active = text(fields.remove("active")).filter(|a| plain_uid(a));
        let list = |v: Option<Value>, what: &str| match v {
            None | Some(Value::Null) => Ok(Vec::new()),
            Some(Value::Array(items)) => Ok(items),
            Some(_) => Err(format!("the {what} are not a list")),
        };
        let games = list(fields.remove("games"), "games")?;
        if games.len() > MAX_RECORDS {
            return Err(format!("more than {MAX_RECORDS} games"));
        }
        let mut skipped = 0usize;
        let mut kept: Vec<GameRecord> = Vec::with_capacity(games.len());
        for v in games {
            let Some(mut g) = serde_json::from_value::<GameRecord>(v).ok().filter(|g| g.problem().is_none()) else {
                skipped = skipped.saturating_add(1);
                continue;
            };
            g.sources.retain(|s| s.rect.iter().all(|v| v.is_finite()));
            match kept.iter_mut().find(|k| k.uid == g.uid) {
                // The same game twice: the higher revision is the game.
                Some(k) => {
                    skipped = skipped.saturating_add(1);
                    if g.rev > k.rev {
                        *k = g;
                    }
                }
                None => kept.push(g),
            }
        }
        let trash = list(fields.remove("trash"), "trash items")?;
        let total = trash.len();
        let mut trash: Vec<Trashed> =
            trash.into_iter().filter_map(|v| serde_json::from_value::<Trashed>(v).ok()).filter(|t| t.pgn.len() <= MAX_TRASHED_PGN).collect();
        skipped = skipped.saturating_add(total.saturating_sub(trash.len()));
        let over = trash.len().saturating_sub(MAX_TRASHED);
        trash.drain(..over);
        let mut taken: Vec<String> = list(fields.remove("taken"), "items taken")?
            .into_iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .filter(|t| t.len() <= 64)
            .collect();
        let over = taken.len().saturating_sub(MAX_TAKEN);
        taken.drain(..over);
        Ok((GamesFile { version: VERSION, doc, instance, active, games: kept, trash, taken, other: fields }, skipped))
    }

    /// The file as bytes, in this version's layout; refused when it would be too big to read back.
    pub fn to_bytes(&self) -> Result<Vec<u8>, GamesStoreError> {
        let written = Written {
            format: FORMAT,
            version: VERSION,
            doc: &self.doc,
            instance: self.instance.as_deref(),
            active: self.active.as_deref(),
            games: &self.games,
            trash: &self.trash,
            taken: &self.taken,
            other: &self.other,
        };
        let bytes = serde_json::to_vec(&written).map_err(|_| GamesStoreError::TooBig)?;
        if bytes.len() > MAX_FILE_BYTES { Err(GamesStoreError::TooBig) } else { Ok(bytes) }
    }
}

/// The games as a kind of document store (plan A9 §2.3): `<hash>.games.json`, merged by
/// [`GamesFile::merge`].
pub struct Games;

impl StoreKind for Games {
    const KIND: &'static str = "games";
    const FORMAT: &'static str = FORMAT;
    const MAX_FILE_BYTES: usize = MAX_FILE_BYTES;
    /// No build before `games.flock` writes a `<hash>.games.json`.
    const OLD_LOCK: bool = false;
    type File = GamesFile;
    type Error = GamesStoreError;

    fn empty(doc: DocRecord) -> GamesFile {
        GamesFile::empty(doc)
    }

    fn doc(file: &GamesFile) -> &DocRecord {
        &file.doc
    }

    fn set_doc(file: &mut GamesFile, doc: DocRecord) {
        file.doc = doc;
    }

    fn read(bytes: &[u8]) -> Parsed<GamesFile> {
        GamesFile::parse(bytes)
    }

    fn to_bytes(file: &GamesFile) -> Result<Vec<u8>, GamesStoreError> {
        file.to_bytes()
    }

    fn holds_anything(file: &GamesFile) -> bool {
        !file.games.is_empty() || !file.trash.is_empty()
    }

    fn merge(mine: &mut GamesFile, theirs: &GamesFile) {
        mine.merge(theirs);
    }
}

/// The games in a folder, one file per document (desktop and command line).
#[cfg(not(target_arch = "wasm32"))]
pub type DirStore = DocStore<Games, super::store::DirBackend>;

/// The games in memory, within a byte budget of their own: the web build's store.
pub type MemStore = DocStore<Games, super::store::MemBackend>;

/// Who keeps a document's games ([`DocStore::keep`]).
pub struct Owner<'a> {
    /// The running PdfCraft keeping them (its lock file's name), which the file then names; `None`
    /// for a tool, which keeps the name the file has.
    pub me: Option<&'a str>,
    /// Whether the PdfCraft of that name is running (its lock is held).
    pub live: &'a dyn Fn(&str) -> bool,
    /// Merge into the games of another running PdfCraft rather than refuse.
    pub merge: bool,
}

/// What [`DocStore::keep`] did.
#[derive(Clone, Debug)]
pub struct Kept {
    /// The games kept, as merged with what the store had: what every copy now has.
    pub file: GamesFile,
    /// The uids of the games the store had that won over the ones given (added by another writer,
    /// or changed later there).
    pub theirs: Vec<String>,
}

impl<B: StoreBackend> DocStore<Games, B> {
    /// Keep `mine` for the document, merged under the store's lock with what it has
    /// ([`GamesFile::merge`], a tie keeping `mine`), so another writer's games are never lost. A
    /// file another running PdfCraft names is refused ([`GamesStoreError::OtherInstance`]) unless
    /// `owner.merge`; a newer version's always is.
    pub fn keep(
        &self,
        keys: &super::DocKeys,
        pages: usize,
        mine: GamesFile,
        owner: &Owner<'_>,
    ) -> Result<(Kept, Loaded<GamesFile>), GamesStoreError> {
        let (theirs, loaded) = self.update(keys, pages, |disk| {
            if let Some(other) = disk.instance.as_deref().filter(|o| owner.me != Some(*o))
                && !owner.merge
                && (owner.live)(other)
            {
                return Err(GamesStoreError::OtherInstance);
            }
            let instance = owner.me.map(str::to_owned).or_else(|| disk.instance.clone());
            let mut merged = mine;
            let theirs = merged.merge(disk);
            merged.instance = instance;
            *disk = merged;
            Ok::<_, GamesStoreError>(theirs)
        })?;
        Ok((Kept { file: loaded.file.clone(), theirs }, loaded))
    }
}

/// Whether the lock file `name` in `dir` is held: the running PdfCraft of that name is autosaving
/// (its instance lock in the recovery folder). Only a plain `instance-….lock` name is looked at, and
/// nothing is removed. `false` when it can't be told (no file locks there).
#[cfg(not(target_arch = "wasm32"))]
pub fn instance_alive(dir: &std::path::Path, name: &str) -> bool {
    let plain = name.starts_with("instance-")
        && name.ends_with(".lock")
        && !name.contains("..")
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.');
    if !plain {
        return false;
    }
    let Ok(file) = std::fs::File::options().read(true).write(true).open(dir.join(name)) else { return false };
    match file.try_lock() {
        Err(std::fs::TryLockError::WouldBlock) => true,
        Ok(()) => {
            let _ = file.unlock();
            false
        }
        Err(std::fs::TryLockError::Error(_)) => false,
    }
}

/// Version 1, `<cid>.games` (PGN toolbar spec §4.7, D10, kept until plan A9-G3b): read, to bring
/// its games in, never written.
pub mod v1 {
    use super::*;

    /// Longest first line read.
    pub const MAX_HEADER_BYTES: usize = 64 << 10;

    /// Largest file read: a header and a body at their caps.
    pub const MAX_FILE_BYTES: usize = (32 << 20) + MAX_HEADER_BYTES + 1;

    #[derive(Deserialize)]
    struct Header {
        format: String,
        version: u32,
        #[serde(default)]
        keys: StoredKeys,
    }

    /// The document's keys as version 1 kept them: the identifier in hex, the path in clear.
    #[derive(Default, Deserialize)]
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
        /// The keys read back; an identifier that isn't 32 hex digits is left out.
        fn keys(&self) -> super::super::DocKeys {
            let id0 = self.id0.as_deref().filter(|h| h.len() == 32 && h.is_ascii()).and_then(|h| {
                let mut id = [0u8; 16];
                for (i, b) in id.iter_mut().enumerate() {
                    *b = u8::from_str_radix(h.get(i.checked_mul(2)?..i.checked_mul(2)?.checked_add(2)?)?, 16).ok()?;
                }
                Some(id)
            });
            super::super::DocKeys { id0, path: self.path.clone(), name: self.name.clone(), size: self.size }
        }
    }

    #[derive(Deserialize)]
    struct Body {
        #[serde(default)]
        active: usize,
        #[serde(default)]
        games: Vec<Value>,
        #[serde(default)]
        trash: Vec<Value>,
    }

    #[derive(Deserialize)]
    struct Record {
        #[serde(default)]
        pgn: String,
        #[serde(default)]
        current: Vec<u16>,
        #[serde(default)]
        flipped: bool,
        #[serde(default)]
        illegal_fen: Option<String>,
        #[serde(default)]
        origin: Option<Origin>,
        #[serde(default)]
        sources: Vec<Source>,
        #[serde(default)]
        comments: Vec<MoveComments>,
    }

    #[derive(Clone, Copy, Deserialize)]
    struct Origin {
        page: usize,
        index: usize,
    }

    #[derive(Deserialize)]
    struct Source {
        path: Vec<u16>,
        page: u32,
        rect: [f32; 4],
    }

    /// A version 1 file's document and games: the keys it names the document by, and its games as
    /// this version keeps them, game `i` named `v1-<hash of the collection id and i>` at revision
    /// `rev` (the file's last change, in seconds since 1970), so bringing the same file in again
    /// changes nothing, and bringing it in once an earlier build changed it takes its games
    /// in place of the ones brought before, unless they were changed here since. The games and
    /// trash items that can't be read are left out and counted. `None` when it is no version 1
    /// file (a newer `.games` file is never one).
    pub fn read(bytes: &[u8], cid: &str, rev: u64) -> Option<(super::super::DocKeys, GamesFile, usize)> {
        if bytes.len() > MAX_FILE_BYTES {
            return None;
        }
        let at = bytes.iter().position(|&b| b == b'\n')?;
        let header: Header = serde_json::from_slice(bytes.get(..at)?).ok()?;
        if header.format != FORMAT || header.version != 1 {
            return None;
        }
        let rest = bytes.get(at.checked_add(1)?..)?;
        let body: Body = serde_json::from_slice(rest).ok()?;
        let keys = header.keys.keys();
        let mut file = GamesFile::empty(DocRecord::new(&keys, 0));
        let mut skipped = 0usize;
        for (i, v) in body.games.into_iter().take(MAX_RECORDS).enumerate() {
            let Ok(r) = serde_json::from_value::<Record>(v) else {
                skipped = skipped.saturating_add(1);
                continue;
            };
            let uid = format!("v1-{}", hash_hex(&[b"games v1", cid.as_bytes(), &u64::try_from(i).unwrap_or(u64::MAX).to_be_bytes()]));
            let record = GameRecord {
                uid,
                rev,
                deleted: false,
                pgn: r.pgn,
                current: r.current,
                flipped: r.flipped,
                illegal_fen: r.illegal_fen,
                origin: r.origin.map(|o| DiagramAt::new(o.page, o.index)),
                sources: r
                    .sources
                    .into_iter()
                    .filter(|s| s.rect.iter().all(|v| v.is_finite()))
                    .map(|s| MoveSource { path: s.path, page: s.page, rect: s.rect, other: Map::new() })
                    .collect(),
                comments: r.comments,
                other: Map::new(),
            };
            if record.problem().is_some() {
                skipped = skipped.saturating_add(1);
                continue;
            }
            if i == body.active {
                file.active = Some(record.uid.clone());
            }
            file.games.push(record);
        }
        let total = body.trash.len();
        file.trash =
            body.trash.into_iter().filter_map(|v| serde_json::from_value::<Trashed>(v).ok()).filter(|t| t.pgn.len() <= MAX_TRASHED_PGN).collect();
        skipped = skipped.saturating_add(total.saturating_sub(file.trash.len()));
        let over = file.trash.len().saturating_sub(MAX_TRASHED);
        file.trash.drain(..over);
        Some((keys, file, skipped))
    }

    /// Whether `name` is a version 1 file's: `<cid>.games`, the id letters, digits and `-`.
    pub fn file_cid(name: &str) -> Option<&str> {
        let cid = name.strip_suffix(".games")?;
        (!cid.is_empty() && cid.len() <= 64 && cid.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')).then_some(cid)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl DocStore<Games, super::store::DirBackend> {
    /// Bring in the games the builds before plan A9-G3b kept in the folder `from` (`<cid>.games`,
    /// version 1; [`v1::read`]): each file merged by [`GamesFile::merge`] into the file here that
    /// records the same document, else into a new `<hash>.games.json`. `from` is only read:
    /// nothing in it is moved, changed or deleted, and a build from before goes on working on it.
    /// Run again, a file whose size and time are what `migrated.json` recorded is not read again;
    /// one changed since is brought in again. A file of another version, or one that can't be
    /// read, is left where it is and counted, as is a file here that can't be written over. Under
    /// the store's lock.
    pub fn migrate_v1(&self, from: &std::path::Path) -> Result<super::store::Migration, GamesStoreError> {
        use super::store::{Migrated, MigratedSource, Migration};
        let mut out = Migration::default();
        let names: Vec<String> = match std::fs::read_dir(from) {
            Ok(entries) => entries
                .flatten()
                .filter_map(|e| e.file_name().to_str().map(str::to_owned))
                .filter(|n| v1::file_cid(n).is_some())
                .take(4 * super::store::MAX_FILES)
                .collect(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(e.into()),
        };
        if names.is_empty() || super::store::same_folder(from, self.dir()) {
            return Ok(out);
        }
        let mut names = names;
        names.sort();
        let _lock = self.backend().lock::<GamesStoreError>(<Games as StoreKind>::KIND, false)?;
        let mut record = Migrated::read(self.backend());
        let mut here = self.records()?;
        let place = from.canonicalize().unwrap_or_else(|_| from.to_path_buf()).to_string_lossy().into_owned();
        for name in names {
            let Some(cid) = v1::file_cid(&name) else { continue };
            let Ok(meta) = std::fs::metadata(from.join(&name)) else { continue };
            let (size, time) = (meta.len(), super::store::nanos(&meta));
            let key = hash_hex(&[b"migrated", place.as_bytes(), name.as_bytes()]);
            let seen = record.sources.iter().position(|s| s.source == key);
            if seen.and_then(|i| record.sources.get(i)).is_some_and(|s| s.size == size && s.time == time) {
                out.unchanged = out.unchanged.saturating_add(1);
                continue;
            }
            let bytes = super::corrections::read_capped(&from.join(&name), v1::MAX_FILE_BYTES)?;
            let Some((keys, theirs, skipped)) = v1::read(&bytes, cid, time / 1_000_000_000) else {
                out.unreadable = out.unreadable.saturating_add(1);
                continue;
            };
            out.skipped = out.skipped.saturating_add(skipped);
            let Some(dest) = here
                .iter()
                .find(|(_, doc)| doc.same_document(&theirs.doc))
                .map(|(n, _)| n.clone())
                .or_else(|| super::store::file_name(&keys, <Games as StoreKind>::KIND))
            else {
                continue;
            };
            let merged = match self.backend().read(&dest, MAX_FILE_BYTES)? {
                None => theirs,
                Some(mine) => match GamesFile::parse(&mine) {
                    Parsed::File { mut file, .. } => {
                        file.merge(&theirs);
                        file
                    }
                    Parsed::Newer { .. } | Parsed::Bad(_) => {
                        out.left_here = out.left_here.saturating_add(1);
                        continue;
                    }
                },
            };
            self.backend().write::<GamesStoreError>(&dest, &merged.to_bytes()?)?;
            if !here.iter().any(|(n, _)| *n == dest) {
                here.push((dest, merged.doc.clone()));
            }
            if seen.is_some() {
                out.changed = out.changed.saturating_add(1);
            } else {
                out.merged = out.merged.saturating_add(1);
            }
            record.keep(MigratedSource { source: key, size, time, hash: hash_hex(&[&bytes]) });
            record.write::<GamesStoreError>(self.backend())?;
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests;
