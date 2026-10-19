//! Chess ▸ Recognised document on a synthetic page: a heading, a caption, a board in a chess
//! diagram font and a game under it, all in the text layer. No model is needed: diagram fonts are
//! read exactly.

use pdfcraft_chessvision::doc::{Block, GameOrigin, HintKind, Inline, MoveStatus};

use super::*;
use crate::chess::document::{HEADING_MAX_CHARS, block_hints, body_size, heading_level, page_hints};

/// A 600 × 800 pt page: `content` with `F1` a Merida diagram font (every code 1000 units wide)
/// and `F2` Helvetica.
pub(super) fn two_font_pdf(content: &str) -> Vec<u8> {
    let widths = vec!["1000"; 95].join(" ");
    let objs: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 600 800] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /Font << /F1 5 0 R /F2 6 0 R >> >> >>".to_vec(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()).into_bytes(),
        format!("<< /Type /Font /Subtype /Type1 /BaseFont /ChessMerida /FirstChar 32 /LastChar 126 /Widths [{widths}] >>").into_bytes(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_vec(),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(o);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
    out
}

/// A Helvetica line at `size` whose baseline is `top` points from the page's top.
fn helv(top: f32, size: f32, text: &str) -> String {
    format!("BT /F2 {size} Tf 60 {} Td ({text}) Tj ET\n", 800.0 - top)
}

/// The page of the blueprint's check: a heading, a caption, the start position in Merida, the
/// opening played from it and a closing paragraph.
pub(super) fn chapter() -> Vec<u8> {
    let mut content = helv(40.0, 24.0, "Chapter 1");
    content.push_str(&helv(70.0, 11.0, "White to play. The Spanish opening."));
    content.push_str(&font_board(60.0, 90.0, MERIDA_START));
    content.push_str(&helv(280.0, 11.0, "1.e4 e5 2.Nf3 Nc6 3.Bb5 a6"));
    content.push_str(&helv(320.0, 11.0, "Black asks the bishop at once."));
    two_font_pdf(&content)
}

fn kinds(doc: &pdfcraft_chessvision::doc::ChessDoc) -> Vec<String> {
    doc.pages
        .iter()
        .flat_map(|p| &p.blocks)
        .map(|b| match b {
            Block::Heading { level, inlines, .. } => format!("H{level}:{}", pdfcraft_chessvision::doc::plain_text(inlines)),
            Block::Paragraph { inlines, .. } => format!("P:{}", pdfcraft_chessvision::doc::plain_text(inlines)),
            Block::Diagram(d) => format!("D{}", d.number),
        })
        .collect()
}

#[test]
fn a_chapter_page_becomes_heading_caption_diagram_and_its_game() {
    let (mut s, id) = open(chapter());
    let read = s.chess_document(id, &[], &no_model(), None).unwrap();
    assert_eq!(
        kinds(&read.doc),
        ["H1:Chapter 1", "P:White to play. The Spanish opening.", "D0", "P:1.e4 e5 2.Nf3 Nc6 3.Bb5 a6", "P:Black asks the bishop at once."],
        "{:#?}",
        read.doc
    );
    assert!(read.needs_ocr.is_empty(), "{:?}", read.needs_ocr);
    assert!(!read.lang_given);
    // The diagram is read from its font, legal and linked.
    let Some(Block::Diagram(d)) = read.doc.pages[0].blocks.get(2) else { panic!("{:#?}", read.doc) };
    assert_eq!(d.reading.as_ref().map(|r| r.fen.split(' ').next().unwrap_or_default().to_owned()), Some(START.to_owned()));
    assert!(d.lichess_url.is_some());
    // The moves are replayed from the diagram's position, into one game.
    let moves: Vec<MoveStatus> = read.doc.pages[0]
        .blocks
        .iter()
        .flat_map(|b| b.inlines())
        .filter_map(|i| if let Inline::Move(m) = i { Some(m.status) } else { None })
        .collect();
    assert_eq!(moves, vec![MoveStatus::Valid; 6]);
    assert_eq!(read.doc.games.len(), 1);
    assert!(matches!(read.doc.games[0].origin, GameOrigin::Diagram { page: 0, diagram: 0, .. }), "{:?}", read.doc.games[0].origin);
    assert!(read.doc.pgn().contains("1. e4 e5 2. Nf3 Nc6 3. Bb5 a6"), "{}", read.doc.pgn());
    // Nothing changed, and the JSON reads back equal.
    assert!(!s.get(id).unwrap().dirty);
    let json = read.doc.to_json().unwrap();
    assert_eq!(pdfcraft_chessvision::doc::ChessDoc::from_json(&json).unwrap(), read.doc);
}

#[test]
fn a_page_without_text_needs_ocr_and_a_given_language_is_kept() {
    let (mut s, id) = open(pdf(&vector_board(60.0, 400.0), "Helvetica", None, ""));
    let read = s.chess_document(id, &[0], &no_model(), Some(pdfcraft_chess::san::Lang::De)).unwrap();
    assert_eq!(read.needs_ocr.len(), 1, "{:?}", read.needs_ocr);
    assert_eq!(read.needs_ocr[0].0, 0);
    assert!(read.lang_given);
    assert_eq!(read.doc.pages[0].lang, "de");
    // The board is a diagram block without a position: no model reads drawn squares here.
    assert_eq!(kinds(&read.doc), ["D0"]);
    let Some(Block::Diagram(d)) = read.doc.pages[0].blocks.first() else { panic!() };
    assert!(d.reading.is_none() && d.lichess_url.is_none(), "{d:?}");
    // A page out of range is refused.
    assert!(s.chess_document(id, &[1], &no_model(), None).unwrap_err().contains("out of range"));
}

fn block(text: &str, size: f64, top: f64) -> pdfcraft_edit::TextBlock {
    pdfcraft_edit::TextBlock {
        text: text.into(),
        rect: [60.0, 800.0 - top - size, 400.0, 800.0 - top],
        base_font: "Helvetica".into(),
        size,
        bold: false,
        italic: false,
        color: [0.0; 3],
        rotation: 0.0,
        lines: Vec::new(),
        rows: Vec::new(),
        editor_text: text.into(),
        spans: Vec::new(),
        invisible: false,
    }
}

#[test]
fn headings_are_larger_than_the_body_and_short() {
    assert_eq!(heading_level(24.0, 9, 11.0), Some(1));
    assert_eq!(heading_level(14.0, 9, 11.0), Some(2));
    assert_eq!(heading_level(12.0, 9, 11.0), None, "within the body's size");
    assert_eq!(heading_level(24.0, HEADING_MAX_CHARS + 1, 11.0), None, "a long block is a paragraph");
    for bad in [f64::NAN, f64::INFINITY, 0.0, -3.0] {
        assert_eq!(heading_level(24.0, 9, bad), None);
        assert_eq!(heading_level(bad, 9, 11.0), None);
    }
    // The body is the size most characters use, not the most blocks.
    let blocks = [block("Title", 11.0, 20.0), block("Sub", 11.0, 40.0), block(&"word ".repeat(40), 9.0, 60.0)];
    let hints = block_hints(&blocks, None);
    assert_eq!(hints.iter().filter(|h| matches!(h.kind, HintKind::Heading { level: 2 })).count(), 2, "{hints:?}");
    assert!(block_hints(&[], None).is_empty());
    let mut broken = block("x", f64::NAN, 0.0);
    broken.rect = [f64::NAN; 4];
    assert!(block_hints(&[broken, block("body", 10.0, 50.0)], None).len() == 1);
    // Against the book's body (11 pt), the page's 9 pt introduction does not make its 11 pt
    // text headings.
    let hints = block_hints(&blocks, body_size(&blocks[..2]));
    assert!(hints.iter().all(|h| h.kind == HintKind::Paragraph), "{hints:?}");
    // A page read with OCR takes no hints from the layer that was not believed.
    let read = Ok(blocks.to_vec());
    assert_eq!(page_hints(Some(&read), None, false).0.len(), 3);
    assert_eq!(page_hints(Some(&read), None, true), (Vec::new(), None));
    assert!(page_hints(Some(&Err("bad".into())), None, false).1.is_some());
}

/// Pages in Helvetica (`/F2`), one content stream each.
fn helvetica_pages(contents: &[String]) -> Vec<u8> {
    let n = contents.len();
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 4 + 2 * i)).collect();
    let mut objs: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        format!("<< /Type /Pages /Kids [{}] /Count {n} /MediaBox [0 0 600 800] >>", kids.join(" ")).into_bytes(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_vec(),
    ];
    for (i, c) in contents.iter().enumerate() {
        objs.push(format!("<< /Type /Page /Parent 2 0 R /Contents {} 0 R /Resources << /Font << /F2 3 0 R >> >> >>", 5 + 2 * i).into_bytes());
        objs.push(
            format!(
                "<< /Length {} >>
stream
{c}
endstream",
                c.len()
            )
            .into_bytes(),
        );
    }
    let mut out = b"%PDF-1.7
"
    .to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(
            format!(
                "{} 0 obj
",
                i + 1
            )
            .as_bytes(),
        );
        out.extend_from_slice(o);
        out.extend_from_slice(
            b"
endobj
",
        );
    }
    let xref = out.len();
    out.extend_from_slice(
        format!(
            "xref
0 {}
0000000000 65535 f 
",
            objs.len() + 1
        )
        .as_bytes(),
    );
    for o in offsets {
        out.extend_from_slice(
            format!(
                "{o:010} 00000 n 
"
            )
            .as_bytes(),
        );
    }
    out.extend_from_slice(
        format!(
            "trailer
<< /Size {} /Root 1 0 R >>
startxref
{xref}
%%EOF
",
            objs.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// A game book whose game text is 13 pt and whose introductions are 9 pt: a page that opens with
/// a long introduction has more 9 pt text than 13 pt, but the body is the book's, so the game's
/// notes there are not headings and the game goes on across the page.
#[test]
fn the_body_size_is_the_books_not_the_pages() {
    let mut p1 = String::new();
    for k in 0..20 {
        p1.push_str(&helv(60.0 + 16.0 * k as f32, 13.0, "The game text runs on in the body size of the book."));
    }
    p1.push_str(&helv(420.0, 13.0, "1.e4 e5 2.Nf3 Nc6"));
    let mut p2 = String::new();
    for k in 0..8 {
        p2.push_str(&helv(60.0 + 12.0 * k as f32, 9.0, "A long introduction set small, as books set them before a game."));
    }
    p2.push_str(&helv(200.0, 13.0, "White pins."));
    p2.push_str(&helv(300.0, 13.0, "3.Bb5 a6"));
    let (mut s, id) = open(helvetica_pages(&[p1, p2]));
    let read = s.chess_document(id, &[], &no_model(), Some(pdfcraft_chess::san::Lang::En)).unwrap();
    assert!(read.doc.pages[1].blocks.iter().all(|b| !matches!(b, pdfcraft_chessvision::doc::Block::Heading { .. })), "{:?}", kinds(&read.doc));
    assert_eq!(read.doc.games.len(), 1);
    assert_eq!(read.doc.games[0].score.mainline().len(), 6);
}

/// A two-column page in Helvetica, lines of both columns on the same baselines (so the layout
/// reads them as one line each): the lines are cut at the gutter and read column by column, and
/// each column's game replays from the initial position.
#[test]
fn a_two_column_page_replays_each_columns_game() {
    let left = ["The open game is shown here.", "1.e4 e5 2.Nf3 Nc6 in the classical way", "3.Bb5 a6 and White keeps the bishop."];
    let right = ["The closed game is shown here.", "1.d4 d5 2.c4 e6 the Queen's Gambit", "3.Nc3 Nf6 and Black develops calmly."];
    let mut content = String::new();
    for (k, (l, r)) in left.iter().zip(&right).enumerate() {
        let y = 700.0 - 16.0 * k as f32;
        content.push_str(&format!("BT /F2 11 Tf 40 {y} Td ({l}) Tj ET\nBT /F2 11 Tf 320 {y} Td ({r}) Tj ET\n"));
    }
    let (mut s, id) = open(two_font_pdf(&content));
    let read = s.chess_document(id, &[], &no_model(), Some(pdfcraft_chess::san::Lang::En)).unwrap();
    let moves: Vec<(String, MoveStatus)> = read.doc.pages[0]
        .blocks
        .iter()
        .flat_map(|b| b.inlines())
        .filter_map(|i| if let Inline::Move(m) = i { Some((m.san.clone(), m.status)) } else { None })
        .collect();
    assert_eq!(
        moves.iter().map(|(m, _)| m.as_str()).collect::<Vec<_>>(),
        ["e4", "e5", "Nf3", "Nc6", "Bb5", "a6", "d4", "d5", "c4", "e6", "Nc3", "Nf6"],
        "{:#?}",
        read.doc.pages[0].blocks
    );
    assert!(moves.iter().all(|(_, st)| *st == MoveStatus::Valid), "{moves:?}");
    assert_eq!(read.doc.games.len(), 2);
}

/// The same with a book's narrow gutter (14 pt), which the text layout reads as one line per
/// baseline: the lines are cut at the gutter first.
#[test]
fn a_narrow_gutter_is_cut_before_the_columns_are_read() {
    let left = ["The open game is in this column here.", "1.e4 e5 2.Nf3 Nc6 in the classical way", "3.Bb5 a6 and White keeps the bishop"];
    let right = ["The closed game is in this column.", "1.d4 d5 2.c4 e6 is the Queen's Gambit", "3.Nc3 Nf6 and Black develops calmly."];
    let width = |t: &str| pdfcraft_fonts::helvetica_width(t, 11.0);
    let gutter = 300.0 + 14.0;
    let mut content = String::new();
    for (k, (l, r)) in left.iter().zip(&right).enumerate() {
        let y = 700.0 - 16.0 * k as f32;
        // The left line ends at x 300: right-aligned to the gutter.
        content.push_str(&format!("BT /F2 11 Tf {} {y} Td ({l}) Tj ET\nBT /F2 11 Tf {gutter} {y} Td ({r}) Tj ET\n", 300.0 - width(l)));
    }
    let (mut s, id) = open(two_font_pdf(&content));
    let read = s.chess_document(id, &[], &no_model(), Some(pdfcraft_chess::san::Lang::En)).unwrap();
    let moves: Vec<(String, MoveStatus)> = read.doc.pages[0]
        .blocks
        .iter()
        .flat_map(|b| b.inlines())
        .filter_map(|i| if let Inline::Move(m) = i { Some((m.san.clone(), m.status)) } else { None })
        .collect();
    assert_eq!(
        moves.iter().map(|(m, _)| m.as_str()).collect::<Vec<_>>(),
        ["e4", "e5", "Nf3", "Nc6", "Bb5", "a6", "d4", "d5", "c4", "e6", "Nc3", "Nf6"],
        "{:#?}",
        read.doc.pages[0].blocks
    );
    assert!(moves.iter().all(|(_, st)| *st == MoveStatus::Valid), "{moves:?}");
}
