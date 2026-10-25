//! The Knife in the real shell (egui_kittest): place cuts on a spread with clicks, take one away
//! with a right-click, cut with Enter; Esc lets go of the cuts.

use egui_kittest::Harness;
use pdfcraft_engine::CutAxis;
use pdfcraft_ui_egui::{PdfCraftApp, QuickTool};

/// One 400 × 300 pt spread: `Left page` at the left, `Right page` at the right, with no text
/// between 120 and 280 pt, so the gap between them is the middle.
fn fixture() -> Vec<u8> {
    let body = "BT /F1 18 Tf 20 150 Td (Left page) Tj ET BT /F1 18 Tf 290 150 Td (Right page) Tj ET";
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

/// Split double pages cuts every page wider than tall at the gap between its columns, as one undo
/// step; with Remove what lies outside each part, each half keeps only its own text. (Fails with
/// the trim not passed on.)
#[test]
fn split_double_pages_cuts_spreads_and_trims_them() {
    let mut h = harness();
    h.state_mut().knife_prefs.trim_outside = true;
    assert!(h.state_mut().execute("page.split_spreads"));
    h.run_steps(4);
    assert_eq!(pages(&h), [(200.0, 300.0), (200.0, 300.0)]);
    let s = h.state();
    let doc = s.session.get(s.views[0].id).unwrap();
    assert_eq!(doc.can_undo(), Some("Cut page"));
    let text = |p: usize| doc.text_lines(p).iter().map(|l| l.text.clone()).collect::<Vec<_>>().join(" ");
    assert!(text(0).contains("Left page") && !text(0).contains("Right"), "{}", text(0));
    assert!(text(1).contains("Right page") && !text(1).contains("Left"), "{}", text(1));
}

/// A cut once placed is dragged to where it should be. (Fails with dragging off.)
#[test]
fn a_placed_cut_is_dragged() {
    let mut h = harness();
    assert!(h.state_mut().execute("page.knife"));
    h.run_steps(4);
    let p = at(&h, 0.5, 0.8);
    click(&mut h, p, egui::PointerButton::Primary, egui::Modifiers::NONE);
    assert_eq!(h.state().views[0].knife.at, [0.5]);
    let (a, b) = (at(&h, 0.5, 0.8), at(&h, 0.25, 0.8));
    h.hover_at(a);
    h.run_steps(2);
    h.drag_at(a);
    h.run_steps(1);
    for k in 1..=4 {
        h.hover_at(a + (b - a) * (k as f32 / 4.0));
        h.run_steps(1);
    }
    h.drop_at(b);
    h.run_steps(3);
    let at_now = h.state().views[0].knife.at.clone();
    assert_eq!(at_now.len(), 1, "{at_now:?}");
    assert!((at_now[0] - 0.25).abs() < 0.01, "{at_now:?}");
}

/// The Knife's choices round-trip through the settings; anything else keeps the defaults.
#[test]
fn knife_choices_are_kept() {
    use pdfcraft_ui_egui::knife::{KnifePages, KnifePrefs};
    let p = KnifePrefs { axis: CutAxis::Horizontal, pages: KnifePages::All, right_to_left: true, trim_outside: true };
    assert_eq!(KnifePrefs::from_json(&p.to_json()), Some(p));
    assert_eq!(KnifePrefs::from_json(&serde_json::json!({ "axis": 3, "pages": "some" })), Some(KnifePrefs::default()));
    assert_eq!(KnifePrefs::from_json(&serde_json::json!([])), None);
}

/// A spread with no text layer (lines drawn as bars, as a scan's would show) whose gap between
/// blocks is off the middle: Split double pages finds it in the page's picture. (Fails with the
/// picture not looked at: it would cut at the middle.)
#[test]
fn split_double_pages_finds_a_scans_gutter() {
    // Bars of "text" from 20 to 140 pt and from 200 to 380 pt: the gap is 140-200, its middle 170.
    let mut body = String::from(
        "0 g
",
    );
    for y in (20..280).step_by(10) {
        body.push_str(&format!(
            "20 {y} 120 3 re f 200 {y} 180 3 re f
"
        ));
    }
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 400 300] >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".to_string(),
        format!(
            "<< /Length {} >>
stream
{body}
endstream",
            body.len()
        ),
    ];
    let mut out = b"%PDF-1.7
"
    .to_vec();
    let mut offs = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offs.push(out.len());
        out.extend_from_slice(
            format!(
                "{} 0 obj
{o}
endobj
",
                i + 1
            )
            .as_bytes(),
        );
    }
    let x = out.len();
    out.extend_from_slice(
        format!(
            "xref
0 {}
0000000000 65535 f 
",
            objs.len() + 1
        )
        .as_bytes(),
    );
    for o in offs {
        out.extend_from_slice(
            format!(
                "{o:010} 00000 n 
"
            )
            .as_bytes(),
        );
    }
    out.extend_from_slice(
        format!(
            "trailer
<< /Size {} /Root 1 0 R >>
startxref
{x}
%%EOF
",
            objs.len() + 1
        )
        .as_bytes(),
    );
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("scan.pdf", None, out.clone()).unwrap();
        app
    });
    h.run_steps(4);
    assert!(h.state_mut().execute("page.split_spreads"));
    h.run_steps(4);
    let widths: Vec<f32> = pages(&h).iter().map(|p| p.0).collect();
    assert_eq!(widths.len(), 2);
    assert!((widths[0] - 170.0).abs() < 4.0, "cut in the gap, not at the middle: {widths:?}");
}
