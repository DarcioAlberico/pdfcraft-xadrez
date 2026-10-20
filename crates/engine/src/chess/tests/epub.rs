//! Chess ▸ Export ▸ EPUB on the chapter page of `document`: the package's structure (OCF
//! container, package document, navigation, one content document per page), every XML file
//! well-formed, every reference resolving, and the archive's first entry as OCF requires.

use pdfcraft_chess::draw::{BoardStyle, PieceSet};
use pdfcraft_chessvision::doc::html::{DiagramMode, Figurines, HtmlOptions};
use pdfcraft_chessvision::doc::{ChessDoc, ChessPage, TextLayer};

use super::*;
use crate::chess::epub::{EpubEntry, EpubMeta, MAX_PAGES, entries, epub, identifier, modified, modified_at};

fn options(pieces: &PieceSet) -> HtmlOptions<'_> {
    HtmlOptions {
        title: "Chess & <Book>",
        pieces,
        style: BoardStyle::default(),
        diagrams: DiagramMode::Svg,
        figurines: Figurines::Unicode,
        links: true,
        link_text: "Analyze on lichess",
        text_font: None,
    }
}

fn meta() -> EpubMeta {
    EpubMeta { title: "Chess & <Book>".into(), lang: "en".into(), identifier: identifier(b"pdf"), modified: modified("D:20261018120000Z") }
}

fn chapter_doc() -> ChessDoc {
    let (mut s, id) = open(super::document::chapter());
    s.chess_document(id, &[], &no_model(), None).unwrap().doc
}

fn file<'a>(files: &'a [EpubEntry], name: &str) -> &'a str {
    let e = files.iter().find(|e| e.name == name).unwrap_or_else(|| panic!("no {name}"));
    std::str::from_utf8(&e.data).unwrap()
}

#[test]
fn the_package_is_well_formed_and_every_reference_resolves() {
    let pieces = PieceSet::geometric();
    let doc = chapter_doc();
    let files = entries(&doc, &options(&pieces), &meta()).unwrap();
    // mimetype first and stored.
    assert_eq!((files[0].name.as_str(), files[0].data.as_slice(), files[0].compress), ("mimetype", b"application/epub+zip".as_slice(), false));
    // Every XML file parses.
    for e in files.iter().filter(|e| e.name != "mimetype") {
        let text = std::str::from_utf8(&e.data).unwrap();
        if let Err(err) = roxmltree::Document::parse_with_options(text, roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() }) {
            panic!("{} is not well-formed: {err}\n{text}", e.name);
        }
    }
    // The container points at the package document.
    assert!(file(&files, "META-INF/container.xml").contains("full-path=\"OEBPS/content.opf\""));
    let opf_text = file(&files, "OEBPS/content.opf");
    let opf = roxmltree::Document::parse(opf_text).unwrap();
    let items: Vec<(String, String, String)> = opf
        .descendants()
        .filter(|n| n.has_tag_name("item"))
        .map(|n| {
            (
                n.attribute("id").unwrap_or_default().into(),
                n.attribute("href").unwrap_or_default().into(),
                n.attribute("properties").unwrap_or_default().into(),
            )
        })
        .collect();
    for (_, href, _) in &items {
        assert!(files.iter().any(|e| e.name == format!("OEBPS/{href}")), "manifest names a missing file {href}");
    }
    for r in opf.descendants().filter(|n| n.has_tag_name("itemref")) {
        assert!(items.iter().any(|(id, _, _)| Some(id.as_str()) == r.attribute("idref")), "spine names a missing item");
    }
    assert!(items.iter().any(|(_, href, props)| href == "nav.xhtml" && props == "nav"));
    // The page with the board says it holds SVG; the title is escaped; the identifier and date are there.
    assert!(items.iter().any(|(_, href, props)| href == "page-00001.xhtml" && props == "svg"), "{items:?}");
    assert!(opf_text.contains("<dc:title>Chess &amp; &lt;Book&gt;</dc:title>"));
    assert!(opf_text.contains("<meta property=\"dcterms:modified\">2026-10-18T12:00:00Z</meta>"));
    // The navigation links to the heading's anchor, which is in that page.
    let nav = file(&files, "OEBPS/nav.xhtml");
    assert!(nav.contains("<a href=\"page-00001.xhtml#p1-h1\">Chapter 1</a>"), "{nav}");
    let page = file(&files, "OEBPS/page-00001.xhtml");
    assert!(page.contains("id=\"p1-h1\""), "{page}");
    assert!(page.contains(&format!("data-fen=\"{START} w KQkq - 0 1\"")), "{page}");
    assert!(page.contains("<svg") && page.contains("href=\"https://lichess.org/analysis/"));
    assert!(!files.iter().any(|e| String::from_utf8_lossy(&e.data).to_ascii_lowercase().contains("<script")));
}

#[test]
fn the_archive_starts_with_the_stored_mimetype() {
    let pieces = PieceSet::geometric();
    let bytes = epub(&chapter_doc(), &options(&pieces), &meta()).unwrap();
    // Local file header: signature, method 0 (stored) at 8, name length at 26, name at 30.
    assert_eq!(&bytes[..4], b"PK\x03\x04");
    assert_eq!(u16::from_le_bytes([bytes[8], bytes[9]]), 0, "stored");
    assert_eq!(u16::from_le_bytes([bytes[26], bytes[27]]), 8);
    assert_eq!(&bytes[30..38], b"mimetype");
    assert_eq!(&bytes[38..58], b"application/epub+zip", "no extra field before the content");
}

#[test]
fn a_book_without_headings_lists_its_pages_and_limits_hold() {
    let empty = ChessPage { index: 4, lang: "en".into(), text_layer: TextLayer::Missing, blocks: Vec::new(), review: Vec::new() };
    let doc = ChessDoc { format: 1, pages: vec![empty.clone()], games: Vec::new() };
    let pieces = PieceSet::geometric();
    let files = entries(&doc, &options(&pieces), &EpubMeta { lang: String::new(), ..meta() }).unwrap();
    let nav = file(&files, "OEBPS/nav.xhtml");
    assert!(nav.contains("<a href=\"page-00001.xhtml\">Page 5</a>"), "{nav}");
    assert!(file(&files, "OEBPS/content.opf").contains("<dc:language>en</dc:language>"), "an empty language tag falls back to en");
    // No pages at all: the navigation still has an entry.
    let none = entries(&ChessDoc { pages: Vec::new(), ..doc.clone() }, &options(&pieces), &meta()).unwrap();
    assert!(file(&none, "OEBPS/nav.xhtml").contains("<li>"));
    let huge = ChessDoc { pages: vec![empty; MAX_PAGES + 1], ..doc };
    assert!(entries(&huge, &options(&pieces), &meta()).unwrap_err().contains("at most"));
    // Identifier and date.
    assert_eq!(identifier(b"a"), identifier(b"a"));
    assert_ne!(identifier(b"a"), identifier(b"b"));
    assert_eq!(modified("D:2026"), "1970-01-01T00:00:00Z");
    assert_eq!(modified("garbage"), "1970-01-01T00:00:00Z");
    assert_eq!(modified_at(1_700_000_000), "2023-11-14T22:13:20Z");
}

#[test]
fn the_users_figurine_font_travels_in_the_book() {
    let pieces = PieceSet::geometric();
    let doc = chapter_doc();
    let font = pdfcraft_chessvision::text_font::from_bytes(b"wOF2 figurine font".to_vec()).unwrap();
    let files = entries(&doc, &HtmlOptions { text_font: Some(&font), ..options(&pieces) }, &meta()).unwrap();
    // The file, its manifest item and every page's face point at the same place.
    let stored = files.iter().find(|e| e.name == "OEBPS/fonts/figurines.woff2").expect("the font file");
    assert_eq!(stored.data, font.bytes);
    assert!(file(&files, "OEBPS/content.opf").contains("<item id=\"font\" href=\"fonts/figurines.woff2\" media-type=\"font/woff2\"/>"));
    let pages: Vec<&EpubEntry> = files.iter().filter(|e| e.name.starts_with("OEBPS/page-")).collect();
    assert!(!pages.is_empty());
    for p in pages {
        let text = std::str::from_utf8(&p.data).unwrap();
        assert!(text.contains("src:url(\"fonts/figurines.woff2\") format(\"woff2\")"), "{}", p.name);
        // Still well-formed XML.
        roxmltree::Document::parse_with_options(text, roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() }).unwrap();
    }
    // Without a font the book has no font file.
    let plain = entries(&doc, &options(&pieces), &meta()).unwrap();
    assert!(!plain.iter().any(|e| e.name.contains("fonts/")));
}
