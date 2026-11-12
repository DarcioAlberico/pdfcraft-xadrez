//! A PGN file cut into its games, each one's text as written, for the readers that take one
//! game at a time ([`super::Game::from_pgn_report`]).

use super::pgn_scan::{ScanKind, scan};

/// The games of a PGN file, one text each: a tag after a game's moves starts the next game. What
/// is a tag and what is a comment is [`scan`]'s decision, the one the reader takes, so a `[` in
/// a `{…}` comment, or a brace in a `;` comment or a `%` line, splits nothing and merges
/// nothing (A1-05). At most `max` games and one more, so the caller can tell there were more. A
/// byte order mark the text starts with (text pasted, or passed through JSON) is left out: it
/// would hide the first tag line.
pub fn split_pgn(text: &str, max: usize) -> Vec<&str> {
    let text = crate::pgn::without_bom(text);
    let mut games = Vec::new();
    // Where the game being read starts, and whether its moves have begun.
    let (mut start, mut moves) = (0usize, false);
    for token in scan(text) {
        if token.kind == ScanKind::Tag && moves {
            // The game ends at the start of the tag's line, so what is between (spaces, a line
            // break) stays with the game before, as it was written.
            let at = line_start(text, token.range.start);
            if let Some(game) = text.get(start..at) {
                games.push(game);
            }
            if games.len() > max {
                return games;
            }
            (start, moves) = (at, false);
        }
        moves |= token.kind.is_movetext();
    }
    if let Some(game) = text.get(start..).filter(|g| !g.trim().is_empty()) {
        games.push(game);
    }
    games
}

/// The start of the line `at` is on.
fn line_start(text: &str, at: usize) -> usize {
    text.get(..at).and_then(|before| before.rfind('\n')).map_or(0, |i| i.saturating_add(1))
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

    /// Plan A9-F2a (A1-05, A7-01): a brace in a `;` comment or a `%` line opens no comment, so
    /// the games after it are still games of their own, each read whole.
    #[test]
    fn a_brace_in_a_line_comment_or_an_escape_merges_no_games() {
        for text in [
            "[Event \"A\"]\n1. e4 e5 ; White plays {the king's pawn\n2. Nf3 *\n[Event \"B\"]\n1. d4 d5 *\n[Event \"C\"]\n1. c4 *\n",
            "[Event \"A\"]\n1. e4 e5\n% exported {by a tool\n2. Nf3 *\n[Event \"B\"]\n1. d4 d5 *\n[Event \"C\"]\n1. c4 *\n",
        ] {
            let games = split_pgn(text, 10);
            assert_eq!(games.len(), 3, "{games:#?}");
            assert_eq!(games.concat(), text);
            for (game, (event, moves)) in games.iter().zip([("A", "1. e4 e5 2. Nf3"), ("B", "1. d4 d5"), ("C", "1. c4")]) {
                let (g, report) = crate::game::Game::from_pgn_report(game).unwrap();
                let moves_only = g.movetext_with(crate::pgn_style::PgnStyle { comments: false, ..crate::pgn_style::PgnStyle::FULL });
                assert_eq!((g.tag("Event"), moves_only.as_str(), report.stopped), (Some(event), moves, None), "{game}");
            }
        }
    }

    /// Plan A9-F2a (A1-16): tags on one line, or indented, are the tag section the reader reads,
    /// not a game of their own.
    #[test]
    fn tags_on_one_line_or_indented_start_one_game() {
        let text = "[Event \"A\"] [Site \"B\"]\n  [White \"C\"]\n\n1. e4 *\n  [Event \"D\"]\n1. d4 *\n";
        let games = split_pgn(text, 10);
        assert_eq!(games.len(), 2, "{games:#?}");
        assert_eq!(games.concat(), text);
        let (g, stopped) = crate::game::Game::from_pgn(games[0]).unwrap();
        assert_eq!((g.tag("Event"), g.tag("Site"), g.tag("White"), g.movetext().as_str(), stopped), (Some("A"), Some("B"), Some("C"), "1. e4", None));
    }

    /// One game's text: tags, moves with `;` and `%` comments that may hold braces and brackets.
    fn game_text() -> impl proptest::strategy::Strategy<Value = String> {
        use proptest::prelude::*;
        let tag = proptest::sample::select(vec!["[Event \"x\"]\n", "[Site \"a ] b\"]\n", "  [Round \"1\"] [White \"y\"]\n"]);
        let part =
            proptest::sample::select(vec!["e4 ", "1. ", "(1... c5) ", "{a [b] {c} ", "; note {x [y]\n", "\n% esc {z [w]\n", "$1 ", "\n", "Nf3 "]);
        let result = proptest::sample::select(vec!["*", "1-0", "0-1", "1/2-1/2"]);
        (proptest::collection::vec(tag, 1..4), proptest::collection::vec(part, 1..12), result)
            .prop_map(|(tags, parts, result)| format!("{}\n{}{result}\n\n", tags.concat(), parts.concat()))
    }

    proptest::proptest! {
        /// N games, whatever their `;` and `%` comments hold, are N pieces that put back together
        /// make the text (Q3's property, A7-01).
        #[test]
        fn n_games_split_into_n_pieces(games in proptest::collection::vec(game_text(), 1..6)) {
            let text = games.concat();
            let pieces = split_pgn(&text, 10);
            proptest::prop_assert_eq!(pieces.len(), games.len(), "{:#?}", pieces);
            proptest::prop_assert_eq!(pieces.concat(), text);
        }

        /// Any text: the pieces are in order and nothing but blank text is left out.
        #[test]
        fn splitting_loses_no_text(text in "\\PC{0,300}") {
            let pieces = split_pgn(&text, usize::MAX);
            let joined = pieces.concat();
            let text = crate::pgn::without_bom(&text);
            proptest::prop_assert!(text.starts_with(&joined));
            proptest::prop_assert!(text[joined.len()..].trim().is_empty());
        }
    }
}
