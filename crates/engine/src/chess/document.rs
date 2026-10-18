//! Chess ▸ Recognised document (blueprint S14b): a document's pages read into one
//! [`ChessDoc`] — headings, paragraphs of chess text and diagrams in reading order, with the
//! games replayed from the diagrams before them — ready for the PDF-and-text view and the exports.
//!
//! Per page, the engine gathers what `chessvision::doc` builds from:
//!
//! - **diagrams**: the page's scan with readings ([`Session::chess_scan`], cached), in the one
//!   reading order. A diagram the user's model could not read (or that needs a model that is
//!   missing) is still a diagram block, without a position;
//! - **text**: the page's text layer read as chess text (`chess_text::from_page_text`). The
//!   piece-letter language is chosen once for the whole document from every page's evidence
//!   (`LangTally`), unless the caller names one. A layer the reader cannot believe (no text, dead
//!   maps, Type0 without Unicode) is reported in [`ChessDocument::needs_ocr`]: chess OCR of the
//!   page image is not wired into the engine yet;
//! - **headings and paragraphs**: the page's reading blocks (`edit::reading_blocks`) as
//!   [`BlockHint`]s. A block set at least [`HEADING_RATIO`] times the body size (the size most of
//!   the page's characters use) and at most [`HEADING_MAX_CHARS`] long is a heading: level 1 at
//!   [`TITLE_RATIO`] or more, else level 2. Bold alone is not a heading, as chess books set main
//!   lines in bold.
//!
//! The text repair of S13c (`BookCipher`, legality repair) works on OCR'd lines, so it has no
//! input here yet and no review is passed.
//!
//! Read-only: nothing here changes a document.

use pdfcraft_chess::san::Lang;
use pdfcraft_chessvision::chess_text::{self, ChessPageText, LangGuess, LangTally, TextQuality};
use pdfcraft_chessvision::coords::PageRect;
use pdfcraft_chessvision::doc::{BlockHint, ChessDoc, DocBuilder, HintKind, PageInput};
use pdfcraft_render::{PageRenderer, RenderConfig, RenderRequest, RequestKind};

use super::ChessScanSettings;
use crate::{DocId, Session};

/// A block at least this many times the body size is a heading.
pub const HEADING_RATIO: f64 = 1.15;
/// A heading at least this many times the body size is level 1.
pub const TITLE_RATIO: f64 = 1.5;
/// A longer block is a paragraph whatever its size (a pull quote, an introduction set large).
pub const HEADING_MAX_CHARS: usize = 160;

/// What [`Session::chess_document`] read.
#[derive(Clone, Debug)]
pub struct ChessDocument {
    pub doc: ChessDoc,
    /// The piece-letter language the moves were read in, and how sure the choice is.
    pub lang: LangGuess,
    /// Whether `lang` was the caller's (`true`) or chosen from the pages' letters.
    pub lang_given: bool,
    /// Pages (0-based) whose text layer is poor or missing, with the reason: their text needs
    /// OCR, which this does not run.
    pub needs_ocr: Vec<(usize, String)>,
    /// What the scan and the text reading noted, page by page (0-based), never silent.
    pub notes: Vec<(usize, String)>,
}

/// The heading level of a block of `size` with `chars` characters on a page whose body text is
/// `body`, or `None` for a paragraph.
pub fn heading_level(size: f64, chars: usize, body: f64) -> Option<u8> {
    if !(size.is_finite() && body.is_finite() && body > 0.0) || chars == 0 || chars > HEADING_MAX_CHARS {
        return None;
    }
    let ratio = size / body;
    if ratio >= TITLE_RATIO {
        Some(1)
    } else if ratio >= HEADING_RATIO {
        Some(2)
    } else {
        None
    }
}

/// The size most of the blocks' characters are set in (to the half point), or `None` for no text.
fn body_size(blocks: &[pdfcraft_edit::TextBlock]) -> Option<f64> {
    let mut sizes: Vec<(i64, usize)> = Vec::new();
    for b in blocks {
        if !(b.size.is_finite() && b.size > 0.0 && b.size < 10_000.0) {
            continue;
        }
        // Half points: 10 and 10.2 pt are the same body; the cast is bounded by the check above.
        let key = (b.size * 2.0).round() as i64;
        let chars = b.text.chars().filter(|c| !c.is_whitespace()).count();
        match sizes.iter_mut().find(|(k, _)| *k == key) {
            Some((_, n)) => *n = n.saturating_add(chars),
            None => sizes.push((key, chars)),
        }
    }
    // Ties go to the smaller size: body text is rarely the larger of two equal amounts.
    sizes.iter().max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0))).map(|(k, _)| *k as f64 / 2.0)
}

/// The page's reading blocks as hints: headings by size, the rest paragraphs.
pub fn block_hints(blocks: &[pdfcraft_edit::TextBlock]) -> Vec<BlockHint> {
    let Some(body) = body_size(blocks) else { return Vec::new() };
    blocks
        .iter()
        .filter_map(|b| {
            let [x0, y0, x1, y1] = b.rect.map(|v| v as f32);
            let rect = PageRect::new(x0, y0, x1, y1);
            if ![rect.x0, rect.y0, rect.x1, rect.y1].iter().all(|v| v.is_finite()) {
                return None;
            }
            let chars = b.text.chars().filter(|c| !c.is_whitespace()).count();
            let kind = match heading_level(b.size, chars, body) {
                Some(level) => HintKind::Heading { level },
                None => HintKind::Paragraph,
            };
            Some(BlockHint { kind, rect })
        })
        .collect()
}

fn poor_reason(quality: &TextQuality) -> Option<String> {
    use pdfcraft_chessvision::chess_text::PoorReason;
    match quality {
        TextQuality::Good => None,
        TextQuality::Missing => Some("the page has no text layer".into()),
        TextQuality::Poor { reason } => Some(match reason {
            PoorReason::Type0WithoutUnicode { .. } => "the text layer has two-byte codes with no Unicode text".into(),
            PoorReason::ReplacementChars { .. } => "the text layer extracts as replacement characters".into(),
            PoorReason::DeadMap { .. } => "the text layer's Unicode map is dead".into(),
            PoorReason::UnresolvedChessCodes { glyphs } => format!("{glyphs} chess-font glyphs could not be resolved"),
        }),
    }
}

impl Session {
    /// Chess ▸ Recognised document: read `pages` (0-based; empty = all) of document `id` into a
    /// [`ChessDoc`], scanning with `settings` (diagrams are read with the user's models when they
    /// load). `lang`: the piece letters' language, or `None` to choose it from the pages. Fails
    /// when the document does not exist, a page is out of range, or its permissions do not allow
    /// copying its content.
    pub fn chess_document(&mut self, id: DocId, pages: &[usize], settings: &ChessScanSettings, lang: Option<Lang>) -> Result<ChessDocument, String> {
        let scanned = self.chess_scan(id, pages, settings, true)?;
        let doc = self.get(id).ok_or("no such document")?;
        let config = RenderConfig { password: doc.password.as_deref().map(std::sync::Arc::from), ..Default::default() };
        let mut renderer = PageRenderer::new(doc.bytes.clone(), config);
        let mut notes: Vec<(usize, String)> = Vec::new();

        // The text layers, then one language for the whole document from all of their letters.
        let mut texts = Vec::with_capacity(scanned.len());
        for page in &scanned {
            let r = renderer.render(RenderRequest { page: page.page, kind: RequestKind::Text, scale: 1.0, ..Default::default() });
            if let Some(e) = &r.error {
                notes.push((page.page, format!("the page's text could not be read: {e}")));
            }
            texts.push(r.text);
        }
        let read = |hint: Option<Lang>| -> Vec<ChessPageText> {
            texts
                .iter()
                .map(|t| {
                    t.as_deref().map(|t| chess_text::from_page_text(t, hint)).unwrap_or_else(|| chess_text::from_page_text(&Default::default(), hint))
                })
                .collect()
        };
        let first = read(lang);
        let (chosen, chess_texts) = match lang {
            Some(l) => (LangGuess { lang: l, confidence: 1.0 }, first),
            None => {
                let mut tally = LangTally::default();
                for t in &first {
                    tally.merge(&t.lang_tally);
                }
                let guess = tally.guess(None);
                // Read again only when some page's own guess differs from the document's.
                let texts = if first.iter().all(|t| t.lang.lang == guess.lang) { first } else { read(Some(guess.lang)) };
                (guess, texts)
            }
        };

        let mut needs_ocr = Vec::new();
        let mut builder = DocBuilder::new();
        for (page, text) in scanned.iter().zip(&chess_texts) {
            notes.extend(page.notes.iter().map(|n| (page.page, n.clone())));
            if let Some(e) = &page.error {
                notes.push((page.page, e.clone()));
            }
            if let Some(why) = poor_reason(&text.quality) {
                needs_ocr.push((page.page, why));
            }
            for font in &text.unknown_chess_fonts {
                notes.push((
                    page.page,
                    format!("the font {font} looks like a chess font but is not in the catalogue; its text was kept as it extracts"),
                ));
            }
            let Some(geometry) = page.geometry else {
                notes.push((page.page, "the page has no visible area; it is left out".into()));
                continue;
            };
            let hints = match doc.editor.as_ref().map(|e| pdfcraft_edit::reading_blocks(&e.cos, page.page)) {
                Some(Ok(blocks)) => block_hints(&blocks),
                Some(Err(e)) => {
                    notes.push((page.page, format!("the page's paragraphs could not be read ({e}); lines were grouped by spacing")));
                    Vec::new()
                }
                None => Vec::new(),
            };
            let diagrams = page.readings.as_deref().unwrap_or_default();
            if page.readings.is_none() && !page.diagrams.is_empty() {
                notes.push((page.page, "the page's diagrams could not be read; they are left out".into()));
            }
            let index = u32::try_from(page.page).unwrap_or(u32::MAX);
            builder.add_page(&PageInput { index, geometry, text, diagrams, hints: &hints, reviews: &[] });
        }
        Ok(ChessDocument { doc: builder.finish(), lang: chosen, lang_given: lang.is_some(), needs_ocr, notes })
    }
}
