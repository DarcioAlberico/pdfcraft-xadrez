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
//! comments next to each other are written as one, the way PGN reads them back.

use crate::game::{Game, ROSTER, Token, kept_tokens};
use crate::pgn_style::PgnStyle;
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
            let joined = comment.get_or_insert_with(String::new);
            if !joined.is_empty() && !body.is_empty() {
                joined.push(' ');
            }
            joined.push_str(&body);
            continue;
        }
        if let Some(body) = comment.take() {
            push_comment(&mut out, &body);
        }
        match token {
            Token::Move { number, color, numbered, san, nags, .. } => {
                if numbered {
                    out.push(Piece::Word(match color {
                        Color::White => format!("{number}."),
                        Color::Black => format!("{number}..."),
                    }));
                }
                out.push(Piece::Word(san));
                out.extend(nags.iter().map(|code| Piece::Word(format!("${code}"))));
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

/// A comment as one piece per word, the braces on the first and the last, so a long one wraps.
/// A brace inside (which a comment set on a game never holds) is written as a parenthesis: it
/// would end the comment.
fn push_comment(out: &mut Vec<Piece>, body: &str) {
    let body = body.replace('{', "(").replace('}', ")");
    let words: Vec<&str> = body.split_whitespace().collect();
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
