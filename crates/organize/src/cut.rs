//! The knife: cut pages into parts (plan/borracha-e-faca.md B1, decision D5). A scanned book with
//! two pages to a sheet becomes one page per side.
//!
//! Each part is the page itself (the first part) or a copy of it inserted right after, showing only
//! its part: its media and crop boxes are the part, and its bleed, trim and art boxes are cut to
//! it. Nothing is erased, so a part's content outside it is still in the file (B3 trims it).
//! The copies share the original's content, fonts and images, and are made without the page's
//! annotations; every annotation then goes to the part holding its centre (a pop-up goes with its
//! parent), so a form field never has a widget on two pages. Links, bookmarks and destinations
//! that pointed at the page go to the part holding the point they open (a `/Fit` one, with no
//! point, stays on the first part).

use pdfcraft_cos::{Dict, Document, ObjRef, Object};

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
        let aimed: Vec<(ObjRef, [f64; 4])> = part_refs.iter().copied().zip(parts.iter().copied()).collect();
        retarget_destinations(doc, page_ref, &aimed)?;
        outcome.parts.push((page, (0..parts.len()).map(|k| at.saturating_add(k)).collect()));
        offset = offset.saturating_add(copies);
    }
    Ok(outcome)
}

/// Most nodes of a name tree or bookmarks walked when destinations are aimed at the parts.
const MAX_DEST_NODES: usize = 100_000;

/// The point a destination array opens on its page, in user space, as far as it says: `/XYZ`
/// left and top (either may be null), `/FitH` and `/FitBH` top, `/FitV` and `/FitBV` left, the
/// middle of a `/FitR`. `None` for `/Fit` and `/FitB`, and anything else.
fn dest_point(doc: &Document, array: &[Object]) -> Option<(Option<f64>, Option<f64>)> {
    let num = |i: usize| array.get(i).and_then(|o| doc.resolve(o).as_f64()).filter(|v| v.is_finite());
    let point = match array.get(1)?.as_name()? {
        b"XYZ" => (num(2), num(3)),
        b"FitH" | b"FitBH" => (None, num(2)),
        b"FitV" | b"FitBV" => (num(2), None),
        b"FitR" => {
            let (x0, y0, x1, y1) = (num(2)?, num(3)?, num(4)?, num(5)?);
            (Some((x0 + x1) / 2.0), Some((y0 + y1) / 2.0))
        }
        _ => return None,
    };
    (point.0.is_some() || point.1.is_some()).then_some(point)
}

/// `dest` (a destination array, or a dictionary whose /D is one) aimed at the part holding its
/// point, when it opens `page` and that point lies in a later part; `None` when it stays as it is.
fn aim(doc: &Document, dest: &Object, page: ObjRef, parts: &[(ObjRef, [f64; 4])]) -> Option<Object> {
    if let Some(d) = dest.as_dict() {
        let inner = d.get(b"D")?;
        let new = match inner {
            Object::Ref(_) => return None,
            other => aim(doc, other, page, parts)?,
        };
        let mut d = d.clone();
        d.set(b"D".to_vec(), new);
        return Some(Object::Dict(d));
    }
    let array = dest.as_array()?;
    if array.first().and_then(Object::as_ref) != Some(page) {
        return None;
    }
    let (x, y) = dest_point(doc, array)?;
    let inside = |r: &[f64; 4]| x.is_none_or(|x| x >= r[0] && x <= r[2]) && y.is_none_or(|y| y >= r[1] && y <= r[3]);
    let k = parts.iter().position(|(_, r)| inside(r))?;
    let (target, _) = parts.get(k).filter(|_| k > 0)?;
    let mut array = array.clone();
    if let Some(first) = array.first_mut() {
        *first = Object::Ref(*target);
    }
    Some(Object::Array(array))
}

/// Aim the destination held in `value`: one written as its own object is changed there, and
/// `None` comes back; one written inline comes back changed, for the holder to write.
fn aim_slot(doc: &mut Document, value: &Object, page: ObjRef, parts: &[(ObjRef, [f64; 4])]) -> Option<Object> {
    match value {
        Object::Ref(r) => {
            let held = doc.get(*r);
            if let Some(new) = aim(doc, &held, page, parts) {
                doc.set(*r, new);
            }
            None
        }
        other => aim(doc, other, page, parts),
    }
}

/// After page `page` was cut into `parts` (each part's page and its rectangle), aim every
/// destination that opens a point in a later part at that part: bookmarks and links (their /Dest,
/// or a GoTo action's /D), the named destinations (the catalog's /Dests and /Names /Dests) and the
/// catalog's /OpenAction.
fn retarget_destinations(doc: &mut Document, page: ObjRef, parts: &[(ObjRef, [f64; 4])]) -> Result<(), OrganizeError> {
    if parts.len() < 2 {
        return Ok(());
    }
    // Holders of a /Dest or an /A: bookmarks, and the links on every page.
    let mut holders = crate::outline::items(doc);
    for (p, _) in walk(doc)? {
        let annots = match doc.get(p).as_dict().and_then(|d| d.get(b"Annots").cloned()) {
            Some(a) => doc.resolve(&a).as_array().cloned().unwrap_or_default(),
            None => Vec::new(),
        };
        holders
            .extend(annots.iter().filter_map(Object::as_ref).filter(|a| doc.get(*a).as_dict().is_some_and(|d| d.name(b"Subtype") == Some(b"Link"))));
    }
    holders.truncate(MAX_DEST_NODES);
    for h in holders {
        let Some(d) = doc.get(h).as_dict().cloned() else { continue };
        if let Some(dest) = d.get(b"Dest").cloned()
            && let Some(new) = aim_slot(doc, &dest, page, parts)
        {
            doc.update_dict(h, |d| d.set(b"Dest".to_vec(), new))?;
        }
        match d.get(b"A").cloned() {
            Some(Object::Ref(a)) => {
                let action = doc.get(a).as_dict().cloned();
                if let Some(mut action) = action.filter(|a| a.name(b"S") == Some(b"GoTo"))
                    && let Some(dest) = action.get(b"D").cloned()
                    && let Some(new) = aim_slot(doc, &dest, page, parts)
                {
                    action.set(b"D".to_vec(), new);
                    doc.set(a, Object::Dict(action));
                }
            }
            Some(Object::Dict(mut action)) if action.name(b"S") == Some(b"GoTo") => {
                if let Some(dest) = action.get(b"D").cloned()
                    && let Some(new) = aim_slot(doc, &dest, page, parts)
                {
                    action.set(b"D".to_vec(), new);
                    doc.update_dict(h, |d| d.set(b"A".to_vec(), Object::Dict(action)))?;
                }
            }
            _ => {}
        }
    }
    let Some(root) = doc.root() else { return Ok(()) };
    let Some(catalog) = doc.get(root).as_dict().cloned() else { return Ok(()) };
    // The catalog's /OpenAction: a destination array, or a GoTo action.
    if let Some(open) = catalog.get(b"OpenAction").cloned() {
        let resolved = (*doc.resolve(&open)).clone();
        let is_goto = resolved.as_dict().is_some_and(|a| a.name(b"S") == Some(b"GoTo"));
        if is_goto || resolved.as_array().is_some() {
            match &open {
                Object::Ref(r) => {
                    if let Some(new) = aim(doc, &resolved, page, parts) {
                        doc.set(*r, new);
                    }
                }
                other => {
                    if let Some(new) = aim(doc, other, page, parts) {
                        doc.update_dict(root, |c| c.set(b"OpenAction".to_vec(), new))?;
                    }
                }
            }
        }
    }
    // Named destinations: the legacy /Dests dictionary, and the /Names /Dests tree.
    match catalog.get(b"Dests").cloned() {
        Some(Object::Ref(r)) => {
            if let Some(mut d) = doc.get(r).as_dict().cloned()
                && aim_dict_values(doc, &mut d, page, parts)
            {
                doc.set(r, Object::Dict(d));
            }
        }
        Some(Object::Dict(mut d)) => {
            if aim_dict_values(doc, &mut d, page, parts) {
                doc.update_dict(root, |c| c.set(b"Dests".to_vec(), Object::Dict(d)))?;
            }
        }
        _ => {}
    }
    let tree = match catalog.get(b"Names").map(|n| doc.resolve(n)).and_then(|n| n.as_dict().cloned()) {
        Some(names) => names.get(b"Dests").cloned(),
        None => None,
    };
    if let Some(tree) = tree {
        let mut seen = Vec::new();
        match tree {
            Object::Ref(r) => aim_tree(doc, r, page, parts, &mut seen)?,
            // An inline tree node: its kids held by reference are still reached.
            Object::Dict(node) => aim_tree_kids(doc, &node, page, parts, &mut seen)?,
            _ => {}
        }
    }
    Ok(())
}

/// Aim the values of a legacy /Dests dictionary. True when an inline value changed.
fn aim_dict_values(doc: &mut Document, dests: &mut Dict, page: ObjRef, parts: &[(ObjRef, [f64; 4])]) -> bool {
    let entries: Vec<(Vec<u8>, Object)> = dests.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    let mut changed = false;
    for (k, v) in entries {
        if let Some(new) = aim_slot(doc, &v, page, parts) {
            dests.set(k, new);
            changed = true;
        }
    }
    changed
}

/// Aim the destinations in one name-tree node (held by reference) and its kids.
fn aim_tree(doc: &mut Document, r: ObjRef, page: ObjRef, parts: &[(ObjRef, [f64; 4])], seen: &mut Vec<ObjRef>) -> Result<(), OrganizeError> {
    if seen.contains(&r) || seen.len() >= MAX_DEST_NODES {
        return Ok(());
    }
    seen.push(r);
    let Some(mut node) = doc.get(r).as_dict().cloned() else { return Ok(()) };
    if let Some(pairs) = node.get(b"Names").map(|n| doc.resolve(n)).and_then(|n| n.as_array().cloned()) {
        let mut changed = false;
        let mut out = Vec::with_capacity(pairs.len());
        for pair in pairs.chunks(2) {
            match pair {
                [key, value] => {
                    out.push(key.clone());
                    match aim_slot(doc, value, page, parts) {
                        Some(new) => {
                            out.push(new);
                            changed = true;
                        }
                        None => out.push(value.clone()),
                    }
                }
                other => out.extend_from_slice(other),
            }
        }
        if changed {
            node.set(b"Names".to_vec(), Object::Array(out));
            doc.set(r, Object::Dict(node.clone()));
        }
    }
    aim_tree_kids(doc, &node, page, parts, seen)
}

fn aim_tree_kids(doc: &mut Document, node: &Dict, page: ObjRef, parts: &[(ObjRef, [f64; 4])], seen: &mut Vec<ObjRef>) -> Result<(), OrganizeError> {
    let kids = node.get(b"Kids").map(|k| doc.resolve(k)).and_then(|k| k.as_array().cloned()).unwrap_or_default();
    for kid in kids {
        if let Some(r) = kid.as_ref() {
            aim_tree(doc, r, page, parts, seen)?;
        }
    }
    Ok(())
}
