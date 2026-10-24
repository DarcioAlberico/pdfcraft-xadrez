//! Chess ▸ PGN toolbar ▸ Source (T8) in the real shell (egui_kittest): a game of the book, read
//! in the Text tab, opened on the board at the move clicked; Show in PDF and Follow scrolling the
//! page to where a move was printed and outlining it, for a book's game and a captured one. The
//! PDF is built here: a heading and a game in Helvetica on page 1, a second page after it.

use egui::accesskit::Role;
use egui::{Modifiers, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use pdfcraft_engine::chess::pdfcraft_chessvision::doc::Inline;
use pdfcraft_ui_egui::{PdfCraftApp, RightPanel};

mod support;

/// The game printed on page 1, and the same as the board writes it.
const MOVES: &str = "1.e4 e5 2.Nf3 Nc6 3.Bb5 a6";
const MOVETEXT: &str = "1. e4 e5 2. Nf3 Nc6 3. Bb5 a6";

/// Two 600 × 800 pt pages: a heading and the game near the top of the first, a line of prose on
/// the second.
fn book() -> Vec<u8> {
    pdf(&[
        format!("BT /F1 24 Tf 60 760 Td (Chapter 1) Tj ET\nBT /F1 11 Tf 60 730 Td ({MOVES}) Tj ET\n"),
        "BT /F1 11 Tf 60 730 Td (The end of the chapter.) Tj ET\n".to_string(),
    ])
}

/// 600 × 800 pt pages with these contents, `F1` Helvetica.
fn pdf(pages: &[String]) -> Vec<u8> {
    let kids: Vec<String> = (0..pages.len()).map(|i| format!("{} 0 R", 4 + 2 * i)).collect();
    let mut objs = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        format!("<< /Type /Pages /Kids [{}] /Count {} /MediaBox [0 0 600 800] >>", kids.join(" "), pages.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_string(),
    ];
    for (i, content) in pages.iter().enumerate() {
        let contents = 5 + 2 * i;
        objs.push(format!("<< /Type /Page /Parent 2 0 R /Contents {contents} 0 R /Resources << /Font << /F1 3 0 R >> >> >>"));
        objs.push(format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()));
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

/// The app on the book, the Chess panel open on its Text tab, page 1 read. Frames are 0.1 s
/// apart, so two clicks a frame apart are a double-click.
fn harness() -> Harness<'static, PdfCraftApp> {
    harness_on(book())
}

/// [`harness`] on the PDF `bytes`.
fn harness_on(bytes: Vec<u8>) -> Harness<'static, PdfCraftApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 1000.0)).with_step_dt(0.1).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("book.pdf", None, bytes).unwrap();
        app.set_option("left", "closed").unwrap();
        app.run_inline = true;
        app
    });
    h.run_steps(4);
    support::wait_until(&mut h, "the page's text layer", |app| app.views[0].glyph_screen_pos(0, 0).is_some());
    h.state_mut().choose_right_panel(Some(RightPanel::Chess));
    h.run_steps(2);
    h.get_by_label("Text").click();
    h.run_steps(4);
    h.get_by_label("Recognised text of page 1");
    h
}

/// Where move `san` was printed, as the Text tab read it: (page, box in user space).
fn printed(h: &Harness<'static, PdfCraftApp>, san: &str) -> (usize, [f32; 4]) {
    let text = h.state().chess.text.as_ref().expect("the page was read");
    let page = &text.chess.pages[0];
    let m = page
        .blocks
        .iter()
        .flat_map(|b| b.inlines())
        .find_map(|i| if let Inline::Move(m) = i { (m.san == san).then_some(m) } else { None })
        .unwrap_or_else(|| panic!("{san} on the page"));
    let r = m.source_rect.expect("its box");
    (page.index as usize, [r.x0, r.y0, r.x1, r.y1])
}

/// The Text tab's label of move `shown` (as printed: `e5`, `a6`).
fn text_move<'a>(h: &'a Harness<'static, PdfCraftApp>, shown: &'a str) -> egui_kittest::Node<'a> {
    h.get_all_by_label(shown).find(|n| n.accesskit_node().role() == Role::Label).unwrap_or_else(|| panic!("{shown} in the Text tab"))
}

/// The board's notation's button for move `san`.
fn notation_move<'a>(h: &'a Harness<'static, PdfCraftApp>, san: &'a str) -> egui_kittest::Node<'a> {
    h.get_all_by_label(san).find(|n| n.accesskit_node().role() != Role::Label).unwrap_or_else(|| panic!("{san} in the notation"))
}

/// The Inspector's Chess panel in front again (opening a game brought the Board panel, a tab
/// beside it, to the front).
fn back_to_text(h: &mut Harness<'static, PdfCraftApp>) {
    let panel = pdfcraft_ui_egui::docking::Panel::Inspector;
    h.state_mut().docking.apply(craft_ui::docking::Action::Activate { panel }).expect("the Inspector comes to the front");
    h.run_steps(3);
    h.get_by_label("Recognised text of page 1");
}

/// Two clicks at `at`, a frame apart: a double-click.
fn double_click_at(h: &mut Harness<'static, PdfCraftApp>, at: Pos2) {
    h.hover_at(at);
    h.run_steps(1);
    for _ in 0..2 {
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        }
        h.step();
    }
    h.run_steps(3);
}

/// Press `key` as the desktop app gets it: the frame's raw input goes through `raw_input_hook`
/// first, where the chess scope takes its keys (kittest itself skips the hook).
fn host_key(h: &mut Harness<'static, PdfCraftApp>, key: egui::Key) {
    use eframe::App as _;
    let ctx = h.ctx.clone();
    for pressed in [true, false] {
        let mut raw = std::mem::take(h.input_mut());
        raw.events.push(egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
        h.state_mut().raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }
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

fn games(h: &Harness<'static, PdfCraftApp>) -> usize {
    h.state().chess.games.collection().len()
}

/// The place of the move last shown on the page.
fn last_shown(h: &Harness<'static, PdfCraftApp>) -> Option<(usize, [f32; 4], bool)> {
    h.state().chess.source.shown().map(|s| (s.page, s.rect, s.scrolled))
}

/// A double-click on a move of the Text tab opens the book's game on the board at that move,
/// with every move keeping where it was printed, as a step of the list Undo takes back; opening
/// the same untouched game again shows it again rather than adding it twice, at the move clicked;
/// once the game was changed, it is opened as a new game. The context menu does the same.
#[test]
fn a_move_of_the_text_tab_opens_its_game_on_the_board() {
    let mut h = harness();
    assert_eq!(games(&h), 1);
    let at = text_move(&h, "a6").rect().center();
    double_click_at(&mut h, at);

    assert_eq!(movetext(&h), MOVETEXT, "the walker's game");
    assert_eq!(shown(&h), "a6", "at the move clicked");
    assert_eq!(games(&h), 1, "the blank game gave way to it");
    {
        let game = &h.state().chess.board().game;
        let source = game.source(game.current()).expect("the move keeps its place");
        assert_eq!((source.page as usize, source.rect_user), printed(&h, "a6"));
        assert_eq!(h.state().chess.board().undo_label(), None, "opening a game is no step of the game");
    }
    // It is a step of the list: Undo takes it back out (the blank game is back), Redo puts it
    // back at the move clicked.
    assert!(h.state_mut().execute("edit.undo"));
    h.run_steps(2);
    assert_eq!(h.state().toast.as_ref().map(|t| t.0.as_str()), Some("Undid Open this game on the board"));
    assert_eq!((games(&h), movetext(&h).as_str()), (1, ""));
    assert!(h.state_mut().execute("edit.redo"));
    h.run_steps(2);
    assert_eq!((movetext(&h).as_str(), shown(&h).as_str()), (MOVETEXT, "a6"));
    // The first click of the double-click outlined the move on the page.
    assert_eq!(h.state().views[0].flash.map(|f| (f.0, f.1)), Some(printed(&h, "a6")));
    // The board's notation names the move shown.
    notation_move(&h, "a6");
    // Opened again, untouched: the same game, now at e5.
    back_to_text(&mut h);
    let at = text_move(&h, "e5").rect().center();
    double_click_at(&mut h, at);
    assert_eq!((games(&h), shown(&h).as_str()), (1, "e5"));
    // Changed (a move played at the end), then opened again from the menu: a new game.
    h.state_mut().chess.board_mut().game.go_to(usize::MAX);
    h.state_mut().chess.board_mut().play_uci_line(&["b5a4".to_owned()]);
    h.run_steps(2);
    assert!(movetext(&h).ends_with("a6 4. Ba4"));
    back_to_text(&mut h);
    text_move(&h, "e5").click_secondary();
    h.run_steps(2);
    h.get_by_label("Open this game on the board").click();
    h.run_steps(3);
    assert_eq!(games(&h), 2, "the changed game is kept, the book's opened beside it");
    assert_eq!((movetext(&h).as_str(), shown(&h).as_str()), (MOVETEXT, "e5"));
}

/// The page each move of the board's game was printed on, in PGN order.
fn move_pages(h: &Harness<'static, PdfCraftApp>) -> Vec<Option<u32>> {
    use pdfcraft_engine::chess::pdfcraft_chess::game::Token;
    let game = &h.state().chess.board().game;
    game.tokens().into_iter().filter_map(|t| if let Token::Move { node, .. } = t { Some(game.source(node).map(|s| s.page)) } else { None }).collect()
}

/// A game the book runs on onto the next page comes whole: a double-click on a move of page 1
/// reads page 2 too, and the board has the game to its last move there, each move keeping its
/// page. On page 2, whose moves have no position alone (the game began on page 1), a
/// double-click on one reads page 1 and opens the same game, at that move.
#[test]
fn a_game_running_on_to_the_next_page_opens_whole() {
    const WHOLE: &str = "1. e4 e5 2. Nf3 Nc6 3. d4 exd4 4. Nxd4 Nf6 5. Nc3 d6";
    let mut h = harness_on(pdf(&[
        "BT /F1 24 Tf 60 760 Td (Chapter 1) Tj ET\nBT /F1 11 Tf 60 730 Td (1.e4 e5 2.Nf3 Nc6 3.d4 exd4) Tj ET\n".to_string(),
        "BT /F1 11 Tf 60 730 Td (4.Nxd4 Nf6 5.Nc3 d6) Tj ET\n".to_string(),
    ]));
    let at = text_move(&h, "e5").rect().center();
    double_click_at(&mut h, at);
    assert_eq!(movetext(&h), WHOLE, "the game runs on to page 2");
    assert_eq!((shown(&h).as_str(), games(&h)), ("e5", 1));
    let pages: Vec<Option<u32>> = [0, 0, 0, 0, 0, 0, 1, 1, 1, 1].into_iter().map(Some).collect();
    assert_eq!(move_pages(&h), pages, "each move keeps its page");

    // Page 2 alone: its moves go on from page 1, so they have no position there.
    back_to_text(&mut h);
    h.state_mut().views[0].go_to_page(1);
    h.run_steps(4);
    h.get_by_label("Recognised text of page 2");
    assert!(h.state().chess.text.as_ref().is_some_and(|t| t.chess.games.is_empty()), "no game on page 2 alone");
    let at = text_move(&h, "d6").rect().center();
    double_click_at(&mut h, at);
    assert_eq!((movetext(&h).as_str(), shown(&h).as_str()), (WHOLE, "d6"));
    assert_eq!(games(&h), 1, "the same game, untouched, shown again");
    assert_eq!(move_pages(&h), pages);
}

/// A move in prose with no game, on its page or the pages around it, opens nothing and says so.
#[test]
fn a_move_with_no_game_around_it_says_so() {
    let mut h = harness_on(pdf(&[
        "BT /F1 11 Tf 60 730 Td (A page of prose.) Tj ET\n".to_string(),
        "BT /F1 11 Tf 60 730 Td (Black should answer 23...d5 at once.) Tj ET\n".to_string(),
    ]));
    h.state_mut().views[0].go_to_page(1);
    h.run_steps(4);
    h.get_by_label("Recognised text of page 2");
    let at = h.get_all_by_label_contains("d5").find(|n| n.accesskit_node().role() == Role::Label).expect("the move").rect().center();
    double_click_at(&mut h, at);
    assert_eq!(games(&h), 1);
    assert!(h.state().chess.board().game.is_empty(), "nothing was opened");
    assert_eq!(h.state().toast.as_ref().map(|t| t.0.as_str()), Some("No game was found for this move, on its page or the pages around it."));
}

/// With Follow on, walking the book's game walks the book: each move shown scrolls the page to
/// where it was printed (only when it is off screen) and outlines it. Show in PDF, in a move's
/// menu, does it once. A move with no place on the page shows nothing, and the switch says so.
#[test]
fn follow_scrolls_the_page_to_each_move_shown() {
    let mut h = harness();
    let at = text_move(&h, "e5").rect().center();
    double_click_at(&mut h, at);
    assert_eq!(shown(&h), "e5");
    // Off: walking the game leaves the page alone.
    let before = last_shown(&h);
    notation_move(&h, "Nc6").click();
    h.run_steps(2);
    assert_eq!((shown(&h).as_str(), last_shown(&h)), ("Nc6", before));
    // On (the rail's switch): the move shown at once, on screen already (the page scrolled a
    // little, its moves still in view), so outlined only: the page does not move.
    h.state_mut().views[0].key_scroll = 40.0;
    h.run_steps(3);
    let page_before = h.state().views[0].page_screen_rect(0).expect("page 1 on screen");
    h.get_by_label("Follow the move on the page (F)").click();
    h.run_steps(3);
    assert!(h.state().chess.source.follow());
    let (page, rect) = printed(&h, "Nc6");
    assert_eq!(last_shown(&h), Some((page, rect, false)));
    assert_eq!(h.state().views[0].page_screen_rect(0), Some(page_before), "not scrolled");
    assert_eq!(h.state().views[0].flash.map(|f| (f.0, f.1)), Some((page, rect)));
    // The page is turned to page 2: the next move scrolls back to page 1, where it is.
    h.state_mut().views[0].go_to_page(1);
    h.run_steps(4);
    assert_eq!(h.state().views[0].current, 1);
    notation_move(&h, "Bb5").click();
    h.run_steps(4);
    let (page, rect) = printed(&h, "Bb5");
    assert_eq!(last_shown(&h), Some((page, rect, true)), "scrolled to it");
    assert_eq!(h.state().views[0].current, 0, "back on page 1");
    let on_screen = h.state().views[0].page_screen_rect(0).is_some();
    assert!(on_screen, "page 1 is on screen");
    // The arrows walk the game (the click put the chess scope on): Follow follows.
    assert!(h.state().chess.in_scope());
    host_key(&mut h, egui::Key::ArrowLeft);
    assert_eq!(shown(&h), "Nc6");
    assert_eq!(last_shown(&h).map(|s| (s.0, s.1)), Some(printed(&h, "Nc6")));
    // Off again, and Show in PDF from a move's menu shows that move once.
    h.get_by_label("Follow the move on the page (F)").click();
    h.run_steps(2);
    assert!(!h.state().chess.source.follow());
    notation_move(&h, "e4").click_secondary();
    h.run_steps(2);
    let menu = h.get_all_by_label("Show in PDF").max_by(|a, b| a.rect().width().total_cmp(&b.rect().width())).expect("the menu item");
    menu.click();
    h.run_steps(3);
    assert_eq!(last_shown(&h).map(|s| (s.0, s.1)), Some(printed(&h, "e4")));
    // A move played on the board has no place: Show in PDF is off, Follow shows nothing.
    h.state_mut().chess.board_mut().game.go_to(usize::MAX);
    h.state_mut().chess.board_mut().play_uci_line(&["b5a4".to_owned()]);
    h.run_steps(2);
    let before = last_shown(&h);
    assert!(!h.state_mut().execute("chess.source.show"), "nothing to show");
    assert_eq!(h.state().toast.as_ref().map(|t| t.0.as_str()), Some("This move has no place on the page."));
    assert!(h.state_mut().execute("chess.source.follow"));
    h.run_steps(3);
    assert!(h.state().chess.source.follow());
    assert_eq!(last_shown(&h), before, "nothing shown");
    h.get_by_label("Follow the move on the page: this move has no place on the page (F)");
}

/// A captured game (T7) keeps where its moves were printed too, and Follow walks it the same way.
#[test]
fn a_captured_game_follows_its_moves_on_the_page() {
    let mut h = harness();
    h.state_mut().show_chess_board();
    h.run_steps(3);
    h.get_by_label("Capture (automatic) (X)").click();
    h.run_steps(3);
    let (a, b) = {
        let v = &h.state().views[0];
        let a = v.text_matches(0, "1.e4").first().expect("1.e4 on the page").start;
        let b = v.text_matches(0, "a6").last().expect("a6 on the page").end - 1;
        (v.glyph_screen_pos(0, a).unwrap(), v.glyph_screen_pos(0, b).unwrap())
    };
    h.hover_at(a);
    h.run_steps(1);
    h.drag_at(a);
    h.run_steps(1);
    for k in 1..=4 {
        h.hover_at(a + (b - a) * (k as f32 / 4.0));
        h.run_steps(1);
    }
    h.drop_at(b);
    h.run_steps(3);
    assert_eq!(movetext(&h), MOVETEXT, "captured");
    assert_eq!(h.state().chess.board().undo_label().as_deref(), Some("Capture line"));
    // Follow on: the move shown (the last captured) at once, then each move walked to.
    assert!(h.state_mut().execute("chess.source.follow"));
    h.run_steps(3);
    let game = &h.state().chess.board().game;
    let a6 = game.source(game.current()).expect("a captured move keeps its place");
    assert_eq!(last_shown(&h).map(|s| (s.0, s.1)), Some((a6.page as usize, a6.rect_user)));
    notation_move(&h, "Nf3").click();
    h.run_steps(3);
    let game = &h.state().chess.board().game;
    let nf3 = game.source(game.current()).expect("a captured move keeps its place");
    assert_eq!(shown(&h), "Nf3");
    assert_eq!(last_shown(&h).map(|s| (s.0, s.1)), Some((nf3.page as usize, nf3.rect_user)));
    assert_ne!(a6.rect_user, nf3.rect_user);
}
