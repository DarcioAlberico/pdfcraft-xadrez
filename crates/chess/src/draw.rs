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

/// Diagonal lines (`/` as seen) across the square `square`, of side `s`: bands of a seventh of
/// the side apart, each a twentieth wide, cut to the square.
fn hatch(square: &Subpath, s: f32) -> Vec<Subpath> {
    let (x0, y0) = square.start;
    let corners = [(x0, y0), (x0 + s, y0), (x0 + s, y0 + s), (x0, y0 + s)];
    let (step, width) = (s / 7.0, s / 20.0);
    let mut out = Vec::new();
    // Bands of x + y between c and c + width, relative to the square's corner.
    for k in 0..15u8 {
        let c = x0 + y0 + f32::from(k) * step - width / 2.0;
        let band = clip(&clip(&corners, |(x, y)| x + y - c), |(x, y)| c + width - (x + y));
        if band.len() >= 3 {
            out.push(polygon(&band));
        }
    }
    out
}

/// The convex polygon `points` cut to where `inside` is not negative (Sutherland–Hodgman).
fn clip(points: &[(f32, f32)], inside: impl Fn((f32, f32)) -> f32) -> Vec<(f32, f32)> {
    let mut out = Vec::with_capacity(points.len() + 2);
    for (i, &p) in points.iter().enumerate() {
        let q = points.get((i + 1) % points.len()).copied().unwrap_or(p);
        let (dp, dq) = (inside(p), inside(q));
        if dp >= 0.0 {
            out.push(p);
        }
        if (dp >= 0.0) != (dq >= 0.0) {
            let t = dp / (dp - dq);
            out.push((p.0 + (q.0 - p.0) * t, p.1 + (q.1 - p.1) * t));
        }
    }
    out
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
    /// The dark squares hatched in diagonal lines of `dark` over `light`, as printed diagrams
    /// (plan HC5), instead of filled.
    pub hatched: bool,
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
            hatched: false,
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

/// A board drawn by [`board`] with `drawings` over it (plan HC7): a coloured square as a ring,
/// an arrow with its head, in the colours an analysis board uses. Paths have no opacity here, so
/// they are drawn solid, as books print them.
pub fn board_with(placement: &Placement, pieces: &PieceSet, style: &BoardStyle, drawings: &[crate::game::Drawing]) -> Drawing {
    let mut d = board(placement, pieces, style);
    let (s, origin) = geometry(style);
    let centre = |sq: Square| {
        let (file, rank) = (f32::from(sq.file()), f32::from(sq.rank()));
        let (col, row) = if style.flipped { (7.0 - file, rank) } else { (file, 7.0 - rank) };
        (origin + (col + 0.5) * s, origin + (row + 0.5) * s)
    };
    for mark in drawings {
        let color = mark_color(mark.color);
        let (ax, ay) = centre(mark.from);
        if !mark.is_arrow() {
            let (outer, inner) = (s * 0.47, s * 0.40);
            d.layers.push(Layer { subpaths: vec![circle(ax, ay, outer), circle(ax, ay, inner)], paint: Paint::Fill { color, even_odd: true } });
            continue;
        }
        let (bx, by) = centre(mark.to);
        let (dx, dy) = (bx - ax, by - ay);
        let len = (dx * dx + dy * dy).sqrt();
        if !(len.is_finite() && len > 0.0) {
            continue;
        }
        let (ux, uy) = (dx / len, dy / len);
        let (nx, ny) = (-uy, ux);
        let head = s * 0.42;
        let tip = (bx - ux * s * 0.12, by - uy * s * 0.12);
        let base = (tip.0 - ux * head, tip.1 - uy * head);
        let start = (ax + ux * s * 0.18, ay + uy * s * 0.18);
        let half = s * 0.085;
        let shaft = polygon(&[
            (start.0 + nx * half, start.1 + ny * half),
            (base.0 + nx * half, base.1 + ny * half),
            (base.0 - nx * half, base.1 - ny * half),
            (start.0 - nx * half, start.1 - ny * half),
        ]);
        let point = polygon(&[tip, (base.0 + nx * head * 0.55, base.1 + ny * head * 0.55), (base.0 - nx * head * 0.55, base.1 - ny * head * 0.55)]);
        d.layers.push(Layer { subpaths: vec![shaft, point], paint: Paint::Fill { color, even_odd: false } });
    }
    d
}

/// A drawing's colour on paper: the analysis board's, solid.
fn mark_color(c: crate::game::DrawColor) -> Rgb {
    use crate::game::DrawColor;
    match c {
        DrawColor::Green => Rgb(21, 120, 27),
        DrawColor::Red => Rgb(170, 30, 30),
        DrawColor::Blue => Rgb(0, 60, 160),
        DrawColor::Yellow => Rgb(230, 143, 0),
    }
}

/// `drawings` in words, for a diagram's text alternative: "green arrow from b5 to c7, red ring
/// on e4".
pub fn describe(drawings: &[crate::game::Drawing]) -> String {
    use crate::game::DrawColor;
    let words: Vec<String> = drawings
        .iter()
        .map(|m| {
            let colour = match m.color {
                DrawColor::Green => "green",
                DrawColor::Red => "red",
                DrawColor::Blue => "blue",
                DrawColor::Yellow => "yellow",
            };
            if m.is_arrow() { format!("{colour} arrow from {} to {}", m.from, m.to) } else { format!("{colour} ring on {}", m.from) }
        })
        .collect();
    words.join(", ")
}

/// A board's square size and where its squares start, as [`board`] lays them out.
fn geometry(style: &BoardStyle) -> (f32, f32) {
    let defaults = BoardStyle::default();
    let s = if style.square.is_finite() && style.square > 0.0 { style.square } else { defaults.square };
    let border = if style.border.is_finite() && style.border >= 0.0 { style.border.min(1.0) * s } else { 0.0 };
    let margin = if style.coordinates { LABEL_MARGIN * s } else { 0.0 };
    (s, margin + border)
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
    let dark = if style.hatched { dark.iter().flat_map(|sq| hatch(sq, s)).collect() } else { dark };
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

    /// Plan HC5: hatched dark squares are diagonal bands cut to their square, about a quarter of
    /// it inked, and the light squares untouched.
    #[test]
    fn hatched_dark_squares_are_bands_inside_their_squares() {
        let d = board(&Placement::empty(), &PieceSet::geometric(), &BoardStyle { border: 0.0, hatched: true, ..BoardStyle::default() });
        assert_eq!(d.layers.len(), 2);
        let bands = &d.layers[1].subpaths;
        assert!(bands.len() > 32 * 10, "{} bands", bands.len());
        let mut inked = 0.0;
        for band in bands {
            let b = bounds(std::slice::from_ref(band)).unwrap();
            // Each band stays in one 20-unit square.
            let (col, row) = ((b[0] / 20.0).floor(), (b[1] / 20.0).floor());
            assert!(b[2] <= (col + 1.0) * 20.0 + 1e-3 && b[3] <= (row + 1.0) * 20.0 + 1e-3, "{b:?}");
            assert!((col + row) % 2.0 == 1.0, "only dark squares: {b:?}");
            inked += area(band);
        }
        let share = inked / (32.0 * 400.0);
        assert!((0.15..0.45).contains(&share), "{share}");
    }

    /// The area of a closed polygon of lines.
    fn area(path: &Subpath) -> f32 {
        let mut points = vec![path.start];
        points.extend(path.segments.iter().filter_map(|s| if let Segment::Line(x, y) = s { Some((*x, *y)) } else { None }));
        let n = points.len();
        (0..n).map(|i| points[i].0 * points[(i + 1) % n].1 - points[(i + 1) % n].0 * points[i].1).sum::<f32>().abs() / 2.0
    }

    /// Plan HC7: drawings go over the board, a ring on its square and an arrow between the two
    /// squares' centres, inside the board; described in words for the text alternative.
    #[test]
    fn drawings_are_drawn_over_the_board_and_described() {
        use crate::game::{DrawColor, Drawing as Mark};
        let sq = |n: &str| Square::parse(n).unwrap();
        let marks = [Mark { from: sq("b5"), to: sq("c7"), color: DrawColor::Green }, Mark { from: sq("e4"), to: sq("e4"), color: DrawColor::Red }];
        let style = BoardStyle { border: 0.0, ..BoardStyle::default() };
        let plain = board(&Placement::empty(), &PieceSet::geometric(), &style);
        let d = board_with(&Placement::empty(), &PieceSet::geometric(), &style, &marks);
        assert_eq!(d.layers.len(), plain.layers.len() + 2);
        let arrow = bounds(&d.layers[2].subpaths).unwrap();
        // b5 is column 1, row 3; c7 column 2, row 1 (20-unit squares).
        assert!(arrow[0] >= 20.0 && arrow[2] <= 60.0 && arrow[1] >= 20.0 && arrow[3] <= 80.0, "{arrow:?}");
        let ring = bounds(&d.layers[3].subpaths).unwrap();
        assert!((ring[0] - 80.6).abs() < 0.1 && (ring[3] - 99.4).abs() < 0.1, "e4's square: {ring:?}");
        assert!(matches!(d.layers[3].paint, Paint::Fill { even_odd: true, .. }), "a ring, not a disc");
        assert_eq!(describe(&marks), "green arrow from b5 to c7, red ring on e4");
        // No drawings: the board as before.
        assert_eq!(board_with(&Placement::empty(), &PieceSet::geometric(), &style, &[]), plain);
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
