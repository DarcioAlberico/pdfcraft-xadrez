//! The book's exercises as a standard problem PGN (plan A9-N10): `SetUp`/`FEN`, the solution as
//! the main line, `Result` and the statement before the first move.

use super::*;
use crate::doc::anki::{AnkiDeck, Card, MAX_PREFIX, anki_deck, image_name, media_prefix};
use crate::doc::exercises::{Exercise, ExercisesPgn, LeftOut, LeftOutReason, exercises_pgn};
use crate::doc::markdown::PgnBook;
use pdfcraft_chess::fen::Fen;
use pdfcraft_chess::game::{Game as PgnGame, split_pgn};
use pdfcraft_chess::pgn_style::PgnStyle;

const ROOK: &str = "8/8/4k3/8/8/4K3/r7/7R w - - 0 1";
/// Black mates at once: `1...Qf1#`.
const QUEEN: &str = "8/8/8/8/8/6k1/5q2/7K b - - 0 1";

/// A puzzle book: three numbered diagrams on a page and a fourth on the next, the solutions of
/// the first three pages on, in another order, and none for the fourth.
pub(super) fn puzzle_book() -> ChessDoc {
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

/// The book's exercises as Anki's cards (plan A9-N10b): the same three as the problem PGN, each
/// with its picture's name, the statement, the solution in SAN and the side it is printed from,
/// and the same numbered diagram left out.
#[test]
fn the_anki_cards_are_the_exercises_with_their_picture_statement_and_solution() {
    let mut doc = puzzle_book();
    // The third diagram is printed from Black's side: its picture is drawn so.
    for block in doc.pages.iter_mut().flat_map(|p| &mut p.blocks) {
        if let Block::Diagram(d) = block
            && d.puzzle == Some(3)
            && let Some(r) = d.reading.as_mut()
        {
            r.orientation = Some(Color::Black);
        }
    }
    let book = PgnBook { title: Some("Puzzles".into()), labels: vec!["7".into(), "8".into(), "9".into()] };
    let deck = anki_deck(&doc, &book);
    assert_eq!(deck.prefix, "puzzles");
    assert_eq!(deck.exercises(), written(&doc).exercises, "the PGN's exercises, in the same order");
    assert_eq!(deck.left_out, written(&doc).left_out);
    let cards: Vec<(&str, &str, bool)> = deck.cards.iter().map(|c| (c.image.as_str(), c.solution.as_str(), c.flipped)).collect();
    assert_eq!(
        cards,
        [("puzzles-p0001-1.png", SCHOLAR_MATE, false), ("puzzles-p0001-2.png", ROOK_CHECKS, false), ("puzzles-p0001-3.png", "1...Qf1#", true),]
    );
    for (card, fen) in deck.cards.iter().zip([SCHOLAR, ROOK, QUEEN]) {
        assert_eq!(Fen::parse(&card.fen).unwrap().book_key(), Fen::parse(fen).unwrap().book_key(), "the solution's start");
    }
    let tsv = deck.tsv(|_| true);
    // For the check by an external process: the file, and each card's position and solution.
    println!("ANKI-TSV-BEGIN\n{tsv}ANKI-TSV-END");
    let cards: String = deck.cards.iter().map(|c| format!("{}\t{}\n", c.fen, c.solution)).collect();
    println!("ANKI-CARDS-BEGIN\n{cards}ANKI-CARDS-END");
    let lines: Vec<&str> = tsv.lines().collect();
    assert_eq!(lines.get(..3), Some(["#separator:tab", "#html:true", "#tags:puzzles"].as_slice()), "{tsv}");
    assert_eq!(lines.len(), 6, "the headers and a line per card: {tsv}");
    let fronts = [
        "<img src=\"puzzles-p0001-1.png\"><br>White to move (№ 1). Page 7 · diagram 1 of 3",
        "<img src=\"puzzles-p0001-2.png\"><br>White to move (№ 2). Page 7 · diagram 2 of 3",
        "<img src=\"puzzles-p0001-3.png\"><br>Black to move (№ 3). Page 7 · diagram 3 of 3",
    ];
    for ((line, front), back) in lines.iter().skip(3).zip(fronts).zip([SCHOLAR_MATE, ROOK_CHECKS, "1...Qf1#"]) {
        let fields: Vec<&str> = line.split('\t').collect();
        assert_eq!(fields.len(), 2, "front and back: {line}");
        assert!(fields.first().is_some_and(|f| f.starts_with(front)), "{line}");
        assert_eq!(fields.get(1), Some(&back), "{line}");
    }
    assert_eq!(anki_deck(&doc, &book).tsv(|_| true), tsv, "the same file on every run");
}

/// A field never breaks Anki's line or its HTML: the book's `<`, `&` and `"` are entities, a tab
/// or a line break a space; a card whose picture was not drawn shows its FEN in its place.
#[test]
fn an_anki_field_keeps_to_its_line_and_its_text() {
    let card = Card {
        exercise: Exercise { at: DiagramAt::on(4, 1), number: 12, moves: 1 },
        image: image_name("book", 4, 1),
        fen: ROOK.into(),
        flipped: false,
        statement: "White to move (№ 12).\tA <b>bold</b> & \"quoted\"\nline".into(),
        solution: "1.Rh6+".into(),
    };
    let deck = AnkiDeck { prefix: "book".into(), cards: vec![card], left_out: Vec::new() };
    let tsv = deck.tsv(|_| true);
    let row = tsv.lines().nth(3).unwrap_or_default();
    assert_eq!(
        row, "<img src=\"book-p0005-2.png\"><br>White to move (№ 12). A &lt;b&gt;bold&lt;/b&gt; &amp; &quot;quoted&quot; line\t1.Rh6+",
        "{tsv}"
    );
    assert_eq!(tsv.lines().count(), 4, "{tsv}");
    let undrawn = deck.tsv(|_| false);
    assert!(undrawn.lines().nth(3).is_some_and(|l| l.starts_with(&format!("FEN {ROOK}<br>White to move"))), "{undrawn}");
}

/// The pictures' names are safe file names on every system, and the same on every run: the
/// title's ASCII letters and digits; with other letters, or past the cap, a hash of the title
/// keeps two books apart in Anki's one media folder.
#[test]
fn the_pictures_names_are_safe_and_the_same_on_every_run() {
    let hashed = |p: &str, head: &str| {
        p.strip_prefix(head).and_then(|h| h.strip_prefix('-')).is_some_and(|h| h.len() == 8 && h.chars().all(|c| c.is_ascii_hexdigit()))
    };
    assert_eq!(media_prefix("My Book: Vol. 2"), "my-book-vol-2");
    assert_eq!(media_prefix("  ../..\\CON*?<>|  "), "con");
    assert_eq!(media_prefix(""), "exercises");
    assert_eq!(media_prefix("---"), "exercises");
    let russian = media_prefix("Шахматы");
    assert!(hashed(&russian, "exercises"), "{russian}");
    assert_ne!(russian, media_prefix("Шахматы 2"), "two titles in other letters stay apart");
    let mixed = media_prefix("Xadrez Básico");
    assert!(hashed(&mixed, "xadrez-b-sico"), "{mixed}");
    let long = media_prefix(&"a".repeat(60));
    assert!(hashed(&long, &"a".repeat(MAX_PREFIX)), "{long}");
    assert_ne!(long, media_prefix(&"a".repeat(61)), "titles past the cap stay apart");
    for title in ["My Book: Vol. 2", "  ../..\\CON*?<>|  ", "Шахматы", "Xadrez Básico", "\u{0}\t.\n", "a/b\\c:d"] {
        let prefix = media_prefix(title);
        assert_eq!(prefix, media_prefix(title), "the same on every run");
        let name = image_name(&prefix, 0, 0);
        assert!(name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.'), "{name}");
        assert!(!name.starts_with(['-', '.']) && name.ends_with("-p0001-1.png"), "{name}");
    }
    assert_eq!(image_name("x", usize::MAX, usize::MAX), format!("x-p{}-{}.png", usize::MAX, usize::MAX), "no overflow");
}
