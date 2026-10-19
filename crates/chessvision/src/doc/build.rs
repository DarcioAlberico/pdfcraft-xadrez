//! One page's blocks from its text lines, diagrams and hints.
//!
//! 0. **Columns.** Lines are read column by column ([`column_order`]): on a page whose lines
//!    leave a gutter free (a two-column book), each band between full-width lines is read down its
//!    left column, then down its right one. The coordinates printed around a diagram (`a b c … h`,
//!    `8 … 1`) are not text and are left out ([`is_coordinates`]).
//! 1. **Diagrams among the lines.** Lines keep their reading order and diagrams theirs; a diagram
//!    goes before the first line below it in its column, or in a column to its right
//!    ([`diagram_before`]). Diagrams are never reordered among themselves, so the n-th diagram
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

use super::games::{Loc, Seed, Walker};
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

/// The puzzle number a text starts with: `№185.`, `N!!185.` (how text layers extract `№`),
/// `N9191.` (how OCR reads it), `No. 12`, `Nr. 12`, `#12`, followed by a full stop, a bracket,
/// a colon or a space. With `bare`, also a caption that is the number alone (`1675`) or the
/// number, a full stop and a name (`185. Teichmann`).
pub(crate) fn puzzle_label(text: &str, bare: bool) -> Option<u32> {
    let t = text.trim_start();
    let prefixed = ["№", "N!!", "N°", "Nº", "No.", "Nr.", "N9", "No", "Nr", "#"].iter().find_map(|p| t.strip_prefix(p)).map(str::trim_start);
    let rest = match prefixed {
        Some(rest) => rest,
        None if bare && t.chars().count() <= MAX_BARE_CAPTION_CHARS => t,
        None => return None,
    };
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 || digits > 5 {
        return None;
    }
    // Digits are ASCII, so `digits` is a char boundary.
    let after = rest.get(digits..)?.chars().next();
    let ends = match (prefixed.is_some(), after) {
        // A bare number stands alone (`1675`) or ends in a full stop, a space and a name
        // (`185. Teichmann`): `1996 was` is prose, `6.` a mark under a diagram and `2.2
        // Combinations` a section.
        (false, _) => {
            let rem = rest.get(digits..).unwrap_or_default();
            let named = rem.strip_prefix('.').is_some_and(|r| r.starts_with(char::is_whitespace) && r.chars().any(char::is_alphabetic));
            rem.trim().is_empty() || named
        }
        (true, None) => true,
        (true, Some(c)) => matches!(c, '.' | ')' | ':') || c.is_whitespace(),
    };
    if !ends {
        return None;
    }
    rest.get(..digits)?.parse().ok().filter(|n| *n > 0 && *n <= MAX_PUZZLE)
}

/// A caption line's text, or `None` for a line with moves in it.
fn caption_text(line: &ChessLine) -> Option<String> {
    let mut out = String::new();
    for span in &line.spans {
        match &span.run {
            ChessRun::Text(t) => out.push_str(t),
            // Read as a move number (`1675` before a move-like word); the caption is the number.
            ChessRun::MoveNumber { number, .. } => out.push_str(&number.to_string()),
            ChessRun::Nag(code) => out.push_str(pdfcraft_chess::nag::by_code(*code).map_or("", |n| n.glyph)),
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
fn caption_numbers(lines: &[&ChessLine], diagrams: &[Option<ViewRect>]) -> Vec<Option<u32>> {
    // (bare, gap, number) per diagram: a caption with its `№` wins over a bare number.
    let mut best: Vec<Option<(bool, f32, u32)>> = vec![None; diagrams.len()];
    for l in lines {
        let Some(text) = caption_text(l) else { continue };
        let (n, bare) = match (puzzle_label(&text, false), puzzle_label(&text, true)) {
            (Some(n), _) => (n, false),
            (None, Some(n)) => (n, true),
            (None, None) => continue,
        };
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
            && slot.is_none_or(|(b, g, _)| (bare, gap) < (b, g))
        {
            *slot = Some((bare, gap, n));
        }
    }
    best.into_iter().map(|b| b.map(|(_, _, n)| n)).collect()
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

/// The x of a gutter that splits the page's lines into two columns, if there is one: the middle
/// of the widest run of positions (between 30% and 70% of the page) that the fewest lines cross,
/// with enough lines on both sides, wide enough on average or with enough wide ones among them
/// ([`MIN_WIDE_COLUMN_LINES`]).
pub(crate) fn gutter(lines: &[&ChessLine], page_width: f32) -> Option<f32> {
    if !(page_width.is_finite() && page_width > 0.0) || lines.len() < 2 * MIN_COLUMN_LINES {
        return None;
    }
    let max_crossing = ((lines.len() as f32) * MAX_CROSSING_SHARE).floor() as usize;
    // Candidate positions, one point apart; bounded by the page width (at most a few thousand).
    let (lo, hi) = (page_width * 0.3, page_width * 0.7);
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

/// The lines in reading order, column by column: lines that cross the gutter (if any) cut the
/// page into bands, and each band is read down its left column, then down its right one. Without a
/// gutter, the lines keep their order.
pub(crate) fn column_order<'a>(lines: &[&'a ChessLine], page_width: f32) -> Vec<&'a ChessLine> {
    let Some(x) = gutter(lines, page_width) else { return lines.to_vec() };
    let top = |l: &&ChessLine| l.rect.y0;
    let mut out = Vec::with_capacity(lines.len());
    let mut left: Vec<&ChessLine> = Vec::new();
    let mut right: Vec<&ChessLine> = Vec::new();
    let mut sorted: Vec<&ChessLine> = lines.to_vec();
    sorted.sort_by(|a, b| top(a).total_cmp(&top(b)).then(a.rect.x0.total_cmp(&b.rect.x0)));
    let flush = |out: &mut Vec<&'a ChessLine>, left: &mut Vec<&'a ChessLine>, right: &mut Vec<&'a ChessLine>| {
        out.append(left);
        out.append(right);
    };
    for line in sorted {
        if line.rect.x1 <= x {
            left.push(line);
        } else if line.rect.x0 >= x {
            right.push(line);
        } else {
            // A full-width line: what is above it is read first.
            flush(&mut out, &mut left, &mut right);
            out.push(line);
        }
    }
    flush(&mut out, &mut left, &mut right);
    out
}

enum Item<'a> {
    Line(&'a ChessLine),
    Diagram(usize),
}

/// Whether diagram `d` comes before line `l` (view space): `l` is below `d`'s middle in the same
/// column, or in a column to its right.
fn diagram_before(d: ViewRect, l: ViewRect) -> bool {
    let same_column = d.x0 < l.x1 && l.x0 < d.x1;
    if same_column { (l.y0 + l.y1) * 0.5 >= (d.y0 + d.y1) * 0.5 } else { l.x0 >= d.x1 }
}

fn interleave<'a>(lines: &[&'a ChessLine], diagrams: &[Option<ViewRect>]) -> Vec<Item<'a>> {
    let mut items = Vec::with_capacity(lines.len().saturating_add(diagrams.len()));
    let mut next = 0usize;
    for &line in lines {
        while let Some(d) = diagrams.get(next) {
            if !d.is_some_and(|d| diagram_before(d, line.rect)) {
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

/// A block's lines as inlines.
fn inlines(lines: &[&ChessLine], geometry: &PageGeometry) -> Vec<Inline> {
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
            let rect = user(span.rect, geometry);
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
    let inlines = inlines(lines, geometry);
    // A block with moves in it is never a heading: books set their main lines larger and bold.
    let moves = inlines.iter().any(|i| matches!(i, Inline::Move(_)));
    match kind {
        Some(HintKind::Heading { level }) if !moves => Block::Heading { level, inlines, source_rect },
        _ => Block::Paragraph { inlines, source_rect },
    }
}

fn blocks(input: &PageInput<'_>) -> Vec<Block> {
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
    let ordered = column_order(&kept, input.geometry.view_size().0);
    let puzzles = caption_numbers(&kept, &diagram_views);
    for item in interleave(&ordered, &diagram_views) {
        match item {
            Item::Diagram(i) => {
                flush(&mut blocks, &mut group, key);
                if let Some(reading) = input.diagrams.get(i) {
                    let mut block = DiagramBlock::from_reading(i, reading);
                    block.puzzle = puzzles.get(i).copied().flatten();
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
    blocks
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
fn walk(page: u32, blocks: &mut [Block], walker: &mut Walker) {
    for block in blocks {
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
            Block::Heading { .. } => walker.heading(),
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
                            if let Some(n) = puzzle_label(text, false) {
                                walker.puzzle(n);
                            } else if let Some(n) = solutions.get(k).copied().flatten() {
                                walker.solution(n);
                            }
                            walker.text(text);
                        }
                        Inline::Nag { code, .. } => walker.nag(*code),
                        Inline::Move(m) => {
                            // The English SAN reads back as the move it was written from.
                            let Ok(san) = San::read(&m.san, Lang::En) else { continue };
                            let out = walker.play(m.number, &san, &m.text, Loc { page, rect: m.source_rect });
                            m.status = out.status;
                            if let Some(s) = out.san {
                                m.san = s;
                            }
                            m.fen = out.fen;
                            m.game = out.node;
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
    }
}

pub(crate) fn page(input: &PageInput<'_>, walker: &mut Walker) -> ChessPage {
    let mut blocks = blocks(input);
    let mut review: Vec<ReviewFlag> = input.text.unknown_chess_fonts.iter().map(|f| ReviewFlag::UnknownChessFont { font: f.to_string() }).collect();
    if let TextQuality::Poor { reason } = &input.text.quality {
        review.push(ReviewFlag::PoorTextLayer { reason: poor_reason(reason) });
    }
    attach_reviews(input, &mut blocks, &mut review);
    walk(input.index, &mut blocks, walker);
    ChessPage { index: input.index, lang: input.text.lang.lang.code().to_owned(), text_layer: (&input.text.quality).into(), blocks, review }
}
