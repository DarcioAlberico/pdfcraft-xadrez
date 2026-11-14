//! A page's reading as it is written (plan A9-N5a): what a scan found ([`ChessPage`]) and what
//! chess OCR read of a page alone ([`PageAlone`]) read back from their JSON as they were, every
//! kind of reading included (a font's, a model's, a repaired one, an illegal one, a board no model
//! read, a mate found, a page cut short), and the JSON names they are written with.

use pdfcraft_chessvision::board::MateFinding;
use pdfcraft_chessvision::coords::{PageGeometry, PageRect, Rotation};

use super::text_ocr::{Templates, scan};
use super::*;
use crate::chess::text_ocr::{PageAlone, finish_page, read_page, read_page_alone};

/// `page` written as JSON and read back.
fn again(page: &ChessPage) -> ChessPage {
    let json = serde_json::to_vec(page).unwrap();
    serde_json::from_slice(&json).unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&json)))
}

#[test]
fn a_pages_scan_reads_back_as_it_was_written() {
    let mut pages: Vec<ChessPage> = Vec::new();
    // A font diagram read exactly, a vector board and an image no model read.
    let (mut s, id) = open(three_boards(400));
    pages.push((*s.chess_page(id, 0, &no_model()).unwrap()).clone());
    // Read by a model: legal, repaired (no kings) and illegal (sixteen queens) readings.
    let job = |model: &StandIn| s.chess_scan_job(id, &[], &no_model(), true).unwrap().run_with(Some(model), |_, _| true).pages[0].clone();
    for model in [StandIn::new(OPENING), StandIn::new("8/8/8/3p4/8/8/8/8"), StandIn::both_ways("QQQQQQQQ/QQQQQQQQ/8/8/8/8/8/8")] {
        pages.push((*job(&model)).clone());
    }
    // Cut short, with a note.
    pages.push((*s.chess_page(id, 0, &ChessScanSettings { max_diagrams: 2, ..no_model() }).unwrap()).clone());
    // A mate the caption asks, found; and both sides mating, noted.
    for (placement, caption) in [("8/8/8/8/4R3/6k1/8/4K2R", "Mate in 2"), ("3r2k1/5ppp/8/8/8/8/5PPP/3R2K1", "#1")] {
        let rows = merida(placement);
        let rows: [&str; 8] = std::array::from_fn(|i| rows[i].as_str());
        let (mut s, id) = open(board_with_words(rows, caption, ""));
        pages.push((*s.chess_page(id, 0, &no_model()).unwrap()).clone());
    }
    // Every kind the test means to cover is there.
    let readings: Vec<&BoardReading> = pages.iter().flat_map(|p| p.readings.iter().flatten()).collect();
    let positions: Vec<&ReadPosition> = readings.iter().filter_map(|r| r.position()).collect();
    assert!(readings.iter().any(|r| r.outcome == Err(Unreadable::NoModel)));
    assert!(positions.iter().any(|p| p.repaired()) && positions.iter().any(|p| !p.legality.is_legal_for_some_side()));
    assert!(positions.iter().any(|p| matches!(&p.mate, Some(MateFinding::Solved(r)) if !r.keys.is_empty())));
    assert!(positions.iter().any(|p| p.mate == Some(MateFinding::BothSides { n: 1 })));
    assert!(positions.iter().any(|p| p.orientation.is_some()) && positions.iter().any(|p| !p.uncertain.is_empty() || !p.repairs.is_empty()));
    assert!(pages.iter().any(|p| p.truncated) && pages.iter().any(|p| !p.notes.is_empty()));
    for page in &pages {
        assert_eq!(&again(page), page);
    }
    // A page that could not be scanned, and one turned and scaled.
    let failed = ChessPage { error: Some("the page could not be read".into()), dpi: 0.0, ..ChessPage::clone(&pages[0]) };
    assert_eq!(again(&failed), failed);
    let turned = PageGeometry::new(PageRect::new(10.0, 20.0, 610.0, 820.0), Rotation::R270).and_then(|g| g.with_user_unit(2.5));
    let turned = ChessPage { geometry: turned, ..failed };
    assert_eq!(again(&turned), turned);
}

/// The names a page's reading is written with: a later build reads what this one wrote (plan
/// A9-N5a). A field renamed here is a field a kept reading no longer has.
#[test]
fn a_pages_scan_is_written_with_these_names() {
    let (mut s, id) = open(three_boards(400));
    let page = s.chess_page(id, 0, &ChessScanSettings { max_diagrams: 1, ..no_model() }).unwrap();
    let json: serde_json::Value = serde_json::to_value(&*page).unwrap();
    // In the order written (a `Value` sorts them).
    let keys = |v: &serde_json::Value| {
        let mut k: Vec<String> = v.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
        k.sort();
        k
    };
    let sorted = |names: &[&str]| {
        let mut k: Vec<String> = names.iter().map(|n| (*n).to_owned()).collect();
        k.sort();
        k
    };
    assert_eq!(keys(&json), sorted(&["page", "geometry", "dpi", "diagrams", "readings", "truncated", "error", "notes"]));
    assert_eq!(keys(&json["geometry"]), sorted(&["crop_box", "rotation", "user_unit"]));
    assert_eq!(json["geometry"]["crop_box"], serde_json::json!({"x0": 0.0, "y0": 0.0, "x1": 600.0, "y1": 800.0}));
    assert_eq!(keys(&json["diagrams"][0]), sorted(&["candidate", "sources"]));
    assert_eq!(keys(&json["diagrams"][0]["candidate"]), sorted(&["bbox_user", "source", "score", "dpi_hint", "placement", "corners"]));
    assert_eq!(json["diagrams"][0]["candidate"]["source"], "Font");
    assert_eq!(json["diagrams"][0]["candidate"]["placement"], START);
    let reading = &json["readings"][0];
    assert_eq!(keys(reading), sorted(&["index", "bbox_user", "source", "dpi", "outcome"]));
    let position = &reading["outcome"]["Ok"];
    assert_eq!(
        keys(position),
        sorted(&["placement", "fen", "legality", "side", "orientation", "min_confidence", "uncertain", "repairs", "accepted", "mate"])
    );
    assert_eq!(position["legality"], "Legal");
    assert_eq!(position["side"], serde_json::json!({"color": "White", "source": "Default", "conflicting": false}));
    let note = serde_json::to_value(ScanNote::ImagesCapped { looked: 1, total: 2 }).unwrap();
    assert_eq!(note, serde_json::json!({"ImagesCapped": {"looked": 1, "total": 2}}));
}

/// A stored geometry that breaks its rule (a turn that is no quarter, a box without area, a unit
/// that is not positive) is refused, not taken.
#[test]
fn a_stored_geometry_that_breaks_its_rule_is_refused() {
    let geometry = |rotation: u32, x1: f32, unit: f32| serde_json::json!({"crop_box": {"x0": 0.0, "y0": 0.0, "x1": x1, "y1": 800.0}, "rotation": rotation, "user_unit": unit});
    assert!(serde_json::from_value::<PageGeometry>(geometry(90, 600.0, 1.0)).is_ok());
    for bad in [geometry(45, 600.0, 1.0), geometry(0, 0.0, 1.0), geometry(0, 600.0, 0.0), geometry(0, 600.0, -2.0)] {
        assert!(serde_json::from_value::<PageGeometry>(bad.clone()).is_err(), "{bad}");
    }
}

/// What chess OCR reads of a page alone reads back as it was written, and the cipher and the
/// repair over it give what reading the page whole gives: the split is the reading's, not a second
/// reading.
#[test]
fn a_pages_own_reading_reads_back_and_finishes_as_the_whole_reading() {
    let templates = Templates::learn("1.e4526Nf3c", 14.0);
    let dpi = crate::chess::text_ocr::OCR_DPI;
    let (w, h, data) = scan(&[("1.e4 e5 2.Nf3 Nc6", 14.0, 300.0)], "", dpi);
    let image = pdfcraft_chessvision::raster::GrayImage::new(w, h, data).unwrap();
    let alone = read_page_alone(&image, dpi, &[], &templates, None).unwrap();
    assert!(!alone.lines.is_empty());
    let json = serde_json::to_vec(&alone).unwrap();
    let back: PageAlone = serde_json::from_slice(&json).unwrap();
    assert_eq!(back, alone);
    let lang = pdfcraft_chess::san::Lang::En;
    let (mut book, mut book_whole) = (Default::default(), Default::default());
    let finished = finish_page(back, lang, None, &mut book, 0);
    let whole = read_page(&image, dpi, &[], &templates, None, lang, None, &mut book_whole, 0).unwrap();
    assert_eq!(format!("{finished:?}"), format!("{whole:?}"));
    assert_eq!(format!("{book:?}"), format!("{book_whole:?}"));
}
