//! The compact index (plan A9-I3b): what it keeps answers as the games it no longer keeps did.

use std::sync::Arc;

use pdfcraft_chess::game::Game as PgnGame;
use proptest::prelude::*;

use super::*;
use crate::doc::reference::{RefNode, link, synthetic};

/// The games of `text`, read as the index reads them.
fn games_of(text: &str) -> Vec<Option<PgnGame>> {
    split_pgn(text, MAX_REFERENCE_GAMES).into_iter().map(|c| PgnGame::from_pgn_report(c).ok().map(|(g, _)| g)).collect()
}

/// For every move of every game of `text`: the index plays it again to the very position the game
/// kept (castling rights, en passant, both clocks), and writes its SAN as the game did; nothing
/// past a game's last node. How many moves were looked at.
fn answers_as_the_games(text: &str) -> usize {
    let index = ReferenceIndex::from_pgn(text).unwrap();
    assert!(!index.truncated());
    let mut moves = 0;
    for (k, game) in games_of(text).iter().enumerate() {
        let Some(game) = game else {
            assert!(index.game(k).is_none() && index.fen(RefNode { game: k, node: 0 }).is_none());
            continue;
        };
        let mut node = 0;
        while let Some(fen) = game.fen_at(node) {
            let at = RefNode { game: k, node };
            assert_eq!(index.fen(at).map(|f| f.to_string()), Some(fen.to_string()), "game {k}, node {node}");
            let san = game.san(node);
            assert_eq!(index.san(at), (node > 0).then_some(san), "game {k}, node {node}");
            moves += usize::from(node > 0);
            node += 1;
        }
        assert_eq!(index.fen(RefNode { game: k, node }), None);
        assert_eq!(index.game(k).map(|g| g.pgn()), Some(game.pgn()), "the game read again is the game");
    }
    assert_eq!(index.moves(), moves);
    moves
}

/// Every kind of step the index writes in two bytes: both castlings, en passant, a promotion to
/// each piece, a null move, a game from a FEN (its clocks and en passant square), variations.
#[test]
fn every_move_is_played_again_to_the_position_the_game_kept() {
    let pgn = "[Event \"Castles\"]\n\n1.e4 e5 2.Nf3 Nc6 3.Bc4 Bc5 4.O-O d6 5.d3 Bg4 6.Nc3 Qd7 7.Be3 O-O-O (7...-- 8.Bxc5 dxc5) 8.a3 *\n\n\
               [Event \"Pawns\"]\n\n1.e4 a6 2.e5 d5 3.exd6 Nf6 4.dxc7 Nbd7 5.cxd8=Q+ (5.cxd8=R+) (5.cxd8=B) (5.cxd8=N Kxd8) 5...Kxd8 *\n\n\
               [Event \"From a FEN\"]\n[SetUp \"1\"]\n[FEN \"4k3/8/8/3pP3/8/8/8/4K2R w K d6 3 40\"]\n\n40.exd6 Kd7 41.O-O Kxd6 (41...Ke6) *\n";
    let index = ReferenceIndex::from_pgn(pgn).unwrap();
    let null = games_of(pgn).first().cloned().flatten().unwrap();
    assert!((0..40).any(|n| null.is_null(n)), "the null move is read as one");
    assert_eq!(answers_as_the_games(pgn), index.moves());
    assert_eq!(index.games().len(), 3);
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 24, ..ProptestConfig::default() })]

    /// Random book PGNs: every answer of the index is the game's.
    #[test]
    fn a_random_book_pgn_is_answered_as_its_games(games in 1usize..4, seed in any::<u64>()) {
        let text = synthetic::book_pgn(games, seed);
        prop_assert!(answers_as_the_games(&text) > 0);
    }
}

/// A PGN past the bytes its index may take (plan A9-I3b): the moves stop there, said by
/// [`ReferenceIndex::cut`] and by the link's warnings; every game is still listed, by its tags'
/// name, and read again whole for the annotated PGN; what went in is answered as before.
#[test]
fn a_pgn_past_the_memory_cap_is_cut_and_said() {
    let text = synthetic::book_pgn(12, 7);
    let whole = ReferenceIndex::from_pgn(&text).unwrap();
    assert_eq!(whole.cut(), None);
    let budget = whole.estimate() / 3;
    let cut = ReferenceIndex::from_pgn_within(&text, budget).unwrap();
    assert_eq!(cut.cut(), Some(Cut::Memory));
    assert!(cut.truncated());
    assert!(cut.moves() > 0 && cut.moves() < whole.moves() / 2, "{} of {}", cut.moves(), whole.moves());
    // The moves and positions stay within it (the games past it are still listed, by name).
    let tables = cut.nodes.len() * NODE_BYTES + cut.keys.len() * POSITION_BYTES;
    assert!(tables <= budget && tables > budget / 2, "{tables} for {budget}");
    assert_eq!(cut.games().len(), 12, "every game listed");
    for (a, b) in cut.games().iter().zip(whole.games()) {
        assert_eq!(a.name, b.name, "the name of a game left out comes from its tags");
    }
    let last = cut.games().last().unwrap();
    assert_eq!((last.moves, last.error.as_deref()), (0, None));
    assert_eq!(cut.game(11).map(|g| g.pgn()), whole.game(11).map(|g| g.pgn()), "read again whole");
    // What went in answers as the whole index does.
    for g in 0..12 {
        for node in 0..300 {
            let at = RefNode { game: g, node };
            if let Some(fen) = cut.fen(at) {
                assert_eq!(Some(fen), whole.fen(at));
                assert_eq!(cut.san(at), whole.san(at));
            }
        }
    }
    let mut doc = crate::doc::reference::tests::doc(Vec::new());
    let warned = link(&mut doc, &cut).warnings;
    assert!(warned.iter().any(|w| w.contains("longer than its index holds") && w.contains(&cut.moves().to_string())), "{warned:?}");
}

/// The default cap holds a book's PGN of about 250,000 moves (plan A9-I3), each with a position of
/// its own, and the estimate the cap is judged by is the tables the index holds, give or take the
/// map's growth by doubling.
#[test]
fn the_default_cap_holds_about_250_000_moves() {
    let text = synthetic::book_pgn(40, 3);
    let index = ReferenceIndex::from_pgn(&text).unwrap();
    let per_move = index.estimate() / index.moves();
    assert!((60..=120).contains(&per_move), "{per_move} bytes a move");
    let held = MAX_REFERENCE_MEMORY / per_move;
    assert!((200_000..300_000).contains(&held), "{held} moves");
    let tables = index.memory() - index.texts.iter().map(|t| t.text.len()).sum::<usize>();
    assert!(index.estimate() * 2 > tables && index.estimate() < tables * 2, "estimate {} against {tables}", index.estimate());
}

/// One digest of the pieces for both sides to move; the side is its lowest bit.
#[test]
fn a_position_key_is_its_pieces_and_its_side() {
    let start = start_key();
    let white = PositionKey::of(start);
    let black = PositionKey::of(BookKey { side: Color::Black, ..start });
    assert_ne!(white, black);
    assert!(white.same_pieces(black));
    assert_eq!((white.side(), black.side()), (Color::White, Color::Black));
    let other = Fen::parse("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1").unwrap().book_key();
    assert!(!PositionKey::of(other).same_pieces(white));
}

/// Plan A9-I3b: the same text is indexed once, whoever asks and from whichever thread, and the
/// memo keeps at most [`MEMO_INDEXES`], the least recently asked for going first.
#[test]
fn the_memo_indexes_a_text_once() {
    let memo = Arc::new(IndexMemo::default());
    let a = synthetic::book_pgn(2, 1);
    let first = memo.index(&a).unwrap();
    let again = memo.index(&a).unwrap();
    assert!(Arc::ptr_eq(&first, &again));
    assert_eq!(memo.made(), 1);
    // Made on a worker, found by the caller.
    let b = synthetic::book_pgn(2, 2);
    let worker = {
        let (memo, b) = (memo.clone(), b.clone());
        std::thread::spawn(move || memo.index(&b).unwrap())
    };
    let made_there = worker.join().unwrap();
    assert!(Arc::ptr_eq(&made_there, &memo.index(&b).unwrap()));
    assert_eq!(memo.made(), 2);
    // A PGN with no moves is said each time, and kept by no one.
    assert_eq!(memo.index("*").unwrap_err(), ReferenceError::NoMoves);
    for seed in 3..3 + MEMO_INDEXES as u64 {
        memo.index(&synthetic::book_pgn(1, seed)).unwrap();
    }
    assert_eq!(memo.made(), 2 + MEMO_INDEXES);
    memo.index(&a).unwrap();
    assert_eq!(memo.made(), 3 + MEMO_INDEXES, "the oldest went first");
}

/// Plan A9-I3b (R8-06: 9.4 s and 1.18 GB for a PGN at the caps, on the interface thread): a book's
/// PGN well past the cap — about a million moves made up, as R8's — is indexed to the cap, a
/// quarter of a million moves, in tens of megabytes. `cargo test --release -p
/// pdfcraft-chessvision --lib perf_book_index -- --ignored --nocapture`.
#[test]
#[ignore = "a timing, in release"]
fn perf_book_index_at_the_cap() {
    let text = synthetic::book_pgn(1200, 0x5_eeda_913b);
    let t = std::time::Instant::now();
    let index = ReferenceIndex::from_pgn(&text).unwrap();
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    println!(
        "PERF perf_book_index_at_the_cap {ms:.0}\n  {} moves of {} games, {} positions, {} MB held",
        index.moves(),
        index.games().len(),
        index.positions(),
        index.memory() >> 20
    );
    assert_eq!(index.cut(), Some(Cut::Memory));
    assert!((200_000..300_000).contains(&index.moves()), "{}", index.moves());
    assert!(index.memory() < 64 << 20, "{} bytes", index.memory());
}
