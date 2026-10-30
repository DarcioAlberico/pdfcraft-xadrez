use super::*;

const PGN: &str = "[Event \"A\"]\n\n1. e4 e5 2. Nf3 *\n";
const OTHER: &str = "[Event \"B\"]\n\n1. d4 d5 *\n";

fn keys() -> DocKeys {
    DocKeys { id0: None, path: Some("C:/books/Livro.pdf".into()), name: Some("Livro.pdf".into()), size: Some(1234) }
}

fn saved_as() -> DocKeys {
    DocKeys { id0: Some([7; 16]), path: Some("C:/books/Livro-2.pdf".into()), name: Some("Livro-2.pdf".into()), size: Some(1300) }
}

fn kept(text: &str) -> KeptPgn {
    KeptPgn::new(Some("livro.pgn"), text.to_owned(), 1, 3, 100).unwrap()
}

/// A folder of its own under the temp folder, empty.
fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcraft-reference-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn a_file_round_trips_and_bad_tampered_or_newer_files_are_told_apart() {
    let file = ReferenceFile::new(DocRecord::new(&keys(), 10), kept(PGN));
    let bytes = file.to_bytes().unwrap();
    assert_eq!(ReferenceFile::read(&bytes), Read::File(file.clone()));
    assert!(!String::from_utf8_lossy(&bytes).contains("C:/books"), "the document's path is kept only as a hash");

    // The same size, another text: only the hash tells.
    let mut tampered = file.clone();
    tampered.pgn.text = PGN.replace("e5", "c5");
    assert!(matches!(ReferenceFile::read(&serde_json::to_vec(&tampered).unwrap()), Read::Bad(why) if why.contains("hash")));
    let mut short = file.clone();
    short.pgn.bytes = 3;
    assert!(matches!(ReferenceFile::read(&serde_json::to_vec(&short).unwrap()), Read::Bad(_)));

    let mut newer = file.clone();
    newer.version = VERSION + 1;
    assert_eq!(ReferenceFile::read(&serde_json::to_vec(&newer).unwrap()), Read::Newer(VERSION + 1, Some(newer.clone())));
    assert!(matches!(ReferenceFile::read(b"{\"format\":\"pdfcraft-chess-reference\",\"version\":9}"), Read::Newer(9, None)));
    assert!(matches!(ReferenceFile::read(b"{\"format\":\"pdfcraft-chess-corrections\",\"version\":1}"), Read::Bad(_)));
    assert!(matches!(ReferenceFile::read(b"not json"), Read::Bad(_)));
    assert!(matches!(ReferenceFile::read(b"{\"format\":\"pdfcraft-chess-reference\",\"version\":0}"), Read::Bad(_)));
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
    assert_eq!(store.load(&keys(), 10).unwrap().file, None);
    assert!(!store.clear(&keys(), 10).unwrap(), "nothing to remove, and the folder is not made");
    assert!(!dir.exists());

    let (had, loaded) = store.set(&keys(), 10, kept(PGN)).unwrap();
    assert!(!had);
    assert_eq!(loaded.pgn().map(|p| p.text.as_str()), Some(PGN));
    assert!(!dir.join("reference.lock").exists(), "the lock is let go");
    let again = store.load(&keys(), 10).unwrap();
    assert_eq!(again.pgn(), loaded.pgn());
    assert_eq!(again.name, corrections::file_name(&keys()).unwrap());

    let (had, _) = store.set(&keys(), 10, kept(OTHER)).unwrap();
    assert!(had, "a PGN was kept before");
    assert_eq!(store.load(&keys(), 10).unwrap().pgn().map(|p| p.text.as_str()), Some(OTHER));
    let other_doc = DocKeys { id0: None, path: Some("C:/books/Outro.pdf".into()), name: Some("Outro.pdf".into()), size: Some(9) };
    assert_eq!(store.load(&other_doc, 3).unwrap().file, None, "a document finds only its own");

    assert!(store.clear(&keys(), 10).unwrap());
    assert_eq!(store.load(&keys(), 10).unwrap().file, None);
    assert!(!store.clear(&keys(), 10).unwrap());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_bad_file_is_set_aside_and_a_newer_one_is_only_read() {
    let dir = temp_dir("bad");
    std::fs::create_dir_all(&dir).unwrap();
    let store = DirStore::new(&dir);
    let name = corrections::file_name(&keys()).unwrap();
    std::fs::write(dir.join(&name), b"{ not json").unwrap();
    let loaded = store.load(&keys(), 10).unwrap();
    assert_eq!(loaded.file, None);
    let (aside, why) = loaded.set_aside.unwrap();
    assert_eq!(aside, format!("{name}.bad"));
    assert!(why.contains("JSON"));
    assert_eq!(std::fs::read(dir.join(&aside)).unwrap(), b"{ not json", "set aside, never deleted");

    let mut newer = ReferenceFile::new(DocRecord::new(&keys(), 10), kept(PGN));
    newer.version = VERSION + 1;
    let bytes = serde_json::to_vec(&newer).unwrap();
    std::fs::write(dir.join(&name), &bytes).unwrap();
    let loaded = store.load(&keys(), 10).unwrap();
    assert_eq!(loaded.read_only, Some(VERSION + 1));
    assert_eq!(loaded.pgn().map(|p| p.text.as_str()), Some(PGN), "read all the same");
    assert!(matches!(store.set(&keys(), 10, kept(OTHER)), Err(ReferenceStoreError::Newer(_))));
    assert!(matches!(store.clear(&keys(), 10), Err(ReferenceStoreError::Newer(_))));
    assert!(!store.rekey(&keys(), &saved_as(), 10).unwrap());
    assert_eq!(std::fs::read(dir.join(&name)).unwrap(), bytes, "never written or removed");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_held_lock_refuses_a_change_and_writes_nothing() {
    let dir = temp_dir("lock");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("reference.lock"), b"").unwrap();
    let store = DirStore::new(&dir);
    assert!(matches!(store.set(&keys(), 10, kept(PGN)), Err(ReferenceStoreError::Busy)));
    assert_eq!(store.load(&keys(), 10).unwrap().file, None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_saved_document_keeps_its_pgn_under_its_new_keys() {
    let dir = temp_dir("rekey");
    let store = DirStore::new(&dir);
    assert!(!store.rekey(&keys(), &saved_as(), 10).unwrap(), "no folder, nothing to move");
    store.set(&keys(), 10, kept(PGN)).unwrap();
    assert!(store.rekey(&keys(), &saved_as(), 10).unwrap());
    let loaded = store.load(&saved_as(), 10).unwrap();
    assert_eq!(loaded.pgn().map(|p| p.text.as_str()), Some(PGN), "found by the document it records");
    assert_eq!(loaded.name, corrections::file_name(&keys()).unwrap(), "in the file it had");
    assert!(loaded.file.unwrap().doc.matches(&saved_as()));
    assert!(!store.rekey(&saved_as(), &saved_as(), 10).unwrap());
    let _ = std::fs::remove_dir_all(&dir);
}

fn decision(inline: usize, choice: Choice) -> Decision {
    Decision { page: 2, block: 1, inline, san: "Bb5".into(), choice }
}

/// R6e: a decision on a move in conflict is kept with the PGN, in place of the one on the same
/// move; it goes with a save as another file, and keeping another PGN drops it. Without a PGN, or
/// naming no move, it is refused.
#[test]
fn decisions_are_kept_with_the_pgn_they_were_made_against() {
    let dir = temp_dir("decide");
    let store = DirStore::new(&dir);
    assert!(matches!(store.decide(&keys(), 10, decision(3, Choice::Pgn)), Err(ReferenceStoreError::NoPgn)), "no folder, no PGN");
    store.set(&keys(), 10, kept(PGN)).unwrap();
    store.decide(&keys(), 10, decision(3, Choice::Pgn)).unwrap();
    store.decide(&keys(), 10, decision(5, Choice::Book)).unwrap();
    let file = store.decide(&keys(), 10, decision(3, Choice::Book)).unwrap();
    assert_eq!(file.decisions, vec![decision(5, Choice::Book), decision(3, Choice::Book)], "the same move's decision replaced");
    assert_eq!(store.load(&keys(), 10).unwrap().file.unwrap().decisions.len(), 2, "kept on disk");
    let mut nameless = decision(7, Choice::Pgn);
    nameless.san.clear();
    assert!(matches!(store.decide(&keys(), 10, nameless), Err(ReferenceStoreError::NoMove)));
    assert!(store.rekey(&keys(), &saved_as(), 10).unwrap());
    assert_eq!(store.load(&saved_as(), 10).unwrap().file.unwrap().decisions.len(), 2, "kept for the saved file");
    store.set(&saved_as(), 10, kept(OTHER)).unwrap();
    assert!(store.load(&saved_as(), 10).unwrap().file.unwrap().decisions.is_empty(), "another PGN, no decisions");
    // A file without decisions (as R5 wrote it) reads, and one with a bad decision does not.
    let bytes = ReferenceFile::new(DocRecord::new(&keys(), 10), kept(PGN)).to_bytes().unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("decisions"));
    let mut bad = ReferenceFile::new(DocRecord::new(&keys(), 10), kept(PGN));
    bad.decisions.push(Decision { san: "x".repeat(MAX_SAN + 1), ..decision(1, Choice::Book) });
    assert!(matches!(ReferenceFile::read(&serde_json::to_vec(&bad).unwrap()), Read::Bad(_)));
    let memory = MemStore::new(4096);
    assert!(matches!(memory.decide(&keys(), 10, decision(3, Choice::Pgn)), Err(ReferenceStoreError::NoPgn)));
    memory.set(&keys(), 10, kept(PGN)).unwrap();
    assert_eq!(memory.decide(&keys(), 10, decision(3, Choice::Pgn)).unwrap().decisions.len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn memory_keeps_within_its_budget_and_round_trips() {
    let store = MemStore::new(4096);
    assert!(matches!(store.load(&DocKeys::default(), 1), Err(ReferenceStoreError::NoKeys)));
    store.set(&keys(), 10, kept(PGN)).unwrap();
    let big = "1. e4 *\n".repeat(600);
    assert!(matches!(store.set(&saved_as(), 3, kept(&big)), Err(ReferenceStoreError::OverBudget(4096))));
    assert_eq!(store.load(&saved_as(), 3).unwrap().file, None, "nothing kept when refused");

    let again = MemStore::with_files(store.files(), 4096);
    assert_eq!(again.load(&keys(), 10).unwrap().pgn().map(|p| p.text.as_str()), Some(PGN));
    assert!(again.rekey(&keys(), &saved_as(), 10).unwrap());
    assert_eq!(again.load(&saved_as(), 10).unwrap().pgn().map(|p| p.text.as_str()), Some(PGN));
    assert!(again.clear(&saved_as(), 10).unwrap());
    assert!(again.files().is_empty());
    assert!(!again.clear(&saved_as(), 10).unwrap());
    assert!(MemStore::with_files(store.files(), 10).files().is_empty(), "a file over the budget is left out");
}
