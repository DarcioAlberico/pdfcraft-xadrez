use super::*;
use crate::fragment::ply_of;
use crate::san::{Lang, San};

/// The position before White's 18th move in the scenario of the spec.
const FEN: &str = "2r2rk1/1p2ppb1/p2p1npp/q4P2/P1b1P2B/2NB4/1PP3PP/R3QR1K w - - 0 18";

/// Items from text the way a reader would give them: `18.Nd5` and `18...Qd8` carry their
/// printed half-move, `(` `)` open and close, a glyph (`!`, `↑`, `$36`) is a [`FragmentItem::Nag`]
/// by White's code, `{a_b}` is the comment "a b" after, `<a_b>` the one before, `*` a result.
fn items(text: &str) -> Vec<FragmentItem> {
    text.split_whitespace()
        .map(|word| match word {
            "(" => FragmentItem::Open,
            ")" => FragmentItem::Close,
            "*" => FragmentItem::Result,
            _ if word.starts_with('{') => FragmentItem::Comment { text: word.trim_matches(['{', '}']).replace('_', " "), before: false },
            _ if word.starts_with('<') => FragmentItem::Comment { text: word.trim_matches(['<', '>']).replace('_', " "), before: true },
            _ if word.starts_with('$') => FragmentItem::Nag(word[1..].parse().unwrap()),
            _ if crate::nag::by_glyph(word).is_some() => FragmentItem::Nag(crate::nag::by_glyph(word).unwrap().code),
            _ => {
                let digits = word.chars().take_while(char::is_ascii_digit).count();
                let (ply, san) = if digits > 0 {
                    let number: u32 = word[..digits].parse().unwrap();
                    let rest = &word[digits..];
                    let san = rest.trim_start_matches('.').trim_start_matches('…');
                    (Some(ply_of(number, rest.len() - san.len() > 1)), san)
                } else {
                    (None, word)
                };
                FragmentItem::Move { ply, san: San::read(san, Lang::En).unwrap(), text: word.to_owned(), bold: None, rect: None }
            }
        })
        .collect()
}

/// The move at `path`.
fn at(g: &Game, path: &[u16]) -> NodeId {
    g.node_at(&NodePath::new(path.to_vec())).unwrap()
}

fn path(p: &[u16]) -> NodePath {
    NodePath::new(p.to_vec())
}

/// `pgn → from_pgn → pgn` gives the same text, and the reader stops nowhere.
fn assert_fixpoint(g: &Game) {
    let pgn = g.pgn();
    let (back, stopped) = Game::from_pgn(&pgn).unwrap();
    assert_eq!(stopped, None, "{pgn}");
    assert_eq!(back.pgn(), pgn);
}

/// The scenario's main line, grafted at the start.
fn main_line() -> Game {
    let g = Game::from_fen(FEN).unwrap();
    let (g, report) = g.graft(0, GraftMode::Follow, &items("18.Nd5 Qxe1 19.Nxe7+ Kh7 20.Raxe1"), None);
    assert_eq!(report.stop, None);
    assert_eq!(report.played, 5);
    g
}

const VARIATION: &str = "18...Qd8 19.Bxf6 Bxf6 20.fxg6 ( 20.Nxf6+ exf6 21.Bxc4 Rxc4 ) 20...fxg6 21.Qg3 Kh7 22.e5";

#[test]
fn the_book_line_and_its_variation_are_grafted() {
    let g = main_line();
    assert_eq!(g.movetext(), "18. Nd5 Qxe1 19. Nxe7+ Kh7 20. Raxe1");
    let nd5 = at(&g, &[0]);
    let (g, report) = g.graft(nd5, GraftMode::Follow, &items(VARIATION), None);
    assert_eq!(
        g.movetext(),
        "18. Nd5 Qxe1 (18... Qd8 19. Bxf6 Bxf6 20. fxg6 (20. Nxf6+ exf6 21. Bxc4 Rxc4) 20... fxg6 21. Qg3 Kh7 22. e5) 19. Nxe7+ Kh7 20. Raxe1"
    );
    assert_eq!(report.stop, None);
    assert_eq!(report.played, 12);
    assert_eq!(report.new_nodes.len(), 12);
    assert_eq!(report.new_nodes[0], path(&[0, 1]));
    assert_eq!(report.new_nodes[4], path(&[0, 1, 0, 0, 1]), "20.Nxf6+ is the alternative to 20.fxg6");
    assert_eq!(report.anchor, path(&[0]));
    assert_eq!(report.last, Some(path(&[0, 1, 0, 0, 0, 0, 0, 0, 0])), "22.e5 is shown");
    assert_eq!(g.node_at(report.last.as_ref().unwrap()), Some(g.current()));
    assert!(!report.demoted && !report.extended_tip);
    assert!(report.unbalanced.is_empty() && report.ply_mismatches.is_empty() && report.unplaced.is_empty());
    assert_eq!((report.rest_from, report.leftover_text.as_str()), (None, ""));
    assert_fixpoint(&g);
}

#[test]
fn grafting_never_changes_the_game_it_is_called_on() {
    let g = main_line();
    let before = g.pgn();
    let revision = g.revision();
    let _ = g.graft(at(&g, &[0]), GraftMode::Line, &items(VARIATION), Some(3));
    assert_eq!((g.pgn(), g.revision()), (before, revision));
}

#[test]
fn a_black_move_fits_on_a_start_where_black_moves() {
    // `… b - - 0 18`: the start itself is half-move 35, that of `18… Qd8`.
    let g = Game::from_fen(&FEN.replace(" w ", " b ")).unwrap();
    let (g2, report) = g.graft(0, GraftMode::Follow, &items("18...Qd8 19.Nd5"), None);
    assert_eq!(report.played, 2);
    assert_eq!(report.ply_mismatches, Vec::<usize>::new());
    assert_eq!(g2.movetext(), "18... Qd8 19. Nd5");
    // Numbers that disagree are reported, but the moves are still played.
    let (_, report) = g.graft(0, GraftMode::Follow, &items("19...Qd8 20.Nd5"), None);
    assert_eq!((report.played, report.ply_mismatches), (2, vec![0, 1]));
}

#[test]
fn glyphs_take_the_code_of_the_side_they_speak_of() {
    let g = main_line();
    let nd5 = at(&g, &[0]);
    let (g, report) = g.graft(nd5, GraftMode::Follow, &items("Qxe1 ! ↑ 19.Nxe7+ ↑ $37 Kh7 $22 20.Raxe1 $22"), None);
    assert!(report.unplaced.is_empty());
    // Black's Qxe1: `!` stays 1, `↑` becomes 37.
    assert_eq!(g.nags(at(&g, &[0, 0])), &[1, 37]);
    // White's Nxe7+: `↑` is 36, and Black's code given for it is mapped back.
    assert_eq!(g.nags(at(&g, &[0, 0, 0])), &[36]);
    // Zugzwang speaks of the side to move: White after Black's Kh7, Black after White's Raxe1.
    assert_eq!(g.nags(at(&g, &[0, 0, 0, 0])), &[22]);
    assert_eq!(g.nags(at(&g, &[0, 0, 0, 0, 0])), &[23]);
    // Glyphs glued to a move are set too, and setting never takes a glyph off.
    let (g, _) = g.graft(nd5, GraftMode::Follow, &items("Qxe1!?"), None);
    assert_eq!(g.nags(at(&g, &[0, 0])), &[5, 37]);
    assert_fixpoint(&g);
}

#[test]
fn a_glyph_with_no_move_before_it_is_left_over() {
    let g = main_line();
    let (g2, report) = g.graft(0, GraftMode::Follow, &items("± 18.Nd5 ( ∞ 18.Bxf6 )"), None);
    assert_eq!(report.unplaced, vec![0, 3]);
    assert_eq!(report.leftover_text, "± ∞");
    assert_eq!(report.played, 2);
    assert_eq!(g2.nags(at(&g2, &[0])), &[] as &[u16]);
    assert_eq!(g2.nags(at(&g2, &[1])), &[] as &[u16]);
}

#[test]
fn line_mode_makes_a_new_move_the_line_and_demotes_the_old_one() {
    let g = main_line();
    let nd5 = at(&g, &[0]);
    let (g, report) = g.graft(nd5, GraftMode::Line, &items("18...Qd8 19.Bxf6 ( 19.Qg3 ) Bxf6"), None);
    assert!(report.demoted);
    assert_eq!(g.movetext(), "18. Nd5 Qd8 (18... Qxe1 19. Nxe7+ Kh7 20. Raxe1) 19. Bxf6 (19. Qg3) 19... Bxf6");
    assert_eq!(report.new_nodes, vec![path(&[0, 0]), path(&[0, 0, 0]), path(&[0, 0, 1]), path(&[0, 0, 0, 0])]);
    assert_eq!(report.last, Some(path(&[0, 0, 0, 0])));
    assert_fixpoint(&g);
    // A move already there is followed, in either mode: nothing is demoted again.
    let (g2, report) = g.graft(nd5, GraftMode::Line, &items("18...Qxe1 19.Nxe7+"), None);
    assert!(!report.demoted && report.new_nodes.is_empty());
    assert_eq!(g2.pgn(), g.pgn());
    // At a tip, the line is extended.
    let tip = at(&g, &[0, 0, 0, 0]);
    let (g3, report) = g.graft(tip, GraftMode::Line, &items("20.Nxf6+"), None);
    assert!(report.extended_tip && !report.demoted);
    assert!(g3.movetext().ends_with("19... Bxf6 20. Nxf6+"));
}

#[test]
fn follow_mode_adds_a_variation_and_never_duplicates_a_move() {
    let g = main_line();
    let (g2, report) = g.graft(0, GraftMode::Follow, &items("18.Nd5 Qxe1 19.Nxe7+ Kh7 20.Raxe1"), None);
    assert_eq!((report.played, report.new_nodes.len()), (5, 0));
    assert_eq!(g2.pgn(), g.pgn());
    assert_eq!(g2.revision(), g.revision());
    let (g3, report) = g.graft(0, GraftMode::Follow, &items("18.Nd5 Qd8"), None);
    assert_eq!(report.new_nodes, vec![path(&[0, 1])]);
    assert!(!report.extended_tip);
    assert!(g3.movetext().starts_with("18. Nd5 Qxe1 (18... Qd8) 19. Nxe7+"));
}

#[test]
fn grafting_the_same_items_twice_changes_nothing_the_second_time() {
    let g = main_line();
    let nd5 = at(&g, &[0]);
    let text = format!("<the_try> {VARIATION} ! {{a_comment}} <before_nothing> ) ( 22.Qh4 {{and_white_attacks}} ) 22...d5 *");
    let fragment = items(&text);
    let (once, first) = g.graft(nd5, GraftMode::Line, &fragment, Some(7));
    let (twice, second) = once.graft(nd5, GraftMode::Line, &fragment, Some(7));
    assert_eq!(twice.pgn(), once.pgn());
    assert_eq!(twice.revision(), once.revision());
    assert!(second.new_nodes.is_empty() && !second.demoted);
    assert_eq!(second.played, first.played);
    assert_eq!(second.unplaced, first.unplaced);
    assert!(!once.pgn().contains("a comment a comment"));
    assert_fixpoint(&once);
}

#[test]
fn comments_go_after_their_move_or_before_the_next() {
    let g = main_line();
    let nd5 = at(&g, &[0]);
    let fragment = items("<the_try> 18...Qd8 {solid} {solid} <now> 19.Bxf6 ( {only_prose} ) <dangling>");
    let (g2, report) = g.graft(nd5, GraftMode::Follow, &fragment, None);
    let qd8 = at(&g2, &[0, 1]);
    assert_eq!(g2.comment_before(qd8), "the try");
    assert_eq!(g2.comment(qd8), "solid", "a comment already there is not added again");
    assert_eq!(g2.comment_before(at(&g2, &[0, 1, 0])), "now");
    assert_eq!(report.unplaced, vec![7, 9]);
    assert_eq!(report.leftover_text, "only prose dangling");
    assert_eq!(g2.movetext(), "18. Nd5 Qxe1 ({the try} 18... Qd8 {solid now} 19. Bxf6) 19. Nxe7+ Kh7 20. Raxe1");
    assert_fixpoint(&g2);
    // With no move before it, a comment waits for the next move.
    let (g3, _) = g.graft(nd5, GraftMode::Follow, &items("{prose} 18...Qd8"), None);
    assert_eq!(g3.comment_before(at(&g3, &[0, 1])), "prose");
    // A comment that would not fit is left over, never cut.
    let long = "x".repeat(MAX_COMMENT_LEN + 1);
    let (g4, report) = g.graft(nd5, GraftMode::Follow, &[items("Qxe1")[0].clone(), FragmentItem::Comment { text: long, before: false }], None);
    assert_eq!(report.unplaced, vec![1]);
    assert_eq!(g4.comment(at(&g4, &[0, 0])), "");
}

#[test]
fn an_open_before_any_move_is_an_alternative_to_the_anchor() {
    let g = main_line();
    // After 18.Nd5 Qxe1, `( 18...Qd8 )` replaces Qxe1, and the line goes on after it.
    let qxe1 = at(&g, &[0, 0]);
    let (g2, report) = g.graft(qxe1, GraftMode::Follow, &items("( 18...Qd8 ) 19.Nxe7+"), None);
    assert!(report.unbalanced.is_empty());
    assert_eq!(report.new_nodes, vec![path(&[0, 1])]);
    assert_eq!(report.last, Some(path(&[0, 0, 0])));
    assert!(g2.movetext().starts_with("18. Nd5 Qxe1 (18... Qd8) 19. Nxe7+"));
    // The start has nothing to be an alternative to: the move is a reply to it.
    let (g3, report) = g.graft(0, GraftMode::Follow, &items("( 18.Bxf6 )"), None);
    assert_eq!(report.new_nodes, vec![path(&[1])]);
    assert!(g3.movetext().starts_with("18. Nd5 (18. Bxf6) 18... Qxe1"));
    // Two `(` with no move between branch from the same move.
    let (g4, report) = g.graft(qxe1, GraftMode::Follow, &items("19.Nxe7+ ( ( 19.Nf4 ) 19.Bxf6 )"), None);
    assert_eq!((report.stop, report.new_nodes), (None, vec![path(&[0, 0, 1]), path(&[0, 0, 2])]));
    assert!(g4.movetext().starts_with("18. Nd5 Qxe1 19. Nxe7+ (19. Nf4) (19. Bxf6) 19... Kh7"));
    assert_fixpoint(&g4);
}

#[test]
fn unbalanced_parentheses_are_reported() {
    let g = main_line();
    let (g2, report) = g.graft(0, GraftMode::Follow, &items("18.Nd5 ) Qxe1 ( 18...Qd8 19.Bxf6"), None);
    assert_eq!(report.unbalanced, vec![1, 3]);
    assert_eq!(report.played, 4);
    assert_eq!(report.stop, None);
    assert!(g2.movetext().starts_with("18. Nd5 Qxe1 (18... Qd8 19. Bxf6) 19. Nxe7+"));
}

#[test]
fn the_graft_stops_at_the_first_move_that_does_not_play() {
    let g = main_line();
    let nd5 = at(&g, &[0]);
    let (g2, report) = g.graft(nd5, GraftMode::Follow, &items("18...Qd8 19.Bxf6 Qh1 {never_read} 20.e5"), None);
    let stop = report.stop.clone().unwrap();
    assert_eq!((stop.index, stop.reason, stop.text.as_str(), stop.maybe_en_passant), (2, GraftStopReason::Illegal, "Qh1", false));
    assert_eq!(report.rest_from, Some(2));
    assert_eq!(report.leftover_text, "Qh1 never read 20.e5");
    assert_eq!(report.played, 2);
    assert!(g2.movetext().contains("(18... Qd8 19. Bxf6)"));
    // The stop keeps the position the move was tried in (after 19.Bxf6), for the moves near it.
    let bxf6 = g2.node_at(&report.new_nodes[1]).unwrap();
    assert_eq!(stop.fen.as_deref(), Some(g2.fen_at(bxf6).unwrap().as_str()));

    let (_, report) = g.graft(at(&g, &[0, 0]), GraftMode::Follow, &items("19.Nxe7+ Kh7 Rxe1"), None);
    assert_eq!(report.stop.unwrap().reason, GraftStopReason::Ambiguous);

    // A null move plays (a threat shown by passing), unless the side to move is in check.
    let null = FragmentItem::Move { ply: None, san: San::read("--", Lang::En).unwrap(), text: "--".into(), bold: None, rect: None };
    let (passed, report) = g.graft(0, GraftMode::Follow, std::slice::from_ref(&null), None);
    assert_eq!((report.stop, report.played), (None, 1));
    assert!(passed.movetext().contains("(18. --)"), "{}", passed.movetext());
    let check = at(&g, &[0, 0, 0]);
    assert_eq!(g.san(check), "Nxe7+");
    let (_, report) = g.graft(check, GraftMode::Follow, &[null], None);
    assert_eq!(report.stop.unwrap().reason, GraftStopReason::NullMove);

    let (g3, report) = g.graft(usize::MAX, GraftMode::Follow, &items("18.Nd5"), None);
    assert_eq!((report.stop.unwrap().reason, report.rest_from), (GraftStopReason::NoAnchor, Some(0)));
    assert_eq!(g3.pgn(), g.pgn());

    // After a result nothing is read, and what follows is left over.
    let (_, report) = g.graft(0, GraftMode::Follow, &items("18.Nd5 * 18...Qd8"), None);
    assert_eq!((report.stop, report.rest_from, report.leftover_text.as_str()), (None, Some(2), "18...Qd8"));
}

#[test]
fn en_passant_right_after_a_diagram_is_named_as_the_likely_reason() {
    // Black's d7-d5 was the last move, but a diagram's FEN cannot say so.
    let g = Game::from_fen("4k3/8/8/3pP3/8/8/8/4K3 w - - 0 30").unwrap();
    let (_, report) = g.graft(0, GraftMode::Follow, &items("30.exd6"), None);
    let stop = report.stop.unwrap();
    assert_eq!((stop.reason, stop.maybe_en_passant), (GraftStopReason::Illegal, true));
    let (_, report) = g.graft(0, GraftMode::Follow, &items("30.Kd2 Kd7 31.exd6"), None);
    assert!(!report.stop.unwrap().maybe_en_passant);
}

#[test]
fn limits_stop_the_graft_with_their_reason() {
    // Too many items: the graft stops at the cap, not before.
    let mut many = vec![FragmentItem::Comment { text: "x".into(), before: false }; MAX_FRAGMENT_ITEMS + 2];
    many[0] = items("18.Nd5")[0].clone();
    let g = Game::from_fen(FEN).unwrap();
    let (_, report) = g.graft(0, GraftMode::Follow, &many, None);
    let stop = report.stop.unwrap();
    assert_eq!((stop.index, stop.reason), (MAX_FRAGMENT_ITEMS, GraftStopReason::TooLong));
    assert_eq!(report.rest_from, Some(MAX_FRAGMENT_ITEMS));
    assert_eq!(report.played, 1);
    // Too deep: a variation under a move already MAX_DEPTH deep.
    let mut g = Game::standard();
    g.play_san("e4").unwrap();
    for _ in 0..MAX_DEPTH {
        let mut moves = g.legal_moves();
        moves.dedup();
        let here = g.current();
        g.play_san(&moves[0]).unwrap();
        g.go_to_node(here);
        g.play_san(&moves[1]).unwrap();
    }
    let deepest = g.current();
    assert_eq!(g.nodes[deepest].depth, MAX_DEPTH);
    // A reply continues the line at the same depth; another one would be a variation too deep.
    let moves = g.legal_moves();
    g.play_san(&moves[0]).unwrap();
    let other = moves.iter().find(|m| **m != moves[0]).unwrap();
    let alt = FragmentItem::Move { ply: None, san: San::parse(other).unwrap(), text: other.clone(), bold: None, rect: None };
    let (g1, report) = g.graft(deepest, GraftMode::Follow, std::slice::from_ref(&alt), None);
    assert_eq!(report.stop.unwrap().reason, GraftStopReason::TooDeep);
    assert_eq!(g1.pgn(), g.pgn());
    // Making it the line would push the reply already there one deeper: refused too.
    let (g2, report) = g.graft(deepest, GraftMode::Line, &[alt], None);
    assert_eq!(report.stop.unwrap().reason, GraftStopReason::TooDeep);
    assert_eq!(g2.pgn(), g.pgn());
}

#[test]
fn rectangles_become_sources_when_the_page_is_known() {
    let g = Game::from_fen(FEN).unwrap();
    let mut fragment = items("18.Nd5 Qxe1");
    if let FragmentItem::Move { rect, .. } = &mut fragment[0] {
        *rect = Some([10.0, 20.0, 30.0, 40.0]);
    }
    if let FragmentItem::Move { rect, .. } = &mut fragment[1] {
        *rect = Some([f32::NAN, 0.0, 1.0, 1.0]);
    }
    let (without, _) = g.graft(0, GraftMode::Follow, &fragment, None);
    assert_eq!(without.source(at(&without, &[0])), None);
    let (with, report) = g.graft(0, GraftMode::Follow, &fragment, Some(4));
    assert_eq!(report.played, 2, "a rectangle that is not finite is skipped, the move is not");
    assert_eq!(with.source(at(&with, &[0])), Some(NodeSource { page: 4, rect_user: [10.0, 20.0, 30.0, 40.0] }));
    assert_eq!(with.source(at(&with, &[0, 0])), None);
    // A source already there is kept.
    if let FragmentItem::Move { rect, .. } = &mut fragment[0] {
        *rect = Some([1.0, 1.0, 2.0, 2.0]);
    }
    let (again, _) = with.graft(0, GraftMode::Follow, &fragment, Some(9));
    assert_eq!(again.source(at(&again, &[0])), with.source(at(&with, &[0])));
}

#[test]
fn comment_mode_writes_the_items_as_text() {
    let g = main_line();
    let nd5 = at(&g, &[0]);
    let fragment = items("18...Qd8 ! 19.Bxf6 ( 19.Qg3 ) Bxf6 {and_so_on} *");
    let (g2, report) = g.graft(nd5, GraftMode::Comment, &fragment, None);
    assert_eq!((report.played, report.stop), (0, None));
    assert_eq!(g2.comment(nd5), "18...Qd8 ! 19.Bxf6 (19.Qg3) Bxf6 and so on");
    // Again: nothing is added twice.
    let (g3, _) = g2.graft(nd5, GraftMode::Comment, &fragment, None);
    assert_eq!(g3.pgn(), g2.pgn());
    // On a move that starts a variation, the text goes before it.
    let (g4, _) = g.graft(nd5, GraftMode::Follow, &items("18...Qd8"), None);
    let qd8 = at(&g4, &[0, 1]);
    let (g5, _) = g4.graft(qd8, GraftMode::Comment, &items("19.Bxf6"), None);
    assert_eq!((g5.comment_before(qd8), g5.comment(qd8)), ("19.Bxf6", ""));
    assert_fixpoint(&g5);
    // Text that would not fit is left over whole.
    let long = vec![FragmentItem::Comment { text: "y".repeat(MAX_COMMENT_LEN), before: false }];
    let (g6, report) = g2.graft(nd5, GraftMode::Comment, &long, None);
    let stop = report.stop.unwrap();
    assert_eq!((stop.index, stop.reason, report.rest_from), (0, GraftStopReason::TooLong, Some(0)));
    assert_eq!(g6.pgn(), g2.pgn());
    assert_eq!(report.leftover_text.len(), MAX_COMMENT_LEN);
}

/// The tree is whole: parents and replies agree, depths are right and within the caps, paths
/// name their moves, and the PGN is a fixpoint.
fn check(g: &Game) -> Result<(), proptest::test_runner::TestCaseError> {
    use proptest::{prop_assert, prop_assert_eq};
    let n = g.nodes.len();
    prop_assert!(n <= MAX_NODES);
    prop_assert!(g.current() < n);
    let mut seen = vec![0usize; n];
    for (id, node) in g.nodes.iter().enumerate() {
        for (i, &c) in node.children.iter().enumerate() {
            prop_assert!(c != 0 && c < n);
            prop_assert_eq!(g.nodes[c].parent, id);
            seen[c] += 1;
            prop_assert_eq!(g.nodes[c].depth, node.depth + usize::from(i > 0));
            prop_assert!(g.nodes[c].depth <= MAX_DEPTH);
        }
    }
    prop_assert!(seen.iter().skip(1).all(|&s| s == 1));
    for id in 0..n {
        prop_assert_eq!(g.node_at(&g.path_of(id).unwrap_or_default()), Some(id));
    }
    let pgn = g.pgn();
    let (back, stopped) = Game::from_pgn(&pgn).map_err(|e| proptest::test_runner::TestCaseError::fail(e.to_string()))?;
    prop_assert_eq!(stopped, None);
    prop_assert_eq!(back.pgn(), pgn);
    Ok(())
}

const TEXTS: [&str; 6] = ["a", "two words", "{braces}", "  spaced \n out ", "é (x) ñ", "1. e4 ; % [x]"];
const NAGS: [u16; 9] = [0, 1, 2, 5, 14, 22, 36, 146, 300];
const JUNK: [&str; 4] = ["Qh8", "Nf3", "O-O-O", "e8=Q"];

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(96))]

    #[test]
    fn random_fragments_graft_within_the_limits(
        base in proptest::collection::vec(0usize..64, 0..12),
        stream in proptest::collection::vec((0u8..12, 0usize..64), 0..48),
        anchor in 0usize..64,
        mode in 0u8..3,
        fen in proptest::bool::ANY,
    ) {
        let mut g = if fen { Game::from_fen(FEN).unwrap() } else { Game::standard() };
        // A base game with a few variations.
        for (i, pick) in base.iter().enumerate() {
            if i % 4 == 3 {
                g.go_to_node(pick % g.nodes.len());
            }
            let moves = g.legal_moves();
            if !moves.is_empty() {
                let _ = g.play_san(&moves[pick % moves.len()]);
            }
        }
        let at = anchor % g.nodes.len();
        // The items follow a shadow game, so most moves are legal where they land.
        let mut shadow = g.clone();
        shadow.go_to_node(at);
        let mut fragment = Vec::new();
        for (kind, pick) in stream {
            fragment.push(match kind {
                0..=4 => {
                    let moves = shadow.legal_moves();
                    if moves.is_empty() {
                        continue;
                    }
                    let san = &moves[pick % moves.len()];
                    let ply = shadow.abs_ply(shadow.current());
                    let _ = shadow.play_san(san);
                    FragmentItem::Move { ply: ply.filter(|_| pick % 2 == 0), san: San::parse(san).unwrap(), text: san.clone(), bold: None, rect: Some([pick as f32, 0.0, 1.0, 1.0]) }
                }
                5 => {
                    let junk = JUNK[pick % JUNK.len()];
                    FragmentItem::Move { ply: None, san: San::parse(junk).unwrap(), text: junk.into(), bold: None, rect: None }
                }
                6 => {
                    shadow.back();
                    FragmentItem::Open
                }
                7 => FragmentItem::Close,
                8 => FragmentItem::Nag(NAGS[pick % NAGS.len()]),
                9 | 10 => FragmentItem::Comment { text: TEXTS[pick % TEXTS.len()].into(), before: pick % 3 == 0 },
                _ => FragmentItem::Result,
            });
        }
        let mode = [GraftMode::Follow, GraftMode::Line, GraftMode::Comment][usize::from(mode)];
        let before = g.pgn();
        let (once, report) = g.graft(at, mode, &fragment, Some(1));
        proptest::prop_assert_eq!(g.pgn(), before);
        check(&once)?;
        proptest::prop_assert!(report.unplaced.iter().chain(&report.unbalanced).chain(&report.ply_mismatches).all(|&i| i < fragment.len()));
        proptest::prop_assert!(report.leftover_text.len() <= MAX_LEFTOVER_LEN);
        for p in report.new_nodes.iter().chain(&report.last) {
            proptest::prop_assert!(once.node_at(p).is_some());
        }
        if let Some(stop) = &report.stop {
            proptest::prop_assert_eq!(report.rest_from, Some(stop.index));
        }
        // Grafting the same items again changes nothing.
        let (twice, again) = once.graft(at, mode, &fragment, Some(1));
        check(&twice)?;
        proptest::prop_assert_eq!(twice.pgn(), once.pgn());
        proptest::prop_assert!(again.new_nodes.is_empty());
    }
}
