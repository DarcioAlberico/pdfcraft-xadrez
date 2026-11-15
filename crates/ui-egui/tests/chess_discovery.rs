//! Chess, found without the panel (plan A9-I16, §2.8.1 "Descoberta", finding R4-02): the
//! palette finds the board, the analysis and the export by name, the Chess menu holds the chess
//! commands, F1 in the chess panels shows the sheet of the chess keys, and a book in a chess font
//! or with a PGN beside it shows the rail's chess button before anything was detected.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use pdfcraft_ui_egui::{PdfCraftApp, RightPanel};

/// A 600 × 800 pt page with a line of text in `font` (a Type1 font, not embedded).
fn page_in(font: &str) -> Vec<u8> {
    let content = "BT /F1 12 Tf 60 700 Td (Page one) Tj ET\n";
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 600 800] >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        format!("<< /Type /Font /Subtype /Type1 /BaseFont /{font} >>"),
    ];
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

/// The app on `bytes` opened as `name` (from `path`, when given).
fn harness_on(name: &str, path: Option<String>, bytes: Vec<u8>) -> Harness<'static, PdfCraftApp> {
    let name = name.to_owned();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 1000.0)).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes(&name, path, bytes).unwrap();
        app.run_inline = true;
        app
    });
    h.run_steps(4);
    h
}

/// A plain document: nothing in it says chess.
fn harness() -> Harness<'static, PdfCraftApp> {
    harness_on("plain.pdf", None, page_in("Helvetica"))
}

/// The palette open on `query`.
fn palette(h: &mut Harness<'static, PdfCraftApp>, query: &str) {
    h.state_mut().set_option("palette", query).unwrap();
    h.run_steps(3);
}

/// The palette finds "analyze", "board" and "export book", and each runs its chess command: the
/// analysis of the position on the board, the board, the export dialog (plan A9-X2).
#[test]
fn the_palette_finds_the_board_the_analysis_and_the_export() {
    let mut h = harness();
    palette(&mut h, "analyze");
    h.get_by_label("Analyze the position").click();
    h.run_steps(3);
    assert!(h.state().chess.study.analysis, "analysis is on");
    assert_eq!(h.state().right, Some(RightPanel::Chess), "the board shows");

    let mut h = harness();
    palette(&mut h, "board");
    h.get_by_label("Chess board").click();
    h.run_steps(3);
    assert_eq!(h.state().right, Some(RightPanel::Chess), "the board shows");
    assert!(!h.state().chess.study.analysis, "only shown");

    let mut h = harness();
    palette(&mut h, "export book");
    h.get_by_label("Export chess book…").click();
    h.run_steps(3);
    assert!(h.state().chess.export_dialog.is_some(), "the export dialog");
    h.get_by_role_and_label(Role::Dialog, "Export chess book");
}

/// The Chess menu (in Menu) holds the chess commands, which run from it: the sheet of the chess
/// keys opens from its item, lists the board's and the Diagram tab's keys, and Esc closes it.
#[test]
fn the_chess_menu_holds_the_chess_commands() {
    let mut h = harness();
    h.get_by_label("Menu").click();
    h.run_steps(2);
    h.get_by_label("Chess ⏵").hover();
    h.run_steps(3);
    for item in [
        "Scan chess diagrams",
        "Chess diagrams panel",
        "Chess board",
        "Analyze the position",
        "Load the book's PGN…",
        "Export chess book…",
        "Make the book's bookmarks…",
        "Chess shortcuts",
    ] {
        // A menu item's name has its shortcut's text after it.
        h.get_by_label_contains(item);
    }
    h.get_by_label_contains("Chess shortcuts").click();
    h.run_steps(3);
    assert!(h.state().chess.shortcuts_open);
    h.get_by_role_and_label(Role::Dialog, "Chess shortcuts");
    for line in ["Board, move list and Chess panel", "Diagram tab", "Move back", "Previous variation", "Confirm and next", "Good move"] {
        h.get_by_label(line);
    }
    h.get_by_label("←, Backspace");
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(!h.state().chess.shortcuts_open, "Esc closes it");
    // Tools › Chess has them too.
    let chess = pdfcraft_engine::catalog::TOOL_GROUPS.iter().find(|g| g.id == "chess").expect("the Chess tools");
    let commands: Vec<&str> = chess.sections.iter().flat_map(|s| s.items.iter().map(|i| i.command)).collect();
    for c in ["chess.scan", "chess.panel", "chess.board.show", "chess.tool.capture", "chess.reference.load", "chess.export"] {
        assert!(commands.contains(&c), "{c} in Tools › Chess: {commands:?}");
    }
}

/// Press `key` as the desktop app gets it: the frame's raw input goes through `raw_input_hook`
/// first, where the chess scope takes its keys (kittest itself skips the hook).
fn host_key(h: &mut Harness<'static, PdfCraftApp>, key: egui::Key) {
    use eframe::App as _;
    let ctx = h.ctx.clone();
    for pressed in [true, false] {
        let mut raw = std::mem::take(h.input_mut());
        raw.events.push(egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::NONE });
        h.state_mut().raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }
    h.run_steps(2);
}

/// F1 in the chess key scope (a click on the Board panel) shows the sheet of the chess keys.
#[test]
fn f1_on_the_board_shows_the_sheet_of_the_chess_keys() {
    let mut h = harness();
    h.state_mut().show_chess_board();
    h.run_steps(3);
    let at =
        h.get_all_by_label_contains(" to move").find(|n| n.accesskit_node().role() == Role::Label).expect("the Board tab's header").rect().center();
    h.hover_at(at);
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
    }
    h.run_steps(2);
    assert!(h.state().chess.in_scope(), "the chess scope");
    host_key(&mut h, egui::Key::F1);
    assert!(h.state().chess.shortcuts_open, "F1 in the scope");
    h.get_by_role_and_label(Role::Dialog, "Chess shortcuts");
}

/// The rail's chess button (its right edge): there for a book in a chess font, or with a PGN beside
/// it, before anything was detected; not for a plain document.
#[test]
fn a_chess_font_or_a_pgn_beside_the_pdf_shows_the_rails_chess_button() {
    let rail_chess = |h: &Harness<'static, PdfCraftApp>| {
        h.query_all_by(|n| n.role() == Role::Button && n.label().as_deref() == Some("Chess")).any(|n| n.rect().min.x > 1300.0)
    };
    let h = harness();
    assert!(!rail_chess(&h), "a plain document has none");
    let h = harness_on("merida.pdf", None, page_in("ChessMerida"));
    assert!(h.state().chess.results().is_none(), "nothing detected");
    assert!(rail_chess(&h), "a chess font");
    let dir = std::env::temp_dir().join(format!("pdfcraft-chess-discovery-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let pdf = dir.join("book.pdf");
    std::fs::write(&pdf, page_in("Helvetica")).unwrap();
    std::fs::write(dir.join("book.pgn"), "[Event \"?\"]\n\n1. e4 *\n").unwrap();
    let h = harness_on("book.pdf", Some(pdf.to_string_lossy().into_owned()), page_in("Helvetica"));
    assert!(rail_chess(&h), "a PGN beside the PDF");
    let _ = std::fs::remove_dir_all(&dir);
}
