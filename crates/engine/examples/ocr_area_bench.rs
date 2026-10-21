//! Chess OCR of an area of a page (PGN toolbar T9), measured: reads one rectangle of a page with
//! the user's glyph model and prints what it read, the moves in it and how long it took.
//!
//! ```text
//! cargo run --release -p pdfcraft-engine --example ocr_area_bench -- <pdf> <page> <x0> <y0> <x1> <y1> [--models DIR] [--lang en] [--general]
//! ```
//!
//! `page` is 1-based. The corners are fractions of the displayed page from its top-left corner
//! when all four are within 0..=1 (a column of a scanned book: `0.08 0.12 0.52 0.40`), else user
//! space points. `--models` names the folder of `chess-glyphs.onnx` (default: where the app
//! looks, `$PDFCRAFT_MODELS`); `--general` also runs the general OCR for prose when its models
//! are installed. Nothing is written: books stay where they are, outside the repository.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use pdfcraft_engine::Session;
use pdfcraft_engine::chess::document::{OcrMode, OcrOptions};
use pdfcraft_engine::chess::pdfcraft_chess::san::Lang;
use pdfcraft_engine::chess::pdfcraft_chessvision::chess_text::ChessRun;
use pdfcraft_engine::chess::pdfcraft_chessvision::coords::{PageRect, ViewRect};

fn main() {
    if let Err(e) = run() {
        eprintln!("ocr_area_bench: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "usage: ocr_area_bench <pdf> <page> <x0> <y0> <x1> <y1> [--models DIR] [--lang en] [--general]";
    let (mut models, mut lang, mut general, mut plain) = (None::<PathBuf>, Lang::En, false, Vec::new());
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--models" => models = Some(it.next().ok_or(usage)?.into()),
            "--lang" => {
                let code = it.next().ok_or(usage)?;
                lang = Lang::ALL.iter().copied().find(|l| l.code() == code).ok_or(format!("unknown language {code}"))?;
            }
            "--general" => general = true,
            _ => plain.push(a),
        }
    }
    let [path, page, x0, y0, x1, y1] = <[String; 6]>::try_from(plain).map_err(|_| usage.to_string())?;
    let page: usize = page.parse::<usize>().map_err(|e| format!("page: {e}"))?.checked_sub(1).ok_or("pages are numbered from 1")?;
    let corners: Vec<f32> = [x0, y0, x1, y1].iter().map(|v| v.parse::<f32>().map_err(|e| format!("{v}: {e}"))).collect::<Result<_, _>>()?;
    let bytes = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
    let mut session = Session::new();
    let id = session.open("book.pdf", None, Arc::new(bytes), None).map_err(|e| e.to_string())?;
    let info = session.get(id).and_then(|d| d.info.pages.get(page).cloned()).ok_or("no such page")?;
    let geometry = pdfcraft_engine::chess::geometry(&info).ok_or("the page has no area")?;
    let rect = if corners.iter().all(|v| (0.0..=1.0).contains(v)) {
        let (w, h) = geometry.view_size();
        let view = ViewRect::new(corners[0] * w, corners[1] * h, corners[2] * w, corners[3] * h);
        geometry.view_to_user(view).ok_or("the area does not convert")?
    } else {
        PageRect::new(corners[0], corners[1], corners[2], corners[3])
    };
    let options = OcrOptions { mode: OcrMode::Off, glyphs: None, general, book: None };
    let start = Instant::now();
    let read = session.chess_read_area(id, page, rect, &options, lang, models.as_deref()).map_err(|e| e.to_string())?;
    let took = start.elapsed();
    let text = read.chess_text(lang);
    let moves: Vec<String> = text
        .lines
        .iter()
        .flat_map(|l| &l.spans)
        .filter_map(|s| match &s.run {
            ChessRun::Move { san, .. } => Some(san.to_english()),
            _ => None,
        })
        .collect();
    let unsure = read.alternatives.iter().filter(|a| a.margin < 0.5).count();
    println!("area (user space): {:?}  at {:.0} dpi", read.rect_user, read.dpi);
    println!(
        "read in {:.0} ms: {} lines, {} glyphs, {unsure} with a margin under 0.5",
        took.as_secs_f64() * 1000.0,
        read.lines,
        read.text.glyphs.len()
    );
    println!("repair: {} words changed, {} flagged; general OCR: {}", read.edits, read.flags, read.general_ocr);
    println!("--- text ---\n{}", read.plain_text());
    println!("--- moves ({}) ---\n{}", moves.len(), moves.join(" "));
    Ok(())
}
