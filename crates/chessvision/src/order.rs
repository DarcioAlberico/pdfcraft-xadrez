//! Reading order of diagrams: the single ordering shared by the screen, export, links and tools.
//!
//! "Diagram 3" must be the same diagram on screen, in an exported PGN, in a link and in a tool
//! result; two orderings make it mean two things exactly when someone checks. This module is the
//! only place that orders diagrams.
//!
//! The default reads by column, as books with two columns of diagrams are read: candidates are
//! sorted by their centre across the page and grouped into a column while their centres stay
//! within [`COLUMN_GAP_SHARE`] of the column's median width; each column is read top to bottom.
//! Positions are taken in view space, so a rotated page is ordered as it is displayed.

use crate::coords::PageGeometry;
use crate::detect::DiagramCandidate;

/// How diagrams are numbered on a page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ReadingOrder {
    /// Column by column, each top to bottom.
    #[default]
    Column,
    /// Row by row, each left to right.
    Row,
}

/// A centre within this share of the median diagram width of a column's centre is in it.
pub const COLUMN_GAP_SHARE: f32 = 0.55;
/// The smallest column gap, in points, whatever the widths.
pub const MIN_COLUMN_GAP: f32 = 2.5;

/// The indices of `candidates` in reading order.
pub fn reading_order(candidates: &[DiagramCandidate], page: &PageGeometry, order: ReadingOrder) -> Vec<usize> {
    // (index, centre x, top y, width) in view space; a box that cannot be placed goes last.
    let mut placed: Vec<(usize, f32, f32, f32)> = Vec::new();
    let mut unplaced: Vec<usize> = Vec::new();
    for (i, c) in candidates.iter().enumerate() {
        match page.user_to_view(c.bbox_user) {
            Some(v) if v.x0.is_finite() && v.x1.is_finite() && v.y0.is_finite() => placed.push((i, (v.x0 + v.x1) / 2.0, v.y0, v.width())),
            _ => unplaced.push(i),
        }
    }
    match order {
        ReadingOrder::Row => {
            placed.sort_by(|a, b| a.2.total_cmp(&b.2).then(a.1.total_cmp(&b.1)));
        }
        ReadingOrder::Column => {
            placed.sort_by(|a, b| a.1.total_cmp(&b.1));
            let mut widths: Vec<f32> = placed.iter().map(|p| p.3).collect();
            widths.sort_by(f32::total_cmp);
            let median = widths.get(widths.len() / 2).copied().unwrap_or(0.0);
            let gap = (median * COLUMN_GAP_SHARE).max(MIN_COLUMN_GAP);
            let mut columns: Vec<Vec<(usize, f32, f32, f32)>> = Vec::new();
            for p in placed.drain(..) {
                let joins = columns.last().is_some_and(|col| {
                    let mean = col.iter().map(|q| q.1).sum::<f32>() / col.len() as f32;
                    (p.1 - mean).abs() <= gap
                });
                match columns.last_mut() {
                    Some(col) if joins => col.push(p),
                    _ => columns.push(vec![p]),
                }
            }
            for mut col in columns {
                col.sort_by(|a, b| a.2.total_cmp(&b.2).then(a.1.total_cmp(&b.1)));
                placed.extend(col);
            }
        }
    }
    placed.into_iter().map(|p| p.0).chain(unplaced).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::Source;
    use pdfcraft_geom::{PageRect, Rotation};

    fn cand(x: f32, y_top_view: f32) -> DiagramCandidate {
        // A 150 pt box whose top is at `y_top_view` from the top of an 800 pt page.
        let y1 = 800.0 - y_top_view;
        DiagramCandidate {
            bbox_user: PageRect::new(x, y1 - 150.0, x + 150.0, y1),
            source: Source::Embedded,
            score: 1.0,
            dpi_hint: 150.0,
            placement: None,
            corners: None,
        }
    }

    fn page(rotation: Rotation) -> PageGeometry {
        PageGeometry::new(PageRect::new(0.0, 0.0, 600.0, 800.0), rotation).unwrap()
    }

    #[test]
    fn two_columns_are_read_column_by_column() {
        // Left column at x 50, right at x 350; three rows.
        let cs = vec![cand(350.0, 50.0), cand(50.0, 400.0), cand(50.0, 50.0), cand(350.0, 400.0), cand(52.0, 220.0)];
        assert_eq!(reading_order(&cs, &page(Rotation::R0), ReadingOrder::Column), vec![2, 4, 1, 0, 3]);
        assert_eq!(reading_order(&cs, &page(Rotation::R0), ReadingOrder::Row), vec![2, 0, 4, 1, 3]);
    }

    #[test]
    fn the_order_is_a_permutation_even_with_bad_boxes() {
        let mut cs = vec![cand(50.0, 50.0), cand(350.0, 50.0)];
        // Built field by field: `PageRect::new` would already discard the NaN.
        cs.push(DiagramCandidate { bbox_user: PageRect { x0: f32::NAN, y0: 0.0, x1: 1.0, y1: 1.0 }, ..cand(0.0, 0.0) });
        let mut order = reading_order(&cs, &page(Rotation::R0), ReadingOrder::Column);
        assert_eq!(order.last(), Some(&2));
        order.sort_unstable();
        assert_eq!(order, vec![0, 1, 2]);
        assert!(reading_order(&[], &page(Rotation::R0), ReadingOrder::Column).is_empty());
    }
}
