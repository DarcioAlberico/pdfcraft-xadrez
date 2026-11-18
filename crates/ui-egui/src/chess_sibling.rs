//! Chess ▸ the PGN beside the PDF (plan "Carregar PGN do livro", R6c and R6f; plan A9-I7,
//! findings A6-02 and A6-03): the PGN that goes with a PDF, offered while the document keeps none.
//!
//! **Which one.** In the PDF's folder, the PGN named exactly as the PDF; else one whose name is the
//! PDF's once symbols, emoji, spaces and case are set aside (`📚Livro.pdf` → `Livro.pgn`, I7a);
//! else the only PGN of the folder (I7b). Only an offer: the user says whether to use it.
//!
//! **How the folder is looked through** (plan A9-I7b, by N15's rules for a library's folders). In a
//! [`Job`], off the interface thread, which stops when asked (between two entries) and when the
//! document closes. At most [`MAX_FOLDER_ENTRIES`] entries are read — the folder may be Downloads —
//! and the only PGN is offered only when the folder was read to its end. Only the names and kinds
//! the directory listing gives are used: no file is opened, read or looked up by its path, and no
//! link or junction is followed, so a cloud file not downloaded (a OneDrive placeholder) stays as
//! it is, and a folder named `x.pgn` is not a PGN. A PGN of the PDF's name past the first entries
//! read is not offered; **Load the book's PGN…** finds it. Never on the web, which has no paths.

use std::path::PathBuf;

use pdfcraft_engine::DocId;

use crate::PdfCraftApp;
use crate::chess_job::Job;
use crate::chess_reference_ui::{is_pgn_name, sibling_pgn};

/// Most entries of the PDF's folder read for its PGN: the folder may be Downloads (plan A9-I7a,
/// I7b).
pub(crate) const MAX_FOLDER_ENTRIES: usize = 4096;

/// The search for one document's PGN beside it: running, or done.
#[derive(Default)]
pub(crate) struct Look {
    job: Option<Job<Option<PathBuf>>>,
    /// The folder was looked through for the document as it is open (or is being).
    started: bool,
}

/// The regular files of a folder, by name, and whether the folder was read to its end.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
struct Listing {
    files: Vec<String>,
    complete: bool,
}

/// The regular files of `dir` by name, from its listing alone: at most `cap` entries read; links,
/// junctions, folders and names that are not Unicode left out. `None` when it can't be listed,
/// or `stop` said so between two entries.
#[cfg(not(target_arch = "wasm32"))]
fn list(dir: &std::path::Path, cap: usize, stop: impl Fn() -> bool) -> Option<Listing> {
    let mut files = Vec::new();
    let mut read = 0usize;
    for entry in std::fs::read_dir(dir).ok()? {
        if read >= cap {
            return Some(Listing { files, complete: false });
        }
        read = read.saturating_add(1);
        if stop() {
            return None;
        }
        let Ok(entry) = entry else { continue };
        // The kind the listing gives (a link is a link, not what it points to): never a lookup
        // by path, which could reach through a link or wake a cloud placeholder.
        if !entry.file_type().is_ok_and(|t| t.is_file()) || in_the_cloud(&entry) {
            continue;
        }
        if let Ok(name) = entry.file_name().into_string() {
            files.push(name);
        }
    }
    Some(Listing { files, complete: true })
}

/// Whether the listing says `entry`'s data is not on this computer — a OneDrive file not
/// downloaded: offline, or fetched when it is opened or read ([`cloud_only`]). Such a file is
/// never offered, so the offer can't start a download. On Windows the attributes are in the
/// listing's own record (`DirEntry::metadata` asks the system nothing more and opens nothing);
/// one it can't give is taken as in the cloud.
#[cfg(windows)]
fn in_the_cloud(entry: &std::fs::DirEntry) -> bool {
    use std::os::windows::fs::MetadataExt;
    entry.metadata().map_or(true, |m| cloud_only(m.file_attributes()))
}

/// Elsewhere a listing gives no such attribute: nothing is taken as in the cloud.
#[cfg(all(not(windows), not(target_arch = "wasm32")))]
fn in_the_cloud(_entry: &std::fs::DirEntry) -> bool {
    false
}

/// Whether Windows file `attributes` say its data is not on the disk: `FILE_ATTRIBUTE_OFFLINE`,
/// `FILE_ATTRIBUTE_RECALL_ON_OPEN` or `FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS` (the attributes
/// plan A9-N15 names for a library's folders).
#[cfg(windows)]
fn cloud_only(attributes: u32) -> bool {
    const OFFLINE: u32 = 0x0000_1000;
    const RECALL_ON_OPEN: u32 = 0x0004_0000;
    const RECALL_ON_DATA_ACCESS: u32 = 0x0040_0000;
    attributes & (OFFLINE | RECALL_ON_OPEN | RECALL_ON_DATA_ACCESS) != 0
}

/// The PGN among the regular files `files` of the PDF named `pdf`'s folder that goes with it: by
/// its name ([`sibling_pgn`]), else the only PGN of the folder, when `complete` says the folder
/// was read to its end (plan A9-I7b).
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn pick<'a>(pdf: &str, files: &'a [String], complete: bool) -> Option<&'a str> {
    if let Some(named) = sibling_pgn(pdf, files.iter().map(String::as_str)) {
        return Some(named);
    }
    let mut pgns = files.iter().filter(|n| is_pgn_name(n));
    let only = pgns.next()?;
    (complete && pgns.next().is_none()).then_some(only.as_str())
}

/// The PGN beside the PDF at `pdf` that goes with it ([`pick`]): its folder looked through as the
/// module says, `stop` asked between two entries.
#[cfg(not(target_arch = "wasm32"))]
fn find(pdf: &std::path::Path, stop: impl Fn() -> bool) -> Option<PathBuf> {
    let dir = pdf.parent()?;
    let listing = list(dir, MAX_FOLDER_ENTRIES, stop)?;
    let name = pick(pdf.file_name()?.to_str()?, &listing.files, listing.complete)?;
    Some(dir.join(name))
}

impl PdfCraftApp {
    /// Look through the folder of document `id`'s PDF for the PGN that goes with it (plan
    /// A9-I7b), off the interface thread: once for the document as it is open, or `again` (its
    /// book's PGN removed). What it finds is offered while the document keeps none
    /// ([`Self::poll_sibling_pgns`]). Nothing on the web, or for a document with no path.
    pub(crate) fn look_for_sibling_pgn(&mut self, id: DocId, again: bool) {
        if self.chess.doc(id).is_some_and(|d| d.pgn_look.started && !again) {
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        let job = {
            let inline = self.run_inline;
            let pdf = self.session.get(id).and_then(|d| d.path.clone()).map(PathBuf::from);
            pdf.map(|pdf| Job::spawn("pdfcraft-chess-pgn-beside", inline, move |ctl| Ok(find(&pdf, || ctl.cancelled()))))
        };
        #[cfg(target_arch = "wasm32")]
        let job = None;
        let view = self.chess.doc_mut(id);
        view.pgn_look.started = true;
        view.pgn_sibling = None;
        // A search running is dropped for this one, which asks it to stop.
        view.pgn_look.job = job;
        self.poll_sibling_pgns();
    }

    /// Take what the searches for the PGNs beside the PDFs found (plan A9-I7b): offered while
    /// the document keeps none, and the document looks like a chess book (the rail's hint).
    pub(crate) fn poll_sibling_pgns(&mut self) {
        let mut running = false;
        let mut found: Vec<(DocId, PathBuf)> = Vec::new();
        for (id, view) in &mut self.chess.docs {
            let Some(job) = view.pgn_look.job.as_mut() else { continue };
            match job.poll() {
                None => running = true,
                Some(out) => {
                    view.pgn_look.job = None;
                    if let Ok(Some(path)) = out {
                        found.push((*id, path));
                    }
                }
            }
        }
        for (id, path) in found {
            if self.session.chess_book_pgn(id).is_some() {
                continue;
            }
            let view = self.chess.doc_mut(id);
            view.pgn_sibling = Some(path);
            if view.chess_hint == Some(false) {
                view.chess_hint = Some(true);
            }
        }
        if running && let Some(ctx) = &self.ctx {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
    }

    /// Whether the folder of document `id`'s PDF is being looked through for its PGN (plan
    /// A9-I7b): its search has not been taken by a frame yet.
    pub fn sibling_pgn_looking(&self, id: DocId) -> bool {
        self.chess.doc(id).is_some_and(|d| d.pgn_look.job.is_some())
    }

    /// Wait until every search for a PGN beside a PDF is done, or `timeout` passed; whether they
    /// are. The next frame takes what they found. The tests wait for the work this way, not by
    /// the clock (plan A9-Q4b).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn wait_sibling_pgn_looks(&self, timeout: std::time::Duration) -> bool {
        self.chess.docs.values().filter_map(|d| d.pgn_look.job.as_ref()).all(|job| job.wait(timeout))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plan A9-I7b: with no PGN of its name, the only PGN of the folder is offered, and only when
    /// the folder was read to its end; two PGNs, or none, offer nothing.
    #[test]
    fn the_only_pgn_of_a_folder_read_to_its_end_is_offered() {
        let files = |names: &[&str]| names.iter().map(|n| (*n).to_owned()).collect::<Vec<String>>();
        let one = files(&["notes.txt", "games.PGN", "Livro.pdf"]);
        assert_eq!(pick("Livro.pdf", &one, true), Some("games.PGN"));
        assert_eq!(pick("Livro.pdf", &one, false), None, "a folder cut at its cap may hold another");
        assert_eq!(pick("Livro.pdf", &files(&["a.pgn", "b.pgn"]), true), None);
        assert_eq!(pick("Livro.pdf", &files(&["Livro.pdf", "notes.txt"]), true), None);
        // The PGN of its name is offered over the only one, and from a folder cut at its cap.
        assert_eq!(pick("Livro.pdf", &files(&["a.pgn", "livro.pgn"]), false), Some("livro.pgn"));
    }

    /// Plan A9-I7b (N15's rules): a folder named `x.pgn` is not a PGN, and a link to a PGN is not
    /// followed; the entries read stop at the cap, and a search asked to stop gives nothing.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_listing_takes_regular_files_only_and_stops_at_its_cap() {
        let dir = std::env::temp_dir().join(format!("pdfcraft-pgn-beside-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("x.pgn")).unwrap();
        std::fs::write(dir.join("book.pdf"), b"%PDF-1.7").unwrap();
        assert_eq!(find(&dir.join("book.pdf"), || false), None, "a folder named x.pgn is not offered");
        let listing = list(&dir, 10, || false).unwrap();
        assert_eq!((listing.files, listing.complete), (vec!["book.pdf".to_owned()], true));
        std::fs::write(dir.join("games.pgn"), b"1.e4 *").unwrap();
        assert_eq!(find(&dir.join("book.pdf"), || false), Some(dir.join("games.pgn")));
        assert_eq!(find(&dir.join("book.pdf"), || true), None, "stopped");
        let cut = list(&dir, 1, || false).unwrap();
        assert!(!cut.complete && cut.files.len() <= 1, "{:?}", cut.files);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Plan A9-I7b (N15's rules): a PGN whose data is not on the disk — a OneDrive file not
    /// downloaded — is never offered, by its name or as the only one, so the offer can't start a
    /// download; the attributes come from the listing. Here the file is made offline as it is
    /// created (`FILE_ATTRIBUTE_OFFLINE`, which a cloud placeholder also carries).
    #[cfg(windows)]
    #[test]
    fn a_pgn_in_the_cloud_is_not_offered() {
        use std::os::windows::fs::OpenOptionsExt;
        assert!(cloud_only(0x0040_0000) && cloud_only(0x0004_0000) && cloud_only(0x1000 | 0x20), "recall on access, offline");
        assert!(!cloud_only(0x20) && !cloud_only(0x400), "an ordinary file, a downloaded placeholder (a reparse point)");
        let dir = std::env::temp_dir().join(format!("pdfcraft-pgn-cloud-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("book.pdf"), b"%PDF-1.7").unwrap();
        let offline = |name: &str| {
            std::fs::OpenOptions::new().write(true).create_new(true).attributes(0x1000).open(dir.join(name)).unwrap();
        };
        offline("games.pgn");
        assert_eq!(list(&dir, 10, || false).unwrap().files, vec!["book.pdf".to_owned()], "the listing leaves it out");
        assert_eq!(find(&dir.join("book.pdf"), || false), None, "the only PGN, in the cloud");
        offline("book.pgn");
        assert_eq!(find(&dir.join("book.pdf"), || false), None, "the PGN of its name, in the cloud");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Plan A9-I7b: the offer comes when the search ends, and no frame waits for the folder:
    /// polled while the search is held, every poll returns at once and nothing is offered; once
    /// it ends, the next poll offers what it found and turns the rail's hint on.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_offer_comes_when_the_search_ends_and_no_poll_waits() {
        let mut app = PdfCraftApp::new();
        let id = DocId(7);
        let (go, gate) = std::sync::mpsc::channel::<()>();
        let found = PathBuf::from("games.pgn");
        let job = Job::spawn("pdfcraft-chess-pgn-beside-test", false, {
            let found = found.clone();
            move |_| Ok(gate.recv().ok().map(|()| found))
        });
        let view = app.chess.doc_mut(id);
        view.pgn_look = Look { job: Some(job), started: true };
        view.chess_hint = Some(false);
        // A net against a poll that waits: the search is let go after 20 s whatever happens.
        let net = std::thread::spawn({
            let go = go.clone();
            move || {
                std::thread::sleep(std::time::Duration::from_secs(20));
                let _ = go.send(());
            }
        });
        let started = std::time::Instant::now();
        for _ in 0..3 {
            app.poll_sibling_pgns();
        }
        assert!(started.elapsed() < std::time::Duration::from_secs(10), "a poll waited {:?}", started.elapsed());
        assert!(app.sibling_pgn_looking(id), "still looking");
        assert!(app.chess.doc(id).is_some_and(|d| d.pgn_sibling.is_none() && d.chess_hint == Some(false)), "nothing yet");
        go.send(()).unwrap();
        assert!(app.wait_sibling_pgn_looks(std::time::Duration::from_secs(60)), "the search ended");
        app.poll_sibling_pgns();
        assert!(!app.sibling_pgn_looking(id), "taken");
        let view = app.chess.doc(id).unwrap();
        assert_eq!(view.pgn_sibling.as_deref(), Some(found.as_path()));
        assert_eq!(view.chess_hint, Some(true), "the rail's hint turns on");
        drop(net);
    }
}
