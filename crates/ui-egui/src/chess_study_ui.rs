//! Chess ▸ Study board: the analysis board as a tab of the Chess panel, beside Diagrams and Text,
//! with what a study board adds to it (in the manner of chessvision.ai's board, from its public
//! pages; nothing of it is used):
//! - **Set up a position**: pieces from a palette onto the board (a click puts the chosen piece,
//!   a click on the same piece or a right-click empties the square), the start position or an
//!   empty board, side to move and castling; Done plays from it as a new game.
//! - **Drawings**: arrows and circles drawn with the right button (`chess_board_ui`), kept for
//!   each position.
//! - **Engine analysis**: the user's own UCI engine (`engine::chess::analysis`) analyses the
//!   position shown, with an evaluation bar beside the board, the best lines (a click on one of
//!   their moves plays the line up to it) and the best move as an arrow. None ships with
//!   PdfCraft; the desktop app finds one or the user picks one, and the web build has none.
//! - **Material**: what each side is ahead by, as pieces and points.
//!
//! The board, moves, variations, annotations, game details and FEN/PGN are the analysis board's
//! own parts, on the same game: the Board panel and this tab are two views of one board.

#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;

use egui::{Color32, TextureHandle, vec2};
#[cfg(not(target_arch = "wasm32"))]
use egui::{Rect, Stroke};
use pdfcraft_engine::chess::pdfcraft_chess::draw::{BoardStyle, PieceSet};
use pdfcraft_engine::chess::pdfcraft_chess::fen::Position;
use pdfcraft_engine::chess::pdfcraft_chess::game::Game;
use pdfcraft_engine::chess::pdfcraft_chess::placement::{Color, Piece, PieceKind, Placement, Square};
#[cfg(not(target_arch = "wasm32"))]
use pdfcraft_engine::chess::pdfcraft_chess::uci::Score;

#[cfg(not(target_arch = "wasm32"))]
use crate::chess_board_ui::MarkColor;
use crate::chess_board_ui::{self, BoardAction, BoardState, Mark};
use crate::chess_editor::Brush;
use crate::theme::Tokens;

/// The set-up board's image, in pixels.
const SETUP_PX: u32 = 560;

/// Room left beside the board for the panel's scroll bar, which floats over its content.
const SCROLL_BAR_ROOM: f32 = 10.0;

/// Width of the evaluation bar, in points.
#[cfg(not(target_arch = "wasm32"))]
const BAR_PT: f32 = 14.0;

/// The pieces in the palette's order.
const KINDS: [PieceKind; 6] = [PieceKind::King, PieceKind::Queen, PieceKind::Rook, PieceKind::Bishop, PieceKind::Knight, PieceKind::Pawn];

/// A position being set up.
pub struct Setup {
    pub position: Position,
    pub brush: Brush,
    texture: Option<(String, TextureHandle)>,
    draw_error: Option<String>,
}

impl Setup {
    /// Set-up from `fen` (or its placement alone, for a position that is not legal); an empty
    /// board with White to move when it cannot be read.
    pub fn from_fen(fen: &str) -> Setup {
        let parse = pdfcraft_engine::chess::pdfcraft_chess::fen::parse;
        let mut position = parse(fen)
            .or_else(|_| pdfcraft_engine::chess::pdfcraft_chess::fen::parse_placement(fen).map(Position::from_placement))
            .unwrap_or_else(|_| Position::from_placement(Placement::empty()));
        position.side_to_move.get_or_insert(Color::White);
        position.en_passant = None;
        Setup { position, brush: Brush::Put(Piece { color: Color::White, kind: PieceKind::Queen }), texture: None, draw_error: None }
    }

    /// The position as a FEN.
    pub fn fen(&self) -> String {
        self.position.fen()
    }

    /// Whether the position can be played: one king each, the side not to move not in check,
    /// castling rights that fit.
    pub fn legal(&self) -> bool {
        Game::from_fen(&self.fen()).is_ok()
    }

    /// Put the brush on `square`; a click on a square holding the brush's piece empties it.
    pub fn paint(&mut self, square: Square) {
        let now = self.position.placement.get(square);
        let piece = match self.brush {
            Brush::Put(p) if now == Some(p) => None,
            Brush::Put(p) => Some(p),
            Brush::Erase => None,
        };
        self.set(square, piece);
    }

    /// Put `piece` on `square` (or empty it), then drop the castling rights that no longer fit.
    pub fn set(&mut self, square: Square, piece: Option<Piece>) {
        self.position.placement.set(square, piece);
        self.fit_castling();
    }

    /// Replace the whole placement (the start position, an empty board).
    pub fn set_placement(&mut self, placement: Placement, castling_all: bool) {
        self.position.placement = placement;
        let c = &mut self.position.castling;
        (c.white_king_side, c.white_queen_side, c.black_king_side, c.black_queen_side) = (castling_all, castling_all, castling_all, castling_all);
        self.fit_castling();
    }

    /// Castling needs the king and that rook on their first squares.
    fn fit_castling(&mut self) {
        let p = self.position.placement;
        let on = |name: &str, color: Color, kind: PieceKind| Square::parse(name).is_some_and(|sq| p.get(sq) == Some(Piece { color, kind }));
        let c = &mut self.position.castling;
        let white_king = on("e1", Color::White, PieceKind::King);
        let black_king = on("e8", Color::Black, PieceKind::King);
        c.white_king_side &= white_king && on("h1", Color::White, PieceKind::Rook);
        c.white_queen_side &= white_king && on("a1", Color::White, PieceKind::Rook);
        c.black_king_side &= black_king && on("h8", Color::Black, PieceKind::Rook);
        c.black_queen_side &= black_king && on("a8", Color::Black, PieceKind::Rook);
    }
}

/// The Study board tab's own state (the board and game are `ChessState::board`).
pub struct StudyState {
    /// A position being set up, in place of the game.
    pub setup: Option<Setup>,
    /// Engine analysis is on.
    pub analysis: bool,
    /// Lines the engine shows, 1–3.
    pub lines: u32,
    /// The engine running, the program the user picked, and why the last one did not start.
    #[cfg(not(target_arch = "wasm32"))]
    engine: Option<pdfcraft_engine::chess::analysis::Analyser>,
    #[cfg(not(target_arch = "wasm32"))]
    pub engine_path: Option<PathBuf>,
    #[cfg(not(target_arch = "wasm32"))]
    start_error: Option<pdfcraft_engine::chess::analysis::AnalysisError>,
    /// No engine was found the last time one was looked for.
    no_engine: bool,
    /// The position last given to the engine.
    sent: Option<String>,
}

impl Default for StudyState {
    fn default() -> StudyState {
        StudyState {
            setup: None,
            analysis: false,
            lines: 3,
            #[cfg(not(target_arch = "wasm32"))]
            engine: None,
            #[cfg(not(target_arch = "wasm32"))]
            engine_path: None,
            #[cfg(not(target_arch = "wasm32"))]
            start_error: None,
            no_engine: false,
            sent: None,
        }
    }
}

/// What the tab asks the app for.
#[derive(Clone, Debug, PartialEq)]
pub enum StudyAction {
    Board(BoardAction),
    /// Pick the engine program (desktop only).
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    ChooseEngine,
}

impl StudyState {
    /// The tab is not shown: the engine stops searching (it starts again when the tab is back).
    pub fn pause(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(engine) = &self.engine
            && self.sent.is_some()
        {
            engine.stop();
        }
        self.sent = None;
    }

    /// Close the engine and look for one again at the next frame (another program was picked,
    /// or analysis was switched).
    pub fn restart(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.engine = None;
            self.start_error = None;
        }
        self.no_engine = false;
        self.sent = None;
    }

    /// Start the engine when analysis is on, close it when off, and give it the position shown.
    #[cfg(not(target_arch = "wasm32"))]
    fn sync_engine(&mut self, ctx: &egui::Context, board: &BoardState) {
        use pdfcraft_engine::chess::analysis::{Analyser, AnalysisOptions, DEFAULT_DEPTH, find_engine};
        if !self.analysis {
            self.engine = None;
            self.sent = None;
            return;
        }
        let options = AnalysisOptions { lines: self.lines, depth: DEFAULT_DEPTH };
        if self.engine.is_none() && self.start_error.is_none() && !self.no_engine {
            match self.engine_path.clone().or_else(find_engine) {
                None => self.no_engine = true,
                Some(path) => {
                    let ctx = ctx.clone();
                    match Analyser::start(&path, options, Arc::new(move || ctx.request_repaint())) {
                        Ok(engine) => self.engine = Some(engine),
                        Err(e) => self.start_error = Some(e),
                    }
                }
            }
        }
        let Some(engine) = &self.engine else { return };
        engine.set_options(options);
        let want = (self.setup.is_none() && board.illegal.is_none()).then(|| board.game.fen());
        if want != self.sent {
            match &want {
                Some(fen) => engine.analyse(fen),
                None => engine.stop(),
            }
            self.sent = want;
        }
    }

    /// What the engine found for the position shown, when it is analysing it.
    #[cfg(not(target_arch = "wasm32"))]
    fn snapshot(&self, board: &BoardState) -> Option<pdfcraft_engine::chess::analysis::Snapshot> {
        let snap = self.engine.as_ref()?.snapshot();
        let fen = board.game.fen();
        Some(snap).filter(|s| board.illegal.is_none() && s.fen.as_deref().is_none_or(|f| f == fen))
    }
}

/// The set-up board's image, redrawn when the pieces or the view change.
fn setup_texture(ctx: &egui::Context, setup: &mut Setup, flipped: bool, pieces: &PieceSet) -> Option<TextureHandle> {
    let key = format!("{}|{flipped}", setup.fen());
    if let Some((k, tex)) = &setup.texture
        && *k == key
    {
        return Some(tex.clone());
    }
    let style = BoardStyle { border: 0.0, coordinates: true, flipped, ..BoardStyle::default() };
    match pdfcraft_engine::chess::office::diagram_rgba(&setup.position.placement, pieces, &style, SETUP_PX) {
        Ok((w, h, rgba)) => {
            let (w, h) = (usize::try_from(w).ok()?, usize::try_from(h).ok()?);
            if rgba.len() != w.checked_mul(h)?.checked_mul(4)? {
                return None;
            }
            let tex = ctx.load_texture("chess-study-setup", egui::ColorImage::from_rgba_premultiplied([w, h], &rgba), egui::TextureOptions::LINEAR);
            setup.texture = Some((key, tex.clone()));
            setup.draw_error = None;
            Some(tex)
        }
        Err(e) => {
            setup.draw_error = Some(e);
            None
        }
    }
}

/// A palette button: the piece's picture when the pieces could be cut out, else its letter.
fn palette_button(ui: &mut egui::Ui, sprites: Option<&TextureHandle>, piece: Piece, on: bool) -> egui::Response {
    let name = crate::chess_editor::piece_name(piece);
    let response = match sprites {
        Some(tex) => ui.add(
            egui::Button::image(egui::Image::new(tex).uv(chess_board_ui::sprite_uv(piece)).fit_to_exact_size(vec2(24.0, 24.0)))
                .selected(on)
                .min_size(vec2(30.0, 30.0)),
        ),
        None => ui.selectable_label(on, egui::RichText::new(crate::chess_editor::piece_letter(piece)).monospace().strong()),
    };
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, name));
    response.on_hover_text(name)
}

/// The set-up view: its board, the palette and the position's fields. Whether it is finished
/// (Done or Cancel).
fn setup_view(ui: &mut egui::Ui, t: &Tokens, setup: &mut Setup, board: &mut BoardState, pieces: &PieceSet) -> bool {
    let mut finished = false;
    ui.label(egui::RichText::new(tl!("Set up a position")).strong());
    let side = (ui.available_width() - SCROLL_BAR_ROOM).clamp(160.0, 480.0);
    if let Some(tex) = setup_texture(ui.ctx(), setup, board.flipped, pieces) {
        let response = ui.add(egui::Image::new(&tex).fit_to_exact_size(vec2(side, side)).sense(egui::Sense::click()));
        let name = crate::i18n::fmt(tl!("Board: {fen}"), &[("fen", &setup.fen())]);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, &name));
        let r = response.rect;
        let at = |p: egui::Pos2| board.square_at((p.x - r.min.x) / r.width().max(1.0), (p.y - r.min.y) / r.height().max(1.0));
        if response.clicked()
            && let Some(sq) = response.interact_pointer_pos().and_then(at)
        {
            setup.paint(sq);
        } else if response.secondary_clicked()
            && let Some(sq) = response.interact_pointer_pos().and_then(at)
        {
            setup.set(sq, None);
        }
        response.on_hover_text(tl!("Click a square to put the chosen piece there; click it again, or right-click it, to empty it."));
    } else if let Some(e) = &setup.draw_error {
        ui.label(egui::RichText::new(crate::i18n::fmt(tl!("The board couldn't be drawn: {e}"), &[("e", e)])).small().color(t.text_muted));
    }
    // The palette: White's pieces, Black's, and the eraser.
    let sprites = board.sprites(ui.ctx(), pieces);
    for color in [Color::White, Color::Black] {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for kind in KINDS {
                let piece = Piece { color, kind };
                if palette_button(ui, sprites.as_ref(), piece, setup.brush == Brush::Put(piece)).clicked() {
                    setup.brush = Brush::Put(piece);
                }
            }
            if color == Color::Black && ui.selectable_label(setup.brush == Brush::Erase, tl!("Erase")).clicked() {
                setup.brush = Brush::Erase;
            }
        });
    }
    ui.horizontal_wrapped(|ui| {
        if ui.small_button(tl!("Standard start position")).clicked() {
            setup.set_placement(Placement::standard(), true);
            setup.position.side_to_move = Some(Color::White);
        }
        if ui.small_button(tl!("Empty board")).clicked() {
            setup.set_placement(Placement::empty(), false);
        }
        if ui.small_button(tl!("Flip board")).clicked() {
            board.flipped = !board.flipped;
        }
    });
    ui.horizontal_wrapped(|ui| {
        let mut side = setup.position.side_to_move.unwrap_or(Color::White);
        ui.label(tl!("To move:"));
        crate::widgets::radio_value(ui, &mut side, Color::White, tl!("White"));
        crate::widgets::radio_value(ui, &mut side, Color::Black, tl_ctx!("chess side", "Black"));
        setup.position.side_to_move = Some(side);
    });
    ui.horizontal_wrapped(|ui| {
        ui.label(tl!("Castling:"));
        let c = &mut setup.position.castling;
        ui.add(crate::widgets::checkbox(&mut c.white_king_side, "K"));
        ui.add(crate::widgets::checkbox(&mut c.white_queen_side, "Q"));
        ui.add(crate::widgets::checkbox(&mut c.black_king_side, "k"));
        ui.add(crate::widgets::checkbox(&mut c.black_queen_side, "q"));
    });
    // A right a box was ticked for that the pieces do not allow is taken off again.
    setup.fit_castling();
    let fen = setup.fen();
    let legal = setup.legal();
    ui.add(egui::Label::new(egui::RichText::new(&fen).monospace().small()).wrap());
    let (text, colour) = if legal {
        (tl!("Legal position"), Color32::from_rgb(0x2D, 0x9D, 0x5B))
    } else {
        (tl!("Not a legal position"), Color32::from_rgb(0xE0, 0x3E, 0x3E))
    };
    ui.label(egui::RichText::new(text).small().color(colour));
    ui.horizontal(|ui| {
        if ui.add_enabled(legal, egui::Button::new(tl!("Done"))).on_hover_text(tl!("Play from this position, as a new game.")).clicked() {
            board.load_fen(&fen, board.flipped, None);
            finished = true;
        }
        if ui.button(tl!("Cancel")).clicked() {
            finished = true;
        }
    });
    finished
}

/// The material each side is ahead by: for each kind, the difference in count.
pub fn material(placement: &Placement) -> [(PieceKind, i32); 5] {
    [PieceKind::Queen, PieceKind::Rook, PieceKind::Bishop, PieceKind::Knight, PieceKind::Pawn].map(|kind| {
        let count = |color| i32::try_from(placement.count(Piece { color, kind })).unwrap_or(i32::MAX);
        (kind, count(Color::White).saturating_sub(count(Color::Black)))
    })
}

/// A piece's worth in pawns, as material is usually counted.
fn points(kind: PieceKind) -> i32 {
    match kind {
        PieceKind::Queen => 9,
        PieceKind::Rook => 5,
        PieceKind::Bishop | PieceKind::Knight => 3,
        PieceKind::Pawn => 1,
        PieceKind::King => 0,
    }
}

/// White's material lead in pawns (negative when Black leads).
pub fn material_lead(placement: &Placement) -> i32 {
    material(placement).iter().fold(0i32, |sum, (kind, d)| sum.saturating_add(d.saturating_mul(points(*kind))))
}

/// The material line: for each side, the opponent's pieces it is ahead by, and its lead.
fn material_view(ui: &mut egui::Ui, t: &Tokens, board: &mut BoardState, pieces: &PieceSet) {
    let placement = board.placement();
    let diff = material(&placement);
    if diff.iter().all(|(_, d)| *d == 0) {
        return;
    }
    let sprites = board.sprites(ui.ctx(), pieces);
    let lead = material_lead(&placement);
    for (side, label) in [(Color::White, tl!("White")), (Color::Black, tl_ctx!("chess side", "Black"))] {
        let ahead: Vec<(PieceKind, i32)> =
            diff.iter().map(|(k, d)| (*k, if side == Color::White { *d } else { d.saturating_neg() })).filter(|(_, d)| *d > 0).collect();
        if ahead.is_empty() {
            continue;
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 1.0;
            ui.label(egui::RichText::new(label).small().color(t.text_muted));
            ui.add_space(4.0);
            // The pieces taken are the other side's.
            let taken = if side == Color::White { Color::Black } else { Color::White };
            for (kind, n) in ahead {
                for _ in 0..n.min(16) {
                    let piece = Piece { color: taken, kind };
                    match &sprites {
                        Some(tex) => {
                            ui.add(egui::Image::new(tex).uv(chess_board_ui::sprite_uv(piece)).fit_to_exact_size(vec2(16.0, 16.0)));
                        }
                        None => {
                            ui.label(egui::RichText::new(crate::chess_editor::piece_letter(piece)).small().monospace());
                        }
                    }
                }
            }
            let mine = if side == Color::White { lead } else { lead.saturating_neg() };
            if mine > 0 {
                ui.add_space(4.0);
                ui.label(egui::RichText::new(format!("+{mine}")).small().strong());
            }
        });
    }
}

/// The evaluation bar beside a board `height` points high: White's share from the bottom (from
/// the top when the board is flipped), the score at White's end.
#[cfg(not(target_arch = "wasm32"))]
fn eval_bar(ui: &mut egui::Ui, score: Option<Score>, flipped: bool, height: f32) {
    let (rect, response) = ui.allocate_exact_size(vec2(BAR_PT, height), egui::Sense::hover());
    let share = score.map_or(0.5, Score::white_share).clamp(0.0, 1.0);
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, Color32::from_gray(64));
    let white_h = rect.height() * share;
    let white = if flipped {
        Rect::from_min_size(rect.min, vec2(rect.width(), white_h))
    } else {
        Rect::from_min_max(egui::pos2(rect.min.x, rect.max.y - white_h), rect.max)
    };
    painter.rect_filled(white, 2.0, Color32::from_gray(240));
    painter.rect_stroke(rect, 2.0, Stroke::new(1.0, Color32::from_gray(120)), egui::StrokeKind::Inside);
    if let Some(score) = score {
        let text = score.text();
        response.on_hover_text(&text).widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &text));
    }
}

/// `sans` as move-list labels from `fen`'s move number and side: `12.` before White's moves, and
/// `12...` before a first move by Black.
#[cfg(not(target_arch = "wasm32"))]
fn numbered_moves(fen: &str, sans: &[String]) -> Vec<(Option<String>, String)> {
    let mut fields = fen.split_ascii_whitespace().skip(1);
    let mut black = fields.next() == Some("b");
    let mut number: u32 = fields.nth(3).and_then(|n| n.parse().ok()).filter(|n| *n > 0).unwrap_or(1);
    let mut out = Vec::with_capacity(sans.len());
    for (i, san) in sans.iter().enumerate() {
        let label = if !black {
            Some(format!("{number}."))
        } else if i == 0 {
            Some(format!("{number}..."))
        } else {
            None
        };
        out.push((label, san.clone()));
        if black {
            number = number.saturating_add(1);
        }
        black = !black;
    }
    out
}

/// The engine's part: the switch, the engine's state and its lines. A click on a move of a line
/// plays the line up to that move.
#[cfg(not(target_arch = "wasm32"))]
fn engine_view(ui: &mut egui::Ui, t: &Tokens, study: &mut StudyState, board: &mut BoardState) -> Option<StudyAction> {
    use pdfcraft_engine::chess::analysis::AnalysisError;
    let mut action = None;
    ui.horizontal_wrapped(|ui| {
        let was = study.analysis;
        ui.add(crate::widgets::checkbox(&mut study.analysis, tl!("Engine analysis")));
        if study.analysis != was {
            study.restart();
        }
        if study.analysis {
            ui.label(egui::RichText::new(tl!("Lines:")).small());
            for n in 1..=3u32 {
                if ui.selectable_label(study.lines == n, n.to_string()).clicked() {
                    study.lines = n;
                }
            }
        }
        if ui.small_button(tl!("Choose engine…")).on_hover_text(tl!("Pick a UCI chess engine program, such as Stockfish.")).clicked() {
            action = Some(StudyAction::ChooseEngine);
        }
    });
    if !study.analysis {
        return action;
    }
    let muted = |ui: &mut egui::Ui, text: &str| ui.label(egui::RichText::new(text).small().color(t.text_muted));
    if study.no_engine {
        muted(
            ui,
            tl!(
                "No chess engine found. Set PDFCRAFT_CHESS_ENGINE to a UCI engine such as Stockfish, put one in an engine folder beside PdfCraft, or choose one."
            ),
        );
        return action;
    }
    let error = study.start_error.clone().or_else(|| study.engine.as_ref().and_then(|e| e.snapshot().error));
    if let Some(e) = error {
        let text = match e {
            AnalysisError::Start(why) => crate::i18n::fmt(tl!("The engine couldn't be started: {e}"), &[("e", &why)]),
            AnalysisError::NotUci => tl!("That program didn't answer as a UCI chess engine.").to_string(),
            AnalysisError::Stopped => tl!("The engine stopped.").to_string(),
        };
        ui.label(egui::RichText::new(text).small().color(Color32::from_rgb(0xE0, 0x3E, 0x3E)));
        return action;
    }
    let Some(snap) = study.snapshot(board) else { return action };
    ui.horizontal_wrapped(|ui| {
        if snap.searching || !snap.ready {
            ui.spinner();
        }
        let mut status = snap.name.clone().unwrap_or_else(|| tl!("Starting the engine…").to_string());
        if let Some(depth) = snap.lines.first().map(|l| l.depth) {
            status.push_str(" · ");
            status.push_str(&crate::i18n::fmt(tl!("depth {n}"), &[("n", &depth.to_string())]));
        }
        if let Some(nps) = snap.nps.filter(|n| *n > 0) {
            status.push_str(&format!(" · {} kN/s", nps / 1000));
        }
        muted(ui, &status);
    });
    let fen = board.game.fen();
    if board.game.legal_moves().is_empty() {
        muted(ui, tl!("No moves to analyse in this position."));
        return action;
    }
    let mut play = None;
    for line in &snap.lines {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let colour = if line.score.white_share() >= 0.5 { Color32::from_gray(30) } else { Color32::from_gray(235) };
            let fill = if line.score.white_share() >= 0.5 { Color32::from_gray(235) } else { Color32::from_gray(50) };
            ui.label(egui::RichText::new(line.score.text()).monospace().strong().color(colour).background_color(fill));
            for (k, (label, san)) in numbered_moves(&fen, &line.san).into_iter().enumerate() {
                if let Some(label) = label {
                    ui.label(egui::RichText::new(label).small().color(t.text_faint));
                }
                if ui
                    .add(egui::Label::new(egui::RichText::new(san).monospace()).sense(egui::Sense::click()))
                    .on_hover_text(tl!("Play the line up to this move"))
                    .clicked()
                {
                    play = line.uci.get(..=k).map(<[String]>::to_vec);
                }
            }
        });
    }
    if let Some(moves) = play {
        board.play_uci_line(&moves);
    }
    action
}

/// The Study board tab.
pub fn study_tab(ui: &mut egui::Ui, t: &Tokens, study: &mut StudyState, board: &mut BoardState, pieces: &PieceSet) -> Option<StudyAction> {
    #[cfg(not(target_arch = "wasm32"))]
    study.sync_engine(ui.ctx(), board);
    if let Some(setup) = study.setup.as_mut() {
        if setup_view(ui, t, setup, board, pieces) {
            study.setup = None;
        }
        return None;
    }
    let mut action = None;
    chess_board_ui::keys(ui, board);
    ui.horizontal_wrapped(|ui| {
        if ui.button(tl!("Set up a position")).on_hover_text(tl!("Put pieces on an empty or starting board, then play from there.")).clicked() {
            study.setup = Some(Setup::from_fen(&board.current_fen()));
        }
        if ui.add_enabled(!board.marks().is_empty(), egui::Button::new(tl!("Clear drawings")).small()).clicked() {
            board.clear_marks();
        }
    });
    chess_board_ui::view_controls(ui, board);
    ui.add_space(4.0);
    // The board, with the evaluation bar beside it while the engine analyses.
    #[cfg(not(target_arch = "wasm32"))]
    let snap = study.analysis.then(|| study.snapshot(board)).flatten();
    #[cfg(not(target_arch = "wasm32"))]
    let best = snap.as_ref().and_then(|s| s.lines.first()).and_then(|l| l.uci.first()).and_then(|mv| {
        let (from, to) = (mv.get(0..2).and_then(Square::parse)?, mv.get(2..4).and_then(Square::parse)?);
        Some(Mark { from, to, color: MarkColor::Engine })
    });
    #[cfg(target_arch = "wasm32")]
    let best: Option<Mark> = None;
    #[cfg(not(target_arch = "wasm32"))]
    let bar = if study.analysis { BAR_PT + ui.spacing().item_spacing.x } else { 0.0 };
    #[cfg(target_arch = "wasm32")]
    let bar = 0.0;
    let side = (ui.available_width() - bar - SCROLL_BAR_ROOM).clamp(160.0, 480.0);
    ui.horizontal_top(|ui| {
        #[cfg(not(target_arch = "wasm32"))]
        if study.analysis {
            eval_bar(ui, snap.as_ref().and_then(|s| s.lines.first()).map(|l| l.score), board.flipped, side);
        }
        ui.allocate_ui(vec2(side, side), |ui| chess_board_ui::board_view(ui, t, board, pieces, best.as_slice()));
    });
    ui.label(
        egui::RichText::new(tl!(
            "Right-click a square to circle it, or drag with the right button to draw an arrow (Shift red, Alt blue, Ctrl yellow)."
        ))
        .small()
        .color(t.text_faint),
    );
    chess_board_ui::promotion_choice(ui, board);
    chess_board_ui::navigation(ui, t, board);
    material_view(ui, t, board, pieces);
    ui.add_space(6.0);
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(a) = engine_view(ui, t, study, board) {
        action = Some(a);
    }
    #[cfg(target_arch = "wasm32")]
    ui.label(egui::RichText::new(tl!("Engine analysis needs the desktop app.")).small().color(t.text_muted));
    ui.add_space(6.0);
    chess_board_ui::moves_and_notes(ui, t, board);
    ui.add_space(6.0);
    if let Some(a) = chess_board_ui::fen_and_pgn(ui, board) {
        action = Some(StudyAction::Board(a));
    }
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sq(name: &str) -> Square {
        Square::parse(name).unwrap()
    }

    #[test]
    fn setup_paints_erases_and_keeps_castling_honest() {
        let mut s = Setup::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1");
        assert!(s.legal());
        // Lifting the h1 rook takes White's short castling away, and only that.
        s.set(sq("h1"), None);
        assert_eq!(s.position.castling.fen(), "Qkq");
        // The brush on its own piece empties the square.
        s.brush = Brush::Put(Piece { color: Color::Black, kind: PieceKind::Queen });
        s.paint(sq("d8"));
        assert_eq!(s.position.placement.get(sq("d8")), None);
        s.paint(sq("d8"));
        assert_eq!(s.position.placement.get(sq("d8")), Some(Piece { color: Color::Black, kind: PieceKind::Queen }));
        s.set_placement(Placement::empty(), false);
        assert!(!s.legal(), "no kings");
        s.set(sq("e1"), Some(Piece { color: Color::White, kind: PieceKind::King }));
        s.set(sq("e8"), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        assert!(s.legal());
        assert_eq!(s.fen(), "4k3/8/8/8/8/8/8/4K3 w - - 0 1");
        // The start position brings every right back.
        s.set_placement(Placement::standard(), true);
        assert_eq!(s.position.castling.fen(), "KQkq");
    }

    #[test]
    fn setup_reads_an_illegal_position_and_a_bad_fen() {
        let s = Setup::from_fen("K7/8/8/8/8/8/8/K6k w - - 0 1");
        assert_eq!(s.position.placement.count_color(Color::White), 2);
        assert!(!s.legal());
        let s = Setup::from_fen("not a fen");
        assert_eq!(s.position.placement.count_color(Color::White), 0);
        assert_eq!(s.position.side_to_move, Some(Color::White));
    }

    #[test]
    fn material_is_the_difference_in_pieces() {
        let start = Placement::standard();
        assert!(material(&start).iter().all(|(_, d)| *d == 0));
        assert_eq!(material_lead(&start), 0);
        // White has a rook for a knight and a pawn more.
        let p = pdfcraft_engine::chess::pdfcraft_chess::fen::parse_placement("4k3/1n6/8/8/8/8/PP6/R3K3").unwrap();
        let d = material(&p);
        assert!(d.contains(&(PieceKind::Rook, 1)) && d.contains(&(PieceKind::Knight, -1)) && d.contains(&(PieceKind::Pawn, 2)));
        assert_eq!(material_lead(&p), 5 - 3 + 2);
    }

    #[test]
    fn line_moves_are_numbered_from_the_position() {
        let sans: Vec<String> = ["e5", "Nf3", "Nc6"].map(String::from).to_vec();
        let labels = numbered_moves("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1", &sans);
        assert_eq!(labels[0], (Some("1...".to_owned()), "e5".to_owned()));
        assert_eq!(labels[1], (Some("2.".to_owned()), "Nf3".to_owned()));
        assert_eq!(labels[2], (None, "Nc6".to_owned()));
    }

    #[test]
    fn a_line_plays_onto_the_board_as_far_as_it_goes() {
        let mut b = BoardState::default();
        b.play_uci_line(&["e2e4".into(), "e7e5".into(), "e1e3".into(), "g1f3".into()]);
        assert_eq!(b.game.movetext(), "1. e4 e5");
        assert!(b.message.is_some(), "the move that could not be played is reported");
    }

    #[test]
    fn drawings_belong_to_their_position() {
        let mut b = BoardState::default();
        let arrow = Mark { from: sq("e2"), to: sq("e4"), color: MarkColor::Green };
        b.toggle_mark(arrow);
        assert_eq!(b.marks(), [arrow]);
        // The same squares in another colour recolour it; in the same colour they take it off.
        b.toggle_mark(Mark { color: MarkColor::Red, ..arrow });
        assert_eq!(b.marks()[0].color, MarkColor::Red);
        b.toggle_mark(Mark { color: MarkColor::Red, ..arrow });
        assert!(b.marks().is_empty());
        b.toggle_mark(arrow);
        b.play_uci_line(&["e2e4".into()]);
        assert!(b.marks().is_empty(), "the next position has its own");
        b.step(false);
        assert_eq!(b.marks(), [arrow]);
        b.clear_marks();
        assert!(b.marks().is_empty());
    }
}
