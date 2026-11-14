//! A notice of what the user just did (plan A9 §2.8.1, made by step G10; step I17 makes the app's
//! other notices these): its text and, when what was done can be taken back, a button that does
//! it ("Diagram 2, page 13: Corrected · Undo").
//!
//! It stands at the foot of the page area, so it never covers a panel's footer, above the toast
//! and the progress notice when they show. Screen readers say it when it appears: it is a polite
//! status, a node of its own for each notice, so two equal notices one after the other are both
//! said. Without an action it goes after [`PLAIN_SECONDS`]; with one it stays until the user's
//! next action — a click outside it, a key other than Tab, or a key while its button does not have
//! the keyboard — so the button is never gone before it can be reached, by the pointer or by Tab
//! (WCAG 2.2.1). A new notice takes the place of the one shown.

use std::sync::atomic::{AtomicU64, Ordering};

use egui::{Align2, Event, Key, vec2};
use pdfcraft_engine::DocId;

use crate::PdfCraftApp;
use crate::theme::Tokens;

/// How long a notice without an action stays, in seconds.
pub const PLAIN_SECONDS: f64 = 5.0;

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
    /// When it was first drawn: egui's input time and the pass.
    shown: Option<(f64, u64)>,
    /// Its own number: a node of its own for screen readers.
    serial: u64,
}

impl Notice {
    /// A notice saying `text`, with `action`'s button when given.
    pub fn new(text: String, action: Option<(String, NoticeAction)>) -> Notice {
        static SERIAL: AtomicU64 = AtomicU64::new(1);
        Notice { text, action, shown: None, serial: SERIAL.fetch_add(1, Ordering::Relaxed) }
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

    /// Run what a notice's button does.
    fn notice_action(&mut self, action: NoticeAction) {
        match action {
            NoticeAction::UndoCorrection { doc, seq } => self.undo_chess_fix_at(doc, seq),
            NoticeAction::ReopenTools => self.left_open = true,
        }
    }
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

/// Draw [`PdfCraftApp::notice`] at the foot of the page area, and run its button when clicked.
pub(crate) fn show(app: &mut PdfCraftApp, ctx: &egui::Context) {
    let (now, pass) = (ctx.input(|i| i.time), ctx.cumulative_pass_nr());
    let Some(n) = app.notice.as_mut() else { return };
    let (since, posted) = *n.shown.get_or_insert((now, pass));
    let id = n.id();
    let gone = match n.action {
        None => now - since > PLAIN_SECONDS,
        Some(_) => pass != posted && next_action(ctx, ctx.memory(|m| m.area_rect(id))),
    };
    if gone {
        app.notice = None;
        return;
    }
    let (text, action) = (n.text.clone(), n.action.clone());
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
    let mut clicked = false;
    egui::Area::new(id).order(egui::Order::Foreground).pivot(Align2::CENTER_BOTTOM).fixed_pos(egui::pos2(area.center().x, bottom)).show(ctx, |ui| {
        egui::Frame::NONE
            .fill(t.card)
            .stroke(egui::Stroke::new(1.0, t.border_strong))
            .corner_radius(egui::CornerRadius::same(t.radius))
            .inner_margin(egui::Margin::symmetric(12, 6))
            .show(ui, |ui| {
                ui.set_max_width((area.width() - 32.0).clamp(1.0, 560.0));
                // A polite status, said when it appears: its text is the label inside it.
                ui.ctx().accesskit_node_builder(ui.unique_id(), |node| {
                    node.set_role(egui::accesskit::Role::Status);
                    node.set_live(egui::accesskit::Live::Polite);
                });
                ui.horizontal_wrapped(|ui| {
                    ui.add(egui::Label::new(egui::RichText::new(&text).color(t.text)).wrap());
                    if let Some((label, _)) = &action {
                        let button = ui.add(egui::Button::new(label.as_str()).min_size(vec2(0.0, crate::widgets::MIN_TARGET)));
                        // Its name says what it takes back, its label first (WCAG 2.5.3).
                        let name = format!("{label}: {text}");
                        button.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
                        clicked = button.clicked();
                    }
                });
            });
    });
    if clicked && let Some((_, action)) = action {
        app.notice = None;
        app.notice_action(action);
    }
    if app.notice.as_ref().is_some_and(|n| n.action.is_none()) {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
}
