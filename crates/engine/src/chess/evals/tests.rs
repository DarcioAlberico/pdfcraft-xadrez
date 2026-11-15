use super::*;
use crate::chess::store::file_name;

const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

fn keys() -> DocKeys {
    DocKeys { id0: None, path: Some("C:/books/Livro.pdf".into()), name: Some("Livro.pdf".into()), size: Some(1234) }
}

fn eval(depth: u32, cp: i32, at: u64) -> Eval {
    Eval::new(depth, Score::Centipawns(cp), Bound::Exact, &["e2e4".to_owned(), "e7e5".to_owned()], Some("Stockfish 17"), at).unwrap()
}

/// A folder of its own under the temp folder, empty.
fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcraft-evals-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// A position is FIDE's same position: the clocks do not split it, the side to move does.
#[test]
fn a_position_is_its_pieces_side_castling_and_en_passant() {
    assert_eq!(position_key(START).as_deref(), Some("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq -"));
    assert_eq!(position_key("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 7 40"), position_key(START));
    assert_ne!(position_key("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR b KQkq - 0 1"), position_key(START));
    assert_eq!(position_key("not a fen"), None);
}

/// Per position the deeper evaluation is kept (as deep: the newer), and the file reads back as
/// it was written.
#[test]
fn the_deeper_evaluation_is_kept_and_the_file_reads_back() {
    let mut f = EvalFile::empty(DocRecord::new(&keys(), 10));
    assert!(f.keep(START, eval(20, 30, 100)));
    assert!(!f.keep(START, eval(18, 50, 200)), "shallower");
    assert!(!f.keep(START, eval(20, 50, 50)), "as deep, older");
    assert!(f.keep(START, eval(20, 40, 300)), "as deep, newer");
    assert_eq!(f.get(START).map(|e| (e.depth, e.cp)), Some((20, Some(40))));
    assert!(!f.keep("not a fen", eval(30, 0, 400)));
    let mated = Eval::new(1, Score::Mated(pdfcraft_chess::placement::Color::White), Bound::Lower, &[], None, 5).unwrap();
    assert!(f.keep("7k/8/8/8/8/8/8/K7 w - - 0 1", mated));
    let bytes = f.to_bytes().unwrap();
    let Parsed::File { file, skipped } = EvalFile::parse(&bytes) else { panic!("read back") };
    assert_eq!((file, skipped), (f, 0));
    assert!(Eval::new(1, Score::Centipawns(0), Bound::Exact, &["e2e4\nquit".to_owned()], None, 0).is_none(), "a move that is not UCI");
}

/// What can't be read is left out and counted; another format is not this one; a newer version
/// is read only; unknown fields are kept.
#[test]
fn bad_entries_are_left_out_and_newer_files_only_read() {
    let doc = serde_json::to_value(DocRecord::new(&keys(), 3)).unwrap();
    let file = serde_json::json!({
        "format": FORMAT, "version": 1, "doc": doc, "later": {"x": 1},
        "evals": {
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq -": {"depth": 12, "cp": 25, "pv": ["e2e4"], "at": 7, "seldepth": 20},
            "not a position": {"depth": 1, "cp": 0, "pv": [], "at": 1},
            "8/8/8/8/8/8/8/K6k w - -": {"depth": 3, "pv": ["a1a2"], "at": 1},
            "8/8/8/8/8/8/8/K5k1 w - -": {"depth": 3, "cp": 1, "pv": ["a1a2\nquit"], "at": 1}
        }
    });
    let Parsed::File { file: f, skipped } = EvalFile::parse(&serde_json::to_vec(&file).unwrap()) else { panic!("read") };
    assert_eq!((f.evals.len(), skipped), (1, 3), "no position, no score, a bad move");
    let kept = f.get(START).unwrap();
    assert_eq!(kept.other.get("seldepth"), Some(&serde_json::json!(20)), "a later field is kept");
    assert!(String::from_utf8(f.to_bytes().unwrap()).unwrap().contains("\"later\""), "a later field of the file too");
    assert!(matches!(EvalFile::parse(br#"{"format":"pdfcraft-chess-reference","version":1}"#), Parsed::Bad(_)));
    let newer = serde_json::json!({"format": FORMAT, "version": 9, "doc": doc, "evals": {}});
    assert!(matches!(EvalFile::parse(&serde_json::to_vec(&newer).unwrap()), Parsed::Newer { version: 9, file: Some(_) }));
}

/// At most [`MAX_EVALS`] positions: the oldest kept goes first.
#[test]
fn past_the_cap_the_oldest_goes() {
    let mut f = EvalFile::empty(DocRecord::new(&keys(), 1));
    for i in 0..MAX_EVALS {
        f.evals.insert(format!("k{i:05}"), eval(1, 0, 1_000 + i as u64));
    }
    assert!(f.keep(START, eval(5, 10, 999_999)));
    assert_eq!(f.evals.len(), MAX_EVALS);
    assert!(!f.evals.contains_key("k00000"), "the oldest went");
    assert!(f.get(START).is_some());
}

/// Two copies merge by the deeper evaluation; the store keeps the document's under
/// `<hash>.evals.json`, and nothing is written when nothing would be kept.
#[test]
fn the_store_keeps_them_by_document_and_merges() {
    let mut mine = EvalFile::empty(DocRecord::new(&keys(), 1));
    let mut theirs = mine.clone();
    mine.keep(START, eval(10, 10, 1));
    theirs.keep(START, eval(15, 20, 1));
    theirs.keep("8/8/8/8/8/8/8/K6k w - - 0 1", eval(30, 0, 1));
    mine.merge(&theirs);
    assert_eq!(mine.evals.len(), 2);
    assert_eq!(mine.get(START).map(|e| e.depth), Some(15));

    let dir = temp_dir("store");
    let store = DirStore::new(&dir);
    assert_eq!(store.keep(&keys(), 1, &[]).unwrap(), 0);
    assert!(!dir.exists(), "nothing kept, no folder made");
    assert_eq!(store.keep(&keys(), 1, &[(START.to_owned(), eval(12, 33, 9))]).unwrap(), 1);
    assert_eq!(store.keep(&keys(), 1, &[(START.to_owned(), eval(8, 1, 10))]).unwrap(), 0, "shallower: not written");
    assert!(dir.join(file_name(&keys(), "evals").unwrap()).is_file());
    let loaded = store.load(&keys(), 1).unwrap();
    assert_eq!(loaded.file.get(START).map(|e| (e.depth, e.cp)), Some((12, Some(33))));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Plan A9-N17a: an exercise's second line is kept with the evaluation, by addition: it reads back,
/// a bad one makes the evaluation unreadable, and a build before it (whose evaluation has no such
/// field) keeps it among the fields it does not know.
#[test]
fn a_second_line_is_kept_by_addition() {
    let second = SecondLine::new(Score::Mate(3), &["a1a7".to_owned()]).unwrap();
    let mut file = EvalFile::empty(DocRecord::default());
    assert!(file.keep(START, eval(24, 900, 5).with_second(Some(second.clone()))));
    let bytes = file.to_bytes().unwrap();
    let Parsed::File { file: back, skipped: 0 } = EvalFile::parse(&bytes) else { panic!("{}", String::from_utf8_lossy(&bytes)) };
    assert_eq!(back.get(START).and_then(|e| e.second.clone()), Some(second));
    assert!(SecondLine::new(Score::Centipawns(1), &["no".to_owned()]).is_none());
    // A second line that is not one: the evaluation is left out, as any unreadable one.
    let mut value: Value = serde_json::from_slice(&bytes).unwrap();
    let key = position_key(START).unwrap();
    value["evals"][&key]["second"] = serde_json::json!({ "cp": 5, "pv": ["nonsense"] });
    let Parsed::File { file: bad, skipped } = EvalFile::parse(&serde_json::to_vec(&value).unwrap()) else { panic!() };
    assert_eq!((bad.evals.len(), skipped), (0, 1));
    // A build before it: its evaluation had no `second`, and keeps it as a field it does not know.
    #[derive(serde::Deserialize, serde::Serialize)]
    struct Before {
        depth: u32,
        #[serde(flatten)]
        other: Map<String, Value>,
    }
    let entry = serde_json::to_value(back.get(START).unwrap()).unwrap();
    let before: Before = serde_json::from_value(entry.clone()).unwrap();
    assert_eq!(serde_json::to_value(&before).unwrap(), entry, "written back as it was");
}
