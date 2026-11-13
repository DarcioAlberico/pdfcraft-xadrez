//! Book-PGN alignment benchmark on a document read before. Not run in CI; `cargo xtask
//! chess-gate` runs it against a baseline when the walker changes (plan A9, Q2).
//!
//! ```text
//! cargo run --release -p pdfcraft-chessvision --example reference_bench -- <document.json> <book.pgn> [--lost=N] [--annotate=OUT.pgn]
//!     [--save-baseline=<file>] [--baseline=<file> [--allow-lost=N]]
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
//! `--save-baseline=<file>` keeps how each move stands (by page, place and text);
//! `--baseline=<file>` matches each move with that run's (the rule of `cargo xtask chess-bench`,
//! `common/moves_match.rs`), lists the moves the PGN confirmed or recovered there and not now,
//! and fails when there are more than `--allow-lost` (0 by default). Flags take their value after
//! `=` or as the next argument.
//!
//! The book and its PGN are the user's own and stay outside the repository.

use std::collections::BTreeMap;
use std::time::Instant;

use pdfcraft_chessvision::doc::reference::{LinkKind, ReferenceIndex, align, annotate};
use pdfcraft_chessvision::doc::{ChessDoc, MoveStatus};

#[path = "common/moves_match.rs"]
mod moves_match;

use moves_match::Key;

/// The baseline file's format name.
const BASELINE: &str = "pdfcraft-chess-reference-baseline";

/// A link the PGN backs: the move is the book's.
fn backed(kind: &str) -> bool {
    matches!(kind, "confirmed" | "recovered")
}

fn kind_name(kind: LinkKind) -> &'static str {
    match kind {
        LinkKind::Confirmed => "confirmed",
        LinkKind::Recovered => "recovered",
        LinkKind::Conflict => "conflict",
        LinkKind::Absent => "absent",
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let valued = ["lost", "annotate", "save-baseline", "baseline", "allow-lost"];
    let (mut flags, mut files) = (BTreeMap::<String, String>::new(), Vec::new());
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let Some(flag) = arg.strip_prefix("--") else {
            files.push(arg);
            continue;
        };
        match flag.split_once('=') {
            Some((name, value)) => flags.insert(name.to_owned(), value.to_owned()),
            None if valued.contains(&flag) => flags.insert(flag.to_owned(), it.next().ok_or(format!("--{flag} needs a value"))?),
            None => flags.insert(flag.to_owned(), String::new()),
        };
    }
    let lost = flags.get("lost").and_then(|n| n.parse::<usize>().ok()).unwrap_or(0);
    let annotate = flags.get("annotate");
    let allow: usize = flags.get("allow-lost").map_or(Ok(0), |n| n.parse())?;
    let [doc_path, pgn_path] = files.as_slice() else {
        return Err(
            "usage: reference_bench <document.json> <book.pgn> [--lost=N] [--annotate=OUT.pgn] [--save-baseline=F] [--baseline=F [--allow-lost=N]]"
                .into(),
        );
    };
    let doc = ChessDoc::from_json(&std::fs::read_to_string(doc_path)?)?;
    let (pgn, _) = pdfcraft_chess::pgn::decode(&std::fs::read(pgn_path)?);

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
    let mut links: BTreeMap<Key, String> = BTreeMap::new();
    for link in &alignment.moves {
        let Some(mv) = link.at.find(&doc) else { continue };
        *by.entry((format!("{:?}", mv.status), format!("{:?}", link.kind))).or_default() += 1;
        if link.kind == LinkKind::Absent && mv.status != MoveStatus::Valid && lost_moves.len() < lost {
            lost_moves.push(format!("p.{} {}{} ({:?})", link.at.page + 1, mv.number.map(|n| n.text()).unwrap_or_default(), mv.text, mv.status));
        }
        let (x, y) = mv.source_rect.map_or((0, 0), |r| (r.x0.round() as i64, r.y0.round() as i64));
        links.insert((u64::from(link.at.page), x, y, mv.text.clone()), kind_name(link.kind).to_owned());
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
        let made = annotate::annotated_pgn(
            &doc,
            &index,
            &book,
            pdfcraft_chessvision::doc::markdown::PgnPositions::None,
            pdfcraft_chess::pgn_style::PgnStyle::FULL,
        );
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
    if let Some(path) = flags.get("save-baseline") {
        let moves: Vec<serde_json::Value> = links.iter().map(|((p, x, y, t), k)| serde_json::json!([p, x, y, t, k])).collect();
        std::fs::write(path, serde_json::to_vec(&serde_json::json!({ "format": BASELINE, "version": 1, "moves": moves }))?)?;
        println!("baseline written to {path}");
    }
    if let Some(path) = flags.get("baseline") {
        let file: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
        if file["format"] != BASELINE {
            return Err(format!("{path}: not a reference_bench baseline").into());
        }
        let mut before: BTreeMap<Key, String> = BTreeMap::new();
        for m in file["moves"].as_array().into_iter().flatten() {
            let (Some(p), Some(x), Some(y), Some(t), Some(k)) = (m[0].as_u64(), m[1].as_i64(), m[2].as_i64(), m[3].as_str(), m[4].as_str()) else {
                return Err(format!("{path}: a move is not [page, x, y, text, link]").into());
            };
            before.insert((p, x, y, t.to_owned()), k.to_owned());
        }
        let (gained, down) = moves_match::compare(&links, &before, backed);
        println!("against the baseline: +{gained} -{} backed by the PGN (allowed: {allow})", down.len());
        for k in down.iter().take(10) {
            println!("    lost: page {} {} -> {}", k.0 + 1, k.3, links.get(*k).map_or("gone", String::as_str));
        }
        if down.len() > allow {
            return Err(format!("{} moves the PGN backed in the baseline are not now (allowed: {allow})", down.len()).into());
        }
    }
    Ok(())
}
