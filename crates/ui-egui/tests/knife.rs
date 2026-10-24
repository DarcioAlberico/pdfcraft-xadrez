//! The Knife in the real shell (egui_kittest): place cuts on a spread with clicks, take one away
//! with a right-click, cut with Enter; Esc lets go of the cuts.

use egui_kittest::Harness;
use pdfcraft_engine::CutAxis;
use pdfcraft_ui_egui::{PdfCraftApp, QuickTool};

/// One 400 × 300 pt spread: `Left page` at the left, `Right page` at the right.
fn fixture() -> Vec<u8> {
    let body = "BT /F1 18 Tf 20 150 Td (Left page) Tj ET BT /F1 18 Tf 230 150 Td (Right page) Tj ET";
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [4 0 R] /Count 1 /MediaBox [0 0 400 300] >>".to_string(),
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
        app.open_bytes("spread.pdf", None, fixture()).unwrap();
        app.set_option("left", "closed").unwrap();
        app.set_option("zoom", "100").unwrap();
        app
    });
    h.run_steps(6);
    h
}

/// A share of page 1's width and height, on screen.
fn at(h: &Harness<'static, PdfCraftApp>, u: f32, v: f32) -> egui::Pos2 {
    let r = h.state().views[0].page_screen_rect(0).unwrap();
    egui::pos2(r.min.x + u * r.width(), r.min.y + v * r.height())
}

fn click(h: &mut Harness<'static, PdfCraftApp>, p: egui::Pos2, button: egui::PointerButton, modifiers: egui::Modifiers) {
    h.hover_at(p);
    h.run_steps(2);
    for pressed in [true, false] {
        h.event_modifiers(egui::Event::PointerButton { pos: p, button, pressed, modifiers }, modifiers);
    }
    h.run_steps(4);
}

fn pages(h: &Harness<'static, PdfCraftApp>) -> Vec<(f32, f32)> {
    let s = h.state();
    s.session.get(s.views[0].id).unwrap().info.pages.iter().map(|p| (p.width, p.height)).collect()
}

/// A click near the middle snaps to it; a right-click takes the nearest cut away; Enter cuts the
/// spread into two pages, one undo step; Esc lets go of cuts placed. (Fails with clicks not placing
/// cuts, or Enter not cutting.)
#[test]
fn clicks_place_cuts_and_enter_cuts_the_page() {
    let mut h = harness();
    assert!(h.state_mut().execute("page.knife"));
    assert_eq!(h.state().quick_tool, QuickTool::Knife);
    // The tool panel opens and the page moves: measure it afterwards.
    h.run_steps(4);
    let p = at(&h, 0.505, 0.8);
    click(&mut h, p, egui::PointerButton::Primary, egui::Modifiers::NONE);
    assert_eq!(h.state().views[0].knife.at, [0.5], "snapped to the middle");
    let p = at(&h, 0.25, 0.8);
    click(&mut h, p, egui::PointerButton::Primary, egui::Modifiers::NONE);
    assert_eq!(h.state().views[0].knife.at.len(), 2);
    let p = at(&h, 0.27, 0.8);
    click(&mut h, p, egui::PointerButton::Secondary, egui::Modifiers::NONE);
    assert_eq!(h.state().views[0].knife.at, [0.5], "the nearest cut went");
    h.key_press(egui::Key::Enter);
    h.run_steps(4);
    assert_eq!(pages(&h), [(200.0, 300.0), (200.0, 300.0)]);
    let s = h.state();
    assert_eq!(s.session.get(s.views[0].id).unwrap().can_undo(), Some("Cut page"));
    assert!(s.views[0].knife.at.is_empty(), "the cuts were used");
    // Esc lets go of cuts placed.
    let p = at(&h, 0.5, 0.5);
    click(&mut h, p, egui::PointerButton::Primary, egui::Modifiers::NONE);
    assert!(!h.state().views[0].knife.at.is_empty());
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert!(h.state().views[0].knife.at.is_empty());
}

/// With Shift held the cut runs across: the page is cut into a top and a bottom part.
#[test]
fn shift_cuts_across() {
    let mut h = harness();
    assert!(h.state_mut().execute("page.knife"));
    h.run_steps(4);
    let p = at(&h, 0.3, 0.5);
    click(&mut h, p, egui::PointerButton::Primary, egui::Modifiers::SHIFT);
    assert_eq!(h.state().views[0].knife.axis, Some(CutAxis::Horizontal));
    h.key_press(egui::Key::Enter);
    h.run_steps(4);
    assert_eq!(pages(&h), [(400.0, 150.0), (400.0, 150.0)]);
}
