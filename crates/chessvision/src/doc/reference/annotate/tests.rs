use proptest::prelude::*;

use super::super::tests::{arb_block, diagram, doc, fen_after, mv, para, played};
use super::*;
use crate::doc::ChessPage;

fn text(t: &str) -> Inline {
    Inline::Text { text: t.to_string(), source_rect: None, review: Vec::new() }
}

fn heading(t: &str) -> Block {
    Block::Heading { level: 1, inlines: vec![text(t)], source_rect: None }
}

/// The games of a PGN, read back; none may stop short.
fn games_of(pgn: &str) -> Vec<PgnGame> {
    pdfcraft_chess::game::split_pgn(pgn, 100)
        .into_iter()
        .map(|chunk| {
            let (game, report) = PgnGame::from_pgn_report(chunk).unwrap();
            assert_eq!(report.stopped, None, "a line stopped in {chunk}");
            game
        })
        .collect()
}

/// How many moves `game` holds, all lines.
fn nodes(game: &PgnGame) -> usize {
    (1..).take_while(|&n| game.fen_at(n).is_some()).count()
}

/// The node of `game` that `line` reaches from the start.
fn at(game: &PgnGame, line: &[&str]) -> usize {
    let mut node = 0;
    for san in line {
        node = *game.replies(node).iter().find(|&&r| game.san(r).trim_end_matches(['+', '#']) == *san).unwrap_or_else(|| panic!("no {san}"));
    }
    node
}

const PGN: &str = "[Event \"Ch 1\"]\n\n1.e4 e6 2.d4 d5 3.Nc3 Bb4 4.e5 c5 *\n\n[Event \"Ch 2\"]\n\n1.d4 Nf6 2.c4 *\n";

/// A book of two pages: the first discusses game 1, with a heading, a diagram and its caption, a
/// line label, a figurine in the prose and a move the PGN does not have; the second, game 2.
fn book() -> ChessDoc {
    let main = ["e4", "e6", "d4", "d5", "Nc3", "Bb4"];
    let mut first = played(&[], &main);
    first.insert(0, text("The French:"));
    first.insert(3, text("is solid."));
    first.insert(7, text("Now ♗b4 pins."));
    let mut line = vec![text("A)")];
    line.extend(played(&main, &["e5"]));
    line.push(text("gains space."));
    line.push(mv(Some((4, false)), "a3", MoveStatus::Valid, Some(fen_after(&["e4", "e6", "d4", "d5", "Nc3", "Bb4", "a3"]))));
    line.push(text("is another try."));
    let caption = vec![text("Position after:"), mv(Some((3, true)), "Bb4", MoveStatus::Caption, None), text("The bishop pins.")];
    let mut d = doc(vec![
        vec![heading("Chapter 1"), para(first), diagram(0, &fen_after(&main)), para(caption), para(line)],
        vec![heading("Capítulo 2"), para(played(&[], &["d4", "Nf6"]))],
    ]);
    if let Some(p) = d.pages.first_mut() {
        p.lang = "pt".into();
    }
    d
}

#[test]
fn the_books_text_becomes_the_comments_of_its_pgn() {
    let index = ReferenceIndex::from_pgn(PGN).unwrap();
    let book_info = PgnBook { title: Some("Beat the French".into()), labels: vec!["7".into(), "8".into()] };
    let made = annotated_pgn(&book(), &index, &book_info, PgnStyle::FULL);
    assert_eq!((made.games, made.book_only, made.diagrams), (2, 1, 1), "{}", made.pgn);
    let games = games_of(&made.pgn);
    assert_eq!(games.len(), 2);
    let g = &games[0];
    assert_eq!(nodes(g), 9, "the PGN's 8 moves and the book's 4.a3: {}", made.pgn);
    // Text before the first move goes before it; between two moves, after the first.
    assert_eq!(g.comment(0), "Chapter 1 The French:");
    assert_eq!(g.comment(at(g, &["e4", "e6"])), "is solid.");
    // Figurines become letters; the caption's cue goes, its other words and the line label stay.
    assert_eq!(g.comment(at(g, &["e4", "e6", "d4", "d5", "Nc3"])), "Now Bb4 pins.");
    let bb4 = at(g, &["e4", "e6", "d4", "d5", "Nc3", "Bb4"]);
    assert_eq!(g.comment(bb4), "The bishop pins. A)");
    assert!(g.nags(bb4).contains(&pdfcraft_chess::nag::DIAGRAM), "{}", made.pgn);
    assert!(!made.pgn.contains("Position after"), "{}", made.pgn);
    assert_eq!(g.comment(at(g, &["e4", "e6", "d4", "d5", "Nc3", "Bb4", "e5"])), "gains space.");
    // The move only the book has: a variation, said so in the book's language.
    let a3 = at(g, &["e4", "e6", "d4", "d5", "Nc3", "Bb4", "a3"]);
    assert_eq!(g.replies(bb4).first().map(|&r| g.san(r)), Some("e5"), "the PGN's move stays the main line");
    assert_eq!(g.comment(a3), "só no livro. is another try.");
    // The origin's tags, from the first page that discusses each game.
    assert_eq!((g.tag("PdfPage"), g.tag("PdfPageLabel"), g.tag("SourceTitle")), (Some("1"), Some("7"), Some("Beat the French")));
    let g2 = &games[1];
    assert_eq!((g2.tag("PdfPage"), g2.comment(0)), (Some("2"), "Capítulo 2"));
    assert_eq!(nodes(g2), 3, "a move the book does not discuss is kept");
    assert_eq!(made.commented, 5, "{}", made.pgn);
}

#[test]
fn a_book_with_nothing_of_the_pgn_writes_the_pgn_as_it_is() {
    let index = ReferenceIndex::from_pgn(PGN).unwrap();
    let other = doc(vec![vec![para(vec![text("A book about something else."), mv(None, "c4", MoveStatus::Unchecked, None)])]]);
    let made = annotated_pgn(&other, &index, &PgnBook::default(), PgnStyle::FULL);
    assert_eq!((made.games, made.commented, made.book_only, made.diagrams), (2, 0, 0, 0));
    let games = games_of(&made.pgn);
    assert_eq!(games.iter().map(nodes).collect::<Vec<_>>(), [8, 3]);
    assert!(games.iter().all(|g| g.tag("PdfPage").is_none()));
    // An empty document too.
    let empty = ChessDoc { pages: Vec::<ChessPage>::new(), ..other };
    assert_eq!(games_of(&annotated_pgn(&empty, &index, &PgnBook::default(), PgnStyle::FULL).pgn).len(), 2);
}

/// A caption read as a plain move (not as a caption) still loses its "Position after:" cue, in
/// any language the reader knows; "after" in prose before a new move stays.
#[test]
fn a_caption_cue_before_the_move_it_names_is_left_out() {
    let index = ReferenceIndex::from_pgn(PGN).unwrap();
    for (cue, kept) in [("Position after:", ""), ("Posição depois de", ""), ("The move. Stellung nach", "The move.")] {
        let e6 = fen_after(&["e4", "e6"]);
        let caption = vec![text(cue), mv(Some((1, true)), "e6", MoveStatus::Valid, Some(e6.clone())), text("Black is solid.")];
        let prose = vec![text("Play goes on after"), mv(Some((2, false)), "d4", MoveStatus::Valid, Some(fen_after(&["e4", "e6", "d4"])))];
        let d = doc(vec![vec![para(played(&[], &["e4", "e6"])), diagram(0, &e6), para(caption), para(prose)]]);
        let made = annotated_pgn(&d, &index, &PgnBook::default(), PgnStyle::FULL);
        let g = &games_of(&made.pgn)[0];
        let want =
            if kept.is_empty() { "Black is solid. Play goes on after".to_string() } else { format!("{kept} Black is solid. Play goes on after") };
        assert_eq!(g.comment(at(g, &["e4", "e6"])), want, "{cue}: {}", made.pgn);
    }
}

#[test]
fn figurines_become_letters() {
    assert_eq!(letters("♔♕♖♗♘♙ ♚♛♜♝♞♟ e4"), "KQRBN KQRBN e4");
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 96, ..ProptestConfig::default() })]

    /// Any document and PGN: no panic, and the PGN written reads back whole, with at least every
    /// move of the reference in each game.
    #[test]
    fn the_annotated_pgn_reads_back_with_every_move(
        pages in prop::collection::vec(prop::collection::vec(arb_block(), 0..6), 0..4),
        pgn in prop_oneof![
            Just("1.e4 e5 (1...c5 2.Nf3) 2.Nf3 Nc6 3.Bb5 a6 *\n\n[Event \"b\"]\n\n1.Nf3 Nc6 2.e4 e5 0-0 *".to_string()),
            "[\\[\\]\" a-hKQRBNOx0-9.+#=(){}*-]{0,80}",
        ],
    ) {
        if let Ok(index) = ReferenceIndex::from_pgn(&pgn) {
            let made = annotated_pgn(&doc(pages), &index, &PgnBook::default(), PgnStyle::FULL);
            let read: Vec<&PgnGame> = index.games().iter().filter_map(|g| g.game.as_ref()).collect();
            let back = games_of(&made.pgn);
            prop_assert_eq!(back.len(), read.len());
            for (a, b) in read.iter().zip(&back) {
                prop_assert!(nodes(b) >= nodes(a), "{} < {}", nodes(b), nodes(a));
            }
        }
    }
}
