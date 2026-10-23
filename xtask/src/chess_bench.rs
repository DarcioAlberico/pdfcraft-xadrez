//! `cargo xtask chess-bench --dir DIR --tag NAME [--pages N] [--jobs N] [--baseline TAG]
//! [--allow-lost N] [--cli PATH] [--models DIR]`: read chess books with `pdfcraft-cli` and compare the moves with
//! an earlier run, so a change to the game reader shows what it gained and what it lost.
//!
//! `DIR` holds the books as `books/*.pdf` (they are never committed: books are copyrighted) and
//! gets each run's documents in `out/<tag>/<book>.json`. Each book is read on `N` pages from a
//! quarter of the way in (20 by default; 0 reads every page). With `--baseline`, every move is
//! matched with the same move (page and text, its place within a few points) of that run: the moves that were valid there
//! and are not now are listed, and more than `--allow-lost` of them (0 by default) fails.
//!
//! The piece models come from `--models` (else `PDFCRAFT_MODELS` or wherever `pdfcraft-cli`
//! finds them). Without them only diagrams set in a chess font are read, and a run compared with
//! one made with them loses most of its moves.
//!
//! A move counts as valid "from a diagram" when its game starts from a diagram: a book's moves
//! read from the initial position are valid by accident when the reader missed the diagram they
//! start from, so that count is the one to watch.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context, bail};
use serde_json::{Value, json};

/// What one run says of one book.
#[derive(Default)]
struct Tally {
    moves: usize,
    valid: usize,
    from_diagram: usize,
    statuses: BTreeMap<String, usize>,
}

/// A move's identity across runs: page, place (rounded to a point) and text.
type Key = (u64, i64, i64, String);

pub fn run(args: &[String]) -> anyhow::Result<()> {
    let opt = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let dir = PathBuf::from(opt("--dir").context("--dir DIR (with books/*.pdf) is required")?);
    let tag = opt("--tag").context("--tag NAME is required")?;
    let pages: u64 = opt("--pages").map_or(Ok(20), |p| p.parse()).context("--pages N")?;
    let jobs: usize = opt("--jobs").map_or(Ok(2), |j| j.parse()).context("--jobs N")?;
    let allow: usize = opt("--allow-lost").map_or(Ok(0), |n| n.parse()).context("--allow-lost N")?;
    let baseline = opt("--baseline");
    let models = opt("--models").map(PathBuf::from);
    let cli = match opt("--cli") {
        Some(c) => PathBuf::from(c),
        None => {
            let mut c = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
            c.current_dir(crate::gates::root()).args(["build", "--release", "-p", "pdfcraft-cli"]);
            if !c.status().context("cargo build -p pdfcraft-cli")?.success() {
                bail!("cargo build --release -p pdfcraft-cli failed");
            }
            crate::gates::release_cli()
        }
    };
    let mut books: Vec<String> = std::fs::read_dir(dir.join("books"))
        .with_context(|| format!("{}: no books/ folder", dir.display()))?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.to_ascii_lowercase().ends_with(".pdf"))
        .collect();
    books.sort();
    if books.is_empty() {
        bail!("{}: books/ has no PDF", dir.join("books").display());
    }
    std::fs::create_dir_all(dir.join("out").join(&tag))?;
    let next = AtomicUsize::new(0);
    let failures = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..jobs.max(1) {
            s.spawn(|| {
                while let Some(book) = books.get(next.fetch_add(1, Ordering::Relaxed)) {
                    if let Err(e) = read_book(&cli, models.as_deref(), &dir, &tag, book, pages) {
                        failures.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(format!("{book}: {e:#}"));
                    }
                }
            });
        }
    });
    let failures = failures.into_inner().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut total = (Tally::default(), Tally::default());
    let (mut gained, mut lost) = (0, 0);
    println!("{:<40} {:>7} {:>7} {:>9}  against the baseline", "book", "moves", "valid", "diagram");
    for book in &books {
        let stem = book.trim_end_matches(".pdf").trim_end_matches(".PDF");
        let Ok(now) = load(&dir.join("out").join(&tag).join(format!("{stem}.json"))) else { continue };
        let counts = tally(&now);
        let mut note = String::new();
        if let Some(base) = &baseline
            && let Ok(before) = load(&dir.join("out").join(base).join(format!("{stem}.json")))
        {
            let (a, b) = (statuses(&before), statuses(&now));
            let (up, down) = (newly_valid(&b, &a).len(), newly_valid(&a, &b));
            gained += up;
            lost += down.len();
            note = format!("+{up} -{}", down.len());
            for k in down.iter().take(10) {
                note.push_str(&format!("\n    lost: page {} {} -> {}", k.0 + 1, k.3, b.get(*k).map_or("gone", String::as_str)));
            }
            total.1.valid += tally(&before).valid;
        }
        println!("{:<40} {:>7} {:>7} {:>9}  {note}", truncate(stem, 40), counts.moves, counts.valid, counts.from_diagram);
        total.0.moves += counts.moves;
        total.0.valid += counts.valid;
        total.0.from_diagram += counts.from_diagram;
        for (s, n) in counts.statuses {
            *total.0.statuses.entry(s).or_default() += n;
        }
    }
    println!("{:<40} {:>7} {:>7} {:>9}", "all", total.0.moves, total.0.valid, total.0.from_diagram);
    println!("statuses: {:?}", total.0.statuses);
    if baseline.is_some() {
        println!("against the baseline: valid {} -> {}, +{gained} -{lost}", total.1.valid, total.0.valid);
    }
    if !failures.is_empty() {
        bail!("books not read:\n{}", failures.join("\n"));
    }
    if lost > allow {
        bail!("{lost} moves valid in the baseline are not now (allowed: {allow})");
    }
    Ok(())
}

/// Read one book's sample of pages into `out/<tag>/<stem>.json`.
fn read_book(cli: &Path, models: Option<&Path>, dir: &Path, tag: &str, book: &str, pages: u64) -> anyhow::Result<()> {
    let stem = book.trim_end_matches(".pdf").trim_end_matches(".PDF");
    let out = dir.join("out").join(tag);
    let open = json!({"tool": "doc_open", "args": {"path": format!("books/{book}")}});
    let report = script(cli, models, dir, &out.join(format!("{stem}.open.json")), &json!([open]))?;
    let count = report["steps"][0]["result"][0]["pages"].as_u64().context("doc_open gave no page count")?;
    let list: Vec<u64> = if pages == 0 {
        (1..=count).collect()
    } else {
        let start = (count / 4).max(1);
        (start..=count.min(start.saturating_add(pages.saturating_sub(1)))).collect()
    };
    let read = json!({"tool": "chess_document", "args": {"doc": 1, "pages": list, "out": format!("out/{tag}/{stem}.json")}});
    script(cli, models, dir, &out.join(format!("{stem}.steps.json")), &json!([open, read]))?;
    Ok(())
}

/// Run a script of automation steps with `dir` as the root; its report.
fn script(cli: &Path, models: Option<&Path>, dir: &Path, path: &Path, steps: &Value) -> anyhow::Result<Value> {
    std::fs::write(path, serde_json::to_string_pretty(steps)?)?;
    let mut command = Command::new(cli);
    if let Some(models) = models {
        command.env("PDFCRAFT_MODELS", models);
    }
    let out = command
        .arg("run")
        .arg("--script")
        .arg(path)
        .arg("--root")
        .arg(dir)
        .arg("--report")
        .output()
        .with_context(|| format!("{}: failed to start", cli.display()))?;
    let report: Value = serde_json::from_slice(&out.stdout).with_context(|| format!("no report: {}", String::from_utf8_lossy(&out.stderr)))?;
    if report["ok"] != json!(true) {
        bail!("{}", report["steps"].as_array().and_then(|s| s.last()).map_or_else(|| report.to_string(), Value::to_string));
    }
    Ok(report)
}

fn load(path: &Path) -> anyhow::Result<Value> {
    Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
}

/// Every move of a document: its key and status.
fn statuses(doc: &Value) -> BTreeMap<Key, String> {
    let mut out = BTreeMap::new();
    for (page, m) in moves(doc) {
        let r = &m["source_rect"];
        let at = |k: &str| r[k].as_f64().unwrap_or(0.0).round() as i64;
        let key = (page, at("x0"), at("y0"), m["text"].as_str().unwrap_or("").to_owned());
        out.insert(key, m["status"].as_str().unwrap_or("?").to_owned());
    }
    out
}

/// The moves valid in `now` and not in `then`.
///
/// A move is the same move in both runs when its page and text are and its place differs by at
/// most [`SAME_PLACE`] points either way: a change to the reader can move a move's box a little
/// (OCR measures its glyphs against another text scale), and that is not a move lost.
fn newly_valid<'a>(now: &'a BTreeMap<Key, String>, then: &BTreeMap<Key, String>) -> Vec<&'a Key> {
    now.iter()
        .filter(|(k, s)| {
            s.as_str() == "valid"
                && !then
                    .range((k.0, i64::MIN, i64::MIN, String::new())..)
                    .take_while(|(o, _)| o.0 == k.0)
                    .any(|(o, st)| st.as_str() == "valid" && o.3 == k.3 && (o.1 - k.1).abs() <= SAME_PLACE && (o.2 - k.2).abs() <= SAME_PLACE)
        })
        .map(|(k, _)| k)
        .collect()
}

/// How far, in points, a move's place may move between runs and still be the same move.
const SAME_PLACE: i64 = 4;

fn tally(doc: &Value) -> Tally {
    let mut t = Tally::default();
    let games = doc["games"].as_array().cloned().unwrap_or_default();
    for (_, m) in moves(doc) {
        let status = m["status"].as_str().unwrap_or("?").to_owned();
        t.moves += 1;
        if status == "valid" {
            t.valid += 1;
            let game = m["game"]["game"].as_u64().and_then(|g| usize::try_from(g).ok()).and_then(|g| games.get(g));
            if game.is_some_and(|g| g["origin"]["kind"] == json!("diagram")) {
                t.from_diagram += 1;
            }
        }
        *t.statuses.entry(status).or_default() += 1;
    }
    t
}

/// The moves of a document with their page indices.
fn moves(doc: &Value) -> Vec<(u64, &Value)> {
    let mut out = Vec::new();
    for page in doc["pages"].as_array().into_iter().flatten() {
        let index = page["index"].as_u64().unwrap_or(0);
        for block in page["blocks"].as_array().into_iter().flatten() {
            for inline in block["inlines"].as_array().into_iter().flatten() {
                if inline["type"] == json!("move") {
                    out.push((index, inline));
                }
            }
        }
    }
    out
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(moves: &[(&str, &str, u64)]) -> Value {
        let inlines: Vec<Value> = moves
            .iter()
            .enumerate()
            .map(|(k, (text, status, game))| json!({"type": "move", "text": text, "status": status, "game": {"game": game, "node": 0}, "source_rect": {"x0": k as f64 * 30.0, "y0": 100.0}}))
            .collect();
        json!({"pages": [{"index": 0, "blocks": [{"type": "paragraph", "inlines": inlines}]}], "games": [{"origin": {"kind": "initial"}}, {"origin": {"kind": "diagram", "page": 0, "diagram": 0}}]})
    }

    #[test]
    fn a_tally_tells_moves_valid_from_a_diagram_from_ones_valid_by_accident() {
        let t = tally(&doc(&[("e4", "valid", 0), ("Rh6+", "valid", 1), ("Kd5", "illegal", 1)]));
        assert_eq!((t.moves, t.valid, t.from_diagram), (3, 2, 1));
        assert_eq!(t.statuses.get("illegal"), Some(&1));
    }

    #[test]
    fn moves_are_matched_by_page_place_and_text() {
        let before = statuses(&doc(&[("e4", "valid", 0), ("e5", "valid", 0)]));
        let after = statuses(&doc(&[("e4", "valid", 0), ("e5", "illegal", 0)]));
        assert_eq!(newly_valid(&before, &after), [&(0, 30, 100, "e5".to_owned())]);
        assert!(newly_valid(&after, &before).is_empty());
    }

    /// A move whose box moved a few points between runs (OCR measured it against another text
    /// scale) is the same move, still valid; one that moved farther, or another move, is not.
    #[test]
    fn a_move_a_few_points_away_is_the_same_move() {
        let at = |x: i64, y: i64, text: &str| -> BTreeMap<Key, String> { [((0, x, y, text.to_owned()), "valid".to_owned())].into() };
        assert!(newly_valid(&at(100, 200, "Nf3"), &at(103, 197, "Nf3")).is_empty());
        assert_eq!(newly_valid(&at(100, 200, "Nf3"), &at(110, 200, "Nf3")).len(), 1);
        assert_eq!(newly_valid(&at(100, 200, "Nf3"), &at(100, 200, "Nc3")).len(), 1);
        let other_page: BTreeMap<Key, String> = [((1, 100, 200, "Nf3".to_owned()), "valid".to_owned())].into();
        assert_eq!(newly_valid(&at(100, 200, "Nf3"), &other_page).len(), 1);
    }
}
