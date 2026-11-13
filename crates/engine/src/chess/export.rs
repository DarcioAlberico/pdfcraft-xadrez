//! Chess ▸ Export (blueprint S15, one entry point for the tools and the interface): the
//! recognised document as HTML, EPUB, Word, RTF, Markdown or PGN.
//!
//! [`Session::chess_export`] reads the pages ([`Session::chess_document_with`], chess OCR as
//! asked) and writes the file's bytes, plus, for Markdown, the diagram images it refers to
//! (paths relative to the Markdown file). Writing them to disk is the caller's: the tools confine
//! them to `--root`, the interface asks where.

use std::path::Path;
use std::sync::Arc;

use pdfcraft_chess::draw::{BoardStyle, PieceSet};
use pdfcraft_chess::san::Lang;
use pdfcraft_chessvision::doc::html::{self, DiagramMode, Figurines, HtmlOptions};
use pdfcraft_chessvision::doc::{Block, PageLabel, markdown};
use pdfcraft_chessvision::text_font::TextFont;

use super::ChessScanSettings;
use super::document::{ChessDocument, OcrMode, OcrOptions};
use crate::{DocId, Session};

/// The files Chess ▸ Export writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChessFormat {
    Html,
    Epub,
    Docx,
    Rtf,
    Markdown,
    Pgn,
}

impl ChessFormat {
    pub const ALL: [ChessFormat; 6] =
        [ChessFormat::Html, ChessFormat::Epub, ChessFormat::Docx, ChessFormat::Rtf, ChessFormat::Markdown, ChessFormat::Pgn];

    /// The file extension.
    pub fn extension(self) -> &'static str {
        match self {
            ChessFormat::Html => "html",
            ChessFormat::Epub => "epub",
            ChessFormat::Docx => "docx",
            ChessFormat::Rtf => "rtf",
            ChessFormat::Markdown => "md",
            ChessFormat::Pgn => "pgn",
        }
    }

    /// The format's name, as menus show it.
    pub fn name(self) -> &'static str {
        match self {
            ChessFormat::Html => "HTML",
            ChessFormat::Epub => "EPUB",
            ChessFormat::Docx => "Word",
            ChessFormat::Rtf => "RTF",
            ChessFormat::Markdown => "Markdown",
            ChessFormat::Pgn => "PGN",
        }
    }
}

/// How to export: [`ChessExportOptions::new`], then the fields.
/// Built outside the engine by its constructor and its fields, never by a literal (plan A9-Q1b,
/// A7-14): a field added here then breaks no caller (the cause of #185).
#[non_exhaustive]
#[derive(Clone, Debug)]
pub struct ChessExportOptions {
    /// Pages (0-based); empty: all.
    pub pages: Vec<usize>,
    pub scan: ChessScanSettings,
    pub lang: Option<Lang>,
    pub ocr: OcrMode,
    pub diagrams: DiagramMode,
    pub figurines: Figurines,
    pub links: bool,
    pub link_text: String,
    pub pieces: Arc<PieceSet>,
    /// HTML and EPUB: the user's figurine font, if any (`chessvision::text_font::find_fonts`).
    pub text_font: Option<Arc<TextFont>>,
    /// HTML and EPUB: the user's sign font (ISChess), if any.
    pub sign_font: Option<Arc<TextFont>>,
    pub title: String,
    /// Word, RTF and Markdown: the diagram images' side in pixels.
    pub diagram_px: u32,
    /// Markdown: the images' folder, relative to the Markdown file.
    pub image_dir: String,
    /// The book's cipher so far: chess OCR starts from it ([`ChessExport::read`] carries it on).
    pub book: Option<pdfcraft_chessvision::cipher::BookCipher>,
    /// How the diagrams are drawn: their squares' colours (a board theme's, plan HC7), hatched or
    /// not. The default is the boards' brown.
    pub style: BoardStyle,
    /// PGN: which diagrams get a game of their own (plan A8 P8b). The default is those no game
    /// starts from or reprints.
    pub positions: markdown::PgnPositions,
    /// PGN: what the moves leave out (plan A8 V4). The default keeps everything.
    pub pgn_style: pdfcraft_chess::pgn_style::PgnStyle,
    /// HTML and EPUB: highlight what the reading flagged for review. `None`: on in HTML (a file
    /// to check), off in EPUB (a book to read; plan A8 P9b).
    pub review_marks: Option<bool>,
    /// PGN: the book's own PGN (plan "Carregar PGN do livro", R4). With it, the PGN written is
    /// that PGN's games with the book's text as their comments, in place of the games read.
    pub reference: Option<Arc<pdfcraft_chessvision::doc::reference::ReferenceIndex>>,
}

impl ChessExportOptions {
    /// The defaults for a document titled `title`: every page, OCR where the layer is poor,
    /// redrawn diagrams, Unicode figurines, lichess links, the user's pieces and fonts as found in
    /// `install` (the folder the app is installed in, which the frontend resolves; `None` on the
    /// web) and the environment.
    pub fn new(title: &str, install: Option<&Path>) -> ChessExportOptions {
        let fonts = pdfcraft_chessvision::text_font::find_fonts(install);
        ChessExportOptions {
            pages: Vec::new(),
            scan: ChessScanSettings::default(),
            lang: None,
            ocr: OcrMode::Auto,
            diagrams: DiagramMode::Svg,
            figurines: Figurines::Unicode,
            links: true,
            link_text: super::links::DEFAULT_TEXT.into(),
            pieces: Arc::new(pdfcraft_chessvision::pieces::find(install).0),
            text_font: fonts.figurines.map(Arc::new),
            sign_font: fonts.signs.map(Arc::new),
            title: title.into(),
            diagram_px: super::office::DEFAULT_DIAGRAM_PX,
            image_dir: markdown::image_folder(title),
            book: None,
            style: BoardStyle::default(),
            positions: markdown::PgnPositions::default(),
            pgn_style: pdfcraft_chess::pgn_style::PgnStyle::FULL,
            review_marks: None,
            reference: None,
        }
    }
}

/// What [`Session::chess_export`] made.
#[derive(Clone, Debug)]
pub struct ChessExport {
    pub bytes: Vec<u8>,
    /// Markdown: the images, as (path relative to the Markdown file, PNG bytes).
    pub images: Vec<(String, Vec<u8>)>,
    pub read: ChessDocument,
    /// Diagrams drawn (SVG boards or images).
    pub drawn: usize,
    /// PGN: games with no moves added for positions (with a reference too, plan A9-F4).
    pub position_games: Option<usize>,
    /// PGN: the diagrams that were to get a game of their own and did not, and why.
    pub positions_skipped: Vec<markdown::SkippedPosition>,
    /// PGN: what writing left out of the games, by game in the order written (plan A9-F4).
    pub pgn_warnings: Vec<pdfcraft_chessvision::doc::pgn::GameWarning>,
    /// PGN with [`ChessExportOptions::reference`]: what the book's PGN was given.
    /// Boxed: the export's result travels in the interface's job messages, which it would make large.
    pub annotated: Option<Box<pdfcraft_chessvision::doc::reference::annotate::AnnotatedPgn>>,
    /// What the writers noted (a diagram that could not be drawn, say).
    pub notes: Vec<String>,
}

impl ChessExport {
    /// How many diagram blocks there are, and how many of them have a position.
    pub fn diagram_counts(&self) -> (usize, usize) {
        self.read
            .doc
            .pages
            .iter()
            .flat_map(|p| &p.blocks)
            .filter_map(|b| if let Block::Diagram(d) = b { Some(d) } else { None })
            .fold((0, 0), |(n, p), d| (n + 1, p + usize::from(d.reading.is_some())))
    }
}

/// What page `index` (0-based) of `doc` is called (plan A9-T2a, R3-08): the label its
/// `/PageLabels` prints, else its number from 1 ([`PageLabel`]). The exports say every page by
/// this one rule, and so does the interface everywhere it names a page (plan A9-T2b,
/// [`page_label_of`]).
pub fn page_label(doc: &crate::Document, index: u32) -> PageLabel {
    page_label_of(&doc.info, usize::try_from(index).unwrap_or(usize::MAX))
}

/// [`page_label`] of a document's `info`, page `index` (0-based; a page past `u32::MAX`, which no
/// document has, as `u32::MAX`).
pub fn page_label_of(info: &crate::DocInfo, index: usize) -> PageLabel {
    PageLabel::new(info.pages.get(index).map(|p| p.label.as_str()), u32::try_from(index).unwrap_or(u32::MAX))
}

impl Session {
    /// Chess ▸ Export: read the document's pages and write them as `format`.
    pub fn chess_export(&mut self, id: DocId, format: ChessFormat, o: &ChessExportOptions) -> Result<ChessExport, String> {
        let ocr = OcrOptions { mode: o.ocr, glyphs: None, general: true, book: o.book.as_ref() };
        let read = self.chess_document_with(id, &o.pages, &o.scan, o.lang, &ocr)?;
        // The book's language, not its piece letters' (plan A9-L6a, A2-08): `<html lang>`,
        // `dc:language`, `w:lang`; the letters' only when nothing says the text's.
        let book_lang = if read.text_lang.is_empty() { read.lang.lang.code() } else { read.text_lang.as_str() };
        // The printed page numbers and the book's language say each diagram (plan A8 P9b, P9d).
        let labels: Vec<String> = self.get(id).map(|d| d.info.pages.iter().map(|p| p.label.clone()).collect()).unwrap_or_default();
        let options = HtmlOptions {
            title: &o.title,
            pieces: &o.pieces,
            style: o.style,
            diagrams: o.diagrams,
            figurines: o.figurines,
            links: o.links,
            link_text: &o.link_text,
            text_font: o.text_font.as_deref(),
            sign_font: o.sign_font.as_deref(),
            lang: book_lang,
            page_labels: &labels,
            review_marks: o.review_marks.unwrap_or(format != ChessFormat::Epub),
        };
        let mut notes = Vec::new();
        let mut images = Vec::new();
        let mut position_games = None;
        let mut positions_skipped = Vec::new();
        let mut pgn_warnings = Vec::new();
        let mut annotated = None;
        // HTML and EPUB draw every diagram that has a position, as SVG, unless asked for FENs.
        let read_diagrams = read.doc.pages.iter().flat_map(|p| &p.blocks).filter(|b| matches!(b, Block::Diagram(d) if d.reading.is_some())).count();
        let mut drawn = if o.diagrams == DiagramMode::Svg { read_diagrams } else { 0 };
        let bytes = match format {
            ChessFormat::Html => html::write(&read.doc, &options).into_bytes(),
            ChessFormat::Epub => {
                let doc = self.get(id).ok_or("no such document")?;
                // The cover is the book's first page; a book without one is still a book.
                let first = read.doc.pages.first().and_then(|p| usize::try_from(p.index).ok()).unwrap_or(0);
                let cover = match super::epub::cover_png(doc.export_source(), first) {
                    Ok(png) => Some(png),
                    Err(e) => {
                        notes.push(format!("the book has no cover: {e}"));
                        None
                    }
                };
                let meta = super::epub::EpubMeta {
                    title: o.title.clone(),
                    lang: book_lang.to_owned(),
                    // The PDF's own identifier outlives saving; its bytes do not. The identifier
                    // every new file used to get names no document (`DocKeys::id`); a part of the
                    // document is another book.
                    identifier: super::epub::identifier_for_pages(
                        doc.doc_keys().id().map_or_else(|| super::epub::identifier(&doc.bytes), |id| super::epub::identifier_from_id(&id)),
                        &o.pages,
                        doc.info.pages.len(),
                    ),
                    modified: super::epub::modified_at(self.now_secs()),
                    cover,
                    creator: doc.info.author.clone(),
                    source: Some(doc.name.clone()).filter(|n| !n.trim().is_empty()),
                };
                // A book with no `/PageLabels` is paged by the numbers it prints (plan A9-L1b).
                let printed = PageLabel::with_folios(&labels, &read.doc);
                super::epub::epub(&read.doc, &HtmlOptions { page_labels: &printed, ..options.clone() }, &meta)?
            }
            ChessFormat::Pgn => {
                // Each game says where in the book it came from (plan A8 P8a).
                let doc = self.get(id).ok_or("no such document")?;
                let book = markdown::PgnBook {
                    title: Some(o.title.clone()).filter(|t| !t.trim().is_empty()),
                    labels: doc.info.pages.iter().map(|p| p.label.clone()).collect(),
                };
                drawn = 0;
                if let Some(index) = &o.reference {
                    let made = pdfcraft_chessvision::doc::reference::annotate::annotated_pgn(&read.doc, index, &book, o.positions, o.pgn_style);
                    let bytes = made.pgn.clone().into_bytes();
                    position_games = Some(made.positions);
                    positions_skipped.clone_from(&made.skipped);
                    pgn_warnings.clone_from(&made.warnings);
                    annotated = Some(Box::new(made));
                    bytes
                } else {
                    let made = markdown::pgn_export(&read.doc, &book, o.positions, o.pgn_style);
                    position_games = Some(made.positions);
                    positions_skipped = made.skipped;
                    pgn_warnings = made.warnings;
                    made.pgn.into_bytes()
                }
            }
            ChessFormat::Markdown => {
                let with_images = o.diagrams == DiagramMode::Svg;
                drawn = 0;
                if with_images {
                    for page in &read.doc.pages {
                        for block in &page.blocks {
                            let Block::Diagram(d) = block else { continue };
                            let Some(shown) = d.shown() else { continue };
                            let png = pdfcraft_chess::fen::Fen::parse(shown.fen)
                                .map_err(|e| e.to_string())
                                .and_then(|f| super::office::diagram_png(f.placement(), &o.pieces, &o.style, o.diagram_px));
                            match png {
                                Ok(png) => {
                                    images.push((format!("{}/{}", o.image_dir, markdown::image_name(page.index, d.number)), png));
                                    drawn += 1;
                                }
                                Err(e) => notes.push(format!("page {}, diagram {}: {e}", PageLabel::of(&labels, page.index), d.number + 1)),
                            }
                        }
                    }
                }
                let md = markdown::MarkdownOptions {
                    title: &o.title,
                    figurines: o.figurines,
                    diagrams: if with_images { markdown::MarkdownDiagrams::Images } else { markdown::MarkdownDiagrams::Fen },
                    links: o.links,
                    link_text: &o.link_text,
                    image_dir: &o.image_dir,
                    lang: book_lang,
                    page_labels: &labels,
                };
                markdown::write(&read.doc, &md).into_bytes()
            }
            ChessFormat::Docx | ChessFormat::Rtf => {
                let office = super::office::OfficeOptions {
                    title: &o.title,
                    pieces: &o.pieces,
                    style: o.style,
                    figurines: o.figurines,
                    links: o.links,
                    link_text: &o.link_text,
                    diagram_px: o.diagram_px,
                    text_font: o.text_font.as_deref(),
                    sign_font: o.sign_font.as_deref(),
                    lang: book_lang,
                    page_labels: &labels,
                };
                let (flow, count) = super::office::to_flow(&read.doc, &office, &mut notes);
                drawn = count;
                if format == ChessFormat::Docx {
                    super::office::docx(&flow).map_err(|e| e.to_string())?
                } else {
                    super::office::rtf(&flow).into_bytes()
                }
            }
        };
        Ok(ChessExport { bytes, images, read, drawn, position_games, positions_skipped, pgn_warnings, annotated, notes })
    }
}
