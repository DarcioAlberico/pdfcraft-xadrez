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
//! out and listed ([`LeftOut`]).
//!
//! **ChessBase's training question** (plan A9-N10c, decision D20): in the ChessBase dialect
//! ([`PgnDialect::ChessBase`]), the statement's comment opens with the `%tqu` command, in the form
//! the D18 session saw ChessBase 26 read and write (`docs/chess/d18-kit/RESULTADO.md`, D1–D3):
//! `{[%tqu "Pt","Brancas jogam (№ 185).","","","d1d8","",10] Pág. 275 · diagrama 1 de 6}`. The
//! fields are the language (ChessBase's tabs En, De, Fr, Es, It, Ne and Pt; any other language is
//! `En`, its text left as it is), the question (the statement's first sentence: the side to move
//! and the number), two empty fields, then one answer: the main line's first move as its two
//! squares, empty feedback and the score 10. The rest of the statement follows the command. The
//! session saw plain moves only, so an exercise whose first move castles or promotes is written
//! without the command and listed ([`TrainingLeftOut`]): no coding is guessed. How ChessBase
//! escapes was not seen either, so the question and the rest write `"` as `'`, `[` and `]` as `(`
//! and `)` (a `]` would end the command) and control characters as spaces. Standard PGN and the
//! Lichess dialect write no command; nor does any dialect with `comments: none`, which writes no
//! comment.
//!
//! The same exercises, by the same rule ([`find`]), are also Anki's cards: [`super::anki`] (plan
//! A9-N10b).

use pdfcraft_chess::describe;
use pdfcraft_chess::pgn_style::{PgnDialect, PgnStyle};
use pdfcraft_chess::placement::Color;
use pdfcraft_chess::san::{Lang, San};

use super::games::{DiagramAt, GameOrigin, GameScore, MAX_COMMENT_BYTES};
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

/// Why an exercise is written without ChessBase's training question (plan A9-N10c).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrainingLeftOutReason {
    /// Its first move castles: the D18 session saw no castling answer, so its coding is not known.
    Castling,
    /// Its first move promotes a pawn: likewise.
    Promotion,
    /// Its first move is no move of its start position (only in a score read from JSON that was
    /// tampered with: the PGN's writer leaves such a move out too).
    NoMove,
}

impl TrainingLeftOutReason {
    /// The reason's name in the tools' replies.
    pub fn name(self) -> &'static str {
        match self {
            TrainingLeftOutReason::Castling => "castling",
            TrainingLeftOutReason::Promotion => "promotion",
            TrainingLeftOutReason::NoMove => "no_move",
        }
    }
}

/// An exercise written without ChessBase's training question, and why.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrainingLeftOut {
    pub at: DiagramAt,
    pub number: u32,
    pub reason: TrainingLeftOutReason,
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
    /// The exercises written without ChessBase's training question, and why, in the book's order
    /// (plan A9-N10c); `None` when no training question is written (another dialect than
    /// ChessBase's, comments left out, or Anki's cards).
    pub training_left_out: Option<Vec<TrainingLeftOut>>,
}

/// An exercise found, as [`exercises_pgn`] writes it and as Anki's cards show it
/// ([`super::anki`]).
pub(crate) struct Found {
    pub(crate) exercise: Exercise,
    /// Its game, with the origin's tags, the title as its `Event`, its number as its `Round` and
    /// the statement as its first comment.
    pub(crate) score: GameScore,
    /// The side the book prints the diagram from.
    pub(crate) orientation: Color,
    /// The statement's two parts: its first sentence, the side to move and the number
    /// ([`question`])…
    question: String,
    /// … and the rest (where it is in the book, and what the book said before the solution).
    rest: String,
    /// The language the statement is in.
    lang: Lang,
}

/// The exercises of `doc` in the book's order, and the numbered diagrams left out (see the module
/// documentation): the one rule both forms of the exercises follow.
pub(crate) fn find(doc: &ChessDoc, book: &PgnBook) -> (Vec<Found>, Vec<LeftOut>) {
    let mut found = Vec::new();
    let mut left_out = Vec::new();
    for page in &doc.pages {
        let lang = page.text_language();
        for block in &page.blocks {
            let Block::Diagram(d) = block else { continue };
            let Some(number) = d.puzzle else { continue };
            let at = DiagramAt::on(page.index, d.number);
            let solution =
                doc.games.iter().find(|g| matches!(g.origin, GameOrigin::Diagram { page: p, diagram, .. } if p == page.index && diagram == d.number));
            let Some(game) = solution.filter(|g| !g.score.mainline().is_empty()) else {
                left_out.push(LeftOut { at, number, reason: LeftOutReason::NoSolution });
                continue;
            };
            let Some(start) = game.score.start_board() else {
                left_out.push(LeftOut { at, number, reason: LeftOutReason::NoPosition });
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
            let question = question(side, number, lang);
            let rest = score.comment.trim().to_owned();
            score.comment = statement(&question, &rest);
            let exercise = Exercise { at, number, moves: score.mainline().len() };
            found.push(Found { exercise, score, orientation: d.orientation(), question, rest, lang });
        }
    }
    (found, left_out)
}

/// The exercises of `doc` by the same rule ([`find`]), without writing them: where each is, its
/// number and its solution's moves, and the numbered diagrams left out (plan A9-N15a, the
/// Library's index of a book).
pub fn list(doc: &ChessDoc) -> (Vec<Exercise>, Vec<LeftOut>) {
    let (found, left_out) = find(doc, &PgnBook::default());
    (found.into_iter().map(|f| f.exercise).collect(), left_out)
}

/// The exercises of `doc` as a problem PGN, from `book` (its title, its pages' labels), the moves
/// written as `style` says (see the module documentation).
pub fn exercises_pgn(doc: &ChessDoc, book: &PgnBook, style: PgnStyle) -> ExercisesPgn {
    let (mut found, left_out) = find(doc, book);
    // ChessBase's training question (plan A9-N10c), in the comment the writer writes.
    let training_left_out = (style.dialect == PgnDialect::ChessBase && style.comments).then(|| {
        let mut untrained = Vec::new();
        for f in &mut found {
            match training_answer(&f.score) {
                Ok(answer) => f.score.comment = training_comment(f.lang, &f.question, &answer, &f.rest),
                Err(reason) => untrained.push(TrainingLeftOut { at: f.exercise.at, number: f.exercise.number, reason }),
            }
        }
        untrained
    });
    let written = super::pgn::write_report(found.iter().map(|f| &f.score), style);
    ExercisesPgn { pgn: written.pgn, exercises: found.iter().map(|f| f.exercise).collect(), left_out, warnings: written.warnings, training_left_out }
}

/// The statement's first sentence: the side to move and the exercise's number (`Brancas jogam
/// (№ 185).`), in `lang`.
fn question(side: Color, number: u32, lang: Lang) -> String {
    let to_move = describe::to_move(side, lang);
    let mut chars = to_move.chars();
    let head: String = chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default();
    format!("{head} (№ {number}).")
}

/// The statement before an exercise's first move: its `question`, then `rest` (where it is in
/// the book, and what the book said before the solution).
fn statement(question: &str, rest: &str) -> String {
    if rest.is_empty() { question.to_owned() } else { format!("{question} {rest}") }
}

/// The score of the one answer written: the 10 the D18 session saw ChessBase give the first.
const TRAINING_SCORE: u32 = 10;

/// The language field of ChessBase's training question for a statement in `lang`: the tab of
/// ChessBase's dialog in that language (En, De, Fr, Es, It, Ne, Pt, as the D18 session saw them),
/// `En` for any other.
pub(crate) fn training_language(lang: Lang) -> &'static str {
    match lang {
        Lang::De => "De",
        Lang::Fr => "Fr",
        Lang::Es => "Es",
        Lang::It => "It",
        Lang::Nl => "Ne",
        Lang::Pt => "Pt",
        _ => "En",
    }
}

/// `text` as it is safe to write in and around the training command, since how ChessBase escapes
/// was not seen: `"` as `'`, `[` and `]` as `(` and `)` (a `]` ends the command, and the reader
/// takes a `[` before it for one left open), control characters as spaces, whitespace runs one
/// space, the ends trimmed.
fn training_text(text: &str) -> String {
    let safe: String = text
        .chars()
        .map(|c| match c {
            '"' => '\'',
            '[' => '(',
            ']' => ')',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    safe.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// ChessBase's training command asking `question` in the tab of `lang`, `answer` (a move as its
/// two squares) its one answer, with no feedback and [`TRAINING_SCORE`]. The question is made
/// safe ([`training_text`]) and cut, at a character, so the command fits a comment
/// ([`MAX_COMMENT_BYTES`]): the comment's cap never cuts it in two.
pub(crate) fn training_command(lang: Lang, question: &str, answer: &str) -> String {
    let head = format!("[%tqu \"{}\",\"", training_language(lang));
    let tail = format!("\",\"\",\"\",\"{answer}\",\"\",{TRAINING_SCORE}]");
    let room = MAX_COMMENT_BYTES.saturating_sub(head.len()).saturating_sub(tail.len());
    let question = training_text(question);
    let mut end = question.len().min(room);
    while !question.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    let question = question.get(..end).unwrap_or_default().trim_end();
    format!("{head}{question}{tail}")
}

/// The statement's comment with ChessBase's training question: the command first, then `rest`,
/// made safe ([`training_text`]), as ChessBase writes a training question before the first move.
fn training_comment(lang: Lang, question: &str, answer: &str, rest: &str) -> String {
    let command = training_command(lang, question, answer);
    let rest = training_text(rest);
    if rest.is_empty() { command } else { format!("{command} {rest}") }
}

/// The training question's answer for an exercise: the first move of its main line as its two
/// squares (`d1d8`), as the D18 session saw ChessBase write it. A castling or a promotion, which
/// the session did not see, is refused, as is a move its start position does not have.
fn training_answer(score: &GameScore) -> Result<String, TrainingLeftOutReason> {
    let board = score.start_board().ok_or(TrainingLeftOutReason::NoMove)?;
    let first = score.roots.first().and_then(|&n| score.nodes.get(n)).ok_or(TrainingLeftOutReason::NoMove)?;
    // Its SAN, else the move that leads to the position after it (as the PGN's writer names a
    // move whose SAN was edited into no move).
    let by_san = San::read(&first.san, Lang::En).ok().and_then(|san| san.to_move(&board).ok());
    let mv = by_san.or_else(|| move_to(&board, &first.fen)).ok_or(TrainingLeftOutReason::NoMove)?;
    let us = board.side_to_move();
    // cozy-chess writes castling as the king taking its own rook (`e1h1`).
    if mv.from == board.king(us) && board.colors(us).has(mv.to) {
        return Err(TrainingLeftOutReason::Castling);
    }
    if mv.promotion.is_some() {
        return Err(TrainingLeftOutReason::Promotion);
    }
    Ok(format!("{}{}", mv.from, mv.to))
}

/// The legal move on `board` that leads to the position `fen` (castling rights, en passant and the
/// side to move included, the move counters aside).
fn move_to(board: &cozy_chess::Board, fen: &str) -> Option<cozy_chess::Move> {
    use pdfcraft_chess::fen::Fen;
    let want = Fen::from_board(&pdfcraft_chess::fen::board_from_fen(fen.trim()).ok()?).full_key();
    let mut found = None;
    board.generate_moves(|moves| {
        for mv in moves {
            let mut after = board.clone();
            after.play_unchecked(mv);
            if Fen::from_board(&after).full_key() == want {
                found = Some(mv);
                return true;
            }
        }
        false
    });
    found
}
