//! Chess ▸ the document's games list in the real shell (egui_kittest), PGN toolbar spec T14:
//! renaming a game through its tags, duplicating, deleting after a question, reordering (the
//! order the collection is exported and autosaved in), a new game from a position, the keys
//! of a focused row, which never reach the chess key scope, and several games selected (Ctrl+click,
//! Shift+click, Shift+arrows, Ctrl+A) acted on together.

use egui::accesskit::Role;
use egui::{Key, Modifiers, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use pdfcraft_ui_egui::PdfCraftApp;

/// A 600 × 800 pt page with the start position set in a chess diagram font (as the Chess
/// diagrams tests build it), so a scan finds one diagram the board can open.
fn boards() -> Vec<u8> {
    const MERIDA_START: [&str; 8] = ["tMvWlVmT", "OoOoOoOo", " + + + +", "+ + + + ", " + + + +", "+ + + + ", "pPpPpPpP", "RnBqKbNr"];
    let mut content = String::from("BT /F1 20 Tf 60 724 Td ");
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

/// Tall enough for the Board panel's board, rail, least notation and a list of a few games.
const WINDOW_HEIGHT: f32 = 1300.0;

/// The app on [`boards`], the Board panel shown.
fn harness() -> Harness<'static, PdfCraftApp> {
    harness_of(WINDOW_HEIGHT)
}

/// [`harness`] in a window `height` points tall.
fn harness_of(height: f32) -> Harness<'static, PdfCraftApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, height)).with_step_dt(0.05).build_eframe(|_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("boards.pdf", None, boards()).unwrap();
        app.run_inline = true;
        app
    });
    h.run_steps(4);
    h.state_mut().show_chess_board();
    h.run_steps(3);
    h
}

/// A folder of its own for one test.
fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcraft-chess-games-t14-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The names of the games in the list, in order, and which one is shown.
fn listed(app: &PdfCraftApp) -> (Vec<String>, usize) {
    let c = app.chess.games.collection();
    (c.iter().enumerate().map(|(i, e)| e.name(i + 1)).collect(), c.active_index())
}

/// The movetext of every game, in list order.
fn movetexts(app: &PdfCraftApp) -> Vec<String> {
    app.chess.games.collection().iter().map(|e| e.board.game.movetext()).collect()
}

/// Three games read from a PGN file: events A, B and C.
fn three_games(h: &mut Harness<'static, PdfCraftApp>) {
    let pgn = "[Event \"A\"]\n\n1. e4 *\n\n[Event \"B\"]\n\n1. d4 *\n\n[Event \"C\"]\n\n1. c4 *\n";
    assert_eq!(h.state_mut().import_chess_pgn("abc.pgn", pgn.as_bytes()), 3);
    h.run_steps(3);
    assert_eq!(listed(h.state()), (vec!["A".to_owned(), "B".to_owned(), "C".to_owned()], 0));
}

/// Export the games with the list's Export… button into `path`, and read the file back.
fn export(h: &mut Harness<'static, PdfCraftApp>, path: &std::path::Path) -> String {
    let _ = std::fs::remove_file(path);
    h.state_mut().pick_override = Some(vec![path.to_string_lossy().into_owned()]);
    h.get_by_label("Export…").click();
    h.run_steps(4);
    h.state_mut().pick_override = None;
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("exported to {}: {e}; toast {:?}", path.display(), h.state().toast))
}

/// The `[Event "…"]` values of a PGN text, in order.
fn events(pgn: &str) -> Vec<&str> {
    pgn.lines().filter_map(|l| l.strip_prefix("[Event \"").and_then(|r| r.strip_suffix("\"]"))).collect()
}

/// Open the menu of the row named `name` (a notice shown at the bottom of the window, over the
/// list's last rows, is put away first).
fn row_menu(h: &mut Harness<'static, PdfCraftApp>, name: &str) {
    h.state_mut().toast = None;
    h.run_steps(1);
    h.get_by_label(name).click_secondary();
    h.run_steps(2);
}

/// The text field labelled `label` (the name editor's).
fn field<'a>(h: &'a Harness<'static, PdfCraftApp>, label: &'a str) -> egui_kittest::Node<'a> {
    h.get_all_by_label(label).find(|n| n.accesskit_node().role() == Role::TextInput).unwrap_or_else(|| panic!("a {label} field"))
}

/// Press `key` with `modifiers` as the desktop app gets it: through `raw_input_hook` first, where
/// the chess key scope and a focused row of the list take their keys (kittest skips the hook).
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

/// A click on the Board panel's title: the chess key scope goes on, and nothing has the focus.
fn click_board_title(h: &mut Harness<'static, PdfCraftApp>) {
    let title = h.get_all_by_label("Chess board").find(|n| n.accesskit_node().role() == Role::Label).expect("the panel's title");
    let at = title.rect().center();
    click_at(h, at);
    assert!(h.state().chess.in_scope());
    assert!(!h.ctx.egui_wants_keyboard_input(), "nothing has the keyboard");
}

/// Give the row named `name` the keyboard focus, as Tab would.
fn focus_row(h: &mut Harness<'static, PdfCraftApp>, name: &str) {
    h.get_by_label(name).focus();
    h.run_steps(2);
    assert!(h.get_by_label(name).is_focused(), "the row {name} has the keyboard");
}

/// Renaming sets the game's White, Black and Event tags in one undo step of its game: the list
/// shows the players, and the exported PGN has the names.
#[test]
fn a_game_renamed_through_its_tags_shows_in_the_list_and_in_the_exported_pgn() {
    let mut h = harness();
    h.state_mut().chess.board_mut().play_uci_line(&["e2e4".into(), "e7e5".into()]);
    h.run_steps(2);
    assert_eq!(listed(h.state()).0, ["Game 1"]);
    row_menu(&mut h, "Game 1");
    h.get_by_label("Rename…").click();
    h.run_steps(4);
    // The editor's first field has the keyboard.
    assert!(field(&h, "White").is_focused());
    field(&h, "White").type_text("Werle");
    h.run_steps(1);
    field(&h, "Black").focus();
    h.run_steps(1);
    field(&h, "Black").type_text("Jianu");
    h.run_steps(1);
    field(&h, "Event").focus();
    h.run_steps(1);
    field(&h, "Event").type_text("Wijk aan Zee B");
    h.run_steps(1);
    h.get_by_label("Rename").click();
    h.run_steps(3);
    assert_eq!(listed(h.state()).0, ["Werle–Jianu"]);
    let game = &h.state().chess.board().game;
    assert_eq!((game.tag("White"), game.tag("Black"), game.tag("Event")), (Some("Werle"), Some("Jianu"), Some("Wijk aan Zee B")));
    assert_eq!(game.movetext(), "1. e4 e5", "the moves are untouched");
    assert_eq!(h.state().chess.board().undo_label().as_deref(), Some("Edit game details"), "one undo step");
    let dir = temp_dir("rename");
    let pgn = export(&mut h, &dir.join("games.pgn"));
    assert!(pgn.contains("[Event \"Wijk aan Zee B\"]\n") && pgn.contains("[White \"Werle\"]\n") && pgn.contains("[Black \"Jianu\"]\n"), "{pgn}");
    // Undo takes the whole rename back.
    assert!(h.state_mut().chess.board_mut().undo());
    h.run_steps(2);
    assert_eq!(listed(h.state()).0, ["Game 1"]);
    // A name with only the event is listed by it; Cancel changes nothing.
    assert!(h.state_mut().execute("chess.game.rename"));
    h.run_steps(4);
    field(&h, "Event").focus();
    h.run_steps(1);
    field(&h, "Event").type_text("Training");
    h.run_steps(1);
    h.get_by_label("Cancel").click();
    h.run_steps(2);
    assert_eq!(listed(h.state()).0, ["Game 1"]);
    assert!(h.state_mut().execute("chess.game.rename"));
    h.run_steps(4);
    field(&h, "Event").focus();
    h.run_steps(1);
    field(&h, "Event").type_text("Training");
    h.run_steps(1);
    h.get_by_label("Rename").click();
    h.run_steps(2);
    assert_eq!(listed(h.state()).0, ["Training"]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Duplicate puts an identical game after the original and shows it: the same PGN, move shown
/// and side; changing the copy leaves the original alone.
#[test]
fn duplicating_makes_an_identical_game_after_it() {
    let mut h = harness();
    three_games(&mut h);
    {
        let b = h.state_mut().chess.board_mut();
        b.play_uci_line(&["e7e5".into(), "g1f3".into()]);
        b.game.go_to(2);
        b.flipped = true;
    }
    h.run_steps(2);
    let original = h.state().chess.board().game.pgn();
    row_menu(&mut h, "A");
    h.get_by_label("Duplicate").click();
    h.run_steps(3);
    assert_eq!(listed(h.state()), (vec!["A".to_owned(), "A".to_owned(), "B".to_owned(), "C".to_owned()], 1), "the copy is after it, shown");
    let c = h.state().chess.games.collection();
    let (a, copy) = (c.get(0).unwrap(), c.get(1).unwrap());
    assert_eq!(copy.board.game.pgn(), original);
    assert_eq!(copy.current_path(), a.current_path(), "the same move shown");
    assert!(copy.board.flipped);
    assert_eq!(copy.board.undo_label(), None, "a history of its own");
    // A move played in the copy, where it shows 1... e5: a variation of the copy only.
    h.state_mut().chess.board_mut().play_uci_line(&["d2d4".into()]);
    h.run_steps(2);
    assert_eq!(movetexts(h.state()), ["1. e4 e5 2. Nf3", "1. e4 e5 2. Nf3 (2. d4)", "1. d4", "1. c4"]);
    // The command duplicates the game shown.
    assert!(h.state_mut().execute("chess.game.duplicate"));
    h.run_steps(2);
    assert_eq!(listed(h.state()).0.len(), 5);
    assert_eq!(listed(h.state()).1, 2);
    assert_eq!(movetexts(h.state())[1], movetexts(h.state())[2]);
}

/// Delete asks first ("It can't be undone") and deletes only on yes; the game after it is shown.
/// Deleting the last game leaves a blank one.
#[test]
fn deleting_asks_first_and_the_last_delete_leaves_a_blank_game() {
    let mut h = harness();
    three_games(&mut h);
    row_menu(&mut h, "B");
    h.get_by_label("Delete…").click();
    h.run_steps(4);
    h.get_by_label("Delete this game? You can undo it.");
    assert_eq!(listed(h.state()).0.len(), 3, "nothing deleted before the answer");
    h.get_by_label("Cancel").click();
    h.run_steps(2);
    assert!(h.query_by_label("Delete this game? You can undo it.").is_none());
    assert_eq!(listed(h.state()).0.len(), 3);
    // Yes: B goes; A stays shown.
    row_menu(&mut h, "B");
    h.get_by_label("Delete…").click();
    h.run_steps(4);
    h.get_by_label("Delete").click();
    h.run_steps(3);
    assert_eq!(listed(h.state()), (vec!["A".to_owned(), "C".to_owned()], 0));
    // The game shown deleted (by the command): the one after it is shown.
    assert!(h.state_mut().execute("chess.game.delete"));
    h.run_steps(4);
    h.get_by_label("Delete").click();
    h.run_steps(3);
    assert_eq!(listed(h.state()), (vec!["C".to_owned()], 0));
    assert_eq!(movetexts(h.state()), ["1. c4"]);
    // The last one: a blank game takes its place.
    assert!(h.state_mut().execute("chess.game.delete"));
    h.run_steps(4);
    h.get_by_label("Delete").click();
    h.run_steps(3);
    assert_eq!(listed(h.state()), (vec!["Game 1".to_owned()], 0));
    let board = h.state().chess.board();
    assert_eq!(board.game.movetext(), "");
    assert!(board.game.tags().is_empty() && board.source.is_none());
}

/// Moving a game up or down changes the order the collection is exported in (and the game shown
/// stays shown); from the keyboard, Alt+↑ and Alt+↓ on a focused row move it and keep its focus.
#[test]
fn reordering_changes_the_export_order() {
    let mut h = harness();
    three_games(&mut h);
    let dir = temp_dir("reorder");
    let path = dir.join("games.pgn");
    assert_eq!(events(&export(&mut h, &path)), ["A", "B", "C"]);
    row_menu(&mut h, "C");
    h.get_by_label("Move up").click();
    h.run_steps(2);
    assert_eq!(listed(h.state()), (vec!["A".to_owned(), "C".to_owned(), "B".to_owned()], 0), "A is still shown");
    let pgn = export(&mut h, &path);
    assert_eq!(events(&pgn), ["A", "C", "B"]);
    assert!(pgn.contains("1. e4 *\n\n[Event \"C\"]"), "a blank line between two games: {pgn}");
    // The first can't go up, nor the last down.
    row_menu(&mut h, "A");
    assert!(h.get_by_label("Move up").accesskit_node().is_disabled());
    h.key_press(Key::Escape);
    h.run_steps(2);
    row_menu(&mut h, "B");
    assert!(h.get_by_label("Move down").accesskit_node().is_disabled());
    h.key_press(Key::Escape);
    h.run_steps(2);
    // From the keyboard: A down twice, the row keeping the keyboard.
    focus_row(&mut h, "A");
    host_key(&mut h, Modifiers::ALT, Key::ArrowDown);
    assert_eq!(listed(h.state()).0, ["C", "A", "B"]);
    assert!(h.get_by_label("A").is_focused(), "the moved row keeps the keyboard");
    host_key(&mut h, Modifiers::ALT, Key::ArrowDown);
    assert_eq!(listed(h.state()), (vec!["C".to_owned(), "B".to_owned(), "A".to_owned()], 2));
    assert_eq!(events(&export(&mut h, &path)), ["C", "B", "A"]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// New game from this position starts at the move shown (or a move of the notation, from its
/// menu): its PGN has the position as SetUp/FEN, and it keeps the diagram it came from.
#[test]
fn a_new_game_from_here_starts_at_that_position() {
    let mut h = harness();
    assert!(h.state_mut().execute("chess.scan"));
    h.run_steps(3);
    h.state_mut().open_chess_board(0, 0);
    h.run_steps(3);
    let source = h.state().chess.board().source.expect("the board came from the diagram");
    h.state_mut().chess.board_mut().play_uci_line(&["e2e4".into(), "e7e5".into(), "g1f3".into()]);
    h.state_mut().chess.board_mut().game.go_to(2);
    h.run_steps(2);
    let after_e5 = h.state().chess.board().game.fen();
    assert!(after_e5.starts_with("rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq"), "{after_e5}");
    let after_e4 = {
        let game = &h.state().chess.board().game;
        game.fen_at(game.main_line()[1]).unwrap()
    };
    assert!(after_e4.starts_with("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq"), "{after_e4}");
    assert!(h.state_mut().execute("chess.game.new_from_here"));
    h.run_steps(2);
    let (names, shown) = listed(h.state());
    assert_eq!((names.len(), shown), (2, 1), "after the game it came from, shown");
    let board = h.state().chess.board();
    assert_eq!(board.game.start_fen(), after_e5);
    assert_eq!(board.game.movetext(), "");
    assert_eq!(board.source, Some(source), "the diagram it came from");
    let pgn = board.game.pgn();
    assert!(pgn.contains("[SetUp \"1\"]\n") && pgn.contains(&format!("[FEN \"{after_e5}\"]\n")), "{pgn}");
    assert_eq!(movetexts(h.state())[0], "1. e4 e5 2. Nf3", "the game it came from is untouched");
    // From a move's menu in the notation: the position after that move. Both games are named by
    // their diagram; the first is the one the new game came from.
    assert_eq!(names, ["Diagram 1, p. 1", "Diagram 1, p. 1"]);
    h.get_all_by_label("Diagram 1, p. 1").next().unwrap().click();
    h.run_steps(2);
    assert_eq!(listed(h.state()).1, 0);
    h.get_by_label("e4").click_secondary();
    h.run_steps(2);
    h.get_by_label("New game from this position").click();
    h.run_steps(3);
    let (names, shown) = listed(h.state());
    assert_eq!((names.len(), shown), (3, 1));
    assert_eq!(h.state().chess.board().game.start_fen(), after_e4);
    assert_eq!(movetexts(h.state()), ["1. e4 e5 2. Nf3", "", ""], "the game it came from is untouched, and so is the first new one");
    // A move played in it is Black's.
    h.state_mut().chess.board_mut().play_uci_line(&["c7c5".into()]);
    assert!(h.state().chess.board().game.movetext().contains("... c5"), "{}", h.state().chess.board().game.movetext());
}

/// Delete pressed on a focused row of the list asks about deleting that game, and never deletes
/// a variation of the game on the board (the chess key scope takes no key then); Delete with the
/// board's scope on deletes the variation and no game. F2 opens the name editor, Enter shows
/// the row's game.
#[test]
fn delete_on_a_focused_row_is_the_lists_and_on_the_board_the_games() {
    use pdfcraft_engine::chess::pdfcraft_chess::game::{Game, NodePath};
    let mut h = harness();
    three_games(&mut h);
    // Game A, with variations, at 1... e6.
    let forks = "[Event \"A\"]

1. e4 e5 (1... c5) (1... e6) 2. Nf3 *";
    let put = |h: &mut Harness<'static, PdfCraftApp>| {
        let (mut game, _) = Game::from_pgn(forks).unwrap();
        game.go_to_node(game.node_at(&NodePath::new(vec![0, 2])).unwrap());
        h.state_mut().chess.board_mut().game = game;
        h.run_steps(2);
    };
    put(&mut h);
    click_board_title(&mut h);
    // The row has the keyboard: Delete is the list's.
    focus_row(&mut h, "B");
    host_key(&mut h, Modifiers::NONE, Key::Delete);
    assert_eq!(movetexts(h.state())[0], "1. e4 e5 (1... c5) (1... e6) 2. Nf3", "no variation deleted");
    h.get_by_label("Delete this game? You can undo it.");
    // The question's Delete button has the keyboard: Delete again deletes nothing either.
    assert!(h.get_by_label("Delete").is_focused());
    host_key(&mut h, Modifiers::NONE, Key::Delete);
    assert_eq!(movetexts(h.state())[0], "1. e4 e5 (1... c5) (1... e6) 2. Nf3");
    assert_eq!(listed(h.state()).0.len(), 3);
    // Esc says no, and the row gets the keyboard back.
    host_key(&mut h, Modifiers::NONE, Key::Escape);
    assert!(h.query_by_label("Delete this game? You can undo it.").is_none());
    assert_eq!(listed(h.state()).0.len(), 3);
    assert!(h.get_by_label("B").is_focused());
    // Backspace too, then Enter on the focused Delete button deletes B.
    host_key(&mut h, Modifiers::NONE, Key::Backspace);
    host_key(&mut h, Modifiers::NONE, Key::Enter);
    assert_eq!(listed(h.state()), (vec!["A".to_owned(), "C".to_owned()], 0));
    assert_eq!(movetexts(h.state())[0], "1. e4 e5 (1... c5) (1... e6) 2. Nf3");
    // The game after it has the keyboard: F2 renames it, Esc in the field leaves it as it was.
    assert!(h.get_by_label("C").is_focused());
    host_key(&mut h, Modifiers::NONE, Key::F2);
    assert!(field(&h, "White").is_focused(), "the name editor is open");
    host_key(&mut h, Modifiers::NONE, Key::Escape);
    assert!(h.query_all_by_label("White").all(|n| n.accesskit_node().role() != Role::TextInput));
    assert!(h.get_by_label("C").is_focused());
    // Enter shows the focused row's game.
    host_key(&mut h, Modifiers::NONE, Key::Enter);
    assert_eq!(listed(h.state()).1, 1);
    h.get_by_label("A").click();
    h.run_steps(2);
    // The board has the scope and nothing has the keyboard: Delete deletes the variation, not a game.
    put(&mut h);
    click_board_title(&mut h);
    host_key(&mut h, Modifiers::NONE, Key::Delete);
    assert_eq!(movetexts(h.state())[0], "1. e4 e5 (1... c5) 2. Nf3", "the variation 1... e6 went");
    assert_eq!(listed(h.state()).0.len(), 2, "no game deleted");
    assert!(h.query_by_label("Delete this game? You can undo it.").is_none(), "and nothing asked");
}

/// The order and the names come back in the next session (autosave).
#[test]
fn the_order_and_the_names_come_back_in_the_next_session() {
    let dir = temp_dir("autosave");
    let store = pdfcraft_ui_egui::RecoveryStore::new(dir.join("recovery"));
    let path = dir.join("book.pdf").to_string_lossy().into_owned();
    let session = |store: &pdfcraft_ui_egui::RecoveryStore| {
        let (store, path) = (store.clone(), path.clone());
        let mut h = Harness::builder().with_size(egui::vec2(1400.0, WINDOW_HEIGHT)).with_step_dt(0.05).build_eframe(move |_cc| {
            let mut app = PdfCraftApp::new();
            app.set_option("language", "en").unwrap();
            app.run_inline = true;
            app.enable_recovery(store);
            app.open_bytes("book.pdf", Some(path), boards()).unwrap();
            app
        });
        h.run_steps(4);
        h.state_mut().show_chess_board();
        h.run_steps(3);
        h
    };
    let expected = {
        let mut h = session(&store);
        three_games(&mut h);
        row_menu(&mut h, "A");
        h.get_by_label("Move down").click();
        h.run_steps(2);
        row_menu(&mut h, "C");
        h.get_by_label("Rename…").click();
        h.run_steps(4);
        field(&h, "White").type_text("Werle");
        h.run_steps(1);
        field(&h, "Black").focus();
        h.run_steps(1);
        field(&h, "Black").type_text("Jianu");
        h.run_steps(1);
        h.get_by_label("Rename").click();
        h.run_steps(2);
        assert_eq!(listed(h.state()), (vec!["B".to_owned(), "A".to_owned(), "Werle–Jianu".to_owned()], 1));
        h.state_mut().flush_chess_games();
        (listed(h.state()), movetexts(h.state()))
    };
    let h = session(&store);
    assert_eq!((listed(h.state()), movetexts(h.state())), expected);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Drag the row named `from` and drop it on the top (`above`) or bottom half of the row named `to`,
/// as a mouse does: press, move past the drag threshold, move over the target, release there.
fn drag_row(h: &mut Harness<'static, PdfCraftApp>, from: &str, to: &str, above: bool) {
    h.state_mut().toast = None;
    h.run_steps(1);
    let start = h.get_by_label(from).rect().center();
    let target = h.get_by_label(to).rect();
    let end = egui::pos2(target.center().x, if above { target.top() + 2.0 } else { target.bottom() - 2.0 });
    h.hover_at(start);
    h.run_steps(1);
    h.drag_at(start);
    h.run_steps(1);
    for k in 1..=4 {
        h.hover_at(start + (end - start) * (k as f32 / 4.0));
        h.run_steps(1);
    }
    h.event(egui::Event::PointerButton { pos: end, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(1);
    h.remove_cursor();
    h.run_steps(2);
}

/// Wave 10 U2: a delete and a move of the list are steps Undo takes back in the chess scope (the
/// menu's and Ctrl+Z's Undo name them), and a row dragged onto another goes there, which Undo
/// takes back too; the game shown stays shown.
#[test]
fn undo_takes_back_a_delete_and_a_move_and_rows_can_be_dragged() {
    let mut h = harness();
    three_games(&mut h);
    row_menu(&mut h, "B");
    h.get_by_label("Delete…").click();
    h.run_steps(4);
    h.get_by_label("Delete").click();
    h.run_steps(3);
    assert_eq!(listed(h.state()), (vec!["A".to_owned(), "C".to_owned()], 0));
    assert!(h.state().chess.in_scope(), "Undo is the chess game's after a change to its list");
    assert!(h.state_mut().execute("edit.undo"));
    h.run_steps(2);
    assert_eq!(listed(h.state()), (vec!["A".to_owned(), "B".to_owned(), "C".to_owned()], 0), "B is back where it was");
    assert_eq!(h.state().toast.as_ref().map(|t| t.0.as_str()), Some("Undid Delete game"));
    assert!(h.state_mut().execute("edit.redo"));
    h.run_steps(2);
    assert_eq!(listed(h.state()).0, ["A", "C"]);
    assert!(h.state_mut().execute("edit.undo"));
    h.run_steps(2);
    // C dragged onto the top half of A goes first; A stays shown.
    drag_row(&mut h, "C", "A", true);
    assert_eq!(listed(h.state()), (vec!["C".to_owned(), "A".to_owned(), "B".to_owned()], 1));
    // A dragged onto the bottom half of B goes last.
    drag_row(&mut h, "A", "B", false);
    assert_eq!(listed(h.state()), (vec!["C".to_owned(), "B".to_owned(), "A".to_owned()], 2));
    // Dropped on itself: nothing moves.
    drag_row(&mut h, "B", "B", true);
    assert_eq!(listed(h.state()).0, ["C", "B", "A"]);
    assert!(h.state_mut().execute("edit.undo"));
    h.run_steps(2);
    assert_eq!(h.state().toast.as_ref().map(|t| t.0.as_str()), Some("Undid Move game"));
    assert_eq!(listed(h.state()), (vec!["C".to_owned(), "A".to_owned(), "B".to_owned()], 1));
    assert!(h.state_mut().execute("edit.undo"));
    h.run_steps(2);
    assert_eq!(listed(h.state()), (vec!["A".to_owned(), "B".to_owned(), "C".to_owned()], 0));
}

/// New game, Duplicate and Open PGN… are steps Undo takes back, and Redo puts back. A game
/// changed after it was added is kept in Deleted moves when Undo takes it out (and said), comes
/// out of it again with Redo, and a game opened from there goes back there with Undo.
#[test]
fn undo_takes_back_games_added_and_keeps_a_changed_one() {
    let mut h = harness();
    three_games(&mut h);
    let toast = |h: &Harness<'static, PdfCraftApp>| h.state().toast.as_ref().map(|t| t.0.clone()).unwrap_or_default();
    // Open PGN… is one step.
    assert!(h.state_mut().execute("edit.undo"));
    h.run_steps(2);
    assert_eq!(toast(&h), "Undid Load the PGN");
    assert_eq!(listed(h.state()), (vec!["Game 1".to_owned()], 0), "a blank game is left");
    assert!(h.state_mut().execute("edit.redo"));
    h.run_steps(2);
    assert_eq!(listed(h.state()), (vec!["A".to_owned(), "B".to_owned(), "C".to_owned()], 0));
    // New game, from the list's button: Undo shows the game shown before again.
    h.get_by_label("New game").click();
    h.run_steps(3);
    assert_eq!(listed(h.state()), (vec!["A".to_owned(), "B".to_owned(), "C".to_owned(), "Game 4".to_owned()], 3));
    assert!(h.state_mut().execute("edit.undo"));
    h.run_steps(2);
    assert_eq!(toast(&h), "Undid New game");
    assert_eq!(listed(h.state()), (vec!["A".to_owned(), "B".to_owned(), "C".to_owned()], 0));
    // A copy of B, changed, then A shown: Undo takes the copy out, into Deleted moves.
    row_menu(&mut h, "B");
    h.get_by_label("Duplicate").click();
    h.run_steps(3);
    assert_eq!(listed(h.state()).1, 2);
    h.state_mut().chess.board_mut().play_uci_line(&["d7d5".into()]);
    h.run_steps(2);
    h.state_mut().toast = None;
    h.run_steps(1);
    h.get_by_label("A").click();
    h.run_steps(2);
    assert_eq!(listed(h.state()).1, 0);
    assert!(h.state_mut().execute("edit.undo"));
    h.run_steps(2);
    assert_eq!(toast(&h), "Undid Duplicate game · 1 changed game was kept in Deleted moves.");
    assert_eq!(listed(h.state()), (vec!["A".to_owned(), "B".to_owned(), "C".to_owned()], 0));
    h.get_by_label("Deleted moves (1)…");
    // Redo: the copy is back with its move, and no longer in Deleted moves.
    assert!(h.state_mut().execute("edit.redo"));
    h.run_steps(2);
    assert_eq!(movetexts(h.state()), ["1. e4", "1. d4", "1. d4 d5", "1. c4"]);
    assert!(h.query_by_label("Deleted moves (1)…").is_none());
    // Taken out again, then opened from Deleted moves: Undo puts it back there.
    assert!(h.state_mut().execute("edit.undo"));
    h.run_steps(2);
    h.state_mut().toast = None;
    h.get_by_label("Deleted moves (1)…").click();
    h.run_steps(2);
    h.get_by_label("Open as a new game").click();
    h.run_steps(3);
    assert_eq!(movetexts(h.state()), ["1. e4", "1. d4 d5", "1. d4", "1. c4"], "after the game shown");
    assert!(h.query_by_label("Deleted moves (1)…").is_none());
    assert!(h.state_mut().execute("edit.undo"));
    h.run_steps(2);
    assert_eq!(toast(&h), "Undid Open as a new game");
    assert_eq!(movetexts(h.state()), ["1. e4", "1. d4", "1. c4"]);
    h.get_by_label("Deleted moves (1)…");
}

/// A diagram opened on the board is a step of the list too: Undo takes its game back out and
/// shows the game shown before, Redo puts it back; changed after it was opened, it is kept in
/// Deleted moves when Undo takes it out, and Redo takes it out of there again.
#[test]
fn undo_takes_back_a_diagram_opened_on_the_board() {
    let mut h = harness();
    three_games(&mut h);
    let toast = |h: &Harness<'static, PdfCraftApp>| h.state().toast.as_ref().map(|t| t.0.clone()).unwrap_or_default();
    assert!(h.state_mut().execute("chess.scan"));
    h.run_steps(3);
    h.state_mut().open_chess_board(0, 0);
    h.run_steps(3);
    let opened = || (vec!["A".to_owned(), "B".to_owned(), "C".to_owned(), "Diagram 1, p. 1".to_owned()], 3);
    assert_eq!(listed(h.state()), opened());
    assert_eq!(h.query_all_by_label_contains("Undo Load the diagram (").count(), 1, "the rail's Undo names it");
    assert!(h.state_mut().execute("edit.undo"));
    h.run_steps(2);
    assert_eq!(toast(&h), "Undid Load the diagram");
    assert_eq!(listed(h.state()), (vec!["A".to_owned(), "B".to_owned(), "C".to_owned()], 0), "A, shown before, is shown again");
    assert!(h.state_mut().execute("edit.redo"));
    h.run_steps(2);
    assert_eq!(listed(h.state()), opened());
    // Opened again while untouched: shown again, and no step of its own.
    h.get_by_label("A").click();
    h.run_steps(2);
    h.state_mut().open_chess_board(0, 0);
    h.run_steps(3);
    assert_eq!(listed(h.state()), opened());
    // Changed, then A shown: Undo takes it out into Deleted moves, and says so.
    h.state_mut().chess.board_mut().play_uci_line(&["e2e4".into()]);
    h.run_steps(2);
    h.state_mut().toast = None;
    h.run_steps(1);
    h.get_by_label("A").click();
    h.run_steps(2);
    assert!(h.state_mut().execute("edit.undo"));
    h.run_steps(2);
    assert_eq!(toast(&h), "Undid Load the diagram · 1 changed game was kept in Deleted moves.");
    assert_eq!(listed(h.state()), (vec!["A".to_owned(), "B".to_owned(), "C".to_owned()], 0));
    h.get_by_label("Deleted moves (1)…");
    assert!(h.state_mut().execute("edit.redo"));
    h.run_steps(2);
    assert_eq!(movetexts(h.state()), ["1. e4", "1. d4", "1. c4", "1. e4"]);
    assert!(h.query_by_label("Deleted moves (1)…").is_none());
}

/// The PGN field's Load PGN puts the first game of the field on the board and adds the others to
/// the list after it, said; Undo takes the games added back first, then the game loaded.
#[test]
fn the_pgn_field_puts_its_first_game_on_the_board_and_adds_the_others() {
    // Tall enough for the PGN section under the list of games.
    let mut h = harness_of(1700.0);
    h.get_by_label("PGN").click();
    h.run_steps(3);
    h.state_mut().chess.board_mut().pgn_text = "[Event \"A\"]\n\n1. e4 *\n\n[Event \"B\"]\n\n1. d4 *\n\n[Event \"C\"]\n\n1. c4 *\n".into();
    // The notation gives the opened section its room over a few frames.
    h.run_steps(10);
    h.get_by_label("Load PGN").click();
    h.run_steps(3);
    assert_eq!(listed(h.state()), (vec!["A".to_owned(), "B".to_owned(), "C".to_owned()], 0), "the first on the board, shown");
    assert_eq!(movetexts(h.state()), ["1. e4", "1. d4", "1. c4"]);
    assert_eq!(h.state().toast.as_ref().map(|t| t.0.as_str()), Some("2 more games were added to the list of games."));
    assert!(h.state_mut().execute("edit.undo"));
    h.run_steps(2);
    assert_eq!(h.state().toast.as_ref().map(|t| t.0.as_str()), Some("Undid Load the PGN"));
    assert_eq!(listed(h.state()), (vec!["A".to_owned()], 0));
    assert!(h.state_mut().execute("edit.undo"));
    h.run_steps(2);
    assert_eq!(listed(h.state()), (vec!["Game 1".to_owned()], 0), "then the game loaded");
}

/// Four games read from a PGN file: events A, B, C and D, A shown.
fn four_games(h: &mut Harness<'static, PdfCraftApp>) {
    let pgn = "[Event \"A\"]\n\n1. e4 *\n\n[Event \"B\"]\n\n1. d4 *\n\n[Event \"C\"]\n\n1. c4 *\n\n[Event \"D\"]\n\n1. Nf3 *\n";
    assert_eq!(h.state_mut().import_chess_pgn("abcd.pgn", pgn.as_bytes()), 4);
    h.run_steps(3);
    assert_eq!(listed(h.state()), (vec!["A".to_owned(), "B".to_owned(), "C".to_owned(), "D".to_owned()], 0));
}

/// Click the row named `name` with `modifiers` held (Ctrl is [`Modifiers::COMMAND`]), as a mouse
/// does: the modifiers are down during the frame of the click.
fn click_with(h: &mut Harness<'static, PdfCraftApp>, name: &str, modifiers: Modifiers) {
    h.state_mut().toast = None;
    h.run_steps(1);
    let at = h.get_by_label(name).rect().center();
    h.hover_at(at);
    h.run_steps(1);
    h.event(egui::Event::ModifiersChanged(modifiers));
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers });
    }
    h.run_steps(1);
    h.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(2);
}

/// The names of the rows a screen reader is told are selected, in list order.
fn selected(h: &Harness<'static, PdfCraftApp>) -> Vec<String> {
    use egui::accesskit::Toggled;
    listed(h.state()).0.into_iter().filter(|name| h.get_by_label(name).accesskit_node().toggled() == Some(Toggled::True)).collect()
}

/// Ctrl+click adds a game to the selection and Shift+click selects a run of games, without
/// changing the game shown; a row's menu then acts on all of them as one step of the list:
/// Delete… (one question), Move up, Save PGN…, and dragging moves them as a block. A plain click
/// shows one game, alone.
#[test]
fn games_selected_with_ctrl_and_shift_are_deleted_moved_and_saved_together() {
    let mut h = harness();
    four_games(&mut h);
    assert_eq!(selected(&h), ["A"], "the game shown is the selection");
    click_with(&mut h, "C", Modifiers::COMMAND);
    assert_eq!((selected(&h), listed(h.state()).1), (vec!["A".to_owned(), "C".to_owned()], 0), "Ctrl+click adds C; A stays shown");
    // The menu of a selected row acts on both: no Rename…, one question.
    row_menu(&mut h, "C");
    assert!(h.query_by_label("Rename…").is_none(), "a selection has no single name");
    h.get_by_label("Delete…").click();
    h.run_steps(4);
    h.get_by_label("Delete these games? You can undo it.");
    h.get_by_label("2 selected");
    assert_eq!(listed(h.state()).0.len(), 4, "nothing deleted before the answer");
    h.get_by_label("Delete").click();
    h.run_steps(3);
    assert_eq!(listed(h.state()), (vec!["B".to_owned(), "D".to_owned()], 0), "both gone; the game after A is shown");
    assert!(h.state_mut().execute("edit.undo"));
    h.run_steps(2);
    assert_eq!(h.state().toast.as_ref().map(|t| t.0.as_str()), Some("Undid Delete game"), "one step");
    assert_eq!(listed(h.state()), (vec!["A".to_owned(), "B".to_owned(), "C".to_owned(), "D".to_owned()], 0), "each back at its place");
    assert_eq!(selected(&h), ["A"], "a delete leaves nothing selected");
    // A plain click shows B alone; Shift+click on D selects B to D, B still shown.
    click_with(&mut h, "B", Modifiers::NONE);
    assert_eq!((selected(&h), listed(h.state()).1), (vec!["B".to_owned()], 1));
    click_with(&mut h, "D", Modifiers::SHIFT);
    assert_eq!((selected(&h), listed(h.state()).1), (vec!["B".to_owned(), "C".to_owned(), "D".to_owned()], 1));
    row_menu(&mut h, "C");
    h.get_by_label("Move up").click();
    h.run_steps(3);
    assert_eq!(listed(h.state()), (vec!["B".to_owned(), "C".to_owned(), "D".to_owned(), "A".to_owned()], 0), "a block, in its order");
    assert_eq!(selected(&h), ["B", "C", "D"], "the selection moved with them");
    assert!(h.state_mut().execute("edit.undo"));
    h.run_steps(2);
    assert_eq!(h.state().toast.as_ref().map(|t| t.0.as_str()), Some("Undid Move game"));
    assert_eq!(listed(h.state()).0, ["A", "B", "C", "D"]);
    // Save PGN… in a selected row's menu writes the games selected, in list order.
    let dir = temp_dir("selection");
    let path = dir.join("some.pgn");
    let bar = h.get_by_label("Save PGN…").rect();
    row_menu(&mut h, "D");
    h.state_mut().pick_override = Some(vec![path.to_string_lossy().into_owned()]);
    h.get_all_by_label("Save PGN…").find(|n| n.rect() != bar).expect("the menu's Save PGN…").click();
    h.run_steps(4);
    h.state_mut().pick_override = None;
    let pgn = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("saved to {}: {e}; toast {:?}", path.display(), h.state().toast));
    assert_eq!(events(&pgn), ["B", "C", "D"]);
    // Dragged by one of them, the selection goes together to where it is dropped.
    drag_row(&mut h, "D", "A", true);
    assert_eq!(listed(h.state()), (vec!["B".to_owned(), "C".to_owned(), "D".to_owned(), "A".to_owned()], 0));
    assert!(h.state_mut().execute("edit.undo"));
    h.run_steps(2);
    assert_eq!(listed(h.state()).0, ["A", "B", "C", "D"]);
    // Ctrl+click on a selected game takes it out.
    click_with(&mut h, "C", Modifiers::COMMAND);
    assert_eq!(selected(&h), ["B", "D"]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// From the keyboard, on a focused row: Shift+↓ selects on to the next row (which takes the
/// keyboard), Alt+↓ moves the selection as a block, Ctrl+A selects every game, Ctrl+Space takes
/// one out, and Delete asks about the games selected.
#[test]
fn the_keys_of_a_focused_row_select_and_act_on_several_games() {
    let mut h = harness();
    four_games(&mut h);
    focus_row(&mut h, "A");
    host_key(&mut h, Modifiers::SHIFT, Key::ArrowDown);
    assert_eq!(selected(&h), ["A", "B"]);
    assert!(h.get_by_label("B").is_focused(), "the keyboard went on to B");
    host_key(&mut h, Modifiers::ALT, Key::ArrowDown);
    assert_eq!(listed(h.state()), (vec!["C".to_owned(), "A".to_owned(), "B".to_owned(), "D".to_owned()], 1), "A and B one place down");
    assert!(h.get_by_label("B").is_focused(), "the moved row keeps the keyboard");
    host_key(&mut h, Modifiers::COMMAND, Key::A);
    assert_eq!(selected(&h), ["C", "A", "B", "D"], "Ctrl+A: every game");
    host_key(&mut h, Modifiers::COMMAND, Key::Space);
    assert_eq!(selected(&h), ["C", "A", "D"], "Ctrl+Space takes the focused row out");
    focus_row(&mut h, "D");
    host_key(&mut h, Modifiers::NONE, Key::Delete);
    h.get_by_label("Delete these games? You can undo it.");
    h.get_by_label("3 selected");
    assert!(h.get_by_label("Delete").is_focused(), "the question has the keyboard");
    h.get_by_label("Delete").click();
    h.run_steps(3);
    assert_eq!(listed(h.state()).0, ["B"]);
    assert!(h.state_mut().execute("edit.undo"));
    h.run_steps(2);
    assert_eq!(listed(h.state()).0, ["C", "A", "B", "D"]);
}

/// The list's Rename… edits Site, Date and Round with the names, as Game details does, in one
/// undo step; a date PGN can't read is said, and Rename waits until it is one.
#[test]
fn the_list_renames_site_date_and_round_and_refuses_a_date_pgn_cannot_read() {
    let mut h = harness();
    h.state_mut().chess.board_mut().play_uci_line(&["e2e4".into()]);
    h.run_steps(2);
    row_menu(&mut h, "Game 1");
    h.get_by_label("Rename…").click();
    h.run_steps(4);
    let hint = "A date is written YYYY.MM.DD, with ? for each digit not known.";
    assert!(h.query_by_label(hint).is_none());
    for (label, text) in [("White", "Werle"), ("Site", "Wijk aan Zee NED"), ("Date", "2025.01.1"), ("Round", "6")] {
        field(&h, label).focus();
        h.run_steps(1);
        field(&h, label).type_text(text);
        h.run_steps(1);
    }
    h.get_by_label(hint);
    assert!(h.get_by_label("Rename").accesskit_node().is_disabled(), "Rename waits for a date PGN can read");
    field(&h, "Date").focus();
    h.run_steps(1);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(1);
    field(&h, "Date").type_text("2025.01.18");
    h.run_steps(2);
    assert!(h.query_by_label(hint).is_none());
    h.get_by_label("Rename").click();
    h.run_steps(3);
    let game = &h.state().chess.board().game;
    assert_eq!(
        (game.tag("White"), game.tag("Site"), game.tag("Date"), game.tag("Round")),
        (Some("Werle"), Some("Wijk aan Zee NED"), Some("2025.01.18"), Some("6"))
    );
    assert_eq!(listed(h.state()).0, ["Werle"]);
    assert_eq!(h.state().chess.board().undo_label().as_deref(), Some("Edit game details"));
    assert!(h.state_mut().chess.board_mut().undo());
    h.run_steps(2);
    assert!(h.state().chess.board().game.tags().is_empty(), "one undo step took them all back");
    // The editor opens with the game's tags.
    assert!(h.state_mut().chess.board_mut().redo());
    assert!(h.state_mut().execute("chess.game.rename"));
    h.run_steps(4);
    assert_eq!(field(&h, "Round").value().as_deref(), Some("6"));
    assert_eq!(field(&h, "Date").value().as_deref(), Some("2025.01.18"));
}

/// Plan HC10: the game's header says only what its tags know (a date as far as it is known, no
/// `?`), a click on it opens Game details, and the result follows the main line. A game with no
/// tags has no header.
#[test]
fn the_games_header_shows_what_is_known_and_opens_its_details() {
    let mut h = harness();
    h.state_mut().chess.board_mut().play_uci_line(&["e2e4".into()]);
    h.run_steps(2);
    assert!(h.query_by_label("Result: *").is_some());
    assert_eq!(h.query_all_by_label_contains("Game details: ").count(), 0, "no header without tags");
    let pgn = "[Event \"Candidates\"]\n[Site \"Curaçao\"]\n[Date \"1962.??.??\"]\n[Round \"?\"]\n[White \"Tal\"]\n[Black \"Fischer\"]\n[WhiteElo \"?\"]\n[BlackElo \"2785\"]\n[Result \"0-1\"]\n\n1. e4 c5 0-1\n";
    assert_eq!(h.state_mut().import_chess_pgn("g.pgn", pgn.as_bytes()), 1);
    h.run_steps(3);
    let header = "Game details: Tal – Fischer (2785). Candidates · Curaçao · 1962";
    assert!(h.query_by_label(header).is_some(), "the header names what is known");
    assert!(h.query_by_label("Result: 0-1").is_some());
    assert_eq!(h.query_all_by_label("Site").count(), 0, "Game details starts closed");
    h.get_by_label(header).click();
    h.run_steps(4);
    assert!(h.query_all_by_label("Site").count() > 0, "a click on the header opens Game details");
    if let Ok(dir) = std::env::var("PDFCRAFT_CHESS_SHOTS") {
        h.render().unwrap().save(std::path::Path::new(&dir).join("game-header.png")).unwrap();
    }
}
