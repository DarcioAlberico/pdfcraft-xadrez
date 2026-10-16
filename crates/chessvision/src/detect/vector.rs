//! Boards drawn with vector paths: a lattice of filled squares in a checker pattern.
//!
//! Some books draw the board itself as 32 (or 64) filled rectangles and put the pieces on top as
//! glyphs or images. Those rectangles are found in the page's path inventory (the engine adapts
//! `edit::paths` into [`FilledBox`]es). A board is at least 28 same-size squares that sit on an
//! 8×8 lattice, all on squares of one colour of that lattice, spanning all eight files and
//! ranks. The position is not read here: the pieces are drawn separately, and reading them is
//! the board reader's job.

use pdfcraft_geom::PageRect;

use super::{DiagramCandidate, Source};

/// A filled rectangle on the page.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FilledBox {
    /// Its box in user space.
    pub bbox_user: PageRect,
    /// Its fill as a gray level 0 (black) ..= 1 (white), when known.
    pub gray: Option<f32>,
}

/// Most boxes looked at on one page.
pub const MAX_BOXES: usize = 50_000;
/// Fewest squares of the 32 of one colour that must be present.
pub const MIN_SQUARES: usize = 28;
/// Most same-shade squares allowed on the other colour of the lattice (highlighted squares).
const MAX_OFF_PATTERN: usize = 4;
/// Squares are the same size, and on the lattice, within this share of a side.
const TOLERANCE: f32 = 0.06;
/// A box is square when its sides differ by less than this share.
const SQUARE_ASPECT: f32 = 0.08;

/// Boards drawn as lattices of filled squares.
pub fn candidates(boxes: &[FilledBox]) -> Vec<DiagramCandidate> {
    let squares: Vec<FilledBox> = boxes
        .iter()
        .take(MAX_BOXES)
        .filter(|b| {
            let (w, h) = (b.bbox_user.width(), b.bbox_user.height());
            w.is_finite() && h.is_finite() && w > 1.0 && h > 1.0 && (w - h).abs() <= SQUARE_ASPECT * w.max(h)
        })
        .copied()
        .collect();
    // Group by size; a board's squares share one.
    let mut sizes: Vec<(f32, Vec<FilledBox>)> = Vec::new();
    for b in squares {
        let side = b.bbox_user.width();
        match sizes.iter_mut().find(|(s, _)| (side - *s).abs() <= TOLERANCE * s.max(side)) {
            Some((_, group)) => group.push(b),
            None => sizes.push((side, vec![b])),
        }
    }
    let mut out = Vec::new();
    for (side, group) in sizes {
        if group.len() < MIN_SQUARES {
            continue;
        }
        // Light and dark squares may both be drawn; keep each gray level apart and look for a
        // lattice in each, darkest first.
        let mut shades: Vec<(Option<f32>, Vec<FilledBox>)> = Vec::new();
        for b in group {
            match shades.iter_mut().find(|(g, _)| match (g, b.gray) {
                (Some(a), Some(c)) => (*a - c).abs() < 0.05,
                (None, None) => true,
                _ => false,
            }) {
                Some((_, v)) => v.push(b),
                None => shades.push((b.gray, vec![b])),
            }
        }
        shades.sort_by(|a, b| a.0.unwrap_or(0.0).total_cmp(&b.0.unwrap_or(0.0)));
        for (_, shade) in shades {
            out.extend(lattices(&shade, side));
        }
    }
    // A board drawn in two shades is found twice; keep the first, which is the darker shade.
    let mut kept: Vec<DiagramCandidate> = Vec::new();
    for c in out {
        if !kept.iter().any(|k| crate::merge::iou(&k.bbox_user, &c.bbox_user) > 0.5) {
            kept.push(c);
        }
    }
    kept
}

/// 8×8 lattices among same-size, same-shade squares.
fn lattices(squares: &[FilledBox], side: f32) -> Vec<DiagramCandidate> {
    let mut used = vec![false; squares.len()];
    let mut out = Vec::new();
    for (i, anchor) in squares.iter().enumerate() {
        if used.get(i).copied().unwrap_or(true) {
            continue;
        }
        // The squares near this one, within a board's reach.
        let reach = 8.0 * side;
        let near: Vec<usize> = (0..squares.len())
            .filter(|&j| {
                !used.get(j).copied().unwrap_or(true)
                    && squares
                        .get(j)
                        .is_some_and(|b| (b.bbox_user.x0 - anchor.bbox_user.x0).abs() < reach && (b.bbox_user.y0 - anchor.bbox_user.y0).abs() < reach)
            })
            .collect();
        if near.len() < MIN_SQUARES {
            continue;
        }
        let x0 = near.iter().filter_map(|&j| squares.get(j)).map(|b| b.bbox_user.x0).fold(f32::INFINITY, f32::min);
        let y0 = near.iter().filter_map(|&j| squares.get(j)).map(|b| b.bbox_user.y0).fold(f32::INFINITY, f32::min);
        let mut cells = [[false; 8]; 8];
        let mut parity: Option<usize> = None;
        let mut members = Vec::new();
        // Squares of the same shade on the other colour: a full grid (a table), not a board.
        let mut off_pattern = 0usize;
        for &j in &near {
            let Some(b) = squares.get(j) else { continue };
            let fi = (b.bbox_user.x0 - x0) / side;
            let fj = (b.bbox_user.y0 - y0) / side;
            let (ci, cj) = (fi.round(), fj.round());
            if (fi - ci).abs() > TOLERANCE || (fj - cj).abs() > TOLERANCE || !(0.0..8.0).contains(&ci) || !(0.0..8.0).contains(&cj) {
                continue;
            }
            let (ci, cj) = (ci as usize, cj as usize);
            let p = (ci + cj) % 2;
            if *parity.get_or_insert(p) != p {
                off_pattern += 1;
                continue;
            }
            if let Some(cell) = cells.get_mut(cj).and_then(|row| row.get_mut(ci)) {
                *cell = true;
            }
            members.push(j);
        }
        let count = cells.iter().flatten().filter(|c| **c).count();
        let spans = |axis: &dyn Fn(usize, usize) -> usize| {
            let mut seen = [false; 8];
            for (cj, row) in cells.iter().enumerate() {
                for (ci, on) in row.iter().enumerate() {
                    if *on && let Some(s) = seen.get_mut(axis(ci, cj)) {
                        *s = true;
                    }
                }
            }
            seen.iter().all(|s| *s)
        };
        if count >= MIN_SQUARES && off_pattern <= MAX_OFF_PATTERN && spans(&|ci, _| ci) && spans(&|_, cj| cj) {
            for j in members {
                if let Some(u) = used.get_mut(j) {
                    *u = true;
                }
            }
            out.push(DiagramCandidate {
                bbox_user: PageRect::new(x0, y0, x0 + 8.0 * side, y0 + 8.0 * side),
                source: Source::Vector,
                score: 0.6 + 0.4 * (count as f32 / 32.0).min(1.0),
                dpi_hint: 300.0,
                placement: None,
                corners: None,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The 32 squares of one colour of a board at (x, y) with side s, gray g.
    fn board(x: f32, y: f32, s: f32, parity: usize, gray: f32) -> Vec<FilledBox> {
        let mut v = Vec::new();
        for j in 0..8 {
            for i in 0..8 {
                if (i + j) % 2 == parity {
                    let (bx, by) = (x + i as f32 * s, y + j as f32 * s);
                    v.push(FilledBox { bbox_user: PageRect::new(bx, by, bx + s, by + s), gray: Some(gray) });
                }
            }
        }
        v
    }

    #[test]
    fn a_lattice_of_dark_squares_is_a_board() {
        let found = candidates(&board(100.0, 300.0, 18.0, 1, 0.5));
        assert_eq!(found.len(), 1);
        let c = &found[0];
        assert_eq!(c.source, Source::Vector);
        assert_eq!(c.bbox_user, PageRect::new(100.0, 300.0, 244.0, 444.0));
        assert!(c.placement.is_none());
    }

    #[test]
    fn both_shades_drawn_give_one_board() {
        let mut boxes = board(100.0, 300.0, 18.0, 1, 0.5);
        boxes.extend(board(100.0, 300.0, 18.0, 0, 0.95));
        assert_eq!(candidates(&boxes).len(), 1);
    }

    #[test]
    fn a_few_missing_squares_are_tolerated_but_not_many() {
        let mut boxes = board(0.0, 0.0, 20.0, 1, 0.4);
        boxes.truncate(29);
        // Dropping from the end loses squares of the last ranks; keep the span by re-adding one.
        boxes.push(FilledBox { bbox_user: PageRect::new(140.0, 140.0, 160.0, 160.0), gray: Some(0.4) });
        assert_eq!(candidates(&boxes).len(), 1);
        let few: Vec<FilledBox> = board(0.0, 0.0, 20.0, 1, 0.4).into_iter().take(20).collect();
        assert!(candidates(&few).is_empty());
    }

    #[test]
    fn a_full_grid_table_is_not_a_board() {
        // 64 cells of one shade: both parities, so not a checker pattern.
        let mut boxes = board(0.0, 0.0, 20.0, 0, 0.9);
        boxes.extend(board(0.0, 0.0, 20.0, 1, 0.9));
        assert!(candidates(&boxes).is_empty());
    }

    #[test]
    fn two_boards_are_two_candidates() {
        let mut boxes = board(50.0, 50.0, 15.0, 1, 0.5);
        boxes.extend(board(300.0, 50.0, 15.0, 1, 0.5));
        assert_eq!(candidates(&boxes).len(), 2);
    }

    #[test]
    fn junk_does_not_panic() {
        let junk: Vec<FilledBox> = (0..3000)
            .map(|i| {
                let v = (i * 31 % 500) as f32;
                let s = if i % 11 == 0 { f32::NAN } else { (i % 9) as f32 };
                FilledBox { bbox_user: PageRect::new(v, v * 0.7, v + s, v * 0.7 + s), gray: None }
            })
            .collect();
        let _ = candidates(&junk);
    }
}
