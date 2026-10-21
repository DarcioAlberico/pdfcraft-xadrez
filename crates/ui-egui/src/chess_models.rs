//! Chess ▸ Chess models… (wave 10, U4): where the user's chess models come from. None ship with
//! PdfCraft (blueprint decision D2): the piece model reads drawn and scanned diagrams, the glyph
//! model chess text (chess OCR).
//!
//! - **Desktop.** The entry (the `chess.models` command: the Diagrams tab's button, Preferences ▸
//!   Chess) picks a folder. It is checked at once (each model loaded, missing, or invalid with the
//!   reason) and remembered in the settings (`chess_models`). Models are looked for in that
//!   folder, then `$PDFCRAFT_MODELS`, then beside the executable
//!   ([`pdfcraft_engine::chess::ChessModels::locate`]): a folder that is gone or holds no model
//!   gives way to the search, and Preferences says so. The setting is untrusted: an empty, long,
//!   relative path, or one with a NUL, is ignored.
//! - **Web.** There is no folder: the entry picks the model files (several at once), each read
//!   into memory up to [`MAX_PICKED_FILE_BYTES`], and the models built from them are used for the
//!   rest of the session ([`pdfcraft_engine::chess::install_models`]). Nothing is kept: the next
//!   visit picks them again.
//!
//! Choosing again reloads the models (the folder's files may have changed). A scan or a reading
//! already running keeps the models it started with: each holds its own `Arc`.

use std::path::{Path, PathBuf};

use egui::RichText;
use pdfcraft_engine::chess::{ChessModels, ModelState};

pub use pdfcraft_engine::chess::pdfcraft_chessvision::models::MAX_PICKED_FILE_BYTES;

use crate::PdfCraftApp;
use crate::theme::{self, Tokens};

/// Longest folder kept from the settings, in bytes.
const MAX_FOLDER_LEN: usize = 4096;

/// What one model's check found.
#[derive(Clone, Debug, PartialEq)]
pub enum ModelStatus {
    Loaded,
    Missing,
    Invalid(String),
}

impl ModelStatus {
    fn of<T>(state: &ModelState<T>) -> ModelStatus {
        match state {
            ModelState::Loaded(_) => ModelStatus::Loaded,
            ModelState::Missing => ModelStatus::Missing,
            ModelState::Invalid(why) => ModelStatus::Invalid(why.clone()),
        }
    }

    /// The status in words, in the interface language.
    fn text(&self) -> String {
        match self {
            ModelStatus::Loaded => tl!("loaded").to_owned(),
            ModelStatus::Missing => tl!("not found").to_owned(),
            ModelStatus::Invalid(why) => crate::i18n::fmt(tl!("couldn't be loaded: {e}"), &[("e", why)]),
        }
    }
}

/// What the last check of the models found.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelsReport {
    /// The folder the models were loaded from (`None`: from picked files, or none found).
    pub folder: Option<PathBuf>,
    /// The folder chosen holds no model (or is gone), so the search was used.
    pub fell_back: bool,
    pub pieces: ModelStatus,
    pub glyphs: ModelStatus,
    /// Files picked that are not model files (the web).
    pub ignored: Vec<String>,
}

impl ModelsReport {
    fn of(models: &ChessModels, chosen: Option<&Path>, ignored: Vec<String>) -> ModelsReport {
        ModelsReport {
            folder: models.folder.clone(),
            fell_back: chosen.is_some() && models.folder.as_deref() != chosen,
            pieces: ModelStatus::of(&models.pieces),
            glyphs: ModelStatus::of(&models.glyphs),
            ignored,
        }
    }

    /// One line for a notice: both models' state.
    fn summary(&self) -> String {
        crate::i18n::fmt(
            tl!("Chess models: piece model {pieces}; glyph model {glyphs}."),
            &[("pieces", &self.pieces.text()), ("glyphs", &self.glyphs.text())],
        )
    }
}

/// Files the browser has read, waiting for the frame: each name with its bytes or why not.
#[cfg(target_arch = "wasm32")]
type Inbox = std::sync::Arc<std::sync::Mutex<Option<Vec<(String, Result<Vec<u8>, String>)>>>>;

/// The models preference and the last check.
#[derive(Debug, Default)]
pub struct ModelsPref {
    /// The folder chosen in the app (the desktop; a preference).
    pub folder: Option<PathBuf>,
    /// What the last check found, shown in Preferences.
    pub report: Option<ModelsReport>,
    #[cfg(target_arch = "wasm32")]
    inbox: Inbox,
    /// The browser's file picker is showing or its files are being read.
    #[cfg(target_arch = "wasm32")]
    busy: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl ModelsPref {
    /// The folder to read the models from: the one chosen when it holds a model, else
    /// `$PDFCRAFT_MODELS` or beside the executable; `None` when none does (and always on the
    /// web, where the models picked as files are used).
    pub fn models_dir(&self) -> Option<PathBuf> {
        if cfg!(target_arch = "wasm32") {
            return None;
        }
        ChessModels::locate(self.folder.as_deref())
    }

    /// The preference, for the settings file.
    pub(crate) fn to_json(&self) -> serde_json::Value {
        serde_json::json!({ "folder": self.folder.as_ref().map(|f| f.to_string_lossy().into_owned()) })
    }

    /// The preference from the settings file. It is untrusted: a folder that is not a string, is
    /// empty, longer than [`MAX_FOLDER_LEN`], has a NUL or is not absolute is ignored. A folder
    /// that is gone is kept (a drive may be out): the models are then looked for as when none
    /// was chosen, and Preferences says so.
    pub(crate) fn restore(&mut self, v: &serde_json::Value) {
        if cfg!(target_arch = "wasm32") {
            return;
        }
        if let Some(folder) = v["folder"].as_str().and_then(valid_folder) {
            self.folder = Some(folder);
        }
    }
}

/// `folder` as a models folder setting, when it can be one.
fn valid_folder(folder: &str) -> Option<PathBuf> {
    if folder.is_empty() || folder.len() > MAX_FOLDER_LEN || folder.contains('\0') {
        return None;
    }
    let path = PathBuf::from(folder);
    path.is_absolute().then_some(path)
}

// ─── The interface ───────────────────────────────────────────────────────────────────────────

/// What a click in the models section asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelsAction {
    /// Pick the folder (desktop) or the files (web).
    Choose,
    /// Look again where the models are (their files may have changed).
    Check,
    /// Forget the folder chosen (desktop) or the models picked (web).
    Forget,
}

/// Preferences ▸ Chess: where the models come from and what was found.
pub(crate) fn section(ui: &mut egui::Ui, t: &Tokens, pref: &ModelsPref) -> Option<ModelsAction> {
    let mut action = None;
    ui.label(RichText::new(tl!("Chess models")).font(theme::semibold(13.0)));
    let muted = |ui: &mut egui::Ui, text: String| {
        ui.label(RichText::new(text).small().color(t.text_muted));
    };
    if cfg!(target_arch = "wasm32") {
        muted(
            ui,
            crate::i18n::fmt(
                tl!(
                    "Choose the model files (chess-pieces.onnx and .json, chess-glyphs.onnx and .json) to use them until this page is closed. Each file may be up to {mb} MB."
                ),
                &[("mb", &(MAX_PICKED_FILE_BYTES >> 20).to_string())],
            ),
        );
    } else {
        let shown = pref
            .folder
            .as_ref()
            .map_or_else(|| tl!("No folder chosen: PDFCRAFT_MODELS, then beside the program").to_owned(), |f| f.display().to_string());
        ui.horizontal_wrapped(|ui| {
            ui.label(tl!("Models folder"));
            ui.label(RichText::new(shown).color(t.text_muted));
        });
        if let Some(folder) = &pref.folder
            && !pdfcraft_engine::chess::pdfcraft_chessvision::models::holds_models(folder)
        {
            muted(
                ui,
                tl!("This folder holds no chess model, or is gone: the models are looked for in PDFCRAFT_MODELS, then beside the program.")
                    .to_owned(),
            );
        }
    }
    if let Some(report) = &pref.report {
        muted(ui, crate::i18n::fmt(tl!("Piece model: {state}"), &[("state", &report.pieces.text())]));
        muted(ui, crate::i18n::fmt(tl!("Glyph model: {state}"), &[("state", &report.glyphs.text())]));
        if let Some(folder) = &report.folder
            && report.fell_back
        {
            muted(ui, crate::i18n::fmt(tl!("Found in {folder} instead."), &[("folder", &folder.display().to_string())]));
        }
        if !report.ignored.is_empty() {
            muted(ui, crate::i18n::fmt(tl!("Not model files, left out: {names}"), &[("names", &report.ignored.join(", "))]));
        }
    }
    ui.horizontal(|ui| {
        if ui.button(tl!("Chess models…")).clicked() {
            action = Some(ModelsAction::Choose);
        }
        if !cfg!(target_arch = "wasm32") && ui.button(tl!("Check again")).clicked() {
            action = Some(ModelsAction::Check);
        }
        let forget = if cfg!(target_arch = "wasm32") { pref.report.is_some() } else { pref.folder.is_some() };
        if forget && ui.button(tl!("Forget")).on_hover_text(tl!("Look for the models as when none were chosen.")).clicked() {
            action = Some(ModelsAction::Forget);
        }
    });
    action
}

impl PdfCraftApp {
    /// The chess scan settings the app scans and reads with: the defaults, with the models folder
    /// chosen (or found).
    pub(crate) fn chess_scan_settings(&self) -> pdfcraft_engine::chess::ChessScanSettings {
        pdfcraft_engine::chess::ChessScanSettings { models_dir: self.chess.models.models_dir(), ..Default::default() }
    }

    /// What a click in the models section asked for.
    pub(crate) fn chess_models_action(&mut self, action: ModelsAction) {
        match action {
            ModelsAction::Choose => self.choose_chess_models(),
            ModelsAction::Check => {
                self.check_chess_models();
                if let Some(report) = &self.chess.models.report {
                    let text = report.summary();
                    self.notify(text);
                }
            }
            ModelsAction::Forget => {
                self.chess.models.folder = None;
                self.chess.models.report = None;
                if cfg!(target_arch = "wasm32") {
                    pdfcraft_engine::chess::install_models(None);
                }
            }
        }
    }

    /// `chess.models`, Chess models…: pick the models folder (desktop) or the model files (web).
    /// The picker answers on a later frame; `pick_override` stands in for it in tests.
    pub fn choose_chess_models(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let dialog = rfd::AsyncFileDialog::new().set_title(tl!("Choose the chess models folder"));
            self.ask_one(crate::pickers::Ask::Folder(dialog), None, |app, path| app.use_chess_models_folder(path));
        }
        #[cfg(target_arch = "wasm32")]
        self.pick_chess_model_files();
    }

    /// Use `folder` for the chess models from now on (and in later sessions), and say what it
    /// holds.
    pub fn use_chess_models_folder(&mut self, folder: PathBuf) {
        let Some(folder) = folder.to_str().and_then(valid_folder) else {
            self.notify(crate::i18n::fmt(tl!("{folder} can't be used as the chess models folder."), &[("folder", &folder.display().to_string())]));
            return;
        };
        self.chess.models.folder = Some(folder);
        self.check_chess_models();
        if let Some(report) = &self.chess.models.report {
            let mut text = report.summary();
            if report.fell_back {
                text = format!(
                    "{text} {}",
                    tl!("This folder holds no chess model, or is gone: the models are looked for in PDFCRAFT_MODELS, then beside the program.")
                );
            }
            self.notify(text);
        }
    }

    /// Load the models again from where they are now, and keep what was found. On the web the
    /// models picked stay as they are.
    pub fn check_chess_models(&mut self) {
        if cfg!(target_arch = "wasm32") {
            return;
        }
        let chosen = self.chess.models.folder.clone();
        let dir = ChessModels::locate(chosen.as_deref());
        let models = pdfcraft_engine::chess::reload_models(dir.as_deref());
        self.chess.models.report = Some(ModelsReport::of(&models, chosen.as_deref(), Vec::new()));
    }

    /// Build the models from files the user picked (each name with its bytes, or why it could not
    /// be read) and use them for the rest of the session wherever no folder is given: the web
    /// app's way to have models. Says what was found.
    pub fn install_chess_model_files(&mut self, files: Vec<(String, Result<Vec<u8>, String>)>) {
        let (models, ignored) = ChessModels::from_files(files);
        let report = ModelsReport::of(&models, None, ignored);
        pdfcraft_engine::chess::install_models(Some(models));
        let text = report.summary();
        self.chess.models.report = Some(report);
        self.notify(text);
    }

    /// The web: pick the model files and read them (each at most [`MAX_PICKED_FILE_BYTES`]; a
    /// larger one is not read) for [`Self::poll_chess_models`].
    #[cfg(target_arch = "wasm32")]
    fn pick_chess_model_files(&mut self) {
        use std::sync::atomic::Ordering;
        if self.chess.models.busy.swap(true, Ordering::SeqCst) {
            self.notify_tr("Another file dialog is still open. Finish with it first.");
            return;
        }
        let inbox = self.chess.models.inbox.clone();
        let busy = self.chess.models.busy.clone();
        let ctx = self.ctx.clone();
        let dialog = rfd::AsyncFileDialog::new().set_title(tl!("Choose the chess model files")).add_filter(tl!("Chess models"), &["onnx", "json"]);
        wasm_bindgen_futures::spawn_local(async move {
            let picked = dialog.pick_files().await.unwrap_or_default();
            let mut files = Vec::new();
            for file in picked {
                let name = file.file_name();
                // Read the Blob directly, so a refused read is an error (rfd's read helper
                // unwraps it), and never one larger than the cap.
                let size = file.inner().size();
                let bytes = if size.is_nan() || size > MAX_PICKED_FILE_BYTES as f64 {
                    Err(format!("{} MB at most", MAX_PICKED_FILE_BYTES >> 20))
                } else {
                    match wasm_bindgen_futures::JsFuture::from(file.inner().array_buffer()).await {
                        Ok(buffer) => {
                            let array = js_sys::Uint8Array::new(&buffer);
                            if array.length() as usize > MAX_PICKED_FILE_BYTES {
                                Err(format!("{} MB at most", MAX_PICKED_FILE_BYTES >> 20))
                            } else {
                                Ok(array.to_vec())
                            }
                        }
                        Err(error) => Err(format!("{error:?}")),
                    }
                };
                files.push((name, bytes));
            }
            if !files.is_empty() {
                *inbox.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(files);
            }
            busy.store(false, Ordering::SeqCst);
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        });
    }

    /// Every frame on the web: install the model files the browser has read.
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn poll_chess_models(&mut self) {
        let files = self.chess.models.inbox.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take();
        if let Some(files) = files {
            self.install_chess_model_files(files);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_absolute_sane_folders_are_taken_from_the_settings() {
        let here = std::env::temp_dir();
        let long = format!("{}{}", here.display(), "x".repeat(MAX_FOLDER_LEN));
        for bad in [
            serde_json::json!("yes"),
            serde_json::json!({ "folder": 3 }),
            serde_json::json!({ "folder": "" }),
            serde_json::json!({ "folder": "models" }),
            serde_json::json!({ "folder": format!("{}\u{0}", here.display()) }),
            serde_json::json!({ "folder": long }),
            serde_json::Value::Null,
        ] {
            let mut pref = ModelsPref::default();
            pref.restore(&bad);
            assert_eq!(pref.folder, None, "{bad}");
        }
        let gone = here.join("pdfcraft-no-such-models-folder");
        let mut pref = ModelsPref::default();
        pref.restore(&serde_json::json!({ "folder": gone }));
        assert_eq!(pref.folder.as_deref(), Some(gone.as_path()), "a folder that is gone is kept, and gives way to the search");
        assert_ne!(pref.models_dir().as_deref(), Some(gone.as_path()));
        let mut back = ModelsPref::default();
        back.restore(&pref.to_json());
        assert_eq!(back.folder, pref.folder);
    }
}
