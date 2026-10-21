//! Chess ▸ PGN toolbar ▸ Collecting corrected characters for retraining (T11; the headless part,
//! wave 10 U3): where a corrected character's sample goes and under what name.
//!
//! The glyph model the OCR area reads with is the user's own, trained by ChessVisionOFF on 32 × 32
//! gray pictures of characters ([`Session::chess_glyph_sample`](crate::Session::chess_glyph_sample)
//! makes one), one folder per character. A sample is written as
//! `<folder>/<class folder>/<confidence>_<document>_p<page>_<n>.png`, the layout of
//! ChessVisionOFF's review queue (`revisao_ocr`), so its review and promotion steps take the files
//! as they are:
//! - the class folder is the corrected character's name as ChessVisionOFF names its classes
//!   ([`class_folder`], reimplemented from that rule, not copied);
//! - the name starts with the model's confidence in what it had read, in thousandths and four
//!   digits, so the most doubtful come first in name order ([`base_name`]);
//! - the document is a 16-digit hash of its bytes ([`doc_hash`]), the page is 1-based, and `n`
//!   counts up from 1 past any file already there: nothing is ever overwritten ([`write_sample`]).
//!
//! The app (`ui-egui`'s `chess_samples`) and the automation tool `chess_glyph_sample` both name
//! and place samples through this module; each writes the file its own way (atomically, and the
//! tool only inside `--root`).

use std::path::{Path, PathBuf};

/// Most names tried for one sample (`_1`, `_2`, …) before giving up.
pub const MAX_COLLISIONS: u32 = 10_000;

/// The folder ChessVisionOFF keeps a character's class in: `upper_A`, `lower_a`, `digit_1` for
/// ASCII letters and digits, `sym_<code point>` for any other character (`sym_9812` is ♔), and for
/// a box read as several characters `ligature_<text>` when they are ASCII letters, digits, `+` or
/// `-`, else `ligature_hex_` and each code point in at least four hexadecimal digits. Reimplemented
/// from the naming rule of ChessVisionOFF's `text/classes.py` (`char_to_folder`), the user's
/// project, so that its training base reads the folders as its own. Always one safe path
/// component: ASCII letters, digits, `_`, `+` and `-` only.
pub fn class_folder(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    match chars.as_slice() {
        [] => "unknown".to_owned(),
        [c] if c.is_ascii_uppercase() => format!("upper_{c}"),
        [c] if c.is_ascii_lowercase() => format!("lower_{c}"),
        [c] if c.is_ascii_digit() => format!("digit_{c}"),
        [c] => format!("sym_{}", u32::from(*c)),
        many if many.iter().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-')) => format!("ligature_{text}"),
        many => format!("ligature_hex_{}", many.iter().map(|c| format!("{:04x}", u32::from(*c))).collect::<String>()),
    }
}

/// A sample's name without its count: the confidence in thousandths (four digits; a value that
/// is not a number counts as 0, and it is kept between 0 and 1), the document's hash and the page
/// (0-based here, 1-based in the name).
pub fn base_name(confidence: f32, doc: &str, page: usize) -> String {
    // Finite and kept between 0 and 1000, so the cast is exact.
    let permille = if confidence.is_finite() { (confidence.clamp(0.0, 1.0) * 1000.0).round() as u32 } else { 0 };
    format!("{permille:04}_{doc}_p{:04}", page.saturating_add(1))
}

/// A 64-bit FNV-1a hash of `bytes`, in 16 hexadecimal digits: a document's name in sample files
/// (its file name could be personal; the hash only tells documents apart).
pub fn doc_hash(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// Write `png` as `<folder>/<class>/<base>_<n>.png`, `n` the first count from 1 that names no file
/// (nor a link) yet, so nothing is overwritten; `write` writes one file, making its folder first.
/// Returns the path written.
pub fn write_sample(
    folder: &Path,
    class: &str,
    base: &str,
    png: &[u8],
    mut write: impl FnMut(&Path, &[u8]) -> std::io::Result<()>,
) -> std::io::Result<PathBuf> {
    let dir = folder.join(class);
    for n in 1..=MAX_COLLISIONS {
        let path = dir.join(format!("{base}_{n}.png"));
        if path.symlink_metadata().is_ok() {
            continue;
        }
        write(&path, png)?;
        return Ok(path);
    }
    Err(std::io::Error::other("too many samples of the same name"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_folders_follow_chessvisionoffs_names() {
        assert_eq!(class_folder("♔"), "sym_9812");
        assert_eq!(class_folder("N"), "upper_N");
        assert_eq!(class_folder("f"), "lower_f");
        assert_eq!(class_folder("3"), "digit_3");
        assert_eq!(class_folder("+"), "sym_43");
        assert_eq!(class_folder("ä"), "sym_228");
        assert_eq!(class_folder("f7"), "ligature_f7");
        assert_eq!(class_folder("-g"), "ligature_-g");
        assert_eq!(class_folder("♘f"), "ligature_hex_26580066");
        assert_eq!(class_folder("a\u{1F600}"), "ligature_hex_00611f600");
        assert_eq!(class_folder(""), "unknown");
        for text in ["..", "/", "a/b", "\\", ".", "a.b"] {
            let class = class_folder(text);
            assert!(class.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '-')), "{text:?} → {class}");
        }
    }

    #[test]
    fn names_start_with_the_confidence_and_never_overwrite() {
        assert_eq!(base_name(0.5, "00ff", 0), "0500_00ff_p0001");
        assert_eq!(base_name(f32::NAN, "x", 67), "0000_x_p0068");
        assert_eq!(base_name(7.0, "x", usize::MAX), format!("1000_x_p{}", usize::MAX));
        assert_eq!(doc_hash(b""), "cbf29ce484222325");
        let dir = std::env::temp_dir().join(format!("pdfcraft-engine-samples-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let write = |p: &Path, b: &[u8]| {
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(p, b)
        };
        let first = write_sample(&dir, "digit_3", "0500_00ff_p0001", b"one", write).unwrap();
        let second = write_sample(&dir, "digit_3", "0500_00ff_p0001", b"two", write).unwrap();
        assert_eq!(first.file_name().unwrap(), "0500_00ff_p0001_1.png");
        assert_eq!(second.file_name().unwrap(), "0500_00ff_p0001_2.png");
        assert_eq!(std::fs::read(&first).unwrap(), b"one", "the first is not overwritten");
        let refused = write_sample(&dir, "digit_3", "x", b"", |_, _| Err(std::io::Error::other("no")));
        assert!(refused.is_err(), "the writer's error comes back");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
