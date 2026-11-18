//! The book's exercises as a sheet to print (plan A9-N12a, finding A8-14): a grid of diagrams a
//! page, each captioned `Nº 185 · p. 40 · White to move`, and the solutions at the end, in the
//! order of the numbers. What this module gives is the sheet's content; the engine draws it as a
//! new PDF (`engine::chess::worksheet`).
//!
//! The exercises are the problem PGN's and Anki's ([`super::exercises`]): the diagrams the book
//! numbers, in the book's order. A numbered diagram whose solution was not read is on the sheet
//! too (decision D30), from the position it shows, and the key says so (and that its side to move
//! was assumed, when it was); one with no position to start from is left out and listed.

use pdfcraft_chess::describe;
use pdfcraft_chess::placement::Color;
use pdfcraft_chess::san::Lang;

use super::exercises::{LeftOut, LeftOutReason, find};
use super::games::DiagramAt;
use super::markdown::PgnBook;
use super::{Block, ChessDoc, PageLabel};

/// Most exercises one sheet holds (a book of problems numbers thousands); the rest are counted.
pub const MAX_EXERCISES: usize = 10_000;

/// How many diagrams a page of the sheet holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PerPage {
    /// Two columns, two rows.
    #[default]
    Four,
    /// Two columns, three rows.
    Six,
    /// Three columns, three rows.
    Nine,
}

impl PerPage {
    /// The page's columns and rows.
    pub fn grid(self) -> (usize, usize) {
        match self {
            PerPage::Four => (2, 2),
            PerPage::Six => (2, 3),
            PerPage::Nine => (3, 3),
        }
    }

    /// How many diagrams a page holds.
    pub fn count(self) -> usize {
        let (c, r) = self.grid();
        c.saturating_mul(r)
    }

    /// The layout of `n` (4, 6 or 9) diagrams a page.
    pub fn of(n: u64) -> Option<PerPage> {
        match n {
            4 => Some(PerPage::Four),
            6 => Some(PerPage::Six),
            9 => Some(PerPage::Nine),
            _ => None,
        }
    }
}

/// How the sheet is laid out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Layout {
    pub per_page: PerPage,
    /// The solutions at the end.
    pub solutions: bool,
    /// The captions' language; `None`: the book's, page by page.
    pub lang: Option<Lang>,
}

/// One exercise on the sheet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub at: DiagramAt,
    /// The book's number for it.
    pub number: u32,
    /// The position to solve from, a full FEN.
    pub fen: String,
    /// The side the book prints it from.
    pub orientation: Color,
    /// `Nº 185 · p. 40 · White to move`.
    pub caption: String,
    /// The caption in English, for a sheet whose font cannot write the book's language.
    pub caption_en: String,
    /// The solution's main line (`1.Qh5 Nf6 2.Qxf7#`); `None` when none was read.
    pub solution: Option<String>,
    /// Its side to move was assumed (nothing said it).
    pub side_assumed: bool,
    /// The language of its caption.
    pub lang: Lang,
}

/// What [`worksheet`] made.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Worksheet {
    /// The diagram pages, each at most [`PerPage::count`] exercises, in the book's order.
    pub pages: Vec<Vec<Item>>,
    /// The key, in the order of the numbers (empty without [`Layout::solutions`]): a line each,
    /// `Nº 185 · p. 40: 1.Qh5 Nf6 2.Qxf7#`, and the same in English.
    pub solutions: Vec<(String, String)>,
    /// Numbered diagrams with no position to start from: not on the sheet.
    pub left_out: Vec<LeftOut>,
    /// Exercises past [`MAX_EXERCISES`], counted, not on the sheet.
    pub over: usize,
    /// Where each of those is, in the book's order (plan A9-N12b2: the export dialog counts only
    /// the pages it believes).
    pub over_at: Vec<DiagramAt>,
}

impl Worksheet {
    /// Every exercise on the sheet, in its order.
    pub fn items(&self) -> impl Iterator<Item = &Item> {
        self.pages.iter().flatten()
    }
}

/// "solution not read" and "side to move assumed", by language.
fn not_read(lang: Lang) -> (&'static str, &'static str) {
    match lang {
        Lang::Pt => ("solução não lida", "lado suposto"),
        Lang::Es => ("solución no leída", "turno supuesto"),
        Lang::De => ("Lösung nicht gelesen", "Zugrecht angenommen"),
        Lang::Fr => ("solution non lue", "trait supposé"),
        Lang::It => ("soluzione non letta", "tratto presunto"),
        Lang::Nl => ("oplossing niet gelezen", "aan zet aangenomen"),
        _ => ("solution not read", "side to move assumed"),
    }
}

/// `Nº 185 · p. 40`.
fn label(number: u32, page: &str) -> String {
    format!("N\u{ba} {number} \u{b7} p. {page}")
}

/// `Nº 185 · p. 40 · White to move`.
fn caption(number: u32, page: &str, side: Color, lang: Lang) -> String {
    let to_move = describe::to_move(side, lang);
    let mut chars = to_move.chars();
    let head: String = chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default();
    format!("{} \u{b7} {head}", label(number, page))
}

/// The key's line of `item`, in `lang`.
fn key_line(item: &Item, page: &str, lang: Lang) -> String {
    let head = label(item.number, page);
    match &item.solution {
        Some(s) => format!("{head}: {s}"),
        None => {
            let (not_read, assumed) = not_read(lang);
            if item.side_assumed { format!("{head}: {not_read} ({assumed})") } else { format!("{head}: {not_read}") }
        }
    }
}

/// The side to move of a full FEN; White when it does not say.
fn side_of(fen: &str) -> Color {
    pdfcraft_chess::fen::Fen::parse(fen).ok().and_then(|f| f.side()).unwrap_or(Color::White)
}

/// The sheet of `doc`'s exercises, `book` naming its pages, as `layout` says.
pub fn worksheet(doc: &ChessDoc, book: &PgnBook, layout: &Layout) -> Worksheet {
    capped(doc, book, layout, MAX_EXERCISES)
}

/// [`worksheet`], with at most `max` exercises on the sheet (the tests' small cap).
pub(crate) fn capped(doc: &ChessDoc, book: &PgnBook, layout: &Layout, max: usize) -> Worksheet {
    let labels = PageLabel::with_folios(&book.labels, doc);
    let (found, left_out) = find(doc, book);
    let mut items: Vec<Item> = Vec::new();
    let mut out = Worksheet::default();
    let page_lang = |page: u32| doc.pages.iter().find(|p| p.index == page).map_or(Lang::En, |p| p.text_language());
    let mut push = |item: Item, out: &mut Worksheet| {
        if items.len() < max {
            items.push(item);
        } else {
            out.over = out.over.saturating_add(1);
            out.over_at.push(item.at);
        }
    };
    let mut solved = found.into_iter().peekable();
    // In the book's order: the exercises with a solution, and those without one in between.
    let unsolved: Vec<LeftOut> = left_out.iter().copied().filter(|l| l.reason == LeftOutReason::NoSolution).collect();
    out.left_out = left_out.into_iter().filter(|l| l.reason == LeftOutReason::NoPosition).collect();
    let mut unsolved = unsolved.into_iter().peekable();
    loop {
        let next_solved = solved.peek().map(|f| f.exercise.at);
        let next_unsolved = unsolved.peek().map(|l| l.at);
        let take_solved = match (next_solved, next_unsolved) {
            (None, None) => break,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (Some(a), Some(b)) => a <= b,
        };
        if take_solved {
            let Some(f) = solved.next() else { break };
            let lang = layout.lang.unwrap_or_else(|| page_lang(f.exercise.at.page_index()));
            let fen = f.score.start_fen.clone().unwrap_or_else(|| cozy_chess::Board::default().to_string());
            let page = PageLabel::of(&labels, f.exercise.at.page_index()).to_string();
            let side = side_of(&fen);
            push(
                Item {
                    at: f.exercise.at,
                    number: f.exercise.number,
                    caption: caption(f.exercise.number, &page, side, lang),
                    caption_en: caption(f.exercise.number, &page, side, Lang::En),
                    solution: Some(super::anki::solution(&f.score)),
                    orientation: f.orientation,
                    // Its solution's first move says who plays.
                    side_assumed: false,
                    lang,
                    fen,
                },
                &mut out,
            );
        } else {
            let Some(l) = unsolved.next() else { break };
            // Its position is the one the diagram shows; none, and it is left out after all.
            let Some((fen, orientation, side_assumed)) = shown(doc, l.at) else {
                out.left_out.push(LeftOut { reason: LeftOutReason::NoPosition, ..l });
                continue;
            };
            let lang = layout.lang.unwrap_or_else(|| page_lang(l.at.page_index()));
            let page = PageLabel::of(&labels, l.at.page_index()).to_string();
            let side = side_of(&fen);
            push(
                Item {
                    at: l.at,
                    number: l.number,
                    caption: caption(l.number, &page, side, lang),
                    caption_en: caption(l.number, &page, side, Lang::En),
                    solution: None,
                    orientation,
                    side_assumed,
                    lang,
                    fen,
                },
                &mut out,
            );
        }
    }
    out.left_out.sort_by_key(|l| l.at);
    if layout.solutions {
        let mut keyed: Vec<&Item> = items.iter().collect();
        keyed.sort_by_key(|i| (i.number, i.at));
        out.solutions = keyed
            .iter()
            .map(|i| {
                let page = PageLabel::of(&labels, i.at.page_index()).to_string();
                (key_line(i, &page, i.lang), key_line(i, &page, Lang::En))
            })
            .collect();
    }
    let per = layout.per_page.count().max(1);
    out.pages = items.chunks(per).map(<[Item]>::to_vec).collect();
    out
}

/// The diagram at `at`: the position it shows (legal), the side it is printed from and whether its
/// side to move was assumed.
fn shown(doc: &ChessDoc, at: DiagramAt) -> Option<(String, Color, bool)> {
    let d = diagram(doc, at)?;
    let s = d.shown()?;
    pdfcraft_chess::fen::board_from_fen(s.fen).ok()?;
    Some((s.fen.to_owned(), d.orientation(), !s.side_known))
}

fn diagram(doc: &ChessDoc, at: DiagramAt) -> Option<&super::DiagramBlock> {
    let page = doc.pages.iter().find(|p| p.index == at.page_index())?;
    page.blocks.iter().find_map(|b| match b {
        Block::Diagram(d) if d.number == at.index => Some(d),
        _ => None,
    })
}
