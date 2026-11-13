//! The book's PGN format of version 1, frozen: a literal copy of the serde structs and the reading
//! rule of `crates/engine/src/chess/reference_store.rs` at `d806fc73` (plan A9-L0b, the last
//! commit to write version 1; the format itself is plan "Carregar PGN do livro" R5's, decisions
//! R6e's), so the tests can do what a build of that version does with a file written now (plan A9
//! §2.10). Nothing here may change: it stands for builds already installed.
//!
//! Left out of the copy: the stores, the document keys' rules, `KeptPgn::new`/`read_from` and the
//! decisions' matching, which decide nothing about what such a build keeps of a file; its error
//! type is cut to what `to_bytes` and `decide` give, the PGN's size cap and the hash written out
//! here. Its store finds a document's file by `<hash>.json`, else by a file of version 1 whose
//! header records the document (`recorded`); keeps a file it reads as [`Read::Newer`] read only;
//! and sets a [`Read::Bad`] file aside at load (`reference_store.rs:452-466` there).

use serde::{Deserialize, Serialize};

/// `pdfcraft_chessvision::doc::reference::MAX_REFERENCE_BYTES` then.
pub const MAX_REFERENCE_BYTES: usize = 32 << 20;

/// The format's name, in every file.
pub const FORMAT: &str = "pdfcraft-chess-reference";

/// The version this build reads and writes.
pub const VERSION: u32 = 1;

/// Largest file read or written: the PGN's [`MAX_REFERENCE_BYTES`] as JSON, where a quote or a
/// line break takes two bytes, and the rest.
pub const MAX_FILE_BYTES: usize = 2 * MAX_REFERENCE_BYTES + (64 << 10);

/// Longest file name kept for the PGN, in bytes; a longer one is not kept.
const MAX_NAME: usize = 512;

/// Most decisions a document keeps.
pub const MAX_DECISIONS: usize = 20_000;

/// Longest SAN a decision names, in bytes.
const MAX_SAN: usize = 16;

/// What the user chose for a move in conflict with the PGN.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Choice {
    /// The book's move stands: checked, nothing to review.
    Book,
    /// The PGN's move is the right one.
    Pgn,
}

/// The user's decision on a move of the text: its page (0-based), block and place in the block,
/// its SAN as the walk wrote it, the SAN as read, and the choice.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    pub page: usize,
    pub block: usize,
    pub inline: usize,
    /// The move's SAN as the walk wrote it when the choice was made (what builds before plan
    /// A9-L0b match by, and still write).
    pub san: String,
    /// The move's SAN as read (`MoveInline::san_read`, plan A9-L0b), which a walk with other
    /// diagrams never changes; `None` in a decision made before it was kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub san_read: Option<String>,
    pub choice: Choice,
}

impl Decision {
    /// Whether `self` and `other` are decisions on the same move: the same address, and the
    /// same SAN read when both kept one, else the same SAN the walk wrote.
    fn same_move(&self, other: &Decision) -> bool {
        (self.page, self.block, self.inline) == (other.page, other.block, other.inline)
            && match (&self.san_read, &other.san_read) {
                (Some(a), Some(b)) => a == b,
                _ => self.san == other.san,
            }
    }
}

/// The PGN a document keeps.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeptPgn {
    /// The file name it came from, never its path.
    #[serde(default)]
    pub name: Option<String>,
    /// A hash of the text ([`text_hash`]).
    pub hash: String,
    pub bytes: usize,
    /// When it was kept, in seconds since 1970.
    pub at: u64,
    pub games: usize,
    pub moves: usize,
    pub text: String,
    /// The hash the readers before plan A9-F1 gave the same file (every byte that is not UTF-8
    /// a U+FFFD, the BOM kept), so a PGN they kept is still known as this one. Never written.
    #[serde(skip)]
    pub legacy_hash: Option<String>,
}

/// The hash a kept PGN's text has: 32 hexadecimal digits, to name and compare (not
/// cryptographic). `corrections::hash_hex(&[b"pgn", text])` written out.
pub fn text_hash(text: &str) -> String {
    let mut a = 0xcbf2_9ce4_8422_2325u64;
    let mut b = 0x6c62_272e_07bb_0142u64;
    let mut eat = |byte: u8| {
        a = (a ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        b = (b ^ u64::from(byte.rotate_left(3))).wrapping_mul(0x0100_0000_01b3);
    };
    for part in [&b"pgn"[..], text.as_bytes()] {
        for byte in u64::try_from(part.len()).unwrap_or(u64::MAX).to_be_bytes() {
            eat(byte);
        }
        for &byte in part {
            eat(byte);
        }
    }
    format!("{a:016x}{b:016x}")
}

/// The document a file belongs to (`corrections::DocRecord` then).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocRecord {
    /// The first file identifier, in hexadecimal.
    #[serde(default)]
    pub id0: Option<String>,
    /// A hash of the path, never the path.
    #[serde(default)]
    pub path_hash: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub pages: usize,
}

/// A document's kept PGN.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReferenceFile {
    pub format: String,
    pub version: u32,
    pub doc: DocRecord,
    pub pgn: KeptPgn,
    /// The user's decisions on moves in conflict with the PGN (R6e; optional, so a file without
    /// them reads as before).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decisions: Vec<Decision>,
}

/// What a file's bytes are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Read {
    File(ReferenceFile),
    /// A file of a newer version: kept as it is, never written or removed by this build. Its
    /// PGN, when this build can make it out, for reading.
    Newer(u32, Option<ReferenceFile>),
    /// Not a file this build can read, and why: set it aside, never delete it.
    Bad(String),
}

/// Why `to_bytes` or `decide` refused.
#[derive(Debug, PartialEq, Eq)]
pub enum ReferenceStoreError {
    TooBig,
    NoMove,
    TooManyDecisions,
}

impl ReferenceFile {
    /// Read a file's bytes: this version's file, a newer one (read only), or why not.
    pub fn read(bytes: &[u8]) -> Read {
        if bytes.len() > MAX_FILE_BYTES {
            return Read::Bad(format!("larger than {} MB", MAX_FILE_BYTES >> 20));
        }
        let value: serde_json::Value = match serde_json::from_slice(bytes) {
            Ok(v) => v,
            Err(e) => return Read::Bad(format!("not JSON: {e}")),
        };
        if value.get("format").and_then(serde_json::Value::as_str) != Some(FORMAT) {
            return Read::Bad("not a book's PGN file".into());
        }
        let version = value.get("version").and_then(serde_json::Value::as_u64).and_then(|v| u32::try_from(v).ok());
        match version {
            Some(VERSION) => {}
            Some(v) if v > VERSION => {
                let file = serde_json::from_value::<ReferenceFile>(value).ok().filter(|f| f.problem().is_none());
                return Read::Newer(v, file);
            }
            _ => return Read::Bad("no version this build reads".into()),
        }
        let file: ReferenceFile = match serde_json::from_value(value) {
            Ok(f) => f,
            Err(e) => return Read::Bad(format!("not a version {VERSION} file: {e}")),
        };
        match file.problem() {
            Some(why) => Read::Bad(why),
            None => Read::File(file),
        }
    }

    /// What makes the file one not to trust, when something does.
    fn problem(&self) -> Option<String> {
        let pgn = &self.pgn;
        if pgn.text.len() > MAX_REFERENCE_BYTES {
            return Some(format!("a PGN larger than {} MB", MAX_REFERENCE_BYTES >> 20));
        }
        if pgn.bytes != pgn.text.len() || pgn.hash != text_hash(&pgn.text) {
            return Some("the PGN is not the one it was kept as (its size or hash differs)".into());
        }
        if pgn.name.as_ref().is_some_and(|n| n.len() > MAX_NAME) {
            return Some("the PGN's file name is too long".into());
        }
        if self.decisions.len() > MAX_DECISIONS {
            return Some(format!("more than {MAX_DECISIONS} decisions"));
        }
        if self.decisions.iter().any(|d| d.san.is_empty() || d.san.len() > MAX_SAN || d.san_read.as_ref().is_some_and(|s| s.len() > MAX_SAN)) {
            return Some("a decision names no move".into());
        }
        None
    }

    /// Keep `decision` in place of the one on the same move. Refused past [`MAX_DECISIONS`].
    pub fn decide(&mut self, decision: Decision) -> Result<(), ReferenceStoreError> {
        if decision.san.is_empty() || decision.san.len() > MAX_SAN || decision.san_read.as_ref().is_some_and(|s| s.len() > MAX_SAN) {
            return Err(ReferenceStoreError::NoMove);
        }
        self.decisions.retain(|d| !d.same_move(&decision));
        if self.decisions.len() >= MAX_DECISIONS {
            return Err(ReferenceStoreError::TooManyDecisions);
        }
        self.decisions.push(decision);
        Ok(())
    }

    /// The file as bytes, refused when it would be too big to read back.
    pub fn to_bytes(&self) -> Result<Vec<u8>, ReferenceStoreError> {
        let bytes = serde_json::to_vec(self).map_err(|_| ReferenceStoreError::TooBig)?;
        if bytes.len() > MAX_FILE_BYTES { Err(ReferenceStoreError::TooBig) } else { Ok(bytes) }
    }
}

/// The document a file's bytes record, read without the PGN's hash checked: enough to find a
/// document's file among the others without hashing each PGN. `None` for bytes that are not a
/// file of this version.
pub fn recorded(bytes: &[u8]) -> Option<DocRecord> {
    #[derive(Deserialize)]
    struct Head {
        format: String,
        version: u32,
        doc: DocRecord,
    }
    if bytes.len() > MAX_FILE_BYTES {
        return None;
    }
    let head: Head = serde_json::from_slice(bytes).ok()?;
    (head.format == FORMAT && head.version == VERSION).then_some(head.doc)
}
