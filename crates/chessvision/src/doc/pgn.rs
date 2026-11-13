//! PGN of a [`super::ChessDoc`]'s games (plan A9-F4): each score is made the board's game by
//! [`to_game`], the one bridge between the two trees, and written by the one PGN writer
//! ([`pdfcraft_chess::pgn::write`]: wrapped at [`LINE_WIDTH`] columns, a comment before a move
//! after the variations of the move before), the figurines of its comments as letters, as every
//! export of a book writes them. What the bridge leaves out (only a score read from JSON that
//! was tampered with has a move the board refuses, or a start no game can have) is said, never
//! lost in silence: [`Written::warnings`].
//!
//! The document has no PGN reader of its own any more (A2-18): what it writes is read back by
//! the board's, `pdfcraft_chess::game::Game::from_pgn`, which is what every reader of the app
//! uses.

use pdfcraft_chess::game::{PgnWarning, PgnWarningKind};
use pdfcraft_chess::pgn::WriteOptions;
use pdfcraft_chess::pgn_style::PgnStyle;

pub use pdfcraft_chess::pgn::LINE_WIDTH;

use super::games::{GameScore, to_game};

/// The games written, and what was left out of them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Written {
    pub pgn: String,
    /// What writing left out or cut, by game (counted from 0, in the order given).
    pub warnings: Vec<GameWarning>,
}

/// A warning on one game of those written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GameWarning {
    pub game: usize,
    pub warning: PgnWarning,
}

/// Every game, one after the other, separated by a blank line, with what was left out of them.
pub fn write_report<'a>(games: impl IntoIterator<Item = &'a GameScore>, style: PgnStyle) -> Written {
    let options = WriteOptions { style, letters: true };
    let mut made = Written::default();
    for (i, score) in games.into_iter().enumerate() {
        let Some(board) = to_game(score) else {
            made.warnings.push(GameWarning { game: i, warning: PgnWarning { kind: PgnWarningKind::GameLeftOut, at: 0 } });
            continue;
        };
        made.warnings.extend(board.warnings.iter().map(|&warning| GameWarning { game: i, warning }));
        if !made.pgn.is_empty() {
            made.pgn.push('\n');
        }
        made.pgn.push_str(&pdfcraft_chess::pgn::write(&board.game, options));
    }
    made
}

/// Every game, one after the other, separated by a blank line.
pub fn write<'a>(games: impl IntoIterator<Item = &'a GameScore>) -> String {
    write_with(games, PgnStyle::FULL)
}

/// [`write`] without what `style` leaves out of the moves (plan A8 V4).
pub fn write_with<'a>(games: impl IntoIterator<Item = &'a GameScore>, style: PgnStyle) -> String {
    write_report(games, style).pgn
}

/// One game: tags, a blank line, the movetext, a newline.
pub fn write_game(game: &GameScore) -> String {
    write_game_with(game, PgnStyle::FULL)
}

/// [`write_game`] without what `style` leaves out of the moves: the variations past its depth,
/// the comments (the game's first one too), the glyphs. The tags are all written.
pub fn write_game_with(game: &GameScore, style: PgnStyle) -> String {
    write_report([game], style).pgn
}

#[cfg(test)]
pub(crate) mod tests;
