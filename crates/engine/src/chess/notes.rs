//! What a scan or a reading of pages says it skipped, approximated or could not do (plan A9-I5,
//! findings A5-13 and A6-08): typed, so the interface says it in its own language and the
//! headless tools keep the English sentence, which is the [`std::fmt::Display`] of each type.
//!
//! The English here is the tools' contract (`notes`, `needs_ocr[].reason`): each variant's text
//! is the sentence the engine wrote before it was typed. A variant's data is what the sentence
//! says; an `error` is a lower layer's message, kept as it came (in English).

use std::fmt;

use pdfcraft_chessvision::chess_text::{PoorReason, TextQuality};

/// One thing a scan or a reading of a page noted, so a partial result is never silent.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ScanNote {
    /// The page draws more paths than the scan looks at.
    PathsCapped,
    /// Content streams that could not be decoded.
    StreamsUndecoded { count: usize },
    /// The page's paths could not be listed.
    PathsUnreadable { error: String },
    /// The document has no editable object graph, so its images and paths were not looked at.
    NotEditable,
    /// The page's text could not be extracted.
    TextUnreadable { error: String },
    /// The page draws more images than the scan looks at.
    ImagesCapped { looked: usize, total: usize },
    /// Images left untrimmed: their pixels exceed the scan's budget.
    ImagesUntrimmed { count: usize },
    /// Diagram `diagram` (from 1) is mate in `n` for both sides; its side to move stays assumed.
    BothSidesMate { diagram: usize, n: u8 },
    /// A diagram offers `offered` dpi, more than the `max` it is read at.
    ReadDpiCapped { offered: f32, max: f32 },
    /// The page is too large to render at `wanted` dpi; its diagrams were read at `read` dpi.
    PageTooLarge { wanted: f32, read: f32 },
    /// The page could not be rendered at `wanted` dpi for reading; read at `read` dpi.
    FinerRenderFailed { wanted: f32, read: f32, error: String },
    /// Diagram `diagram` (from 1) reads otherwise since it was corrected (`now`: its FEN, or
    /// "unread"); the correction is kept.
    CorrectionDiverged { diagram: usize, now: String },
    /// A layout hint of a page read by OCR: the frames around text could not be looked for.
    FramesNotLooked { error: String },
    /// A layout hint: the lines set bolder could not be looked for.
    BolderLinesNotLooked { error: String },
    /// A layout hint: the words set in bold could not be looked for.
    BoldWordsNotLooked { error: String },
    /// `percent` of the page's height holds too much fine ink to be text; those strips were not
    /// read.
    InkSkipped { percent: f64 },
    /// The general OCR failed; the glyph reader read the prose too.
    GeneralOcrFailed { error: String },
    /// Chess OCR failed on the page.
    ChessOcrFailed { error: String },
    /// The page could not be rendered for chess OCR.
    OcrRenderFailed { error: String },
    /// Chess OCR was asked for, and the chess-glyphs model is not loaded.
    OcrNeedsGlyphModel,
    /// A font that looks like a chess font is not in the catalogue; its text is kept as it
    /// extracts.
    UnknownChessFont { font: String },
    /// The page has no visible area; it is left out.
    NoVisibleArea,
    /// The page's paragraphs could not be read; its lines were grouped by spacing.
    ParagraphsUnreadable { error: String },
    /// The page's diagrams could not be read; they are left out.
    DiagramsUnreadable,
    /// A correction (the first 8 characters of its uid) was not used, and why (the corrections'
    /// own wording).
    CorrectionUnused { uid: String, reason: String },
    /// The page is in descriptive notation, which is not read; `examples` are its words.
    DescriptiveNotation { examples: Vec<String> },
    /// The document holds `max` games, the most it keeps: from here on no game starts.
    TooManyGames { max: usize },
    /// The page could not be scanned (`error`: why, as the scan said it).
    ScanFailed { error: String },
    /// The page could not be read at all (`error`: why, as the reading said it).
    PageNotRead { error: String },
}

impl fmt::Display for ScanNote {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScanNote::PathsCapped => f.write_str("the page draws more paths than are looked at; the rest were skipped"),
            ScanNote::StreamsUndecoded { count } => write!(f, "{count} content stream(s) could not be decoded and were skipped"),
            ScanNote::PathsUnreadable { error } => write!(f, "the page's paths could not be read: {error}"),
            ScanNote::NotEditable => f.write_str("the document can't be read for editing, so its images and paths were not looked at"),
            ScanNote::TextUnreadable { error } => write!(f, "the page's text could not be read: {error}"),
            ScanNote::ImagesCapped { looked, total } => write!(f, "only the first {looked} of {total} images were looked at"),
            ScanNote::ImagesUntrimmed { count } => {
                write!(f, "{count} image(s) were not trimmed: the page's image pixels exceed the scan budget")
            }
            ScanNote::BothSidesMate { diagram, n } => write!(f, "diagram {diagram}: both sides mate in {n}; the side to move stays assumed"),
            ScanNote::ReadDpiCapped { offered, max } => write!(f, "a diagram offers {offered:.0} dpi; read at most at {max:.0} dpi (max_read_dpi)"),
            ScanNote::PageTooLarge { wanted, read } => {
                write!(f, "the page is too large to render at {wanted:.0} dpi; diagrams were read at {read:.0} dpi")
            }
            ScanNote::FinerRenderFailed { wanted, read, error } => {
                write!(f, "the page could not be rendered at {wanted:.0} dpi for reading ({error}); read at {read:.0} dpi")
            }
            ScanNote::CorrectionDiverged { diagram, now } => {
                write!(f, "diagram {diagram}: the reading changed since it was corrected (now {now}); the correction is kept")
            }
            ScanNote::FramesNotLooked { error } => write!(f, "the frames around text were not looked for: {error}"),
            ScanNote::BolderLinesNotLooked { error } => write!(f, "the lines set bolder were not looked for: {error}"),
            ScanNote::BoldWordsNotLooked { error } => write!(f, "the words set in bold were not looked for: {error}"),
            ScanNote::InkSkipped { percent } => write!(
                f,
                "{percent:.0}% of the page's height holds too much fine ink to be text (a picture, or a dusty scan): those strips were not read"
            ),
            ScanNote::GeneralOcrFailed { error } => write!(f, "the general OCR failed ({error}); the glyph reader read the prose too"),
            ScanNote::ChessOcrFailed { error } => write!(f, "chess OCR failed: {error}"),
            ScanNote::OcrRenderFailed { error } => write!(f, "the page could not be rendered for chess OCR: {error}"),
            ScanNote::OcrNeedsGlyphModel => f.write_str("chess OCR was asked for, but it needs the chess-glyphs model"),
            ScanNote::UnknownChessFont { font } => {
                write!(f, "the font {font} looks like a chess font but is not in the catalogue; its text was kept as it extracts")
            }
            ScanNote::NoVisibleArea => f.write_str("the page has no visible area; it is left out"),
            ScanNote::ParagraphsUnreadable { error } => {
                write!(f, "the page's paragraphs could not be read ({error}); lines were grouped by spacing")
            }
            ScanNote::DiagramsUnreadable => f.write_str("the page's diagrams could not be read; they are left out"),
            ScanNote::CorrectionUnused { uid, reason } => write!(f, "a correction ({uid}) was not used: {reason}"),
            ScanNote::DescriptiveNotation { examples } => {
                write!(f, "descriptive notation ({}): not supported; its moves were not read", examples.join(", "))
            }
            ScanNote::TooManyGames { max } => write!(f, "the document holds {max} games, the most it keeps: from here on, no game starts"),
            ScanNote::ScanFailed { error } | ScanNote::PageNotRead { error } => f.write_str(error),
        }
    }
}

/// What is wrong with a page's text layer, when something is.
#[derive(Clone, Debug, PartialEq)]
pub enum LayerProblem {
    /// The page has no text layer: a scan.
    Missing,
    /// The layer exists but cannot be trusted, and why.
    Poor(PoorReason),
}

impl LayerProblem {
    /// The problem `quality` says the layer has; `None` for a good layer.
    pub fn of(quality: &TextQuality) -> Option<LayerProblem> {
        match quality {
            TextQuality::Good => None,
            TextQuality::Missing => Some(LayerProblem::Missing),
            TextQuality::Poor { reason } => Some(LayerProblem::Poor(reason.clone())),
        }
    }
}

impl fmt::Display for LayerProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LayerProblem::Missing => f.write_str("the page has no text layer"),
            LayerProblem::Poor(reason) => match reason {
                PoorReason::Type0WithoutUnicode { .. } => f.write_str("the text layer has two-byte codes with no Unicode text"),
                PoorReason::ReplacementChars { .. } => f.write_str("the text layer extracts as replacement characters"),
                PoorReason::DeadMap { .. } => f.write_str("the text layer's Unicode map is dead"),
                PoorReason::UnresolvedChessCodes { glyphs } => write!(f, "{glyphs} chess-font glyphs could not be resolved"),
                PoorReason::GarbledPieces { words } => {
                    write!(f, "{words} moves have a stray symbol for their piece: an earlier OCR layer lost the figurines")
                }
                PoorReason::HiddenOcrLayer { .. } => f.write_str("the text layer is invisible text over an image of the page: another program's OCR"),
            },
        }
    }
}

/// Why a page's text layer was not believed and chess OCR did not read the page instead
/// ([`super::ChessDocument::needs_ocr`]).
#[derive(Clone, Debug, PartialEq)]
pub struct NeedsOcr {
    /// What is wrong with the layer.
    pub layer: LayerProblem,
    /// Chess OCR would have read the page, but the chess-glyphs model is not loaded.
    pub no_glyph_model: bool,
}

impl fmt::Display for NeedsOcr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.layer)?;
        if self.no_glyph_model {
            f.write_str(" (chess OCR needs the chess-glyphs model)")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The English the tools wrote before the notes were typed, kept word for word (a sample of
    /// each shape: no data, a count, an error, a list, a float).
    #[test]
    fn the_english_is_the_sentence_the_tools_wrote() {
        assert_eq!(ScanNote::PathsCapped.to_string(), "the page draws more paths than are looked at; the rest were skipped");
        assert_eq!(ScanNote::StreamsUndecoded { count: 2 }.to_string(), "2 content stream(s) could not be decoded and were skipped");
        assert_eq!(ScanNote::ChessOcrFailed { error: "x".into() }.to_string(), "chess OCR failed: x");
        assert_eq!(
            ScanNote::DescriptiveNotation { examples: vec!["P-K4".into(), "N-KB3".into()] }.to_string(),
            "descriptive notation (P-K4, N-KB3): not supported; its moves were not read"
        );
        assert_eq!(
            ScanNote::InkSkipped { percent: 12.4 }.to_string(),
            "12% of the page's height holds too much fine ink to be text (a picture, or a dusty scan): those strips were not read"
        );
        assert_eq!(ScanNote::ScanFailed { error: "the page could not be read".into() }.to_string(), "the page could not be read");
        let poor = NeedsOcr { layer: LayerProblem::Poor(PoorReason::UnresolvedChessCodes { glyphs: 3 }), no_glyph_model: true };
        assert_eq!(poor.to_string(), "3 chess-font glyphs could not be resolved (chess OCR needs the chess-glyphs model)");
        assert_eq!(NeedsOcr { layer: LayerProblem::Missing, no_glyph_model: false }.to_string(), "the page has no text layer");
        assert_eq!(LayerProblem::of(&TextQuality::Good), None);
    }
}
