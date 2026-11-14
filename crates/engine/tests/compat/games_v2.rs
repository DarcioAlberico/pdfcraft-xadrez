//! The games' format of version 2 as plan A9-G3b's first PR wrote it, frozen: a literal copy of
//! the serde structs and the reading and writing of `crates/engine/src/chess/games_store.rs` at
//! `4c886889` (the commit that introduced version 2), so a test can do what a build of it does with
//! a file written now (plan A9 §2.10). Nothing here may change: it stands for builds already made.
//!
//! Left out of the copy: the stores, the merge, the conversion to a game and version 1; the error
//! type is cut to what `to_bytes` gives, `Parsed` to what `parse` gives, and the caps and the
//! document record are written out here.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const FORMAT: &str = "pdfcraft-chess-games";
pub const VERSION: u32 = 2;
pub const MAX_GAMES: usize = 256;
pub const MAX_RECORDS: usize = 8 * MAX_GAMES;
pub const MAX_FILE_BYTES: usize = (32 << 20) + (64 << 10);
pub const MAX_TRASHED: usize = 30;
pub const MAX_TRASHED_PGN: usize = 256 << 10;
const MAX_TAKEN: usize = 8 * MAX_TRASHED;
const MAX_TEXT: usize = 512;
const MAX_PAGES: usize = 1 << 20;
const MAX_PGN_LEN: usize = 1 << 20;

/// `corrections::DocRecord` then.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocRecord {
    #[serde(default)]
    pub id0: Option<String>,
    #[serde(default)]
    pub path_hash: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub pages: usize,
}

/// `chessvision::doc::DiagramAt` then.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagramAt {
    pub page: usize,
    #[serde(rename = "diagram")]
    pub index: usize,
}

#[derive(Debug)]
pub enum Parsed<F> {
    File { file: F, skipped: usize },
    Newer { version: u32, file: Option<F> },
    Bad(String),
}

#[derive(Debug)]
pub enum GamesStoreError {
    TooBig,
}

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

/// Whether `uid` is one a file may hold: 1 to 64 letters, digits and `-`.
fn plain_uid(uid: &str) -> bool {
    !uid.is_empty() && uid.len() <= 64 && uid.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

impl GameRecord {
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
