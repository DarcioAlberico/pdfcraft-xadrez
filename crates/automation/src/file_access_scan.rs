//! The file-access scan of the tools' source (plan A9-Q1a): every file a tool reads or writes
//! goes through `Automation::resolve`, which holds it to the roots, so each use of `std::fs`,
//! `File::open`, `File::create` or `OpenOptions` must come after a `resolve` call **in code** in
//! the same function, or sit in one of the helpers that only ever get a resolved path.
//!
//! It is plain text processing with no dependency, so `cargo xtask chess-gate` includes this
//! very file (`#[path]`) and runs it in a fraction of a second, while the crate's own test runs
//! it on the same sources: one scan, two callers.

/// Helpers that are only called with a resolved path. `doc_batch` (batch.rs) resolves its inputs
/// and out_dir, walks only inside them (skipping links) and resolves every discovered file again
/// before `process` reads it or `publish` writes it. `read_reference_text` (chess.rs) reads the
/// book's PGN that `read_reference` and `chess_reference_set` resolve first.
pub const RESOLVED_BY_CALLER: &[&str] =
    &["write_atomic", "write_atomic_with", "read_review_sidecar", "process", "linked", "walk", "publish", "read_reference_text"];

/// Whether the file is one the scan reads: a `.rs` file of `src/` that is not this guard's own
/// (`file_access.rs`, `file_access_scan.rs`) nor a test file.
pub fn scanned(file_name: &str) -> bool {
    file_name.ends_with(".rs") && !file_name.starts_with("file_access") && !file_name.starts_with("tests")
}

/// The code of a line: what comes before a `//` comment (a `//` inside a string literal ends it
/// too early, which only ever makes the scan stricter).
fn code(line: &str) -> &str {
    line.split("//").next().unwrap_or("")
}

/// Each line of `text` (a source file called `name`) that touches the file system in a function
/// that neither resolves a path first nor is a [`RESOLVED_BY_CALLER`] helper, as
/// `name:line: in `function`: text`. The test module (from the first `#[cfg(test)]` on) is left
/// out: tests build their own fixtures.
pub fn offenders(name: &str, text: &str) -> Vec<String> {
    let code_part = text.split("\n#[cfg(test)]").next().unwrap_or("");
    let lines: Vec<&str> = code_part.lines().collect();
    let mut out = Vec::new();
    for (n, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        let code_of_line = code(trimmed);
        if !(code_of_line.contains("std::fs")
            || code_of_line.contains("File::open")
            || code_of_line.contains("File::create")
            || code_of_line.contains("OpenOptions"))
        {
            continue;
        }
        let start =
            (0..=n).rev().find(|&i| lines.get(i).is_some_and(|l| code(l.trim_start()).contains("fn ") && l.trim_end().ends_with('{'))).unwrap_or(0);
        let function = lines.get(start).and_then(|l| l.split("fn ").nth(1)).and_then(|r| r.split(['(', '<']).next()).unwrap_or("");
        let resolved = lines.get(start..=n).is_some_and(|body| body.iter().any(|l| code(l).contains("resolve")));
        if !RESOLVED_BY_CALLER.contains(&function) && !resolved {
            out.push(format!("{name}:{}: in `{function}`: {trimmed}", n + 1));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_read_after_resolve_passes_and_one_without_is_named() {
        let ok = "impl A {\n    fn tool(&self) {\n        let p = self.resolve(x)?;\n        std::fs::read(&p)?;\n    }\n}\n";
        assert!(offenders("a.rs", ok).is_empty());
        let bad = "impl A {\n    fn tool(&self) {\n        std::fs::read(\"x\")?;\n    }\n}\n";
        assert_eq!(offenders("a.rs", bad), ["a.rs:3: in `tool`: std::fs::read(\"x\")?;"]);
    }

    #[test]
    fn a_resolve_in_a_comment_does_not_count() {
        let commented =
            "impl A {\n    fn tool(&self) {\n        // the path is resolved by the caller: resolve(x)\n        std::fs::read(\"x\")?;\n    }\n}\n";
        assert_eq!(offenders("a.rs", commented).len(), 1);
        let only_in_comment = "fn helper() {\n    // std::fs::read is fine here\n}\n";
        assert!(offenders("a.rs", only_in_comment).is_empty(), "a mention in a comment is not a use");
    }

    #[test]
    fn helpers_named_resolved_by_caller_and_tests_are_left_out() {
        let helper = "fn write_atomic(p: &Path) {\n    std::fs::write(p, b\"\")?;\n}\n";
        assert!(offenders("a.rs", helper).is_empty());
        let test = "fn f() {}\n#[cfg(test)]\nmod tests {\n    fn t() { std::fs::read(\"x\"); }\n}\n";
        assert!(offenders("a.rs", test).is_empty());
        assert!(scanned("chess.rs") && !scanned("file_access.rs") && !scanned("file_access_scan.rs") && !scanned("tests.rs"));
    }
}
