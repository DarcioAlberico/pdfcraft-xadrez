//! Whole-board preprocessing before the squares are cut: deskew, flat-field, hatch suppression
//! and CLAHE, each measured and each switchable ([`Steps`], [`Preprocessing`]).
//!
//! The piece model was trained on raw boards (the prototype's `norm0`: gray, `INTER_AREA` to
//! 64 px, no normalisation), and its training augmentation (`ColorJitter`, brightness and contrast
//! 0.3) already taught it to ignore global lighting and contrast. So every step here is the
//! identity on a board with nothing to correct (a straight grid is turned by 0°, a flat field is
//! divided by a constant, a board with plain dark squares has no hatch), and what
//! [`super::read`] runs is what the measurements kept ([`Preprocessing::DEFAULT`]; the numbers
//! are in `docs/chess.md`):
//!
//! - **Hatch suppression, on a retry only.** A board whose dark squares are hatched or stippled
//!   and which the gate refused is read once more with its *empty* dark squares painted one flat
//!   gray, the texture's average; a second reading replaces the first only when the gate accepts
//!   it. The model reads a stippled empty square as a king (Reinfeld's *1001 Sacrificios*, 1977:
//!   no board of 106 accepted, 60 after), but it reads pieces on hatched squares better than any
//!   flattening of them: a hatch is as coarse as a piece's outline, so a filter that removes one
//!   removes the other (the prototype's S-39 found the same with medians and morphology), and the
//!   squares with a piece keep their texture.
//! - **Deskew, flat-field and CLAHE: off.** They change no reading on the labelled boards, first or
//!   on a retry.
//!
//! The order is the prototype's (`preprocess.py`, `BoardNormalizer.normalize`, relicensed
//! MIT/Apache by its author): deskew first, because the other steps measure local statistics
//! and a slanted grid puts them on the wrong edges; flat-field before hatch suppression, so an
//! illumination gradient does not move the hatch detector's threshold from corner to corner; CLAHE
//! last, as the only step that amplifies.

use crate::imageops::{Plane, clahe, gaussian_blur_plane, resize_area, rotate_plane, to_gray, to_plane};
use crate::raster::GrayImage;

/// The preprocessing constants. The measured ones say what they were measured on; the rest
/// are safety caps or choices and say so.
pub mod consts {
    /// Largest residual turn, in degrees, the deskew looks for (`MAX_SKEW_DEGREES`). The warp
    /// already undoes perspective; what is left is a fraction of a degree to a few. Searching
    /// further finds the angle at which the measure matches something else.
    pub const MAX_SKEW_DEGREES: f64 = 3.0;

    /// Step of the deskew's search, in degrees (`SKEW_STEP_DEGREES`).
    pub const SKEW_STEP_DEGREES: f64 = 0.25;

    /// Side, in pixels, of the probe the deskew searches on (`SKEW_PROBE_SIDE`): 32 px a square
    /// is enough for the grid's period.
    pub const SKEW_PROBE_SIDE: usize = 256;

    /// Least gain in grid sharpness for a turn to be applied (`MIN_SKEW_GAIN`): below it the
    /// best angle is noise, and every turn resamples the board.
    pub const MIN_SKEW_GAIN: f64 = 1.02;

    /// Standard deviation of the flat-field's background blur, as a share of the board's side
    /// (`flat_field_sigma_ratio`): two squares, far above the checker's period, so the blur keeps
    /// the illumination and loses the squares.
    pub const FLAT_FIELD_SIGMA_RATIO: f64 = 0.25;

    /// Side, in pixels, the background is estimated on before it is stretched back: the blur is
    /// two squares wide, so 128 px (16 a square) loses nothing of it and costs 40 times less.
    pub const FLAT_FIELD_PROBE_SIDE: usize = 128;

    /// Standard deviation of the local mean a square's texture is measured against, as a share
    /// of the board's side (4 px on the 800 px board): finer than a piece, about a hatch's line.
    pub const HATCH_SIGMA_RATIO: f64 = 0.005;

    /// A board's dark squares count as hatched or stippled when their median texture (mean
    /// distance of a pixel from its local mean, in gray levels) is at least this, and this many
    /// times the light squares'. On the field set's misread boards: light squares 6–25, hatched
    /// or stippled dark squares 12–83; pieces crowding the light squares raise their texture
    /// (Reinfeld, 46 against 24), which a ratio of 2 missed.
    pub const HATCH_MIN_TEXTURE: f64 = 8.0;
    pub const HATCH_MIN_TEXTURE_RATIO: f64 = 1.5;

    /// Blocks a side each dark square is cut into to tell an empty one: a block a quarter of a
    /// square wide is wider than the hatch's or stipple's period, so an empty square's blocks
    /// all average to the same gray.
    pub const HATCH_BLOCKS: usize = 4;

    /// The blocks leave out a 1/16 of the square along each edge, where a grid line or the
    /// board's frame may run.
    pub const HATCH_INSET: usize = 16;

    /// A dark square is empty when none of its blocks is further from the dark squares' typical
    /// block than this share of the distance from that block to the light squares' level. The
    /// field set reads the same from 0.2 to 0.4 on the retry.
    pub const HATCH_EMPTY_TOLERANCE: f64 = 0.3;

    /// CLAHE's tiles a side (`tileGridSize = (8, 8)`): one tile a square, so the contrast
    /// equalised is the piece's against its own square.
    pub const CLAHE_TILES: usize = 8;

    /// CLAHE's clip limit, in mean bin heights (the prototype's `clahe_clip`).
    pub const CLAHE_CLIP: f64 = 2.0;
}

use consts::*;

/// Which preprocessing steps run, in the fixed order deskew → flat-field → hatch → CLAHE.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Steps {
    pub deskew: bool,
    pub flat_field: bool,
    pub hatch: bool,
    pub clahe: bool,
}

impl Steps {
    /// No step: the board as cut, which is what the piece model was trained on.
    pub const NONE: Steps = Steps { deskew: false, flat_field: false, hatch: false, clahe: false };

    /// The steps named by letters, as the prototype's normaliser versions name them: `d`
    /// deskew, `f` flat-field, `h` hatch suppression, `c` CLAHE; `""` or `"0"` for none. `None`
    /// for any other letter.
    pub fn from_letters(letters: &str) -> Option<Steps> {
        let mut steps = Steps::NONE;
        for c in letters.chars() {
            match c {
                'd' => steps.deskew = true,
                'f' => steps.flat_field = true,
                'h' => steps.hatch = true,
                'c' => steps.clahe = true,
                '0' => {}
                _ => return None,
            }
        }
        Some(steps)
    }

    /// Whether no step runs.
    pub fn is_none(self) -> bool {
        self == Steps::NONE
    }
}

/// How [`super::read`] preprocesses a board: `first` before every reading, then `retry` on a
/// board the gate refused, whose second reading is kept only when the gate accepts it. A retry
/// costs a second model call only for the refused boards it changes, and never replaces an
/// accepted reading.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Preprocessing {
    pub first: Steps,
    pub retry: Steps,
}

impl Preprocessing {
    /// What [`super::read`] does: the steps the measurements kept (see the module's doc).
    pub const DEFAULT: Preprocessing = Preprocessing { first: Steps::NONE, retry: Steps { hatch: true, ..Steps::NONE } };

    /// `first` and `retry` as letters ([`Steps::from_letters`]), the retry after a `>`: `"0"`,
    /// `"c"`, `"0>h"`. `None` for an unknown letter.
    pub fn from_letters(letters: &str) -> Option<Preprocessing> {
        let (first, retry) = letters.split_once('>').unwrap_or((letters, ""));
        Some(Preprocessing { first: Steps::from_letters(first)?, retry: Steps::from_letters(retry)? })
    }
}

/// The board after `steps`, in their fixed order. A step that cannot run on this board (too
/// small, say) leaves it as it is; with no step the board comes back unchanged.
pub fn preprocess(board: &GrayImage, steps: Steps) -> GrayImage {
    let mut out = board.clone();
    if steps.deskew {
        out = deskew(&out);
    }
    if steps.flat_field {
        out = flat_field(&out);
    }
    if steps.hatch {
        out = suppress_hatch(&out);
    }
    if steps.clahe {
        out = equalize(&out);
    }
    out
}

/// How sharply the plane's gradients line up in columns and rows (`_grid_periodicity`): the
/// variance of the mean horizontal gradient per column plus that of the vertical one per row. A
/// straight grid puts its edges in a few columns and rows; a slanted one smears them.
fn grid_sharpness(plane: &Plane) -> f64 {
    let (w, h) = (plane.width, plane.height);
    let mut columns = vec![0.0_f64; w.saturating_sub(1)];
    let mut rows = vec![0.0_f64; h.saturating_sub(1)];
    for y in 0..h {
        let row = plane.row(y);
        for (slot, pair) in columns.iter_mut().zip(row.windows(2)) {
            if let [a, b] = pair {
                *slot += f64::from((b - a).abs());
            }
        }
        if let Some(slot) = rows.get_mut(y) {
            *slot = row.iter().zip(plane.row(y + 1)).map(|(a, b)| f64::from((b - a).abs())).sum::<f64>() / w.max(1) as f64;
        }
    }
    columns.iter_mut().for_each(|v| *v /= h.max(1) as f64);
    variance(&columns) + variance(&rows)
}

fn variance(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n;
    values.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / n
}

/// The residual turn of the board's grid, in degrees counter-clockwise, or 0 when no turn within
/// ±[`consts::MAX_SKEW_DEGREES`] sharpens the grid by [`consts::MIN_SKEW_GAIN`] (`estimate_skew`).
/// A direct search on a [`consts::SKEW_PROBE_SIDE`] px probe rather than a line transform: what
/// is wanted is the angle that makes the grid sharpest.
pub fn estimate_skew(board: &GrayImage) -> f64 {
    let Some(probe) = resize_area(&to_plane(board), SKEW_PROBE_SIDE, SKEW_PROBE_SIDE) else { return 0.0 };
    let base = grid_sharpness(&probe);
    let steps = (MAX_SKEW_DEGREES / SKEW_STEP_DEGREES).round() as i32;
    let mut best = (0.0_f64, base);
    for k in -steps..=steps {
        if k == 0 {
            continue;
        }
        let angle = f64::from(k) * SKEW_STEP_DEGREES;
        let Some(turned) = rotate_plane(&probe, angle) else { continue };
        let sharpness = grid_sharpness(&turned);
        if sharpness > best.1 {
            best = (angle, sharpness);
        }
    }
    if best.1 < base * MIN_SKEW_GAIN { 0.0 } else { best.0 }
}

/// The board turned by [`estimate_skew`]'s angle, the border replicated; unchanged when that
/// angle is 0.
pub fn deskew(board: &GrayImage) -> GrayImage {
    let angle = estimate_skew(board);
    if angle == 0.0 {
        return board.clone();
    }
    rotate_plane(&to_plane(board), angle).and_then(|p| to_gray(&p)).unwrap_or_else(|| board.clone())
}

/// The board divided by its own heavily blurred copy and scaled back by that copy's mean
/// (`_apply_flat_field`): an illumination gradient and the paper's tint go, the squares stay. On
/// an evenly lit board the blur is nearly a constant and the step nearly the identity.
///
/// The background is estimated on a [`consts::FLAT_FIELD_PROBE_SIDE`] px copy, blurred by
/// [`consts::FLAT_FIELD_SIGMA_RATIO`] of its side, and sampled back bilinearly.
pub fn flat_field(board: &GrayImage) -> GrayImage {
    flat_fielded(board).unwrap_or_else(|| board.clone())
}

fn flat_fielded(board: &GrayImage) -> Option<GrayImage> {
    let plane = to_plane(board);
    let side = FLAT_FIELD_PROBE_SIDE.min(plane.width).min(plane.height);
    let small = resize_area(&plane, side, side)?;
    let background = gaussian_blur_plane(&small, side as f64 * FLAT_FIELD_SIGMA_RATIO)?;
    let mean = background.data.iter().map(|&v| f64::from(v)).sum::<f64>() / background.data.len().max(1) as f64;
    let background = crate::imageops::resize_bilinear(&to_gray(&background)?, board.width(), board.height())?;
    let data = plane.data.iter().zip(background.data()).map(|(&v, &b)| (f64::from(v) / f64::from(b).max(1.0) * mean) as f32).collect();
    to_gray(&Plane { data, ..plane })
}

/// The board with its empty hatched or stippled dark squares painted one flat gray, the
/// texture's average; unchanged when its dark squares are not textured (see the module's doc).
///
/// The dark squares are the darker parity of the 8 × 8 grid. They are textured when their median
/// [`consts::HATCH_MIN_TEXTURE`] and [`consts::HATCH_MIN_TEXTURE_RATIO`] say so. A dark square is
/// empty when each of its [`consts::HATCH_BLOCKS`]² blocks averages to the dark squares' typical
/// block within [`consts::HATCH_EMPTY_TOLERANCE`]: a block is wider than the texture's period, so
/// the texture averages out, and a piece pulls the blocks it covers towards white or black.
pub fn suppress_hatch(board: &GrayImage) -> GrayImage {
    hatch_suppressed(board).unwrap_or_else(|| board.clone())
}

/// Mean absolute difference between a square's pixels and their local mean: the fine texture a
/// hatch or stipple leaves everywhere in a square, and a piece only along its outline.
fn square_texture(plane: &Plane, smooth: &Plane, rect: (usize, usize, usize, usize)) -> f64 {
    let (x0, y0, x1, y1) = rect;
    let mut total = 0.0_f64;
    let mut count = 0_usize;
    for y in y0..y1 {
        let (row, blurred) = (plane.row(y).get(x0..x1).unwrap_or(&[]), smooth.row(y).get(x0..x1).unwrap_or(&[]));
        for (a, b) in row.iter().zip(blurred) {
            total += f64::from((a - b).abs());
            count += 1;
        }
    }
    if count == 0 { 0.0 } else { total / count as f64 }
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    values.get(values.len() / 2).copied().unwrap_or(0.0)
}

fn hatch_suppressed(board: &GrayImage) -> Option<GrayImage> {
    let plane = to_plane(board);
    let (w, h) = (plane.width, plane.height);
    if w < 8 * HATCH_BLOCKS * 2 || h < 8 * HATCH_BLOCKS * 2 {
        return None;
    }
    let smooth = gaussian_blur_plane(&plane, HATCH_SIGMA_RATIO * w as f64)?;
    let rect = |row: usize, column: usize| (column * w / 8, row * h / 8, (column + 1) * w / 8, (row + 1) * h / 8);
    // Each parity's mean level and texture; the darker parity holds the dark squares.
    let mut level = [0.0_f64; 2];
    let mut texture: [Vec<f64>; 2] = [Vec::with_capacity(32), Vec::with_capacity(32)];
    for row in 0..8 {
        for column in 0..8 {
            let r = rect(row, column);
            let parity = (row + column) % 2;
            if let Some(slot) = level.get_mut(parity) {
                *slot += block_mean(&plane, r) / 32.0;
            }
            if let Some(list) = texture.get_mut(parity) {
                list.push(square_texture(&plane, &smooth, r));
            }
        }
    }
    let dark = usize::from(level[1] < level[0]);
    let [even, odd] = &mut texture;
    let (dark_texture, light_texture) = if dark == 1 { (median(odd), median(even)) } else { (median(even), median(odd)) };
    if dark_texture < HATCH_MIN_TEXTURE || dark_texture < HATCH_MIN_TEXTURE_RATIO * light_texture {
        return None;
    }
    // The blocks of every dark square: an empty one averages its texture away in each block, a
    // piece moves the blocks it covers towards white or black.
    let blocks = |(x0, y0, x1, y1): (usize, usize, usize, usize)| -> Vec<f64> {
        // Inset, so a grid line or the frame along the square's edge is not counted.
        let (ix, iy) = ((x1 - x0) / HATCH_INSET, (y1 - y0) / HATCH_INSET);
        let (x0, y0, x1, y1) = (x0 + ix, y0 + iy, x1 - ix, y1 - iy);
        let (bw, bh) = ((x1 - x0) / HATCH_BLOCKS, (y1 - y0) / HATCH_BLOCKS);
        (0..HATCH_BLOCKS * HATCH_BLOCKS)
            .map(|k| {
                let (bx, by) = (x0 + (k % HATCH_BLOCKS) * bw, y0 + (k / HATCH_BLOCKS) * bh);
                block_mean(&plane, (bx, by, bx + bw, by + bh))
            })
            .collect()
    };
    let dark_squares: Vec<(usize, usize, usize, usize)> = (0..64).filter(|k| (k / 8 + k % 8) % 2 == dark).map(|k| rect(k / 8, k % 8)).collect();
    let mut all: Vec<f64> = dark_squares.iter().flat_map(|&r| blocks(r)).collect();
    let typical = median(&mut all);
    let light = level.get(1 - dark).copied().unwrap_or(255.0);
    let tolerance = HATCH_EMPTY_TOLERANCE * (light - typical).max(0.0);
    let mut data = plane.data.clone();
    for &r in &dark_squares {
        if blocks(r).iter().any(|b| (b - typical).abs() > tolerance) {
            continue;
        }
        let (x0, y0, x1, y1) = r;
        for y in y0..y1 {
            let start = y * w;
            if let Some(out) = data.get_mut(start + x0..start + x1) {
                out.fill(typical as f32);
            }
        }
    }
    to_gray(&Plane { data, ..plane })
}

/// Mean level of the pixels of `plane` in `(x0, y0, x1, y1)`.
fn block_mean(plane: &Plane, (x0, y0, x1, y1): (usize, usize, usize, usize)) -> f64 {
    let mut sum = 0.0_f64;
    for y in y0..y1 {
        sum += plane.row(y).get(x0..x1).unwrap_or(&[]).iter().map(|&v| f64::from(v)).sum::<f64>();
    }
    sum / ((x1.saturating_sub(x0)) * (y1.saturating_sub(y0))).max(1) as f64
}

/// The board after CLAHE ([`consts::CLAHE_TILES`] tiles a side, clip
/// [`consts::CLAHE_CLIP`]); unchanged when it is smaller than the tiles.
pub fn equalize(board: &GrayImage) -> GrayImage {
    clahe(board, CLAHE_TILES, CLAHE_CLIP).unwrap_or_else(|| board.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::checker_score;

    const LIGHT: f64 = 230.0;
    const DARK: f64 = 120.0;

    /// A `side`-pixel board of 8 × 8 squares on paper, turned `degrees` counter-clockwise about
    /// its centre and shrunk by `scale` (so its corners stay inside), each pixel's level from
    /// `level(x, y, dark)`.
    fn board(side: usize, degrees: f64, scale: f64, level: impl Fn(usize, usize, bool) -> f64) -> GrayImage {
        let (sin, cos) = degrees.to_radians().sin_cos();
        let c = side as f64 / 2.0;
        let mut data = Vec::with_capacity(side * side);
        for y in 0..side {
            for x in 0..side {
                // The source of the turned pixel: its offset turned back, then scaled up.
                let (dx, dy) = (x as f64 + 0.5 - c, y as f64 + 0.5 - c);
                let (u, v) = ((dx * cos - dy * sin) / scale + c, (dx * sin + dy * cos) / scale + c);
                let inside = (0.0..side as f64).contains(&u) && (0.0..side as f64).contains(&v);
                let value = if inside {
                    let (column, row) = ((u * 8.0 / side as f64) as usize, (v * 8.0 / side as f64) as usize);
                    level(x, y, (row + column) % 2 == 1)
                } else {
                    255.0
                };
                data.push(value.round().clamp(0.0, 255.0) as u8);
            }
        }
        GrayImage::new(side as u32, side as u32, data).unwrap()
    }

    fn plain(_: usize, _: usize, dark: bool) -> f64 {
        if dark { DARK } else { LIGHT }
    }

    /// Mean level of the squares of one colour in columns `columns` (0..8).
    fn mean_of(image: &GrayImage, dark: bool, columns: std::ops::Range<usize>) -> f64 {
        let side = image.width() as usize;
        let cell = side / 8;
        let mut values = Vec::new();
        for row in 0..8 {
            for column in columns.clone() {
                if ((row + column) % 2 == 1) != dark {
                    continue;
                }
                // The middle half of the square.
                for y in row * cell + cell / 4..row * cell + 3 * cell / 4 {
                    for x in column * cell + cell / 4..column * cell + 3 * cell / 4 {
                        values.push(f64::from(image.get(x as u32, y as u32).unwrap()));
                    }
                }
            }
        }
        values.iter().sum::<f64>() / values.len() as f64
    }

    #[test]
    fn steps_are_named_by_letters() {
        assert_eq!(Steps::from_letters("0"), Some(Steps::NONE));
        assert_eq!(Steps::from_letters(""), Some(Steps::NONE));
        assert_eq!(Steps::from_letters("dfhc"), Some(Steps { deskew: true, flat_field: true, hatch: true, clahe: true }));
        assert_eq!(Steps::from_letters("x"), None);
        let retry = Preprocessing::from_letters("c>h").unwrap();
        assert_eq!(retry, Preprocessing { first: Steps { clahe: true, ..Steps::NONE }, retry: Steps { hatch: true, ..Steps::NONE } });
        assert_eq!(Preprocessing::from_letters("0>q"), None);
        assert_eq!(Preprocessing::DEFAULT, Preprocessing::from_letters("0>h").unwrap());
    }

    #[test]
    fn a_slightly_turned_board_is_turned_back() {
        let turned = board(400, 2.0, 0.9, plain);
        let angle = estimate_skew(&turned);
        // The board was turned 2° counter-clockwise: undoing it is a turn of −2°, to the step.
        assert!((angle + 2.0).abs() <= SKEW_STEP_DEGREES, "{angle}");
        // Turned back, it is much nearer the board drawn straight.
        let upright = board(400, 0.0, 0.9, plain);
        let distance = |a: &GrayImage| a.data().iter().zip(upright.data()).map(|(x, y)| f64::from(x.abs_diff(*y))).sum::<f64>() / 160_000.0;
        let straight = deskew(&turned);
        assert!(distance(&straight) < distance(&turned) / 3.0, "{} {}", distance(&straight), distance(&turned));
        // A straight board is left exactly as it is, and so is a blank one.
        assert_eq!(estimate_skew(&upright), 0.0);
        assert_eq!(deskew(&upright), upright);
        let blank = GrayImage::new(64, 64, vec![200; 64 * 64]).unwrap();
        assert_eq!(deskew(&blank), blank);
        // Turned further than the search goes, the turn found stays within it.
        assert!(estimate_skew(&board(400, 8.0, 0.8, plain)).abs() <= MAX_SKEW_DEGREES);
    }

    #[test]
    fn flat_field_evens_out_uneven_lighting() {
        // Light falling off from left to right, to 60 % at the right edge.
        let dim = |x: usize, y: usize, dark: bool| plain(x, y, dark) * (1.0 - 0.4 * x as f64 / 400.0);
        let lit = board(400, 0.0, 1.0, dim);
        let ratio = |image: &GrayImage| mean_of(image, false, 6..8) / mean_of(image, false, 0..2);
        assert!(ratio(&lit) < 0.7, "{}", ratio(&lit));
        let even = flat_field(&lit);
        assert!(ratio(&even) > 0.85, "{}", ratio(&even));
        // The squares keep their contrast.
        assert!(mean_of(&even, false, 0..8) - mean_of(&even, true, 0..8) > 60.0);
        // An evenly lit board barely changes.
        let flat = board(400, 0.0, 1.0, plain);
        let out = flat_field(&flat);
        let drift = flat.data().iter().zip(out.data()).map(|(a, b)| f64::from(a.abs_diff(*b))).sum::<f64>() / flat.data().len() as f64;
        assert!(drift < 8.0, "{drift}");
        // Too small to estimate a background: unchanged; black stays black.
        let tiny = GrayImage::new(1, 1, vec![0]).unwrap();
        assert_eq!(flat_field(&tiny), tiny);
    }

    #[test]
    fn hatched_empty_dark_squares_become_flat() {
        // Dark squares hatched with diagonal lines 12 px apart, as in a 1950s book; a black
        // block (a piece) on one dark square.
        let hatched = |x: usize, y: usize, dark: bool| {
            // b6: column 1, row 2.
            let piece = (100..200).contains(&x) && (200..300).contains(&y);
            if piece && (30..70).contains(&(x % 100)) && (20..80).contains(&(y % 100)) {
                10.0
            } else if dark {
                if (x + y) % 12 < 5 { 20.0 } else { 245.0 }
            } else {
                245.0
            }
        };
        let image = board(800, 0.0, 1.0, hatched);
        let out = suppress_hatch(&image);
        // An empty dark square (b8) is one flat gray, the hatch's average.
        let square = |image: &GrayImage, column: u32, row: u32| -> Vec<u8> {
            (row * 100 + 5..row * 100 + 95)
                .flat_map(|y| (column * 100 + 5..column * 100 + 95).map(move |x| (x, y)))
                .map(|(x, y)| image.get(x, y).unwrap())
                .collect()
        };
        let b8 = square(&out, 1, 0);
        assert!(b8.iter().all(|&v| v == b8[0]), "{:?}", &b8[..20]);
        assert!((100..160).contains(&b8[0]), "{}", b8[0]);
        // The square with the piece, and every light square, are left alone.
        assert_eq!(square(&out, 1, 2), square(&image, 1, 2), "the piece's square");
        assert_eq!(square(&out, 0, 0), square(&image, 0, 0));
        // A board whose dark squares are flat is not touched.
        let clean = board(800, 0.0, 1.0, plain);
        assert_eq!(suppress_hatch(&clean), clean);
        let small = GrayImage::new(16, 16, vec![0; 256]).unwrap();
        assert_eq!(suppress_hatch(&small), small);
    }

    #[test]
    fn clahe_keeps_the_board_size_and_raises_a_faint_board() {
        let faint = |_: usize, _: usize, dark: bool| if dark { 150.0 } else { 170.0 };
        let image = board(400, 0.0, 1.0, faint);
        let out = equalize(&image);
        assert_eq!((out.width(), out.height()), (image.width(), image.height()));
        assert!(checker_score(&out) > checker_score(&image), "{} {}", checker_score(&out), checker_score(&image));
        let tiny = GrayImage::new(4, 4, vec![9; 16]).unwrap();
        assert_eq!(equalize(&tiny), tiny);
    }

    #[test]
    fn preprocessing_with_no_step_is_the_identity() {
        let image = board(200, 1.0, 0.95, plain);
        assert_eq!(preprocess(&image, Steps::NONE), image);
        // Every step on a 1 × 1 board leaves it as it is, without a panic.
        let dot = GrayImage::new(1, 1, vec![77]).unwrap();
        assert_eq!(preprocess(&dot, Steps::from_letters("dfhc").unwrap()), dot);
    }
}
