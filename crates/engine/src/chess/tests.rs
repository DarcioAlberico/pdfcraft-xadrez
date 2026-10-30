//! Chess ▸ Scan diagrams on synthetic PDFs built here: a board set in a chess diagram font, a
//! board drawn as filled squares, and a board drawn with `pdfcraft_chess::draw`, rasterized in
//! the test and embedded as an image. No model ships, so reading with a model uses a stand-in
//! classifier; the user's real models are tried only when `PDFCRAFT_CHESS_TEST_MODELS` names
//! their folder.

use std::sync::Arc;

use pdfcraft_chess::draw::{BoardStyle, Paint, PieceSet, Rgb, Segment, board};
use pdfcraft_chess::fen::parse_placement;
use pdfcraft_chess::placement::Color;
use pdfcraft_chess::recognize::{CLASS_COUNT, class_of};
use pdfcraft_chessvision::board::{ReadingSource, SquareClassifier, Unreadable};
use pdfcraft_chessvision::detect::Source;
use pdfcraft_chessvision::models::ModelError;
use proptest::prelude::*;

use super::*;
use crate::Edit;

const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR";
const OPENING: &str = "r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R";
/// The start position in the Chess Merida layout, top rank first: light squares lower case,
/// dark upper case; empty light ' ', empty dark '+'.
const MERIDA_START: [&str; 8] = ["tMvWlVmT", "OoOoOoOo", " + + + +", "+ + + + ", " + + + +", "+ + + + ", "pPpPpPpP", "RnBqKbNr"];

/// A one-page PDF of 600 × 800 pt drawing `content`, with the font `F1` (`/BaseFont font`,
/// every code 1000 units wide), an image `Im1` when given (8-bit gray) and extra page keys.
fn pdf(content: &str, font: &str, image: Option<(u32, u32, &[u8])>, page_extra: &str) -> Vec<u8> {
    pdf_fonts(content, &[font], image, page_extra)
}

/// [`pdf`] with several fonts: `F1` is `fonts[0]`, `F2` is `fonts[1]`, and so on.
fn pdf_fonts(content: &str, fonts: &[&str], image: Option<(u32, u32, &[u8])>, page_extra: &str) -> Vec<u8> {
    let widths = vec!["1000"; 95].join(" ");
    let font_dict =
        |name: &str| format!("<< /Type /Font /Subtype /Type1 /BaseFont /{name} /FirstChar 32 /LastChar 126 /Widths [{widths}] >>").into_bytes();
    // F1 is object 5 and the image object 6; further fonts follow from object 7.
    let refs: String = (0..fonts.len()).map(|i| format!(" /F{} {} 0 R", i + 1, if i == 0 { 5 } else { 6 + i })).collect();
    let mut objs: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 600 800] >>".to_vec(),
        format!("<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /Font <<{refs} >> /XObject << /Im1 6 0 R >> >> {page_extra} >>")
            .into_bytes(),
        {
            let mut s = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
            s.extend_from_slice(content.as_bytes());
            s.extend_from_slice(b"\nendstream");
            s
        },
        font_dict(fonts.first().copied().unwrap_or("Helvetica")),
    ];
    let (w, h, data) = image.unwrap_or((1, 1, &[255]));
    let mut im = format!(
        "<< /Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceGray /BitsPerComponent 8 /Length {} >>\nstream\n",
        data.len()
    )
    .into_bytes();
    im.extend_from_slice(data);
    im.extend_from_slice(b"\nendstream");
    objs.push(im);
    objs.extend(fonts.iter().skip(1).map(|f| font_dict(f)));
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

/// Eight lines of diagram-font text, 20 pt squares, top-left corner at (x, y) in user space
/// measured from the top of a 800 pt page.
fn font_board(x: f32, top: f32, lines: [&str; 8]) -> String {
    let mut s = format!("BT /F1 20 Tf {x} {} Td ", 800.0 - top - 16.0);
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            s.push_str("0 -20 Td ");
        }
        s.push_str(&format!("({line}) Tj "));
    }
    s.push_str("ET\n");
    s
}

/// The 32 dark squares of a 160 pt board at (x, y) (user space, bottom-left), as one fill of 32
/// rectangles.
fn vector_board(x: f32, y: f32) -> String {
    let mut s = String::from("0.45 g\n");
    for rank in 0..8 {
        for file in 0..8 {
            if (rank + file) % 2 == 0 {
                s.push_str(&format!("{} {} 20 20 re\n", x + file as f32 * 20.0, y + rank as f32 * 20.0));
            }
        }
    }
    s.push_str("f\n");
    s
}

/// A drawing at `px` pixels a side, rasterized to 8-bit gray: fills by the nonzero or even-odd
/// rule at each pixel centre, strokes as a band around the outline.
fn rasterize(placement: &str, pieces: &PieceSet, px: u32) -> Vec<u8> {
    let style = BoardStyle { border: 0.0, coordinates: false, ..BoardStyle::default() };
    let drawing = board(&parse_placement(placement).unwrap(), pieces, &style);
    let scale = px as f32 / drawing.width;
    let n = px as usize;
    let mut data = vec![255u8; n * n];
    let luma = |Rgb(r, g, b): Rgb| ((299 * u32::from(r) + 587 * u32::from(g) + 114 * u32::from(b)) / 1000) as u8;
    for layer in &drawing.layers {
        let polys: Vec<Vec<(f32, f32)>> = layer
            .subpaths
            .iter()
            .map(|sp| {
                let mut pts = vec![(sp.start.0 * scale, sp.start.1 * scale)];
                let mut at = sp.start;
                for seg in &sp.segments {
                    match *seg {
                        Segment::Line(x, y) => at = (x, y),
                        Segment::Cubic(x1, y1, x2, y2, x, y) => {
                            for k in 1..10 {
                                let t = k as f32 / 10.0;
                                let m = 1.0 - t;
                                let p = |a: f32, b: f32, c: f32, d: f32| m * m * m * a + 3.0 * m * m * t * b + 3.0 * m * t * t * c + t * t * t * d;
                                pts.push((p(at.0, x1, x2, x) * scale, p(at.1, y1, y2, y) * scale));
                            }
                            at = (x, y);
                        }
                    }
                    pts.push((at.0 * scale, at.1 * scale));
                }
                pts
            })
            .collect();
        let all: Vec<&(f32, f32)> = polys.iter().flatten().collect();
        if all.is_empty() {
            continue;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for &&(x, y) in &all {
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
        }
        let (pad, color, mode) = match layer.paint {
            Paint::Fill { color, even_odd } => (0.0, color, Some(even_odd)),
            Paint::Stroke { color, width } => (width * scale / 2.0 + 1.0, color, None),
        };
        let clampi = |v: f32| (v.max(0.0) as usize).min(n);
        for py in clampi(y0 - pad)..clampi(y1 + pad + 1.0) {
            for pxi in clampi(x0 - pad)..clampi(x1 + pad + 1.0) {
                let (cx, cy) = (pxi as f32 + 0.5, py as f32 + 0.5);
                let inside = match mode {
                    Some(even_odd) => {
                        let w: i32 = polys.iter().map(|p| winding(p, (cx, cy))).sum();
                        if even_odd { w % 2 != 0 } else { w != 0 }
                    }
                    None => polys.iter().any(|p| near_outline(p, (cx, cy), pad - 1.0)),
                };
                if inside {
                    data[py * n + pxi] = luma(color);
                }
            }
        }
    }
    data
}

fn winding(poly: &[(f32, f32)], (px, py): (f32, f32)) -> i32 {
    let mut w = 0;
    for i in 0..poly.len() {
        let ((x0, y0), (x1, y1)) = (poly[i], poly[(i + 1) % poly.len()]);
        let side = (x1 - x0) * (py - y0) - (px - x0) * (y1 - y0);
        if y0 <= py && y1 > py && side > 0.0 {
            w += 1;
        } else if y0 > py && y1 <= py && side < 0.0 {
            w -= 1;
        }
    }
    w
}

fn near_outline(poly: &[(f32, f32)], (px, py): (f32, f32), half: f32) -> bool {
    (0..poly.len()).any(|i| {
        let ((x0, y0), (x1, y1)) = (poly[i], poly[(i + 1) % poly.len()]);
        let (dx, dy) = (x1 - x0, y1 - y0);
        let t = (((px - x0) * dx + (py - y0) * dy) / (dx * dx + dy * dy).max(1e-9)).clamp(0.0, 1.0);
        (px - x0 - t * dx).hypot(py - y0 - t * dy) <= half
    })
}

/// A page with all three kinds: a font diagram top left, a vector board under it and an image
/// of `OPENING` (`px` pixels a side, drawn 160 pt wide) top right.
fn three_boards(px: u32) -> Vec<u8> {
    let image = rasterize(OPENING, &PieceSet::geometric(), px);
    let content = format!("{}{}q 160 0 0 160 360 580 cm /Im1 Do Q\n", font_board(60.0, 60.0, MERIDA_START), vector_board(60.0, 240.0));
    pdf(&content, "ChessMerida", Some((px, px, &image)), "")
}

fn open(bytes: Vec<u8>) -> (Session, DocId) {
    let mut s = Session::new();
    let id = s.open("chess.pdf", None, Arc::new(bytes), None).unwrap();
    (s, id)
}

/// Settings that look for the piece model in an empty folder: no model, whatever the machine has.
fn no_model() -> ChessScanSettings {
    let dir = std::env::temp_dir().join(format!("pdfcraft-chess-no-model-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    ChessScanSettings { models_dir: Some(dir), ..Default::default() }
}

/// A stand-in piece model: every board's printed squares read as `placement` with 0.99, and its
/// turned squares as empty (so only the printed reading is legal), or as `placement` too.
struct StandIn(Vec<u8>, bool);

impl StandIn {
    fn new(placement: &str) -> StandIn {
        StandIn(parse_placement(placement).unwrap().reading_order().iter().map(|p| class_of(*p)).collect(), false)
    }

    fn both_ways(placement: &str) -> StandIn {
        StandIn(StandIn::new(placement).0, true)
    }
}

impl SquareClassifier for StandIn {
    fn classify(&self, _squares: &[f32], count: usize) -> Result<Vec<f32>, ModelError> {
        let mut out = vec![0.01 / (CLASS_COUNT as f32 - 1.0); count * CLASS_COUNT];
        for i in 0..count {
            let within = i % 128;
            let class = if within < 64 || self.1 { self.0[within % 64] } else { class_of(None) };
            out[i * CLASS_COUNT + class as usize] = 0.99;
        }
        Ok(out)
    }
}

fn sources(page: &ChessPage) -> Vec<Vec<Source>> {
    page.diagrams.iter().map(|d| d.sources.clone()).collect()
}

/// U4: the model cache. Installed models (the web app) answer when no folder is given, never for
/// a folder; models kept for a folder are read again on reload, while a holder of the old ones
/// keeps them; models not worth keeping are loaded each time; and the folders kept are capped.
#[test]
fn the_model_cache_installs_reloads_and_keeps_what_jobs_hold() {
    let mut cache: ModelCache<u32> = ModelCache::new();
    let loads = std::cell::Cell::new(0u32);
    let load = |_: Option<&Path>| {
        loads.set(loads.get() + 1);
        loads.get()
    };
    let keep = |_: &u32| true;
    let dir = Path::new("models");
    let first = cache.get(Some(dir), load, keep);
    assert_eq!(*cache.get(Some(dir), load, keep), 1, "kept");
    let reloaded = cache.reload(Some(dir), load, keep);
    assert_eq!((*first, *reloaded, loads.get()), (1, 2, 2), "a job holding the old models keeps them");
    assert_eq!(*cache.get(Some(dir), load, keep), 2);

    let installed = cache.install(Some(100)).unwrap();
    assert!(Arc::ptr_eq(&cache.get(None, load, keep), &installed));
    assert!(Arc::ptr_eq(&cache.reload(None, load, keep), &installed), "reloading the search keeps installed models");
    assert_eq!(*cache.get(Some(dir), load, keep), 2, "a folder given is that folder's");
    assert!(cache.install(None).is_none());
    assert_eq!(*cache.get(None, load, keep), 3, "uninstalled: the search again");

    let never = |_: &u32| false;
    let before = loads.get();
    cache.get(Some(Path::new("glyphs-only")), load, never);
    cache.get(Some(Path::new("glyphs-only")), load, never);
    assert_eq!(loads.get(), before + 2, "not kept: loaded again each time");
    for i in 0..20 {
        cache.get(Some(Path::new(&format!("folder-{i}"))), load, keep);
    }
    assert_eq!(cache.loaded.len(), MAX_MODEL_FOLDERS);
}

/// Without a model: the font diagram is read exactly (legal, accepted, with a lichess link), and
/// the vector board and the image are found but say `NoModel`. The diagrams come in column
/// order: the left column top to bottom, then the right.
#[test]
fn without_a_model_font_diagrams_are_read_and_the_others_say_no_model() {
    let (mut s, id) = open(three_boards(400));
    let page = s.chess_page(id, 0, &no_model()).unwrap();
    assert!(page.error.is_none(), "{:?}", page.error);
    assert_eq!(page.diagrams.len(), 3, "{:?}", sources(&page));
    // The image is also a contour on the page; the contour's box is the more precise one.
    let found = sources(&page);
    assert_eq!((&found[0], &found[1]), (&vec![Source::Font], &vec![Source::Vector]), "{found:?}");
    assert!(found[2].contains(&Source::Embedded), "{found:?}");
    let readings = page.readings.as_ref().unwrap();
    assert_eq!(readings.len(), 3);
    let font = readings[0].position().unwrap();
    assert_eq!(readings[0].source, ReadingSource::Font);
    assert!(font.fen.starts_with(START), "{}", font.fen);
    assert!(font.accepted && font.min_confidence == 1.0);
    assert_eq!(lichess_url(font).as_deref(), Some("https://lichess.org/analysis/rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR%20w%20KQkq%20-%200%201"));
    for r in &readings[1..] {
        assert_eq!(r.outcome, Err(Unreadable::NoModel), "{r:?}");
        assert_eq!(r.source, ReadingSource::Model);
    }
    // Indices are positions in the reading order, and every reading keeps its resolution.
    assert_eq!(readings.iter().map(|r| r.index).collect::<Vec<_>>(), [0, 1, 2]);
    assert!(readings.iter().all(|r| (r.dpi - page.dpi).abs() < 1.0), "{readings:?}");
    assert!((page.dpi - 220.0).abs() < 1.0, "{}", page.dpi);
}

/// With a model, the page is read: the image board at the image's own resolution (288 dpi here,
/// finer than the 220 dpi detection raster), not cut from the coarser detection raster.
#[test]
fn a_box_only_diagram_is_read_at_its_own_resolution_not_the_detection_raster() {
    let (s, id) = open(three_boards(640));
    let job = s.chess_scan_job(id, &[], &no_model(), true).unwrap();
    let scan = job.run_with(Some(&StandIn::new(OPENING)), |_, _| true);
    let page = &scan.pages[0];
    let readings = page.readings.as_ref().unwrap();
    let image = &readings[2];
    assert!(page.diagrams[2].sources.contains(&Source::Embedded), "{:?}", sources(page));
    assert_eq!(image.source, ReadingSource::Model);
    // 640 px over 160 pt is 288 dpi; the vector board's hint is 300, the finest on the page.
    assert!(image.dpi >= 288.0 && image.dpi > page.dpi + 50.0, "read at {} dpi, detected at {}", image.dpi, page.dpi);
    let read = image.position().unwrap();
    assert!(read.fen.starts_with(OPENING), "{}", read.fen);
    assert!(lichess_url(read).is_some());
    // The font diagram is still read from the font, not by the model.
    assert_eq!(readings[0].source, ReadingSource::Font);
    // A stand-in's readings are not cached: another classifier would read differently.
    let mut s = s;
    assert!(!s.store_chess_scan(&scan));
}

/// The start position set in Chess Merida, with `number` (in `number_font`) and a move under it.
fn board_with_move_number(number: &str, number_font: &str) -> Vec<u8> {
    // The board fills 50..210 pt from the top; the game goes on at 240 pt.
    let content = format!("{}BT /F2 10 Tf 50 550 Td ({number}) Tj /F3 10 Tf (Nf6 24.Bb5) Tj ET\n", font_board(50.0, 50.0, MERIDA_START));
    pdf_fonts(&content, &["ChessMerida", number_font, "Times-Roman"], None, "")
}

/// In a born-digital book the first move number under a diagram is set in bold, and says whose
/// move it is: `23...` makes it Black's, numbered 23, in the FEN and the lichess link.
#[test]
fn a_bold_move_number_under_a_diagram_sets_the_side_to_move() {
    let (mut s, id) = open(board_with_move_number("23...", "Times-Bold"));
    let page = s.chess_page(id, 0, &no_model()).unwrap();
    let read = page.readings.as_ref().unwrap()[0].position().unwrap();
    assert_eq!((read.side.color, read.side.source), (Color::Black, SideSource::MoveNumber), "{read:?}");
    assert_eq!(read.fen, format!("{START} b KQkq - 0 23"));
    assert_eq!(
        lichess_url(read).as_deref(),
        Some("https://lichess.org/analysis/rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR%20b%20KQkq%20-%200%2023")
    );

    // The same number in the body font is prose: White, assumed.
    let (mut s, id) = open(board_with_move_number("23...", "Times-Roman"));
    let page = s.chess_page(id, 0, &no_model()).unwrap();
    let read = page.readings.as_ref().unwrap()[0].position().unwrap();
    assert_eq!((read.side.color, read.side.source), (Color::White, SideSource::Default), "{read:?}");
}

/// An illegal reading (no kings) is reported, but gets no lichess link.
#[test]
fn an_illegal_reading_gets_no_lichess_link() {
    let (s, id) = open(three_boards(400));
    // Sixteen queens and no king: more than the decoder may change to make it legal.
    let scan =
        s.chess_scan_job(id, &[], &no_model(), true).unwrap().run_with(Some(&StandIn::both_ways("QQQQQQQQ/QQQQQQQQ/8/8/8/8/8/8")), |_, _| true);
    let readings = scan.pages[0].readings.as_ref().unwrap();
    let vector = readings[1].position().unwrap();
    assert!(!vector.legality.is_legal_for_some_side(), "{vector:?}");
    assert!(!vector.accepted);
    assert_eq!(lichess_url(vector), None);
}

/// A reading the decoder had to repair (a lone pawn: no kings) is never accepted: a person
/// confirms it (blueprint section 2).
#[test]
fn a_repaired_reading_goes_to_review() {
    let (s, id) = open(three_boards(400));
    let scan = s.chess_scan_job(id, &[], &no_model(), true).unwrap().run_with(Some(&StandIn::new("8/8/8/3p4/8/8/8/8")), |_, _| true);
    let vector = scan.pages[0].readings.as_ref().unwrap()[1].position().unwrap().clone();
    assert!(vector.repaired(), "{vector:?}");
    assert!(!vector.accepted && vector.min_confidence <= 0.5, "{vector:?}");
    // Legal once repaired, but not linked: the kings are the decoder's, not the book's.
    assert!(vector.legality.allows(vector.side.color), "{vector:?}");
    assert_eq!(lichess_url(&vector), None);
}

/// Plan "Carregar PGN do livro", R3: a reading whose position the book's PGN has is confirmed by
/// it, with confidence or with a doubt; what the user did outranks it, and a position the PGN only
/// nearly has is not.
#[test]
fn a_reading_the_books_pgn_has_is_confirmed_by_it() {
    use crate::chess::corrections::{Kind, Status};
    use pdfcraft_chessvision::doc::reference::ReferenceIndex;
    let (s, id) = open(three_boards(400));
    let scan = s.chess_scan_job(id, &[], &no_model(), true).unwrap().run_with(Some(&StandIn::new("4k3/8/8/8/8/8/8/R3K3")), |_, _| true);
    let reading = &scan.pages[0].readings.as_ref().unwrap()[1];
    let read = Status::of(Some(reading), None);
    assert!(matches!(read, Status::Read | Status::Review), "{read:?}");
    let has = ReferenceIndex::from_pgn("[FEN \"4k3/8/8/8/8/8/8/R3K3 w Q - 0 1\"]\n\n1.Ra8+ *").unwrap();
    let near = ReferenceIndex::from_pgn("[FEN \"4k3/8/8/8/8/8/8/R2K4 w - - 0 1\"]\n\n1.Ra8+ *").unwrap();
    assert_eq!(Status::with_reference(Some(reading), None, Some(&has)), Status::Reference);
    assert!(!Status::Reference.to_review());
    assert_eq!(Status::with_reference(Some(reading), None, Some(&near)), read, "one square off is a suggestion, not a confirmation");
    assert_eq!(Status::with_reference(Some(reading), None, None), read);
    assert_eq!(Status::with_reference(Some(reading), Some(Kind::Corrected), Some(&has)), Status::Corrected);
    assert_eq!(Status::with_reference(None, None, Some(&has)), Status::Unread);
}

/// A turned page (`/Rotate 90`) and a `/UserUnit` page are read the same: the geometry adapter
/// carries rotation and unit to the detectors.
#[test]
fn turned_and_scaled_pages_read_the_same_font_diagram() {
    for extra in ["/Rotate 90", "/Rotate 270", "/UserUnit 2"] {
        let (mut s, id) = open(pdf(&font_board(60.0, 60.0, MERIDA_START), "ChessMerida", None, extra));
        let page = s.chess_page(id, 0, &no_model()).unwrap();
        let readings = page.readings.as_ref().unwrap();
        assert_eq!(readings.len(), 1, "{extra}: {:?}", sources(&page));
        assert!(readings[0].position().unwrap().fen.starts_with(START), "{extra}");
        let user = page.diagrams[0].candidate.bbox_user;
        assert!((user.x0 - 60.0).abs() < 1.0 && (user.width() - 160.0).abs() < 1.0, "{extra}: {user:?}");
    }
}

#[test]
fn geometry_carries_crop_rotation_and_user_unit() {
    let info = PageInfo { width: 800.0, height: 1200.0, label: "1".into(), crop: [10.0, 20.0, 610.0, 420.0], rotation: 90 };
    let g = geometry(&info).unwrap();
    assert_eq!(g.rotation(), Rotation::R90);
    assert_eq!(g.crop_box(), PageRect::new(10.0, 20.0, 610.0, 420.0));
    assert_eq!(g.user_unit(), 2.0);
    assert_eq!(g.view_size(), (800.0, 1200.0));
    let degenerate = PageInfo { width: f32::NAN, height: 0.0, label: "1".into(), crop: [0.0, 0.0, 0.0, 10.0], rotation: 0 };
    assert!(geometry(&degenerate).is_none());
    let nan_size = PageInfo { height: f32::NAN, ..info };
    assert_eq!(geometry(&nan_size).unwrap().user_unit(), 1.0);
}

/// The cache answers the same request from the same result, and drops it when a parameter or
/// the document differs: never a stale page.
#[test]
fn the_cache_answers_only_the_parameters_and_document_state_it_was_made_for() {
    let (mut s, id) = open(three_boards(400));
    let settings = no_model();
    let first = s.chess_page(id, 0, &settings).unwrap();
    assert!(Arc::ptr_eq(&first, &s.chess_page(id, 0, &settings).unwrap()), "the same request is answered from the cache");
    // Detection alone is answered by the read result, whatever the reading settings.
    let detected = s.chess_scan(id, &[0], &ChessScanSettings { max_read_dpi: 300.0, models_dir: None, ..settings.clone() }, false).unwrap();
    assert!(Arc::ptr_eq(&first, &detected[0]));
    // Another reading resolution is another read.
    let coarser = ChessScanSettings { max_read_dpi: 300.0, ..settings.clone() };
    let reread = s.chess_page(id, 0, &coarser).unwrap();
    assert!(!Arc::ptr_eq(&first, &reread));
    let first = s.chess_page(id, 0, &settings).unwrap();
    // Another DPI drops the entry and scans again.
    let finer = ChessScanSettings { dpi: 150.0, ..settings.clone() };
    let other = s.chess_page(id, 0, &finer).unwrap();
    assert!(!Arc::ptr_eq(&first, &other));
    assert!((other.dpi - 150.0).abs() < 1.0);
    assert_eq!(s.chess.len(), 1, "one page, one entry");
    // A detection-only entry does not answer a read.
    let (mut s2, id2) = open(three_boards(400));
    let only = s2.chess_scan(id2, &[0], &settings, false).unwrap();
    assert!(only[0].readings.is_none());
    assert!(s2.chess_page(id2, 0, &settings).unwrap().readings.is_some());
    // An edit makes a new document state: the page is scanned again, and finds what is there now.
    let again = s.chess_page(id, 0, &finer).unwrap();
    assert!(Arc::ptr_eq(&other, &again));
    s.apply(id, Edit::RotatePages { pages: vec![0], degrees: 90 }).unwrap();
    let edited = s.chess_page(id, 0, &finer).unwrap();
    assert!(!Arc::ptr_eq(&other, &edited));
    assert_eq!(edited.geometry.unwrap().rotation(), Rotation::R90);
    // Closing the document forgets its pages.
    s.close(id);
    assert_eq!(s.chess.len(), 0);
}

/// A job made before an edit describes a state that is gone: it is not stored.
#[test]
fn a_scan_of_a_document_that_changed_since_is_not_stored() {
    let (mut s, id) = open(three_boards(400));
    let job = s.chess_scan_job(id, &[], &no_model(), true).unwrap();
    s.apply(id, Edit::RotatePages { pages: vec![0], degrees: 90 }).unwrap();
    let scan = job.run(&models(no_model().models_dir.as_deref()), |_, _| true);
    assert!(!s.store_chess_scan(&scan));
    assert_eq!(s.chess.len(), 0);
    // The same job on the unchanged document is stored.
    let job = s.chess_scan_job(id, &[], &no_model(), true).unwrap();
    let scan = job.run(&models(no_model().models_dir.as_deref()), |_, _| true);
    assert!(s.store_chess_scan(&scan));
    assert_eq!(s.chess.len(), 1);
}

#[test]
fn a_scans_stamp_matches_the_document_until_it_changes() {
    let (mut s, id) = open(three_boards(400));
    let scan = s.chess_scan_job(id, &[0], &no_model(), false).unwrap().run_with(None, |_, _| true);
    assert_eq!(s.chess_stamp(id), Some(scan.stamp()));
    s.apply(id, Edit::RotatePages { pages: vec![0], degrees: 90 }).unwrap();
    assert_ne!(s.chess_stamp(id), Some(scan.stamp()), "an edit makes the scan stale");
    s.undo(id).unwrap();
    assert_ne!(s.chess_stamp(id), Some(scan.stamp()), "so does undoing it: the generation moves on");
    assert_eq!(s.chess_stamp(DocId(9999)), None);
}

#[test]
fn progress_can_stop_a_scan() {
    let (s, id) = open(three_boards(400));
    let scan = s.chess_scan_job(id, &[], &no_model(), false).unwrap().run_with(None, |done, _| done == 0 && false);
    assert!(scan.pages.is_empty());
}

#[test]
fn pages_out_of_range_and_unknown_documents_are_errors() {
    let (mut s, id) = open(three_boards(400));
    assert!(s.chess_page(id, 1, &no_model()).unwrap_err().contains("out of range"));
    assert!(s.chess_page(DocId(999), 0, &no_model()).is_err());
    assert!(s.chess_scan_job(DocId(999), &[], &no_model(), true).is_none());
}

#[test]
fn settings_are_clamped_into_range() {
    let wild = ChessScanSettings { dpi: f32::NAN, max_read_dpi: 1.0, max_diagrams: 0, ..Default::default() }.sanitized();
    assert_eq!((wild.dpi, wild.max_read_dpi, wild.max_diagrams), (220.0, 220.0, 1));
    let big = ChessScanSettings { dpi: 1e9, max_read_dpi: f32::INFINITY, max_diagrams: usize::MAX, ..Default::default() }.sanitized();
    assert_eq!((big.dpi, big.max_read_dpi, big.max_diagrams), (MAX_DPI, MAX_DPI, MAX_DIAGRAMS_PER_PAGE));
}

/// `max_diagrams` keeps the first diagrams in reading order and says there were more.
#[test]
fn a_page_with_more_diagrams_than_the_limit_says_so() {
    let (mut s, id) = open(three_boards(400));
    let page = s.chess_page(id, 0, &ChessScanSettings { max_diagrams: 2, ..no_model() }).unwrap();
    assert!(page.truncated);
    assert_eq!(page.diagrams.iter().map(|d| d.candidate.source).collect::<Vec<_>>(), [Source::Font, Source::Vector]);
}

#[test]
fn rectangles_are_told_from_other_subpaths() {
    let sub = |start: [f64; 2], pts: &[[f64; 2]]| Subpath { start, segments: pts.iter().map(|p| PathSegment::Line(*p)).collect(), closed: true };
    // `re`: three sides given, the fourth implicit; or all four.
    assert_eq!(rectangle(&sub([0.0, 0.0], &[[20.0, 0.0], [20.0, 10.0], [0.0, 10.0]])), Some(PageRect::new(0.0, 0.0, 20.0, 10.0)));
    assert!(rectangle(&sub([0.0, 0.0], &[[0.0, 10.0], [20.0, 10.0], [20.0, 0.0], [0.0, 0.0]])).is_some());
    // A skewed quad, a triangle, a degenerate box, NaN and a curve are not rectangles.
    assert_eq!(rectangle(&sub([0.0, 0.0], &[[20.0, 2.0], [20.0, 10.0], [0.0, 10.0]])), None);
    assert_eq!(rectangle(&sub([0.0, 0.0], &[[20.0, 0.0], [20.0, 10.0]])), None);
    assert_eq!(rectangle(&sub([0.0, 0.0], &[[20.0, 0.0], [20.0, 0.0], [0.0, 0.0]])), None);
    assert_eq!(rectangle(&sub([0.0, 0.0], &[[f64::NAN, 0.0], [20.0, 10.0], [0.0, 10.0]])), None);
    let curve = Subpath { start: [0.0, 0.0], segments: vec![PathSegment::Cubic([1.0, 0.0], [2.0, 0.0], [3.0, 0.0])], closed: true };
    assert_eq!(rectangle(&curve), None);
    // Three corners, one repeated, are not a rectangle either.
    assert_eq!(rectangle(&sub([0.0, 0.0], &[[20.0, 0.0], [20.0, 10.0], [20.0, 0.0]])), None);
}

#[test]
fn fill_colours_become_gray_levels() {
    assert_eq!(gray_of(&PathColor::Gray(0.25)), Some(0.25));
    assert_eq!(gray_of(&PathColor::Gray(f64::NAN)), None);
    assert_eq!(gray_of(&PathColor::Rgb([1.0, 1.0, 1.0])), Some(1.0));
    assert_eq!(gray_of(&PathColor::Cmyk([0.0, 0.0, 0.0, 1.0])), Some(0.0));
    assert_eq!(gray_of(&PathColor::Other(vec![0.5])), None);
}

#[test]
fn premultiplied_pixels_are_seen_over_white() {
    let gray = to_gray(&[0, 0, 0, 255, 0, 0, 0, 0, 255, 255, 255, 255, 0, 0, 0, 128], 4, 1).unwrap();
    assert_eq!(gray.data(), &[0, 255, 255, 127]);
    assert!(to_gray(&[0, 0, 0], 1, 1).is_none(), "too few bytes");
    assert!(to_gray(&[], 0, 0).is_none());
}

#[test]
fn crops_are_clamped_to_the_raster() {
    let image = GrayImage::new(4, 3, (0..12).collect()).unwrap();
    let c = crop(&image, PixelRect { x0: 2, y0: 1, x1: 9, y1: 9 }).unwrap();
    assert_eq!((c.width(), c.height(), c.data()), (2, 2, &[6, 7, 10, 11][..]));
    assert!(crop(&image, PixelRect { x0: 4, y0: 0, x1: 9, y1: 3 }).is_none());
    assert!(crop(&image, PixelRect { x0: 3, y0: 2, x1: 1, y1: 1 }).is_none());
}

/// With `PDFCRAFT_CHESS_TEST_MODELS` naming a folder with the user's models, the image board is
/// read by the real piece model, at the image's resolution. The board is drawn with the user's
/// diagram font when one is installed (the model knows those pieces), otherwise with the
/// geometric set, which the model was not trained on: then only the plumbing is checked.
#[test]
fn the_users_models_read_an_embedded_board() {
    let Some(dir) = std::env::var_os("PDFCRAFT_CHESS_TEST_MODELS") else {
        eprintln!("skipped: set PDFCRAFT_CHESS_TEST_MODELS to the folder of the chess models");
        return;
    };
    let (set, source, _) = pdfcraft_chessvision::pieces::find();
    let image = rasterize(OPENING, &set, 640);
    let content = "q 160 0 0 160 220 320 cm /Im1 Do Q\n";
    let (mut s, id) = open(pdf(content, "Helvetica", Some((640, 640, &image)), ""));
    let settings = ChessScanSettings { models_dir: Some(dir.into()), ..Default::default() };
    assert!(models(settings.models_dir.as_deref()).pieces.loaded().is_some(), "the piece model did not load");
    let page = s.chess_page(id, 0, &settings).unwrap();
    let readings = page.readings.as_ref().unwrap();
    assert_eq!(readings.len(), 1, "{:?}", sources(&page));
    assert_eq!(readings[0].source, ReadingSource::Model);
    assert!(readings[0].dpi >= 280.0, "{}", readings[0].dpi);
    let read = readings[0].position().unwrap();
    eprintln!("pieces: {source:?}; read {} (min confidence {})", read.fen, read.min_confidence);
    if matches!(source, pdfcraft_chessvision::pieces::PieceSource::Font { .. }) {
        assert!(read.fen.starts_with(OPENING), "{}", read.fen);
    }
}

mod capture;
mod document;
mod epub;
mod links;
mod office;
mod page_cache;
mod replace;
mod text_ocr;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    /// Corrupt files never panic the scan: arbitrary bytes, and the synthetic page with bytes
    /// overwritten.
    #[test]
    fn corrupt_pages_never_panic(noise in proptest::collection::vec(any::<u8>(), 0..64), at in 0usize..4000, garbage in proptest::collection::vec(any::<u8>(), 0..400)) {
        let mut bytes = pdf(&format!("{}{}", font_board(60.0, 60.0, MERIDA_START), vector_board(60.0, 240.0)), "ChessMerida", Some((2, 2, &[0, 255, 255, 0])), "/Rotate 45");
        let at = at.min(bytes.len());
        bytes.splice(at..(at + noise.len()).min(bytes.len()), noise);
        for b in [bytes, garbage] {
            let mut s = Session::new();
            if let Ok(id) = s.open("x.pdf", None, Arc::new(b), None) {
                let _ = s.chess_scan(id, &[], &ChessScanSettings { dpi: 72.0, ..no_model() }, true);
                if let Some(job) = s.chess_scan_job(id, &[], &no_model(), true) {
                    let _ = job.run_with(Some(&StandIn::new(START)), |_, _| true);
                }
            }
        }
    }

    /// Hostile paths and pixels never panic the adapters.
    #[test]
    fn adapters_never_panic(points in proptest::collection::vec((any::<f64>(), any::<f64>()), 0..8), rgba in proptest::collection::vec(any::<u8>(), 0..64), w in 0u32..9, h in 0u32..9) {
        let sub = Subpath { start: [0.0, 0.0], segments: points.iter().map(|&(x, y)| PathSegment::Line([x, y])).collect(), closed: true };
        let _ = rectangle(&sub);
        let item = PathItem { bbox_user: [0.0; 4], fill: Some(PathColor::Gray(points.first().map_or(0.0, |p| p.0))), stroke: None, subpaths: vec![sub] };
        let _ = filled_boxes(&[item]);
        if let Some(g) = to_gray(&rgba, w, h) {
            let _ = crop(&g, PixelRect { x0: w / 2, y0: 0, x1: u32::MAX, y1: h });
        }
    }
}
