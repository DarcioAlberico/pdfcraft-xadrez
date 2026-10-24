//! Chess OCR of scanned text: our own line and character segmentation read by the user's glyph
//! classifier (figurines, Informator symbols, letters, ligatures), fused word by word with a
//! general OCR's reading of the prose (blueprint S13b, design (iv) of the S13a spike).
//!
//! [`read`] segments a gray raster (a page, or a strip of it) and returns [`GlyphLine`]s of
//! [`GlyphWord`]s. Each [`Glyph`] carries its box, the chosen reading, its top alternatives and
//! the margin between the first two, for the per-book cipher and the legality repair (S13c) and
//! for the review queue. The steps:
//!
//! 1. **Binarize**: Otsu, or a local mean threshold when the ink share is implausible.
//! 2. **Components** (8-connected) and the **scale**: the box height at the median of the ink
//!    mass. Pass the page's scale ([`text_scale`]) when reading strips of it.
//! 3. **Character boxes**: inner detail of outlined glyphs dropped, a floor on area, rules and
//!    bars set aside.
//! 4. **Joins**: dots and accents back on their stems, with an italic correction (the dot of an
//!    italic `i` sits right of its stem); the halves of `:` `;` `=`; the two parts of
//!    `±` `∓` `⩲` `⩱`.
//! 5. **Arbiters**: a wide box is cut at its ink valley when the classifier reads the halves
//!    better (`x` glued to a figurine); overlapping neighbours merge when it reads the union
//!    better (an outlined figurine in pieces).
//! 6. **Lines** and **cells**: 32 × 32 bilinear, as the model was trained.
//! 7. **Corrections**: `=`/`:` by proportion, `0`/`o` inside numbers and castling, the case of
//!    `cosuvwxz` by height, and a move number the segmentation split (`1 9...`) joined.
//!
//! [`fuse`] then routes each group of overlapping words: notation to the glyph reader, prose to
//! the general OCR, and [`fuse::to_chess_text`] turns the result into the same
//! [`crate::chess_text::ChessPageText`] a text layer gives.
//!
//! The classifier is anything that implements [`GlyphClassifier`]: the user's
//! [`GlyphModel`], or a stand-in in tests. Every size taken from the input is capped
//! ([`MAX_GLYPH_PIXELS`], [`MAX_COMPONENTS`]).

mod classify;
mod fix;
pub mod fuse;
mod notation;
mod seg;
mod weight;

#[cfg(test)]
mod tests;

use crate::coords::PixelRect;
use crate::models::{GlyphModel, MAX_BATCH, ModelError};
use crate::raster::GrayImage;

use classify::{Classes, Read, Reader};
use seg::{Bx, Gray};
pub use weight::{BOLD_GAP_ABOVE, BOLD_LEADING, BOLD_MAX_LINES, BOLD_MAX_SIZE, BOLD_MIN_GLYPHS, BOLD_RATIO, RUN_GAP, bolder_lines};

/// Most pixels read at once: 40 megapixels, a letter page at 600 DPI. A larger raster is
/// refused; read it in strips.
pub const MAX_GLYPH_PIXELS: usize = 40_000_000;
/// Most ink components in one [`read`] call. A page of text has a few thousand; the character
/// box rules are quadratic in them, so a noisy region is refused rather than ground through.
pub const MAX_COMPONENTS: usize = 20_000;
/// Most ink components [`text_scale`] measures.
pub const MAX_SCALE_COMPONENTS: usize = 500_000;
/// Most alternatives kept per glyph.
pub const MAX_ALTERNATIVES: usize = 3;
/// Least probability of a kept alternative.
pub const MIN_ALTERNATIVE_P: f32 = 0.02;

/// Something that classifies glyph cells: the glyph model, or a stand-in in tests.
pub trait GlyphClassifier {
    /// The character (or ligature) each output stands for, in order.
    fn classes(&self) -> &[String];

    /// The class probabilities of each of `count` cells. `cells` holds `count` cells of
    /// 32 × 32 gray values in 0..=1 (dark ink on light paper), row-major, one after the other.
    fn classify(&self, cells: &[f32], count: usize) -> Result<Vec<f32>, ModelError>;

    /// Most cells one call accepts.
    fn max_batch(&self) -> usize {
        MAX_BATCH
    }
}

impl GlyphClassifier for GlyphModel {
    fn classes(&self) -> &[String] {
        &self.classes
    }

    fn classify(&self, cells: &[f32], count: usize) -> Result<Vec<f32>, ModelError> {
        self.probabilities(cells, count)
    }
}

/// Why a raster could not be read.
#[derive(Debug, thiserror::Error)]
pub enum GlyphError {
    #[error("{pixels} pixels is more than the {max} the glyph reader takes at once")]
    TooLarge { pixels: usize, max: usize },
    #[error("the image has more than {max} ink components; it is not text")]
    TooManyComponents { max: usize },
    #[error("the glyph classifier has no classes")]
    NoClasses,
    #[error("the glyph classifier returned {got} values, expected {expected}")]
    Output { expected: usize, got: usize },
    #[error(transparent)]
    Model(#[from] ModelError),
}

/// Which corrections and arbiters run. All on by default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlyphOptions {
    /// Cut wide boxes at their ink valley when the classifier reads the halves better.
    pub split_glued: bool,
    /// Merge overlapping neighbours when the classifier reads the union better.
    pub merge_broken: bool,
    /// Decide the case of `cosuvwxz` by height.
    pub case_by_height: bool,
    /// Join a move number the segmentation split, on lines with notation.
    pub join_move_numbers: bool,
}

impl Default for GlyphOptions {
    fn default() -> Self {
        GlyphOptions { split_glued: true, merge_broken: true, case_by_height: true, join_move_numbers: true }
    }
}

/// Another reading of a glyph and its probability.
#[derive(Clone, Debug, PartialEq)]
pub struct Alternative {
    pub text: String,
    pub p: f32,
}

/// One recognised glyph.
#[derive(Clone, Debug, PartialEq)]
pub struct Glyph {
    /// Its box in the raster's pixels.
    pub rect: PixelRect,
    /// The reading, after the corrections: one character, or several for a ligature or a
    /// two-mark class (`!?`).
    pub text: String,
    /// The model's probability of that reading.
    pub p: f32,
    /// The model's top readings (at most [`MAX_ALTERNATIVES`], each at least
    /// [`MIN_ALTERNATIVE_P`]), most likely first. A correction may have chosen a reading
    /// outside them.
    pub alternatives: Vec<Alternative>,
    /// `1 − p2 / p1` of the model's top two: 0 is a tie, 1 is certain.
    pub margin: f32,
}

/// A word: glyphs closer than a space.
#[derive(Clone, Debug, PartialEq)]
pub struct GlyphWord {
    pub text: String,
    pub rect: PixelRect,
    pub glyphs: Vec<Glyph>,
}

/// A line of words, left to right.
#[derive(Clone, Debug, PartialEq)]
pub struct GlyphLine {
    pub words: Vec<GlyphWord>,
    pub rect: PixelRect,
}

/// Where among a line's glyph heights, shortest first, [`GlyphLine::height`] is taken: above the
/// x-height letters, below the brackets and the odd glyph merged with its neighbour.
pub const LINE_HEIGHT_RANK: f32 = 0.8;

impl GlyphLine {
    /// The line's words joined by spaces.
    pub fn text(&self) -> String {
        self.words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ")
    }

    /// The line's glyphs.
    pub fn glyphs(&self) -> impl Iterator<Item = &Glyph> {
        self.words.iter().flat_map(|w| &w.glyphs)
    }

    /// The line's text height: its glyphs' box height at [`LINE_HEIGHT_RANK`] of them, tallest
    /// last. That is the capitals' and ascenders' height whatever the line holds: a line of
    /// moves (digits and figurines, all capital height) and a line of prose (mostly x-height
    /// letters) of one size measure alike, and a line set larger measures larger. `None` for a
    /// line without glyphs.
    pub fn height(&self) -> Option<u32> {
        rank_height(self.glyphs())
    }
}

/// The height of `glyphs` as [`GlyphLine::height`] takes it.
pub(crate) fn rank_height<'a>(glyphs: impl Iterator<Item = &'a Glyph>) -> Option<u32> {
    let mut heights: Vec<u32> = glyphs.map(|g| g.rect.y1.saturating_sub(g.rect.y0)).filter(|&h| h > 0).collect();
    heights.sort_unstable();
    // The rank is a share in [0, 1], so the index is within the list.
    let at = (heights.len().saturating_sub(1) as f32 * LINE_HEIGHT_RANK) as usize;
    heights.get(at).copied()
}

/// The smallest box covering both.
pub(crate) fn union_rect(a: PixelRect, b: PixelRect) -> PixelRect {
    PixelRect { x0: a.x0.min(b.x0), y0: a.y0.min(b.y0), x1: a.x1.max(b.x1), y1: a.y1.max(b.y1) }
}

/// `region` clamped to the image, or `None` when nothing of it is left.
fn clamp_region(image: &GrayImage, region: Option<PixelRect>) -> Option<PixelRect> {
    let full = PixelRect { x0: 0, y0: 0, x1: image.width(), y1: image.height() };
    let r = region.unwrap_or(full);
    let r = PixelRect { x0: r.x0.min(full.x1), y0: r.y0.min(full.y1), x1: r.x1.min(full.x1), y1: r.y1.min(full.y1) };
    (r.width() > 0 && r.height() > 0).then_some(r)
}

/// The pixels of `region`, row-major, after checking the size against [`MAX_GLYPH_PIXELS`].
fn crop(image: &GrayImage, region: PixelRect) -> Result<(usize, usize, Vec<u8>), GlyphError> {
    let (w, h) = (region.width() as usize, region.height() as usize);
    let pixels = w.saturating_mul(h);
    if pixels > MAX_GLYPH_PIXELS {
        return Err(GlyphError::TooLarge { pixels, max: MAX_GLYPH_PIXELS });
    }
    let mut px = Vec::with_capacity(pixels);
    for y in region.y0..region.y1 {
        let row = image.row(y).unwrap_or(&[]);
        px.extend(row.get(region.x0 as usize..region.x1 as usize).unwrap_or(&[]));
    }
    if px.len() != pixels {
        // Unreachable for a clamped region; refuse rather than read a skewed raster.
        return Err(GlyphError::TooLarge { pixels, max: MAX_GLYPH_PIXELS });
    }
    Ok((w, h, px))
}

/// The text scale of a page: the box height at the median of its ink mass (cap height, more or
/// less), measured outside `exclude` (the page's diagrams, in the same pixels). `None` for a
/// page without ink.
pub fn text_scale(image: &GrayImage, exclude: &[PixelRect]) -> Result<Option<u32>, GlyphError> {
    let Some(region) = clamp_region(image, None) else { return Ok(None) };
    let (w, h, px) = crop(image, region)?;
    let bin = seg::binarize(&Gray { w, h, px: &px });
    let comps = seg::components(&bin, MAX_SCALE_COMPONENTS).ok_or(GlyphError::TooManyComponents { max: MAX_SCALE_COMPONENTS })?;
    let outside = |c: &seg::Comp| {
        let (cx, cy) = ((c.b.x0 + c.b.x1) / 2, (c.b.y0 + c.b.y1) / 2);
        !exclude.iter().any(|r| cx >= i64::from(r.x0) && cx < i64::from(r.x1) && cy >= i64::from(r.y0) && cy < i64::from(r.y1))
    };
    let comps: Vec<seg::Comp> = comps.into_iter().filter(outside).collect();
    let s = seg::scale(&comps, w.saturating_mul(h));
    Ok(u32::try_from(s).ok().filter(|&s| s > 0))
}

/// The frames drawn around text in `region` of `image` (all of it when `None`), in `image`'s
/// pixels: a game's header box, a chapter title's. [`read`] reads the text inside them and leaves
/// the frames out; a page read with OCR has no layout but these to tell a heading by. `scale` as
/// for [`read`].
pub fn frames(image: &GrayImage, region: Option<PixelRect>, scale: Option<u32>) -> Result<Vec<PixelRect>, GlyphError> {
    let Some(region) = clamp_region(image, region) else { return Ok(Vec::new()) };
    let (w, h, px) = crop(image, region)?;
    let bin = seg::binarize(&Gray { w, h, px: &px });
    let comps = seg::components(&bin, MAX_COMPONENTS).ok_or(GlyphError::TooManyComponents { max: MAX_COMPONENTS })?;
    let scale = scale.filter(|&s| s > 0).map_or_else(|| seg::scale(&comps, w.saturating_mul(h)), i64::from);
    if scale <= 0 {
        return Ok(Vec::new());
    }
    Ok(comps.iter().filter(|c| seg::is_frame(c, scale)).map(|c| to_image(&c.b, region)).collect())
}

/// Read the glyphs of `region` of `image` (all of it when `None`) into lines of words, with
/// boxes in `image`'s pixels. `scale` is the page's text scale ([`text_scale`]); `None` (or 0)
/// measures it from the region, which is right for a crop of one line or a whole page.
///
/// An empty or blank region reads as no lines. A region above [`MAX_GLYPH_PIXELS`] or with more
/// than [`MAX_COMPONENTS`] ink components is an error, as is a classifier that fails or answers
/// with the wrong number of values.
pub fn read(
    image: &GrayImage,
    region: Option<PixelRect>,
    scale: Option<u32>,
    model: &dyn GlyphClassifier,
    options: &GlyphOptions,
) -> Result<Vec<GlyphLine>, GlyphError> {
    let names = model.classes();
    if names.is_empty() {
        return Err(GlyphError::NoClasses);
    }
    let Some(region) = clamp_region(image, region) else { return Ok(Vec::new()) };
    let (w, h, px) = crop(image, region)?;
    let gray = Gray { w, h, px: &px };
    let bin = seg::binarize(&gray);
    let comps = seg::components(&bin, MAX_COMPONENTS).ok_or(GlyphError::TooManyComponents { max: MAX_COMPONENTS })?;
    let scale = scale.filter(|&s| s > 0).map_or_else(|| seg::scale(&comps, w.saturating_mul(h)), i64::from);
    if scale <= 0 {
        return Ok(Vec::new());
    }
    let (boxes, bars) = seg::char_boxes(&comps, scale);
    let boxes = seg::join_dots(&boxes, &bin);
    let (parts, bars) = seg::join_stacked(&boxes, &bars, scale);
    let boxes = seg::order_in_strip(&seg::join_signs(parts, &bars, scale));
    let reader = Reader { model, classes: Classes::new(names), gray };
    let boxes = if options.split_glued { reader.split_glued(&bin, &boxes, scale)? } else { boxes };
    let boxes = if options.merge_broken { reader.merge_broken(&boxes, scale)? } else { boxes };
    let mut out = Vec::new();
    for line in seg::break_lines(&seg::order_in_strip(&boxes), scale) {
        let reads = reader.read(&line)?;
        let (line, mut reads) = if options.merge_broken { reader.join_bracket_pairs(line, reads, scale)? } else { (line, reads) };
        fix::stacked(&mut reads, &line, &reader.classes);
        let ink: Vec<Bx> = line.iter().map(|b| seg::tighten(&bin, b)).collect();
        fix::square_dots(&mut reads, &ink, &reader.classes);
        if options.case_by_height {
            fix::case_by_height(&mut reads, &line, &reader.classes);
        }
        let ranges = fix::words(&line, &reads, &reader.classes);
        fix::numbers(&mut reads, &ranges, &reader.classes);
        let mut words = Vec::with_capacity(ranges.len());
        for (s, e) in ranges {
            let glyphs: Vec<Glyph> =
                line.get(s..e).unwrap_or(&[]).iter().zip(reads.get(s..e).unwrap_or(&[])).map(|(b, r)| glyph(b, r, &reader.classes, region)).collect();
            let text: String = glyphs.iter().map(|g| g.text.as_str()).collect();
            let Some(rect) = glyphs.iter().map(|g| g.rect).reduce(union_rect) else { continue };
            if !text.trim().is_empty() {
                words.push(GlyphWord { text, rect, glyphs });
            }
        }
        if options.join_move_numbers {
            fix::join_move_numbers(&mut words);
        }
        if let Some(rect) = words.iter().map(|w| w.rect).reduce(union_rect) {
            out.push(GlyphLine { words, rect });
        }
    }
    Ok(out)
}

/// A box of the region's pixels in the image's pixels.
fn to_image(b: &Bx, region: PixelRect) -> PixelRect {
    let c = |v: i64, off: u32| u32::try_from(v.max(0)).unwrap_or(u32::MAX).saturating_add(off);
    PixelRect { x0: c(b.x0, region.x0), y0: c(b.y0, region.y0), x1: c(b.x1, region.x0), y1: c(b.y1, region.y0) }
}

fn glyph(b: &Bx, r: &Read, classes: &Classes, region: PixelRect) -> Glyph {
    // The top `MAX_ALTERNATIVES` classes, most likely first.
    let mut top: Vec<(usize, f32)> = Vec::with_capacity(MAX_ALTERNATIVES + 1);
    for (i, &p) in r.probs.iter().enumerate() {
        if top.len() < MAX_ALTERNATIVES || top.last().is_some_and(|l| p > l.1) {
            let at = top.iter().position(|t| p > t.1).unwrap_or(top.len());
            top.insert(at, (i, p));
            top.truncate(MAX_ALTERNATIVES);
        }
    }
    let p1 = top.first().map_or(0.0, |t| t.1);
    let p2 = top.get(1).map_or(0.0, |t| t.1);
    let margin = if p1 > 0.0 { (1.0 - p2 / p1).clamp(0.0, 1.0) } else { 0.0 };
    let alternatives = top.iter().filter(|t| t.1 >= MIN_ALTERNATIVE_P).map(|&(i, p)| Alternative { text: classes.text(i).to_owned(), p }).collect();
    Glyph { rect: to_image(b, region), text: classes.text(r.class).to_owned(), p: r.p, alternatives, margin }
}
