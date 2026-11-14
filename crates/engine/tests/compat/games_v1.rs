//! The games' autosave of version 1 as the builds installed before plan A9-G12 look through it,
//! frozen: a literal copy of the header's serde structs and of `FileStore::find`'s walk of the
//! folder in `crates/ui-egui/src/chess_autosave.rs` at `cc2d484c` (the last build the user keeps
//! installed, plan A9 §2.10), so a test can do what such a build does in a folder this build
//! writes in. Nothing here may change: it stands for builds already installed.
//!
//! Left out of the copy: the bodies, the stores' writing, the web's value and the document keys'
//! matching (a file the walk does not read the header of is never matched); `log` calls and the
//! `ignored` set are dropped, and `DocKeys` is the engine's.

use std::io::{BufRead, Read};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const FORMAT: &str = "pdfcraft-chess-games";

/// The format's version. Another version is set aside, not read.
pub const VERSION: u32 = 1;

const MAX_BODY_BYTES: usize = 32 << 20;
const MAX_HEADER_BYTES: usize = 64 << 10;
const MAX_FILE_BYTES: u64 = (MAX_BODY_BYTES + MAX_HEADER_BYTES + 1) as u64;
const MAX_FILES: usize = 4096;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Header {
    format: String,
    version: u32,
    #[serde(default)]
    keys: StoredKeys,
    #[serde(default)]
    instance: Option<String>,
    #[serde(default)]
    games: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct StoredKeys {
    #[serde(default)]
    id0: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    size: Option<u64>,
}

fn readable(header: &Header) -> bool {
    header.format == FORMAT && header.version == VERSION
}

fn plain_cid(cid: &str) -> bool {
    !cid.is_empty() && cid.len() <= 64 && cid.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// `FileStore::header`.
fn header(path: &Path) -> Result<Option<Header>, ()> {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(()),
    };
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return Err(());
    }
    let file = std::fs::File::open(path).map_err(|_| ())?;
    let mut line = Vec::new();
    std::io::BufReader::new(file.take(MAX_HEADER_BYTES as u64)).read_until(b'\n', &mut line).map_err(|_| ())?;
    let header: Header = serde_json::from_slice(line.strip_suffix(b"\n").ok_or(())?).map_err(|_| ())?;
    if readable(&header) { Ok(Some(header)) } else { Err(()) }
}

/// `FileStore::set_aside`.
fn set_aside(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    for n in 0..100u32 {
        let bad = if n == 0 { format!("{name}.bad") } else { format!("{name}.bad.{n}") };
        let to = path.with_file_name(&bad);
        if to.exists() {
            continue;
        }
        if std::fs::rename(path, &to).is_ok() {
            return Some(bad);
        }
        break;
    }
    None
}

/// The walk of `FileStore::find` for any document: every `.games` file's header read, and each
/// one whose header such a build can't read set aside. The names set aside.
pub fn walk(dir: &Path) -> Vec<String> {
    let mut set = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return set };
    let mut paths: Vec<PathBuf> =
        entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "games")).take(MAX_FILES).collect();
    paths.sort();
    for path in paths {
        let Some(_cid) = path.file_stem().and_then(|s| s.to_str()).filter(|s| plain_cid(s)).map(str::to_owned) else { continue };
        if header(&path).is_err() {
            set.extend(set_aside(&path));
        }
    }
    set
}
