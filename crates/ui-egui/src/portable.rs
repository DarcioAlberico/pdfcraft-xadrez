//! Portable mode (#157): a marker file beside the executable keeps the settings, log files,
//! crash-recovery autosaves and new digital IDs in `<exe dir>/PdfCraftData`, so a portable copy
//! (on a USB stick, say) writes nothing to `%APPDATA%` or `%LOCALAPPDATA%`. The Windows portable
//! zip ships with `portable.txt`; the MSI doesn't.
//!
//! It is decided with the app's other folders in [`pdfcraft_platform::app_dirs`] (plan A9-T3),
//! which the automation tools share; this module keeps the names the app uses and logs the
//! outcome once. If the folder can't be written (read-only medium, Program Files), portable mode
//! stays off, the per-user folders are used and the app says why.

use std::path::Path;
use std::sync::Once;

use pdfcraft_platform::app_dirs::{AppDirs, Portable};

/// Portable mode for this process, resolved once (the check probes the disk).
pub fn current() -> &'static Portable {
    static LOGGED: Once = Once::new();
    let portable = &AppDirs::current().portable;
    LOGGED.call_once(|| {
        if let Some(dir) = &portable.dir {
            log::info!("portable mode: settings are kept in {}", dir.display());
        }
        if let Some(unwritable) = &portable.unwritable {
            log::warn!("{unwritable}");
        }
    });
    portable
}

/// The portable data folder, when portable mode is on.
pub fn data_dir() -> Option<&'static Path> {
    current().dir.as_deref()
}
