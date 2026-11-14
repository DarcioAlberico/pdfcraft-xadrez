//! Chess: the engine's evaluations kept by position, with the document they were made for (plan
//! A9-N8, finding A8-11), so a position the board comes back to in another session has its
//! evaluation at once, and the batch analysis to come (A8-10) has a place to put its own.
//!
//! **Where** (plan A9 §2.3): a kind of the document store ([`super::store::DocStore`], kind
//! [`Evals`]), as the corrections and the book's PGN are: one file per document,
//! `<hash>.evals.json`, in a folder ([`DirStore`], desktop and command line) or in memory
//! ([`MemStore`], the web build). The kind is new: no build before it reads or writes it, so it
//! takes no lock of theirs, and a build reverted to before it never sees the file.
//!
//! **Format** (version [`VERSION`]): `{"format":"pdfcraft-chess-evals","version":1,"doc":{…},
//! "evals":{"<position>":{…}}}`. A position is its FEN's first four fields as the rules write them
//! (pieces, side to move, castling, en passant: FIDE's same position, `Fen::full_key`), so the
//! clocks and the moves that led there do not split it. Each evaluation keeps the depth, the score
//! for White, the bound, the best line's first moves in UCI (at most [`MAX_MOVES`]), the engine's
//! name and when it was kept (seconds since 1970). At most [`MAX_EVALS`] positions: past them, the
//! oldest kept goes (an evaluation is found again by searching; it is never the user's work).
//! Fields a later build of this version writes are kept as they are.
//!
//! **Merge** ([`StoreKind::merge`]): per position, the deeper evaluation wins; the same depth keeps
//! the newer; a tie keeps this side's.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::corrections::DocRecord;
use super::keys::DocKeys;
use super::store::{DocStore, Parsed, StoreBackend, StoreFailure, StoreKind};
use pdfcraft_chess::fen::Fen;
use pdfcraft_chess::uci::{self, Bound, Score};

/// The format's name, in every file.
pub const FORMAT: &str = "pdfcraft-chess-evals";

/// The version this build writes; a newer one is read only.
pub const VERSION: u32 = 1;

/// Most positions a document keeps.
pub const MAX_EVALS: usize = 20_000;

/// Most moves of an evaluation's line kept.
pub const MAX_MOVES: usize = 16;

/// Longest engine name kept, in characters.
const MAX_ENGINE_CHARS: usize = 80;

/// Largest file read or written: [`MAX_EVALS`] evaluations of about 250 bytes each, and the rest.
pub const MAX_FILE_BYTES: usize = 8 << 20;

/// One position's evaluation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Eval {
    pub depth: u32,
    /// For White: centipawns, or mate in moves (negative when White is mated).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cp: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mate: Option<i32>,
    /// `lower` or `upper` when the score is only a bound; exact when not written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bound: Option<String>,
    /// The best line's first moves, in UCI.
    #[serde(default)]
    pub pv: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine: Option<String>,
    /// When it was kept, in seconds since 1970.
    #[serde(default)]
    pub at: u64,
    /// Fields a later build of this version writes, kept as they are.
    #[serde(flatten)]
    pub other: Map<String, Value>,
}

impl Eval {
    /// An evaluation of `depth` with `score` (for White) and its `bound`, the line `pv` (UCI), by
    /// `engine`, kept at `at`. `None` when a move of the line is not one in UCI.
    pub fn new(depth: u32, score: Score, bound: Bound, pv: &[String], engine: Option<&str>, at: u64) -> Option<Eval> {
        if pv.iter().any(|m| !uci::is_move(m)) {
            return None;
        }
        let (cp, mate) = match score {
            Score::Centipawns(cp) => (Some(cp), None),
            Score::Mate(n) => (None, Some(n)),
            // Mated already: as an engine says it, mate 0 for the side that lost.
            Score::Mated(_) => (None, Some(0)),
        };
        let bound = match bound {
            Bound::Exact => None,
            Bound::Lower => Some("lower".to_owned()),
            Bound::Upper => Some("upper".to_owned()),
        };
        Some(Eval {
            depth,
            cp,
            mate,
            bound,
            pv: pv.iter().take(MAX_MOVES).cloned().collect(),
            engine: engine.map(|e| e.chars().filter(|c| !c.is_control()).take(MAX_ENGINE_CHARS).collect()),
            at,
            other: Map::new(),
        })
    }

    /// Whether this evaluation is the one to keep over `other`: deeper, or as deep and newer.
    fn outranks(&self, other: &Eval) -> bool {
        (self.depth, self.at) > (other.depth, other.at)
    }

    /// Whether it says something this build can show: a score, and a line of moves.
    fn readable(&self) -> bool {
        (self.cp.is_some() || self.mate.is_some()) && self.pv.len() <= MAX_MOVES && self.pv.iter().all(|m| uci::is_move(m))
    }
}

/// The key of `fen`'s position: its first four fields as the rules write them. `None` for a FEN
/// that cannot be read.
pub fn position_key(fen: &str) -> Option<String> {
    let fen = Fen::parse(fen).ok()?;
    let text = fen.to_string();
    let fields: Vec<&str> = text.split_ascii_whitespace().take(4).collect();
    (fields.len() == 4).then(|| fields.join(" "))
}

/// A document's file.
#[derive(Clone, Debug, PartialEq)]
pub struct EvalFile {
    pub version: u32,
    pub doc: DocRecord,
    pub evals: BTreeMap<String, Eval>,
    /// Fields of the file a later build of this version writes, kept.
    pub other: Map<String, Value>,
}

impl EvalFile {
    pub fn empty(doc: DocRecord) -> EvalFile {
        EvalFile { version: VERSION, doc, evals: BTreeMap::new(), other: Map::new() }
    }

    /// Keep `eval` for the position of `fen`, unless it keeps a deeper one (or as deep and
    /// newer). Whether it was kept. Past [`MAX_EVALS`] positions the oldest goes.
    pub fn keep(&mut self, fen: &str, eval: Eval) -> bool {
        let Some(key) = position_key(fen) else { return false };
        if self.evals.get(&key).is_some_and(|old| !eval.outranks(old)) {
            return false;
        }
        self.evals.insert(key, eval);
        self.trim();
        true
    }

    /// The evaluation kept for the position of `fen`.
    pub fn get(&self, fen: &str) -> Option<&Eval> {
        self.evals.get(&position_key(fen)?)
    }

    /// Down to [`MAX_EVALS`] positions, the oldest kept going first (by position on a tie).
    fn trim(&mut self) {
        let over = self.evals.len().saturating_sub(MAX_EVALS);
        if over == 0 {
            return;
        }
        let mut by_age: Vec<(u64, String)> = self.evals.iter().map(|(k, e)| (e.at, k.clone())).collect();
        by_age.sort();
        for (_, key) in by_age.into_iter().take(over) {
            self.evals.remove(&key);
        }
    }

    /// `theirs` merged into `self`: per position, the deeper evaluation (as deep: the newer; a
    /// tie keeps this one's).
    pub fn merge(&mut self, theirs: &EvalFile) {
        for (key, eval) in &theirs.evals {
            match self.evals.get(key) {
                Some(mine) if !eval.outranks(mine) => {}
                _ => {
                    self.evals.insert(key.clone(), eval.clone());
                }
            }
        }
        self.trim();
    }

    /// A file's bytes, the evaluations that can't be read left out and counted.
    pub fn parse(bytes: &[u8]) -> Parsed<EvalFile> {
        if bytes.len() > MAX_FILE_BYTES {
            return Parsed::Bad(format!("larger than {} MB", MAX_FILE_BYTES >> 20));
        }
        let value: Value = match serde_json::from_slice(bytes) {
            Ok(v) => v,
            Err(e) => return Parsed::Bad(format!("not JSON: {e}")),
        };
        let Value::Object(mut fields) = value else { return Parsed::Bad("not an evaluations file".into()) };
        if fields.remove("format").as_ref().and_then(Value::as_str) != Some(FORMAT) {
            return Parsed::Bad("not an evaluations file".into());
        }
        let Some(version) = fields.remove("version").as_ref().and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok()) else {
            return Parsed::Bad("no version this build reads".into());
        };
        if version == 0 {
            return Parsed::Bad("no version this build reads".into());
        }
        let doc: DocRecord = match fields.remove("doc").map(serde_json::from_value) {
            Some(Ok(doc)) => doc,
            Some(Err(e)) => return Parsed::Bad(format!("its document record: {e}")),
            None => return Parsed::Bad("no document record".into()),
        };
        let mut skipped = 0usize;
        let mut evals = BTreeMap::new();
        if let Some(Value::Object(entries)) = fields.remove("evals") {
            for (key, value) in entries {
                let eval = serde_json::from_value::<Eval>(value).ok().filter(Eval::readable);
                match (position_key(&key).filter(|k| *k == key), eval) {
                    (Some(key), Some(eval)) if evals.len() < MAX_EVALS => {
                        evals.insert(key, eval);
                    }
                    _ => skipped = skipped.saturating_add(1),
                }
            }
        }
        let file = EvalFile { version: version.min(VERSION), doc, evals, other: fields };
        if version > VERSION { Parsed::Newer { version, file: Some(EvalFile { version, ..file }) } } else { Parsed::File { file, skipped } }
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, EvalStoreError> {
        let mut out = self.other.clone();
        out.insert("format".into(), Value::from(FORMAT));
        out.insert("version".into(), Value::from(self.version.max(VERSION)));
        out.insert("doc".into(), serde_json::to_value(&self.doc).map_err(|e| EvalStoreError::Io(std::io::Error::other(e)))?);
        out.insert("evals".into(), serde_json::to_value(&self.evals).map_err(|e| EvalStoreError::Io(std::io::Error::other(e)))?);
        let bytes = serde_json::to_vec(&Value::Object(out)).map_err(|e| EvalStoreError::Io(std::io::Error::other(e)))?;
        if bytes.len() > MAX_FILE_BYTES {
            return Err(EvalStoreError::TooBig);
        }
        Ok(bytes)
    }
}

/// What keeping evaluations can fail with.
#[derive(Debug, thiserror::Error)]
pub enum EvalStoreError {
    #[error("the evaluations folder: {0}")]
    Io(#[from] std::io::Error),
    #[error("the document has no name, path or identifier to keep evaluations under")]
    NoKeys,
    #[error("another PdfCraft is writing the evaluations; try again")]
    Busy,
    #[error("the evaluations file was written by a newer PdfCraft (version {0}); this one only reads it")]
    Newer(u32),
    #[error("the evaluations kept here would take more than their {0} bytes")]
    OverBudget(usize),
    #[error("the evaluations file would be larger than {} MB", MAX_FILE_BYTES >> 20)]
    TooBig,
}

impl StoreFailure for EvalStoreError {
    fn no_keys() -> Self {
        EvalStoreError::NoKeys
    }

    fn busy() -> Self {
        EvalStoreError::Busy
    }

    fn newer(version: u32) -> Self {
        EvalStoreError::Newer(version)
    }

    fn over_budget(budget: usize) -> Self {
        EvalStoreError::OverBudget(budget)
    }
}

/// The evaluations as a kind of document store: `<hash>.evals.json`, merged by
/// [`EvalFile::merge`].
pub struct Evals;

impl StoreKind for Evals {
    const KIND: &'static str = "evals";
    const FORMAT: &'static str = FORMAT;
    const MAX_FILE_BYTES: usize = MAX_FILE_BYTES;
    /// New with plan A9-N8: no build before it writes evaluations.
    const OLD_LOCK: bool = false;
    type File = EvalFile;
    type Error = EvalStoreError;

    fn empty(doc: DocRecord) -> EvalFile {
        EvalFile::empty(doc)
    }

    fn doc(file: &EvalFile) -> &DocRecord {
        &file.doc
    }

    fn set_doc(file: &mut EvalFile, doc: DocRecord) {
        file.doc = doc;
    }

    fn read(bytes: &[u8]) -> Parsed<EvalFile> {
        EvalFile::parse(bytes)
    }

    fn to_bytes(file: &EvalFile) -> Result<Vec<u8>, EvalStoreError> {
        file.to_bytes()
    }

    fn holds_anything(file: &EvalFile) -> bool {
        !file.evals.is_empty()
    }

    fn merge(mine: &mut EvalFile, theirs: &EvalFile) {
        mine.merge(theirs);
    }
}

/// The evaluations in a folder, one file per document (desktop and command line).
#[cfg(not(target_arch = "wasm32"))]
pub type DirStore = DocStore<Evals, super::store::DirBackend>;

/// The evaluations in memory, within a byte budget of their own (the web build).
pub type MemStore = DocStore<Evals, super::store::MemBackend>;

impl<B: StoreBackend> DocStore<Evals, B> {
    /// Keep `evals` (each a FEN and its evaluation) for the document ([`EvalFile::keep`]): how
    /// many were kept. Nothing is written when none would be (a folder is not made); refused for a
    /// file of a newer version.
    pub fn keep(&self, keys: &DocKeys, pages: usize, evals: &[(String, Eval)]) -> Result<usize, EvalStoreError> {
        let now = self.load(keys, pages)?;
        if let Some(v) = now.newer {
            return Err(EvalStoreError::Newer(v));
        }
        let mut probe = now.file.clone();
        if !evals.iter().any(|(fen, e)| probe.keep(fen, e.clone())) {
            return Ok(0);
        }
        let (kept, _) = self.update(keys, pages, |f| Ok::<_, EvalStoreError>(evals.iter().filter(|(fen, e)| f.keep(fen, e.clone())).count()))?;
        Ok(kept)
    }
}

#[cfg(test)]
mod tests;
