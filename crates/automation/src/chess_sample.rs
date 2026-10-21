//! `chess_glyph_sample`: Chess ▸ PGN toolbar ▸ Collect corrected characters for retraining,
//! without the interface (PGN toolbar T11; wave 10, U3).
//!
//! One corrected character of a page becomes a 32 × 32 gray PNG for retraining the user's glyph
//! model: the character's box rendered again (`Session::chess_glyph_sample`, the same picture the
//! app saves), written as `<folder>/<class folder>/<confidence>_<document hash>_p<page>_<n>.png`,
//! the layout of ChessVisionOFF's review queue (`revisao_ocr`), named and placed by
//! `engine::chess::samples` as the app's are. Nothing is overwritten (`n` counts up past the files
//! there), and the file is written atomically.
//!
//! `folder` is confined to `--root` and resolved first, before anything is read; the file finally
//! written is resolved again, so a class folder that is a link out of the root is refused too.
//! Saving is extraction: a document that does not allow copying gives none. An encrypted document
//! gives one only with `allow_encrypted` (the sample is written unencrypted), as the app asks.

use pdfcraft_engine::chess::samples;
use pdfcraft_engine::chess::text_ocr::SAMPLE_SIDE;
use serde_json::{Value, json};

use crate::{Args, Automation, Result, ToolError, failed, write_atomic};

/// Longest corrected text taken (a character, or a few read as one box).
const MAX_TEXT_CHARS: usize = 16;

impl Automation {
    pub(crate) fn chess_glyph_sample(&self, a: &Args) -> Result<Value> {
        // The path first: one that leaves the root is refused before anything is read.
        let folder_arg = a.str("folder")?;
        let folder = self.resolve(folder_arg, true)?;
        let text = a.str("text")?;
        let chars = text.chars().count();
        if chars == 0 || chars > MAX_TEXT_CHARS || text.chars().any(char::is_control) {
            return Err(Args::wrong("text", &format!("1 to {MAX_TEXT_CHARS} characters, none a control character")));
        }
        let confidence = a.opt_num("confidence")?.unwrap_or(0.0);
        if !(0.0..=1.0).contains(&confidence) {
            return Err(Args::wrong("confidence", "a number from 0 to 1"));
        }
        let dpi = a.opt_num("dpi")?.unwrap_or(f64::from(pdfcraft_engine::chess::text_ocr::OCR_DPI));
        let page = self.page(a)?;
        let [x0, y0, x1, y1] = a.nums::<4>("rect")?.ok_or_else(|| Args::wrong("rect", "[x0, y0, x1, y1] from the top-left of the displayed page"))?;
        let doc = self.doc(a)?;
        if doc.info.encrypted && !a.opt_bool("allow_encrypted")?.unwrap_or(false) {
            return Err(failed("the document is encrypted and the sample would be written unencrypted: pass allow_encrypted to write it"));
        }
        // Finite (checked by `nums`); a value past f32 becomes infinite, which the engine refuses.
        let rect = [x0 as f32, y0 as f32, x1 as f32, y1 as f32];
        let png = self.session.chess_glyph_sample(doc.id, page, rect, dpi as f32).map_err(failed)?;
        let class = samples::class_folder(text);
        let base = samples::base_name(confidence as f32, &samples::doc_hash(&doc.bytes), page);
        let written = samples::write_sample(&folder, &class, &base, &png, |path, bytes| {
            // The file itself is held to the root too: the class folder may be a link out of it.
            let shown = path.to_str().ok_or_else(|| std::io::Error::other("the folder's path is not valid Unicode"))?;
            let target = self.resolve(shown, true).map_err(|e| std::io::Error::other(e.to_string()))?;
            write_atomic(&target, bytes).map_err(|e| std::io::Error::other(e.to_string()))
        })
        .map_err(|e| ToolError::Failed(format!("{folder_arg}: {e}")))?;
        Ok(json!({
            "path": written.to_string_lossy(),
            "class": class,
            "name": written.file_name().map(|n| n.to_string_lossy().into_owned()),
            "side": SAMPLE_SIDE,
            "bytes": png.len(),
        }))
    }
}
