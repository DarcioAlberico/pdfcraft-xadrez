//! Chess ▸ Diagrams: the corrections kept on disk, without the interface (plan A8, P3a1):
//! `chess_correction_list`, `chess_correction_set` and `chess_correction_delete`, one verb each.
//!
//! The corrections live in a folder confined to `--root` (`store`, [`DEFAULT_STORE`] by default),
//! one file per document, in the format of `pdfcraft_engine::chess::corrections`; a tool lists and
//! changes the corrections of the document it is given, never another's.

use pdfcraft_engine::DocId;
use pdfcraft_engine::chess::corrections::{
    CorrectionError, CorrectionStore, DirStore, Kind, Located, NewCorrection, Original, StoreError, normalize,
};
use pdfcraft_engine::chess::pdfcraft_chess::game::Game;
use pdfcraft_engine::chess::{ChessDocument, DocKeys};
use serde_json::{Value, json};

use crate::{Args, Automation, Result, ToolError, failed};

/// The corrections folder when `store` is not given (relative to `--root` when set).
pub(crate) const DEFAULT_STORE: &str = ".pdfcraft/chess-corrections";

/// Seconds since 1970, now.
fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Verified => "verified",
        Kind::Corrected => "corrected",
    }
}

impl Automation {
    /// The corrections folder `store` names, confined to the root.
    fn correction_store(&self, a: &Args) -> Result<DirStore> {
        let store = a.opt_str("store")?.unwrap_or(DEFAULT_STORE);
        Ok(DirStore::new(self.resolve(store, true)?))
    }

    /// Before a reading tool reads document `id`: give it the corrections `store` keeps for it,
    /// unless `corrections` is false, so its readings, exports and links take the corrected
    /// positions (plan A8, P3a3). What was loaded, for the reply; `null` when not asked.
    pub(crate) fn use_store_corrections(&mut self, a: &Args, id: DocId) -> Result<Value> {
        if !a.opt_bool("corrections")?.unwrap_or(true) {
            self.session.set_chess_corrections(id, None);
            return Ok(Value::Null);
        }
        let store = self.correction_store(a)?;
        let doc = self.session.get(id).ok_or_else(|| failed("no such document"))?;
        let (keys, pages) = (doc.doc_keys(), doc.info.pages.len());
        let loaded = match store.load(&keys, pages) {
            Ok(l) => l,
            // A document with no name, path or identifier has nothing kept under it.
            Err(StoreError::NoKeys) => {
                self.session.set_chess_corrections(id, None);
                return Ok(json!({ "file": Value::Null, "kept": 0 }));
            }
            Err(e) => return Err(failed(format!("{e} (pass corrections: false to read without the corrections)"))),
        };
        let kept = loaded.file.live().count();
        let reply = json!({
            "file": loaded.name,
            "kept": kept,
            "read_only": loaded.read_only,
            "set_aside": loaded.set_aside.map(|(name, why)| json!({ "file": name, "why": why })),
        });
        self.session.set_chess_corrections(id, Some(loaded.file));
        Ok(reply)
    }

    /// After document `id` was saved: its corrections in the default folder, found under the
    /// keys it had `before`, recorded under its keys now. Whether there were any.
    pub(crate) fn rekey_chess_corrections(&self, id: DocId, before: &DocKeys) -> std::result::Result<bool, String> {
        let Ok(dir) = self.resolve(DEFAULT_STORE, true) else { return Ok(false) };
        let doc = self.session.get(id).ok_or("no such document")?;
        DirStore::new(dir).rekey(before, &doc.doc_keys(), doc.info.pages.len()).map_err(|e| e.to_string())
    }

    pub(crate) fn chess_correction_list(&self, a: &Args) -> Result<Value> {
        let store = self.correction_store(a)?;
        let doc = self.doc(a)?;
        let (keys, fingerprints) = (doc.doc_keys(), doc.page_fingerprints());
        let loaded = store.load(&keys, fingerprints.len()).map_err(failed)?;
        let located = loaded.file.locate(&fingerprints);
        let orphans = located.iter().filter(|(at, _)| *at == Located::Orphan).count();
        let corrections: Vec<Value> = located
            .into_iter()
            .map(|(at, e)| {
                json!({
                    "uid": e.uid,
                    "page": at.page().map(|p| p + 1),
                    "located": at.name(),
                    "recorded_page": e.page + 1,
                    "rect": e.rect,
                    "fen": e.fen,
                    "kind": kind_name(e.kind),
                    "original_fen": e.orig.as_ref().and_then(|o| o.fen.clone()),
                    "at": e.at,
                    "rev": e.rev,
                })
            })
            .collect();
        Ok(json!({
            "file": loaded.name,
            "read_only": loaded.read_only,
            "set_aside": loaded.set_aside.map(|(name, why)| json!({ "file": name, "why": why })),
            "count": corrections.len(),
            "orphans": orphans,
            "corrections": corrections,
        }))
    }

    pub(crate) fn chess_correction_set(&mut self, a: &Args) -> Result<Value> {
        let store = self.correction_store(a)?;
        let page = self.page(a)?;
        let index = usize::try_from(a.int("index")?).ok().and_then(|i| i.checked_sub(1)).ok_or_else(|| Args::wrong("index", "1 or more"))?;
        let kind = match a.opt_str("kind")? {
            None | Some("corrected") => Kind::Corrected,
            Some("verified") => Kind::Verified,
            Some(_) => return Err(Args::wrong("kind", "\"corrected\" or \"verified\"")),
        };
        let settings = self.chess_settings(a)?;
        let id = self.doc(a)?.id;
        let scanned = self.session.chess_scan(id, &[page], &settings, true).map_err(failed)?;
        let found = scanned.first().ok_or_else(|| failed("the page could not be scanned"))?;
        let count = found.diagrams.len();
        let diagram = found
            .diagrams
            .get(index)
            .ok_or_else(|| ToolError::InvalidArgs(format!("index {} is out of range: page {} has {count} diagrams", index + 1, page + 1)))?;
        let crop = found.geometry.map(|g| g.crop_box()).ok_or_else(|| failed("the page's crop box could not be read"))?;
        let b = diagram.candidate.bbox_user;
        let rect = normalize([b.x0, b.y0, b.x1, b.y1], [crop.x0, crop.y0, crop.x1, crop.y1])
            .ok_or_else(|| failed("the diagram's box is not on the page"))?;
        let read = found.readings.as_ref().and_then(|r| r.get(index)).and_then(|r| r.position()).map(|p| p.fen.clone());
        // Verified says the reading was right: the reading's position unless fen is given.
        let fen = match (a.opt_str("fen")?, kind, &read) {
            (Some(fen), _, _) => fen.trim().to_owned(),
            (None, Kind::Verified, Some(read)) => read.clone(),
            (None, Kind::Verified, None) => return Err(failed("the diagram was not read, so there is no reading to verify: give fen")),
            (None, Kind::Corrected, _) => return Err(ToolError::InvalidArgs("give fen, the corrected position".into())),
        };
        let doc = self.doc(a)?;
        let (keys, fingerprints) = (doc.doc_keys(), doc.page_fingerprints());
        let page_fp = fingerprints.get(page).cloned().ok_or_else(|| failed("the page's fingerprint could not be made"))?;
        let new = NewCorrection { page, page_fp, rect, orig: Some(Original { fen: read, model: None }), fen: fen.clone(), kind };
        let at = now();
        let (uid, loaded) = store.update(&keys, fingerprints.len(), |f| f.set(new, at)).map_err(failed)?;
        let rev = loaded.file.entries.iter().find(|e| e.uid == uid).map_or(0, |e| e.rev);
        Ok(json!({
            "uid": uid,
            "page": page + 1,
            "index": index + 1,
            "rect": rect,
            "fen": fen,
            "kind": kind_name(kind),
            "legal": Game::from_fen(&fen).is_ok(),
            "rev": rev,
            "file": loaded.name,
        }))
    }

    pub(crate) fn chess_correction_delete(&self, a: &Args) -> Result<Value> {
        let store = self.correction_store(a)?;
        let uid = a.str("uid")?.to_owned();
        let doc = self.doc(a)?;
        let (keys, pages) = (doc.doc_keys(), doc.page_fingerprints().len());
        let at = now();
        let deleted = store.update(&keys, pages, |f| if f.delete(&uid, at) { Ok(()) } else { Err(CorrectionError::NoSuchCorrection(uid.clone())) });
        match deleted {
            Ok((_, loaded)) => Ok(json!({ "uid": uid, "deleted": true, "file": loaded.name })),
            Err(e) => Err(failed(format!("{e} (see chess_correction_list)"))),
        }
    }
}

/// The corrections a reading used, for a reading tool's reply: `loaded` (what
/// [`Automation::use_store_corrections`] gave) with the corrections applied and those whose
/// diagram was not found.
pub(crate) fn corrections_reply(mut loaded: Value, read: &ChessDocument) -> Value {
    if let Some(o) = loaded.as_object_mut() {
        o.insert(
            "applied".into(),
            read.corrections
                .iter()
                .map(|c| {
                    json!({
                        "uid": c.uid,
                        "page": c.page + 1,
                        "index": c.index + 1,
                        "kind": kind_name(c.kind),
                        "fen": c.fen,
                        "read_fen": c.read_fen,
                        "diverged": c.diverged,
                    })
                })
                .collect(),
        );
        o.insert(
            "unmatched".into(),
            read.unmatched_corrections.iter().map(|u| json!({ "uid": u.uid, "page": u.page + 1, "reason": u.reason })).collect(),
        );
    }
    loaded
}
