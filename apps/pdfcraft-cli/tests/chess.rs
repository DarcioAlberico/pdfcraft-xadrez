//! `pdfcraft-cli chess scan` and the chess tools through `run --script`, under `--root`, on a
//! synthetic page with a board set in a chess diagram font and a board drawn as filled squares.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const MERIDA_START: [&str; 8] = ["tMvWlVmT", "OoOoOoOo", " + + + +", "+ + + + ", " + + + +", "+ + + + ", "pPpPpPpP", "RnBqKbNr"];
const START_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

/// A 600 × 800 pt page: a Merida diagram at (60, 60) from the top left and a board of 32 filled
/// squares at (360, 60).
fn chess_pdf() -> Vec<u8> {
    let mut content = String::from("BT /F1 20 Tf 60 724 Td ");
    for (i, line) in MERIDA_START.iter().enumerate() {
        if i > 0 {
            content.push_str("0 -20 Td ");
        }
        content.push_str(&format!("({line}) Tj "));
    }
    content.push_str("ET\n0.45 g\n");
    for rank in 0..8 {
        for file in 0..8 {
            if (rank + file) % 2 == 0 {
                content.push_str(&format!("{} {} 20 20 re\n", 360 + file * 20, 580 + rank * 20));
            }
        }
    }
    content.push_str("f\n");
    let widths = vec!["1000"; 95].join(" ");
    let objs: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 600 800] >>".into(),
        "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".into(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        format!("<< /Type /Font /Subtype /Type1 /BaseFont /ChessMerida /FirstChar 32 /LastChar 126 /Widths [{widths}] >>"),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
    out
}

/// `root/` (with `board.pdf` and an empty `models/`) next to `outside/`.
fn sandbox(test: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("pdfcraft-cli-chess-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(base.join("root").join("models")).unwrap();
    std::fs::create_dir_all(base.join("outside")).unwrap();
    std::fs::write(base.join("root").join("board.pdf"), chess_pdf()).unwrap();
    base
}

fn cli(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_pdfcraft-cli")).current_dir(cwd).args(args).output().unwrap()
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

#[test]
fn chess_scan_prints_json_and_a_summary() {
    let base = sandbox("scan");
    let root = base.join("root");
    let root_s = root.to_str().unwrap();
    let o = cli(&base, &["chess", "scan", "board.pdf", "--json", "--root", root_s, "--models", "models"]);
    assert!(o.status.success(), "{}", text(&o.stderr));
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["model"]["state"], "missing");
    assert_eq!(v["diagrams"], 2, "{v:#}");
    let readings = v["pages"][0]["readings"].as_array().unwrap();
    assert_eq!(readings[0]["fen"], START_FEN);
    assert_eq!(readings[1]["unreadable"], "no_model");

    let o = cli(&base, &["chess", "scan", "board.pdf", "--root", root_s, "--models", "models", "--pages", "1"]);
    assert!(o.status.success(), "{}", text(&o.stderr));
    let out = text(&o.stdout);
    assert!(out.contains("piece model missing: only diagrams set in chess fonts are read"), "{out}");
    assert!(out.contains("page 1 diagram 1 [font] at 60"), "{out}");
    assert!(out.contains(&format!(": {START_FEN} (accepted, min confidence 1.0) https://lichess.org/analysis/")), "{out}");
    assert!(out.contains("page 1 diagram 2 [vector") && out.contains("unreadable (no piece model is loaded)"), "{out}");
    assert!(out.contains("2 diagram(s), 1 read, 1 accepted"), "{out}");

    let o = cli(&base, &["chess", "scan", "board.pdf", "--root", root_s, "--detect-only"]);
    assert!(o.status.success(), "{}", text(&o.stderr));
    assert!(text(&o.stdout).contains("2 diagram(s)\n") || text(&o.stdout).contains("2 diagram(s)\r\n"), "{}", text(&o.stdout));
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn chess_scan_keeps_its_paths_inside_the_root() {
    let base = sandbox("root");
    let root = base.join("root");
    let root_s = root.to_str().unwrap();
    let o = cli(&base, &["chess", "scan", "board.pdf", "--root", root_s, "--models", "../outside"]);
    assert!(!o.status.success());
    assert!(text(&o.stderr).contains("is outside the allowed directory"), "{}", text(&o.stderr));
    let o = cli(&base, &["chess", "scan", "../root/board.pdf", "--root", base.join("outside").to_str().unwrap()]);
    assert!(!o.status.success());
    assert!(text(&o.stderr).contains("is outside the allowed directory"), "{}", text(&o.stderr));
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn chess_scan_refuses_bad_usage() {
    let base = sandbox("usage");
    for args in
        [&["chess"][..], &["chess", "read", "x.pdf"], &["chess", "scan"], &["chess", "scan", "a.pdf", "b.pdf"], &["chess", "scan", "a.pdf", "--frob"]]
    {
        let o = cli(&base, args);
        assert!(!o.status.success(), "{args:?}");
        assert!(text(&o.stderr).contains("chess"), "{args:?}: {}", text(&o.stderr));
    }
    let _ = std::fs::remove_dir_all(base);
}

/// The tools through `run --script`, with the page rendered for a look, all inside the root.
#[test]
fn chess_tools_run_from_a_script_with_a_page_render() {
    let base = sandbox("script");
    let root = base.join("root");
    let steps = serde_json::json!([
        { "tool": "doc_open", "args": { "path": "board.pdf" } },
        { "tool": "chess_detect", "args": { "doc": 1 } },
        { "tool": "chess_read", "args": { "doc": 1, "models_dir": "models" } },
        { "tool": "page_render", "args": { "doc": 1, "page": 1, "dpi": 36 }, "out": "page1.png" },
    ]);
    std::fs::write(root.join("steps.json"), steps.to_string()).unwrap();
    let o = cli(&base, &["run", "--script", root.join("steps.json").to_str().unwrap(), "--root", root.to_str().unwrap(), "--report"]);
    assert!(o.status.success(), "{}", text(&o.stderr));
    let out = text(&o.stdout);
    assert!(out.contains(START_FEN), "{out}");
    assert!(out.contains("no_model"), "{out}");
    assert!(std::fs::read(root.join("page1.png")).unwrap().starts_with(b"\x89PNG"));
    let _ = std::fs::remove_dir_all(base);
}

/// `chess_games` through `run --script` (wave 10, U2): a PGN file in the root reordered and
/// renamed into another, read back by the next step; a step writing outside the root fails the
/// run and writes nothing.
#[test]
fn chess_games_runs_from_a_script_inside_the_root() {
    let base = sandbox("games");
    let root = base.join("root");
    let abc = "[Event \"A\"]\n\n1. e4 *\n\n[Event \"B\"]\n\n1. d4 *\n\n[Event \"C\"]\n\n1. c4 *\n";
    std::fs::write(root.join("abc.pgn"), abc).unwrap();
    let steps = serde_json::json!([
        { "tool": "chess_games", "args": { "path": "abc.pgn", "out": "cab.pgn", "ops": [
            { "op": "move", "game": 2, "to": 0 },
            { "op": "rename", "game": 0, "white": "Werle", "black": "Jianu" },
        ] } },
        { "tool": "chess_games", "args": { "path": "cab.pgn", "ops": [{ "op": "list" }] } },
    ]);
    std::fs::write(root.join("steps.json"), steps.to_string()).unwrap();
    let o = cli(&base, &["run", "--script", root.join("steps.json").to_str().unwrap(), "--root", root.to_str().unwrap(), "--report"]);
    assert!(o.status.success(), "{}", text(&o.stderr));
    let written = std::fs::read_to_string(root.join("cab.pgn")).unwrap();
    let events: Vec<&str> = written.lines().filter_map(|l| l.strip_prefix("[Event \"").and_then(|r| r.strip_suffix("\"]"))).collect();
    assert_eq!(events, ["C", "A", "B"], "{written}");
    assert!(written.contains("[White \"Werle\"]"), "{written}");
    assert!(text(&o.stdout).contains("Werle–Jianu"), "the second step read the file the first wrote: {}", text(&o.stdout));
    let escape = serde_json::json!([{ "tool": "chess_games", "args": { "path": "abc.pgn", "out": "../outside/escaped.pgn" } }]);
    std::fs::write(root.join("escape.json"), escape.to_string()).unwrap();
    let o = cli(&base, &["run", "--script", root.join("escape.json").to_str().unwrap(), "--root", root.to_str().unwrap()]);
    assert!(!o.status.success(), "{}", text(&o.stdout));
    assert!(text(&o.stderr).contains("outside the allowed directory"), "{}", text(&o.stderr));
    assert!(!base.join("outside").join("escaped.pgn").exists());
    let _ = std::fs::remove_dir_all(base);
}
