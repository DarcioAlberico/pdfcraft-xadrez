use pdfcraft_chess::game::Game as PgnGame;
use proptest::prelude::*;

use super::*;
use crate::doc::{ChessPage, DiagramBlock, DiagramReading, DiagramSource, FORMAT_VERSION, MoveNumber, TextLayer};

/// The FEN after `moves` from the initial position.
pub(super) fn fen_after(moves: &[&str]) -> String {
    let mut game = PgnGame::standard();
    for m in moves {
        game.play_san(m).unwrap();
    }
    game.fen()
}

pub(super) fn mv(number: Option<(u16, bool)>, san: &str, status: MoveStatus, fen: Option<String>) -> Inline {
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
        reference: None,
    })
}

/// The moves after `before`, each as the reader plays it.
pub(super) fn played(before: &[&str], moves: &[&str]) -> Vec<Inline> {
    let mut line: Vec<&str> = before.to_vec();
    moves
        .iter()
        .map(|m| {
            line.push(m);
            mv(None, m, MoveStatus::Valid, Some(fen_after(&line)))
        })
        .collect()
}

pub(super) fn para(inlines: Vec<Inline>) -> Block {
    Block::Paragraph { inlines, source_rect: None }
}

/// A diagram read with White to move assumed, as the book's are.
pub(super) fn diagram(number: usize, fen: &str) -> Block {
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
        reference: None,
        review: Vec::new(),
    })
}

pub(super) fn doc(pages: Vec<Vec<Block>>) -> ChessDoc {
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

/// The moves of `doc`'s paragraphs.
fn moves_of(doc: &ChessDoc) -> Vec<&MoveInline> {
    let paragraphs = doc.pages.iter().flat_map(|p| &p.blocks).filter(|b| matches!(b, Block::Paragraph { .. }));
    paragraphs.flat_map(|b| b.inlines()).filter_map(|i| if let Inline::Move(m) = i { Some(m) } else { None }).collect()
}

#[test]
fn link_keeps_the_links_on_the_moves_and_marks_what_the_pgn_recovered() {
    let index = ReferenceIndex::from_pgn("[Event \"Ch 1\"]\n\n1.e4 e6 2.d4 d5 3.Nc3 Bb4! 4.e5 c5 *\n").unwrap();
    let main = ["e4", "e6", "d4", "d5", "Nc3"];
    let after = vec![mv(Some((3, false)), "Nc3", OutOfSequence, None), mv(Some((3, true)), "Bb4", NotReached, None)];
    let mut d = doc(vec![vec![para(played(&[], &main)), diagram(0, &fen_after(&main)), para(after)]]);
    let linked = link(&mut d, &index);
    assert_eq!(linked.alignment.counts, LinkCounts { confirmed: 6, recovered: 1, conflict: 0, absent: 0 });
    assert!(linked.warnings.is_empty(), "{:?}", linked.warnings);
    let moves = moves_of(&d);
    assert_eq!(moves[0].reference, Some(MoveReference { kind: Confirmed, game: Some(0), node: Some(1) }));
    // The caption is the diagram's move; the move after it was recovered, its status kept.
    assert_eq!(moves[5].reference.map(|r| (r.kind, r.node)), Some((Confirmed, Some(5))));
    assert_eq!(moves[6].reference, Some(MoveReference { kind: Recovered, game: Some(0), node: Some(6) }));
    assert_eq!(moves[6].status, NotReached);
    assert_eq!(moves[6].review, [ReviewFlag::FromReference { san: "Bb4".into() }], "the PGN's move, its glyph aside");
    assert!(moves[..6].iter().all(|m| m.review.is_empty()));
    // Linking again is linking once; the links go to JSON and come back.
    let once = d.clone();
    link(&mut d, &index);
    assert_eq!(d, once);
    let json = d.to_json().unwrap();
    assert!(json.contains("\"reference\":{\"kind\":\"recovered\",\"game\":0,\"node\":6}"), "{json}");
    assert_eq!(ChessDoc::from_json(&json).unwrap(), d);
    // A document read before R2 has no links, and reads as one with none.
    let mut plain = doc(vec![vec![para(played(&[], &main))]]);
    let old = plain.to_json().unwrap();
    assert!(!old.contains("reference"), "{old}");
    link(&mut plain, &ReferenceIndex::from_pgn("1.c4 *").unwrap());
    assert!(moves_of(&plain).iter().all(|m| m.reference.is_some_and(|r| r.kind == Absent && r.game.is_none())));
}

#[test]
fn link_warns_when_the_pgn_is_not_the_books() {
    let index = ReferenceIndex::from_pgn("1.d4 d5 2.c4 e6 3.Nc3 *").unwrap();
    // On the judged pages, 2 of the 7 moves (29%) are in the PGN.
    let other = ["e4", "e5", "Nf3", "Nc6", "Bb5"];
    let mut pages = vec![vec![para(played(&[], &["d4", "d5"]))], vec![para(played(&[], &other))]];
    let mut d = doc(pages.clone());
    let warnings = link(&mut d, &index).warnings;
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("does not look like this book's") && warnings[0].contains("2 of the 7"), "{warnings:?}");
    // 3 of 8 (37%) is enough.
    pages[0] = vec![para(played(&[], &["d4", "d5", "c4"]))];
    assert!(link(&mut doc(pages.clone()), &index).warnings.is_empty());
    // Only the first pages read are judged: past them, the moves of another book do not count.
    let mut many: Vec<Vec<Block>> = (0..JUDGED_PAGES).map(|_| vec![para(played(&[], &["d4", "d5"]))]).collect();
    many.push(vec![para(played(&[], &other)); 20]);
    assert!(link(&mut doc(many), &index).warnings.is_empty());
    // A page with no moves warns about nothing.
    assert!(link(&mut doc(vec![vec![diagram(0, &fen_after(&[]))]]), &index).warnings.is_empty());
}

/// The diagrams of `doc`, in reading order.
fn diagrams_of(doc: &ChessDoc) -> Vec<&DiagramBlock> {
    doc.pages.iter().flat_map(|p| &p.blocks).filter_map(|b| if let Block::Diagram(d) = b { Some(d) } else { None }).collect()
}

/// A diagram of `fen`'s pieces with `square` (0 is a8, 63 is h1) set to `piece` (`'.'` empties it).
fn misread(number: usize, fen: &str, changes: &[(usize, char)]) -> Block {
    let mut squares = squares(fen.split(' ').next().unwrap()).unwrap();
    for &(k, piece) in changes {
        squares[k] = if piece == '.' { 0 } else { piece as u8 };
    }
    let ranks: Vec<String> = squares
        .chunks(8)
        .map(|rank| {
            let mut out = String::new();
            let mut empty = 0;
            for &s in rank {
                if s == 0 {
                    empty += 1;
                } else {
                    if empty > 0 {
                        out.push_str(&empty.to_string());
                        empty = 0;
                    }
                    out.push(s as char);
                }
            }
            if empty > 0 {
                out.push_str(&empty.to_string());
            }
            out
        })
        .collect();
    diagram(number, &ranks.join("/"))
}

#[test]
fn link_confirms_the_diagrams_the_pgn_shows_and_suggests_the_near_ones() {
    let index = ReferenceIndex::from_pgn("[Event \"Ch 1\"]\n\n1.e4 e6 2.d4 d5 3.Nc3 Bb4 4.e5 c5 *\n").unwrap();
    let main = ["e4", "e6", "d4", "d5", "Nc3"];
    let line = ["e4", "e6", "d4", "d5", "Nc3", "Bb4", "e5"];
    let mut shown = diagram(0, &fen_after(&main));
    if let Block::Diagram(d) = &mut shown {
        d.review = vec![ReviewFlag::SideAssumed];
        d.lichess_url = Some("https://lichess.org/analysis/x?color=black".into());
    }
    // h7 (square 15) misread as empty: one square from the position after 4.e5.
    let near = misread(0, &fen_after(&line), &[(15, '.')]);
    let far = misread(1, &fen_after(&line), &[(15, '.'), (14, '.'), (13, '.')]);
    let mut set_by_a_person = misread(2, &fen_after(&line), &[(15, '.')]);
    if let Block::Diagram(d) = &mut set_by_a_person {
        d.source = DiagramSource::User;
    }
    let mut white_to_move = diagram(3, &fen_after(&main));
    if let Block::Diagram(d) = &mut white_to_move {
        d.reading.as_mut().unwrap().side_assumed = false;
    }
    let two_pages_on = misread(0, &fen_after(&line), &[(15, '.')]);
    let pages = vec![
        vec![para(played(&[], &main)), shown],
        vec![para(played(&main, &["Bb4", "e5"])), near, far, set_by_a_person, white_to_move],
        vec![],
        vec![two_pages_on],
    ];
    let original = doc(pages);
    let mut d = original.clone();
    let linked = link(&mut d, &index);
    assert_eq!(linked.diagrams, DiagramCounts { confirmed: 1, suggested: 1 });
    let diagrams = diagrams_of(&d);
    // Confirmed: the PGN's side to move, castling and number; the side no longer assumed.
    let confirmed = diagrams[0];
    let pgn_fen = fen_after(&main);
    assert_eq!(confirmed.reading.as_ref().map(|r| (r.fen.as_str(), r.side_assumed)), Some((pgn_fen.as_str(), false)));
    assert!(confirmed.review.is_empty(), "{:?}", confirmed.review);
    assert!(
        confirmed.lichess_url.as_deref().is_some_and(|u| u.contains("%20b%20KQkq") && u.ends_with("?color=black")),
        "{:?}",
        confirmed.lichess_url
    );
    let r = confirmed.reference.as_ref().unwrap();
    assert_eq!((r.kind, r.game, r.node, r.fen.as_str()), (DiagramKind::Confirmed, 0, 5, pgn_fen.as_str()));
    let read = diagrams_of(&original)[0];
    assert_eq!(
        r.before.as_ref().map(|b| (&b.reading, &b.review)),
        Some((read.reading.as_ref().unwrap(), &read.review)),
        "the reading as read is kept"
    );
    // Suggested: the squares that differ, and the reading left alone.
    let suggested = diagrams[1];
    let r = suggested.reference.as_ref().unwrap();
    assert_eq!((r.kind, r.node, r.fen.as_str()), (DiagramKind::Suggested, 7, fen_after(&line).as_str()));
    assert_eq!(r.changes, [SquareChange { square: "h7".into(), from: None, to: Some('p') }]);
    let read = diagrams_of(&original)[1];
    assert_eq!((&suggested.reading, &suggested.review, r.before.is_none()), (&read.reading, &read.review, true));
    // Three squares off, a position a person set, a side read as the other one, two pages away:
    // nothing.
    for (k, d) in diagrams.iter().enumerate().skip(2) {
        assert!(d.reference.is_none(), "diagram {k}: {:?}", d.reference);
    }
    // Linking again is linking once, and the links go to JSON and come back.
    let once = d.clone();
    link(&mut d, &index);
    assert_eq!(d, once);
    let json = d.to_json().unwrap();
    assert!(json.contains("\"kind\":\"suggested\"") && json.contains("\"before\":{"), "{json}");
    assert_eq!(ChessDoc::from_json(&json).unwrap(), d);
    // Linked to a PGN that has none of it, every diagram is as read again.
    link(&mut d, &ReferenceIndex::from_pgn("1.c4 *").unwrap());
    assert_eq!(diagrams_of(&d), diagrams_of(&original));
}

#[test]
fn squares_read_only_eight_ranks_of_eight() {
    let start = squares("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR").unwrap();
    assert_eq!((start[0], start[15], start[16], start[63]), (b'r', b'p', 0, b'R'));
    for bad in ["8/8/8/8/8/8/8", "8/8/8/8/8/8/8/8/8", "9/8/8/8/8/8/8/8", "7/8/8/8/8/8/8/8", "ppppppppp/8/8/8/8/8/8/8", "x7/8/8/8/8/8/8/8", ""] {
        assert_eq!(squares(bad), None, "{bad}");
    }
    assert_eq!((square_name(0), square_name(15), square_name(63)), ("a8".to_string(), "h7".to_string(), "h1".to_string()));
}

#[test]
fn a_diagram_read_as_the_pgn_has_it_still_loses_the_side_assumed() {
    let index = ReferenceIndex::from_pgn("1.e4 e5 *").unwrap();
    let mut start = diagram(0, &fen_after(&[]));
    if let Block::Diagram(d) = &mut start {
        d.reading.as_mut().unwrap().fen = fen_after(&[]);
    }
    let mut d = doc(vec![vec![start]]);
    assert_eq!(link(&mut d, &index).diagrams.confirmed, 1);
    let r = diagrams_of(&d)[0].reading.as_ref().unwrap();
    assert_eq!((r.fen.as_str(), r.side_assumed), (fen_after(&[]).as_str(), false));
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

pub(super) fn arb_block() -> impl Strategy<Value = Block> {
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
            let mut linked = d.clone();
            let l = link(&mut linked, &index);
            prop_assert!(moves_of(&linked).iter().all(|m| m.reference.is_some()));
            for (after, before) in diagrams_of(&linked).iter().zip(diagrams_of(&d)) {
                match after.reference.as_ref().map(|r| r.kind) {
                    Some(DiagramKind::Suggested) => {
                        let r = after.reference.as_ref().unwrap();
                        prop_assert!((1..=MAX_SUGGESTED_SQUARES).contains(&r.changes.len()));
                        prop_assert_eq!(&after.reading, &before.reading, "a suggestion is never applied");
                    }
                    Some(DiagramKind::Confirmed) => prop_assert!(after.reading.as_ref().is_some_and(|r| !r.side_assumed)),
                    None => prop_assert_eq!(*after, before),
                }
            }
            let once = linked.clone();
            link(&mut linked, &index);
            prop_assert_eq!(&linked, &once);
            let a = align(&d, &index);
            prop_assert_eq!(&l.alignment, &a);
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
