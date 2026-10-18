//! Chess OCR of a page (blueprint S13b, the engine's part): the page image read into chess text
//! when its text layer cannot be believed.
//!
//! The page is rendered once, whole, at one resolution, so that the glyph reader's boxes, the
//! general OCR's words and the view-space text all share one pixel grid:
//!
//! 1. the page's diagrams are painted out (a board read as glyphs is noise);
//! 2. the glyph reader (`chessvision::glyphs`, the user's `chess-glyphs` model) reads every line;
//!    the general OCR (`ocrs`, when its models are installed) reads the words;
//! 3. the two are bucketed by line ([`bucket`]: `fuse_line` groups by columns only) and fused:
//!    notation from the glyph reader, prose from the general OCR; without the general OCR, the
//!    glyph reader alone;
//! 4. the book's cipher and the legality repair (`chessvision::cipher::repair_lines`) run over the
//!    page's lines, from the diagram the page shows (or the game start at `1.`); what they change
//!    or cannot settle becomes a review flag on the text;
//! 5. the lines become the same `ChessPageText` a text layer gives.
//!
//! Reading order is top to bottom by line: a two-column page is read across both columns.

use pdfcraft_chess::repair::GameContext;
use pdfcraft_chess::san::Lang;
use pdfcraft_chessvision::chess_text::ChessPageText;
use pdfcraft_chessvision::cipher::{BookCipher, repair_lines};
use pdfcraft_chessvision::coords::PixelRect;
use pdfcraft_chessvision::doc::TextReview;
use pdfcraft_chessvision::glyphs::fuse::{FusedLine, OcrWord, fuse_line, glyphs_only, to_chess_text};
use pdfcraft_chessvision::glyphs::{GlyphClassifier, GlyphLine, GlyphOptions, read, text_scale};
use pdfcraft_chessvision::raster::GrayImage;

/// The resolution pages are read at for chess OCR.
pub const OCR_DPI: f32 = 300.0;

/// What OCR made of one page.
#[derive(Clone, Debug)]
pub struct OcrText {
    pub text: ChessPageText,
    /// The repair's changes and doubts, as review flags over the text.
    pub reviews: Vec<TextReview>,
    pub lines: usize,
    /// Words the repair changed, and words it could not settle.
    pub edits: usize,
    pub flags: usize,
    /// Whether the general OCR read the prose (otherwise the glyph reader read everything).
    pub general_ocr: bool,
}

/// The vertical overlap of two pixel spans, as a share of the shorter.
fn v_overlap(a: &PixelRect, b: &PixelRect) -> f32 {
    let top = a.y0.max(b.y0);
    let bottom = a.y1.min(b.y1);
    let shorter = (a.y1.saturating_sub(a.y0)).min(b.y1.saturating_sub(b.y0)).max(1);
    bottom.saturating_sub(top) as f32 / shorter as f32
}

/// Fuse the glyph reader's lines with the general OCR's words, line by line: each OCR word joins
/// the glyph line it overlaps most vertically (at least half of the shorter of the two); words
/// that join none are lines of their own (fused with no glyphs: prose). Lines come back top to
/// bottom.
pub fn bucket(glyph_lines: &[GlyphLine], ocr: &[OcrWord]) -> Vec<FusedLine> {
    let mut words: Vec<Vec<OcrWord>> = vec![Vec::new(); glyph_lines.len()];
    let mut loose: Vec<OcrWord> = Vec::new();
    for w in ocr {
        let best = glyph_lines
            .iter()
            .enumerate()
            .map(|(i, l)| (i, v_overlap(&l.rect, &w.rect)))
            .filter(|(_, o)| *o >= 0.5)
            .max_by(|a, b| a.1.total_cmp(&b.1));
        match best.and_then(|(i, _)| words.get_mut(i)) {
            Some(slot) => slot.push(w.clone()),
            None => loose.push(w.clone()),
        }
    }
    let mut out: Vec<FusedLine> = glyph_lines.iter().zip(&words).map(|(line, w)| fuse_line(w, std::slice::from_ref(line))).collect();
    // Loose words, grouped into lines among themselves.
    loose.sort_by_key(|w| (w.rect.y0, w.rect.x0));
    let mut group: Vec<OcrWord> = Vec::new();
    for w in loose {
        if group.last().is_some_and(|g| v_overlap(&g.rect, &w.rect) < 0.5) {
            out.push(fuse_line(&group, &[]));
            group.clear();
        }
        group.push(w);
    }
    if !group.is_empty() {
        out.push(fuse_line(&group, &[]));
    }
    out.retain(|l| !l.words.is_empty());
    out.sort_by_key(|l| (l.rect.y0, l.rect.x0));
    out
}

/// `image` with `rects` painted white.
fn masked(image: &GrayImage, rects: &[PixelRect]) -> Option<GrayImage> {
    let (w, h) = (image.width(), image.height());
    let mut data = image.data().to_vec();
    for r in rects {
        for y in r.y0.min(h)..r.y1.min(h) {
            let row = usize::try_from(y).ok()?.checked_mul(usize::try_from(w).ok()?)?;
            let (x0, x1) = (usize::try_from(r.x0.min(w)).ok()?, usize::try_from(r.x1.min(w)).ok()?);
            if let Some(span) = data.get_mut(row + x0..row + x1) {
                span.fill(255);
            }
        }
    }
    GrayImage::new(w, h, data).ok()
}

/// Read one page image (`gray`, the whole displayed page at `dpi`) as chess text. `diagrams`
/// are the page's diagrams in the same pixels; `ocr` the general OCR's words, if it ran;
/// `context` the position the page's moves start from; `book` the document's cipher, which
/// learns from the page.
#[allow(clippy::too_many_arguments)]
pub fn read_page(
    gray: &GrayImage,
    dpi: f32,
    diagrams: &[PixelRect],
    glyphs: &dyn GlyphClassifier,
    ocr: Option<&[OcrWord]>,
    lang: Lang,
    context: Option<&GameContext>,
    book: &mut BookCipher,
    page: u32,
) -> Result<OcrText, String> {
    let image = masked(gray, diagrams).ok_or("the page image could not be prepared")?;
    let scale = text_scale(&image, &[]).map_err(|e| e.to_string())?;
    let lines = read(&image, None, scale, glyphs, &GlyphOptions::default()).map_err(|e| e.to_string())?;
    let fused = match ocr {
        Some(words) => bucket(&lines, words),
        None => glyphs_only(&lines),
    };
    let repaired = repair_lines(&fused, context, lang, &book.proven());
    if let Some(proof) = &repaired.proof {
        book.record(proof, page);
    }
    let mut reviews: Vec<TextReview> = repaired.edits.iter().filter_map(|e| TextReview::from_edit(e, dpi)).collect();
    reviews.extend(repaired.flags.iter().filter_map(|f| TextReview::from_flag(f, dpi)));
    Ok(OcrText {
        text: to_chess_text(&repaired.lines, dpi, Some(lang)),
        reviews,
        lines: repaired.lines.len(),
        edits: repaired.edits.len(),
        flags: repaired.flags.len(),
        general_ocr: ocr.is_some(),
    })
}

/// The general OCR's words on `gray`, as the fusion takes them; `None` when its models are not
/// installed (the glyph reader then reads everything).
pub fn general_ocr_words(gray: &GrayImage) -> Option<Result<Vec<OcrWord>, String>> {
    let engine = crate::ocr::engine().ok()?;
    Some(engine.recognize(gray.data(), gray.width(), gray.height()).map_err(|e| e.to_string()).map(|lines| {
        lines
            .into_iter()
            .flat_map(|l| l.words)
            .filter(|w| !w.text.trim().is_empty() && w.rect.iter().all(|v| v.is_finite()))
            .map(|w| {
                // Finite and clamped at 0 before the casts; `as` saturates at u32::MAX.
                let [x0, y0, x1, y1] = w.rect;
                OcrWord {
                    text: w.text,
                    rect: PixelRect {
                        x0: x0.max(0.0).floor() as u32,
                        y0: y0.max(0.0).floor() as u32,
                        x1: x1.max(0.0).ceil() as u32,
                        y1: y1.max(0.0).ceil() as u32,
                    },
                }
            })
            .collect()
    }))
}
