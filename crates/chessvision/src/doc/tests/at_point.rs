//! The printed move under a point of the page (plan A9-N1): a point on a move finds it and the
//! position after it; prose, the gap between two moves, the margins, another page and a point
//! that is not finite find nothing.

use super::*;
use crate::doc::move_at;

/// The middle of a row whose top is at view `top` (rows are 12 pt high), in user space.
fn middle(top: f32) -> f32 {
    PAGE_H - (top + 6.0)
}

/// Page `index` of rows of text, each `(view top, text)`, from x 40 (7 pt a character, 4 pt
/// between runs).
fn page_of(index: u32, rows: &[(f32, &str)]) -> ChessDoc {
    let text = page_text(rows.iter().map(|(top, t)| line(*top, t)).collect());
    build_page(&input(index, &text, &[], &[], &[]))
}

/// The text of the move at `at`.
fn shown(doc: &ChessDoc, at: MoveAt) -> &str {
    at.find(doc).map_or("", |m| m.text.as_str())
}

/// The move whose text is `text`, mutable.
fn move_mut<'a>(doc: &'a mut ChessDoc, text: &str) -> &'a mut MoveInline {
    doc.pages
        .iter_mut()
        .flat_map(|p| p.blocks.iter_mut())
        .flat_map(|b| match b {
            Block::Heading { inlines, .. } | Block::Paragraph { inlines, .. } => inlines.iter_mut(),
            Block::Diagram(_) => [].iter_mut(),
        })
        .find_map(|i| match i {
            Inline::Move(m) if m.text == text => Some(m),
            _ => None,
        })
        .unwrap()
}

/// `1.e4 e5 2.Nf3`: `1.e4` (its number on its row is in its box) x 40..79, `e5` 83..97, `2.Nf3`
/// 101..147.
#[test]
fn a_point_on_a_move_finds_it_and_the_position_after_it() {
    let doc = page_of(2, &[(230.0, "1.e4 e5 2.Nf3")]);
    let y = middle(230.0);
    let (at, fen) = move_at(&doc, 2, [90.0, y]).expect("e5");
    assert_eq!(shown(&doc, at), "e5");
    assert_eq!(at.page, 2);
    let e5 = at.find(&doc).unwrap();
    assert_eq!(e5.status, MoveStatus::Valid);
    assert_eq!(fen, e5.fen.as_deref(), "the position after the move");
    assert!(fen.is_some_and(|f| f.starts_with("rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w ")), "{fen:?}");
    // The number printed on the move's row is in its box.
    assert_eq!(move_at(&doc, 2, [45.0, y]).map(|(at, _)| shown(&doc, at)), Some("e4"));
    // The edges of a box are in it.
    assert_eq!(move_at(&doc, 2, [83.0, PAGE_H - 230.0]).map(|(at, _)| shown(&doc, at)), Some("e5"));
    assert_eq!(move_at(&doc, 2, [97.0, PAGE_H - 242.0]).map(|(at, _)| shown(&doc, at)), Some("e5"));
}

#[test]
fn between_two_moves_above_and_beside_a_row_nothing_is_found() {
    let doc = page_of(0, &[(230.0, "1.e4 e5 2.Nf3")]);
    let y = middle(230.0);
    assert_eq!(move_at(&doc, 0, [81.0, y]), None, "the gap between e4 and e5");
    assert_eq!(move_at(&doc, 0, [90.0, y + 7.0]), None, "just above the row");
    assert_eq!(move_at(&doc, 0, [90.0, y - 7.0]), None, "just below it");
    assert_eq!(move_at(&doc, 0, [30.0, y]), None, "the left margin");
    assert_eq!(move_at(&doc, 0, [148.0, y]), None, "past the last move");
}

#[test]
fn prose_another_page_and_a_point_not_finite_find_nothing() {
    let doc = page_of(1, &[(100.0, "White should think twice here."), (230.0, "1.e4 e5")]);
    assert!(doc.pages[0].blocks.iter().flat_map(Block::inlines).any(|i| matches!(i, Inline::Text { .. })));
    assert_eq!(move_at(&doc, 1, [60.0, middle(100.0)]), None, "prose");
    assert_eq!(move_at(&doc, 0, [90.0, middle(230.0)]), None, "a page not read");
    assert_eq!(move_at(&doc, u32::MAX, [90.0, middle(230.0)]), None);
    for point in [[f32::NAN, middle(230.0)], [90.0, f32::NAN], [f32::INFINITY, 0.0], [f32::NEG_INFINITY, f32::INFINITY], [-1e30, -1e30], [-5.0, -5.0]]
    {
        assert_eq!(move_at(&doc, 1, point), None, "{point:?}");
    }
    let empty = ChessDoc { format: FORMAT_VERSION, pages: Vec::new(), games: Vec::new() };
    assert_eq!(move_at(&empty, 0, [90.0, middle(230.0)]), None, "an empty document");
}

/// A move's number left at the end of the row above: the move is found on its own row, where it
/// was printed, and keeps its number; the number has no box of its own in the reading.
#[test]
fn a_move_wrapped_onto_the_next_row_is_found_where_it_was_printed() {
    // `2.` at x 101..122 on the first row; `Nf3` at 40..61 on the second, `Nc6` 65..86.
    let doc = page_of(0, &[(230.0, "1.e4 e5 2."), (244.0, "Nf3 Nc6")]);
    let (at, fen) = move_at(&doc, 0, [50.0, middle(244.0)]).expect("Nf3");
    let nf3 = at.find(&doc).unwrap();
    assert_eq!((nf3.text.as_str(), nf3.number.map(|n| n.number)), ("Nf3", Some(2)));
    assert!(fen.is_some(), "played: {:?}", nf3.status);
    let r = nf3.source_rect.unwrap();
    assert!(r.y1 <= PAGE_H - 244.0 + 0.01, "its box is its own row's: {r:?}");
    assert_eq!(move_at(&doc, 0, [110.0, middle(230.0)]), None, "the number on the row above");
    assert_eq!(move_at(&doc, 0, [70.0, middle(244.0)]).map(|(at, _)| shown(&doc, at)), Some("Nc6"));
}

/// Boxes that overlap give the move whose centre is nearest; a box written with its corners
/// swapped (JSON keeps them as written) still holds its points; one that is not finite none.
#[test]
fn overlapping_swapped_and_broken_boxes() {
    let mut doc = page_of(0, &[(230.0, "1.e4 e5")]);
    let y = middle(230.0);
    // e5's box laid over the end of e4's (x 40..79): 70..90.
    let e4 = move_mut(&mut doc, "e4").source_rect.unwrap();
    move_mut(&mut doc, "e5").source_rect = Some(UserRect { x0: 70.0, ..e4 });
    assert_eq!(move_at(&doc, 0, [76.0, y]).map(|(at, _)| shown(&doc, at)), Some("e5"), "nearer e5's centre (74.5)");
    assert_eq!(move_at(&doc, 0, [71.0, y]).map(|(at, _)| shown(&doc, at)), Some("e5"));
    assert_eq!(move_at(&doc, 0, [60.0, y]).map(|(at, _)| shown(&doc, at)), Some("e4"), "e4's alone");
    let swapped = UserRect { x0: e4.x1, y0: e4.y1, x1: e4.x0, y1: e4.y0 };
    move_mut(&mut doc, "e5").source_rect = Some(UserRect { x0: 200.0, x1: 180.0, ..swapped });
    assert_eq!(move_at(&doc, 0, [190.0, y]).map(|(at, _)| shown(&doc, at)), Some("e5"), "corners swapped");
    for broken in [f32::NAN, f32::INFINITY] {
        move_mut(&mut doc, "e5").source_rect = Some(UserRect { x0: 180.0, x1: broken, ..e4 });
        assert_eq!(move_at(&doc, 0, [190.0, y]), None, "{broken}");
    }
    move_mut(&mut doc, "e5").source_rect = None;
    assert_eq!(move_at(&doc, 0, [190.0, y]), None, "no box");
}

/// What opens a game, as the Text tab's double-click: a move played, or one with no position on
/// its page alone (it may go on from the page before); a move refused does not.
#[test]
fn a_move_opens_its_game_when_played_or_unchecked() {
    let opens = |doc: &ChessDoc, text: &str| {
        doc.pages[0].blocks.iter().flat_map(Block::inlines).find_map(|i| match i {
            Inline::Move(m) if m.text == text => Some((m.status, m.opens_game(), m.fen.is_some())),
            _ => None,
        })
    };
    let game = page_of(0, &[(230.0, "1.e4 e5 2.Ke3 Nc6")]);
    assert_eq!(opens(&game, "e5"), Some((MoveStatus::Valid, true, true)));
    assert_eq!(opens(&game, "Ke3"), Some((MoveStatus::Illegal, false, false)), "a move the walk refused opens nothing");
    assert_eq!(opens(&game, "Nc6"), Some((MoveStatus::NotReached, false, false)), "nor one after it");
    let prose = page_of(1, &[(300.0, "Black should answer 23...d5 at once.")]);
    assert_eq!(opens(&prose, "d5"), Some((MoveStatus::Unchecked, true, false)), "no position on the page alone");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Any point, finite or not: no panic, and a move found has the point in its box.
    #[test]
    fn a_move_found_holds_the_point(x in prop_oneof![-50.0f32..700.0, Just(f32::NAN), Just(f32::INFINITY), Just(-1e38f32)],
                                    y in prop_oneof![500.0f32..600.0, -1e3f32..1e3, Just(f32::NAN)],
                                    page in 0u32..3) {
        let doc = page_of(1, &[(230.0, "1.e4 e5 2.Nf3 Nc6 3.Bb5 a6"), (244.0, "4.Ba4 Nf6")]);
        if let Some((at, fen)) = move_at(&doc, page, [x, y]) {
            let m = at.find(&doc).unwrap();
            let r = m.source_rect.unwrap();
            prop_assert!(r.x0 <= x && x <= r.x1 && r.y0 <= y && y <= r.y1);
            prop_assert_eq!(fen, m.fen.as_deref());
            prop_assert_eq!(page, 1);
        }
    }

    /// The centre of every move's box finds that move (the boxes of a row do not overlap).
    #[test]
    fn each_moves_centre_finds_it(k in 0usize..8) {
        let doc = page_of(0, &[(230.0, "1.e4 e5 2.Nf3 Nc6 3.Bb5 a6"), (244.0, "4.Ba4 Nf6")]);
        let all: Vec<(MoveAt, UserRect)> = doc.pages[0].blocks.iter().enumerate().flat_map(|(b, block)| {
            block.inlines().iter().enumerate().filter_map(move |(i, x)| match x {
                Inline::Move(m) => Some((MoveAt::new(0, b, i), m.source_rect?)),
                _ => None,
            })
        }).collect();
        prop_assert_eq!(all.len(), 8);
        let (at, r) = all[k];
        prop_assert_eq!(move_at(&doc, 0, [(r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0]).map(|(a, _)| a), Some(at));
    }
}
