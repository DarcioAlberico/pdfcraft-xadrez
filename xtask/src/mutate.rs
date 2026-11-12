//! `cargo xtask mutate <file.rs> [--lines N,N,…]`: mutation testing of one Rust source file
//! (plan A9-Q4a, A7-07), for the steps that touch pure rules (`rules`, `san`, `nag`, the PGN
//! reader) and promise ≥ 90 % of mutants killed.
//!
//! It makes the mutations the `mutator` tool makes in Rust (the Java set: `==`/`!=`, `<`/`<=`,
//! `>`/`>=`, `&&`/`||`, `+`/`-`, `*`/`/`, `true`/`false`, `0`/`1`, a `!` deleted), one at a time,
//! and runs the crate's tests on each: a mutant the tests fail on (or that does not compile, or
//! times out) is killed; one they pass on survived. The run is the Windows-safe form of that tool:
//!
//! - the workspace is **copied** (the files git tracks, and new ones it does not ignore) to
//!   `<target>/mutate/ws`, never linked (symlinks need a privilege Windows users rarely have),
//!   and only the copy is ever mutated: the tree you work in is never written;
//! - a test run past its time is killed **with its whole process tree** (`taskkill /T /F` on
//!   Windows, where there are no process groups);
//! - the copy builds into `<target>/mutate/target`, so after the first run each mutant rebuilds
//!   only the crate it is in.
//!
//! Binary operators are mutated only where rustfmt spaces them (` < `), so a generic's `<` and a
//! macro's `!` are left alone; strings, chars, comments and the `#[cfg(test)]` module at the end
//! of the file are never mutated. `--lines` keeps the sites on those lines (1-based); `--list`
//! prints the sites and runs nothing; `--test-command "…"` replaces the default
//! `cargo test -p <crate> --lib -j 2`. The timeout is ten times the baseline run and at least
//! `--min-timeout` seconds (240: each mutant recompiles the crate). Survivors fail the command.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, bail};

use crate::gates::{root, target_dir};

/// One mutation: where (1-based line, byte range in the file) and what it becomes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Site {
    pub line: usize,
    pub start: usize,
    pub end: usize,
    pub from: &'static str,
    pub to: &'static str,
}

/// The binary operators mutated where rustfmt writes them, spaced: ` == ` and the others.
const BINARY: &[(&str, &str)] = &[
    ("==", "!="),
    ("!=", "=="),
    ("<=", "<"),
    (">=", ">"),
    ("<", "<="),
    (">", ">="),
    ("&&", "||"),
    ("||", "&&"),
    ("+", "-"),
    ("-", "+"),
    ("*", "/"),
    ("/", "*"),
];

/// What a byte of the source is part of, for the scan.
fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// The mutation sites of `text`, in order, before its `#[cfg(test)]` module.
pub fn sites(text: &str) -> Vec<Site> {
    let bytes = text.as_bytes();
    let end_of_code = text.find("\n#[cfg(test)]").map_or(text.len(), |i| i.saturating_add(1));
    let mut out = Vec::new();
    let (mut i, mut line) = (0usize, 1usize);
    while i < end_of_code {
        let Some(&b) = bytes.get(i) else { break };
        let rest = text.get(i..).unwrap_or_default();
        // Skipped: comments, strings, chars; each moves `i` past itself, counting lines.
        let skip = if rest.starts_with("//") {
            rest.find('\n').unwrap_or(rest.len())
        } else if rest.starts_with("/*") {
            block_comment_len(rest)
        } else if let Some(n) = string_len(rest, i.checked_sub(1).and_then(|p| bytes.get(p)).copied()) {
            n
        } else if b == b'\'' {
            char_len(rest)
        } else {
            0
        };
        if skip > 0 {
            line = line.saturating_add(rest.get(..skip).unwrap_or_default().matches('\n').count());
            i = i.saturating_add(skip);
            continue;
        }
        if b == b'\n' {
            line = line.saturating_add(1);
        }
        let prev = i.checked_sub(1).and_then(|p| bytes.get(p)).copied();
        if let Some(site) = site_at(text, i, line, prev) {
            i = site.end;
            out.push(site);
            continue;
        }
        i = i.saturating_add(1);
    }
    out
}

/// The site starting at byte `i`, if one does.
fn site_at(text: &str, i: usize, line: usize, prev: Option<u8>) -> Option<Site> {
    let rest = text.get(i..)?;
    let site = |len: usize, from: &'static str, to: &'static str| Some(Site { line, start: i, end: i.saturating_add(len), from, to });
    // A spaced binary operator: ` op ` (the longest first: `<=` before `<`).
    if prev == Some(b' ') {
        for (from, to) in BINARY {
            if rest.starts_with(from) && rest.as_bytes().get(from.len()) == Some(&b' ') {
                return site(from.len(), from, to);
            }
        }
    }
    let word_start = !prev.is_some_and(is_ident);
    let word_end = |len: usize| !rest.as_bytes().get(len).is_some_and(|&b| is_ident(b));
    if word_start {
        for (from, to) in [("true", "false"), ("false", "true")] {
            if rest.starts_with(from) && word_end(from.len()) {
                return site(from.len(), from, to);
            }
        }
        // `0` and `1` alone: not part of a number (`10`, `1.5`, `0x1`, `0u8`), but before `..`.
        for (from, to) in [("0", "1"), ("1", "0")] {
            let after = rest.as_bytes().get(1).copied();
            let fraction = after == Some(b'.') && rest.as_bytes().get(2).is_some_and(u8::is_ascii_digit);
            if rest.starts_with(from) && word_end(1) && !fraction && prev != Some(b'.') {
                return site(1, from, to);
            }
        }
        // A logical not: `!x`, `!(…)`; not `!=`, not a macro's `name!`.
        if rest.starts_with('!') && !rest.starts_with("!=") {
            return site(1, "!", "");
        }
    }
    None
}

/// The length of a `/* … */` comment (nested ones included) at the start of `rest`.
fn block_comment_len(rest: &str) -> usize {
    let mut depth = 0usize;
    let mut i = 0usize;
    while let Some(r) = rest.get(i..).filter(|r| !r.is_empty()) {
        if r.starts_with("/*") {
            depth = depth.saturating_add(1);
            i = i.saturating_add(2);
        } else if r.starts_with("*/") {
            depth = depth.saturating_sub(1);
            i = i.saturating_add(2);
            if depth == 0 {
                return i;
            }
        } else {
            i = i.saturating_add(r.chars().next().map_or(1, char::len_utf8));
        }
    }
    rest.len()
}

/// The length of a string literal at the start of `rest` (`"…"`, `b"…"`, `r#"…"#`), or `None`.
fn string_len(rest: &str, prev: Option<u8>) -> Option<usize> {
    if prev.is_some_and(is_ident) {
        return None;
    }
    let body = rest.strip_prefix('b').unwrap_or(rest);
    let prefix = rest.len().saturating_sub(body.len());
    if let Some(raw) = body.strip_prefix('r') {
        let hashes = raw.len().saturating_sub(raw.trim_start_matches('#').len());
        let after = raw.get(hashes..)?;
        if !after.starts_with('"') {
            return None;
        }
        let close = format!("\"{}", "#".repeat(hashes));
        let end = after.get(1..)?.find(&close)?;
        return Some(prefix.saturating_add(1).saturating_add(hashes).saturating_add(1).saturating_add(end).saturating_add(close.len()));
    }
    let inner = body.strip_prefix('"')?;
    let mut escaped = false;
    for (k, c) in inner.char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            '"' => return Some(prefix.saturating_add(1).saturating_add(k).saturating_add(1)),
            _ => {}
        }
    }
    Some(rest.len())
}

/// The length of a char literal (`'x'`, `'\n'`, `'\u{1}'`) at the start of `rest`, or 1 for a
/// lifetime's `'`.
fn char_len(rest: &str) -> usize {
    let inner = rest.get(1..).unwrap_or_default();
    if let Some(escaped) = inner.strip_prefix('\\') {
        return escaped.find('\'').map_or(1, |k| k.saturating_add(3));
    }
    let mut chars = inner.chars();
    match (chars.next(), chars.next()) {
        (Some(c), Some('\'')) => c.len_utf8().saturating_add(2),
        _ => 1,
    }
}

/// `text` with `site` applied.
fn mutated(text: &str, site: &Site) -> String {
    format!("{}{}{}", text.get(..site.start).unwrap_or_default(), site.to, text.get(site.end..).unwrap_or_default())
}

/// How a mutant fared.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    Killed,
    Survived,
    TimedOut,
}

pub fn run(args: &[String]) -> anyhow::Result<()> {
    let mut file: Option<String> = None;
    let mut lines: Option<BTreeSet<usize>> = None;
    let mut command: Option<String> = None;
    let mut list = false;
    let mut min_timeout = 240u64;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--lines" => {
                let given = it.next().context("--lines takes N,N,…")?;
                lines = Some(
                    given.split(',').map(|n| n.trim().parse::<usize>().with_context(|| format!("--lines: {n:?}"))).collect::<anyhow::Result<_>>()?,
                );
            }
            "--test-command" => command = Some(it.next().context("--test-command takes a command")?.clone()),
            "--min-timeout" => min_timeout = it.next().context("--min-timeout takes seconds")?.parse().context("--min-timeout")?,
            "--list" => list = true,
            other if other.starts_with("--") => bail!("unknown option {other}"),
            other => file = Some(other.replace('\\', "/")),
        }
    }
    let file = file.context("usage: cargo xtask mutate <file.rs> [--lines N,N,…] [--list] [--test-command CMD] [--min-timeout S]")?;
    let root = root();
    let text = std::fs::read_to_string(root.join(&file)).with_context(|| file.clone())?;
    let all = sites(&text);
    let chosen: Vec<Site> = all.into_iter().filter(|s| lines.as_ref().is_none_or(|l| l.contains(&s.line))).collect();
    if list || chosen.is_empty() {
        for s in &chosen {
            println!("{file}:{} {} -> {}", s.line, s.from, if s.to.is_empty() { "(deleted)" } else { s.to });
        }
        println!("{} site(s)", chosen.len());
        return Ok(());
    }
    let command = match command {
        Some(c) => c,
        None => format!("cargo test -p {} --lib -j 2", package_of(&root, &file)?),
    };
    let work = target_dir().join("mutate");
    let ws = work.join("ws");
    copy_workspace(&root, &ws)?;
    let env_target = work.join("target");
    eprintln!("mutate: baseline `{command}` in {}", ws.display());
    let started = Instant::now();
    if run_once(&command, &ws, &env_target, None)? != Outcome::Survived {
        bail!("the baseline tests fail in the copy: fix them first");
    }
    let timeout = Duration::from_secs((started.elapsed().as_secs().saturating_mul(10)).max(min_timeout));
    let target_file = ws.join(&file);
    let mut results = Vec::new();
    for (n, site) in chosen.iter().enumerate() {
        std::fs::write(&target_file, mutated(&text, site)).with_context(|| target_file.display().to_string())?;
        let t = Instant::now();
        let outcome = run_once(&command, &ws, &env_target, Some(timeout));
        std::fs::write(&target_file, &text).with_context(|| target_file.display().to_string())?;
        let outcome = outcome?;
        let to = if site.to.is_empty() { "(deleted)" } else { site.to };
        println!(
            "{:<8} {file}:{} {} -> {to} ({}/{}, {:.0} s)",
            format!("{outcome:?}").to_uppercase(),
            site.line,
            site.from,
            n + 1,
            chosen.len(),
            t.elapsed().as_secs_f64()
        );
        results.push((site.clone(), outcome));
    }
    let survived: Vec<&(Site, Outcome)> = results.iter().filter(|(_, o)| *o == Outcome::Survived).collect();
    let killed = results.len().saturating_sub(survived.len());
    let score = if results.is_empty() { 100.0 } else { killed as f64 * 100.0 / results.len() as f64 };
    println!("\nmutate {file}: {killed} of {} killed ({score:.1} %), {} survived", results.len(), survived.len());
    for (s, _) in &survived {
        println!("  SURVIVED {file}:{} {} -> {}", s.line, s.from, if s.to.is_empty() { "(deleted)" } else { s.to });
    }
    if !survived.is_empty() {
        bail!("{} mutant(s) survived", survived.len());
    }
    Ok(())
}

/// The package of the crate `file` is in (`crates/<dir>/…`).
fn package_of(root: &Path, file: &str) -> anyhow::Result<String> {
    let mut parts = file.split('/');
    let (Some(top), Some(dir)) = (parts.next(), parts.next()) else { bail!("{file}: not in a crate") };
    let manifest = root.join(top).join(dir).join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest).with_context(|| manifest.display().to_string())?;
    let value: toml::Value = toml::from_str(&text).with_context(|| manifest.display().to_string())?;
    value
        .get("package")
        .and_then(|p| p.get("name"))
        .and_then(toml::Value::as_str)
        .map(str::to_owned)
        .with_context(|| format!("{}: no package name", manifest.display()))
}

/// Copy the files git tracks, and the new ones it does not ignore, from `root` to `ws` (files
/// that are already there with the same size and contents are left).
fn copy_workspace(root: &Path, ws: &Path) -> anyhow::Result<()> {
    let out = Command::new("git")
        .args(["ls-files", "--cached", "--others", "--exclude-standard", "-z"])
        .current_dir(root)
        .output()
        .context("git ls-files")?;
    if !out.status.success() {
        bail!("git ls-files: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    let listed = String::from_utf8_lossy(&out.stdout).into_owned();
    let mut copied = 0usize;
    for rel in listed.split('\0').filter(|f| !f.is_empty()) {
        let from = root.join(rel);
        if !from.is_file() {
            continue;
        }
        let to = ws.join(rel);
        let same = std::fs::read(&to).ok().zip(std::fs::read(&from).ok()).is_some_and(|(a, b)| a == b);
        if same {
            continue;
        }
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).with_context(|| parent.display().to_string())?;
        }
        std::fs::copy(&from, &to).with_context(|| format!("{} -> {}", from.display(), to.display()))?;
        copied += 1;
    }
    eprintln!("mutate: workspace copy in {} ({copied} file(s) updated)", ws.display());
    Ok(())
}

/// Run `command` in `dir` with its own target directory; killed (with its process tree) past
/// `timeout`. Survived when it passed.
fn run_once(command: &str, dir: &Path, target: &PathBuf, timeout: Option<Duration>) -> anyhow::Result<Outcome> {
    let mut words = command.split_whitespace();
    let program = words.next().context("an empty test command")?;
    let mut child = Command::new(program)
        .args(words)
        .current_dir(dir)
        .env("CARGO_TARGET_DIR", target)
        .env("CARGO_INCREMENTAL", "0")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("run {command}"))?;
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(if status.success() { Outcome::Survived } else { Outcome::Killed });
        }
        if timeout.is_some_and(|t| started.elapsed() > t) {
            kill_tree(&mut child);
            return Ok(Outcome::TimedOut);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Kill `child` and everything it started: `taskkill /T /F` on Windows, which has no process
/// groups for a plain kill to reach.
fn kill_tree(child: &mut std::process::Child) {
    if cfg!(windows) {
        let _ = Command::new("taskkill").args(["/T", "/F", "/PID", &child.id().to_string()]).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(text: &str) -> Vec<String> {
        sites(text).iter().map(|s| format!("{}:{}>{}", s.line, s.from, s.to)).collect()
    }

    #[test]
    fn spaced_operators_words_and_digits_are_sites() {
        let text = "fn f(a: u8, b: bool) -> bool {\n    a == 1 && !b || a <= 0\n}\n";
        assert_eq!(named(text), ["2:==>!=", "2:1>0", "2:&&>||", "2:!>", "2:||>&&", "2:<=><", "2:0>1"]);
    }

    #[test]
    fn generics_macros_strings_chars_comments_and_tests_are_left_alone() {
        let text = "fn f() -> Vec<u8> {\n    // a == b\n    /* 1 < 2 */\n    let s = \"a == 1\";\n    let c = '!';\n    let r = r#\"x < 1\"#;\n    assert!(s.len() > 10, \"{}\", 1.5);\n    vec![0u8, 10]\n}\n#[cfg(test)]\nmod tests { fn g() { 1 == 1; } }\n";
        assert_eq!(named(text), ["7:>>>="]);
    }

    #[test]
    fn a_range_start_is_a_site_and_a_lifetime_is_no_char() {
        assert_eq!(named("fn f<'a>(x: &'a [u8]) { for i in 0..x.len() {} }\n"), ["1:0>1"]);
    }

    #[test]
    fn a_mutation_replaces_its_bytes_only() {
        let text = "a == 1";
        let s = &sites(text)[0];
        assert_eq!(mutated(text, s), "a != 1");
    }
}
