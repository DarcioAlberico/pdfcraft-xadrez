//! The 64 squares inside a crop, found by where an 8 × 8 grid alternates most.
//!
//! A detector's crop can take along what is printed around the board: the frame, the paper
//! between the frame and the squares, and the coordinates (`a`–`h` under the board, `1`–`8`
//! beside it, often between the frame and an outer border). [`super::fit_inner_board`] fits the
//! grid to edge energy, which works for drawn grid lines and clean squares; on hatched or
//! stippled squares every square is all edges, and a strip of letters is as busy as a rank of
//! pieces. What no margin imitates is the board's own alternation: an 8 × 8 grid laid exactly on
//! the squares gives the largest difference between the two colours' means, and every shift
//! puts some paper, frame or letters into the cells and lowers it. So each side of the grid is
//! moved to where the checker contrast (the same measure as [`super::checker_score`], before its
//! clip to 0..=1) is highest.

use crate::coords::PixelRect;
use crate::imageops::{Plane, dims, resize_area, to_plane};
use crate::raster::GrayImage;

use super::consts::{CHECKER_CONTRAST_GAIN, CHECKER_SPREAD_PENALTY, INNER_GRID_MAX_INSET, MAX_WARP_SIDE, MIN_FIT_SIDE};

/// The fit's constants.
pub mod consts {
    /// Side, in pixels, of the image the grid is fitted on: 25 px a square, finer than any margin
    /// worth trimming (one pixel here is 4 on the 800 px board), and small enough for the search.
    pub const FIT_SIDE: usize = 200;

    /// The search tries every second position of each side first, then every position within
    /// this many of the best.
    pub const COARSE_STEP: usize = 2;
    pub const FINE_RADIUS: usize = 2;
}

use consts::*;

/// Summed-area table of a plane: the sum of any box in four lookups.
struct Integral {
    stride: usize,
    sums: Vec<f64>,
}

impl Integral {
    fn new(plane: &Plane) -> Option<Integral> {
        let stride = plane.width.checked_add(1)?;
        let mut sums = vec![0.0_f64; stride.checked_mul(plane.height.checked_add(1)?)?];
        for y in 0..plane.height {
            let mut run = 0.0_f64;
            for (x, &v) in plane.row(y).iter().enumerate() {
                run += f64::from(v);
                let above = sums.get(y * stride + x + 1).copied().unwrap_or(0.0);
                if let Some(slot) = sums.get_mut((y + 1) * stride + x + 1) {
                    *slot = above + run;
                }
            }
        }
        Some(Integral { stride, sums })
    }

    fn at(&self, x: usize, y: usize) -> f64 {
        self.sums.get(y.saturating_mul(self.stride).saturating_add(x)).copied().unwrap_or(0.0)
    }

    /// Mean of the box `x0..x1` × `y0..y1`; 0 for an empty box.
    fn mean(&self, x0: usize, x1: usize, y0: usize, y1: usize) -> f64 {
        let area = x1.saturating_sub(x0).saturating_mul(y1.saturating_sub(y0));
        if area == 0 {
            return 0.0;
        }
        (self.at(x1, y1) - self.at(x0, y1) - self.at(x1, y0) + self.at(x0, y0)) / area as f64
    }
}

/// The 9 grid lines from `start` to `end`, rounded to whole pixels.
fn lines(start: usize, end: usize) -> [usize; 9] {
    let span = end.saturating_sub(start) as f64;
    std::array::from_fn(|k| start + (span * k as f64 / 8.0).round() as usize)
}

/// Checker contrast of the 8 × 8 grid over `x0..x1` × `y0..y1`, unclipped: the difference of the
/// two colours' means less the spread within each, as [`super::checker_score`] weighs them.
fn contrast(integral: &Integral, (x0, x1): (usize, usize), (y0, y1): (usize, usize)) -> f64 {
    let (xs, ys) = (lines(x0, x1), lines(y0, y1));
    let mut sums = [0.0_f64; 2];
    let mut squares = [0.0_f64; 2];
    for (row, band) in ys.windows(2).enumerate() {
        let [top, bottom] = [band.first().copied().unwrap_or(0), band.get(1).copied().unwrap_or(0)];
        for (column, span) in xs.windows(2).enumerate() {
            let [left, right] = [span.first().copied().unwrap_or(0), span.get(1).copied().unwrap_or(0)];
            let m = integral.mean(left, right, top, bottom);
            let colour = (row + column) % 2;
            if let (Some(s), Some(q)) = (sums.get_mut(colour), squares.get_mut(colour)) {
                *s += m;
                *q += m * m;
            }
        }
    }
    // 32 squares of each colour; population standard deviation.
    let [even, odd] = sums.map(|s| s / 32.0);
    let spread = |sum_sq: f64, m: f64| (sum_sq / 32.0 - m * m).max(0.0).sqrt();
    let deviation = spread(squares[0], even) + spread(squares[1], odd);
    let contrast = (even - odd).abs() / 255.0;
    contrast * CHECKER_CONTRAST_GAIN - deviation / (2.0 * 255.0) * CHECKER_SPREAD_PENALTY
}

/// A grid's box in the fit plane: left, right, top, bottom.
type Box4 = (usize, usize, usize, usize);

/// The box, in the fit plane, whose grid alternates most, each side within
/// [`INNER_GRID_MAX_INSET`] of the plane's edge, with its contrast.
fn search(integral: &Integral) -> Option<(Box4, f64)> {
    let side = FIT_SIDE;
    let inset = ((side as f64 * INNER_GRID_MAX_INSET) as usize).max(1);
    let starts = || (0..=inset).step_by(COARSE_STEP);
    let ends = || (side - inset..=side).step_by(COARSE_STEP);
    let mut best: Option<(Box4, f64)> = None;
    let consider = |b: Box4, best: &mut Option<(Box4, f64)>| {
        let c = contrast(integral, (b.0, b.1), (b.2, b.3));
        if best.is_none_or(|(_, top)| c > top) {
            *best = Some((b, c));
        }
    };
    for x0 in starts() {
        for x1 in ends() {
            for y0 in starts() {
                for y1 in ends() {
                    consider((x0, x1, y0, y1), &mut best);
                }
            }
        }
    }
    let ((cx0, cx1, cy0, cy1), _) = best?;
    let around = |v: usize, low: usize, high: usize| v.saturating_sub(FINE_RADIUS).max(low)..=(v + FINE_RADIUS).min(high);
    for x0 in around(cx0, 0, inset) {
        for x1 in around(cx1, side - inset, side) {
            for y0 in around(cy0, 0, inset) {
                for y1 in around(cy1, side - inset, side) {
                    consider((x0, x1, y0, y1), &mut best);
                }
            }
        }
    }
    best
}

/// The squares inside a crop, as a pixel box of it, with the fitted grid's contrast and the whole
/// crop's; `None` when the image is too small or too large to fit, or the best grid is the whole
/// crop.
pub fn fit_checker_board(board: &GrayImage) -> Option<(PixelRect, f64, f64)> {
    let (width, height) = dims(board);
    let max_side = usize::try_from(MAX_WARP_SIDE).unwrap_or(usize::MAX);
    if width < MIN_FIT_SIDE || height < MIN_FIT_SIDE || width > max_side || height > max_side {
        return None;
    }
    let plane = resize_area(&to_plane(board), FIT_SIDE, FIT_SIDE)?;
    let integral = Integral::new(&plane)?;
    let whole = contrast(&integral, (0, FIT_SIDE), (0, FIT_SIDE));
    let ((x0, x1, y0, y1), fitted) = search(&integral)?;
    if (x0, x1, y0, y1) == (0, FIT_SIDE, 0, FIT_SIDE) {
        return None;
    }
    let scale = |v: usize, length: usize| u32::try_from((v as f64 * length as f64 / FIT_SIDE as f64).round() as usize).ok();
    let rect = PixelRect { x0: scale(x0, width)?, x1: scale(x1, width)?, y0: scale(y0, height)?, y1: scale(y1, height)? };
    Some((rect, fitted, whole))
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    const LIGHT: u8 = 235;
    const DARK: u8 = 90;

    /// A white `size` × `size` image.
    fn paper(size: u32) -> Vec<u8> {
        vec![255; (size * size) as usize]
    }

    /// An 8 × 8 board of `square` px squares with its top-left corner at `(x, y)`. With `hatch`,
    /// the dark squares are diagonal lines of black on white averaging about `DARK`, as old books
    /// print them.
    fn board(data: &mut [u8], size: u32, (x, y): (u32, u32), square: u32, hatch: bool) {
        for py in y..y + 8 * square {
            for px in x..x + 8 * square {
                let (column, row) = ((px - x) / square, (py - y) / square);
                let dark = (row + column) % 2 == 1;
                data[(py * size + px) as usize] = match (dark, hatch) {
                    (false, _) => LIGHT,
                    (true, false) => DARK,
                    (true, true) if (px + py) % 4 < 2 => 0,
                    (true, true) => 255,
                };
            }
        }
    }

    /// A ring a glyph tall, as a printed `o` or `0` is.
    fn glyph(data: &mut [u8], size: u32, (cx, cy): (u32, u32), side: u32) {
        for py in cy - side / 2..cy + side / 2 {
            for px in cx - side / 2..cx + side / 2 {
                let edge = px < cx - side / 2 + 2 || px + 2 >= cx + side / 2 || py < cy - side / 2 + 2 || py + 2 >= cy + side / 2;
                if edge {
                    data[(py * size + px) as usize] = 0;
                }
            }
        }
    }

    /// A frame line `width` px wide just inside the box `from..to` on both axes.
    fn frame(data: &mut [u8], size: u32, from: u32, to: u32, width: u32) {
        for p in from..to {
            for w in 0..width {
                for (x, y) in [(p, from + w), (p, to - 1 - w), (from + w, p), (to - 1 - w, p)] {
                    data[(y * size + x) as usize] = 0;
                }
            }
        }
    }

    fn image(size: u32, data: Vec<u8>) -> GrayImage {
        GrayImage::new(size, size, data).unwrap()
    }

    /// Asserts each side of `rect` within `tolerance` px of the board `x0..x1` × `y0..y1`.
    fn assert_near(rect: PixelRect, [x0, y0, x1, y1]: [u32; 4], tolerance: u32) {
        for (got, want) in [(rect.x0, x0), (rect.y0, y0), (rect.x1, x1), (rect.y1, y1)] {
            assert!(got.abs_diff(want) <= tolerance, "{rect:?} against {:?}", [x0, y0, x1, y1]);
        }
    }

    /// The case books print: the coordinates in a strip under the board and one left of it,
    /// inside an outer frame. A contour that closes on the frame takes them all along.
    #[test]
    fn the_squares_are_found_inside_printed_coordinates_and_a_frame() {
        let size = 440;
        for hatch in [false, true] {
            let mut data = paper(size);
            board(&mut data, size, (40, 10), 48, hatch);
            for k in 0..8 {
                glyph(&mut data, size, (40 + 48 * k + 24, 10 + 384 + 22), 16);
                glyph(&mut data, size, (22, 10 + 48 * k + 24), 16);
            }
            frame(&mut data, size, 0, size, 4);
            let (rect, fitted, whole) = fit_checker_board(&image(size, data)).expect("the grid moves");
            // One pixel of the fit plane is 2.2 px here.
            assert_near(rect, [40, 10, 424, 394], 3);
            assert!(fitted > whole + 0.1, "{fitted} against {whole}");
        }
    }

    /// The scanned hatched board that started this: the crop takes the frame line and the paper
    /// between it and the squares, a sliver of a square, and the cells drift off the squares.
    #[test]
    fn a_frame_and_its_margin_are_trimmed_off_a_hatched_board() {
        let size = 412;
        let mut data = paper(size);
        board(&mut data, size, (8, 8), 49, true);
        frame(&mut data, size, 0, size, 3);
        let (rect, _, _) = fit_checker_board(&image(size, data)).expect("the grid moves");
        assert_near(rect, [8, 8, 400, 400], 3);
    }

    #[test]
    fn a_tight_board_is_its_own_fit() {
        for hatch in [false, true] {
            let mut data = paper(400);
            board(&mut data, 400, (0, 0), 50, hatch);
            assert_eq!(fit_checker_board(&image(400, data)), None);
        }
    }

    #[test]
    fn images_too_small_or_too_large_are_not_fitted() {
        assert_eq!(fit_checker_board(&GrayImage::new(8, 8, vec![0; 64]).unwrap()), None);
        let wide = GrayImage::new(MAX_WARP_SIDE + 1, 16, vec![255; (MAX_WARP_SIDE as usize + 1) * 16]).unwrap();
        assert_eq!(fit_checker_board(&wide), None);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(32))]
        /// Any picture is fitted or refused, never a panic, and a fit lies inside the image.
        #[test]
        fn any_image_is_fitted_inside_itself(width in 1_u32..120, height in 1_u32..120, seed in any::<u64>()) {
            let data: Vec<u8> = (0..width * height).map(|i| (u64::from(i).wrapping_mul(seed | 1) >> 7) as u8).collect();
            let image = GrayImage::new(width, height, data).unwrap();
            if let Some((rect, fitted, whole)) = fit_checker_board(&image) {
                prop_assert!(rect.x0 < rect.x1 && rect.x1 <= width && rect.y0 < rect.y1 && rect.y1 <= height, "{rect:?}");
                prop_assert!(fitted.is_finite() && whole.is_finite() && fitted >= whole);
            }
        }
    }
}
