use pdfcraft_chess::game::Game as PgnGame;
use proptest::prelude::*;

use super::*;
use crate::doc::{ChessPage, DiagramBlock, DiagramReading, DiagramSource, FORMAT_VERSION, MoveNumber, TextLayer};

/// The FEN after `moves` from the initial position.
fn fen_after(moves: &[&str]) -> String {
    let mut game = PgnGame::standard();
    for m in moves {
        game.play_san(m).unwrap();
    }
    game.fen()
}

fn mv(number: Option<(u16, bool)>, san: &str, status: MoveStatus, fen: Option<String>) -> Inline {
    Inline::Move(MoveInline {
        number: number.map(|(number, black)| MoveNumber { number, black }),
        text: san.to_string(),
        san: san.to_string(),
        status,
        fen,
        game: None,
        source_rect: None,
        bold: false,
        review: Vec::new(),
    })
}

/// The moves after `before`, each as the reader plays it.
fn played(before: &[&str], moves: &[&str]) -> Vec<Inline> {
    let mut line: Vec<&str> = before.to_vec();
    moves
        .iter()
        .map(|m| {
            line.push(m);
            mv(None, m, MoveStatus::Valid, Some(fen_after(&line)))
        })
        .collect()
}

fn para(inlines: Vec<Inline>) -> Block {
    Block::Paragraph { inlines, source_rect: None }
}

/// A diagram read with White to move assumed, as the book's are.
fn diagram(number: usize, fen: &str) -> Block {
    let placement = fen.split(' ').next().unwrap();
    Block::Diagram(DiagramBlock {
        number,
        index: number,
        source_rect: None,
        source: DiagramSource::Font,
        dpi: 0.0,
        reading: Some(DiagramReading {
            fen: format!("{placement} w - - 0 1"),
            legal: true,
            accepted: true,
            min_confidence: 1.0,
            repaired: false,
            side_assumed: true,
        }),
        lichess_url: None,
        puzzle: None,
        review: Vec::new(),
    })
}

fn doc(pages: Vec<Vec<Block>>) -> ChessDoc {
    let pages = pages
        .into_iter()
        .enumerate()
        .map(|(i, blocks)| ChessPage {
            index: i as u32,
            lang: "en".into(),
            text_layer: TextLayer::Good,
            ocr: false,
            blocks,
            review: Vec::new(),
            trailing: None,
        })
        .collect();
    ChessDoc { format: FORMAT_VERSION, pages, games: Vec::new() }
}

fn kinds(a: &Alignment) -> Vec<LinkKind> {
    a.moves.iter().map(|l| l.kind).collect()
}

/// The FENs (pieces and side) of the moves' nodes.
fn keys(index: &ReferenceIndex, a: &Alignment) -> Vec<Option<String>> {
    a.moves.iter().map(|l| l.node.and_then(|n| index.fen(n)).and_then(|f| position_key(&f))).collect()
}

fn key(moves: &[&str]) -> Option<String> {
    position_key(&fen_after(moves))
}

use LinkKind::{Absent, Confirmed, Conflict, Recovered};
use MoveStatus::{NotReached, OutOfSequence, Unchecked};

#[test]
fn the_index_holds_every_line_and_names_its_games() {
    let pgn = "[Event \"Chapter 1\"]\n[White \"?\"]\n[Black \"Game 2\"]\n\n1.e4 e5 (1...c5 2.Nf3) 2.Nf3 *\n\n[Event \"Chapter 2\"]\n\n1.d4 Nf6 2.Qxh7 d5 *\n";
    let index = ReferenceIndex::from_pgn(pgn).unwrap();
    assert_eq!(index.games().len(), 2);
    assert_eq!(index.games()[0].name, "Chapter 1 · Game 2", "the ? White is not a name");
    assert_eq!(index.games()[0].moves, 5);
    assert_eq!(index.games()[1].moves, 2, "the line stops at the illegal move");
    assert_eq!(index.games()[1].stopped.as_deref(), Some("Qxh7"));
    assert_eq!(index.moves(), 7);
    // Both starts are the one initial position.
    assert_eq!(index.positions(), 8);
    assert!(!index.truncated());
}

#[test]
fn a_pgn_with_no_moves_or_too_long_is_refused() {
    assert_eq!(ReferenceIndex::from_pgn("").unwrap_err(), ReferenceError::NoMoves);
    assert_eq!(ReferenceIndex::from_pgn("[Event \"x\"]\n\n*\n").unwrap_err(), ReferenceError::NoMoves);
    let long = " ".repeat(MAX_REFERENCE_BYTES + 1);
    assert!(matches!(ReferenceIndex::from_pgn(&long), Err(ReferenceError::TooLarge { .. })));
    // A game that cannot be read is kept, with its error, beside one that can.
    let index = ReferenceIndex::from_pgn("[FEN \"8/8/8/8/8/8/8/8 w - - 0 1\"]\n\n1.e4 *\n\n[Event \"b\"]\n\n1.e4 *\n").unwrap();
    assert_eq!(index.games().len(), 2);
    assert!(index.games()[0].error.is_some() && index.games()[0].game.is_none());
}

#[test]
fn a_caption_read_as_a_move_is_confirmed_and_the_line_after_it_recovered() {
    // Page 13 of the book that asked for this: the reader took "Position after: 3.Nc3" as a move
    // that does not follow, and stopped the line there.
    let index = ReferenceIndex::from_pgn("[Event \"Ch 1\"]\n\n1.e4 e6 2.d4 d5 3.Nc3 Bb4 4.e5 c5 *\n").unwrap();
    let main = ["e4", "e6", "d4", "d5", "Nc3"];
    let mut after = vec![mv(Some((3, false)), "Nc3", OutOfSequence, None), mv(Some((3, true)), "Bb4", NotReached, None)];
    after.extend([mv(Some((4, false)), "e5", NotReached, None), mv(None, "c5", NotReached, None)]);
    let d = doc(vec![vec![para(played(&[], &main)), diagram(0, &fen_after(&main)), para(after)]]);
    let a = align(&d, &index);
    assert_eq!(kinds(&a), [Confirmed, Confirmed, Confirmed, Confirmed, Confirmed, Confirmed, Recovered, Recovered, Recovered]);
    let k = keys(&index, &a);
    assert_eq!(k[5], key(&main), "the caption is the move the diagram shows");
    assert_eq!(k[8], key(&["e4", "e6", "d4", "d5", "Nc3", "Bb4", "e5", "c5"]));
    assert_eq!(a.diagrams.len(), 1);
    assert_eq!(a.diagrams[0].node.and_then(|n| index.fen(n)).and_then(|f| position_key(&f)), key(&main));
    assert_eq!(a.counts, LinkCounts { confirmed: 6, recovered: 3, conflict: 0, absent: 0 });
    assert_eq!(a.pages, [PageGame { page: 0, game: 0 }]);
}

#[test]
fn a_line_the_reader_stopped_is_taken_up_where_the_pgn_goes_on() {
    // The reader lost the variation's closing bracket and could not go back to the main line.
    let index = ReferenceIndex::from_pgn("1.e4 e5 (1...c5 2.c3) 2.Nf3 Nc6 *").unwrap();
    let mut text = played(&[], &["e4", "e5"]);
    text.extend(played(&["e4"], &["c5", "c3"]));
    text.extend([mv(Some((2, false)), "Nf3", OutOfSequence, None), mv(None, "Nc6", NotReached, None)]);
    let a = align(&doc(vec![vec![para(text)]]), &index);
    assert_eq!(kinds(&a), [Confirmed, Confirmed, Confirmed, Confirmed, Recovered, Recovered]);
    assert_eq!(keys(&index, &a)[5], key(&["e4", "e5", "Nf3", "Nc6"]));
}

#[test]
fn a_transposition_keeps_each_page_in_its_own_game() {
    let pgn = "[Event \"A\"]\n\n1.e4 e5 2.Nf3 Nc6 3.Bc4 *\n\n[Event \"B\"]\n\n1.Nf3 Nc6 2.e4 e5 3.Bb5 *\n";
    let index = ReferenceIndex::from_pgn(pgn).unwrap();
    let b = ["Nf3", "Nc6", "e4", "e5"];
    let mut first = played(&[], &b);
    first.extend(played(&b, &["Bb5"]));
    let second = vec![diagram(0, &fen_after(&b)), para(vec![mv(Some((3, false)), "Bc4", Unchecked, None)])];
    let a = align(&doc(vec![vec![para(first)], second]), &index);
    assert_eq!(kinds(&a), [Confirmed, Confirmed, Confirmed, Confirmed, Confirmed, Recovered]);
    let games: Vec<usize> = a.moves.iter().map(|l| l.node.unwrap().game).collect();
    assert_eq!(games, [1, 1, 1, 1, 1, 0], "the transposed position stays in the game the page plays");
    assert_eq!(a.pages, [PageGame { page: 0, game: 1 }, PageGame { page: 1, game: 0 }]);
    // The diagram is the position 3.Bc4 is played from, in game A.
    assert_eq!(a.diagrams[0].node, Some(RefNode { game: 0, node: 4 }));
}

#[test]
fn a_move_the_reader_played_elsewhere_is_a_conflict() {
    // The book's 1...c5 is not in the PGN: the reader played 2.Nf3 after it, the PGN after 1...e5.
    let index = ReferenceIndex::from_pgn("1.e4 e5 2.Nf3 Nc6 *").unwrap();
    let mut text = played(&[], &["e4", "e5"]);
    text.extend(played(&["e4"], &["c5", "Nf3"]));
    let a = align(&doc(vec![vec![para(text)]]), &index);
    assert_eq!(kinds(&a), [Confirmed, Confirmed, Absent, Conflict]);
    assert_eq!(keys(&index, &a)[3], key(&["e4", "e5", "Nf3"]));
}

#[test]
fn the_printed_number_outranks_the_most_recent_position() {
    let index = ReferenceIndex::from_pgn("1.e4 e5 2.Nf3 Nc6 (2...Nf6 3.Nc3 Nc6) *").unwrap();
    let mut text = played(&[], &["e4", "e5", "Nf3", "Nf6", "Nc3"]);
    text.push(mv(Some((2, true)), "Nc6", Unchecked, None));
    let a = align(&doc(vec![vec![para(text)]]), &index);
    assert_eq!(keys(&index, &a)[5], key(&["e4", "e5", "Nf3", "Nc6"]), "2...Nc6, not 3...Nc6");
    // Unnumbered, the most recent position wins.
    let mut text = played(&[], &["e4", "e5", "Nf3", "Nf6", "Nc3"]);
    text.push(mv(None, "Nc6+", Unchecked, None));
    let a = align(&doc(vec![vec![para(text)]]), &index);
    assert_eq!(keys(&index, &a)[5], key(&["e4", "e5", "Nf3", "Nf6", "Nc3", "Nc6"]), "check marks aside");
}

#[test]
fn the_game_the_page_discusses_outranks_a_more_recent_position() {
    let index = ReferenceIndex::from_pgn("[Event \"A\"]\n\n1.e4 e5 2.Nf3 *\n\n[Event \"B\"]\n\n1.d4 d5 2.Nf3 *\n").unwrap();
    // Three positions only B has against two only A has: the page discusses B.
    let mut text = played(&[], &["d4", "d5"]);
    text.extend(played(&[], &["e4", "e5"]));
    text.push(mv(Some((2, false)), "Nf3", Unchecked, None));
    let a = align(&doc(vec![vec![diagram(0, &fen_after(&["d4", "d5"])), para(text)]]), &index);
    assert_eq!(a.pages, [PageGame { page: 0, game: 1 }]);
    assert_eq!(keys(&index, &a)[4], key(&["d4", "d5", "Nf3"]), "B's 2.Nf3, though A's position is the more recent");
}

#[test]
fn a_pgn_of_another_book_links_nothing() {
    let index = ReferenceIndex::from_pgn("1.d4 d5 2.c4 *").unwrap();
    let mut text = played(&[], &["e4", "e5", "Nf3"]);
    text.push(mv(Some((2, true)), "Nc6", Unchecked, None));
    let a = align(&doc(vec![vec![diagram(0, &fen_after(&["e4"])), para(text)]]), &index);
    assert_eq!(a.counts, LinkCounts { confirmed: 0, recovered: 0, conflict: 0, absent: 4 });
    assert!(a.pages.is_empty());
    assert_eq!(a.diagrams[0].node, None);
    assert_eq!(a.rounds, 1);
}

#[test]
fn headings_and_moves_without_a_san_are_not_linked() {
    let index = ReferenceIndex::from_pgn("1.e4 e5 *").unwrap();
    let heading = Block::Heading { level: 2, inlines: played(&[], &["e4"]), source_rect: None };
    let a =
        align(&doc(vec![vec![heading, para(vec![mv(Some((1, false)), "", Unchecked, None), mv(Some((1, false)), "0-0", Unchecked, None)])]]), &index);
    assert_eq!(kinds(&a), [Absent, Absent], "the heading's move is not among them");
}

fn arb_san() -> impl Strategy<Value = String> {
    prop_oneof![
        prop::sample::select(vec!["e4", "e5", "Nf3", "Nc6", "Bb5", "O-O", "0-0-0", "d4", "", "+", "Nxe5#"]).prop_map(String::from),
        "[a-hKQRBNOx0-9+#=-]{0,8}",
    ]
}

fn arb_fen() -> impl Strategy<Value = Option<String>> {
    prop_oneof![
        Just(None),
        prop::sample::select(vec![fen_after(&[]), fen_after(&["e4"]), fen_after(&["e4", "e5"]), fen_after(&["e4", "e5", "Nf3"])]).prop_map(Some),
        "[a-zA-Z0-9/ -]{0,30}".prop_map(Some),
    ]
}

fn arb_block() -> impl Strategy<Value = Block> {
    let status = prop::sample::select(vec![
        MoveStatus::Valid,
        MoveStatus::Illegal,
        MoveStatus::Ambiguous,
        OutOfSequence,
        NotReached,
        Unchecked,
        MoveStatus::Caption,
    ]);
    let number = prop::option::of((any::<u16>(), any::<bool>()));
    let inline = (number, arb_san(), status, arb_fen()).prop_map(|(n, san, st, fen)| mv(n, &san, st, fen));
    prop_oneof![prop::collection::vec(inline, 0..8).prop_map(para), (0usize..4, arb_fen()).prop_map(|(n, fen)| diagram(n, &fen.unwrap_or_default())),]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, ..ProptestConfig::default() })]

    #[test]
    fn align_never_panics_and_links_every_move(
        pages in prop::collection::vec(prop::collection::vec(arb_block(), 0..6), 0..4),
        pgn in prop_oneof![
            Just("1.e4 e5 (1...c5 2.Nf3) 2.Nf3 Nc6 3.Bb5 a6 *\n\n[Event \"b\"]\n\n1.Nf3 Nc6 2.e4 e5 0-0 *".to_string()),
            "[\\[\\]\" a-hKQRBNOx0-9.+#=(){}*-]{0,80}",
        ],
    ) {
        let d = doc(pages);
        let moves = d.pages.iter().flat_map(|p| &p.blocks).filter(|b| matches!(b, Block::Paragraph { .. })).flat_map(|b| b.inlines()).filter(|i| matches!(i, Inline::Move(_))).count();
        if let Ok(index) = ReferenceIndex::from_pgn(&pgn) {
            let a = align(&d, &index);
            prop_assert_eq!(a.moves.len(), moves);
            let c = a.counts;
            prop_assert_eq!(c.confirmed + c.recovered + c.conflict + c.absent, moves);
            prop_assert!(a.rounds >= 1 && a.rounds <= MAX_ROUNDS);
            for link in &a.moves {
                prop_assert_eq!(link.node.is_none(), link.kind == Absent);
                if let Some(n) = link.node {
                    prop_assert!(index.fen(n).is_some());
                }
            }
        }
    }
}
