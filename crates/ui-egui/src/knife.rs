//! The Knife (plan/borracha-e-faca.md B2): cut pages into parts on the canvas. A dashed line
//! follows the pointer across the page under it, upright or (with Shift held, or Horizontal
//! chosen in the panel) across; a click places a cut there, a right-click takes away the nearest
//! one, Enter cuts and Esc clears. A line snaps to the page's middle and to the widest gap
//! between its columns of text near the middle, so a spread splits where its two pages meet.

use egui::{Color32, Pos2, Stroke};
use pdfcraft_engine::{Cut, CutAxis, Edit, MAX_CUT_PARTS};
use pdfcraft_render::DocInfo;

use crate::canvas::{DocView, PageXform};

/// How close, in screen points, a line must come to the middle or a gutter to snap to it.
const SNAP: f32 = 10.0;
/// A gutter is looked for in this middle share of the page.
const GUTTER_SEARCH: (f32, f32) = (0.3, 0.7);
/// Least width of a gutter, as a share of the page.
const GUTTER_MIN: f32 = 0.01;
const LINE: Color32 = Color32::from_rgb(0xE3, 0x22, 0x22);

/// Which pages a cut applies to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum KnifePages {
    #[default]
    This,
    Selected,
    All,
}

/// The Knife's state for one view: the cuts placed on a page, as shares (0–1) of its width
/// (vertical) or height (horizontal), as it is shown.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KnifeView {
    pub page: Option<usize>,
    pub axis: Option<CutAxis>,
    pub at: Vec<f32>,
}

/// The Knife's choices, for this run.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KnifePrefs {
    pub axis: CutAxis,
    pub pages: KnifePages,
    pub right_to_left: bool,
}

impl Default for KnifePrefs {
    fn default() -> Self {
        KnifePrefs { axis: CutAxis::Vertical, pages: KnifePages::This, right_to_left: false }
    }
}

impl KnifeView {
    pub fn clear(&mut self) {
        *self = KnifeView::default();
    }
}

/// The axis in force: the panel's, turned while Shift is held.
fn axis(ui: &egui::Ui, prefs: &KnifePrefs) -> CutAxis {
    let shift = ui.input(|i| i.modifiers.shift);
    match (prefs.axis, shift) {
        (CutAxis::Vertical, false) | (CutAxis::Horizontal, true) => CutAxis::Vertical,
        _ => CutAxis::Horizontal,
    }
}

/// The share (0–1) of the page the pointer is at, along `axis`, on the page as shown (the view's
/// own rotation undone).
fn share(xf: &PageXform, axis: CutAxis, p: Pos2) -> f32 {
    let (u, v) = xf.screen_to_norm(p);
    let s = match axis {
        CutAxis::Vertical => u,
        CutAxis::Horizontal => v,
    };
    if s.is_finite() { s.clamp(0.0, 1.0) } else { 0.5 }
}

/// The line at share `at` along `axis`, on screen.
fn ends(xf: &PageXform, axis: CutAxis, at: f32) -> (Pos2, Pos2) {
    match axis {
        CutAxis::Vertical => (xf.norm_to_screen(at, 0.0), xf.norm_to_screen(at, 1.0)),
        CutAxis::Horizontal => (xf.norm_to_screen(0.0, at), xf.norm_to_screen(1.0, at)),
    }
}

/// The middle of the widest run of the page without glyphs, along `axis`, near the middle, as a
/// share; `glyphs` are view-space boxes and `size` the page's view size.
pub fn gutter(glyphs: &[[f32; 4]], size: [f32; 2], axis: CutAxis) -> Option<f32> {
    let (extent, lo, hi) = match axis {
        CutAxis::Vertical => (size[0], 0usize, 2usize),
        CutAxis::Horizontal => (size[1], 1usize, 3usize),
    };
    if !(extent.is_finite() && extent > 0.0) {
        return None;
    }
    let mut spans: Vec<(f32, f32)> = glyphs
        .iter()
        .filter_map(|g| Some((*g.get(lo)? / extent, *g.get(hi)? / extent)))
        .filter(|(a, b)| a.is_finite() && b.is_finite() && b > a)
        .collect();
    if spans.is_empty() {
        return None;
    }
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (from, to) = GUTTER_SEARCH;
    let mut best: Option<(f32, f32)> = None;
    let mut reach = 0.0f32;
    for (a, b) in spans {
        if a > reach {
            // A gap from `reach` to `a`, clipped to the search band.
            let (g0, g1) = (reach.max(from), a.min(to));
            if g1 - g0 >= GUTTER_MIN && best.is_none_or(|(b0, b1)| g1 - g0 > b1 - b0) {
                best = Some((g0, g1));
            }
        }
        reach = reach.max(b);
    }
    best.map(|(a, b)| (a + b) / 2.0)
}

/// The share a line at `p` snaps to: the middle, or the gutter, when close enough on screen.
fn snapped(xf: &PageXform, axis: CutAxis, p: Pos2, gutter: Option<f32>) -> f32 {
    let s = share(xf, axis, p);
    let here = ends(xf, axis, s).0;
    [gutter, Some(0.5)].into_iter().flatten().find(|t| (ends(xf, axis, *t).0 - here).length() <= SNAP).unwrap_or(s)
}

/// Knife gestures on one page: the line under the pointer, clicks and right-clicks.
pub(crate) fn page_input(ui: &egui::Ui, resp: &egui::Response, xf: &PageXform, page: usize, prefs: &KnifePrefs, view: &mut DocView) {
    let Some(p) = ui.input(|i| i.pointer.hover_pos()).filter(|p| xf.rect.contains(*p)) else { return };
    ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
    let axis = axis(ui, prefs);
    let glyphs: Vec<[f32; 4]> = view.page_text(page).map(|t| t.glyphs.iter().map(|g| g.rect).collect()).unwrap_or_default();
    let at = snapped(xf, axis, p, gutter(&glyphs, [xf.pw, xf.ph], axis));
    if view.knife.page != Some(page) || view.knife.axis != Some(axis) {
        if resp.clicked() || resp.secondary_clicked() {
            view.knife = KnifeView { page: Some(page), axis: Some(axis), at: Vec::new() };
        } else {
            view.knife_hover = Some((page, axis, at));
            return;
        }
    }
    view.knife_hover = Some((page, axis, at));
    if resp.clicked() && view.knife.at.len() + 1 < MAX_CUT_PARTS && !view.knife.at.iter().any(|a| (a - at).abs() < 0.005) {
        view.knife.at.push(at);
        view.knife.at.sort_by(f32::total_cmp);
    }
    if resp.secondary_clicked()
        && let Some(i) = view.knife.at.iter().enumerate().min_by(|a, b| (a.1 - at).abs().total_cmp(&(b.1 - at).abs())).map(|(i, _)| i)
    {
        view.knife.at.remove(i);
    }
}

/// The cuts placed, and the line under the pointer, on `page`.
pub(crate) fn paint(painter: &egui::Painter, xf: &PageXform, page: usize, view: &DocView) {
    let line = |at: f32, axis: CutAxis, alpha: f32| {
        let (a, b) = ends(xf, axis, at);
        painter.extend(egui::Shape::dashed_line(&[a, b], Stroke::new(1.5, LINE.gamma_multiply(alpha)), 6.0, 4.0));
    };
    if view.knife.page == Some(page)
        && let Some(axis) = view.knife.axis
    {
        for at in &view.knife.at {
            line(*at, axis, 1.0);
        }
    }
    if let Some((p, axis, at)) = view.knife_hover
        && p == page
    {
        line(at, axis, 0.5);
    }
}

/// The edit for the cuts placed: at the same shares on `pages` (0-based), measured on each page
/// as shown. `None` when no cut is placed.
pub fn cut_edit(knife: &KnifeView, info: &DocInfo, pages: &[usize], right_to_left: bool) -> Option<Edit> {
    let axis = knife.axis?;
    if knife.at.is_empty() {
        return None;
    }
    let cuts: Vec<Cut> = pages
        .iter()
        .filter_map(|&page| {
            let p = info.pages.get(page)?;
            let extent = f64::from(match axis {
                CutAxis::Vertical => p.width,
                CutAxis::Horizontal => p.height,
            });
            Some(Cut { page, axis, at: knife.at.iter().map(|a| f64::from(*a) * extent).collect() })
        })
        .collect();
    (!cuts.is_empty()).then_some(Edit::CutPages { cuts, right_to_left })
}

impl crate::PdfCraftApp {
    /// Cut the pages the Knife's choices name at the cuts placed (Enter, or Cut in the panel),
    /// as one undo step; the cuts are let go of when it worked.
    pub fn knife_cut(&mut self) -> bool {
        let Some((i, id)) = self.active_ids() else { return false };
        let Some(doc) = self.session.get(id) else { return false };
        let view = &self.views[i];
        let Some(page) = view.knife.page else { return false };
        let n = doc.info.pages.len();
        let pages: Vec<usize> = match self.knife_prefs.pages {
            KnifePages::This => vec![page],
            KnifePages::Selected if !view.selected.is_empty() => view.selected.iter().copied().filter(|p| *p < n).collect(),
            KnifePages::Selected => vec![page],
            KnifePages::All => (0..n).collect(),
        };
        let Some(edit) = cut_edit(&view.knife, &doc.info, &pages, self.knife_prefs.right_to_left) else { return false };
        let done = self.apply_edit(edit);
        if done {
            self.views[i].knife.clear();
        }
        done
    }
}

/// The Knife's options in the tool panel.
pub(crate) fn panel(app: &mut crate::PdfCraftApp, ui: &mut egui::Ui) {
    use crate::widgets;
    widgets::section_title(ui, tl!("Knife (cut pages)"));
    ui.label(tl!("Click to place a cut, right-click to take one away, Enter to cut"));
    let p = &mut app.knife_prefs;
    ui.horizontal(|ui| {
        if ui.add(widgets::radio(p.axis == CutAxis::Vertical, tl!("Up and down"))).clicked() {
            p.axis = CutAxis::Vertical;
        }
        if ui.add(widgets::radio(p.axis == CutAxis::Horizontal, tl!("Across"))).clicked() {
            p.axis = CutAxis::Horizontal;
        }
    });
    ui.label(egui::RichText::new(tl!("Hold Shift for the other direction.")).small());
    for (pages, label) in [(KnifePages::This, "Current page"), (KnifePages::Selected, "Selected pages"), (KnifePages::All, "All pages")] {
        if ui.add(widgets::radio(p.pages == pages, tl!(label))).clicked() {
            p.pages = pages;
        }
    }
    ui.add(widgets::checkbox(&mut p.right_to_left, tl!("Right part first")));
    let placed = app.active_ids().map_or(0, |(i, _)| app.views[i].knife.at.len());
    ui.horizontal(|ui| {
        if ui.add_enabled_ui(placed > 0, |ui| widgets::pill_button(ui, tl!("Cut"), true)).inner.clicked() {
            app.knife_cut();
        }
        if ui.add_enabled_ui(placed > 0, |ui| widgets::pill_button(ui, tl!("Clear"), false)).inner.clicked()
            && let Some((i, _)) = app.active_ids()
        {
            app.views[i].knife.clear();
        }
    });
    ui.label(egui::RichText::new(tl!("Nothing is erased: each part shows its piece of the page.")).small());
    ui.add_space(6.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two columns of text with a gap from 45% to 55% of the width: the gutter is its middle.
    /// A page with no gap near the middle, or no text, has none.
    #[test]
    fn the_gutter_is_the_widest_gap_near_the_middle() {
        let size = [200.0, 300.0];
        let glyphs = [[10.0, 10.0, 90.0, 20.0], [110.0, 10.0, 190.0, 20.0], [12.0, 40.0, 88.0, 50.0]];
        let g = gutter(&glyphs, size, CutAxis::Vertical).unwrap();
        assert!((g - 0.5).abs() < 1e-6, "{g}");
        assert_eq!(gutter(&[[10.0, 10.0, 190.0, 20.0]], size, CutAxis::Vertical), None, "one wide line");
        assert_eq!(gutter(&[], size, CutAxis::Vertical), None);
        assert_eq!(gutter(&glyphs, [0.0, 0.0], CutAxis::Vertical), None);
        // The widest gap wins, not the first: 65–70 is narrow, 75–130 is wide.
        let two = [[10.0, 10.0, 65.0, 20.0], [70.0, 10.0, 75.0, 20.0], [130.0, 10.0, 190.0, 20.0]];
        let g = gutter(&two, size, CutAxis::Vertical).unwrap();
        assert!((g - (0.375 + 0.65) / 2.0).abs() < 1e-6, "{g}");
        // Across: lines at the top and the bottom.
        let rows = [[10.0, 10.0, 190.0, 100.0], [10.0, 200.0, 190.0, 290.0]];
        let g = gutter(&rows, size, CutAxis::Horizontal).unwrap();
        assert!((g - 0.5).abs() < 1e-6, "{g}");
    }
}
