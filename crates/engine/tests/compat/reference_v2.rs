//! The book's PGN format of version 2 as plan A9-G3a's second PR wrote it, frozen: a literal copy
//! of the serde structs and the reading, keeping and merging rules of
//! `crates/engine/src/chess/reference_store.rs` at `33d3b732` (the commit that introduced version 2),
//! so the tests can do what a build of it does with a file written now (plan A9 §2.10). Nothing
//! here may change: it stands for builds already installed.
//!
//! Left out of the copy: the stores, the document keys' rules, `KeptPgn::new`/`read_from`/
//! `is_same_as` and the decisions' matching, which decide nothing about what such a build keeps of
//! a file; its error type is cut to what `to_bytes` and `decide` give, its `Parsed` to what
//! `parse` gives, and the PGN's size cap, the document record and the hash are written out here.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// `pdfcraft_chessvision::doc::reference::MAX_REFERENCE_BYTES` then.
pub const MAX_REFERENCE_BYTES: usize = 32 << 20;

pub const FORMAT: &str = "pdfcraft-chess-reference";

/// The version this build writes. Version 1 is read and written again as this one; a newer one
/// is read only. Version 2 (plan A9-G3a) is named `<hash>.reference.json`, can say the PGN was
/// removed, and keeps fields a later build of it writes.
pub const VERSION: u32 = 2;

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
    /// Fields a later build of this version wrote, kept as they are while the decision is.
    #[serde(flatten)]
    pub other: Map<String, Value>,
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

    /// Whether it names a move this build keeps: a SAN of 1 to [`MAX_SAN`] bytes, the SAN read
    /// no longer.
    fn names_a_move(&self) -> bool {
        !self.san.is_empty() && self.san.len() <= MAX_SAN && self.san_read.as_ref().is_none_or(|s| s.len() <= MAX_SAN)
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
    /// Fields a later build of this version wrote, kept as they are while the PGN is.
    #[serde(flatten)]
    pub other: Map<String, Value>,
}

impl KeptPgn {
    /// What makes it a PGN not to trust, when something does.
    fn problem(&self) -> Option<String> {
        if self.text.len() > MAX_REFERENCE_BYTES {
            return Some(format!("a PGN larger than {} MB", MAX_REFERENCE_BYTES >> 20));
        }
        if self.bytes != self.text.len() || self.hash != text_hash(&self.text) {
            return Some("the PGN is not the one it was kept as (its size or hash differs)".into());
        }
        if self.name.as_ref().is_some_and(|n| n.len() > MAX_NAME) {
            return Some("the PGN's file name is too long".into());
        }
        None
    }
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

/// A document's kept PGN, its removal, and the decisions made against it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReferenceFile {
    pub format: String,
    /// [`VERSION`], or the newer version of a file read only.
    pub version: u32,
    pub doc: DocRecord,
    /// The PGN kept; `None` when there is none, or it was removed (`removed`).
    pub pgn: Option<KeptPgn>,
    /// When the PGN was removed, in seconds since 1970.
    pub removed: Option<u64>,
    /// The user's decisions on moves in conflict with the PGN (R6e).
    pub decisions: Vec<Decision>,
    /// Fields a later build of this version wrote, kept as they are.
    pub other: Map<String, Value>,
}

/// What a file's bytes are, for its kind (`store::Parsed` then).
#[derive(Clone, Debug, PartialEq)]
pub enum Parsed<F> {
    File { file: F, skipped: usize },
    Newer { version: u32, file: Option<F> },
    Bad(String),
}

/// A file as written.
#[derive(Serialize)]
struct Written<'a> {
    format: &'a str,
    version: u32,
    doc: &'a DocRecord,
    #[serde(skip_serializing_if = "Option::is_none")]
    pgn: Option<&'a KeptPgn>,
    #[serde(skip_serializing_if = "Option::is_none")]
    removed: Option<u64>,
    #[serde(skip_serializing_if = "<[Decision]>::is_empty")]
    decisions: &'a [Decision],
    #[serde(flatten)]
    other: &'a Map<String, Value>,
}

/// Why `to_bytes` or `decide` refused.
#[derive(Debug, PartialEq, Eq)]
pub enum ReferenceStoreError {
    TooBig,
    NoMove,
    TooManyDecisions,
}

impl ReferenceFile {
    /// Read a file's bytes, decision by decision: one that can't be read (of the wrong shape,
    /// naming no move) is left out and counted, and the rest kept. What makes the whole file
    /// unreadable: not JSON, another format, no version this build reads, no document record, a
    /// PGN not to trust (its text not the one it was kept as), a version 1 file without one, more
    /// than [`MAX_DECISIONS`] decisions.
    pub fn parse(bytes: &[u8]) -> Parsed<ReferenceFile> {
        if bytes.len() > MAX_FILE_BYTES {
            return Parsed::Bad(format!("larger than {} MB", MAX_FILE_BYTES >> 20));
        }
        let value: Value = match serde_json::from_slice(bytes) {
            Ok(v) => v,
            Err(e) => return Parsed::Bad(format!("not JSON: {e}")),
        };
        let Value::Object(fields) = value else { return Parsed::Bad("not a book's PGN file".into()) };
        if fields.get("format").and_then(Value::as_str) != Some(FORMAT) {
            return Parsed::Bad("not a book's PGN file".into());
        }
        let version = fields.get("version").and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok());
        match version {
            Some(v @ 1..=VERSION) => match ReferenceFile::take(fields, v) {
                Ok((file, skipped)) => Parsed::File { file, skipped },
                Err(why) => Parsed::Bad(format!("not a book's PGN file of its version: {why}")),
            },
            // Read in this version's layout, the fields this build doesn't know kept.
            Some(v) if v > VERSION => Parsed::Newer {
                version: v,
                file: ReferenceFile::take(fields, v).ok().map(|(mut f, _)| {
                    f.version = v;
                    f
                }),
            },
            _ => Parsed::Bad("no version this build reads".into()),
        }
    }

    /// The file `fields` hold, of version `version`, and how many of its decisions were left out.
    fn take(mut fields: Map<String, Value>, version: u32) -> Result<(ReferenceFile, usize), String> {
        fields.remove("format");
        fields.remove("version");
        let doc: DocRecord = serde_json::from_value(fields.remove("doc").ok_or("no document record")?).map_err(|e| e.to_string())?;
        let pgn = match fields.remove("pgn") {
            None | Some(Value::Null) if version >= 2 => None,
            None | Some(Value::Null) => return Err("no PGN".into()),
            Some(v) => Some(serde_json::from_value::<KeptPgn>(v).map_err(|e| e.to_string())?),
        };
        if let Some(why) = pgn.as_ref().and_then(KeptPgn::problem) {
            return Err(why);
        }
        let removed = fields.remove("removed").and_then(|v| v.as_u64());
        let decisions = match fields.remove("decisions") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(list)) => list,
            Some(_) => return Err("the decisions are not a list".into()),
        };
        if decisions.len() > MAX_DECISIONS {
            return Err(format!("more than {MAX_DECISIONS} decisions"));
        }
        let total = decisions.len();
        let decisions: Vec<Decision> =
            decisions.into_iter().filter_map(|v| serde_json::from_value::<Decision>(v).ok()).filter(Decision::names_a_move).collect();
        let skipped = total.saturating_sub(decisions.len());
        Ok((ReferenceFile { format: FORMAT.into(), version: VERSION, doc, pgn, removed, decisions, other: fields }, skipped))
    }

    /// Keep `decision` in place of the one on the same move. Refused past [`MAX_DECISIONS`].
    pub fn decide(&mut self, decision: Decision) -> Result<(), ReferenceStoreError> {
        if !decision.names_a_move() {
            return Err(ReferenceStoreError::NoMove);
        }
        self.decisions.retain(|d| !d.same_move(&decision));
        if self.decisions.len() >= MAX_DECISIONS {
            return Err(ReferenceStoreError::TooManyDecisions);
        }
        self.decisions.push(decision);
        Ok(())
    }

    /// The file as bytes, in this version's layout, refused when it would be too big to read back.
    pub fn to_bytes(&self) -> Result<Vec<u8>, ReferenceStoreError> {
        let written = Written {
            format: FORMAT,
            version: VERSION,
            doc: &self.doc,
            pgn: self.pgn.as_ref(),
            removed: self.removed,
            decisions: &self.decisions,
            other: &self.other,
        };
        let bytes = serde_json::to_vec(&written).map_err(|_| ReferenceStoreError::TooBig)?;
        if bytes.len() > MAX_FILE_BYTES { Err(ReferenceStoreError::TooBig) } else { Ok(bytes) }
    }
}
