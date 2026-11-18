//! The Library on synthetic books (plan A9-N15a): folders of PDFs built here, read without a piece
//! model (diagram fonts are read exactly), each rule of the walk and of the build with a test of
//! its own.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::index::{IndexedPosition, Place};
use super::walk::{Cap, Caps, walk};
use super::*;
use crate::chess::tests::{book_of_boards, no_model};

/// The start position in the Chess Merida layout, top rank first.
const MERIDA_START: [&str; 8] = ["tMvWlVmT", "OoOoOoOo", " + + + +", "+ + + + ", " + + + +", "+ + + + ", "pPpPpPpP", "RnBqKbNr"];
const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

/// A fresh folder of the test's own.
fn fresh(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcraft-library-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A PDF of 600 × 800 pt pages, page `i` drawing `pages[i]`, with `F1` a Merida diagram font and
/// `F2` Helvetica.
fn two_font_book(pages: &[String]) -> Vec<u8> {
    let n = pages.len();
    let widths = vec!["1000"; 95].join(" ");
    let (merida, helvetica) = (3 + 2 * n, 4 + 2 * n);
    let kids: String = (0..n).map(|i| format!(" {} 0 R", 3 + i)).collect();
    let mut objs: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        format!("<< /Type /Pages /Kids [{kids} ] /Count {n} /MediaBox [0 0 600 800] >>").into_bytes(),
    ];
    for i in 0..n {
        objs.push(
            format!("<< /Type /Page /Parent 2 0 R /Contents {} 0 R /Resources << /Font << /F1 {merida} 0 R /F2 {helvetica} 0 R >> >> >>", 3 + n + i)
                .into_bytes(),
        );
    }
    for c in pages {
        objs.push(format!("<< /Length {} >>\nstream\n{c}\nendstream", c.len()).into_bytes());
    }
    objs.push(format!("<< /Type /Font /Subtype /Type1 /BaseFont /ChessMerida /FirstChar 32 /LastChar 126 /Widths [{widths}] >>").into_bytes());
    objs.push(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_vec());
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(o);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
    out
}

/// A Helvetica line at `size` whose baseline is `top` points from the page's top.
fn helv(top: f32, size: f32, text: &str) -> String {
    format!("BT /F2 {size} Tf 60 {} Td ({text}) Tj ET\n", 800.0 - top)
}

/// A Merida board with its top-left corner `top` points from the page's top.
fn merida(top: f32, lines: [&str; 8]) -> String {
    let mut s = format!("BT /F1 20 Tf 60 {} Td ", 800.0 - top - 16.0);
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            s.push_str("0 -20 Td ");
        }
        s.push_str(&format!("({line}) Tj "));
    }
    s.push_str("ET\n");
    s
}

/// A chapter: a heading, the start position and the Spanish opening played from it.
fn chapter(title: &str) -> Vec<u8> {
    let mut page = helv(40.0, 24.0, title);
    page.push_str(&helv(70.0, 11.0, "White to play. The Spanish opening."));
    page.push_str(&merida(90.0, MERIDA_START));
    page.push_str(&helv(280.0, 11.0, "1.e4 e5 2.Nf3 Nc6 3.Bb5 a6"));
    two_font_book(&[page])
}

/// Options reading without a piece model, on one worker (a stop lands where it is asked).
fn options() -> BuildOptions {
    BuildOptions { models_dir: no_model().models_dir, workers: 1, ..BuildOptions::default() }
}

fn build(library: &Library, dirs: &[PathBuf], opts: &BuildOptions) -> Built {
    library.build(dirs, opts, &mut |_| true).unwrap()
}

/// The book whose paths include `name`.
fn book<'a>(built: &'a Built, name: &str) -> &'a BookReport {
    built.books.iter().find(|b| b.book.paths.iter().any(|p| p.ends_with(name))).unwrap_or_else(|| panic!("{name} in {:#?}", built.books))
}

/// The positions of book `hash`'s index.
fn positions(library: &Library, hash: &str) -> (index::IndexMeta, Vec<IndexedPosition>) {
    let mut found = Vec::new();
    let meta = index::read(&library.index_path(hash), None, &mut |p| found.push(*p)).unwrap();
    (meta, found)
}

/// The SAN of a record, by the index's table.
fn san(meta: &index::IndexMeta, p: &IndexedPosition) -> Option<String> {
    p.san.and_then(|s| meta.sans.get(usize::try_from(s).ok()?).cloned())
}

/// Plan A9-N15a's first check: three PDFs (a font diagram, moves in the text, a copy of the
/// first under another name) are two books, the copy's path in the first; each is indexed with
/// its diagrams and its moves.
#[test]
fn three_pdfs_are_two_books_and_the_copy_is_a_second_path() {
    let base = fresh("three");
    let (books, lib) = (base.join("books"), Library::new(base.join("library")));
    std::fs::create_dir_all(&books).unwrap();
    std::fs::write(books.join("a.pdf"), book_of_boards(2)).unwrap();
    std::fs::write(books.join("b.pdf"), chapter("Chapter 1")).unwrap();
    std::fs::write(books.join("c.pdf"), book_of_boards(2)).unwrap();
    let built = build(&lib, std::slice::from_ref(&books), &options());
    let hashed: Vec<&BookReport> = built.books.iter().filter(|b| b.book.hash.is_some()).collect();
    assert_eq!(hashed.len(), 2, "{:#?}", built.books);
    let a = book(&built, "a.pdf");
    assert_eq!(a.book.paths.len(), 2);
    assert!(a.book.paths[1].ends_with("c.pdf"), "{:?}", a.book.paths);
    assert!(hashed.iter().all(|b| b.book.state == State::Indexed), "{:#?}", built.books);
    assert_eq!(built.read, 2);
    // The boards: each page's font diagram read (the start position), the others not without a model.
    let (meta, found) = positions(&lib, a.book.hash.as_deref().unwrap());
    assert_eq!((meta.counts.diagrams, meta.counts.read), (6, 2), "{:?}", meta.counts);
    let start = pdfcraft_chess::fen::Fen::parse(START).unwrap().book_key();
    assert_eq!(found.iter().filter(|p| p.key().matches(start, p.side_assumed) && matches!(p.place, Place::Diagram { .. })).count(), 2);
    assert!(a.book.no_piece_model);
    // The chapter: its diagram and the six moves played from it, each numbered with its SAN.
    let b = book(&built, "b.pdf");
    let (meta, found) = positions(&lib, b.book.hash.as_deref().unwrap());
    let moves: Vec<String> = found
        .iter()
        .filter(|p| matches!(p.place, Place::Move { .. }))
        .map(|p| format!("{}{}{}", p.number, if p.black { "..." } else { "." }, san(&meta, p).unwrap_or_default()))
        .collect();
    assert_eq!(moves, ["1.e4", "1...e5", "2.Nf3", "2...Nc6", "3.Bb5", "3...a6"]);
    assert_eq!((meta.title.as_str(), meta.counts.games, meta.pages), ("b", 1, 1));
    assert!(found.iter().all(|p| p.page == Some(0)));
    // The catalog says it all again.
    let catalog = lib.catalog();
    assert_eq!(catalog.books, built.books.iter().map(|b| b.book.clone()).collect::<Vec<_>>());
    assert_eq!(catalog.dirs, [books.to_string_lossy().into_owned()]);
    let _ = std::fs::remove_dir_all(&base);
}

/// A folder with `a.pdf` and a link to the folder above it, `up`: the walk takes the PDF once and
/// says the link, never following it (followed, it would go round to the depth cap).
fn the_walk_does_not_follow(base: &Path, link: &Path) {
    let walked = walk(&[base.to_path_buf()], Caps::default(), &|| false);
    assert_eq!(walked.pdfs.iter().map(|p| p.path.clone()).collect::<Vec<_>>(), [base.join("books").join("a.pdf")], "{walked:#?}");
    assert_eq!(walked.links, [link.to_path_buf()]);
    assert!(walked.capped.is_empty(), "{:?}", walked.capped);
}

/// Plan A9-N15a: a junction (`mklink /J`, which needs no right) and a symbolic link to the folder
/// above are never followed; a symbolic link is made where the system lets this user make one,
/// and the test says when it could not.
#[test]
fn a_junction_or_a_link_to_the_folder_above_is_not_followed() {
    let base = fresh("links");
    std::fs::create_dir_all(base.join("books")).unwrap();
    std::fs::write(base.join("books").join("a.pdf"), b"%PDF-1.7 not opened").unwrap();
    let mut made = 0;
    #[cfg(windows)]
    {
        let junction = base.join("books").join("up");
        let out = std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(&junction).arg(&base).output().unwrap();
        assert!(out.status.success(), "mklink /J: {}", String::from_utf8_lossy(&out.stderr));
        the_walk_does_not_follow(&base, &junction);
        std::fs::remove_dir(&junction).unwrap();
        made += 1;
        let link = base.join("books").join("up-link");
        match std::os::windows::fs::symlink_dir(&base, &link) {
            Ok(()) => {
                the_walk_does_not_follow(&base, &link);
                std::fs::remove_dir(&link).unwrap();
                made += 1;
            }
            Err(e) => eprintln!("no symbolic link made ({e}): this user can't make one without Developer Mode; the junction was checked"),
        }
    }
    #[cfg(unix)]
    {
        let link = base.join("books").join("up");
        std::os::unix::fs::symlink(&base, &link).unwrap();
        the_walk_does_not_follow(&base, &link);
        made += 1;
    }
    assert!(made > 0, "no link could be made: nothing was checked");
    let _ = std::fs::remove_dir_all(&base);
}

/// Plan A9-N15a: a PDF marked offline (`attrib +O`, the attribute a OneDrive file not downloaded
/// carries with the recall ones) is said to be in the cloud, never opened: it is no book, and the
/// catalog lists it so.
#[cfg(windows)]
#[test]
fn an_offline_pdf_is_in_the_cloud_and_never_opened() {
    let base = fresh("offline");
    let (books, lib) = (base.join("books"), Library::new(base.join("library")));
    std::fs::create_dir_all(&books).unwrap();
    std::fs::write(books.join("here.pdf"), chapter("Here")).unwrap();
    let away = books.join("away.pdf");
    std::fs::write(&away, chapter("Away")).unwrap();
    let out = std::process::Command::new("attrib").arg("+O").arg(&away).output().unwrap();
    assert!(out.status.success(), "attrib +O: {}", String::from_utf8_lossy(&out.stderr));
    let walked = walk(std::slice::from_ref(&books), Caps::default(), &|| false);
    assert_eq!(walked.pdfs.iter().map(|p| p.path.clone()).collect::<Vec<_>>(), [books.join("here.pdf")]);
    assert_eq!(walked.cloud, [away.clone()]);
    let built = build(&lib, std::slice::from_ref(&books), &options());
    assert_eq!(book(&built, "away.pdf").book.state, State::Cloud);
    assert_eq!(book(&built, "away.pdf").book.hash, None, "never read");
    assert_eq!((built.read, built.cloud), (1, 1));
    let _ = std::process::Command::new("attrib").arg("-O").arg(&away).output();
    let _ = std::fs::remove_dir_all(&base);
}

/// Plan A9-N15a: twenty folders one in the other stop at the depth cap, which is said; the PDFs
/// above it are taken.
#[test]
fn twenty_nested_folders_stop_at_the_depth_cap() {
    let base = fresh("deep");
    let mut dir = base.clone();
    for level in 1..=20 {
        dir = dir.join(format!("d{level}"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("at{level}.pdf")), b"%PDF").unwrap();
    }
    let walked = walk(std::slice::from_ref(&base), Caps::default(), &|| false);
    let names: Vec<String> = walked.pdfs.iter().map(|p| p.path.file_name().unwrap().to_string_lossy().into_owned()).collect();
    assert_eq!(names, (1..=walk::MAX_DEPTH).map(|l| format!("at{l}.pdf")).collect::<Vec<_>>());
    assert_eq!(walked.capped, [Cap::Depth]);
    let _ = std::fs::remove_dir_all(&base);
}

/// The entries' and the books' caps stop the walk where they say, and are said.
#[test]
fn the_entries_and_books_caps_are_said() {
    let base = fresh("caps");
    for i in 0..6 {
        std::fs::write(base.join(format!("{i}.pdf")), b"%PDF").unwrap();
    }
    let walked = walk(std::slice::from_ref(&base), Caps { books: 4, ..Caps::default() }, &|| false);
    assert_eq!((walked.pdfs.len(), walked.capped.as_slice()), (4, [Cap::Books].as_slice()));
    let walked = walk(std::slice::from_ref(&base), Caps { entries: 3, ..Caps::default() }, &|| false);
    assert_eq!((walked.pdfs.len(), walked.entries, walked.capped.as_slice()), (3, 3, [Cap::Entries].as_slice()));
    let _ = std::fs::remove_dir_all(&base);
}

/// Plan A9-N15a: a build stopped after the first book, in the second's scan, is gone on with: the
/// first is not read again (no page scanned), and the second goes on from the pages it kept; once
/// indexed, its pages in progress are taken out.
#[test]
fn a_stopped_build_goes_on_from_the_pages_on_disk() {
    let base = fresh("resume");
    let (books, lib) = (base.join("books"), Library::new(base.join("library")));
    std::fs::create_dir_all(&books).unwrap();
    // Four pages that draw differently (`book_of_boards` places its boards by page modulo 4).
    std::fs::write(books.join("1.pdf"), book_of_boards(3)).unwrap();
    std::fs::write(books.join("2.pdf"), book_of_boards(4)).unwrap();
    let opts = options();
    // Stopped once two pages of the second book are scanned.
    let built = lib.build(std::slice::from_ref(&books), &opts, &mut |p| !(p.book == 1 && p.page >= 2)).unwrap();
    assert!(built.stopped);
    assert_eq!(book(&built, "1.pdf").book.state, State::Indexed);
    let second = book(&built, "2.pdf");
    assert_eq!(second.book.state, State::Partial);
    assert_eq!(second.run.map(|r| (r.scanned, r.kept)), Some((2, 2)));
    assert_eq!(built.left, 1);
    assert_eq!(lib.catalog().books.iter().map(|b| b.state).collect::<Vec<_>>(), [State::Indexed, State::Partial]);
    // Run again: the first is current, nothing of it scanned; the second goes on.
    let again = build(&lib, std::slice::from_ref(&books), &opts);
    let first = book(&again, "1.pdf").run.unwrap();
    assert!(first.current);
    assert_eq!(first.scanned, 0, "the first book is not read again");
    let second = book(&again, "2.pdf").run.unwrap();
    assert_eq!((second.from_library, second.scanned), (2, 2), "{second:?}");
    assert_eq!(book(&again, "2.pdf").book.state, State::Indexed);
    assert!(!again.stopped && again.left == 0);
    // Its pages in progress are taken out once its index is written.
    let pages = reading::ReadingStore::new(lib.pages_dir()).status();
    assert!(pages.iter().all(|b| b.entries == 0), "{pages:?}");
    // A third run reads nothing.
    let third = build(&lib, std::slice::from_ref(&books), &opts);
    assert!(third.books.iter().all(|b| b.run.is_some_and(|r| r.current && r.scanned == 0)));
    assert_eq!(third.read, 0);
    let _ = std::fs::remove_dir_all(&base);
}

/// `max_books` reads that many books a call, and the next call goes on with the rest; a book
/// already indexed is not one of them.
#[test]
fn max_books_goes_in_parts() {
    let base = fresh("parts");
    let (books, lib) = (base.join("books"), Library::new(base.join("library")));
    std::fs::create_dir_all(&books).unwrap();
    for (i, n) in [2, 3, 4].into_iter().enumerate() {
        std::fs::write(books.join(format!("{i}.pdf")), book_of_boards(n)).unwrap();
    }
    let opts = BuildOptions { max_books: Some(1), ..options() };
    let states = |b: &Built| b.books.iter().map(|b| b.book.state).collect::<Vec<_>>();
    let first = build(&lib, std::slice::from_ref(&books), &opts);
    assert_eq!((first.read, first.left, states(&first)), (1, 2, vec![State::Indexed, State::Pending, State::Pending]));
    let second = build(&lib, std::slice::from_ref(&books), &opts);
    assert_eq!((second.read, second.left, states(&second)), (1, 1, vec![State::Indexed, State::Indexed, State::Pending]));
    let third = build(&lib, std::slice::from_ref(&books), &opts);
    assert_eq!((third.read, third.left), (1, 0));
    assert!(third.books.iter().all(|b| b.book.state == State::Indexed));
    let _ = std::fs::remove_dir_all(&base);
}

/// The key names everything a book's index depends on: another piece model, glyph model, general
/// OCR, corrections or PGN, another content or other settings each make another key.
#[test]
fn the_key_changes_with_each_thing_the_index_depends_on() {
    let (a, b) = ([1u8; 32], [2u8; 32]);
    let settings = ChessScanSettings::default();
    let base = index_key(&a, &settings, &ModelDigests::default());
    let others = [
        index_key(&b, &settings, &ModelDigests::default()),
        index_key(&a, &ChessScanSettings { dpi: 150.0, ..settings.clone() }, &ModelDigests::default()),
        index_key(&a, &settings, &ModelDigests { pieces: Some(&a), ..Default::default() }),
        index_key(&a, &settings, &ModelDigests { pieces: Some(&b), ..Default::default() }),
        index_key(&a, &settings, &ModelDigests { glyphs: Some(&a), ..Default::default() }),
        index_key(&a, &settings, &ModelDigests { general: Some(&a), ..Default::default() }),
        index_key(&a, &settings, &ModelDigests { corrections: Some(&a), ..Default::default() }),
        index_key(&a, &settings, &ModelDigests { pgn: Some("hash"), ..Default::default() }),
    ];
    for (i, k) in others.iter().enumerate() {
        assert_ne!(*k, base, "{i}");
        assert!(others.iter().enumerate().all(|(j, o)| i == j || o != k), "{i}");
    }
    // The model's folder is not in it: the same settings with another folder are the same key.
    let elsewhere = ChessScanSettings { models_dir: Some(PathBuf::from("elsewhere")), ..settings.clone() };
    assert_eq!(index_key(&a, &elsewhere, &ModelDigests::default()), base);
}

/// Plan A9-N15a: another piece model reads both books again. Needs the user's models
/// (`PDFCRAFT_CHESS_TEST_MODELS`, as the chess gate sets it); without them it says so.
#[test]
fn another_piece_model_reads_both_books_again() {
    let base = fresh("model");
    let (books, lib) = (base.join("books"), Library::new(base.join("library")));
    std::fs::create_dir_all(&books).unwrap();
    std::fs::write(books.join("1.pdf"), book_of_boards(2)).unwrap();
    std::fs::write(books.join("2.pdf"), chapter("Two")).unwrap();
    let first = build(&lib, std::slice::from_ref(&books), &options());
    assert_eq!(first.read, 2);
    let Some(models) = pdfcraft_chessvision::models::test_models().unwrap() else {
        eprintln!("skipped the real model: set PDFCRAFT_CHESS_TEST_MODELS to the folder of the chess models");
        let _ = std::fs::remove_dir_all(&base);
        return;
    };
    let with = BuildOptions { models_dir: Some(models), ..options() };
    assert!(super::super::models(with.models_dir.as_deref()).pieces.loaded().is_some(), "the piece model did not load");
    let second = build(&lib, std::slice::from_ref(&books), &with);
    assert_eq!(second.read, 2, "both books read again");
    assert!(second.books.iter().all(|b| b.run.is_some_and(|r| !r.current && r.scanned > 0) && !b.book.no_piece_model), "{:#?}", second.books);
    let third = build(&lib, std::slice::from_ref(&books), &with);
    assert_eq!(third.read, 0);
    let _ = std::fs::remove_dir_all(&base);
}

/// `bytes` encrypted with an empty user password and `permissions`.
fn encrypted(bytes: Vec<u8>, permissions: i32) -> Vec<u8> {
    let mut doc = pdfcraft_cos::Document::open(Arc::new(bytes)).unwrap();
    doc.set_encryption(&pdfcraft_cos::NewEncryption {
        algorithm: pdfcraft_cos::Algorithm::Aes256,
        user_password: "",
        owner_password: "owner",
        permissions,
        encrypt_metadata: true,
        seed: [7; 32],
    })
    .unwrap();
    pdfcraft_cos::write_full(&doc, &Default::default()).unwrap()
}

/// Plan A9-N15a: a book that does not allow copying is skipped and said (`never`); an encrypted
/// one too (`ask`), until the person says yes to it (decision D31 A); a file that does not open is
/// skipped with its error.
#[test]
fn a_book_never_kept_or_not_yet_allowed_is_skipped_and_said() {
    let base = fresh("never");
    let (books, lib) = (base.join("books"), Library::new(base.join("library")));
    std::fs::create_dir_all(&books).unwrap();
    std::fs::write(books.join("ask.pdf"), encrypted(book_of_boards(2), -1)).unwrap();
    std::fs::write(books.join("never.pdf"), encrypted(book_of_boards(3), -1 & !0x10)).unwrap();
    std::fs::write(books.join("broken.pdf"), b"not a PDF at all").unwrap();
    let built = build(&lib, std::slice::from_ref(&books), &options());
    let said = |name: &str| {
        let b = &book(&built, name).book;
        (b.state, b.reason)
    };
    assert_eq!(said("never.pdf"), (State::Skipped, Some(Skip::Never)));
    assert_eq!(said("ask.pdf"), (State::Skipped, Some(Skip::Ask)));
    assert_eq!(said("broken.pdf"), (State::Skipped, Some(Skip::DoesNotOpen)));
    assert!(book(&built, "broken.pdf").book.error.is_some());
    assert_eq!((built.read, lib.indexes().0.len()), (0, 0), "nothing read, no index");
    // Yes to the encrypted one: it is read and indexed; the other is never.
    let hash = book(&built, "ask.pdf").book.hash.clone().unwrap();
    let yes = build(&lib, std::slice::from_ref(&books), &BuildOptions { allowed: vec![hash], ..options() });
    assert_eq!(book(&yes, "ask.pdf").book.state, State::Indexed);
    assert_eq!(book(&yes, "never.pdf").book.reason, Some(Skip::Never));
    let _ = std::fs::remove_dir_all(&base);
}

/// Plan A9-N15a: an index damaged on disk is set aside, never served, and made again.
#[test]
fn a_damaged_index_is_set_aside_and_made_again() {
    let base = fresh("damaged");
    let (books, lib) = (base.join("books"), Library::new(base.join("library")));
    std::fs::create_dir_all(&books).unwrap();
    std::fs::write(books.join("a.pdf"), chapter("A")).unwrap();
    let first = build(&lib, std::slice::from_ref(&books), &options());
    let hash = book(&first, "a.pdf").book.hash.clone().unwrap();
    let path = lib.index_path(&hash);
    let good = std::fs::read(&path).unwrap();
    let mut bad = good.clone();
    let at = bad.len() - 3;
    bad[at] ^= 0x5a;
    std::fs::write(&path, &bad).unwrap();
    let again = build(&lib, std::slice::from_ref(&books), &options());
    let run = book(&again, "a.pdf").run.unwrap();
    assert!(run.index_set_aside && !run.current, "{run:?}");
    assert_eq!(run.scanned, 1, "read again");
    assert_eq!(std::fs::read(lib.dir().join(format!("{hash}.lib.bad"))).unwrap(), bad, "set aside as it was");
    assert_eq!(std::fs::read(&path).unwrap(), good, "made again the same");
    assert_eq!(lib.indexes().1, 1);
    let _ = std::fs::remove_dir_all(&base);
}

/// Every file of a folder below `dir` with its bytes and its time.
fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>, std::time::SystemTime)> {
    let mut out = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let meta = e.metadata().unwrap();
            if meta.is_dir() {
                todo.push(e.path());
            } else {
                out.push((e.path(), std::fs::read(e.path()).unwrap(), meta.modified().unwrap()));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// The coordinator's decision 1 (2026-11-18): a book the app read first is not read again by the
/// Library, which takes its pages from the app's readings and leaves them byte for byte and time
/// for time as they were (decision D31 C).
#[test]
fn a_book_the_app_read_is_not_read_again_and_its_readings_are_left_as_they_were() {
    let base = fresh("app");
    let (books, lib, app) = (base.join("books"), Library::new(base.join("library")), base.join("app-reading"));
    std::fs::create_dir_all(&books).unwrap();
    let bytes = book_of_boards(4);
    let path = books.join("a.pdf");
    std::fs::write(&path, &bytes).unwrap();
    // The app reads it, keeping its pages in its own folder.
    let mut s = Session::new();
    let id = s.open("a.pdf", Some(path.to_string_lossy().into_owned()), Arc::new(bytes), None).unwrap();
    let kept = ChessScanSettings { cache: Some(ReadingCache { dir: app.clone(), encrypted: false }), ..no_model() };
    s.chess_scan(id, &[], &kept, true).unwrap();
    assert_eq!(s.chess_disk().kept, 4);
    let before = snapshot(&app);
    // Time enough for any write to show in the files' times.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let built = build(&lib, std::slice::from_ref(&books), &BuildOptions { app_reading: Some(app.clone()), ..options() });
    let run = book(&built, "a.pdf").run.unwrap();
    assert_eq!((run.scanned, run.from_app, run.from_library), (0, 4, 0), "{run:?}");
    assert_eq!(book(&built, "a.pdf").book.state, State::Indexed);
    assert_eq!(snapshot(&app), before, "the app's readings as they were");
    // Without the app's folder, the same book is read.
    let other = Library::new(base.join("other"));
    let alone = build(&other, std::slice::from_ref(&books), &options());
    assert_eq!(book(&alone, "a.pdf").run.map(|r| r.scanned), Some(4));
    let _ = std::fs::remove_dir_all(&base);
}

/// Clearing one book takes out its index and leaves it pending; clearing all takes out only what
/// the Library made.
#[test]
fn clearing_takes_out_only_what_the_library_made() {
    let base = fresh("clear");
    let (books, lib) = (base.join("books"), Library::new(base.join("library")));
    std::fs::create_dir_all(&books).unwrap();
    std::fs::write(books.join("a.pdf"), chapter("A")).unwrap();
    std::fs::write(books.join("b.pdf"), chapter("B")).unwrap();
    let built = build(&lib, std::slice::from_ref(&books), &options());
    let a = book(&built, "a.pdf").book.hash.clone().unwrap();
    std::fs::write(lib.dir().join("mine.txt"), b"the person's").unwrap();
    let one = lib.clear(Some(&a));
    assert_eq!(one.indexes, 1);
    assert_eq!(lib.indexes().0.len(), 1);
    assert_eq!(lib.catalog().books.iter().find(|b| b.hash.as_deref() == Some(a.as_str())).map(|b| b.state), Some(State::Pending));
    assert_eq!(build(&lib, std::slice::from_ref(&books), &options()).read, 1, "the cleared one is read again");
    let all = lib.clear(None);
    assert_eq!(all.indexes, 2);
    assert!(lib.indexes().0.is_empty() && !lib.dir().join("catalog.json").exists());
    assert!(lib.dir().join("mine.txt").exists(), "another file stays");
    let _ = std::fs::remove_dir_all(&base);
}
