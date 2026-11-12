use super::*;
use crate::pgn_style::PgnStyle;

/// The moves alone, comments and glyphs left out.
fn moves(g: &Game) -> String {
    g.movetext_with(PgnStyle { comments: false, nags: false, ..PgnStyle::FULL })
}

/// The main line's moves in SAN.
fn main_line(g: &Game) -> Vec<String> {
    g.main_line().into_iter().skip(1).map(|n| g.san(n).to_owned()).collect()
}

fn kinds(report: &PgnReport) -> Vec<PgnWarningKind> {
    report.warnings.iter().map(|w| w.kind).collect()
}

/// Plan A9-F2b (A1-03, sonda p03): a variation that starts by repeating the main line's move
/// never takes the main line; the moves it adds become variations where the main line goes on.
#[test]
fn a_variation_repeating_the_main_lines_move_stays_a_variation() {
    let (g, report) = Game::from_pgn_report("1. e4 e5 2. Nf3 (2. Nf3 Nc6 3. Bc4) 2... Nc6 3. Bb5 a6 *").unwrap();
    assert_eq!(report, PgnReport::default());
    assert_eq!(main_line(&g), ["e4", "e5", "Nf3", "Nc6", "Bb5", "a6"]);
    assert_eq!(g.movetext(), "1. e4 e5 2. Nf3 Nc6 3. Bb5 (3. Bc4) 3... a6");
    // The plan's own form, and two variations that repeat it: the main line's reply goes first
    // also where it was already played, as the second of them.
    let (g, _) = Game::from_pgn("1. e4 e5 2.Nf3 (2.Nf3 Nc6 3.Bc4) 2...Nc6 3.Bb5 *").unwrap();
    assert_eq!(main_line(&g), ["e4", "e5", "Nf3", "Nc6", "Bb5"]);
    let (g, _) = Game::from_pgn("1. e4 e5 2. Nf3 (2. Nf3 Nf6) (2. Nf3 Nc6 3. Bc4) 2... Nc6 3. Bb5 *").unwrap();
    assert_eq!(main_line(&g), ["e4", "e5", "Nf3", "Nc6", "Bb5"]);
    assert_eq!(moves(&g), "1. e4 e5 2. Nf3 Nc6 (2... Nf6) 3. Bb5 (3. Bc4)");
    // A variation of a variation never takes its parent's line either.
    let (g, _) = Game::from_pgn("1. d4 (1. e4 e5 (1... e5 2. f4) 2. Nf3) 1... d5 *").unwrap();
    assert_eq!(moves(&g), "1. d4 (1. e4 e5 2. Nf3 (2. f4)) 1... d5");
    // What it writes reads back the same.
    let (back, report) = Game::from_pgn_report(&g.pgn()).unwrap();
    assert_eq!((back.pgn(), report), (g.pgn(), PgnReport::default()));
}

/// Plan A9-F2b (A1-06, sondas p10 and p15): a comment between `)` and the next move is that
/// move's comment-before; one before another `(` stays after the move before; a variation holding
/// only a comment leaves it on the move it was an alternative to.
#[test]
fn a_comment_after_a_variation_goes_with_the_next_move() {
    let (g, _) = Game::from_pgn("1. e4 (1. d4) {about e5} 1... e5 *").unwrap();
    let [e4, e5] = [1, 2].map(|i| g.main_line()[i]);
    assert_eq!((g.comment(e4), g.comment_before(e5)), ("", "about e5"));
    let (g, _) = Game::from_pgn("1. e4 ({only a comment}) e5 *").unwrap();
    assert_eq!((g.comment(0), g.comment(g.main_line()[1])), ("", "only a comment"));
    let (g, _) = Game::from_pgn("1. e4 e5 (1... c5) {x} (1... e6) 2. Nf3 *").unwrap();
    let [e5, nf3] = [2, 3].map(|i| g.main_line()[i]);
    assert_eq!((g.comment(e5), g.comment_before(nf3)), ("x", ""));
    // At the end of the text, and with two comments: after the move before the variations.
    let (g, _) = Game::from_pgn("1. e4 e5 (1... c5) {y} {z}").unwrap();
    assert_eq!(g.comment(g.main_line()[2]), "y z");
}

/// Plan A9-F2b (A1-09, sonda p02): the common spellings books and sites use are read to the end,
/// each noted.
#[test]
fn common_non_standard_spellings_are_read_and_noted() {
    let promotion = "[FEN \"8/P7/8/8/8/8/7k/K7 w - - 0 1\"]\n\n";
    for (text, line, nags) in [
        ("1. e4 a6 2. e5 d5 3. exd6 e.p. cxd6 *", "1. e4 a6 2. e5 d5 3. exd6 cxd6", vec![]),
        ("1. e4 e5 2. Nf3 ± Nc6 *", "1. e4 e5 2. Nf3 Nc6", vec![16]),
        ("1. e4 e5 2. Nf3± Nc6 *", "1. e4 e5 2. Nf3 Nc6", vec![16]),
        ("1. e4 e5 2. Nf3+- Nc6 *", "1. e4 e5 2. Nf3 Nc6", vec![18]),
        ("1. e4 e5 2. Nf3 2…Nc6 (2… Nf6) 3. Bb5 *", "1. e4 e5 2. Nf3 Nc6 (2... Nf6) 3. Bb5", vec![]),
        (&format!("{promotion}1. a8Q Kh3 *"), "1. a8=Q Kh3", vec![]),
        (&format!("{promotion}1. a8=q Kh3 *"), "1. a8=Q Kh3", vec![]),
    ] {
        let (g, report) = Game::from_pgn_report(text).unwrap();
        assert_eq!((moves(&g).as_str(), report.stopped.as_deref()), (line, None), "{text}");
        let glyphs: Vec<u16> = g.main_line().iter().flat_map(|&n| g.nags(n).to_vec()).collect();
        assert_eq!(glyphs, nags, "{text}");
        let noted = kinds(&report);
        assert!(!noted.is_empty() && noted.iter().all(|k| *k == PgnWarningKind::NonStandard), "{text}: {noted:?}");
    }
    // What PGN tools write is not noted: castling with zeros, `!?` alone, a null move.
    let (_, report) = Game::from_pgn_report("1. e4 !? e5 2. Nf3 Nc6 3. Bc4 Bc5 4. 0-0 -- *").unwrap();
    assert_eq!(report, PgnReport::default());
}

/// Plan A9-F2b (A1-15, sonda p14): the cap on a game's moves is said as such, not as an illegal
/// move.
#[test]
fn the_cap_on_moves_is_not_an_illegal_move() {
    let text = "Nf3 Nf6 Ng1 Ng8 ".repeat(2600);
    let (g, report) = Game::from_pgn_report(&text).unwrap();
    assert_eq!(g.main_line().len(), crate::game::MAX_NODES);
    assert_eq!((report.stopped.as_deref(), report.stop_reason), (Some("Ng8"), Some(PgnStopReason::TooLong)));
    // An illegal move, an ambiguous one, and a word that is no move are told apart.
    let (_, report) = Game::from_pgn_report("1. e4 Ke3 *").unwrap();
    assert_eq!((report.stopped.as_deref(), report.stop_reason), (Some("Ke3"), Some(PgnStopReason::Illegal)));
    let (_, report) = Game::from_pgn_report("[FEN \"4k3/8/8/8/8/8/8/1N1NK3 w - - 0 1\"]\n\n1. Nc3 *").unwrap();
    assert_eq!((report.stopped.as_deref(), report.stop_reason), (Some("Nc3"), Some(PgnStopReason::Ambiguous)));
    let (g, report) = Game::from_pgn_report("1. e4 zz e5 *").unwrap();
    assert_eq!((moves(&g).as_str(), report.stopped.as_deref(), report.stop_reason), ("1. e4", Some("zz"), Some(PgnStopReason::Illegal)));
    // The first in the text is the one said, whichever step found it.
    let (_, report) = Game::from_pgn_report("1. e4 (1. d4 zz) e5 2. Ke3 *").unwrap();
    assert_eq!(report.stopped.as_deref(), Some("zz"));
    let (g, report) = Game::from_pgn_report("1. e4 (1. d4 Ke4) e5 2. zz *").unwrap();
    assert_eq!((moves(&g).as_str(), report.stopped.as_deref()), ("1. e4 (1. d4) 1... e5", Some("Ke4")));
}

/// Plan A9-F2b (A1-21, sonda p08): the spellings of a draw are read as a draw, in the tag and
/// after the moves, and a result with no move before it is kept.
#[test]
fn results_are_normalized_and_kept_without_moves() {
    for text in ["[Result \"1/2\"]\n\n1. e4 *", "[Result \"½-½\"]\n\n1. e4 *", "1. e4 0.5-0.5", "1. e4 1/2", "1. e4 ½-½"] {
        let (g, _) = Game::from_pgn(text).unwrap();
        assert_eq!(g.result(), "1/2-1/2", "{text}");
        assert!(g.pgn().ends_with("1. e4 1/2-1/2\n"), "{text}: {}", g.pgn());
    }
    let (g, _) = Game::from_pgn("[Event \"Study\"]\n[FEN \"4k3/8/8/8/8/8/8/R3K3 w - - 0 1\"]\n\n1-0\n").unwrap();
    assert_eq!(g.result(), "1-0");
    assert!(g.pgn().ends_with("\n\n1-0\n"), "{}", g.pgn());
}

/// Plan A9-F2b: `$n` is the code written (Black's `$37` stays `$37`); a glyph read from text is
/// the code for the side that moved.
#[test]
fn written_codes_stay_and_glyphs_take_the_movers_code() {
    let (g, _) = Game::from_pgn("1. e4 $37 e5 $36 *").unwrap();
    assert_eq!([1, 2].map(|i| g.nags(g.main_line()[i]).to_vec()), [vec![37], vec![36]]);
    let (g, _) = Game::from_pgn("1. e4 e5 ↑ *").unwrap();
    assert_eq!(g.nags(g.main_line()[2]), [37]);
}

/// Plan A9-F2b (§2.5, R2 O8): 100,000 `(` in a thread with a small stack read to `Ok`, the
/// levels past the cap kept as text and said; nothing in the reading recurses or nests.
#[test]
fn a_hundred_thousand_brackets_read_in_a_small_stack() {
    let worker = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let text = format!("1. e4 {}*", "(1. d4 ".repeat(100_000));
            let (g, report) = Game::from_pgn_report(&text).unwrap();
            (main_line(&g), kinds(&report))
        })
        .unwrap();
    let (line, kinds) = worker.join().unwrap();
    assert_eq!(line, ["e4"]);
    // The text kept from it passes a comment's cap: cut there, and said.
    assert_eq!(kinds, [PgnWarningKind::TooDeep, PgnWarningKind::CommentCut]);
}

/// The golden of plan A9-F2b: A1's probes that the reader answers (p01, p02, p03, p08, p10, p13,
/// p14 are above or in `split`/`game`), A7-01's, and the corpus, each read to the movetext, the
/// line it stopped at and why, and the warnings. The others (p05 FEN, p06 zugzwang, p07 graft,
/// p11 move numbers, p16 SAN, p17 cuts, p18 Cyrillic O) are F3's and F5's.
#[test]
fn the_golden_reads() {
    let golden: [(&str, &str, Option<&str>, &[PgnWarningKind]); 9] = [
        // A8 §7.4's corpus, unchanged.
        (
            "1. e4 e5 (1... c5 {Sicilian} 2. Nf3 (2. c3 d5 (2... Nf6 3. e5 $1)) d6) 2. Nf3 {A comment} ({Before} 2. Bc4 $5 Nf6) Nc6 3. Bb5 *",
            "1. e4 e5 (1... c5 {Sicilian} 2. Nf3 (2. c3 d5 (2... Nf6 3. e5 $1)) 2... d6) 2. Nf3 {A comment} ({Before} 2. Bc4 $5 Nf6) 2... Nc6 3. Bb5",
            None,
            &[],
        ),
        // p03.
        ("1. e4 e5 2. Nf3 (2. Nf3 Nc6 3. Bc4) 2... Nc6 3. Bb5 a6 *", "1. e4 e5 2. Nf3 Nc6 3. Bb5 (3. Bc4) 3... a6", None, &[]),
        // p10.
        ("1. e4 (1. d4) {about e5} 1... e5 *", "1. e4 {about e5} (1. d4) 1... e5", None, &[]),
        ("1. e4 ({only a comment}) e5 *", "1. e4 {only a comment} 1... e5", None, &[]),
        // p02.
        ("1. e4 e5 2. Nf3 ± Nc6 *", "1. e4 e5 2. Nf3 $16 Nc6", None, &[PgnWarningKind::NonStandard]),
        // p08: tags on one line; the draw.
        ("[Event \"A\"] [Result \"1/2\"]\n\n1. e4 *", "1. e4", None, &[]),
        // An illegal move ends its own line only; the rest of it is a comment where it stopped
        // (plan A9-F3), a variation's on the move it was an alternative to.
        ("1. e4 e5 (1... Ke3 c5) 2. Nf3 Nc6 3. Ke3 Bc5", "1. e4 e5 {Ke3 c5} 2. Nf3 Nc6 {Ke3 Bc5}", Some("Ke3"), &[]),
        // Variations with no move before them, and one never closed.
        ("(1. d4 d5) 1. e4 e5 *", "{(d4 d5)} 1. e4 e5", None, &[PgnWarningKind::OrphanVariation]),
        ("1. e4 e5 ((2. Nf3", "1. e4 e5 {(Nf3}", None, &[PgnWarningKind::OrphanVariation]),
    ];
    for (text, movetext, stopped, warnings) in golden {
        let (g, report) = Game::from_pgn_report(text).unwrap();
        assert_eq!((g.movetext().as_str(), report.stopped.as_deref(), kinds(&report).as_slice()), (movetext, stopped, warnings), "{text}");
    }
    // A7-01: the games after a brace in a `;` comment, each read whole.
    let text = "[Event \"A\"]\n1. e4 e5 ; White plays {the king's pawn\n2. Nf3 *\n[Event \"B\"]\n1. d4 d5 *\n";
    let games = crate::game::split_pgn(text, 10);
    let read: Vec<(String, Option<String>)> = games.iter().map(|t| Game::from_pgn(t).map(|(g, s)| (moves(&g), s)).unwrap()).collect();
    assert_eq!(read, [("1. e4 e5 2. Nf3".to_owned(), None), ("1. d4 d5".to_owned(), None)]);
}

/// The three steps are pure: parse gives the same items for the same text, and build of a parse
/// with nothing in it is the standard start.
#[test]
fn parse_is_flat_and_build_of_nothing_is_the_start() {
    let parsed = parse("[Event \"x\"]\n1. e4 (1. d4 {c} $1) e5 *");
    assert_eq!(parsed.tags, [("Event".to_owned(), "x".to_owned())]);
    let shape: Vec<&str> = parsed
        .items
        .iter()
        .map(|i| match i {
            FragmentItem::Move { .. } => "m",
            FragmentItem::Open => "(",
            FragmentItem::Close => ")",
            FragmentItem::Comment { .. } => "c",
            FragmentItem::Nag(NagCode::Written(_)) => "$",
            FragmentItem::Nag(NagCode::WhiteRelative(_)) => "g",
            FragmentItem::Result => "r",
        })
        .collect();
    assert_eq!(shape, ["m", "(", "m", "c", "$", ")", "m"]);
    assert_eq!(parsed.result.as_deref(), Some("*"));
    assert_eq!(parse("[Event \"x\"]\n1. e4 *"), parse("[Event \"x\"]\n1. e4 *"));
    let (g, report) = build(Parsed::default()).unwrap();
    assert_eq!((g.movetext(), g.start_fen(), report), (String::new(), Game::standard().start_fen(), PgnReport::default()));
}

/// python-chess (GPL: an outside process only; the test skips without it) reads the same main
/// line from the texts where a variation repeats the main line's move, and the same tree from
/// what we write.
#[test]
fn python_chess_reads_the_same_main_lines() {
    let script = "import sys, chess.pgn\n\
        g = chess.pgn.read_game(sys.stdin)\n\
        n = g\n\
        out = []\n\
        while n.variations:\n\
        \x20   n = n.variations[0]\n\
        \x20   out.append(n.san())\n\
        print(' '.join(out))";
    let python = |text: &str| -> Option<String> {
        let mut child = std::process::Command::new("python")
            .args(["-I", "-c", script])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()?;
        {
            use std::io::Write as _;
            child.stdin.take()?.write_all(text.as_bytes()).ok()?;
        }
        let out = child.wait_with_output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
    };
    for text in [
        "1. e4 e5 2. Nf3 (2. Nf3 Nc6 3. Bc4) 2... Nc6 3. Bb5 a6 *",
        "1. e4 e5 2. Nf3 (2. Nf3 Nf6) (2. Nf3 Nc6 3. Bc4) 2... Nc6 3. Bb5 *",
        "1. d4 (1. e4 e5 (1... e5 2. f4) 2. Nf3) 1... d5 *",
    ] {
        let Some(theirs) = python(text) else { return };
        let (g, _) = Game::from_pgn(text).unwrap();
        assert_eq!(main_line(&g).join(" "), theirs, "{text}");
        assert_eq!(python(&g.pgn()).as_deref(), Some(theirs.as_str()), "{}", g.pgn());
    }
}

proptest::proptest! {
    /// Whatever is read, writing it and reading it again gives the same game (the normalization
    /// is idempotent), the variations repeating moves and the comments after `)` included.
    #[test]
    fn reading_what_was_written_changes_nothing(parts in proptest::collection::vec(
        proptest::sample::select(vec![
            "e4", "e5", "Nf3", "Nc6", "Bb5", "d4", "(", ")", "{c}", "$1", "$37", "1.", "2...", "!?", "±", "e.p.", "*", "Nf3±", "O-O",
        ]), 0..60)) {
        let text = parts.join(" ");
        if let Ok((g, _)) = Game::from_pgn(&text) {
            let (again, report) = Game::from_pgn_report(&g.pgn()).unwrap();
            proptest::prop_assert_eq!(again.pgn(), g.pgn());
            proptest::prop_assert_eq!(report.stopped, None);
        }
    }
}

fn count(report: &PgnReport, kind: PgnWarningKind) -> usize {
    report.warnings.iter().filter(|w| w.kind == kind).count()
}

/// Plan A9-F3 (A1-02, sonda p17): a comment of 7,999 bytes is read whole and stays whole when
/// edited; every tag and glyph the reader can't keep is said, one warning each; glyphs of one
/// group are kept side by side.
#[test]
fn nothing_is_cut_or_dropped_unsaid() {
    let long = "a".repeat(7_999);
    let tags: String = (0..80).map(|i| format!("[Tag{i} \"v\"]\n")).collect();
    let text = format!("[Annotator \"{}\"]\n{tags}\n1. e4 {{{long}}} $1 $10 $13 $14 $22 $32 $36 $40 $132 $138 $140 $146 *", "x".repeat(400));
    let (mut g, report) = Game::from_pgn_report(&text).unwrap();
    let e4 = g.main_line()[1];
    assert_eq!(g.comment(e4).len(), 7_999);
    assert_eq!(g.set_comment(e4, &format!("{long}b")), Ok(()));
    assert_eq!(g.comment(e4).len(), 8_000, "edited without a cut");
    assert_eq!(g.nags(e4), [1, 10, 13, 14, 22, 32, 36, 40]);
    assert_eq!(g.tag("Annotator").map(str::len), Some(crate::game::MAX_TAG_LEN));
    let said = [PgnWarningKind::TagCut, PgnWarningKind::TagsDropped, PgnWarningKind::NagDropped].map(|k| count(&report, k));
    assert_eq!(said, [1, 81 - crate::game::MAX_TAGS, 4], "{report:?}");
    // Two glyphs of one group (a quality and another), and a glyph with no move before it.
    let (g, report) = Game::from_pgn_report("$14 1. e4 $1 $3 *").unwrap();
    assert_eq!(g.nags(g.main_line()[1]), [1, 3]);
    assert_eq!(report.warnings, [PgnWarning { kind: PgnWarningKind::NagDropped, at: 0 }]);
    // Codes PGN has none for.
    let (_, report) = Game::from_pgn_report("1. e4 $0 $256 $70000 *").unwrap();
    assert_eq!(count(&report, PgnWarningKind::NagDropped), 3);
    // A comment past the cap: cut there, said.
    let (g, report) = Game::from_pgn_report(&format!("1. e4 {{{}}} *", "b".repeat(crate::game::MAX_COMMENT_LEN + 10))).unwrap();
    assert_eq!(g.comment(g.main_line()[1]).len(), crate::game::MAX_COMMENT_LEN);
    assert_eq!(report.warnings, [PgnWarning { kind: PgnWarningKind::CommentCut, at: g.main_line()[1] }]);
}

/// Plan A9-F3 (A1-08, A1-20, sondas p05 and p11): a FEN another program got slightly wrong opens,
/// made legal, and says so; one already right says nothing.
#[test]
fn a_slightly_wrong_fen_opens_repaired_and_said() {
    for (fen, start) in [
        ("4k3/8/8/8/8/8/8/4K3 w - - 0 0", "4k3/8/8/8/8/8/8/4K3 w - - 0 1"),
        ("4k3/8/8/8/8/8/8/4K3 w - - 120 90", "4k3/8/8/8/8/8/8/4K3 w - - 100 90"),
        ("4k3/8/8/8/8/8/8/4K3 w - - 0 70000", "4k3/8/8/8/8/8/8/4K3 w - - 0 999"),
        ("4k3/8/8/8/8/8/8/4K3 w - - 0 65535", "4k3/8/8/8/8/8/8/4K3 w - - 0 999"),
        ("r3k3/8/8/8/8/8/8/R3K2R w KQkq - 0 1", "r3k3/8/8/8/8/8/8/R3K2R w KQq - 0 1"),
        ("4k3/8/8/8/8/8/8/4K3 w - e3 0 1", "4k3/8/8/8/8/8/8/4K3 w - - 0 1"),
    ] {
        let (g, report) = Game::from_pgn_report(&format!("[FEN \"{fen}\"]\n[SetUp \"1\"]\n\n1. Ke2 *")).unwrap();
        assert_eq!((g.start_fen().as_str(), main_line(&g)), (start, vec!["Ke2".to_owned()]), "{fen}");
        assert_eq!(report.warnings, [PgnWarning { kind: PgnWarningKind::FenRepaired, at: 0 }], "{fen}");
    }
    let (_, report) = Game::from_pgn_report("[FEN \"4k3/8/8/8/8/8/8/4K3 w - - 3 40\"]\n\n1. Ke2 *").unwrap();
    assert_eq!(report, PgnReport::default());
    // A position no repair makes legal is still refused, with why.
    assert!(matches!(Game::from_pgn_report("[FEN \"8/8/8/8/8/8/8/4K3 w - - 0 1\"]\n\n*"), Err(GameError::IllegalPosition(_))));
}

/// Plan A9-F3 (A4-05): the rest of a line after a move that can't be played is a comment where
/// the line stopped, so what is written back still holds it.
#[test]
fn the_rest_after_an_impossible_move_is_kept_as_a_comment() {
    let (g, report) = Game::from_pgn_report("1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 4. Bxh7 {a blunder} Nf6 (4... d6) 5. O-O *").unwrap();
    assert_eq!((report.stopped.as_deref(), report.stop_reason), (Some("Bxh7"), Some(PgnStopReason::Illegal)));
    assert_eq!(g.movetext(), "1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 {Bxh7 a blunder Nf6 (d6) O-O}");
    let (back, _) = Game::from_pgn_report(&g.pgn()).unwrap();
    assert_eq!(back.pgn(), g.pgn());
    // A variation whose first move can't be played: on the move it was an alternative to.
    let (g, _) = Game::from_pgn("1. e4 (1. Ke2 d5) e5 *").unwrap();
    assert_eq!(g.movetext(), "1. e4 {Ke2 d5} 1... e5");
    // A word that is no move, and a comment that waited for the move.
    let (g, _) = Game::from_pgn("1. e4 e5 2. zz 3. Nf3 *").unwrap();
    assert_eq!(g.movetext(), "1. e4 e5 {zz Nf3}");
    let (g, _) = Game::from_pgn("1. e4 e5 (1... c5) {waits} 2. Ke3 Nc6 *").unwrap();
    assert_eq!(g.movetext(), "1. e4 e5 {waits Ke3 Nc6} (1... c5)");
}

/// Plan A9-F3 (A1-21): a result the tag disagrees with, or a tag that is no result, is said.
#[test]
fn a_result_the_reader_ignores_is_said() {
    let (g, report) = Game::from_pgn_report("[Result \"1-0\"]\n\n1. e4 0-1").unwrap();
    assert_eq!((g.result(), report.warnings), ("1-0".to_owned(), vec![PgnWarning { kind: PgnWarningKind::ResultIgnored, at: 0 }]));
    let (_, report) = Game::from_pgn_report("[Result \"won on time\"]\n\n1. e4 *").unwrap();
    assert_eq!(count(&report, PgnWarningKind::ResultIgnored), 1);
    let (_, report) = Game::from_pgn_report("[Result \"1-0\"]\n\n1. e4 1-0").unwrap();
    assert_eq!(report, PgnReport::default());
}
