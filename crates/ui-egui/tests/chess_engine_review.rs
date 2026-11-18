//! Chess ▸ Check with the engine… (plan A9-N17b, finding A8-10): the book says × the calculation
//! says, for a whole book, in the real shell (egui_kittest) with a pretend engine
//! (`Analyser::with_streams`, as the N8 and N17a tests have it). The engine's half is
//! `pdfcraft_engine::chess::review`'s.

use std::io::{BufRead, BufReader, Write};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use pdfcraft_engine::chess::analysis::{Analyser, AnalysisOptions};
use pdfcraft_engine::chess::evals::{DirStore, Eval};
use pdfcraft_engine::chess::pdfcraft_chess::game::Game;
use pdfcraft_engine::chess::pdfcraft_chess::uci::{Bound, Score};
use pdfcraft_ui_egui::{PdfCraftApp, RightPanel};

#[path = "common/gpu.rs"]
mod gpu;

/// How long a test waits for work on another thread: a net against a hang, never a clock it races
/// (plan A9-Q4b).
const NET: Duration = Duration::from_secs(60);

/// The book's line: a `!` the engine can refute.
const MOVES: &str = "1.e4 e5 2.Nf3! Nc6";

/// A PDF of `pages` 600 × 800 pt pages, each with its line of moves (empty: a blank page) in
/// Helvetica near the top; its pages numbered from 11 (`/PageLabels`), as a book's are.
fn book(pages: &[&str]) -> Vec<u8> {
    let n = pages.len();
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 4 + 2 * i)).collect();
    let mut objs = vec![
        "<< /Type /Catalog /Pages 2 0 R /PageLabels << /Nums [0 << /S /D /St 11 >>] >> >>".to_string(),
        format!("<< /Type /Pages /Kids [{}] /Count {n} /MediaBox [0 0 600 800] >>", kids.join(" ")),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_string(),
    ];
    for (i, moves) in pages.iter().enumerate() {
        let content = if moves.is_empty() { String::new() } else { format!("BT /F2 11 Tf 60 730 Td ({moves}) Tj ET\n") };
        objs.push(format!("<< /Type /Page /Parent 2 0 R /Contents {} 0 R /Resources << /Font << /F2 3 0 R >> >> >>", 5 + 2 * i));
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

/// A folder of its own under the temp folder, empty.
fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcraft-engine-check-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The app on `pdf` in a window of `size`, its chess kept in `data`, its page jobs inline, no piece
/// model, in `language` and `theme`; the Chess column's Text tab shown.
fn harness_in(pdf: Vec<u8>, data: &std::path::Path, size: egui::Vec2, language: &str, theme: &str) -> Harness<'static, PdfCraftApp> {
    let (none, _) = pdfcraft_engine::chess::pdfcraft_chessvision::models::ChessModels::from_files(Vec::new());
    pdfcraft_engine::chess::install_models(Some(none));
    let (data, language, theme) = (data.to_path_buf(), language.to_owned(), theme.to_owned());
    let mut h = Harness::builder().with_size(size).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", &language).unwrap();
        app.set_option("theme", &theme).unwrap();
        app.use_chess_data(data, None);
        app.open_bytes("chapter.pdf", None, pdf).unwrap();
        app.run_inline = true;
        app
    });
    h.run_steps(4);
    h.state_mut().choose_right_panel(Some(RightPanel::Chess));
    h.run_steps(2);
    // "Text" in English and in German alike.
    h.get_by(|n| n.role() == egui::accesskit::Role::Tab && n.label().is_some_and(|l| l == "Text")).click();
    h.run_steps(4);
    h
}

fn harness(pdf: Vec<u8>, data: &std::path::Path) -> Harness<'static, PdfCraftApp> {
    harness_in(pdf, data, egui::vec2(1400.0, 1000.0), "en", "light")
}

/// How the pretend engine answers each `go`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Answers {
    /// At once.
    Now,
    /// Once the test lets it ([`Engine::answer`]), or after `wait` without leave.
    Gated { wait: Duration },
    /// It answers the first `n - 1`, then closes its output at the `n`th: an engine that died.
    DiesAt(usize),
}

/// A pretend UCI engine on the other end of two pipes: for each `go depth N` it says one line of
/// depth `N` (at most 30) with the first legal move of the position it was given (never g1f3),
/// scored `cp` for the side to move, then its `bestmove`. Every `position` command goes into
/// `told`.
struct Engine {
    told: Arc<Mutex<Vec<String>>>,
    leave: Sender<()>,
}

impl Engine {
    /// Let the engine answer one more position.
    fn answer(&self, n: usize) {
        for _ in 0..n {
            self.leave.send(()).unwrap();
        }
    }

    /// The `position` commands it was given.
    fn positions(&self) -> Vec<String> {
        self.told.lock().unwrap().clone()
    }
}

fn engine(answers: Answers, cp: i32) -> (Analyser, Engine) {
    engine_logging(answers, cp, Arc::default())
}

/// [`engine`] writing its `position` commands into `told` (a log two engines share: each batch
/// run starts and closes its own).
fn engine_logging(answers: Answers, cp: i32, told: Arc<Mutex<Vec<String>>>) -> (Analyser, Engine) {
    let (engine_in, our_out) = std::io::pipe().unwrap();
    let (our_in, mut out) = std::io::pipe().unwrap();
    let log = Arc::clone(&told);
    let (leave, gate): (Sender<()>, Receiver<()>) = channel();
    std::thread::spawn(move || {
        let mut game: Option<Game> = None;
        let mut gos = 0usize;
        for line in BufReader::new(engine_in).lines() {
            let Ok(line) = line else { break };
            let mut words = line.split_whitespace();
            match words.next() {
                Some("uci") => {
                    let _ = writeln!(out, "id name Fake Engine 1\noption name MultiPV type spin default 1 min 1 max 5\nuciok");
                }
                Some("position") => {
                    log.lock().unwrap().push(line.clone());
                    let rest: Vec<&str> = words.skip(1).collect();
                    let (fen, moves) = match rest.iter().position(|w| *w == "moves") {
                        Some(at) => (rest[..at].join(" "), rest[at + 1..].to_vec()),
                        None => (rest.join(" "), Vec::new()),
                    };
                    let mut g = Game::from_fen(&fen).unwrap();
                    for m in moves {
                        g.play_uci(m).unwrap();
                    }
                    game = Some(g);
                }
                Some("go") => {
                    gos += 1;
                    match answers {
                        Answers::DiesAt(n) if gos >= n => return,
                        Answers::Gated { wait } => {
                            let _ = gate.recv_timeout(wait);
                        }
                        _ => {}
                    }
                    let depth: u32 = line.split_whitespace().last().and_then(|d| d.parse().ok()).unwrap_or(1).min(30);
                    let Some(g) = game.as_ref() else { continue };
                    // Its best move is never 2.Nf3, the move the book signs.
                    let uci = g
                        .legal_moves()
                        .iter()
                        .filter_map(|san| {
                            let mut played = g.clone();
                            played.play_san(san).ok()?;
                            played.uci(played.current())
                        })
                        .find(|uci| uci != "g1f3")
                        .unwrap();
                    let _ = writeln!(out, "info depth {depth} multipv 1 score cp {cp} nodes 10 pv {uci}\nbestmove {uci}");
                }
                Some("quit") => break,
                _ => {}
            }
        }
    });
    (Analyser::with_streams(our_in, our_out, AnalysisOptions::default(), Arc::new(|| {})), Engine { told, leave })
}

/// Run frames until `done` holds, a frame every 10 ms, [`NET`] at most.
fn wait_for(h: &mut Harness<'static, PdfCraftApp>, done: impl Fn(&Harness<'static, PdfCraftApp>) -> bool) -> bool {
    let deadline = std::time::Instant::now() + NET;
    loop {
        h.run_steps(1);
        if done(h) {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Open (or close) the Text tab's menu.
fn text_menu(h: &mut Harness<'static, PdfCraftApp>) {
    h.get_by_label("More Text tab actions").click();
    h.run_steps(2);
}

/// Click item `label` of the Text tab's menu.
#[track_caller]
fn text_menu_item(h: &mut Harness<'static, PdfCraftApp>, label: &str) {
    text_menu(h);
    h.get_by_label(label).click();
    h.run_steps(3);
}

/// Open the dialog from the Text tab's menu and count the positions: what it says.
#[track_caller]
fn count(h: &mut Harness<'static, PdfCraftApp>) -> String {
    text_menu_item(h, "Check with the engine…");
    h.get_by_label("Count the positions").click();
    h.run_steps(3);
    said(h, "Positions to check: ")
}

/// The text of the first node whose text holds `part` (a label's, or its value).
fn said(h: &Harness<'static, PdfCraftApp>, part: &str) -> String {
    h.query_all_by_label_contains(part)
        .next()
        .map(|n| n.accesskit_node().label().or_else(|| n.accesskit_node().value()).unwrap_or_default())
        .unwrap_or_default()
}

/// Whether some node's text holds `part` (a notice says it in more than one node).
fn shown(h: &Harness<'static, PdfCraftApp>, part: &str) -> bool {
    h.query_all_by_label_contains(part).next().is_some()
}

/// The FEN after `sans` from the start.
fn after(sans: &[&str]) -> String {
    let mut g = Game::standard();
    for s in sans {
        g.play_san(s).unwrap();
    }
    g.fen()
}

/// The document's keys and page count, for its evaluations' store.
fn keys(h: &Harness<'static, PdfCraftApp>) -> (pdfcraft_engine::chess::DocKeys, usize) {
    let id = h.state().views[0].id;
    h.state().session.get(id).map(|d| (d.doc_keys(), d.info.pages.len())).unwrap()
}

/// Plan A9-N17b: Check with the engine… is in the Chess menu and the Text tab's menu; before it
/// starts, the dialog says the batch's size and about how long it takes, and nothing is asked of
/// the engine yet; started, the batch runs on a worker (frames go on while the engine is still on
/// a position), and its row says how far it is.
#[test]
fn the_estimate_is_said_before_the_batch_starts_on_a_worker() {
    assert!(pdfcraft_engine::commands::menu("Chess").any(|c| c.id == "chess.engine_review"), "in the Chess menu");
    let data = temp_dir("estimate");
    let mut h = harness(book(&[MOVES]), &data);
    let (analyser, engine) = engine(Answers::Gated { wait: Duration::from_secs(1) }, 100);
    h.state_mut().chess.engine_review.use_engine(analyser);
    let said = count(&mut h);
    assert_eq!(said, "Positions to check: 5 (≈ 1 min)", "the size and the time, before the start");
    assert!(engine.positions().is_empty(), "nothing asked before the start: {:?}", engine.positions());
    h.get_by_label("Start the check").click();
    h.run_steps(2);
    assert!(h.state().chess.engine_review.running(), "on a worker: the frame came back before the batch ended");
    assert!(wait_for(&mut h, |_| !engine.positions().is_empty()), "the engine is asked");
    h.get_by_label_contains("Checking with the engine: position 1 of 5");
    engine.answer(5);
    assert!(wait_for(&mut h, |h| !h.state().chess.engine_review.running()), "the batch ends");
    h.run_steps(2);
    assert!(shown(&h, "The check is done. Positions checked: 5. Divergences: 1."), "said");
    let _ = std::fs::remove_dir_all(data);
}

/// Plan A9-N17b: Stop ends the batch once the position being searched is kept; Continue the check
/// asks only the positions it had not; a new count leaves out what the evaluations hold, and the
/// engine's log of `position` commands has each position once.
#[test]
fn stop_continue_and_check_again_never_ask_a_position_twice() {
    let data = temp_dir("stop");
    let mut h = harness(book(&[MOVES]), &data);
    let (analyser, engine) = engine(Answers::Gated { wait: NET }, 100);
    h.state_mut().chess.engine_review.use_engine(analyser);
    assert_eq!(count(&mut h), "Positions to check: 5 (≈ 1 min)");
    h.get_by_label("Start the check").click();
    engine.answer(2);
    assert!(wait_for(&mut h, |h| h.query_by_label_contains("position 3 of 5").is_some()), "two answered");
    h.get_by_label("Stop").click();
    engine.answer(1);
    assert!(wait_for(&mut h, |h| !h.state().chess.engine_review.running()), "stopped");
    h.run_steps(2);
    assert!(shown(&h, "The check was stopped: 3 of 5 positions checked."), "said");
    let id = h.state().views[0].id;
    assert_eq!(h.state().chess.engine_review.left(id), Some(2));
    // Continue asks the two left, with an engine of its own (the run closed the first one), whose
    // `position` commands go into the same log.
    let (analyser, engine) = engine_logging(Answers::Gated { wait: NET }, 100, Arc::clone(&engine.told));
    h.state_mut().chess.engine_review.use_engine(analyser);
    text_menu_item(&mut h, "Continue the check (2 left)");
    engine.answer(1);
    assert!(wait_for(&mut h, |h| h.query_by_label_contains("position 5 of 5").is_some()), "one more answered");
    h.get_by_label("Stop").click();
    engine.answer(1);
    assert!(wait_for(&mut h, |h| !h.state().chess.engine_review.running()), "stopped again");
    // A new count: what the evaluations hold (written or still on their way) is left out.
    h.run_steps(2);
    text_menu_item(&mut h, "Check with the engine…");
    h.get_by_label("Count the positions").click();
    h.run_steps(3);
    h.get_by_label("Nothing to check: the evaluations hold every position at this depth.");
    h.get_by_label("Checked before at this depth, left out: 5");
    let asked = engine.positions();
    let mut once = asked.clone();
    once.sort();
    once.dedup();
    assert_eq!((asked.len(), once.len()), (5, 5), "each position once: {asked:?}");
    // What was found is in the store, written in batches off the interface thread.
    h.state_mut().settle_chess_games();
    let (keys, pages) = keys(&h);
    assert_eq!(DirStore::new(data.join("evals")).load(&keys, pages).unwrap().file.evals.len(), 5);
    let _ = std::fs::remove_dir_all(data);
}

/// Plan A9-N17b: the Divergences list takes the page text's place: the page as the book numbers
/// it, what the book says and what the calculation says, and the source; Show in PDF opens that
/// page and outlines the move; Esc goes back to the text. The evaluations come from the store: a
/// `!` that loses two pawns to 2.d4.
#[test]
fn the_divergences_list_shows_the_page_that_show_in_pdf_opens() {
    let data = temp_dir("list");
    let mut h = harness(book(&["", MOVES]), &data);
    let (keys, pages) = keys(&h);
    let eval = |cp: i32, best: &str| Eval::new(22, Score::Centipawns(cp), Bound::Exact, &[best.to_owned()], Some("Stockfish 17"), 1).unwrap();
    let kept = [(after(&["e4", "e5"]), eval(50, "d2d4")), (after(&["e4", "e5", "Nf3"]), eval(-150, "b8c6"))];
    DirStore::new(data.join("evals")).keep(&keys, pages, &kept).unwrap();
    let said = count(&mut h);
    assert!(said.starts_with("Positions to check: 3 "), "two held, left out: {said}");
    h.get_by_label("Cancel").click();
    h.run_steps(3);
    // The head row's chip, while there are divergences.
    h.get_by_label("Divergences (1)").click();
    h.run_steps(3);
    let row = h.get_by_label_contains("Show in PDF, p. 12 · 2.Nf3!");
    let name = row.accesskit_node().label().unwrap_or_default();
    assert!(name.contains("The book says: ! (Good move)"), "{name}");
    assert!(name.contains("The calculation says: \u{2212}1.50: loses 2.00 pawns to 2.d4 +0.50 · depth 22 · Stockfish 17"), "{name}");
    assert!(name.ends_with(", Engine"), "{name}");
    assert_eq!(h.state().views[0].current, 0, "the first page shown");
    row.click();
    h.run_steps(3);
    assert_eq!(h.state().views[0].current, 1, "Show in PDF opens the page the row names (12, the second)");
    let flash = h.state().views[0].flash.expect("the move is outlined");
    assert_eq!(flash.0, 1);
    assert!(flash.1[1] > 700.0 && flash.1[3] < 760.0, "the line of moves: {:?}", flash.1);
    // The keyboard goes from "‹ Text" to the row's button; Esc goes back to the page's text.
    h.get_by_label("‹ Text").focus();
    h.run_steps(1);
    h.key_press(egui::Key::Tab);
    h.run_steps(1);
    assert!(h.get_by_label_contains("Show in PDF, p. 12").is_focused(), "Tab reaches the row's Show in PDF");
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(h.query_by_label_contains("Show in PDF, p. 12").is_none(), "back to the text");
    h.get_by_label_contains("Recognised text of page 12");
    // The evaluation beside the move, in words too.
    h.get_by_label_contains("2. knight f3, Engine: \u{2212}1.50 (depth 22)");
    let _ = std::fs::remove_dir_all(data);
}

/// Never a panic (plan A9-N17b): an engine that dies mid-batch is said, what it evaluated is kept,
/// the job ends and Continue is offered for the rest.
#[test]
fn an_engine_that_dies_mid_batch_is_said_and_what_it_found_is_kept() {
    let data = temp_dir("dies");
    let mut h = harness(book(&[MOVES]), &data);
    let (analyser, _engine) = engine(Answers::DiesAt(3), 100);
    h.state_mut().chess.engine_review.use_engine(analyser);
    count(&mut h);
    h.get_by_label("Start the check").click();
    assert!(wait_for(&mut h, |h| !h.state().chess.engine_review.running()), "the job ends");
    h.run_steps(2);
    assert!(shown(&h, "The engine stopped. What the engine found is kept: Continue the check goes on from there."), "said");
    let id = h.state().views[0].id;
    assert_eq!(h.state().chess.engine_review.left(id), Some(3));
    h.state_mut().settle_chess_games();
    let (keys, pages) = keys(&h);
    assert_eq!(DirStore::new(data.join("evals")).load(&keys, pages).unwrap().file.evals.len(), 2, "the two evaluated are kept");
    let _ = std::fs::remove_dir_all(data);
}

/// Never a panic (plan A9-N17b): evaluations that can't be written (the folder is a file) are said
/// once; the batch ends, and what it found stays for the session: the report and the moves have it.
#[test]
fn evaluations_that_cannot_be_written_are_said_and_kept_for_the_session() {
    let data = temp_dir("unwritable");
    let mut h = harness(book(&[MOVES]), &data);
    let blocked = data.join("not-a-folder");
    std::fs::write(&blocked, b"a file").unwrap();
    h.state_mut().chess.evals.use_folder(blocked);
    let (analyser, _engine) = engine(Answers::Now, 100);
    h.state_mut().chess.engine_review.use_engine(analyser);
    count(&mut h);
    h.get_by_label("Start the check").click();
    assert!(wait_for(&mut h, |h| !h.state().chess.engine_review.running()), "the job ends");
    h.state_mut().settle_chess_games();
    assert!(wait_for(&mut h, |h| shown(h, "The engine's evaluations could not be kept")), "said");
    let id = h.state().views[0].id;
    assert_eq!(h.state().chess.engine_review.evals(id).map(|e| e.evals.len()), Some(5), "kept for the session");
    assert_eq!(h.state().chess.engine_review.divergences(id).map(<[_]>::len), Some(1));
    let _ = std::fs::remove_dir_all(data);
}

/// Never a panic (plan A9-N17b): the document closed while its batch runs stops the batch, says
/// so, and what the engine found is written under the document's name all the same: its chess
/// is kept ([`a_document_whose_chess_is_not_kept_has_nothing_written_even_closed_mid_batch`] is the
/// other side).
#[test]
fn a_document_closed_while_its_batch_runs_stops_it_and_keeps_what_was_found() {
    let data = temp_dir("closed");
    let mut h = harness(book(&[MOVES]), &data);
    let (keys, pages) = keys(&h);
    let (analyser, engine) = engine(Answers::Gated { wait: NET }, 100);
    h.state_mut().chess.engine_review.use_engine(analyser);
    count(&mut h);
    h.get_by_label("Start the check").click();
    engine.answer(1);
    assert!(wait_for(&mut h, |h| h.query_by_label_contains("position 2 of 5").is_some()), "one answered");
    assert!(h.state_mut().execute("file.close"));
    assert!(wait_for(&mut h, |h| !h.state().chess.engine_review.running()), "the job ends");
    h.run_steps(2);
    assert!(shown(&h, "The check of chapter.pdf stopped: the document was closed. What the engine found is kept."), "said");
    h.state_mut().settle_chess_games();
    assert_eq!(DirStore::new(data.join("evals")).load(&keys, pages).unwrap().file.evals.len(), 1);
    let _ = std::fs::remove_dir_all(data);
}

/// [`book`], encrypted, its permissions allowing printing and copying: its chess is written only
/// once the user says so (plan A9-I1c, `persist_policy` = Ask).
fn encrypted_book(pages: &[&str]) -> Vec<u8> {
    let mut doc = pdfcraft_cos::Document::open(Arc::new(book(pages))).unwrap();
    doc.set_encryption(&pdfcraft_cos::NewEncryption {
        algorithm: pdfcraft_cos::Algorithm::Aes256,
        user_password: "",
        owner_password: "owner",
        // Print and copy.
        permissions: 0b1_0100,
        encrypt_metadata: true,
        seed: [7; 32],
    })
    .unwrap();
    pdfcraft_cos::write_full(&doc, &Default::default()).unwrap()
}

/// Plan A9-N17b (the review of #420): a document whose chess may not be kept (encrypted, the user
/// not asked yet) has no evaluation written, while its batch runs or after it is closed mid-batch:
/// the rule seen while it was open holds once it is gone. Nothing is asked for an evaluation.
#[test]
fn a_document_whose_chess_is_not_kept_has_nothing_written_even_closed_mid_batch() {
    let data = temp_dir("not-kept");
    let mut h = harness(encrypted_book(&[MOVES]), &data);
    let (keys, pages) = keys(&h);
    let (analyser, engine) = engine(Answers::Gated { wait: NET }, 100);
    h.state_mut().chess.engine_review.use_engine(analyser);
    count(&mut h);
    h.get_by_label("Start the check").click();
    engine.answer(1);
    assert!(wait_for(&mut h, |h| h.query_by_label_contains("position 2 of 5").is_some()), "one answered");
    assert!(h.state_mut().execute("file.close"));
    assert!(wait_for(&mut h, |h| !h.state().chess.engine_review.running()), "the job ends");
    h.run_steps(2);
    h.state_mut().settle_chess_games();
    let kept = DirStore::new(data.join("evals")).load(&keys, pages).unwrap().file.evals.len();
    assert_eq!(kept, 0, "nothing written for a document whose chess is not kept");
    let _ = std::fs::remove_dir_all(data);
}

/// Plan A9-N17b at the reference screen (1366 × 768), light and dark, in English and in German
/// (longer words): the menu item, the dialog's estimate, the batch's row and the list are on
/// screen; the head row's chip is measured and cut before it would push the menu out of view.
#[test]
fn the_check_fits_1366x768_light_and_dark() {
    for (language, theme) in [("en", "light"), ("en", "dark"), ("de", "light"), ("de", "dark")] {
        let data = temp_dir(&format!("fit-{language}-{theme}"));
        let mut h = harness_in(book(&[MOVES]), &data, egui::vec2(1366.0, 768.0), language, theme);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1366.0, 768.0));
        let tr = |en: &str, de: &str| if language == "de" { de.to_owned() } else { en.to_owned() };
        let (analyser, engine) = engine(Answers::Gated { wait: NET }, 100);
        h.state_mut().chess.engine_review.use_engine(analyser);
        let menu_label = tr("More Text tab actions", "Weitere Aktionen des Tabs „Text“");
        h.get_by_label(&menu_label).click();
        h.run_steps(2);
        let item = h.get_by_label(&tr("Check with the engine…", "Mit der Engine prüfen…")).rect();
        assert!(screen.contains_rect(item), "{language} {theme}: the menu item at {item:?}");
        shot(&mut h, &format!("n17b-menu-{language}-{theme}.png"));
        h.get_by_label(&tr("Check with the engine…", "Mit der Engine prüfen…")).click();
        h.run_steps(3);
        h.get_by_label(&tr("Count the positions", "Stellungen zählen")).click();
        h.run_steps(3);
        let estimate = h.get_by_label_contains(&tr("Positions to check: 5", "Zu prüfende Stellungen: 5")).rect();
        assert!(screen.contains_rect(estimate), "{language} {theme}: the estimate at {estimate:?}");
        shot(&mut h, &format!("n17b-estimate-{language}-{theme}.png"));
        h.get_by_label(&tr("Start the check", "Prüfung starten")).click();
        engine.answer(1);
        let row = tr("position 2 of 5", "Stellung 2 von 5");
        assert!(wait_for(&mut h, |h| h.query_by_label_contains(&row).is_some()), "{language} {theme}: the row");
        let r = h.get_by_label_contains(&row).rect();
        assert!(screen.contains_rect(r), "{language} {theme}: the row at {r:?}");
        shot(&mut h, &format!("n17b-bar-{language}-{theme}.png"));
        engine.answer(4);
        assert!(wait_for(&mut h, |h| !h.state().chess.engine_review.running()), "{language} {theme}: done");
        h.run_steps(3);
        // The chip, measured: between the heading and Read with OCR, the menu on screen.
        let chip = h.get_by_label(&tr("Divergences (1)", "Abweichungen (1)")).rect();
        let ocr = h.get_by_label(&tr("Read with OCR", "Mit OCR lesen")).rect();
        let menu = h.get_by_label(&menu_label).rect();
        assert!(screen.contains_rect(menu) && chip.max.x <= ocr.min.x && ocr.max.x <= menu.min.x, "{language} {theme}: {chip:?} {ocr:?} {menu:?}");
        shot(&mut h, &format!("n17b-chip-{language}-{theme}.png"));
        h.get_by_label(&tr("Divergences (1)", "Abweichungen (1)")).click();
        h.run_steps(3);
        let row = h.get_by_label_contains(&tr("Show in PDF, p. 11", "Im PDF zeigen, S. 11")).rect();
        assert!(screen.contains_rect(row), "{language} {theme}: the list's row at {row:?}");
        shot(&mut h, &format!("n17b-list-{language}-{theme}.png"));
        drop(h);
        let _ = std::fs::remove_dir_all(data);
    }
}

/// Save a picture of the window to `PDFCRAFT_CHESS_SHOTS`, when set.
fn shot(h: &mut Harness<'static, PdfCraftApp>, name: &str) {
    if let Ok(dir) = std::env::var("PDFCRAFT_CHESS_SHOTS") {
        let _gpu = gpu::gpu();
        h.render().unwrap().save(std::path::Path::new(&dir).join(name)).unwrap();
    }
}
