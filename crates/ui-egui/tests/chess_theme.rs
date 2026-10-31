//! Board themes (plan HC4): a theme picked in Preferences, in the board panel's menu or through
//! the control channel's `board-theme` is used at once and kept with the preferences.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use pdfcraft_ui_egui::PdfCraftApp;

const PDF: &[u8] = b"%PDF-1.7
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 300 400] >> endobj
3 0 obj << /Type /Page /Parent 2 0 R >> endobj
trailer << /Root 1 0 R >>
%%EOF";

/// The app with a page open, after `settings` were restored; Preferences shown when `prefs`,
/// else the Board panel.
fn harness(settings: &str, prefs: bool) -> Harness<'static, PdfCraftApp> {
    let settings = settings.to_owned();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 1000.0)).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.restore(&settings);
        app.set_option("language", "en").unwrap();
        app.open_bytes("page.pdf", None, PDF.to_vec()).unwrap();
        app.set_option("left", "closed").unwrap();
        if prefs {
            app.set_option("dialog", "preferences").unwrap();
        }
        app
    });
    h.run_steps(4);
    if !prefs {
        h.state_mut().show_chess_board();
        h.run_steps(3);
    }
    h
}

/// With PDFCRAFT_CHESS_SHOTS set, a picture of the window there.
fn shot(h: &mut Harness<'static, PdfCraftApp>, name: &str) {
    if let Ok(dir) = std::env::var("PDFCRAFT_CHESS_SHOTS") {
        h.render().unwrap().save(std::path::Path::new(&dir).join(name)).unwrap();
    }
}

fn press(h: &mut Harness<'static, PdfCraftApp>, label: &str) {
    h.get_by_label(label).scroll_to_me();
    h.run_steps(2);
    h.get_by_label(label).hover();
    h.run_steps(1);
    h.get_by_label(label).click();
    h.run_steps(3);
}

#[test]
fn a_theme_picked_in_preferences_is_kept() {
    let mut h = harness("", true);
    assert_eq!(h.state().board_theme().id, "brown", "brown by default");
    press(&mut h, "Walnut");
    assert_eq!(h.state().board_theme().id, "walnut");
    shot(&mut h, "theme-preferences.png");
    let saved: serde_json::Value = serde_json::from_str(&h.state().persist()).unwrap();
    assert_eq!(saved["chess_board_theme"], "walnut");
    let h = harness(&saved.to_string(), true);
    assert_eq!(h.state().board_theme().id, "walnut", "read back next time");
    // A theme no longer known leaves the default.
    let h = harness(r#"{"chess_board_theme": "gone"}"#, true);
    assert_eq!(h.state().board_theme().id, "brown");
}

#[test]
fn the_board_panels_menu_and_the_control_channel_pick_a_theme() {
    let mut h = harness("", false);
    press(&mut h, "Board theme");
    press(&mut h, "Green");
    assert_eq!(h.state().board_theme().id, "green");
    h.state_mut().chess.board_mut().play_uci_line(&["e2e4".into(), "e7e5".into(), "g1f3".into()]);
    h.run_steps(3);
    shot(&mut h, "theme-green-board.png");
    h.state_mut().set_option("board-theme", "HIGH-CONTRAST").unwrap();
    assert_eq!(h.state().board_theme().id, "high-contrast");
    let e = h.state_mut().set_option("board-theme", "cburnett").unwrap_err();
    assert!(e.contains("brown") && e.contains("coral"), "{e}");
    assert_eq!(h.state().board_theme().id, "high-contrast", "a bad name changes nothing");
}

/// Click square `name` (`e2`) on the board, wherever its squares are drawn.
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

/// Plan HC6: with the coordinates beside the board and a double frame the squares move in, and a
/// click still lands on the square under it; the look is kept with the preferences.
#[test]
fn coordinates_beside_the_board_and_a_frame_keep_clicks_on_their_squares() {
    let mut h = harness("", false);
    let plain = h.get_by_label_contains("Board: ").rect();
    for (key, value) in [("board-coordinates", "outside"), ("board-frame", "double"), ("board-squares", "fill"), ("board-turn-dot", "on")] {
        h.state_mut().set_option(key, value).unwrap();
    }
    h.run_steps(3);
    let framed = h.get_by_label_contains("Board: ").rect();
    assert!(framed.width() < plain.width() - 10.0 && framed.left() > plain.left() + 10.0, "{plain:?} {framed:?}");
    assert!((framed.width() - framed.height()).abs() < 0.5, "still square: {framed:?}");
    click_square(&mut h, "e2");
    click_square(&mut h, "e4");
    assert_eq!(h.state().chess.board().game.movetext(), "1. e4");
    shot(&mut h, "look-outside-double.png");
    let saved: serde_json::Value = serde_json::from_str(&h.state().persist()).unwrap();
    assert_eq!(
        saved["chess_board_look"],
        serde_json::json!({ "coordinates": "outside", "frame": "double", "squares": "fill", "turn_dot": true, "last_move": "fill", "last_move_strength": 100 })
    );
    let h = harness(&saved.to_string(), false);
    assert_eq!(h.state().chess.board_look.to_json(), saved["chess_board_look"], "read back next time");
    let mut h = harness("", false);
    assert!(h.state_mut().set_option("board-frame", "triple").unwrap_err().contains("none, thin or double"));
}

/// Renders (wgpu) one at a time: two devices compiling shaders at once have crashed test runs.
static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The colour at `at` (in points) on the window as drawn now.
fn pixel(h: &mut Harness<'static, PdfCraftApp>, at: egui::Pos2) -> [u8; 3] {
    let image = h.render().unwrap();
    let scale = h.ctx.pixels_per_point();
    let [r, g, b, _] = image.get_pixel((at.x * scale) as u32, (at.y * scale) as u32).0;
    [r, g, b]
}

fn apart(a: [u8; 3], b: [u8; 3]) -> f32 {
    a.iter().zip(b).map(|(x, y)| (f32::from(*x) - f32::from(y)).powi(2)).sum::<f32>().sqrt()
}

/// The last move is marked as the look says and how strongly, always under the pieces: the pawn
/// that moved keeps its colours (it took the mark's tint before), its square shows the mark. The
/// choice is kept with the preferences.
#[test]
fn the_last_move_is_marked_under_the_piece_as_the_look_says() {
    let _gpu = GPU.lock().unwrap_or_else(|e| e.into_inner());
    let mut h = harness("", false);
    h.state_mut().set_option("board-theme", "green").unwrap();
    // A white pawn on a dark square, so that a tint over it shows.
    h.state_mut().chess.board_mut().play_uci_line(&["d2d4".into()]);
    h.run_steps(3);
    let board = h.get_by_label_contains("Board: ").rect();
    let cell = board.width() / 8.0;
    let square = |file: f32, rank: f32| egui::Rect::from_min_size(board.min + egui::vec2(cell * file, cell * (7.0 - rank)), egui::vec2(cell, cell));
    let (d4, d3) = (square(3.0, 3.0), square(3.0, 2.0));
    let piece = d4.center() + egui::vec2(0.0, cell * 0.12);
    let edge = d4.left_center() + egui::vec2(cell * 0.03, 0.0);
    let inside = d4.left_top() + egui::vec2(cell * 0.2, cell * 0.2);
    let look = |h: &mut Harness<'static, PdfCraftApp>, mark: &str, strength: &str| {
        h.state_mut().set_option("board-last-move", mark).unwrap();
        h.state_mut().set_option("board-last-move-strength", strength).unwrap();
        h.run_steps(2);
        (pixel(h, piece), pixel(h, edge), pixel(h, inside), pixel(h, d3.center()))
    };
    let (piece0, edge0, inside0, d3_0) = look(&mut h, "none", "100");
    assert!(apart(piece0, edge0) > 60.0, "the point sampled is on the pawn: {piece0:?} {edge0:?}");
    shot(&mut h, "last-move-none.png");

    let (p, e, i, _) = look(&mut h, "fill", "100");
    assert!(apart(p, piece0) < 8.0, "the pawn keeps its colours under the fill: {p:?} {piece0:?}");
    assert!(apart(e, edge0) > 40.0 && apart(i, inside0) > 40.0, "the square shows it: {e:?} {i:?}");
    shot(&mut h, "last-move-fill.png");
    let (p, e, _, _) = look(&mut h, "fill", "40");
    assert!(apart(p, piece0) < 8.0, "{p:?}");
    let full = apart(look(&mut h, "fill", "100").1, edge0);
    assert!(apart(e, edge0) > 10.0 && apart(e, edge0) < full * 0.6, "weaker at 40%: {e:?} {edge0:?}, full {full}");

    let (p, e, i, _) = look(&mut h, "border", "100");
    assert!(apart(p, piece0) < 8.0 && apart(e, edge0) > 40.0, "a line at the edge: {e:?}");
    assert!(apart(i, inside0) < 8.0, "nothing inside it: {i:?} {inside0:?}");
    shot(&mut h, "last-move-border.png");
    for mark in ["corners", "from-border", "border-fill"] {
        let (p, ..) = look(&mut h, mark, "100");
        assert!(apart(p, piece0) < 8.0, "{mark}: {p:?}");
        shot(&mut h, &format!("last-move-{mark}.png"));
    }
    let (_, e, _, d3_arrow) = look(&mut h, "arrow", "70");
    assert!(apart(e, edge0) < 8.0 && apart(d3_arrow, d3_0) > 40.0, "an arrow over d3, the squares left alone: {d3_arrow:?} {d3_0:?}");
    shot(&mut h, "last-move-arrow.png");

    let saved: serde_json::Value = serde_json::from_str(&h.state().persist()).unwrap();
    assert_eq!(
        (saved["chess_board_look"]["last_move"].as_str(), saved["chess_board_look"]["last_move_strength"].as_u64()),
        (Some("arrow"), Some(70))
    );
    let h2 = harness(&saved.to_string(), false);
    assert_eq!(h2.state().chess.board_look.to_json(), saved["chess_board_look"], "read back next time");
    assert!(h.state_mut().set_option("board-last-move", "glow").unwrap_err().contains("border-fill"));
    assert!(h.state_mut().set_option("board-last-move-strength", "5").unwrap_err().contains("10 to 100"));
    assert_eq!(h.state().chess.board_look.to_json(), saved["chess_board_look"], "a bad value changes nothing");
}

/// The mark and its strength are chosen in the board panel's palette menu and in Preferences.
#[test]
fn the_last_move_mark_is_chosen_in_the_menu_and_the_preferences() {
    use egui::accesskit::Role;
    let mut h = harness("", false);
    press(&mut h, "Board theme");
    shot(&mut h, "last-move-menu-closed.png");
    // A submenu in the menu, its button saying the mark chosen.
    h.get_by(|n| n.role() == Role::Button && n.label().is_some_and(|l| l.starts_with("Filled"))).click();
    h.run_steps(3);
    shot(&mut h, "last-move-menu.png");
    h.get_by_label("Corners").click();
    h.run_steps(3);
    assert_eq!(h.state().chess.board_look.to_json()["last_move"], "corners", "picked in the menu");
    assert!(h.query_by_label("Green").is_some(), "the menu stays open to try another");
    h.get_by_label("Green").click();
    h.run_steps(3);
    assert_eq!(h.state().board_theme().id, "green");
    assert!(h.query_by_label("Coral").is_none(), "a theme picked closes it");
    let mut h = harness("", true);
    h.get_by(|n| n.role() == Role::ComboBox && n.label().is_some_and(|l| l == "Last move")).scroll_to_me();
    h.run_steps(2);
    let list = h.get_by(|n| n.role() == Role::ComboBox && n.label().is_some_and(|l| l == "Last move"));
    assert_eq!(list.value().as_deref(), Some("Filled"), "it says the mark chosen");
    list.hover();
    h.run_steps(1);
    h.get_by(|n| n.role() == Role::ComboBox && n.label().is_some_and(|l| l == "Last move")).click();
    h.run_steps(2);
    h.get_by_label("Start outlined, end filled").click();
    h.run_steps(3);
    assert_eq!(h.state().chess.board_look.to_json()["last_move"], "from-border", "picked in the preferences");
    shot(&mut h, "last-move-preferences.png");
}

/// Plan HC5: every textured theme draws the board (the book's dark squares hatched as paths).
#[test]
fn textured_themes_draw_the_board() {
    let mut h = harness("", false);
    h.state_mut().chess.board_mut().play_uci_line(&["e2e4".into(), "e7e5".into()]);
    for id in ["wood", "marble", "paper", "book"] {
        h.state_mut().set_option("board-theme", id).unwrap();
        h.run_steps(3);
        assert!(h.query_by_label_contains("Board: ").is_some(), "{id}: the board is drawn");
        assert!(h.query_by_label_contains("The board couldn't be drawn").is_none(), "{id}");
        shot(&mut h, &format!("theme-{id}.png"));
    }
}

/// Plan HC8: the move list shows the pieces as figurines by default without making its rows
/// taller, names every move in words, and keeps the choice of letters with the preferences.
#[test]
fn figurines_keep_the_rows_and_moves_are_named_in_words() {
    let mut h = harness("", false);
    h.state_mut().chess.board_mut().play_uci_line(&["e2e4".into(), "e7e5".into(), "g1f3".into(), "b8c6".into()]);
    h.run_steps(4);
    let pawn = h.get_by_label("e4").rect();
    let knight = h.get_by_label("knight f3").rect();
    assert!((knight.height() - pawn.height()).abs() < 0.5, "a figurine keeps the row: {pawn:?} {knight:?}");
    assert!(knight.width() > pawn.width(), "the figurine takes room beside the square: {pawn:?} {knight:?}");
    shot(&mut h, "move-list-figurines.png");
    for mode in ["letters", "english"] {
        h.state_mut().set_option("move-pieces", mode).unwrap();
        h.run_steps(2);
        assert!(h.query_by_label("knight c6").is_some(), "{mode}: named in words");
    }
    h.state_mut().set_option("language", "pt-BR").unwrap();
    h.state_mut().set_option("move-pieces", "letters").unwrap();
    h.run_steps(2);
    assert!(h.query_by_label("cavalo f3").is_some(), "named in the interface's language");
    shot(&mut h, "move-list-letters-pt.png");
    let saved: serde_json::Value = serde_json::from_str(&h.state().persist()).unwrap();
    assert_eq!(saved["chess_move_pieces"], "letters");
    assert!(h.state_mut().set_option("move-pieces", "pictures").is_err());
}

/// Plan HC12: a quality sign is a chip, the move named with its sign; a move with drawings has a dot named
/// by them, and the pointer over it shows them on the board; the comment on the move shown can
/// be large under the board.
#[test]
fn marks_on_moves_and_the_comment_large() {
    use pdfcraft_engine::chess::pdfcraft_chess::game::Game;
    let mut h = harness("", false);
    let (game, _) = Game::from_pgn("1. e4 e5 2. Nf3?! { [%cal Gb5c7,Rd1h5] [%csl Re4] A plan. } Nc6 *").unwrap();
    h.state_mut().chess.board_mut().game = game;
    h.run_steps(3);
    assert!(h.query_by_label("knight f3?!").is_some(), "the move keeps its sign in its name");
    assert!(h.query_by_label("Dubious move").is_none(), "the chip is not a second name beside the palette's");
    let dot = h.get_by_label("2 arrows, 1 ring");
    dot.hover();
    h.run_steps(2);
    assert!(h.state().chess.board().hovered_move().is_some(), "the board shows that move's drawings");
    shot(&mut h, "marks-on-moves.png");
    assert_eq!(h.query_all_by_label("A plan.").count(), 1);
    h.state_mut().set_option("comment-focus", "on").unwrap();
    let nf3 = h.state().chess.board().hovered_move().unwrap();
    h.state_mut().chess.board_mut().game.go_to_node(nf3);
    h.run_steps(3);
    assert_eq!(h.query_all_by_label("A plan.").count(), 2, "and large under the board");
    shot(&mut h, "comment-focus.png");
}

/// Plan HC9: in columns, a move number's two moves share a row; a variation breaks the table and
/// Black's move after it starts a row of its own with "…"; the large text makes the rows taller;
/// the choice is kept. A long game is drawn in both layouts, its frame time measured.
#[test]
fn columns_pair_the_moves_and_the_text_size_scales() {
    use pdfcraft_engine::chess::pdfcraft_chess::game::Game;
    let mut h = harness("", false);
    let (game, _) = Game::from_pgn("1. e4 e5 2. Nf3 (2. c3 d5) Nc6 3. Bb5 *").unwrap();
    h.state_mut().chess.board_mut().game = game;
    h.state_mut().set_option("move-list-layout", "columns").unwrap();
    h.run_steps(3);
    let y = |h: &Harness<'static, PdfCraftApp>, name: &str| h.get_by_label(name).rect().center().y;
    assert!((y(&h, "e4") - y(&h, "e5")).abs() < 1.0, "one row for 1. e4 e5");
    assert!(y(&h, "knight f3") > y(&h, "e5") + 2.0, "2. Nf3 on the next row");
    assert!(y(&h, "c3") > y(&h, "knight f3") + 2.0, "the variation under it");
    assert!(y(&h, "knight c6") > y(&h, "d5") + 2.0, "the table resumes after the variation");
    assert!(h.query_by_label("…").is_some(), "with … for White's move");
    assert!((y(&h, "bishop b5") - y(&h, "knight c6")).abs() > 2.0, "3. Bb5 on its own row");
    let x = |h: &Harness<'static, PdfCraftApp>, name: &str| h.get_by_label(name).rect().left();
    assert!((x(&h, "e5") - x(&h, "knight c6")).abs() < 1.0, "Black's moves in one column");
    shot(&mut h, "move-list-columns.png");
    let normal = h.get_by_label("e4").rect().height();
    h.state_mut().set_option("move-list-size", "large").unwrap();
    h.run_steps(3);
    assert!(h.get_by_label("e4").rect().height() > normal + 2.0, "large text, taller rows");
    let saved: serde_json::Value = serde_json::from_str(&h.state().persist()).unwrap();
    assert_eq!(saved["chess_move_list"], serde_json::json!({ "layout": "columns", "size": "large" }));
    assert!(h.state_mut().set_option("move-list-layout", "table").is_err());
    // A long game: 1,000 moves of knights going out and back.
    let mut long = Game::standard();
    let cycle = ["g1f3", "g8f6", "f3g1", "f6g8"];
    for i in 0..1000 {
        long.play_uci(cycle[i % 4]).unwrap();
    }
    h.state_mut().chess.board_mut().game = long;
    for layout in ["flow", "columns"] {
        h.state_mut().set_option("move-list-layout", layout).unwrap();
        h.run_steps(2);
        let started = std::time::Instant::now();
        h.run_steps(5);
        eprintln!("{layout}: {:?} a frame with 1,000 moves (debug build)", started.elapsed() / 5);
    }
}

/// Plan HC13: a move the pointer rests on shows its position after the delay, not before, and
/// the board keeps the move it shows; off in the preferences, nothing shows.
#[test]
fn a_move_rested_on_shows_its_position_and_leaves_the_board() {
    use pdfcraft_engine::chess::pdfcraft_chess::game::Game;
    let mut h = harness("", false);
    let (game, _) = Game::from_pgn("1. e4 e5 (1... c5 2. Nf3) 2. Nf3 *").unwrap();
    h.state_mut().chess.board_mut().game = game;
    h.run_steps(3);
    let shown = h.state().chess.board().game.current();
    h.get_by_label("c5").hover();
    h.run_steps(2);
    assert_eq!(h.query_all_by_label_contains("Position after").count(), 0, "not before the delay");
    for _ in 0..40 {
        h.step();
    }
    h.get_by_label("c5").hover();
    h.run_steps(2);
    assert!(h.query_by_label("Position after 1…c5").is_some(), "the variation move's position");
    assert_eq!(h.state().chess.board().game.current(), shown, "the board keeps its move");
    shot(&mut h, "move-preview.png");
    h.state_mut().set_option("move-preview", "off").unwrap();
    h.run_steps(3);
    assert_eq!(h.query_all_by_label_contains("Position after").count(), 0, "off");
    let saved: serde_json::Value = serde_json::from_str(&h.state().persist()).unwrap();
    assert_eq!(saved["chess_move_preview"], false);
}
