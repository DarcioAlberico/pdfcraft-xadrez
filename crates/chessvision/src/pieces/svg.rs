//! Piece images drawn as SVG: their own paths, kept as they are.
//!
//! An SVG piece is already vector art, so nothing is traced: `usvg` resolves the file (styles,
//! `use`, transforms, shapes as paths) and each visible path becomes a layer, its fill and then
//! its stroke in the file's order (or the other way round where `paint-order` says so). The
//! image's own size maps to the unit square. A gradient is drawn in its first stop's colour and a
//! pattern in gray: diagrams are flat.

use pdfcraft_chess::draw::{Layer, Paint, PieceGlyph, Rgb, Segment, Subpath};
use usvg::tiny_skia_path::{PathSegment, Transform};

/// Most path segments kept for one piece.
const MAX_SEGMENTS: usize = 20_000;

/// A piece glyph from an SVG file's bytes.
pub(crate) fn glyph_from_svg(bytes: &[u8]) -> Result<PieceGlyph, String> {
    let tree = usvg::Tree::from_data(bytes, &usvg::Options::default()).map_err(|e| e.to_string())?;
    let size = tree.size();
    let (w, h) = (size.width(), size.height());
    if !(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0) {
        return Err("the image has no size".into());
    }
    let mut layers = Vec::new();
    let mut budget = MAX_SEGMENTS;
    collect(tree.root(), (w, h), &mut layers, &mut budget);
    if layers.is_empty() {
        return Err("the image draws nothing".into());
    }
    Ok(PieceGlyph { layers })
}

fn collect(group: &usvg::Group, size: (f32, f32), layers: &mut Vec<Layer>, budget: &mut usize) {
    for node in group.children() {
        match node {
            usvg::Node::Group(g) => collect(g, size, layers, budget),
            usvg::Node::Path(p) if p.is_visible() => {
                let t = p.abs_transform();
                let subpaths = subpaths(p.data(), t, size, budget);
                if subpaths.is_empty() {
                    continue;
                }
                let fill = p.fill().filter(|f| f.opacity().get() > 0.0).map(|f| Layer {
                    subpaths: subpaths.clone(),
                    paint: Paint::Fill { color: color(f.paint()), even_odd: f.rule() == usvg::FillRule::EvenOdd },
                });
                let stroke = p.stroke().filter(|s| s.opacity().get() > 0.0).map(|s| {
                    // A stroke's width scales with the path: by the transform's mean scale, then
                    // to the unit square.
                    let scale = (t.sx * t.sy - t.kx * t.ky).abs().sqrt();
                    let width = s.width().get() * scale / size.0.max(size.1);
                    Layer { subpaths: subpaths.clone(), paint: Paint::Stroke { color: color(s.paint()), width } }
                });
                let (first, second) = match p.paint_order() {
                    usvg::PaintOrder::FillAndStroke => (fill, stroke),
                    usvg::PaintOrder::StrokeAndFill => (stroke, fill),
                };
                layers.extend(first);
                layers.extend(second);
            }
            _ => {}
        }
    }
}

fn color(paint: &usvg::Paint) -> Rgb {
    match paint {
        usvg::Paint::Color(c) => Rgb(c.red, c.green, c.blue),
        usvg::Paint::LinearGradient(g) => g.stops().first().map_or(Rgb::BLACK, |s| Rgb(s.color().red, s.color().green, s.color().blue)),
        usvg::Paint::RadialGradient(g) => g.stops().first().map_or(Rgb::BLACK, |s| Rgb(s.color().red, s.color().green, s.color().blue)),
        usvg::Paint::Pattern(_) => Rgb(128, 128, 128),
    }
}

/// The path's subpaths in the unit square (y down, as SVG), its quadratic curves as cubics.
fn subpaths(path: &usvg::tiny_skia_path::Path, t: Transform, (w, h): (f32, f32), budget: &mut usize) -> Vec<Subpath> {
    let map = |x: f32, y: f32| ((t.sx * x + t.kx * y + t.tx) / w, (t.ky * x + t.sy * y + t.ty) / h);
    let mut out: Vec<Subpath> = Vec::new();
    let mut current: Option<Subpath> = None;
    // The last point in the file's own coordinates, for turning quads into cubics.
    let mut last = (0.0_f32, 0.0_f32);
    for seg in path.segments() {
        if *budget == 0 {
            break;
        }
        *budget -= 1;
        match seg {
            PathSegment::MoveTo(p) => {
                out.extend(current.take());
                current = Some(Subpath { start: map(p.x, p.y), segments: Vec::new(), closed: false });
                last = (p.x, p.y);
            }
            PathSegment::LineTo(p) => {
                let (x, y) = map(p.x, p.y);
                current
                    .get_or_insert_with(|| Subpath { start: map(last.0, last.1), segments: Vec::new(), closed: false })
                    .segments
                    .push(Segment::Line(x, y));
                last = (p.x, p.y);
            }
            PathSegment::QuadTo(c, p) => {
                // The cubic with the same curve: control points two thirds of the way to the quad's.
                let c1 = (last.0 + 2.0 / 3.0 * (c.x - last.0), last.1 + 2.0 / 3.0 * (c.y - last.1));
                let c2 = (p.x + 2.0 / 3.0 * (c.x - p.x), p.y + 2.0 / 3.0 * (c.y - p.y));
                let ((ax, ay), (bx, by), (x, y)) = (map(c1.0, c1.1), map(c2.0, c2.1), map(p.x, p.y));
                current
                    .get_or_insert_with(|| Subpath { start: map(last.0, last.1), segments: Vec::new(), closed: false })
                    .segments
                    .push(Segment::Cubic(ax, ay, bx, by, x, y));
                last = (p.x, p.y);
            }
            PathSegment::CubicTo(c1, c2, p) => {
                let ((ax, ay), (bx, by), (x, y)) = (map(c1.x, c1.y), map(c2.x, c2.y), map(p.x, p.y));
                current
                    .get_or_insert_with(|| Subpath { start: map(last.0, last.1), segments: Vec::new(), closed: false })
                    .segments
                    .push(Segment::Cubic(ax, ay, bx, by, x, y));
                last = (p.x, p.y);
            }
            PathSegment::Close => {
                if let Some(mut s) = current.take() {
                    s.closed = true;
                    out.push(s);
                }
            }
        }
    }
    out.extend(current);
    out.retain(|s| !s.segments.is_empty());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_filled_and_stroked_shape_gives_two_layers_in_the_unit_square() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="45" height="45" viewBox="0 0 45 45">
            <path d="M 9 36 L 36 36 L 22.5 9 Z" fill="#ffffff" stroke="#000000" stroke-width="1.5"/>
        </svg>"##;
        let g = glyph_from_svg(svg).unwrap();
        assert_eq!(g.layers.len(), 2);
        assert_eq!(g.layers[0].paint, Paint::Fill { color: Rgb::WHITE, even_odd: false });
        let Paint::Stroke { color, width } = g.layers[1].paint else { panic!("{:?}", g.layers[1].paint) };
        assert_eq!(color, Rgb::BLACK);
        assert!((width - 1.5 / 45.0).abs() < 1e-4, "{width}");
        let b = pdfcraft_chess::draw::bounds(&g.layers[0].subpaths).unwrap();
        for (got, want) in b.iter().zip([0.2, 0.2, 0.8, 0.8]) {
            assert!((got - want).abs() < 1e-4, "{b:?}");
        }
        assert!(g.layers[0].subpaths[0].closed);
    }

    #[test]
    fn groups_transforms_rules_and_paint_order_are_kept() {
        // A viewBox twice the size, a translated group, an even-odd ring, a stroke painted first.
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="45" height="45" viewBox="0 0 90 90">
            <g transform="translate(10 10)">
              <path d="M 0 0 H 70 V 70 H 0 Z M 20 20 H 50 V 50 H 20 Z" fill="black" fill-rule="evenodd"/>
              <circle cx="35" cy="35" r="5" fill="red" stroke="blue" stroke-width="2" paint-order="stroke"/>
            </g>
        </svg>"##;
        let g = glyph_from_svg(svg).unwrap();
        assert_eq!(g.layers.len(), 3);
        assert_eq!(g.layers[0].paint, Paint::Fill { color: Rgb::BLACK, even_odd: true });
        assert_eq!(g.layers[0].subpaths.len(), 2);
        let b = pdfcraft_chess::draw::bounds(&g.layers[0].subpaths).unwrap();
        for (got, want) in b.iter().zip([10.0 / 90.0, 10.0 / 90.0, 80.0 / 90.0, 80.0 / 90.0]) {
            assert!((got - want).abs() < 1e-4, "{b:?}");
        }
        assert!(matches!(g.layers[1].paint, Paint::Stroke { color: Rgb(0, 0, 255), .. }), "{:?}", g.layers[1].paint);
        assert_eq!(g.layers[2].paint, Paint::Fill { color: Rgb(255, 0, 0), even_odd: false });
        // The circle's arcs are curves.
        assert!(g.layers[2].subpaths[0].segments.iter().any(|s| matches!(s, Segment::Cubic(..))));
    }

    #[test]
    fn hidden_empty_and_broken_images_are_refused() {
        let hidden = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><path d="M0 0 H10 V10 Z" fill="none"/></svg>"#;
        assert!(glyph_from_svg(hidden).is_err());
        assert!(glyph_from_svg(b"<svg").is_err());
        assert!(glyph_from_svg(b"not svg").is_err());
    }
}
