//! The corrections format of version 1, frozen: a literal copy of the serde structs and the
//! reading rule of `crates/engine/src/chess/corrections.rs` at `801e3abf` (plan A8, P3a1), the
//! commit that introduced it, so the tests can do what a build of that version does with a file
//! written now (plan A9 §2.10). Nothing here may change: it stands for builds already installed.
//!
//! Left out of the copy: the document keys' rules (`DocRecord::new`, `matches`), merging, the
//! stores and the fingerprints, which decide nothing about what such a build keeps of a file. Its
//! store keeps a file it reads as [`Read::Newer`] read only: `loaded` sets `read_only`, and
//! `changed` refuses to write it (`corrections.rs:525-551` there).

use serde::{Deserialize, Serialize};

/// The format's name, in every file.
pub const FORMAT: &str = "pdfcraft-chess-corrections";

/// The format's version. A newer one is read only; another is set aside.
pub const VERSION: u32 = 1;

/// Most entries (tombstones included) one document keeps.
pub const MAX_ENTRIES: usize = 20_000;

/// Largest file read or written.
pub const MAX_FILE_BYTES: usize = 4 << 20;

/// Longest FEN, uid, model key or document name kept.
const MAX_TEXT: usize = 512;

/// What a correction says about the reading.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// The reading was right, and the user said so.
    Verified,
    /// The user changed the position.
    Corrected,
}

/// The reading a correction replaced.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Original {
    /// The FEN read, when the diagram was read.
    #[serde(default)]
    pub fen: Option<String>,
    /// The piece model that read it, by its key.
    #[serde(default)]
    pub model: Option<String>,
}

/// One diagram's correction.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Correction {
    pub uid: String,
    /// The page, 0-based, when the correction was last written.
    pub page: usize,
    /// That page's fingerprint then ([`page_fingerprint`]).
    pub page_fp: String,
    /// The diagram's box, normalised to the page's crop box: `[x0, y0, x1, y1]` in 0..1, y up.
    pub rect: [f32; 4],
    #[serde(default)]
    pub orig: Option<Original>,
    /// The position, a full FEN.
    pub fen: String,
    pub kind: Kind,
    /// When it was last written, in seconds since 1970.
    pub at: u64,
    /// Its revision: each change adds one, and the higher one wins a merge.
    pub rev: u64,
    #[serde(default)]
    pub deleted: bool,
}

/// The document a file belongs to.
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

/// A document's corrections.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CorrectionFile {
    pub format: String,
    pub version: u32,
    pub doc: DocRecord,
    #[serde(default)]
    pub entries: Vec<Correction>,
}

/// What a file's bytes are.
#[derive(Clone, Debug, PartialEq)]
pub enum Read {
    /// A file of this version.
    File(CorrectionFile),
    /// A file of a newer version: kept as it is, never written by this build. Its entries, when
    /// this build can make them out (fields it doesn't know are skipped), for reading.
    Newer(u32, Option<CorrectionFile>),
    /// Not a corrections file this build can read, and why: set it aside, never delete it.
    Bad(String),
}

/// Why a change was refused (only what [`CorrectionFile::to_bytes`] gives, here).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CorrectionError {
    TooBig,
}

impl CorrectionFile {
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
            return Read::Bad("not a corrections file".into());
        }
        let version = value.get("version").and_then(serde_json::Value::as_u64).and_then(|v| u32::try_from(v).ok());
        match version {
            Some(VERSION) => {}
            Some(v) if v > VERSION => {
                let file = serde_json::from_value::<CorrectionFile>(value).ok().filter(|f| f.problem().is_none());
                return Read::Newer(v, file);
            }
            _ => return Read::Bad("no version this build reads".into()),
        }
        let file: CorrectionFile = match serde_json::from_value(value) {
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
        if self.entries.len() > MAX_ENTRIES {
            return Some(format!("more than {MAX_ENTRIES} entries"));
        }
        self.entries.iter().find_map(|e| {
            let long = [&e.uid, &e.page_fp, &e.fen].iter().any(|s| s.is_empty() || s.len() > MAX_TEXT);
            if long || !rect_ok(e.rect) { Some(format!("entry {} is not a correction", e.uid.chars().take(64).collect::<String>())) } else { None }
        })
    }

    /// The file as bytes, refused when it would be too big to read back.
    pub fn to_bytes(&self) -> Result<Vec<u8>, CorrectionError> {
        let bytes = serde_json::to_vec(self).map_err(|_| CorrectionError::TooBig)?;
        if bytes.len() > MAX_FILE_BYTES { Err(CorrectionError::TooBig) } else { Ok(bytes) }
    }
}

/// A box in 0..1 with its corners in order and some area.
fn rect_ok(r: [f32; 4]) -> bool {
    r.iter().all(|v| v.is_finite() && (-0.01..=1.01).contains(v)) && r[0] < r[2] && r[1] < r[3]
}
