//! Board reading: crop, normalise, cut into 64 squares and classify with the piece model (blueprint S7b, S8).
//!
//! S7b brings the geometry the reader starts from:
//! - [`warp`] turns a detected quad (four corners on the page raster) into an upright square
//!   board, [`consts::BOARD_SIZE`] pixels a side by default, so S8 can cut it into 64 squares;
//! - [`checker_score`] and [`texture_score`] measure how much a crop looks like a board. The
//!   contour detector ranks and filters its quads with them.
//!
//! Ported from the Python prototype `ChessVisionOFF_Puro` (`board_detection.py`, relicensed
//! MIT/Apache by its author), with its measured constants in [`consts`].

use crate::imageops::{Plane, resize_area, to_gray, to_plane, warp_plane};
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
}
