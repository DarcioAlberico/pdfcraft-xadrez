//! What a move list shows above the moves (plan HC10): the players with their ratings, then the
//! event, site, date and round, each only when its tag says something.
//!
//! The tags are kept as read; this is only how they read. PGN fills unknown values with
//! placeholders (`?`, `????.??.??`, a round of `-`), and those are left out. A date keeps its
//! known parts: `1972.??.??` reads `1972`. The interface's header and `chess_game` both read the
//! game through [`Game::header`], so they never disagree.

use super::Game;

/// A game's details as a move list's header shows them: each field absent when its tag says
/// nothing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GameHeader {
    pub white: Option<String>,
    pub white_elo: Option<String>,
    pub black: Option<String>,
    pub black_elo: Option<String>,
    pub event: Option<String>,
    pub site: Option<String>,
    /// The date as far as it is known (`2018.11`, `1972`).
    pub date: Option<String>,
    pub round: Option<String>,
}

impl Game {
    /// The game's details as a move list's header shows them.
    pub fn header(&self) -> GameHeader {
        let tag = |name| known(self, name).map(str::to_owned);
        GameHeader {
            white: tag("White"),
            white_elo: tag("WhiteElo"),
            black: tag("Black"),
            black_elo: tag("BlackElo"),
            event: tag("Event"),
            site: tag("Site"),
            date: tag("Date"),
            round: tag("Round"),
        }
    }
}

/// A tag's value when it says something: not empty, not PGN's `?` for "not known", and a date
/// only as far as it is known (`1972.??.??` is `1972`).
fn known<'a>(game: &'a Game, name: &str) -> Option<&'a str> {
    let value = game.tag(name)?.trim();
    let value = if name.ends_with("Date") {
        let parts: Vec<&str> = value.split('.').take_while(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit())).collect();
        let len = parts.iter().map(|p| p.len()).sum::<usize>() + parts.len().saturating_sub(1);
        // Digits and dots only, so the length is at a character boundary.
        value.get(..len)?
    } else {
        value
    };
    (!value.is_empty() && !value.chars().all(|c| matches!(c, '?' | '-' | '.' | ' '))).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(tags: &[(&str, &str)]) -> Game {
        let mut g = Game::standard();
        for (name, value) in tags {
            g.set_tag(name, value).unwrap();
        }
        g
    }

    #[test]
    fn the_header_says_only_what_is_known() {
        assert_eq!(game(&[]).header(), GameHeader::default());
        let placeholders = game(&[("White", "?"), ("Event", "?"), ("Site", " - "), ("Date", "????.??.??"), ("Round", "-"), ("WhiteElo", "?")]);
        assert_eq!(placeholders.header(), GameHeader::default());
        let h = game(&[
            ("White", "Carlsen"),
            ("WhiteElo", "2882"),
            ("Black", "Caruana"),
            ("Event", "WCh"),
            ("Site", "London"),
            ("Date", "2018.11.?? "),
            ("Round", "12"),
        ])
        .header();
        assert_eq!(
            (h.white.as_deref(), h.white_elo.as_deref(), h.black.as_deref(), h.black_elo),
            (Some("Carlsen"), Some("2882"), Some("Caruana"), None)
        );
        assert_eq!(
            (h.event.as_deref(), h.site.as_deref(), h.date.as_deref(), h.round.as_deref()),
            (Some("WCh"), Some("London"), Some("2018.11"), Some("12"))
        );
    }

    #[test]
    fn a_date_keeps_its_known_parts() {
        for (date, want) in
            [("2018.11.25", Some("2018.11.25")), ("1972.??.??", Some("1972")), ("????.11.25", None), ("", None), ("1972", Some("1972"))]
        {
            assert_eq!(game(&[("Date", date)]).header().date.as_deref(), want, "{date:?}");
        }
    }
}
