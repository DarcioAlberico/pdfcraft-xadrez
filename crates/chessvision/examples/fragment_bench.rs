//! Fragment-mode self-consistency benchmark (PGN toolbar spec, step T2a). Not run in CI.
//!
//! ```text
//! cargo run --release -p pdfcraft-chessvision --example fragment_bench -- <document.json>… [--show=N] [--plain]
//! ```
//!
//! Each file is a chess document as `chess_document` writes it. Its pages are judged again by the
//! current walker reading the whole book ([`DocBuilder::add_read_page`]), as `walk_bench` does.
//! Then, for every variation the walker built (a move that is not the first reply to the move
//! before it), the text it came from is read again as a selection would be: the inlines from the
//! variation's first printed move to its last (with the glyphs after it), by
//! [`fragment::read_inlines`] from the position before the variation, and grafted at the start of
//! that position. The tree grafted is compared with the walker's variation (its line and the
//! variations inside it), moves and glyphs, comments left out. A variation whose text has moves
//! of other lines in it (a book going back to its main line between two parts of it) or a
//! diagram or heading in it is not comparable and is counted apart. `--show=N` prints the first
//! N differences of each document; `--plain` forgets which moves were printed in bold.
//!
//! The documents come from the user's own books and stay outside the repository.

use std::collections::HashSet;

use pdfcraft_chess::fragment::FragmentItem;
use pdfcraft_chess::game::{Game as ChessGame, GraftMode};
use pdfcraft_chessvision::doc::fragment::{line_items, read_inlines};
use pdfcraft_chessvision::doc::{Block, ChessDoc, DocBuilder, GameScore, Inline, MoveStatus, NodeRef};

const INITIAL: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

/// The movetext of `items` grafted at the start of `fen`, without comments; `None` when the graft
/// stopped.
fn movetext(fen: &str, items: &[FragmentItem]) -> Option<String> {
    let game = ChessGame::from_fen(fen).ok()?;
    let (game, report) = game.graft(0, GraftMode::Follow, items, None);
    let text = game.movetext();
    let mut out = String::new();
    let mut depth = 0usize;
    for c in text.chars() {
        match c {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    let out = out.split_whitespace().collect::<Vec<_>>().join(" ").replace("( ", "(").replace(" )", ")");
    report.stop.is_none().then_some(out)
}

/// Without the glyphs (`$n`).
fn bare(text: &str) -> String {
    text.split_whitespace().filter(|w| !w.starts_with('$')).collect::<Vec<_>>().join(" ")
}

/// Node `head` and every move after it.
fn subtree(score: &GameScore, head: usize) -> HashSet<usize> {
    let mut out = HashSet::new();
    let mut stack = vec![head];
    while let Some(n) = stack.pop() {
        if out.len() > score.nodes.len() || !out.insert(n) {
            continue;
        }
        if let Some(node) = score.nodes.get(n) {
            stack.extend(node.children.iter().copied());
        }
    }
    out
}

#[derive(Default)]
struct Tally {
    variations: usize,
    excluded: usize,
    identical: usize,
    same_moves: usize,
    stopped: usize,
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let show: usize = args.iter().find_map(|a| a.strip_prefix("--show=")).and_then(|n| n.parse().ok()).unwrap_or(0);
    let forget = args.iter().any(|a| a == "--plain");
    let mut total = Tally::default();
    for path in args.iter().filter(|a| !a.starts_with("--")) {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        let doc = ChessDoc::from_json(&text).map_err(|e| format!("{path}: {e}"))?;
        let mut builder = DocBuilder::new();
        for mut page in doc.pages {
            if forget {
                for block in &mut page.blocks {
                    if let Block::Heading { inlines, .. } | Block::Paragraph { inlines, .. } = block {
                        for inline in inlines {
                            if let Inline::Move(m) = inline {
                                m.bold = false;
                            }
                        }
                    }
                }
            }
            builder.add_read_page(page);
        }
        let doc = builder.finish();
        // The document's inlines in reading order; `None` for a diagram or heading between them.
        let mut flat: Vec<Option<&Inline>> = Vec::new();
        for page in &doc.pages {
            for block in &page.blocks {
                match block {
                    Block::Paragraph { inlines, .. } => flat.extend(inlines.iter().map(Some)),
                    _ => flat.push(None),
                }
            }
        }
        let node_of = |i: &Option<&Inline>| match i {
            Some(Inline::Move(m)) if m.status == MoveStatus::Valid => m.game,
            _ => None,
        };
        let mut tally = Tally::default();
        let mut shown = 0usize;
        for (gi, game) in doc.games.iter().enumerate() {
            let score = &game.score;
            for head in 0..score.nodes.len() {
                if !score.starts_variation(head) {
                    continue;
                }
                tally.variations += 1;
                let nodes = subtree(score, head);
                let inside = |r: Option<NodeRef>| r.is_some_and(|r| r.game == gi && nodes.contains(&r.node));
                let positions: Vec<usize> = flat.iter().enumerate().filter(|(_, i)| inside(node_of(i))).map(|(k, _)| k).collect();
                let (Some(&first), Some(&last)) = (positions.first(), positions.last()) else {
                    tally.excluded += 1;
                    continue;
                };
                let mut end = last;
                while matches!(flat.get(end + 1), Some(Some(Inline::Nag { .. }))) {
                    end += 1;
                }
                let range = flat.get(first..=end).unwrap_or_default();
                let mixed = range.iter().any(|i| i.is_none() || node_of(i).is_some_and(|r| !inside(Some(r))));
                if mixed {
                    tally.excluded += 1;
                    continue;
                }
                let inlines: Vec<Inline> = range.iter().flatten().map(|i| (*i).clone()).collect();
                let parent = score.nodes.get(head).and_then(|n| n.parent);
                let fen = match parent {
                    Some(p) => score.nodes.get(p).map(|n| n.fen.clone()),
                    None => Some(score.start_fen.clone().unwrap_or_else(|| INITIAL.to_owned())),
                };
                let Some(fen) = fen else {
                    tally.excluded += 1;
                    continue;
                };
                let read = read_inlines(&inlines, &fen, true);
                let walker = movetext(&fen, &line_items(score, head));
                let fragment = movetext(&fen, &read.items);
                match (&walker, &fragment) {
                    (Some(w), Some(f)) if w == f => tally.identical += 1,
                    (Some(w), Some(f)) if bare(w) == bare(f) => tally.same_moves += 1,
                    (_, None) => tally.stopped += 1,
                    _ => {}
                }
                if walker != fragment && shown < show {
                    shown += 1;
                    let source: Vec<String> = inlines.iter().map(Inline::text).collect();
                    println!("--- {} game {gi} node {head} from {fen}", path);
                    println!("  text:     {}", source.join(" "));
                    println!("  walker:   {}", walker.unwrap_or_default());
                    println!("  fragment: {}", fragment.unwrap_or_default());
                }
            }
        }
        let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        report(&name, &tally);
        total.variations += tally.variations;
        total.excluded += tally.excluded;
        total.identical += tally.identical;
        total.same_moves += tally.same_moves;
        total.stopped += tally.stopped;
    }
    report("total", &total);
    Ok(())
}

fn report(name: &str, t: &Tally) {
    let comparable = t.variations - t.excluded;
    let pct = |n: usize| if comparable == 0 { 0.0 } else { 100.0 * n as f64 / comparable as f64 };
    println!(
        "{name}: {} variations, {comparable} comparable ({} not: other lines or a diagram inside), identical {} ({:.1}%), same moves other glyphs {}, identical or same moves {:.1}%, graft stopped {}",
        t.variations,
        t.excluded,
        t.identical,
        pct(t.identical),
        t.same_moves,
        pct(t.identical + t.same_moves),
        t.stopped
    );
}
