//! `pdfcraft-cli chess scan`: friendly flags over the `chess_read` and `chess_detect` tools, so
//! the command line, `run` and MCP give the same results and `--root` confines the same paths.

use serde_json::{Value, json};

use crate::{CliError, OptionSpec, automation, flag, page_list, print_output, stdout_line, validate_options};

const OPTIONS: &[OptionSpec] = &[
    OptionSpec { name: "--root", takes_value: true },
    OptionSpec { name: "--pages", takes_value: true },
    OptionSpec { name: "--models", takes_value: true },
    OptionSpec { name: "--dpi", takes_value: true },
    OptionSpec { name: "--password", takes_value: true },
    OptionSpec { name: "--json", takes_value: false },
    OptionSpec { name: "--detect-only", takes_value: false },
];

pub(super) fn run(args: &[String]) -> Result<(), CliError> {
    validate_options("chess", args, OPTIONS)?;
    let mut positional = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        if let Some(spec) = OPTIONS.iter().find(|s| s.name == arg) {
            if !seen.insert(spec.name) {
                return Err(format!("chess: {} may only be supplied once", spec.name).into());
            }
            if spec.takes_value {
                if args.get(i + 1).is_none_or(|v| v.is_empty() || v.starts_with("--")) {
                    return Err(format!("chess: {} needs a value", spec.name).into());
                }
                i += 1;
            }
        } else {
            positional.push(arg.as_str());
        }
        i += 1;
    }
    let (Some(&"scan"), Some(&file), None) = (positional.first(), positional.get(1), positional.get(2)) else {
        return Err(
            "chess: usage: pdfcraft-cli chess scan <file.pdf> [--pages 1,3-5] [--json] [--models DIR] [--dpi 220] [--detect-only] [--root DIR]"
                .into(),
        );
    };
    let mut open = json!({ "path": file });
    if let Some(pw) = flag(args, "--password") {
        open["password"] = json!(pw);
    }
    let mut params = json!({ "doc": 1 });
    if let Some(pages) = flag(args, "--pages") {
        let pages: Vec<usize> = page_list(pages).map_err(|e| format!("chess: --pages: {e}"))?.into_iter().map(|p| p + 1).collect();
        params["pages"] = json!(pages);
    }
    if let Some(dpi) = flag(args, "--dpi") {
        params["dpi"] = serde_json::from_str(dpi).map_err(|_| "chess: --dpi needs a number")?;
    }
    let detect_only = seen.contains("--detect-only");
    if let Some(dir) = flag(args, "--models") {
        if detect_only {
            return Err("chess: --models reads diagrams; it does nothing with --detect-only".into());
        }
        params["models_dir"] = json!(dir);
    }
    let mut auto = automation(args)?;
    let opened = auto.call("doc_open", &open).map_err(|e| format!("chess: {e}"))?;
    if let Some(id) = opened.iter().find_map(|c| match c {
        pdfcraft_automation::Content::Json(v) => v["doc"].as_u64(),
        pdfcraft_automation::Content::Png { .. } => None,
    }) {
        params["doc"] = json!(id);
    }
    let tool = if detect_only { "chess_detect" } else { "chess_read" };
    let content = auto.call(tool, &params).map_err(|e| format!("chess: {e}"))?;
    if seen.contains("--json") {
        return print_output(&auto, content, None);
    }
    for c in &content {
        if let pdfcraft_automation::Content::Json(v) = c {
            for line in summary(v) {
                stdout_line(format_args!("{line}"))?;
            }
        }
    }
    Ok(())
}

/// One line per diagram (and per page problem), for people.
fn summary(v: &Value) -> Vec<String> {
    let mut out = Vec::new();
    if v["model"]["state"].as_str().is_some_and(|s| s != "loaded") {
        let why = v["model"]["reason"].as_str().map(|r| format!(" ({r})")).unwrap_or_default();
        out.push(format!(
            "piece model {}{why}: only diagrams set in chess fonts are read (see chess_status)",
            v["model"]["state"].as_str().unwrap_or("missing")
        ));
    }
    for page in v["pages"].as_array().into_iter().flatten() {
        let n = page["page"].as_u64().unwrap_or(0);
        if let Some(e) = page["error"].as_str() {
            out.push(format!("page {n}: {e}"));
        }
        let readings = page["readings"].as_array();
        for d in page["diagrams"].as_array().into_iter().flatten() {
            let i = d["index"].as_u64().unwrap_or(0);
            let sources: Vec<&str> = d["sources"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            let rect = d["rect"].as_array().map(|r| r.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(" ")).unwrap_or_default();
            let mut line = format!("page {n} diagram {i} [{}] at {rect}", sources.join("+"));
            if let Some(r) = readings.and_then(|rs| rs.iter().find(|r| r["index"] == d["index"])) {
                match r["fen"].as_str() {
                    Some(fen) => {
                        let verdict = if r["accepted"] == true {
                            "accepted"
                        } else if r["legal"] == true {
                            "legal, review"
                        } else {
                            "illegal, review"
                        };
                        line.push_str(&format!(": {fen} ({verdict}, min confidence {})", r["min_confidence"]));
                        if let Some(url) = r["lichess_url"].as_str() {
                            line.push_str(&format!(" {url}"));
                        }
                    }
                    None => line.push_str(&format!(": unreadable ({})", r["reason"].as_str().unwrap_or("unknown"))),
                }
            }
            out.push(line);
        }
        if page["truncated"] == true {
            out.push(format!("page {n}: more diagrams than were kept"));
        }
        for note in page["notes"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            out.push(format!("page {n}: {note}"));
        }
    }
    let total = v["diagrams"].as_u64().unwrap_or(0);
    match (v["read"].as_u64(), v["accepted"].as_u64()) {
        (Some(read), Some(accepted)) => out.push(format!("{total} diagram(s), {read} read, {accepted} accepted")),
        _ => out.push(format!("{total} diagram(s)")),
    }
    out
}
