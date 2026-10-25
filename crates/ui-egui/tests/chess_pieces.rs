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
