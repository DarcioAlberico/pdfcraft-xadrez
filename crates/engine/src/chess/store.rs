//! One store per document and per kind of data (plan A9 §2.3, step G3a): **what** is kept
//! ([`StoreKind`]: the diagram corrections now; the book's PGN, the games and the book's cipher
//! next) apart from **where** ([`StoreBackend`]: a folder on the desktop and the command line,
//! [`DirBackend`]; memory the web build saves in the browser's storage, [`MemBackend`]). One
//! [`DocStore`] holds the rules every kind shares, so they are written once:
//!
//! - **Names.** A document's file is `<hash>.<kind>.json` ([`file_name`]), so two kinds kept in
//!   one folder never share a file (finding A4-07). The name earlier builds gave every kind,
//!   `<hash>.json` ([`legacy_file_name`]), is still read when it holds this kind's format, and its
//!   entries are merged in by the kind's rule ([`StoreKind::merge`]); it is never written, moved
//!   or deleted, so a build from before keeps working on it, and what it writes there is seen here
//!   at the next load. A file of another format is never this kind's, and never set aside by it.
//! - **Reading only reads** ([`DocStore::load`]): a file that can't be read is reported
//!   ([`Loaded::unreadable`]), not renamed; an entry that can't be read is left out and counted
//!   ([`Loaded::skipped`]), the rest kept (A4-06). Only a change ([`DocStore::update`]), under the
//!   store's lock, sets a file aside (`<name>.bad`): one that can't be read, or one holding entries
//!   the change would drop. It says so ([`Loaded::set_aside`]); nothing is deleted.
//! - **Versions.** A file of a newer version is read for what this build can make out of it, and
//!   never written ([`Loaded::read_only`]).
//! - **Finding a document's file.** Its own name, else the first `.json` file by name (of at most
//!   [`MAX_FILES`]) named as this kind's or as every kind's before, whose header is this kind's
//!   and records the document; only the header is parsed.
//! - **One writer at a time** ([`DirBackend::lock`], plan A9-G3a): a change holds `<kind>.flock`
//!   by the system's file lock, which a process that stops lets go of, so no lock is ever taken
//!   for "stale" and no writer removes another's (A4-16). For a kind that builds before it can
//!   write ([`StoreKind::OLD_LOCK`]) it also takes their lock, `<kind>.lock`, inside its own; a
//!   folder whose file system has no locks (some network shares) falls back to that one alone
//!   ([`Loaded::old_lock_only`]). Under the lock, the staging files of this kind a writer that
//!   stopped left behind, more than an hour old, are removed: only names the store makes
//!   (`<hash>.<kind>.json.tmp<n>`), never anything else in the folder.
//! - **One place, migrated into** (plan A9-G5a, decision D17): the app and the command line keep
//!   the stores in the app's data folder, and what earlier builds kept elsewhere (the recovery
//!   folder's `chess`, a `.pdfcraft` folder) is merged in by the kind's rule
//!   ([`DocStore::migrate_from`]), never copied over, moved or deleted. `migrated.json` in the
//!   folder records each source file's size and time as merged, so running it again merges only
//!   what changed since (an earlier build goes on writing where it did).
//! - **What may be kept** ([`persist_policy`]): nothing read from a document that does not allow
//!   copying, and from an encrypted one only with the user's consent, since the stores keep it
//!   unencrypted. Every writer asks before keeping (the stores do not see the document).

use std::marker::PhantomData;

use super::corrections::DocRecord;
use super::keys::DocKeys;

/// Most `.json` files looked at to find a document's file.
pub const MAX_FILES: usize = 1024;

/// What one kind of store keeps, and its rules.
pub trait StoreKind {
    /// The kind's name, in its files' names (`<hash>.<KIND>.json`) and its lock's.
    const KIND: &'static str;
    /// The `format` each of its files names.
    const FORMAT: &'static str;
    /// Largest file read or written.
    const MAX_FILE_BYTES: usize;
    /// Whether builds from before `<kind>.flock` (plan A9-G3a) can write this kind's files: then a
    /// change also takes their lock, `<kind>.lock`, inside its own (§2.10). Once the kind's format
    /// is one those builds only read, it is not taken.
    const OLD_LOCK: bool;
    /// A document's file.
    type File: Clone;
    /// What a change, and the store, can fail with.
    type Error: StoreFailure;

    /// An empty file for the document `doc`.
    fn empty(doc: DocRecord) -> Self::File;
    /// The document a file records.
    fn doc(file: &Self::File) -> &DocRecord;
    /// Record the document a file belongs to.
    fn set_doc(file: &mut Self::File, doc: DocRecord);
    /// A file's bytes: the file, with the entries that can't be read left out and counted; a newer
    /// version's; or why it can't be read.
    fn read(bytes: &[u8]) -> Parsed<Self::File>;
    fn to_bytes(file: &Self::File) -> Result<Vec<u8>, Self::Error>;
    /// Whether the file keeps anything (a document without anything has no file to move).
    fn holds_anything(file: &Self::File) -> bool;
    /// `theirs` taken into `mine` by the kind's rule, which never undoes what either did (a
    /// deletion stays deleted); a tie keeps `mine`.
    fn merge(mine: &mut Self::File, theirs: &Self::File);
}

/// The stores' own failures, which each kind's error says in its words.
pub trait StoreFailure: From<std::io::Error> {
    /// The document has no key to keep anything under.
    fn no_keys() -> Self;
    /// Another writer holds the store all the while.
    fn busy() -> Self;
    /// The file is of a newer version, which this build only reads.
    fn newer(version: u32) -> Self;
    /// The files would pass the memory store's budget.
    fn over_budget(budget: usize) -> Self;
}

/// What a file's bytes are, for its kind.
#[derive(Clone, Debug, PartialEq)]
pub enum Parsed<F> {
    /// A file of this version (an older one taken in as this one), and how many of its entries
    /// could not be read and were left out.
    File { file: F, skipped: usize },
    /// A file of a newer version: what this build can make out of it, for reading only.
    Newer { version: u32, file: Option<F> },
    /// Not a file of the kind this build can read, and why.
    Bad(String),
}

/// What [`DocStore::load`] found for a document.
#[derive(Clone, Debug, PartialEq)]
pub struct Loaded<F> {
    /// The file's name in the store: the one a change writes.
    pub name: String,
    /// The document's file, with what its earlier name holds merged in; empty when there is none.
    pub file: F,
    /// A file is of a newer version: shown, never written.
    pub read_only: bool,
    /// That newer version.
    pub newer: Option<u32>,
    /// Entries that could not be read and were left out.
    pub skipped: usize,
    /// A file of this kind that could not be read, and why; a load leaves it as it is.
    pub unreadable: Option<(String, String)>,
    /// A change set a file aside under this name, and why.
    pub set_aside: Option<(String, String)>,
    /// The folder has no file locks (some network shares): the change was held by the lock file
    /// of builds before plan A9-G3a alone, which a writer that stops can leave behind.
    pub old_lock_only: bool,
    /// The entries left out of the file named [`Loaded::name`] itself (not of the earlier name's,
    /// which is never written).
    own_skipped: usize,
}

/// What holds a store while it is changed.
pub trait Held {
    /// Whether only the lock of builds before plan A9-G3a holds it (the folder has no file locks).
    fn old_lock_only(&self) -> bool {
        false
    }
}

impl Held for std::sync::MutexGuard<'_, ()> {}

/// Where a store's files are.
pub trait StoreBackend {
    /// What holds the store while it is changed.
    type Guard<'a>: 'a + Held
    where
        Self: 'a;
    /// The names of the store's `.json` files, sorted, at most `max`.
    fn list(&self, max: usize) -> std::io::Result<Vec<String>>;
    /// The file `name`'s bytes (`None`: there is none), at most `max` and one more, so a larger
    /// file is told by its length.
    fn read(&self, name: &str, max: usize) -> std::io::Result<Option<Vec<u8>>>;
    /// Write the file `name` whole in one step: a reader sees the old bytes or the new ones.
    fn write<E: StoreFailure>(&self, name: &str, bytes: &[u8]) -> Result<(), E>;
    /// Rename `name` aside, to `<name>.bad` (`.bad2`, … when taken). The new name.
    fn set_aside<E: StoreFailure>(&self, name: &str) -> Result<String, E>;
    /// Hold the store of kind `kind` against other writers ([`StoreFailure::busy`] when another
    /// holds it all the while); with `old_lock`, against builds before plan A9-G3a too
    /// ([`StoreKind::OLD_LOCK`]).
    fn lock<E: StoreFailure>(&self, kind: &str, old_lock: bool) -> Result<Self::Guard<'_>, E>;
}

/// A document store of the kind `K` on the backend `B`.
pub struct DocStore<K: StoreKind, B: StoreBackend> {
    backend: B,
    kind: PhantomData<fn() -> K>,
}

/// The name of the document's file of kind `kind`: [`legacy_file_name`]'s hash, then the kind.
/// `None` when no key is known.
pub fn file_name(keys: &DocKeys, kind: &str) -> Option<String> {
    let legacy = legacy_file_name(keys)?;
    legacy.strip_suffix(".json").map(|hash| format!("{hash}.{kind}.json"))
}

/// The name every kind's file had before plan A9-G3a: `<hash>.json`.
pub fn legacy_file_name(keys: &DocKeys) -> Option<String> {
    super::corrections::file_name(keys)
}

/// The format and the document a file records, the rest skipped.
#[derive(serde::Deserialize)]
pub(super) struct Header {
    #[serde(default)]
    pub(super) format: Option<String>,
    #[serde(default)]
    pub(super) doc: Option<DocRecord>,
}

pub(super) fn header(bytes: &[u8]) -> Option<Header> {
    serde_json::from_slice(bytes).ok()
}

/// Whether `name` is named as a file of kind `K` (`<hash>.<kind>.json`) or as every kind's file
/// was before plan A9-G3a (`<hash>.json`, the hash 32 hexadecimal digits).
pub(super) fn named_for<K: StoreKind>(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".json") else { return false };
    let hash = stem.strip_suffix(K::KIND).and_then(|s| s.strip_suffix('.')).unwrap_or(stem);
    is_hash(hash)
}

/// 32 hexadecimal digits, as [`legacy_file_name`] names a document.
fn is_hash(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// What may be kept of a document in the stores, which keep it unencrypted (plan A9 §2.3): the
/// one rule every writer follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Persist {
    /// The document does not allow copying (Table 22, bit 5): nothing read from it is kept.
    Never,
    /// The document is encrypted: kept only once the user says so, for that document.
    Ask,
    /// Kept.
    Yes,
}

/// What may be kept of `doc` ([`Persist`]).
pub fn persist_policy(doc: &crate::Document) -> Persist {
    if !doc.allows_copying() {
        Persist::Never
    } else if doc.info.encrypted {
        Persist::Ask
    } else {
        Persist::Yes
    }
}

impl<K: StoreKind, B: StoreBackend> DocStore<K, B> {
    /// The store on `backend`.
    pub fn on(backend: B) -> Self {
        DocStore { backend, kind: PhantomData }
    }

    /// Where its files are.
    pub(super) fn backend(&self) -> &B {
        &self.backend
    }

    /// The document's file: by its own name, else the file of this kind recording the document
    /// (named for another kind of key, or by its earlier name), with what its earlier name holds
    /// merged in; an empty one named for it when there is none. Only reads.
    pub fn load(&self, keys: &DocKeys, pages: usize) -> Result<Loaded<K::File>, K::Error> {
        let own = file_name(keys, K::KIND).ok_or_else(K::Error::no_keys)?;
        let legacy = legacy_file_name(keys).ok_or_else(K::Error::no_keys)?;
        let ours = format!(".{}.json", K::KIND);
        let mut l = Loaded {
            name: own.clone(),
            file: K::empty(DocRecord::new(keys, pages)),
            read_only: false,
            newer: None,
            skipped: 0,
            unreadable: None,
            set_aside: None,
            old_lock_only: false,
            own_skipped: 0,
        };
        match self.backend.read(&own, K::MAX_FILE_BYTES)? {
            Some(bytes) => self.take_in(&mut l, &own, &bytes, true),
            None => match self.find(keys)? {
                // A file of this kind named for other keys (moved there by a save): it is the one.
                Some((found, bytes)) if found.ends_with(&ours) => {
                    l.name.clone_from(&found);
                    self.take_in(&mut l, &found, &bytes, true);
                }
                // A file of the name before G3a for other keys: read, never written.
                Some((found, bytes)) if found != legacy => self.take_in(&mut l, &found, &bytes, false),
                _ => {}
            },
        }
        if let Some(bytes) = self.backend.read(&legacy, K::MAX_FILE_BYTES)?
            && header(&bytes).is_some_and(|h| h.format.as_deref() == Some(K::FORMAT))
        {
            self.take_in(&mut l, &legacy, &bytes, false);
        }
        Ok(l)
    }

    /// Read `bytes`, the file `name`, into `l`: as its file when `own`, else merged in.
    fn take_in(&self, l: &mut Loaded<K::File>, name: &str, bytes: &[u8], own: bool) {
        let parsed =
            if bytes.len() > K::MAX_FILE_BYTES { Parsed::Bad(format!("larger than {} MB", K::MAX_FILE_BYTES >> 20)) } else { K::read(bytes) };
        let file = match parsed {
            Parsed::File { file, skipped } => {
                l.skipped = l.skipped.saturating_add(skipped);
                if own {
                    l.own_skipped = skipped;
                }
                Some(file)
            }
            Parsed::Newer { version, file } => {
                l.read_only = true;
                l.newer = Some(l.newer.map_or(version, |v| v.max(version)));
                file
            }
            Parsed::Bad(why) => {
                if l.unreadable.is_none() || own {
                    l.unreadable = Some((name.to_owned(), why));
                }
                None
            }
        };
        // The own file is read first, into the empty one; the others are merged into it.
        match (file, own) {
            (Some(file), true) => l.file = file,
            (Some(file), false) => K::merge(&mut l.file, &file),
            (None, _) => {}
        }
    }

    /// The first `.json` file by name whose header is this kind's and records the document named
    /// by `keys`, with its bytes. Only files named as this kind's (`<hash>.<kind>.json`) or as every
    /// kind's before (`<hash>.json`) are read: another kind's in the same folder never is.
    fn find(&self, keys: &DocKeys) -> Result<Option<(String, Vec<u8>)>, K::Error> {
        for name in self.backend.list(MAX_FILES)? {
            if !named_for::<K>(&name) {
                continue;
            }
            let Some(bytes) = self.backend.read(&name, K::MAX_FILE_BYTES)? else { continue };
            let ours = header(&bytes).is_some_and(|h| h.format.as_deref() == Some(K::FORMAT) && h.doc.is_some_and(|d| d.matches(keys)));
            if ours {
                return Ok(Some((name, bytes)));
            }
        }
        Ok(None)
    }

    /// Change the document's file with `change` and keep it, under the store's lock: the file is
    /// read again first (with what its earlier name holds merged in), so two writers never lose
    /// each other's changes; a newer version's file is refused. A file that can't be read, or that
    /// held entries this write drops, is set aside first ([`Loaded::set_aside`]). The document
    /// record takes the keys given (a save may have added an identifier).
    pub fn update<R, E>(
        &self,
        keys: &DocKeys,
        pages: usize,
        change: impl FnOnce(&mut K::File) -> Result<R, E>,
    ) -> Result<(R, Loaded<K::File>), K::Error>
    where
        K::Error: From<E>,
    {
        self.update_in(keys, pages, |_, _| Ok(()), change)
    }

    /// [`DocStore::update`], with `prepare` run under the lock on the file as read, before
    /// `change` (a kind's own housekeeping: the corrections take their entries out of a file
    /// several books shared, plan A9-G1b).
    pub(super) fn update_in<R, E>(
        &self,
        keys: &DocKeys,
        pages: usize,
        prepare: impl FnOnce(&Self, &mut Loaded<K::File>) -> Result<(), K::Error>,
        change: impl FnOnce(&mut K::File) -> Result<R, E>,
    ) -> Result<(R, Loaded<K::File>), K::Error>
    where
        K::Error: From<E>,
    {
        let lock = self.backend.lock::<K::Error>(K::KIND, K::OLD_LOCK)?;
        let mut l = self.load(keys, pages)?;
        l.old_lock_only = lock.old_lock_only();
        if let Some(version) = l.newer {
            return Err(K::Error::newer(version));
        }
        prepare(self, &mut l)?;
        let out = change(&mut l.file).map_err(K::Error::from)?;
        K::set_doc(&mut l.file, DocRecord::new(keys, pages));
        self.write(&mut l)?;
        drop(lock);
        Ok((out, l))
    }

    /// The document was saved, as itself or as another file: its file, found under the keys it
    /// had (`old`), records the keys it has now (`new`), so it is found when the saved file is
    /// opened. Whether there was anything to move: nothing is made for a document that keeps
    /// nothing, and a newer version's file is left as it is.
    pub fn rekey(&self, old: &DocKeys, new: &DocKeys, pages: usize) -> Result<bool, K::Error> {
        if old == new || file_name(old, K::KIND).is_none() {
            return Ok(false);
        }
        // Nothing to move: no lock taken, so a store that is not there (a folder) is not made.
        let before = self.load(old, pages)?;
        if before.read_only || !K::holds_anything(&before.file) {
            return Ok(false);
        }
        let _lock = self.backend.lock::<K::Error>(K::KIND, K::OLD_LOCK)?;
        let mut l = self.load(old, pages)?;
        if l.read_only || !K::holds_anything(&l.file) {
            return Ok(false);
        }
        K::set_doc(&mut l.file, DocRecord::new(new, pages));
        self.write(&mut l)?;
        Ok(true)
    }

    /// Write `l` over its file, after setting the file aside when it could not be read or held
    /// entries `l` left out, so nothing it held is lost.
    pub(super) fn write(&self, l: &mut Loaded<K::File>) -> Result<(), K::Error> {
        let bytes = K::to_bytes(&l.file)?;
        let why = match l.unreadable.take() {
            Some((name, why)) if name == l.name => Some(why),
            other => {
                l.unreadable = other;
                (l.own_skipped > 0).then(|| format!("{} entries could not be read", l.own_skipped))
            }
        };
        if let Some(why) = why {
            let aside = self.backend.set_aside::<K::Error>(&l.name)?;
            l.set_aside = Some((aside, why));
            l.own_skipped = 0;
        }
        self.backend.write::<K::Error>(&l.name, &bytes)
    }
}

/// The name a file that can't be read is set aside under: `<name>.bad`, else `.bad2`, … up to
/// `.bad999`, the first not `taken`.
pub(super) fn aside_name(name: &str, taken: impl Fn(&str) -> bool) -> Option<String> {
    (1..1000u32).map(|n| if n == 1 { format!("{name}.bad") } else { format!("{name}.bad{n}") }).find(|bad| !taken(bad))
}

/// A folder of files, one per document and kind (desktop and command line). A change holds the
/// kind's lock file in the folder, and writes through a staging file renamed over the old one.
#[cfg(not(target_arch = "wasm32"))]
pub struct DirBackend {
    dir: std::path::PathBuf,
    /// How long a change waits for another writer ([`LOCK_WAIT`] by default).
    wait: std::time::Duration,
}

#[cfg(not(target_arch = "wasm32"))]
impl DirBackend {
    pub fn new(dir: impl Into<std::path::PathBuf>) -> DirBackend {
        DirBackend { dir: dir.into(), wait: LOCK_WAIT }
    }

    /// The same folder, a change waiting at most `wait` for another writer (plan A9 §2.3: the
    /// interface tries once, `Duration::ZERO`, and never waits).
    pub fn waiting(mut self, wait: std::time::Duration) -> DirBackend {
        self.wait = wait;
        self
    }

    /// Whether another writer holds the store of kind `kind` now (one try of its lock, let go of
    /// at once). `false` when it can't be told (no folder yet, no file locks).
    pub fn busy(&self, kind: &str) -> bool {
        let Ok(file) = std::fs::OpenOptions::new().read(true).write(true).open(self.dir.join(format!("{kind}.flock"))) else { return false };
        match file.try_lock() {
            Ok(()) => {
                let _ = file.unlock();
                false
            }
            Err(std::fs::TryLockError::WouldBlock) => true,
            Err(std::fs::TryLockError::Error(_)) => false,
        }
    }

    /// The folder.
    pub fn dir(&self) -> &std::path::Path {
        &self.dir
    }
}

/// How long a change waits for another writer before it is refused as busy.
#[cfg(not(target_arch = "wasm32"))]
pub const LOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(3);

/// A staging file older than this was left by a writer that stopped.
#[cfg(not(target_arch = "wasm32"))]
const STALE_STAGING: std::time::Duration = std::time::Duration::from_secs(3600);

/// Most folder entries looked at for staging files left behind, per change.
#[cfg(not(target_arch = "wasm32"))]
const MAX_SWEPT: usize = 4096;

/// A folder held for a change ([`DirBackend::lock`]): `<kind>.flock` locked by the system, and
/// `<kind>.lock`, the lock of builds before, when taken. Dropping it lets go of both, the old one
/// first.
#[cfg(not(target_arch = "wasm32"))]
pub struct DirLock {
    old: Option<super::corrections::Lock>,
    /// `None`: the folder's file system has no locks, and `old` alone holds it.
    file: Option<std::fs::File>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Held for DirLock {
    fn old_lock_only(&self) -> bool {
        self.file.is_none()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for DirLock {
    fn drop(&mut self) {
        drop(self.old.take());
        if let Some(file) = self.file.take() {
            let _ = file.unlock();
        }
    }
}

/// Whether `name` is a staging file of the store's own of kind `kind`: `<hash>.<kind>.json.tmp<n>`
/// ([`super::corrections::write_atomically`]'s name for `<hash>.<kind>.json`).
#[cfg(not(target_arch = "wasm32"))]
fn is_own_staging(name: &str, kind: &str) -> bool {
    let Some((hash, rest)) = name.split_once('.') else { return false };
    let Some(n) = rest.strip_prefix(kind).and_then(|r| r.strip_prefix(".json.tmp")) else { return false };
    is_hash(hash) && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())
}

#[cfg(not(target_arch = "wasm32"))]
impl DirBackend {
    /// Remove the staging files of kind `kind` a writer that stopped left in the folder, more than
    /// an hour old ([`is_own_staging`]): never another name. Under the kind's lock.
    fn sweep(&self, kind: &str) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else { return };
        for e in entries.flatten().take(MAX_SWEPT) {
            let own = e.file_name().to_str().is_some_and(|n| is_own_staging(n, kind));
            let old = || e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > STALE_STAGING);
            if own && old() {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl StoreBackend for DirBackend {
    type Guard<'a> = DirLock;

    /// A folder that isn't there has no files. The `.json` names are filtered before the cap, so
    /// set-aside copies, staging files and locks never take a place.
    fn list(&self, max: usize) -> std::io::Result<Vec<String>> {
        let entries = match std::fs::read_dir(&self.dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let mut names: Vec<String> =
            entries.flatten().filter_map(|e| e.file_name().to_str().map(str::to_owned)).filter(|n| n.ends_with(".json")).take(max).collect();
        names.sort();
        Ok(names)
    }

    fn read(&self, name: &str, max: usize) -> std::io::Result<Option<Vec<u8>>> {
        match super::corrections::read_capped(&self.dir.join(name), max) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn write<E: StoreFailure>(&self, name: &str, bytes: &[u8]) -> Result<(), E> {
        std::fs::create_dir_all(&self.dir)?;
        Ok(super::corrections::write_atomically(&self.dir.join(name), bytes)?)
    }

    fn set_aside<E: StoreFailure>(&self, name: &str) -> Result<String, E> {
        let bad = aside_name(name, |n| self.dir.join(n).exists()).ok_or_else(E::busy)?;
        std::fs::rename(self.dir.join(name), self.dir.join(&bad))?;
        Ok(bad)
    }

    /// `<kind>.flock`, locked by the system for as long as the change lasts (a process that stops
    /// lets go of it), waited for at most [`LOCK_WAIT`]; with `old_lock`, or where the file system
    /// has no locks, `<kind>.lock` too, the lock file builds before plan A9-G3a take, inside it.
    /// Then the kind's staging files left behind are swept ([`DirBackend::sweep`]).
    fn lock<E: StoreFailure>(&self, kind: &str, old_lock: bool) -> Result<Self::Guard<'_>, E> {
        std::fs::create_dir_all(&self.dir)?;
        let path = self.dir.join(format!("{kind}.flock"));
        let file = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path)?;
        let deadline = std::time::Instant::now().checked_add(self.wait);
        let file = loop {
            match file.try_lock() {
                Ok(()) => break Some(file),
                Err(std::fs::TryLockError::WouldBlock) => {
                    if deadline.is_none_or(|d| std::time::Instant::now() >= d) {
                        return Err(E::busy());
                    }
                    std::thread::sleep(std::time::Duration::from_millis(15));
                }
                Err(std::fs::TryLockError::Error(e)) if e.kind() == std::io::ErrorKind::Unsupported => break None,
                Err(std::fs::TryLockError::Error(e)) => return Err(e.into()),
            }
        };
        let mut held = DirLock { old: None, file };
        if old_lock || held.file.is_none() {
            held.old = Some(super::corrections::take_lock(self.dir.join(format!("{kind}.lock")))?.ok_or_else(E::busy)?);
        }
        self.sweep(kind);
        Ok(held)
    }
}

/// Files kept in memory by name, within a byte budget of their own: the web build's store, which
/// it saves in the browser's storage through [`MemBackend::files`] and [`MemBackend::with_files`].
#[derive(Debug, Default)]
pub struct MemBackend {
    files: std::sync::Mutex<std::collections::BTreeMap<String, Vec<u8>>>,
    /// Held while a change reads, changes and writes.
    writing: std::sync::Mutex<()>,
    budget: usize,
}

impl MemBackend {
    /// An empty store keeping at most `budget` bytes in all.
    pub fn new(budget: usize) -> MemBackend {
        MemBackend { files: Default::default(), writing: Default::default(), budget }
    }

    /// A store holding `files` (as [`MemBackend::files`] gave them), every one of them: the
    /// `budget` refuses a write that would pass it ([`StoreFailure::over_budget`]), never drops
    /// what is there (plan A9-I1b, finding A4-15: files past it used to be left out, and gone at
    /// the next save). A file too large to read is reported by a load and set aside by a change,
    /// as in a folder.
    pub fn with_files(files: impl IntoIterator<Item = (String, Vec<u8>)>, budget: usize) -> MemBackend {
        MemBackend { files: std::sync::Mutex::new(files.into_iter().collect()), writing: Default::default(), budget }
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
}

impl StoreBackend for MemBackend {
    type Guard<'a> = std::sync::MutexGuard<'a, ()>;

    fn list(&self, max: usize) -> std::io::Result<Vec<String>> {
        Ok(self.guard().keys().filter(|n| n.ends_with(".json")).take(max).cloned().collect())
    }

    fn read(&self, name: &str, max: usize) -> std::io::Result<Option<Vec<u8>>> {
        Ok(self.guard().get(name).map(|b| b.get(..max.saturating_add(1)).unwrap_or(b).to_vec()))
    }

    /// Refused with [`StoreFailure::over_budget`] when the files would pass the budget.
    fn write<E: StoreFailure>(&self, name: &str, bytes: &[u8]) -> Result<(), E> {
        let mut files = self.guard();
        let others = files.iter().filter(|(n, _)| n.as_str() != name).fold(0usize, |sum, (_, b)| sum.saturating_add(b.len()));
        if others.saturating_add(bytes.len()) > self.budget {
            return Err(E::over_budget(self.budget));
        }
        files.insert(name.to_owned(), bytes.to_vec());
        Ok(())
    }

    fn set_aside<E: StoreFailure>(&self, name: &str) -> Result<String, E> {
        let mut files = self.guard();
        let aside = aside_name(name, |n| files.contains_key(n)).ok_or_else(E::busy)?;
        if let Some(bytes) = files.remove(name) {
            files.insert(aside.clone(), bytes);
        }
        Ok(aside)
    }

    /// One lock for every kind: the web build has one page writing. A lock poisoned by a panic
    /// elsewhere is taken as it is (it guards no data of its own).
    fn lock<E: StoreFailure>(&self, _kind: &str, _old_lock: bool) -> Result<Self::Guard<'_>, E> {
        Ok(self.writing.lock().unwrap_or_else(std::sync::PoisonError::into_inner))
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl<K: StoreKind> DocStore<K, DirBackend> {
    /// The store in the folder `dir` (made when a change is first kept).
    pub fn new(dir: impl Into<std::path::PathBuf>) -> Self {
        DocStore::on(DirBackend::new(dir))
    }

    /// The folder.
    pub fn dir(&self) -> &std::path::Path {
        self.backend.dir()
    }

    /// The store in the folder `dir`, a change waiting at most `wait` for another writer
    /// ([`DirBackend::waiting`]).
    pub fn waiting(dir: impl Into<std::path::PathBuf>, wait: std::time::Duration) -> Self {
        DocStore::on(DirBackend::new(dir).waiting(wait))
    }

    /// Whether another writer holds the store now ([`DirBackend::busy`]).
    pub fn busy(&self) -> bool {
        self.backend.busy(K::KIND)
    }

    /// Merge into this store what the folder `from` keeps of this kind (plan A9-G5a, decision
    /// D17): each file, by the kind's rule ([`StoreKind::merge`]), into the file here that records
    /// the same document, else into one named as it is (`<hash>.<kind>.json`); never copied over,
    /// and `from` is only read: nothing in it is moved, changed or deleted. Run again, a file whose
    /// size and time are what `migrated.json` here recorded when it was merged is not read again;
    /// one changed since (an earlier build went on writing it) is merged again. A file of a newer
    /// version, or one that can't be read, is left where it is and counted; so is a file here
    /// that can't be written over (of a newer version, or unreadable). Under the kind's lock. A
    /// folder `from` that is not there has nothing to merge.
    pub fn migrate_from(&self, from: &std::path::Path) -> Result<Migration, K::Error> {
        let source = DirBackend::new(from);
        let names: Vec<String> = source.list(MAX_FILES)?.into_iter().filter(|n| named_for::<K>(n)).collect();
        let mut out = Migration::default();
        if names.is_empty() || same_folder(from, self.backend.dir()) {
            return Ok(out);
        }
        let _lock = self.backend.lock::<K::Error>(K::KIND, K::OLD_LOCK)?;
        let mut record = Migrated::read(&self.backend);
        let mut here = self.records()?;
        let place = from.canonicalize().unwrap_or_else(|_| from.to_path_buf()).to_string_lossy().into_owned();
        for name in names {
            let Ok(meta) = std::fs::metadata(from.join(&name)) else { continue };
            let (size, time) = (meta.len(), nanos(&meta));
            let key = super::corrections::hash_hex(&[b"migrated", place.as_bytes(), name.as_bytes()]);
            let seen = record.sources.iter().position(|s| s.source == key);
            if seen.and_then(|i| record.sources.get(i)).is_some_and(|s| s.size == size && s.time == time) {
                out.unchanged = out.unchanged.saturating_add(1);
                continue;
            }
            let Some(bytes) = source.read(&name, K::MAX_FILE_BYTES)? else { continue };
            if header(&bytes).is_none_or(|h| h.format.as_deref() != Some(K::FORMAT)) {
                continue; // Another kind's, kept in the same folder.
            }
            let theirs = match K::read(&bytes) {
                Parsed::File { file, skipped } => {
                    out.skipped = out.skipped.saturating_add(skipped);
                    file
                }
                Parsed::Newer { .. } => {
                    out.newer = out.newer.saturating_add(1);
                    continue;
                }
                Parsed::Bad(_) => {
                    out.unreadable = out.unreadable.saturating_add(1);
                    continue;
                }
            };
            let Some(dest) =
                here.iter().find(|(_, doc)| doc.same_document(K::doc(&theirs))).map(|(n, _)| n.clone()).or_else(|| kind_name::<K>(&name))
            else {
                continue;
            };
            let merged = match self.backend.read(&dest, K::MAX_FILE_BYTES)? {
                None => theirs,
                Some(mine) => match K::read(&mine) {
                    Parsed::File { mut file, .. } => {
                        K::merge(&mut file, &theirs);
                        file
                    }
                    Parsed::Newer { .. } | Parsed::Bad(_) => {
                        out.left_here = out.left_here.saturating_add(1);
                        continue;
                    }
                },
            };
            self.backend.write::<K::Error>(&dest, &K::to_bytes(&merged)?)?;
            if !here.iter().any(|(n, _)| *n == dest) {
                here.push((dest, K::doc(&merged).clone()));
            }
            if seen.is_some() {
                out.changed = out.changed.saturating_add(1)
            } else {
                out.merged = out.merged.saturating_add(1)
            }
            let hash = super::corrections::hash_hex(&[&bytes]);
            record.keep(MigratedSource { source: key, size, time, hash });
            record.write::<K::Error>(&self.backend)?;
        }
        Ok(out)
    }

    /// Every file of this kind here, with the document it records (its header only).
    pub(super) fn records(&self) -> Result<Vec<(String, DocRecord)>, K::Error> {
        let mut out = Vec::new();
        for name in self.backend.list(MAX_FILES)?.into_iter().filter(|n| named_for::<K>(n)) {
            let Some(bytes) = self.backend.read(&name, K::MAX_FILE_BYTES)? else { continue };
            if let Some(Header { format: Some(f), doc: Some(doc) }) = header(&bytes)
                && f == K::FORMAT
            {
                out.push((name, doc));
            }
        }
        Ok(out)
    }
}

/// What [`DocStore::migrate_from`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Migration {
    /// Files merged in for the first time.
    pub merged: usize,
    /// Files merged before and changed since (an earlier build went on writing them), merged again.
    pub changed: usize,
    /// Files merged before and not changed since: not read again.
    pub unchanged: usize,
    /// Files of a newer version, which this build only reads: left where they are.
    pub newer: usize,
    /// Files that could not be read: left where they are.
    pub unreadable: usize,
    /// Files here that could not be merged into (of a newer version, or unreadable): left as they are.
    pub left_here: usize,
    /// Entries of the files merged that could not be read, left out.
    pub skipped: usize,
}

/// The name of the file of kind `K` for the document a file named `name` keeps
/// (`<hash>.json` or `<hash>.<kind>.json`): `<hash>.<kind>.json`.
#[cfg(not(target_arch = "wasm32"))]
fn kind_name<K: StoreKind>(name: &str) -> Option<String> {
    let stem = name.strip_suffix(".json")?;
    let hash = stem.strip_suffix(K::KIND).and_then(|s| s.strip_suffix('.')).unwrap_or(stem);
    is_hash(hash).then(|| format!("{hash}.{}.json", K::KIND))
}

/// Whether two folders are one.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn same_folder(a: &std::path::Path, b: &std::path::Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// A file's last change, in nanoseconds since 1970 (0 when the system can't say).
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn nanos(meta: &std::fs::Metadata) -> u64 {
    meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
}

/// The name of the record of what was migrated into a folder.
pub const MIGRATED: &str = "migrated.json";

/// Its format.
#[cfg(not(target_arch = "wasm32"))]
const MIGRATED_FORMAT: &str = "pdfcraft-chess-migrated";

/// Most source files it remembers; the oldest go first.
#[cfg(not(target_arch = "wasm32"))]
const MAX_MIGRATED: usize = 4 * MAX_FILES;

/// `migrated.json`: each source file merged into the folder, by a hash of its folder and name
/// (never its path), with its size and time then and a hash of its bytes.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub(super) struct Migrated {
    format: String,
    version: u32,
    #[serde(default)]
    pub(super) sources: Vec<MigratedSource>,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) struct MigratedSource {
    pub(super) source: String,
    pub(super) size: u64,
    pub(super) time: u64,
    pub(super) hash: String,
}

#[cfg(not(target_arch = "wasm32"))]
impl Migrated {
    /// The record in `backend`'s folder; an empty one when there is none, or it can't be read
    /// (every source is then merged again, which merging makes harmless).
    pub(super) fn read(backend: &DirBackend) -> Migrated {
        let read = backend.read(MIGRATED, 4 << 20).ok().flatten().and_then(|b| serde_json::from_slice::<Migrated>(&b).ok());
        read.filter(|m| m.format == MIGRATED_FORMAT).unwrap_or(Migrated { format: MIGRATED_FORMAT.into(), version: 1, sources: Vec::new() })
    }

    /// Remember `source` in place of what was remembered of it.
    pub(super) fn keep(&mut self, source: MigratedSource) {
        self.sources.retain(|s| s.source != source.source);
        self.sources.push(source);
        let over = self.sources.len().saturating_sub(MAX_MIGRATED);
        self.sources.drain(..over);
    }

    pub(super) fn write<E: StoreFailure>(&self, backend: &DirBackend) -> Result<(), E> {
        let bytes = serde_json::to_vec(self).map_err(|e| E::from(std::io::Error::other(e)))?;
        backend.write::<E>(MIGRATED, &bytes)
    }
}

impl<K: StoreKind> DocStore<K, MemBackend> {
    /// An empty memory store keeping at most `budget` bytes in all.
    pub fn new(budget: usize) -> Self {
        DocStore::on(MemBackend::new(budget))
    }

    /// A memory store holding `files` (as [`DocStore::files`] gave them), all of them, writing
    /// within `budget` bytes ([`MemBackend::with_files`]).
    pub fn with_files(files: impl IntoIterator<Item = (String, Vec<u8>)>, budget: usize) -> Self {
        DocStore::on(MemBackend::with_files(files, budget))
    }

    /// Every file by name, to save.
    pub fn files(&self) -> Vec<(String, Vec<u8>)> {
        self.backend.files()
    }
}
