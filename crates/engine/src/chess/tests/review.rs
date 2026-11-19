//! Plan A9-N17a: the book says × the engine says — the report from what is kept, and the batch
//! that asks an engine for it.

use std::io::{BufRead, BufReader, PipeReader, PipeWriter, Write};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pdfcraft_chess::game::Game as Played;
use pdfcraft_chess::judge::{Band, Limits, Value};
use pdfcraft_chess::pgn_style::PgnDialect;
use pdfcraft_chess::stipulation::Goal;
use pdfcraft_chess::uci::{Bound, Score};
use pdfcraft_chessvision::doc::{Block, ChessDoc, DiagramAt};

use super::*;
use crate::chess::analysis::{Analyser, AnalysisOptions};
use crate::chess::corrections::DocRecord;
use crate::chess::evals::{Eval, EvalFile, SecondLine};
use crate::chess::review::{self, Batch, Budget, Computed, Failure, Found, Part, Place, Ran, Said, Source};

/// White wins with a rook: "White to play and win" (the king off e1: no castling to infer).
const ROOK_WINS: &str = "4k3/8/8/8/8/8/8/R4K2";
/// White has no mate in 2 here (plan A9-N4's case).
const NO_MATE: &str = "8/8/8/4k3/8/8/8/K6R";

/// One page: the moves `moves` at the top, then a diagram of `first` captioned `caption1` and one
/// of `second` captioned `caption2`, each board 160 pt wide in Chess Merida, the text in Times.
fn book(moves: &str, first: (&str, &str), second: (&str, &str)) -> Vec<u8> {
    let rows = |p: &str| {
        let rows = merida(p);
        let rows: [String; 8] = rows;
        rows
    };
    let (a, b) = (rows(first.0), rows(second.0));
    let a: [&str; 8] = std::array::from_fn(|i| a[i].as_str());
    let b: [&str; 8] = std::array::from_fn(|i| b[i].as_str());
    let content = format!(
        "BT /F2 10 Tf 50 760 Td ({moves}) Tj ET\n{}BT /F2 10 Tf 50 526 Td ({}) Tj ET\n{}BT /F2 10 Tf 50 266 Td ({}) Tj ET\n",
        font_board(50.0, 100.0, a),
        first.1,
        font_board(50.0, 360.0, b),
        second.1,
    );
    pdf_fonts(&content, &["ChessMerida", "Times-Roman"], None, "")
}

/// The FEN after `sans` from the start.
fn after(sans: &[&str]) -> String {
    let mut g = Played::standard();
    for s in sans {
        g.play_san(s).unwrap();
    }
    g.fen()
}

fn eval(depth: u32, cp: i32, best: &str) -> Eval {
    Eval::new(depth, Score::Centipawns(cp), Bound::Exact, &[best.to_owned()], Some("Stockfish 17"), 1).unwrap()
}

fn file() -> EvalFile {
    EvalFile::empty(DocRecord::default())
}

/// The book read: `1.e4 e5 2.Nf3! Nc6` over a rook ending ("White to play and win") and a mate in
/// two that has none.
fn read_book() -> ChessDoc {
    let (mut s, id) = open(book("1.e4 e5 2.Nf3! Nc6", (ROOK_WINS, "White to play and win"), (NO_MATE, "White to play. Mate in 2")));
    s.chess_document(id, &[], &no_model(), None).unwrap().doc
}

fn diagrams(doc: &ChessDoc) -> Vec<&pdfcraft_chessvision::doc::DiagramBlock> {
    doc.pages.iter().flat_map(|p| &p.blocks).filter_map(|b| if let Block::Diagram(d) = b { Some(d) } else { None }).collect()
}

/// The card's check (plan A9-N17a): a `!` the kept evaluations refute, the "#2" the solver finds no
/// mate in, and "White to play and win" with −3 kept are one list, each with what computed it;
/// the goal is in the document.
#[test]
fn the_engine_and_the_solver_disagree_with_the_book_in_one_list() {
    let doc = read_book();
    let goals: Vec<Option<Goal>> = diagrams(&doc).iter().map(|d| d.goal).collect();
    assert_eq!(goals, [Some(Goal::Win), Some(Goal::Mate(2))], "the captions' goals are kept");
    let mut evals = file();
    // Before 2.Nf3 the engine prefers 2.d4 (+0.50); after it, White stands at −1.50.
    evals.keep(&after(&["e4", "e5"]), eval(22, 50, "d2d4"));
    evals.keep(&after(&["e4", "e5", "Nf3"]), eval(22, -150, "b8c6"));
    evals.keep(diagrams(&doc)[0].shown().unwrap().fen, eval(20, -300, "a1a8"));
    let r = review::divergences(&doc, &[], &evals, &Limits::default());
    let found: Vec<(Place, &str, Source)> = r.divergences.iter().map(|d| (d.at, d.found.name(), d.source)).collect();
    let nf3 = pdfcraft_chessvision::doc::MoveAt::new(0, first_move_block(&doc), 2);
    assert_eq!(nf3.find(&doc).map(|m| m.san.as_str()), Some("Nf3"));
    assert_eq!(
        found,
        [
            (Place::Move(nf3), "refuted", Source::Engine),
            (Place::Diagram(DiagramAt::new(0, 0)), "goal_contradicted", Source::Engine),
            (Place::Diagram(DiagramAt::new(0, 1)), "no_mate", Source::Solver),
        ],
        "{r:#?}"
    );
    let refuted = &r.divergences[0];
    assert_eq!((refuted.printed.as_str(), refuted.book), ("2.Nf3!", Said::Sign(1)));
    assert_eq!(refuted.found, Found::Refuted { loss: Some(200) });
    let Computed::Engine(e) = &refuted.computed else { panic!("{refuted:?}") };
    assert_eq!(
        (e.value, e.depth, e.best.clone(), e.engine.as_deref()),
        (Value::Cp(-150), 22, Some(("2.d4".to_owned(), Value::Cp(50))), Some("Stockfish 17"))
    );
    assert_eq!(refuted.computed_words(), "it loses 2.00 pawns: \u{2212}1.50; best 2.d4 +0.50 (depth 22, Stockfish 17)");
    assert_eq!(r.divergences[1].found, Found::Goal { band: Band::BlackWins });
    assert_eq!(r.divergences[1].book_words(), "White to play and win");
    assert!(matches!(&r.divergences[2].computed, Computed::Solver(s) if s.n == 2 && s.best_try.is_some()), "{:?}", r.divergences[2]);
    assert_eq!((r.judged, r.no_eval, r.shallow), (3, 0, 0));

    // Shallower than 18, nothing is judged, and that is said.
    let mut shallow = file();
    shallow.keep(&after(&["e4", "e5"]), eval(12, 50, "d2d4"));
    shallow.keep(&after(&["e4", "e5", "Nf3"]), eval(12, -150, "b8c6"));
    let r = review::divergences(&doc, &[], &shallow, &Limits::default());
    assert_eq!((r.divergences.len(), r.shallow, r.no_eval), (1, 1, 1), "the solver's stays; the goal has no evaluation: {r:#?}");
    // The limits are the caller's.
    let r = review::divergences(&doc, &[], &evals, &Limits { refuted: 250, ..Limits::default() });
    assert!(!r.divergences.iter().any(|d| d.found.name() == "refuted"), "{r:#?}");

    // The CSV and the Markdown say each row; the PGN export's comment, the engine's.
    let csv = review::to_csv(&review::divergences(&doc, &[], &evals, &Limits::default()), &["7".to_owned()]);
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines[0], "page,page_label,at,printed,finding,book,computed,eval,depth,best,engine,source");
    assert!(lines[1].starts_with("1,7,move,2.Nf3!,refuted,!,"), "{csv}");
    assert!(lines[1].ends_with(",\u{2212}1.50,22,2.d4,Stockfish 17,engine"), "{csv}");
    assert!(lines[3].contains(",no_mate,White to play, mate in 2,") || lines[3].contains(",no_mate,\"White to play, mate in 2\","), "{csv}");
}

/// The block of the page that holds the moves.
fn first_move_block(doc: &ChessDoc) -> usize {
    doc.pages[0].blocks.iter().position(|b| b.inlines().iter().any(|i| matches!(i, pdfcraft_chessvision::doc::Inline::Move(_)))).unwrap()
}

/// The engine's comment goes to the move's node, for the PGN export (decision C of the card).
#[test]
fn the_pgn_gets_the_engines_comment_on_the_move() {
    let mut doc = read_book();
    let mut evals = file();
    evals.keep(&after(&["e4", "e5"]), eval(22, 50, "d2d4"));
    evals.keep(&after(&["e4", "e5", "Nf3"]), eval(22, -150, "b8c6"));
    let r = review::divergences(&doc, &[], &evals, &Limits::default());
    assert_eq!(review::annotate(&mut doc, &r, "en", PgnDialect::Standard), 1);
    let node = doc.games[0].score.nodes.iter().find(|n| n.san == "Nf3").unwrap();
    assert_eq!(node.comment, "Engine: \u{2212}1.50/22; 2.d4 +0.50/22 (Stockfish 17)");
    // Once only.
    assert_eq!(review::annotate(&mut doc, &r, "en", PgnDialect::Standard), 0);
    let book = pdfcraft_chessvision::doc::markdown::PgnBook { title: None, labels: vec![] };
    let pgn = pdfcraft_chessvision::doc::markdown::pgn_export(&doc, &book, Default::default(), Default::default()).pgn;
    assert!(pgn.contains("{Engine: \u{2212}1.50/22; 2.d4 +0.50/22 (Stockfish 17)}"), "{pgn}");
    // In a Portuguese book, the label is "Motor".
    assert!(review::engine_comment(&r.divergences[0], "pt-BR", PgnDialect::Standard).unwrap().starts_with("Motor: "));
}

/// Plan A9-F7: in the Lichess dialect the engine's word is lichess's evaluation command, the value
/// after the move for White, instead of `{Engine: …}`; the PGN export writes it in the move's
/// comment, and the dialect's glyphs with it.
#[test]
fn the_lichess_pgn_gets_the_engines_value_as_an_eval_command() {
    let mut doc = read_book();
    let mut evals = file();
    evals.keep(&after(&["e4", "e5"]), eval(22, 50, "d2d4"));
    evals.keep(&after(&["e4", "e5", "Nf3"]), eval(22, -150, "b8c6"));
    let r = review::divergences(&doc, &[], &evals, &Limits::default());
    assert_eq!(review::engine_comment(&r.divergences[0], "pt-BR", PgnDialect::Lichess).as_deref(), Some("[%eval -1.50]"));
    assert_eq!(review::annotate(&mut doc, &r, "en", PgnDialect::Lichess), 1);
    let node = doc.games[0].score.nodes.iter().find(|n| n.san == "Nf3").unwrap();
    assert_eq!(node.comment, "[%eval -1.50]");
    assert_eq!(review::annotate(&mut doc, &r, "en", PgnDialect::Lichess), 0, "once only");
    let book = pdfcraft_chessvision::doc::markdown::PgnBook { title: None, labels: vec![] };
    let style = pdfcraft_chess::pgn_style::PgnStyle { dialect: PgnDialect::Lichess, ..Default::default() };
    let pgn = pdfcraft_chessvision::doc::markdown::pgn_export(&doc, &book, Default::default(), style).pgn;
    assert!(pgn.contains("Nf3 $1 {[%eval -1.50]}") || pgn.contains("Nf3 {[%eval -1.50]}"), "{pgn}");
    assert!(!pgn.contains("Engine:"), "{pgn}");
}

/// An exercise (a numbered diagram) asks for two lines, and a second winning line is another
/// solution.
#[test]
fn an_exercise_with_a_second_winning_line_has_another_solution() {
    let mut doc = read_book();
    for page in &mut doc.pages {
        for block in &mut page.blocks {
            if let Block::Diagram(d) = block
                && d.number == 0
            {
                d.puzzle = Some(12);
            }
        }
    }
    let fen = diagrams(&doc)[0].shown().unwrap().fen.to_owned();
    let second = SecondLine::new(Score::Centipawns(900), &["a1a7".to_owned()]);
    let mut evals = file();
    evals.keep(&fen, eval(24, 950, "a1a8").with_second(second));
    let r = review::divergences(&doc, &[], &evals, &Limits::default());
    let other = r.divergences.iter().find(|d| d.found == Found::OtherSolution).unwrap_or_else(|| panic!("{r:#?}"));
    let Computed::Engine(e) = &other.computed else { panic!() };
    assert_eq!(e.second, Some(("1.Ra7".to_owned(), Value::Cp(900))));
    assert!(other.computed_words().contains("also 1.Ra7 +9.00"), "{}", other.computed_words());
    let batch = Batch::new(&doc, &[], &file(), 18);
    assert_eq!(batch.positions().find(|a| a.fen == fen).map(|a| a.lines), Some(2));
}

/// The batch asks each position once; what the store holds deep enough is left out (the order:
/// [`the_batch_asks_signed_moves_and_diagrams_before_the_main_lines`]).
#[test]
fn the_batch_asks_each_position_once_and_leaves_out_what_is_kept() {
    let doc = read_book();
    let batch = Batch::new(&doc, &[], &file(), 18);
    let keys: std::collections::HashSet<&str> = batch.positions().map(|a| a.key.as_str()).collect();
    assert_eq!(keys.len(), batch.len(), "once each");
    // Kept at depth 18 or deeper: left out; shallower: asked again.
    let mut evals = file();
    evals.keep(&after(&["e4"]), eval(18, 30, "e7e5"));
    evals.keep(&after(&["e4", "e5"]), eval(17, 30, "g1f3"));
    let again = Batch::new(&doc, &[], &evals, 18);
    assert_eq!((again.len(), again.kept), (6, 1));
    assert!(!again.positions().any(|a| a.fen == after(&["e4"])));
}

/// A pretend engine on the other end of two pipes, as the analysis tests' one: for each `go` it
/// reports a line of depth `depth` with the first legal move of the position it was given, scored
/// `cp 25` (and a second line), then `bestmove`. It records each `position` command.
fn fake_engine(commands: PipeReader, mut out: PipeWriter, log: Arc<Mutex<Vec<String>>>) {
    let mut game = None;
    for line in BufReader::new(commands).lines() {
        let Ok(line) = line else { break };
        let mut words = line.split_whitespace();
        match words.next() {
            Some("uci") => {
                let _ = writeln!(out, "id name Fake Engine\noption name MultiPV type spin default 1 min 1 max 5\nuciok");
            }
            Some("position") => {
                log.lock().unwrap().push(line.clone());
                let rest: Vec<&str> = words.skip(1).collect();
                let (fen, moves) = match rest.iter().position(|w| *w == "moves") {
                    Some(at) => (rest[..at].join(" "), rest[at + 1..].to_vec()),
                    None => (rest.join(" "), Vec::new()),
                };
                let mut g = Played::from_fen(&fen).unwrap();
                for m in moves {
                    g.play_uci(m).unwrap();
                }
                game = Some(g);
            }
            Some("go") => {
                let depth: u32 = line.split_whitespace().last().and_then(|d| d.parse().ok()).unwrap_or(1);
                let Some(g) = game.as_ref() else { continue };
                let first = g.legal_moves()[0].clone();
                let mut played = g.clone();
                played.play_san(&first).unwrap();
                let uci = played.uci(played.current()).unwrap();
                let _ = writeln!(out, "info depth {depth} multipv 1 score cp 25 nodes 10 pv {uci}");
                let _ = writeln!(out, "info depth {depth} multipv 2 score cp 5 pv {uci}");
                let _ = writeln!(out, "bestmove {uci}");
            }
            Some("quit") => break,
            _ => {}
        }
    }
}

fn connect() -> (Analyser, Arc<Mutex<Vec<String>>>) {
    let (engine_in, our_out) = std::io::pipe().unwrap();
    let (our_in, engine_out) = std::io::pipe().unwrap();
    let log = Arc::new(Mutex::new(Vec::new()));
    let l = Arc::clone(&log);
    std::thread::spawn(move || fake_engine(engine_in, engine_out, l));
    (Analyser::with_streams(our_in, our_out, AnalysisOptions::default(), Arc::new(|| {})), log)
}

/// With the pretend engine: the batch asks its positions in order, with the moves that led to
/// each, and hands the evaluations back in chunks; stopped after two, run again it asks only the
/// rest, and a batch made from what was kept leaves out what was asked.
#[test]
fn a_batch_asks_each_position_once_and_stopped_goes_on_where_it_stopped() {
    let doc = read_book();
    let (analyser, log) = connect();
    let mut batch = Batch::new(&doc, &[], &file(), 20);
    let order: Vec<String> = batch.positions().map(|a| a.fen.clone()).collect();
    assert_eq!(order.len(), 7);
    let budget = Budget { depth: 20, per_position: Duration::from_secs(5), chunk: 2 };
    let stop = AtomicBool::new(false);
    let mut kept = file();
    let mut chunks = Vec::new();
    let ran = review::run(&analyser, &mut batch, &budget, &stop, |p| {
        chunks.push(p.evals.len());
        for (fen, e) in &p.evals {
            kept.keep(fen, e.clone());
        }
        if p.done >= 2 {
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    });
    assert_eq!(ran, Ran::Stopped);
    assert_eq!(batch.len(), 5, "the five not asked stay");
    assert_eq!(chunks.first(), Some(&2), "{chunks:?}");
    let first: Vec<String> = log.lock().unwrap().drain(..).collect();
    assert_eq!(first.len(), 2, "{first:?}");
    // The signed move's positions first, the second with the move that led to it (2.Nf3).
    assert_eq!(first[0], format!("position fen {}", order[0]));
    assert_eq!(first[1], format!("position fen {} moves g1f3", order[0]));
    let e = kept.get(&order[0]).unwrap();
    assert_eq!((e.depth, e.cp, e.engine.as_deref()), (20, Some(25), Some("Fake Engine")));
    // Going on asks the five others, each once.
    stop.store(false, std::sync::atomic::Ordering::Relaxed);
    let ran = review::run(&analyser, &mut batch, &budget, &stop, |p| {
        for (fen, e) in &p.evals {
            kept.keep(fen, e.clone());
        }
    });
    assert_eq!((ran, batch.len()), (Ran::Done { skipped: 0 }, 0));
    let rest: Vec<String> = log.lock().unwrap().clone();
    assert_eq!(rest.len(), 5, "{rest:?}");
    // The main line's last position with the moves since the last pawn move: 2.Nf3 and 2...Nc6.
    assert_eq!(rest[4], format!("position fen {} moves g1f3 b8c6", order[0]));
    assert!(order.iter().all(|f| kept.get(f).is_some()), "all kept");
    // A batch from what is kept is empty.
    assert_eq!(Batch::new(&doc, &[], &kept, 20).len(), 0);
}

/// With a real engine (`PDFCRAFT_CHESS_TEST_ENGINE`; skipped without one): a short batch comes
/// back at the depth asked.
#[test]
fn a_real_engine_evaluates_a_batch() {
    let Some(path) = crate::chess::analysis::test_engine().unwrap() else { return };
    let doc = read_book();
    let analyser = Analyser::start(&path, AnalysisOptions::default(), Arc::new(|| {})).unwrap();
    let mut batch = Batch::new(&doc, &[], &file(), 10);
    let total = batch.len();
    let mut kept = Vec::new();
    let ran =
        review::run(&analyser, &mut batch, &Budget { depth: 10, per_position: Duration::from_secs(10), chunk: 64 }, &AtomicBool::new(false), |p| {
            kept.extend(p.evals)
        });
    assert_eq!(ran, Ran::Done { skipped: 0 });
    assert_eq!(kept.len(), total);
    assert!(kept.iter().all(|(_, e)| e.depth >= 10), "{kept:?}");
}

// ─── Plan A9-N17a2: what #389's review found ─────────────────────────────────────────────────

/// A search stopped mid-way leaves a bound (the score is at least, or at most, this much): never
/// the last word. A `!` whose position after it has only a bound is not judged, and the batch asks
/// that position again.
#[test]
fn a_bound_is_never_the_last_word() {
    let doc = read_book();
    let mut evals = file();
    evals.keep(&after(&["e4", "e5"]), eval(22, 50, "d2d4"));
    let bound = Eval::new(22, Score::Centipawns(-150), Bound::Lower, &["b8c6".to_owned()], Some("Stockfish 17"), 1).unwrap();
    evals.keep(&after(&["e4", "e5", "Nf3"]), bound);
    let r = review::divergences(&doc, &[], &evals, &Limits::default());
    assert!(!r.divergences.iter().any(|d| d.found.name() == "refuted"), "a bound judged as exact: {r:#?}");
    assert_eq!(r.no_eval, 2, "the ! and the goal: {r:#?}");
    let batch = Batch::new(&doc, &[], &evals, 18);
    assert!(batch.positions().any(|a| a.fen == after(&["e4", "e5", "Nf3"])), "the bound's position is asked again");
    assert!(!batch.positions().any(|a| a.fen == after(&["e4", "e5"])), "the exact one is not");
}

/// The store keeps an exact score over a bound, however deep the bound.
#[test]
fn an_exact_score_outranks_a_deeper_bound_in_the_store() {
    let fen = after(&["e4"]);
    let mut evals = file();
    let bound = Eval::new(30, Score::Centipawns(10), Bound::Upper, &["e7e5".to_owned()], None, 1).unwrap();
    assert!(evals.keep(&fen, bound));
    assert!(evals.keep(&fen, eval(18, 25, "e7e5")), "the exact one replaces the bound");
    assert_eq!(evals.get(&fen).map(|e| (e.depth, e.bound.clone())), Some((18, None)));
    let deeper = Eval::new(40, Score::Centipawns(99), Bound::Lower, &["e7e5".to_owned()], None, 2).unwrap();
    assert!(!evals.keep(&fen, deeper), "and a bound never replaces it");
}

/// The positions the report judges come first: a big book's main lines never take the store's
/// room (MAX_EVALS) before a signed move or a diagram is asked.
#[test]
fn the_batch_asks_signed_moves_and_diagrams_before_the_main_lines() {
    let doc = read_book();
    let parts: Vec<Part> = Batch::new(&doc, &[], &file(), 18).positions().map(|a| a.part).collect();
    assert_eq!(parts, [Part::Signed, Part::Signed, Part::Diagram, Part::Diagram, Part::MainLine, Part::MainLine, Part::MainLine]);
}

/// A Black move judged: its `!` loses two pawns for Black (the store's scores are White's).
#[test]
fn a_black_move_is_judged_for_black() {
    let (mut s, id) = open(book("1.e4 e5 2.Nf3 Nc6!", (ROOK_WINS, "White to play and win"), (NO_MATE, "White to play. Mate in 2")));
    let doc = s.chess_document(id, &[], &no_model(), None).unwrap().doc;
    let mut evals = file();
    // Before 2...Nc6 White stands at +0.50 (Black −0.50, best 2...d6); after it, +2.50.
    evals.keep(&after(&["e4", "e5", "Nf3"]), eval(22, 50, "d7d6"));
    evals.keep(&after(&["e4", "e5", "Nf3", "Nc6"]), eval(22, 250, "f1b5"));
    let r = review::divergences(&doc, &[], &evals, &Limits::default());
    let d = r.divergences.iter().find(|d| d.found.name() == "refuted").unwrap_or_else(|| panic!("{r:#?}"));
    assert_eq!(d.found, Found::Refuted { loss: Some(200) });
    let Computed::Engine(e) = &d.computed else { panic!("{d:?}") };
    assert_eq!((e.value, e.best.clone()), (Value::Cp(250), Some(("2...d6".to_owned(), Value::Cp(50)))));
    assert_eq!(d.computed_words(), "it loses 2.00 pawns: +2.50; best 2...d6 +0.50 (depth 22, Stockfish 17)");
}

/// The engine's comment is found again after the comment's rewriting (a `}` in the engine's name
/// becomes `)`): a second run adds none.
#[test]
fn the_engines_comment_is_added_once_whatever_its_characters() {
    let mut doc = read_book();
    let mut evals = file();
    evals.keep(&after(&["e4", "e5"]), Eval::new(22, Score::Centipawns(50), Bound::Exact, &["d2d4".to_owned()], Some("Stock}fish  17"), 1).unwrap());
    evals.keep(&after(&["e4", "e5", "Nf3"]), eval(22, -150, "b8c6"));
    let r = review::divergences(&doc, &[], &evals, &Limits::default());
    assert_eq!(review::annotate(&mut doc, &r, "en", PgnDialect::Standard), 1);
    assert_eq!(
        review::annotate(&mut doc, &r, "en", PgnDialect::Standard),
        0,
        "{:?}",
        doc.games[0].score.nodes.iter().map(|n| &n.comment).collect::<Vec<_>>()
    );
}

/// How [`fake`] behaves once given a position.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fake {
    /// Answers as [`fake_engine`] does.
    Answers,
    /// Says nothing, and ignores `stop`: an engine that hung.
    Hangs,
    /// Closes its output at its first `go`: an engine that crashed.
    Dies,
    /// Finds only a bound (`lowerbound`), and says its `bestmove` when told to stop.
    Bounds,
    /// Finds depth 19 exactly, then only a bound at depth 20, and says its `bestmove` when told
    /// to stop: a search stopped on a bound.
    StopsOnABound,
}

/// A pretend engine that answers the handshake after `handshake`, then behaves as `how` says.
fn fake(commands: PipeReader, mut out: PipeWriter, log: Arc<Mutex<Vec<String>>>, handshake: Duration, how: Fake) {
    let mut game = None;
    // The move of a search waiting for `stop`.
    let mut waiting: Option<String> = None;
    for line in BufReader::new(commands).lines() {
        let Ok(line) = line else { break };
        let mut words = line.split_whitespace();
        match words.next() {
            Some("uci") => {
                std::thread::sleep(handshake);
                let _ = writeln!(out, "id name Fake Engine\noption name MultiPV type spin default 1 min 1 max 5\nuciok");
            }
            Some("position") => {
                log.lock().unwrap().push(line.clone());
                let rest: Vec<&str> = words.skip(1).collect();
                let (fen, moves) = match rest.iter().position(|w| *w == "moves") {
                    Some(at) => (rest[..at].join(" "), rest[at + 1..].to_vec()),
                    None => (rest.join(" "), Vec::new()),
                };
                let mut g = Played::from_fen(&fen).unwrap();
                for m in moves {
                    g.play_uci(m).unwrap();
                }
                game = Some(g);
            }
            Some("go") => {
                let Some(g) = game.as_ref() else { continue };
                let mut played = g.clone();
                played.play_san(&g.legal_moves()[0]).unwrap();
                let uci = played.uci(played.current()).unwrap();
                match how {
                    Fake::Hangs => {}
                    Fake::Dies => return,
                    Fake::Answers => {
                        let _ = writeln!(out, "info depth 20 multipv 1 score cp 25 pv {uci}\nbestmove {uci}");
                    }
                    Fake::Bounds => {
                        let _ = writeln!(out, "info depth 20 multipv 1 score cp 25 lowerbound pv {uci}");
                        waiting = Some(uci);
                    }
                    Fake::StopsOnABound => {
                        let _ =
                            writeln!(out, "info depth 19 multipv 1 score cp 30 pv {uci}\ninfo depth 20 multipv 1 score cp 90 lowerbound pv {uci}");
                        waiting = Some(uci);
                    }
                }
            }
            Some("stop") => {
                if let Some(uci) = waiting.take() {
                    let _ = writeln!(out, "bestmove {uci}");
                }
            }
            Some("quit") => break,
            _ => {}
        }
    }
}

fn connect_to(handshake: Duration, how: Fake) -> (Analyser, Arc<Mutex<Vec<String>>>) {
    let (engine_in, our_out) = std::io::pipe().unwrap();
    let (our_in, engine_out) = std::io::pipe().unwrap();
    let log = Arc::new(Mutex::new(Vec::new()));
    let l = Arc::clone(&log);
    std::thread::spawn(move || fake(engine_in, engine_out, l, handshake, how));
    (Analyser::with_streams(our_in, our_out, AnalysisOptions::default(), Arc::new(|| {})), log)
}

/// Plan A9-N17a3 (A9-N17a2's, asked twice then): a position the engine gives no line of within
/// the budget is given up on and said, never counted as asked, and not asked again (that would
/// spend the budget again).
#[test]
fn a_position_the_engine_gives_no_line_of_is_given_up_without_asking_again() {
    let doc = read_book();
    let (analyser, log) = connect_to(Duration::ZERO, Fake::Hangs);
    let mut batch = Batch::within(&doc, &[], &file(), 20, 1);
    let mut last = None;
    let ran =
        review::run(&analyser, &mut batch, &Budget { depth: 20, per_position: Duration::from_millis(50), chunk: 1 }, &AtomicBool::new(false), |p| {
            last = Some((p.done, p.skipped, p.evals.len()))
        });
    assert_eq!(ran, Ran::Done { skipped: 1 });
    assert_eq!((batch.len(), batch.skipped().len()), (0, 1));
    assert_eq!(last, Some((1, 1, 0)));
    // Taken first: a message that locked the log again would hang the test when it fails.
    let asked = log.lock().unwrap().clone();
    assert_eq!(asked.len(), 1, "asked once: {asked:?}");
}

/// Plan A9-N17a3: an engine that hung (it gives no line, and never answers `stop`) ends the run
/// after three positions in a row, each asked once, instead of spending a budget and the stop's
/// grace on every position of the batch; the three go back to the front of the batch.
#[test]
fn a_hung_engine_fails_the_run_after_three_positions_in_a_row() {
    let doc = read_book();
    let (analyser, log) = connect_to(Duration::ZERO, Fake::Hangs);
    let mut batch = Batch::new(&doc, &[], &file(), 20);
    let order: Vec<String> = batch.positions().map(|a| a.fen.clone()).collect();
    assert_eq!(order.len(), 7);
    let ran =
        review::run(&analyser, &mut batch, &Budget { depth: 20, per_position: Duration::from_millis(50), chunk: 1 }, &AtomicBool::new(false), |_| {});
    assert_eq!(ran, Ran::Failed(Failure::Silent { positions: 3 }));
    let asked = log.lock().unwrap().clone();
    assert_eq!(asked.len(), 3, "each asked once: {asked:?}");
    assert_eq!((batch.len(), batch.skipped().len()), (7, 0), "none taken off");
    assert_eq!(batch.positions().map(|a| a.fen.clone()).collect::<Vec<_>>(), order, "in their order");
}

/// Plan A9-N17a3, what A9-N17a2's fixes lacked: an engine that gives only a bound is never the
/// last word. Each position is asked again at the end (the bound's retry), then given up on, and
/// no evaluation is handed back (the exact filter of the run's `eval_of`).
#[test]
fn a_position_the_engine_gives_only_a_bound_of_is_asked_again_then_skipped() {
    let doc = read_book();
    let (analyser, log) = connect_to(Duration::ZERO, Fake::Bounds);
    let mut batch = Batch::within(&doc, &[], &file(), 20, 2);
    let mut evals = Vec::new();
    let ran =
        review::run(&analyser, &mut batch, &Budget { depth: 20, per_position: Duration::from_millis(50), chunk: 1 }, &AtomicBool::new(false), |p| {
            evals.extend(p.evals)
        });
    assert_eq!((ran, evals), (Ran::Done { skipped: 2 }, Vec::new()));
    let asked = log.lock().unwrap().clone();
    assert_eq!(asked.len(), 4, "each asked twice: {asked:?}");
}

/// Plan A9-N17a3: a search stopped on a depth-20 bound keeps its depth-19 line, the last it found
/// exactly, and that is what is kept: for White, each position's side to move turned.
#[test]
fn a_search_stopped_on_a_bound_keeps_its_last_exact_line() {
    let doc = read_book();
    let (analyser, log) = connect_to(Duration::ZERO, Fake::StopsOnABound);
    let mut batch = Batch::within(&doc, &[], &file(), 20, 2);
    let mut evals = Vec::new();
    let ran =
        review::run(&analyser, &mut batch, &Budget { depth: 20, per_position: Duration::from_millis(50), chunk: 1 }, &AtomicBool::new(false), |p| {
            evals.extend(p.evals)
        });
    assert_eq!(ran, Ran::Done { skipped: 0 });
    let asked = log.lock().unwrap().clone();
    assert_eq!(asked.len(), 2, "each asked once: {asked:?}");
    let found: Vec<(u32, Option<i32>, bool)> = evals.iter().map(|(_, e)| (e.depth, e.cp, e.bound.is_none())).collect();
    // The signed move's two positions: before 2.Nf3 White to move, after it Black.
    assert_eq!(found, [(19, Some(30), true), (19, Some(-30), true)], "{evals:?}");
    let s = analyser.snapshot();
    assert_eq!(s.lines.first().map(|l| (l.depth, l.bound)), Some((20, Bound::Lower)), "the line shown is the bound");
    assert_eq!(s.exact.first().map(|l| (l.depth, l.bound)), Some((19, Bound::Exact)), "beside it, the last exact one");
}

/// Plan A9-N17a3, what A9-N17a2's fixes lacked: a run over positions with Black to move keeps
/// their scores for White (the engine says them for the side to move).
#[test]
fn a_run_keeps_a_black_to_move_score_for_white() {
    let doc = read_book();
    let (analyser, _) = connect_to(Duration::ZERO, Fake::Answers);
    let mut batch = Batch::new(&doc, &[], &file(), 20);
    let mut evals = Vec::new();
    let ran =
        review::run(&analyser, &mut batch, &Budget { depth: 20, per_position: Duration::from_secs(5), chunk: 64 }, &AtomicBool::new(false), |p| {
            evals.extend(p.evals)
        });
    assert_eq!(ran, Ran::Done { skipped: 0 });
    let black = |fen: &str| fen.split_whitespace().nth(1) == Some("b");
    assert!(evals.iter().any(|(fen, _)| black(fen)) && evals.iter().any(|(fen, _)| !black(fen)), "{evals:?}");
    for (fen, e) in &evals {
        assert_eq!(e.cp, Some(if black(fen) { -25 } else { 25 }), "{fen}");
    }
}

/// Plan A9-N17a3: an engine's comment cut at the comment's cap is found again as the piece the cap
/// let in: a second run adds none.
#[test]
fn an_engines_comment_cut_at_the_comments_cap_is_added_once() {
    let mut doc = read_book();
    let name = Some("Stockfish-17-built-for-this-test");
    let mut evals = file();
    evals.keep(&after(&["e4", "e5"]), Eval::new(22, Score::Centipawns(50), Bound::Exact, &["d2d4".to_owned()], name, 1).unwrap());
    evals.keep(&after(&["e4", "e5", "Nf3"]), Eval::new(22, Score::Centipawns(-150), Bound::Exact, &["b8c6".to_owned()], name, 1).unwrap());
    let r = review::divergences(&doc, &[], &evals, &Limits::default());
    let d = r.divergences.iter().find(|d| matches!(d.computed, Computed::Engine(_))).unwrap_or_else(|| panic!("{r:#?}"));
    let full = review::engine_comment(d, "en", PgnDialect::Standard).unwrap();
    // The book's comment leaves room for all of it but its last word (the engine's name), and 20
    // bytes more: the first words would fit again.
    let last = full.rsplit(' ').next().unwrap().len();
    assert!(last > 20, "{full}");
    let room = pdfcraft_chessvision::doc::games::MAX_COMMENT_BYTES - full.len() + last - 20;
    for n in &mut doc.games[0].score.nodes {
        n.comment = "x".repeat(room);
    }
    assert_eq!(review::annotate(&mut doc, &r, "en", PgnDialect::Standard), 1);
    let once: Vec<String> = doc.games[0].score.nodes.iter().map(|n| n.comment.clone()).collect();
    assert!(once.iter().any(|c| c.len() > room && !c.contains(&full)), "cut: {once:?}");
    assert_eq!(review::annotate(&mut doc, &r, "en", PgnDialect::Standard), 0);
    assert_eq!(doc.games[0].score.nodes.iter().map(|n| n.comment.clone()).collect::<Vec<_>>(), once, "nothing appended again");
}

/// Plan A9-N17a2: an engine slow to finish its handshake is waited for, so no position is asked
/// before it can take one (stopping would drop it), and each is asked once.
#[test]
fn an_engine_slow_to_answer_its_handshake_is_waited_for() {
    let doc = read_book();
    let (analyser, log) = connect_to(Duration::from_millis(300), Fake::Answers);
    let mut batch = Batch::new(&doc, &[], &file(), 20);
    let mut evals = 0usize;
    let ran = review::run(
        &analyser,
        &mut batch,
        &Budget { depth: 20, per_position: Duration::from_millis(100), chunk: 64 },
        &AtomicBool::new(false),
        |p| evals += p.evals.len(),
    );
    assert_eq!((ran, evals), (Ran::Done { skipped: 0 }, 7));
    let asked = log.lock().unwrap().clone();
    assert_eq!(asked.len(), 7, "each asked once: {asked:?}");
}

/// An engine that dies fails the run, and the position it was on stays in the batch.
#[test]
fn an_engine_that_dies_fails_the_run_and_keeps_the_batch() {
    let doc = read_book();
    let (analyser, _) = connect_to(Duration::ZERO, Fake::Dies);
    let mut batch = Batch::new(&doc, &[], &file(), 20);
    let ran =
        review::run(&analyser, &mut batch, &Budget { depth: 20, per_position: Duration::from_secs(5), chunk: 1 }, &AtomicBool::new(false), |_| {});
    assert_eq!(ran, Ran::Failed(Failure::Engine(crate::chess::analysis::AnalysisError::Stopped)));
    assert_eq!((batch.len(), batch.skipped().len()), (7, 0), "nothing taken off");
}

/// Past the store's cap the batch keeps what the report judges: the signed moves and the
/// diagrams, not the main lines, and says it was capped.
#[test]
fn a_capped_batch_keeps_what_is_judged() {
    let doc = read_book();
    let batch = Batch::within(&doc, &[], &file(), 18, 3);
    assert_eq!(batch.positions().map(|a| a.part).collect::<Vec<_>>(), [Part::Signed, Part::Signed, Part::Diagram]);
    assert!(batch.capped);
}
