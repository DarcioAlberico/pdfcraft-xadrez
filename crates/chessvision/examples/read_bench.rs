//! Board-reading benchmark on the prototype's labelled boards (blueprint S8). Not run in CI.
//!
//! ```text
//! cargo run --release -p pdfcraft-chessvision --example read_bench -- \
//!     <prototype data folder> <models folder> [split=test] [--misses] [--splits=<file>] [--steps=<letters>]
//! ```
//!
//! Reads the prototype's `labels.csv` (file name and FEN of each board image), `splits.csv` (the
//! split of each file; `--splits=` names another, such as an older backup) and `samples/*.png`, keeps the boards of one split as its
//! `BoardFenDataset` does (a FEN that parses; an illegal position only when a person marked it
//! `illegal_ok`), and counts exact boards three ways:
//!
//! - **cells**: the prototype's `evaluate_dataset`: the image's 64 squares as printed, decoded
//!   under the rules. No orientation, no crop.
//! - **read**: the image as a page with one box candidate, through [`board::read`]: crop,
//!   both orientations, decoding, orientation and the gate.
//! - **read 180°**: the same with the image turned half a circle, which orientation must undo.
//!
//! `--steps=` reads with another board preprocessing than the reader's default (letters as
//! [`board::Preprocessing::from_letters`] takes them: `0` none, `0>h` hatch suppression on a retry).
//!
//! The boards and the models are the user's own and stay outside the repository; nothing derived
//! from them is written anywhere.

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use pdfcraft_chess::fen::{format_placement, parse_placement};
use pdfcraft_chess::recognize::decode::Limits;
use pdfcraft_chess::recognize::{CLASS_COUNT, Probabilities, Reading};
use pdfcraft_chess::rules::fatal_violations;
use pdfcraft_chessvision::board;
use pdfcraft_chessvision::coords::{PageGeometry, PageRect, Rotation};
use pdfcraft_chessvision::detect::{DiagramCandidate, Source};
use pdfcraft_chessvision::models::{ChessModels, ModelState, PieceModel};
use pdfcraft_chessvision::raster::GrayImage;

type Error = Box<dyn std::error::Error>;

/// The fields of one CSV line, with double-quoted fields (and `""` inside them) understood.
fn csv_fields(line: &str) -> Vec<String> {
    let (mut fields, mut field, mut quoted, mut chars) = (Vec::new(), String::new(), false, line.chars().peekable());
    while let Some(c) = chars.next() {
        match (c, quoted) {
            ('"', true) if chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            ('"', _) => quoted = !quoted,
            (',', false) => fields.push(std::mem::take(&mut field)),
            _ => field.push(c),
        }
    }
    fields.push(field);
    fields
}

/// A CSV file as rows keyed by its header.
fn csv(path: &Path) -> Result<Vec<HashMap<String, String>>, Error> {
    let text = std::fs::read_to_string(path)?;
    let mut lines = text.lines();
    let header = csv_fields(lines.next().ok_or("empty csv")?);
    Ok(lines.filter(|l| !l.is_empty()).map(|l| header.iter().cloned().zip(csv_fields(l)).collect()).collect())
}

/// A PNG as gray, with OpenCV's `cvtColor(RGB2GRAY)` fixed-point weights (alpha ignored, as
/// `imread` in colour does).
fn gray_png(path: &Path) -> Result<GrayImage, Error> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(std::fs::read(path)?));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info()?;
    let mut buf = vec![0; reader.output_buffer_size().ok_or("image too large")?];
    let info = reader.next_frame(&mut buf)?;
    let bytes = buf.get(..info.buffer_size()).ok_or("short frame")?;
    let luma = |r: u8, g: u8, b: u8| ((u32::from(r) * 4899 + u32::from(g) * 9617 + u32::from(b) * 1868 + (1 << 13)) >> 14) as u8;
    let data: Vec<u8> = match info.color_type {
        png::ColorType::Grayscale => bytes.to_vec(),
        png::ColorType::GrayscaleAlpha => bytes.as_chunks::<2>().0.iter().map(|&[g, _]| g).collect(),
        png::ColorType::Rgb => bytes.as_chunks::<3>().0.iter().map(|&[r, g, b]| luma(r, g, b)).collect(),
        png::ColorType::Rgba => bytes.as_chunks::<4>().0.iter().map(|&[r, g, b, _]| luma(r, g, b)).collect(),
        png::ColorType::Indexed => return Err("palette not expanded".into()),
    };
    Ok(GrayImage::new(info.width, info.height, data)?)
}

/// The image as a page at 72 DPI (one pixel a point) with one box candidate over all of it.
fn read_whole(image: &GrayImage, model: &PieceModel, steps: board::Preprocessing) -> Option<board::BoardReading> {
    let (w, h) = (image.width() as f32, image.height() as f32);
    let geometry = PageGeometry::new(PageRect::new(0.0, 0.0, w, h), Rotation::R0)?;
    let candidate = DiagramCandidate {
        bbox_user: PageRect::new(0.0, 0.0, w, h),
        source: Source::Embedded,
        score: 1.0,
        dpi_hint: 72.0,
        placement: None,
        corners: None,
    };
    board::read_with(image, &geometry, 72.0, &[candidate], Some(model), steps).pop()
}

#[derive(Default)]
struct Tally {
    exact: usize,
    accepted: usize,
    accepted_wrong: usize,
    turned: usize,
    unreadable: usize,
}

impl Tally {
    fn add(&mut self, reading: Option<&board::BoardReading>, label: &str) -> bool {
        let Some(position) = reading.and_then(board::BoardReading::position) else {
            self.unreadable += 1;
            return false;
        };
        let exact = format_placement(&position.placement) == label;
        self.exact += usize::from(exact);
        self.accepted += usize::from(position.accepted);
        self.accepted_wrong += usize::from(position.accepted && !exact);
        self.turned += usize::from(position.orientation.is_some_and(|v| !v.upright));
        exact
    }

    fn line(&self, name: &str, total: usize) -> String {
        format!(
            "{name:<10} exact {:>4}/{total} = {:.4}  accepted {:>4}  accepted but wrong {:>2}  turned 180° {:>4}  unreadable {}",
            self.exact,
            self.exact as f64 / total.max(1) as f64,
            self.accepted,
            self.accepted_wrong,
            self.turned,
            self.unreadable
        )
    }
}

fn main() -> Result<(), Error> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "usage: read_bench <prototype data folder> <models folder> [split=test] [--misses] [--splits=<file>] [--steps=<letters>]";
    let data = Path::new(args.first().ok_or(usage)?);
    let models = ChessModels::load_from(Path::new(args.get(1).ok_or(usage)?));
    let split = args.get(2).filter(|a| !a.starts_with("--")).map_or("test", String::as_str);
    let misses = args.iter().any(|a| a == "--misses");
    let steps = match args.iter().find_map(|a| a.strip_prefix("--steps=")) {
        Some(letters) => board::Preprocessing::from_letters(letters).ok_or("unknown --steps letters")?,
        None => board::Preprocessing::DEFAULT,
    };
    let splits_file = args.iter().find_map(|a| a.strip_prefix("--splits=")).map_or_else(|| data.join("splits.csv"), Into::into);
    let model = match &models.pieces {
        ModelState::Loaded(m) => m,
        ModelState::Missing => return Err("no chess-pieces.onnx in the models folder".into()),
        ModelState::Invalid(why) => return Err(why.clone().into()),
    };

    let splits: HashMap<String, String> =
        csv(&splits_file)?.into_iter().filter_map(|r| Some((r.get("filename")?.clone(), r.get("split")?.clone()))).collect();
    let mut boards = Vec::new();
    let (mut illegal, mut missing) = (0, 0);
    for row in csv(&data.join("labels.csv"))? {
        let (Some(file), Some(fen)) = (row.get("filename"), row.get("fen")) else { continue };
        if splits.get(file).map(String::as_str) != Some(split) {
            continue;
        }
        let Ok(placement) = parse_placement(fen.split_whitespace().next().unwrap_or("")) else { continue };
        let illegal_ok = row.get("illegal_ok").is_some_and(|v| matches!(v.trim().to_lowercase().as_str(), "1" | "true" | "sim" | "yes" | "s" | "y"));
        if !fatal_violations(&placement).is_empty() && !illegal_ok {
            illegal += 1;
            continue;
        }
        let path = data.join("samples").join(file);
        if !path.is_file() {
            missing += 1;
            continue;
        }
        boards.push((file.clone(), path, format_placement(&placement)));
    }
    println!("split {split} of {}: {} boards ({illegal} illegal labels and {missing} missing images left out)", splits_file.display(), boards.len());

    let (mut cells, mut upright, mut turned) = (Tally::default(), Tally::default(), Tally::default());
    let start = Instant::now();
    for (file, path, label) in &boards {
        let image = gray_png(path)?;
        // The prototype's evaluation: the 64 squares as printed, one decode.
        let squares = board::squares(&image).ok_or("cannot cut the board")?;
        let probs = model.probabilities(&squares, 64)?;
        let reading = Reading::new(&Probabilities::new(probs.get(..64 * CLASS_COUNT).unwrap_or(&[]))?, &Limits::default());
        let cell_exact = format_placement(&reading.placement) == *label;
        cells.exact += usize::from(cell_exact);
        cells.accepted += usize::from(reading.accepted());
        cells.accepted_wrong += usize::from(reading.accepted() && !cell_exact);

        let read = read_whole(&image, model, steps);
        let read_exact = upright.add(read.as_ref(), label);
        let flipped = GrayImage::new(image.width(), image.height(), image.data().iter().rev().copied().collect())?;
        let turned_exact = turned.add(read_whole(&flipped, model, steps).as_ref(), label);
        if misses && !(cell_exact && read_exact && turned_exact) {
            let got = read.as_ref().and_then(|r| r.position()).map(|p| (format_placement(&p.placement), p.min_confidence));
            println!("miss {file}: label {label}  cells {}  read {:?}  180° {}", format_placement(&reading.placement), got, turned_exact);
        }
    }
    let total = boards.len();
    println!("{}", cells.line("cells", total));
    println!("{}", upright.line("read", total));
    println!("{}", turned.line("read 180°", total));
    println!("{:.1} ms per board for all three", start.elapsed().as_secs_f64() * 1000.0 / total.max(1) as f64);
    Ok(())
}
