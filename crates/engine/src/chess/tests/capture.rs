//! Capture planning (PGN toolbar spec §4.3.3, step T3) on the spec's scenario (§4.10): where a
//! selection goes, how it is scored, and when the preview is asked for.

use std::ops::Range;

use pdfcraft_chess::game::{Game as ChessGame, GraftMode, NodeId, NodePath};
use pdfcraft_chess::san::Lang;

use crate::chess::capture::{CaptureError, CaptureMode, CapturePlan, CaptureText, MAX_CAPTURE_TEXT, PreviewReason, plan};

/// The position before White's 18th move in the scenario.
const FEN: &str = "2r2rk1/1p2ppb1/p2p1npp/q4P2/P1b1P2B/2NB4/1PP3PP/R3QR1K w - - 0 18";

/// The scenario's paragraph as the book sets it: the game's moves in bold (`*` toggles bold).
const SCENARIO: &str = "*18.Nd5!* *Qxe1* After 18…Qd8 19.Bxf6! Bxf6 20.fxg6 (less good is
20.Nxf6+, as Black has nothing to fear after 20…exf6 21.Bxc4
Rxc4) 20…fxg6 21.Qg3, White obtains a dangerous initiative. For
example: 21…Kh7 22.e5!. *19.Nxe7+ Kh7 20.Raxe1*";

/// `marked` without its `*`s, and the character ranges they set in bold.
fn styled(marked: &str) -> (String, Vec<Range<usize>>) {
    let (mut text, mut runs, mut open) = (String::new(), Vec::new(), None);
    for c in marked.chars() {
        if c == '*' {
            let at = text.chars().count();
            match open.take() {
                Some(start) => runs.push(start..at),
                None => open = Some(at),
            }
        } else {
            text.push(c);
        }
    }
    (text, runs)
}

/// Plan `marked` (see [`styled`]) after `current` in `mode`, in English.
fn plan_at(game: &ChessGame, current: NodeId, mode: CaptureMode, marked: &str) -> CapturePlan {
    let (text, bold) = styled(marked);
    plan(game, current, mode, CaptureText::Styled { text: &text, bold: &bold }, Lang::En)
}

/// Plan on the move shown and apply the chosen candidate.
fn capture(game: &mut ChessGame, mode: CaptureMode, marked: &str) -> CapturePlan {
    let p = plan_at(game, game.current(), mode, marked);
    p.apply(game, None).unwrap_or_else(|e| panic!("{marked}: {e} ({:?})", p.reasons));
    p
}

/// The movetext without comments, and without the number PGN repeats for a Black move after a
/// comment.
fn moves_only(movetext: &str) -> String {
    let mut flat = String::new();
    let mut depth = 0usize;
    for c in movetext.chars() {
        match c {
            '{' => {
                if depth == 0 {
                    flat.push_str(" {} ");
                }
                depth += 1;
            }
            '}' => depth = depth.saturating_sub(1),
            '(' | ')' if depth == 0 => {
                flat.push(' ');
                flat.push(c);
                flat.push(' ');
            }
            _ if depth == 0 => flat.push(c),
            _ => {}
        }
    }
    let words: Vec<&str> = flat.split_whitespace().collect();
    let mut kept: Vec<&str> = Vec::new();
    for (k, w) in words.iter().enumerate() {
        if *w == "{}" {
            continue;
        }
        let renumbered = w.ends_with("...")
            && w.trim_end_matches('.').chars().all(|c| c.is_ascii_digit())
            && k >= 2
            && words[k - 1] == "{}"
            && !matches!(words[k - 2], "(" | ")");
        if !renumbered {
            kept.push(w);
        }
    }
    kept.join(" ").replace("( ", "(").replace(" )", ")")
}

fn scenario_game() -> ChessGame {
    ChessGame::from_fen(FEN).unwrap()
}

/// The scenario's main line, the move shown its last.
fn main_line_game() -> ChessGame {
    let mut g = scenario_game();
    for m in ["Nd5", "Qxe1", "Nxe7+", "Kh7", "Raxe1"] {
        g.play_san(m).unwrap();
    }
    g
}

fn at(g: &ChessGame, path: &[u16]) -> NodeId {
    g.node_at(&NodePath::new(path.to_vec())).unwrap()
}

/// §4.10 step 2: the whole paragraph in one gesture, applied without the preview.
#[test]
fn the_whole_paragraph_goes_in_at_once() {
    let mut g = scenario_game();
    let p = capture(&mut g, CaptureMode::Auto, SCENARIO);
    assert!(!p.needs_preview, "{:?}", p.reasons);
    assert_eq!(p.candidates.len(), 1);
    let c = p.chosen().unwrap();
    assert_eq!((c.mode_used, c.line, c.anchor.is_start(), c.played), (GraftMode::Line, 0, true, 17));
    assert_eq!(
        moves_only(&g.movetext()),
        "18. Nd5 $1 Qxe1 (18... Qd8 19. Bxf6 $1 Bxf6 20. fxg6 (20. Nxf6+ exf6 21. Bxc4 Rxc4) 20... fxg6 21. Qg3 Kh7 22. e5 $1) 19. Nxe7+ Kh7 20. Raxe1"
    );
    let movetext = g.movetext();
    assert!(movetext.contains("{After}"), "{movetext}");
    assert!(movetext.contains("21. Qg3 {White obtains a dangerous initiative. For example:}"), "{movetext}");
    // The move shown is the last one grafted on the fragment's own line.
    assert_eq!(g.san(g.current()), "Raxe1");
    // Planning changed nothing; the same text again plays every move by following them.
    let again = plan_at(&g, g.current(), CaptureMode::Auto, SCENARIO);
    assert_eq!(again.chosen().unwrap().game.movetext(), movetext);
}

/// §4.10 step 3: a variation that finds the main line's end extends it, with the preview; the bold
/// line then follows `20…Rc7` and demotes `21.e5`, with the preview again.
#[test]
fn a_different_move_on_the_line_is_demoted_with_the_preview() {
    let mut g = main_line_game();
    let p = capture(&mut g, CaptureMode::Auto, "20…Rc7 21.e5!");
    assert_eq!(p.reasons, vec![PreviewReason::TipExtension]);
    assert_eq!(moves_only(&g.movetext()), "18. Nd5 Qxe1 19. Nxe7+ Kh7 20. Raxe1 Rc7 21. e5 $1");

    g.go_to_node(at(&g, &[0, 0, 0, 0, 0]));
    let p = capture(&mut g, CaptureMode::Auto, "*20…Rc7 21.Bxc4 Rxc4*");
    assert_eq!(p.reasons, vec![PreviewReason::Demotes]);
    let c = p.chosen().unwrap();
    assert_eq!((c.mode_used, c.played, c.report.demoted), (GraftMode::Line, 3, true));
    assert_eq!(moves_only(&g.movetext()), "18. Nd5 Qxe1 19. Nxe7+ Kh7 20. Raxe1 Rc7 21. Bxc4 (21. e5 $1) 21... Rxc4");
}

/// §4.10 step 4: the forced modes, one after the other from the empty game.
#[test]
fn forced_modes_build_the_tree_step_by_step() {
    let mut g = scenario_game();
    let p = capture(&mut g, CaptureMode::Line, "18.Nd5! Qxe1");
    assert!(!p.needs_preview, "{:?}", p.reasons);
    assert_eq!(moves_only(&g.movetext()), "18. Nd5 $1 Qxe1");

    let p = capture(&mut g, CaptureMode::Variation, "18…Qd8 19.Bxf6! Bxf6 20.fxg6");
    assert!(!p.needs_preview, "{:?}", p.reasons);
    assert_eq!(moves_only(&g.movetext()), "18. Nd5 $1 Qxe1 (18... Qd8 19. Bxf6 $1 Bxf6 20. fxg6)");
    assert_eq!(g.san(g.current()), "fxg6");

    let p = capture(&mut g, CaptureMode::SubVariation, "20.Nxf6+");
    assert!(!p.needs_preview, "{:?}", p.reasons);
    assert_eq!(moves_only(&g.movetext()), "18. Nd5 $1 Qxe1 (18... Qd8 19. Bxf6 $1 Bxf6 20. fxg6 (20. Nxf6+))");

    let p = capture(&mut g, CaptureMode::Line, "20…exf6 21.Bxc4 Rxc4");
    assert!(!p.needs_preview, "{:?}", p.reasons);
    assert_eq!(moves_only(&g.movetext()), "18. Nd5 $1 Qxe1 (18... Qd8 19. Bxf6 $1 Bxf6 20. fxg6 (20. Nxf6+ exf6 21. Bxc4 Rxc4))");

    // From inside the sub-variation, `20…fxg6` cannot follow 20.Nxf6+ (Black is in check and no
    // White piece stands on g6): the outer line, which ends at 20.fxg6, takes it.
    let p = plan_at(&g, g.current(), CaptureMode::Line, "20…fxg6 21.Qg3");
    // A place on L0 (after 20.Nxf6+) and on L1 (after 20.fxg6); the main line has no 20th move.
    assert_eq!(p.candidates.iter().map(|c| (c.line, c.played)).collect::<Vec<_>>(), vec![(0, 0), (1, 2)]);
    assert_eq!(p.chosen, Some(1), "the best, not the first");
    assert_eq!(p.reasons, vec![PreviewReason::OuterLine]);
    p.apply(&mut g, None).unwrap();
    assert_eq!(moves_only(&g.movetext()), "18. Nd5 $1 Qxe1 (18... Qd8 19. Bxf6 $1 Bxf6 20. fxg6 (20. Nxf6+ exf6 21. Bxc4 Rxc4) 20... fxg6 21. Qg3)");
}

/// A move that fits two places plays as many moves at each: both are offered, the inner one
/// first, and the preview asks.
#[test]
fn two_places_that_fit_are_ambiguous() {
    // 19…Kh8 is a variation of 19…Kh7; 20.Rfxe1 fits after either.
    let mut g = main_line_game();
    g.go_to_node(at(&g, &[0, 0, 0]));
    g.play_san("Kh8").unwrap();
    let kh8 = g.current();
    let p = plan_at(&g, kh8, CaptureMode::SubVariation, "20.Rfxe1");
    assert_eq!(p.candidates.len(), 2);
    assert_eq!(p.candidates.iter().map(|c| (c.line, c.played)).collect::<Vec<_>>(), vec![(0, 1), (1, 1)]);
    assert!(p.needs_preview);
    assert!(p.reasons.contains(&PreviewReason::Ambiguous), "{:?}", p.reasons);
    assert_eq!(p.chosen, Some(0), "the inner line first");
    assert_eq!(moves_only(&p.candidates[1].game.movetext()), "18. Nd5 Qxe1 19. Nxe7+ Kh7 (19... Kh8) 20. Raxe1 (20. Rfxe1)");
    // In brackets, as prose quotes it, the same.
    let p = plan_at(&g, kh8, CaptureMode::Auto, "(20.Rfxe1)");
    assert!(p.reasons.contains(&PreviewReason::Ambiguous), "{:?}", p.reasons);
}

/// A game with no moves whose start says move 1 is offered the start renumbered, never silently.
#[test]
fn a_start_at_move_one_is_offered_renumbered() {
    let unnumbered = FEN.replace(" 0 18", " 0 1");
    let mut g = ChessGame::from_fen(&unnumbered).unwrap();
    g.set_tag("White", "Grooten").unwrap();
    let p = plan_at(&g, 0, CaptureMode::Auto, "18.Nd5 Qxe1");
    assert_eq!(p.candidates.len(), 1);
    let c = p.chosen().unwrap();
    assert_eq!((c.renumbered, c.played), (Some(18), 2));
    assert_eq!(p.reasons, vec![PreviewReason::Renumber]);
    assert!(c.game.start_fen().ends_with(" 0 18"), "{}", c.game.start_fen());
    assert_eq!(c.game.tag("White"), Some("Grooten"));
    p.apply(&mut g, None).unwrap();
    assert_eq!(g.movetext(), "18. Nd5 Qxe1");
    // The other side's number, or the start's own, offers nothing.
    assert!(plan_at(&ChessGame::from_fen(&unnumbered).unwrap(), 0, CaptureMode::Auto, "18…Qd8").candidates.iter().all(|c| c.renumbered.is_none()));
    assert!(plan_at(&ChessGame::from_fen(&unnumbered).unwrap(), 0, CaptureMode::Auto, "1.Nd5").candidates.iter().all(|c| c.renumbered.is_none()));
}

/// Without a number the line goes after the current move; a variation is an alternative to the
/// move after it, then to it.
#[test]
fn a_move_without_a_number_goes_by_the_current_move() {
    let g = main_line_game();
    let kh7 = at(&g, &[0, 0, 0, 0]);
    // As the line, after 19…Kh7: 20.Raxe1 is followed.
    let p = plan_at(&g, kh7, CaptureMode::Line, "Raxe1 Rc7");
    assert_eq!(p.candidates.len(), 1);
    assert_eq!(p.chosen().unwrap().played, 2);
    assert!(p.reasons.is_empty(), "{:?}", p.reasons);
    // As a variation: `Rfxe1` is no alternative to 19…Kh7, but is one to 20.Raxe1.
    let p = plan_at(&g, kh7, CaptureMode::SubVariation, "Rfxe1");
    assert_eq!(p.candidates.iter().map(|c| c.played).collect::<Vec<_>>(), vec![1, 0]);
    assert_eq!(p.chosen, Some(0));
    assert!(p.reasons.is_empty(), "{:?}", p.reasons);
    assert_eq!(moves_only(&p.chosen().unwrap().game.movetext()), "18. Nd5 Qxe1 19. Nxe7+ Kh7 20. Raxe1 (20. Rfxe1)");
    let p = plan_at(&g, kh7, CaptureMode::SubVariation, "Kh8");
    assert_eq!(p.chosen, Some(1));
    assert_eq!(moves_only(&p.chosen().unwrap().game.movetext()), "18. Nd5 Qxe1 19. Nxe7+ Kh7 (19... Kh8) 20. Raxe1");
}

/// Text that does not read through stops the preview from being skipped, and says why.
#[test]
fn an_incomplete_reading_asks_for_the_preview() {
    let g = main_line_game();
    let p = plan_at(&g, g.current(), CaptureMode::Line, "20…Rc7 21.Qh8");
    let c = p.chosen().unwrap();
    assert_eq!(c.played, 1);
    assert!(p.reasons.contains(&PreviewReason::Incomplete), "{:?}", p.reasons);
    // A lost bracket that the walker assumed.
    let p = plan_at(
        &scenario_game(),
        0,
        CaptureMode::Line,
        "18.Nd5 Qxe1 (18…Qd8 19.Bxf6 Bxf6 20.fxg6 (20.Nxf6+ exf6 21.Bxc4 Rxc4 20…fxg6 21.Qg3) 19.Nxe7+",
    );
    assert!(p.reasons.contains(&PreviewReason::ImpliedClose), "{:?}", p.reasons);
    // Nothing that plays.
    let p = plan_at(&g, g.current(), CaptureMode::Auto, "a quiet move");
    assert_eq!((p.chosen, p.reasons.clone()), (None, vec![PreviewReason::NoFit]));
    let p = plan_at(&g, g.current(), CaptureMode::Auto, "20…Qh1");
    assert_eq!(p.chosen, None);
    assert!(p.needs_preview && p.reasons.contains(&PreviewReason::NoFit), "{:?}", p.reasons);
}

/// Comment mode writes the text, moves in English SAN, on the current move or before it.
#[test]
fn comment_mode_writes_the_text_on_the_current_move() {
    let mut g = main_line_game();
    let kh7 = at(&g, &[0, 0, 0, 0]);
    g.go_to_node(kh7);
    let p = capture(&mut g, CaptureMode::Comment { before: false }, "the point is 20.♖axe1! and *Rc7*");
    assert!(!p.needs_preview, "{:?}", p.reasons);
    assert_eq!(g.comment(kh7), "the point is 20.Raxe1! and Rc7");
    assert_eq!(g.current(), kh7);
    // The same again adds nothing.
    let p = capture(&mut g, CaptureMode::Comment { before: false }, "the point is 20.♖axe1! and Rc7");
    assert_eq!(p.chosen().unwrap().game.comment(kh7), "the point is 20.Raxe1! and Rc7");
    // Before a variation's first move: its comment-before. In Portuguese, `C` is the knight.
    g.go_to_node(at(&g, &[0, 0, 0]));
    g.play_san("Kh8").unwrap();
    let kh8 = g.current();
    let (text, _) = styled("a tentativa 20.Cxf6");
    let p = plan(&g, kh8, CaptureMode::Comment { before: true }, CaptureText::Plain(&text), Lang::Pt);
    p.apply(&mut g, None).unwrap();
    assert_eq!(g.comment_before(kh8), "a tentativa 20.Nxf6");
    // Nothing to write.
    let p = plan(&g, kh8, CaptureMode::Comment { before: false }, CaptureText::Plain("  "), Lang::En);
    assert_eq!((p.chosen, p.reasons), (None, vec![PreviewReason::NoFit]));
    // Too long for the comment: kept whole as left over, never cut.
    let long = "word ".repeat(1000);
    let p = plan(&g, kh8, CaptureMode::Comment { before: false }, CaptureText::Plain(&long), Lang::En);
    assert_eq!(p.reasons, vec![PreviewReason::Incomplete]);
    assert_eq!(p.chosen().unwrap().game.comment(kh8), "");
    assert_eq!(p.chosen().unwrap().report.leftover_text, long.trim());
}

/// A plan is applied only to the game it was made on.
#[test]
fn a_stale_plan_is_refused() {
    let mut g = main_line_game();
    let p = plan_at(&g, g.current(), CaptureMode::Auto, "20…Rc7");
    g.set_comment(0, "edited meanwhile");
    let before = g.movetext();
    assert_eq!(p.apply(&mut g, None), Err(CaptureError::Stale));
    assert_eq!(g.movetext(), before);
    let mut fresh = main_line_game();
    assert_eq!(p.apply(&mut fresh, Some(7)), Err(CaptureError::NoCandidate));
}

/// Limits: text past the cap reads nothing; a move not in the game is no place.
#[test]
fn limits_are_said_not_panicked_on() {
    let g = main_line_game();
    let long = "x".repeat(MAX_CAPTURE_TEXT + 1);
    let p = plan(&g, 0, CaptureMode::Auto, CaptureText::Plain(&long), Lang::En);
    assert_eq!((p.candidates.len(), p.reasons.clone(), p.needs_preview), (0, vec![PreviewReason::TooLong], true));
    let p = plan(&g, 9999, CaptureMode::Line, CaptureText::Plain("20…Rc7"), Lang::En);
    assert_eq!(p.reasons, vec![PreviewReason::NoFit]);
    // Bold ranges out of the text, reversed or empty are ignored.
    let bold = [Range { start: 5, end: 2 }, 0..0, 3..usize::MAX, usize::MAX..usize::MAX];
    let p = plan(&g, g.current(), CaptureMode::Auto, CaptureText::Styled { text: "20…Rc7", bold: &bold }, Lang::En);
    assert!(p.chosen.is_some());
}

/// A place on the part two lines share is tried once, on the inner line.
#[test]
fn a_place_lines_share_is_one_candidate() {
    let mut g = scenario_game();
    g.play_san("Nd5").unwrap();
    // 18…Qxe1, with 18…Qd8 and 18…Qb6 as variations; the move shown is 18…Qb6.
    let mut current = g.current();
    for m in ["Qxe1", "Qd8", "Qb6"] {
        g.go_to_node(at(&g, &[0]));
        g.play_san(m).unwrap();
        current = g.current();
    }
    let p = plan_at(&g, current, CaptureMode::SubVariation, "18…Qc5");
    assert_eq!(p.candidates.len(), 1, "one place, shared by the lines through 18.Nd5");
    assert_eq!(p.candidates[0].line, 0);
    assert!(p.reasons.is_empty(), "{:?}", p.reasons);
}

#[cfg(not(target_arch = "wasm32"))]
mod properties {
    use proptest::prelude::*;

    use super::*;

    fn word() -> impl Strategy<Value = String> {
        prop_oneof![
            Just("18.Nd5".to_owned()),
            Just("Qxe1".to_owned()),
            Just("18…Qd8".to_owned()),
            Just("20.Rfxe1".to_owned()),
            Just("*".to_owned()),
            Just("(".to_owned()),
            Just(")".to_owned()),
            Just("1-0".to_owned()),
            Just("the".to_owned()),
            Just("\n".to_owned()),
            "[a-hKQRBNx1-8+#=O-]{1,6}",
            "[0-9]{1,3}[.…]{0,3}",
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(48))]

        #[test]
        fn any_text_plans_within_the_limits(words in proptest::collection::vec(word(), 0..40), mode in 0usize..6, deep in any::<bool>()) {
            let g = if deep { main_line_game() } else { scenario_game() };
            let mode = [CaptureMode::Auto, CaptureMode::Line, CaptureMode::Variation, CaptureMode::SubVariation, CaptureMode::Comment { before: true }, CaptureMode::Comment { before: false }][mode];
            let p = plan_at(&g, g.current(), mode, &words.join(" "));
            prop_assert!(p.candidates.len() <= 35);
            prop_assert_eq!(p.needs_preview, !p.reasons.is_empty());
            prop_assert!(p.chosen.is_none_or(|i| i < p.candidates.len()));
            prop_assert_eq!(p.base_revision, g.revision());
        }
    }
}

/// The cost of a plan (spec target: under 50 ms for the scenario's paragraph in a release build).
/// Run with `cargo test --release -p pdfcraft-engine --lib -- --ignored plan_cost`.
#[test]
#[ignore = "timing; run in a release build"]
fn plan_cost_on_the_scenario() {
    let mut deep = scenario_game();
    capture(&mut deep, CaptureMode::Auto, SCENARIO);
    // The move shown inside the sub-variation, three lines from the main line's start.
    let inner = at(&deep, &[0, 1, 0, 0, 1, 0, 0, 0]);
    for (name, game, current, mode) in [
        ("empty game, auto", scenario_game(), 0, CaptureMode::Auto),
        ("from the sub-variation, as a sub-variation", deep.clone(), inner, CaptureMode::SubVariation),
        ("from the sub-variation, as the line", deep.clone(), inner, CaptureMode::Line),
    ] {
        let runs = 20;
        let start = std::time::Instant::now();
        let mut candidates = 0;
        for _ in 0..runs {
            candidates = plan_at(&game, current, mode, SCENARIO).candidates.len();
        }
        let ms = start.elapsed().as_secs_f64() * 1000.0 / f64::from(runs);
        eprintln!("plan, {name}: {ms:.2} ms ({candidates} candidates)");
        assert!(ms < 500.0, "{name}: {ms} ms");
    }
}
