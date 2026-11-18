//! The printed move under a point of the page (plan A9-N1, §2.9 "Lance clicável no PDF"): what a
//! double-click on a move of a page read opens on the board, and what the pointer underlines
//! there. Pure, so the page in the app, the Text tab and the `chess_move_at` tool find the same
//! move from the same reading.
//!
//! The point is in PDF **user space** (points, y up), as every [`UserRect`] of the reading is: the
//! caller turns a point of the screen or of the displayed page into it with the page's one
//! mapping (its crop box and `/Rotate`), the inverse of the one that shows a move's box on the
//! page. A move is found by its own box ([`MoveInline::source_rect`]), which holds its number when
//! the number was printed on the same row; a number left at the end of the row above has no box
//! of its own in the reading, so pointing at it finds nothing.

use super::{ChessDoc, Inline, MoveAt, MoveInline, MoveStatus, UserRect};

/// The move printed at `point` (user space) on page `page` (0-based) of `doc`, with the position
/// after it (a FEN, when the walk played it). `None` when the page was not read, the point is not
/// finite, or no move's box holds it: prose, the gap between two moves and the margins find
/// nothing. A point on the edge of a box is in it. When boxes overlap, the move whose box's centre
/// is nearest the point is the one found.
pub fn move_at(doc: &ChessDoc, page: u32, point: [f32; 2]) -> Option<(MoveAt, Option<&str>)> {
    let [x, y] = point;
    if !(x.is_finite() && y.is_finite()) {
        return None;
    }
    let read = doc.pages.iter().find(|p| p.index == page)?;
    read.blocks
        .iter()
        .enumerate()
        .flat_map(|(b, block)| block.inlines().iter().enumerate().map(move |(i, inline)| (b, i, inline)))
        .filter_map(|(b, i, inline)| {
            let Inline::Move(m) = inline else { return None };
            let distance = distance_inside(m.source_rect?, x, y)?;
            Some((MoveAt::new(page, b, i), m, distance))
        })
        .min_by(|a, b| a.2.total_cmp(&b.2))
        .map(|(at, m, _)| (at, m.fen.as_deref()))
}

/// How far `(x, y)` is from the centre of `rect` (squared), when `rect` holds it. A box read from
/// JSON may have its corners swapped; one that is not finite holds no point. Measured in `f64`,
/// where the square of any finite `f32` distance is finite.
fn distance_inside(rect: UserRect, x: f32, y: f32) -> Option<f64> {
    if ![rect.x0, rect.y0, rect.x1, rect.y1].iter().all(|v| v.is_finite()) {
        return None;
    }
    let (x0, x1) = (f64::from(rect.x0.min(rect.x1)), f64::from(rect.x0.max(rect.x1)));
    let (y0, y1) = (f64::from(rect.y0.min(rect.y1)), f64::from(rect.y0.max(rect.y1)));
    let (x, y) = (f64::from(x), f64::from(y));
    if !(x0 <= x && x <= x1 && y0 <= y && y <= y1) {
        return None;
    }
    let (dx, dy) = (x - (x0 + x1) / 2.0, y - (y0 + y1) / 2.0);
    Some(dx * dx + dy * dy)
}

impl MoveInline {
    /// Whether its game can be put on the board from it, as a double-click on it in the Text tab
    /// or on the page does (plan A9-N1): it has a game, or it has no position on its page alone
    /// ([`MoveStatus::Unchecked`]: the top of a page going on from the page before), where the
    /// pages before are looked at for one. A move the walk refused, or a caption, opens nothing.
    pub fn opens_game(&self) -> bool {
        self.game.is_some() || self.status == MoveStatus::Unchecked
    }
}
