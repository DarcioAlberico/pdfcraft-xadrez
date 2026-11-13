//! The chess gate's stages 4 (benches by the truth), 4b (performance) and 5 (`--full`), and
//! `--baseline`, which makes what they compare with from `main` (plan A9-Q1b, Q2; `docs/chess/
//! plano-analise-9.md` §5 "Como medir"). See the module above for the gate's folder.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{Gate, strings};

/// A `perf_` test's time above this many times its baseline's fails.
const PERF_SLACK: f64 = 2.0;

/// The tag of the 38 books' run `--baseline --full` makes.
const BENCH38_BASE: &str = "gate-base";

/// What the gate keeps in `<gate>/paths.toml`: where the data outside the repository is.
#[derive(Default, Serialize, Deserialize)]
pub(super) struct Paths {
    /// The OCR ground truth `ocr_bench` reads (line crops and their truth).
    pub ocr_gt: Option<PathBuf>,
    /// The 38 books' folder `cargo xtask chess-bench` reads (`books/`, `out/`).
    pub bench38: Option<PathBuf>,
}

impl Paths {
    fn load(gate: &Path) -> Paths {
        std::fs::read_to_string(gate.join("paths.toml")).ok().and_then(|t| toml::from_str(&t).ok()).unwrap_or_default()
    }

    /// What is kept, with what `given` names instead.
    fn save_with(gate: &Path, given: Paths) -> anyhow::Result<Paths> {
        let kept = Paths::load(gate);
        let paths = Paths { ocr_gt: given.ocr_gt.or(kept.ocr_gt), bench38: given.bench38.or(kept.bench38) };
        std::fs::write(gate.join("paths.toml"), toml::to_string(&paths)?).context("writing paths.toml")?;
        Ok(paths)
    }
}

/// What the timings were measured on: the summary says it (no machine or account name).
pub(super) fn machine() -> serde_json::Value {
    json!({
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "threads": std::thread::available_parallelism().map_or(0, |n| n.get()),
        "profile": "release",
    })
}

/// The chess documents the benches walk: `<gate>/bench/*.json`, by name.
fn bench_docs(gate: &Path) -> Result<Vec<PathBuf>, String> {
    let dir = gate.join("bench");
    let mut docs: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map_err(|e| format!("{}: {e} (put the chess documents the benches walk there, as chess_document writes them)", dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    docs.sort();
    if docs.is_empty() {
        return Err(format!("{} holds no chess document (*.json)", dir.display()));
    }
    Ok(docs)
}

/// The documents with their book's PGN beside them (`<stem>.pgn`), and the name of each one's
/// baseline.
fn reference_pairs(docs: &[PathBuf]) -> Vec<(PathBuf, PathBuf, String)> {
    docs.iter()
        .filter_map(|d| {
            let pgn = d.with_extension("pgn");
            let stem = d.file_stem()?.to_string_lossy().into_owned();
            pgn.is_file().then(|| (d.clone(), pgn, format!("reference-{stem}.json")))
        })
        .collect()
}

fn example(examples: &Path, name: &str) -> PathBuf {
    examples.join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

/// Run a bench: see [`bench_result`].
fn run_bench(program: &Path, args: &[OsString], verdict: &str) -> Result<String, String> {
    eprintln!("$ {} {}", program.display(), args.iter().map(|a| a.to_string_lossy()).collect::<Vec<_>>().join(" "));
    let out = Command::new(program).args(args).output().map_err(|e| format!("{}: {e}", program.display()))?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    bench_result(&text, out.status.success(), verdict)
}

/// What a bench that printed `text` says: when it passed, its last line that holds `verdict`
/// (else its last lines); when it failed, its last line (the error) and then its last lines.
/// The gate's table shows a check's first line.
fn bench_result(text: &str, passed: bool, verdict: &str) -> Result<String, String> {
    let tail = super::tail(text, 12);
    if !passed {
        let last = text.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or_default().trim();
        return Err(format!("{last}\n{tail}"));
    }
    Ok(text.lines().rev().find(|l| !verdict.is_empty() && l.contains(verdict)).map_or(tail, |l| l.trim().to_owned()))
}

/// The `PERF <name> <ms>` lines of a test run.
fn perf_lines(out: &str) -> BTreeMap<String, f64> {
    out.lines()
        .filter_map(|l| {
            let mut words = l.trim().strip_prefix("PERF ")?.split_whitespace();
            let (name, ms) = (words.next()?, words.next()?.parse::<f64>().ok()?);
            ms.is_finite().then(|| (name.to_owned(), ms))
        })
        .collect()
}

/// Each time against its baseline: what is past [`PERF_SLACK`] times it.
fn perf_judge(times: &BTreeMap<String, f64>, base: Option<&BTreeMap<String, f64>>) -> Result<String, String> {
    let mut slow = Vec::new();
    let mut said = Vec::new();
    for (name, ms) in times {
        match base.and_then(|b| b.get(name)) {
            Some(b) if *ms > b * PERF_SLACK => slow.push(format!("{name}: {ms:.1} ms, more than {PERF_SLACK}x the baseline's {b:.1} ms")),
            Some(b) => said.push(format!("{name} {ms:.1} ms (baseline {b:.1})")),
            None => said.push(format!("{name} {ms:.1} ms (no baseline)")),
        }
    }
    if slow.is_empty() { Ok(said.join("; ")) } else { Err(slow.join("\n")) }
}

/// `<gate>/perf-baseline.json`'s times.
fn load_perf(gate: &Path) -> Option<BTreeMap<String, f64>> {
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(gate.join("perf-baseline.json")).ok()?).ok()?;
    Some(v["tests"].as_object()?.iter().filter_map(|(k, v)| Some((k.clone(), v.as_f64()?))).collect())
}

/// Whether a `perf_` test is written in the sources of `dirs` (under `crates/`) of the tree at
/// `root`: a release build only to find none is minutes and gigabytes for nothing.
fn has_perf_tests(root: &Path, dirs: &[&str]) -> bool {
    fn walk(dir: &Path, depth: usize) -> bool {
        let Ok(entries) = std::fs::read_dir(dir) else { return false };
        entries.flatten().any(|e| {
            let path = e.path();
            if path.is_dir() {
                depth < 8 && walk(&path, depth + 1)
            } else {
                path.extension().is_some_and(|x| x == "rs") && std::fs::read_to_string(&path).is_ok_and(|s| s.contains("fn perf_"))
            }
        })
    }
    dirs.iter().any(|d| walk(&root.join("crates").join(d).join("src"), 0))
}

/// The `perf_` test runs of the tree at `root` that have a `perf_` test: the lower crates, and
/// the engine's when `engine` (L6+ changed).
fn perf_runs(root: &Path, engine: bool) -> Vec<(&'static str, Vec<String>)> {
    let mut runs = Vec::new();
    if has_perf_tests(root, &["chess", "chessvision"]) {
        runs.push((
            "chess and chessvision",
            strings(&[
                "test",
                "--release",
                "-j",
                "2",
                "-p",
                "pdfcraft-chess",
                "-p",
                "pdfcraft-chessvision",
                "--",
                "--ignored",
                "perf_",
                "--nocapture",
            ]),
        ));
    }
    if engine && has_perf_tests(root, &["engine"]) {
        runs.push((
            "engine",
            strings(&["test", "--release", "-j", "2", "-p", "pdfcraft-engine", "--lib", "--", "--ignored", "perf_", "--nocapture"]),
        ));
    }
    runs
}

impl Gate {
    /// The walker or the vision changed.
    fn walker_changed(&self) -> bool {
        self.crates().iter().any(|(dir, _)| dir == "chess" || dir == "chessvision")
    }

    /// Stage 4: the benches by the truth.
    pub(super) fn stage4(&mut self) -> bool {
        if self.no_bench {
            eprintln!("chess-gate: benches left out (--no-bench)");
            return true;
        }
        if !self.walker_changed() {
            eprintln!("chess-gate: neither the walker nor the vision changed");
            return true;
        }
        let t = Instant::now();
        let built = super::cargo(
            &self.root,
            &strings(&[
                "build",
                "--release",
                "-j",
                "2",
                "-p",
                "pdfcraft-chessvision",
                "--example",
                "walk_bench",
                "--example",
                "reference_bench",
                "--example",
                "ocr_bench",
            ]),
            &[],
        );
        if !self.record(4, "build the benches (release)", t, built) {
            return false;
        }
        let examples = self.target.join("release").join("examples");
        let t = Instant::now();
        let docs = match bench_docs(&self.gate_dir) {
            Ok(d) => d,
            Err(e) => return self.record(4, "walk_bench against the baseline", t, Err(e)),
        };
        let baseline = self.gate_dir.join("walk-baseline.json");
        let result = if baseline.is_file() {
            let mut args: Vec<OsString> = docs.iter().map(|d| d.clone().into_os_string()).collect();
            args.extend(["--baseline".into(), baseline.into_os_string(), "--allow-lost".into(), "0".into()]);
            run_bench(&example(&examples, "walk_bench"), &args, "against the baseline:")
        } else {
            Err(format!("{} is missing: make it with `cargo xtask chess-gate --baseline`", baseline.display()))
        };
        if !self.record(4, "walk_bench against the baseline", t, result) {
            return false;
        }
        for (doc, pgn, name) in reference_pairs(&docs) {
            let t = Instant::now();
            let baseline = self.gate_dir.join(&name);
            let result = if baseline.is_file() {
                let args: Vec<OsString> = vec![
                    doc.into_os_string(),
                    pgn.into_os_string(),
                    "--baseline".into(),
                    baseline.into_os_string(),
                    "--allow-lost".into(),
                    "0".into(),
                ];
                run_bench(&example(&examples, "reference_bench"), &args, "against the baseline:")
            } else {
                Err(format!("{} is missing: make it with `cargo xtask chess-gate --baseline`", baseline.display()))
            };
            if !self.record(4, &format!("reference_bench {name}"), t, result) {
                return false;
            }
        }
        let t = Instant::now();
        let result = match (Paths::load(&self.gate_dir).ocr_gt, &self.models) {
            (_, None) => Ok("left out: no models (--no-models)".to_owned()),
            (None, _) => Err(format!("no OCR ground truth in {}: give `--ocr-gt DIR` to `--baseline`", self.gate_dir.join("paths.toml").display())),
            (Some(gt), Some(models)) => {
                let args: Vec<OsString> = vec![
                    "--gt".into(),
                    gt.into_os_string(),
                    "--glyphs".into(),
                    models.clone().into_os_string(),
                    "--mode".into(),
                    "glyphs".into(),
                    "--gate".into(),
                ];
                run_bench(&example(&examples, "ocr_bench"), &args, "gate (CER")
            }
        };
        self.record(4, "ocr_bench --gate", t, result)
    }

    /// Stage 4b: the `perf_` tests in release, against the baseline's times.
    pub(super) fn stage4b(&mut self) -> bool {
        let stores = [
            "crates/engine/src/chess/corrections.rs",
            "crates/engine/src/chess/reference_store.rs",
            "crates/engine/src/chess/store.rs",
            "crates/ui-egui/src/chess_autosave.rs",
        ];
        if !self.walker_changed() && !self.changed.iter().any(|f| stores.contains(&f.as_str())) {
            eprintln!("chess-gate: neither the walker, the vision, the stores nor the autosave changed");
            return true;
        }
        let base = load_perf(&self.gate_dir);
        let engine = self.crates().iter().any(|(dir, _)| Self::layer(dir) >= 6);
        let runs = perf_runs(&self.root, engine);
        if runs.is_empty() {
            let t = Instant::now();
            return self.record(4, "perf", t, Ok("no perf_ test yet in the crates it covers".to_owned()));
        }
        for (name, args) in runs {
            let t = Instant::now();
            let result = super::cargo_output(&self.root, &args, &self.test_env()).and_then(|out| {
                let run = super::parse_test_run(name, &out);
                if run.tests.is_empty() && run.crashed.is_empty() {
                    return Ok("no perf_ test yet".to_owned());
                }
                super::judge(&run, &self.known, name)?;
                perf_judge(&perf_lines(&out), base.as_ref())
            });
            if !self.record(4, &format!("perf {name}"), t, result) {
                return false;
            }
        }
        true
    }

    /// Stage 5 (`--full`): the 38 books against the baseline's run, and the fuzz.
    pub(super) fn stage5(&mut self) -> bool {
        if !self.full || self.no_bench {
            eprintln!("chess-gate: stage 5 runs with --full");
            return true;
        }
        let t = Instant::now();
        let head = super::git(&self.root, &["rev-parse", "HEAD"]).map(|h| h.trim().to_owned()).unwrap_or_default();
        let result = match Paths::load(&self.gate_dir).bench38 {
            None => Err(format!("no 38 books in {}: give `--bench38 DIR` to `--baseline --full`", self.gate_dir.join("paths.toml").display())),
            Some(dir) if !dir.join("out").join(BENCH38_BASE).is_dir() => {
                Err(format!("{} is missing: make it with `cargo xtask chess-gate --baseline --full`", dir.join("out").join(BENCH38_BASE).display()))
            }
            Some(dir) => {
                let mut args = vec!["--dir".to_owned(), dir.display().to_string(), "--tag".into(), format!("gate-{}", super::short(&head))];
                args.extend(strings(&["--baseline", BENCH38_BASE, "--allow-lost", "0"]));
                if let Some(m) = &self.models {
                    args.extend(["--models".to_owned(), m.display().to_string()]);
                }
                crate::chess_bench::run(&args).map(|()| String::new()).map_err(|e| format!("{e:#}"))
            }
        };
        if !self.record(5, "chess-bench (38 books)", t, result) {
            return false;
        }
        let t = Instant::now();
        // With the models, the runs read with them and with the hostile ones (plan A9-Q3a).
        let mut args = strings(&["--time", "300"]);
        if let Some(m) = &self.models {
            args.extend(["--models".to_owned(), m.display().to_string()]);
        }
        let result = crate::fuzz::run(&args).map(|()| String::new()).map_err(|e| format!("{e:#}"));
        self.record(5, "fuzz (300 s)", t, result)
    }
}

/// `cargo xtask chess-gate --baseline`: check out `reference` in `<gate>/main`, build it with
/// `<gate>/target`, and write the benches' and the `perf_` tests' baselines (and, with `full`,
/// the 38 books' run).
pub(super) fn baseline(root: &Path, gate: &Path, reference: &str, given: Paths, models: Option<&Path>, full: bool) -> anyhow::Result<()> {
    std::fs::create_dir_all(gate).with_context(|| format!("{}", gate.display()))?;
    let paths = Paths::save_with(gate, given)?;
    let sha = super::git(root, &["rev-parse", "--verify", &format!("{reference}^{{commit}}")]).with_context(|| format!("no commit {reference}"))?;
    let sha = sha.trim().to_owned();
    let tree = gate.join("main");
    if tree.join(".git").exists() {
        super::git(&tree, &["checkout", "--detach", "--force", &sha])?;
    } else {
        super::git(root, &["worktree", "add", "--detach", &tree.display().to_string(), &sha])?;
    }
    let target = gate.join("target");
    let env = vec![("CARGO_TARGET_DIR".to_owned(), Some(target.display().to_string()))];
    eprintln!("chess-gate --baseline: {reference} ({}) in {}", super::short(&sha), tree.display());
    let build = strings(&["build", "--release", "-j", "2", "-p", "pdfcraft-chessvision", "--example", "walk_bench", "--example", "reference_bench"]);
    super::cargo(&tree, &build, &env).map_err(anyhow::Error::msg)?;
    let examples = target.join("release").join("examples");
    let docs = bench_docs(gate).map_err(anyhow::Error::msg)?;
    let mut summary = json!({ "ref": reference, "sha": sha, "machine": machine(), "documents": docs.len() });
    let mut args: Vec<OsString> = docs.iter().map(|d| d.clone().into_os_string()).collect();
    args.extend(["--save-baseline".into(), gate.join("walk-baseline.json").into_os_string()]);
    summary["walk"] = json!(run_bench(&example(&examples, "walk_bench"), &args, "").map_err(|e| anyhow::anyhow!("walk_bench: {e}"))?);
    for (doc, pgn, name) in reference_pairs(&docs) {
        let args: Vec<OsString> = vec![doc.into_os_string(), pgn.into_os_string(), "--save-baseline".into(), gate.join(&name).into_os_string()];
        summary[name.as_str()] =
            json!(run_bench(&example(&examples, "reference_bench"), &args, "").map_err(|e| anyhow::anyhow!("reference_bench: {e}"))?);
    }
    let mut times = BTreeMap::new();
    for (name, args) in perf_runs(&tree, true) {
        let out = super::cargo_output(&tree, &args, &env).map_err(anyhow::Error::msg)?;
        let run = super::parse_test_run(name, &out);
        if run.tests.values().any(|passed| !passed) || !run.crashed.is_empty() {
            bail!("perf {name}: a perf_ test fails on {reference}");
        }
        times.extend(perf_lines(&out));
    }
    let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let perf = json!({ "ref": reference, "sha": sha, "at": at, "machine": machine(), "tests": times });
    std::fs::write(gate.join("perf-baseline.json"), serde_json::to_string_pretty(&perf)?)?;
    summary["perf"] = json!(times.len());
    if full {
        let dir = paths.bench38.context("--full needs the 38 books: --bench38 DIR")?;
        super::cargo(&tree, &strings(&["build", "--release", "-j", "2", "-p", "pdfcraft-cli"]), &env).map_err(anyhow::Error::msg)?;
        let cli = target.join("release").join(format!("pdfcraft-cli{}", std::env::consts::EXE_SUFFIX));
        let mut args =
            vec!["--dir".to_owned(), dir.display().to_string(), "--tag".into(), BENCH38_BASE.into(), "--cli".into(), cli.display().to_string()];
        if let Some(m) = models {
            args.extend(["--models".to_owned(), m.display().to_string()]);
        }
        crate::chess_bench::run(&args)?;
        summary["bench38"] = json!(BENCH38_BASE);
    }
    std::fs::write(gate.join("baseline.json"), serde_json::to_string_pretty(&summary)?)?;
    println!("chess-gate --baseline: written in {} ({})", gate.display(), super::short(&sha));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perf_lines_are_read_and_judged_against_twice_the_baseline() {
        let out = "running 2 tests\nPERF perf_assign 12.5\nPERF perf_walk nan\nnoise PERF x 1\nPERF perf_store 3\n";
        let times = perf_lines(out);
        assert_eq!(times.into_iter().collect::<Vec<_>>(), [("perf_assign".to_owned(), 12.5), ("perf_store".to_owned(), 3.0)]);
        let base: BTreeMap<String, f64> = [("perf_assign".to_owned(), 6.0), ("perf_store".to_owned(), 1.0)].into();
        let now: BTreeMap<String, f64> = [("perf_assign".to_owned(), 12.0), ("perf_store".to_owned(), 2.5)].into();
        let err = perf_judge(&now, Some(&base)).unwrap_err();
        assert!(err.contains("perf_store") && !err.contains("perf_assign"), "{err}");
        assert!(perf_judge(&now, None).unwrap().contains("no baseline"));
    }

    #[test]
    fn a_bench_shows_its_verdict_first() {
        let walked = "a.json: 3 moves\n  against the baseline: +0 -1\nb.json: 2 moves\nagainst the baseline: +0 -1 (allowed: 1)\n";
        assert_eq!(bench_result(walked, true, "against the baseline:"), Ok("against the baseline: +0 -1 (allowed: 1)".to_owned()));
        assert!(bench_result(walked, true, "").unwrap().starts_with("a.json"), "no verdict asked: the last lines");
        assert!(bench_result("a.json: 3 moves\n", true, "gate (CER").unwrap().starts_with("a.json"), "no verdict printed: the last lines");
        let failed = "a.json: 3 moves\nError: \"1 moves valid in the baseline are not now (allowed: 0)\"\n\n";
        let err = bench_result(failed, false, "against the baseline:").unwrap_err();
        assert!(err.starts_with("Error: \"1 moves valid"), "{err}");
        assert!(err.contains("a.json: 3 moves"), "{err}");
        assert_eq!(bench_result("", false, "x"), Err("\n".to_owned()));
    }

    #[test]
    fn perf_runs_need_a_perf_test_in_the_sources() {
        let dir = std::env::temp_dir().join(format!("chess-gate-perf-{}", std::process::id()));
        let src = dir.join("crates").join("chessvision").join("src").join("doc");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("a.rs"), "fn walk() {}").unwrap();
        assert!(perf_runs(&dir, true).is_empty());
        std::fs::write(src.join("b.rs"), "#[test] #[ignore] fn perf_walk() {}").unwrap();
        assert_eq!(perf_runs(&dir, true).iter().map(|(n, _)| *n).collect::<Vec<_>>(), ["chess and chessvision"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_documents_with_a_book_pgn_are_paired() {
        let dir = std::env::temp_dir().join(format!("chess-gate-bench-{}", std::process::id()));
        let bench = dir.join("bench");
        std::fs::create_dir_all(&bench).unwrap();
        for f in ["a.json", "b.json", "b.pgn", "c.txt"] {
            std::fs::write(bench.join(f), "{}").unwrap();
        }
        let docs = bench_docs(&dir).unwrap();
        assert_eq!(docs.iter().map(|d| d.file_name().unwrap().to_string_lossy().into_owned()).collect::<Vec<_>>(), ["a.json", "b.json"]);
        let pairs = reference_pairs(&docs);
        assert_eq!(pairs.iter().map(|(_, _, n)| n.as_str()).collect::<Vec<_>>(), ["reference-b.json"]);
        assert!(bench_docs(&dir.join("none")).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
