//! The golden journey (plan A9-Q1b): a book's diagrams detected, diagram 2 corrected and
//! confirmed, the book saved under another name and reopened, then exported as EPUB, which must
//! carry the corrected position, with a corrections store in the folder that an earlier build
//! (`cc2d484c`, format 1) wrote. It runs in the chess gate's stage 3 and catches what no step's
//! own test does: the stores, the identity of a saved file, the jobs and the exports together.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use pdfcraft_engine::chess::export::ChessFormat;
use pdfcraft_ui_egui::PdfCraftApp;

const START_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
/// The position diagram 2 (the drawn board, which no model reads here) is corrected to.
const EDITED: &str = "4k3/8/8/8/8/8/8/R3K3 w Q - 0 1";
/// The start position in the Chess Merida layout, top rank first.
const MERIDA_START: [&str; 8] = ["tMvWlVmT", "OoOoOoOo", " + + + +", "+ + + + ", " + + + +", "+ + + + ", "pPpPpPpP", "RnBqKbNr"];

/// A 600 × 800 pt page with the start position in a diagram font, top left, and a board of
/// filled squares under it.
fn book() -> Vec<u8> {
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
    content.push_str("f\n");
    let widths = vec!["1000"; 95].join(" ");
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 600 800] >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
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

/// The corrections file an earlier build (`cc2d484c`, format 1) wrote in `store` for `bytes`
/// opened as `name`: diagram 1 verified as the start position.
fn earlier_store(store: &std::path::Path, name: &str, bytes: &[u8]) {
    use pdfcraft_engine::chess::corrections::{DocRecord, file_name, normalize};
    let mut s = pdfcraft_engine::Session::new();
    let id = s.open(name, None, std::sync::Arc::new(bytes.to_vec()), None).unwrap();
    let page = s.chess_scan(id, &[0], &pdfcraft_engine::chess::ChessScanSettings::default(), false).unwrap().remove(0);
    let (b, crop) = (page.diagrams[0].candidate.bbox_user, page.geometry.unwrap().crop_box());
    let rect = normalize([b.x0, b.y0, b.x1, b.y1], [crop.x0, crop.y0, crop.x1, crop.y1]).unwrap();
    let doc = s.get(id).unwrap();
    let (keys, fps) = (doc.doc_keys(), doc.page_fingerprints());
    let file = serde_json::json!({
        "format": "pdfcraft-chess-corrections",
        "version": 1,
        "doc": DocRecord::new(&keys, 1),
        "entries": [{
            "uid": "golden-v1", "page": 0, "page_fp": fps[0], "rect": rect,
            "orig": { "fen": START_FEN, "model": null }, "fen": START_FEN, "kind": "verified",
            "at": 1, "rev": 1, "deleted": false,
        }],
    });
    std::fs::create_dir_all(store).unwrap();
    std::fs::write(store.join(file_name(&keys).unwrap()), serde_json::to_vec(&file).unwrap()).unwrap();
}

/// The app with `bytes` open as `name` (from `path` when given), its corrections in `store`, its
/// jobs run inline, and no piece model whatever the machine has.
fn harness(name: &str, path: Option<String>, bytes: Vec<u8>, store: std::path::PathBuf) -> Harness<'static, PdfCraftApp> {
    let (none, _) = pdfcraft_engine::chess::pdfcraft_chessvision::models::ChessModels::from_files(Vec::new());
    pdfcraft_engine::chess::install_models(Some(none));
    let name = name.to_owned();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 1000.0)).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.chess.corrections.use_folder(store);
        app.open_bytes(&name, path, bytes).unwrap();
        app.run_inline = true;
        app
    });
    h.run_steps(4);
    h
}

/// What the user did with diagram `index` of page 1.
fn kind(h: &Harness<'static, PdfCraftApp>, index: usize) -> Option<String> {
    h.state().chess.results().and_then(|r| r.kind(0, index)).map(|k| format!("{k:?}"))
}

/// One entry of a zip archive (stored or deflated); a test's own reader.
fn zip_entry(zip: &[u8], name: &str) -> Option<Vec<u8>> {
    let at = |i: usize, n: usize| zip.get(i..i + n).map(|b| b.iter().rev().fold(0usize, |v, b| v << 8 | usize::from(*b)));
    let mut i = 0;
    while zip.get(i..)?.starts_with(b"PK\x03\x04") {
        let (method, size, name_len, extra) = (at(i + 8, 2)?, at(i + 18, 4)?, at(i + 26, 2)?, at(i + 28, 2)?);
        let data = i + 30 + name_len + extra;
        let raw = zip.get(data..data + size)?;
        if zip.get(i + 30..i + 30 + name_len)? == name.as_bytes() {
            if method == 0 {
                return Some(raw.to_vec());
            }
            let mut out = Vec::new();
            std::io::Read::read_to_end(&mut flate2::read::DeflateDecoder::new(raw), &mut out).ok()?;
            return Some(out);
        }
        i = data + size;
    }
    None
}

/// Every name in a zip archive.
fn zip_names(zip: &[u8]) -> Vec<String> {
    let at = |i: usize, n: usize| zip.get(i..i + n).map(|b| b.iter().rev().fold(0usize, |v, b| v << 8 | usize::from(*b)));
    let (mut i, mut names) = (0, Vec::new());
    while zip.get(i..).is_some_and(|z| z.starts_with(b"PK\x03\x04")) {
        let (Some(size), Some(name_len), Some(extra)) = (at(i + 18, 4), at(i + 26, 2), at(i + 28, 2)) else { break };
        names.extend(zip.get(i + 30..i + 30 + name_len).map(|n| String::from_utf8_lossy(n).into_owned()));
        i += 30 + name_len + extra + size;
    }
    names
}

#[test]
fn the_golden_journey_detect_correct_save_as_reopen_export() {
    let dir = std::env::temp_dir().join(format!("pdfcraft-chess-golden-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (store, saved, epub) = (dir.join("store"), dir.join("golden copy.pdf"), dir.join("golden.epub"));
    let bytes = book();
    earlier_store(&store, "golden.pdf", &bytes);
    {
        // Detect: diagram 1 is verified by the earlier build's store.
        let mut h = harness("golden.pdf", None, bytes, store.clone());
        assert!(h.state_mut().execute("chess.scan"));
        h.run_steps(4);
        assert_eq!(kind(&h, 0).as_deref(), Some("Verified"), "the earlier build's correction is read");
        // Correct diagram 2 and confirm.
        h.get_all_by_label_contains("Diagram 2 · ").next().expect("the drawn board is listed").click();
        h.run_steps(3);
        let ed = h.state_mut().chess.editor.as_mut().expect("the editor opens on it");
        assert_eq!((ed.page, ed.index), (0, 1));
        ed.position = pdfcraft_engine::chess::pdfcraft_chess::fen::parse(EDITED).unwrap();
        h.run_steps(2);
        assert!(h.state_mut().confirm_chess_diagram(false), "confirmed");
        h.run_steps(3);
        assert_eq!(kind(&h, 1).as_deref(), Some("Corrected"));
        // Save As.
        h.state_mut().save_override = Some(saved.to_string_lossy().into_owned());
        assert!(h.state_mut().save_active(pdfcraft_ui_egui::SaveTarget::As));
        h.run_steps(2);
    }
    // Reopen the saved file: both corrections found, then Export EPUB.
    let saved_bytes = std::fs::read(&saved).unwrap();
    let mut h = harness("golden copy.pdf", Some(saved.to_string_lossy().into_owned()), saved_bytes, store.clone());
    assert!(h.state_mut().execute("chess.scan"));
    h.run_steps(4);
    assert_eq!((kind(&h, 0).as_deref(), kind(&h, 1).as_deref()), (Some("Verified"), Some("Corrected")), "the saved file finds both");
    let id = h.state().views[0].id;
    h.state_mut().export_chess_to(id, ChessFormat::Epub, &epub);
    h.run_steps(4);
    let zip = std::fs::read(&epub).expect("the EPUB is written");
    let pages: String = zip_names(&zip)
        .iter()
        .filter(|n| n.ends_with(".xhtml"))
        .filter_map(|n| zip_entry(&zip, n))
        .map(|x| String::from_utf8_lossy(&x).into_owned())
        .collect();
    assert!(pages.contains(&format!("data-fen=\"{EDITED}\"")), "the corrected position is in the EPUB");
    assert!(pages.contains(&format!("data-fen=\"{START_FEN}\"")), "and the verified one");
    // The store holds this build's format now, the earlier file read and written over.
    let files: Vec<String> = std::fs::read_dir(&store).unwrap().flatten().filter_map(|e| std::fs::read_to_string(e.path()).ok()).collect();
    assert!(files.iter().any(|f| f.contains("\"version\":2") && f.contains("golden-v1")), "{files:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
