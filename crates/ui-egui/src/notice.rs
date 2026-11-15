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
    /// Open document `doc`'s first diagram to review (the end of a scan, plan A9-I17).
    ReviewNow { doc: DocId },
    /// Undo the capture that left game `game` (its id in the collection) at `revision`, while
    /// the game is as it left it (plan A9-I17).
    UndoCapture { game: u64, revision: u64 },
    /// Undo document `doc`'s last edit, while it is the edit that left it at `generation` (a
    /// diagram redrawn, a lichess link put; plan A9-I17).
    UndoEdit { doc: DocId, generation: u64 },
    /// Take away the book's PGN just kept for document `doc`, which kept none before, while it
    /// is still the one kept (plan A9-I17).
    UndoBookPgn { doc: DocId },
    /// Put back what document `doc`'s book was linked to before Align (plan A9-I17).
    UndoAlign { doc: DocId },
}

/// A notice, while it is shown.
#[derive(Clone, Debug, PartialEq)]
pub struct Notice {
    pub text: String,
    /// The button's label and what it does.
    pub action: Option<(String, NoticeAction)>,
    /// What the button acts on, in its name ("Undo: Diagram 2, page 13: Corrected"): the text of
    /// the notice the button came with, when another said in the same pass joined it.
    about: String,
    /// Information (a notice) or an error (shown until dismissed).
    pub level: Level,
    /// An error's: the errors said in the same pass after the first (`about`), joined in it, none
    /// lost; the text shows the first [`JOINED`] and counts the rest, all in its hover.
    more: Vec<String>,
    /// An error's: how many errors said in the same pass past the [`MAX_KEPT_ERRORS`] kept, only
    /// counted.
    dropped: usize,
    /// An error's: the action that raised it ([`PdfCraftApp::post_error_for`]), whose success
    /// later takes it away ([`PdfCraftApp::settle_error`]); `None` once errors of other actions
    /// joined it.
    pub key: Option<&'static str>,
    /// An error's: the widget that had the keyboard when it was first drawn, which gets it back
    /// when the error is dismissed from the keyboard (WCAG 2.4.3).
    focus_before: Option<egui::Id>,
    /// When it was first drawn: egui's input time and the pass.
    shown: Option<(f64, u64)>,
    /// Its own number: a node of its own for screen readers.
    serial: u64,
}

/// How many errors said in the same pass an error notice shows in its text.
pub const JOINED: usize = 3;

/// How many errors said in one pass an error notice keeps for its hover, the first included; the
/// rest are only counted (plan A9-I17: a loop of failures never builds an unbounded tooltip).
pub const MAX_KEPT_ERRORS: usize = 20;

/// How many characters of one error an error notice keeps (a path or an engine's message can be
/// long); a longer one is cut, with … at its end.
pub const MAX_ERROR_CHARS: usize = 300;

/// `text` cut to [`MAX_ERROR_CHARS`] characters (at a character's boundary), with … when cut.
fn cut_error(text: String) -> String {
    if text.chars().count() <= MAX_ERROR_CHARS {
        return text;
    }
    let mut cut: String = text.chars().take(MAX_ERROR_CHARS).collect();
    cut.push('…');
    cut
}

/// The next number of a notice or an announcement.
fn next_serial() -> u64 {
    static SERIAL: AtomicU64 = AtomicU64::new(1);
    SERIAL.fetch_add(1, Ordering::Relaxed)
}

impl Notice {
    /// A notice saying `text`, with `action`'s button when given.
    pub fn new(text: String, action: Option<(String, NoticeAction)>) -> Notice {
        let about = text.clone();
        Notice {
            about,
            text,
            action,
            level: Level::Info,
            more: Vec::new(),
            dropped: 0,
            key: None,
            focus_before: None,
            shown: None,
            serial: next_serial(),
        }
    }

    /// An error notice saying `text`, shown until it is dismissed.
    pub fn error(text: String) -> Notice {
        Notice { level: Level::Error, ..Notice::new(text, None) }
    }

    /// Every error it says, the first first.
    fn all(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.about.as_str()).chain(self.more.iter().map(String::as_str))
    }

    /// Its text: the first [`JOINED`] errors, and how many more ("… (and 2 more)").
    fn joined_text(&self) -> String {
        let mut text = self.all().take(JOINED).collect::<Vec<_>>().join(" · ");
        let rest = self.more.len().saturating_add(1).saturating_add(self.dropped).saturating_sub(JOINED);
        if rest > 0 {
            text.push(' ');
            text.push_str(&crate::i18n::fmt(tl!("(and {n} more)"), &[("n", &rest.to_string())]));
        }
        text
    }

    /// The area it is drawn in.
    fn id(&self) -> egui::Id {
        egui::Id::new(("notice", self.serial))
    }
}

impl PdfCraftApp {
    /// Show `text`, with a button doing `action` (its label, and what it does) when given. It
    /// takes the place of the notice shown, and of the toast, which says the same thing. A notice
    /// with no button said in the same pass as one with a button, still waiting to be drawn, joins
    /// it instead, first ("Review done: 1 of 1 checked · Diagram 2, page 13: Corrected · Undo"),
    /// so that neither the words nor the button are lost.
    pub(crate) fn post_notice(&mut self, text: String, action: Option<(String, NoticeAction)>) {
        let mut notice = Notice::new(text, action);
        match self.notice.take().filter(|n| n.shown.is_none()) {
            // A plain one said after one with a button, still waiting: it joins it, first.
            Some(waiting) if notice.action.is_none() && waiting.action.is_some() => {
                notice.text = format!("{} · {}", notice.text, waiting.text);
                (notice.action, notice.about) = (waiting.action, waiting.about);
            }
            // One with a button said after a plain one, still waiting: the plain one's words go
            // first, the button (and what it names) is the new one's.
            Some(waiting) if notice.action.is_some() && waiting.action.is_none() => {
                notice.text = format!("{} · {}", waiting.text, notice.text);
            }
            _ => {}
        }
        self.notice = Some(notice);
        self.toast = None;
    }

    /// Show the information `text`, with no button (plan A9-I17: every chess notice but an
    /// error's is one).
    pub(crate) fn inform(&mut self, text: impl Into<String>) {
        self.post_notice(text.into(), None);
    }

    /// [`Self::inform`] with a `{name}`-style template in the UI language (placeholders filled
    /// once), as [`Self::notify_fmt`] does.
    pub(crate) fn inform_fmt(&mut self, template: &str, args: &[(&str, &str)]) {
        self.inform(crate::i18n::fmt(tl!(template), args));
    }

    /// Show the error `text` until it is dismissed, in the place of the error shown (plan
    /// A9-I17): what failed, said at once to screen readers. Errors said in the same pass, before
    /// the first is drawn, are joined in one notice: none is lost.
    pub(crate) fn post_error(&mut self, text: impl Into<String>) {
        self.raise(None, text.into());
    }

    /// [`Self::post_error`] for the action `key` ("export", "games-pgn"): its next success takes
    /// the error away ([`Self::settle_error`]), so a stale error never stands beside it.
    pub(crate) fn post_error_for(&mut self, key: &'static str, text: impl Into<String>) {
        self.raise(Some(key), text.into());
    }

    fn raise(&mut self, key: Option<&'static str>, text: String) {
        let text = cut_error(text);
        if let Some(waiting) = self.alert.as_mut().filter(|a| a.shown.is_none()) {
            if waiting.more.len().saturating_add(1) < MAX_KEPT_ERRORS {
                waiting.more.push(text);
            } else {
                waiting.dropped = waiting.dropped.saturating_add(1);
            }
            waiting.text = waiting.joined_text();
            if waiting.key != key {
                waiting.key = None;
            }
            return;
        }
        self.alert = Some(Notice { key, ..Notice::error(text) });
    }

    /// [`Self::post_error_for`] with a `{name}`-style template, as [`Self::post_error_fmt`].
    pub(crate) fn post_error_fmt_for(&mut self, key: &'static str, template: &str, args: &[(&str, &str)]) {
        self.post_error_for(key, crate::i18n::fmt(tl!(template), args));
    }

    /// The action `key` succeeded: the error it raised, still shown, goes.
    pub(crate) fn settle_error(&mut self, key: &'static str) {
        if self.alert.as_ref().is_some_and(|a| a.key == Some(key)) {
            self.alert = None;
        }
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

    /// The texts of the error, the toast and the notice, those shown: what a test means by "not
    /// said" looks at all three, never only at the first [`Self::said`] gives.
    pub fn all_said(&self) -> impl Iterator<Item = &str> {
        let alert = self.alert.as_ref().map(|n| n.text.as_str());
        let toast = self.toast.as_ref().map(|t| t.0.as_str());
        let notice = self.notice.as_ref().map(|n| n.text.as_str());
        [alert, toast, notice].into_iter().flatten()
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
            NoticeAction::ReviewNow { doc } => {
                if self.active_ids().map(|(_, id)| id) == Some(doc) {
                    self.review_chess_now();
                }
            }
            NoticeAction::UndoCapture { game, revision } => self.undo_capture_at(game, revision),
            NoticeAction::UndoEdit { doc, generation } => {
                let last = self.session.get(doc).is_some_and(|d| d.edit_generation() == generation);
                if last && self.active_ids().map(|(_, id)| id) == Some(doc) {
                    self.history_step(true);
                }
            }
            NoticeAction::UndoBookPgn { doc } => self.undo_book_pgn(doc),
            NoticeAction::UndoAlign { doc } => self.undo_book_align(doc),
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
/// action: a pointer press outside the notice (`rect`, where it was last drawn) and the error
/// notice (`error`: dismissing an error is no answer to the notice), or a key press other than
/// Tab unless the keyboard is on either's button (whose Enter and Space press it).
fn next_action(ctx: &egui::Context, rect: Option<egui::Rect>, error: Option<egui::Rect>) -> bool {
    let inside = |p: egui::Pos2| rect.is_some_and(|r| r.contains(p)) || error.is_some_and(|r| r.contains(p));
    let focus_in = ctx.memory(|m| m.focused()).and_then(|id| ctx.read_response(id)).is_some_and(|r| inside(r.rect.center()));
    ctx.input(|i| {
        i.events.iter().any(|e| match e {
            Event::PointerButton { pos, pressed: true, .. } => !inside(*pos),
            Event::Key { key, pressed: true, .. } => *key != Key::Tab && !focus_in,
            _ => false,
        })
    })
}

/// Esc dismisses the error shown when the chess scope is on or the keyboard is on the error
/// (WCAG 2.1.1), taken out of this frame's `raw` input before the chess keys or egui see it
/// (`blocked`: a modal dialog or the command search has the keys). The keyboard goes back to where
/// it was when the error appeared.
pub(crate) fn take_escape(app: &mut PdfCraftApp, ctx: &egui::Context, raw: &mut egui::RawInput, blocked: bool) {
    let Some(error) = app.alert.as_ref() else { return };
    if blocked || !raw.events.iter().any(|e| matches!(e, Event::Key { key: Key::Escape, pressed: true, modifiers, .. } if modifiers.is_none())) {
        return;
    }
    let rect = ctx.memory(|m| m.area_rect(error.id()));
    let focus_in =
        ctx.memory(|m| m.focused()).and_then(|id| ctx.read_response(id)).is_some_and(|r| rect.is_some_and(|a| a.contains(r.rect.center())));
    if !(focus_in || app.chess.in_scope()) {
        return;
    }
    raw.events.retain(|e| !matches!(e, Event::Key { key: Key::Escape, .. }));
    dismiss(app, ctx, focus_in);
}

/// Take the error away; the keyboard goes back to where it was when it appeared, when it was on
/// the error (`focus_in`).
fn dismiss(app: &mut PdfCraftApp, ctx: &egui::Context, focus_in: bool) {
    let Some(error) = app.alert.take() else { return };
    if focus_in && let Some(id) = error.focus_before {
        ctx.memory_mut(|m| m.request_focus(id));
    }
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
                        let words = ui.add(egui::Label::new(egui::RichText::new(&n.text).color(ink)).wrap());
                        if n.more.len() >= JOINED {
                            let mut all = n.all().collect::<Vec<_>>().join("\n");
                            if n.dropped > 0 {
                                all.push('\n');
                                all.push_str(&crate::i18n::fmt(tl!("(and {n} more)"), &[("n", &n.dropped.to_string())]));
                            }
                            words.on_hover_text(all);
                        }
                        if let Some((label, action)) = &n.action {
                            let button = ui.add(egui::Button::new(label.as_str()).min_size(vec2(0.0, crate::widgets::MIN_TARGET)));
                            // Its name says what it takes back, its label first (WCAG 2.5.3).
                            let name = format!("{label}: {}", n.about);
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
    let error = app.alert.as_ref().and_then(|e| ctx.memory(|m| m.area_rect(e.id())));
    if let Some(e) = app.alert.as_mut()
        && e.shown.is_none()
    {
        e.shown = Some((now, pass));
        e.focus_before = ctx.memory(|m| m.focused());
    }
    if let Some(n) = app.notice.as_mut() {
        let (since, posted) = *n.shown.get_or_insert((now, pass));
        let gone = match n.action {
            None => now - since > PLAIN_SECONDS,
            Some(_) => pass != posted && next_action(ctx, ctx.memory(|m| m.area_rect(n.id())), error),
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
            // From the keyboard (Tab to ×, then Enter), the keyboard goes back.
            let focus_in = ctx.memory(|m| m.focused()).and_then(|id| ctx.read_response(id)).is_some_and(|r| rect.contains(r.rect.center()));
            dismiss(app, ctx, focus_in);
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

#[cfg(test)]
mod tests {
    /// Plan A9-I17 (findings R4-10, R5-02): every chess notice is a notice or an error notice,
    /// none the toast (gone in 3.5 s, one node for all): no `notify` call is left in the chess
    /// code.
    #[test]
    fn every_chess_notice_is_a_notice_or_an_error() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut found = Vec::new();
        for entry in std::fs::read_dir(&dir).expect("the sources").flatten() {
            let path = entry.path();
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if !(name.starts_with("chess_") && name.ends_with(".rs")) {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("a source");
            let code = text.split("\n#[cfg(test)]").next().unwrap_or_default();
            for (i, line) in code.lines().enumerate() {
                if [".notify(", ".notify_fmt(", ".notify_tr(", ".notify_error("].iter().any(|c| line.contains(c)) {
                    found.push(format!("{name}:{}: {}", i + 1, line.trim()));
                }
            }
        }
        assert!(found.is_empty(), "chess notices still on the toast:\n{}", found.join("\n"));
    }

    /// Plan A9-I17 (review of #404): a notice with no button said first, then one with a button
    /// in the same pass, keeps both: the first's words go first, the button is the second's.
    /// Before, the second took the first's place and its words were never drawn (a capture's "The
    /// rest is too long to keep as a comment…" before its summary with Undo).
    #[test]
    fn a_notice_with_a_button_after_a_plain_one_keeps_its_words() {
        let mut app = crate::PdfCraftApp::default();
        app.inform("The rest is too long to keep as a comment; it is still on the page.");
        app.post_notice("Diagram 2, page 1: Captured".to_owned(), Some(("Undo".to_owned(), super::NoticeAction::ReopenTools)));
        let n = app.notice.as_ref().expect("a notice");
        assert_eq!(n.text, "The rest is too long to keep as a comment; it is still on the page. · Diagram 2, page 1: Captured");
        assert_eq!(n.action.as_ref().map(|(label, _)| label.as_str()), Some("Undo"));
        assert_eq!(n.about, "Diagram 2, page 1: Captured", "the button says what it takes back");
        // The other way round, as before: the plain one joins the waiting one with a button.
        let mut app = crate::PdfCraftApp::default();
        app.post_notice("Diagram 2, page 1: Captured".to_owned(), Some(("Undo".to_owned(), super::NoticeAction::ReopenTools)));
        app.inform("Review done");
        assert_eq!(app.notice.as_ref().map(|n| n.text.as_str()), Some("Review done · Diagram 2, page 1: Captured"));
    }

    /// Plan A9-I17 (review of #404): errors said in one pass are joined, and what is kept of them
    /// is bounded, in count and in each one's length, the count of the rest exact.
    #[test]
    fn errors_joined_in_one_pass_are_bounded() {
        let mut app = crate::PdfCraftApp::default();
        let long = "x".repeat(10 * super::MAX_ERROR_CHARS);
        for _ in 0..1000 {
            app.post_error(long.clone());
        }
        let n = app.alert.as_ref().expect("an error");
        assert!(n.more.len() < super::MAX_KEPT_ERRORS, "{} kept", n.more.len());
        assert!(n.all().all(|e| e.chars().count() <= super::MAX_ERROR_CHARS + 1), "each one cut");
        assert!(n.text.ends_with(&crate::i18n::fmt(tl!("(and {n} more)"), &[("n", "997")])), "{}", n.text);
    }
}
