//! Model folders for the fuzzer (plan A9-Q3a): the user's own (`--models DIR`), one per hostile
//! model built from `onnx_build.rs` (the file the chessvision tests build theirs from), and one per
//! job holding a model with bytes mutated, written again before each run that uses it.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::Rng;

#[allow(dead_code)]
#[path = "../../../crates/chessvision/src/models/onnx_build.rs"]
mod onnx_build;

/// The model files of a folder, without extension (`chessvision::models::{PIECES, GLYPHS}`).
const PIECES: &str = "chess-pieces";
const GLYPHS: &str = "chess-glyphs";

/// The piece classes, in output order (`chessvision::models::PIECE_CLASS_NAMES`).
const PIECE_CLASSES: [&str; 13] = ["empty", "P", "N", "B", "R", "Q", "K", "p", "n", "b", "r", "q", "k"];

/// A models folder a run may use, and what it holds.
#[derive(Clone, Debug)]
pub struct Folder {
    /// `real`, `hostile-<case>` or `mutated`.
    pub name: String,
    pub dir: PathBuf,
}

/// The user's folder and one folder per hostile model, written under `out` (`fuzz-out/models`):
/// each holds the case as the piece model (`cells`, 64 × 64, 13 classes) and as the glyph model
/// (`glyphs`, 32 × 32, the classes of the user's glyph model, or five).
pub fn folders(real: &Path, out: &Path) -> Result<Vec<Folder>> {
    let glyph_classes = glyph_classes(real);
    let piece_meta = serde_json::json!({ "kind": "chess-pieces", "input": { "name": "cells" },
        "output": { "name": "logits", "classes": PIECE_CLASSES } });
    let glyph_meta = serde_json::json!({ "kind": "chess-glyphs", "input": { "name": "glyphs" },
        "output": { "name": "logits", "classes": glyph_classes } });
    let pieces = onnx_build::hostile_models("cells", 64, PIECE_CLASSES.len() as i64);
    let glyphs = onnx_build::hostile_models("glyphs", 32, glyph_classes.len() as i64);
    let mut folders = vec![Folder { name: "real".into(), dir: real.to_path_buf() }];
    for ((name, piece), (_, glyph)) in pieces.into_iter().zip(glyphs) {
        let dir = out.join(format!("hostile-{name}"));
        std::fs::create_dir_all(&dir).with_context(|| dir.display().to_string())?;
        std::fs::write(dir.join(format!("{PIECES}.onnx")), piece)?;
        std::fs::write(dir.join(format!("{PIECES}.json")), piece_meta.to_string())?;
        std::fs::write(dir.join(format!("{GLYPHS}.onnx")), glyph)?;
        std::fs::write(dir.join(format!("{GLYPHS}.json")), glyph_meta.to_string())?;
        folders.push(Folder { name: format!("hostile-{name}"), dir });
    }
    Ok(folders)
}

/// The class names of the user's glyph model, or five made-up ones when it has none.
fn glyph_classes(real: &Path) -> Vec<String> {
    std::fs::read(real.join(format!("{GLYPHS}.json")))
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .and_then(|v| v["output"]["classes"].as_array().map(|a| a.iter().filter_map(|c| c.as_str().map(str::to_owned)).collect::<Vec<_>>()))
        .filter(|c| !c.is_empty())
        .unwrap_or_else(|| (0..5).map(|c| format!("g{c}")).collect())
}

/// Write into `dir` the models of `from` (one of [`folders`]) with 1 to 16 bytes of one model
/// changed (a byte set, a bit flipped, or a run of bytes overwritten), the descriptions as they
/// are, and the parity files half of the time (a model whose weights changed then loads and runs).
pub fn mutate_into(rng: &mut Rng, from: &Folder, dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir).with_context(|| dir.display().to_string())?;
    let parity = rng.below(2) == 0;
    let target = [PIECES, GLYPHS][rng.below(2)];
    for name in [PIECES, GLYPHS] {
        for ext in ["onnx", "json", "parity.json"] {
            let file = format!("{name}.{ext}");
            let to = dir.join(&file);
            let _ = std::fs::remove_file(&to);
            let Ok(mut bytes) = std::fs::read(from.dir.join(&file)) else { continue };
            if ext == "parity.json" && !parity {
                continue;
            }
            if ext == "onnx" && name == target && !bytes.is_empty() {
                for _ in 0..1 + rng.below(16) {
                    let at = rng.below(bytes.len());
                    match rng.below(3) {
                        0 => bytes[at] = rng.next() as u8,
                        1 => bytes[at] ^= 1 << rng.below(8),
                        _ => {
                            for b in bytes.iter_mut().skip(at).take(1 + rng.below(64)) {
                                *b = [0x00, 0xff, 0x7f, 0x80][rng.below(4)];
                            }
                        }
                    }
                }
            }
            std::fs::write(&to, bytes).with_context(|| to.display().to_string())?;
        }
    }
    Ok(())
}

/// A copy of `from`'s files in `to` (a finding keeps the mutated models it was found with).
pub fn copy(from: &Path, to: &Path) {
    if std::fs::create_dir_all(to).is_err() {
        return;
    }
    for entry in std::fs::read_dir(from).into_iter().flatten().flatten() {
        let _ = std::fs::copy(entry.path(), to.join(entry.file_name()));
    }
}
