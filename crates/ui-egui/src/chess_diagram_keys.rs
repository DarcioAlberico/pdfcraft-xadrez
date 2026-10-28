//! Chess ▸ Diagram by the keyboard and for screen readers (plan A8, P3c).
//!
//! The board is one Tab stop. While it has the keyboard, the arrows move a cursor over its
//! squares as shown, a piece's letter in the interface's notation puts that piece there (R D T B
//! C P in Portuguese, K Q R B N P in English; with Shift, Black's), Delete, Backspace or Space
//! empties the square and Enter puts the palette's piece there as a click does. Each square is a
//! node of its own for screen readers ("e4, White knight"), and the board names the one under the
//! cursor.
//!
//! The tab's own keys work while the keyboard is in the tab or the pointer is over it: Ctrl+Enter
//! confirms and goes on, Page Up and Page Down walk the review queue, Ctrl+Z and Ctrl+Shift+Z undo
//! and redo the editor's changes (the document's Undo when the editor has none), and Esc gives the
//! keyboard back to the diagram's row in the Diagrams list, discarding nothing.
//!
//! The keys leave the frame's input in `raw_input_hook` ([`take_keys`]), before egui moves the
//! focus on the arrows and before the app's shortcuts (the tools' letters, Space for the Hand,
//! Undo) can see them, so they are decided on the focus as the last frame left it.

use egui::{Event, Id, Key, Modifiers};
use pdfcraft_engine::chess::pdfcraft_chess::placement::{Color, Piece, PieceKind, Square};
use pdfcraft_engine::chess::pdfcraft_chess::san::Lang;

use crate::chess_editor::{DiagramEditor, KINDS};
use crate::chess_ui::ChessState;

/// Most keys kept from one frame's input (a held key repeats).
const MAX_KEYS: usize = 64;

/// The board that has the keyboard, and the keys taken for it in a frame.
pub(crate) type BoardKeys = (Id, Vec<(Key, Modifiers)>);

/// The keyboard's state between the hook and the pass that draws the tab.
#[derive(Default)]
pub struct DiagramKeys {
    /// The Diagram tab's rectangle, as the last pass drew it with an editor (taken by every hook:
    /// none when the tab was not drawn).
    pub(crate) tab: Option<egui::Rect>,
    /// A widget of the tab had the keyboard at the end of the last pass.
    pub(crate) focus_in_tab: bool,
    /// The board that has the keyboard, and the keys taken for it this frame.
    pub(crate) board: Option<BoardKeys>,
    /// The tab's own keys taken this frame.
    pub(crate) tab_keys: Vec<(Key, Modifiers)>,
    /// The selected row of the Diagrams list takes the keyboard when it is next drawn (Esc).
    pub(crate) focus_list: bool,
    /// What the tab last said to screen readers: the diagram opened ("Diagram 4 of 40 to review,
    /// page 18, 2 doubtful squares").
    pub(crate) announce: String,
}

/// The board's id: the Diagram tab's (`tab`) or the classic editor's.
pub(crate) fn board_id(tab: bool) -> Id {
    Id::new(if tab { "chess-diagram-board" } else { "chess-editor-board" })
}

/// Take this frame's keys for the board that has the keyboard and for the Diagram tab out of
/// `events`. Nothing is taken unless `free` (no modal dialog, text field or popup has the
/// keyboard). Only presses are taken: a release seen without its press is harmless.
pub(crate) fn take_keys(chess: &mut ChessState, ctx: &egui::Context, events: &mut Vec<Event>, free: bool) {
    // Page Up and Page Down walk the review queue only when there is one (else they are the page's).
    let paging = events.iter().any(|e| matches!(e, Event::Key { key: Key::PageUp | Key::PageDown, pressed: true, .. }));
    let queue = paging && chess.results.as_ref().is_some_and(|r| !r.review_queue().is_empty());
    let keys = &mut chess.diagram_keys;
    let tab = keys.tab.take();
    keys.board = None;
    keys.tab_keys.clear();
    let Some(ed) = chess.editor.as_ref().filter(|_| free) else { return };
    let board = [board_id(true), board_id(false)].into_iter().find(|id| ctx.memory(|m| m.has_focus(*id)));
    let pointer = tab.zip(ctx.pointer_hover_pos()).is_some_and(|(r, p)| r.contains(p));
    let focus_in = tab.is_some() && keys.focus_in_tab;
    let in_tab = focus_in || pointer;
    let (undo, redo) = (ed.can_undo(), ed.can_redo());
    let mut on_board = Vec::new();
    let mut on_tab = Vec::new();
    events.retain(|event| {
        let Event::Key { key, pressed: true, modifiers, .. } = event else { return true };
        if board.is_some() && board_key(*key, *modifiers) {
            if on_board.len() < MAX_KEYS {
                on_board.push((*key, *modifiers));
            }
            return false;
        }
        if in_tab && tab_key(*key, *modifiers, TabState { undo, redo, queue, focus_in }) {
            if on_tab.len() < MAX_KEYS {
                on_tab.push((*key, *modifiers));
            }
            return false;
        }
        true
    });
    keys.board = board.map(|id| (id, on_board));
    keys.tab_keys = on_tab;
}

/// Whether the board takes `key` pressed with `m`: the arrows, Delete, Backspace, Space and Enter
/// alone, and the letters alone or with Shift (all of them, so none of the app's one-letter
/// shortcuts fires while the board has the keyboard).
fn board_key(key: Key, m: Modifiers) -> bool {
    let plain = m.is_none();
    match key {
        Key::ArrowUp | Key::ArrowDown | Key::ArrowLeft | Key::ArrowRight | Key::Delete | Key::Backspace | Key::Space | Key::Enter => plain,
        _ => letter(key).is_some() && (plain || m.matches_exact(Modifiers::SHIFT)),
    }
}

/// What decides whether the tab takes a key.
#[derive(Clone, Copy, Default)]
struct TabState {
    /// The editor has a change to undo, or one undone to redo.
    undo: bool,
    redo: bool,
    /// There are diagrams to review.
    queue: bool,
    /// The keyboard is in the tab (not only the pointer over it).
    focus_in: bool,
}

/// Whether the tab takes `key` pressed with `m`. Ctrl+Z and Ctrl+Shift+Z only while the editor
/// has something to undo or redo, else they are the document's; Page Up and Page Down only with
/// diagrams to review; Esc only while the keyboard is in the tab, not for the pointer over it.
fn tab_key(key: Key, m: Modifiers, s: TabState) -> bool {
    match key {
        Key::Enter => m.matches_exact(Modifiers::COMMAND),
        Key::PageUp | Key::PageDown => s.queue && m.is_none(),
        Key::Z => (s.undo && m.matches_exact(Modifiers::COMMAND)) || (s.redo && m.matches_exact(Modifiers::COMMAND | Modifiers::SHIFT)),
        Key::Escape => s.focus_in && m.is_none(),
        _ => false,
    }
}

/// What the tab's key `key` with `m` asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TabKey {
    ConfirmNext,
    Previous,
    Next,
    Undo,
    Redo,
    BackToList,
}

/// The tab's key as an action (the keys [`take_keys`] takes for the tab).
pub(crate) fn tab_action(key: Key, m: Modifiers) -> Option<TabKey> {
    Some(match key {
        Key::Enter => TabKey::ConfirmNext,
        Key::PageUp => TabKey::Previous,
        Key::PageDown => TabKey::Next,
        Key::Z if m.shift => TabKey::Redo,
        Key::Z => TabKey::Undo,
        Key::Escape => TabKey::BackToList,
        _ => return None,
    })
}

/// The letter `key` types, upper case (`None` for a key that is not a letter).
fn letter(key: Key) -> Option<char> {
    let mut chars = key.name().chars();
    let c = chars.next()?;
    (chars.next().is_none() && c.is_ascii_alphabetic()).then(|| c.to_ascii_uppercase())
}

/// `s` as the one Latin letter it is, upper case.
fn single_latin(s: &str) -> Option<char> {
    let mut chars = s.chars();
    let c = chars.next()?;
    (chars.next().is_none() && c.is_ascii_alphabetic()).then(|| c.to_ascii_uppercase())
}

/// The key that puts a `kind` in `lang`'s notation: its own letter when it is one Latin letter,
/// else the English one unless another piece has that letter in `lang` (the Dutch pawn has no
/// letter and its P is the knight's: no key; the palette and Enter put it). Russian's letters are
/// Cyrillic, which egui's keys do not name, so Russian takes English's.
pub(crate) fn key_letter(lang: Lang, kind: PieceKind) -> Option<char> {
    if let Some(own) = single_latin(lang.letter(kind)) {
        return Some(own);
    }
    let english = single_latin(Lang::En.letter(kind))?;
    let taken = KINDS.iter().any(|k| *k != kind && single_latin(lang.letter(*k)) == Some(english));
    (!taken).then_some(english)
}

/// The piece the letter key `key` puts in `lang`'s notation, Black's with `shift`.
pub(crate) fn key_piece(lang: Lang, key: Key, shift: bool) -> Option<Piece> {
    let c = letter(key)?;
    let kind = KINDS.into_iter().find(|k| key_letter(lang, *k) == Some(c))?;
    Some(Piece { color: if shift { Color::Black } else { Color::White }, kind })
}

/// Act on the board's keys: move the cursor, put or take the piece under it. Whether the position
/// changed.
pub(crate) fn board_keys(ed: &mut DiagramEditor, keys: &[(Key, Modifiers)]) -> bool {
    let lang = crate::chess_figurines::interface_lang();
    let before = ed.fen();
    for &(key, m) in keys {
        match key {
            Key::ArrowUp => ed.move_cursor(0, -1),
            Key::ArrowDown => ed.move_cursor(0, 1),
            Key::ArrowLeft => ed.move_cursor(-1, 0),
            Key::ArrowRight => ed.move_cursor(1, 0),
            Key::Delete | Key::Backspace | Key::Space => ed.put(ed.cursor, None),
            Key::Enter => ed.paint(ed.cursor),
            _ => {
                if let Some(piece) = key_piece(lang, key, m.shift) {
                    ed.brush = crate::chess_editor::Brush::Put(piece);
                    ed.put(ed.cursor, Some(piece));
                }
            }
        }
    }
    ed.fen() != before
}

/// A square as screen readers say it: "e4, White knight", "e4, empty".
pub(crate) fn square_text(ed: &DiagramEditor, square: Square) -> String {
    match ed.position.placement.get(square) {
        Some(p) => format!("{square}, {}", crate::chess_editor::piece_name(p)),
        None => crate::i18n::fmt(tl!("{square}, empty"), &[("square", &square.to_string())]),
    }
}

/// What the tab says when diagram `index` of `page` (its label `label`) opens: its place in the
/// review queue (`place` of `queue`, when it is in it) and how many squares the model doubted.
pub(crate) fn announcement(index: usize, label: &str, place: Option<(usize, usize)>, doubtful: usize) -> String {
    let mut text = match place {
        Some((i, n)) => crate::i18n::fmt(
            tl!("Diagram {i} of {n} to review, page {p}"),
            &[("i", &i.saturating_add(1).to_string()), ("n", &n.to_string()), ("p", label)],
        ),
        None => crate::i18n::fmt(tl!("Diagram {n}, page {p}"), &[("n", &index.saturating_add(1).to_string()), ("p", label)]),
    };
    if doubtful > 0 {
        let n = u64::try_from(doubtful).unwrap_or(u64::MAX);
        text.push_str(", ");
        text.push_str(&crate::i18n::trn(crate::i18n::current(), n, "{n} doubtful square", "{n} doubtful squares"));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every language's keys put six different pieces, or fewer when a letter would be ambiguous;
    /// never two pieces on one key.
    #[test]
    fn each_key_puts_one_piece() {
        for lang in Lang::ALL {
            let letters: Vec<char> = KINDS.iter().filter_map(|k| key_letter(lang, *k)).collect();
            let mut unique = letters.clone();
            unique.sort_unstable();
            unique.dedup();
            assert_eq!(unique.len(), letters.len(), "{lang:?}: {letters:?}");
        }
        assert_eq!(KINDS.map(|k| key_letter(Lang::Pt, k)), ['R', 'D', 'T', 'B', 'C', 'P'].map(Some));
        assert_eq!(KINDS.map(|k| key_letter(Lang::En, k)), ['K', 'Q', 'R', 'B', 'N', 'P'].map(Some));
        assert_eq!(KINDS.map(|k| key_letter(Lang::Ru, k)), ['K', 'Q', 'R', 'B', 'N', 'P'].map(Some), "Cyrillic: the English letters");
        assert_eq!(key_letter(Lang::Nl, PieceKind::Pawn), None, "the Dutch P is the knight");
        assert_eq!(key_piece(Lang::Pt, Key::C, true), Some(Piece { color: Color::Black, kind: PieceKind::Knight }));
        assert_eq!(key_piece(Lang::Pt, Key::K, false), None, "no K in Portuguese");
    }

    /// The keys move the cursor as the board is shown and stop at its edges; letters put pieces
    /// and Delete empties. Undo and Redo step through the changes, a run of move-number changes
    /// being one step, and a change after an undo drops what was undone.
    #[test]
    fn the_keys_edit_and_the_editor_undoes_step_by_step() {
        let mut ed = DiagramEditor::new(pdfcraft_engine::DocId(1), 0, 0, None);
        ed.track();
        assert_eq!(ed.cursor, Square::A8);
        board_keys(&mut ed, &[(Key::ArrowLeft, Modifiers::NONE), (Key::ArrowUp, Modifiers::NONE), (Key::ArrowRight, Modifiers::NONE)]);
        assert_eq!(ed.cursor.to_string(), "b8", "stopped at the corner, then one right");
        ed.flipped = true;
        board_keys(&mut ed, &[(Key::ArrowRight, Modifiers::NONE)]);
        assert_eq!(ed.cursor.to_string(), "a8", "from Black's side, right is towards a");
        ed.flipped = false;
        assert!(board_keys(&mut ed, &[(Key::K, Modifiers::SHIFT)]));
        ed.track();
        assert!(!board_keys(&mut ed, &[(Key::ArrowDown, Modifiers::NONE)]), "a move of the cursor changes nothing");
        assert!(board_keys(&mut ed, &[(Key::Q, Modifiers::NONE)]));
        ed.track();
        for n in 2..6 {
            ed.position.fullmove_number = n;
            ed.track();
        }
        assert_eq!(ed.fen(), "k7/Q7/8/8/8/8/8/8 w - - 0 5");
        ed.undo();
        assert_eq!(ed.fen(), "k7/Q7/8/8/8/8/8/8 w - - 0 1", "the move number's changes are one step");
        ed.undo();
        assert_eq!(ed.fen(), "k7/8/8/8/8/8/8/8 w - - 0 1");
        ed.redo();
        assert_eq!(ed.fen(), "k7/Q7/8/8/8/8/8/8 w - - 0 1");
        board_keys(&mut ed, &[(Key::Delete, Modifiers::NONE)]);
        ed.track();
        assert!(!ed.can_redo(), "a new change drops what was undone");
        ed.undo();
        ed.undo();
        ed.undo();
        assert!(!ed.can_undo());
        assert_eq!(ed.fen(), "8/8/8/8/8/8/8/8 w - - 0 1", "back where it opened");
    }

    /// The board takes its keys alone (letters with Shift too), never with Ctrl or Alt, so
    /// Ctrl+Z, Ctrl+Enter and Alt+← stay what they are.
    #[test]
    fn the_board_takes_only_its_own_keys() {
        assert!(board_key(Key::ArrowLeft, Modifiers::NONE));
        assert!(board_key(Key::D, Modifiers::SHIFT));
        assert!(board_key(Key::V, Modifiers::NONE), "a letter no piece has is taken too: V is no tool there");
        assert!(!board_key(Key::Z, Modifiers::COMMAND));
        assert!(!board_key(Key::ArrowLeft, Modifiers::ALT));
        assert!(!board_key(Key::Enter, Modifiers::COMMAND));
        assert!(!board_key(Key::Tab, Modifiers::NONE), "Tab moves the focus on");
        let all = TabState { undo: true, redo: true, queue: true, focus_in: true };
        assert!(!tab_key(Key::Z, Modifiers::COMMAND, TabState { undo: false, ..all }), "nothing to undo: the document's Undo");
        assert!(tab_key(Key::Z, Modifiers::COMMAND | Modifiers::SHIFT, TabState { undo: false, ..all }));
        assert!(!tab_key(Key::Escape, Modifiers::NONE, TabState { focus_in: false, ..all }), "Esc only with the keyboard in the tab");
        assert!(!tab_key(Key::PageDown, Modifiers::NONE, TabState { queue: false, ..all }), "no queue: the page's Page Down");
    }
}
