//! The chess texts' catalogue coverage (plan A9-Q1a): every `tl!` and `tl_ctx!` literal in the
//! chess source files (`src/chess*.rs`) must be in every catalogue. All 22 are checked: the four
//! that were exempt (`el`, `pl`, `te`, `zh-hant`) got their chess texts in plan A9-I14b.
//!
//! A text is its key as the runtime looks it up: the context and the English source. A `tl!`
//! text has no context; a `tl_ctx!` text needs its own context's entry (plan A9-I14c), because
//! a catalogue without one shows the plain translation, the colour "Black" (黑色) for the side
//! "Black" (黑方).
//!
//! Plain text processing with no dependency beyond `std`, so `cargo xtask chess-gate` includes
//! this very file (`#[path]`) and runs it in a fraction of a second, while the crate's own test
//! runs it on the same files: one scan, two callers.

use std::collections::{BTreeMap, BTreeSet, HashSet};

/// A text's key: its context (empty for `tl!`) and its English source.
pub type Text = (String, String);

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

/// The string literal `code` starts with (after spaces), and the code after its closing quote;
/// `None` when it does not start with one.
fn literal(code: &str) -> Option<(String, &str)> {
    let body_start = code.trim_start().strip_prefix('"')?;
    let mut escaped = false;
    let end = body_start.char_indices().find_map(|(i, c)| {
        if c == '"' && !escaped {
            return Some(i);
        }
        escaped = c == '\\' && !escaped;
        None
    })?;
    // `end` is the index of a `"`, one byte: both ends are char boundaries.
    Some((unescape_rust(body_start.get(..end)?), body_start.get(end.saturating_add(1)..)?))
}

/// The code after each `opening` (a macro's name and its parenthesis) in `code`.
fn after_each<'a>(code: &'a str, opening: &'a str) -> impl Iterator<Item = &'a str> {
    code.match_indices(opening).filter_map(move |(at, _)| code.get(at.saturating_add(opening.len())..))
}

/// The key of every `tl!("…")` and `tl_ctx!("context", "…")` in `source`, before its test
/// module. A call whose arguments are not literals (a variable) is not a text of the scan.
pub fn tl_texts(source: &str) -> BTreeSet<Text> {
    let code = source.split("#[cfg(test)]\nmod ").next().unwrap_or("");
    let mut out = BTreeSet::new();
    for args in after_each(code, "tl!(") {
        if let Some((text, _)) = literal(args) {
            out.insert((String::new(), text));
        }
    }
    for args in after_each(code, "tl_ctx!(") {
        let Some((context, rest)) = literal(args) else { continue };
        let Some(rest) = rest.trim_start().strip_prefix(',') else { continue };
        if let Some((text, _)) = literal(rest) {
            out.insert((context, text));
        }
    }
    out
}

/// A catalogue cell read as the catalogue parser reads it: `\n`, `\t` and `\\` (any other
/// escape is an error there, and is kept as written here).
fn unescape_tsv(cell: &str) -> String {
    let mut out = String::with_capacity(cell.len());
    let mut chars = cell.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// The keys (context, English source) a catalogue (`*.tsv`: context TAB source TAB translation,
/// `#` comments, escapes `\n`, `\t`, `\\`) translates.
pub fn catalogue_keys(tsv: &str) -> HashSet<Text> {
    tsv.lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| {
            let mut parts = l.split('\t');
            let context = parts.next()?;
            let source = parts.next()?;
            parts.next()?;
            Some((unescape_tsv(context), unescape_tsv(source)))
        })
        .collect()
}

/// A text as a message names it: `"Read"`, or `"Read" (context "diagram state")`.
pub fn shown((context, source): &Text) -> String {
    if context.is_empty() { format!("{source:?}") } else { format!("{source:?} (context {context:?})") }
}

/// For each catalogue (`code`, its text), the chess texts it lacks; catalogues that lack none
/// are left out.
pub fn missing(chess_texts: &BTreeSet<Text>, catalogues: &[(String, String)]) -> BTreeMap<String, Vec<Text>> {
    let mut out = BTreeMap::new();
    for (code, text) in catalogues {
        let have = catalogue_keys(text);
        let lacks: Vec<Text> = chess_texts.iter().filter(|t| !have.contains(*t)).cloned().collect();
        if !lacks.is_empty() {
            out.insert(code.clone(), lacks);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(s: &str) -> Text {
        (String::new(), s.to_owned())
    }

    fn ctx(c: &str, s: &str) -> Text {
        (c.to_owned(), s.to_owned())
    }

    #[test]
    fn literals_are_read_with_their_escapes_and_spacing() {
        let src = "fn f() {\n    tl!(\"Detect diagrams\");\n    tl!(\n        \"Say \\\"no\\\"\\n\",\n    );\n    tl!(name);\n}\n#[cfg(test)]\nmod tests { fn t() { tl!(\"only in a test\"); } }\n";
        let got = tl_texts(src);
        assert_eq!(got.into_iter().collect::<Vec<_>>(), [plain("Detect diagrams"), plain("Say \"no\"\n")]);
    }

    /// Plan A9-I14c: a `tl_ctx!` text is read with its context, on one line or several; one
    /// whose source is not a literal is not read.
    #[test]
    fn contextual_texts_are_read_with_their_context() {
        let src = "fn f() {\n    tl_ctx!(\"chess side\", \"Black\");\n    tl_ctx!(\n        \"diagram state\",\n        \"Read\",\n    );\n    tl_ctx!(\"calendar date\", MONTHS[0]);\n    tl!(\"Black\");\n}\n";
        let got = tl_texts(src);
        assert_eq!(got.into_iter().collect::<Vec<_>>(), [plain("Black"), ctx("chess side", "Black"), ctx("diagram state", "Read")]);
    }

    #[test]
    fn a_complete_catalogue_passes_and_a_gap_is_named() {
        let texts: BTreeSet<Text> = [plain("Detect diagrams"), plain("Line\nbreak")].into();
        let full = "# header\n\tDetect diagrams\tDetectar diagramas\n\tLine\\nbreak\tQuebra\\nde linha\n".to_owned();
        let gap = "\tDetect diagrams\tDiagramme erkennen\n".to_owned();
        // No catalogue is exempt any more (plan A9-I14b): a gap in `el`, once skipped, is named.
        let catalogues = vec![("pt-br".to_owned(), full), ("de".to_owned(), gap.clone()), ("el".to_owned(), gap)];
        let m = missing(&texts, &catalogues);
        assert_eq!(m.len(), 2, "{m:?}");
        assert_eq!(m.get("de").map(Vec::as_slice), Some(&[plain("Line\nbreak")][..]));
        assert_eq!(m.get("el").map(Vec::as_slice), Some(&[plain("Line\nbreak")][..]));
        assert!(is_chess_source("chess_ui.rs") && !is_chess_source("canvas.rs"));
    }

    /// Plan A9-I14c: the plain "Black" (the colour) does not stand for the side's own entry, and
    /// a context's entry does not stand for the plain text: each key is its own, as the runtime
    /// looks it up.
    #[test]
    fn a_context_is_part_of_the_key() {
        let texts: BTreeSet<Text> = [plain("Read"), ctx("chess side", "Black")].into();
        let plain_only = "\tBlack\t黑色\ndiagram state\tRead\t已读取\n".to_owned();
        let both = "\tBlack\t黑色\nchess side\tBlack\t黑方\n\tRead\t读取\n".to_owned();
        let m = missing(&texts, &[("zh-hans".to_owned(), plain_only), ("zh-hant".to_owned(), both)]);
        assert_eq!(m.len(), 1, "{m:?}");
        assert_eq!(m.get("zh-hans").map(Vec::as_slice), Some(&[plain("Read"), ctx("chess side", "Black")][..]));
        assert_eq!(shown(&ctx("chess side", "Black")), "\"Black\" (context \"chess side\")");
        assert_eq!(shown(&plain("Read")), "\"Read\"");
    }
}
