//! Chess ▸ the Board tab's **In the book (n)** (plan A9-N3, finding A8-07; §2.9): where the
//! position the board shows stands in the book open — in the book's PGN and among the diagrams
//! read — and what the book plays from it, played again on the board's own position. Found from
//! what is already read, never by reading the book; the list takes the moves' place and is walked
//! with the keyboard.

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use pdfcraft_ui_egui::PdfCraftApp;

#[path = "common/gpu.rs"]
mod gpu;

/// The start position in the Chess Merida layout, top rank first.
const MERIDA_START: [&str; 8] = ["tMvWlVmT", "OoOoOoOo", " + + + +", "+ + + + ", " + + + +", "+ + + + ", "pPpPpPpP", "RnBqKbNr"];

/// Two games reaching the position after 1.e4 e5 2.Nf3 Nc6 by two move orders (the second
/// transposes), a sign on the first's reply, and the second's variation playing it too.
const BOOK_PGN: &str = "[Event \"Open\"]\n\n1.e4 e5 2.Nf3 Nc6 3.Bb5 $1 a6 *\n\n[Event \"Reti\"]\n\n1.Nf3 Nc6 2.e4 e5 3.Bc4 (3.Bb5) *\n";

/// A 600 × 800 pt page with the start position in a diagram font, then a blank page: a book of
/// two pages.
fn book() -> Vec<u8> {
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
    let mut s = pdfcraft_engine::Session::new();
    let id = s.open("book.pdf", None, std::sync::Arc::new(out), None).unwrap();
    s.apply(id, pdfcraft_engine::Edit::InsertBlankPage { at: 1, width: 600.0, height: 800.0 }).unwrap();
    s.save_bytes(id).unwrap().to_vec()
}

/// The app at the reference screen (plan A9 §0, 1366 × 768) with [`book`] open and its Board tab
/// shown, its jobs run inline, and no piece model whatever the machine has.
fn harness() -> Harness<'static, PdfCraftApp> {
    let (none, _) = pdfcraft_engine::chess::pdfcraft_chessvision::models::ChessModels::from_files(Vec::new());
    pdfcraft_engine::chess::install_models(Some(none));
    let bytes = book();
    let mut h = Harness::builder().with_size(egui::vec2(1366.0, 768.0)).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("book.pdf", None, bytes).unwrap();
        app.run_inline = true;
        app
    });
    h.run_steps(4);
    h.state_mut().show_chess_board();
    h.run_steps(3);
    h
}

/// Put the game of `pgn` on the board, at its last move.
fn put_game(h: &mut Harness<'static, PdfCraftApp>, pgn: &str) {
    let (mut game, stopped) = pdfcraft_engine::chess::pdfcraft_chess::game::Game::from_pgn(pgn).unwrap();
    assert_eq!(stopped, None);
    game.go_to(usize::MAX);
    h.state_mut().chess.board_mut().set_game(game);
    h.run_steps(3);
}

/// The move shown, as its SAN.
fn shown(h: &Harness<'static, PdfCraftApp>) -> String {
    let game = h.state().chess.board().game();
    game.san(game.current()).to_owned()
}

/// Press `key` as the desktop app gets it: through `raw_input_hook` first, where the chess scope
/// takes its keys (kittest itself skips the hook).
fn host_key(h: &mut Harness<'static, PdfCraftApp>, modifiers: egui::Modifiers, key: egui::Key) {
    use eframe::App as _;
    let ctx = h.ctx.clone();
    for pressed in [true, false] {
        let mut raw = std::mem::take(h.input_mut());
        raw.events.push(egui::Event::ModifiersChanged(if pressed { modifiers } else { egui::Modifiers::NONE }));
        raw.events.push(egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers });
        h.state_mut().raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }
    h.run_steps(2);
}

/// The name of the widget with the keyboard.
fn focused(h: &Harness<'static, PdfCraftApp>) -> String {
    h.query_all_by(|n| n.is_focused()).find_map(|n| n.accesskit_node().label()).unwrap_or_default()
}

/// A picture of the window, when `PDFCRAFT_CHESS_SHOTS` names a folder.
fn shot(h: &mut Harness<'static, PdfCraftApp>, name: &str) {
    if let Ok(dir) = std::env::var("PDFCRAFT_CHESS_SHOTS") {
        let _gpu = gpu::gpu();
        h.render().unwrap().save(std::path::Path::new(&dir).join(name)).unwrap();
    }
}

/// Plan A9-N3 (the card's check): with the book's PGN, the position two move orders reach is in
/// the book twice, the transposition counted. In the book (2) opens the list in the moves' place:
/// what the book plays there (its main line's reply first, with the author's sign and how many
/// games play it) and where (each game at its move). Walked with the keyboard: Tab from "‹ Moves"
/// reaches the book's reply, Enter plays it on the board and the list follows the new position;
/// a game opens on the board at its move; Esc goes back to the moves.
#[test]
fn a_position_two_move_orders_reach_is_found_twice_and_the_books_reply_is_played() {
    let mut h = harness();
    let id = h.state().views[0].id;
    h.state_mut().set_book_pgn(id, "book.pgn", BOOK_PGN.as_bytes());
    h.run_steps(3);
    put_game(&mut h, "1. e4 e5 2. Nf3 Nc6 *");
    // The bar holds its four items at the reference screen's 360 pt column.
    let bar = h.ctx.data(|d| d.get_temp::<[egui::Rect; 7]>(egui::Id::new("pdfcraft-chess-board-regions"))).expect("the Board tab is drawn")[6];
    for name in ["Game", "Comment", "In the book (2)", "FEN/PGN"] {
        assert!(bar.contains_rect(h.get_by_label(name).rect().shrink(0.5)), "{name} in the bar {bar:?}");
    }
    h.get_by_label("In the book (2)").click();
    h.run_steps(3);
    assert!(h.query_by_label("Moves").is_none(), "the list in the moves' place");
    h.get_by_label("Found: 2 · transpositions: 1");
    h.get_by_label("The book's PGN · Pages read: 0 of 2");
    // What the book plays: Bb5 on the first game's main line and in the second's variation, with
    // the first game's sign; Bc4 on the second's main line.
    h.get_by_label("Play 3. bishop b5 on the board, Good move, Main line, Games: 2");
    h.get_by_label("Play 3. bishop c4 on the board, Main line");
    // Where: each game at its move, the second by another order.
    h.get_by_label("Game 1 · Open: 2… knight c6, Main line");
    h.get_by_label("Game 2 · Reti: 2… e5, Main line, transposition");
    shot(&mut h, "n3-in-the-book-light.png");
    h.state_mut().set_option("theme", "dark").unwrap();
    h.run_steps(3);
    shot(&mut h, "n3-in-the-book-dark.png");
    h.state_mut().set_option("theme", "light").unwrap();
    h.run_steps(3);
    // The keyboard: from the list's switch (This position | By material, plan A9-N16b), Tab
    // reaches the book's first reply, Enter plays it.
    h.get_by_label("By material").focus();
    h.run_steps(1);
    host_key(&mut h, egui::Modifiers::NONE, egui::Key::Tab);
    assert!(focused(&h).starts_with("Play 3. bishop b5"), "{}", focused(&h));
    // ↑ and ↓ go from row to row (plan A9-N3b), not through the game.
    host_key(&mut h, egui::Modifiers::NONE, egui::Key::ArrowDown);
    assert!(focused(&h).starts_with("Play 3. bishop c4"), "{}", focused(&h));
    host_key(&mut h, egui::Modifiers::NONE, egui::Key::ArrowDown);
    assert!(focused(&h).starts_with("Game 1 · Open"), "{}", focused(&h));
    host_key(&mut h, egui::Modifiers::NONE, egui::Key::Home);
    assert!(focused(&h).starts_with("Play 3. bishop b5"), "{}", focused(&h));
    assert_eq!(shown(&h), "Nc6", "the game did not move");
    host_key(&mut h, egui::Modifiers::NONE, egui::Key::Enter);
    assert_eq!(shown(&h), "Bb5", "played on the board");
    assert_eq!(h.state().chess.board().game().movetext(), "1. e4 e5 2. Nf3 Nc6 3. Bb5");
    // The list follows: both games reach it (the second in its variation), and the book goes on
    // with a6.
    h.get_by_label("In the book (2)");
    h.get_by_label("Play 3… a6 on the board, Main line");
    // A game of the PGN opens on the board at its move: Enter on it (the keyboard brings it into
    // sight in the list, which scrolls).
    h.get_by_label("Game 2 · Reti: 3. bishop b5, Variation, transposition").focus();
    h.run_steps(2);
    host_key(&mut h, egui::Modifiers::NONE, egui::Key::Enter);
    h.run_steps(2);
    assert_eq!(h.state().chess.board().game().tag("Event"), Some("Reti"));
    assert_eq!(shown(&h), "Bb5");
    // Esc goes back to the moves.
    h.get_by_label("In the book (2)").focus();
    h.run_steps(1);
    host_key(&mut h, egui::Modifiers::NONE, egui::Key::Escape);
    h.get_by_label("Moves");
}

/// Plan A9-N3, the budget rule: with nothing of the book read — no PGN, no page detected — the
/// list says to read it and nothing is read for it. Once Detect diagrams read the page, the
/// diagram showing the position is found, and shown on its page from the list; a position the
/// book does not reach is said not to be there, with what was looked at.
#[test]
fn with_nothing_read_it_says_so_and_never_reads_the_book() {
    let mut h = harness();
    let id = h.state().views[0].id;
    h.get_by_label("In the book").click();
    h.run_steps(3);
    h.get_by_label("Read the book (Detect diagrams or Text) to search.");
    assert_eq!(h.state().session.chess_scanned(), 0, "nothing read for it");
    assert!(h.state().chess.results_of(id).is_none(), "nothing detected for it");
    // Detect diagrams: the page's diagram is the start position the board shows.
    assert!(h.state_mut().execute("chess.scan"));
    h.run_steps(6);
    h.state_mut().show_chess_board();
    h.run_steps(4);
    h.get_by_label("In the book (1)");
    h.get_by_label_contains("Pages read: ");
    h.get_by_label("Diagram 1, p. 1").click();
    h.run_steps(3);
    let picked = h.state().chess.results_of(id).and_then(|r| r.selected);
    assert_eq!(picked.map(|at| (at.page, at.index)), Some((0, 0)), "shown on its page");
    // A position the book does not reach.
    h.state_mut().show_chess_board();
    h.run_steps(2);
    put_game(&mut h, "1. d4 *");
    h.get_by_label("In the book (0)");
    h.get_by_label("This position does not appear in the book as read.");
}

/// Plan A9-N3 (§2.9): the book's reply played again on the board's position. With the same pieces
/// but castling lost on the board (the king went to f1 and back), the book's O-O is listed and
/// cannot be played, and says why, in sight and to assistive technology.
#[test]
fn a_reply_the_board_cannot_play_is_listed_disabled_with_why() {
    let mut h = harness();
    let id = h.state().views[0].id;
    h.state_mut().set_book_pgn(id, "book.pgn", b"[Event \"Castles\"]\n\n1.e4 e5 2.Nf3 Nc6 3.Bc4 Bc5 4.O-O Nf6 *\n");
    h.run_steps(3);
    put_game(&mut h, "1. e4 e5 2. Nf3 Nc6 3. Bc4 Bc5 4. Kf1 Nf6 5. Ke1 Ng8 *");
    h.get_by_label("In the book (1)").click();
    h.run_steps(3);
    let castles = h.get_by_label_contains("castles short");
    let name = castles.accesskit_node().label().unwrap_or_default().to_owned();
    assert!(name.contains("Castling isn't allowed on this board."), "{name}");
    assert!(castles.accesskit_node().is_disabled(), "it can't be played here");
    h.get_by_label("Castling isn't allowed on this board.");
    shot(&mut h, "n3-castling-lost.png");
}

/// Plan A9-N3: off the interface thread. With the jobs on workers, the answer comes when they
/// end, without a gesture.
#[test]
fn the_search_runs_on_workers() {
    let mut h = harness();
    let id = h.state().views[0].id;
    h.state_mut().set_book_pgn(id, "book.pgn", BOOK_PGN.as_bytes());
    h.run_steps(3);
    h.state_mut().run_inline = false;
    put_game(&mut h, "1. Nf3 Nc6 2. e4 e5 *");
    assert!(h.state_mut().wait_in_book(std::time::Duration::from_secs(60)), "the search ended");
    h.run_steps(3);
    h.get_by_label("In the book (2)");
}

// ─── Plan A9-N16b: by material and pawn structure ───────────────────────────────────────────

/// A rook ending with a pawn, its twin with the colours swapped, and a queen game (plan A9-N16a's).
const ENDINGS_PGN: &str = "[Event \"Rook ending\"]\n[SetUp \"1\"]\n[FEN \"6k1/8/8/3P4/8/8/r7/4K2R w - - 0 1\"]\n\n1.d6 Ra8 2.d7 Rd8 *\n\n[Event \"Twin\"]\n[SetUp \"1\"]\n[FEN \"4k2r/R7/8/8/3p4/8/8/6K1 b - - 0 1\"]\n\n1...d3 2.Ra1 d2 *\n\n[Event \"Queen\"]\n[SetUp \"1\"]\n[FEN \"6k1/5ppp/8/8/8/8/5PPP/3Q2K1 w - - 0 1\"]\n\n1.Qd8# 1-0\n";

/// Plan A9-N16b (the card's check): Chess ▸ Find in the book by material… opens the In the book
/// list looking by material, the keyboard in its field. "KRP-KR" finds the rook ending's stretch;
/// with Either side, the twin's too (the colours swapped), never the queen game. Walked with the
/// keyboard: Tab from the last pawn mark reaches the first stretch, Enter opens its game on the
/// board at its first move; a material that is not a signature says how one is written.
#[test]
fn by_material_the_rook_endings_are_listed_and_opened_from_the_keyboard() {
    let mut h = harness();
    let id = h.state().views[0].id;
    h.state_mut().set_book_pgn(id, "book.pgn", ENDINGS_PGN.as_bytes());
    h.run_steps(3);
    assert!(h.state_mut().execute("chess.find_material"));
    h.run_steps(3);
    assert_eq!(focused(&h), "Material", "the keyboard in its field");
    h.get_by_label("Write a material, such as KRP-KR, or choose a kind of pawn.");
    h.get_by_label("Material").type_text("KRP-KR");
    h.run_steps(3);
    h.get_by_label("Found: 1");
    h.get_by_label("Either side").click();
    h.run_steps(3);
    h.get_by_label("Found: 2");
    h.get_by_label("Game 1 · Rook ending: 1. d6 to 2… rook d8, Main line, Positions: 4");
    h.get_by_label("Game 2 · Twin: 1… d3 to 2… d2, Main line, Positions: 3");
    assert!(h.query_by_label_contains("Game 3").is_none(), "not the queen game");
    shot(&mut h, "n16b-by-material.png");
    // The keyboard: from the last pawn mark to the first stretch, and Enter opens its game.
    h.get_by_label("Passed").focus();
    h.run_steps(1);
    host_key(&mut h, egui::Modifiers::NONE, egui::Key::Tab);
    assert!(focused(&h).starts_with("Game 1 · Rook ending"), "{}", focused(&h));
    host_key(&mut h, egui::Modifiers::NONE, egui::Key::Enter);
    h.run_steps(2);
    assert_eq!(h.state().chess.board().game().tag("Event"), Some("Rook ending"));
    assert_eq!(shown(&h), "d6", "at the stretch's first move");
    // A material that is not a signature.
    h.get_by_label("Material").focus();
    h.run_steps(1);
    h.get_by_label("Material").type_text("Q");
    h.run_steps(2);
    h.get_by_label("Not a material signature: each side's pieces in the order K Q R B N P, White's first, as KRP-KR.");
}

/// Plan A9-N3b: while the next answer is looked for (on a worker), the list keeps the last one,
/// dimmed: the keyboard stays on its row, so a reply played from the keyboard leaves it on the
/// list, on the new position's first reply (it used to show "Looking…" alone for a frame, which
/// dropped the focused row).
#[test]
fn the_list_keeps_the_keyboard_while_it_looks_again() {
    let mut h = harness();
    let id = h.state().views[0].id;
    h.state_mut().set_book_pgn(id, "book.pgn", BOOK_PGN.as_bytes());
    h.run_steps(3);
    put_game(&mut h, "1. e4 e5 2. Nf3 Nc6 *");
    h.get_by_label("In the book (2)").click();
    h.run_steps(3);
    h.state_mut().run_inline = false;
    h.get_by_label("Play 3. bishop b5 on the board, Good move, Main line, Games: 2").focus();
    h.run_steps(1);
    host_key(&mut h, egui::Modifiers::NONE, egui::Key::Enter);
    assert!(h.state_mut().wait_in_book(std::time::Duration::from_secs(60)), "the search ended");
    h.run_steps(3);
    assert_eq!(shown(&h), "Bb5");
    assert!(focused(&h).starts_with("Play 3… a6"), "the keyboard on the list: {:?}", focused(&h));
}
