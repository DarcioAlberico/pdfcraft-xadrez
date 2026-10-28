//! Chess ▸ Diagrams in the real shell (egui_kittest): scanning a page, the boxes on it, the
//! Chess diagrams panel and what it does (blueprint S16a). The PDF is built here: a board set in
//! the Chess Merida diagram font, read exactly without a model, and a board drawn as filled
//! squares, which is found and, without the user's piece model, not read.

use egui::Pos2;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use pdfcraft_engine::{Edit, NewAnnotation, Shape, Style};
use pdfcraft_ui_egui::{PdfCraftApp, RightPanel};

mod support;

const START_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
/// The start position in the Chess Merida layout, top rank first: light squares lower case, dark
/// upper case; empty light ' ', empty dark '+'.
const MERIDA_START: [&str; 8] = ["tMvWlVmT", "OoOoOoOo", " + + + +", "+ + + + ", " + + + +", "+ + + + ", "pPpPpPpP", "RnBqKbNr"];

/// A 600 × 800 pt page with the start position in a diagram font, top left (x 60..220, y
/// 580..740 in user space), and a board of filled squares under it (x 60..220, y 240..400).
fn boards() -> Vec<u8> {
    boards_with("")
}

/// [`boards`] with a line of `moves` in Helvetica between the two boards (y 540).
fn boards_with(moves: &str) -> Vec<u8> {
    let mut content = if moves.is_empty() { String::new() } else { format!("BT /F2 11 Tf 60 540 Td ({moves}) Tj ET\n") };
    content.push_str("BT /F1 20 Tf 60 724 Td ");
    for (i, line) in MERIDA_START.iter().enumerate() {
        if i > 0 {
            content.push_str("0 -20 Td ");
        }
        content.push_str(&format!("({line}) Tj "));
    }
    content.push_str("ET\n0.45 g\n");
    for rank in 0..8 {
        for file in 0..8 {
            if (rank + file) % 2 == 0 {
                content.push_str(&format!("{} {} 20 20 re\n", 60 + file * 20, 240 + rank * 20));
            }
        }
    }
    content.push_str("f\n");
    let widths = vec!["1000"; 95].join(" ");
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 600 800] >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /Font << /F1 5 0 R /F2 6 0 R >> >> >>".to_string(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        format!("<< /Type /Font /Subtype /Type1 /BaseFont /ChessMerida /FirstChar 32 /LastChar 126 /Widths [{widths}] >>"),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_string(),
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

fn harness() -> Harness<'static, PdfCraftApp> {
    harness_stepping(0.25)
}

/// The harness, with frames `step_dt` seconds apart (kittest's default is 0.25 s).
fn harness_stepping(step_dt: f32) -> Harness<'static, PdfCraftApp> {
    harness_on(boards(), step_dt)
}

/// The harness on the PDF `bytes`.
fn harness_on(bytes: Vec<u8>, step_dt: f32) -> Harness<'static, PdfCraftApp> {
    harness_with(bytes, step_dt, egui::vec2(1400.0, 1000.0), None)
}

/// The harness on the PDF `bytes` in a window of `size`.
fn harness_sized(bytes: Vec<u8>, size: egui::Vec2) -> Harness<'static, PdfCraftApp> {
    harness_with(bytes, 0.25, size, None)
}

/// The harness on `bytes`, its diagram corrections kept in `corrections` (a folder) when given.
fn harness_with(bytes: Vec<u8>, step_dt: f32, size: egui::Vec2, corrections: Option<std::path::PathBuf>) -> Harness<'static, PdfCraftApp> {
    let mut h = Harness::builder().with_size(size).with_step_dt(step_dt).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        if let Some(dir) = corrections {
            app.chess.corrections.use_folder(dir);
        }
        app.open_bytes("boards.pdf", None, bytes).unwrap();
        app.run_inline = true;
        app
    });
    h.run_steps(4);
    h
}

/// A point of page 1 given in user space (y up), on screen.
fn on_page(h: &Harness<'static, PdfCraftApp>, x: f32, y: f32) -> Pos2 {
    let r = h.state().views[0].page_screen_rect(0).expect("page 1 on screen");
    let k = r.width() / 600.0;
    r.min + egui::vec2(x * k, (800.0 - y) * k)
}

fn diagrams(h: &Harness<'static, PdfCraftApp>) -> usize {
    h.state().chess.results.as_ref().map_or(0, |r| r.count())
}

#[test]
fn scanning_lists_the_diagrams_with_their_positions_and_marks_them_on_the_page() {
    let mut h = harness();
    assert!(h.state_mut().execute("chess.scan"));
    h.run_steps(3);
    assert!(h.state().chess.run.is_none(), "the scan ran inline and finished");
    assert_eq!(h.state().right, Some(RightPanel::Chess), "scanning shows the panel");
    assert_eq!(diagrams(&h), 2);
    assert_eq!(h.state().toast.as_ref().map(|t| t.0.as_str()), Some("Diagrams found: 2"));
    // The font diagram is first in reading order (the left column, top to bottom), read exactly.
    h.get_by_label("Diagram 1 · Position read");
    h.get_by_label(START_FEN);
    let second = h
        .get_all_by_label_contains("Diagram 2 · ")
        .next()
        .expect("the drawn board is listed")
        .accesskit_node()
        .label()
        .unwrap_or_default()
        .to_string();
    assert_eq!(h.state().chess.results.as_ref().and_then(|r| r.diagram(0, 1)).map(|d| d.0), Some([60.0, 240.0, 220.0, 400.0]), "{second}");

    // Copy FEN puts the position on the clipboard. (The first button is diagram 1's: with the
    // user's piece model installed, the drawn board is read too and has its own.)
    h.get_all_by_label("Copy FEN").next().expect("diagram 1 has a position").click();
    let mut copied = false;
    for _ in 0..3 {
        h.step();
        copied |= h.output().platform_output.commands.iter().any(|c| matches!(c, egui::OutputCommand::CopyText(t) if t == START_FEN));
    }
    assert!(copied, "the FEN was copied");
    // Open in lichess opens the analysis board on that position.
    h.get_all_by_label("Open in lichess").next().expect("and a lichess link").click();
    h.run_steps(2);
    assert_eq!(
        h.state().last_opened_url.as_deref(),
        Some("https://lichess.org/analysis/rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR%20w%20KQkq%20-%200%201")
    );

    // Picking a diagram in the panel goes to it and outlines it on the page.
    h.get_by_label(&second).click();
    h.run_steps(2);
    assert_eq!(h.state().chess.results.as_ref().and_then(|r| r.selected), Some((0, 1)));
    assert_eq!(h.state().views[0].flash.map(|f| (f.0, f.1)), Some((0, [60.0, 240.0, 220.0, 400.0])));

    // Clicking a box on the page picks that diagram.
    let at = on_page(&h, 140.0, 660.0);
    h.hover_at(at);
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
    }
    h.run_steps(3);
    assert_eq!(h.state().chess.results.as_ref().and_then(|r| r.selected), Some((0, 0)), "the font diagram was clicked");
}

/// After an edit the boxes may no longer be over the diagrams: the panel says so, and the next
/// scan starts afresh.
#[test]
fn an_edit_makes_the_scan_stale_until_the_next_one() {
    let mut h = harness();
    assert!(h.state_mut().execute("chess.scan"));
    h.run_steps(3);
    assert_eq!(diagrams(&h), 2);
    // A comment in a corner: the diagrams stay where they were, but nothing says so.
    let shape = Shape::Rectangle { rect: [10.0, 10.0, 50.0, 50.0] };
    h.state_mut().apply_edit(Edit::AddAnnotation(NewAnnotation {
        page: 0,
        style: Style::default_for(&shape),
        shape,
        contents: String::new(),
        author: String::new(),
    }));
    h.run_steps(3);
    h.get_by_label("The document changed since the scan. Scan again to update the diagrams.");
    // The stale boxes are not drawn, and clicking where one was picks nothing.
    let at = on_page(&h, 140.0, 660.0);
    h.hover_at(at);
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
    }
    h.run_steps(3);
    assert_eq!(h.state().chess.results.as_ref().and_then(|r| r.selected), None);
    h.get_by_label("Scan this page").click();
    h.run_steps(3);
    assert!(h.query_by_label("The document changed since the scan. Scan again to update the diagrams.").is_none());
    assert_eq!(diagrams(&h), 2);
}

/// A document that does not allow copying its content can't be scanned.
#[test]
fn a_document_that_forbids_copying_is_not_scanned() {
    let mut doc = pdfcraft_cos::Document::open(std::sync::Arc::new(boards())).unwrap();
    doc.set_encryption(&pdfcraft_cos::NewEncryption {
        algorithm: pdfcraft_cos::Algorithm::Aes256,
        user_password: "",
        owner_password: "owner",
        // Print only: no copying.
        permissions: 0b0100,
        encrypt_metadata: true,
        seed: [7; 32],
    })
    .unwrap();
    let locked = pdfcraft_cos::write_full(&doc, &Default::default()).unwrap();
    let mut app = PdfCraftApp::new();
    app.set_option("language", "en").unwrap();
    app.run_inline = true;
    app.open_bytes("locked.pdf", None, locked).unwrap();
    app.start_chess_scan(true);
    assert!(app.chess.run.is_none() && app.chess.results.is_none(), "nothing was scanned");
}

/// The position the user set in the editor (blueprint S17), for the drawn board no model read.
const EDITED: &str = "4k3/8/8/8/8/8/8/R3K3 w Q - 0 1";

fn scan_and_edit_second(h: &mut Harness<'static, PdfCraftApp>) {
    assert!(h.state_mut().execute("chess.scan"));
    h.run_steps(3);
    let second = h
        .get_all_by_label_contains("Diagram 2 · ")
        .next()
        .expect("the drawn board is listed")
        .accesskit_node()
        .label()
        .unwrap_or_default()
        .to_string();
    h.get_by_label(&second).click();
    h.run_steps(3);
    let ed = h.state_mut().chess.editor.as_mut().expect("picking a diagram opens the editor");
    assert_eq!((ed.page, ed.index), (0, 1));
    ed.position = pdfcraft_engine::chess::pdfcraft_chess::fen::parse(EDITED).unwrap();
    h.run_steps(2);
}

/// The editor shows the position, says whether it is legal, and a click on the board puts the
/// chosen piece on the square under it.
#[test]
fn the_editor_edits_the_selected_diagrams_position() {
    let mut h = harness();
    scan_and_edit_second(&mut h);
    h.get_by_label("Details").click();
    h.run_steps(2);
    h.get_by_label(EDITED);
    h.get_by_label("Legal position");
    // Click e4 on the board (from White's side: file e = column 4, rank 4 = row 4 from the top).
    let board = h.get_by_label_contains("Diagram board: ").rect();
    let at = board.min + egui::vec2(board.width() * 4.5 / 8.0, board.height() * 4.5 / 8.0);
    h.hover_at(at);
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
    }
    h.run_steps(3);
    assert_eq!(
        h.state().chess.editor.as_ref().map(|e| e.fen()).as_deref(),
        Some("4k3/8/8/8/4Q3/8/8/R3K3 w Q - 0 1"),
        "the white queen (the default brush) is on e4"
    );
    // Two white kings is not a position, and the panel says which rule it breaks (plan A8, P3b1).
    let ed = h.state_mut().chess.editor.as_mut().unwrap();
    ed.position = pdfcraft_engine::chess::pdfcraft_chess::fen::parse("4K3/8/8/8/8/8/8/R3K3 w - - 0 1").unwrap();
    h.run_steps(2);
    h.get_by_label("Not a legal position: White has more than one king");
    // A castling right the pieces don't allow is taken off, as in the set-up: the rook left a1.
    let ed = h.state_mut().chess.editor.as_mut().unwrap();
    ed.position = pdfcraft_engine::chess::pdfcraft_chess::fen::parse("4k3/8/8/8/8/8/8/1R2K3 w Q - 0 1").unwrap();
    h.run_steps(2);
    assert_eq!(h.state().chess.editor.as_ref().map(|e| e.fen()).as_deref(), Some("4k3/8/8/8/8/8/8/1R2K3 w - - 0 1"));
    h.get_by_label("Legal position");
    // The board says why it can't play a position, too.
    h.state_mut().chess.board_mut().load_fen("4k3/8/8/8/8/8/8/8 w - - 0 1", false, None);
    assert_eq!(
        h.state().chess.board().message.as_deref(),
        Some("Not a legal position (White's king is missing): correct it in the diagram editor to play moves on it.")
    );
}

/// The palette shows the pieces as the board draws them, in two rows under the board, each
/// button named for its piece (not a letter such as `n`): Black knight puts a black knight on the square clicked, and the eraser
/// empties it again.
#[test]
fn the_editors_palette_is_the_pieces_pictures() {
    let mut h = harness();
    scan_and_edit_second(&mut h);
    // (K, Q, k and q are the castling boxes.)
    for letter in ["N", "n", "B", "b", "P", "p"] {
        assert!(h.query_by_label(letter).is_none(), "no {letter} in the palette");
    }
    let board = h.get_by_label_contains("Diagram board: ").rect();
    // The palette is under the board: White's pieces on a row, Black's and the eraser on the next.
    let (white, black, erase) = (h.get_by_label("White knight").rect(), h.get_by_label("Black knight").rect(), h.get_by_label("Erase").rect());
    assert!(white.top() > board.bottom(), "under the board: {white:?}, {board:?}");
    assert!(black.top() >= white.bottom() - 1.0 && (white.center().x - black.center().x).abs() < 4.0, "{white:?} {black:?}");
    let pawn = h.get_by_label("Black pawn").rect();
    assert!((erase.center().y - pawn.center().y).abs() < 1.0 && erase.left() > pawn.right(), "the eraser ends Black's row: {erase:?}");
    // d5 from White's side: column 3, row 3 from the top.
    let d5 = board.min + egui::vec2(board.width() * 3.5 / 8.0, board.height() * 3.5 / 8.0);
    let click_d5 = |h: &mut Harness<'static, PdfCraftApp>| {
        h.hover_at(d5);
        h.run_steps(1);
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton { pos: d5, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
        }
        h.run_steps(3);
    };
    let fen = |h: &Harness<'static, PdfCraftApp>| h.state().chess.editor.as_ref().map(|e| e.fen());
    h.get_by_label("Black knight").click();
    h.run_steps(2);
    click_d5(&mut h);
    assert_eq!(fen(&h).as_deref(), Some("4k3/8/8/3n4/8/8/8/R3K3 w Q - 0 1"), "a black knight on d5");
    h.get_by_label("Erase").click();
    h.run_steps(2);
    click_d5(&mut h);
    assert_eq!(fen(&h).as_deref(), Some("4k3/8/8/8/8/8/8/R3K3 w Q - 0 1"), "d5 is empty again");
}

/// The Diagram tab's labels stay whole, inside the tab, and its main button is in sight at
/// 1366 × 768 (plan A8, P3b2: every main action visible at that size).
#[test]
fn the_editors_labels_are_not_broken_into_letters() {
    let mut h = harness_sized(boards(), egui::vec2(1366.0, 768.0));
    scan_and_edit_second(&mut h);
    let (white, black) = (h.get_by_label("White").rect(), h.get_by_label("Black").rect());
    assert!(black.height() <= white.height() + 0.5, "Black is on one line: {black:?} against {white:?}");
    let board = h.get_by_label_contains("Diagram board: ").rect();
    let tab = h.get_by_label("Page 1 · diagram 2 of 2").rect();
    let confirm = h.get_by_label("Confirm and next").rect();
    let (left, right) = (tab.left().min(board.left()) - 12.0, confirm.right() + 12.0);
    for name in ["Move number:", "To move:", "White", "Black", "1 of 1 to review", "Details", "Legal position", "Confirm and next", "More actions"] {
        let r = h.get_by_label(name).rect();
        assert!(r.width() >= r.height(), "{name} is wider than tall: {r:?}");
        assert!(r.left() >= left && r.right() <= right, "{name} is inside the tab: {r:?}, from {left} to {right}");
    }
    assert!(confirm.bottom() <= 768.0 && confirm.top() > board.bottom(), "the main button is on screen, under the board: {confirm:?}");
    // The side to move keeps its label: To move:, White and Black on one row.
    let (to_move, white) = (h.get_by_label("To move:").rect(), h.get_by_label("White").rect());
    assert!((to_move.center().y - black.center().y).abs() < 1.0 && (white.center().y - black.center().y).abs() < 1.0, "{to_move:?} {black:?}");
    if let Ok(dir) = std::env::var("PDFCRAFT_CHESS_SHOTS") {
        h.render().unwrap().save(std::path::Path::new(&dir).join("diagram-tab.png")).unwrap();
    }
}

/// The move number is a field of its own: 35 with Black to move is `b … 35` in the FEN.
#[test]
fn the_editor_sets_the_move_number() {
    let mut h = harness();
    scan_and_edit_second(&mut h);
    h.get_by_label("Move number:");
    h.get_by_role(egui::accesskit::Role::SpinButton).click();
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.get_by_role(egui::accesskit::Role::SpinButton).type_text("35");
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    h.get_by_label("Black").click();
    h.run_steps(2);
    assert_eq!(h.state().chess.editor.as_ref().map(|e| e.fen()).as_deref(), Some("4k3/8/8/8/8/8/8/R3K3 b Q - 0 35"));
}

/// Replace on the page draws the corrected position over the diagram as one undo step and scans
/// the page again; undo brings the old page back.
#[test]
fn replace_on_the_page_then_undo() {
    let mut h = harness();
    scan_and_edit_second(&mut h);
    let before = h.state().session.get(h.state().active_ids().unwrap().1).unwrap().edit_generation();
    h.get_by_label("More actions").click();
    h.run_steps(2);
    h.get_by_label("Replace on the page").click();
    h.run_steps(4);
    assert_eq!(h.state().toast.as_ref().map(|t| t.0.as_str()), Some("Diagrams found: 2"), "replaced, then the page was scanned again");
    let id = h.state().active_ids().unwrap().1;
    assert!(h.state().session.get(id).unwrap().edit_generation() > before);
    assert!(h.state().chess.editor.is_none());
    assert_eq!(diagrams(&h), 2, "the new board is found where the old one was");
    assert!(h.state_mut().execute("edit.undo"));
    h.run_steps(2);
    assert_eq!(h.state().session.get(id).unwrap().can_undo(), None, "back to the opened file");
}

/// Add lichess link puts a link to the corrected position under the diagram.
#[test]
fn the_editor_links_the_corrected_position() {
    let mut h = harness();
    scan_and_edit_second(&mut h);
    h.get_by_label("More actions").click();
    h.run_steps(2);
    h.get_by_label("Add lichess link").click();
    h.run_steps(3);
    assert_eq!(h.state().toast.as_ref().map(|t| t.0.as_str()), Some("Lichess link added"));
    let id = h.state().active_ids().unwrap().1;
    let links = h.state().session.get(id).unwrap().links().to_vec();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].action, pdfcraft_engine::LinkAction::Uri("https://lichess.org/analysis/4k3/8/8/8/8/8/8/R3K3%20w%20Q%20-%200%201".into()));
}

/// The review queue: only the drawn board (no model read it) needs review; the list can show
/// only it, and Next goes to it.
#[test]
fn the_review_queue_lists_and_walks_the_diagrams_to_review() {
    let mut h = harness();
    assert!(h.state_mut().execute("chess.scan"));
    h.run_steps(3);
    let queue = h.state().chess.results.as_ref().unwrap().review_queue();
    if queue.is_empty() {
        // With the user's piece model installed the drawn board may be read and accepted.
        return;
    }
    assert_eq!(queue, vec![(0, 1)]);
    h.get_by_label("Only diagrams to review (1)").click();
    h.run_steps(2);
    assert!(h.query_by_label_contains("Diagram 1 · ").is_none(), "the font diagram is not to review");
    h.get_by_label_contains("Diagram 2 · ").click();
    h.run_steps(2);
    h.state_mut().chess.editor = None;
    h.state_mut().select_chess_diagram(0, 0, true);
    h.run_steps(2);
    h.get_by_label("Next to review (Page Down)").click();
    h.run_steps(2);
    assert_eq!(h.state().chess.results.as_ref().and_then(|r| r.selected), Some((0, 1)));
}

/// A page with a heading and moves in Helvetica above a board in the Chess Merida font (top left
/// at 60, 200 from the page's top).
fn chapter() -> Vec<u8> {
    let mut content = String::from("BT /F2 24 Tf 60 760 Td (Chapter 1) Tj ET\nBT /F2 11 Tf 60 730 Td (1.e4 e5 2.Nf3 Nc6 3.Bb5 a6) Tj ET\n");
    content.push_str("BT /F1 20 Tf 60 584 Td ");
    for (i, line) in MERIDA_START.iter().enumerate() {
        if i > 0 {
            content.push_str("0 -20 Td ");
        }
        content.push_str(&format!("({line}) Tj "));
    }
    content.push_str("ET\n");
    let widths = vec!["1000"; 95].join(" ");
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 600 800] >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /Font << /F1 5 0 R /F2 6 0 R >> >> >>".to_string(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        format!("<< /Type /Font /Subtype /Type1 /BaseFont /ChessMerida /FirstChar 32 /LastChar 126 /Widths [{widths}] >>"),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_string(),
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

fn chapter_harness() -> Harness<'static, PdfCraftApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 1000.0)).build_eframe(|_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("chapter.pdf", None, chapter()).unwrap();
        app.run_inline = true;
        app
    });
    h.run_steps(4);
    h.state_mut().choose_right_panel(Some(RightPanel::Chess));
    h.run_steps(2);
    h
}

/// The Text tab reads the page beside it: the heading, the moves (each outlining itself on the
/// page when clicked) and the diagram, which opens in the editor.
#[test]
fn the_text_tab_shows_the_page_read_as_chess_text_and_points_back_at_it() {
    let mut h = chapter_harness();
    h.get_by_label("Text").click();
    h.run_steps(4);
    h.get_by_label("Recognised text of page 1");
    h.get_by_label("Chapter 1");
    let text = h.state().chess.text.as_ref().expect("the page was read");
    let moves: Vec<String> = text.chess.pages[0]
        .blocks
        .iter()
        .flat_map(|b| b.inlines())
        .filter_map(|i| if let pdfcraft_engine::chess::pdfcraft_chessvision::doc::Inline::Move(m) = i { Some(m.san.clone()) } else { None })
        .collect();
    assert_eq!(moves, ["e4", "e5", "Nf3", "Nc6", "Bb5", "a6"]);
    // A move is shown with its figurine when the fonts have it, else as printed.
    h.get_all_by_label_contains("e4").next().expect("the move 1.e4 is shown").click();
    h.run_steps(2);
    let flash = h.state().views[0].flash.expect("the move is outlined on the page");
    assert_eq!(flash.0, 0);
    assert!(flash.1[1] > 700.0 && flash.1[3] < 760.0, "the moves' line, near the top: {:?}", flash.1);
    // The diagram opens in the editor, on the Diagrams tab.
    h.get_by_label("Diagram 1").click();
    h.run_steps(4);
    assert!(h.state().chess.editor.as_ref().is_some_and(|e| (e.page, e.index) == (0, 0)), "the editor is on the diagram");
}

/// Export… writes the document: PGN with the position's game, and Markdown with its image in a
/// folder beside it.
#[test]
fn export_writes_pgn_and_markdown_with_images() {
    let dir = std::env::temp_dir().join(format!("pdfcraft-chess-export-ui-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (format, name) in [("Export as PGN", "book.pgn"), ("Export as Markdown", "book.md")] {
        let mut h = chapter_harness();
        h.state_mut().save_override = Some(dir.join(name).to_string_lossy().into_owned());
        h.get_by_label("Export…").click();
        h.run_steps(2);
        h.get_by_label(format).click();
        h.run_steps(4);
        assert!(h.state().toast.as_ref().is_some_and(|t| t.0.starts_with("Exported to")), "{format}: {:?}", h.state().toast);
    }
    let pgn = std::fs::read_to_string(dir.join("book.pgn")).unwrap();
    assert!(
        pgn.contains("[FEN \"rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1\"]") && pgn.contains("1. e4 e5 2. Nf3 Nc6 3. Bb5 a6"),
        "{pgn}"
    );
    let md = std::fs::read_to_string(dir.join("book.md")).unwrap();
    assert!(md.contains("](book_diagrams/diagram-p0001-1.png)"), "{md}");
    assert!(dir.join("book_diagrams").join("diagram-p0001-1.png").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Off the interface thread: the Text tab says it is reading while a worker reads the page, and
/// shows the text when it is done. A reading of a state the document has left is never shown.
#[test]
fn the_page_is_read_off_the_interface_thread() {
    let mut h = chapter_harness();
    h.state_mut().run_inline = false;
    h.get_by_label("Text").click();
    h.run_steps(1);
    assert!(h.state().chess.job.is_some() || h.state().chess.text.is_some(), "a read was started");
    for _ in 0..200 {
        if h.state().chess.text.is_some() && h.state().chess.job.is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
        h.step();
    }
    h.run_steps(2);
    h.get_by_label("Chapter 1");
    // An edit while a page is being read: the reading, of a state that is gone, is dropped.
    let app = h.state_mut();
    app.chess.text = None;
    app.read_chess_text(0, false);
    assert!(app.chess.job.is_some(), "reading on a worker");
    let shape = Shape::Rectangle { rect: [10.0, 10.0, 50.0, 50.0] };
    app.apply_edit(Edit::AddAnnotation(NewAnnotation {
        page: 0,
        style: Style::default_for(&shape),
        shape,
        contents: String::new(),
        author: String::new(),
    }));
    for _ in 0..500 {
        app.poll_chess_job();
        if app.chess.job.is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(app.chess.job.is_none(), "the worker finished");
    assert!(app.chess.text.is_none(), "the stale reading was not shown");
}

/// Book origin: the user says where the document's pages came from. The choice is kept with the
/// document, and the page shown is read again under it (a scanned book with chess OCR).
#[test]
fn the_book_origin_is_kept_and_the_page_is_read_again_under_it() {
    use pdfcraft_engine::chess::document::BookOrigin;
    let mut h = chapter_harness();
    h.get_by_label("Text").click();
    h.run_steps(4);
    assert_eq!(h.state().chess.text.as_ref().map(|t| t.origin), Some(BookOrigin::Auto));
    // The combo box shows the origin chosen; each choice reads the page again.
    for (shown, label, origin) in [
        ("Auto (page by page)", "Born digital (text layer only)", BookOrigin::Digital),
        ("Born digital (text layer only)", "Scanned (chess OCR on every page)", BookOrigin::Scanned),
    ] {
        // A combo box shows its choice as its value.
        h.get_by(|n| n.role() == egui::accesskit::Role::ComboBox && n.value().is_some_and(|v| v == shown)).click();
        h.run_steps(2);
        h.get_all_by_label(label).last().expect("the choice is listed").click();
        h.run_steps(4);
        let app = h.state();
        assert_eq!(app.chess.origin.map(|(_, o)| o), Some(origin), "{label}");
        let text = app.chess.text.as_ref().expect("the page was read again");
        assert_eq!(text.origin, origin, "{label}");
        // The page's own layer is good, so a born-digital read has nothing to note.
        if origin == BookOrigin::Digital {
            assert!(text.needs_ocr.is_none() && !text.ocr, "{:?}", text.needs_ocr);
        }
    }
}

/// Click the square `name` on the analysis board (from White's side).
fn click_square(h: &mut Harness<'static, PdfCraftApp>, name: &str) {
    let b = name.as_bytes();
    let (file, rank) = (f32::from(b[0] - b'a'), f32::from(b[1] - b'1'));
    let board = h.get_by_label_contains("Board: ").rect();
    let at = board.min + egui::vec2(board.width() * (file + 0.5) / 8.0, board.height() * (7.5 - rank) / 8.0);
    h.hover_at(at);
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
    }
    h.run_steps(2);
}

/// A double-click on a diagram on the page puts it on the chess board, a tab beside the
/// Inspector, where moves are played by clicking and walked back and forth.
#[test]
fn double_clicking_a_diagram_puts_it_on_the_board_to_play() {
    // Short frames, so two clicks a frame apart are within the 0.3 s double-click delay.
    let mut h = harness_stepping(0.05);
    assert!(h.state_mut().execute("chess.scan"));
    h.run_steps(3);
    let board = pdfcraft_ui_egui::docking::Panel::Board;
    assert!(!h.state().docking.layout.contains(&board), "the board is not open until a diagram goes onto it");
    let at = on_page(&h, 140.0, 660.0);
    h.hover_at(at);
    h.run_steps(1);
    for _ in 0..2 {
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
        }
        h.step();
    }
    h.run_steps(3);
    assert!(h.state().docking.layout.contains(&board), "the double-click opened the board");
    assert_eq!(h.state().chess.board().game.fen(), START_FEN);
    assert_eq!(h.state().chess.board().source.map(|s| (s.page, s.index)), Some((0, 0)));
    h.get_by_label("Diagram 1 · Page 1");
    h.get_by_label("White to move");

    click_square(&mut h, "e2");
    click_square(&mut h, "e4");
    click_square(&mut h, "e7");
    click_square(&mut h, "e5");
    assert_eq!(h.state().chess.board().game.movetext(), "1. e4 e5");
    h.get_by_label("e5");
    // Back one move with the button, then a different move starts a variation.
    h.get_by_label("Previous move (Left arrow)").click();
    h.run_steps(2);
    assert_eq!(h.state().chess.board().game.ply(), 1);
    click_square(&mut h, "c7");
    click_square(&mut h, "c5");
    assert_eq!(h.state().chess.board().game.movetext(), "1. e4 e5 (1... c5)");
    assert!(h.state().chess.board().pgn_text.ends_with("1. e4 e5 (1... c5) *\n"), "{}", h.state().chess.board().pgn_text);
    // A glyph on the move shown, from the rail's Move glyphs group (named for the glyphs).
    h.get_by_label("Good move").click_secondary();
    h.run_steps(2);
    h.get_by_label("Interesting move").click();
    h.run_steps(2);
    let c5 = h.state().chess.board().game.current();
    assert_eq!(h.state().chess.board().game.nags(c5), &[5]);
    // The move's menu makes the variation the main line.
    h.get_by_label("c5!?").click_secondary();
    h.run_steps(2);
    h.get_by_label("Make this the main line").click();
    h.run_steps(2);
    assert_eq!(h.state().chess.board().game.movetext(), "1. e4 c5 $5 (1... e5)");
    // Copy FEN, in the FEN section (closed until opened), copies the position on the board.
    h.get_by_label("FEN").click();
    h.run_steps(2);
    h.get_by_label("Copy FEN").click();
    let mut copied = false;
    for _ in 0..3 {
        h.step();
        copied |= h
            .output()
            .platform_output
            .commands
            .iter()
            .any(|c| matches!(c, egui::OutputCommand::CopyText(t) if t == "rnbqkbnr/pp1ppppp/8/2p5/4P3/8/PPPP1PPP/RNBQKBNR w KQkq c6 0 2"));
    }
    assert!(copied, "the board's FEN was copied");
    // A comment typed in two goes keeps the space between them: the game trims what it stores,
    // and the field must not take that back while it is being typed in.
    let comment = h.get_all_by_role(egui::accesskit::Role::MultilineTextInput).find(|n| n.value().unwrap_or_default().is_empty()).unwrap();
    comment.focus();
    comment.type_text("good ");
    h.run_steps(2);
    h.get_all_by_role(egui::accesskit::Role::MultilineTextInput).find(|n| n.value().as_deref() == Some("good ")).unwrap().type_text("move");
    h.run_steps(2);
    let node = h.state().chess.board().game.current();
    assert_eq!(h.state().chess.board().game.comment(node), "good move");
}

/// Two clicks at `at`, a frame apart: a double-click (with frames under egui's 0.3 s delay).
fn double_click_at(h: &mut Harness<'static, PdfCraftApp>, at: Pos2) {
    h.hover_at(at);
    h.run_steps(1);
    for _ in 0..2 {
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
        }
        h.step();
    }
    h.run_steps(3);
}

/// A double-click on a diagram of a page not scanned yet scans the page, then puts the diagram
/// on the board: there is no Chess tool to arm first. The scan is quiet (the board is what it
/// was for); a double-click beside the diagrams scans the page, opens nothing and says so, and
/// one that selected a word says nothing (selecting it may be all it was for).
#[test]
fn double_clicking_a_diagram_not_scanned_yet_scans_its_page_and_puts_it_on_the_board() {
    let board = pdfcraft_ui_egui::docking::Panel::Board;
    let mut h = harness_stepping(0.05);
    assert!(h.state().chess.results.is_none(), "nothing scanned yet");
    let at = on_page(&h, 140.0, 660.0);
    double_click_at(&mut h, at);
    assert!(h.state().chess.run.is_none(), "the scan ran inline and finished");
    assert_eq!(diagrams(&h), 2, "the page was scanned");
    assert!(h.state().docking.layout.contains(&board), "the double-click opened the board");
    assert_eq!(h.state().chess.board().game.fen(), START_FEN);
    assert_eq!(h.state().chess.board().source.map(|s| (s.page, s.index)), Some((0, 0)));
    assert!(h.state().chess.board_at.is_none(), "the double-click was dealt with");
    assert_ne!(h.state().toast.as_ref().map(|t| t.0.as_str()), Some("Diagrams found: 2"), "a quiet scan");

    // Beside the diagrams (white space on the right): the page is scanned, nothing opens.
    let mut h = harness_stepping(0.05);
    let at = on_page(&h, 450.0, 500.0);
    double_click_at(&mut h, at);
    assert_eq!(diagrams(&h), 2, "the page was scanned");
    assert!(!h.state().docking.layout.contains(&board), "no diagram there");
    assert!(h.state().chess.board_at.is_none());
    assert_eq!(h.state().toast.as_ref().map(|t| t.0.as_str()), Some("No chess diagram was found here."));

    // On a word of the text: the word is selected, the page scanned, and nothing is said.
    let mut h = harness_on(boards_with("12...e5 13.Nf3 Nc6"), 0.05);
    support::wait_until(&mut h, "the page's text layer", |app| app.views[0].glyph_screen_pos(0, 0).is_some());
    let at = on_page(&h, 66.0, 544.0);
    double_click_at(&mut h, at);
    assert_eq!(h.state().views[0].selected_text().as_deref(), Some("12...e5"), "the double-click selected a word");
    assert_eq!(diagrams(&h), 2, "the page was scanned");
    assert!(!h.state().docking.layout.contains(&board));
    assert_eq!(h.state().toast.as_ref().map(|t| t.0.as_str()), None);
}

/// A diagram opened on the board after its page was read in the Text tab starts at the move the
/// moves under it give it: the reading assumed White to move at 1, `12...e5` makes it Black at 12.
#[test]
fn a_diagram_on_the_board_takes_the_side_and_move_number_its_moves_give_it() {
    let mut h = harness_on(boards_with("12...e5 13.Nf3 Nc6"), 0.1);
    assert!(h.state_mut().execute("chess.scan"));
    h.run_steps(3);
    let read = h.state().chess.results.as_ref().and_then(|r| r.diagram(0, 0)).and_then(|d| d.1.map(str::to_owned));
    assert_eq!(read.as_deref(), Some(START_FEN), "the reading alone knows no better");
    h.get_by_label("Text").click();
    h.run_steps(4);
    h.get_by_label("Recognised text of page 1");
    h.get_by_label("Diagrams").click();
    h.run_steps(2);
    h.get_all_by_label("Open on the board").next().expect("diagram 1 has a position").click();
    h.run_steps(3);
    assert_eq!(h.state().chess.board().game.fen(), "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR b KQkq - 0 12");
    assert_eq!(h.state().chess.board().game.first_move_number(), 12, "the Move number field shows it");
}

/// A double-click on a diagram whose page the Text tab has not read reads it (off the interface
/// thread, inline here), and the moves under the diagram give the game on the board its side to
/// move and move number, as no step of its own (opening the diagram is one step of the list,
/// which Undo takes back whole): on a page scanned already, and on one the double-click scans
/// first.
#[test]
fn a_double_clicked_diagram_reads_its_page_for_the_move_number() {
    const AT_12: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR b KQkq - 0 12";
    for scanned in [true, false] {
        let mut h = harness_on(boards_with("12...e5 13.Nf3 Nc6"), 0.05);
        if scanned {
            assert!(h.state_mut().execute("chess.scan"));
            h.run_steps(3);
        }
        assert!(h.state().chess.text.is_none(), "the Text tab has read nothing");
        let at = on_page(&h, 140.0, 660.0);
        double_click_at(&mut h, at);
        h.run_steps(3);
        assert_eq!(h.state().chess.board().source.map(|s| (s.page, s.index)), Some((0, 0)));
        assert_eq!(h.state().chess.board().game.fen(), AT_12, "scanned first: {scanned}");
        assert_eq!(h.state().chess.board().game.first_move_number(), 12);
        assert_eq!(h.state().chess.board().undo_label(), None, "the move number is not a step of the game");
        assert!(h.state().chess.board_text.is_none(), "dealt with");
        // Opening it is a step of the list: Undo takes the game out, Redo puts it back with the
        // start its page's moves gave it.
        assert!(h.state_mut().execute("edit.undo"));
        h.run_steps(2);
        assert_eq!(h.state().toast.as_ref().map(|t| t.0.as_str()), Some("Undid Load the diagram"));
        assert_eq!((h.state().chess.games.collection().len(), h.state().chess.board().source), (1, None), "a blank game is left");
        assert!(h.state_mut().execute("edit.redo"));
        h.run_steps(2);
        assert_eq!(h.state().chess.board().game.fen(), AT_12);
        // Played on from there: Black's move.
        click_square(&mut h, "e7");
        click_square(&mut h, "e5");
        assert_eq!(h.state().chess.board().game.movetext(), "12... e5");
    }
}

/// Open on the board, in the Chess diagrams panel, puts that diagram on the Board panel and
/// brings it forward (T5c: the Board panel is the study board; the panel has no Study tab).
#[test]
fn open_on_the_board_from_the_panel() {
    let board = pdfcraft_ui_egui::docking::Panel::Board;
    let mut h = harness();
    assert!(h.state_mut().execute("chess.scan"));
    h.run_steps(3);
    assert!(h.query_by_label("Study board").is_none(), "the Chess panel's tabs are Diagrams and Text");
    assert!(!h.state().docking.layout.contains(&board));
    h.get_all_by_label("Open on the board").next().expect("diagram 1 has a position").click();
    h.run_steps(3);
    assert!(h.state().docking.layout.contains(&board), "the Board panel opened");
    assert_eq!(format!("{:?}", h.state().chess.tab), "Diagrams", "the Chess panel keeps its tab");
    assert_eq!(h.state().chess.board().game.fen(), START_FEN);
    // In front: its board is drawn, the rail's set-up tool and the Analysis section with it.
    h.get_by_label(&format!("Board: {START_FEN}"));
    h.get_by_label("Set up a position");
    h.get_by_label("Analysis");
    click_square(&mut h, "e2");
    click_square(&mut h, "e4");
    assert_eq!(h.state().chess.board().game.movetext(), "1. e4");
}

/// The move number beside "To move:" on the Board panel corrects a game whose diagram was read
/// without its number: every move is renumbered, in one undo step, and moves can be played on.
#[test]
fn the_board_panel_corrects_the_move_number() {
    let mut h = harness();
    assert!(h.state_mut().execute("chess.scan"));
    h.run_steps(3);
    h.get_all_by_label("Open on the board").next().expect("diagram 1 has a position").click();
    h.run_steps(3);
    click_square(&mut h, "e2");
    click_square(&mut h, "e4");
    click_square(&mut h, "e7");
    click_square(&mut h, "e5");
    assert_eq!(h.state().chess.board().game.movetext(), "1. e4 e5");
    h.get_by_label("Move number:");
    // Clicked where it is drawn: the field is in view, not under the panel's edge.
    h.get_by_role(egui::accesskit::Role::SpinButton).click();
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.get_by_role(egui::accesskit::Role::SpinButton).type_text("35");
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert_eq!(h.state().chess.board().game.movetext(), "35. e4 e5");
    assert_eq!(h.state().chess.board().game.start_fen(), START_FEN.replace(" 0 1", " 0 35"));
    click_square(&mut h, "g1");
    click_square(&mut h, "f3");
    assert_eq!(h.state().chess.board().game.movetext(), "35. e4 e5 36. Nf3");
    // Undo the move, then the number: typing it was one step.
    assert!(h.state_mut().chess.board_mut().undo());
    assert!(h.state_mut().chess.board_mut().undo());
    assert_eq!(h.state().chess.board().game.movetext(), "1. e4 e5");
}

/// The same with the chess panel in its own window (narrow, on its Diagrams tab): Open on the
/// board shows the window's board, not a tab of the Inspector.
#[test]
fn open_on_the_board_shows_the_windows_board() {
    let settings = window_settings(serde_json::json!({"detached": true, "size": [560.0, 820.0], "tab": "diagrams"}));
    let mut h = harness_restoring(settings);
    assert!(h.state_mut().execute("chess.scan"));
    h.run_steps(3);
    assert_eq!(h.state().chess_window_tab(), pdfcraft_ui_egui::ChessWindowTab::Diagrams);
    assert!(h.query_by_label_contains("Board: ").is_none());
    h.get_all_by_label("Open on the board").next().expect("diagram 1 has a position").click();
    h.run_steps(3);
    assert_eq!(h.state().chess_window_tab(), pdfcraft_ui_egui::ChessWindowTab::Board);
    assert!(!h.state().docking.layout.contains(&pdfcraft_ui_egui::docking::Panel::Board), "not docked");
    assert_eq!(h.get_all_by_label(&format!("Board: {START_FEN}")).count(), 1, "the window's board, once");
    h.get_by_label("Analysis");
    assert_eq!(format!("{:?}", h.state().chess.tab), "Diagrams");
}

/// Press `button` on square `from`, move to `to` and let go there, holding `modifiers` when
/// letting go.
fn drag_square(h: &mut Harness<'static, PdfCraftApp>, button: egui::PointerButton, from: &str, to: &str, modifiers: egui::Modifiers) {
    let at = |h: &Harness<'static, PdfCraftApp>, name: &str| {
        let b = name.as_bytes();
        let (file, rank) = (f32::from(b[0] - b'a'), f32::from(b[1] - b'1'));
        let board = h.get_by_label_contains("Board: ").rect();
        board.min + egui::vec2(board.width() * (file + 0.5) / 8.0, board.height() * (7.5 - rank) / 8.0)
    };
    let (a, b) = (at(h, from), at(h, to));
    h.hover_at(a);
    h.run_steps(1);
    h.event(egui::Event::PointerButton { pos: a, button, pressed: true, modifiers: egui::Modifiers::NONE });
    h.run_steps(1);
    for k in 1..=4u8 {
        h.event(egui::Event::PointerMoved(a + (b - a) * (f32::from(k) / 4.0)));
        h.run_steps(1);
    }
    h.event(egui::Event::ModifiersChanged(modifiers));
    h.event(egui::Event::PointerButton { pos: b, button, pressed: false, modifiers });
    h.run_steps(1);
    h.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    h.run_steps(2);
}

/// The drawings on the position shown, as `fromtoColour`.
fn marks(h: &Harness<'static, PdfCraftApp>) -> Vec<String> {
    h.state().chess.board().marks().iter().map(|m| format!("{}{}{:?}", m.from, m.to, m.color)).collect()
}

/// The Board panel as the study board (T5c): a position set up with the rail's Set up tool from
/// an empty board and played from (Done is one undo step), drawings made with the right button
/// and cleared in the Analysis section, and the engine switch there saying when its engine
/// cannot be started.
#[test]
fn the_board_panel_sets_up_a_position_and_draws_on_it() {
    let mut h = harness();
    h.state_mut().show_chess_board();
    h.run_steps(3);
    click_square(&mut h, "d2");
    click_square(&mut h, "d4");
    assert_eq!(h.state().chess.board().game.movetext(), "1. d4");
    h.get_by_label("Set up a position").click();
    h.run_steps(2);
    assert_eq!(format!("{:?}", h.state().chess.tools.tool), "Setup");
    assert!(h.query_by_label("Analysis").is_none(), "the set-up view takes the board's place");
    h.get_by_label("Empty board").click();
    h.run_steps(2);
    // The set-up starts from the position shown, Black to move after 1. d4.
    h.get_by_label("White").click();
    h.run_steps(2);
    for (piece, square) in [("White king", "e1"), ("Black king", "e8"), ("White rook", "a1")] {
        h.get_by_label(piece).click();
        h.run_steps(1);
        click_square(&mut h, square);
    }
    h.get_by_label("Legal position");
    // With moves on the board, Done is a new game; replacing the game is said in so many words.
    assert!(h.query_by_label("Done").is_none());
    h.get_by_label("Replace this game").click();
    h.run_steps(2);
    assert!(h.state().chess.study.setup.is_none());
    // The move it took away is told, with Undo.
    assert_eq!(h.state().chess.tools.deleted.text(), Some("1 move deleted"));
    assert_eq!(format!("{:?}", h.state().chess.tools.tool), "Navigate", "back to Navigate");
    assert_eq!(h.state().chess.board().game.fen(), "4k3/8/8/8/8/8/8/R3K3 w - - 0 1");
    // One undo step, named for the set-up, takes the game back as it was.
    assert_eq!(h.state().chess.board().undo_label().as_deref(), Some("Position set-up"));
    assert!(h.state_mut().chess.board_mut().undo());
    assert_eq!(h.state().chess.board().game.movetext(), "1. d4");
    assert!(h.state_mut().chess.board_mut().redo());
    h.run_steps(2);
    click_square(&mut h, "a1");
    click_square(&mut h, "a8");
    assert_eq!(h.state().chess.board().game.movetext(), "1. Ra8+");

    // A right click circles a square; a right drag with Shift held draws a red arrow.
    drag_square(&mut h, egui::PointerButton::Secondary, "d4", "d4", egui::Modifiers::NONE);
    drag_square(&mut h, egui::PointerButton::Secondary, "e8", "d7", egui::Modifiers::SHIFT);
    assert_eq!(marks(&h), ["d4d4Green", "e8d7Red"]);
    // The drawings stay with their position.
    h.get_by_label("Previous move (Left arrow)").click();
    h.run_steps(2);
    assert!(marks(&h).is_empty());
    h.get_by_label("Next move (Right arrow)").click();
    h.run_steps(2);
    assert_eq!(marks(&h).len(), 2);
    // The Analysis section, closed until opened, clears them and says how they are drawn.
    assert!(h.query_by_label("Clear drawings").is_none());
    h.get_by_label("Analysis").click();
    h.run_steps(2);
    h.get_by_label_contains("Right-click a square to circle it");
    h.get_by_label("Clear drawings").click();
    h.run_steps(2);
    assert!(marks(&h).is_empty());

    // Analysis with an engine that cannot be started says so; switched off, it says nothing.
    h.state_mut().chess.study.engine_path = Some("this-engine-does-not-exist-anywhere.exe".into());
    h.get_by_label("Engine analysis").click();
    h.run_steps(3);
    assert!(h.state().chess.study.analysis);
    assert!(h.get_all_by_label_contains("The engine couldn't be started").next().is_some());
    h.get_by_label("Engine analysis").click();
    h.run_steps(3);
    assert!(!h.state().chess.study.analysis);
    assert!(h.query_by_label_contains("The engine couldn't be started").is_none());
    // Choose engine… takes the program picked and switches the analysis on with it.
    h.state_mut().pick_override = Some(vec!["another-engine-that-does-not-exist.exe".into()]);
    h.get_by_label("Choose engine…").click();
    h.run_steps(3);
    assert_eq!(h.state().chess.study.engine_path.as_deref(), Some(std::path::Path::new("another-engine-that-does-not-exist.exe")));
    assert!(h.state().chess.study.analysis);
    assert!(h.get_all_by_label_contains("The engine couldn't be started").next().is_some());
    h.get_by_label("Engine analysis").click();
    h.run_steps(3);
    assert!(!h.state().chess.study.analysis);

    // The set-up ends when another game takes the board, and Esc ends it too.
    h.get_by_label("Set up a position").click();
    h.run_steps(2);
    assert_eq!(format!("{:?}", h.state().chess.tools.tool), "Setup");
    assert!(h.state_mut().execute("chess.game.new"));
    h.run_steps(3);
    assert_eq!(format!("{:?}", h.state().chess.tools.tool), "Navigate");
    h.get_by_label("Analysis");
    assert!(h.state_mut().execute("chess.tool.setup"));
    h.run_steps(2);
    assert!(h.query_by_label("Empty board").is_some(), "the command arms the tool");
    // A click in the panel (a piece put on the set-up board) gives the chess keys to it.
    click_square(&mut h, "h5");
    host_key(&mut h, egui::Modifiers::NONE, egui::Key::Escape);
    assert_eq!(format!("{:?}", h.state().chess.tools.tool), "Navigate");
    assert!(h.query_by_label("Empty board").is_none());
}

/// U7: the drawings belong to the move shown. Each one is an undo step, they go into the game's
/// PGN as `[%csl]`/`[%cal]` in the move's comment (the comment's text kept after them), a PGN
/// loaded with them shows them, moving a piece leaves them, and Clear drawings is an undo step.
#[test]
fn drawings_belong_to_the_move_and_go_into_the_pgn() {
    let mut h = harness();
    h.state_mut().show_chess_board();
    h.run_steps(3);
    click_square(&mut h, "e2");
    click_square(&mut h, "e4");
    drag_square(&mut h, egui::PointerButton::Secondary, "d5", "d5", egui::Modifiers::NONE);
    drag_square(&mut h, egui::PointerButton::Secondary, "g1", "f3", egui::Modifiers::SHIFT);
    assert_eq!(marks(&h), ["d5d5Green", "g1f3Red"]);
    let board = h.state().chess.board();
    let e4 = board.game.current();
    assert_eq!(board.game.drawings(e4).len(), 2, "on the move shown");
    assert!(board.game.drawings(0).is_empty());
    assert!(board.pgn_text.contains("1. e4 {[%csl Gd5][%cal Rg1f3]} *"), "{}", board.pgn_text);
    assert_eq!(board.undo_label().as_deref(), Some("Draw on the board"));
    // Moving a piece (left clicks on the board) leaves them on their move.
    click_square(&mut h, "e7");
    click_square(&mut h, "e5");
    assert_eq!(h.state().chess.board().game.movetext(), "1. e4 {[%csl Gd5][%cal Rg1f3]} 1... e5");
    assert!(marks(&h).is_empty(), "the next move has its own");
    h.get_by_label("Previous move (Left arrow)").click();
    h.run_steps(2);
    assert_eq!(marks(&h).len(), 2);
    // Undo takes the move back, then the arrow; redo draws it again.
    assert!(h.state_mut().chess.board_mut().undo());
    assert_eq!(h.state().chess.board().undo_label().as_deref(), Some("Draw on the board"));
    assert!(h.state_mut().chess.board_mut().undo());
    h.run_steps(2);
    assert_eq!(marks(&h), ["d5d5Green"]);
    assert!(h.state_mut().chess.board_mut().redo());
    h.run_steps(2);
    assert_eq!(marks(&h), ["d5d5Green", "g1f3Red"]);
    // Clear drawings in the Analysis section is a step too.
    h.get_by_label("Analysis").click();
    h.run_steps(2);
    h.get_by_label("Clear drawings").click();
    h.run_steps(2);
    assert!(marks(&h).is_empty());
    assert_eq!(h.state().chess.board().undo_label().as_deref(), Some("Clear drawings"));
    assert!(h.state_mut().chess.board_mut().undo());
    h.run_steps(2);
    assert_eq!(marks(&h).len(), 2);

    // A PGN with drawings, and other commands in the comment, puts them on their move.
    put_game(&mut h, "1. d4 {Center [%clk 0:10:00] [%csl Yd4][%cal Bc1g5]} 1... d5 *", &[0, 0]);
    let game = &h.state().chess.board().game;
    let d4 = game.main_line()[1];
    assert_eq!(game.comment(d4), "Center [%clk 0:10:00]");
    assert_eq!(game.movetext(), "1. d4 {[%csl Yd4][%cal Bc1g5] Center [%clk 0:10:00]} 1... d5");
    assert!(marks(&h).is_empty(), "d5 has none");
    h.get_by_label("Previous move (Left arrow)").click();
    h.run_steps(2);
    assert_eq!(marks(&h), ["d4d4Yellow", "c1g5Blue"]);
}

/// U7: the engine picked and the lines shown come back in the next session, without starting
/// analysis; a bad saved value is ignored on its own (the engine is then looked for as when none
/// was picked), never a panic.
#[test]
fn the_chosen_engine_is_remembered_and_bad_settings_are_ignored() {
    // A file that exists (and is no program, so analysis switched on can't start it), to stand
    // for the engine.
    let exe = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let mut h = harness();
    h.state_mut().chess.study.engine_path = Some(exe.clone());
    h.state_mut().chess.study.lines = 2;
    let saved: serde_json::Value = serde_json::from_str(&h.state().persist()).unwrap();
    assert_eq!(saved["chess_engine"]["path"].as_str(), exe.to_str(), "{saved}");
    assert_eq!(saved["chess_engine"]["lines"], 2);
    let h = harness_restoring(saved.to_string());
    let study = &h.state().chess.study;
    assert_eq!((study.engine_path.as_deref(), study.lines), (Some(exe.as_path()), 2));
    assert!(!study.analysis, "the engine is not started by itself");
    // Choose engine… keeps the program picked for the next session.
    let mut h = harness();
    h.state_mut().show_chess_board();
    h.run_steps(3);
    h.get_by_label("Analysis").click();
    h.run_steps(2);
    h.state_mut().pick_override = Some(vec![exe.display().to_string()]);
    h.get_by_label("Choose engine…").click();
    h.run_steps(3);
    let saved: serde_json::Value = serde_json::from_str(&h.state().persist()).unwrap();
    assert_eq!(saved["chess_engine"]["path"].as_str(), exe.to_str(), "{saved}");
    h.get_by_label("Engine analysis").click();
    h.run_steps(2);
    // Bad values: each ignored, the rest used.
    let missing = exe.with_file_name("no-such-engine-anywhere.exe");
    let long = format!("{}{}", exe.display(), "x".repeat(5000));
    for bad in [
        serde_json::json!("yes"),
        serde_json::json!([1, 2]),
        serde_json::json!({"path": 3, "lines": 9}),
        serde_json::json!({"path": "engines/stockfish.exe", "lines": 0}),
        serde_json::json!({"path": missing, "lines": -1}),
        serde_json::json!({"path": format!("{}\u{0}", exe.display()), "lines": 2.5}),
        serde_json::json!({"path": long, "lines": "2"}),
        serde_json::json!({"path": "", "lines": 4_294_967_297u64}),
        serde_json::Value::Null,
    ] {
        let h = harness_restoring(serde_json::json!({ "chess_engine": bad }).to_string());
        let study = &h.state().chess.study;
        assert_eq!((study.engine_path.as_deref(), study.lines, study.analysis), (None, 3, false), "{bad}");
    }
    let h = harness_restoring(serde_json::json!({ "chess_engine": {"path": "relative.exe", "lines": 1} }).to_string());
    assert_eq!((h.state().chess.study.engine_path.as_deref(), h.state().chess.study.lines), (None, 1), "good lines kept beside a bad path");
}

/// A pretend UCI engine on the other end of two pipes: it reports depths 1–3 of a line from
/// e2e4 (e7e5 for Black) at +0.30 for the side to move, then waits for `stop`, as an engine
/// searching deeper does. Every command it is given goes into the log.
fn fake_engine() -> (pdfcraft_engine::chess::analysis::Analyser, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    use std::io::{BufRead, Write};
    let (engine_in, our_out) = std::io::pipe().unwrap();
    let (our_in, mut engine_out) = std::io::pipe().unwrap();
    let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let commands = std::sync::Arc::clone(&log);
    std::thread::spawn(move || {
        let mut position = String::new();
        for line in std::io::BufReader::new(engine_in).lines() {
            let Ok(line) = line else { break };
            commands.lock().unwrap().push(line.clone());
            let mv = if position.contains(" b ") { "e7e5" } else { "e2e4" };
            let mut say = |text: &str| {
                let _ = writeln!(engine_out, "{text}");
            };
            match line.split_whitespace().next() {
                Some("uci") => {
                    say("id name Fake Engine 1");
                    say("option name MultiPV type spin default 1 min 1 max 5");
                    say("uciok");
                }
                Some("position") => position = line.clone(),
                Some("go") => {
                    for d in 1..=3 {
                        say(&format!("info depth {d} multipv 1 score cp 30 nodes 100 nps 1000 pv {mv}"));
                    }
                }
                Some("stop") => say(&format!("bestmove {mv}")),
                Some("quit") => break,
                _ => {}
            }
        }
    });
    // The study board's own options, so that none changes when it takes the engine.
    let options = pdfcraft_engine::chess::analysis::AnalysisOptions::default();
    (pdfcraft_engine::chess::analysis::Analyser::with_streams(our_in, our_out, options, std::sync::Arc::new(|| {})), log)
}

/// Run frames until `done` holds (at most about five seconds: the engine answers on its own
/// threads).
fn wait_for(h: &mut Harness<'static, PdfCraftApp>, done: impl Fn(&Harness<'static, PdfCraftApp>) -> bool) -> bool {
    for _ in 0..100 {
        h.run_steps(1);
        if done(h) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    false
}

/// With an engine, the Analysis section's switch starts and closes the analysis: the lines in
/// the section, the evaluation bar beside the board and the best move on it.
#[test]
fn the_analysis_section_switches_the_engine() {
    let mut h = harness();
    h.state_mut().show_chess_board();
    h.run_steps(3);
    let (engine, log) = fake_engine();
    h.state_mut().chess.study.use_engine(engine);
    // The bar beside the board makes the board narrower, and what is under it moves up.
    h.run_steps(2);
    h.get_by_label("Analysis").click();
    // The score twice: the line's, and the bar's name.
    let reported = wait_for(&mut h, |h| h.query_by_label_contains("Fake Engine 1").is_some() && h.query_all_by_label("+0.30").count() == 2);
    assert!(reported, "the engine's line is shown");
    // Drawn frame after frame, the board keeps the one search going: it is not stopped and
    // started again each frame.
    h.run_steps(5);
    let searches = log.lock().unwrap().iter().filter(|c| c.starts_with("go ")).count();
    assert_eq!(searches, 1, "{:?}", log.lock().unwrap());
    assert!(!log.lock().unwrap().iter().any(|c| c == "stop"));
    assert!(log.lock().unwrap().iter().any(|c| c == &format!("position fen {START_FEN}")), "{:?}", log.lock().unwrap());
    // The bar, named by its score, is beside the board, as tall as it.
    let board = h.get_by_label(&format!("Board: {START_FEN}")).rect();
    let bar = h.get_all_by_label("+0.30").map(|n| n.rect()).find(|r| r.max.x <= board.min.x).expect("the evaluation bar");
    assert!((bar.height() - board.height()).abs() < 1.0 && bar.width() < 20.0, "{bar:?} {board:?}");
    if let Ok(dir) = std::env::var("PDFCRAFT_CHESS_SHOTS") {
        h.render().unwrap().save(std::path::Path::new(&dir).join("study-board-analysis.png")).unwrap();
    }
    // A click on the line's move plays it.
    h.get_by_label("e4").click();
    h.run_steps(3);
    assert_eq!(h.state().chess.board().game.movetext(), "1. e4");
    // Switched off: the engine is closed and the bar goes.
    h.get_by_label("Engine analysis").click();
    h.run_steps(3);
    assert!(!h.state().chess.study.analysis);
    assert!(h.query_by_label_contains("0.30").is_none(), "no bar, no lines");
    assert!(wait_for(&mut h, |_| log.lock().unwrap().iter().any(|c| c == "quit")), "the engine was told to quit");
}

/// The engine rests while the Board panel is not drawn (T5c: the board is the study board, so
/// its being shown is what counts), and analyses again once it is back.
#[test]
fn analysis_pauses_while_the_board_panel_is_hidden() {
    let board = pdfcraft_ui_egui::docking::Panel::Board;
    let mut h = harness();
    h.state_mut().show_chess_board();
    h.run_steps(3);
    let (engine, log) = fake_engine();
    h.state_mut().chess.study.use_engine(engine);
    let searching = |log: &std::sync::Mutex<Vec<String>>| log.lock().unwrap().iter().filter(|c| c.starts_with("go ")).count();
    assert!(wait_for(&mut h, |_| searching(&log) == 1), "the engine analyses the board shown");
    assert!(!log.lock().unwrap().iter().any(|c| c == "stop"));
    h.state_mut().docking.set_visible(board, false).unwrap();
    assert!(wait_for(&mut h, |_| log.lock().unwrap().iter().any(|c| c == "stop")), "the engine stops: {:?}", log.lock().unwrap());
    assert!(h.state().chess.study.analysis, "analysis stays on, to go on when the board is back");
    // Back, on the position it was stopped on: it searches on.
    h.state_mut().show_chess_board();
    assert!(wait_for(&mut h, |_| searching(&log) == 2), "the engine searches on: {:?}", log.lock().unwrap());
    // On a new position: analysed.
    click_square(&mut h, "e2");
    click_square(&mut h, "e4");
    assert!(wait_for(&mut h, |_| searching(&log) == 3), "{:?}", log.lock().unwrap());
    let after_e4 = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1";
    assert!(log.lock().unwrap().iter().any(|c| c == &format!("position fen {after_e4}")));
}

/// With a real UCI engine (`PDFCRAFT_CHESS_TEST_ENGINE`, e.g. Stockfish), the Board panel's
/// Analysis section shows its lines and plays one when its move is clicked. Skipped when the
/// variable is not set.
#[test]
fn the_board_panel_analyses_with_a_real_engine() {
    let Some(engine) = std::env::var_os("PDFCRAFT_CHESS_TEST_ENGINE") else {
        eprintln!("PDFCRAFT_CHESS_TEST_ENGINE not set: skipped");
        return;
    };
    let mut h = harness();
    h.state_mut().show_chess_board();
    h.run_steps(3);
    h.get_by_label("Analysis").click();
    h.run_steps(2);
    h.state_mut().chess.study.engine_path = Some(engine.into());
    h.get_by_label("Engine analysis").click();
    let mut found = false;
    for _ in 0..200 {
        h.run_steps(1);
        if h.query_by_label_contains("depth ").is_some() && h.query_all_by_label("1.").next().is_some() {
            found = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(found, "the engine reported a line");
    // A click on the first move of the best line plays it.
    let first = h.get_all_by_label("1.").next().unwrap().rect();
    let mv = h
        .get_all_by_role(egui::accesskit::Role::Label)
        .find(|n| n.rect().min.x > first.max.x && (n.rect().center().y - first.center().y).abs() < 4.0)
        .expect("the move after the number");
    mv.click();
    h.run_steps(3);
    assert_eq!(h.state().chess.board().game.len(), 1, "{}", h.state().chess.board().game.movetext());
}

/// Open the Chess board panel on the standard start position, after a PDF edit (a rectangle in a
/// corner of page 1). Returns what the document's Undo would undo.
fn board_after_a_pdf_edit(h: &mut Harness<'static, PdfCraftApp>) -> String {
    let shape = Shape::Rectangle { rect: [10.0, 10.0, 50.0, 50.0] };
    let style = Style::default_for(&shape);
    assert!(h.state_mut().apply_edit(Edit::AddAnnotation(NewAnnotation { page: 0, style, shape, contents: String::new(), author: String::new() })));
    h.state_mut().show_chess_board();
    h.run_steps(3);
    pdf_undo(h).expect("the rectangle can be undone")
}

/// What the document's Undo would undo.
fn pdf_undo(h: &Harness<'static, PdfCraftApp>) -> Option<String> {
    let app = h.state();
    app.session.get(app.views[0].id).unwrap().can_undo().map(str::to_owned)
}

/// Press `key` with Ctrl (Command on macOS), and Shift when `shift`.
fn shortcut(h: &mut Harness<'static, PdfCraftApp>, key: egui::Key, shift: bool) {
    let m = if shift { egui::Modifiers::COMMAND | egui::Modifiers::SHIFT } else { egui::Modifiers::COMMAND };
    h.key_press_modifiers(m, key);
    h.run_steps(2);
}

/// Click at `at` with the primary button, as a pointer would.
fn click_at(h: &mut Harness<'static, PdfCraftApp>, at: Pos2) {
    h.hover_at(at);
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
    }
    h.run_steps(2);
}

/// How many nodes are labelled `label` while the palette lists the Undo and Redo commands.
fn palette_labels(h: &mut Harness<'static, PdfCraftApp>, label: &str) -> usize {
    h.state_mut().set_option("palette", "undo").unwrap();
    h.run_steps(2);
    let n = h.query_all_by_label(label).count();
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    n
}

/// Undo goes where the last action was (D12): after moves on the chess board, Edit ▸ Undo, Ctrl+Z
/// and the board's own button take the moves back and leave the document alone; after a click on
/// the page, Ctrl+Z undoes the document's edit and leaves the game alone. Undo is named after
/// what it would undo, in the palette too.
#[test]
fn undo_follows_the_scope_of_the_last_action() {
    let mut h = harness();
    let pdf = board_after_a_pdf_edit(&mut h);
    // A click in the board panel with nothing played: Undo is off, rather than reaching for the
    // document's history behind the user's back.
    click_square(&mut h, "a4");
    assert!(h.state().chess.in_scope());
    assert!(!h.state_mut().execute("edit.undo"), "nothing to undo in the game");
    assert_eq!(pdf_undo(&h).as_deref(), Some(pdf.as_str()));

    for sq in ["e2", "e4", "e7", "e5", "g1", "f3"] {
        click_square(&mut h, sq);
    }
    assert_eq!(h.state().chess.board().game.movetext(), "1. e4 e5 2. Nf3");
    click_square(&mut h, "a4");
    // The palette and the rail's button name the move (the button with its key too).
    assert_eq!(palette_labels(&mut h, "Undo Play Nf3"), 1, "the palette's Undo");
    assert_eq!(h.query_all_by_label_contains("Undo Play Nf3 (").count(), 1, "the rail's Undo");
    assert!(h.state_mut().execute("edit.undo"));
    h.run_steps(2);
    assert_eq!(h.state().chess.board().game.movetext(), "1. e4 e5");
    shortcut(&mut h, egui::Key::Z, false);
    assert_eq!(h.state().chess.board().game.movetext(), "1. e4");
    assert_eq!(pdf_undo(&h).as_deref(), Some(pdf.as_str()), "the document is untouched");
    shortcut(&mut h, egui::Key::Z, true);
    assert_eq!(h.state().chess.board().game.movetext(), "1. e4 e5");
    h.get_by_label_contains("Undo Play e5 (").click();
    h.run_steps(2);
    assert_eq!(h.state().chess.board().game.movetext(), "1. e4");
    h.get_by_label_contains("Redo Play e5 (").click();
    h.run_steps(2);
    assert_eq!(h.state().chess.board().game.movetext(), "1. e4 e5");

    // A click on the page: Undo goes back to the document.
    let page = on_page(&h, 400.0, 300.0);
    click_at(&mut h, page);
    assert!(!h.state().chess.in_scope());
    assert_eq!(palette_labels(&mut h, &format!("Undo {pdf}")), 1);
    shortcut(&mut h, egui::Key::Z, false);
    assert_eq!(pdf_undo(&h), None, "the rectangle was undone");
    assert_eq!(h.state().chess.board().game.movetext(), "1. e4 e5", "the game is untouched");
    // And a move played on the board brings it back to the game.
    click_square(&mut h, "g1");
    click_square(&mut h, "f3");
    assert!(h.state().chess.in_scope());
    shortcut(&mut h, egui::Key::Z, false);
    assert_eq!(h.state().chess.board().game.movetext(), "1. e4 e5");
}

/// A comment typed in several goes is one undo step, named after what it was.
#[test]
fn a_comment_typed_in_several_goes_is_one_undo_step() {
    let mut h = harness();
    h.state_mut().show_chess_board();
    h.run_steps(3);
    click_square(&mut h, "e2");
    click_square(&mut h, "e4");
    let comment = h.get_all_by_role(egui::accesskit::Role::MultilineTextInput).find(|n| n.value().unwrap_or_default().is_empty()).unwrap();
    comment.focus();
    comment.type_text("good ");
    h.run_steps(2);
    h.get_all_by_role(egui::accesskit::Role::MultilineTextInput).find(|n| n.value().as_deref() == Some("good ")).unwrap().type_text("move");
    h.run_steps(2);
    // Leave the field.
    click_square(&mut h, "a4");
    let node = h.state().chess.board().game.current();
    assert_eq!(h.state().chess.board().game.comment(node), "good move");
    assert_eq!(h.state().chess.board().undo_label().as_deref(), Some("Edit move comment"));
    assert!(h.state_mut().execute("edit.undo"));
    h.run_steps(2);
    assert_eq!(h.state().chess.board().game.comment(node), "");
    assert_eq!(h.state().chess.board().game.movetext(), "1. e4", "one step took the whole comment back, and only it");
    assert_eq!(h.state().chess.board().undo_label().as_deref(), Some("Play e4"));
    assert!(h.state_mut().execute("edit.redo"));
    h.run_steps(2);
    assert_eq!(h.state().chess.board().game.comment(node), "good move");
}

/// The FEN and PGN fields show the game put back by Undo, even when another change follows it
/// before they are drawn again: the game put back gets a newer revision than any seen, so it is
/// never taken for the game the fields last showed.
#[test]
fn the_fields_follow_the_game_put_back() {
    let mut h = harness();
    h.state_mut().show_chess_board();
    h.run_steps(3);
    click_square(&mut h, "e2");
    click_square(&mut h, "e4");
    assert!(h.state().chess.board().pgn_text.contains("1. e4 *"), "{}", h.state().chess.board().pgn_text);
    {
        let app = h.state_mut();
        assert!(app.execute("edit.undo"));
        app.chess.board_mut().play_uci_line(&["d2d4".into()]);
    }
    h.run_steps(2);
    let board = h.state().chess.board();
    assert_eq!(board.game.movetext(), "1. d4");
    assert!(board.pgn_text.contains("1. d4 *"), "the PGN field is stale: {}", board.pgn_text);
    assert_eq!(board.fen_text, "rnbqkbnr/pppppppp/8/8/3P4/8/PPP1PPPP/RNBQKBNR b KQkq d3 0 1");
}

// ─── Chess ▸ Game collections (T4b): a game per diagram, kept across sessions ───────────────────

/// Double-click diagram 1 on page 1 (the scan must have run, with short frames).
fn double_click_diagram(h: &mut Harness<'static, PdfCraftApp>) {
    let at = on_page(h, 140.0, 660.0);
    h.hover_at(at);
    h.run_steps(1);
    for _ in 0..2 {
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
        }
        h.step();
    }
    h.run_steps(3);
}

/// The movetext of every game of the document shown, in list order, and which one is shown.
fn games(app: &PdfCraftApp) -> (Vec<String>, usize) {
    let c = app.chess.games.collection();
    (c.iter().map(|e| e.board.game.movetext()).collect(), c.active_index())
}

/// Opening a diagram on the board again, after moves were played on it, is a new game: the game
/// played stays as it was, and the list switches between them. The same diagram, opened again
/// while its game is untouched, shows that game rather than a copy.
#[test]
fn each_diagram_opened_is_a_new_game_and_the_first_is_kept() {
    let mut h = harness_stepping(0.05);
    assert!(h.state_mut().execute("chess.scan"));
    h.run_steps(3);
    double_click_diagram(&mut h);
    assert_eq!(games(h.state()), (vec![String::new()], 0), "the blank game gave way to the diagram");
    click_square(&mut h, "e2");
    click_square(&mut h, "e4");
    double_click_diagram(&mut h);
    assert_eq!(games(h.state()), (vec!["1. e4".to_owned(), String::new()], 1), "a new game; the first is intact");
    assert_eq!(h.state().chess.board().game.fen(), START_FEN);
    h.get_by_label("Games in this document (2)");
    // Back to the first game from the list: its moves are there, and a click there is not an undo step.
    h.get_all_by_label("Diagram 1, p. 1").next().expect("listed").click();
    h.run_steps(2);
    assert_eq!(games(h.state()).1, 0);
    assert_eq!(h.state().chess.board().game.movetext(), "1. e4");
    assert_eq!(h.state().chess.board().undo_label().as_deref(), Some("Play e4"), "switching games is not a step");
    // The untouched second game is shown again, not copied.
    double_click_diagram(&mut h);
    assert_eq!(games(h.state()), (vec!["1. e4".to_owned(), String::new()], 1));
    // New game adds a blank one.
    h.get_by_label("New game").click();
    h.run_steps(2);
    assert_eq!(games(h.state()).0.len(), 3);
    h.get_by_label("Game 3");
}

/// A temporary recovery folder for one test.
fn store(tag: &str) -> pdfcraft_ui_egui::RecoveryStore {
    let dir = std::env::temp_dir().join(format!("pdfcraft-chess-games-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    pdfcraft_ui_egui::RecoveryStore::new(dir)
}

/// The app with autosave into `store`, on `bytes` opened as `name` from `path`.
fn session(store: &pdfcraft_ui_egui::RecoveryStore, name: &str, path: Option<&str>, bytes: Vec<u8>) -> Harness<'static, PdfCraftApp> {
    let (store, name, path) = (store.clone(), name.to_owned(), path.map(str::to_owned));
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 1000.0)).with_step_dt(0.05).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.run_inline = true;
        app.enable_recovery(store);
        app.open_bytes(&name, path, bytes).unwrap();
        app
    });
    h.run_steps(4);
    h
}

/// The `.games` files (and the files set aside) in the store's chess folder, by name.
fn chess_files(store: &pdfcraft_ui_egui::RecoveryStore) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(store.dir().join("chess"))
        .map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.contains(".games")).collect())
        .unwrap_or_default();
    names.sort();
    names
}

/// Everything about a move the autosave must bring back, by its path: its comments, glyphs and
/// where it was read on the page.
type Move = (Vec<u16>, String, String, Vec<u16>, Option<(u32, [f32; 4])>);

/// Every game of the document shown: its PGN, the move shown, its moves and the board's side.
fn game_state(app: &PdfCraftApp) -> Vec<(String, Vec<u16>, Vec<Move>, bool)> {
    use pdfcraft_engine::chess::pdfcraft_chess::game::Token;
    let c = app.chess.games.collection();
    c.iter()
        .map(|e| {
            let g = &e.board.game;
            let nodes = std::iter::once(0).chain(g.tokens().into_iter().filter_map(|t| match t {
                Token::Move { node, .. } => Some(node),
                _ => None,
            }));
            let moves = nodes
                .map(|n| {
                    let path = g.path_of(n).unwrap().indices().to_vec();
                    (path, g.comment(n).to_owned(), g.comment_before(n).to_owned(), g.nags(n).to_vec(), g.source(n).map(|s| (s.page, s.rect_user)))
                })
                .collect();
            (g.pgn(), e.current_path().indices().to_vec(), moves, e.board.flipped)
        })
        .collect()
}

/// The games, with what PGN can't hold, come back as they were when the document is opened again
/// in a new session: the move shown, where each move was read, and a comment before a first
/// reply (which PGN writes into the comment of the move before it).
#[test]
fn the_games_come_back_exactly_in_the_next_session() {
    use pdfcraft_engine::chess::pdfcraft_chess::game::NodeSource;
    let s = store("round-trip");
    let path = s.dir().join("book.pdf").to_string_lossy().into_owned();
    let expected = {
        let mut h = session(&s, "book.pdf", Some(&path), boards());
        assert!(h.state_mut().execute("chess.scan"));
        h.run_steps(3);
        h.state_mut().open_chess_board(0, 0);
        h.run_steps(2);
        {
            let b = h.state_mut().chess.board_mut();
            b.play_uci_line(&["e2e4".into(), "e7e5".into(), "g1f3".into()]);
            let g = &mut b.game;
            let nf3 = g.current();
            g.go_to(2);
            let e5 = g.current();
            g.set_comment(e5, "after e5");
            g.set_comment_before(nf3, "before Nf3");
            g.toggle_nag(nf3, 1);
            assert!(g.set_source(nf3, Some(NodeSource { page: 0, rect_user: [60.0, 500.0, 90.0, 512.0] })));
            g.set_tag("White", "Werle").unwrap();
            g.set_tag("Black", "Jianu").unwrap();
            b.flipped = true;
            // The move shown: a variation, 1... c5.
            b.game.go_to(1);
        }
        h.state_mut().chess.board_mut().play_uci_line(&["c7c5".into()]);
        // A second game, shown, with a move.
        assert!(h.state_mut().execute("chess.game.new"));
        h.state_mut().chess.board_mut().play_uci_line(&["d2d4".into()]);
        // Written two seconds after the last change (frames are 0.05 s apart).
        h.run_steps(10);
        assert!(chess_files(&s).is_empty(), "not before the changes have rested");
        h.run_steps(40);
        assert_eq!(chess_files(&s).len(), 1, "written once they have");
        let state = game_state(h.state());
        assert_eq!(state.len(), 2);
        assert_eq!(state[0].1, vec![0, 1], "the move shown in the first game: the variation 1... c5");
        state
    };
    let mut h = session(&s, "book.pdf", Some(&path), boards());
    assert_eq!(game_state(h.state()), expected);
    assert_eq!(games(h.state()).1, 1, "the game shown too");
    assert_eq!(h.state().toast.as_ref().map(|t| t.0.as_str()), Some("Chess games restored: 2"));
    // The diagram the first game came from is in this document.
    h.state_mut().show_chess_board();
    h.run_steps(3);
    // The list of games is under the board, at the window's bottom: scrolled into view first.
    let list = h.get_by_label("Werle–Jianu").rect().center();
    h.hover_at(egui::pos2(list.x, 900.0));
    h.run_steps(1);
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -200.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(3);
    h.get_by_label("Werle–Jianu").click();
    h.run_steps(2);
    let source = h.state().chess.board().source.unwrap();
    assert_eq!((source.doc, source.page, source.index), (h.state().views[0].id, 0, 0));
}

/// A file without `/ID` gets one when it is saved, and a new size: the games take the new keys at
/// once, so a copy of the saved file (another name and folder, found by its `/ID` alone) brings
/// them back.
#[test]
fn saving_a_pdf_without_an_id_rekeys_its_games() {
    let s = store("rekey");
    std::fs::create_dir_all(s.dir()).unwrap();
    let path = s.dir().join("book.pdf");
    std::fs::write(&path, boards()).unwrap();
    {
        let mut h = session(&s, "book.pdf", Some(&path.to_string_lossy()), boards());
        h.state_mut().chess.board_mut().play_uci_line(&["e2e4".into()]);
        h.state_mut().flush_chess_games();
        assert_eq!(chess_files(&s).len(), 1);
        let app = h.state_mut();
        assert!(app.apply_edit(Edit::RotatePages { pages: vec![0], degrees: 90 }));
        app.save_override = Some(path.to_string_lossy().into_owned());
        assert!(app.save_active(pdfcraft_ui_egui::SaveTarget::InPlace));
    }
    let copy = s.dir().join("copy of the book.pdf");
    std::fs::copy(&path, &copy).unwrap();
    let h = session(&s, "copy of the book.pdf", Some(&copy.to_string_lossy()), std::fs::read(&copy).unwrap());
    assert_eq!(games(h.state()).0, ["1. e4"], "found by the /ID the save created");
    assert_eq!(h.state().toast.as_ref().map(|t| t.0.as_str()), Some("Chess game restored: Game 1, moves: 1"));
}

/// Files that can't be read (random bytes, a file over the cap, an unknown version) are set
/// aside as `.bad`, never deleted, and the user is told; the document opens with a blank game.
#[test]
fn unreadable_game_files_are_set_aside_and_said() {
    let s = store("corrupt");
    let dir = s.dir().join("chess");
    std::fs::create_dir_all(&dir).unwrap();
    let mut noise = vec![0u8; 4096];
    let mut x: u32 = 12345;
    for b in &mut noise {
        x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
        *b = (x >> 16) as u8;
    }
    std::fs::write(dir.join("aa-1.games"), &noise).unwrap();
    // 2 MiB over the cap of a document's file (32 MiB of games and the header).
    let big = std::fs::File::create(dir.join("bb-2.games")).unwrap();
    big.set_len((34 << 20) + (64 << 10)).unwrap();
    drop(big);
    let newer = "{\"format\":\"pdfcraft-chess-games\",\"version\":99,\"keys\":{\"name\":\"boards.pdf\"}}\n{}\n";
    std::fs::write(dir.join("cc-3.games"), newer).unwrap();
    let h = session(&s, "boards.pdf", None, boards());
    assert_eq!(chess_files(&s), ["aa-1.games.bad", "bb-2.games.bad", "cc-3.games.bad"]);
    let toast = h.state().toast.as_ref().map(|t| t.0.clone()).unwrap_or_default();
    assert!(toast.contains("couldn't be read. It was kept as aa-1.games.bad"), "{toast}");
    assert!(toast.contains("cc-3.games.bad"), "{toast}");
    assert_eq!(games(h.state()).0, [""], "a blank game");
    // What was set aside stays aside: a second session neither reads nor renames it again.
    drop(h);
    let h = session(&s, "boards.pdf", None, boards());
    assert_eq!(chess_files(&s), ["aa-1.games.bad", "bb-2.games.bad", "cc-3.games.bad"]);
    assert!(h.state().toast.is_none());
}

/// Two windows on the same document: the second one finds the games the first autosaves, but
/// doesn't write over them while the first runs (and says so); once the first is gone, it does.
#[test]
fn a_second_instance_does_not_overwrite_the_games_of_the_first() {
    let s = store("instances");
    let mut first = session(&s, "boards.pdf", None, boards());
    first.state_mut().chess.board_mut().play_uci_line(&["e2e4".into()]);
    first.state_mut().flush_chess_games();
    let files = chess_files(&s);
    assert_eq!(files.len(), 1);
    let file = s.dir().join("chess").join(&files[0]);
    let written = std::fs::read(&file).unwrap();
    // Another instance: the same folder, its own lock.
    let other = pdfcraft_ui_egui::RecoveryStore::new(s.dir());
    let mut second = session(&other, "boards.pdf", None, boards());
    assert_eq!(games(second.state()).0, ["1. e4"]);
    second.state_mut().chess.board_mut().play_uci_line(&["e7e5".into()]);
    second.state_mut().flush_chess_games();
    assert_eq!(std::fs::read(&file).unwrap(), written, "the first window's file is untouched");
    let toast = second.state().toast.as_ref().map(|t| t.0.clone()).unwrap_or_default();
    assert!(toast.contains("autosaved by another PdfCraft window"), "{toast}");
    // The first window quits: its lock goes with the last handle on its folder.
    drop(first);
    drop(s);
    second.state_mut().flush_chess_games();
    assert!(String::from_utf8_lossy(&std::fs::read(&file).unwrap()).contains("1. e4 e5"), "written once the first window is gone");
}

/// A document that forbids copying: its games are written only once the user allows it. Until
/// then nothing is written; "Don't save" writes nothing for the session.
#[test]
fn a_protected_document_asks_before_its_games_are_written() {
    let mut doc = pdfcraft_cos::Document::open(std::sync::Arc::new(boards())).unwrap();
    doc.set_encryption(&pdfcraft_cos::NewEncryption {
        algorithm: pdfcraft_cos::Algorithm::Aes256,
        user_password: "",
        owner_password: "owner",
        permissions: 0b0100,
        encrypt_metadata: true,
        seed: [3; 32],
    })
    .unwrap();
    let bytes = pdfcraft_cos::write_full(&doc, &Default::default()).unwrap();
    for allow in [false, true] {
        let s = store(if allow { "protected-yes" } else { "protected-no" });
        let mut h = session(&s, "restricted.pdf", None, bytes.clone());
        // Tall enough for the Games section under the board, its Analysis section and the moves.
        h.set_size(egui::vec2(1400.0, 1200.0));
        h.state_mut().show_chess_board();
        h.run_steps(3);
        click_square(&mut h, "e2");
        click_square(&mut h, "e4");
        h.state_mut().flush_chess_games();
        assert!(chess_files(&s).is_empty(), "nothing before the user answers");
        let toast = h.state().toast.as_ref().map(|t| t.0.clone()).unwrap_or_default();
        assert!(toast.contains("autosaved only if you allow it"), "{toast}");
        h.get_by_label("Save chess games for this protected document?");
        h.get_by_label(if allow { "Save them" } else { "Don't save" }).click();
        h.run_steps(2);
        assert!(h.query_by_label("Save chess games for this protected document?").is_none(), "asked once");
        click_square(&mut h, "e7");
        click_square(&mut h, "e5");
        h.state_mut().flush_chess_games();
        assert_eq!(chess_files(&s).len(), usize::from(allow));
    }
}

/// Open PGN… adds every game of a file (games split at their tag lines); a game that can't be
/// read is left out and said.
#[test]
fn a_pgn_file_with_several_games_is_imported() {
    let mut h = harness();
    // Tall enough for the Games section under the board, its Analysis section and the moves.
    h.set_size(egui::vec2(1400.0, 1200.0));
    h.state_mut().show_chess_board();
    h.run_steps(3);
    let pgn = "[Event \"A\"]\n[White \"Werle\"]\n[Black \"Jianu\"]\n\n1. e4 e5 {open} 2. Nf3 *\n\n\
               [Event \"B\"]\n[FEN \"not a position\"]\n\n1. e4 *\n\n\
               [Event \"C\"]\n\n1. d4 d5 (1... Nf6 2. c4) 2. c4 *\n";
    assert_eq!(h.state_mut().import_chess_pgn("db.pgn", pgn.as_bytes()), 2);
    h.run_steps(2);
    assert_eq!(games(h.state()), (vec!["1. e4 e5 {open} 2. Nf3".to_owned(), "1. d4 d5 (1... Nf6 2. c4) 2. c4".to_owned()], 0));
    assert_eq!(h.state().toast.as_ref().map(|t| t.0.as_str()), Some("Games read from db.pgn: 2 · Not read: 1"));
    h.get_by_label("Werle–Jianu");
    // Named by its event (T14), without players.
    h.get_by_label("C").click();
    h.run_steps(2);
    assert_eq!(h.state().chess.board().game.movetext(), "1. d4 d5 (1... Nf6 2. c4) 2. c4");
}

// ---- T5a: the PGN tool rail and the chess key scope ----

/// A game with variations: 1.e4 e5 (1...c5 2.Nf3 (2.c3) d6) 2.Nf3.
const FORKS: &str = "1. e4 e5 (1... c5 2. Nf3 (2. c3) d6) 2. Nf3 *";

/// Its move text, as the game writes it.
const FORKS_TEXT: &str = "1. e4 e5 (1... c5 2. Nf3 (2. c3) 2... d6) 2. Nf3";

/// Put the game of `pgn` on the board, at the move reached by `path` (reply indices).
fn put_game(h: &mut Harness<'static, PdfCraftApp>, pgn: &str, path: &[u16]) {
    use pdfcraft_engine::chess::pdfcraft_chess::game::{Game, NodePath};
    let (mut game, stopped) = Game::from_pgn(pgn).unwrap();
    assert_eq!(stopped, None);
    game.go_to_node(game.node_at(&NodePath::new(path.to_vec())).unwrap());
    h.state_mut().chess.board_mut().game = game;
    h.run_steps(2);
}

fn movetext(h: &Harness<'static, PdfCraftApp>) -> String {
    h.state().chess.board().game.movetext()
}

/// The move shown, as its SAN.
fn shown(h: &Harness<'static, PdfCraftApp>) -> String {
    let game = &h.state().chess.board().game;
    game.san(game.current()).to_owned()
}

/// A key as the rail writes it on this platform: "Ctrl+Shift+↑" (⇧⌘↑ on macOS).
fn key_text(command: bool, shift: bool, key: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("{}{}{key}", if shift { "⇧" } else { "" }, if command { "⌘" } else { "" })
    } else {
        format!("{}{}{key}", if command { "Ctrl+" } else { "" }, if shift { "Shift+" } else { "" })
    }
}

/// Press `key` with `modifiers` as the desktop app gets it: the frame's raw input goes through
/// `raw_input_hook` first, where the chess scope takes its keys (kittest itself skips the hook).
fn host_key(h: &mut Harness<'static, PdfCraftApp>, modifiers: egui::Modifiers, key: egui::Key) {
    use eframe::App as _;
    let ctx = h.ctx.clone();
    for pressed in [true, false] {
        let mut raw = std::mem::take(h.input_mut());
        raw.events.push(egui::Event::ModifiersChanged(if pressed { modifiers } else { egui::Modifiers::NONE }));
        raw.events.push(egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers });
        h.state_mut().raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }
    h.run_steps(2);
}

/// A click on the Board panel's title: the chess scope goes on, and nothing takes the focus.
fn click_board_title(h: &mut Harness<'static, PdfCraftApp>) {
    let title = h.get_all_by_label("Chess board").find(|n| n.accesskit_node().role() == egui::accesskit::Role::Label).expect("the panel's title");
    let at = title.rect().center();
    click_at(h, at);
    assert!(h.state().chess.in_scope());
    assert!(!h.ctx.egui_wants_keyboard_input(), "nothing has the keyboard");
}

/// The rail's buttons are named by their tool and key, the mode armed says so, and each acts:
/// Promote one level promotes the variation nearest the move shown, Undo and Redo take it back
/// and forth, and Esc puts down the piece picked up.
#[test]
fn the_rail_names_its_tools_and_they_act() {
    let mut h = harness();
    h.state_mut().show_chess_board();
    h.run_steps(3);
    put_game(&mut h, FORKS, &[0, 1, 0, 0]);
    assert_eq!(shown(&h), "d6");
    let navigate_name = format!("Navigate ({})", key_text(false, false, "Esc"));
    let navigate = h.get_by_label(&navigate_name);
    assert_eq!(navigate.accesskit_node().role(), egui::accesskit::Role::Button);
    assert_eq!(navigate.accesskit_node().toggled(), Some(egui::accesskit::Toggled::True), "Navigate is the mode armed");
    navigate.click();
    h.run_steps(2);
    assert_eq!(format!("{:?}", h.state().chess.tools.tool), "Navigate");
    // The Structure group shows its first tool; Undo and Redo name what they would do.
    h.get_by_label(&format!("Promote one level ({})", key_text(true, false, "↑"))).click();
    h.run_steps(2);
    assert_eq!(movetext(&h), "1. e4 c5 (1... e5 2. Nf3) 2. Nf3 (2. c3) 2... d6");
    assert_eq!(h.state().chess.board().message, None);
    h.get_by_label_contains("Undo Promote a variation (").click();
    h.run_steps(2);
    assert_eq!(movetext(&h), FORKS_TEXT);
    h.get_by_label_contains("Redo Promote a variation (").click();
    h.run_steps(2);
    assert_eq!(movetext(&h), "1. e4 c5 (1... e5 2. Nf3) 2. Nf3 (2. c3) 2... d6");
    // Nothing more to redo: Redo is off.
    assert!(h.get_by_label_contains("Redo (").accesskit_node().is_disabled());
    // Esc puts the piece picked up down.
    click_square(&mut h, "b1");
    assert!(h.state().chess.board().selected.is_some());
    host_key(&mut h, egui::Modifiers::NONE, egui::Key::Escape);
    assert_eq!(h.state().chess.board().selected, None);
}

/// With the chess scope on, the keys of the chess keymap edit and walk the game: each structure
/// change is one undo step, a refusal says why in the panel, and the arrows, Home and End move
/// along the line of the move shown.
#[test]
fn the_chess_keys_edit_and_walk_the_game() {
    use egui::{Key, Modifiers};
    let (cmd, shift) = (Modifiers::COMMAND, Modifiers::SHIFT);
    let mut h = harness();
    h.state_mut().show_chess_board();
    h.run_steps(3);
    put_game(&mut h, FORKS, &[0, 1, 0, 0]);
    click_board_title(&mut h);
    let undo = |h: &mut Harness<'static, PdfCraftApp>| {
        assert!(h.state_mut().execute("edit.undo"));
        h.run_steps(2);
        assert_eq!(movetext(h), FORKS_TEXT);
    };
    // Mod+↓ demotes the nearest line that continues its move: 2.Nf3, so 2.c3 continues 1...c5.
    host_key(&mut h, cmd, Key::ArrowDown);
    assert_eq!(movetext(&h), "1. e4 e5 (1... c5 2. c3 (2. Nf3 d6)) 2. Nf3");
    assert_eq!(h.state().chess.board().undo_label().as_deref(), Some("Demote a line"));
    undo(&mut h);
    // Mod+↑ promotes the nearest variation: 1...c5.
    host_key(&mut h, cmd, Key::ArrowUp);
    assert_eq!(movetext(&h), "1. e4 c5 (1... e5 2. Nf3) 2. Nf3 (2. c3) 2... d6");
    undo(&mut h);
    // Mod+Shift+↑ makes the line of the move shown the main line.
    put_game(&mut h, FORKS, &[0, 1, 1]);
    host_key(&mut h, cmd | shift, Key::ArrowUp);
    assert_eq!(movetext(&h), "1. e4 c5 (1... e5 2. Nf3) 2. c3 (2. Nf3 d6)");
    assert_eq!(h.state().chess.board().undo_label().as_deref(), Some("Make main line"));
    put_game(&mut h, FORKS, &[0, 1, 0, 0]);
    // Delete deletes the nearest variation, and the move shown goes to its fork.
    host_key(&mut h, Modifiers::NONE, Key::Delete);
    assert_eq!(movetext(&h), "1. e4 e5 2. Nf3");
    assert_eq!(shown(&h), "e4");
    assert_eq!(h.state().chess.board().undo_label().as_deref(), Some("Delete a variation"));
    undo(&mut h);
    // Backspace goes back a move and deletes nothing (plan A8 §7, V2a).
    let before = movetext(&h);
    host_key(&mut h, Modifiers::NONE, Key::Backspace);
    assert_eq!(movetext(&h), before);
    assert_eq!(shown(&h), "Nf3", "back from 2... d6, in the variation");
    // Home, End and the arrows walk the line of the move shown; Down and Up switch the reply at
    // a fork.
    put_game(&mut h, FORKS, &[0, 1, 0, 0]);
    host_key(&mut h, Modifiers::NONE, Key::Home);
    assert_eq!(h.state().chess.board().game.current(), 0);
    host_key(&mut h, Modifiers::NONE, Key::End);
    assert_eq!(shown(&h), "Nf3", "Home went to the start: the line shown is the main line now");
    put_game(&mut h, FORKS, &[0, 1]);
    host_key(&mut h, Modifiers::NONE, Key::End);
    assert_eq!(shown(&h), "d6", "the end of the line shown, not of the main line");
    host_key(&mut h, Modifiers::NONE, Key::ArrowLeft);
    assert_eq!(shown(&h), "Nf3");
    host_key(&mut h, Modifiers::NONE, Key::ArrowDown);
    assert_eq!(shown(&h), "c3");
    host_key(&mut h, Modifiers::NONE, Key::ArrowUp);
    assert_eq!(shown(&h), "Nf3");
    host_key(&mut h, Modifiers::NONE, Key::ArrowRight);
    assert_eq!(shown(&h), "d6");
    // On the main line before any fork there is nothing to demote: the panel says so.
    put_game(&mut h, FORKS, &[0]);
    assert_eq!(shown(&h), "e4");
    host_key(&mut h, cmd, Key::ArrowDown);
    assert_eq!(movetext(&h), FORKS_TEXT, "refused: nothing changed");
    h.get_by_label("Not demoted: there is no variation to take its place.");
    // Shift+Delete cuts the moves after the move shown.
    host_key(&mut h, shift, Key::Delete);
    assert_eq!(movetext(&h), "1. e4");
    assert_eq!(h.state().chess.board().message, None, "done: the refusal's notice is gone");
    assert_eq!(h.state().chess.board().undo_label().as_deref(), Some("Delete moves"));
    undo(&mut h);
}

/// A move's context menu has the structure tools, acting on that move's nearest variation; one
/// that has nothing to act on there is off.
#[test]
fn a_moves_menu_deletes_its_variation() {
    let mut h = harness();
    h.state_mut().show_chess_board();
    h.run_steps(3);
    put_game(&mut h, FORKS, &[0, 0, 0]);
    h.get_by_label("d6").click_secondary();
    h.run_steps(2);
    let item = h.get_by_label("Delete variation");
    assert!(!item.accesskit_node().is_disabled());
    item.click();
    h.run_steps(2);
    assert_eq!(movetext(&h), "1. e4 e5 2. Nf3");
    // On the main line there is no variation to delete; after the last move nothing to cut.
    h.get_by_label("knight f3").click_secondary();
    h.run_steps(2);
    assert!(h.get_by_label("Delete variation").accesskit_node().is_disabled());
    assert!(h.get_by_label("Cut after this move").accesskit_node().is_disabled());
}

/// A three-page document, for the page grid.
fn three_pages() -> Vec<u8> {
    let n = 3;
    let mut objs: Vec<String> = vec!["<< /Type /Catalog /Pages 2 0 R >>".into()];
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 4 + 2 * i)).collect();
    objs.push(format!("<< /Type /Pages /Kids [{}] /Count {n} /MediaBox [0 0 600 800] >>", kids.join(" ")));
    objs.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into());
    for i in 0..n {
        objs.push(format!("<< /Type /Page /Parent 2 0 R /Contents {} 0 R /Resources << /Font << /F1 3 0 R >> >> >>", 5 + 2 * i));
        let body = format!("BT /F1 24 Tf 60 400 Td (Page {}) Tj ET", i + 1);
        objs.push(format!("<< /Length {} >>\nstream\n{body}\nendstream", body.len()));
    }
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

/// Moves deleted go into the document's trash, which outlives Undo: Deleted moves… under the list
/// of games opens the game as it was as a new game (plan A8 §7, V5).
#[test]
fn deleted_moves_are_kept_and_open_again_as_a_new_game() {
    use egui::{Key, Modifiers};
    let mut h = harness();
    // Tall enough for the list of games under the board to be on screen.
    h.set_size(egui::vec2(1400.0, 1600.0));
    h.state_mut().show_chess_board();
    h.run_steps(3);
    put_game(&mut h, "1. e4 e5 (1... c5 2. Nf3) 2. Nf3 *", &[0, 1]);
    click_board_title(&mut h);
    assert!(h.query_by_label_contains("Deleted moves").is_none(), "nothing kept yet");
    host_key(&mut h, Modifiers::NONE, Key::Delete);
    h.run_steps(2);
    assert_eq!(movetext(&h), "1. e4 e5 2. Nf3");
    assert_eq!(h.state().chess.games.collection().trash().len(), 1);
    h.get_by_label("Deleted moves (1)…").click();
    h.run_steps(2);
    h.get_by_label_contains("5 moves · 2 moves deleted");
    h.get_by_label("Open as a new game").click();
    h.run_steps(2);
    let games = h.state().chess.games.collection();
    assert_eq!((games.len(), games.active_index()), (2, 1));
    assert_eq!(movetext(&h), "1. e4 e5 (1... c5 2. Nf3) 2. Nf3", "the game as it was");
    assert_eq!(games.get(0).unwrap().board.game.movetext(), "1. e4 e5 2. Nf3");
    assert!(games.trash().is_empty(), "taken out of the trash");
}

/// A position set up over a game with moves is a new game after it: the game keeps its moves.
#[test]
fn a_position_set_up_over_a_game_with_moves_is_a_new_game() {
    let mut h = harness();
    h.state_mut().show_chess_board();
    h.run_steps(3);
    click_square(&mut h, "d2");
    click_square(&mut h, "d4");
    h.get_by_label("Set up a position").click();
    h.run_steps(2);
    h.get_by_label("Empty board").click();
    h.run_steps(2);
    for (piece, square) in [("White king", "e1"), ("Black king", "e8")] {
        h.get_by_label(piece).click();
        h.run_steps(1);
        click_square(&mut h, square);
    }
    h.get_by_label("New game from this position").click();
    h.run_steps(2);
    let games = h.state().chess.games.collection();
    assert_eq!((games.len(), games.active_index()), (2, 1));
    assert_eq!(h.state().chess.board().game.fen(), "4k3/8/8/8/8/8/8/4K3 b - - 0 1");
    assert_eq!(games.get(0).unwrap().board.game.movetext(), "1. d4", "the game keeps its moves");
    assert_eq!(h.state().chess.tools.deleted.text(), None, "nothing was deleted");
    assert_eq!(format!("{:?}", h.state().chess.tools.tool), "Navigate");
}

/// Plan A8 §7, V2b: nothing beside the notation deletes moves (W1); Enter in the FEN field with the
/// FEN as shown does nothing, and a FEN typed over a game with moves asks first, New game being
/// the choice Enter takes, so the game keeps its moves (W3).
#[test]
fn the_fen_field_never_takes_the_games_moves() {
    use egui::{Key, Modifiers};
    const KINGS: &str = "4k3/8/8/8/8/8/8/4K3 w - - 0 1";
    let mut h = tall_window(1600.0);
    h.state_mut().show_chess_board();
    h.run_steps(3);
    put_game(&mut h, FORKS, &[0]);
    assert!(h.query_by_label("Delete the moves after this one").is_none(), "no delete button beside the notation");
    h.get_by_label("FEN").click();
    h.run_steps(3);
    let fen = h.state().chess.board().fen_text.clone();
    // The field, not the position's FEN shown elsewhere.
    let field = || egui_kittest::kittest::By::new().role(egui::accesskit::Role::TextInput).value(&fen);
    h.get(field()).focus();
    h.run_steps(1);
    h.key_press(Key::Enter);
    h.run_steps(2);
    assert_eq!(movetext(&h), FORKS_TEXT, "the FEN as shown reloads nothing");
    assert!(h.query_by_label("New game with this position").is_none(), "nor asks");
    h.get(field()).focus();
    h.run_steps(1);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.get(field()).type_text(KINGS);
    h.key_press(Key::Enter);
    h.run_steps(2);
    assert_eq!(movetext(&h), FORKS_TEXT, "asked first");
    h.get_by_label("Replace this game");
    if let Ok(dir) = std::env::var("PDFCRAFT_CHESS_SHOTS") {
        h.render().unwrap().save(std::path::Path::new(&dir).join("board-fen-choice.png")).unwrap();
    }
    // New game has the keyboard: Enter takes it.
    h.key_press(Key::Enter);
    h.run_steps(3);
    let games = h.state().chess.games.collection();
    assert_eq!((games.len(), games.active_index()), (2, 1));
    assert_eq!(h.state().chess.board().game.fen(), KINGS);
    assert_eq!(games.get(0).unwrap().board.game.movetext(), FORKS_TEXT, "the game keeps its moves");
    assert!(h.query_by_label("New game with this position").is_none(), "answered");
}

/// Plan A8 §7, V2b (W4): the standard start position over a game with moves is a new game; on a
/// game without moves it starts that game again.
#[test]
fn the_standard_start_over_moves_is_a_new_game() {
    let mut h = tall_window(1600.0);
    h.state_mut().show_chess_board();
    h.run_steps(3);
    put_game(&mut h, FORKS, &[0]);
    h.get_by_label("FEN").click();
    h.run_steps(3);
    h.get_by_label("Standard start position").click();
    h.run_steps(3);
    let games = h.state().chess.games.collection();
    assert_eq!((games.len(), games.active_index()), (2, 1));
    assert_eq!(games.get(0).unwrap().board.game.movetext(), FORKS_TEXT, "the game keeps its moves");
    assert_eq!(movetext(&h), "", "a new game from the start");
    h.get_by_label("Standard start position").click();
    h.run_steps(3);
    assert_eq!(h.state().chess.games.collection().len(), 2, "no moves: no other game");
}

/// Plan A8 §7, V2b (W6): text typed in the PGN field outlives a step on the board and says it is
/// edited; Load reads it keeping the move shown.
#[test]
fn typed_pgn_outlives_a_step_and_loads_in_place() {
    use egui::{Key, Modifiers};
    const COMMENTED: &str = "1. e4 {King's pawn} e5 2. Nf3 *";
    let mut h = tall_window(1600.0);
    h.state_mut().show_chess_board();
    h.run_steps(3);
    put_game(&mut h, "1. e4 e5 2. Nf3 *", &[0, 0]);
    h.get_by_label("PGN").click();
    h.run_steps(3);
    assert!(h.query_by_label("PGN edited").is_none());
    let pgn = h.state().chess.board().pgn_text.clone();
    h.get_by_value(&pgn).click();
    h.run_steps(1);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.get_by_value(&pgn).type_text(COMMENTED);
    h.run_steps(2);
    h.get_by_label("PGN edited");
    // A step back on the board (1. e4 shown) leaves the field as typed.
    h.state_mut().chess.board_mut().game.go_to(1);
    h.run_steps(2);
    assert_eq!(h.state().chess.board().pgn_text, COMMENTED);
    h.get_by_label("Load").click();
    h.run_steps(2);
    assert_eq!(movetext(&h), "1. e4 {King's pawn} 1... e5 2. Nf3");
    assert_eq!(shown(&h), "e4", "the move shown is kept");
    assert!(h.query_by_label("PGN edited").is_none(), "loaded");
}

/// Delete and Backspace with the chess scope on act on the game only: not on the pages selected
/// in the page grid, nor on the annotation selected on the page (§4.2). Delete takes a variation
/// and says so with Undo; Backspace only goes back a move (plan A8 §7, V2a). The keys leave the input
/// in `raw_input_hook`, before the page grid and the annotation layer can see them.
#[test]
fn delete_and_backspace_in_the_chess_scope_touch_only_the_game() {
    use egui::{Key, Modifiers};
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 1000.0)).build_eframe(|_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("pages.pdf", None, three_pages()).unwrap();
        app
    });
    h.run_steps(4);
    let shape = Shape::Rectangle { rect: [100.0, 100.0, 300.0, 200.0] };
    let style = Style::default_for(&shape);
    assert!(h.state_mut().apply_edit(Edit::AddAnnotation(NewAnnotation { page: 0, style, shape, contents: String::new(), author: String::new() })));
    let pages = |h: &Harness<'static, PdfCraftApp>| h.state().session.get(h.state().views[0].id).unwrap().info.pages.len();
    let annotations = |h: &Harness<'static, PdfCraftApp>| h.state().session.get(h.state().views[0].id).unwrap().info.annotations.len();
    h.state_mut().show_chess_board();
    h.run_steps(3);
    put_game(&mut h, "1. e4 e5 (1... c5) (1... e6) (1... d5) 2. Nf3 *", &[0, 3]);

    // The page grid, page 2 selected.
    h.state_mut().set_option("organize", "on").unwrap();
    h.run_steps(3);
    h.state_mut().views[0].select_pages(&[1]);
    h.run_steps(2);
    click_board_title(&mut h);
    host_key(&mut h, Modifiers::NONE, Key::Delete);
    assert_eq!(pages(&h), 3, "Delete removed no page");
    assert_eq!(movetext(&h), "1. e4 e5 (1... c5) (1... e6) 2. Nf3", "the game's variation went");
    // The deletion is told, with Undo.
    assert_eq!(h.state().chess.tools.deleted.text(), Some("1 move deleted (1 variation)"));
    put_game(&mut h, "1. e4 e5 (1... c5) (1... e6) 2. Nf3 *", &[0, 2]);
    // Backspace goes back a move and deletes nothing (A8-W2).
    host_key(&mut h, Modifiers::NONE, Key::Backspace);
    assert_eq!(pages(&h), 3, "Backspace removed no page");
    assert_eq!(movetext(&h), "1. e4 e5 (1... c5) (1... e6) 2. Nf3");
    assert_eq!(h.state().chess.board().game.ply(), 1, "back to 1. e4");
    assert_eq!(h.state().views[0].target_pages(), [1], "the page selection is untouched");

    // The page, with the rectangle selected.
    h.state_mut().set_option("organize", "off").unwrap();
    h.run_steps(3);
    h.state_mut().views[0].comments.selected = Some((0, 0));
    h.run_steps(2);
    click_board_title(&mut h);
    assert_eq!(h.state().views[0].comments.selected, Some((0, 0)), "a click in the chess panel leaves the selection");
    put_game(&mut h, "1. e4 e5 (1... c5) (1... e6) 2. Nf3 *", &[0, 2]);
    host_key(&mut h, Modifiers::NONE, Key::Delete);
    put_game(&mut h, "1. e4 e5 (1... c5) 2. Nf3 *", &[0, 1]);
    host_key(&mut h, Modifiers::NONE, Key::Backspace);
    assert_eq!(annotations(&h), 1, "the selected rectangle is still there");
    assert_eq!(movetext(&h), "1. e4 e5 (1... c5) 2. Nf3", "Backspace deleted nothing");
    put_game(&mut h, "1. e4 e5 (1... c5) 2. Nf3 *", &[0, 1]);
    host_key(&mut h, Modifiers::NONE, Key::Delete);
    assert_eq!(movetext(&h), "1. e4 e5 2. Nf3", "Delete took the variation");
    assert_eq!(annotations(&h), 1, "the selected rectangle is still there");
    assert_eq!(pages(&h), 3);
}

/// A key typed in a comment is the comment's: Shift+S is a capital S there, not the Structure
/// group's next tool, and Backspace takes back a letter, not a variation.
#[test]
fn keys_typed_in_a_comment_stay_in_the_comment() {
    use egui::{Key, Modifiers};
    let mut h = harness();
    h.state_mut().show_chess_board();
    h.run_steps(3);
    put_game(&mut h, FORKS, &[0, 1, 0, 0]);
    click_board_title(&mut h);
    let promote = format!("Promote one level ({})", key_text(true, false, "↑"));
    h.get_by_label(&promote);
    let comment = h.get_all_by_role(egui::accesskit::Role::MultilineTextInput).find(|n| n.value().unwrap_or_default().is_empty()).unwrap();
    comment.focus();
    comment.type_text("good");
    h.run_steps(2);
    host_key(&mut h, Modifiers::SHIFT, Key::S);
    host_key(&mut h, Modifiers::NONE, Key::Backspace);
    let node = h.state().chess.board().game.current();
    assert_eq!(h.state().chess.board().game.comment(node), "goo", "Backspace took back a letter");
    assert_eq!(movetext(&h), "1. e4 e5 (1... c5 2. Nf3 (2. c3) 2... d6 {goo}) 2. Nf3", "and no variation");
    h.get_by_label(&promote);
}

/// The Structure group: Shift+S switches the tool it shows; Enter on it (or a right-click, or a
/// long press) opens its tools, with the lines a tool could act on when there are several, the
/// nearest first; Esc closes it.
#[test]
fn the_structure_group_opens_and_switches_from_the_keyboard() {
    use egui::{Key, Modifiers};
    let mut h = harness();
    h.state_mut().show_chess_board();
    h.run_steps(3);
    put_game(&mut h, "1. e4 e5 (1... c5) 2. Nf3 Nc6 (2... d6) 3. Bb5 *", &[0, 0, 0, 0, 0]);
    let start = "1. e4 e5 (1... c5) 2. Nf3 Nc6 (2... d6) 3. Bb5";
    click_board_title(&mut h);
    host_key(&mut h, Modifiers::SHIFT, Key::S);
    h.get_by_label(&format!("Make this the main line ({})", key_text(true, true, "↑")));
    host_key(&mut h, Modifiers::SHIFT, Key::S);
    let demote = format!("Demote ({})", key_text(true, false, "↓"));
    h.get_by_label(&demote).focus();
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(2);
    assert_eq!(movetext(&h), start, "Enter opened the group: nothing was demoted");
    h.get_by_label("Promote one level");
    h.get_by_label("Demote: 2…Nc6");
    // Esc closes it: the scope leaves Esc alone while a popup is open.
    host_key(&mut h, Modifiers::NONE, Key::Escape);
    assert!(h.query_by_label("Demote: 2…Nc6").is_none());
    // Down arrow on the focused group opens it too (the scope takes the arrow for the rail).
    h.get_by_label(&demote).focus();
    h.run_steps(2);
    host_key(&mut h, Modifiers::NONE, Key::ArrowDown);
    h.get_by_label("Demote: 2…Nc6");
    assert_eq!(shown(&h), "Bb5", "Down opened the group rather than switching the reply");
    host_key(&mut h, Modifiers::NONE, Key::Escape);
    // A right-click opens it; the farther line is chosen there.
    h.get_by_label(&demote).click_secondary();
    h.run_steps(2);
    h.get_by_label("Demote: 1…e5").click();
    h.run_steps(2);
    let demoted = "1. e4 c5 (1... e5 2. Nf3 Nc6 (2... d6) 3. Bb5)";
    assert_eq!(movetext(&h), demoted);
    assert!(h.query_by_label("Demote: 1…e5").is_none(), "the choice closed the tools");
    // A long press opens it and does not act when let go.
    let at = h.get_by_label(&demote).rect().center();
    h.hover_at(at);
    h.run_steps(1);
    h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(3);
    h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
    assert_eq!(movetext(&h), demoted, "no demote on letting go");
    h.get_by_label("Promote one level");
}

/// The harness of [`harness`], with `settings` (as `persist` writes them) restored first, as
/// the desktop app does at start.
fn harness_restoring(settings: String) -> Harness<'static, PdfCraftApp> {
    harness_restoring_stepping(settings, 0.25)
}

/// [`harness_restoring`], with frames `step_dt` seconds apart.
fn harness_restoring_stepping(settings: String, step_dt: f32) -> Harness<'static, PdfCraftApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 1000.0)).with_step_dt(step_dt).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.restore(&settings);
        app.set_option("language", "en").unwrap();
        app.open_bytes("boards.pdf", None, boards()).unwrap();
        app.run_inline = true;
        app
    });
    h.run_steps(4);
    h
}

/// Settings with the chess board window as given (`docking_detached`).
fn window_settings(window: serde_json::Value) -> String {
    serde_json::json!({ "docking_detached": window }).to_string()
}

/// T13a: the board moves into its own window and back. kittest draws windows inside the main
/// one, as the web does, so neither the command nor the rail offers it there; the app's own
/// window (not egui's fallback) holds the board, with a Dock button. Out, the dock tab is gone
/// and its place kept; docked again, it is back where it was. Opening a diagram on the board
/// while it is out does not dock it, and closing the window docks it.
#[test]
fn the_board_moves_to_its_own_window_and_docks_back_where_it_was() {
    let board = pdfcraft_ui_egui::docking::Panel::Board;
    let mut h = harness();
    h.state_mut().show_chess_board();
    h.run_steps(3);
    let place = h.state().docking.layout.location(&board).expect("the board is docked");
    assert!(!h.state_mut().execute("chess.window.detach"), "off where windows are drawn inside the main one");
    assert!(h.query_by_label("Chess in its own window").is_none(), "nor on the rail, nor in the Board tab's menu");
    assert!(!h.state().chess_window_detached());

    h.state_mut().set_chess_window_detached(true);
    h.run_steps(3);
    assert!(h.state().chess_window_detached());
    assert!(!h.state().docking.layout.contains(&board), "the dock tab is gone");
    assert!(h.state().docking.hidden.iter().any(|(p, _)| *p == board), "its place is kept");
    assert_eq!(h.get_all_by_label_contains("Board: ").count(), 1, "one board, in the window");
    h.get_by_label("Dock");
    // The board in the window plays.
    click_square(&mut h, "e2");
    click_square(&mut h, "e4");
    assert_eq!(movetext(&h), "1. e4");

    h.get_by_label("Dock").click();
    h.run_steps(3);
    assert!(!h.state().chess_window_detached());
    assert_eq!(h.state().docking.layout.location(&board).ok(), Some(place.clone()), "back where it was");
    assert!(h.query_by_label("Dock").is_none());
    assert_eq!(movetext(&h), "1. e4", "the same game");

    // Out again: a diagram opened on the board leaves it out; the command, on while it is out,
    // docks it; and so does closing the window.
    h.state_mut().set_chess_window_detached(true);
    h.run_steps(3);
    h.state_mut().show_chess_board();
    h.run_steps(3);
    assert!(h.state().chess_window_detached());
    assert!(!h.state().docking.layout.contains(&board));
    assert!(h.state_mut().execute("chess.window.detach"));
    h.run_steps(3);
    assert!(!h.state().chess_window_detached());
    assert!(h.state().docking.layout.contains(&board));
    h.state_mut().set_chess_window_detached(true);
    h.run_steps(3);
    h.get_by_label("Close window").click();
    h.run_steps(3);
    assert!(!h.state().chess_window_detached(), "closing the window docks the board");
    assert_eq!(h.state().docking.layout.location(&board).ok(), Some(place));
}

/// T13a: in Read mode, which draws the board beside the page instead of the workspace, a board
/// in its own window is not drawn there too.
#[test]
fn read_mode_shows_the_board_only_in_its_window() {
    let mut h = harness();
    h.state_mut().show_chess_board();
    h.run_steps(3);
    assert!(h.state_mut().execute("view.read_mode"));
    h.run_steps(3);
    assert_eq!(h.get_all_by_label_contains("Board: ").count(), 1, "beside the page");
    h.state_mut().set_chess_window_detached(true);
    h.run_steps(3);
    assert_eq!(h.get_all_by_label_contains("Board: ").count(), 1, "in the window only");
    h.get_by_label("Dock");
}

/// T13a: whether the board is out, and the window's size, come back in the next session; a bad
/// saved value is ignored on its own, never a panic.
#[test]
fn the_window_and_its_size_come_back_and_bad_values_are_ignored() {
    let mut h = harness();
    h.state_mut().set_chess_window_detached(true);
    h.run_steps(3);
    let saved: serde_json::Value = serde_json::from_str(&h.state().persist()).unwrap();
    let window = &saved["docking_detached"];
    assert_eq!(window["detached"], true, "{window}");
    assert_eq!(window["size"].as_array().map(Vec::len), Some(2), "{window}");
    // The next session, with the window made larger meanwhile.
    let mut next = saved.clone();
    next["docking_detached"]["size"] = serde_json::json!([900.0, 640.0]);
    let h = harness_restoring(next.to_string());
    assert!(h.state().chess_window_detached());
    assert!(!h.state().docking.layout.contains(&pdfcraft_ui_egui::docking::Panel::Board));
    h.get_by_label("Dock");
    let [w, ht] = h.state().chess_window_size();
    assert!((w - 900.0).abs() < 1.0 && (ht - 640.0).abs() < 1.0, "the window kept its size: {w} × {ht}");
    let again: serde_json::Value = serde_json::from_str(&h.state().persist()).unwrap();
    assert_eq!(again["docking_detached"]["detached"], true);
    // Bad values: each ignored, the rest used.
    let h = harness_restoring(window_settings(serde_json::json!({"detached": true, "size": [-1.0, 1e40], "position": "here"})));
    assert!(h.state().chess_window_detached());
    assert_eq!(h.state().chess_window_size(), [960.0, 760.0], "the default size");
    for bad in [serde_json::json!("yes"), serde_json::json!({"detached": 3}), serde_json::json!([true]), serde_json::Value::Null] {
        let h = harness_restoring(window_settings(bad.clone()));
        assert!(!h.state().chess_window_detached(), "{bad}");
        assert!(h.query_by_label("Dock").is_none());
    }
}

/// T13a, T13b: the window lifts the docked panel's 480 pt board. A window 760 pt wide or more
/// has the Diagrams and Text tabs beside the board panel, and in a very wide one the board panel
/// has the moves and fields beside the board too (§4.1); a narrow one shows one at a time.
#[test]
fn a_wide_window_has_a_larger_board_beside_the_diagrams() {
    let h = harness_restoring(window_settings(serde_json::json!({"detached": true, "size": [1000.0, 820.0]})));
    assert!(h.state().chess_window_wide());
    let board = h.get_by_label_contains("Board: ").rect();
    assert!(board.width() > 480.0, "{board:?}");
    let scan = h.get_by_label("Scan this page").rect();
    assert!(scan.min.x > board.max.x && scan.min.y < board.max.y, "the Diagrams tab beside the board: {scan:?} {board:?}");
    let moves = h.get_by_label("Moves").rect();
    assert!(moves.min.y > board.max.y && moves.max.x < scan.min.x, "the moves under the board: {moves:?} {board:?}");
    // Very wide: the board column has the notation beside the board (T5b), filling the column
    // from its top, and the Diagrams tab beyond it.
    let mut h = harness_restoring(window_settings(serde_json::json!({"detached": true, "size": [1390.0, 900.0]})));
    let board = h.get_by_label_contains("Board: ").rect();
    let fen = h.get_by_label("FEN").rect();
    assert!(fen.min.x > board.max.x, "the fields beside the board: {fen:?} {board:?}");
    let moves = h.get_by_label("Moves").rect();
    assert!(moves.min.x > board.max.x && moves.min.y < board.center().y, "the moves beside the board: {moves:?} {board:?}");
    assert!(moves.height() > 300.0, "the notation fills the column: {moves:?}");
    let scan = h.get_by_label("Scan this page").rect();
    assert!(scan.min.x > moves.max.x, "and the Diagrams tab beside them: {scan:?} {moves:?}");
    // The study board's Analysis section heads that column (T5c); the set-up view takes the
    // board's place in its own column, and the moves stay beside it.
    let analysis = h.get_by_label("Analysis").rect();
    assert!(analysis.min.x > board.max.x && analysis.max.y <= moves.min.y, "{analysis:?} {moves:?}");
    assert!(h.state_mut().execute("chess.tool.setup"));
    h.run_steps(2);
    assert_eq!(h.get_all_by_label("Empty board").count(), 1);
    assert_eq!(h.get_all_by_label_contains("Board: ").count(), 1, "the set-up board alone");
    assert!(h.query_by_label("Analysis").is_none());
    h.get_by_label("Moves");
    h.get_by_label("Cancel").click();
    h.run_steps(2);
    assert_eq!(format!("{:?}", h.state().chess.tools.tool), "Navigate");
    h.get_by_label_contains("Board: ");
    let h = harness_restoring(window_settings(serde_json::json!({"detached": true, "size": [560.0, 820.0]})));
    assert!(!h.state().chess_window_wide());
    let board = h.get_by_label_contains("Board: ").rect();
    assert!(board.width() <= 520.0, "{board:?}");
    assert!(h.query_by_label("Scan this page").is_none(), "one tab at a time");
}

/// The node labelled `label` in the chess window (the Inspector's Chess panel has some of the
/// same labels).
fn in_window<'a>(h: &'a Harness<'static, PdfCraftApp>, label: &'a str) -> egui_kittest::Node<'a> {
    let window = h.get_all_by_label("Chess").find(|n| n.query_by_label("Dock").is_some()).expect("the chess window");
    window.get_by_label(label)
}

/// T13b: the whole chess panel in its own window. Out, the Diagrams list is drawn once, in the
/// window beside the board; the Inspector's Chess panel only says where it is, and its button
/// docks the window, after which the list is back in the Inspector.
#[test]
fn the_chess_panel_moves_to_the_window_and_the_inspector_says_so() {
    let mut h = harness();
    assert!(h.state_mut().execute("chess.scan"));
    h.run_steps(3);
    assert_eq!(h.state().right, Some(RightPanel::Chess));
    assert_eq!(h.get_all_by_label("Diagram 1 · Position read").count(), 1, "in the Inspector");
    h.state_mut().set_chess_window_detached(true);
    h.run_steps(3);
    assert!(h.state().chess_window_wide(), "the window opens wide");
    let board = h.get_by_label_contains("Board: ").rect();
    let item = h.get_by_label("Diagram 1 · Position read").rect();
    assert!(item.min.x > board.max.x, "the list beside the board, once: {item:?} {board:?}");
    h.get_by_label(START_FEN);
    h.get_by_label("The chess panel is in its own window.");
    assert_eq!(h.get_all_by_label("Scan this page").count(), 1, "the Diagrams tab is drawn once");
    // The Text tab beside the board, chosen in the window.
    in_window(&h, "Text").click();
    h.run_steps(4);
    assert_eq!(h.state().chess_window_tab(), pdfcraft_ui_egui::ChessWindowTab::Text);
    h.get_by_label("Recognised text of page 1");
    h.get_by_label_contains("Board: ");
    assert!(h.query_by_label("Scan this page").is_none());
    // The Inspector brings it back. (Its button is under the window here, which kittest draws
    // inside the main one: pressed the way a screen reader would.)
    h.get_by_label("Bring it back").click_accesskit();
    h.run_steps(3);
    assert!(!h.state().chess_window_detached());
    assert!(h.query_by_label("Dock").is_none(), "the window is gone");
    // The board is back in its tab beside the Inspector, behind it (U5: the user is looking at
    // the Inspector), whose Chess panel has the list again.
    assert!(h.state().docking.layout.contains(&pdfcraft_ui_egui::docking::Panel::Board));
    assert!(h.query_by_label("The chess panel is in its own window.").is_none());
    assert_eq!(h.get_all_by_label("Diagram 1 · Position read").count(), 1, "back in the Inspector");
    pdfcraft_ui_egui::docking::command(h.state_mut(), false, &serde_json::json!({"operation": "activate", "panel": "board"})).unwrap();
    h.run_steps(3);
    assert_eq!(h.get_all_by_label_contains("Board: ").count(), 1);
}

/// The tab in front of the dock group that holds `panel`.
fn front_of(app: &PdfCraftApp, panel: pdfcraft_ui_egui::docking::Panel) -> Option<pdfcraft_ui_egui::docking::Panel> {
    use craft_ui::docking::Node;
    let mut nodes: Vec<&Node<_>> = app.docking.layout.root.iter().collect();
    while let Some(node) = nodes.pop() {
        match node {
            Node::Split { first, second, .. } => nodes.extend([&**first, &**second]),
            Node::Tabs { panels, active } if panels.contains(&panel) => return panels.get(*active).copied(),
            _ => {}
        }
    }
    None
}

/// Wave 10 U5: docking the board back puts it in its place (its group, and its place in the
/// group), and in front only where it was in front when it left. Bring it back, pressed in the
/// Inspector, leaves the Inspector in front, its Chess panel showing the Diagrams again; the
/// window's Dock puts the board in front as it was; a board that left from behind the
/// Inspector comes back behind it; a board in another group goes back there, in front.
#[test]
fn docking_back_returns_the_board_to_its_group_without_covering_the_inspector() {
    use pdfcraft_ui_egui::docking::{self, Panel};
    let (board, inspector) = (Panel::Board, Panel::Inspector);
    let mut h = harness();
    assert!(h.state_mut().execute("chess.scan"));
    h.run_steps(3);
    h.state_mut().show_chess_board();
    h.run_steps(3);
    let place = h.state().docking.layout.location(&board).expect("docked");
    assert_eq!(front_of(h.state(), inspector), Some(board), "the board in front of the Inspector, in its group");
    let out = |h: &mut Harness<'static, PdfCraftApp>| {
        h.state_mut().set_chess_window_detached(true);
        h.run_steps(3);
        assert!(h.state().chess_window_detached());
        assert_eq!(front_of(h.state(), inspector), Some(inspector));
    };

    // Bring it back, from the Inspector the user is looking at.
    out(&mut h);
    h.get_by_label("Bring it back").click_accesskit();
    h.run_steps(3);
    assert!(!h.state().chess_window_detached());
    assert_eq!(h.state().docking.layout.location(&board).ok(), Some(place.clone()), "in its group, where it was in it");
    assert_eq!(front_of(h.state(), board), Some(inspector), "behind the Inspector");
    assert_eq!(h.get_all_by_label("Diagram 1 · Position read").count(), 1, "the Inspector shows the list again");
    assert!(h.query_by_label_contains("Board: ").is_none());

    // In front, out, the window's Dock: in front again, as it left.
    docking::command(h.state_mut(), false, &serde_json::json!({"operation": "activate", "panel": "board"})).unwrap();
    h.run_steps(2);
    out(&mut h);
    h.get_by_label("Dock").click();
    h.run_steps(3);
    assert_eq!(h.state().docking.layout.location(&board).ok(), Some(place.clone()));
    assert_eq!(front_of(h.state(), board), Some(board));

    // Out from behind the Inspector (the command, or the Board tab's menu on a tab behind):
    // back behind it.
    docking::command(h.state_mut(), false, &serde_json::json!({"operation": "activate", "panel": "inspector"})).unwrap();
    h.run_steps(2);
    out(&mut h);
    h.get_by_label("Dock").click();
    h.run_steps(3);
    assert_eq!(h.state().docking.layout.location(&board).ok(), Some(place));
    assert_eq!(front_of(h.state(), board), Some(inspector));

    // In the Tools' group instead: back there, in front, even by Bring it back, and the
    // Inspector stays as it is.
    docking::command(h.state_mut(), false, &serde_json::json!({"operation": "move", "panel": "board", "target": "tools", "zone": "center"})).unwrap();
    docking::command(h.state_mut(), false, &serde_json::json!({"operation": "activate", "panel": "board"})).unwrap();
    h.run_steps(2);
    let place = h.state().docking.layout.location(&board).expect("docked");
    assert_eq!(front_of(h.state(), Panel::Tools), Some(board));
    h.state_mut().set_chess_window_detached(true);
    h.run_steps(3);
    h.get_by_label("Bring it back").click_accesskit();
    h.run_steps(3);
    assert_eq!(h.state().docking.layout.location(&board).ok(), Some(place));
    assert_eq!(front_of(h.state(), Panel::Tools), Some(board), "back with the Tools, in front");
    assert_eq!(front_of(h.state(), inspector), Some(inspector));
}

/// Wave 10 U5: the embedded window keeps the size its contents get, as the native one does
/// (its frame and title bar are added around it), so going from one to the other keeps it.
#[test]
fn the_embedded_window_keeps_the_inner_size_as_the_native_one_does() {
    let mut h = harness_restoring(window_settings(serde_json::json!({"detached": true, "size": [900.0, 640.0]})));
    h.run_steps(4);
    let [w, ht] = h.state().chess_window_size();
    assert!((w - 900.0).abs() < 1.0 && (ht - 640.0).abs() < 1.0, "the size kept: {w} × {ht}");
    let dock = h.get_by_label("Dock").rect().center();
    let window = h.get_all_by_label("Chess").map(|n| n.rect()).find(|r| r.contains(dock)).expect("the chess window");
    assert!(window.width() > 904.0 && window.height() > 660.0, "the frame and title bar around the 900 × 640 contents: {window:?}");
    // Saved and opened again: the same.
    let saved = h.state().persist();
    let h = harness_restoring(saved);
    let [w2, h2] = h.state().chess_window_size();
    assert!((w2 - w).abs() < 0.5 && (h2 - ht).abs() < 0.5, "{w2} × {h2} after {w} × {ht}");
}

/// T13b: a narrow chess window shows the board, the Diagrams or the Text, chosen on a strip. A
/// scan while it is out shows its Diagrams instead of opening the Inspector; a diagram clicked on
/// the page is selected in the window's list, which shows; a double-click puts it on the board,
/// which the window then shows.
#[test]
fn a_narrow_window_switches_tabs_and_follows_the_diagrams_picked_on_the_page() {
    use pdfcraft_ui_egui::ChessWindowTab;
    // Short frames, so two clicks a frame apart are within the 0.3 s double-click delay.
    let mut h = harness_restoring_stepping(window_settings(serde_json::json!({"detached": true, "size": [560.0, 900.0]})), 0.05);
    assert!(!h.state().chess_window_wide());
    assert_eq!(h.state().chess_window_tab(), ChessWindowTab::Board);
    h.get_by_label_contains("Board: ");
    assert!(h.query_by_label("Scan this page").is_none(), "one tab at a time");
    assert!(h.state_mut().execute("chess.scan"));
    h.run_steps(3);
    assert_ne!(h.state().right, Some(RightPanel::Chess), "the diagrams are in the window, not the Inspector");
    assert_eq!(h.state().chess_window_tab(), ChessWindowTab::Diagrams);
    h.get_by_label("Diagram 1 · Position read");
    assert!(h.query_by_label_contains("Board: ").is_none());
    h.get_by_label("Text").click();
    h.run_steps(4);
    assert_eq!(h.state().chess_window_tab(), ChessWindowTab::Text);
    h.get_by_label("Recognised text of page 1");
    assert!(h.query_by_label("Diagram 1 · Position read").is_none());
    h.get_by_label("Board").click();
    h.run_steps(2);
    assert_eq!(h.state().chess_window_tab(), ChessWindowTab::Board);
    h.get_by_label_contains("Board: ");

    // A diagram clicked on the page: selected, and shown selected in the window's list.
    let at = on_page(&h, 140.0, 660.0);
    let dock = h.get_by_label("Dock").rect().center();
    let window = h.get_all_by_label("Chess").map(|n| n.rect()).find(|r| r.contains(dock)).expect("the chess window");
    assert!(!window.contains(at), "the page's diagram is not under the window: {window:?} {at:?}");
    h.hover_at(at);
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
    }
    h.step();
    h.run_steps(8);
    assert_eq!(h.state().chess.results.as_ref().and_then(|r| r.selected), Some((0, 0)));
    assert_eq!(h.state().chess_window_tab(), ChessWindowTab::Diagrams, "the list shows");
    assert_eq!(h.get_by_label("Diagram 1 · Position read").accesskit_node().toggled(), Some(egui::accesskit::Toggled::True), "selected in the list");
    assert_ne!(h.state().right, Some(RightPanel::Chess));
    // A double-click puts it on the board, and the window shows the board.
    h.hover_at(at);
    h.run_steps(1);
    for _ in 0..2 {
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
        }
        h.step();
    }
    h.run_steps(3);
    assert_eq!(h.state().chess.board().source.map(|s| (s.page, s.index)), Some((0, 0)));
    assert_eq!(h.state().chess_window_tab(), ChessWindowTab::Board);
    h.get_by_label("Diagram 1 · Page 1");
    assert!(h.state().chess_window_detached(), "the board stays in the window");
}

/// T13b: the window's tab comes back in the next session; settings saved before it had one
/// (T13a) and a bad value open on the board.
#[test]
fn the_windows_tab_comes_back_and_old_or_bad_values_open_on_the_board() {
    use pdfcraft_ui_egui::ChessWindowTab;
    let mut h = harness_restoring(window_settings(serde_json::json!({"detached": true, "size": [560.0, 900.0]})));
    h.get_by_label("Text").click();
    h.run_steps(3);
    let saved: serde_json::Value = serde_json::from_str(&h.state().persist()).unwrap();
    assert_eq!(saved["docking_detached"]["tab"], "text", "{saved}");
    let h = harness_restoring(saved.to_string());
    assert_eq!(h.state().chess_window_tab(), ChessWindowTab::Text);
    h.get_by_label("Recognised text of page 1");
    assert!(h.query_by_label_contains("Board: ").is_none());
    for tab in [serde_json::json!("diagrams"), serde_json::json!("moves"), serde_json::json!(2), serde_json::Value::Null] {
        let mut window = serde_json::json!({"detached": true, "size": [560.0, 900.0]});
        if !tab.is_null() {
            window["tab"] = tab.clone();
        }
        let h = harness_restoring(window_settings(window));
        assert!(h.state().chess_window_detached(), "{tab}");
        let want = if tab == "diagrams" { ChessWindowTab::Diagrams } else { ChessWindowTab::Board };
        assert_eq!(h.state().chess_window_tab(), want, "{tab}");
    }
}

// ---- T5b: the board panel's layout and the rail's commands ----

/// The rail's registered commands (Ctrl+K, menus, automation) run the rail's own actions: each
/// changes the game as its button does, as one undo step, and the PGN field shows the result.
/// With no moves on the board they are off, and say why.
#[test]
fn the_rails_commands_run_the_rails_actions() {
    let mut h = harness();
    h.state_mut().show_chess_board();
    h.run_steps(3);
    assert!(!h.state_mut().execute("chess.variation.promote_one"), "no moves: off");
    let cases: [(&str, &[u16], &str, &str); 5] = [
        ("chess.variation.promote_one", &[0, 1, 0, 0], "1. e4 c5 (1... e5 2. Nf3) 2. Nf3 (2. c3) 2... d6", "Promote a variation"),
        ("chess.variation.make_main", &[0, 1, 1], "1. e4 c5 (1... e5 2. Nf3) 2. c3 (2. Nf3 d6)", "Make main line"),
        ("chess.variation.demote", &[0, 1, 0, 0], "1. e4 e5 (1... c5 2. c3 (2. Nf3 d6)) 2. Nf3", "Demote a line"),
        ("chess.variation.delete", &[0, 1, 0, 0], "1. e4 e5 2. Nf3", "Delete a variation"),
        ("chess.moves.cut", &[0], "1. e4", "Delete moves"),
    ];
    for (id, path, after, step) in cases {
        put_game(&mut h, FORKS, path);
        assert!(h.state_mut().execute(id), "{id}");
        h.run_steps(2);
        assert_eq!(movetext(&h), after, "{id}");
        let pgn = h.state().chess.board().pgn_text.clone();
        assert!(pgn.ends_with(&format!("{after} *\n")), "{id}: the PGN field shows {pgn}");
        assert_eq!(h.state().chess.board().undo_label().as_deref(), Some(step), "{id}");
        assert!(h.state_mut().execute("edit.undo"));
        h.run_steps(2);
        assert_eq!(movetext(&h), FORKS_TEXT, "{id}: one undo step took it back");
    }
    // Navigate is the mode armed after it.
    assert!(h.state_mut().execute("chess.tool.navigate"));
    assert_eq!(format!("{:?}", h.state().chess.tools.tool), "Navigate");
    // The Structure group shows the tool its command ran last.
    put_game(&mut h, FORKS, &[0, 1, 0, 0]);
    assert!(h.state_mut().execute("chess.variation.demote"));
    h.run_steps(2);
    h.get_by_label(&format!("Demote ({})", key_text(true, false, "↓")));
}

/// The harness in a window `height` points tall.
fn tall_window(height: f32) -> Harness<'static, PdfCraftApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, height)).build_eframe(|_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("boards.pdf", None, boards()).unwrap();
        app
    });
    h.run_steps(4);
    h
}

/// The Board panel's height (spec §4.1): the notation takes what the board and the rest leave,
/// never less than 120 pt; FEN, PGN and the game's details are sections, closed until opened,
/// and one opened takes its room from the notation.
#[test]
fn the_notation_fills_the_panel_and_the_fields_are_closed_sections() {
    let mut h = tall_window(1400.0);
    h.state_mut().show_chess_board();
    h.run_steps(3);
    put_game(&mut h, FORKS, &[0, 1, 0, 0]);
    h.run_steps(3);
    let moves = |h: &Harness<'static, PdfCraftApp>| h.get_by_label("Moves").rect().height();
    let tall = moves(&h);
    assert!(tall > 180.0, "the notation fills the panel, not capped at 180 pt: {tall}");
    for (section, inside) in [("FEN", "Copy FEN"), ("PGN", "Load PGN"), ("Game details", "Event")] {
        assert!(h.query_by_label(inside).is_none(), "{section} is closed until opened");
        h.get_by_label(section).click();
        h.run_steps(3);
        h.get_by_label(inside);
        h.get_by_label(section).click();
        h.run_steps(3);
        assert!(h.query_by_label(inside).is_none(), "{section} closes again");
    }
    assert!((moves(&h) - tall).abs() < 1.0, "all closed again: the notation is as tall as before");
    h.get_by_label("PGN").click();
    h.run_steps(3);
    let shorter = moves(&h);
    assert!(shorter < tall - 40.0 && shorter >= 120.0, "the PGN field took its room from the notation: {tall} → {shorter}");
    // A short window: the notation keeps its least height, and the panel scrolls instead.
    let mut h = tall_window(560.0);
    h.state_mut().show_chess_board();
    h.run_steps(3);
    put_game(&mut h, FORKS, &[0, 1, 0, 0]);
    h.run_steps(3);
    let least = moves(&h);
    assert!((119.0..=121.0).contains(&least), "the notation's least height: {least}");
}

// ---- T6: glyphs, comments and the glyph palette ----

/// Type `text` as the desktop app gets it: through `raw_input_hook`, where the chess scope takes
/// the signs of typed glyphs (kittest itself skips the hook). One frame.
fn host_text(h: &mut Harness<'static, PdfCraftApp>, text: &str) {
    use eframe::App as _;
    let ctx = h.ctx.clone();
    let mut raw = std::mem::take(h.input_mut());
    raw.events.push(egui::Event::Text(text.to_owned()));
    h.state_mut().raw_input_hook(&ctx, &mut raw);
    *h.input_mut() = raw;
    h.step();
}

/// The glyphs of the move shown.
fn glyph_codes(h: &Harness<'static, PdfCraftApp>) -> Vec<u16> {
    let game = &h.state().chess.board().game;
    game.nags(game.current()).to_vec()
}

fn undo_label(h: &Harness<'static, PdfCraftApp>) -> Option<String> {
    h.state().chess.board().undo_label()
}

/// The rail's glyph groups put a glyph on the move shown with the code of the side that made it
/// (↑ on Black's move is $37), a move-quality glyph replaces the other, a click on a glyph the
/// move has takes it off, and each is one undo step. Their buttons are named for the glyphs.
#[test]
fn the_glyph_groups_put_the_sides_code_and_replace_within_their_group() {
    let mut h = harness();
    h.state_mut().show_chess_board();
    h.run_steps(3);
    put_game(&mut h, "1. e4 e5 *", &[0, 0]);
    // Other glyphs shows Development advantage; its tools open on a right-click.
    h.get_by_label("Development advantage").click_secondary();
    h.run_steps(2);
    h.get_by_label("With the initiative").click();
    h.run_steps(2);
    assert_eq!(glyph_codes(&h), [37], "Black made the move: Black's code");
    assert_eq!(undo_label(&h).as_deref(), Some("Glyph ↑"));
    // The group shows it now, on; a click takes it off.
    let shown = h.get_by_label("With the initiative");
    assert_eq!(shown.accesskit_node().toggled(), Some(egui::accesskit::Toggled::True));
    shown.click();
    h.run_steps(2);
    assert_eq!(glyph_codes(&h), [] as [u16; 0]);
    // ! then ?: only ? stays (one quality glyph a move).
    h.get_by_label("Good move").click();
    h.run_steps(2);
    assert_eq!(glyph_codes(&h), [1]);
    h.get_by_label("Good move").click_secondary();
    h.run_steps(2);
    h.get_by_label("Poor move").click();
    h.run_steps(2);
    assert_eq!(glyph_codes(&h), [2]);
    assert!(h.state_mut().execute("edit.undo"));
    h.run_steps(2);
    assert_eq!(glyph_codes(&h), [1], "one undo step took ? back to !");
    // The commands do as the buttons: ?? replaces !.
    assert!(h.state_mut().execute("chess.nag.4"));
    h.run_steps(2);
    assert_eq!(glyph_codes(&h), [4]);
    // White's move: White's code. The group shows the glyph used last.
    put_game(&mut h, "1. e4 e5 *", &[0]);
    h.get_by_label("With the initiative").click();
    h.run_steps(2);
    assert_eq!(glyph_codes(&h), [36]);
}

/// With the chess scope on, the signs typed go on the move shown: two characters within 600 ms
/// make one sign, one alone waits that long, `0` takes the glyphs off; each is one undo step.
/// Typed in a text field they are the field's.
#[test]
fn glyphs_typed_in_the_chess_scope_go_on_the_move_and_not_into_a_text_field() {
    let mut h = harness();
    h.state_mut().show_chess_board();
    h.run_steps(3);
    put_game(&mut h, "1. e4 e5 *", &[0, 0]);
    click_board_title(&mut h);
    host_text(&mut h, "!");
    host_text(&mut h, "?");
    h.run_steps(1);
    assert_eq!(glyph_codes(&h), [5], "!? typed within the wait");
    assert_eq!(undo_label(&h).as_deref(), Some("Glyph !?"));
    host_text(&mut h, "!");
    assert_eq!(glyph_codes(&h), [5], "! waits for a second sign");
    h.run_steps(3);
    assert_eq!(glyph_codes(&h), [1], "the wait is over: ! replaces !?");
    host_text(&mut h, "+=");
    h.run_steps(1);
    assert_eq!(glyph_codes(&h), [1, 14]);
    host_text(&mut h, "0");
    h.run_steps(1);
    assert_eq!(glyph_codes(&h), [] as [u16; 0]);
    assert_eq!(undo_label(&h).as_deref(), Some("Clear glyphs"));
    assert!(h.state_mut().execute("edit.undo"));
    h.run_steps(2);
    assert_eq!(glyph_codes(&h), [1, 14], "one step put both back");
    // In the comment field, ! is text.
    let comment = h.get_all_by_role(egui::accesskit::Role::MultilineTextInput).find(|n| n.value().unwrap_or_default().is_empty()).unwrap();
    comment.focus();
    h.run_steps(2);
    host_text(&mut h, "!");
    h.run_steps(3);
    let node = h.state().chess.board().game.current();
    assert_eq!(h.state().chess.board().game.comment(node), "!", "the field took it");
    assert_eq!(glyph_codes(&h), [1, 14], "and no glyph changed");
}

/// The comment before a move (the comment editor: Enter with the chess scope on) is written
/// after the `(` of the variation the move starts; before a move that continues its line, PGN
/// has no place for it and the editor says where it goes.
#[test]
fn the_comment_before_a_move_goes_where_pgn_has_room_and_says_where_it_has_none() {
    let mut h = harness();
    h.state_mut().show_chess_board();
    h.run_steps(3);
    put_game(&mut h, FORKS, &[0, 1]);
    click_board_title(&mut h);
    assert_eq!(h.get_all_by_role(egui::accesskit::Role::MultilineTextInput).count(), 1, "the comment after the move only");
    host_key(&mut h, egui::Modifiers::NONE, egui::Key::Enter);
    assert!(h.state().chess.board().comment_editor_open(), "Enter opened the comment editor");
    let before = h.get_by_label("Comment before 1…c5");
    before.focus();
    before.type_text("Sicilian");
    h.run_steps(2);
    assert!(h.query_by_label_contains("In PGN this comment").is_none(), "a variation's first move has its place");
    click_board_title(&mut h);
    let c5 = h.state().chess.board().game.current();
    assert_eq!(h.state().chess.board().game.comment_before(c5), "Sicilian");
    assert!(h.state().chess.board().pgn_text.contains("({Sicilian} 1... c5"), "{}", h.state().chess.board().pgn_text);
    assert_eq!(undo_label(&h).as_deref(), Some("Edit the comment before a move"));
    assert!(h.state_mut().execute("edit.undo"));
    h.run_steps(2);
    assert_eq!(h.state().chess.board().game.comment_before(c5), "", "one step");
    // 2.Nf3 continues 1...c5's line: PGN writes its comment after 1...c5.
    put_game(&mut h, FORKS, &[0, 1, 0]);
    let before = h.get_by_label("Comment before 2.Nf3");
    before.focus();
    before.type_text("Main idea");
    h.run_steps(2);
    h.get_by_label_contains("In PGN this comment is written after the previous move.");
    // Esc (the field gave the keyboard back) closes the editor.
    click_board_title(&mut h);
    host_key(&mut h, egui::Modifiers::NONE, egui::Key::Escape);
    assert!(!h.state().chess.board().comment_editor_open());
}

/// The glyph palette (G) types into the field that had the keyboard, at its cursor; with no field
/// it adds to the move's comment. Each insertion is one undo step.
#[test]
fn the_glyph_palette_types_at_the_cursor_of_the_field_that_had_the_keyboard() {
    // Tall enough for the open palette to show whole.
    let mut h = tall_window(1400.0);
    h.state_mut().show_chess_board();
    h.run_steps(3);
    put_game(&mut h, "1. e4 e5 *", &[0]);
    click_board_title(&mut h);
    host_key(&mut h, egui::Modifiers::NONE, egui::Key::G);
    h.get_by_label("White knight");
    let comment = h.get_all_by_role(egui::accesskit::Role::MultilineTextInput).find(|n| n.value().unwrap_or_default().is_empty()).unwrap();
    comment.focus();
    comment.type_text("a b");
    h.run_steps(2);
    for _ in 0..2 {
        h.key_press(egui::Key::ArrowLeft);
        h.run_steps(1);
    }
    click_board_title(&mut h);
    let e4 = h.state().chess.board().game.current();
    assert_eq!(h.state().chess.board().game.comment(e4), "a b");
    // The field lost the keyboard to the title: the knight goes to the comment's end.
    h.get_by_label("White knight").click();
    h.run_steps(3);
    assert_eq!(h.state().chess.board().game.comment(e4), "a b ♘");
    assert!(h.state_mut().execute("edit.undo"));
    h.run_steps(2);
    assert_eq!(h.state().chess.board().game.comment(e4), "a b", "one step");
    // In the field, at its cursor: after "a".
    let comment = h.get_all_by_role(egui::accesskit::Role::MultilineTextInput).find(|n| n.value().as_deref() == Some("a b")).unwrap();
    comment.focus();
    h.run_steps(1);
    h.key_press(egui::Key::Home);
    h.run_steps(1);
    h.key_press(egui::Key::ArrowRight);
    h.run_steps(1);
    h.get_by_label("Black knight").click();
    h.run_steps(3);
    assert_eq!(h.state().chess.board().game.comment(e4), "a♞ b");
    assert!(h.ctx.egui_wants_keyboard_input(), "the field has the keyboard back");
    h.get_by_label("Capture sign").click();
    h.run_steps(3);
    assert_eq!(h.state().chess.board().game.comment(e4), "a♞× b", "after the knight: the cursor moved past it");
}

/// The Move list beside the page: the board's toggle opens it against the page's right edge and
/// the notation is drawn there alone, the board keeping its sections; its own buttons put it
/// against the left edge, where it opens again; a move clicked in it is shown on the board.
/// Closed, the notation is back under the board. Read mode draws it on the same side.
#[test]
fn the_move_list_stands_beside_the_page_on_either_side() {
    use pdfcraft_ui_egui::docking::Panel;
    let mut h = tall_window(1000.0);
    h.state_mut().show_chess_board();
    h.run_steps(3);
    put_game(&mut h, FORKS, &[0, 1, 0, 0]);
    let under_board = |h: &Harness<'static, PdfCraftApp>| {
        let (moves, board) = (h.get_by_label("Moves").rect(), h.get_by_label_contains("Board: ").rect());
        moves.min.y > board.max.y && moves.min.x < board.max.x
    };
    assert!(under_board(&h));
    h.get_by_label("Move list beside the page").click();
    h.run_steps(3);
    assert!(h.state().docking.layout.contains(&Panel::Moves));
    assert_eq!(h.get_all_by_label("Moves").count(), 1, "the notation in one place");
    let page = h.state().views[0].page_screen_rect(0).expect("page 1 on screen");
    let (moves, board) = (h.get_by_label("Moves").rect(), h.get_by_label_contains("Board: ").rect());
    assert!(moves.min.x >= page.max.x && moves.max.x <= board.min.x, "between the page and the board: {moves:?} {page:?} {board:?}");
    assert!(moves.height() > 400.0, "the list's height: {moves:?}");
    h.get_by_label("PGN");
    h.get_by_label("Left").click();
    h.run_steps(4);
    assert!(h.state().docking.moves_on_left());
    let page = h.state().views[0].page_screen_rect(0).expect("page 1 on screen");
    let moves = h.get_by_label("Moves").rect();
    assert!(moves.max.x <= page.min.x, "against the page's left edge: {moves:?} {page:?}");
    // 1. e4 from 2. Nf3 in the variation: the board follows the list.
    h.get_by_label("e4").click();
    h.run_steps(2);
    let game = &h.state().chess.board().game;
    assert_eq!(game.san(game.current()), "e4");
    h.get_by_label("Move list beside the page").click();
    h.run_steps(3);
    assert!(!h.state().docking.layout.contains(&Panel::Moves));
    assert_eq!(h.get_all_by_label("Moves").count(), 1);
    assert!(under_board(&h), "back under the board");
    // The command opens it again where it was; Read mode keeps the side.
    assert!(h.state_mut().execute("chess.move_list"));
    h.run_steps(3);
    let page = h.state().views[0].page_screen_rect(0).expect("page 1 on screen");
    assert!(h.get_by_label("Moves").rect().max.x <= page.min.x);
    h.state_mut().mode = pdfcraft_ui_egui::Mode::Read;
    h.run_steps(4);
    let page = h.state().views[0].page_screen_rect(0).expect("page 1 on screen");
    assert_eq!(h.get_all_by_label("Moves").count(), 1);
    assert!(h.get_by_label("Moves").rect().max.x <= page.min.x, "Read mode: on the left too");
}

/// Plan HC11: Page Down and Page Up, and the buttons beside the move arrows, go from variation
/// to variation in reading order; at either end the button is off and the key does nothing.
#[test]
fn page_keys_and_buttons_walk_the_variations() {
    use egui::{Key, Modifiers};
    let mut h = harness();
    h.state_mut().show_chess_board();
    h.run_steps(3);
    put_game(&mut h, FORKS, &[]);
    click_board_title(&mut h);
    // FORKS: 1. e4 e5 (1... c5 2. Nf3 (2. c3) d6) 2. Nf3
    host_key(&mut h, Modifiers::NONE, Key::PageDown);
    assert_eq!(shown(&h), "c5");
    host_key(&mut h, Modifiers::NONE, Key::PageDown);
    assert_eq!(shown(&h), "c3");
    host_key(&mut h, Modifiers::NONE, Key::PageDown);
    assert_eq!(shown(&h), "c3", "the last variation: nowhere further");
    assert!(h.get_by_label("Next variation (Page Down)").accesskit_node().is_disabled());
    h.get_by_label("Previous variation (Page Up)").click();
    h.run_steps(2);
    assert_eq!(shown(&h), "c5");
    host_key(&mut h, Modifiers::NONE, Key::PageUp);
    assert_eq!(shown(&h), "c5", "the first variation");
    assert!(h.get_by_label("Previous variation (Page Up)").accesskit_node().is_disabled());
}

/// Plan A8 P1b: every button of the chess panels has a name the interface fonts can draw (no
/// loose ⏮ ◀ ▶ ✕ characters that show as boxes), with a game on the board and the editor open.
#[test]
fn no_chess_button_has_a_name_the_fonts_cannot_draw() {
    let mut h = harness();
    h.state_mut().show_chess_board();
    h.run_steps(3);
    put_game(&mut h, FORKS, &[0, 1, 0]);
    h.run_steps(3);
    let font = egui::FontId::proportional(13.0);
    let names: Vec<String> = h.get_all_by_role(egui::accesskit::Role::Button).filter_map(|n| n.accesskit_node().label()).collect();
    assert!(names.iter().any(|n| n == "Previous move (Left arrow)"), "{names:?}");
    let missing: Vec<&String> = names.iter().filter(|n| !h.ctx.fonts_mut(|f| f.has_glyphs(&font, n))).collect();
    assert!(missing.is_empty(), "buttons the fonts cannot draw: {missing:?}");
}

// ─── The Diagram tab and its corrections (plan A8, P3b2) ──────────────────────────────────────

/// An empty folder for one test's diagram corrections.
fn corrections_dir(test: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcraft-ui-corrections-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// The corrections files kept in `dir`.
fn correction_files(dir: &std::path::Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|d| d.flatten().filter_map(|e| e.file_name().to_str().map(str::to_owned)).filter(|n| n.ends_with(".json")).collect())
        .unwrap_or_default()
}

/// What the user did with diagram `index` of page 1, as the list and the page marks show it.
fn kind(h: &Harness<'static, PdfCraftApp>, index: usize) -> Option<String> {
    h.state().chess.results.as_ref().and_then(|r| r.kind(0, index)).map(|k| format!("{k:?}"))
}

/// Picking a diagram opens the Diagram tab beside the Inspector, the list still in sight;
/// Confirm keeps the edited position as a correction on disk, the diagram is Corrected in the list
/// and off the review queue, and the readings the session gives (the Text tab, exports, links)
/// take it. Opened again, the document finds it and the tab shows the kept position.
#[test]
fn confirming_keeps_the_correction_and_it_comes_back_when_reopened() {
    let dir = corrections_dir("confirm");
    {
        let mut h = harness_with(boards(), 0.25, egui::vec2(1400.0, 1000.0), Some(dir.clone()));
        scan_and_edit_second(&mut h);
        assert!(h.state().docking.layout.contains(&pdfcraft_ui_egui::docking::Panel::Diagram), "the Diagram tab opened");
        h.get_by_label_contains("Diagram 1 · ");
        let queue = h.state().chess.results.as_ref().unwrap().review_queue();
        if queue.is_empty() {
            // With the user's piece model installed the drawn board may be read and accepted.
            return;
        }
        h.get_by_label("Confirm and next").click();
        h.run_steps(3);
        assert_eq!(kind(&h, 1).as_deref(), Some("Corrected"));
        assert_eq!(h.state().toast.as_ref().map(|t| t.0.as_str()), Some("Diagram 2, page 1: Corrected"));
        assert!(h.state().chess.results.as_ref().unwrap().review_queue().is_empty(), "off the review queue");
        h.get_by_label("Diagram 2 · Corrected");
        assert_eq!(correction_files(&dir).len(), 1, "kept on disk");
        let id = h.state().active_ids().unwrap().1;
        let settings = pdfcraft_engine::chess::ChessScanSettings::default();
        let corrected = h.state_mut().session.chess_scan_corrected(id, &[0], &settings).unwrap();
        assert_eq!(corrected.applied.iter().map(|a| (a.index, a.fen.as_str())).collect::<Vec<_>>(), vec![(1, EDITED)], "the session reads it");
    }
    let mut h = harness_with(boards(), 0.25, egui::vec2(1400.0, 1000.0), Some(dir.clone()));
    assert!(h.state_mut().execute("chess.scan"));
    h.run_steps(3);
    assert_eq!(kind(&h, 1).as_deref(), Some("Corrected"), "found again");
    h.get_by_label_contains("Diagram 2 · ").click();
    h.run_steps(3);
    assert_eq!(h.state().chess.editor.as_ref().map(|e| e.fen()).as_deref(), Some(EDITED), "the tab shows the kept position");
    // Forgotten: the reading is back, and so is the diagram in the review queue.
    h.get_by_label("More actions").click();
    h.run_steps(2);
    h.get_by_label("Forget the correction").click();
    h.run_steps(3);
    assert_eq!(kind(&h, 1), None);
    assert_eq!(h.state().chess.results.as_ref().unwrap().review_queue(), vec![(0, 1)]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The font diagram confirmed as read is Verified; Ctrl+Enter over the tab confirms too; the
/// last way of confirming is the one in front next time.
#[test]
fn a_reading_confirmed_as_it_is_is_verified() {
    let dir = corrections_dir("verified");
    let mut h = harness_with(boards(), 0.25, egui::vec2(1400.0, 1000.0), Some(dir.clone()));
    assert!(h.state_mut().execute("chess.scan"));
    h.run_steps(3);
    h.get_by_label_contains("Diagram 1 · ").click();
    h.run_steps(3);
    assert_eq!(h.state().chess.editor.as_ref().map(|e| e.fen()).as_deref(), Some(START_FEN));
    h.get_by_label("More ways to confirm").click();
    h.run_steps(2);
    h.get_by_label("Only confirm").click();
    h.run_steps(3);
    assert_eq!(kind(&h, 0).as_deref(), Some("Verified"));
    assert!(h.state().chess.confirm_only, "remembered");
    h.get_by_label("Only confirm");
    // Ctrl+Enter with the pointer over the tab: the drawn board, set and confirmed.
    h.get_by_label_contains("Diagram 2 · ").click();
    h.run_steps(3);
    h.state_mut().chess.editor.as_mut().unwrap().position = pdfcraft_engine::chess::pdfcraft_chess::fen::parse(EDITED).unwrap();
    h.hover_at(h.get_by_label_contains("Diagram board: ").rect().center());
    h.run_steps(1);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Enter);
    h.run_steps(3);
    assert_eq!(kind(&h, 1).as_deref(), Some("Corrected"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Save As takes the corrections along: the saved file, opened again, finds them.
#[test]
fn saving_as_keeps_the_corrections_for_the_saved_file() {
    let dir = corrections_dir("save-as");
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("saved copy.pdf");
    {
        let mut h = harness_with(boards(), 0.25, egui::vec2(1400.0, 1000.0), Some(dir.join("store")));
        assert!(h.state_mut().execute("chess.scan"));
        h.run_steps(3);
        h.get_by_label_contains("Diagram 1 · ").click();
        h.run_steps(3);
        h.state_mut().chess.confirm_only = true;
        h.run_steps(1);
        h.get_by_label("Only confirm").click();
        h.run_steps(3);
        assert_eq!(kind(&h, 0).as_deref(), Some("Verified"));
        h.state_mut().save_override = Some(out.to_string_lossy().into_owned());
        assert!(h.state_mut().save_active(pdfcraft_ui_egui::SaveTarget::As));
    }
    let bytes = std::fs::read(&out).unwrap();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 1000.0)).build_eframe({
        let (out, store) = (out.clone(), dir.join("store"));
        move |_cc| {
            let mut app = PdfCraftApp::new();
            app.set_option("language", "en").unwrap();
            app.chess.corrections.use_folder(store);
            app.open_bytes("saved copy.pdf", Some(out.to_string_lossy().into_owned()), bytes).unwrap();
            app.run_inline = true;
            app
        }
    });
    h.run_steps(4);
    assert!(h.state_mut().execute("chess.scan"));
    h.run_steps(3);
    assert_eq!(kind(&h, 0).as_deref(), Some("Verified"), "the saved file finds them");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Preferences ▸ Classic diagram editor: the editor stays in the Diagrams panel, and no Diagram
/// tab opens.
#[test]
fn the_classic_editor_stays_in_the_diagrams_panel() {
    let mut h = harness();
    h.state_mut().chess.classic_editor = true;
    assert!(h.state_mut().execute("chess.scan"));
    h.run_steps(3);
    h.get_by_label_contains("Diagram 1 · ").click();
    h.run_steps(3);
    assert!(!h.state().docking.layout.contains(&pdfcraft_ui_egui::docking::Panel::Diagram));
    h.get_by_label("Edit Diagram 1, page 1");
    let saved = h.state().persist();
    let mut again = PdfCraftApp::new();
    again.restore(&saved);
    assert!(again.chess.classic_editor, "kept with the preferences");
}

// ─── Drafts and the question before closing, exporting or detecting again (plan A8, P3b3) ─────

/// Change the drawn board's position in the Diagram tab (not confirmed).
fn draft_second(h: &mut Harness<'static, PdfCraftApp>) {
    scan_and_edit_second(h);
    assert!(h.state().chess.editor.as_ref().is_some_and(|e| e.dirty()), "changed, not confirmed");
}

/// Leaving a diagram with changes keeps them as its draft, marked in the list; opening it again
/// shows them, with the tab saying they are not confirmed.
#[test]
fn a_draft_is_kept_per_diagram_and_marked() {
    let mut h = harness();
    draft_second(&mut h);
    h.get_by_label("Unconfirmed changes");
    h.get_by_label_contains("Diagram 1 · ").click();
    h.run_steps(3);
    assert_eq!(h.state().chess.drafts.len(), 1, "the drawn board's draft");
    assert!(h.query_by_label("Unconfirmed changes").is_none(), "diagram 1 has none");
    h.get_by_label_contains("· Unconfirmed changes");
    h.get_by_label_contains("Diagram 2 · ").click();
    h.run_steps(3);
    assert_eq!(h.state().chess.editor.as_ref().map(|e| e.fen()).as_deref(), Some(EDITED), "the draft is back");
    h.get_by_label("Unconfirmed changes");
}

/// Closing the document with a draft asks first; Keep editing keeps it open on the diagram,
/// Discard changes drops the draft and the document closes.
#[test]
fn closing_with_a_draft_asks_first() {
    let mut h = harness();
    draft_second(&mut h);
    h.state_mut().request_close_tab(0);
    h.run_steps(2);
    h.get_by_label("Keep the correction of diagram 2 (page 1)?");
    assert_eq!(h.state().views.len(), 1, "still open");
    h.get_by_label("Keep editing").click();
    h.run_steps(3);
    assert!(h.state().chess.draft_prompt.is_none() && h.state().views.len() == 1);
    assert_eq!(h.state().chess.editor.as_ref().map(|e| (e.page, e.index)), Some((0, 1)));
    h.state_mut().request_close_tab(0);
    h.run_steps(2);
    h.get_by_label("Discard changes").click();
    h.run_steps(3);
    assert!(h.state().views.is_empty(), "closed once the draft was answered");
    assert!(h.state().chess.drafts.is_empty());
}

/// Exporting with a draft asks first; Confirm correction keeps it, and the export then writes
/// the corrected position.
#[test]
fn exporting_with_a_draft_confirms_it_first() {
    let dir = corrections_dir("draft-export");
    let mut h = harness_with(boards(), 0.25, egui::vec2(1400.0, 1000.0), Some(dir.join("store")));
    draft_second(&mut h);
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("book.pgn");
    h.state_mut().save_override = Some(out.to_string_lossy().into_owned());
    h.state_mut().export_chess_dialog(pdfcraft_engine::chess::export::ChessFormat::Pgn);
    h.run_steps(2);
    assert!(!out.exists(), "nothing written before the answer");
    h.get_by_label("Confirm correction").click();
    h.run_steps(6);
    assert_eq!(kind(&h, 1).as_deref(), Some("Corrected"));
    let pgn = std::fs::read_to_string(&out).expect("exported once answered");
    assert!(pgn.contains(EDITED), "the corrected position: {pgn}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Detecting again over a draft asks first; Discard changes drops it and the scan runs.
#[test]
fn detecting_again_asks_about_drafts() {
    let mut h = harness();
    draft_second(&mut h);
    assert!(h.state_mut().execute("chess.scan"));
    h.run_steps(2);
    h.get_by_label("Keep the correction of diagram 2 (page 1)?");
    h.state_mut().toast = None;
    h.get_by_label("Discard changes").click();
    h.run_steps(4);
    assert!(h.state().chess.drafts.is_empty());
    assert_eq!(h.state().toast.as_ref().map(|t| t.0.as_str()), Some("Diagrams found: 2"), "then the scan ran");
}

/// The tab's first-time hint shows until put away, and stays away (kept with the preferences).
#[test]
fn the_first_time_hint_goes_once_put_away() {
    let mut h = harness();
    scan_and_edit_second(&mut h);
    let hint = "Click a square to put the piece there. Confirm keeps the correction without changing the PDF.";
    h.get_by_label(hint);
    h.get_by_label("Dismiss").click();
    h.run_steps(2);
    assert!(h.query_by_label(hint).is_none());
    let mut again = PdfCraftApp::new();
    again.restore(&h.state().persist());
    assert!(again.chess.diagram_hint_seen);
}
