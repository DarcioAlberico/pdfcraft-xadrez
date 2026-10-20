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
        text_font: None,
        sign_font: None,
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

#[test]
fn the_users_figurine_font_is_embedded_for_the_chess_symbols_only() {
    let pieces = PieceSet::geometric();
    let doc = showcase_doc();
    let font = crate::text_font::from_bytes(b"wOF2 figurine font".to_vec(), "dejavu").unwrap();
    let html = write(&doc, &HtmlOptions { text_font: Some(&font), ..options(&pieces) });
    // One face, from a data URL, kept to the chess symbols; first in both families.
    assert_eq!(html.matches("@font-face").count(), 1);
    assert!(html.contains("src:url(\"data:font/woff2;base64,d09GMiBmaWd1cmluZSBmb250\") format(\"woff2\")"), "{html}");
    assert!(html.contains(&format!("unicode-range:{}", crate::text_font::UNICODE_RANGE)));
    assert!(html.contains("body{max-width:46em;margin:2em auto;padding:0 1em;font-family:\"PdfCraft Figurines\",serif;"));
    assert!(html.contains("figcaption{font-family:\"PdfCraft Figurines\",sans-serif;"));
    // Without a font, the style is as before.
    let plain = write(&doc, &options(&pieces));
    assert!(!plain.contains("@font-face") && !plain.contains("PdfCraft Figurines"));
    assert_eq!(plain.replace("font-family:serif", "").len() + "font-family:serif".len(), plain.len());
}

fn role_font(role: crate::text_font::FontRole) -> crate::text_font::TextFont {
    crate::text_font::TextFont { bytes: b"wOF2 font".to_vec(), format: crate::text_font::FontFormat::Woff2, role, family: String::new(), path: None }
}

#[test]
fn a_figurine_font_by_letters_draws_only_the_piece_letters_of_moves() {
    use crate::text_font::FontRole;
    let pieces = PieceSet::geometric();
    let doc = showcase_doc();
    let semfig = role_font(FontRole::Letters);
    let html = write(&doc, &HtmlOptions { text_font: Some(&semfig), ..options(&pieces) });
    // The face has no unicode-range and is used by the fig spans only; the body font is untouched.
    assert!(html.contains("@font-face{font-family:\"PdfCraft Figurines\";src:url(\"data:font/woff2;base64,"), "{html}");
    assert!(html.contains(".fig{font-family:\"PdfCraft Figurines\"}"));
    assert!(html.contains("font-family:serif;"), "the body keeps its font");
    // Every piece letter of a move is a fig span; the moves still carry their SAN.
    let spans = html.matches("<span class=\"fig\">").count();
    assert!(spans > 0, "{html}");
    for piece in ['K', 'Q', 'R', 'B', 'N'] {
        assert!(!html.contains(&format!("{}", ['♔', '♕', '♖', '♗', '♘'][['K', 'Q', 'R', 'B', 'N'].iter().position(|p| *p == piece).unwrap()])));
    }
    assert!(attrs(&html, "data-san").iter().all(|s| !s.is_empty()));
    // Prose is never put in the figurine font: only move spans hold fig spans.
    for (i, _) in html.match_indices("<span class=\"fig\">") {
        let before = &html[..i];
        let open = before.rfind("<span class=\"move").unwrap();
        assert!(
            !before[open..].contains("</span>") || before[open..].matches("<span").count() > before[open..].matches("</span>").count(),
            "a fig outside a move"
        );
    }
    // With figurines as letters of a language, the font is not used for moves.
    let letters =
        write(&doc, &HtmlOptions { text_font: Some(&semfig), figurines: Figurines::Letters(pdfcraft_chess::san::Lang::Pt), ..options(&pieces) });
    assert!(!letters.contains("<span class=\"fig\">"));
}

#[test]
fn the_sign_font_draws_the_annotation_glyphs_it_has() {
    use crate::text_font::FontRole;
    let pieces = PieceSet::geometric();
    let mut doc = showcase_doc();
    // White is clearly better (±): ISChess draws it on the letter O.
    let para = doc.pages[0].blocks.iter_mut().find_map(|b| if let Block::Paragraph { inlines, .. } = b { Some(inlines) } else { None }).unwrap();
    para.push(Inline::Nag { code: 16, source_rect: None, review: Vec::new() });
    let ischess = role_font(FontRole::Signs);
    let html = write(&doc, &HtmlOptions { sign_font: Some(&ischess), ..options(&pieces) });
    assert!(html.contains("@font-face{font-family:\"PdfCraft Signs\""), "{html}");
    assert!(html.contains(".sign{font-family:\"PdfCraft Signs\"}"));
    assert!(html.contains("<span class=\"nag sign\" data-nag=\"16\" title=\"White is clearly better\">O</span>"), "{html}");
    // Glyphs ISChess has no letter for (! and ??) stay as they are.
    assert!(html.contains("<span class=\"nag\" data-nag=\"1\""), "{html}");
    // Without the sign font, ± is written as itself.
    let plain = write(&doc, &options(&pieces));
    assert!(plain.contains(">±</span>") && !plain.contains("nag sign"));
}
