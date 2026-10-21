//! The chess window (PGN toolbar spec T13a, T13b, §4.8, D8 b).
//!
//! `chess.window.detach` moves the Board panel out of the docking workspace into a window of its
//! own and back. While it is out, the panel is closed in the workspace with its place kept (a
//! hidden `Location`), so its slot collapses and docking it again puts it back where it was, in
//! front only if it was then (and never in front of the Inspector after its Bring it back); the
//! Read-mode board panel, which shows only while the panel is in the workspace, is gone too.
//!
//! Geometry (wave 10, U5): the size kept is the inner size in points, what the contents get, for
//! the native window and the embedded one alike; the native window's place is kept in pixels of
//! the desktop with its monitor, and opens by the rules of [`crate::window_place`].
//!
//! The window holds the Inspector Chess panel's Diagrams and Text tabs too (T13b): from 760 pt
//! wide beside the board, narrower behind a Board | Diagrams | Text strip. Meanwhile the
//! Inspector's panel only says where they are, with a button that docks the window, so no frame
//! draws them twice (both views act on the same `ChessState`). The board is the study board
//! (T5c): set-up, analysis and the evaluation bar come with it. What picks a tab for the user (a
//! diagram picked on the page, a scan) picks the window's while it is out
//! ([`PdfCraftApp::show_chess_tab`], [`PdfCraftApp::reveal_chess_diagrams`]); a diagram opened on
//! the board shows the window's board ([`PdfCraftApp::show_chess_board`]).
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
use egui::{Pos2, Rect, Vec2, ViewportId, vec2};
use serde_json::{Value, json};

use crate::chess_board_ui::{self, Sizing};
use crate::chess_text_ui::ChessTab;
use crate::docking::Panel;
use crate::theme::Tokens;
use crate::window_place::{MainWindow, Monitor, Place, Target};
use crate::{PdfCraftApp, RightPanel};

/// The window's inner size the first time, in points: wide enough for the tabs beside the board.
const DEFAULT_SIZE: Vec2 = vec2(960.0, 760.0);
/// The smallest window (inner size).
const MIN_SIZE: Vec2 = vec2(320.0, 360.0);
/// Saved sizes beyond this many points are not believed.
const MAX_SAVED: f32 = 16_384.0;
/// The monitors are read again at most this often while the window is out, in seconds.
#[cfg(not(target_arch = "wasm32"))]
const MONITORS_EVERY: f64 = 1.0;
/// A native window that opened more than this many pixels from where it was asked to is moved
/// there once (see [`Opened`]).
const OFF_BY: f32 = 2.0;
/// What an embedded window's frame and title bar add to its inner size is believed up to this
/// many points each way.
const MAX_CHROME: f32 = 400.0;
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

/// How a window was opened: the viewport builder stays the same while it is shown, or eframe
/// would resize the window to it on every change.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Opened {
    /// The inner size asked for, in points.
    inner: Vec2,
    /// Native: where it was asked to go (none: where the system puts it).
    target: Option<Target>,
    /// Native: its place was compared with `target` once it showed, and corrected if it was off
    /// (the position given at creation is converted at the scale of the monitor the system first
    /// makes the window on, which may not be the one it goes to). Embedded: what its frame and
    /// title bar add was measured.
    checked: bool,
    /// Embedded: the window was made again once with the measured frame (its first size was
    /// off), and is not made again.
    retried: bool,
}

impl Opened {
    fn new(inner: Vec2, target: Option<Target>) -> Opened {
        Opened { inner, target, checked: false, retried: false }
    }
}

/// The window as it showed in the last frame, for the control channel (`ui.state.chess_window`).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Shown {
    native: bool,
    /// Its outer rectangle, in points of its own (native) or of the main window (embedded).
    outer: Rect,
    /// What its contents got, in points.
    inner: Vec2,
    pixels_per_point: f32,
    /// Native only: its monitor's scale, and the monitor's size in points.
    native_pixels_per_point: Option<f32>,
    monitor_size: Option<Vec2>,
}

impl Shown {
    fn to_json(self) -> Value {
        let px = Rect::from_min_size(self.outer.min * self.pixels_per_point, self.outer.size() * self.pixels_per_point);
        json!({
            "native": self.native,
            "outer_rect": [self.outer.min.x, self.outer.min.y, self.outer.max.x, self.outer.max.y],
            "inner_size": [self.inner.x, self.inner.y],
            "pixels_per_point": self.pixels_per_point,
            "native_pixels_per_point": self.native_pixels_per_point,
            "monitor_size": self.monitor_size.map(|m| [m.x, m.y]),
            // On the desktop, in pixels (a native window's only).
            "outer_px": (self.native && px.is_finite()).then(|| [px.min.x, px.min.y, px.width(), px.height()]),
        })
    }
}

/// The chess board window's state: whether it is out, its geometry, and what was asked of it.
pub struct ChessWindow {
    detached: bool,
    /// The window's inner size, in points: what its contents get, native or embedded (so going
    /// from one to the other keeps it). Kept up to date while it is shown, and saved.
    size: Vec2,
    /// Where the native window is on the desktop, and its monitor (saved).
    place: Option<Place>,
    /// The board was the front tab of its dock group when it left: docking back puts it in front
    /// again only then (saved).
    docked_front: bool,
    /// Docking back keeps the front tab of the group the board goes into (the Inspector's Bring
    /// it back: the user is looking at the Inspector).
    dock_behind: bool,
    /// How the window was opened, while it is out.
    opened: Option<Opened>,
    /// The desktop's monitors (desktop only), and when they were read (input time).
    monitors: Vec<Monitor>,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    monitors_read: Option<f64>,
    /// Embedded: what the frame and title bar add to the inner size, as measured (none yet:
    /// estimated), and the window's generation (part of its egui id: a window made again is new).
    chrome: Option<Vec2>,
    generation: u32,
    /// The window as it showed in the last frame it was drawn.
    last_shown: Option<Shown>,
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
            place: None,
            docked_front: true,
            dock_behind: false,
            opened: None,
            monitors: Vec::new(),
            monitors_read: None,
            chrome: None,
            generation: 0,
            last_shown: None,
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
            .field("place", &self.place)
            .field("docked_front", &self.docked_front)
            .field("failed", &self.failed)
            .field("wide", &self.wide)
            .field("tab", &self.tab)
            .finish_non_exhaustive()
    }
}

impl ChessWindow {
    /// The settings value (`docking_detached`): `{detached, size: [w, h], place: {outer: [x, y,
    /// w, h], scale, monitor: {rect: [x, y, w, h], scale} | null} | null, docked_front, tab:
    /// "board" | "diagrams" | "text"}`. `size` is the inner size in points, `place` is in pixels
    /// of the desktop ([`crate::window_place`]).
    pub(crate) fn to_json(&self) -> Value {
        json!({
            "detached": self.detached,
            "size": [self.size.x, self.size.y],
            "place": self.place.map(Place::to_json),
            "docked_front": self.docked_front,
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
    /// to the smallest window), a place [`Place::from_json`] refuses, a `docked_front` that is not
    /// a boolean (in front), a tab that is not one of the three (settings saved before T13b have
    /// none: the board). The `position` of settings saved before U5 (points of whichever monitor
    /// the window was on) is not used: such a window opens beside the main window once. Whether
    /// the place is on a monitor is checked when the window opens.
    fn restore(&mut self, v: &Value) {
        self.detached = v.get("detached").and_then(Value::as_bool).unwrap_or(false);
        if let Some([w, h]) = pair(v.get("size")).filter(|[w, h]| (1.0..=MAX_SAVED).contains(w) && (1.0..=MAX_SAVED).contains(h)) {
            self.size = vec2(w, h).max(MIN_SIZE);
        }
        self.place = v.get("place").and_then(Place::from_json);
        self.docked_front = v.get("docked_front").and_then(Value::as_bool).unwrap_or(true);
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
    /// there. The board is not a tab of the panel: [`PdfCraftApp::show_chess_board`] shows it.
    pub(crate) fn show_chess_tab(&mut self, tab: ChessTab) {
        if !self.chess_window_detached() {
            self.chess.tab = tab;
            return;
        }
        self.chess_window.tab = match tab {
            ChessTab::Diagrams => ChessWindowTab::Diagrams,
            ChessTab::Text => ChessWindowTab::Text,
        };
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

    /// The Inspector's Bring it back: the board back in the dock at the start of the next frame,
    /// in its place but not in front of the Inspector the user is looking at (whose Chess panel
    /// gets its Diagrams and Text back).
    pub(crate) fn bring_back_chess_window(&mut self) {
        self.chess_window.request = Some(false);
        self.chess_window.dock_behind = true;
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
        self.chess_window.dock_behind = false;
        if self.chess_window.detached {
            return;
        }
        let board = Panel::Board;
        let front = !self.docking.layout.contains(&board) || self.docking.exposed(board);
        // The panel's place (its group, and where in it) is kept while it is closed, as a hidden
        // `Location` of the workspace, for docking back.
        if let Err(error) = self.docking.set_visible(board, false) {
            self.notify_fmt("Panel move was not applied: {error}", &[("error", &error)]);
            return;
        }
        self.chess_window.docked_front = front;
        self.chess_window.detached = true;
        self.chess_window.opened = None;
    }

    /// The board back in the dock where it was when it left (the workspace's hidden `Location`:
    /// its group, and its place in it; if that group is gone, at the end of the Inspector's
    /// group or else right of the page, as when it is first opened). It is the front tab of its
    /// group again only if it was when it left, and never in front of the Inspector after Bring
    /// it back (pressed there, where the user is looking); every other group keeps its front tab.
    fn dock_chess_board(&mut self) {
        let behind_inspector = std::mem::take(&mut self.chess_window.dock_behind);
        if !self.chess_window.detached {
            return;
        }
        self.chess_window.detached = false;
        self.chess_window.opened = None;
        self.chess_window.last_shown = None;
        let panel = Panel::Board;
        let front = self.chess_window.docked_front;
        // The tabs in front now that keep the front.
        let keep: Vec<Panel> = Panel::ALL
            .iter()
            .copied()
            .filter(|p| *p != panel && self.docking.exposed(*p) && (!front || (behind_inspector && *p == Panel::Inspector)))
            .collect();
        let docking = &mut self.docking;
        let docked = docking.set_visible(panel, true).and_then(|()| {
            if front {
                docking.apply(Action::Activate { panel })?;
            }
            // Restoring makes the board its group's front tab: the front goes back where it stays.
            for p in keep {
                if !docking.exposed(p) {
                    docking.apply(Action::Activate { panel: p })?;
                }
            }
            Ok(())
        });
        if let Err(error) = docked {
            self.notify_fmt("Panel move was not applied: {error}", &[("error", &error)]);
        }
    }

    /// Read the desktop's monitors while the board is out in a native window: when it opens, then
    /// at most once a second (it may have moved to another one). From `logic`, which has eframe's
    /// main window.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn read_chess_window_monitors(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        let w = &mut self.chess_window;
        if !w.request.unwrap_or(w.detached) || ctx.embed_viewports() {
            return;
        }
        let now = ctx.input(|i| i.time);
        let fresh = w.monitors_read.is_some_and(|t| (t..t + MONITORS_EVERY).contains(&now));
        if w.opened.is_some() && fresh {
            return;
        }
        if let Some(monitors) = crate::window_place::read_monitors(frame) {
            w.monitors = monitors;
        }
        w.monitors_read = Some(now);
    }

    /// The chess window's geometry for the control channel (`ui.state.chess_window`), so a check
    /// across monitors can be recorded by script: whether it is out and how it is drawn, its inner
    /// size (points, saved), its saved place and the place it was opened at (pixels of the
    /// desktop), the monitors read, and the window as it last showed (outer rectangle in points,
    /// pixels per point, its monitor's scale and size, its outer rectangle in pixels).
    pub(crate) fn chess_window_report(&self) -> Value {
        let w = &self.chess_window;
        json!({
            "detached": self.chess_window_detached(),
            "embedded": self.ctx.as_ref().is_none_or(egui::Context::embed_viewports),
            "size": [w.size.x, w.size.y],
            "place": w.place.map(Place::to_json),
            "opened_at": w.opened.and_then(|o| o.target).map(Target::to_json),
            "shown": w.last_shown.map(Shown::to_json),
            "monitors": w.monitors.iter().map(|m| m.to_json()).collect::<Vec<_>>(),
            "docked_front": w.docked_front,
        })
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
    // The engine rests while the board is not drawn, here or in the dock (T5c).
    app.chess.study.new_frame();
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
        app.chess_window.last_shown = None;
        app.chess_window.dock_behind = false;
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
    /// The window's inner and outer rectangles on the desktop, in its points.
    inner: Option<Rect>,
    outer: Option<Rect>,
    /// Its pixels per point, its monitor's scale (without the zoom) and its monitor's size.
    pixels_per_point: f32,
    native_pixels_per_point: Option<f32>,
    monitor_size: Option<Vec2>,
}

/// The native window.
fn native(app: &mut PdfCraftApp, ctx: &egui::Context) {
    let id = crate::viewports::chess();
    let zoom = usable_zoom(ctx.zoom_factor());
    let opened = match app.chess_window.opened {
        Some(opened) => opened,
        None => *app.chess_window.opened.insert(open_native(ctx, &app.chess_window)),
    };
    let mut builder = egui::ViewportBuilder::default().with_title(tl!("Chess")).with_inner_size(opened.inner).with_min_inner_size(MIN_SIZE);
    if let Some(p) = opened.target.and_then(|t| t.pos_in_points(t.scale * zoom)) {
        builder = builder.with_position(p);
    }
    if std::mem::take(&mut app.chess_window.raise) {
        ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Focus);
    }
    let mut pass = Pass::default();
    let shown = pdfcraft_engine::guard(|| {
        ctx.show_viewport_immediate(id, builder, |ui, _class| {
            ui.input(|i| {
                pass.close = i.viewport().close_requested();
                pass.inner = i.viewport().inner_rect;
                pass.outer = i.viewport().outer_rect;
                pass.pixels_per_point = i.pixels_per_point;
                pass.native_pixels_per_point = i.viewport().native_pixels_per_point;
                pass.monitor_size = i.viewport().monitor_size;
            });
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
    keep_native_geometry(&mut app.chess_window, ctx, &pass, zoom);
    if pass.close {
        app.dock_chess_board();
    }
}

/// After a native window's pass: its inner size and its place on the desktop are kept (saved),
/// and the first time it shows, a window that isn't where it was asked to go is moved there.
fn keep_native_geometry(w: &mut ChessWindow, ctx: &egui::Context, pass: &Pass, zoom: f32) {
    let inner = pass.inner.filter(|r| r.is_finite() && r.width() >= 1.0 && r.height() >= 1.0);
    if let Some(inner) = inner {
        w.size = inner.size().max(MIN_SIZE);
    }
    let ppp = pass.pixels_per_point;
    let Some(outer) = pass.outer.filter(|r| r.is_finite() && r.is_positive()) else { return };
    if !(ppp.is_finite() && ppp > 0.0) {
        return;
    }
    let scale = pass.native_pixels_per_point.filter(|s| s.is_finite() && *s > 0.0).unwrap_or(ppp / zoom);
    if let Some(place) = Place::of(outer, ppp, scale, &w.monitors) {
        w.place = Some(place);
    }
    w.last_shown = Some(Shown {
        native: true,
        outer,
        inner: inner.map_or(w.size, |r| r.size()),
        pixels_per_point: ppp,
        native_pixels_per_point: pass.native_pixels_per_point,
        monitor_size: pass.monitor_size,
    });
    let Some(opened) = w.opened.as_mut().filter(|o| !o.checked) else { return };
    opened.checked = true;
    // The position given at creation is converted at the scale of the monitor the system first
    // made the window on: on another monitor's scale it lands elsewhere. Moved once, in this
    // window's points now.
    if let Some(at) = opened.target.and_then(|t| t.pos_in_points(ppp))
        && (outer.min - at).length() * ppp > OFF_BY
    {
        ctx.send_viewport_cmd_to(crate::viewports::chess(), egui::ViewportCommand::OuterPosition(at));
    }
}

/// How to open the native window ([`crate::window_place::target`]): at its saved place on the
/// monitor it was on, else beside the main window, with the saved inner size shrunk to fit. When
/// neither the monitors nor the main window are known, the saved size within the main window's
/// monitor, where the system puts it.
fn open_native(ctx: &egui::Context, w: &ChessWindow) -> Opened {
    let zoom = usable_zoom(ctx.zoom_factor());
    let (outer, ppp, native, monitor) =
        ctx.input(|i| (i.viewport().outer_rect, i.pixels_per_point, i.viewport().native_pixels_per_point, i.viewport().monitor_size));
    let scale = native.filter(|s| s.is_finite() && *s > 0.0).unwrap_or(ppp / zoom);
    let main = outer.and_then(|r| MainWindow::of(r, ppp, scale));
    if let Some(target) = crate::window_place::target(w.place.as_ref(), w.size, MIN_SIZE, zoom, &w.monitors, main.as_ref()) {
        return Opened::new(target.inner, Some(target));
    }
    let size = monitor.filter(|m| m.is_finite() && m.x >= 1.0 && m.y >= 1.0).map_or(w.size, |m| w.size.min(m)).max(MIN_SIZE);
    Opened { checked: true, ..Opened::new(size, None) }
}

fn usable_zoom(zoom: f32) -> f32 {
    if zoom.is_finite() && zoom > 0.0 { zoom } else { 1.0 }
}

/// What an embedded window's frame and title bar add to its inner size, before it has been
/// measured: an estimate from the style (the first window is made again, once, if it is off).
fn chrome_estimate(ctx: &egui::Context) -> Vec2 {
    let margin = ctx.global_style().spacing.window_margin.sum();
    let estimate = margin * 2.0 + vec2(2.0, 30.0);
    if estimate.is_finite() { estimate.clamp(Vec2::ZERO, Vec2::splat(MAX_CHROME)) } else { vec2(16.0, 40.0) }
}

/// The window drawn inside the main one, where native windows aren't available (the web,
/// kittest, the `shot` example). Its size is kept as the native window's is, as what its
/// contents get: egui's window sizes are outer sizes (frame and title bar included), so what
/// those add is measured and added.
fn embedded(app: &mut PdfCraftApp, ctx: &egui::Context) {
    let mut open = true;
    let w = &mut app.chess_window;
    let wanted = w.size;
    let opened = *w.opened.get_or_insert_with(|| Opened::new(wanted, None));
    let chrome = w.chrome.unwrap_or_else(|| chrome_estimate(ctx));
    let id = egui::Id::new(("pdfcraft-chess-window", w.generation));
    if std::mem::take(&mut w.raise) {
        ctx.move_to_top(egui::LayerId::new(egui::Order::Middle, id));
    }
    // At first on the right, over the Inspector (whose Chess panel now only says where its tabs
    // are), leaving the page in view.
    let outer = opened.inner + chrome;
    let screen = ctx.content_rect();
    let right = Pos2::new(screen.right() - outer.x - 8.0, screen.top() + 96.0).max(screen.left_top());
    let mut inner_now = None;
    let window = egui::Window::new(tl!("Chess"))
        .id(id)
        .default_pos(if right.is_finite() { right } else { Pos2::ZERO })
        .default_size(outer)
        .min_size(MIN_SIZE + chrome)
        .resizable(true)
        .collapsible(false)
        .open(&mut open)
        .show(ctx, |ui| {
            // What the contents get: the inner size (not in the invisible pass that sizes a new
            // window).
            if !ui.is_sizing_pass() {
                inner_now = Some(ui.available_size());
            }
            contents(app, ui)
        });
    app.chess_window.shown = true;
    let rect = window.map(|w| w.response.rect).filter(|r| r.is_finite());
    if let (Some(inner), Some(rect)) = (inner_now.filter(|s| s.is_finite() && s.x >= 1.0 && s.y >= 1.0), rect) {
        keep_embedded_geometry(&mut app.chess_window, inner, rect, ctx.pixels_per_point());
    }
    if !open {
        app.dock_chess_board();
    }
}

/// After an embedded window was drawn with `inner` for its contents in an outer `rect`: what its
/// frame adds is measured, and the first time, a window whose contents didn't get the size asked
/// for (the frame was estimated) is made again with the frame measured, once. Then its inner
/// size is kept (saved).
fn keep_embedded_geometry(w: &mut ChessWindow, inner: Vec2, rect: Rect, pixels_per_point: f32) {
    let measured = rect.size() - inner;
    if measured.is_finite() && measured.min_elem() >= 0.0 && measured.max_elem() <= MAX_CHROME {
        w.chrome = Some(measured);
    }
    if let Some(o) = w.opened.as_mut().filter(|o| !o.checked) {
        if (inner - o.inner).abs().max_elem() > 0.5 && !o.retried {
            o.retried = true;
            w.generation = w.generation.wrapping_add(1);
        } else {
            o.checked = true;
        }
    }
    if w.opened.is_some_and(|o| o.checked) {
        w.size = inner.max(MIN_SIZE);
    }
    w.last_shown = Some(Shown { native: false, outer: rect, inner, pixels_per_point, native_pixels_per_point: None, monitor_size: None });
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
    let mut setting_up = false;
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(vec2(side, ui.available_height()), egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.set_width(side);
            asked.board = chess_board_ui::title(ui, chess.board(), source_label);
            ui.add_space(4.0);
            if chess.tools.tool == crate::chess_tools::PgnTool::Setup {
                // The set-up view is taller than the board's controls: it scrolls.
                egui::ScrollArea::vertical().id_salt("chess-window-setup").auto_shrink([false, true]).show(ui, |ui| {
                    setting_up = crate::chess_study_ui::setup_in_place(ui, t, chess, pieces);
                });
            } else {
                setting_up = crate::chess_study_ui::setup_in_place(ui, t, chess, pieces);
            }
            if setting_up {
                return;
            }
            let (study, state) = (&chess.study, chess.games.board_mut());
            chess_board_ui::view_controls(ui, state);
            ui.add_space(4.0);
            crate::chess_study_ui::board_with_bar(ui, t, study, state, pieces, sizing);
            chess_board_ui::promotion_choice(ui, state);
            chess_board_ui::navigation(ui, t, state);
        });
        ui.separator();
        ui.vertical(|ui| {
            // The notation fills the column's height, less the Analysis section above it and
            // what goes under it.
            let room = ui.available_height();
            egui::ScrollArea::vertical().id_salt("chess-window-notes").auto_shrink([false, false]).show(ui, |ui| {
                let top = ui.cursor().top();
                if !setting_up {
                    let (study, state) = (&mut chess.study, chess.games.board_mut());
                    let analysis = crate::chess_study_ui::analysis_section(ui, t, study, state, pieces);
                    asked.board = asked.board.take().or(analysis);
                    ui.add_space(4.0);
                }
                let (fields, listed) = chess_board_ui::moves_column(ui, t, chess, room - (ui.cursor().top() - top));
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
            place: Some(place()),
            docked_front: false,
            tab: ChessWindowTab::Text,
            ..ChessWindow::default()
        };
        let saved = w.to_json();
        let mut back = ChessWindow::default();
        back.restore(&saved);
        assert!(back.detached);
        assert_eq!(back.size, vec2(900.0, 700.0));
        assert_eq!(back.place, Some(place()));
        assert!(!back.docked_front);
        assert_eq!(back.tab, ChessWindowTab::Text);
        // A tab not one of the three, or none (saved before T13b): the board.
        for tab in [json!("Text"), json!("moves"), json!(1), json!(null)] {
            back = ChessWindow::default();
            back.restore(&json!({"detached": true, "tab": tab}));
            assert_eq!(back.tab, ChessWindowTab::Board, "{tab}");
        }
        // Each bad value is ignored on its own; the others are kept.
        let good = place().to_json();
        for (bad, size, kept) in [
            (json!({"detached": true, "size": [-5.0, 700.0], "place": good}), DEFAULT_SIZE, Some(place())),
            (json!({"detached": true, "size": [1e9, 700.0], "place": null}), DEFAULT_SIZE, None),
            (json!({"detached": true, "size": ["wide", 700.0], "place": {"outer": [1e30, 2.0, 800.0, 600.0], "scale": 1.0}}), DEFAULT_SIZE, None),
            (json!({"detached": true, "size": [800.0], "place": {"outer": [1.0, 2.0, 800.0, 600.0], "scale": f64::NAN}}), DEFAULT_SIZE, None),
            (json!({"detached": true, "size": [10.0, 10.0], "place": "here"}), MIN_SIZE, None),
            // Settings from before U5: a position in points of an unknown monitor is not used.
            (json!({"detached": true, "size": [900.0, 700.0], "position": [40.0, 60.0]}), vec2(900.0, 700.0), None),
        ] {
            w = ChessWindow::default();
            w.restore(&bad);
            assert!(w.detached);
            assert!(w.docked_front, "in front unless saved otherwise");
            assert_eq!((w.size, w.place), (size, kept), "{bad}");
        }
        // Not an object at all: nothing out, nothing changed.
        for bad in [json!(null), json!(7), json!("detached"), json!({"detached": "yes"})] {
            w = ChessWindow::default();
            w.restore(&bad);
            assert!(!w.detached, "{bad}");
            assert_eq!(w.size, DEFAULT_SIZE);
        }
    }

    /// A place on the 150% monitor of two.
    fn place() -> Place {
        let monitor = Monitor::new(1920.0, -200.0, 3440.0, 1440.0, 1.5);
        Place { outer: Rect::from_min_size(Pos2::new(2400.0, 0.0), vec2(1464.0, 1200.0)), scale: 1.5, monitor }
    }

    /// PGN toolbar T16, U5: random JSON in place of `docking_detached`, or in any of its fields
    /// (the place's too), never panics, and what is kept is always a usable window: a size within
    /// the limits, a place of finite pixels within ±65,536 and a scale within 0.25-8, a tab of the
    /// three, written back as it was read; and where the window would open is always a finite
    /// place wholly on a monitor. Through the app too, where a window restored out of the dock
    /// takes the Board panel out of the workspace.
    #[test]
    fn random_saved_settings_never_panic_and_only_usable_values_are_kept() {
        use crate::chess_autosave::tests::{Rng, random_value};
        let mut rng = Rng(0x7E16_0D0C);
        let fields = ["detached", "size", "place", "docked_front", "tab"];
        let place_fields = ["outer", "scale", "monitor"];
        let monitors = [Monitor::new(0.0, 0.0, 1920.0, 1080.0, 1.0).unwrap(), place().monitor.unwrap()];
        let main = MainWindow { outer: Rect::from_min_size(Pos2::new(100.0, 50.0), vec2(1200.0, 900.0)), scale: 1.0 };
        let mut app = PdfCraftApp::new();
        for round in 0..2000 {
            let v = if round % 2 == 0 {
                random_value(&mut rng, 3)
            } else {
                // A good value with one or more fields replaced.
                let mut v = ChessWindow { detached: true, place: Some(place()), ..ChessWindow::default() }.to_json();
                for _ in 0..1 + rng.below(3) {
                    let field = rng.below(place_fields.len());
                    let value = random_value(&mut rng, 2);
                    match v.get_mut("place").and_then(Value::as_object_mut) {
                        Some(place) if rng.below(2) == 0 => {
                            place.insert(place_fields[field].to_owned(), value);
                        }
                        _ => v[fields[rng.below(fields.len())]] = value,
                    }
                }
                v
            };
            let mut w = ChessWindow::default();
            w.restore(&v);
            assert!(w.size.x.is_finite() && w.size.y.is_finite(), "{v}");
            assert!((MIN_SIZE.x..=MAX_SAVED).contains(&w.size.x) && (MIN_SIZE.y..=MAX_SAVED).contains(&w.size.y), "{v}: {:?}", w.size);
            assert!(
                w.place.is_none_or(|p| p.outer.is_finite()
                    && p.outer.min.x.abs() <= 65_536.0
                    && p.outer.min.y.abs() <= 65_536.0
                    && (0.25..=8.0).contains(&p.scale)),
                "{v}: {:?}",
                w.place
            );
            assert_eq!(w.detached, v.get("detached").and_then(Value::as_bool).unwrap_or(false), "{v}");
            let mut again = ChessWindow::default();
            again.restore(&w.to_json());
            assert_eq!(
                (again.detached, again.size, again.place, again.docked_front, again.tab),
                (w.detached, w.size, w.place, w.docked_front, w.tab),
                "{v}"
            );
            for monitors in [&monitors[..], &monitors[..1]] {
                let t = crate::window_place::target(w.place.as_ref(), w.size, MIN_SIZE, 1.0, monitors, Some(&main)).expect("a place");
                let on =
                    monitors.iter().any(|m| m.rect.contains(t.pos) && m.rect.contains(t.pos + (t.inner + crate::window_place::CHROME) * m.scale));
                assert!(t.pos.is_finite() && on, "{v}: {t:?}");
            }
            if round % 20 == 0 {
                app.restore_chess_window(&v);
                assert!(!app.chess_window.detached || !app.docking.layout.contains(&Panel::Board), "{v}");
            }
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

    /// Wave 10 U5: the native window and the embedded one keep the same size, what their contents
    /// get, so going from one to the other neither grows nor shrinks the window. The embedded
    /// window's frame is measured; a first one made with a wrong estimate is made again, once.
    #[test]
    fn the_size_kept_is_the_inner_size_native_or_embedded() {
        let ctx = egui::Context::default();
        let mut w = ChessWindow { size: vec2(900.0, 640.0), ..ChessWindow::default() };
        // Native, on a 150% monitor: its inner rectangle is kept, and its place in pixels.
        let inner = Rect::from_min_size(Pos2::new(1608.0, -70.0), vec2(900.0, 640.0));
        let outer = Rect::from_min_max(Pos2::new(1600.0, -100.0), inner.max + vec2(8.0, 8.0));
        let pass = Pass {
            inner: Some(inner),
            outer: Some(outer),
            pixels_per_point: 1.5,
            native_pixels_per_point: Some(1.5),
            monitor_size: Some(vec2(2293.3, 960.0)),
            ..Pass::default()
        };
        w.opened = Some(Opened::new(w.size, None));
        keep_native_geometry(&mut w, &ctx, &pass, 1.0);
        assert_eq!(w.size, vec2(900.0, 640.0));
        assert_eq!(w.place.map(|p| (p.outer.min, p.scale)), Some((Pos2::new(2400.0, -150.0), 1.5)));
        // Embedded: asked for that inner size, with a frame of 12 × 36 points; first estimated
        // as 20 × 30, so its contents first got 892 × 646: made again, and that size not kept.
        w.opened = Some(Opened::new(w.size, None));
        let generation = w.generation;
        keep_embedded_geometry(&mut w, vec2(892.0, 646.0), Rect::from_min_size(Pos2::ZERO, vec2(904.0, 682.0)), 1.0);
        assert_eq!(w.generation, generation.wrapping_add(1), "made again");
        assert_eq!(w.chrome, Some(vec2(12.0, 36.0)), "with its frame measured");
        assert_eq!(w.size, vec2(900.0, 640.0), "the size it got first is not kept");
        // Made again with the frame measured: the size asked for, kept as it was.
        keep_embedded_geometry(&mut w, vec2(900.0, 640.0), Rect::from_min_size(Pos2::ZERO, vec2(912.0, 676.0)), 1.0);
        assert_eq!(w.size, vec2(900.0, 640.0));
        assert_eq!(w.generation, generation.wrapping_add(1), "only once");
        // Resized by the user: the contents' size is kept, never the outer one.
        keep_embedded_geometry(&mut w, vec2(1000.0, 700.0), Rect::from_min_size(Pos2::ZERO, vec2(1012.0, 736.0)), 1.0);
        assert_eq!(w.size, vec2(1000.0, 700.0));
        // A second wrong first frame is lived with (no loop): the size it got is kept.
        w.opened = Some(Opened { retried: true, ..Opened::new(vec2(1000.0, 700.0), None) });
        keep_embedded_geometry(&mut w, vec2(990.0, 690.0), Rect::from_min_size(Pos2::ZERO, vec2(1002.0, 726.0)), 1.0);
        assert_eq!(w.generation, generation.wrapping_add(1));
        assert_eq!(w.size, vec2(990.0, 690.0));
        // Back to native: it opens at that inner size.
        let opened = open_native(&ctx, &w);
        assert_eq!(opened.inner, vec2(990.0, 690.0));
        // Nonsense from the backend is ignored.
        let before = (w.size, w.place);
        let bad = Pass { inner: Some(Rect::NAN), outer: Some(Rect::NAN), pixels_per_point: f32::NAN, ..Pass::default() };
        keep_native_geometry(&mut w, &ctx, &bad, 1.0);
        let zero = Pass { outer: Some(outer), pixels_per_point: 0.0, ..Pass::default() };
        keep_native_geometry(&mut w, &ctx, &zero, 1.0);
        assert_eq!((w.size, w.place), before);
    }
}
