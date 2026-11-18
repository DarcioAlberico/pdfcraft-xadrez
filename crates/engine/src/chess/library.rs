//! The Library: every book of the folders a person chooses, read once and indexed, so a position,
//! a move, a player or a material balance is found across all of them without reading a page
//! (plan A9-N15a, finding A8-08; decisions D22 and D31). Desktop only.
//!
//! **Only when asked** (D31 B): [`Library::build`] walks the folders ([`walk`]: no link
//! followed, nothing in the cloud opened, caps on depth, entries and books) and reads each book
//! that has no index made under its key, one at a time, as "Detect in the book" and the Text tab
//! read it: the scan on the cores (plan A9-V10), the pages kept on disk (plan A9-N5), the book's
//! corrections and its PGN, then chess OCR where the text needs it. What it found is the book's
//! index ([`index`]).
//!
//! **Idempotent and resumable, book by book.** A book is its content (its SHA-256: one book with
//! every path holding the same bytes). Its index records the key it was made under
//! ([`index_key`]: the content, the versions of the reading, its settings, the digests of the
//! models, of the book's corrections and of its PGN): a book whose index is under the key it would
//! be made under now is not read again; a changed book, a new model, a correction or another PGN
//! makes it read again. A book stopped between two pages left the pages it read on disk, and the
//! next build goes on from them.
//!
//! **What is not indexed is said** ([`State`], [`Skip`]): a book that does not allow copying, an
//! encrypted one until the person says yes to it (D31 A), a file that does not open; a book read
//! without the piece model has its diagrams unread, says so, and is read again when the model
//! comes (its key names the model).
//!
//! **Its own space** (D31 C), in the app's data folder's `chess/library`: `catalog.json` (the
//! folders and every book: its paths, size, time, hash, state and counts) and `<hash>.lib` for
//! each book indexed, within [`MAX_BYTES`]; and `pages/`, the pages of the book being read, kept
//! in the mould of N5's readings within [`MAX_PAGE_BYTES`] and taken out once its index is
//! written. The app's own readings (`chess/reading`) are looked at and never changed
//! ([`crate::Session::set_chess_peek`]): a book read in the app is not read again, and nothing the
//! person opens is pushed out of the 512 MiB of D21.

pub mod index;
pub mod walk;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::Digest as _;

use self::index::{IndexCounts, IndexKey, ReadError};
use super::document::{OcrMode, OcrOptions};
use super::{ChessModels, ChessScanSettings, ReadingCache, reading};
use crate::Session;

/// The folder's name in the app's chess folder.
pub const KIND: &str = "library";
/// The folder of the pages in progress, in the Library's.
pub const PAGES: &str = "pages";
/// Most bytes of indexes kept (plan A9-N15a; outside D21's 512 MiB).
pub const MAX_BYTES: u64 = 128 << 20;
/// Most bytes of the pages of the books in progress.
pub const MAX_PAGE_BYTES: u64 = 128 << 20;
/// Largest PDF read.
pub const MAX_BOOK_BYTES: usize = 1 << 30;
/// The version of what makes an index: raised when the index made of the same reading changes.
pub const KEY_VERSION: u32 = 1;

const CATALOG: &str = "catalog.json";
const CATALOG_FORMAT: &str = "pdfcraft-chess-library";
const CATALOG_VERSION: u32 = 1;
/// Largest catalog read.
const MAX_CATALOG_BYTES: usize = 64 << 20;
const LIB: &str = ".lib";
const SET_ASIDE: &str = ".lib.bad";

/// Where a book stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// Its index is written.
    Indexed,
    /// Stopped between two pages: the pages read are on disk, and the next build goes on.
    Partial,
    /// Not reached yet (`max_books`, or a stop before it).
    #[default]
    Pending,
    /// Not indexed, and why ([`Skip`]).
    Skipped,
    /// Its data is in the cloud, not on this computer: never opened.
    Cloud,
    /// Its index would pass [`MAX_BYTES`]: not written.
    Full,
}

impl State {
    pub fn name(self) -> &'static str {
        match self {
            State::Indexed => "indexed",
            State::Partial => "partial",
            State::Pending => "pending",
            State::Skipped => "skipped",
            State::Cloud => "cloud",
            State::Full => "full",
        }
    }
}

/// Why a book is not indexed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Skip {
    /// Its permissions do not allow copying its content: nothing read of it is kept.
    Never,
    /// It is encrypted: left out until the person says yes to it (D31 A).
    Ask,
    /// It does not open (the error says why).
    DoesNotOpen,
    /// Larger than [`MAX_BOOK_BYTES`].
    TooLarge,
    /// The file can't be read.
    Unreadable,
    /// Its reading failed, or its index can't be written (the error says why).
    Failed,
}

impl Skip {
    pub fn name(self) -> &'static str {
        match self {
            Skip::Never => "never",
            Skip::Ask => "ask",
            Skip::DoesNotOpen => "does_not_open",
            Skip::TooLarge => "too_large",
            Skip::Unreadable => "unreadable",
            Skip::Failed => "failed",
        }
    }
}

/// A book in the catalog.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogBook {
    /// Its content's SHA-256 in hexadecimal: its index's name. `None` before it was read.
    #[serde(default)]
    pub hash: Option<String>,
    /// Every path holding it, as the walk found them.
    pub paths: Vec<String>,
    pub size: u64,
    /// Its time, in seconds since 1970.
    pub modified: u64,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub pages: Option<u32>,
    pub state: State,
    #[serde(default)]
    pub reason: Option<Skip>,
    #[serde(default)]
    pub error: Option<String>,
    /// What its index holds.
    #[serde(default)]
    pub counts: Option<IndexCounts>,
    /// Read without the piece model: its diagrams drawn in pictures are not read.
    #[serde(default)]
    pub no_piece_model: bool,
    #[serde(default)]
    pub index_bytes: u64,
}

/// What the last walk said of the folders.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WalkNote {
    pub entries: usize,
    #[serde(default)]
    pub capped: Vec<walk::Cap>,
    /// Links not followed, and folders in the cloud not listed.
    #[serde(default)]
    pub links: Vec<String>,
    #[serde(default)]
    pub cloud_folders: Vec<String>,
    /// Folders that could not be listed, and why.
    #[serde(default)]
    pub unlisted: Vec<(String, String)>,
}

/// `catalog.json`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Catalog {
    pub format: String,
    pub version: u32,
    /// The folders chosen.
    pub dirs: Vec<String>,
    pub books: Vec<CatalogBook>,
    #[serde(default)]
    pub walk: WalkNote,
    /// When it was last written, in seconds since 1970.
    #[serde(default)]
    pub updated: u64,
}

/// How [`Library::build`] reads the books.
#[derive(Clone, Debug, Default)]
pub struct BuildOptions {
    /// The folder of the models (`None`: none; the frontends pass what `ChessModels::locate`
    /// finds).
    pub models_dir: Option<PathBuf>,
    /// Workers a book's scan runs on (0: `ChessScanJob::default_workers`).
    pub workers: usize,
    /// Most books read in this call (`None`: all): a book whose index is current is not one.
    pub max_books: Option<usize>,
    pub caps: walk::Caps,
    /// The folders of the books' corrections and of their PGNs, only read (`None`: none).
    pub corrections: Option<PathBuf>,
    pub reference: Option<PathBuf>,
    /// The app's readings, looked in and never changed (`None`: not looked in).
    pub app_reading: Option<PathBuf>,
    /// The encrypted books the person said yes to, by hash (D31 A).
    pub allowed: Vec<String>,
}

/// Where a build is, for its progress.
#[derive(Clone, Copy, Debug)]
pub struct Progress<'a> {
    /// The book, from 0, of the PDFs the walk found.
    pub book: usize,
    pub books: usize,
    pub path: &'a Path,
    /// The pages scanned of it, of how many (0 of 0 before its scan).
    pub page: usize,
    pub pages: usize,
}

/// What a build did with one book in this call.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BookRun {
    /// Its index was current: nothing read.
    pub current: bool,
    /// Pages scanned now (`Session::chess_scanned`).
    pub scanned: usize,
    /// Page readings (a page's scan or its chess OCR) taken from the Library's pages in progress,
    /// from the app's readings, kept in the Library's, and set aside there.
    pub from_library: usize,
    pub from_app: usize,
    pub kept: usize,
    pub set_aside: usize,
    /// Its index was damaged: set aside (`<hash>.lib.bad`) and made again.
    pub index_set_aside: bool,
    pub seconds: f64,
}

/// One book of a build.
#[derive(Clone, Debug, PartialEq)]
pub struct BookReport {
    pub book: CatalogBook,
    /// `None`: not looked at in this call (kept as the catalog had it).
    pub run: Option<BookRun>,
}

/// What [`Library::build`] did.
#[derive(Clone, Debug, PartialEq)]
pub struct Built {
    pub books: Vec<BookReport>,
    pub walk: WalkNote,
    /// PDFs in the cloud, not opened.
    pub cloud: usize,
    /// Books read in this call.
    pub read: usize,
    /// Stopped (the progress said so).
    pub stopped: bool,
    /// Books left to read: a call with `max_books` stopped before them, or a stop.
    pub left: usize,
    /// Bytes of indexes in the Library.
    pub bytes: u64,
}

/// What [`Library::clear`] removed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cleared {
    pub indexes: usize,
    pub files: usize,
    pub bytes: u64,
}

/// The Library in a folder.
#[derive(Clone, Debug)]
pub struct Library {
    dir: PathBuf,
}

/// Bytes in lower-case hexadecimal.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Whether `name` is a book's hash: 64 hexadecimal digits.
pub fn is_hash(name: &str) -> bool {
    name.len() == 64 && name.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Seconds since 1970 now.
fn now() -> u64 {
    walk::secs(Some(std::time::SystemTime::now()))
}

/// The key a book's index is made under: its content, the versions of the reading and of the
/// index, the scan's settings, the piece model, the glyph model and the general OCR (by their
/// digests, never their folders), the chess OCR's mode, and the digests of the book's corrections
/// and its PGN (`None`: it has none).
pub fn index_key(content: &[u8; 32], settings: &ChessScanSettings, models: &ModelDigests<'_>) -> IndexKey {
    let mut h = sha2::Sha256::new();
    let mut bytes = |b: &[u8]| {
        h.update(u64::try_from(b.len()).unwrap_or(u64::MAX).to_le_bytes());
        h.update(b);
    };
    bytes(b"pdfcraft-chess-library-key");
    for v in [KEY_VERSION, index::VERSION, reading::VERSION, reading::SCAN_VERSION, pdfcraft_chessvision::doc::READER_VERSION] {
        bytes(&v.to_le_bytes());
    }
    bytes(content);
    let s = settings.sanitized();
    bytes(&s.dpi.to_bits().to_le_bytes());
    bytes(&s.max_read_dpi.to_bits().to_le_bytes());
    bytes(&[u8::from(s.order == super::ReadingOrder::Row), u8::from(s.allow_unverified_fonts)]);
    bytes(&u64::try_from(s.max_diagrams).unwrap_or(u64::MAX).to_le_bytes());
    let mut either = |name: &[u8], digest: Option<&[u8]>| match digest {
        Some(d) => {
            bytes(name);
            bytes(d);
        }
        None => bytes(&[name, b" none"].concat()),
    };
    either(b"pieces", models.pieces.map(<[u8; 32]>::as_slice));
    either(b"glyphs", models.glyphs.map(<[u8; 32]>::as_slice));
    either(b"general ocr", models.general.map(<[u8; 32]>::as_slice));
    either(b"ocr auto; corrections", models.corrections.map(<[u8; 32]>::as_slice));
    either(b"pgn", models.pgn.map(str::as_bytes));
    IndexKey(h.finalize().into())
}

/// What a book's index depends on besides its content and the scan's settings ([`index_key`]):
/// the digests of the piece model, the glyph model and the general OCR, of the book's corrections
/// and of its PGN (`None`: none).
#[derive(Clone, Copy, Debug, Default)]
pub struct ModelDigests<'a> {
    pub pieces: Option<&'a [u8; 32]>,
    pub glyphs: Option<&'a [u8; 32]>,
    pub general: Option<&'a [u8; 32]>,
    pub corrections: Option<&'a [u8; 32]>,
    pub pgn: Option<&'a str>,
}

impl Library {
    /// The Library in `dir` (made when something is first kept).
    pub fn new(dir: impl Into<PathBuf>) -> Library {
        Library { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The folder of the pages in progress.
    pub fn pages_dir(&self) -> PathBuf {
        self.dir.join(PAGES)
    }

    /// The index file of the book `hash`.
    pub fn index_path(&self, hash: &str) -> PathBuf {
        self.dir.join(format!("{hash}{LIB}"))
    }

    /// The catalog, as written: an empty one when there is none, or none to trust (one that
    /// can't be read is left where it is, and the next build writes another over it).
    pub fn catalog(&self) -> Catalog {
        super::corrections::read_capped(&self.dir.join(CATALOG), MAX_CATALOG_BYTES)
            .ok()
            .filter(|b| b.len() <= MAX_CATALOG_BYTES)
            .and_then(|b| serde_json::from_slice::<Catalog>(&b).ok())
            .filter(|c| c.format == CATALOG_FORMAT && c.version == CATALOG_VERSION)
            .unwrap_or_default()
    }

    fn write_catalog(&self, catalog: &mut Catalog) -> std::io::Result<()> {
        catalog.format = CATALOG_FORMAT.into();
        catalog.version = CATALOG_VERSION;
        catalog.updated = now();
        let json = serde_json::to_vec_pretty(catalog).map_err(std::io::Error::other)?;
        std::fs::create_dir_all(&self.dir)?;
        reading::write_whole(&self.dir.join(CATALOG), &json)
    }

    /// The indexes in the folder: (hash, bytes), and how many are set aside.
    pub fn indexes(&self) -> (Vec<(String, u64)>, usize) {
        let mut out = Vec::new();
        let mut aside = 0usize;
        let Ok(list) = std::fs::read_dir(&self.dir) else { return (out, 0) };
        for e in list.flatten().take(walk::MAX_BOOKS.saturating_mul(4)) {
            let Some(name) = e.file_name().to_str().map(str::to_owned) else { continue };
            if let Some(hash) = name.strip_suffix(LIB).filter(|h| is_hash(h)) {
                out.push((hash.to_owned(), e.metadata().map_or(0, |m| m.len())));
            } else if name.strip_suffix(SET_ASIDE).is_some_and(is_hash) {
                aside = aside.saturating_add(1);
            }
        }
        out.sort();
        (out, aside)
    }

    /// Bytes of the indexes, all of them but `but`'s.
    fn bytes_but(&self, but: Option<&str>) -> u64 {
        self.indexes().0.iter().filter(|(h, _)| Some(h.as_str()) != but).fold(0u64, |n, (_, b)| n.saturating_add(*b))
    }

    /// Walk `dirs` and index every book there whose index is not current (see the module
    /// documentation), at most `opts.max_books` of them; `progress` is told of each book and each
    /// page scanned, and returning `false` stops between two pages. The catalog is written after
    /// every book, so a stop or a crash keeps what was done. Fails only when the Library's folder
    /// can't be written.
    pub fn build(&self, dirs: &[PathBuf], opts: &BuildOptions, progress: &mut dyn FnMut(&Progress<'_>) -> bool) -> Result<Built, String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| format!("the Library's folder can't be made: {e}"))?;
        let walked = walk::walk(dirs, opts.caps, &|| false);
        let before = self.catalog();
        let models = super::models(opts.models_dir.as_deref());
        let settings = ChessScanSettings {
            models_dir: opts.models_dir.clone(),
            cache: Some(ReadingCache { dir: self.pages_dir(), encrypted: false }),
            ..ChessScanSettings::default()
        }
        .sanitized();
        let general = super::text_ocr::general_ocr_digest();
        let shown = |p: &Path| p.to_string_lossy().into_owned();
        let mut catalog = Catalog {
            dirs: dirs.iter().map(|d| shown(d)).collect(),
            walk: WalkNote {
                entries: walked.entries,
                capped: walked.capped.clone(),
                links: walked.links.iter().map(|p| shown(p)).collect(),
                cloud_folders: Vec::new(),
                unlisted: walked.unlisted.iter().map(|(p, e)| (shown(p), e.clone())).collect(),
            },
            ..Catalog::default()
        };
        let mut reports: Vec<BookReport> = Vec::new();
        let mut by_hash: HashMap<String, usize> = HashMap::new();
        let (mut read, mut stopped) = (0usize, false);
        let books = walked.pdfs.len();
        for (n, pdf) in walked.pdfs.iter().enumerate() {
            let path = shown(&pdf.path);
            let room = opts.max_books.is_none_or(|max| read < max);
            if !stopped && room && !progress(&Progress { book: n, books, path: &pdf.path, page: 0, pages: 0 }) {
                stopped = true;
            }
            // Past a stop or `max_books`: as the catalog had it, when nothing changed, else pending.
            if stopped || !room {
                reports.push(BookReport { book: carried(&before, pdf, &path, self), run: None });
                continue;
            }
            let started = std::time::Instant::now();
            let bytes = match super::corrections::read_capped(&pdf.path, MAX_BOOK_BYTES) {
                Ok(b) if b.len() > MAX_BOOK_BYTES => {
                    reports.push(BookReport { book: skipped(pdf, &path, Skip::TooLarge, None), run: Some(BookRun::default()) });
                    continue;
                }
                Ok(b) => b,
                Err(e) => {
                    reports.push(BookReport { book: skipped(pdf, &path, Skip::Unreadable, Some(e.to_string())), run: Some(BookRun::default()) });
                    continue;
                }
            };
            let content: [u8; 32] = sha2::Sha256::digest(&bytes).into();
            let hash = hex(&content);
            if let Some(&at) = by_hash.get(&hash) {
                if let Some(r) = reports.get_mut(at) {
                    r.book.paths.push(path);
                }
                continue;
            }
            let book = Book { pdf, path: &path, hash: &hash, content: &content, n, books };
            let wants = Wants { settings: &settings, models: &models, general: general.as_ref(), opts };
            let (entry, run, outcome) = self.book(&book, bytes, &wants, progress);
            if outcome == Outcome::Read || outcome == Outcome::Stopped {
                read = read.saturating_add(1);
            }
            let mut run = run;
            run.seconds = started.elapsed().as_secs_f64();
            by_hash.insert(hash, reports.len());
            reports.push(BookReport { book: entry, run: Some(run) });
            if outcome == Outcome::Stopped {
                stopped = true;
            }
            catalog.books = reports.iter().map(|r| r.book.clone()).collect();
            // Kept after every book: a stop, or a crash, keeps what was done.
            if let Err(e) = self.write_catalog(&mut catalog) {
                return Err(format!("the Library's catalog can't be written: {e}"));
            }
        }
        for c in &walked.cloud {
            let is_pdf = c.extension().is_some_and(|x| x.eq_ignore_ascii_case("pdf"));
            if is_pdf {
                let book = CatalogBook { paths: vec![shown(c)], state: State::Cloud, ..CatalogBook::default() };
                reports.push(BookReport { book, run: None });
            } else {
                catalog.walk.cloud_folders.push(shown(c));
            }
        }
        catalog.books = reports.iter().map(|r| r.book.clone()).collect();
        self.write_catalog(&mut catalog).map_err(|e| format!("the Library's catalog can't be written: {e}"))?;
        let left = reports.iter().filter(|r| matches!(r.book.state, State::Pending | State::Partial)).count();
        let cloud = reports.iter().filter(|r| r.book.state == State::Cloud).count();
        Ok(Built { books: reports, walk: catalog.walk, cloud, read, stopped, left, bytes: self.bytes_but(None) })
    }

    /// Read book `book` (its bytes `bytes`) and index it, unless its index is current: what the
    /// catalog says of it, what this call did, and whether it was read.
    fn book(
        &self,
        book: &Book<'_>,
        bytes: Vec<u8>,
        wants: &Wants<'_>,
        progress: &mut dyn FnMut(&Progress<'_>) -> bool,
    ) -> (CatalogBook, BookRun, Outcome) {
        let mut run = BookRun::default();
        let mut entry = CatalogBook {
            hash: Some(book.hash.to_owned()),
            paths: vec![book.path.to_owned()],
            size: book.pdf.size,
            modified: book.pdf.modified,
            ..CatalogBook::default()
        };
        let skip = |mut entry: CatalogBook, why: Skip, error: Option<String>| {
            entry.state = State::Skipped;
            entry.reason = Some(why);
            entry.error = error;
            entry
        };
        let mut session = Session::new();
        let name = book.pdf.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let id = match session.open(name.clone(), Some(book.path.to_owned()), Arc::new(bytes), None) {
            Ok(id) => id,
            Err(e) => return (skip(entry, Skip::DoesNotOpen, Some(e.to_string())), run, Outcome::Skipped),
        };
        let Some(doc) = session.get(id) else { return (skip(entry, Skip::DoesNotOpen, None), run, Outcome::Skipped) };
        let pages = doc.info.pages.len();
        entry.pages = Some(u32::try_from(pages).unwrap_or(u32::MAX));
        let stem = book.pdf.path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or(name);
        let title = doc.info.title.clone().map(|t| t.trim().to_owned()).filter(|t| !t.is_empty()).unwrap_or(stem);
        entry.title = Some(title.clone());
        let labels: Vec<String> = doc.info.pages.iter().map(|p| p.label.clone()).collect();
        let keys = doc.doc_keys();
        match super::store::persist_policy(doc) {
            super::store::Persist::Never => return (skip(entry, Skip::Never, None), run, Outcome::Skipped),
            super::store::Persist::Ask if !wants.opts.allowed.iter().any(|h| h.eq_ignore_ascii_case(book.hash)) => {
                return (skip(entry, Skip::Ask, None), run, Outcome::Skipped);
            }
            super::store::Persist::Ask => session.set_chess_keep(id, true),
            super::store::Persist::Yes => {}
        }
        // The book's corrections and its PGN, as the Text tab reads it with them; only read.
        let mut corrections = None;
        if let Some(dir) = &wants.opts.corrections
            && let Some(prints) = session.chess_fingerprints(id)
            && let Ok((loaded, _)) = super::corrections::DirStore::new(dir).load_with(&keys, prints.pages())
            && loaded.file.live().next().is_some()
        {
            corrections = loaded.file.to_bytes().ok().map(|b| <[u8; 32]>::from(sha2::Sha256::digest(&b)));
            session.set_chess_corrections(id, Some(loaded.file));
        }
        let pgn = wants.opts.reference.as_ref().and_then(|dir| {
            let loaded = super::reference_store::DirStore::new(dir).load(&keys, pages).ok()?;
            let kept = loaded.pgn()?;
            let index = pdfcraft_chessvision::doc::reference::ReferenceIndex::from_pgn(&kept.text).ok()?;
            Some((kept.name.clone().unwrap_or_else(|| "the kept PGN".into()), kept.hash.clone(), index))
        });
        let digests = ModelDigests {
            pieces: wants.models.pieces.loaded().map(|m| m.digest()),
            glyphs: wants.models.glyphs.loaded().map(|m| m.digest()),
            general: wants.general,
            corrections: corrections.as_ref(),
            pgn: pgn.as_ref().map(|p| p.1.as_str()),
        };
        let key = index_key(book.content, wants.settings, &digests);
        let lib = self.index_path(book.hash);
        entry.no_piece_model = wants.models.pieces.loaded().is_none();
        match index::read(&lib, Some(&key), &mut |_| {}) {
            Ok(meta) => {
                run.current = true;
                entry.state = State::Indexed;
                entry.counts = Some(meta.counts);
                entry.index_bytes = std::fs::metadata(&lib).map_or(0, |m| m.len());
                return (entry, run, Outcome::Current);
            }
            Err(ReadError::Damaged(_)) => {
                // Never served: set aside, beside it, in place of an older one, and made again.
                let _ = std::fs::rename(&lib, self.dir.join(format!("{}{SET_ASIDE}", book.hash)));
                run.index_set_aside = true;
            }
            Err(ReadError::Missing | ReadError::Stale) => {}
        }
        session.set_chess_peek(wants.opts.app_reading.as_deref());
        session.set_chess_disk_budget(Some(MAX_PAGE_BYTES));
        session.set_chess_scan_workers((wants.opts.workers > 0).then_some(wants.opts.workers));
        let (n, books, path) = (book.n, book.books, &book.pdf.path);
        let scanned =
            session.chess_scan_with(id, &[], wants.settings, true, &mut |page, pages| progress(&Progress { book: n, books, path, page, pages }));
        let tally = |session: &Session, run: &mut BookRun| {
            let disk = session.chess_disk();
            run.scanned = session.chess_scanned();
            run.from_library = disk.read;
            run.from_app = disk.peeked;
            run.kept = disk.kept;
            run.set_aside = disk.set_aside;
        };
        match scanned {
            Ok((_, true)) => {}
            Ok((_, false)) => {
                tally(&session, &mut run);
                entry.state = State::Partial;
                return (entry, run, Outcome::Stopped);
            }
            Err(e) => {
                tally(&session, &mut run);
                return (skip(entry, Skip::Failed, Some(e)), run, Outcome::Read);
            }
        }
        let read = session.chess_document_with(id, &[], wants.settings, None, &OcrOptions::new(OcrMode::Auto));
        tally(&session, &mut run);
        let mut read = match read {
            Ok(r) => r,
            Err(e) => return (skip(entry, Skip::Failed, Some(e)), run, Outcome::Read),
        };
        if let Some((_, _, index)) = &pgn {
            pdfcraft_chessvision::doc::reference::link(&mut read.doc, index);
        }
        let labels = pdfcraft_chessvision::doc::PageLabel::with_folios(&labels, &read.doc);
        let mut built =
            index::index_of(&read.doc, pgn.as_ref().map(|(name, _, index)| (name.as_str(), index)), &title, entry.pages.unwrap_or(0), labels);
        built.meta.counts.needs_ocr = u32::try_from(read.needs_ocr.len()).unwrap_or(u32::MAX);
        entry.counts = Some(built.meta.counts);
        let Some(file) = index::encode(&built, &key) else {
            return (skip(entry, Skip::Failed, Some("its index is larger than an index may be".into())), run, Outcome::Read);
        };
        let size = u64::try_from(file.len()).unwrap_or(u64::MAX);
        if self.bytes_but(Some(book.hash)).saturating_add(size) > MAX_BYTES {
            entry.state = State::Full;
            return (entry, run, Outcome::Read);
        }
        if let Err(e) = reading::write_whole(&lib, &file) {
            return (skip(entry, Skip::Failed, Some(format!("its index can't be written: {e}"))), run, Outcome::Read);
        }
        entry.state = State::Indexed;
        entry.index_bytes = size;
        // Its pages in progress are done with.
        reading::store_within(&self.pages_dir(), MAX_PAGE_BYTES).clear(Some(&keys));
        (entry, run, Outcome::Read)
    }

    /// Take out the indexes (only `book`'s: its hash), the catalog with all of them, and the pages
    /// in progress; only the names the Library makes, and its folders once nothing else is in them.
    pub fn clear(&self, book: Option<&str>) -> Cleared {
        let mut cleared = Cleared::default();
        let mut remove = |path: &Path, index: bool| {
            let bytes = std::fs::metadata(path).map_or(0, |m| m.len());
            if std::fs::remove_file(path).is_ok() {
                cleared.files = cleared.files.saturating_add(1);
                cleared.bytes = cleared.bytes.saturating_add(bytes);
                if index {
                    cleared.indexes = cleared.indexes.saturating_add(1);
                }
            }
        };
        if let Some(hash) = book.filter(|h| is_hash(h)) {
            let hash = hash.to_ascii_lowercase();
            remove(&self.index_path(&hash), true);
            remove(&self.dir.join(format!("{hash}{SET_ASIDE}")), false);
            let mut catalog = self.catalog();
            if let Some(b) = catalog.books.iter_mut().find(|b| b.hash.as_deref() == Some(hash.as_str())) {
                b.state = State::Pending;
                b.counts = None;
                b.index_bytes = 0;
                let _ = self.write_catalog(&mut catalog);
            }
            return cleared;
        }
        if book.is_some() {
            return cleared;
        }
        if let Ok(list) = std::fs::read_dir(&self.dir) {
            for e in list.flatten().take(walk::MAX_BOOKS.saturating_mul(8)) {
                let Some(name) = e.file_name().to_str().map(str::to_owned) else { continue };
                let index = name.strip_suffix(LIB).is_some_and(is_hash);
                if index || own_file(&name) {
                    remove(&e.path(), index);
                }
            }
        }
        let pages = reading::store_within(&self.pages_dir(), MAX_PAGE_BYTES).clear(None);
        cleared.files = cleared.files.saturating_add(pages.files);
        cleared.bytes = cleared.bytes.saturating_add(pages.bytes);
        let _ = std::fs::remove_dir(self.pages_dir());
        let _ = std::fs::remove_dir(&self.dir);
        cleared
    }
}

/// Whether `name` is a file the Library makes besides an index: its catalog, an index set aside,
/// or the staging file of either.
fn own_file(name: &str) -> bool {
    let staging = |rest: &str| rest.strip_prefix(".tmp").is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit() || b == b'-'));
    if name == CATALOG || name.strip_prefix(CATALOG).is_some_and(staging) {
        return true;
    }
    let Some((hash, rest)) = name.split_at_checked(64) else { return false };
    is_hash(hash) && (rest == SET_ASIDE || rest.strip_prefix(LIB).is_some_and(staging))
}

/// A book as a build met it.
struct Book<'a> {
    pdf: &'a walk::Pdf,
    path: &'a str,
    hash: &'a str,
    content: &'a [u8; 32],
    n: usize,
    books: usize,
}

/// What a build reads books with.
struct Wants<'a> {
    settings: &'a ChessScanSettings,
    models: &'a ChessModels,
    general: Option<&'a [u8; 32]>,
    opts: &'a BuildOptions,
}

/// What became of a book in a build.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    /// Its index was current.
    Current,
    /// Read (indexed, or failed on the way).
    Read,
    /// Stopped between two pages.
    Stopped,
    /// Not read: skipped.
    Skipped,
}

/// A book a build did not reach: as the catalog had it when its path, size and time are the
/// same and its index is still there, else pending.
fn carried(before: &Catalog, pdf: &walk::Pdf, path: &str, library: &Library) -> CatalogBook {
    let same = before.books.iter().find(|b| b.paths.iter().any(|p| p == path) && b.size == pdf.size && b.modified == pdf.modified);
    if let Some(b) = same {
        let indexed = b.state == State::Indexed && b.hash.as_deref().is_some_and(|h| library.index_path(h).is_file());
        if indexed || b.state == State::Skipped || b.state == State::Partial {
            return CatalogBook { paths: vec![path.to_owned()], ..b.clone() };
        }
    }
    CatalogBook { paths: vec![path.to_owned()], size: pdf.size, modified: pdf.modified, state: State::Pending, ..CatalogBook::default() }
}

/// A book not read, and why.
fn skipped(pdf: &walk::Pdf, path: &str, why: Skip, error: Option<String>) -> CatalogBook {
    CatalogBook {
        paths: vec![path.to_owned()],
        size: pdf.size,
        modified: pdf.modified,
        state: State::Skipped,
        reason: Some(why),
        error,
        ..CatalogBook::default()
    }
}
