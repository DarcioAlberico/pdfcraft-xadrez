//! The book's exercises as a standard problem PGN (plan A9-N10): `SetUp`/`FEN`, the solution as
//! the main line, `Result` and the statement before the first move.

use super::*;
use crate::doc::exercises::{ExercisesPgn, LeftOut, LeftOutReason, exercises_pgn};
use crate::doc::markdown::PgnBook;
use pdfcraft_chess::fen::Fen;
use pdfcraft_chess::game::{Game as PgnGame, split_pgn};
use pdfcraft_chess::pgn_style::PgnStyle;

const ROOK: &str = "8/8/4k3/8/8/4K3/r7/7R w - - 0 1";
/// Black mates at once: `1...Qf1#`.
const QUEEN: &str = "8/8/8/8/8/6k1/5q2/7K b - - 0 1";

/// A puzzle book: three numbered diagrams on a page and a fourth on the next, the solutions of
/// the first three pages on, in another order, and none for the fourth.
fn puzzle_book() -> ChessDoc {
    let p0 = page_text(vec![line(45.0, "№1. Brown – Green"), line(245.0, "№2. Smith – Jones"), line(445.0, "№3. Doe – Roe")]);
    let d0 = [reading(0, 60.0, SCHOLAR, true, false), reading(1, 260.0, ROOK, true, false), reading(2, 460.0, QUEEN, true, false)];
    let p1 = page_text(vec![line(45.0, "№4. Black – White")]);
    let d1 = [reading(0, 60.0, ROOK, true, false)];
    let p2 = page_text(vec![
        line(100.0, "№2. Smith – Jones"),
        line(120.0, ROOK_CHECKS),
        line(160.0, "№3. Doe – Roe"),
        line(180.0, "1...Qf1#"),
        line(220.0, "№1. Brown – Green"),
        line(240.0, SCHOLAR_MATE),
    ]);
    let mut b = DocBuilder::new();
    b.add_page(&input(0, &p0, &d0, &[], &[]));
    b.add_page(&input(1, &p1, &d1, &[], &[]));
    b.add_page(&input(2, &p2, &[], &[], &[]));
    b.finish()
}

fn written(doc: &ChessDoc) -> ExercisesPgn {
    let book = PgnBook { title: Some("Puzzles".into()), labels: vec!["7".into(), "8".into(), "9".into()] };
    exercises_pgn(doc, &book, PgnStyle::FULL)
}

/// The card's check (plan A9-N10): the PGN of three exercises, read back, gives each one's
/// position as its FEN, from `SetUp`, and its solution as the main line, with its result and the
/// statement before the first move; the numbered diagram with no solution is left out and said.
#[test]
fn three_exercises_read_back_with_their_fen_and_their_solution_as_the_main_line() {
    let doc = puzzle_book();
    let made = written(&doc);
    println!("EXERCISES-PGN-BEGIN\n{}EXERCISES-PGN-END", made.pgn);
    let numbers: Vec<u32> = made.exercises.iter().map(|e| e.number).collect();
    assert_eq!(numbers, [1, 2, 3], "in the book's order");
    assert_eq!(made.left_out, [LeftOut { at: DiagramAt::on(1, 0), number: 4, reason: LeftOutReason::NoSolution }]);
    let chunks = split_pgn(&made.pgn, 10);
    assert_eq!(chunks.len(), 3, "{}", made.pgn);
    let expected = [
        (SCHOLAR, ["Qh5", "Nf6", "Qxf7#"].as_slice(), "1-0", "White to move (№ 1). Page 7 · diagram 1 of 3"),
        (ROOK, ["Rh6+", "Kd5", "Rh5+"].as_slice(), "*", "White to move (№ 2). Page 7 · diagram 2 of 3"),
        (QUEEN, ["Qf1#"].as_slice(), "0-1", "Black to move (№ 3). Page 7 · diagram 3 of 3"),
    ];
    for ((chunk, (fen, line, result, said)), number) in chunks.iter().zip(expected).zip(1..) {
        let (game, _) = PgnGame::from_pgn(chunk).unwrap();
        assert_eq!(game.tag("SetUp"), None, "SetUp is the writer's, read as the start");
        assert!(chunk.contains("[SetUp \"1\"]"), "{chunk}");
        let start = Fen::parse(&game.start_fen()).unwrap();
        assert_eq!(start.book_key(), Fen::parse(fen).unwrap().book_key(), "{chunk}");
        let mut main: Vec<String> = Vec::new();
        let mut node = 0;
        while let Some(&next) = game.replies(node).first() {
            main.push(game.san(next).to_owned());
            node = next;
        }
        assert_eq!(main, line, "{chunk}");
        assert!(chunk.contains(&format!("[Result \"{result}\"]")), "{chunk}");
        assert_eq!(game.tag("Result").unwrap_or("*"), result, "{chunk}");
        assert_eq!(game.tag("Round"), Some(number.to_string().as_str()));
        assert_eq!(game.tag("Event"), Some("Puzzles"));
        assert_eq!(game.tag("PdfPuzzle"), Some(number.to_string().as_str()));
        assert!(game.comment(0).starts_with(said), "{:?}", game.comment(0));
    }
}

/// The statement is in the book's language, its side to move as `describe` says it.
#[test]
fn the_statement_is_in_the_books_language() {
    let mut doc = puzzle_book();
    for page in &mut doc.pages {
        page.text_lang = LangTag::parse("pt");
    }
    let pgn = written(&doc).pgn;
    assert!(pgn.contains("{Brancas jogam (№ 1). Pág. 7 · diagrama 1 de 3"), "{pgn}");
    assert!(pgn.contains("{Pretas jogam (№ 3)."), "{pgn}");
}

/// A book with no numbered diagram has no exercise: the games read from its other diagrams are
/// not taken for ones.
#[test]
fn a_diagram_without_a_number_is_no_exercise() {
    let text = page_text(vec![line(230.0, SCHOLAR_MATE)]);
    let doc = build_page(&input(0, &text, &[reading(0, 60.0, SCHOLAR, true, false)], &[], &[]));
    assert_eq!(doc.games.len(), 1);
    let made = written(&doc);
    assert!(made.exercises.is_empty() && made.left_out.is_empty() && made.pgn.is_empty(), "{made:?}");
}
