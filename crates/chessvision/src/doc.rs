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
//! Everything is serde JSON ([`ChessDoc::to_json`], [`ChessDoc::from_json`]). The shape is
//! versioned by [`FORMAT_VERSION`].
//!
//! The data model takes ideas from the Caïssa IR and `export/fidelity.py` (the author's own code,
//! relicensed MIT/Apache, `docs/chess/provenance.md`); the code is written here.

mod build;
pub mod games;
pub mod html;
pub mod markdown;
pub mod pgn;

#[cfg(test)]
mod tests;

use pdfcraft_chess::rules::Legality;
use serde::{Deserialize, Serialize};

use crate::board::{BoardReading, ReadPosition, ReadingSource};
use crate::chess_text::{ChessPageText, TextQuality};
use crate::cipher::{WordEdit, WordFlag};
use crate::coords::{PageGeometry, PageRect, ViewRect, pixels_to_view};

pub use games::{Game, GameOrigin, GameResult, GameScore, Node, NodeRef, Roster, Tag};

/// The version of the JSON shape. Bumped when a field changes meaning or is removed.
pub const FORMAT_VERSION: u32 = 1;

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
    /// A comment reached [`games::MAX_COMMENT_CHARS`]; the PGN comment is cut there (the text in
    /// the blocks is whole).
    CommentTruncated,
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
    pub blocks: Vec<Block>,
    /// Flags that belong to the page rather than to one block.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub review: Vec<ReviewFlag>,
}

/// A block of the page, in reading order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Heading { level: u8, inlines: Vec<Inline>, source_rect: Option<UserRect> },
    Paragraph { inlines: Vec<Inline>, source_rect: Option<UserRect> },
    Diagram(DiagramBlock),
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
    /// English SAN: canonical (disambiguation and check marks as the position requires) when
    /// [`MoveStatus::Valid`], else as read.
    pub san: String,
    pub status: MoveStatus,
    /// The position after the move, when valid.
    pub fen: Option<String>,
    /// The move's node in [`ChessDoc::games`], when valid.
    pub game: Option<NodeRef>,
    pub source_rect: Option<UserRect>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub review: Vec<ReviewFlag>,
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

    fn review_mut(&mut self) -> &mut Vec<ReviewFlag> {
        match self {
            Inline::Text { review, .. } | Inline::Nag { review, .. } | Inline::Link { review, .. } => review,
            Inline::Move(m) => &mut m.review,
        }
    }

    /// The inline as it reads: a move with its number, a NAG as its glyph.
    pub fn text(&self) -> String {
        match self {
            Inline::Text { text, .. } | Inline::Link { text, .. } => text.clone(),
            Inline::Move(m) => format!("{}{}", m.number.map(|n| n.text()).unwrap_or_default(), m.text),
            Inline::Nag { code, .. } => pdfcraft_chess::nag::by_code(*code).map_or_else(|| format!("${code}"), |n| n.glyph.to_owned()),
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
}

/// Builds a [`ChessDoc`] page by page, carrying open games across page breaks.
#[derive(Debug, Default)]
pub struct DocBuilder {
    pages: Vec<ChessPage>,
    walker: games::Walker,
}

impl DocBuilder {
    pub fn new() -> DocBuilder {
        DocBuilder::default()
    }

    /// Add the next page. Pages are taken in the order added.
    pub fn add_page(&mut self, input: &PageInput<'_>) {
        let page = build::page(input, &mut self.walker);
        self.pages.push(page);
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
