//! The notation errata list (plan A9-N19, R6 "Recomendado" 1): what a book printed that its own
//! moves do not bear out, move by move, with the page and the move, as CSV or Markdown, for an
//! editor.
//!
//! Nothing here is new reading: each erratum is a fact the walk or the link already kept on the
//! move, said once per issue ([`Issue`]):
//!
//! - **Marks** ([`ReviewFlag::NotationMismatch`], plan A9-L5): a capture mark on a move that takes
//!   nothing, a check or a mate mark on a move that gives none. A mark left out is no erratum:
//!   many books print no check.
//! - **Numbers** ([`MoveStatus::OutOfSequence`], with the number its game's
//!   [`ReviewFlag::OutOfSequence`] says was expected): a numbered move that neither continues its
//!   line nor points back into it.
//! - **The book's PGN** ([`LinkKind::Conflict`], [`super::reference::link`]): on a **born-digital**
//!   page ([`born_digital`]: the text layer was good and read as it is, no word of the move
//!   repaired), a move the reader played to a position the book's PGN does not have, and the PGN
//!   plays from another position. Not on a page read by OCR.
//!
//! Each row has a [`Kind`] (plan A9-N19b): a mark is a [`Kind::Misprint`], since the printed `x`,
//! `+` or `#` disagrees with the position the move was played in, and a number or a conflict with
//! the PGN is a row [`Kind::Check`], since it may be the reading's. On a born-digital page the
//! characters are the book's, but how its lines join is the walk's: on Harikrishna's 60 pages every
//! number out of sequence and every conflict with the PGN was a place where the reading strayed
//! (a line label not attached, a move cited in prose), none a misprint. The rows come misprints
//! first.
//!
//! A move with two issues (`Nxd8+` taking nothing and giving no check) has a row for each. On a
//! page read by OCR a mark may be a misread character: each row says what its page was read from.

use std::fmt::Write as _;

use pdfcraft_chess::fen::Fen;
use pdfcraft_chess::placement::{Color, Piece};

use super::games::GameFlag;
use super::reference::{LinkKind, RefNode, ReferenceIndex};
use super::{ChessDoc, ChessPage, Inline, MoveAt, MoveInline, MoveNumber, MoveStatus, PageLabel, ReviewFlag, TextLayer, UserRect};

/// Most squares a [`Issue::BookPgn`] row names where the two positions differ.
pub const MAX_SQUARES_SAID: usize = 8;

/// What is wrong with a printed move.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Issue {
    /// A capture mark on a move that takes nothing.
    CaptureMark,
    /// A check mark on a move that gives no check.
    CheckMark,
    /// A mate mark on a move that does not mate.
    MateMark,
    /// A move number that neither continues its line nor points back into it.
    MoveNumber,
    /// On a born-digital page, the book's PGN plays the move from another position.
    BookPgn,
}

/// How sure a row is (plan A9-N19b).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    /// The printed mark disagrees with the position the move was played in: the book's.
    Misprint,
    /// Where the reading and the book disagree, which may be the reading's: a number out of
    /// sequence, a conflict with the book's PGN.
    Check,
}

impl Kind {
    pub const ALL: [Kind; 2] = [Kind::Misprint, Kind::Check];

    /// The kind's name in the CSV, in the tools' replies and in `chess_errata`'s `kind` filter.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Misprint => "misprint",
            Kind::Check => "check",
        }
    }

    /// The kind in words, as the Markdown table says it.
    pub fn words(self) -> &'static str {
        match self {
            Kind::Misprint => "misprint",
            Kind::Check => "to check",
        }
    }

    /// The kind named `name` ([`Kind::name`]).
    pub fn from_name(name: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.name() == name)
    }
}

impl Issue {
    pub const ALL: [Issue; 5] = [Issue::CaptureMark, Issue::CheckMark, Issue::MateMark, Issue::MoveNumber, Issue::BookPgn];

    /// The issue's name in the CSV and in the tools' replies.
    pub fn name(self) -> &'static str {
        match self {
            Issue::CaptureMark => "capture_mark",
            Issue::CheckMark => "check_mark",
            Issue::MateMark => "mate_mark",
            Issue::MoveNumber => "move_number",
            Issue::BookPgn => "book_pgn",
        }
    }

    /// The issue in words, as the Markdown table says it.
    pub fn words(self) -> &'static str {
        match self {
            Issue::CaptureMark => "capture mark",
            Issue::CheckMark => "check mark",
            Issue::MateMark => "mate mark",
            Issue::MoveNumber => "move number",
            Issue::BookPgn => "book's PGN",
        }
    }

    /// How sure a row of this issue is: a mark is the book's, a number or a conflict with the PGN
    /// may be the reading's.
    pub fn kind(self) -> Kind {
        match self {
            Issue::CaptureMark | Issue::CheckMark | Issue::MateMark => Kind::Misprint,
            Issue::MoveNumber | Issue::BookPgn => Kind::Check,
        }
    }
}

/// One erratum: a printed move and what is wrong with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Erratum {
    /// Where the move is.
    pub at: MoveAt,
    pub issue: Issue,
    /// The move's number in its game (`4...`): the position after it says it when the move was
    /// played, else the number printed before it; empty when neither does.
    pub number: String,
    /// The move as printed, with the number printed before it (`2.Ng5+`, `Nxd8`).
    pub printed: String,
    /// What it should be: the move as played, with the marks its position gives it (`Nd8`), the
    /// number the line expected (`3.`), or the book's PGN's move with its number (`2.Nf3`; empty
    /// when the PGN is not at hand).
    pub expected: String,
    /// The erratum in words (English, as the engine's notes are).
    pub detail: String,
    /// The page's text was read from a picture of it (OCR), not from its text layer: a mark may be
    /// a misread character.
    pub ocr: bool,
    /// [`Issue::BookPgn`]: the PGN's move.
    pub pgn: Option<RefNode>,
}

/// Whether `page` is born digital as far as its moves go: read from its own text layer, which was
/// judged good, not from a picture of it.
pub fn born_digital(page: &ChessPage) -> bool {
    !page.ocr && page.text_layer == TextLayer::Good
}

/// The document's errata, misprints first, then the rows to check ([`Kind`]), each in reading
/// order, a move's issues in [`Issue::ALL`]'s order.
/// `index` is the book's PGN `doc` was linked to ([`super::reference::link`]), which names the
/// PGN's move of a [`Issue::BookPgn`] row; without it the row says the PGN's game and move only.
/// The page's furniture (its number, the text in a diagram's box, a running head) is passed over,
/// as the exports pass it.
pub fn errata(doc: &ChessDoc, index: Option<&ReferenceIndex>) -> Vec<Erratum> {
    let mut numbers = Sequence::of(doc);
    let mut out = Vec::new();
    for page in &doc.pages {
        for (b, block) in page.book_blocks() {
            for (i, inline) in block.inlines().iter().enumerate() {
                if let Inline::Move(m) = inline {
                    move_errata(page, MoveAt::new(page.index, b, i), m, index, &mut numbers, &mut out);
                }
            }
        }
    }
    // Stable: reading order within each kind.
    out.sort_by_key(|e| e.issue.kind());
    out
}

/// The numbers the walk expected where a move was out of sequence: its games keep them
/// ([`ReviewFlag::OutOfSequence`] in [`super::games::Game::flags`]) by the move's page, box and
/// printed text, each taken once.
struct Sequence<'a> {
    flags: Vec<(&'a GameFlag, bool)>,
}

impl<'a> Sequence<'a> {
    fn of(doc: &'a ChessDoc) -> Sequence<'a> {
        let flags =
            doc.games.iter().flat_map(|g| &g.flags).filter(|f| matches!(f.flag, ReviewFlag::OutOfSequence { .. })).map(|f| (f, false)).collect();
        Sequence { flags }
    }

    /// The number expected where the move printed `printed` in box `rect` of page `page` stood,
    /// when a game kept it.
    fn expected(&mut self, page: u32, rect: Option<UserRect>, printed: &str) -> Option<String> {
        let (flag, used) = self.flags.iter_mut().find(|(f, used)| {
            !*used && f.page == page && (f.source_rect == rect || f.source_rect.is_none() || rect.is_none()) && {
                matches!(&f.flag, ReviewFlag::OutOfSequence { text, .. } if text == printed)
            }
        })?;
        *used = true;
        match &flag.flag {
            ReviewFlag::OutOfSequence { expected, .. } => Some(expected.clone()),
            _ => None,
        }
    }
}

/// The errata of move `m` at `at` of `page`, pushed to `out`.
fn move_errata(page: &ChessPage, at: MoveAt, m: &MoveInline, index: Option<&ReferenceIndex>, numbers: &mut Sequence<'_>, out: &mut Vec<Erratum>) {
    let printed = format!("{}{}", m.number.map(|n| n.text()).unwrap_or_default(), m.text);
    let number = number_in_game(m).map(|n| n.text()).unwrap_or_default();
    let row = |issue: Issue, expected: String, detail: String, pgn: Option<RefNode>| Erratum {
        at,
        issue,
        number: number.clone(),
        printed: printed.clone(),
        expected,
        detail,
        ocr: page.ocr,
        pgn,
    };
    for flag in &m.review {
        if let ReviewFlag::NotationMismatch { capture, check, mate, .. } = flag {
            let san = m.san.as_str();
            if *capture {
                out.push(row(Issue::CaptureMark, san.to_owned(), format!("printed as a capture, but {san} takes nothing"), None));
            }
            if *check {
                out.push(row(Issue::CheckMark, san.to_owned(), format!("printed with a check, but {san} gives none"), None));
            }
            if *mate {
                out.push(row(Issue::MateMark, san.to_owned(), format!("printed as mate, but {san} does not mate"), None));
            }
        }
    }
    if m.status == MoveStatus::OutOfSequence {
        let said = m.number.map_or_else(|| "unnumbered".to_owned(), |n| format!("numbered {}", n.text()));
        let detail =
            |expected: &str| if expected.is_empty() { format!("{said}, out of sequence") } else { format!("{said} where {expected} was expected") };
        let expected = numbers.expected(at.page, m.source_rect, &printed).unwrap_or_default();
        out.push(row(Issue::MoveNumber, expected.clone(), detail(&expected), None));
    }
    if let Some(r) = m.reference.as_ref().filter(|r| r.kind == LinkKind::Conflict)
        && born_digital(page)
        && !repaired(m)
    {
        let node = r.game.zip(r.node).map(|(game, node)| RefNode { game, node });
        let (expected, detail) = pgn_conflict(m, node, index);
        out.push(row(Issue::BookPgn, expected, detail, node));
    }
}

/// Whether the reader rewrote or could not settle the move's word: then its characters are not
/// the page's own, and a conflict may be the reading's.
fn repaired(m: &MoveInline) -> bool {
    m.read_review.iter().chain(&m.review).any(|f| matches!(f, ReviewFlag::TextRepaired { .. } | ReviewFlag::TextUnsettled { .. }))
}

/// The move's number in its game: from the position after it when it was played (the side to move
/// after it is the other), else the number printed before it.
fn number_in_game(m: &MoveInline) -> Option<MoveNumber> {
    let played = m.fen.as_deref().and_then(|f| Fen::parse(f).ok()).and_then(|fen| {
        let full = fen.fullmove_number();
        match fen.side()? {
            Color::Black => u16::try_from(full).ok().map(|number| MoveNumber { number, black: false }),
            Color::White => u16::try_from(full.checked_sub(1)?).ok().filter(|n| *n > 0).map(|number| MoveNumber { number, black: true }),
        }
    });
    played.or(m.number)
}

/// The expected move and the words of a move in conflict with the book's PGN at `node`.
fn pgn_conflict(m: &MoveInline, node: Option<RefNode>, index: Option<&ReferenceIndex>) -> (String, String) {
    let Some(node) = node else {
        return (String::new(), "the book's PGN plays this move from another position".to_owned());
    };
    let game = node.game.saturating_add(1);
    let Some(index) = index else {
        return (String::new(), format!("the book's PGN (game {game}, move {}) plays this move from another position", node.node));
    };
    let pgn_fen = index.fen(node);
    let san = index.san(node).unwrap_or_default();
    // The PGN's number: the position after its move says it, as for the book's.
    let numbered = match pgn_fen.as_ref().and_then(|f| Some((f.side()?, f.fullmove_number()))) {
        Some((Color::Black, full)) => format!("{full}.{san}"),
        Some((Color::White, full)) if full > 1 => format!("{}...{san}", full.saturating_sub(1)),
        _ => san.to_owned(),
    };
    let name = index.games().get(node.game).map(|g| g.name.as_str()).filter(|n| !n.is_empty());
    let named = name.map_or_else(|| format!("game {game}"), |n| format!("game {game}, {n}"));
    let mut detail = format!("the book's PGN ({named}) plays {numbered} from another position");
    let book = m.fen.as_deref().and_then(|f| Fen::parse(f).ok());
    if let (Some(book), Some(pgn)) = (book, pgn_fen) {
        let (a, b) = (book.placement().reading_order(), pgn.placement().reading_order());
        let differ: Vec<String> = a
            .iter()
            .zip(b.iter())
            .enumerate()
            .filter(|(_, (x, y))| x != y)
            .map(|(k, (x, y))| format!("{} ({} here, {} in the PGN)", super::reference::square_name(k), piece(*x), piece(*y)))
            .collect();
        if !differ.is_empty() {
            let more = differ.len().saturating_sub(MAX_SQUARES_SAID);
            let said: Vec<&str> = differ.iter().take(MAX_SQUARES_SAID).map(String::as_str).collect();
            let _ = write!(detail, "; the positions after it differ on {}", said.join(", "));
            if more > 0 {
                let _ = write!(detail, " and {more} more");
            }
        }
    }
    (numbered, detail)
}

/// A square's piece as a FEN letter, `-` for none.
fn piece(p: Option<Piece>) -> char {
    p.map_or('-', Piece::fen_char)
}

/// A field of [`to_csv`]: quoted when it holds a comma, a quote or a line break (RFC 4180), and
/// begun with `'` when a spreadsheet would take it for a formula (`=`, `+`, `-`, `@`, a tab or a
/// carriage return first; the text is a PDF's, anyone's).
fn csv_field(text: &str) -> String {
    let mut text = text.to_owned();
    if text.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        text.insert(0, '\'');
    }
    if text.contains([',', '"', '\n', '\r']) { format!("\"{}\"", text.replace('"', "\"\"")) } else { text }
}

/// The page of `e` as the book numbers it, by `labels` ([`PageLabel::of`]).
fn label(labels: &[String], e: &Erratum) -> String {
    PageLabel::of(labels, e.at.page).to_string()
}

/// `errata` as CSV (RFC 4180: a header, then a row each, lines ended by CR LF): `page` (the PDF's
/// page, from 1), `page_label` (as the book numbers it, `labels` by page index), `move` (the
/// move's number in its game), `printed`, `issue` ([`Issue::name`]), `expected`, `detail`,
/// `read_from` (`text` or `ocr`) and `kind` ([`Kind::name`]; last, as plan A9-N19b added it).
pub fn to_csv(errata: &[Erratum], labels: &[String]) -> String {
    let mut out = String::from("page,page_label,move,printed,issue,expected,detail,read_from,kind\r\n");
    for e in errata {
        let fields = [
            u64::from(e.at.page).saturating_add(1).to_string(),
            label(labels, e),
            e.number.clone(),
            e.printed.clone(),
            e.issue.name().to_owned(),
            e.expected.clone(),
            e.detail.clone(),
            (if e.ocr { "ocr" } else { "text" }).to_owned(),
            e.issue.kind().name().to_owned(),
        ];
        let row: Vec<String> = fields.iter().map(|f| csv_field(f)).collect();
        out.push_str(&row.join(","));
        out.push_str("\r\n");
    }
    out
}

/// `errata` as a Markdown document titled `title`: a table of the same columns as [`to_csv`], the
/// issue in words, its cells escaped so they stay text; a line saying there are none when there
/// are none.
pub fn to_markdown(errata: &[Erratum], labels: &[String], title: &str) -> String {
    let esc = super::markdown::esc;
    let mut out = format!("# Notation errata: {}\n\n", esc(title));
    if errata.is_empty() {
        out.push_str("No errata found.\n");
        return out;
    }
    out.push_str("| Page | Printed page | Move | Printed | Issue | Should be | Detail | Read from | Kind |\n");
    out.push_str("|---:|---|---|---|---|---|---|---|---|\n");
    for e in errata {
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            u64::from(e.at.page).saturating_add(1),
            esc(&label(labels, e)),
            esc(&e.number),
            esc(&e.printed),
            e.issue.words(),
            esc(&e.expected),
            esc(&e.detail),
            if e.ocr { "OCR" } else { "text layer" },
            e.issue.kind().words(),
        );
    }
    out
}
