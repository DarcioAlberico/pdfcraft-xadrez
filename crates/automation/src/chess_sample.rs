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
//!
//! One session (one [`Automation`]: an MCP server's run, a script's) writes at most
//! [`MAX_SESSION_SAMPLES`] samples and [`MAX_SESSION_SAMPLE_BYTES`] bytes of them; a call past
//! either is refused and writes nothing. The app has its own cap (2,000 a session).

use pdfcraft_engine::chess::samples;
use pdfcraft_engine::chess::text_ocr::SAMPLE_SIDE;
use serde_json::{Value, json};

use crate::{Args, Automation, Result, ToolError, failed, write_atomic};

/// Longest corrected text taken (a character, or a few read as one box).
const MAX_TEXT_CHARS: usize = 16;

/// Most samples one session writes: a long review of a book, many times over, and still a
/// bounded number of files in the user's folder.
pub(crate) const MAX_SESSION_SAMPLES: usize = 10_000;

/// Most bytes of samples one session writes. A 32 x 32 gray PNG is about a kilobyte, so the count
/// is reached first; this holds should a sample ever be larger.
pub(crate) const MAX_SESSION_SAMPLE_BYTES: usize = 16 << 20;

/// What one session has written: samples and their bytes.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct SampleBudget {
    written: usize,
    bytes: usize,
}

impl Automation {
    pub(crate) fn chess_glyph_sample(&mut self, a: &Args) -> Result<Value> {
        // The path first: one that leaves the root is refused before anything is read.
        let folder_arg = a.str("folder")?;
        let folder = self.resolve(folder_arg, true)?;
        if self.glyph_samples.written >= MAX_SESSION_SAMPLES {
            return Err(failed(format!(
                "this session has written {MAX_SESSION_SAMPLES} glyph samples, the most one session writes: start a new session to write more"
            )));
        }
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
        let bytes = self.glyph_samples.bytes.saturating_add(png.len());
        if bytes > MAX_SESSION_SAMPLE_BYTES {
            return Err(failed(format!(
                "this session has written its {MAX_SESSION_SAMPLE_BYTES} bytes of glyph samples: start a new session to write more"
            )));
        }
        let class = samples::class_folder(text);
        let base = samples::base_name(confidence as f32, &samples::doc_hash(&doc.bytes), page);
        let written = samples::write_sample(&folder, &class, &base, &png, |path, bytes| {
            // The file itself is held to the root too: the class folder may be a link out of it.
            let shown = path.to_str().ok_or_else(|| std::io::Error::other("the folder's path is not valid Unicode"))?;
            let target = self.resolve(shown, true).map_err(|e| std::io::Error::other(e.to_string()))?;
            write_atomic(&target, bytes).map_err(|e| std::io::Error::other(e.to_string()))
        })
        .map_err(|e| ToolError::Failed(format!("{folder_arg}: {e}")))?;
        self.glyph_samples = SampleBudget { written: self.glyph_samples.written.saturating_add(1), bytes };
        Ok(json!({
            "path": written.to_string_lossy(),
            "class": class,
            "name": written.file_name().map(|n| n.to_string_lossy().into_owned()),
            "side": SAMPLE_SIDE,
            "bytes": png.len(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use super::*;

    /// One page of 600 x 800 pt with "1.e4 e5" in Helvetica at (60, 500).
    fn moves_pdf() -> Vec<u8> {
        let content = "BT /F1 14 Tf 60 500 Td (1.e4 e5) Tj ET\n";
        let objs = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 600 800] >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
            format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_string(),
        ];
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (i, o) in objs.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
        }
        let xref = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
        for o in offsets {
            out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
        out
    }

    /// The files under `dir`, at any depth.
    fn files_in(dir: &Path) -> usize {
        let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
        entries.flatten().map(|e| if e.path().is_dir() { files_in(&e.path()) } else { 1 }).sum()
    }

    /// A session writes samples up to its cap of samples and of bytes; the call past either is
    /// refused and writes nothing, and a fresh budget (a new session) writes again.
    #[test]
    fn a_session_writes_glyph_samples_up_to_its_caps_and_no_more() {
        let root = std::env::temp_dir().join(format!("pdfcraft-sample-cap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let mut a = Automation::new().with_root(&root).unwrap();
        let doc = a.session.open("moves.pdf", None, Arc::new(moves_pdf()), None).unwrap();
        let args = json!({ "doc": doc.0, "page": 1, "rect": [58.0, 288.0, 70.0, 302.0], "text": "1", "folder": "samples" });
        let samples = root.join("samples");
        let refused = |a: &mut Automation, says: &str| match a.call("chess_glyph_sample", &args) {
            Err(ToolError::Failed(m)) => assert!(m.contains(says), "{m}"),
            other => panic!("a sample past the cap was written: {other:?}"),
        };
        // The last sample the cap allows is written, and counted.
        a.glyph_samples.written = MAX_SESSION_SAMPLES - 1;
        assert!(a.call("chess_glyph_sample", &args).is_ok());
        assert_eq!(files_in(&samples), 1);
        assert_eq!(a.glyph_samples.written, MAX_SESSION_SAMPLES);
        assert!(a.glyph_samples.bytes > 0, "its bytes are counted");
        // The next one is refused and writes nothing.
        refused(&mut a, "glyph samples, the most one session writes");
        assert_eq!(files_in(&samples), 1, "nothing written past the cap");
        // So is one that would pass the bytes cap.
        a.glyph_samples = SampleBudget { written: 0, bytes: MAX_SESSION_SAMPLE_BYTES - 1 };
        refused(&mut a, "bytes of glyph samples");
        assert_eq!(files_in(&samples), 1, "nothing written past the bytes cap");
        assert_eq!(a.glyph_samples.written, 0, "a refused call is not counted");
        // A fresh budget writes again.
        a.glyph_samples = SampleBudget::default();
        assert!(a.call("chess_glyph_sample", &args).is_ok());
        assert_eq!(files_in(&samples), 2);
        let _ = std::fs::remove_dir_all(&root);
    }
}
