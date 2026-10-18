//! Chess ▸ Replace diagram (blueprint S10): draw a new board over a diagram the scan found.
//!
//! The board is `pdfcraft_chess::draw`'s drawing (the one drawing PDF, SVG and PNG exports
//! share), turned into content-stream operators by [`drawing_ops`] and placed by
//! [`placement_matrix`] so it shows upright on the displayed page, `/Rotate` included, as large
//! as fits in the diagram's box and centred in it.
//!
//! [`Session::chess_replacement`] turns an [`Edit::ReplaceDiagram`] into one undoable
//! [`Edit::Batch`], chosen by what the scan saw:
//!
//! - **an embedded image** that is the diagram (it lies within the box): the image is deleted
//!   (`ImageEdit::Delete`) and the board drawn where it was;
//! - **text in a chess diagram font**: the lines of that font within the box are removed
//!   ([`Edit::RemoveTextLines`], so the old position is no longer in the text a later scan, a
//!   search or an export reads), then the box is covered and the board drawn;
//! - **anything else** (a contour on a scanned page, squares drawn as paths, an image that also
//!   holds a caption): an opaque white cover over the box, then the board. What was there stays
//!   in the page's content under the cover, and the plan says so.
//!
//! The diagram is named by its number in the page's one reading order and by the document's edit
//! generation it was scanned at: an edit, undo or redo since then refuses the replacement, so a
//! stale number never replaces the wrong diagram.

use std::fmt::Write as _;
use std::sync::Arc;

use pdfcraft_chess::draw::{BoardStyle, Drawing, Paint, PieceSet, Rgb, Segment, board};
use pdfcraft_chess::placement::Placement;
use pdfcraft_chessvision::coords::{PageGeometry, PageRect, finite_page_rect};
use pdfcraft_chessvision::detect::Source;
use pdfcraft_chessvision::merge::iou;

use super::ChessScanSettings;
use crate::{DocId, Edit, EditError, ImageEdit, Session};

/// The undo label of a replacement (listed in [`crate::EDIT_LABELS`] and [`crate::BATCH_LABELS`]).
pub const LABEL: &str = "Replace diagram";
/// An image is the diagram when it reaches past the diagram's box by at most this share of the
/// box's side (a margin or a thin frame); a larger one holds more (a caption) and is covered.
const IMAGE_SLACK: f32 = 0.10;
/// Lines in a diagram font belong to the diagram when they lie within its box grown by this share
/// of its side: one square, for the frame drawn in the font around the board.
const FONT_SLACK: f32 = 1.0 / 8.0;
/// The cover reaches past the box by this share of its side, at most [`COVER_MAX_PAD`] points,
/// so the edge of a scanned board leaves no line.
const COVER_PAD: f32 = 0.02;
const COVER_MAX_PAD: f32 = 3.0;

/// Chess ▸ Replace diagram: what to draw, and over which diagram.
#[derive(Clone, Debug, PartialEq)]
pub struct DiagramReplacement {
    /// The page, 0-based.
    pub page: usize,
    /// The diagram's position in the page's reading order ([`super::ChessPage::diagrams`]), 0-based.
    pub index: usize,
    /// The document's edit generation ([`crate::Document::edit_generation`]) the diagram was
    /// numbered at. Any other generation is refused.
    pub generation: u64,
    pub placement: Placement,
    pub style: BoardStyle,
    /// The pieces to draw with (`chessvision::pieces::find`, or the geometric set).
    pub pieces: Arc<PieceSet>,
    /// The scan the diagram was numbered by: its order and detection settings.
    pub scan: ChessScanSettings,
}

/// How a replacement will be made ([`Session::chess_replacement`]): checked against the page
/// now, and the edit that makes it.
#[derive(Clone, Debug, PartialEq)]
pub struct ReplacementPlan {
    pub page: usize,
    pub index: usize,
    /// The diagram's box, user space.
    pub bbox_user: PageRect,
    /// The detector that placed the box, and every detector that saw the diagram.
    pub source: Source,
    pub sources: Vec<Source>,
    /// The page image deleted (its index in `Document::page_images`).
    pub deleted_image: Option<usize>,
    /// The lines of diagram-font text removed (indexes in `Document::text_lines`).
    pub removed_lines: Vec<usize>,
    /// The white cover drawn over the old diagram, user space.
    pub cover: Option<[f64; 4]>,
    /// Where the new board lands, user space.
    pub board_user: [f64; 4],
    /// What stays on the page, what was approximated: never silent.
    pub notes: Vec<String>,
    /// The undoable edit: a [`Edit::Batch`] labelled [`LABEL`].
    pub edit: Edit,
}

/// A number for a content stream: at most four decimals, no trailing zeros, finite.
fn num(v: f64) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" || s.is_empty() { "0".into() } else { s.into() }
}

fn colour(c: Rgb) -> String {
    let unit = |v: u8| num(f64::from(v) / 255.0);
    format!("{} {} {}", unit(c.0), unit(c.1), unit(c.2))
}

/// The operators that draw `drawing` with `matrix` taking drawing units (origin top left, y down)
/// to user space: its layers in order, then its labels in Helvetica (`/PCHelv`, which
/// `pdfcraft_edit::add_operators` provides), all inside one `q … Q`. A label's glyphs are turned
/// back up, as the drawing's y runs down.
pub fn drawing_ops(drawing: &Drawing, matrix: [f64; 6]) -> Vec<u8> {
    let mut s = String::new();
    // Writing to a String cannot fail, so the results of `write!` are ignored throughout.
    let _ = writeln!(s, "q\n{} cm\n1 j 1 J", matrix.map(num).join(" "));
    for layer in &drawing.layers {
        let mut path = String::new();
        for p in &layer.subpaths {
            let _ = writeln!(path, "{} {} m", num(f64::from(p.start.0)), num(f64::from(p.start.1)));
            for seg in &p.segments {
                let _ = match *seg {
                    Segment::Line(x, y) => writeln!(path, "{} {} l", num(f64::from(x)), num(f64::from(y))),
                    Segment::Cubic(x1, y1, x2, y2, x, y) => {
                        let p = [x1, y1, x2, y2, x, y].map(|v| num(f64::from(v)));
                        writeln!(path, "{} c", p.join(" "))
                    }
                };
            }
            if p.closed {
                path.push_str("h\n");
            }
        }
        if path.is_empty() {
            continue;
        }
        match layer.paint {
            Paint::Fill { color, even_odd } => {
                let _ = write!(s, "{} rg\n{path}{}\n", colour(color), if even_odd { "f*" } else { "f" });
            }
            Paint::Stroke { color, width } => {
                let _ = write!(s, "{} RG {} w\n{path}S\n", colour(color), num(f64::from(width)));
            }
        }
    }
    for label in &drawing.labels {
        let size = f64::from(label.size);
        if !(size.is_finite() && size > 0.0) || label.text.is_empty() {
            continue;
        }
        let width = pdfcraft_fonts::helvetica_width(&label.text, size);
        // Centred on (x, y): half the width to the left, the baseline about half a capital below.
        let (x, y) = (f64::from(label.x) - width / 2.0, f64::from(label.y) + 0.36 * size);
        let text = String::from_utf8_lossy(&pdfcraft_fonts::literal(&pdfcraft_fonts::win_ansi(&label.text))).into_owned();
        let _ = writeln!(s, "BT /PCHelv 1 Tf {} rg {} 0 0 {} {} {} Tm {text} Tj ET", colour(label.color), num(size), num(-size), num(x), num(y));
    }
    s.push_str("Q\n");
    s.into_bytes()
}

/// The matrix that draws a `size` × `size` drawing (drawing units, origin top left, y down) as
/// large as fits in `bbox` (user space), centred, and upright as the page is displayed: the
/// drawing's top runs along the displayed page's top whatever its `/Rotate`. `None` for a box or
/// size that is not finite and positive.
pub fn placement_matrix(geometry: &PageGeometry, bbox: PageRect, size: f32) -> Option<[f64; 6]> {
    if !(size.is_finite() && size > 0.0) {
        return None;
    }
    let view = geometry.user_to_view(finite_page_rect(bbox)?)?;
    let side = f64::from(view.width().min(view.height()));
    if !(side.is_finite() && side > 0.0) {
        return None;
    }
    // View space to user space is affine: its origin and the images of two long axes.
    let (w, h) = geometry.view_size();
    let at = |x: f32, y: f32| geometry.view_to_user_point(x, y).map(|[u, v]| [f64::from(u), f64::from(v)]);
    let origin = at(0.0, 0.0)?;
    let (across, down) = (at(w, 0.0)?, at(0.0, h)?);
    let (w, h) = (f64::from(w), f64::from(h));
    if !(w > 0.0 && h > 0.0) {
        return None;
    }
    let ex = [(across[0] - origin[0]) / w, (across[1] - origin[1]) / w];
    let ey = [(down[0] - origin[0]) / h, (down[1] - origin[1]) / h];
    let scale = side / f64::from(size);
    let ox = f64::from(view.x0) + (f64::from(view.width()) - side) / 2.0;
    let oy = f64::from(view.y0) + (f64::from(view.height()) - side) / 2.0;
    let m = [scale * ex[0], scale * ex[1], scale * ey[0], scale * ey[1], origin[0] + ox * ex[0] + oy * ey[0], origin[1] + ox * ex[1] + oy * ey[1]];
    m.iter().all(|v| v.is_finite()).then_some(m)
}

/// The user-space box of a `size` square under `m`.
fn mapped_box(m: [f64; 6], size: f64) -> [f64; 4] {
    let corners = [(0.0, 0.0), (size, 0.0), (0.0, size), (size, size)].map(|(x, y)| (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]));
    let xs = corners.map(|c| c.0);
    let ys = corners.map(|c| c.1);
    let fold = |v: [f64; 4], f: fn(f64, f64) -> f64| v.iter().copied().reduce(f).unwrap_or(0.0);
    [fold(xs, f64::min), fold(ys, f64::min), fold(xs, f64::max), fold(ys, f64::max)]
}

/// `r` grown by `pad` on every side.
fn grown(r: PageRect, pad: f32) -> PageRect {
    PageRect::new(r.x0 - pad, r.y0 - pad, r.x1 + pad, r.y1 + pad)
}

fn within(inner: [f64; 4], outer: PageRect) -> bool {
    let [x0, y0, x1, y1] = inner;
    x0 >= f64::from(outer.x0) && y0 >= f64::from(outer.y0) && x1 <= f64::from(outer.x1) && y1 <= f64::from(outer.y1)
}

fn page_rect(r: [f64; 4]) -> PageRect {
    let [x0, y0, x1, y1] = r.map(|v| v as f32);
    PageRect::new(x0, y0, x1, y1)
}

fn invalid(why: impl Into<String>) -> EditError {
    EditError::Invalid(why.into())
}

impl Session {
    /// Check a replacement against the document as it is now and plan it: which diagram, what
    /// happens to the old one, where the board goes, and the [`Edit::Batch`] that does it.
    /// Changes nothing (the page's scan is cached). Refused when the document is not editable or
    /// its permissions do not allow changes (or copying, which scanning needs), when it changed
    /// since `generation`, and when the page or diagram does not exist.
    pub fn chess_replacement(&mut self, id: DocId, r: &DiagramReplacement) -> Result<ReplacementPlan, EditError> {
        let doc = self.get(id).ok_or(EditError::NoDocument)?;
        let editor = doc.editor.as_ref().ok_or_else(|| EditError::ReadOnly(doc.read_only_reason.clone().unwrap_or_default()))?;
        if editor.cos.permissions().is_some_and(|p| !p.modify()) {
            return Err(EditError::NotPermitted("changes to the document"));
        }
        if doc.generation != r.generation {
            return Err(invalid(format!(
                "the document changed since its diagrams were numbered (generation {}, now {}): scan the page again",
                r.generation, doc.generation
            )));
        }
        let pages = doc.info.pages.len();
        if r.page >= pages {
            return Err(EditError::Edit(pdfcraft_edit::EditError::NoSuchPage(r.page)));
        }
        let scanned = self.chess_scan(id, &[r.page], &r.scan, false).map_err(invalid)?;
        let page = scanned.into_iter().next().ok_or_else(|| invalid("the page could not be scanned"))?;
        let found = page.diagrams.len();
        let diagram = page.diagrams.get(r.index).ok_or_else(|| {
            invalid(format!("page {} has {found} diagram{}; there is no diagram {}", r.page + 1, if found == 1 { "" } else { "s" }, r.index + 1))
        })?;
        let geometry = page.geometry.ok_or_else(|| invalid(page.error.clone().unwrap_or_else(|| "the page has no visible area".into())))?;
        let bbox = finite_page_rect(diagram.candidate.bbox_user).ok_or_else(|| invalid("the diagram's box is not finite"))?;
        let side = bbox.width().max(bbox.height());
        let doc = self.get(id).ok_or(EditError::NoDocument)?;
        let editor = doc.editor.as_ref().ok_or_else(|| EditError::ReadOnly(doc.read_only_reason.clone().unwrap_or_default()))?;
        let mut notes = Vec::new();
        let mut edits = Vec::new();

        // An image that is the diagram goes; the one that matches the box best.
        let mut deleted_image = None;
        if diagram.sources.contains(&Source::Embedded) {
            let images = pdfcraft_edit::page_images(&editor.cos, r.page).map_err(EditError::Edit)?;
            let reach = grown(bbox, IMAGE_SLACK * side);
            deleted_image = images
                .iter()
                .enumerate()
                .filter(|(_, i)| !i.is_form && i.rect.iter().all(|v| v.is_finite()) && within(i.rect, reach))
                .map(|(n, i)| (n, iou(&page_rect(i.rect), &bbox)))
                .filter(|(_, overlap)| *overlap >= 0.5)
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(n, _)| n);
            match deleted_image {
                Some(index) => edits.push(Edit::EditPageImage { page: r.page, index, change: ImageEdit::Delete }),
                None => notes.push("the diagram's image reaches past the board (a caption or more), so it was covered, not deleted".into()),
            }
        }

        // Text in a diagram font: its lines within the box go, so the old position is not read again.
        let mut removed_lines = Vec::new();
        if diagram.sources.contains(&Source::Font) {
            use pdfcraft_chess::catalog::{FontKind, lookup_family};
            let reach = grown(bbox, FONT_SLACK * side);
            let lines = pdfcraft_edit::text_lines(&editor.cos, r.page).map_err(EditError::Edit)?;
            removed_lines = lines
                .iter()
                .enumerate()
                .filter(|(_, l)| lookup_family(&l.base_font).is_some_and(|f| f.kind == FontKind::Diagram) && within(l.rect, reach))
                .map(|(n, _)| n)
                .collect();
            if removed_lines.is_empty() {
                notes.push("the diagram's chess-font text was not found among the page's lines; it stays under the cover".into());
            } else {
                edits.push(Edit::RemoveTextLines { page: r.page, lines: removed_lines.clone() });
            }
        }

        let drawing = board(&r.placement, &r.pieces, &r.style);
        let matrix = placement_matrix(&geometry, bbox, drawing.width).ok_or_else(|| invalid("the diagram's box has no area to draw in"))?;
        let mut ops = Vec::new();
        let cover = deleted_image.is_none().then(|| {
            let c = grown(bbox, (COVER_PAD * side).min(COVER_MAX_PAD));
            [c.x0, c.y0, c.x1, c.y1].map(f64::from)
        });
        if let Some([x0, y0, x1, y1]) = cover {
            ops.extend_from_slice(format!("q 1 g {} {} {} {} re f Q\n", num(x0), num(y0), num(x1 - x0), num(y1 - y0)).as_bytes());
            if !diagram.sources.contains(&Source::Font) || removed_lines.is_empty() {
                notes.push("the old diagram stays in the page's content under an opaque white cover".into());
            }
        }
        ops.extend(drawing_ops(&drawing, matrix));
        edits.push(Edit::AddContent { page: r.page, ops: Arc::new(ops) });
        Ok(ReplacementPlan {
            page: r.page,
            index: r.index,
            bbox_user: bbox,
            source: diagram.candidate.source,
            sources: diagram.sources.clone(),
            deleted_image,
            removed_lines,
            cover,
            board_user: mapped_box(matrix, f64::from(drawing.width)),
            notes,
            edit: Edit::Batch { label: LABEL.into(), edits },
        })
    }
}
