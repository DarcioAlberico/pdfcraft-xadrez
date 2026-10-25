//! The board theme the boards are drawn in (plan HC4): the analysis and study boards, the
//! set-up view, the diagram editor and the piece picker's tiles. It is chosen in Preferences ▸
//! Board theme or the board's View menu, kept with the preferences, and held in the egui context
//! so that every board drawn in a frame reads the same one. Diagrams redrawn on the page and the
//! exports keep the default palette.

use egui::Color32;
use pdfcraft_engine::chess::pdfcraft_chess::draw::{BoardStyle, Rgb};
use pdfcraft_engine::chess::pdfcraft_chess::theme::{self, BoardTheme, THEMES, Tint};

use crate::PdfCraftApp;
use crate::theme::Tokens;

/// Where the context holds the theme's index in [`THEMES`].
fn key() -> egui::Id {
    egui::Id::new("chess-board-theme")
}

/// The theme the boards are drawn in.
pub(crate) fn current(ctx: &egui::Context) -> &'static BoardTheme {
    ctx.data(|d| d.get_temp::<usize>(key())).and_then(|i| THEMES.get(i)).unwrap_or(&theme::BROWN)
}

/// Draw the boards in `theme` from now on.
pub(crate) fn set(ctx: &egui::Context, theme: &BoardTheme) {
    let index = THEMES.iter().position(|t| t.id == theme.id).unwrap_or(0);
    ctx.data_mut(|d| d.insert_temp(key(), index));
}

pub(crate) fn rgb(c: Rgb) -> Color32 {
    Color32::from_rgb(c.0, c.1, c.2)
}

pub(crate) fn tint(t: Tint) -> Color32 {
    let [r, g, b, a] = t.0;
    Color32::from_rgba_premultiplied(r, g, b, a)
}

/// `style` with the current theme's squares.
pub(crate) fn style(ctx: &egui::Context, style: BoardStyle) -> BoardStyle {
    let t = current(ctx);
    BoardStyle { light: t.light, dark: t.dark, ..style }
}

impl PdfCraftApp {
    /// Use `theme` for the boards, now and in later sessions.
    pub fn set_board_theme(&mut self, theme: &'static BoardTheme) {
        self.chess.board_theme = (theme.id != theme::BROWN.id).then_some(theme);
        if let Some(ctx) = &self.ctx {
            set(ctx, theme);
            ctx.request_repaint();
        }
    }

    /// The boards' theme.
    pub fn board_theme(&self) -> &'static BoardTheme {
        self.chess.board_theme.unwrap_or(&theme::BROWN)
    }
}

/// A theme's name as the interface says it.
pub(crate) fn name(theme: &BoardTheme) -> &'static str {
    match theme.id {
        "brown" => tl!("Brown"),
        "light-wood" => tl!("Light wood"),
        "walnut" => tl!("Walnut"),
        "blue" => tl!("Blue"),
        "ice" => tl!("Ice blue"),
        "green" => tl!("Green"),
        "olive" => tl!("Olive"),
        "grey" => tl!("Grey"),
        "graphite" => tl!("Graphite"),
        "purple" => tl!("Purple"),
        "pink" => tl!("Pink"),
        "sand" => tl!("Sand"),
        "coral" => tl!("Coral"),
        "high-contrast" => tl!("High contrast"),
        _ => theme.name,
    }
}

/// A tile's side and its squares'.
const TILE: f32 = 72.0;
const SQUARE: f32 = 22.0;

/// Preferences ▸ Board theme: a tile per theme, two squares of each colour with the last move's
/// tint on one; a click picks it.
pub(crate) fn preferences_section(ui: &mut egui::Ui, app: &mut PdfCraftApp, t: &Tokens) {
    ui.label(egui::RichText::new(tl!("Board theme")).font(crate::theme::semibold(13.0)));
    let chosen = app.board_theme();
    let mut pick = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
        for theme in &THEMES {
            if tile(ui, t, theme, theme.id == chosen.id).clicked() {
                pick = Some(theme);
            }
        }
    });
    ui.label(
        egui::RichText::new(tl!("For the boards you play and study on. Diagrams on the page and in exports keep their colours."))
            .small()
            .color(t.text_muted),
    );
    if let Some(theme) = pick {
        app.set_board_theme(theme);
    }
}

fn tile(ui: &mut egui::Ui, t: &Tokens, theme: &BoardTheme, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(TILE, TILE), egui::Sense::click());
    let painter = ui.painter_at(rect);
    let fill = if response.hovered() { t.hover } else { t.card };
    painter.rect_filled(rect, f32::from(t.radius), fill);
    let ring = if selected || response.has_focus() { egui::Stroke::new(2.0, t.accent) } else { egui::Stroke::new(1.0, t.border) };
    painter.rect_stroke(rect.shrink(1.0), f32::from(t.radius), ring, egui::StrokeKind::Inside);
    let top = rect.center_top() + egui::vec2(-SQUARE, 6.0);
    for (x, y) in [(0u8, 0u8), (1, 0), (0, 1), (1, 1)] {
        let light = (x + y) % 2 == 0;
        let cell = egui::Rect::from_min_size(top + egui::vec2(f32::from(x) * SQUARE, f32::from(y) * SQUARE), egui::vec2(SQUARE, SQUARE));
        painter.rect_filled(cell, 0.0, rgb(theme.square(light)));
        if (x, y) == (1, 1) {
            painter.rect_filled(cell, 0.0, tint(theme.last_move));
        }
    }
    let name = name(theme);
    let font = egui::FontId::proportional(11.0);
    let line = crate::widgets::fit_line(name, TILE - 6.0, |s| crate::widgets::text_width(ui, s, &font));
    painter.text(rect.center_bottom() - egui::vec2(0.0, 6.0), egui::Align2::CENTER_BOTTOM, line, font, t.text);
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, selected, name));
    response
}

/// The theme items of a menu (the board's View menu): one per theme, the current one checked.
pub(crate) fn menu_items(ui: &mut egui::Ui, app_theme: &BoardTheme) -> Option<&'static BoardTheme> {
    let mut pick = None;
    for theme in &THEMES {
        if ui.add(egui::Button::new(name(theme)).selected(theme.id == app_theme.id)).clicked() {
            pick = Some(theme);
            ui.close();
        }
    }
    pick
}
