//! Plan A9-N16a: the material signature and the pawn traits, by hand and by property.

use super::*;
use crate::fen::parse_placement;
use proptest::prelude::*;

fn placement(fen: &str) -> Placement {
    parse_placement(fen.split(' ').next().unwrap()).unwrap()
}

fn squares(it: impl Iterator<Item = Square>) -> Vec<String> {
    it.map(|s| s.to_string()).collect()
}

/// The card's case: a king, a rook and a pawn against a king.
#[test]
fn the_signature_lists_each_sides_pieces_white_first() {
    let m = Material::of(&placement("8/8/8/4k3/8/8/4P3/4K2R w - - 0 1"));
    assert_eq!(m.to_string(), "KRP-K");
    assert_eq!(Material::of(&Placement::standard()).to_string(), "KQRRBBNNPPPPPPPP-KQRRBBNNPPPPPPPP");
    assert_eq!(Material::parse("KRP-K"), Ok(m));
    assert_eq!(Material::parse(" krp – k "), Ok(m), "either case, spaces, an en dash");
    assert_eq!(m.count(Color::White, PieceKind::Rook), 1);
    assert_eq!(m.count(Color::Black, PieceKind::Pawn), 0);
}

#[test]
fn a_text_that_is_no_signature_is_refused_with_why() {
    assert_eq!(Material::parse("KRP"), Err(MaterialError::NoDash));
    assert_eq!(Material::parse("KRX-K"), Err(MaterialError::NotAPiece('X')));
    assert_eq!(Material::parse("KPR-K"), Err(MaterialError::OutOfOrder('R')));
    assert_eq!(Material::parse("KRP-"), Err(MaterialError::EmptySide));
    assert_eq!(Material::parse(&format!("K{}-K", "P".repeat(16))), Err(MaterialError::TooMany));
    assert_eq!(Material::parse(&"K".repeat(100)), Err(MaterialError::TooLong));
    assert!(Material::parse("KQRRBBNNPPPPPPP-K").is_ok(), "sixteen is not too many");
}

#[test]
fn the_colours_swapped_match_when_asked() {
    let krp_kr = Material::parse("KRP-KR").unwrap();
    let kr_krp = Material::parse("KR-KRP").unwrap();
    assert!(krp_kr.matches(&krp_kr, false));
    assert!(!kr_krp.matches(&krp_kr, false));
    assert!(kr_krp.matches(&krp_kr, true));
    assert_eq!(kr_krp.mirrored(), krp_kr);
}

/// The card's six positions made by hand.
#[test]
fn doubled_isolated_and_passed_pawns() {
    // a2 and a3: doubled, both isolated, both passed (no black pawn).
    let t = PawnTraits::of(&placement("4k3/8/8/8/8/P7/P7/4K3"), Color::White);
    assert_eq!(t.doubled_files().collect::<Vec<_>>(), [0]);
    assert_eq!(squares(t.isolated()), ["a2", "a3"]);
    assert!(t.has_passed());
    // A b-pawn beside them: still doubled, no longer isolated.
    let t = PawnTraits::of(&placement("4k3/8/8/8/8/P7/PP6/4K3"), Color::White);
    assert_eq!((t.doubled_files().count(), t.isolated().count()), (1, 0));
    // d5 against black pawns on a7 and h7: passed; with one on e6, not.
    let t = PawnTraits::of(&placement("4k3/p6p/8/3P4/8/8/8/4K3"), Color::White);
    assert_eq!(squares(t.passed()), ["d5"]);
    let t = PawnTraits::of(&placement("4k3/p6p/4p3/3P4/8/8/8/4K3"), Color::White);
    assert_eq!(t.passed().count(), 0);
    // A black pawn behind it does not stop it (e4 is behind d5).
    let t = PawnTraits::of(&placement("4k3/8/8/3P4/4p3/8/8/4K3"), Color::White);
    assert_eq!(squares(t.passed()), ["d5"]);
    // Black's pawns go down the board: e3 is passed; a7 meets White's b6 ahead of it, on a file
    // beside. Both are isolated.
    let t = PawnTraits::of(&placement("4k3/p7/1P6/8/8/4p3/8/4K3"), Color::Black);
    assert_eq!(squares(t.passed()), ["e3"]);
    assert_eq!(squares(t.isolated()), ["e3", "a7"]);
}

#[test]
fn a_query_asks_for_material_and_pawns_together() {
    let p = placement("8/5k2/8/3P4/8/8/8/R3K1r1");
    let q = Query { material: Material::parse("KRP-KR").ok(), either_side: false, pawns: PawnQuery::default() };
    assert!(q.matches(&p));
    assert!(!Query { material: Material::parse("KR-KRP").ok(), ..q }.matches(&p));
    assert!(Query { material: Material::parse("KR-KRP").ok(), either_side: true, ..q }.matches(&p));
    let passed = PawnQuery { passed: true, ..PawnQuery::default() };
    assert!(Query { pawns: passed, ..q }.matches(&p));
    assert!(!Query { pawns: PawnQuery { side: Some(Color::Black), ..passed }, ..q }.matches(&p));
    assert!(!Query { pawns: PawnQuery { doubled: true, ..passed }, ..q }.matches(&p));
    assert!(Query::default().matches(&p) && Query::default().is_empty());
}

/// A placement of at most 16 pieces a side, any of them anywhere (an illegal reading too).
fn any_placement() -> impl Strategy<Value = Placement> {
    proptest::collection::vec(proptest::option::weighted(0.3, (any::<bool>(), 0usize..6)), 64).prop_map(|cells| {
        let mut p = Placement::empty();
        let mut counts = [0usize; 2];
        for (i, cell) in cells.into_iter().enumerate() {
            if let (Some((white, k)), Some(sq)) = (cell, Square::from_index(i)) {
                let color = if white { Color::White } else { Color::Black };
                let n = &mut counts[usize::from(!white)];
                if *n < MAX_PIECES_A_SIDE {
                    *n += 1;
                    p.set(sq, Some(Piece::new(color, PieceKind::ALL[k])));
                }
            }
        }
        p
    })
}

fn any_query() -> impl Strategy<Value = Query> {
    (any_placement(), any::<bool>(), any::<bool>(), any::<[bool; 3]>(), proptest::option::of(any::<bool>()), any::<bool>()).prop_map(
        |(p, with_material, either_side, [doubled, isolated, passed], side, _)| Query {
            material: with_material.then(|| Material::of(&p)),
            either_side,
            pawns: PawnQuery { doubled, isolated, passed, side: side.map(|w| if w { Color::White } else { Color::Black }) },
        },
    )
}

proptest! {
    /// `parse` reads back what `of` writes.
    #[test]
    fn a_signature_reads_back(p in any_placement()) {
        let m = Material::of(&p);
        let written = m.to_string();
        // A side with no piece at all (a misread board) writes nothing for it, which no signature is.
        if !written.starts_with('-') && !written.ends_with('-') {
            prop_assert_eq!(Material::parse(&written), Ok(m));
        }
    }

    /// The colours swapped and the board turned over, a query finds what it found.
    #[test]
    fn a_query_finds_the_same_on_the_other_side(q in any_query(), p in any_placement()) {
        prop_assert_eq!(q.matches(&p), q.mirrored().matches(&mirrored(&p)));
        prop_assert_eq!(mirrored(&mirrored(&p)), p);
    }
}
