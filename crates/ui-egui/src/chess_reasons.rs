//! The engine's reasons in the interface's language (plan A9-I5, findings A5-13 and A6-08): the
//! engine says what a scan or a reading noted ([`ScanNote`]), why a page's text layer was not
//! believed ([`NeedsOcr`]) and why a diagram could not be read ([`Unreadable`]) as typed values,
//! and this is the one place that says them here. The headless tools keep their English, the
//! types' `Display`.
//!
//! A note is a sentence of its own; a layer's problem and an unreadable diagram are said inside
//! another sentence ("…can't be believed ({e})…", "Couldn't be read: {e}"), so they are
//! fragments. A lower layer's error inside a reason (`error`) is kept as it came.

use pdfcraft_engine::chess::pdfcraft_chessvision::board::Unreadable;
use pdfcraft_engine::chess::pdfcraft_chessvision::chess_text::PoorReason;
use pdfcraft_engine::chess::{LayerProblem, NeedsOcr, ScanNote};

use crate::i18n::fmt;

/// One of the engine's reasons ([`reason_text`]).
#[derive(Clone, Copy, Debug)]
pub(crate) enum Reason<'a> {
    Note(&'a ScanNote),
    Layer(&'a NeedsOcr),
    Unreadable(&'a Unreadable),
}

impl<'a> From<&'a ScanNote> for Reason<'a> {
    fn from(note: &'a ScanNote) -> Self {
        Reason::Note(note)
    }
}

impl<'a> From<&'a NeedsOcr> for Reason<'a> {
    fn from(why: &'a NeedsOcr) -> Self {
        Reason::Layer(why)
    }
}

impl<'a> From<&'a Unreadable> for Reason<'a> {
    fn from(why: &'a Unreadable) -> Self {
        Reason::Unreadable(why)
    }
}

/// `reason` in the interface's language: a sentence for a note, a fragment for a layer's problem
/// and for an unreadable diagram.
pub(crate) fn reason_text<'a>(reason: impl Into<Reason<'a>>) -> String {
    match reason.into() {
        Reason::Note(note) => note_text(note),
        Reason::Layer(why) => {
            let layer = layer_text(&why.layer);
            if why.no_glyph_model { fmt(tl!("{why} (chess OCR needs the chess-glyphs model)"), &[("why", &layer)]) } else { layer }
        }
        Reason::Unreadable(why) => unreadable_text(why),
    }
}

/// A dpi or a share as the engine says it: rounded to a whole number.
fn whole(v: impl Into<f64>) -> String {
    format!("{:.0}", v.into())
}

fn note_text(note: &ScanNote) -> String {
    match note {
        ScanNote::PathsCapped => tl!("The page draws more paths than are looked at; the rest were skipped.").to_owned(),
        ScanNote::StreamsUndecoded { count } => {
            fmt(tl!("Content streams that couldn't be decoded and were skipped: {n}"), &[("n", &count.to_string())])
        }
        ScanNote::PathsUnreadable { error } => fmt(tl!("The page's paths couldn't be read: {e}"), &[("e", error)]),
        ScanNote::NotEditable => tl!("The document can't be read for editing, so its images and paths were not looked at.").to_owned(),
        ScanNote::TextUnreadable { error } => fmt(tl!("The page's text couldn't be read: {e}"), &[("e", error)]),
        ScanNote::ImagesCapped { looked, total } => {
            fmt(tl!("Only the first {n} of the page's {total} images were looked at."), &[("n", &looked.to_string()), ("total", &total.to_string())])
        }
        ScanNote::ImagesUntrimmed { count } => {
            fmt(tl!("Images not trimmed, as the page's image pixels exceed the scan budget: {n}"), &[("n", &count.to_string())])
        }
        ScanNote::BothSidesMate { diagram, n } => {
            fmt(tl!("Diagram {d}: both sides mate in {n}, so who moves stays assumed."), &[("d", &diagram.to_string()), ("n", &n.to_string())])
        }
        ScanNote::ReadDpiCapped { offered, max } => fmt(
            tl!("A diagram offers {offered} dpi; it was read at {max} dpi at most (max_read_dpi)."),
            &[("offered", &whole(*offered)), ("max", &whole(*max))],
        ),
        ScanNote::PageTooLarge { wanted, read } => fmt(
            tl!("The page is too large to render at {wanted} dpi; its diagrams were read at {read} dpi."),
            &[("wanted", &whole(*wanted)), ("read", &whole(*read))],
        ),
        ScanNote::FinerRenderFailed { wanted, read, error } => fmt(
            tl!("The page couldn't be rendered at {wanted} dpi for reading ({e}); it was read at {read} dpi."),
            &[("wanted", &whole(*wanted)), ("read", &whole(*read)), ("e", error)],
        ),
        ScanNote::CorrectionDiverged { diagram, now } => fmt(
            tl!("Diagram {d}: the reading changed since it was corrected (now {fen}); the correction is kept."),
            &[("d", &diagram.to_string()), ("fen", now)],
        ),
        ScanNote::FramesNotLooked { error } => fmt(tl!("The frames around text were not looked for: {e}"), &[("e", error)]),
        ScanNote::BolderLinesNotLooked { error } => fmt(tl!("The lines set bolder were not looked for: {e}"), &[("e", error)]),
        ScanNote::BoldWordsNotLooked { error } => fmt(tl!("The words set in bold were not looked for: {e}"), &[("e", error)]),
        ScanNote::InkSkipped { percent } => fmt(
            tl!("{p}% of the page's height holds too much fine ink to be text (a picture, or a dusty scan); those strips were not read."),
            &[("p", &whole(*percent))],
        ),
        ScanNote::GeneralOcrFailed { error } => fmt(tl!("The general OCR failed ({e}); the glyph reader read the prose too."), &[("e", error)]),
        ScanNote::ChessOcrFailed { error } => fmt(tl!("Chess OCR failed: {e}"), &[("e", error)]),
        ScanNote::OcrRenderFailed { error } => fmt(tl!("The page couldn't be rendered for chess OCR: {e}"), &[("e", error)]),
        ScanNote::OcrNeedsGlyphModel => tl!("Chess OCR was asked for, but it needs the chess-glyphs model.").to_owned(),
        ScanNote::UnknownChessFont { font } => {
            fmt(tl!("The font {font} looks like a chess font but isn't in the catalogue; its text was kept as it extracts."), &[("font", font)])
        }
        ScanNote::NoVisibleArea => tl!("The page has no visible area; it is left out.").to_owned(),
        ScanNote::ParagraphsUnreadable { error } => {
            fmt(tl!("The page's paragraphs couldn't be read ({e}); its lines were grouped by spacing."), &[("e", error)])
        }
        ScanNote::DiagramsUnreadable => tl!("The page's diagrams couldn't be read; they are left out.").to_owned(),
        ScanNote::CorrectionUnused { uid, reason } => fmt(tl!("A correction ({id}) was not used: {e}"), &[("id", uid), ("e", reason)]),
        ScanNote::DescriptiveNotation { examples } => {
            fmt(tl!("Descriptive notation ({examples}) isn't supported; its moves were not read."), &[("examples", &examples.join(", "))])
        }
        ScanNote::TooManyGames { max } => {
            fmt(tl!("The document holds {n} games, the most it keeps: from here on, no game starts."), &[("n", &max.to_string())])
        }
        ScanNote::ScanFailed { error } => fmt(tl!("This page couldn't be scanned: {e}"), &[("e", error)]),
        ScanNote::PageNotRead { error } => fmt(tl!("This page couldn't be read: {e}"), &[("e", error)]),
    }
}

fn layer_text(layer: &LayerProblem) -> String {
    match layer {
        LayerProblem::Missing => tl!("the page has no text layer").to_owned(),
        LayerProblem::Poor(reason) => match reason {
            PoorReason::Type0WithoutUnicode { .. } => tl!("the text layer has two-byte codes with no Unicode text").to_owned(),
            PoorReason::ReplacementChars { .. } => tl!("the text layer extracts as replacement characters").to_owned(),
            PoorReason::DeadMap { .. } => tl!("the text layer's Unicode map is dead").to_owned(),
            PoorReason::UnresolvedChessCodes { glyphs } => {
                fmt(tl!("chess-font glyphs that couldn't be resolved: {n}"), &[("n", &glyphs.to_string())])
            }
            PoorReason::GarbledPieces { words } => fmt(
                tl!("moves with a stray symbol for their piece, where an earlier OCR layer lost the figurines: {n}"),
                &[("n", &words.to_string())],
            ),
            PoorReason::HiddenOcrLayer { .. } => tl!("the text layer is invisible text over an image of the page: another program's OCR").to_owned(),
        },
    }
}

fn unreadable_text(why: &Unreadable) -> String {
    match why {
        Unreadable::NoModel => tl!("no piece model is loaded").to_owned(),
        Unreadable::BadDpi => tl!("the page raster's resolution is not a positive number").to_owned(),
        Unreadable::BadGeometry => tl!("the diagram's box or corners are not finite numbers on this page").to_owned(),
        Unreadable::OffPage => tl!("the diagram lies outside the page raster").to_owned(),
        Unreadable::DegenerateQuad => tl!("the diagram's corners do not form a quadrilateral").to_owned(),
        Unreadable::TooMany => tl!("the page has more diagrams than are read on one page").to_owned(),
        Unreadable::Model(e) => fmt(tl!("the piece model failed: {e}"), &[("e", e)]),
        Unreadable::Probabilities(e) => fmt(tl!("the piece model's output is not a probability matrix: {e}"), &[("e", &e.to_string())]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{Lang, set_current};
    use pdfcraft_engine::chess::pdfcraft_chess::recognize::ProbabilityError;

    /// One value of every variant: the matches have no wildcard, so a variant added to the
    /// engine fails to compile here until it is said (and so translated).
    fn every_reason() -> (Vec<ScanNote>, Vec<NeedsOcr>, Vec<Unreadable>) {
        let e = || "x".to_owned();
        let notes = vec![
            ScanNote::PathsCapped,
            ScanNote::StreamsUndecoded { count: 2 },
            ScanNote::PathsUnreadable { error: e() },
            ScanNote::NotEditable,
            ScanNote::TextUnreadable { error: e() },
            ScanNote::ImagesCapped { looked: 64, total: 70 },
            ScanNote::ImagesUntrimmed { count: 3 },
            ScanNote::BothSidesMate { diagram: 1, n: 2 },
            ScanNote::ReadDpiCapped { offered: 600.0, max: 300.0 },
            ScanNote::PageTooLarge { wanted: 600.0, read: 400.0 },
            ScanNote::FinerRenderFailed { wanted: 600.0, read: 150.0, error: e() },
            ScanNote::CorrectionDiverged { diagram: 1, now: "8/8/8/8/8/8/8/K6k w - - 0 1".into() },
            ScanNote::FramesNotLooked { error: e() },
            ScanNote::BolderLinesNotLooked { error: e() },
            ScanNote::BoldWordsNotLooked { error: e() },
            ScanNote::InkSkipped { percent: 12.4 },
            ScanNote::GeneralOcrFailed { error: e() },
            ScanNote::ChessOcrFailed { error: e() },
            ScanNote::OcrRenderFailed { error: e() },
            ScanNote::OcrNeedsGlyphModel,
            ScanNote::UnknownChessFont { font: "Chess Alpha".into() },
            ScanNote::NoVisibleArea,
            ScanNote::ParagraphsUnreadable { error: e() },
            ScanNote::DiagramsUnreadable,
            ScanNote::CorrectionUnused { uid: "0123abcd".into(), reason: e() },
            ScanNote::DescriptiveNotation { examples: vec!["P-K4".into()] },
            ScanNote::TooManyGames { max: 20_000 },
            ScanNote::ScanFailed { error: e() },
            ScanNote::PageNotRead { error: e() },
        ];
        for n in &notes {
            match n {
                ScanNote::PathsCapped
                | ScanNote::StreamsUndecoded { .. }
                | ScanNote::PathsUnreadable { .. }
                | ScanNote::NotEditable
                | ScanNote::TextUnreadable { .. }
                | ScanNote::ImagesCapped { .. }
                | ScanNote::ImagesUntrimmed { .. }
                | ScanNote::BothSidesMate { .. }
                | ScanNote::ReadDpiCapped { .. }
                | ScanNote::PageTooLarge { .. }
                | ScanNote::FinerRenderFailed { .. }
                | ScanNote::CorrectionDiverged { .. }
                | ScanNote::FramesNotLooked { .. }
                | ScanNote::BolderLinesNotLooked { .. }
                | ScanNote::BoldWordsNotLooked { .. }
                | ScanNote::InkSkipped { .. }
                | ScanNote::GeneralOcrFailed { .. }
                | ScanNote::ChessOcrFailed { .. }
                | ScanNote::OcrRenderFailed { .. }
                | ScanNote::OcrNeedsGlyphModel
                | ScanNote::UnknownChessFont { .. }
                | ScanNote::NoVisibleArea
                | ScanNote::ParagraphsUnreadable { .. }
                | ScanNote::DiagramsUnreadable
                | ScanNote::CorrectionUnused { .. }
                | ScanNote::DescriptiveNotation { .. }
                | ScanNote::TooManyGames { .. }
                | ScanNote::ScanFailed { .. }
                | ScanNote::PageNotRead { .. } => {}
            }
        }
        let poor = [
            PoorReason::Type0WithoutUnicode { ratio: 0.9 },
            PoorReason::ReplacementChars { ratio: 0.9 },
            PoorReason::DeadMap { ratio: 0.9 },
            PoorReason::UnresolvedChessCodes { glyphs: 4 },
            PoorReason::GarbledPieces { words: 4 },
            PoorReason::HiddenOcrLayer { ratio: 1.0 },
        ];
        for p in &poor {
            match p {
                PoorReason::Type0WithoutUnicode { .. }
                | PoorReason::ReplacementChars { .. }
                | PoorReason::DeadMap { .. }
                | PoorReason::UnresolvedChessCodes { .. }
                | PoorReason::GarbledPieces { .. }
                | PoorReason::HiddenOcrLayer { .. } => {}
            }
        }
        let mut layers: Vec<NeedsOcr> = std::iter::once(LayerProblem::Missing)
            .chain(poor.into_iter().map(LayerProblem::Poor))
            .map(|layer| NeedsOcr { layer, no_glyph_model: false })
            .collect();
        layers.push(NeedsOcr { layer: LayerProblem::Missing, no_glyph_model: true });
        let unreadable = vec![
            Unreadable::NoModel,
            Unreadable::BadDpi,
            Unreadable::BadGeometry,
            Unreadable::OffPage,
            Unreadable::DegenerateQuad,
            Unreadable::TooMany,
            Unreadable::Model(e()),
            Unreadable::Probabilities(ProbabilityError::ZeroRow(3)),
        ];
        for u in &unreadable {
            match u {
                Unreadable::NoModel
                | Unreadable::BadDpi
                | Unreadable::BadGeometry
                | Unreadable::OffPage
                | Unreadable::DegenerateQuad
                | Unreadable::TooMany
                | Unreadable::Model(_)
                | Unreadable::Probabilities(_) => {}
            }
        }
        (notes, layers, unreadable)
    }

    fn said(lang: Lang) -> Vec<String> {
        let previous = crate::i18n::current();
        set_current(lang);
        let (notes, layers, unreadable) = every_reason();
        let out = notes.iter().map(reason_text).chain(layers.iter().map(reason_text)).chain(unreadable.iter().map(reason_text)).collect();
        set_current(previous);
        out
    }

    /// Plan A9-I5 (A5-13, A6-08): every reason of the engine is said in Brazilian Portuguese, not
    /// in the engine's English ("…desta página (the page has no text layer)…" before).
    #[test]
    fn every_engine_reason_is_said_in_the_interface_language() {
        let english = said(Lang::EN);
        let pt = said(Lang::from_code("pt-br").expect("pt-br registered"));
        let untranslated: Vec<&String> = english.iter().zip(&pt).filter(|(en, pt)| en == pt).map(|(en, _)| en).collect();
        assert!(untranslated.is_empty(), "said in English in pt-BR: {untranslated:#?}");
        // The data are in the sentence: the count, the dpi, the font, a lower layer's error.
        assert!(pt.iter().any(|s| s.contains("Chess Alpha")) && pt.iter().any(|s| s.contains("600")), "{pt:#?}");
    }

    /// The English the interface says names what the engine's note names (a sample).
    #[test]
    fn the_english_says_the_data() {
        let previous = crate::i18n::current();
        set_current(Lang::EN);
        assert_eq!(reason_text(&ScanNote::ImagesCapped { looked: 64, total: 70 }), "Only the first 64 of the page's 70 images were looked at.");
        assert_eq!(
            reason_text(&NeedsOcr { layer: LayerProblem::Missing, no_glyph_model: true }),
            "the page has no text layer (chess OCR needs the chess-glyphs model)"
        );
        assert_eq!(reason_text(&Unreadable::Model("bad input".into())), "the piece model failed: bad input");
        set_current(previous);
    }
}
