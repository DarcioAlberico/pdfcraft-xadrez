use super::*;
use crate::game::{DrawColor, Drawing, GameError, MAX_NODES, Token};
use crate::placement::Square;

/// The game after `moves` (SAN), shown at the last one.
fn game(moves: &[&str]) -> Game {
    let mut g = Game::standard();
    for m in moves {
        g.play_san(m).unwrap();
    }
    g
}

/// Play `moves` (SAN) after move `at`; returns the last one.
fn play_at(g: &mut Game, at: NodeId, moves: &[&str]) -> NodeId {
    g.go_to_node(at);
    for m in moves {
        g.play_san(m).unwrap();
    }
    g.current()
}

/// The move at `path`.
fn at(g: &Game, path: &[u16]) -> NodeId {
    g.node_at(&NodePath::new(path.to_vec())).unwrap()
}

/// Everything about the tree that does not depend on node ids, depth first: each move's path,
/// SAN, comments, glyphs, drawings and source, and the path of the move shown.
fn dump(g: &Game) -> String {
    let mut out = String::new();
    let mut stack = vec![0];
    while let Some(id) = stack.pop() {
        out.push_str(&format!(
            "{:?} {} [{}] [{}] {:?} {:?} {:?}\n",
            g.path_of(id).unwrap().indices(),
            g.san(id),
            g.comment(id),
            g.comment_before(id),
            g.nags(id),
            g.drawings(id),
            g.source(id)
        ));
        stack.extend(g.nodes[id].children.iter().rev());
    }
    out.push_str(&format!("shown {:?}", g.path_of(g.current()).unwrap().indices()));
    out
}

/// `pgn → from_pgn → pgn` gives the same text, and the reader stops nowhere.
fn assert_fixpoint(g: &Game) {
    let pgn = g.pgn();
    let (back, stopped) = Game::from_pgn(&pgn).unwrap();
    assert_eq!(stopped, None, "{pgn}");
    assert_eq!(back.pgn(), pgn);
}

/// Under the move shown, nest `levels` variations one inside the other (each the second reply to
/// the move before), so the deepest is `levels` deeper than the move shown.
fn nest(g: &mut Game, levels: usize) {
    for _ in 0..levels {
        let mut moves = g.legal_moves();
        moves.dedup();
        let here = g.current();
        g.play_san(&moves[0]).unwrap();
        g.go_to_node(here);
        g.play_san(&moves[1]).unwrap();
    }
}

fn max_depth(g: &Game) -> usize {
    g.nodes.iter().map(|n| n.depth).max().unwrap_or(0)
}

#[test]
fn a_comment_before_a_first_reply_is_written_with_the_comment_before_it() {
    let mut g = game(&["e4", "e5", "Nf3"]);
    let (e4, e5, nf3) = (at(&g, &[0]), at(&g, &[0, 0]), at(&g, &[0, 0, 0]));
    g.set_comment_before(nf3, "now {the} knight");
    assert_eq!(g.movetext(), "1. e4 e5 {now (the) knight} 2. Nf3");
    g.set_comment(e5, "solid");
    assert_eq!(g.movetext(), "1. e4 e5 {solid now (the) knight} 2. Nf3");
    assert_fixpoint(&g);
    // It stays on its move; reading the PGN back puts it after the move before.
    assert_eq!((g.comment(e5), g.comment_before(nf3)), ("solid", "now (the) knight"));
    let (back, _) = Game::from_pgn(&g.pgn()).unwrap();
    assert_eq!(back.comment(at(&back, &[0, 0])), "solid now (the) knight");
    // The first move of the game: with the game's own comment.
    g.set_comment_before(e4, "first");
    g.set_comment(0, "Intro");
    assert_eq!(g.movetext(), "{Intro first} 1. e4 e5 {solid now (the) knight} 2. Nf3");
    assert_fixpoint(&g);
    // With variations of the move before, it comes before them, where PGN has room for it.
    play_at(&mut g, e4, &["c5"]);
    assert_eq!(g.movetext(), "{Intro first} 1. e4 e5 {solid now (the) knight} (1... c5) 2. Nf3");
    assert_fixpoint(&g);
    // The tokens keep the two apart.
    let tokens = g.tokens();
    assert!(tokens.contains(&Token::Comment { node: nf3, text: "now (the) knight".into(), before: true }), "{tokens:?}");
    assert!(tokens.contains(&Token::Comment { node: e5, text: "solid".into(), before: false }), "{tokens:?}");
    // The start has no comment before it.
    g.set_comment_before(0, "x");
    assert_eq!(g.comment_before(0), "");
}

#[test]
fn a_comment_before_a_variation_is_written_after_its_bracket() {
    let mut g = game(&["e4", "e5", "Nf3"]);
    let c5 = play_at(&mut g, 1, &["c5"]);
    play_at(&mut g, c5, &["Nf3"]);
    g.set_comment_before(c5, "an idea");
    assert_eq!(g.movetext(), "1. e4 e5 ({an idea} 1... c5 2. Nf3) 2. Nf3");
    assert_fixpoint(&g);
    let (back, _) = Game::from_pgn(&g.pgn()).unwrap();
    let c5_back = at(&back, &[0, 1]);
    assert_eq!((back.comment_before(c5_back), back.comment(c5_back)), ("an idea", ""));
    let tokens = g.tokens();
    let i = tokens.iter().position(|t| *t == Token::VariationStart).unwrap();
    assert_eq!(tokens[i + 1], Token::Comment { node: c5, text: "an idea".into(), before: true });
}

#[test]
fn abs_ply_counts_half_moves_from_move_one() {
    let mut g = Game::from_fen("6k1/5ppp/8/8/8/8/5PPP/3R2K1 b - - 0 18").unwrap();
    // `18… Kh8` is half-move 35, so it fits on the start itself.
    assert_eq!(g.abs_ply(0), Some(35));
    g.play_san("Kh8").unwrap();
    let kh8 = g.current();
    assert_eq!(g.abs_ply(kh8), Some(36));
    g.play_san("Rd8#").unwrap();
    assert_eq!(g.abs_ply(g.current()), Some(37));
    // Each move's own half-move is its parent's, and matches its number and side.
    for (id, (_, number, color, _)) in g.line_of(g.current()).into_iter().skip(1).zip(g.moves()) {
        let parent = g.nodes[id].parent;
        assert_eq!(g.abs_ply(parent), Some(2 * (number - 1) + u32::from(color == crate::placement::Color::Black)));
    }
    let g = game(&["e4", "e5"]);
    assert_eq!([g.abs_ply(0), g.abs_ply(1), g.abs_ply(2), g.abs_ply(3)], [Some(0), Some(1), Some(2), None]);
}

#[test]
fn heads_are_listed_nearest_first() {
    let mut g = game(&["e4", "e5", "Nf3", "Nc6"]);
    let e4 = at(&g, &[0]);
    play_at(&mut g, e4, &["c5", "Nf3", "d6"]);
    let nf3 = at(&g, &[0, 1, 0]);
    let nc6 = play_at(&mut g, nf3, &["Nc6", "d4"]);
    let (c5, nc6_head) = (at(&g, &[0, 1]), at(&g, &[0, 1, 0, 1]));
    assert_eq!(g.heads_on_path(nc6), vec![nc6_head, c5]);
    assert_eq!(g.heads_on_path(nc6_head), vec![nc6_head, c5]);
    // On the main line, the first reply that has a variation beside it.
    let e5 = at(&g, &[0, 0]);
    assert_eq!(g.heads_on_path(at(&g, &[0, 0, 0, 0])), vec![e5]);
    assert_eq!(g.heads_on_path(e4), Vec::<NodeId>::new());
    assert_eq!(g.heads_on_path(0), Vec::<NodeId>::new());
    assert_eq!(g.heads_on_path(9999), Vec::<NodeId>::new());
}

#[test]
fn paths_name_moves_and_lines_follow_first_replies() {
    let mut g = game(&["e4", "e5", "Nf3"]);
    play_at(&mut g, 1, &["c5", "Nf3"]);
    for id in 0..g.nodes.len() {
        assert_eq!(g.node_at(&g.path_of(id).unwrap()), Some(id));
    }
    assert!(g.path_of(0).unwrap().is_start());
    assert_eq!(g.path_of(99), None);
    assert_eq!(g.node_at(&NodePath::from(vec![0, 2])), None);
    assert_eq!(g.node_at(&NodePath::from(vec![0, 0, 0, 0])), None);
    let c5 = at(&g, &[0, 1]);
    let sans = |line: Vec<NodeId>| line.into_iter().map(|id| g.san(id).to_owned()).collect::<Vec<_>>();
    assert_eq!(sans(g.line_of(c5)), ["", "e4", "c5", "Nf3"]);
    assert_eq!(sans(g.main_line()), ["", "e4", "e5", "Nf3"]);
    assert_eq!(g.line_of(99), Vec::<NodeId>::new());
}

#[test]
fn refusals_say_why_and_change_nothing() {
    let mut g = game(&["e4", "e5", "Nf3"]);
    let c5 = play_at(&mut g, 1, &["c5"]);
    let (e4, e5, nf3) = (at(&g, &[0]), at(&g, &[0, 0]), at(&g, &[0, 0, 0]));
    let (before, revision) = (dump(&g), g.revision());
    assert_eq!(g.promote_one(e5), Err(StructureError::AlreadyFirst));
    assert_eq!(g.promote_one(0), Err(StructureError::NoSuchMove));
    assert_eq!(g.promote_one(9999), Err(StructureError::NoSuchMove));
    assert_eq!(g.demote(c5), Err(StructureError::NotFirst));
    assert_eq!(g.demote(e4), Err(StructureError::NoAlternative));
    assert_eq!(g.demote(nf3), Err(StructureError::NoAlternative));
    assert_eq!(g.demote(0), Err(StructureError::NoSuchMove));
    assert_eq!(g.delete_variation(e5), Err(StructureError::AlreadyFirst));
    assert_eq!(g.delete_variation(e4), Err(StructureError::AlreadyFirst));
    assert_eq!(g.delete_variation(0), Err(StructureError::NoSuchMove));
    assert_eq!((dump(&g), g.revision()), (before, revision));
}

#[test]
fn promote_one_moves_a_variation_up_one_and_demote_undoes_it() {
    let mut g = game(&["e4", "e5", "Nf3"]);
    let e4 = at(&g, &[0]);
    play_at(&mut g, e4, &["c5", "Nf3"]);
    play_at(&mut g, e4, &["e6"]);
    let shown = play_at(&mut g, e4, &["c6"]);
    let c5 = at(&g, &[0, 1]);
    g.set_comment(c5, "sharp");
    g.set_comment_before(c5, "or");
    assert!(g.set_nag(c5, 1));
    assert!(g.set_source(c5, Some(NodeSource { page: 3, rect_user: [1.0, 2.0, 3.0, 4.0] })));
    g.go_to_node(shown);
    assert_eq!(g.movetext(), "1. e4 e5 ({or} 1... c5 $1 {sharp} 2. Nf3) (1... e6) (1... c6) 2. Nf3");
    let before = dump(&g);
    g.promote_one(c5).unwrap();
    // A first reply now: its comment-before comes after the move before.
    assert_eq!(g.movetext(), "1. e4 {or} 1... c5 $1 {sharp} (1... e5 2. Nf3) (1... e6) (1... c6) 2. Nf3");
    let c5 = at(&g, &[0, 0]);
    assert_eq!((g.comment_before(c5), g.source(c5).map(|s| s.page)), ("or", Some(3)));
    assert_eq!(g.san(g.current()), "c6", "the move shown follows its renumbering");
    assert_fixpoint(&g);
    g.demote(c5).unwrap();
    assert_eq!(dump(&g), before);
    // From index 2 the old first reply is second, and demote does not undo it.
    let c6 = at(&g, &[0, 3]);
    g.promote_one(c6).unwrap();
    assert_eq!(g.movetext(), "1. e4 c6 (1... e5 2. Nf3) ({or} 1... c5 $1 {sharp} 2. Nf3) (1... e6)");
    g.demote(at(&g, &[0, 0])).unwrap();
    assert_eq!(g.movetext(), "1. e4 e5 (1... c6) ({or} 1... c5 $1 {sharp} 2. Nf3) (1... e6) 2. Nf3");
}

#[test]
fn delete_variation_takes_its_moves_and_brings_the_move_shown_back() {
    let mut g = game(&["e4", "e5", "Nf3"]);
    let e4 = at(&g, &[0]);
    play_at(&mut g, e4, &["c5", "Nf3"]);
    let shown = play_at(&mut g, e4, &["e6", "d4"]);
    g.go_to_node(shown);
    g.delete_variation(at(&g, &[0, 1])).unwrap();
    assert_eq!(g.movetext(), "1. e4 e5 (1... e6 2. d4) 2. Nf3");
    assert_eq!(g.san(g.current()), "d4", "the move shown survives");
    g.delete_variation(at(&g, &[0, 1])).unwrap();
    assert_eq!(g.movetext(), "1. e4 e5 2. Nf3");
    assert_eq!(g.current(), at(&g, &[0]), "the move shown was deleted: its head's parent");
}

#[test]
fn reshaping_is_refused_past_the_depth_cap_and_allowed_up_to_it() {
    for (levels, allowed) in [(MAX_DEPTH - 1, true), (MAX_DEPTH, false)] {
        // e4 carries variations `levels` deep; d4 is the start's second reply.
        let mut g = game(&["e4"]);
        nest(&mut g, levels);
        assert_eq!(max_depth(&g), levels);
        let d4 = play_at(&mut g, 0, &["d4"]);
        let before = dump(&g);
        // Both push e4's variations one deeper.
        let expected = if allowed { Ok(()) } else { Err(StructureError::TooDeep) };
        assert_eq!(g.promote_one(d4), expected, "{levels}");
        if allowed {
            assert_eq!(max_depth(&g), MAX_DEPTH);
            g.demote(at(&g, &[0])).unwrap();
            assert_eq!(dump(&g), before);
        } else {
            assert_eq!(dump(&g), before);
        }
        assert_eq!(g.demote(at(&g, &[0])), expected, "{levels}");
        assert!(max_depth(&g) <= MAX_DEPTH);
    }
}

#[test]
fn set_nag_sets_and_keeps_the_groups() {
    let mut g = game(&["e4"]);
    let e4 = g.current();
    assert!(g.set_nag(e4, 1));
    let revision = g.revision();
    assert!(g.set_nag(e4, 1), "setting it again keeps it");
    assert_eq!((g.nags(e4), g.revision()), (&[1][..], revision));
    assert!(g.set_nag(e4, 2));
    assert!(g.set_nag(e4, 14));
    assert!(g.set_nag(e4, 36));
    assert!(g.set_nag(e4, 16));
    assert_eq!(g.nags(e4), &[2, 16, 36]);
    assert!(!g.set_nag(0, 1));
    assert!(!g.set_nag(e4, 0));
    assert!(!g.set_nag(e4, 256));
    assert!(!g.set_nag(99, 1));
    for code in 140..160 {
        g.set_nag(e4, code);
    }
    assert_eq!(g.nags(e4).len(), crate::game::MAX_NAGS);
    assert!(!g.set_nag(e4, 200), "full");
}

#[test]
fn sources_stay_with_their_moves_and_out_of_the_pgn() {
    let mut g = game(&["e4", "e5"]);
    let e4 = at(&g, &[0]);
    let d5 = play_at(&mut g, e4, &["d5"]);
    let src = NodeSource { page: 7, rect_user: [10.0, 20.0, 30.0, 40.0] };
    assert!(g.set_source(d5, Some(src)));
    assert!(!g.set_source(0, Some(src)));
    assert!(!g.set_source(99, Some(src)));
    assert!(!g.set_source(d5, Some(NodeSource { page: 1, rect_user: [f32::NAN, 0.0, 0.0, 0.0] })));
    g.promote_one(d5).unwrap();
    assert_eq!(g.source(at(&g, &[0, 0])), Some(src));
    assert_eq!(g.source(at(&g, &[0, 1])), None);
    assert!(!g.pgn().contains("40"), "{}", g.pgn());
    let copy = g.clone();
    assert_eq!(copy.source(at(&copy, &[0, 0])), Some(src));
    assert!(g.set_source(at(&g, &[0, 0]), None));
    assert_eq!(g.source(at(&g, &[0, 0])), None);
}

#[test]
fn bump_revision_passes_both() {
    let mut g = game(&["e4"]);
    let r = g.revision();
    g.bump_revision(r + 10);
    assert_eq!(g.revision(), r + 11);
    g.bump_revision(0);
    assert_eq!(g.revision(), r + 12);
    g.bump_revision(u64::MAX);
    assert_eq!(g.revision(), u64::MAX);
}

#[test]
fn a_variation_changes_places_with_its_neighbour_and_never_with_the_line() {
    let (mut g, _) = Game::from_pgn("1. e4 e5 (1... c5 {Sicilian}) (1... e6 $1) (1... d5) *").unwrap();
    let before = dump(&g);
    let e6 = at(&g, &[0, 2]);
    assert_eq!(g.move_variation(e6, Shift::Up), Ok(()));
    assert_eq!(g.movetext(), "1. e4 e5 (1... e6 $1) (1... c5 {Sicilian}) (1... d5)");
    let e6 = at(&g, &[0, 1]);
    assert_eq!(g.san(e6), "e6");
    assert_eq!(g.move_variation(e6, Shift::Up), Err(StructureError::NoSibling), "the line is not a variation to pass");
    assert_eq!(g.move_variation(e6, Shift::Down), Ok(()));
    assert_eq!(dump(&g), before, "down undid up");
    let d5 = at(&g, &[0, 3]);
    assert_eq!(g.move_variation(d5, Shift::Down), Err(StructureError::NoSibling));
    assert_eq!(g.move_variation(at(&g, &[0, 0]), Shift::Down), Err(StructureError::AlreadyFirst));
    assert_eq!(g.move_variation(0, Shift::Up), Err(StructureError::NoSuchMove));
    assert_eq!(g.move_variation(999, Shift::Up), Err(StructureError::NoSuchMove));
    assert_eq!(dump(&g), before, "refusals change nothing");
    assert_fixpoint(&g);
}

#[test]
fn a_null_move_passes_and_is_written_as_two_dashes() {
    let mut g = game(&["e4"]);
    assert_eq!(g.play_null(), Ok(()));
    let null = g.current();
    assert!(g.is_null(null));
    assert!(!g.is_null(0) && !g.is_null(at(&g, &[0])));
    assert_eq!(g.side_to_move(), crate::placement::Color::White, "Black passed");
    assert_eq!(g.last_move(), None, "a pass moves no piece");
    g.play_san("d4").unwrap();
    assert_eq!(g.movetext(), "1. e4 -- 2. d4");
    assert_fixpoint(&g);
    // A null move already there is followed; `Z0` is one too.
    g.go_to_node(at(&g, &[0]));
    g.play_san("Z0").unwrap();
    assert_eq!(g.current(), null);
    // A side in check cannot pass.
    let mut g = game(&["e4", "f5", "Qh5+"]);
    let before = dump(&g);
    assert_eq!(g.play_null(), Err(GameError::IllegalMove));
    assert_eq!(g.play_san("--"), Err(GameError::IllegalMove));
    assert_eq!(dump(&g), before);
    // Read from PGN, in a variation too: the threat shown by passing.
    let (g, stopped) = Game::from_pgn("1. e4 e5 2. Nf3 (2. -- d5) Nc6 *").unwrap();
    assert_eq!(stopped, None);
    assert_eq!(g.movetext(), "1. e4 e5 2. Nf3 (2. -- d5) 2... Nc6");
    assert!(g.is_null(at(&g, &[0, 0, 1])));
}

/// Checks what every game must keep: parents and replies agree, depths are right and capped,
/// paths name their moves, and the PGN is a fixpoint.
fn check(g: &Game) -> Result<(), proptest::test_runner::TestCaseError> {
    use proptest::prop_assert;
    use proptest::prop_assert_eq;
    let n = g.nodes.len();
    prop_assert!(n <= MAX_NODES);
    prop_assert!(g.current() < n);
    let mut seen = vec![0usize; n];
    for (id, node) in g.nodes.iter().enumerate() {
        for (i, &c) in node.children.iter().enumerate() {
            prop_assert!(c != 0 && c < n);
            prop_assert_eq!(g.nodes[c].parent, id);
            seen[c] += 1;
            let want = node.depth + usize::from(i > 0);
            prop_assert_eq!(g.nodes[c].depth, want);
            prop_assert!(g.nodes[c].depth <= MAX_DEPTH);
        }
    }
    prop_assert!(seen.iter().skip(1).all(|&s| s == 1), "every move is one reply: {seen:?}");
    prop_assert_eq!(g.nodes[0].depth, 0);
    prop_assert_eq!(g.comment_before(0), "");
    for id in 0..n {
        let path = g.path_of(id);
        prop_assert!(path.is_some());
        prop_assert_eq!(g.node_at(&path.unwrap_or_default()), Some(id));
    }
    let pgn = g.pgn();
    let (back, stopped) = Game::from_pgn(&pgn).map_err(|e| proptest::test_runner::TestCaseError::fail(e.to_string()))?;
    prop_assert_eq!(stopped, None);
    prop_assert_eq!(back.pgn(), pgn);
    // The drawings come back on their moves.
    for id in 0..n {
        let there = g.path_of(id).and_then(|path| back.node_at(&path));
        prop_assert!(there.is_some());
        prop_assert_eq!(back.drawings(there.unwrap_or_default()), g.drawings(id));
    }
    Ok(())
}

const TEXTS: [&str; 7] = ["", "a", "two words", "{braces}", "  spaced \n out ", "é (x) ñ", "1. e4 ; % [x]"];

/// Comments after a move may hold drawing commands too (a comment before one keeps them as text,
/// and before a first reply it is written with the comment after the move before: see
/// `drawing`).
const AFTER_TEXTS: [&str; 3] = ["[%csl Ga4] see [%clk 0:01]", "[%cal Ge2e4,Xa1a2] x", "[%csl Rh8]"];

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(96))]

    #[test]
    fn structure_operations_keep_the_tree_whole(ops in proptest::collection::vec((0u8..17, 0usize..64, 0usize..64), 0..60)) {
        let mut g = Game::standard();
        for (op, a, b) in ops {
            let node = a % g.nodes.len();
            match op {
                0..=3 => {
                    g.go_to_node(node);
                    let mut moves = g.legal_moves();
                    moves.dedup();
                    if !moves.is_empty() {
                        match g.play_san(&moves[b % moves.len()]) {
                            Ok(()) | Err(GameError::TooLong) => {}
                            Err(e) => proptest::prop_assert!(false, "{e:?}"),
                        }
                    }
                }
                4 => {
                    let index = g.reply_index(node).map(|(_, i)| i);
                    let before = dump(&g);
                    let parent = g.path_of(g.nodes[node].parent).unwrap_or_default();
                    let done = g.promote_one(node);
                    match (index, done) {
                        (None, Err(StructureError::NoSuchMove)) | (Some(0), Err(StructureError::AlreadyFirst)) => {}
                        (Some(i), Ok(()) | Err(StructureError::TooDeep)) if i > 0 => {}
                        other => proptest::prop_assert!(false, "{other:?}"),
                    }
                    if done.is_err() {
                        proptest::prop_assert_eq!(dump(&g), before.clone());
                    }
                    if index == Some(1) && done.is_ok() {
                        // The inverse: demoting it again restores everything.
                        let mut first = parent.indices().to_vec();
                        first.push(0);
                        let h = g.node_at(&NodePath::new(first));
                        proptest::prop_assert!(h.is_some());
                        proptest::prop_assert_eq!(g.demote(h.unwrap_or_default()), Ok(()));
                        proptest::prop_assert_eq!(dump(&g), before);
                    }
                }
                5 => { let _ = g.demote(node); }
                6 => {
                    let shown = g.current();
                    let inside = g.ancestry(shown).is_some_and(|up| up.contains(&node));
                    let parent = g.path_of(g.nodes[node].parent);
                    if g.delete_variation(node).is_ok() && inside {
                        proptest::prop_assert_eq!(g.path_of(g.current()), parent);
                    }
                }
                7 => g.promote(node),
                8 => g.set_comment(node, TEXTS.iter().chain(&AFTER_TEXTS).nth(b % (TEXTS.len() + AFTER_TEXTS.len())).copied().unwrap_or_default()),
                9 => g.set_comment_before(node, TEXTS[b % TEXTS.len()]),
                10 => { g.set_nag(node, [1u16, 2, 3, 6, 10, 14, 18, 22, 36, 146, 255][b % 11]); }
                11 => g.toggle_nag(node, [1u16, 2, 14, 36][b % 4]),
                12 => { g.set_source(node, Some(NodeSource { page: b as u32, rect_user: [0.0, 1.0, 2.0, b as f32] })); }
                14 => {
                    // A variation moved up or down, and back: the same tree; refused, unchanged.
                    let before = dump(&g);
                    let index = g.reply_index(node).map(|(_, i)| i);
                    let parent = g.path_of(g.nodes[node].parent).unwrap_or_default();
                    let replies = g.nodes[g.nodes[node].parent].children.len();
                    let (there, back) = if b % 2 == 0 { (Shift::Up, Shift::Down) } else { (Shift::Down, Shift::Up) };
                    match (index, g.move_variation(node, there)) {
                        (None, Err(StructureError::NoSuchMove)) | (Some(0), Err(StructureError::AlreadyFirst)) => {}
                        (Some(1), Err(StructureError::NoSibling)) if there == Shift::Up => {}
                        (Some(i), Err(StructureError::NoSibling)) if there == Shift::Down && i + 1 == replies => {}
                        (Some(i), Ok(())) if i > 0 => {
                            let mut moved = parent.indices().to_vec();
                            moved.push(if there == Shift::Up { i as u16 - 1 } else { i as u16 + 1 });
                            let h = g.node_at(&NodePath::new(moved));
                            proptest::prop_assert!(h.is_some());
                            check(&g)?;
                            proptest::prop_assert_eq!(g.move_variation(h.unwrap_or_default(), back), Ok(()));
                            proptest::prop_assert_eq!(dump(&g), before.clone());
                        }
                        other => proptest::prop_assert!(false, "{other:?}"),
                    }
                    proptest::prop_assert_eq!(dump(&g), before);
                }
                15 => {
                    g.go_to_node(node);
                    let in_check = g.in_check();
                    match g.play_null() {
                        Ok(()) => proptest::prop_assert!(!in_check && g.is_null(g.current())),
                        Err(GameError::IllegalMove) => proptest::prop_assert!(in_check),
                        Err(GameError::TooLong) => {}
                        Err(e) => proptest::prop_assert!(false, "{e:?}"),
                    }
                }
                13 => {
                    let (from, to) = (Square::from_index(b), Square::from_index(if a % 3 == 0 { b } else { (a + b) % 64 }));
                    let color = [DrawColor::Green, DrawColor::Red, DrawColor::Yellow, DrawColor::Blue][a % 4];
                    if let (Some(from), Some(to)) = (from, to) {
                        g.toggle_drawing(node, Drawing { from, to, color });
                    }
                }
                16 => { g.toggle_diagram(node); }
                _ => g.go_to_node(node),
            }
            check(&g)?;
        }
    }
}
