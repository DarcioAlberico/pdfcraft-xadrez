//! Board textures made in code (plan HC5, decision D14): no picture from anyone else, each one a
//! deterministic function of the pixel (value noise with fixed seeds) that draws the light and
//! the dark squares at any size. Wood, marble and paper are drawn here as pixels; a book's
//! hatched dark squares are vector paths ([`crate::draw::BoardStyle::hatched`]), so a diagram
//! redrawn on a page or exported keeps them sharp.

use crate::draw::Rgb;

/// A theme's texture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Texture {
    /// Grain along the light squares, across the dark ones, as parquet.
    Wood,
    /// Veins by turbulence.
    Marble,
    /// A fine grain, as a book's paper.
    Paper,
    /// The dark squares hatched in diagonal lines, as printed diagrams: drawn as paths.
    Hatched,
}

/// Largest side drawn, in pixels.
pub const MAX_PX: u32 = 4096;

/// A fixed hash of a lattice point to `0..=1`.
fn hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = x.cast_unsigned().wrapping_mul(0x8DA6_B343) ^ y.cast_unsigned().wrapping_mul(0xD816_3841) ^ seed.wrapping_mul(0xCB1A_B31F);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5BD1_E995);
    h ^= h >> 15;
    f32::from(u16::try_from(h & 0xFFFF).unwrap_or(0)) / 65535.0
}

/// Smooth value noise in `0..=1`.
fn noise(x: f32, y: f32, seed: u32) -> f32 {
    // Bounded first: the lattice coordinates fit an i32 and the casts cannot wrap.
    let (x, y) = (x.clamp(-1.0e6, 1.0e6), y.clamp(-1.0e6, 1.0e6));
    let (fx, fy) = (x.floor(), y.floor());
    let (ix, iy) = (fx as i32, fy as i32);
    let smooth = |t: f32| t * t * (3.0 - 2.0 * t);
    let (tx, ty) = (smooth(x - fx), smooth(y - fy));
    let top = hash(ix, iy, seed) + (hash(ix + 1, iy, seed) - hash(ix, iy, seed)) * tx;
    let bottom = hash(ix, iy + 1, seed) + (hash(ix + 1, iy + 1, seed) - hash(ix, iy + 1, seed)) * tx;
    top + (bottom - top) * ty
}

/// Four octaves of [`noise`], in `0..=1`.
fn fbm(x: f32, y: f32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut freq, mut total) = (0.0, 1.0, 1.0, 0.0);
    for octave in 0..4u32 {
        sum += amp * noise(x * freq, y * freq, seed.wrapping_add(octave.wrapping_mul(101)));
        total += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    sum / total
}

/// How much lighter or darker than its square's colour the point `(u, v)` (0..1 across the
/// square) is drawn, for a square of `seed`; `grain_across` turns wood's grain.
fn shade(texture: Texture, u: f32, v: f32, seed: u32, grain_across: bool) -> f32 {
    match texture {
        Texture::Wood => {
            let (a, b) = if grain_across { (v, u) } else { (u, v) };
            let t = b * 5.0 + fbm(a * 1.5, b * 5.0, seed) * 2.2;
            let rings = 0.5 + 0.5 * (t * std::f32::consts::TAU).sin();
            let fibre = noise(a * 3.0, b * 48.0, seed ^ 0x55);
            1.0 + 0.11 * (rings - 0.5) + 0.07 * (fibre - 0.5)
        }
        Texture::Marble => {
            let t = u * 2.5 + v * 1.7 + fbm(u * 3.5, v * 3.5, seed) * 4.0;
            let vein = (1.0 - (t * std::f32::consts::PI).sin().abs()).powi(7);
            1.0 - 0.28 * vein + 0.05 * (fbm(u * 9.0, v * 9.0, seed ^ 0x33) - 0.5)
        }
        Texture::Paper => 1.0 + 0.05 * (noise(u * 70.0, v * 70.0, seed) - 0.5) + 0.03 * (fbm(u * 6.0, v * 6.0, seed ^ 0x77) - 0.5),
        Texture::Hatched => 1.0,
    }
}

/// The 64 squares of a board `w` × `h` pixels in `texture` over `light` and `dark`, a8's light
/// square at the top left: opaque RGBA, row by row. Sides are capped at [`MAX_PX`]. Hatched
/// squares are paths, not pixels: their squares come back plain here.
pub fn board_rgba(texture: Texture, light: Rgb, dark: Rgb, w: u32, h: u32) -> Vec<u8> {
    let (w, h) = (w.clamp(1, MAX_PX), h.clamp(1, MAX_PX));
    // Capped above: at most 4096 × 4096 × 4 bytes.
    let mut out = Vec::with_capacity((w as usize) * (h as usize) * 4);
    let (cw, ch) = (w as f32 / 8.0, h as f32 / 8.0);
    for y in 0..h {
        let row = ((y as f32 / ch) as u32).min(7);
        let v = (y as f32 - row as f32 * ch) / ch;
        for x in 0..w {
            let col = ((x as f32 / cw) as u32).min(7);
            let u = (x as f32 - col as f32 * cw) / cw;
            let is_light = (col + row).is_multiple_of(2);
            let base = if is_light { light } else { dark };
            let seed = (row * 8 + col).wrapping_mul(7919).wrapping_add(if is_light { 17 } else { 4241 });
            let k = shade(texture, u, v, seed, !is_light);
            let c = |channel: u8| (f32::from(channel) * k).round().clamp(0.0, 255.0) as u8;
            out.extend_from_slice(&[c(base.0), c(base.1), c(base.2), 255]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The mean colour of square (col, row) in a `side`-pixel board.
    fn mean(rgba: &[u8], side: u32, col: u32, row: u32) -> [f32; 3] {
        let cell = side / 8;
        let mut sum = [0.0f32; 3];
        let mut n = 0.0;
        for y in row * cell..(row + 1) * cell {
            for x in col * cell..(col + 1) * cell {
                let i = ((y * side + x) * 4) as usize;
                for (s, c) in sum.iter_mut().zip(&rgba[i..i + 3]) {
                    *s += f32::from(*c);
                }
                n += 1.0;
            }
        }
        sum.map(|s| s / n)
    }

    /// The same texture twice is the same picture, each square keeps its colour on average
    /// (within 8%, the declared tolerance), and the grain is really there.
    #[test]
    fn textures_are_deterministic_and_keep_their_squares_colours() {
        let (light, dark) = (Rgb(229, 196, 146), Rgb(164, 110, 66));
        for texture in [Texture::Wood, Texture::Marble, Texture::Paper] {
            let a = board_rgba(texture, light, dark, 128, 128);
            assert_eq!(a, board_rgba(texture, light, dark, 128, 128), "{texture:?} is deterministic");
            assert_eq!(a.len(), 128 * 128 * 4);
            for (col, row) in [(0, 0), (1, 0), (3, 4), (7, 7)] {
                let base = if (col + row) % 2 == 0 { light } else { dark };
                let m = mean(&a, 128, col, row);
                for (got, want) in m.iter().zip([base.0, base.1, base.2]) {
                    assert!((got - f32::from(want)).abs() <= f32::from(want) * 0.08 + 2.0, "{texture:?} ({col},{row}): {m:?} vs {base:?}");
                }
            }
            let distinct: std::collections::HashSet<&[u8]> = a.chunks(4).collect();
            assert!(distinct.len() > 40, "{texture:?} has a grain: {} colours", distinct.len());
        }
        // Hatched squares are paths: plain pixels here.
        let plain = board_rgba(Texture::Hatched, light, dark, 16, 16);
        assert_eq!(plain.get(..4), Some(&[229, 196, 146, 255][..]));
    }

    /// The golden: a few pixels of a fixed board, within 2 levels (float functions may round
    /// differently on another platform).
    #[test]
    fn a_wood_board_matches_its_golden_pixels() {
        let a = board_rgba(Texture::Wood, Rgb(229, 196, 146), Rgb(164, 110, 66), 64, 64);
        let at = |x: usize, y: usize| [a[(y * 64 + x) * 4], a[(y * 64 + x) * 4 + 1], a[(y * 64 + x) * 4 + 2]];
        let points = [(3, 3), (12, 5), (40, 33), (60, 61)];
        for ((x, y), want) in points.into_iter().zip(GOLDEN) {
            let got = at(x, y);
            assert!(got.iter().zip(want).all(|(g, w)| g.abs_diff(w) <= 2), "({x},{y}): {got:?} vs {want:?}");
        }
    }

    const GOLDEN: [[u8; 3]; 4] = [[234, 201, 149], [158, 106, 64], [163, 109, 65], [221, 189, 141]];

    #[test]
    fn odd_and_huge_sizes_are_bounded() {
        assert_eq!(board_rgba(Texture::Paper, Rgb(1, 2, 3), Rgb(4, 5, 6), 0, 0).len(), 4);
        assert_eq!(board_rgba(Texture::Marble, Rgb(1, 2, 3), Rgb(4, 5, 6), 9, 3).len(), 9 * 3 * 4);
        let started = std::time::Instant::now();
        let big = board_rgba(Texture::Wood, Rgb(229, 196, 146), Rgb(164, 110, 66), 800, 800);
        eprintln!("an 800-pixel wood board took {:?}", started.elapsed());
        assert_eq!(big.len(), 800 * 800 * 4);
    }
}
