//! Chess ▸ what the app keeps of a protected document (plan A9-I1c, §2.3 "Documento protegido,
//! uma política só").
//!
//! For each document the app keeps, unencrypted, what the user did with its chess: the diagram
//! corrections, the book's PGN and the chess games (in the app's data folder) and, when collecting
//! them, the corrected characters (in the folder chosen for them). What may be kept is the
//! engine's one rule ([`persist_policy`]), which every writer here follows:
//!
//! - **Copying not allowed:** nothing of the document is ever written.
//! - **Encrypted:** written only once the user says so. The question is a dialog, asked once per
//!   document, the first time something of it would be written; the answer holds for every kind
//!   and for the rest of the session (while the document is open).
//! - **Until the answer, and after a no,** nothing of it is written, and the session keeps
//!   working in memory: the corrections and the book's PGN are changed in a memory store of their
//!   own ([`crate::chess_store_ui::UiStore`]), the games and the character samples wait.
//! - **Yes** writes what waited: the corrections and the book's PGN merged into their stores by
//!   their kind's rule, the games at once, the samples.

use pdfcraft_engine::DocId;
use pdfcraft_engine::chess::store::{Persist, persist_policy};

use crate::PdfCraftApp;

/// What may be kept of a document now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Keeping {
    /// Written.
    #[default]
    Yes,
    /// Encrypted, and the user has not answered yet: kept in memory meanwhile, and asked.
    Ask,
    /// Never written this session: copying is not allowed, or the user said no.
    No,
}

impl Keeping {
    /// The engine's rule for a document (`policy`) with the user's answer about it, if any.
    pub(crate) fn of(policy: Persist, answer: Option<bool>) -> Keeping {
        match (policy, answer) {
            (Persist::Yes, _) | (Persist::Ask, Some(true)) => Keeping::Yes,
            (Persist::Never, _) | (Persist::Ask, Some(false)) => Keeping::No,
            (Persist::Ask, None) => Keeping::Ask,
        }
    }
}

impl crate::chess_ui::ChessState {
    /// Ask whether to keep document `id`'s chess, unless it is asked already.
    pub(crate) fn ask_keep(&mut self, id: DocId) {
        if !self.keep_asking.contains(&id) {
            self.keep_asking.push(id);
        }
    }
}

impl PdfCraftApp {
    /// What may be kept of document `id` now ([`Keeping`]): nothing of a document that is not
    /// open.
    pub(crate) fn chess_keeping(&self, id: DocId) -> Keeping {
        match self.session.get(id) {
            Some(doc) => Keeping::of(persist_policy(doc), self.chess.doc(id).and_then(|d| d.keep)),
            None => Keeping::No,
        }
    }

    /// [`Self::chess_keeping`] for something of document `id` about to be written: when the user
    /// is still to be asked, the question is put up (once), and nothing is written meanwhile.
    pub(crate) fn chess_keep(&mut self, id: DocId) -> Keeping {
        let keeping = self.chess_keeping(id);
        if keeping == Keeping::Ask {
            self.chess.ask_keep(id);
        }
        keeping
    }

    /// Whether what the app keeps of document `id` may be written now ([`Self::chess_keep`]).
    pub(crate) fn chess_may_write(&mut self, id: DocId) -> bool {
        self.chess_keep(id) == Keeping::Yes
    }

    /// The user's answer about document `id`: its chess is kept from now on, or not, for the rest
    /// of the session. Yes writes what waited.
    pub(crate) fn answer_chess_keep(&mut self, id: DocId, yes: bool) {
        self.chess.keep_asking.retain(|d| *d != id);
        if self.session.get(id).is_none() {
            return;
        }
        self.chess.doc_mut(id).keep = Some(yes);
        self.chess.capture.samples.answer(id, yes);
        let keeping = self.chess_keeping(id);
        self.chess.games.keep_games(id, keeping);
        if yes {
            if let Err(why) = self.chess_corrections_place(id) {
                self.say_corrections_not_kept(&why);
            }
            if let Err(e) = self.book_pgn_place(id) {
                self.notify(crate::i18n::fmt(tl!("The book's PGN couldn't be saved: {e}"), &[("e", &e)]));
            }
            self.flush_chess_games();
        }
    }
}

/// The question about the first document waiting for it, a dialog (plan A9-I11a). Esc, or a click
/// outside, is a no: nothing of it is written, as before the answer.
pub(crate) fn prompt(app: &mut PdfCraftApp, ctx: &egui::Context) {
    // A document closed meanwhile is not asked about.
    let open: Vec<DocId> = app.chess.keep_asking.iter().copied().filter(|id| app.session.get(*id).is_some()).collect();
    app.chess.keep_asking = open;
    let Some(&id) = app.chess.keep_asking.first() else { return };
    let name = app.session.get(id).map(|d| d.name.clone()).unwrap_or_default();
    let body = crate::i18n::fmt(
        tl!(
            "“{name}” is encrypted. The app keeps your diagram corrections, the book's PGN and the chess games of each document unencrypted on this computer, so they come back when the document is opened again; character samples go to the folder chosen for them."
        ),
        &[("name", &name)],
    );
    let mut answer = None;
    let title = tl!("Keep your chess work on this protected document?");
    let shown = crate::widgets::show_titled_modal(ctx, "chess-keep-prompt", title, |ui| {
        ui.set_max_width(460.0);
        ui.add(egui::Label::new(body).wrap());
        ui.add_space(4.0);
        ui.add(egui::Label::new(tl!("Not kept, they last only until the document is closed.")).wrap());
        ui.add_space(10.0);
        ui.horizontal_wrapped(|ui| {
            if crate::widgets::pill_button(ui, tl!("Keep them"), false).clicked() {
                answer = Some(true);
            }
            // The keyboard starts on the answer that writes nothing.
            let no = crate::widgets::pill_button(ui, tl!("Don't keep"), true);
            crate::widgets::dialog_focus(&no);
            if no.clicked() {
                answer = Some(false);
            }
        });
    });
    if answer.is_none() && shown.should_close() {
        answer = Some(false);
    }
    if let Some(yes) = answer {
        app.answer_chess_keep(id, yes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The engine's rule decides, and the user's answer only for an encrypted document: a
    /// document that doesn't allow copying is never kept, whatever was answered.
    #[test]
    fn the_answer_counts_only_for_an_encrypted_document() {
        assert_eq!(Keeping::of(Persist::Yes, None), Keeping::Yes);
        assert_eq!(Keeping::of(Persist::Yes, Some(false)), Keeping::Yes);
        assert_eq!(Keeping::of(Persist::Never, None), Keeping::No);
        assert_eq!(Keeping::of(Persist::Never, Some(true)), Keeping::No);
        assert_eq!(Keeping::of(Persist::Ask, None), Keeping::Ask);
        assert_eq!(Keeping::of(Persist::Ask, Some(true)), Keeping::Yes);
        assert_eq!(Keeping::of(Persist::Ask, Some(false)), Keeping::No);
    }
}
