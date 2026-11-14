//! The book's PGN's index at the caps (plan A9-I3b, R8-06): make a synthetic book's PGN, or index
//! one and say how long it took and what went in. The process's peak memory is measured from
//! outside (the gate's measure script polls it).
//!
//! ```text
//! cargo run --release -p pdfcraft-chessvision --example index_bench -- make 4096 book.pgn
//! cargo run --release -p pdfcraft-chessvision --example index_bench -- index book.pgn [MIB]
//! ```
//!
//! `index` reads the file as the app does (`pgn::decode`) and indexes it once
//! (`ReferenceIndex::from_pgn`): the work the app did on its interface thread when a document's
//! PGN was loaded. With `MIB`, the index is given that many MiB in place of its cap
//! (`from_pgn_within`, plan A9-I3b). Without it, the example uses only what the index had before
//! plan A9-I3b, so the same file measured the build before and after.

use std::time::Instant;

use pdfcraft_chessvision::doc::reference::ReferenceIndex;

#[path = "../src/doc/reference/synthetic.rs"]
mod synthetic;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["make", games, out] => {
            let t = Instant::now();
            let text = synthetic::book_pgn(games.parse()?, 0x5_eeda_913b);
            std::fs::write(out, &text)?;
            println!("made {out}: {games} games, {} bytes, in {:.1} s", text.len(), t.elapsed().as_secs_f64());
        }
        ["index", path, rest @ ..] => {
            // A cap other than the index's own, in MiB (`index PGN 1024`: as good as none).
            let memory: Option<usize> = rest.first().map(|m| m.parse::<usize>().map(|m| m << 20)).transpose()?;
            let bytes = std::fs::read(path)?;
            let t = Instant::now();
            let (text, _) = pdfcraft_chess::pgn::decode(&bytes);
            let index = match memory {
                Some(m) => ReferenceIndex::from_pgn_within(&text, m)?,
                None => ReferenceIndex::from_pgn(&text)?,
            };
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            println!(
                "indexed {path}: {ms:.0} ms, {} games, {} moves, {} positions, truncated {}",
                index.games().len(),
                index.moves(),
                index.positions(),
                index.truncated()
            );
            // Held to the end, so the peak measured from outside is the index's.
            std::hint::black_box(&index);
        }
        _ => return Err("usage: index_bench make GAMES OUT | index PGN [MIB]".into()),
    }
    Ok(())
}
