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
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 1000.0)).build_eframe(|_cc| {
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
