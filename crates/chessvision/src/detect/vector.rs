//! Boards drawn with vector paths: a lattice of filled squares in a checker pattern.
//!
//! Some books draw the board itself as 32 (or 64) filled rectangles and put the pieces on top as
//! glyphs or images. Those rectangles are found in the page's path inventory (the engine adapts
//! `edit::paths` into [`FilledBox`]es). A board is at least 28 same-size squares that sit on an
//! 8×8 lattice, all on squares of one colour of that lattice, spanning all eight files and
//! ranks. The position is not read here: the pieces are drawn separately, and reading them is
//! the board reader's job.

use std::collections::HashMap;

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

/// The cell of a grid of `reach`-sided cells that holds `(x, y)`; a coordinate too large for
/// the grid's numbers falls in its last cell.
fn grid_cell(x: f32, y: f32, reach: f32) -> (i64, i64) {
    // `as` saturates a float into an integer, and makes a NaN 0.
    ((x / reach).floor() as i64, (y / reach).floor() as i64)
}

/// The squares not `used` within `reach` of `anchor` (both ways, by their corners), in their
/// order, looked for in `grid` (the squares' indexes by [`grid_cell`]): in the anchor's cell and
/// the cells two around it (one would do but for a division's rounding), not among all the page's
/// squares (plan A9-V7, A3-08: every anchor looked at every square, and a table of 50,000 took
/// 3 s).
fn near(squares: &[FilledBox], used: &[bool], anchor: &FilledBox, reach: f32, grid: &HashMap<(i64, i64), Vec<usize>>) -> Vec<usize> {
    let within = |j: usize| {
        !used.get(j).copied().unwrap_or(true)
            && squares
                .get(j)
                .is_some_and(|b| (b.bbox_user.x0 - anchor.bbox_user.x0).abs() < reach && (b.bbox_user.y0 - anchor.bbox_user.y0).abs() < reach)
    };
    let (cx, cy) = grid_cell(anchor.bbox_user.x0, anchor.bbox_user.y0, reach);
    let mut near: Vec<usize> = (-2..=2)
        .flat_map(|dy| (-2..=2).map(move |dx| (cx.saturating_add(dx), cy.saturating_add(dy))))
        .filter_map(|cell| grid.get(&cell))
        .flatten()
        .copied()
        .filter(|&j| within(j))
        .collect();
    // A cell met twice at the grid's edge is counted once.
    near.sort_unstable();
    near.dedup();
    near
}

/// 8×8 lattices among same-size, same-shade squares.
fn lattices(squares: &[FilledBox], side: f32) -> Vec<DiagramCandidate> {
    let mut used = vec![false; squares.len()];
    let mut out = Vec::new();
    // A board's reach, and the squares by cell of a grid of that side. With no reach, no square
    // is near another, and the grid stays empty.
    let reach = 8.0 * side;
    let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    if reach > 0.0 && reach.is_finite() {
        for (j, b) in squares.iter().enumerate() {
            grid.entry(grid_cell(b.bbox_user.x0, b.bbox_user.y0, reach)).or_default().push(j);
        }
    }
    for (i, anchor) in squares.iter().enumerate() {
        if used.get(i).copied().unwrap_or(true) {
            continue;
        }
        // The squares near this one, within a board's reach.
        let near = near(squares, &used, anchor, reach, &grid);
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

    use proptest::prelude::*;

    /// [`lattices`]' search for the squares near an anchor before plan A9-V7: every square.
    fn near_one_by_one(squares: &[FilledBox], used: &[bool], anchor: &FilledBox, reach: f32) -> Vec<usize> {
        (0..squares.len())
            .filter(|&j| {
                !used.get(j).copied().unwrap_or(true)
                    && squares
                        .get(j)
                        .is_some_and(|b| (b.bbox_user.x0 - anchor.bbox_user.x0).abs() < reach && (b.bbox_user.y0 - anchor.bbox_user.y0).abs() < reach)
            })
            .collect()
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        /// Plan A9-V7 (A3-08): for every anchor, the grid finds the squares a look at every square
        /// finds, on boards (whole or cut) among stray squares anywhere, some used; the rest of
        /// the lattice search is as it was.
        #[test]
        fn lattices_are_those_a_look_at_every_square_finds(
            boards in proptest::collection::vec((0.0f32..400.0, 0.0f32..600.0, 0usize..2, 20usize..33), 0..3),
            strays in proptest::collection::vec((-50.0f32..700.0, -50.0f32..900.0), 0..80),
            side in 6.0f32..24.0,
        ) {
            let mut squares: Vec<FilledBox> = Vec::new();
            for &(x, y, parity, keep) in &boards {
                squares.extend(board(x, y, side, parity, 0.5).into_iter().take(keep));
            }
            for &(x, y) in &strays {
                squares.push(FilledBox { bbox_user: PageRect::new(x, y, x + side, y + side), gray: Some(0.5) });
            }
            let reach = 8.0 * side;
            let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
            for (j, b) in squares.iter().enumerate() {
                grid.entry(grid_cell(b.bbox_user.x0, b.bbox_user.y0, reach)).or_default().push(j);
            }
            let used: Vec<bool> = (0..squares.len()).map(|j| j % 7 == 3).collect();
            for anchor in &squares {
                prop_assert_eq!(near(&squares, &used, anchor, reach, &grid), near_one_by_one(&squares, &used, anchor, reach));
            }
        }
    }

    /// Plan A9-V7 (A3-08, probe c04): a table of 50,000 equal squares, the most a page is read
    /// for, is looked through in under a second in release (3 s before).
    /// `cargo test --release -p pdfcraft-chessvision --lib perf_vector_table -- --ignored`.
    #[test]
    #[ignore = "a timing, in release"]
    fn perf_vector_table_of_squares() {
        let boxes: Vec<FilledBox> = (0..MAX_BOXES)
            .map(|k| {
                let (x, y) = ((k % 250) as f32 * 12.0, (k / 250) as f32 * 12.0);
                FilledBox { bbox_user: PageRect::new(x, y, x + 10.0, y + 10.0), gray: Some(0.5) }
            })
            .collect();
        let t = std::time::Instant::now();
        let found = candidates(&boxes);
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        println!("PERF vector_table_50k {ms:.0}");
        assert!(found.is_empty(), "a table is no board");
        assert!(ms < 1000.0, "{ms:.0} ms");
    }

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
