//! Chess OCR on scanned pages made here: Helvetica text rendered to an image and embedded, so
//! the page has no text layer. The glyph classifier is a nearest-template stand-in whose
//! templates are the cells the glyph reader cuts from each character drawn alone; the aim is the
//! engine's plumbing (rendering, masking, fusion, repair, the document), not accuracy.

use std::cell::RefCell;

use pdfcraft_chessvision::glyphs::{GlyphClassifier, GlyphOptions, read};
use pdfcraft_chessvision::models::ModelError;
use pdfcraft_chessvision::raster::GrayImage;
use pdfcraft_render::{PageRenderer, RenderConfig, RenderRequest};

use super::*;

/// Helvetica `lines` (text, size, top from the page's top) rendered at `dpi` as an 8-bit gray
/// image of the whole 600 × 800 pt page.
fn scan(lines: &[(&str, f32, f32)], extra: &str, dpi: f32) -> (u32, u32, Vec<u8>) {
    let mut content = String::from(extra);
    for (text, size, top) in lines {
        content.push_str(&format!("BT /F2 {size} Tf 60 {} Td ({text}) Tj ET\n", 800.0 - top));
    }
    let pdf = super::document::two_font_pdf(&content);
    let mut r = PageRenderer::new(Arc::new(pdf), RenderConfig::default());
    let shot = r.render(RenderRequest { page: 0, scale: dpi / 72.0, ..Default::default() });
    assert!(shot.error.is_none(), "{:?}", shot.error);
    let gray = shot.rgba.chunks(4).map(|p| ((u32::from(p[0]) * 299 + u32::from(p[1]) * 587 + u32::from(p[2]) * 114) / 1000) as u8).collect();
    (shot.width, shot.height, gray)
}

/// A one-page PDF whose page is that image: no text layer.
fn scanned_pdf(lines: &[(&str, f32, f32)], extra: &str) -> Vec<u8> {
    let (w, h, data) = scan(lines, extra, 150.0);
    pdf("q 600 0 0 800 0 0 cm /Im1 Do Q\n", "Helvetica", Some((w, h, &data)), "")
}

/// Records every cell it is asked about, answering uniformly.
struct Recorder {
    cells: RefCell<Vec<Vec<f32>>>,
    names: Vec<String>,
}

impl GlyphClassifier for Recorder {
    fn classes(&self) -> &[String] {
        &self.names
    }
    fn classify(&self, cells: &[f32], count: usize) -> Result<Vec<f32>, ModelError> {
        let n = cells.len() / count.max(1);
        self.cells.borrow_mut().extend(cells.chunks(n.max(1)).map(<[f32]>::to_vec));
        Ok(vec![1.0 / self.names.len() as f32; count * self.names.len()])
    }
}

/// Nearest template, with confidence falling off with the distance (a reject mass belongs to no
/// class, so a cell unlike every template is read with little confidence).
struct Templates {
    names: Vec<String>,
    cells: Vec<Vec<f32>>,
}

impl Templates {
    /// One template per character: the cell the glyph reader cuts from it drawn alone, at the
    /// size and resolution the pages are read at.
    fn learn(chars: &str, size: f32) -> Templates {
        let mut names = Vec::new();
        let mut cells = Vec::new();
        for c in chars.chars() {
            let (w, h, data) = scan(&[(&c.to_string(), size, 100.0)], "", crate::chess::text_ocr::OCR_DPI);
            let image = GrayImage::new(w, h, data).unwrap();
            let rec = Recorder { cells: RefCell::new(Vec::new()), names: vec!["?".into()] };
            read(&image, None, None, &rec, &GlyphOptions::default()).unwrap();
            let cell = rec.cells.borrow().first().cloned().unwrap_or_else(|| panic!("no cell for {c:?}"));
            names.push(c.to_string());
            cells.push(cell);
        }
        Templates { names, cells }
    }
}

impl GlyphClassifier for Templates {
    fn classes(&self) -> &[String] {
        &self.names
    }
    fn classify(&self, cells: &[f32], count: usize) -> Result<Vec<f32>, ModelError> {
        let n = cells.len() / count.max(1);
        let (tau, reject) = (0.01f32, 0.05f32);
        let mut out = Vec::with_capacity(count * self.names.len());
        for c in cells.chunks(n.max(1)) {
            let e: Vec<f32> =
                self.cells.iter().map(|t| (-(c.iter().zip(t).map(|(a, b)| (a - b) * (a - b)).sum::<f32>() / n as f32) / tau).exp()).collect();
            let z = e.iter().sum::<f32>() + (-reject / tau).exp();
            out.extend(e.iter().map(|v| v / z));
        }
        Ok(out)
    }
}

const MOVES: &str = "1.e4 e5 2.Nf3 Nc6";

fn options(t: &Templates) -> OcrOptions<'_> {
    OcrOptions { mode: OcrMode::Auto, glyphs: Some(t), general: false, book: None }
}

use crate::chess::document::{OcrMode, OcrOptions};

/// A scanned page of moves above a board drawn over the scan: the page has no text layer, so
/// chess OCR reads it; the board is painted out first (it is not read as glyphs), and the moves
/// replay from the initial position at `1.`. (Moves after a diagram no model read would stay
/// unchecked: there is no position to play them from.)
#[test]
fn a_scanned_page_is_read_by_chess_ocr_and_its_moves_replay() {
    let templates = Templates::learn("1.e4526Nf3c", 14.0);
    // The board, drawn as squares under the moves (user space y 100..260: view 540..700).
    let board = vector_board(60.0, 100.0);
    let bytes = pdf(&format!("q 600 0 0 800 0 0 cm /Im1 Do Q\n{board}"), "Helvetica", Some(scan_image(&[(MOVES, 14.0, 300.0)])), "");
    let (mut s, id) = open(bytes);
    let read = s.chess_document_with(id, &[], &no_model(), Some(pdfcraft_chess::san::Lang::En), &options(&templates)).unwrap();
    assert!(read.needs_ocr.is_empty(), "{:?}", read.needs_ocr);
    assert_eq!(read.ocr_pages.len(), 1, "{:?}", read.notes);
    assert_eq!(read.ocr_pages[0].lines, 1, "only the moves: the board was painted out ({:?})", read.ocr_pages);
    assert!(!read.ocr_pages[0].general_ocr);
    let moves: Vec<(String, pdfcraft_chessvision::doc::MoveStatus)> = read.doc.pages[0]
        .blocks
        .iter()
        .flat_map(|b| b.inlines())
        .filter_map(|i| if let pdfcraft_chessvision::doc::Inline::Move(m) = i { Some((m.san.clone(), m.status)) } else { None })
        .collect();
    let sans: Vec<&str> = moves.iter().map(|(s, _)| s.as_str()).collect();
    assert_eq!(sans, ["e4", "e5", "Nf3", "Nc6"], "{:#?}", read.doc);
    assert!(moves.iter().all(|(_, st)| *st == pdfcraft_chessvision::doc::MoveStatus::Valid), "{moves:?}");
    assert_eq!(read.doc.games.len(), 1);
    // The board is still a diagram block, after the moves.
    assert!(matches!(read.doc.pages[0].blocks.last(), Some(pdfcraft_chessvision::doc::Block::Diagram(_))), "{:#?}", read.doc.pages[0].blocks);
}

/// The scan of `lines` as an image argument for [`pdf`].
fn scan_image(lines: &[(&str, f32, f32)]) -> (u32, u32, &'static [u8]) {
    let (w, h, data) = scan(lines, "", 150.0);
    (w, h, Box::leak(data.into_boxed_slice()))
}

#[test]
fn without_a_glyph_model_or_with_ocr_off_the_page_needs_ocr() {
    let (mut s, id) = open(scanned_pdf(&[(MOVES, 14.0, 500.0)], ""));
    let off = s.chess_document_with(id, &[], &no_model(), None, &OcrOptions { mode: OcrMode::Off, ..OcrOptions::default() }).unwrap();
    assert_eq!(off.needs_ocr.len(), 1);
    assert!(off.ocr_pages.is_empty());
    // No glyph model in the (empty) models folder: said so.
    let dir = std::env::temp_dir().join(format!("pdfcraft-no-glyphs-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let settings = ChessScanSettings { models_dir: Some(dir.clone()), ..no_model() };
    let auto = s.chess_document(id, &[], &settings, None).unwrap();
    assert!(auto.needs_ocr.first().is_some_and(|(_, why)| why.contains("chess-glyphs model")), "{:?}", auto.needs_ocr);
    let _ = std::fs::remove_dir_all(dir);
}

/// The moves as invisible Helvetica (`3 Tr`) where the scan shows them: another program's OCR.
const HIDDEN_LAYER: &str = "BT /F1 14 Tf 3 Tr 60 500 Td (1.e4 e5 2.Nf3 Nc6) Tj ET\n";

/// The SAN of the moves read on the document's first page.
fn sans(read: &crate::chess::document::ChessDocument) -> Vec<String> {
    read.doc.pages[0]
        .blocks
        .iter()
        .flat_map(|b| b.inlines())
        .filter_map(|i| if let pdfcraft_chessvision::doc::Inline::Move(m) = i { Some(m.san.clone()) } else { None })
        .collect()
}

/// A scan with FineReader's (or Acrobat's, or Tesseract's) invisible layer over it: the layer
/// reads cleanly, but it is a general OCR's guess, so Auto reads the page with chess OCR.
#[test]
fn an_invisible_layer_over_a_scan_is_read_with_chess_ocr() {
    let templates = Templates::learn("1.e4526Nf3c", 14.0);
    let scanned = format!("q 600 0 0 800 0 0 cm /Im1 Do Q\n{HIDDEN_LAYER}");
    let (mut s, id) = open(pdf(&scanned, "Helvetica", Some(scan_image(&[(MOVES, 14.0, 300.0)])), ""));
    let read = s.chess_document_with(id, &[], &no_model(), Some(pdfcraft_chess::san::Lang::En), &options(&templates)).unwrap();
    assert!(read.needs_ocr.is_empty(), "{:?}", read.needs_ocr);
    assert_eq!(read.ocr_pages.len(), 1, "the hidden layer was believed: {:?}", read.notes);

    // The same invisible text without the picture under it (text kept for search, say) is no
    // scan's OCR: the layer is read as it is.
    let (mut s, id) = open(pdf(HIDDEN_LAYER, "Helvetica", None, ""));
    let read = s.chess_document_with(id, &[], &no_model(), Some(pdfcraft_chess::san::Lang::En), &options(&templates)).unwrap();
    assert!(read.ocr_pages.is_empty() && read.needs_ocr.is_empty(), "{:?} {:?}", read.ocr_pages, read.needs_ocr);
    assert_eq!(sans(&read), ["e4", "e5", "Nf3", "Nc6"]);
}

/// The user says the book is born digital: chess OCR never runs, the page is read from its text
/// layer, and why that layer looked poor is noted.
#[test]
fn a_digital_book_reads_even_a_hidden_layer_and_notes_it() {
    use crate::chess::document::BookOrigin;
    let templates = Templates::learn("1.e4526Nf3c", 14.0);
    let scanned = format!("q 600 0 0 800 0 0 cm /Im1 Do Q\n{HIDDEN_LAYER}");
    let (mut s, id) = open(pdf(&scanned, "Helvetica", Some(scan_image(&[(MOVES, 14.0, 300.0)])), ""));
    let digital = OcrOptions { mode: BookOrigin::Digital.ocr_mode(), ..options(&templates) };
    let read = s.chess_document_with(id, &[], &no_model(), Some(pdfcraft_chess::san::Lang::En), &digital).unwrap();
    assert!(read.ocr_pages.is_empty(), "{:?}", read.ocr_pages);
    assert_eq!(sans(&read), ["e4", "e5", "Nf3", "Nc6"], "read from the text layer");
    assert_eq!(read.needs_ocr.len(), 1);
    assert!(read.needs_ocr[0].1.contains("invisible text over an image"), "{:?}", read.needs_ocr);
    let review = &read.doc.pages[0].review;
    assert!(review.iter().any(|r| format!("{r:?}").contains("invisible text over an image")), "{review:?}");
    // Scanned reads every page with chess OCR, a clean visible layer too.
    let (mut s, id) = open(pdf("BT /F1 14 Tf 60 500 Td (1.e4 e5 2.Nf3 Nc6) Tj ET", "Helvetica", None, ""));
    let scanned = OcrOptions { mode: BookOrigin::Scanned.ocr_mode(), ..options(&templates) };
    let read = s.chess_document_with(id, &[], &no_model(), Some(pdfcraft_chess::san::Lang::En), &scanned).unwrap();
    assert_eq!(read.ocr_pages.len(), 1, "{:?}", read.notes);
}

#[test]
fn ocr_words_join_the_glyph_line_they_overlap() {
    use pdfcraft_chessvision::coords::PixelRect;
    use pdfcraft_chessvision::glyphs::fuse::OcrWord;
    use pdfcraft_chessvision::glyphs::{Glyph, GlyphLine, GlyphWord};
    let r = |x0, y0, x1, y1| PixelRect { x0, y0, x1, y1 };
    let glyph_word = |text: &str, rect: PixelRect| GlyphWord {
        text: text.into(),
        rect,
        glyphs: vec![Glyph { rect, text: text.into(), p: 0.99, alternatives: Vec::new(), margin: 0.9 }],
    };
    let lines = vec![
        GlyphLine { words: vec![glyph_word("Nf3", r(100, 10, 140, 30))], rect: r(100, 10, 140, 30) },
        GlyphLine { words: vec![glyph_word("e5", r(100, 50, 130, 70))], rect: r(100, 50, 130, 70) },
    ];
    let ocr = vec![
        OcrWord { text: "Then".into(), rect: r(10, 12, 60, 30) },
        OcrWord { text: "again".into(), rect: r(10, 52, 70, 70) },
        OcrWord { text: "Footnote".into(), rect: r(10, 200, 90, 220) },
    ];
    let fused = crate::chess::text_ocr::bucket(&lines, &ocr);
    let texts: Vec<String> = fused.iter().map(|l| l.text()).collect();
    assert_eq!(texts, ["Then Nf3", "again e5", "Footnote"], "{fused:#?}");
    assert!(crate::chess::text_ocr::bucket(&[], &[]).is_empty());
}

/// With `PDFCRAFT_CHESS_TEST_MODELS` naming the folder of the user's models, the real glyph model
/// reads a scanned line of Helvetica moves (a solid font, which the S13a spike read well).
/// Printed, not asserted move by move: this measures the model, not the plumbing.
#[test]
fn the_users_glyph_model_reads_a_scanned_line() {
    let Some(dir) = std::env::var_os("PDFCRAFT_CHESS_TEST_MODELS") else {
        eprintln!("skipped: set PDFCRAFT_CHESS_TEST_MODELS to the folder of the chess models");
        return;
    };
    let line = "1.e4 e5 2.Nf3 Nc6 3.Bb5 a6 4.Ba4 Nf6 5.O-O Be7";
    let (mut s, id) = open(scanned_pdf(&[(line, 14.0, 300.0)], ""));
    let settings = ChessScanSettings { models_dir: Some(dir.into()), ..no_model() };
    let read = s
        .chess_document_with(
            id,
            &[],
            &settings,
            Some(pdfcraft_chess::san::Lang::En),
            &OcrOptions { mode: OcrMode::All, glyphs: None, general: false, book: None },
        )
        .unwrap();
    assert_eq!(read.ocr_pages.len(), 1, "{:?} {:?}", read.needs_ocr, read.notes);
    let moves: Vec<String> = read.doc.pages[0]
        .blocks
        .iter()
        .flat_map(|b| b.inlines())
        .filter_map(|i| if let pdfcraft_chessvision::doc::Inline::Move(m) = i { Some(format!("{}:{:?}", m.san, m.status)) } else { None })
        .collect();
    eprintln!(
        "read: {}\nmoves: {moves:?}",
        pdfcraft_chessvision::doc::plain_text(read.doc.pages[0].blocks.first().map(|b| b.inlines()).unwrap_or_default())
    );
    assert!(moves.len() >= 8, "most of the ten moves are read: {moves:?}");
}

/// The book cipher learned before is where a reading starts, and the reading hands it on.
#[test]
fn the_book_cipher_is_carried_from_one_reading_to_the_next() {
    use pdfcraft_chess::placement::PieceKind;
    use pdfcraft_chessvision::cipher::{BookCipher, Source};
    let mut seed = BookCipher::default();
    for _ in 0..12 {
        seed.observe("W", PieceKind::Queen, Source::Glyph, None);
    }
    assert!(seed.proven().contains_key("W"), "twelve observations settle the row");
    let (mut s, id) = open(scanned_pdf(&[(MOVES, 14.0, 300.0)], ""));
    let off = OcrOptions { mode: OcrMode::Off, book: Some(&seed), ..OcrOptions::default() };
    let read = s.chess_document_with(id, &[], &no_model(), None, &off).unwrap();
    assert_eq!(read.cipher, seed, "nothing read, so the cipher is the one given");
    let fresh = s.chess_document_with(id, &[], &no_model(), None, &OcrOptions { mode: OcrMode::Off, ..OcrOptions::default() }).unwrap();
    assert!(fresh.cipher.entries().is_empty());
}
