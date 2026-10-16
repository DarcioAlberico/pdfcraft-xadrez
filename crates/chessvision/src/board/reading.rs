//! Reading a page's diagrams into positions (blueprint S8).
//!
//! [`read`] takes the page raster, the merged candidates in reading order and the piece model,
//! and returns one [`BoardReading`] per candidate, **in the candidates' order**:
//!
//! 1. A candidate that already carries a placement (a diagram font) is read exactly and skips
//!    the model.
//! 2. Otherwise the board is cut from the raster ([`board_image`]): warped from its corners when
//!    the detector found a quadrilateral, then trimmed of a frame ([`super::tighten`]); cut from
//!    its box and stretched to [`super::consts::BOARD_SIZE`] when it has only a box.
//! 3. The board is cut into 64 squares of 64 × 64 gray values ([`squares`]), and each square is
//!    also taken turned half a circle, so every board sends 128 squares to the model.
//! 4. The squares of up to [`boards_per_call`] boards go to the model in **one** call: with the
//!    model's batch of 1024 squares, a page of up to 8 boards is one inference.
//! 5. `pdfcraft-chess` decodes both readings under the rules ([`Reading::new`]), picks the
//!    orientation ([`orient`]) and the side to move, and the gate decides whether the reading
//!    can be exported without review ([`Reading::accepted`]: legal, and the weakest square at
//!    least `ACCEPT_MIN_CONFIDENCE` = 0.80; squares at or below `UNCERTAIN_SQUARE_THRESHOLD` =
//!    0.90 are listed). A reading the decoder repaired has a square at most 0.5 and never passes:
//!    a person confirms it.
//!
//! A board that cannot be read, or reads as an illegal position, is a value
//! ([`Unreadable`], [`ReadPosition::legality`]), never an error or a panic.
//!
//! The preprocessing is the prototype's, because the model was trained on it: `detect_boards`
//! (`tighten_board(warp_from_quad(...))`), `split_board_into_cells` (the board at 800 px, cut
//! into 100 px cells with no inset), `preprocess_cell_to_tensor` (gray, `INTER_AREA` to 64 px,
//! rounded to 8 bits, divided by 255) and `predict_with_orientation` /
//! `board_probabilities_batch` (one batch with the board as printed and turned 180°, no TTA, no
//! temperature).

use pdfcraft_chess::placement::Placement;
use pdfcraft_chess::recognize::decode::{Change, Limits};
use pdfcraft_chess::recognize::orient::{Verdict, orient};
use pdfcraft_chess::recognize::side::{SideToMove, compose_position, infer_side_to_move};
use pdfcraft_chess::recognize::{CLASS_COUNT, Probabilities, ProbabilityError, Reading, Uncertain};
use pdfcraft_chess::rules::{Legality, check_position};
use pdfcraft_geom::PageRect;

use super::consts::BOARD_SIZE;
use super::{tighten, warp};
use crate::coords::{PageGeometry, PixelRect, pt_to_px, view_to_pixels};
use crate::detect::DiagramCandidate;
use crate::imageops::{Plane, resize_area, resize_bilinear, resize_region_bilinear, to_gray, to_plane};
use crate::models::{MAX_BATCH, ModelError, PIECE_SIDE, PieceModel};
use crate::raster::GrayImage;

/// The reader's constants.
pub mod consts {
    use crate::models::PIECE_SIDE;

    /// Side of one square on the 800 px board (`config.CELL_SIZE = BOARD_SIZE // 8`). The
    /// prototype cuts the cells edge to edge, with no inset.
    pub const CELL_SIZE: usize = 100;

    /// Gray values per square fed to the model (`MODEL_IMAGE_SIZE` squared).
    pub const SQUARE_VALUES: usize = PIECE_SIDE * PIECE_SIDE;

    /// Squares per board sent to the model: 64 as printed and 64 turned half a circle
    /// (`predict_with_orientation`, S-61: one forward of 128 squares instead of two of 64).
    pub const SQUARES_PER_BOARD: usize = 128;

    /// Most boards read by the model on one page; the rest come back
    /// [`super::Unreadable::TooMany`]. A safety cap, not a measurement: the prototype's
    /// detector keeps at most 12 a page, and each board costs an 800 × 800 warp.
    pub const MAX_MODEL_BOARDS_PER_PAGE: usize = 64;
}

use consts::*;

/// Something that classifies board squares: the piece model, or a stand-in in tests.
pub trait SquareClassifier {
    /// The 13 class probabilities of each of `count` squares, in the order of
    /// [`pdfcraft_chess::recognize::CLASSES`]. `squares` holds `count` squares of
    /// [`consts::SQUARE_VALUES`] gray values in 0..=1, row-major, one after the other.
    fn classify(&self, squares: &[f32], count: usize) -> Result<Vec<f32>, ModelError>;

    /// Most squares one call accepts.
    fn max_batch(&self) -> usize {
        MAX_BATCH
    }
}

impl SquareClassifier for PieceModel {
    fn classify(&self, squares: &[f32], count: usize) -> Result<Vec<f32>, ModelError> {
        self.probabilities(squares, count)
    }
}

/// How many boards go to `model` in one call: as many as its batch holds, at least one (a model
/// whose batch is smaller than one board then fails, and says so).
pub fn boards_per_call(model: &dyn SquareClassifier) -> usize {
    (model.max_batch().min(MAX_BATCH) / SQUARES_PER_BOARD).max(1)
}

/// Where a reading's position came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReadingSource {
    /// The piece model read the squares.
    Model,
    /// The detector read the diagram font's characters; exact, no model.
    Font,
}

/// Why a board could not be read.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum Unreadable {
    #[error("no piece model is loaded")]
    NoModel,
    #[error("the page raster's resolution is not a positive number")]
    BadDpi,
    #[error("the diagram's box or corners are not finite numbers on this page")]
    BadGeometry,
    #[error("the diagram lies outside the page raster")]
    OffPage,
    #[error("the diagram's corners do not form a quadrilateral")]
    DegenerateQuad,
    #[error("the page has more diagrams than are read on one page")]
    TooMany,
    #[error("the piece model failed: {0}")]
    Model(String),
    #[error("the piece model's output is not a probability matrix: {0}")]
    Probabilities(ProbabilityError),
}

/// A position read from a diagram.
#[derive(Clone, Debug, PartialEq)]
pub struct ReadPosition {
    /// The pieces, in the orientation chosen.
    pub placement: Placement,
    /// The full FEN: the side to move as [`ReadPosition::side`] decided it, castling inferred
    /// from where the kings and rooks stand.
    pub fen: String,
    pub legality: Legality,
    /// Who is to move and how that is known (legality, or the default White, assumed).
    pub side: SideToMove,
    /// Which way up the board was read, and which rule decided. `None` for a font diagram.
    pub orientation: Option<Verdict>,
    /// The lowest square confidence (1 for a font diagram): the gate's measure, not the mean.
    pub min_confidence: f32,
    /// Squares at or below `UNCERTAIN_SQUARE_THRESHOLD` (0.90), with their second reading.
    pub uncertain: Vec<Uncertain>,
    /// Squares the decoder changed from the most probable class to make the position legal.
    pub repairs: Vec<Change>,
    /// Whether the reading may be exported without review.
    pub accepted: bool,
}

impl ReadPosition {
    /// Whether the decoder changed any square.
    pub fn repaired(&self) -> bool {
        !self.repairs.is_empty()
    }
}

/// One candidate's reading.
#[derive(Clone, Debug, PartialEq)]
pub struct BoardReading {
    /// The candidate's index in the slice given to [`read`].
    pub index: usize,
    /// The candidate's box, in user space.
    pub bbox_user: PageRect,
    pub source: ReadingSource,
    /// The resolution of the raster the board was read from, in dots per inch.
    pub dpi: f32,
    pub outcome: Result<ReadPosition, Unreadable>,
}

impl BoardReading {
    /// The position, when the board could be read.
    pub fn position(&self) -> Option<&ReadPosition> {
        self.outcome.as_ref().ok()
    }

    /// Whether the reading passes the gate.
    pub fn accepted(&self) -> bool {
        self.position().is_some_and(|p| p.accepted)
    }
}

/// Read every candidate of one page. `page` is the whole displayed page (view space, after
/// `/Rotate`) rendered at `dpi`; `geometry` maps the candidates' user-space boxes and corners onto
/// it. Returns one reading per candidate, in the candidates' order. Without a `model`, font
/// diagrams are still read and the others are [`Unreadable::NoModel`].
pub fn read(
    page: &GrayImage,
    geometry: &PageGeometry,
    dpi: f32,
    candidates: &[DiagramCandidate],
    model: Option<&dyn SquareClassifier>,
) -> Vec<BoardReading> {
    read_capped(page, geometry, dpi, candidates, model, MAX_MODEL_BOARDS_PER_PAGE)
}

/// [`read`] with the cap on model boards as a parameter, so tests can reach it cheaply.
fn read_capped(
    page: &GrayImage,
    geometry: &PageGeometry,
    dpi: f32,
    candidates: &[DiagramCandidate],
    model: Option<&dyn SquareClassifier>,
    max_model_boards: usize,
) -> Vec<BoardReading> {
    let mut readings = Vec::with_capacity(candidates.len());
    let per_call = model.map_or(1, boards_per_call);
    // The squares of the boards waiting for the model, and their slots in `readings`.
    let mut batch = Batch { squares: Vec::new(), slots: Vec::new() };
    let mut model_boards = 0_usize;
    for (index, candidate) in candidates.iter().enumerate() {
        let (source, outcome) = match (&candidate.placement, model) {
            (Some(placement), _) => (ReadingSource::Font, Ok(font_position(*placement))),
            (None, None) => (ReadingSource::Model, Err(Unreadable::NoModel)),
            (None, Some(_)) => {
                let outcome = if model_boards >= max_model_boards {
                    Err(Unreadable::TooMany)
                } else {
                    model_boards = model_boards.saturating_add(1);
                    // `squares` fails only on an empty board, which `board_image` never returns.
                    board_image(page, geometry, dpi, candidate).and_then(|board| model_squares(&board).ok_or(Unreadable::OffPage))
                };
                match outcome {
                    Ok(squares) => {
                        batch.squares.extend(squares);
                        batch.slots.push(readings.len());
                        // Filled in when the batch runs.
                        (ReadingSource::Model, Err(Unreadable::NoModel))
                    }
                    Err(e) => (ReadingSource::Model, Err(e)),
                }
            }
        };
        readings.push(BoardReading { index, bbox_user: candidate.bbox_user, source, dpi, outcome });
        if let Some(model) = model
            && batch.slots.len() >= per_call
        {
            batch.run(model, &mut readings);
        }
    }
    if let Some(model) = model {
        batch.run(model, &mut readings);
    }
    readings
}

/// Boards cut and waiting for one model call.
struct Batch {
    squares: Vec<f32>,
    slots: Vec<usize>,
}

impl Batch {
    /// Classify the waiting boards in one call and fill in their readings.
    fn run(&mut self, model: &dyn SquareClassifier, readings: &mut [BoardReading]) {
        if self.slots.is_empty() {
            return;
        }
        let count = self.slots.len().saturating_mul(SQUARES_PER_BOARD);
        let per_board = SQUARES_PER_BOARD * CLASS_COUNT;
        let result = model.classify(&self.squares, count).and_then(|p| {
            let expected = count.saturating_mul(CLASS_COUNT);
            if p.len() == expected { Ok(p) } else { Err(ModelError::BatchSize { expected, count, got: p.len() }) }
        });
        for (k, &slot) in self.slots.iter().enumerate() {
            let outcome = match &result {
                Ok(p) => {
                    let start = k.saturating_mul(per_board);
                    match p.get(start..start.saturating_add(per_board)) {
                        Some(board) => model_position(board),
                        None => Err(Unreadable::Model("the model returned too few values".to_owned())),
                    }
                }
                Err(e) => Err(Unreadable::Model(e.to_string())),
            };
            if let Some(reading) = readings.get_mut(slot) {
                reading.outcome = outcome;
            }
        }
        self.squares.clear();
        self.slots.clear();
    }
}

/// The position a diagram font gave: exact, so every square is certain and only legality gates.
fn font_position(placement: Placement) -> ReadPosition {
    let legality = check_position(&placement);
    let accepted = legality.is_legal_for_some_side();
    finish(placement, legality, None, 1.0, Vec::new(), Vec::new(), accepted)
}

/// The position from one board's 128 rows of probabilities: 64 as printed, then 64 turned.
fn model_position(probabilities: &[f32]) -> Result<ReadPosition, Unreadable> {
    let half = 64 * CLASS_COUNT;
    let (upright, turned) = (probabilities.get(..half).unwrap_or(&[]), probabilities.get(half..).unwrap_or(&[]));
    let limits = Limits::default();
    let upright = Reading::new(&Probabilities::new(upright).map_err(Unreadable::Probabilities)?, &limits);
    let turned = Reading::new(&Probabilities::new(turned).map_err(Unreadable::Probabilities)?, &limits);
    // No printed coordinates are read yet, so the verdict never asks for Black's point of view.
    let verdict = orient(&upright, &turned, None);
    let chosen = if verdict.upright { upright } else { turned };
    let accepted = chosen.accepted();
    let Reading { placement, min_confidence, uncertain, decoded, legality, .. } = chosen;
    Ok(finish(placement, legality, Some(verdict), min_confidence, uncertain, decoded.changes, accepted))
}

fn finish(
    placement: Placement,
    legality: Legality,
    orientation: Option<Verdict>,
    min_confidence: f32,
    uncertain: Vec<Uncertain>,
    repairs: Vec<Change>,
    accepted: bool,
) -> ReadPosition {
    let side = infer_side_to_move(&placement, None);
    let fen = compose_position(placement, side.color, None, 1).fen();
    ReadPosition { placement, fen, legality, side, orientation, min_confidence, uncertain, repairs, accepted }
}

/// The candidate's board as the model reads it: [`BOARD_SIZE`] pixels a side, upright as
/// displayed.
///
/// - With corners (a turned or skewed board), the quad is warped to a square and trimmed of a
///   frame (`tighten_board(warp_from_quad(...))`, as the prototype's `detect_boards`).
/// - With a box only, its pixels are cut and stretched bilinearly (the prototype's embedded
///   crop, resized by `split_board_into_cells`). The box is clamped to the raster.
pub fn board_image(page: &GrayImage, geometry: &PageGeometry, dpi: f32, candidate: &DiagramCandidate) -> Result<GrayImage, Unreadable> {
    if !(dpi.is_finite() && dpi > 0.0) {
        return Err(Unreadable::BadDpi);
    }
    let (width, height) = (page.width(), page.height());
    match candidate.corners {
        Some(corners) => {
            let mut quad = [(0.0_f32, 0.0_f32); 4];
            for (slot, (x, y)) in quad.iter_mut().zip(corners) {
                // An f64 too large for f32 becomes infinite, which the conversion refuses.
                let [vx, vy] = geometry.user_to_view_point(x as f32, y as f32).ok_or(Unreadable::BadGeometry)?;
                *slot = (pt_to_px(vx, dpi).ok_or(Unreadable::BadGeometry)?, pt_to_px(vy, dpi).ok_or(Unreadable::BadGeometry)?);
            }
            // A quad wholly beside the raster would warp to a black board.
            let (w, h) = (width as f32, height as f32);
            let beside = |pick: fn(&(f32, f32)) -> f32, limit: f32| quad.iter().all(|p| pick(p) < 0.0) || quad.iter().all(|p| pick(p) >= limit);
            if beside(|p| p.0, w) || beside(|p| p.1, h) {
                return Err(Unreadable::OffPage);
            }
            let board = warp(page, quad, BOARD_SIZE).ok_or(Unreadable::DegenerateQuad)?;
            Ok(tighten(&board))
        }
        None => {
            let view = geometry.user_to_view(candidate.bbox_user).ok_or(Unreadable::BadGeometry)?;
            let px = view_to_pixels(view, dpi).ok_or(Unreadable::BadGeometry)?;
            let rect = PixelRect { x0: px.x0.min(width), y0: px.y0.min(height), x1: px.x1.min(width), y1: px.y1.min(height) };
            // Sampled in place: a box as large as the page costs no copy of it.
            resize_region_bilinear(page, rect, BOARD_SIZE, BOARD_SIZE).ok_or(Unreadable::OffPage)
        }
    }
}

/// The 64 squares of a board in reading order (a8, b8 … h1 as displayed), each
/// [`consts::SQUARE_VALUES`] gray values in 0..=1, as the model takes them
/// (`split_board_into_cells` + `preprocess_cell_to_tensor`): the board stretched bilinearly to
/// [`BOARD_SIZE`] if it is not that size, cut into [`consts::CELL_SIZE`] px cells edge to edge,
/// each averaged down to 64 px (`INTER_AREA`), rounded to a gray level and divided by 255.
pub fn squares(board: &GrayImage) -> Option<Vec<f32>> {
    let resized;
    let board = if (board.width(), board.height()) == (BOARD_SIZE, BOARD_SIZE) {
        board
    } else {
        resized = resize_bilinear(board, BOARD_SIZE, BOARD_SIZE)?;
        &resized
    };
    let plane = to_plane(board);
    let mut out = Vec::with_capacity(64 * SQUARE_VALUES);
    for row in 0..8 {
        for column in 0..8 {
            let mut data = Vec::with_capacity(CELL_SIZE * CELL_SIZE);
            for y in row * CELL_SIZE..(row + 1) * CELL_SIZE {
                data.extend_from_slice(plane.row(y).get(column * CELL_SIZE..(column + 1) * CELL_SIZE)?);
            }
            let cell = Plane { width: CELL_SIZE, height: CELL_SIZE, data };
            // The prototype resizes 8-bit cells, so OpenCV rounds the result to 8 bits.
            let small = to_gray(&resize_area(&cell, PIECE_SIDE, PIECE_SIDE)?)?;
            out.extend(small.data().iter().map(|&v| f32::from(v) / 255.0));
        }
    }
    Some(out)
}

/// The 128 squares the model reads for one board: [`squares`], then the squares of the board
/// turned half a circle. Turning the 800 px board moves cell `i` to `63 − i` and turns each cell;
/// the area average is symmetric, so turning each 64 px square is the same.
fn model_squares(board: &GrayImage) -> Option<Vec<f32>> {
    let mut out = squares(board)?;
    let turned: Vec<f32> = out.chunks(SQUARE_VALUES).rev().flat_map(|square| square.iter().rev().copied()).collect();
    out.extend(turned);
    Some(out)
}

#[cfg(test)]
mod tests;
