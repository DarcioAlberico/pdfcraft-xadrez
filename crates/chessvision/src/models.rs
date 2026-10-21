//! Loading the user's chess models from disk or from bytes. No weights ship with PdfCraft.
//!
//! Two classifiers, each an ONNX file with a JSON description beside it:
//!
//! - `chess-pieces.onnx` / `.json`: one board square (64×64 gray, 0..1) → 13 classes in the
//!   order of [`pdfcraft_chess::recognize::CLASSES`].
//! - `chess-glyphs.onnx` / `.json`: one character cell (32×32 gray, dark ink on light paper) →
//!   figurines, Informator symbols, digits, letters and ligatures.
//!
//! The weights stay on the user's machine (blueprint decision D2). On the desktop they are found
//! in the folder chosen in the app (Chess models…), then `$PDFCRAFT_MODELS`, then beside the
//! executable ([`ChessModels::locate`]); in the browser there is no file system, so the user
//! picks the files and the page passes their bytes to [`ChessModels::from_files`] (each at most
//! [`MAX_PICKED_FILE_BYTES`]). Without a model, detection still works and reading reports
//! [`ModelState::Missing`].
//!
//! The description and the model are both checked when they are loaded, against each other and
//! against what this crate feeds them, so a mismatched pair fails with a message naming what
//! differs instead of reading the wrong thing.

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

use rten::Model;
use rten_tensor::prelude::*;
use rten_tensor::{NdTensor, NdTensorView};
use serde::Deserialize;

use pdfcraft_chess::recognize::CLASS_COUNT;

/// File names, without extension.
pub const PIECES: &str = "chess-pieces";
pub const GLYPHS: &str = "chess-glyphs";

/// Side of a square image fed to the piece model.
pub const PIECE_SIDE: usize = 64;
/// Side of a character image fed to the glyph model.
pub const GLYPH_SIDE: usize = 32;

/// Largest description file accepted.
const MAX_META_BYTES: usize = 1 << 20;
/// Largest model file accepted.
const MAX_MODEL_BYTES: usize = 256 << 20;
/// Largest model file accepted when the files are picked one by one and read into memory (the
/// web app): the real models are about 3 MB (pieces) and 9 MB (glyphs).
pub const MAX_PICKED_FILE_BYTES: usize = 64 << 20;
/// Most names of picked files that are not model files kept for the report.
const MAX_IGNORED_NAMES: usize = 32;
/// Most glyph classes accepted.
const MAX_GLYPH_CLASSES: usize = 4096;
/// Most images classified in one call; larger batches are split by the caller.
pub const MAX_BATCH: usize = 1024;

/// The piece classes, by name, in output order.
const PIECE_CLASS_NAMES: [&str; CLASS_COUNT] = ["empty", "P", "N", "B", "R", "Q", "K", "p", "n", "b", "r", "q", "k"];

/// Why a model could not be loaded or run.
#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("cannot read {path}: {source}")]
    Read { path: PathBuf, source: std::io::Error },
    #[error("{what} is {size} bytes, more than the {limit} accepted")]
    TooLarge { what: &'static str, size: usize, limit: usize },
    #[error("the model description is not valid: {0}")]
    Meta(String),
    #[error("the description says {found:?}, but this file must describe a {expected} model")]
    WrongKind { expected: &'static str, found: String },
    #[error("the model's output classes do not match: {0}")]
    Classes(String),
    #[error("the model file could not be loaded: {0}")]
    Load(String),
    #[error("the model has no input or output named {0:?}")]
    NoSuchNode(String),
    #[error("the model's input is {found:?}, expected [batch, 1, {side}, {side}]")]
    InputShape { found: String, side: usize },
    #[error("expected {expected} values for {count} images, got {got}")]
    BatchSize { expected: usize, count: usize, got: usize },
    #[error("the model failed to run: {0}")]
    Run(String),
    #[error("the model returned {found:?}, expected [{count}, {classes}]")]
    OutputShape { found: Vec<usize>, count: usize, classes: usize },
    #[error("the model returned a value that is not a finite number")]
    NotFinite,
    #[error("{0} is missing: choose both files of a model together")]
    NotChosen(String),
    #[error("{name} could not be read: {why}")]
    Picked { name: String, why: String },
}

/// Whether a model is available.
#[derive(Debug)]
pub enum ModelState<T> {
    Loaded(T),
    /// No file was found: reading is unavailable, detection still works.
    Missing,
    /// A file was found but could not be used; the message says why.
    Invalid(String),
}

impl<T> ModelState<T> {
    pub fn loaded(&self) -> Option<&T> {
        match self {
            ModelState::Loaded(m) => Some(m),
            _ => None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct MetaFile {
    kind: String,
    #[serde(default)]
    arch_version: String,
    input: MetaInput,
    output: MetaOutput,
    #[serde(default = "one")]
    temperature: f32,
}

fn one() -> f32 {
    1.0
}

#[derive(Debug, Deserialize)]
struct MetaInput {
    name: String,
}

#[derive(Debug, Deserialize)]
struct MetaOutput {
    name: String,
    classes: Vec<String>,
}

fn parse_meta(bytes: &[u8], expected_kind: &'static str) -> Result<MetaFile, ModelError> {
    if bytes.len() > MAX_META_BYTES {
        return Err(ModelError::TooLarge { what: "the model description", size: bytes.len(), limit: MAX_META_BYTES });
    }
    let meta: MetaFile = serde_json::from_slice(bytes).map_err(|e| ModelError::Meta(e.to_string()))?;
    if meta.kind != expected_kind {
        return Err(ModelError::WrongKind { expected: expected_kind, found: meta.kind });
    }
    if !(meta.temperature.is_finite() && meta.temperature > 0.0) {
        return Err(ModelError::Meta(format!("temperature {} is not a positive number", meta.temperature)));
    }
    Ok(meta)
}

/// A loaded network with its input and output resolved and checked.
struct Network {
    model: Model,
    input: rten::NodeId,
    output: rten::NodeId,
    classes: usize,
    side: usize,
}

impl Network {
    fn load(bytes: Vec<u8>, meta: &MetaFile, side: usize) -> Result<Network, ModelError> {
        if bytes.len() > MAX_MODEL_BYTES {
            return Err(ModelError::TooLarge { what: "the model file", size: bytes.len(), limit: MAX_MODEL_BYTES });
        }
        let model = Model::load(bytes).map_err(|e| ModelError::Load(e.to_string()))?;
        let input = model.find_node(&meta.input.name).ok_or_else(|| ModelError::NoSuchNode(meta.input.name.clone()))?;
        let output = model.find_node(&meta.output.name).ok_or_else(|| ModelError::NoSuchNode(meta.output.name.clone()))?;
        // Check the declared input shape: [batch, 1, side, side], the batch free.
        if let Some(index) = model.input_ids().iter().position(|id| *id == input)
            && let Some(shape) = model.input_shape(index)
        {
            let fixed: Vec<Option<usize>> = shape
                .iter()
                .map(|d| match d {
                    rten::Dimension::Fixed(n) => Some(*n),
                    rten::Dimension::Symbolic(_) => None,
                })
                .collect();
            let ok = matches!(fixed.as_slice(), [_, Some(1), Some(h), Some(w)] if *h == side && *w == side);
            if !ok {
                return Err(ModelError::InputShape { found: format!("{shape:?}"), side });
            }
        }
        Ok(Network { model, input, output, classes: meta.output.classes.len(), side })
    }

    /// Logits for `count` images of `side × side` values each, row-major.
    fn logits(&self, images: &[f32], count: usize) -> Result<Vec<f32>, ModelError> {
        let per = self.side * self.side;
        let expected = count.saturating_mul(per);
        if count == 0 || count > MAX_BATCH || images.len() != expected {
            return Err(ModelError::BatchSize { expected, count, got: images.len() });
        }
        let input = NdTensor::from_data([count, 1, self.side, self.side], images.to_vec());
        let mut outputs = self.model.run(vec![(self.input, input.into())], &[self.output], None).map_err(|e| ModelError::Run(e.to_string()))?;
        let value = outputs.pop().ok_or_else(|| ModelError::Run("no output".to_owned()))?;
        let tensor: NdTensor<f32, 2> = value.try_into().map_err(|e: rten::TryFromValueError| ModelError::Run(e.to_string()))?;
        let view: NdTensorView<f32, 2> = tensor.view();
        let shape = view.shape();
        if shape != [count, self.classes] {
            return Err(ModelError::OutputShape { found: shape.to_vec(), count, classes: self.classes });
        }
        let values: Vec<f32> = view.iter().copied().collect();
        if values.iter().any(|v| !v.is_finite()) {
            return Err(ModelError::NotFinite);
        }
        Ok(values)
    }
}

/// Softmax of each row of `classes` logits, after dividing by `temperature`.
fn softmax_rows(logits: &[f32], classes: usize, temperature: f32) -> Vec<f32> {
    let mut out = Vec::with_capacity(logits.len());
    for row in logits.chunks(classes.max(1)) {
        let max = row.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let exps: Vec<f32> = row.iter().map(|v| ((v - max) / temperature).exp()).collect();
        let sum: f32 = exps.iter().sum();
        out.extend(exps.iter().map(|e| if sum > 0.0 { e / sum } else { 0.0 }));
    }
    out
}

/// The square classifier.
pub struct PieceModel {
    net: Network,
    /// The architecture the description names, for display.
    pub arch_version: String,
}

impl fmt::Debug for PieceModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PieceModel").field("arch_version", &self.arch_version).finish()
    }
}

impl PieceModel {
    /// Load from the ONNX file's bytes and its description.
    pub fn from_bytes(model: Vec<u8>, meta: &[u8]) -> Result<PieceModel, ModelError> {
        let meta = parse_meta(meta, PIECES)?;
        if meta.output.classes != PIECE_CLASS_NAMES {
            return Err(ModelError::Classes(format!("expected {PIECE_CLASS_NAMES:?}, found {:?}", meta.output.classes)));
        }
        let net = Network::load(model, &meta, PIECE_SIDE)?;
        Ok(PieceModel { net, arch_version: meta.arch_version })
    }

    /// Class probabilities for `count` squares, each `64 × 64` gray values in 0..1, row-major.
    /// Returns `count × 13` values. The calibration temperature is not applied: measured, it
    /// doubled the review queue without fixing a square.
    pub fn probabilities(&self, squares: &[f32], count: usize) -> Result<Vec<f32>, ModelError> {
        let logits = self.net.logits(squares, count)?;
        Ok(softmax_rows(&logits, CLASS_COUNT, 1.0))
    }
}

/// The chess glyph classifier.
pub struct GlyphModel {
    net: Network,
    /// The character each output stands for, in order.
    pub classes: Vec<String>,
    /// The calibration temperature, applied to the logits.
    pub temperature: f32,
}

impl fmt::Debug for GlyphModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GlyphModel").field("classes", &self.classes.len()).field("temperature", &self.temperature).finish()
    }
}

impl GlyphModel {
    /// Load from the ONNX file's bytes and its description.
    pub fn from_bytes(model: Vec<u8>, meta: &[u8]) -> Result<GlyphModel, ModelError> {
        let meta = parse_meta(meta, GLYPHS)?;
        let classes = meta.output.classes.clone();
        if classes.is_empty() || classes.len() > MAX_GLYPH_CLASSES || classes.iter().any(String::is_empty) {
            return Err(ModelError::Classes(format!("{} classes, or an empty one", classes.len())));
        }
        let net = Network::load(model, &meta, GLYPH_SIDE)?;
        Ok(GlyphModel { net, classes, temperature: meta.temperature })
    }

    /// Calibrated class probabilities for `count` cells, each `32 × 32` gray values in 0..1
    /// (dark ink on light paper), row-major. Returns `count × classes.len()` values.
    pub fn probabilities(&self, cells: &[f32], count: usize) -> Result<Vec<f32>, ModelError> {
        let logits = self.net.logits(cells, count)?;
        Ok(softmax_rows(&logits, self.classes.len(), self.temperature))
    }
}

/// The models found on this machine.
#[derive(Debug)]
pub struct ChessModels {
    pub pieces: ModelState<PieceModel>,
    pub glyphs: ModelState<GlyphModel>,
    /// The folder they were looked for in, when one was found.
    pub folder: Option<PathBuf>,
}

impl ChessModels {
    /// No model at all.
    pub fn none() -> ChessModels {
        ChessModels { pieces: ModelState::Missing, glyphs: ModelState::Missing, folder: None }
    }

    /// Look for the models in `$PDFCRAFT_MODELS`, then beside the executable. In the browser
    /// there is no such folder and both are missing.
    pub fn find() -> ChessModels {
        match ChessModels::locate(None) {
            Some(dir) => ChessModels::load_from(&dir),
            None => ChessModels::none(),
        }
    }

    /// The folder the models are loaded from: `chosen` (the folder picked in the app) when it
    /// holds a model, else the first of `$PDFCRAFT_MODELS`, `models` beside the executable and
    /// the executable's own folder that does. A chosen folder that is gone or holds no model
    /// falls back to the search; the caller can tell by comparing. `None` when no folder holds
    /// one (always in the browser).
    pub fn locate(chosen: Option<&Path>) -> Option<PathBuf> {
        let exe = std::env::current_exe().ok();
        locate_in(chosen, std::env::var_os("PDFCRAFT_MODELS"), exe.as_deref())
    }

    /// Load whatever models `dir` holds.
    pub fn load_from(dir: &Path) -> ChessModels {
        ChessModels {
            pieces: load_pair(dir, PIECES, PieceModel::from_bytes),
            glyphs: load_pair(dir, GLYPHS, GlyphModel::from_bytes),
            folder: Some(dir.to_path_buf()),
        }
    }

    /// The models in files the user picked (the web app, where there is no folder to look in):
    /// each file's name and its bytes, or why it could not be read. Files are known by name
    /// (`chess-pieces.onnx`, `chess-pieces.json`, `chess-glyphs.onnx`, `chess-glyphs.json`, in
    /// any case, any folder part ignored); a model with only one of its two files is invalid and
    /// says which is missing, and a file over [`MAX_PICKED_FILE_BYTES`] is not loaded. Returns
    /// the models (no folder) and the names of the files that are not model files (the first 32).
    pub fn from_files(files: Vec<(String, Result<Vec<u8>, String>)>) -> (ChessModels, Vec<String>) {
        type Slot = Option<Result<Vec<u8>, ModelError>>;
        let (mut pieces_onnx, mut pieces_meta, mut glyphs_onnx, mut glyphs_meta): (Slot, Slot, Slot, Slot) = (None, None, None, None);
        let mut ignored = Vec::new();
        for (name, bytes) in files {
            let base = name.rsplit(['/', '\\']).next().unwrap_or(&name).to_ascii_lowercase();
            let slot = match base.as_str() {
                "chess-pieces.onnx" => &mut pieces_onnx,
                "chess-pieces.json" => &mut pieces_meta,
                "chess-glyphs.onnx" => &mut glyphs_onnx,
                "chess-glyphs.json" => &mut glyphs_meta,
                _ => {
                    if ignored.len() < MAX_IGNORED_NAMES {
                        ignored.push(name);
                    }
                    continue;
                }
            };
            *slot = Some(match bytes {
                Ok(b) if b.len() > MAX_PICKED_FILE_BYTES => {
                    Err(ModelError::TooLarge { what: "a model file", size: b.len(), limit: MAX_PICKED_FILE_BYTES })
                }
                Ok(b) => Ok(b),
                Err(why) => Err(ModelError::Picked { name: base, why }),
            });
        }
        let models = ChessModels {
            pieces: picked_pair(PIECES, pieces_onnx, pieces_meta, PieceModel::from_bytes),
            glyphs: picked_pair(GLYPHS, glyphs_onnx, glyphs_meta, GlyphModel::from_bytes),
            folder: None,
        };
        (models, ignored)
    }
}

/// Whether `dir` holds a chess model (`chess-pieces.onnx` or `chess-glyphs.onnx`).
pub fn holds_models(dir: &Path) -> bool {
    dir.join(format!("{PIECES}.onnx")).is_file() || dir.join(format!("{GLYPHS}.onnx")).is_file()
}

/// The folders looked in, in order: the one chosen in the app, `$PDFCRAFT_MODELS` (`env`), then
/// `models` beside the executable `exe` and the executable's own folder.
fn search_order(chosen: Option<&Path>, env: Option<OsString>, exe: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = chosen.map(Path::to_path_buf).into_iter().collect();
    if let Some(dir) = env.filter(|d| !d.is_empty()) {
        dirs.push(PathBuf::from(dir));
    }
    if let Some(parent) = exe.and_then(Path::parent) {
        dirs.push(parent.join("models"));
        dirs.push(parent.to_path_buf());
    }
    dirs
}

/// [`ChessModels::locate`] with `$PDFCRAFT_MODELS` and the executable given.
fn locate_in(chosen: Option<&Path>, env: Option<OsString>, exe: Option<&Path>) -> Option<PathBuf> {
    search_order(chosen, env, exe).into_iter().find(|d| holds_models(d))
}

/// One model from picked files: missing when neither file was picked.
fn picked_pair<T>(
    name: &str,
    onnx: Option<Result<Vec<u8>, ModelError>>,
    meta: Option<Result<Vec<u8>, ModelError>>,
    load: impl Fn(Vec<u8>, &[u8]) -> Result<T, ModelError>,
) -> ModelState<T> {
    let result = match (onnx, meta) {
        (None, None) => return ModelState::Missing,
        (Some(_), None) => Err(ModelError::NotChosen(format!("{name}.json"))),
        (None, Some(_)) => Err(ModelError::NotChosen(format!("{name}.onnx"))),
        (Some(onnx), Some(meta)) => onnx.and_then(|model| load(model, &meta?)),
    };
    match result {
        Ok(m) => ModelState::Loaded(m),
        Err(e) => ModelState::Invalid(e.to_string()),
    }
}

fn load_pair<T>(dir: &Path, name: &str, load: impl Fn(Vec<u8>, &[u8]) -> Result<T, ModelError>) -> ModelState<T> {
    let onnx = dir.join(format!("{name}.onnx"));
    if !onnx.is_file() {
        return ModelState::Missing;
    }
    let read = |path: &Path, limit: usize| -> Result<Vec<u8>, ModelError> {
        let size = std::fs::metadata(path).map_err(|source| ModelError::Read { path: path.to_path_buf(), source })?.len();
        let size = usize::try_from(size).unwrap_or(usize::MAX);
        if size > limit {
            return Err(ModelError::TooLarge { what: "a model file", size, limit });
        }
        std::fs::read(path).map_err(|source| ModelError::Read { path: path.to_path_buf(), source })
    };
    let result = read(&onnx, MAX_MODEL_BYTES)
        .and_then(|model| Ok((model, read(&dir.join(format!("{name}.json")), MAX_META_BYTES)?)))
        .and_then(|(model, meta)| load(model, &meta));
    match result {
        Ok(m) => ModelState::Loaded(m),
        Err(e) => ModelState::Invalid(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PIECE_META: &str = r#"{"kind":"chess-pieces","input":{"name":"cells"},"output":{"name":"logits","classes":["empty","P","N","B","R","Q","K","p","n","b","r","q","k"]},"temperature":1.4}"#;

    #[test]
    fn descriptions_are_checked() {
        assert!(matches!(parse_meta(b"not json", PIECES), Err(ModelError::Meta(_))));
        assert!(matches!(parse_meta(PIECE_META.as_bytes(), GLYPHS), Err(ModelError::WrongKind { .. })));
        let cold = PIECE_META.replace("1.4", "0");
        assert!(matches!(parse_meta(cold.as_bytes(), PIECES), Err(ModelError::Meta(_))));
        assert!(matches!(parse_meta(&vec![b' '; MAX_META_BYTES + 1], PIECES), Err(ModelError::TooLarge { .. })));
        assert!(parse_meta(PIECE_META.as_bytes(), PIECES).is_ok());
    }

    #[test]
    fn piece_classes_must_match_the_recognizer() {
        let swapped = PIECE_META.replace(r#""K","p""#, r#""p","K""#);
        let err = PieceModel::from_bytes(Vec::new(), swapped.as_bytes()).unwrap_err();
        assert!(matches!(err, ModelError::Classes(_)), "{err}");
    }

    #[test]
    fn garbage_models_are_errors_not_panics() {
        let err = PieceModel::from_bytes(b"definitely not a model".to_vec(), PIECE_META.as_bytes()).unwrap_err();
        assert!(matches!(err, ModelError::Load(_)), "{err}");
        let glyph_meta = r#"{"kind":"chess-glyphs","input":{"name":"glyphs"},"output":{"name":"logits","classes":["♘",""]}}"#;
        assert!(matches!(GlyphModel::from_bytes(Vec::new(), glyph_meta.as_bytes()), Err(ModelError::Classes(_))));
    }

    #[test]
    fn softmax_rows_sum_to_one_and_respect_temperature() {
        let p = softmax_rows(&[0.0, 1.0, 2.0, 1000.0, 0.0, 0.0], 3, 1.0);
        assert!((p[0] + p[1] + p[2] - 1.0).abs() < 1e-6);
        assert!(p[2] > p[1] && p[1] > p[0]);
        assert!((p[3] - 1.0).abs() < 1e-6, "large logits do not overflow");
        let hot = softmax_rows(&[0.0, 2.0], 2, 4.0);
        let cold = softmax_rows(&[0.0, 2.0], 2, 1.0);
        assert!(hot[1] < cold[1]);
    }

    #[test]
    fn an_empty_folder_has_no_models() {
        let dir = std::env::temp_dir().join("pdfcraft-chessvision-no-models");
        let _ = std::fs::create_dir_all(&dir);
        let found = ChessModels::load_from(&dir);
        assert!(matches!(found.pieces, ModelState::Missing));
        assert!(matches!(found.glyphs, ModelState::Missing));
    }

    /// A fresh, empty folder for one test.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pdfcraft-chessvision-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// U4: the folder chosen in the app comes first, then `$PDFCRAFT_MODELS`, then `models`
    /// beside the executable, then the executable's folder; a folder is used only when it holds a
    /// model, so a chosen folder that is gone or empty falls back to the search.
    #[test]
    fn the_chosen_folder_comes_before_the_environment_and_the_executable() {
        let root = scratch("precedence");
        let [chosen, env, exe_dir] = ["chosen", "env", "bin"].map(|d| root.join(d));
        let exe = exe_dir.join("pdfcraft.exe");
        for dir in [&chosen, &env, &exe_dir.join("models"), &exe_dir] {
            std::fs::create_dir_all(dir).unwrap();
            std::fs::write(dir.join(format!("{GLYPHS}.onnx")), b"not a model").unwrap();
        }
        let env_var = || Some(OsString::from(&env));
        assert_eq!(locate_in(Some(&chosen), env_var(), Some(&exe)), Some(chosen.clone()));
        assert_eq!(locate_in(None, env_var(), Some(&exe)), Some(env.clone()));
        assert_eq!(locate_in(None, None, Some(&exe)), Some(exe_dir.join("models")));
        std::fs::remove_dir_all(exe_dir.join("models")).unwrap();
        assert_eq!(locate_in(None, Some(OsString::new()), Some(&exe)), Some(exe_dir.clone()), "an empty PDFCRAFT_MODELS is no folder");
        // A chosen folder without a model, or gone, gives way to the search.
        let empty = root.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert_eq!(locate_in(Some(&empty), env_var(), Some(&exe)), Some(env.clone()));
        assert_eq!(locate_in(Some(&root.join("gone")), env_var(), Some(&exe)), Some(env.clone()));
        assert_eq!(locate_in(Some(&empty), None, None), None);
        // A pieces model alone counts too, and the folder's models are loaded and judged.
        std::fs::write(empty.join(format!("{PIECES}.onnx")), b"not a model").unwrap();
        assert_eq!(locate_in(Some(&empty), env_var(), Some(&exe)), Some(empty.clone()));
        let found = ChessModels::load_from(&empty);
        assert!(matches!(&found.pieces, ModelState::Invalid(why) if why.contains("chess-pieces.json")), "{:?}", found.pieces);
        assert!(matches!(found.glyphs, ModelState::Missing));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// U4, the web: models from picked files, known by name; one file of a pair alone, a file
    /// too large or unreadable, and a broken model each say why, and other files are listed.
    #[test]
    fn picked_files_become_models_or_say_why_not() {
        let meta = |m: &str| Ok(m.as_bytes().to_vec());
        let glyph_meta = r#"{"kind":"chess-glyphs","input":{"name":"glyphs"},"output":{"name":"logits","classes":["a"]}}"#;
        let (models, ignored) = ChessModels::from_files(vec![
            ("C:\\fakepath\\Chess-Pieces.ONNX".into(), Ok(b"garbage".to_vec())),
            ("chess-pieces.json".into(), meta(PIECE_META)),
            ("models/chess-glyphs.json".into(), meta(glyph_meta)),
            ("notes.txt".into(), Ok(Vec::new())),
        ]);
        assert!(matches!(&models.pieces, ModelState::Invalid(why) if why.contains("could not be loaded")), "{:?}", models.pieces);
        assert!(matches!(&models.glyphs, ModelState::Invalid(why) if why.contains("chess-glyphs.onnx is missing")), "{:?}", models.glyphs);
        assert_eq!(ignored, ["notes.txt"]);
        assert!(models.folder.is_none());

        let (models, _) = ChessModels::from_files(vec![
            ("chess-glyphs.onnx".into(), Ok(vec![0; MAX_PICKED_FILE_BYTES + 1])),
            ("chess-glyphs.json".into(), meta(glyph_meta)),
        ]);
        assert!(matches!(&models.glyphs, ModelState::Invalid(why) if why.contains("more than")), "{:?}", models.glyphs);
        assert!(matches!(models.pieces, ModelState::Missing));
        let (models, _) = ChessModels::from_files(vec![
            ("chess-pieces.onnx".into(), Err("the browser refused".into())),
            ("chess-pieces.json".into(), meta(PIECE_META)),
        ]);
        assert!(matches!(&models.pieces, ModelState::Invalid(why) if why.contains("the browser refused")), "{:?}", models.pieces);
        let (models, ignored) = ChessModels::from_files(Vec::new());
        assert!(matches!((models.pieces, models.glyphs), (ModelState::Missing, ModelState::Missing)) && ignored.is_empty());
    }

    /// Runs the user's real exported models when `PDFCRAFT_CHESS_TEST_MODELS` points at their
    /// folder. Skipped otherwise: no weights are committed (decision D2).
    #[test]
    fn the_users_models_load_and_run_when_present() {
        let Some(dir) = std::env::var_os("PDFCRAFT_CHESS_TEST_MODELS") else {
            return;
        };
        let models = ChessModels::load_from(Path::new(&dir));
        let pieces = match &models.pieces {
            ModelState::Loaded(m) => m,
            other => panic!("pieces: {other:?}"),
        };
        // A white square: the model must call it empty, with probabilities summing to 1.
        let white = vec![1.0f32; PIECE_SIDE * PIECE_SIDE * 2];
        let p = pieces.probabilities(&white, 2).unwrap();
        assert_eq!(p.len(), 2 * CLASS_COUNT);
        assert!((p[..CLASS_COUNT].iter().sum::<f32>() - 1.0).abs() < 1e-4);
        assert!(pieces.probabilities(&white, 3).is_err(), "wrong batch size");
        let glyphs = match &models.glyphs {
            ModelState::Loaded(m) => m,
            other => panic!("glyphs: {other:?}"),
        };
        let blank = vec![1.0f32; GLYPH_SIDE * GLYPH_SIDE];
        assert_eq!(glyphs.probabilities(&blank, 1).unwrap().len(), glyphs.classes.len());

        // Parity with torch: the references were written by the export script for the input
        // ((i * 7919) % 256) / 255, three images each.
        let pattern = |n: usize, side: usize| -> Vec<f32> { (0..n * side * side).map(|i| ((i * 7919) % 256) as f32 / 255.0).collect() };
        let reference = |name: &str| -> Vec<f32> {
            let text = std::fs::read_to_string(Path::new(&dir).join(format!("{name}.parity.json"))).unwrap();
            let v: serde_json::Value = serde_json::from_str(&text).unwrap();
            v["probabilities"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap() as f32).collect()
        };
        let worst = |got: &[f32], want: &[f32]| {
            assert_eq!(got.len(), want.len());
            got.iter().zip(want).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max)
        };
        let got = pieces.probabilities(&pattern(3, PIECE_SIDE), 3).unwrap();
        assert!(worst(&got, &reference(PIECES)) <= 1e-4, "pieces differ from torch by {}", worst(&got, &reference(PIECES)));
        let got = glyphs.probabilities(&pattern(3, GLYPH_SIDE), 3).unwrap();
        assert!(worst(&got, &reference(GLYPHS)) <= 1e-4, "glyphs differ from torch by {}", worst(&got, &reference(GLYPHS)));
    }
}
