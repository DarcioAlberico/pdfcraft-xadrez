//! Chess ▸ the key map (plan A9-I18): every key the chess panels take, in one list made from the
//! tables they act on — the rail's tools and groups (`chess_tools::RAIL`), moving about the game
//! (`chess_tools::NAV_KEYS`), the signs typed (`chess_annotate::TYPED`), the Diagram tab's keys
//! (`chess_diagram_keys::TAB_KEYS`) — and the app's own one-letter tools (V, H), which the chess
//! keys must leave alone. The key scope takes its keys from it ([`scope_keys`]), the shortcut sheet
//! lists it (`chess_commands`, plan A9-I16), and a test (`collisions`) refuses one key with two
//! meanings.
//!
//! Two keys may share a trigger only when they mean the same thing, or when neither is a one-key
//! shortcut and they belong to different places: Esc, Page Up and Page Down act on the place the
//! keyboard is in (the board's game or the Diagram tab), while a letter or a sign means one thing
//! in every chess panel. Keys planned for later steps are in the map already ([`Binding::planned`]),
//! so their choice is checked now: the shortcut sheet is F1 (`?` is the "poor move" sign typed).
//! The Diagram tab's Original / Read switch is Alt+O (O alone is the rail's OCR area).

use std::sync::OnceLock;

use egui::Key;

use crate::chess_tools::{ChessKey, NAV_KEYS, groups, tools};

/// Where a key acts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Place {
    /// The chess key scope: the board's game, from the Board panel, the Move list, the Chess
    /// panel, the chess window or the glyph palette.
    Scope,
    /// The Diagram tab, while the keyboard is in it or the pointer over it.
    Diagram,
    /// The app, anywhere the page has the keys.
    App,
}

/// What sets a key off: a key with its modifiers, or a sign typed as text (any keyboard layout).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Trigger {
    Key(ChessKey),
    Text(&'static str),
}

impl Trigger {
    /// Whether this is a one-key shortcut (WCAG 2.1.4): one character typed, a letter or a sign.
    #[cfg(test)]
    pub(crate) fn is_character(self) -> bool {
        match self {
            Trigger::Key(k) => k.is_character(),
            Trigger::Text(_) => true,
        }
    }
}

/// One key of the map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Binding {
    pub place: Place,
    pub trigger: Trigger,
    /// What it does: a rail tool's id, a move about the game, a sign, a Diagram tab action.
    pub action: &'static str,
    /// The step that is to give the key its action; until then nothing takes it.
    pub planned: Option<&'static str>,
}

impl Binding {
    const fn key(place: Place, key: ChessKey, action: &'static str) -> Binding {
        Binding { place, trigger: Trigger::Key(key), action, planned: None }
    }
}

/// The shortcut sheet (`chess.shortcuts`, plan A9-I16): F1, since `?` is the "poor move" sign typed.
pub(crate) const SHORTCUT_SHEET: Binding = Binding::key(Place::Scope, ChessKey::plain(Key::F1), "shortcuts");

/// The Diagram tab's Original / Read switch (finding A6-14, step I0): Alt+O, since O alone is the
/// rail's OCR area. One of the tab's keys (`chess_diagram_keys::TAB_KEYS`).
#[cfg(test)]
pub(crate) const ORIGINAL_SWITCH: Binding = Binding::key(Place::Diagram, ChessKey::alt(Key::O), "original");

/// The Board tab's Move field takes the keyboard (plan A9-N7a): M, a one-key shortcut of the chess
/// scope (off with the others, WCAG 2.1.4).
pub(crate) const MOVE_FIELD: Binding = Binding::key(Place::Scope, ChessKey::plain(Key::M), "move-field");

/// The analysis board's questions (plan A9-N7b, findings A6-25 and A8-18), said as a status:
/// Alt+Shift with P (the pieces), with a rank's digit or with a file's letter. Alt+Shift keeps them
/// apart from the rail's letters (B, C, F, G, O, X), the groups' Shift+letter and the diagram
/// editor's piece letters in every language, which take a letter alone or with Shift only.
pub(crate) const SAY_PIECES: ChessKey = ChessKey::alt_shift(Key::P);
pub(crate) const RANK_KEYS: [Key; 8] = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8];
pub(crate) const FILE_KEYS: [Key; 8] = [Key::A, Key::B, Key::C, Key::D, Key::E, Key::F, Key::G, Key::H];

/// The key to take for `key`, at `physical` on the keyboard, pressed with `m`: a rank's digit with
/// Alt+Shift by where it is, since Shift makes most layouts' digits a sign (Alt+Shift+1 arrives as
/// `!` on a US keyboard); any other key as it arrives.
pub(crate) fn question_key(key: Key, physical: Option<Key>, m: egui::Modifiers) -> Key {
    match physical {
        Some(digit) if RANK_KEYS.contains(&digit) && ChessKey::alt_shift(digit).matches(digit, m) => digit,
        _ => key,
    }
}

/// The question `key` pressed with `m` asks the board, when it is one.
pub(crate) fn question(key: Key, m: egui::Modifiers) -> Option<crate::chess_move_field::Typed> {
    use crate::chess_move_field::Typed;
    if SAY_PIECES.matches(key, m) {
        return Some(Typed::Pieces);
    }
    let at = |keys: &[Key; 8]| keys.iter().position(|k| ChessKey::alt_shift(*k).matches(key, m)).and_then(|i| u8::try_from(i).ok());
    at(&RANK_KEYS).map(Typed::Rank).or_else(|| at(&FILE_KEYS).map(Typed::File))
}

/// F6 leaves the chess surface the keyboard is in (`chess_tools::take_f6`).
const LEAVE: Binding = Binding::key(Place::Scope, ChessKey::plain(Key::F6), "leave");

/// The map, made once from the tables.
pub(crate) fn keymap() -> &'static [Binding] {
    static MAP: OnceLock<Vec<Binding>> = OnceLock::new();
    MAP.get_or_init(build)
}

fn build() -> Vec<Binding> {
    let mut map = Vec::new();
    for (key, nav) in NAV_KEYS {
        map.push(Binding::key(Place::Scope, key, nav.action()));
    }
    for tool in tools() {
        map.extend(tool.keys.iter().map(|key| Binding::key(Place::Scope, *key, tool.id)));
    }
    for group in groups() {
        if let Some(letter) = group.letter {
            map.push(Binding::key(Place::Scope, ChessKey::shift(letter), group.id));
        }
    }
    for (sign, _) in crate::chess_annotate::TYPED {
        map.push(Binding { place: Place::Scope, trigger: Trigger::Text(sign), action: sign, planned: None });
    }
    map.push(LEAVE);
    map.push(SHORTCUT_SHEET);
    map.push(MOVE_FIELD);
    map.push(Binding::key(Place::Scope, SAY_PIECES, "say-pieces"));
    map.extend(RANK_KEYS.iter().map(|k| Binding::key(Place::Scope, ChessKey::alt_shift(*k), "say-rank")));
    map.extend(FILE_KEYS.iter().map(|k| Binding::key(Place::Scope, ChessKey::alt_shift(*k), "say-file")));
    for (key, action) in crate::chess_diagram_keys::TAB_KEYS {
        map.push(Binding::key(Place::Diagram, key, action.action()));
    }
    map.push(Binding::key(Place::App, ChessKey::plain(crate::SELECT_KEY), "select-tool"));
    map.push(Binding::key(Place::App, ChessKey::plain(crate::HAND_KEY), "hand-tool"));
    map
}

/// The keys the chess key scope takes for the rail now: not F6 (`chess_tools::take_f6` takes it
/// on its own) nor the keys still planned.
pub(crate) fn scope_keys() -> impl Iterator<Item = ChessKey> {
    keymap().iter().filter(|b| b.place == Place::Scope && b.planned.is_none() && b.action != LEAVE.action).filter_map(|b| match b.trigger {
        Trigger::Key(k) => Some(k),
        Trigger::Text(_) => None,
    })
}

/// The pairs of `map` that would give one key two meanings: the same trigger and different
/// actions, in the same place, or anywhere when both are one-key shortcuts. The key map's test
/// runs it (a key added to one of the tables is checked there).
#[cfg(test)]
pub(crate) fn collisions(map: &[Binding]) -> Vec<(Binding, Binding)> {
    let mut out = Vec::new();
    for (i, a) in map.iter().enumerate() {
        for b in map.iter().skip(i + 1) {
            let shared = a.trigger == b.trigger && a.action != b.action;
            if shared && (a.place == b.place || (a.trigger.is_character() && b.trigger.is_character())) {
                out.push((*a, *b));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No key of the map has two meanings, and it holds what the panels act on.
    #[test]
    fn the_chess_key_map_gives_every_key_one_meaning() {
        let map = keymap();
        assert_eq!(collisions(map), [], "one key, one meaning");
        let has = |place: Place, trigger: Trigger| map.iter().any(|b| b.place == place && b.trigger == trigger);
        for key in [Key::B, Key::C, Key::F, Key::G, Key::O, Key::X, Key::ArrowLeft, Key::PageDown, Key::Escape, Key::F6] {
            assert!(has(Place::Scope, Trigger::Key(ChessKey::plain(key))), "{key:?}");
        }
        assert!(has(Place::Scope, Trigger::Key(ChessKey::shift(Key::S))), "the Structure group's letter");
        assert!(has(Place::Scope, Trigger::Text("?")) && has(Place::Scope, Trigger::Text("+/-")), "the signs typed");
        assert!(has(Place::Diagram, Trigger::Key(ChessKey::command(Key::Enter))));
        assert!(has(Place::App, Trigger::Key(ChessKey::plain(Key::V))) && has(Place::App, Trigger::Key(ChessKey::plain(Key::H))));
        // The scope takes the rail's keys and moving about, not the keys still planned.
        let taken: Vec<ChessKey> = scope_keys().collect();
        assert!(taken.contains(&ChessKey::plain(Key::B)) && taken.contains(&ChessKey::plain(Key::PageUp)));
        assert!(taken.contains(&ChessKey::plain(Key::F1)), "the sheet, since I16");
        // One-key shortcuts are letters and signs, not Esc, Enter, the arrows or Ctrl with a letter.
        assert!(Trigger::Key(ChessKey::shift(Key::X)).is_character() && Trigger::Text("!").is_character());
        assert!(!Trigger::Key(ChessKey::plain(Key::Escape)).is_character());
        assert!(!Trigger::Key(ChessKey::plain(Key::ArrowUp)).is_character());
        assert!(!Trigger::Key(ChessKey::command(Key::Z)).is_character() && !Trigger::Key(ChessKey::alt(Key::O)).is_character());
    }

    /// Plan A9-N7b: the board's questions are Alt+Shift with P, 1–8 and A–H, in the map with no
    /// collision; none is a one-key shortcut, the diagram editor's board takes none of them (its
    /// piece letters, in every language, go alone or with Shift), and each asks what it says.
    #[test]
    fn the_boards_questions_collide_with_nothing() {
        use crate::chess_move_field::Typed;
        let map = keymap();
        assert_eq!(collisions(map), [], "one key, one meaning");
        let questions: Vec<ChessKey> = map
            .iter()
            .filter(|b| b.action.starts_with("say-"))
            .filter_map(|b| match b.trigger {
                Trigger::Key(k) => Some(k),
                Trigger::Text(_) => None,
            })
            .collect();
        assert_eq!(questions.len(), 17);
        let alt_shift = egui::Modifiers { alt: true, shift: true, ..egui::Modifiers::NONE };
        for lang in crate::i18n::Lang::all() {
            for key in std::iter::once(Key::P).chain(RANK_KEYS).chain(FILE_KEYS) {
                assert!(!crate::chess_diagram_keys::editor_takes(key, alt_shift), "{}: {key:?}", lang.code());
                // The editor's letter for a piece, in this language, alone or with Shift, is never one.
                for shift in [false, true] {
                    let m = egui::Modifiers { shift, ..egui::Modifiers::NONE };
                    assert!(question(key, m).is_none(), "{}: {key:?}", lang.code());
                }
            }
        }
        assert!(questions.iter().all(|k| !Trigger::Key(*k).is_character()));
        assert_eq!(question(Key::P, alt_shift), Some(Typed::Pieces));
        assert_eq!(question(Key::Num4, alt_shift), Some(Typed::Rank(3)));
        assert_eq!(question(Key::E, alt_shift), Some(Typed::File(4)));
        assert_eq!(question(Key::E, egui::Modifiers::ALT), None, "Alt alone is not one");
        assert!(scope_keys().any(|k| k == SAY_PIECES), "the scope takes them");
        // A rank's digit by where it is: Alt+Shift+1 arrives as `!` on a US keyboard.
        assert_eq!(question_key(Key::Exclamationmark, Some(Key::Num1), alt_shift), Key::Num1);
        assert_eq!(question_key(Key::Exclamationmark, Some(Key::Num1), egui::Modifiers::SHIFT), Key::Exclamationmark, "Shift alone: the sign");
        assert_eq!(question_key(Key::A, Some(Key::Q), alt_shift), Key::A, "a letter as the layout gives it (AZERTY)");
        assert_eq!(question_key(Key::Num9, Some(Key::Num9), alt_shift), Key::Num9, "no ninth rank");
    }

    /// The keys the plan first gave the shortcut sheet (`?`) and the Original / Read switch (`O`)
    /// are refused, each naming what it collides with.
    #[test]
    fn the_collision_check_refuses_question_mark_and_o() {
        let swap = |old: Binding, new: Binding| keymap().iter().map(|b| if *b == old { new } else { *b }).collect::<Vec<_>>();
        let sheet = Binding { trigger: Trigger::Text("?"), ..SHORTCUT_SHEET };
        let found = collisions(&swap(SHORTCUT_SHEET, sheet));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found.iter().all(|(a, b)| [a.action, b.action] == ["?", "shortcuts"]), "the poor move sign: {found:?}");
        let original = Binding { trigger: Trigger::Key(ChessKey::plain(Key::O)), ..ORIGINAL_SWITCH };
        let found = collisions(&swap(ORIGINAL_SWITCH, original));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found.iter().all(|(a, b)| [a.action, b.action] == ["ocr-area", "original"]), "the OCR area: {found:?}");
        // A key of the app taken by the board would shadow it.
        let h = keymap().iter().copied().chain([Binding::key(Place::Scope, ChessKey::plain(Key::H), "highlight")]).collect::<Vec<_>>();
        assert_eq!(collisions(&h).len(), 1);
        // Esc, Page Up and Page Down act on the place the keyboard is in: no collision.
        assert!(keymap().iter().filter(|b| b.trigger == Trigger::Key(ChessKey::plain(Key::Escape))).count() >= 2);
    }
}
