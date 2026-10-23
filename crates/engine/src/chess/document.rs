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
//!   maps, Type0 without Unicode, or another program's OCR drawn invisibly over a picture of the
//!   page) is read with chess OCR as [`OcrMode`] says, else reported in
//!   [`ChessDocument::needs_ocr`]. [`BookOrigin`] is the user's say over a whole book;
//! - **headings and paragraphs**: the page's reading blocks (`edit::reading_blocks`) as
//!   [`BlockHint`]s. A block set at least [`HEADING_RATIO`] times the body size (the size most of
//!   the characters of all the pages read use: a book keeps one body size, while a page that
//!   opens a game can hold more of its smaller introduction than of the game) and at most
//!   [`HEADING_MAX_CHARS`] long is a heading: level 1 at [`TITLE_RATIO`] or more, else level 2.
//!   Bold alone is not a heading, as chess books set main lines in bold.
//!
//! The text repair of S13c (`BookCipher`, legality repair) works on OCR'd lines, so it has no
//! input here yet and no review is passed.
//!
//! Read-only: nothing here changes a document.

use pdfcraft_chess::san::Lang;
use pdfcraft_chessvision::chess_text::{self, ChessLine, ChessPageText, ChessRun, LangGuess, LangTally, TextQuality};
use pdfcraft_chessvision::coords::{PageGeometry, PageRect, ViewRect};
use pdfcraft_chessvision::doc::{BlockHint, ChessDoc, DocBuilder, HintKind, PageInput};
use pdfcraft_render::{PageRenderer, RenderConfig, RenderRequest, RequestKind};

use pdfcraft_chess::repair::GameContext;
use pdfcraft_chessvision::cipher::BookCipher;
use pdfcraft_chessvision::doc::TextReview;
use pdfcraft_chessvision::glyphs::GlyphClassifier;

use super::ChessScanSettings;
use super::text_ocr::{self, LargerLine, OCR_DPI};
use crate::{DocId, Session};

/// A block at least this many times the body size is a heading (a game collection's 18 pt titles
/// over its 16 pt game text).
pub const HEADING_RATIO: f64 = 1.1;
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
    /// Pages (0-based) whose text layer is poor or missing and that chess OCR did not read (no
    /// glyph model, or OCR off), with the reason.
    pub needs_ocr: Vec<(usize, String)>,
    /// Pages chess OCR read instead of their text layer.
    pub ocr_pages: Vec<OcrPage>,
    /// The book's cipher as the pages taught it (keep it with the document to read more pages
    /// the same way).
    pub cipher: BookCipher,
    /// What the scan and the text reading noted, page by page (0-based), never silent.
    pub notes: Vec<(usize, String)>,
}

/// When to read pages with chess OCR instead of their text layer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OcrMode {
    /// Never: pages with a poor layer are listed in [`ChessDocument::needs_ocr`].
    Off,
    /// Pages whose text layer is poor or missing.
    #[default]
    Auto,
    /// Every page.
    All,
}

/// Where a book's pages came from, as the user says: which of a page's text layer and chess OCR
/// to believe. The engine decides page by page; this choice overrides it for a whole document.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BookOrigin {
    /// Decide page by page: chess OCR where the text layer is poor or missing, or is another
    /// program's invisible OCR over a scan ([`OcrMode::Auto`]).
    #[default]
    Auto,
    /// Born digital: the text layer is the book's own, so chess OCR never runs. A page whose layer
    /// looks poor is still read from it, and listed in [`ChessDocument::needs_ocr`] with why.
    Digital,
    /// Scanned: every page is read with chess OCR, whatever text layer it has.
    Scanned,
}

impl BookOrigin {
    pub const ALL: [BookOrigin; 3] = [BookOrigin::Auto, BookOrigin::Digital, BookOrigin::Scanned];

    /// When chess OCR reads a page of such a book.
    pub fn ocr_mode(self) -> OcrMode {
        match self {
            BookOrigin::Auto => OcrMode::Auto,
            BookOrigin::Digital => OcrMode::Off,
            BookOrigin::Scanned => OcrMode::All,
        }
    }
}

/// How [`Session::chess_document_with`] runs chess OCR.
#[derive(Clone, Copy, Default)]
pub struct OcrOptions<'a> {
    pub mode: OcrMode,
    /// The glyph classifier; `None` loads the user's `chess-glyphs` model.
    pub glyphs: Option<&'a dyn GlyphClassifier>,
    /// Also run the general OCR (`ocrs`) for prose when its models are installed.
    pub general: bool,
    /// The book's cipher so far (from earlier pages or an earlier run): this reading starts from
    /// it, and [`ChessDocument::cipher`] carries it on with what these pages taught.
    pub book: Option<&'a BookCipher>,
}

/// What chess OCR did on one page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OcrPage {
    pub page: usize,
    pub lines: usize,
    /// Words the repair changed, and words it could not settle (both flagged for review).
    pub edits: usize,
    pub flags: usize,
    pub general_ocr: bool,
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
pub fn body_size<'a>(blocks: impl IntoIterator<Item = &'a pdfcraft_edit::TextBlock>) -> Option<f64> {
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

/// The page's reading blocks as hints: headings by size against `body` (`None`: the page's own
/// body size), the rest paragraphs.
pub fn block_hints(blocks: &[pdfcraft_edit::TextBlock], body: Option<f64>) -> Vec<BlockHint> {
    let Some(body) = body.or_else(|| body_size(blocks)) else { return Vec::new() };
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

/// Most lines a header box holds (Pinski's: the game's number, the players, the place and year).
pub const FRAME_MAX_LINES: usize = 4;
/// Least share of a header box's width and height its text spans: it hugs its text.
pub const FRAME_MIN_FILL_X: f32 = 0.4;
pub const FRAME_MIN_FILL_Y: f32 = 0.5;

/// The headings of a page read with OCR: each frame drawn around text (`frames`, view space: a
/// game's header box, a chapter title's) that holds a short text, at most
/// [`HEADING_MAX_CHARS`] of `text`'s characters on at most [`FRAME_MAX_LINES`] lines, and hugs it
/// ([`FRAME_MIN_FILL_X`], [`FRAME_MIN_FILL_Y`]), is a heading (level 2). A frame around a longer
/// text (a sidebar) or a page's border is no heading, and a framed line with moves is never one
/// (the builder's rule).
pub fn frame_hints(frames: &[ViewRect], text: &ChessPageText, geometry: &PageGeometry) -> Vec<BlockHint> {
    frames
        .iter()
        .filter_map(|f| {
            let inside = lines_inside(text, *f);
            if !short_heading(&inside) {
                return None;
            }
            // A header box hugs its text; a page's or a table's border holds a little text in a
            // large box.
            let held = inside.iter().map(|l| l.rect).reduce(cover)?;
            if held.width() < FRAME_MIN_FILL_X * f.width() || held.height() < FRAME_MIN_FILL_Y * f.height() {
                return None;
            }
            geometry.view_to_user(*f).map(|rect| BlockHint { kind: HintKind::Heading { level: 2 }, rect })
        })
        .collect()
}

/// Largest gap between two larger lines of one title, as a share of the taller line: a two-line
/// chapter title is set with ordinary leading; the chapter's number above it stands a blank line
/// or more away.
pub const LARGER_LINE_GAP_SHARE: f32 = 1.0;

/// The headings of a page read with OCR from its lines set larger than its body (`larger`, view
/// space: a chapter title without a frame). Larger lines one under the other, at most
/// [`LARGER_LINE_GAP_SHARE`] of a line apart, are one title. A title is a heading when it holds a
/// short text (at most [`HEADING_MAX_CHARS`] of `text`'s characters on at most
/// [`FRAME_MAX_LINES`] lines) and no line of body size (another column's line beside it): level 1
/// at [`TITLE_RATIO`] times the body or more, else level 2. A larger line of moves is a main line
/// set large, not a title (the builder's rule).
pub fn size_hints(larger: &[LargerLine], text: &ChessPageText, geometry: &PageGeometry) -> Vec<BlockHint> {
    let mut lines: Vec<LargerLine> = larger.iter().copied().filter(|l| l.ratio.is_finite()).collect();
    lines.sort_by(|a, b| a.rect.y0.total_cmp(&b.rect.y0));
    // Each title, and its last line.
    let mut titles: Vec<(LargerLine, ViewRect)> = Vec::new();
    for line in lines {
        match titles.last_mut() {
            Some((title, last))
                if last.x0 < line.rect.x1
                    && line.rect.x0 < last.x1
                    && line.rect.y0 - last.y1 <= LARGER_LINE_GAP_SHARE * last.height().max(line.rect.height()) =>
            {
                title.rect = cover(title.rect, line.rect);
                title.ratio = title.ratio.max(line.ratio);
                *last = line.rect;
            }
            _ => titles.push((line, line.rect)),
        }
    }
    titles
        .iter()
        .filter_map(|(title, _)| {
            let inside = lines_inside(text, title.rect);
            let body_size = |l: &&ChessLine| !larger.iter().any(|g| centre_in(l.rect, g.rect));
            if !short_heading(&inside) || inside.iter().any(body_size) {
                return None;
            }
            let level = if f64::from(title.ratio) >= TITLE_RATIO { 1 } else { 2 };
            geometry.view_to_user(title.rect).map(|rect| BlockHint { kind: HintKind::Heading { level }, rect })
        })
        .collect()
}

/// Whether the centre of `line` is within `rect`.
fn centre_in(line: ViewRect, rect: ViewRect) -> bool {
    let (cx, cy) = ((line.x0 + line.x1) * 0.5, (line.y0 + line.y1) * 0.5);
    cx >= rect.x0 && cx <= rect.x1 && cy >= rect.y0 && cy <= rect.y1
}

/// The lines of `text` whose centre is within `rect`: the lines the builder gives a hint there.
fn lines_inside(text: &ChessPageText, rect: ViewRect) -> Vec<&ChessLine> {
    text.lines.iter().filter(|l| centre_in(l.rect, rect)).collect()
}

/// Whether `lines` are short enough for a heading: some text, at most [`HEADING_MAX_CHARS`]
/// characters on at most [`FRAME_MAX_LINES`] lines.
fn short_heading(lines: &[&ChessLine]) -> bool {
    let chars: usize = lines.iter().flat_map(|l| &l.spans).map(|s| span_chars(&s.run)).sum();
    chars > 0 && chars <= HEADING_MAX_CHARS && lines.len() <= FRAME_MAX_LINES
}

/// The smallest box covering both.
fn cover(a: ViewRect, b: ViewRect) -> ViewRect {
    ViewRect::new(a.x0.min(b.x0), a.y0.min(b.y0), a.x1.max(b.x1), a.y1.max(b.y1))
}

/// The characters a span of chess text stands for: its text, a move as printed, one for a sign.
fn span_chars(run: &ChessRun) -> usize {
    match run {
        ChessRun::Text(t) => t.chars().filter(|c| !c.is_whitespace()).count(),
        ChessRun::Move { text, .. } => text.chars().count(),
        _ => 1,
    }
}

/// The hints for a page from its reading blocks, and a note when they could not be read. A page
/// read with OCR gets none here: its blocks are the layer that was not believed (an earlier OCR
/// program's invisible text, whose sizes say nothing), so its lines are grouped by spacing, and
/// its headings come from the frames and the larger lines OCR found ([`frame_hints`],
/// [`size_hints`]).
pub fn page_hints(
    blocks: Option<&Result<Vec<pdfcraft_edit::TextBlock>, String>>,
    body: Option<f64>,
    read_by_ocr: bool,
) -> (Vec<BlockHint>, Option<String>) {
    match blocks {
        _ if read_by_ocr => (Vec::new(), None),
        Some(Ok(blocks)) => (block_hints(blocks, body), None),
        Some(Err(e)) => (Vec::new(), Some(format!("the page's paragraphs could not be read ({e}); lines were grouped by spacing"))),
        None => (Vec::new(), None),
    }
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
            PoorReason::GarbledPieces { words } => {
                format!("{words} moves have a stray symbol for their piece: an earlier OCR layer lost the figurines")
            }
            PoorReason::HiddenOcrLayer { .. } => "the text layer is invisible text over an image of the page: another program's OCR".into(),
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
        self.chess_document_with(id, pages, settings, lang, &OcrOptions { general: true, ..OcrOptions::default() })
    }

    /// [`Self::chess_document`], with chess OCR as `ocr` says.
    pub fn chess_document_with(
        &mut self,
        id: DocId,
        pages: &[usize],
        settings: &ChessScanSettings,
        lang: Option<Lang>,
        ocr: &OcrOptions<'_>,
    ) -> Result<ChessDocument, String> {
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
        let (chosen, mut chess_texts) = match lang {
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
        // A layer mostly drawn invisibly over a picture of the page is another program's OCR.
        // Only such layers ask whether images cover the page.
        for (page, text) in scanned.iter().zip(&mut chess_texts) {
            if text.hidden > pdfcraft_chessvision::chess_text::quality::MAX_HIDDEN_RATIO
                && doc.info.pages.get(page.page).is_some_and(|info| crate::scan_fields::is_scan(doc, page.page, info))
            {
                text.over_page_image();
            }
        }

        // The glyph classifier, when chess OCR may run.
        let models = (ocr.mode != OcrMode::Off && ocr.glyphs.is_none()).then(|| super::models(settings.models_dir.as_deref()));
        let glyphs: Option<&dyn GlyphClassifier> = match (ocr.mode, ocr.glyphs) {
            (OcrMode::Off, _) => None,
            (_, Some(g)) => Some(g),
            (_, None) => models.as_deref().and_then(|m| m.glyphs.loaded()).map(|g| g as &dyn GlyphClassifier),
        };
        let mut needs_ocr = Vec::new();
        let mut ocr_pages = Vec::new();
        let mut cipher = ocr.book.cloned().unwrap_or_default();
        // The last legal, accepted position seen: the moves of a page without a diagram go on from it.
        let mut last_fen: Option<String> = None;
        // Every page's reading blocks, and the body size of them all.
        let page_blocks: Vec<Option<Result<Vec<pdfcraft_edit::TextBlock>, String>>> = scanned
            .iter()
            .map(|page| doc.editor.as_ref().map(|e| pdfcraft_edit::reading_blocks(&e.cos, page.page).map_err(|e| e.to_string())))
            .collect();
        let body = body_size(page_blocks.iter().flatten().flatten().flatten());
        let mut builder = DocBuilder::new();
        for ((page, layer_text), blocks) in scanned.iter().zip(&chess_texts).zip(&page_blocks) {
            let mut text = layer_text;
            let mut reviews: Vec<TextReview> = Vec::new();
            let ocr_text;
            let mut ocr_frames: Vec<ViewRect> = Vec::new();
            let mut ocr_larger: Vec<LargerLine> = Vec::new();
            notes.extend(page.notes.iter().map(|n| (page.page, n.clone())));
            if let Some(e) = &page.error {
                notes.push((page.page, e.clone()));
            }
            let poor = poor_reason(&text.quality);
            let wanted = match ocr.mode {
                OcrMode::Off => false,
                OcrMode::Auto => poor.is_some(),
                OcrMode::All => true,
            };
            let readings = page.readings.as_deref().unwrap_or_default();
            let here_fen = readings
                .iter()
                .filter_map(|r| r.position())
                .find(|p| p.accepted && p.legality.allows(p.side.color) && !p.repaired())
                .map(|p| p.fen.clone());
            match (wanted, glyphs, doc.info.pages.get(page.page), page.geometry) {
                (true, Some(g), Some(info), Some(geometry)) => match super::render_gray(&mut renderer, page.page, info, OCR_DPI) {
                    Ok((gray, dpi)) => {
                        let boxes: Vec<_> = page
                            .diagrams
                            .iter()
                            .filter_map(|d| geometry.user_to_view(d.candidate.bbox_user))
                            .filter_map(|v| pdfcraft_chessvision::coords::view_to_pixels(v, dpi))
                            .collect();
                        let words = match if ocr.general { text_ocr::general_ocr_words(&gray) } else { None } {
                            Some(Ok(w)) => Some(w),
                            Some(Err(e)) => {
                                notes.push((page.page, format!("the general OCR failed ({e}); the glyph reader read the prose too")));
                                None
                            }
                            None => None,
                        };
                        let context = here_fen.as_deref().or(last_fen.as_deref()).and_then(GameContext::from_fen);
                        let index = u32::try_from(page.page).unwrap_or(u32::MAX);
                        match text_ocr::read_page(&gray, dpi, &boxes, g, words.as_deref(), chosen.lang, context.as_ref(), &mut cipher, index) {
                            Ok(read) => {
                                ocr_pages.push(OcrPage {
                                    page: page.page,
                                    lines: read.lines,
                                    edits: read.edits,
                                    flags: read.flags,
                                    general_ocr: read.general_ocr,
                                });
                                reviews = read.reviews;
                                ocr_frames = read.frames;
                                ocr_larger = read.larger;
                                ocr_text = read.text;
                                text = &ocr_text;
                            }
                            Err(e) => {
                                notes.push((page.page, format!("chess OCR failed: {e}")));
                                if let Some(why) = &poor {
                                    needs_ocr.push((page.page, why.clone()));
                                }
                            }
                        }
                    }
                    Err(e) => {
                        notes.push((page.page, format!("the page could not be rendered for chess OCR: {e}")));
                        if let Some(why) = &poor {
                            needs_ocr.push((page.page, why.clone()));
                        }
                    }
                },
                _ => {
                    if wanted && glyphs.is_none() && poor.is_none() {
                        notes.push((page.page, "chess OCR was asked for, but it needs the chess-glyphs model".into()));
                    }
                    if let Some(why) = poor {
                        let why = if wanted && glyphs.is_none() { format!("{why} (chess OCR needs the chess-glyphs model)") } else { why };
                        needs_ocr.push((page.page, why));
                    }
                }
            }
            if here_fen.is_some() {
                last_fen = here_fen;
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
            let read_by_ocr = !std::ptr::eq(text, layer_text);
            let (mut hints, note) = page_hints(blocks.as_ref(), body, read_by_ocr);
            if read_by_ocr {
                // A frame's heading comes first: a title inside a box is the box's.
                hints = frame_hints(&ocr_frames, text, &geometry);
                hints.extend(size_hints(&ocr_larger, text, &geometry));
            }
            notes.extend(note.map(|n| (page.page, n)));
            let diagrams = readings;
            if page.readings.is_none() && !page.diagrams.is_empty() {
                notes.push((page.page, "the page's diagrams could not be read; they are left out".into()));
            }
            let index = u32::try_from(page.page).unwrap_or(u32::MAX);
            builder.add_page(&PageInput { index, geometry, text, diagrams, hints: &hints, reviews: &reviews });
        }
        Ok(ChessDocument { doc: builder.finish(), lang: chosen, lang_given: lang.is_some(), needs_ocr, ocr_pages, cipher, notes })
    }
}
