//! Chess ▸ Diagrams: the panel's header (plan A8, P2a2).
//!
//! One 30-point row: Detect diagrams ▾ (the whole book, or this page), and on the right Export…
//! (`file-output`, the menu of `chess_export_ui`), the chess board (`layout-grid`) and ⋯ (Chess models…). While a scan runs, a
//! 4-point progress bar and "Detecting… page 75 of 703" (its width kept for the longest number, so
//! the text does not jump) with × to stop it, the diagrams found so far staying. When a scan has
//! found diagrams, "548 diagrams · 40 to review" with Review now; before any, what detecting does.

use egui::{Align, Layout, RichText, vec2};
use pdfcraft_engine::chess::pdfcraft_chessvision::doc::markdown::PgnPositions;

use crate::chess_ui::PanelAction;
use crate::theme::Tokens;
use crate::{icons, widgets};

/// The header's height, and its icon buttons' side, in points.
const HEAD: f32 = 30.0;
const BUTTON: f32 = 26.0;

/// The header row. `busy`: a scan runs (for this document or another), so none can start;
/// `pages`: the document's page count; `running`: this document's scan, pages done of the total;
/// `positions`: which diagrams the PGN export gives a game of their own; `source`: where it takes
/// its games from, when the document keeps a book's PGN.
pub(crate) fn header(
    ui: &mut egui::Ui,
    t: &Tokens,
    busy: bool,
    pages: usize,
    running: Option<(usize, usize)>,
    positions: PgnPositions,
    source: Option<crate::chess_export_ui::PgnSource>,
) -> Option<PanelAction> {
    let mut action = None;
    ui.horizontal(|ui| {
        ui.set_min_height(HEAD);
        ui.add_enabled_ui(!busy, |ui| {
            let (main, more) = widgets::split_pill(ui, "scan", tl!("Detect diagrams"), tl!("Where to detect"));
            if main.on_hover_text(tl!("Find the diagrams of the whole book and read their positions.")).clicked() {
                action = Some(PanelAction::ScanAll);
            }
            egui::Popup::menu(&more).show(|ui| {
                let n = u64::try_from(pages).unwrap_or(u64::MAX);
                if ui.button(crate::i18n::trn(crate::i18n::current(), n, "In the whole book ({n} page)", "In the whole book ({n} pages)")).clicked() {
                    action = Some(PanelAction::ScanAll);
                }
                if ui.button(tl!("On this page")).clicked() {
                    action = Some(PanelAction::ScanPage);
                }
            });
        });
        // The icon buttons at the right edge, made left to right so Tab meets them in the order
        // they are seen.
        let gap = ui.spacing().item_spacing.x;
        let room = ui.available_width() - 3.0 * BUTTON - 2.0 * gap;
        if room > 0.0 {
            ui.add_space(room);
        }
        let export = icons::button(ui, "file-output", BUTTON, false, tl!("Export…"));
        egui::Popup::menu(&export).show(|ui| {
            if let Some(a) = crate::chess_export_ui::items(ui, positions, source) {
                action = Some(PanelAction::Text(a));
            }
        });
        if icons::button(ui, "layout-grid", BUTTON, false, tl!("Chess board")).clicked() {
            action = Some(PanelAction::ShowBoard);
        }
        let more = icons::button(ui, "ellipsis", BUTTON, false, tl!("More chess actions"));
        egui::Popup::menu(&more).show(|ui| {
            if ui.button(tl!("Chess models…")).on_hover_text(tl!("Choose the piece and glyph models that read diagrams and chess text.")).clicked()
            {
                action = Some(PanelAction::Models);
            }
        });
    });
    if let Some((done, total)) = running {
        let total = total.max(1);
        let page = |d: usize| crate::i18n::fmt(tl!("Detecting… page {d} of {t}"), &[("d", &d.to_string()), ("t", &total.to_string())]);
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            // As wide as the longest it will say, so the × does not move as the pages go by.
            let font = egui::TextStyle::Small.resolve(ui.style());
            let width = widgets::text_width(ui, &page(total), &font);
            let text = page(done.saturating_add(1).min(total));
            ui.allocate_ui_with_layout(vec2(width, 22.0), Layout::left_to_right(Align::Center), |ui| {
                ui.label(RichText::new(text).small().color(t.text_muted));
            });
            if icons::button(ui, "x", 22.0, false, tl!("Stop (those found stay)")).clicked() {
                action = Some(PanelAction::Cancel);
            }
        });
        // Pages done of the total; in 0..=1 as `done <= total`, the cast to f32 only rounds.
        let fraction = (done.min(total) as f32 / total as f32).clamp(0.0, 1.0);
        ui.add(egui::ProgressBar::new(fraction).desired_height(4.0).corner_radius(2));
    }
    action
}

/// What a scan found: "548 diagrams · 40 to review", with Review now when some are.
pub(crate) fn summary(ui: &mut egui::Ui, t: &Tokens, diagrams: usize, to_review: usize) -> Option<PanelAction> {
    let mut action = None;
    ui.horizontal_wrapped(|ui| {
        let n = u64::try_from(diagrams).unwrap_or(u64::MAX);
        let mut text = crate::i18n::trn(crate::i18n::current(), n, "{n} diagram", "{n} diagrams");
        if to_review > 0 {
            text.push_str(" · ");
            text.push_str(&crate::i18n::fmt(tl!("{n} to review"), &[("n", &to_review.to_string())]));
        }
        ui.label(RichText::new(text).color(t.text));
        if to_review > 0 && widgets::pill_button(ui, tl!("Review now"), false).on_hover_text(tl!("Open the first diagram to review.")).clicked() {
            action = Some(PanelAction::ReviewNow);
        }
    });
    action
}

/// Before any scan: what detecting does.
pub(crate) fn empty(ui: &mut egui::Ui, t: &Tokens) {
    ui.label(
        RichText::new(tl!("No diagrams detected yet. PdfCraft finds the diagrams and reads the positions. The whole book takes a few minutes."))
            .color(t.text_muted),
    );
}

#[cfg(test)]
mod tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    use super::*;

    /// While a scan runs: the page it is on, of the total, and × to stop it (the diagrams found
    /// staying); the header's own buttons are named for assistive technology.
    #[test]
    fn a_running_scan_says_its_page_and_stops() {
        let asked = std::rc::Rc::new(std::cell::Cell::new(None));
        let seen = asked.clone();
        let mut fonts = false;
        let mut h = Harness::builder().with_size(egui::vec2(420.0, 160.0)).build_ui(move |ui| {
            // The app's fonts (the pills' medium face) take effect from the next frame.
            if !fonts {
                ui.ctx().set_fonts(crate::theme::font_definitions());
                fonts = true;
                return;
            }
            let t = Tokens::get(ui.ctx());
            if let Some(a) = header(ui, &t, true, 703, Some((74, 703)), PgnPositions::default(), None) {
                seen.set(Some(a));
            }
        });
        h.run();
        h.get_by_label("Detecting… page 75 of 703");
        for name in ["Detect diagrams", "Where to detect", "Export…", "Chess board", "More chess actions"] {
            h.get_by_label(name);
        }
        h.get_by_label("Stop (those found stay)").click();
        h.run();
        assert_eq!(asked.take(), Some(PanelAction::Cancel));
    }
}
