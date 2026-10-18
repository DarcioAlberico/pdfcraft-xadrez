use pdfcraft_chess::fen::parse_placement;
use pdfcraft_chess::nag;
use pdfcraft_chess::placement::{Color, Piece, PieceKind, Square};
use pdfcraft_chess::recognize::decode::Change;
use pdfcraft_chess::recognize::side::{SideSource, SideToMove};
use pdfcraft_chess::rules::check_position;
use pdfcraft_chess::san::{Lang, San};
use proptest::prelude::*;

mod html;

use super::*;
use crate::board::Unreadable;
use crate::chess_text::{ChessLine, ChessRun, ChessSpan, LangGuess, LangTally};
use crate::coords::Rotation;

const PAGE_H: f32 = 800.0;

fn geometry() -> PageGeometry {
    PageGeometry::new(PageRect::new(0.0, 0.0, 600.0, PAGE_H), Rotation::R0).unwrap()
}

/// Runs from space-separated tokens: `12.` / `12...` prefixes are move numbers, readable SAN is a
/// move, a NAG glyph is a NAG, the rest is prose (joined, as `chess_text` joins it).
fn runs(text: &str) -> Vec<ChessRun> {
    let mut out: Vec<ChessRun> = Vec::new();
    for tok in text.split(' ').filter(|t| !t.is_empty()) {
        let digits = tok.chars().take_while(char::is_ascii_digit).count();
        let mut rest = tok;
        if digits > 0 && tok[digits..].starts_with('.') {
            let dots = tok[digits..].chars().take_while(|c| *c == '.').count();
            out.push(ChessRun::MoveNumber { number: tok[..digits].parse().unwrap(), black: dots >= 3 });
            rest = &tok[digits + dots..];
            if rest.is_empty() {
                continue;
            }
        }
        let starts_like_move = rest.starts_with(|c: char| ('a'..='h').contains(&c) || "KQRBNO".contains(c));
        if starts_like_move && let Ok(san) = San::read(rest, Lang::En) {
            out.push(ChessRun::Move { san, text: rest.to_owned() });
        } else if let Some(n) = nag::by_glyph(rest) {
            out.push(ChessRun::Nag(n.code));
        } else if let Some(ChessRun::Text(prev)) = out.last_mut() {
            prev.push(' ');
            prev.push_str(rest);
        } else {
            out.push(ChessRun::Text(rest.to_owned()));
        }
    }
    out
}

/// A line of runs whose top is at view `y`, from `x`, 7 pt a character, 12 pt high.
fn line_at(x: f32, y: f32, text: &str) -> ChessLine {
    let mut cx = x;
    let spans: Vec<ChessSpan> = runs(text)
        .into_iter()
        .map(|run| {
            let chars = match &run {
                ChessRun::Text(s) => s.chars().count(),
                ChessRun::Move { text, .. } => text.chars().count(),
                ChessRun::MoveNumber { .. } => 3,
                _ => 1,
            } as f32;
            let rect = ViewRect::new(cx, y, cx + 7.0 * chars, y + 12.0);
            cx += 7.0 * chars + 4.0;
            ChessSpan { run, rect }
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
    PageInput { index, geometry: geometry(), text, diagrams, hints, reviews }
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
