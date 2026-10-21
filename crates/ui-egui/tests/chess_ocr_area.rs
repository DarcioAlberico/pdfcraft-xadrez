//! Chess ▸ PGN toolbar ▸ OCR area (T9) in the real shell (egui_kittest): a rectangle drawn on a
//! scanned page is read with chess OCR and captured like a selection. The reader is a stand-in
//! (`set_chess_area_reader`) that "reads" the moves it is given inside the area asked about, so
//! these tests check the interface's plumbing; the engine's own tests read real rasters.

use std::sync::{Arc, Mutex};

use egui::{Pos2, pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use pdfcraft_engine::chess::text_ocr::{AreaError, AreaRead};
use pdfcraft_render::text::TextGlyph;
use pdfcraft_ui_egui::{AreaQuery, PdfCraftApp};

mod support;

const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
const MOVES: &str = "1.e4 e5 2.Nf3 Nc6";

/// A one-page 600 × 800 pt PDF with `content`; copying is refused when `locked`.
fn pdf(content: &str, locked: bool) -> Vec<u8> {
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
    if !locked {
        return out;
    }
    let mut doc = pdfcraft_cos::Document::open(Arc::new(out)).unwrap();
    doc.set_encryption(&pdfcraft_cos::NewEncryption {
        algorithm: pdfcraft_cos::Algorithm::Aes256,
        user_password: "",
        owner_password: "owner",
        // Print only: no copying.
        permissions: 0b0100,
        encrypt_metadata: true,
        seed: [7; 32],
    })
    .unwrap();
    pdfcraft_cos::write_full(&doc, &Default::default()).unwrap()
}

/// A "scan": ink where the moves are printed (a dark band, 300 pt from the top), no text layer.
fn scan(locked: bool) -> Vec<u8> {
    pdf("0.2 g 60 488 300 14 re f\n", locked)
}

/// A born-digital page: the moves as Helvetica text, 300 pt from the top.
fn text_page() -> Vec<u8> {
    pdf(&format!("BT /F1 14 Tf 60 500 Td ({MOVES}) Tj ET\n"), false)
}

/// `text` as glyphs on a line inside `rect` (view space), one glyph per character, a gap between
/// words: what a reader of that area would return.
fn glyphs_in(text: &str, rect: [f32; 4]) -> Vec<TextGlyph> {
    let (mut x, top) = (rect[0] + 4.0, rect[1] + 4.0);
    let mut out = Vec::new();
    for word in text.split(' ') {
        for c in word.chars() {
            let r = [x, top, x + 6.0, top + 10.0];
            out.push(TextGlyph::new(c.to_string(), r, [[r[0], r[3]], [r[2], r[3]], [r[2], r[1]], [r[0], r[1]]], [1.0, 0.0]));
            x += 6.0;
        }
        x += 5.0;
    }
    out
}

/// The queries the stand-in reader was asked.
type Asked = Arc<Mutex<Vec<AreaQuery>>>;

/// A reader that reads `text` in any area, with `edits` words changed by the repair.
fn reader(text: &'static str, edits: usize, asked: &Asked) -> impl Fn(&AreaQuery) -> Result<AreaRead, AreaError> + Send + Sync + 'static {
    let asked = asked.clone();
    move |q: &AreaQuery| {
        asked.lock().unwrap().push(q.clone());
        Ok(AreaRead {
            page: q.page,
            rect_user: q.rect_user,
            rect_view: q.rect_view,
            dpi: 300.0,
            text: pdfcraft_render::text::layout(glyphs_in(text, q.rect_view)),
            lines: 1,
            edits,
            ..AreaRead::default()
        })
    }
}

/// The app on `bytes` with the start position on the board and the board shown.
fn harness(bytes: Vec<u8>) -> Harness<'static, PdfCraftApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 1000.0)).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("scan.pdf", None, bytes).unwrap();
        app.set_option("left", "closed").unwrap();
        app.run_inline = true;
        app
    });
    h.run_steps(4);
    support::wait_until(&mut h, "the page on screen", |app| app.views[0].page_screen_rect(0).is_some());
    h.state_mut().chess.board_mut().load_fen(START, false, None);
    h.state_mut().show_chess_board();
    h.run_steps(3);
    h
}

/// Page point `(x, y)` (view space, points from the top left) on screen.
fn at(h: &Harness<'static, PdfCraftApp>, x: f32, y: f32) -> Pos2 {
    let r = h.state().views[0].page_screen_rect(0).expect("the page is on screen");
    pos2(r.left() + x * r.width() / 600.0, r.top() + y * r.height() / 800.0)
}

fn drag(h: &mut Harness<'static, PdfCraftApp>, from: Pos2, to: Pos2) {
    h.hover_at(from);
    h.run_steps(1);
    h.drag_at(from);
    h.run_steps(1);
    for k in 1..=4 {
        h.hover_at(from + (to - from) * (k as f32 / 4.0));
        h.run_steps(1);
    }
    h.drop_at(to);
    h.run_steps(3);
}

/// Draw the area around the moves (view space 50..400 × 280..320).
fn draw_area(h: &mut Harness<'static, PdfCraftApp>) {
    let (a, b) = (at(h, 50.0, 280.0), at(h, 400.0, 320.0));
    drag(h, a, b);
}

fn movetext(h: &Harness<'static, PdfCraftApp>) -> String {
    h.state().chess.board().game.movetext()
}

fn toast(h: &Harness<'static, PdfCraftApp>) -> String {
    h.state().toast.as_ref().map(|t| t.0.clone()).unwrap_or_default()
}

fn close(a: [f32; 4], b: [f32; 4]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1.5)
}

/// On a page with no text layer, arming Capture switches the OCR area on by itself and the chip
/// says so; a rectangle drawn on the page is read and goes in like a selection, as one undo step,
/// each move keeping where it was printed.
#[test]
fn on_a_scan_the_area_switches_on_and_is_captured() {
    let mut h = harness(scan(false));
    let asked: Asked = Arc::default();
    h.state_mut().set_chess_area_reader(Some(reader(MOVES, 0, &asked)));
    assert!(h.state_mut().execute("chess.tool.capture"));
    support::wait_until(&mut h, "the OCR area switching on", |app| app.chess.capture.area.on);
    h.run_steps(2);
    assert!(h.state().chess.capture.area.auto, "the page switched it on");
    h.get_by_label("OCR area");
    h.get_by_label("OCR area (O)");
    draw_area(&mut h);
    let q = asked.lock().unwrap().clone();
    assert_eq!(q.len(), 1, "one area read: {q:?}");
    assert_eq!(q[0].page, 0);
    assert!(close(q[0].rect_view, [50.0, 280.0, 400.0, 320.0]), "{q:?}");
    assert!(close(q[0].rect_user, [50.0, 480.0, 400.0, 520.0]), "{q:?}");
    assert_eq!(movetext(&h), "1. e4 e5 2. Nf3 Nc6");
    assert!(h.state().chess.capture.pending.is_none(), "applied without the preview");
    let board = h.state().chess.board();
    assert_eq!(board.undo_label().as_deref(), Some("Capture line"));
    let source = board.game.source(board.game.current()).expect("the move keeps its place on the page");
    assert_eq!(source.page, 0);
    assert!(source.rect_user[1] >= 480.0 && source.rect_user[3] <= 520.0 && source.rect_user[0] > 50.0, "{source:?}");
    assert!(h.state().views[0].selected_text().is_none_or(|t| t.is_empty()), "the drag selected no text");
    // The tool and the switch stay on.
    assert!(h.state().chess.capture.area.on);
    assert_eq!(format!("{:?}", h.state().chess.tools.tool), "Capture(Auto)");
}

/// On a page whose text layer is good, Capture selects text; the rail's OCR area (or its command)
/// switches the area on and arms Capture when nothing is armed; the chip's "OCR area" turns it
/// off again. What the repair changed is shown in the preview, even when the plan fits.
#[test]
fn on_a_text_page_the_area_is_the_users_choice() {
    let mut h = harness(text_page());
    let asked: Asked = Arc::default();
    h.state_mut().set_chess_area_reader(Some(reader(MOVES, 1, &asked)));
    assert!(h.state_mut().execute("chess.tool.capture"));
    h.run_steps(6);
    assert!(!h.state().chess.capture.area.on, "a good layer is selected, not read");
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert_eq!(format!("{:?}", h.state().chess.tools.tool), "Navigate");
    h.get_by_label("OCR area (O)").click();
    h.run_steps(3);
    assert!(h.state().chess.capture.area.on && !h.state().chess.capture.area.auto);
    assert_eq!(format!("{:?}", h.state().chess.tools.tool), "Capture(Auto)", "the switch armed Capture");
    draw_area(&mut h);
    assert_eq!(asked.lock().unwrap().len(), 1);
    assert_eq!(movetext(&h), "", "a word the repair changed: previewed first");
    assert!(h.state().chess.capture.pending.is_some());
    h.get_by_label_contains("Chess OCR corrected or doubted some words");
    h.get_by_label_contains("Apply").click();
    h.run_steps(3);
    assert_eq!(movetext(&h), "1. e4 e5 2. Nf3 Nc6");
    // The chip's switch turns it off: a drag selects text again.
    h.get_by_label("OCR area").click();
    h.run_steps(2);
    assert!(!h.state().chess.capture.area.on);
    assert!(h.state_mut().execute("chess.tool.ocr_area"));
    h.run_steps(2);
    assert!(h.state().chess.capture.area.on, "the command switches it too");
}

/// Reading an area is extraction: a document that forbids copying refuses it with the usual
/// notice, and nothing is read.
#[test]
fn a_document_that_forbids_copying_refuses_the_area() {
    let mut h = harness(scan(true));
    let asked: Asked = Arc::default();
    h.state_mut().set_chess_area_reader(Some(reader(MOVES, 0, &asked)));
    assert!(h.state_mut().execute("chess.tool.capture"));
    support::wait_until(&mut h, "the OCR area switching on", |app| app.chess.capture.area.on);
    draw_area(&mut h);
    assert!(asked.lock().unwrap().is_empty(), "nothing was read");
    assert_eq!(movetext(&h), "");
    assert_eq!(toast(&h), "The document's security settings don't allow copying text and images");
}

/// Without the glyph model the notice says what to install where; an area with nothing in it
/// says so too.
#[test]
fn no_model_and_nothing_read_are_said() {
    let mut h = harness(scan(false));
    h.state_mut().set_chess_area_reader(Some(|_: &AreaQuery| Err(AreaError::NoModel)));
    assert!(h.state_mut().execute("chess.tool.capture"));
    support::wait_until(&mut h, "the OCR area switching on", |app| app.chess.capture.area.on);
    draw_area(&mut h);
    assert!(toast(&h).contains("chess-glyphs") && toast(&h).contains("PDFCRAFT_MODELS"), "{}", toast(&h));
    let asked: Asked = Arc::default();
    h.state_mut().set_chess_area_reader(Some(reader("", 0, &asked)));
    draw_area(&mut h);
    assert_eq!(toast(&h), "Chess OCR read nothing in the area.");
    assert_eq!(movetext(&h), "");
}

/// Esc drops an area being read on a worker: the chip's spinner goes, the tool stays armed, and
/// what the worker reads afterwards is thrown away.
#[test]
fn esc_drops_an_area_being_read() {
    let mut h = harness(scan(false));
    h.state_mut().run_inline = false;
    let (go, wait) = std::sync::mpsc::channel::<()>();
    let wait = Mutex::new(wait);
    let asked: Asked = Arc::default();
    let read = reader(MOVES, 0, &asked);
    h.state_mut().set_chess_area_reader(Some(move |q: &AreaQuery| {
        // Held until the test lets it go (or ten seconds pass).
        let _ = wait.lock().unwrap().recv_timeout(std::time::Duration::from_secs(10));
        read(q)
    }));
    assert!(h.state_mut().execute("chess.tool.capture"));
    support::wait_until(&mut h, "the OCR area switching on", |app| app.chess.capture.area.on);
    draw_area(&mut h);
    assert!(h.state().chess_area_reading(), "the area is being read");
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert!(!h.state().chess_area_reading(), "Esc dropped it");
    assert_eq!(format!("{:?}", h.state().chess.tools.tool), "Capture(Auto)", "the tool stays armed");
    go.send(()).unwrap();
    support::wait_until(&mut h, "the worker reading", |_| !asked.lock().unwrap().is_empty());
    h.run_steps(4);
    assert_eq!(movetext(&h), "", "the dropped read is not captured");
    assert!(h.state().chess.capture.pending.is_none());
}

// ─── T10: the OCR's readings of a move that did not play ─────────────────────────────────────

use pdfcraft_engine::chess::text_ocr::GlyphAlternatives;

/// A reader that reads `text`, every glyph sure of itself except the `8`, which it nearly took
/// for a `3`.
fn doubtful_reader(text: &'static str) -> impl Fn(&AreaQuery) -> Result<AreaRead, AreaError> + Send + Sync + 'static {
    move |q: &AreaQuery| {
        let glyphs = glyphs_in(text, q.rect_view);
        let alternatives = glyphs
            .iter()
            .map(|g| {
                let (margin, choices) = match g.text.as_str() {
                    "8" => (0.1, vec![("8".to_owned(), 0.5), ("3".to_owned(), 0.45), ("5".to_owned(), 0.05)]),
                    t => (0.95, vec![(t.to_owned(), 0.97)]),
                };
                GlyphAlternatives { rect: g.rect, text: g.text.clone(), margin, choices }
            })
            .collect();
        Ok(AreaRead {
            page: q.page,
            rect_user: q.rect_user,
            rect_view: q.rect_view,
            dpi: 300.0,
            text: pdfcraft_render::text::layout(glyphs),
            alternatives,
            lines: 1,
            ..AreaRead::default()
        })
    }
}

/// A move read from a scan that did not play lists its glyphs, the doubtful one underlined; its
/// readings are offered, and picking one rewrites the glyph and reads the area's text again. The
/// game changes only with Apply.
#[test]
fn an_ocr_reading_picked_rewrites_the_glyph_and_reads_again() {
    let mut h = harness(scan(false));
    h.state_mut().set_chess_area_reader(Some(doubtful_reader("1.e4 e5 2.Nf8 Nc6")));
    assert!(h.state_mut().execute("chess.tool.capture"));
    support::wait_until(&mut h, "the OCR area switching on", |app| app.chess.capture.area.on);
    draw_area(&mut h);
    assert!(h.state().chess.capture.pending.is_some(), "2.Nf8 does not play: shown first");
    h.get_by_label("✗ 2.Nf8 — Illegal move").click();
    h.run_steps(2);
    h.get_by_label("Character N");
    h.get_by_label("Character f");
    h.get_by_label("Character 8, doubtful").click();
    h.run_steps(2);
    h.get_by_label("Read the character as 5");
    h.get_by_label("Read the character as 3").click();
    h.run_steps(3);
    assert_eq!(movetext(&h), "", "read again, not applied");
    let p = h.state().chess.capture.pending.as_ref().expect("still waiting");
    let c = p.plan.candidates.get(p.choice.unwrap()).unwrap();
    assert_eq!((c.played, c.read.stops.len()), (4, 0), "{:?}", c.read.stops);
    h.get_by_label_contains("Apply").click();
    h.run_steps(3);
    assert_eq!(movetext(&h), "1. e4 e5 2. Nf3 Nc6");
    // The move keeps where it was printed.
    let board = h.state().chess.board();
    let nf3 = board.game.node_at(&pdfcraft_engine::chess::pdfcraft_chess::game::NodePath::new(vec![0, 0, 0])).unwrap();
    assert!(board.game.source(nf3).is_some());
}
