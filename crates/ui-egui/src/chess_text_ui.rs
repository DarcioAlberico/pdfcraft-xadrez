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
//! **Export…** writes the whole document as PGN, EPUB, HTML, Word, RTF or Markdown (with its
//! diagram images in a folder beside it), through the engine's `chess_export`, or its exercises
//! as a sheet to print (`chess_worksheet`); the dialog, the notice about diagrams still to review
//! and what the export said are `chess_export_ui`.

use std::sync::Arc;

use egui::{FontId, TextureHandle};
use pdfcraft_engine::DocId;
use pdfcraft_engine::chess::ScanStamp;
use pdfcraft_engine::chess::document::{BookOrigin, OcrMode, OcrOptions};
use pdfcraft_engine::chess::export::ChessExportOptions;
use pdfcraft_engine::chess::pdfcraft_chess::draw::BoardStyle;
use pdfcraft_engine::chess::pdfcraft_chessvision::doc::html::{Figurines, plain_text_with};
use pdfcraft_engine::chess::pdfcraft_chessvision::doc::reference::{self, LinkKind};
use pdfcraft_engine::chess::pdfcraft_chessvision::doc::{Block, ChessDoc, Inline, MoveInline, MoveStatus, ReviewFlag, UserRect};
use pdfcraft_engine::chess::reference_store::Choice;
use pdfcraft_engine::chess::span::{GameSpanRead, MoveAt};

use crate::chess_export_ui::ExportFormat;
use crate::theme::Tokens;
use crate::{PdfCraftApp, theme};

/// The Chess column's tabs (plan A9-I0a, §2.8.0). The study board, a panel of its own since T5c
/// (D11 b), is a tab of the column again; the tab is not saved, so no setting names it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChessTab {
    #[default]
    Diagrams,
    Text,
    Board,
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
    pub needs_ocr: Option<pdfcraft_engine::chess::NeedsOcr>,
    pub notes: Vec<String>,
    /// The diagrams' boards, by block index.
    boards: Vec<(usize, TextureHandle)>,
    /// The page as the book numbers it (plan A9-T2b, R3-08).
    pub label: String,
}

/// Reading a page or exporting the book, off the interface thread: the work runs on a session
/// of its own, opened from the document's working bytes, and the result is picked up when it is
/// done (inline on the web and in tests). The session is given what the app's session read for
/// the document's state, and what it reads anew is given back (plan A9, G11): the book is read
/// once per session. A [`crate::chess_job::Job`] (plan A9-I2): a panic in the work comes back as
/// an error, and dropping it asks the work to stop.
pub struct ChessJob {
    pub doc: DocId,
    pub kind: JobKind,
    /// The document's state when the job began: what it read is kept only while the document is
    /// still in it.
    stamp: Option<ScanStamp>,
    job: crate::chess_job::Job<(JobDone, JobRead)>,
}

/// Where a job reads a document from ([`PdfCraftApp::chess_job_source`]).
pub(crate) struct JobSource {
    name: String,
    bytes: Arc<Vec<u8>>,
    password: Option<String>,
    corrections: Option<pdfcraft_engine::chess::corrections::CorrectionFile>,
    cached: pdfcraft_engine::chess::ChessCached,
}

impl JobSource {
    /// The document opened in a session of its own, given its corrections and what was read.
    pub(crate) fn open(self) -> Result<(pdfcraft_engine::Session, DocId), String> {
        let mut s = pdfcraft_engine::Session::new();
        let id = s.open(self.name, None, self.bytes, self.password.as_deref()).map_err(|e| e.to_string())?;
        s.set_chess_corrections(id, self.corrections);
        s.prime_chess_cache(id, self.cached);
        Ok((s, id))
    }
}

/// What a job's session holds when it is done, to give back to the app's session.
struct JobRead {
    cached: pdfcraft_engine::chess::ChessCached,
    /// Pages it scanned anew.
    scanned: usize,
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
    /// Every page read and linked to the book's PGN (plan "Carregar PGN do livro", R6d).
    LinkBook {
        stamp: ScanStamp,
    },
    /// Every page read for the book's bookmarks, shown before they are made (plan A9-N6).
    Bookmarks {
        stamp: ScanStamp,
    },
    #[cfg(not(target_arch = "wasm32"))]
    Export {
        format: ExportFormat,
        path: std::path::PathBuf,
        image_dir: String,
    },
    #[cfg(target_arch = "wasm32")]
    Export {
        format: ExportFormat,
        name: String,
    },
}

enum JobDone {
    Read(Result<pdfcraft_engine::chess::document::ChessDocument, String>),
    Game(Result<GameSpanRead, String>),
    Export(Result<pdfcraft_engine::chess::export::ChessExport, String>),
    Linked(Result<crate::chess_reference_ui::BookLink, String>),
    /// The whole book read, for its bookmarks, with the cipher the reading ended with.
    Outline(Result<pdfcraft_engine::chess::document::ChessDocument, String>),
}

impl JobKind {
    /// A job that reads the whole book (Export, Align the book), in the slot of the long jobs
    /// ([`crate::chess_ui::ChessState::long`]); the others read a page or a few, in the page
    /// read's ([`crate::chess_ui::ChessState::reading`]).
    pub fn is_long(&self) -> bool {
        matches!(self, JobKind::Export { .. } | JobKind::LinkBook { .. } | JobKind::Bookmarks { .. })
    }

    /// What a job of this kind came to when its work could not run or did not end (`why`).
    fn failed(&self, why: String) -> JobDone {
        match self {
            JobKind::Read { .. } => JobDone::Read(Err(why)),
            JobKind::Game { .. } => JobDone::Game(Err(why)),
            JobKind::Export { .. } => JobDone::Export(Err(why)),
            JobKind::LinkBook { .. } => JobDone::Linked(Err(why)),
            JobKind::Bookmarks { .. } => JobDone::Outline(Err(why)),
        }
    }
}

impl ChessJob {
    /// Wait until the work is done (its result waiting for the next frame) or `timeout` passed;
    /// whether it is done. For tests, which wait for the work and not for the clock (plan
    /// A9-Q4b, finding A7-15); the app never blocks on a job.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn wait(&self, timeout: std::time::Duration) -> bool {
        self.job.wait(timeout)
    }

    /// What the panel says while it runs.
    pub fn label(&self) -> &'static str {
        match self.kind {
            JobKind::Read { ocr: true, .. } => tl!("Reading the page with chess OCR…"),
            JobKind::Read { .. } => tl!("Reading the page…"),
            JobKind::Game { .. } => tl!("Reading the pages around the game…"),
            JobKind::Export { .. } => tl!("Exporting the chess document…"),
            JobKind::LinkBook { .. } => tl!("Aligning the book to the PGN…"),
            JobKind::Bookmarks { .. } => tl!("Reading the book for its bookmarks…"),
        }
    }

    /// The row of a long job ([`JobKind::is_long`]): which page it is on, of how many.
    pub fn long_row(&self) -> LongRow {
        let (done, total) = self.job.progress();
        let said = match self.kind {
            JobKind::Export { .. } => tl!("Exporting the chess document: page {done} of {of}"),
            JobKind::Bookmarks { .. } => tl!("Reading the book for its bookmarks: page {done} of {of}"),
            _ => tl!("Aligning the book to the PGN: page {done} of {of}"),
        };
        // The page it is on, as the scan's row says it: one more than those done, at most the last.
        let on = done.saturating_add(1).min(total);
        LongRow { said: crate::i18n::fmt(said, &[("done", &on.to_string()), ("of", &total.to_string())]), done, total }
    }
}

/// The most pages Export and Align the book read on the web, where they run inside one frame
/// (plan A9-I2); the exercise sheet's count too (plan A9-N12b).
pub(crate) const WEB_WHOLE_BOOK_PAGES: usize = 20;

/// A long job of the document shown, as its row says it: pages done of the total (plan A9-I2).
#[derive(Clone, Debug, PartialEq)]
pub struct LongRow {
    pub said: String,
    pub done: usize,
    pub total: usize,
}

/// The document's own games, `games` (a PGN), read into an index that `options` links to first
/// (plan A9-N2), off the interface's thread; what to note when they can't be indexed (they are
/// left out, the book's PGN and the reading still linked).
pub(crate) fn link_games(options: &mut ChessExportOptions, games: Option<&str>) -> Option<String> {
    let pgn = games?;
    match pdfcraft_engine::chess::pdfcraft_chessvision::doc::reference::ReferenceIndex::from_pgn(pgn) {
        Ok(index) => {
            options.games = Some(Arc::new(index));
            None
        }
        Err(e) => Some(format!("the document's games were not linked: {e}")),
    }
}

/// Export document `id` as `export` (the exercise sheet laid out as `sheet` says) with its own
/// games, `games` (a PGN), read into an index first (plan A9-N2): in the export's job, off the
/// interface's thread. Games that can't be indexed are left out of it, said in the export's notes.
fn export_with_games(
    s: &mut pdfcraft_engine::Session,
    id: DocId,
    export: ExportFormat,
    mut options: ChessExportOptions,
    sheet: pdfcraft_engine::chess::worksheet::SheetOptions,
    games: Option<&str>,
    ctl: &crate::chess_job::JobCtl,
) -> Result<pdfcraft_engine::chess::export::ChessExport, String> {
    let left_out = link_games(&mut options, games);
    scan_in_steps(s, id, &options, ctl)?;
    let mut made = match export.engine() {
        Some(format) => s.chess_export(id, format, &options)?,
        // The exercise sheet, a PDF of its own (plan A9-N12b), said as the other exports are.
        None => s.chess_worksheet(id, &options, &sheet)?.into_export(),
    };
    made.notes.extend(left_out);
    Ok(made)
}

/// Scan the pages an export of document `id` reads (plan A9-I2), a block at a time, so the job
/// says how far it is and stops between two blocks; the export then finds them scanned. Reading
/// their text and writing come after the last page, and are not counted.
fn scan_in_steps(s: &mut pdfcraft_engine::Session, id: DocId, options: &ChessExportOptions, ctl: &crate::chess_job::JobCtl) -> Result<(), String> {
    /// Pages scanned at a time.
    const BLOCK: usize = 16;
    let pages: Vec<usize> = if options.pages.is_empty() { (0..s.get(id).map_or(0, |d| d.info.pages.len())).collect() } else { options.pages.clone() };
    ctl.progress(0, pages.len());
    let mut done: usize = 0;
    for block in pages.chunks(BLOCK) {
        // Stopped: the job was dropped, and what it says now is never seen.
        if ctl.cancelled() {
            return Err(String::new());
        }
        s.chess_scan(id, block, &options.scan, true)?;
        done = done.saturating_add(block.len());
        ctl.progress(done, pages.len());
    }
    Ok(())
}

/// What the text tab asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum TextAction {
    /// Read the current page (with chess OCR when `true`).
    Read(bool),
    /// Outline a box of `page` (user space) on the page.
    Show(usize, [f32; 4]),
    /// Open the diagram in the editor.
    Diagram(pdfcraft_engine::chess::DiagramAt),
    /// Put the game the move belongs to on the board, at that move, the pages around its page
    /// read too (`chess_source`).
    OpenGame(MoveAt),
    /// Open the export dialog (plan A9-X2).
    OpenExport,
    /// Say where the document's pages came from.
    Origin(BookOrigin),
    /// Load the book's PGN (`true`) or remove it (plan "Carregar PGN do livro", R6a).
    BookPgn(bool),
    /// Keep the PGN beside the PDF as the book's PGN (R6c).
    SiblingPgn,
    /// Put game `game` of the book's PGN on the board at node `node` (R6c).
    OpenPgnGame { game: usize, node: usize },
    /// Read every page and link it to the book's PGN (R6d).
    LinkBook,
    /// Stop the long job of the document shown (Export, Align the book; plan A9-I2).
    Stop,
    /// Go to page `page` (0-based) and put game `game` of the book's PGN on the board at its start
    /// (R6d: the PGN's games list).
    PgnGameAt { game: usize, page: Option<usize> },
    /// The user's choice for a move in conflict with the PGN (R6e).
    Decide { at: MoveAt, san: String, san_read: String, choice: pdfcraft_engine::chess::reference_store::Choice },
}

/// The Text tab's menu (⋯; plan A9-I9, sketch c): where the book's pages came from, its PGN
/// (load one, or the one beside the PDF; align the book to it) and the export, with Remove the
/// book's PGN last, in the danger colour. While a PGN is being indexed (`pgn.indexing`, plan
/// A9-I3b) none is offered.
fn text_menu(ui: &mut egui::Ui, t: &Tokens, origin: BookOrigin, pgn: &crate::chess_reference_ui::BookPgnView) -> Option<TextAction> {
    let mut action = None;
    let entry = |ui: &mut egui::Ui, text: egui::WidgetText| crate::widgets::menu_entry(ui, true, egui::Button::new(text));
    // A submenu, not a list: egui keeps one popup open, so a list would close the menu.
    ui.menu_button(tl!("Book origin"), |ui| {
        for o in BookOrigin::ALL {
            if crate::widgets::menu_radio(ui, o == origin, origin_label(o)).clicked() && o != origin {
                action = Some(TextAction::Origin(o));
            }
        }
        ui.label(
            egui::RichText::new(tl!(
                "Where the book's pages came from. Auto believes each page's text layer unless it looks wrong; Born digital never uses chess OCR; Scanned reads every page with chess OCR (needs the chess-glyphs model)."
            ))
            .small()
            .color(t.text_muted),
        );
    });
    ui.separator();
    let book_pgn = pgn.summary.as_ref();
    match book_pgn {
        None if pgn.indexing => {}
        None => {
            if entry(ui, tl!("Load the book's PGN…").into())
                .on_hover_text(tl!(
                    "A PGN of this book's games (a course's or an e-book's export): the moves of each page read here are linked to it."
                ))
                .clicked()
            {
                action = Some(TextAction::BookPgn(true));
            }
            if let Some(name) = pgn.sibling.as_deref()
                && entry(ui, crate::i18n::fmt(tl!("Use {name}"), &[("name", name)]).into())
                    .on_hover_text(tl!("A PGN with the same name sits beside this PDF: keep it as the book's PGN."))
                    .clicked()
            {
                action = Some(TextAction::SiblingPgn);
            }
        }
        // Aligning: the long job's row at the top says how far it is, with Stop.
        Some(_) if pgn.aligning => {}
        Some(_) => {
            let label = if pgn.link.is_some() { tl!("Align again") } else { tl!("Align the book to the PGN") };
            if entry(ui, label.into())
                .on_hover_text(tl!(
                    "Read every page and link it to the book's PGN: how many moves it confirms, reads from the PGN and finds in conflict, and the page each game starts on. It takes as long as exporting the book."
                ))
                .clicked()
            {
                action = Some(TextAction::LinkBook);
            }
        }
    }
    // Export book… opens the export dialog (plan A9-X2), which holds the format and its choices.
    if let Some(a) = crate::chess_export_ui::entry(ui) {
        action = Some(a);
    }
    if book_pgn.is_some() {
        ui.separator();
        if entry(ui, egui::RichText::new(tl!("Remove the book's PGN")).color(t.danger_text).into()).clicked() {
            action = Some(TextAction::BookPgn(false));
        }
    }
    action
}

/// What the document keeps of the book's PGN, in a line.
fn book_pgn_line(ui: &mut egui::Ui, t: &Tokens, s: &crate::chess_reference_ui::BookPgnSummary) {
    let name = s.name.as_deref().unwrap_or("PGN");
    let said = crate::i18n::fmt(tl!("Book's PGN: {name} ({games} games)"), &[("name", name), ("games", &crate::i18n::number(s.games))]);
    ui.label(egui::RichText::new(said).small().color(t.text_muted)).on_hover_text(tl!(
        "The pages read here are linked to the book's PGN: a move the reader could not play is read from the PGN (underlined), and a move the PGN plays from another position is in conflict (in italics)."
    ));
}

/// The whole book and the PGN (R6d): what Align the book found, and the PGN's games, each with
/// the first page that discusses it.
fn book_link_summary(ui: &mut egui::Ui, t: &Tokens, view: &crate::chess_reference_ui::BookPgnView) -> Option<TextAction> {
    let mut action = None;
    view.summary.as_ref()?;
    let link = view.link.as_ref().filter(|_| !view.aligning)?;
    let said = crate::i18n::fmt(
        tl!("Whole book: {linked} of {moves} moves linked ({recovered} read from the PGN), {conflict} in conflict, on {pages} of {all} pages."),
        &[
            ("linked", &crate::i18n::number(link.linked())),
            ("moves", &crate::i18n::number(link.moves())),
            ("recovered", &crate::i18n::number(link.counts.recovered)),
            ("conflict", &crate::i18n::number(link.counts.conflict)),
            ("pages", &link.pages_linked.to_string()),
            ("all", &link.pages.to_string()),
        ],
    );
    ui.label(egui::RichText::new(said).small().color(t.text_muted));
    let title = crate::i18n::fmt(tl!("Games of the book's PGN ({n})"), &[("n", &link.games.len().to_string())]);
    egui::CollapsingHeader::new(title).id_salt("chess-book-pgn-games").show(ui, |ui| {
        for g in &link.games {
            let name =
                if g.name.is_empty() { crate::i18n::fmt(tl!("Game {n}"), &[("n", &g.game.saturating_add(1).to_string())]) } else { g.name.clone() };
            let shown = match g.first_page {
                Some(p) => {
                    let page = g.first_label.clone().unwrap_or_else(|| p.saturating_add(1).to_string());
                    crate::i18n::fmt(tl!("p. {page} · {name}"), &[("page", &page), ("name", &name)])
                }
                None => crate::i18n::fmt(tl!("{name} (no page discusses it)"), &[("name", &name)]),
            };
            if ui.link(shown).on_hover_text(tl!("Go to its page and put the game on the board.")).clicked() {
                action = Some(TextAction::PgnGameAt { game: g.game, page: g.first_page });
            }
        }
    });
    action
}

/// How a move of the text is linked to the book's PGN, for its tooltip and its style: read from
/// the PGN (with the PGN's move), or in conflict with it.
fn reference_note(m: &MoveInline) -> Option<(LinkKind, String)> {
    let kind = m.reference.as_ref()?.kind;
    match kind {
        LinkKind::Recovered => {
            let san = m.review.iter().find_map(|f| if let ReviewFlag::FromReference { san } = f { Some(san.as_str()) } else { None }).unwrap_or("");
            Some((kind, crate::i18n::fmt(tl!("Read from the book's PGN: {san}"), &[("san", san)])))
        }
        LinkKind::Conflict => Some((
            kind,
            tl!("The book's PGN plays this move from another position: check it, then use the PGN's move or keep the book's.").to_string(),
        )),
        LinkKind::Confirmed | LinkKind::Absent => None,
    }
}

fn origin_label(o: BookOrigin) -> &'static str {
    match o {
        BookOrigin::Auto => tl!("Auto (page by page)"),
        BookOrigin::Digital => tl!("Born digital (text layer only)"),
        BookOrigin::Scanned => tl!("Scanned (chess OCR on every page)"),
    }
}

pub(crate) fn status_label(s: MoveStatus) -> &'static str {
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

/// A move of the Text tab as it is drawn, said and acted on (plan A9-I9).
struct MoveView {
    at: MoveAt,
    look: crate::chess_text_para::Look,
    /// Its name for assistive technology: the number, the move in words, its state and its link.
    name: String,
    tip: String,
    /// A double-click opens its game on the board (it has one, or may go on from the page before).
    opens: bool,
    /// Linked to the book's PGN: the PGN's game and node it opens there (R6c).
    in_pgn: Option<(usize, usize)>,
    /// In conflict with the book's PGN, and the user's choice there (R6e).
    conflict: bool,
    choice: Option<Choice>,
}

/// [`MoveView`] of move `m`, at `at`, with the choices made on its page (`chosen`); `caption`:
/// the font has the caption's shape (▸), else a plain one stands in.
fn move_view(m: &MoveInline, at: MoveAt, chosen: &pdfcraft_engine::chess::reference_store::OnPage, t: &Tokens, caption: bool) -> MoveView {
    let mut tip = status_label(m.status).to_string();
    let linked = reference_note(m);
    // A conflict the user decided says what they chose (R6e).
    let conflict = linked.as_ref().is_some_and(|(k, _)| *k == LinkKind::Conflict);
    let choice = if conflict { chosen.found.iter().find(|(f, _)| *f == at).map(|(_, c)| *c) } else { None };
    let note = match choice {
        Some(Choice::Book) => Some(tl!("You kept the book's move.").to_string()),
        Some(Choice::Pgn) => Some(tl!("You chose the book's PGN's move: its game is the one to follow here.").to_string()),
        None => linked.as_ref().map(|(_, n)| n.clone()),
    };
    if let Some(note) = &note {
        tip = format!("{tip}\n{note}");
    }
    if let Some(fen) = &m.fen {
        tip = format!("{tip}\n{fen}");
    }
    // A move with no position on its page alone may go on from the page before: its game is
    // looked for there.
    let opens = m.game.is_some() || m.status == MoveStatus::Unchecked;
    let in_pgn = m.reference.as_ref().and_then(|r| Some((r.game?, r.node?)));
    if in_pgn.is_some() {
        tip = format!("{tip}\n{}", tl!("Double-click to open the book's PGN game here."));
    } else if opens {
        tip = format!("{tip}\n{}", tl!("Double-click to open this game on the board."));
    }
    let kind = linked.as_ref().map(|(k, _)| *k);
    // The link is said in words too, never by the style alone (R6g).
    let said = match (kind, choice) {
        (Some(LinkKind::Recovered), _) => Some(tl!("read from the book's PGN")),
        (Some(LinkKind::Conflict), None) => Some(tl!("in conflict with the book's PGN")),
        (Some(LinkKind::Conflict), Some(Choice::Pgn)) => Some(tl!("in conflict, the PGN's move chosen")),
        (Some(LinkKind::Conflict), Some(Choice::Book)) => Some(tl!("in conflict, the book's move kept")),
        _ => None,
    };
    let number = m.number.map(|n| n.text());
    let san = if m.san.is_empty() { m.read_san() } else { m.san.as_str() };
    let name = crate::chess_text_para::move_name(number.as_deref(), san, m.status, said);
    let marker = match crate::chess_text_para::marker(m.status) {
        "▸" if !caption => ">",
        shape => shape,
    };
    let look = crate::chess_text_para::Look {
        colour: crate::chess_text_para::status_colour(m.status, t),
        marker,
        underline: matches!((kind, choice), (Some(LinkKind::Recovered), _) | (Some(LinkKind::Conflict), Some(Choice::Pgn))),
        italics: matches!((kind, choice), (Some(LinkKind::Conflict), None)),
    };
    MoveView { at, look, name, tip, opens, in_pgn, conflict, choice }
}

/// A move's context menu: open its game (the book's PGN's, or the page's) and, in conflict with
/// the PGN, the user's choice (R6e).
fn move_menu(ui: &mut egui::Ui, v: &MoveView, m: &MoveInline) -> Option<TextAction> {
    let mut action = None;
    if let Some((game, node)) = v.in_pgn
        && ui.button(tl!("Open the book's PGN game here")).clicked()
    {
        action = Some(TextAction::OpenPgnGame { game, node });
        ui.close();
    }
    if v.opens && ui.button(tl!("Open this game on the board")).clicked() {
        action = Some(TextAction::OpenGame(v.at));
        ui.close();
    }
    if v.conflict {
        ui.separator();
        for (c, label) in [(Choice::Pgn, tl!("Use the PGN's move")), (Choice::Book, tl!("Keep the book's move"))] {
            if v.choice != Some(c) && ui.button(label).clicked() {
                action = Some(TextAction::Decide { at: v.at, san: m.san.clone(), san_read: m.read_san().to_owned(), choice: c });
                ui.close();
            }
        }
    }
    action
}

/// A paragraph of the Text tab laid out (plan A9-I9b): kept from frame to frame while its
/// [`paragraph_key`] holds, so a frame neither builds its job, nor finds its moves' rectangles,
/// nor words its moves again.
struct Paragraph {
    key: u64,
    galley: Arc<egui::Galley>,
    /// Each inline's characters, for a click on the prose.
    inlines: Vec<std::ops::Range<usize>>,
    moves: Vec<PlacedMove>,
}

/// A move of a laid-out paragraph: its inline, its rectangles (one per row it is on, in the
/// galley's coordinates) and how it is drawn, said and acted on.
struct PlacedMove {
    k: usize,
    parts: Vec<egui::Rect>,
    view: MoveView,
}

/// What a paragraph's layout depends on beyond the paragraph, the same for every paragraph of a
/// frame: the theme, the interface's language, the figurines, the caption's shape, and the fonts
/// egui lays out with and at what scale (plan A9-I9c: a galley kept across a change of fonts
/// draws its glyphs from an atlas that no longer exists).
fn frame_key(ctx: &egui::Context, figurines: Figurines, caption: bool) -> u64 {
    use std::hash::{Hash, Hasher};
    let t = Tokens::get(ctx);
    let fonts = (theme::fonts_generation(ctx), ctx.pixels_per_point().to_bits());
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (t.dark(), t.text.to_array(), crate::i18n::current().code(), format!("{figurines:?}"), caption, fonts).hash(&mut h);
    h.finish()
}

/// The key of paragraph `inlines`' layout: its text, its moves' states, links and the choices made
/// on them (`chosen`, at `at_of`), the width it wraps at and the frame's (`frame`). No string is
/// made for it.
fn paragraph_key(
    inlines: &[Inline],
    chosen: &pdfcraft_engine::chess::reference_store::OnPage,
    at_of: impl Fn(usize) -> MoveAt,
    width: f32,
    frame: &u64,
) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (frame, width.to_bits(), inlines.len()).hash(&mut h);
    for (k, inline) in inlines.iter().enumerate() {
        std::mem::discriminant(inline).hash(&mut h);
        match inline {
            Inline::Move(m) => {
                (&m.text, &m.san, std::mem::discriminant(&m.status), m.game.is_some(), m.fen.as_deref()).hash(&mut h);
                m.reference.as_ref().map(|r| (std::mem::discriminant(&r.kind), r.game, r.node)).hash(&mut h);
                let at = at_of(k);
                chosen.found.iter().find(|(f, _)| *f == at).map(|(_, c)| std::mem::discriminant(c)).hash(&mut h);
            }
            Inline::Text { text, .. } | Inline::Link { text, .. } => text.hash(&mut h),
            other => other.text().hash(&mut h),
        }
    }
    h.finish()
}

/// Lay paragraph `inlines` out (plan A9-I9): one job in the body font, its moves' views and the
/// rectangles their letters take.
#[allow(clippy::too_many_arguments)]
fn lay_out_paragraph(
    ui: &egui::Ui,
    inlines: &[Inline],
    figurines: Figurines,
    chosen: &pdfcraft_engine::chess::reference_store::OnPage,
    at_of: impl Fn(usize) -> MoveAt,
    tokens: &Tokens,
    caption: bool,
    width: f32,
    key: u64,
) -> Paragraph {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let mut views: Vec<Option<MoveView>> = inlines
        .iter()
        .enumerate()
        .map(|(k, inline)| match inline {
            Inline::Move(m) => Some(move_view(m, at_of(k), chosen, tokens, caption)),
            _ => None,
        })
        .collect();
    let laid = crate::chess_text_para::paragraph(
        inlines,
        &font,
        tokens.text,
        width,
        |i| plain_text_with(std::slice::from_ref(i), figurines),
        |k, _| views.get(k).and_then(|v| v.as_ref()).map(|v| v.look),
    );
    let galley = ui.painter().layout_job(laid.job);
    let moves = laid
        .moves
        .iter()
        .filter_map(|(k, range)| {
            let view = views.get_mut(*k)?.take()?;
            Some(PlacedMove { k: *k, parts: crate::chess_text_para::rects(&galley, range), view })
        })
        .collect();
    Paragraph { key, galley, inlines: laid.inlines, moves }
}

fn rect_of(r: Option<UserRect>) -> Option<[f32; 4]> {
    let r = r?.page_rect();
    [r.x0, r.y0, r.x1, r.y1].iter().all(|v| v.is_finite()).then_some([r.x0, r.y0, r.x1, r.y1])
}

/// How moves are printed here: figurines when the interface's fonts have them.
fn figurines(ctx: &egui::Context) -> Figurines {
    if ctx.fonts_mut(|f| f.has_glyphs(&FontId::proportional(13.0), "♔♕♖♗♘♙")) { Figurines::Unicode } else { Figurines::AsPrinted }
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
    long: Option<&LongRow>,
    pgn: &crate::chess_reference_ui::BookPgnView,
) -> Option<TextAction> {
    let book_pgn = pgn.summary.as_ref();
    let mut action = None;
    if let Some(row) = long
        && crate::chess_panel_head::long_job(ui, t, row)
    {
        action = Some(TextAction::Stop);
    }
    let current = text.filter(|x| x.doc == id && x.page == page && Some(x.stamp) == now && x.origin == origin);
    // With no page shown yet, the work under way is the heading, said once (plan A9-I14, A6-26).
    if let Some(label) = busy.filter(|_| current.is_some()) {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(egui::RichText::new(label).color(t.text_muted));
        });
    }
    // The top (plan A9-I9, sketch c): the page read and Read with OCR; the book's origin, its
    // PGN and the export in the menu.
    let heading = current.as_ref().map_or_else(
        || egui::RichText::new(busy.unwrap_or(tl!("Reading the page…"))).color(t.text_muted),
        |x| egui::RichText::new(crate::i18n::fmt(tl!("Recognised text of page {p}"), &[("p", &x.label)])).font(theme::semibold(12.5)),
    );
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let name = tl!("More Text tab actions");
            let menu = egui::containers::menu::MenuButton::from_button(
                egui::Button::image(crate::icons::image("ellipsis", 16.0, t.icon)).min_size(egui::vec2(28.0, 24.0)),
            );
            let (r, picked) = menu.ui(ui, |ui| text_menu(ui, t, origin, pgn));
            r.on_hover_text(name).widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
            if let Some(a) = picked.and_then(|p| p.inner) {
                action = Some(a);
            }
            if ui
                .button(tl!("Read with OCR"))
                .on_hover_text(tl!("Read the page image instead of its text layer (needs the chess-glyphs model)."))
                .clicked()
            {
                action = Some(TextAction::Read(true));
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                if current.is_none() && busy.is_some() {
                    ui.spinner();
                }
                ui.add(egui::Label::new(heading).truncate());
            });
        });
    });
    // What the document keeps of the book's PGN, and the whole book linked to it.
    if pgn.indexing {
        ui.label(egui::RichText::new(tl!("Indexing the book's PGN…")).small().color(t.text_muted));
    }
    if let Some(s) = book_pgn {
        book_pgn_line(ui, t, s);
    }
    if let Some(a) = book_link_summary(ui, t, pgn) {
        action = Some(a);
    }
    let Some(x) = current else {
        // Follows the page: read its text layer now (quick); OCR is on request.
        if action.is_none() && busy.is_none() {
            action = Some(TextAction::Read(false));
        }
        return action;
    };
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
        ui.label(egui::RichText::new(crate::i18n::fmt(said, &[("e", &crate::chess_reasons::reason_text(why))])).small().color(t.text_muted));
    }
    for n in &x.notes {
        ui.label(egui::RichText::new(n).small().color(t.text_muted));
    }
    if book_pgn.is_some() {
        let mut counts = [0usize; 4];
        // The choices made on the page's moves, found on them as read now (plan A9-L1b).
        let mine: Vec<_> = x.chess.pages.iter().map(|p| pgn.choices(p)).collect();
        let found: Vec<(MoveAt, Choice)> = mine.iter().flat_map(|c| c.found.iter().copied()).collect();
        // Each move with its place on the page, for the decisions on conflicts (R6e).
        let placed = x.chess.pages.iter().flat_map(|p| {
            p.blocks
                .iter()
                .enumerate()
                .flat_map(move |(b, block)| block.inlines().iter().enumerate().map(move |(i, inline)| (MoveAt::new(p.index, b, i), inline)))
        });
        for (at, m) in placed.filter_map(|(at, i)| if let Inline::Move(m) = i { Some((at, m)) } else { None }) {
            let slot = match m.reference.as_ref().map(|r| r.kind) {
                Some(LinkKind::Confirmed) => counts.get_mut(0),
                Some(LinkKind::Recovered) => counts.get_mut(1),
                Some(LinkKind::Conflict) => {
                    if !found.iter().any(|(f, _)| *f == at)
                        && let Some(open) = counts.get_mut(3)
                    {
                        *open = open.saturating_add(1);
                    }
                    counts.get_mut(2)
                }
                Some(LinkKind::Absent) | None => None,
            };
            if let Some(n) = slot {
                *n = n.saturating_add(1);
            }
        }
        let [confirmed, recovered, conflict, open] = counts.map(|n| n.to_string());
        let said = crate::i18n::fmt(
            tl!("Book's PGN on this page: {confirmed} confirmed, {recovered} read from the PGN, {conflict} in conflict ({open} to review)."),
            &[("confirmed", &confirmed), ("recovered", &recovered), ("conflict", &conflict), ("open", &open)],
        );
        ui.label(egui::RichText::new(said).small().color(t.text_muted));
        let unmatched = mine.iter().map(|c| c.unmatched).sum::<usize>();
        if unmatched > 0 {
            let said = crate::i18n::fmt(tl!("Choices made on this page not found in its reading (kept): {n}"), &[("n", &unmatched.to_string())]);
            ui.label(egui::RichText::new(said).small().color(t.text_muted));
        }
    }
    ui.separator();
    let figurines = figurines(ui.ctx());
    let Some(page_doc) = x.chess.pages.first() else {
        ui.label(tl!("No text on this page."));
        return action;
    };
    // The choices made on the page's moves, found on them as read now (plan A9-L1b).
    let chosen = pgn.choices(page_doc);
    if page_doc.blocks.is_empty() {
        ui.label(tl!("No text on this page."));
    }
    // The caption's shape, when the font has it (plan A9-I9).
    let caption = ui.ctx().fonts_mut(|f| f.has_glyphs(&egui::TextStyle::Body.resolve(ui.style()), "▸"));
    let frame = frame_key(ui.ctx(), figurines, caption);
    for (bi, block) in page_doc.blocks.iter().enumerate() {
        ui.push_id(bi, |ui| match block {
            Block::Heading { inlines, source_rect, .. } => {
                // Clickable, but never a stop of the keyboard: only moves and diagrams are (plan A9-I9).
                let r = ui.add(
                    egui::Label::new(egui::RichText::new(plain_text_with(inlines, figurines)).font(theme::semibold(14.0))).sense(egui::Sense::CLICK),
                );
                if r.clicked()
                    && let Some(rect) = rect_of(*source_rect)
                {
                    action = Some(TextAction::Show(page, rect));
                }
            }
            Block::Paragraph { inlines, source_rect, .. } => {
                // One galley (plan A9-I9): the prose read, never a stop of the keyboard; each move
                // a section of its own, with its state's colour and shape, and one stop. Laid out
                // once per reading, width, theme and choices (plan A9-I9b): a frame only looks it
                // up, and asks the moves' words of assistive technology and of a hover.
                let tokens = Tokens::get(ui.ctx());
                let width = ui.available_width();
                let at_of = |k: usize| MoveAt::new(u32::try_from(page).unwrap_or(u32::MAX), bi, k);
                let key = paragraph_key(inlines, &chosen, at_of, width, &frame);
                let id = ui.id().with("chess-paragraph");
                let cached = ui.ctx().data(|d| d.get_temp::<Arc<Paragraph>>(id)).filter(|p| p.key == key);
                let para = cached.unwrap_or_else(|| {
                    let made = Arc::new(lay_out_paragraph(ui, inlines, figurines, &chosen, at_of, &tokens, caption, width, key));
                    ui.ctx().data_mut(|d| d.insert_temp(id, Arc::clone(&made)));
                    made
                });
                let (rect, prose) = ui.allocate_exact_size(para.galley.size(), egui::Sense::CLICK);
                // The paragraph's text, read as one; its moves are labels of their own.
                prose.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, para.galley.text()));
                ui.ctx().accesskit_node_builder(prose.id, |node| node.set_role(egui::accesskit::Role::Paragraph));
                let mut marks: Vec<(egui::Rect, bool, bool)> = Vec::new();
                for placed in &para.moves {
                    let Some(Inline::Move(m)) = inlines.get(placed.k) else { continue };
                    let v = &placed.view;
                    let mut responses = Vec::with_capacity(placed.parts.len());
                    for (n, part) in placed.parts.iter().enumerate() {
                        let part = part.translate(rect.min.to_vec2());
                        // The move's first row takes the keyboard; a row it wraps onto is clicked.
                        let sense = if n == 0 { egui::Sense::click() } else { egui::Sense::CLICK };
                        let r = ui.interact(part, ui.id().with(("chess-move", placed.k, n)), sense);
                        if n == 0 {
                            r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &v.name));
                        }
                        let r = if r.hovered() || r.has_focus() { r.on_hover_text(&v.tip) } else { r };
                        // A click outlines the move on the page; a double-click (its first click
                        // outlined it) opens its game on the board.
                        if r.double_clicked()
                            && let Some((game, node)) = v.in_pgn
                        {
                            action = Some(TextAction::OpenPgnGame { game, node });
                        } else if r.double_clicked() && v.opens {
                            action = Some(TextAction::OpenGame(v.at));
                        } else if r.clicked()
                            && let Some(rect) = rect_of(m.source_rect)
                        {
                            action = Some(TextAction::Show(page, rect));
                        }
                        if v.opens || v.in_pgn.is_some() {
                            r.context_menu(|ui| {
                                if let Some(a) = move_menu(ui, v, m) {
                                    action = Some(a);
                                }
                            });
                        }
                        if r.has_focus() {
                            crate::widgets::own_ring(ui.ctx(), r.id);
                        }
                        responses.push((part, r.hovered(), r.has_focus()));
                    }
                    // Every row of a move shows it pointed at and with the keyboard (plan A9-I9b).
                    marks.extend(crate::chess_text_para::marks(&responses));
                }
                // Under the text: the move pointed at, and the ring of the one with the keyboard.
                for (part, hovered, focused) in marks {
                    if hovered {
                        ui.painter().rect_filled(part.expand(1.0), egui::CornerRadius::same(3), tokens.hover);
                    }
                    if focused {
                        ui.painter().rect_stroke(
                            part.expand(2.0),
                            egui::CornerRadius::same(3),
                            egui::Stroke::new(2.0, tokens.accent),
                            egui::StrokeKind::Outside,
                        );
                    }
                }
                ui.painter().galley(rect.min, Arc::clone(&para.galley), tokens.text);
                // A click on the prose outlines its words on the page.
                if prose.clicked()
                    && let Some(at) = prose.interact_pointer_pos()
                {
                    let c = para.galley.cursor_from_pos(at - rect.min).index.0;
                    let k = para.inlines.iter().position(|r| r.contains(&c));
                    let source = k.and_then(|k| inlines.get(k)).and_then(|i| i.source_rect()).or(*source_rect);
                    if let Some(rect) = rect_of(source) {
                        action = Some(TextAction::Show(page, rect));
                    }
                }
            }
            Block::Diagram(d) => {
                ui.horizontal(|ui| {
                    let title = crate::i18n::fmt(tl!("Diagram {n}"), &[("n", &(d.number + 1).to_string())]);
                    if let Some((_, tex)) = x.boards.iter().find(|(i, _)| *i == bi) {
                        // One stop per diagram (plan A9-I9): its title; the picture is clicked.
                        let r = ui.add(egui::Image::new(tex).fit_to_exact_size(egui::vec2(96.0, 96.0)).sense(egui::Sense::CLICK));
                        // The picture opens the diagram as its title does, and is named by it (plan
                        // A9-I11a, WCAG 1.1.1 and 4.1.2).
                        let named = crate::i18n::fmt(tl!("Diagram {n}, open"), &[("n", &(d.number + 1).to_string())]);
                        r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, &named));
                        if r.clicked() {
                            action = Some(TextAction::Diagram(pdfcraft_engine::chess::DiagramAt::new(page, d.number)));
                        }
                    }
                    ui.vertical(|ui| {
                        if ui.link(&title).clicked() {
                            action = Some(TextAction::Diagram(pdfcraft_engine::chess::DiagramAt::new(page, d.number)));
                        }
                        // The position it shows: the PGN's, the one the text settled, the one read.
                        match d.shown() {
                            Some(shown) => {
                                ui.add(egui::Label::new(egui::RichText::new(shown.fen).monospace().small()).wrap());
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
            let fen = pdfcraft_engine::chess::pdfcraft_chess::fen::Fen::parse(d.shown()?.fen).ok()?;
            let (w, h, rgba) = pdfcraft_engine::chess::office::diagram_rgba(fen.placement(), pieces, &style, 192).ok()?;
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
        let label = self.page_label_in(id, page);
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
            label: label.clone(),
        };
        if self.session.get(id).is_some_and(|d| d.check_copying().is_err()) {
            self.chess.doc_mut(id).text = Some(failed(tl!("This document does not allow copying its content, so its text is not read.").to_string()));
            return;
        }
        // A read running is dropped for this one (plan A9-I2): the last asked wins.
        let ocr = ocr || origin == BookOrigin::Scanned;
        let mode = if ocr { OcrMode::All } else { OcrMode::Off };
        let book = self.chess_cipher(id);
        let settings = self.chess_scan_settings();
        // The book's PGN, when the document keeps one: the page read is linked to it.
        let reference = self.book_pgn_index(id);
        self.start_chess_job(id, JobKind::Read { page, stamp, origin, ocr }, move |s, id, _| {
            let options = OcrOptions::new(mode).with_book(book.as_ref());
            let mut read = s.chess_document_with(id, &[page], &settings, None, &options);
            if let (Ok(read), Some(index)) = (read.as_mut(), reference.as_deref()) {
                reference::link(&mut read.doc, index);
            }
            JobDone::Read(read)
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
        let Some(text) = self.chess.text_of(id).filter(|t| u32::try_from(t.page).ok() == Some(at.page) && t.stamp == stamp) else {
            return;
        };
        let Some(centre) = text.chess.pages.first().filter(|p| p.index == at.page).cloned() else { return };
        let mode = if text.ocr { OcrMode::All } else { OcrMode::Off };
        let book = self.chess_cipher(id);
        let settings = self.chess_scan_settings();
        self.start_chess_job(id, JobKind::Game { at, stamp }, move |s, id, _| {
            let options = OcrOptions::new(mode).with_book(book.as_ref());
            JobDone::Game(s.chess_game_span(id, centre, at, &settings, &options))
        });
    }

    /// Link the whole book (plan "Carregar PGN do livro", R6d): read every page of the active
    /// document as the export does, off the interface thread, a block of pages at a time so the
    /// Text tab can say how far it is, and align them with its book's PGN. What it found is kept
    /// for the session ([`crate::chess_reference_ui::BookLink`]).
    pub(crate) fn link_whole_book(&mut self) {
        /// Pages read at a time.
        const BLOCK: usize = 16;
        let Some((_, id)) = self.active_ids() else { return };
        let Some(index) = self.book_pgn_index(id) else {
            return self.inform(tl!("This document keeps no book's PGN.").to_string());
        };
        if self.chess.long.is_some() {
            return self.inform(tl!("A chess document is already being read or exported").to_string());
        }
        if !self.may_copy(id) {
            return;
        }
        let (Some(stamp), Some(pages)) = (self.session.chess_stamp(id), self.session.get(id).map(|d| d.info.pages.len())) else { return };
        let mode = self.chess_origin(id).ocr_mode();
        let book = self.chess_cipher(id);
        let settings = self.chess_scan_settings();
        self.start_chess_job(id, JobKind::LinkBook { stamp }, move |s, id, ctl| {
            let options = OcrOptions::new(mode).with_book(book.as_ref());
            let all: Vec<usize> = (0..pages).collect();
            let mut doc =
                ChessDoc { format: pdfcraft_engine::chess::pdfcraft_chessvision::doc::FORMAT_VERSION, pages: Vec::new(), games: Vec::new() };
            ctl.progress(0, pages);
            for block in all.chunks(BLOCK) {
                // Stopped: the job was dropped, and what it says now is never seen.
                if ctl.cancelled() {
                    return JobDone::Linked(Err(String::new()));
                }
                match s.chess_document_with(id, block, &settings, None, &options) {
                    Ok(read) => doc.pages.extend(read.doc.pages),
                    Err(e) => return JobDone::Linked(Err(e)),
                }
                ctl.progress(doc.pages.len(), pages);
            }
            let aligned = reference::align(&doc, &index);
            JobDone::Linked(Ok(crate::chess_reference_ui::BookLink::new(&aligned, &index, pages)))
        });
    }

    /// Chess ▸ Make the book's bookmarks… (plan A9-N6): read every page of the active document as
    /// the export does, off the interface thread (the diagrams a block of pages at a time, so the
    /// panel says how far it is), then show the bookmarks it gives before they are made
    /// ([`crate::chess_bookmarks_ui`]).
    pub(crate) fn read_book_for_bookmarks(&mut self) {
        /// Pages read at a time.
        const BLOCK: usize = 16;
        let Some((_, id)) = self.active_ids() else { return };
        if self.chess.long.is_some() {
            return self.inform(tl!("A chess document is already being read or exported").to_string());
        }
        if !self.may_copy(id) {
            return;
        }
        let (Some(stamp), Some(pages)) = (self.session.chess_stamp(id), self.session.get(id).map(|d| d.info.pages.len())) else { return };
        let mode = self.chess_origin(id).ocr_mode();
        let book = self.chess_cipher(id);
        let settings = self.chess_scan_settings();
        self.start_chess_job(id, JobKind::Bookmarks { stamp }, move |s, id, ctl| {
            let options = OcrOptions::new(mode).with_book(book.as_ref());
            let all: Vec<usize> = (0..pages).collect();
            // The diagrams scanned a block at a time, so the panel says how far it is and Stop
            // works between two blocks; the text is then read whole, its games across pages.
            ctl.progress(0, pages);
            let mut done: usize = 0;
            for block in all.chunks(BLOCK) {
                // Stopped: the job was dropped, and what it says now is never seen.
                if ctl.cancelled() {
                    return JobDone::Outline(Err(String::new()));
                }
                if let Err(e) = s.chess_scan(id, block, &settings, true) {
                    return JobDone::Outline(Err(e));
                }
                done = done.saturating_add(block.len());
                ctl.progress(done, pages);
            }
            JobDone::Outline(s.chess_document_with(id, &all, &settings, None, &options))
        });
    }

    /// Where document `id`'s pages came from, as the user said (Auto when not said).
    pub fn chess_origin(&self, id: DocId) -> BookOrigin {
        self.session.chess_origin(id)
    }

    /// Run `work` on a session of its own, opened from document `id`'s working bytes, on a
    /// worker thread (inline on the web and in tests); [`Self::poll_chess_job`] picks it up. A
    /// long job ([`JobKind::is_long`]) waits for none running; a page read takes the place of the
    /// one running, which is dropped (plan A9-I2).
    fn start_chess_job(
        &mut self,
        id: DocId,
        kind: JobKind,
        work: impl FnOnce(&mut pdfcraft_engine::Session, DocId, &crate::chess_job::JobCtl) -> JobDone + Send + 'static,
    ) {
        let long = kind.is_long();
        if long && self.chess.long.is_some() {
            return self.inform(tl!("A chess document is already being read or exported").to_string());
        }
        // On the web the whole book is read inside one frame, the tab frozen until the end: only
        // a short one, until it is read a page at a time as the scan is (plan A9-I2, R8-03).
        let pages = self.session.get(id).map_or(0, |d| d.info.pages.len());
        if long && self.chess_frame_budget().is_some() && pages > WEB_WHOLE_BOOK_PAGES {
            let n = WEB_WHOLE_BOOK_PAGES.to_string();
            return self.inform(crate::i18n::fmt(
                tl!("In the browser, a book of more than {n} pages can't be exported or aligned to its PGN yet: the tab would stop responding until the end. The desktop app does it in the background."),
                &[("n", &n)],
            ));
        }
        let stamp = self.session.chess_stamp(id);
        let Some(source) = self.chess_job_source(id) else { return };
        let job = crate::chess_job::Job::spawn("pdfcraft-chess-job", self.run_inline, move |ctl| {
            let (mut s, job_id) = source.open()?;
            let done = work(&mut s, job_id, ctl);
            Ok((done, JobRead { cached: s.chess_cached(job_id, &[]), scanned: s.chess_scanned() }))
        });
        let job = Some(ChessJob { doc: id, kind, stamp, job });
        if long {
            self.chess.long = job;
        } else {
            self.chess.reading = job;
        }
        self.poll_chess_job();
    }

    /// What a job needs to read document `id` in a session of its own: its working bytes, and the
    /// diagram corrections and what this session read for it as it is, so the job reads none of
    /// it again (plan A9, G11). `None` for a document that is not open.
    pub(crate) fn chess_job_source(&mut self, id: DocId) -> Option<JobSource> {
        let corrections = self.chess_corrections_file(id);
        let cached = self.session.chess_cached(id, &[]);
        let doc = self.session.get(id)?;
        Some(JobSource { name: doc.name.clone(), bytes: doc.bytes.clone(), password: doc.password.clone(), corrections, cached })
    }

    /// Stop the long job running (Export, Align the book; plan A9-I2): dropped at once, so
    /// nothing it did is written or shown, and said.
    pub fn stop_chess_job(&mut self) {
        let Some(job) = self.chess.long.take() else { return };
        let said = match job.kind {
            JobKind::LinkBook { .. } => tl!("Aligning was stopped: the book's link is as it was."),
            JobKind::Bookmarks { .. } => tl!("Reading was stopped: no bookmark was added."),
            _ => tl!("The export was stopped: nothing was written."),
        };
        // Dropping it asks its work to stop; its thread ends at its next block of pages.
        drop(job);
        self.inform(said.to_owned());
    }

    /// Pick up a finished job: show the page read, or write the export.
    pub fn poll_chess_job(&mut self) {
        self.poll_chess_slot(false);
        self.poll_chess_slot(true);
    }

    /// Pick up the page read's job, or (`long`) the long job's, when it is done.
    fn poll_chess_slot(&mut self, long: bool) {
        let slot = if long { &mut self.chess.long } else { &mut self.chess.reading };
        let Some(out) = slot.as_mut().map(|j| j.job.poll()) else { return };
        let Some(out) = out else {
            if let Some(ctx) = &self.ctx {
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            }
            return;
        };
        let Some(job) = (if long { self.chess.long.take() } else { self.chess.reading.take() }) else { return };
        let done = match out {
            Ok((done, read)) => {
                // What the job read is this session's too, while the document is in the state it read.
                self.chess.job_scanned = Some(read.scanned);
                if job.stamp.is_some() && self.session.chess_stamp(job.doc) == job.stamp {
                    self.session.prime_chess_cache(job.doc, read.cached);
                }
                done
            }
            // The document could not be opened again, or the work panicked (said with its message).
            Err(why) => job.kind.failed(why),
        };
        match (job.kind, done) {
            (JobKind::Read { page, stamp, origin, ocr }, JobDone::Read(read)) => {
                // The document changed meanwhile: the reading describes a state that is gone.
                if self.session.chess_stamp(job.doc) != Some(stamp) {
                    return;
                }
                let empty =
                    || ChessDoc { format: pdfcraft_engine::chess::pdfcraft_chessvision::doc::FORMAT_VERSION, pages: Vec::new(), games: Vec::new() };
                let text = match read {
                    Ok(read) => {
                        self.keep_chess_cipher(job.doc, read.cipher.clone());
                        let pieces = self.chess.role_pieces(pdfcraft_engine::chess::pdfcraft_chessvision::pieces::choice::Role::Diagrams);
                        let boards = self.ctx.as_ref().map(|ctx| boards(ctx, &read.doc, &pieces)).unwrap_or_default();
                        let needs_ocr = read.needs_ocr.first().map(|(_, why)| why.clone());
                        // The engine's notes, said here (plan A9-I5).
                        let no_model = read.notes.iter().any(|(_, n)| *n == pdfcraft_engine::chess::ScanNote::OcrNeedsGlyphModel);
                        let mut notes: Vec<String> = read.notes.iter().map(|(_, n)| crate::chess_reasons::reason_text(n)).collect();
                        if ocr && read.ocr_pages.is_empty() && !no_model {
                            notes.push(tl!("Chess OCR read nothing on this page.").to_string());
                        }
                        // The choices made on the page go to its moves as read now (plan A9-L1b).
                        if let Some(read_page) = read.doc.pages.first() {
                            self.rematch_book_decisions(job.doc, read_page);
                        }
                        let label = self.page_label_in(job.doc, page);
                        PageText {
                            doc: job.doc,
                            stamp,
                            page,
                            origin,
                            chess: read.doc,
                            ocr: !read.ocr_pages.is_empty(),
                            needs_ocr,
                            notes,
                            boards,
                            label,
                        }
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
                        label: self.page_label_in(job.doc, page),
                    },
                };
                self.chess.doc_mut(job.doc).text = Some(text);
            }
            (JobKind::Game { stamp, .. }, JobDone::Game(Ok(read))) => {
                // The document changed meanwhile: the move may be gone.
                if self.session.chess_stamp(job.doc) != Some(stamp) {
                    return;
                }
                self.keep_chess_cipher(job.doc, read.cipher);
                match read.span.at {
                    Some(node) => self.put_book_game(job.doc, &read.span.doc, node),
                    None => self.inform(tl!("No game was found for this move, on its page or the pages around it.").to_string()),
                }
            }
            // A sheet refused (no exercise on the pages read, plan A9-N12b) is said in the window
            // the export ends with, with the engine's reason.
            (JobKind::Export { format: ExportFormat::Sheet, .. }, JobDone::Export(Err(why))) if !why.is_empty() => {
                self.settle_error("export");
                let said = tl!("No exercise sheet was written").to_owned();
                self.chess.export_done = Some(crate::chess_export_ui::Done { path: None, said, notes: vec![why] });
            }
            #[cfg(not(target_arch = "wasm32"))]
            (JobKind::Export { format, path, image_dir }, JobDone::Export(made)) => {
                if let Ok(m) = &made {
                    self.keep_chess_cipher(job.doc, m.read.cipher.clone());
                }
                // A PDF may have been opened at the sheet's path while the job ran (plan
                // A9-N12b2): asked again just before the file is written.
                if format == ExportFormat::Sheet && self.is_open_file(&path) {
                    return self.refuse_sheet_over_open_file();
                }
                self.write_chess_export(made, &path, &image_dir);
            }
            #[cfg(target_arch = "wasm32")]
            (JobKind::Export { name, .. }, JobDone::Export(made)) => match made {
                Ok(made) => match crate::editing::download(&name, &made.bytes) {
                    Ok(()) => {
                        let said = crate::i18n::fmt(tl!("Exported to {path}"), &[("path", &name)]);
                        self.chess_export_done(&made, None, said);
                    }
                    Err(e) => self.post_error_for("export", e.to_string()),
                },
                Err(e) => self.post_error_for("export", e),
            },
            (JobKind::LinkBook { stamp }, JobDone::Linked(linked)) => {
                // The document changed meanwhile: the pages linked are of a state that is gone.
                if self.session.chess_stamp(job.doc) != Some(stamp) {
                    return;
                }
                match linked {
                    Ok(link) => {
                        let said = crate::i18n::fmt(
                            tl!("The book is linked to its PGN: {linked} of {moves} moves ({recovered} read from the PGN), {conflict} in conflict."),
                            &[
                                ("linked", &crate::i18n::number(link.linked())),
                                ("moves", &crate::i18n::number(link.moves())),
                                ("recovered", &crate::i18n::number(link.counts.recovered)),
                                ("conflict", &crate::i18n::number(link.counts.conflict)),
                            ],
                        );
                        let mut link = link;
                        for g in &mut link.games {
                            g.first_label = g.first_page.map(|p| self.page_label_in(job.doc, p));
                        }
                        // With Undo, which puts back what the book was linked to (plan A9-I17).
                        let view = self.chess.doc_mut(job.doc);
                        view.link_before = Some(view.book_link.replace(link));
                        self.post_notice(said, Some((tl!("Undo").to_owned(), crate::notice::NoticeAction::UndoAlign { doc: job.doc })));
                    }
                    Err(e) => self.post_error(e),
                }
            }
            (JobKind::Bookmarks { stamp }, JobDone::Outline(read)) => {
                // The document changed meanwhile: its pages are not the ones read.
                if self.session.chess_stamp(job.doc) != Some(stamp) {
                    return;
                }
                match read {
                    // What the reading taught of the book's cipher is kept, as Export and Align
                    // keep theirs (plan A9-G3c).
                    Ok(read) => {
                        self.keep_chess_cipher(job.doc, read.cipher);
                        self.preview_book_bookmarks(job.doc, read.doc);
                    }
                    Err(e) => self.post_error(e),
                }
            }
            (_, JobDone::Export(Err(e))) => self.post_error_for("export", e),
            (_, JobDone::Read(Err(e)) | JobDone::Game(Err(e))) => self.post_error(e),
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
            // The text's page, scanned first when the list doesn't hold it, whatever page is in view
            // (plan A9-I4, finding A5-04).
            TextAction::Diagram(at) => {
                let Some((_, id)) = self.active_ids() else { return };
                self.pick_after_scan(id, at);
            }
            TextAction::OpenGame(at) => self.read_book_game(at),
            TextAction::Stop => self.stop_chess_job(),
            TextAction::OpenExport => self.open_chess_export(),
            // The page shown is read again under the new origin (the tab sees it is not current).
            TextAction::Origin(origin) => {
                if let Some((_, id)) = self.active_ids() {
                    self.session.set_chess_origin(id, origin);
                }
            }
            TextAction::BookPgn(true) => self.load_book_pgn(),
            TextAction::BookPgn(false) => self.clear_book_pgn(),
            TextAction::SiblingPgn => self.use_sibling_pgn(),
            TextAction::LinkBook => self.link_whole_book(),
            TextAction::Decide { at, san, san_read, choice } => self.decide_book_move(at, san, san_read, choice),
            TextAction::PgnGameAt { game, page } => {
                let Some((vi, id)) = self.active_ids() else { return };
                if let Some(view) = page.and_then(|p| self.views.get_mut(vi).map(|v| (v, p))) {
                    view.0.go_to_page(view.1);
                }
                self.put_book_pgn_game(id, game, 0);
            }
            TextAction::OpenPgnGame { game, node } => {
                if let Some((_, id)) = self.active_ids() {
                    self.put_book_pgn_game(id, game, node);
                }
            }
        }
    }

    /// Chess ▸ Export, once the export dialog said what (plan A9-X2, where the diagrams still to
    /// review are said): ask about unconfirmed changes, then [`Self::export_chess_save`].
    pub fn export_chess_dialog(&mut self, format: impl Into<crate::chess_export_ui::ExportFormat>) {
        let format = format.into();
        let Some((_, id)) = self.active_ids() else { return };
        // Diagrams with unconfirmed changes are asked about first (plan A8, P3b3).
        if self.ask_chess_drafts(Some(id), crate::chess_drafts::AfterDrafts::Export(format)) {
            return;
        }
        self.export_chess_save(id, format);
    }

    /// The book's PGN a `format` export of document `id` is linked to: the one the document
    /// keeps, in every format (plan A9-N2: it reached the PGN alone); for the PGN of games,
    /// unless the export dialog says the text read (plan "Carregar PGN do livro", R6b).
    pub(crate) fn export_reference(
        &mut self,
        id: DocId,
        format: crate::chess_export_ui::ExportFormat,
    ) -> Option<Arc<pdfcraft_engine::chess::pdfcraft_chessvision::doc::reference::ReferenceIndex>> {
        if format == crate::chess_export_ui::ExportFormat::Pgn && self.chess.export.source != crate::chess_export_ui::PgnSource::Book {
            return None;
        }
        self.book_pgn_index(id)
    }

    /// Document `id`'s own games as one PGN, which an export of it is linked to first (plan
    /// A9-N2, §2.9: the user's games outrank the book's PGN, and the reading comes last), or
    /// nothing when none of them has a move. Read into an index by the export's job, off the
    /// interface's thread ([`export_with_games`]).
    pub(crate) fn export_games(&self, id: DocId) -> Option<String> {
        let games = self.chess.games.games_of(id);
        let text: Vec<String> = games.iter().filter(|b| b.illegal.is_none() && !b.game.is_empty()).map(|b| b.game.pgn()).collect();
        (!text.is_empty()).then(|| text.join("\n"))
    }

    /// The options an export of document `id` as `export` reads and writes with, titled `stem`:
    /// the scan the app made, so the job finds what it read (plan A9, G11), the diagrams' pieces,
    /// the book's cipher and origin, and what the export dialog chose (plan A9-X2).
    pub(crate) fn chess_export_base(&mut self, id: DocId, export: ExportFormat, stem: &str) -> ChessExportOptions {
        let mut options = ChessExportOptions::new(stem, pdfcraft_platform::app_dirs::AppDirs::current().install());
        options.scan = self.chess_scan_settings();
        options.pieces = self.chess.role_pieces(pdfcraft_engine::chess::pdfcraft_chessvision::pieces::choice::Role::Diagrams);
        options.book = self.chess_cipher(id);
        options.ocr = self.chess_origin(id).ocr_mode();
        self.chess_export_options(id, export, &mut options);
        options
    }

    /// Ask where (`save_override` in tests), then read document `id` and write it as `export`.
    pub(crate) fn export_chess_save(&mut self, id: DocId, export: ExportFormat) {
        if !self.may_copy(id) {
            return;
        }
        let Some(doc) = self.session.get(id) else { return };
        let stem = doc.name.trim_end_matches(".pdf").trim_end_matches(".PDF").to_string();
        #[cfg(not(target_arch = "wasm32"))]
        {
            let write = move |app: &mut Self, path: std::path::PathBuf| app.export_chess_to(id, export, &path);
            match self.save_override.clone() {
                Some(p) => write(self, p.into()),
                None => {
                    let dialog = rfd::AsyncFileDialog::new()
                        .set_title(tl!("Export").to_string())
                        .add_filter(export.name(), &[export.extension()])
                        .set_file_name(export.file_name(&stem));
                    self.ask_one(crate::pickers::Ask::Save(dialog), None, write);
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let options = self.chess_export_base(id, export, &stem);
            let games = self.export_games(id);
            let sheet = self.chess.export.sheet.options();
            self.start_chess_job(id, JobKind::Export { format: export, name: export.file_name(&stem) }, move |s, id, ctl| {
                JobDone::Export(export_with_games(s, id, export, options, sheet, games.as_deref(), ctl))
            });
        }
    }

    /// Whether `path` is the file of a document open in this session: an exercise sheet, a PDF,
    /// is never written over one (plan A9-N12b; the engine's tool refuses the same, G7a).
    #[cfg(not(target_arch = "wasm32"))]
    fn is_open_file(&self, path: &std::path::Path) -> bool {
        let same = |a: &std::path::Path, b: &std::path::Path| match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
            (Ok(a), Ok(b)) => a == b,
            _ => a == b,
        };
        self.session.docs().iter().filter_map(|d| d.path.as_deref()).any(|open| same(std::path::Path::new(open), path))
    }

    /// Say that the exercise sheet is not written over a PDF that is open (plan A9-N12b), when the
    /// export starts and again just before it writes (A9-N12b2).
    #[cfg(not(target_arch = "wasm32"))]
    fn refuse_sheet_over_open_file(&mut self) {
        self.post_error_for("export", tl!("The exercise sheet can't be written over a PDF that is open: choose another name.").to_string());
    }

    /// Write the chess export of document `id` to `path` (Markdown's images in a folder beside it).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn export_chess_to(&mut self, id: DocId, export: impl Into<ExportFormat>, path: &std::path::Path) {
        let export = export.into();
        if self.chess.long.is_some() {
            return self.inform(tl!("A chess document is already being read or exported").to_string());
        }
        if export == ExportFormat::Sheet && self.is_open_file(path) {
            return self.refuse_sheet_over_open_file();
        }
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "document".into());
        let mut options = self.chess_export_base(id, export, &stem);
        options.image_dir = pdfcraft_engine::chess::pdfcraft_chessvision::doc::markdown::image_folder(&stem);
        let games = self.export_games(id);
        let sheet = self.chess.export.sheet.options();
        let kind = JobKind::Export { format: export, path: path.to_path_buf(), image_dir: options.image_dir.clone() };
        self.inform(tl!("Exporting the chess document…").to_string());
        self.start_chess_job(id, kind, move |s, id, ctl| JobDone::Export(export_with_games(s, id, export, options, sheet, games.as_deref(), ctl)));
    }

    /// Write a finished export to `path`, Markdown's images in `image_dir` beside it.
    #[cfg(not(target_arch = "wasm32"))]
    fn write_chess_export(&mut self, made: Result<pdfcraft_engine::chess::export::ChessExport, String>, path: &std::path::Path, image_dir: &str) {
        let made = match made {
            Ok(m) => m,
            Err(e) => return self.post_error_for("export", e),
        };
        if !made.images.is_empty() {
            let dir = path.with_file_name(image_dir);
            if let Err(e) = std::fs::create_dir_all(&dir) {
                return self.post_error_fmt_for(
                    "export",
                    "Couldn't write {name}: {e}",
                    &[("name", &dir.display().to_string()), ("e", &e.to_string())],
                );
            }
            for (rel, png) in &made.images {
                let Some(name) = std::path::Path::new(rel).file_name() else { continue };
                let target = dir.join(name);
                if let Err(e) = crate::editing::write_atomically(&target.to_string_lossy(), png) {
                    return self.post_error_fmt_for(
                        "export",
                        "Couldn't write {name}: {e}",
                        &[("name", &target.display().to_string()), ("e", &e.to_string())],
                    );
                }
            }
        }
        match crate::editing::write_atomically(&path.to_string_lossy(), &made.bytes) {
            Ok(()) => {
                let said = crate::i18n::fmt(tl!("Exported to {path}"), &[("path", &path.display().to_string())]);
                self.chess_export_done(&made, Some(path.to_path_buf()), said);
            }
            Err(e) => {
                self.post_error_fmt_for("export", "Couldn't write {name}: {e}", &[("name", &path.display().to_string()), ("e", &e.to_string())])
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    use super::*;

    /// Plan A9-I14 (finding A6-26): while the page is read, the tab says so once, as its heading
    /// with the spinner; with a page shown, the work under way is a row of its own.
    #[test]
    fn reading_the_page_is_said_once() {
        let mut h = Harness::builder().with_size(egui::vec2(420.0, 240.0)).build_ui(|ui| {
            let t = Tokens::get(ui.ctx());
            let pgn = crate::chess_reference_ui::BookPgnView::default();
            let _ = text_tab(ui, &t, None, DocId(1), 0, None, BookOrigin::Auto, Some(tl!("Reading the page…")), None, &pgn);
        });
        // The spinner asks for frames all the time: a few, not until it settles.
        h.run_steps(3);
        assert_eq!(h.get_all_by_label("Reading the page…").count(), 1);
    }
}
