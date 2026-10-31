//! The board theme the boards are drawn in (plan HC4): the analysis and study boards, the
//! set-up view, the diagram editor and the piece picker's tiles. It is chosen in Preferences ▸
//! Board theme or the board's View menu, kept with the preferences, and held in the egui context
//! so that every board drawn in a frame reads the same one. Diagrams redrawn on the page and the
//! exports keep the default palette.
//!
//! Beside the palette, how the board is laid out ([`BoardLook`], plan HC6): the coordinates
//! inside the edge squares, beside the board or none; no frame, a thin one or a book's double
//! one; a marked square as a ring or filled; a dot on the side to move; and how the last move is
//! marked, and how strongly ([`LastMove`]).

use egui::Color32;
use pdfcraft_engine::chess::pdfcraft_chess::draw::{BoardStyle, Rgb};
use pdfcraft_engine::chess::pdfcraft_chess::theme::{self, BoardTheme, THEMES, Tint};

use crate::PdfCraftApp;
use crate::theme::Tokens;

/// Where the context holds the theme's index in [`THEMES`].
fn key() -> egui::Id {
    egui::Id::new("chess-board-theme")
}

/// Where the board's coordinates go.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Coordinates {
    /// In the corner of the edge squares, in the other square's colour.
    #[default]
    Inside,
    /// Beside the board: the ranks to its left, the files under it.
    Outside,
    None,
}

/// The line around the board.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Frame {
    #[default]
    None,
    Thin,
    /// A thick line and a thin one inside it, as books print diagrams.
    Double,
}

/// How a square marked by hand is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SquareMarks {
    #[default]
    Ring,
    Fill,
}

/// How the last move's two squares are marked, in the theme's colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LastMove {
    /// Both squares tinted.
    #[default]
    Fill,
    /// A line inside both squares.
    Border,
    /// A short line along each corner of both squares.
    Corners,
    /// The square left outlined, the one reached tinted: which way the piece went.
    FromBorder,
    /// Both outlined, over a lighter tint.
    BorderFill,
    /// An arrow from the square left to the one reached, over the pieces.
    Arrow,
    None,
}

impl LastMove {
    pub const ALL: [LastMove; 7] =
        [LastMove::Fill, LastMove::Border, LastMove::Corners, LastMove::FromBorder, LastMove::BorderFill, LastMove::Arrow, LastMove::None];

    /// Its name in the preferences and the control channel.
    pub fn token(self) -> &'static str {
        match self {
            LastMove::Fill => "fill",
            LastMove::Border => "border",
            LastMove::Corners => "corners",
            LastMove::FromBorder => "from-border",
            LastMove::BorderFill => "border-fill",
            LastMove::Arrow => "arrow",
            LastMove::None => "none",
        }
    }

    pub fn from_token(token: &str) -> Option<LastMove> {
        LastMove::ALL.into_iter().find(|m| m.token() == token)
    }

    /// Its name as the interface says it.
    fn label(self) -> &'static str {
        match self {
            LastMove::Fill => tl!("Filled"),
            LastMove::Border => tl!("Border"),
            LastMove::Corners => tl!("Corners"),
            LastMove::FromBorder => tl!("Start outlined, end filled"),
            LastMove::BorderFill => tl!("Border and light fill"),
            LastMove::Arrow => tl!("Arrow"),
            LastMove::None => tl!("None"),
        }
    }
}

/// How strongly the last move is marked, in percent of the theme's tint.
pub const LAST_MOVE_STRENGTHS: std::ops::RangeInclusive<u8> = 10..=100;

/// How the boards are laid out, beside their palette (plan HC6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoardLook {
    pub coordinates: Coordinates,
    pub frame: Frame,
    pub squares: SquareMarks,
    /// A dot on the side to move's edge.
    pub turn_dot: bool,
    pub last_move: LastMove,
    /// In [`LAST_MOVE_STRENGTHS`]; read through [`BoardLook::last_move_strength`].
    pub last_move_strength: u8,
}

impl Default for BoardLook {
    fn default() -> BoardLook {
        BoardLook {
            coordinates: Coordinates::default(),
            frame: Frame::default(),
            squares: SquareMarks::default(),
            turn_dot: false,
            last_move: LastMove::default(),
            last_move_strength: 100,
        }
    }
}

impl BoardLook {
    /// How strongly the last move is marked, in percent, kept within [`LAST_MOVE_STRENGTHS`].
    pub fn last_move_strength(self) -> u8 {
        self.last_move_strength.clamp(*LAST_MOVE_STRENGTHS.start(), *LAST_MOVE_STRENGTHS.end())
    }

    pub fn to_json(self) -> serde_json::Value {
        let coordinates = match self.coordinates {
            Coordinates::Inside => "inside",
            Coordinates::Outside => "outside",
            Coordinates::None => "none",
        };
        let frame = match self.frame {
            Frame::None => "none",
            Frame::Thin => "thin",
            Frame::Double => "double",
        };
        let squares = match self.squares {
            SquareMarks::Ring => "ring",
            SquareMarks::Fill => "fill",
        };
        serde_json::json!({
            "coordinates": coordinates,
            "frame": frame,
            "squares": squares,
            "turn_dot": self.turn_dot,
            "last_move": self.last_move.token(),
            "last_move_strength": self.last_move_strength(),
        })
    }

    /// The look `v` keeps; each field not understood keeps its default, and a strength out of
    /// range is brought within it.
    pub fn from_json(v: &serde_json::Value) -> BoardLook {
        let mut look = BoardLook::default();
        look.set("coordinates", v["coordinates"].as_str().unwrap_or_default()).ok();
        look.set("frame", v["frame"].as_str().unwrap_or_default()).ok();
        look.set("squares", v["squares"].as_str().unwrap_or_default()).ok();
        look.turn_dot = v["turn_dot"].as_bool().unwrap_or(false);
        look.set("last-move", v["last_move"].as_str().unwrap_or_default()).ok();
        if let Some(n) = v["last_move_strength"].as_u64() {
            look.last_move_strength = u8::try_from(n).unwrap_or(u8::MAX);
            look.last_move_strength = look.last_move_strength();
        }
        look
    }

    /// Set one part by name, as the control channel names it: `coordinates` (inside, outside,
    /// none), `frame` (none, thin, double), `squares` (ring, fill), `turn-dot` (on, off),
    /// `last-move` (fill, border, corners, from-border, border-fill, arrow, none) and
    /// `last-move-strength` (10 to 100).
    pub fn set(&mut self, part: &str, value: &str) -> Result<(), String> {
        match part {
            "last-move" => {
                self.last_move =
                    LastMove::from_token(value).ok_or("board-last-move must be fill, border, corners, from-border, border-fill, arrow or none")?;
                return Ok(());
            }
            "last-move-strength" => {
                self.last_move_strength = value
                    .trim()
                    .parse::<u8>()
                    .ok()
                    .filter(|n| LAST_MOVE_STRENGTHS.contains(n))
                    .ok_or("board-last-move-strength must be a whole number from 10 to 100")?;
                return Ok(());
            }
            _ => {}
        }
        match (part, value) {
            ("coordinates", "inside") => self.coordinates = Coordinates::Inside,
            ("coordinates", "outside") => self.coordinates = Coordinates::Outside,
            ("coordinates", "none") => self.coordinates = Coordinates::None,
            ("frame", "none") => self.frame = Frame::None,
            ("frame", "thin") => self.frame = Frame::Thin,
            ("frame", "double") => self.frame = Frame::Double,
            ("squares", "ring") => self.squares = SquareMarks::Ring,
            ("squares", "fill") => self.squares = SquareMarks::Fill,
            ("turn-dot", "on") => self.turn_dot = true,
            ("turn-dot", "off") => self.turn_dot = false,
            ("coordinates", _) => return Err("board-coordinates must be inside, outside or none".into()),
            ("frame", _) => return Err("board-frame must be none, thin or double".into()),
            ("squares", _) => return Err("board-squares must be ring or fill".into()),
            ("turn-dot", _) => return Err("board-turn-dot must be on or off".into()),
            _ => return Err(format!("no board option {part}")),
        }
        Ok(())
    }

    /// Room around the board's squares in a `side`-point square: the frame's on every side, and
    /// the coordinates' to the left and below when they go beside the board.
    fn margins(self, side: f32) -> (f32, f32) {
        let frame = match self.frame {
            Frame::None => 0.0,
            Frame::Thin => 2.0,
            Frame::Double => 6.0,
        };
        let labels = if self.coordinates == Coordinates::Outside { (side / 22.0).clamp(10.0, 16.0) } else { 0.0 };
        (frame, labels)
    }

    /// Where the squares go in `outer`, the square the board was given.
    pub(crate) fn board_rect(self, outer: egui::Rect) -> egui::Rect {
        let (frame, labels) = self.margins(outer.width());
        let side = (outer.width().min(outer.height()) - labels - 2.0 * frame).max(8.0);
        egui::Rect::from_min_size(egui::pos2(outer.left() + labels + frame, outer.top() + frame), egui::vec2(side, side))
    }
}

/// The frame around `board` as the look says, in the interface's text colour.
pub(crate) fn paint_frame(ui: &egui::Ui, t: &Tokens, look: BoardLook, board: egui::Rect) {
    let painter = ui.painter();
    let ink = t.text;
    match look.frame {
        Frame::None => {}
        Frame::Thin => {
            painter.rect_stroke(board.expand(1.0), 0.0, egui::Stroke::new(1.5, ink), egui::StrokeKind::Middle);
        }
        Frame::Double => {
            painter.rect_stroke(board.expand(1.0), 0.0, egui::Stroke::new(1.0, ink), egui::StrokeKind::Middle);
            painter.rect_stroke(board.expand(4.5), 0.0, egui::Stroke::new(2.5, ink), egui::StrokeKind::Middle);
        }
    }
}

fn look_key() -> egui::Id {
    egui::Id::new("chess-board-look")
}

/// How the boards are laid out now.
pub(crate) fn look(ctx: &egui::Context) -> BoardLook {
    ctx.data(|d| d.get_temp::<BoardLook>(look_key())).unwrap_or_default()
}

pub(crate) fn set_look(ctx: &egui::Context, look: BoardLook) {
    ctx.data_mut(|d| d.insert_temp(look_key(), look));
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
    let hatched = t.texture == Some(pdfcraft_engine::chess::pdfcraft_chess::texture::Texture::Hatched);
    BoardStyle { light: t.light, dark: t.dark, hatched, ..style }
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

    /// Lay the boards out as `look` says, now and in later sessions.
    pub fn set_board_look(&mut self, look: BoardLook) {
        self.chess.board_look = look;
        if let Some(ctx) = &self.ctx {
            set_look(ctx, look);
            ctx.request_repaint();
        }
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
        "wood" => tl!("Wood"),
        "marble" => tl!("Marble"),
        "paper" => tl!("Paper"),
        "book" => tl!("Book print"),
        _ => theme.name,
    }
}

/// A tile's side and its squares'.
const TILE: f32 = 72.0;
const SQUARE: f32 = 22.0;

/// Preferences ▸ Board theme: a tile per theme, two squares of each colour with a move between
/// two of them marked as the look says; a click picks it.
pub(crate) fn preferences_section(ui: &mut egui::Ui, app: &mut PdfCraftApp, t: &Tokens) {
    ui.label(egui::RichText::new(tl!("Board theme")).font(crate::theme::semibold(13.0)));
    let chosen = app.board_theme();
    let look = app.chess.board_look;
    let mut pick = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
        for theme in &THEMES {
            if tile(ui, t, theme, look, theme.id == chosen.id).clicked() {
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
    let mut look = app.chess.board_look;
    look_options(ui, &mut look);
    if look != app.chess.board_look {
        app.set_board_look(look);
    }
    // How the move list writes the pieces (plan HC8).
    use crate::chess_figurines::MovePieces;
    ui.horizontal_wrapped(|ui| {
        ui.label(tl!("Pieces in the move list"));
        crate::widgets::radio_value(ui, &mut app.chess.move_pieces, MovePieces::Figurines, tl!("Figurines"));
        crate::widgets::radio_value(ui, &mut app.chess.move_pieces, MovePieces::Letters, tl!("Letters in your language"));
        crate::widgets::radio_value(ui, &mut app.chess.move_pieces, MovePieces::English, tl!("English letters"));
    });
    ui.add(crate::widgets::checkbox(&mut app.chess.comment_focus, tl!("The comment on the move shown, large under the board")));
    let mut preview = !app.chess.no_move_preview;
    if ui.add(crate::widgets::checkbox(&mut preview, tl!("A move the pointer rests on shows its position"))).changed() {
        app.chess.no_move_preview = !preview;
    }
    // Variations (plan A8 V3).
    use crate::chess_moves_tree::{AtFork, NewMove};
    let rules = &mut app.chess.variation_rules;
    ui.horizontal_wrapped(|ui| {
        ui.label(tl!("Stepping forward at a fork"));
        crate::widgets::radio_value(ui, &mut rules.at_fork, AtFork::Follow, tl!("Follow the line remembered"));
        crate::widgets::radio_value(ui, &mut rules.at_fork, AtFork::Ask, tl!("Ask"));
    });
    ui.horizontal_wrapped(|ui| {
        ui.label(tl!("Playing a different move"));
        crate::widgets::radio_value(ui, &mut rules.new_move, NewMove::Variation, tl!("Start a variation"));
        crate::widgets::radio_value(ui, &mut rules.new_move, NewMove::Ask, tl!("Ask"));
        crate::widgets::radio_value(ui, &mut rules.new_move, NewMove::Replace, tl!("Replace the continuation"));
    });
    // The editor in the Diagrams panel, as before the Diagram tab (plan A8, P3b2; for one version).
    ui.add(crate::widgets::checkbox(&mut app.chess.classic_editor, tl!("Classic diagram editor")))
        .on_hover_text(tl!("Edit diagrams inside the Diagrams panel, as before the Diagram tab. This option goes away in a later version."));
    // The move list's layout and text size (plan HC9).
    use crate::chess_figurines::{ListLayout, ListSize};
    let look = &mut app.chess.list_look;
    ui.horizontal_wrapped(|ui| {
        ui.label(tl!("Move list"));
        crate::widgets::radio_value(ui, &mut look.layout, ListLayout::Flow, tl!("Running text"));
        crate::widgets::radio_value(ui, &mut look.layout, ListLayout::Columns, tl!("Columns"));
    });
    ui.horizontal_wrapped(|ui| {
        ui.label(tl!("Text size"));
        crate::widgets::radio_value(ui, &mut look.size, ListSize::Small, tl!("Small"));
        crate::widgets::radio_value(ui, &mut look.size, ListSize::Normal, tl!("Normal"));
        crate::widgets::radio_value(ui, &mut look.size, ListSize::Large, tl!("Large"));
    });
}

/// The look's choices, as radio buttons in a grid.
fn look_options(ui: &mut egui::Ui, look: &mut BoardLook) {
    egui::Grid::new("chess-board-look").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        ui.label(tl!("Coordinates"));
        ui.horizontal(|ui| {
            crate::widgets::radio_value(ui, &mut look.coordinates, Coordinates::Inside, tl!("On the squares"));
            crate::widgets::radio_value(ui, &mut look.coordinates, Coordinates::Outside, tl!("Beside the board"));
            crate::widgets::radio_value(ui, &mut look.coordinates, Coordinates::None, tl!("None"));
        });
        ui.end_row();
        ui.label(tl!("Frame"));
        ui.horizontal(|ui| {
            crate::widgets::radio_value(ui, &mut look.frame, Frame::None, tl!("None"));
            crate::widgets::radio_value(ui, &mut look.frame, Frame::Thin, tl!("Thin"));
            crate::widgets::radio_value(ui, &mut look.frame, Frame::Double, tl!("Double"));
        });
        ui.end_row();
        ui.label(tl!("Marked squares"));
        ui.horizontal(|ui| {
            crate::widgets::radio_value(ui, &mut look.squares, SquareMarks::Ring, tl!("Ring"));
            crate::widgets::radio_value(ui, &mut look.squares, SquareMarks::Fill, tl!("Filled"));
        });
        ui.end_row();
        let label = ui.label(tl!("Last move"));
        ui.horizontal(|ui| {
            let marks = |ui: &mut egui::Ui, look: &mut BoardLook| {
                for mark in LastMove::ALL {
                    ui.selectable_value(&mut look.last_move, mark, mark.label());
                }
            };
            // egui keeps one popup open at a time: in the board's menu a combo box would close the
            // menu on opening, so the list is a submenu there.
            let list = if egui::containers::menu::is_in_menu(ui) {
                ui.menu_button(look.last_move.label(), |ui| marks(ui, look)).response
            } else {
                egui::ComboBox::from_id_salt("chess-last-move").selected_text(look.last_move.label()).show_ui(ui, |ui| marks(ui, look)).response
            };
            list.labelled_by(label.id);
        });
        ui.end_row();
        let label = ui.label(tl!("Strength"));
        let mut strength = look.last_move_strength();
        let slider = ui.scope(|ui| {
            // A menu's background is the rail's colour: there the rail takes the separators'.
            if egui::containers::menu::is_in_menu(ui) {
                ui.visuals_mut().widgets.inactive.bg_fill = ui.visuals().widgets.noninteractive.bg_stroke.color;
            }
            let slider = egui::Slider::new(&mut strength, LAST_MOVE_STRENGTHS).step_by(5.0).suffix(" %").trailing_fill(true);
            ui.add_enabled(look.last_move != LastMove::None, slider)
        });
        if slider.inner.labelled_by(label.id).changed() {
            look.last_move_strength = strength;
        }
        ui.end_row();
    });
    ui.add(crate::widgets::checkbox(&mut look.turn_dot, tl!("A dot on the side to move")));
}

/// The colour of the last move's lines at `percent` of its strength.
fn last_move_ink(theme: &BoardTheme, percent: u8) -> Color32 {
    let c = theme.last_move_line();
    let alpha = u8::try_from(u16::from(percent.min(100)) * 255 / 100).unwrap_or(u8::MAX);
    Color32::from_rgba_unmultiplied(c.0, c.1, c.2, alpha)
}

/// The last move marked under the pieces as the look says, in `theme`'s colour: `from` and `to`
/// are the squares left and reached. The caller draws the pieces on those squares again over it,
/// then [`paint_last_move_arrow`].
pub(crate) fn paint_last_move(painter: &egui::Painter, theme: &BoardTheme, look: BoardLook, from: egui::Rect, to: egui::Rect) {
    let strength = look.last_move_strength();
    let fill = |r: egui::Rect, percent: u8| painter.rect_filled(r, 0.0, tint(theme.last_move.scaled(percent)));
    let stroke = egui::Stroke::new((from.width() * 0.08).max(1.5), last_move_ink(theme, strength));
    let border = |r: egui::Rect| painter.rect_stroke(r, 0.0, stroke, egui::StrokeKind::Inside);
    let corners = |r: egui::Rect| {
        let r = r.shrink(stroke.width / 2.0);
        let arm = r.width() * 0.3;
        for (corner, x, y) in [(r.left_top(), 1.0, 1.0), (r.right_top(), -1.0, 1.0), (r.left_bottom(), 1.0, -1.0), (r.right_bottom(), -1.0, -1.0)] {
            painter.add(egui::Shape::line(vec![corner + egui::vec2(x * arm, 0.0), corner, corner + egui::vec2(0.0, y * arm)], stroke));
        }
    };
    match look.last_move {
        LastMove::Fill => {
            fill(from, strength);
            fill(to, strength);
        }
        LastMove::Border => {
            border(from);
            border(to);
        }
        LastMove::Corners => {
            corners(from);
            corners(to);
        }
        LastMove::FromBorder => {
            border(from);
            fill(to, strength);
        }
        LastMove::BorderFill => {
            let light = u8::try_from(u16::from(strength) * 2 / 5).unwrap_or(u8::MAX);
            fill(from, light);
            fill(to, light);
            border(from);
            border(to);
        }
        LastMove::Arrow | LastMove::None => {}
    }
}

/// The last move's arrow, over the pieces, when the look marks it with one.
pub(crate) fn paint_last_move_arrow(painter: &egui::Painter, theme: &BoardTheme, look: BoardLook, from: egui::Rect, to: egui::Rect) {
    if look.last_move != LastMove::Arrow {
        return;
    }
    let (start, end) = (from.center(), to.center());
    let dir = (end - start).normalized();
    if !(dir.x.is_finite() && dir.y.is_finite()) {
        return;
    }
    let cell = from.width();
    let color = last_move_ink(theme, u8::try_from(u16::from(look.last_move_strength()) * 17 / 20).unwrap_or(u8::MAX));
    // Thinner than the arrows drawn by hand, so the two are told apart.
    let head = cell * 0.36;
    let tip = end - dir * cell * 0.14;
    let base = tip - dir * head;
    let normal = egui::vec2(-dir.y, dir.x);
    painter.line_segment([start + dir * cell * 0.2, base], egui::Stroke::new(cell * 0.11, color));
    painter.add(egui::Shape::convex_polygon(vec![tip, base + normal * head * 0.5, base - normal * head * 0.5], color, egui::Stroke::NONE));
}

fn tile(ui: &mut egui::Ui, t: &Tokens, theme: &BoardTheme, look: BoardLook, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(TILE, TILE), egui::Sense::click());
    let painter = ui.painter_at(rect);
    let fill = if response.hovered() { t.hover } else { t.card };
    painter.rect_filled(rect, f32::from(t.radius), fill);
    let ring = if selected || response.has_focus() { egui::Stroke::new(2.0, t.accent) } else { egui::Stroke::new(1.0, t.border) };
    painter.rect_stroke(rect.shrink(1.0), f32::from(t.radius), ring, egui::StrokeKind::Inside);
    let top = rect.center_top() + egui::vec2(-SQUARE, 6.0);
    let cell = |x: u8, y: u8| egui::Rect::from_min_size(top + egui::vec2(f32::from(x) * SQUARE, f32::from(y) * SQUARE), egui::vec2(SQUARE, SQUARE));
    for (x, y) in [(0u8, 0u8), (1, 0), (0, 1), (1, 1)] {
        painter.rect_filled(cell(x, y), 0.0, rgb(theme.square((x + y) % 2 == 0)));
    }
    // A move down the right-hand file, marked as the boards mark it.
    paint_last_move(&painter, theme, look, cell(1, 0), cell(1, 1));
    paint_last_move_arrow(&painter, theme, look, cell(1, 0), cell(1, 1));
    let name = name(theme);
    let font = egui::FontId::proportional(11.0);
    let line = crate::widgets::fit_line(name, TILE - 6.0, |s| crate::widgets::text_width(ui, s, &font));
    painter.text(rect.center_bottom() - egui::vec2(0.0, 6.0), egui::Align2::CENTER_BOTTOM, line, font, t.text);
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, selected, name));
    response
}

/// What the board's palette menu chose.
pub(crate) enum MenuPick {
    Theme(&'static BoardTheme),
    Look(BoardLook),
}

/// The board's palette menu: one item per theme, the current one checked, then the look's
/// choices.
pub(crate) fn menu_items(ui: &mut egui::Ui) -> Option<MenuPick> {
    let current_theme = current(ui.ctx());
    let mut pick = None;
    for theme in &THEMES {
        if ui.add(egui::Button::new(name(theme)).selected(theme.id == current_theme.id)).clicked() {
            pick = Some(MenuPick::Theme(theme));
            ui.close();
        }
    }
    ui.separator();
    let before = look(ui.ctx());
    let mut after = before;
    look_options(ui, &mut after);
    if after != before {
        pick = Some(MenuPick::Look(after));
    }
    pick
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_look_reads_back_and_refuses_what_it_does_not_know() {
        for last_move in LastMove::ALL {
            let look = BoardLook {
                coordinates: Coordinates::None,
                frame: Frame::Thin,
                squares: SquareMarks::Fill,
                turn_dot: true,
                last_move,
                last_move_strength: 45,
            };
            assert_eq!(BoardLook::from_json(&look.to_json()), look);
            assert_eq!(LastMove::from_token(last_move.token()), Some(last_move));
        }
        assert_eq!(BoardLook::from_json(&serde_json::json!({ "frame": "triple", "coordinates": 3, "last_move": "glow" })), BoardLook::default());
        let mut l = BoardLook::default();
        assert!(l.set("frame", "triple").is_err() && l.set("colour", "red").is_err() && l.set("last-move", "glow").is_err());
        for bad in ["5", "101", "", "fifty", "-20", "300"] {
            assert!(l.set("last-move-strength", bad).is_err(), "{bad}");
        }
        assert_eq!(l, BoardLook::default(), "nothing changed");
        l.set("last-move-strength", "10").unwrap();
        l.set("last-move", "corners").unwrap();
        assert_eq!((l.last_move, l.last_move_strength()), (LastMove::Corners, 10));
    }

    /// Settings kept before the last move had a look mark it as before; a strength out of range
    /// comes back within it.
    #[test]
    fn older_settings_mark_the_last_move_as_before() {
        let old = serde_json::json!({ "coordinates": "outside", "frame": "thin", "squares": "ring", "turn_dot": false });
        let look = BoardLook::from_json(&old);
        assert_eq!((look.last_move, look.last_move_strength()), (LastMove::Fill, 100));
        assert_eq!(look.coordinates, Coordinates::Outside);
        for (kept, read) in [(150u64, 100u8), (3, 10), (100_000, 100), (60, 60)] {
            let look = BoardLook::from_json(&serde_json::json!({ "last_move_strength": kept }));
            assert_eq!(look.last_move_strength, read, "{kept}");
        }
        assert_eq!(BoardLook { last_move_strength: 0, ..BoardLook::default() }.last_move_strength(), 10);
    }

    #[test]
    fn the_squares_fit_inside_what_the_board_was_given() {
        let outer = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(320.0, 320.0));
        assert_eq!(BoardLook::default().board_rect(outer), outer, "no margin by default");
        for coordinates in [Coordinates::Inside, Coordinates::Outside, Coordinates::None] {
            for frame in [Frame::None, Frame::Thin, Frame::Double] {
                let look = BoardLook { coordinates, frame, ..BoardLook::default() };
                let r = look.board_rect(outer);
                assert!(outer.contains_rect(r) && (r.width() - r.height()).abs() < 0.01, "{look:?}: {r:?}");
                if frame == Frame::Double {
                    assert!(outer.contains_rect(r.expand(6.0)), "{look:?}: the frame's lines stay inside");
                }
            }
        }
        // Even a tiny board keeps a positive size.
        let tiny = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(4.0, 4.0));
        assert!(BoardLook { coordinates: Coordinates::Outside, frame: Frame::Double, ..BoardLook::default() }.board_rect(tiny).width() > 0.0);
    }
}
