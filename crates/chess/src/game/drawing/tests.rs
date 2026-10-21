use super::*;
use crate::game::{MAX_COMMENT_LEN, NodePath};

fn sq(name: &str) -> Square {
    Square::parse(name).unwrap()
}

fn square(name: &str, color: DrawColor) -> Drawing {
    Drawing { from: sq(name), to: sq(name), color }
}

fn arrow(from: &str, to: &str, color: DrawColor) -> Drawing {
    Drawing { from: sq(from), to: sq(to), color }
}

/// The game a PGN reads as, which must read back from its own PGN the same.
fn read(pgn: &str) -> Game {
    let (g, stopped) = Game::from_pgn(pgn).unwrap();
    assert_eq!(stopped, None, "{pgn}");
    let written = g.pgn();
    let (back, _) = Game::from_pgn(&written).unwrap();
    assert_eq!(back.pgn(), written, "a fixpoint");
    for node in std::iter::once(0).chain(g.main_line()) {
        assert_eq!(back.drawings(node), g.drawings(node), "{written}");
        assert_eq!(back.comment(node), g.comment(node), "{written}");
    }
    g
}

/// The comment after the first move and its drawings, when `comment` is read after `1. e4`.
fn after_e4(comment: &str) -> (String, Vec<Drawing>) {
    let g = read(&format!("1. e4 {{{comment}}} e5 *"));
    let e4 = g.node_at(&NodePath::new(vec![0])).unwrap();
    (g.comment(e4).to_owned(), g.drawings(e4).to_vec())
}

#[test]
fn squares_and_arrows_are_read_and_the_rest_of_the_comment_stays_in_place() {
    let (text, drawings) = after_e4("Good [%clk 0:01:00] [%csl Ga4,Rb5][%cal Ge2e4,Bd1h5] idea [%eval 0.31]");
    assert_eq!(text, "Good [%clk 0:01:00] idea [%eval 0.31]");
    use DrawColor::*;
    assert_eq!(drawings, [square("a4", Green), square("b5", Red), arrow("e2", "e4", Green), arrow("d1", "h5", Blue)]);
    let g = read("1. e4 {Good [%clk 0:01:00] [%csl Ga4,Rb5][%cal Ge2e4,Bd1h5] idea [%eval 0.31]} e5 *");
    assert!(g.movetext().contains("1. e4 {[%csl Ga4,Rb5][%cal Ge2e4,Bd1h5] Good [%clk 0:01:00] idea [%eval 0.31]} 1... e5"), "{}", g.movetext());
    // Glued to the words around it, a command's place keeps them apart.
    assert_eq!(after_e4("a[%csl Yh8]b").0, "a b");
    // Arrows read before squares are still kept squares first.
    assert_eq!(after_e4("[%cal Ra1a8][%csl Yc3]").1, [square("c3", Yellow), arrow("a1", "a8", Red)]);
}

#[test]
fn what_is_not_a_drawing_stays_in_the_text() {
    // Empty, unknown letters and squares, an arrow onto its own square, a lower-case colour, a
    // name in capitals and a command left open: all kept as they were.
    for kept in ["[%cal]", "[%csl ]", "[%csl Xa4]", "[%csl Gz9]", "[%cal Ge2e2]", "[%csl ga4]", "[%CSL Ga4]", "[%cal Ge2", "[%csl Ga4 Rb5]"] {
        let (text, drawings) = after_e4(kept);
        assert_eq!((text.as_str(), drawings.len()), (kept.trim(), 0), "{kept}");
    }
    // A command with some entries that are drawings keeps the others, under its name.
    let (text, drawings) = after_e4("x [%csl Xa4,Ga4,Gz9] y");
    assert_eq!(text, "x [%csl Xa4,Gz9] y");
    assert_eq!(drawings, [square("a4", DrawColor::Green)]);
    // A second colour for the same square is kept as text; the same one again is the same drawing.
    let (text, drawings) = after_e4("[%csl Ga4,Ra4,Ga4]");
    assert_eq!(text, "[%csl Ra4]");
    assert_eq!(drawings, [square("a4", DrawColor::Green)]);
    // Unicode around and inside.
    let (text, drawings) = after_e4("é[%csl Gé4,Ga4]ñ [%cal Gä1a2]");
    assert_eq!(text, "é [%csl Gé4] ñ [%cal Gä1a2]");
    assert_eq!(drawings.len(), 1);
}

#[test]
fn a_long_list_is_capped_and_the_rest_kept() {
    let names: Vec<String> = (0..64).filter_map(Square::from_index).map(|s| format!("G{s}")).collect();
    let arrows: Vec<String> = (0..64).filter_map(Square::from_index).filter(|s| s.index() != 0).map(|s| format!("Ra1{s}")).collect();
    let comment = format!("[%csl {}][%cal {}]", names.join(","), arrows.join(","));
    let (text, drawings) = after_e4(&comment);
    assert_eq!(drawings.len(), MAX_DRAWINGS);
    assert!(drawings.iter().all(|d| !d.is_arrow()), "the squares came first");
    assert_eq!(text, format!("[%cal {}]", arrows.join(",")), "the arrows past the cap stay as text");
    // Far more than a comment holds never panics; what is past the cap is cut with the comment.
    let huge = format!("[%cal {}]", vec!["Ge2e4"; 20_000].join(","));
    let mut g = Game::standard();
    g.set_comment(0, &huge);
    assert_eq!(g.drawings(0), [arrow("e2", "e4", DrawColor::Green)]);
    assert!(g.comment(0).len() <= MAX_COMMENT_LEN);
}

#[test]
fn drawings_on_the_start_and_alone_are_written_as_a_comment() {
    let mut g = Game::standard();
    assert!(g.toggle_drawing(0, square("e4", DrawColor::Green)));
    g.play_san("e4").unwrap();
    let e4 = g.current();
    assert!(g.toggle_drawing(e4, arrow("e7", "e5", DrawColor::Red)));
    g.play_san("e5").unwrap();
    // Black's move after a comment gets its number again, as after any comment.
    assert_eq!(g.movetext(), "{[%csl Ge4]} 1. e4 {[%cal Re7e5]} 1... e5");
    // The notation's comment tokens have no text of their own.
    assert!(g.tokens().iter().any(|t| matches!(t, crate::game::Token::Comment { node, text, .. } if *node == e4 && text.is_empty())));
    let back = read(&g.pgn());
    assert_eq!(back.drawings(0), g.drawings(0));
    // With the comment before the next move written in the same braces.
    g.set_comment_before(g.current(), "now");
    assert_eq!(g.movetext(), "{[%csl Ge4]} 1. e4 {[%cal Re7e5] now} 1... e5");
    read(&g.pgn());
}

#[test]
fn toggling_draws_recolours_and_takes_off() {
    let mut g = Game::standard();
    let a = arrow("g1", "f3", DrawColor::Green);
    let revision = g.revision();
    assert!(g.toggle_drawing(0, a));
    assert!(g.revision() > revision);
    assert!(g.toggle_drawing(0, square("d4", DrawColor::Blue)));
    assert_eq!(g.drawings(0), [square("d4", DrawColor::Blue), a], "squares first");
    assert!(g.toggle_drawing(0, Drawing { color: DrawColor::Yellow, ..a }));
    assert_eq!(g.drawings(0)[1].color, DrawColor::Yellow, "recoloured in place");
    assert!(g.toggle_drawing(0, Drawing { color: DrawColor::Yellow, ..a }));
    assert_eq!(g.drawings(0), [square("d4", DrawColor::Blue)]);
    assert!(!g.toggle_drawing(99, a), "no such move");
    assert!(g.clear_drawings(0));
    assert!(!g.clear_drawings(0));
    // At the cap a new drawing is refused, an old one still taken off.
    for i in 0..MAX_DRAWINGS {
        assert!(g.toggle_drawing(0, square(&Square::from_index(i).unwrap().to_string(), DrawColor::Red)));
    }
    assert!(!g.toggle_drawing(0, a));
    assert!(g.toggle_drawing(0, square("a1", DrawColor::Red)));
}

#[test]
fn a_comment_typed_with_commands_draws_them() {
    let mut g = Game::standard();
    g.play_san("d4").unwrap();
    let d4 = g.current();
    g.set_comment(d4, "Center [%csl Gd4]");
    assert_eq!((g.comment(d4), g.drawings(d4)), ("Center", &[square("d4", DrawColor::Green)][..]));
    // Typing the same again changes nothing.
    let revision = g.revision();
    g.set_comment(d4, "Center [%csl Gd4]");
    g.set_comment(d4, "Center");
    assert_eq!(g.revision(), revision);
    assert_eq!(g.drawings(d4).len(), 1, "taking the command out of the text leaves the drawing");
}

#[test]
fn a_comment_before_a_variation_keeps_its_commands_as_text() {
    let g = read("1. e4 ({[%csl Gd4] the queen's pawn} 1. d4) 1... e5 *");
    let d4 = g.node_at(&NodePath::new(vec![1])).unwrap();
    assert_eq!(g.comment_before(d4), "[%csl Gd4] the queen's pawn");
    assert!(g.drawings(d4).is_empty() && g.drawings(0).is_empty());
}

#[test]
fn drawings_follow_their_moves_through_promotion() {
    let mut g = read("1. e4 {[%csl Ge4]} (1. d4 {[%cal Gc1g5]}) 1... e5 *");
    let d4 = g.node_at(&NodePath::new(vec![1])).unwrap();
    g.promote(d4);
    let d4 = g.node_at(&NodePath::new(vec![0])).unwrap();
    assert_eq!(g.san(d4), "d4");
    assert_eq!(g.drawings(d4), [arrow("c1", "g5", DrawColor::Green)]);
    assert_eq!(g.movetext(), "1. d4 {[%cal Gc1g5]} (1. e4 {[%csl Ge4]} 1... e5)");
    read(&g.pgn());
}

proptest::proptest! {
    #[test]
    fn splitting_never_panics_and_keeps_every_other_character(text in "\\PC{0,200}") {
        let mut drawings = Vec::new();
        let rest = split_drawings(&text, &mut drawings);
        proptest::prop_assert!(drawings.len() <= MAX_DRAWINGS);
        // Without a drawing taken, the text is exactly as it was.
        if drawings.is_empty() {
            proptest::prop_assert_eq!(rest, text);
        }
    }

    #[test]
    fn comments_with_commands_are_a_pgn_fixpoint(parts in proptest::collection::vec(proptest::sample::select(vec![
        "[%csl Ga4,Rb5]", "[%cal Ge2e4]", "[%cal Ye2e4,Bd1h5]", "[%csl Ra4]", "[%csl Xa4,Gb1]", "[%cal]", "[%clk 0:01]",
        "[%eval -0.3]", "[%csl", "]", "word", "é", "[", "%", ",", "Gc3", " ",
    ]), 0..12), before in proptest::sample::select(vec!["", "b", "[%csl Ga1]"])) {
        let text = parts.concat();
        let pgn = format!("{{{text}}} 1. e4 {{{text}}} ({{{before}}} 1. d4 {{{text}}}) 1... e5 *");
        let (g, _) = Game::from_pgn(&pgn).unwrap();
        let written = g.pgn();
        let (back, stopped) = Game::from_pgn(&written).unwrap();
        proptest::prop_assert_eq!(stopped, None);
        proptest::prop_assert_eq!(back.pgn(), written.clone());
        for path in [vec![], vec![0], vec![1], vec![0, 0]] {
            let (a, b) = (g.node_at(&NodePath::new(path.clone())), back.node_at(&NodePath::new(path)));
            proptest::prop_assert!(a.is_some() && b.is_some());
            let (a, b) = (a.unwrap_or_default(), b.unwrap_or_default());
            proptest::prop_assert_eq!(g.drawings(a), back.drawings(b));
            proptest::prop_assert_eq!(g.comment(a), back.comment(b));
        }
    }
}
