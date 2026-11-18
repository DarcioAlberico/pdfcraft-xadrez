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
//!
//! Not here yet (the contract's phase 2): a preview of the move's position after the pointer
//! rests on it, closed by Esc (WCAG 1.4.13).

use egui::{Color32, CornerRadius, Pos2, Rect, Stroke, vec2};
use pdfcraft_engine::DocId;
use pdfcraft_engine::chess::ScanStamp;
use pdfcraft_engine::chess::pdfcraft_chessvision::doc::{ChessDoc, MoveAt, MoveInline, UserRect, move_at};
use pdfcraft_render::DocInfo;

use crate::PdfCraftApp;
use crate::canvas::PageXform;
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

/// The move printed at `point` (user space) on page `page` of `doc` that a double-click opens, and
/// its box.
pub(crate) fn opening_at(doc: &ChessDoc, page: usize, point: [f32; 2]) -> Option<(MoveAt, UserRect)> {
    let (at, _) = move_at(doc, u32::try_from(page).ok()?, point)?;
    let m = at.find(doc)?;
    open_action(m, at)?;
    Some((at, m.source_rect?))
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
/// box's foot, which the page's rotation and the view's turn with the box.
pub(crate) fn paint_underline(painter: &egui::Painter, xf: &PageXform, info: &DocInfo, page: usize, rect: UserRect, t: &Tokens) {
    // `user_rect` reads the page's box: never for a page the document does not have.
    if info.pages.get(page).is_none() {
        return;
    }
    let foot = xf.user_rect(info, page, [rect.x0, rect.y0.min(rect.y1), rect.x1, rect.y0.min(rect.y1)]);
    // The foot is a segment: one of the rectangle's sides is empty, whichever way the page turns.
    painter.line_segment([foot.min, foot.max], Stroke::new(1.0, t.accent));
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
}
