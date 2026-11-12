//! The files the chess tools read and write, and how their replies name them (plan A9, G7a).
//!
//! - **Reading** a file the caller names goes through one capped read
//!   ([`pdfcraft_engine::chess::corrections::read_capped`]): never more than the cap and one byte
//!   in memory, whatever the file's size (A4-10, A4-22).
//! - **Writing**: the outputs of one call each name a file of their own (two on one file would
//!   leave the second's bytes under the first's name, R7-04), and none is the file of an open
//!   document or a file the same call reads, unless the call says `overwrite: true` (A4-11).
//! - **Replies** name a file relative to the root that holds it when files are confined; outside
//!   every root, by its file name alone with `outside_root: true`, so a confined client never
//!   learns the folders around it (R7-06). Unconfined, a path is given as it is.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::{Args, Automation, Result, ToolError, failed};

/// The bytes of the file at `path` (`shown` as the caller named it), refused past `max` bytes
/// before more than `max` + 1 are read.
pub(crate) fn read_capped(path: &Path, shown: &str, max: usize) -> Result<Vec<u8>> {
    let bytes = pdfcraft_engine::chess::corrections::read_capped(path, max).map_err(|e| failed(format!("{shown}: {e}")))?;
    if bytes.len() > max {
        return Err(failed(format!("{shown}: larger than {max} bytes, the most read")));
    }
    Ok(bytes)
}

/// What `path` is compared by to tell whether two paths are one file: its canonical form when it
/// exists, else its folder's with its name, else it made absolute; without regard to case on
/// Windows.
fn file_key(path: &Path) -> PathBuf {
    let key = path.canonicalize().unwrap_or_else(|_| {
        let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        match (absolute.parent().and_then(|d| d.canonicalize().ok()), absolute.file_name()) {
            (Some(folder), Some(name)) => folder.join(name),
            _ => absolute,
        }
    });
    if cfg!(windows) { PathBuf::from(key.to_string_lossy().to_lowercase()) } else { key }
}

/// `path` without Windows' `\\?\` prefix (which `canonicalize` adds), so it compares with a root
/// written either way.
fn plain(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    match text.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => path.to_path_buf(),
    }
}

impl Automation {
    /// Whether the tools' files are confined (`--root`, or the automation roots).
    pub(crate) fn confined(&self) -> bool {
        self.root.is_some() || self.roots.is_some()
    }

    /// One call's outputs (`(argument, path)`), checked before anything is written: each names a
    /// file of its own, else [`ToolError::InvalidArgs`]; and none is the file of an open document
    /// or one of `inputs` (the files the call reads), unless `overwrite` is true.
    pub(crate) fn check_outputs(&self, a: &Args, outputs: &[(&str, &Path)], inputs: &[&Path]) -> Result<()> {
        let keys: Vec<(&str, PathBuf)> = outputs.iter().map(|(arg, path)| (*arg, file_key(path))).collect();
        for (i, (arg, key)) in keys.iter().enumerate() {
            if let Some((first, _)) = keys.iter().take(i).find(|(_, k)| k == key) {
                return Err(ToolError::InvalidArgs(format!("{first} and {arg} name the same file: each output needs a file of its own")));
            }
        }
        if a.opt_bool("overwrite")?.unwrap_or(false) {
            return Ok(());
        }
        let inputs: Vec<PathBuf> = inputs.iter().map(|p| file_key(p)).collect();
        for (arg, key) in &keys {
            let open = self.session.docs().iter().find(|d| d.path.as_deref().is_some_and(|p| file_key(Path::new(p)) == *key));
            if let Some(doc) = open {
                return Err(ToolError::InvalidArgs(format!(
                    "{arg} is the file of the open document {}: writing there would replace it; pass overwrite: true to write over it",
                    doc.name
                )));
            }
            if inputs.contains(key) {
                return Err(ToolError::InvalidArgs(format!("{arg} is a file this call reads: pass overwrite: true to write over it")));
            }
        }
        Ok(())
    }

    /// `path` as a reply names it, and whether it lies outside every root: relative to the root
    /// that holds it when files are confined (`.` for the root itself), else by its file name
    /// alone. Unconfined, as it is.
    pub(crate) fn shown_path(&self, path: &Path) -> (String, bool) {
        if !self.confined() {
            return (path.to_string_lossy().into_owned(), false);
        }
        let real = plain(&path.canonicalize().unwrap_or_else(|_| std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())));
        let roots = self.root.iter().map(|r| r.as_path()).chain(self.roots.iter().flat_map(|r| r.read_root().into_iter().chain(r.write_root())));
        for root in roots {
            let root = plain(root);
            let inside = if cfg!(windows) {
                PathBuf::from(real.to_string_lossy().to_lowercase()).strip_prefix(root.to_string_lossy().to_lowercase()).ok().map(Path::to_path_buf)
            } else {
                real.strip_prefix(&root).ok().map(Path::to_path_buf)
            };
            if let Some(rest) = inside {
                // The rest as it is spelled, not lowercased: the same number of components from
                // the end of the real path.
                let n = rest.components().count();
                let parts: Vec<_> = real.components().collect();
                let tail: PathBuf = parts.get(parts.len().saturating_sub(n)..).unwrap_or_default().iter().collect();
                return if n == 0 { (".".into(), false) } else { (tail.to_string_lossy().into_owned(), false) };
            }
        }
        (path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), true)
    }

    /// `text` (a message a reply quotes, such as why a model could not be read) with each root's
    /// absolute path taken out, so a file inside a root is named relative to it there too.
    /// Unconfined, as it is.
    pub(crate) fn shown_text(&self, text: &str) -> String {
        if !self.confined() {
            return text.to_owned();
        }
        let roots: Vec<PathBuf> = self
            .root
            .iter()
            .map(|r| plain(r))
            .chain(self.roots.iter().flat_map(|r| r.read_root().into_iter().chain(r.write_root()).map(plain)))
            .collect();
        let mut out = text.to_owned();
        for root in roots {
            let root = root.to_string_lossy().into_owned();
            if root.is_empty() {
                continue;
            }
            // The verbatim form first: the plain one is inside it.
            let forms = if cfg!(windows) { vec![format!(r"\\?\{root}"), root] } else { vec![root] };
            for form in forms {
                out = out.replace(&format!("{form}{}", std::path::MAIN_SEPARATOR), "").replace(&form, ".");
            }
        }
        out
    }

    /// [`shown_path`](Self::shown_path) as a JSON string.
    pub(crate) fn path_json(&self, path: &Path) -> Value {
        json!(self.shown_path(path).0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_key_ignores_how_the_path_is_written() {
        let dir = std::env::temp_dir().join(format!("pdfcraft-chess-files-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.pdf"), b"x").unwrap();
        assert_eq!(file_key(&dir.join("a.pdf")), file_key(&dir.join("sub").join("..").join("a.pdf")));
        assert_eq!(file_key(&dir.join("new.md")), file_key(&dir.join("sub").join("..").join("new.md")), "a file not written yet");
        assert_ne!(file_key(&dir.join("a.pdf")), file_key(&dir.join("b.pdf")));
        if cfg!(windows) {
            assert_eq!(file_key(&dir.join("A.PDF")), file_key(&dir.join("a.pdf")));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
