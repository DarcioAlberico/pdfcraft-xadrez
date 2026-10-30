use std::collections::HashMap;
use std::sync::Arc;

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use pdfcraft_engine::chess::corrections::Kind;
use pdfcraft_engine::chess::pdfcraft_chessvision::coords::PageRect;
use pdfcraft_engine::chess::pdfcraft_chessvision::detect::{DiagramCandidate, Source};
use pdfcraft_engine::chess::pdfcraft_chessvision::merge::Merged;

use super::*;
use crate::PdfCraftApp;
use crate::chess_ui::ModelNote;

const PAGES: usize = 100;
const PER_PAGE: usize = 20;
const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

/// A PDF of `n` blank pages.
fn blank_pages(n: usize) -> Vec<u8> {
    let mut objs: Vec<String> = vec!["<< /Type /Catalog /Pages 2 0 R >>".into()];
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 3 + i)).collect();
    objs.push(format!("<< /Type /Pages /Kids [{}] /Count {n} /MediaBox [0 0 600 800] >>", kids.join(" ")));
    objs.extend((0..n).map(|_| "<< /Type /Page /Parent 2 0 R >>".to_string()));
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

/// `PER_PAGE` diagrams on each of `PAGES` pages, none read; the first of each page verified and
/// the second corrected.
fn synthetic(app: &PdfCraftApp) -> ChessResults {
    let id = app.active_ids().expect("a document").1;
    let stamp = app.session.chess_stamp(id).expect("its state");
    let diagram = |i: usize| {
        let y = 20.0 + 35.0 * i as f32;
        let bbox_user = PageRect { x0: 20.0, y0: y, x1: 50.0, y1: y + 30.0 };
        let candidate = DiagramCandidate { bbox_user, source: Source::Vector, score: 1.0, dpi_hint: 150.0, placement: None, corners: None };
        Merged { candidate, sources: vec![Source::Vector] }
    };
    let pages = (0..PAGES)
        .map(|page| {
            let diagrams = (0..PER_PAGE).map(diagram).collect();
            Arc::new(ChessPage { page, geometry: None, dpi: 150.0, diagrams, readings: None, truncated: false, error: None, notes: Vec::new() })
        })
        .collect();
    let mut kinds = HashMap::new();
    for page in 0..PAGES {
        kinds.insert((page, 0), (Kind::Verified, START.to_owned()));
        kinds.insert((page, 1), (Kind::Corrected, START.to_owned()));
    }
    ChessResults {
        doc: id,
        stamp,
        pages,
        selected: None,
        reveal: false,
        model: ModelNote::Loaded,
        kinds,
        confirmed: Default::default(),
        wanted: (0..PAGES).collect(),
        all: true,
    }
}

/// The app on a 100-page book with its Chess diagrams panel open, its 2,000 diagrams listed
/// (`diagrams`) or none.
fn harness(diagrams: bool) -> Harness<'static, PdfCraftApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 1000.0)).build_eframe(|_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").expect("English");
        app.open_bytes("book.pdf", None, blank_pages(PAGES)).expect("the book opens");
        app
    });
    h.run_steps(4);
    if diagrams {
        let results = synthetic(h.state());
        h.state_mut().chess.results = Some(results);
    }
    h.state_mut().choose_right_panel(Some(crate::RightPanel::Chess));
    h.run_steps(3);
    h
}

/// The diagram rows built, by name.
fn rows(h: &Harness<'static, PdfCraftApp>) -> Vec<String> {
    h.query_all_by(|n| n.label().is_some_and(|l| l.starts_with("Diagram ") && l.contains(" · "))).filter_map(|n| n.accesskit_node().label()).collect()
}

/// Press `key` as the desktop app gets it: through `raw_input_hook` (kittest skips it).
fn key(h: &mut Harness<'static, PdfCraftApp>, key: Key) {
    use eframe::App as _;
    let ctx = h.ctx.clone();
    for pressed in [true, false] {
        let mut raw = std::mem::take(h.input_mut());
        raw.events.push(Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::NONE });
        h.state_mut().raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }
    h.run_steps(3);
}

/// 2,000 diagrams: the chips count each state, only the rows in sight are built, and a chip shows
/// one state only, or the page on screen only.
#[test]
fn a_long_list_builds_only_what_is_in_sight_and_filters_by_state() {
    let mut h = harness(true);
    h.get_by_label("All 2000");
    h.get_by_label("Verified 100");
    h.get_by_label("Corrected 100");
    h.get_by_label("Not read 1800");
    assert!(h.query_by_label_contains("To review").is_none(), "no chip for a state no diagram has");
    let built = rows(&h);
    assert!(!built.is_empty() && built.len() < 60, "{} rows built of 2000", built.len());
    assert_eq!(built.first().map(String::as_str), Some("Diagram 1 · Verified"));
    assert!(h.query_by_label(START).is_none(), "the FEN is not on the row");
    h.get_by_label("Page 1");

    h.get_by_label("Verified 100").click();
    h.run_steps(2);
    let built = rows(&h);
    assert!(!built.is_empty() && built.iter().all(|r| r == "Diagram 1 · Verified"), "{built:?}");
    h.get_by_label("Verified 100").click();
    h.run_steps(2);
    h.get_by_label("On this page").click();
    h.run_steps(2);
    h.get_by_label("All 20");
    // Page 2 on screen: only its diagrams are listed, under its header.
    h.state_mut().views[0].go_to_page(1);
    h.run_steps(4);
    h.get_by_role_and_label(egui::accesskit::Role::Link, "Page 2");
    assert!(h.query_by_role_and_label(egui::accesskit::Role::Link, "Page 1").is_none(), "only the page on screen");
}

/// Follow the page brings the page shown to the top of the list.
#[test]
fn the_list_follows_the_page() {
    let mut h = harness(true);
    assert!(h.query_by_role_and_label(egui::accesskit::Role::Link, "Page 50").is_none(), "far down the list");
    h.get_by_label("Follow the page").click();
    h.run_steps(2);
    h.state_mut().views[0].go_to_page(49);
    h.run_steps(4);
    h.get_by_role_and_label(egui::accesskit::Role::Link, "Page 50");
    // Not following, a page's header goes to it. (Clicked through AccessKit: the pointer that
    // clicked Follow the page left its tooltip over the page.)
    h.get_by_label("Follow the page").click_accesskit();
    h.run_steps(2);
    assert!(!h.state().chess.list.follow);
    h.state_mut().views[0].go_to_page(0);
    h.run_steps(2);
    h.get_by_role_and_label(egui::accesskit::Role::Link, "Page 50").click_accesskit();
    h.run_steps(6);
    assert_eq!(h.state().views[0].current, 49);
}

/// With the keyboard on a row, ↓ goes to the next diagram, End to the last of the book (its row
/// built once it is scrolled to) and Home back.
#[test]
fn the_keys_walk_the_whole_list() {
    let mut h = harness(true);
    h.get_by_label("Diagram 1 · Verified").focus();
    h.run_steps(2);
    assert_eq!(h.state().chess.list.focused, Some((0, 0)));
    key(&mut h, Key::ArrowDown);
    key(&mut h, Key::ArrowDown);
    assert_eq!(h.state().chess.list.focused, Some((0, 2)), "two rows down");
    key(&mut h, Key::End);
    assert_eq!(h.state().chess.list.focused, Some((PAGES - 1, PER_PAGE - 1)), "the book's last diagram");
    key(&mut h, Key::Home);
    assert_eq!(h.state().chess.list.focused, Some((0, 0)));
    // Tab goes to the row's ⋯, and Enter opens its menu.
    h.key_press(Key::Tab);
    h.run_steps(2);
    assert!(h.get_by_label("Diagram actions").is_focused());
    h.key_press(Key::Enter);
    h.run_steps(2);
    h.get_by_label("Copy FEN");
    h.get_by_label("Open on the board");
}

/// Plan A8 P2a's measure: what 2,000 diagrams add to a frame, at most 4 ms. A measurement, so not
/// run by default: `cargo test -p pdfcraft-ui-egui --lib chess_list -- --ignored --nocapture`.
#[test]
#[ignore = "a timing measurement, run by hand"]
fn two_thousand_diagrams_add_at_most_4_ms_to_a_frame() {
    let mean = |h: &mut Harness<'static, PdfCraftApp>| {
        h.run_steps(10);
        let start = std::time::Instant::now();
        for _ in 0..60 {
            h.step();
        }
        start.elapsed().as_secs_f64() * 1000.0 / 60.0
    };
    let without = mean(&mut harness(false));
    let with = mean(&mut harness(true));
    eprintln!("frame without diagrams {without:.2} ms, with 2000 {with:.2} ms, added {:.2} ms", with - without);
    assert!(with - without <= 4.0, "{:.2} ms added", with - without);
}

/// Plan "Carregar PGN do livro", R6b: a diagram whose reading the book's PGN has leaves the review
/// queue and counts as reviewed.
#[test]
fn a_diagram_the_books_pgn_has_leaves_the_review_queue() {
    let h = harness(false);
    let mut results = synthetic(h.state());
    let (queue, reviewed) = (results.review_queue(), results.reviewed());
    assert!(queue.contains(&(0, 2)), "not read: to review");
    results.confirmed.insert((0, 2));
    assert!(!results.review_queue().contains(&(0, 2)));
    assert_eq!(results.review_queue().len(), queue.len() - 1);
    assert_eq!(results.reviewed(), (reviewed.0 + 1, reviewed.1));
}
