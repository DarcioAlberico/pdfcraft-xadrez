//! Chess, found by name (plan A9-I16, §2.8.1 "Descoberta", finding R4-02): what the commands
//! `chess.analyze` and `chess.shortcuts` do, and the sheet of the chess keys.
//!
//! The sheet is made from the one key map (`chess_keymap`, plan A9-I18) and nothing else, so it
//! lists the keys the chess panels run and only those: the board's and the move list's (the rail's
//! tools and groups, moving about the game, the signs typed, F6 and F1 itself) and the Diagram
//! tab's. A key still planned for a later step is not listed (nothing takes it yet), nor the app's
//! own one-letter tools. F1 in the chess key scope (`?` is the "poor move" sign typed) and the
//! command `chess.shortcuts` open it.

use crate::PdfCraftApp;
use crate::chess_keymap::{Binding, Place, Trigger, keymap};
use crate::chess_ui::ChessState;

/// One line of the sheet: the keys that do one thing, and that thing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Line {
    pub keys: Vec<String>,
    pub name: String,
}

/// The bindings the sheet lists: the chess scope's and the Diagram tab's that run now, in the
/// key map's order.
pub(crate) fn listed() -> impl Iterator<Item = &'static Binding> {
    keymap().iter().filter(|b| b.planned.is_none() && b.place != Place::App)
}

/// What binding `b` does, in the current language.
fn name(b: &Binding, chess: &ChessState) -> String {
    if let Trigger::Text(sign) = b.trigger {
        let code = crate::chess_annotate::TYPED.iter().find(|(s, _)| *s == sign).map(|(_, c)| *c);
        // `0` takes the move's glyphs off.
        if code == Some(crate::chess_annotate::CLEAR) {
            return tl!("Remove all glyphs").to_owned();
        }
        return code.and_then(crate::chess_annotate::nag_name).unwrap_or(sign).to_owned();
    }
    if b.place == Place::Diagram {
        return crate::chess_diagram_keys::tab_key_name(b.action).unwrap_or(b.action).to_owned();
    }
    crate::chess_tools::action_name(b.action, chess).unwrap_or_else(|| b.action.to_owned())
}

/// How binding `b`'s key is written on this platform.
fn key(b: &Binding, ctx: &egui::Context) -> String {
    match b.trigger {
        Trigger::Key(k) => k.label(ctx),
        Trigger::Text(sign) => sign.to_owned(),
    }
}

/// The sheet's lines for `place`: one per thing done, its keys together ("←, Backspace").
pub(crate) fn lines(place: Place, chess: &ChessState, ctx: &egui::Context) -> Vec<Line> {
    let mut out: Vec<(&'static str, Line)> = Vec::new();
    for b in listed().filter(|b| b.place == place) {
        let k = key(b, ctx);
        match out.iter_mut().find(|(action, _)| *action == b.action) {
            Some((_, line)) => {
                if !line.keys.contains(&k) {
                    line.keys.push(k);
                }
            }
            None => out.push((b.action, Line { keys: vec![k], name: name(b, chess) })),
        }
    }
    out.into_iter().map(|(_, line)| line).collect()
}

impl PdfCraftApp {
    /// `chess.analyze`: an engine's analysis of the position: the diagram open in the Diagram
    /// tab, as edited, put on the board (as its Analyze on the board does), else the board's own.
    pub fn analyze_chess_position(&mut self) {
        if self.chess.editor.is_some() && self.chess.tab == crate::chess_text_ui::ChessTab::Diagrams {
            self.chess_editor_action(crate::chess_editor::EditorAction::Analyze);
            return;
        }
        self.chess.study.analyze();
        self.show_chess_board();
    }

    /// Whether document `id` looks like a chess book before anything was detected in it: a font
    /// named as the diagram and figurine fonts are (`looks_like_chess_font`: Chess Merida,
    /// DiagramTTF…), or a PGN beside it with its name (`chess_reference_ui::find_sibling_pgn`; not
    /// on the web). Found once per document, as the rail asks every frame (plan A9-I16).
    pub(crate) fn chess_book_hint(&mut self, id: pdfcraft_engine::DocId) -> bool {
        if let Some(hint) = self.chess.doc(id).and_then(|d| d.chess_hint) {
            return hint;
        }
        let Some(doc) = self.session.get(id) else { return false };
        let font = doc.info.fonts.iter().any(|f| pdfcraft_engine::chess::pdfcraft_chess::catalog::looks_like_chess_font(&f.name));
        let hint = font
            || self.chess.doc(id).is_some_and(|d| d.pgn_sibling.is_some())
            || crate::chess_reference_ui::find_sibling_pgn(&doc.doc_keys()).is_some();
        self.chess.doc_mut(id).chess_hint = Some(hint);
        hint
    }

    /// `chess.shortcuts`, F1 in the chess scope: the sheet of the chess keys.
    pub fn show_chess_shortcuts(&mut self) {
        self.chess.shortcuts_open = true;
    }
}

/// The sheet, while it is open: a dialog with the board's keys and the Diagram tab's.
pub(crate) fn sheet(app: &mut PdfCraftApp, ctx: &egui::Context) {
    if !app.chess.shortcuts_open {
        return;
    }
    let sections = [
        (tl!("Board, move list and Chess panel"), lines(Place::Scope, &app.chess, ctx)),
        (tl!("Diagram tab"), lines(Place::Diagram, &app.chess, ctx)),
    ];
    let mut close = false;
    let shown = crate::widgets::show_titled_modal(ctx, "chess-shortcuts", tl!("Chess shortcuts"), |ui| {
        ui.set_max_width(560.0);
        let height = (ctx.content_rect().height() - 220.0).max(160.0);
        egui::ScrollArea::vertical().max_height(height).show(ui, |ui| {
            for (title, lines) in &sections {
                // A section's title in the chess type scale (13 semibold, §2.8.1), as written.
                ui.label(egui::RichText::new(*title).font(crate::theme::semibold(13.0)));
                egui::Grid::new(("chess-shortcuts", *title)).num_columns(2).spacing(egui::vec2(16.0, 4.0)).show(ui, |ui| {
                    for line in lines {
                        ui.label(egui::RichText::new(line.keys.join(", ")).strong());
                        ui.label(&line.name);
                        ui.end_row();
                    }
                });
                ui.add_space(8.0);
            }
        });
        ui.add_space(6.0);
        let done = crate::widgets::pill_button(ui, tl!("Close"), true);
        crate::widgets::dialog_focus(&done);
        close = done.clicked();
    });
    if close || shown.should_close() {
        app.chess.shortcuts_open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sheet lists the keys the scope runs, every one of them and no other (plan A9-I16): the
    /// keys of the key map's chess scope that are not planned, F6 among them (taken on its own),
    /// and the signs typed; and the Diagram tab's.
    #[test]
    fn the_sheet_lists_the_keys_the_scope_runs() {
        use crate::chess_tools::ChessKey;
        let listed_keys: Vec<ChessKey> = listed()
            .filter(|b| b.place == Place::Scope)
            .filter_map(|b| match b.trigger {
                Trigger::Key(k) => Some(k),
                Trigger::Text(_) => None,
            })
            .collect();
        let mut run: Vec<ChessKey> = crate::chess_keymap::scope_keys().collect();
        // F6 leaves the panel: the scope takes it on its own (`chess_tools::take_f6`).
        run.push(ChessKey::plain(egui::Key::F6));
        for k in &run {
            assert!(listed_keys.contains(k), "the scope runs {k:?} and the sheet does not list it");
        }
        for k in &listed_keys {
            assert!(run.contains(k), "the sheet lists {k:?} and the scope does not run it");
        }
        assert!(listed_keys.contains(&ChessKey::plain(egui::Key::F1)), "F1, the sheet itself");
        let signs = listed().filter(|b| matches!(b.trigger, Trigger::Text(_))).count();
        assert_eq!(signs, crate::chess_annotate::TYPED.len(), "every sign typed");
        let tab: Vec<ChessKey> = listed()
            .filter(|b| b.place == Place::Diagram)
            .filter_map(|b| match b.trigger {
                Trigger::Key(k) => Some(k),
                Trigger::Text(_) => None,
            })
            .collect();
        assert_eq!(tab, crate::chess_diagram_keys::TAB_KEYS.map(|(k, _)| k), "the Diagram tab's keys");
    }

    /// Every line says what it does in words, not as an id: a name for every action of the map.
    #[test]
    fn every_line_is_named() {
        let ctx = egui::Context::default();
        let chess = ChessState::default();
        for place in [Place::Scope, Place::Diagram] {
            let lines = lines(place, &chess, &ctx);
            assert!(!lines.is_empty());
            for line in lines {
                let action = listed().find(|b| name(b, &chess) == line.name).map(|b| b.action).unwrap_or_default();
                assert!(!line.name.is_empty() && line.name != action, "{place:?}: {line:?} is not named");
            }
        }
        let back = lines(Place::Scope, &chess, &ctx).into_iter().find(|l| l.keys.iter().any(|k| k == "Backspace")).unwrap();
        assert!(back.keys.contains(&"←".to_owned()), "← and Backspace on one line: {back:?}");
    }
}
