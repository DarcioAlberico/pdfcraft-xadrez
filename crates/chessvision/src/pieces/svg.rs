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
//!
//! usvg converts a file recursively: one level for each element nested in another, each `<use>`
//! and each resource a reference names (a clip path, mask, filter, marker or pattern), whose own
//! contents may name the next. A stack overflow is an abort no caller can catch, so before usvg
//! sees the file [`check_structure`] walks it without recursion and refuses one whose chain of
//! nesting and references is deeper than [`MAX_DEPTH`], loops back on itself, or would expand to
//! more than [`MAX_EXPANDED`] elements. On the desktop usvg then runs on a thread with a stack of
//! its own ([`READER_STACK`]), a margin for a shape of recursion the walk does not foresee; in the
//! browser there are no threads, and the walk alone protects.

use std::collections::HashMap;

use pdfcraft_chess::draw::{Layer, Paint, PieceGlyph, Rgb, Segment, Subpath};
use usvg::roxmltree;
use usvg::tiny_skia_path::{PathSegment, Transform};

/// Most path segments kept for one piece.
const MAX_SEGMENTS: usize = 20_000;

/// Largest SVG read as a piece, in bytes (a set's pieces are a few kilobytes each).
pub(crate) const MAX_SVG_BYTES: usize = 8 << 20;

/// Most XML nodes read from one piece.
const MAX_NODES: u32 = 200_000;

/// Most entities one piece may declare (an exported file declares a handful of namespaces).
const MAX_ENTITIES: usize = 64;

/// Most bytes the file's own entities may add once expanded.
const MAX_ENTITY_BYTES: usize = 1 << 20;

/// Deepest chain of elements usvg converts one inside another: by nesting, through `<use>`, or
/// through a reference to a resource. The shipped pieces measure 7 at most; 1,000 nested groups,
/// 300 chained `<use>`s, or 3,000 clip paths that each clip the next overflow a 1 MiB stack.
const MAX_DEPTH: usize = 32;

/// Most elements a piece may stand for once every `<use>` and every resource is counted where it
/// is used. The file's own nodes are bounded by [`MAX_NODES`], but usvg converts a clip path or
/// mask in bounding-box units again for every shape it clips, so a chain of them multiplies: four
/// levels of 20 took 6 s and 300 MB, and one more would take a hundred times that.
const MAX_EXPANDED: u64 = 200_000;

/// Most style-sheet rules that name a reference (an exported piece has a handful at most).
const MAX_STYLE_LINKS: usize = 64;

/// Most compound selectors in one selector of a style sheet (`svg .white path` has three). The
/// matcher, usvg's and this check's, recurses once per compound: `g + g + … + g` over as many
/// sibling groups would overflow the stack.
const MAX_SELECTOR_PARTS: usize = 8;

/// Most distinct paints and markers one element may inherit by reference.
const MAX_INHERITED: usize = 16;

/// Stack of the thread usvg converts a piece on, on the desktop: a margin far above what
/// [`MAX_DEPTH`] levels take, for a shape of recursion [`check_structure`] does not foresee.
#[cfg(not(target_arch = "wasm32"))]
const READER_STACK: usize = 16 << 20;

/// A piece glyph from an SVG file's bytes.
pub(crate) fn glyph_from_svg(bytes: &[u8]) -> Result<PieceGlyph, String> {
    on_own_stack(|| read_glyph(bytes))
}

/// `read` on a thread with [`READER_STACK`] of stack; on the calling thread if none can be had,
/// where [`check_structure`] still bounds the recursion.
#[cfg(not(target_arch = "wasm32"))]
fn on_own_stack<T: Send>(read: impl Fn() -> Result<T, String> + Sync) -> Result<T, String> {
    std::thread::scope(|scope| match std::thread::Builder::new().stack_size(READER_STACK).spawn_scoped(scope, &read) {
        Ok(reader) => reader.join().unwrap_or_else(|_| Err("the SVG reader stopped on this file".into())),
        Err(_) => read(),
    })
}

/// In the browser there are no threads: [`check_structure`] alone bounds the recursion.
#[cfg(target_arch = "wasm32")]
fn on_own_stack<T>(read: impl Fn() -> Result<T, String>) -> Result<T, String> {
    read()
}

/// [`glyph_from_svg`] on the calling thread.
fn read_glyph(bytes: &[u8]) -> Result<PieceGlyph, String> {
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
    let xml = roxmltree::ParsingOptions { allow_dtd: true, nodes_limit: MAX_NODES };
    let doc = roxmltree::Document::parse_with_options(text, xml).map_err(|e| e.to_string())?;
    check_structure(&doc)?;
    usvg::Tree::from_xmltree(&doc, &options()).map_err(|e| e.to_string())
}

const SVG_NS: &str = "http://www.w3.org/2000/svg";

/// The deepest chain of an SVG's nesting and references, as [`check_structure`] measures it.
#[cfg(test)]
pub(super) fn chain_depth(bytes: &[u8]) -> Result<usize, String> {
    let text = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
    let doc = roxmltree::Document::parse_with_options(text, roxmltree::ParsingOptions { allow_dtd: true, nodes_limit: MAX_NODES })
        .map_err(|e| e.to_string())?;
    check_structure(&doc)
}

/// Properties whose reference an element passes on to the elements inside it.
const INHERITED_LINKS: [&str; 6] = ["fill", "stroke", "marker", "marker-start", "marker-mid", "marker-end"];

/// Elements that draw with a paint or marker, so that usvg converts the pattern or marker it
/// names inside them.
const PAINTED: [&str; 10] = ["path", "rect", "circle", "ellipse", "line", "polyline", "polygon", "text", "tspan", "textPath"];

/// Where usvg goes from one element. `links[..plain]` are converted inside it: its child
/// elements, the resources it names, the paints and markers it draws with. For a `<use>`,
/// `links[plain..plain + used]` are the elements it copies and the rest the paints and markers the
/// copy inherits from it, which usvg reaches at the bottom of the copy at the deepest.
#[derive(Default)]
struct Links {
    links: Vec<u32>,
    plain: usize,
    used: usize,
}

impl Links {
    fn parts(&self) -> (&[u32], &[u32], &[u32]) {
        let (plain, rest) = self.links.split_at(self.plain.min(self.links.len()));
        let (used, inherits) = rest.split_at(self.used.min(rest.len()));
        (plain, used, inherits)
    }
}

fn too_deep() -> String {
    format!("the SVG is nested or chained too deeply (more than {MAX_DEPTH} levels)")
}

/// Refuses an SVG that would make usvg recurse deeper than [`MAX_DEPTH`], loop, or build more than
/// [`MAX_EXPANDED`] elements: measured on the XML, without recursion, before usvg sees it.
///
/// The chain counts each element nested in another and each reference usvg follows: `href` (a
/// `<use>`, a pattern or gradient template, a filter image; not a link's), and `url(#id)` in any
/// attribute, in a `style` attribute and in the rules of a `<style>` sheet, matched to the
/// elements as usvg matches them. A paint or marker is inherited, so it counts where it is drawn,
/// and a `<use>` passes its own to what it copies. Elements outside the SVG namespace are skipped,
/// as usvg skips them. Every count is an upper bound: an `id` given twice stands for both elements.
/// Returns the deepest chain.
fn check_structure(doc: &roxmltree::Document) -> Result<usize, String> {
    let nodes = doc.descendants().map(|n| n.id().get_usize()).max().map_or(0, |m| m.saturating_add(1));
    // The SVG elements in document order (a parent before its children), and each node's place.
    let mut index = vec![u32::MAX; nodes];
    let mut elements: Vec<roxmltree::Node> = Vec::new();
    for node in doc.descendants().filter(|n| n.is_element()) {
        let parent_skipped = node.parent_element().is_some_and(|p| index.get(p.id().get_usize()).is_none_or(|i| *i == u32::MAX));
        if parent_skipped || !matches!(node.tag_name().namespace(), None | Some(SVG_NS)) {
            continue;
        }
        if let (Some(slot), Ok(i)) = (index.get_mut(node.id().get_usize()), u32::try_from(elements.len())) {
            *slot = i;
            elements.push(node);
        }
    }
    let sheet = style_sheet(doc)?;
    let graph = links(&elements, &index, &sheet)?;
    measure(&graph)
}

/// The rules of the file's `<style>` sheets that name a reference (usvg reads every element named
/// `style`), after checking that no selector is long enough to make matching recurse deeply.
fn style_sheet<'a>(doc: &'a roxmltree::Document) -> Result<Vec<simplecss::Rule<'a>>, String> {
    let mut sheet = simplecss::StyleSheet::new();
    for style in doc.descendants().filter(|n| n.is_element() && n.tag_name().name() == "style") {
        for text in style.children().filter_map(|c| c.text()) {
            if longest_selector(text) > MAX_SELECTOR_PARTS {
                return Err(format!("a style rule of the SVG has more than {MAX_SELECTOR_PARTS} parts"));
            }
            sheet.parse_more(text);
        }
    }
    let rules: Vec<_> = sheet.rules.into_iter().filter(|r| r.declarations.iter().any(|d| !url_ids(d.value).is_empty())).collect();
    if rules.len() > MAX_STYLE_LINKS {
        return Err(format!("more than {MAX_STYLE_LINKS} style rules of the SVG name a reference"));
    }
    Ok(rules)
}

/// The most compound selectors in one selector of a style sheet's text, counted generously: each
/// run of characters between combinators (spaces, `>`, `+`, `~`) and commas, outside comments and
/// declaration blocks. Matching a selector recurses once per compound.
fn longest_selector(css: &str) -> usize {
    let (mut longest, mut parts, mut in_part, mut blocks) = (0_usize, 0_usize, false, 0_usize);
    let mut rest = css;
    while let Some(c) = rest.chars().next() {
        if rest.starts_with("/*") {
            rest = rest.get(2..).and_then(|r| r.split_once("*/")).map_or("", |(_, after)| after);
            in_part = false;
            continue;
        }
        rest = rest.get(c.len_utf8()..).unwrap_or_default();
        match c {
            '{' => {
                blocks = blocks.saturating_add(1);
                (parts, in_part) = (0, false);
            }
            '}' => {
                blocks = blocks.saturating_sub(1);
                (parts, in_part) = (0, false);
            }
            _ if blocks > 0 => {}
            ',' => (parts, in_part) = (0, false),
            c if c.is_whitespace() || matches!(c, '>' | '+' | '~') => in_part = false,
            _ if !in_part => {
                (parts, in_part) = (parts.saturating_add(1), true);
                longest = longest.max(parts);
            }
            _ => {}
        }
    }
    longest
}

/// For each element (as `index` places them), where usvg goes from it.
fn links(elements: &[roxmltree::Node], index: &[u32], rules: &[simplecss::Rule]) -> Result<Vec<Links>, String> {
    let at = |node: roxmltree::Node| index.get(node.id().get_usize()).copied().filter(|i| *i != u32::MAX);
    // An id names its first element (as `<use>` reads it) and its last (as a resource is read).
    // `xml:id` counts too, as usvg reads it.
    let mut ids: HashMap<&str, (u32, u32)> = HashMap::new();
    for (i, node) in (0_u32..).zip(elements) {
        for id in node.attributes().filter(|a| a.name() == "id") {
            ids.entry(id.value()).and_modify(|e| e.1 = i).or_insert((i, i));
        }
    }
    let targets = |id: &str| -> Vec<u32> {
        match ids.get(id) {
            Some(&(first, last)) if first == last => vec![first],
            Some(&(first, last)) => vec![first, last],
            None => Vec::new(),
        }
    };
    let mut graph: Vec<Links> = Vec::with_capacity(elements.len());
    let mut depth: Vec<usize> = Vec::with_capacity(elements.len());
    let mut inherited: Vec<Vec<u32>> = Vec::with_capacity(elements.len());
    for node in elements {
        let parent = node.parent_element().and_then(at).and_then(|p| usize::try_from(p).ok());
        let level = parent.and_then(|p| depth.get(p)).map_or(1, |d| d.saturating_add(1));
        if level > MAX_DEPTH {
            return Err(too_deep());
        }
        // The references this element names, each with the property that names it.
        let mut named: Vec<(&str, &str)> = Vec::new();
        let mut copied: Vec<u32> = Vec::new();
        let tag = node.tag_name().name();
        for attr in node.attributes() {
            match attr.name() {
                "style" => {
                    for d in simplecss::DeclarationTokenizer::from(attr.value()) {
                        named.extend(url_ids(d.value).into_iter().map(|id| (d.name, id)));
                    }
                }
                // A link's `href` is a hyperlink, nothing usvg converts.
                "href" if tag != "a" => copied.extend(href_id(attr.value()).map(targets).unwrap_or_default()),
                "href" => {}
                name => named.extend(url_ids(attr.value()).into_iter().map(|id| (name, id))),
            }
        }
        for rule in rules.iter().filter(|r| r.selector.matches(&XmlNode(*node))) {
            for d in &rule.declarations {
                named.extend(url_ids(d.value).into_iter().map(|id| (d.name, id)));
            }
        }
        let mut paints = parent.and_then(|p| inherited.get(p)).cloned().unwrap_or_default();
        let mut links = Links::default();
        for (property, id) in named {
            for t in targets(id) {
                if !INHERITED_LINKS.contains(&property) {
                    links.links.push(t);
                } else if !paints.contains(&t) {
                    paints.push(t);
                }
            }
        }
        if paints.len() > MAX_INHERITED {
            return Err(format!("more than {MAX_INHERITED} paints or markers reach one element of the SVG"));
        }
        links.links.extend(node.children().filter_map(at));
        if PAINTED.contains(&tag) {
            links.links.extend(&paints);
        }
        if tag == "use" {
            links.plain = links.links.len();
            links.used = copied.len();
            links.links.extend(copied);
            links.links.extend(&paints);
        } else {
            links.links.extend(copied);
            links.plain = links.links.len();
        }
        graph.push(links);
        depth.push(level);
        inherited.push(paints);
    }
    Ok(graph)
}

/// The longest chain and the expanded size from each element, depth first with an explicit
/// stack; an element met again while it is still open is a loop. Returns the longest chain.
fn measure(graph: &[Links]) -> Result<usize, String> {
    const NEW: u8 = 0;
    const OPEN: u8 = 1;
    const DONE: u8 = 2;
    let mut state = vec![NEW; graph.len()];
    let mut height = vec![0_usize; graph.len()];
    let mut size = vec![0_u64; graph.len()];
    let at = |i: u32| usize::try_from(i).unwrap_or(usize::MAX);
    let mut deepest = 0;
    for start in 0..graph.len() {
        if state.get(start) != Some(&NEW) {
            continue;
        }
        let mut stack: Vec<(usize, usize)> = vec![(start, 0)];
        if let Some(s) = state.get_mut(start) {
            *s = OPEN;
        }
        while let Some(&mut (v, ref mut next)) = stack.last_mut() {
            let Some(links) = graph.get(v) else { break };
            if let Some(w) = links.links.get(*next).map(|w| at(*w)) {
                *next = next.saturating_add(1);
                match state.get(w) {
                    Some(&NEW) => {
                        if stack.len() >= MAX_DEPTH {
                            return Err(too_deep());
                        }
                        if let Some(s) = state.get_mut(w) {
                            *s = OPEN;
                        }
                        stack.push((w, 0));
                    }
                    Some(&OPEN) => return Err("the SVG refers back to itself (a reference loop)".into()),
                    _ => {}
                }
                continue;
            }
            let (plain, used, inherits) = links.parts();
            let tallest = |set: &[u32]| set.iter().filter_map(|w| height.get(at(*w))).copied().max().unwrap_or(0);
            let total = |set: &[u32]| set.iter().filter_map(|w| size.get(at(*w))).fold(0_u64, |s, n| s.saturating_add(*n));
            // A copy reaches what its `<use>` passes on at its own bottom at the deepest, and may
            // reach it from every element it holds.
            let copy = if used.is_empty() { 0 } else { tallest(used).saturating_add(tallest(inherits)) };
            let h = tallest(plain).max(copy).saturating_add(1);
            let n = total(plain).saturating_add(total(used).saturating_mul(total(inherits).saturating_add(1))).saturating_add(1);
            if h > MAX_DEPTH {
                return Err(too_deep());
            }
            if n > MAX_EXPANDED {
                return Err(format!("the SVG would expand to more than {MAX_EXPANDED} elements"));
            }
            if let (Some(hv), Some(sv), Some(s)) = (height.get_mut(v), size.get_mut(v), state.get_mut(v)) {
                (*hv, *sv, *s) = (h, n, DONE);
            }
            deepest = deepest.max(h);
            stack.pop();
        }
    }
    Ok(deepest)
}

/// The id an `href` names in the file (`#id`), read as usvg reads it.
fn href_id(value: &str) -> Option<&str> {
    let id = value.trim_start_matches(is_space).strip_prefix('#')?;
    id.split(' ').next().filter(|id| !id.is_empty())
}

/// The ids of the local references in a property's value: `url(#id)`, `url("#id")` or
/// `url('#id')`, read as usvg reads them, wherever they stand in the value.
fn url_ids(value: &str) -> Vec<&str> {
    let mut ids = Vec::new();
    let mut rest = value;
    while let Some(at) = rest.find("url(") {
        rest = rest.get(at.saturating_add("url(".len())..).unwrap_or_default();
        let inner = rest.trim_start_matches(is_space);
        let id = match inner.chars().next() {
            Some(quote @ ('"' | '\'')) => {
                inner.get(1..).and_then(|s| s.trim_start_matches(is_space).strip_prefix('#')).map(|s| s.split(quote).next().unwrap_or(s).trim_end())
            }
            _ => inner.strip_prefix('#').map(|s| s.split([' ', ')']).next().unwrap_or(s)),
        };
        ids.extend(id.filter(|id| !id.is_empty()));
    }
    ids
}

fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r')
}

/// An element as the style sheet's selectors see it: the matching usvg does.
struct XmlNode<'a, 'input>(roxmltree::Node<'a, 'input>);

impl simplecss::Element for XmlNode<'_, '_> {
    fn parent_element(&self) -> Option<Self> {
        self.0.parent_element().map(XmlNode)
    }

    fn prev_sibling_element(&self) -> Option<Self> {
        self.0.prev_sibling_element().map(XmlNode)
    }

    fn has_local_name(&self, name: &str) -> bool {
        self.0.tag_name().name() == name
    }

    fn attribute_matches(&self, name: &str, operator: simplecss::AttributeOperator<'_>) -> bool {
        self.0.attribute(name).is_some_and(|v| operator.matches(v))
    }

    fn pseudo_class_matches(&self, class: simplecss::PseudoClass<'_>) -> bool {
        matches!(class, simplecss::PseudoClass::FirstChild) && self.0.prev_sibling_element().is_none()
    }
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

/// The visible paths under `root`, in the file's order, as layers. Without recursion: a stack of
/// the groups being read.
fn collect(root: &usvg::Group, size: (f32, f32), layers: &mut Vec<Layer>, budget: &mut usize) {
    let mut open = vec![root.children().iter()];
    while let Some(group) = open.last_mut() {
        let Some(node) = group.next() else {
            open.pop();
            continue;
        };
        match node {
            usvg::Node::Group(g) => open.push(g.children().iter()),
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

    /// A piece's whole text, `body` inside an `<svg>` that declares the xlink namespace.
    fn whole(body: &str) -> String {
        format!(r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="45" height="45">{body}</svg>"#)
    }

    const TRIANGLE: &str = r#"d="M 9 36 L 36 36 L 22.5 9 Z""#;

    /// A triangle inside `n` nested groups (A3-01).
    fn nested(n: usize) -> String {
        whole(&format!(r#"{}<path {TRIANGLE} fill="black"/>{}"#, r#"<g opacity="0.99">"#.repeat(n), "</g>".repeat(n)))
    }

    /// A triangle reached through `n` chained `<use>`s, each one element deep (R2-01).
    fn use_chain(n: usize) -> String {
        let uses: String = (1..=n).map(|i| format!(r##"<use id="u{i}" xlink:href="#u{}"/>"##, i - 1)).collect();
        whole(&format!(r##"<defs><path id="u0" {TRIANGLE} fill="black"/>{uses}</defs><use xlink:href="#u{n}"/>"##))
    }

    /// A triangle clipped (or masked) by `n` resources, each clipped by the one before (R2-01).
    fn resource_chain(n: usize, kind: &str, property: &str) -> String {
        let chain: String =
            (1..=n).map(|i| format!(r#"<{kind} id="r{i}" {property}="url(#r{})"><path {TRIANGLE} fill="white"/></{kind}>"#, i - 1)).collect();
        whole(&format!(
            r#"<defs><{kind} id="r0"><path {TRIANGLE} fill="white"/></{kind}>{chain}</defs><path {TRIANGLE} fill="black" {property}="url(#r{n})"/>"#
        ))
    }

    /// The clip-path chain of [`resource_chain`] named by a style sheet instead of attributes.
    fn styled_clip_chain(n: usize) -> String {
        let rules: String = (1..=n).map(|i| format!("#c{i}{{clip-path:url(#c{})}}", i - 1)).collect();
        let clips: String = (0..=n).map(|i| format!(r#"<clipPath id="c{i}"><path {TRIANGLE}/></clipPath>"#)).collect();
        whole(&format!(r#"<style>{rules}</style><defs>{clips}</defs><path {TRIANGLE} fill="black" style="clip-path:url(#c{n})"/>"#))
    }

    /// `<use>`s of `fan` copies of the level below, `levels` deep: `fan^levels` triangles.
    fn use_fan(fan: usize, levels: usize) -> String {
        let groups: String = (1..=levels)
            .map(|level| format!(r#"<g id="a{level}">{}</g>"#, format!(r##"<use xlink:href="#a{}"/>"##, level - 1).repeat(fan)))
            .collect();
        whole(&format!(r##"<defs><path id="a0" {TRIANGLE} fill="black"/>{groups}</defs><use xlink:href="#a{levels}"/>"##))
    }

    /// Clip paths in bounding-box units, which usvg converts again for every shape they clip, each
    /// holding `fan` shapes clipped by the one below, `levels` deep: `fan^levels` shapes converted.
    fn clip_fan(fan: usize, levels: usize) -> String {
        let clip = |level: usize, inside: String| format!(r#"<clipPath id="k{level}" clipPathUnits="objectBoundingBox">{inside}</clipPath>"#);
        let clips: String = (1..=levels)
            .map(|level| clip(level, format!(r#"<rect width="0.5" height="0.5" clip-path="url(#k{})"/>"#, level - 1).repeat(fan)))
            .collect();
        let first = clip(0, r#"<rect width="0.5" height="0.5"/>"#.to_owned());
        whole(&format!(r#"<defs>{first}{clips}</defs><path {TRIANGLE} fill="black" clip-path="url(#k{levels})"/>"#))
    }

    /// [`read_glyph`] (the check, without the desktop's own thread) on a thread with 1 MiB of
    /// stack, the main thread's on Windows.
    fn read_on_small_stack(svg: String) -> Result<PieceGlyph, String> {
        std::thread::Builder::new().stack_size(1 << 20).spawn(move || read_glyph(svg.as_bytes())).unwrap().join().unwrap()
    }

    /// Plan A9-V1 (A3-01, R2-01): each of these aborted the process with a stack overflow inside
    /// usvg; the check refuses them before usvg recurses, on a stack the size of the main thread's.
    #[test]
    fn hostile_nesting_and_reference_chains_are_refused_before_usvg_recurses() {
        let three_clips_in_a_loop = whole(&format!(
            r#"<defs>{}</defs><path {TRIANGLE} fill="black" clip-path="url(#c1)"/>"#,
            [(1, 2), (2, 3), (3, 1)]
                .map(|(i, next)| format!(r#"<clipPath id="c{i}" clip-path="url(#c{next})"><path {TRIANGLE}/></clipPath>"#))
                .concat()
        ));
        let n = 20_000;
        let sibling_selector = whole(&format!(r#"<style>{}g{{fill:black}}</style>{}<path {TRIANGLE}/>"#, "g+".repeat(n), "<g/>".repeat(n)));
        for (name, svg, why) in [
            ("1,000 nested groups", nested(1000), "too deeply"),
            ("300 chained uses", use_chain(300), "too deeply"),
            ("3,000 chained clip paths", resource_chain(3000, "clipPath", "clip-path"), "too deeply"),
            ("3,000 chained masks", resource_chain(3000, "mask", "mask"), "too deeply"),
            ("3,000 clip paths chained by a style sheet", styled_clip_chain(3000), "style rules"),
            ("three clip paths in a loop", three_clips_in_a_loop, "loop"),
            ("a 20,000-part sibling selector", sibling_selector, "parts"),
            ("20^4 clip paths converted", clip_fan(20, 4), "expand"),
        ] {
            let err = read_on_small_stack(svg).err().unwrap_or_else(|| panic!("{name}: read"));
            assert!(err.contains(why), "{name}: {err}");
        }
    }

    /// The chain counts every way usvg goes from one element into another, each kept short
    /// enough here to read without the check: a style sheet's references, a paint inherited by
    /// the shapes inside a pattern, a paint a `<use>` passes to what it copies, and groups nested
    /// inside each resource of a chain whose own length passes.
    #[test]
    fn every_reference_usvg_follows_counts_in_the_chain() {
        let inherited: String = (1..=40)
            .map(|i| format!(r#"<pattern id="p{i}" width="4" height="4" fill="url(#p{})"><rect width="2" height="2"/></pattern>"#, i - 1))
            .collect();
        let inherited = whole(&format!(
            r#"<defs><pattern id="p0" width="4" height="4"><rect width="2" height="2" fill="black"/></pattern>{inherited}</defs><path {TRIANGLE} fill="url(#p40)"/>"#
        ));
        let through_use: String = (1..=40)
            .map(|i| format!(r##"<pattern id="p{i}" width="4" height="4"><use xlink:href="#s" fill="url(#p{})"/></pattern>"##, i - 1))
            .collect();
        let through_use = whole(&format!(
            r#"<defs><rect id="s" width="2" height="2"/><pattern id="p0" width="4" height="4"><rect width="2" height="2" fill="black"/></pattern>{through_use}</defs><path {TRIANGLE} fill="url(#p40)"/>"#
        ));
        let groups = |inner: &str| format!("{}{inner}{}", "<g>".repeat(10), "</g>".repeat(10));
        let deep_clips: String = (1..=4)
            .map(|i| format!(r#"<clipPath id="c{i}">{}</clipPath>"#, groups(&format!(r#"<path {TRIANGLE} clip-path="url(#c{})"/>"#, i - 1))))
            .collect();
        let deep_clips = whole(&format!(
            r#"<defs><clipPath id="c0"><path {TRIANGLE}/></clipPath>{deep_clips}</defs><path {TRIANGLE} fill="black" clip-path="url(#c4)"/>"#
        ));
        for (name, svg) in [
            ("styled clip paths", styled_clip_chain(40)),
            ("inherited patterns", inherited),
            ("patterns through uses", through_use),
            ("deep clips", deep_clips),
        ] {
            let err = glyph_from_svg(svg.as_bytes()).err().unwrap_or_else(|| panic!("{name}: read"));
            assert!(err.contains("too deeply"), "{name}: {err}");
        }
        // Short chains, a style sheet's references and a `<use>` of a gradient-filled shape read.
        for svg in [
            nested(20),
            use_chain(20),
            resource_chain(20, "clipPath", "clip-path"),
            styled_clip_chain(20),
            use_fan(8, 3),
            whole(&format!(
                r##"<defs><linearGradient id="g"><stop offset="0" stop-color="red"/></linearGradient><path id="s" {TRIANGLE}/></defs><use xlink:href="#s" fill="url(#g)"/>"##
            )),
        ] {
            glyph_from_svg(svg.as_bytes()).unwrap();
        }
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

    /// A board whose pieces are `<use>`s of one `<symbol>` each (plan A8 P9b) draws the same paths
    /// as the board drawn in full, with every shipped set, flipped and with coordinates.
    #[test]
    fn symbols_draw_what_the_full_board_draws() {
        use pdfcraft_chess::draw::{BoardStyle, board};
        use pdfcraft_chess::svg::{UsedPieces, board_described, symbols, to_svg};
        let placement = pdfcraft_chess::fen::parse_placement("r1bqk2r/pp2bppp/2n1pn2/3p4/2PP4/2N2N2/PP2BPPP/R2QKB1R").unwrap();
        for set in crate::pieces::bundled::SETS {
            let pieces = set.load().unwrap();
            for style in [BoardStyle::default(), BoardStyle { coordinates: true, flipped: true, hatched: true, ..BoardStyle::default() }] {
                let full = glyph_from_svg(to_svg(&board(&placement, &pieces, &style), "full").as_bytes()).unwrap();
                let mut used = UsedPieces::default();
                let svg = board_described(&placement, &pieces, &style, &mut used, "d", "t", "d");
                let defs = symbols(&pieces, &used);
                let (start, end) = (defs.find("<defs>").unwrap(), defs.rfind("</defs>").unwrap() + "</defs>".len());
                let one = svg.replacen("<title", &format!("{}<title", &defs[start..end]), 1);
                assert_eq!(one.matches("<use ").count(), placement.pieces().count());
                let drawn = glyph_from_svg(one.as_bytes()).unwrap();
                assert_eq!(drawn.layers.len(), full.layers.len(), "{}", set.id);
                for (a, b) in drawn.layers.iter().zip(&full.layers) {
                    match (a.paint, b.paint) {
                        (Paint::Fill { color: ca, even_odd: ea }, Paint::Fill { color: cb, even_odd: eb }) => assert_eq!((ca, ea), (cb, eb)),
                        (Paint::Stroke { color: ca, width: wa }, Paint::Stroke { color: cb, width: wb }) => {
                            assert_eq!(ca, cb);
                            assert!((wa - wb).abs() < 2e-4, "{}: width {wa} against {wb}", set.id);
                        }
                        (pa, pb) => panic!("{}: {pa:?} against {pb:?}", set.id),
                    }
                    let points = |l: &Layer| -> Vec<(f32, f32)> {
                        l.subpaths
                            .iter()
                            .flat_map(|p| {
                                std::iter::once(p.start).chain(p.segments.iter().map(|s| match *s {
                                    Segment::Line(x, y) | Segment::Cubic(_, _, _, _, x, y) => (x, y),
                                }))
                            })
                            .collect()
                    };
                    let (pa, pb) = (points(a), points(b));
                    assert_eq!(pa.len(), pb.len(), "{}", set.id);
                    for (p, q) in pa.iter().zip(&pb) {
                        assert!((p.0 - q.0).abs() < 2e-4 && (p.1 - q.1).abs() < 2e-4, "{}: {p:?} against {q:?}", set.id);
                    }
                }
            }
        }
    }
}
