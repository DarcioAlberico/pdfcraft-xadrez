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
//!   Event tags, so the name is in its PGN; one undo step of that game), duplicated, deleted
//!   (after a question: the list has no undo; deleting the last game leaves a blank one) and
//!   moved up or down, from its menu, the buttons shown on the row under the pointer, or the keys
//!   of a focused row (F2, Delete, Alt+↑/↓; Enter shows it). "New game from this position" starts
//!   a game at the move shown, or at a move of the notation, keeping the diagram it came from.
//!   The list's order is the order the collection is exported and autosaved in.
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
use pdfcraft_engine::chess::pdfcraft_chess::game::{Game, NodeId, NodePath};

use crate::PdfCraftApp;
use crate::chess_autosave::{self, Blocked, Store};
use crate::chess_board_ui::{BoardState, Source};
use crate::chess_history::ChessEdit;
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
        let names = self.names();
        let known = |v: String| Some(v).filter(|v| !v.is_empty());
        match (known(names.white), known(names.black)) {
            (Some(w), Some(b)) => return format!("{w}–{b}"),
            (Some(one), None) | (None, Some(one)) => return one,
            (None, None) => {}
        }
        if let Some(event) = known(names.event) {
            return event;
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
        let tag = |t: &str| self.board.game.tag(t).map(str::trim).filter(|v| *v != "?").unwrap_or_default().to_owned();
        GameNames { white: tag("White"), black: tag("Black"), event: tag("Event") }
    }

    /// Name the game: its White, Black and Event tags take `names` (an empty name removes its
    /// tag), as one undo step of the game ("Edit game details"). Whether anything changed.
    fn rename(&mut self, names: &GameNames) -> bool {
        let now = self.names();
        let changes: Vec<(&str, String)> =
            [("White", &now.white, &names.white), ("Black", &now.black, &names.black), ("Event", &now.event, &names.event)]
                .into_iter()
                .filter(|(_, old, new)| old.trim() != new.trim())
                .map(|(tag, _, new)| (tag, new.trim().to_owned()))
                .collect();
        if changes.is_empty() {
            return false;
        }
        self.board.edit(|s| {
            for (tag, value) in &changes {
                // The three names are valid tag names; a game whose tags are full refuses a new
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
        self.board.game.main_line().len().saturating_sub(1).div_ceil(2)
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

/// What a game is named by in the list (its PGN tags).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GameNames {
    pub white: String,
    pub black: String,
    pub event: String,
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

/// A document's games, the active one shown on the board. Never empty: the active game is held
/// apart from the ones before and after it in the list.
pub struct ChessCollection {
    /// What names the document's file (see [`DocKeys`]); empty until it is known.
    pub keys: DocKeys,
    before: Vec<GameEntry>,
    active: GameEntry,
    after: Vec<GameEntry>,
    /// The collection's id in the store.
    cid: String,
    autosave: Autosave,
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
            before: Vec::new(),
            active: GameEntry::new(BoardState::default()),
            after: Vec::new(),
            cid,
            autosave: Autosave::default(),
        };
        c.settle();
        c
    }

    /// The games `boards`, `active` shown (the first when out of range); one blank game when
    /// there are none.
    fn from_boards(keys: DocKeys, cid: String, boards: Vec<BoardState>, active: usize) -> ChessCollection {
        let mut c = ChessCollection::new(keys, cid);
        let mut games = boards.into_iter().map(GameEntry::new);
        if let Some(first) = games.next() {
            c.active = first;
            c.after = games.collect();
            c.switch_to(active);
        }
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
        self.before.len().saturating_add(1).saturating_add(self.after.len())
    }

    /// Never empty (the active game is always there); here for completeness.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// The active game's place in the list (0-based).
    pub fn active_index(&self) -> usize {
        self.before.len()
    }

    /// The games, in list order.
    pub fn iter(&self) -> impl Iterator<Item = &GameEntry> {
        self.before.iter().chain(std::iter::once(&self.active)).chain(self.after.iter())
    }

    /// The game shown.
    pub fn active(&self) -> &GameEntry {
        &self.active
    }

    pub fn active_mut(&mut self) -> &mut GameEntry {
        &mut self.active
    }

    /// Show game `index` (0-based). Whether there is one.
    pub fn switch_to(&mut self, index: usize) -> bool {
        let at = self.active_index();
        if index >= self.len() || index == at {
            return index == at;
        }
        if index < at {
            // before = [..index] [index] [index+1..]: the tail and the active game go after.
            let mut tail = self.before.split_off(index);
            let Some(next) = (!tail.is_empty()).then(|| tail.remove(0)) else { return false };
            let old = std::mem::replace(&mut self.active, next);
            tail.push(old);
            tail.append(&mut self.after);
            self.after = tail;
        } else {
            let j = index.saturating_sub(at).saturating_sub(1);
            let mut rest = self.after.split_off(j);
            let Some(next) = (!rest.is_empty()).then(|| rest.remove(0)) else {
                self.after.append(&mut rest);
                return false;
            };
            let old = std::mem::replace(&mut self.active, next);
            self.before.push(old);
            self.before.append(&mut self.after);
            self.after = rest;
        }
        // Only the game shown needs its pictures; the others drop theirs until shown again.
        for e in self.before.iter_mut().chain(self.after.iter_mut()) {
            e.board.release_textures();
        }
        true
    }

    /// Game `index` (0-based).
    pub fn get(&self, index: usize) -> Option<&GameEntry> {
        self.iter().nth(index)
    }

    fn get_mut(&mut self, index: usize) -> Option<&mut GameEntry> {
        let at = self.active_index();
        match index.cmp(&at) {
            std::cmp::Ordering::Less => self.before.get_mut(index),
            std::cmp::Ordering::Equal => Some(&mut self.active),
            std::cmp::Ordering::Greater => self.after.get_mut(index.saturating_sub(at).saturating_sub(1)),
        }
    }

    /// The place of the game whose id is `uid`.
    fn position(&self, uid: u64) -> Option<usize> {
        self.iter().position(|e| e.uid == uid)
    }

    /// Change the list as one vector, `change` given the games in order and the place of the
    /// game shown (which it keeps pointing at the game to show). An empty list gets a blank game;
    /// a place past the end shows the last game.
    fn rearrange(&mut self, change: impl FnOnce(&mut Vec<GameEntry>, &mut usize)) {
        let mut at = self.active_index();
        // A stand-in while the games are one vector: replaced below, never shown or written.
        let shown = std::mem::replace(&mut self.active, GameEntry { board: BoardState::default(), uid: 0 });
        let mut all = std::mem::take(&mut self.before);
        all.push(shown);
        all.append(&mut self.after);
        change(&mut all, &mut at);
        if all.is_empty() {
            all.push(GameEntry::new(BoardState::default()));
        }
        let at = at.min(all.len().saturating_sub(1));
        self.after = all.split_off(at.saturating_add(1).min(all.len()));
        self.active = all.pop().unwrap_or_else(|| GameEntry::new(BoardState::default()));
        self.before = all;
        for e in self.before.iter_mut().chain(self.after.iter_mut()) {
            e.board.release_textures();
        }
    }

    /// Move game `index` one place up (towards the start) or down; the game shown stays shown.
    /// Whether it moved.
    pub(crate) fn move_game(&mut self, index: usize, up: bool) -> bool {
        let len = self.len();
        let other = if up { index.checked_sub(1) } else { index.checked_add(1) };
        let Some(other) = other.filter(|&j| j < len && index < len) else { return false };
        self.rearrange(|all, at| {
            // Both places are in the list: checked above, and `all` has `len` games.
            all.swap(index, other);
            if *at == index {
                *at = other;
            } else if *at == other {
                *at = index;
            }
        });
        true
    }

    /// Put `entry` right after game `index` and show it.
    fn insert_after(&mut self, index: usize, entry: GameEntry) {
        self.rearrange(|all, at| {
            let place = index.saturating_add(1).min(all.len());
            all.insert(place, entry);
            *at = place;
        });
    }

    /// A copy of game `index`, right after it, shown. Whether there is such a game.
    pub(crate) fn duplicate(&mut self, index: usize) -> bool {
        let Some(copy) = self.get(index).map(GameEntry::copy) else { return false };
        self.insert_after(index, copy);
        true
    }

    /// Delete game `index`. The game shown stays shown; when it is the one deleted, the game
    /// after it is shown (the one before, at the end of the list), and deleting the last game
    /// leaves a blank one. Whether there was such a game.
    pub(crate) fn delete(&mut self, index: usize) -> bool {
        if index >= self.len() {
            return false;
        }
        self.rearrange(|all, at| {
            if index < all.len() {
                all.remove(index);
            }
            if index < *at {
                *at = at.saturating_sub(1);
            } else if index == *at {
                *at = index.min(all.len().saturating_sub(1));
            }
        });
        true
    }

    /// A new game starting at the position of move `node` of the game shown (the move shown when
    /// `None`, or when the game has no such move), on the same side and from the same diagram,
    /// right after it and shown. Its PGN gets the position as its `SetUp`/`FEN` tags. Refused for
    /// an illegal position, which can't start a game; the error says why.
    pub(crate) fn new_from(&mut self, node: Option<NodeId>) -> Result<(), String> {
        let b = &self.active.board;
        if b.illegal.is_some() {
            return Err(tl!("An illegal position can't start a game: correct it in the diagram editor first.").to_string());
        }
        let fen = node.and_then(|n| b.game.fen_at(n)).unwrap_or_else(|| b.game.fen());
        let game = Game::from_fen(&fen).map_err(|e| crate::i18n::fmt(tl!("The FEN couldn't be read: {e}"), &[("e", &e.to_string())]))?;
        let mut board = BoardState::default();
        board.game = game;
        board.source = b.source;
        board.flipped = b.flipped;
        board.refresh_fields(true);
        self.insert_after(self.active_index(), GameEntry::new(board));
        Ok(())
    }

    /// Add `entry` at the end and show it. A blank game alone in the collection gives way to it.
    pub(crate) fn add(&mut self, entry: GameEntry) {
        if self.len() == 1 && self.active.is_blank() {
            self.active = entry;
            return;
        }
        let old = std::mem::replace(&mut self.active, entry);
        self.before.push(old);
        self.before.append(&mut self.after);
    }

    /// What the store keeps of the collection, summed up: the games, their changes, the one shown
    /// and its move. Changes when anything to keep changes.
    fn fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.active_index().hash(&mut h);
        self.active.board.game.current().hash(&mut h);
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
        self.len() > 1 || !self.active.is_blank()
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

    /// The board shown: the active game of the document shown.
    pub fn board(&self) -> &BoardState {
        &self.collection().active.board
    }

    pub fn board_mut(&mut self) -> &mut BoardState {
        &mut self.collection_mut().active.board
    }

    /// The game shown (its id) and how many changes it had: Undo's scope follows one game.
    pub(crate) fn scope_stamp(&self) -> (u64, u64) {
        let active = &self.collection().active;
        (active.uid, active.board.history.edits())
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

    /// Take the keys of a focused row of the list out of `events` (Delete, Backspace, F2, Alt+↑,
    /// Alt+↓), so nothing else sees them: Delete there deletes no variation, page or annotation.
    /// Only presses are taken, as for the chess key scope.
    pub(crate) fn take_list_keys(&mut self, events: &mut Vec<Event>) {
        let keys = &mut self.list.keys;
        events.retain(|event| match event {
            Event::Key { key, pressed: true, modifiers, .. } if list_key(*key, *modifiers) => {
                if keys.len() < MAX_LIST_KEYS {
                    keys.push((*key, *modifiers));
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
                    c.autosave.stored = true;
                    restored = Some((c, lost));
                }
            }
        }
        let mut c = match (restored, pending) {
            (Some((mut c, lost)), _) => {
                // The diagrams the games came from are in this document now.
                for e in c.before.iter_mut().chain(std::iter::once(&mut c.active)).chain(c.after.iter_mut()) {
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
        match self.store.write(&c.cid, &keys, c.active_index(), &boards) {
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
    /// A new game from the position of a move of the game shown (`None`: the move shown).
    NewFrom(Option<NodeId>),
}

/// Most list keys kept between two frames.
const MAX_LIST_KEYS: usize = 16;

/// The most height the list of games takes before it scrolls.
const LIST_HEIGHT: f32 = 180.0;

/// The size of the buttons on a row under the pointer.
const ROW_BUTTON: f32 = 20.0;

/// Whether a focused row of the list acts on `key` pressed with `m`.
fn list_key(key: Key, m: Modifiers) -> bool {
    match key {
        Key::Delete | Key::Backspace | Key::F2 => m.is_none(),
        Key::ArrowUp | Key::ArrowDown => m == Modifiers::ALT,
        _ => false,
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
}

impl ListState {
    /// Rename (or ask about deleting) game `uid`, named `names` now. `focus`: give the editor or
    /// the question the keyboard; `refocus`: give it back to the row afterwards.
    fn ask(&mut self, uid: u64, names: GameNames, rename: bool, focus: bool, refocus: bool) {
        if rename {
            self.renaming = Some(Renaming { uid, names, focus, refocus, reveal: true });
            self.deleting = None;
        } else {
            self.deleting = Some(Deleting { uid, focus, reveal: true });
            self.renaming = None;
        }
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

/// What a game's menu (or its row's buttons) chose.
enum RowChoice {
    Rename,
    Duplicate,
    Up,
    Down,
    Delete,
}

/// The menu of game `index` of `len`.
fn row_menu(ui: &mut egui::Ui, index: usize, len: usize) -> Option<RowChoice> {
    let mut choice = None;
    if ui.button(tl!("Rename…")).clicked() {
        choice = Some(RowChoice::Rename);
    }
    if ui.button(tl!("Duplicate")).clicked() {
        choice = Some(RowChoice::Duplicate);
    }
    if ui.add_enabled(index > 0, egui::Button::new(tl!("Move up"))).clicked() {
        choice = Some(RowChoice::Up);
    }
    if ui.add_enabled(index.saturating_add(1) < len, egui::Button::new(tl!("Move down"))).clicked() {
        choice = Some(RowChoice::Down);
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
        });
    });
    action
}

/// Game `i` of `c` in the list: its name (a click or Enter shows it), its menu, the keys of the
/// focused row and the buttons of the row under the pointer.
fn game_row(ui: &mut egui::Ui, list: &mut ListState, c: &ChessCollection, i: usize, e: &GameEntry) -> Option<GamesAction> {
    let mut action = None;
    let (uid, len, active) = (e.uid, c.len(), i == c.active_index());
    let name = e.name(i.saturating_add(1));
    // The row's buttons show while the pointer is on the row (as the last frame left it) or the
    // keyboard is on the row or its buttons.
    let lit_id = ui.id().with("chess-game-row");
    let lit = ui.ctx().data(|d| d.get_temp::<bool>(lit_id)).unwrap_or(false);
    let mut chosen = None;
    let mut focused = false;
    let mut by_keys = false;
    let row = ui.horizontal(|ui| {
        let r = ui.selectable_label(active, name.as_str());
        if list.focus_row.is_some_and(|(u, asked)| u == uid && ui.ctx().cumulative_pass_nr() > asked) {
            r.request_focus();
            list.reveal(ui, r.rect);
            list.focus_row = None;
        }
        if r.clicked() && !active {
            action = Some(GamesAction::Switch(i));
        }
        r.context_menu(|ui| {
            chosen = row_menu(ui, i, len);
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
    let lit_now = ui.rect_contains_pointer(row.response.rect) || focused;
    if lit_now != lit {
        ui.ctx().data_mut(|d| d.insert_temp(lit_id, lit_now));
        ui.ctx().request_repaint();
    }
    match chosen {
        Some(RowChoice::Rename) => list.ask(uid, e.names(), true, true, by_keys),
        Some(RowChoice::Delete) => list.ask(uid, e.names(), false, by_keys, false),
        Some(RowChoice::Duplicate) => action = Some(GamesAction::Duplicate(i)),
        Some(RowChoice::Up) if i > 0 => {
            action = Some(GamesAction::Move(i, true));
            list.focus_later(ui.ctx(), by_keys.then_some(uid));
        }
        Some(RowChoice::Down) if i.saturating_add(1) < len => {
            action = Some(GamesAction::Move(i, false));
            list.focus_later(ui.ctx(), by_keys.then_some(uid));
        }
        Some(RowChoice::Up | RowChoice::Down) | None => {}
    }
    action
}

/// The question about deleting a game, or its name editor, when one is open: under the list
/// (not inside its scroll area, which would clip it and keep the panel from scrolling to it),
/// headed by the game's name.
fn pending(ui: &mut egui::Ui, t: &Tokens, list: &mut ListState, c: &ChessCollection) -> Option<GamesAction> {
    let deleting = list.deleting.as_ref().and_then(|d| c.position(d.uid));
    let renaming = list.renaming.as_ref().and_then(|r| c.position(r.uid));
    let i = deleting.or(renaming)?;
    let name = c.get(i).map(|e| e.name(i.saturating_add(1))).unwrap_or_default();
    let mut action = None;
    egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
        ui.label(egui::RichText::new(name).strong());
        action = if deleting.is_some() { delete_question(ui, t, list, c, i) } else { name_editor(ui, list, i) };
    });
    action
}

/// "Delete this game? It can't be undone." about game `i`, with Delete and Cancel (or Esc).
fn delete_question(ui: &mut egui::Ui, t: &Tokens, list: &mut ListState, c: &ChessCollection, i: usize) -> Option<GamesAction> {
    let mut action = None;
    let uid = c.get(i).map(|e| e.uid);
    {
        ui.label(egui::RichText::new(tl!("Delete this game? It can't be undone.")).small().color(t.text_muted));
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
                // The keyboard goes to the game that takes the deleted one's place.
                let next = c.get(i.saturating_add(1)).or_else(|| i.checked_sub(1).and_then(|j| c.get(j))).map(|e| e.uid);
                list.focus_later(ui.ctx(), if keys { next } else { None });
                list.deleting = None;
                action = Some(GamesAction::Delete(i));
            } else if no.clicked() || esc {
                list.focus_later(ui.ctx(), if keys || esc { uid } else { None });
                list.deleting = None;
            }
        });
    }
    action
}

/// The name editor of game `i`: White, Black and Event, then Rename (or Enter) and Cancel (or
/// Esc).
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
            let GameNames { white, black, event } = &mut r.names;
            for (k, (label, value)) in [(tl!("White"), white), (tl_ctx!("chess side", "Black"), black), (tl!("Event"), event)].into_iter().enumerate()
            {
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
        ui.horizontal(|ui| {
            let ok = ui.small_button(tl!("Rename"));
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

/// The games of a PGN file, one text each: a tag line after moves starts the next game (a `[`
/// inside a `{…}` comment doesn't). At most `max` games and one more, so the caller can tell
/// there were more.
pub(crate) fn split_pgn(text: &str, max: usize) -> Vec<&str> {
    let mut games = Vec::new();
    let (mut start, mut at) = (0usize, 0usize);
    let (mut moves, mut in_comment) = (false, false);
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if !in_comment && moves && trimmed.starts_with('[') {
            if let Some(game) = text.get(start..at) {
                games.push(game);
            }
            if games.len() > max {
                return games;
            }
            start = at;
            moves = false;
        }
        let tag = !in_comment && trimmed.starts_with('[');
        if !tag && !trimmed.starts_with('%') && !trimmed.trim().is_empty() {
            moves = true;
        }
        if !tag {
            for ch in line.chars() {
                match ch {
                    '{' => in_comment = true,
                    '}' => in_comment = false,
                    _ => {}
                }
            }
        }
        at = at.saturating_add(line.len());
    }
    if let Some(game) = text.get(start..).filter(|g| !g.trim().is_empty()) {
        games.push(game);
    }
    games
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
    }

    /// The web build: autosave the chess games in eframe's `Storage` (read from it now).
    pub fn chess_games_from_storage(&mut self, storage: Option<&dyn eframe::Storage>) {
        let text = storage.and_then(|s| s.get_string(chess_autosave::STORAGE_KEY));
        self.chess.games.autosave_to_storage(text.as_deref());
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
    /// diagram `source`. It is added to the document's games and shown; the game shown before
    /// stays as it was. The same diagram, opened before and untouched since, is shown again
    /// instead. Whether the position could be read (else the user is told why).
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
        // A new game: loading it is not a step to undo.
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
            None => c.add(GameEntry::new(board)),
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
                self.chess.games.collection_mut().add(GameEntry::new(BoardState::default()));
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
                self.chess.games.collection_mut().move_game(i, up);
            }
            GamesAction::NewFrom(node) => match self.chess.games.collection_mut().new_from(node) {
                Ok(()) => {
                    self.chess.study.setup = None;
                    self.chess.enter_scope();
                }
                Err(why) => self.notify(why),
            },
            GamesAction::Open => self.open_chess_pgn(),
            GamesAction::Save => self.save_chess_pgn(false),
            GamesAction::SaveAll => self.save_chess_pgn(true),
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

    /// The PGN of the game shown (`all`: of every game of the document shown, in list order, a
    /// blank line between two), and how many games were left out (illegal positions have no
    /// PGN). `None`, the user told why, when the games come from a document that doesn't allow
    /// copying, or there is nothing to write.
    fn chess_pgn_text(&mut self, all: bool) -> Option<(String, usize)> {
        let c = self.chess.games.collection();
        let boards: Vec<&BoardState> = if all { c.iter().map(|e| &e.board).collect() } else { vec![&c.active().board] };
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
        let Some((text, skipped)) = self.chess_pgn_text(all) else { return };
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
        let mut boards = Vec::new();
        let mut failed = 0usize;
        for chunk in chunks.iter().take(chess_autosave::MAX_GAMES) {
            match Game::from_pgn(chunk) {
                Ok((game, stopped)) => {
                    let mut board = BoardState::default();
                    board.game = game;
                    board.message =
                        stopped.map(|m| crate::i18n::fmt(tl!("Reading stopped at {move}: it is not a legal move there."), &[("move", &m)]));
                    board.refresh_fields(true);
                    boards.push(board);
                }
                Err(_) => failed = failed.saturating_add(1),
            }
        }
        let added = boards.len();
        if added == 0 {
            self.notify_fmt("Couldn't read {name}: {e}", &[("name", name), ("e", tl!("no game could be read"))]);
            return 0;
        }
        let c = self.chess.games.collection_mut();
        let first = if c.len() == 1 && c.active().is_blank() { 0 } else { c.len() };
        for board in boards {
            c.add(GameEntry::new(board));
        }
        c.switch_to(first);
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
    fn renaming_sets_the_three_tags_in_one_undo_step() {
        let mut e = GameEntry::new(board_with(&["e2e4"]));
        let names = GameNames { white: " Werle ".into(), black: "Jianu".into(), event: "Wijk aan Zee".into() };
        assert!(e.rename(&names));
        assert_eq!(e.name(1), "Werle–Jianu");
        assert_eq!(e.names(), GameNames { white: "Werle".into(), black: "Jianu".into(), event: "Wijk aan Zee".into() });
        assert_eq!(e.board.undo_label().as_deref(), Some("Edit game details"));
        assert!(!e.rename(&names), "the same names change nothing");
        assert!(e.board.undo());
        assert_eq!(e.name(1), "Game 1", "one step took all three back");
        assert!(e.rename(&GameNames { event: "Training".into(), ..GameNames::default() }));
        assert_eq!(e.name(1), "Training", "named by its event without players");
        assert!(e.rename(&GameNames::default()));
        assert!(e.board.game.tags().is_empty(), "empty names remove the tags");
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

    #[test]
    fn a_multi_game_pgn_splits_on_its_tag_lines() {
        let text = "[Event \"A\"]\n[White \"x\"]\n\n1. e4 e5 {a comment\n[%clk 0:10] over lines} 2. Nf3 *\n\n[Event \"B\"]\n\n1. d4 *\n[Site \"C\"]\n1. c4 *\n";
        let games = split_pgn(text, 10);
        assert_eq!(games.len(), 3, "{games:#?}");
        assert!(games[0].contains("Nf3") && games[1].contains("d4") && games[2].contains("c4"));
        assert_eq!(games.concat(), text, "nothing lost between games");
        // At most `max` and one more.
        let many = "[Event \"x\"]\n1. e4 *\n".repeat(10);
        assert_eq!(split_pgn(&many, 3).len(), 4);
        assert!(split_pgn("", 3).is_empty());
        assert_eq!(split_pgn("1. e4 e5", 3), ["1. e4 e5"]);
    }

    #[test]
    fn file_names_are_safe() {
        assert_eq!(file_stem("Werle–Jianu"), "Werle–Jianu");
        assert_eq!(file_stem("a/b:c?"), "a_b_c_");
        assert_eq!(file_stem("..."), "game");
        assert_eq!(file_stem(&"x".repeat(500)).len(), 100);
    }
}
