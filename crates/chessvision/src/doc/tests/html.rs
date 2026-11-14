//! The HTML writer, checked by reading its output back (the idea of Caïssa's `fidelity.py`):
//! as many diagrams with the same FENs, every move with its SAN and a figurine where it moves a
//! piece, and nothing a browser would run.

use pdfcraft_chess::draw::{BoardStyle, PieceSet};

use super::*;
use crate::doc::html::{DiagramMode, Figurines, HtmlOptions, REVIEW_LINE, accessibility_summary, write};

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
        lang: "",
        page_labels: &[],
        review_marks: true,
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
    // The board, and before it each of its pieces drawn once, placed on it with `<use>` (plan A8
    // P9b).
    assert_eq!(html.matches("<svg").count(), 2);
    assert_eq!(html.matches("<symbol ").count(), 12);
    assert_eq!(html.matches("<use href=\"#pc-").count(), 32);
    assert!(html.find("<symbol ").unwrap() < html.find("<figure").unwrap());
    // The board is said in words, its name and description joined to it (plan A8 P9b).
    assert!(html.contains(r#"role="img" aria-labelledby="p1-d1-t p1-d1-d"><title id="p1-d1-t">Diagram 1, page 1: White to move</title><desc id="p1-d1-d">White: Ke1, Qd1"#), "{html}");
    assert!(html.contains("<figcaption>Page 1 · diagram 1 · <a href=\"https://lichess.org/analysis/"), "the PGN's comment, then the link: {html}");
    assert!(html.starts_with("<!DOCTYPE html>\n<html>\n"), "no language said, none written");
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
    assert!(html.contains("♛</span>h5") && html.contains(">1.e4<"), "{html}");
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
        role: BlockRole::Content,
    });
    let pieces = PieceSet::geometric();
    let html = write(&doc, &options(&pieces));
    assert!(html.contains("<figure class=\"diagram unread review\" id=\"p1-d1\"><figcaption>Diagram 1, page 1 (not read)</figcaption>"), "{html}");
    assert!(!html.split("<figure").nth(1).unwrap_or_default().split("</figure>").next().unwrap_or_default().contains("data-fen"));
    assert!(html.contains(">click<") && !html.contains("javascript:"), "{html}");
    // Unchecked moves are marked so.
    assert!(html.contains("data-status=\"unchecked\""));
}

/// Plan A8 P9b: in the book's language, with the number printed on the page, and the page's
/// language on `<html>`; ids stay unique when a page has several diagrams.
#[test]
fn diagrams_are_said_in_the_books_language_with_the_printed_page() {
    let rook = "8/8/4k3/8/8/4K3/r7/7R w - - 0 1";
    let text = page_text(vec![line(380.0, "3.Qh5 Nf6 4.Qxf7#"), line(410.0, "1.Rh6+ Kd5 2.Rh5+")]);
    let mut unread = reading(2, 520.0, SCHOLAR, true, false);
    unread.outcome = Err(crate::board::Unreadable::NoModel);
    let mut doc = build_page(&input(0, &text, &[reading(0, 60.0, SCHOLAR, true, false), reading(1, 220.0, rook, true, false), unread], &[], &[]));
    doc.pages[0].lang = "pt".into();
    let pieces = PieceSet::geometric();
    let labels = vec!["xvii".to_owned()];
    let html = write(&doc, &HtmlOptions { lang: "pt-BR", page_labels: &labels, links: false, ..options(&pieces) });
    assert!(html.starts_with("<!DOCTYPE html>\n<html lang=\"pt-BR\">\n"), "{html}");
    assert!(html.contains(r#"<title id="p1-d1-t">Diagrama 1, pág. xvii: brancas jogam</title><desc id="p1-d1-d">Brancas: Re1, Dd1"#), "{html}");
    assert!(
        html.contains(
            r#"<title id="p1-d2-t">Diagrama 2, pág. xvii: brancas jogam</title><desc id="p1-d2-d">Brancas: Re3, Th1. Pretas: Re6, Ta2.</desc>"#
        ),
        "{html}"
    );
    assert!(html.contains("<figcaption>Pág. xvii · diagrama 2 de 3</figcaption>"), "{html}");
    assert!(html.contains("<figcaption>Diagrama 3, pág. xvii (não lido)</figcaption>"), "{html}");
    // Each name and description is found by the id the image points at, once.
    for id in ["p1-d1-t", "p1-d1-d", "p1-d2-t", "p1-d2-d"] {
        assert_eq!(html.matches(&format!("id=\"{id}\"")).count(), 1, "{id}");
    }
    // A language tag that is not one is not written as one.
    let html = write(&doc, &HtmlOptions { lang: "\"><script>", ..options(&pieces) });
    assert!(html.starts_with("<!DOCTYPE html>\n<html lang=\"script\">\n") && !html.contains("<script>"), "{html}");
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
    // Zugzwang is its ringed dot (`[`), and a bishop sign, which no Unicode character has, its letter.
    para.push(Inline::Nag { code: 23, source_rect: None, review: Vec::new() });
    para.push(Inline::Nag { code: 247, source_rect: None, review: Vec::new() });
    let ischess = role_font(FontRole::Signs);
    let html = write(&doc, &HtmlOptions { sign_font: Some(&ischess), ..options(&pieces) });
    assert!(html.contains("@font-face{font-family:\"PdfCraft Signs\""), "{html}");
    assert!(html.contains(".sign{font-family:\"PdfCraft Signs\"}"));
    // Each said by its name, not by its letter (plan A9-X3).
    for (code, name, letter) in [(16, "White is clearly better", 'O'), (23, "Zugzwang", '['), (247, "Bishops of opposite colors", 'n')] {
        let said =
            format!("<span class=\"nag sign\" data-nag=\"{code}\" role=\"img\" aria-label=\"{name}\" lang=\"en\" title=\"{name}\">{letter}</span>");
        assert!(html.contains(&said), "{said}\n{html}");
    }
    // Glyphs ISChess has no letter for (! and ??) stay as they are.
    assert!(html.contains("<span class=\"nag\" data-nag=\"1\""), "{html}");
    // Without the sign font, ± is written as itself.
    let plain = write(&doc, &options(&pieces));
    assert!(plain.contains(">±</span>") && !plain.contains("nag sign"));
}

/// Plan A8 §2.4: the HTML of a book with 548 diagrams stays within 15 MB with every piece set
/// PdfCraft ships, each piece drawn once (`<symbol>`/`<use>`) instead of on every board. Before
/// that, Fantasy reached 82 MB. The sizes are printed to record them.
#[test]
fn five_hundred_and_forty_eight_diagrams_stay_within_fifteen_megabytes() {
    let page = showcase_doc().pages.remove(0);
    let doc = ChessDoc { pages: (0..548).map(|i| ChessPage { index: i, ..page.clone() }).collect(), ..showcase_doc() };
    let mut sizes = vec![("geometric", write(&doc, &options(&PieceSet::geometric())).len())];
    for set in crate::pieces::bundled::SETS {
        let pieces = set.load().unwrap();
        sizes.push((set.id, write(&doc, &options(&pieces)).len()));
    }
    eprintln!("{sizes:?}");
    for (id, len) in sizes {
        assert!(len <= 15_000_000, "{id}: {len} bytes");
    }
}

/// Plan A9-L6a (A2-08, A2's probe3): a book in Portuguese whose moves are figurines (so its piece
/// letters say nothing, read as English) is described in Portuguese: the page's text language is
/// its prose's, apart from its piece letters', and a language the book gives wins over both.
#[test]
fn a_diagram_is_described_in_the_language_of_the_books_text() {
    let text = page_text(vec![
        line(20.0, "As brancas não podem jogar o lance com a dama, mas a torre é forte para o ataque."),
        line(230.0, "3.Qh5 Nf6 4.Qxf7#"),
    ]);
    let doc = build_page(&input(0, &text, &[reading(0, 60.0, SCHOLAR, true, false)], &[], &[]));
    let page = &doc.pages[0];
    assert_eq!((page.lang.as_str(), page.text_lang.as_str()), ("en", "pt"));
    let pieces = PieceSet::geometric();
    let html = write(&doc, &options(&pieces));
    assert!(html.contains(">Diagrama 1, pág. 1: brancas jogam</title>"), "{html}");
    // The book says its language: every page's.
    let mut b = DocBuilder::new();
    b.text_lang(LangTag::parse("de-DE"));
    b.add_page(&input(0, &text, &[reading(0, 60.0, SCHOLAR, true, false)], &[], &[]));
    let doc = b.finish();
    assert_eq!(doc.pages[0].text_lang.as_str(), "de-DE");
    assert!(write(&doc, &options(&pieces)).contains(">Diagramm 1, s. 1: Weiß am Zug</title>"));
    // A tag that is none is not said, and an earlier document's JSON (no `text_lang`) reads as
    // its piece letters.
    assert!(LangTag::parse("pt BR").is_empty() && LangTag::parse("").is_empty() && LangTag::parse(&"a".repeat(40)).is_empty());
    assert_eq!(LangTag::parse(" pt_BR ").as_str(), "pt-BR");
    let json = doc.to_json().unwrap().replace(",\"text_lang\":\"de-DE\"", "");
    let old = ChessDoc::from_json(&json).unwrap();
    assert_eq!(old.pages[0].text_language(), Lang::En);
}

/// Plan A9-L6b (A1-14, R5-05): a diagram is described with what is known of its side to move.
/// A "position after" caption settles it (`Position after: 5.dxc5`: Black to move), and the
/// description, the FEN and the lichess link say so; a side only assumed is said not given, never
/// `White to move`.
#[test]
fn a_diagram_says_the_side_the_text_settled_and_not_one_assumed() {
    let pieces = PieceSet::geometric();
    // Read with White to move, assumed; the caption names the move that led to it.
    let read = "rnbqkbnr/pp3ppp/8/2Pp4/8/2N5/PP2PPPP/R1BQKBNR w KQkq - 0 1";
    let text = page_text(vec![line(230.0, "Position after: 5.dxc5"), line(250.0, "5...Bxc5 6.Nxd5")]);
    let doc = build_page(&input(0, &text, &[reading(0, 60.0, read, true, true)], &[], &[]));
    let Block::Diagram(d) = &doc.pages[0].blocks[0] else { panic!("a diagram first") };
    assert_eq!(d.reading.as_ref().map(|r| r.fen.as_str()), Some(read), "the reading is what was printed");
    let settled = d.settled.as_ref().expect("the caption settles the side");
    assert_eq!((settled.fen.as_str(), settled.side), ("rnbqkbnr/pp3ppp/8/2Pp4/8/2N5/PP2PPPP/R1BQKBNR b KQkq - 0 5", crate::doc::SideSource::Caption));
    let html = write(&doc, &options(&pieces));
    assert!(html.contains(">Diagram 1, page 1: Black to move</title>"), "{html}");
    assert!(html.contains("data-fen=\"rnbqkbnr/pp3ppp/8/2Pp4/8/2N5/PP2PPPP/R1BQKBNR b KQkq - 0 5\""), "{html}");
    assert!(d.link().is_some_and(|u| u.contains("%20b%20KQkq")), "{:?}", d.link());
    let mut user = d.clone();
    user.source = crate::doc::DiagramSource::User;
    assert_eq!(user.shown().map(|s| s.fen), Some(read), "a position a person set is theirs");
    // The first move's number agrees with the side assumed: the side is known, the position the
    // one read.
    let text = page_text(vec![line(230.0, "1.Nxd5 Qxd5")]);
    let doc = build_page(&input(0, &text, &[reading(0, 60.0, read, true, true)], &[], &[]));
    let Block::Diagram(d) = &doc.pages[0].blocks[0] else { panic!("a diagram first") };
    assert_eq!(d.settled.as_ref().map(|s| (s.fen.as_str(), s.side)), Some((read, crate::doc::SideSource::MoveNumber)));
    assert!(write(&doc, &options(&pieces)).contains(">Diagram 1, page 1: White to move</title>"));
    // Nothing settles it: the side is not given.
    let text = page_text(vec![line(230.0, "A quiet position.")]);
    let doc = build_page(&input(0, &text, &[reading(0, 60.0, read, true, true)], &[], &[]));
    let html = write(&doc, &options(&pieces));
    assert!(html.contains(">Diagram 1, page 1: side to move not given</title>"), "{html}");
    assert!(!html.contains("White to move"), "{html}");
}

/// Plan A9-L1b (A2-04, A2-06): the page's furniture is not the book's text. Chess Evolution's
/// `Issue 47` atop every page of an issue was ten entries of its EPUB's contents and ten `##` of
/// its Markdown, each page's number a paragraph of its own (`<p>15</p>`), and a diagram in a font
/// the catalogue does not know a paragraph of glyphs. The HTML, its contents and the Markdown keep
/// the head once, where the issue starts, and write neither the numbers nor the glyphs; the
/// document keeps them all, with what each is.
#[test]
fn the_pages_furniture_is_not_exported() {
    let doc = issue_pages();
    assert_eq!(kinds(&doc.pages[0]), ["H:Issue 47", "D0", "P:4*+*+p+*+5", "P:3.Qh5 Nf6", "P:15"]);
    assert_eq!(kinds(&doc.pages[1]), ["H:Issue 47", "P:4.Qxf7#", "P:16"]);
    assert_eq!((doc.pages[0].running_heads.as_slice(), doc.pages[1].running_heads.as_slice()), (&[][..], &[0][..]));
    assert!(doc.pages.iter().all(|p| moves(p).iter().all(|(_, s)| *s == MoveStatus::Valid)));
    let pieces = PieceSet::geometric();
    let html = write(&doc, &options(&pieces));
    assert_eq!(html.matches("Issue 47").count(), 1, "{html}");
    for furniture in ["<p>15</p>", "<p>16</p>", "4*+"] {
        assert!(!html.contains(furniture), "{furniture}: {html}");
    }
    let toc = crate::doc::html::toc(&doc);
    assert_eq!(toc.iter().map(|t| t.text.as_str()).collect::<Vec<_>>(), ["Issue 47"]);
    assert!(html.contains(&format!("<h1 id=\"{}\">Issue 47</h1>", toc[0].id)), "the contents link to the heading written");
    let md = crate::doc::markdown::write(
        &doc,
        &crate::doc::markdown::MarkdownOptions {
            title: "Magazine",
            figurines: Figurines::Letters(pdfcraft_chess::san::Lang::En),
            diagrams: crate::doc::markdown::MarkdownDiagrams::Fen,
            links: false,
            link_text: "",
            image_dir: "",
            lang: "",
            page_labels: &[],
        },
    );
    assert_eq!(md.matches("Issue 47").count(), 1, "{md}");
    assert!(md.contains("3.Qh5 Nf6") && md.contains("4.Qxf7#"), "{md}");
    for furniture in ["\n15\n", "\n16\n", r"4\*+"] {
        assert!(!md.contains(furniture), "{furniture:?}: {md}");
    }
    // The document keeps what it read, and says what each block is.
    let json = doc.to_json().unwrap();
    assert!(json.contains("\"running_heads\":[0]") && json.contains("\"role\":\"folio\""), "{json}");
    assert_eq!(walk(&ChessDoc::from_json(&json).unwrap().pages, WalkRules::BOOK), doc, "walked again, the same");
}

/// Plan A9-L1b (§2.6): a book whose PDF says no page labels is paged by the numbers it prints,
/// when they agree with the pages' order; a page between two of them that prints none (a
/// chapter's first page) takes the number its place gives, and a stray number is not one.
#[test]
fn a_book_without_page_labels_is_paged_by_its_printed_numbers() {
    let doc = issue_pages();
    let own: Vec<String> = vec!["1".into(), "2".into()];
    assert_eq!(PageLabel::with_folios(&own, &doc), ["15", "16"]);
    // The document's own labels win.
    let labelled: Vec<String> = vec!["i".into(), "ii".into()];
    assert_eq!(PageLabel::with_folios(&labelled, &doc), labelled);
    // Pages that print 15, nothing, 17, a stray 99, nothing: the numbers follow the pages from 15,
    // the page between takes 16, and the pages past the last that follows keep their own.
    let numbered = |folios: &[Option<&str>]| {
        let mut b = DocBuilder::new();
        for (i, folio) in folios.iter().enumerate() {
            let mut lines = vec![line(100.0, "Some prose.")];
            lines.extend(folio.map(|f| line(760.0, f)));
            b.add_page(&input(i as u32, &page_text(lines), &[], &[], &[]));
        }
        b.finish()
    };
    let own = |n: usize| (1..=n).map(|i| i.to_string()).collect::<Vec<String>>();
    let doc = numbered(&[Some("15"), None, Some("17"), Some("99"), None]);
    let folios: Vec<Option<String>> = doc.pages.iter().map(ChessPage::folio).collect();
    assert_eq!(folios, [Some("15".into()), None, Some("17".into()), Some("99".into()), None]);
    assert_eq!(PageLabel::with_folios(&own(5), &doc), ["15", "16", "17", "4", "5"]);
    // Two numbers that disagree, as many of each: neither is taken.
    let doc = numbered(&[Some("15"), None, Some("99")]);
    assert_eq!(PageLabel::with_folios(&own(3), &doc), own(3));
}

/// Plan A9-X1a (A2-17): U+FFFE and U+FFFF, which XML does not allow, and the other noncharacters
/// leave the HTML and the EPUB's pages well-formed: they are dropped, the text around them kept.
#[test]
fn noncharacters_are_dropped_from_the_markup() {
    assert_eq!(crate::doc::html::escape("a\u{FFFE}b\u{FFFF}c\u{FDD0}d\u{1FFFF}e<"), "abcde&lt;");
    assert_eq!(
        crate::doc::html::escape("\u{FFFD}\u{FDCF}\u{FDF0}"),
        "\u{FFFD}\u{FDCF}\u{FDF0}",
        "a replacement character and their neighbours are characters"
    );
}

/// Plan A9-X3 (R5-06): an annotation glyph is said by its name to a screen reader, which reads no
/// `title` and reads `±` as "plus minus"; the names are English, and the span says so in a book
/// in another language or in none given.
#[test]
fn an_annotation_glyph_is_said_by_its_name() {
    let text = page_text(vec![line(230.0, "1.e4 e5 2.Nf3 Nc6")]);
    let mut doc = build_page(&input(0, &text, &[], &[], &[]));
    let para = doc.pages[0].blocks.iter_mut().find_map(|b| if let Block::Paragraph { inlines, .. } = b { Some(inlines) } else { None }).unwrap();
    para.insert(3, Inline::Nag { code: 16, source_rect: None, review: Vec::new() });
    let pieces = PieceSet::geometric();
    let name = pdfcraft_chess::nag::by_code(16).map(|n| n.name).unwrap();
    let html = write(&doc, &HtmlOptions { lang: "pt-BR", ..options(&pieces) });
    assert!(
        html.contains(&format!("<span class=\"nag\" data-nag=\"16\" role=\"img\" aria-label=\"{name}\" lang=\"en\" title=\"{name}\">±</span>")),
        "{html}"
    );
    let html = write(&doc, &HtmlOptions { lang: "en-GB", ..options(&pieces) });
    assert!(html.contains(&format!("<span class=\"nag\" data-nag=\"16\" role=\"img\" aria-label=\"{name}\" title=\"{name}\">±</span>")), "{html}");
}

/// Plan A9-X3 (R5-06): a diagram written as its FEN has the position in words beside it, as the
/// board's own description says it.
#[test]
fn a_diagram_as_fen_has_the_position_in_words() {
    let text = page_text(vec![line(230.0, "3.Qh5 Nf6 4.Qxf7#")]);
    let doc = build_page(&input(0, &text, &[reading(0, 60.0, SCHOLAR, true, false)], &[], &[]));
    let pieces = PieceSet::geometric();
    let html = write(&doc, &HtmlOptions { diagrams: DiagramMode::Fen, ..options(&pieces) });
    let fen = attrs(&html, "data-fen").into_iter().next().unwrap();
    let words = pdfcraft_chess::describe::describe(1, "1", &fen, Lang::En);
    assert!(words.contains(": White to move. White: K"), "{words}");
    assert!(html.contains(&format!("<code>{fen}</code><p class=\"position\">{words}</p>")), "{html}");
}

/// The relative luminance of `#rrggbb` (WCAG 2).
fn luminance(hex: &str) -> f64 {
    let channel = |i: usize| {
        let c = f64::from(u8::from_str_radix(&hex[i..i + 2], 16).unwrap()) / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * channel(1) + 0.7152 * channel(3) + 0.0722 * channel(5)
}

/// Plan A9-X3 (R5-06): what was flagged for review is underlined as well as tinted, so it is not
/// told by colour alone: the line is at least 3:1 against the page and against the tint (the tint
/// alone is 1.1:1 against white).
#[test]
fn a_review_mark_is_not_told_by_colour_alone() {
    let pieces = PieceSet::geometric();
    let html = write(&showcase_doc(), &options(&pieces));
    assert!(html.contains(&format!(".review{{background:#fff3c4;border-bottom:2px solid {REVIEW_LINE}}}")), "{html}");
    let contrast = |a: &str, b: &str| {
        let (la, lb) = (luminance(a), luminance(b));
        (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
    };
    assert!(contrast("#fff3c4", "#ffffff") < 1.2);
    for page in ["#ffffff", "#fff3c4"] {
        assert!(contrast(REVIEW_LINE, page) >= 3.0, "{REVIEW_LINE} on {page}: {}", contrast(REVIEW_LINE, page));
    }
    // A book for readers has neither.
    let html = write(&showcase_doc(), &HtmlOptions { review_marks: false, ..options(&pieces) });
    assert!(!html.contains(".review{"), "{html}");
}

/// Plan A9-X3 (R5-06): what a book says of how it can be read is counted from it: how many
/// diagrams are said in words and name the side to move, and whether the page numbers are the
/// book's own (it said both of every book).
#[test]
fn the_accessibility_summary_is_counted_from_the_book() {
    let rook = "8/8/4k3/8/8/4K3/r7/7R w - - 0 1";
    let text = page_text(vec![line(380.0, "A quiet position.")]);
    let mut unread = reading(2, 520.0, SCHOLAR, true, false);
    unread.outcome = Err(crate::board::Unreadable::NoModel);
    let doc = build_page(&input(0, &text, &[reading(0, 60.0, SCHOLAR, true, false), reading(1, 220.0, rook, true, true), unread], &[], &[]));
    let pieces = PieceSet::geometric();
    assert_eq!(
        accessibility_summary(&doc, &options(&pieces)),
        "The moves are text. Chess diagrams: 3. With a text alternative that lists each piece on its square: 2 (the other 1 could not \
         be read, and say so). Naming the side to move: 1 (the book does not give it for the other 1). Page numbers count the pages of \
         the PDF."
    );
    let labels = vec!["17".to_owned()];
    assert!(
        accessibility_summary(&doc, &HtmlOptions { page_labels: &labels, ..options(&pieces) })
            .ends_with(". Page numbers are the ones the book prints.")
    );
    // A PDF without page labels gives each page its own number from 1: not one the book prints.
    let own = vec!["1".to_owned()];
    assert!(
        accessibility_summary(&doc, &HtmlOptions { page_labels: &own, ..options(&pieces) }).ends_with(". Page numbers count the pages of the PDF.")
    );
    let none = build_page(&input(0, &page_text(vec![line(230.0, "1.e4 e5")]), &[], &[], &[]));
    assert!(accessibility_summary(&none, &options(&pieces)).starts_with("The moves are text. No chess diagrams. Page numbers"));
}

/// A page reading `1.e4 e5 2.Nf3 Nc6` with a sign (`±`, $16) after `2.Nf3`, its prose in `lang`.
fn page_with_a_sign(lang: &str) -> ChessDoc {
    let text = page_text(vec![line(230.0, "1.e4 e5 2.Nf3 Nc6")]);
    let mut doc = build_page(&input(0, &text, &[], &[], &[]));
    let para = doc.pages[0].blocks.iter_mut().find_map(|b| if let Block::Paragraph { inlines, .. } = b { Some(inlines) } else { None }).unwrap();
    para.insert(3, Inline::Nag { code: 16, source_rect: None, review: Vec::new() });
    doc.pages[0].text_lang = LangTag::parse(lang);
    doc
}

/// Plan A9-X3b (R5-06): a sign and a figurine are said in the book's language where the
/// interface names them (`pdfcraft_chess::spoken`), each figurine by the side that moved (both
/// sides' are drawn solid: `♞f3` was "black chess knight" on a White move); a language no
/// catalogue has says them in English, and says so.
#[test]
fn signs_and_figurines_are_said_in_the_books_language() {
    use pdfcraft_chess::placement::{Color, PieceKind};
    let pieces = PieceSet::geometric();
    let html = write(&page_with_a_sign("pt"), &HtmlOptions { lang: "pt-BR", ..options(&pieces) });
    let sign = "As brancas estão claramente melhor";
    assert!(html.contains(&format!("<span class=\"nag\" data-nag=\"16\" role=\"img\" aria-label=\"{sign}\" title=\"{sign}\">±</span>")), "{html}");
    assert!(html.contains("<span role=\"img\" aria-label=\"Cavalo branco\">♞</span>f3"), "{html}");
    let black = pdfcraft_chess::spoken::piece(Color::Black, PieceKind::Knight, Lang::Pt).text;
    assert!(html.contains(&format!("<span role=\"img\" aria-label=\"{black}\">♞</span>c6")), "{html}");
    // Dutch prose is recognised, but no catalogue has it.
    let html = write(&page_with_a_sign("nl"), &HtmlOptions { lang: "nl", ..options(&pieces) });
    assert!(html.contains("aria-label=\"White is clearly better\" lang=\"en\" title="), "{html}");
    assert!(html.contains("<span role=\"img\" aria-label=\"White knight\" lang=\"en\">♞</span>f3"), "{html}");
}

/// Plan A9-X3b (R5-06): with the review highlight on, what was flagged is said to a screen reader
/// as well as shown, with the reason the Text tab gives a move, in the book's language; a book for
/// readers says none of it.
#[test]
fn a_review_mark_is_said_with_its_reason() {
    let mut doc = page_with_a_sign("pt");
    let para = doc.pages[0].blocks.iter_mut().find_map(|b| if let Block::Paragraph { inlines, .. } = b { Some(inlines) } else { None }).unwrap();
    let Some(Inline::Move(m)) = para.last_mut() else { panic!("Nc6 last") };
    m.status = MoveStatus::Illegal;
    m.review.push(ReviewFlag::IllegalMove { text: "Nc6".into(), near: Vec::new() });
    let pieces = PieceSet::geometric();
    let html = write(&doc, &HtmlOptions { lang: "pt-BR", ..options(&pieces) });
    assert!(html.contains("c6</span><span class=\"sr\"> (A revisar: Lance ilegal)</span>"), "{html}");
    assert!(html.contains(".sr{position:absolute;"), "{html}");
    let html = write(&doc, &HtmlOptions { lang: "pt-BR", review_marks: false, ..options(&pieces) });
    assert!(!html.contains("class=\"sr\""), "{html}");
}

/// Plan A9-N2 (A8-03): the book's variation lost its brackets, so the reader played 3...Nf6 in the
/// main line and could not play White's a6 after it. Linked to the book's PGN, the HTML gives that
/// move the PGN's position as its `data-fen`, marked as the PGN's (`linked`,
/// `data-reference`), its printed text and the reader's status kept. `4.Ba4`, which the reader
/// could play (from the wrong position), keeps the reader's position.
#[test]
fn a_move_the_reader_could_not_play_has_the_books_pgn_position() {
    use crate::doc::reference::{ReferenceIndex, link};
    let pieces = PieceSet::geometric();
    let text = page_text(vec![line(230.0, "1.e4 e5 2.Nf3 Nc6 3.Bb5 Nf6 a6 4.Ba4")]);
    let mut doc = build_page(&input(0, &text, &[], &[], &[]));
    let status: Vec<MoveStatus> = moves(&doc.pages[0]).into_iter().map(|(_, s)| s).collect();
    assert_eq!(status[6..], [MoveStatus::Illegal, MoveStatus::Valid], "{status:?}");
    let index = ReferenceIndex::from_pgn("1.e4 e5 2.Nf3 Nc6 3.Bb5 a6 (3...Nf6 4.O-O) 4.Ba4 *").unwrap();
    link(&mut doc, &index);
    let after = |moves: &[&str]| {
        let mut game = pdfcraft_chess::game::Game::standard();
        for m in moves {
            game.play_san(m).unwrap();
        }
        game.fen()
    };
    let html = write(&doc, &options(&pieces));
    let a6 = after(&["e4", "e5", "Nf3", "Nc6", "Bb5", "a6"]);
    let expected = format!(
        "<span class=\"move linked review\" data-san=\"a6\" data-fen=\"{a6}\" data-reference=\"recovered\" data-reference-san=\"a6\" data-status=\"illegal\">a6</span>"
    );
    assert!(html.contains(&expected), "{html}");
    assert_eq!(html.matches("data-reference=").count(), 1, "only the move the reader could not play: {html}");
    // Without the PGN the move has no position, as before.
    let plain = write(&build_page(&input(0, &text, &[], &[], &[])), &options(&pieces));
    assert_eq!(attrs(&plain, "data-fen").len(), 7, "{plain}");
    assert!(!plain.contains("data-reference"));
}
