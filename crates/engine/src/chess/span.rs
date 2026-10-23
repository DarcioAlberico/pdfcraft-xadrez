//! Chess ▸ Recognised text ▸ a game across page edges: a move of one page read alone
//! ([`Session::chess_document_with`] on that page, as the Text tab reads it), with the pages
//! around it read too, so that the game it belongs to is whole when a book runs it on from the
//! page before or onto the page after.
//!
//! The page read stays as it is: the pages around it are read alone the same way and the whole
//! run is walked again in order (`DocBuilder::add_read_page`), so the move keeps its place in
//! the page's blocks and the game walker sees the pages as the whole-document reader does (a
//! game goes on across a page break, and across a diagram that reprints its position).
//!
//! It is bounded: at most [`SPAN_PAGES`] pages each way. A page is read only while the game may
//! go on there:
//!
//! - **before**, while the game's first move in the run is the first move of a game on the
//!   run's first page and the game did not start at a `1.` (a game from the initial position
//!   cannot go on from an earlier page); a move that is in no game yet (moves at the top of a
//!   page that go on from the page before have no position on their own) reads back until it
//!   is in one;
//! - **after**, while the game has no result and its last move is the last move of a game on the
//!   run's last page.
//!
//! A page that cannot be read ends the run there, and so does a page that would take the move
//! out of the game it was in. Read-only.

use pdfcraft_chess::san::Lang;
use pdfcraft_chessvision::doc::{Block, ChessDoc, ChessPage, DocBuilder, GameOrigin, GameResult, Inline, NodeRef};

use super::ChessScanSettings;
use super::document::OcrOptions;
use crate::{DocId, Session};

/// Most pages read before, and after, the page of the move.
pub const SPAN_PAGES: usize = 4;

/// A move of a page: the page (0-based), its block and the inline in that block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MoveAt {
    pub page: usize,
    pub block: usize,
    pub inline: usize,
}

/// A move's game, read across the pages around it.
#[derive(Clone, Debug)]
pub struct GameSpan {
    /// The pages read, in order, walked as one document.
    pub doc: ChessDoc,
    /// The move's node in [`Self::doc`]; `None` when it is in no game even with the pages
    /// around it.
    pub at: Option<NodeRef>,
    /// The first and the last page read (0-based).
    pub pages: (usize, usize),
}

/// What [`Session::chess_game_span`] read.
#[derive(Clone, Debug)]
pub struct GameSpanRead {
    pub span: GameSpan,
    /// The book's cipher as the pages read taught it.
    pub cipher: pdfcraft_chessvision::cipher::BookCipher,
    /// What reading the other pages noted, page by page (0-based).
    pub notes: Vec<(usize, String)>,
}

/// The pages walked again as one document, and the node of the move at `at` in it.
fn join(before: &[ChessPage], centre: &ChessPage, after: &[ChessPage], at: MoveAt) -> (ChessDoc, Option<NodeRef>) {
    let mut builder = DocBuilder::new();
    for page in before.iter().rev().chain(std::iter::once(centre)).chain(after) {
        builder.add_read_page(page.clone());
    }
    let doc = builder.finish();
    let node = node_of(&doc, at);
    (doc, node)
}

/// The node of the move at `at`, when it is a move of a game.
fn node_of(doc: &ChessDoc, at: MoveAt) -> Option<NodeRef> {
    let page = doc.pages.iter().find(|p| usize::try_from(p.index).ok() == Some(at.page))?;
    match page.blocks.get(at.block)?.inlines().get(at.inline)? {
        Inline::Move(m) => m.game,
        _ => None,
    }
}

/// The games of page `page`'s moves, in reading order.
fn games_on(doc: &ChessDoc, page: usize) -> impl Iterator<Item = usize> + '_ {
    doc.pages
        .iter()
        .filter(move |p| usize::try_from(p.index).ok() == Some(page))
        .flat_map(|p| p.blocks.iter().flat_map(Block::inlines).filter_map(|i| if let Inline::Move(m) = i { m.game.map(|r| r.game) } else { None }))
}

/// Whether game `game` may have begun before page `first`: its first move is the page's first
/// move of a game, and it did not start at a `1.`.
fn may_go_back(doc: &ChessDoc, game: usize, first: usize) -> bool {
    let started_at_one = doc.games.get(game).is_none_or(|g| g.origin == GameOrigin::Initial);
    !started_at_one && games_on(doc, first).next() == Some(game)
}

/// Whether game `game` may go on after page `last`: it has no result and its last move is the
/// page's last move of a game.
fn may_go_on(doc: &ChessDoc, game: usize, last: usize) -> bool {
    let ended = doc.games.get(game).is_none_or(|g| g.score.result != GameResult::Unknown);
    !ended && games_on(doc, last).last() == Some(game)
}

/// The game of the move at `at` on page `centre` (read alone), read on across the pages around
/// it as the module says. `pages` is the document's page count; `read(p)` reads page `p` alone
/// (`None` when it cannot be read: the run ends there).
pub fn span_game(centre: ChessPage, at: MoveAt, pages: usize, mut read: impl FnMut(usize) -> Option<ChessPage>) -> GameSpan {
    let mut before: Vec<ChessPage> = Vec::new();
    let mut after: Vec<ChessPage> = Vec::new();
    // A direction stays closed once a page there could not be read or was not the game's.
    let (mut back_open, mut on_open) = (true, true);
    let (mut doc, mut node) = join(&before, &centre, &after, at);
    // Each turn reads a page or ends a direction, so this ends; the bound is only a guard.
    for _ in 0..SPAN_PAGES.saturating_mul(2).saturating_add(2) {
        let first = at.page.saturating_sub(before.len());
        let last = at.page.saturating_add(after.len());
        let game = node.map(|n| n.game);
        let back = back_open && first > 0 && before.len() < SPAN_PAGES && game.is_none_or(|g| may_go_back(&doc, g, first));
        let on = on_open && after.len() < SPAN_PAGES && last.saturating_add(1) < pages && game.is_some_and(|g| may_go_on(&doc, g, last));
        if !back && !on {
            break;
        }
        if back {
            let p = first.saturating_sub(1);
            match read(p).filter(|page| usize::try_from(page.index).ok() == Some(p)) {
                Some(page) => {
                    before.push(page);
                    let (d, n) = join(&before, &centre, &after, at);
                    // A page that takes the move out of its game is not the game's.
                    if node.is_some() && n.is_none() {
                        before.pop();
                        back_open = false;
                    } else {
                        (doc, node) = (d, n);
                    }
                }
                None => back_open = false,
            }
            continue;
        }
        let p = last.saturating_add(1);
        match read(p).filter(|page| usize::try_from(page.index).ok() == Some(p)) {
            Some(page) => {
                after.push(page);
                let (d, n) = join(&before, &centre, &after, at);
                if n.is_none() {
                    after.pop();
                    on_open = false;
                } else {
                    (doc, node) = (d, n);
                }
            }
            None => on_open = false,
        }
    }
    let first = at.page.saturating_sub(before.len());
    let last = at.page.saturating_add(after.len());
    GameSpan { doc, at: node, pages: (first, last) }
}

impl Session {
    /// The game of the move at `at`, on page `centre` as the Text tab read it (alone, with
    /// [`Self::chess_document_with`]), read on across the pages around it ([`span_game`]): each
    /// of them is read alone the same way, with `ocr`, in `centre`'s piece-letter language.
    pub fn chess_game_span(
        &mut self,
        id: DocId,
        centre: ChessPage,
        at: MoveAt,
        settings: &ChessScanSettings,
        ocr: &OcrOptions<'_>,
    ) -> Result<GameSpanRead, String> {
        let pages = self.get(id).ok_or("no such document")?.info.pages.len();
        if usize::try_from(centre.index).ok() != Some(at.page) {
            return Err("the move is not on the page read".into());
        }
        let lang = Lang::ALL.into_iter().find(|l| l.code() == centre.lang);
        let mut cipher = ocr.book.cloned().unwrap_or_default();
        let mut notes = Vec::new();
        let span = span_game(centre, at, pages, |p| {
            let options = OcrOptions { mode: ocr.mode, glyphs: ocr.glyphs, general: ocr.general, book: Some(&cipher) };
            match self.chess_document_with(id, &[p], settings, lang, &options) {
                Ok(read) => {
                    cipher = read.cipher;
                    notes.extend(read.notes);
                    read.doc.pages.into_iter().next()
                }
                Err(e) => {
                    notes.push((p, e));
                    None
                }
            }
        });
        Ok(GameSpanRead { span, cipher, notes })
    }
}
