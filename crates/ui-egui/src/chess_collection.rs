//! Chess ▸ Game collections (PGN toolbar spec §4.7, T4b): every document keeps its own chess
//! games, the board shows the open document's active one, and the games outlive the session.
//!
//! - **Per document.** [`ChessGames`] holds a [`ChessCollection`] for each open document, plus
//!   one for when no document is open. The board panels show the open document's active game
//!   ([`ChessState::board`]); switching documents switches games.
//! - **A diagram opened on the board is a new game** in its document's collection: it never
//!   replaces the game on the board (only a blank, untouched game gives way to it, and opening
//!   the same untouched diagram twice shows the game already there). Loading the PGN field still
//!   replaces the game shown, as one undo step; Open PGN… adds the file's games.
//! - **The list.** "Games in this document" in the board panels lists them by their players, their
//!   event, the diagram they came from, or their number; a click shows one (that is walking, not
//!   an undo step), and New game adds a blank one. Each game can be renamed (its White, Black and
//!   Event tags, so the name is in its PGN, with its Site, Date and Round; one undo step of that
//!   game, and a date PGN can't read is refused), duplicated, deleted (after a question; deleting
//!   the last game leaves a blank one) and moved up or down, from its menu, the buttons shown on
//!   the row under the pointer, or the keys of a focused row (F2, Delete, Alt+↑/↓; Enter shows
//!   it), or dragged to another place.
//! - **Several games at once.** Ctrl+click adds a game to the selection or takes it out, Shift+click
//!   selects the games from the last one clicked, and on a focused row Shift+↑/↓ selects on,
//!   Ctrl+A selects every game and Ctrl+Space picks one ([`Selection`]); the game shown stays
//!   shown. A selected row's menu, buttons and keys then act on every game selected, as one step
//!   of the list: Delete… (one question), Duplicate, Move up/down and dragging (as a block, in
//!   their order), and Save PGN… (the games selected, in list order). "New game from this position"
//!   starts a game at the move shown, or at a move of the notation, keeping the diagram it came
//!   from. The list's order is the order the collection is exported and autosaved in.
//! - **Undo of the list** (wave 10, U2). A delete, a move or an add (New game, Duplicate, New
//!   game from this position, Open PGN…, the PGN field's other games, a game opened from the
//!   trash) is a step of the list's own history ([`ListHistory`], this session only), which Undo
//!   takes in the chess scope (D12) when it is newer than the last change to the game shown;
//!   undoing a delete puts the game back where it was, with its own history, and shows it if it
//!   was shown. Undoing an add takes the games back out; one changed since it was added also
//!   goes into the trash, so no change is lost when the redo step goes.
//! - **The model below** ([`pdfcraft_engine::chess::collection`]): the order, the game shown after
//!   each change, the names and the PGN split are the engine's [`GameList`], which the automation
//!   tool `chess_games` uses too; this module keeps the boards, the undo and the autosave.
//! - **Kept across sessions** ([`crate::chess_autosave`]): two seconds after the last change, on
//!   exit and when a document closes, each collection is written under its document's keys
//!   ([`DocKeys`]), and found again by them when the document is opened. Saving the document can
//!   change its keys (a first `/ID`, a new size or path), so the collection takes the new ones
//!   right after every save ([`PdfCraftApp::rekey_chess_games`]). A protected document's games
//!   (encrypted, or copying not allowed) are written only once the user allows it, asked once
//!   per document and session.
//! - **PGN files.** Save PGN… writes the game shown, Export… (Save all games as PGN…) the whole
//!   collection in list order, to a file the user chooses (a download on the web); Open PGN… adds
//!   every game of a file.
//!
//! Each game's undo history ([`crate::chess_history`]) is its own, and is not kept across
//! sessions. Where each move was read on a page (`NodeSource`) lives only in the game's moves.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};

use egui::{Event, Key, Modifiers};
use pdfcraft_engine::DocId;
use pdfcraft_engine::chess::DocKeys;
pub use pdfcraft_engine::chess::collection::GameNames;
use pdfcraft_engine::chess::collection::split_pgn;
use pdfcraft_engine::chess::collection::{self, GameList, ListedGame};
use pdfcraft_engine::chess::pdfcraft_chess::game::{Game, NodeId, NodePath};

use crate::PdfCraftApp;
use crate::chess_autosave::{self, Blocked, Store};
use crate::chess_board_ui::{BoardState, Source};
use crate::chess_history::{self, ChessEdit};
use crate::chess_trash::Trashed;
use crate::chess_ui::ChessState;
use crate::theme::Tokens;

/// Seconds after the last change before a collection is written.
pub(crate) const DEBOUNCE_SECS: f64 = 2.0;

/// Largest PGN file read.
const MAX_PGN_FILE_BYTES: u64 = 32 << 20;

/// The id of the collection kept for when no document is open.
const NO_DOCUMENT: &str = "no-document";

/// A fresh game id, unique in the process: the scope of Undo (D12) follows one game.
fn next_uid() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// A fresh collection id, unique across sessions and instances: the time, the process and a
/// counter, in hex (a safe file name).
fn new_cid() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    #[cfg(not(target_arch = "wasm32"))]
    let (time, process) = (
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok().and_then(|d| u64::try_from(d.as_millis()).ok()).unwrap_or(0),
        std::process::id(),
    );
    // A finite, non-negative number of milliseconds; `as` saturates anything else to a bound.
    #[cfg(target_arch = "wasm32")]
    let (time, process) = (js_sys::Date::now() as u64, 0u32);
    format!("{time:x}-{process:x}-{n:x}")
}

/// One game of a collection: the board with its game, its undo history, the move shown
/// ([`Self::current_path`]) and the diagram it came from ([`Self::origin`]).
pub struct GameEntry {
    pub board: BoardState,
    uid: u64,
}

impl ListedGame for GameEntry {
    fn blank() -> GameEntry {
        GameEntry::new(BoardState::default())
    }

    fn is_blank(&self) -> bool {
        GameEntry::is_blank(self)
    }

    /// Only the game shown needs its pictures; the others drop theirs until shown again.
    fn hidden(&mut self) {
        self.board.release_textures();
    }
}

impl GameEntry {
    pub fn new(board: BoardState) -> GameEntry {
        GameEntry { board, uid: next_uid() }
    }

    /// The move shown, as a path from the start.
    pub fn current_path(&self) -> NodePath {
        self.board.game.path_of(self.board.game.current()).unwrap_or_default()
    }

    /// The diagram the game was opened from.
    pub fn origin(&self) -> Option<Source> {
        self.board.source
    }

    /// The game's name in the list, `number` being its place (1-based): "White–Black" from its
    /// details, else its event, else the diagram it came from, else "Game {number}".
    pub fn name(&self, number: usize) -> String {
        if let Some(name) = self.names().display() {
            return name;
        }
        match self.board.source {
            Some(s) => crate::i18n::fmt(
                tl!("Diagram {n}, p. {page}"),
                &[("n", &(s.index.saturating_add(1)).to_string()), ("page", &(s.page.saturating_add(1)).to_string())],
            ),
            None => crate::i18n::fmt(tl!("Game {n}"), &[("n", &number.to_string())]),
        }
    }

    /// What the game is named by: its White, Black and Event tags (empty when unknown, `?`
    /// included).
    pub fn names(&self) -> GameNames {
        GameNames::of(&self.board.game)
    }

    /// Name the game: its White, Black, Event, Site, Date and Round tags take `names` (an empty
    /// name removes its tag), as one undo step of the game ("Edit game details"). Whether anything
    /// changed. The list's editor lets no date through that PGN can't read
    /// ([`collection::pgn_date_ok`]).
    fn rename(&mut self, names: &GameNames) -> bool {
        let changes = names.changes_from(&self.names());
        if changes.is_empty() {
            return false;
        }
        self.board.edit(|s| {
            for (tag, value) in &changes {
                // The six names are valid tag names; a game whose tags are full refuses a new
                // one, and the others still change.
                let _ = s.game.set_tag(tag, value);
            }
            s.refresh_fields(true);
            Some(ChessEdit::GameDetails)
        })
    }

    /// A copy, as a game of its own: the same moves, move shown, illegal position, side and
    /// diagram; a history of its own, empty.
    fn copy(&self) -> GameEntry {
        let mut board = BoardState::default();
        board.game = self.board.game.clone();
        board.illegal = self.board.illegal.clone();
        board.source = self.board.source;
        board.flipped = self.board.flipped;
        board.refresh_fields(true);
        GameEntry::new(board)
    }

    /// Moves of the main line, counted as a scoresheet does (a move each side).
    pub fn moves(&self) -> usize {
        collection::scoresheet_moves(&self.board.game)
    }

    /// Never touched: the standard start, nothing played, no diagram, no history.
    fn is_blank(&self) -> bool {
        self.board.illegal.is_none()
            && self.board.source.is_none()
            && self.untouched()
            && self.board.game.is_empty()
            && self.board.game.pgn() == Game::standard().pgn()
    }

    /// Nothing done on it since it was opened (no undo or redo step).
    pub(crate) fn untouched(&self) -> bool {
        self.board.history.undo_edit().is_none() && self.board.history.redo_edit().is_none()
    }
}

/// When a collection was last changed and written.
#[derive(Default)]
struct Autosave {
    /// The fingerprint last seen ([`ChessCollection::fingerprint`]).
    seen: u64,
    /// The fingerprint last written or read.
    saved: u64,
    /// When the last change was seen (egui's clock).
    changed_at: Option<f64>,
    /// Written now, whatever the clock (after a re-key).
    force: bool,
    /// The store has it (read from it or written to it).
    stored: bool,
    /// Why the last write did not happen.
    blocked: Option<Blocked>,
    /// The document is encrypted or does not allow copying: ask before writing.
    protected: bool,
}

/// A change to the list of games that Undo takes back (wave 10, U2): a game deleted, moved or
/// added. Games are named by their id, so a step still finds its game after other changes to the
/// list.
enum ListChange {
    /// Games were deleted, one or several at once (a selection).
    Deleted(Box<Deleted>),
    /// The games a delete took (games `uids`) are back: redoing deletes them again.
    Restored { uids: Vec<u64> },
    /// Game `uid` was moved from place `from` to place `to`.
    Moved { uid: u64, from: usize, to: usize },
    /// Several games were moved at once (a selection, as a block): the order of the whole list
    /// before and after, by game id.
    Arranged { before: Vec<u64>, after: Vec<u64> },
    /// Games `uids` were added (New game, Duplicate, New game from this position, Open PGN…, the
    /// PGN field's games after its first, a game of the trash opened, a diagram or a game of the
    /// book opened on the board), `edit` saying how, while
    /// game `before` was shown; `restored`: the trash item a game was opened from, and its place
    /// in the trash.
    Added { uids: Vec<u64>, edit: ChessEdit, before: u64, restored: Option<(usize, Trashed)> },
    /// Added games Undo took back out: redoing puts them back.
    Removed(Box<Removed>),
}

/// Games a delete took ([`ListChange::Deleted`]).
struct Deleted {
    /// Each with its place before the delete, in list order.
    games: Vec<(usize, GameEntry)>,
    /// The game shown, when it was one of them.
    shown: Option<u64>,
    /// They were all the list had, and a blank game took their place.
    left_blank: bool,
}

/// Added games Undo took back out ([`ListChange::Removed`]).
struct Removed {
    /// Each with its place, in list order.
    games: Vec<(usize, GameEntry)>,
    /// How they were added.
    edit: ChessEdit,
    /// The game shown before they were added.
    before: u64,
    /// The game shown when Undo took them out.
    shown: u64,
    /// They were all the list had, and a blank game took their place.
    left_blank: bool,
    /// The trash item a game was opened from, and its place in the trash.
    restored: Option<(usize, Trashed)>,
    /// What the ones changed since they were added left in the trash.
    kept: Vec<Trashed>,
}

impl ListChange {
    /// What Undo and Redo call it.
    fn edit(&self) -> ChessEdit {
        match self {
            ListChange::Deleted(_) | ListChange::Restored { .. } => ChessEdit::DeleteGame,
            ListChange::Moved { .. } | ListChange::Arranged { .. } => ChessEdit::MoveGame,
            ListChange::Added { edit, .. } => edit.clone(),
            ListChange::Removed(r) => r.edit.clone(),
        }
    }

    /// The memory it keeps, as the caps count it (games taken out, each with its own history,
    /// and the trash items it holds).
    fn bytes(&self) -> usize {
        let item = |t: &Trashed| t.pgn.len().saturating_add(t.name.len());
        match self {
            ListChange::Deleted(d) => d.games.iter().map(|(_, e)| e.board.bytes()).fold(0usize, usize::saturating_add),
            ListChange::Restored { uids } => uids.len().saturating_mul(std::mem::size_of::<u64>()),
            ListChange::Arranged { before, after } => before.len().saturating_add(after.len()).saturating_mul(std::mem::size_of::<u64>()),
            ListChange::Moved { .. } => 0,
            ListChange::Added { restored, .. } => restored.as_ref().map_or(0, |(_, t)| item(t)),
            ListChange::Removed(r) => r
                .games
                .iter()
                .map(|(_, e)| e.board.bytes())
                .chain(r.kept.iter().map(item))
                .chain(r.restored.iter().map(|(_, t)| item(t)))
                .fold(0usize, usize::saturating_add),
        }
    }
}

/// One step of [`ListHistory`]: the change, when it was put on its side (the order Undo goes in
/// across the list and the game shown, [`chess_history::next_seq`]) and its size.
struct ListStep {
    change: ListChange,
    seq: u64,
    bytes: usize,
}

impl ListStep {
    fn new(change: ListChange) -> ListStep {
        let bytes = change.bytes();
        ListStep { change, seq: chess_history::next_seq(), bytes }
    }
}

/// The list's own history: deletes, moves and adds to undo (oldest first) and to redo (next last),
/// capped as a game's history is ([`chess_history::MAX_STEPS`] steps and
/// [`chess_history::MAX_BYTES`]). It lives with the list, for this session only.
#[derive(Default)]
struct ListHistory {
    past: Vec<ListStep>,
    future: Vec<ListStep>,
}

impl ListHistory {
    /// Keep `change`, just made: whatever could be redone goes.
    fn record(&mut self, change: ListChange) {
        self.future.clear();
        self.past.push(ListStep::new(change));
        self.fit();
    }

    /// Drop the oldest steps until both caps hold (redo steps only when no undo step is left).
    fn fit(&mut self) {
        let total = |h: &ListHistory| h.past.iter().chain(h.future.iter()).map(|s| s.bytes).fold(0usize, usize::saturating_add);
        while self.past.len().saturating_add(self.future.len()) > chess_history::MAX_STEPS || total(self) > chess_history::MAX_BYTES {
            if !self.past.is_empty() {
                self.past.remove(0);
            } else if !self.future.is_empty() {
                self.future.remove(0);
            } else {
                break;
            }
        }
    }

    /// When the step Undo (`undo`) or Redo would take was put there.
    fn seq(&self, undo: bool) -> Option<u64> {
        if undo { self.past.last() } else { self.future.last() }.map(|s| s.seq)
    }

    /// What Undo (`undo`) or Redo would take back.
    fn edit(&self, undo: bool) -> Option<ChessEdit> {
        if undo { self.past.last() } else { self.future.last() }.map(|s| s.change.edit())
    }
}

/// A document's games, the active one shown on the board. Never empty ([`GameList`]).
pub struct ChessCollection {
    /// What names the document's file (see [`DocKeys`]); empty until it is known.
    pub keys: DocKeys,
    games: GameList<GameEntry>,
    /// Deletes, moves and adds to undo.
    history: ListHistory,
    /// The collection's id in the store.
    cid: String,
    autosave: Autosave,
    /// The games as they were before moves left them, kept with the games (plan A8 §7, V5).
    trash: crate::chess_trash::Trash,
}

impl Default for ChessCollection {
    /// The collection kept for when no document is open.
    fn default() -> ChessCollection {
        ChessCollection::new(DocKeys::default(), NO_DOCUMENT.to_owned())
    }
}

impl ChessCollection {
    /// One blank game.
    fn new(keys: DocKeys, cid: String) -> ChessCollection {
        let mut c = ChessCollection {
            keys,
            games: GameList::new(),
            history: ListHistory::default(),
            cid,
            autosave: Autosave::default(),
            trash: crate::chess_trash::Trash::default(),
        };
        c.settle();
        c
    }

    /// The games `boards`, `active` shown (the first when out of range); one blank game when
    /// there are none.
    fn from_boards(keys: DocKeys, cid: String, boards: Vec<BoardState>, active: usize) -> ChessCollection {
        let mut c = ChessCollection::new(keys, cid);
        c.games = GameList::from_games(boards.into_iter().map(GameEntry::new).collect(), active);
        c.settle();
        c
    }

    /// The current state counts as written.
    fn settle(&mut self) {
        let fp = self.fingerprint();
        self.autosave.seen = fp;
        self.autosave.saved = fp;
        self.autosave.changed_at = None;
    }

    /// How many games.
    pub fn len(&self) -> usize {
        self.games.len()
    }

    /// Never empty (the active game is always there); here for completeness.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// The active game's place in the list (0-based).
    pub fn active_index(&self) -> usize {
        self.games.active_index()
    }

    /// The games, in list order.
    pub fn iter(&self) -> impl Iterator<Item = &GameEntry> {
        self.games.iter()
    }

    /// The game shown.
    pub fn active(&self) -> &GameEntry {
        self.games.active()
    }

    pub fn active_mut(&mut self) -> &mut GameEntry {
        self.games.active_mut()
    }

    /// Show game `index` (0-based). Whether there is one.
    pub fn switch_to(&mut self, index: usize) -> bool {
        self.games.switch_to(index)
    }

    /// Game `index` (0-based).
    pub fn get(&self, index: usize) -> Option<&GameEntry> {
        self.games.get(index)
    }

    fn get_mut(&mut self, index: usize) -> Option<&mut GameEntry> {
        self.games.get_mut(index)
    }

    /// The place of the game whose id is `uid`.
    fn position(&self, uid: u64) -> Option<usize> {
        self.games.position(|e| e.uid == uid)
    }

    /// Move game `index` one place up (towards the start) or down; the game shown stays shown.
    /// Whether it moved (one undo step of the list).
    pub(crate) fn move_game(&mut self, index: usize, up: bool) -> bool {
        let other = if up { index.checked_sub(1) } else { index.checked_add(1) };
        other.is_some_and(|to| self.move_to(index, to))
    }

    /// Move game `from` to place `to` (its place once moved); the game shown stays shown.
    /// Whether it moved (one undo step of the list).
    pub(crate) fn move_to(&mut self, from: usize, to: usize) -> bool {
        let Some(uid) = self.get(from).map(|e| e.uid) else { return false };
        if !self.games.move_to(from, to) {
            return false;
        }
        self.history.record(ListChange::Moved { uid, from, to });
        true
    }

    /// Put `entry` right after game `index` and show it.
    fn insert_after(&mut self, index: usize, entry: GameEntry) {
        self.games.insert_after(index, entry);
    }

    /// A copy of game `index`, right after it, shown. Whether there is such a game.
    pub(crate) fn duplicate(&mut self, index: usize) -> bool {
        let Some(copy) = self.get(index).map(GameEntry::copy) else { return false };
        let (before, uid) = (self.active().uid, copy.uid);
        self.insert_after(index, copy);
        self.added(vec![uid], ChessEdit::DuplicateGame, before, None);
        true
    }

    /// A blank game at the end, shown (one undo step of the list). A blank game alone in the
    /// list stays as it is: there is nothing to add.
    pub(crate) fn new_game(&mut self) {
        if self.len() == 1 && self.active().is_blank() {
            return;
        }
        let (before, entry) = (self.active().uid, GameEntry::new(BoardState::default()));
        let uid = entry.uid;
        self.games.add(entry);
        self.added(vec![uid], ChessEdit::NewGame, before, None);
    }

    /// Add `boards` at the end, the first of them shown (`show`; else the game shown stays
    /// shown), as one undo step of the list named `edit`; a blank game alone in the list gives
    /// way to them. How many were added.
    pub(crate) fn add_games(&mut self, boards: Vec<BoardState>, edit: ChessEdit, show: bool) -> usize {
        if boards.is_empty() {
            return 0;
        }
        let before = self.active().uid;
        let first = if self.len() == 1 && self.active().is_blank() { 0 } else { self.len() };
        let mut uids = Vec::with_capacity(boards.len());
        for board in boards {
            let entry = GameEntry::new(board);
            uids.push(entry.uid);
            self.games.add(entry);
        }
        let shown = if show { Some(first) } else { self.position(before) };
        if let Some(at) = shown {
            self.games.switch_to(at);
        }
        let added = uids.len();
        self.added(uids, edit, before, None);
        added
    }

    /// Games `uids` were just added while game `before` was shown: one undo step of the list.
    fn added(&mut self, uids: Vec<u64>, edit: ChessEdit, before: u64, restored: Option<(usize, Trashed)>) {
        self.history.record(ListChange::Added { uids, edit, before, restored });
    }

    /// Delete game `index`. The game shown stays shown; when it is the one deleted, the game
    /// after it is shown (the one before, at the end of the list), and deleting the last game
    /// leaves a blank one. One undo step of the list, which puts the game back with its own
    /// history. Whether there was such a game.
    pub(crate) fn delete(&mut self, index: usize) -> bool {
        self.delete_many(&[index]) > 0
    }

    /// Delete games `indices` (a selection; places that are no game are left out) as one undo step
    /// of the list, which puts them all back where they were. The game shown stays shown; when it
    /// is one of them, the game after it that stays is shown (the one before, at the end of the
    /// list), and deleting every game leaves a blank one. How many were deleted.
    pub(crate) fn delete_many(&mut self, indices: &[usize]) -> usize {
        let Some(deleted) = self.take_places(indices) else { return 0 };
        let count = deleted.games.len();
        self.history.record(ListChange::Deleted(Box::new(deleted)));
        count
    }

    /// Take games `indices` out of the list (the last first, so each keeps the place it had among
    /// the others): what a delete keeps to undo it. `None` when none is a game of the list.
    fn take_places(&mut self, indices: &[usize]) -> Option<Deleted> {
        let places = self.places(indices);
        if places.is_empty() {
            return None;
        }
        let shown = places.contains(&self.active_index()).then(|| self.active().uid);
        let left_blank = places.len() == self.len();
        let mut games = Vec::with_capacity(places.len());
        for &index in places.iter().rev() {
            if let Some(entry) = self.games.delete(index) {
                games.push((index, entry));
            }
        }
        games.reverse();
        Some(Deleted { games, shown, left_blank })
    }

    /// Undo of a delete: the games go back to their places (a blank game the delete left gives
    /// way to the first), and the game shown then is shown again.
    fn put_places(&mut self, deleted: Deleted) -> ListChange {
        let Deleted { games, shown, left_blank } = deleted;
        let uids = games.iter().map(|(_, e)| e.uid).collect();
        for (k, (index, entry)) in games.into_iter().enumerate() {
            if k == 0 && left_blank && self.len() == 1 && self.active().is_blank() {
                *self.games.active_mut() = entry;
            } else {
                self.games.insert(index, entry, false);
            }
        }
        if let Some(at) = shown.and_then(|uid| self.position(uid)) {
            self.games.switch_to(at);
        }
        ListChange::Restored { uids }
    }

    /// `indices` that are places of games, in order, each once.
    fn places(&self, indices: &[usize]) -> Vec<usize> {
        let mut places: Vec<usize> = indices.iter().copied().filter(|&i| i < self.len()).collect();
        places.sort_unstable();
        places.dedup();
        places
    }

    /// The games' ids, in list order.
    fn order(&self) -> Vec<u64> {
        self.iter().map(|e| e.uid).collect()
    }

    /// Put the games in `order` (by id; ids no longer in the list are passed over, and games it
    /// does not name keep their order after the ones it does). The game shown stays shown.
    fn arrange(&mut self, order: &[u64]) {
        let mut k = 0usize;
        for &uid in order {
            let Some(at) = self.position(uid) else { continue };
            if at != k {
                self.games.move_to(at, k);
            }
            k = k.saturating_add(1);
        }
    }

    /// Put the games in `after` (by id, every game once), as one undo step of the list. Whether
    /// the order changed.
    fn rearranged(&mut self, after: Vec<u64>) -> bool {
        let before = self.order();
        if after == before || after.len() != before.len() {
            return false;
        }
        self.arrange(&after);
        self.history.record(ListChange::Arranged { before, after });
        true
    }

    /// Move games `indices` (a selection) one place up (towards the start) or down, as a block:
    /// each keeps its order among them, and each game between two of them stays between them.
    /// Refused when the first of them is first (going up) or the last is last (going down). One
    /// undo step of the list; whether they moved.
    pub(crate) fn move_many(&mut self, indices: &[usize], up: bool) -> bool {
        let places = self.places(indices);
        let mut order = self.order();
        match (places.first().copied(), places.last().copied()) {
            (Some(first), _) if up && first > 0 => {
                for &i in &places {
                    order.swap(i.saturating_sub(1), i);
                }
            }
            (_, Some(last)) if !up && last.saturating_add(1) < order.len() => {
                for &i in places.iter().rev() {
                    order.swap(i, i.saturating_add(1));
                }
            }
            _ => return false,
        }
        self.rearranged(order)
    }

    /// Move games `indices` (a selection) into gap `gap` of the list as it is (0: before the
    /// first game, `len`: after the last), together and in their order: a block dragged and
    /// dropped. One undo step of the list; whether the order changed.
    pub(crate) fn move_many_to(&mut self, indices: &[usize], gap: usize) -> bool {
        let order = block_order(&self.order(), &self.places(indices), gap);
        self.rearranged(order)
    }

    /// A copy of each of games `indices` (a selection) right after it, the first copy shown: one
    /// undo step of the list. How many copies were made.
    pub(crate) fn duplicate_many(&mut self, indices: &[usize]) -> usize {
        let places = self.places(indices);
        let before = self.active().uid;
        let mut uids = Vec::with_capacity(places.len());
        // The last first, so the places of the others stay as they are.
        for &i in places.iter().rev() {
            let Some(copy) = self.get(i).map(GameEntry::copy) else { continue };
            uids.push(copy.uid);
            self.insert_after(i, copy);
        }
        uids.reverse();
        if let Some(at) = uids.first().and_then(|&uid| self.position(uid)) {
            self.games.switch_to(at);
        }
        let count = uids.len();
        if count > 0 {
            self.added(uids, ChessEdit::DuplicateGame, before, None);
        }
        count
    }

    /// A new game starting at the position of move `node` of the game shown (the move shown when
    /// `None`, or when the game has no such move), on the same side and from the same diagram,
    /// right after it and shown. Its PGN gets the position as its `SetUp`/`FEN` tags. Refused for
    /// an illegal position, which can't start a game; the error says why.
    pub(crate) fn new_from(&mut self, node: Option<NodeId>) -> Result<(), String> {
        let b = &self.active().board;
        if b.illegal.is_some() {
            return Err(tl!("An illegal position can't start a game: correct it in the diagram editor first.").to_string());
        }
        let game =
            collection::game_from(&b.game, node).map_err(|e| crate::i18n::fmt(tl!("The FEN couldn't be read: {e}"), &[("e", &e.to_string())]))?;
        let source = b.source;
        self.insert_new(game, source, None);
        Ok(())
    }

    /// A new game from position `fen` (one set up on the board), on the side of the game shown,
    /// right after it and shown; the game shown keeps its moves (plan A8 §7, V2a). Refused for a
    /// FEN that can't start a game; the error says why.
    pub(crate) fn new_at(&mut self, fen: &str) -> Result<(), String> {
        let game = Game::from_fen(fen).map_err(|e| crate::i18n::fmt(tl!("The FEN couldn't be read: {e}"), &[("e", &e.to_string())]))?;
        self.insert_new(game, None, None);
        Ok(())
    }

    /// The deleted moves kept.
    pub fn trash(&self) -> &crate::chess_trash::Trash {
        &self.trash
    }

    /// `deleted` moves just left the game shown: the game as it was (the state its last undo
    /// step goes back to) goes into the trash.
    pub(crate) fn trash_deleted(&mut self, deleted: usize) {
        let number = self.active_index().saturating_add(1);
        let entry = self.active();
        let name = entry.name(number);
        if let Some(before) = entry.board.history.undo_state() {
            let game = before.game.clone();
            self.trash.keep(&game, &name, deleted);
        }
    }

    /// Open game `index` of the trash as a new game, right after the one shown, and take it out
    /// of the trash: one undo step of the list, which puts it back in the trash. Refused when it
    /// can't be read back (it stays in the trash then).
    pub(crate) fn restore_trashed(&mut self, index: usize) -> Result<(), String> {
        let item = self.trash.get(index).cloned().ok_or_else(|| tl!("Those deleted moves are no longer kept.").to_string())?;
        let (game, _) = Game::from_pgn(&item.pgn).map_err(|e| crate::i18n::fmt(tl!("The PGN couldn't be read: {e}"), &[("e", &e.to_string())]))?;
        self.trash.take(index);
        self.insert_new(game, None, Some((index, item)));
        Ok(())
    }

    /// `game` as a new game right after the one shown, on its side, and shown: one undo step of
    /// the list ("New game from this position", or "Open as a new game" when it was `restored`
    /// from the trash, the item and its place there).
    fn insert_new(&mut self, game: Game, source: Option<Source>, restored: Option<(usize, Trashed)>) {
        let mut board = BoardState::default();
        board.game = game;
        board.source = source;
        board.flipped = self.active().board.flipped;
        board.refresh_fields(true);
        let (before, entry) = (self.active().uid, GameEntry::new(board));
        let uid = entry.uid;
        self.insert_after(self.active_index(), entry);
        let edit = if restored.is_some() { ChessEdit::RestoreGame } else { ChessEdit::NewGameFromHere };
        self.added(vec![uid], edit, before, restored);
    }

    /// Add `entry` at the end and show it, as no step of the list (the tests build lists so). A
    /// blank game alone in the collection gives way to it.
    #[cfg(test)]
    pub(crate) fn add(&mut self, entry: GameEntry) {
        self.games.add(entry);
    }

    /// Undo (`undo`) or redo the list's last change. What it was, when there was one whose game
    /// is still in the list, and how many games it put into the trash (added games taken back
    /// out after they were changed).
    fn list_step(&mut self, undo: bool) -> Option<(ChessEdit, usize)> {
        let step = if undo { self.history.past.pop() } else { self.history.future.pop() }?;
        let edit = step.change.edit();
        let mut trashed = 0;
        let back = match step.change {
            ListChange::Deleted(deleted) => self.put_places(*deleted),
            ListChange::Restored { uids } => {
                let places: Vec<usize> = uids.iter().filter_map(|&uid| self.position(uid)).collect();
                ListChange::Deleted(Box::new(self.take_places(&places)?))
            }
            ListChange::Moved { uid, from, to } => {
                let at = self.position(uid)?;
                self.games.move_to(at, if undo { from } else { to });
                ListChange::Moved { uid, from, to }
            }
            ListChange::Arranged { before, after } => {
                if !before.iter().any(|&uid| self.position(uid).is_some()) {
                    return None;
                }
                self.arrange(if undo { &before } else { &after });
                ListChange::Arranged { before, after }
            }
            ListChange::Added { uids, edit, before, restored } => {
                let removed = self.take_out(&uids, edit, before, restored)?;
                trashed = removed.kept.len();
                ListChange::Removed(Box::new(removed))
            }
            ListChange::Removed(removed) => self.put_back(*removed),
        };
        let step = ListStep::new(back);
        if undo {
            self.history.future.push(step);
        } else {
            self.history.past.push(step);
        }
        self.history.fit();
        Some((edit, trashed))
    }

    /// Undo of an add: take games `uids` back out of the list and show game `before` again; a game
    /// opened from the trash (`restored`) goes back there. A game changed since it was added (it
    /// has undo or redo steps) also goes into the trash, so its changes outlive the redo step,
    /// which the next change to the list drops. `None` when none of the games is in the list any
    /// more.
    fn take_out(&mut self, uids: &[u64], edit: ChessEdit, before: u64, restored: Option<(usize, Trashed)>) -> Option<Removed> {
        let mut places: Vec<usize> = uids.iter().filter_map(|&uid| self.position(uid)).collect();
        if places.is_empty() {
            return None;
        }
        places.sort_unstable();
        let shown = self.active().uid;
        let (mut games, mut kept, mut left_blank) = (Vec::with_capacity(places.len()), Vec::new(), false);
        // The last place first, so each game keeps the place it had among the others.
        for &index in places.iter().rev() {
            left_blank = self.len() == 1;
            let name = self.get(index).map(|e| e.name(index.saturating_add(1))).unwrap_or_default();
            let Some(entry) = self.games.delete(index) else { continue };
            // An illegal position has no game to keep: it stays on the redo side only.
            if !entry.untouched()
                && entry.board.illegal.is_none()
                && let Some(item) = self.trash.keep_item(&entry.board.game, &name, None)
            {
                kept.push(item);
            }
            games.push((index, entry));
        }
        games.reverse();
        if let Some((at, item)) = &restored {
            self.trash.put_back(*at, item.clone());
        }
        if let Some(at) = self.position(before) {
            self.games.switch_to(at);
        }
        Some(Removed { games, edit, before, shown, left_blank, restored, kept })
    }

    /// Redo of an add Undo took back: the games go back to their places, the game shown then is
    /// shown again, and what the undo put in the trash comes out of it.
    fn put_back(&mut self, removed: Removed) -> ListChange {
        let Removed { games, edit, before, shown, left_blank, restored, kept } = removed;
        let uids = games.iter().map(|(_, e)| e.uid).collect();
        for (k, (index, entry)) in games.into_iter().enumerate() {
            if k == 0 && left_blank && self.len() == 1 && self.active().is_blank() {
                // The blank game the undo left gives way to them again.
                *self.games.active_mut() = entry;
            } else {
                self.games.insert(index, entry, false);
            }
        }
        if let Some(at) = self.position(shown) {
            self.games.switch_to(at);
        }
        for item in &kept {
            self.trash.take_item(item);
        }
        let restored = restored.map(|(at, item)| (self.trash.take_item(&item).unwrap_or(at), item));
        ListChange::Added { uids, edit, before, restored }
    }

    /// What the store keeps of the collection, summed up: the games, their changes, the one shown
    /// and its move. Changes when anything to keep changes.
    fn fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.active_index().hash(&mut h);
        self.active().board.game.current().hash(&mut h);
        self.trash.stamp().hash(&mut h);
        for e in self.iter() {
            e.uid.hash(&mut h);
            e.board.history.edits().hash(&mut h);
            e.board.game.revision().hash(&mut h);
            e.board.flipped.hash(&mut h);
            e.board.illegal.as_ref().map(|(_, f)| f.as_str()).hash(&mut h);
        }
        h.finish()
    }

    /// Whether the store has the collection as it is now.
    fn in_store(&self) -> bool {
        self.autosave.stored && self.autosave.saved == self.fingerprint()
    }

    /// Anything worth keeping: more than one game, or a game that isn't blank.
    fn has_content(&self) -> bool {
        self.len() > 1 || !self.active().is_blank()
    }
}

/// Every document's games.
#[derive(Default)]
pub struct ChessGames {
    /// The open documents' collections.
    docs: HashMap<DocId, ChessCollection>,
    /// The games played while no document is open.
    none: ChessCollection,
    /// Collections of documents closed this session that could not be written yet (not allowed,
    /// or blocked): kept, and taken back if the document is opened again.
    closed: Vec<ChessCollection>,
    /// The document shown.
    current: Option<DocId>,
    /// Where collections are written.
    store: Store,
    /// The user's answer, by collection, to "Save chess games for this protected document?".
    consent: HashMap<String, bool>,
    /// Notices already given (said once).
    said: HashSet<String>,
    /// Notices for the user, shown at the next frame.
    notes: Vec<String>,
    /// The list's question or name editor open, and its keyboard focus.
    list: ListState,
    /// PGN files read by the browser, imported at the next frame.
    #[cfg(target_arch = "wasm32")]
    uploads: std::sync::Arc<std::sync::Mutex<Vec<(String, Vec<u8>)>>>,
}

impl ChessGames {
    /// The collection shown: the open document's, or the one for no document.
    pub fn collection(&self) -> &ChessCollection {
        self.current.and_then(|id| self.docs.get(&id)).unwrap_or(&self.none)
    }

    /// [`Self::collection`] to change. A document whose collection isn't made yet (it is made
    /// at the next frame, from what is stored for it) gets a blank one, which a stored one may
    /// still replace while it stays blank.
    pub fn collection_mut(&mut self) -> &mut ChessCollection {
        match self.current {
            Some(id) => self.docs.entry(id).or_insert_with(|| ChessCollection::new(DocKeys::default(), new_cid())),
            None => &mut self.none,
        }
    }

    /// The notices waiting for the next frame, taken (to say them now).
    pub(crate) fn take_notes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notes)
    }

    /// The board shown: the active game of the document shown.
    pub fn board(&self) -> &BoardState {
        &self.collection().active().board
    }

    pub fn board_mut(&mut self) -> &mut BoardState {
        &mut self.collection_mut().active_mut().board
    }

    /// The game shown (its id) and how many changes it had: Undo's scope follows one game.
    pub(crate) fn scope_stamp(&self) -> (u64, u64) {
        let active = self.collection().active();
        (active.uid, active.board.history.edits())
    }

    /// Whether Undo (`undo`) or Redo takes the step of the list of games rather than the game
    /// shown's: the list has one, and it is the newer (both are in the chess scope, D12).
    fn list_turn(&self, undo: bool) -> bool {
        let c = self.collection();
        match (c.history.seq(undo), c.active().board.history.seq(undo)) {
            (Some(list), Some(game)) => list > game,
            (Some(_), None) => true,
            (None, _) => false,
        }
    }

    /// What Undo (`undo`) or Redo would take back in the chess scope: a change to the list
    /// of games, or a change to the game shown, whichever is the newer.
    pub(crate) fn step_edit(&self, undo: bool) -> Option<ChessEdit> {
        if self.list_turn(undo) {
            return self.collection().history.edit(undo);
        }
        let h = &self.board().history;
        if undo { h.undo_edit() } else { h.redo_edit() }.cloned()
    }

    /// Whether Undo (`undo`) or Redo has a step to take in the chess scope.
    pub(crate) fn can_step(&self, undo: bool) -> bool {
        self.step_edit(undo).is_some()
    }

    /// "Undo Delete game" or "Redo Play Nd5": what Undo (`undo`) or Redo would do in the chess
    /// scope, in the current language.
    pub(crate) fn step_text(&self, undo: bool) -> Option<String> {
        self.step_edit(undo).map(|e| chess_history::command_text(undo, &e))
    }

    /// Undo (`undo`) or redo in the chess scope: the list's last change, or the game
    /// shown's last change, whichever is the newer. The step's name when one was taken.
    pub(crate) fn history_step(&mut self, undo: bool) -> Option<String> {
        if self.list_turn(undo) {
            let (edit, trashed) = self.collection_mut().list_step(undo)?;
            if trashed > 0 {
                let count = u64::try_from(trashed).unwrap_or(u64::MAX);
                self.notes.push(crate::i18n::trn(
                    crate::i18n::current(),
                    count,
                    "{n} changed game was kept in Deleted moves.",
                    "{n} changed games were kept in Deleted moves.",
                ));
            }
            return Some(edit.text());
        }
        let board = self.board_mut();
        let label = if undo { board.undo_label() } else { board.redo_label() };
        let done = if undo { board.undo() } else { board.redo() };
        label.filter(|_| done)
    }

    /// The document shown, or none.
    pub fn current_doc(&self) -> Option<DocId> {
        self.current
    }

    /// Open the name editor of the game shown in the list (Chess ▸ Rename game…), or ask whether
    /// to delete it (Delete game…). The keyboard goes to the editor's first field, or to the
    /// question's Delete button.
    pub(crate) fn ask_about_shown(&mut self, rename: bool) {
        let shown = self.collection().active();
        let (uid, names) = (shown.uid, shown.names());
        self.list.ask(uid, names, rename, true, false);
    }

    /// Whether a row of the games list, or a button of the list, has the keyboard focus (as the
    /// last pass left it): the list's keys are its own then, and the chess key scope takes none.
    pub(crate) fn list_has_focus(&self, ctx: &egui::Context) -> bool {
        self.list.focused.is_some_and(|id| ctx.memory(|m| m.has_focus(id)))
    }

    /// Take the keys of a focused row of the list out of `events` (Delete, Backspace, F2, Alt+↑/↓,
    /// Shift+↑/↓, Ctrl+A, Ctrl+Space: [`list_key`]), so nothing else sees them: Delete there
    /// deletes no variation, page or annotation, and Ctrl+A selects no page text. Only presses are
    /// taken, as for the chess key scope.
    pub(crate) fn take_list_keys(&mut self, events: &mut Vec<Event>) {
        let keys = &mut self.list.keys;
        events.retain(|event| match event {
            Event::Key { key, pressed: true, modifiers, .. } if list_key(*key, *modifiers).is_some() => {
                if keys.len() < MAX_LIST_KEYS
                    && let Some(m) = list_key(*key, *modifiers)
                {
                    keys.push((*key, m));
                }
                false
            }
            _ => true,
        });
    }

    /// Forget the list's keys a previous frame took and no row used.
    pub(crate) fn drop_list_keys(&mut self) {
        self.list.keys.clear();
    }

    /// Whether games are autosaved at all (a store is set).
    pub fn autosave_on(&self) -> bool {
        self.store.is_on()
    }

    /// Autosave into the `chess` folder of the recovery folder, under its instance lock, and take
    /// back the games kept for no document.
    pub(crate) fn autosave_to_files(&mut self, recovery: &crate::RecoveryStore) {
        self.store = Store::Files(chess_autosave::FileStore::new(recovery.dir().join("chess"), recovery.clone()));
        self.take_back_no_document();
    }

    /// Autosave into the web's `Storage` value `text` (what it holds now).
    pub(crate) fn autosave_to_storage(&mut self, text: Option<&str>) {
        let (store, bad) = chess_autosave::MemoryStore::load(text);
        if bad {
            self.notes.push(tl!("Saved chess games couldn't be read. They were kept aside, and new games are saved from now on.").to_string());
        }
        self.store = Store::Memory(store);
        self.take_back_no_document();
    }

    /// The games kept for no document, unless some were played already.
    fn take_back_no_document(&mut self) {
        if self.none.has_content() {
            return;
        }
        let keys = DocKeys { name: Some(NO_DOCUMENT.to_owned()), size: Some(0), ..DocKeys::default() };
        let found = self.store.find(&keys, &HashSet::new());
        self.set_aside_notes(&found.set_aside);
        if let Some((cid, body)) = found.games {
            let (boards, lost) = chess_autosave::decode(&body, None);
            let mut c = ChessCollection::from_boards(DocKeys::default(), cid, boards, body.active);
            c.trash = crate::chess_trash::Trash::from_stored(body.trash);
            c.autosave.stored = true;
            self.none = c;
            if lost > 0 {
                self.notes.push(crate::i18n::fmt(tl!("Saved chess games that couldn't be read: {n}"), &[("n", &lost.to_string())]));
            }
        }
    }

    /// The keys the collection for no document is stored under (it has no file of its own).
    fn store_keys(c: &ChessCollection) -> DocKeys {
        if c.cid == NO_DOCUMENT { DocKeys { name: Some(NO_DOCUMENT.to_owned()), size: Some(0), ..DocKeys::default() } } else { c.keys.clone() }
    }

    fn set_aside_notes(&mut self, names: &[String]) {
        for name in names {
            self.notes.push(crate::i18n::fmt(tl!("A file of saved chess games couldn't be read. It was kept as {name}."), &[("name", name)]));
        }
    }

    /// The ids of every collection in memory.
    fn taken(&self) -> HashSet<String> {
        self.docs.values().chain(self.closed.iter()).chain(std::iter::once(&self.none)).map(|c| c.cid.clone()).collect()
    }

    /// Give document `id` (named by `keys`) its collection: one kept from earlier this session,
    /// else the one stored for it, else a blank one (or the one made before its keys were known).
    fn attach(&mut self, id: DocId, keys: DocKeys, protected: bool) {
        let pending = self.docs.remove(&id);
        let blank = pending.as_ref().is_none_or(|c| !c.has_content());
        let mut restored = None;
        if blank {
            if let Some(i) = self.closed.iter().position(|c| c.keys.matches(&keys)) {
                restored = Some((self.closed.remove(i), 0));
            } else {
                let taken = self.taken();
                let found = self.store.find(&keys, &taken);
                self.set_aside_notes(&found.set_aside);
                if let Some((cid, body)) = found.games {
                    let (boards, lost) = chess_autosave::decode(&body, Some(id));
                    let mut c = ChessCollection::from_boards(keys.clone(), cid, boards, body.active);
                    c.trash = crate::chess_trash::Trash::from_stored(body.trash);
                    c.autosave.stored = true;
                    restored = Some((c, lost));
                }
            }
        }
        let mut c = match (restored, pending) {
            (Some((mut c, lost)), _) => {
                // The diagrams the games came from are in this document now.
                for e in c.games.iter_mut() {
                    if let Some(s) = e.board.source.as_mut() {
                        s.doc = id;
                    }
                }
                self.notes.push(restored_note(&c, lost));
                c
            }
            (None, Some(c)) => c,
            (None, None) => ChessCollection::new(keys.clone(), new_cid()),
        };
        c.keys = keys;
        c.autosave.protected = protected;
        self.docs.insert(id, c);
    }

    /// Document `id` was closed: its games are written now, or kept in memory when they can't be.
    fn detach(&mut self, id: DocId) {
        let Some(mut c) = self.docs.remove(&id) else { return };
        let mut w = Writer { store: &mut self.store, consent: &self.consent, said: &mut self.said, notes: &mut self.notes };
        w.tick(&mut c, None);
        if c.has_content() && !c.in_store() {
            self.closed.push(c);
        }
    }

    /// Write the collections changed `DEBOUNCE_SECS` ago or more (all changed ones when `now` is
    /// `None`).
    fn autosave(&mut self, now: Option<f64>) {
        let ChessGames { docs, none, closed, store, consent, said, notes, .. } = self;
        let mut w = Writer { store, consent, said, notes };
        for c in docs.values_mut().chain(std::iter::once(none)) {
            w.tick(c, now);
        }
        closed.retain_mut(|c| {
            w.tick(c, now);
            !c.in_store()
        });
    }
}

/// Writes collections to the store, telling the user once why one wasn't.
struct Writer<'a> {
    store: &'a mut Store,
    consent: &'a HashMap<String, bool>,
    said: &'a mut HashSet<String>,
    notes: &'a mut Vec<String>,
}

impl Writer<'_> {
    fn say(&mut self, key: String, note: impl FnOnce() -> String) {
        if self.said.insert(key) {
            self.notes.push(note());
        }
    }

    /// Note a change of `c`, and write it once it has rested (`now`), or at once (`None`).
    fn tick(&mut self, c: &mut ChessCollection, now: Option<f64>) {
        let fp = c.fingerprint();
        if fp != c.autosave.seen {
            c.autosave.seen = fp;
            c.autosave.changed_at = Some(now.unwrap_or(0.0));
        }
        if fp == c.autosave.saved && !c.autosave.force {
            c.autosave.changed_at = None;
            return;
        }
        let due = match (now, c.autosave.changed_at) {
            _ if c.autosave.force => true,
            (None, _) => true,
            (Some(now), Some(at)) => now - at >= DEBOUNCE_SECS || now < at,
            (Some(_), None) => false,
        };
        if due {
            self.write(c);
        }
    }

    /// Write `c` now. Whether it is in the store as it is (or needs no writing).
    fn write(&mut self, c: &mut ChessCollection) -> bool {
        let fp = c.fingerprint();
        c.autosave.changed_at = None;
        c.autosave.force = false;
        if !self.store.is_on() {
            return false;
        }
        if !c.has_content() && !c.autosave.stored {
            c.autosave.saved = fp;
            return true;
        }
        if c.autosave.protected {
            match self.consent.get(&c.cid) {
                Some(true) => {}
                Some(false) => {
                    c.autosave.saved = fp;
                    return false;
                }
                None => {
                    self.say(format!("ask {}", c.cid), || {
                        tl!("This document is protected: its chess games are autosaved only if you allow it, in the Chess board panel.").to_string()
                    });
                    return false;
                }
            }
        }
        let keys = ChessGames::store_keys(c);
        let boards: Vec<&BoardState> = c.iter().map(|e| &e.board).collect();
        match self.store.write(&c.cid, &keys, c.active_index(), &boards, &c.trash.stored()) {
            Ok(()) => {
                c.autosave.saved = fp;
                c.autosave.stored = true;
                c.autosave.blocked = None;
                // Over the cap again later: said again.
                self.said.remove(&format!("cap {}", c.cid));
                true
            }
            Err(why) => {
                match &why {
                    Blocked::TooBig => self.say(format!("cap {}", c.cid), || {
                        crate::i18n::fmt(
                            tl!("Too many chess games to autosave for this document (at most {n}, {mb} MB). Save them with Save all games as PGN… so nothing is lost."),
                            &[("n", &chess_autosave::MAX_GAMES.to_string()), ("mb", &(chess_autosave::MAX_BODY_BYTES >> 20).to_string())],
                        )
                    }),
                    Blocked::NoLock => self.say("lock".to_owned(), || tl!("Chess games can't be autosaved: the app's data folder couldn't be locked for this window.").to_string()),
                    Blocked::OtherInstance => self.say(format!("other {}", c.cid), || {
                        tl!("These chess games are autosaved by another PdfCraft window. Changes made here aren't autosaved.").to_string()
                    }),
                    Blocked::Io(e) => self.say(format!("io {}", c.cid), || crate::i18n::fmt(tl!("Chess games couldn't be autosaved: {e}"), &[("e", e)])),
                }
                c.autosave.blocked = Some(why);
                false
            }
        }
    }
}

/// "Chess game restored: Werle–Jianu, moves: 34", or "Chess games restored: 3"; and how many
/// couldn't be read.
fn restored_note(c: &ChessCollection, lost: usize) -> String {
    let mut note = if c.len() == 1 {
        let g = c.active();
        crate::i18n::fmt(tl!("Chess game restored: {name}, moves: {n}"), &[("name", &g.name(1)), ("n", &g.moves().to_string())])
    } else {
        crate::i18n::fmt(tl!("Chess games restored: {n}"), &[("n", &c.len().to_string())])
    };
    if lost > 0 {
        note.push_str(" · ");
        note.push_str(&crate::i18n::fmt(tl!("Saved chess games that couldn't be read: {n}"), &[("n", &lost.to_string())]));
    }
    note
}

impl ChessState {
    /// Undo (`undo`) or redo in the chess scope ([`ChessGames::history_step`]): the list's last
    /// change, or the game shown's last change. When another game is shown after it (a
    /// deleted one back, or the one after it), the scope stays on the chess side for the next
    /// Undo, and a set-up begun was the other game's. The step's name when one was taken.
    pub(crate) fn history_step(&mut self, undo: bool) -> Option<String> {
        let shown = self.games.scope_stamp().0;
        let label = self.games.history_step(undo)?;
        if self.games.scope_stamp().0 != shown {
            self.study.setup = None;
            self.enter_scope();
        }
        Some(label)
    }

    /// The board shown: the open document's active game.
    pub fn board(&self) -> &BoardState {
        self.games.board()
    }

    pub fn board_mut(&mut self) -> &mut BoardState {
        self.games.board_mut()
    }
}

/// What the list of games asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum GamesAction {
    /// Show game `n` (0-based).
    Switch(usize),
    New,
    Open,
    Save,
    SaveAll,
    /// The answer to "Save chess games for this protected document?".
    Consent(bool),
    /// Name game `n` (its White, Black and Event tags).
    Rename(usize, GameNames),
    /// A copy of game `n`, after it.
    Duplicate(usize),
    /// Delete game `n` (the user said yes).
    Delete(usize),
    /// Move game `n` one place up (`true`) or down.
    Move(usize, bool),
    /// Move game `from` to place `to` (its place once moved): a row dragged and dropped.
    MoveTo(usize, usize),
    /// Delete the games selected, at these places (the user said yes): one undo step.
    DeleteMany(Vec<usize>),
    /// A copy of each game selected, after it: one undo step.
    DuplicateMany(Vec<usize>),
    /// Move the games selected one place up (`true`) or down, as a block: one undo step.
    MoveMany(Vec<usize>, bool),
    /// Move the games selected into a gap of the list (0: before the first game), together: rows
    /// dragged and dropped, one undo step.
    MoveManyTo(Vec<usize>, usize),
    /// Save the games selected, in list order, as one PGN file.
    SaveMany(Vec<usize>),
    /// A new game from the position of a move of the game shown (`None`: the move shown).
    NewFrom(Option<NodeId>),
    /// Open game `n` of the trash (deleted moves kept) as a new game (plan A8 §7, V5).
    Restore(usize),
}

/// Most deleted games the menu lists (the newest).
const MENU_TRASHED: usize = 10;

/// **Deleted moves (n)…** under the list, while the document's trash holds games: its popup
/// lists them, newest first (name, moves, how many went, when, the first moves), each with
/// **Open as a new game** (plan A8 §7, V5).
fn trash_button(ui: &mut egui::Ui, t: &Tokens, c: &ChessCollection) -> Option<GamesAction> {
    let trash = c.trash();
    if trash.is_empty() {
        return None;
    }
    let label = crate::i18n::fmt(tl!("Deleted moves ({n})…"), &[("n", &trash.len().to_string())]);
    let mut action = None;
    ui.menu_button(label, |ui| {
        ui.set_max_width(320.0);
        // What these are, said in the menu (a tooltip on the button would lie over the menu).
        let what = tl!("Games as they were before moves were deleted from them, kept with the autosave. Open one as a new game.");
        ui.label(egui::RichText::new(what).small().color(t.text_muted));
        ui.separator();
        let lang = crate::i18n::current();
        let count = |n: usize| u64::try_from(n).unwrap_or(u64::MAX);
        for (index, item) in trash.newest_first().take(MENU_TRASHED) {
            ui.push_id(("chess-trash", index), |ui| {
                let moves = crate::i18n::trn(lang, count(item.moves), "{n} move", "{n} moves");
                let gone = crate::i18n::trn(lang, count(item.deleted), "{n} move deleted", "{n} moves deleted");
                ui.label(egui::RichText::new(&item.name).strong());
                let when = crate::chess_trash::ago(item.at);
                let detail = if when.is_empty() { format!("{moves} · {gone}") } else { format!("{moves} · {gone} · {when}") };
                ui.label(egui::RichText::new(detail).small().color(t.text_muted));
                let preview = crate::chess_trash::preview(item);
                if !preview.is_empty() {
                    ui.label(egui::RichText::new(preview).small().monospace());
                }
                if ui.button(tl!("Open as a new game")).clicked() {
                    action = Some(GamesAction::Restore(index));
                }
                ui.separator();
            });
        }
        let more = trash.len().saturating_sub(MENU_TRASHED);
        if more > 0 {
            ui.label(egui::RichText::new(crate::i18n::fmt(tl!("…and {n} older"), &[("n", &more.to_string())])).small().color(t.text_muted));
        }
    });
    action
}

/// Most list keys kept between two frames.
const MAX_LIST_KEYS: usize = 16;

/// The most height the list of games takes before it scrolls.
const LIST_HEIGHT: f32 = 180.0;

/// The size of the buttons on a row under the pointer.
const ROW_BUTTON: f32 = 20.0;

/// Whether a focused row of the list acts on `key` pressed with `m`: the modifiers as the row
/// looks for them (Ctrl and ⌘ as [`Modifiers::COMMAND`]) when it does. Delete, Backspace, F2;
/// Alt+↑/↓ move, Shift+↑/↓ select on; Ctrl+A selects every game, Ctrl+Space picks one.
fn list_key(key: Key, m: Modifiers) -> Option<Modifiers> {
    let m = if m.command_only() { Modifiers::COMMAND } else { m };
    let acts = match key {
        Key::Delete | Key::Backspace | Key::F2 => m.is_none(),
        Key::ArrowUp | Key::ArrowDown => m == Modifiers::ALT || m == Modifiers::SHIFT,
        Key::A | Key::Space => m == Modifiers::COMMAND,
        _ => false,
    };
    acts.then_some(m)
}

/// What a row being dragged carries: its game's id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DraggedGame(u64);

/// Where game `from` goes when dropped on row `i`, in its top half (`above`) or its bottom half:
/// its place once moved, `None` when it stays where it is.
fn drop_place(from: usize, i: usize, above: bool) -> Option<usize> {
    // The gap it is dropped into, counted in the list as it is (0: before the first game).
    let gap = if above { i } else { i.saturating_add(1) };
    let to = if from < gap { gap.saturating_sub(1) } else { gap };
    (to != from).then_some(to)
}

/// The list `order` with the games at `places` (in order, each once) taken out and put back
/// together, in their order, into gap `gap` of the list as it was (0: before the first game).
fn block_order(order: &[u64], places: &[usize], gap: usize) -> Vec<u64> {
    let picked = |i: usize| places.binary_search(&i).is_ok();
    let block: Vec<u64> = places.iter().filter_map(|&i| order.get(i).copied()).collect();
    let mut out = Vec::with_capacity(order.len());
    for (i, &uid) in order.iter().enumerate() {
        if i == gap {
            out.extend_from_slice(&block);
        }
        if !picked(i) {
            out.push(uid);
        }
    }
    if gap >= order.len() {
        out.extend_from_slice(&block);
    }
    out
}

/// How a click (or a key) picks games in the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pick {
    /// That game alone (a plain click: it is shown too).
    Only,
    /// That game added or taken out (Ctrl+click, Ctrl+Space).
    Toggle,
    /// Every game from the anchor to that one (Shift+click, Shift+↑/↓).
    Range,
    /// Every game (Ctrl+A).
    All,
}

/// The games selected in the list, beyond the one shown: what Delete, Duplicate, Move and Save
/// PGN act on when there are several. Kept by game id, so it follows the games when they move;
/// ids no longer in the list count for nothing.
#[derive(Default)]
struct Selection {
    picked: HashSet<u64>,
    /// Where Shift+click and Shift+↑/↓ select from: the last game clicked or picked.
    anchor: Option<u64>,
}

impl Selection {
    /// Whether any game picked is still in `c` (else the selection is the game shown).
    fn any(&self, c: &ChessCollection) -> bool {
        c.iter().any(|e| self.picked.contains(&e.uid))
    }

    /// Whether game `uid` of `c` is selected: picked, or shown when none is picked.
    fn contains(&self, c: &ChessCollection, uid: u64) -> bool {
        if self.any(c) { self.picked.contains(&uid) } else { c.active().uid == uid }
    }

    /// The places of the games selected in `c`, in list order (the game shown alone when none is
    /// picked).
    fn places(&self, c: &ChessCollection) -> Vec<usize> {
        if !self.any(c) {
            return vec![c.active_index()];
        }
        c.iter().enumerate().filter(|(_, e)| self.picked.contains(&e.uid)).map(|(i, _)| i).collect()
    }

    /// Pick game `uid` of `c` as `how` says.
    fn pick(&mut self, c: &ChessCollection, uid: u64, how: Pick) {
        let order: Vec<u64> = c.iter().map(|e| e.uid).collect();
        match how {
            Pick::Only => {
                self.picked.clear();
                self.anchor = Some(uid);
            }
            Pick::Toggle => {
                if !self.any(c) {
                    self.picked = std::iter::once(c.active().uid).collect();
                }
                if !self.picked.remove(&uid) {
                    self.picked.insert(uid);
                }
                self.anchor = Some(uid);
            }
            Pick::Range => {
                let from = self.anchor.filter(|a| order.contains(a)).unwrap_or(c.active().uid);
                self.anchor = Some(from);
                let (Some(a), Some(b)) = (order.iter().position(|&u| u == from), order.iter().position(|&u| u == uid)) else { return };
                self.picked = order.get(a.min(b)..=a.max(b)).unwrap_or_default().iter().copied().collect();
            }
            Pick::All => self.picked = order.into_iter().collect(),
        }
    }

    /// The places a row's menu, buttons or keys act on: the games selected when there are several
    /// and row `i` (game `uid`) is one of them, else that row alone.
    fn targets(&self, c: &ChessCollection, i: usize, uid: u64) -> Vec<usize> {
        let places = self.places(c);
        if places.len() > 1 && self.contains(c, uid) { places } else { vec![i] }
    }
}

/// A game being renamed: the names typed so far.
struct Renaming {
    uid: u64,
    names: GameNames,
    /// Give the first field the keyboard at the next frame.
    focus: bool,
    /// Started from the keyboard on its row: the row gets the keyboard back when done.
    refocus: bool,
    /// Scroll the editor into view at the next frame.
    reveal: bool,
}

/// A game the user asked to delete, until they say yes or no.
struct Deleting {
    uid: u64,
    /// The games selected, when the question is about several (`uid` is the first of them).
    many: Vec<u64>,
    /// Give the Delete button the keyboard at the next frame.
    focus: bool,
    /// Scroll the question into view at the next frame.
    reveal: bool,
}

/// What the list is in the middle of, and where its keyboard focus is.
#[derive(Default)]
struct ListState {
    renaming: Option<Renaming>,
    deleting: Option<Deleting>,
    /// The row (by game id) to give the keyboard, from the pass after the one that asked (a row's
    /// widgets are named by their place, which the change asked with it may move).
    focus_row: Option<(u64, u64)>,
    /// The last of the list's widgets seen with the keyboard focus.
    focused: Option<egui::Id>,
    /// Keys taken for a focused row before the frame ([`ChessGames::take_list_keys`]).
    keys: Vec<(Key, Modifiers)>,
    /// What to scroll into view in the panel this frame, once the list's own scroll area is
    /// drawn (a scroll area takes the requests made inside it: the panel's never sees them).
    reveal: Option<egui::Rect>,
    /// The games selected beyond the one shown.
    sel: Selection,
}

impl ListState {
    /// Rename (or ask about deleting) game `uid`, named `names` now. `focus`: give the editor or
    /// the question the keyboard; `refocus`: give it back to the row afterwards.
    fn ask(&mut self, uid: u64, names: GameNames, rename: bool, focus: bool, refocus: bool) {
        if rename {
            self.renaming = Some(Renaming { uid, names, focus, refocus, reveal: true });
            self.deleting = None;
        } else {
            self.deleting = Some(Deleting { uid, many: Vec::new(), focus, reveal: true });
            self.renaming = None;
        }
    }

    /// Ask whether to delete games `uids` (a selection of several). `focus`: give the question
    /// the keyboard.
    fn ask_many(&mut self, uids: Vec<u64>, focus: bool) {
        let Some(&uid) = uids.first() else { return };
        self.deleting = Some(Deleting { uid, many: uids, focus, reveal: true });
        self.renaming = None;
    }

    /// Scroll `rect` into view: in the list now, and in the panel once the list is drawn.
    fn reveal(&mut self, ui: &egui::Ui, rect: egui::Rect) {
        ui.scroll_to_rect_animation(rect, None, egui::style::ScrollAnimation::none());
        self.reveal = Some(self.reveal.map_or(rect, |r| r.union(rect)));
    }

    /// Give the row of game `uid` the keyboard from the next pass (none: nothing).
    fn focus_later(&mut self, ctx: &egui::Context, uid: Option<u64>) {
        self.focus_row = uid.map(|u| (u, ctx.cumulative_pass_nr()));
    }

    /// Note `r` as one of the list's widgets: while it has the keyboard, the list's keys are its own.
    fn see(&mut self, r: &egui::Response) -> bool {
        let focused = r.has_focus();
        if focused {
            self.focused = Some(r.id);
        }
        focused
    }

    /// Whether `key` (with `m`) was pressed for the row with the keyboard: taken before the frame,
    /// or still in the frame's input (then consumed).
    fn pressed(&mut self, ui: &egui::Ui, key: Key, m: Modifiers) -> bool {
        let at = self.keys.iter().position(|&k| k == (key, m));
        if let Some(i) = at {
            self.keys.remove(i);
            return true;
        }
        ui.input_mut(|i| i.consume_key(m, key))
    }
}

/// What a game's menu (or its row's buttons or keys) chose.
enum RowChoice {
    Rename,
    Duplicate,
    Up,
    Down,
    Delete,
    Save,
}

/// The menu of a row, acting on the games at `targets` (the row's game, or the games selected
/// with it) of `len`: Rename… for one game, Save PGN… for several.
fn row_menu(ui: &mut egui::Ui, targets: &[usize], len: usize) -> Option<RowChoice> {
    let mut choice = None;
    let many = targets.len() > 1;
    if !many && ui.button(tl!("Rename…")).clicked() {
        choice = Some(RowChoice::Rename);
    }
    if ui.button(tl!("Duplicate")).clicked() {
        choice = Some(RowChoice::Duplicate);
    }
    if ui.add_enabled(targets.first().is_some_and(|&i| i > 0), egui::Button::new(tl!("Move up"))).clicked() {
        choice = Some(RowChoice::Up);
    }
    if ui.add_enabled(targets.last().is_some_and(|&i| i.saturating_add(1) < len), egui::Button::new(tl!("Move down"))).clicked() {
        choice = Some(RowChoice::Down);
    }
    if many && ui.button(tl!("Save PGN…")).clicked() {
        choice = Some(RowChoice::Save);
    }
    ui.separator();
    if ui.button(tl!("Delete…")).clicked() {
        choice = Some(RowChoice::Delete);
    }
    if choice.is_some() {
        ui.close();
    }
    choice
}

/// The games of the document shown: the list, what can be done with them, and why they aren't
/// autosaved when they aren't.
pub(crate) fn games_section(ui: &mut egui::Ui, t: &Tokens, games: &mut ChessGames) -> Option<GamesAction> {
    let ChessGames { docs, none, current, store, consent, list, .. } = games;
    let c: &ChessCollection = current.and_then(|id| docs.get(&id)).unwrap_or(none);
    // A question about a game that is gone (deleted, or another document shown) is dropped.
    if list.renaming.as_ref().is_some_and(|r| c.position(r.uid).is_none()) {
        list.renaming = None;
    }
    if list.deleting.as_ref().is_some_and(|d| c.position(d.uid).is_none()) {
        list.deleting = None;
    }
    // Games no longer in the list (deleted, or another document's) are no longer selected.
    list.sel.picked.retain(|&uid| c.position(uid).is_some());
    let mut action = None;
    let n = c.len().to_string();
    let title = if current.is_some() { tl!("Games in this document ({n})") } else { tl!("Games ({n})") };
    let mut header = egui::CollapsingHeader::new(crate::i18n::fmt(title, &[("n", &n)])).id_salt("chess-games").default_open(true);
    if list.renaming.is_some() || list.deleting.is_some() {
        header = header.open(Some(true));
    }
    header.show(ui, |ui| {
        if c.autosave.protected && store.is_on() && c.has_content() && !consent.contains_key(&c.cid) {
            ui.label(egui::RichText::new(tl!("Save chess games for this protected document?")).small().strong());
            ui.label(
                egui::RichText::new(tl!("They are kept in the app's own folder, unencrypted, so they come back when the document is opened again."))
                    .small()
                    .color(t.text_muted),
            );
            ui.horizontal(|ui| {
                if ui.small_button(tl!("Save them")).clicked() {
                    action = Some(GamesAction::Consent(true));
                }
                if ui.small_button(tl!("Don't save")).clicked() {
                    action = Some(GamesAction::Consent(false));
                }
            });
        }
        let why = match &c.autosave.blocked {
            Some(Blocked::TooBig) => Some(tl!("Too many games to autosave here: save them as PGN.")),
            Some(Blocked::OtherInstance) => Some(tl!("Another PdfCraft window autosaves these games.")),
            Some(Blocked::NoLock | Blocked::Io(_)) => Some(tl!("These games aren't being autosaved.")),
            None => None,
        };
        if let Some(why) = why {
            ui.label(egui::RichText::new(why).small().color(t.text_muted));
        }
        // The list's header: every game, in this order, to one PGN file.
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let r = ui.small_button(tl!("Export…")).on_hover_text(tl!("Save all games as PGN…"));
                list.see(&r);
                if r.clicked() {
                    action = Some(GamesAction::SaveAll);
                }
            });
        });
        list.reveal = None;
        // At least the rows' height (as the last frame measured it), up to the list's own: the
        // panel's notation takes the height left, which would otherwise leave the list a row.
        let tall_id = ui.id().with("chess-games-list-height");
        let tall = ui.ctx().data(|d| d.get_temp::<f32>(tall_id)).filter(|h| h.is_finite()).unwrap_or(0.0).clamp(0.0, LIST_HEIGHT);
        let shown = egui::ScrollArea::vertical()
            .id_salt("chess-games-list")
            .max_height(LIST_HEIGHT)
            .min_scrolled_height(tall)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for (i, e) in c.iter().enumerate() {
                    // The row's state (under the pointer or not) follows its game, not its place.
                    if let Some(a) = ui.push_id(("chess-game", e.uid), |ui| game_row(ui, list, c, i, e)).inner {
                        action = Some(a);
                    }
                }
            });
        let measured = shown.content_size.y;
        if measured.is_finite() && (measured - tall).abs() > 0.5 && (measured < LIST_HEIGHT || tall < LIST_HEIGHT) {
            ui.ctx().data_mut(|d| d.insert_temp(tall_id, measured));
            ui.ctx().request_repaint();
        }
        if let Some(a) = pending(ui, t, list, c) {
            action = Some(a);
        }
        // The panel scrolls too, to what the list scrolled to.
        if let Some(rect) = list.reveal.take() {
            ui.scroll_to_rect_animation(rect, None, egui::style::ScrollAnimation::none());
        }
        ui.horizontal_wrapped(|ui| {
            if ui.small_button(tl!("New game")).clicked() {
                action = Some(GamesAction::New);
            }
            if ui.small_button(tl!("Open PGN…")).clicked() {
                action = Some(GamesAction::Open);
            }
            if ui.small_button(tl!("Save PGN…")).clicked() {
                action = Some(GamesAction::Save);
            }
            if let Some(a) = trash_button(ui, t, c) {
                action = Some(a);
            }
        });
    });
    // Games deleted or copied leave nothing selected (the first copy is shown), and a game added
    // is shown alone.
    if matches!(action, Some(GamesAction::DeleteMany(_) | GamesAction::DuplicateMany(_) | GamesAction::New | GamesAction::Restore(_))) {
        list.sel = Selection::default();
    }
    action
}

/// Game `i` of `c` in the list: its name (a click or Enter shows it; Ctrl+click and Shift+click
/// select it with others), its menu, the keys of the focused row and the buttons of the row under
/// the pointer. A row's state, selected or not, is the button's (screen readers say it).
fn game_row(ui: &mut egui::Ui, list: &mut ListState, c: &ChessCollection, i: usize, e: &GameEntry) -> Option<GamesAction> {
    let mut action = None;
    let (uid, len, active) = (e.uid, c.len(), i == c.active_index());
    let selected = list.sel.contains(c, uid);
    let name = e.name(i.saturating_add(1));
    // The game shown, while other games are selected without it, in bold.
    let text = if active && !selected { egui::RichText::new(name.as_str()).strong() } else { egui::RichText::new(name.as_str()) };
    // The row's buttons show while the pointer is on the row (as the last frame left it) or the
    // keyboard is on the row or its buttons.
    let lit_id = ui.id().with("chess-game-row");
    let lit = ui.ctx().data(|d| d.get_temp::<bool>(lit_id)).unwrap_or(false);
    let mut chosen = None;
    let mut focused = false;
    let mut by_keys = false;
    let row = ui.horizontal(|ui| {
        // Clicked to show the game, or dragged to another place in the list.
        let r = ui.add(egui::Button::selectable(selected, text).sense(egui::Sense::click_and_drag()));
        r.dnd_set_drag_payload(DraggedGame(uid));
        if list.focus_row.is_some_and(|(u, asked)| u == uid && ui.ctx().cumulative_pass_nr() > asked) {
            r.request_focus();
            list.reveal(ui, r.rect);
            list.focus_row = None;
        }
        if r.clicked() {
            let m = ui.input(|i| i.modifiers);
            if m.command {
                list.sel.pick(c, uid, Pick::Toggle);
            } else if m.shift {
                list.sel.pick(c, uid, Pick::Range);
            } else {
                list.sel.pick(c, uid, Pick::Only);
                if !active {
                    action = Some(GamesAction::Switch(i));
                }
            }
        }
        r.context_menu(|ui| {
            chosen = row_menu(ui, &list.sel.targets(c, i, uid), len);
        });
        if list.see(&r) {
            focused = true;
            by_keys = true;
            if list.pressed(ui, Key::F2, Modifiers::NONE) {
                chosen = Some(RowChoice::Rename);
            } else if list.pressed(ui, Key::Delete, Modifiers::NONE) || list.pressed(ui, Key::Backspace, Modifiers::NONE) {
                chosen = Some(RowChoice::Delete);
            } else if list.pressed(ui, Key::ArrowUp, Modifiers::ALT) {
                chosen = Some(RowChoice::Up);
            } else if list.pressed(ui, Key::ArrowDown, Modifiers::ALT) {
                chosen = Some(RowChoice::Down);
            } else if list.pressed(ui, Key::A, Modifiers::COMMAND) {
                list.sel.pick(c, uid, Pick::All);
            } else if list.pressed(ui, Key::Space, Modifiers::COMMAND) {
                list.sel.pick(c, uid, Pick::Toggle);
            } else {
                // Shift+↑/↓: the keyboard goes to the next row up or down, and the selection runs
                // from the anchor (this row, when there is none) to it.
                let next = if list.pressed(ui, Key::ArrowUp, Modifiers::SHIFT) {
                    i.checked_sub(1)
                } else if list.pressed(ui, Key::ArrowDown, Modifiers::SHIFT) {
                    i.checked_add(1)
                } else {
                    None
                };
                if let Some(to) = next.and_then(|n| c.get(n)).map(|n| n.uid) {
                    if !list.sel.anchor.is_some_and(|a| c.position(a).is_some()) {
                        list.sel.anchor = Some(uid);
                    }
                    list.sel.pick(c, to, Pick::Range);
                    list.focus_later(ui.ctx(), Some(to));
                }
            }
        }
        let spacing = ui.spacing().item_spacing.x;
        let needed = 3.0 * ROW_BUTTON + 2.0 * spacing;
        let room = ui.available_width();
        if (lit || focused) && room.is_finite() && room >= needed {
            ui.add_space(room - needed);
            for (icon, label, what) in [
                ("pencil", tl!("Rename game…"), RowChoice::Rename),
                ("copy-plus", tl!("Duplicate game"), RowChoice::Duplicate),
                ("trash-2", tl!("Delete game…"), RowChoice::Delete),
            ] {
                let b = crate::icons::button(ui, icon, ROW_BUTTON, false, label);
                let has = list.see(&b);
                focused |= has;
                if b.clicked() {
                    by_keys = has;
                    chosen = Some(what);
                }
            }
        }
    });
    // A row dragged over this one: a line where it would go; dropped, it goes there (with the
    // games selected with it, as a block).
    let rect = row.response.rect;
    let above = || ui.input(|i| i.pointer.interact_pos()).is_some_and(|p| p.y < rect.center().y);
    if let Some(dragged) = row.response.dnd_hover_payload::<DraggedGame>() {
        let above = above();
        if dropped_move(list, c, dragged.0, i, above).is_some() {
            let y = if above { rect.top() } else { rect.bottom() };
            ui.painter().hline(rect.x_range(), y, ui.visuals().selection.stroke);
        }
    }
    if let Some(dropped) = row.response.dnd_release_payload::<DraggedGame>()
        && let Some(a) = dropped_move(list, c, dropped.0, i, above())
    {
        action = Some(a);
    }
    let lit_now = ui.rect_contains_pointer(row.response.rect) || focused;
    if lit_now != lit {
        ui.ctx().data_mut(|d| d.insert_temp(lit_id, lit_now));
        ui.ctx().request_repaint();
    }
    let targets = list.sel.targets(c, i, uid);
    let many = targets.len() > 1;
    let first = targets.first().copied().unwrap_or(i);
    let last = targets.last().copied().unwrap_or(i);
    match chosen {
        Some(RowChoice::Rename) => list.ask(uid, e.names(), true, true, by_keys),
        Some(RowChoice::Delete) if many => list.ask_many(targets.iter().filter_map(|&k| c.get(k)).map(|g| g.uid).collect(), by_keys),
        Some(RowChoice::Delete) => list.ask(uid, e.names(), false, by_keys, false),
        Some(RowChoice::Duplicate) if many => action = Some(GamesAction::DuplicateMany(targets)),
        Some(RowChoice::Duplicate) => action = Some(GamesAction::Duplicate(i)),
        Some(RowChoice::Save) => action = Some(GamesAction::SaveMany(targets)),
        Some(RowChoice::Up) if first > 0 => {
            action = Some(if many { GamesAction::MoveMany(targets, true) } else { GamesAction::Move(i, true) });
            list.focus_later(ui.ctx(), by_keys.then_some(uid));
        }
        Some(RowChoice::Down) if last.saturating_add(1) < len => {
            action = Some(if many { GamesAction::MoveMany(targets, false) } else { GamesAction::Move(i, false) });
            list.focus_later(ui.ctx(), by_keys.then_some(uid));
        }
        Some(RowChoice::Up | RowChoice::Down) | None => {}
    }
    action
}

/// What dropping game `dragged` on row `i`, in its top half (`above`) or its bottom half, does:
/// the game goes into that gap, or the games selected with it go there together; `None` when
/// nothing would move.
fn dropped_move(list: &ListState, c: &ChessCollection, dragged: u64, i: usize, above: bool) -> Option<GamesAction> {
    let from = c.position(dragged)?;
    let targets = list.sel.targets(c, from, dragged);
    if targets.len() > 1 {
        let gap = if above { i } else { i.saturating_add(1) };
        let order: Vec<u64> = c.iter().map(|e| e.uid).collect();
        return (block_order(&order, &targets, gap) != order).then_some(GamesAction::MoveManyTo(targets, gap));
    }
    drop_place(from, i, above).map(|to| GamesAction::MoveTo(from, to))
}

/// The question about deleting a game (or the games selected), or a game's name editor, when one
/// is open: under the list (not inside its scroll area, which would clip it and keep the panel
/// from scrolling to it), headed by the game's name (or how many are selected).
fn pending(ui: &mut egui::Ui, t: &Tokens, list: &mut ListState, c: &ChessCollection) -> Option<GamesAction> {
    let deleting = list.deleting.as_ref().and_then(|d| c.position(d.uid));
    let renaming = list.renaming.as_ref().and_then(|r| c.position(r.uid));
    let i = deleting.or(renaming)?;
    let many: Vec<usize> = match (&list.deleting, deleting) {
        (Some(d), Some(_)) => d.many.iter().filter_map(|&uid| c.position(uid)).collect(),
        _ => Vec::new(),
    };
    let name = if many.len() > 1 {
        crate::i18n::fmt(tl!("{n} selected"), &[("n", &many.len().to_string())])
    } else {
        c.get(i).map(|e| e.name(i.saturating_add(1))).unwrap_or_default()
    };
    let mut action = None;
    egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
        ui.label(egui::RichText::new(name).strong());
        action = if deleting.is_some() { delete_question(ui, t, list, c, i, many) } else { name_editor(ui, list, i) };
    });
    action
}

/// "Delete this game? You can undo it." about game `i` (or "Delete these games?" about the games
/// at `many`, when there are several), with Delete and Cancel (or Esc).
fn delete_question(ui: &mut egui::Ui, t: &Tokens, list: &mut ListState, c: &ChessCollection, i: usize, many: Vec<usize>) -> Option<GamesAction> {
    let mut action = None;
    let uid = c.get(i).map(|e| e.uid);
    let several = many.len() > 1;
    let question = if several { tl!("Delete these games? You can undo it.") } else { tl!("Delete this game? You can undo it.") };
    ui.label(egui::RichText::new(question).small().color(t.text_muted));
    ui.horizontal(|ui| {
        let yes = ui.small_button(tl!("Delete"));
        let no = ui.small_button(tl!("Cancel"));
        if list.deleting.as_mut().is_some_and(|d| std::mem::take(&mut d.focus)) {
            yes.request_focus();
        }
        // The question may open at the bottom of the panel: shown whole.
        if list.deleting.as_mut().is_some_and(|d| std::mem::take(&mut d.reveal)) {
            list.reveal(ui, yes.rect.union(no.rect));
        }
        let keys = list.see(&yes) | list.see(&no);
        // Esc takes the keyboard from the button before the frame: it was on one of them.
        let esc = (yes.lost_focus() || no.lost_focus()) && ui.input(|inp| inp.key_pressed(Key::Escape));
        if yes.clicked() {
            // The keyboard goes to the game that takes the deleted ones' place: the first after
            // them that stays, else the last before them.
            let gone = if several { many.clone() } else { vec![i] };
            let stays = |k: &usize| !gone.contains(k);
            let after = gone.last().map_or(i, |&l| l.saturating_add(1));
            let next = (after..c.len()).find(stays).or_else(|| (0..gone.first().copied().unwrap_or(i)).rev().find(stays));
            list.focus_later(ui.ctx(), if keys { next.and_then(|k| c.get(k)).map(|e| e.uid) } else { None });
            list.deleting = None;
            action = Some(if several { GamesAction::DeleteMany(many) } else { GamesAction::Delete(i) });
        } else if no.clicked() || esc {
            list.focus_later(ui.ctx(), if keys || esc { uid } else { None });
            list.deleting = None;
        }
    });
    action
}

/// The name editor of game `i`: White, Black, Event, Site, Date and Round (as Game details names
/// them), then Rename (or Enter) and Cancel (or Esc). A date PGN can't read is said under the
/// fields, and keeps Rename from applying.
fn name_editor(ui: &mut egui::Ui, list: &mut ListState, i: usize) -> Option<GamesAction> {
    let r = list.renaming.as_mut()?;
    let (mut commit, mut cancel) = (false, false);
    let mut seen = None;
    // The editor's top, and whether a field just got the keyboard: the whole editor is then
    // scrolled into view, so its buttons never hide under the panel's edge.
    let (mut top, mut gained, mut reveal) = (None::<egui::Rect>, false, None);
    ui.scope(|ui| {
        egui::Grid::new(("chess-game-names", r.uid)).num_columns(2).spacing([6.0, 3.0]).show(ui, |ui| {
            let focus = std::mem::take(&mut r.focus);
            let GameNames { white, black, event, site, date, round } = &mut r.names;
            let fields = [
                (tl!("White"), white),
                (tl_ctx!("chess side", "Black"), black),
                (tl!("Event"), event),
                (tl!("Site"), site),
                (tl!("Date"), date),
                (tl!("Round"), round),
            ];
            for (k, (label, value)) in fields.into_iter().enumerate() {
                let l = ui.label(label);
                let f = ui.add(egui::TextEdit::singleline(value).desired_width(150.0)).labelled_by(l.id);
                if k == 0 && focus {
                    f.request_focus();
                }
                top = Some(top.map_or(f.rect, |t| t.union(f.rect)));
                gained |= f.gained_focus();
                if f.lost_focus() {
                    let (enter, esc) = ui.input(|inp| (inp.key_pressed(Key::Enter), inp.key_pressed(Key::Escape)));
                    commit |= enter;
                    cancel |= esc;
                }
                ui.end_row();
            }
        });
        let date_ok = collection::pgn_date_ok(&r.names.date);
        if !date_ok {
            ui.label(
                egui::RichText::new(tl!("A date is written YYYY.MM.DD, with ? for each digit not known.")).small().color(ui.visuals().error_fg_color),
            );
            commit = false;
        }
        ui.horizontal(|ui| {
            let ok = ui.add_enabled(date_ok, egui::Button::new(tl!("Rename")).small());
            let no = ui.small_button(tl!("Cancel"));
            if std::mem::take(&mut r.reveal) || gained {
                reveal = Some(top.map_or(ok.rect, |t| t.union(ok.rect)).union(no.rect));
            }
            seen = [&ok, &no].into_iter().find(|b| b.has_focus()).map(|b| b.id);
            commit |= ok.clicked();
            cancel |= no.clicked();
        });
    });
    if seen.is_some() {
        list.focused = seen;
    }
    if let Some(rect) = reveal {
        list.reveal(ui, rect);
    }
    if !(commit || cancel) {
        return None;
    }
    let r = list.renaming.take()?;
    list.focus_later(ui.ctx(), r.refocus.then_some(r.uid));
    (commit && !cancel).then_some(GamesAction::Rename(i, r.names))
}

/// `message`, then ` · note` when there is one.
fn with_note(message: String, note: Option<String>) -> String {
    match note {
        Some(note) => format!("{message} · {note}"),
        None => message,
    }
}

/// Ask for a new game from the position of move `node` of the game shown (a move's menu in the
/// notation): the app makes it at the next frame.
pub(crate) fn ask_new_game_from(ctx: &egui::Context, node: NodeId) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(NEW_GAME_FROM), node));
    ctx.request_repaint();
}

/// Where [`ask_new_game_from`] leaves its request.
const NEW_GAME_FROM: &str = "chess-new-game-from";

/// Which games of the document shown go into a PGN file.
enum PgnGames {
    /// The game shown (Save PGN…).
    Shown,
    /// Every game (Export…, Save all games as PGN…).
    All,
    /// The games selected in the list, by place (a row's Save PGN… with several selected).
    These(Vec<usize>),
}

/// A name for a file: the characters file systems refuse replaced.
fn file_stem(name: &str) -> String {
    let clean: String = name.chars().map(|c| if c.is_control() || "\\/:*?\"<>|".contains(c) { '_' } else { c }).take(100).collect();
    let clean = clean.trim().trim_matches('.').to_owned();
    if clean.is_empty() { "game".to_owned() } else { clean }
}

impl PdfCraftApp {
    /// Every frame: the open documents' collections made, closed ones written, changes written
    /// once they have rested, and notices shown.
    pub(crate) fn chess_games_tick(&mut self) {
        let now = self.ctx.as_ref().map_or(0.0, |c| c.input(|i| i.time));
        self.sync_chess_docs();
        // "New game from this position" in a move's menu.
        let asked = self.ctx.as_ref().and_then(|c| c.data_mut(|d| d.remove_temp::<NodeId>(egui::Id::new(NEW_GAME_FROM))));
        if let Some(node) = asked {
            self.chess_games_action(GamesAction::NewFrom(Some(node)));
        }
        self.chess.games.autosave(Some(now));
        #[cfg(target_arch = "wasm32")]
        self.take_chess_uploads();
        self.show_chess_notes();
    }

    /// Write every changed collection now (on exit, when eframe saves, and in tests).
    pub fn flush_chess_games(&mut self) {
        self.sync_chess_docs();
        self.chess.games.autosave(None);
        self.show_chess_notes();
    }

    /// [`Self::flush_chess_games`], then hand the web's value to eframe's `Storage`.
    pub(crate) fn chess_games_to_storage(&mut self, storage: &mut dyn eframe::Storage) {
        self.flush_chess_games();
        if let Store::Memory(m) = &mut self.chess.games.store {
            if let Some(value) = m.take_value() {
                storage.set_string(chess_autosave::STORAGE_KEY, value);
            }
            if let Some(bad) = m.take_bad() {
                storage.set_string(chess_autosave::STORAGE_BAD_KEY, bad);
            }
        }
        // The diagram corrections, under their own key (plan A8, P3b2).
        if let Some(value) = self.chess.corrections.storage_value() {
            storage.set_string(crate::chess_corrections_ui::STORAGE_KEY, value);
        }
    }

    /// The web build: autosave the chess games in eframe's `Storage` (read from it now).
    pub fn chess_games_from_storage(&mut self, storage: Option<&dyn eframe::Storage>) {
        let text = storage.and_then(|s| s.get_string(chess_autosave::STORAGE_KEY));
        self.chess.games.autosave_to_storage(text.as_deref());
        let corrections = storage.and_then(|s| s.get_string(crate::chess_corrections_ui::STORAGE_KEY));
        self.chess.corrections.use_storage(corrections.as_deref());
    }

    fn show_chess_notes(&mut self) {
        let notes = std::mem::take(&mut self.chess.games.notes);
        if !notes.is_empty() {
            self.notify(notes.join(" · "));
        }
    }

    /// The keys document `id` has now, and whether it is protected.
    fn chess_doc_state(&self, id: DocId) -> Option<(DocKeys, bool)> {
        Some((self.session.get(id)?.doc_keys(), self.chess_doc_protected(id)?))
    }

    /// Whether document `id` is protected: encrypted, or copying not allowed. Its games are
    /// written only if the user allows it (the rule recovery follows for its snapshots).
    fn chess_doc_protected(&self, id: DocId) -> Option<bool> {
        self.session.get(id).map(|doc| doc.info.encrypted || !doc.allows_copying())
    }

    /// Match the collections to the open documents: the one shown, new ones given theirs,
    /// closed ones written.
    pub(crate) fn sync_chess_docs(&mut self) {
        self.chess.games.current = self.active_ids().map(|(_, id)| id);
        let open: Vec<DocId> = self.views.iter().map(|v| v.id).collect();
        let gone: Vec<DocId> = self.chess.games.docs.keys().filter(|id| !open.contains(id)).copied().collect();
        for id in gone {
            self.chess.games.detach(id);
        }
        for id in open {
            let known = self.chess.games.docs.get(&id).is_some_and(|c| !c.keys.is_empty());
            if known {
                // Unlocking or a new permission can change this.
                let protected = self.chess_doc_protected(id);
                if let (Some(p), Some(c)) = (protected, self.chess.games.docs.get_mut(&id)) {
                    c.autosave.protected = p;
                }
                continue;
            }
            if let Some((keys, protected)) = self.chess_doc_state(id) {
                self.chess.games.attach(id, keys, protected);
            }
        }
    }

    /// Document `id` was saved: its collection takes the document's keys now (a first `/ID`, a
    /// new size or path) and is written under them at once, so the next open finds it.
    pub(crate) fn rekey_chess_games(&mut self, id: DocId) {
        let Some((keys, protected)) = self.chess_doc_state(id) else { return };
        let games = &mut self.chess.games;
        let Some(c) = games.docs.get_mut(&id) else { return };
        if c.keys == keys {
            return;
        }
        c.keys = keys;
        c.autosave.protected = protected;
        let ChessGames { docs, store, consent, said, notes, .. } = games;
        let mut w = Writer { store, consent, said, notes };
        if let Some(c) = docs.get_mut(&id) {
            w.write(c);
        }
        self.show_chess_notes();
    }

    /// Put a new game on the board: the position `fen` (from Black's side when `flipped`), from
    /// diagram `source`. It is added to the document's games and shown, one undo step of the list
    /// ("Load the diagram"); the game shown before stays as it was. The same diagram, opened
    /// before and untouched since, is shown again instead (no step: switching games is none).
    /// Whether the position could be read (else the user is told why).
    pub(crate) fn open_chess_game(&mut self, fen: &str, flipped: bool, source: Option<Source>) -> bool {
        self.sync_chess_docs();
        let mut board = BoardState::default();
        board.load_fen(fen, flipped, source);
        if board.history.undo_edit().is_none() {
            // The board says why (the FEN couldn't be read).
            if let Some(why) = board.message.take() {
                self.notify(why);
            }
            return false;
        }
        // A new game: its own history starts empty (opening it is a step of the list's).
        board.history = Default::default();
        let c = self.chess.games.collection_mut();
        let same = c.iter().position(|e| {
            source.is_some()
                && e.board.source == source
                && e.untouched()
                && e.board.flipped == flipped
                && e.board.current_fen() == board.current_fen()
        });
        match same {
            Some(i) => {
                c.switch_to(i);
            }
            None => {
                c.add_games(vec![board], ChessEdit::LoadDiagram, true);
            }
        }
        self.chess.study.setup = None;
        self.chess.enter_scope();
        true
    }

    /// What the list of games asked for.
    pub(crate) fn chess_games_action(&mut self, action: GamesAction) {
        self.sync_chess_docs();
        match action {
            GamesAction::Switch(i) => {
                if self.chess.games.collection_mut().switch_to(i) {
                    self.chess.study.setup = None;
                    self.chess.enter_scope();
                }
            }
            GamesAction::New => {
                self.chess.games.collection_mut().new_game();
                self.chess.study.setup = None;
                self.chess.enter_scope();
            }
            GamesAction::Rename(i, names) => {
                if let Some(e) = self.chess.games.collection_mut().get_mut(i) {
                    e.rename(&names);
                }
                self.chess.enter_scope();
            }
            GamesAction::Duplicate(i) => {
                if self.chess.games.collection_mut().duplicate(i) {
                    self.chess.study.setup = None;
                    self.chess.enter_scope();
                }
            }
            GamesAction::Delete(i) => {
                let c = self.chess.games.collection_mut();
                let shown = c.active().uid;
                if c.delete(i) && c.active().uid != shown {
                    self.chess.study.setup = None;
                }
                self.chess.enter_scope();
            }
            GamesAction::Move(i, up) => {
                if self.chess.games.collection_mut().move_game(i, up) {
                    self.chess.enter_scope();
                }
            }
            GamesAction::MoveTo(from, to) => {
                if self.chess.games.collection_mut().move_to(from, to) {
                    self.chess.enter_scope();
                }
            }
            GamesAction::DeleteMany(places) => {
                let c = self.chess.games.collection_mut();
                let shown = c.active().uid;
                if c.delete_many(&places) > 0 && c.active().uid != shown {
                    self.chess.study.setup = None;
                }
                self.chess.enter_scope();
            }
            GamesAction::DuplicateMany(places) => {
                if self.chess.games.collection_mut().duplicate_many(&places) > 0 {
                    self.chess.study.setup = None;
                    self.chess.enter_scope();
                }
            }
            GamesAction::MoveMany(places, up) => {
                if self.chess.games.collection_mut().move_many(&places, up) {
                    self.chess.enter_scope();
                }
            }
            GamesAction::MoveManyTo(places, gap) => {
                if self.chess.games.collection_mut().move_many_to(&places, gap) {
                    self.chess.enter_scope();
                }
            }
            GamesAction::SaveMany(places) => self.save_chess_games_pgn(PgnGames::These(places)),
            GamesAction::NewFrom(node) => match self.chess.games.collection_mut().new_from(node) {
                Ok(()) => {
                    self.chess.study.setup = None;
                    self.chess.enter_scope();
                }
                Err(why) => self.notify(why),
            },
            GamesAction::Restore(index) => match self.chess.games.collection_mut().restore_trashed(index) {
                Ok(()) => {
                    self.chess.study.setup = None;
                    self.chess.enter_scope();
                }
                Err(why) => self.notify(why),
            },
            GamesAction::Open => self.open_chess_pgn(),
            GamesAction::Save => self.save_chess_games_pgn(PgnGames::Shown),
            GamesAction::SaveAll => self.save_chess_games_pgn(PgnGames::All),
            GamesAction::Consent(yes) => {
                let games = &mut self.chess.games;
                let cid = games.collection().cid.clone();
                games.consent.insert(cid, yes);
                if yes {
                    games.collection_mut().autosave.force = true;
                    games.autosave(None);
                    self.show_chess_notes();
                }
            }
        }
    }

    /// Chess ▸ Rename game…, Duplicate game, Delete game… and New game from this position, on the
    /// game shown. Renaming and deleting show the board panel's list, where the name editor or
    /// the question waits.
    pub(crate) fn chess_game_command(&mut self, id: &str) {
        self.sync_chess_docs();
        match id {
            "chess.game.new_from_here" => self.chess_games_action(GamesAction::NewFrom(None)),
            "chess.game.duplicate" => {
                let i = self.chess.games.collection().active_index();
                self.chess_games_action(GamesAction::Duplicate(i));
            }
            "chess.game.rename" | "chess.game.delete" => {
                self.chess.games.ask_about_shown(id == "chess.game.rename");
                self.show_chess_board();
                self.chess.enter_scope();
            }
            _ => {}
        }
    }

    /// The PGN of `which` games of the document shown (in list order, a blank line between two),
    /// and how many games were left out (illegal positions have no PGN). `None`, the user told
    /// why, when the games come from a document that doesn't allow copying, or there is nothing
    /// to write.
    fn chess_pgn_text(&mut self, which: &PgnGames) -> Option<(String, usize)> {
        let c = self.chess.games.collection();
        let boards: Vec<&BoardState> = match which {
            PgnGames::Shown => vec![&c.active().board],
            PgnGames::All => c.iter().map(|e| &e.board).collect(),
            PgnGames::These(places) => c.iter().enumerate().filter(|(i, _)| places.contains(i)).map(|(_, e)| &e.board).collect(),
        };
        let docs: HashSet<DocId> = boards.iter().filter_map(|b| b.source.map(|s| s.doc)).collect();
        // Each game's PGN ends with a line break: joined by another, a blank line parts them.
        let text: Vec<String> = boards.iter().filter(|b| b.illegal.is_none()).map(|b| b.game.pgn()).collect();
        let skipped = boards.len().saturating_sub(text.len());
        for doc in docs {
            if self.session.get(doc).is_some() && !self.may_copy(doc) {
                return None;
            }
        }
        if text.is_empty() {
            self.notify(tl!("Nothing to save: an illegal position has no PGN.").to_string());
            return None;
        }
        Some((text.join("\n"), skipped))
    }

    /// Chess ▸ Save PGN… (`all`: Save all games as PGN…, the list's Export…): write the game
    /// shown, or the document's games in list order, to a `.pgn` file the user chooses
    /// (downloaded on the web). Games that couldn't be written (illegal positions) are said.
    pub fn save_chess_pgn(&mut self, all: bool) {
        self.save_chess_games_pgn(if all { PgnGames::All } else { PgnGames::Shown });
    }

    /// [`Self::save_chess_pgn`] of `which` games: the game shown, every game, or the games
    /// selected in the list (named as every game is: "<document> games.pgn").
    fn save_chess_games_pgn(&mut self, which: PgnGames) {
        let Some((text, skipped)) = self.chess_pgn_text(&which) else { return };
        let all = !matches!(which, PgnGames::Shown);
        let left_out =
            (skipped > 0).then(|| crate::i18n::fmt(tl!("Games not saved (an illegal position has no PGN): {n}"), &[("n", &skipped.to_string())]));
        let stem = if all {
            self.active_ids().and_then(|(_, id)| self.session.get(id)).map_or_else(
                || "games".to_owned(),
                |d| {
                    format!(
                        "{} games",
                        std::path::Path::new(&d.name).file_stem().map_or_else(|| d.name.clone(), |s| s.to_string_lossy().into_owned())
                    )
                },
            )
        } else {
            let c = self.chess.games.collection();
            c.active().name(c.active_index().saturating_add(1))
        };
        let name = format!("{}.pgn", file_stem(&stem));
        #[cfg(not(target_arch = "wasm32"))]
        {
            let dialog = rfd::AsyncFileDialog::new().add_filter("PGN", &["pgn"]).set_file_name(&name);
            self.ask_one(crate::pickers::Ask::Save(dialog), None, move |app, dest| {
                let shown = dest.file_name().map_or_else(|| name.clone(), |n| n.to_string_lossy().into_owned());
                match crate::editing::write_atomically(&dest.to_string_lossy(), text.as_bytes()) {
                    Ok(()) => app.notify(with_note(crate::i18n::fmt(tl!("Saved {name}"), &[("name", &shown)]), left_out)),
                    Err(e) => app.notify_fmt("Couldn't save {name}: {e}", &[("name", &shown), ("e", &e.to_string())]),
                }
            });
        }
        #[cfg(target_arch = "wasm32")]
        match crate::editing::download(&name, text.as_bytes()) {
            Ok(()) => self.notify(with_note(crate::i18n::fmt(tl!("Downloaded {name}"), &[("name", &name)]), left_out)),
            Err(e) => self.notify_fmt("Couldn't download {name}: {e}", &[("name", &name), ("e", &e)]),
        }
    }

    /// Chess ▸ Open PGN…: add the games of one or more `.pgn` files to the document's games.
    pub fn open_chess_pgn(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let dialog = rfd::AsyncFileDialog::new().add_filter("PGN", &["pgn", "txt"]);
            self.ask(crate::pickers::Ask::Files(dialog), None, |app, paths| {
                for path in paths {
                    let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
                    match read_capped(&path) {
                        Ok(bytes) => {
                            app.import_chess_pgn(&name, &bytes);
                        }
                        Err(e) => app.notify_fmt("Couldn't read {name}: {e}", &[("name", &name), ("e", &e)]),
                    }
                }
            });
        }
        #[cfg(target_arch = "wasm32")]
        {
            let uploads = self.chess.games.uploads.clone();
            let ctx = self.ctx.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let handles = rfd::AsyncFileDialog::new().add_filter("PGN", &["pgn", "txt"]).pick_files().await.unwrap_or_default();
                for h in handles {
                    let bytes = h.read().await;
                    if let Ok(mut q) = uploads.lock() {
                        q.push((h.file_name(), bytes));
                    }
                }
                if let Some(ctx) = ctx {
                    ctx.request_repaint();
                }
            });
        }
    }

    /// The PGN files the browser has read since the last frame.
    #[cfg(target_arch = "wasm32")]
    fn take_chess_uploads(&mut self) {
        let files = match self.chess.games.uploads.lock() {
            Ok(mut q) => std::mem::take(&mut *q),
            Err(poisoned) => std::mem::take(&mut *poisoned.into_inner()),
        };
        for (name, bytes) in files {
            if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_PGN_FILE_BYTES {
                self.notify_fmt("Couldn't read {name}: {e}", &[("name", &name), ("e", tl!("the file is too large"))]);
                continue;
            }
            self.import_chess_pgn(&name, &bytes);
        }
    }

    /// Add the games of the PGN file `name` (its `bytes`) to the games of the document shown,
    /// the first of them shown. Returns how many were added. The user is told how many, and
    /// about the ones that couldn't be read or didn't fit.
    pub fn import_chess_pgn(&mut self, name: &str, bytes: &[u8]) -> usize {
        self.sync_chess_docs();
        let text = String::from_utf8_lossy(bytes);
        let chunks = split_pgn(&text, chess_autosave::MAX_GAMES);
        let more = chunks.len() > chess_autosave::MAX_GAMES;
        let (boards, failed) = pgn_boards(chunks.iter().take(chess_autosave::MAX_GAMES).copied());
        let added = boards.len();
        if added == 0 {
            self.notify_fmt("Couldn't read {name}: {e}", &[("name", name), ("e", tl!("no game could be read"))]);
            return 0;
        }
        self.chess.games.collection_mut().add_games(boards, ChessEdit::LoadPgn, true);
        self.chess.study.setup = None;
        self.chess.enter_scope();
        let mut note = crate::i18n::fmt(tl!("Games read from {name}: {n}"), &[("n", &added.to_string()), ("name", name)]);
        if failed > 0 {
            note.push_str(" · ");
            note.push_str(&crate::i18n::fmt(tl!("Not read: {n}"), &[("n", &failed.to_string())]));
        }
        if more {
            note.push_str(" · ");
            note.push_str(&crate::i18n::fmt(tl!("Only the first {n} were read."), &[("n", &chess_autosave::MAX_GAMES.to_string())]));
        }
        self.notify(note);
        added
    }
}

impl PdfCraftApp {
    /// The PGN field's games after its first (`texts`; `more`: the field had more than the
    /// cap), its first just loaded on the board: added to the document's games after the others,
    /// the game shown staying shown, as one undo step of the list ("Load the PGN"). The user is
    /// told how many, and about the ones that couldn't be read or didn't fit.
    pub(crate) fn add_pgn_field_games(&mut self, texts: &[String], more: bool) {
        self.sync_chess_docs();
        let (boards, failed) = pgn_boards(texts.iter().map(String::as_str));
        let added = self.chess.games.collection_mut().add_games(boards, ChessEdit::LoadPgn, false);
        self.chess.enter_scope();
        let count = u64::try_from(added).unwrap_or(u64::MAX);
        let mut note = crate::i18n::trn(
            crate::i18n::current(),
            count,
            "{n} more game was added to the list of games.",
            "{n} more games were added to the list of games.",
        );
        if failed > 0 {
            note.push_str(" · ");
            note.push_str(&crate::i18n::fmt(tl!("Not read: {n}"), &[("n", &failed.to_string())]));
        }
        if more {
            note.push_str(" · ");
            note.push_str(&crate::i18n::fmt(tl!("Only the first {n} were read."), &[("n", &chess_autosave::MAX_GAMES.to_string())]));
        }
        self.notify(note);
    }
}

/// The games of PGN texts (one game each), each on a board of its own, and how many couldn't be
/// read. A game whose reading stopped at a move that can't be played says so on its board.
fn pgn_boards<'a>(texts: impl Iterator<Item = &'a str>) -> (Vec<BoardState>, usize) {
    let mut boards = Vec::new();
    let mut failed = 0usize;
    for text in texts {
        match Game::from_pgn(text) {
            Ok((game, stopped)) => {
                let mut board = BoardState::default();
                board.game = game;
                board.message = stopped.map(|m| crate::i18n::fmt(tl!("Reading stopped at {move}: it is not a legal move there."), &[("move", &m)]));
                board.refresh_fields(true);
                boards.push(board);
            }
            Err(_) => failed = failed.saturating_add(1),
        }
    }
    (boards, failed)
}

/// A file's bytes, refused above [`MAX_PGN_FILE_BYTES`].
#[cfg(not(target_arch = "wasm32"))]
fn read_capped(path: &std::path::Path) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let len = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if len > MAX_PGN_FILE_BYTES {
        return Err(tl!("the file is too large").to_string());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path).and_then(|f| f.take(MAX_PGN_FILE_BYTES).read_to_end(&mut bytes)).map_err(|e| e.to_string())?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board_with(moves: &[&str]) -> BoardState {
        let mut b = BoardState::default();
        for m in moves {
            b.game.play_uci(m).unwrap();
        }
        b
    }

    fn names(c: &ChessCollection) -> Vec<String> {
        c.iter().map(|e| e.board.game.movetext()).collect()
    }

    #[test]
    fn switching_keeps_the_order_and_every_game() {
        let mut c = ChessCollection::new(DocKeys::default(), "c".into());
        c.add(GameEntry::new(board_with(&["e2e4"])));
        c.add(GameEntry::new(board_with(&["d2d4"])));
        c.add(GameEntry::new(board_with(&["c2c4"])));
        // The blank first game gave way to the first one added.
        assert_eq!(names(&c), ["1. e4", "1. d4", "1. c4"]);
        assert_eq!(c.active_index(), 2);
        for (to, shown) in [(0, "1. e4"), (1, "1. d4"), (2, "1. c4"), (0, "1. e4"), (2, "1. c4"), (1, "1. d4")] {
            assert!(c.switch_to(to));
            assert_eq!(c.active_index(), to);
            assert_eq!(c.active().board.game.movetext(), shown);
            assert_eq!(names(&c), ["1. e4", "1. d4", "1. c4"], "after switching to {to}");
        }
        assert!(!c.switch_to(3));
        assert!(!c.switch_to(usize::MAX));
        assert_eq!(names(&c), ["1. e4", "1. d4", "1. c4"]);
    }

    #[test]
    fn moving_duplicating_and_deleting_keep_the_order_and_the_game_shown() {
        let mut c = ChessCollection::new(DocKeys::default(), "c".into());
        for m in ["e2e4", "d2d4", "c2c4"] {
            c.add(GameEntry::new(board_with(&[m])));
        }
        assert!(c.switch_to(1));
        assert!(c.move_game(1, true));
        assert_eq!((names(&c), c.active_index()), (vec!["1. d4".to_owned(), "1. e4".into(), "1. c4".into()], 0), "the game shown moved with it");
        for (index, up) in [(0, true), (2, false), (3, true), (usize::MAX, false), (usize::MAX, true)] {
            assert!(!c.move_game(index, up), "{index} {up}");
        }
        assert!(c.move_game(2, true));
        assert_eq!((names(&c), c.active_index()), (vec!["1. d4".to_owned(), "1. c4".into(), "1. e4".into()], 0));
        assert!(c.duplicate(1));
        assert_eq!(
            (names(&c), c.active_index()),
            (vec!["1. d4".to_owned(), "1. c4".into(), "1. c4".into(), "1. e4".into()], 2),
            "the copy after it, shown"
        );
        let uids: HashSet<u64> = c.iter().map(|e| e.uid).collect();
        assert_eq!(uids.len(), 4, "the copy is a game of its own");
        assert!(!c.duplicate(4));
        assert!(c.delete(0));
        assert_eq!((names(&c), c.active_index()), (vec!["1. c4".to_owned(), "1. c4".into(), "1. e4".into()], 1), "the game shown stays shown");
        assert!(c.delete(1));
        assert_eq!((names(&c), c.active_index()), (vec!["1. c4".to_owned(), "1. e4".into()], 1), "the game after it is shown");
        assert!(c.delete(1));
        assert_eq!((names(&c), c.active_index()), (vec!["1. c4".to_owned()], 0), "at the end, the one before");
        assert!(!c.delete(1) && !c.delete(usize::MAX));
        assert!(c.delete(0));
        assert_eq!((names(&c), c.active_index()), (vec![String::new()], 0));
        assert!(c.active().is_blank(), "deleting the last game leaves a blank one");
    }

    #[test]
    fn renaming_sets_the_names_and_the_other_tags_in_one_undo_step() {
        let mut e = GameEntry::new(board_with(&["e2e4"]));
        let names = GameNames { white: " Werle ".into(), black: "Jianu".into(), event: "Wijk aan Zee".into(), ..GameNames::default() };
        assert!(e.rename(&names));
        assert_eq!(e.name(1), "Werle–Jianu");
        assert_eq!(e.names(), GameNames { white: "Werle".into(), black: "Jianu".into(), event: "Wijk aan Zee".into(), ..GameNames::default() });
        assert_eq!(e.board.undo_label().as_deref(), Some("Edit game details"));
        assert!(!e.rename(&names), "the same names change nothing");
        assert!(e.board.undo());
        assert_eq!(e.name(1), "Game 1", "one step took all three back");
        assert!(e.rename(&GameNames { event: "Training".into(), ..GameNames::default() }));
        assert_eq!(e.name(1), "Training", "named by its event without players");
        assert!(e.rename(&GameNames::default()));
        assert!(e.board.game.tags().is_empty(), "empty names remove the tags");
        // Site, Date and Round, from the list, with the names: one step for all of them.
        let placed = GameNames {
            white: "Werle".into(),
            site: "Wijk aan Zee NED".into(),
            date: "2025.01.??".into(),
            round: "6".into(),
            ..GameNames::default()
        };
        assert!(e.rename(&placed));
        let game = &e.board.game;
        assert_eq!(
            (game.tag("White"), game.tag("Site"), game.tag("Date"), game.tag("Round")),
            (Some("Werle"), Some("Wijk aan Zee NED"), Some("2025.01.??"), Some("6"))
        );
        assert_eq!(e.names(), placed, "the editor opens with what the game has");
        assert!(e.board.undo());
        assert!(e.board.game.tags().is_empty(), "one step took all four back");
    }

    /// Several games deleted at once are one step of the list: Undo puts each back where it was
    /// and shows the game shown before; the game after the last one deleted is shown meanwhile.
    #[test]
    fn several_games_deleted_are_one_step_and_come_back_where_they_were() {
        let mut games = ChessGames::default();
        let c = games.collection_mut();
        for m in ["e2e4", "d2d4", "c2c4", "g1f3"] {
            c.add(GameEntry::new(board_with(&[m])));
        }
        let all = uids(c);
        assert!(c.switch_to(1));
        assert_eq!(c.delete_many(&[3, 1, 1, 99]), 2, "places that are no game, or twice, count once or not at all");
        assert_eq!((names(c), c.active_index()), (vec!["1. e4".to_owned(), "1. c4".into()], 1), "the game after the shown one is shown");
        assert_eq!(games.step_text(true).as_deref(), Some("Undo Delete game"));
        assert!(games.history_step(true).is_some());
        let c = games.collection();
        assert_eq!((uids(c), c.active_index()), (all.clone(), 1), "each back at its place, the game shown before shown");
        assert_eq!(c.active().board.game.movetext(), "1. d4");
        assert!(games.history_step(false).is_some());
        assert_eq!(names(games.collection()), ["1. e4", "1. c4"]);
        assert!(games.history_step(true).is_some());
        // Every game at once: a blank one is left, and Undo puts them all back in its place.
        let c = games.collection_mut();
        assert_eq!(c.delete_many(&[0, 1, 2, 3]), 4);
        assert!(c.len() == 1 && c.active().is_blank());
        assert_eq!(c.delete_many(&[]), 0);
        assert!(games.history_step(true).is_some());
        assert_eq!(uids(games.collection()), all);
        assert!(!games.collection().iter().any(GameEntry::is_blank), "the blank game gave way");
    }

    /// A selection moves up or down a place as a block, each game keeping its order among them,
    /// and dragged into a gap together; each is one step of the list.
    #[test]
    fn a_selection_moves_as_a_block_keeping_its_order() {
        let mut games = ChessGames::default();
        let c = games.collection_mut();
        for m in ["a2a3", "b2b3", "c2c3", "d2d3", "e2e3"] {
            c.add(GameEntry::new(board_with(&[m])));
        }
        let first = |c: &ChessCollection| c.iter().map(|e| e.board.game.movetext().chars().nth(3).unwrap_or('-')).collect::<String>();
        assert!(c.switch_to(3));
        assert!(c.move_many(&[1, 3], true));
        assert_eq!((first(c), c.active_index()), ("badce".to_owned(), 2), "the game shown moved with them");
        assert!(!c.move_many(&[0, 2], true), "the first of them is first");
        assert!(!c.move_many(&[3, 4], false), "the last is last");
        assert!(!c.move_many(&[], true) && !c.move_many(&[usize::MAX], false));
        assert!(c.move_many(&[0, 1], false));
        assert_eq!(first(c), "dbace");
        assert_eq!(games.step_text(true).as_deref(), Some("Undo Move game"));
        assert!(games.history_step(true).is_some());
        assert_eq!(first(games.collection()), "badce");
        assert!(games.history_step(false).is_some());
        assert_eq!(first(games.collection()), "dbace");
        // Dragged: b and c (places 1 and 3) into the gap after e, then before d.
        let c = games.collection_mut();
        assert!(c.move_many_to(&[1, 3], 5));
        assert_eq!(first(c), "daebc");
        assert!(c.move_many_to(&[3, 4], 0));
        assert_eq!(first(c), "bcdae");
        assert!(!c.move_many_to(&[0, 1], 1), "dropped next to themselves, they stay");
        assert!(!c.move_many_to(&[0, 1], 0));
        assert!(games.history_step(true).is_some());
        assert_eq!(first(games.collection()), "daebc");
        assert!(games.history_step(true).is_some());
        assert_eq!(first(games.collection()), "dbace");
    }

    #[test]
    fn a_block_goes_into_the_gap_it_was_dropped_in() {
        let order = [10, 11, 12, 13, 14];
        assert_eq!(block_order(&order, &[1, 3], 0), [11, 13, 10, 12, 14]);
        assert_eq!(block_order(&order, &[1, 3], 5), [10, 12, 14, 11, 13]);
        assert_eq!(block_order(&order, &[1, 3], usize::MAX), [10, 12, 14, 11, 13]);
        assert_eq!(block_order(&order, &[1, 3], 3), [10, 12, 11, 13, 14], "a gap among them: they close up there");
        assert_eq!(block_order(&order, &[0, 1], 2), order, "next to themselves: nothing moves");
        assert_eq!(block_order(&order, &[], 2), order);
        assert_eq!(block_order(&[], &[0], 0), Vec::<u64>::new());
    }

    /// Duplicating a selection copies each game right after it, the first copy shown, as one step
    /// of the list that Undo takes back whole.
    #[test]
    fn duplicating_several_copies_each_after_it_in_one_step() {
        let mut games = ChessGames::default();
        let c = games.collection_mut();
        for m in ["e2e4", "d2d4", "c2c4"] {
            c.add(GameEntry::new(board_with(&[m])));
        }
        let before = uids(c);
        assert_eq!(c.duplicate_many(&[2, 0]), 2);
        assert_eq!((names(c), c.active_index()), (vec!["1. e4".to_owned(), "1. e4".into(), "1. d4".into(), "1. c4".into(), "1. c4".into()], 1));
        assert_eq!(uids(c).iter().collect::<HashSet<_>>().len(), 5, "each copy a game of its own");
        assert_eq!(c.duplicate_many(&[usize::MAX]), 0);
        assert_eq!(games.step_text(true).as_deref(), Some("Undo Duplicate game"));
        assert!(games.history_step(true).is_some());
        let c = games.collection();
        assert_eq!((uids(c), c.active_index()), (before, 2), "both copies out, the game shown before shown");
        assert!(!games.can_step(true));
    }

    /// A plain click picks one game, Ctrl+click adds or takes one out (starting from the game
    /// shown), Shift+click picks the games from the anchor, Ctrl+A all; games deleted count for
    /// nothing, and with none picked the selection is the game shown.
    #[test]
    fn the_selection_follows_clicks_ctrl_shift_and_select_all() {
        let mut c = ChessCollection::new(DocKeys::default(), "c".into());
        for m in ["a2a3", "b2b3", "c2c3", "d2d3", "e2e3"] {
            c.add(GameEntry::new(board_with(&[m])));
        }
        assert!(c.switch_to(1));
        let u = uids(&c);
        let mut s = Selection::default();
        assert_eq!(s.places(&c), [1], "nothing picked: the game shown");
        assert!(s.contains(&c, u[1]) && !s.contains(&c, u[0]));
        s.pick(&c, u[3], Pick::Toggle);
        assert_eq!(s.places(&c), [1, 3], "Ctrl+click adds to the game shown");
        assert_eq!(s.targets(&c, 3, u[3]), [1, 3], "a row selected acts with the others");
        assert_eq!(s.targets(&c, 4, u[4]), [4], "a row not selected acts alone");
        s.pick(&c, u[1], Pick::Toggle);
        assert_eq!(s.places(&c), [3], "and takes one out");
        assert!(!s.contains(&c, u[1]), "the game shown is not selected then");
        assert_eq!(s.targets(&c, 3, u[3]), [3]);
        s.pick(&c, u[0], Pick::Range);
        assert_eq!(s.places(&c), [0, 1], "Shift+click: from the anchor, the game last clicked");
        s.pick(&c, u[4], Pick::Range);
        assert_eq!(s.places(&c), [1, 2, 3, 4], "the anchor stays where it was");
        s.pick(&c, u[2], Pick::All);
        assert_eq!(s.places(&c), [0, 1, 2, 3, 4]);
        s.pick(&c, u[2], Pick::Only);
        assert_eq!(s.places(&c), [1], "a plain click: that game alone, which the list then shows");
        // Shift+click with no anchor: from the game shown.
        let mut s = Selection::default();
        s.pick(&c, u[3], Pick::Range);
        assert_eq!(s.places(&c), [1, 2, 3]);
        // Games deleted are selected no more; none left: the game shown.
        assert_eq!(c.delete_many(&[1, 2, 3]), 3);
        assert_eq!(s.places(&c), [c.active_index()]);
        s.pick(&c, u64::MAX, Pick::Range);
        assert_eq!(s.places(&c), [c.active_index()], "a game that is not there picks nothing");
    }

    #[test]
    fn the_rows_keys_say_which_modifiers_they_want() {
        assert_eq!(list_key(Key::Delete, Modifiers::NONE), Some(Modifiers::NONE));
        assert_eq!(list_key(Key::Delete, Modifiers::SHIFT), None);
        assert_eq!(list_key(Key::ArrowUp, Modifiers::ALT), Some(Modifiers::ALT));
        assert_eq!(list_key(Key::ArrowDown, Modifiers::SHIFT), Some(Modifiers::SHIFT));
        assert_eq!(list_key(Key::ArrowDown, Modifiers::NONE), None, "plain arrows are not the list's");
        // Ctrl on Windows and Linux, ⌘ on a Mac: both are the command key.
        assert_eq!(list_key(Key::A, Modifiers::CTRL | Modifiers::COMMAND), Some(Modifiers::COMMAND));
        assert_eq!(list_key(Key::A, Modifiers::MAC_CMD | Modifiers::COMMAND), Some(Modifiers::COMMAND));
        assert_eq!(list_key(Key::Space, Modifiers::COMMAND), Some(Modifiers::COMMAND));
        assert_eq!(list_key(Key::A, Modifiers::NONE), None, "a letter typed is not the list's");
        assert_eq!(list_key(Key::A, Modifiers::COMMAND | Modifiers::SHIFT), None);
    }

    #[test]
    fn a_new_game_from_here_starts_at_the_position_and_refuses_an_illegal_one() {
        let mut c = ChessCollection::new(DocKeys::default(), "c".into());
        c.active_mut().board = board_with(&["e2e4", "e7e5"]);
        c.active_mut().board.source = Some(Source { doc: DocId(3), page: 1, index: 0 });
        let after_e4 = c.active().board.game.main_line()[1];
        assert!(c.new_from(None).is_ok());
        assert_eq!(c.active_index(), 1);
        assert_eq!(c.active().board.game.start_fen(), c.get(0).unwrap().board.game.fen());
        assert_eq!(c.active().origin(), Some(Source { doc: DocId(3), page: 1, index: 0 }));
        assert!(c.switch_to(0));
        assert!(c.new_from(Some(after_e4)).is_ok());
        assert_eq!(c.active().board.game.start_fen(), c.get(0).unwrap().board.game.fen_at(after_e4).unwrap());
        assert_eq!(names(&c), ["1. e4 e5", "", ""], "after the game it came from");
        // A move the game doesn't have: the move shown.
        assert!(c.switch_to(0));
        assert!(c.new_from(Some(usize::MAX)).is_ok());
        assert_eq!(c.active().board.game.start_fen(), c.get(0).unwrap().board.game.fen());
        let placement = c.active().board.placement();
        c.active_mut().board.illegal = Some((placement, "8/8/8/8/8/8/8/8 w - - 0 1".into()));
        let before = c.len();
        assert!(c.new_from(None).is_err());
        assert_eq!(c.len(), before);
    }

    #[test]
    fn a_position_set_up_is_a_new_game_and_the_game_shown_keeps_its_moves() {
        let mut c = ChessCollection::new(DocKeys::default(), "c".into());
        c.active_mut().board = board_with(&["e2e4", "e7e5"]);
        c.active_mut().board.flipped = true;
        let fen = "4k3/8/8/8/8/8/4P3/4K3 b - - 0 40";
        assert!(c.new_at(fen).is_ok());
        assert_eq!((c.len(), c.active_index()), (2, 1));
        assert_eq!(c.active().board.game.start_fen(), fen);
        assert!(c.active().board.flipped, "on the side of the game it came from");
        assert_eq!(c.active().origin(), None, "a position set up comes from no diagram");
        assert_eq!(c.get(0).unwrap().board.game.movetext(), "1. e4 e5");
        assert!(c.new_at("not a fen").is_err());
        assert_eq!(c.len(), 2);
    }

    #[test]
    fn a_game_is_named_by_its_players_its_diagram_or_its_place() {
        let mut e = GameEntry::new(BoardState::default());
        assert_eq!(e.name(3), "Game 3");
        e.board.source = Some(Source { doc: DocId(1), page: 16, index: 2 });
        assert_eq!(e.name(3), "Diagram 3, p. 17");
        e.board.game.set_tag("White", "?").unwrap();
        assert_eq!(e.name(3), "Diagram 3, p. 17", "an unknown player is no name");
        e.board.game.set_tag("White", "Werle").unwrap();
        e.board.game.set_tag("Black", "Jianu").unwrap();
        assert_eq!(e.name(3), "Werle–Jianu");
        e.board.game.play_uci("e2e4").unwrap();
        e.board.game.play_uci("e7e5").unwrap();
        e.board.game.play_uci("g1f3").unwrap();
        assert_eq!(e.moves(), 2);
    }

    /// Play `uci` on the game shown, as one undo step of the game.
    fn play(c: &mut ChessCollection, uci: &str) {
        let edited = c.active_mut().board.edit(|s| s.game.play_uci(uci).ok().map(|_| ChessEdit::Play(uci.to_owned())));
        assert!(edited, "{uci}");
    }

    /// Wave 10 U2: a delete and a move are steps of the list that Undo takes back, the newer first
    /// against the game shown's own changes; a deleted game comes back where it was, with its
    /// history, shown if it was; the blank game the last delete left gives way to it again.
    #[test]
    fn undo_takes_back_deletes_and_moves_the_newer_step_first() {
        let mut games = ChessGames::default();
        let c = games.collection_mut();
        for m in ["e2e4", "d2d4", "c2c4"] {
            c.add(GameEntry::new(board_with(&[m])));
        }
        assert!(c.switch_to(1));
        play(c, "d7d5");
        assert!(c.delete(2));
        assert_eq!(names(games.collection()), ["1. e4", "1. d4 d5"]);
        // The delete is newer than the move played on the game shown: Undo takes it first.
        assert_eq!(games.step_text(true).as_deref(), Some("Undo Delete game"));
        assert_eq!(games.history_step(true).as_deref(), Some("Delete game"));
        assert_eq!((names(games.collection()), games.collection().active_index()), (vec!["1. e4".to_owned(), "1. d4 d5".into(), "1. c4".into()], 1));
        assert_eq!(games.step_edit(true), Some(ChessEdit::Play("d7d5".into())), "then the game's own change");
        assert_eq!(games.history_step(true).as_deref(), Some("Play d7d5"));
        assert_eq!(names(games.collection()), ["1. e4", "1. d4", "1. c4"]);
        assert!(!games.can_step(true));
        // Redo in the reverse order: the move (undone last), then the delete.
        assert_eq!(games.history_step(false).as_deref(), Some("Play d7d5"));
        assert_eq!(games.history_step(false).as_deref(), Some("Delete game"));
        assert_eq!(names(games.collection()), ["1. e4", "1. d4 d5"]);
        assert!(!games.can_step(false));
        // The game shown deleted: the one after it is shown; Undo shows it again, with its history.
        let c = games.collection_mut();
        assert!(c.switch_to(1) && c.delete(1));
        assert_eq!((names(c), c.active_index()), (vec!["1. e4".to_owned()], 0));
        assert!(games.history_step(true).is_some());
        let c = games.collection();
        assert_eq!((names(c), c.active_index()), (vec!["1. e4".to_owned(), "1. d4 d5".into()], 1));
        assert_eq!(c.active().board.undo_label().as_deref(), Some("Play d7d5"), "its own history came back with it");
        // A move, undone and redone.
        let c = games.collection_mut();
        assert!(c.move_to(0, 1));
        assert_eq!(names(c), ["1. d4 d5", "1. e4"]);
        assert_eq!(games.step_text(true).as_deref(), Some("Undo Move game"));
        assert!(games.history_step(true).is_some());
        assert_eq!(names(games.collection()), ["1. e4", "1. d4 d5"]);
        assert!(games.history_step(false).is_some());
        assert_eq!(names(games.collection()), ["1. d4 d5", "1. e4"]);
        // Every game deleted: a blank one is left, and Undo puts the last one back in its place.
        let c = games.collection_mut();
        assert!(c.delete(0) && c.delete(0));
        assert!(c.len() == 1 && c.active().is_blank());
        assert!(games.history_step(true).is_some());
        assert_eq!(names(games.collection()), ["1. e4"], "the blank game gave way");
        assert!(games.history_step(true).is_some());
        assert_eq!(names(games.collection()), ["1. d4 d5", "1. e4"]);
    }

    #[test]
    fn the_lists_history_is_capped() {
        let mut c = ChessCollection::new(DocKeys::default(), "c".into());
        for m in ["e2e4", "d2d4"] {
            c.add(GameEntry::new(board_with(&[m])));
        }
        for _ in 0..chess_history::MAX_STEPS + 50 {
            assert!(c.move_game(0, false));
        }
        assert_eq!(c.history.past.len(), chess_history::MAX_STEPS, "the oldest steps go");
        assert!(c.list_step(true).is_some());
        assert_eq!((c.history.past.len(), c.history.future.len()), (chess_history::MAX_STEPS - 1, 1));
        // A step whose game is gone does nothing.
        c.history.past.push(ListStep::new(ListChange::Moved { uid: u64::MAX, from: 0, to: 1 }));
        assert_eq!(c.list_step(true), None);
        let order = names(&c);
        c.history.past.push(ListStep::new(ListChange::Added { uids: vec![u64::MAX], edit: ChessEdit::NewGame, before: 0, restored: None }));
        assert_eq!(c.list_step(true), None);
        assert_eq!(names(&c), order);
    }

    /// The uids of the games, in list order.
    fn uids(c: &ChessCollection) -> Vec<u64> {
        c.iter().map(|e| e.uid).collect()
    }

    /// New game and Duplicate are steps of the list: Undo takes the game back out and shows the
    /// game shown before; Redo puts the same game back. A game changed after it was added goes
    /// into the trash too (said once), and Redo takes it out of the trash with its history.
    #[test]
    fn undo_takes_back_new_and_duplicated_games_and_keeps_a_changed_one() {
        let mut games = ChessGames::default();
        let c = games.collection_mut();
        // A game put in the list as the tests set one up is no step of it.
        c.add(GameEntry::new(board_with(&["e2e4"])));
        assert!(!games.can_step(true));
        let c = games.collection_mut();
        c.new_game();
        let blank = c.active().uid;
        assert_eq!((names(c), c.active_index()), (vec!["1. e4".to_owned(), String::new()], 1));
        assert_eq!(games.step_text(true).as_deref(), Some("Undo New game"));
        assert_eq!(games.history_step(true).as_deref(), Some("New game"));
        assert_eq!((names(games.collection()), games.collection().active_index()), (vec!["1. e4".to_owned()], 0));
        assert!(games.collection().trash().is_empty(), "an untouched game is not kept");
        assert_eq!(games.history_step(false).as_deref(), Some("New game"));
        assert_eq!(games.collection().active().uid, blank, "the same game is back, shown");
        // A copy changed while it was shown, then the original shown: Undo is the list's.
        let c = games.collection_mut();
        assert!(c.switch_to(0) && c.duplicate(0));
        let copy = c.active().uid;
        play(c, "e7e5");
        assert!(c.switch_to(0));
        assert_eq!(games.step_text(true).as_deref(), Some("Undo Duplicate game"));
        assert_eq!(games.history_step(true).as_deref(), Some("Duplicate game"));
        let c = games.collection();
        assert_eq!((names(c), c.active_index()), (vec!["1. e4".to_owned(), String::new()], 0));
        let kept = c.trash().newest_first().next().map(|(_, t)| t.clone()).expect("the changed copy is in the trash");
        assert_eq!((kept.name.as_str(), kept.moves, kept.deleted), ("Game 2", 2, 2));
        assert!(kept.pgn.contains("1. e4 e5"), "{}", kept.pgn);
        assert_eq!(games.take_notes(), ["1 changed game was kept in Deleted moves."]);
        // Redo: the copy is back where it was, with its history, and out of the trash.
        assert_eq!(games.history_step(false).as_deref(), Some("Duplicate game"));
        let c = games.collection();
        assert_eq!(names(c), ["1. e4", "1. e4 e5", ""]);
        assert_eq!(c.get(1).map(|e| e.uid), Some(copy));
        assert_eq!(c.get(1).and_then(|e| e.board.undo_label()).as_deref(), Some("Play e7e5"));
        assert!(c.trash().is_empty(), "taken out of the trash again");
        assert!(games.take_notes().is_empty());
        // A new change to the list drops the redo step; the trash still has the changed game.
        assert!(games.history_step(true).is_some());
        games.collection_mut().new_game();
        assert!(!games.can_step(false));
        assert_eq!(games.collection().trash().len(), 1);
    }

    /// The games of a PGN are one step of the list: Undo takes them all out (a blank game they
    /// gave way to comes back), Redo puts them all back in order, the first shown.
    #[test]
    fn the_games_of_a_pgn_are_one_step_of_the_list() {
        let mut games = ChessGames::default();
        let c = games.collection_mut();
        let three = vec![board_with(&["e2e4"]), board_with(&["d2d4"]), board_with(&["c2c4"])];
        assert_eq!(c.add_games(three, ChessEdit::LoadPgn, true), 3);
        let added = uids(c);
        assert_eq!((names(c), c.active_index()), (vec!["1. e4".to_owned(), "1. d4".into(), "1. c4".into()], 0));
        assert_eq!(games.step_text(true).as_deref(), Some("Undo Load the PGN"));
        assert!(games.history_step(true).is_some());
        let c = games.collection();
        assert!(c.len() == 1 && c.active().is_blank(), "a blank game is left");
        assert!(games.history_step(false).is_some());
        let c = games.collection();
        assert_eq!((uids(c), c.active_index()), (added.clone(), 0), "the same games, in order, the first shown");
        // Added after the games there, the game shown before is shown again after Undo.
        let c = games.collection_mut();
        assert!(c.switch_to(1));
        assert_eq!(c.add_games(vec![board_with(&["g1f3"]), board_with(&["b2b3"])], ChessEdit::LoadPgn, true), 2);
        assert_eq!((names(c).len(), c.active_index()), (5, 3));
        assert_eq!(c.add_games(Vec::new(), ChessEdit::LoadPgn, true), 0);
        assert!(games.history_step(true).is_some());
        let c = games.collection();
        assert_eq!((uids(c), c.active_index()), (added, 1));
        assert!(games.history_step(false).is_some());
        assert_eq!(names(games.collection()), ["1. e4", "1. d4", "1. c4", "1. Nf3", "1. b3"]);
        // The PGN field's other games: added at the end, the game shown stays shown.
        let c = games.collection_mut();
        assert!(c.switch_to(0));
        assert_eq!(c.add_games(vec![board_with(&["a2a3"])], ChessEdit::LoadPgn, false), 1);
        assert_eq!((c.len(), c.active_index()), (6, 0));
        assert!(games.history_step(true).is_some());
        assert_eq!((games.collection().len(), games.collection().active_index()), (5, 0));
    }

    /// A game of the trash opened as a new game is a step of the list: Undo puts the item back at
    /// its place in the trash, Redo takes it out again. A position set up is a step too.
    #[test]
    fn undo_puts_a_restored_game_back_in_the_trash() {
        let mut games = ChessGames::default();
        let c = games.collection_mut();
        c.add(GameEntry::new(board_with(&["e2e4", "e7e5"])));
        let old = |moves: &[&str]| board_with(moves).game;
        c.trash.keep(&old(&["d2d4"]), "A", 1);
        c.trash.keep(&old(&["c2c4"]), "B", 1);
        let trashed = |c: &ChessCollection| c.trash().stored().into_iter().map(|t| t.name).collect::<Vec<_>>();
        assert!(c.restore_trashed(0).is_ok());
        assert_eq!((names(c), trashed(c)), (vec!["1. e4 e5".to_owned(), "1. d4".into()], vec!["B".to_owned()]));
        assert!(c.restore_trashed(7).is_err());
        assert_eq!(games.step_text(true).as_deref(), Some("Undo Open as a new game"));
        assert!(games.history_step(true).is_some());
        let c = games.collection();
        assert_eq!((names(c), trashed(c)), (vec!["1. e4 e5".to_owned()], vec!["A".to_owned(), "B".into()]), "back at its place");
        assert!(games.history_step(false).is_some());
        let c = games.collection();
        assert_eq!((names(c), trashed(c)), (vec!["1. e4 e5".to_owned(), "1. d4".into()], vec!["B".to_owned()]));
        // A position set up, as a new game.
        assert!(games.collection_mut().new_at("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1").is_ok());
        assert_eq!(games.step_text(true).as_deref(), Some("Undo New game from this position"));
        assert!(games.history_step(true).is_some());
        assert_eq!(games.collection().len(), 2);
    }

    #[test]
    fn a_dropped_row_goes_into_the_gap_it_was_dropped_in() {
        // Three games; game 0 dropped on the bottom half of row 2 goes last.
        assert_eq!(drop_place(0, 2, false), Some(2));
        assert_eq!(drop_place(0, 2, true), Some(1));
        assert_eq!(drop_place(2, 0, true), Some(0));
        assert_eq!(drop_place(2, 0, false), Some(1));
        // Onto itself, or the gap next to it: nowhere.
        assert_eq!(drop_place(1, 1, true), None);
        assert_eq!(drop_place(1, 1, false), None);
        assert_eq!(drop_place(1, 0, false), None);
        assert_eq!(drop_place(1, 2, true), None);
        assert_eq!(drop_place(usize::MAX, usize::MAX, false), None);
    }

    #[test]
    fn file_names_are_safe() {
        assert_eq!(file_stem("Werle–Jianu"), "Werle–Jianu");
        assert_eq!(file_stem("a/b:c?"), "a_b_c_");
        assert_eq!(file_stem("..."), "game");
        assert_eq!(file_stem(&"x".repeat(500)).len(), 100);
    }
}
