//! A book's cipher kept per document (plan A9-G3c, V4b): kept and read back, two writers merged by
//! page, a newer version only read, a file that can't be read set aside by a change, fields of a
//! later version kept, and the format of version 1 frozen.

use pdfcraft_chess::placement::PieceKind;
use pdfcraft_chess::repair::{CipherMap, Observation, Proof};

use super::*;
use crate::chess::DocKeys;
use crate::chess::store::{MemBackend, file_name};

fn keys() -> DocKeys {
    DocKeys { id0: Some([5; 16]), ..Default::default() }
}

/// A proof of `n` uses of `symbol` as a queen.
fn queen_proof(symbol: &str, n: usize) -> Proof {
    let observations = (0..n)
        .map(|i| Observation { symbol: symbol.into(), piece: PieceKind::Queen, raw: format!("{symbol}d{}", i + 1), san: format!("Qd{}", i + 1) })
        .collect();
    Proof::Proven { assignment: CipherMap::from([(symbol.into(), PieceKind::Queen)]), observations, plain_plies: 0, plies: n, complete: true }
}

/// A table with `n` proofs of `W` on `page`.
fn table(page: u32, n: usize) -> BookCipher {
    let mut c = BookCipher::default();
    c.record(&queen_proof("W", n), page);
    c
}

fn store() -> DocStore<Cipher, MemBackend> {
    DocStore::on(MemBackend::new(1 << 20))
}

#[test]
fn a_kept_cipher_is_read_back_and_two_writers_merge_by_page() {
    let s = store();
    assert!(s.load(&keys(), 100).unwrap().file.cipher.is_empty(), "nothing kept yet");
    let kept = s.keep(&keys(), 100, &table(12, 3)).unwrap();
    assert_eq!(kept, table(12, 3));
    let name = file_name(&keys(), "cipher").unwrap();
    assert!(name.ends_with(".cipher.json"));
    assert_eq!(s.load(&keys(), 100).unwrap().file.cipher, table(12, 3));
    // Another writer read page 13: both pages are kept, and the symbol is settled by their six.
    let kept = s.keep(&keys(), 100, &table(13, 3)).unwrap();
    assert_eq!(kept.proven(), CipherMap::from([("W".into(), PieceKind::Queen)]));
    assert_eq!(s.load(&keys(), 100).unwrap().file.cipher, kept);
    // Page 12 read again with fewer proofs: the writer's reading of it wins, page 13 stays.
    let kept = s.keep(&keys(), 100, &table(12, 1)).unwrap();
    assert_eq!(kept.entries()["W"].evidence[&pdfcraft_chessvision::cipher::Officer::Q].legality, 4);
}

/// The file of version 1, frozen: a later build reads it, or raises the version.
const V1: &str = r#"{"format":"pdfcraft-chess-cipher","version":1,"doc":{"id0":"05050505050505050505050505050505","path_hash":null,"name":null,"size":null,"pages":100},"cipher":{"entries":{"W":{"evidence":{"Q":{"legality":6,"glyph":0}},"examples":[{"page":12,"raw":"Wd1","san":"Qd1"}]}},"pages":{"12":[["W","Q"],["W","Q"],["W","Q"]],"13":[["W","Q"],["W","Q"],["W","Q"]]}},"later":{"kept":true}}"#;

#[test]
fn a_file_of_version_1_reads_and_keeps_what_a_later_version_adds() {
    let Parsed::File { file, skipped: 0 } = CipherFile::parse(V1.as_bytes()) else { panic!("version 1 reads") };
    assert_eq!(file.cipher.proven(), CipherMap::from([("W".into(), PieceKind::Queen)]));
    assert_eq!(file.cipher.entries()["W"].evidence[&pdfcraft_chessvision::cipher::Officer::Q].legality, 6, "the pages' proofs counted once");
    assert_eq!(file.other.get("later"), Some(&serde_json::json!({"kept": true})));
    // Written again, it is the same file.
    let again: serde_json::Value = serde_json::from_slice(&file.to_bytes().unwrap()).unwrap();
    assert_eq!(again, serde_json::from_str::<serde_json::Value>(V1).unwrap());
}

/// A file of a newer version is read for what this build makes of it, never written; one that
/// can't be read is said by a load, set aside by a change, and the change kept.
#[test]
fn a_newer_file_is_only_read_and_a_damaged_one_set_aside_by_a_change() {
    let name = file_name(&keys(), "cipher").unwrap();
    let newer = V1.replace(r#""version":1"#, r#""version":2"#);
    let s = DocStore::<Cipher, MemBackend>::on(MemBackend::with_files([(name.clone(), newer.into_bytes())], 1 << 20));
    let loaded = s.load(&keys(), 100).unwrap();
    assert!(loaded.read_only && !loaded.file.cipher.is_empty(), "{loaded:?}");
    assert!(matches!(s.keep(&keys(), 100, &table(14, 3)), Err(CipherStoreError::Newer(2))));
    let s = DocStore::<Cipher, MemBackend>::on(MemBackend::with_files([(name.clone(), b"{\"format\":\"pdfcraft-chess-cipher\",".to_vec())], 1 << 20));
    assert!(s.load(&keys(), 100).unwrap().unreadable.is_some());
    s.keep(&keys(), 100, &table(14, 3)).unwrap();
    let files = s.files();
    assert!(files.iter().any(|(n, _)| n.ends_with(".bad")), "{:?}", files.iter().map(|f| &f.0).collect::<Vec<_>>());
    assert_eq!(s.load(&keys(), 100).unwrap().file.cipher, table(14, 3));
    // Another kind's file, or a table past its caps, is not a cipher.
    for bad in [
        r#"{"format":"pdfcraft-chess-reference","version":1}"#.to_string(),
        format!(r#"{{"format":"pdfcraft-chess-cipher","version":1,"doc":{{}},"cipher":"{}"}}"#, "x".repeat(10)),
    ] {
        assert!(matches!(CipherFile::parse(bad.as_bytes()), Parsed::Bad(_)), "{bad}");
    }
}
