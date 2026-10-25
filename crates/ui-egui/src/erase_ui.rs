//! The Eraser's own gestures and preview (plan/borracha-e-faca.md A3): a box (with the Redact
//! tool's plumbing, see `redact_ui`) or a brush rubbed over the page, and, while either is drawn,
//! what would go tinted red (`Document::erase_preview`). The choices are kept between runs.

use egui::{Color32, Pos2, Rect, Stroke};
use pdfcraft_engine::{Document, Edit, EraseOptions, GlyphRule, MAX_ERASE_AREAS};
use pdfcraft_render::DocInfo;

use crate::canvas::{DocView, PageXform};

const RED: Color32 = Color32::from_rgb(0xE3, 0x22, 0x22);
/// The brush's radius, in points, and its bounds.
pub const BRUSH_DEFAULT: f32 = 8.0;
pub const BRUSH_RANGE: std::ops::RangeInclusive<f32> = 2.0..=40.0;

/// How the Eraser takes its area.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EraseMode {
    /// A box drawn off the text, or a drag across the text (its selection).
    #[default]
    Box,
    /// Rubbed: squares of the brush's size along the pointer's path.
    Brush,
}

/// The Eraser's choices: what goes, how it is drawn, the brush's size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ErasePrefs {
    pub options: EraseOptions,
    pub mode: EraseMode,
    pub brush: f32,
}

impl Default for ErasePrefs {
    fn default() -> Self {
        ErasePrefs { options: EraseOptions::default(), mode: EraseMode::Box, brush: BRUSH_DEFAULT }
    }
}

impl ErasePrefs {
    /// As saved between runs (`"erase_prefs"` in the settings file).
    pub fn to_json(&self) -> serde_json::Value {
        let o = &self.options;
        serde_json::json!({
            "text": o.text, "images": o.images, "vectors": o.vectors, "annotations": o.annotations,
            "rule": if o.rule == GlyphRule::Overlap { "overlap" } else { "centre" },
            "mode": if self.mode == EraseMode::Brush { "brush" } else { "box" },
            "brush": self.brush,
        })
    }

    /// Settings are untrusted: each value is taken only when it reads as what it should be.
    pub fn from_json(v: &serde_json::Value) -> Option<Self> {
        if !v.is_object() {
            return None;
        }
        let mut p = ErasePrefs::default();
        let flag = |k: &str, slot: &mut bool| {
            if let Some(b) = v[k].as_bool() {
                *slot = b;
            }
        };
        flag("text", &mut p.options.text);
        flag("images", &mut p.options.images);
        flag("vectors", &mut p.options.vectors);
        flag("annotations", &mut p.options.annotations);
        if v["rule"].as_str() == Some("overlap") {
            p.options.rule = GlyphRule::Overlap;
        }
        if v["mode"].as_str() == Some("brush") {
            p.mode = EraseMode::Brush;
        }
        if let Some(b) = v["brush"].as_f64().map(|b| b as f32).filter(|b| BRUSH_RANGE.contains(b)) {
            p.brush = b;
        }
        Some(p)
    }
}

/// The preview's cache: what would go for these areas, on this page, as the document stands.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ErasePreview {
    pub key: Option<(usize, u64, Vec<[i64; 4]>, u64)>,
    pub found: Vec<[f64; 4]>,
}

fn to_user(xf: &PageXform, info: &DocInfo, page: usize, p: Pos2) -> Option<[f64; 2]> {
    let (vx, vy) = xf.screen_to_view(p);
    let u = info.pages.get(page)?.view_to_user(vx, vy);
    Some([f64::from(u[0]), f64::from(u[1])])
}

/// A fingerprint of the options, for the preview's cache.
fn options_key(o: &EraseOptions) -> u64 {
    u64::from(o.text)
        | u64::from(o.images) << 1
        | u64::from(o.vectors) << 2
        | u64::from(o.annotations) << 3
        | u64::from(o.rule == GlyphRule::Overlap) << 4
}

/// The squares a brush of radius `r` (points) lays at `at` (user space).
fn square(at: [f64; 2], r: f64) -> [f64; 4] {
    [at[0] - r, at[1] - r, at[0] + r, at[1] + r]
}

/// The Eraser on one page: the brush's gesture in brush mode, and in either mode the preview of
/// what the box or the brush would take, drawn tinted. Returns whether the brush owns the
/// pointer.
#[allow(clippy::too_many_arguments)]
pub(crate) fn page(
    ui: &egui::Ui,
    resp: &egui::Response,
    painter: &egui::Painter,
    xf: &PageXform,
    page: usize,
    info: &DocInfo,
    doc: &Document,
    prefs: &ErasePrefs,
    view: &mut DocView,
) -> bool {
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let r = f64::from(prefs.brush);
    let mut owned = false;
    // The brush: squares along the path while the primary button is down, one edit on release.
    if prefs.mode == EraseMode::Brush {
        if let Some(p) = pointer.filter(|p| xf.rect.contains(*p)) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::None);
            // Screen points per page point, whatever way the view is turned.
            let zoom = ((xf.rect.width() * xf.rect.height()) / (xf.pw * xf.ph).max(1.0)).sqrt();
            let radius = zoom * prefs.brush;
            painter.circle_stroke(p, radius, Stroke::new(1.0, RED));
        }
        if resp.drag_started()
            && let Some(p) = ui.input(|i| i.pointer.press_origin()).filter(|p| xf.rect.contains(*p))
            && let Some(u) = to_user(xf, info, page, p)
        {
            view.erase_brush = Some((page, vec![square(u, r)]));
        }
        if let Some((bp, squares)) = view.erase_brush.as_mut()
            && *bp == page
        {
            owned = true;
            if let Some(u) = pointer.and_then(|p| to_user(xf, info, page, p))
                && squares.len() < MAX_ERASE_AREAS
                && squares.last().is_none_or(|l| {
                    let (cx, cy) = ((l[0] + l[2]) / 2.0, (l[1] + l[3]) / 2.0);
                    (u[0] - cx).hypot(u[1] - cy) >= r / 2.0
                })
            {
                squares.push(square(u, r));
            }
            for s in squares.iter() {
                let rect = xf.user_rect(info, page, s.map(|v| v as f32));
                painter.rect_filled(rect, 0.0, RED.gamma_multiply(0.10));
            }
            if resp.drag_stopped() || !ui.input(|i| i.pointer.primary_down()) {
                let areas = std::mem::take(squares);
                view.erase_brush = None;
                if !areas.is_empty() {
                    view.pending_edit = Some(Edit::EraseContent { page, areas, options: prefs.options });
                }
            }
        }
    }
    // The areas to preview: the brush's, or the box being drawn.
    let areas: Vec<[f64; 4]> = match (&view.erase_brush, view.redact_drag, pointer) {
        (Some((bp, squares)), _, _) if *bp == page => squares.clone(),
        (_, Some((dp, start)), Some(p)) if dp == page && prefs.mode == EraseMode::Box => {
            let b = Rect::from_two_pos(start, p).intersect(xf.rect);
            match (to_user(xf, info, page, b.min), to_user(xf, info, page, b.max)) {
                (Some(a), Some(c)) if b.width() >= 3.0 && b.height() >= 3.0 => {
                    vec![[a[0].min(c[0]), a[1].min(c[1]), a[0].max(c[0]), a[1].max(c[1])]]
                }
                _ => Vec::new(),
            }
        }
        _ => Vec::new(),
    };
    if areas.is_empty() {
        return owned;
    }
    let key =
        (page, doc.edit_generation(), areas.iter().map(|a| a.map(|v| (v * 2.0).round() as i64)).collect::<Vec<_>>(), options_key(&prefs.options));
    if view.erase_preview.key.as_ref() != Some(&key) {
        view.erase_preview.found = doc.erase_preview(page, &areas, &prefs.options).unwrap_or_default();
        view.erase_preview.key = Some(key);
    }
    for f in &view.erase_preview.found {
        let rect = xf.user_rect(info, page, f.map(|v| v as f32));
        painter.rect_filled(rect, 0.0, RED.gamma_multiply(0.35));
    }
    owned
}

/// The Eraser's options in the tool panel.
pub(crate) fn panel(app: &mut crate::PdfCraftApp, ui: &mut egui::Ui) {
    use crate::widgets;
    widgets::section_title(ui, tl!("Eraser"));
    let p = &mut app.erase_prefs;
    ui.horizontal(|ui| {
        if ui.add(widgets::radio(p.mode == EraseMode::Box, tl!("Box or selection"))).clicked() {
            p.mode = EraseMode::Box;
        }
        if ui.add(widgets::radio(p.mode == EraseMode::Brush, tl!("Brush"))).clicked() {
            p.mode = EraseMode::Brush;
        }
    });
    if p.mode == EraseMode::Brush {
        ui.add(egui::Slider::new(&mut p.brush, BRUSH_RANGE).text(tl!("Brush size")).suffix(" pt"));
        ui.label(tl!("Rub over what you want to erase; letting go erases it"));
    } else {
        ui.label(tl!("Drag across text or draw a box to erase what lies under it"));
    }
    let o = &mut p.options;
    ui.add(widgets::checkbox(&mut o.text, tl!("Text")));
    ui.add(widgets::checkbox(&mut o.images, tl!("Images")));
    ui.add(widgets::checkbox(&mut o.vectors, tl!("Drawings and shapes")));
    ui.add(widgets::checkbox(&mut o.annotations, tl!("Comments, links and form fields")));
    ui.label(egui::RichText::new(tl!("What will go is tinted red while you draw.")).small());
    ui.label(
        egui::RichText::new(tl!(
            "Erasing is an edit: an earlier version of the file can still hold what was erased. Use Redact to remove it for good."
        ))
        .small(),
    );
    ui.add_space(6.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The choices round-trip through the settings, and anything else keeps the defaults.
    #[test]
    fn erase_prefs_round_trip_and_ignore_bad_values() {
        let p = ErasePrefs {
            options: EraseOptions { rule: GlyphRule::Overlap, text: false, images: true, vectors: false, annotations: true },
            mode: EraseMode::Brush,
            brush: 12.0,
        };
        assert_eq!(ErasePrefs::from_json(&p.to_json()), Some(p));
        let bad = serde_json::json!({ "text": "yes", "mode": "spray", "brush": 1e9, "rule": 3 });
        assert_eq!(ErasePrefs::from_json(&bad), Some(ErasePrefs::default()));
        assert_eq!(ErasePrefs::from_json(&serde_json::json!(null)), None);
        assert_eq!(ErasePrefs::from_json(&serde_json::json!({ "brush": f64::NAN })), Some(ErasePrefs::default()));
    }
}
