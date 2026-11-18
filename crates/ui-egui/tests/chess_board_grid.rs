//! Chess ▸ the analysis board walked square by square (plan A9-N7b, findings A6-25 and A8-18): a
//! grid of 64 squares for assistive technology, each with its place on the screen, whose value is
//! the square the keyboard is on; the arrows walk it, Enter plays, and Alt+Shift with P, a rank's
//! digit or a file's letter says what stands there. Keys go through `raw_input_hook`, as in the
//! desktop app.

use egui::accesskit::Role;
use egui::{Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use pdfcraft_engine::chess::pdfcraft_chess::game::{Game, NodePath};
use pdfcraft_ui_egui::PdfCraftApp;

/// A one-page document.
fn one_page() -> Vec<u8> {
    let body = "BT /F1 24 Tf 60 400 Td (Page 1) Tj ET";
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [4 0 R] /Count 1 /MediaBox [0 0 600 800] >>".to_string(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /Contents 5 0 R /Resources << /Font << /F1 3 0 R >> >> >>".to_string(),
        format!("<< /Length {} >>\nstream\n{body}\nendstream", body.len()),
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

/// The app at the reference screen (plan A9 §0) with the Board panel shown, the game 1. e4 e5
/// 2. Nf3 at 1… e5.
fn harness() -> Harness<'static, PdfCraftApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1366.0, 768.0)).build_eframe(|_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("page.pdf", None, one_page()).unwrap();
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

/// Press `key` with `modifiers` as the desktop app gets it: through `raw_input_hook` first, where
/// the chess scope takes its keys (kittest itself skips the hook).
fn host_key(h: &mut Harness<'static, PdfCraftApp>, modifiers: Modifiers, key: Key) {
    host_key_at(h, modifiers, key, None);
}

/// [`host_key`] with the key's place on the keyboard (`physical_key`), as egui-winit sends it.
fn host_key_at(h: &mut Harness<'static, PdfCraftApp>, modifiers: Modifiers, key: Key, physical_key: Option<Key>) {
    use eframe::App as _;
    let ctx = h.ctx.clone();
    for pressed in [true, false] {
        let mut raw = std::mem::take(h.input_mut());
        raw.events.push(egui::Event::ModifiersChanged(if pressed { modifiers } else { Modifiers::NONE }));
        raw.events.push(egui::Event::Key { key, physical_key, pressed, repeat: false, modifiers });
        h.state_mut().raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }
    h.run_steps(2);
}

/// The grid's value: the square the keyboard is on.
fn value(h: &Harness<'static, PdfCraftApp>) -> String {
    h.get_by_role(Role::Grid).accesskit_node().value().unwrap_or_default().to_owned()
}

/// What assistive technology finds focused: with the keyboard on the board, its active square.
fn focused(h: &Harness<'static, PdfCraftApp>) -> Vec<(Role, String)> {
    h.get_all_by(|n| n.is_focused()).map(|n| (n.accesskit_node().role(), n.accesskit_node().label().unwrap_or_default())).collect()
}

/// Said by a live region: a status (information) or an alert.
fn said(h: &Harness<'static, PdfCraftApp>, words: &str) -> Role {
    h.get_all_by_label_contains(words)
        .map(|n| n.accesskit_node().role())
        .find(|r| matches!(r, Role::Status | Role::Alert))
        .unwrap_or_else(|| panic!("nothing said with {words:?}"))
}

/// A picture of the window, when `PDFCRAFT_CHESS_SHOTS` names a folder.
fn shot(h: &mut Harness<'static, PdfCraftApp>, name: &str) {
    if let Ok(dir) = std::env::var("PDFCRAFT_CHESS_SHOTS") {
        h.render().unwrap().save(std::path::Path::new(&dir).join(name)).unwrap();
    }
}

/// Plan A9-N7b (the card's check, keyboard only): the analysis board is a grid of 64 cells, each
/// with its place on the screen (it was an image named by its position). With the keyboard on it,
/// its value is the square the keyboard is on; the arrows walk it as it is shown (the game does not
/// move); Enter on a piece picks it up, and Enter on its square plays the move, said as every move
/// shown is; Alt+Shift+P says the pieces, Alt+Shift+4 the fourth rank's, Alt+Shift+E the e file's.
#[test]
fn the_analysis_board_is_a_grid_walked_and_played_from_the_keyboard() {
    let mut h = harness();
    let grid = h.get_by_role(Role::Grid);
    let cells: Vec<_> = grid.get_all_by_role(Role::GridCell).collect();
    assert_eq!(cells.len(), 64);
    assert!(cells.iter().all(|c| c.accesskit_node().raw_bounds().is_some_and(|b| b.width() > 1.0)), "every square has its place");
    h.get_by_role(Role::Grid).focus();
    h.run_steps(2);
    assert_eq!(value(&h), "e2, empty", "the keyboard starts on e2 (its pawn is on e4)");
    assert_eq!(focused(&h), [(Role::GridCell, "e2, empty".to_owned())], "a screen reader is on the square: the grid's active cell");
    // The arrows walk the squares; the game stays at 1… e5.
    host_key(&mut h, Modifiers::NONE, Key::ArrowUp);
    host_key(&mut h, Modifiers::NONE, Key::ArrowUp);
    assert_eq!(value(&h), "e4, White pawn");
    assert_eq!(shown(&h), "e5", "the game did not move");
    for _ in 0..3 {
        host_key(&mut h, Modifiers::NONE, Key::ArrowLeft);
    }
    host_key(&mut h, Modifiers::NONE, Key::ArrowDown);
    host_key(&mut h, Modifiers::NONE, Key::ArrowDown);
    host_key(&mut h, Modifiers::NONE, Key::ArrowDown);
    assert_eq!(value(&h), "b1, White knight");
    // Enter picks the knight up, then plays it to c3.
    host_key(&mut h, Modifiers::NONE, Key::Enter);
    said(&h, "White knight on b1 picked up");
    host_key(&mut h, Modifiers::NONE, Key::ArrowRight);
    host_key(&mut h, Modifiers::NONE, Key::ArrowUp);
    host_key(&mut h, Modifiers::NONE, Key::ArrowUp);
    assert_eq!(value(&h), "c3, empty");
    shot(&mut h, "n7b-grid-light.png");
    h.state_mut().set_option("theme", "dark").unwrap();
    h.run_steps(3);
    shot(&mut h, "n7b-grid-dark.png");
    h.state_mut().set_option("theme", "light").unwrap();
    h.run_steps(3);
    host_key(&mut h, Modifiers::NONE, Key::Enter);
    assert_eq!(shown(&h), "Nc3", "played");
    assert_eq!(said(&h, "2. knight c3"), Role::Status, "said as every move shown is");
    assert_eq!(focused(&h), [(Role::GridCell, "c3, White knight".to_owned())], "the keyboard stays on the board, on the square played to");
    // The pieces, asked.
    host_key(&mut h, ALT_SHIFT, Key::P);
    assert_eq!(said(&h, "White: king e1, queen d1"), Role::Status);
}

const ALT_SHIFT: Modifiers = Modifiers { alt: true, shift: true, ctrl: false, mac_cmd: false, command: false };

/// Plan A9-N7b: Alt+Shift+P says the pieces, Alt+Shift with a rank's digit that rank's, with a
/// file's letter that file's, as a status, with the keyboard on the chess panels (here the board,
/// found by its name). A rank's digit is taken where it is on the keyboard: Alt+Shift+1 arrives as
/// `!` on a US keyboard.
#[test]
fn the_boards_questions_are_said_as_a_status() {
    let mut h = harness();
    h.get_by_label_contains("Board, White to move").focus();
    h.run_steps(2);
    host_key(&mut h, ALT_SHIFT, Key::P);
    assert_eq!(said(&h, "White: king e1, queen d1"), Role::Status);
    host_key(&mut h, ALT_SHIFT, Key::Num4);
    assert_eq!(said(&h, "Rank 4: White: pawns e4."), Role::Status);
    host_key(&mut h, ALT_SHIFT, Key::E);
    said(&h, "e file: White: king e1; pawns e4. Black: king e8; pawns e5.");
    host_key_at(&mut h, ALT_SHIFT, Key::Exclamationmark, Some(Key::Num1));
    said(&h, "Rank 1: White: king e1");
    assert_eq!(shown(&h), "e5", "a question moves nothing");
}

/// Plan A9-N7b: from Black's side the arrows walk the board as it is shown (↑ goes towards White's
/// side), and the cells' rows and columns are those shown.
#[test]
fn turned_round_the_arrows_follow_the_board_shown() {
    let mut h = harness();
    h.state_mut().chess.board_mut().flipped = true;
    h.run_steps(2);
    h.get_by_role(Role::Grid).focus();
    h.run_steps(2);
    assert_eq!(value(&h), "e2, empty");
    host_key(&mut h, Modifiers::NONE, Key::ArrowUp);
    assert_eq!(value(&h), "e1, White king", "towards the top, White's side from Black's");
    host_key(&mut h, Modifiers::NONE, Key::ArrowRight);
    assert_eq!(value(&h), "d1, White queen");
    host_key(&mut h, Modifiers::NONE, Key::End);
    assert_eq!(value(&h), "a1, White rook", "the row's last square as shown");
}
