//! The chess window (PGN toolbar spec T13a, T13b, §4.8, D8 b).
//!
//! `chess.window.detach` moves the Board panel out of the docking workspace into a window of its
//! own and back. While it is out, the panel is closed in the workspace with its place kept (a
//! hidden `Location`), so its slot collapses and docking it again puts it back where it was; the
//! Read-mode board panel, which shows only while the panel is in the workspace, is gone too.
//!
//! The window holds the Inspector Chess panel's Diagrams and Text tabs too (T13b): from 760 pt
//! wide beside the board, narrower behind a Board | Diagrams | Text strip. Meanwhile the
//! Inspector's panel only says where they are, with a button that docks the window, so no frame
//! draws them twice (both views act on the same `ChessState`). Its Study tab, the same board with
//! set-up and analysis, stays in the Inspector until the dock and Study merge (T5c). What picks a
//! tab for the user (a diagram picked on the page, a diagram opened on the board, a scan) picks
//! the window's while it is out ([`PdfCraftApp::show_chess_tab`],
//! [`PdfCraftApp::reveal_chess_diagrams`]).
//!
//! On the desktop the window is a native one: an immediate viewport
//! (`Context::show_viewport_immediate`), drawn every frame from the main window's pass, before
//! anything that may return early (full screen). Its pass reads its own input, which never goes
//! through `raw_input_hook` (the main window's): the chess keys are taken from it for the
//! window's PGN tool rail (`chess_tools::take_window_keys`), then the app's shortcuts run there
//! while it has the keyboard (`shortcuts_in_pass`); the board's arrow keys never reach the page.
//! Where windows are drawn inside the main one (`embed_viewports`: the web, kittest, the `shot`
//! example) egui's fallback for a viewport panics when it can't show, so it is never used: the app
//! draws its own `egui::Window` with a Dock button instead. Neither the command nor the rail's
//! tool offers the window there, but a window restored from settings is still drawn.
//!
//! Failures dock the board back with a notice:
//! - egui panics when the backend couldn't create the window (or its GL surface) before the pass
//!   began; `pdfcraft_engine::guard` catches it, and nothing of the pass was started.
//! - wgpu couldn't configure the new window's surface: eframe only logs it, runs the pass and
//!   paints nothing, so that pass's texture changes are never uploaded. The patched egui-wgpu
//!   records the window (`take_surface_failures`); the desktop app hands that check over with
//!   [`PdfCraftApp::watch_surface_failures`]. The fonts are installed again (a full font-atlas
//!   upload), and the board's pictures and the icons are loaded again.
//!
//! After either, said once, no separate window is opened again in this session. The board's own
//! code panicking inside the window's pass is caught there too (so the pass ends and egui's state
//! stays whole), and docks the board back with the app's usual "That didn't work" notice.

use craft_ui::docking::Action;
use egui::{Pos2, Vec2, ViewportId, vec2};
use serde_json::{Value, json};

use crate::chess_board_ui::{self, Sizing};
use crate::chess_text_ui::ChessTab;
use crate::docking::Panel;
use crate::theme::Tokens;
use crate::{PdfCraftApp, RightPanel};

/// The window's size the first time, in points: wide enough for the tabs beside the board.
const DEFAULT_SIZE: Vec2 = vec2(960.0, 760.0);
/// The smallest window.
const MIN_SIZE: Vec2 = vec2(320.0, 360.0);
/// Saved sizes and positions beyond this many points are not believed.
const MAX_SAVED: f32 = 16_384.0;
/// From this width the window has the Diagrams and Text tabs beside the board, and the board's
/// column the moves, notes and fields beside the board (§4.1).
const WIDE_FROM: f32 = 760.0;
/// The column of the Diagrams and Text tabs beside the board: this part of the width, within
/// these bounds.
const TABS_SHARE: f32 = 0.4;
const TABS_MIN: f32 = 300.0;
const TABS_MAX: f32 = 560.0;
/// The largest board in the window, in points (§4.8).
const MAX_SIDE: f32 = 1_200.0;
/// The largest board picture in the window, in pixels.
const MAX_PX: u32 = 2_048;
/// Height kept under the board for its controls (flip, navigation, status).
const CONTROLS: f32 = 150.0;
/// The narrowest right-hand column of the wide layout.
const NOTES_MIN: f32 = 300.0;
/// Part of a saved position's window that must be on the monitor for the position to be used.
const ON_SCREEN: Vec2 = vec2(48.0, 48.0);

/// The windows whose surface couldn't be configured since it was last asked (the patched
/// egui-wgpu's `take_surface_failures`), oldest first.
pub type SurfaceFailuresFn = Box<dyn FnMut() -> Vec<ViewportId>>;

/// What the chess window shows: a narrow window one of these, behind a strip; a wide one the
/// board, with Diagrams or Text beside it (Text when chosen, else Diagrams).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChessWindowTab {
    #[default]
    Board,
    Diagrams,
    Text,
}

impl ChessWindowTab {
    /// The settings value.
    fn key(self) -> &'static str {
        match self {
            ChessWindowTab::Board => "board",
            ChessWindowTab::Diagrams => "diagrams",
            ChessWindowTab::Text => "text",
        }
    }

    fn from_key(key: &str) -> Option<ChessWindowTab> {
        [ChessWindowTab::Board, ChessWindowTab::Diagrams, ChessWindowTab::Text].into_iter().find(|t| t.key() == key)
    }
}

/// The chess board window's state: whether it is out, its geometry, and what was asked of it.
pub struct ChessWindow {
    detached: bool,
    /// The window's inner size, in points: kept up to date while it is shown, and saved.
    size: Vec2,
    /// Where the native window's outer top-left corner is on the desktop, in points, when known.
    position: Option<Pos2>,
    /// The size and position the window was opened with: the viewport builder stays the same
    /// while it is shown, or eframe would resize the window to it on every change.
    opened: Option<(Vec2, Option<Pos2>)>,
    /// Out (`true`) or back in the dock (`false`), at the start of the next frame: a request
    /// may come from inside the docking workspace, which holds the layout while it draws.
    request: Option<bool>,
    /// Bring the window to the front next frame.
    raise: bool,
    /// A separate window failed in this session: none is opened again.
    failed: bool,
    /// The patched egui-wgpu's record of windows without a surface (desktop, wgpu only).
    surface_failures: Option<SurfaceFailuresFn>,
    /// The layout the window was last drawn with: the tabs beside the board.
    wide: bool,
    /// The tab chosen (saved).
    tab: ChessWindowTab,
    /// The window was drawn in this frame (reset when the frame starts): the Inspector's panel
    /// then doesn't draw what it holds, even when it was docked during the frame.
    shown: bool,
    /// How many times the font atlas was rebuilt after a failed window (see [`recover_textures`]).
    refreshes: u32,
}

impl Default for ChessWindow {
    fn default() -> ChessWindow {
        ChessWindow {
            detached: false,
            size: DEFAULT_SIZE,
            position: None,
            opened: None,
            request: None,
            raise: false,
            failed: false,
            surface_failures: None,
            wide: false,
            tab: ChessWindowTab::default(),
            shown: false,
            refreshes: 0,
        }
    }
}

impl std::fmt::Debug for ChessWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChessWindow")
            .field("detached", &self.detached)
            .field("size", &self.size)
            .field("position", &self.position)
            .field("failed", &self.failed)
            .field("wide", &self.wide)
            .field("tab", &self.tab)
            .finish_non_exhaustive()
    }
}

impl ChessWindow {
    /// The settings value (`docking_detached`): `{detached, size: [w, h], position: [x, y] |
    /// null, tab: "board" | "diagrams" | "text"}`.
    pub(crate) fn to_json(&self) -> Value {
        json!({
            "detached": self.detached,
            "size": [self.size.x, self.size.y],
            "position": self.position.map(|p| [p.x, p.y]),
            "tab": self.tab.key(),
        })
    }

    /// Whether the Diagrams and Text tabs are in the window in this frame: it is out (or goes
    /// out next frame), or it was drawn in this frame before it was docked.
    pub(crate) fn in_window(&self) -> bool {
        self.shown || self.request.unwrap_or(self.detached)
    }

    /// Show the board: a narrow window switches to it; a wide one always shows it, and keeps
    /// the tab beside it.
    fn show_board(&mut self) {
        if !self.wide {
            self.tab = ChessWindowTab::Board;
        }
    }

    /// Take saved settings. Anything not as written by [`ChessWindow::to_json`] is ignored: a
    /// size that is not two finite numbers between 1 and 16,384 points (a smaller one is raised
    /// to the smallest window), a position that is not two finite numbers within ±16,384, a tab
    /// that is not one of the three (settings saved before T13b have none: the board).
    /// Whether the position is on screen is checked when the window opens.
    fn restore(&mut self, v: &Value) {
        self.detached = v.get("detached").and_then(Value::as_bool).unwrap_or(false);
        if let Some([w, h]) = pair(v.get("size")).filter(|[w, h]| (1.0..=MAX_SAVED).contains(w) && (1.0..=MAX_SAVED).contains(h)) {
            self.size = vec2(w, h).max(MIN_SIZE);
        }
        self.position = pair(v.get("position")).filter(|p| p.iter().all(|c| c.abs() <= MAX_SAVED)).map(|[x, y]| Pos2::new(x, y));
        if let Some(tab) = v.get("tab").and_then(Value::as_str).and_then(ChessWindowTab::from_key) {
            self.tab = tab;
        }
    }
}

/// Two finite numbers, `[a, b]`.
fn pair(v: Option<&Value>) -> Option<[f32; 2]> {
    let [a, b] = v?.as_array()?.as_slice() else { return None };
    let number = |v: &Value| v.as_f64().map(|n| n as f32).filter(|n| n.is_finite());
    Some([number(a)?, number(b)?])
}

impl PdfCraftApp {
    /// Whether the chess board is in its own window (or goes there at the start of the next
    /// frame).
    pub fn chess_window_detached(&self) -> bool {
        self.chess_window.request.unwrap_or(self.chess_window.detached)
    }

    /// The chess board window's size, in points (the last one shown, or the one it will open at).
    pub fn chess_window_size(&self) -> [f32; 2] {
        [self.chess_window.size.x, self.chess_window.size.y]
    }

    /// Whether the chess window was last drawn with the Diagrams and Text tabs beside the board.
    pub fn chess_window_wide(&self) -> bool {
        self.chess_window.wide
    }

    /// The chess window's tab: what a narrow window shows (a wide one shows the board, with Text
    /// beside it when that is the tab, else Diagrams).
    pub fn chess_window_tab(&self) -> ChessWindowTab {
        self.chess_window.tab
    }

    /// Show the chess panel's `tab`: in the Inspector, or in the chess window while the panel is
    /// there, where the Study board is the window's board.
    pub(crate) fn show_chess_tab(&mut self, tab: ChessTab) {
        if !self.chess_window_detached() {
            self.chess.tab = tab;
            return;
        }
        match tab {
            ChessTab::Study => self.chess_window.show_board(),
            ChessTab::Diagrams => self.chess_window.tab = ChessWindowTab::Diagrams,
            ChessTab::Text => self.chess_window.tab = ChessWindowTab::Text,
        }
    }

    /// Show the list of diagrams where it is: the Inspector's Chess panel, or the chess window's
    /// Diagrams tab when a narrow window shows the board. The Text tab stays, as in the Inspector.
    pub(crate) fn reveal_chess_diagrams(&mut self) {
        if self.chess_window_detached() {
            if self.chess_window.tab == ChessWindowTab::Board {
                self.chess_window.tab = ChessWindowTab::Diagrams;
            }
        } else if self.right != Some(RightPanel::Chess) {
            self.choose_right_panel(Some(RightPanel::Chess));
        }
    }

    /// Let the app learn of windows whose surface couldn't be configured: the desktop app passes
    /// the patched egui-wgpu's `take_surface_failures` when it draws with wgpu.
    pub fn watch_surface_failures(&mut self, take: SurfaceFailuresFn) {
        self.chess_window.surface_failures = Some(take);
    }

    /// Whether the board can be moved into a window of its own here: not where windows are drawn
    /// inside the main one (the web, tests), and not after one failed.
    pub(crate) fn chess_window_available(&self) -> bool {
        !self.chess_window.failed && self.ctx.as_ref().is_some_and(|ctx| !ctx.embed_viewports())
    }

    /// `chess.window.detach` (the rail's Own window tool, the Board tab's menu): the board into its own window, or
    /// back into the dock. Takes effect at the start of the next frame. Refused, with a notice,
    /// where no window can be opened.
    pub fn toggle_chess_window(&mut self) {
        let detach = !self.chess_window.request.unwrap_or(self.chess_window.detached);
        if detach && !self.chess_window_available() {
            self.notify_fmt("The chess board couldn't open in its own window, so it stays in its panel.", &[]);
            return;
        }
        self.chess_window.request = Some(detach);
    }

    /// Put the board in its own window (`true`) or back in the dock, at the start of the next
    /// frame, whatever the platform: where native windows can't be opened it is a window drawn
    /// inside the main one. For hosts and tests; the command and the rail offer it only where
    /// native windows can be opened.
    pub fn set_chess_window_detached(&mut self, detached: bool) {
        self.chess_window.request = Some(detached);
    }

    /// The board is in its own window: bring it to the front (next frame), showing the board,
    /// and say so.
    pub(crate) fn raise_chess_window(&mut self) -> bool {
        if self.chess_window.detached || self.chess_window.request == Some(true) {
            self.chess_window.raise = true;
            self.chess_window.show_board();
            return true;
        }
        false
    }

    fn detach_chess_board(&mut self) {
        if self.chess_window.detached {
            return;
        }
        // The panel's place is kept while it is closed, for docking back.
        if let Err(error) = self.docking.set_visible(Panel::Board, false) {
            self.notify_fmt("Panel move was not applied: {error}", &[("error", &error)]);
            return;
        }
        self.chess_window.detached = true;
        self.chess_window.opened = None;
    }

    fn dock_chess_board(&mut self) {
        if !self.chess_window.detached {
            return;
        }
        self.chess_window.detached = false;
        self.chess_window.opened = None;
        let panel = Panel::Board;
        if let Err(error) = self.docking.set_visible(panel, true).and_then(|()| self.docking.apply(Action::Activate { panel })) {
            self.notify_fmt("Panel move was not applied: {error}", &[("error", &error)]);
        }
    }

    /// The window failed: back in the dock for the rest of the session, said once.
    fn chess_window_failed(&mut self, why: &str) {
        log::error!("the chess board window failed, docking it back: {why}");
        self.dock_chess_board();
        self.chess_window.request = None;
        if !self.chess_window.failed {
            self.chess_window.failed = true;
            self.notify_fmt("The chess board couldn't open in its own window, so it is back in its panel.", &[]);
        }
    }

    /// Read `docking_detached` from saved settings (after the workspace: a board in a window is
    /// not also in the dock).
    pub(crate) fn restore_chess_window(&mut self, v: &Value) {
        self.chess_window.restore(v);
        if self.chess_window.detached
            && let Err(error) = self.docking.set_visible(Panel::Board, false)
        {
            log::warn!("the chess board window couldn't be restored: {error}");
            self.chess_window.detached = false;
        }
    }
}

/// The Board tab's context menu in the docking workspace: its own window.
pub(crate) fn dock_menu(app: &mut PdfCraftApp, ui: &mut egui::Ui, panel: Panel) {
    if panel == Panel::Board && app.chess_window_available() && ui.button(tl!("Chess in its own window")).clicked() {
        app.toggle_chess_window();
        ui.close();
    }
}

/// Every frame, from the main window's pass: what was asked of the window, then the window
/// itself while the board is out.
pub(crate) fn frame(app: &mut PdfCraftApp, ctx: &egui::Context) {
    app.chess_window.shown = false;
    // The app commands the PGN tool rails asked for in the last frame (the Own window tool).
    for id in app.chess.tools.take_commands() {
        app.execute(id);
    }
    if let Some(detach) = app.chess_window.request.take() {
        if detach {
            app.detach_chess_board();
        } else {
            app.dock_chess_board();
        }
    }
    if !app.chess_window.detached {
        app.chess_window.raise = false;
        return;
    }
    // Something opened the Board panel in the dock (a saved layout, the control channel): the
    // dock wins, so the board is never in two places.
    if app.docking.layout.contains(&Panel::Board) {
        app.chess_window.detached = false;
        app.chess_window.opened = None;
        return;
    }
    if ctx.embed_viewports() {
        embedded(app, ctx);
    } else {
        native(app, ctx);
    }
}

/// What the window's pass reported.
#[derive(Default)]
struct Pass {
    /// The user closed the window.
    close: bool,
    /// The board's code panicked (the message).
    panic: Option<String>,
    /// The window's inner rectangle and outer top-left corner on the desktop.
    inner: Option<egui::Rect>,
    outer: Option<Pos2>,
}

/// The native window.
fn native(app: &mut PdfCraftApp, ctx: &egui::Context) {
    let id = crate::viewports::chess();
    let (size, position) = *app.chess_window.opened.get_or_insert_with(|| open_geometry(ctx, app.chess_window.size, app.chess_window.position));
    let mut builder = egui::ViewportBuilder::default().with_title(tl!("Chess")).with_inner_size(size).with_min_inner_size(MIN_SIZE);
    if let Some(p) = position {
        builder = builder.with_position(p);
    }
    if std::mem::take(&mut app.chess_window.raise) {
        ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Focus);
    }
    let mut pass = Pass::default();
    let shown = pdfcraft_engine::guard(|| {
        ctx.show_viewport_immediate(id, builder, |ui, _class| {
            pass.close = ui.input(|i| i.viewport().close_requested());
            pass.inner = ui.input(|i| i.viewport().inner_rect);
            pass.outer = ui.input(|i| i.viewport().outer_rect.map(|r| r.min));
            // The chess keys first: the window's keys are its own, and the app's shortcuts
            // (Delete on a selected annotation) must not take them.
            let main = crate::chess_tools::take_window_keys(&mut app.chess, ui.ctx());
            // Caught here, so the window's pass always ends and egui's state stays whole.
            if let Err(m) = pdfcraft_engine::guard(|| {
                // The app's shortcuts read this window's keys while it has the keyboard.
                app.shortcuts_in_pass(ui.ctx());
                contents(app, ui);
            }) {
                pass.panic = Some(m);
            }
            crate::chess_tools::end_window_keys(&mut app.chess, main);
        });
    });
    if let Err(m) = shown {
        app.chess_window_failed(&m);
        return;
    }
    app.chess_window.shown = true;
    let failed_surface = app.chess_window.surface_failures.as_mut().is_some_and(|take| take().contains(&id));
    if failed_surface {
        app.chess_window_failed("its surface couldn't be configured");
        recover_textures(app, ctx);
        return;
    }
    if let Some(m) = pass.panic {
        log::error!("the chess board window's contents failed: {m}");
        app.notify_fmt("That didn't work: an internal error stopped it ({m}). Your documents are unchanged.", &[("m", m.as_str())]);
        app.dock_chess_board();
        return;
    }
    if let Some(inner) = pass.inner.filter(|r| r.is_finite() && r.width() >= 1.0 && r.height() >= 1.0) {
        app.chess_window.size = inner.size().max(MIN_SIZE);
    }
    if let Some(outer) = pass.outer.filter(|p| p.is_finite()) {
        app.chess_window.position = Some(outer);
    }
    if pass.close {
        app.dock_chess_board();
    }
}

/// The size and position to open the window at: the saved size within the main window's
/// monitor, and the saved position only if the window would be on that monitor. egui knows only
/// that monitor's size, not where it is on the desktop, so a position is used only when the
/// main window is on the monitor at the desktop's origin (the primary one) and the position is
/// too; otherwise the system places the window.
fn open_geometry(ctx: &egui::Context, size: Vec2, position: Option<Pos2>) -> (Vec2, Option<Pos2>) {
    let (monitor, main) = ctx.input(|i| (i.viewport().monitor_size, i.viewport().outer_rect));
    let Some(monitor) = monitor.filter(|m| m.is_finite() && m.x >= 1.0 && m.y >= 1.0) else { return (size, None) };
    let screen = egui::Rect::from_min_size(Pos2::ZERO, monitor);
    let size = size.min(monitor).max(MIN_SIZE);
    let main_here = main.is_none_or(|r| screen.contains(r.center()));
    let position = position.filter(|p| main_here && screen.contains(*p) && screen.contains(*p + ON_SCREEN));
    (size, position)
}

/// The window drawn inside the main one, where native windows aren't available (the web,
/// kittest, the `shot` example).
fn embedded(app: &mut PdfCraftApp, ctx: &egui::Context) {
    let mut open = true;
    let id = egui::Id::new("pdfcraft-chess-window");
    if std::mem::take(&mut app.chess_window.raise) {
        ctx.move_to_top(egui::LayerId::new(egui::Order::Middle, id));
    }
    // At first on the right, over the Inspector (whose Chess panel now only says where its tabs
    // are), leaving the page in view. `default_size` is the whole window's, title bar included:
    // so is the size kept.
    let screen = ctx.content_rect();
    let right = Pos2::new(screen.right() - app.chess_window.size.x - 8.0, screen.top() + 96.0).max(screen.left_top());
    let window = egui::Window::new(tl!("Chess"))
        .id(id)
        .default_pos(if right.is_finite() { right } else { Pos2::ZERO })
        .default_size(app.chess_window.size)
        .min_size(MIN_SIZE)
        .resizable(true)
        .collapsible(false)
        .open(&mut open)
        .show(ctx, |ui| contents(app, ui));
    app.chess_window.shown = true;
    if let Some(s) = window.map(|w| w.response.rect.size()).filter(|s| s.is_finite() && s.x >= 1.0 && s.y >= 1.0) {
        app.chess_window.size = s.max(MIN_SIZE);
    }
    if !open {
        app.dock_chess_board();
    }
}

/// The open document, for the Diagrams and Text tabs.
struct OpenDoc<'a> {
    id: pdfcraft_engine::DocId,
    info: &'a pdfcraft_render::DocInfo,
    stamp: Option<pdfcraft_engine::chess::ScanStamp>,
    page: usize,
}

/// What the window's contents asked for, done once they are drawn.
#[derive(Default)]
struct Asked {
    board: Option<chess_board_ui::BoardAction>,
    games: Option<crate::chess_collection::GamesAction>,
    panel: Option<crate::chess_ui::PanelAction>,
    dock: bool,
    /// Paste as ▸: the clipboard's text to capture, in this mode (T7a).
    paste: Option<(pdfcraft_engine::chess::capture::CaptureMode, bool)>,
}

/// The window's contents: the PGN tool rail, a row with the Dock button (and a narrow window's
/// tab strip), then the board panel and the Diagrams or Text tab: side by side from 760 pt wide,
/// else the tab chosen. The chess keys reach the rail from the window's own input in a native
/// window (`take_window_keys`), and through the main window's key scope in an embedded one.
fn contents(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    // A click anywhere in the window puts Undo and Redo on the game (D12).
    if crate::chess_history::pressed_in(ui) {
        app.chess.enter_scope();
    }
    let pieces = crate::chess_editor::board_piece_set(&mut app.chess.board_pieces);
    let source_label = crate::chess_ui::board_source_label(app);
    let open = app.active_ids().and_then(|(vi, id)| {
        let page = app.views.get(vi)?.current;
        let doc = app.session.get(id)?;
        Some(OpenDoc { id, info: &doc.info, stamp: app.session.chess_stamp(id), page })
    });
    let mut tab = app.chess_window.tab;
    let mut asked = Asked::default();
    let mut wide = false;
    egui::Frame::NONE.fill(t.panel).show(ui, |ui| {
        crate::chess_tools::with_rail(ui, &mut app.chess, |ui, chess| {
            egui::Frame::NONE.inner_margin(egui::Margin { left: 10, right: 12, top: 8, bottom: 8 }).show(ui, |ui| {
                wide = ui.available_width() >= WIDE_FROM;
                // One row: a narrow window's tabs on the left, the Dock button on the right.
                ui.allocate_ui_with_layout(vec2(ui.available_width(), 24.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    asked.dock = ui.button(tl!("Dock")).on_hover_text(tl!("Put the board and the chess panel back in the main window")).clicked();
                    if !wide {
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.selectable_value(&mut tab, ChessWindowTab::Board, tl!("Board"));
                            ui.selectable_value(&mut tab, ChessWindowTab::Diagrams, tl!("Diagrams"));
                            ui.selectable_value(&mut tab, ChessWindowTab::Text, tl!("Text"));
                        });
                    }
                });
                if !wide {
                    match tab {
                        ChessWindowTab::Board => board_column(ui, &t, chess, &pieces, source_label, &mut asked),
                        ChessWindowTab::Diagrams | ChessWindowTab::Text => {
                            side_tab(ui, &t, chess, tab == ChessWindowTab::Text, open.as_ref(), &mut asked)
                        }
                    }
                    return;
                }
                // Wide: the board panel on the left, the Diagrams or Text tab on the right.
                let room = ui.available_width();
                let tabs = (room * TABS_SHARE).clamp(TABS_MIN, TABS_MAX);
                // The separator and the spacing on either side of it.
                let board = (room - tabs - 2.0 * ui.spacing().item_spacing.x - 1.0).max(160.0);
                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(vec2(board, ui.available_height()), egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.set_width(board);
                        board_column(ui, &t, chess, &pieces, source_label, &mut asked);
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        let text = tab == ChessWindowTab::Text;
                        ui.horizontal(|ui| {
                            if ui.selectable_label(!text, tl!("Diagrams")).clicked() {
                                tab = ChessWindowTab::Diagrams;
                            }
                            if ui.selectable_label(text, tl!("Text")).clicked() {
                                tab = ChessWindowTab::Text;
                            }
                        });
                        ui.add_space(4.0);
                        side_tab(ui, &t, chess, text, open.as_ref(), &mut asked);
                    });
                });
            });
        });
    });
    app.chess_window.wide = wide;
    if let Some(mode) = asked.paste {
        app.chess_paste_as(mode, crate::chess_capture::clipboard_text());
    }
    app.chess_window.tab = tab;
    if asked.dock {
        app.chess_window.request = Some(false);
    }
    if let Some(a) = asked.board {
        app.chess_board_action(a);
    }
    if let Some(a) = asked.games {
        app.chess_games_action(a);
    }
    if let Some(a) = asked.panel {
        app.chess_action(a);
    }
}

/// The Board panel within the room it has: board, moves, notes and fields in one column, or
/// from 760 pt wide the moves, notes and fields beside the board (§4.1).
fn board_column(
    ui: &mut egui::Ui,
    t: &Tokens,
    chess: &mut crate::chess_ui::ChessState,
    pieces: &pdfcraft_engine::chess::pdfcraft_chess::draw::PieceSet,
    source_label: Option<String>,
    asked: &mut Asked,
) {
    // Captures from the clipboard (T7a) and the layer of captured text (T7b), above the board, as in the docked panel.
    asked.paste = crate::chess_capture::panel_row(ui, chess);
    let sizing = sizing(ui.available_size());
    if !sizing.wide {
        // The notation takes the height the scroll area shows and the rest leaves.
        let viewport = ui.available_height();
        egui::ScrollArea::vertical().id_salt("chess-window-panel").auto_shrink([false, false]).show(ui, |ui| {
            (asked.board, asked.games) = chess_board_ui::panel_body(ui, t, chess, pieces, source_label, sizing, viewport);
        });
        return;
    }
    let side = sizing.max_side;
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(vec2(side, ui.available_height()), egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.set_width(side);
            let state = chess.board_mut();
            asked.board = chess_board_ui::title(ui, state, source_label);
            ui.add_space(4.0);
            chess_board_ui::view_controls(ui, state);
            ui.add_space(4.0);
            chess_board_ui::board_view_sized(ui, t, state, pieces, &[], sizing);
            chess_board_ui::promotion_choice(ui, state);
            chess_board_ui::navigation(ui, t, state);
        });
        ui.separator();
        ui.vertical(|ui| {
            // The notation fills the column's height, less what goes under it.
            let room = ui.available_height();
            egui::ScrollArea::vertical().id_salt("chess-window-notes").auto_shrink([false, false]).show(ui, |ui| {
                let (fields, listed) = chess_board_ui::moves_column(ui, t, chess, room);
                asked.games = listed;
                asked.board = asked.board.take().or(fields);
            });
        });
    });
}

/// The chess panel's Text tab (`text`) or Diagrams tab, for the open document.
fn side_tab(ui: &mut egui::Ui, t: &Tokens, chess: &mut crate::chess_ui::ChessState, text: bool, open: Option<&OpenDoc<'_>>, asked: &mut Asked) {
    egui::ScrollArea::vertical().id_salt(("chess-window-tab", text)).auto_shrink([false, false]).show(ui, |ui| {
        let Some(d) = open else {
            ui.label(egui::RichText::new(tl!("Open a document first")).color(t.text_muted));
            return;
        };
        asked.panel = crate::chess_ui::tab_contents(ui, t, chess, text, d.id, d.info, d.stamp, d.page);
    });
}

/// How the board fits a window whose contents have `room`: as tall as the window allows under
/// its controls, at most 1,200 pt; from 760 pt wide, beside a column of at least 300 pt.
fn sizing(room: Vec2) -> Sizing {
    let room = if room.is_finite() { room } else { DEFAULT_SIZE };
    let wide = room.x >= WIDE_FROM;
    let across = if wide { room.x - NOTES_MIN } else { room.x };
    // `room` is finite here, and the bounds are constants with 160 < 1,200: `clamp` cannot panic.
    let side = (room.y - CONTROLS).min(across).clamp(160.0, MAX_SIDE);
    Sizing { max_side: side, max_px: MAX_PX, wide }
}

/// After a window's pass whose texture changes were never uploaded: install the fonts again (a
/// new font atlas, uploaded whole) and load the board's pictures and the icons again.
fn recover_textures(app: &mut PdfCraftApp, ctx: &egui::Context) {
    let mut fonts = crate::theme::installed_font_definitions(app.fonts_cjk);
    // `set_fonts` ignores definitions equal to the current ones: a face no family uses makes
    // these differ (each refresh flips it), so the atlas is built and uploaded anew.
    if let Some(data) = fonts.font_data.values().next().cloned() {
        app.chess_window.refreshes = app.chess_window.refreshes.wrapping_add(1);
        if app.chess_window.refreshes % 2 == 1 {
            fonts.font_data.insert("pdfcraft-atlas-refresh".to_owned(), data);
        }
    }
    ctx.set_fonts(fonts);
    app.chess.board_mut().release_textures();
    ctx.forget_all_images();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_settings_round_trip_and_bad_values_are_ignored() {
        let mut w = ChessWindow {
            detached: true,
            size: vec2(900.0, 700.0),
            position: Some(Pos2::new(40.0, 60.0)),
            tab: ChessWindowTab::Text,
            ..ChessWindow::default()
        };
        let saved = w.to_json();
        let mut back = ChessWindow::default();
        back.restore(&saved);
        assert!(back.detached);
        assert_eq!(back.size, vec2(900.0, 700.0));
        assert_eq!(back.position, Some(Pos2::new(40.0, 60.0)));
        assert_eq!(back.tab, ChessWindowTab::Text);
        // A tab not one of the three, or none (saved before T13b): the board.
        for tab in [json!("Text"), json!("moves"), json!(1), json!(null)] {
            back = ChessWindow::default();
            back.restore(&json!({"detached": true, "tab": tab}));
            assert_eq!(back.tab, ChessWindowTab::Board, "{tab}");
        }
        // Each bad value is ignored on its own; the others are kept.
        for (bad, size, position) in [
            (json!({"detached": true, "size": [-5.0, 700.0], "position": [1.0, 2.0]}), DEFAULT_SIZE, Some(Pos2::new(1.0, 2.0))),
            (json!({"detached": true, "size": [1e9, 700.0], "position": null}), DEFAULT_SIZE, None),
            (json!({"detached": true, "size": ["wide", 700.0], "position": [1e30, 2.0]}), DEFAULT_SIZE, None),
            (json!({"detached": true, "size": [800.0], "position": [1.0, 2.0, 3.0]}), DEFAULT_SIZE, None),
            (json!({"detached": true, "size": [10.0, 10.0], "position": "here"}), MIN_SIZE, None),
        ] {
            w = ChessWindow::default();
            w.restore(&bad);
            assert!(w.detached);
            assert_eq!((w.size, w.position), (size, position), "{bad}");
        }
        // Not an object at all: nothing out, nothing changed.
        for bad in [json!(null), json!(7), json!("detached"), json!({"detached": "yes"})] {
            w = ChessWindow::default();
            w.restore(&bad);
            assert!(!w.detached, "{bad}");
            assert_eq!(w.size, DEFAULT_SIZE);
        }
    }

    /// A pass of `app`'s chess window over `ctx`, as `ui()` runs it.
    fn pass(ctx: &egui::Context, app: &mut PdfCraftApp) {
        ctx.run_ui(egui::RawInput::default(), |ui| frame(app, ui.ctx())).textures_delta.clear();
    }

    /// The patched egui-wgpu records a window whose surface it couldn't configure, which eframe
    /// only logs: the app docks the board back where it was, says so once, and opens no window
    /// again. (A context that draws native windows, without a backend: egui draws the window's
    /// pass inside the main one, as a backend without windows does.)
    #[test]
    fn a_window_without_a_surface_docks_back_once_with_a_notice() {
        let ctx = egui::Context::default();
        ctx.set_embed_viewports(false);
        crate::theme::install_fonts(&ctx);
        ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.ctx = Some(ctx.clone());
        app.docking.set_visible(Panel::Board, true).unwrap();
        let place = app.docking.layout.location(&Panel::Board).unwrap();
        let asked = std::rc::Rc::new(std::cell::Cell::new(0));
        let counter = std::rc::Rc::clone(&asked);
        app.watch_surface_failures(Box::new(move || {
            counter.set(counter.get() + 1);
            vec![ViewportId::from_hash_of("another window"), crate::viewports::chess()]
        }));
        assert!(app.chess_window_available());
        app.toggle_chess_window();
        pass(&ctx, &mut app);
        assert_eq!(asked.get(), 1, "asked right after the window's pass");
        assert!(!app.chess_window_detached(), "docked back");
        assert_eq!(app.docking.layout.location(&Panel::Board).ok(), Some(place), "where it was");
        let notice = app.toast.take().map(|t| t.0).unwrap_or_default();
        assert!(notice.contains("couldn't open in its own window"), "{notice}");
        // No second try in this session: the command is refused and says why.
        assert!(!app.chess_window_available());
        app.toggle_chess_window();
        pass(&ctx, &mut app);
        assert_eq!(asked.get(), 1, "no window was opened");
        assert!(!app.chess_window_detached());
        assert!(app.toast.take().is_some_and(|t| t.0.contains("stays in its panel")));
    }

    /// A window whose surface works stays open, and only a failure of its own counts.
    #[test]
    fn another_windows_surface_failure_leaves_the_board_out() {
        let ctx = egui::Context::default();
        ctx.set_embed_viewports(false);
        crate::theme::install_fonts(&ctx);
        ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.ctx = Some(ctx.clone());
        app.watch_surface_failures(Box::new(|| vec![ViewportId::from_hash_of("another window")]));
        app.toggle_chess_window();
        pass(&ctx, &mut app);
        pass(&ctx, &mut app);
        assert!(app.chess_window_detached());
        assert!(!app.docking.layout.contains(&Panel::Board));
        assert!(app.toast.is_none(), "{:?}", app.toast);
    }

    #[test]
    fn the_board_grows_with_the_window_and_sits_beside_the_moves_when_wide() {
        // Narrow: one column, the board as tall as the window allows under its controls.
        let s = sizing(vec2(600.0, 900.0));
        assert!(!s.wide);
        assert_eq!(s.max_side, 600.0);
        assert_eq!(s.max_px, MAX_PX);
        // Wide: beside a 300 pt column, never above 1,200 pt.
        let s = sizing(vec2(1000.0, 900.0));
        assert!(s.wide);
        assert_eq!(s.max_side, 700.0);
        assert_eq!(sizing(vec2(4000.0, 3000.0)).max_side, MAX_SIDE);
        // Tiny, or not a size at all: the smallest board, and nothing that could make `clamp` panic.
        assert_eq!(sizing(vec2(10.0, 10.0)).max_side, 160.0);
        assert!(sizing(vec2(f32::NAN, f32::INFINITY)).max_side.is_finite());
    }
}
