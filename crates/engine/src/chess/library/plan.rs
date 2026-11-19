//! What a build would read, said before it starts (plan A9-N15c; R8, the budget in hours): the
//! books of the folders chosen whose index is not current, as the catalog knows them, and their
//! pages, so the Library's panel says about how long a build takes before it begins.
//!
//! **Never a reading.** No page is read, no index is made and nothing is written: the folders are
//! walked as a build walks them ([`walk::walk`]: no link followed, nothing in the cloud opened),
//! and a book the catalog does not know is opened only to count its pages. A book whose path, size
//! and time are as the catalog has them is not opened: indexed (its index there), it is current;
//! skipped for what does not change while the file does not (no copying allowed, encrypted and not
//! said yes to, not a PDF, too large), it costs nothing; read before (stopped, failed, full), its
//! pages are the catalog's. Two PDFs with the same content are one book.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sha2::Digest as _;

use super::walk::{self, Caps};
use super::{Library, MAX_BOOK_BYTES, Skip, State, is_hash};

/// What a build of the folders chosen would read ([`Library::plan`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    /// The PDFs the walk found (not those in the cloud).
    pub pdfs: usize,
    /// Books whose index is current, and books skipped for what does not change.
    pub current: usize,
    pub skipped: usize,
    /// Books a build would read, and their pages.
    pub to_read: usize,
    pub pages: u64,
    /// Of `to_read`, books whose pages could not be counted (they do not open, or can't be
    /// read): the build says what becomes of them.
    pub uncounted: usize,
    /// PDFs in the cloud: never opened.
    pub cloud: usize,
    /// The walk reached a cap: there are more PDFs than were looked at.
    pub capped: bool,
    /// Stopped before the end (`stop`).
    pub stopped: bool,
}

/// Whether a book skipped for `why` is skipped again while its file is unchanged (and, encrypted,
/// still not said yes to).
fn lasting(why: Option<Skip>) -> bool {
    matches!(why, Some(Skip::Never | Skip::Ask | Skip::DoesNotOpen | Skip::TooLarge))
}

impl Library {
    /// What a build of the folders chosen would read now (see the module documentation): `stop`
    /// is asked between two books, and `progress` is told of each (from 0, of how many).
    pub fn plan(&self, caps: Caps, stop: &dyn Fn() -> bool, progress: &mut dyn FnMut(usize, usize)) -> Plan {
        let catalog = self.catalog();
        let mut out = Plan::default();
        if catalog.dirs.is_empty() {
            return out;
        }
        let dirs: Vec<PathBuf> = catalog.dirs.iter().map(PathBuf::from).collect();
        let walked = walk::walk(&dirs, caps, stop);
        let allowed = super::allowed_of(&catalog.allowed, &[]);
        out.pdfs = walked.pdfs.len();
        out.cloud = walked.cloud.len();
        out.capped = !walked.capped.is_empty();
        out.stopped = walked.stopped;
        let mut seen: HashSet<String> = HashSet::new();
        let total = walked.pdfs.len();
        for (n, pdf) in walked.pdfs.iter().enumerate() {
            if stop() {
                out.stopped = true;
                break;
            }
            progress(n, total);
            let path = pdf.path.to_string_lossy();
            let same = catalog.books.iter().find(|b| b.paths.iter().any(|p| *p == path) && b.size == pdf.size && b.modified == pdf.modified);
            let hash = same.and_then(|b| b.hash.as_deref()).filter(|h| is_hash(h)).map(str::to_ascii_lowercase);
            if let Some(h) = &hash
                && !seen.insert(h.clone())
            {
                continue;
            }
            match same {
                Some(b) if b.state == State::Indexed && hash.as_deref().is_some_and(|h| self.index_path(h).is_file()) => {
                    out.current = out.current.saturating_add(1);
                }
                Some(b)
                    if b.state == State::Skipped
                        && lasting(b.reason)
                        && !(b.reason == Some(Skip::Ask) && hash.as_ref().is_some_and(|h| allowed.contains(h))) =>
                {
                    out.skipped = out.skipped.saturating_add(1);
                }
                Some(b) if b.pages.is_some() && b.state != State::Indexed => {
                    out.to_read = out.to_read.saturating_add(1);
                    out.pages = out.pages.saturating_add(u64::from(b.pages.unwrap_or(0)));
                }
                _ => match pages_of(&pdf.path) {
                    Some((content, pages)) => {
                        // Its catalog hash, taken above, is its content: counted once.
                        if hash.as_deref() == Some(content.as_str()) || seen.insert(content) {
                            out.to_read = out.to_read.saturating_add(1);
                            out.pages = out.pages.saturating_add(pages);
                        }
                    }
                    None => {
                        out.to_read = out.to_read.saturating_add(1);
                        out.uncounted = out.uncounted.saturating_add(1);
                    }
                },
            }
        }
        out
    }
}

/// The content's hash and the pages of the PDF at `path`, when it is still a book to open
/// ([`super::still_a_book`]) and opens; read whole, within [`MAX_BOOK_BYTES`].
fn pages_of(path: &Path) -> Option<(String, u64)> {
    let max = u64::try_from(MAX_BOOK_BYTES).unwrap_or(u64::MAX);
    super::still_a_book(path, max).ok()?;
    let bytes = super::super::corrections::read_capped(path, MAX_BOOK_BYTES).ok().filter(|b| b.len() <= MAX_BOOK_BYTES)?;
    let content = super::hex(&sha2::Sha256::digest(&bytes));
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let pages = crate::Session::new().page_count_of(&name, &Arc::new(bytes)).ok()?;
    Some((content, u64::try_from(pages).unwrap_or(u64::MAX)))
}
