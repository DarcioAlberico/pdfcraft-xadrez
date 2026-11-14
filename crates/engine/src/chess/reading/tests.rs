//! The reading kept on disk (plan A9-N5a): an entry read back as it was kept; a damaged one, or
//! one kept for another key, set aside and never served; the books used longest ago leaving
//! first past the budget, never the book being written; nothing kept on a nearly full disk; only
//! the store's own files ever removed; the keys, and the format, as this build makes them.

use std::sync::Arc;
use std::time::Duration;

use super::*;
use crate::chess::ChessPage;

/// A folder of its own for each test.
fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcraft-reading-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// A document named by its identifier `n`.
fn doc(n: u8) -> DocKeys {
    DocKeys { id0: Some([n; 16]), ..Default::default() }
}

fn key(n: u8) -> EntryKey {
    EntryKey([n; 32])
}

/// A small value of `bytes` bytes or so: a page's notes.
fn value(bytes: usize) -> Vec<String> {
    // Hexadecimal of a counter does not deflate much.
    (0..bytes / 8).map(|i| format!("{:08x}", (i as u32).wrapping_mul(2_654_435_761))).collect()
}

/// The entry file of `key` for `doc` in `dir`.
fn entry(dir: &Path, doc: &DocKeys, key: &EntryKey) -> PathBuf {
    dir.join(book_folder(doc).unwrap()).join(format!("{}.page", key.hex()))
}

/// A real page's scan: three diagrams, a font's read.
fn scanned() -> ChessPage {
    let (mut s, id) = crate::chess::tests::open(crate::chess::tests::three_boards(400));
    (*s.chess_page(id, 0, &crate::chess::tests::no_model()).unwrap()).clone()
}

#[test]
fn a_kept_reading_is_read_back_as_it_was() {
    let dir = folder("back");
    let store = ReadingStore::new(&dir);
    let page = scanned();
    assert_eq!(store.get::<ChessPage>(&doc(1), &key(1)), Found::Miss);
    assert_eq!(store.put(&doc(1), 3, &key(1), &page).unwrap(), Kept::Yes);
    assert_eq!(store.get::<ChessPage>(&doc(1), &key(1)), Found::Hit(page.clone()));
    // Another key, another document: nothing.
    assert_eq!(store.get::<ChessPage>(&doc(1), &key(2)), Found::Miss);
    assert_eq!(store.get::<ChessPage>(&doc(2), &key(1)), Found::Miss);
    // A store opened on the folder later (a reopened app) finds it.
    assert_eq!(ReadingStore::new(&dir).get::<ChessPage>(&doc(1), &key(1)), Found::Hit(page));
    // Compressed: a reading of three diagrams takes a few kilobytes.
    let size = std::fs::metadata(entry(&dir, &doc(1), &key(1))).unwrap().len();
    let json = serde_json::to_vec(&scanned()).unwrap().len() as u64;
    assert!(size * 2 < json, "{size} bytes kept for {json} of JSON");
    let status = store.status();
    assert_eq!(status.len(), 1);
    assert_eq!((status[0].entries, status[0].set_aside, status[0].doc.as_ref().map(|d| d.pages)), (1, 0, Some(3)));
    // No key, nothing kept.
    assert_eq!(store.put(&DocKeys::default(), 1, &key(1), &value(8)).unwrap(), Kept::NoKeys);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A change made to an entry's bytes.
type Damage = Box<dyn Fn(Vec<u8>) -> Vec<u8>>;

/// An entry that can't be trusted (damaged, cut short, of another version, kept under another key,
/// too large, of another kind of reading) is set aside and said, never served; the page's next
/// look finds nothing, so it is read again.
#[test]
fn an_entry_that_cant_be_trusted_is_set_aside_never_served() {
    let dir = folder("bad");
    let store = ReadingStore::new(&dir);
    let page = scanned();
    let at = entry(&dir, &doc(1), &key(1));
    let mut bad = at.as_os_str().to_owned();
    bad.push(".bad");
    let bad = PathBuf::from(bad);
    let damage: Vec<(&str, Damage)> = vec![
        (
            "a byte of the data",
            Box::new(|mut b: Vec<u8>| {
                let at = b.len() - 3;
                b[at] ^= 0x5a;
                b
            }),
        ),
        ("cut short", Box::new(|b: Vec<u8>| b[..b.len() / 2].to_vec())),
        ("cut in the header", Box::new(|b: Vec<u8>| b[..20].to_vec())),
        (
            "another version",
            Box::new(|mut b: Vec<u8>| {
                b[MAGIC.len()] = 2;
                b
            }),
        ),
        (
            "the digest",
            Box::new(|mut b: Vec<u8>| {
                b[MAGIC.len() + 4 + 32] ^= 1;
                b
            }),
        ),
        ("another file", Box::new(|_| b"%PDF-1.7".to_vec())),
        ("too large", Box::new(|b: Vec<u8>| [b, vec![0; MAX_ENTRY_BYTES]].concat())),
    ];
    for (what, change) in damage {
        assert_eq!(store.put(&doc(1), 1, &key(1), &page).unwrap(), Kept::Yes);
        let bytes = std::fs::read(&at).unwrap();
        std::fs::write(&at, change(bytes)).unwrap();
        assert!(matches!(store.get::<ChessPage>(&doc(1), &key(1)), Found::SetAside(_)), "{what}");
        assert!(!at.exists() && bad.exists(), "{what}: set aside");
        assert_eq!(store.get::<ChessPage>(&doc(1), &key(1)), Found::Miss, "{what}");
        assert_eq!(store.status()[0].set_aside, 1, "{what}: one set-aside entry, the latest");
    }
    // An entry kept under another key's name.
    store.put(&doc(1), 1, &key(2), &page).unwrap();
    std::fs::rename(entry(&dir, &doc(1), &key(2)), &at).unwrap();
    assert_eq!(store.get::<ChessPage>(&doc(1), &key(1)), Found::SetAside("kept for another key".into()));
    // A reading of another kind under the key asked for.
    store.put(&doc(1), 1, &key(1), &value(64)).unwrap();
    assert!(matches!(store.get::<ChessPage>(&doc(1), &key(1)), Found::SetAside(why) if why.contains("can't be read")));
    // Put again, it is kept and served.
    store.put(&doc(1), 1, &key(1), &page).unwrap();
    assert_eq!(store.get::<ChessPage>(&doc(1), &key(1)), Found::Hit(page));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Set the last use of book `doc` in `dir` to `secs` seconds after 2026-01-01.
fn used_at(dir: &Path, doc: &DocKeys, secs: u64) {
    let record = dir.join(book_folder(doc).unwrap()).join(BOOK);
    let at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_767_225_600 + secs);
    std::fs::OpenOptions::new().write(true).open(record).unwrap().set_modified(at).unwrap();
}

/// Past the budget, whole books leave, the one used longest ago first, until the new entry fits
/// with a tenth to spare; the book being written never leaves, and a book that alone passes the
/// budget keeps what it has.
#[test]
fn past_the_budget_the_books_used_longest_ago_leave_first() {
    let dir = folder("budget");
    let one = {
        let probe = ReadingStore::new(folder("budget-probe"));
        probe.put(&doc(9), 1, &key(9), &value(4000)).unwrap();
        let size = std::fs::metadata(entry(probe.dir(), &doc(9), &key(9))).unwrap().len();
        let _ = std::fs::remove_dir_all(probe.dir());
        size
    };
    // Room for about ten entries.
    let max = one * 10 + 600;
    let store = ReadingStore::new(&dir).with_max_bytes(max);
    for (n, d) in [(0u8, 1u8), (1, 1), (2, 2), (3, 2), (4, 3), (5, 3), (6, 4), (7, 4)] {
        assert_eq!(store.put(&doc(d), 2, &key(n), &value(4000)).unwrap(), Kept::Yes);
    }
    // Used longest ago: book 3, then 1, 4 and 2.
    let order = [3u8, 1, 4, 2];
    for (at, d) in order.iter().enumerate() {
        used_at(&dir, &doc(*d), at as u64 * 10);
    }
    let id = |d: u8| Some(hex(&[d; 16]));
    let left = |store: &ReadingStore| -> Vec<Option<String>> { store.status().iter().map(|b| b.doc.as_ref().and_then(|d| d.id0.clone())).collect() };
    // A new book, in a store opened since (its times are the ones on disk): pages are kept until
    // books leave for them.
    let store = ReadingStore::new(&dir).with_max_bytes(max);
    let mut n = 10u8;
    while left(&store).len() == 5 || n == 10 {
        assert_eq!(store.put(&doc(5), 2, &key(n), &value(4000)).unwrap(), Kept::Yes, "{n}");
        n += 1;
        assert!(n < 40);
    }
    let now = left(&store);
    let gone: Vec<u8> = order.iter().copied().filter(|d| !now.contains(&id(*d))).collect();
    assert!(!gone.is_empty() && gone == order[..gone.len()], "the books used longest ago leave first: {gone:?}");
    assert!(now.contains(&id(5)));
    let total: u64 = store.status().iter().map(|b| b.bytes).sum();
    assert!(total <= max, "{total} > {max}");
    // The book being written never leaves: alone past the budget, it keeps what it has.
    let kept = loop {
        let kept = store.put(&doc(5), 2, &key(n), &value(4000)).unwrap();
        n += 1;
        if kept != Kept::Yes || n == 60 {
            break kept;
        }
    };
    assert_eq!(kept, Kept::Full);
    assert_eq!(left(&store), [id(5)], "only the book written is left");
    assert!(store.status()[0].bytes <= max);
    assert_eq!(store.get::<Vec<String>>(&doc(5), &key(10)), Found::Hit(value(4000)));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Nothing is kept while the disk has less than the free space asked for; a disk that can't say
/// is taken as having room.
#[test]
fn nothing_is_kept_on_a_nearly_full_disk() {
    let dir = folder("space");
    let free = Arc::new(std::sync::atomic::AtomicU64::new(500 << 20));
    let probe = {
        let free = free.clone();
        Arc::new(move |_: &Path| Some(free.load(std::sync::atomic::Ordering::Relaxed))) as FreeSpace
    };
    let store = ReadingStore::new(&dir).with_free_space(probe, MIN_FREE_BYTES);
    assert_eq!(store.put(&doc(1), 1, &key(1), &value(64)).unwrap(), Kept::LowSpace);
    assert!(!dir.exists(), "nothing written");
    free.store(2 << 30, std::sync::atomic::Ordering::Relaxed);
    assert_eq!(store.put(&doc(1), 1, &key(1), &value(64)).unwrap(), Kept::Yes);
    let unknown = ReadingStore::new(&dir).with_free_space(Arc::new(|_: &Path| None), MIN_FREE_BYTES);
    assert_eq!(unknown.put(&doc(1), 1, &key(2), &value(64)).unwrap(), Kept::Yes);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A value JSON can't hold (a NaN, written as `null`) would read back as nothing every time: it
/// is not kept.
#[test]
fn a_reading_json_cant_hold_is_not_kept() {
    let dir = folder("nan");
    let store = ReadingStore::new(&dir);
    let page = ChessPage { dpi: f32::NAN, ..scanned() };
    assert_eq!(store.put(&doc(1), 1, &key(1), &page).unwrap(), Kept::Unwritable);
    assert_eq!(store.get::<ChessPage>(&doc(1), &key(1)), Found::Miss);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Clearing takes out the store's own files and nothing else: a folder given by the user keeps
/// what is its own, and a book's folder holding one of its files is left with it.
#[test]
fn clearing_takes_out_only_the_stores_own_files() {
    let dir = folder("clear");
    let store = ReadingStore::new(&dir);
    for d in 1..=3 {
        store.put(&doc(d), 1, &key(d), &value(64)).unwrap();
    }
    std::fs::write(dir.join("notes.txt"), b"mine").unwrap();
    let book2 = dir.join(book_folder(&doc(2)).unwrap());
    std::fs::write(book2.join("mine.page.txt"), b"mine").unwrap();
    std::fs::write(book2.join(format!("{}.json", "a".repeat(64))), b"mine").unwrap();
    // One book.
    let one = store.clear(Some(&doc(1)));
    assert_eq!((one.books, one.files), (1, 2), "{one:?}");
    assert_eq!(store.get::<Vec<String>>(&doc(1), &key(1)), Found::Miss);
    assert_eq!(store.get::<Vec<String>>(&doc(3), &key(3)), Found::Hit(value(64)));
    // Everything.
    let all = store.clear(None);
    assert_eq!(all.books, 2, "{all:?}");
    assert_eq!(store.status().len(), 1, "book 2's folder is left with the user's files");
    assert_eq!(store.status()[0].entries, 0);
    assert!(dir.join("notes.txt").exists() && book2.join("mine.page.txt").exists());
    assert!(!dir.join(book_folder(&doc(3)).unwrap()).exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Two stores on one folder (two processes): what one takes out under the other, or adds while
/// the other makes room, is taken as it is.
#[test]
fn two_stores_on_one_folder_take_each_others_changes() {
    let dir = folder("two");
    let a = ReadingStore::new(&dir).with_max_bytes(40_000);
    let b = ReadingStore::new(&dir).with_max_bytes(40_000);
    a.put(&doc(1), 1, &key(1), &value(4000)).unwrap();
    b.put(&doc(2), 1, &key(2), &value(4000)).unwrap();
    // `a` counted the folder before `b` cleared it.
    b.clear(None);
    for n in 0..30u8 {
        let kept = a.put(&doc(3 + n % 3), 1, &key(10 + n), &value(4000)).unwrap();
        assert!(matches!(kept, Kept::Yes), "{n}: {kept:?}");
        let kept = b.put(&doc(7 + n % 3), 1, &key(100 + n), &value(4000)).unwrap();
        assert!(matches!(kept, Kept::Yes), "{n}: {kept:?}");
    }
    let total: u64 = a.status().iter().map(|b| b.bytes).sum();
    assert!(total <= 2 * 40_000, "{total}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A document opened again (another session, the same bytes) has the same content keys on disk;
/// an edit to one page changes that page's and no other's.
#[test]
fn a_document_opened_again_has_the_same_keys_on_disk() {
    let bytes = crate::chess::tests::book_of_boards(3);
    let (s, id) = crate::chess::tests::open(bytes.clone());
    let (mut again, other) = crate::chess::tests::open(bytes);
    let pages = [0, 1, 2, 3];
    let first = s.chess_disk_keys(id, &pages);
    assert!(first[..3].iter().all(Option::is_some) && first[3].is_none(), "{first:?}");
    assert_eq!(again.chess_disk_keys(other, &pages), first);
    assert_eq!(HashSet::<_>::from_iter(first.iter().flatten()).len(), 3, "the pages draw differently");
    again.apply(other, crate::Edit::RotatePages { pages: vec![1], degrees: 90 }).unwrap();
    let turned = again.chess_disk_keys(other, &pages);
    assert_eq!((turned[0], turned[2]), (first[0], first[2]));
    assert_ne!(turned[1], first[1]);
    assert_eq!(s.chess_disk_keys(crate::DocId(u64::MAX), &[0]), [None]);
}

/// The keys as this build makes them: the same inputs, the same key; any input changed, another.
/// The recipe is fixed (a frozen key): changing it leaves every kept reading behind, which only a
/// raised [`VERSION`] or [`SCAN_VERSION`] should do.
#[test]
fn the_keys_follow_their_inputs_and_their_recipe_is_fixed() {
    use crate::chess::{ChessScanSettings, ModelKey};
    let settings = ChessScanSettings::default().sanitized();
    let content = [7u8; 32];
    let model = ModelKey::Folder(PathBuf::from("models"), [3; 32]);
    let base = scan_key_of(&content, &settings, Some(&model)).unwrap();
    assert_eq!(
        scan_key_of(&content, &settings, Some(&ModelKey::Folder(PathBuf::from("elsewhere"), [3; 32]))),
        Some(base),
        "the folder is not the model"
    );
    let others = [
        scan_key_of(&[8; 32], &settings, Some(&model)),
        scan_key_of(&content, &settings, Some(&ModelKey::Folder(PathBuf::from("models"), [4; 32]))),
        scan_key_of(&content, &settings, Some(&ModelKey::Missing)),
        scan_key_of(&content, &settings, None),
        scan_key_of(&content, &ChessScanSettings { dpi: 300.0, ..settings.clone() }, Some(&model)),
        scan_key_of(&content, &ChessScanSettings { max_read_dpi: 400.0, ..settings.clone() }, Some(&model)),
        scan_key_of(&content, &ChessScanSettings { max_diagrams: 3, ..settings.clone() }, Some(&model)),
        scan_key_of(&content, &ChessScanSettings { allow_unverified_fonts: true, ..settings.clone() }, Some(&model)),
        scan_key_of(&content, &ChessScanSettings { order: pdfcraft_chessvision::order::ReadingOrder::Row, ..settings.clone() }, Some(&model)),
    ];
    let mut seen = HashSet::from([base]);
    for k in others {
        assert!(seen.insert(k.unwrap()), "{k:?}");
    }
    // Finding only does not depend on how a page is read.
    assert_eq!(scan_key_of(&content, &ChessScanSettings { max_read_dpi: 400.0, ..settings.clone() }, None), scan_key_of(&content, &settings, None));
    assert_eq!(scan_key_of(&content, &settings, Some(&ModelKey::Supplied)), None, "a caller's classifier is never kept");
    let rect = |x: f32| pdfcraft_chessvision::coords::ViewRect { x0: x, y0: 1.0, x1: 50.0, y1: 60.0 };
    let alone = alone_key(&content, 300.0, &[rect(0.0)], &[5; 32], None);
    for other in [
        alone_key(&content, 300.0, &[rect(0.5)], &[5; 32], None),
        alone_key(&content, 300.0, &[], &[5; 32], None),
        alone_key(&content, 301.0, &[rect(0.0)], &[5; 32], None),
        alone_key(&content, 300.0, &[rect(0.0)], &[6; 32], None),
        alone_key(&content, 300.0, &[rect(0.0)], &[5; 32], Some(&[0; 32])),
    ] {
        assert!(seen.insert(other), "{other:?}");
    }
    // Frozen: the recipe of version 1.
    assert_eq!(base.hex(), FROZEN_SCAN_KEY);
    assert_eq!(alone.hex(), FROZEN_ALONE_KEY);
}

const FROZEN_SCAN_KEY: &str = "4dcebc6682906351b527fb922f02fee819537e7ee5c747c9809c2c60087dd1e6";
const FROZEN_ALONE_KEY: &str = "e4830164d92b2708b41e82a8eb62017f826c999b64d6c2deb71ae2446af4eb42";

/// An entry of version 1 as this build wrote it (a page of three diagrams: a font's, read; a
/// vector board's and an image's, no model), frozen: every later build reads it, or raises
/// [`VERSION`].
#[test]
fn an_entry_of_version_1_reads() {
    let bytes = include_bytes!("../../../tests/compat/reading_v1.page");
    let key = EntryKey(bytes[MAGIC.len() + 4..MAGIC.len() + 36].try_into().unwrap());
    let json = decode(bytes, &key).unwrap();
    let page: ChessPage = serde_json::from_slice(&json).unwrap();
    // Read as it was written: nothing lost, nothing added.
    assert_eq!(String::from_utf8(serde_json::to_vec(&page).unwrap()).unwrap(), String::from_utf8(json).unwrap());
    assert_eq!(page.diagrams.len(), 3);
    let readings = page.readings.as_ref().unwrap();
    assert!(readings[0].position().unwrap().fen.starts_with("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w"));
    assert_eq!(readings[1].outcome, Err(pdfcraft_chessvision::board::Unreadable::NoModel));
    // In a store, it is found under its key.
    let dir = folder("v1");
    let at = entry(&dir, &doc(1), &key);
    std::fs::create_dir_all(at.parent().unwrap()).unwrap();
    std::fs::write(&at, bytes).unwrap();
    assert_eq!(ReadingStore::new(&dir).get::<ChessPage>(&doc(1), &key), Found::Hit(page));
    let _ = std::fs::remove_dir_all(&dir);
}

/// How `tests/compat/reading_v1.page` was made, once: never again, unless [`VERSION`] is raised.
#[test]
#[ignore = "writes the frozen entry; run once when VERSION is raised"]
fn freeze_an_entry_of_version_1() {
    let bytes = encode(&scanned(), &key(1)).unwrap();
    std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/compat/reading_v1.page"), bytes).unwrap();
}

/// The plan's measure (A9-N5a): what a real book's page readings take, per page, as JSON and as
/// kept. `PDFCRAFT_READING_MEASURE` names the PDF, `PDFCRAFT_READING_PAGES` the pages (0-based,
/// `first-last`), `PDFCRAFT_CHESS_TEST_MODELS` the models folder. A book of the user's: run in
/// release with `--ignored --nocapture`, never in the gate.
#[test]
#[ignore = "a measure on a book of the user's: PDFCRAFT_READING_MEASURE=<pdf>"]
fn measure_a_books_page_readings() {
    let Some(path) = std::env::var_os("PDFCRAFT_READING_MEASURE") else { return };
    let bytes = std::fs::read(&path).unwrap();
    let mut s = crate::Session::new();
    let id = s.open("book.pdf", None, Arc::new(bytes), None).unwrap();
    let range = std::env::var("PDFCRAFT_READING_PAGES").unwrap_or_default();
    let pages: Vec<usize> = match range.split_once('-') {
        Some((a, b)) => (a.parse().unwrap()..=b.parse().unwrap()).collect(),
        None => Vec::new(),
    };
    let settings =
        crate::chess::ChessScanSettings { models_dir: std::env::var_os("PDFCRAFT_CHESS_TEST_MODELS").map(PathBuf::from), ..Default::default() };
    let started = std::time::Instant::now();
    let read = s.chess_scan(id, &pages, &settings, true).unwrap();
    let took = started.elapsed();
    let json: usize = read.iter().map(|p| serde_json::to_vec(&**p).unwrap().len()).sum();
    let kept: usize = read.iter().map(|p| encode(&**p, &key(0)).unwrap().len()).sum();
    let n = read.len().max(1);
    println!(
        "MEASURE pages {} diagrams {} read in {:.1} s; JSON {} B ({} B a page); kept {} B ({} B a page)",
        read.len(),
        read.iter().map(|p| p.diagrams.len()).sum::<usize>(),
        took.as_secs_f64(),
        json,
        json / n,
        kept,
        kept / n
    );
}
