//! From games to a book (plan A9-N9): a page per game, its header, a diagram at its own start and
//! at each `$220`, the main line in bold and its variations in a paragraph of their own.

use pdfcraft_chess::draw::PieceSet;
use pdfcraft_chess::game::{Game as ChessGame, split_pgn};

use super::*;
use crate::doc::from_games::{FromGames, from_games};

const TWO: &str = "[Event \"Paris\"]\n[White \"Morphy\"]\n[Black \"Duke\"]\n[Date \"1858.??.??\"]\n\n\
1.e4 {The king's pawn.} e5 2.Nf3 d6 $220 3.d4 (3.Bc4 Be7) Bg4 $2 1-0\n\n\
[SetUp \"1\"]\n[FEN \"6k1/5ppp/8/8/8/8/5PPP/3R2K1 w - - 0 1\"]\n\n1.Rd8# 1-0\n";

fn games(pgn: &str) -> Vec<ChessGame> {
    split_pgn(pgn, 16).into_iter().map(|g| ChessGame::from_pgn(g).unwrap().0).collect()
}

fn after(moves: &[&str]) -> String {
    let mut g = ChessGame::standard();
    for m in moves {
        g.play_san(m).unwrap();
    }
    g.fen()
}

/// What each block of `page` is.
fn shape(page: &ChessPage) -> Vec<&'static str> {
    page.blocks
        .iter()
        .map(|b| match b {
            Block::Heading { .. } => "heading",
            Block::Paragraph { .. } => "paragraph",
            Block::Diagram(_) => "diagram",
        })
        .collect()
}

/// A paragraph's moves as their number, SAN and `*` when bold, and its other inlines as their
/// text.
fn paragraph(block: &Block) -> Vec<String> {
    block
        .inlines()
        .iter()
        .map(|i| match i {
            Inline::Move(m) => format!("{}{}{}", m.number.map(|n| n.text()).unwrap_or_default(), m.san, if m.bold { "*" } else { "" }),
            other => other.text(),
        })
        .collect()
}

#[test]
fn games_become_a_book_a_page_each() {
    let doc = from_games(&games(TWO), &FromGames::default());
    assert!(doc.games.is_empty(), "the games are the input");
    let [first, second] = doc.pages.as_slice() else { panic!("{} pages", doc.pages.len()) };
    assert_eq!((first.index, second.index), (0, 1));
    assert_eq!(shape(first), ["heading", "paragraph", "paragraph", "diagram", "paragraph", "paragraph", "paragraph"]);
    assert_eq!(super::plain_text(first.blocks[0].inlines()), "Morphy – Duke");
    assert_eq!(super::plain_text(first.blocks[1].inlines()), "Paris, 1858.??.??");
    assert_eq!(first.blocks[1].role(), Some(BlockRole::GameHeader));
    // The main line in bold, its comment as text; the diagram after the marked move, its mark no
    // glyph; the variations apart, in regular type; the main line on again, numbered.
    assert_eq!(paragraph(&first.blocks[2]), ["1.e4*", "The king's pawn.", "1...e5*", "2.Nf3*", "d6*"]);
    let Block::Diagram(d) = &first.blocks[3] else { panic!() };
    let d6 = after(&["e4", "e5", "Nf3", "d6"]);
    assert_eq!((d.number, d.shown().map(|s| s.fen)), (0, Some(d6.as_str())));
    assert!(d.open_review().next().is_none() && d.reading.as_ref().is_some_and(|r| r.accepted && !r.side_assumed()));
    assert!(d.link().is_some_and(|l| l.starts_with("https://lichess.org/analysis/")));
    assert_eq!(paragraph(&first.blocks[4]), ["3.d4*"]);
    assert_eq!(paragraph(&first.blocks[5]), ["(", "3.Bc4", "Be7", ")"]);
    assert_eq!(paragraph(&first.blocks[6]), ["3...Bg4*", "?", "1-0"]);
    // Every move valid, with its position.
    let moves: Vec<&MoveInline> =
        first.blocks.iter().flat_map(|b| b.inlines()).filter_map(|i| if let Inline::Move(m) = i { Some(m) } else { None }).collect();
    assert!(moves.iter().all(|m| m.status == MoveStatus::Valid && m.fen.is_some()));
    assert_eq!(moves.last().and_then(|m| m.fen.as_deref()), Some(after(&["e4", "e5", "Nf3", "d6", "d4", "Bg4"]).as_str()));
    // A game from a position of its own shows it first, and is named by its number.
    assert_eq!(shape(second), ["heading", "diagram", "paragraph"]);
    assert_eq!(super::plain_text(second.blocks[0].inlines()), "Game 2");
    let Block::Diagram(start) = &second.blocks[1] else { panic!() };
    assert_eq!(start.shown().map(|s| s.fen), Some("6k1/5ppp/8/8/8/8/5PPP/3R2K1 w - - 0 1"));
    assert_eq!(paragraph(&second.blocks[2]), ["1.Rd8#*", "1-0"]);
}

/// The book is written by the exports as a book read from a PDF is: its HTML has a figure per
/// diagram with its position, a move per move with the position after it, and reads back.
#[test]
fn a_book_of_games_is_written_by_the_html_export() {
    use crate::doc::html::{DiagramMode, Figurines, HtmlOptions, write};
    let doc = from_games(&games(TWO), &FromGames { text_lang: "pt-BR".into() });
    assert!(doc.pages.iter().all(|p| p.text_lang == "pt-BR"));
    let pieces = PieceSet::geometric();
    let o = HtmlOptions {
        title: "Two games",
        pieces: &pieces,
        style: pdfcraft_chess::draw::BoardStyle::default(),
        diagrams: DiagramMode::Svg,
        figurines: Figurines::Unicode,
        links: true,
        link_text: "Analyze on lichess",
        text_font: None,
        sign_font: None,
        lang: "pt-BR",
        page_labels: &[],
        review_marks: true,
        naming: pdfcraft_chess::describe::Naming::Letters,
    };
    let html = write(&doc, &o);
    assert_eq!(html.matches("<figure class=\"diagram\"").count(), 2, "{html}");
    assert_eq!(html.matches("class=\"move").count(), 9, "{html}");
    assert!(!html.contains(" review\"") && !html.contains("class=\"review"), "nothing to review in games played: {html}");
    assert!(html.contains(&format!("data-fen=\"{}\"", after(&["e4", "e5", "Nf3", "d6"]))));
    // The JSON reads back.
    assert_eq!(ChessDoc::from_json(&doc.to_json().unwrap()).unwrap(), doc);
}

/// Plan A9-F7 (R6-01): a game downloaded from lichess has a clock and an evaluation in the
/// comment of every move; the book made of it shows each comment's prose and no command, so its
/// exports (HTML, EPUB, Word, RTF, Markdown) never print one. ChessBase's diagram mark, `{[#]}`
/// in a move's comment, is a diagram like `$220`.
#[test]
fn a_book_of_lichess_games_prints_no_command() {
    let pgn = "[Event \"Blitz\"]\n[White \"A\"]\n[Black \"B\"]\n\n\
1. e4 { [%eval 0.17] [%clk 0:03:00] } 1... e5 { [%eval 0.19] [%clk 0:03:00] } \
2. Nf3 { A developing move. [%eval 0.15] [%clk 0:02:58] } 2... d6 { [#] [%clk 0:02:57] } 3. d4 *\n";
    let doc = from_games(&games(pgn), &FromGames::default());
    let [page] = doc.pages.as_slice() else { panic!("{} pages", doc.pages.len()) };
    assert_eq!(shape(page), ["heading", "paragraph", "paragraph", "diagram", "paragraph"]);
    assert_eq!(paragraph(&page.blocks[2]), ["1.e4*", "1...e5*", "2.Nf3*", "A developing move.", "2...d6*"]);
    let Block::Diagram(d) = &page.blocks[3] else { panic!() };
    assert_eq!(d.shown().map(|s| s.fen), Some(after(&["e4", "e5", "Nf3", "d6"]).as_str()));
    let text: String = page.blocks.iter().map(|b| super::plain_text(b.inlines())).collect::<Vec<_>>().join("\n");
    assert!(!text.contains("[%") && !text.contains("[#]"), "{text}");
    let md = crate::doc::markdown::write(
        &doc,
        &crate::doc::markdown::MarkdownOptions {
            title: "Blitz",
            figurines: crate::doc::html::Figurines::Letters(pdfcraft_chess::san::Lang::En),
            diagrams: crate::doc::markdown::MarkdownDiagrams::Fen,
            links: false,
            link_text: "",
            image_dir: "",
            lang: "",
            page_labels: &[],
            naming: pdfcraft_chess::describe::Naming::Letters,
        },
    );
    assert!(!md.contains("[%") && !md.contains("[#]") && md.contains("A developing move."), "{md}");
}
