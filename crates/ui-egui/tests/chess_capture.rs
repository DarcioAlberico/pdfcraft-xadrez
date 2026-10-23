//! Chess ▸ PGN toolbar ▸ Capture (T7a) in the real shell (egui_kittest): text selected on a page,
//! or pasted, goes into the game on the chess board. The PDFs are built here: Helvetica for the
//! prose and Helvetica-Bold for the game's own moves, as books set them; figurines in SemFig, a
//! catalogued figurine font, referred to by its `/BaseFont` name and never embedded.

use egui::{Modifiers, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
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

/// The capture's own notice (what went in, with Undo).
fn capture_toast(h: &Harness<'static, PdfCraftApp>) -> String {
    h.state().chess.capture.toast.as_ref().map(|t| t.text.clone()).unwrap_or_default()
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
    assert_eq!(capture_toast(&h), "Line from the start · moves: 17");
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
    assert_eq!(capture_toast(&h), "Variation of 18…Qxe1 · moves: 4");

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
    h.get_by_label_contains("It extends the end of a line instead of branching from it.");
    // Meanwhile the game changes.
    h.state_mut().chess.board_mut().game.set_comment(0, "edited meanwhile");
    h.get_by_label_contains("Apply").click();
    h.run_steps(3);
    // Planned again on the game as it is now: it still asks.
    assert!(h.state().chess.capture.pending.is_some(), "planned again, and shown again");
    h.get_by_label_contains("Apply").click();
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

// ─── T7b: the preview, the notice and the layer of captured text ────────────────────────────────

use pdfcraft_engine::chess::capture::CaptureMode;

/// The scenario's game: `moves` (SAN) played from the move shown, the last one shown.
fn play(h: &mut Harness<'static, PdfCraftApp>, moves: &[&str]) {
    let board = h.state_mut().chess.board_mut();
    for m in moves {
        board.game.play_san(m).unwrap();
    }
    h.run_steps(2);
}

/// An ambiguous capture opens the preview: it names the place, offers the other one in Fit at,
/// and Enter applies the place chosen.
#[test]
fn the_preview_offers_each_place_and_applies_the_one_chosen() {
    let mut h = harness();
    play(&mut h, &["Nd5", "Qxe1", "Nxe7+", "Kh7", "Raxe1"]);
    {
        // 19...Kh8 beside 19...Kh7: 20.Rfxe1 fits after either.
        let game = &mut h.state_mut().chess.board_mut().game;
        let nxe7 = game.node_at(&pdfcraft_engine::chess::pdfcraft_chess::game::NodePath::new(vec![0, 0, 0])).unwrap();
        game.go_to_node(nxe7);
        game.play_san("Kh8").unwrap();
    }
    h.state_mut().capture_pasted("20.Rfxe1", Some((CaptureMode::SubVariation, false)));
    h.run_steps(2);
    let pending = h.state().chess.capture.pending.as_ref().expect("an ambiguous capture is shown first");
    assert_eq!(pending.plan.candidates.len(), 2);
    h.get_by_label_contains("It fits in more than one place.");
    h.get_by_label("Fit at");
    // The place shown is the inner one; choose the other in Fit at.
    let fit = h
        .query_all_by(|n| n.role() == egui::accesskit::Role::ComboBox && n.value().as_deref() == Some("Line after 19…Kh8"))
        .next()
        .expect("Fit at lists the places");
    fit.click();
    h.run_steps(2);
    h.get_by_label("Variation of 20.Raxe1 · moves: 1").click();
    h.run_steps(2);
    assert_eq!(h.state().chess.capture.pending.as_ref().and_then(|p| p.choice), Some(1));
    // Enter applies it (taken before anything else sees it).
    host_events(
        &mut h,
        vec![egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }],
    );
    assert!(h.state().chess.capture.pending.is_none());
    assert_eq!(movetext(&h), "18. Nd5 Qxe1 19. Nxe7+ Kh7 (19... Kh8) 20. Raxe1 (20. Rfxe1)");
}

/// The preview of a page selection sits beside it; Esc closes it (the tool stays armed), and so
/// does an edit to the document.
#[test]
fn the_preview_sits_beside_the_selection_and_closes_on_esc_or_an_edit() {
    let mut h = harness();
    play(&mut h, &["Nd5", "Qxe1"]);
    assert!(h.state_mut().execute("chess.tool.capture_line"));
    h.run_steps(2);
    // 18...Qd8 as the line demotes 18...Qxe1: shown first.
    select(&mut h, "18\u{2026}Qd8", "19.Bxf6!");
    assert!(h.state().chess.capture.pending.is_some());
    h.get_by_label_contains("A move already there becomes a variation.");
    let (_, end) = ends(&h, "18\u{2026}Qd8", "19.Bxf6!");
    let apply = h.get_by_label_contains("Apply").rect();
    assert!(apply.top() > end.y && apply.top() < end.y + 260.0, "the preview is under the selection: {apply:?} vs {end:?}");
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert!(h.state().chess.capture.pending.is_none(), "Esc closed it");
    assert_eq!(movetext(&h), "18. Nd5 Qxe1");
    assert_eq!(format!("{:?}", h.state().chess.tools.tool), "Capture(Line)", "the tool stays armed");
    // Again, then the document is edited: the preview of a page that changed closes.
    select(&mut h, "18\u{2026}Qd8", "19.Bxf6!");
    assert!(h.state().chess.capture.pending.is_some());
    let shape = pdfcraft_engine::Shape::Rectangle { rect: [10.0, 10.0, 50.0, 50.0] };
    let style = pdfcraft_engine::Style::default_for(&shape);
    let annotation = pdfcraft_engine::NewAnnotation { page: 0, style, shape, contents: String::new(), author: String::new() };
    assert!(h.state_mut().apply_edit(pdfcraft_engine::Edit::AddAnnotation(annotation)));
    h.run_steps(3);
    assert!(h.state().chess.capture.pending.is_none(), "closed: the page was edited");
    assert_eq!(toast(&h), "The capture was closed: the document was edited.");
    assert_eq!(movetext(&h), "18. Nd5 Qxe1");
}

/// Apply and keep the rest as a comment: what was not read goes on the last move put in.
#[test]
fn the_rest_not_read_can_be_kept_as_a_comment() {
    let mut h = harness();
    play(&mut h, &["Nd5", "Qxe1"]);
    h.state_mut().capture_pasted("18...Qd8 19.Bxf6 1-0 White won much later.", Some((CaptureMode::Variation, false)));
    h.run_steps(2);
    assert!(h.state().chess.capture.pending.is_some(), "text after the result: shown first");
    h.get_by_label("Apply and keep the rest as a comment").click();
    h.run_steps(3);
    let game = &h.state().chess.board().game;
    assert_eq!(game.san(game.current()), "Bxf6");
    assert!(game.comment(game.current()).contains("White won much later."), "{}", game.movetext());
    assert_eq!(h.state().chess.board().undo_label().as_deref(), Some("Capture variation"), "one step");
}

/// After a capture goes in, its notice says what it did and its Undo takes it back; the new
/// moves are marked in the notation until the game changes again.
#[test]
fn the_notice_undoes_the_capture_and_the_new_moves_are_marked() {
    let mut h = harness();
    h.state_mut().show_chess_board();
    h.run_steps(3);
    h.state_mut().capture_pasted("18.Nd5 Qxe1 19.Nxe7+", None);
    h.run_steps(2);
    assert_eq!(capture_toast(&h), "Line from the start · moves: 3");
    let board = h.state().chess.board();
    assert_eq!(board.fresh.nodes(&board.game).len(), 3);
    h.get_by_label("Undo this capture").click();
    h.run_steps(3);
    assert_eq!(movetext(&h), "");
    assert!(h.state().chess.capture.toast.is_none());
    // Captured again, then the game changes: the notice and the marks go.
    h.state_mut().capture_pasted("18.Nd5 Qxe1", None);
    h.run_steps(2);
    assert!(h.state().chess.capture.toast.is_some());
    play(&mut h, &["Nxe7+"]);
    assert!(h.state().chess.capture.toast.is_none(), "until the next change");
    let board = h.state().chess.board();
    assert!(board.fresh.nodes(&board.game).is_empty());
}

/// The layer of captured text underlines, on the page, each move captured from it, and only
/// while it is on.
#[test]
fn the_captured_text_layer_underlines_each_captured_move() {
    let mut h = harness();
    h.state_mut().show_chess_board();
    h.run_steps(3);
    assert!(h.state_mut().execute("chess.tool.capture"));
    h.run_steps(2);
    select(&mut h, "18.Nd5!", "20.Raxe1");
    assert_eq!(capture_toast(&h), "Line from the start · moves: 17");
    assert!(h.state().chess.capture.drawn.is_empty(), "off by default");
    h.get_by_label("Underline captured text on the page").click();
    h.run_steps(3);
    let drawn = h.state().chess.capture.drawn.clone();
    assert_eq!(drawn.len(), 17, "one mark per captured move: {drawn:?}");
    assert!(drawn.iter().all(|(page, r)| *page == 0 && r[1] > 640.0 && r[3] < 715.0), "{drawn:?}");
    h.get_by_label("Underline captured text on the page").click();
    h.run_steps(3);
    assert!(h.state().chess.capture.drawn.is_empty());
}

/// With the board panel's rail on screen and the chess scope on, Enter applies the capture
/// waiting (not the rail's Move comment, whose key it also is), and Esc closes a waiting capture
/// before it disarms the tool.
#[test]
fn enter_and_esc_go_to_a_waiting_capture_first() {
    let key = |key| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
    let mut h = harness();
    h.state_mut().show_chess_board();
    h.run_steps(3);
    play(&mut h, &["Nd5", "Qxe1"]);
    assert!(h.state_mut().execute("chess.tool.capture_line"));
    h.run_steps(2);
    h.state_mut().capture_pasted("18...Qd8 19.Bxf6", None);
    h.run_steps(2);
    assert!(h.state().chess.capture.pending.is_some(), "demotes 18...Qxe1: shown first");
    assert!(h.state().chess.in_scope());
    // Esc: the waiting capture closes; the tool stays armed.
    host_events(&mut h, vec![key(egui::Key::Escape)]);
    assert!(h.state().chess.capture.pending.is_none());
    assert_eq!(format!("{:?}", h.state().chess.tools.tool), "Capture(Line)");
    // Again, and Enter applies it.
    h.state_mut().capture_pasted("18...Qd8 19.Bxf6", None);
    h.run_steps(2);
    host_events(&mut h, vec![key(egui::Key::Enter)]);
    assert!(h.state().chess.capture.pending.is_none());
    assert_eq!(movetext(&h), "18. Nd5 Qd8 (18... Qxe1) 19. Bxf6");
    // Esc again disarms.
    host_events(&mut h, vec![key(egui::Key::Escape)]);
    assert_eq!(format!("{:?}", h.state().chess.tools.tool), "Navigate");
}

// ─── T10: correcting a move that did not play ───────────────────────────────────────────────

/// The candidate shown in the preview: moves played, and the moves that did not play.
fn shown(h: &Harness<'static, PdfCraftApp>) -> (usize, usize) {
    let p = h.state().chess.capture.pending.as_ref().expect("a capture waits");
    let c = p.plan.candidates.get(p.choice.expect("a place is chosen")).expect("the candidate");
    (c.played, c.read.stops.len())
}

/// A stopped move's ✗ chip lists the legal moves near it; none goes in by itself: the game is
/// unchanged until one is clicked, and then the text is read again and waits for Apply.
#[test]
fn a_stopped_move_offers_legal_moves_that_go_in_only_when_clicked() {
    let mut h = harness();
    h.state_mut().capture_pasted("18.Nd5 Qxe1 19.Nxe6+ Kh7 20.Raxe1", None);
    h.run_steps(2);
    assert_eq!(shown(&h).0, 2, "19.Nxe6+ is illegal: the reading stops there");
    h.get_by_label("✗ 19.Nxe6+ — Illegal move").click();
    h.run_steps(2);
    h.get_by_label_contains("Correct 19.Nxe6+");
    h.get_by_label("Read as Nxe7+");
    h.get_by_label("Read as Nxf6+");
    h.run_steps(3);
    assert_eq!(movetext(&h), "", "nothing is applied without a click");
    assert_eq!(shown(&h).0, 2, "nor is the text read again with one");
    h.get_by_label("Read as Nxe7+").click();
    h.run_steps(3);
    assert_eq!(shown(&h), (5, 0), "read again with 19.Nxe7+: every move plays");
    assert_eq!(movetext(&h), "", "and still waits for Apply");
    h.get_by_label_contains("Apply").click();
    h.run_steps(3);
    assert_eq!(movetext(&h), "18. Nd5 Qxe1 19. Nxe7+ Kh7 20. Raxe1");
    assert!(h.state().chess.board().undo_label().is_some_and(|l| l.starts_with("Capture")), "one capture step");
}

/// The correction field holds the move as printed; the glyph palette inserts at its cursor, and
/// Enter reads the text again with it.
#[test]
fn the_correction_field_takes_the_glyph_palette() {
    let mut h = harness();
    h.state_mut().capture_pasted("18.Nd5 Qxe1 19.Nxe6+ Kh7", None);
    h.run_steps(2);
    h.get_by_label("✗ 19.Nxe6+ — Illegal move").click();
    h.run_steps(2);
    let field = h.get_by_label("Correction");
    assert_eq!(field.value().as_deref(), Some("Nxe6+"), "the move as printed");
    field.click();
    h.run_steps(1);
    h.key_press(egui::Key::End);
    for _ in 0..5 {
        h.key_press(egui::Key::Backspace);
    }
    h.run_steps(1);
    h.get_by_label("Glyph palette").click();
    h.run_steps(2);
    h.get_by_label("White knight").click();
    h.run_steps(2);
    assert_eq!(h.get_by_label("Correction").value().as_deref(), Some("♘"));
    h.get_by_label("Correction").type_text("xe7+");
    h.run_steps(1);
    assert_eq!(h.get_by_label("Correction").value().as_deref(), Some("♘xe7+"));
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert_eq!(shown(&h), (4, 0), "read again with ♘xe7+");
    assert_eq!(movetext(&h), "");
    h.get_by_label_contains("Apply").click();
    h.run_steps(3);
    assert_eq!(movetext(&h), "18. Nd5 Qxe1 19. Nxe7+ Kh7");
}

/// A selection on a page is corrected in its glyphs: the move keeps where it was printed and its
/// weight (a bold move stays on the main line).
#[test]
fn a_page_selection_is_corrected_in_its_glyphs() {
    let mut h = harness_with(page(&[&[(Bold, "18.Nd5 Qxe1 19.Nxe6+ Kh7"), (Regular, " and Black holds.")]], false));
    assert!(h.state_mut().execute("chess.tool.capture"));
    h.run_steps(2);
    select(&mut h, "18.Nd5", "Kh7");
    assert_eq!(shown(&h).0, 2);
    h.get_by_label("✗ 19.Nxe6+ — Illegal move").click();
    h.run_steps(2);
    h.get_by_label("Read as Nxe7+").click();
    h.run_steps(3);
    assert_eq!(shown(&h), (4, 0));
    let p = h.state().chess.capture.pending.as_ref().unwrap();
    let c = p.plan.candidates.get(p.choice.unwrap()).unwrap();
    let bold: Vec<Option<bool>> = c
        .read
        .items
        .iter()
        .filter_map(|i| match i {
            pdfcraft_engine::chess::pdfcraft_chess::fragment::FragmentItem::Move { bold, .. } => Some(*bold),
            _ => None,
        })
        .collect();
    assert_eq!(bold, [Some(true); 4], "the corrected move keeps its weight");
    h.get_by_label_contains("Apply").click();
    h.run_steps(3);
    assert_eq!(movetext(&h), "18. Nd5 Qxe1 19. Nxe7+ Kh7");
    let game = &h.state().chess.board().game;
    let nxe7 = game.node_at(&pdfcraft_engine::chess::pdfcraft_chess::game::NodePath::new(vec![0, 0, 0])).unwrap();
    let source = game.source(nxe7).expect("the corrected move keeps its place on the page");
    assert_eq!(source.page, 0);
}

/// Renumber is a choice of its own beside the place: the place stays as chosen, the choice plays
/// the moves from where a move without a number goes with the game's numbers and says what
/// changes; switched off it goes back to the place; applied, it is one undo step.
#[test]
fn the_preview_offers_renumbering_as_its_own_choice() {
    let mut h = harness();
    play(&mut h, &["Nd5", "Qxe1", "Nxe7+", "Kh7", "Raxe1"]);
    // 19…Kh8 is an alternative to 19…Kh7 by its number, and 20…Kh8 renumbered; 20.Nxe6 plays in
    // neither, so the preview is shown.
    h.state_mut().capture_pasted("19…Kh8 20.Nxe6", Some((CaptureMode::SubVariation, false)));
    h.run_steps(2);
    let p = h.state().chess.capture.pending.as_ref().expect("an incomplete capture is shown first");
    let (place, renumber) = (p.plan.chosen, p.plan.renumber);
    assert!(renumber.is_some() && place != renumber, "a place, and Renumber beside it");
    assert_eq!(p.choice, place);
    h.get_by_label("Variation of 19…Kh7");
    assert!(h.query_by_label("Fit at").is_none(), "Renumber is not a place to fit at");
    assert!(h.query_by_label_contains("is played as").is_none());
    h.get_by_label("Renumber").click();
    // The popover settles its new width (it is pinned by its right edge).
    h.run_steps(4);
    assert_eq!(h.state().chess.capture.pending.as_ref().and_then(|p| p.choice), renumber);
    h.get_by_label("Line after 20.Raxe1");
    h.get_by_label("• The text's move numbers give way to the game's: 19...Kh8 is played as 20...Kh8.");
    h.get_by_label("✓ 20...Kh8");
    // Off again: back to the place.
    h.get_by_label("Renumber").click();
    // The popover settles its new width (it is pinned by its right edge).
    h.run_steps(4);
    assert_eq!(h.state().chess.capture.pending.as_ref().and_then(|p| p.choice), place);
    h.get_by_label("Renumber").click();
    // The popover settles its new width (it is pinned by its right edge).
    h.run_steps(4);
    assert_eq!(movetext(&h), "18. Nd5 Qxe1 19. Nxe7+ Kh7 20. Raxe1", "nothing goes in before Apply");
    // A correction plans the text again, and Renumber stays chosen.
    h.get_by_label("✗ 21.Nxe6 — Illegal move").click();
    h.run_steps(2);
    let field = h.get_by_label("Correction");
    field.click();
    h.run_steps(1);
    h.key_press(egui::Key::End);
    for _ in 0..4 {
        h.key_press(egui::Key::Backspace);
    }
    h.get_by_label("Correction").type_text("Nxg6+");
    h.run_steps(1);
    h.key_press(egui::Key::Enter);
    h.run_steps(4);
    let p = h.state().chess.capture.pending.as_ref().expect("the corrected text waits");
    assert_eq!(p.choice, p.plan.renumber, "still renumbered");
    assert_eq!(shown(&h), (2, 0));
    host_events(
        &mut h,
        vec![egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }],
    );
    assert!(h.state().chess.capture.pending.is_none());
    assert_eq!(movetext(&h), "18. Nd5 Qxe1 19. Nxe7+ Kh7 20. Raxe1 Kh8 21. Nxg6+");
    assert!(h.state().chess.board().undo_label().is_some_and(|l| l.starts_with("Capture")), "one capture step");
    assert!(h.state_mut().chess.board_mut().undo());
    assert_eq!(movetext(&h), "18. Nd5 Qxe1 19. Nxe7+ Kh7 20. Raxe1");
}

/// The glyph palette's window, if it is open.
fn palette(h: &Harness<'static, PdfCraftApp>) -> Option<egui::Rect> {
    h.query_by_role_and_label(egui::accesskit::Role::Window, "Glyph palette").map(|w| w.rect())
}

/// The button named `label` inside the palette's window.
fn in_palette<'a>(h: &'a Harness<'static, PdfCraftApp>, label: &'a str) -> egui_kittest::Node<'a> {
    let window = palette(h).expect("the palette is open");
    h.get_all_by_label(label)
        .find(|n| n.accesskit_node().role() == egui::accesskit::Role::Button && window.contains(n.rect().center()))
        .unwrap_or_else(|| panic!("{label} in the palette"))
}

/// The board panel's floating glyph palette types into the correction field while it has the
/// keyboard, a figurine and a glyph's sign alike, instead of annotating the move shown.
#[test]
fn the_board_panels_glyph_palette_types_into_the_correction_field() {
    let mut h = harness();
    h.state_mut().show_chess_board();
    h.run_steps(3);
    assert!(h.state_mut().execute("chess.palette.toggle"));
    h.run_steps(3);
    assert!(palette(&h).is_some(), "the floating palette is open");
    h.state_mut().capture_pasted("18.Nd5 Qxe1 19.Nxe6+ Kh7", None);
    h.run_steps(2);
    h.get_by_label("✗ 19.Nxe6+ — Illegal move").click();
    h.run_steps(2);
    h.get_by_label("Correction").click();
    h.run_steps(1);
    h.key_press(egui::Key::End);
    for _ in 0..5 {
        h.key_press(egui::Key::Backspace);
    }
    h.run_steps(1);
    in_palette(&h, "White knight").click();
    h.run_steps(3);
    assert_eq!(h.get_by_label("Correction").value().as_deref(), Some("♘"));
    h.get_by_label("Correction").type_text("xe7+");
    h.run_steps(1);
    in_palette(&h, "Good move").click();
    h.run_steps(3);
    assert_eq!(h.get_by_label("Correction").value().as_deref(), Some("♘xe7+!"));
    assert_eq!(movetext(&h), "", "nothing went on the game: no glyph, no comment");
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert_eq!(shown(&h), (4, 0), "read again with ♘xe7+!");
    h.get_by_label_contains("Apply").click();
    h.run_steps(3);
    assert_eq!(movetext(&h), "18. Nd5 Qxe1 19. Nxe7+ $1 Kh7");
}
