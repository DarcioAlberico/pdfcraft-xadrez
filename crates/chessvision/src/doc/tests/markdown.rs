//! Markdown and PGN-with-diagrams, checked by reading them back: the diagrams' images and FENs
//! in order, text that stays text, and a PGN game for every legal position.

use super::*;
use crate::doc::games::DiagramAt;
use crate::doc::html::Figurines;
use crate::doc::markdown::{
    MarkdownDiagrams, MarkdownOptions, PgnBook, PgnPositions, SkipReason, SkippedPosition, image_name, pgn_export, pgn_with_diagrams, write,
};

fn options() -> MarkdownOptions<'static> {
    MarkdownOptions {
        title: "Book *one*",
        figurines: Figurines::Unicode,
        diagrams: MarkdownDiagrams::Images,
        links: true,
        link_text: "Analyze on lichess",
        image_dir: "",
    }
}

#[test]
fn the_markdown_reads_back_as_the_document() {
    let (text, diagrams, hints) = showcase();
    let doc = build_page(&input(0, &text, &diagrams, &hints, &[]));
    let md = write(&doc, &options());
    assert!(md.starts_with("# Book \\*one\\*\n\n## Chapter 1\n\n"), "{md}");
    // The diagram as an image named for its page and number, its FEN as the alt text, then its link.
    let image = format!("![{SCHOLAR}]({})", image_name(0, 0));
    assert_eq!(md.matches("![").count(), 1);
    let at = md.find(&image).unwrap_or_else(|| panic!("{md}"));
    let link = md.find("[Analyze on lichess](https://lichess.org/analysis/").unwrap();
    assert!(at < link, "{md}");
    assert!(md.contains("\n3.♛h5! ♞f6?? (3...g6 4.♛f3) 4.♛xf7# 1-0\n"), "the mate sign stays as printed: {md}");
    assert!(md.contains("### A game") || md.contains("## A game"), "{md}");
    // FEN instead of images, no links, letters.
    let md = write(&doc, &MarkdownOptions { diagrams: MarkdownDiagrams::Fen, links: false, figurines: Figurines::Letters(Lang::De), ..options() });
    assert!(md.contains(&format!("`{SCHOLAR}`")) && !md.contains("![") && !md.contains("lichess"), "{md}");
    assert!(md.contains("3.Dh5!"), "German letters: {md}");
    let md = write(&doc, &MarkdownOptions { image_dir: "My book_diagrams/", ..options() });
    assert!(md.contains(&format!("](My%20book_diagrams/{})", image_name(0, 0))), "{md}");
}

#[test]
fn text_stays_text_and_unread_diagrams_say_so() {
    let mut r = reading(0, 60.0, SCHOLAR, true, false);
    r.outcome = Err(crate::board::Unreadable::NoModel);
    let text = page_text(vec![line(230.0, "Some prose here.")]);
    let mut doc = build_page(&input(0, &text, &[r], &[], &[]));
    let plain =
        |t: &str| Block::Paragraph { inlines: vec![Inline::Text { text: t.into(), source_rect: None, review: Vec::new() }], source_rect: None };
    doc.pages[0].blocks.extend([plain("1. e4 is a move"), plain("- not a list"), plain("a*b_c [x](y) <b> # ~ |"), plain("# not a heading")]);
    let md = write(&doc, &options());
    assert!(md.contains("*Diagram 1 (not read)*"), "{md}");
    assert!(md.contains("\n1\\. e4 is a move\n") && md.contains("\n\\- not a list\n"), "{md}");
    assert!(md.contains("a\\*b\\_c \\[x\\](y) \\<b\\> # \\~ \\|"), "{md}");
    assert!(md.contains("\n\\# not a heading\n"), "{md}");
}

#[test]
fn the_pgn_has_a_game_for_every_legal_position() {
    // Diagram 0: its solution follows (a game). Diagram 1: legal, nothing follows (a position).
    // Diagram 2: illegal (no game). Diagram 3: repaired (no game).
    let text = page_text(vec![line(230.0, "3.Qh5 Nf6 4.Qxf7#"), line(700.0, "The end.")]);
    let illegal = "8/8/8/8/8/8/8/8 w - - 0 1";
    let mut repaired = reading(3, 760.0, "4k3/8/8/8/8/8/8/4K3 w - - 0 1", true, false);
    if let Ok(p) = &mut repaired.outcome {
        p.repairs.push(Change { square: Square::from_index(0).unwrap(), from: None, to: Some(Piece::new(Color::White, PieceKind::King)) });
    }
    let readings = vec![
        reading(0, 60.0, SCHOLAR, true, false),
        reading(1, 300.0, "4k3/8/8/8/8/8/8/R3K3 w Q - 0 1", false, false),
        reading(2, 500.0, illegal, false, false),
        repaired,
    ];
    let doc = build_page(&input(0, &text, &readings, &[], &[]));
    let (pgn, added) = pgn_with_diagrams(&doc);
    assert_eq!(added, 1, "{pgn}");
    let games = pgn::parse(&pgn).unwrap();
    assert_eq!(games.len(), 2, "{pgn}");
    assert_eq!(games[0].start_fen.as_deref(), Some(SCHOLAR));
    assert!(!games[0].nodes.is_empty());
    assert_eq!(games[1].start_fen.as_deref(), Some("4k3/8/8/8/8/8/8/R3K3 w Q - 0 1"));
    assert!(games[1].nodes.is_empty());
    assert!(games[1].extra_tags.iter().any(|t| t.name == "PdfDiagram" && t.value == "2"), "{:?}", games[1].extra_tags);
    assert!(games[1].comment.contains("did not pass review"), "not accepted, so said");
    // Order: the book's.
    assert!(pgn.find("[PdfDiagram \"1\"]").unwrap() < pgn.find("[PdfDiagram \"2\"]").unwrap());
}

#[test]
fn each_pgn_game_says_where_in_the_book_it_came_from() {
    // Diagram 1 starts a game, diagram 2 is a position (plan A8 P8a).
    let text = page_text(vec![line(230.0, "3.Qh5 Nf6 4.Qxf7#")]);
    let readings = vec![reading(0, 60.0, SCHOLAR, true, false), reading(1, 300.0, "4k3/8/8/8/8/8/8/R3K3 w Q - 0 1", true, false)];
    let doc = build_page(&input(0, &text, &readings, &[], &[]));
    let book = PgnBook { title: Some("My  book".to_owned()), labels: vec!["xvii".to_owned()] };
    let made = pgn_export(&doc, &book, PgnPositions::Unused);
    let (pgn, added) = (made.pgn, made.positions);
    assert_eq!(added, 1, "{pgn}");
    let games = pgn::parse(&pgn).unwrap();
    let tags = |g: &crate::doc::games::GameScore| g.extra_tags.iter().map(|t| format!("{}={}", t.name, t.value)).collect::<Vec<_>>();
    assert_eq!(tags(&games[0]), ["PdfPage=1", "PdfPageLabel=xvii", "PdfDiagram=1", "SourceTitle=My book"], "{pgn}");
    assert!(games[0].comment.starts_with("Page xvii · diagram 1 of 2"), "{pgn}");
    assert_eq!(games[0].roster.event, "?", "a game keeps its own event");
    // The position: the book's title as its event, Round "-", its comment after the origin.
    assert_eq!(tags(&games[1]), ["PdfPage=1", "PdfPageLabel=xvii", "PdfDiagram=2", "SourceTitle=My book"]);
    assert_eq!((games[1].roster.event.as_str(), games[1].roster.round.as_str()), ("My book", "-"));
    assert_eq!(games[1].comment, "Page xvii · diagram 2 of 2");
    // Without a book, only what the reading knows: the physical page and the diagram.
    let (plain, _) = pgn_with_diagrams(&doc);
    let games = pgn::parse(&plain).unwrap();
    assert_eq!(tags(&games[1]), ["PdfPage=1", "PdfDiagram=2"]);
    assert_eq!(games[1].roster.event, "?");
    assert!(!plain.contains("PdfPageLabel") && !plain.contains("SourceTitle"), "{plain}");
}

/// The diagrams that get a game of their own are those `positions` says, in the order of the
/// book; those that were to and could not are listed with the reason (plan A8 P8b).
#[test]
fn the_pgn_positions_option_says_which_diagrams_get_a_game() {
    // Diagram 1 starts a game; 2 is a position; 3 is illegal, 4 repaired, 5 not read.
    let text = page_text(vec![line(230.0, "3.Qh5 Nf6 4.Qxf7#")]);
    let mut repaired = reading(3, 600.0, "4k3/8/8/8/8/8/8/4K3 w - - 0 1", true, false);
    if let Ok(p) = &mut repaired.outcome {
        p.repairs.push(Change { square: Square::from_index(0).unwrap(), from: None, to: Some(Piece::new(Color::White, PieceKind::King)) });
    }
    let mut unread = reading(4, 700.0, SCHOLAR, true, false);
    unread.outcome = Err(crate::board::Unreadable::NoModel);
    let readings = vec![
        reading(0, 60.0, SCHOLAR, true, false),
        reading(1, 300.0, "4k3/8/8/8/8/8/8/R3K3 w Q - 0 1", true, false),
        reading(2, 500.0, "8/8/8/8/8/8/8/8 w - - 0 1", true, false),
        repaired,
        unread,
    ];
    let doc = build_page(&input(0, &text, &readings, &[], &[]));
    let diagram = |pgn: &str| -> Vec<String> {
        pgn::parse(pgn)
            .unwrap()
            .iter()
            .map(|g| {
                format!(
                    "{}{}",
                    g.extra_tags.iter().find(|t| t.name == "PdfDiagram").map_or("?", |t| &t.value),
                    if g.nodes.is_empty() { "" } else { "+moves" }
                )
            })
            .collect()
    };
    let skipped = |made: &crate::doc::markdown::PgnExport| made.skipped.iter().map(|s| (s.at.diagram, s.reason)).collect::<Vec<_>>();
    let bad = vec![(2, SkipReason::Illegal), (3, SkipReason::Repaired), (4, SkipReason::NotRead)];

    let unused = pgn_export(&doc, &PgnBook::default(), PgnPositions::Unused);
    assert_eq!((diagram(&unused.pgn), unused.positions), (vec!["1+moves".to_owned(), "2".to_owned()], 1), "{}", unused.pgn);
    assert_eq!(skipped(&unused), bad);
    // All: the diagram a game starts from also has a game of its own, before that game.
    let all = pgn_export(&doc, &PgnBook::default(), PgnPositions::All);
    assert_eq!((diagram(&all.pgn), all.positions), (vec!["1".to_owned(), "1+moves".to_owned(), "2".to_owned()], 2), "{}", all.pgn);
    assert_eq!(skipped(&all), bad);
    // None: only the games, and nothing said to be skipped.
    let none = pgn_export(&doc, &PgnBook::default(), PgnPositions::None);
    assert_eq!((diagram(&none.pgn), none.positions), (vec!["1+moves".to_owned()], 0), "{}", none.pgn);
    assert_eq!(none.skipped, Vec::<SkippedPosition>::new());
    assert_eq!(unused.skipped.first().map(|s| s.at), Some(DiagramAt { page: 0, diagram: 2 }));
    for (name, positions) in [("all", PgnPositions::All), ("unused", PgnPositions::Unused), ("none", PgnPositions::None)] {
        assert_eq!(PgnPositions::from_name(name), Some(positions));
    }
    assert_eq!(PgnPositions::from_name("used"), None);
}

/// A diagram that reprints a game's position marks the move with `$220` and, by default, gets no
/// game of its own: the game holds it (plan A8 P8b).
#[test]
fn a_diagram_reprinting_a_game_marks_its_move() {
    let after = "rnbqkbnr/pppp1ppp/8/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R b KQkq - 1 2";
    let mut b = DocBuilder::new();
    b.add_page(&input(0, &page_text(vec![line(700.0, "1.e4 e5 2.Nf3")]), &[], &[], &[]));
    b.add_page(&input(1, &page_text(vec![line(230.0, "2...Nc6 3.Bb5")]), &[reading(0, 60.0, after, true, false)], &[], &[]));
    let doc = b.finish();
    assert_eq!(doc.games.len(), 1);
    let unused = pgn_export(&doc, &PgnBook::default(), PgnPositions::Unused);
    assert_eq!(unused.positions, 0, "{}", unused.pgn);
    assert!(unused.pgn.contains("2. Nf3 $220 Nc6"), "{}", unused.pgn);
    assert_eq!(pgn::parse(&unused.pgn).unwrap().len(), 1);
    // The reading's own game keeps no mark: the book's text has none.
    assert!(doc.games[0].score.nodes.iter().all(|n| n.nags.is_empty()));
    // All: the position too, and the mark.
    let all = pgn_export(&doc, &PgnBook::default(), PgnPositions::All);
    assert_eq!(all.positions, 1, "{}", all.pgn);
    assert!(all.pgn.contains("2. Nf3 $220 Nc6"), "{}", all.pgn);
    // A mark the game has already is not doubled.
    let mut marked = doc.clone();
    for n in &mut marked.games[0].score.nodes {
        if n.san == "Nf3" {
            n.nags.push(pdfcraft_chess::nag::DIAGRAM_FROM_BLACK);
        }
    }
    let once = pgn_export(&marked, &PgnBook::default(), PgnPositions::Unused);
    assert!(once.pgn.contains("2. Nf3 $221 Nc6") && !once.pgn.contains("$220"), "{}", once.pgn);
}
