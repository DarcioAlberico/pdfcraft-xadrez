//! Chess ▸ Diagrams: the panel's header (plan A8, P2a2).
//!
//! One 30-point row: Detect diagrams ▾ (the whole book, or this page), and on the right Export book…
//! (`file-output`, the menu of `chess_export_ui`), Board (`grid-3x3` and its word) and ⋯ (Chess models…). While a scan runs, a
//! progress bar (`widgets::progress`) and "Detecting… page 75 of 703" (its width kept for the
//! longest number, so the text does not jump) with Stop, the diagrams found so far staying. When a scan has
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
                // Each item closes the menu, whatever clicked it (plan A9-I15, finding A6-18).
                let n = u64::try_from(pages).unwrap_or(u64::MAX);
                let whole = crate::i18n::trn(crate::i18n::current(), n, "In the whole book ({n} page)", "In the whole book ({n} pages)");
                if widgets::menu_entry(ui, true, egui::Button::new(whole)).clicked() {
                    action = Some(PanelAction::ScanAll);
                }
                if widgets::menu_entry(ui, true, egui::Button::new(tl!("On this page"))).clicked() {
                    action = Some(PanelAction::ScanPage);
                }
            });
        });
        // The icon buttons at the right edge, made left to right so Tab meets them in the order
        // they are seen.
        let gap = ui.spacing().item_spacing.x;
        // The board's button is the one with words beside its icon (plan A9-I8, finding R4-07:
        // its icon was the rail's Set up a position's).
        let board_label = tl!("Board");
        let font = egui::TextStyle::Button.resolve(ui.style());
        let board_width =
            BUTTON.max(14.0 + ui.spacing().icon_spacing + widgets::text_width(ui, board_label, &font) + 2.0 * ui.spacing().button_padding.x);
        let room = ui.available_width() - 2.0 * BUTTON - board_width - 2.0 * gap;
        if room > 0.0 {
            ui.add_space(room);
        }
        let export = icons::button(ui, "file-output", BUTTON, false, tl!("Export book…"));
        egui::Popup::menu(&export).show(|ui| {
            if let Some(a) = crate::chess_export_ui::items(ui, positions, source) {
                action = Some(PanelAction::Text(a));
            }
        });
        let board = ui
            .add(egui::Button::image_and_text(icons::image("grid-3x3", 14.0, t.icon), board_label).min_size(vec2(board_width, BUTTON)))
            .on_hover_text(tl!("Chess board"));
        board.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tl!("Chess board")));
        if board.clicked() {
            action = Some(PanelAction::ShowBoard);
        }
        let more = icons::button(ui, "ellipsis", BUTTON, false, tl!("More chess actions"));
        egui::Popup::menu(&more).show(|ui| {
            if widgets::menu_entry(ui, true, egui::Button::new(tl!("Chess models…")))
                .on_hover_text(tl!("Choose the piece and glyph models that read diagrams and chess text."))
                .clicked()
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
            // As wide as the longest it will say, so Stop does not move as the pages go by.
            let font = egui::TextStyle::Small.resolve(ui.style());
            let width = widgets::text_width(ui, &page(total), &font);
            let text = page(done.saturating_add(1).min(total));
            ui.allocate_ui_with_layout(vec2(width, widgets::MIN_TARGET), Layout::left_to_right(Align::Center), |ui| {
                ui.label(RichText::new(text).small().color(t.text_muted));
            });
            if icons::button(ui, "circle-stop", widgets::MIN_TARGET, false, tl!("Stop (those found stay)")).clicked() {
                action = Some(PanelAction::Cancel);
            }
        });
        // Pages done of the total; in 0..=1 as `done <= total`, the cast to f32 only rounds.
        let fraction = (done.min(total) as f32 / total as f32).clamp(0.0, 1.0);
        // Named for what it is: the words beside it already say the page.
        widgets::progress(ui, fraction, tl_ctx!("progress bar", "Progress"));
    }
    action
}

/// A long job's row (plan A9-I2): what it says ("Exporting the chess document: page 75 of 703"),
/// Stop (its own icon, never the closing ×) and the bar (`widgets::progress`, plan A9-I8), as the
/// scan's. Whether Stop was clicked.
pub(crate) fn long_job(ui: &mut egui::Ui, t: &Tokens, row: &crate::chess_text_ui::LongRow) -> bool {
    let mut stop = false;
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(&row.said).small().color(t.text_muted));
        stop = icons::button(ui, "circle-stop", widgets::MIN_TARGET, false, tl!("Stop")).clicked();
    });
    let total = row.total.max(1);
    // Pages done of the total; in 0..=1 as `done <= total`, the cast to f32 only rounds.
    let fraction = (row.done.min(total) as f32 / total as f32).clamp(0.0, 1.0);
    widgets::progress(ui, fraction, tl_ctx!("progress bar", "Progress"));
    stop
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

/// A scan of another document runs (`done` of `total` pages): why Detect waits, said in the
/// document shown, whose own list stays as it is (plan A9-I1a, §2.7).
pub(crate) fn elsewhere(ui: &mut egui::Ui, t: &Tokens, done: usize, total: usize) {
    let total = total.max(1);
    let page = done.saturating_add(1).min(total);
    let said = crate::i18n::fmt(tl!("Detecting in another book… page {d} of {t}"), &[("d", &page.to_string()), ("t", &total.to_string())]);
    ui.add_space(2.0);
    ui.add(egui::Label::new(RichText::new(said).small().color(t.text_muted)).wrap());
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

    /// While a scan runs: the page it is on, of the total, and Stop (the diagrams found
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
        for name in ["Detect diagrams", "Where to detect", "Export book…", "Chess board", "More chess actions"] {
            h.get_by_label(name);
        }
        // At least 24 × 24 points, as every control the pointer clicks (plan A9-I15, R4-06).
        let stop = h.get_by_label("Stop (those found stay)").rect();
        assert!(stop.width() >= widgets::MIN_TARGET && stop.height() >= widgets::MIN_TARGET, "{stop:?}");
        h.get_by_label("Stop (those found stay)").click();
        h.run();
        assert_eq!(asked.take(), Some(PanelAction::Cancel));
    }
}
