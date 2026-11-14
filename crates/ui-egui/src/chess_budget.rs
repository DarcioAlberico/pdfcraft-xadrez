//! Chess ▸ the columns a window's width allows (plan A9-I0a, §2.8.0; finding R4-03): the chess
//! surfaces are one column, the Inspector's, and how wide it is and what stays beside it is
//! decided by the window's width in points (after the interface's size), when a chess surface
//! opens:
//!
//! - under 1,000 pt ([`Budget::Narrow`]: 1366 × 768 at 150 % or more), the page **or** the
//!   Chess column, never both: the column takes the page's place, and F6 or the column's Page
//!   button switch between them, the keyboard going along;
//! - from 1,000 pt ([`Budget::One`]), one column of [`COLUMN`] points;
//! - from 1,600 pt ([`Budget::Two`]), two: the Chess column, and beside it the diagram being
//!   edited, else the board;
//! - from 1,920 pt ([`Budget::Wide`]), two of [`WIDE_COLUMN`] points (plan A9-I0c, finding R4-13:
//!   a large screen gave the editor's board 280 pt and left 400 pt blank under it).
//!
//! Below 1,920 pt the Tools panel is collapsed, once a session, with a notice that brings it back
//! ("Tools panel collapsed · Undo"); from 1,920 it may stay. The budget never undoes what the
//! user did in the session: the Inspector resized, moved or floated keeps its size, and the
//! Tools reopened stay open (decision 3 of I0a).

/// The Chess column's width, in points.
pub(crate) const COLUMN: f32 = 360.0;

/// The columns' width from 1,920 pt (plan A9-I0c).
pub(crate) const WIDE_COLUMN: f32 = 420.0;

/// What a window's width allows the chess surfaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Budget {
    /// The page or the Chess column.
    Narrow,
    /// One column beside the page.
    One,
    /// Two columns beside the page.
    Two,
    /// Two columns, the Tools left open.
    Wide,
}

impl Budget {
    /// The budget of a window `width` points wide (not a number: the narrowest).
    pub(crate) fn of(width: f32) -> Budget {
        if width.is_nan() || width < 1000.0 {
            Budget::Narrow
        } else if width < 1600.0 {
            Budget::One
        } else if width < 1920.0 {
            Budget::Two
        } else {
            Budget::Wide
        }
    }

    /// How many columns the Chess column has.
    pub(crate) fn columns(self) -> usize {
        match self {
            Budget::Narrow | Budget::One => 1,
            Budget::Two | Budget::Wide => 2,
        }
    }

    /// One column's width: [`COLUMN`], or [`WIDE_COLUMN`] from 1,920 pt.
    pub(crate) fn column(self) -> f32 {
        if self == Budget::Wide { WIDE_COLUMN } else { COLUMN }
    }

    /// The Inspector's width for the Chess column; none where it takes the page's place.
    pub(crate) fn width(self) -> Option<f32> {
        match self {
            Budget::Narrow => None,
            other => Some(other.column() * other.columns() as f32),
        }
    }

    /// Whether the Tools panel is collapsed when a chess surface opens.
    pub(crate) fn collapses_tools(self) -> bool {
        self != Budget::Wide
    }
}

/// Where F6 below 1,000 pt sends the keyboard, when it is drawn: the Chess column (going to the
/// page, the keyboard is only let go).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FocusTo {
    Column,
}

/// The budget's state in the session.
#[derive(Debug, Default)]
pub struct BudgetState {
    /// The Tools panel was collapsed for a chess surface this session (it is done once).
    pub(crate) tools_collapsed: bool,
    /// The user resized, moved, floated or closed a panel of the workspace this session: the
    /// budget leaves the Inspector's width alone.
    pub(crate) user_sized: bool,
    /// The Inspector's width to set when the workspace is next drawn.
    pub(crate) pending_width: Option<f32>,
    /// Under 1,000 pt, the page shows instead of the Chess column.
    pub show_page: bool,
    /// Under 1,000 pt with the Chess column chosen, as last drawn: F6 switches the page and the
    /// column.
    pub(crate) alternating: bool,
    /// Where F6 sent the keyboard, done when the place is next drawn.
    pub(crate) focus_to: Option<FocusTo>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The plan's breakpoints, in points, and their edges.
    #[test]
    fn the_width_decides_the_columns() {
        for (width, budget) in [
            (680.0, Budget::Narrow),
            (999.9, Budget::Narrow),
            (1000.0, Budget::One),
            (1360.0, Budget::One),
            (1599.0, Budget::One),
            (1600.0, Budget::Two),
            (1919.0, Budget::Two),
            (1920.0, Budget::Wide),
            (f32::INFINITY, Budget::Wide),
            (f32::NAN, Budget::Narrow),
            (-5.0, Budget::Narrow),
        ] {
            assert_eq!(Budget::of(width), budget, "{width}");
        }
        assert_eq!((Budget::One.width(), Budget::Two.width(), Budget::Narrow.width()), (Some(360.0), Some(720.0), None));
        assert_eq!((Budget::Wide.column(), Budget::Wide.width()), (420.0, Some(840.0)), "420 pt columns from 1,920 (plan A9-I0c)");
        assert!(Budget::Two.collapses_tools() && !Budget::Wide.collapses_tools());
    }
}
