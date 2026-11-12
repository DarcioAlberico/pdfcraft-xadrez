//! Chess ▸ Diagrams: the corrections a user made to diagram readings, kept on disk so they
//! outlive an edit, a new scan, saving, closing and reopening (plan A8, P3a1).
//!
//! **What names a correction.** The document by its [`DocKeys`] (the one rule of
//! [`DocKeys::matches`], as for the game collections), then each correction by its page index, the page's **fingerprint**
//! ([`page_fingerprint`]: its content streams and its images, as stored, so it survives saving)
//! and the diagram's box normalised to the page's crop box. A diagram is the correction's when
//! their boxes overlap by an intersection over union of at least [`MATCH_IOU`]. A page that moved
//! (pages inserted or removed before it) is found again by its fingerprint, and the fingerprints
//! of the pages around it tell a page that changed from a page that took its place ([`Located`]).
//! When two corrections are over one diagram, the one found by its page's fingerprint wins over
//! the one found by its page's index, then the later one ([`apply`], [`CorrectionFile::find`] and
//! [`CorrectionFile::set`] follow that one rule; plan A9, G2a).
//!
//! **Format** (version [`VERSION`]), one JSON file per document:
//! `{"format":"pdfcraft-chess-corrections","version":2,"doc":{…},"fps":[…],"entries":[…]}`. The
//! document is recorded by its identifier, the hash of its path (never the path itself), its name,
//! size and page count; no text or picture of the document is kept. `fps` holds each page
//! fingerprint once. Each entry has a `uid`, the page (`page`, of `pages`), its fingerprint and
//! those of the pages before and after it (`fp`: three places in `fps`, `null` where there is no
//! page), the normalised box, the position (`fen`), the reading it replaced (`orig`: left out when
//! it is the position, `null` when unknown), its `kind` (verified: the reading was right;
//! corrected), when (`at`, seconds since 1970) and a revision `rev`. Deleting leaves a tombstone
//! (`deleted`), so a merge never brings it back. Version 1 (each entry with its page's fingerprint
//! and nothing of its neighbours) is still read, and written again as version 2.
//!
//! **Merging and safety.** Two copies merge entry by entry, the higher revision winning
//! ([`CorrectionFile::merge`]). An entry that can't be read (a field of the wrong kind, a box with
//! its corners out of order, a page past [`MAX_PAGES`]) is left out and counted, and the rest of
//! the file kept ([`CorrectionFile::parse`]; plan A9-G3a, findings A4-06, R2-03). A file of a newer
//! version is read but never written; one that can't be read is reported, and set aside as
//! `….bad` only by a change, never deleted. Files are kept through the document store of plan A9
//! §2.3 ([`super::store::DocStore`], kind [`Corrections`]): `<hash>.corrections.json` in a folder
//! ([`DirStore`], desktop and command line: read, changed and written under the folder's
//! `corrections.lock`, through a staging file renamed over it) or in memory ([`MemStore`], the web
//! build, within a byte budget of its own). The file earlier builds wrote, `<hash>.json`, is still
//! read and merged in, never written.

use serde::{Deserialize, Serialize};

use super::keys::DocKeys;
use super::store::Parsed;
use crate::Document;

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

/// Least intersection over union for a diagram to be a correction's.
pub const MATCH_IOU: f32 = 0.7;

/// Most pages a document a correction names may have: an entry recording a page or a page count
/// past it is no correction of a real document, and is left out (R2-03).
pub const MAX_PAGES: usize = 1 << 20;

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
#[derive(Clone, Debug, PartialEq)]
pub struct Correction {
    pub uid: String,
    /// The page, 0-based, when the correction was last written.
    pub page: usize,
    /// That page's fingerprint then ([`page_fingerprint`]).
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

impl DocRecord {
    /// The record of the document named by `keys`, of `pages` pages. The identifier every new file
    /// used to get ([`DocKeys::KNOWN_CONSTANT_ID`]) names no document and is not recorded.
    pub fn new(keys: &DocKeys, pages: usize) -> DocRecord {
        DocRecord {
            id0: keys.id().map(|id| hex(&id)),
            path_hash: keys.path.as_deref().map(|p| hash_hex(&[p.as_bytes()])),
            name: keys.name.clone().filter(|n| n.len() <= MAX_TEXT),
            size: keys.size,
            pages,
        }
    }

    /// Whether this is the document named by `keys`, by the one rule of [`DocKeys::matches`]
    /// (the path compared by its hash, the only form a record keeps).
    pub fn matches(&self, keys: &DocKeys) -> bool {
        self.keys().matches(&DocRecord::new(keys, 0).keys())
    }

    /// The record as keys to compare, the path's hash standing for the path.
    fn keys(&self) -> DocKeys {
        DocKeys {
            id0: self.id0.as_deref().and_then(super::keys::id_from_hex),
            path: self.path_hash.clone(),
            name: self.name.clone(),
            size: self.size,
        }
    }
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
    #[error("page {} is not in the document as it is now", .0.saturating_add(1))]
    NoSuchPage(usize),
}

/// A correction to put in: the diagram, and what is said about it. Its page's fingerprint and its
/// neighbours' are taken from the document as it is ([`CorrectionFile::set`]).
#[derive(Clone, Debug, PartialEq)]
pub struct NewCorrection {
    pub page: usize,
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
    /// Only when no page it recorded as its neighbour has moved (then another page took its
    /// place) and, when no neighbour is left to tell, the page count is the one it recorded.
    Changed(usize),
    /// Nowhere: its page is gone, or another page took its place. Listed, never applied.
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

    /// Read a file's bytes: this version's file (an older version's taken in as this one), a
    /// newer one (read only), or why not. Entries that can't be read are left out
    /// ([`CorrectionFile::parse`] counts them).
    pub fn read(bytes: &[u8]) -> Read {
        match CorrectionFile::parse(bytes) {
            Parsed::File { file, .. } => Read::File(file),
            Parsed::Newer { version, file } => Read::Newer(version, file),
            Parsed::Bad(why) => Read::Bad(why),
        }
    }

    /// Read a file's bytes, entry by entry: an entry that can't be read (a field of the wrong
    /// kind, a fingerprint past the table, a box that is no box, a page past [`MAX_PAGES`]) is left
    /// out and counted, and the rest kept (A4-06). Only what makes the whole file unreadable (not
    /// JSON, another format, no version this build reads, no document record, more than
    /// [`MAX_ENTRIES`] entries) makes it [`Parsed::Bad`].
    pub fn parse(bytes: &[u8]) -> Parsed<CorrectionFile> {
        if bytes.len() > MAX_FILE_BYTES {
            return Parsed::Bad(format!("larger than {} MB", MAX_FILE_BYTES >> 20));
        }
        let value: serde_json::Value = match serde_json::from_slice(bytes) {
            Ok(v) => v,
            Err(e) => return Parsed::Bad(format!("not JSON: {e}")),
        };
        if value.get("format").and_then(serde_json::Value::as_str) != Some(FORMAT) {
            return Parsed::Bad("not a corrections file".into());
        }
        let version = value.get("version").and_then(serde_json::Value::as_u64).and_then(|v| u32::try_from(v).ok());
        let read = match version {
            Some(1) => disk::FileV1::read(value),
            Some(VERSION) => disk::FileV2::read(value),
            Some(v) if v > VERSION => {
                // Read in this version's layout, fields this build doesn't know left out.
                return Parsed::Newer { version: v, file: disk::FileV2::read(value).ok().map(|(f, _)| f) };
            }
            _ => return Parsed::Bad("no version this build reads".into()),
        };
        match read {
            Ok((file, skipped)) => Parsed::File { file, skipped },
            Err(why) => Parsed::Bad(format!("not a corrections file of its version: {why}")),
        }
    }

    /// The file as bytes, in this version's layout, refused when it would be too big to read back.
    pub fn to_bytes(&self) -> Result<Vec<u8>, CorrectionError> {
        let bytes = serde_json::to_vec(&disk::FileV2::of(self)).map_err(|_| CorrectionError::TooBig)?;
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

    /// Put `new` in at time `at`, the document's pages having `fingerprints` now: over the live
    /// correction its diagram has now (wherever its page was when it was written, by the rule of
    /// [`apply`]) as its next revision, else as a new entry, never as a second one. It records the
    /// page's fingerprint and its neighbours' as they are now. Returns its uid.
    pub fn set(&mut self, new: NewCorrection, at: u64, fingerprints: &[String]) -> Result<String, CorrectionError> {
        crate::chess::pdfcraft_chess::fen::parse(&new.fen).map_err(|e| CorrectionError::BadFen(e.to_string()))?;
        if new.fen.len() > MAX_TEXT {
            return Err(CorrectionError::BadFen("too long".into()));
        }
        if !rect_ok(new.rect) {
            return Err(CorrectionError::BadRect);
        }
        let page_fp = fingerprint_at(fingerprints, Some(new.page)).ok_or(CorrectionError::NoSuchPage(new.page))?;
        let (prev_fp, next_fp) = (fingerprint_at(fingerprints, new.page.checked_sub(1)), fingerprint_at(fingerprints, new.page.checked_add(1)));
        self.record_neighbours(fingerprints);
        // Only an entry written for this page's index or with this page's fingerprint can be
        // located on it (same or changed, moved): the others are not looked for.
        let over =
            self.entries.iter().enumerate().filter(|(_, e)| !e.deleted && (e.page == new.page || e.page_fp == page_fp)).filter_map(|(i, e)| {
                let (at, overlap) = (locate(e, fingerprints), iou(e.rect, new.rect));
                (at.page() == Some(new.page) && overlap >= MATCH_IOU).then_some((i, Claim { at, e, overlap }))
            });
        let current = winner(over);
        if let Some(e) = current.and_then(|i| self.entries.get_mut(i)) {
            e.page = new.page;
            e.page_fp = page_fp;
            e.prev_fp = prev_fp;
            e.next_fp = next_fp;
            e.page_count = fingerprints.len();
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
        let mut uid = new_uid(&new, &page_fp, at, 0);
        let mut salt = 1u32;
        while self.entries.iter().any(|e| e.uid == uid) && salt < 1000 {
            uid = new_uid(&new, &page_fp, at, salt);
            salt += 1;
        }
        let NewCorrection { page, rect, orig, fen, kind } = new;
        self.entries.push(Correction {
            uid: uid.clone(),
            page,
            page_fp,
            prev_fp,
            next_fp,
            page_count: fingerprints.len(),
            rect,
            orig,
            fen,
            kind,
            at,
            rev: 1,
            deleted: false,
        });
        Ok(uid)
    }

    /// Entries of version 1 still on their page (its fingerprint where it was) take their
    /// neighbours and the page count as they are now, which version 1 did not record: from here,
    /// another page taking their page's place is told from an edit to it ([`Located`]). Their
    /// revision stays: nothing the user made changed.
    fn record_neighbours(&mut self, fingerprints: &[String]) {
        for e in self.entries.iter_mut().filter(|e| !e.deleted && e.page_count == 0) {
            if fingerprints.get(e.page) == Some(&e.page_fp) {
                e.prev_fp = fingerprint_at(fingerprints, e.page.checked_sub(1));
                e.next_fp = fingerprint_at(fingerprints, e.page.checked_add(1));
                e.page_count = fingerprints.len();
            }
        }
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

    /// The live correction of the diagram at `rect` (normalised) on page `page` now: of those
    /// overlapping it by [`MATCH_IOU`] whose position this build reads, the one that takes the
    /// diagram by the rule of [`apply`] ([`Claim::outranks`]).
    pub fn find(&self, fingerprints: &[String], page: usize, rect: [f32; 4]) -> Option<&Correction> {
        let claims = self.locate(fingerprints).into_iter().filter_map(|(at, e)| {
            let overlap = iou(e.rect, rect);
            (at.page() == Some(page) && overlap >= MATCH_IOU && fen_readable(&e.fen)).then_some((e, Claim { at, e, overlap }))
        });
        winner(claims)
    }
}

/// Where `e`'s diagram is now:
/// - its page, when that page is unchanged ([`Located::Same`]);
/// - else the one page with its page's fingerprint ([`Located::Moved`]);
/// - else its page by index, changed ([`Located::Changed`]), unless the document has no such
///   page, or a page it recorded as its neighbour is in the document but not beside that index
///   (pages were removed or inserted there: the page at the index is another one, A4-01), or, with
///   no neighbour left to tell (none recorded, version 1; or each one changed too, as an edit to
///   every page does), the page count is not the one it recorded: then [`Located::Orphan`].
fn locate(e: &Correction, fingerprints: &[String]) -> Located {
    if fingerprints.get(e.page) == Some(&e.page_fp) {
        return Located::Same(e.page);
    }
    let mut same = fingerprints.iter().enumerate().filter(|(_, fp)| **fp == e.page_fp).map(|(i, _)| i);
    if let (Some(p), None) = (same.next(), same.next()) {
        return Located::Moved(p);
    }
    if e.page >= fingerprints.len() {
        return Located::Orphan;
    }
    let mut told = false;
    for (beside, recorded) in [(e.page.checked_sub(1), &e.prev_fp), (e.page.checked_add(1), &e.next_fp)] {
        let Some(recorded) = recorded else { continue };
        if beside.and_then(|p| fingerprints.get(p)) == Some(recorded) {
            told = true;
        } else if fingerprints.contains(recorded) {
            return Located::Orphan;
        }
    }
    if told || e.page_count == 0 || e.page_count == fingerprints.len() { Located::Changed(e.page) } else { Located::Orphan }
}

/// The fingerprint of page `page` (none past either end), when it is one a file keeps.
fn fingerprint_at(fingerprints: &[String], page: Option<usize>) -> Option<String> {
    page.and_then(|p| fingerprints.get(p)).filter(|fp| !fp.is_empty() && fp.len() <= MAX_TEXT).cloned()
}

/// Whether this build reads `fen` as a position: a correction whose FEN it can't read never takes a
/// diagram ([`apply`] lists it).
fn fen_readable(fen: &str) -> bool {
    crate::chess::pdfcraft_chess::fen::parse(fen).is_ok()
}

/// A correction over a diagram: where [`locate`] found it, and how much its box overlaps the
/// diagram's.
#[derive(Clone, Copy)]
struct Claim<'a> {
    at: Located,
    e: &'a Correction,
    overlap: f32,
}

impl Claim<'_> {
    /// Whether this one takes the diagram from `other` (plan A9, G2a; R1-01, A4-02): one found by
    /// its page's fingerprint (same, moved) wins over one found by its page's index alone
    /// (changed); between those, the later one (`at`), then the higher revision, then the box
    /// overlapping more.
    fn outranks(&self, other: &Claim<'_>) -> bool {
        let key = |c: &Claim<'_>| (matches!(c.at, Located::Same(_) | Located::Moved(_)), c.e.at, c.e.rev);
        match key(self).cmp(&key(other)) {
            std::cmp::Ordering::Equal => self.overlap.total_cmp(&other.overlap) == std::cmp::Ordering::Greater,
            order => order == std::cmp::Ordering::Greater,
        }
    }
}

/// Of the claims on one diagram, each with a tag, the tag of the one that takes it
/// ([`Claim::outranks`]); on a full tie, the first.
fn winner<'a, T>(claims: impl IntoIterator<Item = (T, Claim<'a>)>) -> Option<T> {
    let mut best: Option<(T, Claim<'a>)> = None;
    for (tag, claim) in claims {
        if best.as_ref().is_none_or(|(_, b)| claim.outranks(b)) {
            best = Some((tag, claim));
        }
    }
    best.map(|(tag, _)| tag)
}

/// Whether an entry read from a file is one to keep: its texts there and not too long, its box a
/// box, its page and page count within [`MAX_PAGES`].
fn entry_ok(e: &Correction) -> bool {
    let text_ok = |s: &str| !s.is_empty() && s.len() <= MAX_TEXT;
    [&e.uid, &e.page_fp, &e.fen].iter().all(|s| text_ok(s))
        && [&e.prev_fp, &e.next_fp].iter().all(|n| n.as_deref().is_none_or(text_ok))
        && rect_ok(e.rect)
        && e.page < MAX_PAGES
        && e.page_count <= MAX_PAGES
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
fn new_uid(new: &NewCorrection, page_fp: &str, at: u64, salt: u32) -> String {
    let rect: Vec<u8> = new.rect.iter().flat_map(|v| v.to_bits().to_be_bytes()).collect();
    let page = u64::try_from(new.page).unwrap_or(u64::MAX).to_be_bytes();
    hash_hex(&[page_fp.as_bytes(), &page, &rect, &at.to_be_bytes(), &salt.to_be_bytes()])
}

/// The file name a new file for the document named by `keys` gets: a hash of its identifier
/// ([`DocKeys::id`]: never the one every new file used to get), else of its path, else of its name
/// and size. `None` when no key is known.
pub fn file_name(keys: &DocKeys) -> Option<String> {
    let hash = if let Some(id) = keys.id() {
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
pub(super) fn hash_hex(parts: &[&[u8]]) -> String {
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
/// [`MATCH_IOU`] at least; of the corrections on one diagram, the one found by its page's
/// fingerprint takes it over one found by its page's index, then the later one
/// ([`Claim::outranks`]), and the others are listed with the one that took it. It wins over the
/// reading whatever the reading says now. Pages scanned without reading are left as they are; a
/// correction on a read page whose diagram is not there is listed as unmatched, never dropped
/// silently. A correction whose page is gone or was taken by another page ([`Located::Orphan`])
/// is on no page: never applied (`chess_correction_list` lists it).
pub fn apply(pages: &[std::sync::Arc<super::ChessPage>], file: &CorrectionFile, fingerprints: &[String]) -> Corrected {
    use pdfcraft_chessvision::board::{BoardReading, ReadingSource};
    let located = file.locate(fingerprints);
    let mut out = Corrected::default();
    for page in pages {
        let here: Vec<(Located, &Correction)> = located.iter().filter(|(at, _)| at.page() == Some(page.page)).map(|(at, e)| (*at, *e)).collect();
        let (Some(readings), false) = (page.readings.as_ref(), here.is_empty()) else {
            out.pages.push(page.clone());
            continue;
        };
        let unmatched = |e: &Correction, reason: &str| Unmatched { uid: e.uid.clone(), page: page.page, reason: reason.into() };
        let Some(crop) = page.geometry.map(|g| g.crop_box()) else {
            out.unmatched.extend(here.iter().map(|(_, e)| unmatched(e, "the page has no visible area")));
            out.pages.push(page.clone());
            continue;
        };
        let boxes: Vec<Option<[f32; 4]>> = readings
            .iter()
            .map(|r| normalize([r.bbox_user.x0, r.bbox_user.y0, r.bbox_user.x1, r.bbox_user.y1], [crop.x0, crop.y0, crop.x1, crop.y1]))
            .collect();
        // Each correction's diagram: the one its box overlaps most, by MATCH_IOU at least.
        let claims: Vec<Option<(usize, f32)>> = here
            .iter()
            .map(|(_, e)| {
                boxes
                    .iter()
                    .enumerate()
                    .filter_map(|(i, b)| b.map(|b| (i, iou(b, e.rect))))
                    .filter(|(_, overlap)| *overlap >= MATCH_IOU)
                    .max_by(|a, b| a.1.total_cmp(&b.1))
            })
            .collect();
        // Each diagram's correction, by the one rule (index into `here`).
        let taker = |diagram: usize| {
            winner(here.iter().zip(&claims).enumerate().filter_map(|(k, ((at, e), claim))| {
                let (i, overlap) = (*claim)?;
                (i == diagram && fen_readable(&e.fen)).then_some((k, Claim { at: *at, e, overlap }))
            }))
        };
        let takers: Vec<Option<usize>> = (0..readings.len()).map(taker).collect();
        let mut corrected = (**page).clone();
        for (k, ((_, e), claim)) in here.iter().zip(&claims).enumerate() {
            let Some((i, _)) = *claim else {
                out.unmatched.push(unmatched(e, "no diagram on the page is where it was"));
                continue;
            };
            if !fen_readable(&e.fen) {
                out.unmatched.push(unmatched(e, "its position is not a FEN this build reads"));
                continue;
            }
            match takers.get(i).copied().flatten() {
                Some(t) if t == k => {}
                Some(t) => {
                    let by = here.get(t).map_or("", |(_, w)| w.uid.as_str());
                    out.unmatched.push(unmatched(e, &format!("the diagram already has correction {by}")));
                    continue;
                }
                // Not reached (a readable claim takes its diagram unless another does); listed
                // all the same, never dropped.
                None => {
                    out.unmatched.push(unmatched(e, "no diagram on the page is where it was"));
                    continue;
                }
            }
            let Some(reading) = corrected.readings.as_mut().and_then(|r| r.get_mut(i)) else { continue };
            let before = reading.position().cloned();
            let Some(position) = corrected_position(&e.fen, before.as_ref()) else {
                out.unmatched.push(unmatched(e, "its position is not a FEN this build reads"));
                continue;
            };
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

/// The corrections as a kind of document store (plan A9 §2.3): `<hash>.corrections.json`, merged
/// entry by entry ([`CorrectionFile::merge`]).
pub struct Corrections;

impl super::store::StoreKind for Corrections {
    const KIND: &'static str = "corrections";
    const FORMAT: &'static str = FORMAT;
    const MAX_FILE_BYTES: usize = MAX_FILE_BYTES;
    type File = CorrectionFile;
    type Error = StoreError;

    fn empty(doc: DocRecord) -> CorrectionFile {
        CorrectionFile::new(doc)
    }

    fn set_doc(file: &mut CorrectionFile, doc: DocRecord) {
        file.doc = doc;
    }

    fn read(bytes: &[u8]) -> Parsed<CorrectionFile> {
        CorrectionFile::parse(bytes)
    }

    fn to_bytes(file: &CorrectionFile) -> Result<Vec<u8>, StoreError> {
        Ok(file.to_bytes()?)
    }

    fn holds_anything(file: &CorrectionFile) -> bool {
        !file.entries.is_empty()
    }

    fn merge(mine: &mut CorrectionFile, theirs: &CorrectionFile) {
        mine.merge(theirs);
    }
}

/// The corrections in a folder, one file per document (desktop and command line).
#[cfg(not(target_arch = "wasm32"))]
pub type DirStore = super::store::DocStore<Corrections, super::store::DirBackend>;

/// The corrections in memory, within a byte budget of their own: the web build's store, which it
/// saves in the browser's storage ([`super::store::DocStore::files`]).
pub type MemStore = super::store::DocStore<Corrections, super::store::MemBackend>;

/// What a store found for a document: its file, where it is, and what could not be read.
pub type Loaded = super::store::Loaded<CorrectionFile>;

/// Why a corrections store failed.
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

impl super::store::StoreFailure for StoreError {
    fn no_keys() -> Self {
        StoreError::NoKeys
    }

    fn busy() -> Self {
        StoreError::Busy
    }

    fn newer(version: u32) -> Self {
        StoreError::Correction(CorrectionError::Newer(version))
    }

    fn over_budget(budget: usize) -> Self {
        StoreError::Correction(CorrectionError::OverBudget(budget))
    }
}

/// A lock file older than this is taken as left behind by a process that stopped.
#[cfg(not(target_arch = "wasm32"))]
const STALE_LOCK: std::time::Duration = std::time::Duration::from_secs(30);

/// A folder's lock file while it is held; dropping it lets go.
#[cfg(not(target_arch = "wasm32"))]
pub struct Lock {
    path: std::path::PathBuf,
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Take the lock file `path` of a folder: made only when absent, and one left by a process that
/// stopped is taken over after [`STALE_LOCK`]. `None` when another holds it all the while.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn take_lock(path: std::path::PathBuf) -> std::io::Result<Option<Lock>> {
    for _ in 0..200 {
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(_) => return Ok(Some(Lock { path })),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let stale =
                    std::fs::metadata(&path).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > STALE_LOCK);
                if stale {
                    let _ = std::fs::remove_file(&path);
                } else {
                    std::thread::sleep(std::time::Duration::from_millis(15));
                }
            }
            Err(e) => return Err(e),
        }
    }
    Ok(None)
}

/// Read a file of at most `max` bytes (a larger one reads as that many and one more, so it is
/// refused as too big): never more than `max` + 1 bytes in memory, whatever the file's size. The
/// one capped read of the chess stores and tools (plan A9, G7a), which refuse past `max`.
pub fn read_capped(path: &std::path::Path, max: usize) -> std::io::Result<Vec<u8>> {
    use std::io::Read as _;
    let mut bytes = Vec::new();
    let cap = u64::try_from(max).unwrap_or(u64::MAX).saturating_add(1);
    std::fs::File::open(path)?.take(cap).read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Write `bytes` through a staging file beside `path`, flushed and renamed over it.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn write_atomically(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
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

/// The files as stored. Version 1 kept each page fingerprint in every entry; version 2 keeps each
/// once, in `fps`, an entry naming its page's and its neighbours' by their place there, and leaves
/// out what it can (`orig` when it is the position, `deleted` when false).
mod disk {
    use serde::{Deserialize, Deserializer, Serialize};

    use super::{Correction, CorrectionFile, DocRecord, Kind, MAX_ENTRIES, Original};

    /// Version 1, as `801e3abf` (plan A8, P3a1) wrote it; its entries are read one by one.
    #[derive(Deserialize)]
    pub(super) struct FileV1 {
        format: String,
        doc: DocRecord,
        #[serde(default)]
        entries: Vec<serde_json::Value>,
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
        /// The file `value` holds, taken in as this version's (the neighbours unknown), and how
        /// many of its entries could not be read and were left out.
        pub(super) fn read(value: serde_json::Value) -> Result<(CorrectionFile, usize), String> {
            let file: FileV1 = serde_json::from_value(value).map_err(|e| e.to_string())?;
            if file.entries.len() > MAX_ENTRIES {
                return Err(format!("more than {MAX_ENTRIES} entries"));
            }
            let total = file.entries.len();
            let entries: Vec<Correction> = file
                .entries
                .into_iter()
                .filter_map(|v| serde_json::from_value::<EntryV1>(v).ok())
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
                .filter(super::entry_ok)
                .collect();
            let skipped = total.saturating_sub(entries.len());
            Ok((CorrectionFile { format: file.format, version: super::VERSION, doc: file.doc, entries }, skipped))
        }
    }

    /// Version 2 (plan A9, G2a), as written.
    #[derive(Serialize)]
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

    /// Version 2 as read: the entries one by one ([`FileV2::read`]).
    #[derive(Deserialize)]
    struct FileV2In {
        format: String,
        version: u32,
        doc: DocRecord,
        #[serde(default)]
        fps: Vec<String>,
        #[serde(default)]
        entries: Vec<serde_json::Value>,
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

        /// The file `value` holds and how many of its entries could not be read and were left
        /// out (one of the wrong shape, naming a place past the table, or not a correction,
        /// [`super::entry_ok`]); or why the file can't be read: no document record, more than
        /// [`MAX_ENTRIES`] entries.
        pub(super) fn read(value: serde_json::Value) -> Result<(CorrectionFile, usize), String> {
            let file: FileV2In = serde_json::from_value(value).map_err(|e| e.to_string())?;
            if file.fps.len() > MAX_ENTRIES.saturating_mul(3) || file.entries.len() > MAX_ENTRIES {
                return Err(format!("more than {MAX_ENTRIES} entries"));
            }
            let fps = &file.fps;
            let fp = |at: u32| usize::try_from(at).ok().and_then(|i| fps.get(i)).cloned();
            let total = file.entries.len();
            let mut entries = Vec::with_capacity(total);
            for value in file.entries {
                let Ok(e) = serde_json::from_value::<EntryV2>(value) else { continue };
                let (prev, own, next) = e.fp;
                let (Some(page_fp), Some(prev_fp), Some(next_fp)) =
                    (fp(own), prev.map(fp).map_or(Some(None), |f| f.map(Some)), next.map(fp).map_or(Some(None), |f| f.map(Some)))
                else {
                    continue;
                };
                let orig = match e.orig {
                    None => Some(Original { fen: Some(e.fen.clone()), model: None }),
                    Some(o) => o,
                };
                let entry = Correction {
                    uid: e.uid,
                    page: e.page,
                    page_fp,
                    prev_fp,
                    next_fp,
                    page_count: e.pages,
                    rect: e.rect,
                    orig,
                    fen: e.fen,
                    kind: e.kind,
                    at: e.at,
                    rev: e.rev,
                    deleted: e.deleted,
                };
                if super::entry_ok(&entry) {
                    entries.push(entry);
                }
            }
            let skipped = total.saturating_sub(entries.len());
            Ok((CorrectionFile { format: file.format, version: file.version, doc: file.doc, entries }, skipped))
        }
    }
}

#[cfg(test)]
mod tests;
