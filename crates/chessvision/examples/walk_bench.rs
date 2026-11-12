//! Game-walk benchmark on documents read before. Not run in CI; `cargo xtask chess-gate` runs it
//! against a baseline when the walker changes (plan A9, Q2).
//!
//! ```text
//! cargo run --release -p pdfcraft-chessvision --example walk_bench -- <document.json>… [--dump=<file>] [--plain] [--rules=…]
//!     [--reference=<book.pgn>] [--save-baseline=<file>] [--baseline=<file> [--allow-lost=N]]
//! ```
//!
//! Each file is a chess document as `chess_document` writes it (`out`). Its pages are judged again
//! by the current game walker ([`DocBuilder::add_read_page`]), with no reading or OCR, and the
//! moves are counted by status, and the valid ones whose game starts from a diagram (a book's
//! moves read from the initial position are valid by accident when the reader missed the diagram
//! they start from), so a change to the walker is measured on its own. `--dump` writes
//! every move (file, page, printed text with `*` before a bold one, status) one per line, so two runs can be compared move by
//! move. `--plain` forgets which moves were printed in bold first, as a document read before the
//! weight was kept (or from OCR) has it, so the weight's part is measured on the same reading.
//! `--rules=` reads with other [`WalkRules`] than the book's: `none`, `all`, or a comma-separated
//! list of `implied_close`, `prose_moves`, `separators`, `results`, `ellipsis`, `misread_files`, so each rule's
//! effect on whole books is measured before it is turned on for them.
//!
//! The truth, not the count (finding A7-09): a move that is valid but wrong counts as valid, so
//! - `--reference=<book.pgn>` aligns each document walked with the book's own PGN
//!   (`reference::align`) and says how many moves it confirms, recovers, finds in conflict or
//!   does not have;
//! - `--save-baseline=<file>` keeps every move's status, by document, page, place and text;
//!   `--baseline=<file>` matches each move with that run's (the rule of `cargo xtask chess-bench`,
//!   `common/moves_match.rs`), lists the moves valid there and not now, and fails when there are
//!   more than `--allow-lost` (0 by default). Flags take their value after `=` or as the next
//!   argument.
//!
//! The documents come from the user's own books and stay outside the repository.

use std::collections::BTreeMap;

use pdfcraft_chessvision::doc::reference::{ReferenceIndex, align};
use pdfcraft_chessvision::doc::{Block, ChessDoc, ChessPage, DocBuilder, GameOrigin, Inline, MoveStatus, WalkRules};

#[path = "common/moves_match.rs"]
mod moves_match;

use moves_match::Key;

/// The baseline file's format name.
const BASELINE: &str = "pdfcraft-chess-walk-baseline";

/// Forget which moves were printed in bold.
fn plain(page: &mut ChessPage) {
    for block in &mut page.blocks {
        let (Block::Heading { inlines, .. } | Block::Paragraph { inlines, .. }) = block else { continue };
        for inline in inlines {
            if let Inline::Move(m) = inline {
                m.bold = false;
            }
        }
    }
}

/// `--rules=` as rules.
fn rules(spec: &str) -> Result<WalkRules, String> {
    let mut r = WalkRules::NONE;
    for name in spec.split(',').map(str::trim).filter(|n| !n.is_empty()) {
        match name {
            "none" => r = WalkRules::NONE,
            "all" => r = WalkRules::ALL,
            "book" => r = WalkRules::BOOK,
            "implied_close" => r.implied_close = true,
            "prose_moves" => r.prose_moves = true,
            "separators" => r.separators = true,
            "results" => r.results = true,
            "ellipsis" => r.ellipsis = true,
            "misread_files" => r.misread_files = true,
            other => return Err(format!("unknown rule {other}")),
        }
    }
    Ok(r)
}

/// The command line: flags (`--name=value`, `--name value` for those that take one, or `--name`)
/// and the files.
struct Args {
    flags: BTreeMap<String, String>,
    files: Vec<String>,
}

impl Args {
    /// `args`, where the flags `valued` take a value.
    fn parse(args: Vec<String>, valued: &[&str]) -> Result<Args, String> {
        let (mut flags, mut files) = (BTreeMap::new(), Vec::new());
        let mut it = args.into_iter();
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
        Ok(Args { flags, files })
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.flags.get(name).map(String::as_str)
    }
}

/// A move status as the documents write it (`valid`, `illegal`, …).
fn status_name(status: MoveStatus) -> String {
    serde_json::to_value(status).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_else(|| format!("{status:?}"))
}

fn main() -> Result<(), String> {
    let args = Args::parse(std::env::args().skip(1).collect(), &["dump", "rules", "reference", "save-baseline", "baseline", "allow-lost"])?;
    // `--dump=<file>`: every move's page, printed text and status, one per line, to compare runs.
    let mut dump = args.get("dump").map(|p| std::fs::File::create(p).map_err(|e| e.to_string())).transpose()?;
    let forget = args.get("plain").is_some();
    let rules = args.get("rules").map(rules).transpose()?.unwrap_or_default();
    let reference = match args.get("reference") {
        Some(path) => {
            let pgn = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
            Some(ReferenceIndex::from_pgn(&pdfcraft_chess::pgn::decode(&pgn).0).map_err(|e| format!("{path}: {e}"))?)
        }
        None => None,
    };
    let allow: usize = args.get("allow-lost").map_or(Ok(0), str::parse).map_err(|e| format!("--allow-lost: {e}"))?;
    let baseline: Option<BTreeMap<String, BTreeMap<Key, String>>> = args.get("baseline").map(load_baseline).transpose()?;
    let mut total: BTreeMap<String, usize> = BTreeMap::new();
    let (mut from_diagram, mut links) = (0usize, [0usize; 4]);
    let mut kept: BTreeMap<String, BTreeMap<Key, String>> = BTreeMap::new();
    let (mut gained, mut lost) = (0usize, 0usize);
    for path in &args.files {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        let doc = ChessDoc::from_json(&text).map_err(|e| format!("{path}: {e}"))?;
        let mut builder = DocBuilder::with_rules(rules);
        for mut page in doc.pages {
            if forget {
                plain(&mut page);
            }
            builder.add_read_page(page);
        }
        let doc = builder.finish();
        let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        let mut statuses: BTreeMap<Key, String> = BTreeMap::new();
        let mut diagram_moves = 0usize;
        for page in &doc.pages {
            for block in &page.blocks {
                let (Block::Heading { inlines, .. } | Block::Paragraph { inlines, .. }) = block else { continue };
                for inline in inlines {
                    let Inline::Move(m) = inline else { continue };
                    *counts.entry(format!("{:?}", m.status)).or_default() += 1;
                    let status = status_name(m.status);
                    if m.status == MoveStatus::Valid
                        && m.game.and_then(|g| doc.games.get(g.game)).is_some_and(|g| matches!(g.origin, GameOrigin::Diagram { .. }))
                    {
                        diagram_moves += 1;
                    }
                    let (x, y) = m.source_rect.map_or((0, 0), |r| (r.x0.round() as i64, r.y0.round() as i64));
                    statuses.insert((u64::from(page.index), x, y, m.text.clone()), status);
                    if let Some(f) = dump.as_mut() {
                        use std::io::Write;
                        let _ = writeln!(
                            f,
                            "{path}	p.{}	{}{}{}	{:?}",
                            page.index + 1,
                            if m.bold { "*" } else { "" },
                            m.number.map(|n| n.text()).unwrap_or_default(),
                            m.text,
                            m.status
                        );
                    }
                }
            }
        }
        let moves: usize = counts.values().sum();
        let mut note = String::new();
        if let Some(index) = &reference {
            let c = align(&doc, index).counts;
            for (slot, n) in links.iter_mut().zip([c.confirmed, c.recovered, c.conflict, c.absent]) {
                *slot += n;
            }
            note = format!("; book PGN: {} confirmed, {} recovered, {} conflict, {} absent", c.confirmed, c.recovered, c.conflict, c.absent);
        }
        println!("{name}: {moves} moves, games {}, {diagram_moves} valid from a diagram: {counts:?}{note}", doc.games.len());
        if let Some(before) = baseline.as_ref().and_then(|b| b.get(&name)) {
            let (up, down) = moves_match::compare(&statuses, before, |s| s == "valid");
            gained += up;
            lost += down.len();
            println!("  against the baseline: +{up} -{}", down.len());
            for k in down.iter().take(10) {
                println!("    lost: page {} {} -> {}", k.0 + 1, k.3, statuses.get(*k).map_or("gone", String::as_str));
            }
        }
        from_diagram += diagram_moves;
        for (k, v) in counts {
            *total.entry(k).or_default() += v;
        }
        kept.insert(name, statuses);
    }
    let valid = total.get("Valid").copied().unwrap_or(0);
    println!("total: {total:?}");
    println!("valid: {valid}, valid from a diagram: {from_diagram}");
    if reference.is_some() {
        println!("book PGN: {} confirmed, {} recovered, {} conflict, {} absent", links[0], links[1], links[2], links[3]);
    }
    if let Some(path) = args.get("save-baseline") {
        save_baseline(path, &kept)?;
        println!("baseline written to {path}");
    }
    if let Some(b) = &baseline {
        let missing: Vec<&String> = b.keys().filter(|n| !kept.contains_key(*n)).collect();
        println!(
            "against the baseline: +{gained} -{lost} (allowed: {allow}){}",
            if missing.is_empty() { String::new() } else { format!("; not walked: {missing:?}") }
        );
        if lost > allow {
            return Err(format!("{lost} moves valid in the baseline are not now (allowed: {allow})"));
        }
    }
    Ok(())
}

/// Write each document's moves: `{"format", "version": 1, "documents": {name: [[page, x, y, text, status], …]}}`.
fn save_baseline(path: &str, docs: &BTreeMap<String, BTreeMap<Key, String>>) -> Result<(), String> {
    let documents: serde_json::Map<String, serde_json::Value> =
        docs.iter().map(|(name, moves)| (name.clone(), moves.iter().map(|((p, x, y, t), s)| serde_json::json!([p, x, y, t, s])).collect())).collect();
    let file = serde_json::json!({ "format": BASELINE, "version": 1, "documents": documents });
    std::fs::write(path, serde_json::to_vec(&file).map_err(|e| e.to_string())?).map_err(|e| format!("{path}: {e}"))
}

/// A baseline [`save_baseline`] wrote.
fn load_baseline(path: &str) -> Result<BTreeMap<String, BTreeMap<Key, String>>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let file: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
    if file["format"] != BASELINE {
        return Err(format!("{path}: not a walk_bench baseline"));
    }
    let mut out = BTreeMap::new();
    for (name, moves) in file["documents"].as_object().into_iter().flatten() {
        let mut map = BTreeMap::new();
        for m in moves.as_array().into_iter().flatten() {
            let (Some(p), Some(x), Some(y), Some(t), Some(s)) = (m[0].as_u64(), m[1].as_i64(), m[2].as_i64(), m[3].as_str(), m[4].as_str()) else {
                return Err(format!("{path}: {name}: a move is not [page, x, y, text, status]"));
            };
            map.insert((p, x, y, t.to_owned()), s.to_owned());
        }
        out.insert(name.clone(), map);
    }
    Ok(out)
}
