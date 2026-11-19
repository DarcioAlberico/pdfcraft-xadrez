//! A game's header as a database prints it before the moves: the players' line and, under it, the
//! event's. ChessBase sets them so (Chess Evolution 2013 and 2014):
//!
//! ```text
//! (1) Vachier-Lagrave,Maxime (2711) — Grigoryan,Karen H (2513) [C02]
//! Al Ain Classic 2012 Al-Ain UAE (5.3), 26.12.2012 [Balogh Csaba]
//! ```
//!
//! The players' line starts with the game's number in brackets, names both players with their
//! ratings and ends with the opening's code; the event's line names the event, the round in
//! brackets, the date after the last comma and the annotator in square brackets. The spaces
//! around the dash and before the brackets come and go. They fill the game's tags
//! ([`GameHeader::apply`]).
//!
//! Collections of one player's or one match's games number them in a label above instead
//! (Huschenbeth, *Carlsen vs. Caruana*):
//!
//! ```text
//! Game 1
//! Carlsen (2810) - Caruana (2675)
//! Corus-A, Wijk aan Zee (13), 31.01.2010
//! ```
//!
//! The players' line then has no number ([`bare_players_line`]) and is a header only with the
//! event's line under it; the label ([`game_label`]) is the header's too.

use super::games::{GameScore, Tag};

/// Most digits of the game's number that opens a players' line.
pub const MAX_GAME_NUMBER_DIGITS: usize = 4;
/// Most characters of a player's name.
pub const MAX_NAME_CHARS: usize = 60;
/// Most characters of an event's line.
pub const MAX_EVENT_CHARS: usize = 120;
/// Most characters of a round (`5.3`).
pub const MAX_ROUND_CHARS: usize = 8;
/// Most words of a player's name in a players' line without the game's number
/// ([`bare_players_line`]): `José Raúl Capablanca y Graupera`.
pub const MAX_NAME_WORDS: usize = 5;
/// The word of a game's label ([`game_label`]), in the languages the books come in.
const GAME_WORDS: [&str; 7] = ["Game", "Partida", "Partie", "Partita", "Spiel", "Partij", "Jogo"];

/// The players and the event a game's header names.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct GameHeader {
    pub white: String,
    pub black: String,
    pub white_elo: Option<String>,
    pub black_elo: Option<String>,
    pub eco: Option<String>,
    pub event: Option<String>,
    pub round: Option<String>,
    /// As PGN writes it: `2012.12.26`, `1967.??.??`.
    pub date: Option<String>,
    pub annotator: Option<String>,
}

impl GameHeader {
    /// The header in `score`'s tags: the roster's players, event, round and date, and the
    /// ratings, the opening's code and the annotator beside them.
    pub(crate) fn apply(self, score: &mut GameScore) {
        let r = &mut score.roster;
        r.white = self.white;
        r.black = self.black;
        if let Some(event) = self.event {
            r.event = event;
        }
        if let Some(round) = self.round {
            r.round = round;
        }
        if let Some(date) = self.date {
            r.date = date;
        }
        for (name, value) in [("WhiteElo", self.white_elo), ("BlackElo", self.black_elo), ("ECO", self.eco), ("Annotator", self.annotator)] {
            if let Some(value) = value {
                score.extra_tags.retain(|t| t.name != name);
                score.extra_tags.push(Tag { name: name.to_owned(), value });
            }
        }
    }
}

/// The players' line `text` reads as, when it is one: `(N)`, White with a rating in brackets or
/// none, a dash, Black likewise, and an opening's code in square brackets or none.
pub(crate) fn players_line(text: &str) -> Option<GameHeader> {
    let rest = text.trim().strip_prefix('(')?;
    let (number, rest) = rest.split_once(')')?;
    if number.is_empty() || number.len() > MAX_GAME_NUMBER_DIGITS || !number.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    players(rest.trim())
}

/// The players' line `text` reads as without the game's number in front (`Carlsen (2810) -
/// Caruana (2675)`), as books print it under a `Game 1` label ([`game_label`]). Without the number
/// a name and a dash are all it shows, so it is a header only with an event's line under it (the
/// caller's part, [`event_line`]); a side of more than [`MAX_NAME_WORDS`] words is a sentence.
pub(crate) fn bare_players_line(text: &str) -> Option<GameHeader> {
    let text = text.trim();
    if text.starts_with('(') {
        return None;
    }
    let header = players(text)?;
    let words = |name: &str| name.split_whitespace().count();
    (words(&header.white) <= MAX_NAME_WORDS && words(&header.black) <= MAX_NAME_WORDS).then_some(header)
}

/// White with a rating in brackets or none, a dash, Black likewise, and an opening's code in
/// square brackets or none.
fn players(text: &str) -> Option<GameHeader> {
    let (rest, eco) = match text.strip_suffix(']').and_then(|r| r.rsplit_once('[')) {
        Some((before, code)) if is_eco(code) => (before.trim_end(), Some(code.to_owned())),
        _ => (text, None),
    };
    let (white, black) = split_players(rest)?;
    let (white, white_elo) = with_rating(white);
    let (black, black_elo) = with_rating(black);
    Some(GameHeader { white: name(white)?, black: name(black)?, white_elo, black_elo, eco, ..GameHeader::default() })
}

/// Whether `text` is a game's label as a collection prints it over the players' line: `Game 1`,
/// `Partida 12`, `Partie 3`, `Partita 4`, `Spiel 5`, `Partij 6`, `Jogo 7`, a colon or full stop
/// after the number or none.
pub(crate) fn game_label(text: &str) -> bool {
    let t = text.trim().trim_end_matches([':', '.']);
    let Some((word, number)) = t.split_once(char::is_whitespace) else { return false };
    let number = number.trim();
    GAME_WORDS.iter().any(|w| word.eq_ignore_ascii_case(w))
        && !number.is_empty()
        && number.len() <= MAX_GAME_NUMBER_DIGITS
        && number.chars().all(|c| c.is_ascii_digit())
}

/// The event's line `text` reads as, when it is one, into `header`: the event with its round in
/// brackets or none, a comma, the date or nothing, and the annotator in square brackets or none.
/// Returns whether it was one.
pub(crate) fn event_line(text: &str, header: &mut GameHeader) -> bool {
    let t = text.trim();
    if t.chars().count() > MAX_EVENT_CHARS || t.contains(". ") {
        return false;
    }
    let (rest, annotator) = match t.strip_suffix(']').and_then(|r| r.rsplit_once('[')) {
        Some((before, who)) if !who.trim().is_empty() => (before.trim_end(), Some(who.trim().to_owned())),
        _ => (t, None),
    };
    let Some((place, date)) = rest.rsplit_once(',') else { return false };
    let date = date.trim();
    let date = if date.is_empty() {
        None
    } else {
        match pgn_date(date) {
            Some(d) => Some(d),
            None => return false,
        }
    };
    let place = place.trim();
    let (event, round) = match place.strip_suffix(')').and_then(|r| r.rsplit_once('(')) {
        Some((event, round)) if !round.is_empty() && round.len() <= MAX_ROUND_CHARS && round.chars().all(|c| c.is_ascii_digit() || c == '.') => {
            (event.trim_end(), Some(round.to_owned()))
        }
        _ => (place, None),
    };
    if !event.chars().any(char::is_alphabetic) {
        return false;
    }
    header.event = Some(event.to_owned());
    header.round = round;
    header.date = date;
    header.annotator = annotator;
    true
}

/// `A00` to `E99`.
fn is_eco(code: &str) -> bool {
    matches!(code.as_bytes(), [b'A'..=b'E', b'0'..=b'9', b'0'..=b'9'])
}

/// White's part and Black's, either side of the dash between them: an em or en dash, or a
/// hyphen after a rating's bracket or beside a space (a name's own hyphen has letters on both
/// sides: `Vachier-Lagrave`).
fn split_players(text: &str) -> Option<(&str, &str)> {
    if let Some(at) = text.find(['—', '–']) {
        let dash = text.get(at..)?.chars().next()?.len_utf8();
        return Some((text.get(..at)?, text.get(at.saturating_add(dash)..)?));
    }
    let bytes = text.as_bytes();
    let at = (0..bytes.len()).find(|&i| {
        bytes.get(i) == Some(&b'-') && {
            let before = i.checked_sub(1).and_then(|j| bytes.get(j));
            let after = bytes.get(i.saturating_add(1));
            matches!(before, Some(b')' | b' ')) || after == Some(&b' ')
        }
    })?;
    // A hyphen is one byte.
    Some((text.get(..at)?, text.get(at.saturating_add(1)..)?))
}

/// A player's part as the name and the rating in brackets after it, if any (`Carlsen,Magnus
/// (2864)`, `Wojtaszek, Radoslaw(2711)`).
fn with_rating(part: &str) -> (&str, Option<String>) {
    let part = part.trim();
    if let Some((name, rating)) = part.strip_suffix(')').and_then(|p| p.rsplit_once('('))
        && (3..=4).contains(&rating.len())
        && rating.chars().all(|c| c.is_ascii_digit())
    {
        return (name.trim_end(), Some(rating.to_owned()));
    }
    (part, None)
}

/// A player's name as PGN writes it, a space after each comma (`Vachier-Lagrave, Maxime`);
/// `None` for no name: not starting with a letter, too long, or with brackets left in it.
fn name(part: &str) -> Option<String> {
    let part = part.trim();
    if !part.starts_with(char::is_alphabetic) || part.chars().count() > MAX_NAME_CHARS || part.contains(['(', ')', '[', ']']) {
        return None;
    }
    Some(part.split(',').map(str::trim).collect::<Vec<_>>().join(", "))
}

/// A printed date as PGN writes it: `26.12.2012` and `2012.12.26` as `2012.12.26`, `1952` as
/// `1952.??.??`.
fn pgn_date(text: &str) -> Option<String> {
    let parts: Vec<&str> = text.split('.').collect();
    let num = |s: &str, digits: std::ops::RangeInclusive<usize>, max: u32| {
        if !digits.contains(&s.len()) || !s.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        s.parse::<u32>().ok().filter(|n| (1..=max).contains(n))
    };
    match parts.as_slice() {
        [y] => num(y, 4..=4, 9999).map(|y| format!("{y:04}.??.??")),
        [y, m, d] if y.len() == 4 => Some(format!("{:04}.{:02}.{:02}", num(y, 4..=4, 9999)?, num(m, 1..=2, 12)?, num(d, 1..=2, 31)?)),
        [d, m, y] => Some(format!("{:04}.{:02}.{:02}", num(y, 4..=4, 9999)?, num(m, 1..=2, 12)?, num(d, 1..=2, 31)?)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_players_line_reads_as_chessbase_prints_it() {
        let h = players_line("(1) Vachier-Lagrave,Maxime (2711)— Grigoryan,Karen H (2513) [C02]").unwrap();
        assert_eq!((h.white.as_str(), h.black.as_str()), ("Vachier-Lagrave, Maxime", "Grigoryan, Karen H"));
        assert_eq!((h.white_elo.as_deref(), h.black_elo.as_deref(), h.eco.as_deref()), (Some("2711"), Some("2513"), Some("C02")));
        let h = players_line("(19) Iturrizaga, E(2653)-Nakamura, H(2775)").unwrap();
        assert_eq!((h.white.as_str(), h.black.as_str(), h.eco), ("Iturrizaga, E", "Nakamura, H", None));
        let h = players_line("(17) Unzicker, Wolfgang- Botvinnik, Mikhail[B08]").unwrap();
        assert_eq!(
            (h.white.as_str(), h.black.as_str(), h.white_elo, h.eco.as_deref()),
            ("Unzicker, Wolfgang", "Botvinnik, Mikhail", None, Some("B08"))
        );
        let h = players_line("(2) Ponomariov,R (2743) – Eljanov,P (2707) [C91]").unwrap();
        assert_eq!(h.black, "Eljanov, P");
        // Not a players' line: no number, no second player, a name's own hyphen, prose.
        for text in
            ["Carlsen — Anand", "(18) Kubbel", "(3) Vachier-Lagrave", "(a) Smith — Jones", "(1)Smith — Jones", "(12345) A — B", "(2) 1.e4 — 1.d4"]
        {
            assert_eq!(players_line(text), None, "{text}");
        }
    }

    #[test]
    fn a_players_line_without_its_number_and_a_games_label() {
        let h = bare_players_line("Carlsen (2810) - Caruana (2675)").unwrap();
        assert_eq!((h.white.as_str(), h.black.as_str()), ("Carlsen", "Caruana"));
        assert_eq!((h.white_elo.as_deref(), h.black_elo.as_deref()), (Some("2810"), Some("2675")));
        let h = bare_players_line("Magnus Carlsen – Fabiano Caruana").unwrap();
        assert_eq!((h.white.as_str(), h.black.as_str(), h.white_elo), ("Magnus Carlsen", "Fabiano Caruana", None));
        assert_eq!(
            bare_players_line("José Raúl Capablanca y Graupera - Lasker").map(|h| h.white),
            Some("José Raúl Capablanca y Graupera".to_owned())
        );
        // The numbered form is players_line's; a sentence, one name, a name's own hyphen: none.
        for text in [
            "(1) Carlsen - Caruana",
            "This was played in the last round and the game was drawn - a pity",
            "Vachier-Lagrave",
            "Carlsen",
            "1.e4 - 1.d4",
            "",
        ] {
            assert_eq!(bare_players_line(text), None, "{text}");
        }
        for text in ["Game 1", "Game 12:", "Partida 3", "partie 4.", "Partita 5", "Spiel 6", "Partij 7", "Jogo 8", " Game 71 "] {
            assert!(game_label(text), "{text}");
        }
        for text in ["Game", "Game one", "Game 1 Carlsen", "Game 12345", "Games 2", "Example 5", "1 Game", ""] {
            assert!(!game_label(text), "{text}");
        }
    }

    #[test]
    fn the_events_line_gives_event_round_date_and_annotator() {
        let mut h = GameHeader::default();
        assert!(event_line("Al Ain Classic 2012 Al-Ain UAE (5.3), 26.12.2012[Balogh Csaba]", &mut h));
        assert_eq!(h.event.as_deref(), Some("Al Ain Classic 2012 Al-Ain UAE"));
        assert_eq!((h.round.as_deref(), h.date.as_deref(), h.annotator.as_deref()), (Some("5.3"), Some("2012.12.26"), Some("Balogh Csaba")));
        let mut h = GameHeader::default();
        assert!(event_line("Varna ol (Men) qual-A(4), 1962", &mut h));
        assert_eq!((h.event.as_deref(), h.round.as_deref(), h.date.as_deref()), (Some("Varna ol (Men) qual-A"), Some("4"), Some("1962.??.??")));
        let mut h = GameHeader::default();
        assert!(event_line("37th Zurich Christmas 2013(2.1),2013.12.27", &mut h));
        assert_eq!((h.round.as_deref(), h.date.as_deref()), (Some("2.1"), Some("2013.12.27")));
        let mut h = GameHeader::default();
        assert!(event_line("Greek league,", &mut h));
        assert_eq!((h.event.as_deref(), h.date), (Some("Greek league"), None));
        for text in ["A nice fighting game. Korobov played it, 1999", "1.e4 c5 2.Nf3 d6", "Moscow, 31.13.2013", "Moscow, yesterday", "2013, 2014"] {
            assert!(!event_line(text, &mut GameHeader::default()), "{text}");
        }
    }
}
