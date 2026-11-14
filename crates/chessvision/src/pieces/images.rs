//! Piece sets from a folder of twelve piece images, traced into outlines.
//!
//! The folder holds one image per piece, PNG or SVG, named by colour and FEN letter as most chess
//! sites do: `wK.png wQ.svg wR.png … bP.png` (case does not matter; an SVG wins over a PNG of the
//! same name). Each image fills one square; what it leaves transparent shows the square.
//!
//! An SVG is vector art already and keeps its own paths ([`super::svg`]). Drawings are paths
//! everywhere (PDF, SVG, PNG), so a PNG becomes two filled outlines: its
//! opaque silhouette in black, then its light parts in white on top. The outlines are iso-lines
//! of the image at half intensity, found with marching squares on a bilinear upsampling, so an
//! anti-aliased image gives smooth edges at any size.
//!
//! **Large images** (plan A9-V6b, finding A3-07: twelve PNGs of 2,048 px took 2.4 s, on the
//! interface's thread). An image wider or taller than [`TARGET_GRID`] pixels is reduced to it by
//! an area average of its two fields (silhouette and light parts) before it is traced: the
//! outline is the same within a fraction of a pixel of the reduced grid, for a sixteenth of the
//! points. **Traced once**: each image's outlines are kept by the SHA-256 of its bytes
//! ([`MAX_CACHED_GLYPHS`] of them), so the set checked by an import, its copy checked again before
//! it takes the old one's place, its preview and the boards drawn with it are traced once in all.

use std::collections::HashMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use sha2::Digest as _;

use pdfcraft_chess::draw::{Layer, Paint, PieceGlyph, PieceSet, Rgb, Segment, Subpath};
use pdfcraft_chess::placement::{Color, PieceKind};

use super::PieceError;

/// Largest image file read.
const MAX_IMAGE_BYTES: usize = 8 << 20;
/// Largest image side, in pixels.
const MAX_SIDE: u32 = 2048;
/// Grid points per image pixel along each side, for images up to `TARGET_GRID` pixels wide.
const MAX_UPSAMPLE: usize = 4;
/// Upsampling stops once the grid is about this many points wide, and a larger image is reduced to
/// this many pixels on its longer side before it is traced.
const TARGET_GRID: usize = 512;
/// Most piece images whose outlines are kept (four sets of twelve).
pub const MAX_CACHED_GLYPHS: usize = 48;
/// How far a simplified outline may stray from the traced one, in image pixels.
const TOLERANCE_PX: f32 = 0.08;
/// Loops enclosing less than this many square pixels are specks and dropped.
const MIN_AREA_PX: f32 = 0.5;

/// The file stem of `piece`'s image: `wK`, `bP`, …
fn stem(color: Color, kind: PieceKind) -> String {
    let c = match color {
        Color::White => 'w',
        Color::Black => 'b',
    };
    format!("{c}{}", kind.letter())
}

/// The twelve file stems of a piece-image folder, in [`PieceSet`] order.
pub const PIECE_STEMS: [&str; 12] = ["wK", "wQ", "wR", "wB", "wN", "wP", "bK", "bQ", "bR", "bB", "bN", "bP"];

/// Every piece image in `dir` (PNG or SVG named for a piece, either case), by name.
pub fn piece_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<PathBuf> = entries
        .flatten()
        .take(1_000)
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter(|p| p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("png") || e.eq_ignore_ascii_case("svg")))
        .filter(|p| p.file_stem().and_then(|s| s.to_str()).is_some_and(|s| PIECE_STEMS.iter().any(|k| k.eq_ignore_ascii_case(s))))
        .collect();
    out.sort();
    out
}

/// Whether `dir` looks like a piece-image folder: it holds a white king image.
pub fn is_piece_folder(dir: &Path) -> bool {
    dir.is_dir() && images_in(dir).contains_key("wk")
}

/// The pieces a piece-image folder has no image for (`wK`, `bQ`, …), in [`PieceSet`] order.
pub fn missing_pieces(dir: &Path) -> Vec<String> {
    let files = images_in(dir);
    let mut missing = Vec::new();
    for color in [Color::White, Color::Black] {
        for kind in PieceKind::ALL {
            let name = stem(color, kind);
            if !files.contains_key(&name.to_ascii_lowercase()) {
                missing.push(name);
            }
        }
    }
    missing
}

/// Whether `path` is an SVG file (else a PNG), by its extension.
fn is_svg(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("svg"))
}

/// PNG and SVG files in `dir` by lower-case stem; an SVG wins over a PNG of the same stem.
fn images_in(dir: &Path) -> HashMap<String, PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return HashMap::new() };
    let mut out: HashMap<String, PathBuf> = HashMap::new();
    for p in entries.flatten().take(1_000).map(|e| e.path()) {
        let image = p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("png") || e.eq_ignore_ascii_case("svg"));
        let Some(stem) = p.file_stem().and_then(|s| s.to_str()).map(str::to_ascii_lowercase) else { continue };
        if image && !out.get(&stem).is_some_and(|kept| is_svg(kept)) {
            out.insert(stem, p);
        }
    }
    out
}

/// Make a piece set from the twelve images in `dir`.
pub fn from_image_dir(dir: &Path) -> Result<PieceSet, PieceError> {
    let files = images_in(dir);
    let mut glyphs: [PieceGlyph; 12] = Default::default();
    for (i, slot) in glyphs.iter_mut().enumerate() {
        let color = if i < 6 { Color::White } else { Color::Black };
        let kind = PieceKind::ALL.get(i % 6).copied().unwrap_or(PieceKind::Pawn);
        let name = stem(color, kind);
        let path = files
            .get(&name.to_ascii_lowercase())
            .ok_or_else(|| PieceError::MissingImage { dir: dir.to_path_buf(), name: format!("{name}.png or {name}.svg") })?;
        let size = std::fs::metadata(path).map_err(|source| PieceError::Read { path: path.clone(), source })?.len();
        let size = usize::try_from(size).unwrap_or(usize::MAX);
        if size > MAX_IMAGE_BYTES {
            return Err(PieceError::Image { path: path.clone(), reason: format!("{size} bytes, more than the {MAX_IMAGE_BYTES} accepted") });
        }
        let bytes = std::fs::read(path).map_err(|source| PieceError::Read { path: path.clone(), source })?;
        *slot = glyph_of(&bytes, is_svg(path)).map_err(|reason| PieceError::Image { path: path.clone(), reason })?;
    }
    let name = dir.file_name().and_then(|n| n.to_str()).filter(|n| !n.is_empty()).unwrap_or("images").to_owned();
    Ok(PieceSet { glyphs, name })
}

/// The outlines of the piece image `bytes` (an SVG when `svg`, else a PNG), traced once: the same
/// bytes give the outlines kept from before ([`MAX_CACHED_GLYPHS`], the oldest forgotten first).
/// An image that can't be read is not kept, and says why each time.
fn glyph_of(bytes: &[u8], svg: bool) -> Result<PieceGlyph, String> {
    /// The outlines kept, by the SHA-256 of the image's bytes and its kind, the newest last.
    static KEPT: Mutex<Vec<([u8; 32], bool, PieceGlyph)>> = Mutex::new(Vec::new());
    let digest: [u8; 32] = sha2::Sha256::digest(bytes).into();
    let kept = KEPT.lock().unwrap_or_else(PoisonError::into_inner).iter().find(|(d, s, _)| *d == digest && *s == svg).map(|(_, _, g)| g.clone());
    if let Some(glyph) = kept {
        return Ok(glyph);
    }
    // Traced with the lock let go: another thread may trace other pieces meanwhile.
    let glyph = if svg { super::svg::glyph_from_svg(bytes)? } else { glyph_from_image(&decode_png(bytes)?) };
    #[cfg(test)]
    TRACED.with(|n| n.set(n.get() + 1));
    let mut kept = KEPT.lock().unwrap_or_else(PoisonError::into_inner);
    kept.retain(|(d, s, _)| !(*d == digest && *s == svg));
    if kept.len() >= MAX_CACHED_GLYPHS {
        let over = kept.len().saturating_add(1).saturating_sub(MAX_CACHED_GLYPHS).min(kept.len());
        kept.drain(..over);
    }
    kept.push((digest, svg, glyph.clone()));
    Ok(glyph)
}

#[cfg(test)]
thread_local! {
    /// The images [`glyph_of`] traced on this thread.
    static TRACED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A decoded image: straight (not premultiplied) RGBA, 8 bits per channel, row-major.
pub(crate) struct Rgba {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<[u8; 4]>,
}

fn decode_png(bytes: &[u8]) -> Result<Rgba, String> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let (w, h) = reader.info().size();
    if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
        return Err(format!("{w}×{h} pixels; each side must be 1 to {MAX_SIDE}"));
    }
    let len = reader.output_buffer_size().ok_or("the image is too large")?;
    let mut buf = vec![0; len];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let (width, height) = (info.width as usize, info.height as usize);
    let channels = match info.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => return Err("an indexed image that did not expand".into()),
    };
    let mut pixels = Vec::with_capacity(width * height);
    for y in 0..height {
        let row = buf.get(y * info.line_size..).unwrap_or_default();
        for x in 0..width {
            let px = row.get(x * channels..(x + 1) * channels).unwrap_or_default();
            let at = |i: usize| px.get(i).copied().unwrap_or(0);
            pixels.push(match channels {
                1 => [at(0), at(0), at(0), 255],
                2 => [at(0), at(0), at(0), at(1)],
                3 => [at(0), at(1), at(2), 255],
                _ => [at(0), at(1), at(2), at(3)],
            });
        }
    }
    Ok(Rgba { width, height, pixels })
}

/// The two outlines of one piece image: the silhouette filled black, the light parts white.
pub(crate) fn glyph_from_image(image: &Rgba) -> PieceGlyph {
    let alpha: Vec<f32> = image.pixels.iter().map(|p| f32::from(p[3]) / 255.0).collect();
    // Light where the image is both opaque and bright.
    let light: Vec<f32> = image
        .pixels
        .iter()
        .map(|p| {
            let luma = (0.2126 * f32::from(p[0]) + 0.7152 * f32::from(p[1]) + 0.0722 * f32::from(p[2])) / 255.0;
            luma * f32::from(p[3]) / 255.0
        })
        .collect();
    // A large image is traced at the grid it would be traced at anyway (plan A9-V6b).
    let (width, height) = reduced_size(image.width, image.height);
    let alpha = shrink(&alpha, image.width, image.height, width, height);
    let light = shrink(&light, image.width, image.height, width, height);
    let mut layers = Vec::new();
    for (field, color) in [(&alpha, Rgb::BLACK), (&light, Rgb::WHITE)] {
        let subpaths = trace(field, width, height);
        if !subpaths.is_empty() {
            layers.push(Layer { subpaths, paint: Paint::Fill { color, even_odd: true } });
        }
    }
    PieceGlyph { layers }
}

/// The size an image of `width × height` pixels is traced at: itself up to [`TARGET_GRID`]
/// pixels on its longer side, else reduced to that, its proportions kept (each side at least 1).
fn reduced_size(width: usize, height: usize) -> (usize, usize) {
    let longer = width.max(height);
    if longer <= TARGET_GRID {
        return (width, height);
    }
    let side = |n: usize| (n.saturating_mul(TARGET_GRID).saturating_add(longer / 2) / longer).clamp(1, TARGET_GRID);
    (side(width), side(height))
}

/// `field` (`width × height` values) reduced to `to_w × to_h` by an area average: each value the
/// mean of the part of the field its cell covers, fractions of a value weighed by the part
/// covered, so the field keeps its extent and an edge stays where it was. As it is when the size
/// does not shrink.
fn shrink(field: &[f32], width: usize, height: usize, to_w: usize, to_h: usize) -> Vec<f32> {
    if (to_w >= width && to_h >= height) || to_w == 0 || to_h == 0 {
        return field.to_vec();
    }
    // A whole number of pixels per cell each way (2,048 to 512, say): a plain block average.
    let k = width / to_w;
    if k > 0 && width == k.saturating_mul(to_w) && height == k.saturating_mul(to_h) {
        let mut out = vec![0.0f32; to_w.saturating_mul(to_h)];
        for (y, row) in field.chunks(width).take(height).enumerate() {
            let line = (y / k).saturating_mul(to_w);
            for (x, v) in row.iter().enumerate() {
                if let Some(slot) = out.get_mut(line.saturating_add(x / k)) {
                    *slot += *v;
                }
            }
        }
        let cell = k.saturating_mul(k) as f32;
        out.iter_mut().for_each(|v| *v /= cell);
        return out;
    }
    let rows = shrink_axis(field, width, height, to_w, true);
    shrink_axis(&rows, to_w, height, to_h, false)
}

/// One axis of [`shrink`]: along the rows (`along_x`, `width` values to `to`) or the columns
/// (`height` values to `to`).
fn shrink_axis(field: &[f32], width: usize, height: usize, to: usize, along_x: bool) -> Vec<f32> {
    let (from, lines) = if along_x { (width, height) } else { (height, width) };
    if to == 0 || from == 0 {
        return Vec::new();
    }
    let (out_w, out_h) = if along_x { (to, height) } else { (width, to) };
    let mut out = vec![0.0f32; out_w.saturating_mul(out_h)];
    let step = from as f64 / to as f64;
    let at = |line: usize, i: usize| -> f32 {
        let index = if along_x { line * width + i } else { i * width + line };
        field.get(index).copied().unwrap_or(0.0)
    };
    for line in 0..lines {
        for o in 0..to {
            let (a, b) = (o as f64 * step, (o + 1) as f64 * step);
            let mut sum = 0.0f64;
            let first = a.floor() as usize;
            let last = (b.ceil() as usize).min(from);
            for i in first..last {
                let cover = (b.min((i + 1) as f64) - a.max(i as f64)).max(0.0);
                sum += f64::from(at(line, i)) * cover;
            }
            let index = if along_x { line * out_w + o } else { o * out_w + line };
            if let Some(slot) = out.get_mut(index) {
                *slot = (sum / step) as f32;
            }
        }
    }
    out
}

/// The closed outlines where `field` (`width × height` values in 0..1) crosses one half, in the
/// unit square with y down. Outside the image the field is 0, so every outline closes.
pub(crate) fn trace(field: &[f32], width: usize, height: usize) -> Vec<Subpath> {
    if width == 0 || height == 0 {
        return Vec::new();
    }
    let up = (TARGET_GRID / width.max(height)).clamp(1, MAX_UPSAMPLE);
    // Grid points cover the image plus one empty pixel on each side.
    let (gw, gh) = ((width + 2) * up + 1, (height + 2) * up + 1);
    let pixel = |x: isize, y: isize| -> f32 {
        if x < 0 || y < 0 {
            return 0.0;
        }
        let (x, y) = (x as usize, y as usize);
        if x >= width || y >= height { 0.0 } else { field.get(y * width + x).copied().unwrap_or(0.0) }
    };
    // Bilinear between pixel centres; grid point (gx, gy) sits at pixel coordinate gx/up - 1.
    let sample = |gx: usize, gy: usize| -> f32 {
        let fx = gx as f32 / up as f32 - 1.5;
        let fy = gy as f32 / up as f32 - 1.5;
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - x0, fy - y0);
        let (x0, y0) = (x0 as isize, y0 as isize);
        let top = pixel(x0, y0) * (1.0 - tx) + pixel(x0 + 1, y0) * tx;
        let bottom = pixel(x0, y0 + 1) * (1.0 - tx) + pixel(x0 + 1, y0 + 1) * tx;
        top * (1.0 - ty) + bottom * ty
    };
    let grid: Vec<f32> = (0..gh).flat_map(|gy| (0..gw).map(move |gx| (gx, gy))).map(|(gx, gy)| sample(gx, gy)).collect();
    let loops = march(&grid, gw, gh, 0.5);

    let scale = up as f32;
    let tolerance = TOLERANCE_PX * scale;
    let min_area = MIN_AREA_PX * scale * scale;
    let (w, h) = (width as f32, height as f32);
    loops
        .into_iter()
        .filter(|l| polygon_area(l).abs() >= min_area)
        .map(|l| simplify_closed(&l, tolerance))
        .filter(|l| l.len() >= 3)
        .map(|l| {
            let to_unit = |(x, y): (f32, f32)| ((x / scale - 1.0) / w, (y / scale - 1.0) / h);
            let mut points = l.into_iter().map(to_unit);
            let start = points.next().unwrap_or((0.0, 0.0));
            Subpath { start, segments: points.map(|(x, y)| Segment::Line(x, y)).collect(), closed: true }
        })
        .collect()
}

/// The cell edges: top, right, bottom, left.
#[derive(Clone, Copy)]
enum Edge {
    T,
    R,
    B,
    L,
}

/// Marching squares: closed loops (in grid coordinates) where `grid` crosses `level`.
fn march(grid: &[f32], gw: usize, gh: usize, level: f32) -> Vec<Vec<(f32, f32)>> {
    let at = |x: usize, y: usize| grid.get(y * gw + x).copied().unwrap_or(0.0);
    // Each crossing is keyed by its grid edge: (x, y, 0) from (x, y) to (x+1, y), (x, y, 1) to (x, y+1).
    let edge_key = |x: usize, y: usize, e: Edge| -> (usize, usize, u8) {
        match e {
            Edge::T => (x, y, 0),
            Edge::B => (x, y + 1, 0),
            Edge::L => (x, y, 1),
            Edge::R => (x + 1, y, 1),
        }
    };
    let crossing = |(x, y, vertical): (usize, usize, u8)| -> (f32, f32) {
        let (a, b) = if vertical == 1 { (at(x, y), at(x, y + 1)) } else { (at(x, y), at(x + 1, y)) };
        let t = if (b - a).abs() > f32::EPSILON { ((level - a) / (b - a)).clamp(0.0, 1.0) } else { 0.5 };
        if vertical == 1 { (x as f32, y as f32 + t) } else { (x as f32 + t, y as f32) }
    };

    let mut segments: Vec<[(usize, usize, u8); 2]> = Vec::new();
    for y in 0..gh.saturating_sub(1) {
        for x in 0..gw.saturating_sub(1) {
            let (a, b, c, d) = (at(x, y), at(x + 1, y), at(x + 1, y + 1), at(x, y + 1));
            let bit = |v: f32, n: u8| if v >= level { n } else { 0 };
            let case = bit(a, 8) | bit(b, 4) | bit(c, 2) | bit(d, 1);
            let centre_in = (a + b + c + d) / 4.0 >= level;
            let pairs: &[(Edge, Edge)] = match case {
                1 | 14 => &[(Edge::L, Edge::B)],
                2 | 13 => &[(Edge::B, Edge::R)],
                3 | 12 => &[(Edge::L, Edge::R)],
                4 | 11 => &[(Edge::T, Edge::R)],
                6 | 9 => &[(Edge::T, Edge::B)],
                7 | 8 => &[(Edge::T, Edge::L)],
                // Saddles: the centre says which diagonal is connected.
                5 if centre_in => &[(Edge::T, Edge::L), (Edge::B, Edge::R)],
                5 => &[(Edge::T, Edge::R), (Edge::L, Edge::B)],
                10 if centre_in => &[(Edge::T, Edge::R), (Edge::L, Edge::B)],
                10 => &[(Edge::T, Edge::L), (Edge::B, Edge::R)],
                _ => &[],
            };
            for &(p, q) in pairs {
                segments.push([edge_key(x, y, p), edge_key(x, y, q)]);
            }
        }
    }

    // Every crossing touches exactly two segments, one in each cell beside its edge.
    let mut touching: HashMap<(usize, usize, u8), Vec<usize>> = HashMap::new();
    for (i, s) in segments.iter().enumerate() {
        for k in s {
            touching.entry(*k).or_default().push(i);
        }
    }
    let mut used = vec![false; segments.len()];
    let mut loops = Vec::new();
    for first in 0..segments.len() {
        if used.get(first).copied().unwrap_or(true) {
            continue;
        }
        let Some(&[start, mut key]) = segments.get(first) else { continue };
        if let Some(u) = used.get_mut(first) {
            *u = true;
        }
        let mut points = vec![crossing(start), crossing(key)];
        let mut current = first;
        while key != start {
            let next = touching.get(&key).and_then(|v| v.iter().copied().find(|&s| s != current && !used.get(s).copied().unwrap_or(true)));
            let Some(next) = next else { break };
            if let Some(u) = used.get_mut(next) {
                *u = true;
            }
            let Some(&[p, q]) = segments.get(next) else { break };
            key = if p == key { q } else { p };
            current = next;
            if key != start {
                points.push(crossing(key));
            }
        }
        if points.len() >= 3 {
            loops.push(points);
        }
    }
    loops
}

/// Signed area of a closed polygon.
fn polygon_area(points: &[(f32, f32)]) -> f32 {
    let n = points.len();
    (0..n)
        .map(|i| {
            let (x0, y0) = points.get(i).copied().unwrap_or_default();
            let (x1, y1) = points.get((i + 1) % n).copied().unwrap_or_default();
            x0 * y1 - x1 * y0
        })
        .sum::<f32>()
        / 2.0
}

/// Douglas–Peucker on a closed loop: split it at the point farthest from the first, simplify
/// both halves.
fn simplify_closed(points: &[(f32, f32)], tolerance: f32) -> Vec<(f32, f32)> {
    let Some(&first) = points.first() else { return Vec::new() };
    let dist2 = |a: (f32, f32), b: (f32, f32)| (a.0 - b.0).powi(2) + (a.1 - b.1).powi(2);
    let far = (0..points.len()).max_by(|&i, &j| {
        let di = dist2(first, points.get(i).copied().unwrap_or(first));
        let dj = dist2(first, points.get(j).copied().unwrap_or(first));
        di.total_cmp(&dj)
    });
    let Some(far) = far.filter(|&f| f > 0) else { return points.to_vec() };
    let mut closed = points.to_vec();
    closed.push(first);
    let mut out = simplify_open(closed.get(..=far).unwrap_or_default(), tolerance);
    out.pop();
    out.extend(simplify_open(closed.get(far..).unwrap_or_default(), tolerance));
    out.pop();
    out
}

/// Douglas–Peucker on an open polyline; keeps both ends ([`crate::imageops::simplify_open`],
/// plan A9-V9: this one recursed without a bound).
fn simplify_open(points: &[(f32, f32)], tolerance: f32) -> Vec<(f32, f32)> {
    crate::imageops::simplify_open(points, tolerance)
}

/// The recursive Douglas–Peucker this file had before plan A9-V9: the reference the shared one is
/// held to.
#[cfg(test)]
fn simplify_open_recursive(points: &[(f32, f32)], tolerance: f32) -> Vec<(f32, f32)> {
    let (Some(&a), Some(&b)) = (points.first(), points.last()) else { return Vec::new() };
    if points.len() < 3 {
        return points.to_vec();
    }
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = (dx * dx + dy * dy).sqrt();
    let distance = |p: (f32, f32)| {
        if len > f32::EPSILON { ((p.0 - a.0) * dy - (p.1 - a.1) * dx).abs() / len } else { ((p.0 - a.0).powi(2) + (p.1 - a.1).powi(2)).sqrt() }
    };
    let (mut worst, mut index) = (0.0f32, 0);
    for (i, &p) in points.iter().enumerate().take(points.len() - 1).skip(1) {
        let d = distance(p);
        if d > worst {
            (worst, index) = (d, i);
        }
    }
    if worst <= tolerance {
        return vec![a, b];
    }
    let mut left = simplify_open_recursive(points.get(..=index).unwrap_or_default(), tolerance);
    left.pop();
    left.extend(simplify_open_recursive(points.get(index..).unwrap_or_default(), tolerance));
    left
}

#[cfg(test)]
mod tests {
    use super::*;

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(256))]

        /// Plan A9-V9 (A3-11): the one Douglas–Peucker (iterative, in `imageops`) keeps the points
        /// the piece tracer's recursive one kept, on polylines of grid points (ties among the farthest come up) and at any tolerance; the
        /// traces of the tests below are unchanged with it.
        #[test]
        fn the_shared_simplification_keeps_what_the_recursive_one_kept(
            grid in proptest::collection::vec((-8i32..8, -8i32..8), 0..80),
            tolerance in 0.0f32..6.0,
        ) {
            // On a coarse grid, so points as far from a chord as each other (ties) come up often.
            let points: Vec<(f32, f32)> = grid.iter().map(|&(x, y)| (x as f32, y as f32)).collect();
            proptest::prop_assert_eq!(simplify_open(&points, tolerance), simplify_open_recursive(&points, tolerance));
        }
    }

    /// Plan A9-V9: a staircase of 10,000 points, each the farthest from the chord of what is left,
    /// is simplified on a small stack (the recursive one went as deep as the steps).
    #[test]
    fn a_long_staircase_is_simplified_on_a_small_stack() {
        let points: Vec<(f32, f32)> = (0..10_000).map(|i| (i as f32, ((i / 2) % 2) as f32 * 3.0)).collect();
        let kept = std::thread::Builder::new().stack_size(64 * 1024).spawn(move || simplify_open(&points, 0.5).len()).unwrap().join().unwrap();
        assert!(kept > 2);
    }

    /// A black disc filling 80 % of a `size` px image, anti-aliased by its cover of each pixel
    /// (4 × 4 samples).
    fn disc(size: usize) -> Rgba {
        let (c, r) = (size as f32 / 2.0, size as f32 * 0.4);
        let pixels = (0..size * size)
            .map(|i| {
                let (x, y) = ((i % size) as f32, (i / size) as f32);
                let inside = (0..16).filter(|k| (x + (k % 4) as f32 / 4.0 + 0.125 - c).hypot(y + (k / 4) as f32 / 4.0 + 0.125 - c) <= r).count();
                [0, 0, 0, (inside * 255 / 16) as u8]
            })
            .collect();
        Rgba { width: size, height: size, pixels }
    }

    /// The silhouette's points and segments of `glyph`, in the unit square.
    fn silhouette(glyph: &PieceGlyph) -> Vec<Vec<(f32, f32)>> {
        let layer = glyph.layers.first().expect("a silhouette");
        layer
            .subpaths
            .iter()
            .map(|p| {
                let mut points = vec![p.start];
                points.extend(p.segments.iter().filter_map(|s| if let Segment::Line(x, y) = s { Some((*x, *y)) } else { None }));
                points
            })
            .collect()
    }

    /// The farthest a point of `a` is from the closed polylines of `b`, both ways (Hausdorff).
    fn hausdorff(a: &[Vec<(f32, f32)>], b: &[Vec<(f32, f32)>]) -> f32 {
        fn to_segment(p: (f32, f32), s: (f32, f32), e: (f32, f32)) -> f32 {
            let (dx, dy) = (e.0 - s.0, e.1 - s.1);
            let len = dx * dx + dy * dy;
            let t = if len > 0.0 { (((p.0 - s.0) * dx + (p.1 - s.1) * dy) / len).clamp(0.0, 1.0) } else { 0.0 };
            (p.0 - s.0 - t * dx).hypot(p.1 - s.1 - t * dy)
        }
        fn one_way(a: &[Vec<(f32, f32)>], b: &[Vec<(f32, f32)>]) -> f32 {
            let segments: Vec<((f32, f32), (f32, f32))> =
                b.iter().flat_map(|l| l.iter().zip(l.iter().cycle().skip(1)).map(|(s, e)| (*s, *e))).collect();
            a.iter().flatten().map(|p| segments.iter().map(|(s, e)| to_segment(*p, *s, *e)).fold(f32::MAX, f32::min)).fold(0.0, f32::max)
        }
        one_way(a, b).max(one_way(b, a))
    }

    /// Plan A9-V6b (A3-07): an image larger than the grid is traced reduced to it, an area average:
    /// a 2,048 px disc gives the outline the 512 px one gives (Hausdorff under 0.5 % of the side),
    /// with about as many points, not the four times as many a 2,048 px grid draws.
    #[test]
    fn a_large_image_is_traced_reduced_to_the_same_outline() {
        let (big, small) = (silhouette(&glyph_from_image(&disc(2048))), silhouette(&glyph_from_image(&disc(512))));
        assert_eq!((big.len(), small.len()), (1, 1), "one loop each");
        let distance = hausdorff(&big, &small);
        assert!(distance < 0.005, "Hausdorff {distance} of the side");
        let (b, s) = (big.iter().map(Vec::len).sum::<usize>(), small.iter().map(Vec::len).sum::<usize>());
        assert!(b * 4 <= s * 5, "traced at the reduced grid: {b} points against {s}");
    }

    /// The area average keeps a field's extent and its mean at any ratio, whole or not.
    #[test]
    fn the_area_average_keeps_the_mean_and_the_edges() {
        for (w, h) in [(2048, 1024), (1000, 700), (513, 2048), (600, 600)] {
            let field: Vec<f32> = (0..w * h).map(|i| if (i % w) < w / 2 { 1.0 } else { 0.0 }).collect();
            let (tw, th) = reduced_size(w, h);
            assert!(tw.max(th) == TARGET_GRID && tw >= 1 && th >= 1, "{w}x{h} to {tw}x{th}");
            let out = shrink(&field, w, h, tw, th);
            assert_eq!(out.len(), tw * th);
            let mean = |f: &[f32]| f.iter().map(|v| f64::from(*v)).sum::<f64>() / f.len() as f64;
            assert!((mean(&out) - mean(&field)).abs() < 1e-3, "{w}x{h}: {} against {}", mean(&out), mean(&field));
            // The left half stays full and the right half empty, past the cell the edge falls in.
            let row = &out[..tw];
            assert!(row[..tw / 2 - 1].iter().all(|v| (*v - 1.0).abs() < 1e-4) && row[tw / 2 + 1..].iter().all(|v| v.abs() < 1e-4), "{w}x{h}");
        }
        assert_eq!(reduced_size(512, 300), (512, 300), "not larger: as it is");
        assert_eq!(reduced_size(2048, 1), (512, 1), "a side never goes to 0");
    }

    /// Plan A9-V6b: the same image's bytes are traced once; other bytes, or the same bytes read as
    /// the other kind, are not taken for it.
    #[test]
    fn the_same_image_is_traced_once() {
        let mut png = Vec::new();
        {
            let image = disc(64);
            let mut e = png::Encoder::new(&mut png, 64, 64);
            e.set_color(png::ColorType::Rgba);
            e.set_depth(png::BitDepth::Eight);
            let rgba: Vec<u8> = image.pixels.iter().flatten().copied().collect();
            e.write_header().unwrap().write_image_data(&rgba).unwrap();
        }
        let traced = || TRACED.with(std::cell::Cell::get);
        let before = traced();
        let first = glyph_of(&png, false).unwrap();
        assert_eq!(glyph_of(&png, false).unwrap(), first);
        assert_eq!(traced() - before, 1, "traced once");
        assert!(glyph_of(&png, true).is_err(), "a PNG read as an SVG is no glyph, never the PNG's");
        assert!(glyph_of(b"not an image", false).is_err());
    }

    /// Plan A9-V6b (A3-07): tracing a 2,048 px piece takes under an eighth of the time it took at
    /// its full size, in release.
    /// `cargo test --release -p pdfcraft-chessvision --lib perf_trace_a_large_piece -- --ignored`.
    #[test]
    #[ignore = "a timing, in release"]
    fn perf_trace_a_large_piece() {
        let image = disc(2048);
        let alpha: Vec<f32> = image.pixels.iter().map(|p| f32::from(p[3]) / 255.0).collect();
        let best = |f: &dyn Fn() -> usize| {
            (0..3)
                .map(|_| {
                    let t = std::time::Instant::now();
                    assert_eq!(f(), 1, "one loop");
                    t.elapsed().as_secs_f64() * 1000.0
                })
                .fold(f64::MAX, f64::min)
        };
        let full = best(&|| trace(&alpha, 2048, 2048).len());
        let reduced = best(&|| {
            let (w, h) = reduced_size(2048, 2048);
            trace(&shrink(&alpha, 2048, 2048, w, h), w, h).len()
        });
        println!("PERF trace_large_piece {reduced:.0}");
        println!("trace of a 2,048 px disc: {reduced:.1} ms reduced, {full:.1} ms at its full size");
        assert!(reduced * 8.0 < full, "{reduced:.1} ms against {full:.1} ms");
    }

    fn square_image(size: usize, from: usize, to: usize, rgba: [u8; 4]) -> Rgba {
        let pixels = (0..size * size)
            .map(|i| {
                let (x, y) = (i % size, i / size);
                if (from..to).contains(&x) && (from..to).contains(&y) { rgba } else { [0, 0, 0, 0] }
            })
            .collect();
        Rgba { width: size, height: size, pixels }
    }

    #[test]
    fn a_square_traces_to_one_loop_where_it_is() {
        let image = square_image(20, 5, 15, [0, 0, 0, 255]);
        let glyph = glyph_from_image(&image);
        // Black and opaque: a silhouette, no light parts.
        assert_eq!(glyph.layers.len(), 1);
        let layer = &glyph.layers[0];
        assert_eq!(layer.subpaths.len(), 1);
        let b = pdfcraft_chess::draw::bounds(&layer.subpaths).unwrap();
        for (got, want) in b.iter().zip([0.25, 0.25, 0.75, 0.75]) {
            assert!((got - want).abs() < 0.03, "{b:?}");
        }
        // Straight sides simplify to a few points.
        assert!(layer.subpaths[0].segments.len() < 12, "{}", layer.subpaths[0].segments.len());
    }

    #[test]
    fn a_white_piece_has_a_silhouette_and_light_parts() {
        let image = square_image(20, 4, 16, [255, 255, 255, 255]);
        let glyph = glyph_from_image(&image);
        assert_eq!(glyph.layers.len(), 2);
        assert_eq!(glyph.layers[0].paint, Paint::Fill { color: Rgb::BLACK, even_odd: true });
        assert_eq!(glyph.layers[1].paint, Paint::Fill { color: Rgb::WHITE, even_odd: true });
    }

    #[test]
    fn a_ring_gives_an_outer_and_an_inner_loop() {
        let mut image = square_image(30, 5, 25, [0, 0, 0, 255]);
        for y in 10..20 {
            for x in 10..20 {
                image.pixels[y * 30 + x] = [0, 0, 0, 0];
            }
        }
        let glyph = glyph_from_image(&image);
        assert_eq!(glyph.layers[0].subpaths.len(), 2);
    }

    #[test]
    fn specks_and_empty_images_trace_to_nothing() {
        assert!(trace(&[0.0; 16], 4, 4).is_empty());
        let mut speck = vec![0.0; 100];
        speck[55] = 0.6;
        assert!(trace(&speck, 10, 10).is_empty());
        assert!(trace(&[], 0, 0).is_empty());
    }

    #[test]
    fn a_full_image_closes_along_its_border() {
        let loops = trace(&[1.0; 64], 8, 8);
        assert_eq!(loops.len(), 1);
        let b = pdfcraft_chess::draw::bounds(&loops).unwrap();
        assert!(b[0] < 0.05 && b[1] < 0.05 && b[2] > 0.95 && b[3] > 0.95, "{b:?}");
    }

    fn write_png(path: &Path, size: u32, rgba: &[u8]) {
        let file = std::fs::File::create(path).unwrap();
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), size, size);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(rgba).unwrap();
    }

    #[test]
    fn a_folder_of_twelve_images_is_a_piece_set() {
        let dir = std::env::temp_dir().join(format!("pdfcraft-piece-images-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let image = square_image(16, 3, 13, [255, 255, 255, 255]);
        let bytes: Vec<u8> = image.pixels.iter().flatten().copied().collect();
        assert!(!is_piece_folder(&dir));
        for color in ['w', 'b'] {
            for kind in "KQRBNP".chars() {
                // Names in any case.
                let name = if kind == 'N' { format!("{color}n.PNG") } else { format!("{color}{kind}.png") };
                write_png(&dir.join(name), 16, &bytes);
            }
        }
        assert!(is_piece_folder(&dir));
        let set = from_image_dir(&dir).unwrap();
        assert!(set.glyphs.iter().all(|g| g.layers.len() == 2));
        std::fs::remove_file(dir.join("bQ.png")).unwrap();
        let err = from_image_dir(&dir).unwrap_err();
        assert!(err.to_string().contains("bQ.png"), "{err}");
        std::fs::write(dir.join("bQ.png"), b"not a png").unwrap();
        assert!(matches!(from_image_dir(&dir), Err(PieceError::Image { .. })));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_folder_can_mix_svg_and_png_and_svg_wins() {
        let dir = std::env::temp_dir().join(format!("pdfcraft-piece-svg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let image = square_image(16, 3, 13, [255, 255, 255, 255]);
        let bytes: Vec<u8> = image.pixels.iter().flatten().copied().collect();
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="45" height="45"><path d="M9 36H36L22.5 9Z" fill="#fff" stroke="#000" stroke-width="1.5"/></svg>"##;
        for color in ['w', 'b'] {
            for kind in "KQRBNP".chars() {
                write_png(&dir.join(format!("{color}{kind}.png")), 16, &bytes);
            }
        }
        // The white king also as SVG, in upper case: the SVG is drawn, not the PNG.
        std::fs::write(dir.join("WK.SVG"), svg).unwrap();
        // The black queen only as SVG.
        std::fs::remove_file(dir.join("bQ.png")).unwrap();
        std::fs::write(dir.join("bQ.svg"), svg).unwrap();
        let set = from_image_dir(&dir).unwrap();
        let king = &set.glyphs[5];
        assert!(matches!(king.layers.last().map(|l| l.paint), Some(Paint::Stroke { .. })), "{:?}", king.layers);
        assert!(matches!(set.glyphs[10].layers.last().map(|l| l.paint), Some(Paint::Stroke { .. })));
        // A PNG piece is traced: fills only.
        assert!(set.glyphs[0].layers.iter().all(|l| matches!(l.paint, Paint::Fill { .. })));
        // A broken SVG is an error that names the file.
        std::fs::write(dir.join("bQ.svg"), b"<svg").unwrap();
        let err = from_image_dir(&dir).unwrap_err();
        assert!(matches!(&err, PieceError::Image { path, .. } if path.ends_with("bQ.svg")), "{err}");
        std::fs::remove_file(dir.join("bQ.svg")).unwrap();
        assert!(from_image_dir(&dir).unwrap_err().to_string().contains("bQ.png or bQ.svg"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Builds a piece set from the user's real piece images when `PDFCRAFT_CHESS_TEST_PIECE_IMAGES`
    /// names their folder. Skipped otherwise: no piece artwork is committed (decision D4).
    #[test]
    fn a_real_piece_folder_gives_twelve_pieces() {
        let Some(dir) = std::env::var_os("PDFCRAFT_CHESS_TEST_PIECE_IMAGES") else { return };
        let set = from_image_dir(Path::new(&dir)).unwrap();
        for (i, g) in set.glyphs.iter().enumerate() {
            assert!(!g.layers.is_empty(), "glyph {i}");
            let b = pdfcraft_chess::draw::bounds(&g.layers[0].subpaths).unwrap();
            assert!(b[0] >= 0.0 && b[1] >= 0.0 && b[2] <= 1.0 && b[3] <= 1.0, "glyph {i}: {b:?}");
        }
        // For looking at: write a drawn position when asked.
        if let Some(out) = std::env::var_os("PDFCRAFT_CHESS_TEST_SVG_OUT") {
            let placement = pdfcraft_chess::fen::parse_placement("r1bqk2r/pp3ppp/2n2n2/2bpp3/4P3/2NB1N2/PPPP1PPP/R1BQK2R").unwrap();
            let style = pdfcraft_chess::draw::BoardStyle { coordinates: true, ..Default::default() };
            let drawing = pdfcraft_chess::draw::board(&placement, &set, &style);
            std::fs::write(&out, pdfcraft_chess::svg::to_svg(&drawing, "test")).unwrap();
        }
    }

    proptest::proptest! {
        /// Plan A9-Q3b (A7-08): marching squares on any grid, at any level, never panics; every
        /// point lies on the grid, and on a grid whose border is below the level (the traced
        /// image is padded so) every loop closes: its last point and its first share a cell.
        #[test]
        fn marching_squares_trace_closed_loops_on_the_grid(
            gw in 0usize..24,
            gh in 0usize..24,
            values in proptest::collection::vec(proptest::prop_oneof![0.0f32..1.0, proptest::prelude::Just(f32::NAN), proptest::prelude::Just(f32::INFINITY)], 0..600),
            level in proptest::prop_oneof![0.0f32..1.0, proptest::prelude::Just(f32::NAN)],
            short in proptest::prelude::any::<bool>(),
        ) {
            // A grid shorter than gw × gh reads the missing values as 0.
            let len = if short { values.len().min(gw * gh) } else { gw * gh };
            let grid: Vec<f32> = (0..len).map(|i| values.get(i).copied().unwrap_or(0.0)).collect();
            let _ = march(&grid, gw, gh, level);
            // Finite values, a zero border: closed loops on the grid.
            if level.is_finite() && level > 0.0 && gw >= 3 && gh >= 3 {
                let padded: Vec<f32> = (0..gw * gh)
                    .map(|i| {
                        let (x, y) = (i % gw, i / gw);
                        let v = values.get(i).copied().filter(|v| v.is_finite()).unwrap_or(0.0);
                        if x == 0 || y == 0 || x + 1 == gw || y + 1 == gh { 0.0 } else { v }
                    })
                    .collect();
                for points in march(&padded, gw, gh, level) {
                    proptest::prop_assert!(points.len() >= 3);
                    for (x, y) in &points {
                        proptest::prop_assert!(x.is_finite() && y.is_finite() && *x >= 0.0 && *y >= 0.0, "{:?}", points);
                        proptest::prop_assert!(*x <= (gw - 1) as f32 && *y <= (gh - 1) as f32, "{:?}", points);
                    }
                    let (first, last) = (points[0], points[points.len() - 1]);
                    proptest::prop_assert!((first.0 - last.0).abs() <= 1.0 && (first.1 - last.1).abs() <= 1.0, "an open loop: {:?}", points);
                }
            }
        }
    }
}
