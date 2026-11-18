//! Where a chess board is drawn and from which side (plan A9-I6, findings A5-19 and A7-19): the one
//! rule from a point on screen to a square and back. The board panel, the Diagram tab's editor,
//! the study board and the arrows each had a copy of it, and a change of orientation made in one
//! of them would have sent a click to another square on the others.

use egui::{Pos2, Rect, vec2};
use pdfcraft_engine::chess::pdfcraft_chess::placement::Square;

/// A board drawn in `rect` (square), White at the bottom unless `flipped`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BoardFrame {
    pub(crate) rect: Rect,
    pub(crate) flipped: bool,
}

impl BoardFrame {
    /// A board of no size seen from Black's side when `flipped`: for where a square is shown
    /// (column and row) and back, as the keyboard's cursor moves.
    pub(crate) fn seen_from(flipped: bool) -> BoardFrame {
        BoardFrame { rect: Rect::NOTHING, flipped }
    }

    /// The square drawn under `p`; none outside the board, or for a board of no size.
    pub(crate) fn square_at(self, p: Pos2) -> Option<Square> {
        let (w, h) = (self.rect.width(), self.rect.height());
        if !(w > 0.0 && h > 0.0) {
            return None;
        }
        let (x, y) = ((p.x - self.rect.min.x) / w, (p.y - self.rect.min.y) / h);
        if !(x.is_finite() && y.is_finite() && (0.0..1.0).contains(&x) && (0.0..1.0).contains(&y)) {
            return None;
        }
        // In 0..8 after the checks above, so the casts cannot wrap.
        self.square_of((x * 8.0) as u8, (y * 8.0) as u8)
    }

    /// The square shown at column `col` and row `row`, counted from the top left (0..8).
    pub(crate) fn square_of(self, col: u8, row: u8) -> Option<Square> {
        if self.flipped { Square::new(7u8.checked_sub(col)?, row) } else { Square::new(col, 7u8.checked_sub(row)?) }
    }

    /// The column and row, from the top left, `square` is shown at.
    pub(crate) fn place_of(self, square: Square) -> (u8, u8) {
        let (file, rank) = (square.file(), square.rank());
        if self.flipped { (7u8.saturating_sub(file), rank) } else { (file, 7u8.saturating_sub(rank)) }
    }

    /// The rectangle `square` is drawn in.
    pub(crate) fn square_rect(self, square: Square) -> Rect {
        let (col, row) = self.place_of(square);
        let cell = self.rect.width() / 8.0;
        Rect::from_min_size(self.rect.min + vec2(f32::from(col) * cell, f32::from(row) * cell), vec2(cell, cell))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_squares() -> impl Iterator<Item = Square> {
        (0..8u8).flat_map(|file| (0..8u8).filter_map(move |rank| Square::new(file, rank)))
    }

    /// Plan A9-I6's check: every square's rectangle's centre is that square again, from both
    /// sides, on the board panel's frame and on the Diagram tab editor's.
    #[test]
    fn a_squares_centre_is_that_square_on_both_boards_and_both_sides() {
        let rect = Rect::from_min_size(Pos2::new(13.0, 27.0), vec2(333.0, 333.0));
        let mut board = crate::chess_board_ui::BoardState::default();
        let mut editor = crate::chess_editor::DiagramEditor::new(pdfcraft_engine::DocId(1), 0, 0, None);
        for flipped in [false, true] {
            board.flipped = flipped;
            editor.flipped = flipped;
            for frame in [board.frame(rect), editor.frame(rect)] {
                assert_eq!(frame, BoardFrame { rect, flipped });
                let mut seen = 0;
                for sq in all_squares() {
                    assert_eq!(frame.square_at(frame.square_rect(sq).center()), Some(sq), "{sq:?}, flipped {flipped}");
                    let (col, row) = frame.place_of(sq);
                    assert_eq!(frame.square_of(col, row), Some(sq));
                    seen += 1;
                }
                assert_eq!(seen, 64);
            }
        }
    }

    #[test]
    fn the_corners_follow_the_side_and_outside_is_no_square() {
        let unit = |flipped| BoardFrame { rect: Rect::from_min_size(Pos2::ZERO, vec2(80.0, 80.0)), flipped };
        let sq = |name: &str| Square::parse(name);
        assert_eq!(unit(false).square_at(Pos2::new(1.0, 79.0)), sq("a1"));
        assert_eq!(unit(true).square_at(Pos2::new(1.0, 79.0)), sq("h8"));
        assert_eq!(unit(true).square_rect(Square::H1).max, Pos2::new(10.0, 10.0), "h1 is the top left square from Black's side");
        assert_eq!(unit(false).square_at(Pos2::new(80.0, 40.0)), None);
        assert_eq!(unit(false).square_at(Pos2::new(f32::NAN, 40.0)), None);
        assert_eq!(BoardFrame { rect: Rect::from_min_size(Pos2::ZERO, vec2(0.0, 0.0)), flipped: false }.square_at(Pos2::ZERO), None);
        assert_eq!(unit(false).square_of(8, 0), None);
        // The board panel's copy gave these before the change (plan A9-I6).
        let b7 = Square::parse("b7").unwrap();
        assert_eq!((unit(false).square_rect(b7).min, unit(true).square_rect(b7).min), (Pos2::new(10.0, 10.0), Pos2::new(60.0, 60.0)));
    }

    /// Plan A9-I6: the Diagram tab editor's cursor moves as the board is shown, through the same
    /// rule, and stops at the edges, as its own copy did before the change.
    #[test]
    fn the_editors_cursor_moves_as_shown() {
        let mut ed = crate::chess_editor::DiagramEditor::new(pdfcraft_engine::DocId(1), 0, 0, None);
        for (flipped, from, to) in [(false, "a8", "b7"), (true, "h1", "g2"), (false, "h1", "h1"), (true, "a8", "a8")] {
            ed.flipped = flipped;
            ed.cursor = Square::parse(from).unwrap();
            ed.move_cursor(1, 1);
            assert_eq!(Some(ed.cursor), Square::parse(to), "flipped {flipped}, from {from}");
        }
        ed.flipped = false;
        ed.cursor = Square::parse("d4").unwrap();
        ed.move_cursor(i8::MIN, i8::MAX);
        assert_eq!(Some(ed.cursor), Square::parse("a1"), "far past the edges, to the corner");
    }
}
