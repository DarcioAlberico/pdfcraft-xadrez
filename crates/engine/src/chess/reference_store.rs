//! Chess: the book's own PGN kept with its document, so the book opens linked to it again (plan
//! "Carregar PGN do livro", R5; decision D3: a copy of the PGN, not a path to it).
//!
//! **Where** (plan A9 §2.3, step G3a): a kind of the document store ([`super::store::DocStore`],
//! kind [`Reference`]), as the diagram corrections are: one file per document,
//! `<hash>.reference.json`, found by the document's keys and the document it records
//! ([`DocRecord`]), in a folder ([`DirStore`], desktop and command line) or in memory
//! ([`MemStore`], the web build, within a byte budget of its own). The file builds before G3a
//! wrote, `<hash>.json`, is still read and merged in ([`ReferenceFile::merge`]), and never written,
//! moved or deleted; a corrections file under that name is never taken for the PGN, nor set aside
//! by it (A4-07). Reading only reads (A4-13): a file that can't be read is reported, and set aside
//! as `….bad` only by the next change, never deleted.
//!
//! **Format** (version [`VERSION`]):
//! `{"format":"pdfcraft-chess-reference","version":2,"doc":{…},"pgn":{…},"decisions":[…]}`. The
//! PGN is kept as its text (at most [`MAX_REFERENCE_BYTES`]), its hash, its size, the file name it
//! came from (never its path), when it was kept (`at`, seconds since 1970) and its games and
//! moves. A file whose text does not have its hash is not trusted. Removing the PGN leaves the
//! file without it and with when it was removed (`removed`), so a copy kept under the name before,
//! older, does not bring it back. Fields a later build of this version writes, in the file, the
//! PGN or a decision, are kept as they are. Version 1 (the PGN required, no `removed`) is read and
//! written again as version 2; a build of version 1 never finds a version 2 file, and opens one
//! only as read only.
//!
//! **Decisions** (plan "Carregar PGN do livro", R6e): what the user chose for a move of the text
//! the PGN plays from another position (a conflict), the book's move or the PGN's, each by the
//! move's place on its page and its SAN as printed, so a page read differently does not take
//! another move's decision. Each also keeps the PGN's move it was made against (`pgn_san`, plan
//! A9-G4). Keeping the same PGN again keeps them (the same text, the same file as a build before
//! plan A9-F1 read it, with U+FFFD where a byte was not UTF-8, or the same file's bytes:
//! [`KeptPgn::is_same_as`]). Keeping another PGN keeps them too (decision D25), and says how many
//! name a move the new PGN does not have ([`Kept::unmatched`]): those apply again only to a PGN
//! that has their move, since a decision is looked up only for a move in conflict with the PGN's
//! same move. A save as another file keeps them. A decision that can't be read is left out and
//! counted, the others kept.
//!
//! **A page read differently** (plan A9-L1b, §2.6, §2.10). A new reading may cut a page into other
//! blocks (A9-L1a, L3), so a move's place changes. Each decision also keeps its move's ordinal
//! among the page's moves printed the same (`ordinal`), and is found on the page as read now by
//! it ([`Decision::find`]); one made before the ordinal was kept is found as the only move of its
//! page printed so, else at its place. When the interface reads a page, the decisions made on it
//! are taken to their moves there once and written ([`ReferenceFile::rematch`]); one found
//! nowhere is kept and counted, never applied to another move ([`Decisions::on_page`]). The file
//! says the reading of the newest build that wrote it (`reader_version`, `doc::READER_VERSION`):
//! a file a later reading wrote is read only here, and none of its decisions applied.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::corrections::{self, DocRecord};
use super::keys::DocKeys;
use super::store::{DocStore, Parsed, StoreBackend, StoreFailure, StoreKind};
use pdfcraft_chessvision::doc::reference::MAX_REFERENCE_BYTES;
use pdfcraft_chessvision::doc::{ChessPage, Inline, MoveAt, READER_VERSION};

/// The format's name, in every file.
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

/// The user's decision on a move of the text: its place (page from 0, block and place in the
/// block, written as the three numbers files have always had), its SAN as the walk wrote it, the
/// SAN as read, the PGN's move it was in conflict with, and the choice.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    /// Where the move is (plan A9-T2a's one address; `page`, `block` and `inline` in the file).
    #[serde(flatten)]
    pub at: MoveAt,
    /// The move's SAN as the walk wrote it when the choice was made (what builds before plan
    /// A9-L0b match by, and still write).
    pub san: String,
    /// The move's SAN as read (`MoveInline::san_read`, plan A9-L0b), which a walk with other
    /// diagrams never changes; `None` in a decision made before it was kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub san_read: Option<String>,
    /// The move's place among the moves of its page read with the same SAN, from 0 (plan A9-L1b,
    /// §2.6): what finds it again when a later reading cuts the page into other blocks
    /// ([`Decision::find`]). `None` in a decision made before it was kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ordinal: Option<u32>,
    pub choice: Choice,
    /// The move of the book's PGN the move was in conflict with, as the PGN writes it (plan A9-G4,
    /// R6-10): for [`Choice::Pgn`] the move chosen. `None` in a decision made before it was kept,
    /// which stands for the move by its own SAN.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pgn_san: Option<String>,
    /// Fields a later build of this version wrote, kept as they are while the decision is.
    #[serde(flatten)]
    pub other: Map<String, Value>,
}

impl Decision {
    /// The choice `choice` on the move at `at` the walk wrote `san` and the reader read
    /// `san_read`, in conflict with the PGN's move `pgn_san` (empty: not known).
    pub fn new(at: MoveAt, san: String, san_read: String, pgn_san: String, choice: Choice) -> Decision {
        let san_read = Some(san_read).filter(|s| !s.is_empty());
        let pgn_san = Some(pgn_san).filter(|s| !s.is_empty());
        Decision { at, san, san_read, ordinal: None, choice, pgn_san, other: Map::new() }
    }

    /// This decision with its move's ordinal on `page`, the page it is made on as read now
    /// ([`Decision::ordinal`]), so a later reading finds it again; as it is when its move is not
    /// on that page.
    pub fn placed_on(mut self, page: &ChessPage) -> Decision {
        let at = self.at;
        self.ordinal = self.same_moves(page).iter().position(|m| *m == at).and_then(|n| u32::try_from(n).ok());
        self
    }

    /// The moves of `page` printed as this decision's (its SAN read; the SAN the walk wrote, for a
    /// decision made before the SAN read was kept), in reading order: every text block's, the
    /// page's furniture's too, so that a block that changes role from one reading to the next
    /// moves no move.
    fn same_moves(&self, page: &ChessPage) -> Vec<MoveAt> {
        let mut out = Vec::new();
        for (b, block) in page.blocks.iter().enumerate() {
            for (i, inline) in block.inlines().iter().enumerate() {
                let Inline::Move(m) = inline else { continue };
                let printed = match &self.san_read {
                    Some(read) => m.read_san() == read,
                    None => m.san == self.san,
                };
                if printed {
                    out.push(MoveAt::new(page.index, b, i));
                }
            }
        }
        out
    }

    /// Where this decision's move is on `page` as read now, when that is its page (plan A9-L1b,
    /// §2.6): a reading that cuts a page into other blocks (A9-L1a, L3) moves its moves to other
    /// places. Among the page's moves printed as it was ([`Decision::same_moves`]), the one of its
    /// [`Decision::ordinal`]; for a decision made before that was kept, the only one, else the one
    /// at its place, when that is one of them (the rule it was found by before). `None` when none
    /// is, and the decision is unmatched: kept, never applied to another move.
    pub fn find(&self, page: &ChessPage) -> Option<MoveAt> {
        if self.at.page != page.index {
            return None;
        }
        let same = self.same_moves(page);
        match self.ordinal {
            Some(n) => usize::try_from(n).ok().and_then(|n| same.get(n)).copied(),
            None if same.len() == 1 => same.first().copied(),
            None => Some(self.at).filter(|at| same.contains(at)),
        }
    }

    /// The move of the PGN it was made against: [`Decision::pgn_san`], else (a decision made
    /// before it was kept) the move's own SAN, which a move in conflict shares with the PGN's.
    pub fn pgn_move(&self) -> &str {
        self.pgn_san.as_deref().unwrap_or(&self.san)
    }

    /// Whether `self` and `other` are decisions on the same move: the same address, and the
    /// same SAN read when both kept one, else the same SAN the walk wrote.
    fn same_move(&self, other: &Decision) -> bool {
        self.at == other.at
            && match (&self.san_read, &other.san_read) {
                (Some(a), Some(b)) => a == b,
                _ => self.san == other.san,
            }
    }

    /// Whether it names a move this build keeps: a SAN of 1 to [`MAX_SAN`] bytes, the SAN read
    /// and the PGN's no longer.
    fn names_a_move(&self) -> bool {
        let short = |s: &Option<String>| s.as_ref().is_none_or(|s| s.len() <= MAX_SAN);
        !self.san.is_empty() && self.san.len() <= MAX_SAN && short(&self.san_read) && short(&self.pgn_san)
    }
}

/// The move at `at` on `page`, when there is one.
fn move_on(page: &ChessPage, at: MoveAt) -> Option<&pdfcraft_chessvision::doc::MoveInline> {
    let block = page.blocks.get(usize::try_from(at.block).ok()?)?;
    match block.inlines().get(usize::try_from(at.inline).ok()?)? {
        Inline::Move(m) => Some(m),
        _ => None,
    }
}

/// Take the decisions made on `page` to their moves in its reading now (plan A9-L1b, §2.6):
/// each one found ([`Decision::find`]) gets its move's place, SAN as read and ordinal, so it is
/// found by them from now on, past any later reading; one not found is left as it is. Two found
/// on one move: the later made keeps it, the other is left as it was. How many changed.
fn rematch(decisions: &mut [Decision], page: &ChessPage) -> usize {
    let mut taken: Vec<MoveAt> = Vec::new();
    let mut changed = 0usize;
    for d in decisions.iter_mut().rev() {
        let Some(at) = d.find(page).filter(|at| !taken.contains(at)) else { continue };
        taken.push(at);
        // A SAN read longer than a decision names (no move) would leave the decision unreadable.
        let Some(m) = move_on(page, at).filter(|m| m.read_san().len() <= MAX_SAN) else { continue };
        let placed = Decision {
            pgn_san: d.pgn_san.clone(),
            other: d.other.clone(),
            ..Decision::new(at, d.san.clone(), m.read_san().to_owned(), String::new(), d.choice).placed_on(page)
        };
        if placed != *d {
            changed = changed.saturating_add(1);
            *d = placed;
        }
    }
    changed
}

/// A document's decisions as kept, to find on its pages as read (plan A9-L1b, §2.6): the
/// interface shows the choice made on a move of the page it reads, and says how many choices made
/// on that page it could not find.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Decisions {
    /// [`ReferenceFile::reader_version`].
    pub reader_version: u32,
    pub list: Vec<Decision>,
}

/// The decisions made on a page, as found on it ([`Decisions::on_page`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OnPage {
    /// Each move a decision was found on, with the choice: the decision made last, when two
    /// were found on one move.
    pub found: Vec<(MoveAt, Choice)>,
    /// Decisions made on the page and found on no move of it (kept, never applied elsewhere).
    pub unmatched: usize,
}

impl Decisions {
    /// The decisions `file` keeps.
    pub fn of(file: &ReferenceFile) -> Decisions {
        Decisions { reader_version: file.reader_version, list: file.decisions.clone() }
    }

    /// The decisions made on `page`, found on its moves as read now ([`Decision::find`]). A file
    /// a later reading wrote applies none: every decision made on the page is unmatched.
    pub fn on_page(&self, page: &ChessPage) -> OnPage {
        let mut out = OnPage::default();
        let mine = self.list.iter().rev().filter(|d| d.at.page == page.index);
        for d in mine {
            let found = if self.reader_version > READER_VERSION { None } else { d.find(page) };
            match found {
                // An earlier choice on a move chosen again since: the later one stands.
                Some(at) if out.found.iter().any(|(f, _)| *f == at) => {}
                Some(at) => out.found.push((at, d.choice)),
                None => out.unmatched = out.unmatched.saturating_add(1),
            }
        }
        out
    }

    /// Whether [`ReferenceFile::rematch`] would change a decision made on `page`.
    pub fn need_rematch(&self, page: &ChessPage) -> bool {
        if self.reader_version > READER_VERSION {
            return false;
        }
        rematch(&mut self.list.clone(), page) > 0
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
    /// A hash of the file's bytes as read ([`bytes_hash`], plan A9-G4), so the same file is the
    /// same PGN whatever a reader makes of its text; `None` for a PGN kept before it was kept, or
    /// given as text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes_hash: Option<String>,
    /// Fields a later build of this version wrote, kept as they are while the PGN is.
    #[serde(flatten)]
    pub other: Map<String, Value>,
}

impl KeptPgn {
    /// The PGN `text`, read from a file named `name`, with its `games` and `moves`, kept at `at`.
    /// Refused when it is larger than [`MAX_REFERENCE_BYTES`].
    pub fn new(name: Option<&str>, text: String, games: usize, moves: usize, at: u64) -> Result<KeptPgn, ReferenceStoreError> {
        if text.len() > MAX_REFERENCE_BYTES {
            return Err(ReferenceStoreError::TooBig);
        }
        let name = name.filter(|n| !n.is_empty() && n.len() <= MAX_NAME).map(str::to_owned);
        Ok(KeptPgn {
            name,
            hash: text_hash(&text),
            bytes: text.len(),
            at,
            games,
            moves,
            text,
            legacy_hash: None,
            bytes_hash: None,
            other: Map::new(),
        })
    }

    /// This PGN, read from the file's `bytes` (by `pdfcraft_chess::pgn::decode`): also known by
    /// the hash the readers before plan A9-F1 gave those bytes, and by the bytes' own hash.
    pub fn read_from(mut self, bytes: &[u8]) -> KeptPgn {
        self.legacy_hash = Some(text_hash(&pdfcraft_chess::pgn::legacy_text(bytes)));
        self.bytes_hash = Some(bytes_hash(bytes));
        self
    }

    /// Whether this is the PGN `kept` holds: the same text, the same file as a build before plan
    /// A9-F1 kept it, or the same file's bytes (plan A9-G4).
    pub fn is_same_as(&self, kept: &KeptPgn) -> bool {
        self.hash == kept.hash
            || self.legacy_hash.as_deref() == Some(kept.hash.as_str())
            || self.bytes_hash.as_ref().is_some_and(|b| kept.bytes_hash.as_ref() == Some(b))
    }

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
/// cryptographic).
pub fn text_hash(text: &str) -> String {
    corrections::hash_hex(&[b"pgn", text.as_bytes()])
}

/// The hash of a PGN file's bytes as read, before any decoding: 32 hexadecimal digits.
pub fn bytes_hash(bytes: &[u8]) -> String {
    corrections::hash_hex(&[b"pgn-bytes", bytes])
}

/// What keeping a PGN did ([`ReferenceFile::keep`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Kept {
    /// A PGN was kept before.
    pub replaced: bool,
    /// Decisions kept that name a move the PGN does not have (another PGN, decision D25): they
    /// apply again only to a PGN that has it.
    pub unmatched: usize,
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
    /// The reading of the newest build that wrote the file (`doc::READER_VERSION`, plan A9 §2.10,
    /// L1b); 0 for a file written before it was kept. A file a later reading wrote is read only
    /// here, and none of its decisions is applied ([`Decisions::on_page`]): their places and
    /// ordinals are that reading's.
    pub reader_version: u32,
    /// Fields a later build of this version wrote, kept as they are.
    pub other: Map<String, Value>,
}

/// What a file's bytes are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Read {
    File(ReferenceFile),
    /// A file of a newer version: kept as it is, never written or removed by this build. What
    /// this build can make out of it, for reading.
    Newer(u32, Option<ReferenceFile>),
    /// Not a file this build can read, and why: set it aside, never delete it.
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
    reader_version: u32,
    #[serde(flatten)]
    other: &'a Map<String, Value>,
}

impl ReferenceFile {
    /// The file for the document `doc` keeping nothing.
    pub fn empty(doc: DocRecord) -> ReferenceFile {
        ReferenceFile {
            format: FORMAT.into(),
            version: VERSION,
            doc,
            pgn: None,
            removed: None,
            decisions: Vec::new(),
            reader_version: READER_VERSION,
            other: Map::new(),
        }
    }

    /// The file for the document `doc` keeping `pgn`.
    pub fn new(doc: DocRecord, pgn: KeptPgn) -> ReferenceFile {
        ReferenceFile { pgn: Some(pgn), ..ReferenceFile::empty(doc) }
    }

    /// Read a file's bytes: this version's file (version 1 taken in as this one), a newer one
    /// (read only), or why not. Decisions that can't be read are left out
    /// ([`ReferenceFile::parse`] counts them).
    pub fn read(bytes: &[u8]) -> Read {
        match ReferenceFile::parse(bytes) {
            Parsed::File { file, .. } => Read::File(file),
            Parsed::Newer { version, file } => Read::Newer(version, file),
            Parsed::Bad(why) => Read::Bad(why),
        }
    }

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
                // Written by a later reading (plan A9 §2.10): read only, its decisions not applied.
                Ok((file, _)) if file.reader_version > READER_VERSION => Parsed::Newer { version: file.reader_version, file: Some(file) },
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
        // Not written before plan A9-L1b: 0.
        let reader_version = fields.remove("reader_version").and_then(|v| v.as_u64()).map_or(0, |v| u32::try_from(v).unwrap_or(u32::MAX));
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
        Ok((ReferenceFile { format: FORMAT.into(), version: VERSION, doc, pgn, removed, decisions, reader_version, other: fields }, skipped))
    }

    /// Take the decisions made on `page` to their moves in its reading now (plan A9-L1b, §2.6):
    /// [`rematch`]. How many changed.
    pub fn rematch(&mut self, page: &ChessPage) -> usize {
        rematch(&mut self.decisions, page)
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

    /// Keep `pgn` in place of the PGN kept. The decisions stay (decision D25): with the same PGN
    /// all of them hold; with another, those whose move ([`Decision::pgn_move`]) `has_move` says
    /// the new PGN does not have are counted ([`Kept::unmatched`]).
    pub fn keep(&mut self, pgn: KeptPgn, has_move: impl Fn(&str) -> bool) -> Kept {
        let before = self.pgn.take();
        let same = before.as_ref().is_some_and(|old| pgn.is_same_as(old));
        let unmatched = if same { 0 } else { self.decisions.iter().filter(|d| !has_move(d.pgn_move())).count() };
        self.pgn = Some(pgn);
        self.removed = None;
        Kept { replaced: before.is_some(), unmatched }
    }

    /// Remove the PGN kept, and the decisions made against it, at `at` (seconds since 1970).
    /// Whether one was kept.
    pub fn remove(&mut self, at: u64) -> bool {
        let had = self.pgn.take().is_some();
        if had {
            self.removed = Some(at);
            self.decisions.clear();
        }
        had
    }

    /// When the PGN was last kept or removed (0: never).
    fn changed_at(&self) -> u64 {
        self.pgn.as_ref().map_or(self.removed.unwrap_or(0), |p| p.at)
    }

    /// Take `theirs` in (the file of the name before plan A9-G3a, or another copy): the PGN kept
    /// or removed the later wins (a tie keeps this one's). While a PGN is kept, each side's
    /// decisions are kept (decision D25: they outlive a change of PGN), the winner's where both
    /// decided on a move; a removal the later takes them all away.
    pub fn merge(&mut self, theirs: &ReferenceFile) {
        let (mut first, mut second) = (self.decisions.clone(), theirs.decisions.clone());
        if theirs.changed_at() > self.changed_at() {
            self.pgn.clone_from(&theirs.pgn);
            self.removed = theirs.removed;
            std::mem::swap(&mut first, &mut second);
        }
        if self.pgn.is_none() {
            if self.removed.is_some() {
                self.decisions.clear();
            }
            return;
        }
        for d in second {
            if first.len() >= MAX_DECISIONS {
                break;
            }
            if !first.iter().any(|kept| kept.same_move(&d)) {
                first.push(d);
            }
        }
        self.decisions = first;
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
            // This build's reading wrote it (a later reading's file is never written here).
            reader_version: READER_VERSION,
            other: &self.other,
        };
        let bytes = serde_json::to_vec(&written).map_err(|_| ReferenceStoreError::TooBig)?;
        if bytes.len() > MAX_FILE_BYTES { Err(ReferenceStoreError::TooBig) } else { Ok(bytes) }
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

impl StoreFailure for ReferenceStoreError {
    fn no_keys() -> Self {
        ReferenceStoreError::NoKeys
    }

    fn busy() -> Self {
        ReferenceStoreError::Busy
    }

    fn newer(version: u32) -> Self {
        ReferenceStoreError::Newer(version)
    }

    fn over_budget(budget: usize) -> Self {
        ReferenceStoreError::OverBudget(budget)
    }
}

/// The book's PGN as a kind of document store (plan A9 §2.3): `<hash>.reference.json`, merged by
/// [`ReferenceFile::merge`].
pub struct Reference;

impl StoreKind for Reference {
    const KIND: &'static str = "reference";
    const FORMAT: &'static str = FORMAT;
    const MAX_FILE_BYTES: usize = MAX_FILE_BYTES;
    /// Builds of version 1 never find a `<hash>.reference.json` file (they look for version 1),
    /// and only read the `<hash>.json` they write: they never write a file this build writes.
    const OLD_LOCK: bool = false;
    type File = ReferenceFile;
    type Error = ReferenceStoreError;

    fn empty(doc: DocRecord) -> ReferenceFile {
        ReferenceFile::empty(doc)
    }

    fn doc(file: &ReferenceFile) -> &DocRecord {
        &file.doc
    }

    fn set_doc(file: &mut ReferenceFile, doc: DocRecord) {
        file.doc = doc;
    }

    fn read(bytes: &[u8]) -> Parsed<ReferenceFile> {
        ReferenceFile::parse(bytes)
    }

    fn to_bytes(file: &ReferenceFile) -> Result<Vec<u8>, ReferenceStoreError> {
        file.to_bytes()
    }

    fn holds_anything(file: &ReferenceFile) -> bool {
        file.pgn.is_some() || file.removed.is_some() || !file.decisions.is_empty()
    }

    fn merge(mine: &mut ReferenceFile, theirs: &ReferenceFile) {
        mine.merge(theirs);
    }
}

/// The books' PGNs in a folder, one file per document (desktop and command line).
#[cfg(not(target_arch = "wasm32"))]
pub type DirStore = DocStore<Reference, super::store::DirBackend>;

/// The books' PGNs in memory, within a byte budget of their own: the web build's store, which it
/// saves in the browser's storage ([`DocStore::files`]).
pub type MemStore = DocStore<Reference, super::store::MemBackend>;

/// What a store found for a document: its file (empty when it keeps nothing), where it is, and
/// what could not be read.
pub type Loaded = super::store::Loaded<ReferenceFile>;

impl super::store::Loaded<ReferenceFile> {
    /// The PGN kept, when there is one this build can read.
    pub fn pgn(&self) -> Option<&KeptPgn> {
        self.file.pgn.as_ref()
    }
}

impl<B: StoreBackend> DocStore<Reference, B> {
    /// Keep `pgn` for the document, in place of the one it kept, with the decisions
    /// ([`ReferenceFile::keep`]; `has_move`: whether `pgn` has a move). Refused for a file of a
    /// newer version.
    pub fn set(&self, keys: &DocKeys, pages: usize, pgn: KeptPgn, has_move: impl Fn(&str) -> bool) -> Result<(Kept, Loaded), ReferenceStoreError> {
        self.update(keys, pages, |f| Ok::<_, ReferenceStoreError>(f.keep(pgn, has_move)))
    }

    /// Remove the document's PGN at `at` ([`ReferenceFile::remove`]). Whether it kept one;
    /// refused for a file of a newer version. With nothing to remove nothing is written (a folder
    /// is not made).
    pub fn clear(&self, keys: &DocKeys, pages: usize, at: u64) -> Result<bool, ReferenceStoreError> {
        let now = self.load(keys, pages)?;
        if let Some(v) = now.newer {
            return Err(ReferenceStoreError::Newer(v));
        }
        if now.file.pgn.is_none() {
            return Ok(false);
        }
        self.update(keys, pages, |f| Ok::<_, ReferenceStoreError>(f.remove(at))).map(|(had, _)| had)
    }

    /// Keep `decision` with the document's PGN (R6e), in place of the one on the same move, the
    /// decisions made before on its page, `page` as read now, taken to their moves first
    /// ([`ReferenceFile::rematch`]), so one made on the same move under another reading is
    /// replaced too. The file as kept; refused when the document keeps no PGN (nothing is
    /// written, a folder is not made), and for a file of a newer version.
    pub fn decide(&self, keys: &DocKeys, pages: usize, decision: Decision, page: Option<&ChessPage>) -> Result<Loaded, ReferenceStoreError> {
        let now = self.load(keys, pages)?;
        if now.newer.is_none() && now.file.pgn.is_none() {
            return Err(ReferenceStoreError::NoPgn);
        }
        let ((), l) = self.update(keys, pages, |f| {
            if f.pgn.is_none() {
                return Err(ReferenceStoreError::NoPgn);
            }
            if let Some(page) = page {
                f.rematch(page);
            }
            f.decide(decision)
        })?;
        Ok(l)
    }

    /// Take the decisions made on `page`, as read now, to their moves there and write them (plan
    /// A9-L1b, [`ReferenceFile::rematch`]): the file as kept when one changed; `None`, and nothing
    /// written, when none did, and for a file a later build wrote.
    pub fn rematch(&self, keys: &DocKeys, pages: usize, page: &ChessPage) -> Result<Option<Loaded>, ReferenceStoreError> {
        let now = self.load(keys, pages)?;
        if now.newer.is_some() || !Decisions::of(&now.file).need_rematch(page) {
            return Ok(None);
        }
        let (_, l) = self.update(keys, pages, |f| Ok::<_, ReferenceStoreError>(f.rematch(page)))?;
        Ok(Some(l))
    }
}

#[cfg(test)]
mod tests;
