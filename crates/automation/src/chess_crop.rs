//! `chess_diagram_crop`: a diagram's place on its page rendered on demand, for agents (plan
//! A9-I13). The app's Diagram tab shows the page's own picture of the diagram (plan A9-I0) from
//! the raster it drew the page with; an agent has no such raster, so this renders only the
//! diagram's box (with `margin` points around it) at `image_dpi`, as the page shows it: turned
//! as the page turns, with the edits not yet saved.
//!
//! The diagram is numbered as `chess_detect` numbers it, with the same scan settings. The picture
//! comes back inline (a PNG beside the JSON), or is written to `out`, which is confined to
//! `--root` and resolved before anything is scanned or rendered, and may not be the file of an
//! open document unless `overwrite`. Writing it is extraction: refused for a document that does
//! not allow copying, and for an encrypted one unless `allow_encrypted` (the PNG is not
//! encrypted). Nothing in the document changes.

use pdfcraft_engine::chess::pdfcraft_chessvision::coords::ViewRect;
use pdfcraft_render::{RenderRequest, RequestKind, Tile};
use serde_json::json;

use crate::{Args, Automation, Content, Result, ToolError, encode_png, failed, write_atomic};

/// The picture's resolution when `image_dpi` is not given.
pub(crate) const DEFAULT_CROP_DPI: f64 = 150.0;
/// The lowest and highest `image_dpi` taken.
pub(crate) const MIN_CROP_DPI: f64 = 36.0;
pub(crate) const MAX_CROP_DPI: f64 = 600.0;
/// The most points of `margin` around the diagram's box.
pub(crate) const MAX_CROP_MARGIN: f64 = 72.0;
/// The longest side of the picture, in pixels: a larger box is rendered at a lower resolution, and
/// the reply's `dpi` says which.
pub(crate) const MAX_CROP_SIDE: u32 = 4096;

impl Automation {
    pub(crate) fn chess_diagram_crop(&mut self, a: &Args) -> Result<Vec<Content>> {
        // The path first: one that leaves the root is refused before anything is read.
        let out = match a.opt_str("out")? {
            Some(out) => Some((out, self.resolve(out, true)?)),
            None => None,
        };
        let page = self.page(a)?;
        let index = usize::try_from(a.int("index")?).ok().and_then(|i| i.checked_sub(1)).ok_or_else(|| Args::wrong("index", "1 or more"))?;
        let dpi = a.opt_num("image_dpi")?.unwrap_or(DEFAULT_CROP_DPI);
        if !(MIN_CROP_DPI..=MAX_CROP_DPI).contains(&dpi) {
            return Err(Args::wrong("image_dpi", &format!("from {MIN_CROP_DPI} to {MAX_CROP_DPI}")));
        }
        let margin = a.opt_num("margin")?.unwrap_or(0.0);
        if !(0.0..=MAX_CROP_MARGIN).contains(&margin) {
            return Err(Args::wrong("margin", &format!("from 0 to {MAX_CROP_MARGIN} points")));
        }
        if let Some((_, path)) = &out {
            self.check_outputs(a, &[("out", path.as_path())], &[])?;
            let doc = self.doc_to_copy(a)?;
            if doc.info.encrypted && !a.opt_bool("allow_encrypted")?.unwrap_or(false) {
                return Err(failed("the document is encrypted and the picture would be written unencrypted: pass allow_encrypted to write it"));
            }
        }
        let settings = self.chess_settings(a)?;
        let id = self.doc(a)?.id;
        let scanned = self.session.chess_scan(id, &[page], &settings, false).map_err(failed)?;
        let found = scanned.first().ok_or_else(|| failed("the page could not be scanned"))?;
        let count = found.diagrams.len();
        let diagram = found
            .diagrams
            .get(index)
            .ok_or_else(|| ToolError::InvalidArgs(format!("index {} is out of range: page {} has {count} diagrams", index + 1, page + 1)))?;
        let bbox = diagram.candidate.bbox_user;
        let geometry = found.geometry.ok_or_else(|| failed("the page's crop box could not be read"))?;
        let view = geometry.user_to_view(bbox).ok_or_else(|| failed("the diagram's box is not on the page"))?;
        let (w, h) = geometry.view_size();
        let m = margin as f32;
        // `max` then `min`, never `clamp`: no bound can make it panic.
        let area =
            ViewRect::new((view.x0 - m).max(0.0).min(w), (view.y0 - m).max(0.0).min(h), (view.x1 + m).max(0.0).min(w), (view.y1 + m).max(0.0).min(h));
        let (tile, scale) = crop_tile(area, dpi).ok_or_else(|| failed("the diagram's box is empty on the page"))?;
        let shot = self.renderer(id)?.render(RenderRequest { page, kind: RequestKind::Pixels, tile: Some(tile), scale, tag: 0, preview: false });
        if let Some(e) = shot.error {
            return Err(failed(format!("page {} could not be rendered: {e}", page + 1)));
        }
        let png = encode_png(shot.width, shot.height, &shot.rgba)?;
        let round = |v: f32| (f64::from(v) * 100.0).round() / 100.0;
        let mut reply = json!({
            "page": page + 1,
            "index": index + 1,
            "rect": [round(view.x0), round(view.y0), round(view.x1), round(view.y1)],
            "bbox_user": [round(bbox.x0), round(bbox.y0), round(bbox.x1), round(bbox.y1)],
            "area": [round(area.x0), round(area.y0), round(area.x1), round(area.y1)],
            "dpi": round(scale * 72.0),
            "width": shot.width,
            "height": shot.height,
            "bytes": png.len(),
            "written": null,
        });
        let Some((shown_arg, path)) = out else {
            return Ok(vec![Content::Json(reply), Content::Png { data: png, width: shot.width, height: shot.height }]);
        };
        write_atomic(&path, &png).map_err(|e| ToolError::Failed(format!("{shown_arg}: {e}")))?;
        reply["written"] = self.path_json(&path);
        Ok(vec![Content::Json(reply)])
    }
}

/// The pixels of `rect` (view space) at `dpi`, and the scale they are at: `dpi`, or lower so that
/// neither side passes [`MAX_CROP_SIDE`]. `None` for a box that is not finite or has no area.
fn crop_tile(rect: ViewRect, dpi: f64) -> Option<(Tile, f32)> {
    let (w, h) = (rect.width(), rect.height());
    let finite = [rect.x0, rect.y0, rect.x1, rect.y1, w, h].iter().all(|v| v.is_finite());
    if !(finite && w > 0.0 && h > 0.0 && dpi.is_finite() && dpi > 0.0) {
        return None;
    }
    let side = w.max(h);
    let mut scale = (dpi / 72.0) as f32;
    if side * scale > MAX_CROP_SIDE as f32 {
        scale = MAX_CROP_SIDE as f32 / side;
    }
    // Finite and clamped at 0 before the cast; `as` saturates at u32::MAX.
    let px = |v: f32, round: fn(f32) -> f32| {
        let p = round(v * scale);
        if p.is_finite() { p.max(0.0) as u32 } else { 0 }
    };
    let (x0, y0) = (px(rect.x0, f32::floor), px(rect.y0, f32::floor));
    let (x1, y1) = (px(rect.x1, f32::ceil), px(rect.y1, f32::ceil));
    let tile = Tile { x: x0, y: y0, w: x1.saturating_sub(x0).clamp(1, MAX_CROP_SIDE), h: y1.saturating_sub(y0).clamp(1, MAX_CROP_SIDE) };
    Some((tile, scale))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_crop_is_held_to_its_largest_side() {
        let (tile, scale) = crop_tile(ViewRect::new(60.0, 60.0, 220.0, 220.0), 144.0).unwrap();
        assert_eq!((tile.x, tile.y, tile.w, tile.h), (120, 120, 320, 320));
        assert!((scale - 2.0).abs() < 1e-6);
        // A box a page high at 600 dpi: lowered to 4096 pixels on its long side.
        let (tile, scale) = crop_tile(ViewRect::new(0.0, 0.0, 600.0, 800.0), 600.0).unwrap();
        assert!(tile.h <= MAX_CROP_SIDE && tile.w <= MAX_CROP_SIDE, "{tile:?}");
        assert!((scale * 800.0 - MAX_CROP_SIDE as f32).abs() < 1.0);
        // No area, a side of nothing, or not finite (the fields are public: a NaN gets in as is).
        let nan = ViewRect { x0: f32::NAN, y0: 0.0, x1: 1.0, y1: 1.0 };
        for empty in [ViewRect::new(5.0, 5.0, 5.0, 5.0), ViewRect::new(0.0, 0.0, 10.0, 0.0), nan, ViewRect::new(0.0, 0.0, f32::INFINITY, 1.0)] {
            assert_eq!(crop_tile(empty, 150.0), None, "{empty:?}");
        }
        assert_eq!(crop_tile(ViewRect::new(0.0, 0.0, 10.0, 10.0), f64::NAN), None);
    }
}
