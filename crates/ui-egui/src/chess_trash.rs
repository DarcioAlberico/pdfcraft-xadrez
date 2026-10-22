//! Chess ▸ deleted moves kept (plan A8 §7, V5): each time moves leave a game (the deletion
//! `chess_deleted` tells), the game as it was before goes into its document's trash, which the
//! autosave keeps with the games. Undo cannot outlive the session (its history is not saved) while
//! the autosave already holds the game without the moves; the trash is how they come back after a
//! restart: **Deleted moves…** under the list of games opens each one as a new game.
//!
//! Bounded: the newest [`MAX_TRASHED`] of a document, each PGN at most [`MAX_TRASHED_PGN`] bytes
//! (a bigger game is not kept, and the user can still Undo it this session). The trash is the
//! document's, like its games, and goes nowhere else.

use std::collections::VecDeque;

use pdfcraft_engine::chess::pdfcraft_chess::game::Game;
use serde::{Deserialize, Serialize};

/// Most games kept in one document's trash; the oldest goes first.
pub const MAX_TRASHED: usize = 30;

/// Longest PGN kept in the trash.
pub const MAX_TRASHED_PGN: usize = 256 << 10;

/// A game as it was before moves left it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trashed {
    /// The game before the deletion, as PGN.
    #[serde(default)]
    pub pgn: String,
    /// The game's name in the list then.
    #[serde(default)]
    pub name: String,
    /// Its moves then, variations included.
    #[serde(default)]
    pub moves: usize,
    /// How many of them went.
    #[serde(default)]
    pub deleted: usize,
    /// When, in seconds since 1970 (0 when unknown).
    #[serde(default)]
    pub at: u64,
}

/// A document's trash, oldest first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Trash {
    items: VecDeque<Trashed>,
}

/// Now, in seconds since 1970.
#[cfg(not(target_arch = "wasm32"))]
fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Now, in seconds since 1970: milliseconds, finite and non-negative (`as` saturates anything
/// else to a bound).
#[cfg(target_arch = "wasm32")]
fn now() -> u64 {
    (js_sys::Date::now() / 1000.0) as u64
}

impl Trash {
    /// The trash read back from the autosave: the newest [`MAX_TRASHED`] that fit.
    pub fn from_stored(items: Vec<Trashed>) -> Trash {
        let mut trash = Trash::default();
        for item in items {
            trash.push(item);
        }
        trash
    }

    /// Keep `before` (the game before `deleted` of its moves went), named `name`. Whether it
    /// was kept: not when its PGN is too long.
    pub fn keep(&mut self, before: &Game, name: &str, deleted: usize) -> bool {
        let moves = before.tokens().iter().filter(|t| matches!(t, pdfcraft_engine::chess::pdfcraft_chess::game::Token::Move { .. })).count();
        self.push(Trashed { pgn: before.pgn(), name: name.to_owned(), moves, deleted, at: now() })
    }

    fn push(&mut self, item: Trashed) -> bool {
        if item.pgn.len() > MAX_TRASHED_PGN {
            return false;
        }
        self.items.push_back(item);
        while self.items.len() > MAX_TRASHED {
            self.items.pop_front();
        }
        true
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The games kept, newest first, each with its index for [`Trash::take`].
    pub fn newest_first(&self) -> impl Iterator<Item = (usize, &Trashed)> {
        self.items.iter().enumerate().rev()
    }

    /// Take game `index` out of the trash.
    pub fn take(&mut self, index: usize) -> Option<Trashed> {
        self.items.remove(index)
    }

    /// As the autosave stores it, oldest first.
    pub fn stored(&self) -> Vec<Trashed> {
        self.items.iter().cloned().collect()
    }

    /// Changes whenever the trash does, for the autosave's fingerprint.
    pub fn stamp(&self) -> (usize, u64) {
        (self.items.len(), self.items.back().map_or(0, |t| t.at ^ u64::try_from(t.pgn.len()).unwrap_or(0)))
    }
}

/// "e4 e5 Nf3 …": the first moves of a kept game's main line, for the list.
pub fn preview(item: &Trashed) -> String {
    let Ok((game, _)) = Game::from_pgn(&item.pgn) else { return String::new() };
    let line = game.main_line();
    let mut text: Vec<&str> = line.iter().skip(1).take(6).map(|&n| game.san(n)).collect();
    if line.len() > 7 {
        text.push("…");
    }
    text.join(" ")
}

/// How long ago `at` was ("5 min ago"), for the list; nothing when unknown.
#[cfg(not(target_arch = "wasm32"))]
pub fn ago(at: u64) -> String {
    if at == 0 {
        return String::new();
    }
    crate::combine_ui::ago(std::time::UNIX_EPOCH + std::time::Duration::from_secs(at))
}

/// On the web the list says no time (the clock `combine_ui::ago` reads is the desktop's).
#[cfg(target_arch = "wasm32")]
pub fn ago(_at: u64) -> String {
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(pgn: &str) -> Game {
        Game::from_pgn(pgn).unwrap().0
    }

    #[test]
    fn the_newest_are_kept_within_the_caps() {
        let mut t = Trash::default();
        assert!(t.keep(&game("1. e4 e5 (1... c5) 2. Nf3 *"), "Game 1", 2));
        let first = t.newest_first().next().unwrap().1.clone();
        assert_eq!((first.moves, first.deleted, first.name.as_str()), (4, 2, "Game 1"));
        assert!(first.at > 0);
        assert_eq!(preview(&first), "e4 e5 Nf3");
        for i in 0..MAX_TRASHED {
            t.keep(&game("1. d4 *"), &format!("G{i}"), 1);
        }
        assert_eq!(t.len(), MAX_TRASHED);
        assert_eq!(t.newest_first().next().unwrap().1.name, format!("G{}", MAX_TRASHED - 1));
        assert!(t.newest_first().all(|(_, item)| item.name != "Game 1"), "the oldest went first");
        // A game too long to keep is not kept.
        let mut big = Trash::default();
        let mut huge = String::from("{");
        huge.push_str(&"y".repeat(MAX_TRASHED_PGN));
        huge.push_str("} 1. e4 *");
        assert!(!big.push(Trashed { pgn: huge, ..Trashed::default() }));
        assert!(big.is_empty());
    }

    #[test]
    fn a_game_is_taken_out_by_its_index_and_the_stored_order_comes_back() {
        let mut t = Trash::default();
        t.keep(&game("1. e4 *"), "A", 1);
        t.keep(&game("1. d4 *"), "B", 1);
        let stored = t.stored();
        assert_eq!(Trash::from_stored(stored.clone()), t);
        let (index, newest) = t.newest_first().next().map(|(i, item)| (i, item.name.clone())).unwrap();
        assert_eq!(newest, "B");
        assert_eq!(t.take(index).map(|item| item.name), Some("B".to_owned()));
        assert_eq!(t.len(), 1);
        assert_eq!(t.take(7), None);
    }
}
