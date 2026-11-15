//! What the app says about what just happened, in one place (plan A9 §2.8.1 "Avisos, anúncios e
//! desfazer"; step G10 made the notice, step I17 the rest):
//!
//! - a **notice** ([`PdfCraftApp::post_notice`]): what the user just did, and when it can be taken
//!   back a button that does it ("Diagram 2, page 13: Corrected · Undo");
//! - an **error notice** ([`PdfCraftApp::post_error`]): what failed, a danger [`callout`] that
//!   stays until it is dismissed (WCAG 2.2.1, 3.3.1), never gone in a few seconds;
//! - an **announcement** ([`announce`]): words for screen readers only, with nothing drawn (the
//!   move shown, the diagram opened);
//! - a **callout** ([`callout`]): an error or a warning in the flow of a panel, with its icon, its
//!   colour of text and its soft fill, never small grey text (finding R4-09).
//!
//! The notices stand at the foot of the page area, so they never cover a panel's footer (finding
//! A6-26), above the toast and the progress notice when they show; the error under the notice.
//! Screen readers say each when it appears (WCAG 4.1.3): a notice is a polite status, an error an
//! assertive alert, each a node of its own, so two equal ones one after the other are both said
//! (AccessKit tells of a live region only when its node is new or its name changes). A notice
//! without an action goes after [`PLAIN_SECONDS`]; with one it stays until the user's next action —
//! a click outside it, a key other than Tab, or a key while its button does not have the keyboard —
//! so the button is never gone before it can be reached, by the pointer or by Tab. A new notice
//! takes the place of the one shown, and a new error that of the error shown.

use std::sync::atomic::{AtomicU64, Ordering};

use egui::{Align2, Event, Key, vec2};
use pdfcraft_engine::DocId;

use crate::PdfCraftApp;
use crate::theme::Tokens;

/// How long a notice without an action stays, in seconds.
pub const PLAIN_SECONDS: f64 = 5.0;

/// How a notice or an announcement is said to screen readers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// Information: a polite status, said when the reader is done with what it says.
    Info,
    /// An error: an assertive alert, said at once.
    Error,
}

impl Level {
    /// Its role and how it is said.
    fn speech(self) -> (egui::accesskit::Role, egui::accesskit::Live) {
        use egui::accesskit::{Live, Role};
        match self {
            Level::Info => (Role::Status, Live::Polite),
            Level::Error => (Role::Alert, Live::Assertive),
        }
    }
}

/// What a notice's button does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeAction {
    /// Undo the step of document `doc`'s corrections put on the undo side at `seq`
    /// ([`crate::chess_history::next_seq`]), while it is still the one Undo would take there
    /// (plan A9-G10).
    UndoCorrection { doc: DocId, seq: u64 },
    /// Open the Tools panel again, collapsed for a chess surface (plan A9-I0a).
    ReopenTools,
}

/// A notice, while it is shown.
#[derive(Clone, Debug, PartialEq)]
pub struct Notice {
    pub text: String,
    /// The button's label and what it does.
    pub action: Option<(String, NoticeAction)>,
    /// Information (a notice) or an error (shown until dismissed).
    pub level: Level,
    /// When it was first drawn: egui's input time and the pass.
    shown: Option<(f64, u64)>,
    /// Its own number: a node of its own for screen readers.
    serial: u64,
}

/// The next number of a notice or an announcement.
fn next_serial() -> u64 {
    static SERIAL: AtomicU64 = AtomicU64::new(1);
    SERIAL.fetch_add(1, Ordering::Relaxed)
}

impl Notice {
    /// A notice saying `text`, with `action`'s button when given.
    pub fn new(text: String, action: Option<(String, NoticeAction)>) -> Notice {
        Notice { text, action, level: Level::Info, shown: None, serial: next_serial() }
    }

    /// An error notice saying `text`, shown until it is dismissed.
    pub fn error(text: String) -> Notice {
        Notice { text, action: None, level: Level::Error, shown: None, serial: next_serial() }
    }

    /// The area it is drawn in.
    fn id(&self) -> egui::Id {
        egui::Id::new(("notice", self.serial))
    }
}

impl PdfCraftApp {
    /// Show `text`, with a button doing `action` (its label, and what it does) when given. It
    /// takes the place of the notice shown, and of the toast, which says the same thing.
    pub(crate) fn post_notice(&mut self, text: String, action: Option<(String, NoticeAction)>) {
        self.notice = Some(Notice::new(text, action));
        self.toast = None;
    }

    /// Show the error `text` until it is dismissed, in the place of the error shown (plan
    /// A9-I17): what failed, said at once to screen readers.
    pub(crate) fn post_error(&mut self, text: impl Into<String>) {
        self.alert = Some(Notice::error(text.into()));
    }

    /// [`Self::post_error`] with a `{name}`-style template in the UI language (placeholders filled
    /// once), as [`Self::notify_fmt`] does.
    pub(crate) fn post_error_fmt(&mut self, template: &str, args: &[(&str, &str)]) {
        self.post_error(crate::i18n::fmt(tl!(template), args));
    }

    /// Say `text` to screen readers, with nothing drawn ([`announce`]).
    pub(crate) fn announce(&self, level: Level, text: impl Into<String>) {
        if let Some(ctx) = &self.ctx {
            announce(ctx, level, text);
        }
    }

    /// What the app says now: the error shown, else the toast, else the notice (the control
    /// channel's `notice`, and what the tests read). The toast comes before the notice: a notice
    /// with a button stays until the next action, and a toast said after it is the newer.
    pub fn said(&self) -> Option<&str> {
        self.alert.as_ref().map(|n| n.text.as_str()).or(self.toast.as_ref().map(|t| t.0.as_str())).or(self.notice.as_ref().map(|n| n.text.as_str()))
    }

    /// Take away what the app says now (the error, the toast and the notice), and give its text as
    /// [`Self::said`] does (a test starts afresh with it).
    pub fn take_said(&mut self) -> Option<String> {
        let said = self.said().map(str::to_owned);
        (self.alert, self.notice, self.toast) = (None, None, None);
        said
    }

    /// Run what a notice's button does.
    fn notice_action(&mut self, action: NoticeAction) {
        match action {
            NoticeAction::UndoCorrection { doc, seq } => self.undo_chess_fix_at(doc, seq),
            NoticeAction::ReopenTools => self.left_open = true,
        }
    }
}

/// An announcement waiting to be said, or said: its words, and its own number.
#[derive(Clone, Debug)]
struct Announcement {
    level: Level,
    text: String,
    serial: u64,
}

/// Where the announcement is kept, in egui's memory.
fn announcement_id() -> egui::Id {
    egui::Id::new("pdfcraft-announcement")
}

/// Say `text` to screen readers, at `level`, with nothing drawn: a live node of its own (a new
/// node each time, so the same words twice are said twice), kept until the next announcement.
/// Drawn by [`show`], so it is said at the end of the pass that made it, or of the next one.
pub(crate) fn announce(ctx: &egui::Context, level: Level, text: impl Into<String>) {
    let said = Announcement { level, text: text.into(), serial: next_serial() };
    ctx.data_mut(|d| d.insert_temp(announcement_id(), said));
    ctx.request_repaint();
}

/// The words last announced, while they are kept (the tests read them).
pub fn announced(ctx: &egui::Context) -> Option<String> {
    ctx.data(|d| d.get_temp::<Announcement>(announcement_id())).map(|a| a.text)
}

/// The announcement's live node, under the window's root.
fn say(ctx: &egui::Context) {
    let Some(said) = ctx.data(|d| d.get_temp::<Announcement>(announcement_id())) else { return };
    let (role, live) = said.level.speech();
    // A point at the window's corner: a node with no place is one assistive technology and the
    // tests' walks over the tree can't place.
    let corner = egui::Rect::from_min_size(ctx.content_rect().min, vec2(1.0, 1.0));
    ctx.accesskit_node_builder(egui::Id::new(("announcement", said.serial)), |node| {
        node.set_role(role);
        node.set_live(live);
        node.set_label(said.text);
        node.set_bounds(bounds(corner));
    });
}

/// `rect` as AccessKit's bounds (in points, as egui gives its widgets').
fn bounds(rect: egui::Rect) -> egui::accesskit::Rect {
    egui::accesskit::Rect { x0: rect.min.x.into(), y0: rect.min.y.into(), x1: rect.max.x.into(), y1: rect.max.y.into() }
}

/// Give the group `id` (a frame's inner `Ui`, made a live region) the frame's place.
fn place(ctx: &egui::Context, id: egui::Id, rect: egui::Rect) {
    ctx.accesskit_node_builder(id, |node| node.set_bounds(bounds(rect)));
}

/// How a [`callout`] looks: its icon and its colours of text and fill.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tone {
    /// Something failed or cannot be used: an assertive alert.
    Danger,
    /// Something limits what the app does, and the user can change it.
    Warning,
}

impl Tone {
    /// Its icon, its text's colour, its fill and its edge.
    fn look(self, t: &Tokens) -> (&'static str, egui::Color32, egui::Color32, egui::Color32) {
        match self {
            Tone::Danger => ("circle-x", t.danger_text, t.danger_soft, t.danger),
            Tone::Warning => ("triangle-alert", t.warning_text, t.warning_soft, t.warning),
        }
    }
}

/// `text` in the flow of a panel as a callout of `tone`: its icon, the text in the tone's colour
/// on its soft fill (4.5:1 or more in both themes, tested in `theme`), wrapped to the width given.
/// A danger callout is an alert to screen readers.
pub(crate) fn callout(ui: &mut egui::Ui, tone: Tone, text: &str) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (icon, ink, fill, edge) = tone.look(&t);
    let shown = egui::Frame::NONE
        .fill(fill)
        .stroke(egui::Stroke::new(1.0, edge))
        .corner_radius(egui::CornerRadius::same(t.radius))
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            let id = ui.unique_id();
            if tone == Tone::Danger {
                ui.ctx().accesskit_node_builder(id, |node| {
                    node.set_role(egui::accesskit::Role::Alert);
                    node.set_live(egui::accesskit::Live::Assertive);
                });
            }
            ui.horizontal_top(|ui| {
                ui.add(crate::icons::image(icon, 14.0, ink));
                ui.add(egui::Label::new(egui::RichText::new(text).color(ink)).wrap());
            });
            id
        });
    if tone == Tone::Danger {
        place(ui.ctx(), shown.inner, shown.response.rect);
    }
    shown.response
}

/// Whether this pass's input holds the user's next action, which takes away a notice with an
/// action: a pointer press outside the notice (`rect`, where it was last drawn), or a key press
/// other than Tab unless the keyboard is on the notice's button (whose Enter and Space press it).
fn next_action(ctx: &egui::Context, rect: Option<egui::Rect>) -> bool {
    let inside = |p: egui::Pos2| rect.is_some_and(|r| r.contains(p));
    let focus_in = ctx.memory(|m| m.focused()).and_then(|id| ctx.read_response(id)).is_some_and(|r| inside(r.rect.center()));
    ctx.input(|i| {
        i.events.iter().any(|e| match e {
            Event::PointerButton { pos, pressed: true, .. } => !inside(*pos),
            Event::Key { key, pressed: true, .. } => *key != Key::Tab && !focus_in,
            _ => false,
        })
    })
}

/// What a click on a notice's button does.
enum Pressed {
    Action(NoticeAction),
    Dismiss,
}

/// Draw `n` with its bottom edge at `bottom`, centred in `area`. What its button did, and where it
/// was drawn.
fn draw(ctx: &egui::Context, t: &Tokens, n: &Notice, area: egui::Rect, bottom: f32) -> (Option<Pressed>, egui::Rect) {
    let (fill, edge, ink, icon) = match n.level {
        Level::Info => (t.card, t.border_strong, t.text, None),
        Level::Error => (t.danger_soft, t.danger, t.danger_text, Some("circle-x")),
    };
    let mut pressed = None;
    let shown = egui::Area::new(n.id())
        .order(egui::Order::Foreground)
        .pivot(Align2::CENTER_BOTTOM)
        .fixed_pos(egui::pos2(area.center().x, bottom))
        .show(ctx, |ui| {
            let framed = egui::Frame::NONE
                .fill(fill)
                .stroke(egui::Stroke::new(1.0, edge))
                .corner_radius(egui::CornerRadius::same(t.radius))
                .inner_margin(egui::Margin::symmetric(12, 6))
                .show(ui, |ui| {
                    ui.set_max_width((area.width() - 32.0).clamp(1.0, 560.0));
                    // Said when it appears: its text is the label inside it.
                    let (role, live) = n.level.speech();
                    let id = ui.unique_id();
                    ui.ctx().accesskit_node_builder(id, |node| {
                        node.set_role(role);
                        node.set_live(live);
                    });
                    ui.horizontal_wrapped(|ui| {
                        if let Some(icon) = icon {
                            ui.add(crate::icons::image(icon, 14.0, ink));
                        }
                        ui.add(egui::Label::new(egui::RichText::new(&n.text).color(ink)).wrap());
                        if let Some((label, action)) = &n.action {
                            let button = ui.add(egui::Button::new(label.as_str()).min_size(vec2(0.0, crate::widgets::MIN_TARGET)));
                            // Its name says what it takes back, its label first (WCAG 2.5.3).
                            let name = format!("{label}: {}", n.text);
                            button.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
                            if button.clicked() {
                                pressed = Some(Pressed::Action(*action));
                            }
                        }
                        if n.level == Level::Error {
                            let label = tl!("Dismiss");
                            let button = crate::icons::button(ui, "x", crate::widgets::MIN_TARGET, false, label);
                            let name = format!("{label}: {}", n.text);
                            button.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
                            if button.clicked() {
                                pressed = Some(Pressed::Dismiss);
                            }
                        }
                    });
                    id
                });
            place(ui.ctx(), framed.inner, framed.response.rect);
        });
    (pressed, shown.response.rect)
}

/// Draw [`PdfCraftApp::alert`] and [`PdfCraftApp::notice`] at the foot of the page area, the
/// notice above the error, run their buttons when clicked, and say the announcement.
pub(crate) fn show(app: &mut PdfCraftApp, ctx: &egui::Context) {
    say(ctx);
    let (now, pass) = (ctx.input(|i| i.time), ctx.cumulative_pass_nr());
    if let Some(n) = app.notice.as_mut() {
        let (since, posted) = *n.shown.get_or_insert((now, pass));
        let gone = match n.action {
            None => now - since > PLAIN_SECONDS,
            Some(_) => pass != posted && next_action(ctx, ctx.memory(|m| m.area_rect(n.id()))),
        };
        if gone {
            app.notice = None;
        }
    }
    if app.notice.is_none() && app.alert.is_none() {
        return;
    }
    // The page area of the document shown, else the window's.
    let area = app.active.and_then(|i| app.views.get(i)).map(|v| v.viewport_rect()).filter(|r| r.is_positive()).unwrap_or_else(|| ctx.content_rect());
    // Above the toast and the progress notice, which stand at the window's foot.
    let mut bottom = area.bottom() - 16.0;
    for (shown, other) in [(app.toast.is_some(), "toast"), (app.progress_notice.is_some(), "progress-notice")] {
        if let Some(r) = ctx.memory(|m| m.area_rect(egui::Id::new(other))).filter(|_| shown)
            && r.top() < bottom
            && r.x_range().intersects(area.x_range())
        {
            bottom = bottom.min(r.top() - 8.0);
        }
    }
    let t = Tokens::get(ctx);
    if let Some(error) = app.alert.clone() {
        let (pressed, rect) = draw(ctx, &t, &error, area, bottom);
        bottom = bottom.min(rect.top() - 8.0);
        if matches!(pressed, Some(Pressed::Dismiss)) {
            app.alert = None;
        }
    }
    if let Some(n) = app.notice.clone() {
        let (pressed, _) = draw(ctx, &t, &n, area, bottom);
        if let Some(Pressed::Action(action)) = pressed {
            app.notice = None;
            app.notice_action(action);
        }
    }
    if app.notice.as_ref().is_some_and(|n| n.action.is_none()) {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
}
