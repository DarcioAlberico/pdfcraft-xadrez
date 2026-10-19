//! Whether a page's text layer can be believed, or the page needs OCR.
//!
//! A text layer can be confidently wrong: a subset font with a missing or empty `ToUnicode` map
//! draws correctly on screen and extracts as garbage. The checks run in a fixed order and the
//! first that fails names the reason, so the same page always gets the same verdict:
//!
//! 1. **Missing**: not one visible glyph. The page is a scan.
//! 2. **Type0 without usable Unicode**: more than [`MAX_BAD_RATIO`] of the glyphs extract as
//!    nothing usable, and at least half of those were shown with two-byte codes. Those codes are glyph
//!    indices of a Type0 (CID) font, which mean nothing without a `ToUnicode` map.
//! 3. **Replacement characters** or a **dead map**: the same share, of U+FFFD, or of empty,
//!    U+0000, control and private-use text.
//! 4. **Unresolved chess codes**: at least [`MIN_UNRESOLVED`] glyphs are set in a chess font
//!    the catalogue does not know, or stand where a piece belongs under a figurine-font code that
//!    is not a piece letter. Their letters would be read as the wrong pieces.
//! 5. **Hidden OCR layer**: more than [`MAX_HIDDEN_RATIO`] of the glyphs are drawn invisibly
//!    (`3 Tr`) on a page that is a picture. That is the layer FineReader, Acrobat or Tesseract
//!    write over a scan, and a general OCR guesses at figurines and annotation symbols. The text
//!    layer cannot tell whether images cover the page, so this check is the caller's
//!    ([`over_page_image`]), made once it knows.
//!
//! Glyphs of chess diagram fonts are left out of the ratios: they are a position, which the
//! diagram detector reads from their codes, not language.

/// Above this share of unusable glyphs the layer is poor (the reference measured 15% as the
/// limit of "implausible characters" before a page falls through to OCR).
pub const MAX_BAD_RATIO: f32 = 0.15;

/// This many unresolved chess glyphs make the layer poor. One stray glyph in an unknown chess
/// font is not worth an OCR pass; a few mean a whole font's moves are unreadable.
pub const MIN_UNRESOLVED: usize = 3;

/// Above this share of invisible glyphs, a layer over a picture of the page is another program's
/// OCR: "mostly invisible". Born-digital text has none; a scan's OCR layer has nothing else.
pub const MAX_HIDDEN_RATIO: f32 = 0.5;

/// The verdict on one page's text layer.
#[derive(Clone, Debug, PartialEq)]
pub enum TextQuality {
    /// The layer can be read as it is.
    Good,
    /// The layer exists but cannot be trusted; read the page with OCR instead.
    Poor { reason: PoorReason },
    /// The page has no text layer: it is a scan.
    Missing,
}

/// Why a text layer was judged poor. Ratios are shares (0 to 1) of the page's judged glyphs.
#[derive(Clone, Debug, PartialEq)]
pub enum PoorReason {
    /// Two-byte codes (a Type0 font) with no usable Unicode text.
    Type0WithoutUnicode { ratio: f32 },
    /// Glyphs that extract as U+FFFD.
    ReplacementChars { ratio: f32 },
    /// Glyphs whose Unicode map is dead: empty, U+0000, control or private-use text.
    DeadMap { ratio: f32 },
    /// Glyphs in chess fonts whose codes could not be resolved to what they draw.
    UnresolvedChessCodes { glyphs: usize },
    /// Glyphs drawn invisibly over a picture of the page: another program's OCR of a scan.
    HiddenOcrLayer { ratio: f32 },
}

/// Counts gathered while mapping a page's glyphs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct GlyphStats {
    /// Visible glyphs of any font, diagram fonts included.
    pub visible: usize,
    /// Visible glyphs outside diagram fonts: the ones judged.
    pub judged: usize,
    /// Judged glyphs whose text is U+FFFD.
    pub replacement: usize,
    /// Judged glyphs whose text is dead (empty, U+0000, control, private use).
    pub dead: usize,
    /// Of `replacement + dead`, those shown with a two-byte code.
    pub wide_bad: usize,
    /// Chess-font glyphs whose meaning is unknown.
    pub unresolved: usize,
    /// Judged glyphs drawn invisibly (`3 Tr`).
    pub invisible: usize,
}

fn ratio(part: usize, whole: usize) -> f32 {
    if whole == 0 { 0.0 } else { (part as f32 / whole as f32).clamp(0.0, 1.0) }
}

/// The verdict for a page with these counts.
pub(crate) fn verdict(stats: &GlyphStats) -> TextQuality {
    if stats.visible == 0 {
        return TextQuality::Missing;
    }
    let bad = stats.replacement.saturating_add(stats.dead);
    let bad_ratio = ratio(bad, stats.judged);
    if bad_ratio > MAX_BAD_RATIO {
        let reason = if stats.wide_bad.saturating_mul(2) >= bad {
            PoorReason::Type0WithoutUnicode { ratio: ratio(stats.wide_bad, stats.judged) }
        } else if stats.replacement >= stats.dead {
            PoorReason::ReplacementChars { ratio: ratio(stats.replacement, stats.judged) }
        } else {
            PoorReason::DeadMap { ratio: ratio(stats.dead, stats.judged) }
        };
        return TextQuality::Poor { reason };
    }
    if stats.unresolved >= MIN_UNRESOLVED {
        return TextQuality::Poor { reason: PoorReason::UnresolvedChessCodes { glyphs: stats.unresolved } };
    }
    TextQuality::Good
}

/// The share of the judged glyphs drawn invisibly.
pub(crate) fn hidden_ratio(stats: &GlyphStats) -> f32 {
    ratio(stats.invisible, stats.judged)
}

/// The verdict on a page that is a picture (images cover most of it), from the verdict on its
/// text layer alone and the share `hidden` of the layer's judged glyphs drawn invisibly. A layer
/// already poor or missing keeps its reason, as the checks before this one come first.
pub fn over_page_image(quality: &TextQuality, hidden: f32) -> TextQuality {
    match quality {
        TextQuality::Good if hidden > MAX_HIDDEN_RATIO => TextQuality::Poor { reason: PoorReason::HiddenOcrLayer { ratio: hidden } },
        other => other.clone(),
    }
}
