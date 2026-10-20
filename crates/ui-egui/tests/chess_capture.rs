//! Chess ▸ PGN toolbar ▸ Capture (T7a) in the real shell (egui_kittest): text selected on a page,
//! or pasted, goes into the game on the chess board. The PDFs are built here: Helvetica for the
//! prose and Helvetica-Bold for the game's own moves, as books set them; figurines in SemFig, a
//! catalogued figurine font, referred to by its `/BaseFont` name and never embedded.

use egui::{Modifiers, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use pdfcraft_ui_egui::{PdfCraftApp, RightPanel};

mod support;

/// The position before White's 18th move in the spec's scenario (§4.10).
const FEN: &str = "2r2rk1/1p2ppb1/p2p1npp/q4P2/P1b1P2B/2NB4/1PP3PP/R3QR1K w - - 0 18";

/// A run of text on a line: in bold, regular or the figurine font.
#[derive(Clone, Copy)]
enum Font {
    Regular,
    Bold,
    Figurine,
}

use Font::{Bold, Figurine, Regular};

/// The scenario's paragraph as the book sets it, line by line: the game's moves in bold.
const SCENARIO: &[&[(Font, &str)]] = &[
    &[(Bold, "18.Nd5!"), (Regular, " "), (Bold, "Qxe1"), (Regular, " After 18\u{2026}Qd8 19.Bxf6! Bxf6 20.fxg6 (less good is")],
    &[(Regular, "20.Nxf6+, as Black has nothing to fear after 20\u{2026}exf6 21.Bxc4")],
    &[(Regular, "Rxc4) 20\u{2026}fxg6 21.Qg3, White obtains a dangerous initiative. For")],
    &[(Regular, "example: 21\u{2026}Kh7 22.e5!. "), (Bold, "19.Nxe7+ Kh7 20.Raxe1")],
];

/// `text` as a PDF string in WinAnsiEncoding (the ellipsis is 0x85).
fn pdf_string(text: &str) -> String {
    let mut out = String::from("(");
    for c in text.chars() {
        match c {
            '(' | ')' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '\u{2026}' => out.push_str("\\205"),
            c => out.push(c),
        }
    }
    out.push(')');
    out
}

/// A 600 × 800 pt page with `lines` from the top left, 14 pt apart, in 10 pt type. Copying is
/// refused when `locked`.
fn page(lines: &[&[(Font, &str)]], locked: bool) -> Vec<u8> {
    let mut content = String::new();
    for (i, runs) in lines.iter().enumerate() {
        content.push_str(&format!("BT 40 {} Td ", 700 - 14 * i));
        for (font, text) in runs.iter() {
            let f = match font {
                Regular => "F1",
                Bold => "F2",
                Figurine => "F3",
            };
            content.push_str(&format!("/{f} 10 Tf {} Tj ", pdf_string(text)));
        }
        content.push_str("ET\n");
    }
    let widths = vec!["600"; 95].join(" ");
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 600 800] >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /Font << /F1 5 0 R /F2 6 0 R /F3 7 0 R >> >> >>".to_string(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_string(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>".to_string(),
        format!("<< /Type /Font /Subtype /Type1 /BaseFont /SemFig /FirstChar 32 /LastChar 126 /Widths [{widths}] >>"),
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
    if !locked {
        return out;
    }
    let mut doc = pdfcraft_cos::Document::open(std::sync::Arc::new(out)).unwrap();
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
    pdfcraft_cos::write_full(&doc, &Default::default()).unwrap()
}

/// The app on `bytes`, its text layer loaded, the scenario's position on the board.
fn harness_with(bytes: Vec<u8>) -> Harness<'static, PdfCraftApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 1000.0)).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("capture.pdf", None, bytes).unwrap();
        app.set_option("left", "closed").unwrap();
        app.run_inline = true;
        app
    });
    h.run_steps(4);
    support::wait_until(&mut h, "the page's text layer", |app| app.views[0].glyph_screen_pos(0, 0).is_some());
    h.state_mut().chess.board_mut().load_fen(FEN, false, None);
    h.run_steps(2);
    h
}

fn harness() -> Harness<'static, PdfCraftApp> {
    harness_with(page(SCENARIO, false))
}

/// The first glyph of `from` to the last of `to` on page 1, on screen.
fn ends(h: &Harness<'static, PdfCraftApp>, from: &str, to: &str) -> (Pos2, Pos2) {
    let v = &h.state().views[0];
    let a = v.text_matches(0, from).first().unwrap_or_else(|| panic!("{from:?} on the page")).start;
    let b = v.text_matches(0, to).last().unwrap_or_else(|| panic!("{to:?} on the page")).end - 1;
    (v.glyph_screen_pos(0, a).unwrap(), v.glyph_screen_pos(0, b).unwrap())
}

fn drag(h: &mut Harness<'static, PdfCraftApp>, from: Pos2, to: Pos2) {
    h.hover_at(from);
    h.run_steps(1);
    h.drag_at(from);
    h.run_steps(1);
    for k in 1..=4 {
        h.hover_at(from + (to - from) * (k as f32 / 4.0));
        h.run_steps(1);
    }
    h.drop_at(to);
    h.run_steps(3);
}

/// Select from `from` to `to` on page 1 with the mouse.
fn select(h: &mut Harness<'static, PdfCraftApp>, from: &str, to: &str) {
    let (a, b) = ends(h, from, to);
    drag(h, a, b);
}

fn movetext(h: &Harness<'static, PdfCraftApp>) -> String {
    h.state().chess.board().game.movetext()
}

/// What the document's Undo would undo.
fn pdf_undo(h: &Harness<'static, PdfCraftApp>) -> Option<String> {
    let app = h.state();
    app.session.get(app.views[0].id).unwrap().can_undo().map(str::to_owned)
}

fn toast(h: &Harness<'static, PdfCraftApp>) -> String {
    h.state().toast.as_ref().map(|t| t.0.clone()).unwrap_or_default()
}

/// The scenario's whole paragraph, selected with Capture armed: one gesture puts it in, without
/// the preview, as one undo step that Ctrl+Z (the pointer still on the page) takes back while
/// the document stays untouched. The moves keep where they were printed.
#[test]
fn the_paragraph_selected_with_capture_armed_goes_in_at_once() {
    let mut h = harness();
    h.state_mut().show_chess_board();
    h.run_steps(3);
    // Arming from the rail puts the page on Select.
    h.state_mut().quick_tool = pdfcraft_ui_egui::QuickTool::Hand;
    h.get_by_label("Capture (automatic) (X)").click();
    h.run_steps(3);
    assert_eq!(format!("{:?}", h.state().chess.tools.tool), "Capture(Auto)");
    assert_eq!(h.state().quick_tool, pdfcraft_ui_egui::QuickTool::Select);
    h.get_by_label_contains("Capturing");
    select(&mut h, "18.Nd5!", "20.Raxe1");
    assert!(h.state().chess.capture.pending.is_none(), "applied without the preview");
    let pgn = movetext(&h);
    assert!(
        pgn.starts_with("18. Nd5 $1 Qxe1 {After} (18... Qd8 19. Bxf6 $1 Bxf6 20. fxg6 ({less good is} 20. Nxf6+ {as Black has nothing to fear after} 20... exf6 21. Bxc4 Rxc4) 20... fxg6 21. Qg3 {White obtains a dangerous initiative. For example:} 21... Kh7 22. e5 $1) 19. Nxe7+ Kh7 20. Raxe1"),
        "{pgn}"
    );
    let board = h.state().chess.board();
    assert_eq!(board.undo_label().as_deref(), Some("Capture line"));
    assert_eq!(board.game.san(board.game.current()), "Raxe1", "the move shown is the last one grafted");
    // Where 20.Raxe1 was printed: page 1, on the fourth line (its baseline at 658 pt).
    let source = board.game.source(board.game.current()).expect("the move keeps its place on the page");
    assert_eq!(source.page, 0);
    assert!(source.rect_user[1] > 650.0 && source.rect_user[3] < 672.0 && source.rect_user[0] > 150.0, "{source:?}");
    assert!(toast(&h).starts_with("Line from the start · moves: 17"), "{}", toast(&h));
    assert!(h.state().chess.in_scope(), "Undo goes to the game");
    // Ctrl+Z with the pointer on the page takes the whole capture back; the document is untouched.
    let (on_page, _) = ends(&h, "obtains", "obtains");
    h.hover_at(on_page);
    h.run_steps(1);
    h.key_press_modifiers(Modifiers::COMMAND, egui::Key::Z);
    h.run_steps(2);
    assert_eq!(movetext(&h), "");
    assert_eq!(pdf_undo(&h), None, "the document has nothing to undo");
    assert!(!h.state().session.get(h.state().views[0].id).unwrap().dirty);
    // The tool stays armed.
    assert_eq!(format!("{:?}", h.state().chess.tools.tool), "Capture(Auto)");
}

/// Forced modes: a variation of the main line from the middle of a line of prose (the selection
/// grows to whole tokens), then a comment on the move shown, after it and then in place of it.
#[test]
fn forced_variation_and_comment_captures() {
    let mut h = harness();
    {
        let board = h.state_mut().chess.board_mut();
        board.play_uci_line(&["c3d5".into(), "a5e1".into()]);
    }
    assert_eq!(movetext(&h), "18. Nd5 Qxe1");
    assert!(h.state_mut().execute("chess.tool.capture_variation"));
    h.run_steps(2);
    // "Qd8 … 20.fx": the ends grow to "18…Qd8" and "20.fxg6".
    let v = &h.state().views[0];
    let a = v.text_matches(0, "Qd8")[0].start;
    let b = v.text_matches(0, "20.fxg6")[0].start + 4;
    let (a, b) = (v.glyph_screen_pos(0, a).unwrap(), v.glyph_screen_pos(0, b).unwrap());
    drag(&mut h, a, b);
    assert_eq!(movetext(&h), "18. Nd5 Qxe1 (18... Qd8 19. Bxf6 $1 Bxf6 20. fxg6)");
    assert_eq!(h.state().chess.board().undo_label().as_deref(), Some("Capture variation"));
    assert!(toast(&h).starts_with("Variation of 18…Qxe1 · moves: 4"), "{}", toast(&h));

    // Comment, after the move shown (20.fxg6).
    assert!(h.state_mut().execute("chess.tool.comment"));
    h.run_steps(2);
    select(&mut h, "White", "initiative.");
    let game = &h.state().chess.board().game;
    assert_eq!(game.comment(game.current()), "White obtains a dangerous initiative.");
    assert_eq!(h.state().chess.board().undo_label().as_deref(), Some("Capture comment"));
    // The chip's switch: in place of the comment.
    h.get_by_label("replace the comment").click();
    h.run_steps(2);
    select(&mut h, "example:", "example:");
    let game = &h.state().chess.board().game;
    assert_eq!(game.comment(game.current()), "example:");
    // Esc stops capturing.
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert_eq!(format!("{:?}", h.state().chess.tools.tool), "Navigate");
}

/// Send to PGN ▸ in the selection's action bar captures without arming a tool; an empty
/// selection captures nothing.
#[test]
fn send_to_pgn_works_without_arming() {
    let mut h = harness();
    // Nothing selected: nothing happens, and nothing is said.
    h.state_mut().capture_selection(0, Some((pdfcraft_engine::chess::capture::CaptureMode::Line, false)));
    h.run_steps(2);
    assert_eq!(movetext(&h), "");
    assert_eq!(toast(&h), "");
    select(&mut h, "18.Nd5!", "Qxe1");
    h.get_by_label("Send to PGN").click();
    h.run_steps(2);
    h.get_by_label("Capture as the line").click();
    h.run_steps(3);
    assert_eq!(movetext(&h), "18. Nd5 $1 Qxe1");
    assert_eq!(format!("{:?}", h.state().chess.tools.tool), "Navigate", "nothing was armed");
}

/// A document that does not allow copying refuses the capture with the usual notice.
#[test]
fn a_document_that_forbids_copying_refuses_the_capture() {
    let mut h = harness_with(page(SCENARIO, true));
    assert!(h.state_mut().execute("chess.tool.capture"));
    h.run_steps(2);
    select(&mut h, "18.Nd5!", "Qxe1");
    assert_eq!(movetext(&h), "", "nothing was captured");
    assert_eq!(toast(&h), "The document's security settings don't allow copying text and images");
}

/// Send `events` as the desktop app gets them: the frame's raw input goes through
/// `raw_input_hook` first (kittest itself skips the hook).
fn host_events(h: &mut Harness<'static, PdfCraftApp>, events: Vec<egui::Event>) {
    use eframe::App as _;
    let ctx = h.ctx.clone();
    let mut raw = std::mem::take(h.input_mut());
    raw.events.extend(events);
    h.state_mut().raw_input_hook(&ctx, &mut raw);
    *h.input_mut() = raw;
    h.step();
    h.run_steps(2);
}

/// Text pasted with the chess scope on is captured; text over 64 KiB is refused with a notice.
#[test]
fn pasted_text_is_captured() {
    let mut h = harness();
    h.state_mut().show_chess_board();
    h.run_steps(3);
    assert!(h.state_mut().execute("chess.tool.capture"));
    h.run_steps(2);
    assert!(h.state().chess.in_scope());
    host_events(&mut h, vec![egui::Event::Paste("18.Nd5 Qxe1 19.Nxe7+".into())]);
    assert_eq!(movetext(&h), "18. Nd5 Qxe1 19. Nxe7+");
    host_events(&mut h, vec![egui::Event::Paste("x".repeat(64 * 1024 + 1))]);
    assert_eq!(toast(&h), "Too much text to capture at once: at most 64 KiB.");
    assert_eq!(movetext(&h), "18. Nd5 Qxe1 19. Nxe7+");
    // Paste as ▸ a variation (the clipboard's text handed in, as the desktop reads it).
    h.state_mut().chess_paste_as((pdfcraft_engine::chess::capture::CaptureMode::Variation, false), Some("19.Bxf6".into()));
    h.run_steps(2);
    assert_eq!(movetext(&h), "18. Nd5 Qxe1 19. Nxe7+ (19. Bxf6)");
}

/// A plan waiting for the user (it extends the end of the main line) is made again when the game
/// changed before it was applied.
#[test]
fn a_stale_plan_is_planned_again() {
    let mut h = harness();
    h.state_mut().chess.board_mut().play_uci_line(&["c3d5".into(), "a5e1".into()]);
    h.state_mut().capture_pasted("19.Nxe7+ Kh7", Some((pdfcraft_engine::chess::capture::CaptureMode::Variation, false)));
    h.run_steps(2);
    assert!(h.state().chess.capture.pending.is_some(), "the end of the line is extended: shown first");
    h.get_by_label_contains("Check this capture");
    h.get_by_label_contains("It extends the end of a line instead of branching from it.");
    // Meanwhile the game changes.
    h.state_mut().chess.board_mut().game.set_comment(0, "edited meanwhile");
    h.get_by_label("Apply").click();
    h.run_steps(3);
    // Planned again on the game as it is now: it still asks.
    assert!(h.state().chess.capture.pending.is_some(), "planned again, and shown again");
    h.get_by_label("Apply").click();
    h.run_steps(3);
    assert_eq!(movetext(&h), "{edited meanwhile} 18. Nd5 Qxe1 19. Nxe7+ Kh7");
    // Esc closes a plan waiting, and changes nothing.
    h.state_mut().capture_pasted("20.Raxe1 Qa5", Some((pdfcraft_engine::chess::capture::CaptureMode::Line, false)));
    h.run_steps(2);
    assert!(h.state().chess.capture.pending.is_some(), "Black has no queen left for 20...Qa5: shown first");
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert!(h.state().chess.capture.pending.is_none());
    assert_eq!(movetext(&h), "{edited meanwhile} 18. Nd5 Qxe1 19. Nxe7+ Kh7");
}

/// Figurines in a catalogued figurine font (never embedded) and Portuguese piece letters, the
/// document's language as the Text tab read it.
#[test]
fn figurines_and_the_documents_language_are_read() {
    let lines: &[&[(Font, &str)]] = &[
        &[(Bold, "18."), (Figurine, "N"), (Bold, "d5! Dxe1"), (Regular, " e depois 18\u{2026}Dd8 19.Bxf6! Bxf6 20.fxg6 com iniciativa.")],
        &[(Regular, "Na partida: 19.Cxe7+ Rh7 20.Taxe1 Tc7 e 21.Bxc4 Txc4.")],
    ];
    let mut h = harness_with(page(lines, false));
    h.state_mut().choose_right_panel(Some(RightPanel::Chess));
    h.run_steps(2);
    h.get_by_label("Text").click();
    h.run_steps(4);
    let lang = h.state().chess.text.as_ref().map(|t| t.chess.pages[0].lang.clone());
    assert_eq!(lang.as_deref(), Some("pt"), "the page reads as Portuguese");
    assert!(h.state_mut().execute("chess.tool.capture"));
    h.run_steps(2);
    select(&mut h, "18.", "Dxe1");
    assert_eq!(movetext(&h), "18. Nd5 $1 Qxe1");
}
