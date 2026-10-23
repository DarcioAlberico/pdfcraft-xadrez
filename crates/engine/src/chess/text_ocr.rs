//! Chess OCR of a page (blueprint S13b, the engine's part): the page image read into chess text
//! when its text layer cannot be believed.
//!
//! The page is rendered once, whole, at one resolution, so that the glyph reader's boxes, the
//! general OCR's words and the view-space text all share one pixel grid:
//!
//! 1. the page's diagrams are painted out, with a margin for a scan's frame (a board read as
//!    glyphs is noise);
//! 2. the glyph reader (`chessvision::glyphs`, the user's `chess-glyphs` model) reads every line;
//!    the general OCR (`ocrs`, when its models are installed) reads the words;
//! 3. the two are bucketed by line ([`bucket`]: `fuse_line` groups by columns only) and fused:
//!    notation from the glyph reader, prose from the general OCR; without the general OCR, the
//!    glyph reader alone;
//! 4. the book's cipher and the legality repair (`chessvision::cipher::repair_lines`) run over the
//!    page's lines, from the diagram the page shows (or the game start at `1.`); what they change
//!    or cannot settle becomes a review flag on the text;
//! 5. the lines become the same `ChessPageText` a text layer gives, and the frames drawn around
//!    text (a game's header box) are kept for the headings.
//!
//! Reading order is top to bottom by line: a two-column page is read across both columns.
//!
//! **An area of a page** (PGN toolbar T9, [`OcrAreaJob`]): the user draws a rectangle on a scan
//! and only that rectangle is rendered (at [`OCR_DPI`], less for an area too large for one
//! render tile) and read the same way, without the diagram mask: the glyph reader, the general
//! OCR's words when asked for, the fusion, then the book's proven cipher and the repair (which
//! judges by legality only from the game start, when the area's first move is `1.`). The result
//! is the page's text layer for that rectangle ([`AreaRead::glyphs`], view space), which the
//! capture reads like a selection, plus each glyph's top readings for review (T10).
//!
//! **A character as a training sample** (T11, [`Session::chess_glyph_sample`]): a character's box
//! rendered again at the area's resolution, padded to a square and scaled to 32 × 32 gray, the
//! pictures the user's glyph model is trained on.

use std::path::Path;
use std::sync::Arc;

use pdfcraft_chess::repair::GameContext;
use pdfcraft_chess::san::Lang;
use pdfcraft_chessvision::chess_text::{self, ChessPageText, TextQuality};
use pdfcraft_chessvision::cipher::{BookCipher, repair_lines};
use pdfcraft_chessvision::coords::{PageGeometry, PageRect, PixelRect, ViewRect, pixels_to_view};
use pdfcraft_chessvision::doc::TextReview;
use pdfcraft_chessvision::glyphs::fuse::{FusedLine, OcrWord, fuse_line, glyphs_only, to_chess_text, to_page_text};
use pdfcraft_chessvision::glyphs::{GlyphClassifier, GlyphLine, GlyphOptions, read, text_scale};
use pdfcraft_chessvision::raster::GrayImage;
use pdfcraft_render::text::PageText;
use pdfcraft_render::{PageRenderer, RenderConfig, RenderRequest, RequestKind, Tile};

use super::document::OcrOptions;
use crate::{DocId, Session};

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
    /// The frames drawn around text on the page (view space): a game's header box, a chapter
    /// title's. A page read with OCR has no other layout to tell a heading by.
    pub frames: Vec<ViewRect>,
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

/// Share of a diagram's longer side painted out around it, and the least margin in pixels: a
/// scan's frame sticks out a pixel or two past the detected board, and a frame side left on the
/// page reads as one glyph as tall as the board, which joins every line beside it into one.
pub(crate) const MASK_PAD_SHARE: f32 = 0.015;
pub(crate) const MASK_MIN_PAD: u32 = 4;

/// `image` with `rects` painted white, each with a margin ([`MASK_PAD_SHARE`]).
pub(crate) fn masked(image: &GrayImage, rects: &[PixelRect]) -> Option<GrayImage> {
    let (w, h) = (image.width(), image.height());
    let mut data = image.data().to_vec();
    for r in rects {
        let side = r.x1.saturating_sub(r.x0).max(r.y1.saturating_sub(r.y0));
        // Finite and non-negative: `as` saturates.
        let pad = ((side as f32 * MASK_PAD_SHARE).ceil() as u32).max(MASK_MIN_PAD);
        let r = PixelRect { x0: r.x0.saturating_sub(pad), y0: r.y0.saturating_sub(pad), x1: r.x1.saturating_add(pad), y1: r.y1.saturating_add(pad) };
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
    // A frame that touches a diagram is the board's border the mask missed: it goes, with what it
    // holds (pieces the mask missed too, which a frame no longer hides as its inner detail). The
    // other frames are around text: a game's header box, a chapter title's.
    let (boards, frames): (Vec<PixelRect>, Vec<PixelRect>) = pdfcraft_chessvision::glyphs::frames(&image, None, scale)
        .map_err(|e| e.to_string())?
        .into_iter()
        .partition(|f| diagrams.iter().any(|d| f.x0 < d.x1 && d.x0 < f.x1 && f.y0 < d.y1 && d.y0 < f.y1));
    let image = if boards.is_empty() { image } else { masked(&image, &boards).ok_or("the page image could not be prepared")? };
    let lines = read(&image, None, scale, glyphs, &GlyphOptions::default()).map_err(|e| e.to_string())?;
    let frames: Vec<ViewRect> = frames.into_iter().filter_map(|r| pixels_to_view(r, dpi)).collect();
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
        frames,
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

// ─── An area of a page (PGN toolbar T9) ─────────────────────────────────────────────────────

/// Most pixels along either side of an area's render: the renderer's tile limit. A larger area is
/// read at a lower resolution.
pub const MAX_AREA_SIDE: u32 = 4096;
/// The lowest resolution an area is read at: below it the glyph reader's cells are mush, and the
/// area is refused as too large.
pub const MIN_AREA_DPI: f32 = 100.0;
/// The smallest side of an area, in points: a click or a sliver reads nothing.
pub const MIN_AREA_SIDE: f32 = 4.0;

/// Why an area could not be read.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum AreaError {
    #[error("no such document")]
    NoDocument,
    #[error("the document has no page {0}")]
    NoPage(usize),
    #[error("the area is not a rectangle on the page, or too thin to read")]
    Degenerate,
    #[error("the area is too large to read at once")]
    TooLarge,
    #[error("the document's permissions don't allow copying its content")]
    NotPermitted,
    #[error("chess OCR needs the chess-glyphs model: set PDFCRAFT_MODELS to the folder that holds chess-glyphs.onnx")]
    NoModel,
    #[error("the chess-glyphs model could not be loaded: {0}")]
    BadModel(String),
    #[error("the area could not be rendered: {0}")]
    Render(String),
    #[error("the area could not be read: {0}")]
    Read(String),
}

/// A glyph's top readings, kept for review (PGN toolbar T10).
#[derive(Clone, Debug, PartialEq)]
pub struct GlyphAlternatives {
    /// The glyph's box in view space.
    pub rect: [f32; 4],
    /// The reading taken (after the corrections, the cipher and the repair).
    pub text: String,
    /// `1 − p2 / p1` of the model's top two: 0 is a tie, 1 is certain.
    pub margin: f32,
    /// The model's top readings with their probabilities, most likely first (at most three).
    pub choices: Vec<(String, f32)>,
}

/// What chess OCR read in an area of a page.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AreaRead {
    /// The page, 0-based.
    pub page: usize,
    /// The area as asked for, in user space.
    pub rect_user: [f32; 4],
    /// The area as read (clamped to the page), in view space.
    pub rect_view: [f32; 4],
    /// The resolution it was read at.
    pub dpi: f32,
    /// What was read as a text layer of the page, in view space and reading order: a glyph per
    /// glyph read, a general OCR word's box shared among its characters, its lines and word gaps
    /// as the reader cut them. The capture reads it as it reads a selection, without weight
    /// (`capture::CaptureText::Read`).
    pub text: PageText,
    /// The top readings of every glyph the glyph reader's words kept.
    pub alternatives: Vec<GlyphAlternatives>,
    pub lines: usize,
    /// Words the cipher or the repair changed, and words the repair could not settle.
    pub edits: usize,
    pub flags: usize,
    /// Whether the general OCR read the prose (otherwise the glyph reader read everything).
    pub general_ocr: bool,
}

impl AreaRead {
    /// The text read as chess text in `lang`, as the capture reads it.
    pub fn chess_text(&self, lang: Lang) -> ChessPageText {
        pdfcraft_chessvision::doc::fragment::text_of_page(&self.text, Some(lang))
    }

    /// The text read, a line per line.
    pub fn plain_text(&self) -> String {
        self.text.text_of(0..self.text.glyphs.len())
    }
}

/// An area of a page to read with chess OCR, detached from the session
/// ([`Session::chess_ocr_area_job`]): it runs anywhere, a worker thread on the desktop or inline
/// on the web.
#[derive(Clone)]
pub struct OcrAreaJob {
    bytes: Arc<Vec<u8>>,
    password: Option<String>,
    page: usize,
    rect_user: PageRect,
    /// The area in view space, clamped to the page.
    view: ViewRect,
}

/// The area `rect` (user space) of a page with `geometry`, in view space and clamped to the page;
/// `None` when a coordinate is not finite, the corners are not lower-left then upper-right, or
/// less than [`MIN_AREA_SIDE`] of it is on the page either way.
fn area_view(geometry: &PageGeometry, rect: PageRect) -> Option<ViewRect> {
    let corners = [rect.x0, rect.y0, rect.x1, rect.y1];
    if !(corners.iter().all(|v| v.is_finite()) && rect.x1 > rect.x0 && rect.y1 > rect.y0) {
        return None;
    }
    let v = geometry.user_to_view(rect)?;
    let (w, h) = geometry.view_size();
    // `max` then `min`, never `clamp`: no bound can make it panic.
    let x = |v: f32| v.max(0.0).min(w);
    let y = |v: f32| v.max(0.0).min(h);
    let c = ViewRect::new(x(v.x0), y(v.y0), x(v.x1), y(v.y1));
    (c.width() >= MIN_AREA_SIDE && c.height() >= MIN_AREA_SIDE).then_some(c)
}

impl Session {
    /// Chess OCR of an area, the job: area `rect_user` (user space) of page `page` (0-based) of
    /// document `id`. Fails when the document or the page does not exist, its permissions do not
    /// allow copying its content (reading it is extraction), or the area is not a rectangle with
    /// at least [`MIN_AREA_SIDE`] points of it on the page each way.
    pub fn chess_ocr_area_job(&self, id: DocId, page: usize, rect_user: PageRect) -> Result<OcrAreaJob, AreaError> {
        let doc = self.get(id).ok_or(AreaError::NoDocument)?;
        if doc.check_copying().is_err() {
            return Err(AreaError::NotPermitted);
        }
        let info = doc.info.pages.get(page).ok_or(AreaError::NoPage(page))?;
        let geometry = super::geometry(info).ok_or(AreaError::Degenerate)?;
        let view = area_view(&geometry, rect_user).ok_or(AreaError::Degenerate)?;
        Ok(OcrAreaJob { bytes: doc.bytes.clone(), password: doc.password.clone(), page, rect_user, view })
    }

    /// Read area `rect_user` of page `page` of document `id` with chess OCR, as
    /// [`OcrAreaJob::run`] does.
    pub fn chess_read_area(
        &self,
        id: DocId,
        page: usize,
        rect_user: PageRect,
        options: &OcrOptions<'_>,
        lang: Lang,
        models_dir: Option<&Path>,
    ) -> Result<AreaRead, AreaError> {
        self.chess_ocr_area_job(id, page, rect_user)?.run(options, lang, models_dir)
    }

    /// The verdict on page `page`'s text layer `text` (as the view extracted it) for chess OCR,
    /// as the Text tab judges it, another program's invisible OCR over a picture of the page
    /// included. `None` when the document or the page does not exist.
    pub fn chess_layer_quality(&self, id: DocId, page: usize, text: &PageText) -> Option<TextQuality> {
        let doc = self.get(id)?;
        let info = doc.info.pages.get(page)?;
        let mut chess = chess_text::from_page_text(text, None);
        if chess.hidden > chess_text::quality::MAX_HIDDEN_RATIO && crate::scan_fields::is_scan(doc, page, info) {
            chess.over_page_image();
        }
        Some(chess.quality)
    }
}

impl OcrAreaJob {
    /// The page, 0-based.
    pub fn page(&self) -> usize {
        self.page
    }

    /// The area as read (clamped to the page), in view space.
    pub fn rect_view(&self) -> [f32; 4] {
        [self.view.x0, self.view.y0, self.view.x1, self.view.y1]
    }

    /// Read the area: with `options.glyphs`, else the user's `chess-glyphs` model (from
    /// `models_dir`, or where [`super::models`] looks), with the general OCR for prose when
    /// `options.general` and its models are installed, and the book's cipher `options.book`.
    /// `lang` is the language of the piece letters. Without a glyph model it is
    /// [`AreaError::NoModel`]; a blank area reads as no glyphs. `options.mode` is not used.
    pub fn run(&self, options: &OcrOptions<'_>, lang: Lang, models_dir: Option<&Path>) -> Result<AreaRead, AreaError> {
        let models;
        let glyphs: &dyn GlyphClassifier = match options.glyphs {
            Some(g) => g,
            None => {
                models = super::models(models_dir);
                match &models.glyphs {
                    super::ModelState::Loaded(m) => m,
                    super::ModelState::Missing => return Err(AreaError::NoModel),
                    super::ModelState::Invalid(why) => return Err(AreaError::BadModel(why.clone())),
                }
            }
        };
        // A last-resort guard (AGENTS.md §4): every step returns values for any input, but this
        // is untrusted-document code, often on a worker thread.
        crate::guard(|| self.read_with(glyphs, options.general, options.book, lang))
            .unwrap_or_else(|e| Err(AreaError::Read(format!("reading the area failed unexpectedly ({e})"))))
    }

    fn read_with(&self, glyphs: &dyn GlyphClassifier, general: bool, book: Option<&BookCipher>, lang: Lang) -> Result<AreaRead, AreaError> {
        let (gray, tile, dpi) = self.render()?;
        let scale = text_scale(&gray, &[]).map_err(|e| AreaError::Read(e.to_string()))?;
        let lines = read(&gray, None, scale, glyphs, &GlyphOptions::default()).map_err(|e| AreaError::Read(e.to_string()))?;
        // The general OCR failing is not the area failing: the glyph reader reads the prose too.
        let words = if general { general_ocr_words(&gray).and_then(Result::ok) } else { None };
        let fused = match &words {
            Some(w) => bucket(&lines, w),
            None => glyphs_only(&lines),
        };
        let proven = book.map(BookCipher::proven).unwrap_or_default();
        let mut repaired = repair_lines(&fused, None, lang, &proven);
        // The tile's pixels as the whole page's at `dpi`, so the boxes convert to view space.
        shift_lines(&mut repaired.lines, tile.x, tile.y);
        let text = to_page_text(&repaired.lines, dpi);
        let alternatives = repaired
            .lines
            .iter()
            .flat_map(|l| &l.words)
            .flat_map(|w| &w.glyphs)
            .filter_map(|g| {
                let v = pixels_to_view(g.rect, dpi)?;
                Some(GlyphAlternatives {
                    rect: [v.x0, v.y0, v.x1, v.y1],
                    text: g.text.clone(),
                    margin: g.margin,
                    choices: g.alternatives.iter().map(|a| (a.text.clone(), a.p)).collect(),
                })
            })
            .collect();
        let r = self.rect_user;
        Ok(AreaRead {
            page: self.page,
            rect_user: [r.x0, r.y0, r.x1, r.y1],
            rect_view: self.rect_view(),
            dpi,
            text,
            alternatives,
            lines: repaired.lines.len(),
            edits: repaired.edits.len(),
            flags: repaired.flags.len(),
            general_ocr: words.is_some(),
        })
    }

    /// The area rendered to gray, the render tile it is, and its resolution: [`OCR_DPI`], or less
    /// so that neither side passes [`MAX_AREA_SIDE`].
    fn render(&self) -> Result<(GrayImage, Tile, f32), AreaError> {
        let side = self.view.width().max(self.view.height());
        let mut scale = OCR_DPI / 72.0;
        if side * scale > MAX_AREA_SIDE as f32 {
            scale = MAX_AREA_SIDE as f32 / side;
        }
        let dpi = scale * 72.0;
        if !(dpi.is_finite() && dpi >= MIN_AREA_DPI) {
            return Err(AreaError::TooLarge);
        }
        // Finite and clamped at 0 before the cast; `as` saturates at u32::MAX.
        let px = |v: f32, round: fn(f32) -> f32| {
            let p = round(v * scale);
            if p.is_finite() { p.max(0.0) as u32 } else { 0 }
        };
        let (x0, y0) = (px(self.view.x0, f32::floor), px(self.view.y0, f32::floor));
        let (x1, y1) = (px(self.view.x1, f32::ceil), px(self.view.y1, f32::ceil));
        let tile = Tile { x: x0, y: y0, w: x1.saturating_sub(x0).clamp(1, MAX_AREA_SIDE), h: y1.saturating_sub(y0).clamp(1, MAX_AREA_SIDE) };
        let gray = render_gray(self.bytes.clone(), self.password.as_deref(), self.page, tile, scale)?;
        Ok((gray, tile, dpi))
    }
}

/// Tile `tile` of page `page` of the document `bytes` (opened with `password`) at `scale`, in gray.
fn render_gray(bytes: Arc<Vec<u8>>, password: Option<&str>, page: usize, tile: Tile, scale: f32) -> Result<GrayImage, AreaError> {
    let config = RenderConfig { password: password.map(Arc::from), ..Default::default() };
    let mut renderer = PageRenderer::new(bytes, config);
    let shot = renderer.render(RenderRequest { page, kind: RequestKind::Pixels, tile: Some(tile), scale, ..Default::default() });
    if let Some(e) = shot.error {
        return Err(AreaError::Render(e));
    }
    super::to_gray(&shot.rgba, shot.width, shot.height).ok_or_else(|| AreaError::Render("the area's raster is malformed".into()))
}

/// Every box of `lines` moved right by `dx` and down by `dy` pixels (saturating).
fn shift_lines(lines: &mut [FusedLine], dx: u32, dy: u32) {
    let shift = |r: &mut PixelRect| {
        *r = PixelRect { x0: r.x0.saturating_add(dx), y0: r.y0.saturating_add(dy), x1: r.x1.saturating_add(dx), y1: r.y1.saturating_add(dy) };
    };
    for line in lines {
        shift(&mut line.rect);
        for word in &mut line.words {
            shift(&mut word.rect);
            for g in &mut word.glyphs {
                shift(&mut g.rect);
            }
        }
    }
}

// ─── A character's sample for retraining (PGN toolbar T11) ──────────────────────────────────

/// The side of a character sample, in pixels: the 32 × 32 gray pictures the glyph model is trained
/// on (ChessVisionOFF's `training_data`).
pub const SAMPLE_SIDE: u32 = 32;
/// The largest box a sample is made of, in points: a character is a few points, more is a word.
pub const MAX_SAMPLE_BOX: f32 = 72.0;

impl Session {
    /// A character read by chess OCR in an area, as a training sample (PGN toolbar T11): its box
    /// `rect_view` (view space) of page `page` (0-based) of document `id`, rendered at `dpi` (the
    /// area's resolution, kept between [`MIN_AREA_DPI`] and [`OCR_DPI`]), in gray, padded with
    /// white to a square around it and scaled to [`SAMPLE_SIDE`] pixels a side, as a PNG. Refused
    /// as an area is: no such document or page, copying not allowed, and a box that is not
    /// finite, is empty or is larger than [`MAX_SAMPLE_BOX`].
    pub fn chess_glyph_sample(&self, id: DocId, page: usize, rect_view: [f32; 4], dpi: f32) -> Result<Vec<u8>, AreaError> {
        let doc = self.get(id).ok_or(AreaError::NoDocument)?;
        if doc.check_copying().is_err() {
            return Err(AreaError::NotPermitted);
        }
        let info = doc.info.pages.get(page).ok_or(AreaError::NoPage(page))?;
        let geometry = super::geometry(info).ok_or(AreaError::Degenerate)?;
        let [x0, y0, x1, y1] = rect_view;
        let (w, h) = geometry.view_size();
        if !(rect_view.iter().all(|v| v.is_finite()) && x1 > x0 && y1 > y0 && x1 - x0 <= MAX_SAMPLE_BOX && y1 - y0 <= MAX_SAMPLE_BOX) {
            return Err(AreaError::Degenerate);
        }
        // `max` then `min`, never `clamp`: no bound can make it panic.
        let view = ViewRect::new(x0.max(0.0).min(w), y0.max(0.0).min(h), x1.max(0.0).min(w), y1.max(0.0).min(h));
        if !(view.width() > 0.0 && view.height() > 0.0) {
            return Err(AreaError::Degenerate);
        }
        // Constant bounds in order (100 < 300) and a finite value: `clamp` cannot panic.
        let dpi = if dpi.is_finite() { dpi.clamp(MIN_AREA_DPI, OCR_DPI) } else { OCR_DPI };
        let scale = dpi / 72.0;
        // Finite and at most 72 pt × 300 dpi / 72 = 300 px a side, so the casts are exact.
        let px = |v: f32, round: fn(f32) -> f32| round(v * scale).max(0.0) as u32;
        let (tx, ty) = (px(view.x0, f32::floor), px(view.y0, f32::floor));
        let tile = Tile { x: tx, y: ty, w: px(view.x1, f32::ceil).saturating_sub(tx).max(1), h: px(view.y1, f32::ceil).saturating_sub(ty).max(1) };
        let gray = render_gray(doc.bytes.clone(), doc.password.as_deref(), page, tile, scale)?;
        sample_png(&gray).ok_or_else(|| AreaError::Render("the character's sample could not be made".into()))
    }
}

/// `gray` padded with white to a square around it, scaled to [`SAMPLE_SIDE`] pixels a side and
/// encoded as a gray PNG.
fn sample_png(gray: &GrayImage) -> Option<Vec<u8>> {
    let (w, h) = (gray.width(), gray.height());
    let side = w.max(h);
    if side == 0 || side > MAX_AREA_SIDE {
        return None;
    }
    let mut square = image::GrayImage::from_pixel(side, side, image::Luma([255]));
    let (dx, dy) = ((side - w) / 2, (side - h) / 2);
    for y in 0..h {
        for x in 0..w {
            let at = usize::try_from(u64::from(y) * u64::from(w) + u64::from(x)).ok()?;
            square.put_pixel(x + dx, y + dy, image::Luma([*gray.data().get(at)?]));
        }
    }
    let small = image::imageops::resize(&square, SAMPLE_SIDE, SAMPLE_SIDE, image::imageops::FilterType::Triangle);
    let mut png = Vec::new();
    image::ImageEncoder::write_image(
        image::codecs::png::PngEncoder::new(&mut png),
        small.as_raw(),
        SAMPLE_SIDE,
        SAMPLE_SIDE,
        image::ExtendedColorType::L8,
    )
    .ok()?;
    Some(png)
}
