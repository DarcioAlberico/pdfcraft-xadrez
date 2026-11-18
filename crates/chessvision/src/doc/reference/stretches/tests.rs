//! Plan A9-N16a: the book's PGN searched by material, stretch by stretch.

use super::*;

/// Plan A9-N16a: a rook ending with a pawn, its colours-swapped twin and a queen game.
pub(crate) const ENDINGS_PGN: &str = "[Event \"Rook ending\"]\n[SetUp \"1\"]\n[FEN \"6k1/8/8/3P4/8/8/r7/4K2R w - - 0 1\"]\n\n\
1.d6 Ra8 2.d7 Rd8 3.Rh7 (3.Rh4 Kf7) 3...Kf8 4.Rh8+ Ke7 5.Rxd8 Kxd8 *\n\n\
[Event \"Rook ending, Black's pawn\"]\n[SetUp \"1\"]\n[FEN \"4k2r/R7/8/8/3p4/8/8/6K1 b - - 0 1\"]\n\n\
1...d3 2.Ra1 d2 3.Rd1 Rh2 4.Kxh2 *\n\n\
[Event \"Queen\"]\n[SetUp \"1\"]\n[FEN \"6k1/5ppp/8/8/8/8/5PPP/3Q2K1 w - - 0 1\"]\n\n1.Qd8# 1-0\n";

/// A stretch is the positions in a row of one line: the ending's eight on its main line, the two
/// of its variation, and the twin's five with the colours swapped; the queen's game none.
#[test]
fn positions_in_a_row_are_one_stretch_and_a_variation_another() {
    use pdfcraft_chess::pattern::{Material, Query};
    let index = ReferenceIndex::from_pgn(ENDINGS_PGN).unwrap();
    let query = Query { material: Material::parse("KRP-KR").ok(), either_side: true, ..Query::default() };
    let found = index.stretches(&query, 100);
    let said: Vec<(usize, String, String, bool, usize)> =
        found.found.iter().map(|s| (s.game, s.first.to_string(), s.last.to_string(), s.main_line, s.positions)).collect();
    assert_eq!(
        said,
        [
            (0, "1.d6".to_owned(), "4...Ke7".to_owned(), true, 8),
            (0, "3.Rh4".to_owned(), "3...Kf7".to_owned(), false, 2),
            (1, "1...d3".to_owned(), "3...Rh2".to_owned(), true, 5),
        ]
    );
    assert_eq!((found.total, found.per_game.clone(), found.played, found.unreadable), (3, vec![(0, 2), (1, 1)], 3, 0), "each game played once");
    // Each stretch's node is its first move in its game (plan A9-N16b opens the game there).
    for s in &found.found {
        let game = index.game(s.game).unwrap();
        assert_eq!(game.san(s.node), s.first.san, "{s:?}");
    }
    // Without the colours swapped, the twin is not found; at most one is listed, all are counted.
    let white = index.stretches(&Query { either_side: false, ..query }, 1);
    assert_eq!((white.found.len(), white.total, white.per_game), (1, 2, vec![(0, 2)]));
}

/// Plan A9-N16a's budget: the book's PGN at the index's cap (about 250,000 moves, made up as R8's)
/// played again once, each game once, in at most 2 s in release. `cargo test --release -p
/// pdfcraft-chessvision --lib perf_book_pattern -- --ignored --nocapture`. Timed by the crate's
/// one rule ([`crate::timing`]): the best of three runs, within twice the card's 2 s.
#[test]
#[ignore = "a timing, in release"]
fn perf_book_pattern_at_the_cap() {
    use pdfcraft_chess::pattern::{PawnQuery, Query};
    let text = crate::doc::reference::synthetic::book_pgn(1200, 0x5_eeda_913b);
    let index = ReferenceIndex::from_pgn(&text).unwrap();
    let query = Query { pawns: PawnQuery { doubled: true, ..PawnQuery::default() }, ..Query::default() };
    let (ms, runs, found) = crate::timing::best_of_three(|| index.stretches(&query, 1000));
    println!("PERF perf_book_pattern_at_the_cap {ms:.0}\n  {} games played, {} moves, {} stretches", found.played, index.moves(), found.total);
    assert_eq!(found.played, index.games().iter().filter(|g| g.moves > 0).count(), "each game the index holds, once");
    assert!((200_000..300_000).contains(&index.moves()), "{}", index.moves());
    assert!(crate::timing::within(ms, 2000.0), "best of three {ms:.0} ms ({runs:.0?})");
}

/// Plan A9-N16a2: a run open when the walk's budget runs out is still counted, and the result says
/// it was capped.
#[test]
fn a_run_cut_by_the_budget_is_kept_and_said() {
    use pdfcraft_chess::pattern::{Material, Query};
    let index = ReferenceIndex::from_pgn(ENDINGS_PGN).unwrap();
    let game = index.game(0).unwrap();
    let query = Query { material: Material::parse("KRP-KR").ok(), either_side: true, ..Query::default() };
    let mut out = Stretches::default();
    game_stretches(&game, 0, &query, 100, &mut out, 3);
    assert_eq!(
        out.found.iter().map(|s| (s.first.to_string(), s.last.to_string(), s.positions)).collect::<Vec<_>>(),
        [("1.d6".to_owned(), "2.d7".to_owned(), 3)]
    );
    assert!(out.capped);
    assert!(!index.stretches(&query, 100).capped, "a whole game is no cap");
}

/// Plan A9-N16a2: the position a game is set up from is no stretch of its own: a stretch starts at
/// a move. A game set up in a rook ending whose first move takes the rook has none.
#[test]
fn a_games_start_position_is_not_a_stretch() {
    use pdfcraft_chess::pattern::{Material, Query};
    let pgn = "[Event \"Taken\"]\n[SetUp \"1\"]\n[FEN \"6k1/8/8/3P4/8/8/r7/R3K3 w - - 0 1\"]\n\n1.Rxa2 Kf7 *\n";
    let index = ReferenceIndex::from_pgn(pgn).unwrap();
    let query = Query { material: Material::parse("KRP-KR").ok(), ..Query::default() };
    let found = index.stretches(&query, 100);
    assert_eq!((found.total, found.played), (0, 1), "{found:?}");
}
