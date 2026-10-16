//! Pixel operations shared by the contour detector and the board reader: blur, adaptive
//! threshold, morphology, resizing and perspective warps (blueprint S7b).
//!
//! The Python prototype called OpenCV for each of these; this crate has no OpenCV, so they are
//! written here from OpenCV's documented semantics. Where the result can differ from OpenCV the
//! function says so.

use crate::coords::PixelRect;
use crate::raster::GrayImage;

/// A gray plane of `f32` samples, row-major: the working form of a resized or warped image.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Plane {
    pub width: usize,
    pub height: usize,
    pub data: Vec<f32>,
}

impl Plane {
    /// Row `y`, empty outside the plane.
    pub fn row(&self, y: usize) -> &[f32] {
        let start = y.saturating_mul(self.width);
        self.data.get(start..start.saturating_add(self.width)).unwrap_or(&[])
    }
}

/// The width and height of `image` as `usize`.
pub(crate) fn dims(image: &GrayImage) -> (usize, usize) {
    // A GrayImage holds at most `MAX_GRAY_PIXELS` pixels, so each side fits a usize.
    (usize::try_from(image.width()).unwrap_or(0), usize::try_from(image.height()).unwrap_or(0))
}

/// A gray image from a plane, each sample rounded to the nearest level (halves to even, as
/// OpenCV's `saturate_cast` does) and clamped to 0..=255.
pub(crate) fn to_gray(plane: &Plane) -> Option<GrayImage> {
    let data = plane.data.iter().map(|&v| to_level(v)).collect();
    GrayImage::new(u32::try_from(plane.width).ok()?, u32::try_from(plane.height).ok()?, data).ok()
}

/// A sample rounded and clamped to a gray level; NaN becomes 0.
fn to_level(value: f32) -> u8 {
    let v = value.round_ties_even();
    // NaN fails both comparisons and lands on 0; otherwise the value is within 0..=255.
    if v >= 255.0 {
        255
    } else if v > 0.0 {
        v as u8
    } else {
        0
    }
}

/// The index `i` (which may lie up to a kernel radius outside `0..n`) mirrored back inside
/// without repeating the edge pixel (OpenCV's default `BORDER_REFLECT_101`). Images narrower than
/// the kernel clamp instead of reflecting twice.
fn reflect101(i: isize, n: usize) -> usize {
    let last = n.saturating_sub(1);
    let j = i.unsigned_abs();
    let j = if i >= 0 && j > last { (2 * last).saturating_sub(j) } else { j };
    j.min(last)
}

/// The index `i` clamped inside `0..n` (OpenCV's `BORDER_REPLICATE`).
fn replicate(i: isize, n: usize) -> usize {
    if i < 0 { 0 } else { i.unsigned_abs().min(n.saturating_sub(1)) }
}

/// A 5 × 5 Gaussian blur with OpenCV's fixed kernel for that size and σ = 0 (`[1 4 6 4 1] / 16`
/// in each direction), with the border mirrored (`BORDER_REFLECT_101`). Integer arithmetic,
/// rounded half up: OpenCV's bit-exact 8-bit path rounds the same way.
pub(crate) fn gaussian_blur_5(image: &GrayImage) -> GrayImage {
    const TAPS: [u32; 5] = [1, 4, 6, 4, 1];
    let (width, height) = dims(image);
    let mut across = vec![0_u32; image.data().len()];
    for (row, out) in image.data().chunks(width.max(1)).zip(across.chunks_mut(width.max(1))) {
        for (x, slot) in out.iter_mut().enumerate() {
            *slot = TAPS.iter().zip(-2_isize..).map(|(&w, d)| w * u32::from(row.get(reflect101(x as isize + d, width)).copied().unwrap_or(0))).sum();
        }
    }
    let mut data = vec![0_u8; image.data().len()];
    for (y, out) in data.chunks_mut(width.max(1)).enumerate() {
        let rows: Vec<&[u32]> = (-2_isize..=2).map(|d| reflect101(y as isize + d, height)).map(|r| sub_row(&across, r, width)).collect();
        for (x, slot) in out.iter_mut().enumerate() {
            let sum: u32 = TAPS.iter().zip(&rows).map(|(&w, row)| w * row.get(x).copied().unwrap_or(0)).sum();
            // At most 256 × 255 + 128, then shifted back to 0..=255.
            *slot = u8::try_from((sum + 128) >> 8).unwrap_or(u8::MAX);
        }
    }
    // Same size as the input, so the data fills it.
    GrayImage::new(image.width(), image.height(), data).unwrap_or_else(|_| image.clone())
}

/// Row `y` of a row-major buffer `width` wide, empty outside it.
fn sub_row<T>(data: &[T], y: usize, width: usize) -> &[T] {
    let start = y.saturating_mul(width);
    data.get(start..start.saturating_add(width)).unwrap_or(&[])
}

/// The normalised 1-D Gaussian of `size` taps that OpenCV builds for σ = 0:
/// `σ = 0.3 × ((size − 1) / 2 − 1) + 0.8`.
fn gaussian_kernel(size: usize) -> Vec<f64> {
    let radius = size / 2;
    let sigma = 0.3 * ((size as f64 - 1.0) * 0.5 - 1.0) + 0.8;
    let weights: Vec<f64> = (0..size).map(|i| (-((i as f64 - radius as f64).powi(2)) / (2.0 * sigma * sigma)).exp()).collect();
    let total: f64 = weights.iter().sum();
    weights.into_iter().map(|w| if total > 0.0 { w / total } else { 0.0 }).collect()
}

/// OpenCV's `adaptiveThreshold(ADAPTIVE_THRESH_GAUSSIAN_C, THRESH_BINARY_INV, block, c)`: a pixel
/// is foreground (`true`) when it is at least `c` levels darker than the Gaussian-weighted mean
/// of its `block` × `block` neighbourhood (border replicated, as OpenCV does here). `block` must
/// be odd; an even one is rounded up.
///
/// **Deviation.** The mean is computed in `f64` and rounded to a level (halves to even); OpenCV
/// computes it with its own fixed-point Gaussian, so a pixel exactly at the threshold can fall on
/// the other side. The vertical pass keeps only `block` rows of the horizontal pass, so memory
/// stays at `block` rows rather than a second full-page buffer.
pub(crate) fn adaptive_threshold_inv(image: &GrayImage, block: usize, c: f64) -> Vec<bool> {
    let block = block | 1;
    let radius = block / 2;
    let kernel = gaussian_kernel(block);
    let (width, height) = dims(image);
    let delta = c.floor() as i32;
    let horizontal = |y: usize| -> Vec<f64> {
        let row = image.row(u32::try_from(y).unwrap_or(u32::MAX)).unwrap_or(&[]);
        // The row with `radius` replicated pixels on each side, so every window is a plain slice.
        let padded: Vec<f64> =
            (0..width + 2 * radius).map(|i| f64::from(row.get(replicate(i as isize - radius as isize, width)).copied().unwrap_or(0))).collect();
        padded.windows(block).map(|window| window.iter().zip(&kernel).map(|(v, w)| v * w).sum()).collect()
    };
    // Ring of the last `block` horizontally blurred rows, keyed by row index modulo `block`: the
    // rows one output row needs are consecutive and at most `block` of them, so slots never clash.
    let mut ring: Vec<Vec<f64>> = vec![Vec::new(); block];
    let mut computed = 0_usize;
    let mut mean = vec![0.0_f64; width];
    let mut mask = vec![false; image.data().len()];
    for (y, out) in mask.chunks_mut(width.max(1)).enumerate() {
        let need = (y + radius).min(height.saturating_sub(1));
        while computed <= need {
            if let Some(slot) = ring.get_mut(computed % block) {
                *slot = horizontal(computed);
            }
            computed += 1;
        }
        // Whole rows at a time, so the inner loop is a plain multiply-add over a slice.
        mean.iter_mut().for_each(|m| *m = 0.0);
        for (k, w) in kernel.iter().enumerate() {
            let r = replicate(y as isize + k as isize - radius as isize, height);
            let row = ring.get(r % block).map_or(&[][..], Vec::as_slice);
            for (m, v) in mean.iter_mut().zip(row) {
                *m += w * v;
            }
        }
        let src = image.row(u32::try_from(y).unwrap_or(u32::MAX)).unwrap_or(&[]);
        for ((slot, &value), &m) in out.iter_mut().zip(src).zip(&mean) {
            let m = m.round_ties_even().clamp(0.0, 255.0) as i32;
            *slot = i32::from(value) - m <= -delta;
        }
    }
    mask
}

/// The 3 × 3 square structuring element, as offsets `(dx, dy)` from the anchor.
pub(crate) const SQUARE_3: [(isize, isize); 9] = [(-1, -1), (0, -1), (1, -1), (-1, 0), (0, 0), (1, 0), (-1, 1), (0, 1), (1, 1)];

/// The main diagonal of a 3 × 3 element (`np.eye(3)`).
pub(crate) const DIAGONAL_3: [(isize, isize); 3] = [(-1, -1), (0, 0), (1, 1)];

/// The anti-diagonal of a 3 × 3 element (`np.fliplr(np.eye(3))`).
pub(crate) const ANTI_DIAGONAL_3: [(isize, isize); 3] = [(1, -1), (0, 0), (-1, 1)];

/// OpenCV's `morphologyEx(MORPH_CLOSE)` of a `width`-wide mask with one element given as
/// offsets: dilation, then erosion with the same element. Pixels outside the mask take no part
/// (OpenCV's default border value is neutral for both). All three elements used here are
/// symmetric about the anchor, so the reflection OpenCV skips does not matter.
pub(crate) fn close(mask: &[bool], width: usize, element: &[(isize, isize)]) -> Vec<bool> {
    let dilated = morph(mask, width, element, true);
    morph(&dilated, width, element, false)
}

/// One dilation (`grow`) or erosion pass.
fn morph(mask: &[bool], width: usize, element: &[(isize, isize)], grow: bool) -> Vec<bool> {
    let width = width.max(1);
    let height = mask.len() / width;
    // Dilation starts empty and ORs the neighbours in; erosion starts full and ANDs them. A
    // neighbour outside the mask is skipped, which leaves either unchanged.
    let mut out = vec![!grow; mask.len()];
    for (y, row) in out.chunks_mut(width).enumerate() {
        for &(dx, dy) in element {
            let Some(ny) = y.checked_add_signed(dy).filter(|&v| v < height) else { continue };
            let src = sub_row(mask, ny, width);
            let shift = dx.unsigned_abs().min(width);
            // Output column x reads source column x + dx: slide one slice against the other.
            let (dst, src) =
                if dx >= 0 { (row.get_mut(..width - shift), src.get(shift..)) } else { (row.get_mut(shift..), src.get(..width - shift)) };
            let (Some(dst), Some(src)) = (dst, src) else { continue };
            for (o, s) in dst.iter_mut().zip(src) {
                if grow {
                    *o |= *s;
                } else {
                    *o &= *s;
                }
            }
        }
    }
    out
}

/// The element-wise union of two masks of the same size.
pub(crate) fn union(a: &[bool], b: &[bool]) -> Vec<bool> {
    a.iter().zip(b).map(|(x, y)| *x || *y).collect()
}

/// For each destination index along one axis, the source indices it covers and their weights:
/// a destination pixel spans `scale` source pixels, cut fractionally at both ends.
fn area_taps(src: usize, dst: usize) -> Vec<Vec<(usize, f32)>> {
    let scale = src as f64 / dst.max(1) as f64;
    (0..dst)
        .map(|d| {
            let (start, end) = (d as f64 * scale, (d as f64 + 1.0) * scale);
            let first = start.floor() as usize;
            let last = (end.ceil() as usize).min(src);
            (first..last)
                .filter_map(|s| {
                    let cover = (end.min(s as f64 + 1.0) - start.max(s as f64)).max(0.0);
                    (cover > 1e-9).then_some((s, (cover / scale) as f32))
                })
                .collect()
        })
        .collect()
}

/// `image` resized by area averaging (OpenCV's `INTER_AREA` when shrinking): every destination
/// pixel is the mean of the source pixels it covers, fractional ones weighted by coverage.
/// `None` for an empty destination.
pub(crate) fn resize_area(image: &Plane, width: usize, height: usize) -> Option<Plane> {
    if width == 0 || height == 0 || image.width == 0 || image.height == 0 {
        return None;
    }
    let (taps_x, taps_y) = (area_taps(image.width, width), area_taps(image.height, height));
    let mut across = Vec::with_capacity(width.checked_mul(image.height)?);
    for y in 0..image.height {
        let row = image.row(y);
        across.extend(taps_x.iter().map(|taps| taps.iter().map(|&(s, w)| w * row.get(s).copied().unwrap_or(0.0)).sum::<f32>()));
    }
    let mut data = Vec::with_capacity(width.checked_mul(height)?);
    for taps in &taps_y {
        for x in 0..width {
            data.push(taps.iter().map(|&(s, w)| w * across.get(s * width + x).copied().unwrap_or(0.0)).sum::<f32>());
        }
    }
    Some(Plane { width, height, data })
}

/// A gray image as a plane.
pub(crate) fn to_plane(image: &GrayImage) -> Plane {
    let (width, height) = dims(image);
    Plane { width, height, data: image.data().iter().map(|&v| f32::from(v)).collect() }
}

/// `image` resized bilinearly (OpenCV's `INTER_LINEAR`: pixel centres aligned, border
/// replicated). `None` for an empty destination or one larger than a gray image may be.
pub(crate) fn resize_bilinear(image: &GrayImage, width: u32, height: u32) -> Option<GrayImage> {
    resize_region_bilinear(image, PixelRect { x0: 0, y0: 0, x1: image.width(), y1: image.height() }, width, height)
}

/// The pixels of `image` inside `region` resized bilinearly, as [`resize_bilinear`] resizes a copy
/// of them, but sampled in place: a region as large as the page costs no copy of it. `None` for an
/// empty or out-of-bounds region, an empty destination or one larger than a gray image may be.
pub(crate) fn resize_region_bilinear(image: &GrayImage, region: PixelRect, width: u32, height: u32) -> Option<GrayImage> {
    if region.x1 > image.width() || region.y1 > image.height() || region.width() == 0 || region.height() == 0 {
        return None;
    }
    let (sw, sh) = (usize::try_from(region.width()).ok()?, usize::try_from(region.height()).ok()?);
    let (dw, dh) = (usize::try_from(width).ok()?, usize::try_from(height).ok()?);
    if dw == 0 || dh == 0 || dw.checked_mul(dh)? > crate::raster::MAX_GRAY_PIXELS {
        return None;
    }
    // Source offsets within the region, which lie inside the image (checked above), so the sums fit.
    let axis = |d: usize, dst: usize, src: usize| -> (u32, u32, f32) {
        let pos = ((d as f64 + 0.5) * src as f64 / dst as f64 - 0.5).max(0.0);
        let low = (pos.floor() as usize).min(src.saturating_sub(1));
        let high = (low + 1).min(src.saturating_sub(1));
        (low as u32, high as u32, (pos - low as f64).clamp(0.0, 1.0) as f32)
    };
    let at = |x: u32, y: u32| image.get(region.x0.saturating_add(x), region.y0.saturating_add(y)).map_or(0.0, f32::from);
    let columns: Vec<(u32, u32, f32)> = (0..dw).map(|x| axis(x, dw, sw)).collect();
    let mut data = Vec::with_capacity(dw * dh);
    for y in 0..dh {
        let (y0, y1, fy) = axis(y, dh, sh);
        for &(x0, x1, fx) in &columns {
            let top = at(x0, y0) * (1.0 - fx) + at(x1, y0) * fx;
            let bottom = at(x0, y1) * (1.0 - fx) + at(x1, y1) * fx;
            data.push(top * (1.0 - fy) + bottom * fy);
        }
    }
    to_gray(&Plane { width: dw, height: dh, data })
}

/// The 3 × 3 projective transform (row-major, last entry 1) that takes each `from` point to the
/// matching `to` point, as OpenCV's `getPerspectiveTransform` solves it. `None` when the points
/// are degenerate (three in a line) or not finite.
pub(crate) fn perspective(from: [[f64; 2]; 4], to: [[f64; 2]; 4]) -> Option<[f64; 9]> {
    // Eight equations in the eight unknowns h0..h7, as an augmented 8 × 9 matrix.
    let mut m = [[0.0_f64; 9]; 8];
    for (k, ([x, y], [u, v])) in from.iter().zip(to.iter()).enumerate() {
        if let Some(row) = m.get_mut(2 * k) {
            *row = [*x, *y, 1.0, 0.0, 0.0, 0.0, -x * u, -y * u, *u];
        }
        if let Some(row) = m.get_mut(2 * k + 1) {
            *row = [0.0, 0.0, 0.0, *x, *y, 1.0, -x * v, -y * v, *v];
        }
    }
    let h = solve8(m)?;
    let [a, b, c, d, e, f, g, i] = h;
    let out = [a, b, c, d, e, f, g, i, 1.0];
    out.iter().all(|v| v.is_finite()).then_some(out)
}

/// Gaussian elimination with partial pivoting on an augmented 8 × 9 system.
fn solve8(mut m: [[f64; 9]; 8]) -> Option<[f64; 8]> {
    for col in 0..8 {
        let pivot = (col..8).max_by(|&a, &b| cell(&m, a, col).abs().total_cmp(&cell(&m, b, col).abs()))?;
        if cell(&m, pivot, col).abs() < 1e-12 {
            return None;
        }
        m.swap(col, pivot);
        let lead = *m.get(col)?;
        for (r, row) in m.iter_mut().enumerate() {
            if r == col {
                continue;
            }
            let factor = row.get(col).copied().unwrap_or(0.0) / lead.get(col).copied().unwrap_or(1.0);
            for (value, l) in row.iter_mut().zip(lead.iter()) {
                *value -= factor * l;
            }
        }
    }
    let mut out = [0.0; 8];
    for (k, slot) in out.iter_mut().enumerate() {
        *slot = cell(&m, k, 8) / cell(&m, k, k);
    }
    out.iter().all(|v| v.is_finite()).then_some(out)
}

fn cell(m: &[[f64; 9]; 8], r: usize, c: usize) -> f64 {
    m.get(r).and_then(|row| row.get(c)).copied().unwrap_or(0.0)
}

/// Samples `image` at `(x, y)` bilinearly, with pixels outside it black (OpenCV's
/// `BORDER_CONSTANT` with value 0, the `warpPerspective` default).
fn sample(image: &GrayImage, x: f64, y: f64) -> f32 {
    if !(x.is_finite() && y.is_finite()) || x <= -1.0 || y <= -1.0 || x >= f64::from(image.width()) || y >= f64::from(image.height()) {
        return 0.0;
    }
    let (fx, fy) = (x.floor(), y.floor());
    let (ax, ay) = ((x - fx) as f32, (y - fy) as f32);
    let px = |dx: f64, dy: f64| -> f32 {
        let (sx, sy) = (fx + dx, fy + dy);
        if sx < 0.0 || sy < 0.0 {
            return 0.0;
        }
        // Both are below the image's size (checked above), so they fit a u32; `get` gives no
        // pixel one past the far edge, and that neighbour counts as black.
        image.get(sx as u32, sy as u32).map_or(0.0, f32::from)
    };
    let top = px(0.0, 0.0) * (1.0 - ax) + px(1.0, 0.0) * ax;
    let bottom = px(0.0, 1.0) * (1.0 - ax) + px(1.0, 1.0) * ax;
    top * (1.0 - ay) + bottom * ay
}

/// The `size` × `size` view of `image` in which `quad` (four pixel points, in the order
/// top-left, top-right, bottom-right, bottom-left) becomes the square's corners `(0, 0)`,
/// `(size − 1, 0)`, `(size − 1, size − 1)` and `(0, size − 1)` (OpenCV's `getPerspectiveTransform`
/// plus `warpPerspective`, bilinear, outside black). `None` for a degenerate quad or size 0.
pub(crate) fn warp_plane(image: &GrayImage, quad: [[f64; 2]; 4], size: usize) -> Option<Plane> {
    if size == 0 {
        return None;
    }
    let last = (size - 1) as f64;
    let square = [[0.0, 0.0], [last, 0.0], [last, last], [0.0, last]];
    // Output → source directly, so no inverse is needed.
    let [a, b, c, d, e, f, g, h, i] = perspective(square, quad)?;
    let mut data = Vec::with_capacity(size.checked_mul(size)?);
    for v in 0..size {
        for u in 0..size {
            let (u, v) = (u as f64, v as f64);
            let w = g * u + h * v + i;
            let value = if w.abs() < 1e-12 { 0.0 } else { sample(image, (a * u + b * v + c) / w, (d * u + e * v + f) / w) };
            data.push(value);
        }
    }
    Some(Plane { width: size, height: size, data })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gray(width: u32, height: u32, data: Vec<u8>) -> GrayImage {
        GrayImage::new(width, height, data).unwrap()
    }

    #[test]
    fn reflect101_mirrors_without_repeating_the_edge() {
        assert_eq!([-2, -1, 0, 4, 5, 6].map(|i| reflect101(i, 5)), [2, 1, 0, 4, 3, 2]);
        // Narrower than the kernel: clamped, never outside.
        assert_eq!([-2, -1, 0, 1, 2].map(|i| reflect101(i, 1)), [0, 0, 0, 0, 0]);
        assert_eq!([-2, 3].map(|i| reflect101(i, 2)), [1, 0]);
    }

    #[test]
    fn blur_keeps_a_flat_image_and_spreads_a_dot_binomially() {
        let flat = gaussian_blur_5(&gray(7, 6, vec![90; 42]));
        assert!(flat.data().iter().all(|&v| v == 90));
        let mut dot = vec![0; 81];
        dot[4 * 9 + 4] = 255;
        let blurred = gaussian_blur_5(&gray(9, 9, dot));
        // Centre weight 6 × 6 / 256 of 255, the corners of the 5 × 5 support 1 / 256.
        assert_eq!(blurred.get(4, 4), Some(36));
        assert_eq!(blurred.get(2, 2), Some(1));
        assert_eq!(blurred.get(1, 1), Some(0));
    }

    #[test]
    fn adaptive_threshold_marks_ink_darker_than_its_surroundings() {
        // A 3 px dark line on white, and a flat dark area: only the line is marked, and the
        // threshold needs `c` levels of darkness (a line 5 levels darker is not ink).
        let (w, h) = (60_u32, 60_u32);
        let mut data = vec![255_u8; 3600];
        for y in 0..60 {
            for x in 28..31 {
                data[y * 60 + x] = 40;
            }
            for x in 50..53 {
                data[y * 60 + x] = 250;
            }
        }
        let mask = adaptive_threshold_inv(&gray(w, h, data), 41, 8.0);
        assert!(mask[30 * 60 + 29], "the line is ink");
        assert!(!mask[30 * 60 + 10], "white paper is not");
        assert!(!mask[30 * 60 + 51], "5 levels darker is under the threshold");
        let black = adaptive_threshold_inv(&gray(20, 20, vec![0; 400]), 41, 8.0);
        assert!(black.iter().all(|v| !v), "a flat page has no ink, however dark");
    }

    #[test]
    fn close_bridges_a_diagonal_contact_only_along_the_diagonal() {
        // Two 2 × 2 blocks touching at one corner: (0..2, 0..2) and (2..4, 2..4).
        let mut mask = vec![false; 16];
        for (x, y) in [(0, 0), (1, 0), (0, 1), (1, 1), (2, 2), (3, 2), (2, 3), (3, 3)] {
            mask[y * 4 + x] = true;
        }
        // The square element dilates across the corner and erodes the neck away again.
        assert_eq!(close(&mask, 4, &SQUARE_3), mask);
        let diagonal = close(&mask, 4, &DIAGONAL_3);
        assert!(diagonal.iter().zip(&mask).all(|(d, m)| *d || !*m), "closing never removes ink");
        let anti = close(&mask, 4, &ANTI_DIAGONAL_3);
        assert_eq!(union(&diagonal, &anti).len(), 16);
    }

    #[test]
    fn area_resize_averages_blocks() {
        let plane = Plane { width: 4, height: 2, data: vec![0.0, 2.0, 4.0, 6.0, 2.0, 4.0, 6.0, 8.0] };
        let half = resize_area(&plane, 2, 1).unwrap();
        assert_eq!(half.data, vec![2.0, 6.0]);
        // A fractional cover: 3 → 2 pixels, the middle one split in half.
        let third = resize_area(&Plane { width: 3, height: 1, data: vec![0.0, 3.0, 6.0] }, 2, 1).unwrap();
        assert!((third.data[0] - 1.0).abs() < 1e-5 && (third.data[1] - 5.0).abs() < 1e-5, "{:?}", third.data);
        assert!(resize_area(&plane, 0, 1).is_none());
    }

    #[test]
    fn perspective_maps_the_corners_and_refuses_collinear_points() {
        let from = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let to = [[5.0, 3.0], [25.0, 4.0], [27.0, 26.0], [4.0, 23.0]];
        let h = perspective(from, to).unwrap();
        for ([x, y], [u, v]) in from.iter().zip(&to) {
            let w = h[6] * x + h[7] * y + h[8];
            assert!(((h[0] * x + h[1] * y + h[2]) / w - u).abs() < 1e-9);
            assert!(((h[3] * x + h[4] * y + h[5]) / w - v).abs() < 1e-9);
        }
        assert!(perspective(from, [[0.0, 0.0], [1.0, 1.0], [2.0, 2.0], [3.0, 3.0]]).is_none());
        assert!(perspective(from, [[f64::NAN, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]).is_none());
    }

    #[test]
    fn warping_the_whole_image_to_its_own_size_is_the_identity() {
        let data: Vec<u8> = (0..64).map(|i| (i * 3) as u8).collect();
        let plane = gray(8, 8, data.clone());
        let out = warp_plane(&plane, [[0.0, 0.0], [7.0, 0.0], [7.0, 7.0], [0.0, 7.0]], 8).unwrap();
        assert_eq!(to_gray(&out).unwrap().data(), &data[..]);
        assert!(warp_plane(&plane, [[0.0, 0.0]; 4], 8).is_none());
        assert!(warp_plane(&plane, [[0.0, 0.0], [7.0, 0.0], [7.0, 7.0], [0.0, 7.0]], 0).is_none());
    }

    #[test]
    fn bilinear_resize_keeps_flat_images_flat_and_refuses_empty_sizes() {
        let out = resize_bilinear(&gray(3, 5, vec![77; 15]), 11, 4).unwrap();
        assert!(out.data().iter().all(|&v| v == 77));
        assert!(resize_bilinear(&gray(1, 1, vec![9]), 0, 4).is_none());
        assert!(resize_bilinear(&gray(1, 1, vec![9]), u32::MAX, u32::MAX).is_none());
        // A region is resized as a copy of its pixels would be, and must lie inside the image.
        let image = gray(7, 6, (0..42).map(|v| (v * 37 % 256) as u8).collect());
        let region = PixelRect { x0: 2, y0: 1, x1: 6, y1: 5 };
        let copy = gray(4, 4, (1..5).flat_map(|y| (2..6).map(move |x| ((y * 7 + x) * 37 % 256) as u8)).collect());
        assert_eq!(resize_region_bilinear(&image, region, 9, 13), resize_bilinear(&copy, 9, 13));
        assert!(resize_region_bilinear(&image, PixelRect { x1: 8, ..region }, 9, 13).is_none());
        assert!(resize_region_bilinear(&image, PixelRect { x1: 2, ..region }, 9, 13).is_none());
    }
}
