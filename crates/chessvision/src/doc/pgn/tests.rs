use cozy_chess::Board;
use pdfcraft_chess::san::{Lang, San};
use proptest::prelude::*;

use super::*;
use crate::doc::games::{GameScore, Node, Tag};

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
    let pgn = write_game(&g);
    assert_eq!(parse(&pgn).unwrap(), vec![g]);
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
    assert_eq!(parse(&pgn).unwrap(), vec![g]);
}

#[test]
fn several_games_and_lenient_forms_are_read() {
    let text = "[Event \"a\"]\n\n1.e4 e5!? 2.Nf3 ; a line comment\n*\n\n%escaped line\n[Event \"b\"]\n1. d4 {x} 1... d5 1/2-1/2\n";
    let games = parse(text).unwrap();
    assert_eq!(games.len(), 2);
    assert_eq!(games[0].roster.event, "a");
    assert_eq!(games[0].nodes[1].nags, vec![5], "!? is NAG 5");
    assert_eq!(games[1].result, GameResult::Draw);
    assert_eq!(games[1].nodes[0].comment, "x");
}

#[test]
fn malformed_pgn_is_an_error() {
    for bad in ["1. e4 e5 2. Ke3", "1. e4 (", "1. e4 )", "[Event \"x", "{open", "(1. e4)", "$5 1. e4", "1. e4 $x", "[FEN \"nonsense\"] 1. e4"] {
        assert!(parse(bad).is_err(), "{bad} should not parse");
    }
    assert_eq!(parse("").unwrap(), Vec::<GameScore>::new());
}

#[test]
fn a_comment_that_closes_braces_survives_as_text() {
    let mut g = GameScore::new(None);
    let e4 = line(&mut g, None, "e4").unwrap();
    crate::doc::games::append_comment(&mut g.nodes[e4].comment, "a } b ) ( %c ;d");
    let pgn = write_game(&g);
    assert_eq!(parse(&pgn).unwrap(), vec![g.clone()]);
    assert_eq!(g.nodes[e4].comment, "a ) b ) ( %c ;d");
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
    let pgn = write_game(&g);
    assert!(pgn.ends_with("*\n"));
    assert_eq!(g.canonical_order(), vec![0, 1]);
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
    g.roster.event = comments.first().cloned().unwrap_or_default().chars().filter(|c| !c.is_control()).collect();
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
        let back = parse(&pgn);
        prop_assert_eq!(back, Ok(vec![g.clone()]), "{}", pgn);
        // Two games in one text read back as two.
        let two = write([&g, &g]);
        prop_assert_eq!(parse(&two).map(|v| v.len()), Ok(2));
    }

    #[test]
    fn arbitrary_text_never_panics(text in "[\\[\\]{}()$;%\"\\\\ \n.0-9a-hKQRBNOx=+#!?*/-]{0,200}") {
        let _ = parse(&text);
    }

    #[test]
    fn arbitrary_unicode_never_panics(text in "\\PC{0,200}") {
        let _ = parse(&text);
    }
}
