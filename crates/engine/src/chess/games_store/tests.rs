use super::*;
use crate::chess::DocKeys;
use crate::chess::store::{DirBackend, MemBackend};

#[path = "../../../tests/compat/games_v1.rs"]
mod games_v1;

/// The frozen sample of version 1 (plan A9-G12).
const GAMES_V1: &[u8] = include_bytes!("../../../../ui-egui/tests/compat/games_v1.games");

fn keys() -> DocKeys {
    DocKeys { id0: Some([9; 16]), path: Some("/b/book.pdf".into()), name: Some("book.pdf".into()), size: Some(1234) }
}

fn other_keys() -> DocKeys {
    DocKeys { id0: Some([3; 16]), path: Some("/b/other.pdf".into()), name: Some("other.pdf".into()), size: Some(99) }
}

fn dir(test: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("pdfcraft-games-store-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn game(moves: &[&str]) -> Game {
    let mut g = Game::standard();
    for m in moves {
        g.play_uci(m).unwrap();
    }
    g
}

fn record(uid: &str, rev: u64, moves: &[&str]) -> GameRecord {
    GameRecord::of_game(uid.into(), rev, &game(moves)).unwrap()
}

fn file_of(records: Vec<GameRecord>) -> GamesFile {
    let mut f = GamesFile::empty(DocRecord::new(&keys(), 0));
    f.games = records;
    f
}

fn nobody(_: &str) -> bool {
    false
}

const TOOL: Owner<'static> = Owner { me: None, live: &nobody, merge: false };

fn uids(f: &GamesFile) -> Vec<&str> {
    f.live().map(|g| g.uid.as_str()).collect()
}

/// Plan A9-G3b (§2.3): two copies of one document's games merge game by game, the higher revision
/// winning and a tie keeping this copy's; a deletion stays deleted; the games only the other copy
/// has come after this one's, in its order; the trash is the union of both without what either
/// took out.
#[test]
fn copies_merge_by_uid_and_revision() {
    let mut mine = file_of(vec![record("a", 5, &["e2e4"]), record("b", 5, &["d2d4"]), GameRecord::tombstone("c".into(), 9)]);
    mine.trash = vec![Trashed { pgn: "1. e4 *".into(), at: 1, ..Trashed::default() }];
    let mut theirs = file_of(vec![
        record("b", 6, &["c2c4"]),
        record("a", 5, &["g1f3"]),
        record("c", 3, &["e2e4", "e7e5"]),
        record("d", 1, &["b2b3"]),
        record("e", 1, &["b2b4"]),
    ]);
    let kept = Trashed { pgn: "1. d4 *".into(), at: 2, ..Trashed::default() };
    theirs.trash = vec![kept.clone(), Trashed { pgn: "1. c4 *".into(), at: 3, ..Trashed::default() }];
    theirs.taken = vec![Trashed { pgn: "1. e4 *".into(), at: 1, ..Trashed::default() }.id()];
    let taken = mine.merge(&theirs);
    assert_eq!(taken, ["b", "d", "e"]);
    assert_eq!(uids(&mine), ["a", "b", "d", "e"], "c stays deleted; d and e come after, in their order");
    assert_eq!(mine.get("a").unwrap().pgn, record("a", 5, &["e2e4"]).pgn, "a tie keeps this copy's");
    assert_eq!(mine.get("b").unwrap().pgn, record("b", 6, &["c2c4"]).pgn, "the higher revision wins");
    assert!(mine.get("c").unwrap().deleted);
    assert_eq!(mine.trash.iter().map(|t| t.pgn.as_str()).collect::<Vec<_>>(), ["1. d4 *", "1. c4 *"], "e4 was taken out there");
    // Merging again changes nothing.
    let before = mine.clone();
    assert!(mine.merge(&theirs).is_empty());
    assert_eq!(mine, before);
}

/// Plan A9-G3b (A4-06, R2-03): a game that can't be read is left out and counted, the rest kept;
/// a diagram past the last page a document has is one.
#[test]
fn a_bad_game_is_left_out_and_the_others_kept() {
    let mut f = file_of((0..16).map(|i| record(&format!("g{i}"), 1, &["e2e4"])).collect());
    f.games[3].origin = Some(DiagramAt::new(usize::MAX, 0));
    f.games[7].uid = "../x".into();
    let bytes = f.to_bytes().unwrap();
    let Parsed::File { file, skipped } = GamesFile::parse(&bytes) else { panic!("read") };
    assert_eq!((file.live().count(), skipped), (14, 2));
    // A field of the wrong kind in one game.
    let mut v: Value = serde_json::from_slice(&bytes).unwrap();
    v["games"][0]["rev"] = json_str("x");
    let Parsed::File { file, skipped } = GamesFile::parse(&serde_json::to_vec(&v).unwrap()) else { panic!("read") };
    assert_eq!((file.live().count(), skipped), (13, 3));
}

fn json_str(s: &str) -> Value {
    Value::String(s.into())
}

/// Plan A9 §2.10: a newer version's file is read for reading only, with the fields this build
/// doesn't know kept; a file of version 1 is never one of this kind's `.json` files.
#[test]
fn versions_and_unknown_fields() {
    let mut v: Value = serde_json::from_slice(&file_of(vec![record("a", 1, &["e2e4"])]).to_bytes().unwrap()).unwrap();
    v["games"][0]["page_fp"] = json_str("fnv:1");
    v["games"][0]["sources"] = serde_json::json!([{ "path": [0], "page": 2, "rect": [1.0, 2.0, 3.0, 4.0], "page_fp": "fnv:2" }]);
    v["later"] = Value::Bool(true);
    let Parsed::File { file, .. } = GamesFile::parse(&serde_json::to_vec(&v).unwrap()) else { panic!("read") };
    let again: Value = serde_json::from_slice(&file.to_bytes().unwrap()).unwrap();
    assert_eq!(again["games"][0]["page_fp"], "fnv:1");
    assert_eq!(again["games"][0]["sources"][0]["page_fp"], "fnv:2");
    assert_eq!(again["later"], true);
    v["version"] = Value::from(VERSION + 1);
    assert!(matches!(GamesFile::parse(&serde_json::to_vec(&v).unwrap()), Parsed::Newer { version, file: Some(_) } if version == VERSION + 1));
    v["version"] = Value::from(1);
    assert!(matches!(GamesFile::parse(&serde_json::to_vec(&v).unwrap()), Parsed::Bad(_)));
}

/// Plan A9-G3b (R9-01 ends with version 1): what this build's PGN gives back is not stored
/// apart, and a game comes back as it was: comments before a first reply (with and without a
/// variation before it), a comment PGN would change, a move read on a page, the move shown.
#[test]
fn a_game_comes_back_as_it_was() {
    let mut g = Game::standard();
    g.play_uci("e2e4").unwrap();
    let e4 = g.current();
    g.play_uci("e7e5").unwrap();
    let e5 = g.current();
    g.set_comment(e4, "after e4").unwrap();
    g.set_comment_before(e5, "before e5").unwrap();
    g.go_to_node(e4);
    g.play_uci("c7c5").unwrap();
    let c5 = g.current();
    g.set_comment_before(c5, "before c5").unwrap();
    g.go_to_node(e5);
    g.play_uci("g1f3").unwrap();
    let nf3 = g.current();
    g.set_comment(nf3, "[%clk 0:05:00] a command PGN reads as one").unwrap();
    g.set_source(nf3, Some(NodeSource { page: 4, rect_user: [1.0, 2.0, 3.0, 4.0] }));
    g.play_uci("b8c6").unwrap();
    let nc6 = g.current();
    g.set_comment_before(nc6, &"b".repeat(5000)).unwrap();
    g.go_to_node(c5);
    let r = GameRecord::of_game("x".into(), 1, &g).unwrap();
    // e4's and e5's (a comment before a first reply), c5's and nc6's (before a move), Nf3's (a
    // command): what this build's PGN would not give back as it is. A comment of 5,000 bytes is no
    // reason any more (R9-01's 4 KiB were the readers' before plan A9-F2b).
    let stored: Vec<Vec<u16>> = r.comments.iter().map(|c| c.path.clone()).collect();
    assert_eq!(stored, [vec![0], vec![0, 0], vec![0, 1], vec![0, 0, 0], vec![0, 0, 0, 0]]);
    let (back, cut) = r.game().unwrap();
    assert!(!cut);
    for (node, path) in [(e4, vec![0]), (e5, vec![0, 0]), (c5, vec![0, 1]), (nf3, vec![0, 0, 0]), (nc6, vec![0, 0, 0, 0])] {
        let at = back.node_at(&NodePath::new(path)).unwrap();
        assert_eq!((back.comment(at), back.comment_before(at)), (g.comment(node), g.comment_before(node)));
        assert_eq!(back.source(at), g.source(node));
    }
    assert_eq!(back.path_of(back.current()), g.path_of(c5));
    assert_eq!(back.pgn(), g.pgn());
}

/// Plan A9-G3b: version 1's frozen sample brought in, its games as they were (the PGN, what PGN
/// can't hold, the game shown, the trash), its document recorded without its path (R7-03).
#[test]
fn version_1_reads_into_this_version() {
    let (keys_read, file, skipped) = v1::read(GAMES_V1, "abc-1", 77).unwrap();
    assert_eq!((keys_read, skipped), (keys(), 0));
    assert!(file.doc.matches(&keys()));
    assert_eq!(file.games.len(), 3);
    assert_eq!(file.active.as_deref(), Some(file.games[1].uid.as_str()));
    assert!(file.games.iter().all(|g| g.rev == 77 && g.uid.starts_with("v1-")));
    let sample: Value = serde_json::from_slice(GAMES_V1.split(|&b| b == b'\n').nth(1).unwrap()).unwrap();
    for (g, s) in file.games.iter().zip(sample["games"].as_array().unwrap()) {
        assert_eq!(g.pgn, s["pgn"].as_str().unwrap_or_default());
        assert_eq!(serde_json::to_value(&g.current).unwrap(), s["current"]);
        assert_eq!(g.flipped, s["flipped"].as_bool().unwrap());
        assert_eq!(g.illegal_fen.as_deref(), s["illegal_fen"].as_str());
        assert_eq!(
            g.origin.map(|o| (o.page, o.index)),
            s["origin"].as_object().map(|o| (o["page"].as_u64().unwrap() as usize, o["index"].as_u64().unwrap() as usize))
        );
        assert_eq!(g.sources.len(), s["sources"].as_array().unwrap().len());
        assert_eq!(serde_json::to_value(&g.comments).unwrap(), s["comments"]);
    }
    assert_eq!(file.trash, vec![Trashed { pgn: "1. e4 e5 2. Nf3 *".into(), name: "Game 1".into(), moves: 3, deleted: 1, at: 7 }]);
    // The same file, again: the same uids, so merging changes nothing.
    let (_, again, _) = v1::read(GAMES_V1, "abc-1", 77).unwrap();
    let mut merged = file.clone();
    assert!(merged.merge(&again).is_empty());
    let text = String::from_utf8(file.to_bytes().unwrap()).unwrap();
    assert!(!text.contains("/b/book.pdf"), "no path in clear");
    assert!(v1::read(b"{\"format\":\"pdfcraft-chess-games\",\"version\":2}\n{}\n", "abc-1", 0).is_none());
}

/// Plan A9-G3b with G5a's rules: bringing version 1 in merges, can run again, and never touches
/// the source; a file an earlier build changed since is brought in again; another book's file
/// goes to its own document; the builds installed before set aside nothing the new store writes
/// (they look only at `.games`, R3-05), and the version 1 files stay as they were.
#[test]
fn version_1_is_brought_in_and_left_as_it_is() {
    let d = dir("v1");
    let (from, to) = (d.join("recovery-chess"), d.join("games"));
    std::fs::create_dir_all(&from).unwrap();
    std::fs::write(from.join("abc-1.games"), GAMES_V1).unwrap();
    let other = String::from_utf8(GAMES_V1.to_vec())
        .unwrap()
        .replacen("09090909090909090909090909090909", "03030303030303030303030303030303", 1)
        .replacen("/b/book.pdf", "/b/other.pdf", 1);
    std::fs::write(from.join("def-2.games"), &other).unwrap();
    let store = DirStore::new(&to);
    let m = store.migrate_v1(&from).unwrap();
    assert_eq!((m.merged, m.unchanged), (2, 0));
    let l = store.load(&keys(), 0).unwrap();
    assert_eq!(l.file.live().count(), 3);
    assert!(l.name.ends_with(".games.json"));
    let other_keys = DocKeys { id0: Some([3; 16]), path: Some("/b/other.pdf".into()), name: Some("book.pdf".into()), size: Some(1234) };
    assert_eq!(store.load(&other_keys, 0).unwrap().file.live().count(), 3);
    assert_eq!(store.migrate_v1(&from).unwrap().unchanged, 2, "run again: nothing read again");
    // An earlier build writes the file again, a game more.
    let mut lines: Vec<String> = String::from_utf8(GAMES_V1.to_vec()).unwrap().lines().map(str::to_owned).collect();
    let mut body: Value = serde_json::from_str(&lines[1]).unwrap();
    let extra = body["games"][0].clone();
    body["games"].as_array_mut().unwrap().push(extra);
    lines[1] = body.to_string();
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(from.join("abc-1.games"), format!("{}\n{}\n", lines[0], lines[1])).unwrap();
    assert_eq!(store.migrate_v1(&from).unwrap().changed, 1);
    assert_eq!(store.load(&keys(), 0).unwrap().file.live().count(), 4, "what it changed is brought in");
    // The source is as it was written, and an installed build's walk sets nothing aside.
    assert_eq!(std::fs::read_to_string(from.join("def-2.games")).unwrap(), other);
    std::fs::copy(to.join(&l.name), from.join(&l.name)).unwrap();
    assert!(games_v1::walk(&from).is_empty());
    assert!(games_v1::walk(&to).is_empty());
    let names: Vec<String> = std::fs::read_dir(&from).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert!(names.iter().all(|n| !n.contains(".bad")), "{names:?}");
    let _ = std::fs::remove_dir_all(&d);
}

/// Plan A9-G3b (§2.3, finding A4-07 in the games): keeping a document's games merges them with
/// what the store has, so games another writer added are kept (three in the app, two imported:
/// five), and the store's own later change wins.
#[test]
fn keeping_merges_with_what_the_store_has() {
    let store = MemStore::new(1 << 20);
    let imported = file_of(vec![record("i1", 1, &["e2e4"]), record("i2", 1, &["d2d4"])]);
    store.keep(&keys(), 0, imported, &TOOL).unwrap();
    let app = file_of(vec![record("a1", 1, &["c2c4"]), record("a2", 1, &["g1f3"]), record("a3", 1, &["b2b3"])]);
    let me = Owner { me: Some("instance-1-0.lock"), live: &nobody, merge: false };
    let (kept, _) = store.keep(&keys(), 0, app, &me).unwrap();
    assert_eq!(uids(&kept.file), ["a1", "a2", "a3", "i1", "i2"]);
    assert_eq!(kept.theirs, ["i1", "i2"]);
    assert_eq!(kept.file.instance.as_deref(), Some("instance-1-0.lock"));
    // Another book's games are its own.
    assert_eq!(store.load(&other_keys(), 0).unwrap().file.live().count(), 0);
}

/// Plan A9-G3b: a document whose games a running PdfCraft autosaves is not written over by a tool
/// unless it says `merge`; the instance is kept; a newer version's file is never written.
#[test]
fn a_running_pdfcrafts_games_are_merged_into_only_when_asked() {
    let store = MemStore::new(1 << 20);
    let me = Owner { me: Some("instance-1-0.lock"), live: &nobody, merge: false };
    store.keep(&keys(), 0, file_of(vec![record("a1", 1, &["c2c4"])]), &me).unwrap();
    let alive = |name: &str| name == "instance-1-0.lock";
    let tool = Owner { me: None, live: &alive, merge: false };
    let refused = store.keep(&keys(), 0, file_of(vec![record("t1", 1, &["e2e4"])]), &tool);
    assert!(matches!(refused, Err(GamesStoreError::OtherInstance)), "{refused:?}");
    assert_eq!(store.load(&keys(), 0).unwrap().file.live().count(), 1);
    let merging = Owner { me: None, live: &alive, merge: true };
    let (kept, _) = store.keep(&keys(), 0, file_of(vec![record("t1", 1, &["e2e4"])]), &merging).unwrap();
    assert_eq!(uids(&kept.file), ["t1", "a1"]);
    assert_eq!(kept.file.instance.as_deref(), Some("instance-1-0.lock"), "the app still keeps them");
    // Another instance, not running: written.
    let other = Owner { me: Some("instance-2-0.lock"), live: &nobody, merge: false };
    assert!(store.keep(&keys(), 0, file_of(Vec::new()), &other).is_ok());
}

/// The folder store: `<hash>.games.json` under `games.flock`, read back by another store on the
/// same folder (the command line reads what the app wrote).
#[test]
fn a_folder_store_reads_what_another_wrote() {
    let d = dir("folder");
    let app = DocStore::<Games, DirBackend>::new(&d);
    app.keep(&keys(), 3, file_of(vec![record("a1", 1, &["e2e4"])]), &TOOL).unwrap();
    let cli = DirStore::new(&d);
    let l = cli.load(&keys(), 3).unwrap();
    assert_eq!(uids(&l.file), ["a1"]);
    let names: Vec<String> = std::fs::read_dir(&d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert!(names.iter().any(|n| n.ends_with(".games.json")) && names.iter().all(|n| !n.ends_with(".games")), "{names:?}");
    let _ = MemBackend::new(0);
    let _ = std::fs::remove_dir_all(&d);
}

/// A tombstone and the compaction: past [`MAX_RECORDS`] the oldest tombstones go first, never a
/// live game.
#[test]
fn compaction_drops_the_oldest_tombstones_only() {
    let mut f = file_of((0..MAX_GAMES).map(|i| record(&format!("l{i}"), 1, &["e2e4"])).collect());
    let mut theirs = file_of(Vec::new());
    for i in 0..MAX_RECORDS {
        theirs.games.push(GameRecord::tombstone(format!("t{i}"), u64::try_from(i).unwrap()));
    }
    f.merge(&theirs);
    assert_eq!(f.games.len(), MAX_RECORDS);
    assert_eq!(f.live().count(), MAX_GAMES);
    assert!(f.get("t0").is_none() && f.get(&format!("t{}", MAX_RECORDS - 1)).is_some());
}

#[test]
fn next_rev_is_later_and_higher() {
    assert_eq!(next_rev(5, 3), 6);
    assert_eq!(next_rev(5, 100), 100);
    assert_eq!(next_rev(u64::MAX, 0), u64::MAX);
    assert_ne!(new_uid(1, b"x"), new_uid(1, b"x"));
}
