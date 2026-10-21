//! Chess ▸ Chess models… (U4): the folder the chess models are read from, chosen in the app and
//! remembered in the settings, and on the web the model files picked and installed for the
//! session. No weights are committed (decision D2): the folders here hold files that are not
//! models, so what is checked is that each is found, judged and reported.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use pdfcraft_ui_egui::PdfCraftApp;

const PDF: &[u8] = b"%PDF-1.7
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 300 400] >> endobj
3 0 obj << /Type /Page /Parent 2 0 R >> endobj
trailer << /Root 1 0 R >>
%%EOF";

/// The app with a page open, after `settings` were restored and the given options set.
fn harness(settings: &str, options: &[(&str, &str)]) -> Harness<'static, PdfCraftApp> {
    let settings = settings.to_owned();
    let options: Vec<(String, String)> = options.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 1000.0)).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.restore(&settings);
        app.set_option("language", "en").unwrap();
        app.open_bytes("page.pdf", None, PDF.to_vec()).unwrap();
        for (k, v) in &options {
            app.set_option(k, v).unwrap();
        }
        app.run_inline = true;
        app
    });
    h.run_steps(4);
    h
}

/// A fresh folder with a glyph model that is not one (and no description beside it).
fn models_folder(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcraft-ui-models-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("chess-glyphs.onnx"), b"not a model").unwrap();
    dir
}

/// Click the button `label` in the scrolling Preferences: brought into view and pointed at first.
fn press(h: &mut Harness<'static, PdfCraftApp>, label: &str) {
    h.get_by_label(label).scroll_to_me();
    h.run_steps(2);
    h.get_by_label(label).hover();
    h.run_steps(1);
    h.get_by_label(label).click();
    h.run_steps(3);
}

fn toast(h: &Harness<'static, PdfCraftApp>) -> String {
    h.state().toast.as_ref().map(|t| t.0.clone()).unwrap_or_default()
}

/// The Diagrams tab's Chess models… picks the folder, checks it at once (each model found or not,
/// and why one could not be loaded), and the folder is remembered: the next session uses it and
/// Preferences ▸ Chess shows it. Forget goes back to the search.
#[test]
fn the_chosen_models_folder_is_checked_shown_and_remembered() {
    let dir = models_folder("chosen");
    let mut h = harness("", &[]);
    assert!(h.state_mut().execute("chess.panel"));
    h.run_steps(3);
    h.state_mut().pick_override = Some(vec![dir.display().to_string()]);
    h.get_by_label("Chess models…").click();
    h.run_steps(3);
    assert_eq!(h.state().chess.models.folder.as_deref(), Some(dir.as_path()));
    let said = toast(&h);
    assert!(
        said.contains("piece model not found") && said.contains("glyph model couldn't be loaded: ") && said.contains("chess-glyphs.json"),
        "{said}"
    );
    assert_eq!(h.state().chess.models.models_dir().as_deref(), Some(dir.as_path()), "the chosen folder comes first");

    let saved: serde_json::Value = serde_json::from_str(&h.state().persist()).unwrap();
    assert_eq!(saved["chess_models"]["folder"].as_str(), dir.to_str(), "{saved}");
    let mut h = harness(&saved.to_string(), &[("left", "closed"), ("dialog", "preferences")]);
    assert_eq!(h.state().chess.models.folder.as_deref(), Some(dir.as_path()));
    h.get_by_label(&dir.display().to_string());
    press(&mut h, "Check again");
    h.get_by_label("Piece model: not found");
    assert!(h.query_by_label_contains("Glyph model: couldn't be loaded").is_some());

    // The folder gone: kept, but the search is used, and Preferences says so.
    std::fs::remove_dir_all(&dir).unwrap();
    h.run_steps(2);
    assert!(h.query_by_label_contains("This folder holds no chess model, or is gone").is_some());
    assert_ne!(h.state().chess.models.models_dir().as_deref(), Some(dir.as_path()));
    press(&mut h, "Forget");
    assert_eq!(h.state().chess.models.folder, None);
    let saved: serde_json::Value = serde_json::from_str(&h.state().persist()).unwrap();
    assert!(saved["chess_models"]["folder"].is_null(), "{saved}");
}

/// The command picks a folder too; a choice that is not a usable folder (relative) is refused
/// and says so, keeping the folder before.
#[test]
fn the_command_chooses_and_a_relative_folder_is_refused() {
    let dir = models_folder("command");
    let mut h = harness("", &[]);
    h.state_mut().pick_override = Some(vec![dir.display().to_string()]);
    assert!(h.state_mut().execute("chess.models"));
    h.run_steps(3);
    assert_eq!(h.state().chess.models.folder.as_deref(), Some(dir.as_path()));
    h.state_mut().pick_override = Some(vec!["models".into()]);
    assert!(h.state_mut().execute("chess.models"));
    h.run_steps(3);
    assert_eq!(h.state().chess.models.folder.as_deref(), Some(dir.as_path()));
    assert!(toast(&h).contains("can't be used as the chess models folder"), "{}", toast(&h));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The settings are untrusted: a bad value is ignored and the app starts with no folder chosen.
#[test]
fn bad_models_settings_are_ignored() {
    let long = format!("{}{}", std::env::temp_dir().display(), "x".repeat(5000));
    for bad in [
        serde_json::json!("yes"),
        serde_json::json!([1, 2]),
        serde_json::json!({ "folder": 3 }),
        serde_json::json!({ "folder": "" }),
        serde_json::json!({ "folder": "relative/models" }),
        serde_json::json!({ "folder": format!("{}\u{0}", std::env::temp_dir().display()) }),
        serde_json::json!({ "folder": long }),
        serde_json::Value::Null,
    ] {
        let mut app = PdfCraftApp::new();
        app.restore(&serde_json::json!({ "chess_models": bad, "chess_capture_on_copy": true }).to_string());
        assert_eq!(app.chess.models.folder, None, "{bad}");
        assert!(app.chess.capture.on_copy, "the other settings are still read beside {bad}");
    }
}

/// The web's installer, which is target-independent: files picked become the session's models
/// wherever no folder is given, each judged (a model with one of its files is invalid and says
/// which is missing), and files that are not model files are listed.
#[test]
fn model_files_picked_are_installed_for_the_session() {
    let mut app = PdfCraftApp::new();
    app.set_option("language", "en").unwrap();
    let meta = br#"{"kind":"chess-glyphs","input":{"name":"glyphs"},"output":{"name":"logits","classes":["a"]}}"#.to_vec();
    app.install_chess_model_files(vec![
        ("chess-pieces.onnx".into(), Ok(b"not a model".to_vec())),
        ("chess-glyphs.json".into(), Ok(meta)),
        ("readme.txt".into(), Ok(Vec::new())),
    ]);
    let installed = pdfcraft_engine::chess::models(None);
    let report = app.chess.models.report.clone().expect("a report");
    pdfcraft_engine::chess::install_models(None);
    assert!(
        matches!(&installed.pieces, pdfcraft_engine::chess::ModelState::Invalid(why) if why.contains("chess-pieces.json is missing")),
        "{:?}",
        installed.pieces
    );
    assert!(
        matches!(&installed.glyphs, pdfcraft_engine::chess::ModelState::Invalid(why) if why.contains("chess-glyphs.onnx is missing")),
        "{:?}",
        installed.glyphs
    );
    assert_eq!(report.ignored, ["readme.txt"]);
    assert!(app.toast.as_ref().is_some_and(|t| t.0.contains("piece model couldn't be loaded")), "{:?}", app.toast);
}
