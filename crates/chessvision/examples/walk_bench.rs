//! Game-walk benchmark on documents read before. Not run in CI.
//!
//! ```text
//! cargo run --release -p pdfcraft-chessvision --example walk_bench -- <document.json>… [--dump=<file>] [--plain] [--rules=…]
//! ```
//!
//! Each file is a chess document as `chess_document` writes it (`out`). Its pages are judged again
//! by the current game walker ([`DocBuilder::add_read_page`]), with no reading or OCR, and the
//! moves are counted by status, so a change to the walker is measured on its own. `--dump` writes
//! every move (file, page, printed text with `*` before a bold one, status) one per line, so two runs can be compared move by
//! move. `--plain` forgets which moves were printed in bold first, as a document read before the
//! weight was kept (or from OCR) has it, so the weight's part is measured on the same reading.
//! `--rules=` reads with other [`WalkRules`] than the book's: `none`, `all`, or a comma-separated
//! list of `implied_close`, `prose_moves`, `separators`, `results`, `ellipsis`, `misread_files`, so each rule's
//! effect on whole books is measured before it is turned on for them.
//!
//! The documents come from the user's own books and stay outside the repository.

use std::collections::BTreeMap;

use pdfcraft_chessvision::doc::{Block, ChessDoc, ChessPage, DocBuilder, Inline, WalkRules};

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

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // `--dump=<file>`: every move's page, printed text and status, one per line, to compare runs.
    let mut dump = args.iter().find_map(|a| a.strip_prefix("--dump=")).map(|p| std::fs::File::create(p).map_err(|e| e.to_string())).transpose()?;
    let forget = args.iter().any(|a| a == "--plain");
    let rules = args.iter().find_map(|a| a.strip_prefix("--rules=")).map(rules).transpose()?.unwrap_or_default();
    let mut total: BTreeMap<String, usize> = BTreeMap::new();
    for path in args.iter().filter(|a| !a.starts_with("--")) {
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
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for page in &doc.pages {
            for block in &page.blocks {
                let (Block::Heading { inlines, .. } | Block::Paragraph { inlines, .. }) = block else { continue };
                for inline in inlines {
                    let Inline::Move(m) = inline else { continue };
                    *counts.entry(format!("{:?}", m.status)).or_default() += 1;
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
        let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        println!("{name}: {moves} moves, games {}: {counts:?}", doc.games.len());
        for (k, v) in counts {
            *total.entry(k).or_default() += v;
        }
    }
    println!("total: {total:?}");
    Ok(())
}
