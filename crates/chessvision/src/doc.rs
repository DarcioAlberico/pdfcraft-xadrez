//! ChessDoc: the recognised chess document that feeds the text view and every export (blueprint
//! S14).
//!
//! A [`ChessDoc`] is a document's pages as blocks ([`Block::Heading`], [`Block::Paragraph`],
//! [`Block::Diagram`]) of inlines ([`Inline::Text`], [`Inline::Move`], [`Inline::Nag`],
//! [`Inline::Link`]), plus the games found in its text ([`games`]), exported as PGN ([`pgn`]).
//!
//! - **Where everything came from.** Every block and inline has a `source_rect` in PDF **user
//!   space** ([`UserRect`]), so the view can map recognised text back to the page. It is `None`
//!   only when the box was not a finite rectangle.
//! - **Diagrams in the one order.** A page's diagrams are taken in the order they are given, which
//!   must be [`crate::order::reading_order`]'s (the order [`crate::board::read`] returns them in when
//!   given the candidates in that order). [`DiagramBlock::number`] is the position in it, so
//!   "diagram n" is the n-th [`Block::Diagram`] of the page, here, on screen and in every export.
//! - **Nothing is silently fixed.** Whatever needs a person goes in a [`ReviewFlag`]: the gate's
//!   verdict, repaired squares and uncertain squares of a reading (S8), the repairs and ambiguities
//!   of the text (S13c, passed in as [`TextReview`]s), and every move the game replay could not
//!   accept ([`games`]). A move that fails replay stays in the text with its [`MoveStatus`].
//! - **Plain inputs.** [`PageInput`] takes a [`ChessPageText`] (from the text layer or from OCR),
//!   the page's [`BoardReading`]s, its [`PageGeometry`], and optional [`BlockHint`]s (headings and
//!   paragraphs, which the engine takes from `edit::reading_blocks`: `chessvision` is L4 like
//!   `edit` and cannot depend on it). Pages go through one [`DocBuilder`], because a game can run
//!   on from one page to the next.
//!
//! - **What was read, and what the walk made of it** (plan A9-L0a, R3-01). A move keeps what the
//!   reader read ([`MoveInline::san_read`], [`MoveInline::read_review`], its number, text, weight
//!   and box), and the walk writes what it decided beside it (its status, SAN, position, node and
//!   flags), never over it: walking pages again ([`walk`], [`DocBuilder::add_read_page`]) starts
//!   from the reading, so it gives what a new reading would. [`READER_VERSION`] numbers the
//!   reading's segmentation.
//!
//! Everything is serde JSON ([`ChessDoc::to_json`], [`ChessDoc::from_json`]). The shape is
//! versioned by [`FORMAT_VERSION`].
//!
//! The data model takes ideas from the Caïssa IR and `export/fidelity.py` (the author's own code,
//! relicensed MIT/Apache, `docs/chess/provenance.md`); the code is written here.

mod build;
pub mod fragment;
pub mod games;
mod header;
pub mod html;
pub mod markdown;
pub mod pgn;
pub mod reference;

#[cfg(test)]
mod tests;

use pdfcraft_chess::rules::Legality;
use serde::{Deserialize, Serialize};

use crate::board::{BoardReading, ReadPosition, ReadingSource};
use crate::chess_text::{ChessPageText, TextQuality};
use crate::cipher::{WordEdit, WordFlag};
use crate::coords::{PageGeometry, PageRect, ViewRect, pixels_to_view};

pub use games::{Game, GameOrigin, GameResult, GameScore, Node, NodeRef, Roster, Tag, WalkRules};

/// The version of the JSON shape. Bumped when a field changes meaning or is removed.
pub const FORMAT_VERSION: u32 = 1;

/// The version of the reading's segmentation (plan A9 §2.10): how a page is cut into blocks and
/// inlines, which [`MoveAt`] addresses. Every change that moves a block or an inline raises it,
/// so an address kept by an earlier reading (a decision on a move, plan A9-L1b) is known to be
/// of another segmentation and is matched again rather than trusted. 2 (plan A9-L1a): the lines
/// inside a diagram's box are a block of their own ([`BlockRole::DiagramText`]), never joined with
/// lines outside it.
pub const READER_VERSION: u32 = 2;

/// Largest JSON text [`ChessDoc::from_json`] reads.
pub const MAX_JSON_BYTES: usize = 64 << 20;

/// A rectangle in PDF user space (points, y up), always finite.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct UserRect {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl UserRect {
    /// The rectangle, normalised, or `None` when a coordinate is not finite.
    pub fn new(rect: PageRect) -> Option<UserRect> {
        let r = PageRect::new(rect.x0, rect.y0, rect.x1, rect.y1);
        [r.x0, r.y0, r.x1, r.y1].iter().all(|v| v.is_finite()).then_some(UserRect { x0: r.x0, y0: r.y0, x1: r.x1, y1: r.y1 })
    }

    pub fn page_rect(&self) -> PageRect {
        PageRect::new(self.x0, self.y0, self.x1, self.y1)
    }

    /// The smallest rectangle holding both.
    pub fn union(self, other: UserRect) -> UserRect {
        UserRect { x0: self.x0.min(other.x0), y0: self.y0.min(other.y0), x1: self.x1.max(other.x1), y1: self.y1.max(other.y1) }
    }

    /// The area both cover (0 when they do not meet).
    pub fn overlap(&self, other: &UserRect) -> f32 {
        let w = (self.x1.min(other.x1) - self.x0.max(other.x0)).max(0.0);
        let h = (self.y1.min(other.y1) - self.y0.max(other.y0)).max(0.0);
        w * h
    }
}

/// A number that is written to JSON: NaN and the infinities become 0, as JSON cannot hold them.
pub(crate) fn finite(v: f32) -> f32 {
    if v.is_finite() { v } else { 0.0 }
}

/// Something a person should look at. Each says what was found, never what was guessed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "flag", rename_all = "snake_case")]
pub enum ReviewFlag {
    // --- Diagram readings (S8) ---
    /// The board could not be read.
    Unreadable { reason: String },
    /// The reading did not pass the gate (legal, weakest square at least 0.80): it is shown, but
    /// not exported as certain, and games starting from it are not trusted.
    NotAccepted { min_confidence: f32 },
    /// The position is not legal with the side to move it was given.
    Illegal { violations: Vec<String> },
    /// The decoder changed squares from their most probable reading to make the position legal.
    RepairedSquares { changes: Vec<SquareChange> },
    /// Squares at or below the uncertainty threshold, with their second reading.
    UncertainSquares { squares: Vec<UncertainSquare> },
    /// Nothing said who is to move: White was assumed.
    SideAssumed,
    /// The pieces say the board was printed from Black's side (no printed coordinates were read):
    /// its squares were turned half a circle. A guess to check.
    PrintedFromBlack,
    /// What the page said about the side to move and what the position allows disagree.
    SideConflict,
    /// Moves follow the diagram but its reading cannot be replayed from (unreadable or illegal),
    /// so they were not checked.
    NoStartPosition,
    // --- Text (S12, S13c) ---
    /// A word the cipher or the legality repair rewrote.
    TextRepaired { from: String, to: String, why: String, confidence: f32 },
    /// A word the repair could not settle: several legal moves fit (`ambiguous`), or none did.
    TextUnsettled { text: String, ambiguous: bool, candidates: Vec<String> },
    /// A font that looks like a chess font but is not catalogued: its glyphs are as extracted.
    UnknownChessFont { font: String },
    /// The text layer is not to be believed (read the page with OCR instead).
    PoorTextLayer { reason: String },
    // --- Games ---
    /// A move that is not legal where it stands. The line stops there.
    IllegalMove { text: String, near: Vec<String> },
    /// A move several legal moves fit. Not chosen; the line stops there.
    AmbiguousMove { text: String, candidates: Vec<String> },
    /// A numbered move that neither continues its line nor points back into it. The line stops.
    OutOfSequence { text: String, expected: String },
    /// A numbered move that only a line outside the variation it was read in can take: the
    /// variation's closing bracket was assumed (OCR loses brackets), and the move played there.
    ImpliedClose { text: String },
    /// The game starts from a diagram reading that did not pass the gate.
    UntrustedStart { page: u32, diagram: usize },
    /// The diagram's side to move was assumed, and the first move's number said the other side.
    SideFromMoveNumber,
    /// The first move is not legal from the diagram before it, but is from another diagram
    /// nearby (two diagrams printed side by side, their texts after both): the game starts there.
    DiagramMatchedByMove,
    /// A diagram stands between these moves, but the next move continues this game and is not
    /// legal from the diagram, so the game was carried on past it.
    ContinuedPastDiagram { page: u32, diagram: usize },
    /// The game reached [`games::MAX_GAME_NODES`] moves or [`games::MAX_VARIATION_DEPTH`]
    /// nested variations; the rest of the line was not added.
    GameTooLarge,
    /// A comment reached [`games::MAX_COMMENT_BYTES`]; the PGN comment is cut there (the text in
    /// the blocks is whole).
    CommentTruncated,
    // --- The book's own PGN (plan "Carregar PGN do livro", R2) ---
    /// The reader could not play the move; the book's PGN plays it as `san` ([`reference::link`]).
    /// The reader's status is kept.
    FromReference { san: String },
}

/// A square the decoder changed: `from` was the most probable reading, `to` what it chose.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SquareChange {
    pub square: String,
    /// FEN piece letters; `None` for an empty square.
    pub from: Option<char>,
    pub to: Option<char>,
}

/// A square read with low confidence, and its second reading.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UncertainSquare {
    pub square: String,
    pub confidence: f32,
    pub alternative: Option<char>,
    pub alternative_confidence: f32,
}

/// What a page's text layer is worth.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextLayer {
    Good,
    Poor,
    Missing,
}

/// The whole document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChessDoc {
    /// [`FORMAT_VERSION`] when written.
    pub format: u32,
    pub pages: Vec<ChessPage>,
    /// Every game found, in the order they start. [`NodeRef::game`] indexes this.
    pub games: Vec<Game>,
}

/// One page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChessPage {
    /// The page's index in the document, from 0.
    pub index: u32,
    /// The piece-letter language of the page's moves (BCP 47 code).
    pub lang: String,
    pub text_layer: TextLayer,
    /// The text was read from a picture of the page ([`PageInput::ocr`]).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ocr: bool,
    pub blocks: Vec<Block>,
    /// Flags that belong to the page rather than to one block.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub review: Vec<ReviewFlag>,
    /// The puzzle label captioned at the page's foot, after its last diagram, which numbers the
    /// next page's first diagram (Construa Seu Xadrez sets `Exercício 7` at the foot of a page
    /// and its diagram at the top of the next). Kept so that a page read alone and walked again
    /// with the page after it ([`DocBuilder::add_read_page`]) still passes it on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trailing: Option<u32>,
}

/// Where a move of the text is (plan A9-T2a, R3-09): its page (0-based), its block on the page and
/// its place among the block's inlines, both from 0. The one address of a move read: the
/// alignment with the book's PGN ([`reference::MoveLink`]), the Text tab and the game read across
/// pages (`engine::chess::span`) all use it. Written as `{"page", "block", "inline"}`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MoveAt {
    pub page: u32,
    pub block: u32,
    pub inline: u32,
}

impl MoveAt {
    /// Inline `inline` of block `block` on page `page`. A place past `u32::MAX`, which no page
    /// holds, is kept as `u32::MAX`: it names nothing.
    pub fn new(page: u32, block: usize, inline: usize) -> MoveAt {
        let to = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        MoveAt { page, block: to(block), inline: to(inline) }
    }

    /// The inline at this address in `doc`, when the page was read and has it.
    pub fn inline(self, doc: &ChessDoc) -> Option<&Inline> {
        let page = doc.pages.iter().find(|p| p.index == self.page)?;
        page.blocks.get(usize::try_from(self.block).ok()?)?.inlines().get(usize::try_from(self.inline).ok()?)
    }

    /// The move at this address in `doc`, when there is one.
    pub fn find(self, doc: &ChessDoc) -> Option<&MoveInline> {
        match self.inline(doc)? {
            Inline::Move(m) => Some(m),
            _ => None,
        }
    }
}

/// What a page is called (plan A9-T2a, R3-08; glossary "Página (física) · rótulo"): the label
/// printed on it when the document gives one (`/PageLabels`), and its index. Every export says a
/// page by [`PageLabel`]'s one rule, so a diagram's text and an export's notes name it alike.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageLabel {
    /// The printed label, trimmed; `None` when the document gives none, only blanks, or the
    /// page's own number from 1 (a viewer shows that for a page with no label).
    pub printed: Option<String>,
    /// The page's index, from 0.
    pub physical: u32,
}

impl PageLabel {
    /// Page `physical`, labelled `label` (`None`, or a blank label, when it has none).
    pub fn new(label: Option<&str>, physical: u32) -> PageLabel {
        let number = u64::from(physical).saturating_add(1).to_string();
        let printed = label.map(str::trim).filter(|l| !l.is_empty() && *l != number).map(str::to_owned);
        PageLabel { printed, physical }
    }

    /// Page `physical` of a document whose pages are labelled `labels`, by index (empty when it
    /// labels none).
    pub fn of(labels: &[String], physical: u32) -> PageLabel {
        PageLabel::new(usize::try_from(physical).ok().and_then(|i| labels.get(i)).map(String::as_str), physical)
    }

    /// The page counted from 1.
    pub fn number(&self) -> u64 {
        u64::from(self.physical).saturating_add(1)
    }
}

/// The page as the book numbers it: its printed label, else its own number from 1.
impl std::fmt::Display for PageLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.printed {
            Some(printed) => f.write_str(printed),
            None => write!(f, "{}", self.number()),
        }
    }
}

/// A block of the page, in reading order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Heading {
        level: u8,
        inlines: Vec<Inline>,
        source_rect: Option<UserRect>,
        /// What the block is to the book ([`BlockRole`]); the book's text when not written.
        #[serde(default, skip_serializing_if = "BlockRole::is_content")]
        role: BlockRole,
    },
    Paragraph {
        inlines: Vec<Inline>,
        source_rect: Option<UserRect>,
        #[serde(default, skip_serializing_if = "BlockRole::is_content")]
        role: BlockRole,
    },
    Diagram(DiagramBlock),
}

/// What a text block is to the book (plan A9-L1, §2.6; A2-04, A2-06), decided from its page alone
/// when the page is built or walked, so a page reads the same whatever was read before it. Only
/// [`BlockRole::Content`] is the book's text; the others are the page's furniture, which the walk
/// passes over (a game's header gives the next game its tags). A running head is not a role: it
/// needs the pages before it. Written in the document's JSON as `"role"` (`"folio"`,
/// `"game_header"`, `"diagram_text"`), not at all for content, so a document with none reads as
/// before.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockRole {
    /// The book's text.
    #[default]
    Content,
    /// The page's printed number: a bare number at the head or foot of the page.
    Folio,
    /// A game's header: its players' line, and its event's under it ([`games::Game`] takes them as
    /// tags).
    GameHeader,
    /// Text whose lines lie inside a diagram's box: its coordinates, a caption set in it, the
    /// glyphs of a diagram font the catalogue does not know.
    DiagramText,
}

impl BlockRole {
    /// Whether this is the book's text.
    pub fn is_content(&self) -> bool {
        *self == BlockRole::Content
    }
}

impl Block {
    pub fn source_rect(&self) -> Option<UserRect> {
        match self {
            Block::Heading { source_rect, .. } | Block::Paragraph { source_rect, .. } => *source_rect,
            Block::Diagram(d) => d.source_rect,
        }
    }

    /// The text blocks' inlines (empty for a diagram).
    pub fn inlines(&self) -> &[Inline] {
        match self {
            Block::Heading { inlines, .. } | Block::Paragraph { inlines, .. } => inlines,
            Block::Diagram(_) => &[],
        }
    }

    /// A text block's role ([`BlockRole`]); `None` for a diagram.
    pub fn role(&self) -> Option<BlockRole> {
        match self {
            Block::Heading { role, .. } | Block::Paragraph { role, .. } => Some(*role),
            Block::Diagram(_) => None,
        }
    }

    /// Give a text block `role`; a diagram has none.
    pub(crate) fn set_role(&mut self, to: BlockRole) {
        if let Block::Heading { role, .. } | Block::Paragraph { role, .. } = self {
            *role = to;
        }
    }
}

/// A move number printed before a move: `12.` (`black: false`) or `12...`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoveNumber {
    pub number: u16,
    pub black: bool,
}

impl MoveNumber {
    pub fn text(&self) -> String {
        format!("{}{}", self.number, if self.black { "..." } else { "." })
    }
}

/// What the game replay made of a move.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MoveStatus {
    /// Legal where it stands; it is a node of a game.
    Valid,
    /// Not legal where it stands. Flagged; its line stops.
    Illegal,
    /// Several legal moves fit. Flagged, not chosen; its line stops.
    Ambiguous,
    /// Its number does not fit its line. Flagged; its line stops.
    OutOfSequence,
    /// After its line stopped: not judged.
    NotReached,
    /// No position to judge it from: a move in prose with no game around it, in a heading, or
    /// after a diagram that could not be read.
    Unchecked,
    /// The move a "position after" caption names (`Position after: 11...Ba6`): the diagram
    /// before it shows it played, so it is not played again; it gives the diagram its side to
    /// move and number.
    Caption,
}

/// One piece of a text block.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Inline {
    /// Prose, punctuation, and figurines outside moves (as Unicode chess glyphs).
    Text {
        text: String,
        source_rect: Option<UserRect>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        review: Vec<ReviewFlag>,
    },
    Move(MoveInline),
    /// An annotation glyph, as its PGN code.
    Nag {
        code: u16,
        source_rect: Option<UserRect>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        review: Vec<ReviewFlag>,
    },
    /// A web address in the text.
    Link {
        url: String,
        text: String,
        source_rect: Option<UserRect>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        review: Vec<ReviewFlag>,
    },
}

/// A move in the text.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MoveInline {
    /// The number printed before it, when there was one.
    pub number: Option<MoveNumber>,
    /// The move as printed (figurine-font letters already shown as pieces, check marks included).
    pub text: String,
    /// The English SAN as read: the reading, which the walk never writes (plan A9-L0a). Empty in a
    /// document written before it was kept: [`MoveInline::read_san`] is then [`MoveInline::san`].
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub san_read: String,
    /// English SAN: canonical (disambiguation and check marks as the position requires) when
    /// [`MoveStatus::Valid`], else as read. The walk's.
    pub san: String,
    pub status: MoveStatus,
    /// The position after the move, when valid.
    pub fen: Option<String>,
    /// The move's node in [`ChessDoc::games`], when valid.
    pub game: Option<NodeRef>,
    pub source_rect: Option<UserRect>,
    /// Printed in bold: books set their main line in bold and the analysis around it in regular
    /// type. Only a text layer knows; OCR text and documents read before it was kept say `false`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bold: bool,
    /// The reader's flags on the move (a word its repair changed or could not settle): the
    /// reading, which the walk never writes (plan A9-L0a).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub read_review: Vec<ReviewFlag>,
    /// Every flag on the move: the reader's ([`MoveInline::read_review`]), then the walk's, then
    /// the link's ([`ReviewFlag::FromReference`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub review: Vec<ReviewFlag>,
    /// Where the book's own PGN has it, when the document was linked to one
    /// ([`reference::link`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<reference::MoveReference>,
}

impl MoveInline {
    /// The English SAN as read ([`MoveInline::san_read`]; [`MoveInline::san`] in a document
    /// written before the reading was kept apart).
    pub fn read_san(&self) -> &str {
        if self.san_read.is_empty() { &self.san } else { &self.san_read }
    }

    /// A move of a document written before the reading was kept apart (plan A9-L0a) takes its
    /// reading from what it has: its SAN and its flags (the link's aside). That is what walking it
    /// again started from then, so such a document walks as it did; one read since walks from the
    /// reading alone.
    fn keep_reading(&mut self) {
        if self.san_read.is_empty() {
            self.san_read = self.san.clone();
            self.read_review = self.review.iter().filter(|f| !matches!(f, ReviewFlag::FromReference { .. })).cloned().collect();
        }
    }
}

impl Inline {
    pub fn source_rect(&self) -> Option<UserRect> {
        match self {
            Inline::Text { source_rect, .. } | Inline::Nag { source_rect, .. } | Inline::Link { source_rect, .. } => *source_rect,
            Inline::Move(m) => m.source_rect,
        }
    }

    pub fn review(&self) -> &[ReviewFlag] {
        match self {
            Inline::Text { review, .. } | Inline::Nag { review, .. } | Inline::Link { review, .. } => review,
            Inline::Move(m) => &m.review,
        }
    }

    /// Where the reader's flags go: a move's reading ([`MoveInline::read_review`], which the walk
    /// copies into its flags), any other inline's flags.
    fn read_review_mut(&mut self) -> &mut Vec<ReviewFlag> {
        match self {
            Inline::Text { review, .. } | Inline::Nag { review, .. } | Inline::Link { review, .. } => review,
            Inline::Move(m) => &mut m.read_review,
        }
    }

    /// The inline as it reads: a move with its number, a NAG as its glyph.
    pub fn text(&self) -> String {
        match self {
            Inline::Text { text, .. } | Inline::Link { text, .. } => text.clone(),
            Inline::Move(m) => format!("{}{}", m.number.map(|n| n.text()).unwrap_or_default(), m.text),
            Inline::Nag { code, .. } => pdfcraft_chess::nag::shown(*code),
        }
    }
}

/// Inlines joined as they read: one space between them, none after an opening bracket or before
/// closing punctuation, and annotation glyphs glued to the move before them.
pub fn plain_text(inlines: &[Inline]) -> String {
    let mut out = String::new();
    for inline in inlines {
        let t = inline.text();
        let glued = matches!(inline, Inline::Nag { .. }) || t.starts_with([')', ']', ',', '.', ';', ':', '!', '?']);
        if !out.is_empty() && !glued && !out.ends_with(['(', '[', ' ']) {
            out.push(' ');
        }
        out.push_str(&t);
    }
    out
}

/// Where a diagram's position came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagramSource {
    /// The piece model read the squares.
    Model,
    /// The diagram font's characters were read; exact.
    Font,
    /// A person set the position (a correction kept for the document).
    User,
}

/// A diagram and its reading.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DiagramBlock {
    /// Its position in the page's reading order, from 0: "diagram n" everywhere.
    pub number: usize,
    /// Its index among the page's merged candidates ([`BoardReading::index`]).
    pub index: usize,
    pub source_rect: Option<UserRect>,
    pub source: DiagramSource,
    /// The resolution the board was read at.
    pub dpi: f32,
    /// The position, when the board could be read.
    pub reading: Option<DiagramReading>,
    /// The lichess analysis link: only for a position legal with its side to move and not
    /// repaired ([`ReadPosition::lichess_url`]).
    pub lichess_url: Option<String>,
    /// The puzzle number its caption gives (`№185. Teichmann – Lasker`, above or below it): a
    /// solution that starts with that number, however far on, is played from this diagram.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub puzzle: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub review: Vec<ReviewFlag>,
    /// The position of the book's PGN it shows or nearly shows ([`reference::link`], R3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<Box<reference::DiagramReference>>,
}

/// What was read from a diagram.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DiagramReading {
    /// The full FEN as read (side to move as decided, castling inferred).
    pub fen: String,
    /// Legal with its side to move.
    pub legal: bool,
    /// Passed the gate: may be exported without review.
    pub accepted: bool,
    /// The weakest square's confidence (1 for a font diagram).
    pub min_confidence: f32,
    /// The decoder changed squares to make it legal.
    pub repaired: bool,
    /// Nothing said who is to move; White was assumed.
    pub side_assumed: bool,
}

impl DiagramBlock {
    fn from_reading(number: usize, reading: &BoardReading) -> DiagramBlock {
        let source = match reading.source {
            ReadingSource::Model => DiagramSource::Model,
            ReadingSource::Font => DiagramSource::Font,
            ReadingSource::User => DiagramSource::User,
        };
        let mut block = DiagramBlock {
            number,
            index: reading.index,
            source_rect: UserRect::new(reading.bbox_user),
            source,
            dpi: finite(reading.dpi),
            reading: None,
            lichess_url: None,
            puzzle: None,
            reference: None,
            review: Vec::new(),
        };
        match &reading.outcome {
            Err(why) => block.review.push(ReviewFlag::Unreadable { reason: why.to_string() }),
            Ok(p) => {
                let legal = p.legality.allows(p.side.color);
                block.reading = Some(DiagramReading {
                    fen: p.fen.clone(),
                    legal,
                    accepted: p.accepted,
                    min_confidence: finite(p.min_confidence),
                    repaired: p.repaired(),
                    side_assumed: p.side.is_assumed(),
                });
                block.lichess_url = p.lichess_url();
                block.review = position_flags(p, legal);
            }
        }
        block
    }
}

/// The review flags of a read position.
fn position_flags(p: &ReadPosition, legal: bool) -> Vec<ReviewFlag> {
    let mut flags = Vec::new();
    if !p.accepted {
        flags.push(ReviewFlag::NotAccepted { min_confidence: finite(p.min_confidence) });
    }
    if !legal {
        let violations = match &p.legality {
            Legality::Legal => Vec::new(),
            Legality::OnlyWithSideToMove { other, .. } => other.iter().map(|v| format!("{v:?}")).collect(),
            Legality::Illegal(v) => v.iter().map(|v| format!("{v:?}")).collect(),
        };
        flags.push(ReviewFlag::Illegal { violations });
    }
    if p.repaired() {
        let changes = p
            .repairs
            .iter()
            .map(|c| SquareChange { square: c.square.to_string(), from: c.from.map(|x| x.fen_char()), to: c.to.map(|x| x.fen_char()) })
            .collect();
        flags.push(ReviewFlag::RepairedSquares { changes });
    }
    if !p.uncertain.is_empty() {
        let squares = p
            .uncertain
            .iter()
            .map(|u| UncertainSquare {
                square: u.square.to_string(),
                confidence: finite(u.confidence),
                alternative: u.alternative.map(|x| x.fen_char()),
                alternative_confidence: finite(u.alternative_confidence),
            })
            .collect();
        flags.push(ReviewFlag::UncertainSquares { squares });
    }
    if p.orientation.is_some_and(|v| v.black_view_from_pieces) {
        flags.push(ReviewFlag::PrintedFromBlack);
    }
    if p.side.is_assumed() {
        flags.push(ReviewFlag::SideAssumed);
    }
    if p.side.conflicting {
        flags.push(ReviewFlag::SideConflict);
    }
    flags
}

/// What kind of block a hint marks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HintKind {
    /// A heading of this level (1 is the top).
    Heading {
        level: u8,
    },
    Paragraph,
}

/// A block the page's layout already knows about (`edit::reading_blocks`, adapted by the
/// engine): the lines whose centre lies in `rect` form one block of this kind.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockHint {
    pub kind: HintKind,
    /// User space.
    pub rect: PageRect,
}

/// A review flag over a region of the text (from the S13c repair).
#[derive(Clone, Debug, PartialEq)]
pub struct TextReview {
    /// View space.
    pub rect: ViewRect,
    pub flag: ReviewFlag,
}

impl TextReview {
    /// A word the repair rewrote, read from a page raster at `dpi`. `None` for a bad box or DPI.
    pub fn from_edit(edit: &WordEdit, dpi: f32) -> Option<TextReview> {
        let flag =
            ReviewFlag::TextRepaired { from: edit.from.clone(), to: edit.to.clone(), why: edit.why.clone(), confidence: finite(edit.confidence) };
        Some(TextReview { rect: pixels_to_view(edit.rect, dpi)?, flag })
    }

    /// A word the repair could not settle, read from a page raster at `dpi`.
    pub fn from_flag(flag: &WordFlag, dpi: f32) -> Option<TextReview> {
        let candidates = flag.candidates.iter().map(|s| s.to_english()).collect();
        let review = ReviewFlag::TextUnsettled { text: flag.text.clone(), ambiguous: flag.ambiguous, candidates };
        Some(TextReview { rect: pixels_to_view(flag.rect, dpi)?, flag: review })
    }
}

/// One page's inputs.
#[derive(Clone, Copy, Debug)]
pub struct PageInput<'a> {
    /// The page's index in the document, from 0.
    pub index: u32,
    pub geometry: PageGeometry,
    /// The page's chess text, from the text layer (`chess_text::from_page_text`) or from OCR
    /// (`glyphs::fuse::to_chess_text`), after the S13c repair when there was one.
    pub text: &'a ChessPageText,
    /// The page's diagrams with their readings, **in reading order** ([`crate::order`]).
    pub diagrams: &'a [BoardReading],
    /// Headings and paragraphs the layout knows about. May be empty: lines are then grouped
    /// into paragraphs by their spacing, and there are no headings.
    pub hints: &'a [BlockHint],
    /// The text repair's edits and flags ([`TextReview::from_edit`], [`TextReview::from_flag`]).
    pub reviews: &'a [TextReview],
    /// The text was read from a picture of the page, by the chess OCR or by the program whose
    /// invisible layer it is, so its letters can be misread ([`WalkRules::misread_files`]).
    pub ocr: bool,
}

/// Builds a [`ChessDoc`] page by page, carrying open games across page breaks.
#[derive(Debug, Default)]
pub struct DocBuilder {
    pages: Vec<ChessPage>,
    walker: games::Walker,
    /// The last page added (its index) and the caption label it left for the page after it.
    carry: Option<(u32, u32)>,
    /// The offsets of the page numbers from the pages' indices seen so far, newest last.
    folios: Vec<i64>,
    /// The last pages' headings, to pass over running heads.
    heads: build::RunningHeads,
}

impl DocBuilder {
    pub fn new() -> DocBuilder {
        DocBuilder::default()
    }

    /// A builder whose game walker reads with `rules` (see [`WalkRules`]; [`DocBuilder::new`]
    /// uses [`WalkRules::BOOK`]).
    pub fn with_rules(rules: WalkRules) -> DocBuilder {
        DocBuilder { walker: games::Walker::with_rules(rules), ..DocBuilder::default() }
    }

    /// Add the next page. Pages are taken in the order added.
    pub fn add_page(&mut self, input: &PageInput<'_>) {
        // A caption at the foot of a page is the next page's, not a later one's.
        let carried = self.carry.take().filter(|(index, _)| index.checked_add(1) == Some(input.index)).map(|(_, n)| n);
        let (page, trailing, folios) = build::page(input, &mut self.walker, &mut self.heads, carried, &self.folios);
        build::remember_folios(&mut self.folios, &folios);
        self.carry = trailing.map(|n| (input.index, n));
        self.pages.push(page);
    }

    /// Add a page read before (from a document's JSON, or after its moves were corrected): its
    /// moves are judged again, in order with the other pages, from what was read (plan A9-L0a):
    /// the walk writes every move's status, SAN, position, node and flags afresh from its reading,
    /// so nothing the last walk wrote is read back, and the page walks as a new reading of it
    /// would. The diagrams' missing-start flags, the walk's, are made again too.
    ///
    /// A page read alone did not see the page before it: when the page added just before it is
    /// the one before it in the book and left a caption at its foot ([`ChessPage::trailing`]),
    /// the page's first diagram takes that number unless a caption of its own numbers it, as the
    /// whole-document reader gives it ([`DocBuilder::add_page`]).
    pub fn add_read_page(&mut self, mut page: ChessPage) {
        let carried = self.carry.take().filter(|(index, _)| index.checked_add(1) == Some(page.index)).map(|(_, n)| n);
        if let Some(Block::Diagram(first)) = page.blocks.iter_mut().find(|b| matches!(b, Block::Diagram(_)))
            && first.puzzle.is_none()
        {
            first.puzzle = carried;
        }
        for block in &mut page.blocks {
            match block {
                Block::Heading { inlines, .. } | Block::Paragraph { inlines, .. } => {
                    for inline in inlines {
                        if let Inline::Move(m) = inline {
                            m.keep_reading();
                        }
                    }
                }
                Block::Diagram(d) => d.review.retain(|f| !matches!(f, ReviewFlag::NoStartPosition)),
            }
        }
        build::walk(page.index, page.ocr, &mut page.blocks, &mut self.walker, page.trailing, &mut self.heads);
        self.carry = page.trailing.map(|n| (page.index, n));
        self.pages.push(page);
    }

    /// The page before the next one added, page `page`, was not added, but was looked at on its
    /// own: it left the caption `label` at its foot ([`trailing_label`]). The next page's first
    /// diagram takes it (unless a caption of its own numbers it) when that page is `page + 1`.
    pub fn carry_caption(&mut self, page: u32, label: u32) {
        self.carry = Some((page, label));
    }

    /// The document: every open game is closed and every move's node reference made final.
    pub fn finish(self) -> ChessDoc {
        let mut pages = self.pages;
        let done = self.walker.finish();
        for page in &mut pages {
            for block in &mut page.blocks {
                match block {
                    Block::Heading { inlines, .. } | Block::Paragraph { inlines, .. } => {
                        for inline in inlines {
                            if let Inline::Move(m) = inline {
                                m.game = m.game.and_then(|r| done.remap(r));
                            }
                        }
                    }
                    Block::Diagram(d) => {
                        if done.blocked.contains(&(page.index, d.number)) {
                            d.review.push(ReviewFlag::NoStartPosition);
                        }
                    }
                }
            }
        }
        ChessDoc { format: FORMAT_VERSION, pages, games: done.games }
    }
}

/// The puzzle label a page leaves at its foot for the next page's first diagram
/// ([`ChessPage::trailing`]), from its text and where its diagrams are (`diagrams`, user space),
/// without reading them: the cheap look at the page before a page read alone
/// ([`DocBuilder::carry_caption`]).
pub fn trailing_label(text: &ChessPageText, geometry: &PageGeometry, diagrams: &[PageRect]) -> Option<u32> {
    build::trailing_label(text, geometry, diagrams)
}

/// `pages`, read before, walked again as one document with `rules`, in their order, from what
/// was read (plan A9-L0a): `pages` are left as they are, and walking the result again gives it
/// back ([`DocBuilder::add_read_page`]).
pub fn walk(pages: &[ChessPage], rules: WalkRules) -> ChessDoc {
    let mut builder = DocBuilder::with_rules(rules);
    for page in pages {
        builder.add_read_page(page.clone());
    }
    builder.finish()
}

/// A one-page document.
pub fn build_page(input: &PageInput<'_>) -> ChessDoc {
    let mut builder = DocBuilder::new();
    builder.add_page(input);
    builder.finish()
}

/// Why a document's JSON could not be read.
#[derive(Debug, thiserror::Error)]
pub enum DocJsonError {
    #[error("the document JSON is {0} bytes, more than the {MAX_JSON_BYTES} read")]
    TooLarge(usize),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl ChessDoc {
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Read a document written by [`ChessDoc::to_json`]. Node references are not trusted: every
    /// reader of a `ChessDoc` looks them up with `get`.
    pub fn from_json(text: &str) -> Result<ChessDoc, DocJsonError> {
        if text.len() > MAX_JSON_BYTES {
            return Err(DocJsonError::TooLarge(text.len()));
        }
        Ok(serde_json::from_str(text)?)
    }

    /// Every game as PGN, one after the other.
    pub fn pgn(&self) -> String {
        pgn::write(self.games.iter().map(|g| &g.score))
    }
}

impl From<&TextQuality> for TextLayer {
    fn from(q: &TextQuality) -> TextLayer {
        match q {
            TextQuality::Good => TextLayer::Good,
            TextQuality::Poor { .. } => TextLayer::Poor,
            TextQuality::Missing => TextLayer::Missing,
        }
    }
}
