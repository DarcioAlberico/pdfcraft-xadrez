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
//! - **The list.** "Games in this document" in the board panels lists them by their players, the
//!   diagram they came from, or their number; a click shows one (that is walking, not an undo
//!   step), and New game adds a blank one. Renaming, duplicating and deleting come later (T14).
//! - **Kept across sessions** ([`crate::chess_autosave`]): two seconds after the last change, on
//!   exit and when a document closes, each collection is written under its document's keys
//!   ([`DocKeys`]), and found again by them when the document is opened. Saving the document can
//!   change its keys (a first `/ID`, a new size or path), so the collection takes the new ones
//!   right after every save ([`PdfCraftApp::rekey_chess_games`]). A protected document's games
//!   (encrypted, or copying not allowed) are written only once the user allows it, asked once
//!   per document and session.
//! - **PGN files.** Save PGN… writes the game shown, Save all games… the whole collection, to a
//!   file the user chooses (a download on the web); Open PGN… adds every game of a file.
//!
//! Each game's undo history ([`crate::chess_history`]) is its own, and is not kept across
//! sessions. Where each move was read on a page (`NodeSource`) lives only in the game's moves.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};

use pdfcraft_engine::DocId;
use pdfcraft_engine::chess::DocKeys;
use pdfcraft_engine::chess::pdfcraft_chess::game::{Game, NodePath};

use crate::PdfCraftApp;
use crate::chess_autosave::{self, Blocked, Store};
use crate::chess_board_ui::{BoardState, Source};
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
    /// details, else the diagram it came from, else "Game {number}".
    pub fn name(&self, number: usize) -> String {
        let tag = |t: &str| self.board.game.tag(t).map(str::trim).filter(|v| !v.is_empty() && *v != "?").map(str::to_owned);
        match (tag("White"), tag("Black")) {
            (Some(w), Some(b)) => return format!("{w}–{b}"),
            (Some(one), None) | (None, Some(one)) => return one,
            (None, None) => {}
        }
        match self.board.source {
            Some(s) => crate::i18n::fmt(
                tl!("Diagram {n}, p. {page}"),
                &[("n", &(s.index.saturating_add(1)).to_string()), ("page", &(s.page.saturating_add(1)).to_string())],
            ),
            None => crate::i18n::fmt(tl!("Game {n}"), &[("n", &number.to_string())]),
        }
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
}

/// The games of the document shown: the list, what can be done with them, and why they aren't
/// autosaved when they aren't.
pub(crate) fn games_section(ui: &mut egui::Ui, t: &Tokens, games: &ChessGames) -> Option<GamesAction> {
    let c = games.collection();
    let mut action = None;
    let n = c.len().to_string();
    let title = if games.current.is_some() { tl!("Games in this document ({n})") } else { tl!("Games ({n})") };
    egui::CollapsingHeader::new(crate::i18n::fmt(title, &[("n", &n)])).id_salt("chess-games").default_open(true).show(ui, |ui| {
        if c.autosave.protected && games.store.is_on() && c.has_content() && !games.consent.contains_key(&c.cid) {
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
        egui::ScrollArea::vertical().id_salt("chess-games-list").max_height(140.0).auto_shrink([false, true]).show(ui, |ui| {
            for (i, e) in c.iter().enumerate() {
                let name = e.name(i.saturating_add(1));
                if ui.selectable_label(i == c.active_index(), name).clicked() && i != c.active_index() {
                    action = Some(GamesAction::Switch(i));
                }
            }
        });
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
            if ui.small_button(tl!("Save all games as PGN…")).clicked() {
                action = Some(GamesAction::SaveAll);
            }
        });
    });
    action
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

    /// The PGN of the game shown (`all`: of every game of the document shown), unless the games
    /// come from a document that doesn't allow copying (the user is told). `None` when there is
    /// nothing to write (only illegal positions, which have no PGN).
    fn chess_pgn_text(&mut self, all: bool) -> Option<String> {
        let c = self.chess.games.collection();
        let boards: Vec<&BoardState> = if all { c.iter().map(|e| &e.board).collect() } else { vec![&c.active().board] };
        let docs: HashSet<DocId> = boards.iter().filter_map(|b| b.source.map(|s| s.doc)).collect();
        let text: Vec<String> = boards.iter().filter(|b| b.illegal.is_none()).map(|b| b.game.pgn()).collect();
        for doc in docs {
            if self.session.get(doc).is_some() && !self.may_copy(doc) {
                return None;
            }
        }
        if text.is_empty() {
            self.notify(tl!("Nothing to save: an illegal position has no PGN.").to_string());
            return None;
        }
        Some(text.join("\n"))
    }

    /// Chess ▸ Save PGN… (`all`: Save all games as PGN…): write the game shown, or the document's
    /// games, to a `.pgn` file the user chooses (downloaded on the web).
    pub fn save_chess_pgn(&mut self, all: bool) {
        let Some(text) = self.chess_pgn_text(all) else { return };
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
                    Ok(()) => app.notify_fmt("Saved {name}", &[("name", &shown)]),
                    Err(e) => app.notify_fmt("Couldn't save {name}: {e}", &[("name", &shown), ("e", &e.to_string())]),
                }
            });
        }
        #[cfg(target_arch = "wasm32")]
        match crate::editing::download(&name, text.as_bytes()) {
            Ok(()) => self.notify_fmt("Downloaded {name}", &[("name", &name)]),
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
