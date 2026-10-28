//! A recognised chess document as Markdown, and every position as PGN (blueprint S15d).
//!
//! **Markdown** ([`write`]). Headings are `#` lines; paragraphs carry the moves with figurines
//! (or letters, or as printed); each diagram is an image whose alt text is its FEN
//! (`![FEN](diagram.png)`, the file named by the caller, which writes the image) or, without
//! images, the FEN in backticks, with the lichess link under it. Text is escaped so that a
//! Markdown reader shows it as printed: `1. e4` at the start of a paragraph stays text, not a
//! list item, and `*`, `_`, `[` and friends stay characters.
//!
//! **PGN** ([`pgn_with_diagrams`]). The games found in the text, and for every legal diagram
//! that starts none a game of its own with no moves: `[SetUp "1"]`, `[FEN …]` and the page and
//! diagram it came from, so a database gets every position of the book. Each game says where it
//! came from through [`PgnOrigin`]: its tags, and a first comment in the book's language
//! (`Pág. 17 · diagrama 1 de 2`), with the book's title and printed page numbers from
//! [`PgnBook`] when the caller knows them.

use std::fmt::Write as _;

use pdfcraft_chess::origin::{PgnOrigin, with_origin_comment, with_origin_tags};
use pdfcraft_chess::san::Lang;

use super::games::{GameOrigin, GameResult, GameScore, Tag};
use super::html::{Figurines, plain_text_with};
use super::{Block, ChessDoc};

/// How [`write`] sets diagrams.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkdownDiagrams {
    /// An image, named by [`MarkdownOptions::image_name`].
    Images,
    /// The FEN in backticks.
    Fen,
}

/// What [`write`] makes.
#[derive(Clone, Debug)]
pub struct MarkdownOptions<'a> {
    pub title: &'a str,
    pub figurines: Figurines,
    pub diagrams: MarkdownDiagrams,
    pub links: bool,
    pub link_text: &'a str,
    /// The folder the images are in, relative to the Markdown file (empty: beside it).
    pub image_dir: &'a str,
}

/// The image name of diagram `number` (0-based) of page `index` (0-based), relative to the
/// Markdown file: `diagram-p0001-1.png`.
pub fn image_name(index: u32, number: usize) -> String {
    format!("diagram-p{:04}-{}.png", u64::from(index) + 1, number + 1)
}

/// Escape inline Markdown: backslash, emphasis, code, links, HTML and line-start markers.
fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars().filter(|c| !c.is_control()) {
        if matches!(c, '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '|' | '~') {
            out.push('\\');
        }
        out.push(c);
    }
    // A paragraph that starts like a list item or a heading stays a paragraph: `1. e4`, `- x`,
    // `+ x`, `# x` (`#` matters only there: `4.Qxf7#` keeps its mate sign as printed).
    let digits = out.chars().take_while(char::is_ascii_digit).count();
    let rest = out.get(digits..).unwrap_or_default();
    if digits > 0 && (rest.starts_with(". ") || rest.starts_with(") ") || rest == "." || rest == ")") {
        out.insert(digits, '\\');
    } else if out.starts_with("- ") || out.starts_with("+ ") || out == "-" || out == "+" || out.starts_with('#') {
        out.insert(0, '\\');
    }
    out
}

/// An address that may be a link: `http` or `https`, with nothing that ends a Markdown link.
fn safe_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    (lower.starts_with("http://") || lower.starts_with("https://"))
        && !url.chars().any(|c| c.is_whitespace() || c.is_control() || "()<>[]".contains(c))
}

/// The document as Markdown.
pub fn write(doc: &ChessDoc, o: &MarkdownOptions<'_>) -> String {
    let mut s = format!("# {}\n\n", esc(o.title));
    for (n, page) in doc.pages.iter().enumerate() {
        if n > 0 {
            s.push_str("---\n\n");
        }
        for block in &page.blocks {
            match block {
                Block::Heading { level, inlines, .. } => {
                    // The document's title is the one `#`: headings start at `##`.
                    let hashes = "#".repeat(usize::from((*level).clamp(1, 5)) + 1);
                    let _ = write!(s, "{hashes} {}\n\n", esc(&plain_text_with(inlines, o.figurines)));
                }
                Block::Paragraph { inlines, .. } => {
                    let _ = write!(s, "{}\n\n", esc(&plain_text_with(inlines, o.figurines)));
                }
                Block::Diagram(d) => {
                    let Some(reading) = &d.reading else {
                        let _ = write!(s, "*Diagram {} (not read)*\n\n", d.number + 1);
                        continue;
                    };
                    // The FEN holds no Markdown or link characters but `/`, digits, letters,
                    // spaces and `-`; escaped all the same.
                    match o.diagrams {
                        MarkdownDiagrams::Images => {
                            let name = image_name(page.index, d.number);
                            // A link destination: spaces encoded, nothing that would end it.
                            let dir: String = o
                                .image_dir
                                .trim_matches('/')
                                .chars()
                                .filter(|c| !"()<>".contains(*c) && !c.is_control())
                                .collect::<String>()
                                .replace(' ', "%20");
                            let path = if dir.is_empty() { name } else { format!("{dir}/{name}") };
                            let _ = write!(s, "![{}]({path})\n\n", esc(&reading.fen));
                        }
                        MarkdownDiagrams::Fen => {
                            let _ = write!(s, "`{}`\n\n", reading.fen.replace('`', ""));
                        }
                    }
                    if o.links
                        && let Some(url) = d.lichess_url.as_deref().filter(|u| safe_url(u))
                    {
                        let _ = write!(s, "[{}]({url})\n\n", esc(o.link_text));
                    }
                }
            }
        }
    }
    s
}

/// What the document a PGN is written from knows that its reading does not: the book's title and
/// the number printed on each page (its page labels, by page index; empty when it has none).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PgnBook {
    pub title: Option<String>,
    pub labels: Vec<String>,
}

/// A game with no moves for a diagram: its position, and where it came from.
fn position_game(fen: &str, page: u32, number: usize, puzzle: Option<u32>, accepted: bool) -> GameScore {
    let mut g = GameScore::new(Some(fen.to_owned()));
    let origin = PgnOrigin { page_index: page, diagram: Some(number.saturating_add(1)), puzzle, ..PgnOrigin::default() };
    g.extra_tags.extend(origin.tags().into_iter().map(|(name, value)| Tag { name, value }));
    if !accepted {
        g.comment = "The reading of this diagram did not pass review; check it against the book.".to_owned();
    }
    g.result = GameResult::Unknown;
    g
}

/// Every game, and a game with no moves for each legal diagram that starts none, in the order of
/// the book: page by page, a diagram's game where the diagram is, then the page's other games.
/// Returns the PGN and how many position-only games were added.
pub fn pgn_with_diagrams(doc: &ChessDoc) -> (String, usize) {
    pgn_with_diagrams_in(doc, &PgnBook::default())
}

/// [`pgn_with_diagrams`], each game saying where in `book` it came from (plan A8 P8a): the
/// origin's tags, its first comment, and for a position the book's title as its `Event`.
pub fn pgn_with_diagrams_in(doc: &ChessDoc, book: &PgnBook) -> (String, usize) {
    let mut out: Vec<GameScore> = Vec::new();
    let mut used = vec![false; doc.games.len()];
    let mut added = 0;
    for page in &doc.pages {
        for block in &page.blocks {
            let Block::Diagram(d) = block else { continue };
            let from_here = doc
                .games
                .iter()
                .position(|g| matches!(g.origin, GameOrigin::Diagram { page: p, diagram, .. } if p == page.index && diagram == d.number));
            match (from_here, &d.reading) {
                (Some(i), _) => {
                    if let (Some(g), Some(slot)) = (doc.games.get(i), used.get_mut(i)) {
                        out.push(with_origin(doc, book, g));
                        *slot = true;
                    }
                }
                (None, Some(r)) if r.legal && !r.repaired && GameScore::new(Some(r.fen.clone())).start_board().is_some() => {
                    let game = position_game(&r.fen, page.index, d.number, d.puzzle, r.accepted);
                    let origin = origin_of(doc, book, page.index, Some(d.number), d.puzzle);
                    out.push(apply_origin(game, &origin, true));
                    added += 1;
                }
                _ => {}
            }
        }
        for (i, g) in doc.games.iter().enumerate() {
            if g.page == page.index
                && let Some(slot) = used.get_mut(i)
                && !*slot
            {
                out.push(with_origin(doc, book, g));
                *slot = true;
            }
        }
    }
    // Games whose page is not among the pages (none should be): kept, at the end.
    for (i, g) in doc.games.iter().enumerate() {
        if !used.get(i).copied().unwrap_or(true) {
            out.push(with_origin(doc, book, g));
        }
    }
    (super::pgn::write(out.iter()), added)
}

/// A game of the document with its origin: the diagram it starts at, else the page of its first
/// move.
fn with_origin(doc: &ChessDoc, book: &PgnBook, game: &super::games::Game) -> GameScore {
    let (page, diagram) = match game.origin {
        GameOrigin::Diagram { page, diagram, .. } => (page, Some(diagram)),
        GameOrigin::Initial => (game.page, None),
    };
    let puzzle = game.score.extra_tags.iter().find(|t| t.name == "PdfPuzzle").and_then(|t| t.value.parse().ok());
    apply_origin(game.score.clone(), &origin_of(doc, book, page, diagram, puzzle), false)
}

/// The origin of page `page`'s diagram `diagram` (counted from 0), or of the page.
fn origin_of(doc: &ChessDoc, book: &PgnBook, page: u32, diagram: Option<usize>, puzzle: Option<u32>) -> PgnOrigin {
    let read = doc.pages.iter().find(|p| p.index == page);
    let diagrams = read.map_or(0, |p| p.blocks.iter().filter(|b| matches!(b, Block::Diagram(_))).count());
    PgnOrigin {
        book_title: book.title.clone(),
        page_index: page,
        page_label: usize::try_from(page).ok().and_then(|i| book.labels.get(i)).cloned(),
        diagram: diagram.map(|d| d.saturating_add(1)),
        of: (diagrams > 0).then_some(diagrams),
        puzzle,
        lang: read.and_then(|p| Lang::from_tag(&p.lang)),
    }
}

/// The game with the origin's tags in place of its own, the origin's comment first and, for a
/// position, the book's title as its event.
fn apply_origin(mut game: GameScore, origin: &PgnOrigin, position: bool) -> GameScore {
    let tags: Vec<(String, String)> = game.extra_tags.iter().map(|t| (t.name.clone(), t.value.clone())).collect();
    game.extra_tags = with_origin_tags(&tags, origin).into_iter().map(|(name, value)| Tag { name, value }).collect();
    game.comment = with_origin_comment(&game.comment, origin);
    if position && let Some((event, round)) = origin.position_roster() {
        game.roster.event = event;
        game.roster.round = round;
    }
    game
}
