//! `chess_read_area`: Chess ▸ PGN toolbar ▸ OCR area without the interface (PGN toolbar T9 and
//! T10).
//!
//! An area of a page is read with chess OCR (`Session::chess_read_area`: only that rectangle
//! rendered, the user's chess-glyphs model, the general OCR for prose when installed, the repair),
//! and the reply gives what was read: the text, the moves, and each glyph's top readings with the
//! margin between the first two (the preview underlines a glyph below 0.5). With a game (`fen` or
//! `pgn`) it also plans capturing the text after a move (`engine::chess::capture::plan`, as the
//! capture of an OCR area does: no weight, moves placed by their numbers), with the moves that did
//! not play and the legal moves near each. Nothing is applied and no document is changed.
//!
//! The only path, `models_dir`, is confined to `--root` and resolved first, before anything else
//! is read. Reading is extraction: a document that does not allow copying refuses it.

use pdfcraft_engine::chess::capture::{self, CaptureText};
use pdfcraft_engine::chess::document::{OcrMode, OcrOptions};
use pdfcraft_engine::chess::pdfcraft_chess::game::Game;
use pdfcraft_engine::chess::pdfcraft_chess::san::Lang;
use pdfcraft_engine::chess::pdfcraft_chessvision::chess_text::ChessRun;
use pdfcraft_engine::chess::pdfcraft_chessvision::coords::PageRect;
use pdfcraft_engine::chess::text_ocr::AreaRead;
use serde_json::{Value, json};

use crate::{Args, Automation, Result, ToolError, chess_pgn, failed};

/// Two decimals: enough for points, and stable in test output.
fn round(v: f32) -> f64 {
    (f64::from(v) * 100.0).round() / 100.0
}

fn rect_json(r: [f32; 4]) -> Value {
    json!(r.map(round))
}

impl Automation {
    pub(crate) fn chess_read_area(&mut self, a: &Args) -> Result<Value> {
        // The path first: one that leaves the root is refused before anything is read.
        let models_dir = self.models_dir(a)?;
        let lang = match a.opt_str("lang")? {
            None => Lang::En,
            Some(tag) => Lang::from_tag(tag).ok_or_else(|| Args::wrong("lang", "a language tag such as en or pt"))?,
        };
        let general = a.opt_bool("general_ocr")?.unwrap_or(true);
        let page = self.page(a)?;
        let [x0, y0, x1, y1] = a.nums::<4>("rect")?.ok_or_else(|| Args::wrong("rect", "[x0, y0, x1, y1] in PDF user space"))?.map(|v| v as f32);
        // Either corner first; a coordinate past f32 is infinite, and the engine refuses it.
        let rect = PageRect::new(x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1));
        // The game, before the (slow) reading, so a bad one is said at once.
        let game = game_of(a)?;
        let id = self.doc(a)?.id;
        let geometry = self.doc(a)?.info.pages.get(page).and_then(pdfcraft_engine::chess::geometry);
        let options = OcrOptions { mode: OcrMode::Off, glyphs: None, general, book: None };
        let read = self.session.chess_read_area(id, page, rect, &options, lang, models_dir.as_deref()).map_err(failed)?;
        let mut out = read_json(&read, lang);
        if let (Some((game, at, mode)), Some(o)) = (game, out.as_object_mut()) {
            let page = u32::try_from(page).unwrap_or(u32::MAX);
            let plan = capture::plan(&game, at, mode, CaptureText::Read { text: &read.text, geometry, page }, lang);
            o.insert("capture".into(), chess_pgn::capture_json(&plan, false, lang));
        }
        Ok(out)
    }
}

/// The game to plan a capture in, the move to capture after and how: `fen` or `pgn`, `at` (an
/// address; the main line's last move by default) and `mode`. `None` without `fen` and `pgn`.
fn game_of(a: &Args) -> Result<Option<(Game, usize, capture::CaptureMode)>> {
    let game = match (a.opt_str("fen")?, a.opt_str("pgn")?) {
        (Some(_), Some(_)) => return Err(ToolError::InvalidArgs("give fen or pgn, not both".into())),
        (Some(fen), None) => Game::from_fen(fen).map_err(|e| ToolError::InvalidArgs(format!("fen: {e}")))?,
        (None, Some(pgn)) => Game::from_pgn(pgn).map_err(|e| ToolError::InvalidArgs(format!("pgn: {e}")))?.0,
        (None, None) => {
            if a.opt_str("at")?.is_some() || a.opt_str("mode")?.is_some() {
                return Err(ToolError::InvalidArgs("at and mode plan a capture: give fen or pgn too".into()));
            }
            return Ok(None);
        }
    };
    let at = match a.opt_str("at")? {
        Some(at) => chess_pgn::resolve(&game, at).map_err(ToolError::InvalidArgs)?,
        None => game.main_line().last().copied().unwrap_or(0),
    };
    let mode = chess_pgn::capture_mode(a.opt_str("mode")?).map_err(ToolError::InvalidArgs)?;
    Ok(Some((game, at, mode)))
}

/// What was read, as the reply gives it.
fn read_json(read: &AreaRead, lang: Lang) -> Value {
    let chess = read.chess_text(lang);
    let mut moves = Vec::new();
    let mut number: Option<String> = None;
    for span in chess.lines.iter().flat_map(|l| &l.spans) {
        match &span.run {
            ChessRun::MoveNumber { number: n, black } => number = Some(if *black { format!("{n}...") } else { format!("{n}.") }),
            ChessRun::Move { san, text } => moves.push(json!({
                "number": number.take(),
                "text": text,
                "san": san.to_english(),
                "rect": rect_json([span.rect.x0, span.rect.y0, span.rect.x1, span.rect.y1]),
            })),
            _ => number = None,
        }
    }
    json!({
        "page": read.page + 1,
        "rect_user": rect_json(read.rect_user),
        "rect": rect_json(read.rect_view),
        "dpi": round(read.dpi),
        "text": read.plain_text(),
        "lines": read.lines,
        "edits": read.edits,
        "flags": read.flags,
        "general_ocr": read.general_ocr,
        "moves": moves,
        "alternatives": read.alternatives.iter().map(|g| json!({
            "rect": rect_json(g.rect),
            "text": g.text,
            "margin": round(g.margin),
            "choices": g.choices.iter().map(|(text, p)| json!({ "text": text, "p": round(*p) })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}
