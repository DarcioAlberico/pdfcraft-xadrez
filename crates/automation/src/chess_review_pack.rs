//! Chess: a book's review taken to another machine, without the interface (plan A9-G5b, §2.3,
//! finding A8-19): `chess_review_pack_export` and `chess_review_pack_import`.
//!
//! **Export** writes one file, the review pack ([`pdfcraft_engine::chess::review_pack`]): the
//! diagram corrections, the decisions on the book's PGN and the games kept for a document, read
//! from the stores the tools use (`store`, `reference_store`, `games_store`; the app's own by
//! default), never the PGN's text, the path or its hash; the document's name only with
//! `include_name`. The pack names the book by its identifier, size and pages.
//!
//! **Import** reads a pack in two stages. First the whole pack is read and each part validated
//! by its kind's rules (an entry that can't be read, a correction past the last page a document
//! can have, is left out and counted as discarded). Then one change per kind merges it into the
//! chosen document's stores by that kind's rule, so importing the same pack twice changes nothing.
//! A pack of another book (another identifier, or another size and page count) is refused unless
//! the call says `force`. What can't be used is said: the corrections whose page the document
//! does not have now (orphans) and the decisions with no book's PGN kept to apply to.
//!
//! Both follow the document's security (`Automation::may_keep`): nothing for a document that does
//! not allow copying, an encrypted one's only with `allow_encrypted`. The games of a document a
//! running PdfCraft autosaves are merged into only with `merge: true`. Paths are confined to
//! `--root`.

use pdfcraft_engine::chess::corrections::{DocRecord, Located};
use pdfcraft_engine::chess::games_store::{self, GamesStoreError};
use pdfcraft_engine::chess::review_pack::{self, MAX_PACK_BYTES, Unpacked};
use serde_json::{Value, json};

use crate::chess_store_migrate::{GAMES, StoreAt};
use crate::{Args, Automation, Result, failed, write_atomic};

/// Whether `pack` (a pack's document) is `doc`: the same identifier when both have one, else the
/// same size and page count.
fn same_book(pack: &DocRecord, doc: &DocRecord) -> bool {
    match (&pack.id0, &doc.id0) {
        (Some(a), Some(b)) => a == b,
        _ => pack.size == doc.size && pack.pages == doc.pages,
    }
}

impl Automation {
    /// The games' store folder, to read (`None`: the app's, outside the folders the tools are
    /// confined to) or to change.
    fn games_folder(&self, a: &Args, write: bool) -> Result<Option<std::path::PathBuf>> {
        Ok(match self.chess_store_at(a, GAMES, write)? {
            StoreAt::Folder(dir) => Some(dir),
            StoreAt::OutsideRoot => None,
        })
    }

    pub(crate) fn chess_review_pack_export(&mut self, a: &Args) -> Result<Value> {
        let id = self.doc(a)?.id;
        self.may_keep(a, id)?;
        let shown = a.str("out")?;
        let out = self.resolve(shown, true)?;
        self.check_outputs(a, &[("out", out.as_path())], &[])?;
        let doc = self.session.get(id).ok_or_else(|| failed("no such document"))?;
        let (keys, pages) = (doc.doc_keys(), doc.info.pages.len());
        let fingerprints = self.session.chess_fingerprints(id).map(|f| f.pages().to_vec()).unwrap_or_default();
        let outside = || {
            failed(
                "the chess stores are kept in the app's data folder, outside the folders the tools are confined to: give store, reference_store and games_store",
            )
        };
        let corrections = self.correction_store_to_read(a)?.ok_or_else(outside)?.load_with(&keys, &fingerprints).map_err(failed)?.0.file;
        let reference = self.reference_store_to_read(a)?.ok_or_else(outside)?.load(&keys, pages).map_err(failed)?.file;
        let dir = self.games_folder(a, false)?.ok_or_else(outside)?;
        let games = games_folder(&dir).load(&keys, pages).map_err(failed)?.file;
        let name = a.opt_bool("include_name")?.unwrap_or(false);
        let pack = review_pack::pack(&DocRecord::new(&keys, pages), name, Some(corrections), Some(reference), Some(games));
        let bytes = pack.to_bytes().map_err(failed)?;
        write_atomic(&out, &bytes)?;
        let (corrections, decisions, games, trash) = pack.counts();
        Ok(json!({
            "written": self.shown_path(&out).0,
            "pack_id": pack.pack_id,
            "bytes": bytes.len(),
            "corrections": corrections,
            "decisions": decisions,
            "games": games,
            "trash": trash,
            "named": pack.doc.name.is_some(),
        }))
    }

    pub(crate) fn chess_review_pack_import(&mut self, a: &Args) -> Result<Value> {
        let id = self.doc(a)?.id;
        self.may_keep(a, id)?;
        let shown = a.str("path")?;
        let path = self.resolve(shown, false)?;
        // Stage 1: the whole pack read and validated before anything is changed.
        let bytes = crate::chess_files::read_capped(&path, shown, MAX_PACK_BYTES)?;
        let Unpacked { pack, skipped } = review_pack::unpack(&bytes).map_err(|e| failed(format!("{shown}: {e}")))?;
        let doc = self.session.get(id).ok_or_else(|| failed("no such document"))?;
        let (keys, pages) = (doc.doc_keys(), doc.info.pages.len());
        let here = DocRecord::new(&keys, pages);
        let other_book = !same_book(&pack.doc, &here);
        if other_book && !a.opt_bool("force")?.unwrap_or(false) {
            return Err(failed(format!(
                "{shown} is the review of another book ({} pages, {} bytes; this one has {pages} pages): pass force: true to import it into this one",
                pack.doc.pages,
                pack.doc.size.map_or_else(|| "?".to_owned(), |s| s.to_string())
            )));
        }
        let merge = a.opt_bool("merge")?.unwrap_or(false);
        let fingerprints = self.session.chess_fingerprints(id);
        // Stage 2: one change per kind, each merging by its kind's rule.
        let mut orphans = 0usize;
        let mut corrections = 0usize;
        if let Some(theirs) = &pack.corrections {
            let fps: Vec<String> = fingerprints.as_ref().map(|f| f.pages().to_vec()).unwrap_or_default();
            if let Some(f) = fingerprints.as_deref() {
                orphans = theirs.locate(f).iter().filter(|(at, _)| *at == Located::Orphan).count();
            }
            corrections = theirs.live().count();
            self.correction_store(a)?
                .update_with(&keys, &fps, |f| {
                    f.merge(theirs);
                    Ok::<(), pdfcraft_engine::chess::corrections::StoreError>(())
                })
                .map_err(failed)?;
        }
        let (mut decisions, mut without_pgn) = (0usize, 0usize);
        if let Some(theirs) = &pack.reference {
            let store = self.reference_store(a)?;
            let kept = store.load(&keys, pages).map_err(failed)?;
            if kept.file.pgn.is_none() {
                without_pgn = theirs.decisions.len();
            } else {
                decisions = theirs.decisions.len();
                store
                    .update(&keys, pages, |f| {
                        f.merge(theirs);
                        Ok::<(), pdfcraft_engine::chess::reference_store::ReferenceStoreError>(())
                    })
                    .map_err(failed)?;
            }
        }
        let (mut games, mut trash) = (0usize, 0usize);
        if let Some(theirs) = &pack.games {
            let dir = self.games_folder(a, true)?.ok_or_else(|| failed("the games' folder is outside the allowed folders: give games_store"))?;
            let instances = self.chess_data.earlier.as_ref().and_then(|e| e.parent()).map(std::path::Path::to_path_buf);
            games = theirs.live().count();
            trash = theirs.trash.len();
            games_folder(&dir)
                .update(&keys, pages, |f| {
                    if let Some(other) = f.instance.as_deref()
                        && !merge
                        && instances.as_ref().is_some_and(|d| instance_alive(d, other))
                    {
                        return Err(GamesStoreError::OtherInstance);
                    }
                    f.merge(theirs);
                    Ok(())
                })
                .map_err(failed)?;
        }
        Ok(json!({
            "pack_id": pack.pack_id,
            "other_book": other_book,
            "imported": { "corrections": corrections, "decisions": decisions, "games": games, "trash": trash },
            "discarded": { "corrections": skipped.corrections, "decisions": skipped.decisions, "games": skipped.games },
            "unmatched": { "corrections_orphan": orphans, "decisions_without_pgn": without_pgn },
        }))
    }
}

/// The games' store in the folder `dir`. The web build has no folders: there it keeps nothing.
#[cfg(not(target_arch = "wasm32"))]
fn games_folder(dir: &std::path::Path) -> games_store::DirStore {
    games_store::DirStore::new(dir)
}

#[cfg(target_arch = "wasm32")]
fn games_folder(_: &std::path::Path) -> games_store::MemStore {
    games_store::MemStore::new(0)
}

#[cfg(not(target_arch = "wasm32"))]
fn instance_alive(dir: &std::path::Path, name: &str) -> bool {
    games_store::instance_alive(dir, name)
}

#[cfg(target_arch = "wasm32")]
fn instance_alive(_: &std::path::Path, _: &str) -> bool {
    false
}
