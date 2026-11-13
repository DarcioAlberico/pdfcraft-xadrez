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

use pdfcraft_engine::chess::store::{DocStore, MemBackend, Parsed, StoreKind};

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
pub(crate) struct UiStore<K: StoreKind> {
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

impl<K: StoreKind> UiStore<K> {
    /// The session's memory, within `budget` bytes, kept nowhere (tests, a session without a
    /// data folder).
    pub(crate) fn memory(budget: usize) -> Self {
        UiStore { place: Place::Memory(DocStore::<K, MemBackend>::new(budget)), key: None, budget, changed: false, unreadable: None, bad: None }
    }

    /// The folder `dir` (the desktop; tests pass their own folder).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn folder(dir: std::path::PathBuf, budget: usize) -> Self {
        UiStore { place: Place::Folder(DocStore::<K, pdfcraft_engine::chess::store::DirBackend>::new(dir)), ..Self::memory(budget) }
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
        store.writable()?;
        let fps = vec!["fnv:p0".to_owned()];
        let new =
            NewCorrection { page: 0, rect: [0.1, y, 0.4, y + 0.2], orig: None, fen: "4k3/8/8/8/8/8/8/4K3 w - - 0 1".into(), kind: Kind::Corrected };
        let uid = in_store!(&store.place, s => s.update(&keys(), 1, |f| f.set(new, 1, &fps))).map(|(uid, _)| uid).map_err(|e| e.to_string())?;
        store.wrote();
        Ok(uid)
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
