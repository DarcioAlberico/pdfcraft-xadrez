//! Merging candidates from different detectors into one diagram each.
//!
//! Each detector sees a diagram its own way: the embedded image locates it, the contour fits its
//! printed edge, a diagram font reads its position, a vector lattice knows its squares. Merging
//! composes them per diagram instead of picking one detector per page:
//!
//! - candidates that overlap by at least [`OVERLAP_IOU`] are one diagram;
//! - its box comes from the most precise source present (font, then vector, then contour, then
//!   embedded image), and a position read from a font is kept;
//! - when the page has embedded diagrams, a candidate found only by another detector whose size
//!   is far from theirs ([`EMBEDDED_SIZE_TOLERANCE`]) is dropped: on such pages the images are
//!   the diagrams, and the odd box is a table or a frame.

use pdfcraft_geom::PageRect;

use crate::detect::{DiagramCandidate, Source};

/// Two candidates overlapping at least this much are the same diagram.
pub const OVERLAP_IOU: f32 = 0.5;
/// How far, as a share of the embedded diagrams' median side, another-source-only candidate may
/// differ in size before it is dropped.
pub const EMBEDDED_SIZE_TOLERANCE: f32 = 0.30;

/// Intersection over union of two boxes; 0 for empty or invalid boxes.
pub fn iou(a: &PageRect, b: &PageRect) -> f32 {
    let w = (a.x1.min(b.x1) - a.x0.max(b.x0)).max(0.0);
    let h = (a.y1.min(b.y1) - a.y0.max(b.y0)).max(0.0);
    let inter = w * h;
    let union = a.width() * a.height() + b.width() * b.height() - inter;
    if union > 0.0 && union.is_finite() { inter / union } else { 0.0 }
}

/// How precise a source's box is; higher wins.
fn precision(s: Source) -> u8 {
    match s {
        Source::Font => 4,
        Source::Vector => 3,
        Source::Embedded => 1,
        #[allow(unreachable_patterns)]
        _ => 2,
    }
}

/// One diagram, after merging.
#[derive(Clone, Debug, PartialEq)]
pub struct Merged {
    /// The chosen box, source, score, DPI hint and position.
    pub candidate: DiagramCandidate,
    /// Every source that saw this diagram.
    pub sources: Vec<Source>,
}

/// Merge the candidates of one page.
pub fn merge(candidates: Vec<DiagramCandidate>) -> Vec<Merged> {
    // Union-find over overlapping candidates.
    let n = candidates.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        // Paths are halved as they are walked; at most n steps.
        for _ in 0..parent.len() {
            let Some(&p) = parent.get(i) else { break };
            if p == i {
                break;
            }
            let gp = parent.get(p).copied().unwrap_or(p);
            if let Some(slot) = parent.get_mut(i) {
                *slot = gp;
            }
            i = gp;
        }
        i
    }
    for i in 0..n {
        for j in i + 1..n {
            let (Some(a), Some(b)) = (candidates.get(i), candidates.get(j)) else { continue };
            if iou(&a.bbox_user, &b.bbox_user) >= OVERLAP_IOU {
                let (ri, rj) = (root(&mut parent, i), root(&mut parent, j));
                if let Some(slot) = parent.get_mut(rj) {
                    *slot = ri;
                }
            }
        }
    }
    let mut groups: Vec<(usize, Vec<DiagramCandidate>)> = Vec::new();
    for (i, c) in candidates.into_iter().enumerate() {
        let r = root(&mut parent, i);
        match groups.iter_mut().find(|(k, _)| *k == r) {
            Some((_, g)) => g.push(c),
            None => groups.push((r, vec![c])),
        }
    }

    let embedded_sides: Vec<f32> = groups
        .iter()
        .filter_map(|(_, g)| g.iter().find(|c| c.source == Source::Embedded))
        .map(|c| c.bbox_user.width().max(c.bbox_user.height()))
        .collect();
    let embedded_median = {
        let mut v = embedded_sides.clone();
        v.sort_by(f32::total_cmp);
        v.get(v.len() / 2).copied()
    };

    let mut out = Vec::new();
    for (_, group) in groups {
        let sources: Vec<Source> = {
            let mut s: Vec<Source> = group.iter().map(|c| c.source).collect();
            s.dedup();
            s
        };
        let placement = group.iter().find_map(|c| c.placement);
        let score = group.iter().map(|c| c.score).fold(0.0f32, f32::max);
        let dpi_hint = group.iter().map(|c| c.dpi_hint).fold(0.0f32, f32::max);
        let Some(best) = group.into_iter().max_by_key(|c| precision(c.source)) else { continue };
        if let Some(median) = embedded_median
            && !sources.contains(&Source::Embedded)
            && best.placement.is_none()
        {
            let side = best.bbox_user.width().max(best.bbox_user.height());
            if median > 0.0 && ((side - median).abs() / median) > EMBEDDED_SIZE_TOLERANCE {
                continue;
            }
        }
        out.push(Merged { candidate: DiagramCandidate { placement, score, dpi_hint, ..best }, sources });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdfcraft_chess::placement::Placement;

    fn cand(x: f32, y: f32, side: f32, source: Source) -> DiagramCandidate {
        DiagramCandidate { bbox_user: PageRect::new(x, y, x + side, y + side), source, score: 0.7, dpi_hint: 150.0, placement: None }
    }

    #[test]
    fn iou_basics() {
        let a = PageRect::new(0.0, 0.0, 10.0, 10.0);
        assert_eq!(iou(&a, &a), 1.0);
        assert_eq!(iou(&a, &PageRect::new(20.0, 20.0, 30.0, 30.0)), 0.0);
        assert!((iou(&a, &PageRect::new(5.0, 0.0, 15.0, 10.0)) - 1.0 / 3.0).abs() < 1e-6);
        assert_eq!(iou(&PageRect::new(0.0, 0.0, 0.0, 0.0), &a), 0.0);
    }

    #[test]
    fn overlapping_sources_make_one_diagram_with_the_precise_box() {
        let image = cand(100.0, 100.0, 160.0, Source::Embedded);
        let font = DiagramCandidate { placement: Some(Placement::standard()), score: 0.99, ..cand(104.0, 104.0, 152.0, Source::Font) };
        let merged = merge(vec![image, font.clone()]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].candidate.bbox_user, font.bbox_user);
        assert_eq!(merged[0].candidate.placement, Some(Placement::standard()));
        assert_eq!(merged[0].sources, vec![Source::Embedded, Source::Font]);
    }

    #[test]
    fn far_apart_candidates_stay_apart() {
        assert_eq!(merge(vec![cand(0.0, 0.0, 100.0, Source::Embedded), cand(300.0, 0.0, 100.0, Source::Embedded)]).len(), 2);
    }

    #[test]
    fn an_odd_sized_box_on_a_page_of_images_is_dropped() {
        let merged = merge(vec![
            cand(0.0, 0.0, 150.0, Source::Embedded),
            cand(300.0, 0.0, 150.0, Source::Embedded),
            cand(0.0, 400.0, 400.0, Source::Vector),
            cand(300.0, 400.0, 160.0, Source::Vector),
        ]);
        assert_eq!(merged.len(), 3, "the 400 pt vector box is not a diagram here");
    }

    #[test]
    fn empty_input() {
        assert!(merge(Vec::new()).is_empty());
    }
}
