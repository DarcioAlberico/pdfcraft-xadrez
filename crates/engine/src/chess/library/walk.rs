//! A library's folders listed into PDFs (plan A9-N15a; the listing rules of plan A9-I7b, the
//! coordinator's decision 2b of 2026-11-18).
//!
//! **Only the listing.** What an entry is comes from the folder's listing (`DirEntry::file_type`,
//! which follows no link): a file is never opened, read or looked up by its path here.
//!
//! - **Links are never followed**: a symbolic link, a junction, a mount point (on Windows the name
//!   surrogates the listing calls links) is said ([`Walked::links`]), whatever it points to, so a
//!   link to a folder above can't make a loop. A folder that is another kind of reparse point
//!   (OneDrive's synced folders) can't loop, and is followed.
//! - **Nothing in the cloud is touched**: a PDF or a folder whose attributes say its data is not on
//!   this computer (`FILE_ATTRIBUTE_OFFLINE`, `RECALL_ON_OPEN`, `RECALL_ON_DATA_ACCESS`, read from
//!   the listing's own record on Windows) is said ([`Walked::cloud`], [`Walked::cloud_folders`])
//!   and neither opened nor
//!   listed: opening a OneDrive placeholder downloads it, and listing a folder of them can.
//! - **Caps**, each said once when reached ([`Walked::capped`]): folders deeper than
//!   [`MAX_DEPTH`] below a chosen folder are not listed, at most [`MAX_ENTRIES`] entries are
//!   looked at, and at most [`MAX_BOOKS`] PDFs are taken.
//!
//! A folder's entries are taken in the order of their names, so two walks of the same folders
//! give the same books in the same order (the Library's `max_books` goes on where it stopped).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Most levels of folders below a chosen folder that are listed (plan A9-N15a).
pub const MAX_DEPTH: usize = 16;
/// Most entries looked at in one walk.
pub const MAX_ENTRIES: usize = 100_000;
/// Most PDFs taken in one walk.
pub const MAX_BOOKS: usize = 2_000;

/// A walk's caps ([`MAX_DEPTH`], [`MAX_ENTRIES`], [`MAX_BOOKS`] by default).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caps {
    pub depth: usize,
    pub entries: usize,
    pub books: usize,
}

impl Default for Caps {
    fn default() -> Caps {
        Caps { depth: MAX_DEPTH, entries: MAX_ENTRIES, books: MAX_BOOKS }
    }
}

/// A cap a walk reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cap {
    /// Folders deeper than [`Caps::depth`] were not listed.
    Depth,
    /// [`Caps::entries`] entries were looked at; the rest were not.
    Entries,
    /// [`Caps::books`] PDFs were taken; the rest were not.
    Books,
}

impl Cap {
    /// Its name in the tools' replies.
    pub fn name(self) -> &'static str {
        match self {
            Cap::Depth => "depth",
            Cap::Entries => "entries",
            Cap::Books => "books",
        }
    }
}

/// A PDF the listing gave.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pdf {
    pub path: PathBuf,
    /// Its size and its time, as the listing says them (seconds since 1970; 0 when it can't say).
    pub size: u64,
    pub modified: u64,
}

/// What [`walk`] found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Walked {
    /// The PDFs, folder by folder in the order of their names.
    pub pdfs: Vec<Pdf>,
    /// PDFs and folders whose data is not on this computer: never opened or listed.
    pub cloud: Vec<PathBuf>,
    /// Folders whose data is in the cloud: not listed (a folder named `x.pdf` among them is no
    /// book).
    pub cloud_folders: Vec<PathBuf>,
    /// Links (symbolic links, junctions, mount points): never followed.
    pub links: Vec<PathBuf>,
    /// Folders that could not be listed, and why (a chosen folder that is not one among them).
    pub unlisted: Vec<(PathBuf, String)>,
    /// The caps reached, each once, in the order they were.
    pub capped: Vec<Cap>,
    /// Entries looked at.
    pub entries: usize,
    /// `stop` said so between two entries.
    pub stopped: bool,
}

impl Walked {
    fn cap(&mut self, cap: Cap) {
        if !self.capped.contains(&cap) {
            self.capped.push(cap);
        }
    }
}

/// What an entry of a listing is to a walk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Entry {
    /// A folder to list.
    Folder,
    /// A PDF to take.
    Pdf,
    /// A link, a junction or a mount point: not followed.
    Link,
    /// A PDF whose data is in the cloud: not opened.
    Cloud,
    /// A folder whose data is in the cloud: not listed.
    CloudFolder,
    /// Anything else: passed over.
    Other,
}

/// The listing's word on an entry: its kind as the listing gives it (`link` for a name surrogate,
/// which `dir` and `file` are then not), its Windows attributes (0 elsewhere) and its name.
pub(crate) fn classify(link: bool, dir: bool, file: bool, attributes: u32, name: &str) -> Entry {
    if link {
        return Entry::Link;
    }
    if dir {
        return if cloud_only(attributes) { Entry::CloudFolder } else { Entry::Folder };
    }
    if file && is_pdf_name(name) {
        return if cloud_only(attributes) { Entry::Cloud } else { Entry::Pdf };
    }
    Entry::Other
}

/// Whether Windows file `attributes` say an entry's data is not on the disk: offline, or fetched
/// when it is opened or read (the three attributes plan A9-N15 names; a folder's say that listing
/// it fetches its entries).
pub(crate) fn cloud_only(attributes: u32) -> bool {
    const OFFLINE: u32 = 0x0000_1000;
    const RECALL_ON_OPEN: u32 = 0x0004_0000;
    const RECALL_ON_DATA_ACCESS: u32 = 0x0040_0000;
    attributes & (OFFLINE | RECALL_ON_OPEN | RECALL_ON_DATA_ACCESS) != 0
}

/// Whether a file's name is a PDF's: `.pdf` in any case.
fn is_pdf_name(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(stem, ext)| !stem.is_empty() && ext.eq_ignore_ascii_case("pdf"))
}

/// The attributes of a listing's entry (Windows: from the listing's own record, which opens
/// nothing); 0 elsewhere, where a listing gives none.
#[cfg(windows)]
pub(crate) fn attributes_of(meta: Option<&std::fs::Metadata>) -> u32 {
    use std::os::windows::fs::MetadataExt;
    meta.map_or(0, MetadataExt::file_attributes)
}

#[cfg(not(windows))]
pub(crate) fn attributes_of(_meta: Option<&std::fs::Metadata>) -> u32 {
    0
}

/// Seconds since 1970 of a time (0 before it, or when the system can't say).
pub(crate) fn secs(t: Option<std::time::SystemTime>) -> u64 {
    t.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs())
}

/// The PDFs in `dirs` and the folders below them, by the rules of the module documentation,
/// within `caps`; `stop` is asked between two entries. A PDF reached twice (a chosen folder inside
/// another) is taken once.
pub fn walk(dirs: &[PathBuf], caps: Caps, stop: &dyn Fn() -> bool) -> Walked {
    let mut out = Walked::default();
    let mut seen = std::collections::HashSet::new();
    // Folders to list, the next one last: (folder, its depth below the chosen one).
    let mut todo: Vec<(PathBuf, usize)> = Vec::new();
    for dir in dirs.iter().rev() {
        todo.push((dir.clone(), 0));
    }
    // A chosen folder's own attributes: one in the cloud is not listed.
    let chosen: std::collections::HashSet<PathBuf> = dirs.iter().cloned().collect();
    while let Some((dir, depth)) = todo.pop() {
        if chosen.contains(&dir) && depth == 0 {
            match std::fs::symlink_metadata(&dir) {
                Ok(meta) if cloud_only(attributes_of(Some(&meta))) => {
                    out.cloud_folders.push(dir);
                    continue;
                }
                Err(e) => {
                    out.unlisted.push((dir, e.to_string()));
                    continue;
                }
                Ok(_) => {}
            }
        }
        let list = match std::fs::read_dir(&dir) {
            Ok(list) => list,
            Err(e) => {
                out.unlisted.push((dir, e.to_string()));
                continue;
            }
        };
        // The folder's entries, within what is left of the entries' cap, then by name.
        let mut entries: Vec<(String, Entry, PathBuf, Option<std::fs::Metadata>)> = Vec::new();
        for entry in list {
            if out.entries >= caps.entries {
                out.cap(Cap::Entries);
                break;
            }
            if stop() {
                out.stopped = true;
                return out;
            }
            out.entries = out.entries.saturating_add(1);
            let Ok(entry) = entry else { continue };
            // The kind the listing gives: a link is a link, not what it points to.
            let Ok(kind) = entry.file_type() else { continue };
            let meta = entry.metadata().ok();
            let name = entry.file_name().to_string_lossy().into_owned();
            let what = classify(kind.is_symlink(), kind.is_dir(), kind.is_file(), attributes_of(meta.as_ref()), &name);
            entries.push((name, what, entry.path(), meta));
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        let mut folders = Vec::new();
        for (_, what, path, meta) in entries {
            match what {
                Entry::Pdf => {
                    if out.pdfs.len() >= caps.books {
                        out.cap(Cap::Books);
                        continue;
                    }
                    if seen.insert(path.clone()) {
                        let size = meta.as_ref().map_or(0, std::fs::Metadata::len);
                        let modified = secs(meta.as_ref().and_then(|m| m.modified().ok()));
                        out.pdfs.push(Pdf { path, size, modified });
                    }
                }
                Entry::Folder => {
                    if depth >= caps.depth {
                        out.cap(Cap::Depth);
                    } else {
                        folders.push(path);
                    }
                }
                Entry::Link => out.links.push(path),
                Entry::Cloud => out.cloud.push(path),
                Entry::CloudFolder => out.cloud_folders.push(path),
                Entry::Other => {}
            }
        }
        for folder in folders.into_iter().rev() {
            todo.push((folder, depth.saturating_add(1)));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three attributes that put a file's or a folder's data in the cloud, alone or with
    /// others, and none of the rest (plan A9-N15a: the rule over the bits).
    #[test]
    fn the_cloud_is_the_three_attributes_and_no_other() {
        const READ_ONLY: u32 = 0x1;
        const HIDDEN: u32 = 0x2;
        const DIRECTORY: u32 = 0x10;
        const ARCHIVE: u32 = 0x20;
        const REPARSE_POINT: u32 = 0x400;
        const OFFLINE: u32 = 0x1000;
        const PINNED: u32 = 0x8_0000;
        const UNPINNED: u32 = 0x10_0000;
        const RECALL_ON_OPEN: u32 = 0x4_0000;
        const RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;
        for bit in [OFFLINE, RECALL_ON_OPEN, RECALL_ON_DATA_ACCESS] {
            assert!(cloud_only(bit), "{bit:#x}");
            assert!(cloud_only(bit | ARCHIVE | REPARSE_POINT | UNPINNED), "{bit:#x} with others");
        }
        for bit in [0, READ_ONLY, HIDDEN, DIRECTORY, ARCHIVE, REPARSE_POINT, PINNED, UNPINNED] {
            assert!(!cloud_only(bit), "{bit:#x}");
        }
        // OneDrive's file kept on this device: a reparse point, pinned, its data here.
        assert!(!cloud_only(ARCHIVE | REPARSE_POINT | PINNED));
    }

    /// The folder rule (decision 2b): a link is never followed, whatever its attributes; a folder
    /// whose own attributes put it in the cloud is not listed; any other folder is, a reparse
    /// point that is no link (OneDrive's synced folder) among them.
    #[test]
    fn a_folder_is_followed_unless_a_link_or_in_the_cloud() {
        const REPARSE_POINT: u32 = 0x400;
        assert_eq!(classify(true, false, false, 0, "loop"), Entry::Link);
        assert_eq!(classify(true, false, false, REPARSE_POINT, "loop"), Entry::Link);
        assert_eq!(classify(false, true, false, 0, "Books"), Entry::Folder);
        assert_eq!(classify(false, true, false, REPARSE_POINT, "OneDrive"), Entry::Folder);
        for bit in [0x1000, 0x4_0000, 0x40_0000] {
            assert_eq!(classify(false, true, false, bit | REPARSE_POINT, "Books"), Entry::CloudFolder, "{bit:#x}");
            assert_eq!(classify(false, true, false, bit, "x.pdf"), Entry::CloudFolder, "a folder named as a PDF is a folder");
        }
    }

    /// The file rule: a `.pdf` in any case is a PDF unless in the cloud; a link to one is a link;
    /// another file is passed over.
    #[test]
    fn a_file_is_a_pdf_by_its_name_unless_in_the_cloud() {
        assert_eq!(classify(false, false, true, 0, "Book.pdf"), Entry::Pdf);
        assert_eq!(classify(false, false, true, 0, "BOOK.PDF"), Entry::Pdf);
        assert_eq!(classify(false, false, true, 0x1000, "Book.pdf"), Entry::Cloud);
        assert_eq!(classify(true, false, false, 0, "Book.pdf"), Entry::Link);
        for name in ["Book.pgn", "pdf", ".pdf", "Book.pdf.txt"] {
            assert_eq!(classify(false, false, true, 0, name), Entry::Other, "{name}");
        }
    }
}
