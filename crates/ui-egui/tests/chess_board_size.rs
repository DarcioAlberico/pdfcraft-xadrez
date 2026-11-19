//! Chess ▸ the Board tab's board, larger and smaller (2026-11-19: "na aba Tabuleiro gostaria de
//! poder aumentar o tamanho do diagrama"). The board is automatic until the user drags the grip
//! under it or picks Larger board or Smaller board in its ⋯ menu; the side chosen is kept between
//! sessions, and a double-click on the grip or Board size: automatic gives the automatic side
//! back. At the reference screen (1366 × 768) and at the user's short window (1000 × 615), where
//! the automatic board was about 150 points with the moves' region mostly empty under it.

use egui::{Pos2, Rect, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use pdfcraft_engine::chess::pdfcraft_chess::game::Game;
use pdfcraft_ui_egui::PdfCraftApp;

/// The grip's name: its tooltip.
const GRIP: &str = "Drag to change the board's size";

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

/// Pin "no piece model" for this test process (plan A9-Q1a), as the chess tests do.
fn no_piece_model() {
    let (none, _) = pdfcraft_engine::chess::pdfcraft_chessvision::models::ChessModels::from_files(Vec::new());
    pdfcraft_engine::chess::install_models(Some(none));
}

/// Seconds between two frames: kittest's 0.25 s would put the two clicks of a double-click (one
/// event of the pointer a frame) 0.5 s apart, over egui's 0.3 s.
const STEP: f32 = 0.05;

/// The app in a window of `size` with the settings `saved` restored (a session before), the Board
/// tab shown and the game `pgn` at its end.
fn harness(size: egui::Vec2, saved: Option<String>, pgn: &str) -> Harness<'static, PdfCraftApp> {
    no_piece_model();
    let mut h = Harness::builder().with_size(size).with_step_dt(STEP).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        if let Some(json) = &saved {
            app.restore(json);
        }
        app.set_option("language", "en").unwrap();
        app.open_bytes("page.pdf", None, one_page()).unwrap();
        app.run_inline = true;
        app
    });
    h.run_steps(4);
    h.state_mut().show_chess_board();
    h.run_steps(3);
    let (mut game, stopped) = Game::from_pgn(pgn).unwrap();
    assert_eq!(stopped, None);
    game.go_to(usize::MAX);
    h.state_mut().chess.board_mut().set_game(game);
    h.run_steps(3);
    assert!(h.state().chess_board_shown(), "the Board tab shows");
    h
}

/// The Board tab's regions as the last frame measured them: header, board, navigation, engine,
/// trail, moves, bar.
fn regions(h: &Harness<'static, PdfCraftApp>) -> [Rect; 7] {
    h.ctx.data(|d| d.get_temp::<[Rect; 7]>(egui::Id::new("pdfcraft-chess-board-regions"))).expect("the Board tab is drawn")
}

/// The board's side as the last frame drew it.
fn side(h: &Harness<'static, PdfCraftApp>) -> f32 {
    regions(h)[1].width()
}

/// The Chess column, as the last frame drew it.
fn chess_column(h: &Harness<'static, PdfCraftApp>) -> Rect {
    h.ctx.data(|d| d.get_temp::<Rect>(egui::Id::new("pdfcraft-chess-column"))).expect("the Chess column is shown")
}

/// The settings' `chess_board_side`, as they would be saved now.
fn saved_side(h: &Harness<'static, PdfCraftApp>) -> serde_json::Value {
    let v: serde_json::Value = serde_json::from_str(&h.state().persist()).unwrap();
    v["chess_board_side"].clone()
}

/// The grip's rectangle (a splitter; its tooltip, while shown, is a label of the same name).
fn grip(h: &Harness<'static, PdfCraftApp>) -> Rect {
    h.get_by_role_and_label(egui::accesskit::Role::Splitter, GRIP).rect()
}

/// The board's squares (the grid's place on the screen).
fn squares(h: &Harness<'static, PdfCraftApp>) -> Rect {
    h.get_by_label_contains("Board, ").rect()
}

/// Press the primary button at `from`, move to `to` in steps and let go there. The button is held
/// a second first, so that the drag begins where it was pressed: egui starts a drag once the
/// pointer has moved 6 points, and the board takes the square under the pointer then, which on a
/// board of 96 points (12-point squares) may be the next one.
fn drag(h: &mut Harness<'static, PdfCraftApp>, from: Pos2, to: Pos2) {
    h.hover_at(from);
    h.run_steps(1);
    h.event(egui::Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE });
    // A second held: egui's longest click is 0.8 s.
    h.run_steps((1.0 / STEP) as usize);
    for k in 1..=6u8 {
        h.event(egui::Event::PointerMoved(from + (to - from) * (f32::from(k) / 6.0)));
        h.run_steps(1);
    }
    h.event(egui::Event::PointerButton { pos: to, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE });
    h.run_steps(3);
}

/// Drag the grip `dy` points down (up when negative), from its middle.
fn drag_grip(h: &mut Harness<'static, PdfCraftApp>, dy: f32) {
    let at = grip(h).center();
    drag(h, at, at + vec2(0.0, dy));
}

/// Double-click the grip.
fn double_click_grip(h: &mut Harness<'static, PdfCraftApp>) {
    let at = grip(h).center();
    h.hover_at(at);
    h.run_steps(1);
    for pressed in [true, false, true, false] {
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
    }
    h.run_steps(3);
}

/// Pick `item` in the board's ⋯ menu.
fn menu_item(h: &mut Harness<'static, PdfCraftApp>, item: &str) {
    h.get_by_label("More board actions").click();
    h.run_steps(2);
    h.get_by_label(item).click();
    h.run_steps(3);
}

/// Whether the board's ⋯ menu has `item` greyed out (the menu opened and closed again).
fn menu_item_off(h: &mut Harness<'static, PdfCraftApp>, item: &str) -> bool {
    h.get_by_label("More board actions").click();
    h.run_steps(2);
    let off = h.get_by_label(item).accesskit_node().is_disabled();
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    off
}

/// Where the Board tab's regions run outside the Chess column (their bottoms past the window's
/// too), and whether the moves keep three rows.
fn outside(h: &Harness<'static, PdfCraftApp>) -> Vec<String> {
    let column = chess_column(h);
    let window = h.ctx.content_rect();
    let mut out: Vec<String> = regions(h)
        .iter()
        .zip(["header", "board", "navigation", "engine", "trail", "moves", "bar"])
        .filter(|(r, _)| r.height() > 0.0 && !(column.contains_rect(r.shrink(0.5)) && window.contains_rect(r.shrink(0.5))))
        .map(|(r, name)| format!("the {name} {r:?} outside {column:?}"))
        .collect();
    let moves = regions(h)[5];
    if moves.height() < 3.0 * 18.0 {
        out.push(format!("the moves {moves:?} under three rows"));
    }
    out
}

/// A picture of the window, when `PDFCRAFT_CHESS_SHOTS` names a folder.
fn shot(h: &mut Harness<'static, PdfCraftApp>, name: &str) {
    if let Ok(dir) = std::env::var("PDFCRAFT_CHESS_SHOTS") {
        h.render().unwrap().save(std::path::Path::new(&dir).join(name)).unwrap();
    }
}

/// [`shot`] in the light theme, then the dark one (and back to light).
fn shot_light_and_dark(h: &mut Harness<'static, PdfCraftApp>, name: &str) {
    if std::env::var("PDFCRAFT_CHESS_SHOTS").is_err() {
        return;
    }
    shot(h, &format!("{name}-light.png"));
    h.state_mut().set_option("theme", "dark").unwrap();
    h.run_steps(3);
    shot(h, &format!("{name}-dark.png"));
    h.state_mut().set_option("theme", "light").unwrap();
    h.run_steps(3);
}

/// The user's short window (1000 × 615): the automatic board is small and the moves' region
/// mostly empty. The grip is under the board, never over its squares; dragged down, the board
/// grows past the automatic side, dragged up it shrinks, and the regions stay inside the column
/// with three rows of moves at least. The side chosen is in the settings and comes back in the
/// next session; a double-click on the grip gives the automatic side back.
#[test]
fn at_the_users_window_the_grip_makes_the_board_larger_and_smaller() {
    let size = egui::vec2(1000.0, 615.0);
    let mut h = harness(size, None, "1. e4 e5 *");
    let column = chess_column(&h);
    assert!((column.width() - 360.0).abs() <= 2.0, "the Chess column beside the page: {column:?}");
    let auto = side(&h);
    assert!(auto < 200.0, "the automatic board is small in a short window: {auto}");
    assert_eq!(saved_side(&h), serde_json::Value::Null, "automatic until the user sizes it");
    // The grip: under the board's squares, as wide as the board, a few points tall.
    let (g, board, nav) = (grip(&h), squares(&h), regions(&h)[2]);
    assert!(g.min.y >= board.max.y - 0.01 && g.min.y >= regions(&h)[1].max.y - 0.01, "under the board: {g:?} {board:?}");
    assert!(g.height() <= 4.5 && nav.contains_rect(g), "the navigation row's top edge: {g:?} {nav:?}");
    assert!((g.width() - regions(&h)[1].width()).abs() < 0.5, "as wide as the board: {g:?}");
    shot_light_and_dark(&mut h, "board-size-1000x615-auto");
    // Down: larger than automatic, the board's foot following the pointer.
    drag_grip(&mut h, 90.0);
    let larger = side(&h);
    assert!((larger - (auto + 90.0)).abs() < 1.0, "{auto} + 90 = {larger}");
    assert!(outside(&h).is_empty(), "{:#?}", outside(&h));
    let saved = saved_side(&h);
    assert!(saved.as_f64().is_some_and(|s| (s as f32 - larger).abs() < 0.1), "saved {saved}, drawn {larger}");
    shot_light_and_dark(&mut h, "board-size-1000x615-larger");
    // As far down as the pointer goes: the column keeps three rows of moves under it.
    drag_grip(&mut h, 400.0);
    let most = side(&h);
    assert!(most > larger && outside(&h).is_empty(), "{most}: {:#?}", outside(&h));
    shot_light_and_dark(&mut h, "board-size-1000x615-largest");
    // Up: smaller.
    drag_grip(&mut h, -60.0);
    let smaller = side(&h);
    assert!((smaller - (most - 60.0)).abs() < 1.0, "{most} − 60 = {smaller}");
    // The next session draws the side chosen.
    let settings = h.state().persist();
    drop(h);
    let mut h = harness(size, Some(settings), "1. e4 e5 *");
    assert!((side(&h) - smaller).abs() < 0.2, "the side comes back: {} not {smaller}", side(&h));
    // A double-click on the grip: automatic again.
    double_click_grip(&mut h);
    assert!((side(&h) - auto).abs() < 0.01, "automatic: {} not {auto}", side(&h));
    assert_eq!(saved_side(&h), serde_json::Value::Null);
}

/// The board's ⋯ menu, the keyboard's way (and a screen reader's) to what the grip does: Larger
/// board and Smaller board step it by about 10 %, Board size: automatic gives the automatic side
/// back, and each is greyed out when it has nothing to do.
#[test]
fn the_boards_menu_makes_it_larger_smaller_and_automatic() {
    let mut h = harness(egui::vec2(1000.0, 615.0), None, "1. e4 e5 *");
    let auto = side(&h);
    assert!(menu_item_off(&mut h, "Board size: automatic"), "already automatic");
    menu_item(&mut h, "Larger board");
    let larger = side(&h);
    assert!((larger - auto * 1.1).abs() < 0.1, "{auto} × 1.1 = {larger}");
    assert!(saved_side(&h).as_f64().is_some(), "chosen: {}", saved_side(&h));
    menu_item(&mut h, "Larger board");
    assert!((side(&h) - larger * 1.1).abs() < 0.1, "{larger} × 1.1 = {}", side(&h));
    menu_item(&mut h, "Smaller board");
    menu_item(&mut h, "Smaller board");
    assert!((side(&h) - auto).abs() < 0.2, "two steps down from two up: {} not {auto}", side(&h));
    assert!(!menu_item_off(&mut h, "Board size: automatic"));
    menu_item(&mut h, "Board size: automatic");
    assert!((side(&h) - auto).abs() < 0.01, "automatic: {} not {auto}", side(&h));
    assert_eq!(saved_side(&h), serde_json::Value::Null);
    // At the smallest board, Smaller board has nothing to do; at the largest, Larger board.
    h.state_mut().set_option("board-side", "96").unwrap();
    h.run_steps(3);
    assert!((side(&h) - 96.0).abs() < 0.01 && menu_item_off(&mut h, "Smaller board"), "{}", side(&h));
    h.state_mut().set_option("board-side", "1200").unwrap();
    h.run_steps(3);
    assert!(menu_item_off(&mut h, "Larger board") && outside(&h).is_empty(), "{}: {:#?}", side(&h), outside(&h));
    assert!(h.state_mut().set_option("board-side", "big").is_err());
}

/// At the reference screen (1366 × 768): the grip shrinks the board and grows it back, the
/// regions stay inside the column, and a picture of it larger.
#[test]
fn at_1366_the_grip_makes_the_board_smaller_and_larger() {
    let mut h = harness(egui::vec2(1366.0, 768.0), None, "1. e4 e5 *");
    let auto = side(&h);
    drag_grip(&mut h, -50.0);
    let smaller = side(&h);
    assert!((smaller - (auto - 50.0)).abs() < 1.0, "{auto} − 50 = {smaller}");
    drag_grip(&mut h, 120.0);
    let larger = side(&h);
    assert!(larger >= smaller + 49.0 && larger >= auto - 0.01, "{smaller} → {larger} (automatic {auto})");
    assert!(outside(&h).is_empty(), "{:#?}", outside(&h));
    shot(&mut h, "board-size-1366x768-larger.png");
}

/// The grip takes nothing from the board: a piece picked up a point or two above the board's
/// bottom edge, right over the grip, and one dropped there, are played; the board keeps its size.
#[test]
fn a_piece_dragged_at_the_boards_bottom_edge_is_played() {
    for size in [egui::vec2(1000.0, 615.0), egui::vec2(1366.0, 768.0)] {
        let mut h = harness(size, None, "1. e4 e5 *");
        let before = side(&h);
        let shown = |h: &Harness<'static, PdfCraftApp>| {
            let game = h.state().chess.board().game();
            game.san(game.current()).to_owned()
        };
        // The squares' centres, white at the foot: file 0–7, rank 1–8.
        let at = |h: &Harness<'static, PdfCraftApp>, file: f32, rank: f32| {
            let b = squares(h);
            b.min + vec2(b.width() * (file + 0.5) / 8.0, b.height() * (8.5 - rank) / 8.0)
        };
        // Picked up 2 points above the edge: the knight on g1 to f3.
        let edge = squares(&h).max.y - 2.0;
        let g1 = Pos2::new(at(&h, 6.0, 1.0).x, edge);
        let f3 = at(&h, 5.0, 3.0);
        drag(&mut h, g1, f3);
        assert_eq!(shown(&h), "Nf3", "{size:?}: picked up at the edge");
        // Black answers; then dropped 2 points above the edge: the knight back to g1.
        let (c6, b8) = (at(&h, 2.0, 6.0), at(&h, 1.0, 8.0));
        drag(&mut h, b8, c6);
        assert_eq!(shown(&h), "Nc6");
        let f3 = at(&h, 5.0, 3.0);
        let g1 = Pos2::new(at(&h, 6.0, 1.0).x, squares(&h).max.y - 2.0);
        drag(&mut h, f3, g1);
        assert_eq!(shown(&h), "Ng1", "{size:?}: dropped at the edge");
        assert!((side(&h) - before).abs() < 0.01, "{size:?}: the board kept its size");
        assert_eq!(saved_side(&h), serde_json::Value::Null, "{size:?}: still automatic");
    }
}
