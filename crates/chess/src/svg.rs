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

/// `placement` drawn with `pieces` in `style` as a standalone SVG for a reader that takes no
/// `<use>` (Word's `svgBlip`; plan A9-X1b, decision D18): every piece drawn in place, as
/// [`to_svg`] draws it, but each path written from its first point, the rest relative to it
/// (`m`, `l`, `c`, a command letter not repeated), counted on the piece's own square, in tenths
/// of a unit ([`tenths`]). The same piece is then the same text wherever it stands, which the
/// archive's compressor stores once per board. No title and no `role`: the reader names the
/// picture itself (Word's `descr`), and the same position is the same file.
pub fn board_standalone(placement: &Placement, pieces: &PieceSet, style: &BoardStyle) -> String {
    let squares = board(&Placement::empty(), pieces, style);
    let mut s = String::new();
    let _ = write!(
        s,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" width="{w}" height="{h}">"#,
        w = num(squares.width),
        h = num(squares.height),
    );
    // The pieces, painted over the squares and under the labels.
    let mut drawn = String::new();
    for spot in piece_spots(placement, style) {
        for layer in &pieces.glyph(spot.piece).layers {
            let d = relative_path_data(&layer.subpaths, spot.x, spot.y, spot.size);
            if d.is_empty() {
                continue;
            }
            match layer.paint {
                Paint::Fill { color, even_odd } => {
                    let rule = if even_odd { r#" fill-rule="evenodd""# } else { "" };
                    let _ = write!(drawn, r#"<path d="{d}" fill="{}"{rule}/>"#, hex(color));
                }
                Paint::Stroke { color, width } => {
                    // As `board` scales a piece's stroke by its square.
                    let width = num(width * spot.size);
                    let _ = write!(drawn, r#"<path d="{d}" fill="none" stroke="{}" stroke-width="{width}" stroke-linejoin="round"/>"#, hex(color));
                }
            }
        }
    }
    body(&squares, s, &drawn)
}

/// A drawing coordinate in tenths of a unit, the precision of [`board_standalone`]'s pieces: a
/// two-hundredth of a 20-unit square, 0.05 mm on a board three inches wide, finer than a 1200 dpi
/// printer's dot. Non-finite values are 0; the cast saturates, and the clamp keeps every sum and
/// difference of two far from `i64`'s ends.
fn tenths(v: f32) -> i64 {
    if v.is_finite() { (f64::from(v) * 10.0).round().clamp(-1e15, 1e15) as i64 } else { 0 }
}

/// `t` tenths as a number, at most one decimal (`125` → `12.5`, `120` → `12`).
fn fixed(t: i64) -> String {
    let sign = if t < 0 { "-" } else { "" };
    match (t.unsigned_abs() / 10, t.unsigned_abs() % 10) {
        (whole, 0) => format!("{sign}{whole}"),
        (whole, tenth) => format!("{sign}{whole}.{tenth}"),
    }
}

/// Unit-square `subpaths` scaled by `size` and moved to `(x, y)`, as path data: the first point
/// absolute (`M`), every other point relative to the one before (`m`, `l`, `c`; after a `Z`, to
/// the start of the subpath it closed, as SVG has it), a `l` or `c` after another left to be
/// understood, as SVG allows. Every point is rounded on the piece's own square before the
/// differences are taken, so the text after the `M` does not depend on where the piece stands
/// and the rounding never adds up along a path.
fn relative_path_data(subpaths: &[Subpath], x: f32, y: f32, size: f32) -> String {
    let local = |px: f32, py: f32| (tenths(px * size), tenths(py * size));
    let mut d = String::new();
    // The current point, on the piece's square; none before the first subpath.
    let mut at: Option<(i64, i64)> = None;
    // The command written last: the same one again needs only a space.
    let mut last = 'M';
    let mut command = |d: &mut String, c: char| {
        d.push(if c == last && matches!(c, 'l' | 'c') { ' ' } else { c });
        last = c;
    };
    for p in subpaths {
        let start = local(p.start.0, p.start.1);
        match at {
            None => {
                command(&mut d, 'M');
                let _ = write!(d, "{} {}", fixed(tenths(x).saturating_add(start.0)), fixed(tenths(y).saturating_add(start.1)));
            }
            Some((cx, cy)) => {
                command(&mut d, 'm');
                let _ = write!(d, "{} {}", fixed(start.0 - cx), fixed(start.1 - cy));
            }
        }
        let mut cur = start;
        for seg in &p.segments {
            match *seg {
                Segment::Line(px, py) => {
                    let to = local(px, py);
                    command(&mut d, 'l');
                    let _ = write!(d, "{} {}", fixed(to.0 - cur.0), fixed(to.1 - cur.1));
                    cur = to;
                }
                Segment::Cubic(x1, y1, x2, y2, px, py) => {
                    let (c1, c2, to) = (local(x1, y1), local(x2, y2), local(px, py));
                    command(&mut d, 'c');
                    let _ = write!(
                        d,
                        "{} {} {} {} {} {}",
                        fixed(c1.0 - cur.0),
                        fixed(c1.1 - cur.1),
                        fixed(c2.0 - cur.0),
                        fixed(c2.1 - cur.1),
                        fixed(to.0 - cur.0),
                        fixed(to.1 - cur.1)
                    );
                    cur = to;
                }
            }
        }
        if p.closed {
            command(&mut d, 'Z');
            cur = start;
        }
        at = Some(cur);
    }
    d
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
            // Characters XML 1.0 does not allow are dropped, and the noncharacters it discourages.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c if noncharacter(c) => {}
            c => out.push(c),
        }
    }
    out
}

/// Whether `c` is a Unicode noncharacter (plan A9-X1a, A2-17): U+FDD0 to U+FDEF, and the last two
/// code points of every plane (U+FFFE, U+FFFF, U+1FFFE…). XML 1.0 forbids U+FFFE and U+FFFF and
/// discourages the rest; a text layer may hold them (a font's broken `/ToUnicode`), and one made
/// an EPUB page not well-formed.
pub fn noncharacter(c: char) -> bool {
    let n = u32::from(c);
    (0xFDD0..=0xFDEF).contains(&n) || n & 0xFFFE == 0xFFFE
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

    /// The points of path data, absolute, in the order written: `M`/`m`, `L`/`l`, `C`/`c` and `Z`
    /// as SVG reads them (after a `Z`, the current point is the start of the subpath it closed).
    fn absolute_points(d: &str) -> Vec<(f64, f64)> {
        let mut tokens: Vec<String> = Vec::new();
        for c in d.chars() {
            if c.is_ascii_alphabetic() {
                tokens.push(c.to_string());
                tokens.push(String::new());
            } else if c == ' ' || c == '-' {
                tokens.push(if c == '-' { "-".into() } else { String::new() });
            } else if let Some(last) = tokens.last_mut() {
                last.push(c);
            }
        }
        let mut tokens = tokens.into_iter().filter(|t| !t.is_empty() && t != "-").peekable();
        let (mut cur, mut start, mut out) = ((0.0, 0.0), (0.0, 0.0), Vec::new());
        let mut last = String::new();
        while let Some(next) = tokens.peek() {
            // A number where a command goes repeats the last one (after a move, a line).
            let cmd = if next.chars().all(|c| c.is_ascii_alphabetic()) {
                tokens.next().unwrap()
            } else {
                match last.as_str() {
                    "M" => "L".to_owned(),
                    "m" => "l".to_owned(),
                    other => other.to_owned(),
                }
            };
            last.clone_from(&cmd);
            let mut num = || tokens.next().unwrap().parse::<f64>().unwrap();
            match cmd.as_str() {
                "Z" | "z" => cur = start,
                "M" | "m" | "L" | "l" => {
                    let (x, y) = (num(), num());
                    cur = if cmd.chars().all(|c| c.is_ascii_uppercase()) { (x, y) } else { (cur.0 + x, cur.1 + y) };
                    if matches!(cmd.as_str(), "M" | "m") {
                        start = cur;
                    }
                    out.push(cur);
                }
                "C" | "c" => {
                    let v: Vec<f64> = (0..6).map(|_| num()).collect();
                    let abs = cmd == "C";
                    for k in 0..3 {
                        let p = if abs { (v[2 * k], v[2 * k + 1]) } else { (cur.0 + v[2 * k], cur.1 + v[2 * k + 1]) };
                        out.push(p);
                    }
                    cur = *out.last().unwrap();
                }
                other => panic!("{other} in {d}"),
            }
        }
        out
    }

    /// The `d` and the rest of each `<path>` of an SVG.
    fn paths(svg: &str) -> Vec<(String, String)> {
        svg.split("<path d=\"")
            .skip(1)
            .map(|p| {
                let (d, rest) = p.split_once('"').unwrap();
                (d.to_owned(), rest.split_once("/>").unwrap().0.to_owned())
            })
            .collect()
    }

    /// Plan A9-X1b (D18: no `<use>` for Word): the standalone board draws what [`to_svg`] draws,
    /// path by path and point by point within the rounding, with no `<use>`, no title; a piece is
    /// the same text after its first point wherever it stands, which is what the archive's
    /// compressor shares; and it is smaller than the board written point by point.
    #[test]
    fn the_standalone_board_draws_the_same_board_with_pieces_as_repeated_text() {
        let pieces = PieceSet::geometric();
        let middle = crate::fen::parse_placement("r1bq1rk1/pp2bppp/2n1pn2/3p4/2PP4/2N1PN2/PP3PPP/R2QKB1R").unwrap();
        for (placement, flipped, coordinates) in
            [(Placement::standard(), false, true), (Placement::standard(), true, false), (middle, true, true), (Placement::empty(), false, false)]
        {
            let style = BoardStyle { flipped, coordinates, ..BoardStyle::default() };
            let full = to_svg(&board(&placement, &pieces, &style), "t");
            let alone = board_standalone(&placement, &pieces, &style);
            assert!(alone.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=") && alone.ends_with("</svg>"), "{alone}");
            assert!(!alone.contains("<use") && !alone.contains("<title") && !alone.contains("<defs") && !alone.contains("role="), "{alone}");
            assert_eq!(alone.matches("<text ").count(), if coordinates { 16 } else { 0 });
            let (a, b) = (paths(&full), paths(&alone));
            assert_eq!(a.len(), b.len());
            for ((da, ra), (db, rb)) in a.iter().zip(&b) {
                assert_eq!(ra, rb, "the same paint");
                let (pa, pb) = (absolute_points(da), absolute_points(db));
                assert_eq!(pa.len(), pb.len(), "{da}\n{db}");
                // A tenth of a unit: the piece's point and its square's corner each rounded to the
                // nearest tenth, the full drawing's to a hundredth.
                for (p, q) in pa.iter().zip(&pb) {
                    assert!((p.0 - q.0).abs() <= 0.106 && (p.1 - q.1).abs() <= 0.106, "{p:?} {q:?}\n{da}\n{db}");
                }
            }
            if placement != Placement::empty() {
                assert!(alone.len() < full.len(), "{} against {}", alone.len(), full.len());
            }
        }
        // A piece is the same text after its first point wherever it stands.
        let alone = board_standalone(&Placement::standard(), &pieces, &BoardStyle::default());
        let tails: std::collections::BTreeSet<String> =
            paths(&alone).iter().map(|(d, rest)| format!("{}{rest}", &d[d.find(|c: char| c.is_ascii_lowercase()).unwrap_or(d.len())..])).collect();
        // The squares (border, light, dark), then each of the six kinds filled white, filled
        // black, and outlined (the same black outline for both colours).
        assert_eq!(tails.len(), 3 + 6 * 3, "{tails:?}");
    }

    #[test]
    fn tenths_are_written_short_and_exact() {
        assert_eq!([0, 5, -5, 125, -125, 120, 10, -10].map(fixed), ["0", "0.5", "-0.5", "12.5", "-12.5", "12", "1", "-1"]);
        assert_eq!(tenths(f32::NAN), 0);
        assert_eq!(tenths(f32::INFINITY), 0);
        assert_eq!(tenths(1e30), 1_000_000_000_000_000);
        assert_eq!(tenths(-0.04), 0);
        assert_eq!(tenths(12.34), 123);
        assert_eq!(fixed(i64::MIN), "-922337203685477580.8");
        // A piece far off the board, or with points that are not numbers, still writes numbers.
        let wild = Subpath {
            start: (f32::NAN, 1e30),
            segments: vec![Segment::Line(f32::INFINITY, -1e30), Segment::Cubic(1.0, 2.0, 3.0, 4.0, 5.0, 6.0)],
            closed: true,
        };
        let d = relative_path_data(&[wild.clone(), wild], f32::MAX, f32::MIN, 1e30);
        assert!(!d.contains("NaN") && !d.contains("inf"), "{d}");
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

    /// Plan A9-X1a (A2-17): the noncharacters, U+FFFE and U+FFFF above all (XML forbids them), are
    /// dropped; the characters beside them are kept.
    #[test]
    fn noncharacters_are_dropped() {
        assert_eq!(escape("a\u{FFFE}b\u{FFFF}c\u{FDD0}d\u{FDEF}e\u{10FFFF}f"), "abcdef");
        assert_eq!(escape("\u{FFFD}\u{FDCF}\u{FDF0}\u{FFFC}"), "\u{FFFD}\u{FDCF}\u{FDF0}\u{FFFC}");
    }
}
