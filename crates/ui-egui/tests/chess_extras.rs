//! Chess ▸ PGN toolbar T15 in the real shell (egui_kittest): the glyph brush, moving a variation
//! among the others at its fork, the threat (a null move), the diagram mark and copying a line as
//! text.

use egui::{Key, Modifiers, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use pdfcraft_engine::chess::pdfcraft_chess::game::{Game, NodePath};
use pdfcraft_ui_egui::PdfCraftApp;

/// A blank 600 × 800 pt page: the tests use the Board panel only.
fn blank() -> Vec<u8> {
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 600 800] >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << >> >>".to_string(),
        "<< /Length 0 >>\nstream\n\nendstream".to_string(),
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

/// The app on [`blank`], the Board panel shown.
fn harness() -> Harness<'static, PdfCraftApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 1000.0)).build_eframe(|_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("blank.pdf", None, blank()).unwrap();
        app.run_inline = true;
        app
    });
    h.run_steps(4);
    h.state_mut().show_chess_board();
    h.run_steps(3);
    h
}

/// Put the game of `pgn` on the board, at the move reached by `path` (reply indices).
fn put_game(h: &mut Harness<'static, PdfCraftApp>, pgn: &str, path: &[u16]) {
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

fn undo_label(h: &Harness<'static, PdfCraftApp>) -> Option<String> {
    h.state().chess.board().undo_label()
}

fn undo(h: &mut Harness<'static, PdfCraftApp>) {
    assert!(h.state_mut().execute("edit.undo"));
    h.run_steps(2);
}

/// The glyphs of the move at `path`.
fn glyphs_at(h: &Harness<'static, PdfCraftApp>, path: &[u16]) -> Vec<u16> {
    let game = &h.state().chess.board().game;
    game.nags(game.node_at(&NodePath::new(path.to_vec())).unwrap()).to_vec()
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

fn click_at(h: &mut Harness<'static, PdfCraftApp>, at: Pos2) {
    h.hover_at(at);
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    }
    h.run_steps(2);
}

/// A click on a label of the Board panel: the chess scope goes on, and nothing takes the focus.
fn click_board_title(h: &mut Harness<'static, PdfCraftApp>) {
    let title = h
        .get_all_by_label("To move:")
        .find(|n| n.accesskit_node().role() == egui::accesskit::Role::Label)
        .expect("a label of the panel (its title is the docking tab: plan A8, P6)");
    let at = title.rect().center();
    click_at(h, at);
    assert!(h.state().chess.in_scope());
}

/// Open the context menu of the move labelled `label` in the notation, and click `item` in it.
fn move_menu(h: &mut Harness<'static, PdfCraftApp>, label: &str, item: &str) {
    click_in_menu(h, label, item);
    h.run_steps(2);
}

/// [`move_menu`] without the frames after the click: the next frame's output is the click's.
fn click_in_menu(h: &mut Harness<'static, PdfCraftApp>, label: &str, item: &str) {
    h.get_by_label(label).click_secondary();
    h.run_steps(2);
    let entry = h.get_by_label(item);
    assert!(!entry.accesskit_node().is_disabled(), "{item} is on for {label}");
    entry.click();
}

/// Whether the clipboard got `text` within a few frames.
fn copied(h: &mut Harness<'static, PdfCraftApp>, text: &str) -> bool {
    let mut seen = false;
    for _ in 0..3 {
        h.step();
        seen |= h.output().platform_output.commands.iter().any(|c| matches!(c, egui::OutputCommand::CopyText(t) if t == text));
    }
    seen
}

/// The glyph brush: armed from the rail with the glyph last chosen (`!` at first), a glyph chosen
/// in a group while it is armed becomes the brush's; each move clicked in the notation gets it as
/// one undo step, without going to the move; a second click takes it off;
/// Esc disarms it, and a click on a move goes to it again.
#[test]
fn the_glyph_brush_marks_each_move_clicked_and_esc_puts_it_down() {
    let mut h = harness();
    put_game(&mut h, "1. e4 e5 2. Nf3 Nc6 *", &[0]);
    h.get_by_label("Glyph brush (B)").click();
    h.run_steps(2);
    assert_eq!(format!("{:?}", h.state().chess.tools.tool), "GlyphBrush(1)", "! until a glyph is chosen");
    // Brilliant move, chosen in the Move glyphs group, becomes the brush's.
    h.get_by_label("Good move").click_secondary();
    h.run_steps(2);
    h.get_by_label("Brilliant move").click();
    h.run_steps(2);
    assert_eq!(format!("{:?}", h.state().chess.tools.tool), "GlyphBrush(3)");
    assert_eq!(glyphs_at(&h, &[0]), [] as [u16; 0], "choosing the brush's glyph changed no move");
    h.get_by_label_contains("Glyph brush !! (Brilliant move)");
    for label in ["e5", "knight f3", "knight c6"] {
        h.get_by_label(label).click();
        h.run_steps(2);
    }
    assert_eq!((glyphs_at(&h, &[0, 0]), glyphs_at(&h, &[0, 0, 0]), glyphs_at(&h, &[0, 0, 0, 0])), (vec![3], vec![3], vec![3]));
    assert_eq!(shown(&h), "e4", "the brush does not go to the moves");
    assert_eq!(undo_label(&h).as_deref(), Some("Glyph !!"));
    // A second click takes it off.
    h.get_by_label("knight c6!!").click();
    h.run_steps(2);
    assert_eq!(glyphs_at(&h, &[0, 0, 0, 0]), [] as [u16; 0]);
    // One undo step per click: four steps back to none.
    for left in [[3u16].as_slice(), &[], &[], &[]] {
        undo(&mut h);
        assert_eq!(glyphs_at(&h, &[0, 0, 0, 0]), left);
    }
    assert_eq!((glyphs_at(&h, &[0, 0]), glyphs_at(&h, &[0, 0, 0])), (vec![], vec![]));
    // Esc disarms it; a click goes to the move and marks nothing.
    click_board_title(&mut h);
    host_key(&mut h, Modifiers::NONE, Key::Escape);
    assert_eq!(format!("{:?}", h.state().chess.tools.tool), "Navigate");
    assert!(h.query_by_label_contains("Glyph brush !!").is_none(), "the brush's bar is gone");
    h.get_by_label("knight f3").click();
    h.run_steps(2);
    assert_eq!((shown(&h), glyphs_at(&h, &[0, 0, 0])), ("Nf3".to_owned(), vec![]));
    // B arms it again from the keyboard, with the glyph last chosen.
    host_key(&mut h, Modifiers::NONE, Key::B);
    assert_eq!(format!("{:?}", h.state().chess.tools.tool), "GlyphBrush(3)");
}

const THREE: &str = "1. e4 e5 (1... c5) (1... e6 2. d4) (1... d5) *";
const THREE_TEXT: &str = "1. e4 e5 (1... c5) (1... e6 2. d4) (1... d5)";

/// Alt+↑ and Alt+↓ move the nearest variation one place among the others at its fork, as the
/// commands and a move's menu do; each is one undo step, and the line itself never moves.
#[test]
fn variations_move_up_and_down_by_keys_menu_and_command() {
    let mut h = harness();
    put_game(&mut h, THREE, &[0, 2, 0]);
    click_board_title(&mut h);
    host_key(&mut h, Modifiers::ALT, Key::ArrowUp);
    assert_eq!(movetext(&h), "1. e4 e5 (1... e6 2. d4) (1... c5) (1... d5)");
    assert_eq!(shown(&h), "d4", "the move shown stays");
    assert_eq!(undo_label(&h).as_deref(), Some("Move a variation"));
    // First already: refused, and the panel says why.
    host_key(&mut h, Modifiers::ALT, Key::ArrowUp);
    assert_eq!(movetext(&h), "1. e4 e5 (1... e6 2. d4) (1... c5) (1... d5)");
    h.get_by_label("Not moved: it is the first variation here already.");
    host_key(&mut h, Modifiers::ALT, Key::ArrowDown);
    assert_eq!(movetext(&h), THREE_TEXT);
    undo(&mut h);
    undo(&mut h);
    assert_eq!(movetext(&h), THREE_TEXT, "two steps, both undone");
    // A move's menu acts on that move's variation.
    move_menu(&mut h, "d5", "Move variation up");
    assert_eq!(movetext(&h), "1. e4 e5 (1... c5) (1... d5) (1... e6 2. d4)");
    // On the line itself there is nothing to move.
    h.get_by_label("e5").click_secondary();
    h.run_steps(2);
    assert!(h.get_by_label("Move variation up").accesskit_node().is_disabled());
    assert!(h.get_by_label("Move variation down").accesskit_node().is_disabled());
    h.key_press(Key::Escape);
    h.run_steps(2);
    // The commands do as the keys.
    put_game(&mut h, THREE, &[0, 1]);
    assert!(h.state_mut().execute("chess.variation.down"));
    h.run_steps(2);
    assert_eq!(movetext(&h), "1. e4 e5 (1... e6 2. d4) (1... c5) (1... d5)");
    assert!(h.state().chess.board().pgn_text.contains("(1... e6 2. d4) (1... c5)"), "the PGN field follows");
}

/// The threat: a null move after the move, from the Structure group's command or a move's menu;
/// one undo step, written `--` in the PGN, and refused (with the reason) where the side to move is
/// in check.
#[test]
fn a_threat_is_a_null_move_in_the_pgn_and_never_in_check() {
    let mut h = harness();
    put_game(&mut h, "1. e4 e5 2. Nf3 *", &[0, 0, 0]);
    move_menu(&mut h, "e5", "Threat (null move)");
    assert_eq!(movetext(&h), "1. e4 e5 2. Nf3 (2. --)");
    h.get_by_label("--");
    assert_eq!(undo_label(&h).as_deref(), Some("Play a null move"));
    undo(&mut h);
    assert_eq!(movetext(&h), "1. e4 e5 2. Nf3");
    // The command plays it after the move shown.
    assert!(h.state_mut().execute("chess.moves.null"));
    h.run_steps(2);
    assert_eq!(movetext(&h), "1. e4 e5 2. Nf3 --");
    assert_eq!(shown(&h), "--");
    assert!(h.state().chess.board().pgn_text.contains("2. Nf3 -- *"), "{}", h.state().chess.board().pgn_text);
    // In check there is no passing.
    put_game(&mut h, "1. e4 f5 2. Qh5+ *", &[0, 0, 0]);
    assert!(h.state_mut().execute("chess.moves.null"));
    h.run_steps(2);
    assert_eq!(movetext(&h), "1. e4 f5 2. Qh5+");
    h.get_by_label("No threat here: the side to move is in check and cannot pass.");
}

/// The diagram mark: a move's menu puts it on (`$220` in the PGN, a small board after the move in
/// the notation) and takes it off, each one undo step.
#[test]
fn a_diagram_mark_goes_into_the_pgn_and_shows_after_its_move() {
    let mut h = harness();
    put_game(&mut h, "1. e4 e5 2. Nf3 *", &[0]);
    move_menu(&mut h, "e5", "Diagram here");
    assert_eq!(movetext(&h), "1. e4 e5 $220 2. Nf3");
    assert!(h.state().chess.board().pgn_text.contains("e5 $220"), "{}", h.state().chess.board().pgn_text);
    h.get_by_label("Diagram after 1…e5");
    h.get_by_label("e5");
    assert_eq!(undo_label(&h).as_deref(), Some("Diagram mark"));
    assert_eq!(shown(&h), "e4", "marking does not go to the move");
    move_menu(&mut h, "e5", "Diagram here");
    assert_eq!(movetext(&h), "1. e4 e5 2. Nf3");
    assert!(h.query_by_label("Diagram after 1…e5").is_none());
    undo(&mut h);
    assert_eq!(movetext(&h), "1. e4 e5 $220 2. Nf3", "one step");
}

/// Copying a line: a move's menu copies its variation up to it with figurines and glyphs,
/// numbered as books do; the commands copy the line of the move shown, in figurines or letters.
#[test]
fn a_line_is_copied_as_a_book_prints_it() {
    let mut h = harness();
    put_game(&mut h, "1. e4 e5 (1... c5 $1 2. Nf3 d6) 2. Nf3 Nc6 *", &[0]);
    click_in_menu(&mut h, "d6", "Copy line with figurines");
    assert!(copied(&mut h, "1…c5! 2.♘f3 d6"), "the variation from its first move");
    put_game(&mut h, "1. e4 e5 (1... c5 $1 2. Nf3 d6) 2. Nf3 Nc6 *", &[0, 0, 0, 0]);
    assert!(h.state_mut().execute("chess.moves.copy_figurines"));
    assert!(copied(&mut h, "1.e4 e5 2.♘f3 ♘c6"));
    assert!(h.state_mut().execute("chess.moves.copy_letters"));
    assert!(copied(&mut h, "1.e4 e5 2.Nf3 Nc6"));
    assert_eq!(h.state().toast.as_ref().map(|t| t.0.as_str()), Some("Line copied"));
    // At the start there is no line to copy.
    put_game(&mut h, "1. e4 *", &[]);
    assert!(!h.state_mut().execute("chess.moves.copy_letters"));
}
