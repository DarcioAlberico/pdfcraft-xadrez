use std::sync::Arc;

use super::*;
use crate::{DocId, Edit, Session};

const KINGS: &str = "4k3/8/8/8/8/8/8/4K3 w - - 0 1";
const QUEEN: &str = "4k3/8/8/8/8/8/8/3QK3 w - - 0 1";

fn keys() -> DocKeys {
    DocKeys { id0: None, path: Some("C:/books/Livro.pdf".into()), name: Some("Livro.pdf".into()), size: Some(1234) }
}

fn new(page: usize, fp: &str, rect: [f32; 4], fen: &str) -> NewCorrection {
    NewCorrection { page, page_fp: fp.into(), rect, orig: None, fen: fen.into(), kind: Kind::Corrected }
}

/// A PDF of `n` pages, each with its own content stream.
fn pages_pdf(n: usize) -> Vec<u8> {
    let mut objs = vec!["<< /Type /Catalog /Pages 2 0 R >>".to_owned()];
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 3 + 2 * i)).collect();
    objs.push(format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")));
    for i in 0..n {
        objs.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /Contents {} 0 R >>", 4 + 2 * i));
        let content = format!("0 0 1 rg {} 100 50 50 re f", 10 * (i + 1));
        objs.push(format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()));
    }
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
    out
}

fn open(bytes: Vec<u8>) -> (Session, DocId) {
    let mut s = Session::new();
    let id = s.open("book.pdf", None, Arc::new(bytes), None).unwrap();
    (s, id)
}

/// A folder of its own under the temp folder, empty.
fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcraft-corrections-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn a_file_round_trips_and_bad_or_newer_files_are_told_apart() {
    let mut f = CorrectionFile::new(DocRecord::new(&keys(), 10));
    let uid = f.set(new(3, "fnv:a", [0.1, 0.2, 0.5, 0.6], KINGS), 100).unwrap();
    let bytes = f.to_bytes().unwrap();
    assert_eq!(CorrectionFile::read(&bytes), Read::File(f.clone()));
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains(&uid) && !text.contains("C:/books"), "the path is kept only as a hash: {text}");
    assert!(matches!(CorrectionFile::read(b"{not json"), Read::Bad(_)));
    assert!(matches!(CorrectionFile::read(br#"{"format":"other","version":1}"#), Read::Bad(_)));
    assert_eq!(CorrectionFile::read(br#"{"format":"pdfcraft-chess-corrections","version":7,"whatever":[]}"#), Read::Newer(7, None));
    let bad_rect = text.replace("[0.1,0.2,0.5,0.6]", "[0.5,0.2,0.1,0.6]");
    assert!(matches!(CorrectionFile::read(bad_rect.as_bytes()), Read::Bad(_)), "corners out of order");
    assert!(matches!(CorrectionFile::read(&vec![b' '; MAX_FILE_BYTES + 1]), Read::Bad(_)));
}

#[test]
fn the_same_diagram_gets_a_new_revision_and_another_a_new_entry() {
    let mut f = CorrectionFile::new(DocRecord::default());
    let a = f.set(new(3, "fnv:a", [0.1, 0.2, 0.5, 0.6], KINGS), 1).unwrap();
    // The box found again a little off: the same diagram.
    let b = f.set(new(3, "fnv:a", [0.11, 0.21, 0.51, 0.61], QUEEN), 2).unwrap();
    assert_eq!(a, b);
    assert_eq!((f.entries.len(), f.entries[0].rev, f.entries[0].fen.as_str()), (1, 2, QUEEN));
    // Another diagram on the page, and the same box on another page.
    let c = f.set(new(3, "fnv:a", [0.1, 0.65, 0.5, 0.95], KINGS), 3).unwrap();
    let d = f.set(new(4, "fnv:b", [0.1, 0.2, 0.5, 0.6], KINGS), 3).unwrap();
    assert!(c != a && d != a && c != d);
    assert_eq!(f.live().count(), 3);
    assert!(matches!(f.set(new(3, "fnv:a", [0.1, 0.2, 0.5, 0.6], "not a fen"), 4), Err(CorrectionError::BadFen(_))));
    assert_eq!(f.set(new(3, "fnv:a", [0.5, 0.2, 0.1, 0.6], KINGS), 4), Err(CorrectionError::BadRect));
}

#[test]
fn merges_take_the_higher_revision_and_never_bring_a_deletion_back() {
    let mut mine = CorrectionFile::new(DocRecord::default());
    let uid = mine.set(new(0, "fnv:a", [0.1, 0.1, 0.4, 0.4], KINGS), 1).unwrap();
    let mut theirs = mine.clone();
    // Here it is deleted (rev 2); there it was changed (rev 2), then changed again (rev 3).
    assert!(mine.delete(&uid, 2));
    assert!(!mine.delete(&uid, 2), "already deleted");
    theirs.set(new(0, "fnv:a", [0.1, 0.1, 0.4, 0.4], QUEEN), 2).unwrap();
    // An equal revision keeps the copy merged into.
    let mut tie = theirs.clone();
    tie.merge(&mine);
    assert_eq!(tie.live().count(), 1, "a tie keeps this copy's entry");
    let mut deleted = mine.clone();
    deleted.merge(&theirs);
    assert!(deleted.live().next().is_none(), "and that one's tombstone");
    theirs.set(new(0, "fnv:a", [0.1, 0.1, 0.4, 0.4], KINGS), 3).unwrap();
    mine.merge(&theirs);
    assert_eq!(mine.live().map(|e| (e.rev, e.fen.as_str())).collect::<Vec<_>>(), vec![(3, KINGS)], "rev 3 beats the tombstone");
    // A tombstone with the higher revision stays deleted through a merge with an older copy.
    let mut gone = mine.clone();
    gone.delete(&uid, 4);
    gone.merge(&theirs);
    assert!(gone.live().next().is_none());
    // An entry only one copy has is taken.
    theirs.set(new(5, "fnv:z", [0.2, 0.2, 0.6, 0.6], KINGS), 5).unwrap();
    gone.merge(&theirs);
    assert_eq!(gone.live().count(), 1);
}

#[test]
fn diagrams_are_found_by_fingerprint_and_overlap() {
    let mut f = CorrectionFile::new(DocRecord::default());
    f.set(new(1, "fnv:b", [0.1, 0.2, 0.5, 0.6], KINGS), 1).unwrap();
    f.set(new(2, "fnv:c", [0.1, 0.2, 0.5, 0.6], QUEEN), 1).unwrap();
    let fps = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    // Unchanged.
    let now = fps(&["fnv:a", "fnv:b", "fnv:c"]);
    assert_eq!(f.find(&now, 1, [0.12, 0.22, 0.52, 0.62]).map(|e| e.fen.as_str()), Some(KINGS));
    assert_eq!(f.find(&now, 1, [0.4, 0.5, 0.9, 0.9]), None, "too little overlap");
    // A page inserted at the start: both moved one on.
    let now = fps(&["fnv:new", "fnv:a", "fnv:b", "fnv:c"]);
    let at: Vec<Located> = f.locate(&now).into_iter().map(|(l, _)| l).collect();
    assert_eq!(at, vec![Located::Moved(2), Located::Moved(3)]);
    assert_eq!(f.find(&now, 3, [0.1, 0.2, 0.5, 0.6]).map(|e| e.fen.as_str()), Some(QUEEN));
    // Page 1 redrawn (its content changed) and the last page removed.
    let now = fps(&["fnv:a", "fnv:b2"]);
    let at: Vec<Located> = f.locate(&now).into_iter().map(|(l, _)| l).collect();
    assert_eq!(at, vec![Located::Changed(1), Located::Orphan]);
}

#[test]
fn a_document_is_known_by_any_of_its_keys() {
    let record = DocRecord::new(&keys(), 10);
    assert!(record.matches(&DocKeys { path: Some("C:/books/Livro.pdf".into()), ..DocKeys::default() }));
    assert!(record.matches(&DocKeys { name: Some("Livro.pdf".into()), size: Some(1234), ..DocKeys::default() }));
    assert!(!record.matches(&DocKeys { name: Some("Livro.pdf".into()), size: Some(99), ..DocKeys::default() }));
    let with_id = DocRecord::new(&DocKeys { id0: Some([7; 16]), ..keys() }, 10);
    assert!(with_id.matches(&DocKeys { id0: Some([7; 16]), ..DocKeys::default() }));
    assert_eq!(file_name(&DocKeys::default()), None);
    assert_ne!(file_name(&keys()), file_name(&DocKeys { id0: Some([7; 16]), ..keys() }), "the identifier names the file first");
}

#[test]
fn normalised_boxes_and_overlap() {
    assert_eq!(normalize([100.0, 200.0, 300.0, 400.0], [0.0, 0.0, 400.0, 800.0]), Some([0.25, 0.25, 0.75, 0.5]));
    assert_eq!(normalize([100.0, 200.0, 300.0, 400.0], [0.0, 0.0, 0.0, 800.0]), None);
    assert_eq!(normalize([f32::NAN, 0.0, 1.0, 1.0], [0.0, 0.0, 1.0, 1.0]), None);
    assert!((iou([0.0, 0.0, 1.0, 1.0], [0.0, 0.0, 1.0, 1.0]) - 1.0).abs() < 1e-6);
    assert_eq!(iou([0.0, 0.0, 0.2, 0.2], [0.5, 0.5, 0.9, 0.9]), 0.0);
}

/// Page fingerprints follow the pages: an inserted page shifts them, saving keeps them.
#[test]
fn page_fingerprints_survive_saving_and_follow_the_pages() {
    let (mut s, id) = open(pages_pdf(3));
    let before = s.get(id).unwrap().page_fingerprints();
    assert_eq!(before.len(), 3);
    assert!(before[0] != before[1] && before[1] != before[2] && before.iter().all(|f| f.starts_with("fnv:")));
    let saved = s.save_bytes(id).unwrap();
    let (s2, id2) = open(saved.to_vec());
    assert_eq!(s2.get(id2).unwrap().page_fingerprints(), before, "saving keeps them");
    s.apply(id, Edit::InsertBlankPage { at: 0, width: 600.0, height: 800.0 }).unwrap();
    let after = s.get(id).unwrap().page_fingerprints();
    assert_eq!(after.len(), 4);
    assert_eq!(after[1..], before[..], "every page moved one on");
}

#[test]
fn the_folder_store_writes_finds_and_keeps_two_writers_changes() {
    let dir = temp_dir("store");
    let store = DirStore::new(&dir);
    let (uid, loaded) = store.update(&keys(), 10, |f| f.set(new(1, "fnv:b", [0.1, 0.2, 0.5, 0.6], KINGS), 1)).unwrap();
    assert!(dir.join(&loaded.name).is_file());
    assert!(!dir.join("corrections.lock").exists(), "the lock is let go");
    let again = store.load(&keys(), 10).unwrap();
    assert_eq!(again.file.live().map(|e| e.uid.clone()).collect::<Vec<_>>(), vec![uid]);
    // Saving gave the document an identifier and changed its size: found by its path still.
    let saved = DocKeys { id0: Some([9; 16]), size: Some(5678), ..keys() };
    assert_eq!(store.load(&saved, 10).unwrap().name, loaded.name);
    // Four writers at once: each change is kept.
    let threads: Vec<_> = (0..4u8)
        .map(|i| {
            let dir = dir.clone();
            std::thread::spawn(move || {
                let y = 0.1 * f32::from(i);
                DirStore::new(dir).update(&keys(), 10, |f| f.set(new(5, "fnv:e", [0.0, y, 0.09, y + 0.09], KINGS), 2)).map(|(u, _)| u)
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap().unwrap();
    }
    assert_eq!(store.load(&keys(), 10).unwrap().file.live().count(), 5);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unreadable_files_are_set_aside_and_newer_ones_never_written() {
    let dir = temp_dir("bad");
    std::fs::create_dir_all(&dir).unwrap();
    let name = file_name(&keys()).unwrap();
    std::fs::write(dir.join(&name), b"{broken").unwrap();
    let store = DirStore::new(&dir);
    let loaded = store.load(&keys(), 10).unwrap();
    assert_eq!(loaded.set_aside.as_ref().map(|(n, _)| n.clone()), Some(format!("{name}.bad")));
    assert!(dir.join(format!("{name}.bad")).is_file() && loaded.file.entries.is_empty(), "set aside, never deleted");
    let newer = br#"{"format":"pdfcraft-chess-corrections","version":2,"doc":{},"entries":[]}"#;
    std::fs::write(dir.join(&name), newer).unwrap();
    assert!(store.load(&keys(), 10).unwrap().read_only);
    // A newer file whose entries this build can make out shows them, read only.
    let mut kept = CorrectionFile::new(DocRecord::new(&keys(), 10));
    kept.set(new(1, "fnv:b", [0.1, 0.2, 0.5, 0.6], KINGS), 1).unwrap();
    let text = String::from_utf8(kept.to_bytes().unwrap()).unwrap().replace("\"version\":1", "\"version\":3,\"later\":true");
    std::fs::write(dir.join(&name), &text).unwrap();
    let shown = store.load(&keys(), 10).unwrap();
    assert_eq!((shown.read_only, shown.file.version, shown.file.live().count()), (true, 3, 1));
    std::fs::write(dir.join(&name), newer).unwrap();
    let refused = store.update(&keys(), 10, |f| f.set(new(1, "fnv:b", [0.1, 0.2, 0.5, 0.6], KINGS), 1));
    assert!(matches!(refused, Err(StoreError::Correction(CorrectionError::Newer(2)))));
    assert_eq!(std::fs::read(dir.join(&name)).unwrap(), newer.to_vec(), "left as it was");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The web build's store: the same finding, merging and setting aside, within its own budget.
#[test]
fn the_memory_store_keeps_files_within_its_budget() {
    let store = MemStore::new(4096);
    let (uid, loaded) = store.update(&keys(), 10, |f| f.set(new(1, "fnv:b", [0.1, 0.2, 0.5, 0.6], KINGS), 1)).unwrap();
    assert_eq!(store.load(&keys(), 10).unwrap().file.live().map(|e| e.uid.clone()).collect::<Vec<_>>(), vec![uid]);
    // Saved and loaded again, found by the path after a save gave the document an identifier.
    let again = MemStore::with_files(store.files(), 4096);
    let saved = DocKeys { id0: Some([9; 16]), size: Some(5678), ..keys() };
    assert_eq!(again.load(&saved, 10).unwrap().name, loaded.name);
    // Past the budget: refused, the file left as it was.
    let small = MemStore::with_files(store.files(), 600);
    let before = small.files();
    let many = small.update(&keys(), 10, |f| {
        for i in 0..8u8 {
            let y = 0.1 * f32::from(i);
            f.set(new(2, "fnv:c", [0.0, y, 0.09, y + 0.09], QUEEN), 2)?;
        }
        Ok(())
    });
    assert!(matches!(many, Err(StoreError::Correction(CorrectionError::OverBudget(600)))), "{many:?}");
    assert_eq!(small.files(), before);
    assert!(MemStore::with_files(store.files(), 10).files().is_empty(), "a file past the budget is not taken in");
    // A file that can't be read is set aside, never dropped.
    let name = file_name(&keys()).unwrap();
    let broken = MemStore::with_files([(name.clone(), b"{broken".to_vec())], 4096);
    let l = broken.load(&keys(), 10).unwrap();
    assert_eq!(l.set_aside.map(|(n, _)| n), Some(format!("{name}.bad")));
    assert_eq!(broken.files(), vec![(format!("{name}.bad"), b"{broken".to_vec())]);
    assert_eq!(MemStore::new(4096).load(&DocKeys::default(), 1).map(|_| ()).map_err(|e| e.to_string()), Err(StoreError::NoKeys.to_string()));
}
