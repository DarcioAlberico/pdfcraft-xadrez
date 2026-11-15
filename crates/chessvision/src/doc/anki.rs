//! The book's exercises as Anki cards (plan A9-N10b, finding A8-05): a text file that Anki's
//! File ▸ Import reads, one note per exercise, and the diagrams' pictures beside it. The
//! exercises are those of the problem PGN, by the same rule ([`super::exercises::find`]).
//!
//! The form is the one Anki's manual gives for text files ("Importing ▸ Text Files"):
//!
//! - file headers at the top, which Anki 2.1.54 and later read: `#separator:tab`, `#html:true`
//!   and `#tags:` with the pictures' prefix, so the cards of a book can be found by it. No note
//!   type and no deck are named (their names are the user's, and translated): Anki's import
//!   dialog asks, and its fields are filled in order, front then back;
//! - then a line per exercise, its two fields separated by a tab. **Front:** the diagram's
//!   picture as `<img src="…">`, a line break, and the statement (the side to move, the number,
//!   where it is in the book: [`super::exercises`]). **Back:** the solution's main line in SAN
//!   (`1.Qh5 Nf6 2.Qxf7#`; `1...Qf1#` when Black moves first);
//! - HTML is on, so the book's `&`, `<`, `>` and `"` are written as entities, and no field ever
//!   holds a tab or a line break (each becomes a space): no field needs Anki's quoting;
//! - the pictures are copied by the user into the collection's `collection.media` folder, with
//!   no subfolder: a card names its picture by file name alone.
//!
//! Anki matches each line to the notes it has by the first field, so the front is the same on
//! every run, and importing the file again updates the cards instead of adding them twice.
//!
//! The pictures' names ([`media_prefix`], [`image_name`]) are safe file names everywhere and the
//! same on every run: the title's ASCII letters and digits, then the page and the diagram.
//! `collection.media` is one folder for every deck, so the title keeps two books apart; a title
//! with other letters (`Шахматы`), or longer than the prefix, adds 8 hex digits of its SHA-256.
//!
//! The pictures are drawn by the engine, with the board drawing every export uses: each card
//! says which position its picture shows and from which side ([`Card`]).

use std::fmt::Write as _;

use pdfcraft_chess::placement::Color;
use sha2::Digest as _;

use super::ChessDoc;
use super::exercises::{Exercise, LeftOut, find};
use super::games::GameScore;
use super::markdown::PgnBook;

/// The most ASCII letters and digits of the title a picture's name keeps.
pub const MAX_PREFIX: usize = 40;

/// One card.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Card {
    pub exercise: Exercise,
    /// Its picture's file name ([`image_name`]).
    pub image: String,
    /// The position the picture shows: the one the solution starts from, as the problem PGN's
    /// `FEN`.
    pub fen: String,
    /// The picture drawn from Black's side, as the book prints the diagram.
    pub flipped: bool,
    /// The front's text under the picture (plain text: [`AnkiDeck::tsv`] escapes it).
    pub statement: String,
    /// The back: the solution's main line in SAN.
    pub solution: String,
}

/// What [`anki_deck`] found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AnkiDeck {
    /// The pictures' prefix ([`media_prefix`]), also every card's tag.
    pub prefix: String,
    /// In the book's order.
    pub cards: Vec<Card>,
    /// The numbered diagrams left out, as the problem PGN leaves them out.
    pub left_out: Vec<LeftOut>,
}

/// The exercises of `doc` as Anki's cards, their pictures named after `book`'s title.
pub fn anki_deck(doc: &ChessDoc, book: &PgnBook) -> AnkiDeck {
    let prefix = media_prefix(book.title.as_deref().unwrap_or_default());
    let (found, left_out) = find(doc, book);
    let cards = found
        .into_iter()
        .map(|f| Card {
            image: image_name(&prefix, f.exercise.at.page, f.exercise.at.index),
            fen: f.score.start_fen.clone().unwrap_or_else(|| cozy_chess::Board::default().to_string()),
            flipped: f.orientation == Color::Black,
            statement: f.score.comment.clone(),
            solution: solution(&f.score),
            exercise: f.exercise,
        })
        .collect();
    AnkiDeck { prefix, cards, left_out }
}

impl AnkiDeck {
    /// The exercises, in the book's order.
    pub fn exercises(&self) -> Vec<Exercise> {
        self.cards.iter().map(|c| c.exercise).collect()
    }

    /// The text file Anki imports: the headers, then a line per card. `drawn` says whether a
    /// card's picture was made: one that was not shows its position's FEN in its place, so no
    /// card points at a picture that is not there.
    pub fn tsv(&self, drawn: impl Fn(&Card) -> bool) -> String {
        let mut out = format!("#separator:tab\n#html:true\n#tags:{}\n", self.prefix);
        for card in &self.cards {
            let picture = if drawn(card) { format!("<img src=\"{}\">", field(&card.image)) } else { format!("FEN {}", field(&card.fen)) };
            let _ = writeln!(out, "{picture}<br>{}\t{}", field(&card.statement), field(&card.solution));
        }
        out
    }
}

/// The prefix of a book's pictures' names, from its `title`: its ASCII letters and digits in
/// lower case, every run of other characters as one `-`, at most [`MAX_PREFIX`] of them (else
/// `exercises`); then, when the title has other letters or digits or is longer, `-` and 8 hex
/// digits of the SHA-256 of the title, so two such titles do not share names.
pub fn media_prefix(title: &str) -> String {
    let title = title.trim();
    let mut slug = String::new();
    let mut kept = 0usize;
    let mut lossy = false;
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            if kept >= MAX_PREFIX {
                lossy = true;
                break;
            }
            slug.push(c.to_ascii_lowercase());
            kept = kept.saturating_add(1);
        } else {
            lossy |= c.is_alphanumeric();
            if !slug.is_empty() && !slug.ends_with('-') {
                slug.push('-');
            }
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        slug.push_str("exercises");
    }
    if lossy {
        let digest = sha2::Sha256::digest(title.as_bytes());
        slug.push('-');
        for byte in digest.iter().take(4) {
            let _ = write!(slug, "{byte:02x}");
        }
    }
    slug
}

/// The picture of diagram `index` (0-based) of page `page` (0-based): `<prefix>-p0007-1.png`.
pub fn image_name(prefix: &str, page: usize, index: usize) -> String {
    format!("{prefix}-p{:04}-{}.png", page.saturating_add(1), index.saturating_add(1))
}

/// A game's main line in SAN with its numbers: `1.Qh5 Nf6 2.Qxf7#`, `1...Qf1#`.
pub(crate) fn solution(score: &GameScore) -> String {
    let mut out = String::new();
    for (i, n) in score.mainline().into_iter().enumerate() {
        let Some(node) = score.nodes.get(n) else { break };
        let number = (node.ply / 2).saturating_add(1);
        if !out.is_empty() {
            out.push(' ');
        }
        if node.ply % 2 == 0 {
            let _ = write!(out, "{number}.");
        } else if i == 0 {
            let _ = write!(out, "{number}...");
        }
        out.push_str(&node.san);
    }
    out
}

/// `text` as one field of a line with HTML on: `&`, `<`, `>` and `"` as entities, a tab, a line
/// break or any other control character as a space.
fn field(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.trim().chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}
