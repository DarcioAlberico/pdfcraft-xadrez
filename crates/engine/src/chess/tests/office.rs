//! Chess ▸ Export ▸ Word and RTF: the diagram image is the position (checked pixel by pixel),
//! and the chapter is a flowing document (plan A8 P9a): headings, runs whose moves spelling
//! checks skip, each diagram a picture with its text alternative and caption, real links.

use pdfcraft_chess::draw::{BoardStyle, PieceSet};
use pdfcraft_chessvision::doc::html::Figurines;

use super::*;
use crate::chess::office::{DIAGRAM_PT, OfficeOptions, diagram_png, docx, rtf, to_flow};
use pdfcraft_export::flow::{FlowBlock, Run};

/// A PNG's pixels as RGBA, a paletted one expanded (plan A9-X1b).
fn decode(png: &[u8]) -> (u32, u32, Vec<u8>) {
    let mut dec = png::Decoder::new(std::io::Cursor::new(png));
    dec.set_transformations(png::Transformations::EXPAND);
    let mut reader = dec.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgb => buf.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::Rgba => buf,
        other => panic!("{other:?}"),
    };
    (info.width, info.height, rgba)
}

/// A PNG's colour type, as written.
fn color_type(png: &[u8]) -> png::ColorType {
    png::Decoder::new(std::io::Cursor::new(png)).read_info().unwrap().info().color_type
}

/// The names of the files in a package the export crate's `Zip` wrote, in its order.
fn zip_names(zip: &[u8]) -> Vec<String> {
    let at = |i: usize, n: usize| zip[i..i + n].iter().rev().fold(0usize, |v, b| v << 8 | *b as usize);
    let (mut i, mut out) = (0, Vec::new());
    while zip[i..].starts_with(b"PK\x03\x04") {
        let (size, name_len) = (at(i + 18, 4), at(i + 26, 2));
        out.push(String::from_utf8(zip[i + 30..i + 30 + name_len].to_vec()).unwrap());
        i += 30 + name_len + size;
    }
    out
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

/// Plan A9-X1b (R8-09): the diagram's picture is paletted, a byte a pixel where RGBA took four,
/// and is the board drawn for the screen within a few levels of its antialiasing.
#[test]
fn the_diagram_picture_is_paletted_and_the_same_board() {
    let (placement, pieces) = (parse_placement(OPENING).unwrap(), PieceSet::geometric());
    let style = BoardStyle { coordinates: true, ..BoardStyle::default() };
    let png = diagram_png(&placement, &pieces, &style, 300).unwrap();
    assert_eq!(color_type(&png), png::ColorType::Indexed);
    let (w, h, px) = decode(&png);
    let (_, _, screen) = crate::chess::office::diagram_rgba(&placement, &pieces, &style, 300).unwrap();
    assert_eq!((w, h, px.len()), (300, 300, screen.len()));
    let worst = px.iter().zip(&screen).map(|(a, b)| a.abs_diff(*b)).max().unwrap_or(0);
    assert!(worst <= 8, "{worst}");
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
        svg: true,
        text_font: None,
        sign_font: None,
        lang: "en",
        page_labels: &[],
        naming: pdfcraft_chess::describe::Naming::Letters,
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

    let docx = docx(&flow).unwrap();
    for part in ["word/document.xml", "word/styles.xml", "word/_rels/document.xml.rels", "[Content_Types].xml", "docProps/core.xml"] {
        roxmltree::Document::parse(&zip_part(&docx, part)).unwrap_or_else(|e| panic!("{part}: {e}"));
    }
    assert!(docx.windows(23).any(|w| w == b"word/media/diagram1.png"), "the docx holds the diagram image");
    // And the same board as SVG (plan A8 P9a), with no <use> (D18) and no title: the picture's
    // text alternative says it (below), so the same position is the same file (plan A9-X1b).
    let svg = zip_part(&docx, "word/media/diagram1.svg");
    let svg_doc = roxmltree::Document::parse(&svg).unwrap();
    assert!(!svg_doc.descendants().any(|n| n.has_tag_name("title") || n.has_tag_name("use")), "{svg:.300}");
    assert_eq!(svg_doc.descendants().filter(|n| n.has_tag_name("path")).count(), 3 + 64, "the board and 32 pieces in two layers each");
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
    assert!(rtf.contains("\\deflang1033{"), "the book's language: {rtf:.200}");
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
    assert!(
        rtf.starts_with("{\\rtf1\\ansi\\deff0\\deflang1033{\\fonttbl{\\f0\\froman Times New Roman;}{\\f1\\fnil SemFig;}{\\f2\\fnil ISChess;}}"),
        "{rtf:.300}"
    );
    assert!(rtf.contains("{\\f1\\noproof N}"), "{rtf}");
    let xml = zip_part(&docx(&flow).unwrap(), "word/document.xml");
    assert!(xml.contains("<w:rFonts w:ascii=\"SemFig\" w:hAnsi=\"SemFig\" w:cs=\"SemFig\"/><w:noProof/>"), "{xml}");
    // A font with no family name is not named: its letters keep the document's font.
    let (plain, _) = to_flow(&doc, &OfficeOptions { text_font: Some(&font(FontRole::Letters, " ")), ..options(&pieces) }, &mut Vec::new());
    assert!(pdfcraft_export::flow::fonts(&plain).is_empty());
}

/// One part of a package the export crate's `Zip` wrote, inflated.
pub(super) fn zip_part(zip: &[u8], name: &str) -> String {
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

    let word = docx(&flow).unwrap();
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
    assert!(!zip_part(&docx(&flow).unwrap(), "word/document.xml").contains("w:hyperlink"));
    assert!(!rtf(&flow).contains("HYPERLINK"));
}

/// Plan A9-T2a (R3-08): a diagram that cannot be drawn is noted by the page the export names it
/// with, its printed label as in its own caption; the note said the index + 1 ("page 25" for the
/// page the Word file calls "xiii").
#[test]
fn an_undrawn_diagram_is_noted_by_the_page_its_caption_names() {
    let (mut s, id) = open(super::document::chapter());
    let mut doc = s.chess_document(id, &[], &no_model(), None).unwrap().doc;
    doc.pages[0].index = 24;
    for block in &mut doc.pages[0].blocks {
        if let pdfcraft_chessvision::doc::Block::Diagram(d) = block {
            d.reading.as_mut().unwrap().fen = "not a position".into();
            // Nothing settled from it either (the walk would settle nothing from no position).
            d.settled = None;
        }
    }
    let mut labels = vec![String::new(); 25];
    labels[24] = "xiii".into();
    let pieces = PieceSet::geometric();
    let mut notes = Vec::new();
    let (flow, drawn) = to_flow(&doc, &OfficeOptions { page_labels: &labels, ..options(&pieces) }, &mut notes);
    assert_eq!(drawn, 0);
    let caption = flow
        .blocks
        .iter()
        .find_map(|b| match b {
            FlowBlock::Para(runs) if text_of(runs).contains("FEN not a position") => Some(text_of(runs)),
            _ => None,
        })
        .unwrap();
    assert!(caption.starts_with("Page xiii · diagram 1"), "{caption}");
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].starts_with("page xiii, diagram 1: "), "{notes:?}");
    // The engine says the page by the same rule: a document without labels by its number.
    assert_eq!(crate::chess::export::page_label(s.get(id).unwrap(), 0).to_string(), "1");
    assert_eq!(crate::chess::export::page_label(s.get(id).unwrap(), 0).printed, None);
}

/// Plan A9-X3b (R5-06): Word and RTF print a sign's name after it, in the book's language where
/// the interface names it, else in English in a run that says so; the move marks (`!`, `?`) are
/// not named.
#[test]
fn a_sign_is_followed_by_its_name() {
    use pdfcraft_chessvision::doc::{Block, ChessDoc, Inline, LangTag};
    let (mut s, id) = open(super::document::chapter());
    let mut doc = s.chess_document(id, &[], &no_model(), None).unwrap().doc;
    let para = doc
        .pages
        .iter_mut()
        .flat_map(|p| p.blocks.iter_mut())
        .find_map(|b| match b {
            Block::Paragraph { inlines, .. } if inlines.iter().any(|i| matches!(i, Inline::Move(_))) => Some(inlines),
            _ => None,
        })
        .unwrap();
    para.push(Inline::Nag { code: 1, source_rect: None, review: Vec::new() });
    para.push(Inline::Nag { code: 16, source_rect: None, review: Vec::new() });
    let pieces = PieceSet::geometric();
    let flow_in = |doc: &ChessDoc| {
        let (flow, _) = to_flow(doc, &options(&pieces), &mut Vec::new());
        flow.blocks.iter().flat_map(|b| if let FlowBlock::Para(r) = b { r.clone() } else { Vec::new() }).collect::<Vec<Run>>()
    };
    for page in &mut doc.pages {
        page.text_lang = LangTag::parse("pt");
    }
    let runs = flow_in(&doc);
    assert!(text_of(&runs).contains("!± (As brancas estão claramente melhor)"), "{}", text_of(&runs));
    for page in &mut doc.pages {
        page.text_lang = LangTag::parse("nl");
    }
    let runs = flow_in(&doc);
    let named = runs.iter().find(|r| r.text == " (White is clearly better)").unwrap_or_else(|| panic!("{runs:?}"));
    assert_eq!(named.lang.as_deref(), Some("en"));
}

/// The chapter's page with its diagram printed again after it as diagram 2 and, with `other`, a
/// third diagram showing `other` (as the walk would settle nothing from it).
fn chapter_with_repeats(other: Option<&str>) -> pdfcraft_chessvision::doc::ChessDoc {
    use pdfcraft_chessvision::doc::Block;
    let (mut s, id) = open(super::document::chapter());
    let mut doc = s.chess_document(id, &[], &no_model(), None).unwrap().doc;
    let page = &mut doc.pages[0];
    let Some(Block::Diagram(first)) = page.blocks.iter().find(|b| matches!(b, Block::Diagram(_))).cloned() else { panic!("no diagram") };
    page.blocks.push(Block::Diagram(pdfcraft_chessvision::doc::DiagramBlock { number: 1, ..first.clone() }));
    if let Some(fen) = other {
        let mut third = pdfcraft_chessvision::doc::DiagramBlock { number: 2, settled: None, ..first.clone() };
        third.reading.as_mut().unwrap().fen = fen.into();
        page.blocks.push(Block::Diagram(third));
        page.blocks.push(Block::Diagram(pdfcraft_chessvision::doc::DiagramBlock { number: 3, ..first }));
    }
    doc
}

/// The figures of a flowing document.
fn figures(flow: &pdfcraft_export::flow::FlowDoc) -> Vec<&pdfcraft_export::flow::Figure> {
    flow.blocks.iter().filter_map(|b| if let FlowBlock::Figure(f) = b { Some(f) } else { None }).collect()
}

/// Plan A9-X1b (R8-09): a position the book prints again is drawn once and stored once in the
/// Word file, its PNG and its SVG; each figure keeps its own text alternative and drawing id.
#[test]
fn a_position_printed_again_is_one_picture() {
    let doc = chapter_with_repeats(None);
    let pieces = PieceSet::geometric();
    let mut notes = Vec::new();
    let (flow, drawn) = to_flow(&doc, &options(&pieces), &mut notes);
    assert_eq!(drawn, 2, "{notes:?}");
    let f = figures(&flow);
    assert_eq!(f.len(), 2);
    assert!(f[0].png == f[1].png && f[0].svg == f[1].svg, "the same picture");
    assert!(f[0].alt.starts_with("Diagram 1, page 1") && f[1].alt.starts_with("Diagram 2, page 1"), "{} / {}", f[0].alt, f[1].alt);
    // The SVG does not name the diagram: the figure's text alternative does.
    assert!(!f[0].svg.as_deref().unwrap_or_default().contains("<title"));
    let word = docx(&flow).unwrap();
    let media: Vec<String> = zip_names(&word).into_iter().filter(|n| n.starts_with("word/media/")).collect();
    assert_eq!(media, ["word/media/diagram1.png", "word/media/diagram1.svg"]);
    let xml = zip_part(&word, "word/document.xml");
    let body = roxmltree::Document::parse(&xml).unwrap();
    let ids: Vec<&str> = body.descendants().filter(|n| n.has_tag_name("docPr")).filter_map(|n| n.attribute("id")).collect();
    assert_eq!(ids, ["1", "2"]);
    assert_eq!(xml.matches("r:embed=\"rIdImg1\"").count(), 2);
    assert_eq!(xml.matches("r:embed=\"rIdSvg1\"").count(), 2);
    // One picture, two diagrams: each keeps its own text alternative and caption in the file.
    let alts: Vec<&str> = body.descendants().filter(|n| n.has_tag_name("docPr")).filter_map(|n| n.attribute("descr")).collect();
    assert_eq!(alts, [f[0].alt.as_str(), f[1].alt.as_str()]);
    let captions: Vec<String> = body
        .descendants()
        .filter(|p| p.has_tag_name("p") && p.descendants().any(|s| s.has_tag_name("pStyle") && s.attribute((W, "val")) == Some("Caption")))
        .map(|p| p.descendants().filter(|n| n.is_text()).filter_map(|n| n.text()).collect())
        .collect();
    assert_eq!(captions.len(), 2, "{captions:?}");
    assert!(captions[0].starts_with("Page 1 · diagram 1 of 2 · FEN ") && captions[1].starts_with("Page 1 · diagram 2 of 2 · FEN "), "{captions:?}");
    // RTF has no SVG to carry: none is drawn for it.
    let (flow, drawn) = to_flow(&doc, &OfficeOptions { svg: false, ..options(&pieces) }, &mut Vec::new());
    assert_eq!(drawn, 2);
    assert!(figures(&flow).iter().all(|f| f.svg.is_none()));
}

/// Plan A9-X1b: past the most positions, or bytes of their pictures, a file gets drawn, a diagram
/// of a new position is set as its caption (its FEN, never dropped) and a note says how many; a
/// position already drawn is shown again, as it costs nothing.
#[test]
fn past_the_limits_a_new_position_is_set_as_its_fen() {
    use crate::chess::office::{Limits, to_flow_within};
    const OTHER: &str = "r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 2 3";
    let doc = chapter_with_repeats(Some(OTHER));
    let pieces = PieceSet::geometric();
    let (flow, drawn) = to_flow(&doc, &options(&pieces), &mut Vec::new());
    assert_eq!((drawn, figures(&flow).len()), (4, 4), "within the limits, all four");
    for limits in [Limits { boards: 1, bytes: usize::MAX }, Limits { boards: 100, bytes: 1 }] {
        let mut notes = Vec::new();
        let (flow, drawn) = to_flow_within(&doc, &options(&pieces), &mut notes, limits);
        assert_eq!(drawn, 3, "{limits:?}: the start position three times");
        let set_as_fen: Vec<String> = flow
            .blocks
            .iter()
            .filter_map(|b| match b {
                FlowBlock::Para(runs) if text_of(runs).contains("FEN ") => Some(text_of(runs)),
                _ => None,
            })
            .collect();
        assert_eq!(set_as_fen.len(), 1, "{limits:?}: {set_as_fen:?}");
        assert!(set_as_fen[0].starts_with("Page 1 · diagram 3") && set_as_fen[0].contains(OTHER), "{set_as_fen:?}");
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(notes[0].starts_with("diagrams set as their FEN") && notes[0].ends_with(": 1"), "{notes:?}");
    }
}

/// Plan A9-X1b: a diagram set as its FEN past the limits is said in the export's result (its
/// notes, which the tools' reply and the app's export-finished window list), never dropped.
#[test]
fn past_the_limits_the_export_says_so_in_its_notes() {
    use crate::chess::export::{ChessExportOptions, ChessFormat, GamesBookMeta, games_book};
    use crate::chess::office::{Limits, TEST_LIMITS};
    let games = random_games(3);
    let mut o = ChessExportOptions::new("Limits", None);
    o.pieces = Arc::new(PieceSet::geometric());
    (o.text_font, o.sign_font) = (None, None);
    let meta = GamesBookMeta::default();
    let all = games_book(&games, ChessFormat::Docx, &o, &meta).unwrap();
    assert!(all.drawn >= 3 && all.notes.is_empty(), "{} {:?}", all.drawn, all.notes);
    TEST_LIMITS.with(|l| l.set(Some(Limits { boards: 1, bytes: usize::MAX })));
    let capped = games_book(&games, ChessFormat::Docx, &o, &meta);
    TEST_LIMITS.with(|l| l.set(None));
    let capped = capped.unwrap();
    assert_eq!(capped.drawn, 1);
    let over = all.drawn - 1;
    assert!(capped.notes.iter().any(|n| n.starts_with("diagrams set as their FEN") && n.ends_with(&format!(": {over}"))), "{:?}", capped.notes);
}

/// `n` games played at random from a fixed seed, each from 50 to 89 plies (fewer when one ends),
/// with a diagram ($220) after ply 24 to 63, and in every third game a second one after its last
/// ply: about 1,000 diagrams for 750 games, of about 26 pieces on average, as a book of openings
/// prints them (random play takes a piece every dozen plies or so).
fn random_games(n: usize) -> Vec<pdfcraft_chess::game::Game> {
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    (0..n)
        .map(|g| {
            let mut game = pdfcraft_chess::game::Game::standard();
            let plies = 50 + g % 40;
            let marked = 24 + g % 40;
            for ply in 1..=plies {
                let moves = game.legal_moves();
                if moves.is_empty() {
                    break;
                }
                let san = moves[(next() % moves.len() as u64) as usize].clone();
                game.play_san(&san).unwrap();
                if ply == marked || (g % 3 == 0 && ply == plies) {
                    game.toggle_nag(game.current(), 220);
                }
            }
            game
        })
        .collect()
}

/// Plan A9-X1b (R8-09): a book of 750 pages and about 1,000 diagrams, exported to Word, is at
/// most 25 MB. A real book of 748 pages and 999 diagrams made 118.5 MB before (21.2 MB now): each
/// diagram a 900 px RGBA PNG and an SVG of its pieces point by point. The positions come from
/// games played at random (a fixed seed), drawn with a bundled set as detailed as a book's font
/// (chessnut), which every build has. Measured: 987 diagrams, 123.7 MB in 39.1 s with the code
/// before, 22.4 MB in 22.7 s after. Prints the time as `PERF`, the size beside it, and fails past
/// 25 MB.
/// `cargo test --release -p pdfcraft-engine --lib perf_docx -- --ignored --nocapture`
#[test]
#[ignore = "a timing and a size, in release"]
fn perf_docx_of_a_book_of_a_thousand_diagrams() {
    use crate::chess::export::{ChessExportOptions, ChessFormat, GamesBookMeta, games_book};
    use pdfcraft_chessvision::pieces;
    let games = random_games(750);
    let mut o = ChessExportOptions::new("A thousand diagrams", None);
    o.pieces = Arc::new(pieces::from_bundled(pieces::bundled::by_id("chessnut").unwrap()).0);
    (o.text_font, o.sign_font) = (None, None);
    let t = std::time::Instant::now();
    let book = games_book(&games, ChessFormat::Docx, &o, &GamesBookMeta::default()).unwrap();
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    let shown: Vec<String> = book
        .doc
        .pages
        .iter()
        .flat_map(|p| &p.blocks)
        .filter_map(|b| if let pdfcraft_chessvision::doc::Block::Diagram(d) = b { d.shown().map(|s| s.fen.to_owned()) } else { None })
        .collect();
    let diagrams = shown.len();
    let pieces = shown.iter().map(|f| f.split(' ').next().unwrap_or("").chars().filter(char::is_ascii_alphabetic).count()).sum::<usize>() as f64
        / diagrams.max(1) as f64;
    let names = zip_names(&book.bytes);
    let (pngs, svgs) = (names.iter().filter(|n| n.ends_with(".png")).count(), names.iter().filter(|n| n.ends_with(".svg")).count());
    let size = book.bytes.len();
    println!(
        "PERF perf_docx_of_a_book_of_a_thousand_diagrams {ms:.0}\n  {} pages, {diagrams} diagrams of {pieces:.1} pieces on average, drawn as {pngs} PNG and {svgs} SVG: {size} bytes ({:.1} MB)",
        book.doc.pages.len(),
        size as f64 / 1e6
    );
    assert_eq!(book.doc.pages.len(), 750);
    assert!((950..=1050).contains(&diagrams), "{diagrams}");
    // As many pieces as a book's diagrams have: fewer would make the file small for nothing.
    assert!((25.0..=28.0).contains(&pieces), "{pieces:.1} pieces on average");
    assert_eq!(book.drawn, diagrams, "{:?}", book.notes);
    assert!(pngs == svgs && pngs + 10 >= diagrams && pngs <= diagrams, "{pngs} pictures for {diagrams} diagrams");
    assert!(size <= 25_000_000, "{size} bytes, more than 25 MB");
}
