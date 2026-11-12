//! The chess texts' catalogue coverage (plan A9-Q1a): every `tl!` literal in the chess source
//! files (`src/chess*.rs`) must be in every catalogue that is complete for chess.
//!
//! Plain text processing with no dependency beyond `std`, so `cargo xtask chess-gate` includes
//! this very file (`#[path]`) and runs it in a fraction of a second, while the crate's own test
//! runs it on the same files: one scan, two callers.

use std::collections::{BTreeMap, BTreeSet, HashSet};

/// Catalogues not yet complete for chess: each lacks 34 chess texts (measured 2026-11-11, plan
/// A9 R10), so they show those in English. Step A9-I14 completes them and empties this list.
pub const INCOMPLETE: &[&str] = &["el", "pl", "te", "zh-hant"];

/// Whether `file_name` (in `src/`) is a chess source file the scan reads.
pub fn is_chess_source(file_name: &str) -> bool {
    file_name.starts_with("chess") && file_name.ends_with(".rs")
}

/// The string a Rust literal's body stands for: `\"`, `\\`, `\n`, `\t`, `\r`, `\'` and `\0`
/// read; any other escape is kept as written.
fn unescape_rust(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('0') => out.push('\0'),
            Some(other @ ('"' | '\\' | '\'')) => out.push(other),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// The first string literal of every `tl!(…)` in `source`, before its test module.
pub fn tl_literals(source: &str) -> BTreeSet<String> {
    let code = source.split("#[cfg(test)]\nmod ").next().unwrap_or("");
    let mut out = BTreeSet::new();
    let mut rest = code;
    while let Some(at) = rest.find("tl!(") {
        let after = rest.get(at + 4..).unwrap_or("");
        let trimmed = after.trim_start();
        rest = after;
        let Some(body_start) = trimmed.strip_prefix('"') else { continue };
        let mut escaped = false;
        let end = body_start.char_indices().find_map(|(i, c)| {
            if c == '"' && !escaped {
                return Some(i);
            }
            escaped = c == '\\' && !escaped;
            None
        });
        let Some(end) = end else { continue };
        if let Some(body) = body_start.get(..end) {
            out.insert(unescape_rust(body));
        }
    }
    out
}

/// The English sources a catalogue (`*.tsv`: context TAB source TAB translation, `#` comments,
/// escapes `\n`, `\t`, `\\`) translates.
pub fn catalogue_sources(tsv: &str) -> HashSet<String> {
    tsv.lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| {
            let mut parts = l.split('\t');
            let _context = parts.next()?;
            let source = parts.next()?;
            parts.next()?;
            Some(source.replace("\\n", "\n").replace("\\t", "\t").replace("\\\\", "\\"))
        })
        .collect()
}

/// For each catalogue (`code`, its text) outside [`INCOMPLETE`], the chess literals it lacks;
/// catalogues that lack none are left out.
pub fn missing(chess_literals: &BTreeSet<String>, catalogues: &[(String, String)]) -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::new();
    for (code, text) in catalogues {
        if INCOMPLETE.contains(&code.as_str()) {
            continue;
        }
        let have = catalogue_sources(text);
        let lacks: Vec<String> = chess_literals.iter().filter(|l| !have.contains(*l)).cloned().collect();
        if !lacks.is_empty() {
            out.insert(code.clone(), lacks);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_are_read_with_their_escapes_and_spacing() {
        let src = "fn f() {\n    tl!(\"Detect diagrams\");\n    tl!(\n        \"Say \\\"no\\\"\\n\",\n    );\n    tl!(name);\n}\n#[cfg(test)]\nmod tests { fn t() { tl!(\"only in a test\"); } }\n";
        let got = tl_literals(src);
        assert_eq!(got.into_iter().collect::<Vec<_>>(), ["Detect diagrams", "Say \"no\"\n"]);
    }

    #[test]
    fn a_complete_catalogue_passes_and_a_gap_is_named() {
        let literals: BTreeSet<String> = ["Detect diagrams".to_owned(), "Line\nbreak".to_owned()].into();
        let full = "# header\n\tDetect diagrams\tDetectar diagramas\n\tLine\\nbreak\tQuebra\\nde linha\n".to_owned();
        let gap = "\tDetect diagrams\tDiagramme erkennen\n".to_owned();
        let catalogues = vec![("pt-br".to_owned(), full), ("de".to_owned(), gap.clone()), ("el".to_owned(), gap)];
        let m = missing(&literals, &catalogues);
        assert_eq!(m.len(), 1, "{m:?}");
        assert_eq!(m.get("de").map(Vec::as_slice), Some(&["Line\nbreak".to_owned()][..]));
        assert!(is_chess_source("chess_ui.rs") && !is_chess_source("canvas.rs"));
    }
}
