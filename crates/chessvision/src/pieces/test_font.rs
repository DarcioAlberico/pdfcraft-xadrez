//! A synthetic TrueType font for tests. PdfCraft commits no chess font (blueprint decision D4),
//! so the tests that read one build it here: each character draws one rectangle whose width says
//! which character it is ([`width_of`], [`drawn_char`]), and `+`, the empty dark square of the
//! diagram layouts, fills the em square, which becomes the cell the pieces are drawn in.
//!
//! Built for this crate's tests, and for other crates' tests through the `test-fonts` feature.

use pdfcraft_chess::draw::{PieceGlyph, bounds};

/// Units per em.
const UPEM: u16 = 1000;

/// The width of the rectangle `c` draws, in font units: the em for `+`, else one width per
/// character code below 256 (others share them).
pub fn width_of(c: char) -> u16 {
    if c == '+' {
        return UPEM;
    }
    let code = u16::try_from(u32::from(c) & 0xFF).unwrap_or(0);
    10 + 2 * code
}

/// The character whose rectangle `glyph`'s own drawing is (its last layer, over the paper), in a
/// piece set read from a [`font`].
pub fn drawn_char(glyph: &PieceGlyph) -> Option<char> {
    let ink = glyph.layers.last()?;
    let [x0, _, x1, _] = bounds(&ink.subpaths)?;
    let units = ((x1 - x0) * f32::from(UPEM)).round();
    if !(10.0..=520.0).contains(&units) {
        return None;
    }
    // Checked by the range just above: a whole number from 10 to 520.
    let code = (units as u32 - 10) / 2;
    char::from_u32(code)
}

/// A TrueType font named `family` with a glyph for each of `chars`, mapped by a Unicode cmap, or
/// by a symbol one at U+F000 + code when `symbol` (as the chess fonts of the 1990s are). Its
/// OS/2 table says `fs_type`.
pub fn font(family: &str, chars: &str, symbol: bool, fs_type: u16) -> Vec<u8> {
    let mut chars: Vec<char> = chars.chars().filter(|c| !symbol || u32::from(*c) < 0x100).collect();
    chars.sort_unstable();
    chars.dedup();
    let glyph_count = u16::try_from(chars.len() + 1).unwrap_or(u16::MAX);

    // glyf and loca: glyph 0 (.notdef) is empty, from 0 to 0; each glyph ends where the next starts.
    let mut glyf = Vec::new();
    let mut loca = vec![0u32, 0];
    for &c in &chars {
        let (x0, y0, w) = if c == '+' { (0, 0, UPEM) } else { (100, 100, width_of(c)) };
        let (x1, y1) = (x0 + w, if c == '+' { UPEM } else { 800 });
        let to_i16 = |v: u16| i16::try_from(v).unwrap_or(i16::MAX);
        let (x0, y0, x1, y1) = (to_i16(x0), to_i16(y0), to_i16(x1), to_i16(y1));
        push_i16(&mut glyf, 1);
        for v in [x0, y0, x1, y1] {
            push_i16(&mut glyf, v);
        }
        push_u16(&mut glyf, 3); // the last point of the one contour
        push_u16(&mut glyf, 0); // no instructions
        glyf.extend_from_slice(&[1, 1, 1, 1]); // on the curve, coordinates as two-byte deltas
        // Clockwise, as TrueType's outer contours go.
        for dx in [x0, 0, x1 - x0, 0] {
            push_i16(&mut glyf, dx);
        }
        for dy in [y0, y1 - y0, 0, y0 - y1] {
            push_i16(&mut glyf, dy);
        }
        while glyf.len() % 4 != 0 {
            glyf.push(0);
        }
        loca.push(u32::try_from(glyf.len()).unwrap_or(u32::MAX));
    }
    let mut loca_bytes = Vec::new();
    for offset in loca {
        loca_bytes.extend_from_slice(&offset.to_be_bytes());
    }

    let mut head = Vec::new();
    push_u32(&mut head, 0x0001_0000);
    push_u32(&mut head, 0x0001_0000);
    push_u32(&mut head, 0); // checksum adjustment
    push_u32(&mut head, 0x5F0F_3CF5);
    push_u16(&mut head, 0x000B);
    push_u16(&mut head, UPEM);
    head.extend_from_slice(&[0; 16]); // created, modified
    for v in [0, 0, UPEM, UPEM] {
        push_u16(&mut head, v);
    }
    for v in [0, 8, 2, 1, 0] {
        push_u16(&mut head, v); // macStyle, lowestRecPPEM, direction hint, long loca, glyph format
    }

    let mut hhea = Vec::new();
    push_u32(&mut hhea, 0x0001_0000);
    for v in [UPEM, 0, 0, UPEM, 0, 0, UPEM, 1, 0, 0, 0, 0, 0, 0, 0] {
        push_u16(&mut hhea, v);
    }
    push_u16(&mut hhea, glyph_count);

    let mut hmtx = Vec::new();
    for _ in 0..glyph_count {
        push_u16(&mut hmtx, UPEM);
        push_u16(&mut hmtx, 0);
    }

    let mut maxp = Vec::new();
    push_u32(&mut maxp, 0x0001_0000);
    for v in [glyph_count, 4, 1, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0] {
        push_u16(&mut maxp, v);
    }

    // cmap: one format-4 subtable, a segment per character, then the closing one.
    let mut codes: Vec<(u16, u16)> = Vec::new();
    for (gid, &c) in (1u16..).zip(&chars) {
        let code = u32::from(c) + if symbol { 0xF000 } else { 0 };
        if let Ok(code) = u16::try_from(code)
            && code != 0xFFFF
        {
            codes.push((code, gid));
        }
    }
    codes.push((0xFFFF, 0));
    let segments = u16::try_from(codes.len()).unwrap_or(u16::MAX);
    let mut range = 1u16;
    let mut selector = 0u16;
    while range.saturating_mul(2) <= segments {
        range = range.saturating_mul(2);
        selector += 1;
    }
    let mut sub = Vec::new();
    push_u16(&mut sub, 4);
    push_u16(&mut sub, 16u16.saturating_add(segments.saturating_mul(8)));
    push_u16(&mut sub, 0);
    push_u16(&mut sub, segments.saturating_mul(2));
    push_u16(&mut sub, range.saturating_mul(2));
    push_u16(&mut sub, selector);
    push_u16(&mut sub, segments.saturating_mul(2).saturating_sub(range.saturating_mul(2)));
    for (code, _) in &codes {
        push_u16(&mut sub, *code);
    }
    push_u16(&mut sub, 0);
    for (code, _) in &codes {
        push_u16(&mut sub, *code);
    }
    for (code, gid) in &codes {
        // The closing segment maps 0xFFFF to glyph 0.
        let delta = if *code == 0xFFFF { 1 } else { gid.wrapping_sub(*code) };
        push_u16(&mut sub, delta);
    }
    for _ in &codes {
        push_u16(&mut sub, 0);
    }
    let mut cmap = Vec::new();
    push_u16(&mut cmap, 0);
    push_u16(&mut cmap, 1);
    push_u16(&mut cmap, 3);
    push_u16(&mut cmap, if symbol { 0 } else { 1 });
    push_u32(&mut cmap, 12);
    cmap.extend_from_slice(&sub);

    // name: the family (1) and the full name (4), the same UTF-16 text.
    let text: Vec<u8> = family.encode_utf16().flat_map(u16::to_be_bytes).collect();
    let len = u16::try_from(text.len()).unwrap_or(u16::MAX);
    let mut name = Vec::new();
    for v in [0, 2, 6 + 2 * 12] {
        push_u16(&mut name, v);
    }
    for id in [1, 4] {
        for v in [3, 1, 0x0409, id, len, 0] {
            push_u16(&mut name, v);
        }
    }
    name.extend_from_slice(&text);

    // OS/2 version 0: the fsType, the rest left plain.
    let mut os2 = Vec::new();
    for v in [0, 500, 400, 5, fs_type] {
        push_u16(&mut os2, v);
    }
    os2.resize(78, 0);

    let tables: [(&[u8; 4], Vec<u8>); 9] = [
        (b"OS/2", os2),
        (b"cmap", cmap),
        (b"glyf", glyf),
        (b"head", head),
        (b"hhea", hhea),
        (b"hmtx", hmtx),
        (b"loca", loca_bytes),
        (b"maxp", maxp),
        (b"name", name),
    ];
    let mut out = Vec::new();
    push_u32(&mut out, 0x0001_0000);
    for v in [9, 128, 3, 16] {
        push_u16(&mut out, v); // tables, search range, entry selector, range shift
    }
    let mut offset = 12 + 16 * tables.len();
    let mut body = Vec::new();
    for (tag, data) in &tables {
        out.extend_from_slice(*tag);
        push_u32(&mut out, checksum(data));
        push_u32(&mut out, u32::try_from(offset).unwrap_or(u32::MAX));
        push_u32(&mut out, u32::try_from(data.len()).unwrap_or(u32::MAX));
        body.extend_from_slice(data);
        while body.len() % 4 != 0 {
            body.push(0);
        }
        offset = 12 + 16 * tables.len() + body.len();
    }
    out.extend_from_slice(&body);
    out
}

fn push_u16(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_be_bytes());
}

fn push_i16(v: &mut Vec<u8>, x: i16) {
    v.extend_from_slice(&x.to_be_bytes());
}

fn push_u32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_be_bytes());
}

/// A table's checksum: its big-endian words summed, the last one padded with zeros.
fn checksum(data: &[u8]) -> u32 {
    data.chunks(4).fold(0u32, |sum, chunk| {
        let mut word = [0u8; 4];
        for (w, b) in word.iter_mut().zip(chunk) {
            *w = *b;
        }
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}
