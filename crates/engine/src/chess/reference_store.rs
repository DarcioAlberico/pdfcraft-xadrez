//! Chess: the book's own PGN kept with its document, so the book opens linked to it again (plan
//! "Carregar PGN do livro", R5; decision D3: a copy of the PGN, not a path to it).
//!
//! **Format** (version [`VERSION`]), one JSON file per document, named and found as the diagram
//! corrections are ([`corrections::file_name`], the document recorded by [`DocRecord`]):
//! `{"format":"pdfcraft-chess-reference","version":1,"doc":{…},"pgn":{…}}`. The PGN is kept as
//! its text (at most [`MAX_REFERENCE_BYTES`]), its hash, its size, the file name it came from
//! (never its path), when it was kept (`at`, seconds since 1970) and its games and moves. A file
//! whose text does not have its hash is not trusted.
//!
//! **Decisions** (plan "Carregar PGN do livro", R6e): what the user chose for a move of the text
//! the PGN plays from another position (a conflict), the book's move or the PGN's, each by the
//! move's place on its page and its SAN as printed, so a page read differently does not take
//! another move's decision. They belong to the PGN they were made against: keeping another PGN
//! drops them; a save as another file keeps them.
//!
//! **Safety**, as for the corrections: a file of a newer version is read but never written or
//! removed; one that can't be read is set aside as `….bad`, never deleted. The folder store
//! ([`DirStore`], desktop and command line) writes under a lock file through a staging file
//! renamed over the old one, with the corrections' own helpers; the memory store ([`MemStore`],
//! the web build) keeps the files within a byte budget. Removing a document's PGN, which only a
//! person asks for, removes its file.

use serde::{Deserialize, Serialize};

use super::corrections::{self, DocRecord};
use super::keys::DocKeys;
use pdfcraft_chessvision::doc::reference::MAX_REFERENCE_BYTES;

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
/// its SAN as printed, and the choice.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    pub page: usize,
    pub block: usize,
    pub inline: usize,
    pub san: String,
    pub choice: Choice,
}

impl Decision {
    /// Whether this is the decision on the move at `page`, `block`, `inline` printed `san`.
    pub fn is_for(&self, page: usize, block: usize, inline: usize, san: &str) -> bool {
        (self.page, self.block, self.inline) == (page, block, inline) && self.san == san
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
}

impl KeptPgn {
    /// The PGN `text`, read from a file named `name`, with its `games` and `moves`, kept at `at`.
    /// Refused when it is larger than [`MAX_REFERENCE_BYTES`].
    pub fn new(name: Option<&str>, text: String, games: usize, moves: usize, at: u64) -> Result<KeptPgn, ReferenceStoreError> {
        if text.len() > MAX_REFERENCE_BYTES {
            return Err(ReferenceStoreError::TooBig);
        }
        let name = name.filter(|n| !n.is_empty() && n.len() <= MAX_NAME).map(str::to_owned);
        Ok(KeptPgn { name, hash: text_hash(&text), bytes: text.len(), at, games, moves, text })
    }
}

/// The hash a kept PGN's text has: 32 hexadecimal digits, to name and compare (not
/// cryptographic).
pub fn text_hash(text: &str) -> String {
    corrections::hash_hex(&[b"pgn", text.as_bytes()])
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

impl ReferenceFile {
    /// The file for the document `doc` keeping `pgn`.
    pub fn new(doc: DocRecord, pgn: KeptPgn) -> ReferenceFile {
        ReferenceFile { format: FORMAT.into(), version: VERSION, doc, pgn, decisions: Vec::new() }
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
        if self.decisions.iter().any(|d| d.san.is_empty() || d.san.len() > MAX_SAN) {
            return Some("a decision names no move".into());
        }
        None
    }

    /// Keep `decision` in place of the one on the same move. Refused past [`MAX_DECISIONS`].
    pub fn decide(&mut self, decision: Decision) -> Result<(), ReferenceStoreError> {
        if decision.san.is_empty() || decision.san.len() > MAX_SAN {
            return Err(ReferenceStoreError::NoMove);
        }
        self.decisions.retain(|d| !d.is_for(decision.page, decision.block, decision.inline, &decision.san));
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
fn recorded(bytes: &[u8]) -> Option<DocRecord> {
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

/// What a store found for a document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loaded {
    /// The file's name (the one a new file would get when there is none yet).
    pub name: String,
    /// The file, when the document keeps a PGN this build can read.
    pub file: Option<ReferenceFile>,
    /// The file is of a newer version: read, never written or removed.
    pub read_only: Option<u32>,
    /// A file that could not be read was set aside under this name, and why.
    pub set_aside: Option<(String, String)>,
}

impl Loaded {
    /// The PGN kept, when there is one this build can read.
    pub fn pgn(&self) -> Option<&KeptPgn> {
        self.file.as_ref().map(|f| &f.pgn)
    }
}

/// Why a store failed.
#[derive(Debug, thiserror::Error)]
pub enum ReferenceStoreError {
    #[error("the book's PGN folder: {0}")]
    Io(#[from] std::io::Error),
    #[error("the document has no name, path or identifier to keep a PGN under")]
    NoKeys,
    #[error("another PdfCraft is writing the book's PGN; try again")]
    Busy,
    #[error("the book's PGN is larger than {} MB", MAX_REFERENCE_BYTES >> 20)]
    TooBig,
    #[error("the book's PGN file was written by a newer PdfCraft (version {0}); this one only reads it")]
    Newer(u32),
    #[error("the books' PGNs kept here would take more than their {0} bytes")]
    OverBudget(usize),
    #[error("the document keeps no book's PGN")]
    NoPgn,
    #[error("a decision must name a move of at most {MAX_SAN} bytes")]
    NoMove,
    #[error("this document keeps {MAX_DECISIONS} decisions, the most it keeps")]
    TooManyDecisions,
}

/// Where documents keep their book's PGN: a folder on the desktop and the command line
/// ([`DirStore`]), memory the web build saves in the browser's storage ([`MemStore`]).
pub trait ReferenceStore {
    /// The document's kept PGN, found by the name its keys give, else by the document recorded
    /// in each file. A file that can't be read is set aside, never deleted.
    fn load(&self, keys: &DocKeys, pages: usize) -> Result<Loaded, ReferenceStoreError>;

    /// Keep `pgn` for the document, in place of the one it kept. Whether it kept one before;
    /// refused for a file of a newer version.
    fn set(&self, keys: &DocKeys, pages: usize, pgn: KeptPgn) -> Result<(bool, Loaded), ReferenceStoreError>;

    /// Remove the document's PGN. Whether it kept one; refused for a file of a newer version.
    fn clear(&self, keys: &DocKeys, pages: usize) -> Result<bool, ReferenceStoreError>;

    /// Keep `decision` with the document's PGN (R6e), in place of the one on the same move. The
    /// file as kept; refused when the document keeps no PGN, and for a file of a newer version.
    fn decide(&self, keys: &DocKeys, pages: usize, decision: Decision) -> Result<ReferenceFile, ReferenceStoreError>;

    /// The document was saved: its PGN, found under its keys before (`old`), is recorded under
    /// the keys it has now (`new`). Whether it kept one; a file of a newer version is left as it
    /// is.
    fn rekey(&self, old: &DocKeys, new: &DocKeys, pages: usize) -> Result<bool, ReferenceStoreError>;
}

/// What a stored file's bytes give (`None`: no file), and why they can't be read when they can't:
/// then the caller sets the bytes aside.
fn loaded(name: String, bytes: Option<&[u8]>) -> (Loaded, Option<String>) {
    let mut out = Loaded { name, file: None, read_only: None, set_aside: None };
    let Some(bytes) = bytes else { return (out, None) };
    match ReferenceFile::read(bytes) {
        Read::File(file) => out.file = Some(file),
        Read::Newer(v, file) => {
            out.file = file;
            out.read_only = Some(v);
        }
        Read::Bad(why) => return (out, Some(why)),
    }
    (out, None)
}

/// The bytes to keep for [`ReferenceStore::set`].
fn set_bytes(loaded: &Loaded, keys: &DocKeys, pages: usize, pgn: KeptPgn) -> Result<(ReferenceFile, Vec<u8>), ReferenceStoreError> {
    if let Some(v) = loaded.read_only {
        return Err(ReferenceStoreError::Newer(v));
    }
    let file = ReferenceFile::new(DocRecord::new(keys, pages), pgn);
    let bytes = file.to_bytes()?;
    Ok((file, bytes))
}

/// The file and its bytes for [`ReferenceStore::decide`].
fn decide_bytes(loaded: &Loaded, decision: Decision) -> Result<(ReferenceFile, Vec<u8>), ReferenceStoreError> {
    if let Some(v) = loaded.read_only {
        return Err(ReferenceStoreError::Newer(v));
    }
    let mut file = loaded.file.clone().ok_or(ReferenceStoreError::NoPgn)?;
    file.decide(decision)?;
    let bytes = file.to_bytes()?;
    Ok((file, bytes))
}

/// The bytes to keep for [`ReferenceStore::rekey`], when there is a PGN to move.
fn rekey_bytes(loaded: &mut Loaded, new: &DocKeys, pages: usize) -> Result<Option<Vec<u8>>, ReferenceStoreError> {
    if loaded.read_only.is_some() {
        return Ok(None);
    }
    let Some(file) = loaded.file.as_mut() else { return Ok(None) };
    file.doc = DocRecord::new(new, pages);
    file.to_bytes().map(Some)
}

/// A folder of the books' PGNs, one file per document (desktop and command line).
#[cfg(not(target_arch = "wasm32"))]
pub struct DirStore {
    dir: std::path::PathBuf,
}

/// Most files looked at to find a document's.
#[cfg(not(target_arch = "wasm32"))]
const MAX_FILES: usize = 1024;

#[cfg(not(target_arch = "wasm32"))]
impl DirStore {
    pub fn new(dir: impl Into<std::path::PathBuf>) -> DirStore {
        DirStore { dir: dir.into() }
    }

    /// The file name for the document: the one its keys give when the folder has it, else the
    /// one whose recorded document is it, else the one its keys give.
    fn name_for(&self, keys: &DocKeys) -> Result<String, ReferenceStoreError> {
        let own = corrections::file_name(keys).ok_or(ReferenceStoreError::NoKeys)?;
        if self.dir.join(&own).is_file() {
            return Ok(own);
        }
        let Ok(entries) = std::fs::read_dir(&self.dir) else { return Ok(own) };
        let mut names: Vec<String> =
            entries.flatten().take(MAX_FILES).filter_map(|e| e.file_name().to_str().map(str::to_owned)).filter(|n| n.ends_with(".json")).collect();
        names.sort();
        for name in names {
            if let Ok(bytes) = corrections::read_capped(&self.dir.join(&name), MAX_FILE_BYTES)
                && recorded(&bytes).is_some_and(|doc| doc.matches(keys))
            {
                return Ok(name);
            }
        }
        Ok(own)
    }

    fn lock(&self) -> Result<corrections::Lock, ReferenceStoreError> {
        corrections::take_lock(self.dir.join("reference.lock"))?.ok_or(ReferenceStoreError::Busy)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl ReferenceStore for DirStore {
    fn load(&self, keys: &DocKeys, _pages: usize) -> Result<Loaded, ReferenceStoreError> {
        let name = self.name_for(keys)?;
        let bytes = match corrections::read_capped(&self.dir.join(&name), MAX_FILE_BYTES) {
            Ok(b) => Some(b),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        let (mut l, bad) = loaded(name, bytes.as_deref());
        if let Some(why) = bad {
            let aside = corrections::aside_name(&l.name, |n| self.dir.join(n).exists()).ok_or(ReferenceStoreError::Busy)?;
            std::fs::rename(self.dir.join(&l.name), self.dir.join(&aside))?;
            l.set_aside = Some((aside, why));
        }
        Ok(l)
    }

    fn set(&self, keys: &DocKeys, pages: usize, pgn: KeptPgn) -> Result<(bool, Loaded), ReferenceStoreError> {
        std::fs::create_dir_all(&self.dir)?;
        let _lock = self.lock()?;
        let mut loaded = self.load(keys, pages)?;
        let (file, bytes) = set_bytes(&loaded, keys, pages, pgn)?;
        corrections::write_atomically(&self.dir.join(&loaded.name), &bytes)?;
        let had = loaded.file.replace(file).is_some();
        Ok((had, loaded))
    }

    /// Without the folder, nothing (it is not made).
    fn clear(&self, keys: &DocKeys, pages: usize) -> Result<bool, ReferenceStoreError> {
        if !self.dir.is_dir() {
            return Ok(false);
        }
        let _lock = self.lock()?;
        let loaded = self.load(keys, pages)?;
        if let Some(v) = loaded.read_only {
            return Err(ReferenceStoreError::Newer(v));
        }
        if loaded.file.is_none() {
            return Ok(false);
        }
        std::fs::remove_file(self.dir.join(&loaded.name))?;
        Ok(true)
    }

    fn decide(&self, keys: &DocKeys, pages: usize, decision: Decision) -> Result<ReferenceFile, ReferenceStoreError> {
        if !self.dir.is_dir() {
            return Err(ReferenceStoreError::NoPgn);
        }
        let _lock = self.lock()?;
        let loaded = self.load(keys, pages)?;
        let (file, bytes) = decide_bytes(&loaded, decision)?;
        corrections::write_atomically(&self.dir.join(&loaded.name), &bytes)?;
        Ok(file)
    }

    /// Without the folder, nothing (it is not made).
    fn rekey(&self, old: &DocKeys, new: &DocKeys, pages: usize) -> Result<bool, ReferenceStoreError> {
        if old == new || !self.dir.is_dir() || corrections::file_name(old).is_none() {
            return Ok(false);
        }
        let _lock = self.lock()?;
        let mut loaded = self.load(old, pages)?;
        match rekey_bytes(&mut loaded, new, pages)? {
            Some(bytes) => {
                corrections::write_atomically(&self.dir.join(&loaded.name), &bytes)?;
                Ok(true)
            }
            None => Ok(false),
        }
    }
}

/// The books' PGNs kept in memory by name, within a byte budget of their own: the web build's
/// store, which it saves in the browser's storage through [`MemStore::files`] and
/// [`MemStore::with_files`]. A document's file is found as in a folder.
#[derive(Debug, Default)]
pub struct MemStore {
    files: std::sync::Mutex<std::collections::BTreeMap<String, Vec<u8>>>,
    budget: usize,
}

type Files = std::collections::BTreeMap<String, Vec<u8>>;

impl MemStore {
    /// An empty store keeping at most `budget` bytes in all.
    pub fn new(budget: usize) -> MemStore {
        MemStore { files: Default::default(), budget }
    }

    /// A store holding `files` (as [`MemStore::files`] gave them) within `budget` bytes: a file
    /// that would pass it, or [`MAX_FILE_BYTES`], is left out.
    pub fn with_files(files: impl IntoIterator<Item = (String, Vec<u8>)>, budget: usize) -> MemStore {
        let mut kept = Files::new();
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
    fn guard(&self) -> std::sync::MutexGuard<'_, Files> {
        self.files.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// [`ReferenceStore::load`] on `files`, already locked.
    fn load_in(files: &mut Files, keys: &DocKeys) -> Result<Loaded, ReferenceStoreError> {
        let own = corrections::file_name(keys).ok_or(ReferenceStoreError::NoKeys)?;
        let name = if files.contains_key(&own) {
            own
        } else {
            let recorded = files.iter().find(|(_, b)| recorded(b).is_some_and(|doc| doc.matches(keys)));
            recorded.map_or(own, |(n, _)| n.clone())
        };
        let (mut l, bad) = loaded(name.clone(), files.get(&name).map(Vec::as_slice));
        if let Some(why) = bad {
            let aside = corrections::aside_name(&name, |n| files.contains_key(n)).ok_or(ReferenceStoreError::Busy)?;
            if let Some(bytes) = files.remove(&name) {
                files.insert(aside.clone(), bytes);
            }
            l.set_aside = Some((aside, why));
        }
        Ok(l)
    }

    /// Put `bytes` under `name`, refused when the files would pass the budget.
    fn put(&self, files: &mut Files, name: &str, bytes: Vec<u8>) -> Result<(), ReferenceStoreError> {
        let others = files.iter().filter(|(n, _)| n.as_str() != name).fold(0usize, |sum, (_, b)| sum.saturating_add(b.len()));
        if others.saturating_add(bytes.len()) > self.budget {
            return Err(ReferenceStoreError::OverBudget(self.budget));
        }
        files.insert(name.to_owned(), bytes);
        Ok(())
    }
}

impl ReferenceStore for MemStore {
    fn load(&self, keys: &DocKeys, _pages: usize) -> Result<Loaded, ReferenceStoreError> {
        MemStore::load_in(&mut self.guard(), keys)
    }

    /// Refused with [`ReferenceStoreError::OverBudget`] when the files would pass the budget.
    fn set(&self, keys: &DocKeys, pages: usize, pgn: KeptPgn) -> Result<(bool, Loaded), ReferenceStoreError> {
        let mut files = self.guard();
        let mut loaded = MemStore::load_in(&mut files, keys)?;
        let (file, bytes) = set_bytes(&loaded, keys, pages, pgn)?;
        self.put(&mut files, &loaded.name, bytes)?;
        let had = loaded.file.replace(file).is_some();
        Ok((had, loaded))
    }

    fn clear(&self, keys: &DocKeys, _pages: usize) -> Result<bool, ReferenceStoreError> {
        let mut files = self.guard();
        let loaded = MemStore::load_in(&mut files, keys)?;
        if let Some(v) = loaded.read_only {
            return Err(ReferenceStoreError::Newer(v));
        }
        Ok(loaded.file.is_some() && files.remove(&loaded.name).is_some())
    }

    fn decide(&self, keys: &DocKeys, _pages: usize, decision: Decision) -> Result<ReferenceFile, ReferenceStoreError> {
        let mut files = self.guard();
        let loaded = MemStore::load_in(&mut files, keys)?;
        let (file, bytes) = decide_bytes(&loaded, decision)?;
        self.put(&mut files, &loaded.name, bytes)?;
        Ok(file)
    }

    fn rekey(&self, old: &DocKeys, new: &DocKeys, pages: usize) -> Result<bool, ReferenceStoreError> {
        if old == new || corrections::file_name(old).is_none() {
            return Ok(false);
        }
        let mut files = self.guard();
        let mut loaded = MemStore::load_in(&mut files, old)?;
        match rekey_bytes(&mut loaded, new, pages)? {
            Some(bytes) => {
                self.put(&mut files, &loaded.name, bytes)?;
                Ok(true)
            }
            None => Ok(false),
        }
    }
}

#[cfg(test)]
mod tests;
