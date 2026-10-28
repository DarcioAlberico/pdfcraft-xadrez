//! Chess ▸ Export ▸ EPUB on the chapter page of `document`: the package's structure (OCF
//! container, package document, navigation, one content document per page), every XML file
//! well-formed, every reference resolving, the archive's first entry as OCF requires, and the
//! cover drawn from the first page.

use pdfcraft_chess::draw::{BoardStyle, PieceSet};
use pdfcraft_chessvision::doc::html::{DiagramMode, Figurines, HtmlOptions};
use pdfcraft_chessvision::doc::{ChessDoc, ChessPage, TextLayer};

use super::*;
use crate::chess::epub::{
    EpubEntry, EpubMeta, MAX_COVER_PX, MAX_PAGES, cover_png, entries, epub, identifier, identifier_from_id, modified, modified_at,
};
use crate::chess::export::{ChessExportOptions, ChessFormat};

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
        sign_font: None,
        lang: "",
        page_labels: &[],
        review_marks: true,
    }
}

fn meta() -> EpubMeta {
    EpubMeta {
        title: "Chess & <Book>".into(),
        lang: "en".into(),
        identifier: identifier(b"pdf"),
        modified: modified("D:20261018120000Z"),
        cover: None,
        creator: None,
        source: None,
    }
}

/// Every XML file of the package parses (the cover and the stylesheet are not XML).
fn assert_xml_well_formed(files: &[EpubEntry]) {
    for e in files.iter().filter(|e| e.name != "mimetype" && !e.name.ends_with(".png") && !e.name.ends_with(".css")) {
        let text = std::str::from_utf8(&e.data).unwrap();
        if let Err(err) = roxmltree::Document::parse_with_options(text, roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() }) {
            panic!("{} is not well-formed: {err}\n{text}", e.name);
        }
    }
}

/// The (id, href) of each manifest item whose properties include `cover-image`.
fn cover_items(opf: &str) -> Vec<(String, String)> {
    roxmltree::Document::parse(opf)
        .unwrap()
        .descendants()
        .filter(|n| n.has_tag_name("item") && n.attribute("properties").is_some_and(|p| p.split_whitespace().any(|p| p == "cover-image")))
        .map(|n| (n.attribute("id").unwrap_or_default().to_owned(), n.attribute("href").unwrap_or_default().to_owned()))
        .collect()
}

/// A PNG's pixel size.
fn png_size(png: &[u8]) -> (u32, u32) {
    let info = png::Decoder::new(std::io::Cursor::new(png)).read_info().unwrap().info().clone();
    (info.width, info.height)
}

/// One entry of a zip archive (stored or deflated).
fn zip_entry(zip: &[u8], name: &str) -> Vec<u8> {
    let at = |i: usize, n: usize| zip[i..i + n].iter().rev().fold(0usize, |v, b| v << 8 | *b as usize);
    let mut i = 0;
    while zip[i..].starts_with(b"PK\x03\x04") {
        let (method, size, name_len, extra) = (at(i + 8, 2), at(i + 18, 4), at(i + 26, 2), at(i + 28, 2));
        let data = i + 30 + name_len + extra;
        if &zip[i + 30..i + 30 + name_len] == name.as_bytes() {
            let raw = &zip[data..data + size];
            if method == 0 {
                return raw.to_vec();
            }
            let mut out = Vec::new();
            std::io::Read::read_to_end(&mut flate2::read::DeflateDecoder::new(raw), &mut out).unwrap();
            return out;
        }
        i = data + size;
    }
    panic!("{name} missing")
}

#[test]
fn the_cover_is_one_manifest_item_that_resolves_to_the_first_page() {
    let (mut s, id) = open(super::document::chapter());
    let doc = s.chess_document(id, &[], &no_model(), None).unwrap().doc;
    let png = cover_png(s.get(id).unwrap().export_source(), 0).unwrap();
    let (w, h) = png_size(&png);
    assert!(w.max(h) <= MAX_COVER_PX && w.max(h) >= MAX_COVER_PX - 2, "{w}×{h}");
    let pieces = PieceSet::geometric();
    let files = entries(&doc, &options(&pieces), &EpubMeta { cover: Some(png.clone()), ..meta() }).unwrap();
    assert_xml_well_formed(&files);
    let opf = file(&files, "OEBPS/content.opf");
    let covers = cover_items(opf);
    assert_eq!(covers.len(), 1, "{opf}");
    let (cover_id, href) = &covers[0];
    let stored = files.iter().find(|e| e.name == format!("OEBPS/{href}")).expect("the cover file");
    assert_eq!(stored.data, png);
    assert!(!stored.compress, "a PNG is stored");
    // EPUB 2 readers find it by <meta name="cover">.
    let package = roxmltree::Document::parse(opf).unwrap();
    let named: Vec<&str> = package
        .descendants()
        .filter(|n| n.has_tag_name("meta") && n.attribute("name") == Some("cover"))
        .filter_map(|n| n.attribute("content"))
        .collect();
    assert_eq!(named, [cover_id.as_str()]);
    // Without a cover (or an empty one) the book has none.
    for cover in [None, Some(Vec::new())] {
        let plain = entries(&doc, &options(&pieces), &EpubMeta { cover, ..meta() }).unwrap();
        let opf = file(&plain, "OEBPS/content.opf");
        assert!(cover_items(opf).is_empty() && !opf.contains("name=\"cover\""), "{opf}");
        assert!(!plain.iter().any(|e| e.name.ends_with(".png")));
    }
    // A page that does not exist has no cover.
    assert!(cover_png(s.get(id).unwrap().export_source(), 7).unwrap_err().contains("does not exist"));
}

#[test]
fn chess_export_writes_the_book_with_its_cover() {
    let (mut s, id) = open(super::document::chapter());
    let out = s.chess_export(id, ChessFormat::Epub, &ChessExportOptions::new("Chapter")).unwrap();
    assert!(out.notes.is_empty(), "{:?}", out.notes);
    let opf = String::from_utf8(zip_entry(&out.bytes, "OEBPS/content.opf")).unwrap();
    // Without an /ID the identifier is made from the PDF's bytes; the source is its file name.
    let doc = s.get(id).unwrap();
    assert!(doc.doc_keys().id0.is_none());
    assert!(opf.contains(&format!("<dc:identifier id=\"book-id\">{}</dc:identifier>", identifier(&doc.bytes))), "{opf}");
    assert!(opf.contains(&format!("<dc:source>{}</dc:source>", doc.name)), "{opf}");
    // With one, it is the PDF's own /ID, which saving keeps (plan A8 P9b). The trailer comes
    // after the cross-reference table, so no offset moves.
    let pdf = String::from_utf8(super::document::chapter()).unwrap();
    let with_id = pdf.replacen("/Root 1 0 R >>", "/Root 1 0 R /ID [<00112233445566778899aabbccddeeff> <00112233445566778899aabbccddeeff>] >>", 1);
    assert_ne!(with_id, pdf);
    let (mut s2, id2) = open(with_id.into_bytes());
    let out2 = s2.chess_export(id2, ChessFormat::Epub, &ChessExportOptions::new("Chapter")).unwrap();
    let opf2 = String::from_utf8(zip_entry(&out2.bytes, "OEBPS/content.opf")).unwrap();
    assert!(opf2.contains("<dc:identifier id=\"book-id\">urn:pdfcraft:chess:id:00112233445566778899aabbccddeeff</dc:identifier>"), "{opf2}");
    let covers = cover_items(&opf);
    assert_eq!(covers.len(), 1, "{opf}");
    let png = zip_entry(&out.bytes, &format!("OEBPS/{}", covers[0].1));
    let (w, h) = png_size(&png);
    assert!(w.max(h) <= MAX_COVER_PX && w > 0 && h > 0, "{w}×{h}");
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
    // Every XML file parses (the stylesheet is CSS).
    for e in files.iter().filter(|e| e.name != "mimetype" && !e.name.ends_with(".css")) {
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

/// Plan A8 P9b: the package says how the book can be read, who wrote it and what it was made
/// from; every page starts with a page break named by its printed number, listed in the page
/// list; the pages share one stylesheet, which highlights nothing for review unless asked.
#[test]
fn the_book_says_how_it_can_be_read_and_where_each_page_is() {
    let doc = chapter_doc();
    let pieces = PieceSet::geometric();
    let labels: Vec<String> = doc.pages.iter().map(|p| format!("x{}", p.index)).collect();
    let book = EpubMeta { creator: Some("A <Writer>".into()), source: Some("book & co.pdf".into()), ..meta() };
    let files = entries(&doc, &HtmlOptions { page_labels: &labels, review_marks: false, ..options(&pieces) }, &book).unwrap();
    assert_xml_well_formed(&files);
    let opf = file(&files, "OEBPS/content.opf");
    assert!(opf.contains("<dc:creator>A &lt;Writer&gt;</dc:creator>\n<dc:source>book &amp; co.pdf</dc:source>"), "{opf}");
    for line in [
        "<meta property=\"schema:accessMode\">textual</meta>",
        "<meta property=\"schema:accessMode\">visual</meta>",
        "<meta property=\"schema:accessModeSufficient\">textual</meta>",
        "<meta property=\"schema:accessibilityFeature\">alternativeText</meta>",
        "<meta property=\"schema:accessibilityFeature\">pageNavigation</meta>",
        "<meta property=\"schema:accessibilityHazard\">none</meta>",
        "<meta property=\"schema:accessibilitySummary\">",
    ] {
        assert!(opf.contains(line), "{line}\n{opf}");
    }
    assert!(opf.contains("<item id=\"css\" href=\"styles.css\" media-type=\"text/css\"/>"), "{opf}");
    // One stylesheet: the pages link it and carry none of their own.
    let css = file(&files, "OEBPS/styles.css");
    assert!(css.contains("figure.diagram") && !css.contains(".review{"), "no review highlight in a book: {css}");
    let pages: Vec<&EpubEntry> = files.iter().filter(|e| e.name.starts_with("OEBPS/page-")).collect();
    assert_eq!(pages.len(), doc.pages.len());
    for (e, page) in pages.iter().zip(&doc.pages) {
        let text = std::str::from_utf8(&e.data).unwrap();
        assert!(text.contains("<link rel=\"stylesheet\" type=\"text/css\" href=\"styles.css\"/>") && !text.contains("<style>"), "{text}");
        let label = format!("x{}", page.index);
        assert!(
            text.contains(&format!(
                "<span epub:type=\"pagebreak\" role=\"doc-pagebreak\" id=\"pagebreak\" aria-label=\"{label}\" title=\"{label}\"></span>"
            )),
            "{text}"
        );
    }
    // The page list names each page by its printed number and goes to its break.
    let nav = file(&files, "OEBPS/nav.xhtml");
    assert!(nav.contains("<nav epub:type=\"page-list\" id=\"page-list\" hidden=\"hidden\">"), "{nav}");
    for (n, page) in doc.pages.iter().enumerate() {
        assert!(nav.contains(&format!("<li><a href=\"page-{:05}.xhtml#pagebreak\">x{}</a></li>", n + 1, page.index)), "{nav}");
    }
    // Asked for, the highlight is there; nothing is said of an author or a source not given.
    let checked = entries(&doc, &options(&pieces), &meta()).unwrap();
    assert!(file(&checked, "OEBPS/styles.css").contains(".review{background:#fff3c4}"));
    assert!(!file(&checked, "OEBPS/content.opf").contains("dc:creator") && !file(&checked, "OEBPS/content.opf").contains("dc:source"));
    // The identifier from the PDF's /ID.
    assert_eq!(identifier_from_id(&[0xab; 16]), format!("urn:pdfcraft:chess:id:{}", "ab".repeat(16)));
}

/// The package checked by epubcheck, when it is installed (`epubcheck` on the PATH, from
/// https://www.w3.org/publishing/epubcheck/): `cargo test -p pdfcraft-engine --lib
/// epubcheck_finds_no_error -- --ignored`. Ignored by default: CI does not have it.
#[test]
#[ignore = "needs epubcheck on the PATH"]
fn epubcheck_finds_no_error() {
    let pieces = PieceSet::geometric();
    let labels = vec!["i".to_owned(), "ii".to_owned()];
    let book = epub(&chapter_doc(), &HtmlOptions { page_labels: &labels, review_marks: false, ..options(&pieces) }, &meta()).unwrap();
    let path = std::env::temp_dir().join(format!("pdfcraft-epubcheck-{}.epub", std::process::id()));
    std::fs::write(&path, book).unwrap();
    let run = std::process::Command::new("epubcheck").arg(&path).output();
    let _ = std::fs::remove_file(&path);
    let out = run.expect("epubcheck on the PATH");
    assert!(out.status.success(), "{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
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
    let empty =
        ChessPage { index: 4, lang: "en".into(), text_layer: TextLayer::Missing, ocr: false, blocks: Vec::new(), review: Vec::new(), trailing: None };
    let doc = ChessDoc { format: 1, pages: vec![empty.clone()], games: Vec::new() };
    let pieces = PieceSet::geometric();
    let files = entries(&doc, &options(&pieces), &EpubMeta { lang: String::new(), ..meta() }).unwrap();
    let nav = file(&files, "OEBPS/nav.xhtml");
    assert!(nav.contains("<a href=\"page-00001.xhtml\">Page 5</a>"), "{nav}");
    // In the book's language, with the number printed on the page (plan A8 P9b).
    let labels = vec![String::new(); 4].into_iter().chain(["xvii".to_owned()]).collect::<Vec<_>>();
    let pt = entries(&doc, &HtmlOptions { page_labels: &labels, ..options(&pieces) }, &EpubMeta { lang: "pt".into(), ..meta() }).unwrap();
    assert!(file(&pt, "OEBPS/nav.xhtml").contains("<a href=\"page-00001.xhtml\">Pág. xvii</a>"), "{}", file(&pt, "OEBPS/nav.xhtml"));
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
    let font = pdfcraft_chessvision::text_font::from_bytes(b"wOF2 figurine font".to_vec(), "dejavu").unwrap();
    let files = entries(&doc, &HtmlOptions { text_font: Some(&font), ..options(&pieces) }, &meta()).unwrap();
    // The file, its manifest item and every page's face point at the same place.
    let stored = files.iter().find(|e| e.name == "OEBPS/fonts/figurines.woff2").expect("the font file");
    assert_eq!(stored.data, font.bytes);
    assert!(file(&files, "OEBPS/content.opf").contains("<item id=\"font\" href=\"fonts/figurines.woff2\" media-type=\"font/woff2\"/>"));
    let pages: Vec<&EpubEntry> = files.iter().filter(|e| e.name.starts_with("OEBPS/page-")).collect();
    assert!(!pages.is_empty());
    // The pages' one stylesheet loads it, from beside them (plan A8 P9b).
    let css = file(&files, "OEBPS/styles.css");
    assert!(css.contains("src:url(\"fonts/figurines.woff2\") format(\"woff2\")"), "{css}");
    for p in pages {
        let text = std::str::from_utf8(&p.data).unwrap();
        assert!(text.contains("href=\"styles.css\""), "{}", p.name);
        // Still well-formed XML.
        roxmltree::Document::parse_with_options(text, roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() }).unwrap();
    }
    // Without a font the book has no font file.
    let plain = entries(&doc, &options(&pieces), &meta()).unwrap();
    assert!(!plain.iter().any(|e| e.name.contains("fonts/")));
}

#[test]
fn a_figurine_font_by_letters_and_the_sign_font_travel_as_two_files() {
    use pdfcraft_chessvision::text_font::{FontFormat, FontRole, TextFont};
    let font = |role, bytes: &[u8]| TextFont { bytes: bytes.to_vec(), format: FontFormat::TrueType, role, family: String::new(), path: None };
    let (semfig, ischess) = (font(FontRole::Letters, b"semfig"), font(FontRole::Signs, b"ischess"));
    let pieces = PieceSet::geometric();
    let doc = chapter_doc();
    let files = entries(&doc, &HtmlOptions { text_font: Some(&semfig), sign_font: Some(&ischess), ..options(&pieces) }, &meta()).unwrap();
    assert_eq!(files.iter().find(|e| e.name == "OEBPS/fonts/figurines.ttf").map(|e| e.data.as_slice()), Some(b"semfig".as_slice()));
    assert_eq!(files.iter().find(|e| e.name == "OEBPS/fonts/signs.ttf").map(|e| e.data.as_slice()), Some(b"ischess".as_slice()));
    let opf = file(&files, "OEBPS/content.opf");
    assert!(opf.contains("<item id=\"font\" href=\"fonts/figurines.ttf\" media-type=\"font/ttf\"/>"), "{opf}");
    assert!(opf.contains("<item id=\"signs\" href=\"fonts/signs.ttf\" media-type=\"font/ttf\"/>"), "{opf}");
    let css = file(&files, "OEBPS/styles.css");
    assert!(css.contains(".fig{font-family:\"PdfCraft Figurines\"}") && css.contains(".sign{font-family:\"PdfCraft Signs\"}"), "{css}");
    for p in files.iter().filter(|e| e.name.starts_with("OEBPS/page-")) {
        let text = std::str::from_utf8(&p.data).unwrap();
        assert!(text.contains("href=\"styles.css\""), "{}", p.name);
        roxmltree::Document::parse_with_options(text, roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() }).unwrap();
    }
}
