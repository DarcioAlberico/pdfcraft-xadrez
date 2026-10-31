//! The user's chess text fonts for HTML and EPUB exports (blueprint decision D4).
//!
//! Exported text writes figurines (♘f3) and annotation signs (⩲ ± □) as Unicode, and a reader
//! shows them with whatever font it finds. Fonts the user points PdfCraft at are carried into
//! the export instead, each for what it draws ([`FontRole`]):
//!
//! - a font with the Unicode chess symbols (DejaVu Sans, say): an `@font-face` restricted by
//!   `unicode-range` ([`UNICODE_RANGE`]) to those symbols, so the prose keeps the body font;
//! - a figurine font that draws pieces on the letters K Q R B N (SemFig, Figurine CB… the
//!   [`pdfcraft_chess::catalog`] figurine families): the piece letters of each move are written
//!   in it, and nothing else, so "Kasparov" in the prose stays a word;
//! - ISChess, a symbol font in the Chess Informant tradition whose letters draw the signs
//!   ([`ischess_letter`]): the annotation glyphs are written in it.
//!
//! HTML embeds the fonts as data URLs (the file stays self-contained); an EPUB carries them as
//! files of the book.
//!
//! PdfCraft ships no such font. Where they come from ([`find_fonts`]):
//! 1. `$PDFCRAFT_CHESS_TEXT_FONT`: one font file, or several separated as in `PATH`;
//! 2. a `fonts` folder beside the executable: every font in it, by file name, the first of each
//!    role kept (a figurine font by letters is preferred over a Unicode one). Diagram fonts in
//!    the same folder are for the pieces ([`crate::pieces`]) and are passed over here.
//!
//! Without any, exports are as before.

use std::path::{Path, PathBuf};

use pdfcraft_chess::catalog::{FontKind, lookup_family, normalize_font_name};
use skrifa::raw::TableProvider;
use skrifa::string::StringId;
use skrifa::{FontRef, MetadataProvider};

/// Largest font file read.
const MAX_FONT_BYTES: usize = 16 << 20;
/// Most files looked at in the `fonts` folder.
const MAX_FOLDER_FILES: usize = 200;

/// The CSS family name the exports give the figurine font.
pub const FAMILY: &str = "PdfCraft Figurines";
/// The CSS family name the exports give the sign font.
pub const SIGN_FAMILY: &str = "PdfCraft Signs";

/// The characters a Unicode figurine font is used for: the twelve chess symbols, then the
/// Informator signs a body font often lacks (⩲ ⩱ ∓ □ △ ∆ ∇ ⟳ ⇆ ⨀ ⨁ ⌓).
pub const UNICODE_RANGE: &str = "U+2654-265F,U+2A71-2A72,U+2213,U+25A1,U+25B3,U+2206,U+2207,U+27F3,U+21C6,U+2A00-2A01,U+2313";

/// The ISChess letter of each annotation code it draws, both sides' codes listed. Read off the
/// glyph table printed in the font itself, as the PDFimport plugin by Wagner Escorcio records it
/// (used with his permission, `docs/chess/provenance.md`); its other letters draw signs PGN has
/// no code for.
const ISCHESS_SIGNS: &[(u16, char)] = &[
    (5, 'a'),  // !?
    (6, 'b'),  // ?!
    (7, 'd'),  // □ only move
    (10, 'S'), // =
    (13, 'T'), // ∞
    (14, 'M'), // ⩲
    (15, 'N'), // ⩱
    (16, 'O'), // ±
    (17, 'P'), // ∓
    (18, 'Q'), // +−
    (19, 'R'), // −+
    (22, '['), // ⨀ zugzwang
    (23, '['),
    (26, 'W'), // ○ space
    (27, 'W'),
    (32, 'V'), // ⟳ development
    (33, 'V'),
    (36, 'Y'), // ↑ initiative
    (37, 'Y'),
    (40, 'X'), // → attack
    (41, 'X'),
    (44, 'U'), // © compensation
    (45, 'U'),
    (132, 'Z'), // ⇄ counterplay
    (133, 'Z'),
    (138, 'u'), // ⊕ time trouble
    (139, 'u'),
    (140, 'c'), // △ with the idea
    (142, 'e'), // ⌓ better is
    (239, 'f'), // ⇔ file
    (240, 'g'), // ⇗ diagonal
    (241, 'h'), // ⊞ centre
    (242, 'i'), // ⟫ kingside
    (243, 'j'), // ⟪ queenside
    (244, 'k'), // ✕ weak point
    (245, 'l'), // ⊥ endgame
    (246, 'm'), // pair of bishops
    (247, 'n'), // bishops of opposite colours
    (248, 'o'), // bishops of the same colour
    (249, 'p'), // ⯺ connected pawns
    (250, 'q'), // ⯻ isolated pawns
    (251, 'r'), // ⯼ doubled pawns
    (252, 's'), // ⯽ passed pawn
    (254, 'v'), // ∟ with
    (255, 'w'), // ⯾ without
];

/// The ISChess letter that draws annotation `code`, if the font has it.
pub fn ischess_letter(code: u16) -> Option<char> {
    ISCHESS_SIGNS.iter().find(|(c, _)| *c == code).map(|(_, l)| *l)
}

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

/// What a font draws, and so how an export uses it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontRole {
    /// The Unicode chess symbols (U+2654–U+265F).
    Unicode,
    /// Pieces on the letters K Q R B N: a catalogued figurine family (SemFig…).
    Letters,
    /// The annotation signs on letters: ISChess ([`ischess_letter`]).
    Signs,
}

/// A font to show figurines or signs with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextFont {
    pub bytes: Vec<u8>,
    pub format: FontFormat,
    pub role: FontRole,
    /// Its family name, when the file says.
    pub family: String,
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
    #[error("the font has no chess figurines (U+2654–U+265F) and is not a figurine or sign font PdfCraft knows")]
    NoFigurines,
    /// Its OS/2 `fsType` says Restricted License embedding: an export must not carry it (plan A8
    /// P9b).
    #[error("the font {0:?} may not be embedded: its licence says so (OS/2 fsType: Restricted License)")]
    Restricted(String),
}

/// Whether an OS/2 `fsType` forbids embedding: its usage permissions (bits 0–3) say Restricted
/// License (2) alone. Preview & Print, Editable and Installable fonts may be embedded.
pub fn restricted(fs_type: u16) -> bool {
    fs_type & 0x000F == 0x0002
}

/// The role a font named `name` has by its name alone: ISChess, or a catalogued figurine family.
fn role_by_name(name: &str) -> Option<FontRole> {
    let bare = normalize_font_name(name);
    if bare.contains("ischess") {
        return Some(FontRole::Signs);
    }
    lookup_family(name).filter(|f| f.kind == FontKind::Figurine).map(|_| FontRole::Letters)
}

/// Whether `name` is a catalogued chess diagram family's (an empty name is none).
fn is_diagram_font(name: &str) -> bool {
    !normalize_font_name(name).is_empty() && lookup_family(name).is_some_and(|f| f.kind == FontKind::Diagram && f.key != "unicode")
}

/// A font from its bytes. `name_hint` (the file name, say) helps when the font's own name does
/// not say what it is. A TrueType or OpenType font must be ISChess, a catalogued figurine font
/// or have the Unicode chess symbols; WOFF and WOFF2 are compressed, so only their name says,
/// else they are taken as Unicode figurine fonts.
pub fn from_bytes(bytes: Vec<u8>, name_hint: &str) -> Result<TextFont, TextFontError> {
    if bytes.len() > MAX_FONT_BYTES {
        return Err(TextFontError::TooLarge(bytes.len()));
    }
    let format = FontFormat::sniff(&bytes).ok_or(TextFontError::NotAFont)?;
    let (role, family) = if matches!(format, FontFormat::TrueType | FontFormat::OpenType) {
        let font = FontRef::new(&bytes).map_err(|_| TextFontError::NotAFont)?;
        let family = font.localized_strings(StringId::FAMILY_NAME).english_or_first().map(|s| s.to_string()).unwrap_or_default();
        let named = role_by_name(&family).or_else(|| role_by_name(name_hint));
        let unicode = ('\u{2654}'..='\u{265F}').any(|c| font.charmap().map(c).is_some());
        let role = named.or(unicode.then_some(FontRole::Unicode));
        // A chess diagram font with no figurines is for the pieces (`crate::pieces`), which share
        // the folder: passed over without a word about its licence, as no export carries it.
        if role.is_none() && [family.as_str(), name_hint].into_iter().any(is_diagram_font) {
            return Err(TextFontError::NoFigurines);
        }
        // An export embeds the font: one whose licence forbids it is refused (a font with no OS/2
        // table says nothing, and is taken).
        if font.os2().is_ok_and(|os2| restricted(os2.fs_type())) {
            return Err(TextFontError::Restricted(family));
        }
        (role.ok_or(TextFontError::NoFigurines)?, family)
    } else {
        (role_by_name(name_hint).unwrap_or(FontRole::Unicode), String::new())
    };
    Ok(TextFont { bytes, format, role, family, path: None })
}

/// A font read from a file.
pub fn from_file(path: &Path) -> Result<TextFont, TextFontError> {
    let size = std::fs::metadata(path).map_err(|source| TextFontError::Read { path: path.to_path_buf(), source })?.len();
    let size = usize::try_from(size).unwrap_or(usize::MAX);
    if size > MAX_FONT_BYTES {
        return Err(TextFontError::TooLarge(size));
    }
    let bytes = std::fs::read(path).map_err(|source| TextFontError::Read { path: path.to_path_buf(), source })?;
    let hint = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
    from_bytes(bytes, hint).map(|f| TextFont { path: Some(path.to_path_buf()), ..f })
}

/// The export fonts found.
#[derive(Debug, Default)]
pub struct UserFonts {
    /// For the figurines: by letters ([`FontRole::Letters`]) or Unicode.
    pub figurines: Option<TextFont>,
    /// For the annotation signs ([`FontRole::Signs`]).
    pub signs: Option<TextFont>,
    /// Configured fonts that could not be used.
    pub errors: Vec<TextFontError>,
}

impl UserFonts {
    /// Keep `font` if its role's slot is free (a font by letters takes the figurine slot from a
    /// Unicode one).
    fn offer(&mut self, font: TextFont) {
        match font.role {
            FontRole::Signs => {
                self.signs.get_or_insert(font);
            }
            FontRole::Letters if self.figurines.as_ref().is_none_or(|f| f.role == FontRole::Unicode) => self.figurines = Some(font),
            FontRole::Letters | FontRole::Unicode => {
                self.figurines.get_or_insert(font);
            }
        }
    }
}

/// Font files in `dir`, by file name.
fn font_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .take(MAX_FOLDER_FILES)
        .map(|e| e.path())
        .filter(|p| {
            p.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "ttf" | "otf" | "woff" | "woff2"))
        })
        .collect();
    files.sort();
    files
}

/// The export fonts in `dir`. Files that are no export font (a diagram font, say) are skipped;
/// one whose licence forbids embedding is reported, so the user knows why it was not used.
pub fn from_folder(dir: &Path) -> UserFonts {
    let mut found = UserFonts::default();
    for path in font_files(dir) {
        match from_file(&path) {
            Ok(font) => found.offer(font),
            Err(e @ TextFontError::Restricted(_)) => found.errors.push(e),
            Err(_) => {}
        }
    }
    found
}

/// The user's export fonts: the files `$PDFCRAFT_CHESS_TEXT_FONT` names, else the `fonts` folder
/// beside the executable. A configured file that fails is reported, not silently skipped.
pub fn find_fonts() -> UserFonts {
    if let Some(paths) = std::env::var_os("PDFCRAFT_CHESS_TEXT_FONT") {
        let mut found = UserFonts::default();
        for path in std::env::split_paths(&paths).filter(|p| !p.as_os_str().is_empty()) {
            match from_file(&path) {
                Ok(f) => found.offer(f),
                Err(e) => found.errors.push(e),
            }
        }
        return found;
    }
    beside_executable().map(|dir| from_folder(&dir)).unwrap_or_default()
}

/// The `fonts` folder beside the executable.
pub fn beside_executable() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.join("fonts"))
}

/// The `@font-face` rule for `font` loaded from `src` (a URL, already escaped for CSS): named
/// [`FAMILY`] or [`SIGN_FAMILY`], and kept to [`UNICODE_RANGE`] for a Unicode figurine font.
pub fn font_face(font: &TextFont, src: &str) -> String {
    let family = if font.role == FontRole::Signs { SIGN_FAMILY } else { FAMILY };
    let range = if font.role == FontRole::Unicode { format!(";unicode-range:{UNICODE_RANGE}") } else { String::new() };
    format!("@font-face{{font-family:\"{family}\";src:url(\"{src}\") format(\"{}\"){range}}}", font.format.css())
}

/// `font` as a `data:` URL.
pub fn data_url(font: &TextFont) -> String {
    use base64::Engine as _;
    format!("data:{};base64,{}", font.format.mime(), base64::engine::general_purpose::STANDARD.encode(&font.bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font(role: FontRole) -> TextFont {
        TextFont { bytes: b"wOF2 data".to_vec(), format: FontFormat::Woff2, role, family: String::new(), path: None }
    }

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
    fn a_font_without_a_chess_role_or_not_a_font_is_refused() {
        assert!(matches!(from_bytes(b"not a font".to_vec(), "x"), Err(TextFontError::NotAFont)));
        // Inter has no chess symbols and is no catalogued figurine font.
        let inter = include_bytes!("../../../assets/fonts/Inter-Regular.ttf").to_vec();
        assert!(matches!(from_bytes(inter, "Inter-Regular"), Err(TextFontError::NoFigurines)));
        assert!(matches!(from_bytes(vec![0; MAX_FONT_BYTES + 1], "x"), Err(TextFontError::TooLarge(_))));
    }

    /// Inter with its OS/2 fsType set to `fs_type`.
    fn inter_with_fs_type(fs_type: u16) -> Vec<u8> {
        let mut bytes = include_bytes!("../../../assets/fonts/Inter-Regular.ttf").to_vec();
        let tables = usize::from(u16::from_be_bytes([bytes[4], bytes[5]]));
        let os2 = (0..tables)
            .map(|i| 12 + 16 * i)
            .find(|&at| &bytes[at..at + 4] == b"OS/2")
            .map(|at| u32::from_be_bytes([bytes[at + 8], bytes[at + 9], bytes[at + 10], bytes[at + 11]]) as usize)
            .expect("Inter has an OS/2 table");
        bytes[os2 + 8..os2 + 10].copy_from_slice(&fs_type.to_be_bytes());
        bytes
    }

    /// Plan A8 P9b: a font whose licence forbids embedding is refused for the exports, which embed
    /// it, and a folder says why it was not used.
    #[test]
    fn a_font_that_may_not_be_embedded_is_refused_and_said() {
        assert!(restricted(0x0002) && restricted(0x0102), "Restricted License, whatever the other bits");
        assert!(!restricted(0) && !restricted(0x0004) && !restricted(0x0008), "installable, preview & print, editable");
        assert!(matches!(from_bytes(inter_with_fs_type(2), "Inter"), Err(TextFontError::Restricted(f)) if f == "Inter"));
        // The same font with an embeddable licence goes on to the role check.
        assert!(matches!(from_bytes(inter_with_fs_type(0), "Inter"), Err(TextFontError::NoFigurines)));
        let dir = std::env::temp_dir().join(format!("pdfcraft-restricted-fonts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("locked.ttf"), inter_with_fs_type(2)).unwrap();
        std::fs::write(dir.join("plain.ttf"), inter_with_fs_type(0)).unwrap();
        let found = from_folder(&dir);
        assert!(found.figurines.is_none() && found.signs.is_none());
        assert_eq!(found.errors.len(), 1, "only the restricted one is said: {:?}", found.errors);
        assert!(found.errors[0].to_string().contains("may not be embedded"), "{}", found.errors[0]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The fonts folder is shared with the pieces: a chess diagram font there is passed over
    /// without a word, even one whose licence forbids embedding (no export carries it).
    #[test]
    fn a_diagram_font_beside_the_text_fonts_is_passed_over_quietly() {
        use crate::pieces::test_font::font;
        let adventurer = font("Chess Adventurer", "pnbrqkomvtwl+ ", true, 0x0002);
        assert!(matches!(from_bytes(adventurer.clone(), "x"), Err(TextFontError::NoFigurines)));
        let dir = std::env::temp_dir().join(format!("pdfcraft-diagram-beside-text-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Chess Adventurer.ttf"), adventurer).unwrap();
        std::fs::write(dir.join("HastingsFigurine.ttf"), font("HastingsFigurine", "KQRBN", false, 0)).unwrap();
        let found = from_folder(&dir);
        assert!(found.errors.is_empty(), "{:?}", found.errors);
        assert_eq!(found.figurines.map(|f| (f.role, f.family)), Some((FontRole::Letters, "HastingsFigurine".to_owned())));
        // A figurine font that may not be embedded is still said.
        assert!(matches!(from_bytes(font("HastingsFigurine", "KQRBN", false, 0x0002), "x"), Err(TextFontError::Restricted(_))));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn roles_by_name() {
        assert_eq!(role_by_name("ISChess"), Some(FontRole::Signs));
        assert_eq!(role_by_name("ABCDEF+ISChess-Regular"), Some(FontRole::Signs));
        assert_eq!(role_by_name("SemFig"), Some(FontRole::Letters));
        assert_eq!(role_by_name("SEMFIGN"), Some(FontRole::Letters));
        assert_eq!(role_by_name("ChessMerida"), None, "a diagram font is for the pieces");
        assert_eq!(role_by_name("Inter"), None);
        // A compressed font is known by its file name only.
        assert_eq!(from_bytes(b"wOF2 data".to_vec(), "semfign").unwrap().role, FontRole::Letters);
        assert_eq!(from_bytes(b"wOF2 data".to_vec(), "dejavu").unwrap().role, FontRole::Unicode);
    }

    #[test]
    fn ischess_draws_the_informator_signs() {
        assert_eq!(ischess_letter(14), Some('M'));
        assert_eq!(ischess_letter(16), Some('O'));
        assert_eq!(ischess_letter(19), Some('R'));
        assert_eq!(ischess_letter(41), Some('X'), "Black's attack too");
        assert_eq!(ischess_letter(7), Some('d'));
        // Checked against the font's drawings: zugzwang is the ringed dot, the plain ring space.
        assert_eq!(ischess_letter(23), Some('['));
        assert_eq!(ischess_letter(27), Some('W'));
        assert_eq!(ischess_letter(247), Some('n'), "a bishop sign, written in the font");
        assert_eq!(ischess_letter(1), None, "! is a plain character");
        // Every code is a known annotation, and reading the letter back gives that annotation.
        for (code, letter) in ISCHESS_SIGNS {
            let nag = pdfcraft_chess::nag::by_code(*code).unwrap_or_else(|| panic!("${code}"));
            let read = pdfcraft_chess::catalog::ischess_sign(*letter).unwrap_or_else(|| panic!("{letter}"));
            assert!(nag.glyph == Some(read) || nag.aliases.contains(&read), "${code}: {letter} reads {read}");
        }
    }

    #[test]
    fn faces_by_role() {
        let unicode = font_face(&font(FontRole::Unicode), "u.woff2");
        assert_eq!(
            unicode,
            format!("@font-face{{font-family:\"PdfCraft Figurines\";src:url(\"u.woff2\") format(\"woff2\");unicode-range:{UNICODE_RANGE}}}")
        );
        let letters = font_face(&font(FontRole::Letters), "l.woff2");
        assert_eq!(letters, "@font-face{font-family:\"PdfCraft Figurines\";src:url(\"l.woff2\") format(\"woff2\")}");
        let signs = font_face(&font(FontRole::Signs), "s.woff2");
        assert!(signs.starts_with("@font-face{font-family:\"PdfCraft Signs\""), "{signs}");
        assert!(data_url(&font(FontRole::Signs)).starts_with("data:font/woff2;base64,d09GMi"));
    }

    #[test]
    fn a_folder_gives_one_font_per_role_letters_first() {
        let dir = std::env::temp_dir().join(format!("pdfcraft-text-fonts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(from_folder(&dir).figurines.is_none());
        // Inter (no role) and a text file are passed over; a Unicode font comes first by name, a
        // figurine font by letters takes its place, ISChess is the sign font.
        std::fs::write(dir.join("a-inter.ttf"), include_bytes!("../../../assets/fonts/Inter-Regular.ttf")).unwrap();
        std::fs::write(dir.join("notes.txt"), b"not a font").unwrap();
        std::fs::write(dir.join("b-dejavu.woff2"), b"wOF2 unicode").unwrap();
        std::fs::write(dir.join("c-SemFigN.WOFF2"), b"wOF2 letters").unwrap();
        std::fs::write(dir.join("d-ISChess.woff2"), b"wOF2 signs").unwrap();
        let found = from_folder(&dir);
        let figurines = found.figurines.unwrap();
        assert_eq!((figurines.role, figurines.bytes.as_slice()), (FontRole::Letters, b"wOF2 letters".as_slice()));
        let signs = found.signs.unwrap();
        assert_eq!(signs.role, FontRole::Signs);
        assert_eq!(signs.path.as_deref(), Some(dir.join("d-ISChess.woff2").as_path()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The user's real fonts, when `PDFCRAFT_CHESS_TEST_TEXT_FONT` names them (as in `PATH`):
    /// each must have a role. Skipped otherwise: no such font is committed (decision D4).
    #[test]
    fn real_chess_text_fonts_have_a_role() {
        let Some(paths) = std::env::var_os("PDFCRAFT_CHESS_TEST_TEXT_FONT") else { return };
        for path in std::env::split_paths(&paths) {
            let font = from_file(&path).unwrap();
            eprintln!("{}: {:?} ({})", path.display(), font.role, font.family);
            assert!(data_url(&font).len() > font.bytes.len());
        }
    }
}
