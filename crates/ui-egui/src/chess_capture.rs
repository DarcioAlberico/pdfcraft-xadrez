//! Chess ▸ PGN toolbar ▸ Capture (T7a): text selected on a page, or pasted, goes into the game on
//! the chess board (PGN toolbar spec §4.3).
//!
//! **Triggers** (decision D5: release and paste; Copy only when the user asks for it):
//! - with **Capture** (or **Comment**) armed on the rail, letting go of a text selection on the
//!   page captures it ([`armed`]; the page's drag hook in `canvas.rs`);
//! - **Ctrl+V** while the chess scope is on and no text field has the keyboard captures the
//!   pasted text (the `Paste` event is taken in `raw_input_hook`, [`take_paste`]), and the board
//!   panel's **Paste as ▸** does the same in a mode of the user's choosing;
//! - **Send to PGN ▸** in the selection's action bar and in the page's context menu captures the
//!   selection without arming anything ([`send_menu`]);
//! - with the preference [`CaptureState::on_copy`] (off by default), Ctrl+C on the page also
//!   captures while a capture tool is armed.
//!
//! **Reading.** A page selection is read from its glyphs, not from the text Copy would make: their
//! fonts give figurines and weight. It is first grown to whole tokens ([`snap_to_tokens`]: "20.Nx"
//! becomes "20.Nxf6+"). Pasted text has no weight. The language is the document's (the Text tab's
//! reading of it, else English), never guessed from the selection.
//!
//! **Planning** is `engine::chess::capture::plan`, on a copy of the game. A selection of more than
//! [`INLINE_BYTES`] is planned on a worker thread (inline on the web and in tests). Capturing
//! reads the document's content, so a document that does not allow copying refuses it with the
//! usual notice.
//!
//! **Applying** (decision D6 b): a plan that reads completely and fits one place is applied at once
//! as one undo step ("Capture variation"), with the chess scope on, so Ctrl+Z undoes it even with
//! the pointer still on the page; the moves keep where they were printed (`NodeSource`). Any
//! other plan waits for the user ([`CaptureState::pending`]). A plan made on a game that changed
//! since is made again, once. A move of a waiting plan that did not play is corrected in the
//! preview (T10, [`crate::chess_correct`]).

use std::sync::{Arc, Mutex, MutexGuard};

use egui::{Key, Modifiers, Rect, RichText, pos2};
use pdfcraft_engine::DocId;
use pdfcraft_engine::chess::capture::{self, Candidate, CaptureError, CaptureMode, CapturePlan, CaptureText, PreviewReason, StoppedMove};
use pdfcraft_engine::chess::pdfcraft_chess::fragment::FragmentItem;
use pdfcraft_engine::chess::pdfcraft_chess::game::{Game, GraftMode, GraftStopReason, MAX_COMMENT_LEN, NodeId, NodePath, Token};
use pdfcraft_engine::chess::pdfcraft_chess::san::Lang;
use pdfcraft_engine::chess::pdfcraft_chessvision::coords::PageGeometry;
use pdfcraft_engine::chess::pdfcraft_chessvision::doc::MoveStatus;
use pdfcraft_engine::chess::text_ocr::AreaRead;
use pdfcraft_engine::commands::Shortcut;
use pdfcraft_render::text::{PageText, TextGlyph};

use crate::chess_board_ui::BoardState;
use crate::chess_history::ChessEdit;
use crate::chess_tools::{ChessKey, Group, Kind, PgnTool, Tool};
use crate::chess_ui::ChessState;
use crate::theme::Tokens;
use crate::{PdfCraftApp, icons};

/// A selection or paste of more than this many bytes is planned off the interface thread: about
/// what a plan reads within the 50 ms the spec allows a capture (§8).
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const INLINE_BYTES: usize = 4 << 10;

/// Paste (⌘V), as the notices name it.
const PASTE: Shortcut = Shortcut::cmd("V");

/// Most pastes kept from one frame's input.
const MAX_PASTES: usize = 4;

/// What a capture became, as Undo names it ("Undo Capture variation").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Captured {
    Line,
    Variation,
    SubVariation,
    Comment,
}

impl Captured {
    /// The undo step's name, in the current language.
    pub fn text(self) -> &'static str {
        match self {
            Captured::Line => tl!("Capture line"),
            Captured::Variation => tl!("Capture variation"),
            Captured::SubVariation => tl!("Capture sub-variation"),
            Captured::Comment => tl!("Capture comment"),
        }
    }
}

/// The capture's state, kept in [`ChessState`].
#[derive(Default)]
pub struct CaptureState {
    /// The Comment tool's switches, kept while it is not armed: the text goes before the move
    /// (else after it), and in place of what the comment says (else added to it).
    pub before: bool,
    pub replace: bool,
    /// Ctrl+C on the page also captures while a capture tool is armed (D5 c). Off by default:
    /// copying should not change the game unasked.
    pub on_copy: bool,
    /// The language the user chose for a document's captures, over the document's own.
    pub lang: Option<(DocId, Lang)>,
    /// A plan waiting for the user: it did not read completely or fit only one place.
    pub pending: Option<Pending>,
    /// A plan being made off the interface thread.
    job: Option<Job>,
    /// Text pasted while the chess scope was on, captured at the next tick.
    pasted: Vec<String>,
    /// The mode the next paste is captured in (Paste as ▸, where the clipboard can only be read by
    /// a paste: the web).
    paste_mode: Option<(CaptureMode, bool)>,
    /// A capture tool was just armed: the page tool goes to Select at the next tick.
    select_page: bool,
    /// Underline on the page the text already captured into the document's games (§4.3.5).
    pub show_captured: bool,
    /// What that layer drew in the last frame: page and box in user space (tests and automation
    /// read it; nothing else does).
    pub drawn: Vec<(usize, [f32; 4])>,
    /// The layer's boxes, with a key over the games they were read from.
    layer: Option<(u64, Layer)>,
    /// The notice after a capture went in, with its Undo, until the game changes again.
    pub toast: Option<Toast>,
    /// Enter was pressed while a plan waits: it applies the plan (taken before anything else
    /// that uses Enter can see it).
    enter: bool,
    /// Notices for the user, shown at the next tick.
    notes: Vec<String>,
    /// The notice's Undo was clicked: done at the next tick.
    undo: bool,
    /// The OCR area (T9): drawing an area of a scan to read with chess OCR.
    pub area: crate::chess_ocr_area::AreaState,
    /// Collecting corrected characters for retraining (T11, opt-in).
    pub samples: crate::chess_samples::Samples,
}

impl CaptureState {
    /// Tell the user `text` at the next tick.
    pub(crate) fn note(&mut self, text: String) {
        self.notes.push(text);
    }
}

/// The notice after a capture went in: what it did, and Undo. It stays (no timeout: WCAG 2.2.1)
/// while the game is as the capture left it.
pub struct Toast {
    pub text: String,
    /// The game it is about (the collection's id for it) and its revision after the capture.
    game: u64,
    revision: u64,
}

/// The moves a capture just added, tinted in the notation: for a few seconds, or with reduced
/// motion (no animation) until the game changes again.
#[derive(Clone, Debug, Default)]
pub struct Fresh {
    nodes: Vec<NodeId>,
    revision: u64,
    since: f64,
}

impl Fresh {
    /// How long the tint fades, in seconds.
    const SECONDS: f64 = 3.0;

    /// The moves tinted in `game`: none once it has changed.
    pub fn nodes(&self, game: &Game) -> &[NodeId] {
        if game.revision() == self.revision { &self.nodes } else { &[] }
    }

    /// The tint behind move `node` of `game` now, if any.
    pub(crate) fn tint(&self, game: &Game, node: NodeId, ctx: &egui::Context) -> Option<egui::Color32> {
        if !self.nodes(game).contains(&node) {
            return None;
        }
        let base = Tokens::get(ctx).accent_soft;
        if ctx.global_style().animation_time <= 0.0 {
            return Some(base);
        }
        let age = ctx.input(|i| i.time) - self.since;
        // A NaN age is in no range: no tint.
        if !(0.0..Self::SECONDS).contains(&age) {
            return None;
        }
        ctx.request_repaint();
        Some(base.gamma_multiply((1.0 - age / Self::SECONDS) as f32))
    }
}

/// A plan the user is to look at before it goes in.
pub struct Pending {
    pub(crate) request: Request,
    pub plan: CapturePlan,
    /// The candidate chosen (the plan's best, until the user picks another).
    pub choice: Option<usize>,
    /// The moves of the candidate chosen that did not play, with the legal moves near each
    /// (T10): worked out once per candidate, as each is a replay.
    pub(crate) stopped: Vec<StoppedMove>,
    /// The correction panel of one of them, when its chip was clicked (T10).
    pub(crate) fix: Option<crate::chess_correct::Fix>,
}

impl Pending {
    /// `plan` of `request`, its best candidate chosen.
    pub(crate) fn new(request: Request, plan: CapturePlan) -> Pending {
        let choice = plan.chosen;
        let mut p = Pending { request, plan, choice, stopped: Vec::new(), fix: None };
        p.choose(choice);
        p
    }

    /// Choose candidate `choice`: its stopped moves are worked out, and a correction panel closes.
    fn choose(&mut self, choice: Option<usize>) {
        self.choice = choice;
        self.stopped = candidate(&self.plan, choice).map(|c| capture::stopped_moves(c, self.request.lang)).unwrap_or_default();
        self.fix = None;
    }
}

/// What is captured, how, and into which game.
#[derive(Clone)]
pub(crate) struct Request {
    pub mode: CaptureMode,
    /// A comment goes in place of the one there (else it is added to it).
    pub replace: bool,
    pub text: Text,
    /// The document whose games it goes into (`None`: the games kept without a document).
    pub doc: Option<DocId>,
    pub lang: Lang,
    /// The document's edit generation when a page's text was selected: a preview of it closes
    /// once the document is edited.
    pub generation: Option<u64>,
}

/// The text captured.
#[derive(Clone)]
pub(crate) enum Text {
    /// Glyphs of page `page`, with the page's geometry (the moves keep where they were printed)
    /// and their boxes in view space, one per piece of a line (the preview goes beside the last).
    /// `read`: what chess OCR read in an area of a scan (T9), whose glyphs these are; `None` for
    /// the page's text layer.
    Page { page: usize, glyphs: Arc<Vec<TextGlyph>>, geometry: Option<PageGeometry>, rects: Vec<[f32; 4]>, read: Option<Arc<AreaRead>> },
    /// Pasted text.
    Plain(Arc<str>),
}

impl Text {
    /// Chess OCR changed or doubted words of it (the book's cipher, the repair): the user looks
    /// before it goes in.
    fn review(&self) -> bool {
        matches!(self, Text::Page { read: Some(r), .. } if r.edits > 0 || r.flags > 0)
    }
}

impl Request {
    /// The text's size in bytes.
    #[cfg(not(target_arch = "wasm32"))]
    fn len(&self) -> usize {
        match &self.text {
            Text::Page { glyphs, .. } => glyphs.iter().fold(0usize, |n, g| n.saturating_add(g.text.len())),
            Text::Plain(text) => text.len(),
        }
    }
}

/// How a plan made on a worker thread is getting on (the web plans inline: never done there).
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
enum JobState {
    Running,
    Done(Box<CapturePlan>),
    /// The worker ended without a plan.
    Gone,
}

struct Job {
    request: Request,
    state: Arc<Mutex<JobState>>,
}

/// The job's state, even after a worker panicked while holding it (it is plain data).
fn lock(m: &Mutex<JobState>) -> MutexGuard<'_, JobState> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Marks the job gone when the worker ends without a plan, so the interface never waits for a
/// worker that is gone.
#[cfg(not(target_arch = "wasm32"))]
struct Finish(Arc<Mutex<JobState>>);

#[cfg(not(target_arch = "wasm32"))]
impl Drop for Finish {
    fn drop(&mut self) {
        let mut s = lock(&self.0);
        if matches!(*s, JobState::Running) {
            *s = JobState::Gone;
        }
    }
}

// ─── The rail ────────────────────────────────────────────────────────────────────────────────

fn can_capture(board: &BoardState) -> bool {
    board.illegal.is_none()
}

const fn capture_tool(
    id: &'static str,
    icon: &'static str,
    label: fn(&BoardState) -> String,
    keys: &'static [ChessKey],
    command: &'static str,
    mode: CaptureMode,
) -> Tool {
    Tool {
        id,
        icon,
        label,
        keys,
        command: Some(command),
        kind: Kind::Mode(PgnTool::Capture(mode)),
        enabled: can_capture,
        visible: |_| true,
        heads: None,
        on_move: None,
        on: None,
    }
}

/// The Capture group: automatic (X), and the forced modes; Shift+X arms the next.
pub(crate) const CAPTURE: Group = Group {
    id: "capture",
    label: || tl!("Capture"),
    letter: Some(Key::X),
    members: &[
        capture_tool(
            "capture",
            "wand-sparkles",
            |_| tl!("Capture (automatic)").to_owned(),
            &[ChessKey::plain(Key::X)],
            "chess.tool.capture",
            CaptureMode::Auto,
        ),
        capture_tool("capture-line", "list-ordered", |_| tl!("Capture as the line").to_owned(), &[], "chess.tool.capture_line", CaptureMode::Line),
        capture_tool(
            "capture-variation",
            "git-compare",
            |_| tl!("Capture as a variation").to_owned(),
            &[],
            "chess.tool.capture_variation",
            CaptureMode::Variation,
        ),
        capture_tool(
            "capture-subvariation",
            "spline",
            |_| tl!("Capture as a sub-variation").to_owned(),
            &[],
            "chess.tool.capture_subvariation",
            CaptureMode::SubVariation,
        ),
    ],
};

/// Comment (C): the text selected becomes the move's comment. (The rail's Move comment, T6, is
/// the comment editor; this one captures from the page.)
pub(crate) const COMMENT: Tool = Tool {
    id: "capture-comment",
    icon: "message-square-text",
    label: |_| tl!("Capture as a comment").to_owned(),
    keys: &[ChessKey::plain(Key::C)],
    command: Some("chess.tool.comment"),
    kind: Kind::Mode(PgnTool::Comment { before: false, replace: false }),
    enabled: can_capture,
    visible: |_| true,
    heads: None,
    on_move: None,
    on: None,
};

/// How the armed tool captures, with whether a comment replaces the one there; `None` when no
/// capture tool is armed.
pub(crate) fn armed(chess: &ChessState) -> Option<(CaptureMode, bool)> {
    mode_of(chess.tools.tool)
}

fn mode_of(tool: PgnTool) -> Option<(CaptureMode, bool)> {
    match tool {
        // Setting up a position (T5c) and the glyph brush (T15) capture nothing.
        PgnTool::Navigate | PgnTool::Setup | PgnTool::GlyphBrush(_) => None,
        PgnTool::Capture(mode) => Some((mode, false)),
        PgnTool::Comment { before, replace } => Some((CaptureMode::Comment { before }, replace)),
    }
}

/// Arm `tool` (from the rail, a key or a command). A capture tool turns the chess scope on (Undo
/// goes to the game, and the keys of the rail work with the pointer on the page) and the page's
/// tool to Select; Comment keeps the switches it had.
pub(crate) fn arm(chess: &mut ChessState, tool: PgnTool) {
    let tool = match tool {
        PgnTool::Comment { .. } => PgnTool::Comment { before: chess.capture.before, replace: chess.capture.replace },
        other => other,
    };
    chess.tools.tool = tool;
    if mode_of(tool).is_some() {
        chess.enter_scope();
        chess.capture.select_page = true;
    }
}

/// Esc, first part (§4.2: the preview closes, then the armed tool goes back to Navigate). Whether
/// it closed the preview.
pub(crate) fn escape_preview(chess: &mut ChessState) -> bool {
    // An area being read (T9) is dropped first.
    crate::chess_ocr_area::cancel(chess) || chess.capture.pending.take().is_some()
}

/// Take the `Paste` events out of a frame's `events` for the chess game: the chess scope is on,
/// or the chess board's own window has the keyboard, and no text field has it (the caller checks).
pub(crate) fn take_paste(chess: &mut ChessState, events: &mut Vec<egui::Event>) {
    let pasted = &mut chess.capture.pasted;
    events.retain(|event| match event {
        egui::Event::Paste(text) => {
            if pasted.len() < MAX_PASTES {
                pasted.push(text.clone());
            }
            false
        }
        _ => true,
    });
}

// ─── Selection and language ──────────────────────────────────────────────────────────────────

/// `glyphs` (indices into `text`, ascending) grown to whole tokens: a selection that starts or
/// ends inside a word ("20.Nx") takes all of it ("20.Nxf6+"). Indices outside the page are
/// dropped; the result is ascending, each glyph once.
pub(crate) fn snap_to_tokens(text: &PageText, glyphs: &[usize]) -> Vec<usize> {
    let count = text.glyphs.len();
    // Runs of consecutive glyphs: each grows at its ends only.
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for g in glyphs.iter().copied().filter(|&g| g < count) {
        match runs.last_mut() {
            Some((_, end)) if end.checked_add(1) == Some(g) => *end = g,
            Some((_, end)) if g <= *end => {}
            _ => runs.push((g, g)),
        }
    }
    let mut out: Vec<usize> = Vec::with_capacity(glyphs.len());
    for (first, last) in runs {
        let first = text.word_at(first).map_or(first, |(f, _)| f);
        let last = text.word_at(last).map_or(last, |(_, l)| l);
        for g in first..=last {
            if out.last().is_none_or(|&prev| g > prev) {
                out.push(g);
            }
        }
    }
    out
}

/// The language captures into document `doc` are read in: the user's choice for it, else what
/// the Text tab read on its page, else English. Never guessed from the selection (a short one
/// can't tell "Rc7", a rook in English, from a king in Portuguese).
pub(crate) fn document_lang(chess: &ChessState, doc: Option<DocId>) -> Lang {
    if let (Some((chosen, lang)), Some(doc)) = (chess.capture.lang, doc)
        && chosen == doc
    {
        return lang;
    }
    chess
        .text
        .as_ref()
        .filter(|t| Some(t.doc) == doc)
        .and_then(|t| t.chess.pages.first())
        .and_then(|p| Lang::ALL.iter().copied().find(|l| l.code() == p.lang))
        .unwrap_or(Lang::En)
}

/// Plan `request` on `game`, after the move it shows.
pub(crate) fn plan_for(game: &Game, request: &Request) -> CapturePlan {
    let current = game.current();
    let text = match &request.text {
        Text::Page { page, glyphs, geometry, read, .. } => {
            let (geometry, page) = (*geometry, u32::try_from(*page).unwrap_or(u32::MAX));
            // A scan has no weight: OCR'd text is read by numbers only, with the reader's words.
            match read {
                Some(read) => CaptureText::Read { text: &read.text, geometry, page },
                None => CaptureText::Glyphs { glyphs: glyphs.as_slice(), geometry, page },
            }
        }
        Text::Plain(text) => CaptureText::Plain(text),
    };
    if let (true, CaptureMode::Comment { before }) = (request.replace, request.mode) {
        // In place of the comment there: planned on the game without it, which is otherwise the
        // same game, so the plan stands for this one.
        let mut cleared = game.clone();
        if before {
            cleared.set_comment_before(current, "");
        } else {
            cleared.set_comment(current, "");
        }
        let mut plan = capture::plan(&cleared, current, request.mode, text, request.lang);
        plan.base_revision = game.revision();
        return plan;
    }
    capture::plan(game, current, request.mode, text, request.lang)
}

/// Candidate `choice` of `plan`, its best when `None`.
fn candidate(plan: &CapturePlan, choice: Option<usize>) -> Option<&Candidate> {
    choice.or(plan.chosen).and_then(|i| plan.candidates.get(i))
}

/// What candidate `choice` of `plan` (its best when `None`) makes of the game.
fn captured(plan: &CapturePlan, choice: Option<usize>, mode: CaptureMode) -> Captured {
    candidate(plan, choice).map_or(Captured::Variation, |c| captured_by(c, mode))
}

/// What candidate `c` makes of the game, captured in `mode`.
fn captured_by(c: &Candidate, mode: CaptureMode) -> Captured {
    let first_reply = c.report.new_nodes.first().is_some_and(|p| p.indices().last() == Some(&0));
    match c.mode_used {
        GraftMode::Comment => Captured::Comment,
        _ if first_reply || (c.report.new_nodes.is_empty() && c.mode_used == GraftMode::Line) => Captured::Line,
        _ if mode == CaptureMode::SubVariation => Captured::SubVariation,
        _ => Captured::Variation,
    }
}

/// The move before the one at `path` (`None` for the start).
fn parent_path(path: &NodePath) -> Option<NodePath> {
    let (_, up) = path.indices().split_last()?;
    Some(NodePath::new(up.to_vec()))
}

/// What a capture did, in words: "Variation of 18…Qxe1 · moves: 4".
pub(crate) fn summary(plan: &CapturePlan, choice: Option<usize>, kind: Captured) -> String {
    let Some(c) = candidate(plan, choice) else { return String::new() };
    let what = headline(c, kind);
    if kind == Captured::Comment {
        return what;
    }
    format!("{what} · {}", crate::i18n::fmt(tl!("moves: {n}"), &[("n", &c.report.played.to_string())]))
}

/// Where candidate `c` puts the text, in words: "Variation of 18…Qxe1", "Line after 17…Bc4",
/// "Comment on 20.Nxf6+".
pub(crate) fn headline(c: &Candidate, kind: Captured) -> String {
    let g = &c.game;
    let name = |path: &NodePath| g.node_at(path).filter(|&n| n != 0).map(|n| crate::chess_tools::move_name(g, n));
    let first = c.report.new_nodes.first();
    match kind {
        Captured::Comment => match name(&c.report.anchor) {
            Some(mv) => crate::i18n::fmt(tl!("Comment on {move}"), &[("move", &mv)]),
            None => tl!("Comment at the start").to_owned(),
        },
        Captured::Line => match first.and_then(parent_path).or_else(|| Some(c.report.anchor.clone())).and_then(|p| name(&p)) {
            Some(mv) => crate::i18n::fmt(tl!("Line after {move}"), &[("move", &mv)]),
            None => tl!("Line from the start").to_owned(),
        },
        Captured::Variation | Captured::SubVariation => {
            // The move the new one is an alternative to: the first reply at the same place.
            let main = first.map(|p| {
                let mut i = p.indices().to_vec();
                if let Some(last) = i.last_mut() {
                    *last = 0;
                }
                NodePath::new(i)
            });
            match main.and_then(|p| name(&p)) {
                Some(mv) => crate::i18n::fmt(tl!("Variation of {move}"), &[("move", &mv)]),
                None => kind.text().to_owned(),
            }
        }
    }
}

/// Why a plan is shown before it goes in, in words.
pub(crate) fn reason_text(reason: PreviewReason) -> &'static str {
    match reason {
        PreviewReason::Ambiguous => tl!("It fits in more than one place."),
        PreviewReason::Incomplete => tl!("Part of the text wasn't read."),
        PreviewReason::OuterLine => tl!("It goes on a line outside the one of the move shown."),
        PreviewReason::TipExtension => tl!("It extends the end of a line instead of branching from it."),
        PreviewReason::Renumber => tl!("The start is renumbered to the text's first move."),
        PreviewReason::ImpliedClose => tl!("A closing bracket the text lacks was assumed."),
        PreviewReason::Demotes => tl!("A move already there becomes a variation."),
        PreviewReason::NoFit => tl!("No move of the text fits the game here."),
        PreviewReason::TooLong => tl!("The text is too long to capture."),
    }
}

// ─── The app ─────────────────────────────────────────────────────────────────────────────────

impl PdfCraftApp {
    /// Capture the text selected in view `vi`: in `mode` (with whether a comment replaces the one
    /// there), or as the armed tool says. Nothing happens without a selection or a mode; a
    /// document that does not allow copying refuses it with the usual notice.
    pub fn capture_selection(&mut self, vi: usize, mode: Option<(CaptureMode, bool)>) {
        let Some((mode, replace)) = mode.map(|m| with_switches(&self.chess, m)).or_else(|| armed(&self.chess)) else { return };
        let Some(view) = self.views.get(vi) else { return };
        let id = view.id;
        let Some((page, selected)) = view.selection_glyphs() else { return };
        let Some(text) = view.texts.get(&page).cloned() else { return };
        let snapped = snap_to_tokens(&text, &selected);
        if snapped.iter().all(|&i| text.glyphs.get(i).is_none_or(|g| g.text.trim().is_empty())) {
            return;
        }
        // Capturing reads the document's content: its copy permission applies.
        if !self.may_copy(id) {
            return;
        }
        let geometry = self.session.get(id).and_then(|d| d.info.pages.get(page)).and_then(pdfcraft_engine::chess::geometry);
        let glyphs: Vec<TextGlyph> = snapped.iter().filter_map(|&i| text.glyphs.get(i).cloned()).collect();
        let rects = text.glyph_rects(&snapped);
        let generation = self.session.get(id).map(pdfcraft_engine::Document::edit_generation);
        self.sync_chess_docs();
        let lang = document_lang(&self.chess, Some(id));
        let doc = self.chess.games.current_doc();
        let text = Text::Page { page, glyphs: Arc::new(glyphs), geometry, rects, read: None };
        self.start_capture(Request { mode, replace, text, doc, lang, generation });
    }

    /// Capture pasted `text`: in `mode`, else as the armed tool says, else automatically.
    pub fn capture_pasted(&mut self, text: &str, mode: Option<(CaptureMode, bool)>) {
        if text.trim().is_empty() {
            return;
        }
        let (mode, replace) = mode.map(|m| with_switches(&self.chess, m)).or_else(|| armed(&self.chess)).unwrap_or((CaptureMode::Auto, false));
        self.sync_chess_docs();
        let doc = self.chess.games.current_doc();
        let lang = document_lang(&self.chess, doc);
        self.start_capture(Request { mode, replace, text: Text::Plain(Arc::from(text)), doc, lang, generation: None });
    }

    /// Paste as ▸ `mode`: capture the clipboard's text. `text` is the clipboard's, when it can be
    /// read now (the desktop); otherwise the next paste is captured in `mode`.
    pub fn chess_paste_as(&mut self, mode: (CaptureMode, bool), text: Option<String>) {
        match text {
            Some(text) => self.capture_pasted(&text, Some(mode)),
            None => {
                self.chess.capture.paste_mode = Some(mode);
                self.chess.enter_scope();
                let key = self.ctx.as_ref().map(|ctx| crate::commands::shortcut_label(ctx, PASTE)).unwrap_or_default();
                self.notify(crate::i18n::fmt(tl!("Press {key} to capture the text on the clipboard."), &[("key", &key)]));
            }
        }
    }

    /// Plan `request` (off the interface thread when it is long), then apply it or keep it for the
    /// user to look at.
    pub(crate) fn start_capture(&mut self, request: Request) {
        if self.chess.board().illegal.is_some() {
            self.notify(tl!("The board shows an illegal position: correct it before capturing moves into it.").to_owned());
            return;
        }
        // A new capture replaces the one waiting.
        self.chess.capture.pending = None;
        self.chess.capture.job = None;
        #[cfg(not(target_arch = "wasm32"))]
        if request.len() > INLINE_BYTES && !self.run_inline {
            let game = self.chess.board().game.clone();
            let state = Arc::new(Mutex::new(JobState::Running));
            let (slot, work) = (state.clone(), request.clone());
            let run = move || {
                let _finish = Finish(slot.clone());
                let plan = plan_for(&game, &work);
                *lock(&slot) = JobState::Done(Box::new(plan));
            };
            if std::thread::Builder::new().name("pdfcraft-capture".into()).spawn(run).is_ok() {
                self.chess.capture.job = Some(Job { request, state });
                return;
            }
            // No thread: plan here.
        }
        let plan = plan_for(&self.chess.board().game, &request);
        self.finish_capture(request, plan, false);
    }

    /// A plan is made: say why nothing fits, keep it for the user, or apply it.
    fn finish_capture(&mut self, request: Request, plan: CapturePlan, replanned: bool) {
        if self.chess.games.current_doc() != request.doc {
            self.notify(tl!("The capture was dropped: another document's games are shown now.").to_owned());
            return;
        }
        if plan.reasons.contains(&PreviewReason::TooLong) {
            self.notify(tl!("Too much text to capture at once: at most 64 KiB.").to_owned());
            return;
        }
        if plan.chosen.is_none() {
            let why = if matches!(request.mode, CaptureMode::Comment { .. }) {
                tl!("Nothing was captured: there is no text to write.")
            } else {
                tl!("Nothing was captured: no move of the text fits the game here.")
            };
            self.notify(why.to_owned());
            return;
        }
        if plan.needs_preview || request.text.review() {
            self.chess.capture.pending = Some(Pending::new(request, plan));
            self.chess.enter_scope();
            return;
        }
        self.apply_capture(request, &plan, None, replanned, false);
    }

    /// Apply candidate `choice` of `plan` (its best when `None`) as one undo step; `keep_rest`
    /// adds what was not read to the comment of the last move it put in. A plan made on a game
    /// that has changed since is made again, once.
    fn apply_capture(&mut self, request: Request, plan: &CapturePlan, choice: Option<usize>, replanned: bool, keep_rest: bool) {
        let kind = captured(plan, choice, request.mode);
        let rest = if keep_rest { candidate(plan, choice).map(leftover).unwrap_or_default() } else { String::new() };
        let (mut stale, mut rest_kept) = (false, true);
        let recorded = self.chess.board_mut().edit(|s| match plan.apply(&mut s.game, choice) {
            Ok(()) => {
                if !rest.is_empty() {
                    let node = s.game.current();
                    let old = s.game.comment(node);
                    let joined = if old.is_empty() { rest.clone() } else { format!("{old} {rest}") };
                    // Never cut: a rest too long for the comment stays on the page, and is said.
                    if joined.len() <= MAX_COMMENT_LEN {
                        s.game.set_comment(node, &joined);
                    } else {
                        rest_kept = false;
                    }
                }
                s.selected = None;
                s.promotion = None;
                s.message = None;
                Some(ChessEdit::Capture(kind))
            }
            Err(CaptureError::Stale) => {
                stale = true;
                None
            }
            Err(CaptureError::NoCandidate) => None,
        });
        if stale {
            if replanned {
                self.notify(tl!("The game kept changing: the capture was not applied.").to_owned());
                return;
            }
            let again = plan_for(&self.chess.board().game, &request);
            self.finish_capture(request, again, true);
            return;
        }
        // Undo goes to the game, whatever the pointer is over (D12).
        self.chess.enter_scope();
        if !rest_kept {
            self.notify(tl!("The rest is too long to keep as a comment; it is still on the page.").to_owned());
        }
        if !recorded {
            self.notify(tl!("Nothing new: the moves are in the game already.").to_owned());
            return;
        }
        let now = self.ctx.as_ref().map_or(0.0, |ctx| ctx.input(|i| i.time));
        let game_id = self.chess.games.scope_stamp().0;
        let board = self.chess.board_mut();
        let nodes = candidate(plan, choice).map(|c| c.report.new_nodes.iter().filter_map(|p| board.game.node_at(p)).collect()).unwrap_or_default();
        let revision = board.game.revision();
        board.fresh = Fresh { nodes, revision, since: now };
        self.chess.capture.toast = Some(Toast { text: summary(plan, choice, kind), game: game_id, revision });
    }

    /// Apply the plan waiting for the user, as candidate `choice` (its chosen one when `None`),
    /// with what was not read kept as a comment when `keep_rest`.
    pub fn apply_pending_capture(&mut self, choice: Option<usize>, keep_rest: bool) {
        let Some(p) = self.chess.capture.pending.take() else { return };
        let choice = choice.or(p.choice);
        self.apply_capture(p.request, &p.plan, choice, false, keep_rest);
    }

    /// The capture notice's Undo: the capture it is about is undone, if it is still the game's
    /// last change.
    pub fn undo_capture(&mut self) {
        let Some(toast) = self.chess.capture.toast.take() else { return };
        if toast.is_current(&self.chess) && matches!(self.chess.board().history.undo_edit(), Some(ChessEdit::Capture(_))) {
            self.chess.board_mut().undo();
        }
    }

    /// Every frame: a capture tool just armed puts the page tool on Select, pasted text is
    /// captured and a plan made off the thread is picked up.
    pub(crate) fn poll_chess_capture(&mut self) {
        if std::mem::take(&mut self.chess.capture.select_page)
            && !matches!(self.quick_tool, crate::QuickTool::Select | crate::QuickTool::ColumnSelect)
        {
            self.quick_tool = crate::QuickTool::Select;
        }
        if std::mem::take(&mut self.chess.capture.undo) {
            self.undo_capture();
        }
        for note in std::mem::take(&mut self.chess.capture.notes) {
            self.notify(note);
        }
        for text in std::mem::take(&mut self.chess.capture.pasted) {
            let mode = self.chess.capture.paste_mode.take();
            self.capture_pasted(&text, mode);
        }
        self.poll_chess_area();
        self.poll_chess_samples();
        let Some(job) = self.chess.capture.job.as_ref() else { return };
        let state = std::mem::replace(&mut *lock(&job.state), JobState::Running);
        match state {
            JobState::Running => {
                if let Some(ctx) = &self.ctx {
                    ctx.request_repaint_after(std::time::Duration::from_millis(30));
                }
            }
            JobState::Done(plan) => {
                if let Some(job) = self.chess.capture.job.take() {
                    self.finish_capture(job.request, *plan, false);
                }
            }
            JobState::Gone => {
                self.chess.capture.job = None;
                self.notify(tl!("The capture couldn't be planned.").to_owned());
            }
        }
    }

    /// Ctrl+C copied the selection of view `vi`: with the preference on and a capture tool armed,
    /// it is captured too (D5 c).
    pub(crate) fn capture_on_copy(&mut self, vi: usize) {
        if self.chess.capture.on_copy && armed(&self.chess).is_some() {
            self.capture_selection(vi, None);
        }
    }
}

// ─── On the page ─────────────────────────────────────────────────────────────────────────────

/// The modes a capture can be asked for in, as the menus list them. A comment takes the Comment
/// tool's switches when it is captured ([`with_switches`]).
fn menu_modes() -> [((CaptureMode, bool), &'static str); 5] {
    [
        ((CaptureMode::Auto, false), tl!("Capture (automatic)")),
        ((CaptureMode::Line, false), tl!("Capture as the line")),
        ((CaptureMode::Variation, false), tl!("Capture as a variation")),
        ((CaptureMode::SubVariation, false), tl!("Capture as a sub-variation")),
        ((CaptureMode::Comment { before: false }, false), tl!("Capture as a comment")),
    ]
}

/// `mode` with the Comment tool's switches, for a comment.
fn with_switches(chess: &ChessState, mode: (CaptureMode, bool)) -> (CaptureMode, bool) {
    match mode {
        (CaptureMode::Comment { .. }, _) => (CaptureMode::Comment { before: chess.capture.before }, chess.capture.replace),
        other => other,
    }
}

/// Send to PGN ▸: the selection captured in the mode chosen, without arming a tool.
pub(crate) fn send_menu(ui: &mut egui::Ui) -> Option<(CaptureMode, bool)> {
    let mut chosen = None;
    ui.menu_button(tl!("Send to PGN"), |ui| {
        for (mode, label) in menu_modes() {
            if ui.button(label).clicked() {
                chosen = Some(mode);
                ui.close();
            }
        }
    });
    chosen
}

/// The clipboard's text, when it can be read now: on the desktop.
pub(crate) fn clipboard_text() -> Option<String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        arboard::Clipboard::new().ok()?.get_text().ok().filter(|t| !t.trim().is_empty())
    }
    #[cfg(target_arch = "wasm32")]
    {
        None
    }
}

/// How the armed tool captures, as the chip says it after "Capturing ·".
fn mode_phrase(tool: PgnTool) -> &'static str {
    match tool {
        PgnTool::Capture(CaptureMode::Line) => tl!("as the line"),
        PgnTool::Capture(CaptureMode::Variation) => tl!("as a variation"),
        PgnTool::Capture(CaptureMode::SubVariation) => tl!("as a sub-variation"),
        PgnTool::Comment { .. } | PgnTool::Capture(CaptureMode::Comment { .. }) => tl!("as a comment"),
        PgnTool::Capture(CaptureMode::Auto) | PgnTool::Navigate | PgnTool::Setup | PgnTool::GlyphBrush(_) => tl!("automatic"),
    }
}

/// The capture's marks over the page area `area` of `view`, whose document is at edit
/// `generation`: while a tool is armed, the chip in its corner ("Capturing · automatic ▾ · Esc
/// to stop") with the Comment tool's switches; while a plan is made, that it is; while one waits,
/// the preview beside the selection (§4.3.4), closed by Esc or once the document is edited;
/// after a capture went in, its notice with Undo. Returns the candidate to apply, with whether
/// to keep the rest as a comment, when the user said so (Apply, or Enter).
pub(crate) fn page_overlay(
    ui: &mut egui::Ui,
    area: Rect,
    chess: &mut ChessState,
    view: &crate::canvas::DocView,
    generation: u64,
) -> Option<(Option<usize>, bool)> {
    let ctx = ui.ctx().clone();
    // A preview of a selection on a page that has changed since shows a state that is gone.
    if chess.capture.pending.as_ref().is_some_and(|p| p.request.doc == Some(view.id) && p.request.generation.is_some_and(|g| g != generation)) {
        chess.capture.pending = None;
        chess.capture.notes.push(tl!("The capture was closed: the document was edited.").to_owned());
    }
    if chess.capture.toast.as_ref().is_some_and(|t| !t.is_current(chess)) {
        chess.capture.toast = None;
    }
    let tool = chess.tools.tool;
    let armed = mode_of(tool).is_some();
    let reading_area = crate::chess_ocr_area::reading(chess);
    let busy = chess.capture.job.is_some() || reading_area;
    let waiting = chess.capture.pending.is_some();
    if let Some(save) = crate::chess_samples::ask(&ctx, area, chess, view.id) {
        chess.capture.samples.answer(view.id, save);
    }
    if !armed && !busy && !waiting && chess.capture.toast.is_none() {
        return None;
    }
    let free = !ctx.text_edit_focused() && !egui::Popup::is_any_open(&ctx);
    if (armed || waiting || reading_area) && free && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
        if !escape_preview(chess) {
            chess.tools.tool = PgnTool::Navigate;
        }
        return None;
    }
    let t = Tokens::get(&ctx);
    let mut apply = None;
    if waiting && (std::mem::take(&mut chess.capture.enter) || (free && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)))) {
        apply = chess.capture.pending.as_ref().map(|p| (p.choice, false));
    }
    if armed || busy {
        egui::Area::new(egui::Id::new("chess-capture-chip"))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::RIGHT_TOP)
            .fixed_pos(pos2(area.right() - 16.0, area.top() + 10.0))
            .show(&ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if armed {
                            chip(ui, &t, chess, tool);
                        } else {
                            ui.spinner();
                            let what = if reading_area { tl!("Reading the area…") } else { tl!("Reading the selection…") };
                            ui.label(RichText::new(what).color(t.text_muted));
                        }
                    });
                });
            });
    }
    if let Some(action) = preview_area(&ctx, &t, area, chess, view) {
        match action {
            PreviewAction::Apply { keep_rest } => apply = chess.capture.pending.as_ref().map(|p| (p.choice, keep_rest)),
            PreviewAction::Cancel => chess.capture.pending = None,
            PreviewAction::Choose(i) => {
                if let Some(p) = chess.capture.pending.as_mut() {
                    p.choose(Some(i));
                }
            }
            PreviewAction::Lang(lang) => relang(chess, lang),
            PreviewAction::Correct(change) => crate::chess_correct::correct(chess, change),
        }
    }
    if let Some(toast) = chess.capture.toast.as_ref() {
        let text = toast.text.clone();
        let mut undo = false;
        egui::Area::new(egui::Id::new("chess-capture-toast"))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::CENTER_BOTTOM)
            .fixed_pos(pos2(area.center().x, area.bottom() - 24.0))
            .show(&ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(icons::image("check", 14.0, t.accent));
                        ui.label(&text);
                        let r = ui.button(tl!("Undo"));
                        r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tl!("Undo this capture")));
                        undo = r.clicked();
                    });
                });
            });
        if undo {
            chess.capture.undo = true;
        }
    }
    apply
}

/// What the preview asks for.
enum PreviewAction {
    Apply {
        keep_rest: bool,
    },
    Cancel,
    /// Another place to fit at.
    Choose(usize),
    /// Read the text again in another language.
    Lang(Lang),
    /// Read the text again with a move that did not play corrected (T10).
    Correct(crate::chess_correct::Change),
}

/// The language names the preview offers, each in its own language.
fn endonym(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "English",
        Lang::Pt => "Português",
        Lang::De => "Deutsch",
        Lang::Es => "Español",
        Lang::Fr => "Français",
        Lang::It => "Italiano",
        Lang::Ru => "Русский",
        Lang::Nl => "Nederlands",
        Lang::Pl => "Polski",
        Lang::Cs => "Čeština",
        Lang::Hu => "Magyar",
        Lang::Da => "Dansk",
        Lang::Sv => "Svenska",
        Lang::No => "Norsk",
        Lang::Fi => "Suomi",
        Lang::Ro => "Română",
        Lang::Is => "Íslenska",
        Lang::Ca => "Català",
    }
}

/// Plan the waiting capture again, read in `lang`, which becomes its document's language for
/// captures.
fn relang(chess: &mut ChessState, lang: Lang) {
    let Some(mut p) = chess.capture.pending.take() else { return };
    if let Some(doc) = p.request.doc {
        chess.capture.lang = Some((doc, lang));
    }
    p.request.lang = lang;
    let plan = plan_for(&chess.board().game, &p.request);
    chess.capture.pending = Some(Pending::new(p.request, plan));
}

/// Where the preview goes: beside the last piece of the selection while it is on screen, else
/// under the chip.
fn preview_place(area: Rect, p: &Pending, view: &crate::canvas::DocView) -> (egui::Align2, egui::Pos2) {
    let beside = match &p.request.text {
        Text::Page { page, rects, .. } => rects.last().and_then(|r| Some(view.page_xform(*page)?.view_rect(*r))),
        Text::Plain(_) => None,
    };
    match beside.filter(|r| area.intersects(*r)) {
        Some(r) => {
            let x = r.left().clamp(area.left() + 8.0, (area.right() - 460.0).max(area.left() + 8.0));
            (egui::Align2::LEFT_TOP, pos2(x, (r.bottom() + 8.0).min(area.bottom() - 40.0)))
        }
        None => (egui::Align2::RIGHT_TOP, pos2(area.right() - 16.0, area.top() + 56.0)),
    }
}

/// The preview popover of the capture waiting (§4.3.4): where it goes ("Variation of 18…Qxe1")
/// and the other places it fits, the language it is read in, its moves as chips (✓ played, ✗
/// stopped with why, grey not read; the main line's in bold), why it is shown, and Apply, Apply
/// and keep the rest as a comment, Cancel. A stopped move's chip opens its correction panel
/// (T10, `chess_correct`).
fn preview_area(ctx: &egui::Context, t: &Tokens, area: Rect, chess: &mut ChessState, view: &crate::canvas::DocView) -> Option<PreviewAction> {
    let p = chess.capture.pending.as_mut()?;
    let (pivot, at) = preview_place(area, p, view);
    let mut action = None;
    egui::Area::new(egui::Id::new("chess-capture-preview")).order(egui::Order::Foreground).pivot(pivot).fixed_pos(at).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.set_max_width(440.0);
            action = preview(ui, t, p);
        });
    });
    action
}

fn preview(ui: &mut egui::Ui, t: &Tokens, p: &mut Pending) -> Option<PreviewAction> {
    let mut action = None;
    let mut toggle = None;
    let c = candidate(&p.plan, p.choice);
    let title = c.map_or_else(|| tl!("No move of the text fits the game here.").to_owned(), |c| headline(c, captured_by(c, p.request.mode)));
    ui.label(RichText::new(title).font(crate::theme::semibold(14.0)));
    ui.horizontal_wrapped(|ui| {
        if p.plan.candidates.len() > 1 {
            let label = ui.label(RichText::new(tl!("Fit at")).small().color(t.text_muted));
            let shown = c.map(|c| headline(c, captured_by(c, p.request.mode))).unwrap_or_default();
            let combo = egui::ComboBox::from_id_salt("chess-capture-fit").selected_text(shown).show_ui(ui, |ui| {
                for (i, other) in p.plan.candidates.iter().enumerate() {
                    let moves = crate::i18n::fmt(tl!("moves: {n}"), &[("n", &other.played.to_string())]);
                    let label = format!("{} · {moves}", headline(other, captured_by(other, p.request.mode)));
                    if ui.selectable_label(p.choice == Some(i), label).clicked() {
                        action = Some(PreviewAction::Choose(i));
                    }
                }
            });
            combo.response.labelled_by(label.id);
        }
        let label = ui.label(RichText::new(tl!("Language")).small().color(t.text_muted));
        let combo = egui::ComboBox::from_id_salt("chess-capture-lang").selected_text(endonym(p.request.lang)).show_ui(ui, |ui| {
            for lang in Lang::ALL {
                if ui.selectable_label(lang == p.request.lang, endonym(lang)).clicked() && lang != p.request.lang {
                    action = Some(PreviewAction::Lang(lang));
                }
            }
        });
        combo.response.labelled_by(label.id);
    });
    if let Some(c) = c {
        ui.add_space(4.0);
        egui::ScrollArea::vertical().id_salt("chess-capture-chips").max_height(160.0).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(4.0, 3.0);
                if let Some(k) = chips(ui, t, c, &p.stopped, p.fix.as_ref().map(|f| f.stop)) {
                    toggle = Some(k);
                }
            });
        });
    }
    if let Some(k) = toggle {
        p.fix = match (&p.fix, p.stopped.get(k)) {
            (Some(f), _) if f.stop == k => None,
            (_, Some(s)) => Some(crate::chess_correct::Fix::open(k, s)),
            (_, None) => None,
        };
    }
    let lang = p.request.lang;
    let read = match &p.request.text {
        Text::Page { read: Some(read), geometry, .. } => Some((read.clone(), *geometry)),
        _ => None,
    };
    if let Some(fix) = p.fix.as_mut()
        && let Some(s) = p.stopped.get(fix.stop)
        && let Some(change) = crate::chess_correct::panel(ui, t, fix, s, lang, read.as_ref().map(|(r, g)| (&**r, g.as_ref())))
    {
        action = Some(PreviewAction::Correct(change));
    }
    let c = candidate(&p.plan, p.choice);
    let reasons: &[PreviewReason] = match c {
        Some(c) if p.choice != p.plan.chosen => &c.reasons,
        _ => &p.plan.reasons,
    };
    for reason in reasons {
        ui.label(RichText::new(format!("• {}", reason_text(*reason))).small());
    }
    if p.request.text.review() {
        ui.label(RichText::new(format!("• {}", tl!("Chess OCR corrected or doubted some words: check the moves."))).small());
    }
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        let can = c.is_some();
        if ui.add_enabled(can, egui::Button::new(tl!("Apply")).shortcut_text("Enter")).clicked() {
            action = Some(PreviewAction::Apply { keep_rest: false });
        }
        if c.is_some_and(|c| c.mode_used != GraftMode::Comment && !leftover(c).is_empty())
            && ui.button(tl!("Apply and keep the rest as a comment")).clicked()
        {
            action = Some(PreviewAction::Apply { keep_rest: true });
        }
        if ui.add(egui::Button::new(tl!("Cancel")).shortcut_text("Esc")).clicked() {
            action = Some(PreviewAction::Cancel);
        }
    });
    action
}

/// Most chips the preview shows.
const MAX_CHIPS: usize = 160;

/// The candidate's items as chips: each move ✓ played, ✗ where the graft stopped (and why), or
/// grey when not read, bold when printed bold; glyphs, comments and brackets as they read; then
/// the printed moves the reader could not play, ✗ with why. Colour is never the only sign. A ✗
/// chip is a button: it opens (or closes) the correction panel of that move of `stopped`, which
/// `open` is; returns the one clicked.
fn chips(ui: &mut egui::Ui, t: &Tokens, c: &Candidate, stopped: &[StoppedMove], open: Option<usize>) -> Option<usize> {
    let mut clicked = None;
    let mut stop_chip = |ui: &mut egui::Ui, k: Option<usize>, rich: RichText| {
        let Some(k) = k else {
            ui.label(rich);
            return;
        };
        let r = ui.selectable_label(open == Some(k), rich).on_hover_text(tl!("Correct this move"));
        if r.clicked() {
            clicked = Some(k);
        }
    };
    const PLAYED: egui::Color32 = egui::Color32::from_rgb(0x2D, 0x9D, 0x5B);
    const STOPPED: egui::Color32 = egui::Color32::from_rgb(0xE0, 0x3E, 0x3E);
    let stop = c.report.stop.as_ref();
    let rest_from = c.report.rest_from.or(stop.map(|s| s.index));
    let mut shown = 0usize;
    for (k, item) in c.read.items.iter().enumerate() {
        if shown >= MAX_CHIPS {
            ui.label(RichText::new("…").color(t.text_faint));
            return clicked;
        }
        shown = shown.saturating_add(1);
        match item {
            FragmentItem::Move { text, bold, .. } => {
                let (mark, colour, why) = match (stop, rest_from) {
                    (Some(s), _) if s.index == k => ("✗", STOPPED, Some(stop_text(s.reason))),
                    (_, Some(r)) if k >= r => ("·", t.text_faint, None),
                    _ => ("✓", PLAYED, None),
                };
                let mut rich = RichText::new(format!("{mark} {text}")).small().color(colour);
                if *bold == Some(true) {
                    rich = rich.strong();
                }
                match why {
                    Some(why) => {
                        stop_chip(ui, stopped.iter().position(|s| s.graft), rich);
                        ui.label(RichText::new(why).small().color(STOPPED));
                    }
                    None => {
                        ui.label(rich);
                    }
                }
            }
            FragmentItem::Nag(code) => {
                ui.label(RichText::new(glyph_text(*code)).small().color(t.text_muted));
            }
            FragmentItem::Comment { text, .. } => {
                let short: String = text.chars().take(40).collect();
                let more = if text.chars().count() > 40 { "…" } else { "" };
                ui.label(RichText::new(format!("“{short}{more}”")).small().italics().color(t.text_muted));
            }
            FragmentItem::Open => {
                ui.label(RichText::new("(").small().color(t.text_faint));
            }
            FragmentItem::Close => {
                ui.label(RichText::new(")").small().color(t.text_faint));
            }
            FragmentItem::Result => {}
        }
    }
    for (k, s) in c.read.stops.iter().enumerate() {
        let rich = RichText::new(format!("✗ {} — {}", s.text, status_text(s.status))).small().color(STOPPED);
        stop_chip(ui, (k < stopped.len()).then_some(k), rich);
    }
    clicked
}

/// A glyph as the notation shows it, in text: a painted sign or an assessment without one by name.
fn glyph_text(code: u16) -> String {
    crate::chess_board_ui::nag_text(code)
}

/// Why the graft stopped at a move, in words.
fn stop_text(reason: GraftStopReason) -> &'static str {
    match reason {
        GraftStopReason::Illegal => tl!("Illegal move"),
        GraftStopReason::Ambiguous => tl!("Ambiguous move"),
        GraftStopReason::Unreadable => tl!("Unreadable move"),
        GraftStopReason::TooLong => tl!("Past the capture's limits"),
        GraftStopReason::TooDeep => tl!("Variations nest too deep"),
        GraftStopReason::NullMove => tl!("A null move can't be played"),
        GraftStopReason::NoAnchor => tl!("The move it goes after is gone"),
    }
}

/// Why the reader did not play a printed move, in words (the Text tab's words).
fn status_text(status: MoveStatus) -> &'static str {
    match status {
        MoveStatus::Valid => tl!("Valid move"),
        MoveStatus::Illegal => tl!("Illegal move"),
        MoveStatus::Ambiguous => tl!("Ambiguous move"),
        MoveStatus::OutOfSequence => tl!("Move out of sequence"),
        MoveStatus::NotReached => tl!("Not reached: an earlier move failed"),
        MoveStatus::Unchecked => tl!("Unchecked: no position to play it from"),
    }
}

/// What candidate `c` did not read: the graft's leftover, then the text after a result.
fn leftover(c: &Candidate) -> String {
    let mut out = c.report.leftover_text.trim().to_owned();
    let rest = c.read.rest.trim();
    if !rest.is_empty() && !out.contains(rest) {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(rest);
    }
    out
}

impl Toast {
    /// Whether the game is as the capture left it.
    fn is_current(&self, chess: &ChessState) -> bool {
        chess.games.scope_stamp().0 == self.game && chess.board().game.revision() == self.revision
    }
}

/// Take Enter out of a frame's `events` (the main window's raw input, or the chess window's
/// own) while a capture waits for the user: it applies the capture, before the rail, a group or
/// the comment editor can use it. Nothing is taken when `skip` (a modal dialog is open, or a
/// text field has the keyboard: the caller knows, and must not ask the context while it holds
/// its input).
pub(crate) fn take_enter(chess: &mut ChessState, events: &mut Vec<egui::Event>, skip: bool) {
    if skip || chess.capture.pending.is_none() {
        return;
    }
    let enter = &mut chess.capture.enter;
    events.retain(|event| match event {
        egui::Event::Key { key: Key::Enter, pressed, modifiers, .. } if modifiers.is_none() => {
            *enter |= *pressed;
            false
        }
        _ => true,
    });
}

// ─── Text already captured (§4.3.5) ──────────────────────────────────────────────────────────

/// The text already captured, as boxes: page, and box in user space.
pub(crate) type Layer = Arc<Vec<(usize, [f32; 4])>>;

/// Most boxes the layer keeps.
const MAX_LAYER: usize = 100_000;

/// The boxes of the text already captured into the games of document `doc` (page, box in user
/// space), when the layer is on and `doc` is the document shown. Read again only when a game
/// changed.
pub(crate) fn captured_layer(chess: &mut ChessState, doc: DocId) -> Option<Layer> {
    use std::hash::{Hash, Hasher};
    if !chess.capture.show_captured || chess.games.current_doc() != Some(doc) {
        return None;
    }
    let games = chess.games.collection();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    doc.hash(&mut h);
    games.len().hash(&mut h);
    for e in games.iter() {
        e.board.game.revision().hash(&mut h);
        e.board.game.start_fen().hash(&mut h);
    }
    let key = h.finish();
    if let Some((k, layer)) = &chess.capture.layer
        && *k == key
    {
        return Some(layer.clone());
    }
    let mut boxes = Vec::new();
    'games: for e in games.iter() {
        let g = &e.board.game;
        for token in g.tokens() {
            if let Token::Move { node, .. } = token
                && let Some(source) = g.source(node)
                && let Ok(page) = usize::try_from(source.page)
            {
                if boxes.len() >= MAX_LAYER {
                    break 'games;
                }
                boxes.push((page, source.rect_user));
            }
        }
    }
    let layer = Arc::new(boxes);
    chess.capture.layer = Some((key, layer.clone()));
    Some(layer)
}

/// Underline on page `page` the boxes of `layer` there, lightly; each one drawn is added to
/// `drawn`.
pub(crate) fn paint_captured(
    painter: &egui::Painter,
    xf: &crate::canvas::PageXform,
    page: usize,
    info: &pdfcraft_render::DocInfo,
    layer: Option<&[(usize, [f32; 4])]>,
    drawn: &mut Vec<(usize, [f32; 4])>,
) {
    let Some(layer) = layer else { return };
    // `user_rect` reads the page's box: never for a page the document does not have.
    if info.pages.get(page).is_none() {
        return;
    }
    let colour = Tokens::get(painter.ctx()).accent.gamma_multiply(0.75);
    for (_, r) in layer.iter().filter(|(p, _)| *p == page) {
        if !r.iter().all(|v| v.is_finite()) {
            continue;
        }
        let sr = xf.user_rect(info, page, *r);
        let y = sr.bottom() + 1.0;
        painter.line_segment([pos2(sr.left(), y), pos2(sr.right(), y)], egui::Stroke::new(1.5, colour));
        drawn.push((page, *r));
    }
}

/// The board panel's row for captures: Paste as ▸ (the clipboard's text captured in the mode
/// chosen) and the switch for the layer of text already captured.
pub(crate) fn panel_row(ui: &mut egui::Ui, chess: &mut ChessState) -> Option<(CaptureMode, bool)> {
    let mut chosen = None;
    ui.horizontal_wrapped(|ui| {
        ui.menu_button(tl!("Paste as"), |ui| {
            for (mode, label) in menu_modes() {
                if ui.button(label).clicked() {
                    chosen = Some(mode);
                    ui.close();
                }
            }
        });
        ui.add(crate::widgets::checkbox(&mut chess.capture.show_captured, tl!("Underline captured text on the page")));
    });
    chosen
}

/// The chip's line: what is armed, its menu, the Comment tool's switches, and how to stop.
fn chip(ui: &mut egui::Ui, t: &Tokens, chess: &mut ChessState, tool: PgnTool) {
    ui.add(icons::image("wand-sparkles", 14.0, t.accent));
    ui.label(RichText::new(tl!("Capturing")).strong());
    ui.label(RichText::new("·").color(t.text_faint));
    let mut picked = None;
    ui.menu_button((mode_phrase(tool), icons::image("chevron-down", 12.0, t.icon)), |ui| {
        let tools = [
            PgnTool::Capture(CaptureMode::Auto),
            PgnTool::Capture(CaptureMode::Line),
            PgnTool::Capture(CaptureMode::Variation),
            PgnTool::Capture(CaptureMode::SubVariation),
            PgnTool::Comment { before: false, replace: false },
        ];
        for (other, (_, label)) in tools.into_iter().zip(menu_modes()) {
            if ui.selectable_label(other.same(tool), label).clicked() {
                picked = Some(other);
                ui.close();
            }
        }
        ui.separator();
        let copy = crate::commands::shortcut_label(ui.ctx(), crate::commands::COPY);
        ui.add(crate::widgets::checkbox(&mut chess.capture.on_copy, crate::i18n::fmt(tl!("Also capture what is copied ({key})"), &[("key", &copy)])));
        crate::chess_samples::menu(ui, t, chess);
    });
    if let Some(other) = picked {
        arm(chess, other);
    }
    crate::chess_ocr_area::chip(ui, t, chess);
    if let PgnTool::Comment { before, replace } = chess.tools.tool {
        let (mut before, mut replace) = (before, replace);
        ui.label(RichText::new("·").color(t.text_faint));
        if ui.selectable_label(!before, tl!("after the move")).clicked() {
            before = false;
        }
        if ui.selectable_label(before, tl!("before the move")).clicked() {
            before = true;
        }
        ui.label(RichText::new("·").color(t.text_faint));
        if ui.selectable_label(!replace, tl!("add to the comment")).clicked() {
            replace = false;
        }
        if ui.selectable_label(replace, tl!("replace the comment")).clicked() {
            replace = true;
        }
        chess.capture.before = before;
        chess.capture.replace = replace;
        chess.tools.tool = PgnTool::Comment { before, replace };
    }
    ui.label(RichText::new("·").color(t.text_faint));
    if ui.small_button(tl!("Esc to stop")).clicked() {
        chess.tools.tool = PgnTool::Navigate;
    }
}

impl crate::canvas::DocView {
    /// The glyph ranges of page `page` whose text is `needle` (case-insensitive), for tests and
    /// automation that aim a selection; empty while the page's text layer is not loaded.
    pub fn text_matches(&self, page: usize, needle: &str) -> Vec<std::ops::Range<usize>> {
        self.texts.get(&page).map(|t| t.find(needle)).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A line of words as a page's text layer: one glyph per character, a gap between words.
    fn line(words: &[&str]) -> PageText {
        let mut glyphs = Vec::new();
        let mut x = 0.0f32;
        for w in words {
            for c in w.chars() {
                let rect = [x, 0.0, x + 5.0, 10.0];
                glyphs.push(TextGlyph::new(c.to_string(), rect, [[x, 10.0], [x + 5.0, 10.0], [x + 5.0, 0.0], [x, 0.0]], [1.0, 0.0]));
                x += 5.0;
            }
            x += 6.0;
        }
        pdfcraft_render::text::layout(glyphs)
    }

    #[test]
    fn a_selection_grows_to_whole_tokens() {
        // "20.Nxf6+ exf6": glyphs 0..8 and 8..12.
        let text = line(&["20.Nxf6+", "exf6"]);
        assert_eq!(text.text_of(0..text.glyphs.len()), "20.Nxf6+ exf6");
        // "20.Nx" (glyphs 0..=4) becomes the whole first token.
        assert_eq!(snap_to_tokens(&text, &[0, 1, 2, 3, 4]), (0..8).collect::<Vec<_>>());
        // "f6+ ex" becomes both tokens.
        assert_eq!(snap_to_tokens(&text, &[5, 6, 7, 8, 9]), (0..12).collect::<Vec<_>>());
        // Out of range and repeated glyphs are dropped; empty stays empty.
        assert_eq!(snap_to_tokens(&text, &[9, 9, 999]), (8..12).collect::<Vec<_>>());
        assert!(snap_to_tokens(&text, &[]).is_empty());
    }

    #[test]
    fn the_rail_tools_name_their_modes() {
        assert_eq!(mode_of(PgnTool::Navigate), None);
        assert_eq!(mode_of(PgnTool::Comment { before: true, replace: true }), Some((CaptureMode::Comment { before: true }, true)));
        assert!(PgnTool::Comment { before: true, replace: false }.same(PgnTool::Comment { before: false, replace: true }));
        assert!(!PgnTool::Capture(CaptureMode::Auto).same(PgnTool::Capture(CaptureMode::Line)));
    }
}
