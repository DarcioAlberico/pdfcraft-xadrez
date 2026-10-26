//! Chess ▸ PGN toolbar U1 in the real shell (egui_kittest): the floating glyph palette, and the
//! glyphs and the variations to choose from in a move's context menu.

use egui::accesskit::{Role, Toggled};
use egui::{Key, Modifiers, Pos2, Rect, vec2};
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
    harness_of(egui::vec2(1400.0, 1000.0))
}

/// [`harness`] in a window `size` points large.
fn harness_of(size: egui::Vec2) -> Harness<'static, PdfCraftApp> {
    let mut h = Harness::builder().with_size(size).build_eframe(|_cc| {
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

/// A click on the Board panel's title: the chess scope goes on, and nothing takes the focus.
fn click_board_title(h: &mut Harness<'static, PdfCraftApp>) {
    let title = h.get_all_by_label("Chess board").find(|n| n.accesskit_node().role() == Role::Label).expect("the panel's title");
    let at = title.rect().center();
    click_at(h, at);
    assert!(h.state().chess.in_scope());
}

/// The glyph palette's window, if it is open.
fn palette(h: &Harness<'static, PdfCraftApp>) -> Option<Rect> {
    h.query_by_role_and_label(Role::Window, "Glyph palette").map(|w| w.rect())
}

/// The palette's title, where it is dragged by.
fn palette_title(h: &Harness<'static, PdfCraftApp>) -> Pos2 {
    let window = palette(h).expect("the palette is open");
    h.get_all_by_label("Glyph palette")
        .find(|n| n.accesskit_node().role() == Role::Label && window.contains(n.rect().center()))
        .expect("its title")
        .rect()
        .center()
}

/// The button named `label` inside the palette's window (the rail has buttons of the same names).
fn in_palette<'a>(h: &'a Harness<'static, PdfCraftApp>, label: &'a str) -> egui_kittest::Node<'a> {
    let window = palette(h).expect("the palette is open");
    let mut found = h.get_all_by_label(label).filter(|n| n.accesskit_node().role() == Role::Button && window.contains(n.rect().center()));
    let node = found.next().unwrap_or_else(|| panic!("{label} in the palette"));
    assert!(found.next().is_none(), "one {label} in the palette");
    node
}

/// Drag with the primary button from `from` by `by`, over a few frames.
fn drag(h: &mut Harness<'static, PdfCraftApp>, from: Pos2, by: egui::Vec2) {
    h.hover_at(from);
    h.run_steps(1);
    h.drag_at(from);
    h.run_steps(1);
    for k in 1..=4 {
        h.hover_at(from + by * (k as f32 / 4.0));
        h.run_steps(1);
    }
    h.drop_at(from + by);
    h.run_steps(2);
}

/// The palette floats: G (or the rail's button) opens it beside the rail as a window of its own,
/// a glyph clicked in it goes on the move shown with the side's code and is ticked there (one undo
/// step), it is dragged by its title and stays where it was put when it opens again, it never
/// leaves the window, and Esc and its close button close it.
#[test]
fn the_glyph_palette_floats_puts_glyphs_on_the_move_and_remembers_its_place() {
    let mut h = harness();
    put_game(&mut h, "1. e4 e5 *", &[0, 0]);
    click_board_title(&mut h);
    assert!(palette(&h).is_none());
    host_key(&mut h, Modifiers::NONE, Key::G);
    let first = palette(&h).expect("G opened the palette");
    let rail = h.get_by_label("Glyph palette (G)");
    assert_eq!(rail.accesskit_node().toggled(), Some(Toggled::True), "the rail's button shows it open");
    assert!(first.left() >= rail.rect().right(), "beside the rail: {first:?}, rail {:?}", rail.rect());
    // A glyph: on Black's move, Black's code for the initiative; ticked in the palette.
    in_palette(&h, "With the initiative").click();
    h.run_steps(2);
    assert_eq!(glyphs_at(&h, &[0, 0]), [37]);
    assert_eq!(undo_label(&h).as_deref(), Some("Glyph ↑"));
    assert_eq!(in_palette(&h, "With the initiative").accesskit_node().toggled(), Some(Toggled::True));
    // A move-quality glyph replaces the other, as the rail's do.
    in_palette(&h, "Interesting move").click();
    h.run_steps(2);
    in_palette(&h, "Blunder").click();
    h.run_steps(2);
    assert_eq!(glyphs_at(&h, &[0, 0]), [4, 37]);
    undo(&mut h);
    assert_eq!(glyphs_at(&h, &[0, 0]), [5, 37], "one undo step a glyph");
    assert!(h.state().chess.board().pgn_text.contains("e5 $5 $37"), "{}", h.state().chess.board().pgn_text);
    // Dragged by its title, it stays where it was put.
    let title = palette_title(&h);
    drag(&mut h, title, vec2(-300.0, 100.0));
    let moved = palette(&h).unwrap();
    assert!((moved.left() - (first.left() - 300.0)).abs() < 2.0 && (moved.top() - (first.top() + 100.0)).abs() < 2.0, "{first:?} → {moved:?}");
    // Esc closes it; G opens it where it was.
    host_key(&mut h, Modifiers::NONE, Key::Escape);
    assert!(palette(&h).is_none(), "Esc closed it");
    host_key(&mut h, Modifiers::NONE, Key::G);
    assert_eq!(palette(&h), Some(moved), "open again where it was put");
    // Dragged far off, it stays within the window.
    let title = palette_title(&h);
    drag(&mut h, title, vec2(5000.0, 5000.0));
    let screen = h.ctx.content_rect();
    let kept = palette(&h).unwrap();
    assert!(screen.contains_rect(kept), "{kept:?} within {screen:?}");
    // The close button closes it too.
    in_palette(&h, "Close the glyph palette").click();
    h.run_steps(2);
    assert!(palette(&h).is_none());
    assert_eq!(h.get_by_label("Glyph palette (G)").accesskit_node().toggled(), Some(Toggled::False));
    // The rail's button opens it.
    h.get_by_label("Glyph palette (G)").click();
    h.run_steps(2);
    assert!(palette(&h).is_some());
}

/// A click in the palette, even on nothing in it, puts the chess key scope on as a click in the
/// panel does: Delete then deletes the variation, not something of the page, and a typed sign
/// goes on the move.
#[test]
fn a_click_in_the_glyph_palette_keeps_the_keys_on_the_game() {
    let mut h = harness();
    put_game(&mut h, "1. e4 e5 (1... c5) 2. Nf3 *", &[0, 1]);
    click_board_title(&mut h);
    host_key(&mut h, Modifiers::NONE, Key::G);
    assert!(palette(&h).is_some());
    // A click on the page: the keys are the document's again.
    let page = h.state().views[0].page_screen_rect(0).expect("the page on screen");
    click_at(&mut h, page.center());
    assert!(!h.state().chess.in_scope(), "the page has the keys");
    assert!(palette(&h).is_some(), "the palette stays open");
    let title = palette_title(&h);
    click_at(&mut h, title);
    assert!(h.state().chess.in_scope(), "a click in the palette is a click in the chess panels");
    host_text(&mut h, "?");
    h.run_steps(4);
    assert_eq!(glyphs_at(&h, &[0, 1]), [2], "the sign went on the move");
    host_key(&mut h, Modifiers::NONE, Key::Delete);
    assert_eq!(movetext(&h), "1. e4 e5 2. Nf3", "Delete deleted the variation");
}

const NESTED: &str = "1. e4 e5 (1... c5 2. Nf3 d6 (2... Nc6) 3. d4) *";

/// Open the context menu of the move labelled `label` in the notation, and click `item` in it.
fn move_menu(h: &mut Harness<'static, PdfCraftApp>, label: &str, item: &str) {
    h.get_by_label(label).click_secondary();
    h.run_steps(2);
    let entry = h.get_by_label(item);
    assert!(!entry.accesskit_node().is_disabled(), "{item} is on for {label}");
    entry.click();
    h.run_steps(2);
}

/// A move's menu has the move-quality glyphs and the evaluations, ticked while the move has them,
/// and "Remove all glyphs": each one undo step on that move, which stays unshown.
#[test]
fn a_moves_menu_puts_glyphs_on_it_and_takes_them_off() {
    let mut h = harness();
    put_game(&mut h, NESTED, &[0]);
    move_menu(&mut h, "knight c6", "Blunder");
    assert_eq!(glyphs_at(&h, &[0, 1, 0, 1]), [4]);
    assert_eq!(undo_label(&h).as_deref(), Some("Glyph ??"));
    assert_eq!(shown(&h), "e4", "the move shown stays");
    move_menu(&mut h, "knight c6??", "Black is clearly better");
    assert_eq!(glyphs_at(&h, &[0, 1, 0, 1]), [4, 17]);
    // Ticked in the menu; a second click takes it off.
    h.get_by_label_contains("knight c6?? ").click_secondary();
    h.run_steps(2);
    assert_eq!(h.get_by_label("Blunder").accesskit_node().toggled(), Some(Toggled::True));
    assert_eq!(h.get_by_label("Dubious move").accesskit_node().toggled(), Some(Toggled::False));
    h.key_press(Key::Escape);
    h.run_steps(2);
    h.get_by_label_contains("knight c6?? ").click_secondary();
    h.run_steps(2);
    h.get_by_label("Remove all glyphs").click();
    h.run_steps(2);
    assert_eq!(glyphs_at(&h, &[0, 1, 0, 1]), [] as [u16; 0]);
    assert_eq!(undo_label(&h).as_deref(), Some("Clear glyphs"));
    undo(&mut h);
    assert_eq!(glyphs_at(&h, &[0, 1, 0, 1]), [4, 17], "one step put both back");
    // A move without glyphs has nothing to remove.
    h.get_by_label("d6").click_secondary();
    h.run_steps(2);
    assert!(h.get_by_label("Remove all glyphs").accesskit_node().is_disabled());
}

/// §4.5 in a move's menu: where a variation tool could act on several variations on the way to
/// the move, they are listed under it, nearest first, and the one chosen is the one it acts on;
/// the entry itself acts on the nearest.
#[test]
fn a_moves_menu_lists_the_variations_a_tool_could_act_on() {
    let mut h = harness();
    put_game(&mut h, NESTED, &[0]);
    h.get_by_label("knight c6").click_secondary();
    h.run_steps(2);
    let near = h.get_by_label("Promote one level: 2…Nc6");
    let far = h.get_by_label("Promote one level: 1…c5");
    assert!(near.rect().left() < far.rect().left(), "nearest first");
    h.get_by_label("Delete variation: 1…c5");
    assert!(h.query_by_label_contains("Demote:").is_none(), "one variation to demote: no list");
    h.key_press(Key::Escape);
    h.run_steps(2);
    move_menu(&mut h, "knight c6", "Promote one level: 1…c5");
    assert_eq!(movetext(&h), "1. e4 c5 (1... e5) 2. Nf3 d6 (2... Nc6) 3. d4");
    assert_eq!(undo_label(&h).as_deref(), Some("Promote a variation"));
    undo(&mut h);
    move_menu(&mut h, "knight c6", "Promote one level");
    assert_eq!(movetext(&h), "1. e4 e5 (1... c5 2. Nf3 Nc6 (2... d6 3. d4))", "the entry: the nearest");
}

/// The control named `label` inside the palette's window (a section's header, a list row, the
/// filter field), not text the palette floats over (the notation's).
fn named_in_palette<'a>(h: &'a Harness<'static, PdfCraftApp>, label: &'a str) -> Option<egui_kittest::Node<'a>> {
    let window = palette(h).expect("the palette is open");
    h.query_all_by_label(label).find(|n| window.contains(n.rect().center()) && n.accesskit_node().role() != Role::Label)
}

/// The assessments without a printed sign are in the palette, folded under "More assessments" in
/// sections, or found with its filter: one clicked goes on the move shown with the code of the side
/// that made it (one undo step) and is ticked in the list, and the notation names it after the
/// move. It goes on the move even while a comment field had the keyboard (as text it would only be
/// `$n`). The positional row's painted signs go on the move as the other glyphs do.
#[test]
fn the_palette_lists_the_assessments_without_a_sign() {
    // A window tall enough for the palette with its list open (a shorter one scrolls the palette).
    let mut h = harness_of(egui::vec2(1400.0, 1600.0));
    put_game(&mut h, "1. e4 e5 2. Nf3 *", &[0, 0]);
    click_board_title(&mut h);
    host_key(&mut h, Modifiers::NONE, Key::G);
    assert!(named_in_palette(&h, "Slight space advantage").is_none(), "folded away at first");
    named_in_palette(&h, "More assessments").expect("the list's header").click();
    h.run_steps(2);
    named_in_palette(&h, "Space and control").expect("its section").click();
    h.run_steps(2);
    // On Black's move, Black's code.
    named_in_palette(&h, "Slight space advantage").expect("the assessment").click();
    h.run_steps(2);
    assert_eq!(glyphs_at(&h, &[0, 0]), [25]);
    assert_eq!(undo_label(&h).as_deref(), Some("Glyph Slight space advantage"));
    assert_eq!(named_in_palette(&h, "Slight space advantage").unwrap().accesskit_node().toggled(), Some(Toggled::True));
    // The notation names it after the move, as text.
    assert!(h.get_all_by_label("Slight space advantage").any(|n| n.accesskit_node().role() == Role::Label), "named in the notation");
    assert!(h.state().chess.board().pgn_text.contains("e5 $25"), "{}", h.state().chess.board().pgn_text);
    // A second click takes it off; one undo step puts it back.
    named_in_palette(&h, "Slight space advantage").unwrap().click();
    h.run_steps(2);
    assert_eq!(glyphs_at(&h, &[0, 0]), [] as [u16; 0]);
    undo(&mut h);
    assert_eq!(glyphs_at(&h, &[0, 0]), [25]);
    // The filter narrows the list to the names with its text; White's move gets White's code.
    put_game(&mut h, "1. e4 e5 2. Nf3 *", &[0, 0, 0]);
    let filter = named_in_palette(&h, "Filter assessments").expect("the filter field");
    filter.click();
    h.run_steps(1);
    h.get_by_label("Filter assessments").type_text("knight");
    h.run_steps(2);
    assert!(named_in_palette(&h, "Slight space advantage").is_none(), "filtered out");
    assert!(named_in_palette(&h, "Poor knight placement").is_some());
    named_in_palette(&h, "Good knight placement").unwrap().click();
    h.run_steps(2);
    assert_eq!(glyphs_at(&h, &[0, 0, 0]), [88]);
    // With the comment field taking the keyboard, an assessment still goes on the move, not into
    // the comment.
    h.get_by_label("Comment on this move").click();
    h.run_steps(2);
    named_in_palette(&h, "Poor knight placement").unwrap().click();
    h.run_steps(2);
    assert_eq!(glyphs_at(&h, &[0, 0, 0]), [86, 88]);
    assert_eq!(h.state().chess.board().game.comment(h.state().chess.board().game.current()), "");
    // A painted sign of the positional row: a button like the other glyphs (the comment field
    // gives up the keyboard first, or the sign would go into it).
    click_board_title(&mut h);
    in_palette(&h, "Passed pawn").click();
    h.run_steps(2);
    assert_eq!(glyphs_at(&h, &[0, 0, 0]), [86, 88, 252]);
    assert!(h.state().chess.board().pgn_text.contains("Nf3 $86 $88 $252"), "{}", h.state().chess.board().pgn_text);
    if let Ok(dir) = std::env::var("PDFCRAFT_CHESS_SHOTS") {
        h.render().unwrap().save(std::path::Path::new(&dir).join("glyph-palette-assessments.png")).unwrap();
    }
}

/// Ask for the palette's control named `label` to be scrolled into view (as assistive technology
/// does), at most `max` times, until it is inside the palette. Returns whether it got there.
fn scroll_to(h: &mut Harness<'static, PdfCraftApp>, label: &str, max: usize) -> bool {
    for _ in 0..max {
        let window = palette(h).expect("the palette is open");
        let found = h.query_all_by_label(label).find(|n| n.accesskit_node().role() != Role::Label);
        let Some(node) = found else { return false };
        if window.shrink(2.0).contains(node.rect().center()) {
            return true;
        }
        node.scroll_to_me();
        h.run_steps(3);
    }
    false
}

/// In a short window the palette never runs past its bottom: with More assessments open it moves
/// up and its list scrolls, and the last section's last assessment can be scrolled to and put on
/// the move.
#[test]
fn the_palette_stays_in_a_short_window_and_its_list_scrolls() {
    let mut h = harness_of(egui::vec2(1024.0, 600.0));
    put_game(&mut h, "1. e4 e5 2. Nf3 *", &[0, 0]);
    click_board_title(&mut h);
    host_key(&mut h, Modifiers::NONE, Key::G);
    let viewport = h.ctx.content_rect();
    assert!(viewport.contains_rect(palette(&h).expect("G opened the palette")));
    named_in_palette(&h, "More assessments").expect("the list's header").click();
    h.run_steps(4);
    let open = palette(&h).unwrap();
    assert!(viewport.contains_rect(open), "{open:?} within {viewport:?}");
    // The list scrolls: to the last section, open it, and to its last row.
    assert!(scroll_to(&mut h, "Phases of the game", 10), "the last section is reachable");
    named_in_palette(&h, "Phases of the game").unwrap().click();
    h.run_steps(3);
    assert!(viewport.contains_rect(palette(&h).unwrap()), "still within with the section open");
    assert!(scroll_to(&mut h, "Very well played endgame", 10), "its last assessment is reachable");
    named_in_palette(&h, "Very well played endgame").unwrap().click();
    h.run_steps(2);
    assert_eq!(glyphs_at(&h, &[0, 0]), [129], "Black's code");
    assert!(viewport.contains_rect(palette(&h).unwrap()));
}
