//! Edit a PDF ▸ Eraser in the real shell (egui_kittest): a box drawn off the text, or a drag
//! across the text, erases what lies under it as one undoable edit.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use pdfcraft_ui_egui::{PdfCraftApp, QuickTool};

/// One 200 × 300 pt page with `Page one` in 24 pt Helvetica at (20, 150), `Keep me` at (20, 60)
/// and a black square from (20, 220) to (60, 260).
fn fixture() -> Vec<u8> {
    let body = "BT /F1 24 Tf 20 150 Td (Page one) Tj ET BT /F1 24 Tf 20 60 Td (Keep me) Tj ET 0 g 20 220 40 40 re f";
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [4 0 R] /Count 1 /MediaBox [0 0 200 300] >>".to_string(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /Contents 5 0 R /Resources << /Font << /F1 3 0 R >> >> >>".to_string(),
        format!("<< /Length {} >>\nstream\n{body}\nendstream", body.len()),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offs = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offs.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let x = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offs {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{x}\n%%EOF\n", objs.len() + 1).as_bytes());
    out
}

fn harness() -> Harness<'static, PdfCraftApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_eframe(|_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("doc.pdf", None, fixture()).unwrap();
        app.set_option("left", "closed").unwrap();
        app.set_option("zoom", "100").unwrap();
        app
    });
    h.run_steps(6);
    // Wait for the page's text layer (up to ~4 s on a loaded machine): a drag across text selects
    // nothing before it arrives. Then let go of the selection that proved it.
    for _ in 0..200 {
        h.run_steps(2);
        if h.state_mut().views[0].select_all() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(h.state().views[0].selected_text().is_some(), "the text layer arrived");
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert_eq!(h.state().views[0].selected_text(), None);
    h
}

fn lines(h: &Harness<'static, PdfCraftApp>) -> Vec<String> {
    let s = h.state();
    s.session.get(s.views[0].id).unwrap().text_lines(0).iter().map(|l| l.text.clone()).collect()
}

fn undo_label(h: &Harness<'static, PdfCraftApp>) -> Option<String> {
    let s = h.state();
    s.session.get(s.views[0].id).unwrap().can_undo().map(str::to_owned)
}

/// A point of page 1 in user space, on screen.
fn at(h: &Harness<'static, PdfCraftApp>, x: f32, y: f32) -> egui::Pos2 {
    let r = h.state().views[0].page_screen_rect(0).unwrap();
    egui::pos2(r.min.x + x / 200.0 * r.width(), r.min.y + (300.0 - y) / 300.0 * r.height())
}

fn drag(h: &mut Harness<'static, PdfCraftApp>, a: egui::Pos2, b: egui::Pos2) {
    h.hover_at(a);
    h.run_steps(1);
    h.drag_at(a);
    h.run_steps(1);
    for k in 1..=4 {
        h.hover_at(a + (b - a) * (k as f32 / 4.0));
        h.run_steps(1);
    }
    h.drop_at(b);
    h.run_steps(4);
}

/// The tool is in Edit a PDF; a box drawn from the margin around `Page one` erases that line
/// and leaves the other; Undo brings it back. (Fails with the Eraser's edit not applied.)
#[test]
fn a_box_drawn_with_the_eraser_erases_what_lies_under_it() {
    let mut h = harness();
    assert!(h.state_mut().execute("edit.erase"));
    assert_eq!(h.state().quick_tool, QuickTool::Erase);
    h.run_steps(2);
    assert_eq!(lines(&h), ["Page one", "Keep me"]);
    let (a, b) = (at(&h, 5.0, 185.0), at(&h, 195.0, 140.0));
    drag(&mut h, a, b);
    assert_eq!(undo_label(&h).as_deref(), Some("Erase content"));
    assert_eq!(lines(&h), ["Keep me"]);
    let s = h.state_mut();
    let id = s.views[0].id;
    s.session.undo(id).unwrap();
    assert_eq!(lines(&h), ["Page one", "Keep me"]);
}

/// A box over a drawing with no text near it erases the drawing: only the box does that, a text
/// selection takes text alone. (Fails with the Eraser not drawing boxes.)
#[test]
fn a_box_over_a_drawing_erases_the_drawing() {
    let mut h = harness();
    assert!(h.state_mut().execute("edit.erase"));
    h.run_steps(2);
    let (a, b) = (at(&h, 10.0, 275.0), at(&h, 75.0, 205.0));
    drag(&mut h, a, b);
    assert_eq!(undo_label(&h).as_deref(), Some("Erase content"));
    assert_eq!(lines(&h), ["Page one", "Keep me"], "the text stays");
    let s = h.state();
    let found =
        s.session.get(s.views[0].id).unwrap().erase_preview(0, &[[0.0, 0.0, 200.0, 300.0]], &pdfcraft_engine::EraseOptions::default()).unwrap();
    assert!(found.iter().all(|f| f[1] < 200.0), "nothing of the square is left: {found:?}");
}

/// A drag across the text selects it, and letting go erases the selection: the words selected,
/// not the box the pointer drew. A selection made first is erased by choosing the Eraser.
#[test]
fn dragging_across_text_with_the_eraser_erases_the_selection() {
    let mut h = harness();
    assert!(h.state_mut().execute("edit.erase"));
    h.run_steps(2);
    // From inside `Keep` to inside `me`: the selection is whole glyphs of that one line.
    let (a, b) = (at(&h, 24.0, 68.0), at(&h, 110.0, 68.0));
    drag(&mut h, a, b);
    assert_eq!(undo_label(&h).as_deref(), Some("Erase content"));
    let after = lines(&h);
    assert_eq!(after.first().map(String::as_str), Some("Page one"), "{after:?}");
    assert!(!after.iter().any(|l| l.contains("eep")), "{after:?}");
}

/// Choosing the Eraser while text is selected erases that selection at once; without one it
/// only says how to use the tool.
#[test]
fn choosing_the_eraser_with_text_selected_erases_it() {
    let mut h = harness();
    let (a, b) = (at(&h, 24.0, 158.0), at(&h, 120.0, 158.0));
    drag(&mut h, a, b);
    assert_eq!(h.state().quick_tool, QuickTool::Select);
    assert!(undo_label(&h).is_none(), "selecting changes nothing");
    assert!(h.state_mut().execute("edit.erase"));
    h.run_steps(3);
    assert_eq!(undo_label(&h).as_deref(), Some("Erase content"));
    assert!(!lines(&h).iter().any(|l| l.contains("age")), "{:?}", lines(&h));
}

/// While the box is drawn, what would go is listed for the red tint; nothing changes until the
/// pointer lets go. (Fails with the preview off.)
#[test]
fn the_box_previews_what_will_go_before_letting_go() {
    let mut h = harness();
    assert!(h.state_mut().execute("edit.erase"));
    h.run_steps(2);
    let (a, b) = (at(&h, 5.0, 185.0), at(&h, 195.0, 140.0));
    h.hover_at(a);
    h.run_steps(1);
    h.drag_at(a);
    h.run_steps(1);
    for k in 1..=4 {
        h.hover_at(a + (b - a) * (k as f32 / 4.0));
        h.run_steps(1);
    }
    let found = h.state().views[0].erase_preview.found.len();
    assert_eq!(found, "Page one".len(), "each glyph of the line, before letting go");
    assert_eq!(undo_label(&h), None, "nothing erased yet");
    h.drop_at(b);
    h.run_steps(4);
    assert_eq!(undo_label(&h).as_deref(), Some("Erase content"));
}

/// The brush rubs: a stroke along `Keep me` erases it in one step, and only it. (Fails with the
/// brush's edit not made.)
#[test]
fn the_brush_erases_what_it_rubs() {
    let mut h = harness();
    assert!(h.state_mut().execute("edit.erase"));
    h.state_mut().erase_prefs.mode = pdfcraft_ui_egui::erase_ui::EraseMode::Brush;
    h.state_mut().erase_prefs.brush = 12.0;
    h.run_steps(2);
    let (a, b) = (at(&h, 25.0, 68.0), at(&h, 110.0, 68.0));
    drag(&mut h, a, b);
    assert_eq!(undo_label(&h).as_deref(), Some("Erase content"));
    let after = lines(&h);
    assert_eq!(after.first().map(String::as_str), Some("Page one"), "{after:?}");
    assert!(!after.iter().any(|l| l.contains("eep")), "{after:?}");
}

/// The selection's action bar has Erase: the words selected with the Select tool go.
#[test]
fn the_selection_bar_erases_the_selected_words() {
    let mut h = harness();
    let (a, b) = (at(&h, 24.0, 158.0), at(&h, 120.0, 158.0));
    drag(&mut h, a, b);
    assert!(h.state().views[0].selected_text().is_some());
    h.get_by_label("Erase").click();
    h.run_steps(4);
    assert_eq!(undo_label(&h).as_deref(), Some("Erase content"));
    assert!(!lines(&h).iter().any(|l| l.contains("age")), "{:?}", lines(&h));
}
