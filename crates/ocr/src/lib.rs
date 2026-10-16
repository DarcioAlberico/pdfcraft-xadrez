//! pdfcraft-ocr — Scan & OCR ▸ Recognize text (L4).
//!
//! Recognition runs the ocrs engine (MIT/Apache-2.0) with its pre-trained models (CC-BY-SA-4.0,
//! fetched by `cargo xtask models`; see ATTRIBUTION.toml). The caller renders a page to pixels;
//! [`Ocr::recognize`] finds the words in them, and [`write_text_layer`] turns words placed in user
//! space into page content: invisible text (rendering mode 3) over each word, so the page becomes a
//! searchable image (Acrobat's "Searchable Image (Exact)": the image is left untouched). The text
//! is Helvetica when WinAnsiEncoding holds it, else a Type0 font with `ToUnicode` from craft-fonts
//! (chess figurines and signs included); a build without craft-fonts writes figurines as SAN
//! letters and reports it ([`TextLayer::degraded`]).
//!
//! The models read the Latin alphabet (English and other languages written without accents).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use pdfcraft_cos::{Dict, Document};
use pdfcraft_fonts::embedding;
pub use pdfcraft_fonts::helvetica_width;

/// The model files, as named in ATTRIBUTION.toml.
pub const DETECTION_MODEL: &str = "text-detection.rten";
pub const RECOGNITION_MODEL: &str = "text-recognition.rten";

/// The recognition model's alphabet, as it was trained (ocrs-models' `DEFAULT_ALPHABET`): its
/// output classes follow this order. The ocrs crate's own copy has an `E` where the training had
/// `€`, which made a scanned "€250" read as "E250".
pub const ALPHABET: &str = " 0123456789!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~€ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// The languages the models read (ISO 639-1); all use the Latin alphabet without accents.
pub const LANGUAGES: &[(&str, &str)] = &[("en", "English")];

#[derive(Debug, thiserror::Error)]
pub enum OcrError {
    /// Shown to end users (release packages ship the models; a source build fetches them with
    /// `cargo xtask models`). The UI catalogs key this exact text.
    #[error("the text recognition models are not installed (reinstall PdfCraft, or set PDFCRAFT_MODELS to the folder that holds them)")]
    NoModels,
    #[error("loading {0}: {1}")]
    Load(String, String),
    #[error("text recognition failed: {0}")]
    Recognize(String),
    #[error("the image is empty")]
    EmptyImage,
    /// The text layer's font could not be written.
    #[error("writing the text layer's font: {0}")]
    Font(String),
}

/// Where the two model files are.
#[derive(Clone, Debug, PartialEq)]
pub struct Models {
    pub detection: PathBuf,
    pub recognition: PathBuf,
}

impl Models {
    /// The models in `dir`, if both files are there.
    pub fn in_dir(dir: &Path) -> Option<Models> {
        let m = Models { detection: dir.join(DETECTION_MODEL), recognition: dir.join(RECOGNITION_MODEL) };
        (m.detection.is_file() && m.recognition.is_file()).then_some(m)
    }

    /// Look for the models: `$PDFCRAFT_MODELS`, then where the release packages install them
    /// beside the executable ([`Models::dirs_beside_exe`]), then the source tree's `assets/models/`.
    pub fn find() -> Option<Models> {
        Self::search_dirs().iter().find_map(|d| Self::in_dir(d))
    }

    /// The directories [`Models::find`] looks in, in order.
    pub fn search_dirs() -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        if let Some(d) = std::env::var_os("PDFCRAFT_MODELS") {
            dirs.push(PathBuf::from(d));
        }
        if let Some(exe) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
            dirs.extend(Self::dirs_beside_exe(&exe));
        }
        dirs.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/models"));
        dirs
    }

    /// Where the release packages put the models, relative to the directory holding the
    /// executable (packaging/*):
    /// - `models/` beside it: the Windows MSI and portable zip;
    /// - `../Resources/models`: the macOS app bundle (`Contents/MacOS` → `Contents/Resources`);
    /// - `../share/pdfcraft/models`: the FHS-style trees (`bin/` → `share/`) of the Linux
    ///   deb/rpm/tar.gz/AppImage/Flatpak and the FreeBSD tarball, wherever they are installed.
    pub fn dirs_beside_exe(exe_dir: &Path) -> Vec<PathBuf> {
        vec![exe_dir.join("models"), exe_dir.join("../Resources/models"), exe_dir.join("../share/pdfcraft/models")]
    }
}

/// A recognised word: its text and its box in image pixels `[left, top, right, bottom]`.
#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub text: String,
    pub rect: [f32; 4],
}

/// A recognised line of words, in reading order.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Line {
    pub words: Vec<Word>,
}

impl Line {
    pub fn text(&self) -> String {
        self.words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ")
    }
}

/// A loaded recogniser. Loading takes a moment; keep one and reuse it.
pub struct Ocr {
    engine: ocrs::OcrEngine,
}

impl Ocr {
    pub fn load(models: &Models) -> Result<Ocr, OcrError> {
        let load = |p: &Path| rten::Model::load_file(p).map_err(|e| OcrError::Load(p.display().to_string(), e.to_string()));
        let params = ocrs::OcrEngineParams {
            detection_model: Some(load(&models.detection)?),
            recognition_model: Some(load(&models.recognition)?),
            alphabet: Some(ALPHABET.into()),
            ..Default::default()
        };
        let engine = ocrs::OcrEngine::new(params).map_err(|e| OcrError::Load("ocr engine".into(), e.to_string()))?;
        Ok(Ocr { engine })
    }

    /// Load the models found by [`Models::find`].
    pub fn find() -> Result<Ocr, OcrError> {
        Self::load(&Models::find().ok_or(OcrError::NoModels)?)
    }

    /// Recognise the text in an RGBA (or RGB, or grey) image, `width` × `height` pixels.
    pub fn recognize(&self, pixels: &[u8], width: u32, height: u32) -> Result<Vec<Line>, OcrError> {
        if width == 0 || height == 0 || pixels.is_empty() {
            return Err(OcrError::EmptyImage);
        }
        // ocrs wants 1 or 3 channels.
        let rgb: std::borrow::Cow<[u8]> = if pixels.len() == (width * height * 4) as usize {
            pixels.as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]).collect::<Vec<u8>>().into()
        } else {
            pixels.into()
        };
        let err = |e: &dyn std::fmt::Display| OcrError::Recognize(e.to_string());
        let source = ocrs::ImageSource::from_bytes(&rgb, (width, height)).map_err(|e| err(&e))?;
        let input = self.engine.prepare_input(source).map_err(|e| err(&e))?;
        let found = self.engine.detect_words(&input).map_err(|e| err(&e))?;
        let lines = self.engine.find_text_lines(&input, &found);
        let read = self.engine.recognize_text(&input, &lines).map_err(|e| err(&e))?;
        use ocrs::TextItem;
        Ok(read
            .into_iter()
            .flatten()
            .map(|line| Line {
                words: line
                    .words()
                    .map(|w| {
                        let r = w.bounding_rect();
                        Word { text: w.to_string(), rect: [r.left() as f32, r.top() as f32, r.right() as f32, r.bottom() as f32] }
                    })
                    .filter(|w| !w.text.trim().is_empty())
                    .collect(),
            })
            .filter(|l| !l.words.is_empty())
            .collect())
    }
}

/// A word placed on a page, in PDF user space: the bottom-left corner of its box, and the
/// vectors along its bottom edge (`across`, left → right as read) and its left edge (`up`).
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedWord {
    pub text: String,
    pub origin: [f64; 2],
    pub across: [f64; 2],
    pub up: [f64; 2],
}

impl PlacedWord {
    /// Place a pixel-space word with `to_user`, which maps an image pixel (x right, y down) to
    /// user space.
    pub fn place(word: &Word, to_user: impl Fn(f32, f32) -> [f64; 2]) -> PlacedWord {
        let [l, t, r, b] = word.rect;
        let o = to_user(l, b);
        let br = to_user(r, b);
        let tl = to_user(l, t);
        PlacedWord { text: word.text.clone(), origin: o, across: [br[0] - o[0], br[1] - o[1]], up: [tl[0] - o[0], tl[1] - o[1]] }
    }
}

/// Helvetica's ascender and descender (em fractions): a word box spans about this much.
const BOX_EM: f64 = 0.93;
const DESCENT: f64 = 0.21;
/// Helvetica's cap height and x-height (em fractions).
const CAP_HEIGHT: f64 = 0.718;
const X_HEIGHT: f64 = 0.523;

/// How far `text`'s ink reaches above and below the baseline, in em fractions: a word box is
/// tight around the ink, so "DT" spans the cap height only, "sun" the x-height, and "gap" reaches
/// down to the descender. Text without letters or digits keeps the full ascender-descender span.
fn ink_extent(text: &str) -> (f64, f64) {
    if !text.chars().any(char::is_alphanumeric) {
        return (BOX_EM - DESCENT, -DESCENT);
    }
    let tall = text
        .chars()
        .any(|c| c.is_uppercase() || c.is_ascii_digit() || "bdfhijklt!?/\\()[]{}|'\"#$%&@".contains(c) || (c.is_alphabetic() && !c.is_ascii()));
    let low = text.chars().any(|c| "gjpqy()[]{}|_".contains(c));
    (if tall { CAP_HEIGHT } else { X_HEIGHT }, if low { -DESCENT } else { 0.0 })
}

fn num(v: f64) -> String {
    let s = format!("{:.4}", if v.abs() < 1e-9 { 0.0 } else { v });
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

/// The text matrix that fits text `width` em wide (at size 1) to `w`'s box, or `None` for a box
/// that can't hold text (no height, or a corner that is not a finite number).
fn fit(w: &PlacedWord, width: f64) -> Option<[f64; 6]> {
    let height = w.up[0].hypot(w.up[1]);
    if height.is_nan() || height <= 0.0 {
        return None;
    }
    // Characters without an advance (combining marks alone) are still written, unstretched.
    let width = if width > 0.0 && width.is_finite() { width } else { 1.0 };
    // The em square: the word's own ink (cap height or x-height, descenders or not) spans the
    // box, so the text is as tall as the scanned word; its width is stretched to fit the word.
    let (top, bottom) = ink_extent(&w.text);
    let em = [w.up[0] / (top - bottom), w.up[1] / (top - bottom)];
    let ax = [w.across[0] / width, w.across[1] / width];
    let o = [w.origin[0] - em[0] * bottom, w.origin[1] - em[1] * bottom];
    let m = [ax[0], ax[1], em[0], em[1], o[0], o[1]];
    m.iter().all(|v| v.is_finite()).then_some(m)
}

/// One word of the layer: its matrix, then the string that shows it.
fn show(out: &mut Vec<u8>, m: [f64; 6], string: &[u8]) {
    let nums: Vec<String> = m.iter().map(|v| num(*v)).collect();
    out.extend_from_slice(nums.join(" ").as_bytes());
    out.extend_from_slice(b" Tm ");
    out.extend_from_slice(string);
    out.extend_from_slice(b" Tj\n");
}

/// A character the text layer could not write as recognised, and what it wrote instead.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Degraded {
    pub ch: char,
    pub written: String,
}

impl std::fmt::Display for Degraded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "\"{}\" (U+{:04X}) was written as \"{}\"", self.ch.escape_debug(), u32::from(self.ch), self.written)
    }
}

/// What the Helvetica layer writes for characters WinAnsiEncoding lacks: a chess figurine
/// becomes its piece letter, as in SAN, and an evaluation sign the ASCII form chess books use
/// (which `pdfcraft-chess` reads back as the sign). Anything else becomes `?`.
const WIN_ANSI_FORMS: &[(char, &str)] = &[
    ('♔', "K"),
    ('♕', "Q"),
    ('♖', "R"),
    ('♗', "B"),
    ('♘', "N"),
    ('♙', "P"),
    ('♚', "K"),
    ('♛', "Q"),
    ('♜', "R"),
    ('♝', "B"),
    ('♞', "N"),
    ('♟', "P"),
    ('⩲', "+/="),
    ('⩱', "=/+"),
    ('∓', "-/+"),
    ('−', "-"),
];

/// `text` with every character WinAnsiEncoding lacks replaced (see [`WIN_ANSI_FORMS`]); each
/// replacement is recorded once in `degraded`.
fn win_ansi_form(text: &str, degraded: &mut std::collections::BTreeMap<char, &'static str>) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if pdfcraft_fonts::win_ansi_encodable(c) {
            out.push(c);
            continue;
        }
        let written = WIN_ANSI_FORMS.iter().find(|(f, _)| *f == c).map_or("?", |(_, w)| *w);
        out.push_str(written);
        degraded.entry(c).or_insert(written);
    }
    out
}

/// Recognised text and the content that writes it on a page.
#[derive(Clone, Debug, PartialEq)]
pub struct TextLayer {
    pub content: Vec<u8>,
    /// What the layer could not write as recognised (empty when the text is written in full).
    pub degraded: Vec<Degraded>,
}

/// Page content that writes `words` as invisible text in `/PCHelv` (standard Helvetica,
/// WinAnsiEncoding), each word stretched to its box so selection and search highlight the
/// right place. Marked content `/OCR` so it can be told apart from the page's own text.
/// Characters the encoding lacks are replaced and listed in [`TextLayer::degraded`]: chess
/// figurines become SAN letters (♘ → N); [`write_text_layer`] keeps them when it can.
pub fn text_layer(words: &[PlacedWord]) -> TextLayer {
    let mut degraded = std::collections::BTreeMap::new();
    let mut out = b"/OCR BMC\nBT\n3 Tr\n/PCHelv 1 Tf\n".to_vec();
    for w in words {
        let text = win_ansi_form(&w.text, &mut degraded);
        if text.is_empty() {
            continue;
        }
        if let Some(m) = fit(w, helvetica_width(&text, 1.0)) {
            show(&mut out, m, &pdfcraft_fonts::literal(&pdfcraft_fonts::win_ansi(&text)));
        }
    }
    out.extend_from_slice(b"ET\nEMC\n");
    TextLayer { content: out, degraded: degraded.into_iter().map(|(ch, w)| Degraded { ch, written: w.into() }).collect() }
}

/// What [`write_text_layer`] would write differently from `words` in this build: nothing when
/// the Unicode layer's face is there or the text fits WinAnsiEncoding, else the replacements
/// [`text_layer`] makes.
pub fn degraded(words: &[PlacedWord]) -> Vec<Degraded> {
    if !needs_unicode(words) || layer_face().is_some() {
        return Vec::new();
    }
    let mut degraded = std::collections::BTreeMap::new();
    for w in words {
        win_ansi_form(&w.text, &mut degraded);
    }
    degraded.into_iter().map(|(ch, w)| Degraded { ch, written: w.into() }).collect()
}

fn needs_unicode(words: &[PlacedWord]) -> bool {
    words.iter().any(|w| w.text.chars().any(|c| !pdfcraft_fonts::win_ansi_encodable(c)))
}

/// Characters a chess book's text uses beyond Latin: the faces are ranked by how many of them
/// they have (the layer is invisible, so this only gives these characters their own widths).
const CHESS_SIGNS: &str = "♔♕♖♗♘♙♚♛♜♝♞♟⩲⩱±∓∞−→↑⇆⨀⨁⟳∆△□⌓∇≤©";

/// The craft-fonts face the Unicode layer is written in, if this build has one: a static face
/// with every printable ASCII character (so Latin text gets proportional widths), Regular first,
/// then the one with most [`CHESS_SIGNS`], then the smallest program. With the craft-fonts of
/// 2026-10 that is BIZ UDPGothic Regular, which is also in the web build. No face has the
/// figurines; the layer is invisible, so they are written with a stand-in glyph and their own
/// code, which `ToUnicode` maps back to them.
pub fn layer_face() -> Option<&'static pdfcraft_fonts::CraftFont> {
    static FACE: std::sync::OnceLock<Option<&'static pdfcraft_fonts::CraftFont>> = std::sync::OnceLock::new();
    *FACE.get_or_init(|| {
        let ascii: String = (' '..='~').collect();
        let signs = |f: &pdfcraft_fonts::CraftFont| CHESS_SIGNS.chars().filter(|c| embedding::covers(f.bytes, &c.to_string())).count();
        pdfcraft_fonts::CRAFT_FONTS
            .iter()
            .filter(|f| embedding::validate(f.bytes).is_ok() && embedding::covers(f.bytes, &ascii))
            .min_by_key(|f| (f.style != "Regular", std::cmp::Reverse(signs(f)), f.bytes.len()))
    })
}

/// Write `words` as the page's invisible text layer. Text WinAnsiEncoding can hold is written in
/// Helvetica ([`text_layer`]), as before. Text with other characters (chess figurines, ⩲, CJK) is
/// written in full with [`unicode_text_layer`] in the [`layer_face`]; a build without one writes
/// the Helvetica layer and lists what it replaced in [`TextLayer::degraded`]. Fonts are added to
/// `fonts`, the page's font resources, and their programs to `doc`.
pub fn write_text_layer(doc: &mut Document, fonts: &mut Dict, words: &[PlacedWord]) -> Result<TextLayer, OcrError> {
    match layer_face().filter(|_| needs_unicode(words)) {
        Some(face) => Ok(TextLayer { content: unicode_text_layer(doc, fonts, words, face.bytes, face.family)?, degraded: Vec::new() }),
        None => Ok(text_layer(words)),
    }
}

/// Glyphs written for characters the face lacks, first one it has: the layer is invisible, so
/// the glyph only lends its width; the character itself comes back through `ToUnicode`.
const STAND_INS: [char; 3] = ['□', '?', 'x'];

/// At most this many different characters in one layer: a CID for each, from 1 (Identity-H
/// codes are two bytes; `embed_mapping` keeps one more for itself).
const MAX_CODES: usize = 65_534;

/// Page content that writes `words` as invisible text in a Type0 font (CIDFontType2, Identity-H)
/// made from `face`, added to `fonts`: one code per distinct character, with a `ToUnicode` entry
/// for each, so search, selection and copy give back exactly the recognised text, figurines,
/// signs, combining marks and characters beyond U+FFFF included. Characters `face` lacks are
/// drawn with a stand-in glyph (never `.notdef`, which PDF/A forbids even for invisible text).
/// Each word is stretched to its box as in [`text_layer`]. The program is embedded whole, once
/// per document (`pdfcraft_fonts::embedding::embed_mapping`).
pub fn unicode_text_layer(doc: &mut Document, fonts: &mut Dict, words: &[PlacedWord], face: &[u8], family: &str) -> Result<Vec<u8>, OcrError> {
    use skrifa::{
        FontRef, MetadataProvider,
        instance::{LocationRef, Size},
    };
    let font = FontRef::new(face).map_err(|e| OcrError::Font(e.to_string()))?;
    let upm = f64::from(font.metrics(Size::unscaled(), LocationRef::default()).units_per_em);
    if upm <= 0.0 {
        return Err(OcrError::Font("the font has no units per em".into()));
    }
    let advances = font.glyph_metrics(Size::unscaled(), LocationRef::default());
    let charmap = font.charmap();
    let glyph = |c: char| charmap.map(c).filter(|g| g.to_u32() != 0);
    let stand_in = STAND_INS.iter().find_map(|c| glyph(*c)).ok_or_else(|| OcrError::Font("the font has no stand-in glyph".into()))?;
    // `embed_mapping` numbers the CIDs from 1 in mapping order.
    let mut codes: std::collections::HashMap<char, (u16, f64)> = std::collections::HashMap::new();
    let mut mapping = Vec::new();
    for c in words.iter().flat_map(|w| w.text.chars()) {
        if codes.contains_key(&c) {
            continue;
        }
        if mapping.len() >= MAX_CODES {
            return Err(OcrError::Font(format!("the text has more than {MAX_CODES} different characters")));
        }
        let cid = u16::try_from(mapping.len().saturating_add(1)).map_err(|_| OcrError::Font("too many characters".into()))?;
        let g = glyph(c).unwrap_or(stand_in);
        let advance = f64::from(advances.advance_width(g).unwrap_or(0.0)) / upm;
        codes.insert(c, (cid, advance));
        mapping.push((c.to_string(), g));
    }
    let mut out = b"/OCR BMC\nBT\n3 Tr\n".to_vec();
    if !mapping.is_empty() {
        let resource = embedding::embed_mapping(doc, fonts, face, family, &mapping).map_err(OcrError::Font)?;
        out.extend_from_slice(format!("/{resource} 1 Tf\n").as_bytes());
    }
    for w in words.iter().filter(|w| !w.text.is_empty()) {
        let shown: Vec<(u16, f64)> = w.text.chars().filter_map(|c| codes.get(&c).copied()).collect();
        let Some(m) = fit(w, shown.iter().map(|(_, a)| a).sum()) else { continue };
        let mut hex = String::with_capacity(shown.len().saturating_mul(4).saturating_add(2));
        hex.push('<');
        for (cid, _) in &shown {
            let _ = write!(hex, "{cid:04X}");
        }
        hex.push('>');
        show(&mut out, m, hex.as_bytes());
    }
    out.extend_from_slice(b"ET\nEMC\n");
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn places_words_through_the_mapping() {
        // 2 pixels per point, page 612 × 792, y flipped.
        let to_user = |x: f32, y: f32| [x as f64 / 2.0, 792.0 - y as f64 / 2.0];
        let w = PlacedWord::place(&Word { text: "Hello".into(), rect: [100.0, 200.0, 300.0, 240.0] }, to_user);
        assert_eq!(w.origin, [50.0, 672.0]);
        assert_eq!(w.across, [100.0, 0.0]);
        assert_eq!(w.up, [0.0, 20.0]);
    }

    /// The text layer is Helvetica in WinAnsiEncoding: every character the model can recognise
    /// must be one it can hold, so ordinary OCR never embeds a font. Other characters (a wider
    /// alphabet, chess figurines) go in the Unicode layer, which needs craft-fonts.
    #[test]
    fn the_model_alphabet_fits_the_text_layer_font() {
        assert_eq!(pdfcraft_fonts::first_undrawable(ALPHABET), None);
    }

    #[test]
    fn text_layer_is_invisible_text_fitted_to_each_box() {
        let w = PlacedWord { text: "Hi".into(), origin: [10.0, 20.0], across: [30.0, 0.0], up: [0.0, 9.3] };
        let s = String::from_utf8(text_layer(&[w]).content).unwrap();
        assert!(s.contains("3 Tr") && s.contains("/PCHelv 1 Tf"), "{s}");
        let width = helvetica_width("Hi", 1.0);
        // "Hi" has no descender: its box spans the cap height, from the baseline up.
        assert!(s.contains(&format!("{} 0 0 {} 10 20 Tm (Hi) Tj", num(30.0 / width), num(9.3 / CAP_HEIGHT))), "{s}");
        assert!(s.starts_with("/OCR BMC") && s.ends_with("EMC\n"));
    }

    /// A word box is tight around the ink, so the text's size and baseline come from the letters:
    /// capitals, lowercase without ascenders, descenders.
    #[test]
    fn the_text_is_as_tall_as_the_scanned_word() {
        assert_eq!(ink_extent("DT"), (CAP_HEIGHT, 0.0));
        assert_eq!(ink_extent("402337"), (CAP_HEIGHT, 0.0));
        assert_eq!(ink_extent("sun"), (X_HEIGHT, 0.0));
        assert_eq!(ink_extent("gap"), (X_HEIGHT, -DESCENT));
        assert_eq!(ink_extent("Gallus.Ze@camtel.cm"), (CAP_HEIGHT, 0.0));
        assert_eq!(ink_extent("Corporation"), (CAP_HEIGHT, -DESCENT));
        assert_eq!(ink_extent("--"), (BOX_EM - DESCENT, -DESCENT));
        let w = PlacedWord { text: "gap".into(), origin: [0.0, 100.0], across: [20.0, 0.0], up: [0.0, 7.3] };
        let s = String::from_utf8(text_layer(&[w]).content).unwrap();
        let em = 7.3 / (X_HEIGHT + DESCENT);
        assert!(s.contains(&format!(" 0 0 {} 0 {} Tm (gap) Tj", num(em), num(100.0 + em * DESCENT))), "{s}");
    }

    #[test]
    fn rotated_pages_keep_the_reading_direction() {
        // A page turned 90°: image x runs up the page.
        let to_user = |x: f32, y: f32| [y as f64, x as f64];
        let w = PlacedWord::place(&Word { text: "Up".into(), rect: [0.0, 0.0, 50.0, 10.0] }, to_user);
        assert_eq!(w.across, [0.0, 50.0]);
        assert_eq!(w.up, [-10.0, 0.0]);
    }

    /// #103: every release package's layout puts the models where the installed app looks.
    /// Mirrors packaging/{windows/package.ps1,macos/package.sh,linux/package.sh,freebsd/package.sh}.
    #[test]
    fn finds_the_models_in_every_release_package_layout() {
        let base = std::env::temp_dir().join(format!("pdfcraft-ocr-layouts-{}", std::process::id()));
        let layouts = [
            ("windows", "PdfCraft", "PdfCraft/models"),
            ("macos", "PdfCraft.app/Contents/MacOS", "PdfCraft.app/Contents/Resources/models"),
            ("linux-deb-rpm", "usr/bin", "usr/share/pdfcraft/models"),
            ("linux-tar", "pdfcraft-1.0.0-linux-x86_64/bin", "pdfcraft-1.0.0-linux-x86_64/share/pdfcraft/models"),
            ("appimage", "PdfCraft.AppDir/usr/bin", "PdfCraft.AppDir/usr/share/pdfcraft/models"),
            ("flatpak", "app/bin", "app/share/pdfcraft/models"),
            ("freebsd", "usr/local/bin", "usr/local/share/pdfcraft/models"),
        ];
        for (name, exe_dir, models_dir) in layouts {
            let root = base.join(name);
            let (exe_dir, models_dir) = (root.join(exe_dir), root.join(models_dir));
            std::fs::create_dir_all(&exe_dir).unwrap();
            std::fs::create_dir_all(&models_dir).unwrap();
            let found = || Models::dirs_beside_exe(&exe_dir).iter().find_map(|d| Models::in_dir(d));
            assert_eq!(found(), None, "{name}: no models yet");
            for file in [DETECTION_MODEL, RECOGNITION_MODEL] {
                std::fs::write(models_dir.join(file), b"model").unwrap();
            }
            let models = found().unwrap_or_else(|| panic!("{name}: models in {} not found", models_dir.display()));
            assert_eq!(models.detection.canonicalize().unwrap(), models_dir.join(DETECTION_MODEL).canonicalize().unwrap(), "{name}");
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    /// The message tells end users what to do; release builds have no cargo.
    #[test]
    fn the_alphabet_is_the_trained_one() {
        let chars: Vec<char> = ALPHABET.chars().collect();
        assert_eq!(chars.len(), 96);
        // The euro sign follows the ASCII punctuation (class 44; 0 is the CTC blank), not a second E.
        assert_eq!(chars.iter().position(|&c| c == '€'), Some(43));
        assert_eq!(chars.iter().filter(|&&c| c == 'E').count(), 1);
    }

    #[test]
    fn missing_models_message_is_for_end_users() {
        let message = OcrError::NoModels.to_string();
        assert!(!message.contains("cargo") && message.contains("PDFCRAFT_MODELS"), "{message}");
    }

    /// End to end on rendered text, when the models are installed (`cargo xtask models`).
    #[test]
    fn reads_rendered_text() {
        let Some(models) = Models::find() else {
            eprintln!("skipped: OCR models not installed");
            return;
        };
        let ocr = Ocr::load(&models).unwrap();
        let (w, h, px) = test_image::hello();
        let lines = ocr.recognize(&px, w, h).unwrap();
        let text: Vec<String> = lines.iter().map(Line::text).collect();
        assert!(text.iter().any(|t| t.contains("HELL") && t.contains("WOR")), "{text:?}");
        let word = lines.iter().flat_map(|l| &l.words).find(|w| w.text.contains("HELL")).unwrap();
        assert!(word.rect[0] >= 10.0 && word.rect[0] < 60.0, "{word:?}");
    }

    fn word(text: &str, x: f64) -> PlacedWord {
        PlacedWord { text: text.into(), origin: [x, 100.0], across: [30.0, 0.0], up: [0.0, 9.0] }
    }

    /// Without the Unicode layer's face, figurines are written as SAN letters and signs in their
    /// ASCII forms, and every replacement is reported: nothing is lost silently.
    #[test]
    fn the_helvetica_layer_writes_figurines_as_san_letters_and_says_so() {
        let words = [word("1.♘f3", 10.0), word("♞c6", 50.0), word("⩲", 90.0), word("日", 120.0), word("€250", 150.0)];
        let layer = text_layer(&words);
        let s = String::from_utf8_lossy(&layer.content);
        for shown in ["(1.Nf3) Tj", "(Nc6) Tj", "(+/=) Tj", "(?) Tj"] {
            assert!(s.contains(shown), "{shown} in {s}");
        }
        let written: Vec<(char, &str)> = layer.degraded.iter().map(|d| (d.ch, d.written.as_str())).collect();
        assert_eq!(written, [('♘', "N"), ('♞', "N"), ('⩲', "+/="), ('日', "?")]);
        assert_eq!(layer.degraded[0].to_string(), "\"♘\" (U+2658) was written as \"N\"");
        assert!(text_layer(&[word("Nf3+", 0.0)]).degraded.is_empty(), "WinAnsi text is written as it is");
        // `degraded` predicts what this build writes: the replacements, unless it has a face.
        let predicted = degraded(&words);
        assert_eq!(predicted.is_empty(), layer_face().is_some(), "{predicted:?}");
        assert!(degraded(&[word("Nf3", 0.0)]).is_empty());
    }

    /// The shown strings of a text layer's content, in order (`<hex> Tj` or `(literal) Tj`).
    fn shown_hex(content: &[u8]) -> Vec<Vec<u8>> {
        content
            .split(|b| *b == b'\n')
            .filter_map(|line| line.strip_suffix(b" Tj"))
            .filter_map(|line| {
                let start = line.iter().rposition(|b| *b == b'<')?;
                let hex = line.get(start + 1..line.len() - 1)?;
                hex.chunks(2).map(|p| u8::from_str_radix(std::str::from_utf8(p).ok()?, 16).ok()).collect()
            })
            .collect()
    }

    const PAGE: pdfcraft_cos::ObjRef = pdfcraft_cos::ObjRef::new(3, 0);

    /// A one-page file, the Unicode layer written on its page in Inter, saved incrementally and
    /// opened again: the reopened file and the font dictionary its page names.
    fn saved_layer(words: &[PlacedWord]) -> (Document, Dict, Vec<u8>) {
        use pdfcraft_cos::{Object, Stream};
        let original = pdfcraft_testkit::Pdf::new()
            .obj("<< /Type /Catalog /Pages 2 0 R >>")
            .obj("<< /Type /Pages /Kids [3 0 R] /Count 1 >>")
            .obj("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] >>")
            .build();
        let mut doc = Document::open(std::sync::Arc::new(original.clone())).unwrap();
        let mut fonts = Dict::new();
        let content = unicode_text_layer(&mut doc, &mut fonts, words, pdfcraft_fonts::INTER_REGULAR, "Inter").unwrap();
        let stream = doc.add(Object::Stream(Stream::from_raw(Dict::new(), content)));
        let mut resources = Dict::new();
        resources.set(b"Font".to_vec(), Object::Dict(fonts));
        doc.update_dict(PAGE, |d| {
            d.set(b"Resources".to_vec(), Object::Dict(resources));
            d.set(b"Contents".to_vec(), Object::Ref(stream));
        })
        .unwrap();
        let saved = pdfcraft_cos::write_incremental(&doc, &pdfcraft_cos::SaveOptions::default()).unwrap();
        assert!(saved.starts_with(&original), "the save is incremental");
        let reopened = Document::open(std::sync::Arc::new(saved)).unwrap();
        let page = reopened.dict(&Object::Ref(PAGE)).unwrap();
        let resources = reopened.dict(page.get(b"Resources").unwrap()).unwrap();
        let fonts = reopened.dict(resources.get(b"Font").unwrap()).unwrap();
        let font = fonts.iter().find(|(k, _)| k.starts_with(b"PCFull")).map(|(_, f)| reopened.dict(f).unwrap()).unwrap_or_default();
        let content = match reopened.resolve(page.get(b"Contents").unwrap()).as_ref() {
            Object::Stream(s) => s.decoded().unwrap(),
            other => panic!("{other:?}"),
        };
        (reopened, font, content)
    }

    /// The Unicode layer (any face; Inter here, so the test runs without craft-fonts): a Type0
    /// font, Identity-H, a `ToUnicode` entry per character, still invisible, and every word's
    /// text read back exactly after a save and reopen, figurines included.
    #[test]
    fn the_unicode_layer_gives_back_figurines_and_signs_after_a_save() {
        let words = [word("1.♘f3", 10.0), word("⩲", 50.0), word("♔♕♖♗♙♚♛♜♝♞♟", 90.0), word("e4!?", 150.0)];
        let (doc, font, content) = saved_layer(&words);
        let s = String::from_utf8_lossy(&content);
        assert!(s.starts_with("/OCR BMC\nBT\n3 Tr\n/PCFull0 1 Tf\n") && s.ends_with("ET\nEMC\n"), "{s}");
        assert_eq!(font.name(b"Subtype"), Some(&b"Type0"[..]));
        assert_eq!(font.name(b"Encoding"), Some(&b"Identity-H"[..]));
        assert!(font.get(b"ToUnicode").is_some());
        let cid = doc.dict(doc.resolve(font.get(b"DescendantFonts").unwrap()).as_array().unwrap().first().unwrap()).unwrap();
        assert_eq!(cid.name(b"Subtype"), Some(&b"CIDFontType2"[..]));
        let m = pdfcraft_fonts::pdf::Metrics::from_dict(&doc, &font);
        let read: Vec<String> = shown_hex(&content).iter().map(|h| m.decode(h)).collect();
        assert_eq!(read, words.iter().map(|w| w.text.clone()).collect::<Vec<_>>());
        // Each word is stretched to its box: its codes' advances times the matrix's x scale.
        for (w, h) in words.iter().zip(shown_hex(&content)) {
            let width: f64 = m.codes(&h).iter().map(|(c, _)| m.width(*c)).sum();
            assert!(width > 0.0, "{}: the stand-in glyph has an advance", w.text);
        }
    }

    /// The face is chosen from craft-fonts when the build has them, and its absence is no error.
    #[test]
    fn the_layer_face_is_a_static_face_with_latin_when_there_is_one() {
        let Some(face) = layer_face() else {
            eprintln!("skipped: built without craft-fonts (CRAFT_FONTS_DIR)");
            return;
        };
        assert!(embedding::validate(face.bytes).is_ok() && embedding::covers(face.bytes, "Nf3 +/= e4!?"));
        assert_eq!(face.style, "Regular");
    }

    /// Latin text keeps the Helvetica layer (no font program is embedded for it); text with a
    /// figurine gets the Unicode layer when the build has a face, else the reported fallback.
    #[test]
    fn write_text_layer_embeds_a_font_only_for_text_helvetica_lacks() {
        let mut doc = Document::new_empty();
        let objects = doc.object_numbers().len();
        let mut fonts = Dict::new();
        let latin = write_text_layer(&mut doc, &mut fonts, &[word("Nf3", 0.0)]).unwrap();
        assert!(String::from_utf8_lossy(&latin.content).contains("/PCHelv 1 Tf") && latin.degraded.is_empty());
        assert!(fonts.iter().next().is_none() && doc.object_numbers().len() == objects, "nothing embedded for Latin text");
        let chess = write_text_layer(&mut doc, &mut fonts, &[word("♘f3", 0.0)]).unwrap();
        let s = String::from_utf8_lossy(&chess.content);
        match layer_face() {
            Some(_) => assert!(s.contains("/PCFull0 1 Tf") && chess.degraded.is_empty() && fonts.contains(b"PCFull0"), "{s}"),
            None => assert!(s.contains("(Nf3) Tj") && chess.degraded.len() == 1 && fonts.iter().next().is_none(), "{s}"),
        }
    }

    fn any_text() -> impl proptest::strategy::Strategy<Value = String> {
        use proptest::prelude::*;
        let ch = prop_oneof![
            any::<char>(),
            Just('\u{FFFD}'),
            Just('\u{0301}'),
            Just('\0'),
            Just('\n'),
            Just('\u{FFFF}'),
            Just('♘'),
            Just('⩲'),
            Just('\u{1F600}'),
            Just('\u{10FFFF}'),
            Just(')'),
            Just('\\'),
            Just('>'),
        ];
        proptest::collection::vec(ch, 0..10).prop_map(|c| c.into_iter().collect())
    }

    fn any_coordinate() -> impl proptest::strategy::Strategy<Value = f64> {
        use proptest::prelude::*;
        prop_oneof![8 => -1e4..1e4f64, 1 => Just(f64::NAN), 1 => Just(f64::INFINITY), 1 => Just(0.0)]
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(64))]
        /// Any recognised text, in any box: no panic, a file that saves and reopens, and every
        /// word that has a box gives back exactly its text, controls, combining marks, U+FFFD and
        /// characters beyond U+FFFF included. The Helvetica layer takes the same input.
        #[test]
        fn any_text_makes_a_layer_that_reopens(
            words in proptest::collection::vec((any_text(), any_coordinate(), any_coordinate(), any_coordinate()), 0..6)
        ) {
            let words: Vec<PlacedWord> =
                words.into_iter().map(|(text, x, w, h)| PlacedWord { text, origin: [x, 50.0], across: [w, 0.0], up: [0.0, h] }).collect();
            let helvetica = text_layer(&words);
            proptest::prop_assert!(helvetica.content.ends_with(b"ET\nEMC\n"));
            let (doc, font, content) = saved_layer(&words);
            let m = pdfcraft_fonts::pdf::Metrics::from_dict(&doc, &font);
            let read: Vec<String> = shown_hex(&content).iter().map(|h| m.decode(h)).collect();
            let expected: Vec<String> = words.iter().filter(|w| !w.text.is_empty() && fit(w, 1.0).is_some()).map(|w| w.text.clone()).collect();
            proptest::prop_assert_eq!(read, expected);
        }
    }

    mod test_image {
        /// "HELLO WORLD" drawn with blocky 5×7 letters, 4 pixels per dot, black on white.
        pub fn hello() -> (u32, u32, Vec<u8>) {
            const GLYPHS: &[(char, [&str; 7])] = &[
                ('H', ["X...X", "X...X", "X...X", "XXXXX", "X...X", "X...X", "X...X"]),
                ('E', ["XXXXX", "X....", "X....", "XXXX.", "X....", "X....", "XXXXX"]),
                ('L', ["X....", "X....", "X....", "X....", "X....", "X....", "XXXXX"]),
                ('O', [".XXX.", "X...X", "X...X", "X...X", "X...X", "X...X", ".XXX."]),
                ('W', ["X...X", "X...X", "X...X", "X.X.X", "X.X.X", "XX.XX", "X...X"]),
                ('R', ["XXXX.", "X...X", "X...X", "XXXX.", "X.X..", "X..X.", "X...X"]),
                ('D', ["XXXX.", "X...X", "X...X", "X...X", "X...X", "X...X", "XXXX."]),
            ];
            let text = "HELLO WORLD";
            let (dot, x0, y0) = (4u32, 40u32, 40u32);
            let (w, h) = (x0 * 2 + text.len() as u32 * 6 * dot, y0 * 2 + 7 * dot);
            let mut px = vec![255u8; (w * h * 4) as usize];
            for (i, c) in text.chars().enumerate() {
                let Some((_, rows)) = GLYPHS.iter().find(|g| g.0 == c) else { continue };
                for (ry, row) in rows.iter().enumerate() {
                    for (rx, b) in row.bytes().enumerate() {
                        if b != b'X' {
                            continue;
                        }
                        for dy in 0..dot {
                            for dx in 0..dot {
                                let x = x0 + (i as u32 * 6 + rx as u32) * dot + dx;
                                let y = y0 + ry as u32 * dot + dy;
                                let o = ((y * w + x) * 4) as usize;
                                px[o..o + 3].copy_from_slice(&[0, 0, 0]);
                            }
                        }
                    }
                }
            }
            (w, h, px)
        }
    }
}
