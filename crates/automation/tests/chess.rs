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

/// Redraw in the PDF without the interface (plan A8, P3b4): with neither fen nor placement the
/// position kept for the diagram is drawn, and keep_correction keeps it with the page as it is
/// after the drawing, so the correction is on its page unchanged rather than on a changed page.
#[test]
fn chess_replace_diagram_redraws_the_kept_correction() {
    const CORRECTED: &str = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1";
    let (base, root) = workdir("redraw");
    let (mut a, doc) = opened(&root);
    let set = ok(&mut a, "chess_correction_set", json!({ "doc": doc, "page": 1, "index": 1, "fen": CORRECTED, "models_dir": "models" }));
    let generation = ok(&mut a, "chess_detect", json!({ "doc": doc }))["generation"].as_u64().unwrap();
    let args = json!({ "doc": doc, "page": 1, "index": 1, "generation": generation, "pieces": "geometric", "models_dir": "models" });
    let mut dry = args.clone();
    dry["dry_run"] = json!(true);
    let dry = ok(&mut a, "chess_replace_diagram", dry);
    assert_eq!(
        (dry["from_correction"].as_str(), dry["fen"].as_str()),
        (set["uid"].as_str(), Some("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR")),
        "{dry:#}"
    );
    assert_eq!(dry["correction"], Value::Null, "a dry run keeps nothing");
    let mut keep = args;
    keep["keep_correction"] = json!(true);
    let done = ok(&mut a, "chess_replace_diagram", keep);
    assert_eq!(done["from_correction"], set["uid"], "{done:#}");
    assert_eq!(
        (done["correction"]["uid"].as_str(), done["correction"]["kind"].as_str(), done["correction"]["rev"].as_u64()),
        (set["uid"].as_str(), Some("corrected"), Some(2)),
        "{done:#}"
    );
    let listed = ok(&mut a, "chess_correction_list", json!({ "doc": doc }));
    let c = &listed["corrections"][0];
    assert_eq!((listed["count"].as_u64(), c["located"].as_str(), c["fen"].as_str()), (Some(1), Some("same"), Some(CORRECTED)), "{listed:#}");
    // The corrections folder stays inside the root, for the position read and the one kept.
    let generation = ok(&mut a, "chess_detect", json!({ "doc": doc }))["generation"].as_u64().unwrap();
    let outside = base.join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    for store in ["../outside".to_string(), outside.to_string_lossy().into_owned()] {
        for extra in [json!({}), json!({ "fen": CORRECTED, "keep_correction": true })] {
            let mut args = json!({ "doc": doc, "page": 1, "index": 1, "generation": generation, "pieces": "geometric", "store": store });
            if let (Some(o), Value::Object(e)) = (args.as_object_mut(), extra) {
                o.extend(e);
            }
            match a.call("chess_replace_diagram", &args) {
                Err(ToolError::Failed(m)) => assert!(m.contains("is outside the allowed directory"), "{store}: {m}"),
                other => panic!("store {store} outside the root accepted: {other:?}"),
            }
        }
    }
    assert!(std::fs::read_dir(&outside).unwrap().next().is_none(), "nothing written outside");
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
        (json!({ "index": 1 }), "no correction kept"),
        (json!({ "index": 1, "placement": SICILIAN, "fen": "8/8/8/8/8/8/8/8 w - - 0 1" }), "not both"),
        (json!({ "index": 1, "placement": SICILIAN, "keep_correction": true }), "give fen, not placement"),
        (json!({ "index": 1, "placement": "not a board" }), "placement"),
        (json!({ "index": 1, "placement": SICILIAN, "border": 2 }), "border"),
        (json!({ "index": 1, "placement": SICILIAN, "pieces": "merida" }), "a set PdfCraft ships (chessnut"),
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

/// `chess_pieces` lists what Preferences offers: automatic first, the shipped sets with author
/// and licence, plain shapes last; each with its token and, where the tools take it, the value of
/// their `pieces`.
#[test]
fn chess_pieces_lists_the_sets_preferences_offers() {
    let (base, root) = workdir("pieces-list");
    let (mut a, _) = opened(&root);
    let listed = ok(&mut a, "chess_pieces", json!({}));
    let sets = listed["sets"].as_array().unwrap();
    assert_eq!(sets.first().map(|s| &s["token"]), Some(&json!("auto")), "{listed:#}");
    assert_eq!(sets.last().map(|s| &s["pieces"]), Some(&json!("geometric")), "{listed:#}");
    let shipped: Vec<&Value> = sets.iter().filter(|s| s["kind"] == "shipped").collect();
    assert_eq!(shipped.len(), 8, "{listed:#}");
    let chessnut = shipped.iter().find(|s| s["pieces"] == "chessnut").expect("chessnut is listed");
    assert_eq!((&chessnut["token"], &chessnut["licence"]), (&json!("bundled:chessnut"), &json!("Apache-2.0")));
    assert!(shipped.iter().all(|s| s["author"].is_string() && s["missing"] == json!([])), "{listed:#}");
    assert!(listed["environment"].is_boolean());
    // Every value it gives for `pieces` is one the tools take.
    let (mut b, doc) = opened(&root);
    let generation = ok(&mut b, "chess_detect", json!({ "doc": doc }))["generation"].as_u64().unwrap();
    for pieces in sets.iter().filter_map(|s| s["pieces"].as_str()) {
        let args = json!({ "doc": doc, "page": 1, "index": 1, "generation": generation, "placement": SICILIAN, "pieces": pieces, "dry_run": true });
        ok(&mut b, "chess_replace_diagram", args);
    }
    let _ = std::fs::remove_dir_all(base);
}

/// A page of ours carrying one set of twelve squares, as a script's JSON with percent-encoded
/// values.
fn piece_page() -> String {
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><path d="M2 2 H8 V8 H2 Z" fill="black"/></svg>"#;
    let value = format!("data:image/svg+xml,{}", svg.replace('<', "%3C").replace('>', "%3E").replace('"', "%22"));
    let keys = ["wK", "wQ", "wR", "wB", "wN", "wP", "bK", "bQ", "bR", "bB", "bN", "bP"];
    let entries: Vec<String> = keys.iter().map(|k| format!("\"{k}\": \"{value}\"")).collect();
    format!("<html><script>var sets = {{\"squares\": {{{}}}}};</script></html>", entries.join(", "))
}

/// `chess_pieces_import` takes a page's sets into a folder inside the root, says what it wrote and
/// refuses to replace it unless asked; a page or folder outside the root is refused, as is a
/// target outside it, and nothing is written there. The controls pass the same page from inside.
#[test]
fn chess_pieces_import_is_confined_to_the_root() {
    let (base, root) = workdir("pieces-import");
    std::fs::write(root.join("course.html"), piece_page()).unwrap();
    let outside = base.join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("course.html"), piece_page()).unwrap();
    let mut a = Automation::new().with_root(&root).unwrap();
    let outside_page = outside.join("course.html").to_string_lossy().into_owned();
    let outside_dir = outside.join("pieces").to_string_lossy().into_owned();
    for args in [
        json!({ "from": outside_page, "into": "pieces" }),
        json!({ "from": "../outside/course.html", "into": "pieces" }),
        json!({ "from": "course.html", "into": outside_dir }),
        json!({ "from": "course.html", "into": "../outside/pieces" }),
    ] {
        match a.call("chess_pieces_import", &args) {
            Err(ToolError::Failed(m)) => assert!(m.contains("is outside the allowed directory"), "{args}: {m}"),
            other => panic!("accepted {args}: {other:?}"),
        }
    }
    assert!(!outside.join("pieces").exists(), "nothing written outside the root");
    assert!(!root.join("pieces").exists());
    // Without into, a confined call does not fall back to the folder beside the executable.
    assert!(a.call("chess_pieces_import", &json!({ "from": "course.html" })).is_err());
    let done = ok(&mut a, "chess_pieces_import", json!({ "from": "course.html", "into": "pieces" }));
    assert_eq!(done["imported"][0]["name"], "squares", "{done:#}");
    assert!(done["imported"][0]["token"].as_str().is_some_and(|t| t.starts_with("folder:")), "{done:#}");
    assert!(root.join("pieces/squares/wK.svg").is_file() && root.join("pieces/squares/ORIGIN.txt").is_file());
    let again = ok(&mut a, "chess_pieces_import", json!({ "from": "course.html", "into": "pieces" }));
    assert_eq!((again["imported"].as_array().map(Vec::len), again["exists"].as_array().map(Vec::len)), (Some(0), Some(1)), "{again:#}");
    let replaced = ok(&mut a, "chess_pieces_import", json!({ "from": "course.html", "into": "pieces", "replace": true }));
    assert_eq!(replaced["imported"][0]["name"], "squares");
    // A folder inside the root is copied too.
    ok(&mut a, "chess_pieces_import", json!({ "from": "pieces/squares", "into": "more" }));
    assert!(root.join("more/squares/bP.svg").is_file());
    let _ = std::fs::remove_dir_all(base);
}

/// Plan HC7: the diagrams' squares take a board theme (book hatches the dark squares), and the
/// default stays the boards' brown; the replacement draws the drawings asked for and says them in
/// words. A theme or drawing the tools do not know is refused.
#[test]
fn diagrams_take_a_board_theme_and_drawings() {
    let (base, root) = workdir("theme");
    let (mut a, doc) = opened(&root);
    let args = |theme: Option<&str>, out: &str| {
        let mut v = json!({ "doc": doc, "models_dir": "models", "out": out, "pieces": "geometric" });
        if let Some(t) = theme {
            v["board_theme"] = json!(t);
        }
        v
    };
    ok(&mut a, "chess_export_html", args(None, "brown.html"));
    ok(&mut a, "chess_export_html", args(Some("blue"), "blue.html"));
    ok(&mut a, "chess_export_html", args(Some("book"), "book.html"));
    let read = |name: &str| std::fs::read_to_string(root.join(name)).unwrap();
    let (brown, blue, book) = (read("brown.html"), read("blue.html"), read("book.html"));
    assert!(brown.contains("fill=\"#b58863\"") && !brown.contains("#7a96ac"), "brown by default");
    assert!(blue.contains("fill=\"#7a96ac\"") && !blue.contains("#b58863"), "blue's dark squares");
    let paths = |html: &str| html.matches("<path").count();
    assert_eq!(paths(&blue), paths(&brown), "a flat theme changes colours only");
    assert!(paths(&book) == paths(&brown) && book.len() > brown.len() + 2000, "the book's hatch: {} vs {}", book.len(), brown.len());
    match a.call("chess_export_html", &args(Some("cburnett"), "x.html")) {
        Err(ToolError::InvalidArgs(m)) => assert!(m.contains("board_theme") && m.contains("brown"), "{m}"),
        other => panic!("accepted an unknown theme: {other:?}"),
    }
    // The replacement: drawings described in words; bad ones refused.
    let generation = ok(&mut a, "chess_detect", json!({ "doc": doc }))["generation"].as_u64().unwrap();
    let replace = |drawings: &str| json!({ "doc": doc, "page": 1, "index": 1, "generation": generation, "placement": SICILIAN, "pieces": "geometric", "board_theme": "green", "drawings": drawings, "dry_run": true });
    let dry = ok(&mut a, "chess_replace_diagram", replace("[%cal Gb5c7][%csl Re4]"));
    assert_eq!(dry["drawings"], "red ring on e4, green arrow from b5 to c7", "{dry:#}");
    for bad in ["[%cal Xb5c7]", "an arrow", "[%csl Re9]"] {
        match a.call("chess_replace_diagram", &replace(bad)) {
            Err(ToolError::InvalidArgs(m)) => assert!(m.contains("drawings"), "{bad}: {m}"),
            other => panic!("accepted drawings {bad:?}: {other:?}"),
        }
    }
    let _ = std::fs::remove_dir_all(base);
}

/// A set PdfCraft ships is chosen by its id, whatever its case, and the result says it came from
/// the shipped sets.
#[test]
fn chess_replace_diagram_draws_with_a_shipped_piece_set() {
    let (base, root) = workdir("replace-shipped");
    let (mut a, doc) = opened(&root);
    let generation = ok(&mut a, "chess_detect", json!({ "doc": doc }))["generation"].as_u64().unwrap();
    for id in ["chessnut", "Kiwen-Suwi", "shapes"] {
        let args = json!({ "doc": doc, "page": 1, "index": 1, "generation": generation, "placement": SICILIAN, "pieces": id, "dry_run": true });
        let dry = ok(&mut a, "chess_replace_diagram", args);
        assert_eq!(dry["pieces"]["source"], "bundled", "{dry:#}");
        assert_eq!(dry["pieces"]["family"].as_str(), Some(id.to_ascii_lowercase().as_str()), "{dry:#}");
        assert!(dry["pieces"]["error"].is_null(), "{dry:#}");
    }
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
    // The figurine font is reported: used only when the machine has one configured.
    let font = &done["text_font"];
    assert_eq!(font["used"], font["path"].is_string() || font["format"].is_string(), "{font}");
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
    // The two unread diagrams were to get a game and did not, and the reply says why (plan A8 P8b).
    let skipped = pgn["positions_skipped"].as_array().unwrap();
    assert_eq!(skipped.len(), 2, "{pgn:#}");
    assert!(skipped.iter().all(|s| s["reason"] == "not_read" && s["page"].as_u64() >= Some(1) && s["diagram"].as_u64() >= Some(1)), "{pgn:#}");
    let none = ok(&mut a, "chess_export_pgn", json!({ "doc": doc, "out": "none.pgn", "positions": "none" }));
    assert_eq!((none["position_games"].as_u64(), none["positions_skipped"].as_array().map(Vec::len)), (Some(0), Some(0)), "{none:#}");
    assert!(!std::fs::read_to_string(root.join("none.pgn")).unwrap().contains("[PdfDiagram"));
    let bare = ok(&mut a, "chess_export_pgn", json!({ "doc": doc, "out": "bare.pgn", "comments": "none", "nags": "none", "variations": "main" }));
    assert_eq!(bare["position_games"], 1, "{bare:#}");
    let text = std::fs::read_to_string(root.join("bare.pgn")).unwrap();
    assert!(games.contains('{') && !text.contains('{') && text.contains("[PdfDiagram \"1\"]"), "{text}");
    let e = a.call("chess_export_pgn", &json!({ "doc": doc, "out": "x.pgn", "variations": "deep" })).unwrap_err();
    assert!(e.to_string().contains("variations"), "{e}");
    let e = a.call("chess_export_pgn", &json!({ "doc": doc, "out": "x.pgn", "positions": "some" })).unwrap_err();
    assert!(e.to_string().contains("positions"), "{e}");
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

/// cipher_out saves the book cipher, cipher_in starts from it; both stay in the root, and a
/// file that is not a cipher is refused.
#[test]
fn the_book_cipher_is_saved_and_read_back_inside_the_root() {
    let (base, root) = workdir("cipher");
    let (mut a, doc) = opened(&root);
    let first = ok(&mut a, "chess_document", json!({ "doc": doc, "cipher_out": "book.cipher.json" }));
    assert_eq!(first["cipher"]["saved"], "book.cipher.json", "{first:#}");
    let saved = std::fs::read_to_string(root.join("book.cipher.json")).unwrap();
    assert!(serde_json::from_str::<Value>(&saved).is_ok(), "{saved}");
    let again = ok(&mut a, "chess_export_pgn", json!({ "doc": doc, "out": "g.pgn", "cipher_in": "book.cipher.json" }));
    assert_eq!(again["cipher"]["rows"], first["cipher"]["rows"]);
    std::fs::write(root.join("bad.json"), "{ not json").unwrap();
    let e = a.call("chess_document", &json!({ "doc": doc, "cipher_in": "bad.json" })).unwrap_err();
    assert!(e.to_string().contains("bad.json"), "{e}");
    for (key, path) in [("cipher_in", "../outside/c.json"), ("cipher_out", "../outside/c.json")] {
        match a.call("chess_document", &json!({ "doc": doc, key: path })) {
            Err(ToolError::Failed(m)) => assert!(m.contains("is outside the allowed directory"), "{key}: {m}"),
            other => panic!("{key} accepted: {other:?}"),
        }
    }
    assert_eq!(std::fs::read_dir(base.join("outside")).unwrap().count(), 0);
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

/// chess_game plays a game without a document: from a FEN or a PGN, with moves in SAN or UCI,
/// shown at a ply, and stops at the first illegal move.
/// `chess_game`'s header is what the move list shows (plan HC10): PGN's placeholders are null, a
/// date reads as far as it is known, and the result is `*` while the game goes on.
#[test]
fn chess_game_says_what_the_move_list_header_shows() {
    let (base, root) = workdir("game-header");
    let mut a = Automation::new().with_root(&root).unwrap();
    let pgn = "[Event \"Club ch\"]
[Site \"?\"]
[Date \"2026.10.??\"]
[Round \"-\"]
[White \"Doe\"]
[Black \"Roe\"]
[Result \"*\"]
[WhiteElo \"2100\"]

1. f3 e5 2. g4 Qh4# *
";
    let g = ok(&mut a, "chess_game", json!({ "pgn": pgn }));
    let h = &g["header"];
    assert_eq!((&h["white"], &h["white_elo"], &h["black"], &h["black_elo"]), (&json!("Doe"), &json!("2100"), &json!("Roe"), &Value::Null), "{g:#}");
    assert_eq!((&h["event"], &h["site"], &h["date"], &h["round"]), (&json!("Club ch"), &Value::Null, &json!("2026.10"), &Value::Null), "{g:#}");
    assert_eq!(h["result"], "0-1", "the mate decides it: {g:#}");
    let fresh = ok(&mut a, "chess_game", json!({}));
    let fields = fresh["header"].as_object().unwrap();
    assert!(fields.iter().all(|(k, v)| if k == "result" { v == "*" } else { v.is_null() }), "{fresh:#}");
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn chess_game_plays_moves_from_a_fen_or_a_pgn() {
    let (_base, root) = workdir("game");
    let mut auto = Automation::new().with_root(&root).unwrap();
    let call = |auto: &mut Automation, args: Value| ok(auto, "chess_game", args);
    let r = call(&mut auto, json!({ "moves": ["e4", "e7e5", "Qh5", "Nc6", "Bc4", "Nf6", "Qxf7#"] }));
    assert_eq!(r["outcome"], "checkmate");
    assert_eq!(r["plies"], 7);
    assert_eq!(r["movetext"], "1. e4 e5 2. Qh5 Nc6 3. Bc4 Nf6 4. Qxf7#");
    assert!(r["pgn"].as_str().unwrap().contains("[Result \"1-0\"]"));
    assert_eq!(r["legal_moves"].as_array().unwrap().len(), 0);

    let r = call(&mut auto, json!({ "pgn": "1. e4 e5 (1... c5 2. Nf3) 2. Nf3 {develops} *", "ply": 1 }));
    assert_eq!(r["fen"], "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1");
    assert_eq!(r["side_to_move"], "black");
    assert_eq!(r["movetext"], "1. e4 e5 (1... c5 2. Nf3) 2. Nf3 {develops}");
    assert_eq!(r["legal_moves"].as_array().unwrap().len(), 20);

    let r = call(&mut auto, json!({ "fen": "4k3/8/8/8/8/8/8/R3K3 w Q - 0 1", "moves": ["O-O-O", "Ke7", "Kc3"] }));
    assert_eq!(r["movetext"], "1. O-O-O Ke7");
    assert_eq!(r["stopped"], "Kc3");

    for bad in [json!({ "fen": "x" }), json!({ "fen": "8/8/8/8/8/8/8/8", "pgn": "" }), json!({ "moves": "e4" }), json!({ "ply": -1 })] {
        assert!(auto.call("chess_game", &bad).is_err(), "{bad} is refused");
    }
}

/// The position before White's 18th move in the PGN toolbar spec's scenario.
const SCENARIO: &str = "2r2rk1/1p2ppb1/p2p1npp/q4P2/P1b1P2B/2NB4/1PP3PP/R3QR1K w - - 0 18";

/// A `move` fragment item for chess_pgn_edit; `n.` or `n...` before the SAN gives its half-move.
fn item_move(text: &str) -> Value {
    match text.split_once('.') {
        Some((number, san)) => {
            let number: u64 = number.parse().unwrap();
            let black = san.starts_with("..");
            json!({ "item": "move", "san": san.trim_start_matches('.'), "ply": 2 * (number - 1) + u64::from(black) })
        }
        None => json!({ "item": "move", "san": text }),
    }
}

/// chess_pgn_edit with `ops` on the scenario's game (its main line when `main` is set).
fn pgn_edit(auto: &mut Automation, main: bool, ops: Value) -> Value {
    let mut args = if main {
        let r = ok(
            auto,
            "chess_pgn_edit",
            json!({ "fen": SCENARIO, "ops": [{ "op": "play", "at": "main:34", "moves": ["Nd5", "Qxe1", "Nxe7+", "Kh7", "Raxe1"] }] }),
        );
        json!({ "pgn": r["pgn"] })
    } else {
        json!({ "fen": SCENARIO })
    };
    args["ops"] = ops;
    ok(auto, "chess_pgn_edit", args)
}

/// The scenario's variation as fragment items: `18...Qd8 19.Bxf6 Bxf6 20.fxg6 (20.Nxf6+ exf6
/// 21.Bxc4 Rxc4) 20...fxg6 21.Qg3 Kh7 22.e5`.
fn variation_items() -> Value {
    let mut items: Vec<Value> = ["18...Qd8", "19.Bxf6", "Bxf6", "20.fxg6"].iter().map(|m| item_move(m)).collect();
    items.push(json!({ "item": "open" }));
    items.extend(["20.Nxf6+", "exf6", "21.Bxc4", "Rxc4"].iter().map(|m| item_move(m)));
    items.push(json!({ "item": "close" }));
    items.extend(["20...fxg6", "21.Qg3", "Kh7", "22.e5"].iter().map(|m| item_move(m)));
    items.push(json!({ "item": "result" }));
    Value::Array(items)
}

const MAIN: &str = "18. Nd5 Qxe1 19. Nxe7+ Kh7 20. Raxe1";
const WITH_VARIATION: &str =
    "18. Nd5 Qxe1 (18... Qd8 19. Bxf6 Bxf6 20. fxg6 (20. Nxf6+ exf6 21. Bxc4 Rxc4) 20... fxg6 21. Qg3 Kh7 22. e5) 19. Nxe7+ Kh7 20. Raxe1";

/// Plan A8 V4: chess_pgn_edit says which variations of the PGN it kept as comments, and writes
/// the game leaving out what variations, comments and nags say.
#[test]
fn chess_pgn_edit_says_what_it_kept_as_a_comment_and_writes_as_asked() {
    let (_base, root) = workdir("pgn-style");
    let mut auto = Automation::new().with_root(&root).unwrap();
    let pgn = "1. e4 e5 ((1... d5)) (1... c5 {Sicilian} 2. Nf3 (2. c3 $1)) 2. Nf3 *";
    let r = ok(&mut auto, "chess_pgn_edit", json!({ "pgn": pgn, "ops": [] }));
    assert_eq!(r["pgn_warnings"], json!([{ "kind": "orphan_variation", "at": "path:0" }]), "{r:#}");
    assert!(r["movetext"].as_str().unwrap().contains("(d5)"), "{r:#}");
    let main = ok(&mut auto, "chess_pgn_edit", json!({ "pgn": pgn, "ops": [], "variations": "main", "comments": "none" }));
    assert_eq!(main["movetext"], "1. e4 e5 2. Nf3", "{main:#}");
    assert!(
        main["pgn"].as_str().unwrap().ends_with(
            "

1. e4 e5 2. Nf3 *
"
        ),
        "{main:#}"
    );
    let one = ok(&mut auto, "chess_pgn_edit", json!({ "pgn": pgn, "ops": [], "variations": "depth:1", "nags": "none" }));
    assert_eq!(one["movetext"], "1. e4 {(d5)} 1... e5 (1... c5 {Sicilian} 2. Nf3) 2. Nf3", "{one:#}");
    for (key, value) in [("variations", "depth:x"), ("comments", "some"), ("nags", "")] {
        let e = auto.call("chess_pgn_edit", &json!({ "pgn": pgn, "ops": [], key: value })).unwrap_err();
        assert!(e.to_string().contains(key), "{key}: {e}");
    }
}

/// Plan HC11: next_variation and prev_variation walk the variations in reading order, four levels
/// deep, and say when there is nowhere to go.
#[test]
fn chess_pgn_edit_walks_from_variation_to_variation() {
    let (_base, root) = workdir("pgn-variations");
    let mut auto = Automation::new().with_root(&root).unwrap();
    let pgn = "1. e4 e5 (1... c5 2. Nf3 (2. c3 d5 (2... Nf6 3. e5 (3. d3 d5))) d6) (1... e6) 2. Nf3 *";
    // From the start: a game read from PGN shows the end of its main line.
    let mut ops = vec![json!({ "op": "next_variation", "at": "path:" })];
    ops.extend((0..5).map(|_| json!({ "op": "next_variation" })));
    ops.push(json!({ "op": "prev_variation" }));
    let r = ok(&mut auto, "chess_pgn_edit", json!({ "pgn": pgn, "ops": ops }));
    let report = r["report"].as_array().unwrap();
    let paths: Vec<&str> = report.iter().map(|e| e["path"].as_str().unwrap()).collect();
    assert_eq!(paths, ["path:0.1", "path:0.1.1", "path:0.1.1.1", "path:0.1.1.1.1", "path:0.2", "path:0.2", "path:0.1.1.1.1"]);
    let moved: Vec<bool> = report.iter().map(|e| e["moved"].as_bool().unwrap()).collect();
    assert_eq!(moved, [true, true, true, true, true, false, true]);
}

/// chess_pgn_edit steps through the game: back out of a variation and forward again returns into
/// it, and choose picks a reply by its index (the board's ◀, ▶ and continuations).
#[test]
fn chess_pgn_edit_steps_back_and_forward_along_the_line_last_seen() {
    let (_base, root) = workdir("pgn-steps");
    let mut auto = Automation::new().with_root(&root).unwrap();
    let pgn = "1. e4 e5 2. Nf3 Nc6 (2... Nf6 3. Bc4) 3. Bb5 *";
    let ops = json!([
        { "op": "back", "at": "path:0.0.0.1.0", "n": 3 },
        { "op": "forward", "n": 3 },
        { "op": "forward" },
        { "op": "choose", "at": "path:0.0.0", "index": 0 },
        { "op": "back", "n": 99 },
        { "op": "choose", "at": "path:0.0.0", "index": 2 }
    ]);
    let r = ok(&mut auto, "chess_pgn_edit", json!({ "pgn": pgn, "ops": ops }));
    let report = r["report"].as_array().unwrap();
    assert_eq!((report[0]["path"].as_str(), report[0]["moved"].as_u64()), (Some("path:0.0"), Some(3)));
    // Forward retraces the variation, not the main line.
    assert_eq!((report[1]["path"].as_str(), report[1]["moved"].as_u64()), (Some("path:0.0.0.1.0"), Some(3)));
    assert_eq!(report[2]["moved"], 0, "the end of the variation");
    assert_eq!(report[3]["path"], "path:0.0.0.0");
    assert_eq!((report[4]["path"].as_str(), report[4]["moved"].as_u64()), (Some("path:"), Some(4)));
    assert_eq!(report[5]["ok"], false);
    assert!(report[5]["error"].as_str().unwrap().contains("2 replies"), "{}", report[5]);
    assert_eq!(r["applied"], 5);
    // Nothing was lost on the way.
    assert_eq!(r["movetext"], "1. e4 e5 2. Nf3 Nc6 (2... Nf6 3. Bc4) 3. Bb5");
    // Other ops still need at.
    let r = ok(&mut auto, "chess_pgn_edit", json!({ "pgn": pgn, "ops": [{ "op": "cut" }] }));
    assert!(r["report"][0]["error"].as_str().unwrap().contains("at is missing"));
}

/// chess_pgn_edit plays, comments, sets glyphs, reshapes variations and grafts fragments, each
/// at a main-line half-move or a path, and reports every operation.
#[test]
fn chess_pgn_edit_applies_every_operation() {
    let (_base, root) = workdir("pgn-edit");
    let mut auto = Automation::new().with_root(&root).unwrap();
    let a = &mut auto;

    // play, from the start (main:34, the half-move of 18. Nd5).
    let r = pgn_edit(a, true, json!([]));
    assert_eq!((r["movetext"].as_str().unwrap(), r["applied"].as_u64().unwrap()), (MAIN, 0));
    assert!(r["pgn"].as_str().unwrap().contains("[FEN \"2r2rk1/"));
    let r = pgn_edit(a, false, json!([{ "op": "play", "at": "main:34", "moves": ["Nd5", "Qxe1"] }]));
    assert_eq!((r["movetext"].as_str(), r["current_path"].as_str()), (Some("18. Nd5 Qxe1"), Some("path:0.0")));
    assert_eq!(r["report"][0]["path"], "path:0.0");

    // graft: the variation after 18. Nd5 (main:35), in follow mode.
    let r = pgn_edit(a, true, json!([{ "op": "graft", "at": "main:35", "items": variation_items() }]));
    assert_eq!(r["movetext"], WITH_VARIATION);
    let g = &r["report"][0];
    assert_eq!((g["ok"].as_bool(), g["op"].as_str(), g["path"].as_str()), (Some(true), Some("graft"), Some("path:0.1.0.0.0.0.0.0.0")));
    assert_eq!(g["graft"]["played"], 12);
    assert_eq!(g["graft"]["stop"], Value::Null);
    assert_eq!(g["graft"]["new_nodes"][0], "path:0.1");
    assert_eq!(g["graft"]["anchor"], "path:0");
    assert_eq!(g["graft"]["unbalanced"], json!([]));
    assert_eq!(r["current_path"], "path:0.1.0.0.0.0.0.0.0");

    // Line mode demotes the move there; glyphs are given by White's code.
    let line = json!([{ "op": "graft", "at": "path:0", "mode": "line", "items": [item_move("18...Qd8"), { "item": "nag", "code": 36 }, { "item": "comment", "text": "solid" }, { "item": "close" }] }]);
    let r = pgn_edit(a, true, line);
    assert_eq!(r["movetext"], "18. Nd5 Qd8 $37 {solid} (18... Qxe1 19. Nxe7+ Kh7 20. Raxe1)");
    assert_eq!(r["report"][0]["graft"]["demoted"], true);
    assert_eq!(r["report"][0]["graft"]["unbalanced"], json!([3]));

    // A graft that stops reports where and why, and keeps what played.
    let r = pgn_edit(
        a,
        true,
        json!([{ "op": "graft", "at": "main:35", "items": [item_move("18...Qd8"), item_move("19.Qh1"), { "item": "comment", "text": "lost" }] }]),
    );
    let g = &r["report"][0]["graft"];
    assert_eq!((g["stop"]["index"].as_u64(), g["stop"]["reason"].as_str()), (Some(1), Some("illegal")));
    assert_eq!(g["leftover_text"], "Qh1 lost");
    assert!(r["movetext"].as_str().unwrap().starts_with("18. Nd5 Qxe1 (18... Qd8) 19. Nxe7+"));

    // Comment mode writes the items as text.
    let r = pgn_edit(a, true, json!([{ "op": "graft", "at": "path:0", "mode": "comment", "items": [item_move("18...Qd8"), item_move("19.Bxf6")] }]));
    assert_eq!(r["movetext"], "18. Nd5 {18...Qd8 19.Bxf6} 18... Qxe1 19. Nxe7+ Kh7 20. Raxe1");

    // comment, comment_before and nag on the variation's first move, then the structure ops.
    let ops = json!([
        { "op": "graft", "at": "main:35", "items": variation_items() },
        { "op": "comment", "at": "path:0.1", "text": "solid" },
        { "op": "comment_before", "at": "path:0.1", "text": "the try" },
        { "op": "nag", "at": "path:0.1", "code": 5 },
    ]);
    let r = pgn_edit(a, true, ops.clone());
    assert!(r["movetext"].as_str().unwrap().starts_with("18. Nd5 Qxe1 ({the try} 18... Qd8 $5 {solid} 19. Bxf6"), "{}", r["movetext"]);
    assert_eq!(r["applied"], 4);
    assert_eq!(r["report"][3]["path"], "path:0.1");

    let with = |extra: Value| {
        let mut all = ops.as_array().unwrap().clone();
        all.extend(extra.as_array().unwrap().iter().cloned());
        Value::Array(all)
    };
    // The comment before a first reply is written with the comment before it (PGN has no other
    // place for it), and stays on the move.
    let r = pgn_edit(a, true, with(json!([{ "op": "promote_one", "at": "path:0.1" }])));
    assert!(
        r["movetext"].as_str().unwrap().starts_with("18. Nd5 {the try} 18... Qd8 $5 {solid} (18... Qxe1 19. Nxe7+ Kh7 20. Raxe1) 19. Bxf6"),
        "{}",
        r["movetext"]
    );
    let r = pgn_edit(a, true, with(json!([{ "op": "promote_one", "at": "path:0.1" }, { "op": "demote", "at": "path:0.0" }])));
    assert!(r["movetext"].as_str().unwrap().starts_with("18. Nd5 Qxe1 ({the try} 18... Qd8 $5 {solid}"), "{}", r["movetext"]);
    // make_main: 20. Nxf6+ (the variation's variation) becomes the main line.
    let r = pgn_edit(a, true, with(json!([{ "op": "make_main", "at": "path:0.1.0.0.1" }])));
    assert!(r["movetext"].as_str().unwrap().starts_with("18. Nd5 {the try} 18... Qd8 $5 {solid} (18... Qxe1"), "{}", r["movetext"]);
    assert!(r["movetext"].as_str().unwrap().contains("20. Nxf6+ (20. fxg6 fxg6 21. Qg3 Kh7 22. e5) 20... exf6 21. Bxc4 Rxc4"), "{}", r["movetext"]);
    assert_eq!(r["report"][4]["path"], "path:0.0.0.0.0");
    let r = pgn_edit(a, true, with(json!([{ "op": "delete_variation", "at": "path:0.1" }])));
    assert_eq!(r["movetext"], MAIN);
    let r = pgn_edit(a, true, json!([{ "op": "cut", "at": "main:36" }]));
    assert_eq!(r["movetext"], "18. Nd5 Qxe1");
    assert_eq!(r["current_path"], "path:0.0");

    // A PGN move that cannot be played is said, not dropped silently.
    let r = ok(a, "chess_pgn_edit", json!({ "pgn": "1. e4 e5 2. Ke3 Nc6 *", "ops": [] }));
    assert_eq!((r["movetext"].as_str(), r["pgn_stopped"].as_str()), (Some("1. e4 e5"), Some("Ke3")));
    assert_eq!(pgn_edit(a, false, json!([]))["pgn_stopped"], Value::Null);

    // play follows a move already there, and takes UCI too.
    let r =
        pgn_edit(a, true, json!([{ "op": "play", "at": "path:0", "moves": ["Qxe1", "d5e7"] }, { "op": "play", "at": "path:0", "moves": ["a5d8"] }]));
    assert_eq!(r["movetext"], "18. Nd5 Qxe1 (18... Qd8) 19. Nxe7+ Kh7 20. Raxe1");
}

/// T15: a null move (a threat shown by passing), a variation moved among the others at its fork
/// and the diagram mark, each refused where the game does not allow it.
#[test]
fn chess_pgn_edit_plays_null_moves_reorders_variations_and_marks_diagrams() {
    let (_base, root) = workdir("pgn-edit-t15");
    let mut auto = Automation::new().with_root(&root).unwrap();
    let a = &mut auto;
    let forks = "1. e4 e5 (1... c5) (1... e6) (1... d5) 2. Nf3 *";
    let edit = |a: &mut Automation, ops: Value| ok(a, "chess_pgn_edit", json!({ "pgn": forks, "ops": ops }));

    // A null move after 2. Nf3 (Black passes), then White's threat.
    let r = edit(a, json!([{ "op": "play_null", "at": "path:0.0.0" }, { "op": "play", "at": "path:0.0.0.0", "moves": ["Nxe5"] }]));
    assert_eq!(r["movetext"], "1. e4 e5 (1... c5) (1... e6) (1... d5) 2. Nf3 -- 3. Nxe5");
    assert_eq!(r["report"][0]["path"], "path:0.0.0.0");
    // Read back from its own PGN: the same text.
    let back = ok(a, "chess_pgn_edit", json!({ "pgn": r["pgn"], "ops": [] }));
    assert_eq!((back["movetext"].clone(), back["pgn_stopped"].clone()), (r["movetext"].clone(), Value::Null));

    // 1...e6 up before 1...c5, then down again; 1...d5 down is refused (it is the last).
    let r = edit(a, json!([{ "op": "move_variation", "at": "path:0.2", "to": "up" }]));
    assert_eq!(r["movetext"], "1. e4 e5 (1... e6) (1... c5) (1... d5) 2. Nf3");
    assert_eq!(r["report"][0]["path"], "path:0.0.0", "the move shown stays");
    let r = edit(a, json!([{ "op": "move_variation", "at": "path:0.2", "to": "up" }, { "op": "move_variation", "at": "path:0.1", "to": "down" }]));
    assert_eq!(r["movetext"], "1. e4 e5 (1... c5) (1... e6) (1... d5) 2. Nf3");

    // The diagram mark, on and off.
    let r = edit(a, json!([{ "op": "diagram", "at": "main:2" }]));
    assert_eq!(r["movetext"], "1. e4 e5 $220 (1... c5) (1... e6) (1... d5) 2. Nf3");
    let r = edit(
        a,
        json!([{ "op": "diagram", "at": "main:2" }, { "op": "diagram", "at": "main:2", "on": false }, { "op": "diagram", "at": "main:2", "on": false }]),
    );
    assert_eq!((r["movetext"].as_str(), r["applied"].as_u64()), (Some("1. e4 e5 (1... c5) (1... e6) (1... d5) 2. Nf3"), Some(3)));

    for (op, needle) in [
        (json!({ "op": "move_variation", "at": "path:0.1", "to": "up" }), "no variation on that side"),
        (json!({ "op": "move_variation", "at": "path:0.3", "to": "down" }), "no variation on that side"),
        (json!({ "op": "move_variation", "at": "path:0.0", "to": "down" }), "already continues its line"),
        (json!({ "op": "move_variation", "at": "path:0.2" }), "up or down"),
        (json!({ "op": "diagram", "at": "path:" }), "cannot go on this move"),
        (json!({ "op": "diagram", "at": "path:0", "on": "yes" }), "true or false"),
    ] {
        let r = edit(a, json!([op]));
        assert_eq!(r["report"][0]["ok"], false, "{op}");
        assert!(r["report"][0]["error"].as_str().unwrap().contains(needle), "{op}: {}", r["report"][0]["error"]);
        assert_eq!(r["movetext"], "1. e4 e5 (1... c5) (1... e6) (1... d5) 2. Nf3", "{op} changed nothing");
    }
    // A side in check cannot pass.
    let r = ok(a, "chess_pgn_edit", json!({ "pgn": "1. e4 f5 2. Qh5+ *", "ops": [{ "op": "play_null", "at": "main:3" }] }));
    assert_eq!(r["report"][0]["ok"], false);
    assert!(r["report"][0]["error"].as_str().unwrap().contains("in check"), "{}", r["report"][0]["error"]);
}

/// A refused operation says why, changes nothing and stops the list; arguments that are wrong as
/// a whole are refused before anything runs.
#[test]
fn chess_pgn_edit_refuses_what_the_game_does_not_allow() {
    let (_base, root) = workdir("pgn-edit-refusals");
    let mut auto = Automation::new().with_root(&root).unwrap();
    let a = &mut auto;
    let refused = [
        (json!({ "op": "play", "at": "main:99", "moves": ["e4"] }), "no move there"),
        (json!({ "op": "play", "at": "path:7", "moves": ["e4"] }), "no such move"),
        (json!({ "op": "play", "at": "path:0.x", "moves": ["e4"] }), "reply indices"),
        (json!({ "op": "play", "at": "18.Nd5", "moves": ["e4"] }), "an address is"),
        (json!({ "op": "play", "moves": ["e4"] }), "at is missing"),
        (json!({ "op": "play", "at": "path:0", "moves": ["Qd8", "Qh1"] }), "Qh1 cannot be played"),
        (json!({ "op": "comment", "at": "path:0", "text": "x".repeat(4097) }), "longer than"),
        (json!({ "op": "comment_before", "at": "path:", "text": "x" }), "the start"),
        (json!({ "op": "nag", "at": "path:0", "code": 0 }), "cannot be put"),
        (json!({ "op": "nag", "at": "path:", "code": 1 }), "cannot be put"),
        (json!({ "op": "nag", "at": "path:0", "code": "!" }), "PGN code"),
        (json!({ "op": "promote_one", "at": "path:0.0" }), "already continues its line"),
        (json!({ "op": "demote", "at": "path:0.0" }), "no variation to take its place"),
        (json!({ "op": "delete_variation", "at": "path:0" }), "already continues its line"),
        (json!({ "op": "promote_one", "at": "path:" }), "no such move"),
        (json!({ "op": "graft", "at": "path:0", "mode": "sideways", "items": [] }), "follow, line or comment"),
        (json!({ "op": "graft", "at": "path:0", "items": [{ "item": "move", "san": "Zz9" }] }), "items[0]"),
        (json!({ "op": "graft", "at": "path:0", "items": [{ "item": "nag" }] }), "items[0]"),
        (json!({ "op": "graft", "at": "path:0", "items": [{ "item": "jump" }] }), "unknown item"),
        (json!({ "op": "rotate", "at": "path:0" }), "unknown op"),
        (json!("cut"), "an object"),
    ];
    for (op, needle) in refused {
        let r = pgn_edit(a, true, json!([op, { "op": "cut", "at": "path:" }]));
        assert_eq!(r["movetext"], MAIN, "{op} changed the game");
        assert_eq!(r["applied"], 0, "{op}");
        let report = r["report"].as_array().unwrap();
        assert_eq!(report.len(), 1, "{op}: the list stops at the refusal");
        assert_eq!(report[0]["ok"], false);
        assert!(report[0]["error"].as_str().unwrap().contains(needle), "{op}: {}", report[0]["error"]);
    }
    // Moves already played by a refused play are taken back too.
    let r = pgn_edit(a, true, json!([{ "op": "play", "at": "path:0.0.0.0.0", "moves": ["Rf8", "Qh1"] }]));
    assert_eq!(r["movetext"], MAIN);

    let too_many: Vec<Value> = (0..2001).map(|_| json!({ "op": "cut", "at": "path:" })).collect();
    let too_long: Vec<Value> = (0..11).map(|_| json!({ "op": "play", "at": "path:", "moves": vec!["Nd5"; 2000] })).collect();
    for bad in [
        json!({ "fen": SCENARIO, "pgn": "", "ops": [] }),
        json!({ "fen": "x", "ops": [] }),
        json!({ "ops": "cut" }),
        json!({}),
        json!({ "ops": too_many }),
        json!({ "fen": SCENARIO, "ops": too_long }),
    ] {
        assert!(matches!(a.call("chess_pgn_edit", &bad), Err(ToolError::InvalidArgs(_))), "{bad} is refused");
    }
}

/// chess_pgn_edit edits any game of a PGN that holds several: `game` (0-based, the first by
/// default) picks it, only that game comes back, the reply says how many the PGN holds, and an
/// index past the last (or a PGN with more games than a collection takes) is refused.
#[test]
fn chess_pgn_edit_edits_the_game_it_is_given_of_several() {
    let mut auto = Automation::new();
    let a = &mut auto;
    let three = "[White \"A\"]\n\n1. e4 e5 *\n\n[White \"B\"]\n\n1. d4 d5 *\n\n[White \"C\"]\n\n1. c4 { a comment\n[split] over two lines } *\n";
    let first = ok(a, "chess_pgn_edit", json!({ "pgn": three, "ops": [] }));
    assert_eq!((first["games"].as_u64(), first["game"].as_u64()), (Some(3), Some(0)), "{first:#}");
    assert!(first["movetext"].as_str().unwrap().starts_with("1. e4 e5"), "{first:#}");
    let second = ok(a, "chess_pgn_edit", json!({ "pgn": three, "game": 1, "ops": [{ "op": "play", "at": "main:2", "moves": ["Nc3"] }] }));
    assert_eq!((second["games"].as_u64(), second["game"].as_u64(), second["applied"].as_u64()), (Some(3), Some(1), Some(1)), "{second:#}");
    assert!(second["movetext"].as_str().unwrap().starts_with("1. d4 d5 2. Nc3"), "{second:#}");
    let pgn = second["pgn"].as_str().unwrap();
    assert!(pgn.contains("[White \"B\"]") && !pgn.contains("[White \"A\"]") && !pgn.contains("[White \"C\"]"), "only game 1 comes back: {pgn}");
    let last = ok(a, "chess_pgn_edit", json!({ "pgn": three, "game": 2, "ops": [] }));
    assert!(last["pgn"].as_str().unwrap().contains("[White \"C\"]"), "a line of a comment starting with [ does not start a game: {last:#}");
    // One game, and no game at all (one blank game, as a collection reads it).
    for (pgn, games) in [("1. e4 *", 1), ("", 1)] {
        let r = ok(a, "chess_pgn_edit", json!({ "pgn": pgn, "game": 0, "ops": [] }));
        assert_eq!(r["games"].as_u64(), Some(games), "{pgn:?}: {r:#}");
    }
    assert_eq!(ok(a, "chess_pgn_edit", json!({ "ops": [] }))["games"].as_u64(), Some(0), "no pgn, no games");
    let many: String = (0..257).map(|i| format!("[Round \"{i}\"]\n\n1. e4 *\n\n")).collect();
    for (bad, says) in [
        (json!({ "pgn": three, "game": 3, "ops": [] }), "holds 3 games, 0 to 2"),
        (json!({ "pgn": "1. e4 *", "game": 1, "ops": [] }), "holds 1 game, 0 to 0"),
        (json!({ "pgn": three, "game": -1, "ops": [] }), "game must be"),
        (json!({ "pgn": three, "game": "1", "ops": [] }), "game must be"),
        (json!({ "fen": SCENARIO, "game": 0, "ops": [] }), "give pgn"),
        (json!({ "game": 0, "ops": [] }), "give pgn"),
        (json!({ "pgn": many, "game": 0, "ops": [] }), "at most 256 games"),
    ] {
        match a.call("chess_pgn_edit", &bad) {
            Err(ToolError::InvalidArgs(m)) => assert!(m.contains(says), "{says}: {m}"),
            other => panic!("chess_pgn_edit accepted {says}: {other:?}"),
        }
    }
}

/// chess_pgn_edit works only on the game in its arguments: it is read-only and no argument, at
/// any depth of its schema, is a path (so it needs no --root confinement).
#[test]
fn chess_pgn_edit_takes_no_path() {
    let def = pdfcraft_automation::tools().into_iter().find(|t| t.name == "chess_pgn_edit").unwrap();
    assert!(def.read_only && !def.destructive);
    let mut names = Vec::new();
    let mut stack = vec![&def.input_schema];
    while let Some(schema) = stack.pop() {
        if let Some(props) = schema["properties"].as_object() {
            for (name, spec) in props {
                names.push(name.clone());
                stack.push(spec);
            }
        }
        if schema["items"].is_object() {
            stack.push(&schema["items"]);
        }
    }
    assert!(names.contains(&"san".to_owned()), "the nested properties are walked: {names:?}");
    let pathish = ["path", "paths", "folder", "out_dir", "file", "out", "image", "dir", "models_dir", "cipher_in", "cipher_out", "pgn_out"];
    for name in &names {
        assert!(!pathish.contains(&name.as_str()), "{name} looks like a path");
        assert!(!name.contains("path") && !name.contains("file") && !name.contains("dir"), "{name} looks like a path");
    }
}

/// The scenario's paragraph as the book sets it, `*` toggling bold: the game's moves in bold.
const PARAGRAPH: &str = "*18.Nd5!* *Qxe1* After 18…Qd8 19.Bxf6! Bxf6 20.fxg6 (less good is
20.Nxf6+, as Black has nothing to fear after 20…exf6 21.Bxc4
Rxc4) 20…fxg6 21.Qg3, White obtains a dangerous initiative. For
example: 21…Kh7 22.e5!. *19.Nxe7+ Kh7 20.Raxe1*";

/// A capture op on `marked` (see [`PARAGRAPH`]): its text, and its bold runs when it has any.
fn capture_op(at: &str, mode: &str, marked: &str) -> Value {
    let (mut text, mut runs, mut open) = (String::new(), Vec::new(), None);
    for c in marked.chars() {
        if c == '*' {
            let here = text.chars().count();
            match open.take() {
                Some(start) => runs.push(json!([start, here])),
                None => open = Some(here),
            }
        } else {
            text.push(c);
        }
    }
    let mut op = json!({ "op": "capture", "at": at, "mode": mode, "text": text });
    if !runs.is_empty() {
        op["bold_runs"] = Value::Array(runs);
    }
    op
}

/// Movetext without comments, and without the number PGN repeats for a Black move after one.
fn without_comments(movetext: &str) -> String {
    let mut flat = String::new();
    let mut depth = 0usize;
    for c in movetext.chars() {
        match c {
            '{' => {
                if depth == 0 {
                    flat.push_str(" {} ");
                }
                depth += 1;
            }
            '}' => depth = depth.saturating_sub(1),
            '(' | ')' if depth == 0 => flat.push_str(&format!(" {c} ")),
            _ if depth == 0 => flat.push(c),
            _ => {}
        }
    }
    let words: Vec<&str> = flat.split_whitespace().collect();
    let mut kept: Vec<&str> = Vec::new();
    for (k, w) in words.iter().enumerate() {
        let repeated = w.ends_with("...") && k >= 2 && words[k - 1] == "{}" && !matches!(words[k - 2], "(" | ")");
        if *w != "{}" && !repeated {
            kept.push(w);
        }
    }
    kept.join(" ").replace("( ", "(").replace(" )", ")")
}

fn capture_of(r: &Value, index: usize) -> &Value {
    let entry = &r["report"][index];
    assert_eq!(entry["ok"], true, "{entry}");
    &entry["capture"]
}

/// §4.10 step 2: the whole paragraph, bold main line marked, in one capture: main line, variation,
/// sub-variation and comments, applied without the preview (complete, one place).
#[test]
fn chess_pgn_edit_captures_the_scenario_paragraph_at_once() {
    let (_base, root) = workdir("pgn-capture");
    let mut auto = Automation::new().with_root(&root).unwrap();
    let mut op = capture_op("path:", "auto", PARAGRAPH);
    op["apply"] = json!("auto");
    let r = pgn_edit(&mut auto, false, json!([op]));
    let c = capture_of(&r, 0);
    assert_eq!((c["applied"].as_bool(), c["needs_preview"].as_bool()), (Some(true), Some(false)), "{c}");
    assert_eq!(c["reasons"], json!([]));
    assert_eq!(c["candidates"].as_array().unwrap().len(), 1);
    assert_eq!((c["candidates"][0]["mode"].as_str(), c["candidates"][0]["played"].as_u64()), (Some("line"), Some(17)));
    let movetext = r["movetext"].as_str().unwrap();
    assert_eq!(
        without_comments(movetext),
        "18. Nd5 $1 Qxe1 (18... Qd8 19. Bxf6 $1 Bxf6 20. fxg6 (20. Nxf6+ exf6 21. Bxc4 Rxc4) 20... fxg6 21. Qg3 Kh7 22. e5 $1) 19. Nxe7+ Kh7 20. Raxe1"
    );
    assert!(movetext.contains("{less good is} 20. Nxf6+ {as Black has nothing to fear after}"), "{movetext}");
    assert_eq!(c["candidates"][0]["movetext"], r["movetext"]);
    assert_eq!(r["current_path"], "path:0.0.0.0.0");
}

/// §4.10 step 3: prose `20…Rc7 21.e5!` extends the main line's end (preview: tip_extension); the
/// bold `20…Rc7 21.Bxc4 Rxc4` then follows 20…Rc7 and demotes 21.e5 (preview: demotes), which
/// `apply: auto` leaves for the preview and the default applies.
#[test]
fn chess_pgn_edit_capture_on_the_line_demotes_a_different_move() {
    let (_base, root) = workdir("pgn-capture-demote");
    let mut auto = Automation::new().with_root(&root).unwrap();
    let a = &mut auto;
    let extend = capture_op("main:39", "auto", "20…Rc7 21.e5!");
    let r = pgn_edit(a, true, json!([extend]));
    let c = capture_of(&r, 0);
    assert_eq!((c["applied"].as_bool(), c["needs_preview"].as_bool()), (Some(true), Some(true)));
    assert_eq!(c["reasons"], json!(["tip_extension"]));
    assert_eq!(r["movetext"], "18. Nd5 Qxe1 19. Nxe7+ Kh7 20. Raxe1 Rc7 21. e5 $1");

    let mut bold = capture_op("main:39", "auto", "*20…Rc7 21.Bxc4 Rxc4*");
    bold["apply"] = json!("auto");
    let r = pgn_edit(a, true, json!([extend, bold]));
    let c = capture_of(&r, 1);
    assert_eq!((c["applied"].as_bool(), c["needs_preview"].as_bool()), (Some(false), Some(true)));
    assert_eq!(c["reasons"], json!(["demotes"]));
    let candidate = &c["candidates"][0];
    assert_eq!((candidate["mode"].as_str(), candidate["graft"]["demoted"].as_bool()), (Some("line"), Some(true)));
    assert_eq!(candidate["movetext"], "18. Nd5 Qxe1 19. Nxe7+ Kh7 20. Raxe1 Rc7 21. Bxc4 (21. e5 $1) 21... Rxc4");
    assert_eq!(r["movetext"], "18. Nd5 Qxe1 19. Nxe7+ Kh7 20. Raxe1 Rc7 21. e5 $1", "not applied");
    assert_eq!(r["current_path"], "path:0.0.0.0.0");

    bold["apply"] = json!("chosen");
    let r = pgn_edit(a, true, json!([extend, bold]));
    assert_eq!(capture_of(&r, 1)["applied"], true);
    assert_eq!(r["movetext"], "18. Nd5 Qxe1 19. Nxe7+ Kh7 20. Raxe1 Rc7 21. Bxc4 (21. e5 $1) 21... Rxc4");
    assert_eq!(r["current_path"], "path:0.0.0.0.0.0.0.0");
}

/// §4.10 step 4: the forced modes from the empty game. The last, `20…fxg6 21.Qg3` as the line
/// from inside the sub-variation, cannot follow 20.Nxf6+ and lands on L1, the variation it left.
#[test]
fn chess_pgn_edit_capture_in_forced_modes() {
    let (_base, root) = workdir("pgn-capture-forced");
    let mut auto = Automation::new().with_root(&root).unwrap();
    let ops = json!([
        capture_op("path:", "line", "18.Nd5! Qxe1"),
        capture_op("path:0.0", "variation", "18…Qd8 19.Bxf6! Bxf6 20.fxg6"),
        capture_op("path:0.1.0.0.0", "subvariation", "20.Nxf6+"),
        capture_op("path:0.1.0.0.1", "line", "20…exf6 21.Bxc4 Rxc4"),
        capture_op("path:0.1.0.0.1.0.0.0", "line", "20…fxg6 21.Qg3"),
    ]);
    let r = pgn_edit(&mut auto, false, ops);
    assert_eq!(r["applied"], 5, "{}", r["report"]);
    let paths: Vec<&str> = (0..5).map(|i| r["report"][i]["path"].as_str().unwrap()).collect();
    assert_eq!(paths, vec!["path:0.0", "path:0.1.0.0.0", "path:0.1.0.0.1", "path:0.1.0.0.1.0.0.0", "path:0.1.0.0.0.0.0"]);
    for i in 0..4 {
        let c = capture_of(&r, i);
        assert_eq!((c["needs_preview"].as_bool(), c["applied"].as_bool()), (Some(false), Some(true)), "op {i}: {c}");
    }
    let last = capture_of(&r, 4);
    assert_eq!(last["reasons"], json!(["outer_line"]));
    assert_eq!(last["chosen"], 1, "the best candidate, not the first");
    let candidates = last["candidates"].as_array().unwrap();
    let summary: Vec<(u64, u64, &str)> =
        candidates.iter().map(|c| (c["line"].as_u64().unwrap(), c["played"].as_u64().unwrap(), c["anchor"].as_str().unwrap())).collect();
    assert_eq!(summary, vec![(0, 0, "path:0.1.0.0.1"), (1, 2, "path:0.1.0.0.0")]);
    assert_eq!(r["movetext"], "18. Nd5 $1 Qxe1 (18... Qd8 19. Bxf6 $1 Bxf6 20. fxg6 (20. Nxf6+ exf6 21. Bxc4 Rxc4) 20... fxg6 21. Qg3)");
}

/// A move that fits two places is offered at both, with the preview; a start at move 1 is
/// offered renumbered; a capture that fits nowhere changes nothing; comments take the text.
#[test]
fn chess_pgn_edit_capture_reports_ambiguity_renumbering_and_no_fit() {
    let (_base, root) = workdir("pgn-capture-ambiguous");
    let mut auto = Automation::new().with_root(&root).unwrap();
    let a = &mut auto;
    // 19…Kh8, a variation of 19…Kh7: 20.Rfxe1 fits after either.
    let r =
        pgn_edit(a, true, json!([{ "op": "play", "at": "path:0.0.0", "moves": ["Kh8"] }, capture_op("path:0.0.0.1", "subvariation", "(20.Rfxe1)")]));
    let c = capture_of(&r, 1);
    assert_eq!(c["needs_preview"], true);
    assert!(c["reasons"].as_array().unwrap().contains(&json!("ambiguous")), "{c}");
    let lines: Vec<(u64, u64)> =
        c["candidates"].as_array().unwrap().iter().map(|c| (c["line"].as_u64().unwrap(), c["played"].as_u64().unwrap())).collect();
    assert_eq!(lines, vec![(0, 1), (1, 1)]);

    let unnumbered = SCENARIO.replace(" 0 18", " 0 1");
    let r = ok(a, "chess_pgn_edit", json!({ "fen": unnumbered, "ops": [capture_op("path:", "auto", "18.Nd5 Qxe1")] }));
    let c = capture_of(&r, 0);
    assert_eq!(c["reasons"], json!(["renumber"]));
    assert_eq!(c["candidates"][0]["renumbered"], 18);
    assert!(c["candidates"][0]["start_fen"].as_str().unwrap().ends_with(" 0 18"));
    assert_eq!(r["movetext"], "18. Nd5 Qxe1");

    let r = pgn_edit(a, true, json!([capture_op("main:39", "auto", "20…Qh1 is no move here")]));
    let c = capture_of(&r, 0);
    assert_eq!((c["applied"].as_bool(), c["chosen"].is_null()), (Some(false), true));
    assert_eq!(c["reasons"], json!(["no_fit"]));
    assert_eq!(r["movetext"], MAIN);

    // comment_before, in Portuguese (`C` is the knight), on a variation's first move.
    let mut op = capture_op("path:0.0.0.1", "comment_before", "a tentativa 20.Cxf6");
    op["lang"] = json!("pt-BR");
    let r = pgn_edit(a, true, json!([{ "op": "play", "at": "path:0.0.0", "moves": ["Kh8"] }, op]));
    assert!(r["movetext"].as_str().unwrap().contains("({a tentativa 20.Nxf6} 19... Kh8)"), "{}", r["movetext"]);
}

/// The Renumber choice is reported beside the places, never chosen over one that plays, and
/// applied when asked for: the moves take the game's numbers.
#[test]
fn chess_pgn_edit_capture_applies_the_renumber_choice_when_asked() {
    let (_base, root) = workdir("pgn-capture-renumber");
    let mut auto = Automation::new().with_root(&root).unwrap();
    let a = &mut auto;
    // After 20.Raxe1: 19…Kh8 fits by its number as an alternative to 19…Kh7, and renumbered as
    // 20…Kh8.
    let r = pgn_edit(a, true, json!([capture_op("path:0.0.0.0.0", "subvariation", "19…Kh8")]));
    let c = capture_of(&r, 0);
    assert_eq!((c["chosen"].as_u64(), c["renumber"].as_u64(), c["needs_preview"].as_bool()), (Some(0), Some(1), Some(false)));
    assert_eq!(c["candidates"][1]["shifted"], 1);
    assert_eq!(c["candidates"][1]["renumbering"], json!({ "printed": "19...Kh8", "played": "20...Kh8" }));
    assert!(c["candidates"][1]["reasons"].as_array().unwrap().contains(&json!("renumber")), "{c}");
    assert_eq!(r["movetext"], "18. Nd5 Qxe1 19. Nxe7+ Kh7 (19... Kh8) 20. Raxe1");
    let mut op = capture_op("path:0.0.0.0.0", "subvariation", "19…Kh8");
    op["apply"] = json!("renumber");
    let r = pgn_edit(a, true, json!([op]));
    assert_eq!(capture_of(&r, 0)["applied"], true);
    assert_eq!(r["movetext"], "18. Nd5 Qxe1 19. Nxe7+ Kh7 20. Raxe1 Kh8");
    // Without a Renumber choice nothing is applied.
    let mut op = capture_op("path:0.0.0.0", "subvariation", "19…Kh8");
    op["apply"] = json!("renumber");
    let r = pgn_edit(a, true, json!([op]));
    let c = capture_of(&r, 0);
    assert_eq!((c["applied"].as_bool(), c["renumber"].is_null()), (Some(false), true));
    assert_eq!(r["movetext"], MAIN);
}

/// A capture with a bad mode, language, bold range or apply is refused like any other op.
#[test]
fn chess_pgn_edit_capture_refuses_bad_arguments() {
    let (_base, root) = workdir("pgn-capture-refusals");
    let mut auto = Automation::new().with_root(&root).unwrap();
    let a = &mut auto;
    let refused = [
        (json!({ "op": "capture", "at": "path:0" }), "text is missing"),
        (json!({ "op": "capture", "at": "path:0", "text": "18…Qd8", "mode": "sideways" }), "auto, line"),
        (json!({ "op": "capture", "at": "path:0", "text": "18…Qd8", "lang": "xx" }), "language tag"),
        (json!({ "op": "capture", "at": "path:0", "text": "18…Qd8", "lang": 7 }), "language tag"),
        (json!({ "op": "capture", "at": "path:0", "text": "18…Qd8", "bold_runs": [[0]] }), "pairs"),
        (json!({ "op": "capture", "at": "path:0", "text": "18…Qd8", "bold_runs": [[0, -1]] }), "pairs"),
        (json!({ "op": "capture", "at": "path:0", "text": "18…Qd8", "bold_runs": "0-3" }), "an array"),
        (json!({ "op": "capture", "at": "path:0", "text": "18…Qd8", "bold_runs": vec![json!([0, 1]); 4097] }), "at most"),
        (json!({ "op": "capture", "at": "path:0", "text": "18…Qd8", "apply": "always" }), "chosen, auto, never or renumber"),
    ];
    for (op, needle) in refused {
        let r = pgn_edit(a, true, json!([op]));
        assert_eq!(r["movetext"], MAIN, "{op} changed the game");
        let entry = &r["report"][0];
        assert_eq!(entry["ok"], false, "{op}");
        assert!(entry["error"].as_str().unwrap().contains(needle), "{op}: {}", entry["error"]);
    }
    // Too long a text, or ranges past it, are reported, not refused.
    let r = pgn_edit(a, true, json!([{ "op": "capture", "at": "path:0", "text": "x".repeat(65_537) }]));
    assert_eq!(capture_of(&r, 0)["reasons"], json!(["too_long"]));
    let r = pgn_edit(a, true, json!([{ "op": "capture", "at": "path:0", "text": "18…Qd8", "bold_runs": [[3, 1], [0, u64::MAX]] }]));
    assert_eq!(capture_of(&r, 0)["applied"], true);
}

/// xorshift64*: deterministic choices for the fuzz-like test below.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }
    fn pick<'a, T>(&mut self, from: &'a [T]) -> &'a T {
        &from[self.below(from.len())]
    }
}

/// Words a random operation is made of: moves (legal somewhere in the scenario or not, SAN, UCI
/// and figurines), numbers, brackets, separators, glyphs as signs and codes, null moves, results
/// and text that PGN has to escape.
#[rustfmt::skip]
const WORDS: &[&str] = &[
    "18.Nd5!", "Qxe1", "19.Nxe7+", "Kh7", "20.Raxe1", "18…Qd8", "19.Bxf6!", "Bxf6", "20.fxg6", "(", ")", "20.Nxf6+", "exf6", "21.Bxc4", "Rxc4",
    "20...fxg6", "21.Qg3", "22.e5!", "e2e4", "a1a8", "♘d5", "♕xe1", "--", "Z0", "0000", "$14", "$220", "$999", "±", "+-", "!?", "1-0", "1:0",
    "(1-0)", "*", ";", "a)", "b)", "{", "}", "\\", "\"", "posi-", "tion", "Nd7-", "b6", "O-O-O", "0-0", "e8=Q", "1.", "99999999999.", "…",
    "less", "good", "is", "", " ", "\u{0}", "é", "𝄞",
];

/// A random string of [`WORDS`], at most `n` of them.
fn words(rng: &mut Rng, n: usize) -> String {
    let sep = if rng.below(4) == 0 { "\n" } else { " " };
    (0..rng.below(n + 1)).map(|_| *rng.pick(WORDS)).collect::<Vec<_>>().join(sep)
}

/// A random address: mostly one of the scenario's moves, sometimes one that is not there or is
/// not an address.
fn address(rng: &mut Rng) -> String {
    match rng.below(12) {
        0..=3 => format!("main:{}", 34 + rng.below(7)),
        4..=7 => format!("path:{}", (0..rng.below(4)).map(|_| if rng.below(4) == 0 { "1" } else { "0" }).collect::<Vec<_>>().join(".")),
        8 => format!("main:{}", rng.next()),
        9 => format!("path:{}", (0..rng.below(10)).map(|_| rng.below(3).to_string()).collect::<Vec<_>>().join(".")),
        10 => format!("path:{}", rng.next()),
        _ => (*rng.pick(&["main:-1", "path:0..1", "path:x", "", "main:", "path:65536", "cell:3"])).to_owned(),
    }
}

/// A random fragment item for `graft`, valid or not.
fn item(rng: &mut Rng) -> Value {
    match rng.below(7) {
        0 | 1 => {
            let san = *rng.pick(&["Nd5", "Qxe1", "Nxe7+", "Kh7", "Qd8", "Bxf6", "e4", "Kxh1", "--", "Q", ""]);
            json!({ "item": "move", "san": san, "ply": rng.below(80), "bold": rng.below(2) == 0 })
        }
        2 => json!({ "item": "nag", "code": rng.below(300) }),
        3 => json!({ "item": "comment", "text": words(rng, 6), "before": rng.below(2) == 0 }),
        4 => json!({ "item": *rng.pick(&["open", "close", "result"]) }),
        5 => json!({ "item": "move", "san": "Nd5", "ply": rng.next() }),
        _ => json!({ "item": *rng.pick(&["move", "nag", "glyph", "comment"]) }),
    }
}

/// A random operation of chess_pgn_edit, mostly well formed.
fn random_op(rng: &mut Rng) -> Value {
    let at = address(rng);
    match rng.below(16) {
        0 => json!({ "op": "play", "at": at, "moves": (0..rng.below(6)).map(|_| *rng.pick(WORDS)).collect::<Vec<_>>() }),
        1 => json!({ "op": *rng.pick(&["comment", "comment_before"]), "at": at, "text": words(rng, 8) }),
        2 => json!({ "op": "comment", "at": at, "text": "}".repeat(rng.below(5000)) }),
        3 => json!({ "op": "nag", "at": at, "code": rng.below(300) }),
        4 => json!({ "op": "play_null", "at": at }),
        5 => json!({ "op": "move_variation", "at": at, "to": *rng.pick(&["up", "down", "left"]) }),
        6 => json!({ "op": "diagram", "at": at, "on": rng.below(2) == 0 }),
        7 => json!({ "op": *rng.pick(&["promote_one", "demote", "delete_variation", "make_main", "cut"]), "at": at }),
        8 | 9 => {
            let mode = *rng.pick(&["follow", "line", "comment", "sideways"]);
            json!({ "op": "graft", "at": at, "mode": mode, "items": (0..rng.below(12)).map(|_| item(rng)).collect::<Vec<_>>() })
        }
        10..=13 => {
            let text = words(rng, 24);
            let len = text.chars().count();
            let runs: Vec<Value> = (0..rng.below(4)).map(|_| json!([rng.below(len + 3), rng.below(len + 3)])).collect();
            json!({
                "op": "capture", "at": at, "text": text,
                "mode": *rng.pick(&["auto", "line", "variation", "subvariation", "comment", "comment_before"]),
                "lang": *rng.pick(&["en", "pt", "de", "en"]),
                "bold_runs": if rng.below(2) == 0 { Value::Null } else { Value::Array(runs) },
                "apply": *rng.pick(&["chosen", "auto", "never", "renumber"]),
            })
        }
        14 => json!({ "op": *rng.pick(&["unknown", "", "graft"]), "at": at, "items": "x" }),
        _ if rng.below(4) == 0 => json!(rng.below(5)),
        _ => json!({ "op": "play", "at": at, "moves": ["Nd5", "Qxe1", "Nxe7+", "Kh7"] }),
    }
}

/// `pgn` with a few characters deleted, inserted or cut off.
fn mutated(rng: &mut Rng, pgn: &str) -> String {
    let mut chars: Vec<char> = pgn.chars().collect();
    for _ in 0..1 + rng.below(4) {
        let at = rng.below(chars.len() + 1);
        match rng.below(4) {
            0 if at < chars.len() => {
                chars.remove(at);
            }
            1 => chars.insert(at, *rng.pick(&['(', ')', '{', '}', '$', '[', '"', '\\', '!', '\n', '9', ';', '%'])),
            2 => chars.truncate(at),
            _ => {
                let word: Vec<char> = rng.pick(WORDS).chars().collect();
                chars.splice(at..at, word);
            }
        }
    }
    chars.into_iter().collect()
}

/// PGN toolbar T16: random lists of operations (well formed and not, at addresses that are and
/// are not there) on the scenario's game, on games earlier lists built and on mutated PGN. The
/// tool never panics; a reply's game reads back to itself, every operation reported before the
/// last succeeded, the list stops at the first refusal, and the caps hold (at most 2,000
/// operations and 20,000 moves and items a call).
#[test]
fn chess_pgn_edit_survives_random_operation_lists() {
    use pdfcraft_engine::chess::pdfcraft_chess::game::Game;
    let (_base, root) = workdir("pgn-edit-fuzz");
    let mut auto = Automation::new().with_root(&root).unwrap();
    let mut rng = Rng(0x7E16_C0DE);
    let start = format!("[FEN \"{SCENARIO}\"]\n[SetUp \"1\"]\n\n{WITH_VARIATION}");
    let mut kept = start.clone();
    let (mut replied, mut refused, mut applied_ops) = (0usize, 0usize, 0usize);
    for round in 0..4000 {
        // One operation at a time, so the game grows from what the last call made of it; or a list.
        let n = if rng.below(2) == 0 { 1 } else { 1 + rng.below(8) };
        let ops: Vec<Value> = (0..n).map(|_| random_op(&mut rng)).collect();
        let count = ops.len();
        let mut args = match rng.below(8) {
            0 => json!({ "fen": SCENARIO }),
            1..=4 => json!({ "pgn": kept }),
            5 | 6 => json!({ "pgn": mutated(&mut rng, &kept) }),
            _ => json!({ "pgn": mutated(&mut rng, &start) }),
        };
        args["ops"] = Value::Array(ops);
        let Ok(content) = auto.call("chess_pgn_edit", &args) else {
            refused += 1;
            continue;
        };
        replied += 1;
        let r = json_of(content);
        let report = r["report"].as_array().unwrap();
        assert!(!report.is_empty() && report.len() <= count, "round {round}: {} entries for {count} operations", report.len());
        let applied = usize::try_from(r["applied"].as_u64().unwrap()).unwrap();
        applied_ops += applied;
        assert!(applied == report.len() || applied + 1 == report.len(), "round {round}: the list went on after a refusal");
        assert!(report.iter().take(applied).all(|e| e["ok"] == true), "round {round}: {report:?}");
        let pgn = r["pgn"].as_str().unwrap();
        let (again, stopped) = Game::from_pgn(pgn).unwrap_or_else(|e| panic!("round {round}: the reply's PGN does not read: {e}\n{pgn}"));
        assert_eq!(stopped, None, "round {round}: {pgn}");
        assert_eq!(again.pgn(), pgn, "round {round}: the reply's PGN is not a fixed point");
        // The next lists go on from a game with moves, while it stays small.
        if pgn.len() < 16 << 10 && r["movetext"].as_str().is_some_and(|m| !m.is_empty()) {
            kept = pgn.to_owned();
        }
    }
    assert!(replied > 3000 && refused > 0 && applied_ops > 500, "{replied} replies, {refused} refused, {applied_ops} operations applied");
    // The caps on one call: operations, and moves and items over all of them.
    let many = vec![json!({ "op": "cut", "at": "path:" }); 2001];
    assert!(auto.call("chess_pgn_edit", &json!({ "fen": SCENARIO, "ops": many })).is_err());
    let moves = vec![json!("Nd5"); 4000];
    let heavy = vec![json!({ "op": "play", "at": "path:", "moves": moves }); 6];
    assert!(auto.call("chess_pgn_edit", &json!({ "fen": SCENARIO, "ops": heavy })).is_err());
}

// ─── chess_read_area (PGN toolbar T10) ────────────────────────────────────────────────────────

/// A 600 × 800 pt page with `text` in 14 pt Helvetica, its baseline 300 pt from the top.
fn text_pdf(text: &str) -> Vec<u8> {
    let content = format!("BT /F1 14 Tf 60 500 Td ({text}) Tj ET\n");
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 600 800] >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_string(),
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

fn refused(a: &mut Automation, args: Value) -> String {
    match a.call("chess_read_area", &args) {
        Err(ToolError::Failed(m) | ToolError::InvalidArgs(m)) => m,
        other => panic!("chess_read_area accepted {args}: {other:?}"),
    }
}

/// Without the glyph model the area is not read and the reply says what to install; a degenerate
/// area, a page the document lacks, a bad language or game, and a models folder outside the root
/// are refused, each saying why.
#[test]
fn chess_read_area_says_what_it_needs_and_stays_in_the_root() {
    let (base, root) = workdir("area");
    std::fs::write(root.join("moves.pdf"), text_pdf("1.e4 e5 2.Nf3 Nc6")).unwrap();
    let mut a = Automation::new().with_root(&root).unwrap();
    let doc = ok(&mut a, "doc_open", json!({ "path": "moves.pdf" }))["doc"].as_u64().unwrap();
    let area = json!([50, 480, 400, 520]);
    let m = refused(&mut a, json!({ "doc": doc, "page": 1, "rect": area, "models_dir": "models" }));
    assert!(m.contains("chess-glyphs") && m.contains("PDFCRAFT_MODELS"), "{m}");
    let m = refused(&mut a, json!({ "doc": doc, "page": 1, "rect": [50, 480, 51, 481], "models_dir": "models" }));
    assert!(m.contains("not a rectangle"), "{m}");
    let m = refused(&mut a, json!({ "doc": doc, "page": 1, "rect": [-900, -900, -800, -800], "models_dir": "models" }));
    assert!(m.contains("not a rectangle"), "off the page: {m}");
    let m = refused(&mut a, json!({ "doc": doc, "page": 2, "rect": area, "models_dir": "models" }));
    assert!(m.contains("out of range"), "{m}");
    let m = refused(&mut a, json!({ "doc": doc, "page": 1, "rect": [1, 2, 3], "models_dir": "models" }));
    assert!(m.contains("rect"), "{m}");
    let m = refused(&mut a, json!({ "doc": doc, "page": 1, "rect": area, "lang": "xx", "models_dir": "models" }));
    assert!(m.contains("lang"), "{m}");
    let m = refused(&mut a, json!({ "doc": doc, "page": 1, "rect": area, "fen": "nonsense", "models_dir": "models" }));
    assert!(m.starts_with("fen"), "the game is checked before reading: {m}");
    let m = refused(&mut a, json!({ "doc": doc, "page": 1, "rect": area, "at": "main:0", "models_dir": "models" }));
    assert!(m.contains("fen or pgn"), "{m}");
    let m = refused(&mut a, json!({ "doc": doc, "page": 1, "rect": area, "pgn": "1. e4", "at": "main:9", "models_dir": "models" }));
    assert!(m.contains("no move there"), "{m}");
    for dir in ["../outside".to_string(), base.join("outside").to_string_lossy().into_owned()] {
        let m = refused(&mut a, json!({ "doc": doc, "page": 1, "rect": area, "models_dir": dir }));
        assert!(m.contains("is outside the allowed directory"), "{dir}: {m}");
    }
    let _ = std::fs::remove_dir_all(base);
}

/// With `PDFCRAFT_CHESS_TEST_MODELS` naming the folder of the user's models, the glyph model reads
/// the moves of a printed line and the capture is planned from the start position. Printed, and
/// asserted only loosely: this measures the model, not the plumbing.
#[test]
fn chess_read_area_reads_and_plans_with_the_users_model() {
    let Some(models) = std::env::var_os("PDFCRAFT_CHESS_TEST_MODELS") else {
        eprintln!("skipped: set PDFCRAFT_CHESS_TEST_MODELS to the folder of the chess models");
        return;
    };
    let (base, root) = workdir("area-model");
    std::fs::write(root.join("moves.pdf"), text_pdf("1.e4 e5 2.Nf3 Nc6")).unwrap();
    let mut a = Automation::new();
    let path = root.join("moves.pdf").to_string_lossy().into_owned();
    let doc = ok(&mut a, "doc_open", json!({ "path": path }))["doc"].as_u64().unwrap();
    let r = ok(
        &mut a,
        "chess_read_area",
        json!({
            "doc": doc, "page": 1, "rect": [50, 480, 400, 520], "general_ocr": false,
            "models_dir": std::path::PathBuf::from(models).to_string_lossy(),
            "fen": "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1", "at": "path:",
        }),
    );
    eprintln!("read: {} | moves: {} | capture: {}", r["text"], r["moves"], r["capture"]["candidates"]);
    assert_eq!(r["lines"], 1);
    assert!(!r["alternatives"].as_array().unwrap().is_empty());
    assert!(r["capture"]["reasons"].is_array());
    let _ = std::fs::remove_dir_all(base);
}

// ─── chess_games (PGN toolbar wave 10, U2) ────────────────────────────────────────────────────

const ABC: &str = "[Event \"A\"]\n\n1. e4 *\n\n[Event \"B\"]\n\n1. d4 *\n\n[Event \"C\"]\n\n1. c4 *\n";

fn events_of(r: &Value) -> Vec<String> {
    r["games"].as_array().unwrap().iter().map(|g| g["event"].as_str().unwrap().to_owned()).collect()
}

/// Every operation of the list, as the app's list does it: the order, the game shown after each
/// change, the names written as tags, a game from a position, and the export in list order.
#[test]
fn chess_games_renames_duplicates_deletes_moves_and_starts_games_from_a_position() {
    let mut a = Automation::new();
    let r = ok(
        &mut a,
        "chess_games",
        json!({ "pgn": ABC, "active": 1, "ops": [
            { "op": "list" },
            { "op": "rename", "game": 2, "white": "Werle", "black": "Jianu", "site": "Wijk aan Zee NED", "date": "2025.01.??", "round": "6" },
            { "op": "duplicate", "game": 0 },
            { "op": "move", "game": 3, "to": 0 },
            { "op": "delete", "game": 2 },
            { "op": "new_from", "game": 0, "at": "main:1" },
            { "op": "set_active", "game": 3 },
            { "op": "export", "game": 0 },
        ] }),
    );
    let report = r["report"].as_array().unwrap();
    assert_eq!(r["applied"], 8, "{r:#}");
    assert_eq!(report[0]["games"].as_array().unwrap().len(), 3);
    assert_eq!((report[0]["active"].as_u64(), report[0]["games"][1]["shown"].as_bool()), (Some(1), Some(true)));
    assert_eq!(report[1]["name"], "Werle–Jianu");
    assert_eq!((report[2]["game"].as_u64(), report[2]["active"].as_u64()), (Some(1), Some(1)), "the copy right after it, shown");
    // A, copy of A (shown), B, C; then C moved first: C, A, copy (shown), B.
    assert_eq!(report[3]["active"], 2);
    // The copy (shown) deleted: the one after it, B, is shown.
    assert_eq!((report[4]["deleted"].is_null(), report[4]["active"].as_u64()), (false, Some(2)), "{:#}", report[4]);
    assert_eq!((report[5]["game"].as_u64(), report[5]["active"].as_u64()), (Some(1), Some(1)));
    assert_eq!(report[6]["active"], 3);
    assert!(report[7]["pgn"].as_str().unwrap().contains("[White \"Werle\"]"), "{:#}", report[7]);
    assert_eq!(events_of(&r), ["C", "", "A", "B"]);
    assert_eq!(r["count"], 4);
    let games = r["games"].as_array().unwrap();
    assert_eq!(games[0]["name"], "Werle–Jianu");
    assert_eq!((&games[0]["site"], &games[0]["date"], &games[0]["round"]), (&json!("Wijk aan Zee NED"), &json!("2025.01.??"), &json!("6")));
    assert_eq!((&games[2]["site"], &games[2]["date"]), (&json!(""), &json!("")), "an unknown site and date are empty");
    assert_eq!(games[1]["start_fen"], "rnbqkbnr/pppppppp/8/8/2P5/8/PP1PPPPP/RNBQKBNR b KQkq c3 0 1", "after 1. c4 of game 0 (C)");
    assert_eq!((games[0]["moves"].as_u64(), games[3]["shown"].as_bool()), (Some(1), Some(true)));
    let pgn = r["pgn"].as_str().unwrap();
    assert!(pgn.contains("[SetUp \"1\"]") && pgn.contains("1. c4 *\n\n["), "a blank line between two games: {pgn}");
    // Read back: the same list.
    let again = ok(&mut a, "chess_games", json!({ "pgn": pgn }));
    assert_eq!(events_of(&again), events_of(&r));
    assert_eq!(again["pgn"], r["pgn"]);
}

/// new_at starts a game at a position set up, right after the game, which keeps its moves (the
/// board's Set up a position over a game with moves, plan A8 §7, V2a).
#[test]
fn chess_games_starts_a_game_at_a_position_set_up() {
    let mut a = Automation::new();
    let fen = "4k3/8/8/8/8/8/8/R3K3 w - - 0 1";
    let r = ok(
        &mut a,
        "chess_games",
        json!({ "pgn": "1. d4 *", "ops": [{ "op": "new_at", "fen": fen }, { "op": "new_at", "fen": "8/8/8/8/8/8/8/8 w - - 0 1" }] }),
    );
    let report = r["report"].as_array().unwrap();
    assert_eq!((report[0]["game"].as_u64(), report[0]["active"].as_u64()), (Some(1), Some(1)));
    assert_eq!(report[1]["ok"], false, "a position without kings starts no game");
    assert_eq!(r["applied"], 1);
    let games = r["games"].as_array().unwrap();
    assert_eq!((games[0]["moves"].as_u64(), games[1]["start_fen"].as_str()), (Some(1), Some(fen)));
}

/// A refused operation adds an error entry, changes nothing and stops the list; `out` is then not
/// written. Bad collections and arguments are refused with what is allowed.
#[test]
fn chess_games_refuses_what_the_list_does_not_allow() {
    let (base, root) = workdir("games-refused");
    let mut a = Automation::new().with_root(&root).unwrap();
    let r = ok(
        &mut a,
        "chess_games",
        json!({ "pgn": ABC, "out": "games.pgn", "ops": [
            { "op": "move", "game": 0, "to": 2 },
            { "op": "delete", "game": 3 },
            { "op": "rename", "white": "never" },
        ] }),
    );
    assert_eq!(r["applied"], 1);
    assert_eq!(r["report"][1]["ok"], false);
    assert!(r["report"][1]["error"].as_str().unwrap().contains("0 to 2"), "{r:#}");
    assert_eq!(r["report"].as_array().unwrap().len(), 2, "the list stopped there");
    assert_eq!(events_of(&r), ["B", "C", "A"], "the move before it stays");
    assert!(r["written"].is_null() && !root.join("games.pgn").exists(), "nothing written after a refusal");
    for (op, says) in [
        (json!({ "op": "move", "game": 0, "to": 3 }), "to is"),
        (json!({ "op": "new_from", "at": "main:9" }), "no move there"),
        (json!({ "op": "new_from", "at": "nowhere" }), "an address"),
        (json!({ "op": "rename", "white": 3 }), "white is a string"),
        (json!({ "op": "rename", "white": "W", "date": "2025.04.31" }), "not a PGN date"),
        (json!({ "op": "rename", "date": "18/01/2025" }), "YYYY.MM.DD"),
        (json!({ "op": "sort" }), "op \"sort\""),
        (json!({ "op": "set_active", "game": 99 }), "the list has games 0 to 2"),
    ] {
        let r = ok(&mut a, "chess_games", json!({ "pgn": ABC, "ops": [op] }));
        assert_eq!(r["report"][0]["ok"], false, "{r:#}");
        assert!(r["report"][0]["error"].as_str().unwrap().contains(says), "{says}: {r:#}");
        assert_eq!(events_of(&r), ["A", "B", "C"], "unchanged");
    }
    let refused = |a: &mut Automation, args: Value| match a.call("chess_games", &args) {
        Err(ToolError::Failed(m) | ToolError::InvalidArgs(m)) => m,
        other => panic!("chess_games accepted {args}: {other:?}"),
    };
    assert!(refused(&mut a, json!({ "pgn": ABC, "path": "x.pgn" })).contains("not both"));
    assert!(refused(&mut a, json!({ "pgn": ABC, "active": 3 })).contains("0 to 2"));
    assert!(refused(&mut a, json!({ "pgn": "[Event \"x\"]\n1. e4 *\n".repeat(257) })).contains("at most 256 games"));
    assert!(refused(&mut a, json!({ "path": "missing.pgn" })).contains("missing.pgn"));
    // Nothing given: one blank game, and deleting it leaves a blank one.
    let blank = ok(&mut a, "chess_games", json!({ "ops": [{ "op": "delete" }] }));
    assert_eq!((blank["count"].as_u64(), blank["games"][0]["moves"].as_u64()), (Some(1), Some(0)));
    let _ = std::fs::remove_dir_all(base);
}

/// The collection read from a file and written to another, both inside the root; either path
/// outside it is refused before anything is read or written. The control call, the same with
/// both paths inside, succeeds, so the refusals can only come from resolving the paths.
#[test]
fn chess_games_reads_and_writes_pgn_files_inside_the_root_only() {
    let (base, root) = workdir("games-files");
    std::fs::write(root.join("abc.pgn"), ABC).unwrap();
    std::fs::write(base.join("outside").join("abc.pgn"), ABC).unwrap();
    let mut a = Automation::new().with_root(&root).unwrap();
    let ops = json!([{ "op": "move", "game": 2, "to": 0 }]);
    let outside = base.join("outside");
    for (key, path) in [
        ("path", "../outside/abc.pgn".to_owned()),
        ("path", outside.join("abc.pgn").to_string_lossy().into_owned()),
        ("out", "../outside/written.pgn".to_owned()),
        ("out", outside.join("written.pgn").to_string_lossy().into_owned()),
    ] {
        let mut args = json!({ "path": "abc.pgn", "out": "cab.pgn", "ops": ops });
        args[key] = json!(path);
        match a.call("chess_games", &args) {
            Err(ToolError::Failed(m)) => assert!(m.contains("is outside the allowed directory"), "{key} {path}: {m}"),
            other => panic!("chess_games accepted {key} {path}: {other:?}"),
        }
    }
    assert!(!outside.join("written.pgn").exists(), "nothing written outside the root");
    assert!(!root.join("cab.pgn").exists(), "nor inside, for a refused call");
    // The control: both inside.
    let r = ok(&mut a, "chess_games", json!({ "path": "abc.pgn", "out": "cab.pgn", "ops": ops }));
    assert_eq!(events_of(&r), ["C", "A", "B"]);
    let written = std::fs::read_to_string(root.join("cab.pgn")).unwrap();
    assert_eq!(written, r["pgn"].as_str().unwrap());
    assert!(r["written"].as_str().unwrap().ends_with("cab.pgn"));
    // Edited in place: the file read is the file written.
    let r = ok(&mut a, "chess_games", json!({ "path": "cab.pgn", "out": "cab.pgn", "ops": [{ "op": "delete", "game": 0 }] }));
    assert_eq!(events_of(&r), ["A", "B"]);
    assert_eq!(std::fs::read_to_string(root.join("cab.pgn")).unwrap(), r["pgn"].as_str().unwrap());
    let _ = std::fs::remove_dir_all(base);
}

// ─── chess_glyph_sample (PGN toolbar wave 10, U3) ─────────────────────────────────────────────

/// The width and height a PNG's header gives.
fn png_size(bytes: &[u8]) -> (u32, u32) {
    assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"), "a PNG");
    let be = |at: usize| u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    (be(16), be(20))
}

/// The box of the first characters ("1.e4") of the line `text_pdf` sets, from the top left.
const FIRST_CHARS: [f64; 4] = [58.0, 288.0, 70.0, 302.0];

/// A corrected character becomes a 32 × 32 sample in ChessVisionOFF's review-queue layout inside
/// the root: `<folder>/<class>/<confidence>_<hash>_p<page>_<n>.png`, never overwritten.
#[test]
fn chess_glyph_sample_writes_the_review_queue_layout_inside_the_root() {
    let (base, root) = workdir("sample");
    std::fs::write(root.join("moves.pdf"), text_pdf("1.e4 e5 2.Nf3 Nc6")).unwrap();
    let mut a = Automation::new().with_root(&root).unwrap();
    let doc = ok(&mut a, "doc_open", json!({ "path": "moves.pdf" }))["doc"].as_u64().unwrap();
    let args = json!({ "doc": doc, "page": 1, "rect": FIRST_CHARS, "text": "1", "confidence": 0.42, "folder": "samples" });
    let first = ok(&mut a, "chess_glyph_sample", args.clone());
    assert_eq!((first["class"].as_str(), first["side"].as_u64()), (Some("digit_1"), Some(32)), "{first:#}");
    let name = first["name"].as_str().unwrap();
    assert!(name.starts_with("0420_") && name.ends_with("_p0001_1.png") && name.len() == "0420_".len() + 16 + "_p0001_1.png".len(), "{name}");
    let written = root.join("samples").join("digit_1").join(name);
    assert_eq!(png_size(&std::fs::read(&written).unwrap()), (32, 32));
    let second = ok(&mut a, "chess_glyph_sample", args);
    assert!(second["name"].as_str().unwrap().ends_with("_p0001_2.png"), "{second:#}");
    assert!(written.exists(), "the first is not overwritten");
    // A figurine goes to its code point's folder.
    let knight = ok(&mut a, "chess_glyph_sample", json!({ "doc": doc, "page": 1, "rect": FIRST_CHARS, "text": "♘", "folder": "samples" }));
    assert_eq!(knight["class"], "sym_9816");
    assert!(knight["name"].as_str().unwrap().starts_with("0000_"), "no confidence given: 0");
    // Refused: what the sample cannot be made of.
    for (bad, says) in [
        (json!({ "text": "" }), "text"),
        (json!({ "text": "x".repeat(17) }), "text"),
        (json!({ "text": "a\nb" }), "text"),
        (json!({ "confidence": 1.5 }), "confidence"),
        (json!({ "rect": [70, 288, 58, 302] }), "not a rectangle"),
        (json!({ "rect": [0, 0, 100, 10] }), "not a rectangle"),
        (json!({ "page": 2 }), "out of range"),
    ] {
        let mut args = json!({ "doc": doc, "page": 1, "rect": FIRST_CHARS, "text": "1", "folder": "samples" });
        for (k, v) in bad.as_object().unwrap() {
            args[k] = v.clone();
        }
        match a.call("chess_glyph_sample", &args) {
            Err(ToolError::Failed(m) | ToolError::InvalidArgs(m)) => assert!(m.contains(says), "{bad}: {m}"),
            other => panic!("chess_glyph_sample accepted {bad}: {other:?}"),
        }
    }
    let _ = std::fs::remove_dir_all(base);
}

/// `folder` is confined to `--root`. The document, page, box and text are valid, and the same call
/// with the folder inside the root writes the sample, so the refusal can only come from resolving
/// the path (with confinement switched off, the outside calls write there and this test fails).
#[test]
fn a_glyph_sample_folder_outside_the_root_is_refused() {
    let (base, root) = workdir("sample-confined");
    std::fs::write(root.join("moves.pdf"), text_pdf("1.e4 e5 2.Nf3 Nc6")).unwrap();
    let mut a = Automation::new().with_root(&root).unwrap();
    let doc = ok(&mut a, "doc_open", json!({ "path": "moves.pdf" }))["doc"].as_u64().unwrap();
    let outside = base.join("outside");
    for dir in ["../outside".to_string(), outside.to_string_lossy().into_owned(), "samples/../../outside".to_string()] {
        let args = json!({ "doc": doc, "page": 1, "rect": FIRST_CHARS, "text": "1", "folder": dir });
        match a.call("chess_glyph_sample", &args) {
            Err(ToolError::Failed(m)) => assert!(m.contains("is outside the allowed directory"), "{dir}: {m}"),
            other => panic!("chess_glyph_sample accepted folder {dir}: {other:?}"),
        }
    }
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0, "nothing written outside the root");
    // The control: the same call, with the folder inside the root.
    let r = ok(&mut a, "chess_glyph_sample", json!({ "doc": doc, "page": 1, "rect": FIRST_CHARS, "text": "1", "folder": "samples" }));
    assert!(root.join("samples").join("digit_1").join(r["name"].as_str().unwrap()).exists());
    let _ = std::fs::remove_dir_all(base);
}

/// An encrypted document gives a sample only when allowed (it is written unencrypted), as the app
/// asks first; a document that does not allow copying never gives one.
#[test]
fn an_encrypted_document_gives_a_sample_only_when_allowed() {
    let (base, root) = workdir("sample-encrypted");
    std::fs::write(root.join("moves.pdf"), text_pdf("1.e4 e5 2.Nf3 Nc6")).unwrap();
    let mut a = Automation::new().with_root(&root).unwrap();
    let doc = ok(&mut a, "doc_open", json!({ "path": "moves.pdf" }))["doc"].as_u64().unwrap();
    ok(&mut a, "doc_protect", json!({ "doc": doc, "open_password": "open", "permissions_password": "boss" }));
    ok(&mut a, "doc_save", json!({ "doc": doc, "path": "locked.pdf" }));
    let mut b = Automation::new().with_root(&root).unwrap();
    let owner = ok(&mut b, "doc_open", json!({ "path": "locked.pdf", "password": "boss" }))["doc"].as_u64().unwrap();
    let args = json!({ "doc": owner, "page": 1, "rect": FIRST_CHARS, "text": "1", "folder": "samples" });
    match b.call("chess_glyph_sample", &args) {
        Err(ToolError::Failed(m)) => assert!(m.contains("allow_encrypted"), "{m}"),
        other => panic!("an encrypted document gave a sample unasked: {other:?}"),
    }
    assert!(!root.join("samples").exists(), "nothing written");
    let mut allowed = args.clone();
    allowed["allow_encrypted"] = json!(true);
    ok(&mut b, "chess_glyph_sample", allowed);
    // Opened with the open password only: copying is not allowed.
    let mut c = Automation::new().with_root(&root).unwrap();
    let user = ok(&mut c, "doc_open", json!({ "path": "locked.pdf", "password": "open" }))["doc"].as_u64().unwrap();
    let mut no_copy = args;
    no_copy["doc"] = json!(user);
    no_copy["allow_encrypted"] = json!(true);
    match c.call("chess_glyph_sample", &no_copy) {
        Err(ToolError::Failed(m)) => assert!(m.contains("copying"), "{m}"),
        other => panic!("a document that forbids copying gave a sample: {other:?}"),
    }
    let _ = std::fs::remove_dir_all(base);
}

/// Plan A8, P3a1: a correction is kept on disk by the document's keys and the diagram's page
/// fingerprint and box, found again after reopening, revised in place, deleted with a tombstone,
/// and only inside the root.
#[test]
fn diagram_corrections_are_kept_found_again_and_confined() {
    const CORRECTED: &str = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1";
    let (base, root) = workdir("corrections");
    let (mut a, doc) = opened(&root);
    let empty = ok(&mut a, "chess_correction_list", json!({ "doc": doc }));
    assert_eq!((empty["count"].as_u64(), empty["read_only"].as_bool()), (Some(0), Some(false)), "{empty:#}");
    let set = ok(&mut a, "chess_correction_set", json!({ "doc": doc, "page": 1, "index": 1, "fen": CORRECTED, "models_dir": "models" }));
    assert_eq!((set["kind"].as_str(), set["legal"].as_bool(), set["rev"].as_u64()), (Some("corrected"), Some(true), Some(1)), "{set:#}");
    let uid = set["uid"].as_str().unwrap().to_owned();
    let file = root.join(".pdfcraft/chess-corrections").join(set["file"].as_str().unwrap());
    assert!(file.is_file(), "written inside the root: {file:?}");
    // The same diagram again: the next revision of the same correction.
    let verified = ok(&mut a, "chess_correction_set", json!({ "doc": doc, "page": 1, "index": 1, "kind": "verified", "models_dir": "models" }));
    assert_eq!((verified["uid"].as_str(), verified["rev"].as_u64()), (Some(uid.as_str()), Some(2)), "{verified:#}");
    assert_eq!(verified["fen"], "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1", "verified keeps the reading");
    // Another session opening the same file finds it, on the same page, unchanged.
    let (mut b, doc_b) = opened(&root);
    let listed = ok(&mut b, "chess_correction_list", json!({ "doc": doc_b }));
    let c = &listed["corrections"][0];
    assert_eq!(
        (listed["count"].as_u64(), c["uid"].as_str(), c["page"].as_u64(), c["located"].as_str()),
        (Some(1), Some(uid.as_str()), Some(1), Some("same")),
        "{listed:#}"
    );
    assert_eq!(c["original_fen"], "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1");
    // A blank page inserted first: the correction moved with its page.
    ok(&mut b, "page_insert_blank", json!({ "doc": doc_b, "at": 1 }));
    let moved = ok(&mut b, "chess_correction_list", json!({ "doc": doc_b }));
    assert_eq!((moved["corrections"][0]["page"].as_u64(), moved["corrections"][0]["located"].as_str()), (Some(2), Some("moved")), "{moved:#}");
    // Deleted: a tombstone, so it lists no more; a second delete is refused with where to look.
    let deleted = ok(&mut a, "chess_correction_delete", json!({ "doc": doc, "uid": uid }));
    assert_eq!(deleted["deleted"], true);
    assert_eq!(ok(&mut a, "chess_correction_list", json!({ "doc": doc }))["count"], 0);
    match a.call("chess_correction_delete", &json!({ "doc": doc, "uid": uid })) {
        Err(ToolError::Failed(m)) => assert!(m.contains("no correction") && m.contains("chess_correction_list"), "{m}"),
        other => panic!("deleted twice: {other:?}"),
    }
    assert!(std::fs::read_to_string(&file).unwrap().contains("\"deleted\":true"), "kept as a tombstone");
    // Arguments and the root.
    match a.call("chess_correction_set", &json!({ "doc": doc, "page": 1, "index": 9, "fen": CORRECTED, "models_dir": "models" })) {
        Err(ToolError::InvalidArgs(m)) => assert!(m.contains("page 1 has 3 diagrams"), "{m}"),
        other => panic!("index 9 accepted: {other:?}"),
    }
    match a.call("chess_correction_set", &json!({ "doc": doc, "page": 1, "index": 2, "models_dir": "models" })) {
        Err(ToolError::InvalidArgs(m)) => assert!(m.contains("give fen"), "{m}"),
        other => panic!("corrected without a fen: {other:?}"),
    }
    let outside = base.join("outside");
    for store in ["../outside".to_string(), outside.to_string_lossy().into_owned()] {
        match a.call("chess_correction_list", &json!({ "doc": doc, "store": store })) {
            Err(ToolError::Failed(m)) => assert!(m.contains("is outside the allowed directory"), "{store}: {m}"),
            other => panic!("store {store} outside the root accepted: {other:?}"),
        }
    }
    assert!(std::fs::read_dir(&outside).unwrap().next().is_none(), "nothing written outside");
    let _ = std::fs::remove_dir_all(base);
}

/// Plan A8, P3a3: a correction kept for the document wins over the reading in the recognised
/// document, the exports and the lichess links, and is never lost to an edit, an undo, a save,
/// closing and reopening.
#[test]
fn corrections_win_in_readings_exports_and_links_and_are_never_lost() {
    const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
    const E4: &str = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1";
    const KINGS: &str = "4k3/8/8/8/8/8/8/R3K3 w Q - 0 1";
    let (base, root) = workdir("corrected");
    let (mut a, doc) = opened(&root);
    // The font diagram (read) and the vector one (unread without the piece model) corrected.
    ok(&mut a, "chess_correction_set", json!({ "doc": doc, "page": 1, "index": 1, "fen": E4, "models_dir": "models" }));
    ok(&mut a, "chess_correction_set", json!({ "doc": doc, "page": 1, "index": 2, "fen": KINGS, "models_dir": "models" }));
    let fens = |a: &mut Automation, doc: u64, extra: Value| -> (Vec<Value>, Value) {
        let mut args = json!({ "doc": doc, "models_dir": "models", "include": ["json"], "lang": "en" });
        args.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        let read = ok(a, "chess_document", args);
        let blocks = read["document"]["pages"].as_array().unwrap().iter().flat_map(|p| p["blocks"].as_array().unwrap().clone()).collect::<Vec<_>>();
        (blocks.iter().filter(|b| b["type"] == "diagram").map(|b| b["reading"]["fen"].clone()).collect(), read["corrections"].clone())
    };
    let (read, used) = fens(&mut a, doc, json!({}));
    assert_eq!(read, [json!(E4), json!(KINGS), Value::Null], "{used:#}");
    assert_eq!(used["kept"], 2);
    let applied = used["applied"].as_array().unwrap();
    assert_eq!((applied[0]["read_fen"].as_str(), applied[0]["diverged"].as_bool()), (Some(START), Some(false)), "{used:#}");
    assert!(applied[1]["read_fen"].is_null(), "the vector diagram was not read: {used:#}");
    // Asked not to: what was read.
    let (plain, none) = fens(&mut a, doc, json!({ "corrections": false }));
    assert_eq!((plain, none), (vec![json!(START), Value::Null, Value::Null], Value::Null));
    // The exports and the links take the corrected positions.
    ok(&mut a, "chess_export_pgn", json!({ "doc": doc, "out": "book.pgn", "models_dir": "models", "lang": "en" }));
    let pgn = std::fs::read_to_string(root.join("book.pgn")).unwrap();
    assert!(pgn.contains(E4) && pgn.contains(KINGS) && !pgn.contains(START), "{pgn}");
    let html = ok(&mut a, "chess_export_html", json!({ "doc": doc, "out": "book.html", "models_dir": "models", "lang": "en" }));
    assert_eq!((html["diagrams_unread"].as_u64(), html["corrections"]["applied"].as_array().map(Vec::len)), (Some(1), Some(2)), "{html:#}");
    assert!(std::fs::read_to_string(root.join("book.html")).unwrap().contains(&format!("data-fen=\"{KINGS}\"")));
    let links = ok(&mut a, "chess_add_lichess_links", json!({ "doc": doc, "models_dir": "models", "dry_run": true }));
    let linked: Vec<(u64, &str)> =
        links["links"].as_array().unwrap().iter().map(|l| (l["index"].as_u64().unwrap(), l["fen"].as_str().unwrap())).collect();
    assert_eq!(linked, [(1, E4), (2, KINGS)], "{links:#}");
    // An edit that moves the page, its undo, then a save, closing and reopening.
    ok(&mut a, "page_insert_blank", json!({ "doc": doc, "at": 1 }));
    let (moved, used) = fens(&mut a, doc, json!({}));
    assert_eq!((moved, used["applied"][0]["page"].as_u64()), (vec![json!(E4), json!(KINGS), Value::Null], Some(2)), "{used:#}");
    ok(&mut a, "edit_undo", json!({ "doc": doc }));
    let (undone, used) = fens(&mut a, doc, json!({}));
    assert_eq!((undone, used["applied"][0]["page"].as_u64()), (vec![json!(E4), json!(KINGS), Value::Null], Some(1)), "{used:#}");
    ok(&mut a, "page_insert_blank", json!({ "doc": doc, "at": 1 }));
    let saved = ok(&mut a, "doc_save", json!({ "doc": doc, "path": "saved.pdf" }));
    assert_eq!(saved["chess_corrections"], "kept for the saved file", "{saved:#}");
    ok(&mut a, "doc_close", json!({ "doc": doc }));
    let (mut b, again) = {
        let mut b = Automation::new().with_root(&root).unwrap();
        let d = ok(&mut b, "doc_open", json!({ "path": "saved.pdf" }))["doc"].as_u64().unwrap();
        (b, d)
    };
    let (reopened, used) = fens(&mut b, again, json!({}));
    assert_eq!((reopened, used["applied"][1]["page"].as_u64()), (vec![json!(E4), json!(KINGS), Value::Null], Some(2)), "{used:#}");
    let _ = std::fs::remove_dir_all(base);
}
