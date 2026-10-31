//! Preferences ▸ Chess pieces (plan HC2): a set picked in the grid is used at once by its role
//! (the boards, or diagrams and exports), and kept with the preferences.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use pdfcraft_engine::chess::pdfcraft_chessvision::pieces::choice::PieceChoice;
use pdfcraft_ui_egui::PdfCraftApp;

const PDF: &[u8] = b"%PDF-1.7
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 300 400] >> endobj
3 0 obj << /Type /Page /Parent 2 0 R >> endobj
trailer << /Root 1 0 R >>
%%EOF";

/// The app with a page open and Preferences shown, after `settings` were restored.
fn harness(settings: &str) -> Harness<'static, PdfCraftApp> {
    let settings = settings.to_owned();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 1000.0)).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.restore(&settings);
        app.set_option("language", "en").unwrap();
        app.open_bytes("page.pdf", None, PDF.to_vec()).unwrap();
        app.set_option("left", "closed").unwrap();
        app.set_option("dialog", "preferences").unwrap();
        app
    });
    // The pictures are drawn one per frame.
    h.run_steps(16);
    h
}

/// Click `label` in the scrolling Preferences: brought into view and pointed at first.
fn press(h: &mut Harness<'static, PdfCraftApp>, label: &str) {
    h.get_by_label(label).scroll_to_me();
    h.run_steps(2);
    h.get_by_label(label).hover();
    h.run_steps(1);
    h.get_by_label(label).click();
    h.run_steps(3);
}

#[test]
fn a_set_picked_for_each_role_is_used_at_once_and_remembered() {
    let mut h = harness("");
    assert_eq!(h.state().chess.board_piece_choice, PieceChoice::Auto);
    // The boards' set (the role shown first).
    press(&mut h, "Chessnut");
    assert_eq!(h.state().chess.board_piece_choice, PieceChoice::Bundled("chessnut".into()));
    assert_eq!(h.state().chess.board_pieces.as_ref().map(|p| p.name.as_str()), Some("Chessnut"));
    assert!(h.query_by_label_contains("Chessnut · Alexis Luengas · Apache-2.0").is_some(), "the author and licence are shown");
    // The diagrams' set, chosen apart.
    press(&mut h, "Diagrams");
    press(&mut h, "Firi");
    assert_eq!(h.state().chess.diagram_piece_choice, PieceChoice::Bundled("firi".into()));
    assert_eq!(h.state().chess.pieces.as_ref().map(|p| p.name.as_str()), Some("Firi"));
    assert_eq!(h.state().chess.board_piece_choice, PieceChoice::Bundled("chessnut".into()), "the boards keep theirs");
    // Kept with the preferences, and read back next time.
    let saved: serde_json::Value = serde_json::from_str(&h.state().persist()).unwrap();
    assert_eq!((saved["chess_pieces_board"].as_str(), saved["chess_pieces_diagrams"].as_str()), (Some("bundled:chessnut"), Some("bundled:firi")));
    let h = harness(&saved.to_string());
    assert_eq!(h.state().chess.board_piece_choice, PieceChoice::Bundled("chessnut".into()));
    assert_eq!(h.state().chess.diagram_piece_choice, PieceChoice::Bundled("firi".into()));
}

#[test]
fn plain_shapes_and_auto_are_offered_and_a_stale_token_is_ignored() {
    let mut h = harness(r#"{"chess_pieces_board": "nonsense", "chess_pieces_diagrams": "bundled:gone"}"#);
    // An unreadable token leaves the default; a set no longer shipped is kept but says why.
    assert_eq!(h.state().chess.board_piece_choice, PieceChoice::Auto);
    assert_eq!(h.state().chess.diagram_piece_choice, PieceChoice::Bundled("gone".into()));
    press(&mut h, "Diagrams");
    assert!(h.query_by_label_contains("These pieces cannot be used: PdfCraft ships no piece set called \"gone\"").is_some());
    assert_eq!(h.state().chess.pieces.as_ref().map(|p| p.name.as_str()), Some("geometric"), "plain shapes are drawn instead");
    press(&mut h, "Board");
    press(&mut h, "Plain shapes");
    assert_eq!(h.state().chess.board_piece_choice, PieceChoice::Geometric);
    press(&mut h, "Auto");
    assert_eq!(h.state().chess.board_piece_choice, PieceChoice::Auto);
}

/// A page of ours carrying one set of twelve squares, percent-encoded in a script's JSON.
fn piece_page() -> String {
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><path d="M2 2 H8 V8 H2 Z" fill="black"/></svg>"#;
    let value = format!("data:image/svg+xml,{}", svg.replace('<', "%3C").replace('>', "%3E").replace('"', "%22"));
    let keys = ["wK", "wQ", "wR", "wB", "wN", "wP", "bK", "bQ", "bR", "bB", "bN", "bP"];
    let entries: Vec<String> = keys.iter().map(|k| format!("\"{k}\": \"{value}\"")).collect();
    format!("<html><script>var sets = {{\"squares\": {{{}}}}};</script></html>", entries.join(", "))
}

/// Plan HC3: the import buttons are offered; an import says what it wrote and that its licence
/// is unknown, and a second one asks before replacing, Cancel keeping what is there.
#[test]
fn imported_sets_are_reported_and_replaced_only_when_asked() {
    let dir = std::env::temp_dir().join(format!("pdfcraft-ui-piece-import-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let page = dir.join("course.html");
    std::fs::write(&page, piece_page()).unwrap();
    let into = dir.join("pieces");
    let mut h = harness("");
    assert!(h.query_by_label("Import a folder…").is_some() && h.query_by_label("Import from a web page…").is_some());
    h.state_mut().import_pieces_into(page.clone(), into.clone(), false);
    h.run_steps(3);
    assert!(h.query_by_label_contains("Imported: squares. Their licence is not known").is_some());
    assert!(into.join("squares/wK.svg").is_file());
    let first = std::fs::metadata(into.join("squares/ORIGIN.txt")).unwrap().modified().unwrap();
    // Again: nothing is written until Replace.
    std::fs::write(into.join("squares/wK.svg"), "mine").unwrap();
    h.state_mut().import_pieces_into(page.clone(), into.clone(), false);
    h.run_steps(3);
    assert!(h.query_by_label_contains("Already in the pieces folder: squares. Replace them?").is_some());
    press(&mut h, "Cancel");
    assert!(h.query_by_label_contains("Already in the pieces folder").is_none());
    assert_eq!(std::fs::read_to_string(into.join("squares/wK.svg")).unwrap(), "mine");
    h.state_mut().import_pieces_into(page.clone(), into.clone(), false);
    h.run_steps(3);
    press(&mut h, "Replace");
    assert!(h.query_by_label_contains("Imported: squares.").is_some());
    assert_ne!(std::fs::read_to_string(into.join("squares/wK.svg")).unwrap(), "mine");
    assert!(std::fs::metadata(into.join("squares/ORIGIN.txt")).unwrap().modified().unwrap() >= first);
    // A page with no sets says so.
    std::fs::write(dir.join("empty.html"), "<html></html>").unwrap();
    h.state_mut().import_pieces_into(dir.join("empty.html"), into, false);
    h.run_steps(3);
    assert!(h.query_by_label_contains("The pieces cannot be imported:").is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Chess fonts (DiagPDF's board fonts, say) go into the fonts folder: the button is offered, the
/// import names the chess fonts it took (a text font is passed over), and a different file of
/// the same name waits for Replace, Cancel keeping what is there.
#[test]
fn imported_fonts_are_reported_and_replaced_only_when_asked() {
    use pdfcraft_chessvision::pieces::test_font::font;
    let dir = std::env::temp_dir().join(format!("pdfcraft-ui-font-import-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (from, into) = (dir.join("DiagPDF"), dir.join("fonts"));
    std::fs::create_dir_all(&from).unwrap();
    let leipzig = font("Chess Leipzig DG", "phbrqkojntwl+ ", true, 0);
    std::fs::write(from.join("LeipzigDG.ttf"), &leipzig).unwrap();
    std::fs::write(from.join("HastingsFigurine.TTF"), font("HastingsFigurine", "KQRBN", false, 0)).unwrap();
    std::fs::write(from.join("Plain.ttf"), font("Plain Sans", "abc", false, 0)).unwrap();
    let mut h = harness("");
    assert!(h.query_by_label("Import fonts…").is_some());
    h.state_mut().import_fonts_into(vec![from.clone()], into.clone(), false);
    h.run_steps(3);
    assert!(h.query_by_label_contains("Imported: HastingsFigurine, Chess Leipzig DG. Their licence is not known").is_some());
    assert!(into.join("LeipzigDG.ttf").is_file() && !into.join("Plain.ttf").exists());
    // A different font of the same file name: nothing is written until Replace.
    let other = font("Chess Leipzig DG", "phbrqkojntwl+ *", true, 0);
    std::fs::write(from.join("LeipzigDG.ttf"), &other).unwrap();
    h.state_mut().import_fonts_into(vec![from.join("LeipzigDG.ttf")], into.clone(), false);
    h.run_steps(3);
    assert!(h.query_by_label_contains("Already in the fonts folder: LeipzigDG.ttf. Replace them?").is_some());
    press(&mut h, "Cancel");
    assert_eq!(std::fs::read(into.join("LeipzigDG.ttf")).unwrap(), leipzig);
    h.state_mut().import_fonts_into(vec![from.join("LeipzigDG.ttf")], into.clone(), false);
    h.run_steps(3);
    press(&mut h, "Replace");
    assert!(h.query_by_label_contains("Imported: Chess Leipzig DG.").is_some());
    assert_eq!(std::fs::read(into.join("LeipzigDG.ttf")).unwrap(), other);
    // No chess font at all says so.
    h.state_mut().import_fonts_into(vec![from.join("Plain.ttf")], into, false);
    h.run_steps(3);
    assert!(h.query_by_label_contains("The fonts cannot be imported:").is_some());
    let _ = std::fs::remove_dir_all(&dir);
}
