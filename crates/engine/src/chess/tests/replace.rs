//! Chess ▸ Replace diagram on the synthetic pages of `chess::tests`: each kind of diagram is
//! replaced, saved, reopened and scanned again; the new board is checked by its geometry (the
//! vector detector finds it where the old one was), by its pixels (each square's piece and
//! colour, as displayed) and through the stand-in classifier.

use pdfcraft_chess::draw::Drawing;
use pdfcraft_chess::placement::{Color, Placement, Square};
use pdfcraft_chessvision::coords::ViewRect;
use pdfcraft_render::{PageRenderer, RenderConfig, RenderRequest, RequestKind};

use super::*;
use crate::EditError;
use crate::chess::replace::{drawing_ops, placement_matrix};

/// After 1. e4 c5 2. Nf3 d6: not symmetric, so a board drawn turned or mirrored reads wrong.
const SICILIAN: &str = "rnbqkbnr/pp2pppp/3p4/2p5/4P3/5N2/PPPP1PPP/RNBQKB1R";

/// A plain style: no frame, so the eight squares fill the box and can be sampled.
fn plain() -> BoardStyle {
    BoardStyle { border: 0.0, ..BoardStyle::default() }
}

fn replacement(s: &Session, id: DocId, index: usize, fen: &str) -> DiagramReplacement {
    DiagramReplacement {
        drawings: Vec::new(),
        page: 0,
        index,
        generation: s.get(id).unwrap().edit_generation(),
        placement: parse_placement(fen).unwrap(),
        style: plain(),
        pieces: Arc::new(PieceSet::geometric()),
        scan: no_model(),
    }
}

fn reopen(s: &Session, id: DocId) -> (Session, DocId, Arc<Vec<u8>>) {
    let saved = s.save_bytes(id).unwrap();
    let mut s2 = Session::new();
    let id2 = s2.open("saved.pdf", None, saved.clone(), None).unwrap();
    (s2, id2, saved)
}

/// The page rendered at `scale` pixels per point: RGB over white, row by row.
fn render(bytes: Arc<Vec<u8>>, scale: f32) -> (Vec<[u8; 3]>, u32, u32) {
    let mut r = PageRenderer::new(bytes, RenderConfig::default());
    let shot = r.render(RenderRequest { page: 0, kind: RequestKind::Pixels, scale, ..Default::default() });
    assert!(shot.error.is_none(), "{:?}", shot.error);
    let px = shot.rgba.chunks(4).map(|p| [0, 1, 2].map(|i| (u32::from(p[i]) + 255 - u32::from(p[3])).min(255) as u8)).collect();
    (px, shot.width, shot.height)
}

/// What each displayed square of the board in `view` (top-left row first) shows: `W` a white
/// piece, `B` a black one, `.` none. Sampled at the middle of the pieces' base.
fn squares_shown(bytes: Arc<Vec<u8>>, view: ViewRect) -> Vec<String> {
    let scale = 3.0;
    let (px, w, _) = render(bytes, scale);
    let side = view.width().min(view.height());
    let (x0, y0) = (view.x0 + (view.width() - side) / 2.0, view.y0 + (view.height() - side) / 2.0);
    (0..8)
        .map(|row| {
            (0..8)
                .map(|col| {
                    let (x, y) = (x0 + (col as f32 + 0.5) * side / 8.0, y0 + (row as f32 + 0.83) * side / 8.0);
                    let [r, g, b] = px[(y * scale) as usize * w as usize + (x * scale) as usize];
                    match (r, g, b) {
                        _ if r > 215 && g > 215 && b > 215 => 'W',
                        _ if r < 60 && g < 60 && b < 60 => 'B',
                        _ => '.',
                    }
                })
                .collect()
        })
        .collect()
}

/// The same for a placement, seen from White's side.
fn expected(fen: &str) -> Vec<String> {
    let p = parse_placement(fen).unwrap();
    (0..8u8)
        .map(|row| {
            (0..8u8)
                .map(|col| match p.get(Square::new(col, 7 - row).unwrap()) {
                    None => '.',
                    Some(piece) if piece.color == Color::White => 'W',
                    Some(_) => 'B',
                })
                .collect()
        })
        .collect()
}

/// Every object of `a` is in `b`, equal, under the same root: the graph reachable from the root
/// is the same. `b` may hold more objects, none reachable (they would have to be referenced by an
/// object of `a`): an incremental save after undoing past a save leaves the objects the undone
/// edit had added in the file, unreferenced, rather than freeing them.
fn same_graph(a: &Arc<Vec<u8>>, b: &Arc<Vec<u8>>) {
    let (a, b) = (pdfcraft_cos::Document::open(a.clone()).unwrap(), pdfcraft_cos::Document::open(b.clone()).unwrap());
    let numbers = b.object_numbers();
    assert!(a.object_numbers().iter().all(|n| numbers.contains(n)), "{:?} within {numbers:?}", a.object_numbers());
    for n in a.object_numbers() {
        let r = pdfcraft_cos::ObjRef::new(n, a.generation(n));
        assert_eq!(a.get(r), b.get(pdfcraft_cos::ObjRef::new(n, b.generation(n))), "object {n}");
    }
    assert_eq!(a.trailer().get(b"Root"), b.trailer().get(b"Root"));
}

/// An embedded image that is the diagram is deleted and the board drawn in its place. Saved
/// incrementally and reopened, the new board is found where the old one was (as squares drawn
/// with paths), shows the new position square by square, and reads as it through the model path.
#[test]
fn an_image_diagram_is_deleted_and_the_new_board_reads_back() {
    let (mut s, id) = open(three_boards(400));
    let before = s.chess_page(id, 0, &no_model()).unwrap();
    let old = before.diagrams[2].candidate.bbox_user;
    let r = replacement(&s, id, 2, SICILIAN);
    let plan = s.chess_replacement(id, &r).unwrap();
    assert_eq!(plan.deleted_image, Some(0), "{plan:?}");
    assert_eq!((plan.cover, plan.removed_lines.len()), (None, 0));
    s.apply(id, Edit::ReplaceDiagram(Box::new(r))).unwrap();
    assert_eq!(s.get(id).unwrap().can_undo(), Some("Replace diagram"));
    assert!(s.get(id).unwrap().page_images(0).is_empty(), "the old image is gone");

    let original = Arc::new(three_boards(400));
    let (mut s2, id2, saved) = reopen(&s, id);
    assert_eq!(&saved[..original.len()], &original[..], "saved incrementally");
    let page = s2.chess_page(id2, 0, &no_model()).unwrap();
    assert_eq!(page.diagrams.len(), 3, "{:?}", sources(&page));
    let new = &page.diagrams[2];
    assert!(new.sources.contains(&Source::Vector) && !new.sources.contains(&Source::Embedded), "{:?}", new.sources);
    let b = new.candidate.bbox_user;
    for (a, o) in [(b.x0, old.x0), (b.y0, old.y0), (b.x1, old.x1), (b.y1, old.y1)] {
        assert!((a - o).abs() < 2.0, "the new board {b:?} is not where the old one was {old:?}");
    }
    let view = page.geometry.unwrap().user_to_view(b).unwrap();
    assert_eq!(squares_shown(saved.clone(), view), expected(SICILIAN));
    // The model path reads it (a stand-in model: no model ships).
    let scan = s2.chess_scan_job(id2, &[], &no_model(), true).unwrap().run_with(Some(&StandIn::new(SICILIAN)), |_, _| true);
    let read = scan.pages[0].readings.as_ref().unwrap()[2].position().unwrap().clone();
    assert!(read.fen.starts_with(SICILIAN), "{}", read.fen);
    // The other two diagrams are untouched.
    assert_eq!(sources(&page)[..2], sources(&before)[..2]);
}

/// A diagram set in a chess font loses its text (so no later scan reads the old position from
/// it) and is covered, then drawn over. Without the text removal the font reading would win.
#[test]
fn a_font_diagram_loses_its_text_so_the_old_position_is_not_read_again() {
    let (mut s, id) = open(three_boards(400));
    let r = replacement(&s, id, 0, SICILIAN);
    let plan = s.chess_replacement(id, &r).unwrap();
    assert_eq!(plan.removed_lines.len(), 8, "{plan:?}");
    assert!(plan.cover.is_some() && plan.deleted_image.is_none());
    s.apply(id, Edit::ReplaceDiagram(Box::new(r))).unwrap();
    assert!(s.get(id).unwrap().text_lines(0).iter().all(|l| l.base_font != "ChessMerida"), "the diagram font's lines are gone");
    let (mut s2, id2, saved) = reopen(&s, id);
    let page = s2.chess_page(id2, 0, &no_model()).unwrap();
    let first = &page.diagrams[0];
    assert!(!first.sources.contains(&Source::Font), "{:?}", first.sources);
    assert!(first.sources.contains(&Source::Vector), "{:?}", first.sources);
    assert_eq!(page.readings.as_ref().unwrap()[0].outcome, Err(Unreadable::NoModel), "nothing reads the old position");
    let view = page.geometry.unwrap().user_to_view(first.candidate.bbox_user).unwrap();
    assert_eq!(squares_shown(saved, view), expected(SICILIAN));
}

/// A board drawn as squares is covered, white, and drawn over; its squares stay underneath, and
/// the plan says so.
#[test]
fn a_vector_diagram_is_covered_and_drawn_over() {
    let (mut s, id) = open(three_boards(400));
    let r = replacement(&s, id, 1, SICILIAN);
    let plan = s.chess_replacement(id, &r).unwrap();
    let cover = plan.cover.unwrap();
    let b = plan.bbox_user;
    assert!(cover[0] < f64::from(b.x0) && cover[2] > f64::from(b.x1) && cover[2] - cover[0] < f64::from(b.width()) + 7.0, "{cover:?} {b:?}");
    assert!(plan.notes.iter().any(|n| n.contains("under an opaque white cover")), "{:?}", plan.notes);
    s.apply(id, Edit::ReplaceDiagram(Box::new(r))).unwrap();
    let (mut s2, id2, saved) = reopen(&s, id);
    let page = s2.chess_page(id2, 0, &no_model()).unwrap();
    assert_eq!(page.diagrams.len(), 3);
    let view = page.geometry.unwrap().user_to_view(page.diagrams[1].candidate.bbox_user).unwrap();
    assert_eq!(squares_shown(saved, view), expected(SICILIAN));
}

/// Plan HC7: drawings asked for go onto the page over the board: a red ring around d5 (an
/// empty square of the Sicilian) shows red where its line runs, and the square's middle stays
/// the square.
#[test]
fn drawings_go_onto_the_page_over_the_board() {
    let (mut s, id) = open(three_boards(400));
    let page = s.chess_page(id, 0, &no_model()).unwrap();
    let view = page.geometry.unwrap().user_to_view(page.diagrams[1].candidate.bbox_user).unwrap();
    let mut r = replacement(&s, id, 1, SICILIAN);
    let d5 = Square::parse("d5").unwrap();
    r.drawings = vec![pdfcraft_chess::game::Drawing { from: d5, to: d5, color: pdfcraft_chess::game::DrawColor::Red }];
    s.apply(id, Edit::ReplaceDiagram(Box::new(r))).unwrap();
    let (_, _, saved) = reopen(&s, id);
    let scale = 3.0;
    let (px, w, _) = render(saved, scale);
    let side = view.width().min(view.height());
    let (x0, y0) = (view.x0 + (view.width() - side) / 2.0, view.y0 + (view.height() - side) / 2.0);
    let cell = side / 8.0;
    // d5 is the fourth column and the fourth row from the top.
    let (cx, cy) = (x0 + 3.5 * cell, y0 + 3.5 * cell);
    let at = |x: f32, y: f32| px[(y * scale) as usize * w as usize + (x * scale) as usize];
    let [r, g, b] = at(cx + 0.435 * cell, cy);
    assert!(r > 130 && g < 90 && b < 90, "the ring: {:?}", [r, g, b]);
    let [r, g, b] = at(cx, cy);
    assert!(!(r > 130 && g < 90 && b < 90), "inside the ring the square shows: {:?}", [r, g, b]);
}

/// On a page turned by `/Rotate`, the board shows upright as the page is displayed: the eighth
/// rank along the displayed top.
#[test]
fn a_turned_page_shows_the_board_upright() {
    for turn in [90, 180, 270] {
        let (mut s, id) = open(pdf(&vector_board(200.0, 300.0), "Helvetica", None, &format!("/Rotate {turn}")));
        let page = s.chess_page(id, 0, &no_model()).unwrap();
        assert_eq!(page.diagrams.len(), 1, "/Rotate {turn}");
        s.apply(id, Edit::ReplaceDiagram(Box::new(replacement(&s, id, 0, SICILIAN)))).unwrap();
        let (_, _, saved) = reopen(&s, id);
        let view = page.geometry.unwrap().user_to_view(page.diagrams[0].candidate.bbox_user).unwrap();
        assert_eq!(squares_shown(saved, view), expected(SICILIAN), "/Rotate {turn}");
    }
}

/// Undo takes the whole replacement back in one step; saved again after that, the file's object
/// graph is the original's, even when the replacement had been saved in between.
#[test]
fn undo_restores_the_original_object_graph() {
    let original = Arc::new(three_boards(400));
    let (mut s, id) = open(three_boards(400));
    s.apply(id, Edit::ReplaceDiagram(Box::new(replacement(&s, id, 2, SICILIAN)))).unwrap();
    s.undo(id).unwrap();
    assert_eq!(s.save_bytes(id).unwrap(), original, "nothing left to save");
    // Saved with the replacement, then undone and saved again: a new revision back to the original.
    s.redo(id).unwrap();
    let replaced = s.save_bytes(id).unwrap();
    s.mark_saved(id, replaced.clone(), None).unwrap();
    s.undo(id).unwrap();
    let reverted = s.save_bytes(id).unwrap();
    assert_eq!(&reverted[..replaced.len()], &replaced[..], "appended to the saved file");
    same_graph(&original, &reverted);
    let (mut s2, id2) = open(reverted.to_vec());
    assert_eq!(sources(&s2.chess_page(id2, 0, &no_model()).unwrap()), sources(&s.chess_page(id, 0, &no_model()).unwrap()));
}

/// Planning changes nothing: no undo step, no new generation, the same bytes.
#[test]
fn a_plan_changes_nothing() {
    let (mut s, id) = open(three_boards(400));
    let r = replacement(&s, id, 2, SICILIAN);
    let bytes = s.get(id).unwrap().bytes.clone();
    let plan = s.chess_replacement(id, &r).unwrap();
    assert!(matches!(&plan.edit, Edit::Batch { label, edits } if label == "Replace diagram" && edits.len() == 2));
    let doc = s.get(id).unwrap();
    assert_eq!((doc.edit_generation(), doc.dirty, doc.can_undo()), (r.generation, false, None));
    assert!(Arc::ptr_eq(&doc.bytes, &bytes));
}

/// A diagram numbered before an edit, a number past the page's diagrams, a page that does not
/// exist and a replacement nested in a batch are refused, and change nothing.
#[test]
fn stale_or_missing_diagrams_are_refused() {
    let (mut s, id) = open(three_boards(400));
    let stale = replacement(&s, id, 2, SICILIAN);
    s.apply(id, Edit::SetInfo { key: "Title".into(), value: "Games".into() }).unwrap();
    let refused = |s: &mut Session, r: DiagramReplacement, why: &str| {
        let generation = s.get(id).unwrap().edit_generation();
        let err = s.apply(id, Edit::ReplaceDiagram(Box::new(r))).unwrap_err();
        assert!(err.to_string().contains(why), "{err}");
        assert_eq!(s.get(id).unwrap().edit_generation(), generation);
        assert_eq!(s.get(id).unwrap().can_undo(), Some("Change Title"));
    };
    refused(&mut s, stale, "changed since");
    let past = replacement(&s, id, 3, SICILIAN);
    refused(&mut s, past, "has 3 diagrams; there is no diagram 4");
    let no_page = DiagramReplacement { page: 1, ..replacement(&s, id, 0, SICILIAN) };
    refused(&mut s, no_page, "page 2 does not exist");
    let nested = Edit::Batch { label: "x".into(), edits: vec![Edit::ReplaceDiagram(Box::new(replacement(&s, id, 0, SICILIAN)))] };
    assert!(s.apply(id, nested).unwrap_err().to_string().contains("applied by itself"));
}

/// A document whose permissions do not allow changes refuses the replacement before scanning.
#[test]
fn a_document_that_may_not_be_changed_is_refused() {
    let mut doc = pdfcraft_cos::Document::open(Arc::new(three_boards(400))).unwrap();
    doc.set_encryption(&pdfcraft_cos::NewEncryption {
        algorithm: pdfcraft_cos::Algorithm::Aes256,
        user_password: "",
        owner_password: "owner",
        permissions: 0b1_0000, // copying only
        encrypt_metadata: true,
        seed: [7; 32],
    })
    .unwrap();
    let bytes = pdfcraft_cos::write_full(&doc, &Default::default()).unwrap();
    let (mut s, id) = open(bytes);
    let r = replacement(&s, id, 0, SICILIAN);
    assert_eq!(s.chess_replacement(id, &r).unwrap_err(), EditError::NotPermitted("changes to the document"));
    assert_eq!(s.apply(id, Edit::ReplaceDiagram(Box::new(r))), Err(EditError::NotPermitted("changes to the document")));
    assert!(!s.get(id).unwrap().dirty);
}

/// The drawing's operators pass the fragment check, one fill or stroke per layer and one text
/// object per coordinate label, inside one `q … Q`.
#[test]
fn a_drawing_becomes_checked_operators() {
    let style = BoardStyle { coordinates: true, ..BoardStyle::default() };
    let drawing = board(&Placement::standard(), &PieceSet::geometric(), &style);
    let ops = drawing_ops(&drawing, [1.0, 0.0, 0.0, -1.0, 0.0, 800.0]);
    let parsed = pdfcraft_content::parse(&ops);
    let count = |op: &str| parsed.ops.iter().filter(|o| o.is(op)).count();
    assert!(pdfcraft_edit::check_operators(&ops).is_ok());
    // Frame, light squares, dark squares and 32 pieces' fill; 32 outlines.
    assert_eq!((count("f"), count("S"), count("BT"), count("q"), count("Q")), (3 + 32, 32, 16, 1, 1));
    assert!(!String::from_utf8_lossy(&ops).contains("NaN"));
}

/// The board's top-left corner lands at the box's top-left as displayed, and its bottom-right at
/// the bottom-right, for every `/Rotate` and with `/UserUnit`; a non-square box centres it.
#[test]
fn the_placement_matrix_puts_the_drawing_upright_in_its_box() {
    let crop = PageRect::new(10.0, 20.0, 610.0, 820.0);
    let bbox = PageRect::new(100.0, 200.0, 300.0, 360.0);
    for rotation in [Rotation::R0, Rotation::R90, Rotation::R180, Rotation::R270] {
        for unit in [1.0, 2.0] {
            let g = PageGeometry::new(crop, rotation).unwrap().with_user_unit(unit).unwrap();
            let m = placement_matrix(&g, bbox, 100.0).unwrap();
            let view = g.user_to_view(bbox).unwrap();
            let side = view.width().min(view.height());
            let at = |x: f64, y: f64| g.user_to_view_point((m[0] * x + m[2] * y + m[4]) as f32, (m[1] * x + m[3] * y + m[5]) as f32).unwrap();
            let (cx, cy) = ((view.x0 + view.x1) / 2.0, (view.y0 + view.y1) / 2.0);
            for ((x, y), want) in [((0.0, 0.0), [cx - side / 2.0, cy - side / 2.0]), ((100.0, 100.0), [cx + side / 2.0, cy + side / 2.0])] {
                let got = at(x, y);
                assert!((got[0] - want[0]).abs() < 0.01 && (got[1] - want[1]).abs() < 0.01, "{rotation:?} ×{unit}: {got:?} vs {want:?}");
            }
        }
    }
    let g = PageGeometry::new(crop, Rotation::R0).unwrap();
    assert!(placement_matrix(&g, bbox, 0.0).is_none());
    assert!(placement_matrix(&g, bbox, f32::NAN).is_none());
    assert!(placement_matrix(&g, PageRect::new(5.0, 5.0, 5.0, 50.0), 10.0).is_none());
    assert!(placement_matrix(&g, PageRect::new(f32::NAN, 5.0, 5.0, 50.0), 10.0).is_none());
}

/// The image is only deleted when it is the diagram: one reaching well past the board (a
/// caption under it) is covered instead, and the plan says why.
#[test]
fn an_image_holding_more_than_the_board_is_covered_not_deleted() {
    // The image is 160 pt wide and 192 pt tall: the board on top, a caption's space below it
    // (32 pt, a fifth of the board, past the slack). Its aspect, 0.83, is one the detector takes.
    let px = 400u32;
    let board_px = rasterize(OPENING, &PieceSet::geometric(), px);
    let mut image = board_px.clone();
    image.extend(std::iter::repeat_n(255u8, (px * px / 5) as usize));
    let content = "q 160 0 0 192 220 300 cm /Im1 Do Q\n";
    let (mut s, id) = open(pdf(content, "Helvetica", Some((px, px + px / 5, &image)), ""));
    let page = s.chess_page(id, 0, &no_model()).unwrap();
    assert_eq!(page.diagrams.len(), 1, "{:?}", sources(&page));
    let plan = s.chess_replacement(id, &replacement(&s, id, 0, SICILIAN)).unwrap();
    assert_eq!(plan.deleted_image, None, "{plan:?}");
    assert!(plan.cover.is_some());
    assert!(plan.notes.iter().any(|n| n.contains("covered, not deleted")), "{:?}", plan.notes);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Any placement in any style, on any box and turn, never panics, and what is drawn always
    /// passes the fragment check.
    #[test]
    fn arbitrary_placements_and_styles_never_panic(
        cells in proptest::collection::vec(0usize..13, 64),
        square in any::<f32>(), border in any::<f32>(), coordinates in any::<bool>(), flipped in any::<bool>(), hatched in any::<bool>(),
        colours in proptest::collection::vec(any::<u8>(), 9),
        bbox in proptest::array::uniform4(-1e6f32..1e6), turn in 0i64..4, size in any::<f32>(),
    ) {
        let pieces: [Option<pdfcraft_chess::placement::Piece>; 64] = std::array::from_fn(|i| pdfcraft_chess::recognize::piece_of(cells[i] as u8));
        let placement = Placement::from_reading_order(&pieces);
        let rgb = |i: usize| Rgb(colours[i], colours[i + 1], colours[i + 2]);
        let style = BoardStyle { square, light: rgb(0), dark: rgb(3), border, border_color: rgb(6), coordinates, flipped, hatched };
        let drawing: Drawing = board(&placement, &PieceSet::geometric(), &style);
        let ops = drawing_ops(&drawing, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
        prop_assert!(pdfcraft_edit::check_operators(&ops).is_ok(), "{}", String::from_utf8_lossy(&ops));
        let g = PageGeometry::new(PageRect::new(0.0, 0.0, 600.0, 800.0), Rotation::from_degrees(turn * 90)).unwrap();
        if let Some(m) = placement_matrix(&g, PageRect::new(bbox[0], bbox[1], bbox[2], bbox[3]), size) {
            prop_assert!(m.iter().all(|v| v.is_finite()));
            prop_assert!(pdfcraft_edit::check_operators(&drawing_ops(&drawing, m)).is_ok());
        }
    }
}
