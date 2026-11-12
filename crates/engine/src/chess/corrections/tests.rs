use std::sync::Arc;

use super::*;
use crate::chess::store::{DocStore, StoreBackend};
use crate::{DocId, Edit, Session};

const KINGS: &str = "4k3/8/8/8/8/8/8/4K3 w - - 0 1";
const QUEEN: &str = "4k3/8/8/8/8/8/8/3QK3 w - - 0 1";

fn keys() -> DocKeys {
    DocKeys { id0: None, path: Some("C:/books/Livro.pdf".into()), name: Some("Livro.pdf".into()), size: Some(1234) }
}

fn new(page: usize, rect: [f32; 4], fen: &str) -> NewCorrection {
    NewCorrection { page, rect, orig: None, fen: fen.into(), kind: Kind::Corrected }
}

/// Page fingerprints, as given.
fn fps(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| (*s).to_owned()).collect()
}

/// The fingerprints of a document of ten pages (`fnv:p0`…) whose page `page` has `fp`.
fn book_with(page: usize, fp: &str) -> Vec<String> {
    (0..10).map(|i| if i == page { fp.to_owned() } else { format!("fnv:p{i}") }).collect()
}

/// [`CorrectionFile::set`] for a diagram of page `page` whose fingerprint is `fp`, in a document
/// of ten pages ([`book_with`]): the tests about the file, not about where pages went.
trait Put {
    fn put(&mut self, page: usize, fp: &str, rect: [f32; 4], fen: &str, at: u64) -> Result<String, CorrectionError>;
}

impl Put for CorrectionFile {
    fn put(&mut self, page: usize, fp: &str, rect: [f32; 4], fen: &str, at: u64) -> Result<String, CorrectionError> {
        self.set(new(page, rect, fen), at, &book_with(page, fp))
    }
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

/// The name of the document's corrections file since plan A9-G3a: `<hash>.corrections.json`.
fn own_name(keys: &DocKeys) -> String {
    crate::chess::store::file_name(keys, "corrections").unwrap()
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
    let uid = f.put(3, "fnv:a", [0.1, 0.2, 0.5, 0.6], KINGS, 100).unwrap();
    let bytes = f.to_bytes().unwrap();
    assert_eq!(CorrectionFile::read(&bytes), Read::File(f.clone()));
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains(&uid) && !text.contains("C:/books"), "the path is kept only as a hash: {text}");
    assert!(matches!(CorrectionFile::read(b"{not json"), Read::Bad(_)));
    assert!(matches!(CorrectionFile::read(br#"{"format":"other","version":1}"#), Read::Bad(_)));
    assert_eq!(CorrectionFile::read(br#"{"format":"pdfcraft-chess-corrections","version":7,"whatever":[]}"#), Read::Newer(7, None));
    let bad_rect = text.replace("[0.1,0.2,0.5,0.6]", "[0.5,0.2,0.1,0.6]");
    let Parsed::File { file, skipped } = CorrectionFile::parse(bad_rect.as_bytes()) else { panic!("one bad entry is no bad file") };
    assert_eq!((file.entries.len(), skipped), (0, 1), "corners out of order: the entry is left out");
    assert!(matches!(CorrectionFile::read(&vec![b' '; MAX_FILE_BYTES + 1]), Read::Bad(_)));
}

#[test]
fn the_same_diagram_gets_a_new_revision_and_another_a_new_entry() {
    let mut f = CorrectionFile::new(DocRecord::default());
    let a = f.put(3, "fnv:a", [0.1, 0.2, 0.5, 0.6], KINGS, 1).unwrap();
    // The box found again a little off: the same diagram.
    let b = f.put(3, "fnv:a", [0.11, 0.21, 0.51, 0.61], QUEEN, 2).unwrap();
    assert_eq!(a, b);
    assert_eq!((f.entries.len(), f.entries[0].rev, f.entries[0].fen.as_str()), (1, 2, QUEEN));
    // Another diagram on the page, and the same box on another page.
    let c = f.put(3, "fnv:a", [0.1, 0.65, 0.5, 0.95], KINGS, 3).unwrap();
    let d = f.put(4, "fnv:b", [0.1, 0.2, 0.5, 0.6], KINGS, 3).unwrap();
    assert!(c != a && d != a && c != d);
    assert_eq!(f.live().count(), 3);
    assert!(matches!(f.put(3, "fnv:a", [0.1, 0.2, 0.5, 0.6], "not a fen", 4), Err(CorrectionError::BadFen(_))));
    assert_eq!(f.put(3, "fnv:a", [0.5, 0.2, 0.1, 0.6], KINGS, 4), Err(CorrectionError::BadRect));
}

#[test]
fn merges_take_the_higher_revision_and_never_bring_a_deletion_back() {
    let mut mine = CorrectionFile::new(DocRecord::default());
    let uid = mine.put(0, "fnv:a", [0.1, 0.1, 0.4, 0.4], KINGS, 1).unwrap();
    let mut theirs = mine.clone();
    // Here it is deleted (rev 2); there it was changed (rev 2), then changed again (rev 3).
    assert!(mine.delete(&uid, 2));
    assert!(!mine.delete(&uid, 2), "already deleted");
    theirs.put(0, "fnv:a", [0.1, 0.1, 0.4, 0.4], QUEEN, 2).unwrap();
    // An equal revision keeps the copy merged into.
    let mut tie = theirs.clone();
    tie.merge(&mine);
    assert_eq!(tie.live().count(), 1, "a tie keeps this copy's entry");
    let mut deleted = mine.clone();
    deleted.merge(&theirs);
    assert!(deleted.live().next().is_none(), "and that one's tombstone");
    theirs.put(0, "fnv:a", [0.1, 0.1, 0.4, 0.4], KINGS, 3).unwrap();
    mine.merge(&theirs);
    assert_eq!(mine.live().map(|e| (e.rev, e.fen.as_str())).collect::<Vec<_>>(), vec![(3, KINGS)], "rev 3 beats the tombstone");
    // A tombstone with the higher revision stays deleted through a merge with an older copy.
    let mut gone = mine.clone();
    gone.delete(&uid, 4);
    gone.merge(&theirs);
    assert!(gone.live().next().is_none());
    // An entry only one copy has is taken.
    theirs.put(5, "fnv:z", [0.2, 0.2, 0.6, 0.6], KINGS, 5).unwrap();
    gone.merge(&theirs);
    assert_eq!(gone.live().count(), 1);
}

#[test]
fn diagrams_are_found_by_fingerprint_and_overlap() {
    let mut f = CorrectionFile::new(DocRecord::default());
    let book = fps(&["fnv:a", "fnv:b", "fnv:c"]);
    f.set(new(1, [0.1, 0.2, 0.5, 0.6], KINGS), 1, &book).unwrap();
    f.set(new(2, [0.1, 0.2, 0.5, 0.6], QUEEN), 1, &book).unwrap();
    // The neighbours are recorded, none past the last page.
    let recorded: Vec<(Option<&str>, Option<&str>, usize)> =
        f.entries.iter().map(|e| (e.prev_fp.as_deref(), e.next_fp.as_deref(), e.page_count)).collect();
    assert_eq!(recorded, [(Some("fnv:a"), Some("fnv:c"), 3), (Some("fnv:b"), None, 3)]);
    // Unchanged.
    assert_eq!(f.find(&book, 1, [0.12, 0.22, 0.52, 0.62]).map(|e| e.fen.as_str()), Some(KINGS));
    assert_eq!(f.find(&book, 1, [0.4, 0.5, 0.9, 0.9]), None, "too little overlap");
    // A page inserted at the start: both moved one on.
    let now = fps(&["fnv:new", "fnv:a", "fnv:b", "fnv:c"]);
    let at: Vec<Located> = f.locate(&now).into_iter().map(|(l, _)| l).collect();
    assert_eq!(at, vec![Located::Moved(2), Located::Moved(3)]);
    assert_eq!(f.find(&now, 3, [0.1, 0.2, 0.5, 0.6]).map(|e| e.fen.as_str()), Some(QUEEN));
    // Page 1 redrawn (its content changed, its neighbour before still in place) and the last
    // page removed.
    let now = fps(&["fnv:a", "fnv:b2"]);
    let at: Vec<Located> = f.locate(&now).into_iter().map(|(l, _)| l).collect();
    assert_eq!(at, vec![Located::Changed(1), Located::Orphan]);
}

/// A9-G2a (A4-01): the correction of a page that was deleted used to pass, by the page's index,
/// to the diagram in the same box on the page that took its place. Its neighbour after it is now
/// where it was: another page is at its index, and it is an orphan, never applied.
#[test]
fn a_deleted_pages_correction_never_passes_to_the_next_pages_diagram() {
    let book = fps(&["fnv:p0", "fnv:p1", "fnv:p2", "fnv:p3"]);
    let rect = [0.1, 0.6, 0.5, 0.9];
    let mut f = CorrectionFile::new(DocRecord::default());
    f.set(new(1, rect, KINGS), 1, &book).unwrap();
    // Page 1 deleted: old page 2 is page 1 now, its diagram in the same box, read as QUEEN.
    let deleted = fps(&["fnv:p0", "fnv:p2", "fnv:p3"]);
    assert_eq!(f.locate(&deleted).into_iter().map(|(l, _)| l).collect::<Vec<_>>(), [Located::Orphan]);
    assert_eq!(f.find(&deleted, 1, rect), None);
    let next = read_page_at(1, &[[60.0, 480.0, 300.0, 720.0]], &[Some(QUEEN)]);
    let out = apply(std::slice::from_ref(&next), &f, &deleted);
    assert_eq!(fen_of(&out.pages[0], 0).as_deref(), Some(QUEEN), "the reading stays");
    assert!(out.applied.is_empty() && out.unmatched.is_empty(), "{:?} {:?}", out.applied, out.unmatched);
    // The first page is told the same way, by its one neighbour.
    let mut first = CorrectionFile::new(DocRecord::default());
    first.set(new(0, rect, KINGS), 1, &book).unwrap();
    assert_eq!(first.locate(&fps(&["fnv:p1", "fnv:p2", "fnv:p3"]))[0].0, Located::Orphan);
}

/// How a page whose fingerprint is gone is told apart: changed in place, or taken by another.
#[test]
fn a_changed_page_is_told_from_one_that_took_its_place() {
    let book = fps(&["fnv:p0", "fnv:p1", "fnv:p2"]);
    let mut f = CorrectionFile::new(DocRecord::default());
    f.set(new(1, [0.1, 0.6, 0.5, 0.9], KINGS), 1, &book).unwrap();
    let at = |now: &[&str]| f.locate(&fps(now)).first().map(|(l, _)| *l);
    assert_eq!(at(&["fnv:p0", "fnv:x", "fnv:p2"]), Some(Located::Changed(1)), "edited: both neighbours in place");
    assert_eq!(at(&["fnv:p0", "fnv:x", "fnv:y"]), Some(Located::Changed(1)), "it and the next one edited: the one before tells");
    assert_eq!(at(&["fnv:p0", "fnv:new", "fnv:x", "fnv:p2"]), Some(Located::Orphan), "a page inserted before it: the next one moved");
    assert_eq!(at(&["fnv:x", "fnv:p2"]), Some(Located::Orphan), "the page before deleted and it edited: the next one moved");
    // An edit to every page (a header, a watermark, lichess labels): no neighbour left to tell,
    // so its index, while the page count is the one it recorded.
    assert_eq!(at(&["fnv:q0", "fnv:q1", "fnv:q2"]), Some(Located::Changed(1)));
    assert_eq!(at(&["fnv:q0", "fnv:q1", "fnv:q2", "fnv:q3"]), Some(Located::Orphan));
    // A one-page document has no neighbour: its page count tells.
    let mut one = CorrectionFile::new(DocRecord::default());
    one.set(new(0, [0.1, 0.6, 0.5, 0.9], KINGS), 1, &fps(&["fnv:only"])).unwrap();
    assert_eq!(one.locate(&fps(&["fnv:edited"]))[0].0, Located::Changed(0));
}

/// A9-G2a (R1-01): with corrections on pages 1 and 2 in the same box, deleting page 1 made its
/// correction (found by index) beat page 2's own (found by its page's fingerprint), first in the
/// file. The fingerprint wins over the index, whatever the order, with or without neighbours.
#[test]
fn a_moved_pages_own_correction_beats_a_deleted_pages() {
    let book = fps(&["fnv:p0", "fnv:p1", "fnv:p2", "fnv:p3"]);
    let rect = [0.1, 0.6, 0.5, 0.9];
    let deleted = fps(&["fnv:p0", "fnv:p2", "fnv:p3"]);
    let next = read_page_at(1, &[[60.0, 480.0, 300.0, 720.0]], &[Some(START_FEN)]);
    let mut f = CorrectionFile::new(DocRecord::default());
    let gone = f.set(new(1, rect, KINGS), 1, &book).unwrap();
    let own = f.set(new(2, rect, QUEEN), 1, &book).unwrap();
    let out = apply(std::slice::from_ref(&next), &f, &deleted);
    assert_eq!(fen_of(&out.pages[0], 0).as_deref(), Some(QUEEN));
    assert_eq!(out.applied.iter().map(|a| a.uid.as_str()).collect::<Vec<_>>(), [own.as_str()]);
    assert_eq!(f.find(&deleted, 1, rect).map(|e| e.uid.as_str()), Some(own.as_str()));
    // Entries of version 1 recorded no neighbours: the deleted page's is found by index, and loses
    // to the one found by its fingerprint, which the reply names.
    for e in &mut f.entries {
        (e.prev_fp, e.next_fp, e.page_count) = (None, None, 0);
    }
    for order in [false, true] {
        let mut legacy = f.clone();
        if order {
            legacy.entries.reverse();
        }
        assert!(legacy.locate(&deleted).iter().any(|(l, e)| e.uid == gone && *l == Located::Changed(1)), "found by its index");
        let out = apply(std::slice::from_ref(&next), &legacy, &deleted);
        assert_eq!(fen_of(&out.pages[0], 0).as_deref(), Some(QUEEN), "reversed: {order}");
        let unmatched: Vec<(&str, &str)> = out.unmatched.iter().map(|u| (u.uid.as_str(), u.reason.as_str())).collect();
        assert_eq!(unmatched, [(gone.as_str(), format!("the diagram already has correction {own}").as_str())]);
        assert_eq!(legacy.find(&deleted, 1, rect).map(|e| e.uid.as_str()), Some(own.as_str()));
    }
    // Between two found the same way, the later one.
    let mut two = CorrectionFile::new(DocRecord::default());
    two.set(new(1, rect, KINGS), 5, &book).unwrap();
    two.entries.push(Correction { uid: "older".into(), fen: QUEEN.into(), at: 1, ..two.entries[0].clone() });
    assert_eq!(two.find(&book, 1, rect).map(|e| e.fen.as_str()), Some(KINGS));
}

/// A9-G2a (A4-02): correcting again a diagram whose page moved (a page inserted before it) used to
/// add a second live correction, the old one still winning in the exports. It is the same
/// correction's next revision, recorded with its page as it is now.
#[test]
fn correcting_again_after_an_insertion_keeps_one_correction() {
    let book = fps(&["fnv:p0", "fnv:p1", "fnv:p2", "fnv:p3"]);
    let rect = [0.1, 0.6, 0.5, 0.9];
    let mut f = CorrectionFile::new(DocRecord::default());
    let uid = f.set(new(2, rect, KINGS), 1, &book).unwrap();
    let inserted = fps(&["fnv:new", "fnv:p0", "fnv:p1", "fnv:p2", "fnv:p3"]);
    let again = f.set(new(3, [0.11, 0.61, 0.51, 0.91], QUEEN), 2, &inserted).unwrap();
    assert_eq!(again, uid);
    assert_eq!(f.live().count(), 1);
    let e = &f.entries[0];
    assert_eq!(
        (e.page, e.page_fp.as_str(), e.prev_fp.as_deref(), e.next_fp.as_deref(), e.page_count, e.rev),
        (3, "fnv:p2", Some("fnv:p1"), Some("fnv:p3"), 5, 2)
    );
    let page = read_page_at(3, &[[60.0, 480.0, 300.0, 720.0]], &[Some(START_FEN)]);
    assert_eq!(fen_of(&apply(std::slice::from_ref(&page), &f, &inserted).pages[0], 0).as_deref(), Some(QUEEN));
    // A page that is not in the document is refused.
    assert_eq!(f.set(new(9, rect, KINGS), 3, &inserted), Err(CorrectionError::NoSuchPage(9)));
}

/// Entries of version 1 (no neighbours) still on their page record their neighbours when the file
/// is next changed; one whose page moved is left as it was.
#[test]
fn version_1_entries_take_their_neighbours_when_the_file_is_next_changed() {
    let book = fps(&["fnv:p0", "fnv:p1", "fnv:p2", "fnv:p3"]);
    let mut f = CorrectionFile::new(DocRecord::default());
    f.set(new(1, [0.1, 0.6, 0.5, 0.9], KINGS), 1, &book).unwrap();
    f.set(new(2, [0.1, 0.6, 0.5, 0.9], KINGS), 1, &book).unwrap();
    for e in &mut f.entries {
        (e.prev_fp, e.next_fp, e.page_count) = (None, None, 0);
    }
    f.entries[1].page_fp = "fnv:elsewhere".into();
    f.set(new(3, [0.1, 0.1, 0.5, 0.4], QUEEN), 2, &book).unwrap();
    let recorded: Vec<(Option<&str>, Option<&str>, usize, u64)> =
        f.entries.iter().map(|e| (e.prev_fp.as_deref(), e.next_fp.as_deref(), e.page_count, e.rev)).collect();
    assert_eq!(recorded, [(Some("fnv:p0"), Some("fnv:p2"), 4, 1), (None, None, 0, 1), (Some("fnv:p2"), None, 4, 1)]);
}

/// Version 2 keeps each fingerprint once and leaves out a reading that is the position; a file of
/// version 1 is read with every field, and written again as version 2.
#[test]
fn version_2_is_compact_and_version_1_is_read() {
    let book = fps(&["fnv:p0", "fnv:p1", "fnv:p2"]);
    let mut f = CorrectionFile::new(DocRecord::new(&keys(), 3));
    let verified =
        NewCorrection { orig: Some(Original { fen: Some(KINGS.into()), model: None }), kind: Kind::Verified, ..new(1, [0.1, 0.6, 0.5, 0.9], KINGS) };
    f.set(verified, 1, &book).unwrap();
    let corrected = NewCorrection { orig: Some(Original { fen: None, model: None }), ..new(1, [0.1, 0.1, 0.5, 0.4], QUEEN) };
    f.set(corrected, 1, &book).unwrap();
    f.set(new(2, [0.1, 0.6, 0.5, 0.9], QUEEN), 1, &book).unwrap();
    let bytes = f.to_bytes().unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(text.contains("\"version\":2") && text.contains("\"fps\":[\"fnv:p0\",\"fnv:p1\",\"fnv:p2\"]"), "{text}");
    assert_eq!(text.matches("fnv:p1").count(), 1, "each fingerprint once: {text}");
    assert_eq!(text.matches("\"orig\"").count(), 2, "left out when it is the position: {text}");
    assert!(!text.contains("deleted"), "{text}");
    assert_eq!(CorrectionFile::read(&bytes), Read::File(f.clone()));
    // An entry naming a place past the table is left out, the others kept.
    let past = text.replacen("\"fp\":[0,1,2]", "\"fp\":[0,1,9]", 1);
    let Parsed::File { file, skipped } = CorrectionFile::parse(past.as_bytes()) else { panic!("{past}") };
    assert!(past != text && (file.entries.len(), skipped) == (2, 1), "{past}");
    // Version 1, as an earlier build wrote it.
    let v1 = format!(
        r#"{{"format":"pdfcraft-chess-corrections","version":1,"doc":{{"pages":3}},"entries":[{{"uid":"u1","page":1,"page_fp":"fnv:p1","rect":[0.1,0.6,0.5,0.9],"orig":{{"fen":"{KINGS}","model":null}},"fen":"{QUEEN}","kind":"corrected","at":7,"rev":3,"deleted":false}},{{"uid":"u2","page":2,"page_fp":"fnv:p2","rect":[0.1,0.6,0.5,0.9],"fen":"{KINGS}","kind":"verified","at":8,"rev":1}}]}}"#
    );
    let Read::File(old) = CorrectionFile::read(v1.as_bytes()) else { panic!("version 1 not read") };
    let e = &old.entries[0];
    assert_eq!(
        (e.uid.as_str(), e.page, e.page_fp.as_str(), e.prev_fp.as_deref(), e.page_count, e.fen.as_str(), e.kind, e.at, e.rev),
        ("u1", 1, "fnv:p1", None, 0, QUEEN, Kind::Corrected, 7, 3)
    );
    assert_eq!(e.orig.as_ref().and_then(|o| o.fen.as_deref()), Some(KINGS));
    assert_eq!((old.version, old.entries[1].orig.clone()), (VERSION, None), "an unknown reading stays unknown");
    assert_eq!(CorrectionFile::read(&old.to_bytes().unwrap()), Read::File(old.clone()), "written again as version 2, nothing lost");
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

/// Two books with different identifiers are two documents even with the same name and size, or
/// the same path (another book saved over the file): a correction made in one is never shown in
/// the other (plan A9, G1; A4-03). And the stores match or refuse every pair of keys alike: a
/// record's rule is [`DocKeys::matches`], which the game collections use.
#[test]
fn another_identifier_is_another_book_in_every_store() {
    let dir = temp_dir("veto");
    let store = DirStore::new(&dir);
    let a = DocKeys { id0: Some([1; 16]), ..keys() };
    store.update(&a, 10, |f| f.put(0, "fnv:a", [0.1, 0.1, 0.4, 0.4], KINGS, 1)).unwrap();
    let same_name_and_size = DocKeys { id0: Some([2; 16]), path: Some("C:/outros/Livro.pdf".into()), ..keys() };
    assert_eq!(store.load(&same_name_and_size, 10).unwrap().file.live().count(), 0, "another book of the same name and size");
    let saved_over = DocKeys { id0: Some([3; 16]), ..keys() };
    assert_eq!(store.load(&saved_over, 10).unwrap().file.live().count(), 0, "another book saved over the file");
    assert_eq!(store.load(&a, 10).unwrap().file.live().count(), 1);
    assert_eq!(store.load(&DocKeys { id0: None, ..keys() }, 10).unwrap().file.live().count(), 1, "no identifier: the path");
    let _ = std::fs::remove_dir_all(&dir);
    // One rule: every pair of keys, as a record and as keys.
    let constant = Some(DocKeys::KNOWN_CONSTANT_ID);
    let variants: Vec<DocKeys> = [None, Some([1; 16]), Some([2; 16]), constant]
        .into_iter()
        .flat_map(|id0| {
            [
                DocKeys { id0, ..keys() },
                DocKeys { id0, path: Some("C:/outros/Livro.pdf".into()), ..keys() },
                DocKeys { id0, path: None, size: Some(1), ..keys() },
                DocKeys { id0, path: None, name: None, size: None },
            ]
        })
        .collect();
    for x in &variants {
        for y in &variants {
            assert_eq!(DocRecord::new(x, 1).matches(y), x.matches(y), "{x:?} / {y:?}");
        }
    }
}

/// Files made by extract, split or combine before plan A9 all have one identifier
/// ([`DocKeys::KNOWN_CONSTANT_ID`]); each is named by its path, and a file an earlier build
/// named by that identifier is still found by the path it records (plan A9, G1).
#[test]
fn the_identifier_every_new_file_used_to_get_names_no_book() {
    let dir = temp_dir("constant");
    let store = DirStore::new(&dir);
    let constant = Some(DocKeys::KNOWN_CONSTANT_ID);
    let book =
        |n: u64| DocKeys { id0: constant, path: Some(format!("C:/livros/cap{n}.pdf")), name: Some(format!("cap{n}.pdf")), size: Some(100 * n) };
    store.update(&book(1), 10, |f| f.put(0, "fnv:a", [0.1, 0.1, 0.4, 0.4], KINGS, 1)).unwrap();
    assert_eq!(store.load(&book(2), 10).unwrap().file.live().count(), 0, "another part with the same identifier");
    assert_eq!(store.load(&book(1), 10).unwrap().file.live().count(), 1);
    assert_eq!(DocRecord::new(&book(1), 1).id0, None, "the constant is not recorded");
    assert_ne!(file_name(&book(1)), file_name(&book(2)));
    let _ = std::fs::remove_dir_all(&dir);
    // A file an earlier build wrote for part 3: named by the constant, recording it.
    let dir = temp_dir("constant-legacy");
    std::fs::create_dir_all(&dir).unwrap();
    let legacy = format!("{}.json", hash_hex(&[b"id0", &DocKeys::KNOWN_CONSTANT_ID]));
    let mut record = DocRecord::new(&book(3), 10);
    record.id0 = Some(hex(&DocKeys::KNOWN_CONSTANT_ID));
    let mut file = CorrectionFile::new(record);
    file.put(0, "fnv:c", [0.1, 0.1, 0.4, 0.4], QUEEN, 1).unwrap();
    std::fs::write(dir.join(&legacy), file.to_bytes().unwrap()).unwrap();
    let store = DirStore::new(&dir);
    let found = store.load(&book(3), 10).unwrap();
    assert_eq!((found.name, found.file.live().count()), (own_name(&book(3)), 1), "found by the path it records, kept under its own name");
    assert!(dir.join(&legacy).is_file(), "the earlier build's file is never moved");
    assert_eq!(store.load(&book(2), 10).unwrap().file.live().count(), 0, "never by the identifier");
    let _ = std::fs::remove_dir_all(&dir);
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
    let (uid, loaded) = store.update(&keys(), 10, |f| f.put(1, "fnv:b", [0.1, 0.2, 0.5, 0.6], KINGS, 1)).unwrap();
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
                DirStore::new(dir).update(&keys(), 10, |f| f.put(5, "fnv:e", [0.0, y, 0.09, y + 0.09], KINGS, 2)).map(|(u, _)| u)
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap().unwrap();
    }
    assert_eq!(store.load(&keys(), 10).unwrap().file.live().count(), 5);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A9-G3a (A4-13): reading never renames. A file that can't be read is reported and left as it
/// is by a load, as a reading tool given only read access does; the next change sets it aside
/// before it writes, never deleting it. A newer version's file is read only.
#[test]
fn unreadable_files_are_reported_by_reading_and_set_aside_by_a_change() {
    let dir = temp_dir("bad");
    std::fs::create_dir_all(&dir).unwrap();
    let name = own_name(&keys());
    std::fs::write(dir.join(&name), b"{broken").unwrap();
    let store = DirStore::new(&dir);
    let loaded = store.load(&keys(), 10).unwrap();
    assert_eq!((loaded.unreadable.as_ref().map(|(n, _)| n.clone()), loaded.set_aside.clone()), (Some(name.clone()), None));
    assert_eq!(std::fs::read(dir.join(&name)).unwrap(), b"{broken", "a load leaves it as it is");
    assert!(!dir.join(format!("{name}.bad")).exists());
    let (_, changed) = store.update(&keys(), 10, |f| f.put(1, "fnv:b", [0.1, 0.2, 0.5, 0.6], KINGS, 1)).unwrap();
    assert_eq!(changed.set_aside.as_ref().map(|(n, _)| n.clone()), Some(format!("{name}.bad")));
    assert_eq!(std::fs::read(dir.join(format!("{name}.bad"))).unwrap(), b"{broken", "set aside, never deleted");
    assert_eq!(store.load(&keys(), 10).unwrap().file.live().count(), 1);
    let newer = br#"{"format":"pdfcraft-chess-corrections","version":3,"doc":{},"entries":[]}"#;
    std::fs::write(dir.join(&name), newer).unwrap();
    assert!(store.load(&keys(), 10).unwrap().read_only);
    // A newer file whose entries this build can make out shows them, read only.
    let mut kept = CorrectionFile::new(DocRecord::new(&keys(), 10));
    kept.put(1, "fnv:b", [0.1, 0.2, 0.5, 0.6], KINGS, 1).unwrap();
    let text = String::from_utf8(kept.to_bytes().unwrap()).unwrap().replace("\"version\":2", "\"version\":3,\"later\":true");
    std::fs::write(dir.join(&name), &text).unwrap();
    let shown = store.load(&keys(), 10).unwrap();
    assert_eq!((shown.read_only, shown.newer, shown.file.version, shown.file.live().count()), (true, Some(3), 3, 1));
    std::fs::write(dir.join(&name), newer).unwrap();
    let refused = store.update(&keys(), 10, |f| f.put(1, "fnv:b", [0.1, 0.2, 0.5, 0.6], KINGS, 1));
    assert!(matches!(refused, Err(StoreError::Correction(CorrectionError::Newer(3)))));
    assert_eq!(std::fs::read(dir.join(&name)).unwrap(), newer.to_vec(), "left as it was");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A9-G3a (A4-06, R2-03): one entry that can't be read among sixteen used to set the whole file
/// aside, and every correction disappeared from view (16 → 1). Now that entry is left out and
/// counted, the fifteen others are kept, and the change that writes the file without it first
/// sets the file aside as it was, so nothing is lost. An entry recording a page past
/// [`MAX_PAGES`] (`usize::MAX`, whose page number overflowed) is left out the same way.
#[test]
fn one_bad_entry_among_sixteen_keeps_the_fifteen() {
    let dir = temp_dir("one-bad");
    std::fs::create_dir_all(&dir).unwrap();
    let mut f = CorrectionFile::new(DocRecord::new(&keys(), 10));
    for i in 0..16u8 {
        let y = 0.05 * f32::from(i);
        f.put(usize::from(i % 8), "fnv:x", [0.1, y, 0.14, y + 0.04], KINGS, u64::from(i)).unwrap();
    }
    assert_eq!(f.live().count(), 16);
    let text = String::from_utf8(f.to_bytes().unwrap()).unwrap();
    // One box with its corners swapped.
    let bad = text.replacen("[0.1,0.0,0.14,0.04]", "[0.14,0.0,0.1,0.04]", 1);
    assert_ne!(bad, text);
    let name = own_name(&keys());
    std::fs::write(dir.join(&name), &bad).unwrap();
    let store = DirStore::new(&dir);
    let loaded = store.load(&keys(), 10).unwrap();
    assert_eq!((loaded.file.live().count(), loaded.skipped, loaded.unreadable.clone()), (15, 1, None));
    let (_, written) = store.update(&keys(), 10, |f| f.put(9, "fnv:new", [0.5, 0.5, 0.9, 0.9], QUEEN, 99)).unwrap();
    assert_eq!((written.file.live().count(), written.skipped), (16, 1));
    let aside = written.set_aside.as_ref().map(|(n, _)| n.clone());
    assert_eq!(aside, Some(format!("{name}.bad")), "the file as it was is kept before the entry is dropped");
    assert_eq!(std::fs::read_to_string(dir.join(format!("{name}.bad"))).unwrap(), bad);
    assert_eq!(store.load(&keys(), 10).unwrap().skipped, 0, "written without it");
    // R2-03: a page past any document's.
    let huge = text.replacen("\"page\":0,", &format!("\"page\":{},", usize::MAX), 1);
    assert_ne!(huge, text);
    let Parsed::File { file, skipped } = CorrectionFile::parse(huge.as_bytes()) else { panic!("{huge}") };
    assert_eq!((file.live().count(), skipped), (15, 1));
    assert!(file.entries.iter().all(|e| e.page < MAX_PAGES));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A9-G3a (A4-07): the corrections are `<hash>.corrections.json`, so the book's PGN kept in the
/// same folder under the name both used to share is never taken for them, nor set aside by them.
/// The file earlier builds wrote, `<hash>.json`, is read and merged in, and never written: a build
/// from before keeps working on it, and what it writes there is seen here at the next load.
#[test]
fn the_name_before_is_read_and_merged_and_another_format_never_touched() {
    let dir = temp_dir("names");
    std::fs::create_dir_all(&dir).unwrap();
    let legacy = file_name(&keys()).unwrap();
    // The book's PGN, under the name both kinds shared.
    let pgn = br#"{"format":"pdfcraft-chess-reference","version":1,"doc":{"pages":10},"pgn":{}}"#;
    std::fs::write(dir.join(&legacy), pgn).unwrap();
    let store = DirStore::new(&dir);
    let (uid, written) = store.update(&keys(), 10, |f| f.put(1, "fnv:b", [0.1, 0.2, 0.5, 0.6], KINGS, 1)).unwrap();
    assert_eq!((written.name.clone(), written.set_aside.clone(), written.unreadable.clone()), (own_name(&keys()), None, None));
    assert_eq!(std::fs::read(dir.join(&legacy)).unwrap(), pgn.to_vec(), "another format's file is left as it is");
    // An earlier build's corrections under the name before: merged in, never written.
    let mut old = CorrectionFile::new(DocRecord::new(&keys(), 10));
    let old_uid = old.put(2, "fnv:c", [0.1, 0.2, 0.5, 0.6], QUEEN, 5).unwrap();
    std::fs::write(dir.join(&legacy), old.to_bytes().unwrap()).unwrap();
    let both = store.load(&keys(), 10).unwrap();
    let mut uids: Vec<String> = both.file.live().map(|e| e.uid.clone()).collect();
    uids.sort();
    let mut expected = vec![uid.clone(), old_uid.clone()];
    expected.sort();
    assert_eq!(uids, expected);
    let before = std::fs::read(dir.join(&legacy)).unwrap();
    store.update(&keys(), 10, |f| Ok::<_, CorrectionError>(f.delete(&old_uid, 9))).unwrap();
    assert_eq!(std::fs::read(dir.join(&legacy)).unwrap(), before, "the name before is never written");
    assert_eq!(store.load(&keys(), 10).unwrap().file.live().map(|e| e.uid.clone()).collect::<Vec<_>>(), vec![uid], "its deletion holds");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The web build's store: the same finding, merging and setting aside, within its own budget.
#[test]
fn the_memory_store_keeps_files_within_its_budget() {
    let store = MemStore::new(4096);
    let (uid, loaded) = store.update(&keys(), 10, |f| f.put(1, "fnv:b", [0.1, 0.2, 0.5, 0.6], KINGS, 1)).unwrap();
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
            f.put(2, "fnv:c", [0.0, y, 0.09, y + 0.09], QUEEN, 2)?;
        }
        Ok::<(), CorrectionError>(())
    });
    assert!(matches!(many, Err(StoreError::Correction(CorrectionError::OverBudget(600)))), "{many:?}");
    assert_eq!(small.files(), before);
    assert!(MemStore::with_files(store.files(), 10).files().is_empty(), "a file past the budget is not taken in");
    // A file that can't be read is reported by a load, and set aside by a change, never dropped.
    let name = own_name(&keys());
    let broken = MemStore::with_files([(name.clone(), b"{broken".to_vec())], 4096);
    let l = broken.load(&keys(), 10).unwrap();
    assert_eq!((l.unreadable.map(|(n, _)| n), l.set_aside), (Some(name.clone()), None));
    assert_eq!(broken.files(), vec![(name.clone(), b"{broken".to_vec())]);
    let (_, l) = broken.update(&keys(), 10, |f| f.put(1, "fnv:b", [0.1, 0.2, 0.5, 0.6], KINGS, 1)).unwrap();
    assert_eq!(l.set_aside.map(|(n, _)| n), Some(format!("{name}.bad")));
    assert!(broken.files().contains(&(format!("{name}.bad"), b"{broken".to_vec())));
    assert_eq!(MemStore::new(4096).load(&DocKeys::default(), 1).map(|_| ()).map_err(|e| e.to_string()), Err(StoreError::NoKeys.to_string()));
}

// ─── Applying corrections to readings (P3a3) ──────────────────────────────────────────────────

/// A read page 0 of 600 × 800 pt whose diagrams are at `boxes` (user space), read as `fens`
/// (`None`: unread).
fn read_page(boxes: &[[f32; 4]], fens: &[Option<&str>]) -> Arc<crate::chess::ChessPage> {
    read_page_at(0, boxes, fens)
}

/// [`read_page`] as page `page`.
fn read_page_at(page: usize, boxes: &[[f32; 4]], fens: &[Option<&str>]) -> Arc<crate::chess::ChessPage> {
    use pdfcraft_chessvision::board::{BoardReading, ReadingSource, Unreadable};
    use pdfcraft_chessvision::coords::{PageGeometry, PageRect, Rotation};
    let readings = boxes
        .iter()
        .zip(fens)
        .enumerate()
        .map(|(index, (b, fen))| BoardReading {
            index,
            bbox_user: PageRect::new(b[0], b[1], b[2], b[3]),
            source: ReadingSource::Font,
            dpi: 220.0,
            outcome: fen.and_then(|f| corrected_position(f, None)).ok_or(Unreadable::NoModel),
        })
        .collect();
    Arc::new(crate::chess::ChessPage {
        page,
        geometry: PageGeometry::new(PageRect::new(0.0, 0.0, 600.0, 800.0), Rotation::R0),
        dpi: 220.0,
        diagrams: Vec::new(),
        readings: Some(readings),
        truncated: false,
        error: None,
        notes: Vec::new(),
    })
}

fn fen_of(page: &crate::chess::ChessPage, i: usize) -> Option<String> {
    page.readings.as_ref()?.get(i)?.position().map(|p| p.fen.clone())
}

#[test]
fn a_correction_replaces_its_diagrams_reading_whatever_it_reads_now() {
    // Two diagrams: the top one (0.1..0.5 × 0.6..0.9 of the page) read, the bottom one not.
    let page = read_page(&[[60.0, 480.0, 300.0, 720.0], [60.0, 80.0, 300.0, 320.0]], &[Some(KINGS), None]);
    let fps = vec!["fnv:p0".to_owned()];
    let mut f = CorrectionFile::new(DocRecord::default());
    let made_over = NewCorrection { orig: Some(Original { fen: Some(KINGS.into()), model: None }), ..new(0, [0.1, 0.6, 0.5, 0.9], QUEEN) };
    f.set(made_over, 1, &fps).unwrap();
    f.put(0, "fnv:p0", [0.1, 0.1, 0.5, 0.4], KINGS, 1).unwrap();
    let out = apply(std::slice::from_ref(&page), &f, &fps);
    let fixed = &out.pages[0];
    assert_eq!((fen_of(fixed, 0).as_deref(), fen_of(fixed, 1).as_deref()), (Some(QUEEN), Some(KINGS)), "the unread one gets a position too");
    let r = fixed.readings.as_ref().unwrap();
    assert!(r.iter().all(|r| r.source == pdfcraft_chessvision::board::ReadingSource::User && r.accepted()), "{r:#?}");
    assert_eq!(out.applied.iter().map(|a| (a.index, a.diverged)).collect::<Vec<_>>(), vec![(0, false), (1, false)]);
    assert!(out.unmatched.is_empty() && fixed.notes.is_empty());
    // The reading changed since (another model): the correction still wins, and it is said.
    let reread = read_page(&[[60.0, 480.0, 300.0, 720.0], [60.0, 80.0, 300.0, 320.0]], &[Some(START_FEN), None]);
    let out = apply(&[reread], &f, &fps);
    assert_eq!(fen_of(&out.pages[0], 0).as_deref(), Some(QUEEN));
    assert!(
        out.applied[0].diverged && out.pages[0].notes.iter().any(|n| n.contains("diagram 1") && n.contains("changed")),
        "{:?}",
        out.pages[0].notes
    );
    // Without corrections for the page, the very same page comes back.
    let empty = CorrectionFile::new(DocRecord::default());
    assert!(Arc::ptr_eq(&apply(std::slice::from_ref(&page), &empty, &fps).pages[0], &page));
}

const START_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

#[test]
fn a_correction_without_its_diagram_is_listed_never_dropped() {
    let page = read_page(&[[60.0, 480.0, 300.0, 720.0]], &[Some(KINGS)]);
    let fps = vec!["fnv:p0".to_owned()];
    let mut f = CorrectionFile::new(DocRecord::default());
    // One where a diagram was before a new detection; one with a FEN edited by hand into the file.
    f.put(0, "fnv:p0", [0.6, 0.1, 0.9, 0.4], QUEEN, 1).unwrap();
    f.put(0, "fnv:p0", [0.1, 0.6, 0.5, 0.9], QUEEN, 1).unwrap();
    f.entries[1].fen = "not a fen".into();
    let out = apply(std::slice::from_ref(&page), &f, &fps);
    let reasons: Vec<&str> = out.unmatched.iter().map(|u| u.reason.as_str()).collect();
    assert_eq!(reasons, ["no diagram on the page is where it was", "its position is not a FEN this build reads"]);
    assert_eq!(fen_of(&out.pages[0], 0).as_deref(), Some(KINGS), "the reading stays");
    // Two corrections over one diagram: the closer one takes it, the other is listed.
    let mut two = CorrectionFile::new(DocRecord::default());
    two.put(0, "fnv:p0", [0.1, 0.6, 0.5, 0.9], QUEEN, 1).unwrap();
    two.entries.push(Correction { uid: "second".into(), rect: [0.11, 0.61, 0.51, 0.91], ..two.entries[0].clone() });
    let out = apply(std::slice::from_ref(&page), &two, &fps);
    assert_eq!((out.applied.len(), out.unmatched.iter().map(|u| u.uid.as_str()).collect::<Vec<_>>()), (1, vec!["second"]));
    // A page scanned without reading is left as it is.
    let unread = Arc::new(crate::chess::ChessPage { readings: None, ..(*page).clone() });
    let out = apply(std::slice::from_ref(&unread), &f, &fps);
    assert!(Arc::ptr_eq(&out.pages[0], &unread) && out.unmatched.is_empty());
}

#[test]
fn saving_moves_the_corrections_to_the_saved_files_keys() {
    let saved_as =
        DocKeys { id0: Some([3; 16]), path: Some("C:/books/Livro corrigido.pdf".into()), name: Some("Livro corrigido.pdf".into()), size: Some(99) };
    // Nothing to move: the folder is not made (a save in a folder without corrections leaves it as
    // it was).
    let none = temp_dir("rekey-none");
    assert!(!DirStore::new(&none).rekey(&keys(), &saved_as, 10).unwrap());
    assert!(!none.exists(), "no folder made");
    for store in [&MemStore::new(1 << 20) as &dyn Rekey, &DirStore::new(temp_dir("rekey")) as &dyn Rekey] {
        assert!(!store.rekey_dyn(&keys(), &saved_as), "nothing to move");
        store.set_dyn(&keys());
        assert!(store.rekey_dyn(&keys(), &saved_as));
        assert_eq!(store.count_dyn(&saved_as), 1, "found under the saved file's keys");
        assert!(!store.rekey_dyn(&saved_as, &saved_as), "the same keys: nothing to do");
    }
    let _ = std::fs::remove_dir_all(temp_dir("rekey"));
}

/// The two stores behind one object-safe face, for the test above.
trait Rekey {
    fn rekey_dyn(&self, old: &DocKeys, new: &DocKeys) -> bool;
    fn set_dyn(&self, keys: &DocKeys);
    fn count_dyn(&self, keys: &DocKeys) -> usize;
}

impl<B: StoreBackend> Rekey for DocStore<Corrections, B> {
    fn rekey_dyn(&self, old: &DocKeys, new: &DocKeys) -> bool {
        self.rekey(old, new, 10).unwrap()
    }
    fn set_dyn(&self, keys: &DocKeys) {
        self.update(keys, 10, |f| f.put(1, "fnv:b", [0.1, 0.2, 0.5, 0.6], KINGS, 1)).unwrap();
    }
    fn count_dyn(&self, keys: &DocKeys) -> usize {
        self.load(keys, 10).unwrap().file.live().count()
    }
}

// ─── Builds of version 1 (plan A9 §2.10) ───────────────────────────────────────────────────────

#[allow(dead_code)]
#[path = "../../../tests/compat/corrections_v1.rs"]
mod corrections_v1;

/// A9-G2a, plan §2.10: a build of version 1 (the frozen copy of its format) reads a file of
/// version 2 as a newer version, so it keeps it read only and never writes it back without the
/// neighbours. And a file such a build wrote is read here with every field and written as version 2
/// with nothing lost.
#[test]
fn compat_a_version_1_build_never_rewrites_a_version_2_file() {
    let book = fps(&["fnv:p0", "fnv:p1", "fnv:p2"]);
    let mut f = CorrectionFile::new(DocRecord::new(&keys(), 3));
    f.set(new(1, [0.1, 0.6, 0.5, 0.9], QUEEN), 1, &book).unwrap();
    assert!(matches!(corrections_v1::CorrectionFile::read(&f.to_bytes().unwrap()), corrections_v1::Read::Newer(2, _)));
    // What that build writes: a correction, and a tombstone with an unknown reading.
    let entry = |uid: &str, orig: Option<corrections_v1::Original>, deleted: bool| corrections_v1::Correction {
        uid: uid.into(),
        page: 1,
        page_fp: "fnv:p1".into(),
        rect: [0.1, 0.6, 0.5, 0.9],
        orig,
        fen: QUEEN.into(),
        kind: corrections_v1::Kind::Corrected,
        at: 7,
        rev: 3,
        deleted,
    };
    let made_over = corrections_v1::Original { fen: Some(KINGS.into()), model: Some("model".into()) };
    let old = corrections_v1::CorrectionFile {
        format: corrections_v1::FORMAT.into(),
        version: corrections_v1::VERSION,
        doc: corrections_v1::DocRecord { name: Some("Livro.pdf".into()), pages: 3, ..Default::default() },
        entries: vec![entry("u1", Some(made_over), false), entry("u2", None, true)],
    };
    let Read::File(now) = CorrectionFile::read(&old.to_bytes().unwrap()) else { panic!("a version 1 file is read") };
    let Read::File(again) = CorrectionFile::read(&now.to_bytes().unwrap()) else { panic!("written as version 2 and read") };
    assert_eq!(again, now);
    let as_v1 = |e: &Correction| corrections_v1::Correction {
        uid: e.uid.clone(),
        page: e.page,
        page_fp: e.page_fp.clone(),
        rect: e.rect,
        orig: e.orig.clone().map(|o| corrections_v1::Original { fen: o.fen, model: o.model }),
        fen: e.fen.clone(),
        kind: match e.kind {
            Kind::Verified => corrections_v1::Kind::Verified,
            Kind::Corrected => corrections_v1::Kind::Corrected,
        },
        at: e.at,
        rev: e.rev,
        deleted: e.deleted,
    };
    assert_eq!(again.entries.iter().map(as_v1).collect::<Vec<_>>(), old.entries, "every field of version 1 kept");
    assert_eq!((again.doc.name.as_deref(), again.doc.pages), (Some("Livro.pdf"), 3));
}

/// [`pages_pdf`] with a file identifier, so its corrections file is named by it wherever it is
/// opened from.
fn with_id(pdf: Vec<u8>) -> Vec<u8> {
    let text = String::from_utf8(pdf).unwrap();
    let id = "<00112233445566778899aabbccddeeff>";
    text.replace("/Root 1 0 R >>", &format!("/Root 1 0 R /ID [{id} {id}] >>")).into_bytes()
}

/// The same with an earlier build's command line (`PDFCRAFT_OLD_CLI`, such as the installed
/// `dist/app/pdfcraft-cli.exe`): a build older than version 2 lists the file read only, refuses to
/// change it and leaves its bytes as they were; a build of this version or newer lists its
/// correction. Without the variable it is skipped with a warning, and it fails under
/// `PDFCRAFT_REQUIRE_MODELS=1` (the chess gate, which passes the installed build).
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn compat_an_earlier_command_line_keeps_a_version_2_file_as_it_is() {
    let cli = match pdfcraft_chessvision::models::test_resource("PDFCRAFT_OLD_CLI", "an earlier build's pdfcraft-cli") {
        Ok(Some(cli)) => cli,
        Ok(None) => {
            eprintln!(
                "compat_an_earlier_command_line_keeps_a_version_2_file_as_it_is skipped: set PDFCRAFT_OLD_CLI to an earlier build's pdfcraft-cli"
            );
            return;
        }
        Err(e) => panic!("{e}"),
    };
    let dir = temp_dir("old-cli");
    let store = dir.join(".pdfcraft").join("chess-corrections");
    std::fs::create_dir_all(&store).unwrap();
    let pdf = with_id(pages_pdf(3));
    std::fs::write(dir.join("book.pdf"), &pdf).unwrap();
    let (s, id) = open(pdf);
    let (keys, book) = s.get(id).map(|d| (d.doc_keys(), d.page_fingerprints())).unwrap();
    assert!(keys.id().is_some(), "named by its /ID");
    let mut f = CorrectionFile::new(DocRecord::new(&keys, book.len()));
    let uid = f.set(new(1, [0.1, 0.6, 0.5, 0.9], QUEEN), 1, &book).unwrap();
    let name = file_name(&keys).unwrap();
    let bytes = f.to_bytes().unwrap();
    std::fs::write(store.join(&name), &bytes).unwrap();
    let steps = serde_json::json!([
        { "tool": "doc_open", "args": { "path": "book.pdf" } },
        { "tool": "chess_correction_list", "args": { "doc": 1 } },
        { "tool": "chess_correction_delete", "args": { "doc": 1, "uid": uid } },
    ]);
    std::fs::write(dir.join("steps.json"), steps.to_string()).unwrap();
    let out = std::process::Command::new(&cli)
        .args(["run", "--report", "--script"])
        .arg(dir.join("steps.json"))
        .arg("--root")
        .arg(&dir)
        .output()
        .unwrap_or_else(|e| panic!("{}: {e}", cli.display()));
    let report: serde_json::Value = serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)));
    let listed = &report["steps"][1]["result"][0];
    if listed["read_only"] == true {
        let refused = &report["steps"][2];
        assert_eq!(refused["status"], "error", "{report:#}");
        assert!(refused["error"].as_str().is_some_and(|m| m.contains("newer")), "{report:#}");
        assert_eq!(std::fs::read(store.join(&name)).unwrap(), bytes, "left as it was");
    } else {
        assert_eq!((listed["count"].as_u64(), listed["corrections"][0]["uid"].as_str()), (Some(1), Some(uid.as_str())), "{report:#}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A9-G3a, plan §2.10 (what an earlier build sees): the corrections are written as
/// `<hash>.corrections.json` now. An earlier build's command line (`PDFCRAFT_OLD_CLI`, the
/// installed `dist/app/pdfcraft-cli.exe`) keeps to `<hash>.json`: it does not see what this build
/// kept, cannot change it, and leaves its file as it was, setting nothing aside. What such a build
/// keeps under the name before (its frozen format, `corrections_v1`) is read here and merged in.
/// Without the variable it is skipped with a warning, and it fails under
/// `PDFCRAFT_REQUIRE_MODELS=1` (the chess gate).
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn compat_an_earlier_command_line_and_this_build_share_a_folder() {
    let cli = match pdfcraft_chessvision::models::test_resource("PDFCRAFT_OLD_CLI", "an earlier build's pdfcraft-cli") {
        Ok(Some(cli)) => cli,
        Ok(None) => {
            eprintln!(
                "compat_an_earlier_command_line_and_this_build_share_a_folder skipped: set PDFCRAFT_OLD_CLI to an earlier build's pdfcraft-cli"
            );
            return;
        }
        Err(e) => panic!("{e}"),
    };
    let dir = temp_dir("old-cli-names");
    let folder = dir.join(".pdfcraft").join("chess-corrections");
    std::fs::create_dir_all(&folder).unwrap();
    let pdf = with_id(pages_pdf(3));
    std::fs::write(dir.join("book.pdf"), &pdf).unwrap();
    let (s, id) = open(pdf);
    let (keys, book) = s.get(id).map(|d| (d.doc_keys(), d.page_fingerprints())).unwrap();
    let store = DirStore::new(&folder);
    let (uid, _) = store.update(&keys, book.len(), |f| f.set(new(1, [0.1, 0.6, 0.5, 0.9], QUEEN), 1, &book)).unwrap();
    let ours = own_name(&keys);
    let bytes = std::fs::read(folder.join(&ours)).unwrap();
    let steps = serde_json::json!([
        { "tool": "doc_open", "args": { "path": "book.pdf" } },
        { "tool": "chess_correction_list", "args": { "doc": 1 } },
        { "tool": "chess_correction_delete", "args": { "doc": 1, "uid": uid } },
    ]);
    std::fs::write(dir.join("steps.json"), steps.to_string()).unwrap();
    let out = std::process::Command::new(&cli)
        .args(["run", "--report", "--script"])
        .arg(dir.join("steps.json"))
        .arg("--root")
        .arg(&dir)
        .output()
        .unwrap_or_else(|e| panic!("{}: {e}", cli.display()));
    let report: serde_json::Value = serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)));
    let listed = &report["steps"][1]["result"][0];
    let sees = listed["count"].as_u64() != Some(0);
    if sees {
        // A build that reads version 2 finds the file by the document it records, and changes it
        // where it is.
        assert_eq!((listed["count"].as_u64(), listed["file"].as_str()), (Some(1), Some(ours.as_str())), "{report:#}");
        assert_eq!(report["steps"][2]["status"], "ok", "{report:#}");
    } else {
        assert_eq!(listed["file"].as_str(), file_name(&keys).as_deref(), "{report:#}");
        assert_eq!(report["steps"][2]["status"], "error", "it cannot reach what this build kept: {report:#}");
        assert_eq!(std::fs::read(folder.join(&ours)).unwrap(), bytes, "left as it was");
    }
    let names: Vec<String> = std::fs::read_dir(&folder).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert!(names.iter().all(|n| !n.contains(".bad")), "nothing set aside: {names:?}");
    // What that build keeps under the name before, in its own format: seen here, merged.
    let record = DocRecord::new(&keys, book.len());
    let theirs = corrections_v1::CorrectionFile {
        format: corrections_v1::FORMAT.into(),
        version: corrections_v1::VERSION,
        doc: corrections_v1::DocRecord { id0: record.id0, path_hash: record.path_hash, name: record.name, size: record.size, pages: record.pages },
        entries: vec![corrections_v1::Correction {
            uid: "old".into(),
            page: 2,
            page_fp: book[2].clone(),
            rect: [0.1, 0.6, 0.5, 0.9],
            orig: None,
            fen: KINGS.into(),
            kind: corrections_v1::Kind::Verified,
            at: 3,
            rev: 1,
            deleted: false,
        }],
    };
    std::fs::write(folder.join(file_name(&keys).unwrap()), theirs.to_bytes().unwrap()).unwrap();
    let both = store.load(&keys, book.len()).unwrap();
    let mut uids: Vec<&str> = both.file.live().map(|e| e.uid.as_str()).collect();
    uids.sort_unstable();
    let mut expected = if sees { vec!["old"] } else { vec!["old", uid.as_str()] };
    expected.sort_unstable();
    assert_eq!(uids, expected);
    let _ = std::fs::remove_dir_all(&dir);
}

#[allow(dead_code)]
#[path = "../../../tests/compat/corrections_v2.rs"]
mod corrections_v2;

/// A9-G3a, plan §2.10: a build of version 2 from before the document store (G2a's; its format
/// frozen in `corrections_v2`) reads what this build writes as a file of its own version, and what
/// it writes back comes in here with every field, nothing left out or counted as unreadable.
#[test]
fn compat_a_version_2_build_reads_and_writes_back_what_this_one_keeps() {
    let book = fps(&["fnv:p0", "fnv:p1", "fnv:p2"]);
    let mut f = CorrectionFile::new(DocRecord::new(&keys(), 3));
    f.set(new(1, [0.1, 0.6, 0.5, 0.9], QUEEN), 1, &book).unwrap();
    let verified =
        NewCorrection { orig: Some(Original { fen: Some(KINGS.into()), model: None }), kind: Kind::Verified, ..new(2, [0.1, 0.1, 0.5, 0.4], KINGS) };
    f.set(verified, 2, &book).unwrap();
    let gone = f.set(new(0, [0.2, 0.2, 0.6, 0.6], KINGS), 3, &book).unwrap();
    assert!(f.delete(&gone, 4));
    let corrections_v2::Read::File(theirs) = corrections_v2::CorrectionFile::read(&f.to_bytes().unwrap()) else {
        panic!("a version 2 build reads it as its own")
    };
    assert_eq!(theirs.entries.len(), 3);
    let Parsed::File { file, skipped } = CorrectionFile::parse(&theirs.to_bytes().unwrap()) else { panic!("read back") };
    assert_eq!((file, skipped), (f, 0), "every field kept");
}

// ─── The cost of placing the corrections (plan A9-G2b, R8-04) ─────────────────────────────────

/// A synthetic book of `pages` pages (fingerprints `fnv:p<i>`), page `i` with `per_page(i)`
/// diagrams one above the other, each corrected: the fingerprints, the file, and each page's
/// diagram boxes in user space.
fn corrected_book(pages: usize, per_page: impl Fn(usize) -> usize) -> (Vec<String>, CorrectionFile, Vec<Vec<[f32; 4]>>) {
    let book: Vec<String> = (0..pages).map(|i| format!("fnv:p{i}")).collect();
    let mut f = CorrectionFile::new(DocRecord::new(&keys(), pages));
    let mut boxes = Vec::with_capacity(pages);
    for p in 0..pages {
        let here: Vec<[f32; 4]> = (0..per_page(p).min(5))
            .map(|j| {
                let top = 790.0 - 150.0 * j as f32;
                [60.0, top - 140.0, 300.0, top]
            })
            .collect();
        for b in &here {
            f.set(new(p, [b[0] / 600.0, b[1] / 800.0, b[2] / 600.0, b[3] / 800.0], QUEEN), 1, &book).unwrap();
        }
        boxes.push(here);
    }
    (book, f, boxes)
}

/// [`corrected_book`] after its first page was deleted: [`apply`] on every page left, read. The
/// fingerprints it was asked about, the diagrams and the corrections, and what it applied.
fn after_deleting_page_one(pages: usize, per_page: impl Fn(usize) -> usize + Copy) -> (usize, usize, usize, Corrected, std::time::Duration) {
    let (book, f, boxes) = corrected_book(pages, per_page);
    let read: Vec<Arc<crate::chess::ChessPage>> =
        boxes.iter().enumerate().skip(1).map(|(p, b)| read_page_at(p - 1, b, &vec![None; b.len()])).collect();
    let diagrams: usize = boxes.iter().skip(1).map(Vec::len).sum();
    let fps = Fingerprints::new(book.get(1..).unwrap_or_default().to_vec());
    let t = std::time::Instant::now();
    let out = apply(&read, &f, &fps);
    (fps.looked_up(), diagrams, f.live().count(), out, t.elapsed())
}

/// A9-G2b (R8-04): placing the corrections asked about every page for each one (D·C·P in the
/// app, O(C·P) in the engine); with the fingerprints indexed once it costs a few look-ups each,
/// at most 2 · (D + C) once a page was deleted and every later page moved. The corrections of
/// the deleted page are orphans, the others all applied.
#[test]
fn placing_the_corrections_costs_a_few_look_ups_each() {
    let (looked, d, c, out, _) = after_deleting_page_one(240, |p| 1 + p % 3);
    assert!(looked <= 2 * (d + c), "{looked} look-ups for {d} diagrams and {c} corrections");
    assert_eq!(out.applied.len(), d, "every diagram left has its correction");
    assert!(out.unmatched.is_empty(), "{:?}", out.unmatched.first());
}

/// [`placing_the_corrections_costs_a_few_look_ups_each`] at the scale of the Polgar (1,104 pages,
/// 5,334 diagrams, each corrected), in release by the chess gate (stage 4b).
#[test]
#[ignore = "release only: cargo xtask chess-gate runs it (stage 4b)"]
fn perf_assign() {
    let per_page = |p: usize| if p < 918 { 5 } else { 4 };
    let (looked, d, c, out, took) = after_deleting_page_one(1104, per_page);
    assert_eq!(c, 5334);
    assert!(looked <= 2 * (d + c), "{looked} look-ups for {d} diagrams and {c} corrections");
    assert_eq!(out.applied.len(), d);
    eprintln!("PERF perf_assign {:.1}", took.as_secs_f64() * 1000.0);
}

/// [`assign`] is the one rule: of two corrections over one diagram, the one found by its page's
/// fingerprint takes it over the one found by its index alone, even when that one is later, and
/// the other is listed with the one that took the diagram; a correction over no diagram is listed
/// too, and one on a page not given is neither.
#[test]
fn assign_gives_each_diagram_one_correction_by_the_one_rule() {
    let book = fps(&["fnv:p0", "fnv:p1", "fnv:p2"]);
    let mut f = CorrectionFile::new(DocRecord::new(&keys(), 3));
    let by_fp = f.set(new(0, [0.1, 0.6, 0.5, 0.9], QUEEN), 1, &book).unwrap();
    let by_index = f.set(new(1, [0.1, 0.6, 0.5, 0.9], KINGS), 9, &book).unwrap();
    let nowhere = f.set(new(1, [0.1, 0.1, 0.3, 0.3], KINGS), 9, &book).unwrap();
    let elsewhere = f.set(new(2, [0.1, 0.6, 0.5, 0.9], KINGS), 9, &book).unwrap();
    let e = |uid: &str| f.entries.iter().find(|e| e.uid == uid).unwrap();
    let located =
        [(Located::Changed(0), e(&by_index)), (Located::Moved(0), e(&by_fp)), (Located::Changed(0), e(&nowhere)), (Located::Same(2), e(&elsewhere))];
    let pages = [PageBoxes { page: 0, boxes: vec![Some([0.1, 0.6, 0.5, 0.9])] }];
    let (assigned, unmatched) = assign(&pages, &located);
    assert_eq!(assigned.iter().map(|a| (a.correction.uid.as_str(), a.page, a.index)).collect::<Vec<_>>(), vec![(by_fp.as_str(), 0, 0)]);
    let reasons: Vec<(&str, &str)> = unmatched.iter().map(|u| (u.uid.as_str(), u.reason.as_str())).collect();
    let taken = format!("the diagram already has correction {by_fp}");
    assert_eq!(reasons, vec![(by_index.as_str(), taken.as_str()), (nowhere.as_str(), "no diagram on the page is where it was")]);
}
