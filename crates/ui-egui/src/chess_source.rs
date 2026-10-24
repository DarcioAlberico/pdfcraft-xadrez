//! Chess ▸ PGN toolbar ▸ Source (T8, spec §4.2 and §4.10 step 8): a book's game on the board,
//! and the board's moves back on the page.
//!
//! - **A book's game on the board.** In the Text tab, a move belongs to one of the book's games.
//!   A double-click on it, or "Open this game on the board" in its menu, puts that game on the
//!   board ([`PdfCraftApp::read_book_game`], then [`PdfCraftApp::put_book_game`]): the pages
//!   around the page are read too (`engine::chess::span`, up to four each way, off the interface
//!   thread), so a game the book runs on from the page before or onto the page after comes whole,
//!   and a move at the top of a page that goes on from the page before (no position on its page
//!   alone) finds its game there. It is built move by move from the walker's score
//!   (`ChessDoc::board_game`), with every comment, glyph and variation, each move keeping where it
//!   was printed, shown at the move clicked. It is a new game of the document's collection (from
//!   the diagram it started from, when it did), which Undo takes back out as a step of the list;
//!   the same game opened again while untouched is shown again rather than added twice. The rail's tools are then the way to correct what the
//!   walker made of the book. A single click still only outlines the move on the page.
//! - **Show in PDF** (the rail, a move's menu, `chess.source.show`) scrolls the page to where the
//!   move was printed, unless it is on screen already, and outlines it as the Text tab does.
//! - **Follow** (F, `chess.source.follow`) does that whenever the move shown changes: walking the
//!   game walks the book. It is a switch kept for the session ([`SourceState`]). It works with
//!   the chess panel in its own window, as the page stays in the main one. A move with no place
//!   on the page (one played on the board, or a game typed in) shows nothing and says nothing;
//!   the switch's name says so.

use egui::Key;
use pdfcraft_engine::DocId;
use pdfcraft_engine::chess::pdfcraft_chess::game::{Game, NodePath, NodeSource, Token};
use pdfcraft_engine::chess::pdfcraft_chessvision::doc::{ChessDoc, GameOrigin, NodeRef};
use pdfcraft_render::DestView;

use crate::PdfCraftApp;
use crate::chess_board_ui::{BoardState, Source};
use crate::chess_history::ChessEdit;
use crate::chess_tools::{ChessKey, Kind, Target, Tool};

/// Room left above a move the page is scrolled to, in points.
const ROOM_ABOVE: f32 = 72.0;

/// Follow, and what the page was last scrolled to.
#[derive(Default)]
pub struct SourceState {
    follow: bool,
    /// The move Follow last looked at: its game (the game's id) and its path.
    seen: Option<(u64, NodePath)>,
    shown: Option<Shown>,
}

/// A move's place shown on the page.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shown {
    /// The page (0-based) and the box, in user space.
    pub page: usize,
    pub rect: [f32; 4],
    /// The page was scrolled to it (it was not on screen).
    pub scrolled: bool,
}

impl SourceState {
    /// Whether Follow is on.
    pub fn follow(&self) -> bool {
        self.follow
    }

    /// Turn Follow on or off. Turned on, it shows the move shown at once.
    pub fn set_follow(&mut self, on: bool) {
        self.follow = on;
        self.seen = None;
    }

    /// The last move's place shown on the page.
    pub fn shown(&self) -> Option<Shown> {
        self.shown
    }
}

/// The move shown's place on the page, when it has one.
fn current_source(board: &BoardState) -> Option<NodeSource> {
    if board.illegal.is_some() {
        return None;
    }
    board.game.source(board.game.current())
}

/// The rail's Follow switch: its name says when the move shown has no place to show.
pub(crate) const FOLLOW: Tool = Tool {
    id: "follow-source",
    icon: "navigation",
    label: |b| {
        if b.game.current() != 0 && current_source(b).is_none() {
            tl!("Follow the move on the page: this move has no place on the page").to_owned()
        } else {
            tl!("Follow the move on the page").to_owned()
        }
    },
    keys: &[ChessKey::plain(Key::F)],
    command: Some("chess.source.follow"),
    kind: Kind::Switch(|chess| {
        let on = !chess.source.follow;
        chess.source.set_follow(on);
    }),
    enabled: |_| true,
    visible: |_| true,
    heads: None,
    on_move: None,
    on: Some(|chess| chess.source.follow),
};

/// Show in PDF, on the rail (the move shown) and in a move's menu (that move).
pub(crate) const SHOW: Tool = Tool {
    id: "show-source",
    icon: "file-search",
    label: |_| tl!("Show in PDF").to_owned(),
    keys: &[],
    command: Some("chess.source.show"),
    kind: Kind::Action(|board, target| {
        let node = match target {
            Target::Current => board.game.current(),
            Target::Move(node) | Target::Head(node) => node,
        };
        if board.illegal.is_none() && board.game.source(node).is_some() {
            board.show_source = Some(node);
        }
    }),
    enabled: |b| current_source(b).is_some(),
    visible: |_| true,
    heads: None,
    on_move: Some(
        |game, node| {
            if game.source(node).is_some() { Ok(Target::Move(node)) } else { Err(tl!("This move has no place on the page.").to_owned()) }
        },
    ),
    on: None,
};

/// Where each move of `game` was printed, in PGN order: two games read from the same text have
/// the same.
fn places(game: &Game) -> Vec<Option<NodeSource>> {
    game.tokens().into_iter().filter_map(|t| if let Token::Move { node, .. } = t { Some(game.source(node)) } else { None }).collect()
}

impl PdfCraftApp {
    /// Every frame: a move asked to be shown (Show in PDF), then, with Follow on, the move shown
    /// when it changed.
    pub(crate) fn chess_source_tick(&mut self) {
        if self.chess.board().show_source.is_some() {
            let asked = self.chess.board_mut().show_source.take();
            if let Some(source) = asked.and_then(|node| self.chess.board().game.source(node)) {
                self.reveal_chess_source(source);
            }
        }
        if !self.chess.source.follow {
            return;
        }
        let board = self.chess.board();
        let game = &board.game;
        let seen = (self.chess.games.scope_stamp().0, game.path_of(game.current()).unwrap_or_default());
        if self.chess.source.seen.as_ref() == Some(&seen) {
            return;
        }
        let source = current_source(board);
        self.chess.source.seen = Some(seen);
        if let Some(source) = source {
            self.reveal_chess_source(source);
        }
    }

    /// Show where a move was printed: scroll the page there unless it is on screen already, and
    /// outline it. Only in the document whose games the board shows. Whether it was shown.
    pub(crate) fn reveal_chess_source(&mut self, source: NodeSource) -> bool {
        let Some((vi, id)) = self.active_ids() else { return false };
        if self.chess.games.current_doc() != Some(id) {
            return false;
        }
        let rect = source.rect_user;
        let Ok(page) = usize::try_from(source.page) else { return false };
        if !rect.iter().all(|v| v.is_finite()) {
            return false;
        }
        let Some(doc) = self.session.get(id) else { return false };
        let Some(view) = self.views.get_mut(vi) else { return false };
        // `user_rect` reads the page's box: never for a page the document does not have.
        if page >= doc.info.pages.len() {
            return false;
        }
        let on_screen = view.page_xform(page).is_some_and(|xf| view.viewport_rect().contains_rect(xf.user_rect(&doc.info, page, rect)));
        if !on_screen {
            let top = rect[1].max(rect[3]) + ROOM_ABOVE;
            view.go_to_dest(page, DestView::Xyz { left: None, top: Some(top), zoom: None }, &doc.info);
        }
        view.flash = Some((page, rect, 0.0));
        self.chess.source.shown = Some(Shown { page, rect, scrolled: !on_screen });
        true
    }

    /// Put game `at.game` of `read` (pages of document `id` read as chess text) on the board, at
    /// move `at.node`, with the place of each move on its page, and show the board. A new game of
    /// the document's collection, one undo step of the list ("Open this game on the board"),
    /// unless the same one was opened before and is untouched since: that one is shown again (no
    /// step). Only while `id` is the document shown.
    pub(crate) fn put_book_game(&mut self, id: DocId, read: &ChessDoc, at: NodeRef) {
        if self.active_ids().map(|(_, active)| active) != Some(id) {
            return;
        }
        let Some(book) = read.board_game(at.game) else {
            self.notify(tl!("This game can't go on the board: its start position isn't legal.").to_string());
            return;
        };
        let source = match read.games.get(at.game).map(|g| g.origin) {
            Some(GameOrigin::Diagram { page, diagram, .. }) => usize::try_from(page).ok().map(|page| Source { doc: id, page, index: diagram }),
            _ => None,
        };
        let path = book.paths.iter().find(|(n, _)| *n == at.node).map(|(_, p)| p.clone());
        let dropped = book.dropped;
        self.sync_chess_docs();
        let mut board = BoardState::default();
        board.game = book.game;
        board.source = source;
        if let Some(node) = path.as_ref().and_then(|p| board.game.node_at(p)) {
            board.game.go_to_node(node);
        }
        board.refresh_fields(true);
        let (pgn, where_printed) = (board.game.pgn(), places(&board.game));
        let c = self.chess.games.collection_mut();
        let same = c.iter().position(|e| {
            e.untouched()
                && e.board.illegal.is_none()
                && e.board.source == source
                && e.board.game.pgn() == pgn
                && places(&e.board.game) == where_printed
        });
        match same {
            Some(i) => {
                c.switch_to(i);
                let game = &mut c.active_mut().board.game;
                if let Some(node) = path.as_ref().and_then(|p| game.node_at(p)) {
                    game.go_to_node(node);
                }
            }
            None => {
                c.add_games(vec![board], ChessEdit::OpenBookGame, true);
            }
        }
        if dropped > 0 {
            self.notify(crate::i18n::fmt(tl!("Moves left off the board (more than it holds): {n}"), &[("n", &dropped.to_string())]));
        }
        self.chess.study.setup = None;
        self.chess.enter_scope();
        self.show_chess_board();
    }
}
