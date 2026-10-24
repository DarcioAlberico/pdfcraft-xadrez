use pdfcraft_chess::fen::parse_placement;
use pdfcraft_chess::nag;
use pdfcraft_chess::placement::{Color, Piece, PieceKind, Square};
use pdfcraft_chess::recognize::decode::Change;
use pdfcraft_chess::recognize::side::{SideSource, SideToMove};
use pdfcraft_chess::rules::check_position;
use pdfcraft_chess::san::{Lang, San};
use proptest::prelude::*;

mod board;
mod fragment;
mod html;
mod markdown;

use super::*;
use crate::board::Unreadable;
use crate::chess_text::{ChessLine, ChessRun, ChessSpan, LangGuess, LangTally};
use crate::coords::Rotation;

const PAGE_H: f32 = 800.0;

fn geometry() -> PageGeometry {
    PageGeometry::new(PageRect::new(0.0, 0.0, 600.0, PAGE_H), Rotation::R0).unwrap()
}

/// Runs from space-separated tokens: `12.` / `12...` prefixes are move numbers, readable SAN is a
/// move, a NAG glyph is a NAG, the rest is prose (joined, as `chess_text` joins it). A token
/// starting with `*` is set in bold.
fn runs(text: &str) -> Vec<(ChessRun, bool)> {
    let mut out: Vec<(ChessRun, bool)> = Vec::new();
    for tok in text.split(' ').filter(|t| !t.is_empty()) {
        let (tok, bold) = match tok.strip_prefix('*') {
            Some(t) => (t, true),
            None => (tok, false),
        };
        let digits = tok.chars().take_while(char::is_ascii_digit).count();
        let mut rest = tok;
        if digits > 0 && tok[digits..].starts_with('.') {
            let dots = tok[digits..].chars().take_while(|c| *c == '.').count();
            out.push((ChessRun::MoveNumber { number: tok[..digits].parse().unwrap(), black: dots >= 3 }, bold));
            rest = &tok[digits + dots..];
            if rest.is_empty() {
                continue;
            }
        }
        let starts_like_move = rest.starts_with(|c: char| ('a'..='h').contains(&c) || "KQRBNO".contains(c));
        if starts_like_move && let Ok(san) = San::read(rest, Lang::En) {
            out.push((ChessRun::Move { san, text: rest.to_owned() }, bold));
        } else if let Some(n) = nag::by_glyph(rest) {
            out.push((ChessRun::Nag(n.code), bold));
        } else if let Some((ChessRun::Text(prev), _)) = out.last_mut() {
            prev.push(' ');
            prev.push_str(rest);
        } else {
            out.push((ChessRun::Text(rest.to_owned()), bold));
        }
    }
    out
}

/// A line of runs whose top is at view `y`, from `x`, 7 pt a character, 12 pt high.
fn line_at(x: f32, y: f32, text: &str) -> ChessLine {
    let mut cx = x;
    let spans: Vec<ChessSpan> = runs(text)
        .into_iter()
        .map(|(run, bold)| {
            let chars = match &run {
                ChessRun::Text(s) => s.chars().count(),
                ChessRun::Move { text, .. } => text.chars().count(),
                ChessRun::MoveNumber { .. } => 3,
                _ => 1,
            } as f32;
            let rect = ViewRect::new(cx, y, cx + 7.0 * chars, y + 12.0);
            cx += 7.0 * chars + 4.0;
            ChessSpan { run, rect, bold }
        })
        .collect();
    let rect = ViewRect::new(x, y, cx, y + 12.0);
    ChessLine { spans, rect }
}

fn line(y: f32, text: &str) -> ChessLine {
    line_at(40.0, y, text)
}

fn page_text(lines: Vec<ChessLine>) -> ChessPageText {
    ChessPageText {
        lines,
        lang: LangGuess { lang: Lang::En, confidence: 1.0 },
        quality: TextQuality::Good,
        hidden: 0.0,
        unknown_chess_fonts: Vec::new(),
        lang_tally: LangTally::default(),
    }
}

/// A user-space box from a view-space top and height.
fn user_box(x0: f32, top: f32, x1: f32, bottom: f32) -> PageRect {
    PageRect::new(x0, PAGE_H - bottom, x1, PAGE_H - top)
}

fn reading(index: usize, top: f32, fen: &str, accepted: bool, assumed: bool) -> BoardReading {
    let placement = parse_placement(fen).unwrap();
    let color = if fen.split(' ').nth(1) == Some("b") { Color::Black } else { Color::White };
    BoardReading {
        index,
        bbox_user: user_box(40.0, top, 190.0, top + 150.0),
        source: ReadingSource::Model,
        dpi: 220.0,
        outcome: Ok(ReadPosition {
            placement,
            fen: fen.to_owned(),
            legality: check_position(&placement),
            side: SideToMove { color, source: if assumed { SideSource::Default } else { SideSource::Text }, conflicting: false },
            orientation: None,
            min_confidence: if accepted { 0.99 } else { 0.62 },
            uncertain: Vec::new(),
            repairs: Vec::new(),
            accepted,
        }),
    }
}

fn input<'a>(index: u32, text: &'a ChessPageText, diagrams: &'a [BoardReading], hints: &'a [BlockHint], reviews: &'a [TextReview]) -> PageInput<'a> {
    PageInput { index, geometry: geometry(), text, diagrams, hints, reviews, ocr: false }
}

fn kinds(page: &ChessPage) -> Vec<String> {
    page.blocks
        .iter()
        .map(|b| match b {
            Block::Heading { inlines, .. } => format!("H:{}", plain_text(inlines)),
            Block::Paragraph { inlines, .. } => format!("P:{}", plain_text(inlines)),
            Block::Diagram(d) => format!("D{}", d.number),
        })
        .collect()
}

fn moves(page: &ChessPage) -> Vec<(String, MoveStatus)> {
    page.blocks
        .iter()
        .flat_map(|b| b.inlines())
        .filter_map(|i| match i {
            Inline::Move(m) => Some((format!("{}{}", m.number.map(|n| n.text()).unwrap_or_default(), m.text), m.status)),
            _ => None,
        })
        .collect()
}

/// After 1.e4 e5 2.Bc4 Nc6: White to play 3.Qh5.
const SCHOLAR: &str = "r1bqkbnr/pppp1ppp/2n5/4p3/2B1P3/8/PPPP1PPP/RNBQK1NR w KQkq - 2 3";

/// The synthetic page of the blueprint's check: a heading, a paragraph, a diagram and its
/// solution with a NAG, a variation and a result, then a game from the initial position with an
/// illegal move.
fn showcase() -> (ChessPageText, Vec<BoardReading>, Vec<BlockHint>) {
    let text = page_text(vec![
        line(20.0, "Chapter 1"),
        // Prose before any diagram: its move has no position.
        line(40.0, "White to play and win. See 23.Nf5 at www.example.org."),
        // Diagram 0 stands at 60..210.
        line(230.0, "3.Qh5 ! Nf6 ?? ( 3...g6 4.Qf3 ) 4.Qxf7# 1-0"),
        line(260.0, "A classic trap."),
        line(300.0, "A game"),
        line(320.0, "1.e4 e5 2.Nf3 Nc6 3.Bb5 a6 4.Bd7 dxc6 5.O-O"),
    ]);
    let diagrams = vec![reading(0, 60.0, SCHOLAR, true, false)];
    let hints = vec![
        BlockHint { kind: HintKind::Heading { level: 1 }, rect: user_box(30.0, 15.0, 300.0, 35.0) },
        BlockHint { kind: HintKind::Heading { level: 2 }, rect: user_box(30.0, 295.0, 300.0, 315.0) },
    ];
    (text, diagrams, hints)
}

/// A chapter title that names a move in its words (Pinski: `The Italian Regretted: White Plays
/// 5 d3`) is a heading: the game before it ends there, its move is left unplayed, and the
/// introduction's moves after it replay from the start. A line of moves set large under a heading
/// hint is a main line, a paragraph, and so is a hint over a game's moves with a word or two.
#[test]
fn a_title_that_names_a_move_is_a_heading_and_a_main_line_is_not() {
    let text = page_text(vec![
        line(20.0, "1.e4 e5 2.Nf3 Nc6 3.Bc4 Bc5"),
        line(60.0, "The Italian Regretted: White Plays 5.d3"),
        line(100.0, "Black should equalise after 1.e4 e5 2.Nf3 Nc6"),
        line(140.0, "1.d4 d5 2.c4"),
        line(180.0, "Game 5 1.e4 e5"),
    ]);
    let hints: Vec<BlockHint> = [60.0, 140.0, 180.0]
        .iter()
        .map(|y| BlockHint { kind: HintKind::Heading { level: 1 }, rect: user_box(30.0, y - 5.0, 560.0, y + 15.0) })
        .collect();
    let doc = build_page(&input(0, &text, &[], &hints, &[]));
    let page = &doc.pages[0];
    assert_eq!(
        kinds(page),
        vec![
            "P:1.e4 e5 2.Nf3 Nc6 3.Bc4 Bc5",
            "H:The Italian Regretted: White Plays 5.d3",
            "P:Black should equalise after 1.e4 e5 2.Nf3 Nc6",
            "P:1.d4 d5 2.c4",
            "P:Game 5 1.e4 e5",
        ]
    );
    let statuses = moves(page);
    assert_eq!(statuses.get(6), Some(&("5.d3".to_owned(), MoveStatus::Unchecked)), "the title's move is not played");
    assert!(statuses.get(7..11).is_some_and(|after| after.iter().all(|(_, s)| *s == MoveStatus::Valid)), "{statuses:?}");
}

#[test]
fn the_showcase_page_becomes_the_expected_doc() {
    let (text, diagrams, hints) = showcase();
    let doc = build_page(&input(0, &text, &diagrams, &hints, &[]));
    let page = &doc.pages[0];
    assert_eq!(
        kinds(page),
        vec![
            "H:Chapter 1",
            "P:White to play and win. See 23.Nf5 at www.example.org.",
            "D0",
            "P:3.Qh5! Nf6?? (3...g6 4.Qf3) 4.Qxf7# 1-0",
            "P:A classic trap.",
            "H:A game",
            "P:1.e4 e5 2.Nf3 Nc6 3.Bb5 a6 4.Bd7 dxc6 5.O-O",
        ]
    );
    use MoveStatus::*;
    assert_eq!(
        moves(page),
        vec![
            ("23.Nf5".into(), Unchecked),
            ("3.Qh5".into(), Valid),
            ("Nf6".into(), Valid),
            ("3...g6".into(), Valid),
            ("4.Qf3".into(), Valid),
            ("4.Qxf7#".into(), Valid),
            ("1.e4".into(), Valid),
            ("e5".into(), Valid),
            ("2.Nf3".into(), Valid),
            ("Nc6".into(), Valid),
            ("3.Bb5".into(), Valid),
            ("a6".into(), Valid),
            ("4.Bd7".into(), Illegal),
            ("dxc6".into(), NotReached),
            ("5.O-O".into(), NotReached),
        ]
    );
    // The diagram: read, accepted, linked, nothing to review.
    let Block::Diagram(d) = &page.blocks[2] else { panic!() };
    assert_eq!(d.reading.as_ref().map(|r| (r.fen.as_str(), r.accepted, r.legal)), Some((SCHOLAR, true, true)));
    assert!(d.lichess_url.as_deref().is_some_and(|u| u.starts_with("https://lichess.org/analysis/")));
    assert!(d.review.is_empty(), "{:?}", d.review);
    assert_eq!(d.source_rect, UserRect::new(user_box(40.0, 60.0, 190.0, 210.0)));
    // Two games: the solution from the diagram, the game from the initial position.
    assert_eq!(doc.games.len(), 2);
    let solution = &doc.games[0];
    assert_eq!(solution.origin, GameOrigin::Diagram { page: 0, diagram: 0, accepted: true });
    assert!(solution.trusted && solution.flags.is_empty());
    assert_eq!(solution.score.result, GameResult::WhiteWins);
    let pgn = pgn::write_game(&solution.score);
    assert!(pgn.ends_with("\n\n3. Qh5 $1 Nf6 $4 (3... g6 4. Qf3) 4. Qxf7# 1-0\n"), "{pgn}");
    assert!(pgn.contains(&format!("[FEN \"{SCHOLAR}\"]")) && pgn.contains("[PdfDiagram \"1\"]"), "{pgn}");
    let game = &doc.games[1];
    assert_eq!(game.origin, GameOrigin::Initial);
    let pgn = pgn::write_game(&game.score);
    assert!(pgn.ends_with("\n\n1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 {4.Bd7 dxc6 5.O-O} *\n"), "{pgn}");
    assert!(!pgn.contains("[FEN"));
    let flags: Vec<&ReviewFlag> = game.flags.iter().map(|f| &f.flag).collect();
    assert_eq!(flags, vec![&ReviewFlag::IllegalMove { text: "4.Bd7".into(), near: vec![] }]);
    // Moves point at their nodes; the node is the move.
    for m in page.blocks.iter().flat_map(|b| b.inlines()).filter_map(|i| if let Inline::Move(m) = i { Some(m) } else { None }) {
        match m.game {
            Some(r) => {
                let node = &doc.games[r.game].score.nodes[r.node];
                assert_eq!((Some(&node.san), Some(&node.fen)), (Some(&m.san), m.fen.as_ref()));
            }
            None => assert_ne!(m.status, MoveStatus::Valid),
        }
    }
    // The web address became a link.
    assert!(page.blocks[1].inlines().iter().any(|i| matches!(i, Inline::Link { url, .. } if url == "https://www.example.org")));
}

#[test]
fn the_doc_and_its_games_survive_json_and_pgn() {
    let (text, diagrams, hints) = showcase();
    let doc = build_page(&input(0, &text, &diagrams, &hints, &[]));
    let json = doc.to_json().unwrap();
    assert!(json.contains("\"type\":\"diagram\"") && json.contains("\"status\":\"illegal\""));
    assert_eq!(ChessDoc::from_json(&json).unwrap(), doc);
    let scores: Vec<GameScore> = doc.games.iter().map(|g| g.score.clone()).collect();
    assert_eq!(pgn::parse(&doc.pgn()).unwrap(), scores);
    assert!(ChessDoc::from_json(&" ".repeat(MAX_JSON_BYTES + 1)).is_err());
}

#[test]
fn a_rejected_repaired_reading_seeds_an_untrusted_game() {
    let mut r = reading(0, 60.0, SCHOLAR, false, false);
    if let Ok(p) = &mut r.outcome {
        p.repairs = vec![Change { square: Square::parse("c6").unwrap(), from: None, to: Some(Piece::new(Color::Black, PieceKind::Knight)) }];
    }
    let text = page_text(vec![line(230.0, "3.Qh5 Nf6 4.Qxf7#")]);
    let doc = build_page(&input(4, &text, &[r], &[], &[]));
    let Block::Diagram(d) = &doc.pages[0].blocks[0] else { panic!() };
    assert_eq!(d.lichess_url, None, "a repaired reading gets no link");
    assert!(d.review.contains(&ReviewFlag::NotAccepted { min_confidence: 0.62 }));
    assert!(d.review.iter().any(|f| matches!(f, ReviewFlag::RepairedSquares { changes } if changes[0].square == "c6" && changes[0].to == Some('n'))));
    let game = &doc.games[0];
    assert!(!game.trusted);
    assert_eq!(game.origin, GameOrigin::Diagram { page: 4, diagram: 0, accepted: false });
    assert_eq!(game.flags[0].flag, ReviewFlag::UntrustedStart { page: 4, diagram: 0 });
    assert_eq!(game.score.mainline().len(), 3, "the moves are still checked against the reading");
}

#[test]
fn moves_after_an_unreadable_diagram_stay_unchecked() {
    let mut r = reading(0, 60.0, SCHOLAR, true, false);
    r.outcome = Err(Unreadable::NoModel);
    let text = page_text(vec![line(230.0, "1.Qh5 Nf6 2.Qxf7#"), line(300.0, "1.e4 e5")]);
    let doc = build_page(&input(0, &text, &[r], &[], &[]));
    assert!(doc.games.is_empty(), "not even `1.` starts a game from the initial position here");
    assert!(moves(&doc.pages[0]).iter().all(|(_, s)| *s == MoveStatus::Unchecked));
    let Block::Diagram(d) = &doc.pages[0].blocks[0] else { panic!() };
    assert_eq!(d.review, vec![ReviewFlag::Unreadable { reason: "no piece model is loaded".into() }, ReviewFlag::NoStartPosition]);
}

#[test]
fn a_position_after_caption_sets_the_side_and_number_and_is_not_played() {
    // Read with White to move, assumed, at move 1; the caption names the move that led to it.
    let fen = "r1bqkbnr/pppp1ppp/2n5/4p3/2B1P3/8/PPPP1PPP/RNBQK1NR b KQkq - 0 1";
    let text = page_text(vec![line(230.0, "Position after: 2...Nc6"), line(250.0, "3.Qh5 Nf6 4.Qxf7#")]);
    let doc = build_page(&input(0, &text, &[reading(0, 60.0, fen, true, true)], &[], &[]));
    use MoveStatus::*;
    assert_eq!(moves(&doc.pages[0]), vec![("2...Nc6".into(), Caption), ("3.Qh5".into(), Valid), ("Nf6".into(), Valid), ("4.Qxf7#".into(), Valid)]);
    let game = &doc.games[0];
    assert_eq!(game.score.start_fen.as_deref(), Some("r1bqkbnr/pppp1ppp/2n5/4p3/2B1P3/8/PPPP1PPP/RNBQK1NR w KQkq - 0 3"));
    assert!(game.flags.is_empty(), "{:?}", game.flags);
    assert!(pgn::write_game(&game.score).contains("3. Qh5 Nf6 4. Qxf7#"));
    // In Portuguese, without a colon, after a White move: Black to move at the same number.
    let fen = "rnbqkbnr/pppp1ppp/8/4p3/2B1P3/5Q2/PPPP1PPP/RNB1K1NR w KQkq - 0 1";
    let text = page_text(vec![line(230.0, "Posição após 3.Qf3"), line(250.0, "3...Nc6")]);
    let doc = build_page(&input(0, &text, &[reading(0, 60.0, fen, true, true)], &[], &[]));
    assert_eq!(moves(&doc.pages[0]), vec![("3.Qf3".into(), Caption), ("3...Nc6".into(), Valid)]);
    assert_eq!(doc.games[0].score.start_fen.as_deref(), Some("rnbqkbnr/pppp1ppp/8/4p3/2B1P3/5Q2/PPPP1PPP/RNB1K1NR b KQkq - 0 3"));
}

#[test]
fn a_caption_keeps_en_passant_and_the_castling_rights_its_move_left() {
    // After 1...d5 the e4 pawn takes en passant: the caption leaves the square passed.
    let fen = "rnbqkbnr/ppp1pppp/8/3pP3/8/8/PPPP1PPP/RNBQKBNR w KQkq - 0 1";
    let text = page_text(vec![line(230.0, "Position after: 2...d5"), line(250.0, "3.exd6")]);
    let doc = build_page(&input(0, &text, &[reading(0, 60.0, fen, true, true)], &[], &[]));
    assert_eq!(moves(&doc.pages[0]), vec![("2...d5".into(), MoveStatus::Caption), ("3.exd6".into(), MoveStatus::Valid)]);
    assert_eq!(doc.games[0].score.start_fen.as_deref(), Some("rnbqkbnr/ppp1pppp/8/3pP3/8/8/PPPP1PPP/RNBQKBNR w KQkq d6 0 3"));
    // After Black castles, Black's rights go and White's stay: White can still castle.
    let fen = "rnbq1rk1/ppppbppp/5n2/4p3/4P3/5N2/PPPPBPPP/RNBQK2R w KQkq - 0 1";
    let text = page_text(vec![line(230.0, "Position after: 4...O-O"), line(250.0, "5.O-O")]);
    let doc = build_page(&input(0, &text, &[reading(0, 60.0, fen, true, true)], &[], &[]));
    assert_eq!(moves(&doc.pages[0]), vec![("4...O-O".into(), MoveStatus::Caption), ("5.O-O".into(), MoveStatus::Valid)]);
    assert_eq!(doc.games[0].score.start_fen.as_deref(), Some("rnbq1rk1/ppppbppp/5n2/4p3/4P3/5N2/PPPPBPPP/RNBQK2R w KQ - 0 5"));
}

#[test]
fn a_caption_that_is_the_open_games_next_move_is_played() {
    // The game reaches 2.Bc4; the diagram reprints the position after Black's reply, captioned
    // with it: the reply is the game's move, and the game goes on past the diagram.
    let text = page_text(vec![line(20.0, "1.e4 e5 2.Bc4"), line(230.0, "Position after: 2...Nc6"), line(250.0, "3.Qh5 Nf6 4.Qxf7#")]);
    let doc = build_page(&input(0, &text, &[reading(0, 60.0, SCHOLAR, true, true)], &[], &[]));
    assert!(moves(&doc.pages[0]).iter().all(|(_, s)| *s == MoveStatus::Valid), "{:?}", moves(&doc.pages[0]));
    assert_eq!(doc.games.len(), 1);
    assert!(pgn::write_game(&doc.games[0].score).contains("1. e4 e5 2. Bc4 Nc6 3. Qh5 Nf6 4. Qxf7#"));
}

#[test]
fn a_caption_needs_its_cue_its_piece_in_place_and_an_agreeing_side() {
    let fen = "r1bqkbnr/pppp1ppp/2n5/4p3/2B1P3/8/PPPP1PPP/RNBQK1NR w KQkq - 0 1";
    let caption = |words: &str, assumed: bool| {
        let text = page_text(vec![line(230.0, &format!("{words} 2...Nc6")), line(250.0, "3.Qh5")]);
        let doc = build_page(&input(0, &text, &[reading(0, 60.0, fen, true, assumed)], &[], &[]));
        moves(&doc.pages[0]).first().map(|(_, s)| *s)
    };
    assert_eq!(caption("Position after:", true), Some(MoveStatus::Caption));
    assert_eq!(caption("Stellung nach", true), Some(MoveStatus::Caption));
    // No cue: the move is read as the game's first, which it cannot be.
    assert_eq!(caption("Black played", true), Some(MoveStatus::Illegal));
    assert_eq!(caption("Thereafter", true), Some(MoveStatus::Illegal));
    // The reading said White to move, not assumed, and the caption agrees: only the number.
    assert_eq!(caption("Position after:", false), Some(MoveStatus::Caption));
    // A move still to come is never a caption: its piece is not on its square yet.
    let text = page_text(vec![line(230.0, "Position after: 3.Qh5"), line(250.0, "3...Nf6")]);
    let doc = build_page(&input(0, &text, &[reading(0, 60.0, fen, true, true)], &[], &[]));
    assert_eq!(moves(&doc.pages[0]).first().map(|(_, s)| *s), Some(MoveStatus::Valid));
    // Black to move, said by the reading (not assumed): a caption that leaves White to move is
    // not believed.
    let black = "r1bqkbnr/pppp1ppp/2n5/4p3/2B1P3/8/PPPP1PPP/RNBQK1NR b KQkq - 0 1";
    let text = page_text(vec![line(230.0, "Position after: 2...Nc6")]);
    let doc = build_page(&input(0, &text, &[reading(0, 60.0, black, true, false)], &[], &[]));
    assert_eq!(moves(&doc.pages[0]).first().map(|(_, s)| *s), Some(MoveStatus::Illegal));
}

#[test]
fn the_first_moves_number_sets_an_assumed_side_and_the_move_number() {
    // A rook ending read with White to move, assumed; the text says Black moves at move 41.
    let fen = "8/8/4k3/8/8/4K3/r7/7R w - - 0 1";
    let text = page_text(vec![line(230.0, "41...Ra3+ 42.Kd4 Rh3")]);
    let doc = build_page(&input(0, &text, &[reading(0, 60.0, fen, true, true)], &[], &[]));
    let game = &doc.games[0];
    assert_eq!(game.score.start_fen.as_deref(), Some("8/8/4k3/8/8/4K3/r7/7R b - - 0 41"));
    assert_eq!(game.flags[0].flag, ReviewFlag::SideFromMoveNumber);
    assert!(moves(&doc.pages[0]).iter().all(|(_, s)| *s == MoveStatus::Valid));
    assert!(pgn::write_game(&game.score).contains("41... Ra3+ 42. Kd4 Rh3"));
    // Not assumed: the reading wins, the conflict is flagged and the number not believed.
    let doc = build_page(&input(0, &text, &[reading(0, 60.0, fen, true, false)], &[], &[]));
    assert_eq!(doc.games[0].flags[0].flag, ReviewFlag::SideConflict);
    assert_eq!(moves(&doc.pages[0])[0].1, MoveStatus::Illegal, "Ra3+ is not a White move");
}

#[test]
fn unbracketed_second_solutions_and_brackets_become_variations() {
    // 3.Qh5 Nf6 4.Qxf7#, then the text's second line: 3...g6 4.Qf3 as an alternative to 3...Nf6.
    let text = page_text(vec![line(230.0, "3.Qh5 Nf6 4.Qxf7# . If 3...g6 then 4.Qf3 [ 4.Qxe5+ Qe7 ] Nf6")]);
    let doc = build_page(&input(0, &text, &[reading(0, 60.0, SCHOLAR, true, false)], &[], &[]));
    let pgn = pgn::write_game(&doc.games[0].score);
    assert!(pgn.ends_with("3. Qh5 Nf6 (3... g6 {then} 4. Qf3 (4. Qxe5+ Qe7) 4... Nf6) 4. Qxf7# {. If} *\n"), "{pgn}");
}

#[test]
fn a_game_runs_on_across_pages_and_a_new_one_starts_at_one() {
    let p0 = page_text(vec![line(700.0, "1.e4 e5 2.Nf3")]);
    let p1 = page_text(vec![line(40.0, "Nc6 3.Bb5 . Another game: 1.d4 d5")]);
    let mut b = DocBuilder::new();
    b.add_page(&input(0, &p0, &[], &[], &[]));
    b.add_page(&input(1, &p1, &[], &[], &[]));
    let doc = b.finish();
    assert_eq!(doc.games.len(), 2);
    assert_eq!(doc.games[0].score.mainline().len(), 5);
    assert_eq!(doc.games[1].page, 1);
    let r = |i: usize| doc.pages[1].blocks[0].inlines().iter().filter_map(|x| if let Inline::Move(m) = x { m.game } else { None }).nth(i);
    assert_eq!(r(0), Some(NodeRef { game: 0, node: 3 }));
    assert_eq!(r(2), Some(NodeRef { game: 1, node: 0 }));
}

/// Two diagrams, then both texts: each text starts from the diagram its first move is legal in,
/// not from the last one read.
#[test]
fn texts_after_two_diagrams_find_their_own() {
    let rook = "8/8/4k3/8/8/4K3/r7/7R w - - 0 1";
    let text = page_text(vec![line(380.0, "3.Qh5 Nf6 4.Qxf7#"), line(410.0, "1.Rh6+ Kd5 2.Rh5+")]);
    let diagrams = [reading(0, 60.0, SCHOLAR, true, false), reading(1, 220.0, rook, true, false)];
    let doc = build_page(&input(0, &text, &diagrams, &[], &[]));
    assert!(moves(&doc.pages[0]).iter().all(|(_, s)| *s == MoveStatus::Valid), "{:?}", moves(&doc.pages[0]));
    assert_eq!(doc.games.len(), 2);
    assert_eq!(doc.games[0].origin, GameOrigin::Diagram { page: 0, diagram: 0, accepted: true });
    assert_eq!(doc.games[1].origin, GameOrigin::Diagram { page: 0, diagram: 1, accepted: true });
    for game in &doc.games {
        assert!(game.flags.iter().any(|f| f.flag == ReviewFlag::DiagramMatchedByMove));
    }
    // A `1.` that would be another first move of the open game, and fits a diagram with no text
    // yet, starts that diagram's game.
    let rook = "8/8/4k3/8/8/4K3/r5P1/7R w - - 0 1";
    let pawn = "8/8/8/3k4/8/8/6P1/K7 w - - 0 1";
    let text = page_text(vec![line(380.0, "1.Rh6+ Kd5 2.Rh5+"), line(410.0, "1.g3 Ke4")]);
    let two = [reading(0, 60.0, rook, true, false), reading(1, 220.0, pawn, true, false)];
    let doc = build_page(&input(0, &text, &two, &[], &[]));
    assert!(moves(&doc.pages[0]).iter().all(|(_, s)| *s == MoveStatus::Valid), "{:?}", moves(&doc.pages[0]));
    assert_eq!(doc.games.len(), 2);
    assert_eq!(doc.games[1].origin, GameOrigin::Diagram { page: 0, diagram: 1, accepted: true });
    // A text that fits the diagram just before it needs no match.
    let doc = build_page(&input(0, &page_text(vec![line(380.0, "1.Rh6+ Kd5")]), &diagrams, &[], &[]));
    assert_eq!(doc.games[0].origin, GameOrigin::Diagram { page: 0, diagram: 1, accepted: true });
    assert!(doc.games[0].flags.is_empty());
}

/// A game goes on past a diagram of the position it reached, the book reprinting it on the next
/// page; and past a diagram its next move cannot start from, flagged.
#[test]
fn a_game_goes_on_past_a_diagram_of_its_own_position() {
    let after = "rnbqkbnr/pppp1ppp/8/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 0 1";
    let p0 = page_text(vec![line(700.0, "1.e4 e5 2.Nf3")]);
    let p1 = page_text(vec![line(230.0, "Black to play. 2...Nc6 3.Bb5")]);
    for (fen, assumed, flagged) in [(after, true, false), (SCHOLAR, false, true)] {
        let mut b = DocBuilder::new();
        b.add_page(&input(0, &p0, &[], &[], &[]));
        b.add_page(&input(1, &p1, &[reading(0, 60.0, fen, true, assumed)], &[], &[]));
        let doc = b.finish();
        assert_eq!(doc.games.len(), 1, "{fen}");
        assert_eq!(doc.games[0].score.mainline().len(), 5);
        assert!(moves(&doc.pages[1]).iter().all(|(_, s)| *s == MoveStatus::Valid));
        assert_eq!(doc.games[0].flags.iter().any(|f| f.flag == ReviewFlag::ContinuedPastDiagram { page: 1, diagram: 0 }), flagged);
        assert!(!pgn::write_game(&doc.games[0].score).contains("Black to play"), "the caption goes to neither game");
    }
}

/// After an alternative written as prose, the text goes back to the main line: a move the
/// alternative cannot play is tried there.
#[test]
fn the_main_line_resumes_after_a_prose_alternative() {
    let text = page_text(vec![line(230.0, "1.e4 e5 2.Nf3 Nc6 . Instead 2.Bc4 Nf6 is calmer. 3.Nxe5 Nxe5")]);
    let doc = build_page(&input(0, &text, &[], &[], &[]));
    assert!(moves(&doc.pages[0]).iter().all(|(_, s)| *s == MoveStatus::Valid), "{:?}", moves(&doc.pages[0]));
    let score = &doc.games[0].score;
    let main: Vec<&str> = score.mainline().iter().map(|&n| score.nodes[n].san.as_str()).collect();
    assert_eq!(main, ["e4", "e5", "Nf3", "Nc6", "Nxe5", "Nxe5"]);
}

/// After a prose digression, a numbered move that is exactly the main line's next move goes back
/// to the main line, even though it could also be an alternative inside the digression
/// (Dvoretsky's `2...Kd7? 3.Kf4 … 9.Kf5` and then `3.Kd4 Kf6!`). Here `3.Bb5` would be legal in
/// the `2...d6` line too (with check), where `a6` could not answer it.
#[test]
fn a_move_that_is_the_main_lines_next_goes_back_to_it() {
    let text = page_text(vec![line(230.0, "1.e4 e5 2.Nf3 Nc6! If 2...d6? 3.d4 Nd7 4.Bc4 c6 is passive. 3.Bb5 a6 4.Ba4 Nf6")]);
    let doc = build_page(&input(0, &text, &[], &[], &[]));
    assert!(moves(&doc.pages[0]).iter().all(|(_, s)| *s == MoveStatus::Valid), "{:?}", moves(&doc.pages[0]));
    let score = &doc.games[0].score;
    let main: Vec<&str> = score.mainline().iter().map(|&n| score.nodes[n].san.as_str()).collect();
    assert_eq!(main, ["e4", "e5", "Nf3", "Nc6", "Bb5", "a6", "Ba4", "Nf6"]);
    // A move that goes straight on from the digression stays in it: `2.Nf3` continues `1...c5`.
    let text = page_text(vec![line(230.0, "1.e4 e5! Not 1...c5 2.Nf3 d6 here.")]);
    let doc = build_page(&input(0, &text, &[], &[], &[]));
    let score = &doc.games[0].score;
    let main: Vec<&str> = score.mainline().iter().map(|&n| score.nodes[n].san.as_str()).collect();
    assert_eq!(main, ["e4", "e5"]);
    assert!(moves(&doc.pages[0]).iter().all(|(_, s)| *s == MoveStatus::Valid));
}

fn mainline(doc: &ChessDoc) -> Vec<&str> {
    let score = &doc.games[0].score;
    score.mainline().iter().map(|&n| score.nodes[n].san.as_str()).collect()
}

/// Books print their main line in bold and the analysis in regular type. Analysis that comes
/// before the main line (Dvoretsky's `Black wins after 1...Be6+ 2.Kg6`, then the game's bold
/// `1...Bxg7?`) is not the main line: the bold moves are, and the game goes on from them.
#[test]
fn bold_moves_are_the_main_line_when_analysis_comes_first() {
    let text = page_text(vec![line(230.0, "After 3.Qf3 Nf6 4.Qb3 it is level. *3.Qh5 *Nf6 Better 3...g6 4.Qf3 Nf6 *4.Qxf7#")]);
    let diagrams = [reading(0, 60.0, SCHOLAR, true, false)];
    let doc = build_page(&input(0, &text, &diagrams, &[], &[]));
    assert!(moves(&doc.pages[0]).iter().all(|(_, s)| *s == MoveStatus::Valid), "{:?}", moves(&doc.pages[0]));
    assert_eq!(mainline(&doc), ["Qh5", "Nf6", "Qxf7#"]);
    assert_eq!(doc.games.len(), 1);
    // The analysis is still there, as variations, and the bold flag is kept on the moves.
    let pgn = pgn::write_game(&doc.games[0].score).split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(pgn.contains("3. Qh5 (3. Qf3 Nf6 4. Qb3") && pgn.contains("(3... g6 4. Qf3 Nf6 ) 4. Qxf7#"), "{pgn}");
    let bold: Vec<bool> =
        doc.pages[0].blocks.iter().flat_map(|b| b.inlines()).filter_map(|i| if let Inline::Move(m) = i { Some(m.bold) } else { None }).collect();
    assert_eq!(bold, [false, false, false, true, true, false, false, false, true]);
    // Without weights (OCR, documents read before) the first chain stays the main line.
    let plain = page_text(vec![line(230.0, "After 3.Qf3 Nf6 4.Qb3 it is level. 3.Qh5 Nf6 Better 3...g6 4.Qf3 Nf6 4.Qxf7#")]);
    let doc = build_page(&input(0, &plain, &diagrams, &[], &[]));
    assert_eq!(mainline(&doc), ["Qf3", "Nf6", "Qb3"]);
}

/// Analysis that goes straight on from the bold line's last move (`4...Kg4`, then `Tarrasch had
/// expected 5.Kf6? c4`) is not the main line either: the next bold move (`5.Kg6!`) takes its place.
#[test]
fn a_bold_move_takes_the_place_of_analysis_that_went_on_from_it() {
    let text = page_text(vec![line(230.0, "*1.e4 *e5 *2.Nf3 *Nc6 He expected 3.Bc4 Nf6 4.d3 here. *3.Bb5 *a6")]);
    let doc = build_page(&input(0, &text, &[], &[], &[]));
    assert!(moves(&doc.pages[0]).iter().all(|(_, s)| *s == MoveStatus::Valid), "{:?}", moves(&doc.pages[0]));
    assert_eq!(mainline(&doc), ["e4", "e5", "Nf3", "Nc6", "Bb5", "a6"]);
    // All in regular type, the line read first stays the main line.
    let text = page_text(vec![line(230.0, "1.e4 e5 2.Nf3 Nc6 He expected 3.Bc4 Nf6 4.d3 here. 3.Bb5 a6")]);
    let doc = build_page(&input(0, &text, &[], &[], &[]));
    assert_eq!(mainline(&doc), ["e4", "e5", "Nf3", "Nc6", "Bc4", "Nf6", "d3"]);
    // All in bold, nothing changes either: a book that sets everything in bold says nothing.
    let text = page_text(vec![line(230.0, "*1.e4 *e5 *2.Nf3 *Nc6 He expected *3.Bc4 *Nf6 *4.d3 here. *3.Bb5 *a6")]);
    let doc = build_page(&input(0, &text, &[], &[], &[]));
    assert_eq!(mainline(&doc), ["e4", "e5", "Nf3", "Nc6", "Bc4", "Nf6", "d3"]);
}

/// An unnumbered bold move goes on from the bold line too (Fischer prints `6 …` and `e5` on
/// separate lines, after the regular `6 ♗c4`), but not after a bold move that failed: there it
/// would be read a ply early, as the other side's move (`2.Bxf7`, then `e5` as White's pawn).
#[test]
fn an_unnumbered_bold_move_goes_on_from_the_last_bold_move_played() {
    let text = page_text(vec![line(230.0, "*1.e4 *e5 *2.Nf3 *Nc6 *3.Bb5 Instead 3.Bc4 is the Italian. *a6")]);
    let doc = build_page(&input(0, &text, &[], &[], &[]));
    assert!(moves(&doc.pages[0]).iter().all(|(_, s)| *s == MoveStatus::Valid), "{:?}", moves(&doc.pages[0]));
    assert_eq!(mainline(&doc), ["e4", "e5", "Nf3", "Nc6", "Bb5", "a6"]);
    let text = page_text(vec![line(230.0, "*1.e4 *d6 Or 1...c5 2.Nf3 here. *2.Bxf7 *e5")]);
    let doc = build_page(&input(0, &text, &[], &[], &[]));
    let read = moves(&doc.pages[0]);
    assert_eq!(read.last(), Some(&("e5".to_owned(), MoveStatus::NotReached)), "{read:?}");
    assert_eq!(mainline(&doc), ["e4", "d6"]);
}

/// A bold move that the bold line cannot take, read inside a line of analysis, does not make that
/// analysis the main line.
#[test]
fn a_bold_move_inside_analysis_does_not_make_it_the_main_line() {
    let text = page_text(vec![line(230.0, "*1.e4 *e5 Or 1...c5 2.Nf3 d6 *3.Bb5+")]);
    let doc = build_page(&input(0, &text, &[], &[], &[]));
    assert!(moves(&doc.pages[0]).iter().all(|(_, s)| *s == MoveStatus::Valid), "{:?}", moves(&doc.pages[0]));
    assert_eq!(mainline(&doc), ["e4", "e5"]);
}

/// A move's weight is kept in the document's JSON only when it is bold, so documents written
/// before it was kept read as they did.
#[test]
fn a_moves_weight_is_in_the_json_only_when_bold() {
    let text = page_text(vec![line(230.0, "*1.e4 e5")]);
    let doc = build_page(&input(0, &text, &[], &[], &[]));
    let json = doc.to_json().unwrap();
    assert_eq!(json.matches("\"bold\":true").count(), 1, "{json}");
    assert!(!json.contains("\"bold\":false"));
    assert_eq!(ChessDoc::from_json(&json).unwrap(), doc);
    let old = json.replace(",\"bold\":true", "");
    let read = ChessDoc::from_json(&old).unwrap();
    assert!(read.pages[0].blocks.iter().flat_map(|b| b.inlines()).all(|i| !matches!(i, Inline::Move(m) if m.bold)));
}

/// Pages read before are judged again from what is printed: replaying a finished document's
/// pages gives the same document, whatever statuses they carried.
#[test]
fn read_pages_replay_into_the_same_document() {
    let (text, diagrams, hints) = showcase();
    let doc = build_page(&input(0, &text, &diagrams, &hints, &[]));
    let mut stale = doc.pages.clone();
    for block in &mut stale[0].blocks {
        if let Block::Paragraph { inlines, .. } = block {
            for inline in inlines {
                if let Inline::Move(m) = inline {
                    m.status = MoveStatus::Illegal;
                    m.game = None;
                }
            }
        }
    }
    let mut builder = DocBuilder::new();
    for page in stale {
        builder.add_read_page(page);
    }
    assert_eq!(builder.finish(), doc);
}

/// Brackets after a move holding the replies to it: they are played after it, and the main
/// line's own reply still goes first.
#[test]
fn brackets_can_hold_the_replies_to_a_move() {
    let text = page_text(vec![line(230.0, "1.e4 ( 1...c5 2.Nf3 ; 1...e6 2.d4 ) 1...e5 2.Nf3")]);
    let doc = build_page(&input(0, &text, &[], &[], &[]));
    assert!(moves(&doc.pages[0]).iter().all(|(_, s)| *s == MoveStatus::Valid), "{:?}", moves(&doc.pages[0]));
    let score = &doc.games[0].score;
    let main: Vec<&str> = score.mainline().iter().map(|&n| score.nodes[n].san.as_str()).collect();
    assert_eq!(main, ["e4", "e5", "Nf3"]);
    let pgn = pgn::write_game(score);
    assert!(pgn.contains("1. e4 e5 (1... c5 2. Nf3") && pgn.contains("(1... e6 2. d4)"), "{pgn}");
    // A bracket whose first move is an alternative stays one.
    let text = page_text(vec![line(230.0, "1.e4 e5 2.Nf3 ( 2.Bc4 Nf6 ) Nc6")]);
    let doc = build_page(&input(0, &text, &[], &[], &[]));
    let pgn = pgn::write_game(&doc.games[0].score);
    assert!(pgn.contains("2. Nf3 (2. Bc4 Nf6) 2... Nc6"), "{pgn}");
}

/// One diagram, the solution for each side to move: Black's first (`1...Kd5`, the side taken
/// from the move number), then White's (`1.Rh6+`). Each is a game from the diagram.
#[test]
fn a_diagram_can_have_a_solution_for_each_side() {
    let rook = "8/8/4k3/8/8/4K3/r7/7R w - - 0 1";
    let text = page_text(vec![line(230.0, "If Black is on move, 1...Kd5 2.Rh5+ . White plays 1.Rh6+ Kd5 2.Rh5+")]);
    let doc = build_page(&input(0, &text, &[reading(0, 60.0, rook, true, true)], &[], &[]));
    assert!(moves(&doc.pages[0]).iter().all(|(_, s)| *s == MoveStatus::Valid), "{:?}", moves(&doc.pages[0]));
    assert_eq!(doc.games.len(), 2);
    assert_eq!(doc.games[0].score.start_fen.as_deref(), Some("8/8/4k3/8/8/4K3/r7/7R b - - 0 1"));
    assert_eq!(doc.games[1].score.start_fen.as_deref(), Some("8/8/4k3/8/8/4K3/r7/7R w - - 0 1"));
    assert_eq!(doc.games[1].origin, GameOrigin::Diagram { page: 0, diagram: 0, accepted: true });
}

/// Main lines set larger than the notes: a block with moves is a paragraph even when its size
/// makes it a heading, so it does not end the game.
#[test]
fn a_block_with_moves_is_never_a_heading() {
    let text = page_text(vec![line(40.0, "1.e4 e5"), line(70.0, "Both sides open."), line(100.0, "2.Nf3 Nc6")]);
    let hints = [
        BlockHint { kind: HintKind::Heading { level: 2 }, rect: user_box(30.0, 35.0, 300.0, 55.0) },
        BlockHint { kind: HintKind::Heading { level: 2 }, rect: user_box(30.0, 95.0, 300.0, 115.0) },
    ];
    let doc = build_page(&input(0, &text, &[], &hints, &[]));
    assert!(kinds(&doc.pages[0]).iter().all(|k| k.starts_with("P:")), "{:?}", kinds(&doc.pages[0]));
    assert_eq!(doc.games.len(), 1);
    assert_eq!(doc.games[0].score.mainline().len(), 4);
}

/// A game collection: the last game's diagram (at move 26), then the next game from move 1.
#[test]
fn a_new_game_from_one_after_a_later_diagram() {
    // White to play move 27.
    let after = "1r3rk1/3b1p1p/1q1p2p1/3Pb3/1p6/P7/1PR1BQPP/5R1K w - - 0 27";
    let text = page_text(vec![line(230.0, "27.Bc4 Rb7 . Next game 1.c4 Nf6 2.Nc3 g6")]);
    let doc = build_page(&input(0, &text, &[reading(0, 60.0, after, true, false)], &[], &[]));
    assert!(moves(&doc.pages[0]).iter().all(|(_, s)| *s == MoveStatus::Valid), "{:?}", moves(&doc.pages[0]));
    assert_eq!(doc.games.len(), 2);
    assert_eq!(doc.games[1].origin, GameOrigin::Initial);
}

#[test]
fn puzzle_labels() {
    use crate::doc::build::puzzle_label;
    for (text, want) in [
        ("№185. Teichmann – Lasker", 185),
        ("N!!185. Teichmann", 185),
        ("N!! 186. Lasker – Marco", 186),
        ("N9191. Lasker", 191),
        ("No. 12 Smith", 12),
        ("Nr 7)", 7),
        ("#42:", 42),
        ("Exercício 12 ★△", 12),
        ("Exercicio 3", 3),
        ("Exercise 5: White to play", 5),
        ("Ex. 6–12", 12),
        ("Ex. 24-3", 3),
        ("Ex. 7", 7),
    ] {
        assert_eq!(puzzle_label(text, false), Some(want), "{text}");
    }
    // A bare number only in a caption, and only with its full stop.
    assert_eq!(puzzle_label("185. Teichmann – Lasker", true), Some(185));
    assert_eq!(puzzle_label("185. Teichmann – Lasker", false), None);
    assert_eq!(puzzle_label("1675 ", true), Some(1675));
    assert_eq!(puzzle_label("1675", false), None);
    assert_eq!(puzzle_label("2.2 Combinations #2 (451-3514)", true), None, "a section number");
    for text in [
        "1996 was a good year",
        "6.",
        "6. ±",
        "N9",
        "No way",
        "Nothing 12.",
        "№0.",
        "№123456.",
        "the №5 is",
        "",
        "Exercises 1.165-1.168",
        "Ex. 6–",
        "Ex. 6–0",
        "Ex. 6–12a",
        "Example 5",
        "Exercício",
    ] {
        assert_eq!(puzzle_label(text, true), None, "{text}");
    }
}

/// A puzzle book: the diagrams with their numbers, the solutions pages on, in another order.
/// Each solution is played from the diagram its number names.
#[test]
fn solutions_are_played_from_their_numbered_diagrams() {
    let rook = "8/8/4k3/8/8/4K3/r7/7R w - - 0 1";
    // Under each diagram, nearer than its `№` caption, a line that reads as a bare caption: the
    // `№` wins.
    let p0 = page_text(vec![
        line(45.0, "№1. Brown – Green"),
        line(211.0, "12. White to play"),
        line(245.0, "№2. Smith – Jones"),
        line(411.0, "13. Black to play"),
    ]);
    let d0 = [reading(0, 60.0, SCHOLAR, true, false), reading(1, 260.0, rook, true, false)];
    let p1 = page_text(vec![line(100.0, "Solutions follow on the next page.")]);
    let p2 = page_text(vec![
        line(100.0, "№2. Smith – Jones"),
        line(120.0, "1.Rh6+ Kd5 2.Rh5+"),
        line(160.0, "№1. Brown – Green"),
        line(180.0, "3.Qh5 Nf6 4.Qxf7#"),
    ]);
    let mut b = DocBuilder::new();
    b.add_page(&input(0, &p0, &d0, &[], &[]));
    b.add_page(&input(1, &p1, &[], &[], &[]));
    b.add_page(&input(2, &p2, &[], &[], &[]));
    let doc = b.finish();
    let numbers: Vec<Option<u32>> =
        doc.pages[0].blocks.iter().filter_map(|b| if let Block::Diagram(d) = b { Some(d.puzzle) } else { None }).collect();
    assert_eq!(numbers, [Some(1), Some(2)]);
    assert!(moves(&doc.pages[2]).iter().all(|(_, s)| *s == MoveStatus::Valid), "{:?}", moves(&doc.pages[2]));
    assert_eq!(doc.games.len(), 2);
    assert_eq!(doc.games[0].origin, GameOrigin::Diagram { page: 0, diagram: 1, accepted: true });
    assert_eq!(doc.games[1].origin, GameOrigin::Diagram { page: 0, diagram: 0, accepted: true });
    assert!(pgn::write_game(&doc.games[1].score).contains("[PdfPuzzle \"1\"]"));
}

/// Polgar's layout: the number alone above each diagram (`1675`), and solutions as the number
/// alone before the key move (`1675 1.Rd3+`), pages on.
#[test]
fn bare_numbers_link_solutions_to_diagrams() {
    let rook = "8/8/4k3/8/8/4K3/r7/7R w - - 0 1";
    let p0 = page_text(vec![line(20.0, "2.2 Combinations #2"), line(45.0, "1675"), line(245.0, "1676")]);
    let d0 = [reading(0, 60.0, SCHOLAR, true, false), reading(1, 260.0, rook, true, false)];
    let p1 = page_text(vec![line(100.0, "Solutions")]);
    // 1601 has no diagram here: its move is not read into the game before it.
    let p2 = page_text(vec![line(100.0, "1676 1.Rh6+ Kd5"), line(120.0, "1601 1.Re1"), line(140.0, "1675 1.Qh5 Nf6")]);
    let mut b = DocBuilder::new();
    b.add_page(&input(0, &p0, &d0, &[], &[]));
    b.add_page(&input(1, &p1, &[], &[], &[]));
    b.add_page(&input(2, &p2, &[], &[], &[]));
    let doc = b.finish();
    let numbers: Vec<Option<u32>> =
        doc.pages[0].blocks.iter().filter_map(|b| if let Block::Diagram(d) = b { Some(d.puzzle) } else { None }).collect();
    assert_eq!(numbers, [Some(1675), Some(1676)]);
    let statuses: Vec<MoveStatus> = moves(&doc.pages[2]).iter().map(|(_, s)| *s).collect();
    assert_eq!(statuses, [MoveStatus::Valid, MoveStatus::Valid, MoveStatus::Unchecked, MoveStatus::Valid, MoveStatus::Valid]);
    assert_eq!(
        doc.games.iter().map(|g| g.origin).collect::<Vec<_>>(),
        [GameOrigin::Diagram { page: 0, diagram: 1, accepted: true }, GameOrigin::Diagram { page: 0, diagram: 0, accepted: true }]
    );
}

/// A workbook's chapter: "Exercício N" under each diagram, then the solutions, each headed by
/// the chapter and the exercise (`Ex. 6–2`) with the players and the event on the lines (and
/// headings) after it. Each solution is played from the exercise it names.
#[test]
fn exercise_labels_link_solutions_to_diagrams() {
    let rook = "8/8/4k3/8/8/4K3/r7/7R w - - 0 1";
    let p0 = page_text(vec![line(215.0, "Exercício 1 ★△"), line(415.0, "Exercício 2 ★★△")]);
    let d0 = [reading(0, 60.0, SCHOLAR, true, false), reading(1, 260.0, rook, true, false)];
    let p1 = page_text(vec![
        line(100.0, "Ex. 6–2"),
        line(120.0, "A. Smith — B. Jones"),
        line(140.0, "Wijk aan Zee 1999"),
        line(160.0, "1.Rh6+ Kd5 2.Rh5+"),
        line(200.0, "Ex. 6–1"),
        line(220.0, "C. Brown — D. Green"),
        line(240.0, "3.Qh5 Nf6 4.Qxf7#"),
    ]);
    let heading = |top: f32| BlockHint { kind: HintKind::Heading { level: 2 }, rect: user_box(30.0, top - 5.0, 300.0, top + 15.0) };
    let hints = [heading(100.0), heading(120.0), heading(200.0), heading(220.0)];
    let mut b = DocBuilder::new();
    b.add_page(&input(0, &p0, &d0, &[], &[]));
    b.add_page(&input(1, &p1, &[], &hints, &[]));
    let doc = b.finish();
    assert_eq!(kinds(&doc.pages[1]).iter().filter(|k| k.starts_with("H:")).count(), 4, "{:?}", kinds(&doc.pages[1]));
    let numbers: Vec<Option<u32>> =
        doc.pages[0].blocks.iter().filter_map(|b| if let Block::Diagram(d) = b { Some(d.puzzle) } else { None }).collect();
    assert_eq!(numbers, [Some(1), Some(2)]);
    assert!(moves(&doc.pages[1]).iter().all(|(_, s)| *s == MoveStatus::Valid), "{:?}", moves(&doc.pages[1]));
    assert_eq!(
        doc.games.iter().map(|g| g.origin).collect::<Vec<_>>(),
        [GameOrigin::Diagram { page: 0, diagram: 1, accepted: true }, GameOrigin::Diagram { page: 0, diagram: 0, accepted: true }]
    );
}

/// An exercise's label names a solution only standing alone: prose that starts with the word and
/// a number does not, and the `№` labels keep their names after the number.
#[test]
fn solution_labels() {
    use crate::doc::build::solution_label;
    for (text, want) in [("Ex. 6–12", Some(12)), ("Exercício 3 ★△", Some(3)), ("Ex. 7.", Some(7)), ("№191. Lasker – Steinitz", Some(191))]
    {
        assert_eq!(solution_label(text), want, "{text}");
    }
    for text in ["Exercise 5 was difficult", "Ex. 6 is discussed below", "Ex. 5: examples from the previous chapter", "185. Teichmann"] {
        assert_eq!(solution_label(text), None, "{text}");
    }
}

/// The headings a solution's label keeps its diagram through are the players' and the event's:
/// a third heading is a new section's, and moves after it are not read from the exercise.
#[test]
fn a_section_heading_after_an_exercise_label_lets_it_go() {
    let rook = "8/8/4k3/8/8/4K3/r7/7R w - - 0 1";
    let p0 = page_text(vec![line(215.0, "Exercício 1 ★△")]);
    let p1 = page_text(vec![
        line(100.0, "Ex. 6–1"),
        line(120.0, "A. Smith — B. Jones"),
        line(140.0, "Wijk aan Zee 1999"),
        line(160.0, "Chapter 7"),
        line(180.0, "1.e4 e5 2.Nf3"),
    ]);
    let heading = |top: f32| BlockHint { kind: HintKind::Heading { level: 2 }, rect: user_box(30.0, top - 5.0, 300.0, top + 15.0) };
    let hints = [heading(100.0), heading(120.0), heading(140.0), heading(160.0)];
    let mut b = DocBuilder::new();
    b.add_page(&input(0, &p0, &[reading(0, 60.0, rook, true, false)], &[], &[]));
    b.add_page(&input(1, &p1, &[], &hints, &[]));
    let doc = b.finish();
    assert!(moves(&doc.pages[1]).iter().all(|(_, s)| *s == MoveStatus::Valid), "{:?}", moves(&doc.pages[1]));
    assert_eq!(doc.games.last().map(|g| g.origin), Some(GameOrigin::Initial));
}

/// Construa Seu Xadrez sets each exercise's caption above its diagram, and the last caption of a
/// page at its foot, with the diagram at the top of the next page: that diagram takes it, and the
/// solution naming it is played from it. Only the very next page takes it.
#[test]
fn a_caption_at_the_foot_of_a_page_numbers_the_next_pages_first_diagram() {
    let rook = "8/8/4k3/8/8/4K3/r7/7R w - - 0 1";
    let p0 = page_text(vec![line(45.0, "Exercício 1 ★△"), line(700.0, "Exercício 2 ★★△")]);
    let p1 = page_text(vec![line(300.0, "Ex. 6–2"), line(320.0, "1.Rh6+ Kd5 2.Rh5+")]);
    let heading = [BlockHint { kind: HintKind::Heading { level: 2 }, rect: user_box(30.0, 295.0, 300.0, 315.0) }];
    let mut b = DocBuilder::new();
    b.add_page(&input(0, &p0, &[reading(0, 60.0, SCHOLAR, true, false)], &[], &[]));
    b.add_page(&input(1, &p1, &[reading(0, 60.0, rook, true, false)], &heading, &[]));
    let doc = b.finish();
    let numbers: Vec<Option<u32>> =
        doc.pages.iter().flat_map(|p| p.blocks.iter()).filter_map(|b| if let Block::Diagram(d) = b { Some(d.puzzle) } else { None }).collect();
    assert_eq!(numbers, [Some(1), Some(2)]);
    assert!(moves(&doc.pages[1]).iter().all(|(_, s)| *s == MoveStatus::Valid), "{:?}", moves(&doc.pages[1]));
    // Page 0, then page 2: the caption is not carried past the page after it.
    let mut b = DocBuilder::new();
    b.add_page(&input(0, &p0, &[reading(0, 60.0, SCHOLAR, true, false)], &[], &[]));
    b.add_page(&input(2, &p1, &[reading(0, 60.0, rook, true, false)], &heading, &[]));
    let doc = b.finish();
    let numbers: Vec<Option<u32>> =
        doc.pages.iter().flat_map(|p| p.blocks.iter()).filter_map(|b| if let Block::Diagram(d) = b { Some(d.puzzle) } else { None }).collect();
    assert_eq!(numbers, [Some(1), None]);
    // A label followed by more text is not the page's last line: it is not carried. The page's
    // number under it does not count.
    let prose =
        page_text(vec![line(45.0, "Exercício 1 ★△"), line(700.0, "Exercício 2 ★★△"), line(720.0, "Exercício 2 mostra o mate."), line(790.0, "36")]);
    let numbered = page_text(vec![line(45.0, "Exercício 1 ★△"), line(700.0, "Exercício 2 ★★△"), line(790.0, "36")]);
    for (p0, want) in [(prose, None), (numbered, Some(2))] {
        let mut b = DocBuilder::new();
        b.add_page(&input(0, &p0, &[reading(0, 60.0, SCHOLAR, true, false)], &[], &[]));
        b.add_page(&input(1, &p1, &[reading(0, 60.0, rook, true, false)], &heading, &[]));
        let doc = b.finish();
        let numbers: Vec<Option<u32>> =
            doc.pages.iter().flat_map(|p| p.blocks.iter()).filter_map(|b| if let Block::Diagram(d) = b { Some(d.puzzle) } else { None }).collect();
        assert_eq!(numbers, [Some(1), want]);
    }
}

/// The same book read a page at a time (the Text tab, and a game read across the pages around a
/// move): page 1 read alone does not know the caption at the foot of page 0. Walked again after
/// page 0 ([`DocBuilder::add_read_page`]), or told it by a look at page 0
/// ([`DocBuilder::carry_caption`], [`trailing_label`]), its first diagram takes it, and the
/// solution naming it is played from it, as when the whole book is read.
#[test]
fn a_page_read_alone_takes_the_caption_at_the_foot_of_the_page_before() {
    let rook = "8/8/4k3/8/8/4K3/r7/7R w - - 0 1";
    let p0 = page_text(vec![line(45.0, "Exercício 1 ★△"), line(700.0, "Exercício 2 ★★△")]);
    let p1 = page_text(vec![line(300.0, "Ex. 6–2"), line(320.0, "1.Rh6+ Kd5 2.Rh5+")]);
    let heading = [BlockHint { kind: HintKind::Heading { level: 2 }, rect: user_box(30.0, 295.0, 300.0, 315.0) }];
    let d0 = [reading(0, 60.0, SCHOLAR, true, false)];
    let d1 = [reading(0, 60.0, rook, true, false)];
    let numbers = |doc: &ChessDoc| -> Vec<Option<u32>> {
        doc.pages.iter().flat_map(|p| p.blocks.iter()).filter_map(|b| if let Block::Diagram(d) = b { Some(d.puzzle) } else { None }).collect()
    };
    let alone0 = build_page(&input(0, &p0, &d0, &[], &[]));
    let alone1 = build_page(&input(1, &p1, &d1, &heading, &[]));
    assert_eq!(numbers(&alone1), [None], "alone, page 1 cannot know");
    assert_eq!(alone0.pages[0].trailing, Some(2));
    let json = alone0.to_json().unwrap();
    assert_eq!(json.matches("\"trailing\":2").count(), 1, "{json}");
    assert_eq!(ChessDoc::from_json(&json).unwrap(), alone0);
    assert!(!alone1.to_json().unwrap().contains("trailing"));
    // Walked again in order: page 1's diagram is exercise 2, and its solution is played from it.
    let mut b = DocBuilder::new();
    b.add_read_page(ChessDoc::from_json(&json).unwrap().pages.remove(0));
    b.add_read_page(alone1.pages[0].clone());
    let doc = b.finish();
    assert_eq!(numbers(&doc), [Some(1), Some(2)]);
    assert!(moves(&doc.pages[1]).iter().all(|(_, s)| *s == MoveStatus::Valid), "{:?}", moves(&doc.pages[1]));
    // Not past the page after it.
    let mut later = alone1.pages[0].clone();
    later.index = 2;
    let mut b = DocBuilder::new();
    b.add_read_page(alone0.pages[0].clone());
    b.add_read_page(later);
    assert_eq!(numbers(&b.finish()), [Some(1), None]);
    // A look at page 0 alone gives the same label to page 1 read on its own.
    let boxes: Vec<PageRect> = d0.iter().map(|d| d.bbox_user).collect();
    assert_eq!(trailing_label(&p0, &geometry(), &boxes), Some(2));
    let mut b = DocBuilder::new();
    b.carry_caption(0, 2);
    b.add_page(&input(1, &p1, &d1, &heading, &[]));
    assert_eq!(numbers(&b.finish()), [Some(2)]);
    let mut b = DocBuilder::new();
    b.carry_caption(0, 2);
    b.add_page(&input(3, &p1, &d1, &heading, &[]));
    assert_eq!(numbers(&b.finish()), [None], "only the very next page takes it");
    // The whole book read, then walked again page by page, is the same document.
    let mut b = DocBuilder::new();
    b.add_page(&input(0, &p0, &d0, &[], &[]));
    b.add_page(&input(1, &p1, &d1, &heading, &[]));
    let whole = b.finish();
    let mut b = DocBuilder::new();
    for page in whole.pages.clone() {
        b.add_read_page(page);
    }
    assert_eq!(b.finish(), whole);
}

/// A solution that prints its game from move 1 before it reaches the exercise's position: its
/// first move does not start from the exercise, so it is read from the initial position, as
/// without the label.
#[test]
fn a_solution_from_move_one_is_not_read_from_its_exercise() {
    let rook = "8/8/4k3/8/8/4K3/r7/7R w - - 0 1";
    let p0 = page_text(vec![line(45.0, "Exercício 1 ★△")]);
    let p1 = page_text(vec![line(100.0, "Ex. 6–1"), line(120.0, "1.e4 e5 2.Nf3 Nc6")]);
    let hints = [BlockHint { kind: HintKind::Heading { level: 2 }, rect: user_box(30.0, 95.0, 300.0, 115.0) }];
    let mut b = DocBuilder::new();
    b.add_page(&input(0, &p0, &[reading(0, 60.0, rook, true, false)], &[], &[]));
    b.add_page(&input(1, &p1, &[], &hints, &[]));
    let doc = b.finish();
    assert!(moves(&doc.pages[1]).iter().all(|(_, s)| *s == MoveStatus::Valid), "{:?}", moves(&doc.pages[1]));
    assert_eq!(doc.games.last().map(|g| g.origin), Some(GameOrigin::Initial));
}

/// A page of a workbook ([`workbook`]): two exercises numbered 1 and 2 under a chapter's heading
/// (when there is one), or solutions, one line each, those starting with `#` set as headings.
enum WorkbookPage<'a> {
    Exercises(Option<&'a str>, [&'a str; 2]),
    Solutions(&'a [&'a str]),
}

const ROOK: &str = "8/8/4k3/8/8/4K3/r7/7R w - - 0 1";
/// The solutions of the Scholar's position and of the rook's: each fails from the other.
const SCHOLAR_MATE: &str = "3.Qh5 Nf6 4.Qxf7#";
const ROOK_CHECKS: &str = "1.Rh6+ Kd5 2.Rh5+";

fn workbook(pages: &[WorkbookPage<'_>]) -> ChessDoc {
    let heading = |top: f32| BlockHint { kind: HintKind::Heading { level: 1 }, rect: user_box(30.0, top - 5.0, 300.0, top + 15.0) };
    let mut b = DocBuilder::new();
    for (index, page) in pages.iter().enumerate() {
        let (mut lines, mut hints, mut diagrams) = (Vec::new(), Vec::new(), Vec::new());
        match page {
            WorkbookPage::Exercises(title, fens) => {
                if let Some(t) = title {
                    lines.push(line(20.0, t));
                    hints.push(heading(20.0));
                }
                lines.extend([line(215.0, "Exercise 1"), line(415.0, "Exercise 2")]);
                diagrams = vec![reading(0, 60.0, fens[0], true, false), reading(1, 260.0, fens[1], true, false)];
            }
            WorkbookPage::Solutions(texts) => {
                for (k, t) in texts.iter().enumerate() {
                    let y = 40.0 + 20.0 * k as f32;
                    match t.strip_prefix('#') {
                        Some(h) => {
                            lines.push(line(y, h));
                            hints.push(heading(y));
                        }
                        None => lines.push(line(y, t)),
                    }
                }
            }
        }
        let text = page_text(lines);
        b.add_page(&input(index as u32, &text, &diagrams, &hints, &[]));
    }
    b.finish()
}

/// Every move of the document's solution pages is valid, and the games start from these
/// diagrams (page, diagram), in order.
fn assert_solved(doc: &ChessDoc, solution_pages: &[usize], origins: &[(u32, usize)]) {
    for &p in solution_pages {
        assert!(moves(&doc.pages[p]).iter().all(|(_, s)| *s == MoveStatus::Valid), "page {p}: {:?}", moves(&doc.pages[p]));
    }
    let want: Vec<GameOrigin> = origins.iter().map(|&(page, diagram)| GameOrigin::Diagram { page, diagram, accepted: true }).collect();
    assert_eq!(doc.games.iter().map(|g| g.origin).collect::<Vec<_>>(), want);
}

/// A workbook read in the chapters' order, whose second chapter's last caption (`Exercise 2`)
/// is set at the foot of a page, with its board on the next. That caption is not a solution: it
/// does not mark the first chapter solved, so the solutions still go to the chapters in order,
/// both when the book is read and when its pages are walked again ([`DocBuilder::add_read_page`]).
#[test]
fn a_caption_at_the_foot_is_not_a_solution_when_the_pages_are_walked_again() {
    let p0 = page_text(vec![line(215.0, "Exercise 1"), line(415.0, "Exercise 2")]);
    let d0 = [reading(0, 60.0, SCHOLAR, true, false), reading(1, 260.0, ROOK, true, false)];
    let p1 = page_text(vec![line(215.0, "Exercise 1"), line(760.0, "Exercise 2")]);
    let d1 = [reading(0, 60.0, ROOK, true, false)];
    let p2 = page_text(Vec::new());
    let d2 = [reading(0, 60.0, SCHOLAR, true, false)];
    let solutions = ["Exercise 1", SCHOLAR_MATE, "Exercise 2", ROOK_CHECKS, "Exercise 1", ROOK_CHECKS, "Exercise 2", SCHOLAR_MATE];
    let p3 = page_text(solutions.iter().enumerate().map(|(k, t)| line(40.0 + 20.0 * k as f32, t)).collect());
    let mut b = DocBuilder::new();
    b.add_page(&input(0, &p0, &d0, &[], &[]));
    b.add_page(&input(1, &p1, &d1, &[], &[]));
    b.add_page(&input(2, &p2, &d2, &[], &[]));
    b.add_page(&input(3, &p3, &[], &[], &[]));
    let doc = b.finish();
    assert_solved(&doc, &[3], &[(0, 0), (0, 1), (1, 0), (2, 0)]);
    let mut b = DocBuilder::new();
    for page in doc.pages.clone() {
        b.add_read_page(page);
    }
    assert_eq!(b.finish(), doc);
}

/// A workbook that numbers each chapter's exercises from 1 and prints all the solutions after
/// them: each solution is played from the exercise of its own chapter, found from the structure.
/// The two chapters set their positions the other way round, so a solution read from the other
/// chapter's exercise fails.
#[test]
fn solutions_after_every_chapter_link_to_their_own_chapters_exercises() {
    use WorkbookPage::{Exercises, Solutions};
    let chapters = |a: &'static str, b: &'static str| [Exercises(Some(a), [SCHOLAR, ROOK]), Exercises(Some(b), [ROOK, SCHOLAR])];
    // The label gives the chapter (`Ex. 2–1`); the solutions are printed last chapter first.
    let [c1, c2] = chapters("Chapter 1", "Chapter 2");
    let doc = workbook(&[c1, c2, Solutions(&["#Ex. 2–1", ROOK_CHECKS, "#Ex. 2–2", SCHOLAR_MATE, "#Ex. 1–1", SCHOLAR_MATE, "#Ex. 1–2", ROOK_CHECKS])]);
    assert_solved(&doc, &[2], &[(1, 0), (1, 1), (0, 0), (0, 1)]);
    // Each chapter's solutions under a heading that numbers it (in another language, or in
    // Roman numerals), last chapter first.
    let [c1, c2] = chapters("CAPÍTULO 1", "Capítulo 2 – Garfos");
    let doc = workbook(&[
        c1,
        c2,
        Solutions(&[
            "#Solutions",
            "#Solutions to Chapter II",
            "Exercise 1",
            ROOK_CHECKS,
            "Exercise 2",
            SCHOLAR_MATE,
            "#Chapter 1",
            "Exercise 1",
            SCHOLAR_MATE,
            "Exercise 2",
            ROOK_CHECKS,
        ]),
    ]);
    assert_solved(&doc, &[2], &[(1, 0), (1, 1), (0, 0), (0, 1)]);
    // Under a heading that names the chapter: its title, repeated.
    let [c1, c2] = chapters("Pins", "Forks");
    let doc = workbook(&[
        c1,
        c2,
        Solutions(&["#Solutions", "#Forks", "Exercise 1", ROOK_CHECKS, "Exercise 2", SCHOLAR_MATE, "#Pins", "Exercise 1", SCHOLAR_MATE]),
    ]);
    assert_solved(&doc, &[2], &[(1, 0), (1, 1), (0, 0)]);
    // In the chapters' order, with no heading to tell them apart: the solutions number again
    // for each chapter.
    let doc = workbook(&[
        Exercises(None, [SCHOLAR, ROOK]),
        Exercises(None, [ROOK, SCHOLAR]),
        Solutions(&["#Solutions", "Exercise 1", SCHOLAR_MATE, "Exercise 2", ROOK_CHECKS, "Exercise 1", ROOK_CHECKS, "Exercise 2", SCHOLAR_MATE]),
    ]);
    assert_solved(&doc, &[2], &[(0, 0), (0, 1), (1, 0), (1, 1)]);
}

/// Solutions printed after each chapter's own exercises still link to them, with or without
/// the chapter in their labels.
#[test]
fn solutions_after_their_own_chapter_link_to_it() {
    use WorkbookPage::{Exercises, Solutions};
    for labels in [["Exercise 1", "Exercise 2", "Exercise 1", "Exercise 2"], ["Ex. 1–1", "Ex. 1–2", "Ex. 2–1", "Ex. 2–2"]] {
        let doc = workbook(&[
            Exercises(Some("Chapter 1"), [SCHOLAR, ROOK]),
            Solutions(&[labels[0], SCHOLAR_MATE, labels[1], ROOK_CHECKS]),
            Exercises(Some("Chapter 2"), [ROOK, SCHOLAR]),
            Solutions(&[labels[2], ROOK_CHECKS, labels[3], SCHOLAR_MATE]),
        ]);
        assert_solved(&doc, &[1, 3], &[(0, 0), (0, 1), (2, 0), (2, 1)]);
    }
}

#[test]
fn chapter_headings() {
    use crate::doc::build::{chapter_number, heading_keys};
    use crate::doc::games::Key;
    for (text, want) in [
        ("Chapter 6", Some(6)),
        ("CAPÍTULO 7", Some(7)),
        ("Solutions to Chapter IV", Some(4)),
        ("Soluções do capítulo 12.", Some(12)),
        ("Kapitel XL: Endspiele", Some(40)),
        ("Chapter", None),
        ("Chapter one", None),
        ("Chapter 0", None),
        ("Chapter 1234", None),
        ("Chapter IIII", None),
        ("The chapter on pins", None),
        ("6 Pins", None),
    ] {
        assert_eq!(chapter_number(text), want, "{text}");
    }
    assert_eq!(heading_keys("Capítulo 2 – Garfos"), [Key::Chapter(2), Key::Title("capítulo 2 garfos".to_owned())]);
    assert!(heading_keys("Ex. 6–12").is_empty(), "a solution's label");
    assert!(heading_keys(&"Long ".repeat(40)).iter().all(|k| !matches!(k, Key::Title(_))));
}

/// A heading that names no puzzle still ends the game and lets go of the diagram waiting.
#[test]
fn a_heading_after_a_diagram_still_lets_it_go() {
    let rook = "8/8/4k3/8/8/4K3/r7/7R w - - 0 1";
    let p0 = page_text(vec![line(215.0, "Chapter 7"), line(240.0, "1.Rh6+ Kd5")]);
    let hints = [BlockHint { kind: HintKind::Heading { level: 1 }, rect: user_box(30.0, 210.0, 300.0, 230.0) }];
    let doc = build_page(&input(0, &p0, &[reading(0, 60.0, rook, true, false)], &hints, &[]));
    assert!(moves(&doc.pages[0]).iter().all(|(_, s)| *s != MoveStatus::Valid), "{:?}", moves(&doc.pages[0]));
}

/// A book's page number under a diagram at the foot of the page is not its caption: a bare
/// number alone at the head or foot of the page, as far from the page's index as the page
/// numbers of the pages before it, is the page's number. Another bare number there still numbers
/// the diagram, and so does that number with a line below it, or the page's number on a book's
/// first page read, with none before it.
#[test]
fn a_page_number_under_a_diagram_does_not_number_it() {
    let numbers = |doc: &ChessDoc| -> Vec<Option<u32>> {
        doc.pages.iter().flat_map(|p| p.blocks.iter()).filter_map(|b| if let Block::Diagram(d) = b { Some(d.puzzle) } else { None }).collect()
    };
    let p0 = page_text(vec![line(100.0, "Some prose."), line(780.0, "36")]);
    let at_foot = |n: &str| page_text(vec![line(100.0, "More prose."), line(780.0, n)]);
    let diagram = [reading(0, 620.0, SCHOLAR, true, false)];
    let not_at_foot = page_text(vec![line(100.0, "More prose."), line(780.0, "37"), line(795.0, "A note.")]);
    for (p1, want) in [(at_foot("37"), None), (at_foot("52"), Some(52)), (not_at_foot, Some(37))] {
        let mut b = DocBuilder::new();
        b.add_page(&input(0, &p0, &[], &[], &[]));
        b.add_page(&input(1, &p1, &diagram, &[], &[]));
        assert_eq!(numbers(&b.finish()), [want], "{:?}", p1.lines.len());
    }
    let p1 = at_foot("37");
    assert_eq!(numbers(&build_page(&input(1, &p1, &diagram, &[], &[]))), [Some(37)]);
}

/// A caption under the diagram gives its number too; one with no number gives none.
#[test]
fn a_caption_below_gives_the_puzzle_number() {
    let text = page_text(vec![line(215.0, "No. 7 White to play"), line(420.0, "A quiet position.")]);
    let diagrams = [reading(0, 60.0, SCHOLAR, true, false), reading(1, 260.0, SCHOLAR, true, false)];
    let doc = build_page(&input(0, &text, &diagrams, &[], &[]));
    let numbers: Vec<Option<u32>> =
        doc.pages[0].blocks.iter().filter_map(|b| if let Block::Diagram(d) = b { Some(d.puzzle) } else { None }).collect();
    assert_eq!(numbers, [Some(7), None]);
}

#[test]
fn node_references_follow_the_canonical_order() {
    // The variation is read before the main move it is an alternative to is written: nodes are
    // reordered when the game closes, and the inlines must still point at their own moves.
    let text = page_text(vec![line(230.0, "1.e4 e5 2.Nf3 Nc6 3.Bb5 . Instead 1...c5 2.Nf3 d6")]);
    let doc = build_page(&input(0, &text, &[], &[], &[]));
    for m in doc.pages[0].blocks.iter().flat_map(|b| b.inlines()).filter_map(|i| if let Inline::Move(m) = i { Some(m) } else { None }) {
        let r = m.game.unwrap();
        assert_eq!(doc.games[r.game].score.nodes[r.node].fen, *m.fen.as_ref().unwrap());
    }
    assert_eq!(doc.games[0].score.canonical_order(), (0..doc.games[0].score.nodes.len()).collect::<Vec<_>>());
}

#[test]
fn diagrams_interleave_by_column_and_keep_their_numbers() {
    // Two columns: left text at x 40, right column diagrams at x 320.
    let mut d0 = reading(0, 60.0, SCHOLAR, true, false);
    d0.bbox_user = user_box(320.0, 40.0, 470.0, 190.0);
    let mut d1 = reading(1, 60.0, SCHOLAR, true, false);
    d1.bbox_user = user_box(320.0, 400.0, 550.0, 550.0);
    let text = page_text(vec![
        line_at(40.0, 40.0, "left column one"),
        line_at(40.0, 400.0, "left column two"),
        line_at(320.0, 200.0, "under diagram one"),
        line_at(320.0, 560.0, "under diagram two"),
    ]);
    let doc = build_page(&input(0, &text, &[d0, d1], &[], &[]));
    assert_eq!(kinds(&doc.pages[0]), vec!["P:left column one", "P:left column two", "D0", "P:under diagram one", "D1", "P:under diagram two"]);
    // A diagram no line comes after goes last.
    let text = page_text(vec![line(20.0, "only text above")]);
    let doc = build_page(&input(0, &text, &[reading(0, 60.0, SCHOLAR, true, false)], &[], &[]));
    assert_eq!(kinds(&doc.pages[0]), vec!["P:only text above", "D0"]);
}

/// A two-column scan (Pinski): the left column's first row is cut at a word gap into two pieces,
/// the second right of the board below, which is narrower than its column. That piece is in the
/// board's column and above it: the board comes after both pieces, not between them.
#[test]
fn a_piece_of_a_row_beside_a_narrow_board_is_read_before_it() {
    let mut d0 = reading(0, 60.0, SCHOLAR, true, false);
    d0.bbox_user = user_box(60.0, 80.0, 180.0, 200.0);
    let mut lines = vec![line_at(40.0, 40.0, "first piece"), line_at(185.0, 40.0, "second")];
    for (k, y) in [60.0, 220.0, 240.0, 260.0].into_iter().enumerate() {
        lines.push(line_at(40.0, y, &format!("left column line {k} with more words")));
    }
    for (k, y) in [40.0, 60.0, 220.0, 240.0, 260.0].into_iter().enumerate() {
        lines.push(line_at(320.0, y, &format!("right column line {k}")));
    }
    let doc = build_page(&input(0, &page_text(lines.clone()), &[d0.clone()], &[], &[]));
    let cut = kinds(&doc.pages[0]);
    let at = |s: &str| cut.iter().position(|k| k.contains(s)).unwrap_or_else(|| panic!("{s} in {cut:?}"));
    assert!(at("second") < at("D0"), "{cut:?}");
    assert!(at("left column line 0") < at("D0") && at("D0") < at("left column line 1"), "{cut:?}");
    // Text beside the board, level with its top, with nothing of its row over the board, is the
    // board's text on the same page with columns: read after it.
    lines.retain(|l| l.rect.y0 != 40.0 || l.rect.x0 > 300.0);
    lines.push(line_at(185.0, 85.0, "beside"));
    let doc = build_page(&input(0, &page_text(lines), &[d0], &[], &[]));
    let beside = kinds(&doc.pages[0]);
    let at = |s: &str| beside.iter().position(|k| k.contains(s)).unwrap_or_else(|| panic!("{s} in {beside:?}"));
    assert!(at("D0") < at("beside"), "{beside:?}");
}

/// Chess Tactics' solutions: the board on the left, its solution beside it on the right, on a
/// page with no gutter. A short line right of the board, level with its top, is the board's text
/// and is read after it.
#[test]
fn a_solution_beside_its_board_is_read_after_it() {
    let mut d0 = reading(0, 60.0, SCHOLAR, true, false);
    d0.bbox_user = user_box(40.0, 60.0, 180.0, 200.0);
    let text = page_text(vec![line_at(40.0, 40.0, "Exercise solution"), line_at(200.0, 60.0, "Solution")]);
    let doc = build_page(&input(0, &text, &[d0], &[], &[]));
    assert_eq!(kinds(&doc.pages[0]), vec!["P:Exercise solution", "D0", "P:Solution"]);
}

/// Chess Tactics' page: the label and the board on the left, the solutions on the right, no
/// gutter. The text at the top right, well above the board, ends the previous solution and is
/// read before the board; the solution starting level with the label, just above the board, is
/// the board's and is read after it.
#[test]
fn text_well_above_a_board_beside_it_is_read_before_it() {
    let mut d0 = reading(0, 60.0, SCHOLAR, true, false);
    d0.bbox_user = user_box(40.0, 200.0, 140.0, 300.0);
    let text = page_text(vec![
        line_at(160.0, 60.0, "Previous"),
        line_at(40.0, 175.0, "Label"),
        line_at(160.0, 175.0, "Solution"),
        line_at(160.0, 230.0, "More"),
    ]);
    let doc = build_page(&input(0, &text, &[d0], &[], &[]));
    assert_eq!(kinds(&doc.pages[0]), vec!["P:Previous", "P:Label", "D0", "P:Solution", "P:More"]);
}

/// Two boards stacked in the left column, no gutter, a line right of them between the two: the
/// board above it is in another column, so it does not count as the line's own board; the line
/// is read after the first board and before the second, as it lies (Codex's case).
#[test]
fn a_line_between_two_stacked_boards_beside_them_is_read_between() {
    let board = |k: usize, top: f32| {
        let mut d = reading(k, 60.0, SCHOLAR, true, false);
        d.bbox_user = user_box(40.0, top, 140.0, top + 100.0);
        d
    };
    let text = page_text(vec![line_at(160.0, 250.0, "Between")]);
    let doc = build_page(&input(0, &text, &[board(0, 100.0), board(1, 300.0)], &[], &[]));
    assert_eq!(kinds(&doc.pages[0]), vec!["D0", "P:Between", "D1"]);
}

/// A Workbook's exercise page: four boards in a 2 × 2 grid, found column by column (top left,
/// bottom left, top right, bottom right; the blocks keep that order), a question under the
/// top-right board, and too few lines for a gutter. The question, right of the bottom-left board
/// and well above it, is under the top-right board in its own column: it is read after that
/// board, not before the bottom-left one.
#[test]
fn a_question_under_a_board_in_a_grid_waits_for_it() {
    let board = |k: usize, x: f32, top: f32| {
        let mut d = reading(k, 60.0, SCHOLAR, true, false);
        d.bbox_user = user_box(x, top, x + 180.0, top + 180.0);
        d
    };
    let boards = [board(0, 60.0, 60.0), board(1, 60.0, 340.0), board(2, 300.0, 60.0), board(3, 300.0, 340.0)];
    let text = page_text(vec![line_at(300.0, 260.0, "Question for the top right")]);
    let doc = build_page(&input(0, &text, &boards, &[], &[]));
    assert_eq!(kinds(&doc.pages[0]), vec!["D0", "D1", "D2", "P:Question for the top right", "D3"]);
}

/// A diagram centred on the page goes before a short line below it, even one that lies entirely
/// to its left (Fischer pt-BR p.47: `19 … ♗xd5` under a centred diagram).
#[test]
fn a_centred_diagram_goes_before_a_short_line_below_it() {
    let mut d0 = reading(0, 100.0, SCHOLAR, true, false);
    d0.bbox_user = user_box(200.0, 100.0, 400.0, 250.0);
    let text = page_text(vec![line(60.0, "text above the diagram"), line(270.0, "short"), line(290.0, "and on")]);
    let doc = build_page(&input(0, &text, &[d0], &[], &[]));
    assert_eq!(kinds(&doc.pages[0]), vec!["P:text above the diagram", "D0", "P:short and on"]);
}

/// On a two-column page, a diagram in the right column still goes after a left-column line below
/// it: the left column is read first.
#[test]
fn a_diagram_in_the_right_column_waits_for_the_left_one() {
    let mut d0 = reading(0, 100.0, SCHOLAR, true, false);
    d0.bbox_user = user_box(330.0, 100.0, 480.0, 250.0);
    let mut lines = Vec::new();
    for k in 0..6 {
        lines.push(line_at(30.0, 100.0 + 40.0 * k as f32, &format!("Left {k}, a fairly long line.")));
    }
    for k in 0..3 {
        lines.push(line_at(330.0, 270.0 + 40.0 * k as f32, &format!("Right {k}, a fairly long line.")));
    }
    let refs: Vec<&ChessLine> = lines.iter().collect();
    assert!(crate::doc::build::columns(&refs, 600.0).gutters().first().copied().is_some(), "two columns");
    let doc = build_page(&input(0, &page_text(lines), &[d0], &[], &[]));
    let kinds = kinds(&doc.pages[0]);
    let at = kinds.iter().position(|k| k == "D0").unwrap();
    assert!(kinds[..at].iter().any(|k| k.contains("Left 5")), "{kinds:?}");
    assert!(kinds[at + 1..].iter().all(|k| k.contains("Right")), "{kinds:?}");
}

/// A page with no gutter: a diagram set wholly on the right half of the page, and a short line
/// below it wholly on the left half (a move at the margin under a diagram set to the right). The
/// page is read top down, so the diagram comes before the line. On a page of short lines, which
/// may be two columns too short for a gutter, the line still comes first
/// (`diagrams_interleave_by_column_and_keep_their_numbers`).
#[test]
fn a_diagram_on_one_side_of_a_one_column_page_goes_before_a_line_below_it() {
    let mut d0 = reading(0, 100.0, SCHOLAR, true, false);
    d0.bbox_user = user_box(330.0, 100.0, 480.0, 250.0);
    let above = "A line of text set across the whole page, as the body is";
    let text = page_text(vec![line(40.0, above), line(60.0, above), line(270.0, "short"), line(290.0, "and on")]);
    let doc = build_page(&input(0, &text, &[d0.clone()], &[], &[]));
    assert_eq!(kinds(&doc.pages[0]), vec![format!("P:{above} {above}"), "D0".into(), "P:short and on".into()]);
    // The game goes on from the diagram, not from before it.
    let text = page_text(vec![line(40.0, above), line(60.0, "White to play, a line across the page here."), line(270.0, "3.Qh5 Nf6 4.Qxf7#")]);
    let doc = build_page(&input(0, &text, &[d0], &[], &[]));
    assert!(moves(&doc.pages[0]).iter().all(|(_, s)| *s == MoveStatus::Valid), "{:?}", moves(&doc.pages[0]));
}

/// Two boards side by side on a page with no gutter, a caption under the left one: the right
/// board does not jump before the left board's caption, which is under a board in its own column.
#[test]
fn a_caption_under_the_left_board_of_a_row_is_read_before_the_right_board() {
    let board = |k: usize, x: f32| {
        let mut d = reading(k, 60.0, SCHOLAR, true, false);
        d.bbox_user = user_box(x, 60.0, x + 180.0, 240.0);
        d
    };
    let text = page_text(vec![line_at(60.0, 250.0, "Left caption"), line_at(330.0, 250.0, "Right caption")]);
    let doc = build_page(&input(0, &text, &[board(0, 60.0), board(1, 330.0)], &[], &[]));
    assert_eq!(kinds(&doc.pages[0]), vec!["D0", "P:Left caption", "D1", "P:Right caption"]);
}

#[test]
fn text_reviews_go_to_the_word_they_cover() {
    let text = page_text(vec![line(100.0, "1.e4 e5 2.Nf3")]);
    let span = text.lines[0].spans[2].rect; // e5
    let flag = ReviewFlag::TextRepaired { from: "e8".into(), to: "e5".into(), why: "legality".into(), confidence: 0.92 };
    let reviews = vec![
        TextReview { rect: span, flag: flag.clone() },
        TextReview { rect: ViewRect::new(500.0, 700.0, 510.0, 710.0), flag: ReviewFlag::SideAssumed },
    ];
    let doc = build_page(&input(0, &text, &[], &[], &reviews));
    let e5 = &doc.pages[0].blocks[0].inlines()[1];
    assert_eq!(e5.review(), &[flag]);
    assert_eq!(doc.pages[0].review, vec![ReviewFlag::SideAssumed]);
}

#[test]
fn a_poor_layer_and_unknown_fonts_are_page_flags() {
    let mut text = page_text(vec![line(100.0, "words")]);
    text.quality = TextQuality::Poor { reason: crate::chess_text::PoorReason::UnresolvedChessCodes { glyphs: 4 } };
    text.unknown_chess_fonts = vec!["ChessAlpha".into()];
    let doc = build_page(&input(0, &text, &[], &[], &[]));
    assert_eq!(doc.pages[0].text_layer, TextLayer::Poor);
    assert_eq!(doc.pages[0].review[0], ReviewFlag::UnknownChessFont { font: "ChessAlpha".into() });
    assert!(matches!(&doc.pages[0].review[1], ReviewFlag::PoorTextLayer { .. }));
}

#[test]
fn deep_brackets_and_long_comments_are_bounded() {
    let deep = format!("1.e4 e5 {} 2.Nf3 {}", "( ".repeat(500), ") ".repeat(500));
    let long = format!("1.e4 {}", "word ".repeat(2000));
    let text = page_text(vec![line(100.0, &deep), line(112.0, &long)]);
    let doc = build_page(&input(0, &text, &[], &[], &[]));
    for g in &doc.games {
        assert!(g.score.nodes.iter().all(|n| n.comment.chars().count() <= games::MAX_COMMENT_CHARS));
    }
    assert!(doc.games.iter().any(|g| g.flags.iter().any(|f| f.flag == ReviewFlag::CommentTruncated)));
    let scores: Vec<GameScore> = doc.games.iter().map(|g| g.score.clone()).collect();
    assert_eq!(pgn::parse(&doc.pgn()).unwrap(), scores);
}

/// Tokens a generated page is made of.
const VOCAB: &[&str] = &[
    "1.e4",
    "e5",
    "2.Nf3",
    "Nc6",
    "3.Bb5",
    "a6",
    "1...c5",
    "d6",
    "Nf6",
    "O-O",
    "Qxf7#",
    "4.Bd7",
    "(",
    ")",
    "[",
    "]",
    "!",
    "±",
    "1-0",
    "½-½",
    "and",
    "then",
    "12.Kh9",
    "e8=Q",
    "Kd7",
    "1.",
    "2...",
    "www.x.org",
    "Rd8+",
    "Bxe5",
];

fn arb_line() -> impl Strategy<Value = (f32, f32, Vec<usize>)> {
    (0.0f32..600.0, -50.0f32..900.0, proptest::collection::vec(0..VOCAB.len(), 0..30))
}

fn arb_diagram() -> impl Strategy<Value = (usize, f32, f32, bool, bool, u8)> {
    (0usize..3, -100.0f32..900.0, 0.0f32..600.0, any::<bool>(), any::<bool>(), 0u8..4)
}

const FENS: &[&str] =
    &[SCHOLAR, "8/8/4k3/8/8/4K3/r7/7R w - - 0 1", "8/8/8/8/8/8/8/8 w - - 0 1", "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"];

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    #[test]
    fn arbitrary_pages_never_panic_and_their_games_round_trip(
        lines in proptest::collection::vec(arb_line(), 0..12),
        diagrams in proptest::collection::vec(arb_diagram(), 0..4),
        hint in proptest::option::of((-100.0f32..700.0, -100.0f32..900.0)),
        pages in 1u32..3,
    ) {
        let text = page_text(lines.iter().map(|(x, y, toks)| {
            let words: Vec<&str> = toks.iter().map(|&t| VOCAB[t]).collect();
            line_at(*x, *y, &words.join(" "))
        }).collect());
        let readings: Vec<BoardReading> = diagrams.iter().enumerate().map(|(i, (f, top, x, accepted, assumed, kind))| {
            let mut r = reading(i, *top, FENS[*f % FENS.len()], *accepted, *assumed);
            r.bbox_user = user_box(*x, *top, *x + 150.0, *top + 150.0);
            match kind {
                0 => r.outcome = Err(Unreadable::BadGeometry),
                1 => r.bbox_user = PageRect { x0: f32::NAN, y0: 0.0, x1: f32::INFINITY, y1: 1.0 },
                _ => {}
            }
            r
        }).collect();
        let hints: Vec<BlockHint> = hint.iter().map(|(x, y)| BlockHint { kind: HintKind::Heading { level: 2 }, rect: user_box(*x, *y, *x + 200.0, *y + 30.0) }).collect();
        let mut b = DocBuilder::new();
        for p in 0..pages {
            b.add_page(&input(p, &text, &readings, &hints, &[]));
        }
        let doc = b.finish();
        // Every diagram, in order, exactly once per page.
        for page in &doc.pages {
            let numbers: Vec<usize> = page.blocks.iter().filter_map(|b| if let Block::Diagram(d) = b { Some(d.number) } else { None }).collect();
            prop_assert_eq!(numbers, (0..readings.len()).collect::<Vec<_>>());
            for m in page.blocks.iter().flat_map(|b| b.inlines()).filter_map(|i| if let Inline::Move(m) = i { Some(m) } else { None }) {
                if let Some(r) = m.game {
                    let node = doc.games.get(r.game).and_then(|g| g.score.nodes.get(r.node));
                    prop_assert_eq!(node.map(|n| &n.fen), m.fen.as_ref());
                }
            }
        }
        let json = doc.to_json().unwrap();
        prop_assert_eq!(ChessDoc::from_json(&json).unwrap(), doc.clone());
        let scores: Vec<GameScore> = doc.games.iter().map(|g| g.score.clone()).collect();
        prop_assert_eq!(pgn::parse(&doc.pgn()), Ok(scores));
    }

    #[test]
    fn columns_are_capped_and_their_order_keeps_every_line(
        lines in proptest::collection::vec((-50.0f32..2050.0, -50.0f32..900.0, 1usize..60), 0..80),
        width in prop_oneof![Just(600.0f32), Just(2000.0f32), Just(f32::NAN), Just(0.0f32)],
    ) {
        let lines: Vec<ChessLine> = lines.iter().map(|(x, y, n)| line_at(*x, *y, &"w".repeat(*n))).collect();
        let refs: Vec<&ChessLine> = lines.iter().collect();
        let columns = crate::doc::build::columns(&refs, width);
        let gutters = columns.gutters();
        prop_assert!(gutters.len() < crate::doc::build::MAX_COLUMNS);
        prop_assert!(gutters.windows(2).all(|w| w[0] < w[1]), "{:?}", gutters);
        let order = crate::doc::build::column_order(&refs, &columns);
        let mut got: Vec<*const ChessLine> = order.iter().map(|l| std::ptr::from_ref::<ChessLine>(l)).collect();
        let mut want: Vec<*const ChessLine> = refs.iter().map(|l| std::ptr::from_ref::<ChessLine>(l)).collect();
        got.sort();
        want.sort();
        prop_assert_eq!(got, want);
    }

    #[test]
    fn hostile_json_never_panics(text in "\\PC{0,300}") {
        if let Ok(doc) = ChessDoc::from_json(&text) {
            let _ = doc.pgn();
        }
    }
}

#[test]
fn a_hostile_doc_writes_pgn_without_panicking() {
    let json = r#"{"format":1,"pages":[],"games":[{"origin":{"kind":"initial"},"trusted":true,"page":0,
        "score":{"roster":{"event":"?","site":"?","date":"?","round":"?","white":"?","black":"?"},
        "start_fen":"not a fen","nodes":[{"san":"e4 }{","ply":4294967295,"fen":"x","parent":5,"children":[0,0,9]}],
        "roots":[0,3],"result":"*"}}]}"#;
    let doc = ChessDoc::from_json(json).unwrap();
    assert!(doc.pgn().contains("e4"));
}

/// A game in long algebraic replays like any other.
#[test]
fn a_long_algebraic_game_replays() {
    let text = page_text(vec![line(100.0, "1.e2-e4 e7-e5 2.Ng1-f3 Nb8-c6 3.Bf1-b5 a7-a6 4.Bb5:c6 d7:c6")]);
    let doc = build_page(&input(0, &text, &[], &[], &[]));
    let got = moves(&doc.pages[0]);
    assert_eq!(got.len(), 8, "{got:?}");
    assert!(got.iter().all(|(_, s)| *s == MoveStatus::Valid), "{got:?}");
    let pgn = doc.pgn();
    assert!(pgn.contains("1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 4. Bxc6 dxc6"), "written back in short algebraic: {pgn}");
}

/// A two-column page whose lines come interleaved (the layout reads across): each column's game
/// replays in its own order.
#[test]
fn a_two_column_page_is_read_column_by_column() {
    let left = ["The open game in this column.", "1.e4 e5 2.Nf3 Nc6 a classical start", "3.Bb5 a6 and White keeps the bishop."];
    let right = ["The closed game in this column.", "1.d4 d5 2.c4 e6 is the Queen's Gambit", "3.Nc3 Nf6 and Black develops calmly."];
    let mut lines = Vec::new();
    for (k, (l, r)) in left.iter().zip(&right).enumerate() {
        let y = 100.0 + 20.0 * k as f32;
        lines.push(line_at(30.0, y, l));
        lines.push(line_at(320.0, y, r));
    }
    let doc = build_page(&input(0, &page_text(lines), &[], &[], &[]));
    use MoveStatus::*;
    let got = moves(&doc.pages[0]);
    assert_eq!(
        got.iter().map(|(m, _)| m.as_str()).collect::<Vec<_>>(),
        ["1.e4", "e5", "2.Nf3", "Nc6", "3.Bb5", "a6", "1.d4", "d5", "2.c4", "e6", "3.Nc3", "Nf6"],
        "left column first, then the right"
    );
    assert!(got.iter().all(|(_, s)| *s == Valid), "{got:?}");
    assert_eq!(doc.games.len(), 2);
}

/// A page with one column keeps its order, whatever the line widths; full-width lines cut the
/// columns into bands read in turn.
/// A two-column page whose right column holds a game score's table (`2 f4 ef`, cut at its wide
/// gaps into narrow pieces) beside its prose: the pieces bring the column's average width down,
/// but its prose lines still make it a column.
#[test]
fn a_column_with_a_score_table_is_still_a_column() {
    let prose = "the text runs on in its column here";
    let mut lines = Vec::new();
    for k in 0..8 {
        lines.push(line_at(30.0, 100.0 + 20.0 * k as f32, prose));
    }
    for k in 0..5 {
        lines.push(line_at(330.0, 100.0 + 20.0 * k as f32, prose));
    }
    for k in 0..6 {
        let y = 200.0 + 20.0 * k as f32;
        lines.push(line_at(330.0, y, &format!("{}.", k + 2)));
        lines.push(line_at(380.0, y, "f4"));
        lines.push(line_at(460.0, y, "ef"));
    }
    let refs: Vec<&ChessLine> = lines.iter().collect();
    let x = crate::doc::build::columns(&refs, 600.0).gutters().first().copied().expect("a gutter between the columns");
    assert!((290.0..330.0).contains(&x), "{x}");
    let order = crate::doc::build::column_order(&refs, &crate::doc::build::columns(&refs, 600.0));
    assert!(order[..8].iter().all(|l| l.rect.x0 < 100.0), "the left column first");
}

#[test]
fn columns_need_a_real_gutter_and_full_width_lines_cut_bands() {
    let one = [line_at(30.0, 100.0, "1.e4 e5 2.Nf3 Nc6"), line_at(30.0, 120.0, "3.Bb5 a6 4.Ba4 Nf6"), line_at(30.0, 140.0, "5.O-O Be7")];
    let refs: Vec<&ChessLine> = one.iter().collect();
    assert!(crate::doc::build::columns(&refs, 600.0).gutters().first().copied().is_none(), "one column, all on the left");
    // Band 1: two columns; a full-width line; band 2: two columns.
    let rows = [
        (100.0, "Left one, a fairly long line.", "Right one, a fairly long line."),
        (120.0, "Left two, a fairly long line.", "Right two, a fairly long line."),
        (140.0, "Left three, a fairly long line.", "Right three, a fairly long line."),
    ];
    let mut lines = Vec::new();
    for (y, l, r) in rows {
        lines.push(line_at(30.0, y, l));
        lines.push(line_at(320.0, y, r));
    }
    lines.push(line_at(30.0, 170.0, "A heading set across the whole page, from one margin to the other one"));
    for (y, l, r) in rows {
        lines.push(line_at(30.0, y + 100.0, &l.replace("one", "four").replace("two", "five").replace("three", "six")));
        lines.push(line_at(320.0, y + 100.0, &r.replace("one", "four").replace("two", "five").replace("three", "six")));
    }
    let refs: Vec<&ChessLine> = lines.iter().collect();
    let order: Vec<String> = crate::doc::build::column_order(&refs, &crate::doc::build::columns(&refs, 600.0))
        .iter()
        .map(|l| {
            l.spans
                .iter()
                .map(|s| {
                    if let ChessRun::Text(t) = &s.run {
                        t.split(' ').next().unwrap_or("").to_owned() + &t.split(' ').nth(1).unwrap_or("").replace(',', "")
                    } else {
                        String::new()
                    }
                })
                .collect()
        })
        .collect();
    assert_eq!(
        order,
        [
            "Leftone",
            "Lefttwo",
            "Leftthree",
            "Rightone",
            "Righttwo",
            "Rightthree",
            "Aheading",
            "Leftfour",
            "Leftfive",
            "Leftsix",
            "Rightfour",
            "Rightfive",
            "Rightsix"
        ]
    );
}

/// The first word of each line, for checking an order.
fn first_words(lines: &[&ChessLine]) -> Vec<String> {
    lines
        .iter()
        .map(|l| match l.spans.first().map(|s| &s.run) {
            Some(ChessRun::Text(t)) => t.split(' ').next().unwrap_or("").to_owned(),
            Some(ChessRun::MoveNumber { number, .. }) => format!("{number}."),
            _ => String::new(),
        })
        .collect()
}

/// A three-column page whose lines come interleaved (the layout reads across): each column is
/// read down in turn, left to right, and each column's game replays in its own order.
#[test]
fn a_three_column_page_is_read_column_by_column() {
    let cols = [
        ["The open game here.", "1.e4 e5 2.Nf3 Nc6", "3.Bb5 a6 and so on."],
        ["The closed game here.", "1.d4 d5 2.c4 e6", "3.Nc3 Nf6 is calm."],
        ["The flank game here.", "1.c4 e5 2.Nc3 Nf6", "3.g3 d5 is sharp."],
    ];
    let mut lines = Vec::new();
    for row in 0..3 {
        for (c, col) in cols.iter().enumerate() {
            lines.push(line_at(30.0 + 190.0 * c as f32, 100.0 + 20.0 * row as f32, col[row]));
        }
    }
    let refs: Vec<&ChessLine> = lines.iter().collect();
    let columns = crate::doc::build::columns(&refs, 600.0);
    assert_eq!(columns.gutters().len(), 2, "{columns:?}");
    let doc = build_page(&input(0, &page_text(lines), &[], &[], &[]));
    use MoveStatus::*;
    let got = moves(&doc.pages[0]);
    assert_eq!(
        got.iter().map(|(m, _)| m.as_str()).collect::<Vec<_>>(),
        ["1.e4", "e5", "2.Nf3", "Nc6", "3.Bb5", "a6", "1.d4", "d5", "2.c4", "e6", "3.Nc3", "Nf6", "1.c4", "e5", "2.Nc3", "Nf6", "3.g3", "d5"],
        "left column first, then the middle, then the right"
    );
    assert!(got.iter().all(|(_, s)| *s == Valid), "{got:?}");
    assert_eq!(doc.games.len(), 3);
}

/// On a three-column page, a line across the whole page cuts every column into bands, and a line
/// across the two right columns only cuts those two: the left column reads on past it.
#[test]
fn lines_across_some_of_three_columns_cut_only_those_into_bands() {
    let mut lines = Vec::new();
    for (k, y) in [100.0, 120.0, 140.0, 200.0, 220.0, 240.0].into_iter().enumerate() {
        lines.push(line_at(30.0, y, &format!("L{k} in the left column")));
        lines.push(line_at(220.0, y, &format!("M{k} in the mid column")));
        lines.push(line_at(410.0, y, &format!("R{k} in the right col")));
    }
    // Across the middle and right columns, between their two bands.
    lines.push(line_at(220.0, 170.0, "Across the middle and the right columns here"));
    // Across the whole page, under everything.
    lines.push(line_at(30.0, 300.0, "Across the whole page from one margin to the other margin of it"));
    for (k, y) in [320.0, 340.0, 360.0].into_iter().enumerate() {
        lines.push(line_at(30.0, y, &format!("l{k} in the left column")));
        lines.push(line_at(220.0, y, &format!("m{k} in the mid column")));
        lines.push(line_at(410.0, y, &format!("r{k} in the right col")));
    }
    let refs: Vec<&ChessLine> = lines.iter().collect();
    let columns = crate::doc::build::columns(&refs, 600.0);
    assert_eq!(columns.gutters().len(), 2, "{columns:?}");
    let order = first_words(&crate::doc::build::column_order(&refs, &columns));
    assert_eq!(
        order,
        [
            "L0", "L1", "L2", "L3", "L4", "L5", "M0", "M1", "M2", "R0", "R1", "R2", "Across", "M3", "M4", "M5", "R3", "R4", "R5", "Across", "l0",
            "l1", "l2", "m0", "m1", "m2", "r0", "r1", "r2"
        ]
    );
}

/// A diagram in the middle column of three goes after the left column's lines below it and before
/// the middle column's lines under it; the right column's lines come after.
#[test]
fn a_diagram_in_the_middle_of_three_columns_waits_for_the_left_one() {
    let mut d0 = reading(0, 100.0, SCHOLAR, true, false);
    d0.bbox_user = user_box(220.0, 100.0, 370.0, 250.0);
    let mut lines = Vec::new();
    for k in 0..8 {
        let y = 100.0 + 25.0 * k as f32;
        lines.push(line_at(30.0, y, &format!("Left {k}, a line in it.")));
        lines.push(line_at(410.0, y, &format!("Right {k}, a line in it.")));
    }
    for k in 0..3 {
        lines.push(line_at(220.0, 270.0 + 25.0 * k as f32, &format!("Mid {k}, a line in it.")));
    }
    let doc = build_page(&input(0, &page_text(lines), &[d0], &[], &[]));
    let kinds = kinds(&doc.pages[0]);
    let at = |s: &str| kinds.iter().position(|k| k.contains(s)).unwrap_or_else(|| panic!("{s} in {kinds:?}"));
    assert!(at("Left 7") < at("D0") && at("D0") < at("Mid 0") && at("Mid 2") < at("Right 0"), "{kinds:?}");
}

/// The coordinates printed around a diagram are left out; the same letters elsewhere stay.
#[test]
fn a_diagrams_printed_coordinates_are_not_text() {
    let text = page_text(vec![
        line_at(40.0, 212.0, "a b c d e f g h"),
        line_at(28.0, 62.0, "8"),
        line(230.0, "3.Qh5 Nf6 4.Qxf7#"),
        line_at(300.0, 600.0, "a b c"),
    ]);
    let doc = build_page(&input(0, &text, &[reading(0, 60.0, SCHOLAR, true, false)], &[], &[]));
    let k = kinds(&doc.pages[0]);
    assert_eq!(k, ["D0", "P:3.Qh5 Nf6 4.Qxf7#", "P:a b c"], "{k:?}");
}

/// A diagram with White to move whose text first gives the alternative to Black's move before it
/// (Catastrophes & Tactics: `A dreadful blunder.`, then `the correct way: 7...d3`, then the bold
/// game `8.Nxe4 fxe4`). That move is Black's and cannot be played from the diagram, but its
/// number still says which White move comes next: the bold line is played from the diagram.
#[test]
fn a_move_before_the_diagram_numbers_the_moves_after_it() {
    let text = page_text(vec![line(230.0, "Better was 2...Nf6 3.d3 Bc5 with equality."), line(250.0, "*3.Qh5 *Nf6"), line(270.0, "*4.Qxf7#")]);
    let diagrams = [reading(0, 60.0, SCHOLAR, true, false)];
    let doc = build_page(&input(0, &text, &diagrams, &[], &[]));
    let statuses: Vec<MoveStatus> = moves(&doc.pages[0]).into_iter().map(|(_, s)| s).collect();
    assert_eq!(statuses[0], MoveStatus::Illegal);
    assert!(statuses[3..].iter().all(|s| *s == MoveStatus::Valid), "{:?}", moves(&doc.pages[0]));
    assert_eq!(mainline(&doc), ["Qh5", "Nf6", "Qxf7#"]);
    assert_eq!(doc.games[0].score.start_fen.as_deref(), Some("r1bqkbnr/pppp1ppp/2n5/4p3/2B1P3/8/PPPP1PPP/RNBQK1NR w KQkq - 2 3"));
}

/// On a page read by OCR, an illegal move that one misread file letter explains is played as that
/// reading, and the move says so (a scan loses the bar of an `e`: `cxd4` for `exd4`); a text
/// layer's move stays illegal, and so does one whose reading two moves fit.
#[test]
fn a_misread_file_letter_is_read_again_on_a_page_read_by_ocr() {
    fn played(doc: &ChessDoc) -> Vec<&MoveInline> {
        doc.pages
            .iter()
            .flat_map(|p| &p.blocks)
            .flat_map(|b| b.inlines())
            .filter_map(|i| if let Inline::Move(m) = i { Some(m) } else { None })
            .collect()
    }
    let text = page_text(vec![line(230.0, "1.e4 e5 2.d4 cxd4+ 3.Qxd4 Nc6 4.Qe3")]);
    let read = |ocr: bool| build_page(&PageInput { ocr, ..input(0, &text, &[], &[], &[]) });
    let doc = read(true);
    assert!(moves(&doc.pages[0]).iter().all(|(_, s)| *s == MoveStatus::Valid), "{:?}", moves(&doc.pages[0]));
    assert_eq!(mainline(&doc), ["e4", "e5", "d4", "exd4", "Qxd4", "Nc6", "Qe3"]);
    let flags: Vec<(&str, &ReviewFlag)> = played(&doc).into_iter().flat_map(|m| m.review.iter().map(|f| (m.san.as_str(), f))).collect();
    assert!(matches!(flags.as_slice(), [("exd4", ReviewFlag::TextRepaired { from, to, .. })] if from == "cxd4+" && to == "exd4"), "{flags:?}");
    assert!(doc.pages[0].ocr);
    // Judged again from its JSON, the move keeps its reading and its flag.
    let mut builder = DocBuilder::new();
    for page in doc.pages.clone() {
        builder.add_read_page(page);
    }
    assert_eq!(builder.finish(), doc);
    // The book's own text layer printed what it meant.
    let layer = read(false);
    let statuses: Vec<MoveStatus> = moves(&layer.pages[0]).into_iter().map(|(_, s)| s).collect();
    assert_eq!(statuses[3], MoveStatus::Illegal);
    assert!(played(&layer).iter().all(|m| m.review.is_empty()));
    // `Ne4` is the only reading, but both knights reach e4.
    let text = page_text(vec![line(230.0, "1.Nc3 a6 2.Nf3 a5 3.Ng5 a4 4.Nc4")]);
    let doc = build_page(&PageInput { ocr: true, ..input(0, &text, &[], &[], &[]) });
    assert_eq!(moves(&doc.pages[0]).last().map(|(_, s)| *s), Some(MoveStatus::Illegal));
    // Black's queens on e7 and c1: `Qee5` and `Qcc5` are both legal readings of `Qce5`.
    let text = page_text(vec![line(230.0, "40...Qce5")]);
    let diagrams = [reading(0, 60.0, "5br1/3kqpp1/1p6/1b1P3p/1P1P3P/5N1R/5K2/r1q4R b - - 4 40", true, false)];
    let doc = build_page(&PageInput { ocr: true, ..input(0, &text, &diagrams, &[], &[]) });
    assert_eq!(moves(&doc.pages[0]), [("40...Qce5".to_owned(), MoveStatus::Illegal)]);
}
