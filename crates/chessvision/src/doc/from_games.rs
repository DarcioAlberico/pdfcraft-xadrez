//! From games to a book (plan A9-N9, A8-12): games, from a PGN or the board's collection, as a
//! [`ChessDoc`] every export writes as it writes a book read from a PDF (HTML, EPUB, Word, RTF,
//! Markdown), unchanged.
//!
//! Each game is a page of its own: a heading with its players (else its event, else its number),
//! a line of its other tags (event, site, date, round), a diagram of its start position when it
//! does not start from the initial one (`SetUp`/`FEN`), and its moves in paragraphs, as a book
//! sets them:
//!
//! - the main line in bold and the analysis in regular type ([`MoveInline::bold`]), the
//!   variations of a main-line move in a paragraph of their own, between brackets, the main line
//!   going on in the next with its number (`12...Nf6`);
//! - a move's comments before and after it, its annotation glyphs, and the result at the end;
//! - a diagram of the position after each move marked `$220` (`$221`: printed from Black's side),
//!   the paragraph going on after it; the mark itself is the diagram, not a glyph.
//!
//! Every move is valid, with the position after it ([`MoveInline::fen`]): the games were played to
//! be read in. The document's [`ChessDoc::games`] stay empty, since the games are the input; the
//! PGN of such a book is theirs.

use pdfcraft_chess::game::{Game, Token};
use pdfcraft_chess::nag;
use pdfcraft_chess::placement::Color;

use super::{
    Block, BlockRole, ChessDoc, ChessPage, DiagramBlock, DiagramReading, DiagramSource, FORMAT_VERSION, Inline, LangTag, MoveInline, MoveNumber,
    MoveStatus, SideSource, TextLayer,
};

/// Most games one book holds.
pub const MAX_BOOK_GAMES: usize = 4096;

/// How [`from_games`] makes the book.
#[derive(Clone, Debug, Default)]
pub struct FromGames {
    /// The language of the book's text (its comments; [`ChessPage::text_lang`]); empty when not
    /// said, and the exports then say the piece letters' (English).
    pub text_lang: LangTag,
}

/// `games` as a book (see the module documentation), one page per game in their order, at most
/// [`MAX_BOOK_GAMES`]; the rest are left out, as the caller is told by the count of pages.
pub fn from_games(games: &[Game], opts: &FromGames) -> ChessDoc {
    let pages = games.iter().take(MAX_BOOK_GAMES).enumerate().map(|(k, game)| game_page(u32::try_from(k).unwrap_or(u32::MAX), game, opts)).collect();
    ChessDoc { format: FORMAT_VERSION, pages, games: Vec::new() }
}

/// The page of game `game`, the `index`-th (from 0).
fn game_page(index: u32, game: &Game, opts: &FromGames) -> ChessPage {
    let mut page = Page::default();
    page.blocks.push(Block::Heading { level: 1, inlines: vec![text(&title(index, game))], source_rect: None, role: BlockRole::Content });
    let details = details(game);
    if !details.is_empty() {
        page.blocks.push(Block::Paragraph { inlines: vec![text(&details)], source_rect: None, role: BlockRole::GameHeader });
    }
    // A game that starts from a position of its own shows it first.
    let start = game.start_fen();
    if start != Game::standard().start_fen() {
        page.diagram(&start, false);
    }
    page.moves(game);
    ChessPage {
        index,
        lang: "en".into(),
        text_lang: opts.text_lang.clone(),
        text_layer: TextLayer::Good,
        ocr: false,
        blocks: page.blocks,
        review: Vec::new(),
        trailing: None,
        running_heads: Vec::new(),
    }
}

/// A tag's value when it says something (not empty, not `?`).
fn tag<'a>(game: &'a Game, name: &str) -> Option<&'a str> {
    game.tag(name).map(str::trim).filter(|v| !v.is_empty() && !v.chars().all(|c| matches!(c, '?' | '.')))
}

/// The game's heading: its players, else its event, else its number.
fn title(index: u32, game: &Game) -> String {
    match (tag(game, "White"), tag(game, "Black")) {
        (Some(w), Some(b)) => format!("{w} – {b}"),
        (Some(one), None) | (None, Some(one)) => one.to_owned(),
        (None, None) => tag(game, "Event").map_or_else(|| format!("Game {}", u64::from(index).saturating_add(1)), str::to_owned),
    }
}

/// The game's other tags, in one line: event (unless it is the heading), site, date, round.
fn details(game: &Game) -> String {
    let players = tag(game, "White").is_some() || tag(game, "Black").is_some();
    let mut parts: Vec<String> = Vec::new();
    if players && let Some(event) = tag(game, "Event") {
        parts.push(event.to_owned());
    }
    parts.extend(["Site", "Date"].iter().filter_map(|t| tag(game, t)).map(str::to_owned));
    if let Some(round) = tag(game, "Round") {
        parts.push(format!("round {round}"));
    }
    parts.join(", ")
}

fn text(s: &str) -> Inline {
    Inline::Text { text: s.to_owned(), source_rect: None, review: Vec::new() }
}

/// The page as it is made: its blocks, and the paragraph being filled.
#[derive(Default)]
struct Page {
    blocks: Vec<Block>,
    para: Vec<Inline>,
    diagrams: usize,
}

impl Page {
    /// End the paragraph being filled, when it holds anything.
    fn flush(&mut self) {
        if !self.para.is_empty() {
            let inlines = std::mem::take(&mut self.para);
            self.blocks.push(Block::Paragraph { inlines, source_rect: None, role: BlockRole::Content });
        }
    }

    /// A diagram of `fen` (printed from Black's side when `black`), after the paragraph so far.
    fn diagram(&mut self, fen: &str, black: bool) {
        self.flush();
        let number = self.diagrams;
        self.diagrams = self.diagrams.saturating_add(1);
        let orientation = black.then_some(Color::Black);
        let link =
            pdfcraft_chess::fen::parse(fen).ok().map(|p| pdfcraft_chess::lichess::analysis_url(&p, if black { Color::Black } else { Color::White }));
        self.blocks.push(Block::Diagram(DiagramBlock {
            number,
            index: number,
            source_rect: None,
            source: DiagramSource::User,
            dpi: 0.0,
            reading: Some(DiagramReading {
                fen: fen.to_owned(),
                side_source: SideSource::Printed,
                orientation,
                accepted: true,
                min_confidence: 1.0,
                repaired: false,
            }),
            lichess_url: link,
            puzzle: None,
            review: Vec::new(),
            reference: None,
            settled: None,
        }));
    }

    /// The game's moves, comments, variations and result, from its PGN tokens.
    fn moves(&mut self, game: &Game) {
        // How deep in brackets, and whether the paragraph being filled holds the main line's
        // variations (it ends where the main line goes on).
        let mut level = 0usize;
        let mut in_variations = false;
        for token in game.tokens() {
            match token {
                Token::VariationStart => {
                    if level == 0 && !in_variations {
                        self.flush();
                        in_variations = true;
                    }
                    level = level.saturating_add(1);
                    self.para.push(text("("));
                }
                Token::VariationEnd => {
                    level = level.saturating_sub(1);
                    self.para.push(text(")"));
                }
                Token::Comment { text: comment, .. } => {
                    if !comment.trim().is_empty() {
                        self.para.push(text(comment.trim()));
                    }
                }
                Token::Move { node, number, color, numbered, san, nags, depth } => {
                    if level == 0 && in_variations {
                        self.flush();
                        in_variations = false;
                    }
                    // A move that opens a paragraph says its number, as after a diagram.
                    let numbered = numbered || color == Color::White || self.para.is_empty();
                    let number = numbered.then(|| MoveNumber { number: u16::try_from(number).unwrap_or(u16::MAX), black: color == Color::Black });
                    let fen = game.fen_at(node).map(|f| f.to_string());
                    self.para.push(Inline::Move(MoveInline {
                        number,
                        text: san.clone(),
                        san_read: san.clone(),
                        san,
                        status: MoveStatus::Valid,
                        fen: fen.clone(),
                        game: None,
                        source_rect: None,
                        bold: depth == 0,
                        read_review: Vec::new(),
                        review: Vec::new(),
                        reference: None,
                    }));
                    for &code in nags.iter().filter(|c| !nag::is_diagram(**c)) {
                        self.para.push(Inline::Nag { code, source_rect: None, review: Vec::new() });
                    }
                    if let Some(code) = nags.iter().copied().find(|c| nag::is_diagram(*c))
                        && let Some(fen) = fen
                    {
                        self.diagram(&fen, code == nag::DIAGRAM_FROM_BLACK);
                    }
                }
            }
        }
        let result = game.result();
        if matches!(result.as_str(), "1-0" | "0-1" | "1/2-1/2") {
            self.para.push(text(&result));
        }
        self.flush();
    }
}
