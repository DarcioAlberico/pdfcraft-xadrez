//! Fragment mode (PGN toolbar spec T2a): selections read by the walker into fragment items, and
//! grafted with `pdfcraft_chess::game::Game::graft` to check what they make.

use std::sync::Arc;

use pdfcraft_chess::fragment::{FragmentItem, MAX_FRAGMENT_ITEMS};
use pdfcraft_chess::game::{Game as ChessGame, GraftMode};
use pdfcraft_render::text::TextGlyph;

use super::*;
use crate::doc::fragment::{FragmentOptions, FragmentRead, line_items, read, text_of_glyphs};

/// The position before White's 18th move in the spec's scenario (§4.10).
const FEN: &str = "2r2rk1/1p2ppb1/p2p1npp/q4P2/P1b1P2B/2NB4/1PP3PP/R3QR1K w - - 0 18";

/// Glyph advance and line pitch, in points.
const W: f32 = 5.0;
const PITCH: f32 = 14.0;

/// A selection's glyphs: one line per string, a character a glyph, `*` switching bold on and off
/// (it is not a glyph). A space is a word gap.
fn glyphs(lines: &[&str]) -> Vec<TextGlyph> {
    let mut out = Vec::new();
    let mut bold = false;
    for (row, line) in lines.iter().enumerate() {
        let y = 100.0 + row as f32 * PITCH;
        let mut x = 40.0;
        for c in line.chars() {
            match c {
                '*' => bold = !bold,
                ' ' => x += W,
                _ => {
                    let rect = [x, y, x + W, y + 10.0];
                    let quad = [[rect[0], rect[3]], [rect[2], rect[3]], [rect[2], rect[1]], [rect[0], rect[1]]];
                    let font: Arc<str> = Arc::from(if bold { "Times-Bold" } else { "Times-Roman" });
                    let code = u32::from(c);
                    out.push(TextGlyph::new(c.to_string(), rect, quad, [1.0, 0.0]).with_source(Some(font), (code <= 0xFF).then_some(code)));
                    x += W;
                }
            }
        }
    }
    out
}

fn options() -> FragmentOptions {
    FragmentOptions { geometry: Some(geometry()), weight: true }
}

/// Read `lines` (see [`glyphs`]) from `fen` in English.
fn read_lines(lines: &[&str], fen: &str) -> FragmentRead {
    read(&text_of_glyphs(&glyphs(lines), Some(Lang::En)), fen, &options())
}

/// The game `items` make grafted at the start of `fen`, as movetext.
fn grafted(fen: &str, items: &[FragmentItem]) -> String {
    let game = ChessGame::from_fen(fen).unwrap();
    let (game, report) = game.graft(0, GraftMode::Follow, items, Some(0));
    assert_eq!(report.stop, None, "{items:?}");
    assert!(report.unbalanced.is_empty(), "{items:?}");
    game.movetext()
}

/// The movetext without comments, and without the number PGN gives a Black move again after a
/// move's comment.
fn moves_only(movetext: &str) -> String {
    // Comments become one `{}` token each.
    let mut flat = String::new();
    let mut depth = 0usize;
    for c in movetext.chars() {
        match c {
            '{' => {
                if depth == 0 {
                    flat.push_str(" {} ");
                }
                depth += 1;
            }
            '}' => depth = depth.saturating_sub(1),
            '(' | ')' if depth == 0 => {
                flat.push(' ');
                flat.push(c);
                flat.push(' ');
            }
            _ if depth == 0 => flat.push(c),
            _ => {}
        }
    }
    let words: Vec<&str> = flat.split_whitespace().collect();
    let mut kept: Vec<&str> = Vec::new();
    for (k, w) in words.iter().enumerate() {
        if *w == "{}" {
            continue;
        }
        let renumbered = w.ends_with("...")
            && w.trim_end_matches('.').chars().all(|c| c.is_ascii_digit())
            && k >= 2
            && words[k - 1] == "{}"
            && !matches!(words[k - 2], "(" | ")");
        if !renumbered {
            kept.push(w);
        }
    }
    kept.join(" ").replace("( ", "(").replace(" )", ")")
}

/// The scenario's paragraph, set as the book sets it: the game's moves in bold.
const SCENARIO: &[&str] = &[
    "*18.Nd5!* *Qxe1* After 18…Qd8 19.Bxf6! Bxf6 20.fxg6 (less good is",
    "20.Nxf6+, as Black has nothing to fear after 20…exf6 21.Bxc4",
    "Rxc4) 20…fxg6 21.Qg3, White obtains a dangerous initiative. For",
    "example: 21…Kh7 22.e5!. *19.Nxe7+ Kh7 20.Raxe1*",
];

#[test]
fn the_scenario_grafts_as_the_main_line_with_its_variation() {
    let read = read_lines(SCENARIO, FEN);
    assert!(read.start_ok);
    assert_eq!(read.stops, Vec::new());
    assert_eq!(read.result, None);
    assert_eq!(read.played, 17);
    let movetext = grafted(FEN, &read.items);
    assert_eq!(
        moves_only(&movetext),
        "18. Nd5 $1 Qxe1 (18... Qd8 19. Bxf6 $1 Bxf6 20. fxg6 (20. Nxf6+ exf6 21. Bxc4 Rxc4) 20... fxg6 21. Qg3 Kh7 22. e5 $1) 19. Nxe7+ Kh7 20. Raxe1"
    );
    // The prose is in comments, where it was printed.
    assert!(movetext.contains("{After}"), "{movetext}");
    assert!(movetext.contains("{less good is} 20. Nxf6+ {as Black has nothing to fear after}"), "{movetext}");
    assert!(movetext.contains("21. Qg3 {White obtains a dangerous initiative. For example:}"), "{movetext}");
    // The moves carry their printed half-move, weight and box.
    let first = read.items.iter().find_map(|i| match i {
        FragmentItem::Move { ply, text, bold, rect, .. } => Some((*ply, text.clone(), *bold, *rect)),
        _ => None,
    });
    let (ply, text, bold, rect) = first.unwrap();
    assert_eq!((ply, text.as_str(), bold), (Some(34), "18.Nd5", Some(true)));
    let [x0, y0, x1, y1] = rect.unwrap();
    assert!(x0 >= 40.0 && x1 > x0 && y1 > y0 && y1 <= PAGE_H - 100.0 + 1.0, "{rect:?} is in user space");
    let qd8 = read.items.iter().find_map(|i| match i {
        FragmentItem::Move { ply, text, bold, .. } if text.ends_with("Qd8") => Some((*ply, *bold)),
        _ => None,
    });
    assert_eq!(qd8, Some((Some(35), Some(false))));
}

#[test]
fn without_the_weight_the_first_chain_stays_the_main_line_and_moves_say_nothing() {
    let text = text_of_glyphs(&glyphs(SCENARIO), Some(Lang::En));
    let read = read(&text, FEN, &FragmentOptions { geometry: None, weight: false });
    assert!(read.items.iter().all(|i| !matches!(i, FragmentItem::Move { bold: Some(_), .. } | FragmentItem::Move { rect: Some(_), .. })));
    assert_eq!(read.stops, Vec::new());
}

#[test]
fn a_first_move_quoted_in_prose_does_not_end_the_fragment() {
    let read = read_lines(&["18.Nd5 Qxe1 as after 1.e4 e5 2.Nf3 in the opening, 19.Nxe7+ Kh7"], FEN);
    assert_eq!(read.stops, Vec::new());
    let movetext = grafted(FEN, &read.items);
    assert_eq!(moves_only(&movetext), "18. Nd5 Qxe1 19. Nxe7+ Kh7");
    assert!(movetext.contains("{as after 1.e4 e5 2.Nf3 in the opening,}"), "{movetext}");
}

#[test]
fn a_lost_closing_bracket_is_assumed_and_flagged() {
    // `Rxc4` should be followed by `)`: OCR lost it.
    let read = read_lines(&["18.Nd5 Qxe1 (18…Qd8 19.Bxf6 Bxf6 20.fxg6 (20.Nxf6+ exf6 21.Bxc4 Rxc4 20…fxg6 21.Qg3) 19.Nxe7+"], FEN);
    assert_eq!(read.stops, Vec::new());
    assert_eq!(read.flags, vec![ReviewFlag::ImpliedClose { text: "20...fxg6".to_owned() }]);
    assert_eq!(
        moves_only(&grafted(FEN, &read.items)),
        "18. Nd5 Qxe1 (18... Qd8 19. Bxf6 Bxf6 20. fxg6 (20. Nxf6+ exf6 21. Bxc4 Rxc4) 20... fxg6 21. Qg3) 19. Nxe7+"
    );
}

#[test]
fn a_move_without_a_number_in_prose_stays_in_the_comment() {
    // `Kh7` after prose words is prose; `Qxe1` right after `18.Nd5` and `…Kh7` after `19.Nxe7+`
    // continue the run.
    let read = read_lines(&["18.Nd5 Qxe1 19.Nxe7+ when the king goes Kh7 is forced. 19…Kh7 20.Raxe1"], FEN);
    let movetext = grafted(FEN, &read.items);
    assert_eq!(moves_only(&movetext), "18. Nd5 Qxe1 19. Nxe7+ Kh7 20. Raxe1");
    assert!(movetext.contains("{when the king goes Kh7 is forced.}"), "{movetext}");
    assert_eq!(read.stops, Vec::new());
}

#[test]
fn a_bold_move_after_prose_is_still_the_main_line() {
    let read = read_lines(&["*18.Nd5!* A strong jump. *Qxe1 19.Nxe7+*"], FEN);
    assert_eq!(moves_only(&grafted(FEN, &read.items)), "18. Nd5 $1 Qxe1 19. Nxe7+");
}

#[test]
fn labels_and_semicolons_separate_sibling_variations() {
    // Without the rule `b)` would close the bracket, and `Kh7` would be White's. (`(a)` with
    // nothing between is a label in brackets, as below.)
    let read = read_lines(&["18.Nd5 Qxe1 ( a) 18…Qd8 19.Bxf6 b) Kh7 19.Nxe7) 19.Nxe7+"], FEN);
    assert_eq!(read.stops, Vec::new());
    assert_eq!(moves_only(&grafted(FEN, &read.items)), "18. Nd5 Qxe1 (18... Qd8 19. Bxf6) (18... Kh7 19. Nxe7) 19. Nxe7+");
    // Labels in brackets, as some books print them, open nothing either.
    let read = read_lines(&["18.Nd5 Qxe1 ((a) 18…Qd8 19.Bxf6 (b) Kh7 19.Nxe7) 19.Nxe7+"], FEN);
    assert_eq!(read.stops, Vec::new());
    assert_eq!(moves_only(&grafted(FEN, &read.items)), "18. Nd5 Qxe1 (18... Qd8 19. Bxf6) (18... Kh7 19. Nxe7) 19. Nxe7+");
    // `;` before a move without a number: without the rule `Kh7` would go on after `19.Bxf6`.
    let read = read_lines(&["18.Nd5 Qxe1 (18…Qd8 19.Bxf6; Kh7 19.Nxe7) 19.Nxe7+"], FEN);
    assert_eq!(read.stops, Vec::new());
    assert_eq!(moves_only(&grafted(FEN, &read.items)), "18. Nd5 Qxe1 (18... Qd8 19. Bxf6) (18... Kh7 19. Nxe7) 19. Nxe7+");
    let read = read_lines(&["18.Nd5 Qxe1 (18…Qd8 19.Bxf6; 18…Kh7 19.Nxe7; but not in prose) 19.Nxe7+"], FEN);
    assert_eq!(read.stops, Vec::new());
    let movetext = grafted(FEN, &read.items);
    assert_eq!(moves_only(&movetext), "18. Nd5 Qxe1 (18... Qd8 19. Bxf6) (18... Kh7 19. Nxe7) 19. Nxe7+");
    assert!(movetext.contains("{; but not in prose}") || movetext.contains("{but not in prose}"), "{movetext}");
}

#[test]
fn every_ellipsis_form_is_blacks_move() {
    for form in ["18…Qxe1", "18...Qxe1", "18. ...Qxe1", "18. …Qxe1", "18.…Qxe1", "18.… Qxe1", "18. … Qxe1"] {
        let read = read_lines(&[&format!("18.Nd5 {form} 19.Nxe7+")], FEN);
        assert_eq!(moves_only(&grafted(FEN, &read.items)), "18. Nd5 Qxe1 19. Nxe7+", "{form}");
        let ply = read.items.iter().find_map(|i| match i {
            FragmentItem::Move { ply, san, .. } if san.to_english() == "Qxe1" => Some(*ply),
            _ => None,
        });
        assert_eq!(ply, Some(Some(35)), "{form}");
    }
    // An ellipsis alone before a move says it is Black's: here the reply in brackets to 18.Nd5.
    let read = read_lines(&["18.Nd5 (…Kh7 19.Nxe7) Qxe1"], FEN);
    assert_eq!(read.stops, Vec::new());
    assert_eq!(moves_only(&grafted(FEN, &read.items)), "18. Nd5 Qxe1 (18... Kh7 19. Nxe7)");
}

#[test]
fn results_end_the_fragment_and_what_follows_is_left_over() {
    for (text, rest) in [
        ("18.Nd5 Qxe1 1-0 19.Nxe7+ Kh7", "19.Nxe7+ Kh7"),
        ("18.Nd5 Qxe1 1:0. Then 19.Nxe7+", "Then 19.Nxe7+"),
        ("18.Nd5 Qxe1 (1-0) and 19.Nxe7+", "and 19.Nxe7+"),
        ("18.Nd5 Qxe1 ½-½", ""),
    ] {
        let read = read_lines(&[text], FEN);
        assert!(read.result.is_some(), "{text}");
        assert_eq!(read.rest.split_whitespace().collect::<Vec<_>>().join(" "), rest, "{text}");
        let game = ChessGame::from_fen(FEN).unwrap();
        let (game, report) = game.graft(0, GraftMode::Follow, &read.items, None);
        assert_eq!(moves_only(&game.movetext()), "18. Nd5 Qxe1", "{text}");
        assert_eq!(report.leftover_text.split_whitespace().collect::<Vec<_>>().join(" "), rest, "{text}: the graft reports what follows the result");
    }
    // In a variation a result is prose.
    let read = read_lines(&["18.Nd5 (18.Bxf6 Bxf6 1-0) Qxe1"], FEN);
    assert_eq!(read.result, None);
    assert_eq!(moves_only(&grafted(FEN, &read.items)), "18. Nd5 (18. Bxf6 Bxf6) 18... Qxe1");
}

#[test]
fn a_year_in_a_source_is_never_a_move_number() {
    let read = read_lines(&["18.Nd5 Qxe1 Karpov - Kasparov, Moscow 1985. 19.Nxe7+ Kh7 (Moscow 1985)"], FEN);
    assert_eq!(read.stops, Vec::new());
    let movetext = grafted(FEN, &read.items);
    assert_eq!(moves_only(&movetext), "18. Nd5 Qxe1 19. Nxe7+ Kh7");
    assert!(movetext.contains("Moscow 1985"), "{movetext}");
}

#[test]
fn hyphens_join_words_but_never_moves() {
    let read = read_lines(&["18.Nd5 a strong posi-", "tion on the e-", "file 18…Qxe1 19.e4-", "e5 Kh7 on the 2nd-", "rank"], FEN);
    let movetext = grafted(FEN, &read.items);
    assert!(movetext.contains("{a strong position on the e-file}"), "{movetext}");
    assert!(movetext.contains("2nd- rank"), "letters after a digit are no word's end: {movetext}");
    // `e4-` and `e5` stay apart (and `e4-` is no move: this position has a pawn on e4 already).
    assert!(!movetext.contains("e4e5") && !movetext.contains("e4-e5"), "{movetext}");
    // Directly on lines: `Nd7-` `b6`, `0-` `1`, `+-` are never joined.
    let lines =
        vec![line(100.0, "after Nd7-"), line(114.0, "b6 the score 0-"), line(128.0, "1 or +-"), line(142.0, "and posi-"), line(156.0, "tion")];
    let joined = crate::doc::fragment::join_hyphens(&lines);
    let texts: Vec<String> = joined.iter().map(|l| l.spans.iter().map(|s| format!("{:?}", s.run)).collect::<Vec<_>>().join("|")).collect();
    assert!(texts[0].ends_with("Text(\"after Nd7-\")"), "{texts:?}");
    assert!(texts[1].ends_with("Text(\"the score 0-\")"), "{texts:?}");
    assert!(texts[2].starts_with("Text(\"1 or\")"), "{texts:?}");
    assert_eq!(texts[3], "Text(\"and position\")", "{texts:?}");
    assert_eq!(joined[4].spans.len(), 0, "{texts:?}");
}

#[test]
fn the_selection_is_read_in_the_documents_language_never_a_guessed_one() {
    // `Rc7` is a rook in English and a king (rei) in Portuguese; the knight letters (`C`) of
    // Portuguese would win a guess.
    let text = ["1.Cf3 Cc6 2.Cc3 Cd4 3.Rc7"];
    let san = |t: &ChessPageText| {
        t.lines
            .iter()
            .flat_map(|l| &l.spans)
            .filter_map(|s| if let ChessRun::Move { san, .. } = &s.run { Some(san.to_english()) } else { None })
            .collect::<Vec<_>>()
    };
    assert_eq!(san(&text_of_glyphs(&glyphs(&text), None)), vec!["Rc7"]);
    assert_eq!(san(&text_of_glyphs(&glyphs(&text), Some(Lang::Pt))), vec!["Nf3", "Nc6", "Nc3", "Nd4", "Kc7"]);
    let page = crate::chess_text::from_glyphs(&glyphs(&text), None);
    assert_eq!(page.lang.lang, Lang::Pt, "a page's own reading still guesses");
}

#[test]
fn moves_that_do_not_play_are_kept_as_stops_and_comments() {
    let read = read_lines(&["18.Nd5 Qxe1 19.Nxe8 Kh7 20.Raxe1"], FEN);
    assert_eq!(
        read.stops.iter().map(|s| (s.text.as_str(), s.status)).collect::<Vec<_>>(),
        vec![("19.Nxe8", MoveStatus::Illegal), ("Kh7", MoveStatus::NotReached), ("20.Raxe1", MoveStatus::NotReached),]
    );
    assert!(read.stops.iter().all(|s| s.rect.is_some()));
    // The illegal move keeps the position it was tried in (after 18...Qxe1); the moves not
    // reached have none.
    let tried = read.stops.first().and_then(|s| s.fen.as_deref()).expect("the position 19.Nxe8 was tried in");
    assert!(tried.starts_with("2r2rk1/1p2ppb1/p2p1npp/3N1P2/P1b1P2B/3B4/1PP3PP/R3qR1K w"), "{tried}");
    assert!(read.stops.iter().skip(1).all(|s| s.fen.is_none()));
    let movetext = grafted(FEN, &read.items);
    assert_eq!(moves_only(&movetext), "18. Nd5 Qxe1 20. Raxe1".replace(" 20. Raxe1", ""), "{movetext}");
    assert!(movetext.contains("19.Nxe8") && movetext.contains("Kh7"), "{movetext}");
}

#[test]
fn an_illegal_start_position_reads_nothing_and_keeps_the_text() {
    let read = read_lines(&["18.Nd5 Qxe1"], "8/8/8/8/8/8/8/8 w - - 0 1");
    assert!(!read.start_ok);
    assert!(read.items.is_empty());
    assert_eq!(read.rest, "18.Nd5 Qxe1");
}

#[test]
fn whole_book_reading_takes_only_the_book_rules() {
    // A move after prose words: the book's rules play it (books go on after prose without
    // numbers); all the rules, as a fragment, leave it in the comment. The lost bracket is
    // assumed by both.
    let reading_fen = "2r2rk1/1p2ppb1/p2p1npp/q4P2/P1b1P2B/2NB4/1PP3PP/R3QR1K w - - 0 1";
    let text = page_text(vec![line(
        250.0,
        "18.Nd5 Qxe1 ( 18...Qd8 19.Bxf6 Bxf6 20.fxg6 ( 20.Nxf6+ exf6 21.Bxc4 Rxc4 20...fxg6 21.Qg3 ) 19.Nxe7+ and then Kh7",
    )]);
    let diagrams = [reading(0, 40.0, reading_fen, true, false)];
    let page = |mut builder: DocBuilder| {
        builder.add_page(&input(0, &text, &diagrams, &[], &[]));
        builder.finish()
    };
    let kh7 = |doc: &ChessDoc| moves(&doc.pages[0]).into_iter().find(|(t, _)| t == "Kh7").map(|(_, s)| s);
    let implied = |doc: &ChessDoc| doc.games.iter().flat_map(|g| &g.flags).any(|f| matches!(f.flag, ReviewFlag::ImpliedClose { .. }));
    assert_eq!(WalkRules::BOOK, WalkRules::default());
    let book = page(DocBuilder::new());
    assert_eq!(kh7(&book), Some(MoveStatus::Valid));
    assert!(implied(&book));
    let all = page(DocBuilder::with_rules(WalkRules::ALL));
    assert_eq!(kh7(&all), Some(MoveStatus::Unchecked));
    assert!(implied(&all));
    let none = page(DocBuilder::with_rules(WalkRules::NONE));
    assert!(!implied(&none));
}

#[test]
fn line_items_write_a_variation_of_a_book_game() {
    // The book's game, read whole; its variation from 18...Qd8, written as items and grafted
    // from the position before it, is the variation with the one inside it, and nothing of the
    // main line.
    let reading_fen = "2r2rk1/1p2ppb1/p2p1npp/q4P2/P1b1P2B/2NB4/1PP3PP/R3QR1K w - - 0 1";
    let text =
        page_text(vec![line(250.0, "18.Nd5 ! Qxe1 ( 18...Qd8 19.Bxf6 Bxf6 20.fxg6 ( 20.Nxf6+ exf6 21.Bxc4 Rxc4 ) 20...fxg6 21.Qg3 ) 19.Nxe7+ Kh7")]);
    let diagrams = [reading(0, 40.0, reading_fen, true, false)];
    let doc = build_page(&input(0, &text, &diagrams, &[], &[]));
    let score = &doc.games[0].score;
    let head = score.nodes.iter().position(|n| n.san == "Qd8").unwrap();
    let parent = score.nodes[head].parent.unwrap();
    let fen = &score.nodes[parent].fen;
    let items = line_items(score, head);
    assert_eq!(moves_only(&grafted(fen, &items)), "18... Qd8 19. Bxf6 Bxf6 20. fxg6 (20. Nxf6+ exf6 21. Bxc4 Rxc4) 20... fxg6 21. Qg3");
    // The whole game from its first move, glyph included.
    let start = score.start_fen.as_deref().unwrap();
    assert_eq!(
        moves_only(&grafted(start, &line_items(score, score.roots[0]))),
        "18. Nd5 $1 Qxe1 (18... Qd8 19. Bxf6 Bxf6 20. fxg6 (20. Nxf6+ exf6 21. Bxc4 Rxc4) 20... fxg6 21. Qg3) 19. Nxe7+ Kh7"
    );
}

#[test]
fn a_number_cut_from_its_move_is_given_back_to_it() {
    // Paragraphs read before can end with a move's number and start with its move (`… 19.` |
    // `Nxe7+`): the move keeps its number, so it is no move in prose.
    let mv = |number: Option<(u16, bool)>, san: &str| {
        Inline::Move(MoveInline {
            number: number.map(|(number, black)| MoveNumber { number, black }),
            text: san.to_owned(),
            san: san.to_owned(),
            status: MoveStatus::Unchecked,
            fen: None,
            game: None,
            source_rect: None,
            bold: false,
            review: Vec::new(),
            reference: None,
        })
    };
    let text = |t: &str| Inline::Text { text: t.to_owned(), source_rect: None, review: Vec::new() };
    let inlines = vec![mv(Some((18, false)), "Nd5"), mv(None, "Qxe1"), text("and then 19."), mv(None, "Nxe7+"), text("19..."), mv(None, "Kh7")];
    let read = crate::doc::fragment::read_inlines(&inlines, FEN, false);
    assert_eq!(read.stops, Vec::new());
    let movetext = grafted(FEN, &read.items);
    assert_eq!(moves_only(&movetext), "18. Nd5 Qxe1 19. Nxe7+ Kh7");
    assert!(movetext.contains("{and then}"), "{movetext}");
    let plies: Vec<Option<u32>> = read.items.iter().filter_map(|i| if let FragmentItem::Move { ply, .. } = i { Some(*ply) } else { None }).collect();
    assert_eq!(plies, vec![Some(34), None, Some(36), Some(37)]);
}

#[test]
fn glyphs_are_written_by_whites_code() {
    // `↑` after a Black move is `$37` in the walker's game; a fragment item carries White's
    // `$36`, and the graft writes Black's again.
    let read = read_lines(&["18.Nd5 ↑ Qxe1 ↑"], FEN);
    let nags: Vec<u16> = read.items.iter().filter_map(|i| if let FragmentItem::Nag(c) = i { Some(*c) } else { None }).collect();
    assert_eq!(nags, vec![36, 36]);
    assert_eq!(moves_only(&grafted(FEN, &read.items)), "18. Nd5 $36 Qxe1 $37");
}

fn arb_word() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("18.Nd5".to_owned()),
        Just("Qxe1".to_owned()),
        Just("18…Qd8".to_owned()),
        Just("19.Bxf6!".to_owned()),
        Just("Bxf6".to_owned()),
        Just("20.fxg6".to_owned()),
        Just("…Kh7".to_owned()),
        Just("1.e4".to_owned()),
        Just("(".to_owned()),
        Just(")".to_owned()),
        Just("[".to_owned()),
        Just("]".to_owned()),
        Just(";".to_owned()),
        Just("a)".to_owned()),
        Just("b)".to_owned()),
        Just("1-0".to_owned()),
        Just("(1:0)".to_owned()),
        Just("posi-".to_owned()),
        Just("the".to_owned()),
        Just("…".to_owned()),
        Just("!?".to_owned()),
        Just("+-".to_owned()),
        Just("1985".to_owned()),
        "[a-hKQRBNx1-8+#=O-]{1,6}",
        "[0-9]{1,3}\\.{0,3}",
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn random_selections_read_within_the_limits_and_graft(words in proptest::collection::vec(arb_word(), 0..80), cut in 1usize..12, start in 0usize..2) {
        let lines: Vec<String> = words.chunks(cut).map(|c| c.join(" ")).collect();
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let fen = [FEN, "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"][start];
        let read = read_lines(&refs, fen);
        prop_assert!(read.items.len() <= MAX_FRAGMENT_ITEMS);
        let game = ChessGame::from_fen(fen).unwrap();
        let (_, report) = game.graft(0, GraftMode::Follow, &read.items, Some(0));
        // The items replay the walker's tree exactly.
        prop_assert!(report.unbalanced.is_empty());
        prop_assert_eq!(report.stop, None);
        prop_assert_eq!(report.played, read.played);
    }
}
