//! The knife: cut pages into parts (plan/borracha-e-faca.md B1, decision D5). A scanned book with
//! two pages to a sheet becomes one page per side.
//!
//! Each part is the page itself (the first part) or a copy of it inserted right after, showing only
//! its part: its media and crop boxes are the part, and its bleed, trim and art boxes are cut to
//! it. Nothing is erased, so a part's content outside it is still in the file (B3 trims it).
//! The copies share the original's content, fonts and images, and are made without the page's
//! annotations; every annotation then goes to the part holding its centre (a pop-up goes with its
//! parent), so a form field never has a widget on two pages. Links, bookmarks and destinations
//! that pointed at the page point at the first part.

use pdfcraft_cos::{Document, ObjRef, Object};

use crate::{OrganizeError, duplicate_pages, page_boxes, page_rotation, walk};

/// Most parts one page is cut into.
pub const MAX_PARTS: usize = 16;
/// Most pages a document may have after a cut.
pub const MAX_PAGES_AFTER_CUT: usize = 100_000;
/// Least distance, in points, between two cuts and between a cut and the page's edge.
pub const MIN_PART: f64 = 1.0;

/// Which way a cut runs, on the page as it is shown (after its rotation).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// An up-and-down line: the parts are side by side, left first.
    Vertical,
    /// A side-to-side line: the parts are one above the other, top first.
    Horizontal,
}

/// Cut page `page` (0-based) along lines at `at`: points from the left edge (vertical) or the top
/// edge (horizontal) of the page as shown, rotation and `/UserUnit` applied.
#[derive(Clone, Debug, PartialEq)]
pub struct Cut {
    pub page: usize,
    pub axis: Axis,
    pub at: Vec<f64>,
}

/// What a cut made: for each page cut (0-based, as before), its parts (0-based, after).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CutOutcome {
    pub parts: Vec<(usize, Vec<usize>)>,
}

fn bad(m: impl Into<String>) -> OrganizeError {
    OrganizeError::Invalid(m.into())
}

/// The user-space rectangle under the shown rectangle `[sx0, sy0, sx1, sy1]` (user units from the
/// shown top-left) of a page whose visible box is `vis` and whose rotation is `rot`.
fn user_rect(vis: [f64; 4], rot: i64, s: [f64; 4]) -> [f64; 4] {
    let [x0, y0, x1, y1] = vis;
    let point = |sx: f64, sy: f64| match rot {
        90 => (x0 + sy, y0 + sx),
        180 => (x1 - sx, y0 + sy),
        270 => (x1 - sy, y1 - sx),
        _ => (x0 + sx, y1 - sy),
    };
    let (a, b) = (point(s[0], s[1]), point(s[2], s[3]));
    [a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1)]
}

/// The parts of one page, in user space, in reading order (or right to left).
fn parts_of(doc: &Document, cut: &Cut, vis: [f64; 4], unit: f64, right_to_left: bool) -> Result<Vec<[f64; 4]>, OrganizeError> {
    let page = cut.page.saturating_add(1);
    let rot = page_rotation(doc, cut.page)?;
    let (w, h) = (vis[2] - vis[0], vis[3] - vis[1]);
    let (shown_w, shown_h) = if rot % 180 == 0 { (w, h) } else { (h, w) };
    let extent = if cut.axis == Axis::Vertical { shown_w } else { shown_h } * unit;
    if cut.at.is_empty() || cut.at.len() >= MAX_PARTS {
        return Err(bad(format!("give page {page} between 1 and {} cuts, not {}", MAX_PARTS - 1, cut.at.len())));
    }
    let mut at = cut.at.clone();
    if !at.iter().all(|v| v.is_finite()) {
        return Err(bad(format!("a cut on page {page} is not a number")));
    }
    at.sort_by(f64::total_cmp);
    let mut edges = vec![0.0];
    for &a in &at {
        let last = edges.last().copied().unwrap_or(0.0);
        if a - last < MIN_PART || extent - a < MIN_PART {
            return Err(bad(format!(
                "the cuts on page {page} must lie inside its {extent:.0} pt, at least {MIN_PART} pt from each other and from its edges"
            )));
        }
        edges.push(a);
    }
    edges.push(extent);
    let mut parts: Vec<[f64; 4]> = edges
        .windows(2)
        .filter_map(|e| {
            let [a, b] = e else { return None };
            // Back to user units.
            let (a, b) = (a / unit, b / unit);
            let shown = match cut.axis {
                Axis::Vertical => [a, 0.0, b, shown_h],
                Axis::Horizontal => [0.0, a, shown_w, b],
            };
            Some(user_rect(vis, rot, shown))
        })
        .collect();
    if right_to_left {
        parts.reverse();
    }
    Ok(parts)
}

fn rect_obj(r: [f64; 4]) -> Object {
    Object::Array(r.iter().map(|v| Object::Real(*v)).collect())
}

fn rect_of(doc: &Document, o: Option<&Object>) -> Option<[f64; 4]> {
    let v: Vec<f64> = doc.resolve(o?).as_array()?.iter().filter_map(|x| doc.resolve(x).as_f64()).collect();
    let [a, b, c, d] = <[f64; 4]>::try_from(v).ok()?;
    [a, b, c, d].iter().all(|x| x.is_finite()).then(|| [a.min(c), b.min(d), a.max(c), b.max(d)])
}

/// Cut pages into parts (see the module). `right_to_left` orders each page's parts from the right
/// (or the bottom), for books read that way. Every cut is checked before anything changes; a
/// page may be cut once.
///
/// # Errors
///
/// [`OrganizeError::NoSuchPage`], and [`OrganizeError::Invalid`] for a page cut twice, cuts that
/// are not numbers, outside the page, closer than [`MIN_PART`] to each other or to an edge, more
/// than [`MAX_PARTS`] parts, or a document that would pass [`MAX_PAGES_AFTER_CUT`]. Not
/// transactional past the checks: callers apply it to a working copy.
pub fn cut_pages(doc: &mut Document, cuts: &[Cut], right_to_left: bool) -> Result<CutOutcome, OrganizeError> {
    let n = walk(doc)?.len();
    let boxes = page_boxes(doc)?;
    let mut sorted: Vec<&Cut> = cuts.iter().collect();
    sorted.sort_by_key(|c| c.page);
    if sorted.windows(2).any(|w| matches!(w, [a, b] if a.page == b.page)) {
        return Err(bad("a page can be cut once at a time; give all its cuts together"));
    }
    if cuts.is_empty() {
        return Err(bad("there is nothing to cut"));
    }
    let mut plans = Vec::with_capacity(sorted.len());
    let mut total = n;
    let pages = walk(doc)?;
    for cut in &sorted {
        let b = boxes.get(cut.page).ok_or(OrganizeError::NoSuchPage(cut.page))?;
        let (page_ref, _) = pages.get(cut.page).ok_or(OrganizeError::NoSuchPage(cut.page))?;
        let page_ref = *page_ref;
        let unit = doc
            .get(page_ref)
            .as_dict()
            .and_then(|d| d.get(b"UserUnit").and_then(|u| doc.resolve(u).as_f64()))
            .filter(|u| u.is_finite() && *u > 0.0)
            .unwrap_or(1.0);
        let parts = parts_of(doc, cut, b[1], unit, right_to_left)?;
        total = total.saturating_add(parts.len().saturating_sub(1));
        if total > MAX_PAGES_AFTER_CUT {
            return Err(bad(format!("the cut would make more than {MAX_PAGES_AFTER_CUT} pages")));
        }
        plans.push((cut.page, page_ref, parts));
    }

    let mut outcome = CutOutcome::default();
    let mut offset = 0usize;
    for (page, page_ref, parts) in plans {
        let at = page.saturating_add(offset);
        let copies = parts.len().saturating_sub(1);
        // The copies are made without annotations: they are given out below.
        let annots: Vec<Object> = match doc.get(page_ref).as_dict().and_then(|d| d.get(b"Annots").cloned()) {
            Some(a) => doc.resolve(&a).as_array().cloned().unwrap_or_default(),
            None => Vec::new(),
        };
        doc.update_dict(page_ref, |d| {
            d.remove(b"Annots");
        })?;
        for _ in 0..copies {
            duplicate_pages(doc, &[at], Some(at.saturating_add(1)))?;
        }
        let all = walk(doc)?;
        let part_refs: Vec<ObjRef> = (0..parts.len()).filter_map(|k| all.get(at.saturating_add(k)).map(|(r, _)| *r)).collect();
        if part_refs.len() != parts.len() || part_refs.first() != Some(&page_ref) {
            return Err(bad(format!("page {} could not be copied", page.saturating_add(1))));
        }
        // Boxes.
        for (r, part) in part_refs.iter().zip(&parts) {
            let d = doc.get(*r).as_dict().cloned().unwrap_or_default();
            let child: Vec<(&[u8], Option<[f64; 4]>)> = [b"BleedBox".as_slice(), b"TrimBox", b"ArtBox"]
                .into_iter()
                .filter(|k| d.contains(k))
                .map(|k| {
                    let cut = rect_of(doc, d.get(k)).map(|c| [c[0].max(part[0]), c[1].max(part[1]), c[2].min(part[2]), c[3].min(part[3])]);
                    (k, cut.filter(|c| c[2] - c[0] >= MIN_PART && c[3] - c[1] >= MIN_PART))
                })
                .collect();
            doc.update_dict(*r, |d| {
                d.set(b"MediaBox".to_vec(), rect_obj(*part));
                d.set(b"CropBox".to_vec(), rect_obj(*part));
                for (k, v) in &child {
                    match v {
                        Some(c) => d.set(k.to_vec(), rect_obj(*c)),
                        None => {
                            d.remove(k);
                        }
                    }
                }
            })?;
        }
        // Annotations: to the part holding the centre (a pop-up follows its parent).
        let owner = |doc: &Document, o: &Object| -> usize {
            let a = doc.resolve(o);
            let Some(d) = a.as_dict() else { return 0 };
            let Some(r) = rect_of(doc, d.get(b"Rect")) else { return 0 };
            let (cx, cy) = ((r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0);
            parts.iter().position(|p| cx >= p[0] && cx <= p[2] && cy >= p[1] && cy <= p[3]).unwrap_or(0)
        };
        let mut given: Vec<Vec<Object>> = vec![Vec::new(); parts.len()];
        let mut parent_part: Vec<(ObjRef, usize)> = Vec::new();
        for a in &annots {
            let is_popup = doc.resolve(a).as_dict().is_some_and(|d| d.name(b"Subtype") == Some(b"Popup"));
            if !is_popup {
                let k = owner(doc, a);
                if let Some(r) = a.as_ref() {
                    parent_part.push((r, k));
                }
                if let Some(list) = given.get_mut(k) {
                    list.push(a.clone());
                }
            }
        }
        for a in &annots {
            let d = doc.resolve(a).as_dict().cloned();
            if d.as_ref().is_some_and(|d| d.name(b"Subtype") == Some(b"Popup")) {
                let parent = d.as_ref().and_then(|d| d.reference(b"Parent"));
                let k = parent.and_then(|p| parent_part.iter().find(|(r, _)| *r == p).map(|(_, k)| *k)).unwrap_or_else(|| owner(doc, a));
                if let Some(list) = given.get_mut(k) {
                    list.push(a.clone());
                }
            }
        }
        for (k, (r, list)) in part_refs.iter().zip(given).enumerate() {
            if list.is_empty() {
                continue;
            }
            for a in &list {
                if let Some(ar) = a.as_ref()
                    && k > 0
                {
                    doc.update_dict(ar, |d| d.set(b"P".to_vec(), Object::Ref(*r)))?;
                }
            }
            doc.update_dict(*r, |d| d.set(b"Annots".to_vec(), Object::Array(list)))?;
        }
        outcome.parts.push((page, (0..parts.len()).map(|k| at.saturating_add(k)).collect()));
        offset = offset.saturating_add(copies);
    }
    Ok(outcome)
}
