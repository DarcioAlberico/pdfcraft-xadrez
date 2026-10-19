//! Synthetic text layers: glyphs built directly, as `render` would hand them over.

use std::sync::Arc;

use pdfcraft_chess::placement::PieceKind;
use pdfcraft_chess::san::{Lang, San};
use pdfcraft_render::text::{PageText, TextGlyph};

use super::*;
use crate::coords::{PageGeometry, PageRect, Rotation};

/// Advance of one glyph, in points.
const W: f32 = 5.0;
/// Line pitch, in points.
const LINE: f32 = 12.0;
/// The body font of the synthetic pages.
const BODY: &str = "Times-Roman";

/// Builds a laid-out page glyph by glyph: lines, word gaps and fonts are explicit, so the tests
/// do not depend on the layout heuristics.
#[derive(Default)]
struct Page {
    text: PageText,
    x: f32,
    line: u32,
    space: bool,
}

impl Page {
    fn new() -> Self {
        Self::default()
    }

    /// One glyph with its own text, font and code.
    fn glyph(mut self, text: &str, font: &str, code: Option<u32>) -> Self {
        let y = self.line as f32 * LINE;
        let rect = [self.x, y, self.x + W, y + 10.0];
        let quad = [[rect[0], rect[3]], [rect[2], rect[3]], [rect[2], rect[1]], [rect[0], rect[1]]];
        let font: Option<Arc<str>> = Some(Arc::from(font));
        self.text.glyphs.push(TextGlyph::new(text, rect, quad, [1.0, 0.0]).with_source(font, code));
        self.text.line_of.push(self.line);
        self.text.space_before.push(std::mem::take(&mut self.space));
        self.x += W;
        self
    }

    /// Text in `font`, one glyph per character, each with its character as code. A space is a
    /// word gap, not a glyph.
    fn text_in(mut self, s: &str, font: &str) -> Self {
        for c in s.chars() {
            if c == ' ' {
                self.space = true;
                self.x += W;
            } else {
                let code = u32::from(c);
                self = self.glyph(&c.to_string(), font, (code <= 0xFF).then_some(code));
            }
        }
        self
    }

    fn text(self, s: &str) -> Self {
        self.text_in(s, BODY)
    }

    fn newline(mut self) -> Self {
        self.line += 1;
        self.x = 0.0;
        self.space = false;
        self
    }

    fn read(&self, hint: Option<Lang>) -> ChessPageText {
        from_page_text(&self.text, hint)
    }
}

fn runs(page: &ChessPageText) -> Vec<Vec<ChessRun>> {
    page.lines.iter().map(|l| l.spans.iter().map(|s| s.run.clone()).collect()).collect()
}

fn mv(english: &str, text: &str) -> ChessRun {
    ChessRun::Move { san: San::read(english, Lang::En).unwrap(), text: text.to_owned() }
}

fn white(number: u16) -> ChessRun {
    ChessRun::MoveNumber { number, black: false }
}

fn black(number: u16) -> ChessRun {
    ChessRun::MoveNumber { number, black: true }
}

fn text(s: &str) -> ChessRun {
    ChessRun::Text(s.to_owned())
}

/// The assumed figurine layout: SemFig, a catalogued figurine family, draws its knight under code
/// 0x4E (`N`), as the catalogue's figurine layout says. The test's `ToUnicode` map is broken and
/// extracts that glyph as the two characters `l0`. The code, not the text, says what it is.
#[test]
fn figurine_font_code_reads_as_a_knight() {
    let page = Page::new().text("2.").glyph("l0", "SemFig", Some(0x4E)).text("xd4");
    let read = page.read(None);
    assert_eq!(runs(&read), vec![vec![white(2), mv("Nxd4", "♘xd4")]]);
    assert_eq!(read.quality, TextQuality::Good);
    // The move's box starts at the figurine glyph, the third glyph of the line.
    let span = read.lines[0].spans.get(1).unwrap();
    assert_eq!(span.rect, ViewRect::new(2.0 * W, 0.0, 6.0 * W, 10.0));
}

/// The same codes in a figurine font, in a sentence: no notation follows, so they stay letters.
#[test]
fn figurine_font_prose_stays_text() {
    let read = Page::new().text_in("Black's threat", "SemFig").read(None);
    assert_eq!(runs(&read), vec![vec![text("Black's threat")]]);
    // A lone figurine-font letter is a piece.
    let read = Page::new().text("the ").text_in("B", "SemFig").text(" pair").read(None);
    assert_eq!(runs(&read), vec![vec![text("the"), ChessRun::Figurine(PieceKind::Bishop), text("pair")]]);
}

/// A figurine font with its symbol cmap: the codes come in the Windows symbol range.
#[test]
fn figurine_codes_in_the_symbol_range_are_folded() {
    let page = Page::new().glyph("\u{F051}", "FigurineCB-Time", Some(0xF051)).text("h5+");
    assert_eq!(runs(&page.read(None)), vec![vec![mv("Qh5+", "♕h5+")]]);
    // With a dead map only the code is left, still in the symbol range.
    let page = Page::new().glyph("", "FigurineCB-Time", Some(0xF04E)).text("f3");
    assert_eq!(runs(&page.read(None)), vec![vec![mv("Nf3", "♘f3")]]);
}

/// A subset figurine font renumbers its glyphs (Dvoretsky's SemFig draws `K` under code 46, `.`):
/// the code names no piece, so the extracted letter decides, and the page stays Good instead of
/// being handed to OCR.
#[test]
fn subset_figurine_code_falls_back_to_the_text() {
    let read = Page::new().text("1.").glyph("K", "SemFigBold", Some(46)).text("g8!! ").glyph("K", "SemFigNormal", Some(46)).text("xf5").read(None);
    assert_eq!(runs(&read), vec![vec![white(1), mv("Kg8", "♔g8"), ChessRun::Nag(3), mv("Kxf5", "♔xf5")]]);
    assert_eq!(read.quality, TextQuality::Good);
}

/// Move numbers with no full stop, as some books print them (`19 Rxd5`, Black's `15 … Rxb6`); a
/// number in prose is not one.
#[test]
fn bare_move_numbers_before_moves() {
    let read = Page::new().text("15 … Rxb6 16 Nd5 Nxd5").read(Some(Lang::En));
    assert_eq!(runs(&read), vec![vec![black(15), mv("Rxb6", "Rxb6"), white(16), mv("Nd5", "Nd5"), mv("Nxd5", "Nxd5")]]);
    // After an opening quote.
    let read = Page::new().text("line: “57 Rh7 c6").read(Some(Lang::En));
    assert_eq!(runs(&read), vec![vec![text("line: “"), white(57), mv("Rh7", "Rh7"), mv("c6", "c6")]]);
    let read = Page::new().text("in 1968 he won 20 games").read(Some(Lang::En));
    assert_eq!(runs(&read), vec![vec![text("in 1968 he won 20 games")]]);
    // A number after a move, before prose, is not one either.
    let read = Page::new().text("he played Nf3 2 times").read(Some(Lang::En));
    assert_eq!(runs(&read), vec![vec![text("he played"), mv("Nf3", "Nf3"), text("2 times")]]);
}

/// An earlier OCR program's layer over a scanned book, its figurines read as stray symbols:
/// the layer is poor, so chess OCR reads the page again. Ordinary prose and moves are not.
#[test]
fn stray_symbols_for_pieces_make_the_layer_poor() {
    let read = Page::new().text("49...f6 50.&xe5 ®f7 51.^d5 §fl and the extra material decides.").read(Some(Lang::En));
    assert_eq!(read.quality, TextQuality::Poor { reason: PoorReason::GarbledPieces { words: 4 } });
    // Two are not enough, and moves, quotes, ellipses and brackets are not stray symbols.
    let read = Page::new().text("50.&xe5 ®f7 then 51.Nd5 …e5 (e4) “Qf3” –e5 ×d5 +f7 and 52.Kxd5 – a draw.").read(Some(Lang::En));
    assert_eq!(read.quality, TextQuality::Good);
}

/// Books (and OCR, which reads it as a cross) print check as `†` and mate as `‡`.
#[test]
fn a_dagger_after_a_move_is_check() {
    let read = Page::new().text("47...f6✝!! 48.Kxf6 Qa1† 49.Qh7‡").read(Some(Lang::En));
    let sans: Vec<String> =
        read.lines[0].spans.iter().filter_map(|s| if let ChessRun::Move { san, .. } = &s.run { Some(san.to_english()) } else { None }).collect();
    assert_eq!(sans, ["f6+", "Kxf6", "Qa1+", "Qh7#"]);
    // In prose it stays a dagger.
    let read = Page::new().text("Rubinstein † 1961").read(Some(Lang::En));
    assert_eq!(runs(&read), vec![vec![text("Rubinstein † 1961")]]);
}

#[test]
fn novelty_after_a_move_is_nag_146() {
    let read = Page::new().text("12.Nf4N").read(None);
    assert_eq!(runs(&read), vec![vec![white(12), mv("Nf4", "Nf4"), ChessRun::Nag(146)]]);
}

#[test]
fn publisher_superscript_three_is_nag_15() {
    let read = Page::new().text("Rad8³").read(None);
    assert_eq!(runs(&read), vec![vec![mv("Rad8", "Rad8"), ChessRun::Nag(15)]]);
    // The NAG's box is the superscript's glyph, the move's the four before it.
    let spans = &read.lines[0].spans;
    assert_eq!(spans[0].rect, ViewRect::new(0.0, 0.0, 4.0 * W, 10.0));
    assert_eq!(spans[1].rect, ViewRect::new(4.0 * W, 0.0, 5.0 * W, 10.0));
    // A three-character alias is one NAG over its three glyphs; `¹` glued to a move is a footnote.
    let read = Page::new().text("Nf3¹ +/=").read(None);
    assert_eq!(runs(&read), vec![vec![mv("Nf3", "Nf3"), text("¹"), ChessRun::Nag(14)]]);
    assert_eq!(read.lines[0].spans[2].rect, ViewRect::new(5.0 * W, 0.0, 8.0 * W, 10.0));
}

#[test]
fn informator_symbol_font_signs_are_mapped() {
    // `¢` in an Informator font is ⩲, White slightly better ($14).
    let page = Page::new().text("Nf3").text_in("¢", "Informator-Symbols");
    assert_eq!(runs(&page.read(None)), vec![vec![mv("Nf3", "Nf3"), ChessRun::Nag(14)]]);
}

#[test]
fn annotations_and_checks_split() {
    let read = Page::new().text("23.Rxe8+! Kxe8?? 24.Qh5+- ±").read(None);
    assert_eq!(
        runs(&read),
        vec![vec![
            white(23),
            mv("Rxe8+", "Rxe8+"),
            ChessRun::Nag(1),
            mv("Kxe8", "Kxe8"),
            ChessRun::Nag(4),
            white(24),
            mv("Qh5", "Qh5"),
            ChessRun::Nag(18),
            ChessRun::Nag(16),
        ]]
    );
}

#[test]
fn portuguese_letters_are_detected() {
    let read = Page::new().text("1.e4 e5 2.Cf3 Cc6").read(None);
    assert_eq!(read.lang.lang, Lang::Pt);
    assert!(read.lang.confidence > 0.0);
    assert_eq!(runs(&read), vec![vec![white(1), mv("e4", "e4"), mv("e5", "e5"), white(2), mv("Nf3", "Cf3"), mv("Nc6", "Cc6")]]);
}

#[test]
fn german_letters_are_detected() {
    let read = Page::new().text("1.e4 e5 2.Sf3 Sc6 3.Lb5").read(None);
    assert_eq!(read.lang.lang, Lang::De);
    assert_eq!(runs(&read)[0].get(4), Some(&mv("Nf3", "Sf3")));
    assert_eq!(runs(&read)[0].get(7), Some(&mv("Bb5", "Lb5")));
    // English letters stay English, and the bishop tells it from Romanian (`B` is not theirs).
    let read = Page::new().text("1.e4 e5 2.Nf3 Nc6 3.Bb5").read(None);
    assert_eq!(read.lang.lang, Lang::En);
}

#[test]
fn a_hint_wins_only_without_contrary_evidence() {
    // Two words are too few to overrule the document's language.
    let read = Page::new().text("2.Cf3 Cc6").read(Some(Lang::Es));
    assert_eq!(read.lang.lang, Lang::Es);
    // Many German moves overrule an English hint.
    let read = Page::new().text("1.Sf3 Sf6 2.Sc3 Sc6 3.Sd5").read(Some(Lang::En));
    assert_eq!(read.lang.lang, Lang::De);
}

#[test]
fn figurines_count_for_every_language() {
    let mut tally = LangTally::default();
    tally.merge(&Page::new().text("1.♘f3 ♞f6").read(None).lang_tally);
    tally.merge(&Page::new().text("2.Cc3 Cc6").read(None).lang_tally);
    assert_eq!(tally.candidates(), 4);
    assert_eq!(tally.guess(None).lang, Lang::Pt);
    assert_eq!(tally.guess(Some(Lang::Fr)).lang, Lang::Fr);
}

#[test]
fn black_move_numbers() {
    for line in ["7...h6", "7…h6", "7... h6", "7. ... h6"] {
        let read = Page::new().text(line).read(None);
        assert_eq!(runs(&read), vec![vec![black(7), mv("h6", "h6")]], "{line}");
    }
    // The move after a black number is Black's: the compensation sign is $45, not $44.
    let read = Page::new().text("7...h6© 8.Nf3©").read(None);
    assert_eq!(runs(&read)[0].get(2), Some(&ChessRun::Nag(45)));
    assert_eq!(runs(&read)[0].get(5), Some(&ChessRun::Nag(44)));
}

#[test]
fn prose_stays_text() {
    let read = Page::new().text("The knight on f3 is strong.").read(None);
    assert_eq!(runs(&read), vec![vec![text("The knight on f3 is strong.")]]);
    // Prose around moves keeps its words; a pawn move next to notation is a move.
    let read = Page::new().text("After 5.e4 d5 Black is fine, 1-0").read(None);
    assert_eq!(runs(&read), vec![vec![text("After"), white(5), mv("e4", "e4"), mv("d5", "d5"), text("Black is fine, 1-0")]]);
    // A capture with nothing before the `x` is not a move.
    let read = Page::new().text("xf6").read(None);
    assert_eq!(runs(&read), vec![vec![text("xf6")]]);
    // A decimal is not a move number, nor is a lone number in prose.
    let read = Page::new().text("He scored 3.5 points in Chapter 3. Then").read(None);
    assert_eq!(runs(&read), vec![vec![text("He scored 3.5 points in Chapter 3. Then")]]);
}

#[test]
fn castling_and_promotion() {
    let read = Page::new().text("12.O-O-O e8=Q+ 0-0").read(None);
    assert_eq!(runs(&read), vec![vec![white(12), mv("O-O-O", "O-O-O"), mv("e8=Q+", "e8=Q+"), mv("O-O", "0-0")]]);
}

#[test]
fn lines_keep_their_order_and_boxes() {
    let read = Page::new().text("1.e4 e5").newline().text("2.Nf3").read(None);
    assert_eq!(runs(&read), vec![vec![white(1), mv("e4", "e4"), mv("e5", "e5")], vec![white(2), mv("Nf3", "Nf3")]]);
    // A move number at the end of a line belongs to the move that starts the next.
    let wrapped = Page::new().text("the main line is 2.").newline().text("Nf3 Nc6").read(None);
    assert_eq!(runs(&wrapped), vec![vec![text("the main line is"), white(2)], vec![mv("Nf3", "Nf3"), mv("Nc6", "Nc6")]]);
    assert_eq!(read.lines[1].rect, ViewRect::new(0.0, LINE, 5.0 * W, LINE + 10.0));
    // User space through `coords`: a 612x792 page, view y down from the top.
    let geometry = PageGeometry::new(PageRect::new(0.0, 0.0, 612.0, 792.0), Rotation::R0).unwrap();
    let user = read.lines[1].spans[0].user_rect(&geometry).unwrap();
    assert_eq!(user, PageRect::new(0.0, 792.0 - LINE - 10.0, 2.0 * W, 792.0 - LINE));
}

#[test]
fn replacement_characters_make_a_poor_layer() {
    let read = Page::new().text("\u{FFFD}\u{FFFD}\u{FFFD} \u{FFFD}\u{FFFD}.").read(None);
    match read.quality {
        TextQuality::Poor { reason: PoorReason::ReplacementChars { ratio } } => assert!(ratio > 0.8, "{ratio}"),
        other => panic!("{other:?}"),
    }
    // A clean line is good, and an empty page is missing.
    assert_eq!(Page::new().text("1.e4 e5").read(None).quality, TextQuality::Good);
    assert_eq!(Page::new().read(None).quality, TextQuality::Missing);
}

#[test]
fn an_invisible_layer_over_a_picture_is_a_hidden_ocr_layer() {
    // FineReader's layer over a scan: plain text, all of it drawn with `3 Tr`.
    let mut page = Page::new().text("1.e4 e5 2.Nf3 Nc6");
    for g in &mut page.text.glyphs {
        g.invisible = true;
    }
    let mut read = page.read(None);
    // The layer alone reads well; only a picture under it makes it another program's OCR.
    assert_eq!(read.quality, TextQuality::Good);
    assert_eq!(read.hidden, 1.0);
    read.over_page_image();
    assert_eq!(read.quality, TextQuality::Poor { reason: PoorReason::HiddenOcrLayer { ratio: 1.0 } });

    // Visible text over a picture (a photographed page with a caption set as text) is believed.
    let mut visible = Page::new().text("1.e4 e5 2.Nf3 Nc6").read(None);
    visible.over_page_image();
    assert_eq!(visible.quality, TextQuality::Good);

    // A few invisible glyphs among visible ones are not a layer of their own.
    let mut mixed = Page::new().text("1.e4 e5 2.Nf3 Nc6");
    for g in mixed.text.glyphs.iter_mut().take(3) {
        g.invisible = true;
    }
    let mut mixed = mixed.read(None);
    mixed.over_page_image();
    assert_eq!(mixed.quality, TextQuality::Good, "hidden {}", mixed.hidden);

    // A layer already poor keeps the reason found first, and a page without one stays missing.
    let mut dead = Page::new();
    for code in 0x41..0x48u32 {
        dead = dead.glyph("\u{0}", "SomeSerif", Some(code));
    }
    for g in &mut dead.text.glyphs {
        g.invisible = true;
    }
    let mut dead = dead.read(None);
    dead.over_page_image();
    assert!(matches!(dead.quality, TextQuality::Poor { reason: PoorReason::DeadMap { .. } }), "{:?}", dead.quality);
    let mut missing = Page::new().read(None);
    missing.over_page_image();
    assert_eq!(missing.quality, TextQuality::Missing);
}

#[test]
fn dead_two_byte_codes_are_type0_without_unicode() {
    let mut page = Page::new();
    for gid in 3..12u32 {
        page = page.glyph("", "ABCDEF+SomeSerif", Some(gid + 0x0100));
    }
    match page.read(None).quality {
        TextQuality::Poor { reason: PoorReason::Type0WithoutUnicode { ratio } } => assert!(ratio > 0.9, "{ratio}"),
        other => panic!("{other:?}"),
    }
    // One-byte codes with U+0000 text are a dead map.
    let mut page = Page::new();
    for code in 0x41..0x48u32 {
        page = page.glyph("\u{0}", "SomeSerif", Some(code));
    }
    assert!(matches!(page.read(None).quality, TextQuality::Poor { reason: PoorReason::DeadMap { .. } }));
}

#[test]
fn unknown_chess_fonts_are_flagged_not_guessed() {
    let read = Page::new().text("1.").text_in("N", "ChessFancyFigures").text("f3 ").text_in("NB", "ChessFancyFigures").read(None);
    assert_eq!(read.unknown_chess_fonts, vec![Arc::<str>::from("ChessFancyFigures")]);
    assert_eq!(read.quality, TextQuality::Poor { reason: PoorReason::UnresolvedChessCodes { glyphs: 3 } });
    // A figurine-font code that is not a piece letter, where a piece belongs, is unresolved too.
    let read =
        Page::new().text("1.").text_in("l", "SemFig").text("f3 2.").text_in("l", "SemFig").text("c3 3.").text_in("l", "SemFig").text("e2").read(None);
    assert_eq!(read.quality, TextQuality::Poor { reason: PoorReason::UnresolvedChessCodes { glyphs: 3 } });
}

#[test]
fn diagram_font_glyphs_are_not_text() {
    let read = Page::new().text_in("rnbqkbnr", "ChessMerida").newline().text("Diagram 1").read(None);
    assert_eq!(runs(&read), vec![vec![text("Diagram 1")]]);
    assert_eq!(read.quality, TextQuality::Good);
    // They are not judged either: one bad glyph in five is poor, however large the diagram.
    let read = Page::new().text_in("rnbqkbnr", "ChessMerida").newline().text("Fig.\u{FFFD}").read(None);
    assert!(matches!(read.quality, TextQuality::Poor { reason: PoorReason::ReplacementChars { .. } }), "{:?}", read.quality);
}

#[test]
fn from_glyphs_lays_the_glyphs_out() {
    let page = Page::new().text("1.e4 e5");
    let read = from_glyphs(&page.text.glyphs, None);
    assert_eq!(runs(&read), vec![vec![white(1), mv("e4", "e4"), mv("e5", "e5")]]);
}

mod soup {
    use proptest::prelude::*;

    use super::*;

    const FONTS: &[&str] = &["Times-Roman", "SemFig", "ChessMerida", "Informator", "ChessUnknown", "", "FigurineCB-Time"];
    const CHARS: &str = "KQRBNPkqrbnpxabcdefgh12345678.…-+#!?=²³¹µ()[]{} ,;:♔♕♖♗♘♙♚♛♜♝♞♟⩲⩱±∓∞©N0Oo\u{FFFD}\u{0}\u{E000}\u{F04E}é中";

    fn glyph_strategy() -> impl Strategy<Value = (String, usize, Option<u32>, [f32; 4], u32, bool)> {
        let chars: Vec<char> = CHARS.chars().collect();
        (
            proptest::collection::vec(proptest::sample::select(chars), 0..4).prop_map(|v| v.into_iter().collect::<String>()),
            0..FONTS.len(),
            proptest::option::of(prop_oneof![0u32..0x200, 0xF000u32..0xF100, any::<u32>()]),
            prop_oneof![
                Just([0.0f32, 0.0, 5.0, 10.0]),
                (any::<f32>(), any::<f32>(), any::<f32>(), any::<f32>()).prop_map(|(a, b, c, d)| [a, b, c, d]),
                (0.0f32..600.0, 0.0f32..800.0).prop_map(|(x, y)| [x, y, x + 5.0, y + 10.0]),
            ],
            0u32..4,
            any::<bool>(),
        )
    }

    proptest! {
        #[test]
        fn glyph_soup_never_panics(
            soup in proptest::collection::vec(glyph_strategy(), 0..80),
            hint in proptest::option::of(proptest::sample::select(Lang::ALL.to_vec())),
            truncate in 0usize..90,
        ) {
            let mut text = PageText::default();
            for (s, font, code, rect, line, space) in soup {
                let quad = [[rect[0], rect[3]], [rect[2], rect[3]], [rect[2], rect[1]], [rect[0], rect[1]]];
                let font: Option<Arc<str>> = FONTS.get(font).map(|f| Arc::from(*f));
                text.glyphs.push(TextGlyph::new(s, rect, quad, [1.0, 0.0]).with_source(font, code));
                text.line_of.push(line);
                text.space_before.push(space);
            }
            let read = from_page_text(&text, hint);
            prop_assert!(read.lang.confidence >= 0.0 && read.lang.confidence <= 1.0);
            let _ = from_glyphs(&text.glyphs, hint);
            // Line structure that disagrees with the glyphs must not panic either.
            text.line_of.truncate(truncate);
            text.space_before.truncate(truncate / 2);
            let _ = from_page_text(&text, hint);
        }
    }
}

/// Long algebraic, as German books print it, reads as moves.
#[test]
fn long_algebraic_moves_are_moves() {
    let read = Page::new().text("1.e2-e4 e7–e5 2.Sg1-f3 Sb8-c6 3.Lf1:c6 d7:c6").read(Some(Lang::De));
    let moves: Vec<String> = read
        .lines
        .iter()
        .flat_map(|l| &l.spans)
        .filter_map(|s| if let ChessRun::Move { san, .. } = &s.run { Some(san.to_english()) } else { None })
        .collect();
    assert_eq!(moves, ["e2e4", "e7e5", "Ng1f3", "Nb8c6", "Bf1xc6", "d7xc6"], "{:?}", runs(&read));
}
