//! End-to-end tests of the chess tools (`chess_detect`, `chess_read`, `chess_status`) on a
//! synthetic page with a board in a chess diagram font, a board drawn as filled squares and a
//! checkerboard image, under `--root`. No chess model ships, so reading with one is covered in the
//! engine with a stand-in; here the model is always missing or broken.

use std::path::{Path, PathBuf};

use pdfcraft_automation::{Automation, Content, ToolError};
use serde_json::{Value, json};

/// The start position in the Chess Merida layout, top rank first.
const MERIDA_START: [&str; 8] = ["tMvWlVmT", "OoOoOoOo", " + + + +", "+ + + + ", " + + + +", "+ + + + ", "pPpPpPpP", "RnBqKbNr"];

/// A 600 × 800 pt page: a Merida diagram at (60, 60) from the top left, 32 filled squares at
/// (60, 400) and a 320 px checkerboard image at (360, 60), each 160 pt wide.
pub fn chess_pdf() -> Vec<u8> {
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
                content.push_str(&format!("{} {} 20 20 re\n", 60 + file * 20, 240 + rank * 20));
            }
        }
    }
    content.push_str("f\nq 160 0 0 160 360 580 cm /Im1 Do Q\n");
    let px = 320usize;
    let image: Vec<u8> = (0..px * px).map(|i| if ((i % px) / 40 + (i / px) / 40).is_multiple_of(2) { 235 } else { 120 }).collect();
    let widths = vec!["1000"; 95].join(" ");
    let mut objs: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 600 800] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> /XObject << /Im1 6 0 R >> >> >>".to_vec(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()).into_bytes(),
        format!("<< /Type /Font /Subtype /Type1 /BaseFont /ChessMerida /FirstChar 32 /LastChar 126 /Widths [{widths}] >>").into_bytes(),
    ];
    let mut im = format!(
        "<< /Type /XObject /Subtype /Image /Width {px} /Height {px} /ColorSpace /DeviceGray /BitsPerComponent 8 /Length {} >>\nstream\n",
        image.len()
    )
    .into_bytes();
    im.extend_from_slice(&image);
    im.extend_from_slice(b"\nendstream");
    objs.push(im);
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(o);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
    out
}

/// A fresh folder holding `root/` (with `board.pdf` and an empty `models/`) and `outside/` (a
/// models folder beside the root, out of its reach).
#[track_caller]
fn workdir(test: &str) -> (PathBuf, PathBuf) {
    let line = std::panic::Location::caller().line();
    let base = std::env::temp_dir().join(format!("pdfcraft-chess-{test}-{}-{line}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("root");
    std::fs::create_dir_all(root.join("models")).unwrap();
    std::fs::create_dir_all(base.join("outside")).unwrap();
    std::fs::write(root.join("board.pdf"), chess_pdf()).unwrap();
    (base, root)
}

fn json_of(content: Vec<Content>) -> Value {
    content
        .into_iter()
        .find_map(|c| match c {
            Content::Json(v) => Some(v),
            Content::Png { .. } => None,
        })
        .unwrap()
}

fn ok(a: &mut Automation, tool: &str, args: Value) -> Value {
    json_of(a.call(tool, &args).unwrap_or_else(|e| panic!("{tool} {args}: {e}")))
}

fn opened(root: &Path) -> (Automation, u64) {
    let mut a = Automation::new().with_root(root).unwrap();
    let doc = ok(&mut a, "doc_open", json!({ "path": "board.pdf" }))["doc"].as_u64().unwrap();
    (a, doc)
}

#[test]
fn chess_detect_lists_the_diagrams_in_reading_order() {
    let (base, root) = workdir("detect");
    let (mut a, doc) = opened(&root);
    let found = ok(&mut a, "chess_detect", json!({ "doc": doc }));
    assert_eq!(found["diagrams"], 3, "{found:#}");
    let page = &found["pages"][0];
    assert_eq!(page["page"], 1);
    assert!(page.get("readings").is_none(), "detection does not read: {page:#}");
    let diagrams = page["diagrams"].as_array().unwrap();
    let placed: Vec<&str> = diagrams.iter().map(|d| d["source"].as_str().unwrap()).collect();
    assert_eq!(placed, ["font", "vector", "image"], "{page:#}");
    assert_eq!(diagrams.iter().map(|d| d["index"].as_u64().unwrap()).collect::<Vec<_>>(), [1, 2, 3]);
    // The font diagram: 160 pt from (60, 60) on the displayed page; y up in user space.
    let rect: Vec<f64> = diagrams[0]["rect"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    assert!((rect[0] - 60.0).abs() < 1.5 && (rect[2] - rect[0] - 160.0).abs() < 1.5, "{rect:?}");
    let user: Vec<f64> = diagrams[0]["bbox_user"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    assert!((user[3] - (800.0 - rect[1])).abs() < 1.5, "{user:?} vs {rect:?}");
    assert_eq!(diagrams[0]["position_in_text"], true);
    assert!(diagrams[2]["sources"].as_array().unwrap().contains(&json!("image")));
    // Detection with dpi and order.
    let rows = ok(&mut a, "chess_detect", json!({ "doc": doc, "pages": [1], "dpi": 150, "order": "row", "max_per_page": 2 }));
    let placed: Vec<&str> = rows["pages"][0]["diagrams"].as_array().unwrap().iter().map(|d| d["source"].as_str().unwrap()).collect();
    assert_eq!(placed, ["font", "image"], "row order reads across first: {rows:#}");
    assert_eq!(rows["pages"][0]["truncated"], true);
    assert!((rows["pages"][0]["dpi"].as_f64().unwrap() - 150.0).abs() < 1.0);
    let _ = std::fs::remove_dir_all(base);
}

/// Without a model, the font diagram is read exactly, with its lichess link; the other two are
/// found and say why they were not read. The document is not changed.
#[test]
fn chess_read_reads_font_diagrams_without_a_model_and_says_why_the_others_are_not() {
    let (base, root) = workdir("read");
    let (mut a, doc) = opened(&root);
    let read = ok(&mut a, "chess_read", json!({ "doc": doc, "models_dir": "models" }));
    assert_eq!(read["model"]["state"], "missing", "{read:#}");
    assert_eq!((read["diagrams"].as_u64(), read["read"].as_u64(), read["accepted"].as_u64()), (Some(3), Some(1), Some(1)), "{read:#}");
    let readings = read["pages"][0]["readings"].as_array().unwrap();
    let font = &readings[0];
    assert_eq!(font["read_by"], "font");
    assert_eq!(font["fen"], "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1");
    assert_eq!((font["legal"].as_bool(), font["legality"].as_str(), font["accepted"].as_bool()), (Some(true), Some("legal"), Some(true)));
    assert_eq!(font["min_confidence"], 1.0);
    assert_eq!(font["repaired"], false);
    assert_eq!(font["lichess_url"], "https://lichess.org/analysis/rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR%20w%20KQkq%20-%200%201");
    for r in &readings[1..] {
        assert_eq!(r["unreadable"], "no_model", "{r:#}");
        assert_eq!(r["reason"], "no piece model is loaded");
        assert!(r.get("lichess_url").is_none(), "{r:#}");
        assert!(r["dpi"].as_f64().unwrap() > 200.0, "{r:#}");
    }
    let info = ok(&mut a, "doc_list", json!({}))["documents"][0].clone();
    assert_eq!(info["dirty"], false, "reading is read-only");
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn chess_status_says_whether_models_are_missing_or_invalid_and_why() {
    let (base, root) = workdir("status");
    let mut a = Automation::new().with_root(&root).unwrap();
    let missing = ok(&mut a, "chess_status", json!({ "models_dir": "models" }));
    assert_eq!((missing["pieces"]["state"].as_str(), missing["glyphs"]["state"].as_str()), (Some("missing"), Some("missing")), "{missing:#}");
    assert!(missing["pieces"]["reason"].is_null());
    assert_eq!(missing["looked_in"], "models_dir");
    assert!(["font", "geometric"].contains(&missing["piece_set"]["source"].as_str().unwrap()), "{missing:#}");
    // A model file that is not a model: invalid, with the reason, both here and when reading.
    std::fs::write(root.join("models").join("chess-pieces.onnx"), b"not a model").unwrap();
    std::fs::write(root.join("models").join("chess-pieces.json"), b"{\"kind\": \"pieces\"}").unwrap();
    let invalid = ok(&mut a, "chess_status", json!({ "models_dir": "models" }));
    assert_eq!(invalid["pieces"]["state"], "invalid", "{invalid:#}");
    assert!(!invalid["pieces"]["reason"].as_str().unwrap().is_empty());
    let doc = ok(&mut a, "doc_open", json!({ "path": "board.pdf" }))["doc"].as_u64().unwrap();
    let read = ok(&mut a, "chess_read", json!({ "doc": doc, "models_dir": "models" }));
    assert_eq!(read["model"]["state"], "invalid", "{read:#}");
    assert_eq!(read["model"]["reason"], invalid["pieces"]["reason"]);
    assert_eq!(read["pages"][0]["readings"][1]["unreadable"], "no_model");
    // A file is not a models folder.
    let not_dir = a.call("chess_status", &json!({ "models_dir": "board.pdf" })).unwrap_err();
    assert!(not_dir.to_string().contains("not a folder"), "{not_dir}");
    let _ = std::fs::remove_dir_all(base);
}

/// `models_dir` is confined to `--root`. The document is real and the same call with a folder
/// inside the root succeeds, so the refusal can only come from resolving the path.
#[test]
fn a_models_dir_outside_the_root_is_refused() {
    let (base, root) = workdir("confined");
    let (mut a, doc) = opened(&root);
    let outside = base.join("outside");
    for dir in ["../outside".to_string(), outside.to_string_lossy().into_owned()] {
        for (tool, args) in [("chess_read", json!({ "doc": doc, "models_dir": dir })), ("chess_status", json!({ "models_dir": dir }))] {
            match a.call(tool, &args) {
                Err(ToolError::Failed(m)) => assert!(m.contains("is outside the allowed directory"), "{tool} {dir}: {m}"),
                other => panic!("{tool} accepted models_dir {dir}: {other:?}"),
            }
        }
    }
    // The control: the same calls, with the folder inside the root.
    ok(&mut a, "chess_read", json!({ "doc": doc, "models_dir": "models" }));
    ok(&mut a, "chess_status", json!({ "models_dir": "models" }));
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn chess_arguments_out_of_range_are_refused_with_what_is_allowed() {
    let (base, root) = workdir("args");
    let (mut a, doc) = opened(&root);
    for (args, needle) in [
        (json!({ "doc": doc, "dpi": 10 }), "dpi must be between 72 and 600"),
        (json!({ "doc": doc, "dpi": 300, "max_read_dpi": 200 }), "max_read_dpi must be between dpi"),
        (json!({ "doc": doc, "order": "diagonal" }), "order must be"),
        (json!({ "doc": doc, "max_per_page": 0 }), "max_per_page must be from 1 to 64"),
        (json!({ "doc": doc, "pages": [2] }), "out of range"),
    ] {
        let e = a.call("chess_read", &args).unwrap_err();
        assert!(e.to_string().contains(needle), "{args}: {e}");
    }
    let _ = std::fs::remove_dir_all(base);
}
