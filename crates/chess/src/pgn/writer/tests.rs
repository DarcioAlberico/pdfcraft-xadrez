use proptest::prelude::*;

use super::*;
use crate::game::{Game, NodeId, PgnReport};
use crate::pgn_style::Variations;

fn read(text: &str) -> Game {
    let (game, report) = Game::from_pgn_report(text).unwrap();
    assert_eq!(report.stopped, None, "{text}");
    game
}

/// The move text of a written game: everything after the blank line.
fn moves_of(pgn: &str) -> &str {
    pgn.split_once("\n\n").map_or("", |(_, moves)| moves)
}

/// Every move of the tree in PGN order with its depth, glyphs and both comments, and the start's
/// comment: what a round trip must keep.
fn tree(g: &Game) -> Vec<String> {
    fn walk(g: &Game, id: NodeId, depth: usize, out: &mut Vec<String>) {
        for (i, &child) in g.replies(id).iter().enumerate() {
            let d = if i == 0 { depth } else { depth + 1 };
            out.push(format!("{d} {} {:?} [{}] [{}]", g.san(child), g.nags(child), g.comment_before(child), g.comment(child)));
            walk(g, child, d, out);
        }
    }
    let mut out = vec![format!("start {} [{}]", g.start_fen(), g.comment(0))];
    walk(g, 0, 0, &mut out);
    out
}

/// Each line of a written game's move text fits in [`LINE_WIDTH`], unless it is one piece alone
/// or a piece kept off the start of the next line because it starts with `%`; none starts with
/// `%`.
fn assert_wrapped(pgn: &str) {
    for line in moves_of(pgn).lines() {
        assert!(!line.starts_with('%'), "{line:?} in\n{pgn}");
        assert!(line.chars().count() <= LINE_WIDTH || !line.contains(' ') || line.contains(" %"), "{line:?} in\n{pgn}");
    }
}

/// A1-06/A2-02: the comment before a move comes after the variations of the move before, so it
/// keeps its place when read and written again (it used to be written before the variations, in
/// one comment with the move before's: `1. e4 {A B} (1. d4 d5) 1... e5`).
#[test]
fn a_comment_before_a_move_comes_after_the_variations_of_the_move_before() {
    let text = "1. e4 {A} (1. d4 d5) {B} 1... e5 *";
    let g = read(text);
    let e5 = g.main_line()[2];
    assert_eq!((g.comment(g.main_line()[1]), g.comment_before(e5)), ("A", "B"));
    assert_eq!(moves_of(&g.pgn()), format!("{text}\n"));
    assert_eq!(g.movetext(), "1. e4 {A} (1. d4 d5) {B} 1... e5");
    // Read back, the same tree.
    assert_eq!(tree(&read(&g.pgn())), tree(&g));
    // Nested: a comment before a move inside a variation, after that variation's own variations.
    let text = "1. e4 e5 (1... c5 2. Nf3 (2. c3) {Back to} 2... d6) 2. Nf3 *";
    let g = read(text);
    assert_eq!(moves_of(&g.pgn()), format!("{text}\n"));
}

/// A1-11: a long game is wrapped at 79 columns, and reads back the same.
#[test]
fn a_long_game_is_wrapped_at_79_columns_and_reads_back_the_same() {
    let mut g = Game::standard();
    for _ in 0..10 {
        for m in ["Nf3", "Nf6", "Ng1", "Ng8"] {
            g.play_san(m).unwrap();
        }
    }
    let node = g.current();
    g.set_comment(node, "A comment long enough to have been cut at the eightieth column\nof the line, and then some more words").unwrap();
    let pgn = g.pgn();
    assert_wrapped(&pgn);
    assert!(moves_of(&pgn).lines().count() > 3, "{pgn}");
    assert!(moves_of(&pgn).lines().all(|l| l.chars().count() <= LINE_WIDTH), "{pgn}");
    let back = read(&pgn);
    assert_eq!(tree(&back), tree(&g));
    assert_eq!(back.pgn(), pgn);
    // The move text alone stays on one line.
    assert_eq!(g.movetext().lines().count(), 1);
}

/// A piece longer than a line stands alone on its line, and a comment's word starting with `%`
/// never starts a line (PGN's escape).
#[test]
fn a_long_piece_stands_alone_and_no_line_starts_with_an_escape() {
    let mut g = Game::standard();
    g.play_san("e4").unwrap();
    let e4 = g.current();
    let long = "x".repeat(LINE_WIDTH + 10);
    g.set_comment(e4, &format!("short {long} words %escape {}", "%y ".repeat(40))).unwrap();
    let pgn = g.pgn();
    assert_wrapped(&pgn);
    assert!(moves_of(&pgn).lines().any(|l| l == long), "{pgn}");
    assert_eq!(tree(&read(&pgn)), tree(&g));
}

/// R6-08: `PlyCount` is written from the moves once they no longer have the length the tag was
/// read with (it stayed `80` after an edit).
#[test]
fn ply_count_follows_the_moves() {
    let mut text = String::from("[PlyCount \"80\"]\n\n");
    for n in 0..20 {
        text.push_str(&format!("{}. Nf3 Nf6 {}. Ng1 Ng8 ", 2 * n + 1, 2 * n + 2));
    }
    text.push('*');
    let mut g = read(&text);
    assert!(g.pgn().contains("[PlyCount \"80\"]"), "unchanged moves keep the tag as read");
    // The third move edited: the line now ends there.
    g.go_to(2);
    g.truncate();
    g.play_san("Nc3").unwrap();
    let pgn = g.pgn();
    assert!(pgn.contains("[PlyCount \"3\"]") && !pgn.contains("\"80\""), "{pgn}");
    // A tag that was wrong as read is left as it was while the moves are.
    let g = read("[PlyCount \"5\"]\n\n1. e4 e5 *");
    assert!(g.pgn().contains("[PlyCount \"5\"]"));
    // A reading that stopped in the main line: the moves written are not all the text's.
    let (g, _) = Game::from_pgn("[PlyCount \"4\"]\n\n1. e4 e5 2. Ke3 Nc6 *").unwrap();
    assert!(g.pgn().contains("[PlyCount \"2\"]"), "{}", g.pgn());
    // Set by hand, it is the moves' as they are then; without the tag, none is written.
    let mut g = read("1. e4 e5 *");
    assert!(!g.pgn().contains("PlyCount"));
    g.set_tag("PlyCount", "9").unwrap();
    assert!(g.pgn().contains("[PlyCount \"9\"]"));
    g.play_san("Nf3").unwrap();
    assert!(g.pgn().contains("[PlyCount \"3\"]"));
}

/// R6-08: a read `ECO` is kept until the main line changes before move 10.
#[test]
fn eco_is_left_out_once_the_opening_changes() {
    let text = "[ECO \"C65\"]\n\n1. e4 e5 2. Nf3 Nc6 3. Bb5 Nf6 4. O-O Nxe4 5. d4 Nd6 6. Bxc6 dxc6 7. dxe5 Nf5 8. Qxd8+ Kxd8 9. Nc3 Ke8 10. h3 h5 *";
    let mut g = read(text);
    assert!(g.pgn().contains("[ECO \"C65\"]"));
    // A change at move 10 keeps it.
    g.go_to(18);
    g.truncate();
    g.play_san("Rd1").unwrap();
    assert!(g.pgn().contains("[ECO \"C65\"]"), "{}", g.pgn());
    // A variation does not change the main line.
    g.go_to(4);
    g.play_san("Bc4").unwrap();
    assert!(g.pgn().contains("[ECO \"C65\"]"));
    // Promoted, it does: the third move is another.
    let bc4 = g.current();
    g.promote(bc4).unwrap();
    let pgn = g.pgn();
    assert!(!pgn.contains("ECO"), "{pgn}");
    assert_eq!(g.tag("ECO"), Some("C65"), "the tag is kept on the game, only not written");
    // Set again, it is the opening's as it is now.
    g.set_tag("ECO", "C50").unwrap();
    assert!(g.pgn().contains("[ECO \"C50\"]"));
    // A reading that stopped before move 10 has not read the opening the code was given for.
    let (g, _) = Game::from_pgn("[ECO \"C20\"]\n\n1. e4 e5 2. Ke3 *").unwrap();
    assert!(!g.pgn().contains("ECO"), "{}", g.pgn());
}

#[test]
fn figurines_in_comments_are_written_as_letters_when_asked() {
    let mut g = Game::standard();
    g.play_san("e4").unwrap();
    let e4 = g.current();
    g.set_comment(e4, "Threatens ♘f3, ♗c4 and ♙d4; ♚ is safe").unwrap();
    let options = WriteOptions { letters: true, ..WriteOptions::default() };
    assert!(write(&g, options).contains("{Threatens Nf3, Bc4 and d4; K is safe}"), "{}", write(&g, options));
    assert!(g.pgn().contains("{Threatens ♘f3, ♗c4 and ♙d4; ♚ is safe}"), "the board's PGN keeps them");
}

#[test]
fn tags_are_escaped_and_never_break_their_line() {
    let mut g = Game::standard();
    g.set_tag("Event", "Club \"open\" \\ 2026").unwrap();
    g.set_tag("Annotator", "a\u{7}b").unwrap();
    let pgn = g.pgn();
    assert!(pgn.starts_with("[Event \"Club \\\"open\\\" \\\\ 2026\"]\n[Site \"?\"]\n[Date \"????.??.??\"]"), "{pgn}");
    assert!(pgn.contains("[Annotator \"a b\"]\n"), "{pgn}");
    assert_eq!(read(&pgn).tag("Event"), Some("Club \"open\" \\ 2026"));
    // A control character read in a tag is the space it is written as, so what is written reads
    // back to itself (a `Date` of "\u{1}????.??.??" was written " ????.??.??", then "????.??.??").
    let g = read("[Date \"\u{1}????.??.??\"]\n[Round \"\u{b}1\"]\n\n1. e4 *");
    assert_eq!((g.tag("Date"), g.tag("Round")), (Some("????.??.??"), Some("1")));
    assert_eq!(read(&g.pgn()).pgn(), g.pgn());
    // Several games, a blank line between them.
    let two = write_games([&g, &g], WriteOptions::default());
    assert_eq!(crate::game::split_pgn(&two, 10).len(), 2);
}

#[test]
fn the_style_still_leaves_out_what_it_says() {
    let g = read("1. e4 {A} (1. d4 {B} d5) {C} 1... e5 $1 2. Nf3 *");
    let style = PgnStyle { variations: Variations::Depth(0), comments: false, nags: false };
    let pgn = write(&g, WriteOptions { style, ..WriteOptions::default() });
    assert_eq!(moves_of(&pgn), "1. e4 e5 2. Nf3 *\n");
    // Without the variation the comments before and after it are next to each other: one.
    let style = PgnStyle { variations: Variations::Depth(0), ..PgnStyle::FULL };
    assert_eq!(g.movetext_with(style), "1. e4 {A C} 1... e5 $1 2. Nf3");
}

/// Plan A9-F4, §2.5's guiding property on the D18 kit's games (the synthetic files of the
/// ChessBase session: every encoding, the glyphs of both sides, diagram marks, an exercise):
/// read, written and read again, the same tree, and the same text written again.
#[test]
fn the_kit_reads_back_the_same_after_it_is_written() {
    let kit: [&[u8]; 6] = [
        include_bytes!("../../../../../docs/chess/d18-kit/1-utf8.pgn"),
        include_bytes!("../../../../../docs/chess/d18-kit/2-utf8-com-bom.pgn"),
        include_bytes!("../../../../../docs/chess/d18-kit/3-windows-1252.pgn"),
        include_bytes!("../../../../../docs/chess/d18-kit/4-nags-das-pretas.pgn"),
        include_bytes!("../../../../../docs/chess/d18-kit/5-marcas-de-diagrama.pgn"),
        include_bytes!("../../../../../docs/chess/d18-kit/6-exercicio.pgn"),
    ];
    for bytes in kit {
        let (text, _) = crate::pgn::decode(bytes);
        for game in crate::game::split_pgn(&text, 10) {
            let g = read(game);
            let written = g.pgn();
            assert_wrapped(&written);
            let back = read(&written);
            assert_eq!(tree(&back), tree(&g), "{written}");
            assert_eq!(back.pgn(), written);
        }
    }
}

/// python-chess (GPL) as an outside oracle, run as a process only: a game written wrapped, with
/// long comments and a comment between a variation and the next move, reads as the same moves,
/// glyphs, tags and comments (python-chess joins a comment after `)` to the move before the
/// variation, as PGN readers do). Skipped without Python or python-chess.
#[test]
fn python_chess_reads_a_written_game_the_same() {
    let long = "a long comment that will need more than one line to be written at seventy-nine columns";
    let text = format!(
        "[White \"Réti, Richard\"]\n[ECO \"A09\"]\n\n{{Start}} 1. Nf3 {{{long}}} d5 (1... Nf6 $5 2. c4 {{{long}}}) {{Back}} 2. c4 $1 d4 3. b4 *"
    );
    let g = read(&text);
    let pgn = g.pgn();
    assert_wrapped(&pgn);
    let script = "import sys, chess.pgn\n\
        g = chess.pgn.read_game(sys.stdin)\n\
        def t(s): return ' '.join(s.split())\n\
        print(g.headers.get('White'), g.headers.get('ECO'))\n\
        print(t(g.comment))\n\
        def walk(n):\n\
        \x20   for c in n.variations:\n\
        \x20       print(c.san(), sorted(c.nags), '|' + t(c.starting_comment) + '|' + t(c.comment) + '|')\n\
        \x20       walk(c)\n\
        walk(g)\n\
        print(len(g.errors))";
    let child = std::process::Command::new("python")
        .args(["-I", "-c", script])
        .env("PYTHONIOENCODING", "utf-8")
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
    let got = String::from_utf8_lossy(&out.stdout).replace('\r', "");
    // Ours, in python-chess's order (a move, then each of its variations, depth first), the
    // comment before a first reply joined to the comment of the move before.
    let mut ours = vec!["Réti, Richard A09".to_owned(), g.comment(0).to_owned()];
    fn walk(g: &Game, id: NodeId, out: &mut Vec<String>) {
        for (i, &c) in g.replies(id).iter().enumerate() {
            let (before, mut after) = (if i > 0 { g.comment_before(c) } else { "" }, g.comment(c).to_owned());
            if let Some(&next) = g.replies(c).first()
                && !g.comment_before(next).is_empty()
            {
                after = if after.is_empty() { g.comment_before(next).to_owned() } else { format!("{after} {}", g.comment_before(next)) };
            }
            let nags: Vec<u16> = g.nags(c).to_vec();
            out.push(format!("{} {nags:?} |{before}|{after}|", g.san(c)));
            walk(g, c, out);
        }
    }
    walk(&g, 0, &mut ours);
    ours.push("0".to_owned());
    assert_eq!(got.lines().map(str::to_owned).collect::<Vec<_>>(), ours, "{pgn}");
}

/// The game of the frozen sample, as one line: long comments, a variation inside a variation, a
/// comment before a move after the variations of the move before, glyphs, a drawing, `PlyCount`
/// and `ECO`.
pub(crate) const FROZEN_SOURCE: &str = "[Event \"Compat F4\"]\n[White \"Réti, Richard\"]\n[Black \"Capablanca, José Raúl\"]\n[Result \"1-0\"]\n[ECO \"A09\"]\n[PlyCount \"13\"]\n[Annotator \"PdfCraft\"]\n\n{A long comment before the first move, long enough to be wrapped over more than one line of the PGN.} 1. Nf3 d5 2. c4 {Réti's opening: the c-pawn hits the centre from the wing, %100 hypermodern} (2. g3 Nf6 (2... c5 $5) 3. Bg2) {Back to the game:} 2... d4 {[%csl Gd4] The pawn on d4 cramps White.} 3. b4 $1 g6 4. g3 Bg7 5. d3 e5 6. Bg2 Ne7 7. O-O 1-0\n";

/// Plan A9-F4's written format (PGN in 79 columns), frozen: what this build writes for the sample
/// game, byte for byte (`crates/chess/tests/compat/pgn_79.pgn`). A change to the writer that
/// changes it changes what every build after it reads; the earlier builds' reading of it is
/// checked with `PDFCRAFT_OLD_CLI` in `crates/automation/tests/chess.rs`.
#[test]
fn compat_the_written_format_is_frozen() {
    const FROZEN: &str = include_str!("../../../tests/compat/pgn_79.pgn");
    let g = read(FROZEN_SOURCE);
    assert_eq!(g.pgn(), FROZEN);
    // Read back, it writes itself.
    assert_eq!(read(FROZEN).pgn(), FROZEN);
    assert_eq!(tree(&read(FROZEN)), tree(&g));
}

/// A random game: `choices` pick legal moves; every third move opens a variation from the
/// position before it, some moves get a comment before them, after them, or a glyph.
fn random_game(choices: &[(u16, u8)], comments: &[String]) -> Game {
    let mut g = Game::standard();
    for (i, &(choice, extra)) in choices.iter().enumerate() {
        let moves = g.legal_moves();
        let Some(m) = moves.get(usize::from(choice) % moves.len().max(1)) else { break };
        let before = g.current();
        if g.play_san(m).is_err() {
            break;
        }
        let at = g.current();
        let text = comments.get(i % comments.len().max(1)).cloned().unwrap_or_default();
        match extra % 5 {
            0 => {
                let _ = g.set_comment(at, &text);
            }
            1 => {
                let _ = g.set_comment_before(at, &text);
            }
            2 => g.toggle_nag(at, u16::from(extra)),
            _ => {}
        }
        if i % 3 == 2 {
            g.go_to_node(before);
            let alts = g.legal_moves();
            if let Some(alt) = alts.get(usize::from(choice).wrapping_add(1) % alts.len().max(1)) {
                let _ = g.play_san(alt);
            }
            g.go_to_node(at);
        }
    }
    g
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// §2.5's guiding property: a game written, read and written again is the same text, its
    /// lines wrapped, and the same tree, wherever PGN has room for what it holds (a comment
    /// before a first reply with no variation before it is read as the one after the move
    /// before, so the comparison is of what is written).
    #[test]
    fn written_games_read_back_the_same(
        choices in proptest::collection::vec((any::<u16>(), any::<u8>()), 0..80),
        comments in proptest::collection::vec("[ -~é♘{}()%;\n\t]{0,90}", 1..4),
        letters in any::<bool>(),
    ) {
        let g = random_game(&choices, &comments);
        let options = WriteOptions { letters, ..WriteOptions::default() };
        let pgn = write(&g, options);
        for line in moves_of(&pgn).lines() {
            prop_assert!(!line.starts_with('%'), "{}", pgn);
            prop_assert!(line.chars().count() <= LINE_WIDTH || !line.contains(' ') || line.contains(" %"), "{}", pgn);
        }
        let (back, report) = Game::from_pgn_report(&pgn).unwrap();
        prop_assert_eq!(&report, &PgnReport::default(), "{}", pgn);
        prop_assert_eq!(write(&back, options), pgn.clone());
        prop_assert_eq!(movetext(&back, WriteOptions::default()), movetext(&g, options));
    }
}
