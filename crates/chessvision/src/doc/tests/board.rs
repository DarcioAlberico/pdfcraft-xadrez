//! The document's games on the chess board (PGN toolbar spec T8): `games::to_game` and
//! `ChessDoc::board_game` keep every move, its glyphs and comments, the variations in their
//! order, and where each move was printed.

use pdfcraft_chess::game::{Game as ChessGame, NodeSource, Token};

use super::*;
use crate::doc::games::{BoardGame, to_game};

/// A score's movetext as `pgn::write_game` writes it: the tags, the line breaks and the result
/// left out.
fn movetext_of(score: &GameScore) -> String {
    let pgn = pgn::write_game(score);
    let moves = pgn.split_once("\n\n").map_or("", |(_, m)| m);
    let mut words: Vec<&str> = moves.split_whitespace().collect();
    words.pop(); // the result
    words.join(" ")
}

/// Every move of `game` in PGN order, with the move it is.
fn board_moves(game: &ChessGame) -> Vec<usize> {
    game.tokens().into_iter().filter_map(|t| if let Token::Move { node, .. } = t { Some(node) } else { None }).collect()
}

/// The showcase's games on the board: the movetext the document's PGN has, the tags, the move
/// of every node, and every move keeping its place on the page.
#[test]
fn the_documents_games_go_onto_the_board_with_their_places() {
    let (text, diagrams, hints) = showcase();
    let doc = build_page(&input(3, &text, &diagrams, &hints, &[]));
    assert_eq!(doc.games.len(), 2);
    for (i, g) in doc.games.iter().enumerate() {
        let BoardGame { game, paths, dropped } = doc.board_game(i).expect("a legal start");
        assert_eq!(dropped, 0);
        assert_eq!(game.movetext(), movetext_of(&g.score), "game {i}");
        assert_eq!(game.current(), 0, "shown at its start");
        assert_eq!(paths.len(), g.score.nodes.len(), "every node is on the board");
        for (n, path) in &paths {
            let mv = game.node_at(path).unwrap();
            assert_eq!(game.fen_at(mv).as_deref(), Some(g.score.nodes[*n].fen.as_str()), "node {n} is its own move");
        }
    }
    let solution = doc.board_game(0).unwrap().game;
    assert_eq!(solution.start_fen(), SCHOLAR);
    assert_eq!(solution.tag("Result"), Some("1-0"));
    assert_eq!(solution.tag("PdfDiagram"), Some("1"));
    assert_eq!(solution.tag("Event"), None, "the roster's placeholders are no values");
    // Each move printed on the page keeps where: its page and its box.
    for g in 0..2 {
        let board = doc.board_game(g).unwrap();
        let mut placed = 0;
        for m in doc.pages[0].blocks.iter().flat_map(|b| b.inlines()).filter_map(|i| if let Inline::Move(m) = i { Some(m) } else { None }) {
            let Some(r) = m.game.filter(|r| r.game == g) else { continue };
            let rect = m.source_rect.unwrap();
            let mv = board.node(r.node).unwrap();
            assert_eq!(board.game.source(mv), Some(NodeSource { page: 3, rect_user: [rect.x0, rect.y0, rect.x1, rect.y1] }));
            placed += 1;
        }
        assert_eq!(placed, board_moves(&board.game).len(), "game {g}: every move has a place");
    }
    // `to_game` alone carries no places.
    let plain = to_game(&doc.games[0].score).unwrap();
    assert!(board_moves(&plain.game).iter().all(|&n| plain.game.source(n).is_none()));
    assert!(doc.board_game(2).is_none(), "no such game");
}

/// A comment before a variation's first move, and one before a move that continues its line
/// (which PGN can only write with the comment of the move before), stay on their own moves.
#[test]
fn comments_before_stay_on_their_moves() {
    let scores = pgn::parse("1. e4 {King's pawn} e5 ({Instead} 1... c5 $5 2. Nf3) 2. Nf3 *").unwrap();
    let mut score = scores[0].clone();
    let nf3 = score.mainline()[2];
    score.nodes[nf3].comment_before = "Then".into();
    let board = to_game(&score).unwrap();
    let g = &board.game;
    let c5 = board.node(score.nodes.iter().position(|n| n.san == "c5").unwrap()).unwrap();
    assert_eq!((g.comment_before(c5), g.nags(c5)), ("Instead", &[5u16][..]));
    let e4 = board.node(score.mainline()[0]).unwrap();
    assert_eq!(g.comment(e4), "King's pawn");
    let nf3 = board.node(nf3).unwrap();
    assert_eq!(g.comment_before(nf3), "Then", "kept on the move, not merged into the comment before it");
    assert_eq!(g.comment(board.node(score.mainline()[1]).unwrap()), "", "the move before has none of it");
    // PGN writes it after the move before, where the reader takes it for that move's comment.
    assert_eq!(g.movetext(), "1. e4 {King's pawn} 1... e5 {Then} ({Instead} 1... c5 $5 2. Nf3) 2. Nf3");
    // The main line is the main line, its variation second.
    assert_eq!(g.main_line().iter().skip(1).map(|&n| g.san(n)).collect::<Vec<_>>(), ["e4", "e5", "Nf3"]);
}

/// A move the board refuses ends its line: it and the moves after it are counted, the rest of
/// the game (the variations before it) is kept.
#[test]
fn a_move_the_board_refuses_ends_its_line_and_is_counted() {
    let mut score = pgn::parse("1. e4 e5 (1... c5 2. Nf3 d6) 2. Nf3 Nc6 3. Bb5 *").unwrap().remove(0);
    let nf3 = score.mainline()[2];
    score.nodes[nf3].san = "Qh8".into();
    let board = to_game(&score).unwrap();
    assert_eq!(board.dropped, 3, "2.Nf3 (refused), Nc6 and 3.Bb5");
    assert_eq!(board.game.movetext(), "1. e4 e5 (1... c5 2. Nf3 d6)");
    assert_eq!(board.paths.len(), score.nodes.len() - 3);
    assert!(board.node(nf3).is_none());
}

/// A tree read from JSON that loops, points twice at one node or out of range, or a start that
/// is not a position: nothing panics, every move is played at most once.
#[test]
fn tampered_trees_are_bounded() {
    let mut score = pgn::parse("1. e4 e5 2. Nf3 Nc6 *").unwrap().remove(0);
    let last = *score.mainline().last().unwrap();
    score.nodes[last].children = vec![0, last, 99];
    score.roots.push(0);
    score.roots.push(usize::MAX);
    let board = to_game(&score).unwrap();
    assert_eq!(board.game.movetext(), "1. e4 e5 2. Nf3 Nc6");
    assert_eq!(board.dropped, 0);
    // Two replies that are the same move: the second is left out, not merged.
    let mut twins = pgn::parse("1. e4 e5 (1... c5) *").unwrap().remove(0);
    let c5 = twins.nodes.iter().position(|n| n.san == "c5").unwrap();
    twins.nodes[c5].san = "e5".into();
    let board = to_game(&twins).unwrap();
    assert_eq!((board.game.movetext().as_str(), board.dropped), ("1. e4 e5", 1));
    let mut bad = GameScore::new(Some("not a position".into()));
    assert!(to_game(&bad).is_none());
    bad.start_fen = Some("8/8/8/8/8/8/8/8 w - - 0 1".into());
    assert!(to_game(&bad).is_none(), "no kings: not legal");
}
