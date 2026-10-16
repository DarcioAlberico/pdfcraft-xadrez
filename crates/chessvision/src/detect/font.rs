//! Diagrams set in a chess diagram font, read straight from the text layer.
//!
//! In most publisher PDFs a diagram is eight lines of text in a font such as Chess Merida: each
//! character is one square, its letter says which piece and its case which square colour. The
//! position is in the file, so it is read exactly, with no model and no rasterizing.
//! [`pdfcraft_chess::catalog`] says what each character of each family means.
//!
//! The glyphs are placed on a grid by position, not by order in the text: extraction may drop a
//! space glyph or emit the ranks in another order, and the geometry still says which square a
//! glyph is on. A block is a board when eight rows of square glyphs line up on one grid. When the
//! glyphs carry square colours, their checker pattern must agree with a chessboard's (the top
//! left square is light from either side), or the block is rejected.
//!
//! Only families whose layout was checked ([`Family::readable`]) are read. An unverified family
//! is skipped unless the caller opts in.

use pdfcraft_chess::catalog::{Family, GlyphKind, SquareColor, lookup_family};
use pdfcraft_chess::placement::{Piece, Placement};

use super::{DiagramCandidate, Source};
use crate::coords::{PageGeometry, ViewRect};

/// One glyph of the text layer, as the engine hands it over from the renderer's text extraction.
#[derive(Clone, Debug, PartialEq)]
pub struct FontGlyph {
    /// The font's name, subset prefix removed (`ChessMerida`).
    pub font: String,
    /// The character code the content stream showed, when it is a one-byte code.
    pub code: Option<u32>,
    /// The Unicode text the extractor produced, used when there is no code.
    pub text: String,
    /// The glyph's box in view space (points, y down).
    pub rect: ViewRect,
}

/// Options for [`candidates`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// Also read families whose layout was not checked against the font.
    pub allow_unverified: bool,
}

/// Most glyphs looked at on one page.
pub const MAX_GLYPHS: usize = 200_000;
/// Two glyph centres are on one row when their vertical distance is below this share of the
/// glyph height.
const ROW_TOLERANCE: f32 = 0.35;
/// A glyph sits on a grid column when its centre is within this share of a square from it.
const COLUMN_TOLERANCE: f32 = 0.30;
/// Rows of a board are one square apart, within this share of a square.
const ROW_STEP_TOLERANCE: f32 = 0.25;
/// Fewest square glyphs a rank must show; the rest are taken as empty squares whose glyph the
/// extractor dropped.
const MIN_CELLS_PER_ROW: usize = 4;

/// A glyph that stands on a square, with what it shows.
#[derive(Clone, Copy, Debug)]
struct Cell {
    cx: f32,
    cy: f32,
    w: f32,
    h: f32,
    piece: Option<Piece>,
    square: Option<SquareColor>,
}

/// Diagrams set in a chess diagram font on one page.
pub fn candidates(glyphs: &[FontGlyph], page: &PageGeometry, options: Options) -> Vec<DiagramCandidate> {
    let mut by_font: Vec<(&'static Family, Vec<Cell>)> = Vec::new();
    for g in glyphs.iter().take(MAX_GLYPHS) {
        let Some(family) = lookup_family(&g.font) else { continue };
        if !(family.readable() || (options.allow_unverified && family.kind == pdfcraft_chess::catalog::FontKind::Diagram)) {
            continue;
        }
        // The Unicode family needs no special font; only glyphs of a real chess font count here,
        // so prose with an occasional ♔ in a text font is not read as a board.
        if family.key == "unicode" {
            continue;
        }
        let Some(c) = glyph_char(g) else { continue };
        let Some(role) = family.role(c) else { continue };
        if !role.occupies_cell() {
            continue;
        }
        let (w, h) = (g.rect.width(), g.rect.height());
        if !(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0) {
            continue;
        }
        let piece = match role.kind {
            GlyphKind::Piece(p) => Some(p),
            _ => None,
        };
        let cell = Cell { cx: (g.rect.x0 + g.rect.x1) / 2.0, cy: (g.rect.y0 + g.rect.y1) / 2.0, w, h, piece, square: role.square };
        match by_font.iter_mut().find(|(f, _)| f.key == family.key) {
            Some((_, cells)) => cells.push(cell),
            None => by_font.push((family, vec![cell])),
        }
    }
    let mut out = Vec::new();
    for (_, cells) in by_font {
        for board in boards(&cells) {
            if let Some(user) = page.view_to_user(board.rect) {
                out.push(DiagramCandidate {
                    bbox_user: user,
                    source: Source::Font,
                    score: board.score,
                    // The position is exact; any resolution will do for display.
                    dpi_hint: 150.0,
                    placement: Some(board.placement),
                });
            }
        }
    }
    out
}

/// The character the catalogue is indexed by: the one-byte code when there is one, else the text.
fn glyph_char(g: &FontGlyph) -> Option<char> {
    match g.code {
        Some(code) if code <= 0xFF => char::from_u32(code),
        _ => {
            let mut chars = g.text.chars();
            let c = chars.next()?;
            chars.next().is_none().then_some(c)
        }
    }
}

struct Board {
    rect: ViewRect,
    placement: Placement,
    score: f32,
}

fn median(mut v: Vec<f32>) -> Option<f32> {
    v.sort_by(f32::total_cmp);
    v.get(v.len() / 2).copied()
}

/// Group cells into rows by their centre's height.
fn rows(cells: &[Cell]) -> Vec<Vec<Cell>> {
    let mut sorted: Vec<Cell> = cells.to_vec();
    sorted.sort_by(|a, b| a.cy.total_cmp(&b.cy));
    let mut rows: Vec<Vec<Cell>> = Vec::new();
    for c in sorted {
        let same = rows.last().and_then(|r| r.first()).is_some_and(|first| (c.cy - first.cy).abs() <= ROW_TOLERANCE * first.h.max(c.h));
        match rows.last_mut() {
            Some(row) if same => row.push(c),
            _ => rows.push(vec![c]),
        }
    }
    for row in &mut rows {
        row.sort_by(|a, b| a.cx.total_cmp(&b.cx));
    }
    rows
}

/// Split a row into runs of cells one square apart (a line can hold two boards side by side).
fn runs(row: &[Cell], square: f32) -> Vec<Vec<Cell>> {
    let mut runs: Vec<Vec<Cell>> = Vec::new();
    for c in row {
        let joins = runs.last().and_then(|r| r.last()).is_some_and(|prev| c.cx - prev.cx <= square * (1.0 + COLUMN_TOLERANCE) * 2.0);
        match runs.last_mut() {
            Some(run) if joins => run.push(*c),
            _ => runs.push(vec![*c]),
        }
    }
    runs
}

/// Boards among the square glyphs of one font.
fn boards(cells: &[Cell]) -> Vec<Board> {
    let Some(square) = median(cells.iter().map(|c| c.w).collect()) else { return Vec::new() };
    if square <= 0.0 || !square.is_finite() {
        return Vec::new();
    }
    // Every run of square glyphs, with its row's height.
    let mut lines: Vec<Vec<Cell>> = Vec::new();
    for row in rows(cells) {
        lines.extend(runs(&row, square).into_iter().filter(|r| r.len() >= MIN_CELLS_PER_ROW && r.len() <= 8));
    }
    lines.sort_by(|a, b| {
        let ya = a.first().map_or(0.0, |c| c.cy);
        let yb = b.first().map_or(0.0, |c| c.cy);
        ya.total_cmp(&yb)
    });
    let mut used = vec![false; lines.len()];
    let mut boards = Vec::new();
    for start in 0..lines.len() {
        if used.get(start).copied().unwrap_or(true) {
            continue;
        }
        // Collect eight lines one square apart, starting here, sharing a left edge.
        let Some(first) = lines.get(start) else { continue };
        let left = first.first().map_or(0.0, |c| c.cx);
        let mut chosen = vec![start];
        let mut y = first.first().map_or(0.0, |c| c.cy);
        for (i, line) in lines.iter().enumerate().skip(start + 1) {
            if chosen.len() == 8 {
                break;
            }
            if used.get(i).copied().unwrap_or(true) {
                continue;
            }
            let Some(c0) = line.first() else { continue };
            let step = c0.cy - y;
            // Loose on purpose: the first line may lack its leftmost glyph. `read_block` checks
            // the grid exactly once the eight lines are known.
            let reach = 7.0 * square + square * COLUMN_TOLERANCE;
            let aligned = line.iter().all(|c| c.cx >= left - reach && c.cx <= left + reach);
            if (step - square).abs() <= square * ROW_STEP_TOLERANCE && aligned {
                chosen.push(i);
                y = c0.cy;
            } else if step > square * (1.0 + ROW_STEP_TOLERANCE) {
                break;
            }
        }
        if chosen.len() != 8 {
            continue;
        }
        let rows: Vec<&Vec<Cell>> = chosen.iter().filter_map(|&i| lines.get(i)).collect();
        // The grid's left column: the leftmost glyph of the block.
        let x0 = rows.iter().filter_map(|r| r.first()).map(|c| c.cx).fold(f32::INFINITY, f32::min);
        let top = rows.first().and_then(|r| r.first()).map_or(0.0, |c| c.cy);
        if let Some(board) = read_block(&rows, x0, top, square) {
            for &i in &chosen {
                if let Some(u) = used.get_mut(i) {
                    *u = true;
                }
            }
            boards.push(board);
        }
    }
    boards
}

/// Read eight rows of cells on a grid whose first column centre is `x0`.
fn read_block(rows: &[&Vec<Cell>], x0: f32, top: f32, square: f32) -> Option<Board> {
    let mut cells: [Option<Piece>; 64] = [None; 64];
    let mut colours_checked = 0usize;
    let mut colours_wrong = 0usize;
    let mut known = 0usize;
    for (r, row) in rows.iter().enumerate() {
        let mut seen = [false; 8];
        for c in row.iter() {
            let col = ((c.cx - x0) / square).round();
            if !(0.0..8.0).contains(&col) || ((c.cx - x0) / square - col).abs() > COLUMN_TOLERANCE {
                return None;
            }
            let col = col as usize;
            if std::mem::replace(seen.get_mut(col)?, true) {
                // Two glyphs on one square: not a diagram line.
                return None;
            }
            *cells.get_mut(r * 8 + col)? = c.piece;
            known += 1;
            if let Some(colour) = c.square {
                colours_checked += 1;
                // Reading order: the top left square is light from either side of the board.
                let light = (r + col).is_multiple_of(2);
                if (colour == SquareColor::Light) != light {
                    colours_wrong += 1;
                }
            }
        }
    }
    if colours_checked > 0 && colours_wrong * 10 > colours_checked {
        return None;
    }
    let placement = Placement::from_reading_order(&cells);
    let half = square / 2.0;
    let rect = ViewRect::new(x0 - half, top - half, x0 + 7.0 * square + half, top + 7.0 * square + half);
    // Full rows with checked colours are as sure as a reading gets; missing glyphs cost a little.
    let score = 0.8 + 0.2 * (known as f32 / 64.0);
    // Legality is the caller's to judge, but an empty grid is a frame, not a position.
    if placement.is_empty() {
        return None;
    }
    Some(Board { rect, placement, score })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdfcraft_chess::fen::format_placement;
    use pdfcraft_geom::{PageRect, Rotation};

    fn page() -> PageGeometry {
        PageGeometry::new(PageRect::new(0.0, 0.0, 600.0, 800.0), Rotation::R0).unwrap()
    }

    /// Lines of a Merida diagram, top rank first, each character one 20 pt square, from (100, 100).
    fn merida(lines: [&str; 8], font: &str) -> Vec<FontGlyph> {
        let mut out = Vec::new();
        for (r, line) in lines.iter().enumerate() {
            for (c, ch) in line.chars().enumerate() {
                let x = 100.0 + c as f32 * 20.0;
                let y = 100.0 + r as f32 * 20.0;
                out.push(FontGlyph {
                    font: font.to_owned(),
                    code: Some(ch as u32),
                    text: ch.to_string(),
                    rect: ViewRect::new(x, y, x + 20.0, y + 20.0),
                });
            }
        }
        out
    }

    // The initial position in Marroquin's layout: light squares lower case, dark squares upper
    // case, black pieces o m v t w l, white p n b r q k; empty light ' ', empty dark '+'.
    const START: [&str; 8] = ["tMvWlVmT", "OoOoOoOo", " + + + +", "+ + + + ", " + + + +", "+ + + + ", "pPpPpPpP", "RnBqKbNr"];

    #[test]
    fn a_merida_diagram_is_read_exactly() {
        let found = candidates(&merida(START, "ABCDEF+ChessMerida"), &page(), Options::default());
        assert_eq!(found.len(), 1);
        let c = &found[0];
        assert_eq!(c.source, Source::Font);
        assert_eq!(format_placement(c.placement.as_ref().unwrap()), "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR");
        // 160 × 160 pt from (100, 100) in view space; y flips into user space on an 800 pt page.
        assert!((c.bbox_user.x0 - 100.0).abs() < 0.01 && (c.bbox_user.x1 - 260.0).abs() < 0.01, "{:?}", c.bbox_user);
        assert!((c.bbox_user.y0 - 540.0).abs() < 0.01 && (c.bbox_user.y1 - 700.0).abs() < 0.01, "{:?}", c.bbox_user);
    }

    #[test]
    fn dropped_space_glyphs_are_empty_squares() {
        let mut glyphs = merida(START, "ChessMerida");
        glyphs.retain(|g| g.text != " ");
        let found = candidates(&glyphs, &page(), Options::default());
        assert_eq!(found.len(), 1);
        assert_eq!(format_placement(found[0].placement.as_ref().unwrap()), "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR");
    }

    #[test]
    fn ranks_in_any_text_order_still_make_one_board() {
        let mut glyphs = merida(START, "ChessMerida");
        glyphs.reverse();
        assert_eq!(candidates(&glyphs, &page(), Options::default()).len(), 1);
    }

    #[test]
    fn a_wrong_checker_pattern_is_not_a_board() {
        // Every row shifted one square: light-square glyphs land on dark squares.
        let shifted: [&str; 8] = ["MvWlVmT+", "oOoOoOo ", "+ + + + ", " + + + +", "+ + + + ", " + + + +", "PpPpPpP ", "nBqKbNr "];
        let mut glyphs = merida(shifted, "ChessMerida");
        // Keep the grid's left edge where it was by dropping nothing; the colours are what fail.
        glyphs.retain(|g| g.text != " ");
        assert!(candidates(&glyphs, &page(), Options::default()).is_empty());
    }

    #[test]
    fn alpha_reads_n_as_the_black_bishop() {
        // Black king g8 (light), black bishop d1 (light: Alpha's `n`), white king e1 (dark).
        let lines: [&str; 8] = [" + + +l+", "+ + + + ", " + + + +", "+ + + + ", " + + + +", "+ + + + ", " + + + +", "+ +nK + "];
        let found = candidates(&merida(lines, "ChessAlpha"), &page(), Options::default());
        assert_eq!(format_placement(found[0].placement.as_ref().unwrap()), "6k1/8/8/8/8/8/8/3bK3");
    }

    #[test]
    fn unverified_families_need_opting_in() {
        let glyphs = merida(START, "ChessBerlin");
        assert!(candidates(&glyphs, &page(), Options::default()).is_empty());
        assert_eq!(candidates(&glyphs, &page(), Options { allow_unverified: true }).len(), 1);
    }

    #[test]
    fn figurine_and_text_fonts_are_not_boards() {
        assert!(candidates(&merida(START, "SemFig"), &page(), Options::default()).is_empty());
        assert!(candidates(&merida(START, "Times-Roman"), &page(), Options::default()).is_empty());
    }

    #[test]
    fn two_boards_side_by_side() {
        let mut glyphs = merida(START, "ChessMerida");
        for g in merida(START, "ChessMerida") {
            glyphs.push(FontGlyph { rect: ViewRect::new(g.rect.x0 + 250.0, g.rect.y0, g.rect.x1 + 250.0, g.rect.y1), ..g });
        }
        assert_eq!(candidates(&glyphs, &page(), Options::default()).len(), 2);
    }

    #[test]
    fn junk_does_not_panic() {
        let junk: Vec<FontGlyph> = (0..500)
            .map(|i| {
                let x = (i * 37 % 600) as f32;
                let y = (i * 53 % 800) as f32;
                let w = if i % 7 == 0 { f32::NAN } else { (i % 5) as f32 };
                FontGlyph {
                    font: "ChessMerida".into(),
                    code: Some((i * 13 % 300) as u32),
                    text: String::new(),
                    rect: ViewRect::new(x, y, x + w, y + w),
                }
            })
            .collect();
        let _ = candidates(&junk, &page(), Options::default());
    }
}
