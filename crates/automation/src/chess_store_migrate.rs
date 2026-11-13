//! Chess: where the tools keep the diagram corrections and the books' PGNs (plan A9-G5a, decision
//! D17), and `chess_store_migrate`, which brings in what an earlier build kept elsewhere.
//!
//! **One place.** A frontend that knows the app's data folder (the command line and the MCP
//! server: [`Automation::with_chess_data`]) keeps both stores where the app does, in its `chess`
//! folder (`corrections`, `reference`), whatever the working folder. With `--root` or the
//! automation roots that folder must be inside them: when it is not, a reading tool reads nothing
//! and says so (`outside_root`), and a change is refused until `store` / `reference_store` names
//! a folder inside them. An `Automation` without one (tests, an embedding) keeps the stores in
//! `.pdfcraft/chess-corrections` and `.pdfcraft/chess-reference` of its root or working folder,
//! as before.
//!
//! **Bringing in what was kept elsewhere.** The first time the app's folder is used without
//! confinement, the recovery folder's `chess` stores (where the app kept them before) are merged
//! in; `chess_store_migrate` merges a `.pdfcraft` folder (or any folder of stores) on request.
//! Merging is the kinds' own rule ([`pdfcraft_engine::chess::store::DocStore::migrate_from`]): it
//! can run again, and the source is only read.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use pdfcraft_engine::chess::store::Migration;
use serde_json::{Value, json};

use crate::{Args, Automation, Result, failed};

/// Where a store is, as a tool resolved it.
pub(crate) enum StoreAt {
    /// A folder (it may not be there yet: then it keeps nothing).
    Folder(PathBuf),
    /// The app's folder, outside the folders the tools are confined to: nothing is read there.
    OutsideRoot,
}

/// The app's chess stores' folder, and the folder where earlier builds of the app kept them.
#[derive(Debug, Default)]
pub(crate) struct ChessData {
    pub dir: Option<PathBuf>,
    pub earlier: Option<PathBuf>,
    /// The earlier folder was merged in, this session.
    pub migrated: AtomicBool,
}

/// A kind of store: its argument, its folder in the app's `chess` folder, and its folder before.
#[derive(Clone, Copy)]
pub(crate) struct Kind {
    pub arg: &'static str,
    pub dir: &'static str,
    pub legacy: &'static str,
}

pub(crate) const CORRECTIONS: Kind = Kind { arg: "store", dir: "corrections", legacy: crate::chess_corrections::DEFAULT_STORE };
pub(crate) const REFERENCE: Kind = Kind { arg: "reference_store", dir: "reference", legacy: crate::chess_reference::DEFAULT_STORE };

/// The summary of one kind's migration, for a reply.
fn migration_reply(to: &str, m: &Migration) -> Value {
    json!({
        "to": to,
        "merged": m.merged,
        "changed": m.changed,
        "unchanged": m.unchanged,
        "newer": m.newer,
        "unreadable": m.unreadable,
        "left_here": m.left_here,
        "skipped": m.skipped,
    })
}

impl Automation {
    /// Keep the chess stores in `dir` (the app's `chess` folder: corrections and reference in it),
    /// and merge in, the first time it is used without confinement, the stores earlier builds
    /// kept in `earlier` (the recovery folder's `chess`). `None`: keep them in `.pdfcraft` of the
    /// root or working folder, as before plan A9-G5a.
    pub fn with_chess_data(mut self, dir: Option<PathBuf>, earlier: Option<PathBuf>) -> Self {
        self.chess_data = ChessData { dir, earlier, migrated: AtomicBool::new(false) };
        self
    }

    /// The folder of kind `kind`'s store, as `a` names it or by default, to change (`write`) or
    /// only to read. The default is the app's folder ([`Automation::with_chess_data`]): confined,
    /// it must be inside the roots; reading, a folder outside them reads as
    /// [`StoreAt::OutsideRoot`]; changing, it is refused.
    pub(crate) fn chess_store_at(&self, a: &Args, kind: Kind, write: bool) -> Result<StoreAt> {
        if let Some(given) = a.opt_str(kind.arg)? {
            let dir = if write { self.resolve(given, true)? } else { self.resolve_to_read(given)? };
            return Ok(StoreAt::Folder(dir));
        }
        let Some(app) = self.chess_data.dir.as_ref() else {
            let dir = if write { self.resolve(kind.legacy, true)? } else { self.resolve_to_read(kind.legacy)? };
            return Ok(StoreAt::Folder(dir));
        };
        let dir = app.join(kind.dir);
        if !self.confined() {
            self.migrate_earlier_once();
            return Ok(StoreAt::Folder(dir));
        }
        let shown = dir.to_string_lossy().into_owned();
        let access = if write { crate::file_access::Access::Write } else { crate::file_access::Access::Read };
        match self.resolve_for(&shown, access, true) {
            Ok(dir) => Ok(StoreAt::Folder(dir)),
            Err(_) if !write => Ok(StoreAt::OutsideRoot),
            Err(_) => Err(failed(format!(
                "the chess {} are kept in the app's data folder, outside the folders the tools are confined to: give {} (a folder inside them)",
                kind.dir, kind.arg
            ))),
        }
    }

    /// Merge the stores earlier builds of the app kept (the recovery folder's `chess`) into the
    /// app's folder, once a session. Never fails a tool: what can't be merged stays where it is,
    /// and is tried again next time.
    fn migrate_earlier_once(&self) {
        if self.chess_data.migrated.swap(true, Ordering::Relaxed) {
            return;
        }
        let (Some(dir), Some(earlier)) = (self.chess_data.dir.as_ref(), self.chess_data.earlier.as_ref()) else { return };
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = pdfcraft_platform::app_dirs::create_private_dir(dir);
            let corrections = pdfcraft_engine::chess::corrections::DirStore::new(dir.join(CORRECTIONS.dir));
            let _ = corrections.migrate_from(&earlier.join(CORRECTIONS.dir));
            let reference = pdfcraft_engine::chess::reference_store::DirStore::new(dir.join(REFERENCE.dir));
            let _ = reference.migrate_from(&earlier.join(REFERENCE.dir));
        }
        #[cfg(target_arch = "wasm32")]
        let _ = (dir, earlier);
    }

    /// The store's folder for a reply: relative to the root when confined, else as it is
    /// (`crate::chess_files` `shown_path`).
    fn store_shown(&self, dir: &std::path::Path) -> String {
        self.shown_path(dir).0
    }

    /// `chess_store_migrate`: merge the stores kept in `from` (a `.pdfcraft` folder, the app's
    /// `chess` folder or recovery folder, or a folder of store files) into the stores the tools
    /// use (`store`, `reference_store`, or the defaults).
    pub(crate) fn chess_store_migrate(&mut self, a: &Args) -> Result<Value> {
        let shown = a.str("from")?;
        let from = self.resolve(shown, false)?;
        if !from.is_dir() {
            return Err(failed(format!("{shown} is not a folder")));
        }
        let mut reply = serde_json::Map::new();
        for (kind, sources) in [(CORRECTIONS, ["chess-corrections", "corrections"]), (REFERENCE, ["chess-reference", "reference"])] {
            let StoreAt::Folder(to) = self.chess_store_at(a, kind, true)? else { return Err(failed("no store to migrate into")) };
            // The `.pdfcraft` layout, the app's `chess` layout, or the folder itself.
            let mut found: Vec<PathBuf> = sources.iter().map(|s| from.join(s)).filter(|d| d.is_dir()).collect();
            if found.is_empty() {
                found.push(from.clone());
            }
            let mut total = Migration::default();
            for source in found {
                let m = migrate(kind, &to, &source)?;
                total.merged = total.merged.saturating_add(m.merged);
                total.changed = total.changed.saturating_add(m.changed);
                total.unchanged = total.unchanged.saturating_add(m.unchanged);
                total.newer = total.newer.saturating_add(m.newer);
                total.unreadable = total.unreadable.saturating_add(m.unreadable);
                total.left_here = total.left_here.saturating_add(m.left_here);
                total.skipped = total.skipped.saturating_add(m.skipped);
            }
            reply.insert(kind.dir.to_owned(), migration_reply(&self.store_shown(&to), &total));
        }
        Ok(Value::Object(reply))
    }
}

/// Merge `source` into the store of kind `kind` in `to`.
#[cfg(not(target_arch = "wasm32"))]
fn migrate(kind: Kind, to: &std::path::Path, source: &std::path::Path) -> Result<Migration> {
    if kind.dir == CORRECTIONS.dir {
        pdfcraft_engine::chess::corrections::DirStore::new(to).migrate_from(source).map_err(failed)
    } else {
        pdfcraft_engine::chess::reference_store::DirStore::new(to).migrate_from(source).map_err(failed)
    }
}

/// The web build has no folders: nothing to migrate.
#[cfg(target_arch = "wasm32")]
fn migrate(_: Kind, _: &std::path::Path, _: &std::path::Path) -> Result<Migration> {
    Ok(Migration::default())
}
