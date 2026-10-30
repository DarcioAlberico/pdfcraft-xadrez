//! A diagram's state, in one place (plan A8 P1a): the icon, the colours and the words every list,
//! mark and editor shows it with. A state is never colour alone: its icon and its name carry it.

use egui::Color32;
use pdfcraft_engine::chess::corrections::{Kind, Status};
use pdfcraft_engine::chess::pdfcraft_chessvision::board::BoardReading;

use crate::theme::Tokens;

/// What became of a diagram's reading.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DiagramState {
    /// Read with confidence.
    Read,
    /// Read, with a doubt on some square.
    Review,
    /// Not read yet, or no model to read it.
    Unread,
    /// The reading failed.
    Error,
    /// The user looked and kept the reading.
    Verified,
    /// The user changed the position.
    Corrected,
}

impl DiagramState {
    /// The state of `reading`, after what the user did with it (`kind`, from the corrections):
    /// the engine's rule ([`Status::of`]), which `chess_diagram_states` counts by too.
    pub(crate) fn of(reading: Option<&BoardReading>, kind: Option<Kind>) -> DiagramState {
        match Status::of(reading, kind) {
            Status::Read => DiagramState::Read,
            Status::Review => DiagramState::Review,
            Status::Unread => DiagramState::Unread,
            Status::Error => DiagramState::Error,
            Status::Verified => DiagramState::Verified,
            Status::Corrected => DiagramState::Corrected,
            // Only `Status::with_reference` gives it, and the app has no book's PGN before R6.
            Status::Reference => DiagramState::Read,
        }
    }

    /// Its Lucide icon.
    pub(crate) fn icon(self) -> &'static str {
        match self {
            DiagramState::Read => "circle-check",
            DiagramState::Review => "triangle-alert",
            DiagramState::Unread => "circle-dashed",
            DiagramState::Error => "circle-x",
            DiagramState::Verified | DiagramState::Corrected => "badge-check",
        }
    }

    /// Its mark's colour (icons, bars, strokes).
    pub(crate) fn colour(self, t: &Tokens) -> Color32 {
        match self {
            DiagramState::Read | DiagramState::Verified | DiagramState::Corrected => t.success,
            DiagramState::Review => t.warning,
            DiagramState::Unread => t.text_faint,
            DiagramState::Error => t.danger,
        }
    }

    /// Its text colour and the soft fill it reads on at 4.5:1 or more (the page marks' tabs).
    pub(crate) fn text_on_soft(self, t: &Tokens) -> (Color32, Color32) {
        match self {
            DiagramState::Read | DiagramState::Verified | DiagramState::Corrected => (t.success_text, t.success_soft),
            DiagramState::Review => (t.warning_text, t.warning_soft),
            DiagramState::Unread => (t.text_muted, t.card),
            DiagramState::Error => (t.danger_text, t.danger_soft),
        }
    }

    /// Its name.
    pub(crate) fn name(self) -> &'static str {
        match self {
            DiagramState::Read => tl!("Read"),
            DiagramState::Review => tl!("To review"),
            DiagramState::Unread => tl!("Not read"),
            DiagramState::Error => tl!("With an error"),
            DiagramState::Verified => tl!("Verified"),
            DiagramState::Corrected => tl!("Corrected"),
        }
    }

    /// What it means, for a tooltip.
    pub(crate) fn hint(self) -> &'static str {
        match self {
            DiagramState::Read => tl!("Position read with confidence."),
            DiagramState::Review => tl!("Some square was in doubt. Check it before using it."),
            DiagramState::Unread | DiagramState::Error => tl!("It couldn't be read. Set the position up in the editor."),
            DiagramState::Verified => tl!("You checked it and kept the reading."),
            DiagramState::Corrected => tl!("You corrected this position."),
        }
    }
}

/// A state's icon, `size` points, in its colour, named for assistive technology. `draft`: the
/// diagram has unconfirmed changes, a warning dot on the icon (plan A8, P3b3).
pub(crate) fn state_icon(ui: &mut egui::Ui, t: &Tokens, state: DiagramState, size: f32, draft: bool) -> egui::Response {
    let hint = if draft { format!("{}\n{}", state.hint(), tl!("Unconfirmed changes")) } else { state.hint().to_owned() };
    let r = ui.add(crate::icons::image(state.icon(), size, state.colour(t)).sense(egui::Sense::hover())).on_hover_text(hint);
    let name = if draft { format!("{} · {}", state.name(), tl!("Unconfirmed changes")) } else { state.name().to_owned() };
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, &name));
    if draft {
        draft_dot(ui.painter(), r.rect, t);
    }
    r
}

/// The draft dot at the top right of `rect`.
pub(crate) fn draft_dot(painter: &egui::Painter, rect: egui::Rect, t: &Tokens) {
    let r = (rect.width().min(rect.height()) * 0.2).max(2.5);
    painter.circle(rect.right_top() + egui::vec2(-r * 0.5, r * 0.5), r, t.warning, egui::Stroke::new(1.0, t.card));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_the_user_did_outranks_the_reading() {
        assert_eq!(DiagramState::of(None, None), DiagramState::Unread);
        assert_eq!(DiagramState::of(None, Some(Kind::Corrected)), DiagramState::Corrected);
        assert_eq!(DiagramState::of(None, Some(Kind::Verified)), DiagramState::Verified);
        // Every state has its own icon but the two the user made, which share the badge.
        let all = [DiagramState::Read, DiagramState::Review, DiagramState::Unread, DiagramState::Error, DiagramState::Verified];
        for (i, a) in all.iter().enumerate() {
            assert!(all.iter().skip(i + 1).all(|b| b.icon() != a.icon() && b.name() != a.name()), "{a:?}");
        }
        assert_ne!(DiagramState::Corrected.name(), DiagramState::Verified.name());
    }
}
