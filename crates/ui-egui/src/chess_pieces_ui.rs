//! Preferences ▸ Chess pieces (plan HC2), above the chess models: the piece set of the boards
//! and the one of diagrams and exports, each picked from a grid of the sets this machine has
//! (`chessvision::pieces::choice`). A pick is applied at once and kept with the preferences.

use std::collections::HashMap;
use std::path::PathBuf;

use egui::{Color32, Rect, Sense, Stroke, TextureHandle, vec2};
use pdfcraft_engine::chess::pdfcraft_chess::placement::{Color, Piece, PieceKind};
use pdfcraft_engine::chess::pdfcraft_chessvision::pieces::choice::{self, Candidate, CandidateKind, PieceChoice, Role};
use pdfcraft_engine::chess::pdfcraft_chessvision::pieces::{self, import};

use crate::PdfCraftApp;
use crate::theme::{self, Tokens};
use crate::widgets;

/// A tile's side and its squares'.
const TILE: f32 = 96.0;
const SQUARE: f32 = 26.0;
/// The squares' colours (the boards' brown).
const LIGHT: Color32 = Color32::from_rgb(240, 217, 181);
const DARK: Color32 = Color32::from_rgb(181, 136, 99);

/// The picker while Preferences is open: the sets found and their pictures, drawn one per frame.
pub(crate) struct PiecePicker {
    role: Role,
    candidates: Vec<Candidate>,
    /// Each set's twelve pieces as one picture (`None` when the set cannot be drawn), by
    /// [`preview_key`].
    previews: HashMap<String, Option<TextureHandle>>,
    /// What the last import did, shown under its buttons.
    pub(crate) import_note: Option<String>,
    /// A page or folder whose sets are already in the pieces folder, waiting for Replace.
    pub(crate) import_pending: Option<(PathBuf, PathBuf)>,
}

impl PiecePicker {
    fn new() -> PiecePicker {
        PiecePicker { role: Role::Board, candidates: choice::candidates(), previews: HashMap::new(), import_note: None, import_pending: None }
    }
}

/// Automatic draws what each role finds on its own, so its picture is per role.
fn preview_key(choice: &PieceChoice, role: Role) -> String {
    match choice {
        PieceChoice::Auto => format!("auto:{role:?}"),
        other => other.token(),
    }
}

/// The section, above Preferences' chess models.
pub(crate) fn preferences_section(ui: &mut egui::Ui, app: &mut PdfCraftApp, t: &Tokens) {
    ui.label(egui::RichText::new(tl!("Chess pieces")).font(theme::semibold(13.0)));
    let mut picker = app.chess.piece_picker.take().unwrap_or_else(PiecePicker::new);
    ui.horizontal(|ui| {
        widgets::radio_value(ui, &mut picker.role, Role::Board, tl!("Board"));
        widgets::radio_value(ui, &mut picker.role, Role::Diagrams, tl!("Diagrams"));
    });
    ui.label(
        egui::RichText::new(tl!("Board: the analysis and study boards. Diagrams: redrawn on the page, in the editor and in exports."))
            .small()
            .color(t.text_muted),
    );
    if choice::environment_overrides() {
        ui.label(egui::RichText::new(tl!("PDFCRAFT_CHESS_PIECES is set and takes precedence over this choice.")).small().color(t.text_muted));
    }
    // One picture per frame, so opening Preferences never stalls on a dozen sets.
    let role = picker.role;
    if let Some(c) = picker.candidates.iter().find(|c| c.missing.is_empty() && !picker.previews.contains_key(&preview_key(&c.choice, role))) {
        let (set, _, error) = choice::load(&c.choice, role);
        let picture = error.is_none().then(|| crate::chess_board_ui::cut_sprites(ui.ctx(), &set, "chess-piece-preview")).flatten();
        picker.previews.insert(preview_key(&c.choice, role), picture);
        ui.ctx().request_repaint();
    }
    // The role's set loaded, so that one that cannot be used says why below.
    app.chess.role_pieces(role);
    let chosen = app.chess.piece_choice(role).clone();
    let mut pick = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        for c in &picker.candidates {
            let picture = picker.previews.get(&preview_key(&c.choice, role)).cloned().flatten();
            if tile(ui, t, c, picture.as_ref(), c.choice == chosen).clicked() {
                pick = Some(c.choice.clone());
            }
        }
    });
    if let Some(c) = picker.candidates.iter().find(|c| c.choice == chosen) {
        let about = about(c);
        if !about.is_empty() {
            ui.label(egui::RichText::new(about).small().color(t.text_muted));
        }
    }
    if let Some(reason) = app.chess.piece_error(role) {
        let text = crate::i18n::fmt(tl!("These pieces cannot be used: {reason}"), &[("reason", reason)]);
        ui.label(egui::RichText::new(text).small().color(t.text_muted));
    }
    let action = import_row(ui, t, &picker);
    app.chess.piece_picker = Some(picker);
    if let Some(choice) = pick {
        app.set_piece_choice(role, choice);
    }
    match action {
        Some(ImportAction::Folder) => app.choose_pieces_to_import(true),
        Some(ImportAction::Page) => app.choose_pieces_to_import(false),
        Some(ImportAction::Replace) => {
            if let Some((from, into)) = app.chess.piece_picker.as_mut().and_then(|p| p.import_pending.take()) {
                app.import_pieces_into(from, into, true);
            }
        }
        Some(ImportAction::Keep) => {
            if let Some(p) = app.chess.piece_picker.as_mut() {
                p.import_pending = None;
                p.import_note = None;
            }
        }
        None => {}
    }
}

/// What the import row asked for.
enum ImportAction {
    Folder,
    Page,
    Replace,
    Keep,
}

/// Import a folder or a saved web page's sets into the pieces folder (plan HC3), and what the
/// last import did; with sets already there, Replace or Cancel.
fn import_row(ui: &mut egui::Ui, t: &Tokens, picker: &PiecePicker) -> Option<ImportAction> {
    if cfg!(target_arch = "wasm32") {
        return None;
    }
    let mut action = None;
    ui.horizontal_wrapped(|ui| {
        if ui
            .button(tl!("Import a folder…"))
            .on_hover_text(tl!("Copy a folder of twelve piece images (wK to bP, SVG or PNG) into the pieces folder."))
            .clicked()
        {
            action = Some(ImportAction::Folder);
        }
        if ui
            .button(tl!("Import from a web page…"))
            .on_hover_text(tl!("Take the piece sets a saved web page carries inside it into the pieces folder."))
            .clicked()
        {
            action = Some(ImportAction::Page);
        }
    });
    if let Some(note) = &picker.import_note {
        ui.label(egui::RichText::new(note).small().color(t.text_muted));
    }
    if picker.import_pending.is_some() {
        ui.horizontal(|ui| {
            if ui.button(tl!("Replace")).clicked() {
                action = Some(ImportAction::Replace);
            }
            if ui.button(tl!("Cancel")).clicked() {
                action = Some(ImportAction::Keep);
            }
        });
    }
    action
}

impl PdfCraftApp {
    /// Ask for a folder of piece images (`folder`) or a saved web page, then import its sets.
    pub(crate) fn choose_pieces_to_import(&mut self, folder: bool) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let then = |app: &mut PdfCraftApp, path: PathBuf| app.import_pieces(path);
            if folder {
                let dialog = rfd::AsyncFileDialog::new().set_title(tl!("Choose a folder of piece images"));
                self.ask_one(crate::pickers::Ask::Folder(dialog), None, then);
            } else {
                let dialog = rfd::AsyncFileDialog::new().set_title(tl!("Choose a saved web page")).add_filter(tl!("Web pages"), &["html", "htm"]);
                self.ask_one(crate::pickers::Ask::File(dialog), None, then);
            }
        }
        #[cfg(target_arch = "wasm32")]
        let _ = folder;
    }

    /// Import the sets in `from` into the pieces folder beside the program.
    pub fn import_pieces(&mut self, from: PathBuf) {
        match pieces::pieces_folder() {
            Some(into) => self.import_pieces_into(from, into, false),
            None => self.notify(tl!("There is no pieces folder beside the program to import into.")),
        }
    }

    /// Import the sets in `from` into `into`, replacing sets of the same name only when
    /// `replace`; what happened is shown under the import buttons, and the grid lists the sets.
    pub fn import_pieces_into(&mut self, from: PathBuf, into: PathBuf, replace: bool) {
        let result = import::import(&from, &into, replace);
        let picker = self.chess.piece_picker.get_or_insert_with(PiecePicker::new);
        let names = |dirs: &mut dyn Iterator<Item = &std::path::Path>| {
            dirs.filter_map(|d| d.file_name()).map(|n| n.to_string_lossy().into_owned()).collect::<Vec<_>>().join(", ")
        };
        match result {
            Ok(sets) => {
                let list = names(&mut sets.iter().map(|s| s.dir.as_path()));
                *picker = PiecePicker { role: picker.role, ..PiecePicker::new() };
                picker.import_note =
                    Some(crate::i18n::fmt(tl!("Imported: {sets}. Their licence is not known: use them for personal use only."), &[("sets", &list)]));
            }
            Err(import::ImportError::Exists(dirs)) => {
                let list = names(&mut dirs.iter().map(PathBuf::as_path));
                picker.import_note = Some(crate::i18n::fmt(tl!("Already in the pieces folder: {sets}. Replace them?"), &[("sets", &list)]));
                picker.import_pending = Some((from, into));
            }
            Err(e) => {
                picker.import_pending = None;
                picker.import_note = Some(crate::i18n::fmt(tl!("The pieces cannot be imported: {reason}"), &[("reason", &e.to_string())]));
            }
        }
    }
}

/// What a set is called in the grid.
fn label(c: &Candidate) -> String {
    match c.kind {
        CandidateKind::Auto => tl!("Auto").to_owned(),
        CandidateKind::Geometric => tl!("Plain shapes").to_owned(),
        _ => c.name.clone(),
    }
}

/// A set's author and licence, or where it is on disk.
fn about(c: &Candidate) -> String {
    match (c.author, c.licence, &c.path) {
        (Some(author), Some(licence), _) => format!("{} · {author} · {licence}", c.name),
        (_, _, Some(path)) => path.to_string_lossy().into_owned(),
        _ => String::new(),
    }
}

/// One set: a white king and a black knight over a white knight and a black king on two light
/// and two dark squares, its name under them. A folder missing pieces cannot be picked and says
/// what it lacks.
fn tile(ui: &mut egui::Ui, t: &Tokens, c: &Candidate, picture: Option<&TextureHandle>, selected: bool) -> egui::Response {
    let usable = c.missing.is_empty();
    let name = label(c);
    let sense = if usable { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(vec2(TILE, TILE), sense);
    let painter = ui.painter_at(rect);
    let fill = if response.hovered() && usable { t.hover } else { t.card };
    painter.rect_filled(rect, f32::from(t.radius), fill);
    let ring = if selected || response.has_focus() { Stroke::new(2.0, t.accent) } else { Stroke::new(1.0, t.border) };
    painter.rect_stroke(rect.shrink(1.0), f32::from(t.radius), ring, egui::StrokeKind::Inside);
    let top = rect.center_top() + vec2(-SQUARE, 8.0);
    let cells = [
        (0.0, 0.0, LIGHT, Piece { color: Color::White, kind: PieceKind::King }),
        (1.0, 0.0, DARK, Piece { color: Color::Black, kind: PieceKind::Knight }),
        (0.0, 1.0, DARK, Piece { color: Color::White, kind: PieceKind::Knight }),
        (1.0, 1.0, LIGHT, Piece { color: Color::Black, kind: PieceKind::King }),
    ];
    for (x, y, square, piece) in cells {
        let cell = Rect::from_min_size(top + vec2(x * SQUARE, y * SQUARE), vec2(SQUARE, SQUARE));
        painter.rect_filled(cell, 0.0, square);
        if let Some(tex) = picture {
            painter.image(tex.id(), cell, crate::chess_board_ui::sprite_uv(piece), Color32::WHITE);
        }
    }
    let text = if usable { t.text } else { t.text_faint };
    let font = egui::FontId::proportional(11.5);
    let line = widgets::fit_line(&name, TILE - 8.0, |s| widgets::text_width(ui, s, &font));
    painter.text(rect.center_bottom() - vec2(0.0, 8.0), egui::Align2::CENTER_BOTTOM, line, font, text);
    let missing = (!usable).then(|| crate::i18n::fmt(tl!("Missing: {pieces}"), &[("pieces", &c.missing.join(", "))]));
    let hover = missing.clone().unwrap_or_else(|| about(c));
    let response = if hover.is_empty() { response } else { response.on_hover_text(hover) };
    let spoken = missing.map_or_else(|| name.clone(), |m| format!("{name}. {m}"));
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, usable, selected, &spoken));
    response
}
