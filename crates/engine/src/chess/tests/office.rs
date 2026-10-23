//! Chess ▸ Export ▸ Word and RTF: the diagram image is the position (checked pixel by pixel),
//! the chapter page lays out as the writers expect, and both files carry the diagram.

use pdfcraft_chess::draw::{BoardStyle, PieceSet};
use pdfcraft_chessvision::doc::html::Figurines;

use super::*;
use crate::chess::office::{DIAGRAM_PT, OfficeOptions, diagram_png, to_pages};

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
        pieces,
        style: BoardStyle::default(),
        figurines: Figurines::Unicode,
        links: true,
        link_text: "Analyze on lichess",
        diagram_px: 240,
    }
}

#[test]
fn the_chapter_lays_out_in_order_and_both_files_carry_the_diagram() {
    let (mut s, id) = open(super::document::chapter());
    let doc = s.chess_document(id, &[], &no_model(), None).unwrap().doc;
    let pieces = PieceSet::geometric();
    let mut notes = Vec::new();
    let (pages, drawn) = to_pages(&doc, &options(&pieces), &mut notes);
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!((pages.len(), drawn), (1, 1));
    let page = &pages[0];
    let texts: Vec<&str> = page.blocks.iter().map(|b| b.text.as_str()).collect();
    assert_eq!(texts[0], "Chapter 1");
    assert!(texts.contains(&"1.e4 e5 2.♞f3 ♞c6 3.♝b5 a6"), "{texts:?}");
    assert!(texts.iter().any(|t| t.starts_with("FEN: rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w")), "{texts:?}");
    assert!(texts.iter().any(|t| t.starts_with("Analyze on lichess: https://lichess.org/analysis/")), "{texts:?}");
    // Stacked top to bottom without overlapping: the image sits between the caption and the moves.
    let image = &page.images[0];
    assert_eq!(image.size, [DIAGRAM_PT, DIAGRAM_PT]);
    let caption = page.blocks.iter().find(|b| b.text.starts_with("White to play")).unwrap();
    let moves = page.blocks.iter().find(|b| b.text.starts_with("1.e4")).unwrap();
    assert!(caption.rect[1] >= image.rect[3] && image.rect[1] >= moves.rect[3], "{:?} {:?} {:?}", caption.rect, image.rect, moves.rect);
    for (i, a) in page.blocks.iter().enumerate() {
        for b in page.blocks.iter().skip(i + 1) {
            assert!(a.rect[1] >= b.rect[3] || b.rect[1] >= a.rect[3], "{} overlaps {}", a.text, b.text);
        }
    }

    let rtf = pdfcraft_export::rtf(&pages);
    assert!(rtf.contains("\\pngblip") && rtf.contains("\\fs40\\b Chapter 1"), "{rtf:.400}");
    assert!(rtf.contains("\\u9822?f3"), "the knight figurine as \\uN?");
    let docx = pdfcraft_export::docx(&pages, "Chess");
    assert!(docx.windows(16).any(|w| w == b"word/media/image"), "the docx holds the diagram image");
}

#[test]
fn an_unread_diagram_is_said_in_words() {
    let (mut s, id) = open(pdf(&vector_board(60.0, 400.0), "Helvetica", None, ""));
    let doc = s.chess_document(id, &[], &no_model(), None).unwrap().doc;
    let pieces = PieceSet::geometric();
    let mut notes = Vec::new();
    let (pages, drawn) = to_pages(&doc, &options(&pieces), &mut notes);
    assert_eq!(drawn, 0);
    assert!(pages[0].images.is_empty());
    assert_eq!(pages[0].blocks.iter().map(|b| b.text.as_str()).collect::<Vec<_>>(), ["[Diagram 1 not read]"]);
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

#[test]
fn the_lichess_address_is_a_real_hyperlink_in_word_and_rtf() {
    let (mut s, id) = open(super::document::chapter());
    let doc = s.chess_document(id, &[], &no_model(), None).unwrap().doc;
    let pieces = PieceSet::geometric();
    let (pages, _) = to_pages(&doc, &options(&pieces), &mut Vec::new());
    let url = pages[0].blocks.iter().find_map(|b| b.text.strip_prefix("Analyze on lichess: ")).unwrap().to_owned();
    assert!(url.starts_with("https://lichess.org/analysis/"), "{url}");

    let docx = pdfcraft_export::docx(&pages, "Chess");
    let xml = zip_part(&docx, "word/document.xml");
    let rels = zip_part(&docx, "word/_rels/document.xml.rels");
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
    assert_eq!(links, [(url.clone(), format!("Analyze on lichess: {url}"))]);

    let rtf = pdfcraft_export::rtf(&pages);
    assert!(rtf.contains(&format!("{{\\field{{\\*\\fldinst{{HYPERLINK \"{url}\"}}}}{{\\fldrslt{{\\ul Analyze on lichess: {url}}}}}}}")), "{rtf}");

    // Without the addresses, no link either.
    let (pages, _) = to_pages(&doc, &OfficeOptions { links: false, ..options(&pieces) }, &mut Vec::new());
    assert!(pages[0].links.is_empty());
    assert!(!zip_part(&pdfcraft_export::docx(&pages, "Chess"), "word/document.xml").contains("w:hyperlink"));
}
