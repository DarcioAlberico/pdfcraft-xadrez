//! Where the app is and where it keeps the user's data (plan A9-T3, finding R3-07), decided once,
//! here at L7. The engine and the chess vision crate never ask the system where the executable is:
//! the frontends (the desktop app, the automation tools) hand them [`AppDirs::install`] as a
//! parameter, and they look for the app's own folders (`models`, `pieces`, `fonts`, `engine`) in
//! it. In the browser there is no such folder, and every lookup gets `None`.
//!
//! Portable mode (#157): a marker file beside the executable ([`MARKERS`]) keeps the settings,
//! log files, crash-recovery autosaves and new digital IDs in `<exe dir>/PdfCraftData`, so a
//! portable copy (on a USB stick, say) writes nothing to `%APPDATA%` or `%LOCALAPPDATA%`. The
//! Windows portable zip ships with `portable.txt`; the MSI doesn't. PhotoCraft uses the same
//! scheme (`photocraft/apps/photocraft/src/app_dirs.rs`). If that folder can't be written
//! (read-only medium, Program Files), portable mode stays off, the per-user folders are used and
//! [`Portable::unwritable`] says why.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Files beside the executable that switch on portable mode (either one; contents are ignored).
pub const MARKERS: [&str; 2] = ["portable.txt", "PdfCraft.portable"];
/// The data folder created beside the executable in portable mode.
pub const DATA_DIR: &str = "PdfCraftData";

/// The app's folders, as this process found them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AppDirs {
    /// The folder the running executable is in: where the app's own folders are looked for
    /// (`models`, `pieces`, `fonts`, `engine`). `None` when the system won't say, and in the
    /// browser.
    pub install: Option<PathBuf>,
    /// The folder the app keeps the user's data in: [`DATA_DIR`] beside the executable in
    /// portable mode, otherwise the platform's per-user data folder ([`per_user_data`]). The
    /// crash-recovery folder is inside it. `None` when neither can be told (and in the browser).
    pub data: Option<PathBuf>,
    /// Portable mode, as found beside the executable.
    pub portable: Portable,
}

impl AppDirs {
    /// The folders of this process, resolved once (the portable check probes the disk).
    pub fn current() -> &'static AppDirs {
        static CURRENT: OnceLock<AppDirs> = OnceLock::new();
        CURRENT.get_or_init(|| AppDirs::resolve(std::env::current_exe().ok().as_deref(), |k| std::env::var_os(k)))
    }

    /// The folders of the executable `exe`, with environment variables read through `env`.
    pub fn resolve(exe: Option<&Path>, env: impl Fn(&str) -> Option<OsString>) -> AppDirs {
        let install = exe.and_then(Path::parent).map(Path::to_path_buf);
        let portable = resolve(install.as_deref());
        let data = portable.dir.clone().or_else(|| per_user_data(env));
        AppDirs { install, data, portable }
    }

    /// [`AppDirs::install`] as a path, what the engine and the chess vision lookups take.
    pub fn install(&self) -> Option<&Path> {
        self.install.as_deref()
    }

    /// The crash-recovery folder in [`AppDirs::data`]: `Recovery`, in lower case in an XDG data
    /// folder (`~/.local/share/pdfcraft/recovery`, like the folders around it).
    pub fn recovery(&self) -> Option<PathBuf> {
        let xdg = self.portable.dir.is_none() && cfg!(all(unix, not(target_os = "macos")));
        self.data.as_ref().map(|d| d.join(if xdg { "recovery" } else { "Recovery" }))
    }

    /// The folder of the chess stores in [`AppDirs::data`] (plan A9-G5a, decision D17): `chess`,
    /// with `corrections` and `reference` in it, the one place the app and the command line keep
    /// the diagram corrections and the books' PGNs. Not under [`AppDirs::recovery`], which is
    /// for what a crash left behind.
    pub fn chess(&self) -> Option<PathBuf> {
        self.data.as_ref().map(|d| d.join("chess"))
    }
}

/// Make the folder `dir` (with its parents) and, where the system has permissions (Unix), open
/// to its owner alone (0700), as the user's data in it is: the chess stores keep what was read
/// from the books, unencrypted. Elsewhere it is made as the system makes folders.
pub fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let open_to_others = std::fs::metadata(dir).is_ok_and(|m| m.permissions().mode() & 0o077 != 0);
        if open_to_others {
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        }
    }
    Ok(())
}

/// The platform's per-user data folder for PdfCraft, from the environment variables `env` gives:
/// `~/Library/Application Support/PdfCraft` (macOS), `%LOCALAPPDATA%\PdfCraft` (Windows), or
/// `$XDG_DATA_HOME/pdfcraft` / `~/.local/share/pdfcraft` (others). An empty variable counts as
/// unset; `None` when the one needed is unset.
pub fn per_user_data(env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let var = |k: &str| env(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if cfg!(target_os = "macos") {
        var("HOME").map(|h| h.join("Library/Application Support/PdfCraft"))
    } else if cfg!(windows) {
        var("LOCALAPPDATA").map(|d| d.join("PdfCraft"))
    } else {
        var("XDG_DATA_HOME").or_else(|| var("HOME").map(|h| h.join(".local/share"))).map(|d| d.join("pdfcraft"))
    }
}

/// The outcome of looking for a portable marker.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Portable {
    /// `<exe dir>/PdfCraftData` when portable mode is on, `None` otherwise.
    pub dir: Option<PathBuf>,
    /// Set when a marker was found but its data folder can't be written.
    pub unwritable: Option<Unwritable>,
}

/// A marker was found, but the data folder beside it can't be created or written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unwritable {
    pub marker: PathBuf,
    pub folder: PathBuf,
    /// The operating system's error text.
    pub error: String,
}

impl std::fmt::Display for Unwritable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "portable mode is off: {} was found, but {} can't be written ({}); settings are kept in the user folder instead",
            self.marker.display(),
            self.folder.display(),
            self.error
        )
    }
}

/// Look for a marker in `exe_dir` and, when one is there, create its data folder beside it.
pub fn resolve(exe_dir: Option<&Path>) -> Portable {
    let Some((exe_dir, marker)) = exe_dir.and_then(|d| find_marker(d).map(|m| (d, m))) else {
        return Portable::default();
    };
    let folder = exe_dir.join(DATA_DIR);
    match ensure_writable(&folder) {
        Ok(()) => Portable { dir: Some(folder), unwritable: None },
        Err(e) => Portable { dir: None, unwritable: Some(Unwritable { marker, folder, error: e.to_string() }) },
    }
}

/// The first marker file in `exe_dir`, if any. A folder with a marker's name is not a marker.
pub fn find_marker(exe_dir: &Path) -> Option<PathBuf> {
    MARKERS.iter().map(|m| exe_dir.join(m)).find(|p| p.is_file())
}

/// Create `dir` and prove a file can be written in it (a folder can exist yet be read-only).
fn ensure_writable(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let probe = dir.join(".write-test");
    std::fs::write(&probe, b"")?;
    // A leftover probe is harmless; don't fail portable mode over it.
    let _ = std::fs::remove_file(&probe);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn temp(tag: &str) -> PathBuf {
        static N: AtomicUsize = AtomicUsize::new(0);
        let d = std::env::temp_dir().join(format!("pdfcraft-portable-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// An environment with only `vars` set.
    fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let vars: Vec<(String, OsString)> = vars.iter().map(|(k, v)| ((*k).to_owned(), OsString::from(v))).collect();
        move |k| vars.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone())
    }

    #[test]
    fn no_marker_leaves_portable_mode_off() {
        let exe = temp("off");
        assert_eq!(resolve(Some(&exe)), Portable::default());
        assert!(!exe.join(DATA_DIR).exists(), "no data folder without a marker");
        let _ = std::fs::remove_dir_all(&exe);
    }

    #[test]
    fn either_marker_keeps_the_data_beside_the_exe() {
        for marker in MARKERS {
            let exe = temp("on");
            std::fs::write(exe.join(marker), b"").unwrap();
            let p = resolve(Some(&exe));
            assert_eq!(p.unwritable, None, "{marker}");
            let dir = p.dir.unwrap();
            assert_eq!(dir, exe.join(DATA_DIR));
            assert!(dir.is_dir(), "the data folder is created");
            assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "the write probe is removed");
            let _ = std::fs::remove_dir_all(&exe);
        }
    }

    #[test]
    fn an_existing_data_folder_is_reused() {
        let exe = temp("reuse");
        std::fs::write(exe.join("portable.txt"), b"PdfCraft portable mode").unwrap();
        std::fs::create_dir_all(exe.join(DATA_DIR)).unwrap();
        std::fs::write(exe.join(DATA_DIR).join("app.ron"), b"()").unwrap();
        assert_eq!(resolve(Some(&exe)).dir, Some(exe.join(DATA_DIR)));
        assert_eq!(std::fs::read(exe.join(DATA_DIR).join("app.ron")).unwrap(), b"()", "existing settings are kept");
        let _ = std::fs::remove_dir_all(&exe);
    }

    #[test]
    fn a_marker_folder_is_not_a_marker() {
        let exe = temp("dir-marker");
        std::fs::create_dir_all(exe.join("portable.txt")).unwrap();
        assert_eq!(resolve(Some(&exe)), Portable::default());
        let _ = std::fs::remove_dir_all(&exe);
    }

    #[test]
    fn unwritable_data_folder_turns_portable_mode_off_and_says_why() {
        let exe = temp("unwritable");
        std::fs::write(exe.join("portable.txt"), b"").unwrap();
        // A file where the data folder should go: it can't be created, on every platform.
        std::fs::write(exe.join(DATA_DIR), b"in the way").unwrap();
        let p = resolve(Some(&exe));
        assert_eq!(p.dir, None);
        let unwritable = p.unwritable.unwrap();
        assert_eq!(unwritable.marker, exe.join("portable.txt"));
        assert_eq!(unwritable.folder, exe.join(DATA_DIR));
        let message = unwritable.to_string();
        assert!(message.contains("portable mode is off") && message.contains("can't be written"), "{message}");
        let _ = std::fs::remove_dir_all(&exe);
    }

    #[test]
    fn no_exe_dir_leaves_portable_mode_off() {
        assert_eq!(resolve(None), Portable::default());
    }

    #[test]
    fn the_install_folder_is_the_executables() {
        let exe = temp("install");
        let dirs = AppDirs::resolve(Some(&exe.join("pdfcraft-cli.exe")), env(&[]));
        assert_eq!(dirs.install(), Some(exe.as_path()));
        assert_eq!(dirs.portable, Portable::default());
        assert_eq!(AppDirs::resolve(None, env(&[])), AppDirs::default(), "nothing known: nothing found");
        let _ = std::fs::remove_dir_all(&exe);
    }

    #[test]
    fn portable_mode_keeps_the_data_beside_the_executable() {
        let exe = temp("portable-data");
        std::fs::write(exe.join("portable.txt"), b"").unwrap();
        let everywhere = [("HOME", "/home/u"), ("LOCALAPPDATA", "C:\\Users\\u\\AppData\\Local"), ("XDG_DATA_HOME", "/home/u/.data")];
        let dirs = AppDirs::resolve(Some(&exe.join("pdfcraft.exe")), env(&everywhere));
        assert_eq!(dirs.data, Some(exe.join(DATA_DIR)), "never the per-user folder in portable mode");
        let _ = std::fs::remove_dir_all(&exe);
    }

    #[test]
    fn the_chess_stores_and_the_recovery_folder_are_in_the_data_folder() {
        let everywhere = [("HOME", "/home/u"), ("LOCALAPPDATA", "C:\\Users\\u\\AppData\\Local"), ("XDG_DATA_HOME", "/home/u/.data")];
        let dirs = AppDirs::resolve(None, env(&everywhere));
        let data = dirs.data.clone().unwrap();
        assert_eq!(dirs.chess(), Some(data.join("chess")));
        let recovery = if cfg!(all(unix, not(target_os = "macos"))) { "recovery" } else { "Recovery" };
        assert_eq!(dirs.recovery(), Some(data.join(recovery)));
        assert_eq!(AppDirs::default().chess(), None, "no data folder, no stores");
        let made = temp("private").join("a").join("chess");
        create_private_dir(&made).unwrap();
        assert!(made.is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&made).unwrap().permissions().mode() & 0o777, 0o700);
        }
    }

    #[test]
    fn the_per_user_data_folder_is_the_platforms() {
        let everywhere = [("HOME", "/home/u"), ("LOCALAPPDATA", "C:\\Users\\u\\AppData\\Local"), ("XDG_DATA_HOME", "/home/u/.data")];
        let found = per_user_data(env(&everywhere));
        let expected = if cfg!(target_os = "macos") {
            PathBuf::from("/home/u").join("Library/Application Support/PdfCraft")
        } else if cfg!(windows) {
            PathBuf::from("C:\\Users\\u\\AppData\\Local").join("PdfCraft")
        } else {
            PathBuf::from("/home/u/.data").join("pdfcraft")
        };
        assert_eq!(found, Some(expected));
        // An empty variable is unset; with nothing to go by there is no folder.
        assert_eq!(per_user_data(env(&[("HOME", ""), ("LOCALAPPDATA", ""), ("XDG_DATA_HOME", "")])), None);
        if !cfg!(any(windows, target_os = "macos")) {
            assert_eq!(per_user_data(env(&[("HOME", "/home/u")])), Some(PathBuf::from("/home/u/.local/share").join("pdfcraft")));
        }
    }
}
