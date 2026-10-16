//! Fusion of two readings of one line: a general OCR's words (`ocrs`, which reads prose well and
//! never reads a figurine) and the glyph reader's (which reads notation well).
//!
//! The rule measured in the S13a spike (CER 1.02% against 1.59% for the glyph reader alone and
//! 3.79% for `ocrs` alone, on the Caïssa benchmark):
//!
//! - words of both readers whose columns overlap form a **group**;
//! - a group is **notation** when one of its glyph words holds a figurine or another symbol, a
//!   move, a move number, or only marks (`!?`, `+-`); notation takes the glyph reading, the rest
//!   the OCR reading;
//! - a number both read alike but split differently (`1 984` against `1984`) takes the OCR's
//!   spacing;
//! - a prose word the OCR could only read with `?` for a letter outside its alphabet (`H?bner`)
//!   takes the glyph reading when that has an accented letter there.
//!
//! The general OCR is not a dependency: its words come in as plain [`OcrWord`]s.

use std::sync::Arc;

use pdfcraft_chess::san::Lang;
use pdfcraft_render::text::{PageText, TextGlyph};

use super::notation::is_notation;
use super::{Glyph, GlyphLine, GlyphWord, union_rect};
use crate::chess_text::{ChessPageText, from_page_text};
use crate::coords::{PixelRect, pixels_to_view};

/// A word from the general OCR: its text and its box in the same pixels as the glyph reader's.
#[derive(Clone, Debug, PartialEq)]
pub struct OcrWord {
    pub text: String,
    pub rect: PixelRect,
}

/// Which reader a fused word comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WordSource {
    Glyphs,
    Ocr,
}

/// A word of the fused line.
#[derive(Clone, Debug, PartialEq)]
pub struct FusedWord {
    pub text: String,
    pub rect: PixelRect,
    pub source: WordSource,
    /// The glyphs, with their alternatives, when the glyph reader's word was taken.
    pub glyphs: Vec<Glyph>,
    /// What the other reader read over the same columns (its whole group, words joined by
    /// spaces), for review; `None` when it read nothing there.
    pub other: Option<String>,
}

/// A recognised line.
#[derive(Clone, Debug, PartialEq)]
pub struct FusedLine {
    pub words: Vec<FusedWord>,
    pub rect: PixelRect,
}

impl FusedLine {
    /// The line's words joined by spaces.
    pub fn text(&self) -> String {
        self.words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ")
    }
}

#[derive(Clone, Copy)]
enum Item<'a> {
    Ocr(&'a OcrWord),
    Glyph(&'a GlyphWord),
}

impl Item<'_> {
    fn rect(&self) -> PixelRect {
        match self {
            Item::Ocr(w) => w.rect,
            Item::Glyph(w) => w.rect,
        }
    }
}

fn squash<'a>(words: impl Iterator<Item = &'a str>) -> String {
    words.flat_map(str::chars).filter(|c| !c.is_whitespace()).collect()
}

/// An OCR word that stands `?` for a letter (`H?bner`), not a question mark.
fn unknown_letter(word: &str) -> bool {
    let c: Vec<char> = word.chars().collect();
    c.windows(3).any(|w| matches!(w, [a, '?', b] if a.is_alphabetic() && b.is_alphabetic()))
}

/// Fuse one line's readings. `glyph_lines` are the glyph reader's lines of the same strip (it
/// may break one strip into several); their words are taken together.
pub fn fuse_line(ocr: &[OcrWord], glyph_lines: &[GlyphLine]) -> FusedLine {
    let mut items: Vec<Item> = ocr.iter().map(Item::Ocr).collect();
    items.extend(glyph_lines.iter().flat_map(|l| &l.words).map(Item::Glyph));
    items.sort_by_key(|i| (i.rect().x0, i.rect().x1));
    let mut groups: Vec<Vec<Item>> = Vec::new();
    let mut reach = 0u32;
    for it in items {
        let r = it.rect();
        match groups.last_mut() {
            Some(g) if r.x0 < reach => {
                g.push(it);
                reach = reach.max(r.x1);
            }
            _ => {
                groups.push(vec![it]);
                reach = r.x1;
            }
        }
    }
    let mut words = Vec::new();
    for g in groups {
        let glyph: Vec<&GlyphWord> = g.iter().filter_map(|i| if let Item::Glyph(w) = i { Some(*w) } else { None }).collect();
        let ocr: Vec<&OcrWord> = g.iter().filter_map(|i| if let Item::Ocr(w) = i { Some(*w) } else { None }).collect();
        let glyph_text = || glyph.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
        let ocr_text = || ocr.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
        // Same characters, different spacing: the OCR places spaces better in a split number.
        let digits = glyph.iter().all(|w| w.text.chars().all(|c| c.is_ascii_digit() || ".,;:()".contains(c)));
        let same = ocr.len() == 1 && digits && squash(glyph.iter().map(|w| w.text.as_str())) == squash(ocr.iter().map(|w| w.text.as_str()));
        let notation = glyph.iter().any(|w| is_notation(&w.text));
        let accent = ocr.iter().any(|w| unknown_letter(&w.text)) && glyph.iter().any(|w| w.text.chars().any(|c| c.is_alphabetic() && !c.is_ascii()));
        let take_glyph = !glyph.is_empty() && !same && (ocr.is_empty() || notation || accent);
        if take_glyph {
            let other = (!ocr.is_empty()).then(ocr_text);
            words.extend(glyph.iter().map(|w| FusedWord {
                text: w.text.clone(),
                rect: w.rect,
                source: WordSource::Glyphs,
                glyphs: w.glyphs.clone(),
                other: other.clone(),
            }));
        } else {
            let other = (!glyph.is_empty()).then(glyph_text);
            words.extend(ocr.iter().map(|w| FusedWord {
                text: w.text.clone(),
                rect: w.rect,
                source: WordSource::Ocr,
                glyphs: Vec::new(),
                other: other.clone(),
            }));
        }
    }
    let rect = words.iter().map(|w| w.rect).reduce(union_rect).unwrap_or_default();
    FusedLine { words, rect }
}

/// The glyph reader's lines alone, as fused lines (no general OCR available).
pub fn glyphs_only(glyph_lines: &[GlyphLine]) -> Vec<FusedLine> {
    glyph_lines.iter().map(|l| fuse_line(&[], std::slice::from_ref(l))).collect()
}

/// A text glyph over `rect` (raster pixels at `dpi`), or `None` for a DPI that cannot convert.
fn text_glyph(text: &str, rect: PixelRect, dpi: f32) -> Option<TextGlyph> {
    let v = pixels_to_view(rect, dpi)?;
    let quad = [[v.x0, v.y1], [v.x1, v.y1], [v.x1, v.y0], [v.x0, v.y0]];
    Some(TextGlyph::new(text, [v.x0, v.y0, v.x1, v.y1], quad, [1.0, 0.0]).with_source(None::<Arc<str>>, None))
}

/// The recognised lines as a page text layer in view space, for a raster of the whole page at
/// `dpi`: one glyph per recognised glyph, and an OCR word's box shared evenly among its
/// characters. Nothing converts for a DPI that is not finite and positive.
pub fn to_page_text(lines: &[FusedLine], dpi: f32) -> PageText {
    let mut text = PageText { glyphs: Vec::new(), line_of: Vec::new(), space_before: Vec::new() };
    for (li, line) in lines.iter().enumerate() {
        let li = u32::try_from(li).unwrap_or(u32::MAX);
        for (wi, w) in line.words.iter().enumerate() {
            let mut first = wi > 0;
            let mut push = |g: Option<TextGlyph>| {
                if let Some(g) = g {
                    text.glyphs.push(g);
                    text.line_of.push(li);
                    text.space_before.push(std::mem::take(&mut first));
                }
            };
            if w.glyphs.is_empty() {
                let chars: Vec<char> = w.text.chars().filter(|c| !c.is_whitespace()).collect();
                let n = u32::try_from(chars.len()).unwrap_or(u32::MAX).max(1);
                let step = w.rect.width() / n;
                for (i, c) in chars.iter().enumerate() {
                    let i = u32::try_from(i).unwrap_or(u32::MAX);
                    let x0 = w.rect.x0.saturating_add(step.saturating_mul(i));
                    let x1 = if i.saturating_add(1) == n { w.rect.x1 } else { x0.saturating_add(step) };
                    push(text_glyph(&c.to_string(), PixelRect { x0, x1, ..w.rect }, dpi));
                }
            } else {
                for g in &w.glyphs {
                    push(text_glyph(&g.text, g.rect, dpi));
                }
            }
        }
    }
    text
}

/// The recognised lines read as chess text, exactly as a page's text layer is read
/// ([`crate::chess_text::from_page_text`]), so later steps treat both sources alike.
pub fn to_chess_text(lines: &[FusedLine], dpi: f32, lang_hint: Option<Lang>) -> ChessPageText {
    from_page_text(&to_page_text(lines, dpi), lang_hint)
}
