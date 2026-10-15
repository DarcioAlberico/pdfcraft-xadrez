//! Vector path inventory: the paths a page paints, in PDF user space (ISO 32000-2 §8.5).
//!
//! [`page_paths`] interprets the page's content streams and the Form XObjects they draw
//! (§8.10), following `q`/`Q`, `cm` and each form's `/Matrix`, and reports one [`PathItem`]
//! per painting operator (`S`, `s`, `f`, `F`, `f*`, `B`, `B*`, `b`, `b*`). A path ended by `n`,
//! such as a clipping path, is not painted and is not reported. A board drawn as 64 `re f`
//! squares is 64 items; 32 `re` followed by one `f` is one item with 32 subpaths.
//!
//! The walk is read-only and lenient. Hard limits on items, segments, decoded bytes, streams,
//! undecodable streams, form nesting and `q` nesting stop it early and set [`PathInventory::truncated`]; what was
//! painted before the limit is kept. A content stream that cannot be decoded is skipped and
//! counted in [`PathInventory::unreadable`]. A form that draws itself, directly or through
//! other forms, is drawn once per chain: the recursive invocation is skipped.
//!
//! Not interpreted: clipping (boxes are not clipped), line width, caps and joins (a stroked
//! box is the path's geometric extent), optional-content visibility, annotation appearances,
//! Type 3 glyphs, patterns' own content and shadings. Colour is reported as set, not converted:
//! ICCBased spaces with 1, 3 or 4 components and CalGray/CalRGB report as the matching device
//! family; other spaces report their raw components.
//!
//! The design follows the snapping extractor in `pdfcraft-measure` (`snap.rs`), which cannot
//! share this code: `measure → edit` is a same-layer edge the layering does not allow.

use std::collections::HashSet;

use kurbo::{CubicBez, ParamCurveExtrema};
use pdfcraft_content::{Matrix, Op, parse};
use pdfcraft_cos::{Dict, Document, ObjRef, Object};

use crate::EditError;

/// Painted items reported per page.
pub const MAX_ITEMS: usize = 20_000;
/// Path segments reported per page, counting the path being built.
pub const MAX_SEGMENTS: usize = 100_000;
/// Decoded content bytes interpreted per page, across the page and every form it draws. A
/// single stream that decodes to more than this is counted as unreadable.
pub const MAX_BYTES: usize = 16 * 1024 * 1024;
/// Content streams interpreted per page, counting each form drawing and each `/Contents` part.
pub const MAX_STREAMS: usize = 4_096;
/// Undecodable streams skipped before the walk stops. A failed decode can cost up to
/// [`MAX_BYTES`] of work that the byte budget never sees, so failures have their own bound.
pub const MAX_UNREADABLE: usize = 16;
/// Nested Form XObjects below the page.
pub const MAX_FORM_DEPTH: usize = 32;
/// Nested `q` within one content stream.
pub const MAX_STATE_DEPTH: usize = 256;
/// Colour components kept for a colour in an unconverted space (DeviceN allows 32).
const MAX_COMPONENTS: usize = 32;

/// A point in PDF user space.
pub type Point = [f64; 2];

/// A fill or stroke colour as the content stream set it.
#[derive(Clone, Debug, PartialEq)]
pub enum PathColor {
    Gray(f64),
    Rgb([f64; 3]),
    Cmyk([f64; 4]),
    /// Separation, DeviceN, Indexed, Lab, Pattern or an unresolved space: the numeric
    /// components as given (at most 32), not converted. Empty for a pattern without tints.
    Other(Vec<f64>),
}

/// A segment from the previous point to the last point given, in user space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathSegment {
    Line(Point),
    /// Two control points, then the end point.
    Cubic(Point, Point, Point),
}

/// A connected run of segments. `closed` means it was closed explicitly (`h`, `re`, `s`,
/// `b`, `b*`): the closing edge back to `start` is implicit and not among `segments`. Fills
/// also close open subpaths implicitly; that is not recorded here. A subpath with no segments
/// (a lone `m`) is not reported.
#[derive(Clone, Debug, PartialEq)]
pub struct Subpath {
    pub start: Point,
    pub segments: Vec<PathSegment>,
    pub closed: bool,
}

/// One painted path.
#[derive(Clone, Debug, PartialEq)]
pub struct PathItem {
    /// `[x0, y0, x1, y1]` in user space after the CTM: the exact extent of the segments,
    /// curves included, without line width or clipping.
    pub bbox_user: [f64; 4],
    /// The fill colour, when the operator fills.
    pub fill: Option<PathColor>,
    /// The stroke colour, when the operator strokes.
    pub stroke: Option<PathColor>,
    pub subpaths: Vec<Subpath>,
}

/// Every path a page paints, in painting order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PathInventory {
    pub items: Vec<PathItem>,
    /// Segments across `items`.
    pub segments: usize,
    /// A limit stopped the walk early; `items` holds what was painted before it.
    pub truncated: bool,
    /// Content streams that could not be decoded and were skipped (each counted once).
    pub unreadable: usize,
}

/// The paths painted on `page` (zero-based). Only a missing page is an error; limits and
/// unreadable streams are reported in the inventory.
pub fn page_paths(doc: &Document, page: usize) -> Result<PathInventory, EditError> {
    let p = pdfcraft_model::pages(doc).into_iter().nth(page).ok_or(EditError::NoSuchPage(page))?;
    let resources = p.dict.get(b"Resources").map(|o| doc.resolve(o));
    let empty = Dict::new();
    let resources = resources.as_deref().and_then(Object::as_dict).unwrap_or(&empty);
    let mut walker = Walker { doc, out: PathInventory::default(), active: HashSet::new(), failed: HashSet::new(), bytes: 0, streams: 0 };
    if let Some(contents) = p.dict.get(b"Contents") {
        walker.walk(contents, resources, State::default(), 0);
    }
    Ok(walker.out)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Space {
    Gray,
    Rgb,
    Cmyk,
    Other,
}

#[derive(Clone, Debug)]
struct Paint {
    space: Space,
    color: PathColor,
}

impl Paint {
    /// The initial colour a `cs`/`CS` selects (§8.6.8, Table 73).
    fn initial(space: Space) -> Self {
        let color = match space {
            Space::Gray => PathColor::Gray(0.0),
            Space::Rgb => PathColor::Rgb([0.0; 3]),
            Space::Cmyk => PathColor::Cmyk([0.0, 0.0, 0.0, 1.0]),
            Space::Other => PathColor::Other(Vec::new()),
        };
        Self { space, color }
    }

    /// `sc`/`scn`: components in the current space; a mismatched count keeps them raw.
    fn set(&mut self, op: &Op) {
        let c: Vec<f64> = op.operands.iter().filter_map(Object::as_f64).filter(|v| v.is_finite()).take(MAX_COMPONENTS).collect();
        self.color = match (self.space, c.as_slice()) {
            (Space::Gray, &[g]) => PathColor::Gray(g),
            (Space::Rgb, &[r, g, b]) => PathColor::Rgb([r, g, b]),
            (Space::Cmyk, &[c, m, y, k]) => PathColor::Cmyk([c, m, y, k]),
            _ => PathColor::Other(c),
        };
    }
}

/// The graphics state this inventory needs. Forms start from the state at their `Do`.
#[derive(Clone, Debug)]
struct State {
    ctm: Matrix,
    fill: Paint,
    stroke: Paint,
}

impl Default for State {
    fn default() -> Self {
        Self { ctm: Matrix::IDENTITY, fill: Paint::initial(Space::Gray), stroke: Paint::initial(Space::Gray) }
    }
}

fn device_space(name: &[u8]) -> Option<Space> {
    match name {
        b"DeviceGray" | b"CalGray" => Some(Space::Gray),
        b"DeviceRGB" | b"CalRGB" => Some(Space::Rgb),
        b"DeviceCMYK" => Some(Space::Cmyk),
        _ => None,
    }
}

/// The family of a colour space operand, looked up in `/ColorSpace` when it is not a device
/// name. One level of lookup, no recursion.
fn space(doc: &Document, resources: &Dict, name: &[u8]) -> Space {
    if let Some(s) = device_space(name) {
        return s;
    }
    let spaces = resources.get(b"ColorSpace").map(|o| doc.resolve(o));
    let Some(entry) = spaces.as_deref().and_then(Object::as_dict).and_then(|d| d.get(name)).map(|o| doc.resolve(o)) else {
        return Space::Other;
    };
    if let Some(n) = entry.as_name() {
        return device_space(n).unwrap_or(Space::Other);
    }
    let Some(array) = entry.as_array() else { return Space::Other };
    let family = array.first().map(|o| doc.resolve(o));
    match family.as_deref().and_then(Object::as_name) {
        Some(b"ICCBased") => {
            let profile = array.get(1).map(|o| doc.resolve(o));
            let n = match profile.as_deref() {
                Some(Object::Stream(s)) => s.dict.get(b"N").map(|o| doc.resolve(o)).and_then(|o| o.as_int()),
                _ => None,
            };
            match n {
                Some(1) => Space::Gray,
                Some(3) => Space::Rgb,
                Some(4) => Space::Cmyk,
                _ => Space::Other,
            }
        }
        Some(n) => device_space(n).unwrap_or(Space::Other),
        None => Space::Other,
    }
}

/// The path under construction, already in user space.
#[derive(Default)]
struct Builder {
    subpaths: Vec<Subpath>,
    open: Option<Subpath>,
    point: Option<Point>,
    segments: usize,
}

impl Builder {
    fn finish(&mut self) {
        if let Some(s) = self.open.take()
            && !s.segments.is_empty()
        {
            self.subpaths.push(s);
        }
    }

    fn move_to(&mut self, p: Option<Point>) {
        self.finish();
        self.point = p;
        self.open = p.map(|start| Subpath { start, segments: Vec::new(), closed: false });
    }

    /// Append a segment ending at `end`; after `h` it starts a new subpath at the current point.
    fn push(&mut self, segment: PathSegment, end: Point) {
        let Some(at) = self.point else { return };
        let open = self.open.get_or_insert_with(|| Subpath { start: at, segments: Vec::new(), closed: false });
        open.segments.push(segment);
        self.segments = self.segments.saturating_add(1);
        self.point = Some(end);
    }

    fn close(&mut self) {
        if let Some(mut s) = self.open.take() {
            s.closed = true;
            self.point = Some(s.start);
            self.open = Some(s);
            self.finish();
        }
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}

fn finite(p: Point) -> Option<Point> {
    p.iter().all(|v| v.is_finite()).then_some(p)
}

fn user(m: &Matrix, x: f64, y: f64) -> Option<Point> {
    let (x, y) = m.apply(x, y);
    finite([x, y])
}

/// The corners of `x y w h re` in user space, in drawing order.
fn corners(m: &Matrix, x: f64, y: f64, w: f64, h: f64) -> Option<[Point; 4]> {
    Some([user(m, x, y)?, user(m, x + w, y)?, user(m, x + w, y + h)?, user(m, x, y + h)?])
}

/// The exact extent of the subpaths (curve extrema included).
fn bbox(subpaths: &[Subpath]) -> Option<[f64; 4]> {
    let mut b: Option<[f64; 4]> = None;
    let mut add = |x0: f64, y0: f64, x1: f64, y1: f64| {
        b = Some(match b {
            None => [x0, y0, x1, y1],
            Some(r) => [r[0].min(x0), r[1].min(y0), r[2].max(x1), r[3].max(y1)],
        });
    };
    for s in subpaths {
        let mut at = s.start;
        add(at[0], at[1], at[0], at[1]);
        for seg in &s.segments {
            match *seg {
                PathSegment::Line(p) => {
                    add(p[0], p[1], p[0], p[1]);
                    at = p;
                }
                PathSegment::Cubic(c1, c2, p) => {
                    let k = |p: Point| (p[0], p[1]);
                    let r = CubicBez::new(k(at), k(c1), k(c2), k(p)).bounding_box();
                    add(r.x0, r.y0, r.x1, r.y1);
                    at = p;
                }
            }
        }
    }
    b.filter(|r| r.iter().all(|v| v.is_finite()))
}

struct Walker<'a> {
    doc: &'a Document,
    out: PathInventory,
    /// Forms being drawn on the current chain: a form met again is skipped (cycle guard).
    active: HashSet<ObjRef>,
    /// Streams that failed to decode, so a form drawn many times is decoded (and counted) once.
    failed: HashSet<ObjRef>,
    bytes: usize,
    streams: usize,
}

impl Walker<'_> {
    /// One stream's decoded data within the remaining budget, or `None` (recorded) when it is
    /// not a stream, cannot be decoded, or a limit was reached.
    fn decode(&mut self, object: &Object) -> Option<Vec<u8>> {
        let r = object.as_ref();
        if r.is_some_and(|r| self.failed.contains(&r)) {
            return None;
        }
        let resolved = self.doc.resolve(object);
        let Object::Stream(stream) = &*resolved else { return None };
        if self.streams >= MAX_STREAMS || self.out.unreadable >= MAX_UNREADABLE {
            self.out.truncated = true;
            return None;
        }
        self.streams = self.streams.saturating_add(1);
        match stream.decoded_within(MAX_BYTES) {
            Ok(data) => {
                let total = self.bytes.saturating_add(data.len());
                if total > MAX_BYTES {
                    self.out.truncated = true;
                    return None;
                }
                self.bytes = total;
                Some(data)
            }
            Err(_) => {
                self.out.unreadable = self.out.unreadable.saturating_add(1);
                if let Some(r) = r {
                    self.failed.insert(r);
                }
                None
            }
        }
    }

    /// Interpret `contents` (a stream, or an array of streams forming one stream) from `state`.
    fn walk(&mut self, contents: &Object, resources: &Dict, state: State, depth: usize) {
        if depth > MAX_FORM_DEPTH {
            self.out.truncated = true;
            return;
        }
        let resolved = self.doc.resolve(contents);
        let data = match &*resolved {
            // Parts form one stream: q/Q and paths may span their boundaries (§7.8.2).
            Object::Array(parts) => {
                let mut joined = Vec::new();
                for part in parts {
                    if let Some(data) = self.decode(part) {
                        joined.extend(data);
                        joined.push(b'\n');
                    }
                    if self.out.truncated {
                        return;
                    }
                }
                joined
            }
            _ => match self.decode(contents) {
                Some(data) => data,
                None => return,
            },
        };
        self.ops(&data, resources, state, depth);
    }

    fn ops(&mut self, data: &[u8], resources: &Dict, initial: State, depth: usize) {
        let mut state = initial.clone();
        let mut stack: Vec<State> = Vec::new();
        let mut path = Builder::default();
        for op in parse(data).ops {
            if self.out.truncated {
                return;
            }
            let ctm = state.ctm;
            match op.op.as_slice() {
                b"q" => {
                    if stack.len() >= MAX_STATE_DEPTH {
                        // Later transforms can't be tracked reliably; keep what was found.
                        self.out.truncated = true;
                        return;
                    }
                    stack.push(state.clone());
                }
                b"Q" => state = stack.pop().unwrap_or_else(|| initial.clone()),
                b"cm" => {
                    if let Some(m) = Matrix::from_operands(&op.operands) {
                        state.ctm = m.then(&ctm);
                    }
                }
                b"g" | b"G" | b"rg" | b"RG" | b"k" | b"K" => {
                    let paint = match op.op.as_slice() {
                        b"g" | b"G" => op.nums::<1>().map(|[g]| (Space::Gray, PathColor::Gray(g))),
                        b"rg" | b"RG" => op.nums::<3>().map(|c| (Space::Rgb, PathColor::Rgb(c))),
                        _ => op.nums::<4>().map(|c| (Space::Cmyk, PathColor::Cmyk(c))),
                    };
                    if let Some((space, color)) = paint {
                        let target = if op.op.first().is_some_and(u8::is_ascii_lowercase) { &mut state.fill } else { &mut state.stroke };
                        *target = Paint { space, color };
                    }
                }
                b"cs" => {
                    if let Some(name) = op.name(0) {
                        state.fill = Paint::initial(space(self.doc, resources, name));
                    }
                }
                b"CS" => {
                    if let Some(name) = op.name(0) {
                        state.stroke = Paint::initial(space(self.doc, resources, name));
                    }
                }
                b"sc" | b"scn" => state.fill.set(&op),
                b"SC" | b"SCN" => state.stroke.set(&op),
                b"m" => path.move_to(op.nums::<2>().and_then(|[x, y]| user(&ctm, x, y))),
                b"l" => {
                    if let Some(p) = op.nums::<2>().and_then(|[x, y]| user(&ctm, x, y))
                        && self.room(&path, 1)
                    {
                        path.push(PathSegment::Line(p), p);
                    }
                }
                b"c" | b"v" | b"y" => {
                    let at = path.point;
                    let points = match op.op.as_slice() {
                        b"c" => {
                            op.nums::<6>().and_then(|[x1, y1, x2, y2, x3, y3]| Some((user(&ctm, x1, y1)?, user(&ctm, x2, y2)?, user(&ctm, x3, y3)?)))
                        }
                        // `v`: the first control point is the current point.
                        b"v" => op.nums::<4>().and_then(|[x2, y2, x3, y3]| Some((at?, user(&ctm, x2, y2)?, user(&ctm, x3, y3)?))),
                        // `y`: the second control point is the end point.
                        _ => op.nums::<4>().and_then(|[x1, y1, x3, y3]| {
                            let end = user(&ctm, x3, y3)?;
                            Some((user(&ctm, x1, y1)?, end, end))
                        }),
                    };
                    if let Some((c1, c2, end)) = points
                        && self.room(&path, 1)
                    {
                        path.push(PathSegment::Cubic(c1, c2, end), end);
                    }
                }
                b"h" => path.close(),
                b"re" => {
                    let Some([x, y, w, h]) = op.nums::<4>() else { continue };
                    // A move, three lines and a close (§8.5.2.1, Table 58).
                    let Some([a, b, c, d]) = corners(&ctm, x, y, w, h) else {
                        path.move_to(None);
                        continue;
                    };
                    if !self.room(&path, 3) {
                        return;
                    }
                    path.move_to(Some(a));
                    for p in [b, c, d] {
                        path.push(PathSegment::Line(p), p);
                    }
                    path.close();
                }
                b"S" | b"s" | b"f" | b"F" | b"f*" | b"B" | b"B*" | b"b" | b"b*" => {
                    let o = op.op.as_slice();
                    if matches!(o, b"s" | b"b" | b"b*") {
                        path.close();
                    }
                    path.finish();
                    let fills = !matches!(o, b"S" | b"s");
                    let strokes = !matches!(o, b"f" | b"F" | b"f*");
                    let built = std::mem::take(&mut path);
                    self.paint(built, fills.then(|| state.fill.color.clone()), strokes.then(|| state.stroke.color.clone()));
                }
                b"n" => path.reset(),
                b"Do" => self.draw(&op, resources, &state, depth),
                _ => {}
            }
        }
    }

    /// Whether `more` segments still fit; records truncation when they don't.
    fn room(&mut self, path: &Builder, more: usize) -> bool {
        let fits = self.out.segments.saturating_add(path.segments).saturating_add(more) <= MAX_SEGMENTS;
        if !fits {
            self.out.truncated = true;
        }
        fits
    }

    fn paint(&mut self, path: Builder, fill: Option<PathColor>, stroke: Option<PathColor>) {
        let Some(bbox_user) = bbox(&path.subpaths) else { return };
        if self.out.items.len() >= MAX_ITEMS {
            self.out.truncated = true;
            return;
        }
        self.out.segments = self.out.segments.saturating_add(path.segments);
        self.out.items.push(PathItem { bbox_user, fill, stroke, subpaths: path.subpaths });
    }

    /// `Do`: draw a Form XObject from the current state; images and unknown names are ignored.
    fn draw(&mut self, op: &Op, resources: &Dict, state: &State, depth: usize) {
        let Some(name) = op.name(0) else { return };
        let xobjects = resources.get(b"XObject").map(|o| self.doc.resolve(o));
        let Some(object) = xobjects.as_deref().and_then(Object::as_dict).and_then(|d| d.get(name)) else { return };
        let r = object.as_ref();
        if r.is_some_and(|r| self.active.contains(&r)) {
            return;
        }
        let form = self.doc.resolve(object);
        let Object::Stream(s) = &*form else { return };
        if s.dict.name(b"Subtype") != Some(b"Form") {
            return;
        }
        let matrix = s.dict.get(b"Matrix").map(|m| self.doc.resolve(m)).and_then(|o| o.as_array().and_then(|a| Matrix::from_operands(a)));
        let mut inner = state.clone();
        inner.ctm = matrix.unwrap_or_default().then(&state.ctm);
        // A form without its own resources uses its parent's (§7.8.3, deprecated but common).
        let own = s.dict.get(b"Resources").map(|o| self.doc.resolve(o));
        let res = own.as_deref().and_then(Object::as_dict).unwrap_or(resources);
        if let Some(r) = r {
            self.active.insert(r);
        }
        self.walk(object, res, inner, depth.saturating_add(1));
        if let Some(r) = r {
            self.active.remove(&r);
        }
    }
}

#[cfg(test)]
mod tests;
