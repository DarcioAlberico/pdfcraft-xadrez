//! A PGN file cut into its games, each one's text as written, for the readers that take one
//! game at a time ([`super::Game::from_pgn_report`]).

/// The games of a PGN file, one text each: a tag line after moves starts the next game (a `[`
/// inside a `{…}` comment doesn't). At most `max` games and one more, so the caller can tell
/// there were more. A byte order mark the text starts with (text pasted, or passed through JSON)
/// is left out: it would hide the first tag line.
pub fn split_pgn(text: &str, max: usize) -> Vec<&str> {
    let text = crate::pgn::without_bom(text);
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

#[cfg(test)]
mod tests {
    use super::*;

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

    /// Plan A9-F1 (A1-01): a byte order mark before the first tag no longer makes that line a
    /// game of its own, so a problem file keeps its first FEN.
    #[test]
    fn a_byte_order_mark_does_not_split_off_the_first_tag() {
        let text = "\u{feff}[FEN \"4k3/8/8/8/8/8/8/4K2R w K - 0 1\"]\n[SetUp \"1\"]\n\n1. O-O *\n";
        let games = split_pgn(text, 10);
        assert_eq!(games.len(), 1, "{games:?}");
        assert!(games[0].starts_with("[FEN"), "{games:?}");
        let (game, stopped) = crate::game::Game::from_pgn(games[0]).unwrap();
        assert_eq!((game.start_fen().as_str(), stopped), ("4k3/8/8/8/8/8/8/4K2R w K - 0 1", None));
    }
}
