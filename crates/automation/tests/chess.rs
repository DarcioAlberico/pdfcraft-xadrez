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

/// After 1. e4 c5 2. Nf3 d6: not symmetric, so a board drawn turned or mirrored would show.
const SICILIAN: &str = "rnbqkbnr/pp2pppp/3p4/2p5/4P3/5N2/PPPP1PPP/RNBQKB1R";

fn nums(v: &Value) -> Vec<f64> {
    v.as_array().unwrap().iter().map(|n| n.as_f64().unwrap()).collect()
}

/// The font diagram is replaced by a vector board in its place: a dry run says what would be
/// done and changes nothing; the real one removes the font text, covers its box, and the scan
/// afterwards finds a board where the font board was. Undo brings the font board back.
#[test]
fn chess_replace_diagram_draws_a_vector_board_where_the_font_diagram_was() {
    let (base, root) = workdir("replace");
    let (mut a, doc) = opened(&root);
    let found = ok(&mut a, "chess_detect", json!({ "doc": doc }));
    let generation = found["generation"].as_u64().unwrap();
    let before = nums(&found["pages"][0]["diagrams"][0]["bbox_user"]);
    let args = json!({ "doc": doc, "page": 1, "index": 1, "generation": generation, "placement": SICILIAN, "pieces": "geometric", "dry_run": true });
    let dry = ok(&mut a, "chess_replace_diagram", args.clone());
    assert_eq!(
        (dry["dry_run"].as_bool(), dry["source"].as_str(), dry["old_diagram"].as_str()),
        (Some(true), Some("font"), Some("text_removed_and_covered")),
        "{dry:#}"
    );
    assert_eq!(dry["removed_text_lines"], 8, "{dry:#}");
    assert_eq!(dry["fen"], SICILIAN);
    assert_eq!(dry["pieces"]["source"], "geometric");
    assert_eq!(ok(&mut a, "doc_list", json!({}))["documents"][0]["dirty"], false, "a dry run changes nothing");
    let mut real = args;
    real["dry_run"] = json!(false);
    let done = ok(&mut a, "chess_replace_diagram", real.clone());
    assert_eq!(done["dry_run"], false);
    let board = nums(&done["board_user"]);
    // The new board sits inside the old diagram's box.
    assert!(
        board[0] >= before[0] - 0.5 && board[2] <= before[2] + 0.5 && board[1] >= before[1] - 0.5 && board[3] <= before[3] + 0.5,
        "{board:?} in {before:?}"
    );
    let after = ok(&mut a, "chess_detect", json!({ "doc": doc }));
    let sources: Vec<&str> = after["pages"][0]["diagrams"].as_array().unwrap().iter().map(|d| d["source"].as_str().unwrap()).collect();
    assert!(!sources.contains(&"font"), "the font diagram is gone: {after:#}");
    assert_eq!(after["diagrams"], 3, "{after:#}");
    let same_place = after["pages"][0]["diagrams"].as_array().unwrap().iter().any(|d| {
        let r = nums(&d["bbox_user"]);
        d["source"] != "font" && (0..4).all(|i| (r[i] - board[i]).abs() < 3.0)
    });
    // The redrawn board is one path per colour, so the page contour finds it, not the vector detector.
    assert!(same_place, "a board where the new one was drawn: {after:#}");
    // The generation it was numbered under is stale now.
    let stale = a.call("chess_replace_diagram", &real).unwrap_err();
    assert!(stale.to_string().contains("generation") || stale.to_string().contains("changed"), "{stale}");
    ok(&mut a, "edit_undo", json!({ "doc": doc }));
    let undone = ok(&mut a, "chess_detect", json!({ "doc": doc }));
    assert_eq!(undone["pages"][0]["diagrams"][0]["source"], "font", "{undone:#}");
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn chess_replace_diagram_refuses_what_it_cannot_place() {
    let (base, root) = workdir("replace-args");
    let (mut a, doc) = opened(&root);
    let generation = ok(&mut a, "chess_detect", json!({ "doc": doc }))["generation"].as_u64().unwrap();
    for (extra, needle) in [
        (json!({ "index": 9, "placement": SICILIAN }), ""),
        (json!({ "index": 0, "placement": SICILIAN }), "index"),
        (json!({ "index": 1 }), "give either fen or placement"),
        (json!({ "index": 1, "placement": SICILIAN, "fen": "8/8/8/8/8/8/8/8 w - - 0 1" }), "give either fen or placement"),
        (json!({ "index": 1, "placement": "not a board" }), "placement"),
        (json!({ "index": 1, "placement": SICILIAN, "border": 2 }), "border"),
        (json!({ "index": 1, "placement": SICILIAN, "pieces": "merida" }), "pieces"),
        (json!({ "index": 1, "placement": SICILIAN, "generation": generation + 1 }), ""),
    ] {
        let mut args = json!({ "doc": doc, "page": 1, "generation": generation });
        if let (Some(o), Value::Object(e)) = (args.as_object_mut(), extra.clone()) {
            o.extend(e);
        }
        let e = a.call("chess_replace_diagram", &args).unwrap_err();
        assert!(e.to_string().contains(needle), "{extra}: {e}");
    }
    assert_eq!(ok(&mut a, "doc_list", json!({}))["documents"][0]["dirty"], false, "nothing was changed");
    let _ = std::fs::remove_dir_all(base);
}

/// `content_draw` adds checked operators as one undo step and refuses fragments that name a
/// page resource or are unbalanced, saying why.
#[test]
fn content_draw_adds_checked_operators_and_refuses_the_rest() {
    let (base, root) = workdir("draw");
    let (mut a, doc) = opened(&root);
    let before = ok(&mut a, "chess_detect", json!({ "doc": doc }))["diagrams"].as_u64().unwrap();
    for (ops, needle) in [
        ("/Im1 Do", "not allowed"),
        ("q 0 0 10 10 re f", "q without a matching Q"),
        ("BT /PCHelv 12 Tf (x) Tj", "not closed with ET"),
        ("0 0 re f", "wrong operands"),
        ("", "no operators"),
    ] {
        let e = a.call("content_draw", &json!({ "doc": doc, "page": 1, "ops": ops })).unwrap_err();
        assert!(e.to_string().contains(needle), "{ops:?}: {e}");
    }
    assert_eq!(ok(&mut a, "doc_list", json!({}))["documents"][0]["dirty"], false);
    // A 4×4 checkerboard of 30 pt squares is too small a grid to be a board; a full 8×8 one is.
    let mut ops = String::from("0.4 g ");
    for rank in 0..8 {
        for file in 0..8 {
            if (rank + file) % 2 == 0 {
                ops.push_str(&format!("{} {} 15 15 re ", 360 + file * 15, 60 + rank * 15));
            }
        }
    }
    ops.push('f');
    let drawn = ok(&mut a, "content_draw", json!({ "doc": doc, "page": 1, "ops": ops }));
    assert_eq!(drawn["operators"], 34, "{drawn:#}");
    assert_eq!(ok(&mut a, "chess_detect", json!({ "doc": doc }))["diagrams"].as_u64().unwrap(), before + 1);
    ok(&mut a, "edit_undo", json!({ "doc": doc }));
    assert_eq!(ok(&mut a, "chess_detect", json!({ "doc": doc }))["diagrams"].as_u64().unwrap(), before);
    let _ = std::fs::remove_dir_all(base);
}

/// The page's three diagrams become three diagram blocks in reading order; the JSON and PGN come
/// inline or as files in the root, and the document is not changed.
#[test]
fn chess_document_reads_the_page_into_blocks_and_writes_json_and_pgn() {
    let (base, root) = workdir("document");
    let (mut a, doc) = opened(&root);
    let read = ok(&mut a, "chess_document", json!({ "doc": doc, "models_dir": "models", "include": ["json", "pgn"], "lang": "en" }));
    assert_eq!(
        (read["pages"].as_u64(), read["diagrams"].as_u64(), read["lang"].as_str(), read["lang_given"].as_bool()),
        (Some(1), Some(3), Some("en"), Some(true)),
        "{read:#}"
    );
    let blocks = read["document"]["pages"][0]["blocks"].as_array().unwrap();
    assert_eq!(blocks.iter().map(|b| b["type"].as_str().unwrap()).collect::<Vec<_>>(), ["diagram"; 3], "{read:#}");
    // The font diagram is read, with its link; the two others need the piece model.
    assert_eq!(blocks[0]["reading"]["fen"], "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1", "{:#}", blocks[0]);
    assert!(blocks[0]["lichess_url"].as_str().unwrap().starts_with("https://lichess.org/analysis/"));
    assert!(blocks[1]["reading"].is_null() && blocks[2]["reading"].is_null(), "{blocks:#?}");
    assert!(read["pgn"].is_string());
    // To files inside the root.
    let files = ok(&mut a, "chess_document", json!({ "doc": doc, "out": "doc.json", "pgn_out": "games.pgn" }));
    assert!(files.get("document").is_none() && files.get("pgn").is_none(), "nothing inline unless asked: {files:#}");
    let written: Value = serde_json::from_str(&std::fs::read_to_string(root.join("doc.json")).unwrap()).unwrap();
    assert_eq!(written["pages"][0]["blocks"].as_array().unwrap().len(), 3);
    assert!(root.join("games.pgn").exists());
    assert_eq!(ok(&mut a, "doc_list", json!({}))["documents"][0]["dirty"], false, "reading is read-only");
    let _ = std::fs::remove_dir_all(base);
}

/// `out` and `pgn_out` are confined to `--root`, and nothing is written outside it. The same
/// call into the root succeeds, so the refusal comes from resolving the path.
#[test]
fn chess_document_outputs_outside_the_root_are_refused() {
    let (base, root) = workdir("document-confined");
    let (mut a, doc) = opened(&root);
    for key in ["out", "pgn_out"] {
        for dir in ["../outside/x.out".to_string(), base.join("outside").join("y.out").to_string_lossy().into_owned()] {
            match a.call("chess_document", &json!({ "doc": doc, key: dir })) {
                Err(ToolError::Failed(m)) => assert!(m.contains("is outside the allowed directory"), "{key} {dir}: {m}"),
                other => panic!("{key} {dir} accepted: {other:?}"),
            }
        }
        ok(&mut a, "chess_document", json!({ "doc": doc, key: "inside.out" }));
    }
    assert_eq!(std::fs::read_dir(base.join("outside")).unwrap().count(), 0, "nothing was written outside");
    for (args, needle) in [
        (json!({ "doc": doc, "lang": "klingon" }), "lang"),
        (json!({ "doc": doc, "include": ["html"] }), "include"),
        (json!({ "doc": doc, "pages": [2] }), "out of range"),
    ] {
        let e = a.call("chess_document", &args).unwrap_err();
        assert!(e.to_string().contains(needle), "{args}: {e}");
    }
    let _ = std::fs::remove_dir_all(base);
}

/// The font diagram gets a label and a link under it (found by `link_list`); the two diagrams
/// no model reads are skipped with the reason. A dry run changes nothing, a second run adds
/// nothing, and undo takes it all back.
#[test]
fn chess_add_lichess_links_links_the_read_diagram_and_says_why_not_the_others() {
    let (base, root) = workdir("links");
    let (mut a, doc) = opened(&root);
    let dry = ok(&mut a, "chess_add_lichess_links", json!({ "doc": doc, "models_dir": "models", "dry_run": true }));
    assert_eq!((dry["dry_run"].as_bool(), dry["added"].as_u64()), (Some(true), Some(0)), "{dry:#}");
    assert_eq!(dry["links"].as_array().unwrap().len(), 1, "{dry:#}");
    assert_eq!(ok(&mut a, "doc_list", json!({}))["documents"][0]["dirty"], false, "a dry run changes nothing");
    let done = ok(&mut a, "chess_add_lichess_links", json!({ "doc": doc, "models_dir": "models", "label": "Analisar no lichess" }));
    assert_eq!(done["added"], 1, "{done:#}");
    let link = &done["links"][0];
    assert_eq!((link["page"].as_u64(), link["index"].as_u64(), link["side"].as_str()), (Some(1), Some(1), Some("below")), "{done:#}");
    assert_eq!(link["url"], "https://lichess.org/analysis/rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR%20w%20KQkq%20-%200%201");
    let skipped = done["skipped"].as_array().unwrap();
    assert_eq!(skipped.len(), 2, "{done:#}");
    assert!(skipped.iter().all(|s| s["reason"].as_str().unwrap().contains("no piece model")), "{skipped:#?}");
    let listed = ok(&mut a, "link_list", json!({ "doc": doc }));
    assert!(listed.to_string().contains("lichess.org/analysis"), "{listed:#}");
    let again = ok(&mut a, "chess_add_lichess_links", json!({ "doc": doc, "models_dir": "models" }));
    assert_eq!(again["added"], 0, "{again:#}");
    ok(&mut a, "edit_undo", json!({ "doc": doc }));
    assert!(!ok(&mut a, "link_list", json!({ "doc": doc })).to_string().contains("lichess.org"));
    for (args, needle) in [
        (json!({ "doc": doc, "label": "♘ lichess" }), "cannot draw"),
        (json!({ "doc": doc, "size": 1 }), "size"),
        (json!({ "doc": doc, "pages": [2] }), "out of range"),
    ] {
        let e = a.call("chess_add_lichess_links", &args).unwrap_err();
        assert!(e.to_string().contains(needle), "{args}: {e}");
    }
    let _ = std::fs::remove_dir_all(base);
}

/// The page exports as HTML with its three diagrams in order: the font one redrawn as SVG with
/// its FEN and lichess link, the two unread ones as figures without a position. Read back from
/// the file.
#[test]
fn chess_export_html_writes_the_diagrams_in_order_and_reads_back() {
    let (base, root) = workdir("html");
    let (mut a, doc) = opened(&root);
    let done = ok(&mut a, "chess_export_html", json!({ "doc": doc, "models_dir": "models", "out": "book.html", "pieces": "geometric" }));
    assert_eq!(
        (done["diagrams"].as_u64(), done["diagrams_drawn"].as_u64(), done["diagrams_unread"].as_u64()),
        (Some(3), Some(1), Some(2)),
        "{done:#}"
    );
    assert_eq!(done["pieces"]["source"], "geometric");
    let html = std::fs::read_to_string(root.join("book.html")).unwrap();
    assert_eq!(done["bytes"].as_u64(), Some(html.len() as u64));
    assert_eq!(html.matches("<figure class=\"diagram").count(), 3, "{html}");
    assert_eq!(html.matches("<figure class=\"diagram unread").count(), 2);
    assert!(html.contains("data-fen=\"rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1\""), "{html}");
    assert_eq!(html.matches("<svg").count(), 1);
    assert_eq!(html.matches("href=\"https://lichess.org/analysis/").count(), 1);
    assert!(!html.contains("<script"));
    // FEN text instead of boards, no links.
    let fen = ok(&mut a, "chess_export_html", json!({ "doc": doc, "out": "fen.html", "diagrams": "fen", "links": false }));
    assert_eq!(fen["diagrams_drawn"], 0);
    let html = std::fs::read_to_string(root.join("fen.html")).unwrap();
    assert!(html.contains("<code>rnbqkbnr/") && !html.contains("<svg") && !html.contains("lichess.org"), "{html}");
    assert_eq!(ok(&mut a, "doc_list", json!({}))["documents"][0]["dirty"], false);
    let _ = std::fs::remove_dir_all(base);
}

/// `out` is confined to `--root`; the same call into the root succeeds. Bad options are refused.
#[test]
fn chess_export_html_outside_the_root_is_refused() {
    let (base, root) = workdir("html-confined");
    let (mut a, doc) = opened(&root);
    for out in ["../outside/x.html".to_string(), base.join("outside").join("y.html").to_string_lossy().into_owned()] {
        match a.call("chess_export_html", &json!({ "doc": doc, "out": out })) {
            Err(ToolError::Failed(m)) => assert!(m.contains("is outside the allowed directory"), "{out}: {m}"),
            other => panic!("{out} accepted: {other:?}"),
        }
    }
    assert_eq!(std::fs::read_dir(base.join("outside")).unwrap().count(), 0, "nothing was written outside");
    ok(&mut a, "chess_export_html", json!({ "doc": doc, "out": "inside.html" }));
    for (args, needle) in [
        (json!({ "doc": doc, "out": "a.html", "diagrams": "png" }), "diagrams"),
        (json!({ "doc": doc, "out": "a.html", "figurines": "emoji" }), "figurines"),
        (json!({ "doc": doc, "out": "a.html", "figurines": "letters", "letters_lang": "xx" }), "letters_lang"),
        (json!({ "doc": doc, "out": "a.html", "link_text": "" }), "link_text"),
        (json!({ "doc": doc }), "out"),
    ] {
        let e = a.call("chess_export_html", &args).unwrap_err();
        assert!(e.to_string().contains(needle), "{args}: {e}");
    }
    let _ = std::fs::remove_dir_all(base);
}

/// The EPUB is written inside the root, starts with the stored `mimetype`, and holds the
/// package, the navigation and the page; outside the root it is refused and nothing is written.
#[test]
fn chess_export_epub_writes_a_book_inside_the_root_only() {
    let (base, root) = workdir("epub");
    let (mut a, doc) = opened(&root);
    let done =
        ok(&mut a, "chess_export_epub", json!({ "doc": doc, "models_dir": "models", "out": "book.epub", "pieces": "geometric", "title": "Board" }));
    assert_eq!((done["diagrams"].as_u64(), done["diagrams_drawn"].as_u64()), (Some(3), Some(1)), "{done:#}");
    let bytes = std::fs::read(root.join("book.epub")).unwrap();
    assert_eq!(done["bytes"].as_u64(), Some(bytes.len() as u64));
    assert_eq!(&bytes[30..58], b"mimetypeapplication/epub+zip");
    for name in ["META-INF/container.xml", "OEBPS/content.opf", "OEBPS/nav.xhtml", "OEBPS/page-00001.xhtml"] {
        assert!(bytes.windows(name.len()).any(|w| w == name.as_bytes()), "no {name} in the archive");
    }
    for out in ["../outside/x.epub".to_string(), base.join("outside").join("y.epub").to_string_lossy().into_owned()] {
        match a.call("chess_export_epub", &json!({ "doc": doc, "out": out })) {
            Err(ToolError::Failed(m)) => assert!(m.contains("is outside the allowed directory"), "{out}: {m}"),
            other => panic!("{out} accepted: {other:?}"),
        }
    }
    assert_eq!(std::fs::read_dir(base.join("outside")).unwrap().count(), 0, "nothing was written outside");
    assert_eq!(ok(&mut a, "doc_list", json!({}))["documents"][0]["dirty"], false);
    let _ = std::fs::remove_dir_all(base);
}

/// Word and RTF: the read diagram becomes an image, the two unread ones are said in words, and
/// both files are confined to the root.
#[test]
fn chess_export_docx_and_rtf_carry_the_diagram_as_an_image() {
    let (base, root) = workdir("office");
    let (mut a, doc) = opened(&root);
    let rtf =
        ok(&mut a, "chess_export_rtf", json!({ "doc": doc, "models_dir": "models", "out": "book.rtf", "pieces": "geometric", "diagram_px": 120 }));
    assert_eq!((rtf["diagrams"].as_u64(), rtf["diagrams_drawn"].as_u64(), rtf["diagrams_unread"].as_u64()), (Some(3), Some(1), Some(2)), "{rtf:#}");
    let text = std::fs::read_to_string(root.join("book.rtf")).unwrap();
    assert_eq!(text.matches("\\pngblip").count(), 1, "{text:.300}");
    assert!(text.contains("\\picw120\\pich120"));
    assert_eq!(text.matches("not read]").count(), 2);
    assert!(text.contains("FEN: rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"));
    assert!(text.contains("https://lichess.org/analysis/"));
    let docx = ok(&mut a, "chess_export_docx", json!({ "doc": doc, "out": "book.docx", "pieces": "geometric", "links": false }));
    assert_eq!(docx["diagrams_drawn"], 1, "{docx:#}");
    let bytes = std::fs::read(root.join("book.docx")).unwrap();
    assert_eq!(&bytes[..2], b"PK");
    assert!(bytes.windows(16).any(|w| w == b"word/media/image"));
    for (tool, out) in [("chess_export_docx", "../outside/x.docx"), ("chess_export_rtf", "../outside/x.rtf")] {
        match a.call(tool, &json!({ "doc": doc, "out": out })) {
            Err(ToolError::Failed(m)) => assert!(m.contains("is outside the allowed directory"), "{tool}: {m}"),
            other => panic!("{tool} accepted {out}: {other:?}"),
        }
    }
    assert_eq!(std::fs::read_dir(base.join("outside")).unwrap().count(), 0);
    let e = a.call("chess_export_docx", &json!({ "doc": doc, "out": "a.docx", "diagram_px": 5000 })).unwrap_err();
    assert!(e.to_string().contains("diagram_px"), "{e}");
    let _ = std::fs::remove_dir_all(base);
}

/// Markdown: the read diagram is an image in a folder beside the file, with its FEN as alt text;
/// the unread ones say so. PGN: the font diagram's position as a game. Both stay in the root.
#[test]
fn chess_export_markdown_and_pgn_write_inside_the_root() {
    let (base, root) = workdir("markdown");
    let (mut a, doc) = opened(&root);
    std::fs::create_dir_all(root.join("out")).unwrap();
    let md = ok(
        &mut a,
        "chess_export_markdown",
        json!({ "doc": doc, "models_dir": "models", "out": "out/book.md", "pieces": "geometric", "diagram_px": 96 }),
    );
    assert_eq!((md["diagrams_drawn"].as_u64(), md["image_dir"].as_str()), (Some(1), Some("book_diagrams")), "{md:#}");
    let text = std::fs::read_to_string(root.join("out").join("book.md")).unwrap();
    assert!(text.contains("![rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1](book_diagrams/diagram-p0001-1.png)"), "{text}");
    assert_eq!(text.matches("(not read)").count(), 2, "{text}");
    let png = std::fs::read(root.join("out").join("book_diagrams").join("diagram-p0001-1.png")).unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    // FEN only: no folder.
    let fen = ok(&mut a, "chess_export_markdown", json!({ "doc": doc, "out": "fen.md", "diagrams": "fen" }));
    assert!(fen["image_dir"].is_null());
    assert!(!root.join("fen_diagrams").exists());
    assert!(std::fs::read_to_string(root.join("fen.md")).unwrap().contains("`rnbqkbnr/"));
    let pgn = ok(&mut a, "chess_export_pgn", json!({ "doc": doc, "out": "games.pgn" }));
    assert_eq!(pgn["position_games"], 1, "{pgn:#}");
    let games = std::fs::read_to_string(root.join("games.pgn")).unwrap();
    assert!(
        games.contains("[SetUp \"1\"]")
            && games.contains("[FEN \"rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1\"]")
            && games.contains("[PdfDiagram \"1\"]"),
        "{games}"
    );
    for (tool, out) in [("chess_export_markdown", "../outside/x.md"), ("chess_export_pgn", "../outside/x.pgn")] {
        match a.call(tool, &json!({ "doc": doc, "out": out })) {
            Err(ToolError::Failed(m)) => assert!(m.contains("is outside the allowed directory"), "{tool}: {m}"),
            other => panic!("{tool} accepted {out}: {other:?}"),
        }
    }
    assert_eq!(std::fs::read_dir(base.join("outside")).unwrap().count(), 0, "nothing was written outside");
    let _ = std::fs::remove_dir_all(base);
}

/// Chess OCR asked for every page, without the glyph model in models_dir: nothing is read, and
/// the reply says what is missing; a bad mode is refused.
#[test]
fn chess_document_ocr_says_what_it_needs() {
    let (base, root) = workdir("ocr");
    let (mut a, doc) = opened(&root);
    let all = ok(&mut a, "chess_document", json!({ "doc": doc, "models_dir": "models", "ocr": "all" }));
    assert!(all["ocr_pages"].as_array().unwrap().is_empty(), "{all:#}");
    assert!(all["notes"].to_string().contains("needs the chess-glyphs model"), "{all:#}");
    let off = ok(&mut a, "chess_document", json!({ "doc": doc, "models_dir": "models", "ocr": "off" }));
    assert!(!off["notes"].to_string().contains("chess-glyphs"), "{off:#}");
    let e = a.call("chess_document", &json!({ "doc": doc, "ocr": "sometimes" })).unwrap_err();
    assert!(e.to_string().contains("ocr"), "{e}");
    let e = a.call("chess_export_html", &json!({ "doc": doc, "out": "x.html", "ocr": "sometimes" })).unwrap_err();
    assert!(e.to_string().contains("ocr"), "{e}");
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
