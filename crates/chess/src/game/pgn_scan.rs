//! The one lexical scanner of PGN (plan A9-F2a): what is a tag, a comment, an escape line, a
//! bracket, a glyph code, a result or a word, decided in one place for every reader.
//!
//! Before it, three lexers each decided what a comment was, and [`split_pgn`](super::split_pgn)
//! counted the braces inside a `;` comment and a `%` line, so one `{` there merged every later
//! game into one (A1-05, A7-01). The readers now all take this scanner's tokens.
//!
//! The rules, from the PGN standard (§8) where it has one:
//!
//! - **A tag** is `[Name "value"]`. Its `[` opens it when it is the first thing on its line
//!   (after spaces) or comes right after another tag on the same line (A1-16); anywhere else a
//!   `[` is part of a word. It ends at its `]` outside the quotes, or at the end of the line.
//!   In the value only `\\` and `\"` are escapes ([`tag_value`]).
//! - **A brace comment** runs from `{` to the next `}`, across lines; a `[` or `;` in it is text.
//!   One never closed runs to the end of the text, and says so.
//! - **A line comment** runs from `;` to the end of its line, and **an escape line** is a line
//!   whose first character is `%`: a brace in either is text.
//! - `(` and `)` are brackets; whitespace separates.
//! - Anything else is a run of characters up to whitespace or one of `{ ( ) ;`: a glyph code
//!   when it is `$` and digits, a result when it is one, else a word (a move, a move number, a
//!   glyph). What a word means is the reader's business.
//!
//! Every token carries the byte range it was read from, so a caller can slice the text (the
//! scanner never copies it). The scanner never fails: any text is a sequence of tokens, and each
//! token moves at least one byte on.

use std::ops::Range;

/// What a token of PGN is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanKind {
    /// `[Name "value"]`, brackets included ([`tag_parts`] reads it).
    Tag,
    /// `{…}`, braces included; `closed` is false when the text ended first.
    BraceComment { closed: bool },
    /// `;…` up to the end of the line (the line break not included).
    LineComment,
    /// A line starting with `%`, up to its end (the line break not included).
    Escape,
    /// `(`.
    Open,
    /// `)`.
    Close,
    /// `$` and digits.
    Nag,
    /// A result: `1-0`, `0-1`, `1/2-1/2` or `*`, or a draw as `½-½`, `1/2` or `0.5-0.5`.
    Result,
    /// Anything else up to whitespace or `{ ( ) ;`.
    Word,
}

impl ScanKind {
    /// Whether the token belongs to a game's moves rather than its tag section: everything but
    /// a tag and an escape line. A tag after one of these starts the next game.
    pub fn is_movetext(self) -> bool {
        !matches!(self, ScanKind::Tag | ScanKind::Escape)
    }
}

/// A token and where in the text it was read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scanned {
    pub kind: ScanKind,
    pub range: Range<usize>,
}

/// The results a word can be: the standard's four, and a draw as `½-½`, `1/2` or `0.5-0.5`.
const RESULTS: [&str; 7] = ["*", "1-0", "0-1", "1/2-1/2", "½-½", "1/2", "0.5-0.5"];

/// The tokens of `text`, in order. A byte order mark the text may start with is not skipped: the
/// callers take it off first ([`crate::pgn::without_bom`]), so ranges stay ranges of what they
/// hold.
pub fn scan(text: &str) -> Scanner<'_> {
    Scanner { text, pos: 0, line_start: true, after_tag: false }
}

/// The iterator [`scan`] returns.
#[derive(Clone, Debug)]
pub struct Scanner<'a> {
    text: &'a str,
    /// Where the next token is looked for (always at a character boundary).
    pos: usize,
    /// Only whitespace since the last line break (or the start).
    line_start: bool,
    /// The last token on this line was a tag (only whitespace after it).
    after_tag: bool,
}

impl Scanner<'_> {
    /// The character at `at`, if any.
    fn char_at(&self, at: usize) -> Option<char> {
        self.text.get(at..)?.chars().next()
    }

    /// The end of the line `at` is on: the byte of its line break, or the end of the text.
    fn line_end(&self, at: usize) -> usize {
        self.text.get(at..).and_then(|rest| rest.find('\n')).map_or(self.text.len(), |i| at.saturating_add(i))
    }

    /// The end of a tag starting at `start` (its `[`): after its `]` outside the quotes, or at
    /// the end of its line.
    fn tag_end(&self, start: usize) -> usize {
        let end = self.line_end(start);
        let body = self.text.get(start..end).unwrap_or_default();
        let mut in_quotes = false;
        let mut chars = body.char_indices().skip(1);
        while let Some((i, c)) = chars.next() {
            match c {
                '"' => in_quotes = !in_quotes,
                // `\\` and `\"` are the escapes: the character after the backslash is skipped.
                '\\' if in_quotes && matches!(body.get(i.saturating_add(1)..).and_then(|s| s.chars().next()), Some('\\' | '"')) => {
                    chars.next();
                }
                ']' if !in_quotes => return start.saturating_add(i).saturating_add(1),
                _ => {}
            }
        }
        end
    }

    /// The end of a brace comment starting at `start` (its `{`), and whether it was closed.
    fn brace_end(&self, start: usize) -> (usize, bool) {
        match self.text.get(start.saturating_add(1)..).and_then(|rest| rest.find('}')) {
            Some(i) => (start.saturating_add(1).saturating_add(i).saturating_add(1), true),
            None => (self.text.len(), false),
        }
    }

    /// The end of a run of characters starting at `start`: before whitespace or `{ ( ) ;`.
    fn word_end(&self, start: usize) -> usize {
        let rest = self.text.get(start..).unwrap_or_default();
        rest.find(|c: char| c.is_whitespace() || matches!(c, '{' | '(' | ')' | ';')).map_or(self.text.len(), |i| start.saturating_add(i))
    }
}

impl Iterator for Scanner<'_> {
    type Item = Scanned;

    fn next(&mut self) -> Option<Scanned> {
        loop {
            let start = self.pos;
            let c = self.char_at(start)?;
            if c.is_whitespace() {
                self.pos = start.saturating_add(c.len_utf8());
                if c == '\n' {
                    (self.line_start, self.after_tag) = (true, false);
                }
                continue;
            }
            // Only the first column of a line starts an escape (§6).
            let column0 = start == 0 || self.text.get(..start).is_some_and(|s| s.ends_with('\n'));
            let (kind, end) = match c {
                '%' if column0 => (ScanKind::Escape, self.line_end(start)),
                '[' if self.line_start || self.after_tag => (ScanKind::Tag, self.tag_end(start)),
                '{' => {
                    let (end, closed) = self.brace_end(start);
                    (ScanKind::BraceComment { closed }, end)
                }
                ';' => (ScanKind::LineComment, self.line_end(start)),
                '(' => (ScanKind::Open, start.saturating_add(1)),
                ')' => (ScanKind::Close, start.saturating_add(1)),
                _ => {
                    let end = self.word_end(start);
                    let word = self.text.get(start..end).unwrap_or_default();
                    let kind = match word.strip_prefix('$') {
                        Some(digits) if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) => ScanKind::Nag,
                        _ if RESULTS.contains(&word) => ScanKind::Result,
                        _ => ScanKind::Word,
                    };
                    (kind, end)
                }
            };
            // Every token holds at least its first character; a brace comment may span lines.
            let end = end.max(start.saturating_add(c.len_utf8()));
            if matches!(kind, ScanKind::BraceComment { .. }) && self.text.get(start..end).is_some_and(|s| s.contains('\n')) {
                self.line_start = false;
            }
            self.after_tag = kind == ScanKind::Tag;
            self.line_start = false;
            self.pos = end;
            return Some(Scanned { kind, range: start..end });
        }
    }
}

/// A tag token's name and value as written (the value still escaped), or `None` when it has no
/// quoted value. What follows the closing quote before `]` is part of the value, as the readers
/// have always taken it.
pub fn tag_parts(token: &str) -> Option<(&str, &str)> {
    let body = token.strip_prefix('[').unwrap_or(token);
    let body = body.strip_suffix(']').unwrap_or(body);
    let (name, value) = body.split_once('"')?;
    Some((name.trim(), value))
}

/// A tag value as read: `\\` is a backslash and `\"` a quote, any other backslash stays (A1-16:
/// `C:\games` is kept as written), and the quotes around it are taken out.
pub fn tag_value(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if matches!(chars.peek(), Some('\\' | '"')) => {
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            }
            '"' => {}
            c => out.push(c),
        }
    }
    out
}

/// A brace comment's text, without its braces.
pub fn brace_body(token: &str) -> &str {
    let body = token.strip_prefix('{').unwrap_or(token);
    body.strip_suffix('}').unwrap_or(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<(ScanKind, &str)> {
        scan(text).map(|t| (t.kind, &text[t.range])).collect()
    }

    #[test]
    fn every_kind_is_told_apart() {
        use ScanKind::*;
        let text = "[Event \"A\"] [Site \"B\"]\n% escape {x\n1. e4 {a [b] c} ; line {y\n(1... c5 $14) e5!? 1-0";
        assert_eq!(
            kinds(text),
            [
                (Tag, "[Event \"A\"]"),
                (Tag, "[Site \"B\"]"),
                (Escape, "% escape {x"),
                (Word, "1."),
                (Word, "e4"),
                (BraceComment { closed: true }, "{a [b] c}"),
                (LineComment, "; line {y"),
                (Open, "("),
                (Word, "1..."),
                (Word, "c5"),
                (Nag, "$14"),
                (Close, ")"),
                (Word, "e5!?"),
                (Result, "1-0"),
            ]
        );
    }

    /// A1-16: a tag opens a line (after spaces) or follows a tag on its line; elsewhere a `[` is
    /// part of a word.
    #[test]
    fn a_tag_opens_its_line_or_follows_a_tag() {
        use ScanKind::*;
        assert_eq!(kinds("  [FEN \"8/8\"]"), [(Tag, "[FEN \"8/8\"]")]);
        assert_eq!(kinds("1. e4 [x]"), [(Word, "1."), (Word, "e4"), (Word, "[x]")]);
        assert_eq!(kinds("1. e4\n[Event \"B\"]"), [(Word, "1."), (Word, "e4"), (Tag, "[Event \"B\"]")]);
        // A bracket in a quoted value does not end it; an escaped quote does not close it.
        assert_eq!(kinds("[Event \"a ] \\\" b\"] e4"), [(Tag, "[Event \"a ] \\\" b\"]"), (Word, "e4")]);
        // Never closed: the tag ends with its line.
        assert_eq!(kinds("[Event \"A\n1. e4"), [(Tag, "[Event \"A"), (Word, "1."), (Word, "e4")]);
    }

    #[test]
    fn an_escape_is_only_in_the_first_column() {
        use ScanKind::*;
        assert_eq!(kinds("e4 %x"), [(Word, "e4"), (Word, "%x")]);
        assert_eq!(kinds(" %x"), [(Word, "%x")]);
        assert_eq!(kinds("e4\n%x {\ne5"), [(Word, "e4"), (Escape, "%x {"), (Word, "e5")]);
    }

    #[test]
    fn a_brace_comment_never_closed_runs_to_the_end() {
        use ScanKind::*;
        assert_eq!(kinds("e4 {open\n[Event \"B\"]"), [(Word, "e4"), (BraceComment { closed: false }, "{open\n[Event \"B\"]")]);
        assert_eq!(kinds("{"), [(BraceComment { closed: false }, "{")]);
        // After a comment that ends on a later line, a `[` there is not at a line start.
        assert_eq!(kinds("{a\nb} [x]"), [(BraceComment { closed: true }, "{a\nb}"), (Word, "[x]")]);
    }

    #[test]
    fn glyph_codes_and_results_are_whole_words() {
        use ScanKind::*;
        assert_eq!(
            kinds("$1 $ $x1 e4$1 * ½-½ 1/2"),
            [(Nag, "$1"), (Word, "$"), (Word, "$x1"), (Word, "e4$1"), (Result, "*"), (Result, "½-½"), (Result, "1/2")]
        );
    }

    #[test]
    fn tag_values_keep_every_backslash_but_the_escapes() {
        assert_eq!(tag_parts("[Annotator \"C:\\games\"]"), Some(("Annotator", "C:\\games\"")));
        assert_eq!(tag_value("C:\\games\""), "C:\\games");
        assert_eq!(tag_value("Club \\\"open\\\" \\\\ x\""), "Club \"open\" \\ x");
        assert_eq!(tag_parts("[Event]"), None);
        assert_eq!(brace_body("{a}"), "a");
        assert_eq!(brace_body("{a"), "a");
    }

    proptest::proptest! {
        /// Any text: the tokens are in order, never overlap, stay on character boundaries, and
        /// only whitespace lies between them.
        #[test]
        fn tokens_cover_the_text(text in "\\PC{0,200}") {
            let mut at = 0;
            for t in scan(&text) {
                proptest::prop_assert!(t.range.start >= at && t.range.end > t.range.start);
                proptest::prop_assert!(text.is_char_boundary(t.range.start) && text.is_char_boundary(t.range.end));
                proptest::prop_assert!(text[at..t.range.start].chars().all(char::is_whitespace));
                at = t.range.end;
            }
            proptest::prop_assert!(text[at..].chars().all(char::is_whitespace));
        }
    }
}
