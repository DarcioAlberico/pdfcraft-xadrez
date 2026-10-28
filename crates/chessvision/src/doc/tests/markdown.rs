//! Markdown and PGN-with-diagrams, checked by reading them back: the diagrams' images and FENs
//! in order, text that stays text, and a PGN game for every legal position.

use super::*;
use crate::doc::html::Figurines;
use crate::doc::markdown::{MarkdownDiagrams, MarkdownOptions, PgnBook, image_name, pgn_with_diagrams, pgn_with_diagrams_in, write};

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
    let (pgn, added) = pgn_with_diagrams_in(&doc, &book);
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
