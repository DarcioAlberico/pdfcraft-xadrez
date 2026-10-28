//! Lines set bolder than a scanned page's body, standing alone: a heading set at the body's size
//! in bold (Pinski's `Summary`), which neither a frame nor a larger size tells.
//!
//! The same measure tells the words set in bold ([`bold_words`]): a book's main line, in bold
//! among the analysis in regular type, which a text layer tells by its fonts.
//!
//! A line is cut into runs at gutters (lines of two columns on one baseline are one line), and
//! each run's stroke width is measured on the page's ink: twice its ink over its edge
//! pixels (ink with paper beside it), the width of a long stroke whatever its length. Bold type
//! measures 1.45 to 1.9 times the body on Pinski's scans, an italic running head 1.2.

use super::seg::{self, Gray};
use super::{GlyphError, GlyphLine, clamp_region, crop, rank_height, union_rect};
use crate::coords::PixelRect;
use crate::raster::GrayImage;

/// Words further apart than this many times the body's text height are in different columns.
pub const RUN_GAP: f32 = 2.0;
/// A run whose strokes are at least this many times as wide as the body's is set bolder.
pub const BOLD_RATIO: f32 = 1.35;
/// A run at least this many times as tall as the body is set larger, not only bolder: its
/// strokes are wider for its size (the larger lines are a title's by their size).
pub const BOLD_MAX_SIZE: f32 = 1.4;
/// Least blank space above a bolder heading, in body text heights: a blank line. A bold main
/// line inside a paragraph, or a bold player's line under the moves, stands at the leading.
pub const BOLD_GAP_ABOVE: f32 = 1.0;
/// Most space between two bolder runs of one heading, in body text heights: the leading.
pub const BOLD_LEADING: f32 = 0.8;
/// Fewest glyphs of a bolder heading (a lone bold figure or sign is no heading).
pub const BOLD_MIN_GLYPHS: usize = 3;
/// Most runs of one bolder heading; a longer bold block is a paragraph set in bold.
pub const BOLD_MAX_LINES: usize = 3;
/// Fewest glyphs of a run that stands as a line above or below a heading: a lone glyph is a mark
/// (a speck of the scan, a stray stroke between a board and its caption).
pub const MIN_LINE_GLYPHS: usize = 2;
/// Most runs on a page measured; a page with more is no page of text.
pub const MAX_RUNS: usize = 4000;

/// Part of a line between gutters.
struct Run {
    rect: PixelRect,
    glyphs: usize,
    height: Option<u32>,
    stroke: Option<f32>,
}

/// The value at the middle of the weights, `(value, weight)` pairs in any order.
fn weighted_median(mut v: Vec<(f32, usize)>) -> Option<f32> {
    v.retain(|(x, n)| x.is_finite() && *n > 0);
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    let total = v.iter().fold(0usize, |acc, &(_, n)| acc.saturating_add(n));
    let mut seen = 0usize;
    v.iter()
        .find(|&&(_, n)| {
            seen = seen.saturating_add(n);
            seen.saturating_mul(2) >= total
        })
        .map(|&(x, _)| x)
}

/// The runs of `lines`: words further apart than [`RUN_GAP`] times `body` pixels are cut apart.
fn runs(lines: &[GlyphLine], body: f32) -> Vec<Run> {
    let mut out = Vec::new();
    for line in lines {
        let mut start = 0usize;
        let words = &line.words;
        for i in 1..=words.len() {
            let cut = match (words.get(i.saturating_sub(1)), words.get(i)) {
                (Some(a), Some(b)) => b.rect.x0.saturating_sub(a.rect.x1) as f32 > RUN_GAP * body,
                _ => true,
            };
            if !cut {
                continue;
            }
            let part = words.get(start..i).unwrap_or(&[]);
            start = i;
            let Some(rect) = part.iter().map(|w| w.rect).reduce(union_rect) else { continue };
            let glyphs = part.iter().flat_map(|w| &w.glyphs);
            out.push(Run { rect, glyphs: glyphs.clone().count(), height: rank_height(glyphs), stroke: None });
        }
    }
    out
}

/// The stroke width of the ink in `rect` of `bin` (twice its ink over its edge pixels), and the
/// pixels looked at.
fn stroke(bin: &seg::Bin, rect: PixelRect) -> (Option<f32>, u64) {
    let (ink, edge, looked) = ink_edge(bin, rect);
    // Pixel counts on one image, exact enough in `f32`.
    ((edge > 0).then(|| 2.0 * ink as f32 / edge as f32), looked)
}

/// The ink pixels in `rect` of `bin`, those of them with paper beside them, and the pixels looked at.
fn ink_edge(bin: &seg::Bin, rect: PixelRect) -> (u64, u64, u64) {
    let (mut ink, mut edge) = (0u64, 0u64);
    for y in i64::from(rect.y0)..i64::from(rect.y1) {
        for x in i64::from(rect.x0)..i64::from(rect.x1) {
            if bin.at(x, y) {
                ink += 1;
                if !(bin.at(x - 1, y) && bin.at(x + 1, y) && bin.at(x, y - 1) && bin.at(x, y + 1)) {
                    edge += 1;
                }
            }
        }
    }
    let looked = u64::from(rect.width()).saturating_mul(u64::from(rect.height()));
    (ink, edge, looked)
}

/// The boxes (in `image`'s pixels) of the headings among `lines` (read from `image` with
/// [`super::read`]) set bolder than the page's body and standing alone: runs of at least
/// [`BOLD_MIN_GLYPHS`] glyphs at the body's size (under [`BOLD_MAX_SIZE`]) whose strokes are at
/// least [`BOLD_RATIO`] times the body's (the stroke most glyphs are set in), with a line above
/// them in their column at least [`BOLD_GAP_ABOVE`] text heights up and a line below them. Bolder
/// runs one under another at the leading ([`BOLD_LEADING`]) are one heading, of at most
/// [`BOLD_MAX_LINES`]. The first and the last line of a column are never one (a running head, a
/// bold line carried over from the page before), nor is a line next to one of `diagrams` (in
/// `image`'s pixels) with no line between them: a bold caption under or over a board (`Diagrama
/// 51 Posição após c5`, `Fighting for the light squares`) belongs to it. Whether its words read
/// as a heading is the caller's to judge.
pub fn bolder_lines(image: &GrayImage, lines: &[GlyphLine], diagrams: &[PixelRect]) -> Result<Vec<PixelRect>, GlyphError> {
    let heights: Vec<(f32, usize)> = lines.iter().filter_map(|l| l.height().map(|h| (h as f32, l.glyphs().count()))).collect();
    let Some(body) = weighted_median(heights).filter(|&b| b > 0.0) else { return Ok(Vec::new()) };
    let mut runs = runs(lines, body);
    if runs.is_empty() || runs.len() > MAX_RUNS {
        return Ok(Vec::new());
    }
    let Some(region) = clamp_region(image, None) else { return Ok(Vec::new()) };
    let (w, h, px) = crop(image, region)?;
    let bin = seg::binarize(&Gray { w, h, px: &px });
    // Lines may overlap a little; more than twice the page is no page of text.
    let budget = (w as u64).saturating_mul(h as u64).saturating_mul(2);
    let mut looked = 0u64;
    for run in &mut runs {
        let (s, n) = stroke(&bin, run.rect);
        looked = looked.saturating_add(n);
        if looked > budget {
            return Ok(Vec::new());
        }
        run.stroke = s;
    }
    let body_size = |r: &Run| r.height.is_some_and(|h| (h as f32) < BOLD_MAX_SIZE * body);
    let strokes: Vec<(f32, usize)> = runs.iter().filter(|r| body_size(r)).filter_map(|r| r.stroke.map(|s| (s, r.glyphs))).collect();
    let Some(body_stroke) = weighted_median(strokes).filter(|&s| s > 0.0) else { return Ok(Vec::new()) };
    let bold: Vec<bool> =
        runs.iter().map(|r| r.glyphs >= BOLD_MIN_GLYPHS && body_size(r) && r.stroke.is_some_and(|s| s >= BOLD_RATIO * body_stroke)).collect();
    let across = |a: &PixelRect, b: &PixelRect| a.x0 < b.x1 && b.x0 < a.x1;
    let mid = |r: &PixelRect| (u64::from(r.y0) + u64::from(r.y1)) / 2;
    // What stands on the page in each run's column: the runs of words (by index; a lone glyph is a
    // mark or a speck, not a line) and the diagrams.
    let items = || {
        runs.iter()
            .enumerate()
            .filter(|(_, r)| r.glyphs >= MIN_LINE_GLYPHS)
            .map(|(j, r)| (Some(j), r.rect))
            .chain(diagrams.iter().map(|&d| (None, d)))
    };
    // The nearest run above each run in its column, and the blank space between them; none when
    // that is a diagram.
    let above = |i: usize| -> Option<(usize, f32)> {
        let r = runs.get(i)?.rect;
        let (j, o) = items().filter(|(j, o)| *j != Some(i) && across(o, &r) && mid(o) < u64::from(r.y0)).max_by_key(|(_, o)| o.y1)?;
        j.map(|j| (j, r.y0.saturating_sub(o.y1) as f32))
    };
    // Whether the nearest thing below a run in its column is a run.
    let has_below = |i: usize| {
        runs.get(i).is_some_and(|r| {
            items()
                .filter(|(j, o)| *j != Some(i) && across(o, &r.rect) && mid(o) > u64::from(r.rect.y1))
                .min_by_key(|(_, o)| o.y0)
                .is_some_and(|(j, _)| j.is_some())
        })
    };
    let mut order: Vec<usize> = (0..runs.len()).filter(|&i| bold.get(i).copied().unwrap_or(false)).collect();
    order.sort_by_key(|&i| runs.get(i).map_or(0, |r| r.rect.y0));
    // Each heading's runs, and whether it still stands as one.
    let mut groups: Vec<(Vec<usize>, bool)> = Vec::new();
    let mut group_of: Vec<Option<usize>> = vec![None; runs.len()];
    for i in order {
        let Some((a, gap)) = above(i) else { continue };
        let open = group_of.get(a).copied().flatten();
        let joined = match open.and_then(|g| groups.get_mut(g)) {
            Some((members, valid)) if gap <= BOLD_LEADING * body && members.last() == Some(&a) => {
                members.push(i);
                if members.len() > BOLD_MAX_LINES {
                    *valid = false;
                }
                true
            }
            _ => false,
        };
        if joined {
            if let Some(slot) = group_of.get_mut(i) {
                *slot = open;
            }
        } else if gap >= BOLD_GAP_ABOVE * body {
            if let Some(slot) = group_of.get_mut(i) {
                *slot = Some(groups.len());
            }
            groups.push((vec![i], true));
        }
    }
    Ok(groups
        .into_iter()
        .filter(|(members, valid)| *valid && members.last().is_some_and(|&l| has_below(l)))
        .filter_map(|(members, _)| members.iter().filter_map(|&i| runs.get(i).map(|r| r.rect)).reduce(union_rect))
        .collect())
}

/// Fewest glyphs of a word measured for its weight: one glyph's strokes say too little.
pub const WORD_MIN_GLYPHS: usize = 2;
/// A word whose strokes are at least this many times as wide as the body's is set in bold. Lower
/// than [`BOLD_RATIO`]: a short bold move (`♗g7`, two letters and a digit measured) measured 1.25
/// to 1.3 times the body on Ulf the Attacker's pages rendered at 300 dpi, and the regular words at
/// most 1.15.
pub const WORD_BOLD_RATIO: f32 = 1.2;

/// Whether a glyph's strokes tell the weight of its word: letters and digits. An outline
/// figurine is drawn in thin strokes in a bold line too, and dots and signs are too small.
fn weighs(text: &str) -> bool {
    !text.is_empty() && text.chars().all(char::is_alphanumeric) && !text.chars().any(|c| ('\u{2654}'..='\u{265F}').contains(&c))
}

/// The boxes (in `image`'s pixels) of the words of `lines` (read from `image` with
/// [`super::read`]) set bolder than the page's body: words with at least [`WORD_MIN_GLYPHS`]
/// letters or digits whose strokes are at least [`WORD_BOLD_RATIO`] times the body's (the stroke most
/// of the page's measured glyphs are set in). Books print their main line in bold and the analysis
/// around it in regular type, which a text layer tells by its fonts and a scan only by its ink.
/// A page set mostly in bold has a bold body, and no word is bolder than it.
pub fn bold_words(image: &GrayImage, lines: &[GlyphLine]) -> Result<Vec<PixelRect>, GlyphError> {
    let words: Vec<&super::GlyphWord> = lines.iter().flat_map(|l| &l.words).collect();
    if words.is_empty() || words.len() > MAX_RUNS.saturating_mul(4) {
        return Ok(Vec::new());
    }
    let Some(region) = clamp_region(image, None) else { return Ok(Vec::new()) };
    let (w, h, px) = crop(image, region)?;
    let bin = seg::binarize(&Gray { w, h, px: &px });
    let budget = (w as u64).saturating_mul(h as u64).saturating_mul(2);
    let mut looked = 0u64;
    // (word's box, stroke, glyphs measured)
    let mut measured: Vec<(PixelRect, f32, usize)> = Vec::new();
    for word in words {
        let (mut ink, mut edge, mut n) = (0u64, 0u64, 0usize);
        for g in word.glyphs.iter().filter(|g| weighs(&g.text)) {
            let (i, e, l) = ink_edge(&bin, g.rect);
            looked = looked.saturating_add(l);
            if looked > budget {
                return Ok(Vec::new());
            }
            ink = ink.saturating_add(i);
            edge = edge.saturating_add(e);
            n += 1;
        }
        if n >= WORD_MIN_GLYPHS && edge > 0 {
            // Pixel counts on one image, exact enough in `f32`.
            measured.push((word.rect, 2.0 * ink as f32 / edge as f32, n));
        }
    }
    let Some(body) = weighted_median(measured.iter().map(|&(_, s, n)| (s, n)).collect()).filter(|&s| s > 0.0) else {
        return Ok(Vec::new());
    };
    Ok(measured.into_iter().filter(|&(_, s, _)| s >= WORD_BOLD_RATIO * body).map(|(r, _, _)| r).collect())
}
