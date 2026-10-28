//! Chess ▸ Export ▸ Word and RTF: the diagram image is the position (checked pixel by pixel),
//! and the chapter is a flowing document (plan A8 P9a): headings, runs whose moves spelling
//! checks skip, each diagram a picture with its text alternative and caption, real links.

use pdfcraft_chess::draw::{BoardStyle, PieceSet};
use pdfcraft_chessvision::doc::html::Figurines;

use super::*;
use crate::chess::office::{DIAGRAM_PT, OfficeOptions, diagram_png, docx, rtf, to_flow};
use pdfcraft_export::flow::{FlowBlock, Run};

fn decode(png: &[u8]) -> (u32, u32, Vec<u8>) {
    let dec = png::Decoder::new(std::io::Cursor::new(png));
    let mut reader = dec.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    buf.truncate(info.buffer_size());
    assert_eq!(info.color_type, png::ColorType::Rgba);
    (info.width, info.height, buf)
}

/// The RGB at the centre of square (file, rank), 0-based from a1, on a board without a frame.
fn square_rgb(px: &[u8], side: u32, file: u32, rank: u32, dx: f32, dy: f32) -> [u8; 3] {
    let cell = side as f32 / 8.0;
    let x = ((file as f32 + dx) * cell) as u32;
    let y = ((7 - rank) as f32 * cell + dy * cell) as u32;
    let i = ((y * side + x) * 4) as usize;
    [px[i], px[i + 1], px[i + 2]]
}

fn plain() -> BoardStyle {
    BoardStyle { border: 0.0, coordinates: false, ..BoardStyle::default() }
}

#[test]
fn the_diagram_image_shows_the_position_square_by_square() {
    let style = plain();
    let png = diagram_png(&parse_placement(START).unwrap(), &PieceSet::geometric(), &style, 400).unwrap();
    let (w, h, px) = decode(&png);
    assert_eq!((w, h), (400, 400));
    let rgb = |c: pdfcraft_chess::draw::Rgb| [c.0, c.1, c.2];
    // Empty squares in the middle show the board's colours: a3 dark, b3 light (a1 is dark).
    assert_eq!(square_rgb(&px, w, 0, 2, 0.5, 0.5), rgb(style.dark));
    assert_eq!(square_rgb(&px, w, 1, 2, 0.5, 0.5), rgb(style.light));
    // A square with a piece does not show its square's colour at the piece's base.
    for (file, rank) in [(4, 0), (0, 1), (3, 7), (7, 6)] {
        let base = square_rgb(&px, w, file, rank, 0.5, 0.8);
        assert_ne!(base, rgb(style.light), "({file}, {rank})");
        assert_ne!(base, rgb(style.dark), "({file}, {rank})");
    }
    // Sizes are clamped; coordinates (text) draw too.
    let small = decode(&diagram_png(&parse_placement(START).unwrap(), &PieceSet::geometric(), &style, 1).unwrap());
    assert_eq!((small.0, small.1), (32, 32));
    let labelled = BoardStyle { coordinates: true, ..BoardStyle::default() };
    assert!(diagram_png(&parse_placement(START).unwrap(), &PieceSet::geometric(), &labelled, 300).is_ok());
}

fn options(pieces: &PieceSet) -> OfficeOptions<'_> {
    OfficeOptions {
        title: "Chess",
        pieces,
        style: BoardStyle::default(),
        figurines: Figurines::Unicode,
        links: true,
        link_text: "Analyze on lichess",
        diagram_px: 240,
        text_font: None,
        sign_font: None,
        lang: "en",
        page_labels: &[],
    }
}

/// A block's text, its runs joined.
fn text_of(runs: &[Run]) -> String {
    runs.iter().map(|r| r.text.as_str()).collect()
}

#[test]
fn the_chapter_is_a_flowing_document_and_both_files_carry_the_diagram() {
    let (mut s, id) = open(super::document::chapter());
    let doc = s.chess_document(id, &[], &no_model(), None).unwrap().doc;
    let pieces = PieceSet::geometric();
    let mut notes = Vec::new();
    let (flow, drawn) = to_flow(&doc, &options(&pieces), &mut notes);
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(drawn, 1);
    assert_eq!((flow.title.as_str(), flow.lang.as_str()), ("Chess", "en"));
    // In the book's order: the heading, the prose, the diagram, the moves.
    let kinds: Vec<String> = flow
        .blocks
        .iter()
        .map(|b| match b {
            FlowBlock::Heading { level, runs } => format!("h{level} {}", text_of(runs)),
            FlowBlock::Para(runs) => format!("p {}", text_of(runs)),
            FlowBlock::Figure(f) => format!("figure {}", text_of(&f.caption)),
        })
        .collect();
    assert_eq!(kinds[0], "h1 Chapter 1", "{kinds:?}");
    let figure = kinds.iter().position(|k| k.starts_with("figure")).unwrap();
    let moves = kinds.iter().position(|k| k == "p 1.e4 e5 2.♞f3 ♞c6 3.♝b5 a6").unwrap_or_else(|| panic!("{kinds:?}"));
    assert!(kinds.iter().position(|k| k.starts_with("p White to play")).unwrap() < figure && figure < moves, "{kinds:?}");
    // The caption: the PGN's comment, the FEN, the link.
    assert!(kinds[figure].starts_with("figure Page 1 · diagram 1 · FEN rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w"), "{}", kinds[figure]);
    assert!(kinds[figure].ends_with(" · Analyze on lichess"), "{}", kinds[figure]);
    let FlowBlock::Figure(f) = &flow.blocks[figure] else { panic!("not a figure") };
    assert_eq!(f.side_pt, DIAGRAM_PT);
    assert!(f.alt.starts_with("Diagram 1, page 1: White to move. White: Ke1, Qd1"), "{}", f.alt);
    // Moves are marked for spelling checks to skip; the prose is not.
    let FlowBlock::Para(runs) = &flow.blocks[moves] else { panic!("not a paragraph") };
    assert!(runs.iter().filter(|r| r.text.trim().len() > 1).all(|r| r.no_proof), "{runs:?}");
    let FlowBlock::Para(prose) = &flow.blocks[1] else { panic!("not a paragraph") };
    assert!(prose.iter().all(|r| !r.no_proof), "{prose:?}");

    let docx = docx(&flow);
    for part in ["word/document.xml", "word/styles.xml", "word/_rels/document.xml.rels", "[Content_Types].xml", "docProps/core.xml"] {
        roxmltree::Document::parse(&zip_part(&docx, part)).unwrap_or_else(|e| panic!("{part}: {e}"));
    }
    assert!(docx.windows(23).any(|w| w == b"word/media/diagram1.png"), "the docx holds the diagram image");
    let xml = zip_part(&docx, "word/document.xml");
    let body = roxmltree::Document::parse(&xml).unwrap();
    let doc_pr = body.descendants().find(|n| n.has_tag_name("docPr")).unwrap();
    assert_eq!(doc_pr.attribute("descr"), Some(f.alt.as_str()), "the picture's text alternative");
    let styles: Vec<&str> = body.descendants().filter(|n| n.has_tag_name("pStyle")).filter_map(|n| n.attribute((W, "val"))).collect();
    assert!(styles.contains(&"Heading1") && styles.contains(&"Figure") && styles.contains(&"Caption"), "{styles:?}");
    assert!(xml.contains("<w:noProof/>"), "{xml:.600}");
    let styles_xml = zip_part(&docx, "word/styles.xml");
    assert!(styles_xml.contains("<w:docDefaults><w:rPrDefault><w:rPr><w:lang w:val=\"en\"/>"), "{styles_xml}");
    assert!(
        styles_xml
            .contains("w:styleId=\"Figure\"><w:name w:val=\"Figure\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Caption\"/><w:pPr><w:keepNext/>"),
        "the picture stays with its caption: {styles_xml}"
    );
    assert!(styles_xml.contains("w:styleId=\"Caption\"><w:name w:val=\"caption\"/>"), "{styles_xml}");

    let rtf = rtf(&flow);
    assert!(rtf.contains("\\pngblip") && rtf.contains("{\\sn wzDescription}{\\sv Diagram 1, page 1: White to move."), "{rtf:.600}");
    assert!(rtf.contains("\\outlinelevel0") && rtf.contains("Chapter 1"), "{rtf:.400}");
    assert!(rtf.contains("\\u9822?f3") && rtf.contains("\\noproof"), "the knight figurine as \\uN?, moves not proofed");
}

#[test]
fn an_unread_diagram_is_said_in_words() {
    let (mut s, id) = open(pdf(&vector_board(60.0, 400.0), "Helvetica", None, ""));
    let doc = s.chess_document(id, &[], &no_model(), None).unwrap().doc;
    let pieces = PieceSet::geometric();
    let mut notes = Vec::new();
    let (flow, drawn) = to_flow(&doc, &options(&pieces), &mut notes);
    assert_eq!(drawn, 0);
    assert_eq!(flow.blocks, [FlowBlock::Para(vec![Run::text("Diagram 1, page 1 (not read)")])]);
}

/// Plan A8 P9a: piece letters and signs in the user's fonts, named on their runs; the RTF's font
/// table lists them, and the reply says they must be installed where the file is read.
#[test]
fn the_users_fonts_are_named_on_the_runs_that_need_them() {
    use pdfcraft_chessvision::text_font::{FontFormat, FontRole, TextFont};
    let font = |role, family: &str| TextFont { bytes: Vec::new(), format: FontFormat::TrueType, role, family: family.into(), path: None };
    let (semfig, ischess) = (font(FontRole::Letters, "SemFig"), font(FontRole::Signs, "ISChess"));
    let (mut s, id) = open(super::document::chapter());
    let mut doc = s.chess_document(id, &[], &no_model(), None).unwrap().doc;
    // A sign after the moves that ISChess draws ($14, White is slightly better: its M).
    for block in doc.pages.iter_mut().flat_map(|p| p.blocks.iter_mut()) {
        if let pdfcraft_chessvision::doc::Block::Paragraph { inlines, .. } = block
            && inlines.iter().any(|i| matches!(i, pdfcraft_chessvision::doc::Inline::Move(_)))
        {
            inlines.push(pdfcraft_chessvision::doc::Inline::Nag { code: 14, source_rect: None, review: Vec::new() });
        }
    }
    let pieces = PieceSet::geometric();
    let mut notes = Vec::new();
    let (flow, _) = to_flow(&doc, &OfficeOptions { text_font: Some(&semfig), sign_font: Some(&ischess), ..options(&pieces) }, &mut notes);
    let runs: Vec<&Run> = flow.blocks.iter().flat_map(|b| if let FlowBlock::Para(r) = b { r.as_slice() } else { &[] }).collect();
    let in_font = |name: &str| runs.iter().filter(|r| r.font.as_deref() == Some(name)).map(|r| r.text.as_str()).collect::<Vec<_>>();
    assert_eq!(in_font("SemFig"), ["N", "N", "B"], "the piece letters of 2.Nf3 Nc6 3.Bb5: {runs:?}");
    assert_eq!(in_font("ISChess").len(), 1, "the sign as the sign font's letter: {runs:?}");
    assert!(notes.iter().any(|n| n.contains("SemFig, ISChess") && n.contains("installed")), "{notes:?}");
    let rtf = rtf(&flow);
    assert!(rtf.starts_with("{\\rtf1\\ansi\\deff0{\\fonttbl{\\f0\\froman Times New Roman;}{\\f1\\fnil SemFig;}{\\f2\\fnil ISChess;}}"), "{rtf:.300}");
    assert!(rtf.contains("{\\f1\\noproof N}"), "{rtf}");
    let xml = zip_part(&docx(&flow), "word/document.xml");
    assert!(xml.contains("<w:rFonts w:ascii=\"SemFig\" w:hAnsi=\"SemFig\" w:cs=\"SemFig\"/><w:noProof/>"), "{xml}");
    // A font with no family name is not named: its letters keep the document's font.
    let (plain, _) = to_flow(&doc, &OfficeOptions { text_font: Some(&font(FontRole::Letters, " ")), ..options(&pieces) }, &mut Vec::new());
    assert!(pdfcraft_export::flow::fonts(&plain).is_empty());
}

/// One part of a package the export crate's `Zip` wrote, inflated.
fn zip_part(zip: &[u8], name: &str) -> String {
    let at = |i: usize, n: usize| zip[i..i + n].iter().rev().fold(0usize, |v, b| v << 8 | *b as usize);
    let mut i = 0;
    while zip[i..].starts_with(b"PK\x03\x04") {
        let (size, name_len) = (at(i + 18, 4), at(i + 26, 2));
        let data = i + 30 + name_len;
        if &zip[i + 30..data] == name.as_bytes() {
            let mut s = String::new();
            std::io::Read::read_to_string(&mut flate2::read::DeflateDecoder::new(&zip[data..data + size]), &mut s).unwrap();
            return s;
        }
        i = data + size;
    }
    panic!("{name} missing")
}

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

#[test]
fn the_lichess_address_is_a_real_hyperlink_in_word_and_rtf() {
    let (mut s, id) = open(super::document::chapter());
    let doc = s.chess_document(id, &[], &no_model(), None).unwrap().doc;
    let pieces = PieceSet::geometric();
    let (flow, _) = to_flow(&doc, &options(&pieces), &mut Vec::new());
    let url = doc
        .pages
        .iter()
        .flat_map(|p| &p.blocks)
        .find_map(|b| if let pdfcraft_chessvision::doc::Block::Diagram(d) = b { d.lichess_url.clone() } else { None })
        .unwrap();
    assert!(url.starts_with("https://lichess.org/analysis/"), "{url}");

    let word = docx(&flow);
    let xml = zip_part(&word, "word/document.xml");
    let rels = zip_part(&word, "word/_rels/document.xml.rels");
    let body = roxmltree::Document::parse(&xml).unwrap();
    let rel_doc = roxmltree::Document::parse(&rels).unwrap();
    const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    let links: Vec<(String, String)> = body
        .descendants()
        .filter(|n| n.has_tag_name("hyperlink"))
        .map(|h| {
            let rel = rel_doc.descendants().find(|n| n.attribute("Id") == h.attribute((R, "id"))).unwrap();
            assert_eq!(rel.attribute("TargetMode"), Some("External"));
            assert!(rel.attribute("Type").unwrap().ends_with("/relationships/hyperlink"));
            (rel.attribute("Target").unwrap().to_owned(), h.descendants().filter(|n| n.is_text()).filter_map(|n| n.text()).collect())
        })
        .collect();
    assert_eq!(links, [(url.clone(), "Analyze on lichess".to_owned())]);

    let rich = rtf(&flow);
    assert!(rich.contains(&format!("{{\\field{{\\*\\fldinst{{HYPERLINK \"{url}\"}}}}{{\\fldrslt{{\\ul {{ Analyze on lichess}}}}}}}}")), "{rich}");

    // Without the addresses, no link either.
    let (flow, _) = to_flow(&doc, &OfficeOptions { links: false, ..options(&pieces) }, &mut Vec::new());
    assert!(!zip_part(&docx(&flow), "word/document.xml").contains("w:hyperlink"));
    assert!(!rtf(&flow).contains("HYPERLINK"));
}
