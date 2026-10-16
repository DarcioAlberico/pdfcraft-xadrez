//! Chess text from a page text layer: figurine fonts mapped to pieces, moves, move numbers and
//! NAGs (blueprint S12).
//!
//! Many chess books set their moves in a figurine font: the glyph drawn is a knight, the code
//! underneath is `N`, and a careless (or missing) `ToUnicode` map extracts something else again,
//! so the layer reads `2.l0xd4` where the page shows `2.♘xd4`. [`from_page_text`] undoes that
//! and cuts each line into [`ChessRun`]s:
//!
//! 1. **Glyphs.** Each glyph's font is looked up in the `pdfcraft-chess` catalogue. In a
//!    catalogued *figurine* font the character code decides, not the extracted text: the
//!    catalogue puts the pieces on the English capitals (`K Q R B N P`, also in the Windows
//!    symbol range U+F0xx; lower case stays a letter, as `b` is also a file). A map that already
//!    says `♘` is believed. Whether a capital is drawn as a piece is decided by context, as
//!    publishers set whole sentences in figurine fonts: it is a piece when notation follows
//!    (`Nf3`, `Nxe5`, `Nbd7`, `R1e2`), after a promotion (`e8=Q`), or in a word of at most two
//!    such letters. A glyph in a figurine font whose code is not a piece letter but which stands
//!    where a piece belongs is *unresolved*. Glyphs in fonts that look like chess fonts but are
//!    not catalogued are kept as they extract and flagged ([`ChessPageText::unknown_chess_fonts`]),
//!    never guessed. Chess Informant symbol fonts park their evaluation signs on Latin-1 code
//!    points (`¢` for `⩲`); those are mapped back. Glyphs of chess *diagram* fonts are a position,
//!    not text, and are left out (the diagram detector reads them).
//! 2. **Publisher symbols.** `pdfcraft_chess::nag::normalize_publisher_symbols` runs on the
//!    mapped text before anything else touches it, so `Rad8³` keeps its evaluation.
//! 3. **Language.** Which language's piece letters the page uses ([`lang`]).
//! 4. **Runs.** Move numbers (`12.`, `7...`, `7…`), moves read with `San::read` in that language,
//!    annotation glyphs, lone figurines and prose. A bare pawn move (`f3`) is a move only next to
//!    other notation, so "the knight on f3" stays prose.
//!
//! Alongside the runs comes a verdict on the layer itself ([`quality`]), so the engine knows when
//! to read the page with OCR instead.
//!
//! **Coordinates.** Every rectangle here is in **view space** ([`ViewRect`]: points, y down,
//! origin at the displayed page's top-left corner), the space of `TextGlyph::rect`.
//! [`ChessSpan::user_rect`] converts to PDF user space through [`crate::coords`].

mod glyph;
pub mod lang;
pub mod quality;
mod token;

use std::sync::Arc;

use pdfcraft_chess::placement::PieceKind;
use pdfcraft_chess::san::{Lang, San};
use pdfcraft_render::text::{PageText, TextGlyph};

use crate::coords::{PageGeometry, PageRect, ViewRect};

pub use lang::{LangGuess, LangTally};
pub use quality::{PoorReason, TextQuality};

/// One piece of a line of chess text.
#[derive(Clone, Debug, PartialEq)]
pub enum ChessRun {
    /// Prose, punctuation, results and anything else that is not notation. Words keep the single
    /// spaces between them.
    Text(String),
    /// A move number: `12.` is White's (`black: false`), `12...` and `12…` are Black's.
    MoveNumber { number: u16, black: bool },
    /// A move. `san.suffix` holds only the check and mate marks; annotation glyphs glued to the
    /// move follow it as [`ChessRun::Nag`]s. `text` is the move as printed (check marks
    /// included, annotations not), with figurine-font letters already shown as pieces
    /// (`♘xd4`).
    Move { san: San, text: String },
    /// An annotation glyph, as its PGN code (`$15` for `⩱`).
    Nag(u16),
    /// A figurine that is not part of a move: "the ♘ on f3".
    Figurine(PieceKind),
}

/// A run and where it is on the page.
#[derive(Clone, Debug, PartialEq)]
pub struct ChessSpan {
    pub run: ChessRun,
    /// The union of the run's glyph boxes, in view space.
    pub rect: ViewRect,
}

impl ChessSpan {
    /// The run's box in PDF user space, for a page with this geometry. `None` when the box is
    /// not finite.
    pub fn user_rect(&self, page: &PageGeometry) -> Option<PageRect> {
        page.view_to_user(self.rect)
    }
}

/// One line of the page, in reading order.
#[derive(Clone, Debug, PartialEq)]
pub struct ChessLine {
    pub spans: Vec<ChessSpan>,
    /// The union of the line's span boxes, in view space.
    pub rect: ViewRect,
}

/// A page's text layer read as chess text.
#[derive(Clone, Debug, PartialEq)]
pub struct ChessPageText {
    /// Lines in reading order. Lines with nothing but diagram-font glyphs are left out.
    pub lines: Vec<ChessLine>,
    /// The language the piece letters were read in.
    pub lang: LangGuess,
    /// Whether the layer can be believed.
    pub quality: TextQuality,
    /// Fonts that look like chess fonts but are not in the catalogue. Their glyphs are kept as
    /// they extract; adding the family to the catalogue fixes them.
    pub unknown_chess_fonts: Vec<Arc<str>>,
    /// The page's language evidence, for choosing one language for a whole document.
    pub lang_tally: LangTally,
}

/// Read a page's text layer, already laid out into lines (`pdfcraft_render::text::layout`).
///
/// `lang_hint` is the document's language when known ([`LangTally::guess`] says when it wins over
/// the page's own letters).
pub fn from_page_text(page: &PageText, lang_hint: Option<Lang>) -> ChessPageText {
    // 1. Glyphs to cells, line by line, figurine letters settled by what follows them.
    let mut mapper = glyph::GlyphMapper::default();
    let mut lines: Vec<Vec<glyph::Cell>> = Vec::new();
    let mut line: Vec<glyph::Cell> = Vec::new();
    let mut line_id: Option<u32> = None;
    for (i, g) in page.glyphs.iter().enumerate() {
        // A glyph without a line number stays on the current line.
        let id = page.line_of.get(i).copied().or(line_id);
        if id != line_id {
            lines.push(std::mem::take(&mut line));
            line_id = id;
        } else if page.space_before.get(i).copied().unwrap_or(false) {
            line.push(glyph::Cell::space());
        }
        mapper.map(g, &mut line);
    }
    lines.push(line);
    for line in &mut lines {
        glyph::resolve_figurines(line, &mut mapper.stats);
    }
    // 2. Publisher symbols, word by word.
    let lines: Vec<Vec<Vec<glyph::Cell>>> = lines.iter().map(|l| token::words(l).map(token::normalize_symbols).collect()).collect();
    // 3. The language.
    let mut lang_tally = LangTally::default();
    for word in lines.iter().flatten() {
        token::tally_word(word, &mut lang_tally);
    }
    let lang = lang_tally.guess(lang_hint);
    // 4. Runs.
    let mut tokens: Vec<Vec<Vec<token::Token>>> = lines.iter().map(|words| words.iter().map(|w| token::tokenize(w, lang.lang)).collect()).collect();
    token::settle(&mut tokens);
    let mut side = token::Side::default();
    let lines = tokens.into_iter().filter_map(|words| token::assemble(words, &mut side)).collect();
    ChessPageText { lines, lang, quality: quality::verdict(&mapper.stats), unknown_chess_fonts: mapper.unknown_fonts, lang_tally }
}

/// [`from_page_text`] for glyphs in content order: lays them out into lines first.
pub fn from_glyphs(glyphs: &[TextGlyph], lang_hint: Option<Lang>) -> ChessPageText {
    from_page_text(&pdfcraft_render::text::layout(glyphs.to_vec()), lang_hint)
}
