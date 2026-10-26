//! One page's blocks from its text lines, diagrams and hints.
//!
//! 0. **Columns.** Lines are read column by column ([`column_order`]): on a page whose lines
//!    leave a gutter free (a two-column book), each band between full-width lines is read down its
//!    left column, then down its right one; each side may be cut again by a gutter of its own, up
//!    to [`MAX_COLUMNS`] columns ([`columns`]). The coordinates printed around a diagram (`a b c … h`,
//!    `8 … 1`) are not text and are left out ([`is_coordinates`]).
//! 1. **Diagrams among the lines.** Lines keep their reading order and diagrams theirs; a diagram
//!    goes before the first line below it in its column, or in a column to its right, or wholly
//!    below it and not in another column ([`diagram_before`]). Diagrams are never reordered among themselves, so the n-th diagram
//!    block is diagram n of the reading order. Diagrams no line comes after go at the end.
//! 2. **Blocks.** A line belongs to the first hint whose box holds its centre; consecutive lines of
//!    one hint form one heading or paragraph. Lines without a hint form paragraphs, a new one at a
//!    gap taller than [`PARAGRAPH_GAP_SHARE`] of a line or a change of column. A diagram ends the
//!    block before it.
//! 3. **Inlines.** Runs become inlines in order; a move number is kept with the move after it.
//! 4. **Review.** Text reviews go to the inline they overlap most; what overlaps nothing, unknown
//!    chess fonts and a poor text layer go to the page.
//! 5. **Games.** The blocks go through the [`Walker`] in order.

use pdfcraft_chess::san::{FigurineSet, Lang, San, figurine};

use super::games::{Key, Loc, Name, Seed, Walker};
use super::{Block, ChessPage, DiagramBlock, HintKind, Inline, MoveInline, MoveNumber, MoveStatus, PageInput, ReviewFlag, UserRect};
use crate::chess_text::{ChessLine, ChessRun, PoorReason, TextQuality};
use crate::coords::{PageGeometry, ViewRect};

/// A gap between two lines taller than this share of a line starts a new paragraph (when the
/// layout gave no hint). Leading in books is 0.2–0.5 of a line; a blank line is 1.
pub const PARAGRAPH_GAP_SHARE: f32 = 0.8;
/// How far outside a hint's box a line's centre may be and still belong to it, in points.
pub const HINT_TOLERANCE: f32 = 1.0;

/// A gutter must leave at least this many lines on each side.
pub const MIN_COLUMN_LINES: usize = 3;
/// At most this share of the page's lines may cross a gutter (headings, captions, wide lines).
pub const MAX_CROSSING_SHARE: f32 = 0.15;
/// A column's lines must be this wide on average, as a share of the page (narrower blocks side
/// by side are a table or a game score, not columns).
pub const MIN_COLUMN_SHARE: f32 = 0.2;
/// Or at least this many of them must be that wide each: a column that holds a game score's
/// table (`2 f4 ef`, cut at its wide gaps) beside its prose is still a column.
pub const MIN_WIDE_COLUMN_LINES: usize = 5;

/// Longest caption that may give a puzzle number without a `№` (`185. Teichmann – Lasker`).
pub const MAX_BARE_CAPTION_CHARS: usize = 80;
/// Largest puzzle number read.
pub const MAX_PUZZLE: u32 = 99_999;

/// The words that number a workbook's exercises, before the number: `Exercício 12` under the
/// diagram, `Ex. 6–12` (chapter 6, exercise 12) over its solution.
const EXERCISE_WORDS: [&str; 9] = ["Exercício", "Exercicio", "Exercise", "Exercice", "Ejercicio", "Esercizio", "Aufgabe", "Übung", "Ex."];

/// The puzzle number a text starts with: `№185.`, `N!!185.` (how text layers extract `№`),
/// `N9191.` (how OCR reads it), `No. 12`, `Nr. 12`, `#12`, an exercise (`Exercício 12`), followed
/// by a full stop, a bracket, a colon or a space. A chapter's number before it (`Ex. 6–12`, `Ex.
/// 24-3`) is not part of it: the number is the exercise's within its chapter ([`solution_name`]
/// keeps the chapter). With `bare`, also a caption that is the number alone (`1675`) or the
/// number, a full stop and a name (`185. Teichmann`).
pub(crate) fn puzzle_label(text: &str, bare: bool) -> Option<u32> {
    label(text, bare).map(|l| l.number)
}

/// The puzzle a solution names ([`puzzle_label`], never bare). An exercise's label must stand
/// alone, with no word after its number (`Ex. 6–12`, `Exercício 3 ★△`): `Exercise 5 was hard` and
/// `Ex. 5: examples from the last chapter` are prose.
pub(crate) fn solution_label(text: &str) -> Option<u32> {
    solution_name(text).map(|(n, _)| n)
}

/// The puzzle a solution names ([`solution_label`]) and the chapter its label gives before it
/// (`Ex. 6–12`: exercise 12 of chapter 6).
pub(crate) fn solution_name(text: &str) -> Option<(u32, Option<u32>)> {
    label(text, false).filter(|l| !l.exercise || !l.after.chars().any(char::is_alphabetic)).map(|l| (l.number, l.chapter))
}

/// A puzzle label: its number, the chapter before it, whether an exercise word gave it, and the
/// text after the number.
struct Label<'a> {
    number: u32,
    chapter: Option<u32>,
    exercise: bool,
    after: &'a str,
}

fn label(text: &str, bare: bool) -> Option<Label<'_>> {
    let t = text.trim_start();
    let numbered = ["№", "N!!", "N°", "Nº", "No.", "Nr.", "N9", "No", "Nr", "#"].iter().map(|p| (p, false));
    let prefixed = numbered
        .chain(EXERCISE_WORDS.iter().map(|p| (p, true)))
        .find_map(|(p, exercise)| t.strip_prefix(p).map(|rest| (rest, exercise)))
        // `Exercises 1.165-1.168` is a section's title, not an exercise.
        .filter(|(rest, _)| rest.starts_with(|c: char| c.is_whitespace() || c.is_ascii_digit()))
        .map(|(rest, exercise)| (rest.trim_start(), exercise));
    let (rest, exercise) = match prefixed {
        Some(found) => found,
        None if bare && t.chars().count() <= MAX_BARE_CAPTION_CHARS => (t, false),
        None => return None,
    };
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 || digits > 5 {
        return None;
    }
    // `6–12`: the exercise after the chapter. Digits and the dashes are one char each in `rest`
    // as `get` slices them, at char boundaries.
    let chapter_dash = rest.get(digits..).and_then(|r| r.strip_prefix(['–', '-']));
    let (rest, digits, chapter) = match chapter_dash {
        Some(after) if prefixed.is_some() => {
            let n = after.chars().take_while(char::is_ascii_digit).count();
            if n == 0 || n > 5 {
                return None;
            }
            let chapter = rest.get(..digits).and_then(|c| c.parse().ok()).filter(|c| *c > 0 && *c <= MAX_PUZZLE);
            (after, n, chapter)
        }
        _ => (rest, digits, None),
    };
    // Digits are ASCII, so `digits` is a char boundary.
    let after = rest.get(digits..)?;
    let ends = match (prefixed.is_some(), after.chars().next()) {
        // A bare number stands alone (`1675`) or ends in a full stop, a space and a name
        // (`185. Teichmann`): `1996 was` is prose, `6.` a mark under a diagram and `2.2
        // Combinations` a section.
        (false, _) => {
            let named = after.strip_prefix('.').is_some_and(|r| r.starts_with(char::is_whitespace) && r.chars().any(char::is_alphabetic));
            after.trim().is_empty() || named
        }
        (true, None) => true,
        (true, Some(c)) => matches!(c, '.' | ')' | ':') || c.is_whitespace(),
    };
    if !ends {
        return None;
    }
    let number = rest.get(..digits)?.parse().ok().filter(|n| *n > 0 && *n <= MAX_PUZZLE)?;
    Some(Label { number, chapter, exercise, after })
}

/// The words that name a chapter, before its number: `Chapter 6`, `CAPÍTULO 7`, `Solutions to
/// Chapter IV`.
const CHAPTER_WORDS: [&str; 8] = ["chapter", "capítulo", "capitulo", "chapitre", "capitolo", "kapitel", "hoofdstuk", "rozdział"];
/// Longest heading whose words are kept as a chapter's title ([`heading_keys`]).
pub const MAX_TITLE_CHARS: usize = 80;
/// Most words of a heading looked at for a chapter's number.
const MAX_HEADING_WORDS: usize = 24;

/// A Roman numeral from I to L (`IV`, `xii`), written the usual way.
fn roman(word: &str) -> Option<u32> {
    let up = word.to_ascii_uppercase();
    if up.is_empty() || up.len() > 7 || !up.chars().all(|c| matches!(c, 'I' | 'V' | 'X' | 'L')) {
        return None;
    }
    (1..=50u32).find(|n| {
        let (tens, ones) = (n / 10, n % 10);
        let tens = ["", "X", "XX", "XXX", "XL", "L"].get(tens as usize).copied().unwrap_or("");
        let ones = ["", "I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX"].get(ones as usize).copied().unwrap_or("");
        format!("{tens}{ones}") == up
    })
}

/// The chapter a heading numbers: a chapter word ([`CHAPTER_WORDS`]) and the number after it, in
/// figures (up to 999) or Roman numerals.
pub(crate) fn chapter_number(text: &str) -> Option<u32> {
    let lower = text.to_lowercase();
    let words: Vec<&str> = lower.split_whitespace().take(MAX_HEADING_WORDS).collect();
    words.windows(2).find_map(|pair| {
        let [word, number] = pair else { return None };
        if !CHAPTER_WORDS.contains(&word.trim_end_matches(['.', ':'])) {
            return None;
        }
        let number = number.trim_end_matches(['.', ':', ',', ')', '–', '-']);
        let figures = (!number.is_empty() && number.len() <= 3 && number.chars().all(|c| c.is_ascii_digit())).then(|| number.parse().ok()).flatten();
        figures.filter(|n| *n > 0).or_else(|| roman(number))
    })
}

/// What a heading says about its chapter, for the solutions that name its exercises: its
/// chapter's number ([`chapter_number`]) and its title (its words, when it is at most
/// [`MAX_TITLE_CHARS`] long). A solution's label (`Ex. 6–12`) says nothing.
pub(crate) fn heading_keys(text: &str) -> Vec<Key> {
    if solution_label(text).is_some() {
        return Vec::new();
    }
    let mut keys: Vec<Key> = chapter_number(text).map(Key::Chapter).into_iter().collect();
    if text.chars().count() <= MAX_TITLE_CHARS {
        let title = text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect::<Vec<_>>().join(" ");
        if !title.is_empty() {
            keys.push(Key::Title(title));
        }
    }
    keys
}

/// A caption line's text, or `None` for a line with moves in it.
fn caption_text(line: &ChessLine) -> Option<String> {
    let mut out = String::new();
    for span in &line.spans {
        match &span.run {
            ChessRun::Text(t) => out.push_str(t),
            // Read as a move number (`1675` before a move-like word); the caption is the number.
            ChessRun::MoveNumber { number, .. } => out.push_str(&number.to_string()),
            ChessRun::Nag(code) => out.push_str(pdfcraft_chess::nag::by_code(*code).and_then(|n| n.glyph).unwrap_or("")),
            ChessRun::Figurine(_) => {}
            _ => return None,
        }
        out.push(' ');
    }
    Some(out)
}

/// The puzzle number of each diagram (view space): the caption just above it, or else just below
/// it, in its column, that starts with one. A caption goes to the one diagram it is nearest (the
/// line between two diagrams stacked in a column is the lower one's caption, or the upper one's),
/// and a diagram takes its nearest caption.
///
/// A bare number on one of the lines `folios` lists is the page's number, not a caption.
///
/// Also the page's trailing label: one with its word or `№` (not a bare number, which may be the
/// page's number), standing alone ([`solution_label`]), that went to no diagram, with no diagram below it and no line after it but the
/// page's number. It is the caption of the next page's first diagram (Construa Seu Xadrez sets
/// `Exercício 7` at the foot of the page and its diagram at the top of the next).
fn caption_numbers(lines: &[&ChessLine], diagrams: &[Option<ViewRect>], folios: &[usize]) -> (Vec<Option<u32>>, Option<u32>) {
    // (bare, gap, number, line) per diagram: a caption with its `№` wins over a bare number.
    let mut best: Vec<Option<(bool, f32, u32, usize)>> = vec![None; diagrams.len()];
    // The labels with a word or `№`: (line, number).
    let mut labelled: Vec<(usize, u32)> = Vec::new();
    for (li, l) in lines.iter().enumerate() {
        let Some(text) = caption_text(l) else { continue };
        let (n, bare) = match (puzzle_label(&text, false), puzzle_label(&text, true)) {
            (Some(n), _) => (n, false),
            (None, Some(n)) => (n, true),
            (None, None) => continue,
        };
        if bare && folios.contains(&li) {
            continue;
        }
        // Standing alone, as a solution's label must: `Exercício 2 mostra o mate` is prose.
        if !bare && solution_label(&text).is_some() {
            labelled.push((li, n));
        }
        let gap_to = |d: &ViewRect| {
            if !(l.rect.x0 < d.x1 && d.x0 < l.rect.x1) {
                return None;
            }
            let gap = if l.rect.y1 <= d.y0 + 2.0 {
                d.y0 - l.rect.y1
            } else if l.rect.y0 >= d.y1 - 2.0 {
                // Below counts a little farther: a caption above is the usual place.
                l.rect.y0 - d.y1 + 1.0
            } else {
                return None;
            };
            (gap <= d.height() * 0.35).then_some(gap)
        };
        let nearest = diagrams.iter().enumerate().filter_map(|(k, d)| Some((k, gap_to(d.as_ref()?)?))).min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((k, gap)) = nearest
            && let Some(slot) = best.get_mut(k)
            && slot.is_none_or(|(b, g, _, _)| (bare, gap) < (b, g))
        {
            *slot = Some((bare, gap, n, li));
        }
    }
    let used: Vec<usize> = best.iter().flatten().map(|(_, _, _, li)| *li).collect();
    let trailing = labelled
        .iter()
        .filter(|(li, _)| !used.contains(li))
        .filter_map(|&(li, n)| lines.get(li).map(|l| (l.rect.y0, n)))
        .filter(|(y, _)| diagrams.iter().flatten().all(|d| d.y0 < *y))
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .filter(|(y, _)| lines.iter().filter(|o| o.rect.y0 > *y + 1.0).all(|o| is_page_number(o)))
        .map(|(_, n)| n);
    (best.into_iter().map(|b| b.map(|(_, _, n, _)| n)).collect(), trailing)
}

/// Whether `line` is only a number: a page's number under the text.
fn is_page_number(line: &ChessLine) -> bool {
    caption_text(line).is_some_and(|t| !t.trim().is_empty() && t.trim().chars().all(|c| c.is_ascii_digit()))
}

/// Most page-number offsets a document remembers ([`folio_offsets`]).
pub const MAX_FOLIO_OFFSETS: usize = 4;

/// The page's lines that may be its printed number (its folio): a number alone at the head or the
/// foot of the page (no line above it, or none below it). Each with the offset of its number
/// from the page's index, the same on every page of a book that numbers its pages there.
fn folio_offsets(lines: &[&ChessLine], index: u32) -> Vec<(usize, i64)> {
    let mut out = Vec::new();
    for (li, l) in lines.iter().enumerate() {
        let head = lines.iter().all(|o| o.rect.y1 > l.rect.y0 || std::ptr::eq(*o, *l));
        let foot = lines.iter().all(|o| o.rect.y0 < l.rect.y1 || std::ptr::eq(*o, *l));
        if !(head || foot) || !is_page_number(l) {
            continue;
        }
        let number = caption_text(l).and_then(|t| t.trim().parse::<u32>().ok());
        if let Some(n) = number {
            out.push((li, i64::from(n) - i64::from(index)));
        }
    }
    out
}

/// Remember the page-number `offsets` of a page ([`folio_offsets`]), newest last.
pub(crate) fn remember_folios(known: &mut Vec<i64>, offsets: &[i64]) {
    for &o in offsets {
        known.retain(|k| *k != o);
        if known.len() >= MAX_FOLIO_OFFSETS {
            known.remove(0);
        }
        known.push(o);
    }
}

/// The line's text, when it holds only text and move numbers.
fn plain_text(line: &ChessLine) -> Option<String> {
    let mut out = String::new();
    for span in &line.spans {
        match &span.run {
            ChessRun::Text(t) => out.push_str(t),
            ChessRun::MoveNumber { number, .. } => out.push_str(&number.to_string()),
            _ => return None,
        }
        out.push(' ');
    }
    Some(out)
}

/// Whether `line` is a diagram's printed coordinates: only single files (`a`–`h`) or ranks
/// (`1`–`8`), beside or under one of the diagrams (within a quarter of its side).
pub(crate) fn is_coordinates(line: &ChessLine, diagrams: &[Option<ViewRect>]) -> bool {
    let Some(text) = plain_text(line) else { return false };
    let mut tokens = text.split_whitespace().peekable();
    if tokens.peek().is_none() {
        return false;
    }
    let coordinate = |t: &str| t.chars().count() == 1 && t.chars().all(|c| ('a'..='h').contains(&c) || ('1'..='8').contains(&c));
    if !tokens.all(coordinate) {
        return false;
    }
    let r = line.rect;
    diagrams.iter().flatten().any(|d| {
        let pad = d.width().max(d.height()) * 0.25;
        r.x0 < d.x1 + pad && d.x0 - pad < r.x1 && r.y0 < d.y1 + pad && d.y0 - pad < r.y1
    })
}

/// Most columns a page is read in: a page that seems to have more is read as this many at most.
pub const MAX_COLUMNS: usize = 4;

/// The x of a gutter that splits `lines` into two columns, if there is one: the middle of the
/// widest run of positions (between 30% and 70% of the way from `lo` to `hi`) that the fewest
/// lines cross, with enough lines on both sides, wide enough on average or with enough wide ones
/// among them ([`MIN_WIDE_COLUMN_LINES`]). A column must be as wide as on the whole page
/// ([`MIN_COLUMN_SHARE`] of `page_width`) wherever it is.
fn gutter_between(lines: &[&ChessLine], page_width: f32, lo: f32, hi: f32) -> Option<f32> {
    let finite = [page_width, lo, hi].iter().all(|v| v.is_finite());
    if !finite || page_width <= 0.0 || hi <= lo || lines.len() < 2 * MIN_COLUMN_LINES {
        return None;
    }
    let max_crossing = ((lines.len() as f32) * MAX_CROSSING_SHARE).floor() as usize;
    // Candidate positions, one point apart; bounded by the page width (at most a few thousand).
    let (lo, hi) = (lo + (hi - lo) * 0.3, lo + (hi - lo) * 0.7);
    let steps = ((hi - lo).max(0.0) as usize).min(20_000);
    let mut best: Option<(usize, usize, usize)> = None; // (crossings, run length, run start)
    let mut run: Option<(usize, usize)> = None; // (start, crossings)
    let score = |x: f32| {
        let crossing = lines.iter().filter(|l| l.rect.x0 < x && x < l.rect.x1).count();
        let left: Vec<&&ChessLine> = lines.iter().filter(|l| l.rect.x1 <= x).collect();
        let right: Vec<&&ChessLine> = lines.iter().filter(|l| l.rect.x0 >= x).collect();
        let wide = |side: &[&&ChessLine]| {
            let min = MIN_COLUMN_SHARE * page_width;
            side.iter().map(|l| l.rect.width()).sum::<f32>() / side.len().max(1) as f32 >= min
                || side.iter().filter(|l| l.rect.width() >= min).count() >= MIN_WIDE_COLUMN_LINES
        };
        let ok = crossing <= max_crossing && left.len() >= MIN_COLUMN_LINES && right.len() >= MIN_COLUMN_LINES && wide(&left) && wide(&right);
        ok.then_some(crossing)
    };
    for k in 0..=steps {
        let x = lo + k as f32;
        match (score(x), run) {
            (Some(c), Some((start, rc))) if c == rc => run = Some((start, rc)),
            (now, prev) => {
                if let Some((start, rc)) = prev {
                    let len = k - start;
                    if best.is_none_or(|(bc, bl, _)| rc < bc || (rc == bc && len > bl)) {
                        best = Some((rc, len, start));
                    }
                }
                run = now.map(|c| (k, c));
            }
        }
    }
    if let Some((start, rc)) = run {
        let len = steps + 1 - start;
        if best.is_none_or(|(bc, bl, _)| rc < bc || (rc == bc && len > bl)) {
            best = Some((rc, len, start));
        }
    }
    best.map(|(_, len, start)| lo + start as f32 + len as f32 / 2.0)
}

/// How a page's lines fall into columns: one column, or two sides of a gutter, each of which may
/// be cut again (a page of three columns is a column and a pair of columns beside it).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Columns {
    One,
    Split { x: f32, left: Box<Columns>, right: Box<Columns> },
}

impl Columns {
    /// The gutters, left to right.
    pub(crate) fn gutters(&self) -> Vec<f32> {
        let mut out = Vec::new();
        self.push_gutters(&mut out);
        out
    }

    fn push_gutters(&self, out: &mut Vec<f32>) {
        if let Columns::Split { x, left, right } = self {
            left.push_gutters(out);
            out.push(*x);
            right.push_gutters(out);
        }
    }
}

/// The page's columns: a gutter across the whole page ([`gutter_between`]), then a gutter again among the lines on
/// each side of it (found the same way, between that side's edges), and so on, up to
/// [`MAX_COLUMNS`] columns. A page with no gutter is one column; one whose sides cannot be cut
/// again is read in two, as before.
pub(crate) fn columns(lines: &[&ChessLine], page_width: f32) -> Columns {
    let mut cuts = MAX_COLUMNS.saturating_sub(1);
    split_columns(lines, page_width, 0.0, page_width, &mut cuts)
}

/// [`columns`] among `lines`, between `lo` and `hi`, with at most `cuts` gutters left to find.
/// Each call that recurses uses one of them up, so the depth is at most [`MAX_COLUMNS`].
fn split_columns(lines: &[&ChessLine], page_width: f32, lo: f32, hi: f32, cuts: &mut usize) -> Columns {
    if *cuts == 0 {
        return Columns::One;
    }
    let Some(x) = gutter_between(lines, page_width, lo, hi) else { return Columns::One };
    *cuts -= 1;
    let left: Vec<&ChessLine> = lines.iter().copied().filter(|l| l.rect.x1 <= x).collect();
    let right: Vec<&ChessLine> = lines.iter().copied().filter(|l| l.rect.x0 >= x).collect();
    let left = split_columns(&left, page_width, lo, x, cuts);
    let right = split_columns(&right, page_width, x, hi, cuts);
    Columns::Split { x, left: Box::new(left), right: Box::new(right) }
}

/// The lines in reading order, column by column: lines that cross a gutter cut the columns on
/// both sides of it into bands, and each band is read down its left side, then down its right
/// one, each side read the same way by its own gutter. On one column, the lines keep their order.
pub(crate) fn column_order<'a>(lines: &[&'a ChessLine], columns: &Columns) -> Vec<&'a ChessLine> {
    let Columns::Split { .. } = columns else { return lines.to_vec() };
    let top = |l: &&ChessLine| l.rect.y0;
    let mut sorted: Vec<&ChessLine> = lines.to_vec();
    sorted.sort_by(|a, b| top(a).total_cmp(&top(b)).then(a.rect.x0.total_cmp(&b.rect.x0)));
    let mut out = Vec::with_capacity(lines.len());
    bands(&sorted, columns, &mut out);
    out
}

/// [`column_order`] of `sorted` (top down), by `columns`, onto `out`. Each level of `columns` is
/// one level of recursion, so the depth is at most [`MAX_COLUMNS`].
fn bands<'a>(sorted: &[&'a ChessLine], columns: &Columns, out: &mut Vec<&'a ChessLine>) {
    let Columns::Split { x, left: lc, right: rc } = columns else {
        out.extend_from_slice(sorted);
        return;
    };
    let mut left: Vec<&ChessLine> = Vec::new();
    let mut right: Vec<&ChessLine> = Vec::new();
    for &line in sorted {
        if line.rect.x1 <= *x {
            left.push(line);
        } else if line.rect.x0 >= *x {
            right.push(line);
        } else {
            // A line across the gutter: what is above it is read first.
            bands(&left, lc, out);
            bands(&right, rc, out);
            left.clear();
            right.clear();
            out.push(line);
        }
    }
    bands(&left, lc, out);
    bands(&right, rc, out);
}

enum Item<'a> {
    Line(&'a ChessLine),
    Diagram(usize),
}

/// How far above a board, as a share of its height, a line to its right may end and still be the
/// board's own text (its solution starting level with the label above it): Chess Tactics' first
/// solution line ends 14 pt above a 100 pt board, the previous solution's last 52 pt above.
pub const BESIDE_ABOVE_SHARE: f32 = 0.3;

/// Whether diagram `d` comes before line `l` (view space): `l` is below `d`'s middle under it or
/// beside it in its column between the `splits` (the gutters, left to right, or the page's middle
/// on a page without them), in a column to its right, or wholly below it in a column it shares
/// ([`sides`]). A diagram or line across a split is in the columns on both sides: a short line
/// under a centred diagram is read after it, however far left it starts, while a left-column line
/// below a right-column diagram is still read before it. On a page of `one_column` (no gutter,
/// and at least half of its lines across the middle: [`one_column`]), a line wholly below a
/// diagram is read after it on either side of the middle, unless it is under another board in its
/// own column (`under_a_board`: a row of boards, each with its caption). A page of short lines
/// with no gutter may be two columns too short to find one, and keeps the sides apart. A line right of a board is its text beside it (Chess Tactics: the board on the left, the
/// solution on the right) and comes after it, except a piece of a row a scan cut at a word gap:
/// on a page with a `gutter` (columns found, not the page's middle), on the board's side of it,
/// and with another piece of its row over the board's columns (`row_over_board`), the line is in
/// the board's column and is read by its height. On a page with no gutter, a line right of a board
/// that ends more than [`BESIDE_ABOVE_SHARE`] of the board's height above its top, with no other
/// board above it in its own column (`under_a_board`), is the text before it and is read first.
fn diagram_before(d: ViewRect, l: ViewRect, splits: &[f32], gutter: bool, one_column: bool, row_over_board: bool, under_a_board: bool) -> bool {
    let (sd, sl) = (sides(d, splits), sides(l, splits));
    let same_column = d.x0 < l.x1 && l.x0 < d.x1;
    let beside = gutter && row_over_board && l.x0 >= d.x1 && sd.0 == sd.1 && sd == sl;
    if same_column || beside {
        return (l.y0 + l.y1) * 0.5 >= (d.y0 + d.y1) * 0.5;
    }
    if l.x0 >= d.x1 {
        // On a page with no columns, read top down, a line right of the board that ends well above
        // it is the text before it (Chess Tactics: the end of the previous solution at the top of
        // the page), not the board's own solution, which starts level with its label. A line under
        // another board in its own column is that board's (a grid of boards, found column by
        // column): it waits for that board, which comes after this one.
        return gutter || under_a_board || l.y1 >= d.y0 - BESIDE_ABOVE_SHARE * d.height();
    }
    // `l` is to the left of `d`. On a page of one column, read top down, a line wholly below the
    // board comes after it on either side of the middle, unless it is under another board in its
    // own column (a row of boards: it is that board's caption).
    l.y0 >= d.y1 && ((sd.0 <= sl.1 && sl.0 <= sd.1) || (one_column && !under_a_board))
}

/// The columns `r` lies in, first and last (0-based, left to right), between the gutters
/// `splits` (sorted): a box across a gutter is in the columns on both sides of it.
fn sides(r: ViewRect, splits: &[f32]) -> (usize, usize) {
    let first = splits.iter().filter(|s| **s <= r.x0).count();
    let last = splits.iter().filter(|s| **s < r.x1).count();
    (first, last.max(first))
}

/// Whether a page with no gutter is one column: at least half of its `lines` cross its middle
/// `split` (two columns too short for a gutter have none that do).
fn one_column(lines: &[&ChessLine], split: f32) -> bool {
    let across = lines.iter().filter(|l| l.rect.x0 < split && split < l.rect.x1).count();
    !lines.is_empty() && across.saturating_mul(2) >= lines.len()
}

fn interleave<'a>(lines: &[&'a ChessLine], diagrams: &[Option<ViewRect>], splits: &[f32], gutter: bool) -> Vec<Item<'a>> {
    let one_column = !gutter && splits.first().is_some_and(|s| one_column(lines, *s));
    let mut items = Vec::with_capacity(lines.len().saturating_add(diagrams.len()));
    let mut next = 0usize;
    for &line in lines {
        // Another board above this line in its own column: the line is that board's text.
        let under_a_board =
            |d: ViewRect| diagrams.iter().flatten().any(|e| *e != d && e.x0 < line.rect.x1 && line.rect.x0 < e.x1 && e.y1 <= line.rect.y0);
        while let Some(d) = diagrams.get(next) {
            // Another line of this one's row over the board's columns: a row cut into pieces.
            let row_over = |d: ViewRect| {
                let mid = (line.rect.y0 + line.rect.y1) * 0.5;
                lines.iter().any(|o| !std::ptr::eq(*o, line) && o.rect.y0 <= mid && mid <= o.rect.y1 && o.rect.x0 < d.x1 && d.x0 < o.rect.x1)
            };
            if !d.is_some_and(|d| diagram_before(d, line.rect, splits, gutter, one_column, row_over(d), under_a_board(d))) {
                break;
            }
            items.push(Item::Diagram(next));
            next += 1;
        }
        items.push(Item::Line(line));
    }
    items.extend((next..diagrams.len()).map(Item::Diagram));
    items
}

/// Whether `cur` continues the paragraph whose last line is `prev`.
fn same_paragraph(prev: ViewRect, cur: ViewRect) -> bool {
    let same_column = prev.x0 < cur.x1 && cur.x0 < prev.x1;
    let height = prev.height().max(cur.height()).max(1.0);
    let gap = cur.y0 - prev.y1;
    same_column && gap <= PARAGRAPH_GAP_SHARE * height && gap > -height
}

/// The first hint whose box holds the line's centre.
fn hint_of(line: &ChessLine, hints: &[(HintKind, Option<ViewRect>)]) -> Option<usize> {
    let (cx, cy) = ((line.rect.x0 + line.rect.x1) * 0.5, (line.rect.y0 + line.rect.y1) * 0.5);
    hints.iter().position(|(_, r)| {
        r.is_some_and(|r| cx >= r.x0 - HINT_TOLERANCE && cx <= r.x1 + HINT_TOLERANCE && cy >= r.y0 - HINT_TOLERANCE && cy <= r.y1 + HINT_TOLERANCE)
    })
}

fn user(rect: ViewRect, geometry: &PageGeometry) -> Option<UserRect> {
    geometry.view_to_user(rect).and_then(UserRect::new)
}

fn union(a: Option<UserRect>, b: Option<UserRect>) -> Option<UserRect> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.union(b)),
        (a, b) => a.or(b),
    }
}

/// A word that is a web address: the address to open, and the word without trailing punctuation.
fn link_of(word: &str) -> Option<(String, &str)> {
    let w = word.trim_end_matches(['.', ',', ';', ':', ')', ']', '!', '?']);
    let lower = w.to_ascii_lowercase();
    if (lower.starts_with("http://") || lower.starts_with("https://")) && w.len() > "https://".len() {
        Some((w.to_owned(), w))
    } else if lower.starts_with("www.") && w.len() > "www.".len() + 2 {
        Some((format!("https://{w}"), w))
    } else {
        None
    }
}

/// Prose as text inlines, with its web addresses as links. All take the run's box: a run's words
/// have no boxes of their own.
fn push_text(out: &mut Vec<Inline>, text: &str, rect: Option<UserRect>) {
    let plain = |t: String| Inline::Text { text: t, source_rect: rect, review: Vec::new() };
    let mut before = String::new();
    for word in text.split(' ') {
        match link_of(word) {
            Some((url, shown)) => {
                if !before.is_empty() {
                    out.push(plain(std::mem::take(&mut before)));
                }
                out.push(Inline::Link { url, text: shown.to_owned(), source_rect: rect, review: Vec::new() });
                before.push_str(word.get(shown.len()..).unwrap_or(""));
            }
            None => {
                if !before.is_empty() {
                    before.push(' ');
                }
                before.push_str(word);
            }
        }
    }
    if !before.trim().is_empty() {
        out.push(plain(before));
    }
}

/// A block's lines as inlines; without a geometry, they have no source boxes.
pub(crate) fn inlines(lines: &[&ChessLine], geometry: Option<&PageGeometry>) -> Vec<Inline> {
    let mut out = Vec::new();
    // A move number waiting for its move: the number, its box and its line.
    let mut number: Option<(MoveNumber, Option<UserRect>, usize)> = None;
    let flush = |out: &mut Vec<Inline>, number: &mut Option<(MoveNumber, Option<UserRect>, usize)>| {
        if let Some((n, rect, _)) = number.take() {
            out.push(Inline::Text { text: n.text(), source_rect: rect, review: Vec::new() });
        }
    };
    for (li, line) in lines.iter().enumerate() {
        for span in &line.spans {
            let rect = geometry.and_then(|g| user(span.rect, g));
            match &span.run {
                ChessRun::MoveNumber { number: n, black } => {
                    flush(&mut out, &mut number);
                    number = Some((MoveNumber { number: *n, black: *black }, rect, li));
                }
                ChessRun::Move { san, text } => {
                    let (n, source_rect) = match number.take() {
                        Some((n, r, l)) if l == li => (Some(n), union(r, rect)),
                        Some((n, _, _)) => (Some(n), rect),
                        None => (None, rect),
                    };
                    out.push(Inline::Move(MoveInline {
                        number: n,
                        text: text.clone(),
                        san: san.to_english(),
                        status: MoveStatus::Unchecked,
                        fen: None,
                        game: None,
                        source_rect,
                        bold: span.bold,
                        review: Vec::new(),
                    }));
                }
                ChessRun::Nag(code) => {
                    flush(&mut out, &mut number);
                    out.push(Inline::Nag { code: *code, source_rect: rect, review: Vec::new() });
                }
                ChessRun::Figurine(kind) => {
                    flush(&mut out, &mut number);
                    out.push(Inline::Text { text: figurine(*kind, FigurineSet::Outline).to_string(), source_rect: rect, review: Vec::new() });
                }
                ChessRun::Text(text) => {
                    flush(&mut out, &mut number);
                    push_text(&mut out, text, rect);
                }
            }
        }
    }
    flush(&mut out, &mut number);
    out
}

fn text_block(kind: Option<HintKind>, lines: &[&ChessLine], geometry: &PageGeometry) -> Block {
    let source_rect = lines.iter().map(|l| user(l.rect, geometry)).fold(None, union);
    let inlines = inlines(lines, Some(geometry));
    match kind {
        Some(HintKind::Heading { level }) if titled(&inlines) => Block::Heading { level, inlines, source_rect },
        _ => Block::Paragraph { inlines, source_rect },
    }
}

/// Most moves a heading names (`The Evans Gambit: 4...Bb6 5 a4`).
pub const MAX_HEADING_MOVES: usize = 2;
/// Fewest letters of words a heading holds for each move it names.
pub const HEADING_LETTERS_PER_MOVE: usize = 12;

/// Whether a block a hint calls a heading reads as one. Books set their main lines larger and
/// bold, so a block of moves is a paragraph; a title names a move or two in its words
/// (`The Italian Regretted: White Plays 5 d3`): at most [`MAX_HEADING_MOVES`], with
/// [`HEADING_LETTERS_PER_MOVE`] letters each.
fn titled(inlines: &[Inline]) -> bool {
    let moves = inlines.iter().filter(|i| matches!(i, Inline::Move(_))).count();
    let letters: usize =
        inlines.iter().map(|i| if let Inline::Text { text, .. } = i { text.chars().filter(|c| c.is_alphabetic()).count() } else { 0 }).sum();
    moves == 0 || (moves <= MAX_HEADING_MOVES && letters >= moves.saturating_mul(HEADING_LETTERS_PER_MOVE))
}

/// The page's blocks, its trailing caption label ([`caption_numbers`]) and its page-number
/// offsets ([`folio_offsets`]). `carried` is the previous page's label, for this page's first
/// diagram when no caption numbers it. A bare number at the head or foot of the page whose offset
/// from the page's index is one of `folios` (seen on pages before) is the page's number, not a
/// caption.
fn blocks(input: &PageInput<'_>, carried: Option<u32>, folios: &[i64]) -> (Vec<Block>, Option<u32>, Vec<i64>) {
    let g = &input.geometry;
    let diagram_views: Vec<Option<ViewRect>> = input.diagrams.iter().map(|d| g.user_to_view(d.bbox_user)).collect();
    let hints: Vec<(HintKind, Option<ViewRect>)> = input.hints.iter().map(|h| (h.kind, g.user_to_view(h.rect))).collect();
    let mut blocks = Vec::new();
    let mut group: Vec<&ChessLine> = Vec::new();
    let mut key: Option<usize> = None;
    let flush = |blocks: &mut Vec<Block>, group: &mut Vec<&ChessLine>, key: Option<usize>| {
        if !group.is_empty() {
            let kind = key.and_then(|k| hints.get(k)).map(|h| h.0);
            blocks.push(text_block(kind, group, g));
            group.clear();
        }
    };
    let kept: Vec<&ChessLine> = input.text.lines.iter().filter(|l| !is_coordinates(l, &diagram_views)).collect();
    let page_width = input.geometry.view_size().0;
    let columns = columns(&kept, page_width);
    let ordered = column_order(&kept, &columns);
    let gutters = columns.gutters();
    let offsets = folio_offsets(&kept, input.index);
    let folio_lines: Vec<usize> = offsets.iter().filter(|(_, o)| folios.contains(o)).map(|(li, _)| *li).collect();
    let (puzzles, trailing) = caption_numbers(&kept, &diagram_views, &folio_lines);
    // The previous page's trailing label is the first diagram's in reading order.
    let mut carried = carried;
    // Without a gutter, the page's middle stands for one.
    let splits = if gutters.is_empty() { vec![page_width * 0.5] } else { gutters.clone() };
    for item in interleave(&ordered, &diagram_views, &splits, !gutters.is_empty()) {
        match item {
            Item::Diagram(i) => {
                flush(&mut blocks, &mut group, key);
                if let Some(reading) = input.diagrams.get(i) {
                    let mut block = DiagramBlock::from_reading(i, reading);
                    block.puzzle = puzzles.get(i).copied().flatten().or(carried.take());
                    carried = None;
                    blocks.push(Block::Diagram(block));
                }
            }
            Item::Line(line) => {
                let k = hint_of(line, &hints);
                let joins = group.last().is_some_and(|prev| k == key && (k.is_some() || same_paragraph(prev.rect, line.rect)));
                if !joins {
                    flush(&mut blocks, &mut group, key);
                    key = k;
                }
                group.push(line);
            }
        }
    }
    flush(&mut blocks, &mut group, key);
    (blocks, trailing, offsets.into_iter().map(|(_, o)| o).collect())
}

/// The page's trailing caption label ([`caption_numbers`]) from its text and its diagrams' boxes
/// (user space), as [`blocks`] finds it. Folios do not matter: a bare number is never a trailing
/// label, and a caption with its word or `№` wins over a bare number for a diagram.
pub(crate) fn trailing_label(text: &crate::chess_text::ChessPageText, geometry: &PageGeometry, diagrams: &[crate::coords::PageRect]) -> Option<u32> {
    let views: Vec<Option<ViewRect>> = diagrams.iter().map(|d| geometry.user_to_view(*d)).collect();
    let kept: Vec<&ChessLine> = text.lines.iter().filter(|l| !is_coordinates(l, &views)).collect();
    caption_numbers(&kept, &views, &[]).1
}

/// Give each text review to the inline it overlaps most; the rest to the page.
fn attach_reviews(input: &PageInput<'_>, blocks: &mut [Block], page: &mut Vec<ReviewFlag>) {
    for review in input.reviews {
        let Some(rect) = user(review.rect, &input.geometry) else {
            page.push(review.flag.clone());
            continue;
        };
        let mut best: Option<(f32, usize, usize)> = None;
        for (bi, block) in blocks.iter().enumerate() {
            for (ii, inline) in block.inlines().iter().enumerate() {
                let overlap = inline.source_rect().map_or(0.0, |r| r.overlap(&rect));
                if overlap > 0.0 && best.is_none_or(|(b, _, _)| overlap > b) {
                    best = Some((overlap, bi, ii));
                }
            }
        }
        let target = best.and_then(|(_, bi, ii)| match blocks.get_mut(bi) {
            Some(Block::Heading { inlines, .. } | Block::Paragraph { inlines, .. }) => inlines.get_mut(ii),
            _ => None,
        });
        match target {
            Some(inline) => inline.review_mut().push(review.flag.clone()),
            None => page.push(review.flag.clone()),
        }
    }
}

/// Run the page's blocks through the game walker, recording what became of each move.
/// `trailing` is the page's trailing caption label ([`caption_numbers`]), the next page's first
/// diagram's.
///
/// A label is a diagram's caption, not a solution, when a diagram beside it (the block before or
/// after) has its number, or when it is the trailing label after the page's last diagram: it
/// does not count among the solutions ([`Name::caption`]).
pub(crate) fn walk(page: u32, ocr: bool, blocks: &mut [Block], walker: &mut Walker, trailing: Option<u32>) {
    walker.read_by_ocr(ocr);
    let numbers: Vec<Option<u32>> = blocks.iter().map(|b| if let Block::Diagram(d) = b { d.puzzle } else { None }).collect();
    let last_diagram = blocks.iter().rposition(|b| matches!(b, Block::Diagram(_)));
    for (bi, block) in blocks.iter_mut().enumerate() {
        let caption = |n: u32| {
            let beside = |j: Option<usize>| j.and_then(|j| numbers.get(j)).copied().flatten() == Some(n);
            beside(bi.checked_sub(1)) || beside(bi.checked_add(1)) || (trailing == Some(n) && last_diagram.is_none_or(|d| d < bi))
        };
        let name = |(number, chapter): (u32, Option<u32>)| Name { number, chapter, caption: caption(number) };
        match block {
            Block::Diagram(d) => {
                let r = d.reading.as_ref();
                walker.diagram(Seed {
                    page,
                    diagram: d.number,
                    puzzle: d.puzzle,
                    fen: r.filter(|r| r.legal).map(|r| r.fen.clone()),
                    accepted: r.is_some_and(|r| r.accepted),
                    side_assumed: r.is_some_and(|r| r.side_assumed),
                });
            }
            Block::Heading { inlines, .. } => {
                let texts: Vec<&str> =
                    inlines.iter().filter_map(|i| if let Inline::Text { text, .. } = i { Some(text.as_str()) } else { None }).collect();
                walker.heading(heading_keys(&texts.join(" ")));
                // A solution's heading names its exercise (`Ex. 6–12`): its moves start from it.
                if let Some(label) = solution_name(&texts.concat()) {
                    walker.puzzle(name(label));
                }
            }
            Block::Paragraph { inlines, .. } => {
                // A number alone just before a first move (`1675 1.Rd3+`) names a puzzle too.
                let solutions: Vec<Option<u32>> = (0..inlines.len())
                    .map(|k| match (inlines.get(k), inlines.get(k + 1)) {
                        (Some(Inline::Text { text, .. }), Some(Inline::Move(m))) if m.number.is_some_and(|n| n.number == 1) => {
                            let t = text.trim();
                            (!t.is_empty() && t.len() <= 5 && t.chars().all(|c| c.is_ascii_digit())).then(|| t.parse().ok()).flatten()
                        }
                        _ => None,
                    })
                    .collect();
                for (k, inline) in inlines.iter_mut().enumerate() {
                    match inline {
                        Inline::Text { text, .. } | Inline::Link { text, .. } => {
                            // A solution names its puzzle (`№191. Lasker – Steinitz`): its moves
                            // start from that puzzle's diagram.
                            if let Some(label) = solution_name(text) {
                                walker.puzzle(name(label));
                            } else if let Some(n) = solutions.get(k).copied().flatten() {
                                walker.solution(name((n, None)));
                            }
                            walker.text(text);
                        }
                        Inline::Nag { code, .. } => walker.nag(*code),
                        Inline::Move(m) => {
                            // The English SAN reads back as the move it was written from.
                            let Ok(san) = San::read(&m.san, Lang::En) else { continue };
                            let out = walker.play(m.number, &san, &m.text, m.bold, Loc { page, rect: m.source_rect });
                            m.status = out.status;
                            if let Some(s) = out.san {
                                m.san = s;
                            }
                            m.fen = out.fen;
                            m.game = out.node;
                            m.review.extend(out.repaired);
                        }
                    }
                }
            }
        }
    }
}

fn poor_reason(reason: &PoorReason) -> String {
    match reason {
        PoorReason::Type0WithoutUnicode { ratio } => format!("two-byte codes with no Unicode text ({:.0}% of glyphs)", ratio * 100.0),
        PoorReason::ReplacementChars { ratio } => format!("replacement characters ({:.0}% of glyphs)", ratio * 100.0),
        PoorReason::DeadMap { ratio } => format!("characters with no text ({:.0}% of glyphs)", ratio * 100.0),
        PoorReason::UnresolvedChessCodes { glyphs } => format!("{glyphs} chess-font glyphs not resolved"),
        PoorReason::GarbledPieces { words } => format!("{words} moves whose piece is a stray symbol (an earlier OCR layer)"),
        PoorReason::HiddenOcrLayer { ratio } => format!("invisible text over an image of the page ({:.0}% of glyphs)", ratio * 100.0),
    }
}

/// One page, with the caption label `carried` from the page before it and the page-number
/// offsets `folios` seen before ([`blocks`]); also the label this page leaves for the next, and
/// its own page-number offsets.
pub(crate) fn page(input: &PageInput<'_>, walker: &mut Walker, carried: Option<u32>, folios: &[i64]) -> (ChessPage, Option<u32>, Vec<i64>) {
    let (mut blocks, trailing, offsets) = blocks(input, carried, folios);
    let mut review: Vec<ReviewFlag> = input.text.unknown_chess_fonts.iter().map(|f| ReviewFlag::UnknownChessFont { font: f.to_string() }).collect();
    if let TextQuality::Poor { reason } = &input.text.quality {
        review.push(ReviewFlag::PoorTextLayer { reason: poor_reason(reason) });
    }
    attach_reviews(input, &mut blocks, &mut review);
    walk(input.index, input.ocr, &mut blocks, walker, trailing);
    let page = ChessPage {
        index: input.index,
        lang: input.text.lang.lang.code().to_owned(),
        text_layer: (&input.text.quality).into(),
        ocr: input.ocr,
        blocks,
        review,
        trailing,
    };
    (page, trailing, offsets)
}
