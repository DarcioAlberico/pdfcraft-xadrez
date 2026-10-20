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
//! since is made again, once.

use std::sync::{Arc, Mutex, MutexGuard};

use egui::{Key, Modifiers, Rect, RichText, pos2};
use pdfcraft_engine::DocId;
use pdfcraft_engine::chess::capture::{self, CaptureError, CaptureMode, CapturePlan, CaptureText, PreviewReason};
use pdfcraft_engine::chess::pdfcraft_chess::game::{Game, GraftMode, NodePath};
use pdfcraft_engine::chess::pdfcraft_chess::san::Lang;
use pdfcraft_engine::chess::pdfcraft_chessvision::coords::PageGeometry;
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
}

/// A plan the user is to look at before it goes in.
pub struct Pending {
    pub(crate) request: Request,
    pub plan: CapturePlan,
    /// The candidate chosen (the plan's best, until the user picks another).
    pub choice: Option<usize>,
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
}

/// The text captured.
#[derive(Clone)]
pub(crate) enum Text {
    /// Glyphs of page `page`, with the page's geometry (the moves keep where they were printed).
    Page { page: usize, glyphs: Arc<Vec<TextGlyph>>, geometry: Option<PageGeometry> },
    /// Pasted text.
    Plain(Arc<str>),
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
        PgnTool::Navigate => None,
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
    chess.capture.pending.take().is_some()
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
fn plan_for(game: &Game, request: &Request) -> CapturePlan {
    let current = game.current();
    let text = match &request.text {
        Text::Page { page, glyphs, geometry, .. } => {
            CaptureText::Glyphs { glyphs: glyphs.as_slice(), geometry: *geometry, page: u32::try_from(*page).unwrap_or(u32::MAX) }
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

/// What candidate `choice` of `plan` (its best when `None`) makes of the game.
fn captured(plan: &CapturePlan, choice: Option<usize>, mode: CaptureMode) -> Captured {
    let Some(c) = choice.or(plan.chosen).and_then(|i| plan.candidates.get(i)) else { return Captured::Variation };
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
    let Some(c) = choice.or(plan.chosen).and_then(|i| plan.candidates.get(i)) else { return String::new() };
    let g = &c.game;
    let name = |path: &NodePath| g.node_at(path).filter(|&n| n != 0).map(|n| crate::chess_tools::move_name(g, n));
    let first = c.report.new_nodes.first();
    let what = match kind {
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
    };
    if kind == Captured::Comment {
        return what;
    }
    format!("{what} · {}", crate::i18n::fmt(tl!("moves: {n}"), &[("n", &c.report.played.to_string())]))
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
        self.sync_chess_docs();
        let lang = document_lang(&self.chess, Some(id));
        let doc = self.chess.games.current_doc();
        self.start_capture(Request { mode, replace, text: Text::Page { page, glyphs: Arc::new(glyphs), geometry }, doc, lang });
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
        self.start_capture(Request { mode, replace, text: Text::Plain(Arc::from(text)), doc, lang });
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
    fn start_capture(&mut self, request: Request) {
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
        if plan.needs_preview {
            let choice = plan.chosen;
            self.chess.capture.pending = Some(Pending { request, plan, choice });
            self.chess.enter_scope();
            return;
        }
        self.apply_capture(request, &plan, None, replanned);
    }

    /// Apply candidate `choice` of `plan` (its best when `None`) as one undo step. A plan made on
    /// a game that has changed since is made again, once.
    fn apply_capture(&mut self, request: Request, plan: &CapturePlan, choice: Option<usize>, replanned: bool) {
        let kind = captured(plan, choice, request.mode);
        let mut stale = false;
        self.chess.board_mut().edit(|s| match plan.apply(&mut s.game, choice) {
            Ok(()) => {
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
        let said = summary(plan, choice, kind);
        if !said.is_empty() {
            self.notify(said);
        }
    }

    /// Apply the plan waiting for the user, as candidate `choice` (its chosen one when `None`).
    pub fn apply_pending_capture(&mut self, choice: Option<usize>) {
        let Some(p) = self.chess.capture.pending.take() else { return };
        let choice = choice.or(p.choice);
        self.apply_capture(p.request, &p.plan, choice, false);
    }

    /// Every frame: a capture tool just armed puts the page tool on Select, pasted text is
    /// captured and a plan made off the thread is picked up.
    pub(crate) fn poll_chess_capture(&mut self) {
        if std::mem::take(&mut self.chess.capture.select_page)
            && !matches!(self.quick_tool, crate::QuickTool::Select | crate::QuickTool::ColumnSelect)
        {
            self.quick_tool = crate::QuickTool::Select;
        }
        for text in std::mem::take(&mut self.chess.capture.pasted) {
            let mode = self.chess.capture.paste_mode.take();
            self.capture_pasted(&text, mode);
        }
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

/// The board panel's Paste as ▸: the clipboard's text captured in the mode chosen.
pub(crate) fn paste_menu(ui: &mut egui::Ui) -> Option<(CaptureMode, bool)> {
    let mut chosen = None;
    ui.menu_button(tl!("Paste as"), |ui| {
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
        PgnTool::Capture(CaptureMode::Auto) | PgnTool::Navigate => tl!("automatic"),
    }
}

/// The capture's marks over the page area `area`: while a tool is armed, the chip in its corner
/// ("Capturing · automatic ▾ · Esc to stop"), with the Comment tool's switches; while a plan is
/// made, that it is; while one waits, what to look at, with Apply and Cancel. Esc closes what
/// waits, then disarms. Returns the candidate to apply when the user said so.
pub(crate) fn page_overlay(ui: &mut egui::Ui, area: Rect, chess: &mut ChessState) -> Option<Option<usize>> {
    let ctx = ui.ctx().clone();
    let tool = chess.tools.tool;
    let armed = mode_of(tool).is_some();
    let busy = chess.capture.job.is_some();
    if !armed && !busy && chess.capture.pending.is_none() {
        return None;
    }
    if !ctx.text_edit_focused() && !egui::Popup::is_any_open(&ctx) && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
        if !escape_preview(chess) {
            chess.tools.tool = PgnTool::Navigate;
        }
        return None;
    }
    let t = Tokens::get(&ctx);
    let (mut apply, mut cancel) = (None, false);
    egui::Area::new(egui::Id::new("chess-capture-chip"))
        .order(egui::Order::Foreground)
        .pivot(egui::Align2::RIGHT_TOP)
        .fixed_pos(pos2(area.right() - 16.0, area.top() + 10.0))
        .show(&ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    if armed {
                        chip(ui, &t, chess, tool);
                    } else if busy {
                        ui.spinner();
                        ui.label(RichText::new(tl!("Reading the selection…")).color(t.text_muted));
                    }
                });
                if let Some(p) = chess.capture.pending.as_ref() {
                    ui.separator();
                    ui.set_max_width(360.0);
                    ui.label(RichText::new(tl!("Check this capture before it goes in:")).strong());
                    for reason in &p.plan.reasons {
                        ui.label(format!("• {}", reason_text(*reason)));
                    }
                    ui.horizontal(|ui| {
                        if ui.button(tl!("Apply")).clicked() {
                            apply = Some(p.choice);
                        }
                        cancel = ui.button(tl!("Cancel")).clicked();
                    });
                }
            });
        });
    if cancel {
        chess.capture.pending = None;
    }
    apply
}

/// The chip's line: what is armed, its menu, the Comment tool's switches, and how to stop.
fn chip(ui: &mut egui::Ui, t: &Tokens, chess: &mut ChessState, tool: PgnTool) {
    ui.add(icons::image("wand-sparkles", 14.0, t.accent));
    ui.label(RichText::new(tl!("Capturing")).strong());
    ui.label(RichText::new("·").color(t.text_faint));
    let mut picked = None;
    ui.menu_button(format!("{} ▾", mode_phrase(tool)), |ui| {
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
    });
    if let Some(other) = picked {
        arm(chess, other);
    }
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
