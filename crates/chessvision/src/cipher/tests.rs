use pdfcraft_chess::placement::PieceKind;
use pdfcraft_chess::repair::{CipherMap, GameContext, Proof, RepairKind};
use pdfcraft_chess::san::Lang;
use proptest::prelude::*;

use super::*;
use crate::chess_text::ChessRun;
use crate::glyphs::fuse::{WordSource, to_chess_text};

/// The Opera game (Morphy, Paris 1858).
const OPERA: &str = "1.e4 e5 2.Nf3 d6 3.d4 Bg4 4.dxe5 Bxf3 5.Qxf3 dxe5 6.Bc4 Nf6 7.Qb3 Qe7 8.Nc3 c6 9.Bg5 b5 10.Nxb5 cxb5 \
                     11.Bxb5+ Nbd7 12.O-O-O Rd8 13.Rxd7 Rxd7 14.Rd1 Qe6 15.Bxd7+ Nxd7 16.Qb8+ Nxb8 17.Rd8#";

/// Glyph-read lines of `text`, eight words a line, ten pixels a glyph.
fn lines(text: &str) -> Vec<FusedLine> {
    let words: Vec<&str> = text.split_whitespace().collect();
    words
        .chunks(8)
        .enumerate()
        .map(|(li, chunk)| {
            let y = u32::try_from(li).unwrap() * 30;
            let mut x = 0;
            let words: Vec<FusedWord> = chunk
                .iter()
                .map(|w| {
                    let glyphs: Vec<Glyph> = w
                        .chars()
                        .map(|c| {
                            let rect = PixelRect { x0: x, y0: y, x1: x + 10, y1: y + 20 };
                            x += 10;
                            Glyph { rect, text: c.to_string(), p: 0.9, alternatives: Vec::new(), margin: 0.8 }
                        })
                        .collect();
                    x += 10;
                    let rect = glyphs.iter().map(|g| g.rect).reduce(union_rect).unwrap();
                    FusedWord { text: (*w).to_owned(), rect, source: WordSource::Glyphs, glyphs, other: None }
                })
                .collect();
            let rect = words.iter().map(|w| w.rect).reduce(union_rect).unwrap();
            FusedLine { words, rect }
        })
        .collect()
}

fn text(lines: &[FusedLine]) -> String {
    lines.iter().map(FusedLine::text).collect::<Vec<_>>().join(" ")
}

#[test]
fn a_row_is_settled_by_five_legal_proofs_or_ten_visual_ones() {
    // Ported from Caïssa's `test_book_cipher.py`.
    let mut book = BookCipher::default();
    for _ in 0..4 {
        book.observe("W", PieceKind::Queen, Source::Legality, None);
    }
    assert!(book.proven().is_empty(), "four legality proofs are not five");
    book.observe("W", PieceKind::Queen, Source::Legality, None);
    assert_eq!(book.proven(), CipherMap::from([("W".into(), PieceKind::Queen)]));
    for _ in 0..9 {
        book.observe("H", PieceKind::Rook, Source::Glyph, None);
    }
    assert!(!book.proven().contains_key("H"), "nine visual proofs are not ten");
    book.observe("H", PieceKind::Rook, Source::Glyph, None);
    assert_eq!(book.proven().get("H"), Some(&PieceKind::Rook));
    book.observe("W", PieceKind::Bishop, Source::Legality, None);
    assert!(!book.proven().contains_key("W"), "one contradiction demotes a legality row");
    assert_eq!(book.entries()["W"].majority(), Some((Officer::Q, 5, 1)));
}

#[test]
fn a_visual_row_tolerates_the_readers_error_rate_and_no_more() {
    let mut book = BookCipher::default();
    for _ in 0..50 {
        book.observe("S", PieceKind::King, Source::Glyph, None);
    }
    book.observe("S", PieceKind::Knight, Source::Glyph, None);
    assert_eq!(book.proven().get("S"), Some(&PieceKind::King), "1 in 51 is the glyph reader's own error rate");
    book.observe("S", PieceKind::Knight, Source::Glyph, None);
    assert!(!book.proven().contains_key("S"), "2 in 52 is above 2 %");
    // The row's piece is the majority, not the first observation.
    let mut book = BookCipher::default();
    for _ in 0..3 {
        book.observe("'it", PieceKind::Queen, Source::Glyph, None);
    }
    for _ in 0..26 {
        book.observe("'it", PieceKind::King, Source::Glyph, None);
    }
    assert_eq!(book.entries()["'it"].majority(), Some((Officer::K, 26, 3)));
    assert!(book.proven().is_empty(), "3 in 29 is above the 2 % share");
}

#[test]
fn tables_round_trip_and_drop_what_they_cannot_hold() {
    let mut book = BookCipher::default();
    for page in 0..5 {
        book.observe("W", PieceKind::Queen, Source::Legality, Some(Example { page, raw: "Wd5".into(), san: "Qd5".into() }));
    }
    book.observe("W", PieceKind::Pawn, Source::Legality, None);
    book.observe("too long", PieceKind::Rook, Source::Legality, None);
    assert_eq!(book.entries().len(), 1, "a pawn and a symbol with a space are not cipher rows");
    let json = book.to_json().unwrap();
    let back = BookCipher::from_json(&json).unwrap();
    assert_eq!(back, book);
    assert_eq!(back.entries()["W"].examples.len(), 5);
    // Another program's file: unknown pieces are refused, oversized parts are cut.
    assert!(BookCipher::from_json(r#"{"entries":{"W":{"evidence":{"P":{"legality":9}}}}}"#).is_err());
    assert!(BookCipher::from_json("not json").is_err());
    let many: Vec<String> = (0..200).map(|i| format!(r#""s{i}":{{"evidence":{{"Q":{{"glyph":1}}}}}}"#)).collect();
    let big = BookCipher::from_json(&format!(r#"{{"entries":{{{},"a b":{{}}}}}}"#, many.join(","))).unwrap();
    assert_eq!(big.entries().len(), MAX_ENTRIES);
    assert!(matches!(BookCipher::from_json(&" ".repeat(MAX_JSON_BYTES + 1)), Err(CipherError::TooLarge(_))));
}

#[test]
fn the_opera_with_queens_read_as_w_is_proved_and_decoded_in_its_boxes() {
    let read = lines(&OPERA.replace('Q', "W"));
    let out = repair_lines(&read, None, Lang::En, &CipherMap::new());
    assert_eq!(out.context, ContextUsed::GameStart, "the moves start at 1.");
    let Some(Proof::Proven { assignment, observations, .. }) = &out.proof else { panic!("{:?}", out.proof) };
    assert_eq!(assignment, &CipherMap::from([("W".into(), PieceKind::Queen)]));
    assert_eq!(observations.len(), 5);
    assert_eq!(out.edits.len(), 5);
    assert!(out.flags.is_empty());
    assert!(out.edits.iter().all(|e| e.kind == RepairKind::Cipher && e.san.as_ref().is_some_and(|s| s.to_english().starts_with('Q'))));
    assert_eq!(text(&out.lines), OPERA.replace('Q', "♛"));
    // Every box is where it was; only the symbol's glyph changed.
    let first = &out.edits[0];
    let (before, after) = (&read[first.line].words[first.word], &out.lines[first.line].words[first.word]);
    assert_eq!((before.rect, after.glyphs.len()), (after.rect, before.glyphs.len()));
    assert_eq!((after.glyphs[2].text.as_str(), after.glyphs[2].rect), ("♛", before.glyphs[2].rect));
    // The book keeps the proof: five legality observations settle the symbol.
    let mut book = BookCipher::default();
    book.record(out.proof.as_ref().unwrap(), 7);
    assert_eq!(book.proven(), CipherMap::from([("W".into(), PieceKind::Queen)]));
    assert_eq!(book.entries()["W"].examples[0], Example { page: 7, raw: "5.Wxf3".into(), san: "Qxf3".into() });
    // And the decoded lines read as chess text, the queen move included.
    let chess = to_chess_text(&out.lines, 72.0, None);
    let moves: Vec<String> = chess
        .lines
        .iter()
        .flat_map(|l| &l.spans)
        .filter_map(|s| if let ChessRun::Move { san, .. } = &s.run { Some(san.to_english()) } else { None })
        .collect();
    assert!(moves.contains(&"Qxf3".to_owned()), "{moves:?}");
}

#[test]
fn without_a_position_nothing_is_repaired() {
    // Not numbered from 1, no diagram: `Nf8` stays as read, though only Nf3 would be legal from
    // the initial position.
    let read = lines("12.Nf8 e5 13.Bxb5 Wd1");
    let out = repair_lines(&read, None, Lang::En, &CipherMap::new());
    assert_eq!(out.context, ContextUsed::None);
    assert_eq!((out.lines, out.edits.len(), out.flags.len(), out.proof), (read, 0, 0, None));
}

#[test]
fn with_a_diagram_a_misread_square_is_repaired_in_place() {
    let ctx = GameContext::from_fen("r1bqkbnr/pppp1ppp/2n5/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 2 3").unwrap();
    // "f3" in the sentence is prose: a bare pawn push is a move only next to other notation.
    let read = lines("The knight on f3 is weak: 3.Nf8 Nf6");
    let out = repair_lines(&read, Some(&ctx), Lang::En, &CipherMap::new());
    assert_eq!(out.context, ContextUsed::Given);
    assert_eq!(out.edits.len(), 1, "{:?}", out.flags);
    let e = &out.edits[0];
    assert_eq!((e.from.as_str(), e.to.as_str(), e.kind, e.rect), ("3.Nf8", "3.Nf3", RepairKind::Confusion, read[0].words[6].rect));
    assert_eq!(text(&out.lines), "The knight on f3 is weak: 3.Nf3 Nf6");
    assert_eq!(out.lines[0].words[6].glyphs[4].rect, read[0].words[6].glyphs[4].rect);
}

#[test]
fn a_proven_table_decodes_a_ciphered_region_without_a_position() {
    // Ported from Caïssa's `test_book_cipher.py` (its table and its block).
    let table = CipherMap::from([
        ("W".into(), PieceKind::Queen),
        ("H".into(), PieceKind::Rook),
        ("&".into(), PieceKind::Bishop),
        ("S".into(), PieceKind::King),
    ]);
    let out = repair_lines(&lines("23...Wd5 24.Hb1 &xd5 25.Sf1 Hxb7 26.Wc3 &d4"), None, Lang::En, &table);
    assert_eq!(text(&out.lines), "23...♛d5 24.♜b1 ♝xd5 25.♚f1 ♜xb7 26.♛c3 ♝d4");
    assert_eq!(out.edits.len(), 7);
    assert!(out.edits.iter().all(|e| e.kind == RepairKind::Cipher && e.san.is_none()));
    // Correct notation and prose come back byte for byte.
    for clean in ["23...Qd5 24.Rb1 Bxd5 25.Kf1 Rxb7 26.Qc3 Bd4", "White stands better. However 23...Wd5 is weak"] {
        let read = lines(clean);
        let out = repair_lines(&read, None, Lang::En, &table);
        assert_eq!((out.lines, out.edits.len()), (read, 0), "{clean}");
    }
    // Two symbols among many correct moves: not a ciphered region.
    let mostly = lines("1.e4 e5 2.Nf3 Nc6 3.Bb5 a6 4.Ba4 Nf6 5.Wd3 Be7 6.Re1 b5 7.Wd1 d6");
    let out = repair_lines(&mostly[1..], None, Lang::En, &table);
    assert!(out.edits.is_empty());
}

#[test]
fn ambiguous_and_unreadable_words_are_flagged_and_left_as_read() {
    let ctx = GameContext::from_fen("4k3/8/8/8/8/5N2/8/1N2K3 w - - 0 1").unwrap();
    let read = lines("1.Nd2 Ke7");
    let out = repair_lines(&read, Some(&ctx), Lang::En, &CipherMap::new());
    assert_eq!(out.lines, read);
    assert_eq!(out.flags.len(), 1);
    let f = &out.flags[0];
    assert!(f.ambiguous && f.candidates.len() == 2 && f.text == "1.Nd2" && f.rect == read[0].words[0].rect);
    let out = repair_lines(&lines("1.Qa8 Ke7"), Some(&ctx), Lang::En, &CipherMap::new());
    assert!(!out.flags[0].ambiguous && !out.flags[0].candidates.is_empty());
}

#[test]
fn variations_are_not_replayed_in_the_main_line() {
    let read = lines("1.e4 e5 (1...c5 2.Nf8) 2.Nf3 Nc6");
    let out = repair_lines(&read, None, Lang::En, &CipherMap::new());
    assert!(out.edits.is_empty() && out.flags.is_empty(), "{:?} {:?}", out.edits, out.flags);
    assert_eq!(out.lines, read);
}

fn word_strategy() -> impl Strategy<Value = String> {
    "[KQRBNWH&()0-9a-hx=+#.O\\-]{1,8}"
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    #[test]
    fn arbitrary_lines_never_panic(words in proptest::collection::vec(word_strategy(), 0..12), diagram: bool, lang in 0usize..18) {
        let read = lines(&words.join(" "));
        let table = CipherMap::from([("W".into(), PieceKind::Queen), ("H".into(), PieceKind::Rook)]);
        let ctx = GameContext::game_start();
        let out = repair_lines(&read, diagram.then_some(&ctx), Lang::ALL[lang], &table);
        prop_assert_eq!(out.lines.len(), read.len());
        for (a, b) in out.lines.iter().zip(&read) {
            prop_assert_eq!(a.words.len(), b.words.len());
            prop_assert!(a.words.iter().zip(&b.words).all(|(x, y)| x.rect == y.rect));
        }
    }
}
