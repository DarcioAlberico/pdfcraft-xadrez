//! Board drawing: a placement and a piece set become resolution-free shapes.
//!
//! This is the single source of every drawn diagram. The engine turns the shapes into PDF content
//! when it replaces a diagram, [`crate::svg`] writes them as SVG for HTML and EPUB, and PNG for
//! DOCX, RTF and Markdown is rendered from the same shapes. One drawing, so a diagram looks the
//! same in the PDF and in every export.
//!
//! Coordinates are in drawing units with the origin at the top left and y growing downwards, as
//! in SVG; the PDF writer flips them. One square is [`BoardStyle::square`] units wide.
//!
//! Piece shapes come from the caller as a [`PieceSet`], already as paths. This crate never reads a
//! font or image file: the engine loads the user's piece set (a diagram font or a folder of
//! images on the user's machine) and converts it. [`PieceSet::geometric`] is a plain set drawn in
//! code, for tests and as the last resort when no set is configured.

use crate::placement::{Color, Piece, PieceKind, Placement, Square};

/// An sRGB colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub const BLACK: Rgb = Rgb(0, 0, 0);
    pub const WHITE: Rgb = Rgb(255, 255, 255);
}

/// One segment of a subpath, ending at its last point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Segment {
    Line(f32, f32),
    /// Two control points, then the end point.
    Cubic(f32, f32, f32, f32, f32, f32),
}

/// A connected run of segments.
#[derive(Clone, Debug, PartialEq)]
pub struct Subpath {
    pub start: (f32, f32),
    pub segments: Vec<Segment>,
    pub closed: bool,
}

/// How a path is painted.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Paint {
    /// Filled with the nonzero winding rule, or even-odd when `even_odd`.
    Fill {
        color: Rgb,
        even_odd: bool,
    },
    Stroke {
        color: Rgb,
        width: f32,
    },
}

/// A painted path.
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub subpaths: Vec<Subpath>,
    pub paint: Paint,
}

/// What a piece looks like: layers painted in order, in a unit square (0..1, y down).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct PieceGlyph {
    pub layers: Vec<Layer>,
}

/// The twelve pieces.
#[derive(Clone, Debug, PartialEq)]
pub struct PieceSet {
    /// White pawn, knight, bishop, rook, queen, king, then Black's.
    pub glyphs: [PieceGlyph; 12],
    /// A short name for the set, to show where it came from.
    pub name: String,
}

impl PieceSet {
    pub fn glyph(&self, piece: Piece) -> &PieceGlyph {
        let kind = PieceKind::ALL.iter().position(|k| *k == piece.kind).unwrap_or(0);
        let index = match piece.color {
            Color::White => kind,
            Color::Black => kind + 6,
        };
        // `index < 12` by construction; the fallback only keeps the lookup total.
        self.glyphs.get(index).unwrap_or(&EMPTY_GLYPH)
    }

    /// A plain set drawn in code from polygons: white pieces filled white with a black outline,
    /// black pieces filled black. Recognisable, and free of any third-party artwork.
    pub fn geometric() -> PieceSet {
        let glyphs = std::array::from_fn(|i| {
            let color = if i < 6 { Color::White } else { Color::Black };
            let kind = PieceKind::ALL.get(i % 6).copied().unwrap_or(PieceKind::Pawn);
            geometric_glyph(kind, color)
        });
        PieceSet { glyphs, name: "geometric".to_owned() }
    }
}

static EMPTY_GLYPH: PieceGlyph = PieceGlyph { layers: Vec::new() };

fn polygon(points: &[(f32, f32)]) -> Subpath {
    let mut it = points.iter();
    let start = it.next().copied().unwrap_or((0.0, 0.0));
    Subpath { start, segments: it.map(|&(x, y)| Segment::Line(x, y)).collect(), closed: true }
}

fn circle(cx: f32, cy: f32, r: f32) -> Subpath {
    // Four cubic arcs; 0.5523 is the usual control-point factor for a quarter circle.
    let k = 0.5523 * r;
    Subpath {
        start: (cx + r, cy),
        segments: vec![
            Segment::Cubic(cx + r, cy + k, cx + k, cy + r, cx, cy + r),
            Segment::Cubic(cx - k, cy + r, cx - r, cy + k, cx - r, cy),
            Segment::Cubic(cx - r, cy - k, cx - k, cy - r, cx, cy - r),
            Segment::Cubic(cx + k, cy - r, cx + r, cy - k, cx + r, cy),
        ],
        closed: true,
    }
}

fn geometric_glyph(kind: PieceKind, color: Color) -> PieceGlyph {
    let base = polygon(&[(0.2, 0.88), (0.8, 0.88), (0.8, 0.78), (0.2, 0.78)]);
    let body = match kind {
        PieceKind::Pawn => vec![circle(0.5, 0.42, 0.14), polygon(&[(0.35, 0.78), (0.65, 0.78), (0.56, 0.52), (0.44, 0.52)])],
        PieceKind::Knight => {
            vec![polygon(&[(0.3, 0.78), (0.72, 0.78), (0.68, 0.3), (0.5, 0.15), (0.28, 0.38), (0.36, 0.46), (0.5, 0.4), (0.42, 0.6)])]
        }
        PieceKind::Bishop => vec![polygon(&[(0.32, 0.78), (0.68, 0.78), (0.5, 0.18)]), circle(0.5, 0.15, 0.05)],
        PieceKind::Rook => vec![polygon(&[
            (0.28, 0.78),
            (0.72, 0.78),
            (0.72, 0.2),
            (0.62, 0.2),
            (0.62, 0.28),
            (0.55, 0.28),
            (0.55, 0.2),
            (0.45, 0.2),
            (0.45, 0.28),
            (0.38, 0.28),
            (0.38, 0.2),
            (0.28, 0.2),
        ])],
        PieceKind::Queen => vec![polygon(&[(0.25, 0.78), (0.75, 0.78), (0.82, 0.25), (0.64, 0.5), (0.5, 0.16), (0.36, 0.5), (0.18, 0.25)])],
        PieceKind::King => vec![
            polygon(&[(0.27, 0.78), (0.73, 0.78), (0.66, 0.36), (0.34, 0.36)]),
            polygon(&[
                (0.46, 0.36),
                (0.54, 0.36),
                (0.54, 0.24),
                (0.62, 0.24),
                (0.62, 0.17),
                (0.54, 0.17),
                (0.54, 0.08),
                (0.46, 0.08),
                (0.46, 0.17),
                (0.38, 0.17),
                (0.38, 0.24),
                (0.46, 0.24),
            ]),
        ],
    };
    let mut shapes = vec![base];
    shapes.extend(body);
    let fill = match color {
        Color::White => Rgb::WHITE,
        Color::Black => Rgb::BLACK,
    };
    PieceGlyph {
        layers: vec![
            Layer { subpaths: shapes.clone(), paint: Paint::Fill { color: fill, even_odd: false } },
            Layer { subpaths: shapes, paint: Paint::Stroke { color: Rgb::BLACK, width: 0.03 } },
        ],
    }
}

/// How a board is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoardStyle {
    /// Width of one square, in drawing units.
    pub square: f32,
    pub light: Rgb,
    pub dark: Rgb,
    /// Frame width in square widths (0 for none), and its colour.
    pub border: f32,
    pub border_color: Rgb,
    /// Print a–h and 1–8 outside the frame.
    pub coordinates: bool,
    /// Draw from Black's side: h1 at the top left.
    pub flipped: bool,
}

impl Default for BoardStyle {
    fn default() -> Self {
        BoardStyle {
            square: 20.0,
            light: Rgb(240, 217, 181),
            dark: Rgb(181, 136, 99),
            border: 0.04,
            border_color: Rgb::BLACK,
            coordinates: false,
            flipped: false,
        }
    }
}

/// Text in a drawing (the coordinates), centred on `(x, y)`.
#[derive(Clone, Debug, PartialEq)]
pub struct Label {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub color: Rgb,
}

/// A drawn board.
#[derive(Clone, Debug, PartialEq)]
pub struct Drawing {
    pub width: f32,
    pub height: f32,
    /// Painted in order.
    pub layers: Vec<Layer>,
    pub labels: Vec<Label>,
}

/// The coordinate margin, in square widths, when coordinates are printed.
const LABEL_MARGIN: f32 = 0.5;

/// Draw `placement` with `pieces` in `style`. Non-finite or non-positive sizes fall back to the
/// default style's, so the drawing is always well formed.
pub fn board(placement: &Placement, pieces: &PieceSet, style: &BoardStyle) -> Drawing {
    let defaults = BoardStyle::default();
    let s = if style.square.is_finite() && style.square > 0.0 { style.square } else { defaults.square };
    let border = if style.border.is_finite() && style.border >= 0.0 { style.border.min(1.0) * s } else { 0.0 };
    let margin = if style.coordinates { LABEL_MARGIN * s } else { 0.0 };
    let origin = margin + border;
    let board_size = 8.0 * s;
    let total = board_size + 2.0 * (border + margin);

    let rect = |x: f32, y: f32, w: f32, h: f32| polygon(&[(x, y), (x + w, y), (x + w, y + h), (x, y + h)]);
    let mut layers = Vec::new();
    if border > 0.0 {
        let outer = board_size + 2.0 * border;
        layers.push(Layer { subpaths: vec![rect(margin, margin, outer, outer)], paint: Paint::Fill { color: style.border_color, even_odd: false } });
    }
    layers.push(Layer { subpaths: vec![rect(origin, origin, board_size, board_size)], paint: Paint::Fill { color: style.light, even_odd: false } });

    // Screen position of a square: column and row from the top left, as seen.
    let cell = |sq: Square| {
        let (file, rank) = (f32::from(sq.file()), f32::from(sq.rank()));
        if style.flipped { (7.0 - file, rank) } else { (file, 7.0 - rank) }
    };
    let dark: Vec<Subpath> = (0..64)
        .filter_map(Square::from_index)
        .filter(|sq| !sq.is_light())
        .map(|sq| {
            let (col, row) = cell(sq);
            rect(origin + col * s, origin + row * s, s, s)
        })
        .collect();
    layers.push(Layer { subpaths: dark, paint: Paint::Fill { color: style.dark, even_odd: false } });

    for (sq, piece) in placement.pieces() {
        let (col, row) = cell(sq);
        let (dx, dy) = (origin + col * s, origin + row * s);
        for layer in &pieces.glyph(piece).layers {
            let paint = match layer.paint {
                Paint::Stroke { color, width } => Paint::Stroke { color, width: width * s },
                fill => fill,
            };
            let subpaths = layer.subpaths.iter().map(|p| place(p, dx, dy, s)).collect();
            layers.push(Layer { subpaths, paint });
        }
    }

    let mut labels = Vec::new();
    if style.coordinates {
        let size = 0.35 * s;
        for i in 0u8..8 {
            let file = if style.flipped { 7 - i } else { i };
            let rank = if style.flipped { i } else { 7 - i };
            let at = origin + (f32::from(i) + 0.5) * s;
            labels.push(Label { text: char::from(b'a' + file).to_string(), x: at, y: total - margin / 2.0, size, color: Rgb::BLACK });
            labels.push(Label { text: char::from(b'1' + rank).to_string(), x: margin / 2.0, y: at, size, color: Rgb::BLACK });
        }
    }
    Drawing { width: total, height: total, layers, labels }
}

/// A unit-square subpath scaled by `s` and moved to `(dx, dy)`.
fn place(p: &Subpath, dx: f32, dy: f32, s: f32) -> Subpath {
    let t = |x: f32, y: f32| (dx + x * s, dy + y * s);
    Subpath {
        start: t(p.start.0, p.start.1),
        segments: p
            .segments
            .iter()
            .map(|seg| match *seg {
                Segment::Line(x, y) => {
                    let (x, y) = t(x, y);
                    Segment::Line(x, y)
                }
                Segment::Cubic(x1, y1, x2, y2, x, y) => {
                    let ((x1, y1), (x2, y2), (x, y)) = (t(x1, y1), t(x2, y2), t(x, y));
                    Segment::Cubic(x1, y1, x2, y2, x, y)
                }
            })
            .collect(),
        closed: p.closed,
    }
}

/// The bounding box of some subpaths' points (control points included): `[x0, y0, x1, y1]`.
pub fn bounds(subpaths: &[Subpath]) -> Option<[f32; 4]> {
    let mut b: Option<[f32; 4]> = None;
    let mut add = |x: f32, y: f32| {
        b = Some(match b {
            None => [x, y, x, y],
            Some([x0, y0, x1, y1]) => [x0.min(x), y0.min(y), x1.max(x), y1.max(y)],
        });
    };
    for p in subpaths {
        add(p.start.0, p.start.1);
        for seg in &p.segments {
            match *seg {
                Segment::Line(x, y) => add(x, y),
                Segment::Cubic(x1, y1, x2, y2, x, y) => {
                    add(x1, y1);
                    add(x2, y2);
                    add(x, y);
                }
            }
        }
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fen::parse_placement;

    #[test]
    fn the_board_has_its_squares() {
        let d = board(&Placement::empty(), &PieceSet::geometric(), &BoardStyle { border: 0.0, ..BoardStyle::default() });
        assert_eq!(d.width, 160.0);
        // Light background, then the 32 dark squares in one layer.
        assert_eq!(d.layers.len(), 2);
        assert_eq!(d.layers[1].subpaths.len(), 32);
        assert!(d.labels.is_empty());
    }

    #[test]
    fn pieces_stay_inside_their_squares() {
        let style = BoardStyle { border: 0.0, ..BoardStyle::default() };
        for flipped in [false, true] {
            let style = BoardStyle { flipped, ..style };
            let d = board(&parse_placement("4k3/8/8/8/8/8/8/4K3").unwrap(), &PieceSet::geometric(), &style);
            // Two pieces, two layers each, after the two board layers.
            assert_eq!(d.layers.len(), 6);
            let white_king = bounds(&d.layers[2].subpaths).unwrap();
            // e1 is column 4, bottom row when White is below; column 3, top row when flipped.
            let (col, row) = if flipped { (3.0, 0.0) } else { (4.0, 7.0) };
            assert!(white_king[0] >= col * 20.0 && white_king[2] <= (col + 1.0) * 20.0, "{white_king:?}");
            assert!(white_king[1] >= row * 20.0 && white_king[3] <= (row + 1.0) * 20.0, "{white_king:?}");
        }
    }

    #[test]
    fn a1_is_dark_and_drawn_bottom_left() {
        let d = board(&Placement::empty(), &PieceSet::geometric(), &BoardStyle { border: 0.0, ..BoardStyle::default() });
        let a1 = [0.0, 140.0, 20.0, 160.0];
        assert!(d.layers[1].subpaths.iter().any(|p| bounds(std::slice::from_ref(p)) == Some(a1)));
    }

    #[test]
    fn coordinates_and_border_grow_the_drawing() {
        let style = BoardStyle { coordinates: true, ..BoardStyle::default() };
        let d = board(&Placement::empty(), &PieceSet::geometric(), &style);
        assert_eq!(d.labels.len(), 16);
        assert!(d.width > 160.0);
        assert_eq!(d.labels[0].text, "a");
        let flipped = board(&Placement::empty(), &PieceSet::geometric(), &BoardStyle { flipped: true, ..style });
        assert_eq!(flipped.labels[0].text, "h");
    }

    #[test]
    fn bad_sizes_fall_back() {
        for square in [f32::NAN, f32::INFINITY, 0.0, -3.0] {
            let d = board(&Placement::standard(), &PieceSet::geometric(), &BoardStyle { square, border: f32::NAN, ..BoardStyle::default() });
            assert!(d.width.is_finite() && d.width > 0.0);
        }
    }

    #[test]
    fn every_piece_has_a_distinct_glyph() {
        let set = PieceSet::geometric();
        for (i, a) in set.glyphs.iter().enumerate() {
            assert!(!a.layers.is_empty());
            for b in set.glyphs.iter().skip(i + 1) {
                assert_ne!(a, b);
            }
        }
    }
}
