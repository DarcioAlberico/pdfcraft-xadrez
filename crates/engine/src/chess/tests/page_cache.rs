//! The scan cache across edits (plan A8 P3a2): an edit reads again only the pages it touched,
//! pages moved by inserting or removing pages are found at their new index, and an undo reads
//! nothing again. Counted with `ChessCache::scanned`, on a book of 48 synthetic pages, and of
//! 703 in the plan's measure, which the everyday tests leave out (plan A9-Q4b, finding A7-17:
//! about a minute, for an invariant that does not depend on the length).

use super::*;

/// The pages (0-based) with a board set in Chess Merida; the others say their number.
const BOARDS: [usize; 2] = [4, 40];

/// A `pages`-page PDF of 600 × 800 pt: a Chess Merida board on [`BOARDS`], the page number in
/// Helvetica on the others. `F1` (Merida) and `F2` (Helvetica) are shared by every page.
fn book(pages: usize) -> Vec<u8> {
    // 1 catalog, 2 page tree, 3 and 4 the fonts, then each page and its content.
    let widths = vec!["1000"; 95].join(" ");
    let font =
        |name: &str| format!("<< /Type /Font /Subtype /Type1 /BaseFont /{name} /FirstChar 32 /LastChar 126 /Widths [{widths}] >>").into_bytes();
    let kids: String = (0..pages).map(|i| format!(" {} 0 R", 5 + 2 * i)).collect();
    let mut objs: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        format!("<< /Type /Pages /Kids [{kids}] /Count {pages} /MediaBox [0 0 600 800] >>").into_bytes(),
        font("ChessMerida"),
        font("Helvetica"),
    ];
    for i in 0..pages {
        let content = if BOARDS.contains(&i) { font_board(60.0, 60.0, MERIDA_START) } else { format!("BT /F2 12 Tf 280 40 Td ({}) Tj ET\n", i + 1) };
        objs.push(
            format!("<< /Type /Page /Parent 2 0 R /Contents {} 0 R /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> >>", 6 + 2 * i).into_bytes(),
        );
        let mut s = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
        s.extend_from_slice(content.as_bytes());
        s.extend_from_slice(b"\nendstream");
        objs.push(s);
    }
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(o);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
    out
}

/// Detection at the coarsest resolution, no model: the book's 703 pages scan quickly.
fn quick() -> ChessScanSettings {
    ChessScanSettings { dpi: MIN_DPI, ..no_model() }
}

/// Scan every page; how many were scanned rather than answered from the cache.
fn scan_all(s: &mut Session, id: DocId) -> (Vec<Arc<ChessPage>>, usize) {
    let before = s.chess.scanned;
    let pages = s.chess_scan(id, &[], &quick(), true).unwrap();
    (pages, s.chess.scanned - before)
}

/// The pages with a diagram, and the position read there.
fn boards(pages: &[Arc<ChessPage>]) -> Vec<(usize, String)> {
    pages
        .iter()
        .filter(|p| !p.diagrams.is_empty())
        .map(|p| (p.page, p.readings.as_ref().and_then(|r| r.first()).and_then(|r| r.position()).map(|r| r.fen.clone()).unwrap_or_default()))
        .collect()
}

#[test]
fn an_edit_reads_again_only_the_pages_it_touched() {
    edits_read_again_only_the_pages_they_touched(48);
}

/// The plan's measure, "editing page 5 of 703 reads 1 page": `cargo xtask chess-gate --full`
/// runs it (stage 5).
#[test]
#[ignore = "the plan's measure on 703 pages, about a minute: run with --ignored (chess-gate stage 5)"]
fn an_edit_reads_again_only_the_pages_it_touched_in_703_pages() {
    edits_read_again_only_the_pages_they_touched(703);
}

/// Scan a book of `pages` pages, then turn, undo, insert and delete pages, counting the pages
/// each scan reads again.
fn edits_read_again_only_the_pages_they_touched(pages: usize) {
    let (mut s, id) = open(book(pages));
    let (first, scanned) = scan_all(&mut s, id);
    assert_eq!((first.len(), scanned), (pages, pages));
    let found = boards(&first);
    assert_eq!(found.iter().map(|b| b.0).collect::<Vec<_>>(), BOARDS, "{found:?}");
    assert!(found.iter().all(|b| b.1.starts_with(START)), "{found:?}");
    assert_eq!(scan_all(&mut s, id).1, 0, "nothing changed");

    // Turning page 5 reads page 5 again, and only it.
    s.apply(id, Edit::RotatePages { pages: vec![4], degrees: 90 }).unwrap();
    let (turned, scanned) = scan_all(&mut s, id);
    assert_eq!(scanned, 1);
    assert_eq!(turned[4].geometry.unwrap().rotation(), Rotation::R90, "page 5 as it is now");
    assert!(Arc::ptr_eq(&turned[40], &first[40]), "the other board is the same result");
    assert_eq!(boards(&turned), found, "the turned board reads the same");

    // Undoing it reads nothing: page 5 is again what its first scan read.
    s.undo(id).unwrap();
    let (undone, scanned) = scan_all(&mut s, id);
    assert_eq!(scanned, 0);
    assert!(Arc::ptr_eq(&undone[4], &first[4]));

    // A page inserted first is the only page read: every other page moved one on, and is found
    // there with its new index.
    s.apply(id, Edit::InsertBlankPage { at: 0, width: 600.0, height: 800.0 }).unwrap();
    let (shifted, scanned) = scan_all(&mut s, id);
    assert_eq!((shifted.len(), scanned), (pages + 1, 1));
    assert!(shifted.iter().enumerate().all(|(i, p)| p.page == i), "each page carries its index now");
    assert_eq!(boards(&shifted), found.iter().map(|(p, fen)| (p + 1, fen.clone())).collect::<Vec<_>>());

    // Deleting it again reads nothing.
    s.apply(id, Edit::DeletePages { pages: vec![0] }).unwrap();
    let (back, scanned) = scan_all(&mut s, id);
    assert_eq!(scanned, 0);
    assert_eq!(boards(&back), found);
}

/// A scan finished after an edit to another page keeps its untouched pages, and drops the page
/// the edit changed.
#[test]
fn a_scan_that_outlived_an_edit_keeps_the_pages_it_did_not_touch() {
    let (mut s, id) = open(book(6));
    let job = s.chess_scan_job(id, &[], &quick(), true).unwrap();
    s.apply(id, Edit::RotatePages { pages: vec![2], degrees: 90 }).unwrap();
    let scan = job.run(&models(quick().models_dir.as_deref()), |_, _| true);
    assert!(s.store_chess_scan(&scan));
    let (pages, scanned) = scan_all(&mut s, id);
    assert_eq!(scanned, 1, "only the turned page");
    assert_eq!(pages[2].geometry.unwrap().rotation(), Rotation::R90);
    assert!(Arc::ptr_eq(&pages[4], &scan.pages[4]));
}

/// Plan A9-I3b (§2.4, R8-05): the memo of page keys and page fingerprints outlives an edit for
/// what the edit did not touch. A new Title hashes no page again, of either; turning one page
/// hashes that page, once each; an undo hashes nothing. What comes out is what a fresh memo
/// gives, every time.
#[test]
fn the_keys_and_fingerprints_outlive_an_edit_for_the_pages_it_did_not_touch() {
    const N: usize = 40;
    let (mut s, id) = open(book(N));
    let all: Vec<usize> = (0..N).collect();
    let fresh = |s: &Session| {
        let doc = s.get(id).unwrap();
        (keys(&doc.editor.as_ref().unwrap().cos, true), doc.page_fingerprints())
    };
    let now = |s: &mut Session| (s.chess_page_keys(id, &all), s.chess_fingerprints(id).unwrap().pages().to_vec());
    let counts = |s: &Session| (s.chess.keys_hashed, s.chess.fingerprints_made);
    assert_eq!(now(&mut s), fresh(&s));
    assert_eq!(counts(&s), (N, N));

    s.apply(id, Edit::SetInfo { key: "Title".into(), value: "A book of games".into() }).unwrap();
    assert_eq!(now(&mut s), fresh(&s));
    assert_eq!(counts(&s), (N, N), "the Title: no page hashed again");

    s.apply(id, Edit::RotatePages { pages: vec![7], degrees: 90 }).unwrap();
    let turned = now(&mut s);
    assert_eq!(turned, fresh(&s));
    assert_eq!(counts(&s), (N + 1, N + 1), "one page turned: that page hashed again");

    s.undo(id).unwrap();
    assert_eq!(now(&mut s), fresh(&s));
    let (k, f) = counts(&s);
    assert!(k <= N + 2 && f <= N + 2, "an undo hashes at most the page it gives back: {k}, {f}");
}

/// The keys of every page of `cos`, each from a fresh memo when `fresh`, else from one shared.
fn keys(cos: &pdfcraft_cos::Document, fresh: bool) -> Vec<Option<page_key::PageKey>> {
    keys_with::<page_key::Wide>(cos, fresh)
}

/// [`keys`], made with the hasher `H`.
fn keys_with<H: page_key::KeyHasher>(cos: &pdfcraft_cos::Document, fresh: bool) -> Vec<Option<H::Key>> {
    let pages = page_objects(cos, pdfcraft_organize::pages(cos).unwrap().len());
    let mut shared = page_key::KeyMemo::<H>::default();
    pages.iter().map(|p| if fresh { page_key::KeyMemo::<H>::default().page(cos, *p) } else { shared.page(cos, *p) }).collect()
}

/// Plan A9-N5a: the key a page's reading is kept on disk under (SHA-256) hashes what the
/// session's key hashes, the same way. Through every change below, the pages the session's keys
/// tell apart the disk's keys tell apart, and the ones they take for the same page they take for
/// the same page; the disk's keys are the same with a shared memo or without; and their value is
/// the same from one build to the next (frozen: a change to what is hashed leaves every reading
/// kept behind, which only a raised `reading::VERSION` should do).
#[test]
fn the_disk_key_hashes_what_the_session_key_hashes() {
    use pdfcraft_cos::{ObjRef, Object};
    let variant = |f: &dyn Fn(&mut pdfcraft_cos::Document)| {
        let mut cos = cos_of(book(6));
        f(&mut cos);
        cos
    };
    let docs = [
        cos_of(book(6)),
        variant(&|c| c.update_dict(ObjRef::new(3, 0), |d| d.set("BaseFont", Object::name("Helvetica"))).unwrap()),
        variant(&|c| c.update_dict(ObjRef::new(2, 0), |d| d.set("Rotate", Object::Int(90))).unwrap()),
        variant(&|c| {
            let content = b"BT /F2 12 Tf 10 40 Td (two) Tj ET".to_vec();
            c.set(ObjRef::new(8, 0), pdfcraft_cos::Stream::from_raw(pdfcraft_cos::Dict::new(), content));
        }),
        variant(&|c| {
            let resources = c.get(ObjRef::new(5, 0)).as_dict().unwrap().get(b"Resources").unwrap().clone();
            let r = c.add(resources);
            c.update_dict(ObjRef::new(5, 0), |d| {
                d.set("MediaBox", Object::Array([0, 0, 600, 800].map(Object::Int).to_vec()));
                d.set("Resources", Object::Ref(r));
            })
            .unwrap();
        }),
    ];
    let session: Vec<Option<page_key::PageKey>> = docs.iter().flat_map(|c| keys(c, false)).collect();
    let disk: Vec<Option<page_key::DiskKey>> = docs.iter().flat_map(|c| keys_with::<page_key::Sha>(c, false)).collect();
    assert!(disk.iter().all(Option::is_some));
    for i in 0..session.len() {
        for j in 0..session.len() {
            assert_eq!(session[i] == session[j], disk[i] == disk[j], "pages {i} and {j}");
        }
    }
    assert_eq!(docs.iter().flat_map(|c| keys_with::<page_key::Sha>(c, true)).collect::<Vec<_>>(), disk);
    let hex = |k: &page_key::DiskKey| k.iter().map(|b| format!("{b:02x}")).collect::<String>();
    assert_eq!(hex(&disk[0].unwrap()), FROZEN_PAGE_KEY);
}

/// The disk key of the first page of `book(6)` as plan A9-N5a made it.
const FROZEN_PAGE_KEY: &str = "2000d571a6cad1ba5def410d9f6db32c0155769ec3d350d3060052767314af9f";

fn cos_of(bytes: Vec<u8>) -> pdfcraft_cos::Document {
    pdfcraft_cos::Document::open(Arc::new(bytes)).unwrap()
}

/// A key says what draws the page: the same with a shared memo or without, different for pages
/// that draw differently, the same for pages that draw the same.
#[test]
fn page_keys_tell_pages_apart_by_what_draws_them() {
    let cos = cos_of(book(6));
    let shared = keys(&cos, false);
    assert!(shared.iter().all(Option::is_some));
    assert_eq!(shared, keys(&cos, true), "sharing the memo changes no key");
    let distinct: std::collections::HashSet<_> = shared.iter().collect();
    assert_eq!(distinct.len(), 6, "every page draws something else");
    assert_eq!(keys(&cos_of(book(6)), false), shared, "the same bytes, the same keys");
}

/// A change to what a page reaches changes its key: a shared font changes every page's, the
/// page tree's inherited `/MediaBox` too, an attribute of one page only that page's.
#[test]
fn page_keys_follow_shared_and_inherited_objects() {
    use pdfcraft_cos::{ObjRef, Object};
    let before = keys(&cos_of(book(6)), false);
    let changed = |f: &dyn Fn(&mut pdfcraft_cos::Document)| {
        let mut cos = cos_of(book(6));
        f(&mut cos);
        keys(&cos, false)
    };
    let font = changed(&|c| c.update_dict(ObjRef::new(3, 0), |d| d.set("BaseFont", Object::name("Helvetica"))).unwrap());
    assert!(font.iter().zip(&before).all(|(a, b)| a != b), "every page uses the font");
    let tree = changed(&|c| c.update_dict(ObjRef::new(2, 0), |d| d.set("Rotate", Object::Int(90))).unwrap());
    assert!(tree.iter().zip(&before).all(|(a, b)| a != b), "every page inherits the rotation");
    // Page 2's own content stream (object 8): only page 2.
    let one = changed(&|c| {
        let content = b"BT /F2 12 Tf 10 40 Td (two) Tj ET".to_vec();
        c.set(ObjRef::new(8, 0), pdfcraft_cos::Stream::from_raw(pdfcraft_cos::Dict::new(), content));
    });
    let differ: Vec<usize> = one.iter().zip(&before).enumerate().filter(|(_, (a, b))| a != b).map(|(i, _)| i).collect();
    assert_eq!(differ, [1]);
    // Page 1 given the box it inherits, and its resources by reference instead of directly, as
    // restructuring the page tree does: it draws the same, so its key stays.
    let pushed = changed(&|c| {
        let resources = c.get(ObjRef::new(5, 0)).as_dict().unwrap().get(b"Resources").unwrap().clone();
        let r = c.add(resources);
        c.update_dict(ObjRef::new(5, 0), |d| {
            d.set("MediaBox", Object::Array([0, 0, 600, 800].map(Object::Int).to_vec()));
            d.set("Resources", Object::Ref(r));
        })
        .unwrap();
    });
    assert_eq!(pushed, before);
}

/// A page linking to another (an annotation's `/Dest`) and an annotation pointing back to its
/// page (`/P`) still have a key: another page stands as its number, a cycle ends. A change to
/// the page linked to leaves the linking page's key alone.
#[test]
fn links_and_cycles_do_not_reach_other_pages() {
    use pdfcraft_cos::{ObjRef, Object};
    let mut cos = cos_of(book(3));
    let link = cos.add(Object::Dict({
        let mut d = pdfcraft_cos::Dict::new();
        d.set("Type", Object::name("Annot"));
        d.set("Subtype", Object::name("Link"));
        d.set("P", Object::Ref(ObjRef::new(5, 0)));
        d.set("Dest", Object::Array(vec![Object::Ref(ObjRef::new(7, 0)), Object::name("Fit")]));
        d
    }));
    cos.update_dict(ObjRef::new(5, 0), |d| d.set("Annots", Object::Array(vec![Object::Ref(link)]))).unwrap();
    let before = keys(&cos, false);
    assert!(before[0].is_some());
    cos.set(ObjRef::new(8, 0), pdfcraft_cos::Stream::from_raw(pdfcraft_cos::Dict::new(), b"BT /F2 12 Tf 10 40 Td (two) Tj ET".to_vec()));
    let after = keys(&cos, false);
    assert_eq!(after[0], before[0], "page 1 links to page 2, whose content changed");
    assert_ne!(after[1], before[1]);
}

/// Objects nested past the depth limit leave the page without a key (scanned again after any
/// edit), never with a key that misses a part of it.
#[test]
fn a_page_nested_too_deep_has_no_key() {
    use pdfcraft_cos::{ObjRef, Object};
    let mut cos = cos_of(book(2));
    let mut deep = Object::Null;
    for _ in 0..200 {
        deep = Object::Array(vec![deep]);
    }
    cos.update_dict(ObjRef::new(5, 0), |d| d.set("PieceInfo", deep)).unwrap();
    let k = keys(&cos, false);
    assert_eq!(k[0], None);
    assert!(k[1].is_some(), "the other page is not affected");
}

/// A scan carried across edits (plan A8 P3b4): the pages scanned follow their content to where
/// it is now, and only a page whose content changed is left to read again.
#[test]
fn a_carried_scan_follows_its_pages_and_leaves_the_changed_ones_to_read() {
    let (mut s, id) = open(book(60));
    let then = s.chess_stamp(id).unwrap();
    let first = s.chess_scan(id, &BOARDS, &quick(), true).unwrap();
    let found = boards(&first);
    let shift = |by: usize| found.iter().map(|(p, fen)| (p + by, fen.clone())).collect::<Vec<_>>();

    // A page inserted first moves both boards one on; nothing is left to read.
    s.apply(id, Edit::InsertBlankPage { at: 0, width: 600.0, height: 800.0 }).unwrap();
    let carry = s.chess_carry(id, then, &BOARDS, false, &quick(), true).unwrap();
    assert_eq!(Some(carry.stamp), s.chess_stamp(id));
    assert_eq!((carry.wanted.as_slice(), carry.missing.as_slice()), ([5, 41].as_slice(), [].as_slice()));
    assert_eq!(carry.follow, [(4, Some(5)), (40, Some(41))]);
    assert_eq!(boards(&carry.pages), shift(1));

    // Twice over: the carry is followed from the state it was made in.
    s.apply(id, Edit::InsertBlankPage { at: 0, width: 600.0, height: 800.0 }).unwrap();
    let carry = s.chess_carry(id, carry.stamp, &carry.wanted, false, &quick(), true).unwrap();
    assert_eq!((carry.wanted.as_slice(), carry.missing.as_slice()), ([6, 42].as_slice(), [].as_slice()));
    assert_eq!(boards(&carry.pages), shift(2));

    // Turning the second board's page leaves that page alone to read.
    s.apply(id, Edit::RotatePages { pages: vec![42], degrees: 90 }).unwrap();
    let before = s.chess.scanned;
    let carry = s.chess_carry(id, carry.stamp, &carry.wanted, false, &quick(), true).unwrap();
    assert_eq!((carry.wanted.as_slice(), carry.missing.as_slice()), ([6, 42].as_slice(), [42].as_slice()));
    assert_eq!(carry.follow, [(6, Some(6)), (42, Some(42))], "the turned page stays where it is");
    assert_eq!(boards(&carry.pages), shift(2)[..1]);
    assert_eq!(s.chess.scanned, before, "nothing is scanned by a carry");
}

/// A page removed is gone from a carried scan; the pages after it follow their content.
#[test]
fn a_removed_page_is_gone_from_a_carried_scan() {
    let (mut s, id) = open(book(60));
    let then = s.chess_stamp(id).unwrap();
    let scanned = [4, 10, 40];
    s.chess_scan(id, &scanned, &quick(), true).unwrap();
    s.apply(id, Edit::DeletePages { pages: vec![10] }).unwrap();
    let carry = s.chess_carry(id, then, &scanned, false, &quick(), true).unwrap();
    assert_eq!(carry.follow, [(4, Some(4)), (10, None), (40, Some(39))]);
    assert_eq!((carry.wanted.as_slice(), carry.missing.as_slice()), ([4, 39].as_slice(), [].as_slice()));
    assert_eq!((carry.now_at(10), carry.now_at(40), carry.now_at(20)), (None, Some(39), None), "page 21 may have moved");
    // An undo brings it back where it was.
    let then = carry.stamp;
    s.undo(id).unwrap();
    let carry = s.chess_carry(id, then, &carry.wanted, false, &quick(), true).unwrap();
    assert_eq!(carry.follow, [(4, Some(4)), (39, Some(40))]);
    assert_eq!(carry.now_at(20), None);
}

/// A whole-book scan carried across an edit covers every page now, the changed one left to read.
#[test]
fn a_carried_whole_scan_covers_every_page() {
    let (mut s, id) = open(book(12));
    let then = s.chess_stamp(id).unwrap();
    scan_all(&mut s, id);
    s.apply(id, Edit::RotatePages { pages: vec![4], degrees: 90 }).unwrap();
    s.apply(id, Edit::InsertBlankPage { at: 12, width: 600.0, height: 800.0 }).unwrap();
    let carry = s.chess_carry(id, then, &[], true, &quick(), true).unwrap();
    assert_eq!(carry.wanted, (0..13).collect::<Vec<_>>());
    assert_eq!(carry.missing, [4, 12]);
    assert_eq!(carry.pages.len(), 11);
    // What a scan of the missing pages finds is then all a scan of the book reads again.
    s.chess_scan(id, &carry.missing, &quick(), true).unwrap();
    assert_eq!(scan_all(&mut s, id).1, 0);
}

/// A closed document's scan is not carried, and that is the reason given (plan A9-Q4b, finding
/// A7-18: any error passed).
#[test]
fn a_carry_needs_the_document() {
    let (mut s, id) = open(book(2));
    let then = s.chess_stamp(id).unwrap();
    assert!(s.chess_carry(id, then, &[], true, &quick(), true).is_ok(), "carried while it is open");
    s.close(id);
    assert_eq!(s.chess_carry(id, then, &[], true, &quick(), true).err().as_deref(), Some("no such document"));
}

// ─── Plan A9, G11: the book read once per session ──────────────────────────────────────────────

/// A9-G11 (R8-01): the Text tab's reading, the exports and Link the whole book run in a session
/// of their own, opened from the document's bytes, and read every page again (748 pages of a
/// book after Detect in the book, 304 s instead of 5 s). Given what the app's session read for
/// that state, the job's session reads nothing again; what a job reads goes back, so nothing is
/// read twice.
#[test]
fn a_jobs_session_reads_nothing_the_session_read_and_gives_back_what_it_read() {
    let (mut s, id) = open(book(12));
    assert_eq!(scan_all(&mut s, id).1, 12, "Detect in the book");
    let bytes = s.get(id).unwrap().bytes.to_vec();
    let cached = s.chess_cached(id, &[]);
    assert_eq!(cached.pages(), (0..12).collect::<Vec<_>>());
    // The export's session, opened from the same bytes.
    let (mut job, jid) = open(bytes.clone());
    assert_eq!(job.prime_chess_cache(jid, cached), 12);
    let read = job.chess_document(jid, &[], &quick(), None).unwrap();
    assert_eq!(job.chess_scanned(), 0, "the export reads no page again");
    assert_eq!(read.doc.pages.len(), 12);
    // A job that read what the session had not gives it back: the session reads nothing then.
    let (mut fresh, fid) = open(bytes.clone());
    let (mut other, oid) = open(bytes);
    other.chess_scan(oid, &[2, 4, 5], &quick(), true).unwrap();
    assert_eq!(other.chess_scanned(), 3);
    assert_eq!(fresh.prime_chess_cache(fid, other.chess_cached(oid, &[])), 3);
    let before = fresh.chess_scanned();
    fresh.chess_scan(fid, &[2, 4, 5], &quick(), true).unwrap();
    assert_eq!(fresh.chess_scanned(), before, "given back");
    // After an edit, what the session holds for the state now: the pages that moved are given at
    // their index now, the page inserted is not (it was never read).
    s.apply(id, Edit::InsertBlankPage { at: 0, width: 600.0, height: 800.0 }).unwrap();
    assert_eq!(s.chess_cached(id, &[]).pages(), (1..13).collect::<Vec<_>>());
    assert!(s.chess_cached(DocId(999), &[]).is_empty(), "no such document");
}

/// A9-G11: each document keeps its own [`MAX_CACHED_PAGES`] results, its least recently used
/// page going first: a second long book read in the same session never puts the first one's
/// pages out (they were one list of 2,048 for every document).
#[test]
fn each_document_keeps_its_own_pages() {
    use pdfcraft_chessvision::coords::{PageGeometry, PageRect};
    let page = |p: usize| {
        Arc::new(ChessPage {
            page: p,
            geometry: PageGeometry::new(PageRect::new(0.0, 0.0, 600.0, 800.0), Rotation::R0),
            dpi: MIN_DPI,
            diagrams: Vec::new(),
            readings: None,
            truncated: false,
            error: None,
            notes: Vec::new(),
        })
    };
    let params = ScanParams { generation: 1, bytes: 1, pages: MAX_CACHED_PAGES + 1, settings: quick(), model: None };
    let key = |p: usize| Some(u128::try_from(p).unwrap() + 1);
    let (a, b) = (DocId(1), DocId(2));
    let mut cache = ChessCache::default();
    for p in 0..MAX_CACHED_PAGES {
        cache.put(a, p, key(p), params.clone(), page(p));
    }
    for p in 0..10 {
        cache.put(b, p, key(p), params.clone(), page(p));
    }
    assert_eq!((cache.len_of(a), cache.len_of(b)), (MAX_CACHED_PAGES, 10), "the second book put none of the first's out");
    // The first book's least recently used page goes for its next one: page 1, once page 0 is used.
    assert!(cache.get(a, 0, key(0), &params).is_some());
    cache.put(a, MAX_CACHED_PAGES, key(MAX_CACHED_PAGES), params.clone(), page(MAX_CACHED_PAGES));
    assert_eq!(cache.len_of(a), MAX_CACHED_PAGES);
    assert!(cache.get(a, 1, key(1), &params).is_none() && cache.get(a, 0, key(0), &params).is_some());
    // A page that moved is found by its content key at its new index.
    let moved = cache.get(a, 5000, key(7), &params).unwrap();
    assert_eq!(moved.page, 5000);
    assert_eq!(cache.len_of(b), 10);
    cache.forget(a);
    assert_eq!(cache.len(), 10);
}
