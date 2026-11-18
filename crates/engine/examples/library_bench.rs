//! The Library built over a folder of books (plan A9-N15a), measured: every book read and indexed
//! as `chess_library_build` does, with the time each took, what its index holds and its size.
//!
//! ```text
//! cargo run --release -p pdfcraft-engine --example library_bench -- <books folder> <library folder> [--models DIR] [--workers N] [--max-books N] [--app-reading DIR]
//! ```
//!
//! `--models` names the folder of the chess models (default: none, diagrams in pictures unread);
//! `--workers` the scan's cores (default the cores less one, at most 4). The app's readings are
//! not looked in unless `--app-reading` names them, so every page is read. The books stay where
//! they are, outside the repository; the Library goes to the folder given.

use std::path::PathBuf;
use std::time::Instant;

use pdfcraft_engine::chess::ChessScanJob;
use pdfcraft_engine::chess::library::{BuildOptions, Library};

fn main() {
    if let Err(e) = run() {
        eprintln!("library_bench: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let usage = "usage: library_bench <books folder> <library folder> [--models DIR] [--workers N] [--max-books N] [--app-reading DIR]";
    let mut opts = BuildOptions { workers: ChessScanJob::default_workers(), ..BuildOptions::default() };
    let mut plain = Vec::new();
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--models" => opts.models_dir = Some(it.next().ok_or(usage)?.into()),
            "--workers" => opts.workers = it.next().ok_or(usage)?.parse().map_err(|e| format!("--workers: {e}"))?,
            "--max-books" => opts.max_books = Some(it.next().ok_or(usage)?.parse().map_err(|e| format!("--max-books: {e}"))?),
            "--app-reading" => opts.app_reading = Some(it.next().ok_or(usage)?.into()),
            _ => plain.push(a),
        }
    }
    let [books, library] = <[String; 2]>::try_from(plain).map_err(|_| usage.to_string())?;
    let library = Library::new(PathBuf::from(library));
    let started = Instant::now();
    let mut last: Option<usize> = None;
    let built = library
        .build(&[PathBuf::from(books)], &opts, &mut |p| {
            if last != Some(p.book) {
                last = Some(p.book);
                eprintln!("[{:>6.0} s] book {}/{}: {}", started.elapsed().as_secs_f64(), p.book + 1, p.books, p.path.display());
            }
            true
        })
        .map_err(|e| e.to_string())?;
    let total = started.elapsed().as_secs_f64();
    let (mut pages, mut positions, mut scanned) = (0u64, 0u64, 0u64);
    println!("book\tstate\tpages\tseconds\tscanned\tdiagrams\tread\tmoves\tgames\texercises\tpositions\tindex bytes");
    for b in &built.books {
        let c = b.book.counts.unwrap_or_default();
        let run = b.run.unwrap_or_default();
        pages = pages.saturating_add(u64::from(b.book.pages.unwrap_or(0)));
        positions = positions.saturating_add(u64::from(c.positions));
        scanned = scanned.saturating_add(run.scanned as u64);
        let name = b.book.paths.first().map(|p| p.rsplit(['/', '\\']).next().unwrap_or(p).to_owned()).unwrap_or_default();
        println!(
            "{name}\t{}{}\t{}\t{:.1}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            b.book.state.name(),
            b.book.reason.map(|r| format!(" ({})", r.name())).unwrap_or_default(),
            b.book.pages.unwrap_or(0),
            run.seconds,
            run.scanned,
            c.diagrams,
            c.read,
            c.moves.saturating_add(c.pgn_moves),
            c.games,
            c.exercises,
            c.positions,
            b.book.index_bytes,
        );
    }
    let per = if positions == 0 { 0.0 } else { built.bytes as f64 / positions as f64 };
    println!(
        "TOTAL\t{} books ({} read now, {} left, {} in the cloud)\t{pages} pages\t{total:.0} s\t{scanned} scanned\t{positions} positions\t{} bytes of indexes ({per:.1} B a position)\tworkers {}",
        built.books.len(),
        built.read,
        built.left,
        built.cloud,
        built.bytes,
        opts.workers
    );
    Ok(())
}
