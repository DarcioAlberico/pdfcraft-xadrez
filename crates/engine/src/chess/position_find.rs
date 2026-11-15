//! "This position in the book" (plan A9-N3, finding A8-07; §2.9): where a position stands among
//! the diagrams already read of a book, in this session (G11) or kept on disk (N5). The book's
//! PGN answers through its compact index
//! ([`pdfcraft_chessvision::doc::reference::ReferenceIndex::occurrences`]).
//!
//! **Never a reading.** Nothing here scans, renders or reads a page: a page no session read and
//! no disk keeps is not looked at, and the answer counts the pages it looked at, so "nothing read"
//! is said and never taken for "not in the book" (reading a whole book took 4.7 to 12 minutes, R8).
//!
//! **Memory.** Only the diagrams that match are kept, a [`DiagramFound`] of 8 bytes each; the
//! position looked for is one [`BookKey`], compared with each reading's as it is met. A page kept
//! on disk is read and dropped one at a time.

use std::collections::BTreeMap;
use std::sync::Arc;

use pdfcraft_chess::fen::{BookKey, Fen};

use super::reading::{self, ReadingStore};
use super::{ChessPage, ChessScanSettings};
use crate::{DocId, Session};

/// A diagram read as the position looked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiagramFound {
    /// The page, 0-based.
    pub page: u32,
    /// The diagram's number on the page, 0-based, in the reading order of the scan that read it.
    pub index: u16,
    /// Its reading assumed the side to move: only the pieces were compared.
    pub side_assumed: bool,
    /// Kept on disk, not read in this session.
    pub from_disk: bool,
}

/// What [`Session::chess_position_diagrams`] found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiagramsFound {
    /// The first ones, page by page, at most the `max` asked for.
    pub found: Vec<DiagramFound>,
    /// How many diagrams are the position, all of them.
    pub total: usize,
    /// The pages whose diagrams were looked at: read in this session or kept on disk.
    pub pages_read: usize,
    /// Of those, the pages taken from the disk.
    pub from_disk: usize,
}

impl Session {
    /// The diagrams of document `id` already read whose position is the one keyed `key`: the
    /// same pieces, and the same side to move unless `any_side` or the reading assumed it
    /// ([`BookKey::matches`]), at most `max` listed (plan A9-N3). From this session's scans (any
    /// settings or model; a page with several readings counts once, with its first) and, for the
    /// pages it has not read, the readings `disk` keeps for `settings` and the piece model they
    /// name ([`reading::scan_key`]). Nothing is scanned. Fails as [`Session::chess_scan`] would:
    /// no such document, or one that does not allow copying.
    pub fn chess_position_diagrams(
        &mut self,
        id: DocId,
        key: BookKey,
        any_side: bool,
        disk: Option<(&ReadingStore, &ChessScanSettings)>,
        max: usize,
    ) -> Result<DiagramsFound, String> {
        let doc = self.get(id).ok_or("no such document")?;
        doc.check_copying().map_err(|e| e.to_string())?;
        let (count, keys) = (doc.info.pages.len(), doc.doc_keys());
        // This session's readings, a page's first (the cache holds one per settings and model).
        let mut pages: BTreeMap<usize, (Arc<ChessPage>, bool)> = BTreeMap::new();
        for (page, _, result) in self.chess_cached(id, &[]).entries {
            if result.readings.is_some() {
                pages.entry(page).or_insert((result, false));
            }
        }
        if let Some((store, settings)) = disk {
            let missing: Vec<usize> = (0..count).filter(|p| !pages.contains_key(p)).collect();
            let models = super::models(settings.models_dir.as_deref());
            let contents = if missing.is_empty() { Vec::new() } else { self.chess_disk_keys(id, &missing) };
            for (page, content) in missing.into_iter().zip(contents) {
                let Some(entry) = content.and_then(|c| reading::scan_key(&c, settings, Some(&*models))) else { continue };
                if let reading::Found::Hit(kept) = store.get::<ChessPage>(&keys, &entry)
                    && kept.readings.is_some()
                {
                    pages.insert(page, (Arc::new(kept), true));
                }
            }
        }
        Ok(find_in_pages(key, any_side, pages.iter().map(|(p, (r, d))| (*p, &**r, *d)), max))
    }
}

/// The diagrams of `pages` (page, its reading, whether it came from the disk; in page order)
/// whose reading is the position keyed `key`; see [`Session::chess_position_diagrams`].
fn find_in_pages<'a>(key: BookKey, any_side: bool, pages: impl IntoIterator<Item = (usize, &'a ChessPage, bool)>, max: usize) -> DiagramsFound {
    let mut out = DiagramsFound::default();
    for (page, read, from_disk) in pages {
        let Some(readings) = read.readings.as_ref() else { continue };
        out.pages_read = out.pages_read.saturating_add(1);
        out.from_disk = out.from_disk.saturating_add(usize::from(from_disk));
        for (index, reading) in readings.iter().enumerate() {
            let Some(position) = reading.position() else { continue };
            let Ok(fen) = Fen::parse(&position.fen) else { continue };
            let side_assumed = position.side.is_assumed();
            if !fen.book_key().matches(key, any_side || side_assumed) {
                continue;
            }
            out.total = out.total.saturating_add(1);
            if out.found.len() < max
                && let (Ok(page), Ok(index)) = (u32::try_from(page), u16::try_from(index))
            {
                out.found.push(DiagramFound { page, index, side_assumed, from_disk });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a found diagram is kept as stays within the plan's 64 bytes a position.
    #[test]
    fn a_diagram_found_is_small() {
        assert!(size_of::<DiagramFound>() <= 64, "{}", size_of::<DiagramFound>());
    }
}
