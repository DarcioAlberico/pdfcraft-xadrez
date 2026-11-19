//! Chess ▸ Export the book's review… and Import a book's review… (plan A9-G5b, finding A8-19) in
//! the real shell (egui_kittest): a book's diagram correction and games go to "another machine"
//! (other folders, the same book) in one file and come in once; a review of another book is asked
//! about first. The PDF is built here.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use pdfcraft_engine::chess::corrections::{Corrections, Kind, NewCorrection, Original};
use pdfcraft_engine::chess::store::{DirBackend, DocStore};
use pdfcraft_ui_egui::{PdfCraftApp, RecoveryStore, RightPanel};

/// The position the correction keeps.
const FEN: &str = "8/8/8/8/8/8/8/K6k w - - 0 1";

/// Two 600 × 800 pt pages of `text`, `F1` Helvetica.
fn pdf(text: &str) -> Vec<u8> {
    let pages = [format!("BT /F1 24 Tf 60 760 Td ({text}) Tj ET\n"), "BT /F1 11 Tf 60 730 Td (The end.) Tj ET\n".to_owned()];
    let kids: Vec<String> = (0..pages.len()).map(|i| format!("{} 0 R", 4 + 2 * i)).collect();
    let mut objs = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        format!("<< /Type /Pages /Kids [{}] /Count {} /MediaBox [0 0 600 800] >>", kids.join(" "), pages.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_string(),
    ];
    for (i, content) in pages.iter().enumerate() {
        let contents = 5 + 2 * i;
        objs.push(format!("<< /Type /Page /Parent 2 0 R /Contents {contents} 0 R /Resources << /Font << /F1 3 0 R >> >> >>"));
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

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcraft-chess-review-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// One machine: the app on `bytes`, its chess kept in `dir` (the data folder and the recovery
/// folder).
fn machine(dir: &Path, bytes: Vec<u8>) -> Harness<'static, PdfCraftApp> {
    let dir = dir.to_path_buf();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 1000.0)).with_step_dt(0.1).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.run_inline = true;
        app.use_chess_data(dir.join("chess"), None);
        app.enable_recovery(RecoveryStore::new(dir.join("recovery")));
        app.open_bytes("book.pdf", None, bytes).unwrap();
        app
    });
    h.run_steps(4);
    h
}

/// A correction of the diagram at the top of page 1 kept in `dir`'s store for the book `bytes`,
/// as the app keeps one.
fn keep_correction(dir: &Path, bytes: &[u8]) {
    let mut s = pdfcraft_engine::Session::new();
    let id = s.open("book.pdf", None, Arc::new(bytes.to_vec()), None).unwrap();
    let keys = s.get(id).unwrap().doc_keys();
    let fingerprints = s.chess_fingerprints(id).unwrap();
    let store = DocStore::<Corrections, DirBackend>::new(dir.join("chess").join("corrections"));
    let new = NewCorrection {
        page: 0,
        rect: [0.1, 0.1, 0.5, 0.5],
        orig: Some(Original { fen: None, model: None }),
        fen: FEN.to_owned(),
        kind: Kind::Corrected,
    };
    store.update_with(&keys, fingerprints.pages(), |f| f.replace(new, 1, fingerprints.as_ref()).map(|_| ())).unwrap();
}

/// The live corrections the app holds for its document.
fn corrections(h: &mut Harness<'static, PdfCraftApp>) -> usize {
    let (_, id) = h.state().active_ids().unwrap();
    h.state_mut().session.chess_corrections(id).map_or(0, |f| f.live().count())
}

/// The games of the document's list that have moves, by their movetext.
fn games(h: &Harness<'static, PdfCraftApp>) -> Vec<String> {
    let c = h.state().chess.games.collection();
    let mut out: Vec<String> = c.iter().filter(|e| e.moves() > 0).map(|e| e.board.game().movetext()).collect();
    out.sort();
    out
}

/// Import the review `path` through the Chess panel's ⋯ menu.
fn import_from_menu(h: &mut Harness<'static, PdfCraftApp>, path: &Path) {
    h.state_mut().choose_right_panel(Some(RightPanel::Chess));
    h.run_steps(2);
    h.state_mut().pick_override = Some(vec![path.to_string_lossy().into_owned()]);
    h.get_by_label("More chess actions").click();
    h.run_steps(2);
    h.get_by_label("Import a book's review…").click();
    h.run_steps(3);
    h.state_mut().pick_override = None;
}

/// The card's check (plan A9-G5b): a book's correction and its two games, exported on one
/// machine, come in on another (other folders) with the same counts; imported again, nothing
/// doubles. The pack has no PGN text, no path and not the document's name.
#[test]
fn a_books_review_goes_to_another_machine_and_comes_in_once() {
    let book = pdf("Chapter 1");
    let (a, b) = (temp_dir("a"), temp_dir("b"));
    keep_correction(&a, &book);
    let pack = a.join("review.json");
    let mut h = machine(&a, book.clone());
    let pgn = "[Event \"Ruy\"]\n\n1. e4 e5 2. Nf3 *\n\n[Event \"Queen\"]\n\n1. d4 d5 *\n";
    assert_eq!(h.state_mut().import_chess_pgn("ab.pgn", pgn.as_bytes()), 2);
    h.run_steps(3);
    let expected = games(&h);
    assert_eq!(expected.len(), 2);
    h.state_mut().pick_override = Some(vec![pack.to_string_lossy().into_owned()]);
    assert!(h.state_mut().execute("chess.review.export"));
    h.run_steps(3);
    h.state_mut().pick_override = None;
    let said = h.state().said().unwrap_or_default().to_owned();
    assert!(said.contains("(corrections: 1, choices: 0, games: 2)") && said.contains("for your own use"), "{said}");
    let text = std::fs::read_to_string(&pack).unwrap_or_else(|e| panic!("{e}: {said}"));
    let around = a.file_name().unwrap().to_string_lossy().into_owned();
    for absent in ["book.pdf", around.as_str()] {
        assert!(!text.contains(absent), "{absent} in the pack");
    }
    drop(h);

    // Another machine: the same book, nothing kept yet.
    let mut h = machine(&b, book);
    assert_eq!((corrections(&mut h), games(&h).len()), (0, 0));
    import_from_menu(&mut h, &pack);
    let said = h.state().said().unwrap_or_default().to_owned();
    assert!(said.contains("Review imported from review.json (corrections: 1, choices: 0, games: 2)."), "{said}");
    assert_eq!((corrections(&mut h), games(&h)), (1, expected.clone()), "the same review here");
    import_from_menu(&mut h, &pack);
    assert_eq!((corrections(&mut h), games(&h)), (1, expected), "imported twice: nothing doubles");
    let _ = std::fs::remove_dir_all(&a);
    let _ = std::fs::remove_dir_all(&b);
}

/// A review of another book is asked about before anything changes: Cancel imports nothing,
/// Import anyway brings it in.
#[test]
fn another_books_review_is_asked_about_first() {
    let book = pdf("Chapter 1");
    let (a, b) = (temp_dir("other-a"), temp_dir("other-b"));
    keep_correction(&a, &book);
    let pack = a.join("review.json");
    let mut h = machine(&a, book);
    h.state_mut().pick_override = Some(vec![pack.to_string_lossy().into_owned()]);
    assert!(h.state_mut().execute("chess.review.export"));
    h.run_steps(3);
    h.state_mut().pick_override = None;
    assert!(pack.is_file(), "{:?}", h.state().said());
    drop(h);

    let mut h = machine(&b, pdf("Another book, longer than the first"));
    import_from_menu(&mut h, &pack);
    // Asked first, naming the file and both books' pages.
    let asked = h.query_by_label_contains("review.json is the review of another book (2 pages; this one has 2)").is_some();
    assert!(asked, "asked first: {:?}", h.state().said());
    h.get_by_label("Cancel").click();
    h.run_steps(2);
    assert!(h.query_by_label("Another book's review").is_none(), "the question is gone");
    assert_eq!(corrections(&mut h), 0, "Cancel imports nothing");
    import_from_menu(&mut h, &pack);
    h.get_by_label("Import anyway").click();
    h.run_steps(2);
    let said = h.state().said().unwrap_or_default().to_owned();
    assert!(said.contains("(corrections: 1, choices: 0, games: 0)"), "{said}");
    assert_eq!(corrections(&mut h), 1, "kept in this document's store");
    let _ = std::fs::remove_dir_all(&a);
    let _ = std::fs::remove_dir_all(&b);
}
