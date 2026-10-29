//! Chess ▸ Diagrams: the list of the diagrams found (plan A8, P2a1).
//!
//! Filter chips over the list say how many diagrams each state has ("To review 40") and show only
//! one state, or only the page on screen; Follow the page keeps the page on screen at the top of
//! the list. Each page has a header ("Page 17 · 25 of 703" when its printed number is not its
//! place in the file) that goes to it, and each diagram a 32-point row: its state's icon, "Diagram
//! 1", the state's name, and ⋯ (on hover, selection or focus, and as the row's context menu) with
//! Open on the board, Copy FEN and Open in lichess. The FEN is in the row's tooltip and its menu
//! only.
//!
//! The list is virtualised: it takes its whole height in the panel's scroll area, worked out by
//! prefix sums over rows of three heights (page headers, notes and diagram rows), and builds only
//! the rows inside the visible part. With the keyboard on a row, ↑ ↓ Home End go to another
//! diagram (scrolling it into view first; the keys leave the input in `raw_input_hook`, before egui
//! moves the focus by itself), and Enter or Space opens it.

use egui::{Align, Event, Id, Key, Rect, Sense, Stroke, UiBuilder, pos2, vec2};
use pdfcraft_engine::chess::ChessPage;
use pdfcraft_engine::chess::pdfcraft_chessvision::board::BoardReading;
use pdfcraft_render::DocInfo;

use crate::chess_ui::{ChessResults, PanelAction};
use crate::chess_widgets::DiagramState;
use crate::theme::{self, Tokens};

/// The heights of a page's header (24 points and 4 above it), a note under it and a diagram's row.
const PAGE_H: f32 = 28.0;
const NOTE_H: f32 = 20.0;
const ROW_H: f32 = 32.0;

/// A filter chip's height, in points.
const CHIP_H: f32 = 24.0;

/// How far beyond the visible part rows are built, in points.
const OVERSCAN: f32 = 64.0;

/// Most keys kept from one frame's input.
const MAX_KEYS: usize = 64;

/// The states the chips filter by, in the order shown.
const STATES: [DiagramState; 6] =
    [DiagramState::Read, DiagramState::Review, DiagramState::Verified, DiagramState::Corrected, DiagramState::Unread, DiagramState::Error];

/// The list's own state.
#[derive(Default)]
pub struct ListState {
    /// The one state shown (`None`: all of them).
    pub(crate) filter: Option<DiagramState>,
    /// Only the page on screen.
    pub(crate) this_page: bool,
    /// Keep the page on screen at the top of the list.
    pub(crate) follow: bool,
    /// The page Follow the page last brought up.
    followed: Option<usize>,
    /// The row (page, index) that takes the keyboard when it is next built.
    focus: Option<(usize, usize)>,
    /// The row whose ⋯ menu is open.
    menu: Option<(usize, usize)>,
    /// The row that had the keyboard when the list was last built.
    focused: Option<(usize, usize)>,
    /// The keys taken for the focused row this frame.
    keys: Vec<Key>,
}

/// What the list is made of: a page's header, a note under it (it could not be scanned, it had
/// more diagrams than listed) and a diagram's row; pages by their place in `ChessResults::pages`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Item {
    Page(usize),
    Error(usize),
    Truncated(usize),
    Row(usize, usize),
}

impl Item {
    fn height(self) -> f32 {
        match self {
            Item::Page(_) => PAGE_H,
            Item::Error(_) | Item::Truncated(_) => NOTE_H,
            Item::Row(..) => ROW_H,
        }
    }
}

/// A diagram row's id, the same from frame to frame (the keyboard goes to it by it).
pub(crate) fn row_id(page: usize, index: usize) -> Id {
    Id::new(("chess-diagram-row", page, index))
}

/// Take ↑ ↓ Home End out of `events` while a row of the list has the keyboard (as the last frame
/// left it) and nothing else must have them (`free`).
pub(crate) fn take_keys(list: &mut ListState, ctx: &egui::Context, events: &mut Vec<Event>, free: bool) {
    list.keys.clear();
    let Some((page, index)) = list.focused.filter(|_| free) else { return };
    if !ctx.memory(|m| m.has_focus(row_id(page, index))) {
        return;
    }
    let keys = &mut list.keys;
    events.retain(|event| match event {
        Event::Key { key: key @ (Key::ArrowUp | Key::ArrowDown | Key::Home | Key::End), pressed: true, modifiers, .. } if modifiers.is_none() => {
            if keys.len() < MAX_KEYS {
                keys.push(*key);
            }
            false
        }
        _ => true,
    });
}

/// Diagram `index` of `page`'s state.
fn state_of(results: &ChessResults, page: &ChessPage, index: usize) -> DiagramState {
    DiagramState::of(ChessResults::reading(page, index), results.kind(page.page, index))
}

/// What the list shows, in order: the pages with a diagram in the filter (or, unfiltered, one that
/// could not be scanned), each with its header, its notes and its rows.
fn items(list: &ListState, results: &ChessResults, page_now: usize) -> Vec<Item> {
    let mut out = Vec::new();
    for (pi, page) in results.pages.iter().enumerate() {
        if list.this_page && page.page != page_now {
            continue;
        }
        let error = page.error.is_some() && list.filter.is_none();
        let mut header = false;
        for i in 0..page.diagrams.len() {
            if list.filter.is_some_and(|f| state_of(results, page, i) != f) {
                continue;
            }
            if !header {
                header = true;
                out.push(Item::Page(pi));
                if error {
                    out.push(Item::Error(pi));
                }
                if page.truncated {
                    out.push(Item::Truncated(pi));
                }
            }
            out.push(Item::Row(pi, i));
        }
        if !header && error {
            out.push(Item::Page(pi));
            out.push(Item::Error(pi));
        }
    }
    out
}

/// How many diagrams each of [`STATES`] has, and all of them, on the page on screen only when
/// that chip is on.
fn counts(list: &ListState, results: &ChessResults, page_now: usize) -> ([usize; STATES.len()], usize) {
    let mut counts = [0usize; STATES.len()];
    let mut all = 0usize;
    for page in results.pages.iter().filter(|p| !list.this_page || p.page == page_now) {
        for i in 0..page.diagrams.len() {
            let state = state_of(results, page, i);
            if let Some(slot) = STATES.iter().position(|s| *s == state).and_then(|k| counts.get_mut(k)) {
                *slot = (*slot).saturating_add(1);
            }
            all = all.saturating_add(1);
        }
    }
    (counts, all)
}

/// The list under the panel's buttons: the chips, Follow the page and the rows. `reveal`: scroll
/// the selected diagram into view (it was picked on the page); `focus_row`: and give it the
/// keyboard (Esc in the Diagram tab).
#[allow(clippy::too_many_arguments)]
pub(crate) fn list(
    ui: &mut egui::Ui,
    t: &Tokens,
    list: &mut ListState,
    results: &ChessResults,
    info: &DocInfo,
    page_now: usize,
    drafts: &[(usize, usize)],
    reveal: bool,
    focus_row: bool,
) -> Option<PanelAction> {
    let mut action = None;
    chips(ui, t, list, results, page_now);
    ui.add(crate::widgets::checkbox(&mut list.follow, tl!("Follow the page"))).on_hover_text(tl!("Keep the page on screen at the top of the list."));
    ui.add_space(4.0);

    let items = items(list, results, page_now);
    if items.is_empty() {
        ui.label(egui::RichText::new(tl!("No diagram in this filter.")).color(t.text_muted));
        list.focused = None;
        return None;
    }
    let mut tops = Vec::with_capacity(items.len());
    let mut total = 0.0f32;
    for item in &items {
        tops.push(total);
        total += item.height();
    }
    let row_at =
        |key: (usize, usize)| items.iter().position(|it| matches!(it, Item::Row(pi, i) if results.pages.get(*pi).map(|p| (p.page, *i)) == Some(key)));

    // Where to scroll, and the row the keyboard goes to.
    let mut scroll = None;
    if let Some(at) = list.focused.and_then(row_at) {
        for key in std::mem::take(&mut list.keys) {
            let rows = || items.iter().enumerate().filter(|(_, it)| matches!(it, Item::Row(..)));
            let to = match key {
                Key::ArrowUp => rows().rfind(|(k, _)| *k < at),
                Key::ArrowDown => rows().find(|(k, _)| *k > at),
                Key::Home => rows().next(),
                _ => rows().next_back(),
            };
            if let Some((k, Item::Row(pi, i))) = to {
                list.focus = results.pages.get(*pi).map(|p| (p.page, *i));
                scroll = Some((k, None));
            }
        }
    }
    if let Some(selected) = results.selected.filter(|_| reveal || focus_row)
        && let Some(k) = row_at(selected)
    {
        scroll = Some((k, Some(Align::Center)));
        if focus_row {
            list.focus = Some(selected);
        }
    }
    if !list.follow {
        list.followed = None;
    } else if list.followed != Some(page_now) {
        list.followed = Some(page_now);
        if let Some(k) = items.iter().position(|it| matches!(it, Item::Page(pi) if results.pages.get(*pi).is_some_and(|p| p.page == page_now))) {
            scroll = Some((k, Some(Align::Min)));
        }
    }
    // A row the keyboard was sent to that the filter hides takes it no more.
    if list.focus.is_some_and(|key| row_at(key).is_none()) {
        list.focus = None;
    }

    let width = ui.available_width();
    let (whole, _) = ui.allocate_exact_size(vec2(width, total), Sense::hover());
    let rect_of = |k: usize| {
        let top = tops.get(k).copied().unwrap_or(0.0);
        let height = items.get(k).map_or(ROW_H, |it| it.height());
        Rect::from_min_size(pos2(whole.min.x, whole.min.y + top), vec2(width, height))
    };
    if let Some((k, align)) = scroll {
        ui.scroll_to_rect(rect_of(k), align);
    }
    // Only what is in sight (and a little more) is built.
    let clip = ui.clip_rect();
    let first = tops.partition_point(|top| whole.min.y + top + ROW_H < clip.min.y - OVERSCAN);
    let total_pages = info.pages.len();
    let mut focused = None;
    let mut menu = None;
    for (k, item) in items.iter().enumerate().skip(first) {
        let r = rect_of(k);
        if r.min.y > clip.max.y + OVERSCAN {
            break;
        }
        match *item {
            Item::Page(pi) => {
                if let Some(page) = results.pages.get(pi)
                    && let Some(a) = page_header(ui, t, r, page.page, info, total_pages)
                {
                    action = Some(a);
                }
            }
            Item::Error(pi) => {
                let e = results.pages.get(pi).and_then(|p| p.error.as_deref()).unwrap_or_default();
                note(ui, t, r, &crate::i18n::fmt(tl!("This page couldn't be scanned: {e}"), &[("e", e)]));
            }
            Item::Truncated(pi) => {
                let n = results.pages.get(pi).map_or(0, |p| p.diagrams.len());
                note(ui, t, r, &crate::i18n::fmt(tl!("Only the first {n} diagrams on this page are listed."), &[("n", &n.to_string())]));
            }
            Item::Row(pi, i) => {
                let Some(page) = results.pages.get(pi) else { continue };
                let out = row(ui, t, r, results, page, i, drafts, list);
                if out.focused {
                    focused = Some((page.page, i));
                }
                if out.menu_open {
                    menu = Some((page.page, i));
                }
                if out.action.is_some() {
                    action = out.action;
                }
            }
        }
    }
    list.focused = focused;
    list.menu = menu;
    action
}

/// The chips: All, a chip per state that has diagrams (or is the one chosen), and On this page.
fn chips(ui: &mut egui::Ui, t: &Tokens, list: &mut ListState, results: &ChessResults, page_now: usize) {
    let (counts, all) = counts(list, results, page_now);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        if chip(ui, t, None, &format!("{} {all}", tl!("All")), list.filter.is_none()).clicked() {
            list.filter = None;
        }
        for (state, n) in STATES.into_iter().zip(counts) {
            let on = list.filter == Some(state);
            if (n > 0 || on) && chip(ui, t, Some(state), &format!("{} {n}", state.name()), on).on_hover_text(state.hint()).clicked() {
                list.filter = if on { None } else { Some(state) };
            }
        }
        if chip(ui, t, None, tl!("On this page"), list.this_page).clicked() {
            list.this_page = !list.this_page;
        }
    });
}

/// A filter chip, 24 points tall: the state's icon (a check when it is on) and `text`; on, it is
/// filled with the accent's soft colour and outlined.
fn chip(ui: &mut egui::Ui, t: &Tokens, state: Option<DiagramState>, text: &str, on: bool) -> egui::Response {
    let font = theme::medium(12.0);
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, t.text);
    let icon = if on { Some(("check", t.accent_text)) } else { state.map(|s| (s.icon(), s.colour(t))) };
    let lead = if icon.is_some() { 18.0 } else { 0.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x + 20.0 + lead, CHIP_H), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, on, text));
    let (fill, stroke) =
        if on { (t.accent_soft, Stroke::new(1.0, t.accent)) } else { (if resp.hovered() { t.hover } else { t.card }, Stroke::new(1.0, t.border)) };
    ui.painter().rect(rect, egui::CornerRadius::same(12), fill, stroke, egui::StrokeKind::Inside);
    let mut x = rect.min.x + 10.0;
    if let Some((name, colour)) = icon {
        crate::icons::paint(ui, Rect::from_center_size(pos2(x + 7.0, rect.center().y), egui::Vec2::splat(14.0)), name, 14.0, colour);
        x += lead;
    }
    let y = rect.center().y - galley.size().y / 2.0;
    ui.painter().galley(pos2(x, y), galley, t.text);
    crate::widgets::focus_ring(ui, &resp, rect, 12);
    theme::hand(resp)
}

/// A page's header in `rect` ("Page 17", and "· 25 of 703" when its label is not its number):
/// a click goes to the page; its context menu marks the page as checked (plan A8, P2b).
fn page_header(ui: &mut egui::Ui, t: &Tokens, rect: Rect, page: usize, info: &DocInfo, total: usize) -> Option<PanelAction> {
    let number = page.saturating_add(1).to_string();
    let label = info.pages.get(page).map_or_else(|| number.clone(), |p| p.label.clone());
    let head = crate::i18n::fmt(tl!("Page {label}"), &[("label", &label)]);
    let place = (label != number).then(|| crate::i18n::fmt(tl!("· {n} of {m}"), &[("n", &number), ("m", &total.to_string())]));
    let r = Rect::from_min_max(pos2(rect.min.x, rect.min.y + 4.0), rect.max);
    let resp = ui.interact(r, Id::new(("chess-diagram-page", page)), Sense::click());
    let name = place.as_ref().map_or_else(|| head.clone(), |p| format!("{head} {p}"));
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Link, true, &name));
    let font = theme::semibold(12.5);
    let galley = ui.painter().layout_no_wrap(head, font, t.accent_text);
    let width = galley.size().x;
    ui.painter().galley(pos2(r.min.x + 4.0, r.center().y - galley.size().y / 2.0), galley, t.accent_text);
    if let Some(place) = place {
        let left = pos2(r.min.x + 10.0 + width, r.center().y);
        crate::widgets::paint_left(ui, left, &place, theme::regular(12.0), t.text_muted, (r.max.x - left.x).max(0.0));
    }
    crate::widgets::focus_ring(ui, &resp, r, 4);
    let mut action = None;
    resp.context_menu(|ui| {
        if ui
            .button(tl!("Mark the page as checked"))
            .on_hover_text(tl!("Keep every diagram read with confidence on this page as checked; those to review are left for you."))
            .clicked()
        {
            action = Some(PanelAction::VerifyPage(page));
        }
    });
    if theme::hand(resp).on_hover_text(tl!("Go to this page")).clicked() {
        action = Some(PanelAction::GoToPage(page));
    }
    action
}

/// A note under a page's header, in `rect`.
fn note(ui: &egui::Ui, t: &Tokens, rect: Rect, text: &str) {
    crate::widgets::paint_left(ui, rect.left_center() + vec2(8.0, 0.0), text, theme::regular(11.5), t.text_muted, (rect.width() - 12.0).max(0.0));
}

/// What a diagram's row did this frame.
struct RowOut {
    action: Option<PanelAction>,
    focused: bool,
    menu_open: bool,
}

/// Diagram `index` of `page`'s row in `rect`.
#[allow(clippy::too_many_arguments)]
fn row(
    ui: &mut egui::Ui,
    t: &Tokens,
    rect: Rect,
    results: &ChessResults,
    page: &ChessPage,
    index: usize,
    drafts: &[(usize, usize)],
    list: &mut ListState,
) -> RowOut {
    let key = (page.page, index);
    let reading = ChessResults::reading(page, index);
    let kind = results.kind(page.page, index);
    let state = DiagramState::of(reading, kind);
    let (label, _) = crate::chess_ui::status(reading, kind, t);
    let title = crate::i18n::fmt(tl!("Diagram {n}"), &[("n", &index.saturating_add(1).to_string())]);
    let selected = results.selected == Some(key);
    let draft = drafts.contains(&key);
    let resp = ui.interact(rect, row_id(page.page, index), Sense::click());
    let name = if draft { format!("{title} · {label} · {}", tl!("Unconfirmed changes")) } else { format!("{title} · {label}") };
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, &name));
    if list.focus == Some(key) {
        resp.request_focus();
        list.focus = None;
    }
    let focused = resp.has_focus();
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect.shrink2(vec2(0.0, 1.0)), 4.0, t.accent_soft);
    } else if resp.hovered() {
        painter.rect_filled(rect.shrink2(vec2(0.0, 1.0)), 4.0, t.hover);
    }
    if focused {
        painter.rect_stroke(rect.shrink(1.0), 4.0, Stroke::new(2.0, t.accent), egui::StrokeKind::Inside);
    }
    let icon = Rect::from_center_size(pos2(rect.min.x + 16.0, rect.center().y), egui::Vec2::splat(16.0));
    crate::icons::paint(ui, icon, state.icon(), 16.0, state.colour(t));
    if draft {
        crate::chess_widgets::draft_dot(ui.painter(), icon, t);
    }

    // ⋯ while the row is pointed at, selected or focused, or its menu is open.
    let position = reading.and_then(BoardReading::position);
    let fen = results.kinds.get(&key).map(|(_, f)| f.as_str()).or(position.map(|p| p.fen.as_str()));
    let mut action = None;
    let mut menu_open = false;
    let mut focused = focused;
    let mut right = rect.max.x - 6.0;
    // The row that had the keyboard last frame too: Tab, moving it on, takes it away before then.
    if resp.hovered() || selected || focused || list.focused == Some(key) || list.menu == Some(key) {
        let at = Rect::from_center_size(pos2(right - 12.0, rect.center().y), egui::Vec2::splat(24.0));
        right = at.min.x - 4.0;
        let button =
            ui.scope_builder(UiBuilder::new().max_rect(at), |ui| crate::icons::button(ui, "ellipsis", 24.0, false, tl!("Diagram actions"))).inner;
        menu_open = egui::Popup::menu(&button).show(|ui| menu(ui, key, position, fen, &mut action)).is_some();
        // Its ⋯ with the keyboard keeps the row's ⋯ built.
        focused |= button.has_focus();
    }
    let state_text = if draft { format!("{} · {}", state.name(), tl!("Unconfirmed changes")) } else { state.name().to_owned() };
    let galley = ui.painter().layout_no_wrap(state_text, theme::regular(12.0), t.text_muted);
    let state_left = (right - galley.size().x).max(rect.min.x + 90.0);
    ui.painter().galley(pos2(state_left, rect.center().y - galley.size().y / 2.0), galley, t.text_muted);
    crate::widgets::paint_left(
        ui,
        pos2(rect.min.x + 32.0, rect.center().y),
        &title,
        theme::regular(13.0),
        t.text,
        (state_left - rect.min.x - 38.0).max(0.0),
    );

    let tip = match fen {
        Some(fen) => format!("{name}\n{fen}\n{}", state.hint()),
        None => format!("{name}\n{}", state.hint()),
    };
    if resp.clicked() {
        action = Some(PanelAction::Select(page.page, index));
    }
    resp.context_menu(|ui| menu(ui, key, position, fen, &mut action));
    theme::hand(resp).on_hover_text(tip);
    RowOut { action, focused, menu_open }
}

/// A row's menu: Open on the board, Analyze, Copy FEN and Open in lichess, for a diagram with a
/// position.
fn menu(
    ui: &mut egui::Ui,
    (page, index): (usize, usize),
    position: Option<&pdfcraft_engine::chess::pdfcraft_chessvision::board::ReadPosition>,
    fen: Option<&str>,
    action: &mut Option<PanelAction>,
) {
    if ui.add_enabled(position.is_some(), egui::Button::new(tl!("Open on the board"))).clicked() {
        *action = Some(PanelAction::OpenBoard(page, index));
    }
    let icon = crate::icons::image("gauge", 14.0, crate::theme::Tokens::get(ui.ctx()).icon);
    if ui
        .add_enabled(position.is_some(), egui::Button::image_and_text(icon, tl!("Analyze")))
        .on_hover_text(tl!("Open on the board with the engine analysing it"))
        .clicked()
    {
        *action = Some(PanelAction::Analyze(page, index));
    }
    if ui.add_enabled(fen.is_some(), egui::Button::new(tl!("Copy FEN"))).clicked()
        && let Some(fen) = fen
    {
        *action = Some(PanelAction::CopyFen(fen.to_owned()));
    }
    if let Some(url) = position.and_then(pdfcraft_engine::chess::lichess_url)
        && ui.button(tl!("Open in lichess")).on_hover_text(&url).clicked()
    {
        *action = Some(PanelAction::OpenLichess(url));
    }
}

#[cfg(test)]
mod tests;
