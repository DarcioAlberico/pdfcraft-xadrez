//! A book's reading kept on disk (plan A9-N5a, decision D21; findings A8-06, R3-10): reopening
//! a book reads none of its pages again, where the session's cache (G11) lasts only as long as
//! the session.
//!
//! **What is kept** is each page's reading at its local stage, the part that depends on the page
//! alone: what a scan found on it ([`super::ChessPage`]) and what chess OCR read of it before the
//! book's cipher and the legality repair ([`super::text_ocr::PageAlone`]). Those two depend on
//! the other pages, and run again over what is kept.
//!
//! **Keys.** An entry's key ([`EntryKey`]) is a SHA-256 over the page's content as the session's
//! key hashes it (its content streams and images as stored, the attributes it inherits, the
//! catalog entries that change how it is drawn: [`super::page_key`] with [`super::page_key::Sha`]),
//! the reading's versions ([`VERSION`], [`SCAN_VERSION`], `doc::READER_VERSION`), the settings and
//! the models it was made with (by their digests, never their folders: plan A9-G6). The session's
//! key is never written: its value changes with the build of Rust. An entry holds its own key, and
//! one whose key is not the key asked for is a miss, never served.
//!
//! **Where.** `reading` in the app's chess folder, beside the other stores: a folder per document
//! (named as the stores name a document's file, `<hash>`), with `book.json` (the document's
//! record, whose time is the book's last use) and an entry per page reading, `<key>.page`. An
//! entry is written whole through a staging file renamed over it, so a reader sees the old entry
//! or the new one; never synced, since a torn entry is only a miss.
//!
//! **An entry** is the magic line, [`VERSION`], its key, the SHA-256 and the length of its JSON,
//! then the JSON deflated. It is untrusted input like any file: read within [`MAX_ENTRY_BYTES`],
//! inflated within [`MAX_JSON_BYTES`], checked against its key and its digest. One that fails is
//! set aside (`<key>.page.bad`, replacing an older one) and said ([`Found::SetAside`]), and the
//! page is read again.
//!
//! **How much** ([`MAX_BYTES`], D21: 512 MiB): past it the books used longest ago leave first,
//! whole, never the book being written; a book that alone passes it keeps what it has. Nothing is
//! kept while the disk has less than [`MIN_FREE_BYTES`] free. Only the names the store makes are
//! ever removed, so a folder given by the user keeps everything else; a removal another process
//! made first, or a file it added meanwhile, is taken as it is.

use std::collections::{HashMap, HashSet};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use serde::Serialize;
use serde::de::DeserializeOwned;

use super::corrections::DocRecord;
use super::keys::DocKeys;
use super::page_key::DiskKey;

/// The folder's name in the app's chess folder.
pub const KIND: &str = "reading";
/// Most bytes kept in all (decision D21).
pub const MAX_BYTES: u64 = 512 << 20;
/// Least free space left on the disk: below it nothing more is kept (plan A9-N5).
pub const MIN_FREE_BYTES: u64 = 1 << 30;
/// The entry format's version.
pub const VERSION: u32 = 1;
/// The version of what a page's scan, and chess OCR of a page alone, give for the same page,
/// settings and models. Raised by a change to detection, to the reading of boards or to chess OCR
/// that changes what they find, so a reading an earlier build kept is not taken for this one's.
pub const SCAN_VERSION: u32 = 1;
/// Largest entry file read.
pub const MAX_ENTRY_BYTES: usize = 16 << 20;
/// Largest JSON an entry holds, inflated.
pub const MAX_JSON_BYTES: usize = 64 << 20;
/// Most document folders looked at.
pub const MAX_BOOKS: usize = 4096;
/// Most files looked at in one document's folder.
pub const MAX_FILES_PER_BOOK: usize = 65_536;

/// An entry starts with it.
const MAGIC: &[u8] = b"pdfcraft-chess-reading\n";
/// The magic line, the version, the key, the JSON's digest and its length.
const HEADER: usize = MAGIC.len() + 4 + 32 + 32 + 4;
/// A document's record in its folder.
const BOOK: &str = "book.json";
const BOOK_FORMAT: &str = "pdfcraft-chess-reading-book";
/// An entry's extension, and a set-aside one's.
const ENTRY: &str = ".page";
const SET_ASIDE: &str = ".page.bad";
/// A staging file older than this was left by a writer that stopped.
const STALE_STAGING: std::time::Duration = std::time::Duration::from_secs(3600);

/// The key of one page's reading on disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EntryKey([u8; 32]);

impl EntryKey {
    /// The key in lower-case hexadecimal: the entry's name before [`ENTRY`].
    pub fn hex(&self) -> String {
        hex(&self.0)
    }
}

/// Bytes in lower-case hexadecimal.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
        s.push(char::from_digit(u32::from(b >> 4), 16).unwrap_or('0'));
        s.push(char::from_digit(u32::from(b & 15), 16).unwrap_or('0'));
        s
    })
}

/// What a key is made of, fed to SHA-256 with lengths and tags, so no two inputs share one.
struct KeyInput(sha2::Sha256);

impl KeyInput {
    fn new(kind: &[u8], content: &DiskKey) -> KeyInput {
        use sha2::Digest as _;
        let mut k = KeyInput(sha2::Sha256::new());
        k.bytes(b"pdfcraft-chess-reading-key");
        k.bytes(kind);
        for v in [VERSION, SCAN_VERSION, pdfcraft_chessvision::doc::READER_VERSION] {
            k.0.update(v.to_le_bytes());
        }
        k.0.update(content);
        k
    }

    fn bytes(&mut self, b: &[u8]) {
        sha2::Digest::update(&mut self.0, u64::try_from(b.len()).unwrap_or(u64::MAX).to_le_bytes());
        sha2::Digest::update(&mut self.0, b);
    }

    fn u64(&mut self, v: u64) {
        sha2::Digest::update(&mut self.0, v.to_le_bytes());
    }

    fn f32(&mut self, v: f32) {
        self.u64(u64::from(v.to_bits()));
    }

    fn finish(self) -> EntryKey {
        EntryKey(sha2::Digest::finalize(self.0).into())
    }
}

/// The key of the scan of a page whose content is `content` ([`crate::Session::chess_disk_keys`]),
/// made with `settings` and, reading, the piece model of `models` (`None`: diagrams only found,
/// not read). `None` for a piece model loaded from no folder (a caller's, or installed from
/// files), whose readings the session's cache does not keep either.
pub fn scan_key(content: &[u8; 32], settings: &super::ChessScanSettings, models: Option<&super::ChessModels>) -> Option<EntryKey> {
    scan_key_of(content, &settings.sanitized(), models.map(super::ModelKey::of).as_ref())
}

/// [`scan_key`] with the piece model's key; `settings` sanitized.
pub(super) fn scan_key_of(content: &DiskKey, settings: &super::ChessScanSettings, model: Option<&super::ModelKey>) -> Option<EntryKey> {
    let mut k = KeyInput::new(b"scan", content);
    let s = settings;
    k.f32(s.dpi);
    k.u64(match s.order {
        super::ReadingOrder::Column => 0,
        super::ReadingOrder::Row => 1,
    });
    k.u64(u64::try_from(s.max_diagrams).unwrap_or(u64::MAX));
    k.u64(u64::from(s.allow_unverified_fonts));
    match model {
        None => k.u64(0),
        Some(super::ModelKey::Missing) => {
            k.u64(1);
            k.f32(s.max_read_dpi);
        }
        Some(super::ModelKey::Folder(_, digest)) => {
            k.u64(2);
            k.f32(s.max_read_dpi);
            k.bytes(digest);
        }
        Some(super::ModelKey::Supplied) => return None,
    }
    Some(k.finish())
}

/// The key of chess OCR's reading of a page alone ([`super::text_ocr::PageAlone`]): its content,
/// the resolution it is read at, the boxes painted out (the page's diagrams, in view space), the
/// glyph model's digest and the general OCR's (`None`: it did not run).
pub fn alone_key(
    content: &[u8; 32],
    dpi: f32,
    diagrams: &[pdfcraft_chessvision::coords::ViewRect],
    glyphs: &[u8; 32],
    general: Option<&[u8; 32]>,
) -> EntryKey {
    let mut k = KeyInput::new(b"alone", content);
    k.f32(dpi);
    k.u64(u64::try_from(diagrams.len()).unwrap_or(u64::MAX));
    for r in diagrams {
        for v in [r.x0, r.y0, r.x1, r.y1] {
            k.f32(v);
        }
    }
    k.bytes(glyphs);
    match general {
        None => k.u64(0),
        Some(d) => {
            k.u64(1);
            k.bytes(d);
        }
    }
    k.finish()
}

/// The name of a document's folder: the hash the stores name its file by. `None` when no key is
/// known.
pub fn book_folder(keys: &DocKeys) -> Option<String> {
    super::store::legacy_file_name(keys)?.strip_suffix(".json").map(str::to_owned)
}

/// Whether `name` is a document's folder: 32 hexadecimal digits.
fn is_book_folder(name: &str) -> bool {
    name.len() == 32 && name.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Whether `name` is a file the store makes in a document's folder: its record, an entry
/// (`<64 hex>.page`), one set aside, or the staging file of one of them.
fn is_own_file(name: &str) -> bool {
    own_kind(name).is_some()
}

/// What file of the store's `name` is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Own {
    Record,
    Entry,
    SetAside,
    Staging,
}

fn own_kind(name: &str) -> Option<Own> {
    let staging = |rest: &str| rest.strip_prefix(".tmp").is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit() || b == b'-'));
    if name == BOOK {
        return Some(Own::Record);
    }
    if name.strip_prefix(BOOK).is_some_and(staging) {
        return Some(Own::Staging);
    }
    let (key, rest) = name.split_at_checked(64)?;
    if !key.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    match rest {
        ENTRY => Some(Own::Entry),
        SET_ASIDE => Some(Own::SetAside),
        _ => rest.strip_prefix(ENTRY).is_some_and(staging).then_some(Own::Staging),
    }
}

/// What looking up an entry found.
#[derive(Clone, Debug, PartialEq)]
pub enum Found<T> {
    /// The page's reading, as kept.
    Hit(T),
    /// Nothing kept for that key.
    Miss,
    /// An entry was there that could not be trusted, and why: it was set aside, and the page is
    /// to be read again.
    SetAside(String),
}

/// What keeping an entry did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kept {
    /// Kept.
    Yes,
    /// Not kept: the document has no key to keep anything under.
    NoKeys,
    /// Not kept: the book alone would pass [`ReadingStore::max_bytes`].
    Full,
    /// Not kept: the disk has less than [`MIN_FREE_BYTES`] free.
    LowSpace,
    /// Not kept: it would not read back as it is (a number JSON can't hold), or it is larger than
    /// an entry may be.
    Unwritable,
}

/// A probe of the free space on the disk holding a folder (the frontend's: the engine does not
/// ask the system).
pub type FreeSpace = Arc<dyn Fn(&Path) -> Option<u64> + Send + Sync>;

/// One document's folder as the store last counted it.
#[derive(Clone, Copy, Debug)]
struct Book {
    bytes: u64,
    used: SystemTime,
}

#[derive(Default)]
struct State {
    /// The documents' folders, counted once and kept up to date by this store's writes; `None`
    /// until first needed.
    books: Option<HashMap<String, Book>>,
    /// The books whose use this session has recorded already.
    touched: HashSet<String>,
}

/// The books' readings kept in a folder.
pub struct ReadingStore {
    dir: PathBuf,
    max_bytes: u64,
    min_free: u64,
    free_space: Option<FreeSpace>,
    state: Mutex<State>,
}

impl std::fmt::Debug for ReadingStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReadingStore").field("dir", &self.dir).field("max_bytes", &self.max_bytes).finish()
    }
}

/// One document's folder, for [`ReadingStore::status`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BookStatus {
    /// The folder's name.
    pub folder: String,
    /// The document as its record says (no path: a hash of it).
    pub doc: Option<DocRecord>,
    /// Page readings kept, and entries set aside.
    pub entries: usize,
    pub set_aside: usize,
    pub bytes: u64,
    /// Its last use.
    pub used: SystemTime,
}

/// What [`ReadingStore::clear`] removed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cleared {
    pub books: usize,
    pub files: usize,
    pub bytes: u64,
}

impl ReadingStore {
    /// The store in the folder `dir` (made when something is first kept), within [`MAX_BYTES`],
    /// without a probe of the disk's free space.
    pub fn new(dir: impl Into<PathBuf>) -> ReadingStore {
        ReadingStore { dir: dir.into(), max_bytes: MAX_BYTES, min_free: MIN_FREE_BYTES, free_space: None, state: Mutex::default() }
    }

    /// Keeping at most `max` bytes in all.
    pub fn with_max_bytes(mut self, max: u64) -> ReadingStore {
        self.max_bytes = max;
        self
    }

    /// Keeping nothing while `probe` says the disk has less than `min_free` bytes free.
    pub fn with_free_space(mut self, probe: FreeSpace, min_free: u64) -> ReadingStore {
        self.free_space = Some(probe);
        self.min_free = min_free;
        self
    }

    /// The folder.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Most bytes it keeps.
    pub fn max_bytes(&self) -> u64 {
        self.max_bytes
    }

    /// The state. A panic elsewhere while it was held leaves counts at worst stale, which the
    /// next count past the budget corrects.
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The reading kept under `key` for the document `doc`.
    pub fn get<T: DeserializeOwned>(&self, doc: &DocKeys, key: &EntryKey) -> Found<T> {
        let Some(folder) = book_folder(doc) else { return Found::Miss };
        let path = self.dir.join(&folder).join(format!("{}{ENTRY}", key.hex()));
        let bytes = match super::corrections::read_capped(&path, MAX_ENTRY_BYTES) {
            Ok(b) => b,
            // Not there, or not readable now (another process renaming it): a miss.
            Err(_) => return Found::Miss,
        };
        let found = decode(&bytes, key).and_then(|json| serde_json::from_slice::<T>(&json).map_err(|e| format!("its reading can't be read: {e}")));
        match found {
            Ok(value) => {
                self.touch(&folder);
                Found::Hit(value)
            }
            Err(why) => {
                set_aside(&path);
                Found::SetAside(why)
            }
        }
    }

    /// Keep `value` under `key` for the document `doc` of `pages` pages, making room by taking out
    /// the books used longest ago. Fails only when the folder can't be written.
    pub fn put<T: Serialize + DeserializeOwned>(&self, doc: &DocKeys, pages: usize, key: &EntryKey, value: &T) -> std::io::Result<Kept> {
        let Some(folder) = book_folder(doc) else { return Ok(Kept::NoKeys) };
        let Some(bytes) = encode(value, key) else { return Ok(Kept::Unwritable) };
        let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if let Some(probe) = &self.free_space {
            // A disk that can't say is taken as having room.
            if probe(&self.dir).is_some_and(|free| free < self.min_free.saturating_add(size)) {
                return Ok(Kept::LowSpace);
            }
        }
        let book_dir = self.dir.join(&folder);
        let path = book_dir.join(format!("{}{ENTRY}", key.hex()));
        let mut state = self.state();
        let books = self.books(&mut state);
        let before = std::fs::metadata(&path).map_or(0, |m| m.len());
        let total = books.values().fold(0u64, |sum, b| sum.saturating_add(b.bytes));
        if total.saturating_sub(before).saturating_add(size) > self.max_bytes {
            self.make_room(books, &folder, size.saturating_sub(before));
            let total = books.values().fold(0u64, |sum, b| sum.saturating_add(b.bytes));
            if total.saturating_sub(before).saturating_add(size) > self.max_bytes {
                return Ok(Kept::Full);
            }
        }
        std::fs::create_dir_all(&book_dir)?;
        let record = book_dir.join(BOOK);
        if !record.exists() {
            let json = serde_json::json!({ "format": BOOK_FORMAT, "version": 1, "doc": DocRecord::new(doc, pages) });
            write_whole(&record, json.to_string().as_bytes())?;
        }
        write_whole(&path, &bytes)?;
        let now = SystemTime::now();
        let book = books.entry(folder.clone()).or_insert(Book { bytes: 0, used: now });
        book.bytes = book.bytes.saturating_sub(before).saturating_add(size);
        book.used = now;
        drop(state);
        self.touch(&folder);
        Ok(Kept::Yes)
    }

    /// The books' folders, counted the first time.
    fn books<'a>(&self, state: &'a mut State) -> &'a mut HashMap<String, Book> {
        state.books.get_or_insert_with(|| self.count())
    }

    /// Every document's folder with what it holds, counted on disk.
    fn count(&self) -> HashMap<String, Book> {
        let mut books = HashMap::new();
        let Ok(entries) = std::fs::read_dir(&self.dir) else { return books };
        for e in entries.flatten().take(MAX_BOOKS) {
            let Some(name) = e.file_name().to_str().map(str::to_owned) else { continue };
            if !is_book_folder(&name) || !e.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let (bytes, _, _) = folder_size(&e.path(), true);
            books.insert(name, Book { bytes, used: last_use(&e.path()) });
        }
        books
    }

    /// Take out whole books, the ones used longest ago first and never `keep`, until `more`
    /// bytes fit with a tenth of the budget to spare (so the books are not counted again on every
    /// page that follows). The books are counted again on disk first: another process may have
    /// written or taken out some since.
    fn make_room(&self, books: &mut HashMap<String, Book>, keep: &str, more: u64) {
        *books = self.count();
        let room = self.max_bytes.saturating_sub(self.max_bytes / 10);
        let mut order: Vec<(SystemTime, String)> = books.iter().filter(|(n, _)| n.as_str() != keep).map(|(n, b)| (b.used, n.clone())).collect();
        order.sort();
        for (_, name) in order {
            let total = books.values().fold(0u64, |sum, b| sum.saturating_add(b.bytes));
            if total.saturating_add(more) <= room {
                break;
            }
            remove_book(&self.dir.join(&name));
            // What is left of it (a file another process added meanwhile) still counts.
            let (left, _, _) = folder_size(&self.dir.join(&name), false);
            if left == 0 {
                books.remove(&name);
            } else if let Some(b) = books.get_mut(&name) {
                b.bytes = left;
            }
        }
    }

    /// Record that the book in `folder` was used now, once a session: its record's time.
    fn touch(&self, folder: &str) {
        let mut state = self.state();
        if !state.touched.insert(folder.to_owned()) {
            return;
        }
        let now = SystemTime::now();
        let record = self.dir.join(folder).join(BOOK);
        if let Ok(f) = std::fs::OpenOptions::new().write(true).open(&record) {
            let _ = f.set_modified(now);
        }
        if let Some(book) = state.books.as_mut().and_then(|b| b.get_mut(folder)) {
            book.used = now;
        }
    }

    /// Every document's folder and what it holds.
    pub fn status(&self) -> Vec<BookStatus> {
        let mut out = Vec::new();
        let Ok(entries) = std::fs::read_dir(&self.dir) else { return out };
        for e in entries.flatten().take(MAX_BOOKS) {
            let Some(name) = e.file_name().to_str().map(str::to_owned) else { continue };
            if !is_book_folder(&name) || !e.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let path = e.path();
            let (bytes, entries, set_aside) = folder_size(&path, false);
            let doc = super::corrections::read_capped(&path.join(BOOK), 1 << 16)
                .ok()
                .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
                .filter(|v| v.get("format").and_then(|f| f.as_str()) == Some(BOOK_FORMAT))
                .and_then(|v| serde_json::from_value::<DocRecord>(v.get("doc")?.clone()).ok());
            out.push(BookStatus { folder: name, doc, entries, set_aside, bytes, used: last_use(&path) });
        }
        out.sort_by(|a, b| b.used.cmp(&a.used).then_with(|| a.folder.cmp(&b.folder)));
        out
    }

    /// Take out everything kept (`doc`: only that document's readings). Only the files the store
    /// made are removed, and a folder left empty.
    pub fn clear(&self, doc: Option<&DocKeys>) -> Cleared {
        let mut cleared = Cleared::default();
        let folders: Vec<String> = match doc {
            Some(keys) => book_folder(keys).into_iter().collect(),
            None => self.status().into_iter().map(|b| b.folder).collect(),
        };
        for name in folders {
            let path = self.dir.join(&name);
            if !path.is_dir() {
                continue;
            }
            let (bytes, _, _) = folder_size(&path, false);
            let removed = remove_book(&path);
            cleared.books = cleared.books.saturating_add(1);
            cleared.files = cleared.files.saturating_add(removed);
            cleared.bytes = cleared.bytes.saturating_add(bytes);
        }
        let mut state = self.state();
        state.books = None;
        state.touched.clear();
        cleared
    }
}

/// The bytes of the store's own files in a document's folder, how many page readings and how many
/// set aside. With `sweep`, the staging files a writer that stopped left, more than an hour old,
/// are removed and not counted.
fn folder_size(dir: &Path, sweep: bool) -> (u64, usize, usize) {
    let (mut bytes, mut entries, mut aside) = (0u64, 0usize, 0usize);
    let Ok(list) = std::fs::read_dir(dir) else { return (0, 0, 0) };
    for e in list.flatten().take(MAX_FILES_PER_BOOK) {
        let Some(kind) = e.file_name().to_str().and_then(own_kind) else { continue };
        let meta = e.metadata().ok();
        if kind == Own::Staging && sweep {
            let stale = meta.as_ref().and_then(|m| m.modified().ok()).and_then(|t| t.elapsed().ok()).is_some_and(|age| age > STALE_STAGING);
            if stale && std::fs::remove_file(e.path()).is_ok() {
                continue;
            }
        }
        bytes = bytes.saturating_add(meta.map_or(0, |m| m.len()));
        match kind {
            Own::Entry => entries = entries.saturating_add(1),
            Own::SetAside => aside = aside.saturating_add(1),
            Own::Record | Own::Staging => {}
        }
    }
    (bytes, entries, aside)
}

/// A book's last use: its record's time, else its folder's.
fn last_use(dir: &Path) -> SystemTime {
    std::fs::metadata(dir.join(BOOK)).or_else(|_| std::fs::metadata(dir)).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH)
}

/// Remove the store's own files in a document's folder, then the folder when nothing else is left
/// in it. A file already gone is taken as removed. How many it removed.
fn remove_book(dir: &Path) -> usize {
    let mut removed = 0usize;
    if let Ok(list) = std::fs::read_dir(dir) {
        for e in list.flatten().take(MAX_FILES_PER_BOOK) {
            let own = e.file_name().to_str().is_some_and(is_own_file);
            if own && std::fs::remove_file(e.path()).is_ok() {
                removed = removed.saturating_add(1);
            }
        }
    }
    let _ = std::fs::remove_dir(dir);
    removed
}

/// Set the entry `path` aside as `<name>.bad`, in place of an older one.
fn set_aside(path: &Path) {
    let mut bad = path.as_os_str().to_owned();
    bad.push(".bad");
    let _ = std::fs::rename(path, PathBuf::from(bad));
}

/// Write `bytes` over `path` whole, through a staging file beside it renamed over it. Not synced:
/// a torn entry after a crash is a miss.
fn write_whole(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut staging = path.as_os_str().to_owned();
    staging.push(format!(".tmp{}-{n}", pid()));
    let staging = PathBuf::from(staging);
    let result = std::fs::File::create(&staging).and_then(|mut f| f.write_all(bytes)).and_then(|()| std::fs::rename(&staging, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&staging);
    }
    result
}

/// This process's id, in staging files' names (none on the web, which has no files).
fn pid() -> u32 {
    #[cfg(not(target_arch = "wasm32"))]
    let id = std::process::id();
    #[cfg(target_arch = "wasm32")]
    let id = 0;
    id
}

/// `value` as an entry under `key`: `None` when it would not read back as it is, or is too large.
fn encode<T: Serialize + DeserializeOwned>(value: &T, key: &EntryKey) -> Option<Vec<u8>> {
    use sha2::Digest as _;
    let json = serde_json::to_vec(value).ok()?;
    // A value JSON can't hold (a NaN is written as `null`) would be read back as nothing, every
    // time: it is not kept.
    serde_json::from_slice::<T>(&json).ok()?;
    if json.len() > MAX_JSON_BYTES {
        return None;
    }
    let mut out = Vec::with_capacity(HEADER.saturating_add(json.len() / 4));
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&key.0);
    out.extend_from_slice(&sha2::Sha256::digest(&json));
    out.extend_from_slice(&u32::try_from(json.len()).ok()?.to_le_bytes());
    let mut z = flate2::write::DeflateEncoder::new(out, flate2::Compression::default());
    z.write_all(&json).ok()?;
    let out = z.finish().ok()?;
    (out.len() <= MAX_ENTRY_BYTES).then_some(out)
}

/// An entry's JSON, when it is an entry of this version under `key` whose JSON is what its digest
/// says; else why not.
fn decode(bytes: &[u8], key: &EntryKey) -> Result<Vec<u8>, String> {
    use sha2::Digest as _;
    if bytes.len() > MAX_ENTRY_BYTES {
        return Err(format!("larger than {} MB", MAX_ENTRY_BYTES >> 20));
    }
    let head = bytes.get(..HEADER).ok_or("too short to be a page's reading")?;
    let short = || "too short to be a page's reading".to_owned();
    let (magic, rest) = head.split_at_checked(MAGIC.len()).ok_or_else(short)?;
    if magic != MAGIC {
        return Err("not a page's reading".into());
    }
    let (version, rest) = rest.split_at_checked(4).ok_or_else(short)?;
    let (stored_key, rest) = rest.split_at_checked(32).ok_or_else(short)?;
    let (digest, len) = rest.split_at_checked(32).ok_or_else(short)?;
    let version = u32::from_le_bytes(version.try_into().map_err(|_| "no version")?);
    if version != VERSION {
        return Err(format!("version {version}, not {VERSION}"));
    }
    if stored_key != key.0 {
        return Err("kept for another key".into());
    }
    let len = usize::try_from(u32::from_le_bytes(len.try_into().map_err(|_| "no length")?)).map_err(|_| "too long")?;
    if len > MAX_JSON_BYTES {
        return Err(format!("larger than {} MB inflated", MAX_JSON_BYTES >> 20));
    }
    // Grown as it inflates: the length said is not trusted for an allocation.
    let mut json = Vec::new();
    let body = bytes.get(HEADER..).unwrap_or_default();
    // One byte more than said: a longer stream is told by its length.
    flate2::read::DeflateDecoder::new(body)
        .take(u64::try_from(len).unwrap_or(u64::MAX).saturating_add(1))
        .read_to_end(&mut json)
        .map_err(|e| format!("its data is damaged: {e}"))?;
    if json.len() != len || sha2::Sha256::digest(&json).as_slice() != digest {
        return Err("its data is damaged".into());
    }
    Ok(json)
}

#[cfg(test)]
mod tests;
