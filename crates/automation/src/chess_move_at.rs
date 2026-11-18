//! `chess_move_at`: the printed move under a point of a page, without the interface (plan A9-N1,
//! §2.9): what a double-click on it in the app opens on the board.
//!
//! The page is read alone, as the app's Text tab reads it (`Session::chess_document_with` on that
//! page: its text layer, with chess OCR when the book's origin is Scanned or `ocr` says), and the
//! move found by `chessvision::doc::move_at`, the function the app's page calls. The point is given
//! as every point of these tools is, in points from the top-left of the displayed page, and goes to
//! user space through the page's one mapping (`PageInfo::view_to_user`: its crop box and
//! `/Rotate`), as the app's double-click does. A point off the page, between two moves or on prose
//! finds nothing, which is no error; a point that is not a finite number, or a page out of range,
//! is refused.

use pdfcraft_engine::chess::document::{BookOrigin, OcrMode, OcrOptions};
use pdfcraft_engine::chess::pdfcraft_chessvision::doc::{Inline, MoveAt, MoveInline, move_at};
use serde_json::{Value, json};

use crate::{Args, Automation, Result, failed};

impl Automation {
    pub(crate) fn chess_move_at(&mut self, a: &Args) -> Result<Value> {
        let page = self.page(a)?;
        let point = [point_arg(a, "x")?, point_arg(a, "y")?];
        let ocr = match a.opt_str("ocr")? {
            None => None,
            Some("auto") => Some(OcrMode::Auto),
            Some("off") => Some(OcrMode::Off),
            Some("all") => Some(OcrMode::All),
            Some(_) => return Err(Args::wrong("ocr", "\"auto\", \"off\" or \"all\"")),
        };
        let settings = self.scan_settings(a)?;
        let id = self.doc(a)?.id;
        let (user, width, height) = {
            let info = self.doc(a)?.info.pages.get(page).ok_or_else(|| failed("no such page"))?;
            (info.view_to_user(point[0], point[1]), info.width, info.height)
        };
        if !user.iter().all(|v| v.is_finite()) {
            return Err(Args::wrong("x and y", "points on the page"));
        }
        let on_page = (0.0..=width).contains(&point[0]) && (0.0..=height).contains(&point[1]);
        // As the Text tab: the text layer, or chess OCR on every page of a scanned book.
        let mode = ocr.unwrap_or(if self.session.chess_origin(id) == BookOrigin::Scanned { OcrMode::All } else { OcrMode::Off });
        let corrections = self.use_store_corrections(a, id)?;
        let read = self.session.chess_document_with(id, &[page], &settings, None, &OcrOptions::new(mode)).map_err(failed)?;
        let index = u32::try_from(page).map_err(|_| failed("no such page"))?;
        let doc = &read.doc;
        let moves = doc.pages.iter().flat_map(|p| p.blocks.iter()).flat_map(|b| b.inlines()).filter(|i| matches!(i, Inline::Move(_))).count();
        let found = move_at(doc, index, user).and_then(|(at, _)| Some((at, at.find(doc)?)));
        Ok(json!({
            "page": page.saturating_add(1),
            "point": point,
            "user": user,
            "on_page": on_page,
            "found": found.is_some(),
            "move": found.map(|(at, m)| move_reply(at, m)),
            "read": {
                "moves": moves,
                "ocr": !read.ocr_pages.is_empty(),
                "needs_ocr": crate::chess::needs_ocr_reply(&read.needs_ocr),
                "notes": crate::chess::notes_reply(&read.notes),
            },
            "corrections": corrections,
        }))
    }
}

/// Coordinate `key` of the point: a number that stays finite as the page's `f32`.
fn point_arg(a: &Args, key: &str) -> Result<f32> {
    let v = a.opt_num(key)?.ok_or_else(|| Args::wrong(key, "a number of points from the top-left of the displayed page"))?;
    // Past `f32::MAX` the narrowing gives infinity, which no page holds.
    let v = v as f32;
    if v.is_finite() { Ok(v) } else { Err(Args::wrong(key, "a number of points from the top-left of the displayed page")) }
}

/// The move found, for the reply: where it is in the reading (block and inline from 0, as the
/// document's JSON indexes them), what was printed and read, its box in user space, the position
/// after it and whether a double-click on it in the app opens a game (it has one, or may go on
/// from the page before).
fn move_reply(at: MoveAt, m: &MoveInline) -> Value {
    json!({
        "block": at.block,
        "inline": at.inline,
        "text": m.text,
        "number": m.number.map(|n| n.text()),
        "san": m.san,
        "status": serde_json::to_value(m.status).unwrap_or(Value::Null),
        "fen": m.fen,
        "game": m.game.map(|g| g.game.saturating_add(1)),
        "rect": m.source_rect.map(|r| [r.x0, r.y0, r.x1, r.y1]),
        "opens": m.opens_game(),
    })
}
