//! Help ▸ Check for updates (issue #28), with stand-in release sources (no network).
//!
//! In this fork (plan A9, E2) the command never asks for releases: the original PdfCraft's lack
//! the chess tools, and new builds come from `dist/app`. The tests that offered a newer release,
//! said a version was up to date or that GitHub could not be reached went with that path; the
//! version comparison it would use is still checked.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use pdfcraft_ui_egui::PdfCraftApp;
use pdfcraft_ui_egui::updates::{Release, is_newer};

#[test]
fn versions_compare_by_number() {
    assert!(is_newer("v0.2.0", "0.1.1"));
    assert!(is_newer("v0.1.10", "0.1.9"));
    assert!(is_newer("1", "0.9.9"));
    assert!(!is_newer("v0.1.1", "0.1.1"));
    assert!(!is_newer("v0.1.0", "0.1.1"));
    assert!(!is_newer("v0.1.1-beta.2", "0.1.1"), "suffixes are ignored");
    assert!(!is_newer("nightly", "0.1.1"), "a tag that isn't a version is never newer");
    assert!(!is_newer("v1.2.3.4", "0.1.1"));
    assert!(!is_newer("v99999999999999999999.0.0", "0.1.1"), "out of range");
}

/// Plan A9, E2 (R10-02): Check for updates used to ask GitHub for the original PdfCraft's latest
/// release and offer it for download, a build without the chess tools. Now the release source (a
/// stand-in that counts its calls) is never called, at start or from the command, and the dialog
/// says which build this is and where new ones come from.
#[test]
fn the_fork_never_asks_for_the_original_pdfcrafts_releases() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        // Settings from a build that had the startup option are ignored.
        app.restore(r#"{"check_updates_at_start": true}"#);
        let counted = counted.clone();
        app.update_source = Some(Arc::new(move || {
            counted.fetch_add(1, Ordering::SeqCst);
            Ok(Release { version: "v99.0.0".into(), url: "https://github.com/storytold/pdfcraft/releases/tag/v99.0.0".into() })
        }));
        app
    });
    h.run_steps(4);
    h.state_mut().execute("help.check_updates");
    // Long enough for a check thread to have answered, had one started.
    for _ in 0..20 {
        h.run_steps(2);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0, "the release source was called");
    assert!(h.query_by_label_contains("is available").is_none());
    assert!(h.query_by_label("Download").is_none());
    let build = pdfcraft_engine::build_info::BUILD;
    h.get_by_label_contains(&format!("PdfCraft Xadrez {build}: new versions come from the dist/app folder."));
    h.get_by_label_contains("Nothing is asked over the network");
    h.get_by_label("Close").click();
    h.run_steps(3);
    assert!(h.query_by_label_contains("PdfCraft Xadrez").is_none(), "Close closes the dialog");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
