//! Board reading: crop, normalise, cut into 64 squares and classify with the piece model (blueprint S7b, S8).
//!
//! S7b brings the geometry the reader starts from:
//! - [`warp`] turns a detected quad (four corners on the page raster) into an upright square
//!   board, [`consts::BOARD_SIZE`] pixels a side by default, so S8 can cut it into 64 squares;
//! - [`fit_inner_board`] and [`tighten`] trim the frame a contour sometimes includes, so the
//!   8 × 8 grid sits on the squares;
//! - [`checker_score`] and [`texture_score`] measure how much a crop looks like a board. The
//!   contour detector ranks and filters its quads with them.
//!
//! S8 reads the boards: [`read`] turns a page's diagram candidates into positions (see
//! [`reading`]). [`preprocess`] holds the whole-board steps measured before the squares are cut
//! (deskew, flat-field, hatch suppression, CLAHE) and says which the reader runs.
//!
//! Ported from the Python prototype `ChessVisionOFF_Puro` (`board_detection.py`, relicensed
//! MIT/Apache by its author), with its measured constants in [`consts`].

pub mod preprocess;
pub mod reading;

pub use preprocess::{Preprocessing, Steps, preprocess};
pub use reading::{BoardReading, ReadPosition, ReadingSource, SquareClassifier, Unreadable, board_image, read, read_with, squares};

use crate::coords::PixelRect;
use crate::imageops::{Plane, dims, resize_area, resize_bilinear, to_gray, to_plane, warp_plane};
use crate::raster::GrayImage;

/// The reader's constants. The measured ones come from the prototype and say what they
/// separate; the rest are safety caps and say so.
pub mod consts {
    /// Side of a warped board, in pixels (`BOARD_SIZE`): 100 px per square.
    pub const BOARD_SIZE: u32 = 800;

    /// Most pixels a side [`super::warp`] produces. A safety cap, not a measurement: a larger
    /// request is refused rather than allocated.
    pub const MAX_WARP_SIDE: u32 = 4096;

    /// Side of the warp the texture is measured on (`_contour_candidates` warps to 320) and of
    /// the image it is then averaged down to (`_small_gray`: 160, 20 px per square, where both
    /// parts of the texture were calibrated).
    pub const TEXTURE_WARP_SIDE: usize = 320;
    pub const TEXTURE_SIDE: usize = 160;
    pub const TEXTURE_CELL: usize = TEXTURE_SIDE / 8;

    /// Checker contrast is `clip(contrast × 2.4 − spread × 0.9, 0, 1)` (`_checker_score`). It is
    /// exactly 0 wherever there is no 8 × 8 board under the crop: there the difference between
    /// the two square colours is noise and the spread within each colour outweighs it. The
    /// prototype's floor sits at that 0 (S-143).
    pub const CHECKER_CONTRAST_GAIN: f64 = 2.4;
    pub const CHECKER_SPREAD_PENALTY: f64 = 0.9;

    /// Texture is `0.6 × checker + 0.4 × grid` (`_texture_from_parts`). The grid part alone is
    /// imitated by picture frames and photo strips (0.43–0.80 measured on the S-143 pages), so
    /// it only ever weighs in, never decides.
    pub const TEXTURE_CHECKER_WEIGHT: f64 = 0.6;
    pub const TEXTURE_GRID_WEIGHT: f64 = 0.4;

    /// The grid part compares the mean peak near each expected line with these percentiles of
    /// the gradient profile (`_periodic_peak_score`): the 55th is the baseline, the 90th minus
    /// it the spread.
    pub const GRID_BASELINE_PERCENTILE: f64 = 55.0;
    pub const GRID_SPREAD_PERCENTILE: f64 = 90.0;

    /// Least half-width, in pixels, of the window searched around each expected grid line
    /// (`max(2, period // 8)`).
    pub const GRID_MIN_PEAK_RADIUS: usize = 2;

    /// How far inside the crop, as a share of its side, the board's edge may sit
    /// (`INNER_GRID_MAX_INSET`).
    pub const INNER_GRID_MAX_INSET: f64 = 0.12;

    /// A fitted grid must respond this many times more than the uniform one before its axis is
    /// even considered (`INNER_GRID_MIN_GAIN`). The cheap first filter.
    pub const INNER_GRID_MIN_GAIN: f64 = 1.15;

    /// A tightened crop is kept only when its checker contrast rises by at least 0.10 **and** by
    /// at least 30 % (`INNER_GRID_MIN_CHECKER_GAIN`). Edge energy alone accepted 2–13 px trims
    /// of crops that were already right and cost 6 exact boards on the field set (103 → 97);
    /// on double-framed crops the contrast doubles (Koblenz 0.31–0.39 → 0.58–0.67).
    pub const INNER_GRID_MIN_CHECKER_GAIN: f64 = 0.10;
    pub const INNER_GRID_MIN_CHECKER_RATIO: f64 = 1.3;

    /// Width of the box filter that smooths the edge profiles (`_edge_profile`: 5 px).
    pub const EDGE_PROFILE_SMOOTHING: usize = 5;

    /// Smallest board side, in pixels, [`super::fit_inner_board`] works on: below it the
    /// smoothing and the inset search have nothing to work with. A guard, not a measurement.
    pub const MIN_FIT_SIDE: usize = 16;
}

use consts::*;

/// Four points on a raster, `(x, y)` in pixels (y down).
pub type Quad = [(f32, f32); 4];

/// The quad's corners in the order top-left, top-right, bottom-right, bottom-left, never
/// repeating one (`order_quad_points`, S-363). `None` when a coordinate is not finite.
///
/// The corners go clockwise by their angle around the centre, starting from the one with the
/// smallest `x + y`. The old rule (smallest sum is top-left, smallest difference top-right)
/// picked one point twice on a quad turned 45°.
pub fn order_quad(quad: Quad) -> Option<Quad> {
    if quad.iter().any(|(x, y)| !(x.is_finite() && y.is_finite())) {
        return None;
    }
    let (cx, cy) = quad.iter().fold((0.0_f64, 0.0_f64), |(sx, sy), &(x, y)| (sx + f64::from(x) / 4.0, sy + f64::from(y) / 4.0));
    let mut by_angle = quad;
    // A stable sort, as the prototype's `argsort(kind="stable")`; with y down, a rising angle is clockwise.
    by_angle.sort_by(|a, b| angle(*a, cx, cy).total_cmp(&angle(*b, cx, cy)));
    let start = (0..4).min_by(|&a, &b| sum(&by_angle, a).total_cmp(&sum(&by_angle, b)))?;
    by_angle.rotate_left(start);
    Some(by_angle)
}

fn angle((x, y): (f32, f32), cx: f64, cy: f64) -> f64 {
    (f64::from(y) - cy).atan2(f64::from(x) - cx)
}

fn sum(quad: &Quad, i: usize) -> f32 {
    quad.get(i).map_or(f32::INFINITY, |(x, y)| x + y)
}

/// The quad as f64 points for the warp, after ordering.
fn warp_corners(quad: Quad) -> Option<[[f64; 2]; 4]> {
    Some(order_quad(quad)?.map(|(x, y)| [f64::from(x), f64::from(y)]))
}

/// The `size` × `size` board in which the quad's corners (`quad_px`, pixels of `page`, in any
/// order) become the square's corners: a perspective warp, bilinear, black outside the page
/// (`warp_from_quad`). [`consts::BOARD_SIZE`] is the size S8 reads.
///
/// `None` for a degenerate quad (three corners in a line), a non-finite coordinate, size 0 or a
/// size above [`consts::MAX_WARP_SIDE`].
pub fn warp(page: &GrayImage, quad_px: Quad, size: u32) -> Option<GrayImage> {
    if size == 0 || size > MAX_WARP_SIDE {
        return None;
    }
    to_gray(&warp_plane(page, warp_corners(quad_px)?, usize::try_from(size).ok()?)?)
}

/// Checker contrast and grid regularity of the board under `quad_px` on `page`, measured as the
/// detector does: a 320 px warp averaged down to 160 px. `None` for a degenerate quad.
pub(crate) fn quad_texture_parts(page: &GrayImage, quad_px: Quad) -> Option<(f64, f64)> {
    let warped = warp_plane(page, warp_corners(quad_px)?, TEXTURE_WARP_SIDE)?;
    // The prototype rounds the warp to 8 bits before measuring it.
    let warped = Plane { data: warped.data.iter().map(|v| v.round_ties_even().clamp(0.0, 255.0)).collect(), ..warped };
    let small = resize_area(&warped, TEXTURE_SIDE, TEXTURE_SIDE)?;
    Some((checker(&small), grid(&small)))
}

/// Texture from its two parts, in 0..=1 (`_texture_from_parts`).
pub(crate) fn texture_from_parts(checker: f64, grid: f64) -> f64 {
    (TEXTURE_CHECKER_WEIGHT * checker + TEXTURE_GRID_WEIGHT * grid).clamp(0.0, 1.0)
}

/// A board image averaged down to the 160 px texture side.
fn small(board: &GrayImage) -> Option<Plane> {
    resize_area(&to_plane(board), TEXTURE_SIDE, TEXTURE_SIDE)
}

/// How much the squares of one colour differ from those of the other across an 8 × 8 grid laid
/// over the whole image, less the spread within each colour, in 0..=1 (`board_checker_score`).
///
/// It answers "is this a board?", and nothing else imitates it: a photo, a portrait or a table
/// has no 8 × 8 alternation, and the score is exactly 0. A board crowded with pieces lowers it
/// without zeroing it.
pub fn checker_score(board: &GrayImage) -> f64 {
    small(board).map_or(0.0, |s| checker(&s))
}

/// The board-likeness the detector ranks by: `0.6 × checker + 0.4 × grid` in 0..=1
/// (`_board_pattern_score`).
pub fn texture_score(board: &GrayImage) -> f64 {
    small(board).map_or(0.0, |s| texture_from_parts(checker(&s), grid(&s)))
}

/// Checker contrast of a 160 × 160 plane.
fn checker(small: &Plane) -> f64 {
    let mut even = Vec::with_capacity(32);
    let mut odd = Vec::with_capacity(32);
    for row in 0..8 {
        for column in 0..8 {
            let mut total = 0.0_f64;
            for y in row * TEXTURE_CELL..(row + 1) * TEXTURE_CELL {
                let line = small.row(y);
                total += line.get(column * TEXTURE_CELL..(column + 1) * TEXTURE_CELL).unwrap_or(&[]).iter().map(|&v| f64::from(v)).sum::<f64>();
            }
            let mean = total / (TEXTURE_CELL * TEXTURE_CELL) as f64;
            if (row + column) % 2 == 0 { even.push(mean) } else { odd.push(mean) }
        }
    }
    let contrast = (mean(&even) - mean(&odd)).abs() / 255.0;
    let spread = (std_dev(&even) + std_dev(&odd)) / (2.0 * 255.0);
    (contrast * CHECKER_CONTRAST_GAIN - spread * CHECKER_SPREAD_PENALTY).clamp(0.0, 1.0)
}

fn mean(values: &[f64]) -> f64 {
    if values.is_empty() { 0.0 } else { values.iter().sum::<f64>() / values.len() as f64 }
}

/// Population standard deviation (`np.std`, ddof 0).
fn std_dev(values: &[f64]) -> f64 {
    let m = mean(values);
    mean(&values.iter().map(|v| (v - m) * (v - m)).collect::<Vec<_>>()).sqrt()
}

/// How regularly the crop's edges repeat every square, across and down, in 0..=1 (`_grid_score`).
fn grid(small: &Plane) -> f64 {
    let (w, h) = (small.width, small.height);
    let mut across = vec![0.0_f64; w.saturating_sub(1)];
    let mut down = vec![0.0_f64; h.saturating_sub(1)];
    for y in 0..h {
        let row = small.row(y);
        let next = small.row(y + 1);
        for (x, pair) in row.windows(2).enumerate() {
            if let (Some(slot), [a, b]) = (across.get_mut(x), pair) {
                *slot += f64::from((b - a).abs());
            }
        }
        if let Some(slot) = down.get_mut(y) {
            *slot = row.iter().zip(next).map(|(a, b)| f64::from((b - a).abs())).sum::<f64>() / w.max(1) as f64;
        }
    }
    across.iter_mut().for_each(|v| *v /= h.max(1) as f64);
    (periodic_peaks(&across, TEXTURE_CELL) + periodic_peaks(&down, TEXTURE_CELL)) / 2.0
}

/// How far the profile's peaks near the 7 inner grid lines rise above its baseline, as a share
/// of its spread, in 0..=1 (`_periodic_peak_score`).
fn periodic_peaks(profile: &[f64], period: usize) -> f64 {
    if profile.len() <= period {
        return 0.0;
    }
    let radius = (period / 8).max(GRID_MIN_PEAK_RADIUS);
    let peaks: Vec<f64> = (1..8)
        .filter_map(|k| {
            let centre = period * k;
            let window = profile.get(centre.saturating_sub(radius)..(centre + radius + 1).min(profile.len()))?;
            window.iter().copied().reduce(f64::max)
        })
        .collect();
    if peaks.is_empty() {
        return 0.0;
    }
    let baseline = percentile(profile, GRID_BASELINE_PERCENTILE);
    let spread = percentile(profile, GRID_SPREAD_PERCENTILE) - baseline;
    if spread <= 1e-6 {
        return 0.0;
    }
    ((mean(&peaks) - baseline) / spread).clamp(0.0, 1.0)
}

/// The `q`-th percentile with linear interpolation between ranks (numpy's default).
fn percentile(values: &[f64], q: f64) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let Some(last) = sorted.len().checked_sub(1) else { return 0.0 };
    let rank = q / 100.0 * last as f64;
    let (low, high) = (rank.floor() as usize, (rank.ceil() as usize).min(last));
    let (a, b) = (sorted.get(low).copied().unwrap_or(0.0), sorted.get(high).copied().unwrap_or(0.0));
    a + (b - a) * (rank - low as f64)
}

/// Edge energy per column (`across`) or per row, smoothed by a 5 px box (`_edge_profile`).
fn edge_profile(board: &Plane, across: bool) -> Vec<f64> {
    let (w, h) = (board.width, board.height);
    let raw: Vec<f64> = if across {
        let mut sums = vec![0.0_f64; w.saturating_sub(1)];
        for y in 0..h {
            for (slot, pair) in sums.iter_mut().zip(board.row(y).windows(2)) {
                if let [a, b] = pair {
                    *slot += f64::from((b - a).abs());
                }
            }
        }
        sums
    } else {
        (0..h.saturating_sub(1)).map(|y| board.row(y).iter().zip(board.row(y + 1)).map(|(a, b)| f64::from((b - a).abs())).sum()).collect()
    };
    // `np.convolve(mode="same")` with a centred box: zeros beyond either end.
    let half = EDGE_PROFILE_SMOOTHING / 2;
    (0..raw.len())
        .map(|i| raw.get(i.saturating_sub(half)..(i + half + 1).min(raw.len())).unwrap_or(&[]).iter().sum::<f64>() / EDGE_PROFILE_SMOOTHING as f64)
        .collect()
}

/// Where the 8-square grid starts and ends along one axis, with its response and the response
/// of the uniform grid over the whole length (`_fit_grid_1d`): the pair whose nine lines sum the
/// most edge energy, the start within the first [`consts::INNER_GRID_MAX_INSET`] of the length
/// and the end within the last.
fn fit_grid_1d(profile: &[f64]) -> Option<(usize, usize, f64, f64)> {
    let last = profile.len().checked_sub(1)?;
    let length = profile.len() + 1;
    let inset = ((length as f64 * INNER_GRID_MAX_INSET) as usize).max(1);
    let response = |start: usize, end: usize| -> f64 {
        (0..=8_u32)
            .map(|k| {
                let position = start as f64 + (end as f64 - start as f64) * f64::from(k) / 8.0;
                let index = (position.round_ties_even().max(0.0) as usize).min(last);
                profile.get(index).copied().unwrap_or(0.0)
            })
            .sum()
    };
    let ends = length.saturating_sub(inset)..length;
    let mut best: Option<(usize, usize, f64)> = None;
    for start in 0..inset {
        for end in ends.clone() {
            let r = response(start, end);
            if best.is_none_or(|(_, _, b)| r > b) {
                best = Some((start, end, r));
            }
        }
    }
    let (start, end, fitted) = best?;
    Some((start, end, fitted, response(0, length - 1)))
}

/// The board inside a crop that took its frame along, as a pixel box of the crop, or `None` when
/// the crop is already tight (`fit_inner_board`).
///
/// A contour often closes on a diagram's frame; with a double frame the board sits 1–2 % inside
/// it and a uniform 8 × 8 grid over the whole crop misses the squares. Each axis is refitted
/// where the nine grid lines catch the most edge energy, and taken only when the fitted grid
/// responds [`consts::INNER_GRID_MIN_GAIN`] times more than the uniform one.
///
/// `None` as well for a side above [`consts::MAX_WARP_SIDE`]: boards come from [`warp`], and the
/// search is quadratic in the side.
pub fn fit_inner_board(board: &GrayImage) -> Option<PixelRect> {
    let (width, height) = dims(board);
    let max_side = usize::try_from(MAX_WARP_SIDE).unwrap_or(usize::MAX);
    if width < MIN_FIT_SIDE || height < MIN_FIT_SIDE || width > max_side || height > max_side {
        return None;
    }
    let plane = to_plane(board);
    let accept = |(start, end, fitted, uniform): (usize, usize, f64, f64), full: usize| {
        if uniform > 0.0 && fitted >= INNER_GRID_MIN_GAIN * uniform { (start, end) } else { (0, full) }
    };
    let (x0, x1) = accept(fit_grid_1d(&edge_profile(&plane, true))?, width);
    let (y0, y1) = accept(fit_grid_1d(&edge_profile(&plane, false))?, height);
    if (x0, y0, x1, y1) == (0, 0, width, height) {
        return None;
    }
    Some(PixelRect { x0: u32::try_from(x0).ok()?, y0: u32::try_from(y0).ok()?, x1: u32::try_from(x1).ok()?, y1: u32::try_from(y1).ok()? })
}

/// The board cut to [`fit_inner_board`]'s box and stretched back to its size, when that raises
/// its checker contrast enough ([`consts::INNER_GRID_MIN_CHECKER_GAIN`],
/// [`consts::INNER_GRID_MIN_CHECKER_RATIO`]); otherwise a copy of `board` (`tighten_board`). A
/// crop that was already right comes back unchanged, which is why every board can go through it.
pub fn tighten(board: &GrayImage) -> GrayImage {
    tightened(board).unwrap_or_else(|| board.clone())
}

fn tightened(board: &GrayImage) -> Option<GrayImage> {
    let inner = fit_inner_board(board)?;
    let interior = crop(board, inner)?;
    let tight = resize_bilinear(&interior, board.width(), board.height())?;
    let (before, after) = (checker_score(board), checker_score(&tight));
    (after >= before + INNER_GRID_MIN_CHECKER_GAIN && after >= before * INNER_GRID_MIN_CHECKER_RATIO).then_some(tight)
}

/// The pixels of `image` inside `rect`, or `None` for an empty or out-of-bounds box.
fn crop(image: &GrayImage, rect: PixelRect) -> Option<GrayImage> {
    if rect.x1 > image.width() || rect.y1 > image.height() || rect.width() == 0 || rect.height() == 0 {
        return None;
    }
    let (x0, x1) = (usize::try_from(rect.x0).ok()?, usize::try_from(rect.x1).ok()?);
    let mut data = Vec::with_capacity(usize::try_from(rect.width()).ok()?.checked_mul(usize::try_from(rect.height()).ok()?)?);
    for y in rect.y0..rect.y1 {
        data.extend_from_slice(image.row(y)?.get(x0..x1)?);
    }
    GrayImage::new(rect.width(), rect.height(), data).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIGHT: u8 = 235;
    const DARK: u8 = 60;

    /// A `side`-pixel board of 8 × 8 squares inside `margin` pixels of a frame drawn `ink`, with
    /// `gap` pixels of paper between the frame and the board.
    fn framed_board(side: u32, margin: u32, gap: u32) -> GrayImage {
        let total = side + 2 * margin;
        let mut data = vec![255_u8; (total * total) as usize];
        for y in 0..total {
            for x in 0..total {
                let in_board = |v: u32| v >= margin && v < margin + side;
                let value = if in_board(x) && in_board(y) {
                    let (c, r) = ((x - margin) * 8 / side, (y - margin) * 8 / side);
                    if (r + c) % 2 == 1 { DARK } else { LIGHT }
                } else if margin > gap && (x < margin - gap || y < margin - gap || x >= total - margin + gap || y >= total - margin + gap) {
                    20
                } else {
                    255
                };
                data[(y * total + x) as usize] = value;
            }
        }
        GrayImage::new(total, total, data).unwrap()
    }

    #[test]
    fn order_quad_never_repeats_a_corner_on_a_diamond() {
        // The S-363 case: a quad turned 45°, where sums and differences tie.
        let diamond = [(100.0, 0.0), (200.0, 100.0), (100.0, 200.0), (0.0, 100.0)];
        let ordered = order_quad(diamond).unwrap();
        for (i, a) in ordered.iter().enumerate() {
            for b in ordered.iter().skip(i + 1) {
                assert_ne!(a, b, "{ordered:?}");
            }
        }
        // Upright: top-left, top-right, bottom-right, bottom-left, whatever the input order.
        let upright = [(50.0, 60.0), (10.0, 60.0), (50.0, 20.0), (10.0, 20.0)];
        assert_eq!(order_quad(upright), Some([(10.0, 20.0), (50.0, 20.0), (50.0, 60.0), (10.0, 60.0)]));
        // The first corner by angle (just above the centre's left) is not the top-left one: the
        // order still starts at the smallest x + y.
        let leaning = [(0.0, 95.0), (60.0, 0.0), (200.0, 105.0), (80.0, 200.0)];
        assert_eq!(order_quad(leaning), Some([(60.0, 0.0), (200.0, 105.0), (80.0, 200.0), (0.0, 95.0)]));
        assert_eq!(order_quad([(f32::NAN, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]), None);
    }

    #[test]
    fn warp_straightens_a_board_from_its_corners() {
        let board = framed_board(160, 0, 0);
        let out = warp(&board, [(159.0, 159.0), (0.0, 0.0), (159.0, 0.0), (0.0, 159.0)], 80).unwrap();
        assert_eq!((out.width(), out.height()), (80, 80));
        // Square a8 (light) and b8 (dark), sampled in the middle of each.
        assert_eq!(out.get(5, 5), Some(LIGHT));
        assert_eq!(out.get(15, 5), Some(DARK));
        assert!(checker_score(&out) > 0.9, "{}", checker_score(&out));
        // Outside the page is black, also as the far neighbour of a sample half a pixel off it:
        // source x runs −10..9 in half-pixel steps across a white page.
        let white = GrayImage::new(20, 20, vec![255; 400]).unwrap();
        let edge = warp(&white, [(-10.0, 0.0), (9.0, 0.0), (9.0, 19.0), (-10.0, 19.0)], 39).unwrap();
        assert_eq!((edge.get(0, 20), edge.get(20, 20), edge.get(38, 20)), (Some(0), Some(255), Some(255)));
        // Half black, half white (127.5, rounded either way by the solver's last bit).
        assert!(matches!(edge.get(19, 20), Some(127..=128)), "{:?}", edge.get(19, 20));
        assert!(warp(&board, [(0.0, 0.0); 4], 80).is_none());
        assert!(warp(&board, [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)], 0).is_none());
        assert!(warp(&board, [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)], MAX_WARP_SIDE + 1).is_none());
    }

    #[test]
    fn checker_tells_a_board_from_a_table_and_a_blank() {
        let board = framed_board(160, 0, 0);
        assert!(checker_score(&board) > 0.9);
        // Full checker contrast; the grid part is 0 on a clean board, whose gradient is zero at
        // more than 90 % of the columns (the prototype's percentiles behave the same).
        assert!((texture_score(&board) - TEXTURE_CHECKER_WEIGHT).abs() < 1e-9, "{}", texture_score(&board));
        // A table: the same grid lines, every cell the same colour.
        let mut table = vec![255_u8; 160 * 160];
        for y in 0..160 {
            for x in 0..160 {
                if x % 20 < 2 || y % 20 < 2 {
                    table[y * 160 + x] = 0;
                }
            }
        }
        let table = GrayImage::new(160, 160, table).unwrap();
        assert_eq!(checker_score(&table), 0.0);
        assert_eq!(checker_score(&GrayImage::new(160, 160, vec![128; 160 * 160]).unwrap()), 0.0);
        // A difference between the colours smaller than the spread within each is noise: 0.
        let mut noisy = vec![0_u8; 160 * 160];
        for (i, v) in noisy.iter_mut().enumerate() {
            let (row, column) = (i / 160 / 20, i % 160 / 20);
            let parity = ((row + column) % 2) as u8;
            *v = 70 + 10 * parity + if (row * 8 + column) % 4 < 2 { 100 } else { 0 };
        }
        assert_eq!(checker_score(&GrayImage::new(160, 160, noisy).unwrap()), 0.0);
        assert_eq!(texture_score(&GrayImage::new(1, 1, vec![0]).unwrap()), 0.0);
    }

    #[test]
    fn periodic_peaks_rise_where_the_grid_lines_are_expected() {
        // A noisy profile with tall peaks every 20 entries scores 1; a flat one and one shorter
        // than a period score 0.
        let mut profile: Vec<f64> = (0..159).map(|i| f64::from((i * 7919) % 13) / 13.0).collect();
        for k in 1..8 {
            profile[20 * k] = 5.0;
        }
        assert_eq!(periodic_peaks(&profile, 20), 1.0);
        let shifted: Vec<f64> = (0..159).map(|i| if i % 20 == 10 { 5.0 } else { f64::from((i * 7919) % 13) / 13.0 }).collect();
        assert!(periodic_peaks(&shifted, 20) < 0.9, "{}", periodic_peaks(&shifted, 20));
        assert_eq!(periodic_peaks(&[3.0; 159], 20), 0.0);
        assert_eq!(periodic_peaks(&[3.0; 20], 20), 0.0);
    }

    #[test]
    fn percentile_interpolates_between_ranks() {
        let values = [4.0, 1.0, 3.0, 2.0];
        assert_eq!(percentile(&values, 0.0), 1.0);
        assert_eq!(percentile(&values, 100.0), 4.0);
        assert!((percentile(&values, 55.0) - 2.65).abs() < 1e-12);
        assert_eq!(percentile(&[], 50.0), 0.0);
    }

    #[test]
    fn fit_inner_board_finds_the_board_inside_a_double_frame() {
        // A 400 px board inside a frame 20 px out, with 8 px of paper between them.
        let image = framed_board(400, 20, 8);
        let inner = fit_inner_board(&image).expect("the frame is off the grid");
        // The board's edges are at 20 and 420. The 5 px smoothing spreads each line over a
        // plateau and the first maximum is its left end, so the fit lands up to 3 px early.
        for (got, want) in [(inner.x0, 20), (inner.y0, 20), (inner.x1, 420), (inner.y1, 420)] {
            assert!(got.abs_diff(want) <= 3, "{inner:?}");
        }
        let tight = tighten(&image);
        assert_eq!((tight.width(), tight.height()), (image.width(), image.height()));
        assert!(checker_score(&tight) > checker_score(&image) + INNER_GRID_MIN_CHECKER_GAIN);
    }

    /// A table of empty cells inside a double frame: the grid fit moves inside the frame, but the
    /// checker contrast stays 0, so the crop is not tightened.
    #[test]
    fn a_fit_that_does_not_raise_the_checker_contrast_is_refused() {
        let mut image = framed_board(400, 20, 8);
        let width = image.width();
        let mut data = image.data().to_vec();
        for (i, v) in data.iter_mut().enumerate() {
            let (x, y) = (i as u32 % width, i as u32 / width);
            let inside = |v: u32| (20..420).contains(&v);
            if inside(x) && inside(y) {
                *v = if (x - 20) % 50 < 2 || (y - 20) % 50 < 2 { 0 } else { 255 };
            }
        }
        image = GrayImage::new(width, width, data).unwrap();
        assert!(fit_inner_board(&image).is_some(), "premise: the fit moves");
        assert_eq!(tighten(&image), image);
    }

    #[test]
    fn a_tight_board_is_left_alone() {
        let image = framed_board(400, 0, 0);
        assert_eq!(fit_inner_board(&image), None, "the uniform grid is already the best fit");
        assert_eq!(tighten(&image), image);
        assert_eq!(fit_inner_board(&GrayImage::new(8, 8, vec![0; 64]).unwrap()), None);
        assert_eq!(tighten(&GrayImage::new(1, 1, vec![0]).unwrap()).width(), 1);
    }

    /// The grid search is quadratic in the side: a long strip within the gray-image cap would
    /// hang it, so a side above the warp cap is refused, even with a frame to trim.
    #[test]
    fn a_board_larger_than_any_warp_is_not_fitted() {
        let side = MAX_WARP_SIDE - 400 + 1;
        let image = framed_board(side, 200, 80);
        assert!(image.width() > MAX_WARP_SIDE);
        assert_eq!(fit_inner_board(&image), None);
    }
}
