//! Chess ▸ the book's exercises as a sheet to print (plan A9-N12a, finding A8-14): a **new PDF**,
//! never the book (which does not change), of A4 or Letter pages, each a grid of 4, 6 or 9
//! diagrams with their captions, then the solutions in the order of the numbers.
//!
//! What goes on it is [`pdfcraft_chessvision::doc::worksheet`]'s, from the reading the session
//! (or the disk) holds, as for every export. Each board is the Redraw's vector drawing
//! ([`pdfcraft_chess::draw::board_with`] → [`super::replace::drawing_ops`]) with the export's
//! pieces and board style, from the side the book prints it. The text is added text
//! ([`pdfcraft_edit::added`]): Helvetica, and for a caption Helvetica cannot write (a language
//! outside WinAnsi) the build's own font (`craft-fonts`, optional) when it has one, else the
//! caption in English. The file gets its own `/ID`, its title and its language.

use std::sync::Arc;

use pdfcraft_chess::draw::{BoardStyle, board_with};
use pdfcraft_chessvision::doc::markdown::PgnBook;
use pdfcraft_chessvision::doc::reference::{self, ReferenceIndex};
use pdfcraft_chessvision::doc::worksheet::{self, Item, Layout, Worksheet};
use pdfcraft_cos::{Document, Object, PdfString};
use pdfcraft_edit::{AddedText, Align, Content as AddedContent};

use super::document::OcrOptions;
use super::export::ChessExportOptions;
use super::{ChessDocument, replace};
use crate::{DocId, Session};

/// The paper a sheet is printed on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Paper {
    #[default]
    A4,
    Letter,
}

impl Paper {
    /// Its width and height in points.
    pub fn size(self) -> (f64, f64) {
        match self {
            Paper::A4 => (595.276, 841.89),
            Paper::Letter => (612.0, 792.0),
        }
    }

    pub fn of(name: &str) -> Option<Paper> {
        match name {
            "a4" | "A4" => Some(Paper::A4),
            "letter" | "Letter" => Some(Paper::Letter),
            _ => None,
        }
    }
}

/// The page's margin, in points.
const MARGIN: f64 = 36.0;
/// The room a caption takes under its board, and its size.
const CAPTION_ROOM: f64 = 20.0;
const CAPTION_SIZE: f64 = 9.0;
/// The space between two cells of the grid.
const GAP: f64 = 12.0;
/// The key's text size and line height.
const KEY_SIZE: f64 = 10.0;
const KEY_LINE: f64 = 14.0;

/// Where a board and its caption go on a page: the board's left, top and side, user space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    pub x: f64,
    pub top: f64,
    pub side: f64,
    /// The cell's width, which the caption is centred in.
    pub width: f64,
}

/// The cells of a page of `paper` holding `layout`'s grid, row by row.
pub fn cells(paper: Paper, layout: &Layout) -> Vec<Cell> {
    let (w, h) = paper.size();
    let (cols, rows) = layout.per_page.grid();
    let (cols_f, rows_f) = (cols as f64, rows as f64);
    let cell_w = (w - 2.0 * MARGIN - GAP * (cols_f - 1.0)) / cols_f;
    let cell_h = (h - 2.0 * MARGIN - GAP * (rows_f - 1.0)) / rows_f;
    let side = cell_w.min(cell_h - CAPTION_ROOM).max(0.0);
    let mut out = Vec::with_capacity(cols.saturating_mul(rows));
    for r in 0..rows {
        for c in 0..cols {
            let x0 = MARGIN + c as f64 * (cell_w + GAP);
            let top = h - MARGIN - r as f64 * (cell_h + GAP);
            out.push(Cell { x: x0 + (cell_w - side) / 2.0, top, side, width: cell_w });
        }
    }
    out
}

/// How a sheet is made, besides the export's look and reading (the [`ChessExportOptions`] given
/// with it).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SheetOptions {
    pub layout: Layout,
    pub paper: Paper,
}

/// What [`Session::chess_worksheet`] made.
#[derive(Clone, Debug)]
pub struct MadeSheet {
    /// The new PDF.
    pub bytes: Arc<Vec<u8>>,
    pub sheet: Worksheet,
    /// Its pages: the diagrams', then the key's.
    pub diagram_pages: usize,
    pub key_pages: usize,
    /// Captions and key lines written in English: no font of this build writes the book's
    /// language.
    pub in_english: usize,
    pub read: ChessDocument,
    pub notes: Vec<String>,
}

/// `text` cut into lines no wider than `width` points at `size` (Helvetica's widths), between
/// words; a word wider than a line has a line of its own.
fn wrap(text: &str, size: f64, width: f64) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let candidate = if line.is_empty() { word.to_owned() } else { format!("{line} {word}") };
        if !line.is_empty() && pdfcraft_fonts::helvetica_width(&candidate, size) > width {
            lines.push(std::mem::take(&mut line));
            line = word.to_owned();
        } else {
            line = candidate;
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// Add `text` to `page` of `doc` in the box `rect` (user space of an upright page), or, when no
/// font of this build writes it, `english`. Whether it went in English.
fn add_text(doc: &mut Document, page: usize, rect: [f64; 4], size: f64, align: Align, text: &str, english: &str) -> Result<bool, String> {
    let item = |t: &str| AddedContent::Text(AddedText { rect, text: t.to_owned(), size, align, ..AddedText::default() });
    match pdfcraft_edit::add_content(doc, page, &item(text)) {
        Ok(_) => Ok(false),
        Err(e) if text != english => pdfcraft_edit::add_content(doc, page, &item(english)).map(|_| true).map_err(|_| e.to_string()),
        Err(e) => Err(e.to_string()),
    }
}

impl Session {
    /// The sheet of document `id`'s exercises (plan A9-N12a): the pages `o` names read as every
    /// export reads them (linked to its references), laid out as `sheet` says, drawn with `o`'s
    /// pieces and board style, titled `o.title`, in the book's language. The document itself is
    /// not changed.
    pub fn chess_worksheet(&mut self, id: DocId, o: &ChessExportOptions, sheet: &SheetOptions) -> Result<MadeSheet, String> {
        let ocr = OcrOptions { mode: o.ocr, glyphs: None, general: true, book: o.book.as_ref() };
        let mut read = self.chess_document_with(id, &o.pages, &o.scan, o.lang, &ocr)?;
        let refs: Vec<&ReferenceIndex> = [o.games.as_deref(), o.reference.as_deref()].into_iter().flatten().collect();
        if !refs.is_empty() {
            reference::link_all(&mut read.doc, &refs);
        }
        let labels: Vec<String> = self.get(id).map(|d| d.info.pages.iter().map(|p| p.label.clone()).collect()).unwrap_or_default();
        let book = PgnBook { title: Some(o.title.clone()).filter(|t| !t.trim().is_empty()), labels };
        let made = worksheet::worksheet(&read.doc, &book, &sheet.layout);
        let book_lang = if read.text_lang.is_empty() { read.lang.lang.code().to_owned() } else { read.text_lang.as_str().to_owned() };
        let (doc, key_pages, in_english, notes) = sheet_document(&made, sheet, o, &book_lang)?;
        let bytes = self.write_new(&doc).map_err(|e| e.to_string())?;
        Ok(MadeSheet { bytes, diagram_pages: made.pages.len(), key_pages, in_english, notes, sheet: made, read })
    }
}

/// The sheet as a new document: its diagram pages, then the key's; the number of key pages, how
/// many texts went in English, and the notes.
fn sheet_document(
    sheet: &Worksheet,
    options: &SheetOptions,
    o: &ChessExportOptions,
    lang: &str,
) -> Result<(Document, usize, usize, Vec<String>), String> {
    let (w, h) = options.paper.size();
    let cells = cells(options.paper, &options.layout);
    // The key's lines, wrapped, and its pages.
    let width = w - 2.0 * MARGIN;
    let mut key: Vec<(String, String)> = Vec::new();
    for (line, english) in &sheet.solutions {
        let (a, b) = (wrap(line, KEY_SIZE, width), wrap(english, KEY_SIZE, width));
        // A line and its English take the same room: as many lines as the longer.
        let n = a.len().max(b.len());
        for k in 0..n {
            key.push((a.get(k).cloned().unwrap_or_default(), b.get(k).cloned().unwrap_or_default()));
        }
    }
    let per_page = (((h - 2.0 * MARGIN) / KEY_LINE).floor() as usize).max(1);
    let key_pages = key.len().div_ceil(per_page);
    let total = sheet.pages.len().saturating_add(key_pages).max(1);
    let mut doc = pdfcraft_create::blank(w, h, total).map_err(|e| e.to_string())?;
    let mut in_english = 0usize;
    let mut notes = Vec::new();
    for (p, items) in sheet.pages.iter().enumerate() {
        for (item, cell) in items.iter().zip(&cells) {
            board_ops(&mut doc, p, item, cell, o)?;
            let rect = [
                cell.x + (cell.side - cell.width) / 2.0,
                cell.top - cell.side - CAPTION_ROOM,
                cell.x + (cell.side + cell.width) / 2.0,
                cell.top - cell.side - 4.0,
            ];
            if add_text(&mut doc, p, rect, CAPTION_SIZE, Align::Center, &item.caption, &item.caption_en)? {
                in_english = in_english.saturating_add(1);
            }
        }
    }
    for (k, chunk) in key.chunks(per_page).enumerate() {
        let page = sheet.pages.len().saturating_add(k);
        for (i, (line, english)) in chunk.iter().enumerate() {
            let top = h - MARGIN - i as f64 * KEY_LINE;
            if line.is_empty() && english.is_empty() {
                continue;
            }
            if add_text(&mut doc, page, [MARGIN, top - KEY_LINE, w - MARGIN, top], KEY_SIZE, Align::Left, line, english)? {
                in_english = in_english.saturating_add(1);
            }
        }
    }
    if sheet.over > 0 {
        notes.push(format!("{} exercises past the sheet's {} were left out", sheet.over, worksheet::MAX_EXERCISES));
    }
    if in_english > 0 {
        notes.push(format!("{in_english} captions or key lines are in English: no font of this build writes the book's language"));
    }
    set_title_and_lang(&mut doc, &o.title, lang);
    Ok((doc, key_pages, in_english, notes))
}

/// The board of `item` drawn in `cell` of `page`: the export's pieces and style, from the side the
/// book prints it.
fn board_ops(doc: &mut Document, page: usize, item: &Item, cell: &Cell, o: &ChessExportOptions) -> Result<(), String> {
    let fen = pdfcraft_chess::fen::Fen::parse(&item.fen).map_err(|e| format!("exercise {}: {e}", item.number))?;
    let style = BoardStyle { flipped: item.orientation == pdfcraft_chess::placement::Color::Black, ..o.style };
    let drawing = board_with(fen.placement(), &o.pieces, &style, &[]);
    let size = f64::from(drawing.width.max(drawing.height));
    if !(size.is_finite() && size > 0.0) {
        return Err(format!("exercise {}: the board has no size", item.number));
    }
    let s = cell.side / size;
    // Drawing units run down from the top left; user space up from the bottom left.
    let ops = replace::drawing_ops(&drawing, [s, 0.0, 0.0, -s, cell.x, cell.top]);
    pdfcraft_edit::add_operators(doc, page, &ops).map_err(|e| e.to_string())
}

/// The document's title (`/Info /Title`) and language (`/Lang`, a BCP 47 tag), as the exports
/// say theirs.
fn set_title_and_lang(doc: &mut Document, title: &str, lang: &str) {
    let mut info = pdfcraft_cos::Dict::new();
    info.set(b"Title".to_vec(), PdfString::text(title));
    info.set(b"Producer".to_vec(), PdfString::text("PdfCraft"));
    let r = doc.add(info);
    doc.trailer_mut().set(b"Info".to_vec(), Object::Ref(r));
    if let Some(root) = doc.root()
        && !lang.trim().is_empty()
    {
        let _ = doc.update_dict(root, |d| d.set(b"Lang".to_vec(), PdfString::text(lang.trim())));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdfcraft_chessvision::doc::worksheet::PerPage;

    /// Plan A9-N12a: each board on A4 is at least 50 mm a side, at 4, 6 and 9 a page, inside the
    /// page and its margins; Letter too.
    #[test]
    fn every_board_is_at_least_fifty_millimetres() {
        let mm50 = 50.0 * 72.0 / 25.4;
        for paper in [Paper::A4, Paper::Letter] {
            let (w, h) = paper.size();
            for per_page in [PerPage::Four, PerPage::Six, PerPage::Nine] {
                let layout = Layout { per_page, ..Layout::default() };
                let cells = cells(paper, &layout);
                assert_eq!(cells.len(), per_page.count());
                for c in &cells {
                    assert!(c.side >= mm50, "{paper:?} {per_page:?}: {} pt", c.side);
                    assert!(c.x >= MARGIN && c.x + c.side <= w - MARGIN, "{paper:?} {per_page:?}: {c:?}");
                    assert!(c.top <= h - MARGIN && c.top - c.side - CAPTION_ROOM >= MARGIN - 0.01, "{paper:?} {per_page:?}: {c:?}");
                }
            }
        }
    }

    /// The decoded content of page `page` of `doc`, its streams joined.
    fn page_content(doc: &Document, page: usize) -> Vec<u8> {
        let Some(p) = pdfcraft_model::pages(doc).into_iter().nth(page) else { return Vec::new() };
        let refs: Vec<pdfcraft_cos::ObjRef> = match p.dict.get(b"Contents") {
            Some(Object::Ref(r)) => vec![*r],
            Some(Object::Array(a)) => a.iter().filter_map(|o| if let Object::Ref(r) = o { Some(*r) } else { None }).collect(),
            _ => Vec::new(),
        };
        let mut out = Vec::new();
        for r in refs {
            if let Object::Stream(s) = &*doc.get(r) {
                out.extend(s.decoded().unwrap_or_default());
                out.push(b'\n');
            }
        }
        out
    }

    fn find(haystack: &[u8], needle: &[u8]) -> bool {
        !needle.is_empty() && haystack.windows(needle.len()).any(|w| w == needle)
    }

    /// Plan A9-N12a: each board is the Redraw's drawing of the exercise's position, in its cell; a
    /// diagram the book prints from Black's side is drawn from Black's side; the key has a page of
    /// its own.
    #[test]
    fn each_board_is_the_redraws_and_a_black_one_is_drawn_from_blacks_side() {
        use pdfcraft_chess::placement::Color;
        use pdfcraft_chessvision::doc::DiagramAt;
        const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
        const ROOK: &str = "8/8/4k3/8/8/4K3/r7/7R w - - 0 1";
        let o = ChessExportOptions::new("Sheet", None);
        let item = |n: u32, fen: &str, orientation| Item {
            at: DiagramAt::new(0, n as usize),
            number: n,
            fen: fen.to_owned(),
            orientation,
            caption: format!("Nº {n} · p. 1 · White to move"),
            caption_en: format!("Nº {n} · p. 1 · White to move"),
            solution: Some("1.e4".to_owned()),
            side_assumed: false,
            lang: pdfcraft_chess::san::Lang::En,
        };
        let items = vec![item(1, START, Color::White), item(2, ROOK, Color::Black)];
        let sheet = Worksheet {
            pages: vec![items.clone()],
            solutions: vec![("Nº 1 · p. 1: 1.e4".into(), "Nº 1 · p. 1: 1.e4".into())],
            left_out: Vec::new(),
            over: 0,
        };
        let (doc, key_pages, in_english, notes) = sheet_document(&sheet, &SheetOptions::default(), &o, "en").unwrap();
        assert_eq!((pdfcraft_model::pages(&doc).len(), key_pages, in_english), (2, 1, 0), "{notes:?}");
        let content = page_content(&doc, 0);
        let cells = cells(Paper::A4, &Layout::default());
        let ops = |item: &Item, cell: &Cell, flipped: bool| {
            let fen = pdfcraft_chess::fen::Fen::parse(&item.fen).unwrap();
            let drawing = board_with(fen.placement(), &o.pieces, &BoardStyle { flipped, ..o.style }, &[]);
            let s = cell.side / f64::from(drawing.width.max(drawing.height));
            replace::drawing_ops(&drawing, [s, 0.0, 0.0, -s, cell.x, cell.top])
        };
        assert!(find(&content, &ops(&items[0], &cells[0], false)), "the first board is the Redraw's");
        assert!(find(&content, &ops(&items[1], &cells[1], true)), "the second is drawn from Black's side");
        assert!(!find(&content, &ops(&items[1], &cells[1], false)), "and not from White's");
    }

    /// Decision (B) of the card: a caption Helvetica cannot write uses the build's own font when it
    /// has one, else goes in English; a build without craft-fonts makes the sheet all the same.
    #[test]
    fn a_caption_no_font_writes_goes_in_english() {
        use pdfcraft_chess::placement::Color;
        use pdfcraft_chessvision::doc::DiagramAt;
        let o = ChessExportOptions::new("Sheet", None);
        let item = Item {
            at: DiagramAt::new(0, 0),
            number: 1,
            fen: "8/8/4k3/8/8/4K3/r7/7R w - - 0 1".to_owned(),
            orientation: Color::White,
            caption: "Nº 1 · p. 1 · Ход белых".to_owned(),
            caption_en: "Nº 1 · p. 1 · White to move".to_owned(),
            solution: None,
            side_assumed: false,
            lang: pdfcraft_chess::san::Lang::Ru,
        };
        let sheet = Worksheet { pages: vec![vec![item]], solutions: Vec::new(), left_out: Vec::new(), over: 0 };
        let (doc, _, in_english, notes) = sheet_document(&sheet, &SheetOptions::default(), &o, "ru").unwrap();
        assert_eq!(pdfcraft_model::pages(&doc).len(), 1);
        // Without craft-fonts no font of the build writes Cyrillic; with them it depends on the faces.
        if pdfcraft_fonts::CRAFT_FONTS.is_empty() {
            assert_eq!(in_english, 1, "{notes:?}");
            assert!(notes.iter().any(|n| n.contains("English")), "{notes:?}");
        }
    }

    #[test]
    fn a_long_key_line_is_wrapped_between_words() {
        let line = "Nº 1 · p. 7: ".to_owned() + &"1.e4 e5 ".repeat(40);
        let lines = wrap(&line, KEY_SIZE, 300.0);
        assert!(lines.len() > 1, "{lines:?}");
        assert!(lines.iter().all(|l| pdfcraft_fonts::helvetica_width(l, KEY_SIZE) <= 300.0), "{lines:?}");
        assert_eq!(lines.join(" "), line.split_whitespace().collect::<Vec<_>>().join(" "));
    }
}
