//! Chess ▸ PGN toolbar ▸ Collecting corrected characters for retraining (T11, decision D9: opt-in).
//!
//! The glyph model the OCR area reads with is the user's own, trained by ChessVisionOFF on 32 × 32
//! gray pictures of characters, one folder per character. When the user turns on **Collect
//! corrected characters for retraining** (in the capture chip's menu, off by default; the
//! desktop only, as it writes to a folder the user picks) every character of an OCR area that is
//! corrected by hand in the capture preview (T10) is saved as such a picture, so the model can
//! learn from its mistakes:
//!
//! - a reading picked for a character saves that character's box as what was picked;
//! - a move typed in the correction field (or a legal move clicked) saves each character whose box
//!   kept its place, when the move has as many characters as the reader cut it into, each read as
//!   one character by the glyph reader, as what it became.
//!
//! **Where.** `<folder>/<class folder>/<confidence>_<document>_p<page>_<n>.png`, the layout of
//! ChessVisionOFF's review queue (`revisao_ocr`), so its review and promotion steps take the files
//! as they are: the class folder named as ChessVisionOFF names its classes, the name starting with
//! the model's confidence in thousandths (the most doubtful first), the document a 16-digit hash
//! of its bytes, the page 1-based, and `n` counting up past any file already there (nothing is
//! ever overwritten). The names and places are the engine's ([`pdfcraft_engine::chess::samples`]),
//! which the automation tool `chess_glyph_sample` uses too (wave 10, U3).
//!
//! **Rules.** Saving is extraction: a document that doesn't allow copying never gives samples
//! (it can't be read by the OCR area anyway), and an encrypted one is asked about first, once per
//! document and session, as its games' autosave is (`chess_collection`): declined, nothing of it
//! is written. Files are written atomically (`editing::write_atomically`), at most
//! [`MAX_SAMPLES`] a session (the user is told when the cap is reached). Nothing is trained in the
//! app, and nothing leaves the computer.

use std::collections::HashMap;
use std::path::PathBuf;

use egui::RichText;
use pdfcraft_engine::DocId;
use pdfcraft_engine::chess::samples;

use crate::chess_ui::ChessState;
use crate::theme::Tokens;

/// Most samples saved in one session.
pub const MAX_SAMPLES: usize = 2000;
/// Most corrections waiting to be saved, or waiting for the user's answer.
const MAX_WAITING: usize = 256;
/// Longest folder kept from the settings.
const MAX_FOLDER_LEN: usize = 4096;

/// A character of an OCR area corrected by hand: what a sample is made of.
#[derive(Clone, Debug, PartialEq)]
pub struct GlyphFix {
    /// The document the area was read in, and its page (0-based).
    pub doc: DocId,
    pub page: usize,
    /// The character's box, in view space, and the resolution the area was read at.
    pub rect: [f32; 4],
    pub dpi: f32,
    /// What the character is (the corrected reading).
    pub text: String,
    /// The model's probability for its first reading, from 0 to 1.
    pub confidence: f32,
}

/// The collection's state, kept in `CaptureState`.
#[derive(Debug, Default)]
pub struct Samples {
    /// Collect (a preference, off by default).
    pub on: bool,
    /// The folder samples go to (a preference).
    pub folder: Option<PathBuf>,
    /// Samples saved this session.
    pub written: usize,
    /// The answer for each encrypted document: save its samples or not.
    consent: HashMap<DocId, bool>,
    /// Corrections to save at the next tick.
    queue: Vec<GlyphFix>,
    /// Corrections of encrypted documents waiting for the user's answer.
    held: Vec<GlyphFix>,
    /// The folder picker is to be shown at the next tick.
    pick: bool,
    /// The cap was reached and said.
    capped: bool,
    /// Each document's hash, as file names use it.
    hashes: HashMap<DocId, String>,
}

impl Samples {
    /// Take `fixes` to save, when collecting.
    pub(crate) fn collect(&mut self, fixes: Vec<GlyphFix>) {
        if !self.on || self.folder.is_none() {
            return;
        }
        let room = MAX_WAITING.saturating_sub(self.queue.len());
        self.queue.extend(fixes.into_iter().take(room));
    }

    /// The encrypted document whose samples wait for the user's answer, if any.
    pub fn asking(&self) -> Option<DocId> {
        self.held.first().map(|f| f.doc)
    }

    /// The user's answer for document `doc`: its samples are saved from now on, or never.
    pub fn answer(&mut self, doc: DocId, save: bool) {
        self.consent.insert(doc, save);
        let (mine, others): (Vec<GlyphFix>, Vec<GlyphFix>) = std::mem::take(&mut self.held).into_iter().partition(|f| f.doc == doc);
        self.held = others;
        if save {
            self.queue.extend(mine);
        }
    }

    /// The preferences, for the settings file.
    pub(crate) fn to_json(&self) -> serde_json::Value {
        serde_json::json!({ "on": self.on, "folder": self.folder.as_ref().map(|f| f.to_string_lossy().into_owned()) })
    }

    /// The preferences from the settings file (untrusted: a malformed value is ignored).
    pub(crate) fn restore(&mut self, v: &serde_json::Value) {
        if cfg!(target_arch = "wasm32") {
            return;
        }
        if let Some(on) = v["on"].as_bool() {
            self.on = on;
        }
        if let Some(folder) = v["folder"].as_str().filter(|f| !f.is_empty() && f.len() <= MAX_FOLDER_LEN) {
            self.folder = Some(PathBuf::from(folder));
        }
    }
}

/// Write one sample file atomically, making its folder first.
fn write_file(path: &std::path::Path, png: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = path.to_str().ok_or_else(|| std::io::Error::other("the folder's path is not valid Unicode"))?;
    crate::editing::write_atomically(text, png)
}

// ─── The interface ───────────────────────────────────────────────────────────────────────────

/// The capture chip menu's part: the switch and the folder (the desktop only).
pub(crate) fn menu(ui: &mut egui::Ui, t: &Tokens, chess: &mut ChessState) {
    if cfg!(target_arch = "wasm32") {
        return;
    }
    let s = &mut chess.capture.samples;
    ui.separator();
    let r = ui.add(crate::widgets::checkbox(&mut s.on, tl!("Collect corrected characters for retraining")));
    if r.changed() && s.on && s.folder.is_none() {
        s.pick = true;
    }
    r.on_hover_text(tl!(
        "Each character you correct in an OCR area is saved as a 32 × 32 picture, in a folder per character, to train the glyph model with. Nothing leaves this computer."
    ));
    ui.horizontal(|ui| {
        let shown = s.folder.as_ref().map_or_else(|| tl!("No folder chosen").to_owned(), |f| f.to_string_lossy().into_owned());
        ui.label(RichText::new(shown).small().color(t.text_muted));
        if ui.small_button(tl!("Choose folder…")).clicked() {
            s.pick = true;
            ui.close();
        }
    });
}

/// The question for an encrypted document whose corrected characters wait, over the page area
/// `area` of document `doc`. Returns the answer.
pub(crate) fn ask(ctx: &egui::Context, area: egui::Rect, chess: &ChessState, doc: DocId) -> Option<bool> {
    if chess.capture.samples.asking() != Some(doc) {
        return None;
    }
    let mut answer = None;
    egui::Area::new(egui::Id::new("chess-samples-ask"))
        .order(egui::Order::Foreground)
        .pivot(egui::Align2::LEFT_BOTTOM)
        .fixed_pos(egui::pos2(area.left() + 16.0, area.bottom() - 24.0))
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.label(RichText::new(tl!("Save character samples from this protected document?")).strong());
                ui.label(RichText::new(tl!("They are saved unencrypted, in the folder you chose for retraining.")).small());
                ui.horizontal(|ui| {
                    if ui.button(tl!("Save them")).clicked() {
                        answer = Some(true);
                    }
                    if ui.button(tl!("Don't save")).clicked() {
                        answer = Some(false);
                    }
                });
            });
        });
    answer
}

impl crate::PdfCraftApp {
    /// Every frame: show the folder picker when asked, and save the corrections waiting.
    pub(crate) fn poll_chess_samples(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if std::mem::take(&mut self.chess.capture.samples.pick) {
            let dialog = rfd::AsyncFileDialog::new().set_title(tl!("Choose the folder for character samples"));
            self.ask_one(crate::pickers::Ask::Folder(dialog), None, |app, path| {
                app.chess.capture.samples.folder = Some(path);
                app.chess.capture.samples.on = true;
            });
        }
        for fix in std::mem::take(&mut self.chess.capture.samples.queue) {
            let Some(doc) = self.session.get(fix.doc) else { continue };
            // Copying not allowed: never (the area could not have been read either).
            if !doc.allows_copying() {
                continue;
            }
            if doc.info.encrypted {
                let s = &mut self.chess.capture.samples;
                match s.consent.get(&fix.doc) {
                    Some(true) => {}
                    Some(false) => continue,
                    None => {
                        if s.held.len() < MAX_WAITING {
                            s.held.push(fix);
                        }
                        continue;
                    }
                }
            }
            self.save_chess_sample(&fix);
        }
    }

    /// Save one corrected character, unless the session's cap is reached or there is no folder.
    fn save_chess_sample(&mut self, fix: &GlyphFix) {
        let s = &mut self.chess.capture.samples;
        let Some(folder) = s.folder.clone().filter(|_| s.on) else { return };
        if s.written >= MAX_SAMPLES {
            if !std::mem::replace(&mut s.capped, true) {
                let n = MAX_SAMPLES.to_string();
                self.notify(crate::i18n::fmt(tl!("Collecting stopped: {n} character samples were saved this session."), &[("n", &n)]));
            }
            return;
        }
        let png = match self.session.chess_glyph_sample(fix.doc, fix.page, fix.rect, fix.dpi) {
            Ok(png) => png,
            Err(e) => {
                self.notify(crate::i18n::fmt(tl!("Couldn't save a character sample: {e}"), &[("e", &e.to_string())]));
                return;
            }
        };
        let hash = match self.chess.capture.samples.hashes.get(&fix.doc) {
            Some(h) => h.clone(),
            None => {
                let h = self.session.get(fix.doc).map(|d| samples::doc_hash(&d.bytes)).unwrap_or_default();
                self.chess.capture.samples.hashes.insert(fix.doc, h.clone());
                h
            }
        };
        let base = samples::base_name(fix.confidence, &hash, fix.page);
        match samples::write_sample(&folder, &samples::class_folder(&fix.text), &base, &png, write_file) {
            Ok(_) => self.chess.capture.samples.written = self.chess.capture.samples.written.saturating_add(1),
            Err(e) => self.notify(crate::i18n::fmt(tl!("Couldn't save a character sample: {e}"), &[("e", &e.to_string())])),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_collecting_takes_corrections_and_a_declined_document_gives_none() {
        let fix = |doc| GlyphFix { doc: DocId(doc), page: 0, rect: [0.0, 0.0, 5.0, 8.0], dpi: 300.0, text: "3".into(), confidence: 0.5 };
        let mut s = Samples::default();
        s.collect(vec![fix(1)]);
        assert!(s.queue.is_empty(), "off: nothing");
        s.on = true;
        s.collect(vec![fix(1)]);
        assert!(s.queue.is_empty(), "no folder: nothing");
        s.folder = Some(PathBuf::from("samples"));
        s.collect(vec![fix(1); MAX_WAITING + 5]);
        assert_eq!(s.queue.len(), MAX_WAITING, "capped");
        s.queue.clear();
        s.held = vec![fix(1), fix(2), fix(1)];
        assert_eq!(s.asking(), Some(DocId(1)));
        s.answer(DocId(1), false);
        assert!(s.queue.is_empty() && s.held == vec![fix(2)]);
        assert_eq!(s.asking(), Some(DocId(2)));
        s.answer(DocId(2), true);
        assert_eq!(s.queue, vec![fix(2)]);
        assert_eq!(s.asking(), None);
    }
}
