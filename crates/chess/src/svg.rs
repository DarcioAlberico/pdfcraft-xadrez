//! SVG output for drawn boards, used by HTML and EPUB export.
//!
//! The SVG is static: no script, no external reference, no font file. Coordinate labels use the
//! reader's sans-serif font.

use std::fmt::Write as _;

use crate::draw::{Drawing, Paint, Rgb, Segment, Subpath};

/// Write `drawing` as a standalone SVG element. `title` becomes its accessible name (the FEN, or
/// a caption); it is escaped.
pub fn to_svg(drawing: &Drawing, title: &str) -> String {
    // Writing to a String cannot fail, so the results of `write!` are ignored throughout.
    let mut s = String::new();
    let _ = write!(s, r#"{}><title>{}</title>"#, open(drawing), escape(title));
    body(drawing, s)
}

/// [`to_svg`] with an accessible name and a description (plan A8 P9b): `<title id="{id}-t">` and
/// `<desc id="{id}-d">`, joined to the image by `aria-labelledby`, so a screen reader says both.
/// `id` is made of letters, digits, `-` and `_` only (others are dropped): it must be unique in
/// the page.
pub fn to_svg_described(drawing: &Drawing, id: &str, title: &str, desc: &str) -> String {
    let id: String = id.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_')).collect();
    let mut s = String::new();
    let _ = write!(
        s,
        r#"{} aria-labelledby="{id}-t {id}-d"><title id="{id}-t">{}</title><desc id="{id}-d">{}</desc>"#,
        open(drawing),
        escape(title),
        escape(desc),
    );
    body(drawing, s)
}

/// The `<svg` element's opening, without its closing `>`.
fn open(drawing: &Drawing) -> String {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" width="{w}" height="{h}" role="img""#,
        w = num(drawing.width),
        h = num(drawing.height),
    )
}

/// The drawing's paths and labels after `s`, and the closing tag.
fn body(drawing: &Drawing, mut s: String) -> String {
    for layer in &drawing.layers {
        let d = path_data(&layer.subpaths);
        if d.is_empty() {
            continue;
        }
        match layer.paint {
            Paint::Fill { color, even_odd } => {
                let rule = if even_odd { r#" fill-rule="evenodd""# } else { "" };
                let _ = write!(s, r#"<path d="{d}" fill="{}"{rule}/>"#, hex(color));
            }
            Paint::Stroke { color, width } => {
                let _ = write!(s, r#"<path d="{d}" fill="none" stroke="{}" stroke-width="{}" stroke-linejoin="round"/>"#, hex(color), num(width));
            }
        }
    }
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

fn path_data(subpaths: &[Subpath]) -> String {
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
    if !v.is_finite() {
        return "0".to_owned();
    }
    let s = format!("{v:.2}");
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
