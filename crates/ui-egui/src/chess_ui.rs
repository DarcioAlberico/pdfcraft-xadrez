//! Chess ▸ Diagrams (blueprint S16a): find the chess diagrams in the open document, mark them on
//! its pages and list what was read from each in the Chess diagrams panel: the position (FEN),
//! whether the reading passed the gate, and a lichess link when the position is legal and was not
//! repaired.
//!
//! The scan is the engine's ([`pdfcraft_engine::chess`]); this file runs it and shows it. Pages
//! are scanned in batches on a worker thread (inline on the web and in tests) and each batch is
//! shown when it is done, so a long book fills in as it goes. Results are kept for one document.
//! Once the document changes (an edit, undo or redo) the boxes may no longer be where its
//! diagrams are: they leave the pages, the panel says so, and the next scan starts afresh.
//! Nothing here changes the document.

use std::sync::{Arc, Mutex, MutexGuard};

use egui::{Color32, CornerRadius, Pos2, Stroke};
use pdfcraft_engine::DocId;
use pdfcraft_engine::chess::pdfcraft_chessvision::board::{BoardReading, Unreadable};
use pdfcraft_engine::chess::{ChessModels, ChessPage, ChessScan, ChessScanJob, ModelState, ScanStamp};
use pdfcraft_render::DocInfo;

use crate::canvas::PageXform;
use crate::theme::{self, Tokens};
use crate::{PdfCraftApp, widgets};

/// Pages scanned per job. Each job opens the file in its own renderer, so smaller batches show
/// results sooner and larger ones parse the file fewer times.
const PAGES_PER_JOB: usize = 8;

/// Diagrams listed in the panel at most; the page marks show them all.
const MAX_LISTED: usize = 2000;

/// The colour of a diagram's state, on the page and beside it in the panel.
const READ: Color32 = Color32::from_rgb(0x2D, 0x9D, 0x5B);
const REVIEW: Color32 = Color32::from_rgb(0xE0, 0x8A, 0x1A);
const FOUND: Color32 = Color32::from_rgb(0x3A, 0x6F, 0xE8);
const FAILED: Color32 = Color32::from_rgb(0xE0, 0x3E, 0x3E);

/// Chess diagrams: the last results and the running scan.
#[derive(Default)]
pub struct ChessState {
    pub results: Option<ChessResults>,
    pub run: Option<ChessRun>,
    /// A diagram clicked on the page this frame (page, index), picked at the next poll.
    pub picked: Option<(usize, usize)>,
    /// The diagram picked was double-clicked: it goes onto the chess board too.
    pub to_board: bool,
    /// The last diagram clicked on the page and when (egui's input time), to tell a double-click.
    pub last_pick: Option<((usize, usize), f64)>,
    /// A double-click on a page not scanned yet: the diagram there goes onto the board once the
    /// page is scanned ([`PdfCraftApp::chess_board_at_tick`]).
    pub board_at: Option<BoardAt>,
    /// The chess games, per document: the board panels show the open document's active one
    /// ([`ChessState::board`]).
    pub games: crate::chess_collection::ChessGames,
    /// The selected diagram's position being edited (S17).
    pub editor: Option<crate::chess_editor::DiagramEditor>,
    /// List only the diagrams to review.
    pub review_only: bool,
    /// The pieces the editor draws with, found once.
    pub pieces: Option<Arc<pdfcraft_engine::chess::pdfcraft_chess::draw::PieceSet>>,
    /// The pieces the analysis board draws with, found once (the user's piece pictures first).
    pub board_pieces: Option<Arc<pdfcraft_engine::chess::pdfcraft_chess::draw::PieceSet>>,
    /// The panel's tab: the diagrams or the page's recognised text (S16).
    pub tab: crate::chess_text_ui::ChessTab,
    /// What the Board panel adds as the study board: set-up, engine analysis (the board itself
    /// is [`ChessState::board`]).
    pub study: crate::chess_study_ui::StudyState,
    /// The page last read as text.
    pub text: Option<crate::chess_text_ui::PageText>,
    /// A page being read, or the book being exported, off the interface thread.
    pub job: Option<crate::chess_text_ui::ChessJob>,
    /// The open document's book cipher, as its reads so far taught it: every later read and
    /// export starts from it.
    pub cipher: Option<(DocId, pdfcraft_engine::chess::pdfcraft_chessvision::cipher::BookCipher)>,
    /// Where the open document's pages came from, as the user said (Auto when not said): every
    /// later read and export follows it.
    pub origin: Option<(DocId, pdfcraft_engine::chess::document::BookOrigin)>,
    /// Undo and Redo go to the chess game, not the document: the last action was a click in the
    /// chess panels (D12). See [`ChessState::in_scope`], which also counts changes to the game.
    pub scope: bool,
    /// The game shown (its id) and its history's step count when the scope was last settled.
    pub(crate) scope_edits: (u64, u64),
    /// The PGN tool rail: the tool armed and the keys the chess scope took (T5a).
    pub tools: crate::chess_tools::ToolState,
    /// Text captured from the page or the clipboard (T7a).
    pub capture: crate::chess_capture::CaptureState,
    /// Follow, and the move's place last shown on the page (T8).
    pub source: crate::chess_source::SourceState,
    /// Where the chess models come from: the folder chosen, or the files picked on the web (U4).
    pub models: crate::chess_models::ModelsPref,
}

/// A double-click at `point` (user space) on `page` of `doc`, whose diagrams were not looked
/// for yet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoardAt {
    pub doc: DocId,
    pub page: usize,
    pub point: [f32; 2],
    /// The double-click selected a word: it may have been for that alone, so finding no diagram
    /// there is not said.
    pub on_text: bool,
    /// The scan of the page was started for it.
    pub started: bool,
}

/// What scanning found in one document.
pub struct ChessResults {
    pub doc: DocId,
    /// The document state the pages were scanned in.
    pub stamp: ScanStamp,
    /// The scanned pages, by page number, each once.
    pub pages: Vec<Arc<ChessPage>>,
    /// The diagram picked in the panel or on the page: page (0-based) and its index there.
    pub selected: Option<(usize, usize)>,
    /// The panel scrolls the picked diagram into view once (it was picked on the page).
    pub reveal: bool,
    /// Whether the piece model could be used.
    pub model: ModelNote,
}

impl ChessResults {
    /// How many diagrams were found.
    pub fn count(&self) -> usize {
        self.pages.iter().map(|p| p.diagrams.len()).sum()
    }

    /// The diagrams to review, in reading order: page, then index.
    pub fn review_queue(&self) -> Vec<(usize, usize)> {
        self.pages
            .iter()
            .flat_map(|p| (0..p.diagrams.len()).filter(|i| crate::chess_editor::needs_review(Self::reading(p, *i))).map(|i| (p.page, i)))
            .collect()
    }

    /// The reading of diagram `index` on `page`, when it was read.
    pub(crate) fn reading(page: &ChessPage, index: usize) -> Option<&BoardReading> {
        page.readings.as_ref()?.iter().find(|r| r.index == index)
    }

    /// Diagram `index` on `page` (0-based): its box in user space and its FEN when read.
    pub fn diagram(&self, page: usize, index: usize) -> Option<([f32; 4], Option<&str>)> {
        let p = self.pages.iter().find(|p| p.page == page)?;
        let d = p.diagrams.get(index)?;
        let b = d.candidate.bbox_user;
        let fen = Self::reading(p, index).and_then(BoardReading::position).map(|pos| pos.fen.as_str());
        Some(([b.x0, b.y0, b.x1, b.y1], fen))
    }
}

/// Whether the scan could use the piece model. Without one only diagrams set in a chess font are
/// read; the others are found and marked.
#[derive(Clone, Debug, PartialEq)]
pub enum ModelNote {
    Loaded,
    Missing,
    Invalid(String),
}

impl ModelNote {
    fn of(models: &ChessModels) -> ModelNote {
        match &models.pieces {
            ModelState::Loaded(_) => ModelNote::Loaded,
            ModelState::Missing => ModelNote::Missing,
            ModelState::Invalid(why) => ModelNote::Invalid(why.clone()),
        }
    }
}

/// A running scan: pages done of the total, the batches finished and not yet shown, and a
/// cancel request.
#[derive(Default)]
pub struct ChessProgress {
    pub done: usize,
    pub total: usize,
    pub scans: Vec<ChessScan>,
    pub model: Option<ModelNote>,
    pub finished: bool,
    pub cancel: bool,
}

pub struct ChessRun {
    pub doc: DocId,
    /// Started by a double-click on the page (`BoardAt`): the board shows what came of it, so the
    /// scan neither brings the Chess panel forward nor announces the count of diagrams found.
    pub quiet: bool,
    pub progress: Arc<Mutex<ChessProgress>>,
    /// Diagrams in the batches shown so far.
    pub found: usize,
    /// A batch was dropped because the document changed during the scan (said once).
    pub stale: bool,
}

/// The progress, even after a worker panicked while holding it (it is plain data).
fn lock(p: &Mutex<ChessProgress>) -> MutexGuard<'_, ChessProgress> {
    p.lock().unwrap_or_else(|e| e.into_inner())
}

/// Marks the scan finished when the worker ends, however it ends, so the screen never waits for
/// a worker that is gone.
struct Finish(Arc<Mutex<ChessProgress>>);

impl Drop for Finish {
    fn drop(&mut self) {
        lock(&self.0).finished = true;
    }
}

/// What the panel asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum PanelAction {
    ScanPage,
    ScanAll,
    Cancel,
    Select(usize, usize),
    /// Put diagram (page, index) on the chess board.
    OpenBoard(usize, usize),
    /// Show the chess board panel.
    ShowBoard,
    CopyFen(String),
    OpenLichess(String),
    Editor(crate::chess_editor::EditorAction),
    Text(crate::chess_text_ui::TextAction),
    /// Put the chess window's panel back in the main window (T13b).
    DockWindow,
    /// Chess models…: choose where the models come from (U4).
    Models,
}

/// One diagram's state: its label and colour.
fn status(reading: Option<&BoardReading>) -> (String, Color32) {
    match reading.map(|r| &r.outcome) {
        Some(Ok(p)) if p.accepted => (tl!("Position read").to_string(), READ),
        Some(Ok(_)) => (tl!("Needs review").to_string(), REVIEW),
        Some(Err(Unreadable::NoModel)) => (tl!("Not read: no piece model").to_string(), FOUND),
        Some(Err(e)) => (crate::i18n::fmt(tl!("Couldn't be read: {e}"), &[("e", &e.to_string())]), FAILED),
        None => (tl!("Not read").to_string(), FOUND),
    }
}

/// Whether `page` (0-based) is among the pages scanned in `results`.
pub(crate) fn page_scanned(results: Option<&ChessResults>, page: usize) -> bool {
    results.is_some_and(|r| r.pages.iter().any(|p| p.page == page))
}

/// The diagram of `page` whose box holds `point` (user space): the smallest one, when boxes
/// overlap.
pub(crate) fn diagram_at(page: &ChessPage, point: [f32; 2]) -> Option<usize> {
    let [x, y] = point;
    page.diagrams
        .iter()
        .enumerate()
        .filter_map(|(i, d)| {
            let b = d.candidate.bbox_user;
            let (x0, x1, y0, y1) = (b.x0.min(b.x1), b.x0.max(b.x1), b.y0.min(b.y1), b.y0.max(b.y1));
            let area = (x1 - x0) * (y1 - y0);
            // A box that is not finite holds no point (NaN fails every comparison).
            (x0 <= x && x <= x1 && y0 <= y && y <= y1 && area.is_finite()).then_some((i, area))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// The results to show for document `id`: none for another document, and none once it changed
/// since the scan (`current` is its state now).
pub(crate) fn shown(state: &ChessState, id: DocId, current: Option<ScanStamp>) -> Option<&ChessResults> {
    state.results.as_ref().filter(|r| r.doc == id && Some(r.stamp) == current)
}

/// Outline the diagrams of `page` and number them. Returns the diagram under `pointer` (its
/// index on the page) with its hover text.
pub(crate) fn paint(
    painter: &egui::Painter,
    xf: &PageXform,
    page: usize,
    info: &DocInfo,
    results: Option<&ChessResults>,
    pointer: Option<Pos2>,
) -> Option<(usize, String)> {
    let results = results?;
    // `user_rect` reads the page's box: never for a page the document does not have.
    info.pages.get(page)?;
    let p = results.pages.iter().find(|p| p.page == page)?;
    let mut hovered = None;
    for (i, d) in p.diagrams.iter().enumerate() {
        let b = d.candidate.bbox_user;
        if ![b.x0, b.y0, b.x1, b.y1].iter().all(|v| v.is_finite()) {
            continue;
        }
        let r = xf.user_rect(info, page, [b.x0, b.y0, b.x1, b.y1]);
        let reading = ChessResults::reading(p, i);
        let (label, colour) = status(reading);
        let selected = results.selected == Some((page, i));
        painter.rect_filled(r, CornerRadius::same(2), colour.gamma_multiply(0.10));
        painter.rect_stroke(r, CornerRadius::same(2), Stroke::new(if selected { 3.0 } else { 1.5 }, colour), egui::StrokeKind::Outside);
        if selected {
            painter.rect_stroke(r.expand(5.0), CornerRadius::same(4), Stroke::new(1.5, colour.gamma_multiply(0.6)), egui::StrokeKind::Outside);
        }
        // The diagram's number in reading order, in a tab at its top-left corner.
        let number = painter.layout_no_wrap((i + 1).to_string(), theme::semibold(11.0), Color32::WHITE);
        let tab = egui::Rect::from_min_size(r.left_top(), number.size() + egui::vec2(8.0, 2.0));
        painter.rect_filled(tab, CornerRadius::same(2), colour);
        painter.galley(tab.min + egui::vec2(4.0, 1.0), number, Color32::WHITE);
        if hovered.is_none() && pointer.is_some_and(|q| r.contains(q)) {
            let title = crate::i18n::fmt(tl!("Diagram {n}"), &[("n", &(i + 1).to_string())]);
            let fen = reading.and_then(BoardReading::position).map(|pos| pos.fen.as_str());
            let text = match fen {
                Some(fen) => format!("{title} · {label}\n{fen}"),
                None => format!("{title} · {label}"),
            };
            hovered = Some((i, text));
        }
    }
    hovered
}

/// The Chess diagrams panel: its tabs, and the tab chosen. While the panel is in its own window
/// (`in_window`, T13b) the Diagrams and Text tabs are drawn there and only said to be here, so
/// one frame never draws them twice. The study board is the Board panel (T5c).
#[allow(clippy::too_many_arguments)]
pub(crate) fn panel(
    ui: &mut egui::Ui,
    t: &Tokens,
    state: &mut ChessState,
    id: DocId,
    info: &DocInfo,
    current: Option<ScanStamp>,
    page_now: usize,
    in_window: bool,
) -> Option<PanelAction> {
    use crate::chess_text_ui::ChessTab;
    // A click anywhere in the panel, whichever tab shows, puts Undo and Redo on the game (D12).
    if crate::chess_history::pressed_in(ui) {
        state.enter_scope();
    }
    ui.horizontal(|ui| {
        ui.selectable_value(&mut state.tab, ChessTab::Diagrams, tl!("Diagrams"));
        ui.selectable_value(&mut state.tab, ChessTab::Text, tl!("Text"));
    });
    ui.add_space(4.0);
    if in_window {
        return in_window_note(ui, t);
    }
    tab_contents(ui, t, state, state.tab == ChessTab::Text, id, info, current, page_now)
}

/// Where the Diagrams and Text tabs were: they are in the chess window, which can be docked.
fn in_window_note(ui: &mut egui::Ui, t: &Tokens) -> Option<PanelAction> {
    ui.add_space(6.0);
    ui.label(egui::RichText::new(tl!("The chess panel is in its own window.")).color(t.text_muted));
    ui.add_space(4.0);
    ui.button(tl!("Bring it back")).clicked().then_some(PanelAction::DockWindow)
}

/// The Text tab (`text`) or the Diagrams tab: in the Inspector's Chess panel, or in the chess
/// window (T13b).
#[allow(clippy::too_many_arguments)]
pub(crate) fn tab_contents(
    ui: &mut egui::Ui,
    t: &Tokens,
    state: &mut ChessState,
    text: bool,
    id: DocId,
    info: &DocInfo,
    current: Option<ScanStamp>,
    page_now: usize,
) -> Option<PanelAction> {
    if text {
        let busy = state.job.as_ref().filter(|j| j.doc == id).map(crate::chess_text_ui::ChessJob::label);
        let origin = state.origin.filter(|(d, _)| *d == id).map(|(_, o)| o).unwrap_or_default();
        return crate::chess_text_ui::text_tab(ui, t, state.text.as_mut(), id, page_now, current, origin, busy).map(PanelAction::Text);
    }
    let mut action = None;
    let running = state.run.as_ref().filter(|r| r.doc == id).map(|r| {
        let s = lock(&r.progress);
        (s.done, s.total)
    });
    ui.horizontal_wrapped(|ui| {
        ui.add_enabled_ui(state.run.is_none(), |ui| {
            if widgets::pill_button(ui, tl!("Scan this page"), true).clicked() {
                action = Some(PanelAction::ScanPage);
            }
            if widgets::pill_button(ui, tl!("Scan all pages"), false).clicked() {
                action = Some(PanelAction::ScanAll);
            }
        });
        if let Some(a) = crate::chess_text_ui::export_menu(ui) {
            action = Some(PanelAction::Text(a));
        }
        if ui.button(tl!("Chess board")).on_hover_text(tl!("Show the analysis board. Double-click a diagram on the page to load it there.")).clicked()
        {
            action = Some(PanelAction::ShowBoard);
        }
        if ui.button(tl!("Chess models…")).on_hover_text(tl!("Choose the piece and glyph models that read diagrams and chess text.")).clicked() {
            action = Some(PanelAction::Models);
        }
    });
    if let Some((done, total)) = running {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(
                egui::RichText::new(crate::i18n::fmt(
                    tl!("Scanning for diagrams… page {d} of {t}"),
                    &[("d", &(done + 1).min(total.max(1)).to_string()), ("t", &total.max(1).to_string())],
                ))
                .small()
                .color(t.text_muted),
            );
            if ui.button(tl!("Cancel")).clicked() {
                action = Some(PanelAction::Cancel);
            }
        });
    }
    ui.add_space(6.0);
    let mine = state.results.as_mut().filter(|r| r.doc == id);
    let review_only = state.review_only;
    let Some(results) = mine else {
        if running.is_none() {
            ui.label(egui::RichText::new(tl!("Scan this page or all pages to find the chess diagrams in this document.")).color(t.text_muted));
        }
        return action;
    };
    if Some(results.stamp) != current {
        ui.label(egui::RichText::new(tl!("The document changed since the scan. Scan again to update the diagrams.")).color(t.text_muted));
        ui.add_space(4.0);
    }
    match &results.model {
        ModelNote::Loaded => {}
        ModelNote::Missing => {
            ui.label(
                egui::RichText::new(tl!(
                    "No piece model: only diagrams set in a chess font are read. Use Chess models… to give the app chess-pieces.onnx."
                ))
                .small()
                .color(t.text_muted),
            );
        }
        ModelNote::Invalid(why) => {
            ui.label(
                egui::RichText::new(crate::i18n::fmt(tl!("The piece model couldn't be loaded: {e}"), &[("e", why)])).small().color(t.text_muted),
            );
        }
    }
    if running.is_none() && results.count() == 0 {
        ui.add_space(4.0);
        ui.label(tl!("No diagrams found."));
    }
    // The selected diagram's editor (S17), above the list.
    let queue = results.review_queue();
    if let Some(ed) = state.editor.as_mut().filter(|e| e.doc == id && Some(results.stamp) == current) {
        ui.separator();
        let pieces = crate::chess_editor::piece_set(&mut state.pieces);
        if let Some(a) = crate::chess_editor::editor_ui(ui, t, ed, &pieces, queue.len()) {
            action = Some(PanelAction::Editor(a));
        }
    }
    ui.separator();
    ui.add(crate::widgets::checkbox(
        &mut state.review_only,
        crate::i18n::fmt(tl!("Only diagrams to review ({n})"), &[("n", &queue.len().to_string())]),
    ));
    let reveal = std::mem::take(&mut results.reveal);
    let mut listed = 0;
    for page in &results.pages {
        if page.diagrams.is_empty() && page.error.is_none() {
            continue;
        }
        let label = info.pages.get(page.page).map_or_else(|| (page.page + 1).to_string(), |p| p.label.clone());
        ui.add_space(4.0);
        ui.label(egui::RichText::new(crate::i18n::fmt(tl!("Page {label}"), &[("label", &label)])).font(theme::semibold(12.5)));
        if let Some(e) = &page.error {
            ui.label(egui::RichText::new(crate::i18n::fmt(tl!("This page couldn't be scanned: {e}"), &[("e", e)])).small().color(t.text_muted));
        }
        if page.truncated {
            ui.label(
                egui::RichText::new(crate::i18n::fmt(
                    tl!("Only the first {n} diagrams on this page are listed."),
                    &[("n", &page.diagrams.len().to_string())],
                ))
                .small()
                .color(t.text_muted),
            );
        }
        for (i, _) in page.diagrams.iter().enumerate() {
            if listed >= MAX_LISTED {
                break;
            }
            let reading = ChessResults::reading(page, i);
            if review_only && !crate::chess_editor::needs_review(reading) {
                continue;
            }
            listed += 1;
            let (label, colour) = status(reading);
            let position = reading.and_then(BoardReading::position);
            let selected = results.selected == Some((page.page, i));
            let row = ui.push_id((page.page, i), |ui| {
                ui.horizontal(|ui| {
                    let (bar, _) = ui.allocate_exact_size(egui::vec2(4.0, if position.is_some() { 58.0 } else { 20.0 }), egui::Sense::hover());
                    ui.painter().rect_filled(bar, CornerRadius::same(2), colour);
                    ui.vertical(|ui| {
                        let title = crate::i18n::fmt(tl!("Diagram {n}"), &[("n", &(i + 1).to_string())]);
                        let head = ui.selectable_label(selected, format!("{title} · {label}"));
                        if let Some(pos) = position {
                            ui.add(egui::Label::new(egui::RichText::new(&pos.fen).monospace().small()).wrap());
                            ui.horizontal(|ui| {
                                if ui.small_button(tl!("Copy FEN")).clicked() {
                                    action = Some(PanelAction::CopyFen(pos.fen.clone()));
                                }
                                if let Some(url) = pdfcraft_engine::chess::lichess_url(pos)
                                    && ui.small_button(tl!("Open in lichess")).on_hover_text(&url).clicked()
                                {
                                    action = Some(PanelAction::OpenLichess(url));
                                }
                                if ui.small_button(tl!("Open on the board")).clicked() {
                                    action = Some(PanelAction::OpenBoard(page.page, i));
                                }
                            });
                        }
                        head
                    })
                    .inner
                })
                .inner
            });
            if selected && reveal {
                row.response.scroll_to_me(Some(egui::Align::Center));
            }
            if row.inner.clicked() {
                action = Some(PanelAction::Select(page.page, i));
            }
        }
    }
    action
}

impl PdfCraftApp {
    /// Chess ▸ Scan diagrams: scan the current page (`all`: every page) of the active document in
    /// the background and show the Chess diagrams panel.
    pub fn start_chess_scan(&mut self, all: bool) {
        let Some((vi, id)) = self.active_ids() else { return };
        let Some(count) = self.session.get(id).map(|d| d.info.pages.len()) else { return };
        let pages: Vec<usize> =
            if all { (0..count).collect() } else { self.views.get(vi).map(|v| v.current).filter(|p| *p < count).into_iter().collect() };
        self.start_chess_scan_pages(pages, false);
    }

    /// Scan `pages` of the active document. `quiet`: for a double-click on a page (`BoardAt`),
    /// which neither shows the Chess panel nor announces what was found.
    fn start_chess_scan_pages(&mut self, pages: Vec<usize>, quiet: bool) {
        let Some((_, id)) = self.active_ids() else { return };
        if self.chess.run.is_some() {
            self.notify(tl!("Diagrams are already being scanned").to_string());
            return;
        }
        // Reading positions out of a document copies its content. A double-click's scan is
        // refused in silence: the double-click may only have been selecting a word.
        let allowed = if quiet { self.session.get(id).is_none_or(pdfcraft_engine::Document::allows_copying) } else { self.may_copy(id) };
        if !allowed {
            return;
        }
        let Some(count) = self.session.get(id).map(|d| d.info.pages.len()) else { return };
        let pages: Vec<usize> = pages.into_iter().filter(|p| *p < count).collect();
        let settings = self.chess_scan_settings();
        // `chunks` needs a batch size above zero: PAGES_PER_JOB is 8.
        let jobs: Vec<ChessScanJob> = pages.chunks(PAGES_PER_JOB).filter_map(|c| self.session.chess_scan_job(id, c, &settings, true)).collect();
        // Results for another document, or for this one before it changed, give way.
        let now = self.session.chess_stamp(id);
        if self.chess.results.as_ref().is_some_and(|r| r.doc != id || Some(r.stamp) != now) {
            self.chess.results = None;
        }
        let progress = Arc::new(Mutex::new(ChessProgress { total: pages.len(), ..Default::default() }));
        let p = progress.clone();
        let work = move || {
            let _finish = Finish(p.clone());
            let models = pdfcraft_engine::chess::models(settings.models_dir.as_deref());
            lock(&p).model = Some(ModelNote::of(&models));
            let mut base: usize = 0;
            for job in jobs {
                let size = job.pages().len();
                let scan = job.run(&models, |done, _| {
                    let mut s = lock(&p);
                    s.done = base.saturating_add(done);
                    !s.cancel
                });
                base = base.saturating_add(size);
                let mut s = lock(&p);
                s.scans.push(scan);
                if s.cancel {
                    break;
                }
            }
        };
        #[cfg(not(target_arch = "wasm32"))]
        if self.run_inline {
            work();
        } else if std::thread::Builder::new().name("pdfcraft-chess".into()).spawn(work).is_err() {
            // The work never ran, so no `Finish` will mark the scan done: do it here.
            lock(&progress).finished = true;
        }
        #[cfg(target_arch = "wasm32")]
        work();
        self.chess.run = Some(ChessRun { doc: id, quiet, progress, found: 0, stale: false });
        if !quiet {
            self.reveal_chess_diagrams();
        }
        self.poll_chess();
    }

    /// Stop a running scan (the pages scanned so far are kept).
    pub fn cancel_chess_scan(&mut self) {
        if let Some(r) = &self.chess.run {
            lock(&r.progress).cancel = true;
        }
    }

    /// Show the batches the worker finished; report the scan once it is done.
    pub(crate) fn poll_chess(&mut self) {
        self.poll_chess_scan();
        self.chess_board_at_tick();
    }

    fn poll_chess_scan(&mut self) {
        self.chess_games_tick();
        self.poll_chess_job();
        self.poll_chess_capture();
        self.chess_source_tick();
        // A closed document takes its results with it.
        if self.chess.results.as_ref().is_some_and(|r| self.session.get(r.doc).is_none()) {
            self.chess.results = None;
        }
        if let Some((page, index)) = self.chess.picked.take() {
            self.select_chess_diagram(page, index, false);
            if std::mem::take(&mut self.chess.to_board) {
                self.open_chess_board(page, index);
            }
        }
        let Some(progress) = self.chess.run.as_ref().map(|r| r.progress.clone()) else { return };
        let (scans, model, finished) = {
            let mut s = lock(&progress);
            (std::mem::take(&mut s.scans), s.model.clone(), s.finished)
        };
        for scan in scans {
            let found: usize = scan.pages.iter().map(|p| p.diagrams.len()).sum();
            let kept = self.keep_chess_scan(scan, model.clone());
            let Some(run) = self.chess.run.as_mut() else { return };
            let first_stale = if kept {
                run.found = run.found.saturating_add(found);
                false
            } else {
                !std::mem::replace(&mut run.stale, true)
            };
            if first_stale {
                self.notify(tl!("The document changed since the scan. Scan again to update the diagrams.").to_string());
            }
        }
        if !finished {
            if let Some(ctx) = &self.ctx {
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            }
            return;
        }
        let Some(run) = self.chess.run.take() else { return };
        if !run.stale && !run.quiet {
            let n = run.found;
            self.notify(crate::i18n::fmt(tl!("Diagrams found: {n}"), &[("n", &n.to_string())]));
        }
    }

    /// A double-click on a page not scanned yet (`BoardAt`): scan the page (after the scan
    /// running, if one is), then put the diagram under the double-click on the board. A point with
    /// no diagram says so, unless the double-click selected a word there (which may be all it was
    /// for). Dropped when its document is no longer the one shown.
    pub(crate) fn chess_board_at_tick(&mut self) {
        let Some(mut at) = self.chess.board_at else { return };
        if self.active_ids().map(|(_, id)| id) != Some(at.doc) {
            self.chess.board_at = None;
            return;
        }
        let stamp = self.session.chess_stamp(at.doc);
        let scanned = shown(&self.chess, at.doc, stamp).and_then(|r| r.pages.iter().find(|p| p.page == at.page)).cloned();
        if let Some(page) = scanned {
            self.chess.board_at = None;
            match diagram_at(&page, at.point) {
                Some(index) => {
                    self.select_chess_diagram(at.page, index, false);
                    self.open_chess_board(at.page, index);
                }
                None if !at.on_text => self.notify(tl!("No chess diagram was found here.").to_string()),
                None => {}
            }
            return;
        }
        if self.chess.run.is_some() {
            return;
        }
        // The scan ended without the page: cancelled, or the document changed meanwhile.
        if at.started {
            self.chess.board_at = None;
            return;
        }
        at.started = true;
        self.chess.board_at = Some(at);
        // A scan that does not start (the document forbids copying its content, say) leaves no
        // run: the next tick drops the double-click.
        self.start_chess_scan_pages(vec![at.page], true);
    }

    /// Add one batch to the results. Returns `false` when the document changed since the batch
    /// was made: its boxes describe a state that is gone, and it is dropped.
    fn keep_chess_scan(&mut self, scan: ChessScan, model: Option<ModelNote>) -> bool {
        let stamp = scan.stamp();
        if self.session.chess_stamp(scan.doc) != Some(stamp) {
            return false;
        }
        // Agents and the CLI in this session reuse what was found.
        self.session.store_chess_scan(&scan);
        let fresh = self.chess.results.as_ref().is_none_or(|r| r.doc != scan.doc || r.stamp != stamp);
        if fresh {
            self.chess.results =
                Some(ChessResults { doc: scan.doc, stamp, pages: Vec::new(), selected: None, reveal: false, model: ModelNote::Missing });
        }
        let Some(results) = self.chess.results.as_mut() else { return true };
        if let Some(m) = model {
            results.model = m;
        }
        for page in scan.pages {
            match results.pages.binary_search_by_key(&page.page, |p| p.page) {
                Ok(i) => {
                    if let Some(slot) = results.pages.get_mut(i) {
                        *slot = page;
                    }
                }
                Err(i) => results.pages.insert(i, page),
            }
        }
        if results.selected.is_some_and(|(p, i)| results.diagram(p, i).is_none()) {
            results.selected = None;
        }
        true
    }

    /// Pick diagram `index` on `page`. From the panel (`go`), the view goes to it and outlines it;
    /// from the page, the panel shows its entry.
    pub fn select_chess_diagram(&mut self, page: usize, index: usize, go: bool) {
        let Some((vi, id)) = self.active_ids() else { return };
        let Some(results) = self.chess.results.as_mut().filter(|r| r.doc == id) else { return };
        let Some((rect, _)) = results.diagram(page, index) else { return };
        results.selected = Some((page, index));
        let reading = results.pages.iter().find(|p| p.page == page).and_then(|p| ChessResults::reading(p, index));
        if self.chess.editor.as_ref().is_none_or(|e| (e.doc, e.page, e.index) != (id, page, index)) {
            self.chess.editor = Some(crate::chess_editor::DiagramEditor::new(id, page, index, reading));
        }
        results.reveal = !go;
        if go && let Some(view) = self.views.get_mut(vi) {
            view.go_to_page(page);
            view.flash = Some((page, rect, 0.0));
        }
        if !go {
            self.reveal_chess_diagrams();
        }
    }

    pub(crate) fn chess_action(&mut self, action: PanelAction) {
        match action {
            PanelAction::ScanPage => self.start_chess_scan(false),
            PanelAction::ScanAll => self.start_chess_scan(true),
            PanelAction::Cancel => self.cancel_chess_scan(),
            PanelAction::Select(page, index) => self.select_chess_diagram(page, index, true),
            // The Board panel is the study board (T5c): it comes forward, docked or in its window.
            PanelAction::OpenBoard(page, index) => {
                self.select_chess_diagram(page, index, false);
                self.open_chess_board(page, index);
            }
            PanelAction::ShowBoard => self.show_chess_board(),
            PanelAction::CopyFen(fen) => {
                let Some((_, id)) = self.active_ids() else { return };
                if !self.may_copy(id) {
                    return;
                }
                if let Some(ctx) = &self.ctx {
                    ctx.copy_text(fen);
                }
                self.notify(tl!("FEN copied").to_string());
            }
            // A lichess address PdfCraft built from the position, opened because the user asked.
            PanelAction::OpenLichess(url) => self.open_url(&url),
            PanelAction::Editor(a) => self.chess_editor_action(a),
            PanelAction::Text(a) => self.chess_text_action(a),
            PanelAction::DockWindow => self.bring_back_chess_window(),
            PanelAction::Models => self.choose_chess_models(),
        }
    }

    /// Chess board ▸ Analysis ▸ Choose engine: pick the UCI engine program for this session (the
    /// picker answers on a later frame; `pick_override` stands in for it in tests).
    pub fn choose_chess_engine(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut dialog = rfd::AsyncFileDialog::new().set_title(tl!("Choose engine…").to_string());
            if cfg!(windows) {
                dialog = dialog.add_filter(tl!("Programs"), &["exe"]);
            }
            self.ask_one(crate::pickers::Ask::File(dialog), None, |app, path| {
                app.chess.study.engine_path = Some(path);
                app.chess.study.analysis = true;
                app.chess.study.restart();
            });
        }
    }

    /// What the diagram editor asked for.
    fn chess_editor_action(&mut self, action: crate::chess_editor::EditorAction) {
        use crate::chess_editor::EditorAction;
        match action {
            EditorAction::CopyFen(fen) => self.chess_action(PanelAction::CopyFen(fen)),
            EditorAction::OpenLichess(url) => self.chess_action(PanelAction::OpenLichess(url)),
            EditorAction::Close => self.chess.editor = None,
            EditorAction::Previous | EditorAction::Next => {
                let Some(results) = self.chess.results.as_ref() else { return };
                let queue = results.review_queue();
                let at = self.chess.editor.as_ref().map(|e| (e.page, e.index)).or(results.selected);
                let next = match (action == EditorAction::Next, at) {
                    (true, Some(at)) => queue.iter().find(|q| **q > at).or(queue.first()),
                    (false, Some(at)) => queue.iter().rev().find(|q| **q < at).or(queue.last()),
                    (true, None) => queue.first(),
                    (false, None) => queue.last(),
                };
                if let Some(&(page, index)) = next {
                    self.select_chess_diagram(page, index, true);
                }
            }
            EditorAction::OpenBoard => {
                if let Some(ed) = self.chess.editor.as_ref() {
                    let source = crate::chess_board_ui::Source { doc: ed.doc, page: ed.page, index: ed.index };
                    let (fen, flipped) = (ed.fen(), ed.flipped);
                    // A new game in the document's games: the game on the board stays as it is.
                    if self.open_chess_game(&fen, flipped, Some(source)) {
                        self.show_chess_board();
                    }
                }
            }
            EditorAction::Replace => self.replace_chess_diagram(),
            EditorAction::Link => self.link_chess_diagram(),
        }
    }

    /// Draw the editor's position over its diagram on the page, then scan that page again.
    pub fn replace_chess_diagram(&mut self) {
        let Some(ed) = self.chess.editor.as_ref() else { return };
        let Some(generation) = self.session.get(ed.doc).map(|d| d.edit_generation()) else { return };
        let pieces = crate::chess_editor::piece_set(&mut self.chess.pieces);
        let replacement = pdfcraft_engine::chess::DiagramReplacement {
            page: ed.page,
            index: ed.index,
            generation,
            placement: ed.position.placement,
            style: pdfcraft_engine::chess::pdfcraft_chess::draw::BoardStyle { flipped: ed.flipped, ..Default::default() },
            pieces,
            scan: self.chess_scan_settings(),
        };
        if self.apply_edit(pdfcraft_engine::Edit::ReplaceDiagram(Box::new(replacement))) {
            self.chess.editor = None;
            self.notify(tl!("Diagram replaced").to_string());
            self.start_chess_scan(false);
        }
    }

    /// Put a lichess link to the editor's position under its diagram.
    pub fn link_chess_diagram(&mut self) {
        let Some(ed) = self.chess.editor.as_ref() else { return };
        let (id, page, index, fen) = (ed.doc, ed.page, ed.index, ed.fen());
        let settings = pdfcraft_engine::chess::LinkSettings {
            only: Some(pdfcraft_engine::chess::LinkTarget { page, index, fen }),
            ..pdfcraft_engine::chess::LinkSettings::default()
        };
        let scan = self.chess_scan_settings();
        match self.session.chess_link_plan(id, &[], &scan, &settings) {
            Ok(plan) => match plan.edit {
                Some(edit) => {
                    if self.apply_edit(edit) {
                        self.notify(tl!("Lichess link added").to_string());
                    }
                }
                None => {
                    let why = plan.skipped.first().map(|s| s.reason.clone()).unwrap_or_default();
                    self.notify(crate::i18n::fmt(tl!("No link was added: {e}"), &[("e", &why)]));
                }
            },
            Err(e) => self.notify(crate::i18n::fmt(tl!("No link was added: {e}"), &[("e", &e)])),
        }
    }

    /// Put diagram `index` of `page` on the chess board and show the Board panel.
    pub fn open_chess_board(&mut self, page: usize, index: usize) {
        if self.load_chess_board(page, index) {
            self.show_chess_board();
        }
    }

    /// Put diagram `index` of `page` on the chess board, as a new game of the document (the game
    /// on the board stays as it was): the position being edited when the editor has that diagram,
    /// else the position read. Whether there was one (else the user is told why not).
    pub fn load_chess_board(&mut self, page: usize, index: usize) -> bool {
        let Some((_, id)) = self.active_ids() else { return false };
        let source = crate::chess_board_ui::Source { doc: id, page, index };
        let edited = self.chess.editor.as_ref().filter(|e| (e.doc, e.page, e.index) == (id, page, index)).map(|e| (e.fen(), e.flipped));
        let read = || {
            let results = self.chess.results.as_ref().filter(|r| r.doc == id)?;
            let reading = results.pages.iter().find(|p| p.page == page).and_then(|p| ChessResults::reading(p, index))?;
            let pos = reading.position()?;
            Some((pos.fen.clone(), pos.orientation.is_some_and(|o| o.black_point_of_view)))
        };
        // The editor is open on the diagram picked, changed or not: changed, its position is the
        // user's and goes on the board as it is.
        let changed = self
            .chess
            .editor
            .as_ref()
            .filter(|e| (e.doc, e.page, e.index) == (id, page, index))
            .is_some_and(|e| e.original.as_deref() != Some(e.fen().as_str()));
        let Some((fen, flipped)) = edited.or_else(read) else {
            self.notify(tl!("This diagram hasn't been read: scan its page first, or set its position in the editor.").to_string());
            return false;
        };
        let fen = if changed { fen } else { self.chess_text_start(id, page, index, &fen).unwrap_or(fen) };
        self.open_chess_game(&fen, flipped, Some(source))
    }

    /// The start of the game the Text tab's reading of `page` begins at diagram `index`, when it
    /// has the pieces of `fen`: the moves after the diagram (their first number, a "position
    /// after" caption) settle its side to move and move number better than the reading alone.
    fn chess_text_start(&self, id: DocId, page: usize, index: usize, fen: &str) -> Option<String> {
        use pdfcraft_engine::chess::pdfcraft_chessvision::doc::GameOrigin;
        let stamp = self.session.chess_stamp(id)?;
        let text = self.chess.text.as_ref().filter(|t| t.doc == id && t.page == page && t.stamp == stamp)?;
        let game = text.chess.games.iter().find(
            |g| matches!(g.origin, GameOrigin::Diagram { page: p, diagram, .. } if usize::try_from(p).ok() == Some(page) && diagram == index),
        )?;
        let start = game.score.start_fen.as_deref()?;
        // The walker settles the side and the number, never the pieces.
        (start.split_whitespace().next() == fen.split_whitespace().next()).then(|| start.to_owned())
    }

    /// Open the chess board panel and bring it to the front. The docking workspace does it when it
    /// is next drawn: it may be drawing now (this is often called from a panel), holding the layout.
    /// Read mode draws no workspace, so there it is done at once.
    pub fn show_chess_board(&mut self) {
        // In its own window (T13a), the board is brought to the front instead of docked.
        if self.raise_chess_window() {
            return;
        }
        let panel = crate::docking::Panel::Board;
        if self.mode == crate::Mode::Read {
            let opened = self.docking.set_visible(panel, true).and_then(|()| self.docking.apply(craft_ui::docking::Action::Activate { panel }));
            if let Err(e) = opened {
                self.notify_fmt("Panel move was not applied: {error}", &[("error", &e)]);
            }
        } else {
            self.pending_panel_reveals.push(panel);
        }
    }

    /// What the chess board panel asked for.
    pub(crate) fn chess_board_action(&mut self, action: crate::chess_board_ui::BoardAction) {
        use crate::chess_board_ui::BoardAction;
        match action {
            BoardAction::CopyFen(fen) => self.copy_from_board(fen, tl!("FEN copied")),
            BoardAction::CopyPgn(pgn) => self.copy_from_board(pgn, tl!("PGN copied")),
            BoardAction::CopyLine(line) => self.copy_from_board(line, tl!("Line copied")),
            // A lichess address PdfCraft built from the position, opened because the user asked.
            BoardAction::OpenLichess(url) => self.open_url(&url),
            BoardAction::ShowSource(s) => {
                if self.active_ids().is_some_and(|(_, id)| id == s.doc) {
                    self.select_chess_diagram(s.page, s.index, true);
                }
            }
            BoardAction::ChooseEngine => self.choose_chess_engine(),
        }
    }

    /// The commands "Copy line with figurines" and "Copy line (letters)" (T15): the line of the
    /// move shown, from the start of its variation (of the game, on the main line), as a move's
    /// menu copies it.
    pub(crate) fn copy_chess_line(&mut self, figurines: bool) {
        let board = self.chess.board();
        if board.illegal.is_some() || board.game.current() == 0 {
            return;
        }
        let line = board.game.line_text(board.game.current(), figurines);
        self.copy_from_board(line, tl!("Line copied"));
    }

    /// Copy what the board shows, and say so. A position read from a document is that
    /// document's content: its copy permission applies while it is open.
    fn copy_from_board(&mut self, text: String, done: &str) {
        if let Some(s) = self.chess.board().source
            && self.session.get(s.doc).is_some()
            && !self.may_copy(s.doc)
        {
            return;
        }
        if let Some(ctx) = &self.ctx {
            ctx.copy_text(text);
        }
        self.notify(done.to_string());
    }
}

/// The chess board panel, in the docking workspace or beside the page in Read mode.
pub(crate) fn board_panel(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    // A click anywhere in the panel puts Undo and Redo on the game (D12).
    if crate::chess_history::pressed_in(ui) {
        app.chess.enter_scope();
    }
    let pieces = crate::chess_editor::board_piece_set(&mut app.chess.board_pieces);
    let source_label = board_source_label(app);
    let mut action = None;
    let mut games = None;
    let mut paste = None;
    // The PGN tool rail on the left edge, at the full height and outside the scroll area.
    egui::Frame::NONE.fill(t.panel).stroke(Stroke::new(1.0, t.divider)).show(ui, |ui| {
        crate::chess_tools::with_rail(ui, &mut app.chess, |ui, chess| {
            egui::Frame::NONE.inner_margin(egui::Margin { left: 10, right: 12, top: 12, bottom: 8 }).show(ui, |ui| {
                // Captures from the clipboard (T7a) and the layer of captured text (T7b), above the board.
                paste = crate::chess_capture::panel_row(ui, chess);
                // The notation takes the height the scroll area shows and the rest leaves.
                let viewport = ui.available_height();
                egui::ScrollArea::vertical().id_salt("chess-board-panel").auto_shrink([false, false]).show(ui, |ui| {
                    (action, games) =
                        crate::chess_board_ui::panel_body(ui, &t, chess, &pieces, source_label, crate::chess_board_ui::Sizing::DOCKED, viewport);
                });
            });
        });
    });
    if let Some(a) = action {
        app.chess_board_action(a);
    }
    if let Some(a) = games {
        app.chess_games_action(a);
    }
    if let Some(mode) = paste {
        app.chess_paste_as(mode, crate::chess_capture::clipboard_text());
    }
}

/// The diagram the board came from, when it belongs to the document shown: "Diagram 2 · Page 14".
pub(crate) fn board_source_label(app: &PdfCraftApp) -> Option<String> {
    let active = app.active_ids().map(|(_, id)| id);
    app.chess.board().source.filter(|s| active == Some(s.doc)).map(|s| {
        let label = app.session.get(s.doc).and_then(|d| d.info.pages.get(s.page)).map_or_else(|| (s.page + 1).to_string(), |p| p.label.clone());
        let title = crate::i18n::fmt(tl!("Diagram {n}"), &[("n", &(s.index + 1).to_string())]);
        let page = crate::i18n::fmt(tl!("Page {label}"), &[("label", &label)]);
        format!("{title} · {page}")
    })
}
