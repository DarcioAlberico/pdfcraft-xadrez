//! Piece images drawn as SVG: their own paths, kept as they are.
//!
//! An SVG piece is already vector art, so nothing is traced: `usvg` resolves the file (styles,
//! `use`, transforms, shapes as paths) and each visible path becomes a layer, its fill and then
//! its stroke in the file's order (or the other way round where `paint-order` says so). The
//! image's own size maps to the unit square. A gradient is drawn in its first stop's colour and a
//! pattern in gray: diagrams are flat.
//!
//! The bytes are untrusted (a folder the user points at, a set they import), so the file is read
//! from its own bytes alone ([`parse`]): no reference leaves it, a compressed (svgz) file is not
//! inflated, and neither its size nor its XML entities can grow it past fixed limits.

use pdfcraft_chess::draw::{Layer, Paint, PieceGlyph, Rgb, Segment, Subpath};
use usvg::tiny_skia_path::{PathSegment, Transform};

/// Most path segments kept for one piece.
const MAX_SEGMENTS: usize = 20_000;

/// Largest SVG read as a piece, in bytes (a set's pieces are a few kilobytes each).
const MAX_SVG_BYTES: usize = 8 << 20;

/// Most XML nodes read from one piece.
const MAX_NODES: u32 = 200_000;

/// Most entities one piece may declare (an exported file declares a handful of namespaces).
const MAX_ENTITIES: usize = 64;

/// Most bytes the file's own entities may add once expanded.
const MAX_ENTITY_BYTES: usize = 1 << 20;

/// A piece glyph from an SVG file's bytes.
pub(crate) fn glyph_from_svg(bytes: &[u8]) -> Result<PieceGlyph, String> {
    let tree = parse(bytes)?;
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

/// The SVG as a tree, from these bytes alone. `usvg::Tree::from_data` would inflate a compressed
/// file without a limit, expand entities with no limit on the total, and read any file an
/// `<image>` names; each is closed here before usvg sees the file.
fn parse(bytes: &[u8]) -> Result<usvg::Tree, String> {
    if bytes.len() > MAX_SVG_BYTES {
        return Err(format!("{} bytes, more than the {MAX_SVG_BYTES} accepted", bytes.len()));
    }
    if bytes.starts_with(&[0x1f, 0x8b]) {
        return Err("a compressed SVG (svgz) is not read".into());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "the SVG is not UTF-8 text".to_owned())?;
    check_entities(text)?;
    // A DTD is allowed: many exported files carry one, and its entities were bounded above.
    let xml = usvg::roxmltree::ParsingOptions { allow_dtd: true, nodes_limit: MAX_NODES };
    let doc = usvg::roxmltree::Document::parse_with_options(text, xml).map_err(|e| e.to_string())?;
    usvg::Tree::from_xmltree(&doc, &options()).map_err(|e| e.to_string())
}

/// usvg's options with no way out of the file: an `<image>` (or `feImage`) reference resolves to
/// nothing, whether a path or a `data:` URL. usvg's default reads any path that exists on disk.
/// A piece draws its paths only, so an image would be dropped anyway.
fn options() -> usvg::Options<'static> {
    usvg::Options {
        resources_dir: None,
        image_href_resolver: usvg::ImageHrefResolver { resolve_data: Box::new(|_, _, _| None), resolve_string: Box::new(|_, _| None) },
        ..usvg::Options::default()
    }
}

/// Refuses the XML entities that could make the file larger than it was read: a parameter or
/// external entity, one made of other entities, more than [`MAX_ENTITIES`], or expansions adding
/// more than [`MAX_ENTITY_BYTES`] in all. Every reference to a name is counted, wherever it
/// stands, so the bound is never below what the parser can expand.
fn check_entities(text: &str) -> Result<(), String> {
    let mut added = 0_usize;
    let mut declared = 0_usize;
    let mut rest = text;
    while let Some((_, decl)) = rest.split_once("<!ENTITY") {
        declared += 1;
        if declared > MAX_ENTITIES {
            return Err(format!("more than {MAX_ENTITIES} XML entities"));
        }
        let decl = decl.trim_start();
        if decl.starts_with('%') {
            return Err("a parameter entity is not read".into());
        }
        let (name, after) = decl.split_once(char::is_whitespace).ok_or("an XML entity without a value")?;
        let after = after.trim_start();
        let quote = after.chars().next().filter(|c| *c == '"' || *c == '\'').ok_or("an external XML entity is not read")?;
        let (value, tail) = after.strip_prefix(quote).and_then(|v| v.split_once(quote)).ok_or("an XML entity that is not closed")?;
        // Character references (`&#38;`) are one character each; a named one would nest.
        if value.split('&').skip(1).any(|r| !r.starts_with('#')) {
            return Err("an XML entity made of other entities is not read".into());
        }
        let uses = text.matches(&format!("&{name};")).count();
        added = added.saturating_add(uses.saturating_mul(value.len()));
        if added > MAX_ENTITY_BYTES {
            return Err(format!("XML entities that would add more than {MAX_ENTITY_BYTES} bytes"));
        }
        rest = tail;
    }
    Ok(())
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

    /// A piece that draws one black triangle, with `head` before it (a DOCTYPE) and `body` inside.
    fn piece(head: &str, body: &str) -> Vec<u8> {
        format!(
            r#"{head}<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="45" height="45">{body}<path d="M 9 36 L 36 36 L 22.5 9 Z" fill="black"/></svg>"#
        )
        .into_bytes()
    }

    #[test]
    fn no_image_reference_is_resolved_from_disk_or_a_data_url() {
        let dir = std::env::temp_dir().join(format!("pdfcraft-svg-href-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("p.png");
        let bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x06\0\0\0";
        std::fs::write(&png, bytes).unwrap();
        let href = png.to_string_lossy().replace('\\', "/");
        // usvg's default resolver reads this file; the piece's options resolve nothing.
        let opts = options();
        assert!((opts.image_href_resolver.resolve_string)(&href, &opts).is_none());
        let data = std::sync::Arc::new(bytes.to_vec());
        assert!((opts.image_href_resolver.resolve_data)("image/png", data, &opts).is_none());
        // The piece still draws its own paths.
        let g = glyph_from_svg(&piece("", &format!(r#"<image xlink:href="{href}" width="45" height="45"/>"#))).unwrap();
        assert_eq!(g.layers.len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_compressed_svg_is_not_inflated() {
        use std::io::Write;
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(&piece("", "")).unwrap();
        let err = glyph_from_svg(&gz.finish().unwrap()).unwrap_err();
        assert!(err.contains("svgz"), "{err}");
    }

    #[test]
    fn a_file_over_the_size_limit_is_refused() {
        let pad = format!("<!--{}-->", "x".repeat(MAX_SVG_BYTES));
        let err = glyph_from_svg(&piece("", &pad)).unwrap_err();
        assert!(err.contains("accepted"), "{err}");
    }

    #[test]
    fn entities_are_read_only_while_they_stay_small() {
        // A DOCTYPE without entities, and small namespace entities, as exported files carry.
        let public = r#"<!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.1//EN" "http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd">"#;
        assert!(glyph_from_svg(&piece(public, "")).is_ok());
        let small = r#"<!DOCTYPE svg [<!ENTITY ns_x "http://example.org/x"> <!ENTITY amp2 "&#38;">]>"#;
        assert!(glyph_from_svg(&piece(small, "<desc>&ns_x; &amp2;</desc>")).is_ok());
        // One entity used often enough to add more than the limit: the parser alone would take it.
        let big = format!(r#"<!DOCTYPE svg [<!ENTITY e "{}">]>"#, "x".repeat(4096));
        let err = glyph_from_svg(&piece(&big, &format!("<desc>{}</desc>", "&e;".repeat(300)))).unwrap_err();
        assert!(err.contains("would add"), "{err}");
        for (head, why) in [
            (r#"<!DOCTYPE svg [<!ENTITY a "x"> <!ENTITY b "&a;&a;">]>"#, "other entities"),
            (r#"<!DOCTYPE svg [<!ENTITY % p "x">]>"#, "parameter"),
            (r#"<!DOCTYPE svg [<!ENTITY f SYSTEM "file:///etc/passwd">]>"#, "external"),
        ] {
            let err = glyph_from_svg(&piece(head, "")).unwrap_err();
            assert!(err.contains(why), "{head}: {err}");
        }
        // Inside a whole piece a later quote would close it; at the end of the text, nothing does.
        let err = check_entities(r#"<!DOCTYPE svg [<!ENTITY u "never closed"#).unwrap_err();
        assert!(err.contains("not closed"), "{err}");
    }

    proptest::proptest! {
        #[test]
        fn any_bytes_are_read_or_refused(bytes in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..2048)) {
            let _ = glyph_from_svg(&bytes);
        }

        #[test]
        fn any_text_inside_a_piece_is_read_or_refused(head in "\\PC{0,200}", body in "\\PC{0,400}") {
            let _ = glyph_from_svg(&piece(&head, &body));
        }
    }

    #[test]
    fn hidden_empty_and_broken_images_are_refused() {
        let hidden = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><path d="M0 0 H10 V10 Z" fill="none"/></svg>"#;
        assert!(glyph_from_svg(hidden).is_err());
        assert!(glyph_from_svg(b"<svg").is_err());
        assert!(glyph_from_svg(b"not svg").is_err());
    }
}
