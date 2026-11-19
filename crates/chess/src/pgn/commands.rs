//! Commands inside a PGN comment (plan A9-F7, finding R6-01): `[%clk 0:03:00]`, `[%eval 0.17]`,
//! `[%emt 0:00:05]`, `[%csl Ga4]`, `[%evp …]`, `[%tqu …]` and any other `[%name …]`, and
//! ChessBase's diagram mark `[#]` (the D18 session, 2026-11-19: ChessBase writes a diagram as the
//! comment `{[#]}`, not as a glyph).
//!
//! PGN's standard (§8.2.5) has no commands; programs put them in comments, in the convention
//! ChessBase started and lichess and chess.com follow: a `[`, a `%`, a name of letters and digits,
//! then what it says up to the `]`. Every game downloaded from lichess or chess.com has a clock in
//! the comment of every move, so a comment holding only commands is not a comment a person wrote:
//! the move list, the comment under the board and the exports show the comment's **prose**
//! ([`prose`]), and the game keeps the comment whole, so the PGN written ([`super::write`]) gives
//! every command back where it was.
//!
//! [`split_commands`] is total and linear on any text: an unclosed `[%`, a `[` before the `]`
//! (one command never holds another) or a name that is not one leaves the text as prose, and the
//! prose and the commands give the text back ([`join_commands`]), its whitespace aside.

use std::borrow::Cow;
use std::ops::Range;

use crate::judge::Value;

/// Longest command name read (`eval`, `clk`; a longer run of letters is not taken for one).
const MAX_NAME: usize = 32;

/// ChessBase's diagram mark in a comment: a command with no `%` and no arguments.
pub const DIAGRAM_COMMAND: &str = "[#]";

/// One `[%name args]` command of a comment (or ChessBase's diagram mark, [`DIAGRAM_COMMAND`]),
/// kept as it was read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    /// The command as written, from its `[` to its `]`.
    pub text: String,
    /// Where it stood: a byte offset into the prose [`split_commands`] returned with it (a
    /// character boundary; the commands of one comment are in order).
    pub at: usize,
}

impl Command {
    /// Its name, after the `%` (`clk`, `eval`), as written; `#` for ChessBase's diagram mark.
    pub fn name(&self) -> &str {
        if self.text == DIAGRAM_COMMAND {
            return "#";
        }
        let body = self.body();
        let end = body.find(|c: char| !is_name_char(c)).unwrap_or(body.len());
        body.get(..end).unwrap_or_default()
    }

    /// What it says after its name, trimmed (`0:03:00`).
    pub fn args(&self) -> &str {
        if self.text == DIAGRAM_COMMAND {
            return "";
        }
        let body = self.body();
        body.get(self.name().len()..).unwrap_or_default().trim()
    }

    /// Between `[%` and `]`.
    fn body(&self) -> &str {
        let inner = self.text.strip_prefix("[%").unwrap_or(&self.text);
        inner.strip_suffix(']').unwrap_or(inner)
    }
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// The length in bytes of the command `from` starts with (`from` starts with `[`), or `None`
/// when it starts none: ChessBase's `[#]`, or `[%`, a name of letters, digits and `_` that starts
/// with a letter, then a space or the `]`, and the `]` before any other `[`.
fn command_len(from: &str) -> Option<usize> {
    if from.starts_with(DIAGRAM_COMMAND) {
        return Some(DIAGRAM_COMMAND.len());
    }
    let after = from.strip_prefix("[%")?;
    let name = after.find(|c: char| !is_name_char(c)).unwrap_or(after.len());
    if name == 0 || name > MAX_NAME || !after.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return None;
    }
    let rest = after.get(name..)?;
    if !(rest.starts_with(']') || rest.starts_with(char::is_whitespace)) {
        return None;
    }
    // The first bracket after the name: a `]` ends the command, a `[` (another command, or a
    // stray bracket) means this one is not closed.
    let close = rest.find(['[', ']'])?;
    if !rest.get(close..)?.starts_with(']') {
        return None;
    }
    // `[%`, the name, what follows it and the `]`: all ASCII but the arguments, whose end is a
    // character boundary (the `]`).
    2usize.checked_add(name)?.checked_add(close)?.checked_add(1)
}

/// The byte ranges of `text`'s commands, in order.
pub(crate) fn command_spans(text: &str) -> Vec<Range<usize>> {
    let mut spans = Vec::new();
    let mut from = 0usize;
    while let Some(found) = text.get(from..).and_then(|rest| rest.find('[')) {
        let start = from.saturating_add(found);
        match text.get(start..).and_then(command_len) {
            Some(len) => {
                let end = start.saturating_add(len);
                spans.push(start..end);
                from = end;
            }
            // Not a command: look again after its `[` (ASCII, so `start + 1` is a boundary). A
            // `[%` that is not closed stops its search at the next `[`, where the next look
            // starts, so each character is looked at a bounded number of times, however hostile
            // the text.
            None => from = start.saturating_add(1),
        }
    }
    spans
}

/// Append the words of `text` to `prose`, one space before each but the first. The pieces of
/// text between two commands are words apart: a command glued between two words leaves a space.
fn push_words(prose: &mut String, text: &str) {
    for word in text.split_whitespace() {
        if !prose.is_empty() {
            prose.push(' ');
        }
        prose.push_str(word);
    }
}

/// A comment's prose and its commands: the text without its `[%…]` commands, each run of
/// whitespace one space and the ends trimmed (a command between two words leaves a space), and the
/// commands as read, each with the place in the prose where it stood.
pub fn split_commands(text: &str) -> (String, Vec<Command>) {
    let mut prose = String::with_capacity(text.len());
    let mut commands = Vec::new();
    let mut last = 0usize;
    for span in command_spans(text) {
        push_words(&mut prose, text.get(last..span.start).unwrap_or_default());
        commands.push(Command { text: text.get(span.clone()).unwrap_or_default().to_owned(), at: prose.len() });
        last = span.end;
    }
    push_words(&mut prose, text.get(last..).unwrap_or_default());
    (prose, commands)
}

/// Whether `text` may hold a command: a `[%` or ChessBase's `[#]`.
fn may_hold_commands(text: &str) -> bool {
    text.contains("[%") || text.contains(DIAGRAM_COMMAND)
}

/// The prose of a comment ([`split_commands`]): the text itself when it holds no command, so a
/// comment without commands costs nothing.
pub fn prose(text: &str) -> Cow<'_, str> {
    if may_hold_commands(text) { Cow::Owned(split_commands(text).0) } else { Cow::Borrowed(text) }
}

/// Whether a comment holds ChessBase's diagram mark, `[#]` ([`DIAGRAM_COMMAND`]): a diagram of the
/// position after the move, as the glyphs [`crate::nag::is_diagram`] names are.
pub fn has_diagram_command(text: &str) -> bool {
    text.contains(DIAGRAM_COMMAND)
}

/// `text` without its diagram marks `[#]`, whitespace runs made one space: what is left when a
/// diagram is taken off a move.
pub fn without_diagram_command(text: &str) -> String {
    diagram_command_as(text, "")
}

/// `text` with each diagram mark `[#]` written as `word` (a word apart), whitespace runs made one
/// space.
pub fn diagram_command_as(text: &str, word: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0usize;
    for span in command_spans(text).into_iter().filter(|s| text.get(s.clone()) == Some(DIAGRAM_COMMAND)) {
        out.push_str(text.get(last..span.start).unwrap_or_default());
        out.push(' ');
        out.push_str(word);
        out.push(' ');
        last = span.end;
    }
    out.push_str(text.get(last..).unwrap_or_default());
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether a comment says anything besides its commands.
pub fn has_prose(text: &str) -> bool {
    !prose(text).trim().is_empty()
}

/// The comment the prose and the commands of [`split_commands`] came from: each command put back
/// at its place, a space either side, whitespace runs made one space. The text split, but for
/// its whitespace. A place past the prose's end (or inside a character) is taken as the nearest
/// boundary before it.
pub fn join_commands(prose: &str, commands: &[Command]) -> String {
    let mut out = String::with_capacity(prose.len().saturating_add(commands.iter().map(|c| c.text.len().saturating_add(2)).sum()));
    let mut last = 0usize;
    for command in commands {
        let mut at = command.at.clamp(last, prose.len());
        while !prose.is_char_boundary(at) {
            at = at.saturating_sub(1);
        }
        let at = at.max(last);
        out.push_str(prose.get(last..at).unwrap_or_default());
        out.push(' ');
        out.push_str(&command.text);
        out.push(' ');
        last = at;
    }
    out.push_str(prose.get(last..).unwrap_or_default());
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The first command named `name` (`clk`), case as written.
fn named<'a>(commands: &'a [Command], name: &str) -> Option<&'a Command> {
    commands.iter().find(|c| c.name() == name)
}

/// Longest clock or evaluation shown in a badge, in characters: anything longer is not one.
const MAX_BADGE: usize = 16;

/// The clock a move's commands give, as a badge shows it: the time left (`%clk`), else the
/// time the move took (`%emt`), with the leading zero hours and tens left out (`0:02:58` is
/// `2:58`). `None` when there is none, or it is not a time (digits, `:` and `.`).
pub fn shown_clock(commands: &[Command]) -> Option<String> {
    let time = named(commands, "clk").or_else(|| named(commands, "emt"))?.args();
    let time = time.split(',').next().unwrap_or_default().trim();
    let well_formed = !time.is_empty()
        && time.chars().count() <= MAX_BADGE
        && time.starts_with(|c: char| c.is_ascii_digit())
        && time.chars().all(|c| c.is_ascii_digit() || c == ':' || c == '.');
    if !well_formed {
        return None;
    }
    let mut short = time;
    // Hours of 0 are left out when minutes follow, and then a leading 0 of the minutes.
    if let Some(rest) = short.strip_prefix("0:").filter(|r| r.contains(':')) {
        short = rest;
    }
    if short.len() > 1 && short.starts_with('0') && short.get(1..2).is_some_and(|c| c.chars().all(|c| c.is_ascii_digit())) {
        short = short.get(1..).unwrap_or(short);
    }
    Some(short.to_owned())
}

/// The evaluation a move's commands give (`%eval`, for White), as a badge shows it: `+0.17`,
/// `−1.50` (a minus sign), `#3` (White mates in 3), `#−2`. `None` when there is none, or it is not
/// a number of pawns or a mate.
pub fn shown_eval(commands: &[Command]) -> Option<String> {
    let value = named(commands, "eval")?.args();
    // `[%eval 0.17,22]`: the value, then the depth some programs write.
    let value = value.split(',').next().unwrap_or_default().trim();
    if value.is_empty() || value.chars().count() > MAX_BADGE {
        return None;
    }
    let (mate, number) = match value.strip_prefix('#') {
        Some(n) => (true, n),
        None => (false, value),
    };
    let (negative, digits) = match number.strip_prefix('-') {
        Some(d) => (true, d),
        None => (false, number.strip_prefix('+').unwrap_or(number)),
    };
    let mut parts = digits.splitn(2, '.');
    let whole = parts.next().unwrap_or_default();
    let fraction = parts.next();
    let digits_only = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    let well_formed = if mate { digits_only(whole) && fraction.is_none() } else { digits_only(whole) && fraction.is_none_or(digits_only) };
    if !well_formed {
        return None;
    }
    // A level score has no sign, as an engine's 0.00 has none.
    let zero = !mate && digits.chars().all(|c| c == '0' || c == '.');
    let sign = if zero || (mate && !negative) {
        ""
    } else if negative {
        "\u{2212}"
    } else {
        "+"
    };
    Some(if mate { format!("#{sign}{digits}") } else { format!("{sign}{digits}") })
}

/// An engine's value for White as the evaluation command lichess reads and writes: `[%eval 0.17]`,
/// `[%eval -1.50]`, `[%eval #3]` (White mates in 3), `[%eval #-2]` (Black does). `None` for a
/// mate already on the board (`#0`, which says nothing the move's `#` does not).
pub fn eval_command(value: Value) -> Option<String> {
    let text = match value {
        Value::Cp(cp) => {
            let sign = if cp < 0 { "-" } else { "" };
            let abs = cp.unsigned_abs();
            format!("{sign}{}.{:02}", abs / 100, abs % 100)
        }
        Value::Mates(0) | Value::Mated(0) => return None,
        Value::Mates(n) => format!("#{n}"),
        Value::Mated(n) => format!("#-{n}"),
    };
    Some(format!("[%eval {text}]"))
}

#[cfg(test)]
pub(crate) mod tests;
