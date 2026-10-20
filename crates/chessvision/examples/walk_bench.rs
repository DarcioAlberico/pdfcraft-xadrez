//! Game-walk benchmark on documents read before. Not run in CI.
//!
//! ```text
//! cargo run --release -p pdfcraft-chessvision --example walk_bench -- <document.json>… [--dump=<file>] [--plain]
//! ```
//!
//! Each file is a chess document as `chess_document` writes it (`out`). Its pages are judged again
//! by the current game walker ([`DocBuilder::add_read_page`]), with no reading or OCR, and the
//! moves are counted by status, so a change to the walker is measured on its own. `--dump` writes
//! every move (file, page, printed text with `*` before a bold one, status) one per line, so two runs can be compared move by
//! move. `--plain` forgets which moves were printed in bold first, as a document read before the
//! weight was kept (or from OCR) has it, so the weight's part is measured on the same reading.
//!
//! The documents come from the user's own books and stay outside the repository.

use std::collections::BTreeMap;

use pdfcraft_chessvision::doc::{Block, ChessDoc, ChessPage, DocBuilder, Inline};

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

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // `--dump=<file>`: every move's page, printed text and status, one per line, to compare runs.
    let mut dump = args.iter().find_map(|a| a.strip_prefix("--dump=")).map(|p| std::fs::File::create(p).map_err(|e| e.to_string())).transpose()?;
    let forget = args.iter().any(|a| a == "--plain");
    let mut total: BTreeMap<String, usize> = BTreeMap::new();
    for path in args.iter().filter(|a| !a.starts_with("--")) {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        let doc = ChessDoc::from_json(&text).map_err(|e| format!("{path}: {e}"))?;
        let mut builder = DocBuilder::new();
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
