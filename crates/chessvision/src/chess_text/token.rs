//! Steps 2 to 4: publisher symbols, words and runs.
//!
//! A line arrives as cells (characters with boxes) split into words at the word gaps. Each word
//! is normalized ([`normalize_symbols`]), counted for the language ([`tally_word`]) and cut into
//! tokens ([`tokenize`]); [`assemble`] then turns a line's tokens into [`ChessSpan`]s, keeping
//! track of whose move it is so side-dependent annotations get the right code.

use std::ops::Range;
use std::sync::OnceLock;

use pdfcraft_chess::nag::{self, NAGS};
use pdfcraft_chess::placement::PieceKind;
use pdfcraft_chess::san::{Lang, MAX_SAN_LEN, San, SanBody, figurine_piece};

use super::glyph::Cell;
use super::lang::LangTally;
use super::{ChessLine, ChessRun, ChessSpan};
use crate::coords::ViewRect;

/// Words longer than this are prose (or noise) and are not parsed.
const MAX_WORD_CHARS: usize = 64;
/// Longest publisher alias, in characters (`+/=`, `=/∞`).
const MAX_ALIAS_CHARS: usize = 4;
/// Annotation characters allowed after a move: `+!?`, `#⩲`, `!!N`.
const MAX_TAIL_CHARS: usize = 8;
/// Digits of a move number: games end long before move 1000.
const MAX_NUMBER_DIGITS: usize = 3;

/// Punctuation that wraps a word without being part of it.
const LEADING: &[char] = &['(', '[', '{', '"', '\'', '«', '“', '‘'];
/// Closing punctuation, and footnote marks: a `¹` glued after a move is a footnote, which
/// `normalize_publisher_symbols` leaves alone there (`²` and `³` are always evaluations).
const TRAILING: &[char] = &[')', ']', '}', ',', ';', ':', '"', '\'', '»', '”', '’', '¹', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹', '⁰'];

fn union(rects: impl IntoIterator<Item = [f32; 4]>) -> Option<[f32; 4]> {
    rects.into_iter().fold(None, |acc, r| {
        Some(match acc {
            None => r,
            Some(a) => [a[0].min(r[0]), a[1].min(r[1]), a[2].max(r[2]), a[3].max(r[3])],
        })
    })
}

fn rect_of(cells: &[Cell]) -> Option<[f32; 4]> {
    union(cells.iter().map(|c| c.rect))
}

fn view(rect: [f32; 4]) -> ViewRect {
    ViewRect::new(rect[0], rect[1], rect[2], rect[3])
}

/// Split a line into words at its word gaps.
pub(crate) fn words(line: &[Cell]) -> impl Iterator<Item = &[Cell]> {
    line.split(|c| c.ch.is_whitespace()).filter(|w| !w.is_empty())
}

fn normalized(chars: &[char]) -> Vec<char> {
    let s: String = chars.iter().collect();
    nag::normalize_publisher_symbols(&s).0.chars().collect()
}

/// Step 2: `normalize_publisher_symbols` on a word, keeping each character's box. An alias of
/// several characters (`+/=`) becomes one glyph with the union of their boxes. The word is
/// still the raw extracted text (figurines aside): nothing else has normalized it.
///
/// Words longer than [`MAX_WORD_CHARS`] are prose or noise and come back as they are: aligning
/// the aliases re-normalizes the rest of the word at each one, which is quadratic in its length.
pub(crate) fn normalize_symbols(word: &[Cell]) -> Vec<Cell> {
    if word.len() > MAX_WORD_CHARS {
        return word.to_vec();
    }
    let src: Vec<char> = word.iter().map(|c| c.ch).collect();
    let out = normalized(&src);
    if out == src {
        return word.to_vec();
    }
    // The normalizer replaces aliases left to right and copies everything else, so a character
    // that differs from the source starts an alias; its length is the one after which the rest
    // of the source normalizes to the rest of the output.
    let mut cells = Vec::with_capacity(out.len());
    let (mut i, mut j) = (0, 0);
    while let (Some(s), Some(o)) = (src.get(i), out.get(j)) {
        if s == o {
            if let Some(cell) = word.get(i) {
                cells.push(*cell);
            }
            i += 1;
        } else {
            let rest_out = out.get(j + 1..).unwrap_or(&[]);
            let Some(k) = (1..=MAX_ALIAS_CHARS).find(|k| src.get(i + k..).is_some_and(|rest| normalized(rest) == rest_out)) else { break };
            let rect = word.get(i..i + k).and_then(rect_of).unwrap_or([0.0; 4]);
            cells.push(Cell::new(*o, rect));
            i += k;
        }
        j += 1;
    }
    if i == src.len() && j == out.len() {
        return cells;
    }
    // Not aligned (cannot happen with the normalizer as written): every character gets the
    // word's box.
    let rect = rect_of(word).unwrap_or([0.0; 4]);
    out.into_iter().map(|c| Cell::new(c, rect)).collect()
}

/// One annotation symbol after a move, or a check mark.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mark {
    Check(char),
    Nag(&'static str),
}

/// The canonical annotation glyphs, longest first. `standalone`: only those that can stand alone
/// in text (not the novelty `N`, not `RR`). Aliases are not here: [`normalize_symbols`] has
/// already turned them into these glyphs, and is the one place that does.
fn symbols(standalone: bool) -> &'static [&'static str] {
    static AFTER_MOVE: OnceLock<Vec<&'static str>> = OnceLock::new();
    static STANDALONE: OnceLock<Vec<&'static str>> = OnceLock::new();
    let build = || {
        let mut v: Vec<&'static str> = NAGS.iter().filter(|n| n.safe_glyph || (n.suffix_only && !standalone)).map(|n| n.glyph).collect();
        v.sort_by_key(|s| std::cmp::Reverse(s.chars().count()));
        v
    };
    if standalone { STANDALONE.get_or_init(build) } else { AFTER_MOVE.get_or_init(build) }
}

/// Read `chars` as annotation marks only, each with the characters it covers. After a move
/// (`standalone == false`) check and mate marks are allowed too. `None` if anything else is
/// there.
fn split_marks(chars: &[char], standalone: bool) -> Option<Vec<(Mark, Range<usize>)>> {
    let symbols = symbols(standalone);
    let s: String = chars.iter().collect();
    let mut out = Vec::new();
    let (mut byte, mut at) = (0, 0);
    while let Some(rest) = s.get(byte..).filter(|r| !r.is_empty()) {
        if let Some(sym) = symbols.iter().find(|sym| rest.starts_with(**sym)) {
            let n = sym.chars().count();
            out.push((Mark::Nag(sym), at..at + n));
            byte += sym.len();
            at += n;
        } else {
            match rest.chars().next() {
                Some(c @ ('+' | '#')) if !standalone => {
                    out.push((Mark::Check(c), at..at + 1));
                    byte += 1;
                    at += 1;
                }
                _ => return None,
            }
        }
    }
    Some(out)
}

/// A move read from the start of a word.
struct MoveParse {
    san: San,
    /// Characters of the move proper (before its marks).
    head: usize,
    marks: Vec<(Mark, Range<usize>)>,
}

/// Whether a lexically valid SAN is shaped like a move a book prints: pawn captures name their
/// file (`xf6` is not a move), pawns promote exactly on the last rank, pieces never promote.
fn plausible(san: &San) -> bool {
    match san.body {
        SanBody::Castle(_) => true,
        SanBody::Null => false,
        SanBody::Move { piece: Some(_), promotion, .. } => promotion.is_none(),
        SanBody::Move { piece: None, from_file, from_rank, capture, to, promotion } => {
            let last_rank = to.rank() == 0 || to.rank() == 7;
            from_rank.is_none() && capture == from_file.is_some() && promotion.is_some() == last_rank
        }
    }
}

/// Read `chars` (a whole word core) as a move followed by annotation marks.
fn parse_move(chars: &[char], lang: Lang) -> Option<MoveParse> {
    if chars.len() < 2 || chars.len() > MAX_SAN_LEN + MAX_TAIL_CHARS {
        return None;
    }
    // The longest move first, so `e8Q` is a promotion and `Nf4N` falls back to `Nf4` + novelty.
    for head in (2..=chars.len().min(MAX_SAN_LEN)).rev() {
        let (Some(body), Some(tail)) = (chars.get(..head), chars.get(head..)) else { continue };
        if tail.len() > MAX_TAIL_CHARS || body.last().is_some_and(|c| matches!(c, '+' | '#' | '!' | '?')) {
            continue;
        }
        let Some(marks) = split_marks(tail, false) else { continue };
        let Ok(mut san) = San::read(&body.iter().collect::<String>(), lang) else { continue };
        if !san.suffix.is_empty() || !plausible(&san) {
            continue;
        }
        let checks: String = marks.iter().filter_map(|(m, _)| if let Mark::Check(c) = m { Some(*c) } else { None }).collect();
        san.suffix = if checks == "++" { "#".to_owned() } else { checks };
        return Some(MoveParse { san, head, marks });
    }
    None
}

fn is_piece_move(m: &MoveParse) -> bool {
    matches!(m.san.body, SanBody::Move { piece: Some(_), .. })
}

/// A word cut into its parts: wrapping punctuation, a move number, and the core between.
struct Parts {
    lead: usize,
    /// `(number, black, characters)`: `12.`, `7...`, `7…`.
    number: Option<(u16, bool, usize)>,
    /// The core after the number, without wrapping punctuation or a final full stop.
    core: Range<usize>,
}

fn is_ellipsis(chars: &[char]) -> bool {
    matches!(chars, ['…'] | ['.', '.'] | ['.', '.', '.'])
}

fn move_number(chars: &[char]) -> Option<(u16, bool, usize)> {
    let digits = chars.iter().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 || digits > MAX_NUMBER_DIGITS {
        return None;
    }
    let number: u16 = chars.get(..digits)?.iter().collect::<String>().parse().ok().filter(|n| *n > 0)?;
    let rest = chars.get(digits..)?;
    let dots = rest.iter().take_while(|c| **c == '.').count().min(3);
    match rest.first() {
        Some('…') => Some((number, true, digits + 1)),
        Some('.') => Some((number, dots >= 2, digits + dots)),
        _ => None,
    }
}

fn parts(chars: &[char]) -> Parts {
    let lead = chars.iter().take_while(|c| LEADING.contains(c)).count();
    let mut end = chars.len();
    while end > lead && chars.get(end - 1).is_some_and(|c| TRAILING.contains(c)) {
        end -= 1;
    }
    let number = chars.get(lead..end).and_then(move_number);
    let start = lead + number.map_or(0, |(_, _, n)| n);
    let mut core_end = end;
    // A full stop ends a sentence; a move never ends in one. An ellipsis is kept whole.
    if !chars.get(start..end).is_some_and(is_ellipsis) {
        while core_end > start && chars.get(core_end - 1) == Some(&'.') {
            core_end -= 1;
        }
    }
    Parts { lead, number, core: start..core_end.max(start) }
}

/// Step 3: count a word as language evidence ([`LangTally`]).
pub(crate) fn tally_word(word: &[Cell], tally: &mut LangTally) {
    if word.len() > MAX_WORD_CHARS {
        return;
    }
    let chars: Vec<char> = word.iter().map(|c| c.ch).collect();
    let core = chars.get(parts(&chars).core).unwrap_or(&[]);
    match core.first() {
        Some(c) if figurine_piece(*c).is_some() => {
            if parse_move(core, Lang::En).is_some() {
                tally.add(|_| true);
            }
        }
        Some(c) if c.is_alphabetic() && c.is_uppercase() => tally.add(|lang| parse_move(core, lang).is_some_and(|m| is_piece_move(&m))),
        _ => {}
    }
}

/// What a token is.
#[derive(Clone, Debug, PartialEq)]
enum Kind {
    Text,
    Number {
        number: u16,
        black: bool,
    },
    /// `...` on its own: Black's move number, split from its number (`7. ... h6`).
    Ellipsis,
    Move(San),
    Nag(&'static str),
    Figurine(PieceKind),
}

impl Kind {
    fn is_notation(&self) -> bool {
        !matches!(self, Kind::Text | Kind::Figurine(_))
    }
}

/// A piece of a word, with its text and box.
#[derive(Clone, Debug)]
pub(crate) struct Token {
    kind: Kind,
    text: String,
    rect: Option<[f32; 4]>,
    /// A bare pawn move or a number with nothing after it: notation only next to other notation.
    weak: bool,
}

fn token(kind: Kind, cells: &[Cell]) -> Token {
    Token { kind, text: cells.iter().map(|c| c.ch).collect(), rect: rect_of(cells), weak: false }
}

/// Step 4a: cut one word into tokens, reading moves in `lang`.
pub(crate) fn tokenize(word: &[Cell], lang: Lang) -> Vec<Token> {
    if word.len() > MAX_WORD_CHARS {
        return vec![token(Kind::Text, word)];
    }
    let chars: Vec<char> = word.iter().map(|c| c.ch).collect();
    let p = parts(&chars);
    let cells = |r: Range<usize>| word.get(r).unwrap_or(&[]);
    let mut out = Vec::new();
    let push_text = |out: &mut Vec<Token>, r: Range<usize>| {
        if !r.is_empty() {
            out.push(token(Kind::Text, cells(r)));
        }
    };
    push_text(&mut out, 0..p.lead);
    let core = chars.get(p.core.clone()).unwrap_or(&[]);
    let core_cells = cells(p.core.clone());
    let mut body = Vec::new();
    if let Some((number, black, n)) = p.number {
        body.push(Token { weak: core.is_empty(), ..token(Kind::Number { number, black }, cells(p.lead..p.lead + n)) });
    }
    if core.is_empty() {
    } else if p.number.is_none() && is_ellipsis(core) {
        body.push(token(Kind::Ellipsis, core_cells));
    } else if let Some(m) = parse_move(core, lang) {
        let head = core_cells.get(..m.head).unwrap_or(&[]);
        let tail = core_cells.get(m.head..).unwrap_or(&[]);
        let pawn_push = matches!(m.san.body, SanBody::Move { piece: None, capture: false, .. });
        let checks: Vec<Cell> = m.marks.iter().filter(|(k, _)| matches!(k, Mark::Check(_))).filter_map(|(_, r)| tail.get(r.start)).copied().collect();
        let mut mv = token(Kind::Move(m.san), &[head, &checks].concat());
        mv.weak = pawn_push && p.number.is_none();
        body.push(mv);
        for (mark, r) in m.marks {
            if let Mark::Nag(sym) = mark {
                body.push(token(Kind::Nag(sym), tail.get(r).unwrap_or(&[])));
            }
        }
    } else if let Some(marks) = split_marks(core, true) {
        body.extend(marks.into_iter().map(|(mark, r)| match mark {
            Mark::Nag(sym) => token(Kind::Nag(sym), core_cells.get(r).unwrap_or(&[])),
            Mark::Check(_) => token(Kind::Text, core_cells.get(r).unwrap_or(&[])),
        }));
    } else {
        let figurines = core.iter().take_while(|c| figurine_piece(**c).is_some()).count();
        for (i, c) in core.iter().take(figurines).enumerate() {
            if let Some(kind) = figurine_piece(*c) {
                body.push(token(Kind::Figurine(kind), core_cells.get(i..i + 1).unwrap_or(&[])));
            }
        }
        if figurines < core.len() {
            body.push(token(Kind::Text, core_cells.get(figurines..).unwrap_or(&[])));
        }
    }
    // A number followed by something that is not notation was not a move number (`3.5`).
    if p.number.is_some() && body.iter().skip(1).any(|t| t.kind == Kind::Text) {
        body = vec![token(Kind::Text, cells(p.lead..p.core.end))];
    }
    out.extend(body);
    push_text(&mut out, p.core.end..chars.len());
    out
}

/// Step 4b: settle the weak tokens of a page's words (`lines` of words of tokens). A bare pawn
/// move or a lone number is notation when a neighbouring word is notation (or another weak
/// token); otherwise it is prose. Words run on across line ends, so a move number that ends a
/// line (`… 12.`) is settled by the move that starts the next.
pub(crate) fn settle(lines: &mut [Vec<Vec<Token>>]) {
    let mut words: Vec<&mut Vec<Token>> = lines.iter_mut().flatten().collect();
    // A word "is notation" when it holds a notation token and no word-like prose.
    let notation: Vec<bool> = words
        .iter()
        .map(|w| w.iter().any(|t| t.kind.is_notation()) && !w.iter().any(|t| t.kind == Kind::Text && t.text.chars().any(char::is_alphanumeric)))
        .collect();
    for (i, word) in words.iter_mut().enumerate() {
        let neighbour = i.checked_sub(1).and_then(|p| notation.get(p)).copied().unwrap_or(false) || notation.get(i + 1).copied().unwrap_or(false);
        for t in word.iter_mut() {
            if t.weak && !neighbour {
                t.kind = Kind::Text;
            }
            t.weak = false;
        }
    }
}

/// Whose move it is, carried from line to line.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Side {
    white_next: bool,
    /// Whether White made the last move, for annotations that stand alone.
    white_moved: bool,
}

impl Default for Side {
    fn default() -> Self {
        Side { white_next: true, white_moved: true }
    }
}

fn nag_code(sym: &str, white_moved: bool) -> Option<u16> {
    nag::nags_from_suffix(sym, white_moved).first().copied()
}

/// Step 4c: a line's settled tokens as spans. `None` for a line with nothing in it.
pub(crate) fn assemble(words: Vec<Vec<Token>>, side: &mut Side) -> Option<ChessLine> {
    let mut spans: Vec<ChessSpan> = Vec::new();
    let mut boxes: Vec<[f32; 4]> = Vec::new();
    for (w, word) in words.into_iter().enumerate() {
        for (k, t) in word.into_iter().enumerate() {
            let rect = t.rect.unwrap_or([0.0; 4]);
            boxes.push(rect);
            let gap = w > 0 && k == 0;
            let run = match t.kind {
                Kind::Number { number, black } => {
                    side.white_next = !black;
                    ChessRun::MoveNumber { number, black }
                }
                Kind::Ellipsis => match spans.last_mut() {
                    Some(ChessSpan { run: ChessRun::MoveNumber { black, .. }, rect: r }) => {
                        *black = true;
                        side.white_next = false;
                        *r = view(union([[r.x0, r.y0, r.x1, r.y1], rect]).unwrap_or(rect));
                        continue;
                    }
                    _ => ChessRun::Text(t.text),
                },
                Kind::Move(san) => {
                    side.white_moved = side.white_next;
                    side.white_next = !side.white_next;
                    ChessRun::Move { san, text: t.text }
                }
                Kind::Nag(sym) => match nag_code(sym, side.white_moved) {
                    Some(code) => ChessRun::Nag(code),
                    None => ChessRun::Text(t.text),
                },
                Kind::Figurine(kind) => ChessRun::Figurine(kind),
                Kind::Text => ChessRun::Text(t.text),
            };
            // Prose runs join, keeping single spaces at word gaps.
            if let ChessRun::Text(s) = &run
                && let Some(ChessSpan { run: ChessRun::Text(prev), rect: r }) = spans.last_mut()
            {
                if gap {
                    prev.push(' ');
                }
                prev.push_str(s);
                *r = view(union([[r.x0, r.y0, r.x1, r.y1], rect]).unwrap_or(rect));
                continue;
            }
            spans.push(ChessSpan { run, rect: view(rect) });
        }
    }
    let rect = view(union(boxes)?);
    Some(ChessLine { spans, rect })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_word_is_not_normalized() {
        // A line of aliases with no word gap: aligning it would be quadratic in its length.
        let word: Vec<Cell> = "+/=".repeat(MAX_WORD_CHARS).chars().map(|c| Cell::new(c, [0.0; 4])).collect();
        assert_eq!(normalize_symbols(&word), word);
        let short: Vec<Cell> = "+/=".chars().map(|c| Cell::new(c, [0.0; 4])).collect();
        assert_eq!(normalize_symbols(&short).len(), 1, "a short alias is still normalized");
    }
}
