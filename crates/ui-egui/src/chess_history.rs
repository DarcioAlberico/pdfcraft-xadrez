//! Chess ▸ Undo: the chess board's game history, and which history Undo goes to.
//!
//! Every change to the board's game (a move played, moves deleted, a line promoted, a glyph, a
//! comment, a drawing on the board, the game's details, a position or a PGN loaded) is one step. A step keeps the whole
//! board as it was before it ([`Snapshot`]: the game with its move shown, the illegal position
//! shown in its place, where it came from and the board's side), so undoing it puts that back
//! exactly. Typing in a comment or a detail field is one step per editing session of the field,
//! not one per key. Walking the moves is not a change and is never undone.
//!
//! The history is capped twice: at [`MAX_STEPS`] steps and at [`MAX_BYTES`] of estimated memory
//! ([`Snapshot::bytes`]); the oldest steps go first.
//!
//! **Which history Undo goes to** (D12, the scope of the last action): a click in the chess
//! panels or a change to the game puts Undo and Redo on the game; a click on the page or a change
//! to the document puts them back on the document. See [`ChessState::in_scope`].

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};

use pdfcraft_engine::chess::pdfcraft_chess::game::{Game, Token};
use pdfcraft_engine::chess::pdfcraft_chess::placement::Placement;

use crate::chess_board_ui::Source;
use crate::chess_ui::ChessState;

/// Most steps kept (undo and redo together).
pub(crate) const MAX_STEPS: usize = 200;

/// Most memory the steps may take, as [`Estimate::bytes`] counts it (undo and redo together).
pub(crate) const MAX_BYTES: usize = 64 << 20;

/// Bytes a move of a game takes in memory, besides its text: its board (bitboards and state,
/// about 100 bytes), the node's links, glyph and comment fields and its place on the page. A
/// little more than a 64-bit build's `Node`, so the estimate errs high.
const NODE_BYTES: usize = 384;

/// What a step did, as Undo and Redo name it ("Undo Play Nd5").
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChessEdit {
    /// A move played on the board (its SAN).
    Play(String),
    /// A line the engine suggested, played from the move shown.
    EngineLine,
    /// The moves after a move, or a move and what follows it, deleted.
    DeleteMoves,
    /// A variation made the main line.
    MakeMainLine,
    /// A variation promoted one level (it continues its move; the old line is its first variation).
    PromoteOne,
    /// A line demoted: its first variation continues its move instead.
    Demote,
    /// A variation deleted, with the moves after it.
    DeleteVariation,
    /// A variation moved one place among the others at its fork (T15).
    MoveVariation,
    /// A null move played: the side to move passes, to show the other side's threat (T15).
    NullMove,
    /// A move marked for a diagram, or the mark taken off (T15).
    Diagram,
    /// A glyph put on or taken off a move (as it is shown).
    Glyph(String),
    /// A move's comment typed in.
    Comment,
    /// The comment before a move typed in.
    CommentBefore,
    /// Every glyph taken off a move (typed `0`).
    ClearGlyphs,
    /// A square coloured or an arrow drawn on the position shown, or taken off (U7).
    Draw,
    /// Every drawing taken off the position shown (U7).
    ClearDrawings,
    /// The game's details (its PGN tags) typed in.
    GameDetails,
    /// Whose move it is at the start.
    SideToMove,
    /// The number of the move the game starts at, corrected.
    MoveNumber,
    /// A FEN typed in the FEN field.
    LoadFen,
    /// The PGN field read into the board; as a step of the list of games, the games of a PGN
    /// (Open PGN…, or the PGN field's games after its first) added to it.
    LoadPgn,
    /// A diagram of the document put on the board.
    LoadDiagram,
    /// A position set up on the study board.
    SetUp,
    /// Back to the standard start position.
    StandardStart,
    /// Text from the page or the clipboard captured into the game (T7a).
    Capture(crate::chess_capture::Captured),
    /// A game deleted from the document's list of games (a step of the list, wave 10 U2).
    DeleteGame,
    /// A game moved in the document's list of games (a step of the list).
    MoveGame,
    /// A blank game added to the list of games (a step of the list).
    NewGame,
    /// A copy of a game put after it in the list (a step of the list).
    DuplicateGame,
    /// A game started at a move's position, or at a position set up (a step of the list).
    NewGameFromHere,
    /// A game of the trash (deleted moves kept) opened as a new game (a step of the list).
    RestoreGame,
}

impl ChessEdit {
    /// The step's name in the current language, as it reads after "Undo" and "Redo".
    pub fn text(&self) -> String {
        match self {
            ChessEdit::Play(san) => crate::i18n::fmt(tl!("Play {san}"), &[("san", san)]),
            ChessEdit::EngineLine => tl!("Play the engine's line").to_string(),
            ChessEdit::DeleteMoves => tl!("Delete moves").to_string(),
            ChessEdit::MakeMainLine => tl!("Make main line").to_string(),
            ChessEdit::PromoteOne => tl!("Promote a variation").to_string(),
            ChessEdit::Demote => tl!("Demote a line").to_string(),
            ChessEdit::DeleteVariation => tl!("Delete a variation").to_string(),
            ChessEdit::MoveVariation => tl!("Move a variation").to_string(),
            ChessEdit::NullMove => tl!("Play a null move").to_string(),
            ChessEdit::Diagram => tl!("Diagram mark").to_string(),
            ChessEdit::Glyph(glyph) => crate::i18n::fmt(tl!("Glyph {glyph}"), &[("glyph", glyph)]),
            ChessEdit::Comment => tl!("Edit move comment").to_string(),
            ChessEdit::CommentBefore => tl!("Edit the comment before a move").to_string(),
            ChessEdit::ClearGlyphs => tl!("Clear glyphs").to_string(),
            ChessEdit::Draw => tl!("Draw on the board").to_string(),
            // The button says it as a command; as a step it is named like the others.
            ChessEdit::ClearDrawings => tl_ctx!("chess undo step", "Clear drawings").to_string(),
            ChessEdit::GameDetails => tl!("Edit game details").to_string(),
            ChessEdit::SideToMove => tl!("Change side to move").to_string(),
            ChessEdit::MoveNumber => tl!("Change the move number").to_string(),
            ChessEdit::LoadFen => tl!("Load the FEN").to_string(),
            ChessEdit::LoadPgn => tl!("Load the PGN").to_string(),
            ChessEdit::LoadDiagram => tl!("Load the diagram").to_string(),
            ChessEdit::SetUp => tl!("Position set-up").to_string(),
            ChessEdit::StandardStart => tl!("Reset to the start position").to_string(),
            ChessEdit::Capture(kind) => kind.text().to_string(),
            ChessEdit::DeleteGame => tl!("Delete game").to_string(),
            ChessEdit::MoveGame => tl!("Move game").to_string(),
            ChessEdit::NewGame => tl!("New game").to_string(),
            ChessEdit::DuplicateGame => tl!("Duplicate game").to_string(),
            ChessEdit::NewGameFromHere => tl!("New game from this position").to_string(),
            ChessEdit::RestoreGame => tl!("Open as a new game").to_string(),
        }
    }

    /// Whether the step puts a new game in place of the old one. Such a step counts even when the
    /// new game's revision happens to equal the old one's.
    pub(crate) fn replaces_game(&self) -> bool {
        matches!(
            self,
            ChessEdit::SideToMove | ChessEdit::LoadFen | ChessEdit::LoadPgn | ChessEdit::LoadDiagram | ChessEdit::SetUp | ChessEdit::StandardStart
        )
    }

    /// Whether the step also chose the board's side (loading a position does): only then does
    /// undoing it turn the board back. Flipping the board by hand is not a step of its own, and
    /// undoing a move must not undo it.
    pub(crate) fn sets_side(&self) -> bool {
        matches!(self, ChessEdit::LoadFen | ChessEdit::LoadDiagram | ChessEdit::SetUp)
    }
}

/// "Undo Play Nd5" (`undo`) or "Redo Play Nd5", in the current language: the same composition as
/// the document's history labels (`i18n::command_label`), the verb before the step's name.
pub fn command_text(undo: bool, edit: &ChessEdit) -> String {
    let verb = if undo { tl!("Undo") } else { tl!("Redo") };
    format!("{verb} {}", edit.text())
}

/// How much memory a kept state takes, as the caps count it.
pub(crate) trait Estimate {
    fn bytes(&self) -> usize;
}

/// The board as a step found it.
#[derive(Clone)]
pub(crate) struct Snapshot {
    pub game: Game,
    pub illegal: Option<(Placement, String)>,
    pub source: Option<Source>,
    pub flipped: bool,
}

impl Estimate for Snapshot {
    /// The game's PGN text (its moves, comments and tags, which the game keeps as strings) plus
    /// [`NODE_BYTES`] for each move, variations included, and one for the start. Counting the PGN
    /// text costs a pass over the game, once per step.
    fn bytes(&self) -> usize {
        let moves = self.game.tokens().iter().filter(|t| matches!(t, Token::Move { .. })).count();
        let text = self.game.pgn().len().saturating_add(self.illegal.as_ref().map_or(0, |(_, fen)| fen.len()));
        moves.saturating_add(1).saturating_mul(NODE_BYTES).saturating_add(text)
    }
}

/// A number for a step put on either side of a history, larger than any before it in the process:
/// when Undo has a step of the game shown and one of the document's list of games, it takes the
/// newer ([`crate::chess_collection`]).
pub(crate) fn next_seq() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// One step: what it did, the state on the far side of it (before it on the undo side, after it
/// on the redo side), and when it was put on its side ([`next_seq`]).
struct Step<S> {
    edit: ChessEdit,
    state: S,
    bytes: usize,
    seq: u64,
}

impl<S: Estimate> Step<S> {
    fn new(edit: ChessEdit, state: S) -> Step<S> {
        let bytes = state.bytes();
        Step { edit, state, bytes, seq: next_seq() }
    }
}

/// The steps that can be undone (oldest first) and redone (next one last).
pub(crate) struct History<S> {
    past: VecDeque<Step<S>>,
    future: Vec<Step<S>>,
    /// The bytes of every step kept, undo and redo.
    bytes: usize,
    /// The newest game revision seen: a game put back gets a newer one, so a revision never
    /// names two different games (the FEN and PGN fields refresh on it).
    max_revision: u64,
    /// Steps recorded so far, so the app can tell the game was changed.
    edits: u64,
}

impl<S> Default for History<S> {
    fn default() -> History<S> {
        History { past: VecDeque::new(), future: Vec::new(), bytes: 0, max_revision: 0, edits: 0 }
    }
}

/// The board's history.
pub(crate) type GameHistory = History<Snapshot>;

impl<S: Estimate> History<S> {
    /// Keep `before`, the state before a change named `edit`. Whatever could be redone goes.
    pub(crate) fn record(&mut self, edit: ChessEdit, before: S) {
        self.future.clear();
        let step = Step::new(edit, before);
        self.bytes = self.past.iter().map(|s| s.bytes).fold(step.bytes, usize::saturating_add);
        self.past.push_back(step);
        self.edits = self.edits.wrapping_add(1);
        self.fit();
    }

    /// Step back: `current` goes to the redo side and the state before the last step comes back,
    /// with that step's name. `None` (and `current` dropped) when there is nothing to undo.
    pub(crate) fn undo(&mut self, current: S) -> Option<(ChessEdit, S)> {
        let step = self.past.pop_back()?;
        self.bytes = self.bytes.saturating_sub(step.bytes);
        let back = Step::new(step.edit.clone(), current);
        self.bytes = self.bytes.saturating_add(back.bytes);
        self.future.push(back);
        self.fit();
        Some((step.edit, step.state))
    }

    /// Step forward again: the mirror of [`Self::undo`].
    pub(crate) fn redo(&mut self, current: S) -> Option<(ChessEdit, S)> {
        let step = self.future.pop()?;
        self.bytes = self.bytes.saturating_sub(step.bytes);
        let back = Step::new(step.edit.clone(), current);
        self.bytes = self.bytes.saturating_add(back.bytes);
        self.past.push_back(back);
        self.fit();
        Some((step.edit, step.state))
    }

    /// Drop the oldest steps until both caps hold. Redo steps go only when no undo step is left
    /// to drop (the farthest redo first), so the next Undo and Redo always stay.
    fn fit(&mut self) {
        while self.past.len().saturating_add(self.future.len()) > MAX_STEPS || self.bytes > MAX_BYTES {
            let dropped = match self.past.pop_front() {
                Some(step) => step,
                None if !self.future.is_empty() => self.future.remove(0),
                None => break,
            };
            self.bytes = self.bytes.saturating_sub(dropped.bytes);
        }
    }
}

impl<S> History<S> {
    /// What Undo would undo.
    pub(crate) fn undo_edit(&self) -> Option<&ChessEdit> {
        self.past.back().map(|s| &s.edit)
    }

    /// The state before the step Undo would undo.
    pub(crate) fn undo_state(&self) -> Option<&S> {
        self.past.back().map(|s| &s.state)
    }

    /// What Redo would redo.
    pub(crate) fn redo_edit(&self) -> Option<&ChessEdit> {
        self.future.last().map(|s| &s.edit)
    }

    /// When the step Undo (`undo`) or Redo would take was put on its side ([`next_seq`]).
    pub(crate) fn seq(&self, undo: bool) -> Option<u64> {
        if undo { self.past.back().map(|s| s.seq) } else { self.future.last().map(|s| s.seq) }
    }

    /// The memory every step kept takes, as the caps count it.
    pub(crate) fn bytes(&self) -> usize {
        self.bytes
    }

    /// Note a game revision shown.
    pub(crate) fn see(&mut self, revision: u64) {
        self.max_revision = self.max_revision.max(revision);
    }

    /// The newest game revision seen.
    pub(crate) fn max_revision(&self) -> u64 {
        self.max_revision
    }

    /// How many steps were recorded so far (wrapping).
    pub(crate) fn edits(&self) -> u64 {
        self.edits
    }

    #[cfg(test)]
    fn lens(&self) -> (usize, usize) {
        (self.past.len(), self.future.len())
    }
}

impl ChessState {
    /// Whether Undo and Redo go to the chess game: the last action was a click in the chess
    /// panels or a change to the game (counted by the game's history), not a click on the page or
    /// a change to the document.
    ///
    /// The changes counted are the game shown's: showing another game (another document's, or
    /// another of the list) is not a change to it. Before the scope was ever settled, any change
    /// counts.
    pub fn in_scope(&self) -> bool {
        let (game, edits) = self.games.scope_stamp();
        let (settled_game, settled_edits) = self.scope_edits;
        self.scope || ((game == settled_game || settled_game == 0) && edits != settled_edits)
    }

    /// Undo and Redo go to the chess game.
    pub(crate) fn enter_scope(&mut self) {
        self.scope = true;
        self.scope_edits = self.games.scope_stamp();
    }

    /// Undo and Redo go to the document again.
    pub(crate) fn leave_scope(&mut self) {
        self.scope = false;
        self.scope_edits = self.games.scope_stamp();
    }
}

/// Whether a pointer button went down this frame inside `ui`'s area, on `ui`'s own layer (not on
/// a menu or a window over it).
pub(crate) fn pressed_in(ui: &egui::Ui) -> bool {
    let area = ui.max_rect().intersect(ui.clip_rect());
    let Some(at) = ui.input(|i| if i.pointer.any_pressed() { i.pointer.interact_pos() } else { None }) else { return false };
    area.contains(at) && ui.ctx().layer_id_at(at) == Some(ui.layer_id())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A state that only says how big it is.
    #[derive(Debug, PartialEq)]
    struct Weighed(usize);

    impl Estimate for Weighed {
        fn bytes(&self) -> usize {
            self.0
        }
    }

    #[test]
    fn undo_and_redo_swap_states_and_keep_their_names() {
        let mut h = History::default();
        h.record(ChessEdit::Play("e4".into()), Weighed(1));
        h.record(ChessEdit::Comment, Weighed(2));
        assert_eq!(h.undo_edit(), Some(&ChessEdit::Comment));
        assert_eq!(h.undo(Weighed(3)), Some((ChessEdit::Comment, Weighed(2))));
        assert_eq!(h.redo_edit(), Some(&ChessEdit::Comment));
        assert_eq!(h.undo_edit(), Some(&ChessEdit::Play("e4".into())));
        assert_eq!(h.redo(Weighed(2)), Some((ChessEdit::Comment, Weighed(3))));
        assert_eq!(h.redo(Weighed(9)), None);
        // A new step drops what could be redone.
        assert_eq!(h.undo(Weighed(3)).map(|(e, _)| e), Some(ChessEdit::Comment));
        h.record(ChessEdit::DeleteMoves, Weighed(2));
        assert_eq!(h.redo_edit(), None);
        assert_eq!(h.lens(), (2, 0));
        assert_eq!(h.edits(), 3);
        let mut empty: History<Weighed> = History::default();
        assert_eq!(empty.undo(Weighed(0)), None);
        assert_eq!(empty.redo(Weighed(0)), None);
    }

    #[test]
    fn at_most_two_hundred_steps_the_oldest_dropped() {
        let mut h = History::default();
        for i in 0..250 {
            h.record(ChessEdit::Play(format!("m{i}")), Weighed(i));
        }
        assert_eq!(h.lens(), (MAX_STEPS, 0));
        // The newest are kept: undoing all of them ends at the 51st state.
        let mut last = None;
        while let Some((_, s)) = h.undo(Weighed(0)) {
            last = Some(s);
        }
        assert_eq!(last, Some(Weighed(50)));
        // Undo and redo together stay within the cap.
        assert_eq!(h.lens(), (0, MAX_STEPS));
    }

    #[test]
    fn the_byte_cap_drops_the_oldest() {
        let mut h = History::default();
        let big = MAX_BYTES / 4;
        for i in 0..4 {
            h.record(ChessEdit::Play(format!("m{i}")), Weighed(big));
        }
        assert_eq!(h.lens(), (4, 0), "exactly at the cap: all kept");
        h.record(ChessEdit::Comment, Weighed(1));
        assert_eq!(h.lens(), (4, 0), "one more byte drops the oldest");
        assert!(h.bytes <= MAX_BYTES);
        // The oldest went: the first undo state left is m1's.
        let names: Vec<_> = std::iter::from_fn(|| h.undo(Weighed(0)).map(|(e, _)| e)).collect();
        assert_eq!(names.last(), Some(&ChessEdit::Play("m1".into())));
        // A step bigger than the cap on its own does not stay, and goes too (after the older ones).
        let mut h = History::default();
        h.record(ChessEdit::Comment, Weighed(10));
        h.record(ChessEdit::Comment, Weighed(usize::MAX));
        assert_eq!(h.lens(), (0, 0));
        assert_eq!(h.bytes, 0);
    }

    #[test]
    fn a_games_estimate_grows_with_its_moves_and_text() {
        let mut game = Game::standard();
        let snap = |game: &Game| Snapshot { game: game.clone(), illegal: None, source: None, flipped: false };
        let empty = snap(&game).bytes();
        assert!(empty >= NODE_BYTES);
        for mv in ["e2e4", "e7e5", "g1f3"] {
            game.play_uci(mv).unwrap();
        }
        let three = snap(&game).bytes();
        assert!(three >= empty + 3 * NODE_BYTES, "{three} vs {empty}");
        game.set_comment(game.current(), &"x".repeat(1000));
        assert!(snap(&game).bytes() >= three + 1000);
    }
}
