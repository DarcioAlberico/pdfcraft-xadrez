//! The corrections format of version 2, frozen: a literal copy of the serde structs and the
//! reading rule of `crates/engine/src/chess/corrections.rs` at `7e4770f9` (plan A9-G2a), the
//! commit that introduced it, so the tests can do what a build of that version does with a file
//! written now (plan A9 §2.10). Nothing here may change: it stands for builds already installed.
//!
//! Left out of the copy: the document keys' rules, merging, the stores, the fingerprints and the
//! matching of corrections to diagrams, which decide nothing about what such a build keeps of a
//! file; and its error type is cut to what `to_bytes` gives. Its store keeps a file it reads as
//! [`Read::Newer`] read only, and sets a [`Read::Bad`] file aside (`corrections.rs:924-957` there).

use serde::{Deserialize, Serialize};

/// The format's name, in every file.
pub const FORMAT: &str = "pdfcraft-chess-corrections";

/// The format's version. A newer one is read only; an older one is read and written again as this
/// one; another is set aside. Version 2 (plan A9, G2a) records each correction's neighbouring
/// pages and keeps the fingerprints in one table; a build of version 1 opens it read only.
pub const VERSION: u32 = 2;

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
#[derive(Clone, Debug, PartialEq)]
pub struct Correction {
    pub uid: String,
    /// The page, 0-based, when the correction was last written.
    pub page: usize,
    /// That page's fingerprint then (`page_fingerprint`).
    pub page_fp: String,
    /// The fingerprints of the pages before and after it then; `None` where there was no page
    /// (the first page, the last), and for an entry of version 1, which did not record them.
    pub prev_fp: Option<String>,
    pub next_fp: Option<String>,
    /// How many pages the document had then; 0 for an entry of version 1.
    pub page_count: usize,
    /// The diagram's box, normalised to the page's crop box: `[x0, y0, x1, y1]` in 0..1, y up.
    pub rect: [f32; 4],
    pub orig: Option<Original>,
    /// The position, a full FEN.
    pub fen: String,
    pub kind: Kind,
    /// When it was last written, in seconds since 1970.
    pub at: u64,
    /// Its revision: each change adds one, and the higher one wins a merge.
    pub rev: u64,
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

/// A document's corrections. Read and written by [`CorrectionFile::read`] and
/// [`CorrectionFile::to_bytes`], in the layout of [`VERSION`].
#[derive(Clone, Debug, PartialEq)]
pub struct CorrectionFile {
    pub format: String,
    /// [`VERSION`], or the newer version of a file read only ([`Read::Newer`]).
    pub version: u32,
    pub doc: DocRecord,
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
    /// Read a file's bytes: this version's file (an older version's taken in as this one), a
    /// newer one (read only), or why not.
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
        let file = match version {
            Some(1) => serde_json::from_value::<disk::FileV1>(value).map_err(|e| e.to_string()).map(disk::FileV1::into_file),
            Some(VERSION) => serde_json::from_value::<disk::FileV2>(value).map_err(|e| e.to_string()).and_then(disk::FileV2::into_file),
            Some(v) if v > VERSION => {
                // Read in this version's layout, fields this build doesn't know left out.
                let file = serde_json::from_value::<disk::FileV2>(value).ok().and_then(|f| f.into_file().ok()).filter(|f| f.problem().is_none());
                return Read::Newer(v, file);
            }
            _ => return Read::Bad("no version this build reads".into()),
        };
        let file = match file {
            Ok(f) => f,
            Err(e) => return Read::Bad(format!("not a corrections file of its version: {e}")),
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
        let text_ok = |s: &str| !s.is_empty() && s.len() <= MAX_TEXT;
        self.entries.iter().find_map(|e| {
            let texts = [&e.uid, &e.page_fp, &e.fen].iter().all(|s| text_ok(s));
            let neighbours = [&e.prev_fp, &e.next_fp].iter().all(|n| n.as_deref().is_none_or(text_ok));
            if texts && neighbours && rect_ok(e.rect) {
                None
            } else {
                Some(format!("entry {} is not a correction", e.uid.chars().take(64).collect::<String>()))
            }
        })
    }

    /// The file as bytes, in this version's layout, refused when it would be too big to read back.
    pub fn to_bytes(&self) -> Result<Vec<u8>, CorrectionError> {
        let bytes = serde_json::to_vec(&disk::FileV2::of(self)).map_err(|_| CorrectionError::TooBig)?;
        if bytes.len() > MAX_FILE_BYTES { Err(CorrectionError::TooBig) } else { Ok(bytes) }
    }
}

/// A box in 0..1 with its corners in order and some area.
fn rect_ok(r: [f32; 4]) -> bool {
    r.iter().all(|v| v.is_finite() && (-0.01..=1.01).contains(v)) && r[0] < r[2] && r[1] < r[3]
}

/// The files as stored. Version 1 kept each page fingerprint in every entry; version 2 keeps each
/// once, in `fps`, an entry naming its page's and its neighbours' by their place there, and leaves
/// out what it can (`orig` when it is the position, `deleted` when false).
mod disk {
    use serde::{Deserialize, Deserializer, Serialize};

    use super::{Correction, CorrectionFile, DocRecord, Kind, MAX_ENTRIES, Original};

    /// Version 1, as `801e3abf` (plan A8, P3a1) wrote it.
    #[derive(Deserialize)]
    pub(super) struct FileV1 {
        format: String,
        doc: DocRecord,
        #[serde(default)]
        entries: Vec<EntryV1>,
    }

    #[derive(Deserialize)]
    struct EntryV1 {
        uid: String,
        page: usize,
        page_fp: String,
        rect: [f32; 4],
        #[serde(default)]
        orig: Option<Original>,
        fen: String,
        kind: Kind,
        at: u64,
        rev: u64,
        #[serde(default)]
        deleted: bool,
    }

    impl FileV1 {
        /// Taken in as this version's file, the neighbours unknown.
        pub(super) fn into_file(self) -> CorrectionFile {
            let entries = self
                .entries
                .into_iter()
                .map(|e| Correction {
                    uid: e.uid,
                    page: e.page,
                    page_fp: e.page_fp,
                    prev_fp: None,
                    next_fp: None,
                    page_count: 0,
                    rect: e.rect,
                    orig: e.orig,
                    fen: e.fen,
                    kind: e.kind,
                    at: e.at,
                    rev: e.rev,
                    deleted: e.deleted,
                })
                .collect();
            CorrectionFile { format: self.format, version: super::VERSION, doc: self.doc, entries }
        }
    }

    /// Version 2 (plan A9, G2a).
    #[derive(Serialize, Deserialize)]
    pub(super) struct FileV2 {
        format: String,
        version: u32,
        doc: DocRecord,
        /// Each page fingerprint the entries name, once.
        #[serde(default)]
        fps: Vec<String>,
        #[serde(default)]
        entries: Vec<EntryV2>,
    }

    #[derive(Serialize, Deserialize)]
    struct EntryV2 {
        uid: String,
        page: usize,
        /// The document's page count then; 0 when not known (an entry of version 1 never placed
        /// again).
        #[serde(default)]
        pages: usize,
        /// The page's fingerprint and its neighbours' (before, page, after), as places in `fps`.
        fp: (Option<u32>, u32, Option<u32>),
        rect: [f32; 4],
        fen: String,
        /// Left out: the reading was the position (`fen`); `null`: not known.
        #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
        orig: Option<Option<Original>>,
        kind: Kind,
        at: u64,
        rev: u64,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        deleted: bool,
    }

    /// The fingerprints of a file being written, each once, with its place.
    #[derive(Default)]
    struct Table<'a> {
        fps: Vec<String>,
        places: std::collections::HashMap<&'a str, u32>,
    }

    impl<'a> Table<'a> {
        /// `fp`'s place in the table, put in when new.
        fn place(&mut self, fp: &'a str) -> u32 {
            if let Some(at) = self.places.get(fp) {
                return *at;
            }
            // At most three per entry, and a file's entries are capped: always within a u32.
            let at = u32::try_from(self.fps.len()).unwrap_or(u32::MAX);
            self.fps.push(fp.to_owned());
            self.places.insert(fp, at);
            at
        }
    }

    /// A field that is there, `null` or not: `Some`, so that `None` means left out.
    fn present<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Option<Original>>, D::Error> {
        Option::<Original>::deserialize(d).map(Some)
    }

    impl FileV2 {
        /// `file` in this layout.
        pub(super) fn of(file: &CorrectionFile) -> FileV2 {
            let mut table = Table::default();
            let mut entries = Vec::with_capacity(file.entries.len());
            for e in &file.entries {
                let fp = (e.prev_fp.as_deref().map(|f| table.place(f)), table.place(&e.page_fp), e.next_fp.as_deref().map(|f| table.place(f)));
                let orig = match &e.orig {
                    Some(o) if o.model.is_none() && o.fen.as_deref() == Some(e.fen.as_str()) => None,
                    other => Some(other.clone()),
                };
                entries.push(EntryV2 {
                    uid: e.uid.clone(),
                    page: e.page,
                    pages: e.page_count,
                    fp,
                    rect: e.rect,
                    fen: e.fen.clone(),
                    orig,
                    kind: e.kind,
                    at: e.at,
                    rev: e.rev,
                    deleted: e.deleted,
                });
            }
            FileV2 { format: file.format.clone(), version: super::VERSION, doc: file.doc.clone(), fps: table.fps, entries }
        }

        /// The file these bytes held, or why not: a place past the table.
        pub(super) fn into_file(self) -> Result<CorrectionFile, String> {
            if self.fps.len() > MAX_ENTRIES.saturating_mul(3) || self.entries.len() > MAX_ENTRIES {
                return Err(format!("more than {MAX_ENTRIES} entries"));
            }
            let fps = &self.fps;
            let fp = |at: u32| usize::try_from(at).ok().and_then(|i| fps.get(i)).cloned().ok_or_else(|| format!("no fingerprint {at}"));
            let mut entries = Vec::with_capacity(self.entries.len());
            for e in self.entries {
                let (prev, own, next) = e.fp;
                let orig = match e.orig {
                    None => Some(Original { fen: Some(e.fen.clone()), model: None }),
                    Some(o) => o,
                };
                entries.push(Correction {
                    uid: e.uid,
                    page: e.page,
                    page_fp: fp(own)?,
                    prev_fp: prev.map(fp).transpose()?,
                    next_fp: next.map(fp).transpose()?,
                    page_count: e.pages,
                    rect: e.rect,
                    orig,
                    fen: e.fen,
                    kind: e.kind,
                    at: e.at,
                    rev: e.rev,
                    deleted: e.deleted,
                });
            }
            Ok(CorrectionFile { format: self.format, version: self.version, doc: self.doc, entries })
        }
    }
}
