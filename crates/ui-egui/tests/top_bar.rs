//! The mode bar's zoom and page, and the right rail measured (plan A8, P5): no two of the rail's
//! targets overlap however short the window, and the search box gives way to the view's
//! controls in a narrow one.

use egui::accesskit::Role;
use egui::{Rect, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use pdfcraft_ui_egui::PdfCraftApp;

/// `n` pages of 200×300.
fn fixture(n: usize) -> Vec<u8> {
    let mut objs: Vec<String> = vec!["<< /Type /Catalog /Pages 2 0 R >>".into()];
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 4 + 2 * i)).collect();
    objs.push(format!("<< /Type /Pages /Kids [{}] /Count {n} /MediaBox [0 0 200 300] >>", kids.join(" ")));
    objs.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into());
    for i in 0..n {
        objs.push(format!("<< /Type /Page /Parent 2 0 R /Contents {} 0 R /Resources << /Font << /F1 3 0 R >> >> >>", 5 + 2 * i));
        let body = format!("BT /F1 18 Tf 20 150 Td (Page {}) Tj ET", i + 1);
        objs.push(format!("<< /Length {} >>\nstream\n{body}\nendstream", body.len()));
    }
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

fn harness(width: f32, height: f32) -> Harness<'static, PdfCraftApp> {
    let mut h = Harness::builder().with_size(vec2(width, height)).build_eframe(|_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("a.pdf", None, fixture(3)).unwrap();
        app
    });
    h.run_steps(4);
    h
}

const PANELS: [&str; 8] = ["Comments", "Bookmarks", "Page thumbnails", "Form fields", "Layers", "Attachments", "Signatures", "Tags"];

/// The rail's targets on screen: every node with one of its names at its right edge.
fn rail_targets(h: &Harness<'static, PdfCraftApp>, width: f32) -> Vec<(String, Rect)> {
    let mut out = Vec::new();
    let names = PANELS.iter().copied().chain(["More panels", "Next page", "Previous page"]);
    for name in names {
        for node in h.query_all_by_label(name) {
            let r = node.rect();
            if r.min.x >= width - 48.0 {
                out.push((name.to_owned(), r));
            }
        }
    }
    for name in ["Next view (", "Previous view ("] {
        for node in h.query_all_by_label_contains(name) {
            let r = node.rect();
            if r.min.x >= width - 48.0 {
                out.push((name.to_owned(), r));
            }
        }
    }
    out
}

/// Plan A8, P5 (finding A8-V1): at 705 points every panel button fits above the navigation and
/// none overlaps another; in a window too short for them, the rest go under ⋯ and still none
/// overlaps.
#[test]
fn the_rails_targets_never_overlap() {
    for (height, all) in [(705.0, true), (420.0, false)] {
        let h = harness(1366.0, height);
        let targets = rail_targets(&h, 1366.0);
        for (i, (a, ra)) in targets.iter().enumerate() {
            for (b, rb) in targets.iter().skip(i + 1) {
                assert!(!ra.shrink(0.5).intersects(rb.shrink(0.5)), "at {height}: {a} {ra:?} overlaps {b} {rb:?}");
            }
            assert!(ra.max.y <= height, "at {height}: {a} {ra:?} below the window");
        }
        let shown = PANELS.iter().filter(|p| targets.iter().any(|(n, _)| n == *p)).count();
        let more = targets.iter().any(|(n, _)| n == "More panels");
        if all {
            assert_eq!((shown, more), (PANELS.len(), false), "at {height}: {targets:?}");
        } else {
            assert!(more && shown < PANELS.len() && shown > 0, "at {height}: {targets:?}");
        }
        assert_eq!(targets.iter().filter(|(n, _)| n.ends_with("page")).count(), 2, "the navigation stays: {targets:?}");
    }
}

/// The panels under ⋯ open from its menu.
#[test]
fn the_rails_more_menu_opens_the_other_panels() {
    let mut h = harness(1366.0, 420.0);
    h.get_by_label("More panels").click();
    h.run_steps(2);
    h.get_by_label("Tags").click();
    h.run_steps(2);
    assert_eq!(h.state().right, Some(pdfcraft_ui_egui::RightPanel::Tags));
}

/// Plan A8, P5 (spec §2.1): the mode bar has the zoom as a framed field between its buttons, and
/// "Page [1] of 3", or "Page [i] · 1 of 3" where the book prints its own number; in a narrow
/// window the search box becomes its icon and nothing overlaps the mode tabs.
#[test]
fn the_mode_bar_shows_the_zoom_and_the_page() {
    let mut h = harness(1366.0, 705.0);
    let zoom = h.get_by_role_and_label(Role::TextInput, "Zoom").rect();
    assert!(zoom.max.y < 100.0, "in the mode bar: {zoom:?}");
    h.get_by_label("View options");
    h.get_by_label("of 3");
    let search = h.get_by_label("Find tools and commands").rect();
    assert!(search.width() > 100.0, "the search box: {search:?}");
    h.state_mut().apply_edit(pdfcraft_engine::Edit::NumberPages {
        from: 0,
        to: 1,
        style: pdfcraft_engine::LabelStyle::LowerRoman,
        prefix: String::new(),
        first: 1,
    });
    h.run_steps(3);
    h.get_by_label("· 1 of 3");

    let h = harness(900.0, 705.0);
    let search = h.get_by_label("Find tools and commands").rect();
    assert!((28.0..=40.0).contains(&search.width()), "the search box gave way to its icon: {search:?}");
    let zoom = h.get_by_role_and_label(Role::TextInput, "Zoom").rect();
    let tab = h.get_by_label("E-Sign").rect();
    assert!(!zoom.intersects(tab) && zoom.min.x > tab.max.x, "the zoom {zoom:?} clear of the tabs {tab:?}");

    // Longer tab names (Portuguese) leave less: the bar gets compact, then goes, never over a tab.
    for (width, zoom_shown) in [(1000.0, true), (900.0, false)] {
        let mut h = harness(width, 705.0);
        h.state_mut().set_option("language", "pt-br").unwrap();
        h.run_steps(4);
        let tab = h.get_by_label("Assinatura eletrônica").rect();
        let zoom = h.query_by_role_and_label(Role::TextInput, "Zoom").map(|n| n.rect());
        assert_eq!(zoom.is_some(), zoom_shown, "at {width}: {zoom:?}");
        if let Some(zoom) = zoom {
            assert!(zoom.min.x > tab.max.x, "at {width}: the zoom {zoom:?} clear of the tabs {tab:?}");
        }
        let search = h.get_by_label("Localizar ferramentas e comandos").rect();
        assert!(search.min.x > tab.max.x, "at {width}: the search {search:?} clear of the tabs {tab:?}");
    }
}
