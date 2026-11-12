//! `cargo xtask chess-gate`: the chess fork's one local gate (plan A9-Q1a; the fork has no CI).
//!
//! It looks at what changed since the merge base with `--base` (default `origin/main`: committed,
//! staged, unstaged and new files) and runs, stopping at the first stage that fails:
//!
//! 0. **Text**: no CRLF in a changed text file; `cargo fmt --check` of the changed crates.
//! 1. **Scans**, in seconds: the tools' file access (`automation/src/file_access_scan.rs`), the
//!    chess texts in every complete catalogue (`ui-egui/src/i18n/chess_scan.rs`; both files are
//!    included here as they are, so the crate's own tests run the same code), the parity
//!    manifest (no TOML 1.1 escape, so TOML 1.0 tools read it, then `xtask parity`'s
//!    validation), `xtask layers`, `xtask assets`, and `cargo test -p xtask` when xtask changed.
//! 2. **Lower crates**: clippy `-D warnings` and the tests of each changed crate below L6, and
//!    the wasm32 check of `pdfcraft-chess` and `pdfcraft-chessvision` when they changed.
//! 3. **Engine and up** (when L6+, an app or the CLI changed): `cargo check` of the CLI and the
//!    app, wasm32 of the engine and automation, clippy of the changed crates, and the tests:
//!    the engine's `chess` ones, automation's all, ui-egui's `chess*` test targets.
//!
//! Tests run with the user's real models and engine passed **only** through the test variables
//! (`PDFCRAFT_CHESS_TEST_MODELS`, `PDFCRAFT_CHESS_TEST_ENGINE`, from `--models` / `--engine` or
//! those variables), with `PDFCRAFT_MODELS` and `PDFCRAFT_CHESS_ENGINE` taken out of their
//! environment, and with `PDFCRAFT_REQUIRE_MODELS=1`, so a test that needs them fails instead of
//! skipping. `--no-models` runs without them, and the summary says so.
//!
//! A failing test passes the gate only when `xtask/known-failures.toml` lists it; a listed test
//! that passes again fails the gate (unless its kind is `flaky`), so the list never hides a fix
//! or rots. Every run writes a summary to `<target>/chess-gate.json` for the PR to cite.
//!
//! `--all` treats every chess crate as changed (a full run); `--from N` starts at stage N.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use anyhow::{Context, bail};
use serde::Deserialize;
use serde_json::json;

#[path = "../../crates/automation/src/file_access_scan.rs"]
mod file_access_scan;

#[path = "../../crates/ui-egui/src/i18n/chess_scan.rs"]
mod chess_i18n_scan;

use crate::gates::root;

/// The crates `--all` treats as changed.
const CHESS_CRATES: &[&str] = &["chess", "chessvision", "engine", "automation", "ui-egui"];

/// What a changed file belongs to.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Owner {
    /// `crates/<dir>`: the dir and the package name.
    Crate {
        dir: String,
        package: String,
    },
    /// `apps/<dir>`.
    App {
        package: String,
    },
    Xtask,
    Other,
}

/// One entry of `xtask/known-failures.toml`.
#[derive(Debug, Deserialize)]
struct Known {
    /// `<package> <test name as cargo prints it>`, or `<package> <target> (crashed)`.
    test: String,
    reason: String,
    #[allow(dead_code)]
    since: String,
    /// `upstream`, `machine` or `flaky` (a flaky test may also pass).
    kind: String,
}

#[derive(Debug, Default, Deserialize)]
struct KnownFile {
    #[serde(default)]
    failure: Vec<Known>,
}

/// The outcome of one check, for the summary.
struct Check {
    stage: u8,
    name: String,
    ok: bool,
    seconds: f64,
    detail: String,
}

struct Gate {
    root: PathBuf,
    target: PathBuf,
    owners: BTreeSet<Owner>,
    changed: Vec<String>,
    base: String,
    merge_base: String,
    models: Option<PathBuf>,
    engine: Option<PathBuf>,
    no_models: bool,
    known: Vec<Known>,
    checks: Vec<Check>,
}

pub fn run(args: &[String]) -> anyhow::Result<()> {
    let opt = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let root = root();
    let base = opt("--base").unwrap_or_else(|| "origin/main".into());
    let from: u8 = opt("--from").map_or(Ok(0), |s| s.parse()).context("--from N")?;
    let all = args.iter().any(|a| a == "--all");
    let no_models = args.iter().any(|a| a == "--no-models");
    let models = opt("--models").map(PathBuf::from).or_else(|| std::env::var_os("PDFCRAFT_CHESS_TEST_MODELS").map(PathBuf::from));
    let engine = opt("--engine").map(PathBuf::from).or_else(|| std::env::var_os("PDFCRAFT_CHESS_TEST_ENGINE").map(PathBuf::from));

    let merge_base = git(&root, &["merge-base", "HEAD", &base]).with_context(|| format!("no merge base with {base} (git fetch?)"))?;
    let merge_base = merge_base.trim().to_owned();
    let mut changed: BTreeSet<String> = git(&root, &["diff", "--name-only", &merge_base])?.lines().map(str::to_owned).collect();
    changed.extend(git(&root, &["ls-files", "--others", "--exclude-standard"])?.lines().map(str::to_owned));
    let changed: Vec<String> = changed.into_iter().filter(|f| !f.is_empty()).collect();
    let mut owners: BTreeSet<Owner> = changed.iter().map(|f| owner(&root, f)).collect();
    if all {
        for dir in CHESS_CRATES {
            owners.insert(Owner::Crate { dir: (*dir).to_owned(), package: package_name(&root.join("crates").join(dir))? });
        }
    }
    let known_text = std::fs::read_to_string(root.join("xtask/known-failures.toml")).unwrap_or_default();
    let known: KnownFile = toml::from_str(&known_text).context("xtask/known-failures.toml")?;

    let mut gate = Gate {
        target: crate::gates::target_dir(),
        root,
        owners,
        changed,
        base,
        merge_base,
        models,
        engine,
        no_models,
        known: known.failure,
        checks: Vec::new(),
    };
    eprintln!("chess-gate: {} changed file(s) since {} ({})", gate.changed.len(), gate.base, short(&gate.merge_base));
    let stages: [(u8, &str, fn(&mut Gate) -> bool); 4] =
        [(0, "text", Gate::stage0), (1, "scans", Gate::stage1), (2, "lower crates", Gate::stage2), (3, "engine and up", Gate::stage3)];
    let mut failed = None;
    for (n, name, stage) in stages {
        if n < from {
            continue;
        }
        eprintln!("\n=== chess-gate stage {n}: {name} ===");
        if !stage(&mut gate) {
            failed = Some((n, name));
            break;
        }
    }
    let summary = gate.summary(failed);
    let path = gate.target.join("chess-gate.json");
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).ok();
    }
    std::fs::write(&path, serde_json::to_string_pretty(&summary)?).with_context(|| format!("writing {}", path.display()))?;
    println!();
    for c in &gate.checks {
        println!("  [{}] stage {} {:<34} {:>7.1} s  {}", if c.ok { "ok" } else { "FAIL" }, c.stage, c.name, c.seconds, first_line(&c.detail));
    }
    match failed {
        None => {
            println!("\nchess-gate: passed ({})", path.display());
            Ok(())
        }
        Some((n, name)) => bail!("stage {n} ({name}) failed; summary in {}", path.display()),
    }
}

impl Gate {
    fn record(&mut self, stage: u8, name: &str, started: Instant, result: Result<String, String>) -> bool {
        let ok = result.is_ok();
        let detail = result.unwrap_or_else(|e| e);
        if !ok {
            eprintln!("chess-gate: {name} FAILED:\n{detail}");
        }
        self.checks.push(Check { stage, name: name.to_owned(), ok, seconds: started.elapsed().as_secs_f64(), detail });
        ok
    }

    fn crates(&self) -> Vec<(String, String)> {
        self.owners
            .iter()
            .filter_map(|o| match o {
                Owner::Crate { dir, package } => Some((dir.clone(), package.clone())),
                _ => None,
            })
            .collect()
    }

    fn layer(dir: &str) -> u8 {
        crate::layers::classify(dir).and_then(|c| c.layer()).unwrap_or(9)
    }

    /// Stage 0: CRLF and formatting.
    fn stage0(&mut self) -> bool {
        let t = Instant::now();
        let crlf = crlf_files(&self.root, &self.changed);
        let ok = self.record(0, "no CRLF in changed text files", t, if crlf.is_empty() { Ok(String::new()) } else { Err(crlf.join("\n")) });
        if !ok {
            return false;
        }
        let mut packages: Vec<String> = self
            .owners
            .iter()
            .filter_map(|o| match o {
                Owner::Crate { package, .. } | Owner::App { package } => Some(package.clone()),
                Owner::Xtask => Some("xtask".into()),
                Owner::Other => None,
            })
            .collect();
        packages.dedup();
        if packages.is_empty() {
            return true;
        }
        let t = Instant::now();
        let mut args = vec!["fmt".to_owned(), "--check".into()];
        for p in &packages {
            args.extend(["-p".into(), p.clone()]);
        }
        let result = cargo(&self.root, &args, &[]);
        self.record(0, "cargo fmt --check", t, result)
    }

    /// Stage 1: the scans, in seconds.
    fn stage1(&mut self) -> bool {
        let t = Instant::now();
        let result = file_access(&self.root);
        if !self.record(1, "tools resolve their paths", t, result) {
            return false;
        }
        let t = Instant::now();
        let result = chess_catalogues(&self.root);
        if !self.record(1, "chess texts in every catalogue", t, result) {
            return false;
        }
        let t = Instant::now();
        let result = toml10(&self.root.join("parity/acrobat-features.toml"));
        if !self.record(1, "parity manifest is TOML 1.0", t, result) {
            return false;
        }
        for (name, f) in [
            ("xtask parity", crate::parity::run as fn(&[String]) -> anyhow::Result<()>),
            ("xtask layers", crate::gates::layers),
            ("xtask assets", crate::assets::run),
        ] {
            let t = Instant::now();
            let result = f(&[]).map_err(|e| format!("{e:#}"));
            if !self.record(1, name, t, result.map(|()| String::new())) {
                return false;
            }
        }
        if self.owners.contains(&Owner::Xtask) {
            let t = Instant::now();
            let result = self.tests("xtask", &[], &[]);
            if !self.record(1, "cargo test -p xtask", t, result) {
                return false;
            }
        }
        true
    }

    /// Stage 2: the changed crates below L6.
    fn stage2(&mut self) -> bool {
        let lower: Vec<(String, String)> = self.crates().into_iter().filter(|(dir, _)| Self::layer(dir) < 6).collect();
        if lower.is_empty() {
            eprintln!("chess-gate: no crate below L6 changed");
            return true;
        }
        if !self.models_ready(2) {
            return false;
        }
        for (dir, package) in &lower {
            let t = Instant::now();
            let result = cargo(&self.root, &strings(&["clippy", "-j", "2", "-p", package, "--all-targets", "--", "-D", "warnings"]), &[]);
            if !self.record(2, &format!("clippy {package}"), t, result) {
                return false;
            }
            let t = Instant::now();
            let result = self.tests(package, &[], &[]);
            if !self.record(2, &format!("tests {package}"), t, result) {
                return false;
            }
            if dir == "chess" || dir == "chessvision" {
                let t = Instant::now();
                let result = cargo(&self.root, &strings(&["check", "-j", "2", "--target", "wasm32-unknown-unknown", "-p", package]), &[]);
                if !self.record(2, &format!("wasm32 {package}"), t, result) {
                    return false;
                }
            }
        }
        true
    }

    /// Stage 3: the engine, automation, the UI and the apps.
    fn stage3(&mut self) -> bool {
        let upper: Vec<(String, String)> = self.crates().into_iter().filter(|(dir, _)| Self::layer(dir) >= 6).collect();
        let apps = self.owners.iter().any(|o| matches!(o, Owner::App { .. }));
        if upper.is_empty() && !apps {
            eprintln!("chess-gate: nothing at L6 or above changed");
            return true;
        }
        if !self.models_ready(3) {
            return false;
        }
        let t = Instant::now();
        let result = cargo(&self.root, &strings(&["check", "-j", "2", "-p", "pdfcraft-cli", "-p", "pdfcraft"]), &[]);
        if !self.record(3, "check the CLI and the app", t, result) {
            return false;
        }
        let t = Instant::now();
        let result = cargo(
            &self.root,
            &strings(&["check", "-j", "2", "--target", "wasm32-unknown-unknown", "-p", "pdfcraft-engine", "-p", "pdfcraft-automation"]),
            &[],
        );
        if !self.record(3, "wasm32 engine and automation", t, result) {
            return false;
        }
        for (dir, package) in &upper {
            let t = Instant::now();
            let result = cargo(&self.root, &strings(&["clippy", "-j", "2", "-p", package, "--all-targets", "--", "-D", "warnings"]), &[]);
            if !self.record(3, &format!("clippy {package}"), t, result) {
                return false;
            }
            let (targets, filter): (Vec<String>, Vec<String>) = match dir.as_str() {
                "engine" => (vec!["--lib".into()], vec!["chess".into()]),
                "ui-egui" => (chess_test_targets(&self.root), Vec::new()),
                _ => (Vec::new(), Vec::new()),
            };
            let t = Instant::now();
            let result = self.tests(package, &targets, &filter);
            if !self.record(3, &format!("tests {package}"), t, result) {
                return false;
            }
        }
        true
    }

    /// Whether the models and the engine are given, or `--no-models` says to run without them.
    fn models_ready(&mut self, stage: u8) -> bool {
        if self.no_models {
            return true;
        }
        let missing: Vec<&str> =
            [("--models DIR (or PDFCRAFT_CHESS_TEST_MODELS)", &self.models), ("--engine EXE (or PDFCRAFT_CHESS_TEST_ENGINE)", &self.engine)]
                .into_iter()
                .filter(|(_, p)| !p.as_ref().is_some_and(|p| p.is_absolute() && p.exists()))
                .map(|(what, _)| what)
                .collect();
        let t = Instant::now();
        let result = if missing.is_empty() {
            Ok(String::new())
        } else {
            Err(format!(
                "the tests run with the user's models and engine: pass {} as absolute paths (a worktree has no dist/app), or --no-models",
                missing.join(" and ")
            ))
        };
        self.record(stage, "models and engine given", t, result)
    }

    /// The test run's environment: the user's models and engine only through the test variables.
    fn test_env(&self) -> Vec<(String, Option<String>)> {
        let mut env: Vec<(String, Option<String>)> = vec![("PDFCRAFT_MODELS".into(), None), ("PDFCRAFT_CHESS_ENGINE".into(), None)];
        if self.no_models {
            env.push(("PDFCRAFT_REQUIRE_MODELS".into(), None));
            env.push(("PDFCRAFT_CHESS_TEST_MODELS".into(), None));
            env.push(("PDFCRAFT_CHESS_TEST_ENGINE".into(), None));
        } else {
            env.push(("PDFCRAFT_REQUIRE_MODELS".into(), Some("1".into())));
            env.push(("PDFCRAFT_CHESS_TEST_MODELS".into(), self.models.as_ref().map(|p| p.display().to_string())));
            env.push(("PDFCRAFT_CHESS_TEST_ENGINE".into(), self.engine.as_ref().map(|p| p.display().to_string())));
        }
        env
    }

    /// `cargo test -p package [targets] -- [filter]` with `--no-fail-fast`, judged against the
    /// known failures.
    fn tests(&self, package: &str, targets: &[String], filter: &[String]) -> Result<String, String> {
        let mut args = strings(&["test", "-j", "2", "--no-fail-fast", "-p", package]);
        args.extend(targets.iter().cloned());
        if !filter.is_empty() {
            args.push("--".into());
            args.extend(filter.iter().cloned());
        }
        let out = cargo_output(&self.root, &args, &self.test_env())?;
        let run = parse_test_run(package, &out);
        judge(&run, &self.known, package)
    }

    fn summary(&self, failed: Option<(u8, &str)>) -> serde_json::Value {
        json!({
            "base": self.base,
            "merge_base": self.merge_base,
            "changed": self.changed,
            "models": if self.no_models { json!("skipped (--no-models)") } else { json!({ "models": self.models, "engine": self.engine }) },
            "passed": failed.is_none(),
            "failed_stage": failed.map(|(n, name)| json!({ "stage": n, "name": name })),
            "checks": self.checks.iter().map(|c| json!({
                "stage": c.stage, "name": c.name, "ok": c.ok, "seconds": (c.seconds * 10.0).round() / 10.0,
                "detail": c.detail.chars().take(4000).collect::<String>(),
            })).collect::<Vec<_>>(),
        })
    }
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| (*s).to_owned()).collect()
}

fn short(rev: &str) -> &str {
    rev.get(..10).unwrap_or(rev)
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or("")
}

fn git(root: &Path, args: &[&str]) -> anyhow::Result<String> {
    let out = Command::new("git").args(args).current_dir(root).output().context("git")?;
    if !out.status.success() {
        bail!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn cargo_command(root: &Path, args: &[String], env: &[(String, Option<String>)]) -> Command {
    let mut c = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    c.args(args).current_dir(root);
    for (k, v) in env {
        match v {
            Some(v) => c.env(k, v),
            None => c.env_remove(k),
        };
    }
    c
}

/// Run cargo with its output shown; Err with the command when it fails.
fn cargo(root: &Path, args: &[String], env: &[(String, Option<String>)]) -> Result<String, String> {
    eprintln!("$ cargo {}", args.join(" "));
    match cargo_command(root, args, env).status() {
        Ok(s) if s.success() => Ok(String::new()),
        Ok(s) => Err(format!("cargo {} exited with {s}", args.join(" "))),
        Err(e) => Err(format!("cargo {}: {e}", args.join(" "))),
    }
}

/// Run cargo and return its stdout and stderr together (tests print to stdout, cargo to stderr),
/// echoing nothing but the command. A run that fails is still returned: its tests are judged.
fn cargo_output(root: &Path, args: &[String], env: &[(String, Option<String>)]) -> Result<String, String> {
    eprintln!("$ cargo {}", args.join(" "));
    let out = cargo_command(root, args, env).output().map_err(|e| format!("cargo {}: {e}", args.join(" ")))?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push('\n');
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    if !out.status.success() && !text.contains("test result:") && !text.contains("Running ") {
        return Err(format!("cargo {} failed before running tests:\n{}", args.join(" "), tail(&text, 40)));
    }
    Ok(text)
}

fn tail(text: &str, lines: usize) -> String {
    let all: Vec<&str> = text.lines().collect();
    all.get(all.len().saturating_sub(lines)..).unwrap_or(&[]).join("\n")
}

fn owner(root: &Path, file: &str) -> Owner {
    let parts: Vec<&str> = file.split('/').collect();
    match parts.as_slice() {
        ["crates", dir, _, ..] => match package_name(&root.join("crates").join(dir)) {
            Ok(package) => Owner::Crate { dir: (*dir).to_owned(), package },
            Err(_) => Owner::Other,
        },
        ["apps", dir, _, ..] => match package_name(&root.join("apps").join(dir)) {
            Ok(package) => Owner::App { package },
            Err(_) => Owner::Other,
        },
        ["xtask", _, ..] => Owner::Xtask,
        _ => Owner::Other,
    }
}

/// The `name` of the package whose `Cargo.toml` is in `dir`.
fn package_name(dir: &Path) -> anyhow::Result<String> {
    let text = std::fs::read_to_string(dir.join("Cargo.toml")).with_context(|| format!("{}/Cargo.toml", dir.display()))?;
    #[derive(Deserialize)]
    struct Manifest {
        package: Package,
    }
    #[derive(Deserialize)]
    struct Package {
        name: String,
    }
    let m: Manifest = toml::from_str(&text).with_context(|| format!("{}/Cargo.toml", dir.display()))?;
    Ok(m.package.name)
}

/// Changed files that exist, are text (no NUL byte, not marked binary by extension) and hold a
/// CRLF line ending. `.gitattributes` sets `* -text`, so what is written is what is committed.
fn crlf_files(root: &Path, changed: &[String]) -> Vec<String> {
    const BINARY: &[&str] = &["pdf", "p12", "pfb", "ttf", "otf", "icc", "icns", "ico", "png", "jpg", "jpeg", "brotli", "woff", "woff2", "onnx"];
    changed
        .iter()
        .filter(|f| !Path::new(f).extension().and_then(|e| e.to_str()).is_some_and(|e| BINARY.contains(&e.to_ascii_lowercase().as_str())))
        .filter_map(|f| {
            let bytes = std::fs::read(root.join(f)).ok()?;
            (!bytes.contains(&0) && bytes.windows(2).any(|w| w == b"\r\n")).then(|| format!("{f}: CRLF line endings (the repository keeps LF)"))
        })
        .collect()
}

/// The file-access scan over `crates/automation/src/`.
fn file_access(root: &Path) -> Result<String, String> {
    let dir = root.join("crates/automation/src");
    let mut offenders = Vec::new();
    let entries = std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if !file_access_scan::scanned(&name) {
            continue;
        }
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        offenders.extend(file_access_scan::offenders(&format!("crates/automation/src/{name}"), &text));
    }
    if offenders.is_empty() { Ok(String::new()) } else { Err(format!("resolve the path with `self.resolve` first:\n{}", offenders.join("\n"))) }
}

/// The chess texts of `crates/ui-egui/src/chess*.rs` in every catalogue complete for chess.
fn chess_catalogues(root: &Path) -> Result<String, String> {
    let src = root.join("crates/ui-egui/src");
    let mut literals = BTreeSet::new();
    let entries = std::fs::read_dir(&src).map_err(|e| format!("{}: {e}", src.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if chess_i18n_scan::is_chess_source(&name) {
            let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            literals.extend(chess_i18n_scan::tl_literals(&text.replace("\r\n", "\n")));
        }
    }
    let i18n = src.join("i18n");
    let mut catalogues = Vec::new();
    let entries = std::fs::read_dir(&i18n).map_err(|e| format!("{}: {e}", i18n.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "tsv") {
            let code = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            catalogues.push((code, std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?));
        }
    }
    let missing = chess_i18n_scan::missing(&literals, &catalogues);
    if missing.is_empty() {
        Ok(format!("{} chess texts", literals.len()))
    } else {
        let lines: Vec<String> =
            missing.iter().map(|(code, texts)| format!("{code}.tsv lacks {}: {:?}", texts.len(), texts.iter().take(5).collect::<Vec<_>>())).collect();
        Err(format!(
            "add each new chess text to every catalogue (or to the four of chess_scan::INCOMPLETE only through A9-I14):\n{}",
            lines.join("\n")
        ))
    }
}

/// A TOML 1.1 escape (`\xHH`, `\e`) in a basic string: TOML 1.0 tools (Python's tomllib, many
/// editors) refuse the whole file. Each backslash escape is read in order, so `\\x` is not one.
fn toml10(path: &Path) -> Result<String, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut bad = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let mut chars = line.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('x') | Some('e') => bad.push(format!("{}:{}: a TOML 1.1 escape (write the character itself)", path.display(), n + 1)),
                    _ => {}
                }
            }
        }
    }
    if bad.is_empty() { Ok(String::new()) } else { Err(bad.join("\n")) }
}

/// ui-egui's integration test targets whose file name starts with `chess`, as `--test` args.
fn chess_test_targets(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(root.join("crates/ui-egui/tests"))
        .map(|d| {
            d.flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    name.strip_prefix("chess").is_some().then_some(())?;
                    name.strip_suffix(".rs").map(str::to_owned)
                })
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names.into_iter().flat_map(|n| ["--test".to_owned(), n]).collect()
}

/// What one `cargo test` printed: each test's name and whether it passed, and the targets whose
/// process died.
#[derive(Debug, Default, PartialEq)]
struct TestRun {
    /// `<package> <name>` → passed.
    tests: BTreeMap<String, bool>,
    /// `<package> <target> (crashed)`, the target as cargo's rerun hint names it (`--test content`).
    crashed: Vec<String>,
}

/// Read a `cargo test` run. Tests print `test <name> ... ok|FAILED` (stdout). A target that fails
/// makes cargo print ``error: test failed, to rerun pass `<target>` `` and then why (stderr,
/// within a few lines): exit code 101 is failed tests, already counted; any other (an access
/// violation, a stack overflow) is a process that died, which prints no `FAILED` line for the
/// test that killed it. stdout and stderr arrive apart, so nothing here depends on their order.
fn parse_test_run(package: &str, out: &str) -> TestRun {
    let mut run = TestRun::default();
    let lines: Vec<&str> = out.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let l = line.trim();
        if let Some(rest) = l.strip_prefix("test ") {
            if let Some(name) = rest.strip_suffix(" ... ok") {
                run.tests.insert(format!("{package} {name}"), true);
            } else if let Some(name) = rest.strip_suffix(" ... FAILED") {
                run.tests.insert(format!("{package} {name}"), false);
            }
        } else if let Some(rest) = l.strip_prefix("error: test failed, to rerun pass `") {
            let target = rest.split('`').next().unwrap_or(rest);
            let why = lines.get(i + 1..(i + 5).min(lines.len())).unwrap_or(&[]).join(" ");
            let died = why.contains("exit code") && !why.contains("exit code: 101");
            let crashed = format!("{package} {target} (crashed)");
            if died && !run.crashed.contains(&crashed) {
                run.crashed.push(crashed);
            }
        }
    }
    run
}

/// Fail on a failure the list does not hold and on a listed (non-flaky) test that passed.
fn judge(run: &TestRun, known: &[Known], package: &str) -> Result<String, String> {
    let listed = |name: &str| known.iter().find(|k| k.test == name);
    let mut problems = Vec::new();
    let mut tolerated = Vec::new();
    for (name, passed) in &run.tests {
        match (passed, listed(name)) {
            (false, None) => problems.push(format!("FAILED: {name}")),
            (false, Some(k)) => tolerated.push(format!("{name} ({})", k.reason)),
            (true, Some(k)) if k.kind != "flaky" => {
                problems.push(format!("passes again, take it out of xtask/known-failures.toml: {name}"));
            }
            _ => {}
        }
    }
    for name in &run.crashed {
        match listed(name) {
            None => problems.push(format!("CRASHED: {name}")),
            Some(k) => tolerated.push(format!("{name} ({})", k.reason)),
        }
    }
    let passed = run.tests.values().filter(|p| **p).count();
    if run.tests.is_empty() && run.crashed.is_empty() {
        return Err(format!("{package}: no test ran"));
    }
    if problems.is_empty() {
        Ok(format!("{passed} passed, {} known failure(s) tolerated: {}", tolerated.len(), tolerated.join("; ")))
    } else {
        Err(problems.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known(test: &str, kind: &str) -> Known {
        Known { test: test.into(), reason: "why".into(), since: "2026-11-11".into(), kind: kind.into() }
    }

    #[test]
    fn a_test_run_is_read_test_by_test() {
        let out = "     Running unittests src/lib.rs (target/debug/deps/x.exe)\n\
                   test a::ok_one ... ok\n\
                   test a::bad_one ... FAILED\n\
                   test result: FAILED. 1 passed; 1 failed\n";
        let run = parse_test_run("pdfcraft-x", out);
        assert_eq!(run.tests.get("pdfcraft-x a::ok_one"), Some(&true));
        assert_eq!(run.tests.get("pdfcraft-x a::bad_one"), Some(&false));
        assert!(run.crashed.is_empty());
    }

    #[test]
    fn a_crashed_target_is_named() {
        let out = "test a ... ok\n\
                   error: test failed, to rerun pass `--test content`\n\
                   \n\
                   Caused by:\n\
                   process didn't exit successfully: `content.exe` (exit code: 0xc0000005, STATUS_ACCESS_VIOLATION)\n\
                   error: test failed, to rerun pass `--lib`\n\
                   \n\
                   Caused by:\n\
                   process didn't exit successfully: `lib.exe` (exit code: 101)\n";
        let run = parse_test_run("pdfcraft-ui-egui", out);
        assert_eq!(run.crashed, ["pdfcraft-ui-egui --test content (crashed)"], "exit code 101 is failed tests, not a crash");
    }

    #[test]
    fn the_known_failures_decide_and_never_hide_a_fix() {
        let mut run = TestRun::default();
        run.tests.insert("p a".into(), false);
        run.tests.insert("p b".into(), true);
        assert!(judge(&run, &[], "p").unwrap_err().contains("FAILED: p a"));
        assert!(judge(&run, &[known("p a", "upstream")], "p").is_ok());
        let fixed = judge(&run, &[known("p a", "upstream"), known("p b", "upstream")], "p").unwrap_err();
        assert!(fixed.contains("passes again") && fixed.contains("p b"), "{fixed}");
        assert!(judge(&run, &[known("p a", "upstream"), known("p b", "flaky")], "p").is_ok(), "a flaky test may pass");
        assert!(judge(&TestRun::default(), &[], "p").unwrap_err().contains("no test ran"));
    }

    #[test]
    fn toml_1_1_escapes_are_found_and_doubled_backslashes_are_not() {
        let dir = std::env::temp_dir().join(format!("chess-gate-toml-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("p.toml");
        std::fs::write(&f, "a = \"it\\x27s\"\nb = \"C:\\\\x\"\nc = \"tab\\t\"\n").unwrap();
        let err = toml10(&f).unwrap_err();
        assert!(err.contains(":1:") && !err.contains(":2:") && !err.contains(":3:"), "{err}");
        std::fs::write(&f, "a = \"it's\"\n").unwrap();
        assert!(toml10(&f).is_ok());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn crlf_is_found_in_text_and_ignored_in_binaries() {
        let dir = std::env::temp_dir().join(format!("chess-gate-crlf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.rs"), "fn a() {}\r\n").unwrap();
        std::fs::write(dir.join("b.rs"), "fn b() {}\n").unwrap();
        std::fs::write(dir.join("c.png"), b"\r\n\0").unwrap();
        let found = crlf_files(&dir, &["a.rs".into(), "b.rs".into(), "c.png".into(), "gone.rs".into()]);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].starts_with("a.rs"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_committed_tree_passes_the_scans() {
        let root = root();
        file_access(&root).unwrap();
        chess_catalogues(&root).unwrap();
        toml10(&root.join("parity/acrobat-features.toml")).unwrap();
        let known: KnownFile = toml::from_str(&std::fs::read_to_string(root.join("xtask/known-failures.toml")).unwrap()).unwrap();
        assert!(known.failure.iter().all(|k| matches!(k.kind.as_str(), "upstream" | "machine" | "flaky") && !k.reason.is_empty()));
    }
}
