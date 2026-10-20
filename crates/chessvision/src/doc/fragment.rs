//! Reading a selection of move text into fragment items: the game walker in fragment mode (PGN
//! toolbar spec §4.3.2, step T2a).
//!
//! A selection on the page (`18.Nd5! Qxe1 After 18…Qd8 19.Bxf6! …`) is read by the same walker
//! that reads whole books ([`super::games`]), so prose becomes comments, brackets variations, a
//! numbered move goes back to its place, and a bold chain is the main line. In **fragment mode**
//! the walker:
//!
//! - starts from the position it is given, as it is (`GameScore::new`), never renumbered from the
//!   moves or turned to the other side as a diagram's reading is;
//! - reads one game: a `1.` quoted in prose (`as after 1.e4`) does not end it, and neither do
//!   diagrams, puzzle labels or headings, which a selection does not carry; a move numbered before
//!   the start position is prose;
//! - applies every [`WalkRules`] rule: a bracket lost in the text is assumed when a numbered move
//!   fits only outside the variation (flagged [`ReviewFlag::ImpliedClose`]); a move without a
//!   number after prose words stays in the comment (unless bold); `;` and `a)` `b)` separate
//!   sibling variations; `1:0` and `(1-0)` end the game as `1-0` does; a move after a lone
//!   ellipsis (`…Rc7`) is Black's.
//!
//! The text of a selection comes from [`text_of_glyphs`]: in the document's language, never one
//! guessed from a few words, and with the ellipsis forms `20.…`, `20. …Qd8` and `…Qd8` read as
//! notation. A word cut by a hyphen at a line's end is joined when letters stand on both sides
//! (`posi-` `tion`), never after a square or a digit (`Nd7-` `b6`, `e4-` `e5`, `1-` `0`).
//!
//! The game read is written out as [`FragmentItem`]s in PGN order, the input of
//! `pdfcraft_chess::game::Game::graft`: a move (with the half-move of its printed number, its
//! weight and its box), its glyphs (by White's code), its comment, then its alternatives each
//! between [`FragmentItem::Open`] and [`FragmentItem::Close`] (a variation's comment-before
//! first), then the move after it; the game's opening comment comes first, as a comment-before.
//! A result ends the items, and whatever came after it in the selection follows it as a comment,
//! which the graft reports as left over. Nothing read is dropped: a move that could not be played
//! is in the comment where it stood (as the walker writes it) and in [`FragmentRead::stops`].
//! Comments lose leading punctuation (`, White obtains…`), comments of punctuation alone are
//! left out, and figurines in them become English letters (`♘` is `N`).

use pdfcraft_chess::fragment::{FragmentItem, MAX_FRAGMENT_ITEMS, ply_of};
use pdfcraft_chess::nag;
use pdfcraft_chess::san::{Lang, San, figurine_piece};
use pdfcraft_render::text::TextGlyph;

use super::build::inlines;
use super::games::{GameScore, Loc, MAX_VARIATION_DEPTH, Walker};
use super::{GameResult, Inline, MoveInline, MoveNumber, MoveStatus, ReviewFlag, UserRect, plain_text};
use crate::chess_text::{self, ChessLine, ChessPageText, ChessRun};
use crate::coords::PageGeometry;

/// Longest text kept after a result or past [`MAX_FRAGMENT_ITEMS`], in bytes (cut at a character
/// boundary). A selection is at most 64 KiB.
pub const MAX_REST_LEN: usize = 1 << 16;

/// How a selection is read.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FragmentOptions {
    /// The page's geometry: with it each move carries its box in PDF user space; without it, no
    /// box (the text's boxes are in view space, which a game does not keep).
    pub geometry: Option<PageGeometry>,
    /// The text knows its weight (a text layer): each move says whether it is bold. OCR text and
    /// pasted text do not, and their moves say nothing.
    pub weight: bool,
}

/// A printed move that was not played, as the walker judged it.
#[derive(Clone, Debug, PartialEq)]
pub struct FragmentStop {
    /// The move as printed, with its number.
    pub text: String,
    /// Why: illegal, ambiguous, out of sequence, or not reached after one of those.
    pub status: MoveStatus,
    /// Its box in user space, when the geometry was given.
    pub rect: Option<[f32; 4]>,
}

/// What a selection was read as.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FragmentRead {
    /// The items, in PGN order, at most [`MAX_FRAGMENT_ITEMS`].
    pub items: Vec<FragmentItem>,
    /// Moves played: the moves of the tree the items hold.
    pub played: usize,
    /// Printed moves that were not played, in reading order. Their text is also in the comment
    /// where they stood.
    pub stops: Vec<FragmentStop>,
    /// What the walker flagged: illegal or out-of-sequence moves, an assumed closing bracket.
    pub flags: Vec<ReviewFlag>,
    /// The result that ended the selection, when one did.
    pub result: Option<GameResult>,
    /// Text not read: after the result (also the comment after [`FragmentItem::Result`]), and the
    /// items past [`MAX_FRAGMENT_ITEMS`]. At most [`MAX_REST_LEN`] bytes.
    pub rest: String,
    /// The start position was legal. When it was not, nothing was read and the whole selection
    /// is in [`FragmentRead::rest`].
    pub start_ok: bool,
}

/// A selection's glyphs (in content order, as the page's text layer gives them) as chess text, in
/// `lang`, the document's language (English when it is not known). The language is never guessed
/// from the selection; to read it in another one, call this again.
pub fn text_of_glyphs(glyphs: &[TextGlyph], lang: Option<Lang>) -> ChessPageText {
    chess_text::from_selection(&pdfcraft_render::text::layout(glyphs.to_vec()), lang.unwrap_or(Lang::En))
}

/// Read a selection's chess text from the position `start_fen` (see the [module](self)).
pub fn read(text: &ChessPageText, start_fen: &str, opts: &FragmentOptions) -> FragmentRead {
    let lines = join_hyphens(&text.lines);
    let refs: Vec<&ChessLine> = lines.iter().collect();
    read_inlines(&inlines(&refs, opts.geometry.as_ref()), start_fen, opts.weight)
}

/// [`read`] for text already cut into inlines (a document's paragraphs, say): `weight` as in
/// [`FragmentOptions::weight`].
pub fn read_inlines(inlines: &[Inline], start_fen: &str, weight: bool) -> FragmentRead {
    let Some(mut walker) = Walker::fragment(start_fen) else {
        return FragmentRead { rest: cut(&plain_text(inlines)).to_owned(), ..FragmentRead::default() };
    };
    let attached = attach_numbers(inlines);
    let inlines = attached.as_slice();
    let mut moves: Vec<(usize, super::games::MoveOutcome)> = Vec::new();
    let mut rest = String::new();
    for (k, inline) in inlines.iter().enumerate() {
        match inline {
            Inline::Text { text, .. } | Inline::Link { text, .. } => walker.text(text),
            Inline::Nag { code, .. } => walker.nag(*code),
            Inline::Move(m) => match San::read(&m.san, Lang::En) {
                Ok(san) => {
                    let out = walker.play(m.number, &san, &m.text, m.bold, Loc { page: 0, rect: m.source_rect });
                    moves.push((k, out));
                }
                // Not a move after all: its text is prose.
                Err(_) => walker.text(&inline.text()),
            },
        }
        if !walker.is_open() {
            rest = walker.take_rest().unwrap_or_default();
            let after = plain_text(inlines.get(k.saturating_add(1)..).unwrap_or_default());
            if !after.is_empty() {
                if !rest.trim().is_empty() {
                    rest.push(' ');
                }
                rest.push_str(&after);
            }
            break;
        }
    }
    let done = walker.finish();
    let Some(game) = done.games.first() else { return FragmentRead::default() };
    let score = &game.score;
    // Each node's printed move: the first inline played as it.
    let mut source: Vec<Option<&MoveInline>> = vec![None; score.nodes.len()];
    let mut stops = Vec::new();
    for (k, out) in &moves {
        let Some(Inline::Move(m)) = inlines.get(*k) else { continue };
        match out.node.and_then(|r| done.remap(r)) {
            Some(r) if out.status == MoveStatus::Valid => {
                if let Some(slot) = source.get_mut(r.node)
                    && slot.is_none()
                {
                    *slot = Some(m);
                }
            }
            _ if matches!(out.status, MoveStatus::Illegal | MoveStatus::Ambiguous | MoveStatus::OutOfSequence | MoveStatus::NotReached) => {
                stops.push(FragmentStop { text: Inline::Move(m.clone()).text(), status: out.status, rect: m.source_rect.map(corners) });
            }
            _ => {}
        }
    }
    let mut items = Vec::new();
    write_tree(score, &|n| source.get(n).copied().flatten(), weight, &mut items);
    // Only a result read in the text sets one.
    let result = (score.result != GameResult::Unknown).then_some(score.result);
    if result.is_some() {
        items.push(FragmentItem::Result);
        if let Some(text) = clean_comment(&rest) {
            items.push(FragmentItem::Comment { text, before: false });
        }
    }
    if items.len() > MAX_FRAGMENT_ITEMS {
        let over = items.split_off(MAX_FRAGMENT_ITEMS);
        let text = over.iter().map(FragmentItem::text).filter(|t| !t.is_empty()).collect::<Vec<_>>().join(" ");
        if !rest.is_empty() {
            rest.push(' ');
        }
        rest.push_str(&text);
    }
    FragmentRead {
        items,
        played: score.nodes.len(),
        stops,
        flags: game.flags.iter().map(|f| f.flag.clone()).collect(),
        result,
        rest: cut(&rest).to_owned(),
        start_ok: true,
    }
}

/// The inlines with a move number that ends a text inline given to the move without one right
/// after it (`… 30.` | `♖xh7`): a text cut into paragraphs between them, as a layout can cut a
/// number at a line's end from its move.
fn attach_numbers(inlines: &[Inline]) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::with_capacity(inlines.len());
    for inline in inlines {
        if let Inline::Move(m) = inline
            && m.number.is_none()
            && let Some(Inline::Text { text, .. }) = out.last_mut()
            && let Some((before, number)) = trailing_number(text)
        {
            let keep = before.trim_end().to_owned();
            if keep.is_empty() {
                out.pop();
            } else {
                *text = keep;
            }
            out.push(Inline::Move(MoveInline { number: Some(number), ..m.clone() }));
            continue;
        }
        out.push(inline.clone());
    }
    out
}

/// A move number that ends `text` as a word of its own (`30.`, `40...`, `40…`): the text before it
/// and the number.
fn trailing_number(text: &str) -> Option<(&str, MoveNumber)> {
    let trimmed = text.trim_end();
    let word = trimmed.rsplit(char::is_whitespace).next()?;
    let digits = word.chars().take_while(char::is_ascii_digit).count();
    // Digits are ASCII, so `digits` is a char boundary. At most three, as `chess_text` reads them.
    if digits == 0 || digits > 3 {
        return None;
    }
    let black = match word.get(digits..)? {
        "." => false,
        "..." | "\u{2026}" | ".\u{2026}" => true,
        _ => return None,
    };
    let number: u16 = word.get(..digits)?.parse().ok().filter(|n| *n > 0)?;
    let before = trimmed.get(..trimmed.len().saturating_sub(word.len()))?;
    Some((before, MoveNumber { number, black }))
}

/// The line from node `head` of `score`, with the variations inside it (not `head`'s own
/// alternatives), as items: moves by their canonical English SAN and half-move, glyphs, comments.
/// Nodes the tree reaches twice, or deeper than [`MAX_VARIATION_DEPTH`], are left out.
pub fn line_items(score: &GameScore, head: usize) -> Vec<FragmentItem> {
    let mut out = Vec::new();
    let mut seen = vec![false; score.nodes.len()];
    write_line(score, head, &[], 0, &mut seen, &|_| None, false, &mut out);
    out
}

/// The whole tree of `score` as items, its opening comment first.
fn write_tree<'a>(score: &GameScore, source: &dyn Fn(usize) -> Option<&'a MoveInline>, weight: bool, out: &mut Vec<FragmentItem>) {
    if let Some(text) = clean_comment(&score.comment) {
        out.push(FragmentItem::Comment { text, before: true });
    }
    let mut seen = vec![false; score.nodes.len()];
    if let Some((&first, alternatives)) = score.roots.split_first() {
        write_line(score, first, alternatives, 0, &mut seen, source, weight, out);
    }
}

/// One line from `start`, each move followed by its alternatives (`alternatives` for `start`).
#[allow(clippy::too_many_arguments)]
fn write_line<'a>(
    score: &GameScore,
    start: usize,
    alternatives: &[usize],
    depth: usize,
    seen: &mut [bool],
    source: &dyn Fn(usize) -> Option<&'a MoveInline>,
    weight: bool,
    out: &mut Vec<FragmentItem>,
) {
    let (mut current, mut alternatives) = (start, alternatives);
    loop {
        match seen.get_mut(current) {
            Some(s) if !*s => *s = true,
            _ => return,
        }
        let Some(node) = score.nodes.get(current) else { return };
        if depth > 0
            && current == start
            && let Some(text) = clean_comment(&node.comment_before)
        {
            out.push(FragmentItem::Comment { text, before: true });
        }
        let printed = source(current);
        let Ok(san) = San::read(&node.san, Lang::En) else { return };
        out.push(FragmentItem::Move {
            ply: match printed {
                Some(m) => m.number.map(|n| ply_of(u32::from(n.number), n.black)),
                None => Some(node.ply),
            },
            san,
            text: printed.map_or_else(|| node.san.clone(), |m| Inline::Move(m.clone()).text()),
            bold: printed.filter(|_| weight).map(|m| m.bold),
            rect: printed.and_then(|m| m.source_rect).map(corners),
        });
        // A glyph is written by White's code; the graft applies the side's.
        out.extend(node.nags.iter().map(|&code| FragmentItem::Nag(nag::by_code(code).map_or(code, |n| n.code))));
        if let Some(text) = clean_comment(&node.comment) {
            out.push(FragmentItem::Comment { text, before: false });
        }
        if depth < MAX_VARIATION_DEPTH {
            for &alt in alternatives {
                out.push(FragmentItem::Open);
                write_line(score, alt, &[], depth.saturating_add(1), seen, source, weight, out);
                out.push(FragmentItem::Close);
            }
        }
        let Some((&next, rest)) = node.children.split_first() else { return };
        (current, alternatives) = (next, rest);
    }
}

fn corners(r: UserRect) -> [f32; 4] {
    [r.x0, r.y0, r.x1, r.y1]
}

/// `text` cut to [`MAX_REST_LEN`] bytes at a character boundary.
fn cut(text: &str) -> &str {
    if text.len() <= MAX_REST_LEN {
        return text;
    }
    let mut end = MAX_REST_LEN;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or_default()
}

/// A comment as a fragment carries it: without leading punctuation, figurines as English letters;
/// `None` when nothing but punctuation is left.
fn clean_comment(text: &str) -> Option<String> {
    let glue = |c: char| matches!(c, ',' | ';' | ':' | '.') || c.is_whitespace();
    let t = text.trim_start_matches(glue).trim_end();
    if t.chars().all(glue) {
        return None;
    }
    Some(t.chars().map(|c| figurine_piece(c).map_or(c, |k| k.letter())).collect())
}

/// The lines with words cut by a hyphen at a line's end joined: `posi-` and `tion` become
/// `position` on the first line, when letters stand on both sides of the cut. After one letter
/// alone the hyphen stays (`e-` `file` is `e-file`). After a square, a digit or a sign nothing is
/// joined (`Nd7-` `b6`, `e4-` `e5`, `0-` `1`, `+-`).
pub(crate) fn join_hyphens(lines: &[ChessLine]) -> Vec<ChessLine> {
    let mut out: Vec<ChessLine> = lines.to_vec();
    for i in 1..out.len() {
        let Some((head, tail)) = out.split_at_mut_checked(i) else { break };
        let (Some(prev), Some(next)) = (head.last_mut(), tail.first_mut()) else { continue };
        let Some(ChessRun::Text(end)) = prev.spans.last_mut().map(|s| &mut s.run) else { continue };
        let Some(ChessRun::Text(start)) = next.spans.first().map(|s| &s.run) else { continue };
        let Some(stem) = end.strip_suffix('-') else { continue };
        let word = stem.rsplit(char::is_whitespace).next().unwrap_or_default();
        let letters = word.chars().rev().take_while(|c| c.is_alphabetic()).count();
        // The letters are a word's end, not a square's (`Nd7-`) or a sign's.
        let alone = word.chars().rev().nth(letters).is_none_or(|c| !c.is_alphanumeric());
        let starts = start.chars().next().is_some_and(|c| c.is_alphabetic() && c.is_lowercase());
        if letters == 0 || !alone || !starts {
            continue;
        }
        let first_word: String = start.chars().take_while(|c| !c.is_whitespace()).collect();
        let joined = if letters == 1 { format!("{end}{first_word}") } else { format!("{stem}{first_word}") };
        *end = joined;
        let remainder = start.get(first_word.len()..).unwrap_or_default().trim_start().to_owned();
        if remainder.is_empty() {
            next.spans.remove(0);
        } else if let Some(span) = next.spans.first_mut() {
            span.run = ChessRun::Text(remainder);
        }
    }
    out
}
