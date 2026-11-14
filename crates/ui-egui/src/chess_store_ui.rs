//! Chess ▸ where the app keeps each kind of the engine's document stores (plan A9-I1b, §2.3 and
//! §2.7): the diagram corrections and the books' PGNs, one type for both, so the two copies the
//! app had are one. What is kept, and how two writers never lose each other's changes, are the
//! engine's ([`pdfcraft_engine::chess::store`]); the session holds what was loaded. Here is only
//! *where*: a folder on the desktop, else memory, which the web build keeps in eframe's
//! `Storage` under a key of its own.
//!
//! **The web's `Storage`** (finding A4-15):
//! - A value that can't be read is never written over: it is kept aside under `<key>.bad`
//!   (`.bad2`, … when taken) at the next save, and until then the store only reads, which the app
//!   says ([`UiStore::unreadable`]).
//! - Before writing, what the value holds now is read again and merged in by the kind's rule
//!   ([`StoreKind::merge`]): another tab's changes since this one loaded are kept, and a deletion
//!   stays deleted.
//! - A file past the budget is never left out when loaded; a change that would pass it is refused,
//!   and said.
//!
//! **A document whose chess is not written** (plan A9-I1c, [`crate::chess_keep`]): an encrypted
//! one before the user's yes, or after a no, or one that doesn't allow copying. Its changes go to
//! a memory store of their own ([`UiStore::at`]), never saved, which takes what the store held of
//! it first ([`UiStore::hold`]); a yes merges them into the store by the kind's rule.
//!
//! **The interface never waits for another writer** (plan A9 §2.3, G3b): a folder's lock is tried
//! once. When another writer (the command line, another window) holds it, the change is made in
//! that same memory, from what the folder has, and the store's writer thread
//! ([`crate::chess_writer`]) merges it in by the kind's rule once the lock is free
//! ([`UiStore::change`]); until then the document's changes and reads go there, so nothing seen
//! is lost. Quit waits for those writes as for the games' ([`UiStore::settle`]).

use pdfcraft_engine::chess::DocKeys;
use pdfcraft_engine::chess::store::{DocStore, MemBackend, Parsed, StoreKind, file_name};

/// The documents' files the memory for documents not written holds at most, at the kind's
/// largest file each (plan A9-I1c).
const UNKEPT_FILES: usize = 16;

/// Where a kind of store is kept.
pub(crate) enum Place<K: StoreKind> {
    #[cfg(not(target_arch = "wasm32"))]
    Folder(DocStore<K, pdfcraft_engine::chess::store::DirBackend>),
    Memory(DocStore<K, MemBackend>),
}

/// `$body` with `$s` the store at `$place`, wherever it is kept: the stores of a folder and of
/// memory are two types, whose methods are the same.
macro_rules! in_store {
    ($place:expr, $s:ident => $body:expr) => {
        match $place {
            #[cfg(not(target_arch = "wasm32"))]
            $crate::chess_store_ui::Place::Folder($s) => $body,
            $crate::chess_store_ui::Place::Memory($s) => $body,
        }
    };
}
pub(crate) use in_store;

/// A kind of store, where the app keeps it.
pub(crate) struct UiStore<K: StoreKind>
where
    K::File: Send + 'static,
{
    pub(crate) place: Place<K>,
    /// The web's `Storage` key it is kept under (`None`: a folder, or the session's memory).
    key: Option<&'static str>,
    /// The bytes it writes within in memory.
    budget: usize,
    /// Its files changed since the `Storage` value was last taken.
    changed: bool,
    /// Why the `Storage` value could not be read: until it is kept aside, nothing is written.
    unreadable: Option<String>,
    /// That value, to keep under `<key>.bad` at the next save.
    bad: Option<String>,
    /// The documents whose chess is not written (plan A9-I1c): their changes, in memory for the
    /// session, never saved. Always [`Place::Memory`]. Also the changes held while another writer
    /// has the folder (`held`).
    unkept: Place<K>,
    /// The documents whose changes are held in memory until the writer thread merged them into
    /// the folder (plan A9-G3b), by file name, with their last request's generation.
    held: std::collections::HashMap<String, u64>,
    /// The writer thread that merges them, made with the first held change.
    #[cfg(not(target_arch = "wasm32"))]
    writer: Option<HeldWriter<K::File>>,
}

/// The writer thread of a store's held changes.
#[cfg(not(target_arch = "wasm32"))]
type HeldWriter<F> = crate::chess_writer::StoreWriter<HeldJob<F>, Result<(), String>>;

/// How many more times the writer thread waits for a folder another writer keeps (3 s each).
#[cfg(not(target_arch = "wasm32"))]
const HELD_TRIES: u32 = 10;

/// A change held in memory while another writer had the folder: the document's file as it is
/// there, for the writer thread to merge in (plan A9-G3b).
#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct HeldJob<F> {
    keys: DocKeys,
    pages: usize,
    file: F,
}

/// The files of a `Storage` value: each file's name and text, as [`UiStore::storage_values`]
/// writes them. Why not, when it is not that.
fn parse_value(text: &str) -> Result<Vec<(String, Vec<u8>)>, String> {
    let files: Vec<(String, String)> = serde_json::from_str(text).map_err(|e| e.to_string())?;
    Ok(files.into_iter().map(|(name, body)| (name, body.into_bytes())).collect())
}

/// `theirs` (what another tab wrote) taken into `mine` file by file: a file only one side has is
/// kept; one both have, merged by the kind's rule; one that either side can't read (or of a newer
/// version) is left as `mine` has it, `theirs` never written over it.
fn merge_files<K: StoreKind>(mine: Vec<(String, Vec<u8>)>, theirs: Vec<(String, Vec<u8>)>) -> Vec<(String, Vec<u8>)> {
    let mut out: std::collections::BTreeMap<String, Vec<u8>> = mine.into_iter().collect();
    for (name, bytes) in theirs {
        let merged = match out.get(&name) {
            None => Some(bytes),
            Some(m) if *m == bytes => None,
            Some(m) => match (K::read(m), K::read(&bytes)) {
                (Parsed::File { file: mut a, .. }, Parsed::File { file: b, .. }) => {
                    K::merge(&mut a, &b);
                    K::to_bytes(&a).ok()
                }
                _ => None,
            },
        };
        if let Some(bytes) = merged {
            out.insert(name, bytes);
        }
    }
    out.into_iter().collect()
}

impl<K: StoreKind> UiStore<K>
where
    K::File: Send + 'static,
{
    /// The session's memory, within `budget` bytes, kept nowhere (tests, a session without a
    /// data folder).
    pub(crate) fn memory(budget: usize) -> Self {
        UiStore {
            place: Place::Memory(DocStore::<K, MemBackend>::new(budget)),
            key: None,
            budget,
            changed: false,
            unreadable: None,
            bad: None,
            unkept: Place::Memory(DocStore::<K, MemBackend>::new(UNKEPT_FILES.saturating_mul(K::MAX_FILE_BYTES))),
            held: std::collections::HashMap::new(),
            #[cfg(not(target_arch = "wasm32"))]
            writer: None,
        }
    }

    /// Where a change of a document goes: the store when its chess may be written (`write`), else
    /// the memory for the documents whose chess is not (plan A9-I1c).
    pub(crate) fn at(&self, write: bool) -> &Place<K> {
        if write { &self.place } else { &self.unkept }
    }

    /// The document named by `keys`, of `pages` pages, is not written from now on: its changes go
    /// to memory ([`Self::at`]), which starts from `file`, what the store holds of it now. Why
    /// not, in words that follow "couldn't be saved:".
    pub(crate) fn hold(&self, keys: &DocKeys, pages: usize, file: K::File) -> Result<(), String>
    where
        K::Error: std::fmt::Display,
    {
        let held = in_store!(&self.unkept, s => s.update(keys, pages, |f| {
            *f = file;
            Ok::<(), std::io::Error>(())
        }));
        held.map(|_| ()).map_err(|e| e.to_string())
    }

    /// The folder `dir` (the desktop; tests pass their own folder). A change tries its lock once:
    /// the interface never waits for another writer ([`Self::change`]).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn folder(dir: std::path::PathBuf, budget: usize) -> Self {
        UiStore {
            place: Place::Folder(DocStore::<K, pdfcraft_engine::chess::store::DirBackend>::waiting(dir, std::time::Duration::ZERO)),
            ..Self::memory(budget)
        }
    }

    /// Where the document named by `keys` is read now: the memory holding its changes while the
    /// writer thread has not merged them yet (plan A9-G3b), else the store.
    pub(crate) fn reading(&self, keys: &DocKeys) -> &Place<K> {
        match file_name(keys, K::KIND) {
            Some(name) if self.held.contains_key(&name) => &self.unkept,
            _ => &self.place,
        }
    }

    /// Whether another writer holds the folder now.
    fn folder_busy(&self) -> bool {
        match &self.place {
            #[cfg(not(target_arch = "wasm32"))]
            Place::Folder(s) => s.busy(),
            Place::Memory(_) => false,
        }
    }

    /// The web's `Storage` value under `key`, `text` (what it holds now). A value that can't be
    /// read is kept to be set aside at the next save, and the store only reads until then.
    pub(crate) fn storage(key: &'static str, budget: usize, text: Option<&str>) -> Self {
        let mut store = UiStore { key: Some(key), ..Self::memory(budget) };
        match text.map(parse_value) {
            None => {}
            Some(Ok(files)) => store.place = Place::Memory(DocStore::<K, MemBackend>::with_files(files, budget)),
            Some(Err(why)) => {
                store.unreadable = Some(why);
                store.bad = text.map(str::to_owned);
            }
        }
        store
    }

    /// Why the `Storage` value could not be read, while it is not kept aside yet: nothing is
    /// written until then.
    pub(crate) fn unreadable(&self) -> Option<&str> {
        self.unreadable.as_deref()
    }

    /// Whether a change may be written: not while a value that could not be read is still to be
    /// kept aside; then why, in words a refusal says after "couldn't be saved:".
    pub(crate) fn writable(&self) -> Result<(), String> {
        match &self.unreadable {
            Some(_) => Err(tl!("what was saved before couldn't be read, and is kept aside first; try again in a moment").to_string()),
            None => Ok(()),
        }
    }

    /// A change was written: the `Storage` value is to be saved again.
    pub(crate) fn wrote(&mut self) {
        self.changed = true;
    }

    /// The `Storage` values to save, in order, each `(key, value)`: the value that could not be
    /// read, under the first of `<key>.bad`, `<key>.bad2`, … that `taken` says is free; then, when
    /// the files changed (or that value was just set aside), the files, with what `current` (the
    /// value the `Storage` holds now: another tab may have written it) holds merged in. Nothing for
    /// a folder, or the session's memory.
    pub(crate) fn storage_values(&mut self, current: Option<&str>, taken: impl Fn(&str) -> bool) -> Vec<(String, String)> {
        let Some(key) = self.key else { return Vec::new() };
        let aside = |taken: &dyn Fn(&str) -> bool| {
            (1..1000u32).map(|n| if n == 1 { format!("{key}.bad") } else { format!("{key}.bad{n}") }).find(|n| !taken(n))
        };
        let mut out = Vec::new();
        if let Some(bad) = self.bad.take() {
            let Some(name) = aside(&taken) else {
                // Nowhere to keep it: it stays, and so does the key, unwritten.
                self.bad = Some(bad);
                return out;
            };
            out.push((name, bad));
            self.unreadable = None;
            // The key is this store's now: its value goes too, so the next start reads it.
            self.changed = true;
        }
        if !std::mem::take(&mut self.changed) {
            return out;
        }
        let mine = match &self.place {
            Place::Memory(store) => store.files(),
            #[cfg(not(target_arch = "wasm32"))]
            Place::Folder(_) => return out,
        };
        let theirs = match current.map(parse_value) {
            None => Vec::new(),
            Some(Ok(files)) => files,
            // Written meanwhile and unreadable: kept aside too, never written over.
            Some(Err(_)) => {
                let taken_now = |n: &str| taken(n) || out.iter().any(|(k, _)| k == n);
                if let (Some(name), Some(text)) = (aside(&taken_now), current) {
                    out.push((name, text.to_owned()));
                }
                Vec::new()
            }
        };
        let merged = merge_files::<K>(mine, theirs);
        let texts: Vec<(String, String)> = merged.iter().filter_map(|(n, b)| String::from_utf8(b.clone()).ok().map(|b| (n.clone(), b))).collect();
        if let Ok(value) = serde_json::to_string(&texts) {
            out.push((key.to_owned(), value));
        }
        self.place = Place::Memory(DocStore::<K, MemBackend>::with_files(merged, self.budget));
        out
    }
}

impl<K: StoreKind + 'static> UiStore<K>
where
    K::File: Send + 'static,
    K::Error: std::fmt::Display,
{
    /// Make a change of the document named by `keys`, of `pages` pages, with `op` (run once, on
    /// where the change goes): memory only when its chess is not written (`write` false, plan
    /// A9-I1c); the store when it may be, unless another writer holds the folder now (plan A9 §2.3:
    /// the interface tries the lock once) or the document has changes the writer thread has not
    /// merged yet: then in memory, from what the folder has, and handed to the writer thread to
    /// merge in. Why not, in words that follow "couldn't be saved:".
    pub(crate) fn change<R>(
        &mut self,
        write: bool,
        keys: &DocKeys,
        pages: usize,
        op: impl FnOnce(&Place<K>) -> Result<R, String>,
    ) -> Result<R, String> {
        if !write {
            return op(&self.unkept);
        }
        self.writable()?;
        let name = file_name(keys, K::KIND);
        let held = name.as_ref().is_some_and(|n| self.held.contains_key(n));
        let Some(name) = name.filter(|_| held || self.folder_busy()) else {
            let out = op(&self.place)?;
            self.wrote();
            return Ok(out);
        };
        if !held {
            let now = in_store!(&self.place, s => s.load(keys, pages)).map_err(|e| e.to_string())?;
            if let Some(version) = now.newer {
                return Err(<K::Error as pdfcraft_engine::chess::store::StoreFailure>::newer(version).to_string());
            }
            self.hold(keys, pages, now.file)?;
        }
        let out = op(&self.unkept)?;
        let file = in_store!(&self.unkept, s => s.load(keys, pages)).map_err(|e| e.to_string())?.file;
        let generation = self.submit(&name, keys, pages, file);
        self.held.insert(name, generation);
        Ok(out)
    }

    /// Have the writer thread merge `file`, the document's held changes, into the folder.
    #[cfg(not(target_arch = "wasm32"))]
    fn submit(&mut self, name: &str, keys: &DocKeys, pages: usize, file: K::File) -> u64 {
        let Place::Folder(store) = &self.place else { return 0 };
        let dir = store.dir().to_path_buf();
        let writer = self.writer.get_or_insert_with(|| {
            crate::chess_writer::StoreWriter::new(
                "chess-store-writer",
                move |_name: &str, job: HeldJob<K::File>| {
                    let store = DocStore::<K, pdfcraft_engine::chess::store::DirBackend>::new(&dir);
                    // Each try waits the store's 3 s for the lock; a writer that keeps it longer is
                    // waited for again, a few times, off the interface thread.
                    let mut tries = 0u32;
                    loop {
                        // The error type the store's own failures convert from (a generic `K` takes the
                        // `From<io::Error>` its `StoreFailure` gives).
                        let merged = store.update(&job.keys, job.pages, |f| {
                            K::merge(f, &job.file);
                            Ok::<(), std::io::Error>(())
                        });
                        match merged {
                            Ok(_) => return Ok(()),
                            Err(_) if tries < HELD_TRIES && store.busy() => tries = tries.saturating_add(1),
                            Err(e) => return Err(e.to_string()),
                        }
                    }
                },
                |panic| Err(format!("the writer stopped: {panic}")),
            )
        });
        writer.submit(name, HeldJob { keys: keys.clone(), pages, file })
    }

    /// The web build has no folders, so nothing is ever held.
    #[cfg(target_arch = "wasm32")]
    fn submit(&mut self, _name: &str, _keys: &DocKeys, _pages: usize, _file: K::File) -> u64 {
        0
    }

    /// Take in what became of the held changes the writer thread merged: a document whose last
    /// one is in the folder is read there again. Why each one that failed did (it stays held, and
    /// the next change of it tries again).
    pub(crate) fn poll_held(&mut self) -> Vec<String> {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(w) = &self.writer {
            let mut failed = Vec::new();
            for d in w.take_done() {
                match d.result {
                    Ok(()) if self.held.get(&d.file) == Some(&d.generation) => {
                        self.held.remove(&d.file);
                    }
                    Ok(()) => {}
                    Err(why) => failed.push(why),
                }
            }
            return failed;
        }
        Vec::new()
    }

    /// Wait at most `timeout` for the held changes to be merged in (Close and Quit), then take in
    /// what became of them ([`Self::poll_held`]).
    pub(crate) fn settle(&mut self, timeout: std::time::Duration) -> Vec<String> {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(w) = &self.writer {
            w.settle(timeout);
        }
        #[cfg(target_arch = "wasm32")]
        let _ = timeout;
        self.poll_held()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdfcraft_engine::chess::DocKeys;
    use pdfcraft_engine::chess::corrections::{Corrections, Kind, NewCorrection};

    fn keys() -> DocKeys {
        DocKeys { id0: None, path: None, name: Some("book.pdf".into()), size: Some(10) }
    }

    /// One correction of page 1's diagram at `y`, in a document of one page.
    fn put(store: &mut UiStore<Corrections>, y: f32) -> Result<String, String> {
        let fps = vec!["fnv:p0".to_owned()];
        let new =
            NewCorrection { page: 0, rect: [0.1, y, 0.4, y + 0.2], orig: None, fen: "4k3/8/8/8/8/8/8/4K3 w - - 0 1".into(), kind: Kind::Corrected };
        store.change(true, &keys(), 1, |place| {
            in_store!(place, s => s.update(&keys(), 1, |f| f.set(new, 1, &fps))).map(|(uid, _)| uid).map_err(|e| e.to_string())
        })
    }

    fn live(value: &str) -> usize {
        let store = UiStore::<Corrections>::storage("k", 1 << 20, Some(value));
        in_store!(&store.place, s => s.load(&keys(), 1)).map_or(0, |l| l.file.live().count())
    }

    /// Plan A9-I1b (finding A4-15): a `Storage` value that can't be read is never written over. It
    /// is kept aside under `<key>.bad` at the next save, the first name free, and until then
    /// nothing is written; then the key is the store's.
    #[test]
    fn an_unreadable_value_is_kept_aside_and_never_written_over() {
        let mut store = UiStore::<Corrections>::storage("k", 1 << 20, Some("{trunc"));
        assert!(store.unreadable().is_some());
        assert!(put(&mut store, 0.1).is_err(), "nothing written while it is not kept aside");
        let saved = store.storage_values(Some("{trunc"), |n| n == "k.bad");
        assert_eq!(saved.first(), Some(&("k.bad2".to_owned(), "{trunc".to_owned())), "kept aside, under the first name free");
        assert!(saved.iter().all(|(k, v)| k != "k" || v != "{trunc"), "{saved:?}");
        assert!(store.unreadable().is_none());
        put(&mut store, 0.1).expect("written once it is kept aside");
        let saved = store.storage_values(None, |_| false);
        let (_, value) = saved.iter().find(|(k, _)| k == "k").expect("the key's value");
        assert_eq!(live(value), 1);
    }

    /// Plan A9-G3b (§2.3): the interface never waits for another writer's lock. While another
    /// writer (the command line, another window) holds the folder, a correction is made at once in
    /// memory and read back from there; the writer thread merges it in once the lock is free, and
    /// Quit waits for it. Before: the interface waited up to 3 s, then refused.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_change_never_waits_for_another_writer() {
        let dir = std::env::temp_dir().join(format!("pdfcraft-ui-store-busy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut store = UiStore::<Corrections>::folder(dir.clone(), 1 << 20);
        let other = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(dir.join("corrections.flock")).unwrap();
        other.try_lock().expect("another writer holds the folder");
        let started = std::time::Instant::now();
        let uid = put(&mut store, 0.1).expect("made at once");
        assert!(started.elapsed() < std::time::Duration::from_millis(1500), "waited {:?}", started.elapsed());
        let seen = in_store!(store.reading(&keys()), s => s.load(&keys(), 1)).unwrap();
        assert_eq!(seen.file.live().map(|c| c.uid.clone()).collect::<Vec<_>>(), std::slice::from_ref(&uid), "read back from memory");
        let on_disk = || pdfcraft_engine::chess::corrections::DirStore::new(&dir).load(&keys(), 1).unwrap().file.live().count();
        assert_eq!(on_disk(), 0, "nothing written while the other writer holds it");
        other.unlock().unwrap();
        assert!(store.settle(std::time::Duration::from_secs(5)).is_empty());
        assert_eq!(on_disk(), 1, "merged in once the lock was free");
        // A second change, the folder free: straight to the store.
        put(&mut store, 0.6).unwrap();
        assert_eq!(on_disk(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Plan A9-I1b (finding A4-15): before writing, the value another tab wrote since this one
    /// loaded is merged in, never written over; a deletion stays deleted.
    #[test]
    fn another_tabs_changes_are_merged_in_before_writing() {
        let mut first = UiStore::<Corrections>::storage("k", 1 << 20, None);
        let mut second = UiStore::<Corrections>::storage("k", 1 << 20, None);
        put(&mut first, 0.1).unwrap();
        let (_, written) = first.storage_values(None, |_| false).pop().unwrap();
        put(&mut second, 0.6).unwrap();
        let (_, both) = second.storage_values(Some(&written), |_| false).pop().unwrap();
        assert_eq!(live(&both), 2, "the other tab's correction is kept");
        // Not changed since: nothing to write.
        assert!(second.storage_values(Some(&both), |_| false).is_empty());
    }
}
