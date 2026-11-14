//! Chess ▸ games the autosave can't keep (plan A9-G8, A5-01): quitting, closing a document and
//! closing all ask about them first, one collection after another, as unsaved PDFs are asked
//! about. "Chess games not saved", with **Save all games as PGN…** (the save dialog; quitting or
//! closing stops there, so nothing is lost), **Quit without saving** or **Close without saving**,
//! and **Cancel**.
//!
//! A collection is asked about when a write of it was refused (too many games, another window, no
//! lock, a newer file, an error) and its games changed since they were last written or read; its
//! untouched diagram games don't count, since they open again from their diagram. A collection
//! left as it is, or whose games were all saved as PGN, is not asked about again until it changes.
//! The web has no quit to ask before; closing a document there asks the same.

use pdfcraft_engine::DocId;

use crate::PdfCraftApp;
use crate::chess_collection::Unsaved;

/// What waits for the answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AfterGames {
    /// Closing document `.0` (its games only are asked about).
    CloseTab(DocId),
    /// File ▸ Close all (the open documents' games).
    CloseAll,
    /// Quitting (every collection: the open documents', the games with no document, and the
    /// closed documents' kept for the session).
    Quit,
}

/// The question asked about one collection, and what then waits.
#[derive(Clone, Debug, PartialEq)]
pub struct GamesPrompt {
    pub(crate) unsaved: Unsaved,
    pub then: AfterGames,
}

/// An answer to the question.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GamesAnswer {
    /// Save all games as PGN…: what waited stops there.
    SavePgn,
    /// Quit (or Close) without saving: the collection is left as it is, and what waited goes on.
    Leave,
    Cancel,
}

impl PdfCraftApp {
    /// Before `then`: write every collection that can be written now, wait for the writes at most
    /// [`crate::chess_writer::SETTLE`] (plan A9-I3b: the store's writer thread writes them) and,
    /// when one of those `then` concerns still isn't kept, or still being written, ask about it.
    /// Whether `then` must wait for the answer.
    pub(crate) fn ask_chess_games(&mut self, then: AfterGames) -> bool {
        self.flush_chess_games();
        self.settle_chess_games();
        let order: Vec<DocId> = match then {
            AfterGames::CloseTab(id) => vec![id],
            AfterGames::CloseAll | AfterGames::Quit => self.views.iter().map(|v| v.id).collect(),
        };
        match self.chess.games.unsaved(&order, then == AfterGames::Quit) {
            Some(unsaved) => {
                self.chess.games_prompt = Some(GamesPrompt { unsaved, then });
                true
            }
            None => false,
        }
    }

    /// Answer the question: Save all games as PGN… opens the save dialog for that collection
    /// (its document brought forward) and stops what waited; leaving it goes on with what waited,
    /// which asks about the next collection, if any; Cancel stops.
    pub fn answer_chess_games(&mut self, answer: GamesAnswer) {
        let Some(GamesPrompt { unsaved, then }) = self.chess.games_prompt.take() else { return };
        match answer {
            GamesAnswer::Cancel => {}
            GamesAnswer::SavePgn => {
                if let Some(i) = unsaved.doc.and_then(|id| self.views.iter().position(|v| v.id == id)) {
                    self.active = Some(i);
                }
                self.save_chess_collection_pgn(&unsaved.cid);
            }
            GamesAnswer::Leave => {
                self.chess.games.leave(&unsaved.cid);
                match then {
                    AfterGames::CloseTab(id) => {
                        if let Some(i) = self.views.iter().position(|v| v.id == id) {
                            self.request_close_tab(i);
                        }
                    }
                    AfterGames::CloseAll => self.close_all(),
                    AfterGames::Quit => {
                        if let Some(ctx) = self.ctx.clone() {
                            self.request_quit(&ctx);
                        }
                    }
                }
            }
        }
    }
}

/// The question about a collection's games, when one waits.
pub(crate) fn prompt(app: &mut PdfCraftApp, ctx: &egui::Context) {
    let Some(p) = app.chess.games_prompt.as_ref() else { return };
    let name = p.unsaved.doc.and_then(|id| app.session.get(id)).map(|d| d.name.clone()).or_else(|| p.unsaved.name.clone());
    let n = p.unsaved.games.to_string();
    let body = match &name {
        Some(name) => crate::i18n::fmt(tl!("Games of “{name}” that autosave doesn't keep: {n}"), &[("name", name), ("n", &n)]),
        None => crate::i18n::fmt(tl!("Games with no document open that autosave doesn't keep: {n}"), &[("n", &n)]),
    };
    let why = app.chess.games.why_not_kept(&p.unsaved.cid);
    let leave = if p.then == AfterGames::Quit { tl!("Quit without saving") } else { tl!("Close without saving") };
    let t = crate::theme::Tokens::get(ctx);
    let mut answer = None;
    let title = tl!("Chess games not saved");
    let shown = crate::widgets::show_titled_modal(ctx, "chess-games-prompt", title, |ui| {
        ui.set_max_width(440.0);
        ui.add(egui::Label::new(body).wrap());
        if let Some(why) = why {
            ui.label(egui::RichText::new(why).small().color(t.text_muted));
        }
        ui.add_space(10.0);
        ui.horizontal_wrapped(|ui| {
            let save = crate::widgets::pill_button(ui, tl!("Save all games as PGN…"), true);
            crate::widgets::dialog_focus(&save);
            if save.clicked() {
                answer = Some(GamesAnswer::SavePgn);
            }
            if crate::widgets::pill_button(ui, leave, false).clicked() {
                answer = Some(GamesAnswer::Leave);
            }
            if crate::widgets::pill_button(ui, tl!("Cancel"), false).clicked() {
                answer = Some(GamesAnswer::Cancel);
            }
        });
    });
    // Esc, or a click outside, cancels: nothing closes, nothing is lost.
    if answer.is_none() && shown.should_close() {
        answer = Some(GamesAnswer::Cancel);
    }
    if let Some(a) = answer {
        app.answer_chess_games(a);
    }
}
