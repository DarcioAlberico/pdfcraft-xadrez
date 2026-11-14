//! Plan A9-I18 in the real shell (egui_kittest): the chess keys follow the keyboard focus and can
//! be left from the keyboard (finding R5-01), the one-key shortcuts can be turned off (WCAG 2.1.4),
//! a typed sign waits for Enter when asked to (R5-11), and a newer build's preferences survive
//! this one (R7-05). Keys go through `raw_input_hook`, as in the desktop app.

use egui::accesskit::Role;
use egui::{Key, Modifiers, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use pdfcraft_engine::chess::pdfcraft_chess::game::{Game, NodePath};
use pdfcraft_ui_egui::PdfCraftApp;

/// A three-page document, for turning pages.
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

/// The app on [`three_pages`], the Board panel shown, the game 1. e4 e5 2. Nf3 at 1... e5.
fn harness() -> Harness<'static, PdfCraftApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 1000.0)).build_eframe(|_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("pages.pdf", None, three_pages()).unwrap();
        app.run_inline = true;
        app
    });
    h.run_steps(4);
    h.state_mut().show_chess_board();
    h.run_steps(3);
    let (mut game, stopped) = Game::from_pgn("1. e4 e5 2. Nf3 *").unwrap();
    assert_eq!(stopped, None);
    game.go_to_node(game.node_at(&NodePath::new(vec![0, 0])).unwrap());
    h.state_mut().chess.board_mut().game = game;
    h.run_steps(2);
    h
}

/// The move shown, as its SAN.
fn shown(h: &Harness<'static, PdfCraftApp>) -> String {
    let game = &h.state().chess.board().game;
    game.san(game.current()).to_owned()
}

/// The page shown (0-based).
fn page(h: &Harness<'static, PdfCraftApp>) -> usize {
    h.state().views[0].current
}

/// Press `key` with `modifiers` as the desktop app gets it: the frame's raw input goes through
/// `raw_input_hook` first, where the chess scope takes its keys (kittest itself skips the hook).
fn host_key(h: &mut Harness<'static, PdfCraftApp>, modifiers: Modifiers, key: Key) {
    use eframe::App as _;
    let ctx = h.ctx.clone();
    for pressed in [true, false] {
        let mut raw = std::mem::take(h.input_mut());
        raw.events.push(egui::Event::ModifiersChanged(if pressed { modifiers } else { Modifiers::NONE }));
        raw.events.push(egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers });
        h.state_mut().raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }
    h.run_steps(2);
}

/// Type `text` as the desktop app gets it, through `raw_input_hook`. One frame.
fn host_text(h: &mut Harness<'static, PdfCraftApp>, text: &str) {
    use eframe::App as _;
    let ctx = h.ctx.clone();
    let mut raw = std::mem::take(h.input_mut());
    raw.events.push(egui::Event::Text(text.to_owned()));
    h.state_mut().raw_input_hook(&ctx, &mut raw);
    *h.input_mut() = raw;
    h.step();
}

fn click_at(h: &mut Harness<'static, PdfCraftApp>, at: Pos2) {
    h.hover_at(at);
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    }
    h.run_steps(2);
}

/// Where the Board tab's header says whose move it is (plan A9-I0b).
fn board_label(h: &Harness<'static, PdfCraftApp>) -> egui::Rect {
    h.get_all_by_label_contains(" to move").find(|n| n.accesskit_node().role() == Role::Label).expect("the Board tab's header").rect()
}

/// A click on a label of the Board panel: the chess scope goes on, and nothing takes the focus.
fn click_board_label(h: &mut Harness<'static, PdfCraftApp>) {
    let at = board_label(h).center();
    click_at(h, at);
    assert!(h.state().chess.in_scope());
    assert!(!h.ctx.egui_wants_keyboard_input(), "nothing has the keyboard");
}

fn tool(h: &Harness<'static, PdfCraftApp>) -> String {
    format!("{:?}", h.state().chess.tools.tool)
}

/// Whether the comment editor is open: the rail's Move comment shows it.
fn comment_open(h: &Harness<'static, PdfCraftApp>) -> bool {
    h.get_by_label_contains("Move comment (").accesskit_node().toggled() == Some(egui::accesskit::Toggled::True)
}

/// The glyphs of the move shown.
fn glyphs(h: &Harness<'static, PdfCraftApp>) -> Vec<u16> {
    let game = &h.state().chess.board().game;
    game.nags(game.current()).to_vec()
}

/// With the chess scope on and the panels' divider focused, ← moves the divider and not the
/// game: the divider's keys are its own (R5-01).
#[test]
fn the_divider_keeps_its_arrows_with_the_chess_scope_on() {
    let mut h = harness();
    click_board_label(&mut h);
    let label = board_label(&h);
    // The divider on the Board panel's left, between it and the page.
    let divider = h
        .get_all_by_label("Resize panels")
        .filter(|n| n.rect().center().x < label.left())
        .max_by(|a, b| a.rect().center().x.total_cmp(&b.rect().center().x))
        .expect("the divider left of the board");
    divider.focus();
    h.run_steps(2);
    host_key(&mut h, Modifiers::NONE, Key::ArrowLeft);
    assert_eq!(shown(&h), "e5", "the game did not move");
    assert!(board_label(&h).left() < label.left() - 1.0, "the divider moved left: {:?} then {:?}", label, board_label(&h));
    assert!(!h.state().chess.in_scope(), "the keyboard is on the divider, outside the chess panels");
}

/// Tab takes the keyboard out of the chess panel, and Esc out of the control it went to: → turns
/// the page again, and the game stays where it was (R5-01; WCAG 2.1.2).
#[test]
fn tab_out_of_the_chess_panel_gives_the_arrows_back_to_the_page() {
    let mut h = harness();
    click_board_label(&mut h);
    assert_eq!(page(&h), 0);
    host_key(&mut h, Modifiers::NONE, Key::Tab);
    assert!(h.ctx.egui_wants_keyboard_input(), "Tab gave a control the keyboard");
    let out = h.state().chess.in_scope();
    host_key(&mut h, Modifiers::NONE, Key::Escape);
    assert!(!h.ctx.egui_wants_keyboard_input(), "Esc let it go");
    host_key(&mut h, Modifiers::NONE, Key::ArrowRight);
    assert_eq!(shown(&h), "e5", "the game did not move");
    assert_eq!(page(&h), 1, "→ turned the page");
    assert!(!out, "Tab took the keyboard out of the chess panels");
}

/// F6 leaves the chess panels at once, from a click there or from a control of theirs with the
/// keyboard; → is the page's again.
#[test]
fn f6_leaves_the_chess_panels() {
    let mut h = harness();
    click_board_label(&mut h);
    host_key(&mut h, Modifiers::NONE, Key::F6);
    assert!(!h.state().chess.in_scope());
    host_key(&mut h, Modifiers::NONE, Key::ArrowRight);
    assert_eq!((page(&h), shown(&h).as_str()), (1, "e5"), "the page turned, the game stayed");
    // The keyboard on a rail button puts the scope on; F6 lets the button go and leaves.
    h.get_by_label_contains("Glyph brush (").focus();
    h.run_steps(2);
    assert!(h.state().chess.in_scope(), "the keyboard in the rail is in the chess panels");
    host_key(&mut h, Modifiers::NONE, Key::F6);
    assert!(!h.ctx.egui_wants_keyboard_input(), "nothing has the keyboard");
    assert!(!h.state().chess.in_scope());
    host_key(&mut h, Modifiers::NONE, Key::ArrowLeft);
    assert_eq!((page(&h), shown(&h).as_str()), (0, "e5"));
}

/// A click in another panel gives the keys and Undo back to the document, as a click on the page
/// does.
#[test]
fn a_click_in_the_tools_panel_ends_the_chess_scope() {
    let mut h = harness();
    click_board_label(&mut h);
    // The Tools panel's margin, at the board's height: no control there.
    let y = board_label(&h).center().y;
    click_at(&mut h, egui::pos2(4.0, y));
    assert!(!h.state().chess.in_scope());
    host_key(&mut h, Modifiers::NONE, Key::ArrowRight);
    assert_eq!((page(&h), shown(&h).as_str()), (1, "e5"));
}

/// Preferences ▸ One-key shortcuts off: B arms no brush and a sign typed puts no glyph, while the
/// rail's button, Esc and the arrows still act (WCAG 2.1.4).
#[test]
fn one_key_shortcuts_off_leave_letters_and_signs_alone() {
    let mut h = harness();
    h.state_mut().set_option("one-key-shortcuts", "off").unwrap();
    click_board_label(&mut h);
    host_key(&mut h, Modifiers::NONE, Key::B);
    assert_eq!(tool(&h), "Navigate", "B is not taken");
    host_text(&mut h, "!");
    h.run_steps(8);
    assert_eq!(glyphs(&h), [] as [u16; 0], "! is not taken");
    host_key(&mut h, Modifiers::NONE, Key::ArrowLeft);
    assert_eq!(shown(&h), "e4", "← still walks the game");
    // The button's name offers no B, which would do nothing; it still arms the brush.
    assert!(h.query_by_label("Glyph brush (B)").is_none());
    h.get_by_label("Glyph brush").click();
    h.run_steps(2);
    assert!(tool(&h).starts_with("GlyphBrush"), "the rail's button still arms it: {}", tool(&h));
    host_key(&mut h, Modifiers::NONE, Key::Escape);
    assert_eq!(tool(&h), "Navigate");
    // On again: B arms the brush, and the button says so.
    h.state_mut().set_option("one-key-shortcuts", "on").unwrap();
    h.run_steps(2);
    h.get_by_label("Glyph brush (B)");
    host_key(&mut h, Modifiers::NONE, Key::B);
    assert!(tool(&h).starts_with("GlyphBrush"), "{}", tool(&h));
}

/// With "Until Enter", a typed sign waits as long as it takes; Enter puts it on at once without
/// opening the comment editor, and Esc drops one (R5-11).
#[test]
fn enter_puts_a_typed_sign_on_and_esc_drops_it() {
    let mut h = harness();
    h.state_mut().set_option("typed-wait", "enter").unwrap();
    click_board_label(&mut h);
    host_text(&mut h, "!");
    // Five seconds of frames (kittest's 0.25 s each).
    h.run_steps(20);
    assert_eq!(glyphs(&h), [] as [u16; 0], "still waiting for a second character");
    host_key(&mut h, Modifiers::NONE, Key::Enter);
    assert_eq!(glyphs(&h), [1], "Enter put ! on");
    assert!(!comment_open(&h), "Enter did nothing else");
    host_text(&mut h, "?");
    host_key(&mut h, Modifiers::NONE, Key::Escape);
    h.run_steps(20);
    assert_eq!(glyphs(&h), [1], "Esc dropped ?");
    // Enter with nothing waiting is the comment editor's, as before.
    host_key(&mut h, Modifiers::NONE, Key::Enter);
    assert!(comment_open(&h));
}

/// A preference this build does not know (a newer build's) is written back by `persist` as it
/// was read; the ones it knows are written as they are now (R7-05).
#[test]
fn a_newer_builds_preferences_survive_persist() {
    let mut app = PdfCraftApp::new();
    app.restore(
        r#"{"chess_future": {"keys": [1, 2], "on": true}, "chess_comment_focus": true, "chess_one_key_shortcuts": false, "chess_typed_wait": "1.5"}"#,
    );
    assert!(app.chess.comment_focus && app.chess.one_key_off);
    assert_eq!(app.chess.typed_wait.token(), "1.5");
    app.chess.comment_focus = false;
    let saved: serde_json::Value = serde_json::from_str(&app.persist()).unwrap();
    assert_eq!(saved["chess_future"], serde_json::json!({"keys": [1, 2], "on": true}), "kept as read");
    assert_eq!(saved["chess_comment_focus"], false, "a known key is written as it is now");
    assert_eq!((&saved["chess_one_key_shortcuts"], &saved["chess_typed_wait"]), (&serde_json::json!(false), &serde_json::json!("1.5")));
    // And again, through a build that reads and writes them.
    let mut again = PdfCraftApp::new();
    again.restore(&app.persist());
    let saved: serde_json::Value = serde_json::from_str(&again.persist()).unwrap();
    assert_eq!(saved["chess_future"]["keys"], serde_json::json!([1, 2]));
    // Settings that are not an object keep nothing.
    let mut odd = PdfCraftApp::new();
    odd.restore("[1, 2]");
    let saved: serde_json::Value = serde_json::from_str(&odd.persist()).unwrap();
    assert!(saved.get("0").is_none() && saved["chess_typed_wait"] == "0.6");
}
