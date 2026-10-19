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

#[cfg(test)]
mod tests;

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
    /// The share (0 to 1) of the judged glyphs drawn invisibly, as an OCR layer over a scan is.
    /// [`Self::over_page_image`] judges it once the caller knows the page is a picture.
    pub hidden: f32,
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
    // 1. Glyphs to cells, line by line, figurine letters settled by what follows them. A line is
    //    first cut where it crosses a gutter, so the columns of a two-column page are lines apart.
    let line_of = split_at_gutters(page);
    let mut mapper = glyph::GlyphMapper::default();
    let mut lines: Vec<Vec<glyph::Cell>> = Vec::new();
    let mut line: Vec<glyph::Cell> = Vec::new();
    let mut line_id: Option<u32> = None;
    for (i, g) in page.glyphs.iter().enumerate() {
        // A glyph without a line number stays on the current line.
        let id = line_of.get(i).copied().flatten().or(line_id);
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
    for word in lines.iter().flatten() {
        let chars: Vec<char> = word.iter().map(|c| c.ch).collect();
        if quality::is_garbled_piece(&chars) {
            mapper.stats.garbled = mapper.stats.garbled.saturating_add(1);
        }
    }
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
    ChessPageText {
        lines,
        lang,
        quality: quality::verdict(&mapper.stats),
        hidden: quality::hidden_ratio(&mapper.stats),
        unknown_chess_fonts: mapper.unknown_fonts,
        lang_tally,
    }
}

impl ChessPageText {
    /// Judge the layer again on a page that is a picture (images cover most of it): a layer
    /// mostly drawn invisibly over it is another program's OCR ([`quality::over_page_image`]).
    pub fn over_page_image(&mut self) {
        self.quality = quality::over_page_image(&self.quality, self.hidden);
    }
}

/// A gap between two glyphs of one line wider than this many times the taller glyph's height is
/// a gutter between columns wherever it is: justified text stretches its spaces to about one
/// height at most.
pub const GUTTER_SHARE: f32 = 2.5;
/// A narrower gap (down to [`ALIGNED_SHARE`] of a height and [`MIN_GUTTER`] points) is a gutter
/// when at least [`ALIGNED_LINES`] other lines have one at the same place: books leave 12–24 pt
/// between columns, and word spaces do not line up from line to line.
pub const ALIGNED_SHARE: f32 = 0.6;
pub const ALIGNED_LINES: usize = 2;
/// The narrowest gutter, in points.
pub const MIN_GUTTER: f32 = 6.0;
/// Candidate gaps looked at per page, at most.
const MAX_GAPS: usize = 4096;

/// Each glyph's line, with a layout line that crosses a gutter cut into one line per side. The
/// numbers are fresh (in order) and say only which glyphs share a line.
fn split_at_gutters(page: &PageText) -> Vec<Option<u32>> {
    // 1. Candidate gaps: (glyph that starts after the gap, layout line, gap x0, gap x1, wide).
    let mut gaps: Vec<(usize, u32, f32, f32, bool)> = Vec::new();
    let mut prev: Option<(u32, [f32; 4])> = None;
    for (i, g) in page.glyphs.iter().enumerate() {
        let Some(layout) = page.line_of.get(i).copied() else { continue };
        if let Some((pl, p)) = prev
            && pl == layout
            && gaps.len() < MAX_GAPS
        {
            let h = (p[3] - p[1]).max(g.rect[3] - g.rect[1]).max(0.0);
            let gap = g.rect[0] - p[2];
            if gap.is_finite() && h.is_finite() && gap > MIN_GUTTER && gap > ALIGNED_SHARE * h {
                gaps.push((i, layout, p[2], g.rect[0], gap > GUTTER_SHARE * h));
            }
        }
        prev = Some((layout, g.rect));
    }
    // 2. The gaps that cut: wide ones, and those whose middle other lines' gaps also cover.
    let cuts: std::collections::HashSet<usize> = gaps
        .iter()
        .filter(|(_, line, x0, x1, wide)| {
            let mid = (x0 + x1) * 0.5;
            let aligned = || {
                let mut lines: Vec<u32> = gaps.iter().filter(|(_, l, a, b, _)| l != line && *a <= mid && mid <= *b).map(|g| g.1).collect();
                lines.dedup();
                lines.len() >= ALIGNED_LINES
            };
            *wide || aligned()
        })
        .map(|g| g.0)
        .collect();
    // 3. The lines.
    let mut out = Vec::with_capacity(page.glyphs.len());
    let mut next: u32 = 0;
    let mut current: Option<(u32, u32)> = None; // (layout line, our line)
    for i in 0..page.glyphs.len() {
        let Some(layout) = page.line_of.get(i).copied() else {
            out.push(current.map(|c| c.1));
            continue;
        };
        if current.is_none_or(|c| c.0 != layout) || cuts.contains(&i) {
            current = Some((layout, next));
            next = next.saturating_add(1);
        }
        out.push(current.map(|c| c.1));
    }
    out
}

/// [`from_page_text`] for glyphs in content order: lays them out into lines first.
pub fn from_glyphs(glyphs: &[TextGlyph], lang_hint: Option<Lang>) -> ChessPageText {
    from_page_text(&pdfcraft_render::text::layout(glyphs.to_vec()), lang_hint)
}
