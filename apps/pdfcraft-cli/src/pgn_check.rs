//! The fuzzing child's PGN checks (`check-one --chess`, plan A9-Q3a): a page's text read as a
//! PGN file is, by the same functions that load a book's PGN ([`split_pgn`], then
//! [`Game::from_pgn_report`] game by game, and [`ReferenceIndex::from_pgn`]).
//!
//! Besides not panicking, the games [`split_pgn`] cuts must be the text itself, in order (what is
//! between two games stays with the one before), and each must be one game as the reader reads
//! it: a tag after a piece's moves is a game the reader never reaches, lost without a count. That
//! is how a brace in a `;` comment merged every game after it (A1-05, before plan A9-F2).

use pdfcraft_engine::chess::pdfcraft_chess::game::pgn_scan::{ScanKind, scan};
use pdfcraft_engine::chess::pdfcraft_chess::game::{Game, split_pgn};
use pdfcraft_engine::chess::pdfcraft_chess::pgn::without_bom;
use pdfcraft_engine::chess::pdfcraft_chessvision::doc::reference::{MAX_REFERENCE_GAMES, ReferenceIndex};

/// Most games of one page's text read (the cut says when there were more).
const MAX_GAMES: usize = 64;

/// What is wrong with how `text` is cut into games and read, or `None`.
pub(crate) fn problem(text: &str) -> Option<String> {
    let games = split_pgn(text, MAX_GAMES);
    if let Some(problem) = cut_problem(text, &games, MAX_GAMES) {
        return Some(problem);
    }
    for game in games.iter().take(MAX_GAMES) {
        let _ = Game::from_pgn_report(game);
    }
    // The book's PGN keeps every game it cut, one that could not be read with its error.
    if let Ok(index) = ReferenceIndex::from_pgn(text) {
        let cut = split_pgn(text, MAX_REFERENCE_GAMES).len().min(MAX_REFERENCE_GAMES);
        if index.games().len() != cut {
            return Some(format!("ReferenceIndex::from_pgn kept {} games of the {cut} split_pgn cut", index.games().len()));
        }
    }
    None
}

/// What is wrong with `games` as [`split_pgn`]'s cut of `text` at most `max` games, or `None`.
fn cut_problem(text: &str, games: &[&str], max: usize) -> Option<String> {
    let text = without_bom(text);
    let joined = games.concat();
    let Some(rest) = text.strip_prefix(joined.as_str()) else {
        return Some("split_pgn's games are not the text, in order".to_owned());
    };
    // Past `max` games the rest is not cut; otherwise only blank text may follow the last game.
    if games.len() <= max && !rest.trim().is_empty() {
        return Some(format!("split_pgn left out the text after its last game ({} bytes)", rest.len()));
    }
    for (i, game) in games.iter().enumerate() {
        let mut moves = false;
        for token in scan(game) {
            if token.kind == ScanKind::Tag && moves {
                return Some(format!("split_pgn merged games: game {} of {} has a tag after its moves", i + 1, games.len()));
            }
            moves |= token.kind.is_movetext();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A1-05 (fixed by plan A9-F2a): a `;` comment or a `%` line holding `{` merged every game
    /// after it into the game before, which the reader read up to the next tag. Cut so, the
    /// check names the merge; as `split_pgn` cuts it now, nothing.
    #[test]
    fn a_merged_game_is_found() {
        let text = "[Event \"A\"]\n1. e4 e5 ; White plays {the king's pawn\n2. Nf3 *\n[Event \"B\"]\n1. d4 d5 *\n[Event \"C\"]\n1. c4 *\n";
        let merged = [text];
        let why = cut_problem(text, &merged, MAX_GAMES).expect("the merge is found");
        assert!(why.contains("merged games: game 1 of 1"), "{why}");
        assert_eq!(problem(text), None);
        assert_eq!(split_pgn(text, MAX_GAMES).len(), 3);
    }

    /// Games out of order, text left out, a cut that is no cut: each is named.
    #[test]
    fn a_cut_that_is_not_the_text_is_found() {
        let text = "[Event \"A\"]\n1. e4 *\n[Event \"B\"]\n1. d4 *\n";
        let (a, b) = text.split_at(text.find("[Event \"B\"]").unwrap());
        assert_eq!(cut_problem(text, &[a, b], MAX_GAMES), None);
        assert!(cut_problem(text, &[b, a], MAX_GAMES).unwrap().contains("not the text"));
        assert!(cut_problem(text, &[a], MAX_GAMES).unwrap().contains("left out"));
        // Past the most cut, the rest is not the cut's.
        assert_eq!(cut_problem(text, &[a], 0), None);
        // A byte order mark is not text of a game, and blank text after the last game is none.
        assert_eq!(problem(&format!("\u{feff}{text}\n \n")), None);
        assert_eq!(problem(""), None);
    }
}
