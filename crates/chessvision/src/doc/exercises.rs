//! The book's exercises as a standard problem PGN (plan A9-N10, finding A8-05, decision D20): what
//! every program that trains on positions reads — ChessBase's Replay training, a lichess study, a
//! Chessable private course, the Anki add-ons that take PGN.
//!
//! **An exercise** is a diagram the book numbers (`№185`, `1675`, `Exercise 12`: its caption's
//! puzzle number, [`super::DiagramBlock::puzzle`]) whose solution was read: the game the walk
//! played from it, from the moves after it or from a solution that names its number pages later
//! ([`super::games`]). Each becomes one game:
//!
//! - `SetUp "1"` and `FEN`: the position the solution starts from (as the text settled it, or as a
//!   person or the book's PGN confirmed it);
//! - the solution as the main line, with the book's variations and comments;
//! - `Result` as printed, else the board's at the end of the main line (a mate), else `*`;
//! - `Event` the book's title when the game has none, `Round` the exercise's number, and the
//!   origin's tags (`PdfPage`, `PdfPuzzle`…; plan A8 P8a);
//! - the statement, in the comment before the first move, in the book's language: the side to
//!   move and the number, then where it is in the book (`Brancas jogam (№ 185). Pág. 275 ·
//!   diagrama 1 de 6`).
//!
//! A numbered diagram whose solution was not read, or that has no position to start from, is left
//! out and listed ([`LeftOut`]). ChessBase's training command (`%tqu`) is not written: decision
//! D20 waits for it to be tried in ChessBase first.

use pdfcraft_chess::describe;
use pdfcraft_chess::pgn_style::PgnStyle;
use pdfcraft_chess::placement::Color;

use super::games::{DiagramAt, GameOrigin, GameScore};
use super::markdown::{PgnBook, with_origin};
use super::pgn::GameWarning;
use super::{Block, ChessDoc};

/// An exercise written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Exercise {
    pub at: DiagramAt,
    /// The book's number for it.
    pub number: u32,
    /// The moves of its solution's main line.
    pub moves: usize,
}

/// Why a numbered diagram is not among the exercises.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeftOutReason {
    /// No solution was read from it: no moves after it, and none that name its number.
    NoSolution,
    /// Its solution starts from no position a game can be played from.
    NoPosition,
}

impl LeftOutReason {
    /// The reason's name in the tools' replies.
    pub fn name(self) -> &'static str {
        match self {
            LeftOutReason::NoSolution => "no_solution",
            LeftOutReason::NoPosition => "no_position",
        }
    }
}

/// A numbered diagram left out, and why.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeftOut {
    pub at: DiagramAt,
    pub number: u32,
    pub reason: LeftOutReason,
}

/// What [`exercises_pgn`] wrote.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExercisesPgn {
    pub pgn: String,
    /// In the book's order.
    pub exercises: Vec<Exercise>,
    pub left_out: Vec<LeftOut>,
    /// What writing left out of a game (plan A9-F4), by exercise in the order written.
    pub warnings: Vec<GameWarning>,
}

/// The exercises of `doc` as a problem PGN, from `book` (its title, its pages' labels), the moves
/// written as `style` says (see the module documentation).
pub fn exercises_pgn(doc: &ChessDoc, book: &PgnBook, style: PgnStyle) -> ExercisesPgn {
    let mut made = ExercisesPgn::default();
    let mut games: Vec<GameScore> = Vec::new();
    for page in &doc.pages {
        let lang = page.text_language();
        for block in &page.blocks {
            let Block::Diagram(d) = block else { continue };
            let Some(number) = d.puzzle else { continue };
            let at = DiagramAt::on(page.index, d.number);
            let solution =
                doc.games.iter().find(|g| matches!(g.origin, GameOrigin::Diagram { page: p, diagram, .. } if p == page.index && diagram == d.number));
            let Some(game) = solution.filter(|g| !g.score.mainline().is_empty()) else {
                made.left_out.push(LeftOut { at, number, reason: LeftOutReason::NoSolution });
                continue;
            };
            let Some(start) = game.score.start_board() else {
                made.left_out.push(LeftOut { at, number, reason: LeftOutReason::NoPosition });
                continue;
            };
            let side = if start.side_to_move() == cozy_chess::Color::Black { Color::Black } else { Color::White };
            let mut score = with_origin(doc, book, game);
            if let Some(title) = book.title.as_deref().map(str::trim).filter(|t| !t.is_empty())
                && matches!(score.roster.event.trim(), "" | "?")
            {
                title.clone_into(&mut score.roster.event);
            }
            score.roster.round = number.to_string();
            score.comment = statement(side, number, lang, &score.comment);
            made.exercises.push(Exercise { at, number, moves: score.mainline().len() });
            games.push(score);
        }
    }
    let written = super::pgn::write_report(games.iter(), style);
    (made.pgn, made.warnings) = (written.pgn, written.warnings);
    made
}

/// The statement before an exercise's first move: the side to move and its number, then `rest`
/// (where it is in the book, and what the book said before the solution).
fn statement(side: Color, number: u32, lang: pdfcraft_chess::san::Lang, rest: &str) -> String {
    let to_move = describe::to_move(side, lang);
    let mut chars = to_move.chars();
    let head: String = chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default();
    let rest = rest.trim();
    if rest.is_empty() { format!("{head} (№ {number}).") } else { format!("{head} (№ {number}). {rest}") }
}
