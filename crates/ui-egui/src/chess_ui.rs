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

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use egui::{Color32, CornerRadius, Pos2, Stroke};
use pdfcraft_engine::DocId;
use pdfcraft_engine::chess::corrections::{Kind, corrected_position};
use pdfcraft_engine::chess::pdfcraft_chessvision::board::{BoardReading, ReadPosition, Unreadable};
use pdfcraft_engine::chess::pdfcraft_chessvision::pieces::choice::{PieceChoice, Role};
use pdfcraft_engine::chess::{ChessModels, ChessPage, ChessScan, ChessScanJob, ChessScanSteps, DiagramAt, ModelState, ScanStamp};
use pdfcraft_render::DocInfo;

use crate::PdfCraftApp;
use crate::canvas::PageXform;
use crate::chess_widgets::DiagramState;
use crate::theme::{self, Tokens};

/// Pages scanned per job. Each job opens the file in its own renderer, so smaller batches show
/// results sooner and larger ones parse the file fewer times.
const PAGES_PER_JOB: usize = 8;

/// One open document's chess view (plan A9-I1a, §2.7): what the Chess panel shows of it, kept
/// while another document is shown or scanned. Made and dropped with the documents
/// ([`PdfCraftApp::sync_chess_docs`]). What the document's reads taught (its book cipher), what the
/// user said of it (its book origin), its corrections and its book's PGN with the decisions live
/// in the engine's session (plan A9-I1b).
#[derive(Default)]
pub struct DocChess {
    /// What scanning found in it.
    pub results: Option<ChessResults>,
    /// The page the Text tab last read in it.
    pub text: Option<crate::chess_text_ui::PageText>,
    /// The Diagrams list's filter, its keyboard and what it follows (plan A8, P2a1).
    pub(crate) list: crate::chess_list::ListState,
    /// Which diagrams the PGN export gives a game of their own, for the session (plan A8, P9c).
    pub pgn_positions: pdfcraft_engine::chess::pdfcraft_chessvision::doc::markdown::PgnPositions,
    /// Where the PGN export takes its games from when the document keeps a book's PGN, for the
    /// session (plan "Carregar PGN do livro", R6b).
    pub pgn_source: crate::chess_export_ui::PgnSource,
    /// The corrections kept and forgotten in it, as steps Undo and Redo take (plan A9-G10).
    pub(crate) fixes: crate::chess_history::History<crate::chess_fixes::FixState>,
    /// A PGN beside the PDF with its name (`book.pdf` → `book.pgn`), offered while the document
    /// keeps none (R6c; desktop only).
    pub(crate) pgn_sibling: Option<std::path::PathBuf>,
    /// What Link the whole book found, for the session (R6d).
    pub(crate) book_link: Option<crate::chess_reference_ui::BookLink>,
    /// The user's answer about keeping its chess, when it is encrypted and was asked (plan
    /// A9-I1c, [`crate::chess_keep`]): for the session.
    pub(crate) keep: Option<bool>,
}

/// Chess diagrams: each open document's view, and the running scan.
#[derive(Default)]
pub struct ChessState {
    /// Each open document's view (plan A9-I1a): scanning one document never empties another's
    /// list (finding A6-01).
    pub docs: HashMap<DocId, DocChess>,
    /// The document shown, as the last pass found it: whose view [`ChessState::results`] and
    /// [`ChessState::text`] give.
    pub shown: Option<DocId>,
    pub run: Option<ChessRun>,
    /// A diagram clicked on the page this frame, picked at the next poll.
    pub picked: Option<DiagramAt>,
    /// The diagram picked was double-clicked: it goes onto the chess board too.
    pub to_board: bool,
    /// The last diagram clicked on the page and when (egui's input time), to tell a double-click.
    pub last_pick: Option<(DiagramAt, f64)>,
    /// A double-click on a page not scanned yet: the diagram there goes onto the board once the
    /// page is scanned ([`PdfCraftApp::chess_board_at_tick`]).
    pub board_at: Option<BoardAt>,
    /// A diagram to open in the editor once its page's scan is in ([`PickAfterScan`]).
    pub pick_after_scan: Option<PickAfterScan>,
    /// A diagram put on the board before the Text tab read its page: the page is read, and the
    /// moves under the diagram give the game its side to move and move number if nothing was
    /// done on it meanwhile ([`PdfCraftApp::chess_board_text_tick`]).
    pub board_text: Option<BoardText>,
    /// The chess games, per document: the board panels show the open document's active one
    /// ([`ChessState::board`]).
    pub games: crate::chess_collection::ChessGames,
    /// The selected diagram's position being edited (S17), in the Diagram tab (plan A8, P3b2).
    pub editor: Option<crate::chess_editor::DiagramEditor>,
    /// The corrections the user confirmed, and where they are kept (plan A8, P3b2).
    pub corrections: crate::chess_corrections_ui::Corrections,
    /// The books' own PGNs, and where they are kept (plan "Carregar PGN do livro", R6a).
    pub book_pgns: crate::chess_reference_ui::BookPgns,
    /// Edit diagrams in the Diagrams panel as before the Diagram tab (a preference, for one
    /// version).
    pub classic_editor: bool,
    /// The Diagram tab's main button is Only confirm (else Confirm and next): the last choice.
    pub confirm_only: bool,
    /// Changes made in the Diagram tab and not confirmed, per diagram, for the session (P3b3).
    pub drafts: Vec<crate::chess_drafts::Draft>,
    /// The question asked about a draft before closing, quitting, exporting or detecting again.
    pub draft_prompt: Option<crate::chess_drafts::DraftPrompt>,
    /// The question asked before quitting or closing about chess games the autosave doesn't keep
    /// (plan A9-G8).
    pub games_prompt: Option<crate::chess_unsaved::GamesPrompt>,
    /// The Diagram tab's first-time hint was read (dismissed, or a diagram confirmed).
    pub diagram_hint_seen: bool,
    /// Redraw in the PDF draws at once: its first-time question was answered with Don't show
    /// again (plan A8, P3b4).
    pub redraw_asked: bool,
    /// The first-time question before Redraw in the PDF, while it is asked.
    pub redraw_prompt: Option<crate::chess_redraw::RedrawPrompt>,
    /// The question before forgetting a correction with changes over it not confirmed yet, while
    /// it is asked (plan A9-G10).
    pub forget_prompt: bool,
    /// The encrypted documents to ask about keeping their chess, the first asked now (plan
    /// A9-I1c, [`crate::chess_keep`]).
    pub(crate) keep_asking: Vec<DocId>,
    /// The question before exporting while diagrams are still to review (P9c).
    pub export_prompt: Option<crate::chess_export_ui::PendingPrompt>,
    /// What the last export said, while it is shown (P9c).
    pub export_done: Option<crate::chess_export_ui::Done>,
    /// The file Open folder last showed (what tests see instead of the file manager).
    pub last_revealed: Option<std::path::PathBuf>,
    /// The Diagram tab's keys and what it says to screen readers (plan A8, P3c).
    pub(crate) diagram_keys: crate::chess_diagram_keys::DiagramKeys,
    /// The review just finished, (checked, of): the Diagram tab says so until another diagram
    /// opens (plan A8, P4).
    pub(crate) review_done: Option<Reviewed>,
    /// The chess panel offers to collapse the tools panel this frame (plan A8, P5).
    pub(crate) tools_offer: bool,
    /// The offer was answered (Collapse or Not now): it is not made again. Kept in the settings.
    pub tools_offer_answered: bool,
    /// The pieces the editor draws with, found once.
    pub pieces: Option<Arc<pdfcraft_engine::chess::pdfcraft_chess::draw::PieceSet>>,
    /// The pieces the analysis board draws with, found once (the user's piece pictures first).
    pub board_pieces: Option<Arc<pdfcraft_engine::chess::pdfcraft_chess::draw::PieceSet>>,
    /// The piece sets chosen in Preferences (plan HC2): the boards', and the diagrams' and
    /// exports' (which the editor draws too).
    pub board_piece_choice: PieceChoice,
    pub diagram_piece_choice: PieceChoice,
    /// Why the set chosen for the boards, and for diagrams, could not be used, when it could not.
    pub board_piece_error: Option<String>,
    pub diagram_piece_error: Option<String>,
    /// The boards' theme chosen in Preferences or the board's menu (plan HC4); `None` is the
    /// default, brown.
    pub board_theme: Option<&'static pdfcraft_engine::chess::pdfcraft_chess::theme::BoardTheme>,
    /// How the boards are laid out (plan HC6).
    pub board_look: crate::chess_theme::BoardLook,
    /// How the move list writes the pieces (plan HC8).
    pub move_pieces: crate::chess_figurines::MovePieces,
    /// The comment on the move shown, large under the board (plan HC12).
    pub comment_focus: bool,
    /// The move list's layout and text size (plan HC9).
    pub list_look: crate::chess_figurines::ListLook,
    /// No preview of a move's position when the pointer rests on it (plan HC13; shown by
    /// default).
    pub no_move_preview: bool,
    /// What a step forward at a fork and a move played beside the reply there do (plan A8 V3).
    pub variation_rules: crate::chess_moves_tree::VariationRules,
    /// Preferences' piece picker while it is shown.
    pub(crate) piece_picker: Option<crate::chess_pieces_ui::PiecePicker>,
    /// The panel's tab: the diagrams or the page's recognised text (S16).
    pub tab: crate::chess_text_ui::ChessTab,
    /// What the Board panel adds as the study board: set-up, engine analysis (the board itself
    /// is [`ChessState::board`]).
    pub study: crate::chess_study_ui::StudyState,
    /// A page being read off the interface thread (or the pages around a move's game): the last
    /// read asked takes the place of the one running (plan A9-I2).
    pub reading: Option<crate::chess_text_ui::ChessJob>,
    /// The time a frame gives a scan run a page at a time, as the web runs it (tests; the web
    /// takes 50 ms when `None`, the desktop scans on a worker; plan A9-I2).
    pub frame_budget_ms: Option<f64>,
    /// The book being exported or aligned to its PGN, off the interface thread, one at a time:
    /// a slot of its own, so the page read never waits for it (plan A9-I2, finding A5-12).
    pub long: Option<crate::chess_text_ui::ChessJob>,
    /// How many pages the last finished job scanned anew, not given from what the session had
    /// read (plan A9, G11): shown nowhere, for tests and measurements.
    pub job_scanned: Option<usize>,
    /// Undo and Redo go to the chess game, not the document: the last action was a click in the
    /// chess panels (D12), or the keyboard focus was there (plan A9-I18). See
    /// [`ChessState::in_scope`], which also counts changes to the game.
    pub scope: bool,
    /// The game shown (its id) and its history's step count when the scope was last settled.
    pub(crate) scope_edits: (u64, u64),
    /// The last action was a correction kept or forgotten, or one of theirs undone or redone:
    /// Undo and Redo go to the shown document's corrections (plan A9-G10;
    /// [`ChessState::undo_target`]) until a click on the page or in the chess board's panels, a
    /// change to the document or the keyboard taken to the page.
    pub(crate) fix_scope: bool,
    /// A widget outside the chess board's surfaces had the keyboard focus at the end of the last
    /// pass: the scope is off however it was settled (plan A9-I18).
    pub(crate) focus_out: bool,
    /// The chess board's surfaces drawn this pass, with their window and layer
    /// ([`ChessState::mark_surface`]).
    pub(crate) surfaces: Vec<(egui::ViewportId, egui::LayerId, egui::Rect)>,
    /// Preferences ▸ Chess: the one-key shortcuts (B, C, F, G, O, X, Shift with a group's letter
    /// and the typed signs) are off; the rail's buttons and the commands still act (plan A9-I18,
    /// WCAG 2.1.4). On by default.
    pub one_key_off: bool,
    /// Preferences ▸ Chess: how long a typed sign waits for its next character (plan A9-I18).
    pub typed_wait: crate::chess_annotate::TypedWait,
    /// The PGN tool rail: the tool armed and the keys the chess scope took (T5a).
    pub tools: crate::chess_tools::ToolState,
    /// Text captured from the page or the clipboard (T7a).
    pub capture: crate::chess_capture::CaptureState,
    /// Follow, and the move's place last shown on the page (T8).
    pub source: crate::chess_source::SourceState,
    /// Where the chess models come from: the folder chosen, or the files picked on the web (U4).
    pub models: crate::chess_models::ModelsPref,
    /// The Move list shows beside the page this frame: the notation is drawn there, not under
    /// the board (in the dock or in the chess window).
    pub(crate) moves_beside: bool,
    /// The Move list stands against the page's left edge (else its right one), as of this frame.
    pub(crate) moves_left: bool,
    /// What was asked of the Move list, done at the start of the next frame, when the layout is
    /// not being drawn (`docking::drain_moves`).
    pub(crate) moves_request: Option<crate::docking::MovesRequest>,
}

impl ChessState {
    /// Where the last pass drew the Diagram tab, in points, or `None` when it was not drawn.
    /// The layout tests check that the tab's board stays inside it.
    pub fn diagram_tab_rect(&self) -> Option<egui::Rect> {
        self.diagram_keys.tab
    }

    /// The diagrams the Diagram tab walks in document `id` (plan A8, P4): the state its Diagrams
    /// list's chip shows, or, with none, the ones to review.
    pub(crate) fn walk(&self, id: DocId) -> Vec<DiagramAt> {
        self.results_of(id).map_or_else(Vec::new, |r| r.walk(self.filter(id)))
    }

    /// Document `id`'s view, when it has one.
    pub fn doc(&self, id: DocId) -> Option<&DocChess> {
        self.docs.get(&id)
    }

    /// Document `id`'s view, made when it has none.
    pub(crate) fn doc_mut(&mut self, id: DocId) -> &mut DocChess {
        self.docs.entry(id).or_default()
    }

    /// What scanning found in document `id`.
    pub fn results_of(&self, id: DocId) -> Option<&ChessResults> {
        self.docs.get(&id)?.results.as_ref()
    }

    pub fn results_of_mut(&mut self, id: DocId) -> Option<&mut ChessResults> {
        self.docs.get_mut(&id)?.results.as_mut()
    }

    /// What scanning found in the document shown.
    pub fn results(&self) -> Option<&ChessResults> {
        self.results_of(self.shown?)
    }

    pub fn results_mut(&mut self) -> Option<&mut ChessResults> {
        let id = self.shown?;
        self.results_of_mut(id)
    }

    /// The page the Text tab last read in document `id`.
    pub fn text_of(&self, id: DocId) -> Option<&crate::chess_text_ui::PageText> {
        self.docs.get(&id)?.text.as_ref()
    }

    /// The page the Text tab last read in the document shown.
    pub fn text(&self) -> Option<&crate::chess_text_ui::PageText> {
        self.text_of(self.shown?)
    }

    /// The document shown's Diagrams list (its tests read where the keyboard is).
    #[cfg(test)]
    pub(crate) fn shown_list(&self) -> &crate::chess_list::ListState {
        self.shown.and_then(|id| self.docs.get(&id)).map(|d| &d.list).expect("a document is shown")
    }

    /// The state document `id`'s Diagrams list shows (`None`: all of them).
    pub(crate) fn filter(&self, id: DocId) -> Option<DiagramState> {
        self.docs.get(&id).and_then(|d| d.list.filter)
    }

    /// The piece set chosen for `role`.
    pub(crate) fn piece_choice(&self, role: Role) -> &PieceChoice {
        match role {
            Role::Board => &self.board_piece_choice,
            Role::Diagrams => &self.diagram_piece_choice,
        }
    }

    /// Why the set chosen for `role` could not be used, when it could not.
    pub(crate) fn piece_error(&self, role: Role) -> Option<&str> {
        match role {
            Role::Board => self.board_piece_error.as_deref(),
            Role::Diagrams => self.diagram_piece_error.as_deref(),
        }
    }

    /// `role`'s pieces, found once (and why the chosen set could not be used, kept).
    pub(crate) fn role_pieces(&mut self, role: Role) -> Arc<pdfcraft_engine::chess::pdfcraft_chess::draw::PieceSet> {
        match role {
            Role::Board => crate::chess_editor::board_piece_set(&mut self.board_pieces, &self.board_piece_choice, &mut self.board_piece_error),
            Role::Diagrams => crate::chess_editor::piece_set(&mut self.pieces, &self.diagram_piece_choice, &mut self.diagram_piece_error),
        }
    }
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

/// A diagram to open in the editor once its page's scan is in (plan A9-I4, findings A5-04, A5-09):
/// "Diagram N" of the Text tab, whose page may not be the one in view, and Keep editing a draft of
/// another document. It waits for its page's scan as a double-click on the page does
/// ([`BoardAt`]), and goes when its document is no longer the one shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PickAfterScan {
    pub doc: DocId,
    pub at: DiagramAt,
    /// The scan of its page was started for it.
    pub started: bool,
}

/// A diagram put on the board (diagram `index` of `page` of `doc`, its game started from `fen`)
/// whose page the Text tab had not read.
#[derive(Clone, Debug, PartialEq)]
pub struct BoardText {
    pub doc: DocId,
    pub page: usize,
    pub index: usize,
    pub fen: String,
    /// The page's reading was started for it.
    pub started: bool,
}

/// A correction over a diagram, as the engine's one rule gives it the diagram
/// ([`pdfcraft_engine::chess::corrections::assign`], plan A9-G2b): Verified or Corrected, the
/// position kept, and which correction it is.
#[derive(Clone, Debug, PartialEq)]
pub struct Kept {
    pub uid: String,
    pub kind: Kind,
    pub fen: String,
}

/// How many diagrams needed review, and how many of those the user has confirmed
/// ([`ChessResults::reviewed`]): the review is done when they are equal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reviewed {
    pub checked: usize,
    pub of: usize,
}

/// What scanning found in one document.
pub struct ChessResults {
    pub doc: DocId,
    /// The document state the pages were scanned in.
    pub stamp: ScanStamp,
    /// The scanned pages, by page number, each once.
    pub pages: Vec<Arc<ChessPage>>,
    /// The diagram picked in the panel or on the page.
    pub selected: Option<DiagramAt>,
    /// The panel scrolls the picked diagram into view once (it was picked on the page).
    pub reveal: bool,
    /// Whether the piece model could be used.
    pub model: ModelNote,
    /// What the user did with each diagram: the correction the engine's one rule gives it
    /// ([`PdfCraftApp::refresh_chess_kinds`]; plan A9-G2b).
    pub kinds: HashMap<DiagramAt, Kept>,
    /// The diagrams whose reading the book's PGN has, unless the user confirmed them
    /// ([`PdfCraftApp::refresh_chess_kinds`]; plan "Carregar PGN do livro", R6b).
    pub confirmed: std::collections::HashSet<DiagramAt>,
    /// The pages scans were asked for (ascending), and whether one was of every page: what an
    /// edit carries to the document's next state ([`PdfCraftApp::carry_chess_results`]).
    pub wanted: Vec<usize>,
    pub all: bool,
}

impl ChessResults {
    /// The list covers `pages` too (`all`: every page).
    fn ask(&mut self, pages: &[usize], all: bool) {
        self.all |= all;
        self.wanted.extend_from_slice(pages);
        self.wanted.sort_unstable();
        self.wanted.dedup();
    }

    /// How many diagrams were found.
    pub fn count(&self) -> usize {
        self.pages.iter().map(|p| p.diagrams.len()).sum()
    }

    /// The diagrams of state `filter` in reading order, as the Diagrams list shows them, or, with
    /// none, the ones to review ([`Self::review_queue`], which takes the ones not read too): what
    /// the Diagram tab's arrows, Page Up / Page Down and Confirm and next walk, so they follow
    /// the list's chip (plan A8, P4).
    pub(crate) fn walk(&self, filter: Option<DiagramState>) -> Vec<DiagramAt> {
        match filter {
            None => self.review_queue(),
            Some(state) => self
                .pages
                .iter()
                .flat_map(|p| (0..p.diagrams.len()).filter(move |i| self.state(p, *i) == state).map(|i| DiagramAt::new(p.page, i)))
                .collect(),
        }
    }

    /// How many diagrams needed review, and how many of those the user has confirmed: (checked,
    /// of). The review is done when they are equal.
    pub fn reviewed(&self) -> Reviewed {
        let mut out = Reviewed::default();
        for p in &self.pages {
            for i in 0..p.diagrams.len() {
                if crate::chess_editor::needs_review(Self::reading(p, i)) {
                    out.of = out.of.saturating_add(1);
                    let at = DiagramAt::new(p.page, i);
                    if self.kinds.contains_key(&at) || self.confirmed.contains(&at) {
                        out.checked = out.checked.saturating_add(1);
                    }
                }
            }
        }
        out
    }

    /// The diagrams to review, in reading order: page, then index. A diagram the user confirmed
    /// (Verified or Corrected), or whose reading the book's PGN has, is not among them.
    pub fn review_queue(&self) -> Vec<DiagramAt> {
        self.pages
            .iter()
            .flat_map(|p| {
                (0..p.diagrams.len()).map(|i| DiagramAt::new(p.page, i)).filter(|at| {
                    !self.kinds.contains_key(at) && !self.confirmed.contains(at) && crate::chess_editor::needs_review(Self::reading(p, at.index))
                })
            })
            .collect()
    }

    /// What the user did with diagram `index` of `page`, when they confirmed it.
    pub fn kind(&self, page: usize, index: usize) -> Option<Kind> {
        self.kinds.get(&DiagramAt::new(page, index)).map(|k| k.kind)
    }

    /// The position kept for diagram `index` of `page`, when the user confirmed it.
    pub fn kept_fen(&self, page: usize, index: usize) -> Option<&str> {
        self.kinds.get(&DiagramAt::new(page, index)).map(|k| k.fen.as_str())
    }

    /// Whether the book's PGN has the reading of diagram `index` of `page`.
    pub fn confirmed(&self, page: usize, index: usize) -> bool {
        self.confirmed.contains(&DiagramAt::new(page, index))
    }

    /// Diagram `index` of `page`'s state.
    pub(crate) fn state(&self, page: &ChessPage, index: usize) -> DiagramState {
        DiagramState::of(Self::reading(page, index), self.kind(page.page, index), self.confirmed(page.page, index))
    }

    /// The reading of diagram `index` on `page`, when it was read.
    pub(crate) fn reading(page: &ChessPage, index: usize) -> Option<&BoardReading> {
        page.readings.as_ref()?.iter().find(|r| r.index == index)
    }

    /// The position diagram `index` of `page` shows (plan A9-I4, finding A5-05): the correction
    /// the engine's one rule gave it (`corrections::assign`, kept in [`ChessResults::kinds`]) as the
    /// engine reads a correction ([`corrected_position`]), else the reading's. The one place the
    /// list's row, its menu, the board and the lichess link take it from: a diagram not read but
    /// corrected has one, and a corrected diagram links to its correction.
    pub(crate) fn effective<'a>(&'a self, page: &'a ChessPage, index: usize) -> Option<Cow<'a, ReadPosition>> {
        let read = Self::reading(page, index).and_then(BoardReading::position);
        match self.kept_fen(page.page, index).and_then(|fen| corrected_position(fen, read)) {
            Some(kept) => Some(Cow::Owned(kept)),
            None => read.map(Cow::Borrowed),
        }
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

/// A running scan: pages done of the total, and the batches finished and not yet shown.
#[derive(Default)]
pub struct ChessProgress {
    pub done: usize,
    pub total: usize,
    pub scans: Vec<ChessScan>,
    pub model: Option<ModelNote>,
}

pub struct ChessRun {
    pub doc: DocId,
    /// Started by a double-click on the page (`BoardAt`): the board shows what came of it, so the
    /// scan neither brings the Chess panel forward nor announces the count of diagrams found.
    pub quiet: bool,
    pub progress: Arc<Mutex<ChessProgress>>,
    /// The scan's work (plan A9-I2): on a worker, or a few pages a frame.
    pub(crate) work: ScanWork,
    /// Diagrams in the batches shown so far.
    pub found: usize,
    /// A batch was dropped because the document changed during the scan.
    pub stale: bool,
    /// The pages it scans, and whether they are every page.
    pub pages: Vec<usize>,
    pub all: bool,
}

/// How a scan runs (plan A9-I2).
pub(crate) enum ScanWork {
    /// On a worker thread (the desktop): done when it gives its result, stopped by its cancel.
    Thread(crate::chess_job::Job<()>),
    /// A page at a time, as many as a frame's budget allows (the web, which has no threads;
    /// finding R8-03: the whole book used to be scanned inside one frame, the tab frozen and Stop
    /// out of reach).
    Steps(Box<StepScan>),
}

impl ScanWork {
    /// Ask the scan to stop at its next page.
    fn cancel(&mut self) {
        match self {
            ScanWork::Thread(job) => job.cancel(),
            ScanWork::Steps(steps) => steps.cancelled = true,
        }
    }
}

/// A scan run a page at a time on the interface thread ([`ScanWork::Steps`]).
pub(crate) struct StepScan {
    models: Arc<ChessModels>,
    jobs: std::collections::VecDeque<ChessScanJob>,
    current: Option<ChessScanSteps>,
    /// Pages of the batches finished.
    base: usize,
    cancelled: bool,
}

impl StepScan {
    /// Scan pages until `budget_ms` milliseconds have gone (one page at least), the batches
    /// finished into `progress`. Whether the scan is done.
    fn advance(&mut self, progress: &Mutex<ChessProgress>, budget_ms: f64) -> bool {
        let start = now_ms();
        loop {
            let Some(steps) = self.current.as_mut() else {
                match self.jobs.pop_front().filter(|_| !self.cancelled) {
                    Some(job) => {
                        self.current = Some(job.steps(&self.models));
                        continue;
                    }
                    None => return true,
                }
            };
            if self.cancelled || steps.step(&self.models).is_none() {
                // The batch is done (or stopped): its pages are shown.
                if let Some(steps) = self.current.take() {
                    self.base = self.base.saturating_add(steps.done());
                    lock(progress).scans.push(steps.finish());
                }
                continue;
            }
            lock(progress).done = self.base.saturating_add(steps.done());
            if now_ms() - start >= budget_ms {
                return false;
            }
        }
    }
}

/// Milliseconds on a clock that only goes forward (the browser's on the web, where
/// `std::time::Instant` has none).
#[cfg(target_arch = "wasm32")]
fn now_ms() -> f64 {
    js_sys::Date::now()
}

#[cfg(not(target_arch = "wasm32"))]
fn now_ms() -> f64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64() * 1000.0
}

/// A frame's time for a scan run a page at a time, on the web.
const WEB_FRAME_BUDGET_MS: f64 = 50.0;

/// The progress, even after a worker panicked while holding it (it is plain data).
fn lock(p: &Mutex<ChessProgress>) -> MutexGuard<'_, ChessProgress> {
    p.lock().unwrap_or_else(|e| e.into_inner())
}

/// What the panel asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum PanelAction {
    ScanPage,
    ScanAll,
    Cancel,
    Select(DiagramAt),
    /// Put the diagram on the chess board.
    OpenBoard(DiagramAt),
    /// Put the diagram on the chess board and analyse it (plan A8, P7).
    Analyze(DiagramAt),
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
    /// Go to this page (0-based): its header in the list was clicked (plan A8, P2a1).
    GoToPage(usize),
    /// Review now: open the first diagram of the review queue (plan A8, P2a2).
    ReviewNow,
    /// Mark the page as checked (0-based): its diagrams read with confidence become Verified
    /// (plan A8, P2b).
    VerifyPage(usize),
    /// The answer to the offer to collapse the tools panel (plan A8, P5).
    ToolsOffer {
        collapse: bool,
    },
    /// The answer to the offer of the PGN beside the PDF (plan "Carregar PGN do livro", R6f).
    PgnOffer {
        use_it: bool,
    },
}

/// One diagram's state: its label, and its mark's colour from the theme's state tokens (plan A8
/// P1a). What the user did with it (`kind`) outranks the reading, and the book's PGN having it
/// (`confirmed`) outranks a doubt.
pub(crate) fn status(reading: Option<&BoardReading>, kind: Option<Kind>, confirmed: bool, t: &Tokens) -> (String, Color32) {
    let state = DiagramState::of(reading, kind, confirmed);
    let colour = state.colour(t);
    let label = match reading.map(|r| &r.outcome) {
        _ if kind.is_some() || state == DiagramState::Reference => state.name().to_string(),
        Some(Ok(p)) if p.accepted => tl!("Position read").to_string(),
        Some(Ok(_)) => tl!("Needs review").to_string(),
        Some(Err(Unreadable::NoModel)) => tl!("Not read: no piece model").to_string(),
        Some(Err(e)) => crate::i18n::fmt(tl!("Couldn't be read: {e}"), &[("e", &e.to_string())]),
        None => tl!("Not read").to_string(),
    };
    (label, colour)
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
    state.results_of(id).filter(|r| Some(r.stamp) == current)
}

/// Outline the diagrams of `page` and number them (plan A8, P2b): a 1.5-point stroke in the
/// state's colour at rest, an 8 % fill under the pointer, a 2.5-point stroke and an accent ring
/// when selected; the number in a tab outside the box, above it (below when there is no room)
/// with the state's 10-point icon, in the state's text colour on its soft fill (4.5:1). Returns
/// the diagram under `pointer` (its index on the page) with its hover text.
pub(crate) fn paint(
    painter: &egui::Painter,
    xf: &PageXform,
    page: usize,
    info: &DocInfo,
    results: Option<&ChessResults>,
    pointer: Option<Pos2>,
    drafts: &[DiagramAt],
) -> Option<(usize, String)> {
    let results = results?;
    let tokens = Tokens::get(painter.ctx());
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
        let kind = results.kind(page, i);
        let state = results.state(p, i);
        let (label, colour) = status(reading, kind, results.confirmed(page, i), &tokens);
        let selected = results.selected == Some(DiagramAt::new(page, i));
        let under = hovered.is_none() && pointer.is_some_and(|q| r.contains(q));
        if under {
            painter.rect_filled(r, CornerRadius::same(2), colour.gamma_multiply(0.08));
        }
        painter.rect_stroke(r, CornerRadius::same(2), Stroke::new(if selected { 2.5 } else { 1.5 }, colour), egui::StrokeKind::Outside);
        if selected {
            painter.rect_stroke(r.expand(4.0), CornerRadius::same(4), Stroke::new(2.0, tokens.accent), egui::StrokeKind::Outside);
        }
        // The diagram's number in reading order, in a tab outside the box.
        let (text, fill) = state.text_on_soft(&tokens);
        let number = painter.layout_no_wrap((i + 1).to_string(), theme::semibold(11.0), text);
        let size = egui::vec2(number.size().x + MARK_ICON + 10.0, number.size().y.max(MARK_ICON) + 2.0);
        let tab = mark_tab(r, size, xf.rect, if selected { 4.0 } else { 0.0 });
        painter.rect(tab, CornerRadius::same(2), fill, Stroke::new(1.0, colour), egui::StrokeKind::Inside);
        let icon = egui::Rect::from_center_size(egui::pos2(tab.min.x + 3.0 + MARK_ICON / 2.0, tab.center().y), egui::Vec2::splat(MARK_ICON));
        crate::icons::paint_with(painter, icon, state.icon(), MARK_ICON, text);
        painter.galley(egui::pos2(icon.max.x + 3.0, tab.center().y - number.size().y / 2.0), number, text);
        let draft = drafts.contains(&DiagramAt::new(page, i));
        if draft {
            crate::chess_widgets::draft_dot(painter, tab.expand(3.0), &tokens);
        }
        if under {
            let title = crate::i18n::fmt(tl!("Diagram {n}"), &[("n", &(i + 1).to_string())]);
            let kept = results.kept_fen(page, i);
            let fen = kept.or_else(|| reading.and_then(BoardReading::position).map(|pos| pos.fen.as_str()));
            let label = if draft { format!("{label} · {}", tl!("Unconfirmed changes")) } else { label };
            let text = match fen {
                Some(fen) => format!(
                    "{title} · {label}
{fen}"
                ),
                None => format!("{title} · {label}"),
            };
            hovered = Some((i, text));
        }
    }
    hovered
}

/// The side of a mark's state icon, in points.
const MARK_ICON: f32 = 10.0;

/// Where a diagram's number tab of `size` goes for its box `r` on a page shown in `page`: on top
/// of the box at its left, outside it (and outside the selection ring, `ring` points wide); below
/// the box when there is no room above it on the page.
pub(crate) fn mark_tab(r: egui::Rect, size: egui::Vec2, page: egui::Rect, ring: f32) -> egui::Rect {
    let above = egui::Rect::from_min_size(egui::pos2(r.min.x, r.min.y - ring - 1.0 - size.y), size);
    if above.min.y >= page.min.y { above } else { egui::Rect::from_min_size(egui::pos2(r.min.x, r.max.y + ring + 1.0), size) }
}

/// The Chess diagrams panel: its tabs, and the tab chosen. While the panel is in its own window
/// (`in_window`, T13b) the Diagrams and Text tabs are drawn there and only said to be here, so
/// one frame never draws them twice. The study board is the Board panel (T5c). `editor_here`:
/// the diagram editor is drawn in the Diagrams tab ([`PdfCraftApp::chess_editor_in_panel`]).
#[allow(clippy::too_many_arguments)]
pub(crate) fn panel(
    ui: &mut egui::Ui,
    t: &Tokens,
    state: &mut ChessState,
    id: DocId,
    info: &DocInfo,
    current: Option<ScanStamp>,
    origin: pdfcraft_engine::chess::document::BookOrigin,
    page_now: usize,
    in_window: bool,
    editor_here: bool,
    pgn: &crate::chess_reference_ui::BookPgnView,
) -> Option<PanelAction> {
    use crate::chess_text_ui::ChessTab;
    // A click anywhere in the panel, whichever tab shows, puts Undo and Redo on the game (D12),
    // and so does the keyboard focus in it (plan A9-I18).
    if crate::chess_history::pressed_in(ui) {
        state.enter_scope();
    }
    state.mark_surface(ui);
    // In the docking workspace's own tab style (plan A8, P6).
    crate::widgets::dock_tabs(ui, &mut state.tab, &[(ChessTab::Diagrams, tl!("Diagrams")), (ChessTab::Text, tl!("Text"))]);
    ui.add_space(4.0);
    if in_window {
        return in_window_note(ui, t);
    }
    let offer = if state.tools_offer { tools_offer(ui, t) } else { None };
    let pgn_offer = pgn.offer.as_deref().and_then(|name| pgn_offer(ui, t, name));
    tab_contents(ui, t, state, state.tab == ChessTab::Text, id, info, current, origin, page_now, editor_here, pgn).or(offer).or(pgn_offer)
}

/// The offer of the PGN beside the PDF (plan "Carregar PGN do livro", R6f): Use it, or Not now,
/// which is remembered for the document. The buttons are short words and the file's name is in
/// the text, which wraps, so a long name never widens the panel (plan A9-I7a, finding A6-02).
fn pgn_offer(ui: &mut egui::Ui, t: &Tokens, name: &str) -> Option<PanelAction> {
    let mut action = None;
    egui::Frame::NONE.fill(t.card).stroke(Stroke::new(1.0, t.border)).corner_radius(CornerRadius::same(6)).inner_margin(egui::Margin::same(8)).show(
        ui,
        |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.add(crate::icons::image("book-open", 14.0, t.icon));
                let said = crate::i18n::fmt(tl!("{name} sits beside this PDF. Use it as the book's PGN?"), &[("name", name)]);
                ui.add(egui::Label::new(egui::RichText::new(said).small()).wrap());
            });
            ui.horizontal_wrapped(|ui| {
                // Each named in full for screen readers, its visible word first (WCAG 2.5.3): the
                // tools panel's offer has a Not now too.
                let named = |r: &egui::Response, text: &str| r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, text));
                let use_it = ui.button(tl!("Use"));
                named(&use_it, &crate::i18n::fmt(tl!("Use {name} as the book's PGN"), &[("name", name)]));
                if use_it.clicked() {
                    action = Some(PanelAction::PgnOffer { use_it: true });
                }
                let not_now = ui.button(tl!("Not now"));
                named(&not_now, &crate::i18n::fmt(tl!("Not now: don't use {name} as the book's PGN"), &[("name", name)]));
                if not_now.clicked() {
                    action = Some(PanelAction::PgnOffer { use_it: false });
                }
            });
        },
    );
    ui.add_space(6.0);
    action
}

/// Windows narrower than this, in points, get the offer to collapse the tools panel.
pub(crate) const TOOLS_OFFER_BELOW: f32 = 1600.0;

/// The offer to collapse the tools panel, for room beside the page (plan A8, P5, finding X4):
/// Collapse, or Not now; either answer is kept, so it is made once.
fn tools_offer(ui: &mut egui::Ui, t: &Tokens) -> Option<PanelAction> {
    let mut action = None;
    egui::Frame::NONE.fill(t.card).stroke(Stroke::new(1.0, t.border)).corner_radius(CornerRadius::same(6)).inner_margin(egui::Margin::same(8)).show(
        ui,
        |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.add(crate::icons::image("panel-left", 14.0, t.icon));
                ui.label(egui::RichText::new(tl!("More room for the page: collapse the tools panel?")).small());
            });
            ui.horizontal(|ui| {
                if ui.button(tl!("Collapse")).clicked() {
                    action = Some(PanelAction::ToolsOffer { collapse: true });
                }
                if ui.button(tl!("Not now")).clicked() {
                    action = Some(PanelAction::ToolsOffer { collapse: false });
                }
            });
        },
    );
    ui.add_space(6.0);
    action
}

/// Where the Diagrams and Text tabs were: they are in the chess window, which can be docked.
fn in_window_note(ui: &mut egui::Ui, t: &Tokens) -> Option<PanelAction> {
    ui.add_space(6.0);
    ui.label(egui::RichText::new(tl!("The chess panel is in its own window.")).color(t.text_muted));
    ui.add_space(4.0);
    ui.button(tl!("Back to the main window")).clicked().then_some(PanelAction::DockWindow)
}

/// The Text tab (`text`) or the Diagrams tab: in the Inspector's Chess panel, or in the chess
/// window (T13b). `editor_here`: the selected diagram's editor is drawn above the Diagrams list
/// ([`PdfCraftApp::chess_editor_in_panel`]).
#[allow(clippy::too_many_arguments)]
pub(crate) fn tab_contents(
    ui: &mut egui::Ui,
    t: &Tokens,
    state: &mut ChessState,
    text: bool,
    id: DocId,
    info: &DocInfo,
    current: Option<ScanStamp>,
    origin: pdfcraft_engine::chess::document::BookOrigin,
    page_now: usize,
    editor_here: bool,
    pgn: &crate::chess_reference_ui::BookPgnView,
) -> Option<PanelAction> {
    // The document's long job (Export, Align the book), with Stop (plan A9-I2).
    let long = state.long.as_ref().filter(|j| j.doc == id).map(crate::chess_text_ui::ChessJob::long_row);
    if text {
        let busy = state.reading.as_ref().filter(|j| j.doc == id).map(crate::chess_text_ui::ChessJob::label);
        let doc = state.doc_mut(id);
        let (positions, source) = (doc.pgn_positions, doc.pgn_source);
        return crate::chess_text_ui::text_tab(ui, t, doc.text.as_mut(), id, page_now, current, origin, busy, long.as_ref(), positions, pgn, source)
            .map(PanelAction::Text);
    }
    let mut action = None;
    let running = state.run.as_ref().filter(|r| r.doc == id).map(|r| {
        let s = lock(&r.progress);
        (s.done, s.total)
    });
    // The header: Detect diagrams ▾, the export, board and models buttons, the progress (P2a2).
    let kept = pgn.summary.is_some();
    let doc = state.doc_mut(id);
    let (positions, source) = (doc.pgn_positions, kept.then_some(doc.pgn_source));
    if let Some(a) = crate::chess_panel_head::header(ui, t, state.run.is_some(), info.pages.len(), running, positions, source) {
        action = Some(a);
    }
    if let Some(row) = &long
        && crate::chess_panel_head::long_job(ui, t, row)
    {
        action = Some(PanelAction::Text(crate::chess_text_ui::TextAction::Stop));
    }
    // A scan of another document holds the header's Detect: it says so (plan A9-I1a).
    let elsewhere = state.run.as_ref().filter(|r| r.doc != id).map(|r| {
        let s = lock(&r.progress);
        (s.done, s.total)
    });
    if let Some((done, total)) = elsewhere {
        crate::chess_panel_head::elsewhere(ui, t, done, total);
    }
    ui.add_space(6.0);
    // The diagrams with unconfirmed changes, marked in the list (P3b3).
    let drafts = state.draft_keys(id);
    let Some(DocChess { results: Some(results), list, .. }) = state.docs.get_mut(&id) else {
        if running.is_none() {
            crate::chess_panel_head::empty(ui, t);
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
    // What the scan found, and Review now (P2a2).
    let queue = results.review_queue();
    if running.is_none() && results.count() > 0 && Some(results.stamp) == current {
        ui.add_space(2.0);
        if let Some(a) = crate::chess_panel_head::summary(ui, t, results.count(), queue.len()) {
            action = Some(a);
        }
    }
    // The selected diagram's editor (S17), above the list.
    if let Some(ed) = state.editor.as_mut().filter(|e| editor_here && e.doc == id && Some(results.stamp) == current) {
        ui.separator();
        let pieces = crate::chess_editor::piece_set(&mut state.pieces, &state.diagram_piece_choice, &mut state.diagram_piece_error);
        let done = results.kind(ed.page, ed.index).is_some() && !ed.dirty();
        let confirm = crate::chess_diagram_tab::Confirm::of(done, !queue.is_empty(), state.confirm_only);
        let page = pdfcraft_engine::chess::export::page_label_of(info, ed.page).to_string();
        if let Some(a) = crate::chess_editor::editor_ui(ui, t, ed, &pieces, &page, queue.len(), confirm, state.diagram_keys.board.as_ref()) {
            action = Some(PanelAction::Editor(a));
        }
    }
    ui.separator();
    let reveal = std::mem::take(&mut results.reveal);
    // Esc in the Diagram tab: the selected row takes the keyboard (plan A8, P3c).
    let focus_row = std::mem::take(&mut state.diagram_keys.focus_list);
    if let Some(a) = crate::chess_list::list(ui, t, list, results, info, page_now, &drafts, reveal, focus_row) {
        action = Some(a);
    }
    action
}

impl PdfCraftApp {
    /// Chess ▸ Scan diagrams: scan the current page (`all`: every page) of the active document in
    /// the background and show the Chess diagrams panel.
    pub fn start_chess_scan(&mut self, all: bool) {
        let Some((vi, id)) = self.active_ids() else { return };
        // Detecting again over diagrams with unconfirmed changes asks about them first (P3b3).
        if self.chess.results_of(id).is_some() && self.ask_chess_drafts(Some(id), crate::chess_drafts::AfterDrafts::Scan(all)) {
            return;
        }
        let Some(count) = self.session.get(id).map(|d| d.info.pages.len()) else { return };
        let pages: Vec<usize> =
            if all { (0..count).collect() } else { self.views.get(vi).map(|v| v.current).filter(|p| *p < count).into_iter().collect() };
        self.start_chess_scan_pages(id, pages, false, all);
    }

    /// Scan `pages` of document `id` (`all`: they are every page). `quiet`: for a double-click
    /// on a page (`BoardAt`) or the pages an edit changed, which neither shows the Chess panel
    /// nor announces what was found.
    fn start_chess_scan_pages(&mut self, id: DocId, pages: Vec<usize>, quiet: bool, all: bool) {
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
        let pages: Vec<usize> = jobs.iter().flat_map(ChessScanJob::pages).collect();
        // Results for this document from before it changed give way; another document's stay
        // (plan A9-I1a, finding A6-01).
        let now = self.session.chess_stamp(id);
        if let Some(doc) = self.chess.docs.get_mut(&id)
            && doc.results.as_ref().is_some_and(|r| Some(r.stamp) != now)
        {
            doc.results = None;
        }
        let progress = Arc::new(Mutex::new(ChessProgress { total: pages.len(), ..Default::default() }));
        let p = progress.clone();
        let work = if self.chess_frame_budget().is_some() {
            let models = pdfcraft_engine::chess::models(settings.models_dir.as_deref());
            lock(&progress).model = Some(ModelNote::of(&models));
            ScanWork::Steps(Box::new(StepScan { models, jobs: jobs.into(), current: None, base: 0, cancelled: false }))
        } else {
            ScanWork::Thread(Self::scan_on_a_worker(p, jobs, settings, self.run_inline))
        };
        self.chess.run = Some(ChessRun { doc: id, quiet, progress, work, found: 0, stale: false, pages, all });
        if !quiet {
            self.reveal_chess_diagrams();
        }
        self.poll_chess();
    }

    /// The scan of `jobs` on a worker thread (inline when `inline`), into `p`.
    fn scan_on_a_worker(
        p: Arc<Mutex<ChessProgress>>,
        jobs: Vec<ChessScanJob>,
        settings: pdfcraft_engine::chess::ChessScanSettings,
        inline: bool,
    ) -> crate::chess_job::Job<()> {
        crate::chess_job::Job::spawn("pdfcraft-chess", inline, move |ctl| {
            let models = pdfcraft_engine::chess::models(settings.models_dir.as_deref());
            lock(&p).model = Some(ModelNote::of(&models));
            let mut base: usize = 0;
            for job in jobs {
                let size = job.pages().len();
                let scan = job.run(&models, |done, _| {
                    lock(&p).done = base.saturating_add(done);
                    !ctl.cancelled()
                });
                base = base.saturating_add(size);
                lock(&p).scans.push(scan);
                if ctl.cancelled() {
                    break;
                }
            }
            Ok(())
        })
    }

    /// The time a frame gives a scan run a page at a time, when scans run so: always on the web,
    /// and in tests that ask ([`ChessState::frame_budget_ms`]).
    pub(crate) fn chess_frame_budget(&self) -> Option<f64> {
        if cfg!(target_arch = "wasm32") { Some(self.chess.frame_budget_ms.unwrap_or(WEB_FRAME_BUDGET_MS)) } else { self.chess.frame_budget_ms }
    }

    /// Stop a running scan (the pages scanned so far are kept).
    pub fn cancel_chess_scan(&mut self) {
        if let Some(r) = &mut self.chess.run {
            r.work.cancel();
        }
    }

    /// Show the batches the worker finished; report the scan once it is done.
    pub(crate) fn poll_chess(&mut self) {
        self.poll_chess_scan();
        self.chess_board_at_tick();
        self.chess_pick_after_scan_tick();
        self.chess_board_text_tick();
    }

    fn poll_chess_scan(&mut self) {
        self.chess_games_tick();
        self.poll_chess_job();
        self.poll_chess_capture();
        self.chess_source_tick();
        // A closed document takes its view with it.
        let session = &self.session;
        self.chess.docs.retain(|id, _| session.get(*id).is_some());
        if let Some(DiagramAt { page, index }) = self.chess.picked.take() {
            if std::mem::take(&mut self.chess.to_board) {
                self.select_chess_diagram(page, index, false);
                self.open_chess_board(page, index);
            } else {
                self.edit_chess_diagram(page, index, false);
            }
        }
        let budget = self.chess_frame_budget().unwrap_or(WEB_FRAME_BUDGET_MS);
        let Some(run) = self.chess.run.as_mut() else {
            // An edit, undo or redo since the scan: the list follows it.
            self.carry_chess_results();
            return;
        };
        // Whether it is done first: once it is, every batch is in. A scan run a page at a time
        // runs its pages for this frame here.
        let finished = match &mut run.work {
            ScanWork::Thread(job) => job.poll(),
            ScanWork::Steps(steps) => steps.advance(&run.progress, budget).then_some(Ok(())),
        };
        let stepping = matches!(run.work, ScanWork::Steps(_));
        let (scans, model) = {
            let mut s = lock(&run.progress);
            (std::mem::take(&mut s.scans), s.model.clone())
        };
        for scan in scans {
            let found: usize = scan.pages.iter().map(|p| p.diagrams.len()).sum();
            let kept = self.keep_chess_scan(scan, model.clone());
            let Some(run) = self.chess.run.as_mut() else { return };
            if kept {
                run.found = run.found.saturating_add(found);
            } else {
                run.stale = true;
            }
        }
        let Some(finished) = finished else {
            if let Some(ctx) = &self.ctx {
                // A scan run a page at a time goes on in the next frame, at once.
                ctx.request_repaint_after(std::time::Duration::from_millis(if stepping { 0 } else { 200 }));
            }
            return;
        };
        let Some(run) = self.chess.run.take() else { return };
        // The scan's work panicked: said, the pages it scanned kept.
        if let Err(why) = finished {
            self.notify(why);
        }
        // The pages it was for are the list's even when the document changed meanwhile: the list
        // is carried to the state now, and they with it.
        match self.chess.results_of_mut(run.doc) {
            Some(results) => results.ask(&run.pages, run.all),
            None if run.stale => self.notify(tl!("The document changed since the scan. Scan again to update the diagrams.").to_string()),
            None => {}
        }
        if !run.stale && !run.quiet {
            let n = run.found;
            self.notify(crate::i18n::fmt(tl!("Diagrams found: {n}"), &[("n", &n.to_string())]));
        }
        self.carry_chess_results();
    }

    /// The document changed since its diagrams were scanned (an edit, undo or redo): carry the
    /// list to the state it is in now (plan A8, P3b4). The pages whose content did not change keep
    /// what was found there, wherever they are now, and only the others are scanned again, in the
    /// background and without a word. The diagram picked, the one in the editor and the drafts
    /// follow their pages; those of a page removed go. Waits for a scan that is running.
    pub(crate) fn carry_chess_results(&mut self) {
        // Each document's list, as its document changed (plan A9-I1a): one carry at a time, as
        // the pages it leaves to scan wait for the scan it starts.
        let changed: Vec<DocId> = self
            .chess
            .docs
            .iter()
            .filter(|(id, d)| d.results.as_ref().is_some_and(|r| self.session.chess_stamp(**id).is_some_and(|now| now != r.stamp)))
            .map(|(id, _)| *id)
            .collect();
        for id in changed {
            if self.chess.run.is_some() {
                return;
            }
            self.carry_chess_results_of(id);
        }
    }

    /// [`Self::carry_chess_results`] for document `id`'s list.
    fn carry_chess_results_of(&mut self, id: DocId) {
        let Some((then, wanted, all)) = self.chess.results_of(id).map(|r| (r.stamp, r.wanted.clone(), r.all)) else { return };
        if self.session.chess_stamp(id).is_none_or(|now| now == then) {
            return;
        }
        let settings = self.chess_scan_settings();
        let carry = match self.session.chess_carry(id, then, &wanted, all, &settings, true) {
            Ok(carry) => carry,
            // Closed, or its content may no longer be copied: nothing of it is shown.
            Err(_) => {
                self.chess.doc_mut(id).results = None;
                return;
            }
        };
        if let Some(results) = self.chess.results_of_mut(id) {
            results.stamp = carry.stamp;
            results.wanted = carry.wanted.clone();
            results.pages = carry.pages.clone();
            results.selected = results.selected.and_then(|at| carry.now_at(at.page).map(|p| DiagramAt::new(p, at.index)));
        }
        let editing = self.chess.editor.as_ref().filter(|e| e.doc == id).map(|e| carry.now_at(e.page));
        match editing {
            Some(Some(page)) => {
                if let Some(ed) = self.chess.editor.as_mut() {
                    ed.page = page;
                }
            }
            Some(None) => self.chess.editor = None,
            None => {}
        }
        self.chess.drafts.retain_mut(|d| {
            if d.target.doc != id {
                return true;
            }
            match carry.now_at(d.target.page) {
                Some(page) => {
                    d.target.page = page;
                    true
                }
                None => false,
            }
        });
        self.refresh_chess_kinds(id);
        if !carry.missing.is_empty() {
            self.start_chess_scan_pages(id, carry.missing, true, false);
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
        self.start_chess_scan_pages(at.doc, vec![at.page], true, false);
    }

    /// Open diagram `at` of document `doc` in the editor (plan A9-I4): its document comes to the
    /// front, and when the list doesn't hold the diagram's page yet, that page is scanned first
    /// and the diagram opens once the scan is in ([`PickAfterScan`]).
    pub(crate) fn pick_after_scan(&mut self, doc: DocId, at: DiagramAt) {
        if let Some(i) = self.views.iter().position(|v| v.id == doc) {
            self.active = Some(i);
        }
        self.chess.shown = Some(doc);
        self.chess.pick_after_scan = Some(PickAfterScan { doc, at, started: false });
        self.chess_pick_after_scan_tick();
    }

    /// Every poll: the diagram waiting for its page's scan ([`PickAfterScan`]) opens once the list
    /// holds its page, the page scanned for it when no scan is running. Dropped when its document
    /// is no longer the one shown, or when the scan ended without the page (stopped, or the
    /// document changed meanwhile).
    pub(crate) fn chess_pick_after_scan_tick(&mut self) {
        let Some(mut want) = self.chess.pick_after_scan else { return };
        if self.active_ids().map(|(_, id)| id) != Some(want.doc) {
            self.chess.pick_after_scan = None;
            return;
        }
        let stamp = self.session.chess_stamp(want.doc);
        let scanned = shown(&self.chess, want.doc, stamp).filter(|r| r.pages.iter().any(|p| p.page == want.at.page));
        if let Some(results) = scanned {
            let there = results.diagram(want.at.page, want.at.index).is_some();
            self.chess.pick_after_scan = None;
            if there {
                self.edit_chess_diagram(want.at.page, want.at.index, true);
                self.show_chess_tab(crate::chess_text_ui::ChessTab::Diagrams);
            } else {
                let (n, p) = (want.at.index.saturating_add(1).to_string(), self.page_label_in(want.doc, want.at.page));
                self.notify(crate::i18n::fmt(tl!("Diagram {n} wasn't found on page {p}."), &[("n", &n), ("p", &p)]));
            }
            return;
        }
        if self.chess.run.is_some() {
            return;
        }
        if want.started {
            self.chess.pick_after_scan = None;
            return;
        }
        want.started = true;
        self.chess.pick_after_scan = Some(want);
        // A scan that does not start (copying not allowed, said) leaves no run: the next tick
        // drops the request.
        self.start_chess_scan_pages(want.doc, vec![want.at.page], false, false);
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
        let doc = self.chess.doc_mut(scan.doc);
        if doc.results.as_ref().is_none_or(|r| r.stamp != stamp) {
            doc.results = Some(ChessResults {
                doc: scan.doc,
                stamp,
                pages: Vec::new(),
                selected: None,
                reveal: false,
                model: ModelNote::Missing,
                kinds: HashMap::new(),
                confirmed: Default::default(),
                wanted: Vec::new(),
                all: false,
            });
        }
        let asked = self.chess.run.as_ref().filter(|r| r.doc == scan.doc).map(|r| (r.pages.clone(), r.all));
        let Some(results) = self.chess.results_of_mut(scan.doc) else { return true };
        if let Some((pages, all)) = asked {
            results.ask(&pages, all);
        }
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
        if results.selected.is_some_and(|at| results.diagram(at.page, at.index).is_none()) {
            results.selected = None;
        }
        // The diagrams the user confirmed, and the corrections given to the session (P3a3).
        self.refresh_chess_kinds(scan.doc);
        true
    }

    /// Pick diagram `index` on `page`. From the panel (`go`), the view goes to it and outlines it;
    /// from the page, the panel shows its entry.
    pub fn select_chess_diagram(&mut self, page: usize, index: usize, go: bool) {
        let Some((vi, id)) = self.active_ids() else { return };
        let Some(results) = self.chess.results_of_mut(id) else { return };
        let Some((rect, _)) = results.diagram(page, index) else { return };
        results.selected = Some(DiagramAt::new(page, index));
        if self.chess.editor.as_ref().is_none_or(|e| (e.doc, e.page, e.index) != (id, page, index)) {
            // The diagram left keeps its unconfirmed changes as a draft (P3b3).
            self.park_chess_editor();
            let filter = self.chess.filter(id);
            let Some(results) = self.chess.results_of(id) else { return };
            let reading = results.pages.iter().find(|p| p.page == page).and_then(|p| ChessResults::reading(p, index));
            let mut ed = crate::chess_editor::DiagramEditor::new(id, page, index, reading);
            // A diagram the user confirmed opens on the position kept for it.
            if let Some(fen) = results.kept_fen(page, index) {
                ed.show_kept(fen);
            }
            // Said to screen readers: which diagram, its place in what the tab walks, the doubts.
            let walk = results.walk(filter);
            let place = walk.iter().position(|q| *q == DiagramAt::new(page, index));
            let of = results.pages.iter().find(|p| p.page == page).map_or(0, |p| p.diagrams.len());
            let label = self.chess_page_label(page);
            let said = crate::chess_diagram_keys::announcement(
                &diagram_title(&label, index, of),
                &walk_place(filter, place, walk.len()),
                ed.uncertain.len(),
            );
            self.chess.diagram_keys.announce = said;
            self.chess.review_done = None;
            self.chess.editor = Some(ed);
            self.restore_chess_draft();
        }
        let Some(results) = self.chess.results_of_mut(id) else { return };
        results.reveal = !go;
        if go && let Some(view) = self.views.get_mut(vi) {
            view.go_to_page(page);
            view.flash = Some((page, rect, 0.0));
        }
        if !go {
            self.reveal_chess_diagrams();
        }
    }

    /// Pick diagram `index` on `page` to check it: [`Self::select_chess_diagram`], and the
    /// Diagram tab comes forward with it (plan A8, P3b2) unless the editor is drawn in the
    /// Diagrams panel ([`Self::chess_editor_in_panel`]). Picked on the page (`go` false), the tab
    /// comes forward only when it is open already: one opening there would move the page under
    /// the pointer, and a second click of a double-click would miss the diagram.
    pub fn edit_chess_diagram(&mut self, page: usize, index: usize, go: bool) {
        self.select_chess_diagram(page, index, go);
        let shown = self.active_ids().map(|(_, id)| id);
        let picked = shown.and_then(|id| self.chess.results_of(id)).and_then(|r| r.selected) == Some(DiagramAt::new(page, index));
        let open = go || self.docking.layout.contains(&crate::docking::Panel::Diagram);
        if picked && open && !self.chess_editor_in_panel() {
            self.pending_panel_reveals.push(crate::docking::Panel::Diagram);
        }
    }

    /// Whether the selected diagram's editor is drawn above the Diagrams list instead of in the
    /// Diagram tab: the classic editor was chosen, or Read mode draws no docking workspace and so
    /// no Diagram tab (plan A9-I15, finding R4-01). A reveal asked for there would be done only
    /// when Read mode is left, bringing out a tab the user never saw open.
    pub(crate) fn chess_editor_in_panel(&self) -> bool {
        self.chess.classic_editor || self.mode == crate::Mode::Read
    }

    /// Opening a diagram on the board is a place to come back to (plan A8, P4): the page on
    /// screen goes on the view's history, so Previous view (Alt+←) returns to it after the page
    /// has been scrolled away.
    pub(crate) fn mark_chess_history(&mut self) {
        if let Some(view) = self.active_ids().and_then(|(vi, _)| self.views.get_mut(vi)) {
            view.mark_history();
        }
    }

    /// Review now: open the first diagram to review, the list's chip back on All, so the tab walks
    /// the diagrams to review (plan A9-I4, finding A5-17).
    pub(crate) fn review_chess_now(&mut self) {
        let Some((_, id)) = self.active_ids() else { return };
        self.chess.doc_mut(id).list.filter = None;
        if let Some(DiagramAt { page, index }) = self.chess.results_of(id).and_then(|r| r.review_queue().first().copied()) {
            self.edit_chess_diagram(page, index, true);
        }
    }

    pub(crate) fn chess_action(&mut self, action: PanelAction) {
        match action {
            PanelAction::ScanPage => self.start_chess_scan(false),
            PanelAction::ScanAll => self.start_chess_scan(true),
            PanelAction::Cancel => self.cancel_chess_scan(),
            PanelAction::Select(DiagramAt { page, index }) => self.edit_chess_diagram(page, index, true),
            // The Board panel is the study board (T5c): it comes forward, docked or in its window.
            PanelAction::OpenBoard(DiagramAt { page, index }) => {
                self.select_chess_diagram(page, index, false);
                self.mark_chess_history();
                self.open_chess_board(page, index);
            }
            PanelAction::Analyze(DiagramAt { page, index }) => {
                self.select_chess_diagram(page, index, false);
                self.mark_chess_history();
                if self.load_chess_board(page, index) {
                    self.chess.study.analyze();
                    self.show_chess_board();
                }
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
            PanelAction::VerifyPage(page) => {
                self.verify_chess_page(page);
            }
            PanelAction::ReviewNow => self.review_chess_now(),
            // Either answer is kept: the offer is made once (plan A8, P5).
            PanelAction::PgnOffer { use_it } => self.answer_pgn_offer(use_it),
            PanelAction::ToolsOffer { collapse } => {
                self.chess.tools_offer_answered = true;
                if collapse {
                    self.left_open = false;
                }
            }
            PanelAction::GoToPage(page) => {
                if let Some(view) = self.active_ids().and_then(|(vi, _)| self.views.get_mut(vi)) {
                    view.go_to_page(page);
                }
            }
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
    pub(crate) fn chess_editor_action(&mut self, action: crate::chess_editor::EditorAction) {
        use crate::chess_editor::EditorAction;
        match action {
            EditorAction::CopyFen(fen) => self.chess_action(PanelAction::CopyFen(fen)),
            EditorAction::OpenLichess(url) => self.chess_action(PanelAction::OpenLichess(url)),
            EditorAction::Close => {
                self.park_chess_editor();
                self.chess.editor = None;
            }
            EditorAction::Previous | EditorAction::Next => {
                // The editor's document, else the one shown.
                let Some(id) = self.chess.editor.as_ref().map(|e| e.doc).or_else(|| self.active_ids().map(|(_, id)| id)) else { return };
                let Some(results) = self.chess.results_of(id) else { return };
                // What the list's chip shows, or the ones to review (plan A8, P4).
                let walk = results.walk(self.chess.filter(id));
                let at = self.chess.editor.as_ref().map(|e| DiagramAt::new(e.page, e.index)).or(results.selected);
                let forward = action == EditorAction::Next;
                let ahead = match (forward, at) {
                    (true, Some(at)) => walk.iter().find(|q| **q > at),
                    (false, Some(at)) => walk.iter().rev().find(|q| **q < at),
                    (_, None) => None,
                };
                // Past either end, round to the other, and say so: the end is not silent.
                let wrapped = ahead.is_none() && at.is_some();
                let next = ahead.or(if forward { walk.first() } else { walk.last() }).copied();
                if let Some(DiagramAt { page, index }) = next {
                    self.select_chess_diagram(page, index, true);
                    if wrapped && next != at {
                        self.notify(if forward { tl!("Back to the first diagram") } else { tl!("Back to the last diagram") }.to_string());
                    }
                }
            }
            EditorAction::OpenBoard | EditorAction::Analyze => {
                self.mark_chess_history();
                if let Some(ed) = self.chess.editor.as_ref() {
                    let source = crate::chess_board_ui::Source { doc: ed.doc, page: ed.page, index: ed.index };
                    let (fen, flipped) = (ed.fen(), ed.flipped);
                    // A new game in the document's games: the game on the board stays as it is.
                    if self.open_chess_game(&fen, flipped, Some(source)) {
                        // The position as edited is the one analysed (plan A8, P7).
                        if action == EditorAction::Analyze {
                            self.chess.study.analyze();
                        }
                        self.show_chess_board();
                    }
                }
            }
            EditorAction::Redraw => self.redraw_chess_diagram(),
            EditorAction::Link => self.link_chess_diagram(),
            EditorAction::Confirm { next } => {
                self.chess.confirm_only = !next;
                self.confirm_chess_diagram(next);
                // The keyboard is back on the board, of this diagram or the next one.
                if let Some(ed) = self.chess.editor.as_mut() {
                    ed.focus = true;
                }
            }
            EditorAction::Forget => {
                self.forget_chess_correction();
            }
            EditorAction::ShowOnPage => {
                if let Some((page, index)) = self.chess.editor.as_ref().map(|e| (e.page, e.index)) {
                    self.select_chess_diagram(page, index, true);
                }
            }
        }
    }

    /// Use `choice` for `role`'s pieces from now on: the set is loaded at once (so a set that
    /// cannot be used says why in Preferences), and every board, the page text's diagrams
    /// included, is drawn again with it.
    pub(crate) fn set_piece_choice(&mut self, role: Role, choice: PieceChoice) {
        if *self.chess.piece_choice(role) == choice {
            return;
        }
        match role {
            Role::Board => self.chess.board_piece_choice = choice,
            Role::Diagrams => self.chess.diagram_piece_choice = choice,
        }
        self.reload_pieces(role);
    }

    /// Load `role`'s pieces again, from its choice or from what Auto finds now (a font just
    /// imported, say), and draw every board with them.
    pub(crate) fn reload_pieces(&mut self, role: Role) {
        match role {
            Role::Board => self.chess.board_pieces = None,
            Role::Diagrams => self.chess.pieces = None,
        }
        self.chess.role_pieces(role);
        crate::chess_editor::pieces_changed();
        if role == Role::Diagrams
            && let (Some(ctx), Some(pieces)) = (self.ctx.as_ref(), self.chess.pieces.clone())
        {
            for text in self.chess.docs.values_mut().filter_map(|d| d.text.as_mut()) {
                text.redraw_boards(ctx, &pieces);
            }
        }
    }

    /// Put a lichess link to the editor's position under its diagram.
    pub fn link_chess_diagram(&mut self) {
        let Some(ed) = self.chess.editor.as_ref() else { return };
        let (id, page, index, fen) = (ed.doc, ed.page, ed.index, ed.fen());
        let mut settings = pdfcraft_engine::chess::LinkSettings::default();
        settings.only = Some(pdfcraft_engine::chess::LinkTarget { page, index, fen });
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
        // The position the diagram shows: its correction, else its reading (plan A9-I4).
        let read = || {
            let results = self.chess.results_of(id)?;
            let pos = results.effective(results.pages.iter().find(|p| p.page == page)?, index)?;
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
        let text_start = if changed { None } else { self.chess_text_start(id, page, index, &fen) };
        let read = self.chess_text_read(id, page);
        let fen = text_start.unwrap_or(fen);
        if !self.open_chess_game(&fen, flipped, Some(source)) {
            return false;
        }
        // The page's moves were not read yet: they are (`chess_board_text_tick`), and may still
        // give the game its side to move and number.
        let board = self.chess.board();
        self.chess.board_text = (!changed && !read && board.illegal.is_none() && board.source == Some(source)).then(|| BoardText {
            doc: id,
            page,
            index,
            fen: board.game.start_fen(),
            started: false,
        });
        true
    }

    /// Whether the Text tab's reading of `page` of document `id` is of the document as it is.
    fn chess_text_read(&self, id: DocId, page: usize) -> bool {
        let stamp = self.session.chess_stamp(id);
        self.chess.text_of(id).is_some_and(|t| t.page == page && Some(t.stamp) == stamp)
    }

    /// A diagram put on the board before its page was read as text ([`BoardText`]): read the page
    /// (off the interface thread, as the Text tab does, after any reading running), then give the
    /// game the side to move and move number the moves under the diagram give it, when nothing
    /// was done on the game meanwhile (no move played, nothing changed). This is no step of its
    /// own: opening the diagram was the list's ("Load the diagram"), which takes the game, start
    /// and all, back out. Dropped when its document is no longer the one shown, or
    /// when the reading did not come (the document changed meanwhile).
    pub(crate) fn chess_board_text_tick(&mut self) {
        let Some(mut want) = self.chess.board_text.clone() else { return };
        if self.active_ids().map(|(_, id)| id) != Some(want.doc) {
            self.chess.board_text = None;
            return;
        }
        if self.chess_text_read(want.doc, want.page) {
            self.chess.board_text = None;
            self.apply_chess_text_start(&want);
            return;
        }
        if self.chess.reading.is_some() {
            return;
        }
        if want.started {
            self.chess.board_text = None;
            return;
        }
        want.started = true;
        let page = want.page;
        self.chess.board_text = Some(want);
        self.read_chess_text(page, false);
    }

    /// Give the game shown, when it was started from `want`'s diagram and is untouched, the start
    /// the Text tab's reading of its page gives it ([`Self::chess_text_start`]).
    fn apply_chess_text_start(&mut self, want: &BoardText) {
        let Some(start) = self.chess_text_start(want.doc, want.page, want.index, &want.fen) else { return };
        if start == want.fen {
            return;
        }
        let source = Some(crate::chess_board_ui::Source { doc: want.doc, page: want.page, index: want.index });
        self.sync_chess_docs();
        let e = self.chess.games.collection_mut().active_mut();
        let waiting =
            e.untouched() && e.board.source == source && e.board.illegal.is_none() && e.board.game.is_empty() && e.board.game.start_fen() == want.fen;
        if waiting {
            let flipped = e.board.flipped;
            e.board.load_fen(&start, flipped, source);
            // As when it was opened: its own history starts empty.
            e.board.history = Default::default();
        }
    }

    /// The start of the game the Text tab's reading of `page` begins at diagram `index`, when it
    /// has the pieces of `fen`: the moves after the diagram (their first number, a "position
    /// after" caption) settle its side to move and move number better than the reading alone.
    fn chess_text_start(&self, id: DocId, page: usize, index: usize, fen: &str) -> Option<String> {
        use pdfcraft_engine::chess::pdfcraft_chessvision::doc::GameOrigin;
        let stamp = self.session.chess_stamp(id)?;
        let text = self.chess.text_of(id).filter(|t| t.page == page && t.stamp == stamp)?;
        let game = text.chess.games.iter().find(
            |g| matches!(g.origin, GameOrigin::Diagram { page: p, diagram, .. } if usize::try_from(p).ok() == Some(page) && diagram == index),
        )?;
        let start = game.score.start_fen.as_deref()?;
        // The walker settles the side and the number, never the pieces: the same pieces on the
        // same squares (plan A9-T1b).
        same_pieces(start, fen).then(|| start.to_owned())
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

    /// `chess.move_list`: the Move list beside the page, or closed when it shows.
    pub fn toggle_move_list(&mut self) {
        self.ask_move_list(crate::docking::MovesRequest::Show(!self.chess.moves_beside));
    }

    /// Ask the Move list for `request`, done at the start of the next frame.
    pub(crate) fn ask_move_list(&mut self, request: crate::docking::MovesRequest) {
        self.chess.moves_request = Some(request);
        if let Some(ctx) = &self.ctx {
            ctx.request_repaint();
        }
    }

    /// What was asked of the Move list, and where it shows this frame. Called before anything is
    /// drawn: the docking workspace settles `moves_beside` again from what it draws.
    pub(crate) fn settle_move_list(&mut self) {
        if let Err(error) = crate::docking::drain_moves(&mut self.docking, self.chess.moves_request.take()) {
            self.notify_fmt("Panel move was not applied: {error}", &[("error", &error)]);
        }
        let drawn = !(self.full_screen && self.active.is_some()) && (self.mode != crate::Mode::Read || self.active.is_some());
        self.chess.moves_beside = drawn && self.docking.exposed(crate::docking::Panel::Moves);
        self.chess.moves_left = self.docking.moves_on_left();
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
            // Back to the diagram (plan A8, P4): its page, outlined, and the Diagram tab with it.
            BoardAction::ShowSource(s) => {
                if self.active_ids().is_some_and(|(_, id)| id == s.doc) {
                    self.edit_chess_diagram(s.page, s.index, true);
                }
            }
            BoardAction::ChooseEngine => self.choose_chess_engine(),
            BoardAction::Analyze => self.chess.study.toggle_analysis(),
            BoardAction::AddGames { texts, more } => self.add_pgn_field_games(&texts, more),
            BoardAction::ToggleMoveList => self.toggle_move_list(),
            BoardAction::BoardTheme(theme) => self.set_board_theme(theme),
            BoardAction::BoardLook(look) => self.set_board_look(look),
            BoardAction::NewGameAt(fen) => {
                if let Err(why) = self.chess.games.collection_mut().new_at(&fen) {
                    self.chess.games.board_mut().message = Some(why);
                }
            }
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
    // A click anywhere in the panel puts Undo and Redo on the game (D12), and so does the
    // keyboard focus in it (plan A9-I18).
    if crate::chess_history::pressed_in(ui) {
        app.chess.enter_scope();
    }
    app.chess.mark_surface(ui);
    let pieces = app.chess.role_pieces(Role::Board);
    let source_label = board_source_label(app);
    // Docked, the workspace's tab names the panel already (plan A8, P6).
    let named = app.mode == crate::Mode::Read;
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
                    (action, games) = crate::chess_board_ui::panel_body(
                        ui,
                        &t,
                        chess,
                        &pieces,
                        source_label,
                        crate::chess_board_ui::Sizing::DOCKED,
                        viewport,
                        named,
                    );
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

/// The Move list beside the page (in the docking workspace, or in Read mode): the board's game,
/// each move a click away, and the move's glyphs and comment under it.
pub(crate) fn moves_panel(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    // A click in the list puts Undo and Redo on the game, as in the Board panel (D12), and so
    // does the keyboard focus in it (plan A9-I18).
    if crate::chess_history::pressed_in(ui) {
        app.chess.enter_scope();
    }
    app.chess.mark_surface(ui);
    let mut action = None;
    let mut side = None;
    egui::Frame::NONE.fill(t.panel).stroke(Stroke::new(1.0, t.divider)).show(ui, |ui| {
        egui::Frame::NONE.inner_margin(egui::Margin { left: 10, right: 10, top: 10, bottom: 8 }).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Right to left: the right edge's button first, then the text size's, and the
                    // title in the width left (cut short in a narrow list). Compact buttons, so a
                    // title such as "Lista de lances" keeps its room at the list's opening width.
                    ui.spacing_mut().button_padding = egui::vec2(3.0, 3.0);
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for (left, icon, name) in [(false, "panel-right", tl!("Right")), (true, "panel-left", tl!("Left"))] {
                        let r = ui
                            .add(egui::Button::image(crate::icons::image(icon, 14.0, t.icon)).selected(app.chess.moves_left == left))
                            .on_hover_text(name);
                        r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, app.chess.moves_left == left, name));
                        if r.clicked() && app.chess.moves_left != left {
                            side = Some(left);
                        }
                    }
                    ui.separator();
                    if crate::chess_figurines::size_buttons(ui, t.icon, &mut app.chess.list_look.size, false) {
                        // The list below reads the size from the context: this frame already.
                        crate::chess_figurines::set_list_look(ui.ctx(), app.chess.list_look);
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add(egui::Label::new(egui::RichText::new(tl!("Move list")).font(theme::semibold(15.5))).truncate());
                    });
                });
            });
            ui.add_space(4.0);
            let room = ui.available_height();
            action = crate::chess_board_ui::moves_list(ui, &t, &mut app.chess, room);
        });
    });
    if let Some(a) = action {
        app.chess_board_action(a);
    }
    if let Some(left) = side {
        app.ask_move_list(crate::docking::MovesRequest::Side { left });
    }
}

/// The Move list tab's menu: the page's edge it stands against.
pub(crate) fn moves_side_menu(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    ui.label(egui::RichText::new(tl!("Side of the page")).small());
    for (left, name) in [(true, tl!("Left")), (false, tl!("Right"))] {
        if ui.add(egui::Button::new(name).selected(app.chess.moves_left == left)).clicked() {
            app.ask_move_list(crate::docking::MovesRequest::Side { left });
            ui.close();
        }
    }
}

/// The diagram the board came from, when it belongs to the document shown, as the link back to
/// it: "Back to diagram 2 (page 14)" (plan A8, P4).
pub(crate) fn board_source_label(app: &PdfCraftApp) -> Option<String> {
    let active = app.active_ids().map(|(_, id)| id);
    app.chess.board().source.filter(|s| active == Some(s.doc)).map(|s| {
        let label = app
            .session
            .get(s.doc)
            .map_or_else(|| s.page.saturating_add(1).to_string(), |d| pdfcraft_engine::chess::export::page_label_of(&d.info, s.page).to_string());
        let label = if label.is_empty() { (s.page + 1).to_string() } else { label };
        crate::i18n::fmt(tl!("Back to diagram {n} (page {p})"), &[("n", &(s.index + 1).to_string()), ("p", &label)])
    })
}

/// A diagram as the module names it everywhere (plan A8, P4: one numbering, by page): "Page 17 ·
/// diagram 1 of 2". `of` is how many the page has.
/// Whether FENs `a` and `b` have the same pieces on the same squares (plan A9-T1b, finding R3-03):
/// their placement keys, never a slice of the FEN text, so FENs written differently for one
/// placement (`44` for `8`, fields joined by `_`) agree. What can't be read is no position.
pub(crate) fn same_pieces(a: &str, b: &str) -> bool {
    use pdfcraft_engine::chess::pdfcraft_chess::fen::Fen;
    matches!((Fen::parse(a), Fen::parse(b)), (Ok(a), Ok(b)) if a.placement_key() == b.placement_key())
}

pub(crate) fn diagram_title(label: &str, index: usize, of: usize) -> String {
    crate::i18n::fmt(
        tl!("Page {p} · diagram {n} of {m}"),
        &[("p", label), ("n", &index.saturating_add(1).to_string()), ("m", &of.max(index.saturating_add(1)).to_string())],
    )
}

/// Where the Diagram tab is in what it walks (`place` of `len`, when the diagram is in it), named
/// by the list's chip: "12 of 40 to review", "3 of 12 · Corrected"; without a place, how many.
pub(crate) fn walk_place(filter: Option<DiagramState>, place: Option<usize>, len: usize) -> String {
    let n = len.to_string();
    match (filter, place) {
        (None | Some(DiagramState::Review), Some(k)) => crate::i18n::fmt(tl!("{i} of {n} to review"), &[("i", &(k + 1).to_string()), ("n", &n)]),
        (None | Some(DiagramState::Review), None) => crate::i18n::fmt(tl!("{n} to review"), &[("n", &n)]),
        (Some(state), Some(k)) => crate::i18n::fmt(tl!("{i} of {n} · {state}"), &[("i", &(k + 1).to_string()), ("n", &n), ("state", state.name())]),
        (Some(state), None) => crate::i18n::fmt(tl!("{n} · {state}"), &[("n", &n), ("state", state.name())]),
    }
}

#[cfg(test)]
mod mark_tests {
    use egui::{Rect, pos2, vec2};

    /// Plan A9-T1b (R3-03): "the same pieces" is the placement key, never a slice of the FEN text:
    /// FENs written differently for one placement agree, other pieces never do, and what can't be
    /// read is no position.
    #[test]
    fn the_same_pieces_whatever_the_fen_is_written_like() {
        use super::same_pieces;
        assert!(same_pieces("4k3/8/8/8/8/8/8/4K3 w - - 0 1", "4k3/44/8/8/8/8/8/4K3 b - - 0 12"));
        assert!(same_pieces("4k3/8/8/8/8/8/8/4K3 w - - 0 1", "4k3/8/8/8/8/8/8/4K3_w_-_-_0_1"));
        assert!(!same_pieces("4k3/8/8/8/8/8/8/4K3 w", "4k3/8/8/8/8/8/8/3K4 w"));
        assert!(!same_pieces("no fen", "no fen"), "what can't be read is no position");
    }

    /// A mark's number tab sits above its box, clear of the selection ring; on a box at the top
    /// of the page, below it.
    #[test]
    fn the_number_tab_is_outside_the_box() {
        let page = Rect::from_min_size(pos2(0.0, 0.0), vec2(600.0, 800.0));
        let size = vec2(24.0, 14.0);
        let middle = Rect::from_min_size(pos2(100.0, 200.0), vec2(150.0, 150.0));
        let tab = super::mark_tab(middle, size, page, 4.0);
        assert!(tab.max.y < middle.min.y - 4.0 && tab.min.x == middle.min.x, "{tab:?}");
        let top = Rect::from_min_size(pos2(100.0, 5.0), vec2(150.0, 150.0));
        let tab = super::mark_tab(top, size, page, 0.0);
        assert!(tab.min.y > top.max.y && !tab.intersects(top), "below a box at the top: {tab:?}");
    }
}

#[cfg(test)]
mod view_tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    use super::*;

    /// Plan A9-I1a (§2.7): while another document is being scanned, the Diagrams tab of one with
    /// no scan running says which page that scan is on, beside the Detect it holds.
    #[test]
    fn a_scan_of_another_document_is_said_where_it_holds_detect() {
        let (shown, scanned) = (DocId(1), DocId(2));
        let progress = Arc::new(Mutex::new(ChessProgress { done: 305, total: 748, ..Default::default() }));
        let run = ChessRun {
            doc: scanned,
            quiet: false,
            progress,
            work: ScanWork::Thread(crate::chess_job::Job::waiting()),
            found: 0,
            stale: false,
            pages: Vec::new(),
            all: true,
        };
        let mut state = ChessState { run: Some(run), ..Default::default() };
        let info = DocInfo::default();
        let mut fonts = false;
        let mut h = Harness::builder().with_size(egui::vec2(360.0, 300.0)).build_ui(move |ui| {
            // The app's fonts (the pills' medium face) take effect from the next frame.
            if !fonts {
                ui.ctx().set_fonts(crate::theme::font_definitions());
                fonts = true;
                return;
            }
            let t = Tokens::get(ui.ctx());
            let _ = tab_contents(ui, &t, &mut state, false, shown, &info, None, Default::default(), 0, false, &Default::default());
        });
        h.run();
        h.get_by_label("Detecting in another book… page 306 of 748");
    }
}

#[cfg(test)]
mod pick_tests {
    use super::*;

    /// A ten-page book whose fifth page has the start position set in the Chess Merida font, read
    /// without a model.
    fn book() -> Vec<u8> {
        const ROWS: [&str; 8] = ["tMvWlVmT", "OoOoOoOo", " + + + +", "+ + + + ", " + + + +", "+ + + + ", "pPpPpPpP", "RnBqKbNr"];
        let mut content = String::from("BT /F1 20 Tf 60 724 Td ");
        for (i, line) in ROWS.iter().enumerate() {
            if i > 0 {
                content.push_str("0 -20 Td ");
            }
            content.push_str(&format!("({line}) Tj "));
        }
        content.push_str("ET\n");
        let widths = vec!["1000"; 95].join(" ");
        let objs = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 600 800] >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
            format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
            format!("<< /Type /Font /Subtype /Type1 /BaseFont /ChessMerida /FirstChar 32 /LastChar 126 /Widths [{widths}] >>"),
        ];
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
        let mut s = pdfcraft_engine::Session::new();
        let id = s.open("book.pdf", None, Arc::new(out), None).expect("the book opens");
        for at in [0, 0, 0, 0, 5, 5, 5, 5, 5] {
            s.apply(id, pdfcraft_engine::Edit::InsertBlankPage { at, width: 600.0, height: 800.0 }).expect("a blank page");
        }
        s.save_bytes(id).expect("saved").to_vec()
    }

    /// Plan A9-I4 (finding A5-04): "Diagram N" of the Text tab opens the diagram of the page the
    /// text is of, whatever page is in view: that page is scanned (it used to scan the page in
    /// view) on a worker, and the editor opens once the scan is in (it used to pick the diagram
    /// before, which did nothing).
    #[test]
    fn a_diagram_of_the_text_opens_on_the_texts_page_not_the_views() {
        let (none, _) = pdfcraft_engine::chess::pdfcraft_chessvision::models::ChessModels::from_files(Vec::new());
        pdfcraft_engine::chess::install_models(Some(none));
        let mut app = PdfCraftApp::new();
        app.open_bytes("book.pdf", None, book()).expect("opens");
        app.active = Some(0);
        let id = app.views[0].id;
        // The text of page 5 was read; the view is on page 9.
        app.views[0].go_to_page(8);
        app.run_inline = false;
        app.chess_text_action(crate::chess_text_ui::TextAction::Diagram(DiagramAt::new(4, 0)));
        for _ in 0..500 {
            app.poll_chess();
            if app.chess.editor.is_some() && app.chess.run.is_none() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let ed = app.chess.editor.as_ref().expect("the editor opened");
        assert_eq!((ed.doc, ed.page, ed.index), (id, 4, 0));
        assert!(app.chess.results_of(id).is_some_and(|r| r.diagram(4, 0).is_some()), "the text's page was scanned");
    }
}
