//! Classifying boxes: the 32 × 32 cell, batched calls to a [`GlyphClassifier`], and the two
//! arbiters that let the classifier decide segmentation (glued glyphs split, broken ones merged).

use std::collections::HashMap;

use super::seg::{Bin, Bx, Gray, valley};
use super::{GlyphClassifier, GlyphError};
use crate::models::{GLYPH_SIDE, MAX_BATCH};

/// Values in one cell.
pub(crate) const CELL_VALUES: usize = GLYPH_SIDE * GLYPH_SIDE;
/// Least confidence gain for an arbiter to change the segmentation (the trunk's `colados`).
const ARBITER_GAIN: f32 = 0.05;
/// Glued boxes at least this wide, in scales, are tried as two glyphs.
const GLUED_MIN_WIDTH: f64 = 1.35;
/// A broken glyph's union is at most this tall and this wide, in scales.
const BROKEN_MAX_HEIGHT: f64 = 1.5;
const BROKEN_MAX_WIDTH: f64 = 1.6;
/// Neighbours this close, in scales, are also tried as one broken letter: a scan's hairlines lost
/// to the threshold cut `d` into `(` and `1`, `b` into `l` and `)`, `o` into `(` and `)`, about
/// 0.05 scales apart, where two letters of a word stand 0.1 or more apart.
const BROKEN_MAX_GAP: f64 = 0.07;
/// A letter rejoined across a gap is at most this wide, in scales: one letter, not two.
const BROKEN_LETTER_WIDTH: f64 = 0.9;
/// Least share of the shorter part's height the two parts of a letter share: the same line.
const BROKEN_MIN_ROWS: f64 = 0.5;
/// A `(` and a `)` side by side this close, in scales, are a round letter cut in two: a scan that
/// lost the hairlines at the top and bottom of an `o` or a `0` leaves its two arcs 0.03-0.17
/// scales apart (Pinski 2005), each read as a bracket at full confidence. Print never sets an
/// empty pair of brackets.
const BRACKETS_MAX_GAP: f64 = 0.25;
/// Least confidence for the union of two arcs to be taken as the letter it reads as.
const BRACKETS_MIN_P: f32 = 0.5;
/// A fragment of its neighbour's letter lies in at least this share of its own columns over the
/// neighbour's, is at most this share of the neighbour's height, and stays within the
/// neighbour's rows give or take this many scales: the ear of a `g` that a scan cut off (Pinski
/// 2005: 10 of its 11 columns over the `g`'s, half its height, read `y` at 0.96, so `g5` read
/// `gy5`), a piece of an `e`'s eye (read `)`). A letter of its own stands beside its neighbour,
/// not over it.
const FRAGMENT_MIN_COLUMNS: f64 = 0.75;
const FRAGMENT_MAX_HEIGHT: f64 = 0.7;
const FRAGMENT_ROW_SLACK: f64 = 0.1;

/// One classified box: the chosen class, its probability and the whole row.
#[derive(Clone, Debug)]
pub(crate) struct Read {
    pub class: usize,
    pub p: f32,
    pub probs: Vec<f32>,
}

impl Read {
    /// Choose `class` instead, with its own probability.
    pub fn set(&mut self, class: usize) {
        self.class = class;
        self.p = self.probs.get(class).copied().unwrap_or(0.0);
    }
}

/// The classifier's class names with a lookup by name.
pub(crate) struct Classes<'a> {
    pub names: &'a [String],
    index: HashMap<&'a str, usize>,
}

impl<'a> Classes<'a> {
    pub fn new(names: &'a [String]) -> Self {
        let mut index = HashMap::with_capacity(names.len());
        for (i, n) in names.iter().enumerate() {
            index.entry(n.as_str()).or_insert(i);
        }
        Classes { names, index }
    }

    pub fn of(&self, name: &str) -> Option<usize> {
        self.index.get(name).copied()
    }

    /// The class's text, without the stray whitespace some class names carry.
    pub fn text(&self, class: usize) -> &str {
        self.names.get(class).map_or("", |s| s.trim())
    }
}

/// OpenCV's `resize(INTER_LINEAR)` of the gray crop `b` to 32 × 32, rounded to 8 bits like
/// OpenCV on a `uint8` image, as values in 0..1 (dark ink on light paper). Outside the image
/// is paper.
pub(crate) fn cell(g: &Gray, b: &Bx) -> Vec<f32> {
    let n = GLYPH_SIDE;
    let bw = usize::try_from(b.w().max(1)).unwrap_or(1);
    let bh = usize::try_from(b.h().max(1)).unwrap_or(1);
    let (sx, sy) = (bw as f64 / n as f64, bh as f64 / n as f64);
    let px = |x: usize, y: usize| -> f64 {
        let gx = b.x0.saturating_add(x.min(bw - 1) as i64);
        let gy = b.y0.saturating_add(y.min(bh - 1) as i64);
        match (usize::try_from(gx), usize::try_from(gy)) {
            (Ok(x), Ok(y)) => f64::from(g.at(x, y)),
            _ => 255.0,
        }
    };
    let coord = |d: usize, s: f64, len: usize| -> (usize, f64) {
        let f = (d as f64 + 0.5) * s - 0.5;
        let (i, a) = if f < 0.0 { (0.0, 0.0) } else { (f.floor(), f - f.floor()) };
        // `i` is a finite, non-negative float below `len` (at most the box size).
        let i = i as usize;
        if i + 1 >= len { (len - 1, 0.0) } else { (i, a) }
    };
    let mut out = Vec::with_capacity(CELL_VALUES);
    for y in 0..n {
        let (y0, ay) = coord(y, sy, bh);
        let y1 = (y0 + 1).min(bh - 1);
        for x in 0..n {
            let (x0, ax) = coord(x, sx, bw);
            let x1 = (x0 + 1).min(bw - 1);
            let v = (1.0 - ay) * ((1.0 - ax) * px(x0, y0) + ax * px(x1, y0)) + ay * ((1.0 - ax) * px(x0, y1) + ax * px(x1, y1));
            out.push((v.round().clamp(0.0, 255.0) as f32) / 255.0);
        }
    }
    out
}

/// Classifies boxes of one image.
pub(crate) struct Reader<'a> {
    pub model: &'a dyn GlyphClassifier,
    pub classes: Classes<'a>,
    pub gray: Gray<'a>,
}

impl Reader<'_> {
    /// One [`Read`] per box, in order. Calls the model in batches of at most its `max_batch`.
    pub fn read(&self, boxes: &[Bx]) -> Result<Vec<Read>, GlyphError> {
        let k = self.classes.names.len();
        let batch = self.model.max_batch().clamp(1, MAX_BATCH);
        let mut out = Vec::with_capacity(boxes.len());
        for chunk in boxes.chunks(batch) {
            let mut data = Vec::with_capacity(chunk.len() * CELL_VALUES);
            for b in chunk {
                data.extend(cell(&self.gray, b));
            }
            let probs = self.model.classify(&data, chunk.len())?;
            if probs.len() != chunk.len().saturating_mul(k) {
                return Err(GlyphError::Output { expected: chunk.len().saturating_mul(k), got: probs.len() });
            }
            for row in probs.chunks(k.max(1)) {
                let row: Vec<f32> = row.iter().map(|v| if v.is_finite() { *v } else { 0.0 }).collect();
                let (class, p) = row.iter().enumerate().fold((0usize, f32::MIN), |acc, (i, &v)| if v > acc.1 { (i, v) } else { acc });
                out.push(Read { class, p, probs: row });
            }
        }
        Ok(out)
    }

    /// Mean winning confidence of two parts: the trunk's arbiter.
    fn confidence(a: &Read, b: &Read) -> f32 {
        (a.p + b.p) / 2.0
    }

    /// `colados.separar(auto)`: a box at least 1.35 scales wide is cut at its ink valley when
    /// the classifier reads the halves better than the whole. One model call for all of them.
    pub fn split_glued(&self, bin: &Bin, boxes: &[Bx], scale: i64) -> Result<Vec<Bx>, GlyphError> {
        let floor = GLUED_MIN_WIDTH * scale as f64;
        let cuts: Vec<Option<i64>> =
            boxes.iter().map(|b| if (b.w() as f64) < floor { None } else { valley(bin, b).filter(|&c| c > b.x0 && c < b.x1) }).collect();
        let mut trial = Vec::new();
        for (b, c) in boxes.iter().zip(&cuts) {
            if let Some(c) = c {
                trial.extend([*b, Bx { x1: *c, ..*b }, Bx { x0: *c, ..*b }]);
            }
        }
        let reads = self.read(&trial)?;
        let mut tried = reads.chunks(3);
        let mut out = Vec::with_capacity(boxes.len());
        for (b, c) in boxes.iter().zip(&cuts) {
            let Some(c) = c else {
                out.push(*b);
                continue;
            };
            let split = match tried.next() {
                Some([whole, left, right]) => Self::confidence(left, right) > whole.p + ARBITER_GAIN,
                _ => false,
            };
            if split {
                out.extend([Bx { x1: *c, ..*b }, Bx { x0: *c, ..*b }]);
            } else {
                out.push(*b);
            }
        }
        Ok(out)
    }

    /// The opposite of [`Self::split_glued`]: neighbours in reading order merge when the
    /// classifier reads the union better than the parts. Tried for neighbours whose columns
    /// overlap (outlined figurines whose inner strokes came apart) and for neighbours on one line
    /// no more than [`BROKEN_MAX_GAP`] apart whose union is one letter wide (a scan's letters cut
    /// at their hairlines).
    pub fn merge_broken(&self, boxes: &[Bx], scale: i64) -> Result<Vec<Bx>, GlyphError> {
        let s = scale as f64;
        let mut out: Vec<Bx> = Vec::with_capacity(boxes.len());
        for b in boxes {
            if let Some(last) = out.last_mut() {
                let u = last.union(b);
                let rows = (last.y1.min(b.y1) - last.y0.max(b.y0)) as f64;
                let shorter = last.h().min(b.h()).max(1) as f64;
                let cut_letter = last.common_x(b) <= 0
                    && (-last.common_x(b) as f64) <= BROKEN_MAX_GAP * s
                    && (u.w() as f64) <= BROKEN_LETTER_WIDTH * s
                    && rows >= BROKEN_MIN_ROWS * shorter;
                let overlapping = last.common_x(b) > 0 && (u.w() as f64) <= BROKEN_MAX_WIDTH * s;
                if (overlapping || cut_letter) && (u.h() as f64) <= BROKEN_MAX_HEIGHT * s {
                    let reads = self.read(&[*last, *b, u])?;
                    if let [l, r, whole] = reads.as_slice()
                        && whole.p > Self::confidence(l, r) + ARBITER_GAIN
                    {
                        *last = u;
                        continue;
                    }
                }
            }
            out.push(*b);
        }
        Ok(out)
    }

    /// A line's classified boxes with each empty pair of brackets read again as the letter it was
    /// before the scan cut it in two (`()` for `o` or `0`, see [`BRACKETS_MAX_GAP`]): a `(` with a
    /// `)` straight after it, at most a letter wide, on the same rows, whose union the classifier
    /// reads as a letter or a digit. The arcs each read as a bracket at full confidence, so
    /// [`Reader::merge_broken`] (which asks the union to read better) keeps them apart.
    pub fn join_bracket_pairs(&self, line: Vec<Bx>, reads: Vec<Read>, scale: i64) -> Result<(Vec<Bx>, Vec<Read>), GlyphError> {
        let s = scale as f64;
        let is = |r: &Read, name: &str| self.classes.names.get(r.class).is_some_and(|n| n == name);
        let mut boxes = Vec::with_capacity(line.len());
        let mut out = Vec::with_capacity(reads.len());
        let mut i = 0;
        while let (Some(b), Some(r)) = (line.get(i), reads.get(i)) {
            if let (Some(next), Some(nr)) = (line.get(i + 1), reads.get(i + 1))
                && is(r, "(")
                && is(nr, ")")
            {
                let u = b.union(next);
                let rows = (b.y1.min(next.y1) - b.y0.max(next.y0)) as f64;
                let shorter = b.h().min(next.h()).max(1) as f64;
                let close = (-b.common_x(next) as f64) <= BRACKETS_MAX_GAP * s;
                if close
                    && (u.w() as f64) <= BROKEN_LETTER_WIDTH * s
                    && (u.h() as f64) <= BROKEN_MAX_HEIGHT * s
                    && rows >= BROKEN_MIN_ROWS * shorter
                    && let Some(whole) = self.read(&[u])?.pop()
                    && whole.p >= BRACKETS_MIN_P
                    && self.classes.names.get(whole.class).is_some_and(|n| n.chars().all(char::is_alphanumeric) && !n.is_empty())
                {
                    boxes.push(u);
                    out.push(whole);
                    i += 2;
                    continue;
                }
            }
            boxes.push(*b);
            out.push(r.clone());
            i += 1;
        }
        Ok((boxes, out))
    }

    /// A line's classified boxes with each fragment of a neighbour's letter ([`FRAGMENT_MIN_COLUMNS`])
    /// joined to it, when their union reads as a letter or a digit at [`BRACKETS_MIN_P`] or more:
    /// the union is then read as the one glyph. Both parts read at full confidence (the `g` and
    /// its ear as `y`), so [`Reader::merge_broken`], which asks the union to read better than the
    /// parts, keeps them apart.
    pub fn join_fragments(&self, line: Vec<Bx>, reads: Vec<Read>, scale: i64) -> Result<(Vec<Bx>, Vec<Read>), GlyphError> {
        let slack = FRAGMENT_ROW_SLACK * scale as f64;
        let fragment_of = |f: &Bx, host: &Bx| {
            f.w() < host.w()
                && (f.h() as f64) <= FRAGMENT_MAX_HEIGHT * host.h() as f64
                && (f.common_x(host) as f64) >= FRAGMENT_MIN_COLUMNS * f.w() as f64
                && (f.y0 as f64) >= host.y0 as f64 - slack
                && (f.y1 as f64) <= host.y1 as f64 + slack
        };
        let mut boxes = Vec::with_capacity(line.len());
        let mut out = Vec::with_capacity(reads.len());
        let mut i = 0;
        while let (Some(b), Some(r)) = (line.get(i), reads.get(i)) {
            if let Some(next) = line.get(i + 1)
                && (fragment_of(next, b) || fragment_of(b, next))
                && let Some(whole) = self.read(&[b.union(next)])?.pop()
                && whole.p >= BRACKETS_MIN_P
                && self.classes.names.get(whole.class).is_some_and(|n| n.chars().all(char::is_alphanumeric) && !n.is_empty())
            {
                boxes.push(b.union(next));
                out.push(whole);
                i += 2;
                continue;
            }
            boxes.push(*b);
            out.push(r.clone());
            i += 1;
        }
        Ok((boxes, out))
    }
}
