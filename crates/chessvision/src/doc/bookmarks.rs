//! A book's bookmarks from its reading (plan A9-N6, A8-13): its chapters (the headings), its games
//! (those a header names) and its exercises (the diagrams a caption numbers), in reading order,
//! for a PDF's outline under one bookmark of their own; and the index of its games, as CSV or
//! Markdown.
//!
//! Nothing here is new reading: each entry is what the walk already kept.
//!
//! - **Chapters**: the book's headings (`Block::Heading`, its text, not the page's furniture:
//!   running heads, folios and text inside a diagram are left out, [`ChessPage::book_blocks`]),
//!   level 1 or 2 as read.
//! - **Games**: the games whose header names both players ([`games::Game`]'s roster, filled from
//!   a game's header, plan A9-L1a), numbered in the book's order, titled as an index prints them:
//!   `3. Fischer–Spassky, Siegen 1970`. A game read from a diagram with no header (an exercise's
//!   solution, a line) is not one.
//! - **Exercises**: the diagrams a caption numbers (`№185`, `Exercise 4`: [`DiagramBlock::puzzle`]).
//!
//! Games and exercises go one level under the heading before them (at the top before any), where
//! their first move or their diagram stands; chapters nest by their level. At most
//! [`MAX_ENTRIES`] entries, and a title at most [`MAX_TITLE_CHARS`] characters.

use std::fmt::Write as _;

use super::games::Game;
use super::{Block, ChessDoc, Inline, PageLabel, plain_text};

/// Most bookmarks made from one book.
pub const MAX_ENTRIES: usize = 20_000;

/// Longest title of a bookmark, in characters (a longer one is cut, with `…`).
pub const MAX_TITLE_CHARS: usize = 120;

/// What a bookmark stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EntryKind {
    Chapter,
    Game,
    Exercise,
}

impl EntryKind {
    /// As the tools name it.
    pub fn name(self) -> &'static str {
        match self {
            EntryKind::Chapter => "chapter",
            EntryKind::Game => "game",
            EntryKind::Exercise => "exercise",
        }
    }
}

/// One bookmark of the book's outline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BookEntry {
    /// 1 = directly under the book's own bookmark; a deeper one nests under the last shallower
    /// entry before it.
    pub level: u8,
    pub kind: EntryKind,
    pub title: String,
    /// The page (0-based) it goes to.
    pub page: u32,
}

/// Which kinds of entries an outline has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Include {
    pub chapters: bool,
    pub games: bool,
    pub exercises: bool,
}

impl Default for Include {
    fn default() -> Include {
        Include { chapters: true, games: true, exercises: true }
    }
}

/// `text` cut to [`MAX_TITLE_CHARS`] characters (with `…`), its runs of white space one space.
fn title(text: &str) -> String {
    let words: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if words.chars().count() <= MAX_TITLE_CHARS {
        return words;
    }
    let mut cut: String = words.chars().take(MAX_TITLE_CHARS.saturating_sub(1)).collect();
    cut.push('…');
    cut
}

/// A name of the roster that is said (not `?` nor empty).
fn said(name: &str) -> Option<&str> {
    let name = name.trim();
    (!name.is_empty() && name != "?").then_some(name)
}

/// Whether the game is one a header names: both players said.
pub fn is_titled(game: &Game) -> bool {
    said(&game.score.roster.white).is_some() && said(&game.score.roster.black).is_some()
}

/// A player as an index names them: the surname, the part before a comma (`Fischer, Robert James`
/// is `Fischer`).
fn surname(name: &str) -> &str {
    let name = said(name).unwrap_or("?");
    name.split(',').next().map(str::trim).filter(|s| !s.is_empty()).unwrap_or(name)
}

/// A game's title as an index prints it: `3. Fischer–Spassky, Siegen 1970` (the players'
/// surnames, then the event and the year when said).
pub fn game_title(number: usize, game: &Game) -> String {
    let r = &game.score.roster;
    let mut out = format!("{number}. {}–{}", surname(&r.white), surname(&r.black));
    let year = r.date.get(..4).filter(|y| y.chars().all(|c| c.is_ascii_digit()));
    match (said(&r.event), year) {
        (Some(event), Some(year)) => {
            let _ = write!(out, ", {event} {year}");
        }
        (Some(event), None) => {
            let _ = write!(out, ", {event}");
        }
        (None, Some(year)) => {
            let _ = write!(out, ", {year}");
        }
        (None, None) => {}
    }
    title(&out)
}

/// Where each game starts in the book: the page and block of its first move (`None` for a game
/// none of whose moves the pages show).
fn starts(doc: &ChessDoc) -> Vec<Option<(u32, usize, usize)>> {
    let mut at: Vec<Option<(u32, usize, usize)>> = vec![None; doc.games.len()];
    for page in &doc.pages {
        for (bi, block) in page.blocks.iter().enumerate() {
            for (ii, inline) in block.inlines().iter().enumerate() {
                if let Inline::Move(m) = inline
                    && let Some(slot) = m.game.and_then(|g| at.get_mut(g.game))
                    && slot.is_none()
                {
                    *slot = Some((page.index, bi, ii));
                }
            }
        }
    }
    at
}

/// The book's outline: its chapters, games and exercises as `include` says, in reading order,
/// nested as the module says. An exercise is titled `exercise_label` and its number
/// (`Exercise 185`).
pub fn outline(doc: &ChessDoc, include: Include, exercise_label: &str) -> Vec<BookEntry> {
    // (page, block, place in the block, entry); a heading's level as read.
    let mut events: Vec<(u32, usize, usize, EntryKind, u8, String)> = Vec::new();
    for page in &doc.pages {
        for (bi, block) in page.book_blocks() {
            match block {
                Block::Heading { level, inlines, role, .. } if include.chapters && role.is_content() => {
                    let text = title(&plain_text(inlines));
                    if !text.is_empty() {
                        events.push((page.index, bi, 0, EntryKind::Chapter, (*level).clamp(1, 2), text));
                    }
                }
                Block::Diagram(d) if include.exercises => {
                    if let Some(n) = d.puzzle {
                        events.push((page.index, bi, 0, EntryKind::Exercise, 0, title(&format!("{} {n}", exercise_label.trim()))));
                    }
                }
                _ => {}
            }
        }
    }
    if include.games {
        let starts = starts(doc);
        let mut number = 0usize;
        for (game, start) in doc.games.iter().zip(starts) {
            let Some((page, block, place)) = start.filter(|_| is_titled(game)) else { continue };
            number = number.saturating_add(1);
            events.push((page, block, place, EntryKind::Game, 0, game_title(number, game)));
        }
    }
    events.sort_by_key(|e| (e.0, e.1, e.2, e.3 != EntryKind::Chapter));
    let mut heading = 0u8;
    let mut out = Vec::new();
    for (page, _, _, kind, level, title) in events.into_iter().take(MAX_ENTRIES) {
        let level = match kind {
            EntryKind::Chapter => {
                heading = level;
                level
            }
            EntryKind::Game | EntryKind::Exercise => heading.saturating_add(1),
        };
        out.push(BookEntry { level, kind, title, page });
    }
    out
}

/// One game of the book's index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexRow {
    /// From 1, in the book's order: the number its bookmark has.
    pub number: usize,
    pub white: String,
    pub black: String,
    pub event: String,
    pub site: String,
    pub date: String,
    pub round: String,
    pub result: String,
    pub eco: String,
    /// The page (0-based) its first move is on.
    pub page: u32,
}

/// The book's games a header names ([`is_titled`]), numbered as their bookmarks are.
pub fn game_index(doc: &ChessDoc) -> Vec<IndexRow> {
    let starts = starts(doc);
    doc.games
        .iter()
        .zip(starts)
        .filter_map(|(g, start)| start.filter(|_| is_titled(g)).map(|(page, _, _)| (g, page)))
        .enumerate()
        .map(|(i, (g, page))| {
            let r = &g.score.roster;
            let shown = |s: &str| if s.trim() == "?" { String::new() } else { s.trim().to_owned() };
            IndexRow {
                number: i.saturating_add(1),
                white: shown(&r.white),
                black: shown(&r.black),
                event: shown(&r.event),
                site: shown(&r.site),
                date: if r.date.starts_with("????") { String::new() } else { r.date.clone() },
                round: shown(&r.round),
                result: g.score.result.as_pgn().to_owned(),
                eco: g.score.extra_tags.iter().find(|t| t.name == "ECO").map(|t| t.value.clone()).unwrap_or_default(),
                page,
            }
        })
        .collect()
}

/// The index as CSV (RFC 4180: a header, then a row each, lines ended by CR LF): `number`,
/// `white`, `black`, `event`, `site`, `date`, `round`, `result`, `eco`, `page` (the PDF's, from
/// 1) and `page_label` (as the book numbers it, `labels` by page index).
pub fn index_csv(rows: &[IndexRow], labels: &[String]) -> String {
    let mut out = String::from("number,white,black,event,site,date,round,result,eco,page,page_label\r\n");
    for r in rows {
        let fields = [
            r.number.to_string(),
            r.white.clone(),
            r.black.clone(),
            r.event.clone(),
            r.site.clone(),
            r.date.clone(),
            r.round.clone(),
            r.result.clone(),
            r.eco.clone(),
            u64::from(r.page).saturating_add(1).to_string(),
            PageLabel::of(labels, r.page).to_string(),
        ];
        let row: Vec<String> = fields.iter().map(|f| super::errata::csv_field(f)).collect();
        out.push_str(&row.join(","));
        out.push_str("\r\n");
    }
    out
}

/// The index as a Markdown document titled `title`: a table of the same columns as [`index_csv`],
/// its cells escaped so they stay text; a line saying there are none when there are none.
pub fn index_markdown(rows: &[IndexRow], labels: &[String], title: &str) -> String {
    let esc = super::markdown::esc;
    let mut out = format!("# Index of games: {}\n\n", esc(title));
    if rows.is_empty() {
        out.push_str("No game with a header was found.\n");
        return out;
    }
    out.push_str("| No. | White | Black | Event | Site | Date | Round | Result | ECO | Page | Printed page |\n");
    out.push_str("|---:|---|---|---|---|---|---|---|---|---:|---|\n");
    for r in rows {
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            r.number,
            esc(&r.white),
            esc(&r.black),
            esc(&r.event),
            esc(&r.site),
            esc(&r.date),
            esc(&r.round),
            esc(&r.result),
            esc(&r.eco),
            u64::from(r.page).saturating_add(1),
            esc(&PageLabel::of(labels, r.page).to_string()),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(white: &str, event: &str) -> IndexRow {
        IndexRow {
            number: 1,
            white: white.into(),
            black: "Spassky, Boris".into(),
            event: event.into(),
            site: String::new(),
            date: "1970.09.20".into(),
            round: "5".into(),
            result: "1-0".into(),
            eco: "C95".into(),
            page: 0,
        }
    }

    /// The index is text a spreadsheet and a Markdown reader keep as text: a cell that would be a
    /// formula is defused, a comma or quote quoted, a bar escaped; a long title is cut.
    #[test]
    fn the_index_is_csv_with_formulas_defused_and_markdown_escaped() {
        let rows = [row("Fischer, Robert", "=HYPERLINK(\"x\")"), IndexRow { number: 2, ..row("Tal | Mikhail", "Moscow") }];
        let labels = vec!["xiii".to_owned()];
        let csv = index_csv(&rows, &labels);
        assert!(csv.starts_with("number,white,black,event,site,date,round,result,eco,page,page_label\r\n"), "{csv}");
        assert!(csv.contains("1,\"Fischer, Robert\",\"Spassky, Boris\",\"'=HYPERLINK(\"\"x\"\")\",,1970.09.20,5,1-0,C95,1,xiii\r\n"), "{csv}");
        let md = index_markdown(&rows, &labels, "Book");
        assert!(md.contains("| 2 | Tal \\| Mikhail |"), "{md}");
        assert!(index_markdown(&[], &labels, "Book").contains("No game with a header was found."));
        let long = "x".repeat(MAX_TITLE_CHARS + 10);
        let cut = title(&long);
        assert_eq!((cut.chars().count(), cut.ends_with('…')), (MAX_TITLE_CHARS, true));
        assert_eq!(title("  Chapter \n 1 "), "Chapter 1");
        assert_eq!(surname("Fischer, Robert James"), "Fischer");
        assert_eq!(surname("?"), "?");
    }
}
