//! The side to move from the move number under a diagram.
//!
//! In a typeset (born-digital) book the game goes on under the diagram, and the number of its
//! first move is set in bold: **23.** Nf3 says White is to move in the diagram, **23...** Rxe4
//! says Black is. [`hints`] finds that number for each diagram of a page:
//!
//! - in the lines below the diagram that share its column (they overlap it horizontally), in
//!   reading order, up to the next diagram below it;
//! - the first move number set in a bold font and followed by a move (`23.` alone, a heading
//!   number or a page number, is not a move number);
//! - a number in a regular font is passed over: in prose ("after 23.Nf3 the knight…") it says
//!   nothing about the diagram.
//!
//! Bold is read from the font name (`Times-Bold`, `MinionPro-Semibold`, `Arial,Bold`, …). A scan
//! has no fonts, so this only ever helps the text layer of a born-digital PDF. The caller weighs
//! the hint against the position ([`pdfcraft_chess::recognize::side::infer_side_to_move`]): when
//! the position proves the other side, the position wins and the conflict is flagged.

use pdfcraft_chess::placement::Color;
use pdfcraft_render::text::PageText;

use crate::chess_text::{ChessPageText, ChessRun};
use crate::coords::ViewRect;

/// How far a line may start above a diagram's bottom edge and still count as below it, in points
/// (the descenders of the file letters printed under the board).
const BELOW_TOLERANCE: f32 = 2.0;

/// What the first bold move number under a diagram says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MoveNumberHint {
    /// The side to move in the diagram: White for `23.`, Black for `23...`.
    pub color: Color,
    /// The move number, the FEN's fullmove number.
    pub number: u16,
}

/// Whether a font name says the font is bold (or heavier).
pub fn is_bold_font(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    // Strip a subset prefix (`ABCDEF+`), then look at the style part of the name.
    let base = lower.rsplit('+').next().unwrap_or(&lower);
    ["bold", "black", "heavy", "demi"].iter().any(|w| base.contains(w)) || (base.ends_with("bd") && base.len() > 4)
}

/// For each diagram (its box in view space, in the page's reading order), the first bold move
/// number under it, when there is one.
pub fn hints(text: &PageText, chess: &ChessPageText, diagrams: &[ViewRect]) -> Vec<Option<MoveNumberHint>> {
    diagrams.iter().enumerate().map(|(i, d)| hint_for(text, chess, diagrams, i, *d)).collect()
}

fn overlaps_horizontally(a: &ViewRect, b: &ViewRect) -> bool {
    a.x0 < b.x1 && b.x0 < a.x1
}

fn hint_for(text: &PageText, chess: &ChessPageText, diagrams: &[ViewRect], index: usize, diagram: ViewRect) -> Option<MoveNumberHint> {
    // The text under this diagram stops where the next diagram of its column starts.
    let stop = diagrams
        .iter()
        .enumerate()
        .filter(|(j, e)| *j != index && e.y0 >= diagram.y1 - BELOW_TOLERANCE && overlaps_horizontally(&diagram, e))
        .map(|(_, e)| e.y0)
        .fold(f32::INFINITY, f32::min);
    for line in &chess.lines {
        let r = line.rect;
        if r.y0 < diagram.y1 - BELOW_TOLERANCE || r.y0 >= stop || !overlaps_horizontally(&diagram, &r) {
            continue;
        }
        for (k, span) in line.spans.iter().enumerate() {
            let ChessRun::MoveNumber { number, black } = span.run else { continue };
            let followed_by_move = line.spans.get(k + 1).is_some_and(|s| matches!(s.run, ChessRun::Move { .. }));
            if followed_by_move && is_bold(text, span.rect) {
                return Some(MoveNumberHint { color: if black { Color::Black } else { Color::White }, number });
            }
        }
    }
    None
}

/// Whether the digits inside `rect` are set in a bold font (most of them, when fonts are mixed).
fn is_bold(text: &PageText, rect: ViewRect) -> bool {
    let inside = |g: &&pdfcraft_render::text::TextGlyph| {
        let (cx, cy) = ((g.rect[0] + g.rect[2]) / 2.0, (g.rect[1] + g.rect[3]) / 2.0);
        cx >= rect.x0 && cx <= rect.x1 && cy >= rect.y0 && cy <= rect.y1 && g.text.chars().all(|c| c.is_ascii_digit())
    };
    let (mut bold, mut total) = (0, 0);
    for g in text.glyphs.iter().filter(inside) {
        total += 1;
        if g.font.as_deref().is_some_and(is_bold_font) {
            bold += 1;
        }
    }
    total > 0 && bold * 2 > total
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use pdfcraft_render::text::TextGlyph;

    use super::*;
    use crate::chess_text::from_page_text;

    const W: f32 = 5.0;
    const LINE: f32 = 12.0;
    const BODY: &str = "Times-Roman";
    const BOLD: &str = "ABCDEF+Times-Bold";

    /// A laid-out page built glyph by glyph, lines starting at `top`.
    struct Page {
        text: PageText,
        x: f32,
        top: f32,
        line: u32,
        space: bool,
    }

    impl Page {
        fn new(top: f32) -> Self {
            Page { text: PageText::default(), x: 0.0, top, line: 0, space: false }
        }

        fn text_in(mut self, s: &str, font: &str) -> Self {
            for c in s.chars() {
                if c == ' ' {
                    self.space = true;
                    self.x += W;
                    continue;
                }
                let y = self.top + self.line as f32 * LINE;
                let rect = [self.x, y, self.x + W, y + 10.0];
                let quad = [[rect[0], rect[3]], [rect[2], rect[3]], [rect[2], rect[1]], [rect[0], rect[1]]];
                let code = u32::from(c);
                let glyph = TextGlyph::new(c.to_string(), rect, quad, [1.0, 0.0]).with_source(Some(Arc::from(font)), (code <= 0xFF).then_some(code));
                self.text.glyphs.push(glyph);
                self.text.line_of.push(self.line);
                self.text.space_before.push(std::mem::take(&mut self.space));
                self.x += W;
            }
            self
        }

        fn bold(self, s: &str) -> Self {
            self.text_in(s, BOLD)
        }

        fn body(self, s: &str) -> Self {
            self.text_in(s, BODY)
        }

        fn newline(mut self) -> Self {
            self.line += 1;
            self.x = 0.0;
            self.space = false;
            self
        }

        fn hints(&self, diagrams: &[ViewRect]) -> Vec<Option<MoveNumberHint>> {
            hints(&self.text, &from_page_text(&self.text, None), diagrams)
        }
    }

    /// A diagram from x 0 to 150, from y 0 to 150: the text starts under it at y 160.
    fn diagram() -> ViewRect {
        ViewRect::new(0.0, 0.0, 150.0, 150.0)
    }

    #[test]
    fn bold_font_names() {
        for name in [
            "Times-Bold",
            "ABCDEF+MinionPro-Semibold",
            "Arial,Bold",
            "TimesNewRomanPS-BoldMT",
            "Helvetica-BoldOblique",
            "FuturaBT-Heavy",
            "GaramondBd",
        ] {
            assert!(is_bold_font(name), "{name}");
        }
        for name in ["Times-Roman", "MinionPro-Regular", "Arial", "Helvetica-Oblique", "ChessMerida", "Times-Italic"] {
            assert!(!is_bold_font(name), "{name}");
        }
    }

    #[test]
    fn a_bold_white_move_number_says_white() {
        let page = Page::new(160.0).bold("23.").body("Nf3 Nc6 24.Bb5");
        assert_eq!(page.hints(&[diagram()]), vec![Some(MoveNumberHint { color: Color::White, number: 23 })]);
    }

    #[test]
    fn a_bold_black_move_number_says_black() {
        for dots in ["...", "…"] {
            let page = Page::new(160.0).bold(&format!("31{dots}")).body("Rxe4 32.Qd2");
            assert_eq!(page.hints(&[diagram()]), vec![Some(MoveNumberHint { color: Color::Black, number: 31 })], "{dots}");
        }
    }

    #[test]
    fn regular_move_numbers_and_prose_say_nothing() {
        let page = Page::new(160.0).body("After 23...Rxe4 the game was over.");
        assert_eq!(page.hints(&[diagram()]), vec![None]);
        // A bold number that no move follows is a heading, not the game.
        let heading = Page::new(160.0).bold("12.").body(" The endgame");
        assert_eq!(heading.hints(&[diagram()]), vec![None]);
    }

    #[test]
    fn the_first_bold_number_wins_after_prose() {
        let page = Page::new(160.0).body("The game went on:").newline().bold("17...").body("Qh4 18.g3").newline().bold("19.").body("Nf3");
        assert_eq!(page.hints(&[diagram()]), vec![Some(MoveNumberHint { color: Color::Black, number: 17 })]);
    }

    #[test]
    fn text_above_or_beside_the_diagram_does_not_count() {
        // The text ends above the diagram, which starts at y 40.
        let above = Page::new(0.0).bold("23.").body("Nf3 Nc6");
        assert_eq!(above.hints(&[ViewRect::new(0.0, 40.0, 150.0, 190.0)]), vec![None]);
        // The text is in another column.
        let beside = Page::new(160.0).bold("23.").body("Nf3 Nc6");
        assert_eq!(beside.hints(&[ViewRect::new(300.0, 0.0, 450.0, 150.0)]), vec![None]);
    }

    #[test]
    fn each_diagram_reads_its_own_text() {
        // Diagram one, its text, diagram two, its text.
        let page = Page::new(160.0).bold("10.").body("e4 e5").newline().newline().newline().newline().bold("40...").body("Kf7 41.Rd7+");
        let second = ViewRect::new(0.0, 180.0, 150.0, 205.0);
        let hints = page.hints(&[diagram(), second]);
        assert_eq!(hints, vec![Some(MoveNumberHint { color: Color::White, number: 10 }), Some(MoveNumberHint { color: Color::Black, number: 40 })]);
    }

    #[test]
    fn the_next_diagram_ends_the_text_under_the_first() {
        // The only bold number is under the second diagram.
        let page = Page::new(160.0).body("A quiet move.").newline().newline().newline().newline().bold("40...").body("Kf7");
        let second = ViewRect::new(0.0, 180.0, 150.0, 205.0);
        assert_eq!(page.hints(&[diagram(), second]), vec![None, Some(MoveNumberHint { color: Color::Black, number: 40 })]);
    }
}
