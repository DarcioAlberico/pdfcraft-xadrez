use cozy_chess::Board;
use pdfcraft_chess::game::{Game, NodeId, PgnReport, PgnWarningKind, split_pgn};
use pdfcraft_chess::san::{Lang, San};
use proptest::prelude::*;

use super::*;
use crate::doc::games::{GameResult, GameScore, Node, Tag};

fn san(s: &str) -> San {
    San::read(s, Lang::En).unwrap()
}

/// Play a line of English SAN after `at`, returning the last node.
fn line(g: &mut GameScore, at: Option<usize>, moves: &str) -> Option<usize> {
    let mut at = at;
    for m in moves.split_whitespace() {
        at = Some(g.play(at, &san(m), false).unwrap());
    }
    at
}

/// Every move of the board's tree in PGN order, with its depth, glyphs and both comments, the
/// start and its comment: what reading the PGN back must give.
pub(crate) fn tree(g: &Game) -> Vec<String> {
    tree_with(g, str::to_owned)
}

/// [`tree`] with the comments as the exports write them, figurines as letters.
pub(crate) fn tree_in_letters(g: &Game) -> Vec<String> {
    tree_with(g, pdfcraft_chess::pgn::comment_letters)
}

fn tree_with(g: &Game, comment: fn(&str) -> String) -> Vec<String> {
    fn walk(g: &Game, id: NodeId, depth: usize, comment: fn(&str) -> String, out: &mut Vec<String>) {
        for (i, &child) in g.replies(id).iter().enumerate() {
            let d = if i == 0 { depth } else { depth + 1 };
            out.push(format!("{d} {} {:?} [{}] [{}]", g.san(child), g.nags(child), comment(g.comment_before(child)), comment(g.comment(child))));
            walk(g, child, d, comment, out);
        }
    }
    let mut out = vec![format!("start {} [{}]", g.start_fen(), comment(g.comment(0)))];
    walk(g, 0, 0, comment, &mut out);
    out
}

/// The games of a PGN as the board reads them, each read whole (nothing stopped, nothing noted).
pub(crate) fn read_games(pgn: &str) -> Vec<Game> {
    split_pgn(pgn, 100_000)
        .into_iter()
        .map(|text| {
            let (game, report) = Game::from_pgn_report(text).unwrap();
            assert_eq!(report, PgnReport::default(), "{text}");
            game
        })
        .collect()
}

/// Plan A9-F4 (A2-18): the round trip proven with the board's reader. The document's PGN reads
/// back, game by game, as the game [`to_game`] makes of each score (every move, glyph, comment,
/// comment before and variation in its place), and writes the same text again.
pub(crate) fn assert_reads_back(scores: &[GameScore], pgn: &str) {
    let games = read_games(pgn);
    assert_eq!(games.len(), scores.len(), "{pgn}");
    let options = WriteOptions { letters: true, ..WriteOptions::default() };
    let mut written = Vec::new();
    for (score, back) in scores.iter().zip(&games) {
        let board = to_game(score).unwrap();
        assert_eq!(tree(back), tree_in_letters(&board.game), "{pgn}");
        written.push(pdfcraft_chess::pgn::write(back, options));
    }
    assert_eq!(written.join("\n"), pgn);
}

/// A document's score made from a PGN game read by the board, for tests that need one: the
/// start, the tags, every move with its glyphs and comments, the variations in their order.
pub(crate) fn score_of(text: &str) -> GameScore {
    let (g, report) = Game::from_pgn_report(text).unwrap();
    assert_eq!(report.stopped, None, "{text}");
    let start = (g.start_fen() != Board::default().to_string()).then(|| g.start_fen());
    let mut s = GameScore::new(start);
    for (name, value) in g.tags() {
        match name.as_str() {
            "Event" => s.roster.event = value.clone(),
            "Site" => s.roster.site = value.clone(),
            "Date" => s.roster.date = value.clone(),
            "Round" => s.roster.round = value.clone(),
            "White" => s.roster.white = value.clone(),
            "Black" => s.roster.black = value.clone(),
            "Result" => s.result = GameResult::from_pgn(value).unwrap_or(GameResult::Unknown),
            _ => s.extra_tags.push(Tag { name: name.clone(), value: value.clone() }),
        }
    }
    s.comment = g.comment(0).to_owned();
    let mut todo = vec![(0, None)];
    while let Some((id, at)) = todo.pop() {
        for &child in g.replies(id) {
            let n = s.play(at, &San::parse(g.san(child)).unwrap(), false).unwrap();
            s.nodes[n].nags = g.nags(child).to_vec();
            s.nodes[n].comment = g.comment(child).to_owned();
            s.nodes[n].comment_before = g.comment_before(child).to_owned();
            todo.push((child, Some(n)));
        }
    }
    s.reorder_canonical();
    s
}

/// The Opera game's opening with a variation, NAGs and comments.
fn opera() -> GameScore {
    let mut g = GameScore::new(None);
    g.roster.white = "Morphy, Paul".to_owned();
    g.roster.black = "Duke Karl \"and\" Count Isouard".to_owned();
    g.roster.site = "Paris".to_owned();
    g.extra_tags.push(Tag { name: "PdfPage".to_owned(), value: "3".to_owned() });
    g.comment = "The Opera game.".to_owned();
    let e4 = line(&mut g, None, "e4");
    let e5 = line(&mut g, e4, "e5");
    let d6 = line(&mut g, e5, "Nf3 d6");
    // 1...c5 as an alternative to 1...e5.
    let c5 = line(&mut g, e4, "c5");
    let c5 = c5.unwrap();
    g.comment_before(c5, "or the Sicilian");
    line(&mut g, Some(c5), "Nf3");
    let bg4 = line(&mut g, d6, "d4 Bg4").unwrap();
    g.nodes[bg4].nags.push(6);
    g.nodes[bg4].comment = "A weak move.".to_owned();
    line(&mut g, Some(bg4), "dxe5 Bxf3 Qxf3");
    g.result = GameResult::WhiteWins;
    g.reorder_canonical();
    g
}

#[test]
fn a_game_is_written_in_export_format() {
    let pgn = write_game(&opera());
    let expected = "[Event \"?\"]\n[Site \"Paris\"]\n[Date \"????.??.??\"]\n[Round \"?\"]\n[White \"Morphy, Paul\"]\n\
[Black \"Duke Karl \\\"and\\\" Count Isouard\"]\n[Result \"1-0\"]\n[PdfPage \"3\"]\n\n\
{The Opera game.} 1. e4 e5 ({or the Sicilian} 1... c5 2. Nf3) 2. Nf3 d6 3. d4\n\
Bg4 $6 {A weak move.} 4. dxe5 Bxf3 5. Qxf3 1-0\n";
    // The comment before 1...c5 starts the variation; lines stay within 79 columns.
    assert!(pgn.lines().all(|l| l.chars().count() <= 79), "{pgn}");
    assert_eq!(pgn, expected);
}

#[test]
fn a_written_game_reads_back_equal() {
    let g = opera();
    assert_reads_back(std::slice::from_ref(&g), &write_game(&g));
}

/// A1-06/A2-02 (plan A9-F4): a comment printed after a move's variations is the next move's
/// comment-before, written there, and read back there by the board's reader, in both writers.
#[test]
fn a_comment_after_the_variations_stays_there() {
    let mut g = GameScore::new(None);
    let e4 = line(&mut g, None, "e4").unwrap();
    g.nodes[e4].comment = "A".to_owned();
    let e5 = line(&mut g, Some(e4), "e5").unwrap();
    line(&mut g, None, "d4 d5");
    g.put_before(e5, "B");
    g.reorder_canonical();
    let pgn = write_game(&g);
    assert!(pgn.ends_with("\n\n1. e4 {A} (1. d4 d5) {B} 1... e5 *\n"), "{pgn}");
    assert_reads_back(&[g], &pgn);
}

#[test]
fn a_game_from_a_position_carries_setup_and_fen() {
    // Black to move at move 23; ...g6 gives the king g7, so Rd8 is check, not mate.
    let fen = "6k1/5ppp/8/8/8/8/5PPP/3R2K1 b - - 0 23";
    let mut g = GameScore::new(Some(Board::from_fen(fen, false).unwrap().to_string()));
    line(&mut g, None, "g6 Rd8+");
    g.result = GameResult::WhiteWins;
    let pgn = write_game(&g);
    assert!(pgn.contains("[SetUp \"1\"]\n[FEN \"6k1/5ppp/8/8/8/8/5PPP/3R2K1 b - - 0 23\"]"), "{pgn}");
    assert!(pgn.contains("23... g6 24. Rd8+ 1-0"), "{pgn}");
    assert_reads_back(&[g], &pgn);
}

#[test]
fn a_comment_that_closes_braces_survives_as_text() {
    let mut g = GameScore::new(None);
    let e4 = line(&mut g, None, "e4").unwrap();
    crate::doc::games::append_comment(&mut g.nodes[e4].comment, "a } b ) ( %c ;d");
    let pgn = write_game(&g);
    assert_eq!(g.nodes[e4].comment, "a ) b ) ( %c ;d");
    assert!(pgn.contains("{a ) b ) ( %c ;d}"), "{pgn}");
    assert_reads_back(&[g], &pgn);
}

/// The book's figurines in a comment are written as letters, as every export of a book writes
/// them (plan A9-F4: the book's PGN with its text did already).
#[test]
fn figurines_in_comments_are_letters() {
    let mut g = GameScore::new(None);
    let e4 = line(&mut g, None, "e4").unwrap();
    g.nodes[e4].comment = "Better is ♘f3 or ♗c4".to_owned();
    assert!(write_game(&g).contains("{Better is Nf3 or Bc4}"), "{}", write_game(&g));
}

#[test]
fn a_malformed_tree_is_written_without_looping() {
    let node = |parent, children| Node {
        san: "e4".to_owned(),
        ply: 0,
        nags: vec![],
        comment_before: String::new(),
        comment: String::new(),
        fen: String::new(),
        parent,
        children,
    };
    let mut g = GameScore::new(None);
    // 0 → 1 → 0 (a cycle) and a child that does not exist.
    g.nodes = vec![node(None, vec![1, 99]), node(Some(0), vec![0])];
    g.roots = vec![0, 0, 7];
    let made = write_report([&g], PgnStyle::FULL);
    assert!(made.pgn.ends_with("1. e4 *\n"), "{}", made.pgn);
    assert_eq!(g.canonical_order(), vec![0, 1]);
    // Node 1 plays `e4` again, which no position allows: left out, and said.
    assert_eq!(made.warnings, [GameWarning { game: 0, warning: PgnWarning { kind: PgnWarningKind::MovesLeftOut, at: 1 } }]);
}

/// A game no start can be given is left out of the PGN, and said: the others are written.
#[test]
fn a_game_that_cannot_start_is_left_out_and_said() {
    let mut bad = GameScore::new(Some("8/8/8/8/8/8/8/8 w - - 0 1".to_owned()));
    bad.roster.event = "bad".to_owned();
    let good = opera();
    let made = write_report([&bad, &good], PgnStyle::FULL);
    assert_eq!(made.pgn, write_game(&good));
    assert_eq!(made.warnings, [GameWarning { game: 0, warning: PgnWarning { kind: PgnWarningKind::GameLeftOut, at: 0 } }]);
}

/// A random legal game: `choices` pick moves; every fourth choice also opens a variation of up
/// to three moves from the position before the move just played.
fn random_game(choices: &[u16], from_middle: bool, comments: &[String], nags: &[u8]) -> GameScore {
    let mut start = Board::default();
    if from_middle {
        // Walk a few moves to get a non-initial start, then start the game there.
        for c in choices.iter().take(5) {
            let mut moves = Vec::new();
            start.generate_moves(|m| {
                moves.extend(m);
                false
            });
            let Some(&mv) = moves.get(usize::from(*c) % moves.len().max(1)) else { break };
            start.play(mv);
        }
    }
    let mut g = GameScore::new(from_middle.then(|| start.to_string()));
    g.roster.event = comments.first().cloned().unwrap_or_default().chars().filter(|c| !c.is_control()).collect::<String>().trim().to_owned();
    g.extra_tags.push(Tag { name: "Annotator".to_owned(), value: "someone \\ \"quoted\"".to_owned() });
    let mut at: Option<usize> = None;
    let pick = |g: &GameScore, at: Option<usize>, c: u16| -> Option<San> {
        let board = g.board_after(at)?;
        let mut moves = Vec::new();
        board.generate_moves(|m| {
            moves.extend(m);
            false
        });
        let mv = *moves.get(usize::from(c) % moves.len().max(1))?;
        San::from_move(&board, mv).ok()
    };
    for (i, &c) in choices.iter().enumerate() {
        let Some(s) = pick(&g, at, c) else { break };
        let before = at;
        let Ok(n) = g.play(at, &s, false) else { break };
        at = Some(n);
        if let Some(text) = comments.get(i % comments.len().max(1)).filter(|_| i % 3 == 0) {
            crate::doc::games::append_comment(&mut g.nodes[n].comment, text);
        }
        if let Some(&nag) = nags.get(i).filter(|n| **n > 0) {
            g.nodes[n].nags.push(u16::from(nag));
        }
        if i % 4 == 1 {
            let mut v = before;
            for k in 0..3u16 {
                let Some(s) = pick(&g, v, c.wrapping_add(k).wrapping_add(1)) else { break };
                let Ok(vn) = g.play(v, &s, false) else { break };
                if k == 0 && g.starts_variation(vn) {
                    g.comment_before(vn, "variation");
                }
                v = Some(vn);
            }
        }
    }
    g.result = [GameResult::WhiteWins, GameResult::BlackWins, GameResult::Draw, GameResult::Unknown][choices.len() % 4];
    // A game that ends in mate or stalemate has the board's result: the reader says when the tag
    // says another (plan A9-F5a, `ResultMismatch`), and this round trip expects no warning.
    if let Some(end) = g.board_after(at) {
        match end.status() {
            cozy_chess::GameStatus::Won if end.side_to_move() == cozy_chess::Color::White => g.result = GameResult::BlackWins,
            cozy_chess::GameStatus::Won => g.result = GameResult::WhiteWins,
            cozy_chess::GameStatus::Drawn => g.result = GameResult::Draw,
            cozy_chess::GameStatus::Ongoing => {}
        }
    }
    g.reorder_canonical();
    g
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn random_legal_games_round_trip(
        choices in proptest::collection::vec(any::<u16>(), 0..60),
        from_middle in any::<bool>(),
        comments in proptest::collection::vec("[ -~é♘{}()%;\n\t]{0,40}", 1..4),
        nags in proptest::collection::vec(0u8..20, 0..60),
    ) {
        let g = random_game(&choices, from_middle, &comments, &nags);
        let pgn = write_game(&g);
        for line in pgn.lines().filter(|l| !l.starts_with('[')) {
            // Only a lone long word, or a comment word that would start a line with `%`, overruns.
            prop_assert!(line.chars().count() <= LINE_WIDTH || !line.contains(' ') || line.contains(" %"), "line too long: {line}");
            prop_assert!(!line.starts_with('%'));
        }
        // The result written is the score's, unless the board decides it (a mate, a stalemate).
        let back = read_games(&pgn);
        prop_assert_eq!(back.len(), 1);
        let board = to_game(&g).unwrap();
        prop_assert_eq!(tree(&back[0]), tree_in_letters(&board.game), "{}", pgn);
        prop_assert_eq!(pdfcraft_chess::pgn::write(&back[0], WriteOptions { letters: true, ..WriteOptions::default() }), pgn.clone());
        // Two games in one text read back as two.
        let two = write([&g, &g]);
        prop_assert_eq!(read_games(&two).len(), 2);
    }
}

/// The synthetic book's game (plan A8 P8a): every castling right, an en passant square, Black's
/// 23rd move first, a comment holding `}` and `%`, a NAG past 139, tags holding `"` and `\`, and
/// where in the book it came from.
fn book_game() -> GameScore {
    let fen = "r3k2r/pppq1ppp/8/8/3Pp3/8/PPP2PPP/R3K2R b KQkq d3 0 23";
    let mut g = GameScore::new(Some(Board::from_fen(fen, false).unwrap().to_string()));
    let origin = pdfcraft_chess::origin::PgnOrigin {
        book_title: Some("Livro \"um\" \\ dois".to_owned()),
        page_index: 22,
        page_label: Some("17".to_owned()),
        diagram: Some(1),
        of: Some(2),
        puzzle: None,
        lang: Some(Lang::Pt),
    };
    g.extra_tags.push(Tag { name: "Annotator".to_owned(), value: String::new() });
    g.extra_tags.extend(origin.tags().into_iter().map(|(name, value)| Tag { name, value }));
    g.comment = pdfcraft_chess::origin::with_origin_comment("", &origin);
    let ep = line(&mut g, None, "exd3").unwrap();
    crate::doc::games::append_comment(&mut g.nodes[ep].comment, "en passant } then %castle");
    let long = line(&mut g, Some(ep), "O-O-O").unwrap();
    g.nodes[long].nags.push(146);
    line(&mut g, Some(long), "O-O");
    g
}

/// The document's writer leaves out what the style says, as the board's does (plan A8 V4): the
/// tags are all kept, and what is written reads back.
#[test]
fn the_style_leaves_out_variations_comments_and_glyphs() {
    use pdfcraft_chess::pgn_style::{PgnStyle, Variations};
    let mut g = book_game();
    let ep = g.roots[0];
    line(&mut g, Some(ep), "Kf1");
    let tags = |pgn: &str| pgn.split("\n\n").next().unwrap_or_default().to_owned();
    let moves = |pgn: &str| pgn.split("\n\n").nth(1).unwrap_or_default().split_whitespace().collect::<Vec<_>>().join(" ");
    let full = write_game(&g);
    assert_eq!(write_game_with(&g, PgnStyle::FULL), full);
    assert!(moves(&full).contains("(24. Kf1)") && moves(&full).contains("$146") && moves(&full).contains('{'), "{full}");
    let main = write_game_with(&g, PgnStyle { variations: Variations::Depth(0), ..PgnStyle::FULL });
    assert_eq!(tags(&main), tags(&full));
    assert!(!main.contains("Kf1") && moves(&main).contains("24. O-O-O $146 O-O *"), "{main}");
    let bare = write_game_with(&g, PgnStyle { variations: Variations::Depth(0), comments: false, nags: false });
    assert_eq!(tags(&bare), tags(&full));
    assert_eq!(moves(&bare), "23... exd3 24. O-O-O O-O *", "{bare}");
    let back = read_games(&bare);
    assert_eq!(back.len(), 1);
    assert!(back[0].comment(0).is_empty() && back[0].tokens().iter().all(|t| !matches!(t, pdfcraft_chess::game::Token::Comment { .. })));
}

#[test]
fn the_book_game_is_written_as_the_golden_and_read_back() {
    let g = book_game();
    let pgn = write_game(&g);
    let golden = r#"[Event "?"]
[Site "?"]
[Date "????.??.??"]
[Round "?"]
[White "?"]
[Black "?"]
[Result "*"]
[SetUp "1"]
[FEN "r3k2r/pppq1ppp/8/8/3Pp3/8/PPP2PPP/R3K2R b KQkq d3 0 23"]
[PdfPage "23"]
[PdfPageLabel "17"]
[PdfDiagram "1"]
[SourceTitle "Livro \"um\" \\ dois"]

{Pág. 17 · diagrama 1 de 2} 23... exd3 {en passant ) then %castle} 24. O-O-O
$146 O-O *
"#;
    assert!(!pgn.starts_with('\u{feff}') && pgn.lines().all(|l| l.chars().count() <= LINE_WIDTH && !l.starts_with('%')), "{pgn}");
    assert_eq!(pgn, golden, "{pgn}");
    assert_reads_back(std::slice::from_ref(&g), &pgn);
    // The board's reader: the same start, moves and tags.
    let board = &read_games(&pgn)[0];
    assert_eq!(board.start_fen(), g.start_fen.clone().unwrap());
    assert_eq!(board.tag("SourceTitle"), Some("Livro \"um\" \\ dois"));
    assert_eq!(board.tag("PdfPage"), Some("23"), "the physical page");
    assert_eq!(board.tag("PdfPageLabel"), Some("17"));
    // One writer: the board's game of the score writes the same text (the board keeps no tag
    // without a value, so the empty Annotator is not written by either).
    assert_eq!(pdfcraft_chess::pgn::write(&to_game(&g).unwrap().game, WriteOptions { letters: true, ..WriteOptions::default() }), pgn);
}

#[test]
fn an_unwritable_san_is_named_again_never_written_as_a_null_move() {
    let mut g = GameScore::new(None);
    let e4 = line(&mut g, None, "e4").unwrap();
    let e5 = line(&mut g, Some(e4), "e5").unwrap();
    g.nodes[e5].san = "♟??".to_owned();
    let made = write_report([&g], PgnStyle::FULL);
    assert!(made.pgn.contains("1. e4 e5 *") && !made.pgn.contains("--"), "{}", made.pgn);
    assert_eq!(made.warnings, []);
    // No legal move reaches the position after it: the line stops there, and says so.
    g.nodes[e5].fen = "4k3/8/8/8/8/8/8/4K3 w - - 0 2".to_owned();
    let made = write_report([&g], PgnStyle::FULL);
    assert!(made.pgn.contains("1. e4 *") && !made.pgn.contains("--"), "{}", made.pgn);
    assert_eq!(made.warnings, [GameWarning { game: 0, warning: PgnWarning { kind: PgnWarningKind::MovesLeftOut, at: 1 } }]);
}

/// python-chess (GPL) as an outside oracle, run as a process only: it reads the book game to the
/// same start, moves and tags. Skipped without Python or python-chess.
#[test]
fn python_chess_reads_the_book_game_the_same() {
    let pgn = write_game(&book_game());
    let script = "import sys, chess.pgn\ng = chess.pgn.read_game(sys.stdin)\nprint(g.board().fen())\nprint(' '.join(g.board().variation_san(g.mainline_moves()).split()))\nprint(g.headers.get('SourceTitle'))\nprint(len(g.errors))";
    let child = std::process::Command::new("python")
        .args(["-I", "-c", script])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn();
    let Ok(mut child) = child else { return };
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write as _;
        let _ = stdin.write_all(pgn.as_bytes());
    }
    let Ok(out) = child.wait_with_output() else { return };
    if !out.status.success() {
        return; // no python-chess
    }
    let text = String::from_utf8_lossy(&out.stdout).replace('\r', "");
    let lines: Vec<&str> = text.lines().collect();
    // python-chess (1.11) gives a tag's value as written, its escapes kept.
    assert_eq!(lines, ["r3k2r/pppq1ppp/8/8/3Pp3/8/PPP2PPP/R3K2R b KQkq d3 0 23", "23...exd3 24. O-O-O O-O", r#"Livro \"um\" \\ dois"#, "0"], "{pgn}");
}
