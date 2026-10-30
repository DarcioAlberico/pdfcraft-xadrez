//! Book-PGN alignment benchmark on a document read before. Not run in CI.
//!
//! ```text
//! cargo run --release -p pdfcraft-chessvision --example reference_bench -- <document.json> <book.pgn> [--lost=N] [--annotate=OUT.pgn]
//! ```
//!
//! The document is a chess document as `chess_document` writes it (`out`); the PGN is the book's
//! own (plan "Carregar PGN do livro", R1). It prints how the reader's moves stand against the PGN:
//! the valid moves the PGN confirms, the moves the reader could not play that the PGN recovers
//! (by the reader's status), the conflicts and what stays absent, and how long the alignment took.
//! `--lost=N` lists the first N moves the reader could not play and the PGN did not find either.
//! `--annotate=OUT.pgn` also writes the PGN with the book's text as comments (R4) and says what
//! went in.
//!
//! The book and its PGN are the user's own and stay outside the repository.

use std::collections::BTreeMap;
use std::time::Instant;

use pdfcraft_chessvision::doc::reference::{LinkKind, ReferenceIndex, align, annotate};
use pdfcraft_chessvision::doc::{ChessDoc, Inline, MoveStatus};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let lost = args.iter().find_map(|a| a.strip_prefix("--lost=")).and_then(|n| n.parse::<usize>().ok()).unwrap_or(0);
    let annotate = args.iter().find_map(|a| a.strip_prefix("--annotate="));
    let files: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    let [doc_path, pgn_path] = files.as_slice() else {
        return Err("usage: reference_bench <document.json> <book.pgn> [--lost=N] [--annotate=OUT.pgn]".into());
    };
    let doc = ChessDoc::from_json(&std::fs::read_to_string(doc_path)?)?;
    let pgn = std::fs::read_to_string(pgn_path)?;

    let started = Instant::now();
    let index = ReferenceIndex::from_pgn(&pgn)?;
    let indexed = started.elapsed();
    let started = Instant::now();
    let alignment = align(&doc, &index);
    let aligned = started.elapsed();

    let stopped = index.games().iter().filter(|g| g.stopped.is_some() || g.error.is_some()).count();
    println!(
        "PGN: {} games ({stopped} stopped), {} moves, {} positions; indexed in {indexed:.1?}",
        index.games().len(),
        index.moves(),
        index.positions()
    );
    println!("aligned in {aligned:.1?}, {} rounds; {} pages placed in a game", alignment.rounds, alignment.pages.len());

    let mut by: BTreeMap<(String, String), usize> = BTreeMap::new();
    let mut lost_moves = Vec::new();
    for link in &alignment.moves {
        let Some(Inline::Move(mv)) = doc
            .pages
            .iter()
            .find(|p| p.index == link.at.page)
            .and_then(|p| p.blocks.get(link.at.block))
            .and_then(|b| b.inlines().get(link.at.inline))
        else {
            continue;
        };
        *by.entry((format!("{:?}", mv.status), format!("{:?}", link.kind))).or_default() += 1;
        if link.kind == LinkKind::Absent && mv.status != MoveStatus::Valid && lost_moves.len() < lost {
            lost_moves.push(format!("p.{} {}{} ({:?})", link.at.page + 1, mv.number.map(|n| n.text()).unwrap_or_default(), mv.text, mv.status));
        }
    }
    let c = alignment.counts;
    println!("moves: {} confirmed, {} recovered, {} conflict, {} absent", c.confirmed, c.recovered, c.conflict, c.absent);
    for ((status, kind), n) in &by {
        println!("  {status:>14} -> {kind:<9} {n}");
    }
    let diagrams = alignment.diagrams.iter().filter(|d| d.node.is_some()).count();
    println!("diagrams: {diagrams} of {} placed on a PGN move", alignment.diagrams.len());
    for m in lost_moves {
        println!("  lost: {m}");
    }
    if let Some(out) = annotate {
        let book = pdfcraft_chessvision::doc::markdown::PgnBook::default();
        let started = Instant::now();
        let made = annotate::annotated_pgn(&doc, &index, &book, pdfcraft_chess::pgn_style::PgnStyle::FULL);
        println!(
            "annotated in {:.1?}: {} games, {} moves commented, {} moves only in the book, {} diagram marks, {} bytes",
            started.elapsed(),
            made.games,
            made.commented,
            made.book_only,
            made.diagrams,
            made.pgn.len()
        );
        std::fs::write(out, made.pgn)?;
    }
    Ok(())
}
