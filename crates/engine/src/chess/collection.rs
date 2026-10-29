//! Chess ▸ Game collections, below the interface (PGN toolbar spec §4.7; wave 10, U2): a
//! document's games as an ordered list with one of them shown, and what is done to the list.
//!
//! The app keeps one list per document (`ui-egui`'s `chess_collection`, whose entries carry the
//! board, its undo history and the diagram each game came from, and which autosaves the list);
//! the automation tool `chess_games` keeps one for a call, of plain [`Game`]s read from a PGN.
//! Both go through [`GameList`], so the order, the game shown after each change and the blank
//! game that a deleted last game leaves are the same in both:
//!
//! - **Never empty.** The game shown is held apart from the games before and after it, so there
//!   is always one; deleting the last game leaves a blank one ([`ListedGame::blank`]).
//! - **The game shown stays shown** when others move, are added before it or are deleted; when it
//!   is deleted, the game after it is shown (the one before, at the end of the list).
//! - **Adding to a list that holds only a blank game** puts the new game in its place.
//!
//! A game is named by its White, Black and Event tags, and placed by its Site, Date and Round
//! ([`GameNames`]), so its name is in its PGN; [`pgn_date_ok`] says whether a date is one PGN reads.
//! [`split_pgn`] cuts a PGN file into its games, and [`game_from`] starts a new game at a move of
//! another ("New game from this position").

use pdfcraft_chess::game::{Game, GameError, NodeId};
// In `chess` so that `chessvision` cuts a book's PGN the same way (plan PGN do livro R1).
pub use pdfcraft_chess::game::split_pgn;

/// Most games a document's collection holds: what a PGN import reads, the tool accepts and the
/// app autosaves.
pub const MAX_GAMES: usize = 256;

/// A game as a [`GameList`] holds it.
pub trait ListedGame {
    /// A blank game: the standard start, nothing played (the list's game when it has no other).
    fn blank() -> Self;
    /// Never touched: a game added to a list holding only this one takes its place.
    fn is_blank(&self) -> bool;
    /// No longer the game shown: drop what only the game shown needs. Nothing by default.
    fn hidden(&mut self) {}
}

impl ListedGame for Game {
    fn blank() -> Game {
        Game::standard()
    }

    fn is_blank(&self) -> bool {
        self.is_empty() && self.pgn() == Game::standard().pgn()
    }
}

/// An ordered list of games, one of them shown. Never empty.
pub struct GameList<T> {
    before: Vec<T>,
    active: T,
    after: Vec<T>,
}

impl<T: ListedGame> Default for GameList<T> {
    fn default() -> GameList<T> {
        GameList::new()
    }
}

impl<T: ListedGame> GameList<T> {
    /// One blank game.
    pub fn new() -> GameList<T> {
        GameList { before: Vec::new(), active: T::blank(), after: Vec::new() }
    }

    /// The games `games` in order, game `active` shown (the first when out of range); one blank
    /// game when there are none.
    pub fn from_games(games: Vec<T>, active: usize) -> GameList<T> {
        let mut list = GameList::new();
        let mut games = games.into_iter();
        if let Some(first) = games.next() {
            list.active = first;
            list.after = games.collect();
            if !list.switch_to(active) {
                list.hide_others();
            }
        }
        list
    }

    /// How many games.
    pub fn len(&self) -> usize {
        self.before.len().saturating_add(1).saturating_add(self.after.len())
    }

    /// Never empty (the game shown is always there); here for completeness.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// The place of the game shown (0-based).
    pub fn active_index(&self) -> usize {
        self.before.len()
    }

    /// The games, in list order.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.before.iter().chain(std::iter::once(&self.active)).chain(self.after.iter())
    }

    /// The games, in list order, to change.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.before.iter_mut().chain(std::iter::once(&mut self.active)).chain(self.after.iter_mut())
    }

    /// The game shown.
    pub fn active(&self) -> &T {
        &self.active
    }

    pub fn active_mut(&mut self) -> &mut T {
        &mut self.active
    }

    /// Game `index` (0-based).
    pub fn get(&self, index: usize) -> Option<&T> {
        self.iter().nth(index)
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        let at = self.active_index();
        match index.cmp(&at) {
            std::cmp::Ordering::Less => self.before.get_mut(index),
            std::cmp::Ordering::Equal => Some(&mut self.active),
            std::cmp::Ordering::Greater => self.after.get_mut(index.saturating_sub(at).saturating_sub(1)),
        }
    }

    /// The place of the first game `matches` takes.
    pub fn position(&self, matches: impl FnMut(&T) -> bool) -> Option<usize> {
        self.iter().position(matches)
    }

    /// Every game but the one shown is told it is hidden.
    fn hide_others(&mut self) {
        for game in self.before.iter_mut().chain(self.after.iter_mut()) {
            game.hidden();
        }
    }

    /// Show game `index` (0-based). Whether there is one.
    pub fn switch_to(&mut self, index: usize) -> bool {
        let at = self.active_index();
        if index >= self.len() || index == at {
            return index == at;
        }
        if index < at {
            // before = [..index] [index] [index+1..]: the tail and the game shown go after.
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
        self.hide_others();
        true
    }

    /// Change the list as one vector, `change` given the games in order and the place of the
    /// game shown (which it keeps pointing at the game to show). An empty list gets a blank game;
    /// a place past the end shows the last game.
    fn rearrange<R>(&mut self, change: impl FnOnce(&mut Vec<T>, &mut usize) -> R) -> R {
        let mut at = self.active_index();
        // A stand-in while the games are one vector: replaced below, never shown.
        let shown = std::mem::replace(&mut self.active, T::blank());
        let mut all = std::mem::take(&mut self.before);
        all.push(shown);
        all.append(&mut self.after);
        let out = change(&mut all, &mut at);
        if all.is_empty() {
            all.push(T::blank());
        }
        let at = at.min(all.len().saturating_sub(1));
        self.after = all.split_off(at.saturating_add(1).min(all.len()));
        self.active = all.pop().unwrap_or_else(T::blank);
        self.before = all;
        self.hide_others();
        out
    }

    /// Move game `from` to place `to` (both 0-based, `to` its place once moved); the game shown
    /// stays shown. Whether it moved.
    pub fn move_to(&mut self, from: usize, to: usize) -> bool {
        let len = self.len();
        if from >= len || to >= len || from == to {
            return false;
        }
        self.rearrange(|all, at| {
            if from >= all.len() || to >= all.len() {
                return;
            }
            let game = all.remove(from);
            all.insert(to, game);
            if *at == from {
                *at = to;
            } else if from < *at && to >= *at {
                *at = at.saturating_sub(1);
            } else if from > *at && to <= *at {
                *at = at.saturating_add(1);
            }
        });
        true
    }

    /// Move game `index` one place up (towards the start) or down. Whether it moved.
    pub fn move_game(&mut self, index: usize, up: bool) -> bool {
        let other = if up { index.checked_sub(1) } else { index.checked_add(1) };
        other.is_some_and(|to| self.move_to(index, to))
    }

    /// Put `game` at place `index` (the end when past it); shown when `show`, else the game shown
    /// stays shown. Returns its place.
    pub fn insert(&mut self, index: usize, game: T, show: bool) -> usize {
        self.rearrange(|all, at| {
            let place = index.min(all.len());
            all.insert(place, game);
            if show {
                *at = place;
            } else if place <= *at {
                *at = at.saturating_add(1);
            }
            place
        })
    }

    /// Put `game` right after game `index` and show it. Returns its place.
    pub fn insert_after(&mut self, index: usize, game: T) -> usize {
        self.insert(index.saturating_add(1), game, true)
    }

    /// Delete game `index` and return it. The game shown stays shown; when it is the one deleted,
    /// the game after it is shown (the one before, at the end of the list), and deleting the last
    /// game leaves a blank one. `None` when there is no such game.
    pub fn delete(&mut self, index: usize) -> Option<T> {
        if index >= self.len() {
            return None;
        }
        self.rearrange(|all, at| {
            let gone = (index < all.len()).then(|| all.remove(index));
            if index < *at {
                *at = at.saturating_sub(1);
            } else if index == *at {
                *at = index.min(all.len().saturating_sub(1));
            }
            gone
        })
    }

    /// Add `game` at the end and show it. A blank game alone in the list gives way to it.
    pub fn add(&mut self, game: T) {
        if self.len() == 1 && self.active.is_blank() {
            self.active = game;
            return;
        }
        let old = std::mem::replace(&mut self.active, game);
        self.before.push(old);
        self.before.append(&mut self.after);
    }

    /// The games in order, and the place of the one shown.
    pub fn into_games(self) -> (Vec<T>, usize) {
        let at = self.active_index();
        let mut all = self.before;
        all.push(self.active);
        all.extend(self.after);
        (all, at)
    }
}

/// What a game is named by in a list (its White, Black and Event tags) and the other tags of the
/// Seven Tag Roster a list edits with them (Site, Date, Round): empty when unknown (`?`, and the
/// date `????.??.??`, included).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GameNames {
    pub white: String,
    pub black: String,
    pub event: String,
    pub site: String,
    /// `YYYY.MM.DD`, `?` for each unknown digit ([`pgn_date_ok`]).
    pub date: String,
    pub round: String,
}

impl GameNames {
    /// The names `game` has.
    pub fn of(game: &Game) -> GameNames {
        let tag = |t: &str| game.tag(t).map(str::trim).filter(|v| *v != "?" && *v != UNKNOWN_DATE).unwrap_or_default().to_owned();
        GameNames { white: tag("White"), black: tag("Black"), event: tag("Event"), site: tag("Site"), date: tag("Date"), round: tag("Round") }
    }

    /// The tags to set so that a game named `now` is named `self`: each tag that changes with its
    /// new value (trimmed; empty removes the tag).
    pub fn changes_from(&self, now: &GameNames) -> Vec<(&'static str, String)> {
        [
            ("White", &now.white, &self.white),
            ("Black", &now.black, &self.black),
            ("Event", &now.event, &self.event),
            ("Site", &now.site, &self.site),
            ("Date", &now.date, &self.date),
            ("Round", &now.round, &self.round),
        ]
        .into_iter()
        .filter(|(_, old, new)| old.trim() != new.trim())
        .map(|(tag, _, new)| (tag, new.trim().to_owned()))
        .collect()
    }

    /// The name these make, when they make one: "White–Black", else the player known, else the
    /// event.
    pub fn display(&self) -> Option<String> {
        let known = |v: &str| Some(v.trim().to_owned()).filter(|v| !v.is_empty());
        match (known(&self.white), known(&self.black)) {
            (Some(w), Some(b)) => Some(format!("{w}–{b}")),
            (Some(one), None) | (None, Some(one)) => Some(one),
            (None, None) => known(&self.event),
        }
    }
}

/// The date PGN writes when nothing of it is known.
const UNKNOWN_DATE: &str = "????.??.??";

/// Whether `date` (trimmed) is a date as PGN writes one, `YYYY.MM.DD` with `?` for each unknown
/// digit (`1992.??.??`, `19??.??.??`), or empty (no date). A month or a day is known whole or not
/// at all, and a known one is a real one: `04.31` is not, `02.29` only in a leap year or an
/// unknown one.
pub fn pgn_date_ok(date: &str) -> bool {
    let date = date.trim();
    if date.is_empty() {
        return true;
    }
    let b = date.as_bytes();
    if b.len() != 10 || b.get(4) != Some(&b'.') || b.get(7) != Some(&b'.') {
        return false;
    }
    let (Some(year), Some(month), Some(day)) = (b.get(0..4), b.get(5..7), b.get(8..10)) else { return false };
    if !year.iter().all(|c| c.is_ascii_digit() || *c == b'?') {
        return false;
    }
    let (Some(month), Some(day)) = (date_part(month), date_part(day)) else { return false };
    // Four ASCII digits: at most 9999.
    let known_year = year.iter().all(u8::is_ascii_digit).then(|| year.iter().fold(0u32, |n, c| n * 10 + u32::from(c - b'0')));
    let leap = known_year.is_none_or(|y| (y % 4 == 0 && y % 100 != 0) || y % 400 == 0);
    let longest = match month {
        None => 31,
        Some(2) if leap => 29,
        Some(2) => 28,
        Some(4 | 6 | 9 | 11) => 30,
        Some(1..=12) => 31,
        Some(_) => return false,
    };
    day.is_none_or(|d| (1..=longest).contains(&d))
}

/// A month or a day of a PGN date: `Some(None)` when unknown (`??`), `None` when half known or
/// not digits.
fn date_part(p: &[u8]) -> Option<Option<u32>> {
    match p {
        [b'?', b'?'] => Some(None),
        // Two ASCII digits: at most 99.
        [a, b] if a.is_ascii_digit() && b.is_ascii_digit() => Some(Some(u32::from(a - b'0') * 10 + u32::from(b - b'0'))),
        _ => None,
    }
}

/// Name `game` `names` (its White, Black, Event, Site, Date and Round tags; an empty name removes
/// its tag). Whether anything changed. A game whose tags are full refuses a new one, and the
/// others still change. The date is set as given: check it with [`pgn_date_ok`] first.
pub fn rename(game: &mut Game, names: &GameNames) -> bool {
    let changes = names.changes_from(&GameNames::of(game));
    for (tag, value) in &changes {
        // The six names are valid tag names; only a full set of tags refuses one.
        let _ = game.set_tag(tag, value);
    }
    !changes.is_empty()
}

/// A new game starting at the position of move `node` of `game` (the move shown when `None`, or
/// when the game has no such move). Its PGN has the position as its `SetUp`/`FEN` tags.
pub fn game_from(game: &Game, node: Option<NodeId>) -> Result<Game, GameError> {
    let fen = node.and_then(|n| game.fen_at(n)).unwrap_or_else(|| game.fen());
    Game::from_fen(&fen)
}

/// Moves of the main line, counted as a scoresheet does (a move each side).
pub fn scoresheet_moves(game: &Game) -> usize {
    game.main_line().len().saturating_sub(1).div_ceil(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(moves: &[&str]) -> Game {
        let mut g = Game::standard();
        for m in moves {
            g.play_uci(m).unwrap();
        }
        g
    }

    fn texts(list: &GameList<Game>) -> Vec<String> {
        list.iter().map(Game::movetext).collect()
    }

    fn three() -> GameList<Game> {
        let mut list = GameList::new();
        for m in ["e2e4", "d2d4", "c2c4"] {
            list.add(game(&[m]));
        }
        list
    }

    #[test]
    fn a_blank_game_gives_way_and_switching_keeps_the_order() {
        let mut list = three();
        assert_eq!(texts(&list), ["1. e4", "1. d4", "1. c4"], "the blank first game gave way");
        assert_eq!(list.active_index(), 2);
        for to in [0, 1, 2, 0, 2, 1] {
            assert!(list.switch_to(to));
            assert_eq!(list.active_index(), to);
            assert_eq!(texts(&list), ["1. e4", "1. d4", "1. c4"]);
        }
        assert!(!list.switch_to(3) && !list.switch_to(usize::MAX));
        let (games, at) = list.into_games();
        assert_eq!((games.len(), at), (3, 1));
        let back = GameList::from_games(games, 7);
        assert_eq!(back.active_index(), 0, "out of range: the first");
        assert_eq!(GameList::<Game>::from_games(Vec::new(), 3).len(), 1, "none: one blank game");
    }

    #[test]
    fn moving_to_any_place_keeps_the_game_shown() {
        // Every move of every game to every place, with every game shown: the order is the
        // vector's remove-then-insert, and the game shown is the same game.
        for shown in 0..4 {
            for from in 0..4 {
                for to in 0..4 {
                    let mut list = GameList::new();
                    for m in ["e2e4", "d2d4", "c2c4", "g1f3"] {
                        list.add(game(&[m]));
                    }
                    assert!(list.switch_to(shown));
                    let mut expected = texts(&list);
                    let shown_text = list.active().movetext();
                    let moved = list.move_to(from, to);
                    assert_eq!(moved, from != to, "{from}→{to}");
                    if moved {
                        let g = expected.remove(from);
                        expected.insert(to, g);
                    }
                    assert_eq!(texts(&list), expected, "{from}→{to} with {shown} shown");
                    assert_eq!(list.active().movetext(), shown_text, "{from}→{to} with {shown} shown");
                }
            }
        }
        let mut list = three();
        assert!(!list.move_to(3, 0) && !list.move_to(0, 3) && !list.move_to(usize::MAX, 0));
        assert!(!list.move_game(0, true) && !list.move_game(2, false) && !list.move_game(usize::MAX, false));
        assert!(list.move_game(0, false));
        assert_eq!(texts(&list), ["1. d4", "1. e4", "1. c4"]);
    }

    #[test]
    fn deleting_returns_the_game_and_inserting_puts_it_back() {
        let mut list = three();
        assert!(list.switch_to(1));
        let gone = list.delete(1).unwrap();
        assert_eq!((texts(&list), list.active_index()), (vec!["1. e4".to_owned(), "1. c4".into()], 1), "the game after it is shown");
        assert_eq!(list.insert(1, gone, true), 1);
        assert_eq!((texts(&list), list.active_index()), (vec!["1. e4".to_owned(), "1. d4".into(), "1. c4".into()], 1));
        // Put back before the game shown without showing it: the game shown stays shown.
        let first = list.delete(0).unwrap();
        assert_eq!(list.active_index(), 0);
        assert_eq!(list.insert(0, first, false), 0);
        assert_eq!((list.active_index(), list.active().movetext()), (1, "1. d4".to_owned()));
        assert_eq!(list.insert(usize::MAX, game(&["b2b3"]), false), 3, "past the end: at the end");
        assert!(list.delete(4).is_none() && list.delete(usize::MAX).is_none());
        while list.len() > 1 {
            assert!(list.delete(0).is_some());
        }
        assert!(list.delete(0).is_some());
        assert!(list.active().is_blank(), "deleting the last game leaves a blank one");
    }

    #[test]
    fn names_come_from_the_tags_and_a_rename_sets_only_what_changes() {
        let mut g = game(&["e2e4"]);
        assert_eq!(GameNames::of(&g), GameNames::default());
        assert_eq!(GameNames::default().display(), None);
        let names = GameNames { white: " Werle ".into(), black: "Jianu".into(), event: "Wijk aan Zee".into(), ..GameNames::default() };
        assert!(rename(&mut g, &names));
        assert_eq!(GameNames::of(&g).display().as_deref(), Some("Werle–Jianu"));
        assert!(!rename(&mut g, &names), "the same names change nothing");
        assert_eq!(names.changes_from(&GameNames::of(&g)), Vec::new());
        assert!(rename(&mut g, &GameNames { event: "Training".into(), ..GameNames::default() }));
        assert_eq!(GameNames::of(&g).display().as_deref(), Some("Training"));
        assert!(rename(&mut g, &GameNames::default()));
        assert!(g.tags().is_empty(), "empty names remove the tags");
        g.set_tag("White", "?").unwrap();
        assert_eq!(GameNames::of(&g).white, "", "an unknown player is no name");
        // Site, Date and Round go with them, in one rename.
        let placed = GameNames { site: "Wijk aan Zee NED".into(), date: "2025.01.18".into(), round: " 6 ".into(), ..GameNames::default() };
        assert_eq!(placed.changes_from(&GameNames::of(&g)).len(), 3);
        assert!(rename(&mut g, &placed));
        assert_eq!((g.tag("Site"), g.tag("Date"), g.tag("Round")), (Some("Wijk aan Zee NED"), Some("2025.01.18"), Some("6")));
        assert_eq!(GameNames::of(&g).display(), None, "they name no game");
        g.set_tag("Date", "????.??.??").unwrap();
        assert_eq!(GameNames::of(&g).date, "", "an unknown date is no date");
    }

    #[test]
    fn a_pgn_date_is_year_month_day_with_question_marks_for_what_is_unknown() {
        for ok in
            ["", "  ", "2025.01.18", " 1992.??.?? ", "19??.??.??", "????.??.??", "????.02.29", "2000.02.29", "2024.02.29", "1999.12.31", "2025.??.31"]
        {
            assert!(pgn_date_ok(ok), "{ok:?}");
        }
        for bad in [
            "2025-01-18",
            "2025.1.18",
            "25.01.18",
            "2025.01.180",
            "2025.13.01",
            "2025.00.10",
            "2025.01.00",
            "2025.01.32",
            "2025.04.31",
            "1900.02.29",
            "2023.02.29",
            "2025.0?.10",
            "2025.01.1?",
            "20x5.01.01",
            "２０２５.01.18",
            "2025.٠١.18",
            "yesterday",
        ] {
            assert!(!pgn_date_ok(bad), "{bad:?}");
        }
    }

    #[test]
    fn a_game_from_a_move_starts_at_its_position() {
        let mut g = game(&["e2e4", "e7e5"]);
        let after_e4 = g.main_line()[1];
        assert_eq!(game_from(&g, None).unwrap().start_fen(), g.fen());
        assert_eq!(game_from(&g, Some(after_e4)).unwrap().start_fen(), g.fen_at(after_e4).unwrap());
        assert_eq!(game_from(&g, Some(usize::MAX)).unwrap().start_fen(), g.fen(), "no such move: the move shown");
        g.go_to_node(after_e4);
        assert_eq!(game_from(&g, None).unwrap().start_fen(), g.fen_at(after_e4).unwrap());
        assert_eq!(scoresheet_moves(&game(&["e2e4", "e7e5", "g1f3"])), 2);
    }
}
