//! Plumbing tests for the board reader. Boards are drawn with `pdfcraft_chess::draw` and the
//! geometric piece set, rasterized here, and classified by a nearest-template model trained in
//! the test on one drawn board: the aim is the plumbing (cut, order, orientation, batching,
//! geometry), not accuracy.

use std::cell::RefCell;

use pdfcraft_chess::draw::{BoardStyle, Paint, PieceSet, Rgb, Segment, Subpath, board};
use pdfcraft_chess::fen::{format_placement, parse_placement};
use pdfcraft_chess::recognize::{CLASS_COUNT, class_of};
use proptest::prelude::*;

use super::*;
use crate::coords::{PageGeometry, PageRect, Rotation, pixels_to_view, px_to_pt};
use crate::detect::Source;

const DPI: f32 = 144.0;
/// Side of a drawn board, in raster pixels.
const SIDE: f32 = 200.0;
/// Every piece on a light and on a dark square, and empty squares of both colours.
const TRAINING: &str = "KQRBNPkq/KQRBNPkq/rbnp4/rbnp4/8/8/8/8";
const OPENING: &str = "r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R";
const ENDGAME: &str = "8/5pk1/6p1/8/3P4/2K5/5P2/8";
const MIDDLE: &str = "r4rk1/pp3ppp/2n1b3/3q4/3P4/2N2N2/PP3PPP/R2Q1RK1";

/// `(x, y)` in drawing units to raster pixels: `x' = a x + b y + e`, `y' = c x + d y + f`.
#[derive(Clone, Copy)]
struct Affine {
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
    f: f32,
}

impl Affine {
    fn apply(&self, (x, y): (f32, f32)) -> (f32, f32) {
        (self.a * x + self.b * y + self.e, self.c * x + self.d * y + self.f)
    }

    fn scale(&self) -> f32 {
        (self.a * self.d - self.b * self.c).abs().sqrt()
    }
}

fn style() -> BoardStyle {
    BoardStyle { square: 10.0, border: 0.0, coordinates: false, ..BoardStyle::default() }
}

/// A board `SIDE` pixels wide with its top-left corner at `(x, y)`, turned by `degrees`
/// clockwise about its centre.
fn placed(x: f32, y: f32, degrees: f32) -> Affine {
    let s = SIDE / (8.0 * style().square);
    let (sin, cos) = degrees.to_radians().sin_cos();
    let (cx, cy) = (x + SIDE / 2.0, y + SIDE / 2.0);
    let half = 4.0 * style().square;
    // Centre the drawing on the origin, scale, turn, then move to the centre.
    Affine { a: s * cos, b: -s * sin, c: s * sin, d: s * cos, e: cx - s * half * (cos - sin), f: cy - s * half * (sin + cos) }
}

fn luma(Rgb(r, g, b): Rgb) -> u8 {
    ((299 * u32::from(r) + 587 * u32::from(g) + 114 * u32::from(b)) / 1000) as u8
}

fn flatten(path: &Subpath, t: &Affine) -> Vec<(f32, f32)> {
    let mut points = vec![t.apply(path.start)];
    let mut at = path.start;
    for segment in &path.segments {
        match *segment {
            Segment::Line(x, y) => at = (x, y),
            Segment::Cubic(x1, y1, x2, y2, x, y) => {
                for k in 1..12 {
                    let s = k as f32 / 12.0;
                    let m = 1.0 - s;
                    let p = |a: f32, b: f32, c: f32, d: f32| m * m * m * a + 3.0 * m * m * s * b + 3.0 * m * s * s * c + s * s * s * d;
                    points.push(t.apply((p(at.0, x1, x2, x), p(at.1, y1, y2, y))));
                }
                at = (x, y);
            }
        }
        points.push(t.apply(at));
    }
    points
}

fn winding(polygon: &[(f32, f32)], (px, py): (f32, f32)) -> i32 {
    let mut w = 0;
    for (i, &(x0, y0)) in polygon.iter().enumerate() {
        let (x1, y1) = polygon[(i + 1) % polygon.len()];
        let side = (x1 - x0) * (py - y0) - (px - x0) * (y1 - y0);
        if y0 <= py && y1 > py && side > 0.0 {
            w += 1;
        } else if y0 > py && y1 <= py && side < 0.0 {
            w -= 1;
        }
    }
    w
}

fn near_outline(polygon: &[(f32, f32)], (px, py): (f32, f32), half: f32) -> bool {
    (0..polygon.len()).any(|i| {
        let ((x0, y0), (x1, y1)) = (polygon[i], polygon[(i + 1) % polygon.len()]);
        let (dx, dy) = (x1 - x0, y1 - y0);
        let t = (((px - x0) * dx + (py - y0) * dy) / (dx * dx + dy * dy).max(1e-9)).clamp(0.0, 1.0);
        (px - x0 - t * dx).hypot(py - y0 - t * dy) <= half
    })
}

/// A white page raster.
struct Page {
    width: usize,
    height: usize,
    data: Vec<u8>,
}

impl Page {
    fn new(width: usize, height: usize) -> Page {
        Page { width, height, data: vec![255; width * height] }
    }

    /// Paint `fen` with the geometric set through `t`: a minimal rasterizer, one sample per pixel
    /// centre, nonzero or even-odd fills and strokes as a distance to the outline.
    fn draw(&mut self, fen: &str, t: Affine) {
        let drawing = board(&parse_placement(fen).unwrap(), &PieceSet::geometric(), &style());
        for layer in &drawing.layers {
            let polygons: Vec<Vec<(f32, f32)>> = layer.subpaths.iter().map(|p| flatten(p, &t)).collect();
            let (color, half) = match layer.paint {
                Paint::Fill { color, .. } => (color, 0.0),
                Paint::Stroke { color, width } => (color, width * t.scale() / 2.0),
            };
            let all = polygons.iter().flatten();
            let x0 = (all.clone().map(|p| p.0).fold(f32::INFINITY, f32::min) - half - 1.0).max(0.0) as usize;
            let x1 = (all.clone().map(|p| p.0).fold(0.0, f32::max) + half + 1.0).min(self.width as f32) as usize;
            let y0 = (all.clone().map(|p| p.1).fold(f32::INFINITY, f32::min) - half - 1.0).max(0.0) as usize;
            let y1 = (all.map(|p| p.1).fold(0.0, f32::max) + half + 1.0).min(self.height as f32) as usize;
            for y in y0..y1 {
                for x in x0..x1 {
                    let p = (x as f32 + 0.5, y as f32 + 0.5);
                    let hit = match layer.paint {
                        Paint::Fill { even_odd, .. } => {
                            let w: i32 = polygons.iter().map(|poly| winding(poly, p)).sum();
                            if even_odd { w % 2 != 0 } else { w != 0 }
                        }
                        Paint::Stroke { .. } => polygons.iter().any(|poly| near_outline(poly, p, half)),
                    };
                    if hit {
                        self.data[y * self.width + x] = luma(color);
                    }
                }
            }
        }
    }

    fn image(&self) -> GrayImage {
        GrayImage::new(self.width as u32, self.height as u32, self.data.clone()).unwrap()
    }

    /// The page's geometry: its raster is the displayed page at `DPI`.
    fn geometry(&self, rotation: Rotation) -> PageGeometry {
        let (w, h) = (px_to_pt(self.width as f32, DPI).unwrap(), px_to_pt(self.height as f32, DPI).unwrap());
        let (w, h) = if rotation.swaps_axes() { (h, w) } else { (w, h) };
        PageGeometry::new(PageRect::new(0.0, 0.0, w, h), rotation).unwrap()
    }
}

fn candidate(bbox_user: PageRect, corners: Option<[(f64, f64); 4]>, placement: Option<&str>) -> DiagramCandidate {
    DiagramCandidate {
        bbox_user,
        source: if placement.is_some() { Source::Font } else { Source::Embedded },
        score: 1.0,
        dpi_hint: DPI,
        placement: placement.map(|fen| parse_placement(fen).unwrap()),
        corners,
    }
}

/// The candidate a detector gives for a board drawn by `placed(x, y, 0)`: its box only.
fn box_candidate(geometry: &PageGeometry, x: f32, y: f32) -> DiagramCandidate {
    let rect = PixelRect { x0: x as u32, y0: y as u32, x1: (x + SIDE) as u32, y1: (y + SIDE) as u32 };
    candidate(geometry.view_to_user(pixels_to_view(rect, DPI).unwrap()).unwrap(), None, None)
}

/// The candidate for a board drawn through `t`: its four corners as displayed, and their box.
fn quad_candidate(geometry: &PageGeometry, t: Affine) -> DiagramCandidate {
    // A detector's corners are the board's outermost pixels, and the warp samples pixel `i` at
    // coordinate `i`, where this rasterizer paints it at `i + 0.5`: half a pixel in, then back.
    let (inset, far) = (0.5 / t.scale(), 8.0 * style().square - 0.5 / t.scale());
    let user = |p: (f32, f32)| {
        let (x, y) = t.apply(p);
        let [ux, uy] = geometry.view_to_user_point(px_to_pt(x - 0.5, DPI).unwrap(), px_to_pt(y - 0.5, DPI).unwrap()).unwrap();
        (f64::from(ux), f64::from(uy))
    };
    let corners = [user((inset, inset)), user((far, inset)), user((far, far)), user((inset, far))];
    let xs = corners.map(|c| c.0 as f32);
    let ys = corners.map(|c| c.1 as f32);
    let bbox = PageRect::new(
        xs.into_iter().fold(f32::INFINITY, f32::min),
        ys.into_iter().fold(f32::INFINITY, f32::min),
        xs.into_iter().fold(f32::NEG_INFINITY, f32::max),
        ys.into_iter().fold(f32::NEG_INFINITY, f32::max),
    );
    candidate(bbox, Some(corners), None)
}

/// A classifier trained in the test: the squares of one drawn board are its templates, and a
/// square's class probabilities are a softmax of its distance to each class's nearest template.
/// It records the size of every call.
struct Nearest {
    templates: Vec<(u8, Vec<f32>)>,
    calls: RefCell<Vec<usize>>,
}

impl Nearest {
    fn trained() -> Nearest {
        let mut page = Page::new(240, 240);
        page.draw(TRAINING, placed(20.0, 20.0, 0.0));
        let geometry = page.geometry(Rotation::R0);
        let board = board_image(&page.image(), &geometry, DPI, &box_candidate(&geometry, 20.0, 20.0)).unwrap();
        let cells = parse_placement(TRAINING).unwrap().reading_order();
        let templates = cells.iter().zip(squares(&board).unwrap().chunks(SQUARE_VALUES)).map(|(c, s)| (class_of(*c), s.to_vec())).collect();
        Nearest { templates, calls: RefCell::new(Vec::new()) }
    }
}

impl SquareClassifier for Nearest {
    fn classify(&self, squares: &[f32], count: usize) -> Result<Vec<f32>, ModelError> {
        self.calls.borrow_mut().push(count);
        assert_eq!(squares.len(), count * SQUARE_VALUES);
        let mut out = Vec::with_capacity(count * CLASS_COUNT);
        for square in squares.chunks(SQUARE_VALUES) {
            let mut best = [f32::INFINITY; CLASS_COUNT];
            for (class, template) in &self.templates {
                let d = square.iter().zip(template).map(|(a, b)| (a - b).abs()).sum::<f32>() / SQUARE_VALUES as f32;
                best[usize::from(*class)] = best[usize::from(*class)].min(d);
            }
            let exps: Vec<f32> = best.iter().map(|d| (-150.0 * d).exp()).collect();
            let sum: f32 = exps.iter().sum();
            out.extend(exps.iter().map(|e| e / sum));
        }
        Ok(out)
    }
}

/// A classifier that ignores the pixels: `upright` on the 64 squares as printed of every board,
/// a weak "empty" on the turned ones. It records the size of every call.
struct Fixed {
    upright: Vec<f32>,
    calls: RefCell<Vec<usize>>,
    batch: usize,
}

impl Fixed {
    fn new(upright: Vec<f32>) -> Fixed {
        Fixed { upright, calls: RefCell::new(Vec::new()), batch: MAX_BATCH }
    }
}

impl SquareClassifier for Fixed {
    fn classify(&self, _: &[f32], count: usize) -> Result<Vec<f32>, ModelError> {
        self.calls.borrow_mut().push(count);
        let weak: Vec<f32> = (0..CLASS_COUNT).map(|k| if k == 0 { 0.4 } else { 0.05 }).collect();
        Ok((0..count).flat_map(|i| if i % 128 < 64 { self.upright[(i % 128) * 13..(i % 128 + 1) * 13].to_vec() } else { weak.clone() }).collect())
    }

    fn max_batch(&self) -> usize {
        self.batch
    }
}

/// Probabilities with `fen`'s class at `p` on every square.
fn matrix(fen: &str, p: f32) -> Vec<f32> {
    parse_placement(fen)
        .unwrap()
        .reading_order()
        .iter()
        .flat_map(|c| (0..13).map(move |k| if k == class_of(*c) { p } else { (1.0 - p) / 12.0 }))
        .collect()
}

fn placement_of(reading: &BoardReading) -> String {
    format_placement(&reading.position().unwrap_or_else(|| panic!("{reading:?}")).placement)
}

#[test]
fn boards_are_read_in_the_detectors_order() {
    let mut page = Page::new(700, 300);
    for (k, fen) in [OPENING, ENDGAME, MIDDLE].iter().enumerate() {
        page.draw(fen, placed(20.0 + 230.0 * k as f32, 40.0, 0.0));
    }
    let geometry = page.geometry(Rotation::R0);
    let boxes: Vec<DiagramCandidate> = (0..3).map(|k| box_candidate(&geometry, 20.0 + 230.0 * k as f32, 40.0)).collect();
    let font = candidate(PageRect::new(0.0, 0.0, 10.0, 10.0), None, Some(ENDGAME));
    let given = [boxes[2].clone(), boxes[0].clone(), font, boxes[1].clone()];
    let model = Nearest::trained();
    let readings = read(&page.image(), &geometry, DPI, &given, Some(&model));
    assert_eq!(readings.iter().map(|r| r.index).collect::<Vec<_>>(), [0, 1, 2, 3]);
    assert_eq!(readings.iter().map(placement_of).collect::<Vec<_>>(), [MIDDLE, OPENING, ENDGAME, ENDGAME]);
    assert_eq!(
        readings.iter().map(|r| r.source).collect::<Vec<_>>(),
        [ReadingSource::Model, ReadingSource::Model, ReadingSource::Font, ReadingSource::Model]
    );
    for (reading, given) in readings.iter().zip(&given) {
        assert_eq!(reading.bbox_user, given.bbox_user);
        assert_eq!(reading.dpi, DPI);
        assert!(reading.accepted(), "{reading:?}");
    }
    let opening = readings[1].position().unwrap();
    assert!(opening.orientation.unwrap().upright);
    assert_eq!(opening.fen, format!("{OPENING} w KQkq - 0 1"));
    assert!(opening.uncertain.is_empty() && !opening.repaired());
    // Three boards, one call of 3 × 128 squares.
    assert_eq!(*model.calls.borrow(), [384]);
}

#[test]
fn a_font_placement_skips_the_model() {
    let page = Page::new(100, 100);
    let geometry = page.geometry(Rotation::R0);
    let model = Fixed::new(matrix(OPENING, 0.99));
    let given = [
        candidate(PageRect::new(0.0, 0.0, 30.0, 30.0), None, Some(MIDDLE)),
        candidate(PageRect::new(0.0, 0.0, 30.0, 30.0), None, Some("8/8/8/8/8/8/8/8")),
    ];
    let readings = read(&page.image(), &geometry, DPI, &given, Some(&model));
    assert!(model.calls.borrow().is_empty(), "the model ran for font diagrams");
    let middle = readings[0].position().unwrap();
    assert_eq!((readings[0].source, middle.min_confidence, middle.orientation), (ReadingSource::Font, 1.0, None));
    assert!(middle.accepted);
    // An exact reading of an illegal position is a value, and it does not pass the gate.
    let empty = readings[1].position().unwrap();
    assert!(!empty.legality.is_legal_for_some_side() && !empty.accepted);
    // Without a model, font diagrams are still read; the others say why not.
    let mixed = [given[0].clone(), box_candidate(&geometry, 0.0, 0.0)];
    let readings = read(&page.image(), &geometry, DPI, &mixed, None);
    assert!(readings[0].accepted());
    assert_eq!(readings[1].outcome, Err(Unreadable::NoModel));
}

#[test]
fn a_rotated_page_is_read_from_its_box_and_from_its_corners() {
    // `/Rotate 90`: the raster is the displayed page, 400 × 600 px, the crop box 300 × 200 pt.
    let mut page = Page::new(400, 600);
    page.draw(MIDDLE, placed(30.0, 320.0, 0.0));
    page.draw(ENDGAME, placed(170.0, 40.0, 0.0));
    let geometry = page.geometry(Rotation::R90);
    assert_eq!(geometry.crop_box(), PageRect::new(0.0, 0.0, 300.0, 200.0));
    let given = [box_candidate(&geometry, 30.0, 320.0), quad_candidate(&geometry, placed(170.0, 40.0, 0.0))];
    let readings = read(&page.image(), &geometry, DPI, &given, Some(&Nearest::trained()));
    assert_eq!(placement_of(&readings[0]), MIDDLE);
    assert_eq!(placement_of(&readings[1]), ENDGAME);
}

#[test]
fn a_turned_board_is_read_from_its_corners() {
    let mut page = Page::new(320, 320);
    let turned = placed(60.0, 60.0, 9.0);
    page.draw(OPENING, turned);
    let geometry = page.geometry(Rotation::R0);
    let model = Nearest::trained();
    let reading = &read(&page.image(), &geometry, DPI, &[quad_candidate(&geometry, turned)], Some(&model))[0];
    assert_eq!(placement_of(reading), OPENING);
}

#[test]
fn an_upside_down_board_is_turned_back() {
    let mut page = Page::new(260, 260);
    page.draw(MIDDLE, placed(30.0, 30.0, 180.0));
    let geometry = page.geometry(Rotation::R0);
    let reading = &read(&page.image(), &geometry, DPI, &[box_candidate(&geometry, 30.0, 30.0)], Some(&Nearest::trained()))[0];
    assert_eq!(placement_of(reading), MIDDLE);
    let verdict = reading.position().unwrap().orientation.unwrap();
    assert!(!verdict.upright, "{verdict:?}");
}

#[test]
fn a_box_past_the_page_edge_is_clamped() {
    // The board fills the page's bottom-right corner and the box runs half a point past it.
    let mut page = Page::new(240, 240);
    page.draw(ENDGAME, placed(40.0, 40.0, 0.0));
    let geometry = page.geometry(Rotation::R0);
    let mut edge = box_candidate(&geometry, 40.0, 40.0);
    edge.bbox_user = PageRect::new(edge.bbox_user.x0, edge.bbox_user.y0 - 0.5, edge.bbox_user.x1 + 0.5, edge.bbox_user.y1);
    let reading = &read(&page.image(), &geometry, DPI, &[edge], Some(&Nearest::trained()))[0];
    assert_eq!(placement_of(reading), ENDGAME);
}

#[test]
fn bad_geometry_is_an_unreadable_value() {
    let page = Page::new(100, 100);
    let geometry = page.geometry(Rotation::R0);
    let model = Fixed::new(matrix(OPENING, 0.99));
    let quad = |corners| candidate(PageRect::new(0.0, 0.0, 10.0, 10.0), Some(corners), None);
    let given = [
        quad([(5.0, 5.0); 4]),
        quad([(f64::NAN, 5.0), (9.0, 5.0), (9.0, 9.0), (5.0, 9.0)]),
        quad([(1e300, 5.0), (9.0, 5.0), (9.0, 9.0), (5.0, 9.0)]),
        quad([(-50.0, 5.0), (-40.0, 5.0), (-40.0, 15.0), (-50.0, 15.0)]),
        candidate(PageRect::new(60.0, 60.0, 90.0, 90.0), None, None),
        candidate(PageRect { x0: f32::NAN, y0: 0.0, x1: 10.0, y1: 10.0 }, None, None),
    ];
    let outcomes: Vec<_> = read(&page.image(), &geometry, DPI, &given, Some(&model)).into_iter().map(|r| r.outcome.err()).collect();
    use Unreadable::*;
    assert_eq!(outcomes, [Some(DegenerateQuad), Some(BadGeometry), Some(BadGeometry), Some(OffPage), Some(OffPage), Some(BadGeometry)]);
    assert!(model.calls.borrow().is_empty());
    for dpi in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        let reading = &read(&page.image(), &geometry, dpi, &[box_candidate(&geometry, 0.0, 0.0)], Some(&model))[0];
        assert_eq!(reading.outcome, Err(BadDpi));
    }
    assert!(read(&page.image(), &geometry, DPI, &[], Some(&model)).is_empty());
    assert!(model.calls.borrow().is_empty());
}

#[test]
fn squares_go_to_the_model_in_batches_of_whole_boards() {
    let page = Page::new(100, 100);
    let geometry = page.geometry(Rotation::R0);
    let given: Vec<DiagramCandidate> = (0..9).map(|_| candidate(PageRect::new(5.0, 5.0, 40.0, 40.0), None, None)).collect();
    let model = Fixed::new(matrix(OPENING, 0.99));
    let readings = read(&page.image(), &geometry, DPI, &given, Some(&model));
    assert_eq!(*model.calls.borrow(), [1024, 128]);
    assert!(readings.iter().all(|r| placement_of(r) == OPENING));
    let small = Fixed { batch: 300, ..Fixed::new(matrix(OPENING, 0.99)) };
    read(&page.image(), &geometry, DPI, &given[..5], Some(&small));
    assert_eq!(*small.calls.borrow(), [256, 256, 128]);
    // Past the cap, boards are not read; font diagrams still are.
    let mut capped = given[..3].to_vec();
    capped.insert(1, candidate(PageRect::new(0.0, 0.0, 9.0, 9.0), None, Some(OPENING)));
    let readings = read_capped(&page.image(), &geometry, DPI, &capped, Some(&model), 2, Preprocessing::DEFAULT);
    let outcomes: Vec<bool> = readings.iter().map(|r| r.outcome.is_ok()).collect();
    assert_eq!(outcomes, [true, true, true, false]);
    assert_eq!(readings[3].outcome, Err(Unreadable::TooMany));
}

#[test]
fn a_failing_model_is_an_unreadable_value() {
    struct Broken(usize);
    impl SquareClassifier for Broken {
        fn classify(&self, _: &[f32], count: usize) -> Result<Vec<f32>, ModelError> {
            if self.0 == 0 { Err(ModelError::Run("out of memory".to_owned())) } else { Ok(vec![1.0; count * self.0]) }
        }
    }
    let page = Page::new(100, 100);
    let geometry = page.geometry(Rotation::R0);
    let given = [candidate(PageRect::new(5.0, 5.0, 40.0, 40.0), None, None), candidate(PageRect::new(0.0, 0.0, 9.0, 9.0), None, Some(OPENING))];
    for broken in [Broken(0), Broken(12), Broken(14)] {
        let readings = read(&page.image(), &geometry, DPI, &given, Some(&broken));
        assert!(matches!(readings[0].outcome, Err(Unreadable::Model(_))), "{:?}", readings[0].outcome);
        assert!(readings[1].accepted());
    }
    // Probabilities that are not numbers are refused by the recognizer, not believed.
    let mut nan = matrix(OPENING, 0.99);
    nan[7] = f32::NAN;
    let reading = &read(&page.image(), &geometry, DPI, &given[..1], Some(&Fixed::new(nan)))[0];
    assert!(matches!(reading.outcome, Err(Unreadable::Probabilities(_))), "{:?}", reading.outcome);
}

#[test]
fn a_repaired_reading_goes_to_review() {
    // The white king read as a queen, the king a close second (the German figurine case).
    let mut v = matrix("8/8/8/4k3/8/8/8/4K3", 0.99);
    let e1 = 7 * 8 + 4;
    v[e1 * 13..(e1 + 1) * 13].copy_from_slice(&[0.0, 0.0, 0.0, 0.0, 0.0, 0.55, 0.44, 0.0, 0.0, 0.0, 0.0, 0.0, 0.01]);
    let page = Page::new(100, 100);
    let geometry = page.geometry(Rotation::R0);
    let reading = &read(&page.image(), &geometry, DPI, &[candidate(PageRect::new(5.0, 5.0, 40.0, 40.0), None, None)], Some(&Fixed::new(v)))[0];
    let position = reading.position().unwrap();
    assert_eq!(format_placement(&position.placement), "8/8/8/4k3/8/8/8/4K3");
    assert!(position.repaired() && position.min_confidence <= 0.5 && !position.accepted);
    assert_eq!(position.uncertain.iter().map(|u| u.square.to_string()).collect::<Vec<_>>(), ["e1"]);
    // The turned reading is weak everywhere: the board as printed wins.
    assert!(position.orientation.unwrap().upright);
}

#[test]
fn squares_are_the_prototypes_cells() {
    // Cell (row, column) of an 800 px board is grey level 3 × (8 row + column), except a8,
    // whose right half is white: 100 px → 64 px puts that edge exactly between columns 31 and 32.
    let mut data = vec![0_u8; 800 * 800];
    for (i, v) in data.iter_mut().enumerate() {
        let (x, y) = (i % 800, i / 800);
        *v = if x < 100 && y < 100 { if x >= 50 { 255 } else { 0 } } else { (3 * (8 * (y / 100) + x / 100)) as u8 };
    }
    let board = GrayImage::new(800, 800, data).unwrap();
    let cut = squares(&board).unwrap();
    assert_eq!(cut.len(), 64 * SQUARE_VALUES);
    let a8 = &cut[..SQUARE_VALUES];
    assert!(a8.chunks(64).all(|row| row[..32].iter().all(|&v| v == 0.0) && row[32..].iter().all(|&v| v == 1.0)));
    for (k, square) in cut.chunks(SQUARE_VALUES).enumerate().skip(1) {
        assert!(square.iter().all(|&v| v == (3 * k) as f32 / 255.0), "square {k}");
    }
    // A board of another size is stretched to 800 first: the middle of each square is its pixel.
    let small = GrayImage::new(8, 8, (0..64).map(|k| (3 * k) as u8).collect()).unwrap();
    let cut = squares(&small).unwrap();
    assert!(cut.chunks(SQUARE_VALUES).enumerate().all(|(k, s)| (s[32 * 64 + 32] - (3 * k) as f32 / 255.0).abs() <= 1.0 / 255.0));
}

#[test]
fn the_turned_squares_are_the_squares_of_the_turned_board() {
    let data: Vec<u8> = (0..800 * 800_usize).map(|i| ((i * 7919 + i / 800 * 13) % 251) as u8).collect();
    let board = GrayImage::new(800, 800, data.clone()).unwrap();
    let turned_board = GrayImage::new(800, 800, data.into_iter().rev().collect()).unwrap();
    let both = model_squares(&board).unwrap();
    assert_eq!(both.len(), SQUARES_PER_BOARD * SQUARE_VALUES);
    let expected = squares(&turned_board).unwrap();
    let worst = both[64 * SQUARE_VALUES..].iter().zip(&expected).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
    assert!(worst <= 1.0 / 255.0 + 1e-6, "{worst}");
}

fn coordinate() -> impl Strategy<Value = f64> {
    prop_oneof![-100.0..300.0_f64, Just(f64::NAN), Just(f64::INFINITY), Just(-1e300), Just(1e30), Just(0.0)]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    /// Any boxes and quads, on any page geometry, give one reading each and never panic.
    #[test]
    fn arbitrary_geometry_never_panics(
        boxes in prop::collection::vec((coordinate(), coordinate(), coordinate(), coordinate()), 0..3),
        quads in prop::collection::vec(prop::array::uniform4((coordinate(), coordinate())), 0..3),
        rotation in prop_oneof![Just(Rotation::R0), Just(Rotation::R90), Just(Rotation::R180), Just(Rotation::R270)],
        dpi in prop_oneof![Just(DPI), Just(72.0_f32), Just(0.0), Just(f32::NAN), Just(1e30), Just(1e-30)],
    ) {
        let page = Page::new(60, 80);
        let geometry = page.geometry(rotation);
        let mut given: Vec<DiagramCandidate> =
            boxes.iter().map(|&(a, b, c, d)| candidate(PageRect { x0: a as f32, y0: b as f32, x1: c as f32, y1: d as f32 }, None, None)).collect();
        given.extend(quads.iter().map(|&q| candidate(PageRect::new(0.0, 0.0, 1.0, 1.0), Some(q), None)));
        let model = Fixed::new(matrix(OPENING, 0.99));
        let readings = read(&page.image(), &geometry, dpi, &given, Some(&model));
        prop_assert_eq!(readings.len(), given.len());
        prop_assert!(readings.iter().enumerate().all(|(k, r)| r.index == k));
    }
}

#[test]
fn a_declared_side_and_move_number_go_into_the_fen() {
    let mut p = font_position(parse_placement(MIDDLE).unwrap());
    assert_eq!(p.side.source, SideSource::Default);
    p.declare_side(Color::Black, SideSource::MoveNumber, 23);
    assert_eq!((p.side.color, p.side.source, p.side.conflicting), (Color::Black, SideSource::MoveNumber, false));
    assert!(p.fen.ends_with(" b - - 0 23"), "{}", p.fen);
    assert!(p.lichess_url().unwrap().contains("%20b%20"), "{:?}", p.lichess_url());
}

#[test]
fn the_position_overrules_a_declared_side() {
    // The black king on e8 is in check from the rook on e1: only Black can be to move.
    let mut p = font_position(parse_placement("4k3/8/8/8/8/8/8/K3R3").unwrap());
    p.declare_side(Color::White, SideSource::MoveNumber, 40);
    assert_eq!((p.side.color, p.side.source, p.side.conflicting), (Color::Black, SideSource::Legality, true));
    assert!(p.fen.ends_with(" b - - 0 1"), "{}", p.fen);
}

/// Hansen, Chess Tactics 2, exercise 24.1 as printed (Black to move, from Black's side): the
/// reading is turned square by square, flagged as told by the pieces, and linked from Black's side.
#[test]
fn a_board_printed_from_blacks_side_is_read_from_whites() {
    let printed = "RQ2R1K1/2BN1PPP/4B3/P2P2p1/pP1p1N1p/3b1n2/1ppbnq2/1k3r1r";
    let page = Page::new(300, 300);
    let geometry = page.geometry(Rotation::R0);
    let model = Fixed::new(matrix(printed, 0.99));
    let readings = read(&page.image(), &geometry, DPI, &[box_candidate(&geometry, 20.0, 40.0)], Some(&model));
    assert_eq!(placement_of(&readings[0]), "r1r3k1/2qnbpp1/2n1b3/p1N1p1Pp/1p2P2P/3B4/PPP1NB2/1K1R2QR");
    let position = readings[0].position().unwrap();
    let verdict = position.orientation.unwrap();
    assert!(verdict.upright && verdict.black_point_of_view && verdict.black_view_from_pieces);
    assert!(position.accepted);
    assert!(position.lichess_url().unwrap().ends_with("?color=black"), "{:?}", position.lichess_url());
}

/// The same board set in a chess font: exact, and turned the same way.
#[test]
fn a_font_board_printed_from_blacks_side_is_read_from_whites() {
    let printed = "RQ2R1K1/2BN1PPP/4B3/P2P2p1/pP1p1N1p/3b1n2/1ppbnq2/1k3r1r";
    let page = Page::new(100, 100);
    let geometry = page.geometry(Rotation::R0);
    let given =
        [candidate(PageRect::new(0.0, 0.0, 30.0, 30.0), None, Some(printed)), candidate(PageRect::new(0.0, 0.0, 30.0, 30.0), None, Some(MIDDLE))];
    let readings = read(&page.image(), &geometry, DPI, &given, None);
    assert_eq!(placement_of(&readings[0]), "r1r3k1/2qnbpp1/2n1b3/p1N1p1Pp/1p2P2P/3B4/PPP1NB2/1K1R2QR");
    let verdict = readings[0].position().unwrap().orientation.unwrap();
    assert!(verdict.black_point_of_view && verdict.black_view_from_pieces);
    // A board from White's side keeps no verdict, as before.
    assert_eq!((placement_of(&readings[1]).as_str(), readings[1].position().unwrap().orientation), (MIDDLE, None));
}

/// The page with the empty dark squares of the board drawn by `placed(x, y, 0)` stippled as some
/// scanned books print them: a checker of 2 px dots `amplitude` levels either side of the square's
/// colour, so each square still averages to it.
fn stippled(page: &mut Page, x: f32, y: f32, fen: &str, amplitude: u8) {
    let dark = luma(style().dark);
    let cells = parse_placement(fen).unwrap().reading_order();
    let square = SIDE / 8.0;
    let mut changed = 0;
    for py in 0..page.height {
        for px in 0..page.width {
            let (u, v) = ((px as f32 + 0.5 - x) / square, (py as f32 + 0.5 - y) / square);
            if !(0.0..8.0).contains(&u) || !(0.0..8.0).contains(&v) {
                continue;
            }
            let (column, row) = (u as usize, v as usize);
            let value = &mut page.data[py * page.width + px];
            if (row + column) % 2 == 1 && cells[row * 8 + column].is_none() && *value == dark {
                *value = if (px / 2 + py / 2) % 2 == 0 { dark - amplitude } else { dark + amplitude };
                changed += 1;
            }
        }
    }
    assert!(changed > 0);
}

/// [`Nearest`], unsure of every square with a fine texture, as the piece model is of stippled
/// squares (it reads them as kings, at a confidence the gate refuses).
struct Fussy(Nearest);

impl SquareClassifier for Fussy {
    fn classify(&self, squares: &[f32], count: usize) -> Result<Vec<f32>, ModelError> {
        let mut out = self.0.classify(squares, count)?;
        for (square, probabilities) in squares.chunks(SQUARE_VALUES).zip(out.chunks_mut(CLASS_COUNT)) {
            // Neighbours a tenth of the gray range apart: a few along a piece's outline, most
            // pairs in a stipple.
            let jumps = square.windows(2).filter(|w| (w[0] - w[1]).abs() > 0.1).count();
            if jumps * 4 > square.len() {
                probabilities.fill(0.0);
                probabilities[0] = 0.5;
                probabilities[6] = 0.5;
            }
        }
        Ok(out)
    }
}

#[test]
fn a_stippled_board_is_read_on_the_retry() {
    let mut page = Page::new(240, 240);
    page.draw(ENDGAME, placed(20.0, 20.0, 0.0));
    stippled(&mut page, 20.0, 20.0, ENDGAME, 70);
    let geometry = page.geometry(Rotation::R0);
    let given = [box_candidate(&geometry, 20.0, 20.0)];
    let model = Fussy(Nearest::trained());
    // As printed, the stipple keeps the board from the gate.
    let plain = Preprocessing::from_letters("0").unwrap();
    let first = &read_with(&page.image(), &geometry, DPI, &given, Some(&model), plain)[0];
    assert!(!first.accepted(), "{first:?}");
    // The retry flattens the empty stippled squares, and its reading is the board's.
    let retry = Preprocessing::from_letters("0>h").unwrap();
    model.0.calls.borrow_mut().clear();
    let second = &read_with(&page.image(), &geometry, DPI, &given, Some(&model), retry)[0];
    assert_eq!(placement_of(second), ENDGAME);
    assert!(second.accepted(), "{second:?}");
    assert_eq!(*model.0.calls.borrow(), [SQUARES_PER_BOARD, SQUARES_PER_BOARD], "one call to read, one to retry");
    // A board the gate accepts is never retried.
    let mut clean = Page::new(240, 240);
    clean.draw(ENDGAME, placed(20.0, 20.0, 0.0));
    model.0.calls.borrow_mut().clear();
    let accepted = &read_with(&clean.image(), &geometry, DPI, &given, Some(&model), retry)[0];
    assert!(accepted.accepted(), "{accepted:?}");
    assert_eq!(*model.0.calls.borrow(), [SQUARES_PER_BOARD]);
}
