//! Board themes (plan HC4): a theme picked in Preferences, in the board panel's menu or through
//! the control channel's `board-theme` is used at once and kept with the preferences.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use pdfcraft_ui_egui::PdfCraftApp;

const PDF: &[u8] = b"%PDF-1.7
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 300 400] >> endobj
3 0 obj << /Type /Page /Parent 2 0 R >> endobj
trailer << /Root 1 0 R >>
%%EOF";

/// The app with a page open, after `settings` were restored; Preferences shown when `prefs`,
/// else the Board panel.
fn harness(settings: &str, prefs: bool) -> Harness<'static, PdfCraftApp> {
    let settings = settings.to_owned();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 1000.0)).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.restore(&settings);
        app.set_option("language", "en").unwrap();
        app.open_bytes("page.pdf", None, PDF.to_vec()).unwrap();
        app.set_option("left", "closed").unwrap();
        if prefs {
            app.set_option("dialog", "preferences").unwrap();
        }
        app
    });
    h.run_steps(4);
    if !prefs {
        h.state_mut().show_chess_board();
        h.run_steps(3);
    }
    h
}

/// With PDFCRAFT_CHESS_SHOTS set, a picture of the window there.
fn shot(h: &mut Harness<'static, PdfCraftApp>, name: &str) {
    if let Ok(dir) = std::env::var("PDFCRAFT_CHESS_SHOTS") {
        h.render().unwrap().save(std::path::Path::new(&dir).join(name)).unwrap();
    }
}

fn press(h: &mut Harness<'static, PdfCraftApp>, label: &str) {
    h.get_by_label(label).scroll_to_me();
    h.run_steps(2);
    h.get_by_label(label).hover();
    h.run_steps(1);
    h.get_by_label(label).click();
    h.run_steps(3);
}

#[test]
fn a_theme_picked_in_preferences_is_kept() {
    let mut h = harness("", true);
    assert_eq!(h.state().board_theme().id, "brown", "brown by default");
    press(&mut h, "Walnut");
    assert_eq!(h.state().board_theme().id, "walnut");
    shot(&mut h, "theme-preferences.png");
    let saved: serde_json::Value = serde_json::from_str(&h.state().persist()).unwrap();
    assert_eq!(saved["chess_board_theme"], "walnut");
    let h = harness(&saved.to_string(), true);
    assert_eq!(h.state().board_theme().id, "walnut", "read back next time");
    // A theme no longer known leaves the default.
    let h = harness(r#"{"chess_board_theme": "gone"}"#, true);
    assert_eq!(h.state().board_theme().id, "brown");
}

#[test]
fn the_board_panels_menu_and_the_control_channel_pick_a_theme() {
    let mut h = harness("", false);
    press(&mut h, "Board theme");
    press(&mut h, "Green");
    assert_eq!(h.state().board_theme().id, "green");
    h.state_mut().chess.board_mut().play_uci_line(&["e2e4".into(), "e7e5".into(), "g1f3".into()]);
    h.run_steps(3);
    shot(&mut h, "theme-green-board.png");
    h.state_mut().set_option("board-theme", "HIGH-CONTRAST").unwrap();
    assert_eq!(h.state().board_theme().id, "high-contrast");
    let e = h.state_mut().set_option("board-theme", "cburnett").unwrap_err();
    assert!(e.contains("brown") && e.contains("coral"), "{e}");
    assert_eq!(h.state().board_theme().id, "high-contrast", "a bad name changes nothing");
}
