//! Chess ▸ the book's exercises as a sheet to print (plan A9-N12a, finding A8-14): a **new PDF**,
//! never the book (which does not change), of A4 or Letter pages, each a grid of 4, 6 or 9
//! diagrams with their captions, then the solutions in the order of the numbers.
//!
//! What goes on it is [`pdfcraft_chessvision::doc::worksheet`]'s, from the reading the session
//! (or the disk) holds, as for every export. Each board is the Redraw's vector drawing
//! ([`pdfcraft_chess::draw::board_with`] → [`super::replace::drawing_ops`]) with the export's
//! pieces and board style, from the side the book prints it. The text is Helvetica (WinAnsi), in
//! each page's one content stream with its boards (plan A9-N12a2: the time grows with the pages,
//! not with their square). A caption Helvetica cannot write (a language outside WinAnsi) is added
//! text ([`pdfcraft_edit::added`]) in the build's own font (`craft-fonts`, optional) when one
//! draws it, else goes in English. The file gets its own `/ID`, its title and its language.

use std::fmt::Write as _;
use std::io::Write as _;
use std::sync::Arc;

use pdfcraft_chess::draw::{BoardStyle, board_with};
use pdfcraft_chessvision::doc::markdown::PgnBook;
use pdfcraft_chessvision::doc::reference::{self, ReferenceIndex};
use pdfcraft_chessvision::doc::worksheet::{self, Item, Layout, Worksheet};
use pdfcraft_cos::{Document, Object, PdfString};
use pdfcraft_edit::{AddedText, Align, Content as AddedContent};

use super::document::{OcrMode, OcrOptions};
use super::export::{ChessExport, ChessExportOptions};
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
    /// Captions and key entries written in English: no font of this build writes the book's
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

impl MadeSheet {
    /// The sheet as the other exports' result (plan A9-N12b: the app writes it and says what it
    /// noted as it does theirs): its bytes, its reading and notes, and its exercises as
    /// [`ChessExport::exercises`] — each with the moves of its solution's main line (a word each of
    /// its key line), 0 for one whose solution was not read (decision D30) — and the numbered
    /// diagrams left out.
    pub fn into_export(self) -> ChessExport {
        use pdfcraft_chessvision::doc::exercises::{Exercise, ExercisesPgn};
        let exercises = self
            .sheet
            .items()
            .map(|i| Exercise { at: i.at, number: i.number, moves: i.solution.as_deref().map_or(0, |s| s.split_whitespace().count()) })
            .collect();
        let drawn = self.sheet.items().count();
        let listed = ExercisesPgn { pgn: String::new(), exercises, left_out: self.sheet.left_out.clone(), warnings: Vec::new() };
        ChessExport {
            bytes: Arc::try_unwrap(self.bytes).unwrap_or_else(|shared| (*shared).clone()),
            images: Vec::new(),
            read: self.read,
            drawn,
            position_games: None,
            positions_skipped: Vec::new(),
            pgn_warnings: Vec::new(),
            annotated: None,
            exercises: Some(Box::new(listed)),
            notes: self.notes,
            linked: Vec::new(),
            engine_comments: None,
        }
    }
}

/// What a sheet of a book's exercises would hold, counted from what is already read
/// ([`Session::chess_worksheet_count`], plan A9-N12b).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SheetCount {
    /// The exercises that go on it, of the pages counted.
    pub exercises: usize,
    /// Of those, the ones whose solution was not read: on it all the same, the key says so (D30).
    pub unsolved: usize,
    /// Numbered diagrams left out: no position to start from.
    pub no_position: usize,
    /// Exercises past [`worksheet::MAX_EXERCISES`].
    pub over: usize,
    /// The pages counted: read already, their text layer believed.
    pub pages_counted: usize,
    /// The pages chosen not counted: not read yet, or whose text the export reads with chess OCR.
    /// The export counts them.
    pub pages_not_read: usize,
}

impl Session {
    /// `pages` of document `id` read as every export reads them, with chess OCR as `ocr` says
    /// (and the pages before looked at as `look_before` says, [`Session::chess_document_of`]), the
    /// book's language as the user says it and linked to `o`'s references; and the book they
    /// make, as the sheet names it.
    fn sheet_reading(
        &mut self,
        id: DocId,
        pages: &[usize],
        o: &ChessExportOptions,
        ocr: &OcrOptions<'_>,
        look_before: bool,
    ) -> Result<(ChessDocument, PgnBook), String> {
        let mut read = self.chess_document_of(id, pages, &o.scan, o.lang, ocr, look_before)?;
        // The book's language as the user says it (`text_lang`): the captions' and the key's,
        // page by page, and the sheet's `/Lang` (plan A9-N12a2; it was ignored).
        super::export::say_text_lang(&mut read, o);
        let refs: Vec<&ReferenceIndex> = [o.games.as_deref(), o.reference.as_deref()].into_iter().flatten().collect();
        if !refs.is_empty() {
            reference::link_all(&mut read.doc, &refs);
        }
        let labels: Vec<String> = self.get(id).map(|d| d.info.pages.iter().map(|p| p.label.clone()).collect()).unwrap_or_default();
        Ok((read, PgnBook { title: Some(o.title.clone()).filter(|t| !t.trim().is_empty()), labels }))
    }

    /// The sheet of document `id`'s exercises (plan A9-N12a): the pages `o` names read as every
    /// export reads them (linked to its references), laid out as `sheet` says, drawn with `o`'s
    /// pieces and board style, titled `o.title`, in the book's language. The document itself is
    /// not changed.
    pub fn chess_worksheet(&mut self, id: DocId, o: &ChessExportOptions, sheet: &SheetOptions) -> Result<MadeSheet, String> {
        let ocr = OcrOptions { mode: o.ocr, glyphs: None, general: true, book: o.book.as_ref() };
        let (read, book) = self.sheet_reading(id, &o.pages, o, &ocr, true)?;
        let made = worksheet::worksheet(&read.doc, &book, &sheet.layout);
        let book_lang = if read.text_lang.is_empty() { read.lang.lang.code().to_owned() } else { read.text_lang.as_str().to_owned() };
        let (doc, key_pages, in_english, notes) = sheet_document(&made, sheet, o, &book_lang)?;
        let bytes = self.write_new(&doc).map_err(|e| e.to_string())?;
        Ok(MadeSheet { bytes, diagram_pages: made.pages.len(), key_pages, in_english, notes, sheet: made, read })
    }

    /// What the sheet of document `id`'s exercises would hold (plan A9-N12b, for the export
    /// dialog), from what is **already read** of the pages `o` names: those a reading scan finds
    /// in this session or on disk ([`Session::chess_held`]), their text layers read, never chess
    /// OCR. Nothing is scanned, so an unread book is never read for a count (plan A9-N3's rule); a
    /// page not read, or whose layer the export would read with chess OCR (`o.ocr`), is counted
    /// in [`SheetCount::pages_not_read`] and left to the export.
    pub fn chess_worksheet_count(&mut self, id: DocId, o: &ChessExportOptions, layout: &Layout) -> Result<SheetCount, String> {
        let doc = self.get(id).ok_or("no such document")?;
        doc.check_copying().map_err(|e| e.to_string())?;
        let chosen = pdfcraft_chessvision::doc::PageSet::new(&o.pages, doc.info.pages.len())?.pages().len();
        let not_read =
            |counted: usize| SheetCount { pages_counted: counted, pages_not_read: chosen.saturating_sub(counted), ..SheetCount::default() };
        // Every page read with chess OCR: none is read until the export.
        if o.ocr == OcrMode::All {
            return Ok(not_read(0));
        }
        let held = self.chess_held(id, &o.pages, &o.scan);
        if held.is_empty() {
            return Ok(not_read(0));
        }
        let ocr = OcrOptions { mode: OcrMode::Off, glyphs: None, general: false, book: o.book.as_ref() };
        let (read, book) = self.sheet_reading(id, &held, o, &ocr, false)?;
        // A layer the export reads with chess OCR is not believed here.
        let poor: Vec<usize> = if o.ocr == OcrMode::Auto { read.needs_ocr.iter().map(|(p, _)| *p).collect() } else { Vec::new() };
        let made = worksheet::worksheet(&read.doc, &book, layout);
        let counted = |page: usize| !poor.contains(&page);
        let items: Vec<&Item> = made.items().filter(|i| counted(i.at.page)).collect();
        Ok(SheetCount {
            exercises: items.len(),
            unsolved: items.iter().filter(|i| i.solution.is_none()).count(),
            no_position: made.left_out.iter().filter(|l| counted(l.at.page)).count(),
            over: made.over,
            ..not_read(held.iter().filter(|p| counted(**p)).count())
        })
    }
}

/// How a text goes on the sheet.
#[derive(Clone, Debug, PartialEq)]
enum Written {
    /// In Helvetica (WinAnsi), in the page's one content stream with its boards.
    Helvetica(String),
    /// In a font of this build (`craft-fonts`) that draws it: added text, one item at a time.
    Face(String),
}

/// How `text` is written at `size`: in Helvetica when WinAnsi holds it, else in a font of this
/// build that draws it, else as `english` in Helvetica. Whether it went in English.
fn written(text: &str, english: &str, size: f64) -> (Written, bool) {
    if pdfcraft_fonts::first_non_win_ansi(text).is_none() {
        return (Written::Helvetica(text.to_owned()), false);
    }
    let probe = AddedText { text: text.to_owned(), size, ..AddedText::default() };
    if pdfcraft_edit::first_undrawable(&probe).is_none() {
        return (Written::Face(text.to_owned()), false);
    }
    (Written::Helvetica(english.to_owned()), true)
}

/// A number for a content stream: two decimals, 0 for one that is not finite.
fn num(v: f64) -> String {
    let v = if v.is_finite() { v } else { 0.0 };
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// The operators writing `text` in Helvetica ([`HELVETICA`]) at `size` on the baseline `y`: from
/// `x0`, or centred between `x0` and `x1`.
fn text_ops(out: &mut Vec<u8>, text: &str, size: f64, (x0, x1): (f64, f64), y: f64, align: Align) {
    let width = pdfcraft_fonts::helvetica_width(text, size);
    let x = if align == Align::Center { x0 + ((x1 - x0) - width) / 2.0 } else { x0 };
    let mut hex = String::with_capacity(text.len().saturating_mul(2));
    for b in pdfcraft_fonts::win_ansi(text) {
        let _ = write!(hex, "{b:02X}");
    }
    let _ = write!(out, "BT\n/{HELVETICA} {} Tf\n{} {} Td\n<{hex}> Tj\nET\n", num(size), num(x), num(y));
}

/// The font resource name the sheet's Helvetica goes by.
const HELVETICA: &str = "PCHelv";

/// The baseline of a line of `size` points whose box's top is `top`: an ascent below it.
fn baseline(top: f64, size: f64) -> f64 {
    top - 0.8 * size
}

/// A text left for added text ([`Written::Face`]): its page, box, size, alignment, text and
/// English.
struct Faced {
    page: usize,
    rect: [f64; 4],
    size: f64,
    align: Align,
    text: String,
    english: String,
}

/// The key's entries laid out in pages of `per_page` lines, each entry whole on one page when it
/// fits: every line of an entry in one language (plan A9-N12a2: a native line beside an English
/// one could mix two entries' words), wrapped to `width`. How many entries went in English.
fn key_pages(solutions: &[(String, String)], width: f64, per_page: usize) -> (Vec<Vec<(Written, String)>>, usize) {
    let per_page = per_page.max(1);
    let mut pages: Vec<Vec<(Written, String)>> = Vec::new();
    let mut page: Vec<(Written, String)> = Vec::new();
    let mut in_english = 0usize;
    for (line, english) in solutions {
        let (how, en) = written(line, english, KEY_SIZE);
        in_english = in_english.saturating_add(usize::from(en));
        let lines: Vec<(Written, String)> = match how {
            Written::Helvetica(t) => wrap(&t, KEY_SIZE, width).into_iter().map(|l| (Written::Helvetica(l), String::new())).collect(),
            // Added text wraps itself; its English is wrapped in case the face fails it.
            Written::Face(t) => vec![(Written::Face(t), english.clone())],
        };
        if !page.is_empty() && page.len().saturating_add(lines.len()) > per_page {
            pages.push(std::mem::take(&mut page));
        }
        for l in lines {
            if page.len() >= per_page {
                pages.push(std::mem::take(&mut page));
            }
            page.push(l);
        }
    }
    if !page.is_empty() {
        pages.push(page);
    }
    (pages, in_english)
}

/// The sheet as a new document: its diagram pages, then the key's; the number of key pages, how
/// many texts went in English, and the notes. Each page is one content stream, its boards and
/// its text in Helvetica together (plan A9-N12a2: one added item at a time read every item
/// written before again, which a sheet of thousands took minutes for); only a text a font of
/// this build draws instead is added text, item by item. Refused when there is no exercise to
/// put on it.
fn sheet_document(
    sheet: &Worksheet,
    options: &SheetOptions,
    o: &ChessExportOptions,
    lang: &str,
) -> Result<(Document, usize, usize, Vec<String>), String> {
    if sheet.pages.iter().all(Vec::is_empty) {
        let without = sheet.left_out.len();
        let why = if without > 0 { format!(" ({without} numbered diagrams have no position to solve from)") } else { String::new() };
        return Err(format!("no exercise to put on the sheet: the pages read have no numbered diagram with a position{why}"));
    }
    let (w, h) = options.paper.size();
    let cells = cells(options.paper, &options.layout);
    let per_page = ((h - 2.0 * MARGIN) / KEY_LINE).floor().max(1.0) as usize;
    let (key, mut in_english) = key_pages(&sheet.solutions, w - 2.0 * MARGIN, per_page);
    let total = sheet.pages.len().saturating_add(key.len());
    let mut doc = pdfcraft_create::blank(w, h, total).map_err(|e| e.to_string())?;
    let pages: Vec<pdfcraft_cos::ObjRef> = pdfcraft_model::pages(&doc).into_iter().map(|p| p.obj).collect();
    let resources = helvetica_resources(&mut doc);
    let mut faced: Vec<Faced> = Vec::new();
    for (p, items) in sheet.pages.iter().enumerate() {
        let mut ops = Vec::new();
        for (item, cell) in items.iter().zip(&cells) {
            ops.extend(board_ops(item, cell, o)?);
            let rect = [
                cell.x + (cell.side - cell.width) / 2.0,
                cell.top - cell.side - CAPTION_ROOM,
                cell.x + (cell.side + cell.width) / 2.0,
                cell.top - cell.side - 4.0,
            ];
            let (how, en) = written(&item.caption, &item.caption_en, CAPTION_SIZE);
            in_english = in_english.saturating_add(usize::from(en));
            match how {
                Written::Helvetica(t) => text_ops(&mut ops, &t, CAPTION_SIZE, (rect[0], rect[2]), baseline(rect[3], CAPTION_SIZE), Align::Center),
                Written::Face(text) => {
                    faced.push(Faced { page: p, rect, size: CAPTION_SIZE, align: Align::Center, text, english: item.caption_en.clone() })
                }
            }
        }
        set_content(&mut doc, pages.get(p).copied(), &ops, &resources)?;
    }
    for (k, lines) in key.iter().enumerate() {
        let page = sheet.pages.len().saturating_add(k);
        let mut ops = Vec::new();
        for (i, (how, english)) in lines.iter().enumerate() {
            let top = h - MARGIN - i as f64 * KEY_LINE;
            match how {
                Written::Helvetica(t) => text_ops(&mut ops, t, KEY_SIZE, (MARGIN, w - MARGIN), baseline(top, KEY_SIZE), Align::Left),
                Written::Face(text) => faced.push(Faced {
                    page,
                    rect: [MARGIN, top - KEY_LINE, w - MARGIN, top],
                    size: KEY_SIZE,
                    align: Align::Left,
                    text: text.clone(),
                    english: english.clone(),
                }),
            }
        }
        set_content(&mut doc, pages.get(page).copied(), &ops, &resources)?;
    }
    // What a font of this build draws: added text, which reads the page's items again each time
    // (only a language Helvetica cannot write; none of the captions' today).
    for f in &faced {
        let item = AddedContent::Text(AddedText { rect: f.rect, text: f.text.clone(), size: f.size, align: f.align, ..AddedText::default() });
        if pdfcraft_edit::add_content(&mut doc, f.page, &item).is_err() {
            let mut ops = Vec::new();
            text_ops(&mut ops, &f.english, f.size, (f.rect[0], f.rect[2]), baseline(f.rect[3], f.size), f.align);
            pdfcraft_edit::add_operators(&mut doc, f.page, &ops).map_err(|e| e.to_string())?;
            in_english = in_english.saturating_add(1);
        }
    }
    let mut notes = Vec::new();
    if sheet.over > 0 {
        notes.push(format!("{} exercises past the sheet's {} were left out", sheet.over, worksheet::MAX_EXERCISES));
    }
    if in_english > 0 {
        notes.push(format!("{in_english} captions or key entries are in English: no font of this build writes the book's language"));
    }
    set_title_and_lang(&mut doc, &o.title, lang);
    Ok((doc, key.len(), in_english, notes))
}

/// The page resources of the sheet's text: Helvetica, WinAnsi, as [`HELVETICA`].
fn helvetica_resources(doc: &mut Document) -> pdfcraft_cos::Dict {
    let mut font = pdfcraft_cos::Dict::new();
    font.set(b"Type".to_vec(), Object::name("Font"));
    font.set(b"Subtype".to_vec(), Object::name("Type1"));
    font.set(b"BaseFont".to_vec(), Object::name("Helvetica"));
    font.set(b"Encoding".to_vec(), Object::name("WinAnsiEncoding"));
    let r = doc.add(font);
    let mut fonts = pdfcraft_cos::Dict::new();
    fonts.set(HELVETICA.as_bytes().to_vec(), Object::Ref(r));
    let mut resources = pdfcraft_cos::Dict::new();
    resources.set(b"Font".to_vec(), Object::Dict(fonts));
    resources
}

/// `content` as the page `page`'s content, with `resources`.
fn set_content(doc: &mut Document, page: Option<pdfcraft_cos::ObjRef>, content: &[u8], resources: &pdfcraft_cos::Dict) -> Result<(), String> {
    let page = page.ok_or("the sheet has fewer pages than it was made with")?;
    let stream = doc.add(Object::Stream(pdfcraft_cos::Stream::flate(pdfcraft_cos::Dict::new(), content)));
    doc.update_dict(page, |d| {
        d.set(b"Contents".to_vec(), Object::Ref(stream));
        d.set(b"Resources".to_vec(), Object::Dict(resources.clone()));
    })
    .map_err(|e| e.to_string())
}

/// The board of `item` drawn in `cell`: the export's pieces and style, from the side the book
/// prints it; the operators checked as [`pdfcraft_edit::add_operators`] checks them, in their
/// own `q … Q`.
fn board_ops(item: &Item, cell: &Cell, o: &ChessExportOptions) -> Result<Vec<u8>, String> {
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
    pdfcraft_edit::check_operators(&ops).map_err(|e| format!("exercise {}: {e}", item.number))?;
    let mut out = Vec::with_capacity(ops.len().saturating_add(8));
    out.extend_from_slice(b"q\n");
    out.extend_from_slice(&ops);
    out.extend_from_slice(b"\nQ\n");
    Ok(out)
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

    /// `f` run three times: the fastest run's milliseconds, every run's, and the last result. A
    /// loaded machine slows one run, not the work (as `chessvision::timing`, which is that crate's
    /// and test-only).
    fn best_of_three<T>(mut f: impl FnMut() -> T) -> (f64, Vec<f64>, T) {
        let mut runs = Vec::new();
        let mut last = None;
        for _ in 0..3 {
            let t = std::time::Instant::now();
            last = Some(f());
            runs.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        let ms = runs.iter().copied().fold(f64::MAX, f64::min);
        (ms, runs, last.expect("three runs"))
    }

    /// A sheet of `n` exercises, four a page, each with its key line.
    fn sheet_of(n: u32) -> Worksheet {
        use pdfcraft_chess::placement::Color;
        use pdfcraft_chessvision::doc::DiagramAt;
        let items: Vec<Item> = (1..=n)
            .map(|k| Item {
                at: DiagramAt::new(k as usize / 4, k as usize % 4),
                number: k,
                fen: "8/8/4k3/8/8/4K3/r7/7R w - - 0 1".to_owned(),
                orientation: Color::White,
                caption: format!("Nº {k} · p. {} · White to move", k / 4 + 1),
                caption_en: format!("Nº {k} · p. {} · White to move", k / 4 + 1),
                solution: Some("1.Rh6+ Kd5 2.Rxa2".to_owned()),
                side_assumed: false,
                lang: pdfcraft_chess::san::Lang::En,
            })
            .collect();
        let solutions = items
            .iter()
            .map(|i| (format!("Nº {} · p. 1: 1.Rh6+ Kd5 2.Rxa2", i.number), format!("Nº {} · p. 1: 1.Rh6+ Kd5 2.Rxa2", i.number)))
            .collect();
        Worksheet { pages: items.chunks(4).map(<[Item]>::to_vec).collect(), solutions, left_out: Vec::new(), over: 0 }
    }

    /// Plan A9-N12a2: a sheet of 2,000 exercises (500 pages of boards and the key) is written in
    /// time that grows with its pages, not with their square: each page's text goes in with its
    /// boards, not one item at a time with every item written before read again. Measured with
    /// the best of three runs: 0.31, 0.64, 1.6, 3.4, 12.8 and 78 s for 50, 100, 200, 400, 800 and
    /// 2,000 exercises before; 0.9 s for 200 and 5.9 s for 2,000 after. `cargo test --release -p
    /// pdfcraft-engine --lib perf_worksheet -- --ignored --nocapture`
    /// (`PDFCRAFT_WORKSHEET_PERF_N` for another count).
    #[test]
    #[ignore = "a timing, in release"]
    fn perf_worksheet_of_two_thousand_exercises() {
        let n = std::env::var("PDFCRAFT_WORKSHEET_PERF_N").ok().and_then(|v| v.parse().ok()).unwrap_or(2000u32);
        let sheet = sheet_of(n);
        let o = ChessExportOptions::new("Sheet", None);
        let (ms, runs, made) = best_of_three(|| sheet_document(&sheet, &SheetOptions::default(), &o, "en"));
        let (doc, key_pages, in_english, _) = made.unwrap();
        println!(
            "PERF perf_worksheet_of_two_thousand_exercises {ms:.0}\n  {n} exercises, {} pages ({key_pages} of key), runs {runs:.0?}",
            pdfcraft_model::pages(&doc).len()
        );
        assert_eq!((pdfcraft_model::pages(&doc).len(), in_english), ((n as usize).div_ceil(4) + key_pages, 0));
        // 10 s for 2,000 (5.9 s here, on a machine other builds load; 78 s before A9-N12a2),
        // failing past twice that, as chessvision's timings do.
        let budget = 10_000.0 * f64::from(n) / 2000.0;
        assert!(ms < 2.0 * budget, "best of three {ms:.0} ms ({runs:.0?}), budget {budget:.0} ms");
    }

    /// Each page's text as the renderer extracts it from the written file.
    fn texts(doc: &Document) -> Vec<String> {
        let bytes = Session::new().write_new(doc).unwrap();
        let mut r = pdfcraft_render::PageRenderer::new(bytes, pdfcraft_render::RenderConfig::default());
        (0..pdfcraft_model::pages(doc).len())
            .map(|page| {
                let request = pdfcraft_render::RenderRequest { page, kind: pdfcraft_render::RequestKind::Text, scale: 1.0, ..Default::default() };
                r.render(request).text.map(|t| t.plain_text()).unwrap_or_default()
            })
            .collect()
    }

    /// Plan A9-N12a2: the written sheet's text reads back: each caption under its board, and the
    /// key.
    #[test]
    fn the_captions_and_the_key_read_back_from_the_file() {
        let o = ChessExportOptions::new("Sheet", None);
        let (doc, key_pages, in_english, _) = sheet_document(&sheet_of(5), &SheetOptions::default(), &o, "en").unwrap();
        assert_eq!((key_pages, in_english), (1, 0));
        let pages = texts(&doc);
        assert_eq!(pages.len(), 3, "{pages:?}");
        assert!(pages[0].contains("Nº 1 · p. 1 · White to move") && pages[0].contains("Nº 4 · p. 2 · White to move"), "{:?}", pages[0]);
        assert!(pages[1].contains("Nº 5 · p. 2 · White to move"), "{:?}", pages[1]);
        assert!(pages[2].contains("Nº 5 · p. 1: 1.Rh6+ Kd5 2.Rxa2"), "{:?}", pages[2]);
    }

    /// Plan A9-N12a2: a sheet with no exercise to put on it is refused, saying why, instead of a
    /// blank page reported as made.
    #[test]
    fn a_sheet_with_no_exercise_is_refused() {
        let o = ChessExportOptions::new("Sheet", None);
        let err = sheet_document(&Worksheet::default(), &SheetOptions::default(), &o, "en").err();
        assert!(err.as_deref().is_some_and(|e| e.contains("no exercise")), "{err:?}");
    }

    /// Plan A9-N12a2: a key entry is written in one language. One whose own text has a letter no
    /// font of this build writes goes whole in English, never its first lines as written and the
    /// rest from the English, which wraps elsewhere.
    #[test]
    fn a_key_entry_is_written_in_one_language() {
        let o = ChessExportOptions::new("Sheet", None);
        let mut sheet = sheet_of(1);
        let native = format!("Nº 1 · p. 1: {}4.Кf3 Кf6", "1.e4 e5 2.d4 d5 3.c4 c6 ".repeat(6));
        let english = "Nº 1 · p. 1: the entry in English".to_owned();
        let probe = AddedText { text: native.clone(), size: KEY_SIZE, ..AddedText::default() };
        if pdfcraft_edit::first_undrawable(&probe).is_none() {
            // A font of this build writes Cyrillic: nothing goes in English.
            return;
        }
        assert!(wrap(&native, KEY_SIZE, Paper::A4.size().0 - 2.0 * MARGIN).len() > 1, "its own text takes more lines");
        sheet.solutions = vec![(native, english)];
        let (doc, key_pages, in_english, _) = sheet_document(&sheet, &SheetOptions::default(), &o, "ru").unwrap();
        assert_eq!((key_pages, in_english), (1, 1));
        let key = texts(&doc).pop().unwrap_or_default();
        assert!(key.contains("the entry in English"), "{key:?}");
        assert!(!key.contains("1.e4 e5"), "none of its own lines beside it: {key:?}");
    }

    /// A PDF of a page per `(caption, solution)`: the caption in Helvetica, the start position in
    /// the Chess Merida font under it, and the solution under the board.
    fn exercise_book(pages: &[(&str, Option<&str>)]) -> Vec<u8> {
        const MERIDA_START: [&str; 8] = ["tMvWlVmT", "OoOoOoOo", " + + + +", "+ + + + ", " + + + +", "+ + + + ", "pPpPpPpP", "RnBqKbNr"];
        let widths = vec!["1000"; 95].join(" ");
        let n = pages.len();
        let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 5 + 2 * i)).collect();
        let mut objs: Vec<String> = vec![
            "<< /Type /Catalog /Pages 2 0 R >>".into(),
            format!("<< /Type /Pages /Kids [{}] /Count {n} /MediaBox [0 0 600 800] >>", kids.join(" ")),
            format!("<< /Type /Font /Subtype /Type1 /BaseFont /ChessMerida /FirstChar 32 /LastChar 126 /Widths [{widths}] >>"),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".into(),
        ];
        for (i, (caption, solution)) in pages.iter().enumerate() {
            let mut c = format!("BT /F2 11 Tf 60 755 Td ({caption}) Tj ET\nBT /F1 20 Tf 60 724 Td ");
            for (k, line) in MERIDA_START.iter().enumerate() {
                c.push_str(if k > 0 { "0 -20 Td " } else { "" });
                c.push_str(&format!("({line}) Tj "));
            }
            c.push_str("ET\n");
            if let Some(moves) = solution {
                c.push_str(&format!("BT /F2 11 Tf 60 540 Td ({moves}) Tj ET\n"));
            }
            objs.push(format!("<< /Type /Page /Parent 2 0 R /Contents {} 0 R /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> >>", 6 + 2 * i));
            objs.push(format!("<< /Length {} >>\nstream\n{c}\nendstream", c.len()));
        }
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (i, o) in objs.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
        }
        let xref = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
        for o in offsets {
            out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
        out
    }

    /// Plan A9-N12b (the coordinator's rule, 2026-11-16: choosing the format never reads an unread
    /// book): the count is of the pages already read, and it scans nothing — not even the page
    /// before a page read, whose foot a caption may be on. The rest is said as not read, and once
    /// every page is read the count is the sheet's: three solved, and the one whose solution was
    /// not read (D30). With every page read by chess OCR, nothing is counted.
    #[test]
    fn the_count_is_of_the_pages_read_and_scans_nothing() {
        let pages =
            [("Exercise 1", Some("1.e4 e5 2.Nf3 Nc6")), ("Exercise 2", Some("1.d4 d5 2.c4")), ("Exercise 3", Some("1.c4 c5")), ("Exercise 4", None)];
        let mut s = Session::new();
        let id = s.open("exercises.pdf", None, Arc::new(exercise_book(&pages)), None).unwrap();
        let mut o = ChessExportOptions::new("Exercises", None);
        o.scan = crate::chess::tests::no_model();
        o.lang = Some(pdfcraft_chess::san::Lang::En);
        let layout = Layout::default();
        let count = s.chess_worksheet_count(id, &o, &layout).unwrap();
        assert_eq!((count, s.chess_scanned()), (SheetCount { pages_not_read: 4, ..SheetCount::default() }, 0), "nothing read, nothing scanned");
        // Pages 2 and 3 read: page 1, before them, is not looked at for the count.
        s.chess_scan(id, &[1, 2], &o.scan, true).unwrap();
        let scanned = s.chess_scanned();
        let count = s.chess_worksheet_count(id, &o, &layout).unwrap();
        assert_eq!(s.chess_scanned(), scanned, "the count scanned nothing");
        assert_eq!(count, SheetCount { exercises: 2, pages_counted: 2, pages_not_read: 2, ..SheetCount::default() });
        s.chess_scan(id, &[], &o.scan, true).unwrap();
        let count = s.chess_worksheet_count(id, &o, &layout).unwrap();
        assert_eq!(count, SheetCount { exercises: 4, unsolved: 1, pages_counted: 4, ..SheetCount::default() });
        let made = s.chess_worksheet(id, &o, &SheetOptions::default()).unwrap();
        assert_eq!(made.sheet.items().count(), count.exercises, "the export puts on the sheet what the count said");
        let export = made.into_export();
        let listed = export.exercises.as_deref().map(|e| e.exercises.iter().map(|x| (x.number, x.moves)).collect::<Vec<_>>());
        assert_eq!(listed, Some(vec![(1, 4), (2, 3), (3, 2), (4, 0)]), "the sheet as an export: each exercise's moves, 0 unsolved");
        // Chosen pages only; every page read by chess OCR is left to the export.
        o.pages = vec![0, 3];
        assert_eq!(
            s.chess_worksheet_count(id, &o, &layout).unwrap(),
            SheetCount { exercises: 2, unsolved: 1, pages_counted: 2, ..SheetCount::default() }
        );
        o.ocr = OcrMode::All;
        assert_eq!(s.chess_worksheet_count(id, &o, &layout).unwrap(), SheetCount { pages_not_read: 2, ..SheetCount::default() });
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
