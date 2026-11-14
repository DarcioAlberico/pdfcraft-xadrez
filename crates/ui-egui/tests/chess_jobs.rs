//! Chess jobs and the session's scans (plan A9, G11; finding R8-01): the export, Link the whole
//! book and the Text tab read in a session of their own, opened from the document's bytes. They
//! are given what the app's session read for the document as it is, and what they read anew is
//! given back, so the book is read once per session (an export after Detect in the book used to
//! read all 748 pages of a real book again: 304 s instead of 5 s).

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use pdfcraft_engine::chess::export::ChessFormat;
use pdfcraft_ui_egui::PdfCraftApp;

#[path = "common/gpu.rs"]
mod gpu;

/// The start position in the Chess Merida layout, top rank first.
const MERIDA_START: [&str; 8] = ["tMvWlVmT", "OoOoOoOo", " + + + +", "+ + + + ", " + + + +", "+ + + + ", "pPpPpPpP", "RnBqKbNr"];

/// A 600 × 800 pt page with the start position in a diagram font, then a blank page: a book of
/// two pages.
fn book() -> Vec<u8> {
    let mut content = String::from("BT /F1 20 Tf 60 724 Td ");
    for (i, line) in MERIDA_START.iter().enumerate() {
        if i > 0 {
            content.push_str("0 -20 Td ");
        }
        content.push_str(&format!("({line}) Tj "));
    }
    content.push_str("ET\n");
    let widths = vec!["1000"; 95].join(" ");
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 600 800] >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        format!("<< /Type /Font /Subtype /Type1 /BaseFont /ChessMerida /FirstChar 32 /LastChar 126 /Widths [{widths}] >>"),
    ];
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
    let mut s = pdfcraft_engine::Session::new();
    let id = s.open("book.pdf", None, std::sync::Arc::new(out), None).unwrap();
    s.apply(id, pdfcraft_engine::Edit::InsertBlankPage { at: 1, width: 600.0, height: 800.0 }).unwrap();
    s.save_bytes(id).unwrap().to_vec()
}

/// The app with [`book`] open, its jobs run inline, and no piece model whatever the machine has.
fn harness() -> Harness<'static, PdfCraftApp> {
    let (none, _) = pdfcraft_engine::chess::pdfcraft_chessvision::models::ChessModels::from_files(Vec::new());
    pdfcraft_engine::chess::install_models(Some(none));
    let bytes = book();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 1000.0)).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("book.pdf", None, bytes).unwrap();
        app.run_inline = true;
        app
    });
    h.run_steps(4);
    h
}

/// A folder of its own under the temp folder, empty.
fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcraft-chess-jobs-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A9-G11 (R8-01): exporting after Detect in the book reads no page again, and what an export
/// reads is kept, so the next export reads nothing.
#[test]
fn a_book_detected_or_exported_once_is_not_read_again_by_the_next_export() {
    let dir = temp_dir("export");
    let epub = dir.join("book.epub");
    // Detect in the book, then Export.
    let mut h = harness();
    assert!(h.state_mut().execute("chess.scan"));
    h.run_steps(4);
    assert_eq!(h.state().chess.results().map(|r| r.count()), Some(1), "the diagram is found");
    let id = h.state().views[0].id;
    h.state_mut().export_chess_to(id, ChessFormat::Epub, &epub);
    h.run_steps(4);
    assert!(epub.is_file(), "exported");
    assert_eq!(h.state().chess.job_scanned, Some(0), "the export read again what Detect read");
    // Without Detect: the first export reads both pages, the next one none.
    let mut h = harness();
    let id = h.state().views[0].id;
    h.state_mut().export_chess_to(id, ChessFormat::Epub, &epub);
    h.run_steps(4);
    assert_eq!(h.state().chess.job_scanned, Some(2));
    h.state_mut().export_chess_to(id, ChessFormat::Pgn, &dir.join("book.pgn"));
    h.run_steps(4);
    assert_eq!(h.state().chess.job_scanned, Some(0), "the second export read again what the first read");
    // An edit in between: only the page it changed is read again.
    h.state_mut().session.apply(id, pdfcraft_engine::Edit::RotatePages { pages: vec![1], degrees: 90 }).unwrap();
    h.run_steps(2);
    h.state_mut().export_chess_to(id, ChessFormat::Epub, &epub);
    h.run_steps(4);
    assert_eq!(h.state().chess.job_scanned, Some(1));
    let _ = std::fs::remove_dir_all(&dir);
}

/// [`book`] with its two pages copied until it has `pages` (a power of two from 2): a book long
/// enough that aligning it to its PGN takes seconds.
fn long_book(pages: usize) -> Vec<u8> {
    let mut s = pdfcraft_engine::Session::new();
    let id = s.open("book.pdf", None, std::sync::Arc::new(book()), None).unwrap();
    let mut n = 2;
    while n < pages {
        s.apply(id, pdfcraft_engine::Edit::DuplicatePages { pages: (0..n).collect(), at: None }).unwrap();
        n *= 2;
    }
    s.save_bytes(id).unwrap().to_vec()
}

/// Plan A9-I2 (findings A5-12, A5-03): a long job has a slot of its own. While Align the book to
/// the PGN runs on a worker, the Text tab still reads the page shown (it used to wait, silently,
/// for the whole book); the job says which page it is on, and × stops it, the book's link as it
/// was.
#[test]
fn a_long_job_never_holds_the_page_read_and_stops_when_asked() {
    let dir = temp_dir("long");
    let (none, _) = pdfcraft_engine::chess::pdfcraft_chessvision::models::ChessModels::from_files(Vec::new());
    pdfcraft_engine::chess::install_models(Some(none));
    let (bytes, store) = (long_book(256), dir.join("reference"));
    let mut h = Harness::builder().with_size(egui::vec2(1366.0, 768.0)).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.chess.book_pgns.use_folder(store);
        app.open_bytes("book.pdf", None, bytes).unwrap();
        app.run_inline = true;
        app
    });
    h.run_steps(4);
    h.state_mut().choose_right_panel(Some(pdfcraft_ui_egui::RightPanel::Chess));
    h.run_steps(2);
    h.get_by_label("Text").click();
    h.run_steps(4);
    let id = h.state().views[0].id;
    h.state_mut().set_book_pgn(id, "book.pgn", b"[Event \"Book\"]\n\n1.e4 e5 *\n");
    h.run_steps(4);
    h.get_by_label("Recognised text of page 1");
    // From here the jobs run on workers.
    h.state_mut().run_inline = false;
    h.get_by_label("Align the book to the PGN").click();
    h.run_steps(1);
    assert!(h.state().chess.long.is_some(), "the book is being aligned on a worker");
    // The page shown is to be read again (as after an edit): the long job does not hold it.
    h.state_mut().chess.docs.get_mut(&id).unwrap().text = None;
    h.run_steps(1);
    assert!(h.state().chess.long.is_some(), "the book is still being aligned");
    assert!(h.state().chess.reading.is_some() || h.state().chess.text().is_some(), "the page is read while the book is aligned");
    // The page's job is waited for, not the clock (plan A9-Q4b, finding A7-15).
    assert!(
        h.state().chess.reading.as_ref().is_none_or(|job| job.wait(std::time::Duration::from_secs(60))),
        "the page is read on a worker while the book is aligned"
    );
    h.run_steps(1);
    assert!(h.state().chess.text().is_some(), "the page was read while the book is aligned");
    assert!(h.state().chess.long.is_some(), "the book is still being aligned");
    h.run_steps(1);
    h.get_by_label("Recognised text of page 1");
    h.get_by_label_contains("Aligning the book to the PGN: page ");
    // With PDFCRAFT_CHESS_SHOTS set, a picture of the Text tab with the long job's row.
    if let Ok(shots) = std::env::var("PDFCRAFT_CHESS_SHOTS") {
        let _gpu = gpu::gpu();
        h.render().unwrap().save(std::path::Path::new(&shots).join("i2-long-job-1366.png")).unwrap();
    }
    h.get_by_label("Stop").click();
    h.run_steps(2);
    assert!(h.state().chess.long.is_none(), "stopped at once");
    assert_eq!(h.state().toast.as_ref().map(|t| t.0.as_str()), Some("Aligning was stopped: the book's link is as it was."));
    // Nothing linked: Align again would say the book was.
    h.get_by_label("Align the book to the PGN");
    let _ = std::fs::remove_dir_all(dir);
}

/// The pages done of the scan running, and of how many.
fn scan_done(h: &Harness<'static, PdfCraftApp>) -> Option<(usize, usize)> {
    h.state().chess.run.as_ref().map(|r| {
        let p = r.progress.lock().unwrap();
        (p.done, p.total)
    })
}

/// Pages of the active document's list, and the diagrams on them.
fn listed(h: &Harness<'static, PdfCraftApp>) -> (usize, usize) {
    h.state().chess.results().map_or((0, 0), |r| (r.pages.len(), r.pages.iter().map(|p| p.diagrams.len()).sum()))
}

/// [`long_book`] open, its jobs inline, a scan run a page at a time as the web runs it (a frame's
/// budget of `budget_ms`).
fn web_harness(pages: usize, budget_ms: f64) -> Harness<'static, PdfCraftApp> {
    let (none, _) = pdfcraft_engine::chess::pdfcraft_chessvision::models::ChessModels::from_files(Vec::new());
    pdfcraft_engine::chess::install_models(Some(none));
    let bytes = long_book(pages);
    let mut h = Harness::builder().with_size(egui::vec2(1366.0, 768.0)).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("book.pdf", None, bytes).unwrap();
        app.run_inline = true;
        app.chess.frame_budget_ms = Some(budget_ms);
        app
    });
    h.run_steps(4);
    h
}

/// Plan A9-I2 (finding R8-03): scanned as the web scans, a page at a time within a frame's
/// budget, Detect in the whole book goes on across frames, says how far it is and stops when
/// asked, the pages scanned kept. It used to scan the whole book inside one frame: the tab froze
/// and Stop could not be clicked.
#[test]
fn the_web_scans_the_book_a_page_at_a_time_and_stops_when_asked() {
    // A frame's budget of nothing: one page a frame.
    let mut h = web_harness(8, 0.0);
    h.state_mut().start_chess_scan(true);
    let first = scan_done(&h).expect("the scan runs on").0;
    assert!((1..8).contains(&first), "a page at a time, not the book in one frame: {first} of 8");
    h.step();
    let next = scan_done(&h).expect("the scan runs on").0;
    assert!(next > first && next < 8, "on in the next frame: {next} of 8");
    h.state_mut().cancel_chess_scan();
    h.step();
    assert!(h.state().chess.run.is_none(), "stopped");
    let (pages, _) = listed(&h);
    assert!(pages >= next && pages < 8, "the pages scanned stay: {pages}");
    // Again, to the end, across frames.
    h.state_mut().start_chess_scan(true);
    let mut frames = 0;
    while h.state().chess.run.is_some() && frames < 100 {
        h.step();
        frames += 1;
    }
    assert!(frames >= 2, "the rest across frames: {frames}");
    assert_eq!(listed(&h), (8, 4), "every page, a diagram on every other");
}

/// Plan A9-I2: until the whole book's Export and Align run a page at a time too, the web refuses
/// them above 20 pages, with the reason; a short book is exported.
#[test]
fn the_web_refuses_a_whole_book_export_above_twenty_pages() {
    let dir = temp_dir("web-export");
    let mut h = web_harness(32, 50.0);
    let id = h.state().views[0].id;
    h.state_mut().export_chess_to(id, ChessFormat::Pgn, &dir.join("long.pgn"));
    h.run_steps(2);
    let said = h.state().toast.as_ref().map(|t| t.0.clone()).unwrap_or_default();
    assert!(said.starts_with("In the browser, a book of more than 20 pages can't be exported"), "{said}");
    assert!(h.state().chess.long.is_none() && !dir.join("long.pgn").exists(), "nothing exported");
    let mut h = web_harness(16, 50.0);
    let id = h.state().views[0].id;
    h.state_mut().export_chess_to(id, ChessFormat::Pgn, &dir.join("short.pgn"));
    h.run_steps(2);
    assert!(dir.join("short.pgn").exists(), "a book of 16 pages is exported");
    let _ = std::fs::remove_dir_all(dir);
}
