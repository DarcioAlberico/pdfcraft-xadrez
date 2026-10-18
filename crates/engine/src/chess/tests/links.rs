//! Chess ▸ Add lichess links on synthetic pages: a board in a chess diagram font (read without a
//! model) and a board drawn as squares (found, but unread without a model).

use pdfcraft_annot::links::LinkAction;
use pdfcraft_chessvision::coords::ViewRect;
use pdfcraft_render::{PageRenderer, RenderConfig, RenderRequest, RequestKind};

use super::*;
use crate::chess::links::{GAP, LinkSettings, MARGIN, Side, check_label, place_label};

/// The start position's lichess analysis URL, from White's side.
const START_URL: &str = "https://lichess.org/analysis/rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR%20w%20KQkq%20-%200%201";

/// A Merida board at (60, `top`) from the top-left, and a vector board at (360, 400) user space.
fn page(top: f32, extra: &str) -> Vec<u8> {
    let content = format!("{}{}", font_board(60.0, top, MERIDA_START), vector_board(360.0, 400.0));
    pdf(&content, "ChessMerida", None, extra)
}

/// The page's text with each glyph's box (view space), from saved bytes.
fn glyphs(bytes: Arc<Vec<u8>>) -> Vec<(String, [f32; 4])> {
    let mut r = PageRenderer::new(bytes, RenderConfig::default());
    let text = r.render(RenderRequest { page: 0, kind: RequestKind::Text, scale: 1.0, ..Default::default() });
    text.text.map(|t| t.glyphs.iter().map(|g| (g.text.to_string(), g.rect)).collect()).unwrap_or_default()
}

/// The box (view space) of the first run of glyphs spelling `word`.
fn word_box(glyphs: &[(String, [f32; 4])], word: &str) -> Option<[f32; 4]> {
    let text: String = glyphs.iter().map(|g| g.0.as_str()).collect();
    let start = text.find(word)?;
    let first = text[..start].chars().count();
    let n = word.chars().count();
    let boxes: Vec<[f32; 4]> = glyphs.iter().skip(first).take(n).map(|g| g.1).collect();
    Some(boxes.iter().fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]))
}

#[test]
fn a_read_diagram_gets_a_label_and_a_link_below_it_and_the_others_say_why_not() {
    let (mut s, id) = open(page(60.0, ""));
    let plan = s.chess_link_plan(id, &[], &no_model(), &LinkSettings::default()).unwrap();
    assert_eq!(plan.links.len(), 1, "{plan:#?}");
    let link = &plan.links[0];
    assert_eq!((link.page, link.index, link.side, link.url.as_str()), (0, 0, Side::Below, START_URL));
    // Just under the board's bottom (60 + 160 pt from the top), aligned with its left edge.
    assert!((link.label_view.y0 - (220.0 + GAP)).abs() < 1.5 && (link.label_view.x0 - 60.0).abs() < 1.5, "{:?}", link.label_view);
    assert_eq!(plan.skipped.len(), 1, "{plan:#?}");
    assert!(plan.skipped[0].reason.contains("no piece model"), "{:?}", plan.skipped);
    // Planning changed nothing.
    assert!(!s.get(id).unwrap().dirty);

    s.apply(id, plan.edit.clone().unwrap()).unwrap();
    let (s2, id2, saved) = {
        let saved = s.save_bytes(id).unwrap();
        let mut s2 = Session::new();
        let id2 = s2.open("saved.pdf", None, saved.clone(), None).unwrap();
        (s2, id2, saved)
    };
    let links = s2.get(id2).unwrap().links().to_vec();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].action, LinkAction::Uri(START_URL.into()));
    assert!(links[0].rect.iter().zip(link.link_user).all(|(a, b)| (a - b).abs() < 1.0), "{:?} vs {:?}", links[0].rect, link.link_user);
    // The label is drawn where it was planned: below the board, in reading order.
    let text = glyphs(saved);
    let label = word_box(&text, "Analyze on lichess").unwrap_or_else(|| panic!("{text:?}"));
    assert!(label[1] >= 220.0 && label[1] < 220.0 + GAP + 12.0 && (label[0] - 60.0).abs() < 3.0, "{label:?}");

    // Again: the page already links to the position, so nothing more is added.
    let again = s.chess_link_plan(id, &[], &no_model(), &LinkSettings::default()).unwrap();
    assert!(again.links.is_empty() && again.edit.is_none(), "{again:#?}");
    assert!(again.skipped.iter().any(|k| k.reason.contains("already links")), "{again:#?}");
    // One undo step takes label and link away.
    s.undo(id).unwrap();
    assert!(s.get(id).unwrap().links().is_empty());
    assert!(word_box(&glyphs(s.save_bytes(id).unwrap()), "Analyze").is_none());
}

/// A board at the foot of the page has its label to its right; on a turned page the label
/// still reads upright, under the board as displayed.
#[test]
fn the_label_moves_beside_a_board_at_the_foot_and_stays_upright_on_a_turned_page() {
    let (mut s, id) = open(page(800.0 - 160.0 - 4.0, ""));
    let plan = s.chess_link_plan(id, &[], &no_model(), &LinkSettings::default()).unwrap();
    assert_eq!(plan.links.iter().map(|l| l.side).collect::<Vec<_>>(), [Side::Right], "{plan:#?}");

    let (mut s, id) = open(page(60.0, "/Rotate 90"));
    let plan = s.chess_link_plan(id, &[], &no_model(), &LinkSettings::default()).unwrap();
    let link = plan.links.first().unwrap_or_else(|| panic!("{plan:#?}"));
    s.apply(id, plan.edit.clone().unwrap()).unwrap();
    let text = glyphs(s.save_bytes(id).unwrap());
    let label = word_box(&text, "Analyze on lichess").unwrap_or_else(|| panic!("{text:?}"));
    // Upright: wider than tall, and where it was planned in view space.
    assert!(label[2] - label[0] > 3.0 * (label[3] - label[1]), "{label:?}");
    assert!((label[0] - link.label_view.x0).abs() < 3.0 && (label[1] - link.label_view.y0).abs() < 4.0, "{label:?} vs {:?}", link.label_view);
}

#[test]
fn labels_are_placed_below_else_right_else_above_and_never_off_the_page() {
    let page = (600.0, 800.0);
    let label = (80.0, 10.0);
    let board = ViewRect::new(60.0, 60.0, 220.0, 220.0);
    assert_eq!(place_label(board, page, label, &[]).map(|p| p.0), Some(Side::Below));
    // Below is taken: to the right.
    let under = ViewRect::new(0.0, 221.0, 600.0, 260.0);
    assert_eq!(place_label(board, page, label, &[under]).map(|p| p.0), Some(Side::Right));
    // At the foot and against the right edge: above.
    let corner = ViewRect::new(600.0 - MARGIN - 160.0, 800.0 - MARGIN - 160.0, 600.0 - MARGIN, 800.0 - MARGIN);
    assert_eq!(place_label(corner, page, label, &[]).map(|p| p.0), Some(Side::Above));
    // A board filling the page: nowhere.
    assert_eq!(place_label(ViewRect::new(0.0, 0.0, 600.0, 800.0), page, label, &[]), None);
    // A label wider than what is left to the board's right is pulled back onto the page.
    let (_, r) = place_label(ViewRect::new(500.0, 60.0, 590.0, 150.0), page, label, &[]).unwrap();
    assert!(r.x1 <= 600.0 - MARGIN, "{r:?}");
    for bad in [f32::NAN, f32::INFINITY] {
        assert_eq!(place_label(ViewRect { x0: bad, y0: 0.0, x1: 10.0, y1: 10.0 }, page, label, &[]), None);
        assert_eq!(place_label(board, (bad, 800.0), label, &[]), None);
        assert_eq!(place_label(board, page, (bad, 10.0), &[]), None);
    }
    assert_eq!(place_label(board, page, (0.0, 10.0), &[]), None);
}

#[test]
fn labels_the_font_cannot_draw_and_bad_sizes_are_refused() {
    assert_eq!(check_label("  Analisar no lichess "), Ok("Analisar no lichess"));
    assert!(check_label("Análise no lichess").is_ok(), "WinAnsi has the accents");
    for bad in ["", "   ", "♘ lichess", "one\ntwo", &"x".repeat(81)] {
        assert!(check_label(bad).is_err(), "{bad:?}");
    }
    let (mut s, id) = open(page(60.0, ""));
    for size in [f64::NAN, 2.0, 100.0] {
        let e = s.chess_link_plan(id, &[], &no_model(), &LinkSettings { size, ..LinkSettings::default() }).unwrap_err();
        assert!(e.contains("size"), "{e}");
    }
    let e = s.chess_link_plan(id, &[], &no_model(), &LinkSettings { text: "↗ lichess".into(), ..LinkSettings::default() }).unwrap_err();
    assert!(e.contains("cannot draw"), "{e}");
}
