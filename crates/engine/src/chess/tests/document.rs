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

/// Pages with `F1` a Merida diagram font and `F2` Helvetica ([`two_font_pdf`]), one content
/// stream each.
fn two_font_pages(contents: &[String]) -> Vec<u8> {
    let widths = vec!["1000"; 95].join(" ");
    let n = contents.len();
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 5 + 2 * i)).collect();
    let mut objs: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        format!("<< /Type /Pages /Kids [{}] /Count {n} /MediaBox [0 0 600 800] >>", kids.join(" ")).into_bytes(),
        format!("<< /Type /Font /Subtype /Type1 /BaseFont /ChessMerida /FirstChar 32 /LastChar 126 /Widths [{widths}] >>").into_bytes(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_vec(),
    ];
    for (i, c) in contents.iter().enumerate() {
        objs.push(
            format!("<< /Type /Page /Parent 2 0 R /Contents {} 0 R /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> >>", 6 + 2 * i).into_bytes(),
        );
        objs.push(format!("<< /Length {} >>\nstream\n{c}\nendstream", c.len()).into_bytes());
    }
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

/// A workbook sets each exercise's caption above its board, and a page's last caption at its
/// foot, with the board at the top of the next page. Read alone (as the Text tab reads it), the
/// next page's board still takes that number from a look at the page before; read with the page
/// before, as before. A page whose page before ends in prose takes none.
#[test]
fn a_page_read_alone_takes_the_caption_at_the_foot_of_the_page_before() {
    let mut first = helv(45.0, 11.0, "Exercise 1");
    first.push_str(&font_board(60.0, 60.0, MERIDA_START));
    first.push_str(&helv(760.0, 11.0, "Exercise 2"));
    let mut second = font_board(60.0, 60.0, MERIDA_START);
    second.push_str(&helv(280.0, 11.0, "1.e4 e5 2.Nf3 Nc6"));
    let puzzles = |doc: &pdfcraft_chessvision::doc::ChessDoc| -> Vec<Option<u32>> {
        doc.pages.iter().flat_map(|p| &p.blocks).filter_map(|b| if let Block::Diagram(d) = b { Some(d.puzzle) } else { None }).collect()
    };
    let (mut s, id) = open(two_font_pages(&[first, second.clone()]));
    let alone = s.chess_document(id, &[1], &no_model(), Some(pdfcraft_chess::san::Lang::En)).unwrap();
    assert_eq!(puzzles(&alone.doc), [Some(2)], "{:#?}", alone.doc);
    let both = s.chess_document(id, &[0, 1], &no_model(), Some(pdfcraft_chess::san::Lang::En)).unwrap();
    assert_eq!(puzzles(&both.doc), [Some(1), Some(2)]);
    assert_eq!(both.doc.pages[0].trailing, Some(2));
    // The label must stand at the foot: prose after it keeps it on its page.
    let mut prose = helv(45.0, 11.0, "Exercise 1");
    prose.push_str(&font_board(60.0, 60.0, MERIDA_START));
    prose.push_str(&helv(740.0, 11.0, "Exercise 2"));
    prose.push_str(&helv(760.0, 11.0, "is the next one, overleaf."));
    let (mut s, id) = open(two_font_pages(&[prose, second]));
    let alone = s.chess_document(id, &[1], &no_model(), Some(pdfcraft_chess::san::Lang::En)).unwrap();
    assert_eq!(puzzles(&alone.doc), [None]);
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

/// A book set with ISChess (Quality Chess, Chess Informant): its letters are signs and pieces.
/// `1.e4 e5 2.` then `C` (a white knight) `f3` and `O` (±) read as 2.Nf3 with White clearly better.
#[test]
fn ischess_pieces_and_signs_are_read_from_the_text_layer() {
    let content = format!(
        "{}BT /F2 10 Tf 50 550 Td (1.e4 e5 2.) Tj /F3 10 Tf (C) Tj /F2 10 Tf (f3) Tj /F3 10 Tf (O) Tj ET\n",
        super::font_board(50.0, 50.0, super::MERIDA_START)
    );
    let (mut s, id) = open(super::pdf_fonts(&content, &["ChessMerida", "Times-Roman", "ISChess"], None, ""));
    let read = s.chess_document(id, &[], &no_model(), None).unwrap();
    let inlines: Vec<&Inline> = read.doc.pages[0].blocks.iter().flat_map(|b| b.inlines()).collect();
    let moves: Vec<(&str, MoveStatus)> =
        inlines.iter().filter_map(|i| if let Inline::Move(m) = i { Some((m.san.as_str(), m.status)) } else { None }).collect();
    assert_eq!(moves, vec![("e4", MoveStatus::Valid), ("e5", MoveStatus::Valid), ("Nf3", MoveStatus::Valid)], "{:#?}", read.doc);
    let nags: Vec<u16> = inlines.iter().filter_map(|i| if let Inline::Nag { code, .. } = i { Some(*code) } else { None }).collect();
    assert_eq!(nags, vec![16], "{:#?}", read.doc);
}

/// `pages` Helvetica pages, page `k` holding the moves `2k+1.Nf3 Nf6 2k+2.Ng1 Ng8` (one game
/// running on over all of them, from `1.Nf3` on the first), then `last` on the last page.
fn knight_shuffle(pages: usize, last: &str) -> Vec<u8> {
    let contents: Vec<String> = (0..pages)
        .map(|k| {
            let mut c = helv(60.0, 11.0, &format!("{}.Nf3 Nf6 {}.Ng1 Ng8", 2 * k + 1, 2 * k + 2));
            if k + 1 == pages {
                c.push_str(&helv(90.0, 11.0, last));
            }
            c
        })
        .collect();
    helvetica_pages(&contents)
}

/// Page `page` of `id` read alone (as the Text tab reads it), and where its first move is.
fn page_alone(s: &mut Session, id: DocId, page: usize) -> (pdfcraft_chessvision::doc::ChessPage, crate::chess::span::MoveAt) {
    let read = s.chess_document(id, &[page], &no_model(), Some(pdfcraft_chess::san::Lang::En)).unwrap();
    let centre = read.doc.pages.into_iter().next().unwrap();
    let (block, inline) = centre
        .blocks
        .iter()
        .enumerate()
        .find_map(|(b, block)| block.inlines().iter().position(|i| matches!(i, Inline::Move(_))).map(|i| (b, i)))
        .unwrap();
    (centre, crate::chess::span::MoveAt { page, block, inline })
}

fn options() -> crate::chess::document::OcrOptions<'static> {
    crate::chess::document::OcrOptions { mode: crate::chess::document::OcrMode::Off, ..Default::default() }
}

/// A move at the top of a page that goes on from the page before has no game on its own page:
/// read with the pages around it, it is a move of the game begun two pages back, which runs on
/// to the page after; the result there ends it, so the page after that is not read.
#[test]
fn a_game_is_read_on_across_the_pages_around_a_move() {
    let mut contents: Vec<String> = Vec::new();
    for k in 0..4 {
        let mut c = helv(60.0, 11.0, &format!("{}.Nf3 Nf6 {}.Ng1 Ng8", 2 * k + 1, 2 * k + 2));
        if k == 3 {
            c.push_str(&helv(90.0, 11.0, "1-0"));
        }
        contents.push(c);
    }
    contents.push(helv(60.0, 11.0, "1.e4 e5 2.Nf3 Nc6"));
    let (mut s, id) = open(helvetica_pages(&contents));
    let (centre, at) = page_alone(&mut s, id, 2);
    assert!(
        centre.blocks.iter().flat_map(|b| b.inlines()).all(|i| !matches!(i, Inline::Move(m) if m.game.is_some())),
        "alone, page 3's moves have no position"
    );
    let read = s.chess_game_span(id, centre, at, &no_model(), &options()).unwrap();
    let span = read.span;
    assert_eq!(span.pages, (0, 3), "back to the game's first page, on to its result");
    let at = span.at.expect("the move is in the game");
    let game = &span.doc.games[at.game];
    assert_eq!(game.score.mainline().len(), 16, "four pages of four moves");
    assert_eq!(game.score.nodes[at.node].san, "Nf3");
    let board = span.doc.board_game(at.game).unwrap();
    assert_eq!(board.game.first_move_number(), 1);
}

/// The pages read are bounded: four each way. A move whose game starts further back than that
/// stays in no game; one whose game runs on further is cut after four pages.
#[test]
fn a_game_span_reads_at_most_four_pages_each_way() {
    let (mut s, id) = open(knight_shuffle(10, ""));
    let (centre, at) = page_alone(&mut s, id, 5);
    let span = s.chess_game_span(id, centre, at, &no_model(), &options()).unwrap().span;
    assert_eq!((span.pages, span.at), ((1, 5), None), "four pages back, no game start among them");

    let (centre, at) = page_alone(&mut s, id, 1);
    let span = s.chess_game_span(id, centre, at, &no_model(), &options()).unwrap().span;
    assert_eq!(span.pages, (0, 5));
    let at = span.at.unwrap();
    assert_eq!(span.doc.games[at.game].score.mainline().len(), 24);
    assert_eq!(span.doc.games[at.game].score.nodes[at.node].san, "Nf3");
}

/// A page that cannot be read ends the run there; the page read keeps its own game.
#[test]
fn a_page_that_cannot_be_read_ends_the_span() {
    let (mut s, id) = open(knight_shuffle(3, ""));
    let (centre, at) = page_alone(&mut s, id, 0);
    let mut asked = Vec::new();
    let span = crate::chess::span::span_game(centre, at, 3, |p| {
        asked.push(p);
        None
    });
    assert_eq!(asked, [1], "only the page after: page 1 has no page before");
    assert_eq!(span.pages, (0, 0));
    let at = span.at.unwrap();
    assert_eq!(span.doc.games[at.game].score.mainline().len(), 4);
}

/// A game that starts at `1.` is not read back from, and one whose result is printed is not
/// read on from: a page holding a whole game reads no other page. Without its result, the page
/// after is read, but holds none of its moves: it is not kept, and its prose is not the game's
/// comment.
#[test]
fn a_whole_game_on_its_page_reads_no_other_page() {
    let contents = [helv(60.0, 11.0, "Prose."), helv(60.0, 11.0, "1.e4 e5 2.Nf3 Nc6 1-0"), helv(60.0, 11.0, "Prose.")];
    let (mut s, id) = open(helvetica_pages(&contents));
    let (centre, at) = page_alone(&mut s, id, 1);
    let mut asked = Vec::new();
    let span = crate::chess::span::span_game(centre, at, 3, |p| {
        asked.push(p);
        None
    });
    assert!(asked.is_empty(), "{asked:?}");
    assert_eq!(span.pages, (1, 1));
    assert!(span.at.is_some());

    let contents = [helv(60.0, 11.0, "Prose."), helv(60.0, 11.0, "1.e4 e5 2.Nf3 Nc6"), helv(60.0, 11.0, "Prose.")];
    let (mut s, id) = open(helvetica_pages(&contents));
    let (centre, at) = page_alone(&mut s, id, 1);
    let mut asked = Vec::new();
    let read = |p: usize| {
        asked.push(p);
        let doc = s.chess_document(id, &[p], &no_model(), Some(pdfcraft_chess::san::Lang::En)).unwrap().doc;
        doc.pages.into_iter().next()
    };
    let span = crate::chess::span::span_game(centre, at, 3, read);
    assert_eq!(asked, [2], "the page after was read: the game had no result");
    assert_eq!(span.pages, (1, 1), "and not kept");
    let at = span.at.unwrap();
    let score = &span.doc.games[at.game].score;
    assert_eq!(score.mainline().len(), 4);
    assert!(score.nodes.iter().all(|n| n.comment.is_empty()), "{:?}", score.nodes.last().map(|n| &n.comment));
}
