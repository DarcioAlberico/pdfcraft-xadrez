//! Chess ▸ a book's review, taken to another machine (plan A9-G5b, §2.3, finding A8-19): the
//! diagram corrections, the decisions on the book's PGN and the games kept for one document, in one
//! file, the **review pack**, never the PGN's text.
//!
//! **Format** (version [`VERSION`]): one JSON file,
//! `{"format":"pdfcraft-chess-review-pack","version":1,"pack_id":…,"reader_version":…,"doc":{…},
//! "corrections":…,"reference":…,"games":…}`. Each part is its kind's own file as the stores
//! write it ([`super::corrections`], [`super::reference_store`] with no PGN, [`super::games_store`]),
//! so reading it back goes through each kind's own validation and caps: an entry that can't be read
//! is left out and counted ([`Unpacked::skipped`]), never the whole pack refused for it. The
//! document is recorded without the hash of its path (a hash anyone can guess, of no use on another machine)
//! and without its name unless asked ([`pack`]'s `name`); `pack_id` is a hash of what the pack holds,
//! so the same review packed twice is the same pack.
//!
//! **Importing** merges each part into the chosen document's stores by its kind's rule
//! ([`super::store::StoreKind::merge`]): corrections by uid and revision, decisions by move, games
//! by uid and revision; so importing the same pack twice changes nothing. What can't be used is
//! said, never dropped in silence: the caller counts the corrections whose page the document no
//! longer has and the decisions with no book's PGN to apply to.

use serde_json::{Map, Value, json};

use super::corrections::{CorrectionFile, Corrections, DocRecord, hash_hex};
use super::games_store::{Games, GamesFile};
use super::reference_store::{Reference, ReferenceFile};
use super::store::{Parsed, StoreKind};

/// The format's name.
pub const FORMAT: &str = "pdfcraft-chess-review-pack";

/// The version this build writes and reads.
pub const VERSION: u32 = 1;

/// Largest pack read or written: each part at its kind's largest file, and the rest.
pub const MAX_PACK_BYTES: usize = <Corrections as StoreKind>::MAX_FILE_BYTES
    .saturating_add(<Reference as StoreKind>::MAX_FILE_BYTES)
    .saturating_add(<Games as StoreKind>::MAX_FILE_BYTES)
    .saturating_add(64 << 10);

/// What a pack holds.
#[derive(Clone, Debug, PartialEq)]
pub struct Pack {
    /// A hash of what it holds.
    pub pack_id: String,
    /// The reading (`doc::READER_VERSION`) of the build that made it: the decisions' places are
    /// that reading's.
    pub reader_version: u32,
    /// The document it was made from: its identifier, its name when asked, its size and pages.
    pub doc: DocRecord,
    pub corrections: Option<CorrectionFile>,
    /// The decisions, the PGN itself left out.
    pub reference: Option<ReferenceFile>,
    pub games: Option<GamesFile>,
}

/// What [`unpack`] read: the pack, and how many entries of each part could not be read and were
/// left out.
#[derive(Clone, Debug, PartialEq)]
pub struct Unpacked {
    pub pack: Pack,
    pub skipped: Skipped,
}

/// Entries left out, by part.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Skipped {
    pub corrections: usize,
    pub decisions: usize,
    pub games: usize,
}

/// `doc` as a pack records it: no path hash, the name only when `name`.
fn bare(doc: &DocRecord, name: bool) -> DocRecord {
    DocRecord { path_hash: None, name: doc.name.clone().filter(|_| name), ..doc.clone() }
}

/// The pack of a document's review: its corrections, decisions and games (each `None` when the
/// document keeps none), its document recorded as [`Pack::doc`] says. The PGN is left out of the
/// reference: only the decisions go.
pub fn pack(doc: &DocRecord, name: bool, corrections: Option<CorrectionFile>, reference: Option<ReferenceFile>, games: Option<GamesFile>) -> Pack {
    let doc = bare(doc, name);
    let corrections = corrections.filter(|c| c.live().next().is_some()).map(|mut c| {
        c.doc = doc.clone();
        c
    });
    let reference = reference.filter(|r| !r.decisions.is_empty()).map(|mut r| {
        r.doc = doc.clone();
        r.pgn = None;
        r.removed = None;
        r
    });
    let games = games.filter(|g| !g.games.is_empty() || !g.trash.is_empty()).map(|mut g| {
        g.doc = doc.clone();
        g.instance = None;
        g
    });
    let mut p =
        Pack { pack_id: String::new(), reader_version: crate::chess::pdfcraft_chessvision::doc::READER_VERSION, doc, corrections, reference, games };
    p.pack_id = p.parts().map(|parts| hash_hex(&[b"review pack", parts.as_bytes()])).unwrap_or_default();
    p
}

impl Pack {
    /// The parts as JSON text, for the pack's id.
    fn parts(&self) -> Result<String, String> {
        let v = json!({ "corrections": part::<Corrections>(self.corrections.as_ref())?, "reference": part::<Reference>(self.reference.as_ref())?, "games": part::<Games>(self.games.as_ref())? });
        Ok(v.to_string())
    }

    /// The pack's bytes; refused past [`MAX_PACK_BYTES`].
    pub fn to_bytes(&self) -> Result<Vec<u8>, String> {
        let mut out = Map::new();
        out.insert("format".into(), json!(FORMAT));
        out.insert("version".into(), json!(VERSION));
        out.insert("pack_id".into(), json!(self.pack_id));
        out.insert("reader_version".into(), json!(self.reader_version));
        out.insert("doc".into(), serde_json::to_value(&self.doc).map_err(|e| e.to_string())?);
        out.insert("corrections".into(), part::<Corrections>(self.corrections.as_ref())?);
        out.insert("reference".into(), part::<Reference>(self.reference.as_ref())?);
        out.insert("games".into(), part::<Games>(self.games.as_ref())?);
        let bytes = serde_json::to_vec(&Value::Object(out)).map_err(|e| e.to_string())?;
        if bytes.len() > MAX_PACK_BYTES {
            return Err(format!("the pack would be larger than {} MB", MAX_PACK_BYTES >> 20));
        }
        Ok(bytes)
    }

    /// How many corrections (live), decisions, games (live) and trash items it holds.
    pub fn counts(&self) -> (usize, usize, usize, usize) {
        (
            self.corrections.as_ref().map_or(0, |c| c.live().count()),
            self.reference.as_ref().map_or(0, |r| r.decisions.len()),
            self.games.as_ref().map_or(0, |g| g.live().count()),
            self.games.as_ref().map_or(0, |g| g.trash.len()),
        )
    }
}

/// A part as its kind writes its file, as JSON; `null` for none.
fn part<K: StoreKind>(file: Option<&K::File>) -> Result<Value, String>
where
    K::Error: std::fmt::Display,
{
    let Some(file) = file else { return Ok(Value::Null) };
    let bytes = K::to_bytes(file).map_err(|e| e.to_string())?;
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}

/// A part read back by its kind's own reading: its file and the entries left out. A part of a
/// newer version, or one that is no file of its kind, is refused with why.
fn unpart<K: StoreKind>(value: Option<Value>, what: &str) -> Result<Option<(K::File, usize)>, String> {
    let Some(value) = value.filter(|v| !v.is_null()) else { return Ok(None) };
    let bytes = serde_json::to_vec(&value).map_err(|e| e.to_string())?;
    match K::read(&bytes) {
        Parsed::File { file, skipped } => Ok(Some((file, skipped))),
        Parsed::Newer { version, .. } => Err(format!("its {what} were written by a newer PdfCraft (version {version})")),
        Parsed::Bad(why) => Err(format!("its {what} can't be read: {why}")),
    }
}

/// The pack `bytes` hold, each part read by its kind's rules (plan A9 §2.3: the same validation
/// and caps as a store's file). Why not, when it is no pack this build reads.
pub fn unpack(bytes: &[u8]) -> Result<Unpacked, String> {
    if bytes.len() > MAX_PACK_BYTES {
        return Err(format!("larger than {} MB", MAX_PACK_BYTES >> 20));
    }
    let value: Value = serde_json::from_slice(bytes).map_err(|e| format!("not JSON: {e}"))?;
    let Value::Object(mut fields) = value else { return Err("not a review pack".into()) };
    if fields.get("format").and_then(Value::as_str) != Some(FORMAT) {
        return Err("not a review pack".into());
    }
    match fields.get("version").and_then(Value::as_u64) {
        Some(1) => {}
        Some(v) => return Err(format!("a review pack of version {v}, which this PdfCraft doesn't read")),
        None => return Err("a review pack without a version".into()),
    }
    let doc: DocRecord = serde_json::from_value(fields.remove("doc").ok_or("no document record")?).map_err(|e| e.to_string())?;
    let pack_id = fields.remove("pack_id").and_then(|v| v.as_str().map(str::to_owned)).filter(|p| p.len() <= 64).unwrap_or_default();
    let reader_version = fields.remove("reader_version").and_then(|v| v.as_u64()).map_or(0, |v| u32::try_from(v).unwrap_or(u32::MAX));
    let corrections = unpart::<Corrections>(fields.remove("corrections"), "corrections")?;
    let reference = unpart::<Reference>(fields.remove("reference"), "decisions")?;
    let games = unpart::<Games>(fields.remove("games"), "games")?;
    let skipped = Skipped {
        corrections: corrections.as_ref().map_or(0, |(_, n)| *n),
        decisions: reference.as_ref().map_or(0, |(_, n)| *n),
        games: games.as_ref().map_or(0, |(_, n)| *n),
    };
    let pack = Pack {
        pack_id,
        reader_version,
        doc,
        corrections: corrections.map(|(f, _)| f),
        reference: reference.map(|(f, _)| f),
        games: games.map(|(f, _)| f),
    };
    Ok(Unpacked { pack, skipped })
}

#[cfg(test)]
mod tests;
