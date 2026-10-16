//! Diagram candidates from the images a PDF embeds on a page (blueprint S7a).
//!
//! When a book stores each diagram as an image, the PDF already knows its box: looking for a
//! contour there only rebuilds what was available. This detector filters the page's images
//! down to the ones shaped like a diagram, joins images that are pieces of one picture, and,
//! when pixels are given, trims frame and caption off so the box is the board itself.
//!
//! The order of the guards is the design (prototype S-81):
//! 1. **Page coverage first.** A background scan touches every other image on the page, so it
//!    must be gone before tiles are joined, or the whole page collapses into one group.
//! 2. **Joining before size and shape.** A tile of a broken-up diagram fails both on its own
//!    (one measured tile was 240 × 96 px, aspect 2.5) and passes once joined.
//! 3. **Size in pixels, shape, size on the page**, then trimming.

use pdfcraft_geom::PageRect;

use super::{DiagramCandidate, Source};
use crate::coords::{PageGeometry, PixelRect, finite_page_rect, region_pixels_to_view};
use crate::raster::GrayImage;

/// The detector's thresholds.
///
/// The measured ones come from the Python prototype `ChessVisionOFF_Puro`
/// (`src/chess_diagram_ocr/detection/embedded.py`, relicensed MIT/Apache by its author), which
/// measured them on its corpus of chess books; each doc says what the number separates. The
/// rest are safety caps, and say so.
pub mod consts {
    /// Least pixels on the image's shorter side (`MIN_EMBEDDED_SIDE`). Refuses an image too
    /// coarse to read however large it is drawn. It measures resolution, not size on the page:
    /// that is [`MIN_EMBEDDED_SIDE_PT`].
    pub const MIN_EMBEDDED_SIDE: u32 = 120;

    /// Least size of the image's longer side on the page, in points (`MIN_EMBEDDED_SIDE_PT`):
    /// one inch, eight ranks of 9 pt. Measured by the prototype's census (S-82) over its whole
    /// corpus: header glyphs and ornaments drawn from embedded images span 15–50 pt, the
    /// smallest real diagram is 105.6 pt (`Euwe Band 1-2`). A knight ornament of 128 px in
    /// 15.4 pt (600 DPI) passed the pixel floor and was reported as a diagram on 71 of the
    /// 1181 pages of `Secrets of Chess Training`.
    pub const MIN_EMBEDDED_SIDE_PT: f32 = 72.0;

    /// Largest share of the page an image may cover and still be a diagram
    /// (`MAX_PAGE_COVERAGE`). Above it the image is the page's background scan: `1937 Kemeri`
    /// has a 1633 × 2468 px image under every page.
    pub const MAX_PAGE_COVERAGE: f32 = 0.70;

    /// How far the image's aspect (width / height in pixels) may stray from 1
    /// (`ASPECT_TOLERANCE`). Covers an image that carries its caption (620 × 704 = 1.14,
    /// measured in `Aagaard - Practical Chess Defence`).
    pub const ASPECT_TOLERANCE: f32 = 0.20;

    /// Gap, in points, within which two images count as tiles of one picture (`TILE_GAP_PT`).
    /// Small on purpose: the pieces to join touch (the `Gallagher` patches share an edge to
    /// 0.1 pt), while the gaps between separate diagrams on the corpus' three-diagram page are
    /// 30 and 100 pt.
    pub const TILE_GAP_PT: f32 = 2.0;

    /// Touching tiles from which a join applies (`MIN_TILES_TO_MERGE`): two already make a
    /// broken-up picture. `Gallagher` had 33 fragments against 7 real images in 192 pages.
    pub const MIN_TILES_TO_MERGE: usize = 2;

    /// Least share of the 7 inner grid lines `trim_to_grid` must find before it trusts a crop
    /// (its `min_periodicity`). A wrong crop shifts all 64 squares, so no crop is better.
    pub const MIN_GRID_PERIODICITY: f64 = 0.70;

    /// A gradient-profile entry at or above this share of the profile's peak lights a grid
    /// line (`_line_positions`, `relative_threshold`).
    pub const GRID_LINE_THRESHOLD: f64 = 0.25;

    /// Lit entries at most this many pixels apart belong to one line: a drawn line lights two
    /// or three neighbouring columns of the gradient (`_line_positions`).
    pub const GRID_LINE_GROUP_GAP: usize = 2;

    /// Fewest lines from which a grid step is measured (`_board_span`).
    pub const GRID_MIN_LINES: usize = 4;

    /// Smallest grid step, in pixels, worth trusting (`_board_span`).
    pub const GRID_MIN_STEP: f64 = 4.0;

    /// Smallest board side, in pixels, that either trim accepts (`trim_to_grid`,
    /// `_grid_periodicity`, `trim_to_frame`).
    pub const MIN_TRIM_SIDE: u32 = 32;

    /// A grid line counts as found where the profile rises this share of the way from its
    /// median to its peak (`_grid_periodicity`).
    pub const GRID_PEAK_SHARE: f64 = 0.30;

    /// Half-width of the window, as a share of one square, searched around each expected grid
    /// line (`_grid_periodicity`).
    pub const GRID_WINDOW_SHARE: f64 = 0.18;

    /// Below this gray level a pixel counts as frame ink (`FRAME_DARK_LEVEL`): book diagrams
    /// are black on white.
    pub const FRAME_DARK_LEVEL: u8 = 128;

    /// Share of a row or column that must be dark for it to be frame rather than pieces
    /// (`FRAME_MIN_FILL`). A rank full of black pieces stays under about 0.45, a frame line
    /// passes 0.95: the valley is wide enough not to need tuning per book.
    pub const FRAME_MIN_FILL: f64 = 0.80;

    /// The frame must span at least this share of the image each way, or it is a rule or an
    /// underline rather than a board frame (`FRAME_MIN_COVERAGE`).
    pub const FRAME_MIN_COVERAGE: f64 = 0.55;

    /// What is left inside the frame must be this close to square (`FRAME_ASPECT_TOLERANCE`).
    pub const FRAME_ASPECT_TOLERANCE: f64 = 0.10;

    /// Pixels dropped inside the frame: the frame stroke is not a square and touches rank 8
    /// (`FRAME_INSET`). The frame trim fixed all 14 off-grid crops among 173 diagrams of
    /// `Karpov 1` (pages 70–99), 8 of which had read a wrong position.
    pub const FRAME_INSET: u32 = 3;

    /// Range of the resolution hint, in DPI (`_pixels_for_bbox` clips its rendering DPI to
    /// 72..=600).
    pub const MIN_DPI_HINT: f32 = 72.0;
    pub const MAX_DPI_HINT: f32 = 600.0;

    /// Score of a crop confirmed by a trim: `base + base × shape`, where shape is 1 for a
    /// square image and 0 at the aspect tolerance (`candidates_from_embedded_images`).
    pub const TRIMMED_SCORE_BASE: f32 = 0.5;

    /// Score base of an untrimmed box, kept below every trimmed one.
    pub const UNTRIMMED_SCORE_BASE: f32 = 0.35;

    /// Most images of one page considered. Joining compares every pair, so this bounds the
    /// work on a hostile page. A safety cap, not a measurement: a page with more images than
    /// this has its later ones ignored.
    pub const MAX_IMAGES: usize = 4096;
}

use consts::*;

/// One image drawn on the page, as the engine adapts it from `edit::PageImage`.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageBox {
    /// Where the image is drawn, in user space.
    pub bbox_user: PageRect,
    /// The image's own size in pixels (its resolution, not its size on the page).
    pub pixel_width: u32,
    pub pixel_height: u32,
    /// The region `bbox_user` as it is displayed, rendered to gray: top row at the top of the
    /// displayed page (after `/Rotate`), at any resolution. Optional: without it the box is not
    /// trimmed.
    pub gray: Option<GrayImage>,
}

/// A candidate before the shape filters: one image, or the union of touching tiles.
struct Unit {
    bbox: PageRect,
    /// Pixel size, fractional for a union (its density times its size).
    native: (f64, f64),
    /// The image's index when the unit is a single image (only those carry pixels to trim).
    single: Option<usize>,
}

/// The diagram candidates among a page's embedded images, in the order of the images (a
/// group of joined tiles takes the place of its first tile). Reading order is `order`'s job.
pub fn candidates_from_images(images: &[ImageBox], page: &PageGeometry) -> Vec<DiagramCandidate> {
    let crop = page.crop_box();
    let page_area = f64::from(crop.width()) * f64::from(crop.height());
    let max_area = page_area * f64::from(MAX_PAGE_COVERAGE);
    let tiles: Vec<(usize, PageRect)> = images
        .iter()
        .take(MAX_IMAGES)
        .enumerate()
        .filter_map(|(index, image)| {
            let bbox = finite_page_rect(image.bbox_user)?;
            let area = f64::from(bbox.width()) * f64::from(bbox.height());
            // An empty box is nothing; a box over most of the page is its background scan.
            (area > 0.0 && area <= max_area).then_some((index, bbox))
        })
        .collect();
    join_tiles(&tiles, images).into_iter().filter_map(|unit| candidate(&unit, images, page)).collect()
}

/// Groups tiles whose boxes come within [`TILE_GAP_PT`] of each other and returns each group's
/// union, with a resolution taken from the pixels the group carries per unit of area.
fn join_tiles(tiles: &[(usize, PageRect)], images: &[ImageBox]) -> Vec<Unit> {
    let mut parent: Vec<usize> = (0..tiles.len()).collect();
    for (i, (_, a)) in tiles.iter().enumerate() {
        for (j, (_, b)) in tiles.iter().enumerate().skip(i + 1) {
            if touch(a, b) {
                let (ri, rj) = (root(&mut parent, i), root(&mut parent, j));
                if let Some(slot) = parent.get_mut(ri) {
                    *slot = rj;
                }
            }
        }
    }
    // Groups in the order of their first member.
    let mut slot_of_root: Vec<Option<usize>> = vec![None; tiles.len()];
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for i in 0..tiles.len() {
        let r = root(&mut parent, i);
        match slot_of_root.get(r).copied().flatten() {
            Some(g) => {
                if let Some(group) = groups.get_mut(g) {
                    group.push(i);
                }
            }
            None => {
                if let Some(slot) = slot_of_root.get_mut(r) {
                    *slot = Some(groups.len());
                }
                groups.push(vec![i]);
            }
        }
    }

    let native_of = |index: usize| images.get(index).map_or((0.0, 0.0), |m| (f64::from(m.pixel_width), f64::from(m.pixel_height)));
    let mut units = Vec::with_capacity(groups.len());
    for group in groups {
        let members: Vec<(usize, PageRect)> = group.iter().filter_map(|&i| tiles.get(i).copied()).collect();
        if members.len() < MIN_TILES_TO_MERGE {
            units.extend(members.into_iter().map(|(index, bbox)| Unit { bbox, native: native_of(index), single: Some(index) }));
            continue;
        }
        let Some(bbox) = members.iter().map(|&(_, b)| b).reduce(|u, b| PageRect::new(u.x0.min(b.x0), u.y0.min(b.y0), u.x1.max(b.x1), u.y1.max(b.y1)))
        else {
            continue;
        };
        let (w, h) = (f64::from(bbox.width()), f64::from(bbox.height()));
        let area = w * h;
        let pixels: f64 = members.iter().map(|&(index, _)| native_of(index).0 * native_of(index).1).sum();
        // Pixels per point from the content the tiles carry, not the finest tile: in `Polgar`
        // 19 stretched 1 × 1 px images frame each diagram, and the finest-tile density gave a
        // "217 px" union that carried 19 pixels. Measured honestly it is 4 px and fails the floor.
        let per_point = if area > 0.0 && area.is_finite() { (pixels / area).sqrt() } else { 0.0 };
        let native = ((per_point * w).round_ties_even().max(1.0), (per_point * h).round_ties_even().max(1.0));
        units.push(Unit { bbox, native, single: None });
    }
    units
}

/// Whether `a` grown by [`TILE_GAP_PT`] on every side overlaps `b` with some area.
fn touch(a: &PageRect, b: &PageRect) -> bool {
    let gap = TILE_GAP_PT;
    a.x0 - gap < b.x1 && b.x0 < a.x1 + gap && a.y0 - gap < b.y1 && b.y0 < a.y1 + gap
}

/// The root of `i` in a union-find forest, halving the path on the way.
fn root(parent: &mut [usize], mut i: usize) -> usize {
    // Every step moves to a parent, and the forest has no cycles (a union links a root to a
    // root), so this ends within `parent.len()` steps; the bound is a guard all the same.
    for _ in 0..parent.len() {
        let Some(&p) = parent.get(i) else { return i };
        if p == i {
            return i;
        }
        let grand = parent.get(p).copied().unwrap_or(p);
        if let Some(slot) = parent.get_mut(i) {
            *slot = grand;
        }
        i = grand;
    }
    i
}

/// A unit that passes the size and shape filters, as a candidate, trimmed when it can be.
fn candidate(unit: &Unit, images: &[ImageBox], page: &PageGeometry) -> Option<DiagramCandidate> {
    let (nw, nh) = unit.native;
    if nw.min(nh) < f64::from(MIN_EMBEDDED_SIDE) {
        return None;
    }
    let aspect = nw / nh;
    let tolerance = f64::from(ASPECT_TOLERANCE);
    if !((1.0 - tolerance)..=(1.0 + tolerance)).contains(&aspect) {
        return None;
    }
    let side_pt = f64::from(unit.bbox.width().max(unit.bbox.height())) * f64::from(page.user_unit());
    if side_pt < f64::from(MIN_EMBEDDED_SIDE_PT) {
        return None;
    }
    let dpi = (f64::from(crate::coords::POINTS_PER_INCH) * nw.max(nh) / side_pt) as f32;
    let dpi_hint = if dpi.is_finite() { dpi.clamp(MIN_DPI_HINT, MAX_DPI_HINT) } else { MAX_DPI_HINT };

    let trimmed =
        unit.single.and_then(|index| images.get(index)).and_then(|image| image.gray.as_ref()).and_then(|gray| trimmed_box(unit.bbox, gray, page));
    // A perfect aspect and a confirmed crop are worth more than a stretched box accepted at the
    // edge of the tolerance.
    let shape = (1.0 - (aspect - 1.0).abs() / tolerance.max(1e-6)).clamp(0.0, 1.0) as f32;
    let base = if trimmed.is_some() { TRIMMED_SCORE_BASE } else { UNTRIMMED_SCORE_BASE };
    Some(DiagramCandidate {
        bbox_user: trimmed.unwrap_or(unit.bbox),
        source: Source::Embedded,
        score: base + base * shape,
        dpi_hint,
        placement: None,
    })
}

/// The board's box in user space after trimming `gray` (a raster of `bbox` as displayed), or
/// `None` when neither trim trusts a crop.
fn trimmed_box(bbox: PageRect, gray: &GrayImage, page: &PageGeometry) -> Option<PageRect> {
    // The grid first; the frame when the grid is silent (hatched squares draw no grid line).
    let pixels = trim_to_grid(gray).or_else(|| trim_to_frame(gray))?;
    let region = page.user_to_view(bbox)?;
    let view = region_pixels_to_view(region, gray.width(), gray.height(), pixels)?;
    let user = page.view_to_user(view)?;
    (user.width() > 0.0 && user.height() > 0.0).then_some(user)
}

/// The board inside an image, found from its drawn 8 × 8 grid: a square pixel box, or `None`
/// when the grid is not clear enough to trust a crop.
///
/// The grid lines are the long straight edges of the image, so the mean gradient along each
/// axis shows them as peaks. Their median spacing is the square size; the span is extended
/// outward to 8 squares (a frameless board shows only its 7 inner lines), forced square
/// around its centre (a caption stretches one axis), and accepted only when at least
/// [`MIN_GRID_PERIODICITY`] of the 7 inner lines are where the step puts them.
pub fn trim_to_grid(image: &GrayImage) -> Option<PixelRect> {
    let (width, height) = (image.width(), image.height());
    if width < MIN_TRIM_SIDE || height < MIN_TRIM_SIDE {
        return None;
    }
    let (profile_x, profile_y) = gradient_profiles(image)?;
    let (left, right) = board_span(&profile_x, i64::from(width))?;
    let (top, bottom) = board_span(&profile_y, i64::from(height))?;
    let min_side = i64::from(MIN_TRIM_SIDE);
    if right - left < min_side || bottom - top < min_side {
        return None;
    }
    let side = (right - left).min(bottom - top);
    let x0 = ((left + right).div_euclid(2) - side.div_euclid(2)).min(i64::from(width) - side).max(0);
    let y0 = ((top + bottom).div_euclid(2) - side.div_euclid(2)).min(i64::from(height) - side).max(0);
    let periodicity = grid_periodicity(&profile_x, x0, x0 + side).min(grid_periodicity(&profile_y, y0, y0 + side));
    if periodicity < MIN_GRID_PERIODICITY {
        return None;
    }
    Some(PixelRect {
        x0: u32::try_from(x0).ok()?,
        y0: u32::try_from(y0).ok()?,
        x1: u32::try_from(x0 + side).ok()?.min(width),
        y1: u32::try_from(y0 + side).ok()?.min(height),
    })
}

/// Mean absolute 3 × 3 Sobel gradient per column (across) and per row (down), with the border
/// mirrored without repeating the edge pixel (OpenCV's default, which the prototype used).
fn gradient_profiles(image: &GrayImage) -> Option<(Vec<f64>, Vec<f64>)> {
    let width = usize::try_from(image.width()).ok()?;
    let height = usize::try_from(image.height()).ok()?;
    if width < 2 || height < 2 {
        return None;
    }
    let mirror = |i: usize, delta: isize, n: usize| -> usize {
        match i.checked_add_signed(delta) {
            None => 1,
            Some(j) if j >= n => n.saturating_sub(2),
            Some(j) => j,
        }
    };
    let at = |row: &[u8], x: usize| row.get(x).map_or(0, |&v| i32::from(v));
    let mut sum_x = vec![0.0_f64; width];
    let mut sum_y = vec![0.0_f64; height];
    for y in 0..height {
        let row_of = |yy: usize| u32::try_from(yy).ok().and_then(|yy| image.row(yy)).unwrap_or(&[]);
        let (up, here, down) = (row_of(mirror(y, -1, height)), row_of(y), row_of(mirror(y, 1, height)));
        let mut row_sum = 0.0;
        for (x, column) in sum_x.iter_mut().enumerate() {
            let (l, r) = (mirror(x, -1, width), mirror(x, 1, width));
            let gx = (at(up, r) - at(up, l)) + 2 * (at(here, r) - at(here, l)) + (at(down, r) - at(down, l));
            let gy = (at(down, l) - at(up, l)) + 2 * (at(down, x) - at(up, x)) + (at(down, r) - at(up, r));
            *column += f64::from(gx.abs());
            row_sum += f64::from(gy.abs());
        }
        if let Some(slot) = sum_y.get_mut(y) {
            *slot = row_sum / width as f64;
        }
    }
    let rows = height as f64;
    sum_x.iter_mut().for_each(|v| *v /= rows);
    Some((sum_x, sum_y))
}

/// Positions of grid lines in a gradient profile: one index per peak, not one per lit pixel.
fn line_positions(profile: &[f64]) -> Vec<usize> {
    let peak = profile.iter().copied().fold(0.0_f64, f64::max);
    if peak <= 1e-6 {
        return Vec::new();
    }
    let threshold = peak * GRID_LINE_THRESHOLD;
    let mut lines = Vec::new();
    let mut group: Option<(usize, usize)> = None;
    for (index, _) in profile.iter().enumerate().filter(|(_, v)| **v >= threshold) {
        group = match group {
            Some((start, previous)) if index.saturating_sub(previous) <= GRID_LINE_GROUP_GAP => Some((start, index)),
            Some((start, previous)) => {
                lines.push((start + previous) / 2);
                Some((index, index))
            }
            None => Some((index, index)),
        };
    }
    if let Some((start, previous)) = group {
        lines.push((start + previous) / 2);
    }
    lines
}

/// Where the board starts and ends along one axis, in pixels: the line step extended outward
/// until it closes 8 squares, clamped to `0..limit`.
fn board_span(profile: &[f64], limit: i64) -> Option<(i64, i64)> {
    let lines = line_positions(profile);
    if lines.len() < GRID_MIN_LINES {
        return None;
    }
    let mut steps: Vec<f64> = lines.windows(2).filter_map(|w| Some(w.get(1)?.checked_sub(*w.first()?)? as f64)).filter(|d| *d > 1.0).collect();
    let step = median(&mut steps)?;
    if step < GRID_MIN_STEP {
        return None;
    }
    let (first, last) = (*lines.first()? as f64, *lines.last()? as f64);
    // Python's `round` rounds halves to even; the prototype's spans depend on it.
    let squares_between = ((last - first) / step).round_ties_even();
    let missing = 8.0 - squares_between;
    if missing < 0.0 {
        return None;
    }
    let before = (missing / 2.0).ceil();
    let start = (first - before * step).round_ties_even();
    let end = (start + 8.0 * step).round_ties_even();
    // Both are bounded by the profile length and 8 steps of at most that length, so they fit.
    Some(((start as i64).max(0), (end as i64).min(limit)))
}

/// The share (0..=1) of the 7 inner grid lines found where a board spanning `start..end` puts
/// them. Counting lines, rather than comparing peak and percentile levels, does not collapse
/// on a clean board whose gradient is zero almost everywhere.
fn grid_periodicity(profile: &[f64], start: i64, end: i64) -> f64 {
    let span = end - start;
    if span < i64::from(MIN_TRIM_SIDE) {
        return 0.0;
    }
    let slice = |a: i64, b: i64| profile.get(usize::try_from(a).ok()?..usize::try_from(b).ok()?);
    let Some(segment) = slice(start, end) else { return 0.0 };
    let top = segment.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let Some(baseline) = median(&mut segment.to_vec()) else { return 0.0 };
    if top - baseline <= 1e-6 {
        return 0.0;
    }
    let threshold = baseline + GRID_PEAK_SHARE * (top - baseline);
    let square = span as f64 / 8.0;
    let window = ((square * GRID_WINDOW_SHARE) as i64).max(1);
    let found = (1..8)
        .filter(|&k| {
            let centre = (start as f64 + square * f64::from(k)) as i64;
            let (low, high) = ((centre - window).max(start), (centre + window + 1).min(end));
            high > low && slice(low, high).is_some_and(|w| w.iter().any(|v| *v >= threshold))
        })
        .count();
    found as f64 / 7.0
}

/// The median (mean of the middle two for an even count), or `None` for no values.
fn median(values: &mut [f64]) -> Option<f64> {
    values.sort_by(f64::total_cmp);
    let mid = values.len() / 2;
    let upper = *values.get(mid)?;
    if values.len() % 2 == 1 { Some(upper) } else { Some((values.get(mid.checked_sub(1)?)? + upper) / 2.0) }
}

/// The board inside an image, found from its printed frame: the pixel box inside the frame, or
/// `None` when there is no frame to trust.
///
/// The second try, for when [`trim_to_grid`] is silent: on hatched squares the boundary
/// between light and dark is a change of texture, not a drawn line. A frame is a straight dark
/// line across the whole image, and a rank full of black pieces comes nowhere near
/// [`FRAME_MIN_FILL`].
pub fn trim_to_frame(image: &GrayImage) -> Option<PixelRect> {
    let (width, height) = (image.width(), image.height());
    let mut dark_in_column = vec![0_u32; usize::try_from(width).ok()?];
    let mut frame_rows = Vec::new();
    for y in 0..height {
        let row = image.row(y)?;
        let mut dark_in_row = 0_u32;
        for (count, &v) in dark_in_column.iter_mut().zip(row) {
            if v < FRAME_DARK_LEVEL {
                *count = count.saturating_add(1);
                dark_in_row = dark_in_row.saturating_add(1);
            }
        }
        if f64::from(dark_in_row) / f64::from(width) >= FRAME_MIN_FILL {
            frame_rows.push(y);
        }
    }
    let frame_columns: Vec<u32> =
        (0..width).zip(&dark_in_column).filter(|&(_, &n)| f64::from(n) / f64::from(height) >= FRAME_MIN_FILL).map(|(x, _)| x).collect();
    if frame_rows.len() < 2 || frame_columns.len() < 2 {
        return None;
    }
    let (top, bottom) = (i64::from(*frame_rows.first()?), i64::from(*frame_rows.last()?) + 1);
    let (left, right) = (i64::from(*frame_columns.first()?), i64::from(*frame_columns.last()?) + 1);
    // A frame that spans little of the image is something else: a rule, a band, an underline.
    if ((bottom - top) as f64) < f64::from(height) * FRAME_MIN_COVERAGE || ((right - left) as f64) < f64::from(width) * FRAME_MIN_COVERAGE {
        return None;
    }
    let inset = i64::from(FRAME_INSET);
    let (x0, y0, x1, y1) = (left + inset, top + inset, right - inset, bottom - inset);
    let min_side = i64::from(MIN_TRIM_SIDE);
    if x1 - x0 < min_side || y1 - y0 < min_side {
        return None;
    }
    let aspect = (x1 - x0) as f64 / (y1 - y0) as f64;
    if (aspect - 1.0).abs() > FRAME_ASPECT_TOLERANCE {
        return None;
    }
    // A "frame" that is the image's own edge leaves nothing to trim, and calling it a trim would
    // raise the score of a crop nobody checked.
    if bottom - top == i64::from(height) && right - left == i64::from(width) {
        return None;
    }
    Some(PixelRect { x0: u32::try_from(x0).ok()?, y0: u32::try_from(y0).ok()?, x1: u32::try_from(x1).ok()?, y1: u32::try_from(y1).ok()? })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coords::{PageGeometry, Rotation};

    const LETTER: PageRect = PageRect { x0: 0.0, y0: 0.0, x1: 612.0, y1: 792.0 };
    const LIGHT: u8 = 245;
    const DARK: u8 = 45;

    fn letter(rotation: Rotation) -> PageGeometry {
        PageGeometry::new(LETTER, rotation).unwrap()
    }

    /// A white canvas to draw synthetic fixtures on.
    struct Canvas {
        width: u32,
        height: u32,
        data: Vec<u8>,
    }

    impl Canvas {
        fn new(width: u32, height: u32) -> Self {
            Self { width, height, data: vec![255; (width * height) as usize] }
        }

        fn fill(&mut self, x0: u32, y0: u32, x1: u32, y1: u32, value: u8) {
            for y in y0..y1.min(self.height) {
                for x in x0..x1.min(self.width) {
                    self.data[(y * self.width + x) as usize] = value;
                }
            }
        }

        /// An 8 × 8 checkerboard of `cell`-pixel squares with its top-left corner at (x, y).
        fn board(&mut self, x: u32, y: u32, cell: u32) {
            for row in 0..8 {
                for column in 0..8 {
                    let value = if (row + column) % 2 == 1 { DARK } else { LIGHT };
                    self.fill(x + column * cell, y + row * cell, x + (column + 1) * cell, y + (row + 1) * cell, value);
                }
            }
        }

        /// A caption line: short dark glyph-sized strokes, nowhere near a grid line.
        fn caption(&mut self, x: u32, y: u32) {
            for i in 0..5 {
                let left = x + i * 16;
                self.fill(left, y, left + 7, y + 2, 0);
                self.fill(left, y, left + 2, y + 14, 0);
                self.fill(left + 5, y + 6, left + 7, y + 14, 0);
            }
        }

        fn image(self) -> GrayImage {
            GrayImage::new(self.width, self.height, self.data).unwrap()
        }
    }

    fn image(bbox: PageRect, pixel_width: u32, pixel_height: u32) -> ImageBox {
        ImageBox { bbox_user: bbox, pixel_width, pixel_height, gray: None }
    }

    fn close(a: PageRect, b: PageRect, tolerance: f32) -> bool {
        [(a.x0, b.x0), (a.y0, b.y0), (a.x1, b.x1), (a.y1, b.y1)].iter().all(|(p, q)| (p - q).abs() <= tolerance)
    }

    /// A 320 px board in white margins with a caption under it: 400 × 440 px.
    fn board_with_margins_and_caption() -> GrayImage {
        let mut canvas = Canvas::new(400, 440);
        canvas.board(40, 40, 40);
        canvas.caption(60, 385);
        canvas.image()
    }

    #[test]
    fn trim_to_grid_finds_a_board_inside_margins_and_a_caption() {
        let board = trim_to_grid(&board_with_margins_and_caption()).expect("the grid is drawn plainly");
        assert_eq!(board.width(), board.height(), "a board is square: {board:?}");
        // The grid's edges sit at 40 and 360 in both directions; a line is located at the first
        // of the two gradient columns it lights, so 1 px off is the expected rounding.
        for (got, want) in [(board.x0, 40), (board.y0, 40), (board.x1, 360), (board.y1, 360)] {
            assert!(got.abs_diff(want) <= 1, "{board:?}");
        }
    }

    #[test]
    fn trim_to_grid_refuses_what_is_not_a_grid() {
        let flat = Canvas::new(300, 300);
        assert_eq!(trim_to_grid(&flat.image()), None);
        let mut stripes = Canvas::new(300, 300);
        for i in 0..30 {
            stripes.fill(i * 10, 0, i * 10 + 3, 300, 0);
        }
        assert_eq!(trim_to_grid(&stripes.image()), None);
    }

    /// Lines at the board's step, but only 2 of the 7 inner ones: the span closes 8 squares,
    /// and only the periodicity check stops the crop.
    #[test]
    fn trim_to_grid_refuses_a_grid_missing_its_inner_lines() {
        let mut outline = Canvas::new(400, 400);
        for at in [40, 80, 320, 360] {
            outline.fill(at, 40, at + 2, 362, 0);
            outline.fill(40, at, 362, at + 2, 0);
        }
        assert_eq!(trim_to_grid(&outline.image()), None);
    }

    /// A 10 × 10 draughts board is a clean grid, but not 8 squares: the middle 8 would pass
    /// every other check.
    #[test]
    fn trim_to_grid_refuses_a_draughts_board() {
        let mut draughts = Canvas::new(400, 400);
        for row in 0..10 {
            for column in 0..10 {
                let value = if (row + column) % 2 == 1 { DARK } else { LIGHT };
                draughts.fill(40 + column * 32, 40 + row * 32, 40 + (column + 1) * 32, 40 + (row + 1) * 32, value);
            }
        }
        assert_eq!(trim_to_grid(&draughts.image()), None);
    }

    /// A printed frame around a board whose squares have no drawn edge (the grid finder has
    /// nothing to find), with an evaluation footer under it: the `Karpov 1` shape.
    fn framed_board_with_footer() -> GrayImage {
        let (side, frame, margin, footer) = (320, 4, 6, 40);
        let mut canvas = Canvas::new(side + 2 * (frame + margin), side + 2 * (frame + margin) + footer);
        let outer = side + 2 * frame;
        canvas.fill(margin, margin, margin + outer, margin + outer, 20);
        canvas.fill(margin + frame, margin + frame, margin + frame + side, margin + frame + side, LIGHT);
        canvas.fill(margin + side - 40, margin + outer + margin + 10, margin + side - 20, margin + outer + margin + 14, 0);
        canvas.image()
    }

    #[test]
    fn trim_to_frame_keeps_what_is_inside_the_frame() {
        let image = framed_board_with_footer();
        assert_eq!(trim_to_grid(&image), None, "premise: the grid finder has nothing to find here");
        // Frame rows/columns 6..334; inside it, 3 px further in.
        assert_eq!(trim_to_frame(&image), Some(PixelRect { x0: 9, y0: 9, x1: 331, y1: 331 }));
    }

    #[test]
    fn trim_to_frame_refuses_a_frameless_or_full_bleed_image() {
        let mut board = Canvas::new(340, 340);
        board.board(10, 10, 40);
        assert_eq!(trim_to_frame(&board.image()), None, "a row of dark squares is half dark, not a frame");
        let mut bleed = Canvas::new(200, 200);
        bleed.fill(0, 0, 200, 200, 0);
        bleed.fill(4, 4, 196, 196, 255);
        assert_eq!(trim_to_frame(&bleed.image()), None, "the image's own edge is not a frame to trim");
    }

    /// The whole pipeline on an upright page: the trimmed pixel box goes back to user space.
    #[test]
    fn a_board_with_margins_is_trimmed_to_the_board_in_user_space() {
        // 400 × 440 px drawn in 100 × 110 pt: 4 px per point, 288 DPI.
        let bbox = PageRect::new(100.0, 300.0, 200.0, 410.0);
        let images = [ImageBox { gray: Some(board_with_margins_and_caption()), ..image(bbox, 400, 440) }];
        let found = candidates_from_images(&images, &letter(Rotation::R0));
        assert_eq!(found.len(), 1, "{found:?}");
        let candidate = &found[0];
        assert_eq!(candidate.source, Source::Embedded);
        // Board pixels 40..360 across and down → x 110..190, y (from the top, 410) 400..320.
        assert!(close(candidate.bbox_user, PageRect::new(110.0, 320.0, 190.0, 400.0), 0.5), "{:?}", candidate.bbox_user);
        assert!((candidate.dpi_hint - 288.0).abs() < 1.0, "{}", candidate.dpi_hint);

        // The same image without pixels keeps its raw box, and scores lower than a confirmed crop.
        let untrimmed = candidates_from_images(&[image(bbox, 400, 440)], &letter(Rotation::R0));
        assert_eq!(untrimmed.len(), 1);
        assert_eq!(untrimmed[0].bbox_user, bbox);
        assert!(untrimmed[0].score < candidate.score, "{} vs {}", untrimmed[0].score, candidate.score);
    }

    /// When the grid is silent the frame trims: the board without its frame and footer.
    #[test]
    fn a_framed_board_is_trimmed_inside_its_frame() {
        // 340 × 380 px drawn in 85 × 95 pt; inside the frame are pixels 9..331 both ways.
        let bbox = PageRect::new(100.0, 300.0, 185.0, 395.0);
        let images = [ImageBox { gray: Some(framed_board_with_footer()), ..image(bbox, 340, 380) }];
        let found = candidates_from_images(&images, &letter(Rotation::R0));
        assert_eq!(found.len(), 1, "{found:?}");
        // x 100 + 9/4 .. 100 + 331/4; y from the top (395) down 9/4 .. 331/4.
        assert!(close(found[0].bbox_user, PageRect::new(102.25, 312.25, 182.75, 392.75), 0.01), "{:?}", found[0].bbox_user);
    }

    /// On a turned page the pixels are in display orientation, and the trimmed box must come
    /// back through the turn: off-centre margins catch a mapping that ignores it.
    #[test]
    fn a_trimmed_box_comes_back_through_the_page_turn() {
        // 400 × 380 px; board at pixels x 30..350, y 20..340 (margins 30/50 across, 20/40 down).
        let mut canvas = Canvas::new(400, 380);
        canvas.board(30, 20, 40);
        // At 180° the region is displayed at view x 412..512, y 300..395 (user 100 × 95 pt).
        let bbox = PageRect::new(100.0, 300.0, 200.0, 395.0);
        let images = [ImageBox { gray: Some(canvas.image()), ..image(bbox, 400, 380) }];
        let found = candidates_from_images(&images, &letter(Rotation::R180));
        assert_eq!(found.len(), 1, "{found:?}");
        // View x 419.5..499.5 → user x 112.5..192.5; view y 305..385 → user y 305..385.
        assert!(close(found[0].bbox_user, PageRect::new(112.5, 305.0, 192.5, 385.0), 0.5), "{:?}", found[0].bbox_user);
    }

    #[test]
    fn a_full_page_scan_is_rejected_but_a_diagram_on_it_is_kept() {
        let scan = image(LETTER, 2550, 3300);
        assert!(candidates_from_images(std::slice::from_ref(&scan), &letter(Rotation::R0)).is_empty());
        // The `1937 Kemeri` case: a background scan under a real diagram must not swallow it
        // into one group of tiles.
        let diagram = image(PageRect::new(150.0, 300.0, 300.0, 450.0), 600, 600);
        let found = candidates_from_images(&[scan, diagram.clone()], &letter(Rotation::R0));
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].bbox_user, diagram.bbox_user);
    }

    #[test]
    fn tiles_side_by_side_merge_into_one_candidate() {
        // Each tile alone is 55 × 100 pt at 4 px/pt: aspect 0.55, rejected on its own.
        let left = image(PageRect::new(100.0, 300.0, 155.0, 400.0), 220, 400);
        let right = image(PageRect::new(155.0, 300.0, 210.0, 400.0), 220, 400);
        assert!(candidates_from_images(std::slice::from_ref(&left), &letter(Rotation::R0)).is_empty());
        // A third tile 30 pt away is a neighbour, not a piece: it stays alone and is rejected.
        let far = image(PageRect::new(240.0, 300.0, 295.0, 400.0), 220, 400);
        let found = candidates_from_images(&[left, far, right], &letter(Rotation::R0));
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].bbox_user, PageRect::new(100.0, 300.0, 210.0, 400.0));
        // The union's resolution comes from the pixels the tiles carry: still 4 px/pt.
        assert!((found[0].dpi_hint - 288.0).abs() < 1.0, "{}", found[0].dpi_hint);
    }

    #[test]
    fn tiles_join_across_a_hairline_gap_but_not_a_wider_one() {
        let pair = |gap: f32| {
            [image(PageRect::new(100.0, 300.0, 155.0, 400.0), 220, 400), image(PageRect::new(155.0 + gap, 300.0, 210.0 + gap, 400.0), 220, 400)]
        };
        assert_eq!(candidates_from_images(&pair(1.5), &letter(Rotation::R0)).len(), 1);
        assert!(candidates_from_images(&pair(3.0), &letter(Rotation::R0)).is_empty());
    }

    #[test]
    fn a_photo_and_an_ornament_are_rejected() {
        // A 3:2 photo: aspect 1.5.
        let photo = image(PageRect::new(100.0, 300.0, 250.0, 400.0), 600, 400);
        // A header glyph: plenty of pixels (600 DPI) but 15.4 pt on the page.
        let ornament = image(PageRect::new(100.0, 700.0, 115.4, 715.4), 128, 128);
        // A square image with too few pixels to read, however large it is drawn.
        let coarse = image(PageRect::new(100.0, 100.0, 250.0, 250.0), 100, 100);
        assert!(candidates_from_images(&[photo, ornament, coarse], &letter(Rotation::R0)).is_empty());
    }

    #[test]
    fn degenerate_inputs_give_no_candidate_and_no_panic() {
        let nan = PageRect { x0: f32::NAN, y0: 300.0, x1: 250.0, y1: 450.0 };
        let infinite = PageRect { x0: 100.0, y0: 300.0, x1: f32::INFINITY, y1: 450.0 };
        let flat = PageRect::new(100.0, 300.0, 100.0, 450.0);
        let huge = PageRect::new(-f32::MAX, -f32::MAX, f32::MAX, f32::MAX);
        let square = PageRect::new(100.0, 300.0, 250.0, 450.0);
        let images = [
            image(nan, 600, 600),
            image(infinite, 600, 600),
            image(flat, 600, 600),
            image(huge, 600, 600),
            image(square, 0, 0),
            image(square, u32::MAX, 0),
            // Pixels that do not fit the box are tolerated: a 1 × 1 raster is simply not trimmed.
            ImageBox { gray: Some(GrayImage::new(1, 1, vec![0]).unwrap()), ..image(nan, 600, 600) },
        ];
        assert!(candidates_from_images(&images, &letter(Rotation::R90)).is_empty());
        // Data that does not fill the image never becomes a GrayImage at all.
        assert!(GrayImage::new(400, 440, vec![255; 400 * 440 - 1]).is_err());
        assert!(GrayImage::new(0, 0, Vec::new()).is_err());
        assert!(candidates_from_images(&[], &letter(Rotation::R0)).is_empty());
    }

    /// Hostile pixels: whatever the raster holds, trimming returns a box inside it or nothing,
    /// and a tiny raster on a valid image still yields the untrimmed candidate.
    #[test]
    fn trimming_any_raster_stays_inside_it() {
        let mut seed: u32 = 0x2545_f491;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        for _ in 0..200 {
            let (w, h) = (1 + next() % 90, 1 + next() % 90);
            let sparse = next() % 4;
            let data = (0..w * h).map(|_| if next() % 4 <= sparse { 0 } else { 255 }).collect();
            let gray = GrayImage::new(w, h, data).unwrap();
            for trimmed in [trim_to_grid(&gray), trim_to_frame(&gray)].into_iter().flatten() {
                assert!(trimmed.x0 < trimmed.x1 && trimmed.x1 <= w && trimmed.y0 < trimmed.y1 && trimmed.y1 <= h, "{trimmed:?} in {w}x{h}");
            }
        }
        let square = PageRect::new(100.0, 300.0, 250.0, 450.0);
        let tiny = ImageBox { gray: Some(GrayImage::new(1, 1, vec![0]).unwrap()), ..image(square, 600, 600) };
        let found = candidates_from_images(&[tiny], &letter(Rotation::R270));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].bbox_user, square);
    }
}
