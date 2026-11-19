//! The one PGN writer (plan A9-F4; A1-11, A2-09): the board's game ([`Game::pgn`]), the
//! collection saved, the games of a read book and the book's PGN with the book's text are all
//! written here, the same way.
//!
//! **Tags** first, one to a line: the Seven Tag Roster in its order, `SetUp` and `FEN` when the
//! game does not start from the standard position, then the others as they were read or set. Two
//! of them say what the moves say, and follow them (R6-08): `PlyCount` is written from the main
//! line once that is no longer as long as when the tag was read or set, and `ECO` is left out
//! once the main line no longer opens with the moves it had then (its first [`ECO_PLIES`]).
//!
//! **Move text** after a blank line, wrapped at [`LINE_WIDTH`] columns as PGN's export format
//! asks (decision D19: the copy to the clipboard too): one space between pieces, none after a
//! variation's `(` or before its `)`; a line breaks only between pieces (a move number, a move, a
//! glyph, a word of a comment), never inside one, so a piece longer than a line stands alone;
//! and no line starts with `%`, PGN's escape. A comment before a move comes right before it,
//! after the variations of the move before (A1-06/A2-02: `1. e4 {A} (1. d4 d5) {B} 1... e5`);
//! comments next to each other are written as one, the way PGN reads them back. A comment's
//! `[%…]` commands (`[%clk 0:03:00]`) are written where they were read, each one piece, so a line
//! never breaks inside one (plan A9-F7).
//!
//! **Dialects** ([`PgnDialect`], plan A9-F7): standard PGN writes every glyph as read; the Lichess
//! dialect writes the glyphs lichess knows, Black's code of a pair as White's, and the others as
//! their signs in the move's comment ([`crate::nag::lichess`]); the ChessBase dialect writes
//! Black's code of a pair as White's and a diagram mark as ChessBase's comment `{[#]}`.

use crate::game::{Game, ROSTER, Token, kept_tokens};
use crate::nag;
use crate::pgn_style::{PgnDialect, PgnStyle};
use crate::placement::{Color, PieceKind};
use crate::san::figurine_piece;

/// Longest move-text line written, in characters (a longer piece stands alone).
pub const LINE_WIDTH: usize = 79;

/// The moves of the main line an `ECO` code is kept for: moves 1 to 9 of both sides. Any change
/// among them leaves the code out of what is written.
pub const ECO_PLIES: usize = 18;

/// How a game is written.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WriteOptions {
    /// What of the moves is written (everything by default).
    pub style: PgnStyle,
    /// Figurines in comments written as English letters (`♘f3` as `Nf3`, a pawn's as nothing),
    /// for the programs that read PGN's Latin-1 only: the exports of a book ask for it; the
    /// board's own PGN keeps a comment as it was typed.
    pub letters: bool,
}

/// One game as PGN: its tags, a blank line, the move text and the result, a line break.
pub fn write(game: &Game, options: WriteOptions) -> String {
    let mut out = String::new();
    tags(game, &mut out);
    out.push('\n');
    let mut pieces = pieces(game, options);
    pieces.push(Piece::Word(game.result()));
    wrap(&pieces, &mut out);
    out
}

/// Games one after the other, a blank line between them.
pub fn write_games<'a>(games: impl IntoIterator<Item = &'a Game>, options: WriteOptions) -> String {
    let mut out = String::new();
    for game in games {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&write(game, options));
    }
    out
}

/// The move text alone, on one line and without the result: what [`write`] wraps.
pub fn movetext(game: &Game, options: WriteOptions) -> String {
    let mut out = String::new();
    let mut after_open = true;
    for piece in pieces(game, options) {
        if !(after_open || matches!(piece, Piece::Close)) {
            out.push(' ');
        }
        out.push_str(piece.text());
        after_open = matches!(piece, Piece::Open);
    }
    out
}

/// The tags, one to a line.
fn tags(game: &Game, out: &mut String) {
    for name in ROSTER {
        let value = match name {
            "Result" => game.result(),
            "Date" => game.tag(name).unwrap_or("????.??.??").to_owned(),
            _ => game.tag(name).unwrap_or("?").to_owned(),
        };
        tag(out, name, &value);
    }
    // The start position before the other tags, as a book's origin wants them (plan A8 P8a).
    if !game.starts_standard() {
        tag(out, "SetUp", "1");
        tag(out, "FEN", &game.start_fen());
    }
    for (name, value) in game.tags().iter().filter(|(n, _)| !ROSTER.contains(&n.as_str())) {
        match name.as_str() {
            "PlyCount" => {
                if let Some(count) = game.written_ply_count() {
                    tag(out, name, &count);
                }
            }
            "ECO" if !game.eco_holds() => {}
            _ => tag(out, name, value),
        }
    }
}

/// `[Name "value"]`, the value's `\` and `"` escaped and any control character a space (a line
/// break would end the tag).
fn tag(out: &mut String, name: &str, value: &str) {
    out.push('[');
    out.push_str(name);
    out.push_str(" \"");
    for c in value.chars() {
        match c {
            '\\' | '"' => {
                out.push('\\');
                out.push(c);
            }
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out.push_str("\"]\n");
}

/// A piece of move text: a variation's bracket, or a word (a move number, a move, a glyph, a
/// word of a comment with its brace).
#[derive(Debug, PartialEq, Eq)]
enum Piece {
    Open,
    Close,
    Word(String),
}

impl Piece {
    fn text(&self) -> &str {
        match self {
            Piece::Open => "(",
            Piece::Close => ")",
            Piece::Word(w) => w,
        }
    }
}

/// The move text's pieces, in PGN order, as `options` write them.
fn pieces(game: &Game, options: WriteOptions) -> Vec<Piece> {
    let mut out = Vec::new();
    // Comments next to each other, joined into one.
    let mut comment: Option<String> = None;
    for token in kept_tokens(game.tokens(), options.style) {
        if let Token::Comment { node, text, before } = &token {
            let body = game.comment_body(*node, text, *before);
            let body = if options.letters { comment_letters(&body) } else { body };
            // Lichess reads no diagram mark: ChessBase's `[#]` is the word, as a diagram glyph is.
            let body = if options.style.dialect == PgnDialect::Lichess && crate::pgn::has_diagram_command(&body) {
                crate::pgn::diagram_command_as(&body, LICHESS_DIAGRAM)
            } else {
                body
            };
            let joined = comment.get_or_insert_with(String::new);
            if !joined.is_empty() && !body.is_empty() {
                joined.push(' ');
            }
            joined.push_str(&body);
            continue;
        }
        let after_comment = match comment.take() {
            Some(body) => {
                push_comment(&mut out, &body);
                true
            }
            None => false,
        };
        match token {
            Token::Move { node, number, color, numbered, san, nags, .. } => {
                // A Black move right after a comment says its number (a comment a dialect made of
                // a glyph is one the tokens did not have).
                if numbered || after_comment {
                    out.push(Piece::Word(match color {
                        Color::White => format!("{number}."),
                        Color::Black => format!("{number}..."),
                    }));
                }
                out.push(Piece::Word(san));
                match options.style.dialect {
                    PgnDialect::Standard => out.extend(nags.iter().map(|code| Piece::Word(format!("${code}")))),
                    PgnDialect::Lichess => {
                        let mut said = lichess_glyphs(&nags, &mut out);
                        // A comment with ChessBase's mark says Diagram already.
                        if crate::pgn::has_diagram_command(game.comment(node)) {
                            said.retain(|w| !w.starts_with(LICHESS_DIAGRAM));
                        }
                        // What lichess has no glyph for opens the move's comment (the move's own
                        // comment, next, joins it), unless comments are left out.
                        if options.style.comments && !said.is_empty() {
                            comment = Some(said.join(" "));
                        }
                    }
                    PgnDialect::ChessBase => {
                        // ChessBase's own diagram mark opens the move's comment, unless the
                        // comment has it already (read from ChessBase) or comments are left out.
                        let diagram = chessbase_glyphs(&nags, &mut out);
                        if diagram && options.style.comments && !crate::pgn::has_diagram_command(game.comment(node)) {
                            comment = Some(crate::pgn::DIAGRAM_COMMAND.to_owned());
                        }
                    }
                }
            }
            Token::VariationStart => out.push(Piece::Open),
            Token::VariationEnd => out.push(Piece::Close),
            Token::Comment { .. } => {}
        }
    }
    if let Some(body) = comment.take() {
        push_comment(&mut out, &body);
    }
    out
}

/// What the Lichess dialect writes for a diagram mark, in the move's comment.
const LICHESS_DIAGRAM: &str = "Diagram";

/// The glyphs `nags` as the Lichess dialect writes them: those lichess knows pushed onto `out`,
/// each once (`$37` as `$36`), and the signs or names of the others, each once, returned for the
/// move's comment ([`nag::lichess`]).
fn lichess_glyphs(nags: &[u16], out: &mut Vec<Piece>) -> Vec<String> {
    let mut written: Vec<u16> = Vec::with_capacity(nags.len());
    let mut said: Vec<String> = Vec::new();
    for &code in nags {
        match nag::lichess(code) {
            Ok(known) => {
                if !written.contains(&known) {
                    written.push(known);
                }
            }
            Err(word) => {
                if !said.contains(&word) {
                    said.push(word);
                }
            }
        }
    }
    out.extend(written.iter().map(|code| Piece::Word(format!("${code}"))));
    said
}

/// The glyphs `nags` as the ChessBase dialect writes them: pushed onto `out`, Black's code of a
/// pair as White's, each once, and the diagram marks left out; whether there was one (the
/// dialect writes ChessBase's `{[#]}` for it).
fn chessbase_glyphs(nags: &[u16], out: &mut Vec<Piece>) -> bool {
    let mut written: Vec<u16> = Vec::with_capacity(nags.len());
    let mut diagram = false;
    for &code in nags {
        if nag::is_diagram(code) {
            diagram = true;
            continue;
        }
        let white = nag::white_code(code);
        if !written.contains(&white) {
            written.push(white);
        }
    }
    out.extend(written.iter().map(|code| Piece::Word(format!("${code}"))));
    diagram
}

/// A comment's words for wrapping: split at whitespace, but never inside a `[%…]` command, which
/// is one word (its own whitespace runs made one space, as reading makes them).
fn comment_words(body: &str) -> Vec<String> {
    let spans = super::commands::command_spans(body);
    let mut spans = spans.iter().peekable();
    let mut words = Vec::new();
    let mut word = String::new();
    for (i, c) in body.char_indices() {
        while spans.peek().is_some_and(|s| s.end <= i) {
            spans.next();
        }
        let inside = spans.peek().is_some_and(|s| s.start <= i);
        if !c.is_whitespace() {
            word.push(c);
        } else if inside {
            // A command never starts or ends with whitespace, so the word has its `[%` already.
            if !word.ends_with(' ') {
                word.push(' ');
            }
        } else if !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

/// A comment as one piece per word (a command is one word), the braces on the first and the
/// last, so a long one wraps. A brace inside (which a comment set on a game never holds) is
/// written as a parenthesis: it would end the comment.
fn push_comment(out: &mut Vec<Piece>, body: &str) {
    let body = body.replace('{', "(").replace('}', ")");
    let words = comment_words(&body);
    let last = words.len().saturating_sub(1);
    if words.is_empty() {
        out.push(Piece::Word("{}".to_owned()));
        return;
    }
    for (i, word) in words.iter().enumerate() {
        let mut piece = String::with_capacity(word.len().saturating_add(2));
        if i == 0 {
            piece.push('{');
        }
        piece.push_str(word);
        if i == last {
            piece.push('}');
        }
        out.push(Piece::Word(piece));
    }
}

/// A comment as [`WriteOptions::letters`] writes it: its figurines as English letters (a pawn's
/// as nothing: `♙e4` is `e4`); a comment the letters would empty (a pawn's figurine alone) is
/// kept as it is.
pub fn comment_letters(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match figurine_piece(c) {
            Some(PieceKind::Pawn) => {}
            Some(kind) => out.push(kind.letter()),
            None => out.push(c),
        }
    }
    if out.trim().is_empty() { text.to_owned() } else { out }
}

/// The pieces as lines of at most [`LINE_WIDTH`] characters, each ended by a line break.
fn wrap(pieces: &[Piece], out: &mut String) {
    let mut line = String::new();
    let mut width = 0usize;
    let mut after_open = false;
    for piece in pieces {
        let text = piece.text();
        let w = text.chars().count();
        let space = !line.is_empty() && !after_open && !matches!(piece, Piece::Close);
        after_open = matches!(piece, Piece::Open);
        let fits = width.saturating_add(usize::from(space)).saturating_add(w) <= LINE_WIDTH;
        // A piece that would start a line with `%` (PGN's escape) stays on the line before.
        if !line.is_empty() && !fits && !text.starts_with('%') {
            out.push_str(&line);
            out.push('\n');
            line.clear();
            width = 0;
        } else if space {
            line.push(' ');
            width = width.saturating_add(1);
        }
        line.push_str(text);
        width = width.saturating_add(w);
    }
    if !line.is_empty() {
        out.push_str(&line);
        out.push('\n');
    }
}

#[cfg(test)]
mod tests;
