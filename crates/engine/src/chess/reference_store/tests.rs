use super::*;
use crate::chess::store::file_name;

const PGN: &str = "[Event \"A\"]\n\n1. e4 e5 2. Nf3 *\n";
const OTHER: &str = "[Event \"B\"]\n\n1. d4 d5 *\n";

fn keys() -> DocKeys {
    DocKeys { id0: None, path: Some("C:/books/Livro.pdf".into()), name: Some("Livro.pdf".into()), size: Some(1234) }
}

fn saved_as() -> DocKeys {
    DocKeys { id0: Some([7; 16]), path: Some("C:/books/Livro-2.pdf".into()), name: Some("Livro-2.pdf".into()), size: Some(1300) }
}

fn kept(text: &str) -> KeptPgn {
    kept_at(text, 100)
}

fn kept_at(text: &str, at: u64) -> KeptPgn {
    KeptPgn::new(Some("livro.pgn"), text.to_owned(), 1, 3, at).unwrap()
}

/// The name of the document's file since plan A9-G3a: `<hash>.reference.json`.
fn own_name(keys: &DocKeys) -> String {
    file_name(keys, "reference").unwrap()
}

/// The name every kind's file had before plan A9-G3a: `<hash>.json`.
fn name_before(keys: &DocKeys) -> String {
    corrections::file_name(keys).unwrap()
}

/// A folder of its own under the temp folder, empty.
fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcraft-reference-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// The files of a folder by name, with their bytes.
fn folder(dir: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<(String, Vec<u8>)> = std::fs::read_dir(dir)
        .map(|entries| {
            entries.flatten().map(|e| (e.file_name().to_string_lossy().into_owned(), std::fs::read(e.path()).unwrap_or_default())).collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

#[test]
fn a_file_round_trips_and_bad_tampered_or_newer_files_are_told_apart() {
    let file = ReferenceFile::new(DocRecord::new(&keys(), 10), kept(PGN));
    let bytes = file.to_bytes().unwrap();
    assert_eq!(ReferenceFile::read(&bytes), Read::File(file.clone()));
    assert!(!String::from_utf8_lossy(&bytes).contains("C:/books"), "the document's path is kept only as a hash");

    // The same size, another text: only the hash tells.
    let mut tampered = file.clone();
    tampered.pgn.as_mut().unwrap().text = PGN.replace("e5", "c5");
    assert!(matches!(ReferenceFile::read(&tampered.to_bytes().unwrap()), Read::Bad(why) if why.contains("hash")));
    let mut short = file.clone();
    short.pgn.as_mut().unwrap().bytes = 3;
    assert!(matches!(ReferenceFile::read(&short.to_bytes().unwrap()), Read::Bad(_)));

    let newer = String::from_utf8(bytes.clone()).unwrap().replace("\"version\":2", "\"version\":3");
    let Read::Newer(3, Some(shown)) = ReferenceFile::read(newer.as_bytes()) else { panic!("{newer}") };
    assert_eq!((shown.version, shown.pgn), (3, file.pgn.clone()), "read all the same, as version 3");
    assert!(matches!(ReferenceFile::read(b"{\"format\":\"pdfcraft-chess-reference\",\"version\":9}"), Read::Newer(9, None)));
    assert!(matches!(ReferenceFile::read(b"{\"format\":\"pdfcraft-chess-corrections\",\"version\":1}"), Read::Bad(_)));
    assert!(matches!(ReferenceFile::read(b"not json"), Read::Bad(_)));
    assert!(matches!(ReferenceFile::read(b"[1]"), Read::Bad(_)));
    assert!(matches!(ReferenceFile::read(b"{\"format\":\"pdfcraft-chess-reference\",\"version\":0}"), Read::Bad(_)));
    // Version 1 needs its PGN; version 2 says when it was removed instead.
    assert!(matches!(ReferenceFile::read(br#"{"format":"pdfcraft-chess-reference","version":1,"doc":{}}"#), Read::Bad(_)));
    let mut gone = file.clone();
    assert!(gone.remove(7));
    let Read::File(back) = ReferenceFile::read(&gone.to_bytes().unwrap()) else { panic!("a removal reads") };
    assert_eq!((back.pgn, back.removed), (None, Some(7)));
}

#[test]
fn a_pgn_too_large_is_refused_and_a_long_name_is_not_kept() {
    let big = "x".repeat(MAX_REFERENCE_BYTES + 1);
    assert!(matches!(KeptPgn::new(None, big, 0, 0, 0), Err(ReferenceStoreError::TooBig)));
    let long = "n".repeat(MAX_NAME + 1);
    assert_eq!(KeptPgn::new(Some(&long), PGN.into(), 1, 3, 0).unwrap().name, None);
    assert_eq!(KeptPgn::new(Some(""), PGN.into(), 1, 3, 0).unwrap().name, None);
}

#[test]
fn the_folder_keeps_replaces_and_removes_a_documents_pgn() {
    let dir = temp_dir("keep");
    let store = DirStore::new(&dir);
    assert_eq!(store.load(&keys(), 10).unwrap().pgn(), None);
    assert!(!store.clear(&keys(), 10, 1).unwrap(), "nothing to remove, and the folder is not made");
    assert!(!dir.exists());

    let (had, loaded) = store.set(&keys(), 10, kept(PGN)).unwrap();
    assert!(!had);
    assert_eq!(loaded.pgn().map(|p| p.text.as_str()), Some(PGN));
    assert!(!dir.join("reference.lock").exists(), "no lock of the builds before taken: they never write this file");
    let again = store.load(&keys(), 10).unwrap();
    assert_eq!(again.pgn(), loaded.pgn());
    assert_eq!(again.name, own_name(&keys()));
    assert!(dir.join(own_name(&keys())).is_file() && !dir.join(name_before(&keys())).exists(), "written under its own name only");

    let (had, _) = store.set(&keys(), 10, kept_at(OTHER, 200)).unwrap();
    assert!(had, "a PGN was kept before");
    assert_eq!(store.load(&keys(), 10).unwrap().pgn().map(|p| p.text.as_str()), Some(OTHER));
    let other_doc = DocKeys { id0: None, path: Some("C:/books/Outro.pdf".into()), name: Some("Outro.pdf".into()), size: Some(9) };
    assert_eq!(store.load(&other_doc, 3).unwrap().pgn(), None, "a document finds only its own");

    assert!(store.clear(&keys(), 10, 300).unwrap());
    let cleared = store.load(&keys(), 10).unwrap();
    assert_eq!((cleared.pgn(), cleared.file.removed), (None, Some(300)), "the file says it was removed");
    assert!(!store.clear(&keys(), 10, 400).unwrap());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A9-G3a (A4-13): reading never renames. A file that can't be read is reported and left as it
/// is by a load, as a reading tool given only read access does; the next change sets it aside
/// before it writes, never deleting it. A newer version's file is only read.
#[test]
fn a_bad_file_is_reported_by_reading_and_set_aside_by_a_change_and_a_newer_one_is_only_read() {
    let dir = temp_dir("bad");
    std::fs::create_dir_all(&dir).unwrap();
    let store = DirStore::new(&dir);
    let name = own_name(&keys());
    std::fs::write(dir.join(&name), b"{ not json").unwrap();
    let loaded = store.load(&keys(), 10).unwrap();
    assert_eq!((loaded.pgn(), loaded.set_aside.clone()), (None, None));
    let (unread, why) = loaded.unreadable.unwrap();
    assert!(unread == name && why.contains("JSON"), "{why}");
    assert_eq!(folder(&dir), vec![(name.clone(), b"{ not json".to_vec())], "a load leaves the folder as it is");
    let (_, changed) = store.set(&keys(), 10, kept(PGN)).unwrap();
    assert_eq!(changed.set_aside.map(|(n, _)| n), Some(format!("{name}.bad")));
    assert_eq!(std::fs::read(dir.join(format!("{name}.bad"))).unwrap(), b"{ not json", "set aside, never deleted");

    let newer = String::from_utf8(ReferenceFile::new(DocRecord::new(&keys(), 10), kept(PGN)).to_bytes().unwrap())
        .unwrap()
        .replace("\"version\":2", "\"version\":3");
    std::fs::write(dir.join(&name), &newer).unwrap();
    let loaded = store.load(&keys(), 10).unwrap();
    assert_eq!((loaded.read_only, loaded.newer), (true, Some(3)));
    assert_eq!(loaded.pgn().map(|p| p.text.as_str()), Some(PGN), "read all the same");
    assert!(matches!(store.set(&keys(), 10, kept(OTHER)), Err(ReferenceStoreError::Newer(3))));
    assert!(matches!(store.clear(&keys(), 10, 9), Err(ReferenceStoreError::Newer(3))));
    assert!(matches!(store.decide(&keys(), 10, decision(3, Choice::Pgn)), Err(ReferenceStoreError::Newer(3))));
    assert!(!store.rekey(&keys(), &saved_as(), 10).unwrap());
    assert_eq!(std::fs::read(dir.join(&name)).unwrap(), newer.as_bytes(), "never written or removed");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A9-G3a (A4-16): another writer holding the folder (its `reference.flock`) makes a change wait,
/// then refuses it as busy, writing nothing. The lock file of the builds before (`reference.lock`)
/// no longer holds this kind: those builds never write a file this one writes.
#[test]
fn a_held_lock_refuses_a_change_and_writes_nothing() {
    let dir = temp_dir("lock");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("reference.lock"), b"").unwrap();
    let held = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(dir.join("reference.flock")).unwrap();
    held.lock().unwrap();
    let store = DirStore::new(&dir);
    assert!(matches!(store.set(&keys(), 10, kept(PGN)), Err(ReferenceStoreError::Busy)));
    assert_eq!(store.load(&keys(), 10).unwrap().pgn(), None);
    held.unlock().unwrap();
    store.set(&keys(), 10, kept(PGN)).unwrap();
    assert!(dir.join("reference.lock").exists(), "the old lock file is not this kind's any more, and left alone");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_saved_document_keeps_its_pgn_under_its_new_keys() {
    let dir = temp_dir("rekey");
    let store = DirStore::new(&dir);
    assert!(!store.rekey(&keys(), &saved_as(), 10).unwrap(), "no folder, nothing to move");
    assert!(!dir.exists());
    store.set(&keys(), 10, kept(PGN)).unwrap();
    assert!(store.rekey(&keys(), &saved_as(), 10).unwrap());
    let loaded = store.load(&saved_as(), 10).unwrap();
    assert_eq!(loaded.pgn().map(|p| p.text.as_str()), Some(PGN), "found by the document it records");
    assert_eq!(loaded.name, own_name(&keys()), "in the file it had");
    assert!(loaded.file.doc.matches(&saved_as()));
    assert!(!store.rekey(&saved_as(), &saved_as(), 10).unwrap());
    let _ = std::fs::remove_dir_all(&dir);
}

fn decision(inline: usize, choice: Choice) -> Decision {
    Decision { page: 2, block: 1, inline, san: "Bb5".into(), san_read: None, choice, other: Map::new() }
}

/// Plan A9-L0b (R3-01): a decision is for its move by the move's address and the SAN as read. A
/// move walked again from a corrected or confirmed diagram keeps its SAN read, while the SAN the
/// walk writes can change (a repair undone): the decision stays on it; and a move printed
/// otherwise at the same place, which the walk may write with the same SAN, is another move. A
/// decision made before the SAN read was kept still goes by the walk's SAN.
#[test]
fn a_decision_stays_on_its_move_by_the_san_read() {
    let at = pdfcraft_chessvision::doc::MoveAt { page: 0, block: 2, inline: 3 };
    let d = Decision::new(at, "exd4".into(), "cxd4".into(), Choice::Book);
    assert_eq!(d.san_read.as_deref(), Some("cxd4"));
    assert!(d.is_for(at, "exd4", "cxd4"), "as decided");
    assert!(d.is_for(at, "cxd4", "cxd4"), "walked again under the corrected diagram: the same printed move");
    assert!(!d.is_for(at, "exd4", "exd4"), "another printed move at the same place");
    assert!(!d.is_for(pdfcraft_chessvision::doc::MoveAt { inline: 4, ..at }, "exd4", "cxd4"), "another place");
    let older = Decision { san_read: None, ..d.clone() };
    assert!(older.is_for(at, "exd4", "anything"), "an older decision by the walk's SAN");
    assert!(!older.is_for(at, "cxd4", "cxd4"));
    // Written as before, with the SAN read beside; read by a build before as before.
    let json = serde_json::to_string(&d).unwrap();
    assert_eq!(json, r#"{"page":0,"block":2,"inline":3,"san":"exd4","san_read":"cxd4","choice":"book"}"#);
    assert_eq!(serde_json::from_str::<Decision>(r#"{"page":0,"block":2,"inline":3,"san":"exd4","choice":"book"}"#).unwrap(), older);
    // Deciding again on the same move replaces the choice, whatever the walk wrote since.
    let mut file = ReferenceFile::new(DocRecord::new(&keys(), 10), kept(PGN));
    file.decide(d.clone()).unwrap();
    file.decide(Decision::new(at, "cxd4".into(), "cxd4".into(), Choice::Pgn)).unwrap();
    assert_eq!(file.decisions.iter().map(|d| d.choice).collect::<Vec<_>>(), [Choice::Pgn]);
    let long = Decision::new(at, "exd4".into(), "x".repeat(MAX_SAN + 1), Choice::Book);
    assert!(matches!(file.decide(long), Err(ReferenceStoreError::NoMove)));
}

/// R6e: a decision on a move in conflict is kept with the PGN, in place of the one on the same
/// move; it goes with a save as another file, and keeping another PGN drops it. Without a PGN, or
/// naming no move, it is refused.
#[test]
fn decisions_are_kept_with_the_pgn_they_were_made_against() {
    let dir = temp_dir("decide");
    let store = DirStore::new(&dir);
    assert!(matches!(store.decide(&keys(), 10, decision(3, Choice::Pgn)), Err(ReferenceStoreError::NoPgn)), "no folder, no PGN");
    assert!(!dir.exists(), "and none made");
    store.set(&keys(), 10, kept(PGN)).unwrap();
    store.decide(&keys(), 10, decision(3, Choice::Pgn)).unwrap();
    store.decide(&keys(), 10, decision(5, Choice::Book)).unwrap();
    let file = store.decide(&keys(), 10, decision(3, Choice::Book)).unwrap().file;
    assert_eq!(file.decisions, vec![decision(5, Choice::Book), decision(3, Choice::Book)], "the same move's decision replaced");
    assert_eq!(store.load(&keys(), 10).unwrap().file.decisions.len(), 2, "kept on disk");
    let mut nameless = decision(7, Choice::Pgn);
    nameless.san.clear();
    assert!(matches!(store.decide(&keys(), 10, nameless), Err(ReferenceStoreError::NoMove)));
    assert!(store.rekey(&keys(), &saved_as(), 10).unwrap());
    assert_eq!(store.load(&saved_as(), 10).unwrap().file.decisions.len(), 2, "kept for the saved file");
    store.set(&saved_as(), 10, kept_at(OTHER, 200)).unwrap();
    assert!(store.load(&saved_as(), 10).unwrap().file.decisions.is_empty(), "another PGN, no decisions");
    // A file without decisions (as R5 wrote it) has none written.
    let bytes = ReferenceFile::new(DocRecord::new(&keys(), 10), kept(PGN)).to_bytes().unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("decisions"));
    let memory = MemStore::new(4096);
    assert!(matches!(memory.decide(&keys(), 10, decision(3, Choice::Pgn)), Err(ReferenceStoreError::NoPgn)));
    memory.set(&keys(), 10, kept(PGN)).unwrap();
    assert_eq!(memory.decide(&keys(), 10, decision(3, Choice::Pgn)).unwrap().file.decisions.len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A9-G3a (A4-06 for the book's PGN): a decision that can't be read used to make the whole file
/// unreadable, set aside with the PGN in it. Now it is left out and counted, the PGN and the other
/// decisions kept; the change that writes the file without it keeps the file as it was beside it.
#[test]
fn one_bad_decision_among_others_keeps_the_others() {
    let dir = temp_dir("one-bad");
    std::fs::create_dir_all(&dir).unwrap();
    let mut file = ReferenceFile::new(DocRecord::new(&keys(), 10), kept(PGN));
    for i in 0..5 {
        file.decide(decision(i, Choice::Book)).unwrap();
    }
    let text = String::from_utf8(file.to_bytes().unwrap()).unwrap();
    let bad = text.replacen("\"san\":\"Bb5\"", &format!("\"san\":\"{}\"", "x".repeat(MAX_SAN + 1)), 1).replacen(
        "{\"page\":2",
        "{\"page\":\"two\"},{\"page\":2",
        1,
    );
    assert_ne!(bad, text);
    let name = own_name(&keys());
    std::fs::write(dir.join(&name), &bad).unwrap();
    let store = DirStore::new(&dir);
    let loaded = store.load(&keys(), 10).unwrap();
    assert_eq!((loaded.pgn().is_some(), loaded.file.decisions.len(), loaded.skipped, loaded.unreadable.clone()), (true, 4, 2, None));
    let written = store.decide(&keys(), 10, decision(9, Choice::Pgn)).unwrap();
    assert_eq!((written.file.decisions.len(), written.set_aside.map(|(n, _)| n)), (5, Some(format!("{name}.bad"))));
    assert_eq!(std::fs::read_to_string(dir.join(format!("{name}.bad"))).unwrap(), bad, "the file as it was, beside it");
    assert_eq!(store.load(&keys(), 10).unwrap().skipped, 0, "written without them");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A9-G3a (A4-07): the book's PGN is `<hash>.reference.json`. A corrections file an earlier build
/// wrote under the name both kinds shared (`<hash>.json`) is never taken for the PGN, never set
/// aside, never written. The book's PGN an earlier build keeps under that name is read and merged:
/// the PGN kept or removed the later wins, and with the same PGN both sides' decisions are kept.
#[test]
fn the_name_before_is_read_and_merged_and_a_corrections_file_never_touched() {
    let dir = temp_dir("names");
    std::fs::create_dir_all(&dir).unwrap();
    let before = name_before(&keys());
    let corrections_file = br#"{"format":"pdfcraft-chess-corrections","version":1,"doc":{"pages":10},"entries":[]}"#;
    std::fs::write(dir.join(&before), corrections_file).unwrap();
    let store = DirStore::new(&dir);
    let l = store.load(&keys(), 10).unwrap();
    assert_eq!((l.pgn(), l.unreadable.clone()), (None, None), "not this kind's, so not unreadable either");
    let (_, l) = store.set(&keys(), 10, kept_at(PGN, 200)).unwrap();
    assert_eq!((l.name.clone(), l.set_aside.clone()), (own_name(&keys()), None));
    store.clear(&keys(), 10, 250).unwrap();
    assert_eq!(std::fs::read(dir.join(&before)).unwrap(), corrections_file.to_vec(), "never touched");
    assert!(folder(&dir).iter().all(|(n, _)| !n.contains(".bad")));

    // An earlier build's PGN under the name before: its own, older than the removal here.
    let mut theirs = ReferenceFile::new(DocRecord::new(&keys(), 10), kept_at(OTHER, 240));
    theirs.decide(decision(1, Choice::Book)).unwrap();
    std::fs::write(dir.join(&before), theirs.to_bytes().unwrap()).unwrap();
    assert_eq!(store.load(&keys(), 10).unwrap().pgn(), None, "removed here later: it stays removed");
    // Kept again there, later: seen here.
    let theirs_later = ReferenceFile { pgn: Some(kept_at(OTHER, 300)), ..theirs.clone() };
    std::fs::write(dir.join(&before), theirs_later.to_bytes().unwrap()).unwrap();
    let l = store.load(&keys(), 10).unwrap();
    assert_eq!((l.pgn().map(|p| p.text.as_str()), l.file.decisions.len()), (Some(OTHER), 1));
    // The same PGN kept here, decided on here: both sides' decisions; this side's on a move both decided.
    store.set(&keys(), 10, kept_at(OTHER, 400)).unwrap();
    store.decide(&keys(), 10, decision(1, Choice::Pgn)).unwrap();
    store.decide(&keys(), 10, decision(2, Choice::Pgn)).unwrap();
    let l = store.load(&keys(), 10).unwrap();
    let mut got: Vec<(usize, Choice)> = l.file.decisions.iter().map(|d| (d.inline, d.choice)).collect();
    got.sort_by_key(|(i, _)| *i);
    assert_eq!(got, vec![(1, Choice::Pgn), (2, Choice::Pgn)]);
    let bytes_before = std::fs::read(dir.join(&before)).unwrap();
    assert_eq!(bytes_before, theirs_later.to_bytes().unwrap(), "the name before is never written");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Plan A9 §2.10: the fields a later build of version 2 writes (in the file, the PGN, a decision)
/// are kept as they are by a change that keeps what they belong to, so a build of this version
/// writing after a later one (step G4's `pgn_san` and `bytes_hash`) loses nothing of it.
#[test]
fn fields_a_later_build_writes_are_kept() {
    let dir = temp_dir("later");
    std::fs::create_dir_all(&dir).unwrap();
    let mut file = ReferenceFile::new(DocRecord::new(&keys(), 10), kept(PGN));
    file.decide(decision(1, Choice::Pgn)).unwrap();
    let text = String::from_utf8(file.to_bytes().unwrap())
        .unwrap()
        .replacen("\"choice\":\"pgn\"", "\"choice\":\"pgn\",\"pgn_san\":\"Nf3\"", 1)
        .replacen("\"text\":", "\"bytes_hash\":\"abc\",\"text\":", 1)
        .replacen("\"version\":2,", "\"version\":2,\"later\":{\"n\":1},", 1);
    std::fs::write(dir.join(own_name(&keys())), &text).unwrap();
    let store = DirStore::new(&dir);
    store.decide(&keys(), 10, decision(2, Choice::Book)).unwrap();
    let after: Value = serde_json::from_slice(&std::fs::read(dir.join(own_name(&keys()))).unwrap()).unwrap();
    assert_eq!(after["later"], serde_json::json!({ "n": 1 }), "{after}");
    assert_eq!(after["pgn"]["bytes_hash"], "abc", "{after}");
    assert_eq!(after["decisions"][0]["pgn_san"], "Nf3", "{after}");
    assert!(after["decisions"][1].get("pgn_san").is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Plan A9-F1 (A1-10): a Latin-1 book PGN kept by a build before F1 (read with U+FFFD for each
/// accent) and the same file read now (decoded as Windows-1252) are the same PGN: the decisions
/// made against it stay, and the text kept from then on has its accents. The same text kept
/// again keeps them too; another PGN still drops them.
#[test]
fn the_same_pgn_kept_again_keeps_its_decisions_even_as_an_older_build_read_it() {
    let dir = temp_dir("same-pgn");
    let store = DirStore::new(&dir);
    let bytes = b"[White \"R\xe9ti\"]\n\n1. e4 e5 2. Nf3 *\n";
    let before_f1 = KeptPgn::new(Some("livro.pgn"), String::from_utf8_lossy(bytes).into_owned(), 1, 3, 100).unwrap();
    store.set(&keys(), 10, before_f1).unwrap();
    store.decide(&keys(), 10, decision(3, Choice::Pgn)).unwrap();
    let (text, encoding) = pdfcraft_chess::pgn::decode(bytes);
    assert_eq!(encoding, pdfcraft_chess::pgn::PgnEncoding::Windows1252);
    let now = KeptPgn::new(Some("livro.pgn"), text.clone(), 1, 3, 200).unwrap().read_from(bytes);
    let (replaced, loaded) = store.set(&keys(), 10, now).unwrap();
    assert!(replaced);
    let file = loaded.file;
    assert_eq!(file.decisions, vec![decision(3, Choice::Pgn)], "the same file, read as before F1");
    assert!(file.pgn.as_ref().unwrap().text.contains("Réti"));
    let on_disk = store.load(&keys(), 10).unwrap().file;
    assert_eq!((on_disk.decisions.len(), on_disk.pgn.map(|p| p.hash)), (1, Some(text_hash(&text))));
    // The same text again, then another PGN.
    store.set(&keys(), 10, KeptPgn::new(Some("livro.pgn"), text, 1, 3, 300).unwrap()).unwrap();
    assert_eq!(store.load(&keys(), 10).unwrap().file.decisions.len(), 1, "the same text");
    store.set(&keys(), 10, kept_at(OTHER, 400)).unwrap();
    assert!(store.load(&keys(), 10).unwrap().file.decisions.is_empty(), "another PGN");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn memory_keeps_within_its_budget_and_round_trips() {
    let store = MemStore::new(4096);
    assert!(matches!(store.load(&DocKeys::default(), 1), Err(ReferenceStoreError::NoKeys)));
    store.set(&keys(), 10, kept(PGN)).unwrap();
    let big = "1. e4 *\n".repeat(600);
    assert!(matches!(store.set(&saved_as(), 3, kept(&big)), Err(ReferenceStoreError::OverBudget(4096))));
    assert_eq!(store.load(&saved_as(), 3).unwrap().pgn(), None, "nothing kept when refused");

    let again = MemStore::with_files(store.files(), 4096);
    assert_eq!(again.load(&keys(), 10).unwrap().pgn().map(|p| p.text.as_str()), Some(PGN));
    assert!(again.rekey(&keys(), &saved_as(), 10).unwrap());
    assert_eq!(again.load(&saved_as(), 10).unwrap().pgn().map(|p| p.text.as_str()), Some(PGN));
    assert!(again.clear(&saved_as(), 10, 500).unwrap());
    assert_eq!(again.load(&saved_as(), 10).unwrap().pgn(), None);
    assert!(!again.clear(&saved_as(), 10, 600).unwrap());
    assert!(MemStore::with_files(store.files(), 10).files().is_empty(), "a file over the budget is left out");
}

// ─── Builds of version 1 (plan A9 §2.10) ───────────────────────────────────────────────────────

#[allow(dead_code)]
#[path = "../../../tests/compat/reference_v1.rs"]
mod reference_v1;

/// A9-G3a, plan §2.10: a build of version 1 (its format frozen in `reference_v1`) never finds a
/// version 2 file (it looks for version 1), and given one reads it as a newer version, read only,
/// never rewriting it without what it does not know. A file such a build wrote (decisions with and
/// without the SAN read) is read here with every field and written as version 2 with nothing lost.
#[test]
fn compat_a_version_1_build_never_finds_or_rewrites_a_version_2_file() {
    let mut file = ReferenceFile::new(DocRecord::new(&keys(), 10), kept(PGN));
    file.decide(Decision::new(pdfcraft_chessvision::doc::MoveAt { page: 1, block: 0, inline: 2 }, "Nf3".into(), "Nf3".into(), Choice::Pgn)).unwrap();
    let bytes = file.to_bytes().unwrap();
    assert_eq!(reference_v1::recorded(&bytes), None, "not found among a folder's files");
    assert!(matches!(reference_v1::ReferenceFile::read(&bytes), reference_v1::Read::Newer(2, Some(_))));
    let mut removed = file.clone();
    removed.remove(9);
    assert!(matches!(reference_v1::ReferenceFile::read(&removed.to_bytes().unwrap()), reference_v1::Read::Newer(2, None)));

    // What that build writes, read here.
    let record = DocRecord::new(&keys(), 10);
    let mut theirs = reference_v1::ReferenceFile {
        format: reference_v1::FORMAT.into(),
        version: reference_v1::VERSION,
        doc: reference_v1::DocRecord {
            id0: record.id0.clone(),
            path_hash: record.path_hash.clone(),
            name: record.name.clone(),
            size: record.size,
            pages: 10,
        },
        pgn: reference_v1::KeptPgn {
            name: Some("livro.pgn".into()),
            hash: reference_v1::text_hash(PGN),
            bytes: PGN.len(),
            at: 77,
            games: 1,
            moves: 3,
            text: PGN.into(),
            legacy_hash: None,
        },
        decisions: Vec::new(),
    };
    let old_decision = |inline: usize, san_read: Option<&str>| reference_v1::Decision {
        page: 2,
        block: 1,
        inline,
        san: "Bb5".into(),
        san_read: san_read.map(str::to_owned),
        choice: reference_v1::Choice::Book,
    };
    theirs.decide(old_decision(1, None)).unwrap();
    theirs.decide(old_decision(2, Some("Bb5+"))).unwrap();
    let Parsed::File { file: now, skipped } = ReferenceFile::parse(&theirs.to_bytes().unwrap()) else { panic!("a version 1 file is read") };
    assert_eq!(skipped, 0);
    assert_eq!(now.version, VERSION);
    let pgn = now.pgn.clone().unwrap();
    assert_eq!(
        (pgn.name.as_deref(), pgn.hash.as_str(), pgn.bytes, pgn.at, pgn.games, pgn.moves, pgn.text.as_str()),
        (Some("livro.pgn"), theirs.pgn.hash.as_str(), PGN.len(), 77, 1, 3, PGN)
    );
    let as_v1 = |d: &Decision| reference_v1::Decision {
        page: d.page,
        block: d.block,
        inline: d.inline,
        san: d.san.clone(),
        san_read: d.san_read.clone(),
        choice: match d.choice {
            Choice::Book => reference_v1::Choice::Book,
            Choice::Pgn => reference_v1::Choice::Pgn,
        },
    };
    assert_eq!(now.decisions.iter().map(as_v1).collect::<Vec<_>>(), theirs.decisions, "every decision, every field");
    assert_eq!(now.doc, record);
    let Parsed::File { file: again, .. } = ReferenceFile::parse(&now.to_bytes().unwrap()) else { panic!("written as version 2 and read") };
    assert_eq!(again, now);
}

/// A9-G3a, plan §2.10 (what an earlier build sees): this build keeps the book's PGN in
/// `<hash>.reference.json`. An earlier build's command line (`PDFCRAFT_OLD_CLI`, the installed
/// `dist/app/pdfcraft-cli.exe`) keeps to `<hash>.json`: it does not see what this build kept, and
/// leaves its file as it was, setting nothing aside; the PGN it keeps there later is seen here,
/// merged. Without the variable it is skipped with a warning, and it fails under
/// `PDFCRAFT_REQUIRE_MODELS=1` (the chess gate).
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn compat_an_earlier_command_line_and_this_build_share_a_folder() {
    use crate::chess::corrections::tests::{open, pages_pdf, with_id};
    let cli = match pdfcraft_chessvision::models::test_resource("PDFCRAFT_OLD_CLI", "an earlier build's pdfcraft-cli") {
        Ok(Some(cli)) => cli,
        Ok(None) => {
            eprintln!(
                "compat_an_earlier_command_line_and_this_build_share_a_folder (reference) skipped: set PDFCRAFT_OLD_CLI to an earlier build's pdfcraft-cli"
            );
            return;
        }
        Err(e) => panic!("{e}"),
    };
    let dir = temp_dir("old-cli");
    let folder_dir = dir.join(".pdfcraft").join("chess-reference");
    std::fs::create_dir_all(&folder_dir).unwrap();
    let pdf = with_id(pages_pdf(3));
    std::fs::write(dir.join("book.pdf"), &pdf).unwrap();
    std::fs::write(dir.join("other.pgn"), OTHER).unwrap();
    let (s, id) = open(pdf);
    let (keys, pages) = s.get(id).map(|d| (d.doc_keys(), d.info.pages.len())).unwrap();
    let store = DirStore::new(&folder_dir);
    store.set(&keys, pages, kept_at(PGN, 100)).unwrap();
    let ours = folder(&folder_dir);
    let steps = serde_json::json!([
        { "tool": "doc_open", "args": { "path": "book.pdf" } },
        { "tool": "chess_reference_status", "args": { "doc": 1 } },
        { "tool": "chess_reference_set", "args": { "doc": 1, "path": "other.pgn" } },
    ]);
    std::fs::write(dir.join("steps.json"), steps.to_string()).unwrap();
    let out = std::process::Command::new(&cli)
        .args(["run", "--report", "--script"])
        .arg(dir.join("steps.json"))
        .arg("--root")
        .arg(&dir)
        .output()
        .unwrap_or_else(|e| panic!("{}: {e}", cli.display()));
    let report: serde_json::Value = serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)));
    let status = &report["steps"][1]["result"][0];
    assert_eq!((status["kept"].as_bool(), status["file"].as_str()), (Some(false), Some(name_before(&keys).as_str())), "{report:#}");
    assert_eq!(report["steps"][2]["status"], "ok", "{report:#}");
    let after = folder(&folder_dir);
    let own = own_name(&keys);
    assert_eq!(after.iter().find(|(n, _)| *n == own), ours.iter().find(|(n, _)| *n == own), "this build's file left as it was");
    assert!(after.iter().all(|(n, _)| !n.contains(".bad")), "nothing set aside: {:?}", after.iter().map(|(n, _)| n).collect::<Vec<_>>());
    // What it kept under the name before, later: seen here.
    assert_eq!(store.load(&keys, pages).unwrap().pgn().map(|p| p.text.as_str()), Some(OTHER));
    let _ = std::fs::remove_dir_all(&dir);
}
