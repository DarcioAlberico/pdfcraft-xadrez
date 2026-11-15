//! Chess ▸ Export the book's review… and Import a book's review… (plan A9-G5b, §2.3, finding
//! A8-19): a document's diagram corrections, its choices on the book's PGN and its chess games,
//! taken to another machine in one file, the review pack
//! ([`pdfcraft_engine::chess::review_pack`]), never the PGN's text, the document's path or its
//! name. The tools `chess_review_pack_export`/`_import` do the same without the interface.
//!
//! **What may go out or come in** follows what the app keeps of the document
//! ([`crate::chess_keep`]): nothing for a document that doesn't allow copying; for an encrypted
//! one, only after the user's yes (the question is put up, as for any first write), never after a
//! no. The tools ask for `allow_encrypted` where the app asks this question.
//!
//! **Export** takes what the app holds now: the corrections and choices from their stores, the
//! games from the list (written first, so every game has its uid and the same review gives the
//! same pack) with what the store keeps of them merged in. It says that the games' comments may
//! hold text of the book (the pack is for the user's own use).
//!
//! **Import** reads the whole pack first (at most [`MAX_PACK_BYTES`], each part by its kind's
//! rules), then merges each part by its kind's rule: the corrections into their store, the
//! choices into the book's PGN kept for the document (none without one: they are counted and
//! said), and the games into the list, those it has already (by uid) left out, so importing the
//! same pack twice changes nothing; the autosave writes them. A pack of another book (another
//! identifier, else another size or page count) is asked about first. What could not be used is
//! said, never dropped in silence: entries that could not be read, corrections for pages the
//! document doesn't have (kept, on no diagram), and choices waiting for the book's PGN.

use pdfcraft_engine::DocId;
use pdfcraft_engine::chess::BookPgn;
use pdfcraft_engine::chess::corrections::{DocRecord, Located};
use pdfcraft_engine::chess::reference_store::{Decisions, Reference, ReferenceFile, ReferenceStoreError};
use pdfcraft_engine::chess::review_pack::{self, MAX_PACK_BYTES, Pack, Unpacked};
use pdfcraft_engine::chess::store::StoreKind;

use crate::PdfCraftApp;
use crate::chess_keep::Keeping;
use crate::chess_store_ui::in_store;

/// An import of another book's review, waiting for the user's answer.
pub(crate) struct PendingImport {
    doc: DocId,
    /// The file's name, as the question says it.
    name: String,
    unpacked: Unpacked,
}

/// What an import brought in, and what it could not use.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Imported {
    corrections: usize,
    decisions: usize,
    games: usize,
    /// Corrections for pages the document doesn't have: kept, on no diagram.
    orphans: usize,
    /// Choices not imported: the document keeps no book's PGN to apply them to.
    without_pgn: usize,
    /// Entries of the pack that could not be read, left out.
    unreadable: usize,
}

/// The file name a document's review is offered under.
fn file_name(doc_name: &str) -> String {
    let stem = doc_name.trim_end_matches(".pdf").trim_end_matches(".PDF");
    format!("{stem}.chess-review.json")
}

impl PdfCraftApp {
    /// Whether document `id`'s review may be taken out or brought in now: not when copying is
    /// not allowed (said), nor for an encrypted document before the user's yes (the question is
    /// put up) or after a no (said).
    fn chess_review_allowed(&mut self, id: DocId) -> bool {
        if !self.may_copy(id) {
            return false;
        }
        match self.chess_keep(id) {
            Keeping::Yes => true,
            Keeping::Ask => false,
            Keeping::No => {
                self.notify(
                    tl!("The chess of this encrypted document is not kept, as you said: a review can't be exported from it or imported into it.")
                        .to_string(),
                );
                false
            }
        }
    }

    /// Chess ▸ Export the book's review…: ask where (`save_override` in tests; a download on the
    /// web), then write the active document's review pack there.
    pub fn export_chess_review(&mut self) {
        let Some((_, id)) = self.active_ids() else { return };
        if !self.chess_review_allowed(id) {
            return;
        }
        let Some(name) = self.session.get(id).map(|d| file_name(&d.name)) else { return };
        #[cfg(not(target_arch = "wasm32"))]
        {
            let write = move |app: &mut Self, path: std::path::PathBuf| app.export_chess_review_to(id, &path);
            match self.save_override.clone() {
                Some(p) => write(self, p.into()),
                None => {
                    let dialog = rfd::AsyncFileDialog::new()
                        .set_title(tl!("Export the book's review…").to_string())
                        .add_filter("JSON", &["json"])
                        .set_file_name(name);
                    self.ask_one(crate::pickers::Ask::Save(dialog), None, write);
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let Some((pack, bytes)) = self.chess_review_bytes(id) else { return };
            match crate::editing::download(&name, &bytes) {
                Ok(()) => self.say_chess_review_exported(&pack, &name),
                Err(e) => self.post_error(e.to_string()),
            }
        }
    }

    /// Write document `id`'s review pack to `path`, and say what went.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn export_chess_review_to(&mut self, id: DocId, path: &std::path::Path) {
        if !self.chess_review_allowed(id) {
            return;
        }
        let Some((pack, bytes)) = self.chess_review_bytes(id) else { return };
        let shown = path.display().to_string();
        match crate::editing::write_atomically(&path.to_string_lossy(), &bytes) {
            Ok(()) => self.say_chess_review_exported(&pack, &shown),
            Err(e) => self.post_error_fmt("Couldn't write {name}: {e}", &[("name", &shown), ("e", &e.to_string())]),
        }
    }

    /// Document `id`'s review pack and its bytes; `None`, said, when it keeps nothing or the pack
    /// can't be made.
    fn chess_review_bytes(&mut self, id: DocId) -> Option<(Pack, Vec<u8>)> {
        let pack = self.chess_review_pack(id)?;
        if pack.corrections.is_none() && pack.reference.is_none() && pack.games.is_none() {
            self.notify(tl!("There is nothing to export yet: this document keeps no correction, choice or game.").to_string());
            return None;
        }
        match pack.to_bytes() {
            Ok(bytes) => Some((pack, bytes)),
            Err(e) => {
                self.post_error(e.to_string());
                None
            }
        }
    }

    /// Document `id`'s review as the app holds it now: its corrections, its choices on the book's
    /// PGN and its games (the list written first, so each game has its uid).
    pub(crate) fn chess_review_pack(&mut self, id: DocId) -> Option<Pack> {
        let corrections = self.chess_corrections_file(id);
        let reference = self.chess_review_reference(id);
        self.flush_chess_games();
        let games = self.chess.games.review_games(id);
        let doc = self.session.get(id)?;
        Some(review_pack::pack(&DocRecord::new(&doc.doc_keys(), doc.info.pages.len()), false, corrections, reference, games))
    }

    /// What the store keeps of document `id`'s book's PGN, with its choices, when it keeps one.
    fn chess_review_reference(&mut self, id: DocId) -> Option<ReferenceFile> {
        // Loaded, so the store's file is the one the session holds.
        self.book_pgn_index(id)?;
        let doc = self.session.get(id)?;
        let (keys, pages) = (doc.doc_keys(), doc.info.pages.len());
        let store = &self.chess.book_pgns.store;
        in_store!(store.reading(&keys), s => s.load(&keys, pages)).ok().map(|l| l.file)
    }

    /// Say what an export wrote to `shown`.
    fn say_chess_review_exported(&mut self, pack: &Pack, shown: &str) {
        let (corrections, decisions, games, _) = pack.counts();
        self.notify(crate::i18n::fmt(
            tl!(
                "Review exported to {path} (corrections: {corrections}, choices: {decisions}, games: {games}). The games' comments may hold the book's text: keep the file for your own use."
            ),
            &[("path", shown), ("corrections", &corrections.to_string()), ("decisions", &decisions.to_string()), ("games", &games.to_string())],
        ));
    }

    /// Chess ▸ Import a book's review…: choose a review pack and merge it into the active
    /// document's review.
    pub fn import_chess_review(&mut self) {
        let Some((_, id)) = self.active_ids() else { return };
        if !self.chess_review_allowed(id) {
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let dialog = rfd::AsyncFileDialog::new().set_title(tl!("Import a book's review…").to_string()).add_filter("JSON", &["json"]);
            self.ask(crate::pickers::Ask::File(dialog), None, move |app, paths| {
                let Some(path) = paths.into_iter().next() else { return };
                let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
                match pdfcraft_engine::chess::corrections::read_capped(&path, MAX_PACK_BYTES) {
                    Ok(bytes) => app.import_chess_review_bytes(id, &name, &bytes),
                    Err(e) => app.post_error_fmt("Couldn't read {name}: {e}", &[("name", &name), ("e", &e.to_string())]),
                }
            });
        }
        #[cfg(target_arch = "wasm32")]
        {
            let uploads = self.chess.review_uploads.clone();
            let ctx = self.ctx.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let Some(h) = rfd::AsyncFileDialog::new().add_filter("JSON", &["json"]).pick_file().await else { return };
                let bytes = crate::chess_models::read_picked(&h, MAX_PACK_BYTES).await;
                uploads.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push((id, h.file_name(), bytes));
                if let Some(ctx) = ctx {
                    ctx.request_repaint();
                }
            });
        }
    }

    /// The review packs the browser has read since the last frame, imported.
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn take_chess_review_uploads(&mut self) {
        let picked = std::mem::take(&mut *self.chess.review_uploads.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
        for (id, name, bytes) in picked {
            match bytes {
                Ok(bytes) => self.import_chess_review_bytes(id, &name, &bytes),
                Err(e) => self.post_error_fmt("Couldn't read {name}: {e}", &[("name", &name), ("e", &e)]),
            }
        }
    }

    /// Import the review pack `bytes` (the file `name`) into document `id`: read whole first; a
    /// pack of another book is asked about before anything changes.
    pub fn import_chess_review_bytes(&mut self, id: DocId, name: &str, bytes: &[u8]) {
        if !self.chess_review_allowed(id) {
            return;
        }
        let unpacked = match review_pack::unpack(bytes) {
            Ok(u) => u,
            Err(e) => return self.post_error_fmt("Couldn't read {name}: {e}", &[("name", name), ("e", &e)]),
        };
        let Some(doc) = self.session.get(id) else { return };
        let here = DocRecord::new(&doc.doc_keys(), doc.info.pages.len());
        if review_pack::same_book(&unpacked.pack.doc, &here) {
            self.merge_chess_review(id, name, &unpacked);
        } else {
            self.chess.review_import = Some(PendingImport { doc: id, name: name.to_owned(), unpacked });
        }
    }

    /// Merge `unpacked` (the file `name`) into document `id`'s review, each part by its kind's
    /// rule, and say what came in and what could not be used.
    fn merge_chess_review(&mut self, id: DocId, name: &str, unpacked: &Unpacked) {
        let pack = &unpacked.pack;
        let skipped = unpacked.skipped;
        let mut done =
            Imported { unreadable: skipped.corrections.saturating_add(skipped.decisions).saturating_add(skipped.games), ..Imported::default() };
        let mut failed: Vec<String> = Vec::new();
        if let Some(theirs) = &pack.corrections {
            match self.merge_review_corrections(id, theirs) {
                Ok(orphans) => {
                    done.corrections = theirs.live().count();
                    done.orphans = orphans;
                }
                Err(e) => failed.push(crate::i18n::fmt(tl!("The diagram corrections couldn't be saved: {e}"), &[("e", &e)])),
            }
        }
        if let Some(theirs) = &pack.reference {
            match self.merge_review_decisions(id, theirs) {
                Ok(true) => done.decisions = theirs.decisions.len(),
                Ok(false) => done.without_pgn = theirs.decisions.len(),
                Err(e) => failed.push(crate::i18n::fmt(tl!("The choice was not kept: {e}"), &[("e", &e)])),
            }
        }
        if let Some(theirs) = &pack.games {
            // The document's list made first, from what is stored for it.
            self.sync_chess_docs();
            done.games = self.chess.games.take_review(id, theirs);
        }
        let mut said = crate::i18n::fmt(
            tl!("Review imported from {name} (corrections: {corrections}, choices: {decisions}, games: {games})."),
            &[
                ("name", name),
                ("corrections", &done.corrections.to_string()),
                ("decisions", &done.decisions.to_string()),
                ("games", &done.games.to_string()),
            ],
        );
        let more = [
            (done.orphans, tl!("Corrections for pages this document doesn't have: {n} (kept, on no diagram).")),
            (done.without_pgn, tl!("Choices waiting for the book's PGN: {n} (load the PGN, then import the review again).")),
            (done.unreadable, tl!("Entries that couldn't be read and were left out: {n}.")),
        ];
        for (n, text) in more {
            if n > 0 {
                said.push(' ');
                said.push_str(&crate::i18n::fmt(text, &[("n", &n.to_string())]));
            }
        }
        let any_failed = !failed.is_empty();
        for f in failed {
            said.push(' ');
            said.push_str(&f);
        }
        if any_failed { self.post_error(said) } else { self.notify(said) }
    }

    /// Merge `theirs` into document `id`'s corrections: how many of them are for pages the
    /// document doesn't have (located by its page fingerprints).
    fn merge_review_corrections(&mut self, id: DocId, theirs: &pdfcraft_engine::chess::corrections::CorrectionFile) -> Result<usize, String> {
        let closed = || tl!("the document is closed").to_string();
        if !self.load_chess_corrections(id) {
            return Err(closed());
        }
        let fingerprints = self.session.chess_fingerprints(id).ok_or_else(closed)?;
        let keys = self.session.get(id).ok_or_else(closed)?.doc_keys();
        let orphans = theirs.locate(fingerprints.as_ref()).iter().filter(|(at, _)| *at == Located::Orphan).count();
        let write = self.chess_corrections_place(id)?;
        let (_, loaded) = self.chess.corrections.store.update(write, &keys, fingerprints.pages(), |f| {
            f.merge(theirs);
            Ok(())
        })?;
        self.chess.corrections.said = false;
        self.say_corrections_set_aside(&loaded);
        self.chess_corrections_kept(id, keys, loaded.file);
        Ok(orphans)
    }

    /// Merge the choices of `theirs` into the book's PGN document `id` keeps: whether it keeps
    /// one (without one, nothing is merged: a choice is about a move of the PGN).
    fn merge_review_decisions(&mut self, id: DocId, theirs: &ReferenceFile) -> Result<bool, String> {
        if self.book_pgn_index(id).is_none() {
            return Ok(false);
        }
        let Some(doc) = self.session.get(id) else { return Ok(false) };
        let (keys, pages) = (doc.doc_keys(), doc.info.pages.len());
        let write = self.book_pgn_place(id)?;
        let (_, loaded) = self.chess.book_pgns.store.change(write, &keys, pages, |place| {
            in_store!(place, s => s.update(&keys, pages, |f| {
                Reference::merge(f, theirs);
                Ok::<(), ReferenceStoreError>(())
            }))
            .map_err(|e| e.to_string())
        })?;
        if let Some(kept) = self.session.chess_book_pgn(id) {
            let pgn = BookPgn { decisions: Decisions::of(&loaded.file), ..BookPgn::clone(&kept) };
            self.session.set_chess_book_pgn(id, Some(pgn));
        }
        Ok(true)
    }
}

/// The question before importing another book's review: Import anyway, or Cancel (Esc, a click
/// outside).
pub(crate) fn prompt(app: &mut PdfCraftApp, ctx: &egui::Context) {
    let Some(pending) = &app.chess.review_import else { return };
    // The document was closed meanwhile: nothing to ask about.
    let Some(doc) = app.session.get(pending.doc) else {
        app.chess.review_import = None;
        return;
    };
    let body = crate::i18n::fmt(
        tl!("{name} is the review of another book ({pages} pages; this one has {here}). Import it into this document anyway?"),
        &[("name", &pending.name), ("pages", &pending.unpacked.pack.doc.pages.to_string()), ("here", &doc.info.pages.len().to_string())],
    );
    let mut answer = None;
    let shown = crate::widgets::show_titled_modal(ctx, "chess-review-import", tl!("Another book's review"), |ui| {
        ui.set_max_width(460.0);
        ui.add(egui::Label::new(body).wrap());
        ui.add_space(10.0);
        ui.horizontal_wrapped(|ui| {
            if crate::widgets::pill_button(ui, tl!("Import anyway"), false).clicked() {
                answer = Some(true);
            }
            // The keyboard starts on the answer that changes nothing.
            let cancel = crate::widgets::pill_button(ui, tl!("Cancel"), true);
            crate::widgets::dialog_focus(&cancel);
            if cancel.clicked() {
                answer = Some(false);
            }
        });
    });
    if answer.is_none() && shown.should_close() {
        answer = Some(false);
    }
    if let Some(yes) = answer
        && let Some(pending) = app.chess.review_import.take()
        && yes
    {
        app.merge_chess_review(pending.doc, &pending.name, &pending.unpacked);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The file offered is named after the document, never its folder.
    #[test]
    fn the_review_file_is_named_after_the_document() {
        assert_eq!(file_name("Harikrishna.pdf"), "Harikrishna.chess-review.json");
        assert_eq!(file_name("book.PDF"), "book.chess-review.json");
        assert_eq!(file_name("notes"), "notes.chess-review.json");
    }
}
