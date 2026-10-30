//! Chess ▸ Diagrams: the corrections a user made to diagram readings, kept on disk so they
//! outlive an edit, a new scan, saving, closing and reopening (plan A8, P3a1).
//!
//! **What names a correction.** The document by its [`DocKeys`] (any one key is enough, as for
//! the game collections), then each correction by its page index, the page's **fingerprint**
//! ([`page_fingerprint`]: its content streams and its images, as stored, so it survives saving)
//! and the diagram's box normalised to the page's crop box. A diagram is the correction's when
//! their boxes overlap by an intersection over union of at least [`MATCH_IOU`]. A page that moved
//! (pages inserted or removed before it) is found again by its fingerprint ([`Located`]).
//!
//! **Format** (version [`VERSION`]), one JSON file per document:
//! `{"format":"pdfcraft-chess-corrections","version":1,"doc":{…},"entries":[…]}`. The document
//! is recorded by its identifier, the hash of its path (never the path itself), its name, size and
//! page count; no text or picture of the document is kept. Each entry has a `uid`, the page and
//! its fingerprint, the normalised box, the reading it replaced (`orig`), the position (`fen`),
//! its `kind` (verified: the reading was right; corrected), when (`at`, seconds since 1970) and a
//! revision `rev`. Deleting leaves a tombstone (`deleted`), so a merge never brings it back.
//!
//! **Merging and safety.** Two copies merge entry by entry, the higher revision winning
//! ([`CorrectionFile::merge`]). A file of a newer version is read but never written; one that
//! can't be read is set aside as `….bad`, never deleted ([`Read`]). Files are kept through a
//! [`CorrectionStore`]: the folder store ([`DirStore`], desktop and command line) reads, changes
//! and writes a file under a lock file, through a staging file renamed over it; the memory store
//! ([`MemStore`], the web build) keeps them within a byte budget of their own.

use serde::{Deserialize, Serialize};

use super::keys::DocKeys;
use crate::Document;

/// The format's name, in every file.
pub const FORMAT: &str = "pdfcraft-chess-corrections";

/// The format's version. A newer one is read only; another is set aside.
pub const VERSION: u32 = 1;

/// Most entries (tombstones included) one document keeps.
pub const MAX_ENTRIES: usize = 20_000;

/// Largest file read or written.
pub const MAX_FILE_BYTES: usize = 4 << 20;

/// Least intersection over union for a diagram to be a correction's.
pub const MATCH_IOU: f32 = 0.7;

/// Longest FEN, uid, model key or document name kept.
const MAX_TEXT: usize = 512;

/// Most bytes of one stream hashed into a page's fingerprint (its length is hashed too).
const MAX_HASHED_STREAM: usize = 1 << 20;

/// Most streams of one page hashed.
const MAX_HASHED_STREAMS: usize = 256;

/// What a correction says about the reading.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// The reading was right, and the user said so.
    Verified,
    /// The user changed the position.
    Corrected,
}

/// What became of a diagram, as the Chess diagrams panel shows it and `chess_diagram_states`
/// counts it (plan A8, P2a2): what the user did with it outranks the reading.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Status {
    /// Read with confidence (the reading passed the gate).
    Read,
    /// Read, with a doubt on some square or a repair: to review.
    Review,
    /// Not read yet, or no piece model to read it.
    Unread,
    /// The reading failed.
    Error,
    /// The user looked and kept the reading.
    Verified,
    /// The user changed the position.
    Corrected,
    /// The book's PGN has the position read (plan "Carregar PGN do livro", R3): confirmed by it.
    Reference,
}

impl Status {
    /// Every status, in the order the panel's chips show them.
    pub const ALL: [Status; 7] =
        [Status::Read, Status::Review, Status::Reference, Status::Verified, Status::Corrected, Status::Unread, Status::Error];

    /// The status of a diagram read as `reading` (`None`: not read), after what the user did
    /// with it (`kind`).
    pub fn of(reading: Option<&pdfcraft_chessvision::board::BoardReading>, kind: Option<Kind>) -> Status {
        use pdfcraft_chessvision::board::Unreadable;
        match (kind, reading.map(|r| &r.outcome)) {
            (Some(Kind::Corrected), _) => Status::Corrected,
            (Some(Kind::Verified), _) => Status::Verified,
            (None, Some(Ok(p))) if p.accepted => Status::Read,
            (None, Some(Ok(_))) => Status::Review,
            (None, Some(Err(Unreadable::NoModel)) | None) => Status::Unread,
            (None, Some(Err(_))) => Status::Error,
        }
    }

    /// [`Status::of`] with the book's PGN (plan "Carregar PGN do livro", R3): a diagram read, with
    /// confidence or with a doubt, whose position `reference` has is [`Status::Reference`]. What
    /// the user did still outranks it, and a suggestion near the position never counts.
    pub fn with_reference(
        reading: Option<&pdfcraft_chessvision::board::BoardReading>,
        kind: Option<Kind>,
        reference: Option<&pdfcraft_chessvision::doc::reference::ReferenceIndex>,
    ) -> Status {
        let status = Status::of(reading, kind);
        let shown = || {
            let read = reading.and_then(|r| r.outcome.as_ref().ok());
            read.zip(reference).is_some_and(|(p, index)| index.has_position(&p.fen, p.side.is_assumed()))
        };
        if matches!(status, Status::Read | Status::Review) && shown() { Status::Reference } else { status }
    }

    /// Its name in the tools' replies.
    pub fn name(self) -> &'static str {
        match self {
            Status::Read => "read",
            Status::Review => "review",
            Status::Unread => "unread",
            Status::Error => "error",
            Status::Verified => "verified",
            Status::Corrected => "corrected",
            Status::Reference => "reference",
        }
    }

    /// Whether Mark the page as checked keeps a diagram in this status as Verified: only one read
    /// with confidence (plan A8, P2b). Those to review, not read or with an error are for a person
    /// to check one by one; the plan rejects accepting them in bulk.
    pub fn checked_with_the_page(self) -> bool {
        self == Status::Read
    }

    /// Whether a diagram in this status is in the review queue: not confirmed, and not read with
    /// confidence.
    pub fn to_review(self) -> bool {
        matches!(self, Status::Review | Status::Unread | Status::Error)
    }
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

impl DocRecord {
    /// The record of the document named by `keys`, of `pages` pages.
    pub fn new(keys: &DocKeys, pages: usize) -> DocRecord {
        DocRecord {
            id0: keys.id0.map(|id| hex(&id)),
            path_hash: keys.path.as_deref().map(|p| hash_hex(&[p.as_bytes()])),
            name: keys.name.clone().filter(|n| n.len() <= MAX_TEXT),
            size: keys.size,
            pages,
        }
    }

    /// Whether this is the document named by `keys`: the same identifier, the same path, or the
    /// same name and size ([`DocKeys::matches`]).
    pub fn matches(&self, keys: &DocKeys) -> bool {
        let other = DocRecord::new(keys, 0);
        fn same<T: PartialEq>(a: &Option<T>, b: &Option<T>) -> bool {
            a.is_some() && a == b
        }
        same(&self.id0, &other.id0) || same(&self.path_hash, &other.path_hash) || (same(&self.name, &other.name) && same(&self.size, &other.size))
    }
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

/// Why a change was refused.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CorrectionError {
    #[error("the FEN could not be read: {0}")]
    BadFen(String),
    #[error("the diagram's box is not a box on the page")]
    BadRect,
    #[error("this document has {MAX_ENTRIES} corrections, the most one keeps")]
    Full,
    #[error("the corrections would take more than {} MB", MAX_FILE_BYTES >> 20)]
    TooBig,
    #[error("the corrections file was written by a newer PdfCraft (version {0}); this one only reads it")]
    Newer(u32),
    #[error("no correction {0}")]
    NoSuchCorrection(String),
    #[error("the corrections kept here would take more than their {0} bytes")]
    OverBudget(usize),
}

/// A correction to put in: the diagram, and what is said about it.
#[derive(Clone, Debug, PartialEq)]
pub struct NewCorrection {
    pub page: usize,
    pub page_fp: String,
    /// The box normalised to the crop box ([`normalize`]).
    pub rect: [f32; 4],
    pub orig: Option<Original>,
    pub fen: String,
    pub kind: Kind,
}

/// Where a correction's diagram is in the document as it is now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Located {
    /// On its page, which has not changed.
    Same(usize),
    /// On another page with its page's fingerprint: pages were inserted or removed before it.
    Moved(usize),
    /// On its page, whose content changed (an edit, a redrawn diagram): matched by its box alone.
    Changed(usize),
    /// Nowhere: its page is gone.
    Orphan,
}

impl Located {
    /// The page now, when there is one.
    pub fn page(self) -> Option<usize> {
        match self {
            Located::Same(p) | Located::Moved(p) | Located::Changed(p) => Some(p),
            Located::Orphan => None,
        }
    }

    /// Its name in the tools' output.
    pub fn name(self) -> &'static str {
        match self {
            Located::Same(_) => "same",
            Located::Moved(_) => "moved",
            Located::Changed(_) => "changed",
            Located::Orphan => "orphan",
        }
    }
}

impl CorrectionFile {
    /// An empty file for the document `doc`.
    pub fn new(doc: DocRecord) -> CorrectionFile {
        CorrectionFile { format: FORMAT.into(), version: VERSION, doc, entries: Vec::new() }
    }

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

    /// The corrections not deleted.
    pub fn live(&self) -> impl Iterator<Item = &Correction> {
        self.entries.iter().filter(|e| !e.deleted)
    }

    /// Take `other`'s entries in: each entry by its uid, the higher revision winning (a tie keeps
    /// this file's). Tombstones merge like any entry, so a deletion is never undone by a merge.
    pub fn merge(&mut self, other: &CorrectionFile) {
        for theirs in &other.entries {
            match self.entries.iter().position(|e| e.uid == theirs.uid) {
                Some(i) => {
                    if let Some(mine) = self.entries.get_mut(i)
                        && theirs.rev > mine.rev
                    {
                        *mine = theirs.clone();
                    }
                }
                None if self.entries.len() < MAX_ENTRIES => self.entries.push(theirs.clone()),
                None => {}
            }
        }
    }

    /// Put `new` in at time `at`: over the live correction of the same diagram (same page, boxes
    /// overlapping by [`MATCH_IOU`]) as its next revision, else as a new entry. Returns its uid.
    pub fn set(&mut self, new: NewCorrection, at: u64) -> Result<String, CorrectionError> {
        crate::chess::pdfcraft_chess::fen::parse(&new.fen).map_err(|e| CorrectionError::BadFen(e.to_string()))?;
        if new.fen.len() > MAX_TEXT || new.page_fp.is_empty() || new.page_fp.len() > MAX_TEXT {
            return Err(CorrectionError::BadFen("too long".into()));
        }
        if !rect_ok(new.rect) {
            return Err(CorrectionError::BadRect);
        }
        let same = self
            .entries
            .iter_mut()
            .filter(|e| !e.deleted && e.page == new.page && iou(e.rect, new.rect) >= MATCH_IOU)
            .max_by(|a, b| iou(a.rect, new.rect).total_cmp(&iou(b.rect, new.rect)));
        if let Some(e) = same {
            e.page_fp = new.page_fp;
            e.rect = new.rect;
            e.fen = new.fen;
            e.kind = new.kind;
            e.at = at;
            e.rev = e.rev.saturating_add(1);
            if e.orig.is_none() {
                e.orig = new.orig;
            }
            return Ok(e.uid.clone());
        }
        if self.entries.len() >= MAX_ENTRIES {
            return Err(CorrectionError::Full);
        }
        let mut uid = new_uid(&new, at, 0);
        let mut salt = 1u32;
        while self.entries.iter().any(|e| e.uid == uid) && salt < 1000 {
            uid = new_uid(&new, at, salt);
            salt += 1;
        }
        let NewCorrection { page, page_fp, rect, orig, fen, kind } = new;
        self.entries.push(Correction { uid: uid.clone(), page, page_fp, rect, orig, fen, kind, at, rev: 1, deleted: false });
        Ok(uid)
    }

    /// Delete correction `uid` at time `at`, leaving a tombstone. Whether it was there and live.
    pub fn delete(&mut self, uid: &str, at: u64) -> bool {
        match self.entries.iter_mut().find(|e| e.uid == uid && !e.deleted) {
            Some(e) => {
                e.deleted = true;
                e.at = at;
                e.rev = e.rev.saturating_add(1);
                true
            }
            None => false,
        }
    }

    /// Where each live correction's diagram is now, given every page's fingerprint now.
    pub fn locate(&self, fingerprints: &[String]) -> Vec<(Located, &Correction)> {
        self.live().map(|e| (locate(e, fingerprints), e)).collect()
    }

    /// The live correction of the diagram at `rect` (normalised) on page `page` now, when one
    /// overlaps it by [`MATCH_IOU`]: the best overlap wins.
    pub fn find(&self, fingerprints: &[String], page: usize, rect: [f32; 4]) -> Option<&Correction> {
        self.locate(fingerprints)
            .into_iter()
            .filter(|(at, e)| at.page() == Some(page) && iou(e.rect, rect) >= MATCH_IOU)
            .max_by(|(_, a), (_, b)| iou(a.rect, rect).total_cmp(&iou(b.rect, rect)))
            .map(|(_, e)| e)
    }
}

/// Where `e`'s diagram is now: its page when that page is unchanged; else the one page with its
/// page's fingerprint; else its page by index when the document still has it.
fn locate(e: &Correction, fingerprints: &[String]) -> Located {
    if fingerprints.get(e.page) == Some(&e.page_fp) {
        return Located::Same(e.page);
    }
    let mut same = fingerprints.iter().enumerate().filter(|(_, fp)| **fp == e.page_fp).map(|(i, _)| i);
    match (same.next(), same.next()) {
        (Some(p), None) => Located::Moved(p),
        _ if e.page < fingerprints.len() => Located::Changed(e.page),
        _ => Located::Orphan,
    }
}

/// A box in 0..1 with its corners in order and some area.
fn rect_ok(r: [f32; 4]) -> bool {
    r.iter().all(|v| v.is_finite() && (-0.01..=1.01).contains(v)) && r[0] < r[2] && r[1] < r[3]
}

/// The intersection over union of two boxes; 0 when they don't overlap or one has no area.
pub fn iou(a: [f32; 4], b: [f32; 4]) -> f32 {
    let w = (a[2].min(b[2]) - a[0].max(b[0])).max(0.0);
    let h = (a[3].min(b[3]) - a[1].max(b[1])).max(0.0);
    let inter = w * h;
    let area = |r: [f32; 4]| (r[2] - r[0]).max(0.0) * (r[3] - r[1]).max(0.0);
    let union = area(a) + area(b) - inter;
    if union > 0.0 && union.is_finite() { inter / union } else { 0.0 }
}

/// `bbox` (user space) as a fraction of `crop` (the page's crop box): `None` when either has no
/// area or is not finite.
pub fn normalize(bbox: [f32; 4], crop: [f32; 4]) -> Option<[f32; 4]> {
    let (w, h) = (crop[2] - crop[0], crop[3] - crop[1]);
    if !(w > 0.0 && h > 0.0 && w.is_finite() && h.is_finite()) {
        return None;
    }
    let r = [(bbox[0] - crop[0]) / w, (bbox[1] - crop[1]) / h, (bbox[2] - crop[0]) / w, (bbox[3] - crop[1]) / h];
    rect_ok(r).then_some(r)
}

/// A new entry's uid: a hash of what it was made from and when.
fn new_uid(new: &NewCorrection, at: u64, salt: u32) -> String {
    let rect: Vec<u8> = new.rect.iter().flat_map(|v| v.to_bits().to_be_bytes()).collect();
    let page = u64::try_from(new.page).unwrap_or(u64::MAX).to_be_bytes();
    hash_hex(&[new.page_fp.as_bytes(), &page, &rect, &at.to_be_bytes(), &salt.to_be_bytes()])
}

/// The file name a new file for the document named by `keys` gets: a hash of its identifier, else
/// of its path, else of its name and size. `None` when no key is known.
pub fn file_name(keys: &DocKeys) -> Option<String> {
    let hash = if let Some(id) = keys.id0 {
        hash_hex(&[b"id0", &id])
    } else if let Some(p) = &keys.path {
        hash_hex(&[b"path", p.as_bytes()])
    } else {
        let (name, size) = (keys.name.as_ref()?, keys.size?);
        hash_hex(&[b"name", name.as_bytes(), &size.to_be_bytes()])
    };
    Some(format!("{hash}.json"))
}

/// Bytes in lower-case hexadecimal.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A 128-bit hash of `parts` (each with its length, so `["ab","c"]` and `["a","bc"]` differ), as
/// 32 hexadecimal digits: two FNV-1a passes with different offsets. Not cryptographic; it names
/// and compares, nothing more.
fn hash_hex(parts: &[&[u8]]) -> String {
    let mut a = 0xcbf2_9ce4_8422_2325u64;
    let mut b = 0x6c62_272e_07bb_0142u64;
    let mut eat = |byte: u8| {
        a = (a ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        b = (b ^ u64::from(byte.rotate_left(3))).wrapping_mul(0x0100_0000_01b3);
    };
    for part in parts {
        for byte in u64::try_from(part.len()).unwrap_or(u64::MAX).to_be_bytes() {
            eat(byte);
        }
        for &byte in part.iter() {
            eat(byte);
        }
    }
    format!("{a:016x}{b:016x}")
}

impl Document {
    /// Every page's fingerprint ([`page_fingerprint`]), in page order; empty for a document that
    /// can't be read yet (encrypted and not unlocked).
    pub fn page_fingerprints(&self) -> Vec<String> {
        let Some(editor) = self.editor.as_ref() else { return Vec::new() };
        let cos = &editor.cos;
        let Ok(pages) = pdfcraft_organize::pages(cos) else { return Vec::new() };
        pages.iter().map(|p| page_fingerprint(cos, p.obj)).collect()
    }
}

/// A page's fingerprint: a hash of its content streams and of the images and forms its own
/// resources name, as they are stored (at most [`MAX_HASHED_STREAM`] bytes of each, and its
/// length). Saving keeps it; an edit to the page's content or its pictures changes it. `fnv:`
/// and 32 hexadecimal digits.
pub fn page_fingerprint(cos: &pdfcraft_cos::Document, page: pdfcraft_cos::ObjRef) -> String {
    use pdfcraft_cos::Object;
    let page = cos.get(page);
    let Some(dict) = page.as_dict() else { return format!("fnv:{}", hash_hex(&[b"no page"])) };
    let mut streams: Vec<std::sync::Arc<Object>> = Vec::new();
    let contents = dict.get(b"Contents").map(|c| cos.resolve(c));
    match contents.as_deref() {
        Some(Object::Array(parts)) => streams.extend(parts.iter().take(MAX_HASHED_STREAMS).map(|p| cos.resolve(p))),
        Some(_) => streams.extend(contents.clone()),
        None => {}
    }
    let resources = dict.get(b"Resources").map(|r| cos.resolve(r));
    let xobjects = resources.as_deref().and_then(Object::as_dict).and_then(|r| r.get(b"XObject")).map(|x| cos.resolve(x));
    if let Some(x) = xobjects.as_deref().and_then(Object::as_dict) {
        let mut named: Vec<_> = x.iter().take(MAX_HASHED_STREAMS).collect();
        named.sort_by(|a, b| a.0.cmp(b.0));
        streams.extend(named.into_iter().map(|(_, o)| cos.resolve(o)));
    }
    // Each stream's length and its first bytes, hashed where they are (never copied).
    let raws: Vec<&[u8]> = streams.iter().filter_map(|s| if let Object::Stream(s) = s.as_ref() { Some(s.raw.as_ref()) } else { None }).collect();
    let lengths: Vec<[u8; 8]> = raws.iter().map(|raw| u64::try_from(raw.len()).unwrap_or(u64::MAX).to_be_bytes()).collect();
    let mut parts: Vec<&[u8]> = Vec::with_capacity(raws.len().saturating_mul(2));
    for (raw, len) in raws.iter().zip(&lengths) {
        parts.push(len);
        parts.push(raw.get(..raw.len().min(MAX_HASHED_STREAM)).unwrap_or_default());
    }
    format!("fnv:{}", hash_hex(&parts))
}

/// A correction that replaced a diagram's reading ([`apply`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Applied {
    pub uid: String,
    /// The page (0-based) and the diagram's index among its readings.
    pub page: usize,
    pub index: usize,
    pub kind: Kind,
    /// The position that replaced the reading.
    pub fen: String,
    /// What the diagram reads as now, when it was read.
    pub read_fen: Option<String>,
    /// The reading changed since the correction was made over it (another model, another scan):
    /// the correction still wins, and this says so.
    pub diverged: bool,
}

/// A correction on a page that was read whose diagram was not found there, and why.
#[derive(Clone, Debug, PartialEq)]
pub struct Unmatched {
    pub uid: String,
    pub page: usize,
    pub reason: String,
}

/// What [`apply`] made of the pages.
#[derive(Clone, Debug, Default)]
pub struct Corrected {
    /// The pages, each with its corrected readings (the same `Arc` when nothing changed it).
    pub pages: Vec<std::sync::Arc<super::ChessPage>>,
    pub applied: Vec<Applied>,
    pub unmatched: Vec<Unmatched>,
}

/// The reading a correction gives: the position as the person set it, legal or not, with the
/// side to move it says (White when it says none), set by hand, nothing uncertain or repaired.
/// Accepted when legal with that side. `before`'s orientation is kept, so a board printed from
/// Black's side links from Black's side still. `None` for a FEN that can't be read.
pub fn corrected_position(
    fen: &str,
    before: Option<&pdfcraft_chessvision::board::ReadPosition>,
) -> Option<pdfcraft_chessvision::board::ReadPosition> {
    use pdfcraft_chess::recognize::side::{SideSource, SideToMove};
    let parsed = crate::chess::pdfcraft_chess::fen::parse(fen).ok()?;
    let color = parsed.side_to_move.unwrap_or(pdfcraft_chess::placement::Color::White);
    let legality = pdfcraft_chess::rules::check_position(&parsed.placement);
    let accepted = legality.allows(color);
    Some(pdfcraft_chessvision::board::ReadPosition {
        placement: parsed.placement,
        fen: fen.trim().to_owned(),
        legality,
        side: SideToMove { color, source: SideSource::Manual, conflicting: false },
        orientation: before.and_then(|p| p.orientation),
        min_confidence: 1.0,
        uncertain: Vec::new(),
        repairs: Vec::new(),
        accepted,
    })
}

/// `pages` with `file`'s corrections in place of their readings, given every page's fingerprint
/// now. A correction is its page's diagram whose box overlaps its own the most, by
/// [`MATCH_IOU`] at least, each diagram taking one correction; it wins over the reading whatever
/// the reading says now. Pages scanned without reading are left as they are; a correction on a
/// read page whose diagram is not there is listed as unmatched, never dropped silently.
pub fn apply(pages: &[std::sync::Arc<super::ChessPage>], file: &CorrectionFile, fingerprints: &[String]) -> Corrected {
    use pdfcraft_chessvision::board::{BoardReading, ReadingSource};
    let located = file.locate(fingerprints);
    let mut out = Corrected::default();
    for page in pages {
        let here: Vec<&Correction> = located.iter().filter(|(at, _)| at.page() == Some(page.page)).map(|(_, e)| *e).collect();
        let (Some(readings), false) = (page.readings.as_ref(), here.is_empty()) else {
            out.pages.push(page.clone());
            continue;
        };
        let unmatched = |e: &Correction, reason: &str| Unmatched { uid: e.uid.clone(), page: page.page, reason: reason.into() };
        let Some(crop) = page.geometry.map(|g| g.crop_box()) else {
            out.unmatched.extend(here.iter().map(|e| unmatched(e, "the page has no visible area")));
            out.pages.push(page.clone());
            continue;
        };
        let boxes: Vec<Option<[f32; 4]>> = readings
            .iter()
            .map(|r| normalize([r.bbox_user.x0, r.bbox_user.y0, r.bbox_user.x1, r.bbox_user.y1], [crop.x0, crop.y0, crop.x1, crop.y1]))
            .collect();
        let mut taken = vec![false; readings.len()];
        let mut corrected = (**page).clone();
        for e in here {
            let best = boxes
                .iter()
                .enumerate()
                .filter(|(i, _)| !taken.get(*i).copied().unwrap_or(true))
                .filter_map(|(i, b)| b.map(|b| (i, iou(b, e.rect))))
                .filter(|(_, overlap)| *overlap >= MATCH_IOU)
                .max_by(|a, b| a.1.total_cmp(&b.1));
            let Some((i, _)) = best else {
                out.unmatched.push(unmatched(e, "no diagram on the page is where it was"));
                continue;
            };
            let Some(reading) = corrected.readings.as_mut().and_then(|r| r.get_mut(i)) else { continue };
            let before = reading.position().cloned();
            let Some(position) = corrected_position(&e.fen, before.as_ref()) else {
                out.unmatched.push(unmatched(e, "its position is not a FEN this build reads"));
                continue;
            };
            if let Some(t) = taken.get_mut(i) {
                *t = true;
            }
            let read_fen = before.map(|p| p.fen);
            let made_over = e.orig.as_ref().and_then(|o| o.fen.clone());
            let diverged = e.orig.is_some() && read_fen != made_over;
            if diverged {
                let now = read_fen.as_deref().unwrap_or("unread");
                corrected.notes.push(format!("diagram {}: the reading changed since it was corrected (now {now}); the correction is kept", i + 1));
            }
            *reading = BoardReading {
                index: reading.index,
                bbox_user: reading.bbox_user,
                source: ReadingSource::User,
                dpi: reading.dpi,
                outcome: Ok(position),
            };
            out.applied.push(Applied { uid: e.uid.clone(), page: page.page, index: i, kind: e.kind, fen: e.fen.clone(), read_fen, diverged });
        }
        out.pages.push(std::sync::Arc::new(corrected));
    }
    out
}

/// Where a document's corrections are kept: a folder on the desktop and the command line
/// ([`DirStore`]), memory the web build saves in the browser's storage ([`MemStore`]).
pub trait CorrectionStore {
    /// The document's corrections: its file (found by the name its keys give, else by the
    /// document recorded in each file), or an empty one named for it. A file that can't be read
    /// is set aside, never deleted.
    fn load(&self, keys: &DocKeys, pages: usize) -> Result<Loaded, StoreError>;

    /// Change the document's corrections with `change` and keep them: the file is read again
    /// first, so two writers never lose each other's changes, and a file of a newer version is
    /// refused. The document record takes the keys given (a save may have added an identifier).
    fn update<R>(
        &self,
        keys: &DocKeys,
        pages: usize,
        change: impl FnOnce(&mut CorrectionFile) -> Result<R, CorrectionError>,
    ) -> Result<(R, Loaded), StoreError>;

    /// The document was saved, as itself or as another file: its corrections, found under its
    /// keys before (`old`), are recorded under the keys it has now (`new`), so they are found
    /// when the saved file is opened, as a game collection follows its document. Whether there
    /// were corrections to move; nothing is made for a document that has none, and a file of a
    /// newer version is left as it is.
    fn rekey(&self, old: &DocKeys, new: &DocKeys, pages: usize) -> Result<bool, StoreError>;
}

/// [`CorrectionStore::rekey`] on a loaded file: the bytes to keep under its name, when it has
/// entries to move.
fn rekeyed(loaded: &mut Loaded, new: &DocKeys, pages: usize) -> Result<Option<Vec<u8>>, CorrectionError> {
    if loaded.read_only || loaded.file.entries.is_empty() {
        return Ok(None);
    }
    loaded.file.doc = DocRecord::new(new, pages);
    loaded.file.to_bytes().map(Some)
}

/// What a stored file's bytes give for the document named by `keys` (`None`: no file yet), and
/// why they can't be read when they can't: then the file is empty and the caller sets the bytes
/// aside.
fn loaded(name: String, bytes: Option<&[u8]>, keys: &DocKeys, pages: usize) -> (Loaded, Option<String>) {
    let empty = |name: String| Loaded { name, file: CorrectionFile::new(DocRecord::new(keys, pages)), read_only: false, set_aside: None };
    let Some(bytes) = bytes else { return (empty(name), None) };
    match CorrectionFile::read(bytes) {
        Read::File(file) => (Loaded { name, file, read_only: false, set_aside: None }, None),
        Read::Newer(v, file) => {
            let mut file = file.unwrap_or_else(|| CorrectionFile::new(DocRecord::new(keys, pages)));
            file.version = v;
            (Loaded { name, file, read_only: true, set_aside: None }, None)
        }
        Read::Bad(why) => (empty(name), Some(why)),
    }
}

/// Apply `change` to `loaded` (refused for a newer version's file) and give the bytes to keep.
fn changed<R>(
    loaded: &mut Loaded,
    keys: &DocKeys,
    pages: usize,
    change: impl FnOnce(&mut CorrectionFile) -> Result<R, CorrectionError>,
) -> Result<(R, Vec<u8>), CorrectionError> {
    if loaded.read_only {
        return Err(CorrectionError::Newer(loaded.file.version));
    }
    let out = change(&mut loaded.file)?;
    loaded.file.doc = DocRecord::new(keys, pages);
    Ok((out, loaded.file.to_bytes()?))
}

/// The name a file that can't be read is set aside under: `<name>.bad`, else `.bad2`, … up to
/// `.bad999`, the first not `taken`.
fn aside_name(name: &str, taken: impl Fn(&str) -> bool) -> Option<String> {
    (1..1000u32).map(|n| if n == 1 { format!("{name}.bad") } else { format!("{name}.bad{n}") }).find(|bad| !taken(bad))
}

/// A folder of corrections files, one per document (desktop and command line; the web build
/// keeps its copy in the browser's storage, [`MemStore`]). Each change reads, changes and writes
/// a file under a lock file in the folder, through a staging file renamed over the old one.
#[cfg(not(target_arch = "wasm32"))]
pub struct DirStore {
    dir: std::path::PathBuf,
}

/// What [`DirStore::load`] found for a document.
#[derive(Clone, Debug, PartialEq)]
pub struct Loaded {
    /// The file's name in the folder (the one a new file would get when there is none yet).
    pub name: String,
    pub file: CorrectionFile,
    /// The file is of a newer version: shown, never written.
    pub read_only: bool,
    /// A file that could not be read was set aside under this name, and why.
    pub set_aside: Option<(String, String)>,
}

/// Why the folder store failed.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("the corrections folder: {0}")]
    Io(#[from] std::io::Error),
    #[error("the document has no name, path or identifier to keep corrections under")]
    NoKeys,
    #[error("another PdfCraft is writing the corrections; try again")]
    Busy,
    #[error(transparent)]
    Correction(#[from] CorrectionError),
}

/// Most files looked at to find a document's.
#[cfg(not(target_arch = "wasm32"))]
const MAX_FILES: usize = 1024;

/// A lock file older than this is taken as left behind by a process that stopped.
#[cfg(not(target_arch = "wasm32"))]
const STALE_LOCK: std::time::Duration = std::time::Duration::from_secs(30);

#[cfg(not(target_arch = "wasm32"))]
impl DirStore {
    pub fn new(dir: impl Into<std::path::PathBuf>) -> DirStore {
        DirStore { dir: dir.into() }
    }

    /// The file name for the document: the one whose recorded document is it, when a file in the
    /// folder has it, else the one its keys give.
    fn name_for(&self, keys: &DocKeys) -> Result<String, StoreError> {
        let own = file_name(keys).ok_or(StoreError::NoKeys)?;
        if self.dir.join(&own).is_file() {
            return Ok(own);
        }
        let Ok(entries) = std::fs::read_dir(&self.dir) else { return Ok(own) };
        let mut names: Vec<String> =
            entries.flatten().take(MAX_FILES).filter_map(|e| e.file_name().to_str().map(str::to_owned)).filter(|n| n.ends_with(".json")).collect();
        names.sort();
        for name in names {
            if let Ok(bytes) = read_capped(&self.dir.join(&name))
                && let Read::File(file) = CorrectionFile::read(&bytes)
                && file.doc.matches(keys)
            {
                return Ok(name);
            }
        }
        Ok(own)
    }

    /// Rename a file that can't be read to `<name>.bad` (`.bad2`, … when taken). The new name.
    fn set_aside(&self, name: &str) -> Result<String, StoreError> {
        let bad = aside_name(name, |n| self.dir.join(n).exists()).ok_or(StoreError::Busy)?;
        std::fs::rename(self.dir.join(name), self.dir.join(&bad))?;
        Ok(bad)
    }

    /// Take the folder's lock: a lock file made only when absent. One left by a process that
    /// stopped is taken over after [`STALE_LOCK`].
    fn lock(&self) -> Result<Lock, StoreError> {
        let path = self.dir.join("corrections.lock");
        for _ in 0..200 {
            match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(_) => return Ok(Lock { path }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let stale =
                        std::fs::metadata(&path).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > STALE_LOCK);
                    if stale {
                        let _ = std::fs::remove_file(&path);
                    } else {
                        std::thread::sleep(std::time::Duration::from_millis(15));
                    }
                }
                Err(e) => return Err(e.into()),
            }
        }
        Err(StoreError::Busy)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl CorrectionStore for DirStore {
    fn load(&self, keys: &DocKeys, pages: usize) -> Result<Loaded, StoreError> {
        let name = self.name_for(keys)?;
        let bytes = match read_capped(&self.dir.join(&name)) {
            Ok(b) => Some(b),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        let (mut l, bad) = loaded(name, bytes.as_deref(), keys, pages);
        if let Some(why) = bad {
            l.set_aside = Some((self.set_aside(&l.name)?, why));
        }
        Ok(l)
    }

    /// Under the folder's lock, through a staging file renamed over the old one.
    fn update<R>(
        &self,
        keys: &DocKeys,
        pages: usize,
        change: impl FnOnce(&mut CorrectionFile) -> Result<R, CorrectionError>,
    ) -> Result<(R, Loaded), StoreError> {
        std::fs::create_dir_all(&self.dir)?;
        let _lock = self.lock()?;
        let mut loaded = self.load(keys, pages)?;
        let (out, bytes) = changed(&mut loaded, keys, pages, change)?;
        write_atomically(&self.dir.join(&loaded.name), &bytes)?;
        Ok((out, loaded))
    }

    /// Without the folder, nothing (it is not made).
    fn rekey(&self, old: &DocKeys, new: &DocKeys, pages: usize) -> Result<bool, StoreError> {
        if old == new || !self.dir.is_dir() || file_name(old).is_none() {
            return Ok(false);
        }
        let _lock = self.lock()?;
        let mut loaded = self.load(old, pages)?;
        match rekeyed(&mut loaded, new, pages)? {
            Some(bytes) => {
                write_atomically(&self.dir.join(&loaded.name), &bytes)?;
                Ok(true)
            }
            None => Ok(false),
        }
    }
}

/// Corrections files kept in memory by name, within a byte budget of their own: the web build's
/// store, which it saves in the browser's storage (apart from the autosave's) through
/// [`MemStore::files`] and [`MemStore::with_files`]. A document's file is found as in a folder.
#[derive(Debug, Default)]
pub struct MemStore {
    files: std::sync::Mutex<std::collections::BTreeMap<String, Vec<u8>>>,
    budget: usize,
}

impl MemStore {
    /// An empty store keeping at most `budget` bytes in all.
    pub fn new(budget: usize) -> MemStore {
        MemStore { files: Default::default(), budget }
    }

    /// A store holding `files` (as [`MemStore::files`] gave them) within `budget` bytes: a file
    /// that would pass it, or [`MAX_FILE_BYTES`], is left out.
    pub fn with_files(files: impl IntoIterator<Item = (String, Vec<u8>)>, budget: usize) -> MemStore {
        let mut kept = std::collections::BTreeMap::new();
        let mut used = 0usize;
        for (name, bytes) in files {
            let total = used.saturating_add(bytes.len());
            if total <= budget && bytes.len() <= MAX_FILE_BYTES {
                used = total;
                kept.insert(name, bytes);
            }
        }
        MemStore { files: std::sync::Mutex::new(kept), budget }
    }

    /// Every file by name, to save.
    pub fn files(&self) -> Vec<(String, Vec<u8>)> {
        self.guard().iter().map(|(n, b)| (n.clone(), b.clone())).collect()
    }

    /// The files. A lock poisoned by a panic elsewhere is taken as it is: each write replaces a
    /// whole file, so none is ever half written.
    fn guard(&self) -> std::sync::MutexGuard<'_, std::collections::BTreeMap<String, Vec<u8>>> {
        self.files.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// [`CorrectionStore::load`] on `files`, already locked.
    fn load_in(files: &mut std::collections::BTreeMap<String, Vec<u8>>, keys: &DocKeys, pages: usize) -> Result<Loaded, StoreError> {
        let own = file_name(keys).ok_or(StoreError::NoKeys)?;
        let name = if files.contains_key(&own) {
            own
        } else {
            let recorded = files.iter().find(|(_, b)| matches!(CorrectionFile::read(b), Read::File(f) if f.doc.matches(keys)));
            recorded.map_or(own, |(n, _)| n.clone())
        };
        let bytes = files.get(&name).map(Vec::as_slice);
        let (mut l, bad) = loaded(name.clone(), bytes, keys, pages);
        if let Some(why) = bad {
            let aside = aside_name(&name, |n| files.contains_key(n)).ok_or(StoreError::Busy)?;
            if let Some(bytes) = files.remove(&name) {
                files.insert(aside.clone(), bytes);
            }
            l.set_aside = Some((aside, why));
        }
        Ok(l)
    }
}

impl CorrectionStore for MemStore {
    fn load(&self, keys: &DocKeys, pages: usize) -> Result<Loaded, StoreError> {
        MemStore::load_in(&mut self.guard(), keys, pages)
    }

    /// Refused with [`CorrectionError::OverBudget`] when the files would pass the budget.
    fn update<R>(
        &self,
        keys: &DocKeys,
        pages: usize,
        change: impl FnOnce(&mut CorrectionFile) -> Result<R, CorrectionError>,
    ) -> Result<(R, Loaded), StoreError> {
        let mut files = self.guard();
        let mut loaded = MemStore::load_in(&mut files, keys, pages)?;
        let (out, bytes) = changed(&mut loaded, keys, pages, change)?;
        let others = files.iter().filter(|(n, _)| **n != loaded.name).fold(0usize, |sum, (_, b)| sum.saturating_add(b.len()));
        if others.saturating_add(bytes.len()) > self.budget {
            return Err(CorrectionError::OverBudget(self.budget).into());
        }
        files.insert(loaded.name.clone(), bytes);
        Ok((out, loaded))
    }

    fn rekey(&self, old: &DocKeys, new: &DocKeys, pages: usize) -> Result<bool, StoreError> {
        if old == new || file_name(old).is_none() {
            return Ok(false);
        }
        let mut files = self.guard();
        let mut loaded = MemStore::load_in(&mut files, old, pages)?;
        match rekeyed(&mut loaded, new, pages)? {
            Some(bytes) => {
                files.insert(loaded.name, bytes);
                Ok(true)
            }
            None => Ok(false),
        }
    }
}

/// The folder's lock while it is held; dropping it lets go.
#[cfg(not(target_arch = "wasm32"))]
struct Lock {
    path: std::path::PathBuf,
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Read a file of at most [`MAX_FILE_BYTES`] (a larger one reads as that many and one more, so
/// it is refused as too big).
#[cfg(not(target_arch = "wasm32"))]
fn read_capped(path: &std::path::Path) -> std::io::Result<Vec<u8>> {
    use std::io::Read as _;
    let mut bytes = Vec::new();
    let cap = u64::try_from(MAX_FILE_BYTES).unwrap_or(u64::MAX).saturating_add(1);
    std::fs::File::open(path)?.take(cap).read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Write `bytes` through a staging file beside `path`, flushed and renamed over it.
#[cfg(not(target_arch = "wasm32"))]
fn write_atomically(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    let staging = path.with_extension(format!("json.tmp{}", std::process::id()));
    let result = (|| {
        let mut f = std::fs::File::create(&staging)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&staging, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&staging);
    }
    result
}

#[cfg(test)]
mod tests;
