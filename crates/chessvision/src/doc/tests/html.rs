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
    assert!(html.contains("<span class=\"nag sign\" data-nag=\"16\" title=\"White is clearly better\">O</span>"), "{html}");
    assert!(html.contains("<span class=\"nag sign\" data-nag=\"23\" title=\"Zugzwang\">[</span>"), "{html}");
    assert!(html.contains("<span class=\"nag sign\" data-nag=\"247\" title=\"Bishops of opposite colors\">n</span>"), "{html}");
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
