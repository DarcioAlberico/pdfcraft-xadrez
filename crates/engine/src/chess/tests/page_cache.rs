//! The scan cache across edits (plan A8 P3a2): an edit reads again only the pages it touched,
//! pages moved by inserting or removing pages are found at their new index, and an undo reads
//! nothing again. Counted with `ChessCache::scanned`, on a book of 703 synthetic pages.

use super::*;

/// The book's length, as the plan's measure has it ("editing page 5 of 703 reads 1 page").
const PAGES: usize = 703;
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
    let (mut s, id) = open(book(PAGES));
    let (first, scanned) = scan_all(&mut s, id);
    assert_eq!((first.len(), scanned), (PAGES, PAGES));
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
    assert_eq!((shifted.len(), scanned), (PAGES + 1, 1));
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

/// The keys of every page of `cos`, each from a fresh memo when `fresh`, else from one shared.
fn keys(cos: &pdfcraft_cos::Document, fresh: bool) -> Vec<Option<page_key::PageKey>> {
    let pages = page_objects(cos, pdfcraft_organize::pages(cos).unwrap().len());
    let mut shared = page_key::KeyMemo::default();
    pages.iter().map(|p| if fresh { page_key::KeyMemo::default().page(cos, *p) } else { shared.page(cos, *p) }).collect()
}

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
