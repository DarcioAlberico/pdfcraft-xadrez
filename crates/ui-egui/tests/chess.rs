//! Chess ▸ Diagrams in the real shell (egui_kittest): scanning a page, the boxes on it, the
//! Chess diagrams panel and what it does (blueprint S16a). The PDF is built here: a board set in
//! the Chess Merida diagram font, read exactly without a model, and a board drawn as filled
//! squares, which is found and, without the user's piece model, not read.

use egui::Pos2;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use pdfcraft_engine::{Edit, NewAnnotation, Shape, Style};
use pdfcraft_ui_egui::{PdfCraftApp, RightPanel};

const START_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
/// The start position in the Chess Merida layout, top rank first: light squares lower case, dark
/// upper case; empty light ' ', empty dark '+'.
const MERIDA_START: [&str; 8] = ["tMvWlVmT", "OoOoOoOo", " + + + +", "+ + + + ", " + + + +", "+ + + + ", "pPpPpPpP", "RnBqKbNr"];

/// A 600 × 800 pt page with the start position in a diagram font, top left (x 60..220, y
/// 580..740 in user space), and a board of filled squares under it (x 60..220, y 240..400).
fn boards() -> Vec<u8> {
    let mut content = String::from("BT /F1 20 Tf 60 724 Td ");
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
        "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        format!("<< /Type /Font /Subtype /Type1 /BaseFont /ChessMerida /FirstChar 32 /LastChar 126 /Widths [{widths}] >>"),
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
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 1000.0)).with_step_dt(step_dt).build_eframe(|_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("boards.pdf", None, boards()).unwrap();
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
    h.get_by_label(EDITED);
    h.get_by_label("Legal position");
    // Click e4 on the board (from White's side: file e = column 4, rank 4 = row 4 from the top).
    let board = h.get_by_label_contains("Board: ").rect();
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
    // Two white kings is not a position.
    let ed = h.state_mut().chess.editor.as_mut().unwrap();
    ed.position = pdfcraft_engine::chess::pdfcraft_chess::fen::parse("4K3/8/8/8/8/8/8/R3K3 w - - 0 1").unwrap();
    h.run_steps(2);
    h.get_by_label("Not a legal position");
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
    h.get_by_label("Next to review ▶").click();
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
    assert_eq!(h.state().chess.board.game.fen(), START_FEN);
    assert_eq!(h.state().chess.board.source.map(|s| (s.page, s.index)), Some((0, 0)));
    h.get_by_label("Diagram 1 · Page 1");
    h.get_by_label("White to move");

    click_square(&mut h, "e2");
    click_square(&mut h, "e4");
    click_square(&mut h, "e7");
    click_square(&mut h, "e5");
    assert_eq!(h.state().chess.board.game.movetext(), "1. e4 e5");
    h.get_by_label("e5");
    // Back one move with the button, then a different move starts a variation.
    h.get_by_label("◀").click();
    h.run_steps(2);
    assert_eq!(h.state().chess.board.game.ply(), 1);
    click_square(&mut h, "c7");
    click_square(&mut h, "c5");
    assert_eq!(h.state().chess.board.game.movetext(), "1. e4 e5 (1... c5)");
    assert!(h.state().chess.board.pgn_text.ends_with("1. e4 e5 (1... c5) *\n"), "{}", h.state().chess.board.pgn_text);
    // A glyph on the move shown.
    h.get_by_label("!?").click();
    h.run_steps(2);
    let c5 = h.state().chess.board.game.current();
    assert_eq!(h.state().chess.board.game.nags(c5), &[5]);
    // The move's menu makes the variation the main line.
    h.get_by_label("c5!?").click_secondary();
    h.run_steps(2);
    h.get_by_label("Make this the main line").click();
    h.run_steps(2);
    assert_eq!(h.state().chess.board.game.movetext(), "1. e4 c5 $5 (1... e5)");
    // Copy FEN copies the position on the board.
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
    let node = h.state().chess.board.game.current();
    assert_eq!(h.state().chess.board.game.comment(node), "good move");
}

/// Open on the board, in the Chess diagrams panel, puts that diagram on the panel's Study board
/// tab, where it is played.
#[test]
fn open_on_the_board_from_the_panel() {
    let mut h = harness();
    assert!(h.state_mut().execute("chess.scan"));
    h.run_steps(3);
    h.get_all_by_label("Open on the board").next().expect("diagram 1 has a position").click();
    h.run_steps(3);
    assert_eq!(format!("{:?}", h.state().chess.tab), "Study");
    assert!(!h.state().docking.layout.contains(&pdfcraft_ui_egui::docking::Panel::Board), "the tab shows it; the Board panel stays closed");
    assert_eq!(h.state().chess.board.game.fen(), START_FEN);
    h.get_by_label(&format!("Board: {START_FEN}"));
    click_square(&mut h, "e2");
    click_square(&mut h, "e4");
    assert_eq!(h.state().chess.board.game.movetext(), "1. e4");
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
    h.state().chess.board.marks().iter().map(|m| format!("{}{}{:?}", m.from, m.to, m.color)).collect()
}

/// The Study board tab: a position set up from an empty board and played from, drawings made
/// with the right button, and the engine part saying when its engine cannot be started.
#[test]
fn the_study_board_sets_up_a_position_and_draws_on_it() {
    let mut h = harness();
    assert!(h.state_mut().execute("chess.panel"));
    h.run_steps(2);
    h.get_by_label("Study board").click();
    h.run_steps(2);
    h.get_by_label(&format!("Board: {START_FEN}"));
    h.get_by_label("Set up a position").click();
    h.run_steps(2);
    h.get_by_label("Empty board").click();
    h.run_steps(2);
    for (piece, square) in [("White king", "e1"), ("Black king", "e8"), ("White rook", "a1")] {
        h.get_by_label(piece).click();
        h.run_steps(1);
        click_square(&mut h, square);
    }
    h.get_by_label("Legal position");
    h.get_by_label("Done").click();
    h.run_steps(2);
    assert!(h.state().chess.study.setup.is_none());
    assert_eq!(h.state().chess.board.game.fen(), "4k3/8/8/8/8/8/8/R3K3 w - - 0 1");
    click_square(&mut h, "a1");
    click_square(&mut h, "a8");
    assert_eq!(h.state().chess.board.game.movetext(), "1. Ra8+");

    // A right click circles a square; a right drag with Shift held draws a red arrow.
    drag_square(&mut h, egui::PointerButton::Secondary, "d4", "d4", egui::Modifiers::NONE);
    drag_square(&mut h, egui::PointerButton::Secondary, "e8", "d7", egui::Modifiers::SHIFT);
    assert_eq!(marks(&h), ["d4d4Green", "e8d7Red"]);
    // The drawings stay with their position.
    h.get_by_label("◀").click();
    h.run_steps(2);
    assert!(marks(&h).is_empty());
    h.get_by_label("▶").click();
    h.run_steps(2);
    assert_eq!(marks(&h).len(), 2);
    h.get_by_label("Clear drawings").click();
    h.run_steps(2);
    assert!(marks(&h).is_empty());

    // Analysis with an engine that cannot be started says so.
    h.state_mut().chess.study.engine_path = Some("this-engine-does-not-exist-anywhere.exe".into());
    h.get_by_label("Engine analysis").click();
    h.run_steps(3);
    assert!(h.get_all_by_label_contains("The engine couldn't be started").next().is_some());
}

/// With a real UCI engine (`PDFCRAFT_CHESS_TEST_ENGINE`, e.g. Stockfish), the Study board shows
/// its lines and plays one when its move is clicked. Skipped when the variable is not set.
#[test]
fn the_study_board_analyses_with_a_real_engine() {
    let Some(engine) = std::env::var_os("PDFCRAFT_CHESS_TEST_ENGINE") else {
        eprintln!("PDFCRAFT_CHESS_TEST_ENGINE not set: skipped");
        return;
    };
    let mut h = harness();
    assert!(h.state_mut().execute("chess.panel"));
    h.run_steps(2);
    h.get_by_label("Study board").click();
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
    assert_eq!(h.state().chess.board.game.len(), 1, "{}", h.state().chess.board.game.movetext());
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
    assert_eq!(h.state().chess.board.game.movetext(), "1. e4 e5 2. Nf3");
    click_square(&mut h, "a4");
    // The palette and the board's button name the move.
    assert_eq!(palette_labels(&mut h, "Undo Play Nf3"), 2, "the palette's Undo and the board's button");
    assert!(h.state_mut().execute("edit.undo"));
    h.run_steps(2);
    assert_eq!(h.state().chess.board.game.movetext(), "1. e4 e5");
    shortcut(&mut h, egui::Key::Z, false);
    assert_eq!(h.state().chess.board.game.movetext(), "1. e4");
    assert_eq!(pdf_undo(&h).as_deref(), Some(pdf.as_str()), "the document is untouched");
    shortcut(&mut h, egui::Key::Z, true);
    assert_eq!(h.state().chess.board.game.movetext(), "1. e4 e5");
    h.get_by_label("Undo Play e5").click();
    h.run_steps(2);
    assert_eq!(h.state().chess.board.game.movetext(), "1. e4");
    h.get_by_label("Redo Play e5").click();
    h.run_steps(2);
    assert_eq!(h.state().chess.board.game.movetext(), "1. e4 e5");

    // A click on the page: Undo goes back to the document.
    let page = on_page(&h, 400.0, 300.0);
    click_at(&mut h, page);
    assert!(!h.state().chess.in_scope());
    assert_eq!(palette_labels(&mut h, &format!("Undo {pdf}")), 1);
    shortcut(&mut h, egui::Key::Z, false);
    assert_eq!(pdf_undo(&h), None, "the rectangle was undone");
    assert_eq!(h.state().chess.board.game.movetext(), "1. e4 e5", "the game is untouched");
    // And a move played on the board brings it back to the game.
    click_square(&mut h, "g1");
    click_square(&mut h, "f3");
    assert!(h.state().chess.in_scope());
    shortcut(&mut h, egui::Key::Z, false);
    assert_eq!(h.state().chess.board.game.movetext(), "1. e4 e5");
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
    let node = h.state().chess.board.game.current();
    assert_eq!(h.state().chess.board.game.comment(node), "good move");
    assert_eq!(h.state().chess.board.undo_label().as_deref(), Some("Edit move comment"));
    assert!(h.state_mut().execute("edit.undo"));
    h.run_steps(2);
    assert_eq!(h.state().chess.board.game.comment(node), "");
    assert_eq!(h.state().chess.board.game.movetext(), "1. e4", "one step took the whole comment back, and only it");
    assert_eq!(h.state().chess.board.undo_label().as_deref(), Some("Play e4"));
    assert!(h.state_mut().execute("edit.redo"));
    h.run_steps(2);
    assert_eq!(h.state().chess.board.game.comment(node), "good move");
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
    assert!(h.state().chess.board.pgn_text.contains("1. e4 *"), "{}", h.state().chess.board.pgn_text);
    {
        let app = h.state_mut();
        assert!(app.execute("edit.undo"));
        app.chess.board.play_uci_line(&["d2d4".into()]);
    }
    h.run_steps(2);
    let board = &h.state().chess.board;
    assert_eq!(board.game.movetext(), "1. d4");
    assert!(board.pgn_text.contains("1. d4 *"), "the PGN field is stale: {}", board.pgn_text);
    assert_eq!(board.fen_text, "rnbqkbnr/pppppppp/8/8/3P4/8/PPP1PPPP/RNBQKBNR b KQkq d3 0 1");
}
