//! The notice after moves left the game on the board (plan A8 §7, V2a): how many moves went, and
//! how many variations among them, with Undo.
//!
//! It is noticed from the game itself rather than from each command: a new undo step after which
//! the game shown has fewer moves. So whatever took them (Delete, Shift+Delete, a move's menu, the
//! button under the notation, a FEN or a set-up put in the game's place) gets the notice, and Undo
//! or Redo, which go back to steps already seen, do not. It stays, with no timeout (WCAG 2.2.1),
//! while the game is as the deletion left it, until it is dismissed.

use pdfcraft_engine::chess::pdfcraft_chess::game::{Game, Token};

use crate::chess_board_ui::BoardState;
use crate::icons;
use crate::theme::Tokens;

/// What the panel last saw of the game shown, and the notice it shows.
#[derive(Default)]
pub struct DeleteWatch {
    /// The game last seen: its id in the collection, its revision, its moves and its variations.
    seen: Option<Seen>,
    /// The newest undo step seen: an Undo goes back to an older one, so it is no deletion.
    newest_step: u64,
    notice: Option<Notice>,
}

#[derive(Clone, Copy)]
struct Seen {
    game: u64,
    revision: u64,
    moves: usize,
    variations: usize,
}

struct Notice {
    text: String,
    game: u64,
    revision: u64,
}

/// The moves in `game`, variations included, and its variations.
fn counts(game: &Game) -> (usize, usize) {
    game.tokens().iter().fold((0, 0), |(moves, variations), token| match token {
        Token::Move { .. } => (moves + 1, variations),
        Token::VariationStart => (moves, variations + 1),
        _ => (moves, variations),
    })
}

/// "5 moves deleted", with "(2 variations)" when variations went too.
fn text(moves: usize, variations: usize) -> String {
    let lang = crate::i18n::current();
    let count = |n: usize| u64::try_from(n).unwrap_or(u64::MAX);
    let mut text = crate::i18n::trn(lang, count(moves), "{n} move deleted", "{n} moves deleted");
    if variations > 0 {
        text.push(' ');
        text.push_str(&crate::i18n::trn(lang, count(variations), "({n} variation)", "({n} variations)"));
    }
    text
}

impl DeleteWatch {
    /// Look at the board of game `game` (its id in the collection), once a frame. How many moves
    /// left it, when that is new this frame.
    pub(crate) fn see(&mut self, game: u64, board: &BoardState) -> Option<usize> {
        let mut deleted = None;
        let revision = board.game.revision();
        let step = board.history.seq(true).unwrap_or(0);
        let fresh = step > self.newest_step;
        self.newest_step = self.newest_step.max(step);
        match self.seen {
            Some(seen) if seen.game == game && seen.revision == revision => {}
            seen => {
                let (moves, variations) = counts(&board.game);
                if let Some(before) = seen.filter(|s| s.game == game)
                    && fresh
                    && moves < before.moves
                {
                    let text = text(before.moves - moves, before.variations.saturating_sub(variations));
                    self.notice = Some(Notice { text, game, revision });
                    deleted = Some(before.moves - moves);
                }
                self.seen = Some(Seen { game, revision, moves, variations });
            }
        }
        if self.notice.as_ref().is_some_and(|n| n.game != game || n.revision != revision) {
            self.notice = None;
        }
        deleted
    }

    /// The notice's text, while it shows.
    pub fn text(&self) -> Option<&str> {
        self.notice.as_ref().map(|n| n.text.as_str())
    }

    /// Draw the notice across the top of the panel, in the flow: it covers nothing (the panel
    /// moves down while it shows). Undo, and Dismiss to take it away. Whether Undo was clicked.
    pub(crate) fn show(&mut self, ui: &mut egui::Ui, t: &Tokens) -> bool {
        let Some(text) = self.text().map(str::to_owned) else { return false };
        let (mut undo, mut dismiss) = (false, false);
        egui::Frame::NONE
            .fill(t.card)
            .stroke(egui::Stroke::new(1.0, t.border))
            .corner_radius(egui::CornerRadius::same(t.radius))
            .inner_margin(egui::Margin::symmetric(8, 4))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.add(icons::image("trash-2", 14.0, t.text_muted));
                    ui.label(text);
                    let r = ui.button(tl!("Undo"));
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tl!("Bring the deleted moves back")));
                    undo = r.clicked();
                    dismiss = icons::button(ui, "x", 22.0, false, tl!("Dismiss")).clicked();
                });
            });
        if undo || dismiss {
            self.notice = None;
        }
        undo
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chess_history::ChessEdit;

    fn board(pgn: &str) -> BoardState {
        let mut board = BoardState::default();
        board.game = Game::from_pgn(pgn).unwrap().0;
        board
    }

    #[test]
    fn a_new_step_that_takes_moves_away_is_told_and_undo_is_not() {
        crate::i18n::set_current(crate::i18n::Lang::from_code("en").unwrap());
        let mut b = board("1. e4 e5 (1... c5 2. Nf3 (2. c3)) 2. Nf3 *");
        let mut w = DeleteWatch::default();
        w.see(1, &b);
        assert_eq!(w.text(), None);
        // Cut after 1. e4: four moves, two variations.
        b.edit(|s| {
            s.game.go_to(1);
            s.game.truncate();
            Some(ChessEdit::DeleteMoves)
        });
        assert_eq!(w.see(1, &b), Some(5));
        assert_eq!(w.text(), Some("5 moves deleted (2 variations)"));
        // Still shown while the game is as the deletion left it (and told once); gone once it
        // changes.
        assert_eq!(w.see(1, &b), None);
        assert!(w.text().is_some());
        assert!(b.undo());
        w.see(1, &b);
        assert_eq!(w.text(), None, "an undo is no deletion");
        // Undoing a move played takes a move away, and is no deletion either.
        b.edit(|s| {
            s.game.play_san("Nc6").ok()?;
            Some(ChessEdit::Play("Nc6".into()))
        });
        w.see(1, &b);
        assert!(b.undo());
        w.see(1, &b);
        assert_eq!(w.text(), None);
        // One move, and another game shown, which is not this one's deletion.
        b.edit(|s| {
            s.game.go_to(usize::MAX);
            s.game.back();
            s.game.truncate();
            Some(ChessEdit::DeleteMoves)
        });
        w.see(1, &b);
        assert_eq!(w.text(), Some("1 move deleted"));
        w.see(2, &b);
        assert_eq!(w.text(), None);
    }
}
