//! What a book asks of a diagram (plan A9-L6c, §2.6; R6-03): its *stipulation*, the goal (mate in
//! n, a helpmate, a selfmate, a win, a draw) and the side to play, read from the words a book
//! prints over or under a diagram, or over a page or a section of them: `White to play and win`,
//! `Pretas jogam`, `Weiß am Zug`, `Brancas jogam e empatam`, `Mate in 2`, `#2`, `Mate em 2`,
//! `Matt in 2 Zügen`, `h#2`, `s#2`, `ajudamate em 3`, `automate em 2`.
//!
//! [`read`] reads such a short text only ([`MAX_STIPULATION_WORDS`], [`MAX_STIPULATION_CHARS`]): a
//! caption, a diagram's title, a heading. Never a line of analysis, where `Black to move` may be a
//! supposition: the side's words count only where a clause starts, after a label at most
//! (`Diagram 12: White to play`, `It was necessary for Black to play` says nothing), and words
//! that are also prose's (`White plays`, `Weiß zieht`) only with a goal after them (`White plays
//! and wins`; `5.3 – Black plays ...Nd7` is a plan, not a stipulation).
//! The words are data ([`SIDE_WORDS`] and the goals' lists), beside the words
//! [`crate::describe`] says a side to move with, so what an export writes reads back.

use crate::placement::Color;

/// What the book asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Goal {
    /// The side to play mates in this many moves (`#2`, `Mate in 2`).
    Mate(u8),
    /// The other side helps the side to play be mated in this many moves (`h#2`).
    HelpMate(u8),
    /// The side to play forces the other to mate it in this many moves (`s#2`).
    SelfMate(u8),
    /// The side to play wins (`and win`).
    Win,
    /// The side to play draws (`and draw`).
    Draw,
}

/// How far a stipulation reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Scope {
    /// One diagram's caption or title.
    Diagram,
    /// A page's heading: every diagram of the page.
    Page,
    /// A section's heading or a running head: every diagram until the next section.
    Section,
}

/// What a book asks of a diagram.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Stipulation {
    pub goal: Option<Goal>,
    /// The side to play, when the words say it.
    pub side: Option<Color>,
    pub scope: Scope,
}

/// Longest text [`read`] looks at: a caption or a heading, not a paragraph.
pub const MAX_STIPULATION_CHARS: usize = 120;
/// Most words a text [`read`] looks at has (`Diagram 12: White to play and win` has seven): a
/// line of prose has more.
pub const MAX_STIPULATION_WORDS: usize = 8;
/// Most moves a stipulation asks for.
pub const MAX_STIPULATION_MOVES: u8 = 99;

/// Words that say who is to play, lower case: those [`crate::describe`] writes, and the ways books
/// print them.
pub const SIDE_WORDS: [(Color, &[&str]); 2] = [
    (
        Color::White,
        &[
            "white to move",
            "white to play",
            "brancas jogam",
            "jogam as brancas",
            "jogam brancas",
            "as brancas jogam",
            "weiß am zug",
            "weiss am zug",
            "juegan las blancas",
            "juegan blancas",
            "blancas juegan",
            "les blancs jouent",
            "trait aux blancs",
            "muove il bianco",
            "ход белых",
            "wit aan zet",
        ],
    ),
    (
        Color::Black,
        &[
            "black to move",
            "black to play",
            "pretas jogam",
            "jogam as pretas",
            "jogam pretas",
            "as pretas jogam",
            "negras jogam",
            "schwarz am zug",
            "juegan las negras",
            "juegan negras",
            "negras juegan",
            "les noirs jouent",
            "trait aux noirs",
            "muove il nero",
            "ход чёрных",
            "ход черных",
            "zwart aan zet",
        ],
    ),
];

/// Words that say who is to play only when a goal follows them ([`WIN_WORDS`], [`DRAW_WORDS`]):
/// prose says them too (`If White plays 12.Nf3`, `Black plays ...Nd7 in this line`).
pub const SIDE_WORDS_WITH_GOAL: [(Color, &[&str]); 2] =
    [(Color::White, &["white plays", "white moves", "weiß zieht", "weiss zieht"]), (Color::Black, &["black plays", "black moves", "schwarz zieht"])];
/// Words a caption may put before the side's: its label (`Diagram 12:`, `No diagrama 64`,
/// `Exercise 3`), besides numbers and signs.
pub const LABEL_WORDS: [&str; 24] = [
    "diagram",
    "diagrama",
    "diagramm",
    "diagramme",
    "diagramma",
    "position",
    "posição",
    "posicao",
    "posición",
    "stellung",
    "exercise",
    "exercício",
    "exercicio",
    "ejercicio",
    "puzzle",
    "problem",
    "problema",
    "test",
    "teste",
    "no",
    "na",
    "nr",
    "nº",
    "fig",
];
/// Words after the side's that ask for a win (`White to play and win`).
pub const WIN_WORDS: [&str; 10] =
    ["and win", "and wins", "e ganham", "e ganha", "e vencem", "e vence", "und gewinnt", "y ganan", "et gagnent", "e vince"];
/// Words after the side's that ask for a draw.
pub const DRAW_WORDS: [&str; 9] =
    ["and draw", "and draws", "e empatam", "e empata", "und hält remis", "y tablas", "y hacen tablas", "et font nulle", "e patta"];
/// Words before a number of moves that ask for mate (`Mate in 2`, `Mate em 2`, `Matt in 2 Zügen`).
pub const MATE_WORDS: [&str; 6] = ["mate in", "mate em", "mate en", "mat en", "matt in", "mat in"];
/// Words before a number of moves that ask for a helpmate.
pub const HELPMATE_WORDS: [&str; 5] = ["helpmate in", "ajudamate em", "ajudamate in", "hilfsmatt in", "ayudamate en"];
/// Words before a number of moves that ask for a selfmate.
pub const SELFMATE_WORDS: [&str; 4] = ["selfmate in", "automate em", "selbstmatt in", "automate en"];
/// Numbers of moves written in words, up to five.
const NUMBER_WORDS: [(&str, u8); 14] = [
    ("one", 1),
    ("two", 2),
    ("three", 3),
    ("four", 4),
    ("five", 5),
    ("um", 1),
    ("dois", 2),
    ("três", 3),
    ("quatro", 4),
    ("cinco", 5),
    ("zwei", 2),
    ("drei", 3),
    ("vier", 4),
    ("fünf", 5),
];

/// The stipulation `text` states, at `scope`, when it states a goal or a side; `None` for a
/// longer text than [`MAX_STIPULATION_CHARS`] or [`MAX_STIPULATION_WORDS`], or one that states
/// neither.
pub fn read(text: &str, scope: Scope) -> Option<Stipulation> {
    if text.chars().count() > MAX_STIPULATION_CHARS {
        return None;
    }
    // Lower case, one space between words, punctuation that ends a clause made a space.
    let lower: String = text.to_lowercase().chars().map(|c| if is_clause_break(c) { ' ' } else { c }).collect();
    let words: Vec<&str> = lower.split_whitespace().collect();
    if words.len() > MAX_STIPULATION_WORDS {
        return None;
    }
    let joined = words.join(" ");
    let has = |phrase: &str| contains_phrase(&joined, phrase);
    let side = text.to_lowercase().split(is_clause_break).find_map(clause_side);
    let goal = goal(&words, &has);
    (side.is_some() || goal.is_some()).then_some(Stipulation { goal, side, scope })
}

/// Where a clause ends: the punctuation [`read`] turns into spaces.
fn is_clause_break(c: char) -> bool {
    matches!(c, ',' | ';' | ':' | '!' | '?' | '(' | ')' | '.' | '\u{2013}' | '\u{2014}')
}

/// The side a clause says, when it starts with the side's words, after its label at most: label
/// words, numbers (`12`, `54A`) and signs (`-`, `#`).
fn clause_side(clause: &str) -> Option<Color> {
    let words: Vec<&str> = clause
        .split_whitespace()
        .skip_while(|w| !w.chars().any(char::is_alphabetic) || w.chars().any(|c| c.is_ascii_digit()) || LABEL_WORDS.contains(w))
        .collect();
    let rest = words.join(" ");
    let starts = |phrase: &&str| rest == *phrase || rest.strip_prefix(*phrase).is_some_and(|after| after.starts_with(' '));
    if let Some((color, _)) = SIDE_WORDS.iter().find(|(_, phrases)| phrases.iter().any(&starts)) {
        return Some(*color);
    }
    SIDE_WORDS_WITH_GOAL
        .iter()
        .find(|(_, phrases)| {
            phrases.iter().any(|p| {
                rest.strip_prefix(*p).is_some_and(|after| {
                    WIN_WORDS
                        .iter()
                        .chain(DRAW_WORDS.iter())
                        .any(|g| after.strip_prefix(' ').is_some_and(|a| a == *g || a.starts_with(&format!("{g} "))))
                })
            })
        })
        .map(|(c, _)| *c)
}

/// Whether `phrase` stands in `text` as whole words.
fn contains_phrase(text: &str, phrase: &str) -> bool {
    text.match_indices(phrase).any(|(i, _)| {
        let before = text.get(..i).and_then(|t| t.chars().next_back());
        let after = text.get(i.saturating_add(phrase.len())..).and_then(|t| t.chars().next());
        before.is_none_or(|c| c == ' ') && after.is_none_or(|c| c == ' ')
    })
}

/// The goal the words state: a mate, helpmate or selfmate in a number of moves (`#2`, `h#2`,
/// `s#2`, or its words and a number), else a win or a draw.
fn goal(words: &[&str], has: &dyn Fn(&str) -> bool) -> Option<Goal> {
    for word in words {
        for (prefix, make) in
            [("h#", Goal::HelpMate as fn(u8) -> Goal), ("s#", Goal::SelfMate as fn(u8) -> Goal), ("#", Goal::Mate as fn(u8) -> Goal)]
        {
            if let Some(n) = word.strip_prefix(prefix).and_then(moves_count) {
                return Some(make(n));
            }
        }
    }
    for (phrases, make) in [
        (&HELPMATE_WORDS[..], Goal::HelpMate as fn(u8) -> Goal),
        (&SELFMATE_WORDS[..], Goal::SelfMate as fn(u8) -> Goal),
        (&MATE_WORDS[..], Goal::Mate as fn(u8) -> Goal),
    ] {
        for phrase in phrases {
            let n = phrase.split(' ').count();
            for (k, window) in words.windows(n).enumerate() {
                if window.join(" ") == *phrase
                    && let Some(count) = words.get(k.saturating_add(n)).and_then(|w| moves_count(w).or_else(|| number_word(w)))
                {
                    return Some(make(count));
                }
            }
        }
    }
    if WIN_WORDS.iter().any(|p| has(p)) {
        return Some(Goal::Win);
    }
    DRAW_WORDS.iter().any(|p| has(p)).then_some(Goal::Draw)
}

/// A number of moves written in digits, 1 to [`MAX_STIPULATION_MOVES`].
fn moves_count(word: &str) -> Option<u8> {
    word.parse::<u8>().ok().filter(|n| (1..=MAX_STIPULATION_MOVES).contains(n))
}

fn number_word(word: &str) -> Option<u8> {
    NUMBER_WORDS.iter().find(|(w, _)| *w == word).map(|(_, n)| *n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_at(text: &str) -> Option<(Option<Goal>, Option<Color>)> {
        read(text, Scope::Diagram).map(|s| (s.goal, s.side))
    }

    #[test]
    fn a_books_stipulations_in_its_languages() {
        use Color::{Black, White};
        assert_eq!(read_at("Black to move"), Some((None, Some(Black))));
        assert_eq!(read_at("White to play and win."), Some((Some(Goal::Win), Some(White))));
        assert_eq!(read_at("Pretas jogam"), Some((None, Some(Black))));
        assert_eq!(read_at("Weiß am Zug"), Some((None, Some(White))));
        assert_eq!(read_at("Brancas jogam e empatam"), Some((Some(Goal::Draw), Some(White))));
        assert_eq!(read_at("Mate in 2"), Some((Some(Goal::Mate(2)), None)));
        assert_eq!(read_at("White to play. Mate in two"), Some((Some(Goal::Mate(2)), Some(White))));
        assert_eq!(read_at("#2"), Some((Some(Goal::Mate(2)), None)));
        assert_eq!(read_at("Mate em 3"), Some((Some(Goal::Mate(3)), None)));
        assert_eq!(read_at("Matt in 2 Zügen"), Some((Some(Goal::Mate(2)), None)));
        assert_eq!(read_at("h#2"), Some((Some(Goal::HelpMate(2)), None)));
        assert_eq!(read_at("s#3"), Some((Some(Goal::SelfMate(3)), None)));
        assert_eq!(read_at("Ajudamate em 3"), Some((Some(Goal::HelpMate(3)), None)));
        assert_eq!(read_at("Automate em 2"), Some((Some(Goal::SelfMate(2)), None)));
        assert_eq!(read_at("(2) Black to play"), Some((None, Some(Black))));
        assert_eq!(read_at("Black to move in all positions"), Some((None, Some(Black))));
        assert_eq!(read_at("White plays and wins"), Some((Some(Goal::Win), Some(White))));
        assert_eq!(read_at("No diagrama 64 as brancas jogam"), Some((None, Some(White))));
        assert_eq!(read_at("Diagrama 12B - Jogam as pretas"), Some((None, Some(Black))));
        assert_eq!(read_at("White to move \u{2013} should he"), Some((None, Some(White))));
        assert_eq!(read_at("Diagram 12 \u{2014} White to play and win"), Some((Some(Goal::Win), Some(White))));
        assert_eq!(read("Combinations #2", Scope::Section).map(|s| s.scope), Some(Scope::Section));
        // Not stipulations: prose, a line of analysis, a word that only contains one, a number out
        // of range, a long text.
        assert_eq!(read_at("The knight is strong"), None);
        assert_eq!(read_at("if it were Black to move here he would lose at once"), None);
        assert_eq!(read_at("Black to move would lose at once to the knight fork"), None);
        // The side's words inside a clause, or prose's without a goal: no side.
        assert_eq!(read_at("It was necessary for Black to play"), None);
        assert_eq!(read_at("If White plays"), None);
        assert_eq!(read_at("5.3 \u{2013} Black plays ...Nf6-d7-c5-e6"), None);
        assert_eq!(read_at("that it is Black to move first"), None);
        assert_eq!(read_at("whitewash to moves"), None);
        assert_eq!(read_at("#0 #100"), None);
        assert_eq!(read_at(&format!("{} Black to move", "word ".repeat(30))), None);
    }
}
