//! SVG output for drawn boards, used by HTML and EPUB export.
//!
//! The SVG is static: no script, no external reference, no font file. Coordinate labels use the
//! reader's sans-serif font.
//!
//! A page with many boards draws each piece once, as a `<symbol>` in [`symbols`], and each board
//! from [`board_described`] places it with `<use>` (plan A8 P9b): 548 diagrams drawn in full reach
//! 82 MB with a detailed piece set, and the same drawing repeated is most of it.

use std::fmt::Write as _;

use crate::draw::{BoardStyle, Drawing, Layer, Paint, PieceSet, Rgb, Segment, Subpath, board, glyph_index, piece_spots};
use crate::placement::{Color, Piece, PieceKind, Placement};

/// Write `drawing` as a standalone SVG element. `title` becomes its accessible name (the FEN, or
/// a caption); it is escaped.
pub fn to_svg(drawing: &Drawing, title: &str) -> String {
    // Writing to a String cannot fail, so the results of `write!` are ignored throughout.
    let mut s = String::new();
    let _ = write!(s, r#"{}><title>{}</title>"#, open(drawing, ""), escape(title));
    body(drawing, s, "")
}

/// [`to_svg`] with an accessible name and a description (plan A8 P9b): `<title id="{id}-t">` and
/// `<desc id="{id}-d">`, joined to the image by `aria-labelledby`, so a screen reader says both.
/// `id` is made of letters, digits, `-` and `_` only (others are dropped): it must be unique in
/// the page.
pub fn to_svg_described(drawing: &Drawing, id: &str, title: &str, desc: &str) -> String {
    body(drawing, described(drawing, id, title, desc, ""), "")
}

/// [`to_svg_described`] for `placement` drawn with `pieces` in `style`, the pieces placed with
/// `<use>` from the `<symbol>`s [`symbols`] writes, which must be in the same document; `used`
/// gains the pieces this board needs. Draws exactly as the full drawing, smaller.
pub fn board_described(
    placement: &Placement,
    pieces: &PieceSet,
    style: &BoardStyle,
    used: &mut UsedPieces,
    id: &str,
    title: &str,
    desc: &str,
) -> String {
    let squares = board(&Placement::empty(), pieces, style);
    let mut uses = String::new();
    for spot in piece_spots(placement, style) {
        used.add(spot.piece);
        let href = symbol_id(spot.piece);
        let (x, y, w) = (num(spot.x), num(spot.y), num(spot.size));
        let _ = write!(uses, r##"<use href="#{href}" xlink:href="#{href}" x="{x}" y="{y}" width="{w}" height="{w}"/>"##);
    }
    body(&squares, described(&squares, id, title, desc, XLINK), &uses)
}

/// The `xmlns:xlink` declaration: `<use>` names its symbol with both `href` (SVG 2) and
/// `xlink:href` (SVG 1.1, which older e-book readers still need), and XHTML wants the prefix
/// declared.
const XLINK: &str = r#" xmlns:xlink="http://www.w3.org/1999/xlink""#;

/// The opening of a described `<svg>`, through its `<desc>`.
fn described(drawing: &Drawing, id: &str, title: &str, desc: &str, extra: &str) -> String {
    let id: String = id.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_')).collect();
    let mut s = String::new();
    let _ = write!(
        s,
        r#"{} aria-labelledby="{id}-t {id}-d"><title id="{id}-t">{}</title><desc id="{id}-d">{}</desc>"#,
        open(drawing, extra),
        escape(title),
        escape(desc),
    );
    s
}

/// The pieces a document's boards use, so [`symbols`] writes only those.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UsedPieces([bool; 12]);

impl UsedPieces {
    pub fn add(&mut self, piece: Piece) {
        if let Some(slot) = self.0.get_mut(glyph_index(piece)) {
            *slot = true;
        }
    }

    pub fn is_empty(&self) -> bool {
        !self.0.contains(&true)
    }
}

/// The id of `piece`'s `<symbol>`: `pc-wK`, `pc-bp`, and so on (the colour, then the FEN letter).
pub fn symbol_id(piece: Piece) -> String {
    format!("pc-{}{}", piece.color.fen_char(), piece.fen_char())
}

/// A hidden `<svg>` holding one `<symbol>` for each piece in `used`, drawn from `pieces` in a unit
/// square, for the boards [`board_described`] writes in the same document. Empty when no piece is
/// used. The symbols are not clipped to their square, as the full drawing is not.
pub fn symbols(pieces: &PieceSet, used: &UsedPieces) -> String {
    if used.is_empty() {
        return String::new();
    }
    let mut s = String::from(r#"<svg xmlns="http://www.w3.org/2000/svg" width="0" height="0" style="position:absolute" aria-hidden="true"><defs>"#);
    for color in [Color::White, Color::Black] {
        for kind in PieceKind::ALL {
            let piece = Piece::new(color, kind);
            if used.0.get(glyph_index(piece)) != Some(&true) {
                continue;
            }
            let _ = write!(s, r#"<symbol id="{}" viewBox="0 0 1 1" overflow="visible">"#, symbol_id(piece));
            paint_layers(&mut s, &pieces.glyph(piece).layers, UNIT_PLACES);
            s.push_str("</symbol>");
        }
    }
    s.push_str("</defs></svg>");
    s
}

/// Decimals in a symbol's unit square: a thousandth of a square is finer than the two decimals of
/// a board 20 units a square wide.
const UNIT_PLACES: usize = 3;

/// The `<svg` element's opening, without its closing `>`; `extra` follows the namespace.
fn open(drawing: &Drawing, extra: &str) -> String {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg"{extra} viewBox="0 0 {w} {h}" width="{w}" height="{h}" role="img""#,
        w = num(drawing.width),
        h = num(drawing.height),
    )
}

/// The drawing's paths after `s`, then `uses`, the labels and the closing tag.
fn body(drawing: &Drawing, mut s: String, uses: &str) -> String {
    paint_layers(&mut s, &drawing.layers, 2);
    s.push_str(uses);
    for label in &drawing.labels {
        let _ = write!(
            s,
            r#"<text x="{}" y="{}" font-size="{}" fill="{}" font-family="sans-serif" text-anchor="middle" dominant-baseline="central">{}</text>"#,
            num(label.x),
            num(label.y),
            num(label.size),
            hex(label.color),
            escape(&label.text)
        );
    }
    s.push_str("</svg>");
    s
}

/// `layers` as `<path>`s, their numbers with at most `places` decimals.
fn paint_layers(s: &mut String, layers: &[Layer], places: usize) {
    for layer in layers {
        let d = path_data(&layer.subpaths, places);
        if d.is_empty() {
            continue;
        }
        match layer.paint {
            Paint::Fill { color, even_odd } => {
                let rule = if even_odd { r#" fill-rule="evenodd""# } else { "" };
                let _ = write!(s, r#"<path d="{d}" fill="{}"{rule}/>"#, hex(color));
            }
            Paint::Stroke { color, width } => {
                let width = num_places(width, places);
                let _ = write!(s, r#"<path d="{d}" fill="none" stroke="{}" stroke-width="{width}" stroke-linejoin="round"/>"#, hex(color));
            }
        }
    }
}

fn path_data(subpaths: &[Subpath], places: usize) -> String {
    let num = |v: f32| num_places(v, places);
    let mut d = String::new();
    for p in subpaths {
        let _ = write!(d, "M{} {}", num(p.start.0), num(p.start.1));
        for seg in &p.segments {
            match *seg {
                Segment::Line(x, y) => {
                    let _ = write!(d, "L{} {}", num(x), num(y));
                }
                Segment::Cubic(x1, y1, x2, y2, x, y) => {
                    let _ = write!(d, "C{} {} {} {} {} {}", num(x1), num(y1), num(x2), num(y2), num(x), num(y));
                }
            }
        }
        if p.closed {
            d.push('Z');
        }
    }
    d
}

/// A number with at most two decimals and no trailing zeros. Non-finite values become 0, so the
/// SVG stays valid whatever the drawing holds.
fn num(v: f32) -> String {
    num_places(v, 2)
}

/// [`num`] with at most `places` decimals.
fn num_places(v: f32, places: usize) -> String {
    if !v.is_finite() {
        return "0".to_owned();
    }
    let s = format!("{v:.places$}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" || s.is_empty() { "0".to_owned() } else { s.to_owned() }
}

fn hex(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            // Characters XML 1.0 does not allow are dropped.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw::{BoardStyle, PieceSet, board};
    use crate::placement::Placement;

    #[test]
    fn a_board_becomes_one_svg_element() {
        let d = board(&Placement::standard(), &PieceSet::geometric(), &BoardStyle { coordinates: true, ..BoardStyle::default() });
        let svg = to_svg(&d, "start <position> & \"more\"");
        assert!(svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""));
        assert!(svg.ends_with("</svg>"));
        assert!(svg.contains("<title>start &lt;position&gt; &amp; &quot;more&quot;</title>"));
        // Border, background, dark squares, and two layers for each of 32 pieces.
        assert_eq!(svg.matches("<path ").count(), 3 + 64);
        assert_eq!(svg.matches("<text ").count(), 16);
        assert!(!svg.contains("NaN") && !svg.contains("inf") && !svg.contains("<script"));
    }

    #[test]
    fn numbers_are_short_and_finite() {
        assert_eq!(num(20.0), "20");
        assert_eq!(num(0.126), "0.13");
        assert_eq!(num(1.5), "1.5");
        assert_eq!(num(-0.001), "0");
        assert_eq!(num(f32::NAN), "0");
        assert_eq!(num(f32::NEG_INFINITY), "0");
    }

    #[test]
    fn control_characters_are_dropped() {
        assert_eq!(escape("a\u{1}b\nc"), "ab\nc");
    }
}
