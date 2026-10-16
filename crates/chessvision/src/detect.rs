//! Diagram candidates from embedded images, page contours, diagram fonts and vector grids (blueprint S7a-S7c).
//!
//! Every detector returns [`DiagramCandidate`]s in user space. Each source is a submodule; the
//! candidates of different sources are joined later by `merge` and put in reading order by
//! `order`.

pub mod contour;
pub mod embedded;
pub mod font;
pub mod vector;

use pdfcraft_chess::placement::Placement;
use pdfcraft_geom::PageRect;

/// Which detector found a candidate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    /// An image the PDF embeds on the page ([`embedded`]).
    Embedded,
    /// A contour on the rendered page ([`contour`]).
    Contour,
    /// Text set in a chess diagram font ([`font`]); the position is read exactly.
    Font,
    /// A grid of filled squares drawn with vector paths ([`vector`]).
    Vector,
}

/// A region of a page that a detector believes holds one chess diagram.
#[derive(Clone, Debug, PartialEq)]
pub struct DiagramCandidate {
    /// The diagram's box in PDF user space (points, y up): the space of page images and links.
    pub bbox_user: PageRect,
    /// The detector that found it.
    pub source: Source,
    /// The detector's confidence in this box, in 0..=1. Not a reading confidence.
    pub score: f32,
    /// The resolution the source offers for this box, in dots per inch: rendering it finer
    /// than this adds no detail. Clamped to a sensible rendering range by each detector. A
    /// [`contour`] candidate cannot know the source's resolution and gives the DPI of the
    /// raster it was found on.
    pub dpi_hint: f32,
    /// The position, when the detector could read it without a model (a diagram font).
    pub placement: Option<Placement>,
    /// The board's four corners in user space, in the order top-left, top-right, bottom-right,
    /// bottom-left as the page is displayed, when the detector found a quadrilateral rather than
    /// a box (a turned or skewed board). The reader warps the board from them. `None` when the
    /// box is the board ([`embedded`]).
    pub corners: Option<[(f64, f64); 4]>,
}
