//! Chess: `chess_evals`, the engine's evaluations kept for a document (plan A9-N8, A8-11), read
//! without the interface. No tool runs an engine (`pdfcraft_engine::chess::analysis`): the
//! evaluations are what the app's analysis kept, one file per document in the evaluations store
//! (`chess/evals` in the app's data folder, as the corrections are kept), only read here.

use pdfcraft_engine::chess::evals::{Eval, EvalFile, MAX_EVALS, position_key};
use serde_json::{Value, json};

use crate::chess_store_migrate::{EVALS, StoreAt};
use crate::{Args, Automation, Result, failed};

/// Where the evaluations are kept without the app's data folder (tests, an embedding).
pub(crate) const DEFAULT_STORE: &str = ".pdfcraft/chess-evals";

/// Most evaluations one reply lists.
pub const MAX_LISTED: usize = 1000;

#[cfg(not(target_arch = "wasm32"))]
type FolderStore = pdfcraft_engine::chess::evals::DirStore;
#[cfg(target_arch = "wasm32")]
type FolderStore = pdfcraft_engine::chess::evals::MemStore;

/// One evaluation for a reply, with its position.
fn eval_reply(position: &str, e: &Eval) -> Value {
    json!({
        "position": position,
        "depth": e.depth,
        "cp": e.cp,
        "mate": e.mate,
        "bound": e.bound.as_deref().unwrap_or("exact"),
        "pv": e.pv,
        "engine": e.engine,
        "at": e.at,
    })
}

/// The evaluations of `file` for a reply: the one of `fen`'s position, or the newest first, at
/// most `limit`.
fn evals_reply(file: &EvalFile, fen: Option<&str>, limit: usize) -> Result<Value> {
    if let Some(fen) = fen {
        let key = position_key(fen).ok_or_else(|| crate::ToolError::InvalidArgs("fen is not a FEN this build reads".into()))?;
        let listed: Vec<Value> = file.evals.get(&key).map(|e| eval_reply(&key, e)).into_iter().collect();
        return Ok(json!({ "positions": file.evals.len(), "evals": listed, "truncated": false }));
    }
    let mut all: Vec<(&String, &Eval)> = file.evals.iter().collect();
    all.sort_by(|a, b| b.1.at.cmp(&a.1.at).then_with(|| a.0.cmp(b.0)));
    let listed: Vec<Value> = all.iter().take(limit).map(|(k, e)| eval_reply(k, e)).collect();
    Ok(json!({ "positions": file.evals.len(), "evals": listed, "truncated": all.len() > limit }))
}

impl Automation {
    /// `chess_evals`: the evaluations kept for `doc` in `evals_store` (the app's by default),
    /// only read.
    pub(crate) fn chess_evals(&self, a: &Args) -> Result<Value> {
        let limit = a.opt_int("limit")?.map_or(MAX_LISTED, |n| usize::try_from(n.clamp(1, MAX_LISTED as i64)).unwrap_or(MAX_LISTED));
        let fen = a.opt_str("fen")?;
        let dir = match self.chess_store_at(a, EVALS, false)? {
            StoreAt::Folder(dir) => dir,
            StoreAt::OutsideRoot => {
                return Ok(json!({
                    "positions": 0, "evals": [], "outside_root": true,
                    "why": "the evaluations are kept in the app's data folder, outside the folders the tools are confined to: give evals_store to read them",
                }));
            }
        };
        #[cfg(not(target_arch = "wasm32"))]
        let store = FolderStore::new(dir);
        #[cfg(target_arch = "wasm32")]
        let store = {
            let _ = dir;
            FolderStore::new(0)
        };
        let id = self.doc(a)?.id;
        let doc = self.session.get(id).ok_or_else(|| failed("no such document"))?;
        let loaded = store.load(&doc.doc_keys(), doc.info.pages.len()).map_err(failed)?;
        let mut reply = evals_reply(&loaded.file, fen, limit)?;
        reply["file"] = json!(loaded.name);
        reply["max_positions"] = json!(MAX_EVALS);
        Ok(crate::chess_corrections::with_store_notes(reply, &loaded))
    }
}
