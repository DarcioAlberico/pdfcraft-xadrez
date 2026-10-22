//! Chess ▸ Diagrams ▸ Edit (blueprint S17): the selected diagram's position on an editable
//! board, with what to do with it, and the review queue.
//!
//! The board is drawn by the engine with the drawing the PDF replacement and the exports use
//! (`chess::draw`, the user's diagram font when one is installed) and shown as an image. A piece
//! from the palette goes where the board is clicked (the eraser empties the square); the move
//! number (read from the bold move number under the diagram when one is printed there), side to
//! move, castling and the view from Black's side complete the position, and the panel says at
//! once whether it is legal. Squares the model was unsure of are listed with its second reading,
//! one click away. From here the corrected position replaces the diagram on the page (one undo
//! step), gets a lichess link under it, is copied as FEN or opened on lichess.
//!
//! The review queue is the diagrams whose reading did not pass the gate (repaired ones
//! included), those the model could not read, and those not read at all: the panel can list only
//! them, and Previous / Next walk them in the one reading order.

use std::sync::Arc;

use egui::{Color32, TextureHandle};
use pdfcraft_engine::DocId;
use pdfcraft_engine::chess::pdfcraft_chess::draw::{BoardStyle, PieceSet};
use pdfcraft_engine::chess::pdfcraft_chess::fen::{Castling, Position};
use pdfcraft_engine::chess::pdfcraft_chess::placement::{Color, Piece, PieceKind, Placement, Square};
use pdfcraft_engine::chess::pdfcraft_chess::recognize::Uncertain;
use pdfcraft_engine::chess::pdfcraft_chess::repair::GameContext;
use pdfcraft_engine::chess::pdfcraft_chessvision::board::BoardReading;

use crate::theme::Tokens;

/// The board's side on screen, in points, and the image drawn for it, in pixels.
const BOARD_PT: f32 = 248.0;
const BOARD_PX: u32 = 496;

/// Highest move number the editor takes.
const MAX_MOVE_NUMBER: u32 = 999;

/// What a click on the board puts there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Brush {
    Put(Piece),
    Erase,
}

/// The diagram being edited.
pub struct DiagramEditor {
    pub doc: DocId,
    /// Page (0-based) and the diagram's index in the page's reading order.
    pub page: usize,
    pub index: usize,
    pub position: Position,
    /// The FEN as read, to go back to (`None`: the diagram was not read).
    pub original: Option<String>,
    pub uncertain: Vec<Uncertain>,
    pub flipped: bool,
    pub brush: Brush,
    texture: Option<(String, TextureHandle)>,
    /// Why the board could not be drawn, once.
    draw_error: Option<String>,
}

impl DiagramEditor {
    /// The editor for diagram `index` of `page`, from its reading (an empty board when it was not
    /// read). Black's point of view is kept from the reading.
    pub fn new(doc: DocId, page: usize, index: usize, reading: Option<&BoardReading>) -> DiagramEditor {
        let read = reading.and_then(BoardReading::position);
        let position = read
            .and_then(|p| pdfcraft_engine::chess::pdfcraft_chess::fen::parse(&p.fen).ok())
            .unwrap_or_else(|| Position { side_to_move: Some(Color::White), ..Position::from_placement(Placement::empty()) });
        DiagramEditor {
            doc,
            page,
            index,
            position,
            original: read.map(|p| p.fen.clone()),
            uncertain: read.map(|p| p.uncertain.clone()).unwrap_or_default(),
            flipped: read.and_then(|p| p.orientation).is_some_and(|o| o.black_point_of_view),
            brush: Brush::Put(Piece { color: Color::White, kind: PieceKind::Queen }),
            texture: None,
            draw_error: None,
        }
    }

    /// The position as a full FEN (White to move when the side is unknown).
    pub fn fen(&self) -> String {
        self.position.fen()
    }

    /// Whether the position is legal: one king each, nothing impossible, the side not to move not
    /// in check, castling rights that fit.
    pub fn legal(&self) -> bool {
        GameContext::from_fen(&self.fen()).is_some()
    }

    /// Put the brush on `square`; clicking a square that already holds the brush's piece empties it.
    pub fn paint(&mut self, square: Square) {
        let now = self.position.placement.get(square);
        let piece = match self.brush {
            Brush::Put(p) if now == Some(p) => None,
            Brush::Put(p) => Some(p),
            Brush::Erase => None,
        };
        self.position.placement.set(square, piece);
        self.uncertain.retain(|u| u.square != square);
    }

    /// The square at `(x, y)` in the board's own units (0..1 from its top left), as shown.
    pub fn square_at(&self, x: f32, y: f32) -> Option<Square> {
        if !(x.is_finite() && y.is_finite() && (0.0..1.0).contains(&x) && (0.0..1.0).contains(&y)) {
            return None;
        }
        // In 0..8 after the checks above, so the casts cannot wrap.
        let (col, row) = ((x * 8.0) as u8, (y * 8.0) as u8);
        if self.flipped { Square::new(7 - col, row) } else { Square::new(col, 7 - row) }
    }

    /// Back to the reading.
    pub fn reset(&mut self) {
        if let Some(fen) = &self.original
            && let Ok(p) = pdfcraft_engine::chess::pdfcraft_chess::fen::parse(fen)
        {
            self.position = p;
        }
    }
}

/// What the editor asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum EditorAction {
    Replace,
    Link,
    CopyFen(String),
    OpenLichess(String),
    Previous,
    Next,
    Close,
    /// Put the position on the chess board.
    OpenBoard,
}

pub(crate) fn piece_letter(p: Piece) -> String {
    let c = match p.kind {
        PieceKind::King => 'K',
        PieceKind::Queen => 'Q',
        PieceKind::Rook => 'R',
        PieceKind::Bishop => 'B',
        PieceKind::Knight => 'N',
        PieceKind::Pawn => 'P',
    };
    if p.color == Color::White { c.to_string() } else { c.to_ascii_lowercase().to_string() }
}

pub(crate) fn piece_name(p: Piece) -> &'static str {
    match (p.color, p.kind) {
        (Color::White, PieceKind::King) => tl!("White king"),
        (Color::White, PieceKind::Queen) => tl!("White queen"),
        (Color::White, PieceKind::Rook) => tl!("White rook"),
        (Color::White, PieceKind::Bishop) => tl!("White bishop"),
        (Color::White, PieceKind::Knight) => tl!("White knight"),
        (Color::White, PieceKind::Pawn) => tl!("White pawn"),
        (Color::Black, PieceKind::King) => tl!("Black king"),
        (Color::Black, PieceKind::Queen) => tl!("Black queen"),
        (Color::Black, PieceKind::Rook) => tl!("Black rook"),
        (Color::Black, PieceKind::Bishop) => tl!("Black bishop"),
        (Color::Black, PieceKind::Knight) => tl!("Black knight"),
        (Color::Black, PieceKind::Pawn) => tl!("Black pawn"),
    }
}

const KINDS: [PieceKind; 6] = [PieceKind::King, PieceKind::Queen, PieceKind::Rook, PieceKind::Bishop, PieceKind::Knight, PieceKind::Pawn];

/// The board image for the editor's position, redrawn when the position or the view changes.
fn board_texture(ui: &egui::Ui, ed: &mut DiagramEditor, pieces: &PieceSet) -> Option<TextureHandle> {
    let key = format!("{}|{}", ed.fen(), ed.flipped);
    if let Some((k, tex)) = &ed.texture
        && *k == key
    {
        return Some(tex.clone());
    }
    let style = BoardStyle { border: 0.0, coordinates: false, flipped: ed.flipped, ..BoardStyle::default() };
    match pdfcraft_engine::chess::office::diagram_rgba(&ed.position.placement, pieces, &style, BOARD_PX) {
        Ok((w, h, rgba)) => {
            let (w, h) = (usize::try_from(w).ok()?, usize::try_from(h).ok()?);
            if rgba.len() != w.checked_mul(h)?.checked_mul(4)? {
                return None;
            }
            let image = egui::ColorImage::from_rgba_premultiplied([w, h], &rgba);
            let tex = ui.ctx().load_texture("chess-editor-board", image, egui::TextureOptions::LINEAR);
            ed.texture = Some((key, tex.clone()));
            ed.draw_error = None;
            Some(tex)
        }
        Err(e) => {
            ed.draw_error = Some(e);
            None
        }
    }
}

/// The editor: the board, the palette, the position's fields and the actions. `queue` is the
/// number of diagrams to review (for Previous / Next).
pub fn editor_ui(ui: &mut egui::Ui, t: &Tokens, ed: &mut DiagramEditor, pieces: &PieceSet, queue: usize) -> Option<EditorAction> {
    let mut action = None;
    ui.horizontal(|ui| {
        let title = crate::i18n::fmt(tl!("Diagram {n}"), &[("n", &(ed.index + 1).to_string())]);
        ui.label(egui::RichText::new(crate::i18n::fmt(tl!("Edit {d}, page {p}"), &[("d", &title), ("p", &(ed.page + 1).to_string())])).strong());
        if ui.small_button("✕").on_hover_text(tl!("Close the editor")).clicked() {
            action = Some(EditorAction::Close);
        }
    });
    // The board.
    if let Some(tex) = board_texture(ui, ed, pieces) {
        let response = ui.add(egui::Image::new(&tex).fit_to_exact_size(egui::vec2(BOARD_PT, BOARD_PT)).sense(egui::Sense::click()));
        let response = response.on_hover_text(tl!("Click a square to put the chosen piece there; click it again to empty it."));
        // Its accessible name: what the board is, and the position on it.
        let board_name = crate::i18n::fmt(tl!("Board: {fen}"), &[("fen", &ed.fen())]);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, &board_name));
        if response.clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            let r = response.rect;
            if let Some(sq) = ed.square_at((pos.x - r.min.x) / r.width().max(1.0), (pos.y - r.min.y) / r.height().max(1.0)) {
                ed.paint(sq);
            }
        }
        // The squares the model was unsure of, outlined.
        for u in &ed.uncertain {
            let (file, rank) = (f32::from(u.square.file()), f32::from(u.square.rank()));
            let (col, row) = if ed.flipped { (7.0 - file, rank) } else { (file, 7.0 - rank) };
            let cell = response.rect.width() / 8.0;
            let sq = egui::Rect::from_min_size(response.rect.min + egui::vec2(col * cell, row * cell), egui::vec2(cell, cell));
            ui.painter().rect_stroke(sq.shrink(1.0), 1.0, egui::Stroke::new(2.0, Color32::from_rgb(0xE0, 0x8A, 0x1A)), egui::StrokeKind::Inside);
        }
    } else if let Some(e) = &ed.draw_error {
        ui.label(egui::RichText::new(crate::i18n::fmt(tl!("The board couldn't be drawn: {e}"), &[("e", e)])).small().color(t.text_muted));
    }
    // The palette: white pieces, black pieces, the eraser.
    for color in [Color::White, Color::Black] {
        ui.horizontal(|ui| {
            for kind in KINDS {
                let p = Piece { color, kind };
                let on = ed.brush == Brush::Put(p);
                if ui.selectable_label(on, egui::RichText::new(piece_letter(p)).monospace().strong()).on_hover_text(piece_name(p)).clicked() {
                    ed.brush = Brush::Put(p);
                }
            }
            if color == Color::Black && ui.selectable_label(ed.brush == Brush::Erase, tl!("Erase")).clicked() {
                ed.brush = Brush::Erase;
            }
        });
    }
    // Move number, side to move, castling, view. The side to move has a row of its own, so its
    // label and its two choices stay together when the panel is narrow.
    ui.horizontal_wrapped(|ui| {
        ui.label(tl!("Move number:"));
        ui.add(egui::DragValue::new(&mut ed.position.fullmove_number).range(1..=MAX_MOVE_NUMBER).speed(0.2))
            .on_hover_text(tl!("The move the diagram stands at: 35 with Black to move is 35…"));
    });
    ui.horizontal_wrapped(|ui| {
        let mut side = ed.position.side_to_move.unwrap_or(Color::White);
        ui.label(tl!("To move:"));
        crate::widgets::radio_value(ui, &mut side, Color::White, tl!("White"));
        crate::widgets::radio_value(ui, &mut side, Color::Black, tl_ctx!("chess side", "Black"));
        ed.position.side_to_move = Some(side);
    });
    ui.horizontal_wrapped(|ui| {
        ui.label(tl!("Castling:"));
        let c: &mut Castling = &mut ed.position.castling;
        ui.add(crate::widgets::checkbox(&mut c.white_king_side, "K"));
        ui.add(crate::widgets::checkbox(&mut c.white_queen_side, "Q"));
        ui.add(crate::widgets::checkbox(&mut c.black_king_side, "k"));
        ui.add(crate::widgets::checkbox(&mut c.black_queen_side, "q"));
        if ui.small_button(tl!("Flip board")).clicked() {
            ed.flipped = !ed.flipped;
        }
        if ed.original.is_some() && ui.small_button(tl!("Back to the reading")).clicked() {
            ed.reset();
        }
    });
    let fen = ed.fen();
    let legal = ed.legal();
    ui.add(egui::Label::new(egui::RichText::new(&fen).monospace().small()).wrap());
    ui.label(egui::RichText::new(if legal { tl!("Legal position") } else { tl!("Not a legal position") }).small().color(if legal {
        Color32::from_rgb(0x2D, 0x9D, 0x5B)
    } else {
        Color32::from_rgb(0xE0, 0x3E, 0x3E)
    }));
    // The model's second readings.
    let mut take = None;
    for (k, u) in ed.uncertain.iter().enumerate() {
        ui.horizontal(|ui| {
            let now = ed.position.placement.get(u.square).map_or_else(|| "·".to_owned(), piece_letter);
            let second = u.alternative.map_or_else(|| "·".to_owned(), piece_letter);
            ui.label(
                egui::RichText::new(format!("{}: {now} {:.0}% · {second} {:.0}%", u.square, u.confidence * 100.0, u.alternative_confidence * 100.0))
                    .small()
                    .monospace(),
            );
            if ui.small_button(tl!("Use the second reading")).clicked() {
                take = Some(k);
            }
        });
    }
    if let Some(u) = take.and_then(|k| ed.uncertain.get(k).cloned()) {
        ed.position.placement.set(u.square, u.alternative);
        ed.uncertain.retain(|x| x.square != u.square);
    }
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        if ui.button(tl!("Replace on the page")).on_hover_text(tl!("Draw this position over the diagram on the page (one undo step).")).clicked() {
            action = Some(EditorAction::Replace);
        }
        if ui
            .add_enabled(legal, egui::Button::new(tl!("Add lichess link")))
            .on_hover_text(tl!("Put a link to this position on lichess under the diagram."))
            .clicked()
        {
            action = Some(EditorAction::Link);
        }
        if ui.small_button(tl!("Copy FEN")).clicked() {
            action = Some(EditorAction::CopyFen(fen.clone()));
        }
        if legal && let Ok(p) = pdfcraft_engine::chess::pdfcraft_chess::fen::parse(&fen) {
            let url = pdfcraft_engine::chess::pdfcraft_chess::lichess::analysis_url(&p, if ed.flipped { Color::Black } else { Color::White });
            if ui.small_button(tl!("Open in lichess")).on_hover_text(&url).clicked() {
                action = Some(EditorAction::OpenLichess(url));
            }
        }
        if ui.small_button(tl!("Open on the board")).clicked() {
            action = Some(EditorAction::OpenBoard);
        }
    });
    ui.horizontal_wrapped(|ui| {
        ui.add_enabled_ui(queue > 0, |ui| {
            if ui.small_button(tl!("◀ Previous to review")).clicked() {
                action = Some(EditorAction::Previous);
            }
            if ui.small_button(tl!("Next to review ▶")).clicked() {
                action = Some(EditorAction::Next);
            }
        });
        let left = egui::RichText::new(crate::i18n::fmt(tl!("{n} to review"), &[("n", &queue.to_string())])).small().color(t.text_muted);
        ui.add(egui::Label::new(left).wrap_mode(egui::TextWrapMode::Extend));
    });
    action
}

/// Whether a diagram belongs in the review queue: not read, not readable, or read without
/// passing the gate (repaired readings never pass).
pub fn needs_review(reading: Option<&BoardReading>) -> bool {
    !reading.is_some_and(BoardReading::accepted)
}

/// The piece set the editor draws with, found once.
pub fn piece_set(slot: &mut Option<Arc<PieceSet>>) -> Arc<PieceSet> {
    slot.get_or_insert_with(|| Arc::new(pdfcraft_engine::chess::pdfcraft_chessvision::pieces::find().0)).clone()
}

/// The piece set the analysis board draws with, found once.
pub fn board_piece_set(slot: &mut Option<Arc<PieceSet>>) -> Arc<PieceSet> {
    slot.get_or_insert_with(|| Arc::new(pdfcraft_engine::chess::pdfcraft_chessvision::pieces::find_for_board().0)).clone()
}
