use super::*;
use crate::chess::store::file_name;
use pdfcraft_chessvision::doc::MoveAt;

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

    let (had, loaded) = store.set(&keys(), 10, kept(PGN), any).unwrap();
    assert!(!had.replaced);
    assert_eq!(loaded.pgn().map(|p| p.text.as_str()), Some(PGN));
    assert!(!dir.join("reference.lock").exists(), "no lock of the builds before taken: they never write this file");
    let again = store.load(&keys(), 10).unwrap();
    assert_eq!(again.pgn(), loaded.pgn());
    assert_eq!(again.name, own_name(&keys()));
    assert!(dir.join(own_name(&keys())).is_file() && !dir.join(name_before(&keys())).exists(), "written under its own name only");

    let (had, _) = store.set(&keys(), 10, kept_at(OTHER, 200), any).unwrap();
    assert!(had.replaced, "a PGN was kept before");
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
    let (_, changed) = store.set(&keys(), 10, kept(PGN), any).unwrap();
    assert_eq!(changed.set_aside.map(|(n, _)| n), Some(format!("{name}.bad")));
    assert_eq!(std::fs::read(dir.join(format!("{name}.bad"))).unwrap(), b"{ not json", "set aside, never deleted");

    let newer = String::from_utf8(ReferenceFile::new(DocRecord::new(&keys(), 10), kept(PGN)).to_bytes().unwrap())
        .unwrap()
        .replace("\"version\":2", "\"version\":3");
    std::fs::write(dir.join(&name), &newer).unwrap();
    let loaded = store.load(&keys(), 10).unwrap();
    assert_eq!((loaded.read_only, loaded.newer), (true, Some(3)));
    assert_eq!(loaded.pgn().map(|p| p.text.as_str()), Some(PGN), "read all the same");
    assert!(matches!(store.set(&keys(), 10, kept(OTHER), any), Err(ReferenceStoreError::Newer(3))));
    assert!(matches!(store.clear(&keys(), 10, 9), Err(ReferenceStoreError::Newer(3))));
    assert!(matches!(store.decide(&keys(), 10, decision(3, Choice::Pgn), None), Err(ReferenceStoreError::Newer(3))));
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
    assert!(matches!(store.set(&keys(), 10, kept(PGN), any), Err(ReferenceStoreError::Busy)));
    assert_eq!(store.load(&keys(), 10).unwrap().pgn(), None);
    held.unlock().unwrap();
    store.set(&keys(), 10, kept(PGN), any).unwrap();
    assert!(dir.join("reference.lock").exists(), "the old lock file is not this kind's any more, and left alone");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_saved_document_keeps_its_pgn_under_its_new_keys() {
    let dir = temp_dir("rekey");
    let store = DirStore::new(&dir);
    assert!(!store.rekey(&keys(), &saved_as(), 10).unwrap(), "no folder, nothing to move");
    assert!(!dir.exists());
    store.set(&keys(), 10, kept(PGN), any).unwrap();
    assert!(store.rekey(&keys(), &saved_as(), 10).unwrap());
    let loaded = store.load(&saved_as(), 10).unwrap();
    assert_eq!(loaded.pgn().map(|p| p.text.as_str()), Some(PGN), "found by the document it records");
    assert_eq!(loaded.name, own_name(&keys()), "in the file it had");
    assert!(loaded.file.doc.matches(&saved_as()));
    assert!(!store.rekey(&saved_as(), &saved_as(), 10).unwrap());
    let _ = std::fs::remove_dir_all(&dir);
}

fn decision(inline: u32, choice: Choice) -> Decision {
    Decision { at: MoveAt { page: 2, block: 1, inline }, san: "Bb5".into(), san_read: None, ordinal: None, choice, pgn_san: None, other: Map::new() }
}

/// Every move counts as one the PGN has.
fn any(_: &str) -> bool {
    true
}

/// A page of the reading as the interface has it: `blocks`, each a paragraph of moves given as
/// (SAN as read, SAN the walk wrote) or of words (`None`).
fn page(index: u32, blocks: &[&[Option<(&str, &str)>]]) -> ChessPage {
    use pdfcraft_chessvision::doc::{Block, BlockRole, MoveInline, MoveStatus, TextLayer};
    let inline = |i: &Option<(&str, &str)>| match i {
        Some((read, san)) => Inline::Move(MoveInline {
            number: None,
            text: (*read).into(),
            san_read: (*read).into(),
            san: (*san).into(),
            status: MoveStatus::Valid,
            fen: None,
            game: None,
            source_rect: None,
            bold: false,
            read_review: Vec::new(),
            review: Vec::new(),
            reference: None,
        }),
        None => Inline::Text { text: "words".into(), source_rect: None, review: Vec::new() },
    };
    ChessPage {
        index,
        lang: "en".into(),
        text_lang: Default::default(),
        text_layer: TextLayer::Good,
        ocr: false,
        blocks: blocks
            .iter()
            .map(|b| Block::Paragraph { inlines: b.iter().map(inline).collect(), source_rect: None, role: BlockRole::Content })
            .collect(),
        review: Vec::new(),
        trailing: None,
        running_heads: Vec::new(),
    }
}

/// Plan A9-L0b (R3-01): a decision is for its move by the SAN as read. A move walked again from a
/// corrected or confirmed diagram keeps its SAN read, while the SAN the walk writes can change (a
/// repair undone): the decision stays on it; and a move printed otherwise at the same place, which
/// the walk may write with the same SAN, is another move. A decision made before the SAN read was
/// kept still goes by the walk's SAN.
#[test]
fn a_decision_stays_on_its_move_by_the_san_read() {
    let at = MoveAt { page: 0, block: 2, inline: 3 };
    let read = |read: &str, san: &str| page(0, &[&[None], &[None], &[None, None, None, Some((read, san))]]);
    let d = Decision::new(at, "exd4".into(), "cxd4".into(), String::new(), Choice::Book).placed_on(&read("cxd4", "exd4"));
    assert_eq!((d.san_read.as_deref(), d.ordinal), (Some("cxd4"), Some(0)));
    assert_eq!(d.find(&read("cxd4", "exd4")), Some(at), "as decided");
    assert_eq!(d.find(&read("cxd4", "cxd4")), Some(at), "walked again under the corrected diagram: the same printed move");
    assert_eq!(d.find(&read("exd4", "exd4")), None, "another printed move at the same place");
    assert_eq!(d.find(&ChessPage { index: 1, ..read("cxd4", "exd4") }), None, "another page");
    let older = Decision { san_read: None, ordinal: None, ..d.clone() };
    assert_eq!(older.find(&read("anything", "exd4")), Some(at), "an older decision by the walk's SAN");
    assert_eq!(older.find(&read("cxd4", "cxd4")), None);
    // Written as before, with the SAN read and the ordinal beside; read by a build before as
    // before.
    let json = serde_json::to_string(&d).unwrap();
    assert_eq!(json, r#"{"page":0,"block":2,"inline":3,"san":"exd4","san_read":"cxd4","ordinal":0,"choice":"book"}"#);
    assert_eq!(serde_json::from_str::<Decision>(r#"{"page":0,"block":2,"inline":3,"san":"exd4","choice":"book"}"#).unwrap(), older);
    // Deciding again on the same move replaces the choice, whatever the walk wrote since.
    let mut file = ReferenceFile::new(DocRecord::new(&keys(), 10), kept(PGN));
    file.decide(d.clone()).unwrap();
    file.decide(Decision::new(at, "cxd4".into(), "cxd4".into(), String::new(), Choice::Pgn)).unwrap();
    assert_eq!(file.decisions.iter().map(|d| d.choice).collect::<Vec<_>>(), [Choice::Pgn]);
    let long = Decision::new(at, "exd4".into(), "x".repeat(MAX_SAN + 1), String::new(), Choice::Book);
    assert!(matches!(file.decide(long), Err(ReferenceStoreError::NoMove)));
}

/// Plan A9-L1b (§2.6, "Endereços estáveis"): a reading that cuts a page into other blocks (A9-L1a
/// set a diagram's text apart, L3 joins a paragraph the layout cut) moves its moves to other places,
/// and a decision kept by its place found nothing there any more. It is found again by its move:
/// the only one of its page printed as it was, or, for a decision made since, the one of its
/// ordinal among them; one that is neither is kept and counted, never put on another move. The
/// interface takes the decisions of a page it reads to their moves once, and writes them.
#[test]
fn a_decision_made_under_an_earlier_reading_is_found_again() {
    // The reading the decisions were made under: the page's number, then one paragraph.
    let before = page(4, &[&[None], &[Some(("e4", "e4")), None, Some(("Nf3", "Nf3")), None, Some(("Bb5", "Bb5")), Some(("Nf3", "Nf3"))]]);
    let made = |inline: usize, read: &str, choice: Choice| Decision {
        ordinal: None,
        ..Decision::new(MoveAt { page: 4, block: 1, inline: inline as u32 }, read.into(), read.into(), String::new(), choice)
    };
    let bb5 = made(4, "Bb5", Choice::Book);
    let second_nf3 = made(5, "Nf3", Choice::Pgn);
    let elsewhere_nf3 = made(1, "Nf3", Choice::Book);
    assert_eq!((bb5.find(&before), second_nf3.find(&before)), (Some(bb5.at), Some(second_nf3.at)), "found where they were made");
    // The page read now: the paragraph cut in two after `e4 …`, its number last.
    let now = page(4, &[&[Some(("e4", "e4")), None], &[Some(("Nf3", "Nf3")), None, Some(("Bb5", "Bb5")), Some(("Nf3", "Nf3"))], &[None]]);
    let at = |block: u32, inline: u32| Some(MoveAt { page: 4, block, inline });
    assert_eq!(bb5.find(&now), at(1, 2), "the only Bb5 of the page");
    assert_eq!(second_nf3.find(&now), None, "two Nf3 and its place holds neither: not found, not guessed");
    assert_eq!(elsewhere_nf3.find(&now), None);
    // Made under this reading, a decision keeps its move's ordinal, which a later reading finds.
    let placed = Decision::new(at(1, 3).unwrap(), "Nf3".into(), "Nf3".into(), String::new(), Choice::Pgn).placed_on(&now);
    assert_eq!(placed.ordinal, Some(1));
    let later = page(4, &[&[Some(("e4", "e4")), None, Some(("Nf3", "Nf3")), None], &[Some(("Bb5", "Bb5")), Some(("Nf3", "Nf3"))]]);
    assert_eq!(placed.find(&later), at(1, 1));

    // What the interface shows of the page: the choices found, and how many were not.
    let mut file = ReferenceFile::new(DocRecord::new(&keys(), 10), kept(PGN));
    for d in [bb5.clone(), second_nf3.clone(), made(0, "e4", Choice::Book)] {
        file.decide(d).unwrap();
    }
    file.reader_version = 1;
    let shown = Decisions::of(&file).on_page(&now);
    assert_eq!((shown.found.as_slice(), shown.unmatched), (&[(at(0, 0).unwrap(), Choice::Book), (at(1, 2).unwrap(), Choice::Book)][..], 1));
    assert!(Decisions::of(&file).need_rematch(&now));
    // Taken to their moves once: places, SANs read and ordinals; the one not found as it was.
    assert_eq!(file.rematch(&now), 2);
    assert_eq!(
        file.decisions.iter().map(|d| (Some(d.at), d.ordinal)).collect::<Vec<_>>(),
        [(at(1, 2), Some(0)), (Some(second_nf3.at), None), (at(0, 0), Some(0))]
    );
    assert_eq!(file.rematch(&now), 0, "once");
    assert!(!Decisions::of(&file).need_rematch(&now));
    // Written with the reading that wrote it; a file before it says 0.
    let bytes = file.to_bytes().unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(text.contains(&format!("\"reader_version\":{READER_VERSION}")) && text.contains("\"ordinal\":0"), "{text}");
    let Parsed::File { file: back, .. } = ReferenceFile::parse(&bytes) else { panic!("read back") };
    assert_eq!(back.reader_version, READER_VERSION);
    let before_l1b = text.replace(&format!(",\"reader_version\":{READER_VERSION}"), "");
    let Parsed::File { file: old, .. } = ReferenceFile::parse(before_l1b.as_bytes()) else { panic!("read back") };
    assert_eq!(old.reader_version, 0);
}

/// Plan A9 §2.10: a file a later reading wrote (a build after the next `READER_VERSION`) is read
/// only, and none of its decisions is applied here: their places and ordinals are that reading's.
#[test]
fn a_file_a_later_reading_wrote_is_read_only_and_applies_no_decision() {
    let now = page(4, &[&[Some(("Bb5", "Bb5"))]]);
    let mut file = ReferenceFile::new(DocRecord::new(&keys(), 10), kept(PGN));
    file.decide(Decision::new(MoveAt { page: 4, block: 0, inline: 0 }, "Bb5".into(), "Bb5".into(), String::new(), Choice::Book).placed_on(&now))
        .unwrap();
    let later = READER_VERSION + 1;
    let text = String::from_utf8(file.to_bytes().unwrap())
        .unwrap()
        .replace(&format!("\"reader_version\":{READER_VERSION}"), &format!("\"reader_version\":{later}"));
    let Parsed::Newer { version, file: Some(newer) } = ReferenceFile::parse(text.as_bytes()) else { panic!("read only: {text}") };
    assert_eq!((version, newer.reader_version, newer.decisions.len()), (later, later, 1));
    let shown = Decisions::of(&newer).on_page(&now);
    assert_eq!((shown.found.len(), shown.unmatched), (0, 1));
    assert!(!Decisions::of(&newer).need_rematch(&now));
    // In a folder: nothing is written, and deciding is refused.
    let dir = temp_dir("later-reading");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(own_name(&keys())), &text).unwrap();
    let store = DirStore::new(&dir);
    assert_eq!(store.rematch(&keys(), 10, &now).unwrap(), None);
    let d = Decision::new(MoveAt { page: 4, block: 0, inline: 0 }, "Bb5".into(), "Bb5".into(), String::new(), Choice::Pgn);
    assert!(matches!(store.decide(&keys(), 10, d, Some(&now)), Err(ReferenceStoreError::Newer(v)) if v == later));
    assert_eq!(std::fs::read_to_string(dir.join(own_name(&keys()))).unwrap(), text, "left as it was");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Plan A9-L1b: a page read takes its decisions to their moves in the folder once; deciding on a
/// move of it replaces a choice made on the same move under an earlier reading.
#[test]
fn reading_a_page_rewrites_its_decisions_once() {
    let dir = temp_dir("rematch");
    let store = DirStore::new(&dir);
    let before = page(2, &[&[None], &[Some(("Bb5", "Bb5"))]]);
    let now = page(2, &[&[Some(("Bb5", "Bb5"))], &[None]]);
    assert_eq!(store.rematch(&keys(), 10, &now).unwrap(), None, "nothing kept, nothing written");
    assert!(!dir.exists());
    store.set(&keys(), 10, kept(PGN), any).unwrap();
    let old =
        Decision { ordinal: None, ..Decision::new(MoveAt { page: 2, block: 1, inline: 0 }, "Bb5".into(), "Bb5".into(), String::new(), Choice::Book) };
    assert_eq!(old.find(&before), Some(old.at));
    store.decide(&keys(), 10, old, None).unwrap();
    let written = store.rematch(&keys(), 10, &now).unwrap().expect("taken to its move");
    assert_eq!(
        written.file.decisions.iter().map(|d| (Some(d.at), d.ordinal)).collect::<Vec<_>>(),
        [(Some(MoveAt { page: 2, block: 0, inline: 0 }), Some(0))]
    );
    assert_eq!(store.rematch(&keys(), 10, &now).unwrap(), None, "once");
    // Under the earlier reading again: the choice made since replaces it.
    let mut file = store.load(&keys(), 10).unwrap().file;
    file.decisions[0].at.block = 1;
    file.decisions[0].ordinal = None;
    let d = Decision::new(MoveAt { page: 2, block: 0, inline: 0 }, "Bb5".into(), "Bb5".into(), String::new(), Choice::Pgn).placed_on(&now);
    std::fs::write(dir.join(own_name(&keys())), file.to_bytes().unwrap()).unwrap();
    let kept = store.decide(&keys(), 10, d, Some(&now)).unwrap().file;
    assert_eq!(kept.decisions.iter().map(|d| d.choice).collect::<Vec<_>>(), [Choice::Pgn]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// R6e: a decision on a move in conflict is kept with the PGN, in place of the one on the same
/// move; it goes with a save as another file, and (decision D25, plan A9-G4) stays when another
/// PGN is kept. Without a PGN, or naming no move, it is refused.
#[test]
fn decisions_are_kept_with_the_pgn_they_were_made_against() {
    let dir = temp_dir("decide");
    let store = DirStore::new(&dir);
    assert!(matches!(store.decide(&keys(), 10, decision(3, Choice::Pgn), None), Err(ReferenceStoreError::NoPgn)), "no folder, no PGN");
    assert!(!dir.exists(), "and none made");
    store.set(&keys(), 10, kept(PGN), any).unwrap();
    store.decide(&keys(), 10, decision(3, Choice::Pgn), None).unwrap();
    store.decide(&keys(), 10, decision(5, Choice::Book), None).unwrap();
    let file = store.decide(&keys(), 10, decision(3, Choice::Book), None).unwrap().file;
    assert_eq!(file.decisions, vec![decision(5, Choice::Book), decision(3, Choice::Book)], "the same move's decision replaced");
    assert_eq!(store.load(&keys(), 10).unwrap().file.decisions.len(), 2, "kept on disk");
    let mut nameless = decision(7, Choice::Pgn);
    nameless.san.clear();
    assert!(matches!(store.decide(&keys(), 10, nameless, None), Err(ReferenceStoreError::NoMove)));
    assert!(store.rekey(&keys(), &saved_as(), 10).unwrap());
    assert_eq!(store.load(&saved_as(), 10).unwrap().file.decisions.len(), 2, "kept for the saved file");
    store.set(&saved_as(), 10, kept_at(OTHER, 200), any).unwrap();
    assert_eq!(store.load(&saved_as(), 10).unwrap().file.decisions.len(), 2, "another PGN keeps them (D25)");
    // A file without decisions (as R5 wrote it) has none written.
    let bytes = ReferenceFile::new(DocRecord::new(&keys(), 10), kept(PGN)).to_bytes().unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("decisions"));
    let memory = MemStore::new(4096);
    assert!(matches!(memory.decide(&keys(), 10, decision(3, Choice::Pgn), None), Err(ReferenceStoreError::NoPgn)));
    memory.set(&keys(), 10, kept(PGN), any).unwrap();
    assert_eq!(memory.decide(&keys(), 10, decision(3, Choice::Pgn), None).unwrap().file.decisions.len(), 1);
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
    let written = store.decide(&keys(), 10, decision(9, Choice::Pgn), None).unwrap();
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
    let (_, l) = store.set(&keys(), 10, kept_at(PGN, 200), any).unwrap();
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
    store.set(&keys(), 10, kept_at(OTHER, 400), any).unwrap();
    store.decide(&keys(), 10, decision(1, Choice::Pgn), None).unwrap();
    store.decide(&keys(), 10, decision(2, Choice::Pgn), None).unwrap();
    let l = store.load(&keys(), 10).unwrap();
    let mut got: Vec<(u32, Choice)> = l.file.decisions.iter().map(|d| (d.at.inline, d.choice)).collect();
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
    store.decide(&keys(), 10, decision(2, Choice::Book), None).unwrap();
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
/// again keeps them too, and so does another PGN (D25), which counts the one it lacks.
#[test]
fn the_same_pgn_kept_again_keeps_its_decisions_even_as_an_older_build_read_it() {
    let dir = temp_dir("same-pgn");
    let store = DirStore::new(&dir);
    let bytes = b"[White \"R\xe9ti\"]\n\n1. e4 e5 2. Nf3 *\n";
    let before_f1 = KeptPgn::new(Some("livro.pgn"), String::from_utf8_lossy(bytes).into_owned(), 1, 3, 100).unwrap();
    store.set(&keys(), 10, before_f1, any).unwrap();
    store.decide(&keys(), 10, decision(3, Choice::Pgn), None).unwrap();
    let (text, encoding) = pdfcraft_chess::pgn::decode(bytes);
    assert_eq!(encoding, pdfcraft_chess::pgn::PgnEncoding::Windows1252);
    let now = KeptPgn::new(Some("livro.pgn"), text.clone(), 1, 3, 200).unwrap().read_from(bytes);
    let (replaced, loaded) = store.set(&keys(), 10, now, any).unwrap();
    assert_eq!((replaced.replaced, replaced.unmatched), (true, 0));
    let file = loaded.file;
    assert_eq!(file.decisions, vec![decision(3, Choice::Pgn)], "the same file, read as before F1");
    assert!(file.pgn.as_ref().unwrap().text.contains("Réti"));
    let on_disk = store.load(&keys(), 10).unwrap().file;
    assert_eq!((on_disk.decisions.len(), on_disk.pgn.map(|p| p.hash)), (1, Some(text_hash(&text))));
    // The same text again, then another PGN.
    store.set(&keys(), 10, KeptPgn::new(Some("livro.pgn"), text, 1, 3, 300).unwrap(), any).unwrap();
    assert_eq!(store.load(&keys(), 10).unwrap().file.decisions.len(), 1, "the same text");
    let (kept, _) = store.set(&keys(), 10, kept_at(OTHER, 400), |_| false).unwrap();
    assert_eq!((kept.replaced, kept.unmatched), (true, 1), "another PGN without its move");
    assert_eq!(store.load(&keys(), 10).unwrap().file.decisions.len(), 1, "kept all the same");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn memory_keeps_within_its_budget_and_round_trips() {
    let store = MemStore::new(4096);
    assert!(matches!(store.load(&DocKeys::default(), 1), Err(ReferenceStoreError::NoKeys)));
    store.set(&keys(), 10, kept(PGN), any).unwrap();
    let big = "1. e4 *\n".repeat(600);
    assert!(matches!(store.set(&saved_as(), 3, kept(&big), any), Err(ReferenceStoreError::OverBudget(4096))));
    assert_eq!(store.load(&saved_as(), 3).unwrap().pgn(), None, "nothing kept when refused");

    let again = MemStore::with_files(store.files(), 4096);
    assert_eq!(again.load(&keys(), 10).unwrap().pgn().map(|p| p.text.as_str()), Some(PGN));
    assert!(again.rekey(&keys(), &saved_as(), 10).unwrap());
    assert_eq!(again.load(&saved_as(), 10).unwrap().pgn().map(|p| p.text.as_str()), Some(PGN));
    assert!(again.clear(&saved_as(), 10, 500).unwrap());
    assert_eq!(again.load(&saved_as(), 10).unwrap().pgn(), None);
    assert!(!again.clear(&saved_as(), 10, 600).unwrap());
    // Over the budget when taken in: kept, never left out (plan A9-I1b, A4-15).
    assert_eq!(MemStore::with_files(store.files(), 10).files(), store.files(), "a file over the budget is kept");
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
    file.decide(Decision::new(
        pdfcraft_chessvision::doc::MoveAt { page: 1, block: 0, inline: 2 },
        "Nf3".into(),
        "Nf3".into(),
        String::new(),
        Choice::Pgn,
    ))
    .unwrap();
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
        page: usize::try_from(d.at.page).unwrap(),
        block: usize::try_from(d.at.block).unwrap(),
        inline: usize::try_from(d.at.inline).unwrap(),
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
    // Written, it says the reading that wrote it (plan A9-L1b); a version 1 file says none.
    assert_eq!(now.reader_version, 0);
    assert_eq!(again, ReferenceFile { reader_version: READER_VERSION, ..now });
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
    store.set(&keys, pages, kept_at(PGN, 100), any).unwrap();
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

// ─── The PGN's move, the file's bytes, another PGN (plan A9-G4, decision D25) ──────────────────

/// A9-G4 (A4-08, D25, R6-10): keeping another PGN used to drop every decision made against the
/// one before. They stay; each knows the PGN's move it was made against (`pgn_san`), and those
/// whose move the new PGN doesn't have are counted (they apply again only to a PGN that has it,
/// since a choice is looked up only for a move in conflict with that same move). The same PGN
/// kept again counts none.
#[test]
fn another_pgn_keeps_the_decisions_and_counts_those_it_lacks() {
    let dir = temp_dir("d25");
    let store = DirStore::new(&dir);
    store.set(&keys(), 10, kept(PGN), any).unwrap();
    let at = |inline| MoveAt { page: 0, block: 1, inline };
    store.decide(&keys(), 10, Decision::new(at(1), "Nf3".into(), "Nf3".into(), "Nf3".into(), Choice::Pgn), None).unwrap();
    store.decide(&keys(), 10, Decision::new(at(2), "Bc4".into(), "Bc4".into(), "Bb5+".into(), Choice::Book), None).unwrap();
    // An older decision, made before the PGN's move was kept: it stands for its own SAN.
    let mut older = decision(3, Choice::Book);
    older.san = "Qh5".into();
    store.decide(&keys(), 10, older, None).unwrap();
    let file = store.load(&keys(), 10).unwrap().file;
    assert_eq!(file.decisions.iter().map(|d| d.pgn_move()).collect::<Vec<_>>(), ["Nf3", "Bb5+", "Qh5"]);
    // Another PGN that plays Nf3 and Bb5 (check marks set aside) but not Qh5.
    let has = |san: &str| ["Nf3", "Bb5"].contains(&san.trim_end_matches('+'));
    let (kept_other, loaded) = store.set(&keys(), 10, kept_at(OTHER, 200), has).unwrap();
    assert_eq!((kept_other.replaced, kept_other.unmatched), (true, 1));
    assert_eq!(loaded.file.decisions, file.decisions, "every decision kept");
    // The same PGN again: nothing counted, whatever it has.
    let (again, _) = store.set(&keys(), 10, kept_at(OTHER, 300), |_| false).unwrap();
    assert_eq!(again.unmatched, 0);
    // Written with the PGN's move beside the rest, the address as the three numbers it always was.
    let json = serde_json::to_string(&file.decisions[1]).unwrap();
    assert_eq!(json, r#"{"page":0,"block":1,"inline":2,"san":"Bc4","san_read":"Bc4","choice":"book","pgn_san":"Bb5+"}"#);
    let too_long = Decision::new(at(4), "Nf3".into(), String::new(), "x".repeat(MAX_SAN + 1), Choice::Pgn);
    assert!(matches!(store.decide(&keys(), 10, too_long, None), Err(ReferenceStoreError::NoMove)));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A9-G4: the same file is the same PGN by its bytes too (`bytes_hash`), so a reader that makes
/// another text of it (a decoder changed again, as F1 did) keeps the decisions as the same PGN's;
/// a PGN given as text, without bytes, is known by its text alone. The hash is kept in the file.
#[test]
fn the_same_files_bytes_are_the_same_pgn() {
    let bytes = PGN.as_bytes();
    let a = KeptPgn::new(Some("livro.pgn"), PGN.into(), 1, 3, 1).unwrap().read_from(bytes);
    let read_otherwise = KeptPgn::new(Some("livro.pgn"), PGN.replace("Nf3", "Nf3 "), 1, 3, 2).unwrap().read_from(bytes);
    assert_ne!(a.hash, read_otherwise.hash);
    assert!(read_otherwise.is_same_as(&a), "the same bytes");
    let as_text = KeptPgn::new(None, PGN.replace("Nf3", "Nf3 "), 1, 3, 3).unwrap();
    assert!(!as_text.is_same_as(&a), "no bytes to tell");
    assert_eq!(a.bytes_hash.as_deref(), Some(bytes_hash(bytes).as_str()));
    let file = ReferenceFile::new(DocRecord::new(&keys(), 10), a.clone());
    let Read::File(back) = ReferenceFile::read(&file.to_bytes().unwrap()) else { panic!("read back") };
    assert_eq!(back.pgn.and_then(|p| p.bytes_hash), a.bytes_hash);
    let mut decided = ReferenceFile::new(DocRecord::new(&keys(), 10), a);
    decided.decide(decision(1, Choice::Pgn)).unwrap();
    assert_eq!(decided.keep(read_otherwise, |_| false).unmatched, 0, "the same PGN: nothing counted");
}

/// A9-G4 (D25) in a merge: with another PGN on the other side, the later PGN wins and both sides'
/// decisions are kept, the later side's where both decided on one move; a removal, the later,
/// still takes them all away.
#[test]
fn a_merge_keeps_both_sides_decisions_across_pgns() {
    let mut mine = ReferenceFile::new(DocRecord::new(&keys(), 10), kept_at(PGN, 100));
    mine.decide(decision(1, Choice::Book)).unwrap();
    mine.decide(decision(2, Choice::Book)).unwrap();
    let mut theirs = ReferenceFile::new(DocRecord::new(&keys(), 10), kept_at(OTHER, 200));
    theirs.decide(decision(2, Choice::Pgn)).unwrap();
    let mut merged = mine.clone();
    merged.merge(&theirs);
    assert_eq!(merged.pgn.as_ref().map(|p| p.text.as_str()), Some(OTHER));
    let mut got: Vec<(u32, Choice)> = merged.decisions.iter().map(|d| (d.at.inline, d.choice)).collect();
    got.sort_by_key(|(i, _)| *i);
    assert_eq!(got, [(1, Choice::Book), (2, Choice::Pgn)]);
    let mut gone = theirs.clone();
    gone.remove(300);
    merged.merge(&gone);
    assert_eq!((merged.pgn.is_none(), merged.decisions.len()), (true, 0));
}

#[allow(dead_code)]
#[path = "../../../tests/compat/reference_v2.rs"]
mod reference_v2;

/// A9-G4, plan §2.10: version 2 as G3a wrote it (frozen in `reference_v2`) reads what this build
/// writes as its own version, and writes back the PGN's move of each decision and the hash of the
/// file's bytes it does not know, as the fields of a later build of its version; a decision it
/// makes keeps the others' moves.
#[test]
fn compat_a_g3a_build_keeps_what_this_one_adds() {
    let bytes = PGN.as_bytes();
    let mut file = ReferenceFile::new(DocRecord::new(&keys(), 10), kept(PGN).read_from(bytes));
    file.decide(Decision::new(MoveAt { page: 1, block: 0, inline: 2 }, "Nf3".into(), "Nf3".into(), "Nf3".into(), Choice::Pgn)).unwrap();
    let reference_v2::Parsed::File { file: mut theirs, skipped: 0 } = reference_v2::ReferenceFile::parse(&file.to_bytes().unwrap()) else {
        panic!("a G3a build reads it as its own version")
    };
    let made_there = reference_v2::Decision {
        page: 3,
        block: 0,
        inline: 0,
        san: "e4".into(),
        san_read: None,
        choice: reference_v2::Choice::Book,
        other: Default::default(),
    };
    theirs.decide(made_there).unwrap();
    let Parsed::File { file: back, skipped } = ReferenceFile::parse(&theirs.to_bytes().unwrap()) else { panic!("read back") };
    assert_eq!(skipped, 0);
    assert_eq!(back.pgn.and_then(|p| p.bytes_hash), Some(bytes_hash(bytes)), "the bytes' hash kept");
    assert_eq!(back.decisions.iter().map(|d| d.pgn_san.as_deref()).collect::<Vec<_>>(), [Some("Nf3"), None], "the PGN's move kept");
}

/// A9-L1b, plan §2.10 (what an earlier build sees): a build of version 2 before this step (G3a,
/// frozen in `reference_v2`) reads what this build writes as its own version, and writes back the
/// reading that wrote it and each decision's ordinal, as fields of a later build of its version.
/// A decision it adds has no ordinal, and is found here by its move.
#[test]
fn compat_an_earlier_build_keeps_the_reading_and_the_ordinals() {
    let now = page(1, &[&[Some(("Nf3", "Nf3")), None, Some(("Nf3", "Nf3"))], &[None, Some(("e4", "e4"))]]);
    let mut file = ReferenceFile::new(DocRecord::new(&keys(), 10), kept(PGN));
    file.decide(Decision::new(MoveAt { page: 1, block: 0, inline: 2 }, "Nf3".into(), "Nf3".into(), String::new(), Choice::Pgn).placed_on(&now))
        .unwrap();
    let reference_v2::Parsed::File { file: mut theirs, skipped: 0 } = reference_v2::ReferenceFile::parse(&file.to_bytes().unwrap()) else {
        panic!("a G3a build reads it as its own version")
    };
    let made_there = reference_v2::Decision {
        page: 1,
        block: 0,
        inline: 1,
        san: "e4".into(),
        san_read: Some("e4".into()),
        choice: reference_v2::Choice::Book,
        other: Default::default(),
    };
    theirs.decide(made_there).unwrap();
    let Parsed::File { file: back, skipped } = ReferenceFile::parse(&theirs.to_bytes().unwrap()) else { panic!("read back") };
    assert_eq!(skipped, 0);
    assert_eq!(back.reader_version, READER_VERSION, "the reading that wrote it, kept");
    assert_eq!(back.decisions.iter().map(|d| d.ordinal).collect::<Vec<_>>(), [Some(1), None], "the ordinal kept");
    let shown = Decisions::of(&back).on_page(&now);
    let at = |block: u32, inline: u32| MoveAt { page: 1, block, inline };
    assert_eq!((shown.found.as_slice(), shown.unmatched), (&[(at(1, 1), Choice::Book), (at(0, 2), Choice::Pgn)][..], 0));
}
