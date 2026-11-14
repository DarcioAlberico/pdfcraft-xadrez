//! A book's PGN made up, for the index's tests and its bench (plan A9-I3b): `games` games, each a
//! main line of [`MAIN_PLIES`] random legal moves and [`VARIATIONS`] variations of
//! [`VARIATION_PLIES`] from points of it, about 240 moves a game — the shape of the PGN the R8
//! measure used (4,096 games, about a million moves). The same `seed` gives the same text.
//!
//! Included by the crate's tests and by `examples/index_bench.rs` (`#[path]`); it uses only
//! `pdfcraft_chess::game::Game`'s public moves and writer.

use pdfcraft_chess::game::Game;

/// Moves of a game's main line.
pub const MAIN_PLIES: usize = 60;

/// Variations a game has, each from a move of its main line.
pub const VARIATIONS: usize = 6;

/// Moves of a variation.
pub const VARIATION_PLIES: usize = 30;

/// The next number of a xorshift sequence.
fn next(state: &mut u64) -> u64 {
    let mut x = (*state).max(1);
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

/// Play one of the legal moves at random; whether there was one.
fn random_move(game: &mut Game, state: &mut u64) -> bool {
    let moves = game.legal_moves();
    if moves.is_empty() {
        return false;
    }
    let pick = (next(state) % moves.len() as u64) as usize;
    moves.get(pick).is_some_and(|m| game.play_san(m).is_ok())
}

/// One game, its main line and its variations.
pub fn game(number: usize, state: &mut u64) -> Game {
    let mut game = Game::standard();
    let _ = game.set_tag("Event", &format!("Chapter {}", number / 64 + 1));
    let _ = game.set_tag("White", &format!("Game {}", number + 1));
    let mut line = vec![0];
    for _ in 0..MAIN_PLIES {
        if !random_move(&mut game, state) {
            break;
        }
        line.push(game.current());
    }
    for _ in 0..VARIATIONS {
        // From a move of the main line before its last: another move there starts a variation.
        let from = line.get((next(state) % line.len().saturating_sub(1).max(1) as u64) as usize).copied().unwrap_or(0);
        game.go_to_node(from);
        for _ in 0..VARIATION_PLIES {
            if !random_move(&mut game, state) {
                break;
            }
        }
    }
    game
}

/// `games` games as one PGN text.
pub fn book_pgn(games: usize, seed: u64) -> String {
    let mut state = seed;
    let mut out = String::new();
    for number in 0..games {
        out.push_str(&game(number, &mut state).pgn());
        out.push_str("\n\n");
    }
    out
}
