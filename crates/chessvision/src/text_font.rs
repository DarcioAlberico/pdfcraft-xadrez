//! The user's figurine font for HTML and EPUB exports (blueprint decision D4).
//!
//! Exported text writes figurines as Unicode chess symbols (♘f3), and a reader shows them with
//! whatever font it finds, often one without them or with pieces that do not match the book.
//! A font the user points PdfCraft at is carried into the export for those symbols only: an
//! `@font-face` restricted by `unicode-range` ([`UNICODE_RANGE`]), so the prose keeps the body
//! font. HTML embeds it as a data URL (the file stays self-contained); an EPUB carries it as a
//! file of the book.
//!
//! PdfCraft ships no such font. Where it comes from, in order ([`find`]):
//! 1. `$PDFCRAFT_CHESS_TEXT_FONT`, the path of a TrueType, OpenType, WOFF or WOFF2 font;
//! 2. a `fonts` folder beside the executable: its first font with chess figurines.
//!
//! Without either, exports are as before.

use std::path::{Path, PathBuf};

use skrifa::{FontRef, MetadataProvider};

/// Largest font file read.
const MAX_FONT_BYTES: usize = 16 << 20;
/// Most files looked at in the `fonts` folder.
const MAX_FOLDER_FILES: usize = 200;

/// The CSS family name the exports give the font.
pub const FAMILY: &str = "PdfCraft Figurines";

/// The characters the font is used for: the twelve chess symbols, then the Informator signs a
/// body font often lacks (⩲ ⩱ ∓ □ △ ∆ ∇ ⟳ ⇆ ⨀ ⨁ ⌓).
pub const UNICODE_RANGE: &str = "U+2654-265F,U+2A71-2A72,U+2213,U+25A1,U+25B3,U+2206,U+2207,U+27F3,U+21C6,U+2A00-2A01,U+2313";

/// A font file's format, from its first bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontFormat {
    TrueType,
    OpenType,
    Woff,
    Woff2,
}

impl FontFormat {
    fn sniff(bytes: &[u8]) -> Option<FontFormat> {
        match bytes.get(..4)? {
            [0, 1, 0, 0] | b"true" => Some(FontFormat::TrueType),
            b"OTTO" => Some(FontFormat::OpenType),
            b"wOFF" => Some(FontFormat::Woff),
            b"wOF2" => Some(FontFormat::Woff2),
            _ => None,
        }
    }

    /// The `format()` hint of a CSS `src`.
    pub fn css(self) -> &'static str {
        match self {
            FontFormat::TrueType => "truetype",
            FontFormat::OpenType => "opentype",
            FontFormat::Woff => "woff",
            FontFormat::Woff2 => "woff2",
        }
    }

    /// The media type (also an EPUB 3 core media type).
    pub fn mime(self) -> &'static str {
        match self {
            FontFormat::TrueType => "font/ttf",
            FontFormat::OpenType => "font/otf",
            FontFormat::Woff => "font/woff",
            FontFormat::Woff2 => "font/woff2",
        }
    }

    /// The file extension, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            FontFormat::TrueType => "ttf",
            FontFormat::OpenType => "otf",
            FontFormat::Woff => "woff",
            FontFormat::Woff2 => "woff2",
        }
    }
}

/// A font to show figurines with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextFont {
    pub bytes: Vec<u8>,
    pub format: FontFormat,
    /// Where it was read from, to tell the user.
    pub path: Option<PathBuf>,
}

/// Why a font cannot be used.
#[derive(Debug, thiserror::Error)]
pub enum TextFontError {
    #[error("cannot read {path}: {source}")]
    Read { path: PathBuf, source: std::io::Error },
    #[error("the font file is {0} bytes, more than the {MAX_FONT_BYTES} accepted")]
    TooLarge(usize),
    #[error("not a TrueType, OpenType, WOFF or WOFF2 font")]
    NotAFont,
    #[error("the font has no chess figurines (U+2654–U+265F)")]
    NoFigurines,
}

/// A font from its bytes. TrueType and OpenType fonts must have the chess figurines; WOFF and
/// WOFF2 are compressed and taken as they are.
pub fn from_bytes(bytes: Vec<u8>) -> Result<TextFont, TextFontError> {
    if bytes.len() > MAX_FONT_BYTES {
        return Err(TextFontError::TooLarge(bytes.len()));
    }
    let format = FontFormat::sniff(&bytes).ok_or(TextFontError::NotAFont)?;
    if matches!(format, FontFormat::TrueType | FontFormat::OpenType) {
        let font = FontRef::new(&bytes).map_err(|_| TextFontError::NotAFont)?;
        let charmap = font.charmap();
        if !('\u{2654}'..='\u{265F}').any(|c| charmap.map(c).is_some()) {
            return Err(TextFontError::NoFigurines);
        }
    }
    Ok(TextFont { bytes, format, path: None })
}

/// A font read from a file.
pub fn from_file(path: &Path) -> Result<TextFont, TextFontError> {
    let size = std::fs::metadata(path).map_err(|source| TextFontError::Read { path: path.to_path_buf(), source })?.len();
    let size = usize::try_from(size).unwrap_or(usize::MAX);
    if size > MAX_FONT_BYTES {
        return Err(TextFontError::TooLarge(size));
    }
    let bytes = std::fs::read(path).map_err(|source| TextFontError::Read { path: path.to_path_buf(), source })?;
    from_bytes(bytes).map(|f| TextFont { path: Some(path.to_path_buf()), ..f })
}

/// The first font with figurines in `dir`, by file name.
pub fn from_folder(dir: &Path) -> Option<TextFont> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .take(MAX_FOLDER_FILES)
        .map(|e| e.path())
        .filter(|p| {
            p.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "ttf" | "otf" | "woff" | "woff2"))
        })
        .collect();
    files.sort();
    files.iter().find_map(|p| from_file(p).ok())
}

/// The user's figurine font: `$PDFCRAFT_CHESS_TEXT_FONT`, else the `fonts` folder beside the
/// executable. A configured font that fails is reported, not silently replaced.
pub fn find() -> (Option<TextFont>, Option<TextFontError>) {
    if let Some(path) = std::env::var_os("PDFCRAFT_CHESS_TEXT_FONT").map(PathBuf::from) {
        return match from_file(&path) {
            Ok(f) => (Some(f), None),
            Err(e) => (None, Some(e)),
        };
    }
    let beside = std::env::current_exe().ok().and_then(|exe| Some(exe.parent()?.join("fonts")));
    (beside.and_then(|dir| from_folder(&dir)), None)
}

/// The `@font-face` rule for `font` loaded from `src` (a URL, already escaped for CSS).
pub fn font_face(font: &TextFont, src: &str) -> String {
    format!("@font-face{{font-family:\"{FAMILY}\";src:url(\"{src}\") format(\"{}\");unicode-range:{UNICODE_RANGE}}}", font.format.css())
}

/// `font` as a `data:` URL.
pub fn data_url(font: &TextFont) -> String {
    use base64::Engine as _;
    format!("data:{};base64,{}", font.format.mime(), base64::engine::general_purpose::STANDARD.encode(&font.bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_are_told_by_their_first_bytes() {
        assert_eq!(FontFormat::sniff(&[0, 1, 0, 0, 9]), Some(FontFormat::TrueType));
        assert_eq!(FontFormat::sniff(b"OTTO...."), Some(FontFormat::OpenType));
        assert_eq!(FontFormat::sniff(b"wOFF...."), Some(FontFormat::Woff));
        assert_eq!(FontFormat::sniff(b"wOF2...."), Some(FontFormat::Woff2));
        assert_eq!(FontFormat::sniff(b"%PDF"), None);
        assert_eq!(FontFormat::sniff(b"ab"), None);
    }

    #[test]
    fn a_font_without_figurines_or_not_a_font_is_refused() {
        assert!(matches!(from_bytes(b"not a font".to_vec()), Err(TextFontError::NotAFont)));
        // Inter has no chess symbols.
        let inter = include_bytes!("../../../assets/fonts/Inter-Regular.ttf").to_vec();
        assert!(matches!(from_bytes(inter), Err(TextFontError::NoFigurines)));
        assert!(matches!(from_bytes(vec![0; MAX_FONT_BYTES + 1]), Err(TextFontError::TooLarge(_))));
    }

    #[test]
    fn woff_is_taken_as_it_is_and_written_as_a_face() {
        let font = from_bytes(b"wOF2 compressed font data".to_vec()).unwrap();
        assert_eq!(font.format, FontFormat::Woff2);
        let url = data_url(&font);
        assert!(url.starts_with("data:font/woff2;base64,d09GMi"), "{url}");
        let face = font_face(&font, "fonts/figurines.woff2");
        assert_eq!(
            face,
            format!(
                "@font-face{{font-family:\"PdfCraft Figurines\";src:url(\"fonts/figurines.woff2\") format(\"woff2\");unicode-range:{UNICODE_RANGE}}}"
            )
        );
    }

    #[test]
    fn a_folder_gives_its_first_font_with_figurines() {
        let dir = std::env::temp_dir().join(format!("pdfcraft-text-font-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(from_folder(&dir).is_none());
        // Inter has no figurines and is passed over; the WOFF2 is taken.
        std::fs::write(dir.join("a-inter.ttf"), include_bytes!("../../../assets/fonts/Inter-Regular.ttf")).unwrap();
        std::fs::write(dir.join("notes.txt"), b"not a font").unwrap();
        assert!(from_folder(&dir).is_none());
        std::fs::write(dir.join("b-figurines.WOFF2"), b"wOF2 data").unwrap();
        let found = from_folder(&dir).unwrap();
        assert_eq!(found.format, FontFormat::Woff2);
        assert_eq!(found.path.as_deref(), Some(dir.join("b-figurines.WOFF2").as_path()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Builds a face from a real figurine font when `PDFCRAFT_CHESS_TEST_TEXT_FONT` names one
    /// (DejaVu Sans, say). Skipped otherwise: no such font is committed (decision D4).
    #[test]
    fn a_real_figurine_font_is_accepted() {
        let Some(path) = std::env::var_os("PDFCRAFT_CHESS_TEST_TEXT_FONT") else { return };
        let font = from_file(Path::new(&path)).unwrap();
        assert!(matches!(font.format, FontFormat::TrueType | FontFormat::OpenType));
        assert!(data_url(&font).len() > font.bytes.len());
    }
}
