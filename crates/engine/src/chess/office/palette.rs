//! Diagrams as paletted PNG (plan A9-X1b, finding R8-09). A board has few colours: its squares,
//! the pieces' fills and outlines, the frame, and the levels of antialiasing between them. One
//! byte a pixel, an index into at most 256 colours, holds it where RGBA took four, and it
//! compresses better: a Word book's diagrams were 92 MB of RGBA pictures.
//!
//! The palette keeps the colours that cover most of the picture exactly (a square stays its
//! colour), then levels of the edges between them, evenly spaced, until it is full; every other
//! colour is drawn with its nearest. No dithering: it would scatter noise over flat squares.

use std::collections::HashMap;

/// The most colours a PNG palette holds.
pub const MAX_COLORS: usize = 256;

/// The most distinct colours this quantises. A picture with more (a photograph, not a board) is
/// written as RGBA instead: the time to choose a palette grows with distinct colours × palette.
const MAX_DISTINCT: usize = 1 << 15;

/// The passes over the colours, by squared distance in RGBA: a colour farther than this from
/// every colour taken so far is taken, the most frequent first. The coarse passes take the
/// board's own colours and spread the rest; the last takes whatever is left while room remains.
const PASSES: [u32; 7] = [64 * 64, 32 * 32, 16 * 16, 8 * 8, 4 * 4, 2 * 2, 0];

/// A pixel as `0xRRGGBBAA`.
type Rgba = u32;

fn channels(c: Rgba) -> [u8; 4] {
    c.to_be_bytes()
}

/// The squared distance of two colours in RGBA (at most 4 × 255², well inside `u32`).
fn distance(a: Rgba, b: Rgba) -> u32 {
    channels(a).iter().zip(channels(b)).map(|(&x, y)| u32::from(x.abs_diff(y)).pow(2)).sum()
}

/// Premultiplied RGBA pixels as straight `0xRRGGBBAA`, as [`crate::export::encode_png`] undoes
/// the premultiplication. `None` when the buffer is not `width × height` pixels.
fn straight(width: u32, height: u32, premultiplied: &[u8]) -> Option<Vec<Rgba>> {
    let pixels = usize::try_from(width).ok()?.checked_mul(usize::try_from(height).ok()?)?;
    if premultiplied.len() != pixels.checked_mul(4)? {
        return None;
    }
    let (chunks, _) = premultiplied.as_chunks::<4>();
    Some(
        chunks
            .iter()
            .map(|&[r, g, b, a]| {
                let un = |c: u8| match a {
                    0 | 255 => c,
                    // c ≤ a in premultiplied pixels; `min` keeps a malformed one in range.
                    a => ((u32::from(c) * 255 + u32::from(a) / 2) / u32::from(a)).min(255) as u8,
                };
                u32::from_be_bytes([un(r), un(g), un(b), a])
            })
            .collect(),
    )
}

/// At most `max` colours (1..=[`MAX_COLORS`]) for a picture whose colours are `counts`, and the
/// palette index of each of them. Deterministic: the order is by count, then by colour.
fn quantise(counts: &HashMap<Rgba, u32>, max: usize) -> (Vec<Rgba>, HashMap<Rgba, u8>) {
    let max = max.clamp(1, MAX_COLORS);
    let mut colors: Vec<(Rgba, u32)> = counts.iter().map(|(&c, &n)| (c, n)).collect();
    colors.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut palette: Vec<Rgba> = Vec::with_capacity(max);
    // Each colour's nearest palette entry so far: its squared distance and index.
    let mut nearest = vec![(u32::MAX, 0u8); colors.len()];
    'passes: for threshold in PASSES {
        for i in 0..colors.len() {
            if palette.len() >= max {
                break 'passes;
            }
            let (Some(&(color, _)), Some(&(d, _))) = (colors.get(i), nearest.get(i)) else { break };
            if d <= threshold {
                continue;
            }
            // Below `max` ≤ 256 entries: the index fits a byte.
            let Ok(index) = u8::try_from(palette.len()) else { break 'passes };
            palette.push(color);
            for ((c, _), n) in colors.iter().zip(nearest.iter_mut()) {
                let d = distance(*c, color);
                if d < n.0 {
                    *n = (d, index);
                }
            }
        }
    }
    let index = colors.iter().zip(&nearest).map(|((c, _), (_, i))| (*c, *i)).collect();
    (palette, index)
}

/// Premultiplied RGBA → PNG with a palette of at most `max_colors` (1..=[`MAX_COLORS`]) colours,
/// as few bits a pixel as they need, transparency in `tRNS` when any colour has it. A picture of
/// more than [`MAX_DISTINCT`] colours is written as RGBA ([`crate::export::encode_png`]).
pub fn encode(width: u32, height: u32, premultiplied: &[u8], max_colors: usize) -> Result<Vec<u8>, String> {
    let pixels = straight(width, height, premultiplied).ok_or_else(|| format!("the picture is not {width} × {height} RGBA pixels"))?;
    let mut counts: HashMap<Rgba, u32> = HashMap::new();
    for &p in &pixels {
        let n = counts.entry(p).or_insert(0);
        *n = n.saturating_add(1);
        if counts.len() > MAX_DISTINCT {
            return crate::export::encode_png(width, height, premultiplied);
        }
    }
    let (palette, index) = quantise(&counts, max_colors);
    let (depth, bits) = match palette.len() {
        0..=2 => (png::BitDepth::One, 1),
        3..=4 => (png::BitDepth::Two, 2),
        5..=16 => (png::BitDepth::Four, 4),
        _ => (png::BitDepth::Eight, 8),
    };
    let per_byte = 8 / bits;
    let width_px = usize::try_from(width).map_err(|e| e.to_string())?;
    let row_bytes = width_px.div_ceil(per_byte);
    let mut data = Vec::with_capacity(row_bytes.saturating_mul(usize::try_from(height).unwrap_or(0)));
    for row in pixels.chunks(width_px.max(1)) {
        let mut packed = vec![0u8; row_bytes];
        for (x, p) in row.iter().enumerate() {
            let i = index.get(p).copied().unwrap_or(0);
            // MSB first; the shift is below 8 as `bits` divides 8.
            let shift = 8 - bits * (x % per_byte + 1);
            if let Some(b) = packed.get_mut(x / per_byte) {
                *b |= i << shift;
            }
        }
        data.extend_from_slice(&packed);
    }
    let rgb: Vec<u8> = palette
        .iter()
        .flat_map(|&c| {
            let [r, g, b, _] = channels(c);
            [r, g, b]
        })
        .collect();
    let alpha: Vec<u8> = palette
        .iter()
        .map(|&c| {
            let [_, _, _, a] = channels(c);
            a
        })
        .collect();
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, width, height);
    enc.set_color(png::ColorType::Indexed);
    enc.set_depth(depth);
    enc.set_palette(rgb);
    if alpha.iter().any(|&a| a != 255) {
        enc.set_trns(alpha);
    }
    // Indices are not intensities: the PNG specification advises no filter for them, and on the
    // boards measured every filter made the file 15–20 % larger.
    enc.set_filter(png::Filter::NoFilter);
    enc.set_deflate_compression(png::DeflateCompression::Level(9));
    let mut w = enc.write_header().map_err(|e| e.to_string())?;
    w.write_image_data(&data).map_err(|e| e.to_string())?;
    w.finish().map_err(|e| e.to_string())?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PNG's colour type and bit depth, and its pixels as straight RGBA.
    fn decode(png: &[u8]) -> (png::ColorType, png::BitDepth, Vec<u8>) {
        let mut dec = png::Decoder::new(std::io::Cursor::new(png));
        dec.set_transformations(png::Transformations::EXPAND);
        let mut reader = dec.read_info().unwrap();
        let (color, depth) = (reader.info().color_type, reader.info().bit_depth);
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        buf.truncate(info.buffer_size());
        let rgba = match info.color_type {
            png::ColorType::Rgb => buf.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
            png::ColorType::Rgba => buf,
            other => panic!("{other:?}"),
        };
        (color, depth, rgba)
    }

    /// An opaque picture of `w` × `h` from a colour per pixel.
    fn picture(w: u32, h: u32, color: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
        (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .flat_map(|(x, y)| {
                let [r, g, b] = color(x, y);
                [r, g, b, 255]
            })
            .collect()
    }

    /// Plan A9-X1b: a board is paletted and decodes to the picture it was made from, the colours
    /// that cover most of it exact and the rest within a few levels; a board has some 700
    /// colours, its antialiasing, which 256 hold within 8 levels and Word's 64 within 16 (its
    /// fallback picture), the squares and the pieces' fills exact in both.
    #[test]
    fn a_board_is_paletted_and_decodes_to_the_same_picture() {
        use pdfcraft_chess::draw::{BoardStyle, PieceSet, Rgb};
        let placement = pdfcraft_chess::fen::parse_placement("r1bq1rk1/pp2bppp/2n1pn2/3p4/2PP4/2N1PN2/PP3PPP/R2QKB1R").unwrap();
        let style = BoardStyle { coordinates: true, ..BoardStyle::default() };
        let (w, h, rgba) = crate::chess::office::diagram_rgba(&placement, &PieceSet::geometric(), &style, 400).unwrap();
        let distinct: std::collections::HashSet<&[u8]> = rgba.chunks(4).collect();
        assert!(distinct.len() > MAX_COLORS, "{} colours: the palette has to choose", distinct.len());
        let rgba_file = crate::export::encode_png(w, h, &rgba).unwrap().len();
        for (colors, bound) in [(MAX_COLORS, 8), (crate::chess::office::WORD_DIAGRAM_COLORS, 16)] {
            let png = encode(w, h, &rgba, colors).unwrap();
            let (color, depth, back) = decode(&png);
            assert_eq!((color, depth), (png::ColorType::Indexed, png::BitDepth::Eight), "{colors}");
            assert_eq!(back.len(), rgba.len());
            let errors: Vec<u8> = rgba.iter().zip(&back).map(|(a, b)| a.abs_diff(*b)).collect();
            let worst = errors.iter().copied().max().unwrap_or(0);
            let mean = errors.iter().map(|&e| f64::from(e)).sum::<f64>() / errors.len() as f64;
            assert!(worst <= bound && mean < 0.2, "{colors} colours: worst {worst}, mean {mean}");
            // The squares and the pieces' fills are exact.
            for c in [style.light, style.dark, Rgb::WHITE, Rgb::BLACK] {
                let want = [c.0, c.1, c.2, 255];
                let (mut n, mut kept) = (0, 0);
                for (a, b) in rgba.chunks(4).zip(back.chunks(4)).filter(|(a, _)| *a == want) {
                    n += 1;
                    kept += usize::from(a == b);
                }
                assert!(n > 100 && kept == n, "{colors} colours, {c:?}: {kept} of {n}");
            }
            assert!(png.len() * 2 < rgba_file, "{colors} colours: less than half the RGBA file");
            // The same picture, the same file.
            assert_eq!(encode(w, h, &rgba, colors).unwrap(), png);
        }
    }

    /// A picture of 2, 4, 16 or 17 colours takes 1, 2, 4 or 8 bits a pixel, rows of any width
    /// packed and decoded exactly; fewer colours asked than it has are the nearest of those taken.
    #[test]
    fn few_colours_take_few_bits_and_come_back_exact() {
        for (n, depth) in
            [(1u32, png::BitDepth::One), (2, png::BitDepth::One), (4, png::BitDepth::Two), (16, png::BitDepth::Four), (17, png::BitDepth::Eight)]
        {
            for w in [1u32, 7, 9, 33] {
                // Every one of the `n` colours, whatever the width: pixel i has colour i mod n.
                let rgba = picture(w, 17, |x, y| {
                    let k = u8::try_from((y * w + x) % n).unwrap();
                    [k * 13, 255 - k * 11, k.wrapping_mul(97)]
                });
                let (color, d, back) = decode(&encode(w, 17, &rgba, MAX_COLORS).unwrap());
                assert_eq!((color, d), (png::ColorType::Indexed, depth), "{n} colours, {w} wide");
                assert_eq!(back, rgba, "{n} colours, {w} wide");
            }
        }
        // Two colours asked of four: the frequent black and white kept, the grays drawn with
        // the nearest of them.
        let gray = |v: u8| [v, v, v];
        let row = [gray(0), gray(255), gray(0), gray(255), gray(0), gray(255), gray(40), gray(200)];
        let rgba = picture(8, 1, |x, _| row[x as usize]);
        let (_, depth, back) = decode(&encode(8, 1, &rgba, 2).unwrap());
        assert_eq!(depth, png::BitDepth::One);
        let want: Vec<u8> = [0u8, 255, 0, 255, 0, 255, 0, 255].iter().flat_map(|&v| [v, v, v, 255]).collect();
        assert_eq!(back, want);
    }

    /// Transparency is kept (`tRNS`), and a premultiplied pixel comes back straight.
    #[test]
    fn transparency_is_kept() {
        let premultiplied = [0, 0, 0, 0, 100, 50, 0, 128, 255, 255, 255, 255];
        let (color, _, back) = decode(&encode(3, 1, &premultiplied, MAX_COLORS).unwrap());
        assert_eq!(color, png::ColorType::Indexed);
        assert_eq!(back, [0, 0, 0, 0, 199, 100, 0, 128, 255, 255, 255, 255]);
    }

    /// A picture of more colours than a palette is worth choosing for (a photograph) is written
    /// as RGBA, the same picture; one that is not its size in pixels is refused, whatever the
    /// size says.
    #[test]
    fn many_colours_are_rgba_and_a_wrong_buffer_is_refused() {
        let (w, h) = (256, 160);
        let rgba = picture(w, h, |x, y| [x as u8, y as u8, (x ^ y) as u8 ^ 0x5A]);
        let (color, _, back) = decode(&encode(w, h, &rgba, MAX_COLORS).unwrap());
        assert_eq!((color, back), (png::ColorType::Rgba, rgba));
        assert!(encode(10, 10, &[0; 12], MAX_COLORS).is_err());
        assert!(encode(u32::MAX, u32::MAX, &[], MAX_COLORS).is_err());
        assert!(encode(0, 0, &[], MAX_COLORS).is_err(), "no picture: the encoder refuses it");
    }
}
