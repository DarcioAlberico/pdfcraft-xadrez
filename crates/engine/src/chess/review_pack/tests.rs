use super::*;
use crate::chess::DocKeys;
use crate::chess::corrections::{Kind, NewCorrection};
use crate::chess::games_store::GameRecord;
use crate::chess::pdfcraft_chess::game::Game;
use crate::chess::pdfcraft_chessvision::doc::MoveAt;
use crate::chess::reference_store::{Choice, Decision};

fn keys() -> DocKeys {
    DocKeys { id0: Some([9; 16]), path: Some("C:\\Users\\someone\\book.pdf".into()), name: Some("book.pdf".into()), size: Some(1234) }
}

fn fps() -> Vec<String> {
    vec!["fnv:p0".into(), "fnv:p1".into(), "fnv:p2".into()]
}

/// Two corrections, a decision, two games and a trash item for the document.
fn review() -> (CorrectionFile, ReferenceFile, GamesFile) {
    let doc = DocRecord::new(&keys(), 3);
    let mut c = CorrectionFile::new(doc.clone());
    for page in [0, 2] {
        let new = NewCorrection { page, rect: [0.1, 0.1, 0.4, 0.4], orig: None, fen: "4k3/8/8/8/8/8/8/4K3 w - - 0 1".into(), kind: Kind::Corrected };
        c.set(new, 7, &fps()).unwrap();
    }
    let mut r = ReferenceFile::empty(doc.clone());
    r.decisions.push(Decision::new(MoveAt { page: 1, block: 0, inline: 2 }, "Nf3".into(), "Nf3".into(), "Nc3".into(), Choice::Book));
    let mut g = GamesFile::empty(doc);
    for (uid, m) in [("g1", "e2e4"), ("g2", "d2d4")] {
        let mut game = Game::standard();
        game.play_uci(m).unwrap();
        g.games.push(GameRecord::of_game(uid.into(), 1, &game).unwrap());
    }
    g.instance = Some("instance-1-0.lock".into());
    g.trash.push(crate::chess::games_store::Trashed { pgn: "1. c4 *".into(), at: 3, ..Default::default() });
    (c, r, g)
}

/// Plan A9-G5b (A8-19): a pack holds the corrections, the decisions and the games, never the
/// PGN's text, the path's hash or the running instance, and reads back to the same counts;
/// packing the same review twice gives the same pack.
#[test]
fn a_pack_reads_back_whole_without_the_path() {
    let (c, mut r, g) = review();
    r.pgn = Some(crate::chess::reference_store::KeptPgn::new(Some("book.pgn"), "1. e4 e5 *".into(), 1, 2, 5).unwrap());
    let doc = DocRecord::new(&keys(), 3);
    let p = pack(&doc, false, Some(c.clone()), Some(r.clone()), Some(g.clone()));
    assert_eq!(p.counts(), (2, 1, 2, 1));
    let bytes = p.to_bytes().unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    for absent in ["Users", "someone", "1. e4 e5", "instance-1-0", "book.pdf", "book.pgn"] {
        assert!(!text.contains(absent), "{absent} in the pack");
    }
    assert!(p.doc.path_hash.is_none() && p.doc.id0.is_some());
    let back = unpack(&bytes).unwrap();
    assert_eq!(back.skipped, Skipped::default());
    assert_eq!(back.pack.counts(), (2, 1, 2, 1));
    assert_eq!(back.pack.pack_id, p.pack_id);
    assert_eq!(pack(&doc, false, Some(c), Some(r), Some(g)).pack_id, p.pack_id, "the same review, the same pack");
    // The name only when asked.
    assert_eq!(pack(&doc, true, None, None, None).doc.name.as_deref(), Some("book.pdf"));
}

/// Plan A9-G5b (§2.3, R2-03): a pack's entries go through each kind's own validation: a
/// correction at `page = usize::MAX` is left out and counted, the rest read.
#[test]
fn a_bad_entry_is_left_out_and_counted() {
    let (c, r, g) = review();
    let doc = DocRecord::new(&keys(), 3);
    let bytes = pack(&doc, false, Some(c), Some(r), Some(g)).to_bytes().unwrap();
    let mut v: Value = serde_json::from_slice(&bytes).unwrap();
    v["corrections"]["entries"][0]["page"] = json!(u64::MAX);
    let back = unpack(&serde_json::to_vec(&v).unwrap()).unwrap();
    assert_eq!(back.skipped.corrections, 1);
    assert_eq!(back.pack.counts(), (1, 1, 2, 1));
    // Not a pack, a pack of a later version, a part of a later version: refused with why.
    assert!(unpack(b"{}").is_err());
    v["version"] = json!(2);
    assert!(unpack(&serde_json::to_vec(&v).unwrap()).unwrap_err().contains("version 2"));
    v["version"] = json!(1);
    v["games"]["version"] = json!(9);
    assert!(unpack(&serde_json::to_vec(&v).unwrap()).unwrap_err().contains("newer"));
}

/// An empty review packs to no parts at all.
#[test]
fn nothing_kept_packs_nothing() {
    let doc = DocRecord::new(&keys(), 3);
    let p = pack(&doc, false, Some(CorrectionFile::new(doc.clone())), Some(ReferenceFile::empty(doc.clone())), Some(GamesFile::empty(doc.clone())));
    assert_eq!((p.corrections.is_none(), p.reference.is_none(), p.games.is_none()), (true, true, true));
    assert_eq!(unpack(&p.to_bytes().unwrap()).unwrap().pack.counts(), (0, 0, 0, 0));
}
