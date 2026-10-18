//! The HTML writer, checked by reading its output back (the idea of Caïssa's `fidelity.py`):
//! as many diagrams with the same FENs, every move with its SAN and a figurine where it moves a
//! piece, and nothing a browser would run.

use pdfcraft_chess::draw::{BoardStyle, PieceSet};

use super::*;
use crate::doc::html::{DiagramMode, Figurines, HtmlOptions, write};

fn options(pieces: &PieceSet) -> HtmlOptions<'_> {
    HtmlOptions {
        title: "Chapter <1> & \"more\"",
        pieces,
        style: BoardStyle::default(),
        diagrams: DiagramMode::Svg,
        figurines: Figurines::Unicode,
        links: true,
        link_text: "Analyze on lichess",
    }
}

/// Every value of `attr="…"` in `html`, in order, unescaped for `&quot;`/`&amp;` only.
fn attrs(html: &str, attr: &str) -> Vec<String> {
    let key = format!(" {attr}=\"");
    html.match_indices(&key)
        .filter_map(|(i, _)| {
            let rest = html.get(i + key.len()..)?;
            rest.split_once('"').map(|(v, _)| v.replace("&quot;", "\"").replace("&amp;", "&"))
        })
        .collect()
}

fn showcase_doc() -> ChessDoc {
    let (text, diagrams, hints) = showcase();
    build_page(&input(0, &text, &diagrams, &hints, &[]))
}

#[test]
fn the_html_reads_back_as_the_document() {
    let pieces = PieceSet::geometric();
    let doc = showcase_doc();
    let html = write(&doc, &options(&pieces));
    // Diagrams: as many, the same FENs, in the same order.
    let fens: Vec<String> = doc
        .pages
        .iter()
        .flat_map(|p| &p.blocks)
        .filter_map(|b| if let Block::Diagram(d) = b { d.reading.as_ref().map(|r| r.fen.clone()) } else { None })
        .collect();
    assert_eq!(fens, vec![SCHOLAR.to_owned()]);
    let figure_fens: Vec<String> = html.split("<figure").skip(1).flat_map(|f| attrs(&format!(" {f}"), "data-fen").into_iter().take(1)).collect();
    assert_eq!(figure_fens, fens);
    assert_eq!(html.matches("<svg").count(), 1);
    assert!(html.contains("<title>rnbqkbnr") || html.contains(&format!("<title>{SCHOLAR}")), "the board's accessible name is its FEN");
    // Moves: every one, with its SAN; figurines for piece moves, none for pawn moves.
    let moves: Vec<&MoveInline> = doc
        .pages
        .iter()
        .flat_map(|p| &p.blocks)
        .flat_map(|b| b.inlines())
        .filter_map(|i| if let Inline::Move(m) = i { Some(m) } else { None })
        .collect();
    assert_eq!(attrs(&html, "data-san"), moves.iter().map(|m| m.san.clone()).collect::<Vec<_>>());
    let piece_moves = moves.iter().filter(|m| m.san.starts_with(['K', 'Q', 'R', 'B', 'N'])).count();
    let figurines = html.chars().filter(|c| pdfcraft_chess::san::figurine_piece(*c).is_some()).count();
    assert_eq!(figurines, piece_moves, "{html}");
    assert!(html.contains("♛h5") && html.contains(">1.e4<"), "{html}");
    // The illegal move and the ones after it say so; NAGs keep their glyph and code.
    assert!(html.contains("data-san=\"Bd7\" data-status=\"illegal\""), "{html}");
    assert!(html.contains("data-nag=\"1\"") && html.contains("data-nag=\"4\""), "{html}");
    // The link under the diagram; the title is escaped; there is no script.
    assert_eq!(html.matches("href=\"https://lichess.org/analysis/").count(), 1);
    assert!(html.contains("<title>Chapter &lt;1&gt; &amp; &quot;more&quot;</title>"));
    assert!(!html.to_ascii_lowercase().contains("<script") && !html.contains("javascript:"));
}

#[test]
fn options_change_diagrams_letters_and_links() {
    let pieces = PieceSet::geometric();
    let doc = showcase_doc();
    let html = write(&doc, &HtmlOptions { diagrams: DiagramMode::Fen, figurines: Figurines::Letters(Lang::Pt), links: false, ..options(&pieces) });
    assert_eq!(html.matches("<svg").count(), 0);
    assert!(html.contains(&format!("<code>{SCHOLAR}</code>")));
    assert!(html.contains(">3.Dh5<"), "Portuguese letters: {html}");
    assert!(!html.contains("lichess.org"));
    let printed = write(&doc, &HtmlOptions { figurines: Figurines::AsPrinted, ..options(&pieces) });
    assert!(printed.contains(">3.Qh5<"), "{printed}");
}

#[test]
fn an_unread_diagram_is_a_figure_without_a_position_and_no_unsafe_link_is_made() {
    let mut r = reading(0, 60.0, SCHOLAR, true, false);
    r.outcome = Err(crate::board::Unreadable::NoModel);
    let text = page_text(vec![line(230.0, "1.Qh5 Nf6")]);
    let mut doc = build_page(&input(0, &text, &[r], &[], &[]));
    // A link inline whose address is a script, and a diagram link that is not http(s).
    doc.pages[0].blocks.push(Block::Paragraph {
        inlines: vec![Inline::Link { url: "javascript:alert(1)".into(), text: "click".into(), source_rect: None, review: Vec::new() }],
        source_rect: None,
    });
    let pieces = PieceSet::geometric();
    let html = write(&doc, &options(&pieces));
    assert!(html.contains("<figure class=\"diagram unread review\""), "{html}");
    assert!(!html.split("<figure").nth(1).unwrap_or_default().split("</figure>").next().unwrap_or_default().contains("data-fen"));
    assert!(html.contains(">click<") && !html.contains("javascript:"), "{html}");
    // Unchecked moves are marked so.
    assert!(html.contains("data-status=\"unchecked\""));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Any text through the writer: never a raw `<` from the text, and never a panic.
    #[test]
    fn arbitrary_text_is_escaped(words in proptest::collection::vec("\\PC{0,12}", 1..12)) {
        let text = page_text(vec![line(230.0, &words.join(" "))]);
        let doc = build_page(&input(0, &text, &[], &[], &[]));
        let pieces = PieceSet::geometric();
        let html = write(&doc, &options(&pieces));
        let body = html.split("<body>").nth(1).unwrap_or_default();
        // Every `<` in the body opens one of the writer's own tags.
        for (i, _) in body.match_indices('<') {
            let tag = &body[i..];
            prop_assert!(["<section", "</section", "<p>", "</p>", "<span", "</span", "<h", "</h", "<figure", "</figure", "</body", "<a ", "</a"].iter().any(|t| tag.starts_with(t)), "{tag:.40}");
        }
    }
}
