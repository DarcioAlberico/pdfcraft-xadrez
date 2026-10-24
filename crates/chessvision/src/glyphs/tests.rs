//! Glyph reader and fusion tests. Glyphs are drawn here with rectangles, strokes and discs; the
//! classifier is a nearest-template stand-in built from the same drawings, whose confidence
//! falls off with the distance to its nearest template. The aim is the plumbing (segmentation,
//! arbiters, corrections, routing, limits), not accuracy.

use pdfcraft_chess::san::{Lang, San};
use proptest::prelude::*;

use super::classify::{CELL_VALUES, Classes, Read, Reader, cell};
use super::fuse::{FusedLine, FusedWord, OcrWord, WordSource, fuse_line, to_chess_text, to_page_text};
use super::seg::{self, Bin, Bx, Gray};
use super::*;
use crate::chess_text::ChessRun;

/// A white canvas to draw glyphs on.
#[derive(Clone)]
struct Canvas {
    w: usize,
    h: usize,
    px: Vec<u8>,
}

impl Canvas {
    fn new(w: usize, h: usize) -> Self {
        Canvas { w, h, px: vec![255; w * h] }
    }
    fn ink(&mut self, x: i64, y: i64) {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
            self.px[y as usize * self.w + x as usize] = 0;
        }
    }
    fn rect(&mut self, x0: i64, y0: i64, x1: i64, y1: i64) -> &mut Self {
        for y in y0..y1 {
            for x in x0..x1 {
                self.ink(x, y);
            }
        }
        self
    }
    /// A stroke of half-width `r` from `(x0, y0)` to `(x1, y1)`.
    fn stroke(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, r: f64) -> &mut Self {
        let (dx, dy) = (x1 - x0, y1 - y0);
        let len2 = (dx * dx + dy * dy).max(1e-9);
        for y in 0..self.h as i64 {
            for x in 0..self.w as i64 {
                let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
                let t = (((px - x0) * dx + (py - y0) * dy) / len2).clamp(0.0, 1.0);
                let (qx, qy) = (x0 + t * dx - px, y0 + t * dy - py);
                if qx * qx + qy * qy <= r * r {
                    self.ink(x, y);
                }
            }
        }
        self
    }
    fn disc(&mut self, cx: f64, cy: f64, r: f64) -> &mut Self {
        self.stroke(cx, cy, cx, cy, r)
    }
    fn gray(&self) -> Gray<'_> {
        Gray { w: self.w, h: self.h, px: &self.px }
    }
    fn image(&self) -> GrayImage {
        GrayImage::new(self.w as u32, self.h as u32, self.px.clone()).unwrap()
    }
    fn bin(&self) -> Bin {
        seg::binarize(&self.gray())
    }
    /// Character boxes and bars at `scale`.
    fn boxes(&self, scale: i64) -> (Vec<Bx>, Vec<Bx>) {
        let comps = seg::components(&self.bin(), MAX_COMPONENTS).unwrap();
        seg::char_boxes(&comps, scale)
    }
}

/// `n` x-height letters (14 × 20 blocks, baseline 60) from `x`, so medians are those of text.
fn letters(c: &mut Canvas, x: i64, n: i64) {
    for i in 0..n {
        c.rect(x + i * 18, 40, x + i * 18 + 14, 60);
    }
}

const SCALE: i64 = 20;

// ---------------------------------------------------------------- stand-in classifier

/// Nearest template: `p_k ∝ exp(−d_k / τ)` over the classes plus a reject mass `exp(−D0 / τ)`
/// that belongs to no class, so a cell far from every template is read with low confidence.
struct Templates {
    names: Vec<String>,
    cells: Vec<(usize, Vec<f32>)>,
    calls: std::cell::Cell<usize>,
}

impl Templates {
    fn new(names: &[&str]) -> Self {
        Templates { names: names.iter().map(|s| (*s).to_owned()).collect(), cells: Vec::new(), calls: std::cell::Cell::new(0) }
    }
    /// Learn class `name` from the ink of `c` (its tight box).
    fn learn(mut self, name: &str, c: &Canvas) -> Self {
        let bin = c.bin();
        let b = seg::tighten(&bin, &Bx { x0: 0, y0: 0, x1: c.w as i64, y1: c.h as i64 });
        let k = self.names.iter().position(|n| n == name).unwrap();
        self.cells.push((k, cell(&c.gray(), &b)));
        self
    }
}

impl GlyphClassifier for Templates {
    fn classes(&self) -> &[String] {
        &self.names
    }
    fn classify(&self, cells: &[f32], count: usize) -> Result<Vec<f32>, ModelError> {
        self.calls.set(self.calls.get() + 1);
        assert_eq!(cells.len(), count * CELL_VALUES);
        let (tau, d0) = (0.01f32, 0.03f32);
        let mut out = Vec::new();
        for c in cells.chunks(CELL_VALUES) {
            let mut best = vec![f32::INFINITY; self.names.len()];
            for (k, t) in &self.cells {
                let d = c.iter().zip(t).map(|(a, b)| (a - b) * (a - b)).sum::<f32>() / CELL_VALUES as f32;
                best[*k] = best[*k].min(d);
            }
            let e: Vec<f32> = best.iter().map(|d| (-d / tau).exp()).collect();
            let z = e.iter().sum::<f32>() + (-d0 / tau).exp();
            out.extend(e.iter().map(|v| v / z));
        }
        Ok(out)
    }
}

/// A classifier that answers with whatever it was given.
struct Fixed {
    names: Vec<String>,
    answer: fn(usize, usize) -> Vec<f32>,
}

impl GlyphClassifier for Fixed {
    fn classes(&self) -> &[String] {
        &self.names
    }
    fn classify(&self, _cells: &[f32], count: usize) -> Result<Vec<f32>, ModelError> {
        Ok((self.answer)(count, self.names.len()))
    }
}

fn uniform(count: usize, k: usize) -> Vec<f32> {
    vec![1.0 / k as f32; count * k]
}

fn names(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| (*s).to_owned()).collect()
}

// ---------------------------------------------------------------- segmentation

#[test]
fn the_dots_of_i_and_j_rejoin_their_stems() {
    let mut c = Canvas::new(260, 90);
    letters(&mut c, 10, 5);
    c.rect(100, 40, 106, 60).disc(103.0, 33.0, 3.0); // i
    c.rect(130, 40, 136, 70).disc(133.0, 33.0, 3.0); // j
    c.rect(160, 56, 164, 60); // a full stop beside the last letter: not a dot of anything
    letters(&mut c, 170, 2);
    let (boxes, _) = c.boxes(SCALE);
    assert_eq!(boxes.len(), 5 + 2 + 2 + 1 + 2);
    let joined = seg::join_dots(&boxes, &c.bin());
    assert_eq!(joined.len(), 5 + 2 + 1 + 2, "two dots absorbed, the stop kept");
    assert!(joined.iter().any(|b| b.x0 == 100 && b.y0 == 30 && b.y1 == 60), "i: {joined:?}");
    assert!(joined.iter().any(|b| b.x0 == 130 && b.y0 == 30 && b.y1 == 70), "j: {joined:?}");
    assert!(joined.contains(&Bx { x0: 160, y0: 56, x1: 164, y1: 60 }), "the stop stays alone");
}

/// The italic `i`: the dot sits right of the slanted stem's box, overlapping it by half its
/// width, under the 0.55 the upright rule asks. Projected back by the stem's slant it overlaps
/// by more, and joins. (Fails with the shift set to zero: the dot reads as a full stop.)
#[test]
fn the_dot_of_an_italic_i_rejoins_its_slanted_stem() {
    let mut c = Canvas::new(300, 90);
    letters(&mut c, 10, 5);
    // Stem from (200..206, y 60) up to (209..215, y 30): slant 0.3.
    for y in 30..60 {
        let x = 200 + ((60 - y) as f64 * 0.3).round() as i64;
        c.rect(x, y, x + 6, y + 1);
    }
    c.disc(215.0, 21.0, 3.0);
    let (boxes, _) = c.boxes(SCALE);
    let stem = *boxes.iter().find(|b| b.x0 == 200).unwrap();
    let dot = *boxes.iter().find(|b| b.y1 <= 25).unwrap();
    let plain = stem.common_x(&dot) as f64 / dot.w() as f64;
    assert!(plain < 0.55, "the upright rule alone must miss it: {plain}");
    let joined = seg::join_dots(&boxes, &c.bin());
    assert_eq!(joined.len(), boxes.len() - 1, "{joined:?}");
    assert!(joined.iter().any(|b| b.x0 == 200 && b.y0 == dot.y0), "{joined:?}");
}

#[test]
fn colon_and_equals_halves_fuse() {
    let mut c = Canvas::new(260, 90);
    letters(&mut c, 10, 5);
    c.disc(120.0, 45.0, 2.5).disc(120.0, 57.0, 2.5); // :
    c.rect(140, 46, 160, 49).rect(140, 53, 160, 56); // = (two bars, flatter than 6:1)
    let (boxes, bars) = c.boxes(SCALE);
    assert_eq!(bars.len(), 2, "the bars of = are set aside: {bars:?}");
    let (parts, left) = seg::join_stacked(&boxes, &bars, SCALE);
    assert!(left.is_empty());
    assert!(parts.iter().any(|p| p.b == Bx { x0: 118, y0: 43, x1: 122, y1: 59 } && !p.flat), "{parts:?}");
    assert!(parts.iter().any(|p| p.b == Bx { x0: 140, y0: 46, x1: 160, y1: 56 } && p.flat), "{parts:?}");
    assert_eq!(parts.len(), 5 + 2);
}

/// Draws a `+` of side `side` whose top-left corner is `(x, y)`.
fn plus(c: &mut Canvas, x: i64, y: i64, side: i64) {
    let mid = side / 2;
    c.rect(x, y + mid - 2, x + side, y + mid + 2).rect(x + mid - 2, y, x + mid + 2, y + side);
}

/// `±` `∓` `⩲` `⩱`: the `+` is too tall to be half of a stacked glyph, so the trunk dropped the
/// bar (`∓` read `+`). (Fails with `join_signs` reduced to dropping the bars.)
#[test]
fn the_two_parts_of_informator_signs_fuse() {
    let mut c = Canvas::new(420, 90);
    letters(&mut c, 10, 5);
    plus(&mut c, 110, 33, 18);
    c.rect(110, 55, 128, 57); // ±: bar under
    c.rect(150, 30, 168, 32);
    plus(&mut c, 150, 37, 18); // ∓: bar over
    plus(&mut c, 190, 34, 14);
    c.rect(190, 52, 204, 54).rect(190, 57, 204, 59); // ⩲: = under
    c.rect(230, 28, 244, 30).rect(230, 33, 244, 35);
    plus(&mut c, 230, 39, 14); // ⩱: = over
    let (boxes, bars) = c.boxes(SCALE);
    let (parts, left) = seg::join_stacked(&boxes, &bars, SCALE);
    let signs = seg::join_signs(parts, &left, SCALE);
    for want in [
        Bx { x0: 110, y0: 33, x1: 128, y1: 57 },
        Bx { x0: 150, y0: 30, x1: 168, y1: 55 },
        Bx { x0: 190, y0: 34, x1: 204, y1: 59 },
        Bx { x0: 230, y0: 28, x1: 244, y1: 53 },
    ] {
        assert!(signs.contains(&want), "{want:?} not in {signs:?}");
    }
    assert_eq!(signs.len(), 5 + 4);
}

#[test]
fn a_lone_bar_stays_out_of_the_text() {
    let mut c = Canvas::new(200, 90);
    letters(&mut c, 10, 5);
    c.rect(110, 50, 128, 52); // a rule beside the letters, over and under nothing
    let (boxes, bars) = c.boxes(SCALE);
    assert_eq!(bars.len(), 1);
    let (parts, left) = seg::join_stacked(&boxes, &bars, SCALE);
    assert_eq!(seg::join_signs(parts, &left, SCALE).len(), 5);
}

/// On a crop of one short line every letter is more than 1% of the image, and the trunk's rule
/// measured the dots that were left. (Fails without the ink-share fallback: scale 4.)
#[test]
fn the_scale_of_a_short_line_is_its_letters() {
    let mut c = Canvas::new(80, 50);
    c.rect(5, 10, 21, 40).rect(25, 10, 41, 40).rect(45, 10, 61, 40);
    c.rect(65, 36, 69, 40).rect(72, 36, 76, 40);
    let comps = seg::components(&c.bin(), MAX_COMPONENTS).unwrap();
    assert_eq!(seg::scale(&comps, c.w * c.h), 30);
}

#[test]
fn lines_break_and_order_left_to_right() {
    let mut c = Canvas::new(200, 140);
    letters(&mut c, 100, 3);
    letters(&mut c, 10, 3);
    for x in [10, 28, 46] {
        c.rect(x, 100, x + 14, 120);
    }
    let (boxes, _) = c.boxes(SCALE);
    let lines = seg::break_lines(&seg::order_in_strip(&boxes), SCALE);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].iter().map(|b| b.x0).collect::<Vec<_>>(), vec![10, 28, 46, 100, 118, 136]);
    assert!(lines[1].iter().all(|b| b.y0 == 100));
}

// ---------------------------------------------------------------- arbiters

/// A solid figurine-like blob, 20 × 24, at `x`.
fn blob(c: &mut Canvas, x: i64) {
    c.rect(x, 36, x + 20, 60).rect(x + 4, 30, x + 12, 36);
}

/// An `x` of the blob's height, 16 wide, at `x`.
fn ex(c: &mut Canvas, x: i64) {
    let x = x as f64;
    c.stroke(x + 1.5, 31.5, x + 14.5, 58.5, 1.6).stroke(x + 14.5, 31.5, x + 1.5, 58.5, 1.6);
}

fn reader<'a>(model: &'a Templates, c: &'a Canvas) -> Reader<'a> {
    Reader { model, classes: Classes::new(&model.names), gray: c.gray() }
}

fn blob_and_x() -> Templates {
    let mut b = Canvas::new(40, 90);
    blob(&mut b, 10);
    let mut x = Canvas::new(40, 90);
    ex(&mut x, 10);
    Templates::new(&["F", "x"]).learn("F", &b).learn("x", &x)
}

/// An `x` glued to the figurine before it by a hairline reads badly as one box and well as two;
/// the arbiter cuts it at the hairline. (Fails with the arbiter's verdict inverted.)
#[test]
fn a_glued_x_is_split_off_its_figurine() {
    let model = blob_and_x();
    let mut c = Canvas::new(120, 90);
    blob(&mut c, 20);
    ex(&mut c, 43);
    c.rect(40, 57, 46, 58); // the hairline
    let (boxes, _) = c.boxes(SCALE);
    assert_eq!(boxes.len(), 1, "glued: {boxes:?}");
    let split = reader(&model, &c).split_glued(&c.bin(), &boxes, SCALE).unwrap();
    assert_eq!(split.len(), 2, "{split:?}");
    let reads = reader(&model, &c).read(&split).unwrap();
    assert_eq!(reads.iter().map(|r| r.class).collect::<Vec<_>>(), vec![0, 1]);
    // A lone figurine is not cut: no halves read better than the whole.
    let mut alone = Canvas::new(120, 90);
    blob(&mut alone, 20);
    let (boxes, _) = alone.boxes(SCALE);
    assert_eq!(reader(&model, &alone).split_glued(&alone.bin(), &boxes, SCALE).unwrap(), boxes);
}

/// An outlined figurine whose stroke broke in two pieces that overlap in columns: the union
/// reads well, the pieces do not, so they merge. Two letters side by side do not.
#[test]
fn a_broken_outlined_figurine_is_merged() {
    let outline = |c: &mut Canvas, x: i64, gap: bool| {
        c.rect(x, 30, x + 20, 33).rect(x, 57, x + 20, 60).rect(x, 30, x + 3, 60).rect(x + 17, 30, x + 20, 60);
        c.rect(x + 8, 38, x + 12, 52);
        if gap {
            // Break both sides: a top piece and a bottom piece.
            for y in 44..46 {
                for dx in 0..20 {
                    c.px[y as usize * c.w + (x + dx) as usize] = 255;
                }
            }
        }
    };
    let mut whole = Canvas::new(40, 90);
    outline(&mut whole, 10, false);
    let model = Templates::new(&["F", "x"]).learn("F", &whole);
    let mut c = Canvas::new(120, 90);
    outline(&mut c, 20, true);
    let (boxes, _) = c.boxes(SCALE);
    let ordered = seg::order_in_strip(&boxes);
    assert!(ordered.len() >= 2, "broken: {ordered:?}");
    let merged = reader(&model, &c).merge_broken(&ordered, SCALE).unwrap();
    assert_eq!(merged, vec![Bx { x0: 20, y0: 30, x1: 40, y1: 60 }]);
    // Neighbours that do not overlap in columns are never offered to the arbiter, even when their
    // union would read well: the figurine cut down the middle stays two boxes.
    let mut halves = Canvas::new(120, 90);
    outline(&mut halves, 20, false);
    for y in 0..90 {
        halves.px[y * halves.w + 30] = 255;
    }
    let (boxes, _) = halves.boxes(SCALE);
    let ordered = seg::order_in_strip(&boxes);
    assert_eq!(ordered.len(), 2, "{ordered:?}");
    assert_eq!(reader(&model, &halves).merge_broken(&ordered, SCALE).unwrap(), ordered);
}

/// An `o` or `0` whose hairlines a scan lost: two arcs 3 px apart (0.15 scales), each read as a
/// bracket at full confidence, so `merge_broken` keeps them; read as one letter here. The same
/// arcs 6 px apart (0.3 scales) are two glyphs, and real brackets around a letter are left alone.
#[test]
fn an_empty_pair_of_brackets_is_a_round_letter_cut_in_two() {
    let arcs = |c: &mut Canvas, x: i64, gap: i64| {
        c.rect(x, 30, x + 3, 60).rect(x, 30, x + 6, 33).rect(x, 57, x + 6, 60);
        let r = x + 6 + gap;
        c.rect(r + 3, 30, r + 6, 60).rect(r, 30, r + 6, 33).rect(r, 57, r + 6, 60);
    };
    let (mut left, mut right, mut oval) = (Canvas::new(40, 90), Canvas::new(40, 90), Canvas::new(40, 90));
    left.rect(10, 30, 13, 60).rect(10, 30, 16, 33).rect(10, 57, 16, 60);
    right.rect(13, 30, 16, 60).rect(10, 30, 16, 33).rect(10, 57, 16, 60);
    arcs(&mut oval, 10, 3);
    let mut x = Canvas::new(40, 90);
    x.rect(10, 40, 22, 60);
    let model = Templates::new(&["(", ")", "O", "x"]).learn("(", &left).learn(")", &right).learn("O", &oval).learn("x", &x);
    for (gap, joined) in [(3, true), (6, false)] {
        let mut c = Canvas::new(120, 90);
        arcs(&mut c, 20, gap);
        let (boxes, _) = c.boxes(SCALE);
        let ordered = seg::order_in_strip(&boxes);
        assert_eq!(ordered.len(), 2, "gap {gap}: {ordered:?}");
        let r = reader(&model, &c);
        assert_eq!(r.merge_broken(&ordered, SCALE).unwrap(), ordered, "gap {gap}: the arcs read as brackets");
        let reads = r.read(&ordered).unwrap();
        let (line, reads) = r.join_bracket_pairs(ordered.clone(), reads, SCALE).unwrap();
        let names: Vec<&str> = reads.iter().map(|r| model.names[r.class].as_str()).collect();
        if joined {
            assert_eq!(line, vec![Bx { x0: 20, y0: 30, x1: 35, y1: 60 }], "gap {gap}");
            assert_eq!(names, ["O"]);
        } else {
            assert_eq!(line, ordered, "gap {gap}");
            assert_eq!(names, ["(", ")"]);
        }
    }
    // `(x)`: the brackets hold a letter, so no pair stands side by side.
    let mut c = Canvas::new(160, 90);
    c.rect(20, 30, 23, 60).rect(20, 30, 26, 33).rect(20, 57, 26, 60);
    c.rect(32, 40, 44, 60);
    c.rect(53, 30, 56, 60).rect(50, 30, 56, 33).rect(50, 57, 56, 60);
    let (boxes, _) = c.boxes(SCALE);
    let ordered = seg::order_in_strip(&boxes);
    let r = reader(&model, &c);
    let reads = r.read(&ordered).unwrap();
    let (line, _) = r.join_bracket_pairs(ordered.clone(), reads, SCALE).unwrap();
    assert_eq!(line, ordered);
}

/// A frame's side left on a scan (2 × 60 px at a 20 px scale) is no character, and it would join
/// every line beside it; an `l` (3 × 20 px), as thin but one scale tall, stays, and so does a
/// tall bracket (5 × 50 px, ten times taller than wide).
#[test]
fn an_upright_rule_is_not_a_character() {
    let mut c = Canvas::new(120, 100);
    c.rect(10, 20, 12, 80).rect(40, 40, 43, 60).rect(70, 30, 75, 80);
    let (mut boxes, _) = c.boxes(SCALE);
    boxes.sort_by_key(|b| b.x0);
    assert_eq!(boxes, vec![Bx { x0: 40, y0: 40, x1: 43, y1: 60 }, Bx { x0: 70, y0: 30, x1: 75, y1: 80 }]);
}

/// A frame drawn around a header (8 × 4 scales, a 2 px outline) is no glyph, and the letter it
/// holds is read, not dropped as the frame's inner detail. An outlined figurine, as hollow but one
/// scale across, is still one glyph with its inner strokes dropped.
#[test]
fn a_frame_around_text_is_no_glyph_and_its_text_is_read() {
    let mut c = Canvas::new(240, 120);
    c.rect(20, 20, 180, 22).rect(20, 98, 180, 100).rect(20, 20, 22, 100).rect(178, 20, 180, 100);
    c.rect(60, 50, 74, 70);
    // An outlined figurine: a 20 px square ring with a stroke inside.
    c.rect(200, 50, 220, 53).rect(200, 67, 220, 70).rect(200, 50, 203, 70).rect(217, 50, 220, 70).rect(208, 56, 212, 64);
    let (mut boxes, _) = c.boxes(SCALE);
    boxes.sort_by_key(|b| b.x0);
    assert_eq!(boxes, vec![Bx { x0: 60, y0: 50, x1: 74, y1: 70 }, Bx { x0: 200, y0: 50, x1: 220, y1: 70 }]);
    let frames = crate::glyphs::frames(&c.image(), None, Some(SCALE as u32)).unwrap();
    assert_eq!(frames, vec![PixelRect { x0: 20, y0: 20, x1: 180, y1: 100 }]);
}

/// A box with a drop shadow (Pinski's `CHAPTER FOUR`: a 2 px outline, an 8 px edge offset under
/// it and right of it) is inked on 19% of its box, more than a plain frame, and is a frame all
/// the same: no glyph, and the letter it holds is read. A display letter as large and as inked
/// that is no closed box, an `L` (sides open) or an `O` (its bowl inside the band at its
/// diagonals), stays a glyph. (Fails without the ring rule: the box is one glyph and the letter
/// inside it its inner detail.)
#[test]
fn a_box_with_a_drop_shadow_is_a_frame_and_its_text_is_read() {
    let mut c = Canvas::new(480, 140);
    c.rect(20, 20, 180, 22).rect(20, 98, 180, 100).rect(20, 20, 22, 100).rect(178, 20, 180, 100);
    c.rect(28, 100, 188, 108).rect(180, 28, 188, 108);
    c.rect(60, 50, 74, 70);
    // An `L`, 90 px with 10 px strokes: 21% inked.
    c.rect(220, 20, 230, 110).rect(220, 100, 310, 110);
    // An `O`, 90 px across with a 7 px stroke: 22% inked.
    for y in 20..110i64 {
        for x in 340..430i64 {
            let (dx, dy) = (x as f64 + 0.5 - 385.0, y as f64 + 0.5 - 65.0);
            if (38.0..=45.0).contains(&dx.hypot(dy)) {
                c.ink(x, y);
            }
        }
    }
    let (mut boxes, _) = c.boxes(SCALE);
    boxes.sort_by_key(|b| b.x0);
    assert_eq!(
        boxes,
        vec![Bx { x0: 60, y0: 50, x1: 74, y1: 70 }, Bx { x0: 220, y0: 20, x1: 310, y1: 110 }, Bx { x0: 340, y0: 20, x1: 430, y1: 110 }]
    );
    let frames = crate::glyphs::frames(&c.image(), None, Some(SCALE as u32)).unwrap();
    assert_eq!(frames, vec![PixelRect { x0: 20, y0: 20, x1: 188, y1: 108 }]);
}

/// A scan's letter cut at its hairline: the bowl and the stem of a `d`, 1 px apart (0.05 scales),
/// are read as one when the union reads well and the pieces do not. The same pieces at the
/// distance of two letters of a word (3 px, 0.15 scales) are never offered to the arbiter.
#[test]
fn a_letter_cut_at_its_hairline_is_merged() {
    let letter = |c: &mut Canvas, x: i64, gap: i64| {
        c.rect(x, 40, x + 6, 60).rect(x + 6 + gap, 30, x + 9 + gap, 60);
    };
    let mut whole = Canvas::new(40, 90);
    letter(&mut whole, 10, 0);
    let model = Templates::new(&["d", "x"]).learn("d", &whole);
    for (gap, merged) in [(1, true), (3, false)] {
        let mut c = Canvas::new(120, 90);
        letter(&mut c, 20, gap);
        let (boxes, _) = c.boxes(SCALE);
        let ordered = seg::order_in_strip(&boxes);
        assert_eq!(ordered.len(), 2, "cut {gap}: {ordered:?}");
        let out = reader(&model, &c).merge_broken(&ordered, SCALE).unwrap();
        if merged {
            assert_eq!(out, vec![Bx { x0: 20, y0: 30, x1: 30, y1: 60 }], "cut {gap}");
        } else {
            assert_eq!(out, ordered, "cut {gap}");
        }
    }
}

// ---------------------------------------------------------------- corrections

fn read_of(classes: &Classes, text: &str) -> Read {
    let k = classes.names.len();
    let class = classes.of(text).unwrap();
    let mut probs = vec![0.0; k];
    probs[class] = 0.9;
    Read { class, p: 0.9, probs }
}

fn bx(x0: i64, y0: i64, x1: i64, y1: i64) -> Bx {
    Bx { x0, y0, x1, y1 }
}

/// `o` drawn tall is `O`, `O` drawn small is `o`; letters outside `cosuvwxz` keep their case.
/// (Fails with the correction off.)
#[test]
fn case_follows_height() {
    let names = names(&["o", "O", "n", "x", "X", "P"]);
    let classes = Classes::new(&names);
    let texts = ["n", "o", "O", "n", "P", "x"];
    let boxes = [bx(0, 40, 14, 60), bx(18, 30, 32, 60), bx(36, 40, 50, 60), bx(54, 40, 68, 60), bx(72, 40, 86, 60), bx(90, 40, 104, 60)];
    let mut reads: Vec<Read> = texts.iter().map(|t| read_of(&classes, t)).collect();
    fix::case_by_height(&mut reads, &boxes, &classes);
    let got: Vec<&str> = reads.iter().map(|r| classes.text(r.class)).collect();
    assert_eq!(got, vec!["n", "O", "o", "n", "P", "x"]);
}

#[test]
fn stacked_glyphs_take_their_shape_and_square_dots_their_place() {
    let names = names(&["=", ":", ".", "—", "-", "a"]);
    let classes = Classes::new(&names);
    let mut reads: Vec<Read> = ["=", ":", "a"].iter().map(|t| read_of(&classes, t)).collect();
    fix::stacked(&mut reads, &[bx(0, 40, 6, 60), bx(10, 46, 28, 56), bx(30, 40, 44, 60)], &classes);
    assert_eq!(reads.iter().map(|r| classes.text(r.class)).collect::<Vec<_>>(), vec![":", "=", "a"]);
    // A square box on the baseline read as a dash is a dot; a dash at mid height stays.
    let boxes = [bx(0, 40, 14, 60), bx(18, 54, 24, 60), bx(28, 48, 34, 53), bx(38, 40, 52, 60)];
    let mut reads: Vec<Read> = ["a", "—", "-", "a"].iter().map(|t| read_of(&classes, t)).collect();
    fix::square_dots(&mut reads, &boxes, &classes);
    assert_eq!(reads.iter().map(|r| classes.text(r.class)).collect::<Vec<_>>(), vec!["a", ".", "-", "a"]);
}

#[test]
fn ovals_in_numbers_are_zeros_and_castling_is_read() {
    let names = names(&["1", "o", "O", "0", "-", ".", "a", "!"]);
    let classes = Classes::new(&names);
    let word = |s: &str| s.chars().map(|c| read_of(&classes, &c.to_string())).collect::<Vec<_>>();
    let mut reads = [word("1o"), word("o-o!"), word("ao"), word("O.O.O")].concat();
    let ranges = [(0, 2), (2, 6), (6, 8), (8, 13)];
    fix::numbers(&mut reads, &ranges, &classes);
    let text: String = reads.iter().map(|r| classes.text(r.class)).collect();
    assert_eq!(text, "100-0!ao0-0-0");
}

#[test]
fn words_split_at_wide_gaps_but_marks_stick() {
    let names = names(&["a", "!", "?"]);
    let classes = Classes::new(&names);
    let boxes = [bx(0, 40, 10, 60), bx(12, 40, 22, 60), bx(40, 40, 50, 60), bx(58, 40, 62, 60)];
    let reads: Vec<Read> = ["a", "a", "a", "!"].iter().map(|t| read_of(&classes, t)).collect();
    assert_eq!(fix::words(&boxes, &reads, &classes), vec![(0, 2), (2, 4)]);
    assert!(fix::words(&[], &[], &classes).is_empty());
}

fn gw(text: &str, x0: u32, x1: u32) -> GlyphWord {
    let rect = PixelRect { x0, y0: 40, x1, y1: 60 };
    let glyphs = text.chars().map(|c| Glyph { rect, text: c.to_string(), p: 0.9, alternatives: Vec::new(), margin: 0.5 }).collect();
    GlyphWord { text: text.to_owned(), rect, glyphs }
}

/// `1 9...♕xf4` is move 19 split by the segmentation; prose (`In 1 9`) is never joined.
/// (Fails with the join off.)
#[test]
fn a_split_move_number_is_joined_on_notation_lines_only() {
    let mut words = vec![gw("1", 0, 10), gw("9...♕xf4", 14, 90), gw("e5", 100, 120)];
    fix::join_move_numbers(&mut words);
    assert_eq!(words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>(), vec!["19...♕xf4", "e5"]);
    assert_eq!(words[0].rect, PixelRect { x0: 0, y0: 40, x1: 90, y1: 60 });
    assert_eq!(words[0].glyphs.len(), 9);
    let mut prose = vec![gw("In", 0, 20), gw("1", 30, 40), gw("9", 44, 54), gw("games", 60, 120)];
    fix::join_move_numbers(&mut prose);
    assert_eq!(prose.len(), 4);
    // The `0` of `10` read as a letter once a scan cut it and the book's wide figures set it off.
    let mut oval = vec![gw("1", 0, 10), gw("O...♕e7", 16, 90), gw("1", 100, 110), gw("O", 116, 126), gw("♘xc4", 130, 160)];
    fix::join_move_numbers(&mut oval);
    assert_eq!(oval.iter().map(|w| w.text.as_str()).collect::<Vec<_>>(), vec!["10...♕e7", "10", "♘xc4"]);
    assert_eq!(oval[0].glyphs.iter().map(|g| g.text.as_str()).collect::<String>(), "10...♕e7");
    // Castling after a number stays apart: `5 O-O` is move 5.
    let mut castle = vec![gw("5", 0, 10), gw("O-O", 16, 50), gw("♘f6", 60, 90)];
    fix::join_move_numbers(&mut castle);
    assert_eq!(castle.len(), 3);
    // Four digits are a year, not a move number.
    let mut year = vec![gw("1", 0, 10), gw("984", 14, 50), gw("♘f3", 60, 90)];
    fix::join_move_numbers(&mut year);
    assert_eq!(year.len(), 3);
}

#[test]
fn notation_is_told_from_prose() {
    for w in ["♘f3", "exd5", "O-O", "0-0-0", "24", "24...", "19.", "!?", "+-", "±", "e8=Q+", "Nf3"] {
        assert!(notation::is_notation(w), "{w}");
    }
    for w in ["the", "If", "Hübner", "1984a", "", "(and", "x", "wins"] {
        assert!(!notation::is_notation(w), "{w}");
    }
    // A figurine after a letter is a misread prose digraph (German `ch` as `♔`), not a piece.
    for w in ["g1ei♔e", "s♔wierig", "M♘lich", "Lãufer"] {
        assert!(!notation::is_notation(w), "{w}");
    }
    // A figurine after a move number, a bracket or at the start still is.
    for w in ["12.♘f3", "(♖d1", "♔g2", "e8♕"] {
        assert!(notation::is_notation(w), "{w}");
    }
}

// ---------------------------------------------------------------- whole reader

#[test]
fn reads_a_drawn_line_into_words_with_alternatives() {
    let model = blob_and_x();
    let mut c = Canvas::new(200, 90);
    blob(&mut c, 10);
    ex(&mut c, 34);
    blob(&mut c, 90);
    let lines = read(&c.image(), None, None, &model, &GlyphOptions::default()).unwrap();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].text(), "Fx F");
    let g = &lines[0].words[0].glyphs[0];
    assert_eq!(g.text, "F");
    assert_eq!(g.alternatives.first().map(|a| a.text.as_str()), Some("F"));
    assert!(g.alternatives.len() <= MAX_ALTERNATIVES && g.alternatives.iter().all(|a| a.p >= MIN_ALTERNATIVE_P));
    assert!((0.0..=1.0).contains(&g.margin) && g.margin > 0.5, "{}", g.margin);
    // A region reads only what it covers, with boxes in the image's pixels.
    let region = PixelRect { x0: 80, y0: 0, x1: 200, y1: 90 };
    let lines = read(&c.image(), Some(region), Some(30), &model, &GlyphOptions::default()).unwrap();
    assert_eq!(lines[0].text(), "F");
    assert_eq!(lines[0].words[0].rect, PixelRect { x0: 90, y0: 30, x1: 110, y1: 60 });
}

#[test]
fn empty_blank_and_huge_inputs_are_values_not_panics() {
    let model = blob_and_x();
    let opts = GlyphOptions::default();
    let blank = GrayImage::new(50, 20, vec![255; 1000]).unwrap();
    assert!(read(&blank, None, None, &model, &opts).unwrap().is_empty());
    let black = GrayImage::new(50, 20, vec![0; 1000]).unwrap();
    assert!(read(&black, None, None, &model, &opts).is_ok());
    let dot = GrayImage::new(1, 1, vec![0]).unwrap();
    assert!(read(&dot, None, None, &model, &opts).is_ok());
    let outside = PixelRect { x0: 500, y0: 500, x1: 900, y1: 900 };
    assert!(read(&blank, Some(outside), None, &model, &opts).unwrap().is_empty());
    let inverted = PixelRect { x0: 40, y0: 10, x1: 10, y1: 5 };
    assert!(read(&blank, Some(inverted), None, &model, &opts).unwrap().is_empty());
    assert_eq!(text_scale(&blank, &[]).unwrap(), None);
    // Larger than the cap: refused before any work, for a whole image or a region of it.
    let huge = GrayImage::new(8000, 5001, vec![255; 8000 * 5001]).unwrap();
    assert!(matches!(read(&huge, None, None, &model, &opts), Err(GlyphError::TooLarge { .. })));
    assert!(matches!(text_scale(&huge, &[]), Err(GlyphError::TooLarge { .. })));
    assert!(read(&huge, Some(PixelRect { x0: 0, y0: 0, x1: 100, y1: 100 }), None, &model, &opts).unwrap().is_empty());
    // Speckle: more components than a text region has.
    let side = 320usize;
    let speckle: Vec<u8> = (0..side * side).map(|i| if (i % side).is_multiple_of(2) && (i / side).is_multiple_of(2) { 0 } else { 255 }).collect();
    let speckle = GrayImage::new(side as u32, side as u32, speckle).unwrap();
    assert!(matches!(read(&speckle, None, None, &model, &opts), Err(GlyphError::TooManyComponents { .. })));
}

#[test]
fn a_misbehaving_classifier_is_an_error() {
    let mut c = Canvas::new(100, 90);
    blob(&mut c, 10);
    let image = c.image();
    let opts = GlyphOptions::default();
    let none = Fixed { names: Vec::new(), answer: uniform };
    assert!(matches!(read(&image, None, None, &none, &opts), Err(GlyphError::NoClasses)));
    let short = Fixed { names: names(&["a", "b"]), answer: |count, _| vec![0.5; count] };
    assert!(matches!(read(&image, None, None, &short, &opts), Err(GlyphError::Output { .. })));
    // Not-a-number probabilities read as zero rather than poisoning the choice.
    let nan = Fixed { names: names(&["a", "b"]), answer: |count, k| vec![f32::NAN; count * k] };
    let lines = read(&image, None, None, &nan, &opts).unwrap();
    assert!(lines.iter().flat_map(|l| &l.words).flat_map(|w| &w.glyphs).all(|g| g.p == 0.0 && g.margin == 0.0));
}

#[test]
fn the_glyph_model_is_a_classifier() {
    let Some(dir) = std::env::var_os("PDFCRAFT_CHESS_TEST_MODELS") else { return };
    let models = crate::models::ChessModels::load_from(std::path::Path::new(&dir));
    let model = models.glyphs.loaded().expect("glyph model");
    let mut c = Canvas::new(120, 90);
    plus(&mut c, 20, 33, 18);
    c.rect(20, 55, 38, 58);
    let lines = read(&c.image(), None, Some(25), model, &GlyphOptions::default()).unwrap();
    assert_eq!(lines.len(), 1);
}

// ---------------------------------------------------------------- fusion

fn ow(text: &str, x0: u32, x1: u32) -> OcrWord {
    OcrWord { text: text.to_owned(), rect: PixelRect { x0, y0: 40, x1, y1: 60 } }
}

fn line(words: Vec<GlyphWord>) -> GlyphLine {
    let rect = words.iter().map(|w| w.rect).reduce(union_rect).unwrap_or_default();
    GlyphLine { words, rect }
}

/// Prose goes to the OCR, notation to the glyph reader, a number both read alike keeps the OCR's spacing,
/// and an accent the OCR could not read comes from the glyphs. (Fails with routing inverted.)
#[test]
fn fusion_routes_notation_to_glyphs_and_prose_to_the_ocr() {
    let ocr = [ow("If", 0, 20), ow("37", 30, 50), ow("Bb7+", 60, 110), ow("there", 120, 170), ow("1984", 180, 230), ow("H?bner", 240, 300)];
    let glyphs = line(vec![
        gw("lf", 0, 20),
        gw("37", 30, 50),
        gw("♖b7+", 60, 110),
        gw("thero", 120, 170),
        gw("1", 180, 190),
        gw("984", 196, 230),
        gw("Hübner", 240, 300),
    ]);
    let fused = fuse_line(&ocr, std::slice::from_ref(&glyphs));
    assert_eq!(fused.text(), "If 37 ♖b7+ there 1984 Hübner");
    let src: Vec<WordSource> = fused.words.iter().map(|w| w.source).collect();
    use WordSource::{Glyphs as G, Ocr as O};
    assert_eq!(src, vec![O, O, G, O, O, G]);
    assert_eq!(fused.words[2].other.as_deref(), Some("Bb7+"));
    assert_eq!(fused.words[3].other.as_deref(), Some("thero"));
    assert!(!fused.words[2].glyphs.is_empty() && fused.words[0].glyphs.is_empty());
    assert_eq!(fused.rect, PixelRect { x0: 0, y0: 40, x1: 300, y1: 60 });
    // A word only one reader saw is kept from it.
    let fused = fuse_line(&[ow("prose", 0, 40)], &[line(vec![gw("♘", 100, 110)])]);
    assert_eq!(fused.text(), "prose ♘");
    assert!(fuse_line(&[], &[]).words.is_empty());
}

#[test]
fn fused_lines_read_as_chess_text_like_a_text_layer() {
    let glyphs = line(vec![gw("12.♘f3", 0, 60), gw("e5!", 70, 100)]);
    let fused = fuse_line(&[ow("12.Bf3", 0, 60), ow("e5!", 70, 100), ow("wins", 110, 150)], &[glyphs]);
    let text = to_chess_text(std::slice::from_ref(&fused), 72.0, Some(Lang::En));
    let runs: Vec<ChessRun> = text.lines[0].spans.iter().map(|s| s.run.clone()).collect();
    let mv = |s: &str, t: &str| ChessRun::Move { san: San::read(s, Lang::En).unwrap(), text: t.to_owned() };
    assert_eq!(runs[..3], [ChessRun::MoveNumber { number: 12, black: false }, mv("Nf3", "♘f3"), mv("e5", "e5")]);
    // At 72 DPI a pixel is a point: the boxes carry over.
    let page = to_page_text(&[fused], 72.0);
    assert_eq!(page.glyphs.len(), 6 + 3 + 4);
    assert_eq!(page.glyphs[0].rect, [0.0, 40.0, 60.0, 60.0]);
    assert_eq!(page.space_before.iter().filter(|s| **s).count(), 2);
    // The OCR word's box is shared among its letters, the last one taking the remainder.
    assert_eq!(page.glyphs[9].rect[0], 110.0);
    assert_eq!(page.glyphs[12].rect[2], 150.0);
    assert_eq!((page.glyphs[10].rect[0], page.glyphs[10].rect[2]), (120.0, 130.0));
    // A DPI that cannot convert gives no glyphs, not a panic.
    let bad = FusedLine {
        words: vec![FusedWord { text: "a".into(), rect: PixelRect::default(), source: WordSource::Ocr, glyphs: Vec::new(), other: None }],
        rect: PixelRect::default(),
    };
    assert!(to_page_text(&[bad], 0.0).glyphs.is_empty());
}

/// A recognised glyph's box is its ink (`a` half the height of `h`), but on the page text every
/// glyph spans its line, as a text layer's glyphs span the font: the layout measures word gaps
/// and gutters against glyph height.
#[test]
fn page_text_glyphs_span_their_line() {
    let glyph = |c: &str, x0: u32, y0: u32| Glyph {
        rect: PixelRect { x0, y0, x1: x0 + 8, y1: 60 },
        text: c.into(),
        p: 0.9,
        alternatives: Vec::new(),
        margin: 0.5,
    };
    let word = FusedWord {
        text: "ah".into(),
        rect: PixelRect { x0: 0, y0: 40, x1: 16, y1: 60 },
        source: WordSource::Glyphs,
        glyphs: vec![glyph("a", 0, 50), glyph("h", 8, 40)],
        other: None,
    };
    let fused = FusedLine { words: vec![word], rect: PixelRect { x0: 0, y0: 38, x1: 16, y1: 62 } };
    let page = to_page_text(&[fused], 72.0);
    assert!(page.glyphs.iter().all(|g| g.rect[1] == 38.0 && g.rect[3] == 62.0), "{:?}", page.glyphs.iter().map(|g| g.rect).collect::<Vec<_>>());
    assert_eq!((page.glyphs[0].rect[0], page.glyphs[0].rect[2]), (0.0, 8.0), "the horizontal box is the glyph's");
}

// ---------------------------------------------------------------- properties

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// No image, region or scale makes the reader panic.
    #[test]
    fn reading_random_images_never_panics(
        w in 1u32..48, h in 1u32..48,
        seed in proptest::collection::vec(any::<u8>(), 48 * 48),
        region in proptest::option::of((0u32..60, 0u32..60, 0u32..60, 0u32..60)),
        scale in proptest::option::of(0u32..80),
        sparse in any::<bool>(),
    ) {
        let n = (w * h) as usize;
        let px: Vec<u8> = seed.iter().take(n).map(|v| if sparse { if *v < 40 { 0 } else { 255 } } else { *v }).collect();
        let image = GrayImage::new(w, h, px).unwrap();
        let region = region.map(|(x0, y0, x1, y1)| PixelRect { x0, y0, x1, y1 });
        let model = Fixed { names: names(&["a", "0", "o", "O", "=", ":", "-", ".", "♘"]), answer: |count, k| (0..count * k).map(|i| ((i * 7919) % 97) as f32 / 97.0).collect() };
        let lines = read(&image, region, scale, &model, &GlyphOptions::default()).unwrap();
        for l in &lines {
            for word in &l.words {
                for g in &word.glyphs {
                    prop_assert!(g.rect.x1 <= w && g.rect.y1 <= h && g.rect.x0 <= g.rect.x1);
                }
            }
        }
        let _ = text_scale(&image, &[PixelRect { x0: 0, y0: 0, x1: w / 2, y1: h }]);
    }

    /// No pair of word lists makes the fusion panic, and it keeps every word one reader saw.
    #[test]
    fn fusing_random_words_never_panics(
        ocr in proptest::collection::vec(("[a-z?0-9♘]{0,6}", 0u32..400, 0u32..60), 0..8),
        glyph in proptest::collection::vec(("[a-z0-9♘.!?+-]{1,6}", 0u32..400, 0u32..60), 0..8),
    ) {
        let ocr: Vec<OcrWord> = ocr.into_iter().map(|(t, x, w)| ow(&t, x, x + w)).collect();
        let glyphs = line(glyph.into_iter().map(|(t, x, w)| gw(&t, x, x + w)).collect());
        let fused = fuse_line(&ocr, std::slice::from_ref(&glyphs));
        prop_assert!(fused.words.len() <= ocr.len() + glyphs.words.len());
        let _ = to_chess_text(&[fused], 300.0, None);
    }

    #[test]
    fn notation_rules_never_panic(s in "\\PC{0,24}") {
        let _ = notation::is_notation(&s);
        let _ = notation::has_move(&notation::fold(&s));
    }
}

/// A line of `(height, count)` glyphs, each its own word.
fn line_of(heights: &[(u32, usize)]) -> GlyphLine {
    let words: Vec<GlyphWord> = heights
        .iter()
        .flat_map(|&(h, n)| std::iter::repeat_n(h, n))
        .enumerate()
        .map(|(i, h)| {
            let x0 = u32::try_from(i).unwrap() * 20;
            let rect = PixelRect { x0, y0: 100 - h, x1: x0 + 12, y1: 100 };
            GlyphWord { text: "a".into(), rect, glyphs: vec![Glyph { rect, text: "a".into(), p: 0.9, alternatives: Vec::new(), margin: 0.5 }] }
        })
        .collect();
    GlyphLine { words, rect: PixelRect { x0: 0, y0: 60, x1: 400, y1: 100 } }
}

/// A line's height is its capitals' and ascenders': a line of prose (mostly x-height letters, a
/// few ascenders, a comma) and a line of moves (digits and figurines, all capital height) of one
/// size measure alike; a title set larger measures larger; an empty line has none. (Fails with
/// the median: the prose line measures its x-height, 20 against 30.)
#[test]
fn a_lines_height_is_its_capitals_whatever_it_holds() {
    let prose = line_of(&[(6, 2), (20, 22), (30, 9), (38, 1)]);
    let moves = line_of(&[(30, 14), (8, 3)]);
    let title = line_of(&[(30, 10), (45, 6)]);
    assert_eq!(prose.height(), Some(30));
    assert_eq!(moves.height(), Some(30));
    assert_eq!(title.height(), Some(45));
    assert_eq!(line_of(&[]).height(), None);
    assert_eq!(line_of(&[(0, 3)]).height(), None, "glyphs without height");
}
