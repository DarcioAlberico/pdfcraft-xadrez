//! One page's blocks from its text lines, diagrams and hints.
//!
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

fn interleave<'a>(lines: &'a [ChessLine], diagrams: &[Option<ViewRect>]) -> Vec<Item<'a>> {
    let mut items = Vec::with_capacity(lines.len().saturating_add(diagrams.len()));
    let mut next = 0usize;
    for line in lines {
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
    match kind {
        Some(HintKind::Heading { level }) => Block::Heading { level, inlines, source_rect },
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
    for item in interleave(&input.text.lines, &diagram_views) {
        match item {
            Item::Diagram(i) => {
                flush(&mut blocks, &mut group, key);
                if let Some(reading) = input.diagrams.get(i) {
                    blocks.push(Block::Diagram(DiagramBlock::from_reading(i, reading)));
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
                    fen: r.filter(|r| r.legal).map(|r| r.fen.clone()),
                    accepted: r.is_some_and(|r| r.accepted),
                    side_assumed: r.is_some_and(|r| r.side_assumed),
                });
            }
            Block::Heading { .. } => walker.heading(),
            Block::Paragraph { inlines, .. } => {
                for inline in inlines {
                    match inline {
                        Inline::Text { text, .. } | Inline::Link { text, .. } => walker.text(text),
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
