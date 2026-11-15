//! Positions of the book by material and pawn structure (plan A9-N16a, finding A8-09): where the
//! search for one position looks ([`super::position_find`], plan A9-N3), and only there — the
//! book's PGN, each of its games played again once ([`ReferenceIndex::stretches`]: positions in a
//! row of one line are one stretch), and the diagrams already read, in this session or kept on
//! disk ([`Session::chess_matching_diagrams`]). Nothing here reads a page.
//!
//! **Memory.** Only what is found is kept: the stretches listed, at most `max`, and the diagrams,
//! 8 bytes each; a page kept on disk is read and dropped one at a time, and a game played again is
//! dropped before the next.

use pdfcraft_chess::pattern::Query;
use pdfcraft_chessvision::doc::reference::{ReferenceIndex, Stretches};

use super::ChessScanSettings;
use super::position_find::DiagramsFound;
use super::reading::ReadingStore;
use crate::{DocId, Session};

/// What [`Session::chess_pattern_find`] found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PatternFound {
    /// In the book's PGN, when one was given.
    pub pgn: Option<Stretches>,
    pub diagrams: DiagramsFound,
}

impl Session {
    /// The positions of document `id` that `query` finds (plan A9-N16a): the stretches of `book`
    /// (the book's PGN's index, when there is one) and the diagrams already read (this session's,
    /// and those `disk` keeps), at most `max` of each listed. Nothing is scanned. Fails as
    /// [`Session::chess_scan`] would: no such document, or one that does not allow copying.
    pub fn chess_pattern_find(
        &mut self,
        id: DocId,
        query: &Query,
        book: Option<&ReferenceIndex>,
        disk: Option<(&ReadingStore, &ChessScanSettings)>,
        max: usize,
    ) -> Result<PatternFound, String> {
        let wanted = |p: &pdfcraft_chessvision::board::ReadPosition| query.matches(&p.placement);
        let diagrams = self.chess_matching_diagrams(id, &wanted, disk, max)?;
        Ok(PatternFound { pgn: book.map(|b| b.stretches(query, max)), diagrams })
    }
}
