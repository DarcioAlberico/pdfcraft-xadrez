//! Chess ▸ Make the book's bookmarks… (plan A9-N6, finding A8-13) in the real shell
//! (egui_kittest): the whole book read, its chapters and games shown before anything changes,
//! then made under one bookmark in one step that Undo takes back, the PDF as it was. The PDF is
//! built here: two chapters and three games, each under a header as ChessBase prints it.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use pdfcraft_ui_egui::{PdfCraftApp, RightPanel};

mod support;

/// A line of `text` at `top` points from the top of the page, in `size` point Helvetica.
fn helv(top: f32, size: f32, text: &str) -> String {
    format!("BT /F1 {size} Tf 60 {} Td ({text}) Tj ET\n", 800.0 - top)
}

/// The engine's synthetic book of plan A9-N6: a 24 pt heading per page, then its games.
fn book() -> Vec<u8> {
    let page = |chapter: &str, games: &[(&str, &str)]| {
        let mut c = helv(40.0, 24.0, chapter);
        let mut top = 100.0;
        for (players, event) in games {
            c.push_str(&helv(top, 11.0, players));
            c.push_str(&helv(top + 60.0, 11.0, event));
            c.push_str(&helv(top + 120.0, 11.0, "1.e4 e5 2.Nf3 Nc6 3.Bb5 a6"));
            top += 200.0;
        }
        c
    };
    pdf(&[
        page(
            "Chapter 1",
            &[
                ("(1) Fischer,Robert (2760) - Spassky,Boris (2690) [C95]", "Siegen ol (5), 1970"),
                ("(2) Tal,Mikhail - Botvinnik,Mikhail", "Moscow, 1960"),
            ],
        ),
        page("Chapter 2", &[("(3) Carlsen,Magnus - Anand,Viswanathan", "Chennai (9), 2013")]),
    ])
}

/// 600 × 800 pt pages with these contents, `F1` Helvetica.
fn pdf(pages: &[String]) -> Vec<u8> {
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

/// Pin "no piece model" for this test process (plan A9-Q1a): the book has no diagram, but the
/// reading must not depend on the models installed on the machine.
fn no_piece_model() {
    let (none, _) = pdfcraft_engine::chess::pdfcraft_chessvision::models::ChessModels::from_files(Vec::new());
    pdfcraft_engine::chess::install_models(Some(none));
}

fn harness() -> Harness<'static, PdfCraftApp> {
    no_piece_model();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 1000.0)).with_step_dt(0.1).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("book.pdf", None, book()).unwrap();
        app.set_option("left", "closed").unwrap();
        app.run_inline = true;
        app
    });
    h.run_steps(4);
    h
}

/// The document's outline, each bookmark as (depth, title), in order.
fn outline(h: &Harness<'static, PdfCraftApp>) -> Vec<(usize, String)> {
    fn walk(items: &[pdfcraft_render::OutlineItem], depth: usize, out: &mut Vec<(usize, String)>) {
        for i in items {
            out.push((depth, i.title.clone()));
            walk(&i.children, depth + 1, out);
        }
    }
    let (_, id) = h.state().active_ids().unwrap();
    let mut out = Vec::new();
    walk(&h.state().session.get(id).unwrap().info.outline, 0, &mut out);
    out
}

/// The card's check in the app (plan A9-N6): Make the book's bookmarks… from the Chess panel's ⋯
/// menu reads the book and shows its chapters and games before anything changes; leaving the
/// games out shows the chapters alone; Add the bookmarks makes them under one "Chess" bookmark in
/// one step, which Undo takes back, the PDF as it was, byte for byte.
#[test]
fn the_books_bookmarks_are_shown_then_made_in_one_undoable_step() {
    let mut h = harness();
    let (_, id) = h.state().active_ids().unwrap();
    let before = h.state().session.get(id).unwrap().bytes.clone();
    h.state_mut().choose_right_panel(Some(RightPanel::Chess));
    h.run_steps(2);
    h.get_by_label("More chess actions").click();
    h.run_steps(2);
    h.get_by_label("Make the book's bookmarks…").click();
    h.run_steps(2);
    support::wait_until(&mut h, "the book read", |app| app.chess.long.is_none());
    h.run_steps(2);
    h.get_by_label("Chapters: 2 · Games: 3 · Exercises: 0");
    for title in ["Chapter 1", "1. Fischer–Spassky, Siegen ol 1970", "2. Tal–Botvinnik, Moscow 1960", "Chapter 2", "3. Carlsen–Anand, Chennai 2013"]
    {
        assert!(h.query_by_label(title).is_some(), "{title} in the preview");
    }
    assert!(outline(&h).is_empty(), "nothing made before the answer");
    // Without the games, the chapters alone.
    h.get_by_label("Games").click();
    h.run_steps(2);
    h.get_by_label("Chapters: 2 · Games: 0 · Exercises: 0");
    assert!(h.query_by_label("2. Tal–Botvinnik, Moscow 1960").is_none());
    h.get_by_label("Games").click();
    h.run_steps(2);
    h.get_by_label("Add the bookmarks").click();
    h.run_steps(3);
    assert!(h.query_by_label("The book's bookmarks").is_none(), "the preview is closed");
    assert_eq!(
        outline(&h),
        [
            (0, "Chess".to_owned()),
            (1, "Chapter 1".to_owned()),
            (2, "1. Fischer–Spassky, Siegen ol 1970".to_owned()),
            (2, "2. Tal–Botvinnik, Moscow 1960".to_owned()),
            (1, "Chapter 2".to_owned()),
            (2, "3. Carlsen–Anand, Chennai 2013".to_owned()),
        ]
    );
    let said = h.state().toast.as_ref().map(|t| t.0.clone()).unwrap_or_default();
    assert!(said.contains("Bookmarks added under “Chess”: 5."), "{said}");
    assert_eq!(h.state().session.get(id).unwrap().can_undo(), Some("Add chess bookmarks"));
    assert!(h.state_mut().execute("edit.undo"));
    h.run_steps(2);
    assert!(outline(&h).is_empty(), "undone");
    assert!(h.state().session.get(id).unwrap().bytes == before, "the PDF as it was, byte for byte");
}

/// Cancel makes nothing; the bookmark that holds them is named as typed.
#[test]
fn cancel_makes_no_bookmark_and_the_title_can_be_named() {
    let mut h = harness();
    assert!(h.state_mut().execute("chess.bookmarks"));
    h.run_steps(2);
    support::wait_until(&mut h, "the book read", |app| app.chess.long.is_none());
    h.run_steps(2);
    h.get_by_label("Cancel").click();
    h.run_steps(2);
    assert!(h.query_by_label("The book's bookmarks").is_none());
    assert!(outline(&h).is_empty(), "Cancel makes nothing");
    assert!(h.state_mut().execute("chess.bookmarks"));
    h.run_steps(2);
    support::wait_until(&mut h, "the book read", |app| app.chess.long.is_none());
    h.run_steps(2);
    h.get_by_label("Title of the bookmark").focus();
    h.run_steps(1);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(1);
    h.get_by_label("Title of the bookmark").type_text("Xadrez");
    h.run_steps(1);
    h.get_by_label("Add the bookmarks").click();
    h.run_steps(3);
    let top: Vec<String> = outline(&h).into_iter().filter(|(d, _)| *d == 0).map(|(_, t)| t).collect();
    assert_eq!(top, ["Xadrez"], "the bookmark named as typed");
}
