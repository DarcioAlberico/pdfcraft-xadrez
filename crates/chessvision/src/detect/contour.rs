//! Diagram candidates from contours on the rendered page (blueprint S7b).
//!
//! This is the detector for diagrams that are neither an embedded image nor text: a scanned
//! page, or a board drawn in vectors or in a Form as one picture. It sees only pixels: the page
//! rendered to gray at a known DPI.
//!
//! The pipeline is the prototype's (`board_detection.py`, `_extract_candidate_quads` and
//! `detect_boards`):
//! 1. 5 × 5 Gaussian blur, then an inverted Gaussian adaptive threshold (block 41, C 8).
//! 2. Three binarisations, each giving its own contours: the raw one, a 3 × 3 close, and the
//!    union of two diagonal closes (squares of one colour touch only at their corners, and a
//!    lost corner contact splits the board).
//! 3. Each contour is simplified to 4 corners (Douglas-Peucker at 2 % of its perimeter) or,
//!    failing that, boxed by its minimum-area rectangle.
//! 4. Geometry (size, squareness) times `0.55 + 0.45 × texture`, where texture combines checker
//!    contrast and grid regularity on a 320 px warp. A quad with no checker contrast at all is
//!    a photo, a frame or a table, and is dropped before it can compete.
//! 5. The same search on the page at half size (hatched squares average to solid there), and
//!    the largest square inside each elongated quad that failed the checker test (a board
//!    joined to its caption), both scored on the full page.
//! 6. Duplicates and specks dropped, then a score floor, overlap suppression and a count cap.

use rten_imageproc::{PointF, RetrievalMode, find_contours, min_area_rect};
use rten_tensor::NdTensorView;

use super::{DiagramCandidate, Source};
use crate::board::{Quad, order_quad, quad_texture_parts, texture_from_parts};
use crate::coords::{PageGeometry, ViewRect, px_to_pt};
use crate::imageops::{ANTI_DIAGONAL_3, DIAGONAL_3, SQUARE_3, adaptive_threshold_inv, close, dims, gaussian_blur_5, union};
use crate::raster::GrayImage;

/// The detector's thresholds.
///
/// The measured ones come from the Python prototype `ChessVisionOFF_Puro`
/// (`src/chess_diagram_ocr/board_detection.py` and `config.py`, relicensed MIT/Apache by its
/// author). The prototype itself records that most of the geometry numbers date from its first
/// phase and were never re-measured; the recall options were measured on its field set (68
/// pages, 115 diagrams). The rest are safety caps, and say so.
pub mod consts {
    /// Block size of the adaptive threshold, in pixels, and how many levels darker than the
    /// local Gaussian mean a pixel must be to count as ink.
    pub const THRESHOLD_BLOCK: usize = 41;
    pub const THRESHOLD_C: f64 = 8.0;

    /// Douglas-Peucker tolerance as a share of the contour's perimeter (`approxPolyDP(0.02 ×
    /// perimeter)`).
    pub const APPROX_EPSILON_SHARE: f32 = 0.02;

    /// Share of the page below which a quad is not considered (`MIN_AREA_FRACTION`). Not
    /// re-measured since the prototype's first phase.
    pub const MIN_AREA_FRACTION: f64 = 0.004;

    /// Longest over shortest side of the quad's minimum-area rectangle (`ASPECT_MAX`). Wide on
    /// purpose: a printed board's contour rarely closes square, and a caption stretches it. It
    /// measures the quad in its own tilt: an axis-aligned box made a 45° hatching band (sides
    /// 620 × 314, elongation 1.98) look squarer than the board (S-160).
    pub const ASPECT_MAX: f64 = 1.62;

    /// Share of the page at which the size part of the geometry saturates (`AREA_SATURATION`),
    /// so a large box does not win on size alone.
    pub const AREA_SATURATION: f64 = 0.12;

    /// Power of the squareness term in the geometry score (`squareness ** 2.4`).
    pub const SQUARENESS_EXPONENT: f64 = 2.4;

    /// Least share of the quad's box that must lie on the page (`MIN_VISIBLE_RATIO`): cuts quads
    /// that bleed off the sheet.
    pub const MIN_VISIBLE_RATIO: f64 = 0.65;

    /// Least share of the 4 corners on the page (`MIN_QUAD_INSIDE_RATIO`), each allowed
    /// [`QUAD_MARGIN_RATIO`] of the page's side outside it.
    pub const MIN_QUAD_INSIDE_RATIO: f64 = 0.5;
    pub const QUAD_MARGIN_RATIO: f64 = 0.015;

    /// Checker contrast at or below which a quad is not a board (`MIN_CHECKER_CONTRAST`, S-143):
    /// exactly where the contrast-against-spread comparison changes sign, so it does not depend
    /// on the corpus. The closest legitimate board measured 0.0616 (an opening position).
    pub const MIN_CHECKER_CONTRAST: f64 = 0.0;

    /// Score is `geometry × (0.55 + 0.45 × texture)`.
    pub const SCORE_BASE: f64 = 0.55;
    pub const SCORE_TEXTURE_WEIGHT: f64 = 0.45;

    /// Above this box IoU two quads (from the three binarisations or the two scales) are the
    /// same candidate, and the lower-scored one goes (`DEDUPE_IOU`).
    pub const DEDUPE_IOU: f64 = 0.9;

    /// Share of the largest candidate's area below which the others are noise of the same page
    /// (`MIN_RELATIVE_AREA`): a single square of a large board is 1/64 = 0.016 of it.
    pub const MIN_RELATIVE_AREA: f64 = 0.02;

    /// Absolute score floor and floor relative to the best candidate; the larger applies
    /// (`MIN_SCORE_FLOOR`, `MIN_SCORE_RELATIVE`).
    pub const MIN_SCORE_FLOOR: f64 = 0.06;
    pub const MIN_SCORE_RELATIVE: f64 = 0.25;

    /// Above this box IoU with an already selected board a candidate is the same diagram
    /// (`detect_boards`, `iou_threshold`).
    pub const SELECT_IOU: f64 = 0.25;

    /// Most boards per page (`DEFAULT_MAX_BOARDS`): 8 against 30 on 6 books lost only 3 of 58
    /// boards in one, and raising it admitted nothing; 12 covers 3 × 4 grids.
    pub const MAX_BOARDS: usize = 12;

    /// The extra search runs on the page shrunk by this factor (`RecallOptions.scales = (0.5,)`).
    /// Measured on the field set: recall 0.9478 without it, 0.9826 with it; 0.66 and 0.35 gave
    /// 0.9739, and adding 0.35 to 0.5 changed nothing.
    pub const RECALL_SCALE_DIVISOR: u32 = 2;

    /// Least elongation of a checker-less quad for the square rescue to try it
    /// (`SQUARE_MIN_ELONGATION`): below it the largest square inside is the quad itself. The
    /// measured `Reinfeld` cases were 1.087 and 1.090. Rescue alone took recall to 0.9652, and
    /// with the half-size search 0.9913 (precision 1.0).
    pub const SQUARE_MIN_ELONGATION: f64 = 1.02;

    /// Most pixels the detector works on. A safety cap, not a measurement: a larger page is
    /// shrunk by the smallest whole factor that fits (12 MP is US Letter at 350 DPI; the
    /// prototype rendered at 220). Working memory is a few bytes per pixel.
    pub const MAX_PAGE_PIXELS: usize = 12_000_000;

    /// Most contours per binarisation that pass the size prefilter and are measured. A safety
    /// cap: a real page has a handful; each costs a 320 px warp.
    pub const MAX_CONTOURS_PER_PASS: usize = 1024;

    /// Contours with more points are skipped. A safety cap on the polygon work: a board's
    /// outline at the page cap has some tens of thousands.
    pub const MAX_CONTOUR_POINTS: usize = 400_000;

    /// Most checker-less quads offered to the square rescue per page. A safety cap.
    pub const MAX_RESCUES: usize = 64;
}

use consts::*;

/// A pixel box as the prototype keeps it: whole-pixel origin and size, the size at least 1
/// (`_bbox_from_quad`).
#[derive(Clone, Copy, Debug, PartialEq)]
struct BoxPx {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

/// A quad that passed the guards, with its score, box and area.
#[derive(Clone, Debug, PartialEq)]
struct Scored {
    quad: Quad,
    score: f64,
    bbox: BoxPx,
    area: f64,
}

/// What one search at one scale found, and the boxes of the quads it dropped for having no
/// checker contrast (the square rescue's input).
struct Pass {
    found: Vec<Scored>,
    unreadable: Vec<BoxPx>,
}

/// The diagram candidates on a page rendered to `page`, a gray raster of the whole displayed
/// page (view space: after `/Rotate`, top row at the top) at `dpi`. Boxes and corners are in
/// user space; candidates come best score first (reading order is `order`'s job).
///
/// Each candidate carries the quad's corners, in the order top-left, top-right, bottom-right,
/// bottom-left **as displayed**, so S8 can warp the board from them. A page above
/// [`consts::MAX_PAGE_PIXELS`] is shrunk first; a non-finite or non-positive DPI gives nothing.
pub fn candidates(page: &GrayImage, geometry: &PageGeometry, dpi: f32) -> Vec<DiagramCandidate> {
    candidates_capped(page, geometry, dpi, MAX_PAGE_PIXELS)
}

fn candidates_capped(page: &GrayImage, geometry: &PageGeometry, dpi: f32, max_pixels: usize) -> Vec<DiagramCandidate> {
    if !(dpi.is_finite() && dpi > 0.0) {
        return Vec::new();
    }
    let (w, h) = dims(page);
    let mut factor = 1_u32;
    while (w / factor as usize).saturating_mul(h / factor as usize) > max_pixels {
        factor = factor.saturating_add(1);
    }
    let shrunk;
    let work = if factor == 1 {
        page
    } else {
        let Some(smaller) = downscale(page, factor) else { return Vec::new() };
        shrunk = smaller;
        &shrunk
    };
    let work_dpi = dpi / factor as f32;
    select(detect_quads(work)).iter().filter_map(|s| to_candidate(s, work, geometry, work_dpi)).collect()
}

/// `image` shrunk by a whole `factor`, each pixel the mean of a `factor` × `factor` block. The
/// last partial row and column are dropped. `None` when nothing is left.
///
/// **Deviation.** OpenCV's `INTER_AREA` at a scale that does not divide the size spreads the
/// remainder fractionally; dropping it moves nothing by more than one source pixel.
fn downscale(image: &GrayImage, factor: u32) -> Option<GrayImage> {
    let (w, h) = (image.width() / factor.max(1), image.height() / factor.max(1));
    if w == 0 || h == 0 {
        return None;
    }
    let f = usize::try_from(factor).ok()?;
    let width = usize::try_from(w).ok()?;
    let mut data = Vec::with_capacity(width.checked_mul(usize::try_from(h).ok()?)?);
    let mut sums = vec![0_u32; width];
    for y in 0..h {
        sums.iter_mut().for_each(|s| *s = 0);
        for dy in 0..factor {
            let row = image.row(y * factor + dy)?;
            for (sum, block) in sums.iter_mut().zip(row.chunks_exact(f)) {
                *sum += block.iter().map(|&v| u32::from(v)).sum::<u32>();
            }
        }
        let count = factor.saturating_mul(factor).max(1);
        data.extend(sums.iter().map(|&s| u8::try_from((s + count / 2) / count).unwrap_or(u8::MAX)));
    }
    GrayImage::new(w, h, data).ok()
}

/// Every scored quad on the page: the full-size search, the half-size one and the square
/// rescue, pooled, deduplicated and cut to size (`_extract_candidate_quads`), best first.
fn detect_quads(image: &GrayImage) -> Vec<Scored> {
    let (w, h) = dims(image);
    let area = (w * h) as f64;
    let full = search(image, true);
    let mut pooled = full.found;

    if let Some(half) = downscale(image, RECALL_SCALE_DIVISOR) {
        let back = f64::from(RECALL_SCALE_DIVISOR) as f32;
        for found in search(&half, false).found {
            let quad = found.quad.map(|(x, y)| (x * back, y * back));
            let bbox = box_of(&quad);
            // A duplicate of a full-size find is dropped before the expensive rescoring.
            if pooled.iter().any(|other| iou(&bbox, &other.bbox) > DEDUPE_IOU) {
                continue;
            }
            if let Some(scored) = score_quad(image, quad, area) {
                pooled.push(scored);
            }
        }
    }

    pooled.extend(rescue(image, &full.unreadable, area));
    finish(pooled)
}

/// The largest squares inside the elongated checker-less quads, scored on the page: when a
/// contour joins a board to its caption, the 64 squares fall off the grid and the checker test
/// rightly drops the quad; the square inside it is the board (`square_anchors`). Measured on
/// `Reinfeld`: checker contrast 0.0000 for the joined quad, 0.2317 and 0.3209 for the squares.
fn rescue(image: &GrayImage, unreadable: &[BoxPx], area: f64) -> Vec<Scored> {
    let mut found = Vec::new();
    for bbox in unreadable.iter().take(MAX_RESCUES) {
        let shorter = bbox.w.min(bbox.h);
        if shorter <= 0.0 || bbox.w.max(bbox.h) / shorter < SQUARE_MIN_ELONGATION {
            continue;
        }
        found.extend(square_anchors(bbox).into_iter().filter_map(|quad| score_quad(image, quad, area)));
    }
    found
}

/// One search at one scale (`_contour_candidates`). `record` keeps the boxes of quads dropped
/// for having no checker contrast.
fn search(image: &GrayImage, record: bool) -> Pass {
    let (w, h) = dims(image);
    let area = (w * h) as f64;
    let base = adaptive_threshold_inv(&gaussian_blur_5(image), THRESHOLD_BLOCK, THRESHOLD_C);
    let mut raw = Vec::new();
    let mut unreadable = Vec::new();
    for pass in 0..3 {
        // Three images, not one: joining two components removes both from the contour list, so
        // the tight contour of one pass would vanish if the passes were merged (S-175).
        let closed;
        let mask: &[bool] = match pass {
            0 => &base,
            1 => {
                closed = close(&base, w, &SQUARE_3);
                &closed
            }
            _ => {
                closed = union(&close(&base, w, &DIAGONAL_3), &close(&base, w, &ANTI_DIAGONAL_3));
                &closed
            }
        };
        let Ok(view) = NdTensorView::try_from_data([h, w], mask) else { continue };
        let contours = find_contours(view, RetrievalMode::List);
        let mut measured = 0_usize;
        for contour in contours.iter() {
            if contour.len() < 4 || contour.len() > MAX_CONTOUR_POINTS {
                continue;
            }
            // The quad's corners are contour points or lie on its minimum-area rectangle, so its
            // area never exceeds the contour's box: this prefilter drops only quads the area
            // floor would drop, before any polygon work.
            let (x0, x1) = contour.iter().fold((i32::MAX, i32::MIN), |(lo, hi), p| (lo.min(p.x), hi.max(p.x)));
            let (y0, y1) = contour.iter().fold((i32::MAX, i32::MIN), |(lo, hi), p| (lo.min(p.y), hi.max(p.y)));
            if f64::from(x1.saturating_sub(x0)) * f64::from(y1.saturating_sub(y0)) < area * MIN_AREA_FRACTION {
                continue;
            }
            measured += 1;
            if measured > MAX_CONTOURS_PER_PASS {
                break;
            }
            let points: Vec<PointF> = contour.iter().map(|p| p.to_f32()).collect();
            let Some(quad) = quad_of(&points) else { continue };
            match measure(image, quad, area) {
                Measured::Board(scored) => raw.push(scored),
                Measured::NoChecker(bbox) if record => unreadable.push(bbox),
                Measured::NoChecker(_) | Measured::Rejected => {}
            }
        }
    }
    Pass { found: finish(raw), unreadable }
}

/// What the guards made of a quad.
enum Measured {
    Board(Scored),
    /// Shaped and placed like a diagram, but with no checker contrast: its box.
    NoChecker(BoxPx),
    Rejected,
}

/// The guards in the prototype's order: geometry, position on the page, checker contrast.
fn measure(image: &GrayImage, quad: Quad, area: f64) -> Measured {
    let geometry = geometry_score(&quad, area);
    if geometry <= 0.0 {
        return Measured::Rejected;
    }
    let bbox = box_of(&quad);
    let (w, h) = dims(image);
    if visible_ratio(&bbox, w, h) < MIN_VISIBLE_RATIO || inside_ratio(&quad, w, h) < MIN_QUAD_INSIDE_RATIO {
        return Measured::Rejected;
    }
    let Some((checker, grid)) = quad_texture_parts(image, quad) else { return Measured::Rejected };
    // Before any competition (S-143, moved there in S-160): a checker-less winner would suppress
    // the real board by overlap and only then be dropped, taking the board with it.
    if checker <= MIN_CHECKER_CONTRAST {
        return Measured::NoChecker(bbox);
    }
    let score = geometry * (SCORE_BASE + SCORE_TEXTURE_WEIGHT * texture_from_parts(checker, grid));
    Measured::Board(Scored { quad, score, bbox, area: polygon_area(&quad) })
}

/// A quad from the half-size search or the rescue, scored on the full page by the same guards
/// (`_score_quad`), or `None` when a guard drops it.
fn score_quad(image: &GrayImage, quad: Quad, area: f64) -> Option<Scored> {
    match measure(image, quad, area) {
        Measured::Board(scored) => Some(scored),
        Measured::NoChecker(_) | Measured::Rejected => None,
    }
}

/// The largest square inside a box, at its start, its end and its middle along the long axis
/// (`square_anchors`): a caption joined under a board leaves the board at the start.
fn square_anchors(bbox: &BoxPx) -> [Quad; 3] {
    let side = bbox.w.min(bbox.h);
    let (slack_x, slack_y) = (bbox.w - side, bbox.h - side);
    [(0.0, 0.0), (slack_x, slack_y), (slack_x / 2.0, slack_y / 2.0)].map(|(ox, oy)| {
        let (ax, ay) = ((bbox.x + ox) as f32, (bbox.y + oy) as f32);
        let side = side as f32;
        [(ax, ay), (ax + side, ay), (ax + side, ay + side), (ax, ay + side)]
    })
}

/// Sorts best first, drops a candidate whose box overlaps a better one's above
/// [`consts::DEDUPE_IOU`], then those under [`consts::MIN_RELATIVE_AREA`] of the largest
/// (`_pool_finish`).
fn finish(mut pooled: Vec<Scored>) -> Vec<Scored> {
    pooled.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut kept: Vec<Scored> = Vec::new();
    for candidate in pooled {
        if !kept.iter().any(|other| iou(&candidate.bbox, &other.bbox) > DEDUPE_IOU) {
            kept.push(candidate);
        }
    }
    let largest = kept.iter().map(|s| s.area).fold(0.0_f64, f64::max);
    kept.retain(|s| s.area >= largest * MIN_RELATIVE_AREA);
    kept
}

/// The boards to report from candidates sorted best first (`detect_boards`): above the score
/// floor, not overlapping a better one beyond [`consts::SELECT_IOU`], at most
/// [`consts::MAX_BOARDS`].
fn select(candidates: Vec<Scored>) -> Vec<Scored> {
    let top = candidates.first().map_or(0.0, |s| s.score);
    let floor = MIN_SCORE_FLOOR.max(top * MIN_SCORE_RELATIVE);
    let mut selected: Vec<Scored> = Vec::new();
    for candidate in candidates {
        if candidate.score < floor || selected.iter().any(|kept| iou(&candidate.bbox, &kept.bbox) > SELECT_IOU) {
            continue;
        }
        if selected.len() >= MAX_BOARDS {
            break;
        }
        selected.push(candidate);
    }
    selected
}

/// A scored quad as a candidate in user space, or `None` when it does not map to a box.
fn to_candidate(scored: &Scored, image: &GrayImage, geometry: &PageGeometry, dpi: f32) -> Option<DiagramCandidate> {
    let to_user = |(x, y): (f32, f32)| -> Option<(f64, f64)> {
        let [ux, uy] = geometry.view_to_user_point(px_to_pt(x, dpi)?, px_to_pt(y, dpi)?)?;
        Some((f64::from(ux), f64::from(uy)))
    };
    let [a, b, c, d] = order_quad(scored.quad)?;
    let corners = [to_user(a)?, to_user(b)?, to_user(c)?, to_user(d)?];
    // The box is the quad's extent on the page; corners may stray just off it.
    let (w, h) = (image.width() as f32, image.height() as f32);
    let xs = scored.quad.map(|(x, _)| x.clamp(0.0, w));
    let ys = scored.quad.map(|(_, y)| y.clamp(0.0, h));
    let (x0, x1) = (xs.iter().copied().fold(f32::INFINITY, f32::min), xs.iter().copied().fold(f32::NEG_INFINITY, f32::max));
    let (y0, y1) = (ys.iter().copied().fold(f32::INFINITY, f32::min), ys.iter().copied().fold(f32::NEG_INFINITY, f32::max));
    let view = ViewRect::new(px_to_pt(x0, dpi)?, px_to_pt(y0, dpi)?, px_to_pt(x1, dpi)?, px_to_pt(y1, dpi)?);
    let bbox_user = geometry.view_to_user(view)?;
    if !(bbox_user.width() > 0.0 && bbox_user.height() > 0.0) {
        return None;
    }
    let dpi_hint = dpi.clamp(super::embedded::consts::MIN_DPI_HINT, super::embedded::consts::MAX_DPI_HINT);
    Some(DiagramCandidate { bbox_user, source: Source::Contour, score: scored.score as f32, dpi_hint, placement: None, corners: Some(corners) })
}

/// A contour's quad: its Douglas-Peucker simplification when that has 4 corners, else its
/// minimum-area rectangle. `None` for a contour with no length.
fn quad_of(points: &[PointF]) -> Option<Quad> {
    let perimeter: f32 = points.iter().zip(points.iter().cycle().skip(1)).map(|(a, b)| a.distance(*b)).sum();
    if !(perimeter.is_finite() && perimeter > 0.0) {
        return None;
    }
    if let [a, b, c, d] = approx_closed(points, APPROX_EPSILON_SHARE * perimeter)[..] {
        return Some([(a.x, a.y), (b.x, b.y), (c.x, c.y), (d.x, d.y)]);
    }
    let rect = min_area_rect(points)?;
    Some(rect.corners().map(|p| (p.x, p.y)))
}

/// Douglas-Peucker on a closed contour, split as OpenCV's `approxPolyDP` splits it: between the
/// two points found by moving three times to the point farthest from the current one.
///
/// **Deviation.** Written here rather than `rten_imageproc::simplify_polygon` because that one
/// recurses once per kept split (a hostile contour could exhaust the stack) and always keeps
/// the contour's first point, which can leave a fifth corner on a rotated board. The distance is
/// to the line through the chord, as in OpenCV.
fn approx_closed(points: &[PointF], epsilon: f32) -> Vec<PointF> {
    let n = points.len();
    if n < 3 || !(epsilon.is_finite() && epsilon >= 0.0) {
        return points.to_vec();
    }
    let point = |i: usize| points.get(i % n).copied().unwrap_or(PointF::from_yx(0.0, 0.0));
    let farthest = |from: usize| -> (usize, f32) {
        let origin = point(from);
        points.iter().enumerate().fold((from, 0.0), |(best, d), (i, p)| {
            let dist = origin.distance(*p);
            if dist > d { (i, dist) } else { (best, d) }
        })
    };
    let (mut a, (mut b, mut span)) = (0, farthest(0));
    for _ in 0..2 {
        a = b;
        (b, span) = farthest(a);
    }
    if span <= epsilon || a == b {
        return vec![point(a)];
    }
    let mut out = Vec::new();
    for (from, to) in [(a, b), (b, a)] {
        let steps = (to + n - from) % n;
        let arc: Vec<PointF> = (0..=steps).map(|k| point(from + k)).collect();
        let kept = simplify_open(&arc, epsilon);
        out.extend(kept.iter().take(kept.len().saturating_sub(1)));
    }
    out
}

/// Douglas-Peucker on an open polyline, with an explicit stack. Keeps both ends.
fn simplify_open(points: &[PointF], epsilon: f32) -> Vec<PointF> {
    let Some(last) = points.len().checked_sub(1) else { return Vec::new() };
    let mut keep = vec![false; points.len()];
    for i in [0, last] {
        if let Some(slot) = keep.get_mut(i) {
            *slot = true;
        }
    }
    let mut stack = vec![(0, last)];
    while let Some((start, end)) = stack.pop() {
        if end <= start + 1 {
            continue;
        }
        let (Some(&p), Some(&q)) = (points.get(start), points.get(end)) else { continue };
        let (best, distance) = (start + 1..end).fold((start, -1.0_f32), |(best, d), i| {
            let dist = points.get(i).map_or(0.0, |r| line_distance(*r, p, q));
            if dist > d { (i, dist) } else { (best, d) }
        });
        if distance > epsilon {
            if let Some(slot) = keep.get_mut(best) {
                *slot = true;
            }
            stack.push((start, best));
            stack.push((best, end));
        }
    }
    points.iter().zip(&keep).filter(|(_, k)| **k).map(|(p, _)| *p).collect()
}

/// Distance from `r` to the line through `p` and `q` (to `p` when they coincide).
fn line_distance(r: PointF, p: PointF, q: PointF) -> f32 {
    let (dx, dy) = (q.x - p.x, q.y - p.y);
    let length = (dx * dx + dy * dy).sqrt();
    if length <= f32::EPSILON {
        return r.distance(p);
    }
    ((r.x - p.x) * dy - (r.y - p.y) * dx).abs() / length
}

/// Size and squareness of a quad, in 0..=1; 0 when it is too small or too elongated
/// (`_contour_geometry_score`).
fn geometry_score(quad: &Quad, image_area: f64) -> f64 {
    let area = polygon_area(quad);
    // NaN fails `is_finite`, so it is refused here rather than slipping through a comparison.
    if !area.is_finite() || area <= 0.0 || area < image_area * MIN_AREA_FRACTION {
        return 0.0;
    }
    let elongation = elongation(quad);
    // Squareness below reaches 0 at exactly `ASPECT_MAX`, so this guard only says so early
    // (the prototype logs it as its own rejection reason, "aspecto").
    if elongation.is_nan() || elongation > ASPECT_MAX {
        return 0.0;
    }
    let size = (area / image_area / AREA_SATURATION).min(1.0);
    let squareness = (1.0 - elongation.ln() / ASPECT_MAX.ln()).max(0.0);
    size * squareness.powf(SQUARENESS_EXPONENT)
}

/// Longest over shortest side of the quad's minimum-area rectangle, at least 1; infinite for a
/// flat quad (`_quad_elongation`).
fn elongation(quad: &Quad) -> f64 {
    let points = quad.map(|(x, y)| PointF::from_yx(y, x));
    let Some(rect) = min_area_rect(&points) else { return f64::INFINITY };
    let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
    if !(w > 0.0 && h > 0.0) {
        return f64::INFINITY;
    }
    w.max(h) / w.min(h)
}

/// The quad's area by the shoelace formula (`contourArea`).
fn polygon_area(quad: &Quad) -> f64 {
    let twice: f64 =
        quad.iter().zip(quad.iter().cycle().skip(1)).map(|(a, b)| f64::from(a.0) * f64::from(b.1) - f64::from(b.0) * f64::from(a.1)).sum();
    (twice / 2.0).abs()
}

/// The quad's box: whole-pixel origin and far corner, size at least 1 (`_bbox_from_quad`).
fn box_of(quad: &Quad) -> BoxPx {
    let fold = |f: fn(&(f32, f32)) -> f32| quad.iter().map(f).fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), v| (lo.min(v), hi.max(v)));
    let ((x0, x1), (y0, y1)) = (fold(|p| p.0), fold(|p| p.1));
    // `int()` truncates toward zero.
    let (x0, y0, x1, y1) = (f64::from(x0).trunc(), f64::from(y0).trunc(), f64::from(x1).trunc(), f64::from(y1).trunc());
    BoxPx { x: x0, y: y0, w: (x1 - x0).max(1.0), h: (y1 - y0).max(1.0) }
}

/// Intersection over union of two boxes (`_bbox_iou`).
fn iou(a: &BoxPx, b: &BoxPx) -> f64 {
    let iw = ((a.x + a.w).min(b.x + b.w) - a.x.max(b.x)).max(0.0);
    let ih = ((a.y + a.h).min(b.y + b.h) - a.y.max(b.y)).max(0.0);
    let inter = iw * ih;
    if inter <= 0.0 {
        return 0.0;
    }
    let union = a.w * a.h + b.w * b.h - inter;
    if union > 0.0 { inter / union } else { 0.0 }
}

/// Share of the box on the page (`_bbox_visible_ratio`).
fn visible_ratio(bbox: &BoxPx, width: usize, height: usize) -> f64 {
    let iw = ((bbox.x + bbox.w).min(width as f64) - bbox.x.max(0.0)).max(0.0);
    let ih = ((bbox.y + bbox.h).min(height as f64) - bbox.y.max(0.0)).max(0.0);
    iw * ih / (bbox.w * bbox.h)
}

/// Share of the quad's corners on the page, with a margin (`_quad_point_inside_ratio`).
fn inside_ratio(quad: &Quad, width: usize, height: usize) -> f64 {
    let (w, h) = (width as f64, height as f64);
    let (mx, my) = (w * QUAD_MARGIN_RATIO, h * QUAD_MARGIN_RATIO);
    let inside = quad
        .iter()
        .filter(|(x, y)| {
            let (x, y) = (f64::from(*x), f64::from(*y));
            x >= -mx && x <= (w - 1.0) + mx && y >= -my && y <= (h - 1.0) + my
        })
        .count();
    inside as f64 / 4.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coords::{PageRect, Rotation};

    const LETTER: PageRect = PageRect { x0: 0.0, y0: 0.0, x1: 612.0, y1: 792.0 };
    const LIGHT: u8 = 240;
    const DARK: u8 = 70;

    fn letter(rotation: Rotation) -> PageGeometry {
        PageGeometry::new(LETTER, rotation).unwrap()
    }

    /// A white page to draw synthetic fixtures on.
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

        /// A framed 8 × 8 board turned `degrees` clockwise about its centre `(cx, cy)`: `side`
        /// pixels of squares inside a `frame`-pixel black frame.
        fn board(&mut self, cx: f64, cy: f64, side: f64, frame: f64, degrees: f64) {
            let (sin, cos) = degrees.to_radians().sin_cos();
            let half = side / 2.0 + frame;
            for y in 0..self.height {
                for x in 0..self.width {
                    let (dx, dy) = (x as f64 + 0.5 - cx, y as f64 + 0.5 - cy);
                    // Into the board's own frame: undo the turn.
                    let (u, v) = (dx * cos + dy * sin, -dx * sin + dy * cos);
                    if u.abs() >= half || v.abs() >= half {
                        continue;
                    }
                    let (bu, bv) = (u + side / 2.0, v + side / 2.0);
                    let value = if bu < 0.0 || bv < 0.0 || bu >= side || bv >= side {
                        0
                    } else {
                        let (c, r) = ((bu * 8.0 / side) as u32, (bv * 8.0 / side) as u32);
                        if (r + c) % 2 == 1 { DARK } else { LIGHT }
                    };
                    self.data[(y * self.width + x) as usize] = value;
                }
            }
        }

        fn image(self) -> GrayImage {
            GrayImage::new(self.width, self.height, self.data).unwrap()
        }
    }

    fn close_rect(a: PageRect, b: PageRect, tolerance: f32) -> bool {
        [(a.x0, b.x0), (a.y0, b.y0), (a.x1, b.x1), (a.y1, b.y1)].iter().all(|(p, q)| (p - q).abs() <= tolerance)
    }

    /// A US Letter page at 72 DPI (one pixel per point) with one board: squares 25 px, frame
    /// 2 px, so the frame's outer edge spans pixels 98..302 across and 148..352 down.
    fn one_board_page() -> GrayImage {
        let mut page = Canvas::new(612, 792);
        page.board(200.0, 250.0, 200.0, 2.0, 0.0);
        page.image()
    }

    #[test]
    fn one_framed_board_is_found_with_its_box_in_user_space() {
        let found = candidates(&one_board_page(), &letter(Rotation::R0), 72.0);
        assert_eq!(found.len(), 1, "{found:?}");
        let board = &found[0];
        assert_eq!(board.source, Source::Contour);
        // View y 148..352 is user y 792 − 352 .. 792 − 148.
        assert!(close_rect(board.bbox_user, PageRect::new(98.0, 440.0, 302.0, 644.0), 3.0), "{:?}", board.bbox_user);
        let corners = board.corners.expect("contour candidates carry their quad");
        // Top-left as displayed is the user-space corner with the smallest x and largest y.
        assert!((corners[0].0 - 98.0).abs() < 3.0 && (corners[0].1 - 644.0).abs() < 3.0, "{corners:?}");
        assert!((corners[2].0 - 302.0).abs() < 3.0 && (corners[2].1 - 440.0).abs() < 3.0, "{corners:?}");
        assert!(board.score > 0.3, "{}", board.score);
        assert_eq!(board.dpi_hint, 72.0);
    }

    #[test]
    fn two_boards_on_a_page_are_both_found() {
        let mut page = Canvas::new(612, 792);
        page.board(170.0, 200.0, 180.0, 2.0, 0.0);
        page.board(420.0, 560.0, 200.0, 3.0, 0.0);
        let found = candidates(&page.image(), &letter(Rotation::R0), 72.0);
        assert_eq!(found.len(), 2, "{found:?}");
        let mut boxes: Vec<PageRect> = found.iter().map(|c| c.bbox_user).collect();
        boxes.sort_by(|a, b| a.x0.total_cmp(&b.x0));
        assert!(close_rect(boxes[0], PageRect::new(78.0, 500.0, 262.0, 684.0), 3.0), "{boxes:?}");
        assert!(close_rect(boxes[1], PageRect::new(317.0, 129.0, 523.0, 335.0), 3.0), "{boxes:?}");
    }

    #[test]
    fn a_board_turned_five_degrees_gives_its_turned_quad() {
        let mut page = Canvas::new(612, 792);
        let (cx, cy, half) = (300.0, 380.0, 112.0);
        page.board(cx, cy, 220.0, 2.0, 5.0);
        let found = candidates(&page.image(), &letter(Rotation::R0), 72.0);
        assert_eq!(found.len(), 1, "{found:?}");
        let (sin, cos) = 5.0_f64.to_radians().sin_cos();
        // The frame's outer corners, turned clockwise (y down), in view pixels = points.
        let expected =
            [(-half, -half), (half, -half), (half, half), (-half, half)].map(|(u, v): (f64, f64)| (cx + u * cos - v * sin, cy + u * sin + v * cos));
        let corners = found[0].corners.unwrap();
        for ((ux, uy), (vx, vy)) in corners.iter().zip(expected) {
            // User y is 792 − view y.
            let (gx, gy) = (*ux, 792.0 - uy);
            assert!((gx - vx).abs() < 3.0 && (gy - vy).abs() < 3.0, "corner ({gx:.1}, {gy:.1}) against ({vx:.1}, {vy:.1}): {corners:?}");
        }
    }

    #[test]
    fn the_box_comes_back_through_a_page_turn() {
        // At /Rotate 90 the displayed page is 792 × 612; view (x, y) is user (y, x).
        let mut page = Canvas::new(792, 612);
        page.board(300.0, 250.0, 200.0, 2.0, 0.0);
        let found = candidates(&page.image(), &letter(Rotation::R90), 72.0);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(close_rect(found[0].bbox_user, PageRect::new(148.0, 198.0, 352.0, 402.0), 3.0), "{:?}", found[0].bbox_user);
    }

    #[test]
    fn a_page_of_text_has_no_board() {
        // Lines of word-like blobs, 3 px apart within a word: the close bridges none of them.
        let mut page = Canvas::new(612, 792);
        let mut seed: u32 = 0x9e37_79b9;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        for line in 0..48 {
            let y = 40 + line * 15;
            let mut x = 50;
            while x < 560 {
                let letters = 2 + next() % 8;
                for _ in 0..letters {
                    let (w, h) = (2 + next() % 5, 5 + next() % 5);
                    page.fill(x, y + 9 - h, x + w, y + 9, (next() % 80) as u8);
                    x += w + 3;
                }
                x += 9;
            }
        }
        assert!(candidates(&page.image(), &letter(Rotation::R0), 72.0).is_empty());
    }

    #[test]
    fn a_table_grid_is_rejected_by_its_texture() {
        // An 8 × 8 grid of empty cells: the shape and the grid lines of a board, no checker.
        let mut page = Canvas::new(612, 792);
        for k in 0..=8 {
            let at = 100 + k * 25;
            page.fill(at, 100, at + 2, 302, 0);
            page.fill(100, at, 302, at + 2, 0);
        }
        assert!(candidates(&page.image(), &letter(Rotation::R0), 72.0).is_empty());
    }

    #[test]
    fn degenerate_pages_give_nothing_and_do_not_panic() {
        let page = letter(Rotation::R0);
        for image in [
            GrayImage::new(1, 1, vec![0]).unwrap(),
            GrayImage::new(1, 1, vec![255]).unwrap(),
            GrayImage::new(3, 2, vec![0; 6]).unwrap(),
            GrayImage::new(300, 300, vec![255; 90_000]).unwrap(),
            GrayImage::new(300, 300, vec![0; 90_000]).unwrap(),
            GrayImage::new(1, 400, vec![0; 400]).unwrap(),
        ] {
            assert!(candidates(&image, &page, 72.0).is_empty(), "{}x{}", image.width(), image.height());
        }
        let board = one_board_page();
        for dpi in [0.0, -72.0, f32::NAN, f32::INFINITY] {
            assert!(candidates(&board, &page, dpi).is_empty(), "dpi {dpi}");
        }
    }

    #[test]
    fn a_page_over_the_pixel_cap_is_shrunk_and_still_mapped_to_points() {
        // The same board at 144 DPI (2 px per point), with a cap that forces a halving.
        let mut page = Canvas::new(1224, 1584);
        page.board(400.0, 500.0, 400.0, 4.0, 0.0);
        let image = page.image();
        let found = candidates_capped(&image, &letter(Rotation::R0), 144.0, 1224 * 1584 / 3);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(close_rect(found[0].bbox_user, PageRect::new(98.0, 440.0, 302.0, 644.0), 3.0), "{:?}", found[0].bbox_user);
        assert_eq!(found[0].dpi_hint, 72.0, "measured on the halved page");
    }

    #[test]
    fn a_board_joined_to_its_caption_is_rescued_as_a_square() {
        // A caption bar touching the frame's foot: one contour, stretched 12 % down, whose 8 × 8
        // grid falls off the squares.
        let mut page = Canvas::new(612, 792);
        page.board(200.0, 250.0, 200.0, 2.0, 0.0);
        page.fill(98, 352, 302, 376, 0);
        let image = page.image();
        let full = search(&image, true);
        assert!(full.found.is_empty() && !full.unreadable.is_empty(), "premise: only the joined, checker-less quad is found");
        // The rescue alone finds the board (the half-size search happens to as well, so the
        // end-to-end check below would not notice the rescue missing).
        let rescued = finish(rescue(&image, &full.unreadable, 612.0 * 792.0));
        let best = rescued.first().expect("the square at the top of the joined quad is the board");
        assert_eq!(best.bbox, BoxPx { x: 98.0, y: 147.0, w: 204.0, h: 204.0 });
        let found = candidates(&image, &letter(Rotation::R0), 72.0);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(close_rect(found[0].bbox_user, PageRect::new(98.0, 440.0, 302.0, 644.0), 4.0), "{:?}", found[0].bbox_user);
    }

    /// Dark squares drawn one pixel short on their right and bottom, so two of them meet only
    /// across a one-pixel diagonal gap: the lost corner contact of S-175.
    fn corner_gap_board() -> GrayImage {
        let mut page = Canvas::new(612, 792);
        for row in 0..8 {
            for column in 0..8 {
                if (row + column) % 2 == 1 {
                    let (x, y) = (100 + column * 25, 100 + row * 25);
                    page.fill(x, y, x + 24, y + 24, 0);
                }
            }
        }
        page.image()
    }

    #[test]
    fn the_diagonal_close_rejoins_squares_that_lost_their_corner_contact() {
        // One scale only: at half size the averaging rejoins the squares by itself.
        let found = search(&corner_gap_board(), false).found;
        let best = found.first().expect("the diagonal pass joins the squares into one board");
        assert!((best.bbox.x - 100.0).abs() <= 2.0 && (best.bbox.w - 199.0).abs() <= 3.0, "{:?}", best.bbox);
    }

    #[test]
    fn hatched_squares_are_found_on_the_half_size_page() {
        // Dark squares drawn as one-pixel hatching every 6 px, no frame: at full size every
        // stroke is its own contour (the 3 × 3 close bridges 2 px, not 5); averaged to half
        // size the strokes are 3 px apart and the closes make the squares solid.
        let mut page = Canvas::new(612, 792);
        for y in 0..200_u32 {
            for x in 0..200_u32 {
                if ((x / 25) + (y / 25)) % 2 == 1 && y % 6 == 0 {
                    page.data[((150 + y) * 612 + 150 + x) as usize] = 0;
                }
            }
        }
        let image = page.image();
        assert!(search(&image, false).found.is_empty(), "premise: the full-size search alone finds nothing");
        let found = candidates(&image, &letter(Rotation::R0), 72.0);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(close_rect(found[0].bbox_user, PageRect::new(150.0, 442.0, 350.0, 642.0), 4.0), "{:?}", found[0].bbox_user);
    }

    #[test]
    fn a_quad_mostly_off_the_page_is_rejected_before_its_texture_is_measured() {
        let image = one_board_page();
        let area = 612.0 * 792.0;
        // The board itself passes; the same-sized quad pushed two-thirds off the left edge does not.
        assert!(matches!(measure(&image, [(98.0, 148.0), (302.0, 148.0), (302.0, 352.0), (98.0, 352.0)], area), Measured::Board(_)));
        let off = [(-140.0, 148.0), (64.0, 148.0), (64.0, 352.0), (-140.0, 352.0)];
        assert!(matches!(measure(&image, off, area), Measured::Rejected));
        assert!(visible_ratio(&box_of(&off), 612, 792) < MIN_VISIBLE_RATIO);
        // Corners: two of four inside is enough, one is not.
        assert_eq!(inside_ratio(&[(-50.0, 10.0), (10.0, 10.0), (10.0, 20.0), (-50.0, 20.0)], 612, 792), 0.5);
    }

    fn scored(x: f64, side: f64, score: f64) -> Scored {
        let (x0, x1) = (x as f32, (x + side) as f32);
        let quad = [(x0, 0.0), (x1, 0.0), (x1, side as f32), (x0, side as f32)];
        Scored { quad, score, bbox: box_of(&quad), area: side * side }
    }

    #[test]
    fn selection_applies_the_floor_the_overlap_and_the_cap() {
        let picked = select(vec![scored(0.0, 100.0, 0.8), scored(10.0, 100.0, 0.7), scored(300.0, 100.0, 0.3), scored(600.0, 100.0, 0.1)]);
        // The second overlaps the first (IoU 0.82); the last is under a quarter of the best.
        assert_eq!(picked.iter().map(|s| s.score).collect::<Vec<_>>(), vec![0.8, 0.3]);
        let many: Vec<Scored> = (0..20).map(|i| scored(f64::from(i) * 200.0, 100.0, 0.5)).collect();
        assert_eq!(select(many).len(), MAX_BOARDS);
        assert!(select(vec![scored(0.0, 100.0, 0.05)]).is_empty(), "under the absolute floor");
    }

    #[test]
    fn finishing_drops_duplicates_and_specks() {
        let kept = finish(vec![scored(0.0, 100.0, 0.5), scored(1.0, 100.0, 0.6), scored(500.0, 10.0, 0.9)]);
        // The two near-identical boxes keep the better one; the 10 px one is 1 % of the largest.
        assert_eq!(kept.iter().map(|s| s.score).collect::<Vec<_>>(), vec![0.6]);
    }

    #[test]
    fn geometry_refuses_small_and_elongated_quads_measured_in_their_own_tilt() {
        let area = 1000.0 * 1000.0;
        let square = [(0.0, 0.0), (300.0, 0.0), (300.0, 300.0), (0.0, 300.0)];
        assert!(geometry_score(&square, area) > 0.7);
        let tiny = [(0.0, 0.0), (60.0, 0.0), (60.0, 60.0), (0.0, 60.0)];
        assert_eq!(geometry_score(&tiny, area), 0.0, "0.36 % of the page");
        // A 2:1 band turned 45°: its axis-aligned box is square, its own shape is not.
        let band = [(200.0, 0.0), (400.0, 200.0), (300.0, 300.0), (100.0, 100.0)];
        assert!((elongation(&band) - 2.0).abs() < 1e-3, "{}", elongation(&band));
        assert_eq!(geometry_score(&band, area), 0.0);
        assert_eq!(geometry_score(&[(0.0, 0.0); 4], area), 0.0);
    }

    #[test]
    fn approximation_finds_the_corners_of_a_skewed_outline() {
        // The outline of a turned trapezoid (a board seen in perspective), one point per step,
        // starting mid-edge. Its minimum-area rectangle would not have these corners.
        let corners = [(100.0, 0.0), (210.0, 90.0), (100.0, 230.0), (10.0, 100.0)];
        let mut outline = Vec::new();
        for (k, &(x0, y0)) in corners.iter().enumerate() {
            let (x1, y1) = corners[(k + 1) % 4];
            for step in 0..100 {
                let t = step as f32 / 100.0;
                outline.push(PointF::from_yx(y0 + (y1 - y0) * t, x0 + (x1 - x0) * t));
            }
        }
        outline.rotate_left(50);
        let quad = quad_of(&outline).unwrap();
        let ordered = order_quad(quad).unwrap();
        assert_eq!(order_quad(corners), Some(ordered), "{quad:?}");
        assert!(approx_closed(&outline[..2], 1.0).len() == 2 && approx_closed(&outline, f32::NAN).len() == outline.len());
    }

    #[test]
    fn anchors_slide_a_square_along_the_long_axis() {
        let anchors = square_anchors(&BoxPx { x: 10.0, y: 20.0, w: 100.0, h: 130.0 });
        assert_eq!(anchors[0][0], (10.0, 20.0));
        assert_eq!(anchors[1][0], (10.0, 50.0));
        assert_eq!(anchors[2][2], (110.0, 135.0));
    }
}
