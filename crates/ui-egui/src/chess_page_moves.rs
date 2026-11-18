//! Chess ▸ a printed move of the page, clicked (plan A9-N1, §2.9 "Lance clicável no PDF"): a
//! double-click on a move printed on the page opens its game on the board at that move, as a
//! double-click on it in the Text tab does (the book's PGN's game when the move is linked to one,
//! else the page's game read on across the pages around it, `read_book_game`; the same untouched
//! game is shown again, never added twice). The word stays selected, as any double-click leaves
//! it. Alt+click does the same without selecting.
//!
//! - **The page read.** The moves are the Text tab's reading of the page (`ChessState::text_of`,
//!   one page at a time, at the document's state now). On a page not read, the gesture reads it
//!   first as the Text tab does, on demand, in its job ([`MoveClick`]), saying "Reading the page…"
//!   at the foot of the page, then opens the move found there. On a page whose diagrams were not
//!   looked for, a double-click looks for a diagram first, as before (`chess_ui::BoardAt`), and
//!   only where there is none goes on to the move; a double-click on prose does nothing new.
//! - **The affordance.** Over a move of the page read that opens a game, the pointer is a hand and
//!   the move is underlined, 1 point in `accent`.
//! - **One mapping.** The pointer's point goes from the screen to the displayed page
//!   (`PageXform::screen_to_view`) and then to user space (`PageInfo::view_to_user`, the page's
//!   crop box and `/Rotate`): the inverse of `PageXform::user_rect`, which shows a move's box on
//!   the page (the Text tab's Show in PDF, the underline here). The move under it is
//!   `chessvision::doc::move_at`, the function the `chess_move_at` tool calls too.
//! - **The preview** (the contract's phase 2, plan A9-N1b). Once the pointer rests on such a move
//!   for as long as on a move of the list (`chess_board_ui::PREVIEW_DELAY`), a small board of the
//!   position after it shows beside the move, never over it and inside the window: the move list's
//!   picture of a position (`chess_board_ui::position_picture`), from the position the reading
//!   holds (`MoveInline::fen`), drawn once while the pointer stays on the move. A move with no
//!   position shows none. It goes when the pointer leaves the move (it may go onto the preview),
//!   on Esc, which leaves the keyboard where it was ([`take_escape`]), on a scroll, a zoom or
//!   another page, and when the move's game opens. The move list's switch turns it off too
//!   (`no_move_preview`): one switch for a move's position on hover. WCAG 1.4.13: dismissable,
//!   hoverable, persistent.

use egui::{Color32, CornerRadius, Event, Key, Pos2, Rect, Stroke, TextureHandle, Vec2, pos2, vec2};
use pdfcraft_engine::DocId;
use pdfcraft_engine::chess::ScanStamp;
use pdfcraft_engine::chess::pdfcraft_chess::fen::Fen;
use pdfcraft_engine::chess::pdfcraft_chess::placement::Color;
use pdfcraft_engine::chess::pdfcraft_chessvision::doc::{ChessDoc, MoveAt, MoveInline, UserRect, move_at};
use pdfcraft_render::DocInfo;

use crate::PdfCraftApp;
use crate::canvas::PageXform;
use crate::chess_board_ui::{PREVIEW_DELAY, PREVIEW_SIDE};
use crate::chess_text_ui::TextAction;
use crate::chess_ui::ChessState;
use crate::theme::{self, Tokens};

/// A double-click or an Alt+click at `point` (user space) on `page` of `doc`, for the printed move
/// there, waiting for the page's reading when it was not read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MoveClick {
    pub doc: DocId,
    pub page: usize,
    pub point: [f32; 2],
    /// Finding no move there says "No chess diagram was found here.": the double-click of a page
    /// whose diagrams were not looked for, found none there and selected no word
    /// (`chess_ui::BoardAt::on_text`), as it said before.
    pub say_none: bool,
    /// The page's reading was started for it.
    pub started: bool,
}

/// The page of document `id` the Text tab read, and its reading, while the document is in the
/// state it was read in (`stamp`, the document's state now).
pub(crate) fn read_page(state: &ChessState, id: DocId, stamp: Option<ScanStamp>) -> Option<(usize, &ChessDoc)> {
    state.text_of(id).filter(|t| Some(t.stamp) == stamp).map(|t| (t.page, &t.chess))
}

/// The game and node of the book's PGN that move `m` is linked to (plan "Carregar PGN do livro",
/// R6c): what a double-click on it opens, before the page's own game.
pub(crate) fn in_pgn(m: &MoveInline) -> Option<(usize, usize)> {
    m.reference.as_ref().and_then(|r| Some((r.game?, r.node?)))
}

/// What a double-click on move `m` at `at` does, as the Text tab's double-click: open the book's
/// PGN's game when the move is linked to one, else the page's game when it has one or may go on
/// from the page before; `None` for a move that opens nothing (one the walk refused, a caption).
pub(crate) fn open_action(m: &MoveInline, at: MoveAt) -> Option<TextAction> {
    match in_pgn(m) {
        Some((game, node)) => Some(TextAction::OpenPgnGame { game, node }),
        None => m.opens_game().then_some(TextAction::OpenGame(at)),
    }
}

/// The move printed at `point` (user space) on page `page` of `doc` that a double-click opens: its
/// place, the move and its box.
pub(crate) fn opening_at(doc: &ChessDoc, page: usize, point: [f32; 2]) -> Option<(MoveAt, &MoveInline, UserRect)> {
    let (at, _) = move_at(doc, u32::try_from(page).ok()?, point)?;
    let m = at.find(doc)?;
    open_action(m, at)?;
    Some((at, m, m.source_rect?))
}

/// The point of `page` under the screen point `p` in user space: the inverse of
/// [`PageXform::user_rect`], through the page's view space. `None` off the page, and for a page
/// the document does not have.
pub(crate) fn user_point(xf: &PageXform, info: &DocInfo, page: usize, p: Pos2) -> Option<[f32; 2]> {
    if !xf.rect.contains(p) {
        return None;
    }
    let (vx, vy) = xf.screen_to_view(p);
    let [x, y] = info.pages.get(page)?.view_to_user(vx, vy);
    (x.is_finite() && y.is_finite()).then_some([x, y])
}

/// Underline the move whose box is `rect` (user space) on `page`: 1 point in `accent` along its
/// box's foot, which the page's rotation and the view's turn with the box. The move's box on
/// screen; `None` for a page the document does not have.
pub(crate) fn paint_underline(painter: &egui::Painter, xf: &PageXform, info: &DocInfo, page: usize, rect: UserRect, t: &Tokens) -> Option<Rect> {
    // `user_rect` reads the page's box: never for a page the document does not have.
    info.pages.get(page)?;
    let foot = xf.user_rect(info, page, [rect.x0, rect.y0.min(rect.y1), rect.x1, rect.y0.min(rect.y1)]);
    // The foot is a segment: one of the rectangle's sides is empty, whichever way the page turns.
    painter.line_segment([foot.min, foot.max], Stroke::new(1.0, t.accent));
    Some(xf.user_rect(info, page, [rect.x0, rect.y0, rect.x1, rect.y1]))
}

/// A move of the page read under the pointer, as a canvas found it this frame (plan A9-N1b): its
/// page, its place, the move and its box on screen.
pub(crate) struct Under<'a> {
    pub page: usize,
    pub at: MoveAt,
    pub inline: &'a MoveInline,
    pub rect: Rect,
}

/// The page's move preview: the move under the pointer and since when, kept in the context's
/// memory (one pointer, one preview for the window).
#[derive(Clone)]
struct Preview {
    /// The canvas that found the move: a window may show two documents.
    canvas: egui::Id,
    /// The move: its document, its page and its place there.
    key: (DocId, usize, MoveAt),
    /// Its box on screen when the pointer came onto it: a scroll, a zoom or another page moves the
    /// box, and the wait starts again.
    anchor: Rect,
    /// When the pointer came onto it (`InputState::time`).
    since: f64,
    /// Closed by Esc or by the gesture that opened the move's game: closed until the pointer
    /// leaves the move.
    closed: bool,
    /// The picture, made when it first shows and kept while the pointer stays on the move, with
    /// the look it was made in (`chess_board_ui::picture_look`); `None` inside when the position
    /// could not be drawn, which is not tried again in that look.
    picture: Option<((&'static str, u64), Option<Picture>)>,
    /// Where it showed, and in which pass (`Context::cumulative_pass_nr`).
    shown: Option<(Rect, u64)>,
    /// The last pass its canvas looked at it in: one not drawn since lets it go.
    seen: u64,
}

/// The preview's picture of the position after a move, and its name ("Position after 12…e5", as
/// the move list's preview names its own).
#[derive(Clone)]
struct Picture {
    texture: TextureHandle,
    name: String,
}

fn preview_id() -> egui::Id {
    egui::Id::new("chess-page-move-preview")
}

/// The gap between the move and its preview, and between the preview and the window's edge.
const GAP: f32 = 6.0;

/// How far the pointer may go from where a gesture opened a move, its preview staying closed
/// before the move is under the pointer: a page read for the gesture takes a moment.
const OPENED_REACH: f32 = 8.0;

impl Preview {
    /// Whether the pointer at `p` keeps the preview shown with no move under it: on the preview, or
    /// on its way there from the move (WCAG 1.4.13, hoverable).
    fn keeps(&self, p: Pos2) -> bool {
        self.shown.is_some_and(|(r, _)| r.contains(p) || bridge(self.anchor, r).contains(p))
    }
}

/// The preview of the position after the move under the pointer (plan A9-N1b; the move list's
/// own, plan HC13, for a move printed on the page). Called once a frame by each canvas after its
/// pages: canvas `canvas` of document `doc` found `under` under the pointer (`None`: no move of
/// the page read with a game there, or the pointer not on its pages); `allowed`: the canvas takes
/// the page's gestures (the Select tool, nothing over the page); `opened`: a double-click or an
/// Alt+click on its pages opens the move there, if any.
pub(crate) fn preview(ctx: &egui::Context, canvas: egui::Id, doc: DocId, under: Option<Under<'_>>, allowed: bool, opened: bool) {
    let id = preview_id();
    let opened_id = id.with("opened");
    let pass = ctx.cumulative_pass_nr();
    let (stored, opened_before) = ctx.data(|d| (d.get_temp::<Preview>(id), d.get_temp::<Pos2>(opened_id)));
    // No move under the pointer, nothing shown or waiting: nothing to do (most frames).
    if under.is_none() && stored.is_none() && opened_before.is_none() && !opened {
        return;
    }
    let kept = stored.is_some();
    let old = stored.filter(|p| p.seen.saturating_add(1) >= pass);
    // Another canvas's, and the pointer is not on this one's moves.
    if under.is_none() && old.as_ref().is_some_and(|p| p.canvas != canvas) {
        return;
    }
    let (now, pointer, pressed) = ctx.input(|i| (i.time, i.pointer.hover_pos(), i.pointer.any_down()));
    let scrolled = ctx.input(|i| i.events.iter().any(|e| matches!(e, Event::MouseWheel { .. } | Event::Zoom(_))));
    // Where a gesture opened a move: the move there comes closed, also one whose page was read
    // only for the gesture (no move under the pointer yet when it was made). Forgotten once the
    // pointer goes away from that point.
    let opened_at = if opened { pointer } else { opened_before };
    let opened_at = opened_at.filter(|q| pointer.is_some_and(|p| p.distance(*q) <= OPENED_REACH));
    if opened_at != opened_before {
        ctx.data_mut(|d| match opened_at {
            Some(q) => {
                d.insert_temp(opened_id, q);
            }
            None => d.remove::<Pos2>(opened_id),
        });
    }
    let fresh = |u: &Under<'_>, key| {
        let closed = opened_at.is_some_and(|q| u.rect.contains(q));
        Preview { canvas, key, anchor: u.rect, since: now, closed, picture: None, shown: None, seen: pass }
    };
    let next = match &under {
        _ if !allowed || !crate::chess_figurines::preview_on(ctx) => None,
        // A move with no position: no preview.
        Some(u) if u.inline.fen.is_none() => None,
        Some(u) => {
            let key = (doc, u.page, u.at);
            Some(match old {
                // Moved under the pointer (a scroll, a zoom, another page): wait again.
                Some(p) if p.key == key && (scrolled || moved(p.anchor, u.rect)) => Preview { canvas, anchor: u.rect, since: now, shown: None, ..p },
                Some(p) if p.key == key => Preview { canvas, ..p },
                _ => fresh(u, key),
            })
        }
        None => old.filter(|p| !p.closed && !scrolled && !pressed && pointer.is_some_and(|q| p.keeps(q))),
    };
    let Some(mut p) = next else {
        if kept {
            ctx.data_mut(|d| d.remove::<Preview>(id));
        }
        return;
    };
    p.seen = pass;
    p.closed |= opened;
    let waited = now - p.since;
    if p.closed || waited < PREVIEW_DELAY {
        if !p.closed
            && let Ok(left) = std::time::Duration::try_from_secs_f64(PREVIEW_DELAY - waited)
        {
            ctx.request_repaint_after(left);
        }
        p.shown = None;
        ctx.data_mut(|d| d.insert_temp(id, p));
        return;
    }
    // Drawn once while the pointer stays, again only in another theme or pieces (with the move
    // under the pointer: on the preview, a new look hides it).
    let look = crate::chess_board_ui::picture_look(ctx);
    if p.picture.as_ref().is_none_or(|(l, _)| *l != look) {
        p.picture = under.as_ref().map(|u| (look, picture(ctx, u.inline)));
    }
    p.shown = p.picture.as_ref().and_then(|(_, pic)| pic.as_ref()).map(|pic| {
        let frame = egui::Frame::popup(&ctx.global_style());
        let size = Vec2::splat(PREVIEW_SIDE) + frame.total_margin().sum();
        // Placed once, where it first shows. Its own rectangle, not the area's response: on the
        // first pass an area shown again answers with where it was last time.
        let at = match p.shown {
            Some((r, _)) => r.min,
            None => place(p.anchor, pointer.unwrap_or(p.anchor.center()), size, ctx.content_rect()),
        };
        egui::Area::new(id).order(egui::Order::Tooltip).fixed_pos(at).show(ctx, |ui| {
            frame.show(ui, |ui| {
                let image = egui::Image::new(&pic.texture).fit_to_exact_size(Vec2::splat(PREVIEW_SIDE)).sense(egui::Sense::hover());
                let r = ui.add(image);
                r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, &pic.name));
            });
        });
        (Rect::from_min_size(at, size), pass)
    });
    ctx.data_mut(|d| d.insert_temp(id, p));
}

/// Whether the move's box went from `was` to `now` on screen: the page scrolled, zoomed or turned.
fn moved(was: Rect, now: Rect) -> bool {
    (was.min - now.min).length() > 0.5 || (was.max - now.max).length() > 0.5
}

/// The preview's picture of the position after move `m`; `None` for a move with no position (or
/// one that does not read).
fn picture(ctx: &egui::Context, m: &MoveInline) -> Option<Picture> {
    let fen = Fen::parse(m.fen.as_deref()?).ok()?;
    // As printed, White at the foot: the page's diagrams are.
    let texture = crate::chess_board_ui::position_picture(ctx, fen.placement(), false)?;
    let san = crate::chess_figurines::Notation::current().san(if m.san.is_empty() { m.read_san() } else { &m.san });
    let name = crate::i18n::fmt(tl!("Position after {move}"), &[("move", &numbered(&san, &fen))]);
    Some(Picture { texture, name })
}

/// Move `san` with its number, as the move list writes it ("12.Nf3", "12…e5"), from `after`, the
/// position after it: Black to move, White made it. Just `san` when the position does not say
/// who is to move.
fn numbered(san: &str, after: &Fen) -> String {
    match after.side() {
        Some(Color::Black) => format!("{}.{san}", after.fullmove_number()),
        Some(Color::White) => format!("{}…{san}", after.fullmove_number().saturating_sub(1).max(1)),
        None => san.to_owned(),
    }
}

/// Where the preview of size `size` goes for the move whose box on screen is `anchor`, the pointer
/// at `pointer`: under the move, else over it, else beside it, [`GAP`] away from it and inside
/// `window` with the same gap. Under or over, it starts just left of the pointer, as a tooltip
/// does, so the pointer goes straight onto it. Over the move only when the window has no room for
/// it anywhere else.
fn place(anchor: Rect, pointer: Pos2, size: Vec2, window: Rect) -> Pos2 {
    let inner = window.shrink(GAP);
    // `lo` wins over `hi`: a window narrower than the preview keeps its left or top edge.
    let fit = |v: f32, lo: f32, hi: f32| v.min(hi).max(lo);
    let x = fit(pointer.x - 2.0 * GAP, inner.left(), inner.right() - size.x);
    let y = fit(pointer.y - size.y / 2.0, inner.top(), inner.bottom() - size.y);
    if anchor.bottom() + GAP + size.y <= inner.bottom() {
        pos2(x, anchor.bottom() + GAP)
    } else if anchor.top() - GAP - size.y >= inner.top() {
        pos2(x, anchor.top() - GAP - size.y)
    } else if anchor.right() + GAP + size.x <= inner.right() {
        pos2(anchor.right() + GAP, y)
    } else if anchor.left() - GAP - size.x >= inner.left() {
        pos2(anchor.left() - GAP - size.x, y)
    } else {
        pos2(x, y)
    }
}

/// The strip between rectangles `a` and `b` that the pointer crosses going from one to the other:
/// on each axis, where they overlap, else the gap between them.
fn bridge(a: Rect, b: Rect) -> Rect {
    let span = |a0: f32, a1: f32, b0: f32, b1: f32| {
        if a1 < b0 {
            (a1, b0)
        } else if b1 < a0 {
            (b1, a0)
        } else {
            (a0.max(b0), a1.min(b1))
        }
    };
    let (x0, x1) = span(a.left(), a.right(), b.left(), b.right());
    let (y0, y1) = span(a.top(), a.bottom(), b.top(), b.bottom());
    Rect::from_min_max(pos2(x0, y0), pos2(x1, y1))
}

/// Esc closes the page's move preview while it shows (WCAG 1.4.13, dismissable), taken out of this
/// frame's `raw` input before egui, which would take the keyboard focus away on it, or a key of the
/// app (Esc lets go of the selection, closes the find bar) can see it: the keyboard stays where it
/// was. `blocked`: a modal dialog or the command search has the keys.
pub(crate) fn take_escape(ctx: &egui::Context, raw: &mut egui::RawInput, blocked: bool) {
    let esc = |e: &Event| matches!(e, Event::Key { key: Key::Escape, pressed: true, modifiers, .. } if modifiers.is_none());
    if blocked || !raw.events.iter().any(esc) {
        return;
    }
    let id = preview_id();
    let Some(mut p) = ctx.data(|d| d.get_temp::<Preview>(id)) else { return };
    // Shown in the pass before this input: on screen now.
    let showing = p.shown.is_some_and(|(_, pass)| pass.saturating_add(1) == ctx.cumulative_pass_nr());
    if p.closed || !showing {
        return;
    }
    raw.events.retain(|e| !esc(e));
    p.closed = true;
    p.shown = None;
    ctx.data_mut(|d| d.insert_temp(id, p));
}

/// "Reading the page…" at the foot of page `index`, whose screen rectangle is `page`, kept inside
/// the part of it on screen (`visible`), while the page is read for a gesture. Said politely to
/// screen readers when it appears.
pub(crate) fn paint_reading(ui: &egui::Ui, index: usize, page: Rect, visible: Rect, t: &Tokens) {
    let shown = page.intersect(visible);
    if !shown.is_positive() {
        return;
    }
    let text = tl!("Reading the page…");
    let galley = ui.painter().layout_no_wrap(text.to_owned(), theme::regular(12.5), t.text);
    let size = galley.size() + vec2(24.0, 12.0);
    let bottom = shown.bottom() - 12.0;
    let rect = Rect::from_min_size(egui::pos2(shown.center().x - size.x / 2.0, bottom - size.y), size);
    let painter = ui.painter();
    painter.rect(rect, CornerRadius::same(t.radius), t.card, Stroke::new(1.0, t.border_strong), egui::StrokeKind::Inside);
    painter.galley(rect.min + vec2(12.0, 6.0), galley, Color32::PLACEHOLDER);
    let id = ui.id().with(("chess-page-reading", index));
    let response = ui.interact(rect, id, egui::Sense::hover());
    // Its text as its name (a Label's text would be its value), on a status node.
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, text));
    ui.ctx().accesskit_node_builder(id, |node| {
        node.set_role(egui::accesskit::Role::Status);
        node.set_live(egui::accesskit::Live::Polite);
    });
}

impl PdfCraftApp {
    /// A printed move clicked on the page ([`MoveClick`]): once the Text tab's reading of its page
    /// is in, the move there opens as a double-click on it in the Text tab does; the page is read
    /// first (after any reading running) when it was not. Dropped when its document is no longer
    /// the one shown, and when the reading ended without the page (another page's reading took
    /// its place, or the document changed meanwhile).
    pub(crate) fn chess_move_click_tick(&mut self) {
        let Some(mut want) = self.chess.move_click else { return };
        if self.active_ids().map(|(_, id)| id) != Some(want.doc) {
            self.chess.move_click = None;
            return;
        }
        if self.settle_move_click(&want) {
            return;
        }
        if self.chess.reading.is_some() {
            return;
        }
        if want.started {
            self.chess.move_click = None;
            return;
        }
        want.started = true;
        self.chess.move_click = Some(want);
        self.read_chess_text(want.page, false);
        // Read inline (the web, tests): the move opens now, before anything reads another page.
        self.settle_move_click(&want);
    }

    /// When the page of `want` is read: open the move under its point, or say there is none when
    /// it is to be said, and let the gesture go. Whether the page was read.
    fn settle_move_click(&mut self, want: &MoveClick) -> bool {
        let stamp = self.session.chess_stamp(want.doc);
        let Some((page, doc)) = read_page(&self.chess, want.doc, stamp) else { return false };
        if page != want.page {
            return false;
        }
        let action = u32::try_from(page).ok().and_then(|p| move_at(doc, p, want.point)).and_then(|(at, _)| open_action(at.find(doc)?, at));
        self.chess.move_click = None;
        match action {
            Some(action) => self.chess_text_action(action),
            None if want.say_none => self.inform(tl!("No chess diagram was found here.").to_string()),
            None => {}
        }
        true
    }
}

/// Whether a gesture on page `page` of document `id` (at `stamp`, its state now) waits for the
/// page to be read: a move clicked on a page not read yet ([`MoveClick`]), or a double-click
/// looking for a diagram on a page not scanned yet (`chess_ui::BoardAt`).
pub(crate) fn reading(state: &ChessState, id: DocId, stamp: Option<ScanStamp>, page: usize) -> bool {
    let read = read_page(state, id, stamp).is_some_and(|(p, _)| p == page);
    let move_wait = state.move_click.is_some_and(|c| c.doc == id && c.page == page && !read);
    let board_wait = state.board_at.is_some_and(|b| b.doc == id && b.page == page);
    move_wait || board_wait
}

#[cfg(test)]
mod tests {
    use pdfcraft_engine::chess::pdfcraft_chessvision::doc::reference::{LinkKind, MoveReference};
    use pdfcraft_engine::chess::pdfcraft_chessvision::doc::{MoveStatus, NodeRef};

    use super::*;

    fn a_move(status: MoveStatus, game: Option<NodeRef>, reference: Option<MoveReference>) -> MoveInline {
        MoveInline {
            number: None,
            text: "e5".into(),
            san_read: "e5".into(),
            san: "e5".into(),
            status,
            fen: None,
            game,
            source_rect: None,
            bold: false,
            read_review: Vec::new(),
            review: Vec::new(),
            reference,
        }
    }

    /// The page's double-click does what the Text tab's does: the book's PGN's game first, then
    /// the page's game when the move has one or may go on from the page before; a move refused,
    /// or linked to no node of the PGN, opens the page's game or nothing.
    #[test]
    fn a_move_opens_the_pgns_game_then_the_pages() {
        let at = MoveAt { page: 3, block: 1, inline: 2 };
        let node = Some(NodeRef { game: 0, node: 4 });
        let linked = |game, node| Some(MoveReference { kind: LinkKind::Confirmed, game, node, source: 0, fen: None });
        assert_eq!(open_action(&a_move(MoveStatus::Valid, node, linked(Some(2), Some(7))), at), Some(TextAction::OpenPgnGame { game: 2, node: 7 }));
        assert_eq!(open_action(&a_move(MoveStatus::Valid, node, None), at), Some(TextAction::OpenGame(at)));
        assert_eq!(open_action(&a_move(MoveStatus::Valid, node, linked(None, None)), at), Some(TextAction::OpenGame(at)));
        assert_eq!(open_action(&a_move(MoveStatus::Unchecked, None, None), at), Some(TextAction::OpenGame(at)));
        for refused in [MoveStatus::Illegal, MoveStatus::Ambiguous, MoveStatus::NotReached, MoveStatus::Caption] {
            assert_eq!(open_action(&a_move(refused, None, None), at), None, "{refused:?}");
        }
    }

    /// Plan A9-N1b: wherever the move is in the window, its preview goes inside the window and off
    /// the move, under it when there is room, else over it, else beside it; over it only when the
    /// window has room nowhere else. A window too small for it keeps its top left corner in.
    #[test]
    fn the_preview_goes_beside_the_move_inside_the_window() {
        let window = Rect::from_min_size(Pos2::ZERO, vec2(1366.0, 768.0));
        let size = vec2(180.0, 180.0);
        for ax in (0..=1366).step_by(23) {
            for ay in (0..=768).step_by(17) {
                let anchor = Rect::from_min_size(pos2(ax as f32 - 20.0, ay as f32 - 6.0), vec2(40.0, 12.0));
                let pointer = anchor.center();
                let shown = Rect::from_min_size(place(anchor, pointer, size, window), size);
                assert!(window.shrink(GAP - 0.01).contains_rect(shown), "{anchor:?}: {shown:?} out of the window");
                assert!(!shown.intersects(anchor), "{anchor:?}: {shown:?} covers the move");
            }
        }
        // Room below: just under the move, starting just left of the pointer.
        let anchor = Rect::from_min_size(pos2(600.0, 300.0), vec2(40.0, 12.0));
        assert_eq!(place(anchor, pos2(610.0, 306.0), size, window), pos2(598.0, 312.0 + GAP));
        // At the foot of the window: over it; at its right edge, pushed in.
        let low = anchor.translate(vec2(0.0, 440.0));
        assert_eq!(place(low, pos2(610.0, 746.0), size, window), pos2(598.0, 740.0 - GAP - 180.0));
        let corner = low.translate(vec2(700.0, 0.0));
        assert_eq!(place(corner, pos2(1310.0, 746.0), size, window), pos2(1366.0 - GAP - 180.0, 740.0 - GAP - 180.0));
        // A window too short for it above or below: beside the move, on the right, else the left.
        let short = Rect::from_min_size(Pos2::ZERO, vec2(1366.0, 300.0));
        let mid = Rect::from_min_size(pos2(600.0, 144.0), vec2(40.0, 12.0));
        assert_eq!(place(mid, mid.center(), size, short).x, 640.0 + GAP);
        let right = mid.translate(vec2(700.0, 0.0));
        assert_eq!(place(right, right.center(), size, short).x, 1300.0 - GAP - 180.0);
        // No room anywhere: inside the window, over the move.
        let tiny = Rect::from_min_size(Pos2::ZERO, vec2(100.0, 100.0));
        assert_eq!(place(Rect::from_min_size(pos2(30.0, 40.0), vec2(40.0, 12.0)), pos2(50.0, 46.0), size, tiny), pos2(GAP, GAP));
    }

    /// The strip the pointer crosses from the move to its preview: the gap between them, as wide as
    /// they overlap; touching or overlapping rectangles give their common edge or part.
    #[test]
    fn the_bridge_is_the_gap_between_the_move_and_its_preview() {
        let anchor = Rect::from_min_max(pos2(100.0, 100.0), pos2(140.0, 112.0));
        let below = Rect::from_min_max(pos2(30.0, 118.0), pos2(210.0, 298.0));
        assert_eq!(bridge(anchor, below), Rect::from_min_max(pos2(100.0, 112.0), pos2(140.0, 118.0)));
        let above = Rect::from_min_max(pos2(120.0, 0.0), pos2(300.0, 94.0));
        assert_eq!(bridge(anchor, above), Rect::from_min_max(pos2(120.0, 94.0), pos2(140.0, 100.0)));
        let beside = Rect::from_min_max(pos2(146.0, 20.0), pos2(326.0, 200.0));
        assert_eq!(bridge(anchor, beside), Rect::from_min_max(pos2(140.0, 100.0), pos2(146.0, 112.0)));
        // A point of the gap keeps the preview; one beside the move on its line does not.
        let p = Preview {
            canvas: egui::Id::NULL,
            key: (DocId(1), 0, MoveAt { page: 0, block: 0, inline: 0 }),
            anchor,
            since: 0.0,
            closed: false,
            picture: None,
            shown: Some((below, 0)),
            seen: 0,
        };
        assert!(p.keeps(pos2(120.0, 115.0)) && p.keeps(pos2(40.0, 200.0)));
        assert!(!p.keeps(pos2(160.0, 106.0)) && !p.keeps(pos2(120.0, 320.0)));
        assert!(!Preview { shown: None, ..p }.keeps(pos2(40.0, 200.0)), "not shown, nothing to keep");
    }

    /// Plan A9-N1b: Esc is taken out of the input, and the preview closed, only while a preview
    /// showed in the pass before (not one closed, not one shown earlier), and with no modifier or
    /// dialog in the way; any other Esc goes on to egui and the app.
    #[test]
    fn esc_is_taken_only_while_a_preview_shows() {
        let ctx = egui::Context::default();
        ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
        ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
        let last = ctx.cumulative_pass_nr() - 1;
        let esc = |modifiers| egui::RawInput {
            events: vec![Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers }],
            ..Default::default()
        };
        let preview = |shown: Option<u64>, closed: bool| Preview {
            canvas: egui::Id::NULL,
            key: (DocId(1), 0, MoveAt { page: 0, block: 0, inline: 0 }),
            anchor: Rect::from_min_size(Pos2::ZERO, vec2(20.0, 10.0)),
            since: 0.0,
            closed,
            picture: None,
            shown: shown.map(|pass| (Rect::from_min_size(pos2(0.0, 16.0), vec2(180.0, 180.0)), pass)),
            seen: last,
        };
        let taken = |p: Option<Preview>, modifiers, blocked| {
            ctx.data_mut(|d| match p {
                Some(p) => {
                    d.insert_temp(preview_id(), p);
                }
                None => d.remove::<Preview>(preview_id()),
            });
            let mut raw = esc(modifiers);
            take_escape(&ctx, &mut raw, blocked);
            let closed = ctx.data(|d| d.get_temp::<Preview>(preview_id())).is_some_and(|p| p.closed && p.shown.is_none());
            (raw.events.is_empty(), closed)
        };
        assert_eq!(taken(Some(preview(Some(last), false)), egui::Modifiers::NONE, false), (true, true), "shown: taken, closed");
        assert_eq!(taken(Some(preview(Some(last), false)), egui::Modifiers::NONE, true), (false, false), "a dialog has the keys");
        assert_eq!(taken(Some(preview(Some(last), false)), egui::Modifiers::SHIFT, false), (false, false), "Shift+Esc");
        assert_eq!(taken(Some(preview(Some(last - 1), false)), egui::Modifiers::NONE, false), (false, false), "shown earlier, not now");
        assert_eq!(taken(Some(preview(None, false)), egui::Modifiers::NONE, false), (false, false), "waiting, not shown");
        assert!(!taken(Some(preview(Some(last), true)), egui::Modifiers::NONE, false).0, "closed already");
        assert_eq!(taken(None, egui::Modifiers::NONE, false), (false, false), "no preview");
    }

    /// The preview's name gives the move its number as the move list does, from the position
    /// after it.
    #[test]
    fn the_preview_names_the_move_with_its_number() {
        let after = |fen: &str| Fen::parse(fen).unwrap();
        assert_eq!(numbered("e4", &after("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1")), "1.e4");
        assert_eq!(numbered("e5", &after("rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2")), "1…e5");
        assert_eq!(numbered("Nf6", &after("8/8/8/8/8/8/8/K6k w - - 0 13")), "12…Nf6");
        assert_eq!(numbered("Kb2", &after("8/8/8/8/8/8/8/K6k")), "Kb2", "who is to move not said");
    }
}
