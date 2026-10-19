//! Step 1: what each glyph of the text layer says, once its font is known.
//!
//! Each glyph becomes one or more [`Cell`]s (one per character it reads as), carrying the glyph's
//! box. Figurine-font letters are only *candidates* here; [`resolve_figurines`] decides from the
//! rest of the line whether each one is drawn as a piece.

use std::collections::HashMap;
use std::sync::Arc;

use pdfcraft_chess::catalog::{self, FontKind};
use pdfcraft_chess::placement::PieceKind;
use pdfcraft_chess::san::{FigurineSet, figurine, figurine_piece};
use pdfcraft_render::text::TextGlyph;

use super::quality::GlyphStats;

/// Characters kept from one glyph's text. A glyph is one character or a ligature; more is noise.
const MAX_GLYPH_CHARS: usize = 8;

/// Unknown chess fonts reported per page, at most.
const MAX_UNKNOWN_FONTS: usize = 32;

/// How a font's glyphs are read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FontClass {
    /// Ordinary text: the extracted Unicode is believed.
    Plain,
    /// A catalogued inline figurine font: piece letters by code.
    Figurine,
    /// A Chess Informant symbol font: evaluation signs on Latin-1 code points.
    Informator,
    /// A catalogued chess diagram font: a position, not text.
    Diagram,
    /// Looks like a chess font but is not catalogued.
    UnknownChess,
}

fn classify(name: &str) -> FontClass {
    let bare = catalog::normalize_font_name(name);
    if bare.contains("informator") || bare.contains("informant") {
        return FontClass::Informator;
    }
    match catalog::lookup_family(name) {
        Some(f) if f.kind == FontKind::Figurine => FontClass::Figurine,
        // The Unicode family has no name pattern: it is what an empty font name maps to.
        Some(f) if f.kind == FontKind::Diagram && f.key != "unicode" => FontClass::Diagram,
        Some(_) => FontClass::Plain,
        None if catalog::looks_like_chess_font(name) => FontClass::UnknownChess,
        None => FontClass::Plain,
    }
}

/// The symbols Chess Informant fonts put on Latin-1 code points, read off a book's own key to
/// symbols (the user's Caïssa importer, relicensed for PdfCraft: `docs/chess/provenance.md`).
const INFORMATOR: &[(char, char)] = &[
    ('¢', '⩲'),
    ('£', '⩱'),
    ('¥', '±'),
    ('¤', '∓'),
    ('»', '−'),
    ('ú', '='),
    ('Õ', '∞'),
    ('§', '©'),
    ('¶', '⇑'),
    ('ï', '→'),
    ('î', '↑'),
    ('|', '⇆'),
    ('Â', '⊙'),
    ('Å', '△'),
    ('ì', '□'),
    ('Ä', '⌓'),
    ('‹', '≤'),
    ('°', '⊕'),
    ('Á', '◻'),
    ('À', '◼'),
];

fn informator(c: char) -> char {
    INFORMATOR.iter().find(|(from, _)| *from == c).map_or(c, |(_, to)| *to)
}

/// One character of a line, with the box of the glyph it came from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Cell {
    /// The character; a space is a word gap.
    pub ch: char,
    pub rect: [f32; 4],
    /// A figurine-font letter whose code names this piece: drawn as the piece or as the letter,
    /// depending on what follows.
    pub piece_code: Option<PieceKind>,
    /// A figurine-font glyph whose code is not a piece letter: unresolved if it stands where a
    /// piece belongs.
    pub odd_code: bool,
}

impl Cell {
    pub fn new(ch: char, rect: [f32; 4]) -> Self {
        Cell { ch, rect, piece_code: None, odd_code: false }
    }

    pub fn space() -> Self {
        Cell::new(' ', [0.0; 4])
    }
}

/// How much of a glyph's extracted text can be used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Health {
    Ok,
    /// U+FFFD: the extractor knew it had nothing.
    Replacement,
    /// Empty, U+0000, controls, private use: a dead map.
    Dead,
}

fn is_dead_char(c: char) -> bool {
    let u = u32::from(c);
    c.is_control() || (0xE000..=0xF8FF).contains(&u) || u >= 0xF_0000 || (u & 0xFFFE) == 0xFFFE
}

fn health(chars: &[char]) -> Health {
    if chars.contains(&'\u{FFFD}') {
        Health::Replacement
    } else if chars.iter().all(|c| is_dead_char(*c)) {
        Health::Dead
    } else {
        Health::Ok
    }
}

/// A one-byte character code (or one in the Windows symbol range, folded to ASCII) as a
/// character. Two-byte codes are glyph indices of a Type0 font and say nothing by themselves.
fn code_char(code: Option<u32>) -> Option<char> {
    let code = code.filter(|c| *c <= 0xFF || (0xF000..=0xF0FF).contains(c))?;
    char::from_u32(code).map(catalog::normalize_char)
}

fn is_wide(code: Option<u32>) -> bool {
    code.is_some_and(|c| c > 0xFF && !(0xF000..=0xF0FF).contains(&c))
}

/// The piece an upper-case figurine-font letter draws. Lower case stays a letter: `b` is also a
/// file.
fn piece_letter(c: char) -> Option<PieceKind> {
    if c.is_ascii_uppercase() { catalog::figurine_letter(c) } else { None }
}

/// Maps a page's glyphs to cells, remembering what each font is.
#[derive(Default)]
pub(crate) struct GlyphMapper {
    fonts: HashMap<Arc<str>, FontClass>,
    pub stats: GlyphStats,
    pub unknown_fonts: Vec<Arc<str>>,
}

impl GlyphMapper {
    fn class(&mut self, font: Option<&Arc<str>>) -> FontClass {
        let Some(font) = font else { return FontClass::Plain };
        if let Some(class) = self.fonts.get(font) {
            return *class;
        }
        let class = classify(font);
        if class == FontClass::UnknownChess && self.unknown_fonts.len() < MAX_UNKNOWN_FONTS {
            self.unknown_fonts.push(Arc::clone(font));
        }
        self.fonts.insert(Arc::clone(font), class);
        class
    }

    /// Push the cells `g` reads as onto `out`. Whitespace becomes a word gap; diagram-font glyphs
    /// push nothing.
    pub fn map(&mut self, g: &TextGlyph, out: &mut Vec<Cell>) {
        if !g.text.is_empty() && g.text.chars().all(char::is_whitespace) {
            out.push(Cell::space());
            return;
        }
        self.stats.visible = self.stats.visible.saturating_add(1);
        let class = self.class(g.font.as_ref());
        if class == FontClass::Diagram {
            return;
        }
        self.stats.judged = self.stats.judged.saturating_add(1);
        let chess = class != FontClass::Plain;
        let chars: Vec<char> = g.text.chars().take(MAX_GLYPH_CHARS).map(|c| if chess { catalog::normalize_char(c) } else { c }).collect();
        let code = code_char(g.code);
        let cell = |ch: char| Cell::new(ch, g.rect);
        match class {
            FontClass::Figurine => {
                // A map that already says "knight" is believed.
                if let [c] = chars.as_slice()
                    && let Some(kind) = figurine_piece(*c)
                {
                    out.push(cell(figurine(kind, FigurineSet::Outline)));
                    return;
                }
                // Otherwise the code decides: the catalogue's figurine layout puts the pieces on
                // the English letters, whatever the text map says. A subset font renumbers its
                // codes (`K` drawn from code 46): when the code names no piece, a text map that
                // says a piece letter is believed.
                let single = if let [c] = chars.as_slice() { Some(*c) } else { None };
                if let Some((letter, kind)) = [code, single].into_iter().flatten().find_map(|l| piece_letter(l).map(|k| (l, k))) {
                    out.push(Cell { piece_code: Some(kind), ..cell(letter) });
                    return;
                }
                let odd = |ch: char| Cell { odd_code: true, ..cell(ch) };
                if health(&chars) == Health::Ok {
                    out.extend(chars.iter().map(|c| odd(*c)));
                } else if let Some(c) = code.filter(|c| c.is_ascii_graphic()) {
                    out.push(odd(c));
                } else {
                    self.stats.unresolved = self.stats.unresolved.saturating_add(1);
                    self.count_bad(&chars, g.code);
                    out.extend(chars.iter().map(|c| cell(*c)));
                }
            }
            FontClass::Informator => {
                if health(&chars) == Health::Ok {
                    out.extend(chars.iter().map(|c| cell(informator(*c))));
                } else if let Some(c) = code {
                    out.push(cell(informator(c)));
                } else {
                    self.count_bad(&chars, g.code);
                }
            }
            FontClass::UnknownChess | FontClass::Plain => {
                if class == FontClass::UnknownChess {
                    self.stats.unresolved = self.stats.unresolved.saturating_add(1);
                }
                self.count_bad(&chars, g.code);
                out.extend(chars.iter().map(|c| cell(*c)));
            }
            // Returned above.
            FontClass::Diagram => {}
        }
    }

    fn count_bad(&mut self, chars: &[char], code: Option<u32>) {
        let s = &mut self.stats;
        match health(chars) {
            Health::Ok => return,
            Health::Replacement => s.replacement = s.replacement.saturating_add(1),
            Health::Dead => s.dead = s.dead.saturating_add(1),
        }
        if is_wide(code) {
            s.wide_bad = s.wide_bad.saturating_add(1);
        }
    }
}

fn is_file(c: char) -> bool {
    ('a'..='h').contains(&c)
}

fn is_rank(c: char) -> bool {
    ('1'..='8').contains(&c)
}

fn is_capture(c: char) -> bool {
    matches!(c, 'x' | ':' | '×')
}

/// Whether the characters after a letter make it a piece: a square (`Na3`), a capture (`Nxe5`),
/// a file or rank disambiguation (`Nbd7`, `R1e2`, `R1xe2`).
fn notation_after(next: &[char]) -> bool {
    match next {
        [a, b, ..] if is_file(*a) && (is_rank(*b) || is_capture(*b)) => true,
        [a, b, c, ..] if is_file(*a) && is_file(*b) && is_rank(*c) => true,
        [a, b, ..] if is_capture(*a) && is_file(*b) => true,
        [a, b, ..] if is_rank(*a) && (is_file(*b) || is_capture(*b)) => true,
        _ => false,
    }
}

/// A promotion letter: after `=`, after `(`, or straight after a square (`e8Q`).
fn promotion_before(prev: &[char]) -> bool {
    match prev {
        [.., '=' | '('] => true,
        [.., f, r] => is_file(*f) && is_rank(*r),
        _ => false,
    }
}

/// Words of at most this many figurine-font piece letters are pieces wherever they stand.
const MAX_ISOLATED: usize = 2;

/// Decide, for each figurine-font piece letter of a line, whether it is drawn as a piece, and
/// count the odd codes that stand where a piece belongs. `cells` is one line, word gaps
/// included.
pub(crate) fn resolve_figurines(cells: &mut [Cell], stats: &mut GlyphStats) {
    if !cells.iter().any(|c| c.piece_code.is_some() || c.odd_code) {
        return;
    }
    let chars: Vec<char> = cells.iter().map(|c| c.ch).collect();
    let mut start = 0;
    while start < cells.len() {
        let len = chars.get(start..).map_or(0, |rest| rest.iter().take_while(|c| !c.is_whitespace()).count());
        let end = start.saturating_add(len);
        let isolated = (1..=MAX_ISOLATED).contains(&len) && cells.get(start..end).is_some_and(|w| w.iter().all(|c| c.piece_code.is_some()));
        for i in start..end {
            let next = chars.get(i.saturating_add(1)..end.min(i.saturating_add(4))).unwrap_or(&[]);
            let prev = chars.get(start..i).unwrap_or(&[]);
            let Some(cell) = cells.get_mut(i) else { break };
            if let Some(kind) = cell.piece_code {
                if isolated || notation_after(next) || promotion_before(prev) {
                    cell.ch = figurine(kind, FigurineSet::Outline);
                }
            } else if cell.odd_code && !(is_file(cell.ch) || is_capture(cell.ch) || cell.ch.is_ascii_digit()) && notation_after(next) {
                stats.unresolved = stats.unresolved.saturating_add(1);
            }
        }
        start = end.saturating_add(1);
    }
}
