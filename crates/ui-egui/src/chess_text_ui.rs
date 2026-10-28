//! Chess ▸ Recognised text (blueprint S16): beside the page, what was read from it, and the
//! chess exports.
//!
//! The Chess diagrams panel has a second tab, **Text**: the current page read as a chess
//! document (`Session::chess_document_with`): headings, paragraphs whose moves are coloured by
//! what replaying them found (valid, illegal or ambiguous, not reached, unchecked), annotation
//! glyphs, and the diagrams as small boards. It follows the page shown (reading its text layer,
//! which is quick); a page whose layer cannot be believed is read with chess OCR on request.
//! **Book origin** is the user's say over the whole document: Auto (the above, and exports read
//! with chess OCR where a layer cannot be believed), Digital (never chess OCR: a poor layer is
//! read as it is, with a note) or Scanned (every page and export read with chess OCR). It is kept
//! with the document like its book cipher.
//! Clicking a move, a paragraph or a diagram outlines where it is on the page; a diagram also
//! opens in the editor, and a double-click on a move opens its game on the board
//! (`chess_source`): the pages around it are read for that too, off the interface thread
//! (`engine::chess::span`), so a game the book runs on from the page before or onto the page
//! after comes whole. A move with no position on its page alone (the top of a page going on
//! from the page before) opens the game the pages before give it. Figurines are shown when the
//! interface's fonts have them, else the moves as printed.
//!
//! **Export…** writes the whole document as HTML, EPUB, Word, RTF, Markdown (with its diagram
//! images in a folder beside it) or PGN, through the engine's `chess_export`.

use std::sync::{Arc, Mutex};

use egui::{Color32, FontId, TextureHandle};
use pdfcraft_engine::DocId;
use pdfcraft_engine::chess::ScanStamp;
use pdfcraft_engine::chess::document::{BookOrigin, OcrMode, OcrOptions};
use pdfcraft_engine::chess::export::{ChessExportOptions, ChessFormat};
use pdfcraft_engine::chess::pdfcraft_chess::draw::BoardStyle;
use pdfcraft_engine::chess::pdfcraft_chessvision::doc::html::{Figurines, plain_text_with};
use pdfcraft_engine::chess::pdfcraft_chessvision::doc::{Block, ChessDoc, Inline, MoveStatus, UserRect};
use pdfcraft_engine::chess::span::{GameSpanRead, MoveAt};

use crate::theme::Tokens;
use crate::{PdfCraftApp, theme};

/// The panel's tabs. The study board that was a third tab is the Board panel since T5c (D11 b);
/// the tab is not saved, so no setting names it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChessTab {
    #[default]
    Diagrams,
    Text,
}

/// One page read as text.
pub struct PageText {
    pub doc: DocId,
    pub stamp: ScanStamp,
    pub page: usize,
    /// The book origin the page was read under.
    pub origin: BookOrigin,
    pub chess: ChessDoc,
    /// Read with chess OCR (not the text layer).
    pub ocr: bool,
    /// Why the page's layer cannot be believed, when it cannot (and OCR did not read it).
    pub needs_ocr: Option<String>,
    pub notes: Vec<String>,
    /// The diagrams' boards, by block index.
    boards: Vec<(usize, TextureHandle)>,
}

/// Reading a page or exporting the book, off the interface thread: the work runs on a session
/// of its own, opened from the document's working bytes, and the result is picked up when it is
/// done (inline on the web and in tests).
pub struct ChessJob {
    pub doc: DocId,
    pub kind: JobKind,
    result: Arc<Mutex<Option<JobDone>>>,
}

/// What a job does.
#[derive(Clone, Debug, PartialEq)]
pub enum JobKind {
    Read {
        page: usize,
        stamp: ScanStamp,
        origin: BookOrigin,
        ocr: bool,
    },
    /// The game of a move of the page read, with the pages around it (`chess_source`).
    Game {
        at: MoveAt,
        stamp: ScanStamp,
    },
    #[cfg(not(target_arch = "wasm32"))]
    Export {
        format: ChessFormat,
        path: std::path::PathBuf,
        image_dir: String,
    },
    #[cfg(target_arch = "wasm32")]
    Export {
        format: ChessFormat,
        name: String,
    },
}

enum JobDone {
    Read(Result<pdfcraft_engine::chess::document::ChessDocument, String>),
    Game(Result<GameSpanRead, String>),
    Export(Result<pdfcraft_engine::chess::export::ChessExport, String>),
}

impl ChessJob {
    /// What the panel says while it runs.
    pub fn label(&self) -> &'static str {
        match self.kind {
            JobKind::Read { ocr: true, .. } => tl!("Reading the page with chess OCR…"),
            JobKind::Read { .. } => tl!("Reading the page…"),
            JobKind::Game { .. } => tl!("Reading the pages around the game…"),
            JobKind::Export { .. } => tl!("Exporting the chess document…"),
        }
    }
}

/// The lock, even after a worker panicked while holding it (it holds plain data).
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// What the text tab asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum TextAction {
    /// Read the current page (with chess OCR when `true`).
    Read(bool),
    /// Outline a box of `page` (user space) on the page.
    Show(usize, [f32; 4]),
    /// Open diagram `index` of `page` in the editor.
    Diagram(usize, usize),
    /// Put the game the move belongs to on the board, at that move, the pages around its page
    /// read too (`chess_source`).
    OpenGame(MoveAt),
    Export(ChessFormat),
    /// Say where the document's pages came from.
    Origin(BookOrigin),
}

fn origin_label(o: BookOrigin) -> &'static str {
    match o {
        BookOrigin::Auto => tl!("Auto (page by page)"),
        BookOrigin::Digital => tl!("Born digital (text layer only)"),
        BookOrigin::Scanned => tl!("Scanned (chess OCR on every page)"),
    }
}

/// The Book origin choice.
fn origin_menu(ui: &mut egui::Ui, origin: BookOrigin) -> Option<TextAction> {
    let mut action = None;
    ui.label(tl!("Book origin"));
    let mut chosen = origin;
    egui::ComboBox::from_id_salt("chess-book-origin")
        .selected_text(origin_label(origin))
        .show_ui(ui, |ui| {
            for o in BookOrigin::ALL {
                ui.selectable_value(&mut chosen, o, origin_label(o));
            }
        })
        .response
        .on_hover_text(tl!(
            "Where the book's pages came from. Auto believes each page's text layer unless it looks wrong; Born digital never uses chess OCR; Scanned reads every page with chess OCR (needs the chess-glyphs model)."
        ));
    if chosen != origin {
        action = Some(TextAction::Origin(chosen));
    }
    action
}

fn status_colour(s: MoveStatus, t: &Tokens) -> Color32 {
    match s {
        MoveStatus::Valid => t.success_text,
        MoveStatus::Illegal | MoveStatus::Ambiguous => t.danger_text,
        MoveStatus::OutOfSequence | MoveStatus::NotReached => t.warning_text,
        MoveStatus::Unchecked => Color32::GRAY,
        MoveStatus::Caption => Color32::from_rgb(0x3B, 0x82, 0xC4),
    }
}

fn status_label(s: MoveStatus) -> &'static str {
    match s {
        MoveStatus::Valid => tl!("Valid move"),
        MoveStatus::Illegal => tl!("Illegal move"),
        MoveStatus::Ambiguous => tl!("Ambiguous move"),
        MoveStatus::OutOfSequence => tl!("Move out of sequence"),
        MoveStatus::NotReached => tl!("Not reached: an earlier move failed"),
        MoveStatus::Unchecked => tl!("Unchecked: no position to play it from"),
        MoveStatus::Caption => tl!("Caption: the move that led to the diagram, which gives it its side to move and move number"),
    }
}

fn rect_of(r: Option<UserRect>) -> Option<[f32; 4]> {
    let r = r?.page_rect();
    [r.x0, r.y0, r.x1, r.y1].iter().all(|v| v.is_finite()).then_some([r.x0, r.y0, r.x1, r.y1])
}

/// How moves are printed here: figurines when the interface's fonts have them.
fn figurines(ctx: &egui::Context) -> Figurines {
    if ctx.fonts_mut(|f| f.has_glyphs(&FontId::proportional(13.0), "♔♕♖♗♘♙")) { Figurines::Unicode } else { Figurines::AsPrinted }
}

/// The Export… menu.
pub fn export_menu(ui: &mut egui::Ui) -> Option<TextAction> {
    ui.menu_button(tl!("Export…"), export_items).inner.flatten()
}

/// The export menu's items, one per format (in a menu that `ui` is).
pub(crate) fn export_items(ui: &mut egui::Ui) -> Option<TextAction> {
    let mut action = None;
    for f in ChessFormat::ALL {
        if ui.button(crate::i18n::fmt(tl!("Export as {f}"), &[("f", f.name())])).clicked() {
            action = Some(TextAction::Export(f));
            ui.close();
        }
    }
    action
}

/// The Text tab for page `page` of document `id`, whose book origin is `origin`.
#[allow(clippy::too_many_arguments)]
pub fn text_tab(
    ui: &mut egui::Ui,
    t: &Tokens,
    text: Option<&mut PageText>,
    id: DocId,
    page: usize,
    now: Option<ScanStamp>,
    origin: BookOrigin,
    busy: Option<&'static str>,
) -> Option<TextAction> {
    let mut action = None;
    if let Some(label) = busy {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(egui::RichText::new(label).color(t.text_muted));
        });
    }
    let current = text.filter(|x| x.doc == id && x.page == page && Some(x.stamp) == now && x.origin == origin);
    ui.horizontal_wrapped(|ui| {
        if let Some(a) = origin_menu(ui, origin) {
            action = Some(a);
        }
    });
    ui.horizontal_wrapped(|ui| {
        if ui
            .button(tl!("Read this page with chess OCR"))
            .on_hover_text(tl!("Read the page image instead of its text layer (needs the chess-glyphs model)."))
            .clicked()
        {
            action = Some(TextAction::Read(true));
        }
        if let Some(a) = export_menu(ui) {
            action = Some(a);
        }
    });
    let Some(x) = current else {
        // Follows the page: read its text layer now (quick); OCR is on request.
        if action.is_none() && busy.is_none() {
            action = Some(TextAction::Read(false));
        }
        ui.label(egui::RichText::new(tl!("Reading the page…")).color(t.text_muted));
        return action;
    };
    ui.label(
        egui::RichText::new(crate::i18n::fmt(tl!("Recognised text of page {p}"), &[("p", &(page + 1).to_string())])).font(theme::semibold(12.5)),
    );
    if x.ocr {
        ui.label(egui::RichText::new(tl!("Read with chess OCR: check it against the page.")).small().color(t.text_muted));
    }
    if let Some(why) = &x.needs_ocr {
        // The user said the book is born digital: the layer was read anyway, and OCR is not urged.
        let said = if x.origin == BookOrigin::Digital {
            tl!("This page's text layer looks poor ({e}); it was read as it is, as the book is born digital.")
        } else {
            tl!("This page's text layer can't be believed ({e}): read it with chess OCR.")
        };
        ui.label(egui::RichText::new(crate::i18n::fmt(said, &[("e", why)])).small().color(t.text_muted));
    }
    for n in &x.notes {
        ui.label(egui::RichText::new(n).small().color(t.text_muted));
    }
    ui.separator();
    let figurines = figurines(ui.ctx());
    let Some(page_doc) = x.chess.pages.first() else {
        ui.label(tl!("No text on this page."));
        return action;
    };
    if page_doc.blocks.is_empty() {
        ui.label(tl!("No text on this page."));
    }
    for (bi, block) in page_doc.blocks.iter().enumerate() {
        ui.push_id(bi, |ui| match block {
            Block::Heading { inlines, source_rect, .. } => {
                let r = ui.add(
                    egui::Label::new(egui::RichText::new(plain_text_with(inlines, figurines)).font(theme::semibold(14.0)))
                        .sense(egui::Sense::click()),
                );
                if r.clicked()
                    && let Some(rect) = rect_of(*source_rect)
                {
                    action = Some(TextAction::Show(page, rect));
                }
            }
            Block::Paragraph { inlines, source_rect } => {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for (k, inline) in inlines.iter().enumerate() {
                        ui.push_id(k, |ui| match inline {
                            Inline::Move(m) => {
                                let shown = plain_text_with(std::slice::from_ref(inline), figurines);
                                let mut tip = status_label(m.status).to_string();
                                if let Some(fen) = &m.fen {
                                    tip = format!("{tip}\n{fen}");
                                }
                                // A move with no position on its page alone may go on from the
                                // page before: its game is looked for there.
                                let opens = m.game.is_some() || m.status == MoveStatus::Unchecked;
                                if opens {
                                    tip = format!("{tip}\n{}", tl!("Double-click to open this game on the board."));
                                }
                                let at = MoveAt { page, block: bi, inline: k };
                                let r = ui
                                    .add(
                                        egui::Label::new(egui::RichText::new(shown).color(status_colour(m.status, &Tokens::get(ui.ctx()))).strong())
                                            .sense(egui::Sense::click()),
                                    )
                                    .on_hover_text(tip);
                                // A click outlines the move on the page; a double-click (its first
                                // click outlined it) opens its game on the board.
                                if r.double_clicked() && opens {
                                    action = Some(TextAction::OpenGame(at));
                                } else if r.clicked()
                                    && let Some(rect) = rect_of(m.source_rect)
                                {
                                    action = Some(TextAction::Show(page, rect));
                                }
                                if opens {
                                    r.context_menu(|ui| {
                                        if ui.button(tl!("Open this game on the board")).clicked() {
                                            action = Some(TextAction::OpenGame(at));
                                            ui.close();
                                        }
                                    });
                                }
                            }
                            other => {
                                let r = ui.add(egui::Label::new(other.text()).sense(egui::Sense::click()));
                                if r.clicked()
                                    && let Some(rect) = rect_of(other.source_rect().or(*source_rect))
                                {
                                    action = Some(TextAction::Show(page, rect));
                                }
                            }
                        });
                    }
                });
            }
            Block::Diagram(d) => {
                ui.horizontal(|ui| {
                    if let Some((_, tex)) = x.boards.iter().find(|(i, _)| *i == bi) {
                        let r = ui.add(egui::Image::new(tex).fit_to_exact_size(egui::vec2(96.0, 96.0)).sense(egui::Sense::click()));
                        if r.clicked() {
                            action = Some(TextAction::Diagram(page, d.number));
                        }
                    }
                    ui.vertical(|ui| {
                        let title = crate::i18n::fmt(tl!("Diagram {n}"), &[("n", &(d.number + 1).to_string())]);
                        if ui.link(title).clicked() {
                            action = Some(TextAction::Diagram(page, d.number));
                        }
                        match &d.reading {
                            Some(r) => {
                                ui.add(egui::Label::new(egui::RichText::new(&r.fen).monospace().small()).wrap());
                            }
                            None => {
                                ui.label(egui::RichText::new(tl!("Not read")).small().color(t.text_muted));
                            }
                        }
                    });
                });
            }
        });
        ui.add_space(4.0);
    }
    action
}

impl PageText {
    /// Draw the page's diagrams again, with other pieces.
    pub(crate) fn redraw_boards(&mut self, ctx: &egui::Context, pieces: &pdfcraft_engine::chess::pdfcraft_chess::draw::PieceSet) {
        self.boards = boards(ctx, &self.chess, pieces);
    }
}

/// Small boards for the page's diagrams.
fn boards(ctx: &egui::Context, chess: &ChessDoc, pieces: &pdfcraft_engine::chess::pdfcraft_chess::draw::PieceSet) -> Vec<(usize, TextureHandle)> {
    let Some(page) = chess.pages.first() else { return Vec::new() };
    let style = BoardStyle { border: 0.0, coordinates: false, ..BoardStyle::default() };
    page.blocks
        .iter()
        .enumerate()
        .filter_map(|(i, b)| {
            let Block::Diagram(d) = b else { return None };
            let fen = pdfcraft_engine::chess::pdfcraft_chess::fen::parse(&d.reading.as_ref()?.fen).ok()?;
            let (w, h, rgba) = pdfcraft_engine::chess::office::diagram_rgba(&fen.placement, pieces, &style, 192).ok()?;
            let (w, h) = (usize::try_from(w).ok()?, usize::try_from(h).ok()?);
            (rgba.len() == w.checked_mul(h)?.checked_mul(4)?).then_some(())?;
            let image = egui::ColorImage::from_rgba_premultiplied([w, h], &rgba);
            Some((i, ctx.load_texture(format!("chess-text-board-{i}"), image, egui::TextureOptions::LINEAR)))
        })
        .collect()
}

impl PdfCraftApp {
    /// Read page `page` of the active document as chess text for the Text tab (`ocr`: with chess
    /// OCR, as asked). Otherwise the page's text layer is read, quickly, unless the book is
    /// scanned: then every page is read with chess OCR.
    pub fn read_chess_text(&mut self, page: usize, ocr: bool) {
        let Some((_, id)) = self.active_ids() else { return };
        let Some(stamp) = self.session.chess_stamp(id) else { return };
        let origin = self.chess_origin(id);
        // A failed read is kept too, with why, so the tab does not try again every frame.
        let failed = |why: String| PageText {
            doc: id,
            stamp,
            page,
            origin,
            chess: ChessDoc { format: pdfcraft_engine::chess::pdfcraft_chessvision::doc::FORMAT_VERSION, pages: Vec::new(), games: Vec::new() },
            ocr: false,
            needs_ocr: None,
            notes: vec![why],
            boards: Vec::new(),
        };
        if self.session.get(id).is_some_and(|d| d.check_copying().is_err()) {
            self.chess.text = Some(failed(tl!("This document does not allow copying its content, so its text is not read.").to_string()));
            return;
        }
        if self.chess.job.is_some() {
            return;
        }
        let ocr = ocr || origin == BookOrigin::Scanned;
        let mode = if ocr { OcrMode::All } else { OcrMode::Off };
        let book = self.chess_cipher(id);
        let settings = self.chess_scan_settings();
        self.start_chess_job(id, JobKind::Read { page, stamp, origin, ocr }, move |s, id| {
            let options = OcrOptions { mode, glyphs: None, general: true, book: book.as_ref() };
            JobDone::Read(s.chess_document_with(id, &[page], &settings, None, &options))
        });
    }

    /// Open the game of the move `at` of the page the Text tab read on the board
    /// ([`Self::put_book_game`]), with the pages around that page read too, off the interface
    /// thread, so the game comes whole when the book runs it on across the page's edges
    /// (`engine::chess::span`). The pages around are read as the page was: with chess OCR when it
    /// was.
    pub(crate) fn read_book_game(&mut self, at: MoveAt) {
        let Some((_, id)) = self.active_ids() else { return };
        let Some(stamp) = self.session.chess_stamp(id) else { return };
        let Some(text) = self.chess.text.as_ref().filter(|t| t.doc == id && t.page == at.page && t.stamp == stamp) else { return };
        let Some(centre) = text.chess.pages.first().filter(|p| usize::try_from(p.index).ok() == Some(at.page)).cloned() else { return };
        let mode = if text.ocr { OcrMode::All } else { OcrMode::Off };
        if self.chess.job.is_some() {
            return self.notify(tl!("A chess document is already being read or exported").to_string());
        }
        let book = self.chess_cipher(id);
        let settings = self.chess_scan_settings();
        self.start_chess_job(id, JobKind::Game { at, stamp }, move |s, id| {
            let options = OcrOptions { mode, glyphs: None, general: true, book: book.as_ref() };
            JobDone::Game(s.chess_game_span(id, centre, at, &settings, &options))
        });
    }

    /// The book cipher learned so far for document `id`, if any.
    fn chess_cipher(&self, id: DocId) -> Option<pdfcraft_engine::chess::pdfcraft_chessvision::cipher::BookCipher> {
        self.chess.cipher.as_ref().filter(|(d, _)| *d == id).map(|(_, c)| c.clone())
    }

    /// Where document `id`'s pages came from, as the user said (Auto when not said).
    pub fn chess_origin(&self, id: DocId) -> BookOrigin {
        self.chess.origin.filter(|(d, _)| *d == id).map(|(_, o)| o).unwrap_or_default()
    }

    /// Run `work` on a session of its own, opened from document `id`'s working bytes, on a
    /// worker thread (inline on the web and in tests); [`Self::poll_chess_job`] picks it up.
    fn start_chess_job(&mut self, id: DocId, kind: JobKind, work: impl FnOnce(&mut pdfcraft_engine::Session, DocId) -> JobDone + Send + 'static) {
        // The diagram corrections go with the document: the job reads in a session of its own.
        let corrections = self.chess_corrections_file(id);
        let Some(doc) = self.session.get(id) else { return };
        let (name, bytes, password) = (doc.name.clone(), doc.bytes.clone(), doc.password.clone());
        let result: Arc<Mutex<Option<JobDone>>> = Arc::default();
        let slot = result.clone();
        let is_read = matches!(kind, JobKind::Read { .. });
        let run = move || {
            let mut s = pdfcraft_engine::Session::new();
            let done = match s.open(name, None, bytes, password.as_deref()) {
                Ok(job_id) => {
                    s.set_chess_corrections(job_id, corrections);
                    work(&mut s, job_id)
                }
                Err(e) if is_read => JobDone::Read(Err(e.to_string())),
                Err(e) => JobDone::Export(Err(e.to_string())),
            };
            *lock(&slot) = Some(done);
        };
        self.chess.job = Some(ChessJob { doc: id, kind, result });
        #[cfg(not(target_arch = "wasm32"))]
        if self.run_inline {
            run();
        } else if let Err(e) = std::thread::Builder::new().name("pdfcraft-chess-job".into()).spawn(run) {
            self.chess.job = None;
            self.notify(e.to_string());
            return;
        }
        #[cfg(target_arch = "wasm32")]
        run();
        self.poll_chess_job();
    }

    /// Pick up a finished job: show the page read, or write the export.
    pub fn poll_chess_job(&mut self) {
        let Some(job) = self.chess.job.as_ref() else { return };
        let Some(done) = lock(&job.result).take() else {
            if let Some(ctx) = &self.ctx {
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            }
            return;
        };
        let Some(job) = self.chess.job.take() else { return };
        match (job.kind, done) {
            (JobKind::Read { page, stamp, origin, ocr }, JobDone::Read(read)) => {
                // The document changed meanwhile: the reading describes a state that is gone.
                if self.session.chess_stamp(job.doc) != Some(stamp) {
                    return;
                }
                let empty =
                    || ChessDoc { format: pdfcraft_engine::chess::pdfcraft_chessvision::doc::FORMAT_VERSION, pages: Vec::new(), games: Vec::new() };
                self.chess.text = Some(match read {
                    Ok(read) => {
                        self.chess.cipher = Some((job.doc, read.cipher.clone()));
                        let pieces = self.chess.role_pieces(pdfcraft_engine::chess::pdfcraft_chessvision::pieces::choice::Role::Diagrams);
                        let boards = self.ctx.as_ref().map(|ctx| boards(ctx, &read.doc, &pieces)).unwrap_or_default();
                        let needs_ocr = read.needs_ocr.first().map(|(_, why)| why.clone());
                        let mut notes: Vec<String> = read.notes.into_iter().map(|(_, n)| n).collect();
                        if ocr && read.ocr_pages.is_empty() && !notes.iter().any(|n| n.contains("chess-glyphs")) {
                            notes.push(tl!("Chess OCR read nothing on this page.").to_string());
                        }
                        PageText { doc: job.doc, stamp, page, origin, chess: read.doc, ocr: !read.ocr_pages.is_empty(), needs_ocr, notes, boards }
                    }
                    Err(e) => PageText {
                        doc: job.doc,
                        stamp,
                        page,
                        origin,
                        chess: empty(),
                        ocr: false,
                        needs_ocr: None,
                        notes: vec![e],
                        boards: Vec::new(),
                    },
                });
            }
            (JobKind::Game { stamp, .. }, JobDone::Game(Ok(read))) => {
                // The document changed meanwhile: the move may be gone.
                if self.session.chess_stamp(job.doc) != Some(stamp) {
                    return;
                }
                self.chess.cipher = Some((job.doc, read.cipher));
                match read.span.at {
                    Some(node) => self.put_book_game(job.doc, &read.span.doc, node),
                    None => self.notify(tl!("No game was found for this move, on its page or the pages around it.").to_string()),
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            (JobKind::Export { path, image_dir, .. }, JobDone::Export(made)) => {
                if let Ok(m) = &made {
                    self.chess.cipher = Some((job.doc, m.read.cipher.clone()));
                }
                self.write_chess_export(made, &path, &image_dir);
            }
            #[cfg(target_arch = "wasm32")]
            (JobKind::Export { name, .. }, JobDone::Export(made)) => match made {
                Ok(made) => {
                    if let Err(e) = crate::editing::download(&name, &made.bytes) {
                        self.notify_error(e);
                    }
                }
                Err(e) => self.notify(e),
            },
            (_, JobDone::Read(Err(e)) | JobDone::Game(Err(e)) | JobDone::Export(Err(e))) => self.notify(e),
            _ => {}
        }
    }

    pub(crate) fn chess_text_action(&mut self, action: TextAction) {
        match action {
            TextAction::Read(ocr) => {
                let Some((vi, _)) = self.active_ids() else { return };
                let Some(page) = self.views.get(vi).map(|v| v.current) else { return };
                self.read_chess_text(page, ocr);
            }
            TextAction::Show(page, rect) => {
                let Some((vi, _)) = self.active_ids() else { return };
                if let Some(view) = self.views.get_mut(vi) {
                    view.go_to_page(page);
                    view.flash = Some((page, rect, 0.0));
                }
            }
            TextAction::Diagram(page, index) => {
                // The diagram list must hold this page's scan for the editor to open on it.
                if self.chess.results.as_ref().is_none_or(|r| r.diagram(page, index).is_none()) {
                    self.start_chess_scan(false);
                }
                self.select_chess_diagram(page, index, true);
                self.show_chess_tab(ChessTab::Diagrams);
            }
            TextAction::OpenGame(at) => self.read_book_game(at),
            TextAction::Export(format) => self.export_chess_dialog(format),
            // The page shown is read again under the new origin (the tab sees it is not current).
            TextAction::Origin(origin) => {
                if let Some((_, id)) = self.active_ids() {
                    self.chess.origin = Some((id, origin));
                }
            }
        }
    }

    /// Chess ▸ Export: ask where (`save_override` in tests), then read the document and write it.
    pub fn export_chess_dialog(&mut self, format: ChessFormat) {
        let Some((_, id)) = self.active_ids() else { return };
        // Diagrams with unconfirmed changes are asked about first (plan A8, P3b3).
        if self.ask_chess_drafts(Some(id), crate::chess_drafts::AfterDrafts::Export(format)) {
            return;
        }
        if !self.may_copy(id) {
            return;
        }
        let Some(doc) = self.session.get(id) else { return };
        let stem = doc.name.trim_end_matches(".pdf").trim_end_matches(".PDF").to_string();
        let ext = format.extension();
        #[cfg(not(target_arch = "wasm32"))]
        {
            let write = move |app: &mut Self, path: std::path::PathBuf| app.export_chess_to(id, format, &path);
            match self.save_override.clone() {
                Some(p) => write(self, p.into()),
                None => {
                    let dialog = rfd::AsyncFileDialog::new()
                        .set_title(tl!("Export").to_string())
                        .add_filter(format.name(), &[ext])
                        .set_file_name(format!("{stem}.{ext}"));
                    self.ask_one(crate::pickers::Ask::Save(dialog), None, write);
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let mut options = ChessExportOptions::new(&stem);
            options.pieces = self.chess.role_pieces(pdfcraft_engine::chess::pdfcraft_chessvision::pieces::choice::Role::Diagrams);
            options.book = self.chess_cipher(id);
            options.ocr = self.chess_origin(id).ocr_mode();
            self.start_chess_job(id, JobKind::Export { format, name: format!("{stem}.{ext}") }, move |s, id| {
                JobDone::Export(s.chess_export(id, format, &options))
            });
        }
    }

    /// Write the chess export of document `id` to `path` (Markdown's images in a folder beside it).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn export_chess_to(&mut self, id: DocId, format: ChessFormat, path: &std::path::Path) {
        if self.chess.job.is_some() {
            return self.notify(tl!("A chess document is already being read or exported").to_string());
        }
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "document".into());
        let mut options = ChessExportOptions::new(&stem);
        options.image_dir = pdfcraft_engine::chess::pdfcraft_chessvision::doc::markdown::image_folder(&stem);
        options.pieces = self.chess.role_pieces(pdfcraft_engine::chess::pdfcraft_chessvision::pieces::choice::Role::Diagrams);
        options.book = self.chess_cipher(id);
        options.ocr = self.chess_origin(id).ocr_mode();
        let kind = JobKind::Export { format, path: path.to_path_buf(), image_dir: options.image_dir.clone() };
        self.notify(tl!("Exporting the chess document…").to_string());
        self.start_chess_job(id, kind, move |s, id| JobDone::Export(s.chess_export(id, format, &options)));
    }

    /// Write a finished export to `path`, Markdown's images in `image_dir` beside it.
    #[cfg(not(target_arch = "wasm32"))]
    fn write_chess_export(&mut self, made: Result<pdfcraft_engine::chess::export::ChessExport, String>, path: &std::path::Path, image_dir: &str) {
        let made = match made {
            Ok(m) => m,
            Err(e) => return self.notify(e),
        };
        if !made.images.is_empty() {
            let dir = path.with_file_name(image_dir);
            if let Err(e) = std::fs::create_dir_all(&dir) {
                return self.notify_fmt("Couldn't write {name}: {e}", &[("name", &dir.display().to_string()), ("e", &e.to_string())]);
            }
            for (rel, png) in &made.images {
                let Some(name) = std::path::Path::new(rel).file_name() else { continue };
                let target = dir.join(name);
                if let Err(e) = crate::editing::write_atomically(&target.to_string_lossy(), png) {
                    return self.notify_fmt("Couldn't write {name}: {e}", &[("name", &target.display().to_string()), ("e", &e.to_string())]);
                }
            }
        }
        match crate::editing::write_atomically(&path.to_string_lossy(), &made.bytes) {
            Ok(()) => self.notify_fmt("Exported to {path}", &[("path", &path.display().to_string())]),
            Err(e) => self.notify_fmt("Couldn't write {name}: {e}", &[("name", &path.display().to_string()), ("e", &e.to_string())]),
        }
    }
}
