//! A book's reading kept on disk, in use (plan A9-N5b): a book opened again in another session
//! reads none of its pages; other settings or another model read them all again; an entry that
//! can't be trusted is read again, never served; an encrypted document keeps nothing before the
//! user says so; the app's job takes its pages from the disk too; and chess OCR of a scanned page
//! is not run again.

use std::cell::Cell;

use pdfcraft_chessvision::glyphs::GlyphClassifier;
use pdfcraft_chessvision::models::ModelError;

use super::text_ocr::{Templates, scan_image};
use super::*;
use crate::chess::document::{OcrMode, OcrOptions};

/// A folder of its own for each test.
fn cache_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcraft-kept-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// [`no_model`] keeping the pages' readings in `dir`.
fn kept(dir: &Path) -> ChessScanSettings {
    ChessScanSettings { cache: Some(ReadingCache { dir: dir.to_path_buf(), encrypted: false }), ..no_model() }
}

/// The entries kept in `dir`, by path.
fn entries(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for book in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        for e in std::fs::read_dir(book.path()).into_iter().flatten().flatten() {
            if e.path().extension().is_some_and(|x| x == "page") {
                out.push(e.path());
            }
        }
    }
    out.sort();
    out
}

#[test]
fn a_book_opened_again_reads_none_of_its_pages() {
    let dir = cache_dir("again");
    // Four pages that draw differently (`book_of_boards` places its boards by page modulo 4).
    let bytes = book_of_boards(4);
    let (mut s, id) = open(bytes.clone());
    let first = s.chess_scan(id, &[], &kept(&dir), true).unwrap();
    assert_eq!(s.chess_scanned(), 4);
    assert_eq!(s.chess_disk(), DiskUse { kept: 4, ..Default::default() });
    assert_eq!(entries(&dir).len(), 4);
    // Opened again: another session, as after closing the book (or the app) and opening it.
    let (mut again, other) = open(bytes.clone());
    let second = again.chess_scan(other, &[], &kept(&dir), true).unwrap();
    assert_eq!(again.chess_scanned(), 0, "no page read again");
    assert_eq!(again.chess_disk(), DiskUse { read: 4, ..Default::default() });
    let pages = |p: &[Arc<ChessPage>]| p.iter().map(|p| (**p).clone()).collect::<Vec<_>>();
    assert_eq!(pages(&second), pages(&first));
    // A page asked for alone comes from the disk with its own index.
    let (mut third, doc) = open(bytes.clone());
    let one = third.chess_scan(doc, &[3], &kept(&dir), true).unwrap();
    assert_eq!((third.chess_scanned(), one[0].page), (0, 3));
    // Without the folder, the session reads them all.
    let (mut plain, doc) = open(bytes);
    plain.chess_scan(doc, &[], &no_model(), true).unwrap();
    assert_eq!(plain.chess_scanned(), 4);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Other settings read every page again, and so does another model: with the user's models (the
/// chess gate), a book kept with no model is read again with them, once.
#[test]
fn other_settings_or_another_model_read_every_page_again() {
    let dir = cache_dir("model");
    let bytes = book_of_boards(3);
    let scanned = |settings: &ChessScanSettings, read: bool| {
        let (mut s, id) = open(bytes.clone());
        s.chess_scan(id, &[], settings, read).unwrap();
        s.chess_scanned()
    };
    assert_eq!(scanned(&kept(&dir), true), 3);
    assert_eq!(scanned(&kept(&dir), true), 0);
    assert_eq!(scanned(&ChessScanSettings { dpi: 150.0, ..kept(&dir) }, true), 3, "another resolution");
    assert_eq!(scanned(&kept(&dir), false), 3, "finding only is kept apart from reading");
    assert_eq!(scanned(&kept(&dir), false), 0);
    let Some(models) = pdfcraft_chessvision::models::test_models().unwrap() else {
        eprintln!("skipped the real model: set PDFCRAFT_CHESS_TEST_MODELS to the folder of the chess models");
        let _ = std::fs::remove_dir_all(&dir);
        return;
    };
    let real = ChessScanSettings { models_dir: Some(models), ..kept(&dir) };
    assert!(super::super::models(real.models_dir.as_deref()).pieces.loaded().is_some(), "the piece model did not load");
    assert_eq!(scanned(&real, true), 3, "another model: every page read again");
    assert_eq!(scanned(&real, true), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

/// An entry damaged on disk is set aside and its page read again; the other pages come from the
/// disk; what is read is what a fresh reading reads.
#[test]
fn an_entry_that_cant_be_trusted_is_read_again_not_served() {
    let dir = cache_dir("damaged");
    let bytes = book_of_boards(4);
    let (mut s, id) = open(bytes.clone());
    let first = s.chess_scan(id, &[], &kept(&dir), true).unwrap();
    let damaged = entries(&dir)[1].clone();
    let mut b = std::fs::read(&damaged).unwrap();
    let at = b.len() - 2;
    b[at] ^= 0x33;
    std::fs::write(&damaged, b).unwrap();
    let (mut again, other) = open(bytes);
    let second = again.chess_scan(other, &[], &kept(&dir), true).unwrap();
    assert_eq!(again.chess_scanned(), 1, "the damaged page alone");
    assert_eq!(again.chess_disk(), DiskUse { read: 3, kept: 1, set_aside: 1, not_kept: 0 });
    assert_eq!(second.iter().map(|p| (**p).clone()).collect::<Vec<_>>(), first.iter().map(|p| (**p).clone()).collect::<Vec<_>>());
    let mut aside = damaged.into_os_string();
    aside.push(".bad");
    assert!(PathBuf::from(aside).exists(), "set aside");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `book_of_boards(2)` encrypted with an empty user password: it opens without asking, and allows
/// copying.
fn encrypted_book() -> Vec<u8> {
    let mut doc = pdfcraft_cos::Document::open(Arc::new(book_of_boards(2))).unwrap();
    doc.set_encryption(&pdfcraft_cos::NewEncryption {
        algorithm: pdfcraft_cos::Algorithm::Aes256,
        user_password: "",
        owner_password: "owner",
        permissions: -1,
        encrypt_metadata: true,
        seed: [7; 32],
    })
    .unwrap();
    pdfcraft_cos::write_full(&doc, &Default::default()).unwrap()
}

/// An encrypted document's reading is kept unencrypted, so not before the user says so (the
/// app's question, or a tool's `allow_encrypted`): until then nothing is written, and what is
/// kept is still taken.
#[test]
fn an_encrypted_document_keeps_nothing_before_the_user_says_yes() {
    let dir = cache_dir("encrypted");
    let bytes = encrypted_book();
    let (mut s, id) = open(bytes.clone());
    assert_eq!(s.get(id).map(crate::chess::store::persist_policy), Some(crate::chess::store::Persist::Ask));
    s.chess_scan(id, &[], &kept(&dir), true).unwrap();
    assert_eq!(s.chess_scanned(), 2);
    assert!(entries(&dir).is_empty() && !dir.exists(), "no reading file before the yes");
    assert_eq!(s.chess_disk().kept, 0);
    // Yes, in another session: its pages are kept.
    let (mut yes, doc) = open(bytes.clone());
    yes.set_chess_keep(doc, true);
    yes.chess_scan(doc, &[], &kept(&dir), true).unwrap();
    assert_eq!((yes.chess_scanned(), entries(&dir).len()), (2, 2));
    // Without the yes, what is kept is taken, and nothing more written.
    let (mut no, doc) = open(bytes.clone());
    no.chess_scan(doc, &[], &kept(&dir), true).unwrap();
    assert_eq!((no.chess_scanned(), no.chess_disk().read), (0, 2));
    // A tool's allow_encrypted says yes for its call.
    let other = cache_dir("encrypted-tool");
    let (mut tool, doc) = open(bytes);
    let allowed = ChessScanSettings { cache: Some(ReadingCache { dir: other.clone(), encrypted: true }), ..no_model() };
    tool.chess_scan(doc, &[], &allowed, true).unwrap();
    assert_eq!(entries(&other).len(), 2);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&other);
}

/// The app's scan (a job made by the session, run on a worker) takes its pages from the disk too,
/// and keeps what it reads.
#[test]
fn the_apps_job_takes_its_pages_from_the_disk() {
    let dir = cache_dir("job");
    let bytes = book_of_boards(3);
    let run = || {
        let (mut s, id) = open(bytes.clone());
        let settings = kept(&dir);
        let job = s.chess_scan_job(id, &[], &settings, true).unwrap();
        let scan = job.run(&models(settings.models_dir.as_deref()), |_, _| true);
        let disk = scan.disk;
        assert!(s.store_chess_scan(&scan));
        assert_eq!(s.chess_disk(), disk);
        (disk, scan.pages.iter().map(|p| (**p).clone()).collect::<Vec<_>>())
    };
    let (first, read) = run();
    assert_eq!(first, DiskUse { kept: 3, ..Default::default() });
    let (second, taken) = run();
    assert_eq!(second, DiskUse { read: 3, ..Default::default() });
    assert_eq!(taken, read);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The test's glyph classifier with a digest, counting the cells it is asked about.
struct Counted<'a> {
    inner: &'a Templates,
    calls: Cell<usize>,
}

impl GlyphClassifier for Counted<'_> {
    fn classes(&self) -> &[String] {
        self.inner.classes()
    }

    fn classify(&self, cells: &[f32], count: usize) -> Result<Vec<f32>, ModelError> {
        self.calls.set(self.calls.get() + count);
        self.inner.classify(cells, count)
    }

    fn digest(&self) -> Option<[u8; 32]> {
        Some([9; 32])
    }
}

/// Chess OCR of a scanned page is kept at its local stage (before the cipher and the repair), so
/// the page opened again is not read by the glyph classifier again, and reads the same.
#[test]
fn chess_ocr_of_a_scanned_page_is_not_run_again() {
    let dir = cache_dir("ocr");
    let templates = Templates::learn("1.e4526Nf3c", 14.0);
    let bytes = pdf("q 600 0 0 800 0 0 cm /Im1 Do Q\n", "Helvetica", Some(scan_image(&[("1.e4 e5 2.Nf3 Nc6", 14.0, 300.0)])), "");
    let read = || {
        let glyphs = Counted { inner: &templates, calls: Cell::new(0) };
        let (mut s, id) = open(bytes.clone());
        let options = OcrOptions { mode: OcrMode::Auto, glyphs: Some(&glyphs), general: false, book: None };
        let doc = s.chess_document_with(id, &[], &kept(&dir), Some(pdfcraft_chess::san::Lang::En), &options).unwrap();
        (glyphs.calls.get(), s.chess_disk(), doc.ocr_pages.clone(), format!("{:?}", doc.doc.pages))
    };
    let (calls, disk, pages, text) = read();
    assert!(calls > 0 && pages.len() == 1, "{calls} {pages:?}");
    assert_eq!(disk.kept, 2, "the page's scan and its chess OCR: {disk:?}");
    let (calls, disk, again, same) = read();
    assert_eq!(calls, 0, "no glyph read again");
    assert_eq!(disk, DiskUse { read: 2, ..Default::default() });
    assert_eq!((again, same), (pages, text));
    // A classifier that can't say what it is keeps nothing.
    let other = cache_dir("ocr-plain");
    let (mut s, id) = open(bytes.clone());
    let options = OcrOptions { mode: OcrMode::Auto, glyphs: Some(&templates), general: false, book: None };
    s.chess_document_with(id, &[], &kept(&other), Some(pdfcraft_chess::san::Lang::En), &options).unwrap();
    assert_eq!(s.chess_disk().kept, 1, "the scan alone");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&other);
}

/// A job's session (the app's Text tab reads in one, opened from the document's bytes under its
/// name, with no path) keeps a book's pages in the folder the app's own session keeps them in.
#[test]
fn a_jobs_session_keeps_the_book_in_the_books_own_folder() {
    let bytes = Arc::new(book_of_boards(2));
    let mut app = Session::new();
    let id = app.open("book.pdf", Some("C:/books/book.pdf".into()), bytes.clone(), None).unwrap();
    let mut job = Session::new();
    let in_job = job.open("book.pdf", None, bytes, None).unwrap();
    let folder = |s: &Session, id: DocId| reading::book_folder(&s.get(id).unwrap().doc_keys());
    assert!(folder(&app, id).is_some());
    assert_eq!(folder(&app, id), folder(&job, in_job));
}
