//! Chess ▸ Export (blueprint S15, one entry point for the tools and the interface): the
//! recognised document as HTML, EPUB, Word, RTF, Markdown or PGN.
//!
//! [`Session::chess_export`] reads the pages ([`Session::chess_document_with`], chess OCR as
//! asked) and writes the file's bytes, plus, for Markdown, the diagram images it refers to
//! (paths relative to the Markdown file). Writing them to disk is the caller's: the tools confine
//! them to `--root`, the interface asks where.

use std::sync::Arc;

use pdfcraft_chess::draw::{BoardStyle, PieceSet};
use pdfcraft_chess::san::Lang;
use pdfcraft_chessvision::doc::html::{self, DiagramMode, Figurines, HtmlOptions};
use pdfcraft_chessvision::doc::{Block, markdown};
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

/// How to export.
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
}

impl ChessExportOptions {
    /// The defaults for a document titled `title`: every page, OCR where the layer is poor,
    /// redrawn diagrams, Unicode figurines, lichess links, the user's pieces.
    pub fn new(title: &str) -> ChessExportOptions {
        let fonts = pdfcraft_chessvision::text_font::find_fonts();
        ChessExportOptions {
            pages: Vec::new(),
            scan: ChessScanSettings::default(),
            lang: None,
            ocr: OcrMode::Auto,
            diagrams: DiagramMode::Svg,
            figurines: Figurines::Unicode,
            links: true,
            link_text: super::links::DEFAULT_TEXT.into(),
            pieces: Arc::new(pdfcraft_chessvision::pieces::find().0),
            text_font: fonts.figurines.map(Arc::new),
            sign_font: fonts.signs.map(Arc::new),
            title: title.into(),
            diagram_px: super::office::DEFAULT_DIAGRAM_PX,
            image_dir: format!("{title}_diagrams"),
            book: None,
            style: BoardStyle::default(),
            positions: markdown::PgnPositions::default(),
            pgn_style: pdfcraft_chess::pgn_style::PgnStyle::FULL,
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
    /// PGN: games with no moves added for positions.
    pub position_games: Option<usize>,
    /// PGN: the diagrams that were to get a game of their own and did not, and why.
    pub positions_skipped: Vec<markdown::SkippedPosition>,
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

impl Session {
    /// Chess ▸ Export: read the document's pages and write them as `format`.
    pub fn chess_export(&mut self, id: DocId, format: ChessFormat, o: &ChessExportOptions) -> Result<ChessExport, String> {
        let ocr = OcrOptions { mode: o.ocr, glyphs: None, general: true, book: o.book.as_ref() };
        let read = self.chess_document_with(id, &o.pages, &o.scan, o.lang, &ocr)?;
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
        };
        let mut notes = Vec::new();
        let mut images = Vec::new();
        let mut position_games = None;
        let mut positions_skipped = Vec::new();
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
                    lang: read.lang.lang.code().to_owned(),
                    identifier: super::epub::identifier(&doc.bytes),
                    modified: super::epub::modified_at(self.now_secs()),
                    cover,
                };
                super::epub::epub(&read.doc, &options, &meta)?
            }
            ChessFormat::Pgn => {
                // Each game says where in the book it came from (plan A8 P8a).
                let doc = self.get(id).ok_or("no such document")?;
                let book = markdown::PgnBook {
                    title: Some(o.title.clone()).filter(|t| !t.trim().is_empty()),
                    labels: doc.info.pages.iter().map(|p| p.label.clone()).collect(),
                };
                let made = markdown::pgn_export(&read.doc, &book, o.positions, o.pgn_style);
                position_games = Some(made.positions);
                positions_skipped = made.skipped;
                drawn = 0;
                made.pgn.into_bytes()
            }
            ChessFormat::Markdown => {
                let with_images = o.diagrams == DiagramMode::Svg;
                drawn = 0;
                if with_images {
                    for page in &read.doc.pages {
                        for block in &page.blocks {
                            let Block::Diagram(d) = block else { continue };
                            let Some(reading) = &d.reading else { continue };
                            let png = pdfcraft_chess::fen::parse(&reading.fen)
                                .map_err(|e| e.to_string())
                                .and_then(|f| super::office::diagram_png(&f.placement, &o.pieces, &o.style, o.diagram_px));
                            match png {
                                Ok(png) => {
                                    images.push((format!("{}/{}", o.image_dir, markdown::image_name(page.index, d.number)), png));
                                    drawn += 1;
                                }
                                Err(e) => notes.push(format!("page {}, diagram {}: {e}", u64::from(page.index) + 1, d.number + 1)),
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
                };
                markdown::write(&read.doc, &md).into_bytes()
            }
            ChessFormat::Docx | ChessFormat::Rtf => {
                let office = super::office::OfficeOptions {
                    pieces: &o.pieces,
                    style: o.style,
                    figurines: o.figurines,
                    links: o.links,
                    link_text: &o.link_text,
                    diagram_px: o.diagram_px,
                };
                let (pages, count) = super::office::to_pages(&read.doc, &office, &mut notes);
                drawn = count;
                if format == ChessFormat::Docx { super::office::docx(&pages, &o.title) } else { super::office::rtf(&pages).into_bytes() }
            }
        };
        Ok(ChessExport { bytes, images, read, drawn, position_games, positions_skipped, notes })
    }
}
