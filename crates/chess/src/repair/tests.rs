use cozy_chess::Board;
use proptest::prelude::*;

use super::*;
use crate::san::Style;

/// The Opera game (Morphy, Paris 1858) in English SAN.
const OPERA: &str = "1.e4 e5 2.Nf3 d6 3.d4 Bg4 4.dxe5 Bxf3 5.Qxf3 dxe5 6.Bc4 Nf6 7.Qb3 Qe7 8.Nc3 c6 9.Bg5 b5 10.Nxb5 cxb5 \
                     11.Bxb5+ Nbd7 12.O-O-O Rd8 13.Rxd7 Rxd7 14.Rd1 Qe6 15.Bxd7+ Nxd7 16.Qb8+ Nxb8 17.Rd8#";

fn plain(text: &str) -> Vec<ReadToken> {
    text.split_whitespace().map(ReadToken::plain).collect()
}

fn start() -> GameContext {
    GameContext::game_start()
}

fn en(san: &str) -> San {
    San::read(san, Lang::En).unwrap()
}

/// The Opera game printed with figurines, as a book sets it.
fn opera_figurines() -> String {
    OPERA
        .chars()
        .map(|c| match c {
            'K' => '♚',
            'Q' => '♛',
            'R' => '♜',
            'B' => '♝',
            'N' => '♞',
            c => c,
        })
        .collect()
}

#[test]
fn the_queen_read_as_w_is_learned_and_proved_by_replay() {
    // ♕ → W systematically, as an OCR engine does on a figurine book.
    let text = OPERA.replace('Q', "W");
    let tokens = plain(&text);
    let report = infer(&tokens);
    assert_eq!(report.symbols, vec![("W".to_owned(), 5)]);
    let Proof::Proven { assignment, observations, plain_plies, plies, complete } = prove(&start(), &tokens, Lang::En, &report) else {
        panic!("not proven: {:?}", prove(&start(), &tokens, Lang::En, &report));
    };
    assert_eq!(assignment, CipherMap::from([("W".to_owned(), PieceKind::Queen)]));
    assert_eq!((plain_plies, plies, complete), (8, 33, true), "as written the replay stops at 5.Wxf3");
    assert_eq!(observations.len(), 5);
    assert_eq!(observations[0], Observation { symbol: "W".into(), piece: PieceKind::Queen, raw: "5.Wxf3".into(), san: "Qxf3".into() });
    // The repair then decodes every W, and only the W tokens.
    let replay = repair(&start(), &tokens, Lang::En, &assignment);
    assert!(replay.complete);
    let repaired: Vec<&Repair> = replay.outcomes.iter().filter_map(|o| if let Outcome::Repaired(r) = o { Some(r) } else { None }).collect();
    assert_eq!(repaired.len(), 5);
    assert_eq!((repaired[0].from.as_str(), repaired[0].to.as_str(), repaired[0].kind), ("5.Wxf3", "5.♛xf3", RepairKind::Cipher));
    assert_eq!(repaired[4].san.to_english(), "Qb8+");
}

#[test]
fn several_symbols_are_assigned_together() {
    // Caïssa's book-cipher test: ♗ → &, ♕ → W, ♖ → H; the knights print N.
    let text = OPERA.replace('B', "&").replace('Q', "W").replace('R', "H");
    let tokens = plain(&text);
    let report = infer(&tokens);
    let Proof::Proven { assignment, complete, .. } = prove(&start(), &tokens, Lang::En, &report) else { panic!("not proven") };
    assert!(complete);
    assert_eq!(assignment, CipherMap::from([("&".into(), PieceKind::Bishop), ("H".into(), PieceKind::Rook), ("W".into(), PieceKind::Queen)]));
}

#[test]
fn correct_notation_in_its_own_language_is_never_proved_a_cipher() {
    // German letters are symbols to `infer` (they are not English), but reading them as German
    // replays as far as any decoding, so nothing is proved.
    let german: String = OPERA.replace('N', "S").replace('B', "L").replace('Q', "D").replace('R', "T");
    let tokens = plain(&german);
    let report = infer(&tokens);
    assert!(!report.symbols.is_empty());
    assert_eq!(prove(&start(), &tokens, Lang::De, &report), Proof::NoGain { plain_plies: 33, best_plies: 33 });
}

#[test]
fn a_cipher_that_makes_the_replay_worse_is_rejected() {
    // Polish prints the rook W and the queen H. Every assignment but the Polish one breaks the
    // game earlier, and the Polish one only matches the reading as written.
    let polish: String = OPERA.replace('N', "S").replace('B', "G").replace('Q', "@").replace('R', "W").replace('@', "H");
    let tokens = plain(&polish);
    let report = infer(&tokens);
    let Proof::NoGain { plain_plies, best_plies } = prove(&start(), &tokens, Lang::Pl, &report) else { panic!("a cipher was accepted") };
    assert_eq!((plain_plies, best_plies), (33, 33));
    // A wrong table applied by force replays less far than the text as written.
    let wrong = CipherMap::from([("W".into(), PieceKind::Queen), ("H".into(), PieceKind::Rook)]);
    let shaped: Vec<(&ReadToken, Option<Shape>)> = tokens.iter().map(|t| (t, shape(t))).collect();
    assert!(replay_strict(&start(), &shaped, Lang::Pl, &wrong).sans.len() < 33);
}

#[test]
fn two_assignments_reading_different_moves_prove_nothing() {
    // `Xf3` after 1.e4 e5: the queen and the knight both reach f3, and the page does not say.
    let tokens = plain("1.e4 e5 2.Xf3 Xa3");
    let report = infer(&tokens);
    assert_eq!(report.symbols, vec![("X".to_owned(), 2)]);
    assert!(matches!(prove(&start(), &tokens, Lang::En, &report), Proof::Disputed { plies: 3 }));
}

#[test]
fn the_ciphered_page_guard() {
    // Twelve judged piece moves with a ciphered majority (`cipher.py:206`).
    let page = OPERA.replace('Q', "W").replace('N', "%").replace('B', "&");
    let report = infer(&plain(&page));
    assert!(report.judged() >= MIN_JUDGED_MOVES && report.is_ciphered(), "{report:?}");
    // Two smudges among correct moves: W is a symbol, but the page is not ciphered.
    let mostly_clean = OPERA.replacen("Qxf3", "Wxf3", 1).replacen("Qb3", "Wb3", 1);
    let report = infer(&plain(&mostly_clean));
    assert_eq!(report.symbols, vec![("W".to_owned(), 2)]);
    assert!(!report.is_ciphered());
    // A symbol seen once is not an alphabet.
    assert!(infer(&plain("1.e4 e5 2.Wh5")).symbols.is_empty());
}

#[test]
fn a_misread_square_is_repaired_when_only_one_move_is_legal() {
    let replay = repair(&start(), &plain("1.Nf8"), Lang::En, &CipherMap::new());
    let Outcome::Repaired(r) = &replay.outcomes[0] else { panic!("{:?}", replay.outcomes) };
    assert_eq!((r.from.as_str(), r.to.as_str(), r.san.to_english().as_str(), r.kind), ("1.Nf8", "1.Nf3", "Nf3", RepairKind::Confusion));
    assert_eq!(r.confidence, CONFIDENCE_ONE_SWAP);
    assert!(r.why.contains("'8' taken as '3'"), "{}", r.why);
    assert!(replay.complete);
}

#[test]
fn a_correct_move_is_never_improved() {
    let replay = repair(&start(), &plain(OPERA), Lang::En, &CipherMap::new());
    assert!(replay.complete);
    assert!(replay.outcomes.iter().all(|o| matches!(o, Outcome::Kept(_))));
    assert_eq!(replay.plies, 33);
}

#[test]
fn the_readers_alternatives_are_tried_and_ties_are_flagged() {
    // Knights on g1 and e4: f3 and f6 are both in reach.
    let ctx = GameContext::from_fen("4k3/8/8/8/4N3/8/8/4K1N1 w - - 0 1").unwrap();
    let mut token = ReadToken::plain("Nf8");
    token.slots[2].alternatives = vec![Alternative { text: "8".into(), p: 0.6 }, Alternative { text: "3".into(), p: 0.3 }];
    let one = repair(&ctx, std::slice::from_ref(&token), Lang::En, &CipherMap::new());
    let Outcome::Repaired(r) = &one.outcomes[0] else { panic!("{:?}", one.outcomes) };
    assert_eq!((r.san.to_english().as_str(), r.kind, r.confidence), ("Nf3", RepairKind::Alternative, 0.3));
    token.slots[2].alternatives.push(Alternative { text: "6".into(), p: 0.1 });
    let two = repair(&ctx, &[token, ReadToken::plain("Ke7")], Lang::En, &CipherMap::new());
    let Outcome::Ambiguous { candidates } = &two.outcomes[0] else { panic!("{:?}", two.outcomes) };
    let mut names: Vec<String> = candidates.iter().map(San::to_english).collect();
    names.sort();
    assert_eq!(names, ["Nf3", "Nf6+"]);
    assert_eq!(two.outcomes[1], Outcome::NotReached, "the replay stops at a flag");
    assert!(!two.complete);
}

#[test]
fn a_missing_disambiguation_is_flagged_not_guessed() {
    let ctx = GameContext::from_fen("4k3/8/8/8/8/5N2/8/1N2K3 w - - 0 1").unwrap();
    let replay = repair(&ctx, &plain("Nd2"), Lang::En, &CipherMap::new());
    let Outcome::Ambiguous { candidates } = &replay.outcomes[0] else { panic!("{:?}", replay.outcomes) };
    let mut names: Vec<String> = candidates.iter().map(San::to_english).collect();
    names.sort();
    assert_eq!(names, ["Nbd2", "Nfd2"]);
}

#[test]
fn the_other_readers_word_is_a_candidate() {
    let mut token = ReadToken::plain("Mf3");
    token.other = Some("Nf3".into());
    let replay = repair(&start(), &[token], Lang::En, &CipherMap::new());
    let Outcome::Repaired(r) = &replay.outcomes[0] else { panic!("{:?}", replay.outcomes) };
    assert_eq!((r.to.as_str(), r.kind, r.slots.as_ref()), ("Nf3", RepairKind::OtherReader, None));
}

#[test]
fn an_unreadable_token_stops_the_replay_with_near_misses() {
    let replay = repair(&start(), &plain("1.e4 e5 2.Kf7 Nc6"), Lang::En, &CipherMap::new());
    let Outcome::Unresolved { near } = &replay.outcomes[2] else { panic!("{:?}", replay.outcomes) };
    assert_eq!(near.len(), MAX_NEAR_MISSES);
    assert_eq!(replay.outcomes[3], Outcome::NotReached);
    assert_eq!(replay.plies, 2);
}

#[test]
fn castling_promotion_and_en_passant() {
    let castle = GameContext::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").unwrap();
    for (read, want) in [("0-0", "O-O"), ("O-O-O", "O-O-O"), ("0-Q", "O-O"), ("o—o—o", "O-O-O")] {
        let replay = repair(&castle, &plain(read), Lang::En, &CipherMap::new());
        let san = match &replay.outcomes[0] {
            Outcome::Kept(s) => s.clone(),
            Outcome::Repaired(r) => r.san.clone(),
            other => panic!("{read}: {other:?}"),
        };
        assert_eq!(san.to_english(), want, "{read}");
    }
    let promote = GameContext::from_fen("4k3/1P6/8/8/8/8/8/4K3 w - - 0 1").unwrap();
    let cipher = CipherMap::from([("W".into(), PieceKind::Queen)]);
    let Outcome::Repaired(r) = &repair(&promote, &plain("b8=W+"), Lang::En, &cipher).outcomes[0] else { panic!() };
    assert_eq!((r.to.as_str(), r.san.to_english().as_str()), ("b8=♛+", "b8=Q+"));
    // A promotion with its piece lost is four legal moves: flagged.
    let Outcome::Ambiguous { candidates } = &repair(&promote, &plain("b8"), Lang::En, &cipher).outcomes[0] else { panic!() };
    assert_eq!(candidates.len(), 4);
    let ep = GameContext::from_fen("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1").unwrap();
    assert_eq!(repair(&ep, &plain("exd6"), Lang::En, &CipherMap::new()).outcomes[0], Outcome::Kept(en("exd6")));
    // The glyph reader saw a 5 and offered a 6: only the capture en passant is legal.
    let mut misread = ReadToken::plain("exd5");
    misread.slots[3].alternatives = vec![Alternative { text: "5".into(), p: 0.7 }, Alternative { text: "6".into(), p: 0.25 }];
    let Outcome::Repaired(r) = &repair(&ep, &[misread], Lang::En, &CipherMap::new()).outcomes[0] else { panic!() };
    assert_eq!((r.san.to_english().as_str(), r.kind), ("exd6", RepairKind::Alternative));
}

#[test]
fn figurines_in_every_language() {
    // The Opera game set in figurines, its queens read as W: proved and repaired whatever the
    // language the page is read in, Polish (where W is the rook) included.
    let text = opera_figurines().replace('♛', "W");
    let tokens = plain(&text);
    let report = infer(&tokens);
    for lang in Lang::ALL {
        let Proof::Proven { assignment, plies, .. } = prove(&start(), &tokens, lang, &report) else { panic!("{lang:?}") };
        assert_eq!((assignment.get("W"), plies), (Some(&PieceKind::Queen), 33), "{lang:?}");
        let replay = repair(&start(), &tokens, lang, &assignment);
        assert!(replay.complete, "{lang:?}");
        let Outcome::Repaired(r) = &repair(&start(), &plain("♞f8"), lang, &CipherMap::new()).outcomes[0] else { panic!("{lang:?}") };
        assert_eq!(r.san.write(lang, Style::Figurine(FigurineSet::Solid)), "♞f3", "{lang:?}");
    }
}

#[test]
fn shapes() {
    let s = |t: &str| shape(&ReadToken::plain(t));
    assert_eq!(s("12...Wxe5+!"), Some(Shape { core: 5..9, number: Some((12, true)), prefix: 1, promotion: false, castle: false, capture: true }));
    assert_eq!(s("(4.Nbd2),").map(|x| (x.core, x.prefix)), Some((3..7, 1)));
    assert_eq!(s("l:tb1").map(|x| x.prefix), Some(3), "a damaged layer's cluster");
    assert_eq!(s("0-Q").map(|x| x.castle), Some(true), "castling misread");
    assert_eq!(s("a1=W").map(|x| (x.promotion, x.prefix)), Some((true, 0)));
    assert_eq!(s("88c7").map(|x| x.prefix), Some(1), "a rank digit needs a piece before it");
    assert!(s("f3").is_some_and(|x| !x.is_strong()));
    for prose in ["the", "1-0", "1984", "Cada", "3.5", "", "abcdefgh1"] {
        assert_eq!(s(prose), None, "{prose}");
    }
}

/// A position after `plies` random legal moves from the start, seeded.
fn position(mut seed: u64, plies: u8) -> GameContext {
    let mut board = Board::default();
    for _ in 0..plies {
        let moves = legal_moves(&board);
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let Some(&mv) = moves.get((seed % moves.len().max(1) as u64) as usize) else { break };
        board.play(mv);
    }
    GameContext { board }
}

fn token_strategy() -> impl Strategy<Value = ReadToken> {
    let slot = ("\\PC{0,3}", proptest::collection::vec(("\\PC{0,2}", proptest::num::f32::ANY), 0..4))
        .prop_map(|(text, alts)| Slot { text, alternatives: alts.into_iter().map(|(text, p)| Alternative { text, p }).collect() });
    (proptest::collection::vec(slot, 0..12), proptest::option::of("\\PC{0,12}")).prop_map(|(slots, other)| ReadToken { slots, other })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn arbitrary_tokens_and_positions_never_panic(
        seed in 1u64..u64::MAX,
        plies in 0u8..60,
        tokens in proptest::collection::vec(token_strategy(), 0..6),
        words in "[KQRBNWH&0-9a-hx=+#.O\\- ]{0,40}",
        lang in 0usize..18,
    ) {
        let ctx = position(seed, plies);
        let lang = Lang::ALL[lang];
        let cipher = CipherMap::from([("W".into(), PieceKind::Queen), ("&".into(), PieceKind::Bishop)]);
        let _ = repair(&ctx, &tokens, lang, &cipher);
        let words = plain(&words);
        let replay = repair(&ctx, &words, lang, &cipher);
        prop_assert_eq!(replay.outcomes.len(), words.len());
        let report = infer(&words);
        let _ = prove(&ctx, &words, lang, &report);
        let _ = prove(&ctx, &tokens, lang, &infer(&tokens));
    }
}
