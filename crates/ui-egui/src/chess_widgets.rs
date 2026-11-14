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
    /// Read, and the book's PGN has the position (plan "Carregar PGN do livro", R6b).
    Reference,
}

impl DiagramState {
    /// The state of `reading`, after what the user did with it (`kind`, from the corrections) and
    /// whether the book's PGN has its position (`confirmed`, by [`Status::with_reference`]): the
    /// engine's rule, which `chess_diagram_states` counts by too.
    pub(crate) fn of(reading: Option<&BoardReading>, kind: Option<Kind>, confirmed: bool) -> DiagramState {
        match Status::of(reading, kind) {
            Status::Read | Status::Review if confirmed => DiagramState::Reference,
            Status::Read => DiagramState::Read,
            Status::Review => DiagramState::Review,
            Status::Unread => DiagramState::Unread,
            Status::Error => DiagramState::Error,
            Status::Verified => DiagramState::Verified,
            Status::Corrected => DiagramState::Corrected,
            Status::Reference => DiagramState::Reference,
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
            DiagramState::Reference => "book-open",
        }
    }

    /// Its mark's colour (icons, bars, strokes).
    pub(crate) fn colour(self, t: &Tokens) -> Color32 {
        match self {
            DiagramState::Read | DiagramState::Verified | DiagramState::Corrected | DiagramState::Reference => t.success,
            DiagramState::Review => t.warning,
            DiagramState::Unread => t.text_faint,
            DiagramState::Error => t.danger,
        }
    }

    /// Its text colour and the soft fill it reads on at 4.5:1 or more (the page marks' tabs).
    pub(crate) fn text_on_soft(self, t: &Tokens) -> (Color32, Color32) {
        match self {
            DiagramState::Read | DiagramState::Verified | DiagramState::Corrected | DiagramState::Reference => (t.success_text, t.success_soft),
            DiagramState::Review => (t.warning_text, t.warning_soft),
            DiagramState::Unread => (t.text_muted, t.card),
            DiagramState::Error => (t.danger_text, t.danger_soft),
        }
    }

    /// Its name.
    pub(crate) fn name(self) -> &'static str {
        match self {
            DiagramState::Read => tl_ctx!("diagram state", "Read"),
            DiagramState::Review => tl!("To review"),
            DiagramState::Unread => tl!("Not read"),
            DiagramState::Error => tl!("With an error"),
            DiagramState::Verified => tl!("Verified"),
            DiagramState::Corrected => tl!("Corrected"),
            DiagramState::Reference => tl!("Confirmed by the PGN"),
        }
    }

    /// Its word in a list row, short so the row's title keeps its room (plan A9-I8, §2.8.1,
    /// finding A6-13): the name, but "PGN" for one the book's PGN confirms. The whole name is the
    /// row's tooltip and accessible name.
    pub(crate) fn short_name(self) -> &'static str {
        match self {
            DiagramState::Reference => "PGN",
            other => other.name(),
        }
    }

    /// Its filter chip's words: the diagrams in this state, as a group, and how many (`n`).
    pub(crate) fn filter_name(self, n: usize) -> String {
        let name = match self {
            DiagramState::Read => tl!("Read ({n})"),
            DiagramState::Review => tl!("To review ({n})"),
            DiagramState::Unread => tl!("Not read ({n})"),
            DiagramState::Error => tl!("With an error ({n})"),
            DiagramState::Verified => tl!("Verified ({n})"),
            DiagramState::Corrected => tl!("Corrected ({n})"),
            DiagramState::Reference => tl!("Confirmed by the PGN ({n})"),
        };
        crate::i18n::fmt(name, &[("n", &n.to_string())])
    }

    /// What it means, for a tooltip.
    pub(crate) fn hint(self) -> &'static str {
        match self {
            DiagramState::Read => tl!("Position read with confidence."),
            DiagramState::Review => tl!("Some square was in doubt. Check it before using it."),
            DiagramState::Unread | DiagramState::Error => tl!("It couldn't be read. Set the position up in the editor."),
            DiagramState::Verified => tl!("You checked it and kept the reading."),
            DiagramState::Corrected => tl!("You corrected this position."),
            DiagramState::Reference => tl!("The book's PGN has this position, so it needs no review."),
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

    /// One icon, one concept (plan A9-I8, §2.8.1, finding R4-07): the icons the chess panels gave two
    /// meanings, each with the one it keeps and the files that may draw it for that. The test reads
    /// the chess sources and refuses any other use, and the × as Stop.
    const ONE_ICON_ONE_CONCEPT: [(&str, &str, &[&str]); 4] = [
        // Set up a position, the rail's tool; the Board button has grid-3x3 and its word.
        ("layout-grid", "Set up a position", &["chess_tools.rs"]),
        // The Move list beside the page; Capture as the line has rows-3.
        ("list-ordered", "Move list beside the page", &["chess_board_tab.rs"]),
        // The state Not read; the empty Diagram tab has square-dashed-mouse-pointer.
        ("circle-dashed", "Not read", &["chess_widgets.rs"]),
        // Stop, for a long job; × is closing.
        ("circle-stop", "Stop", &["chess_panel_head.rs"]),
    ];

    /// The chess sources (`chess_*.rs`), without their test modules.
    fn chess_sources() -> Vec<(String, String)> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut out = Vec::new();
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("chess_") && name.ends_with(".rs") {
                let text = std::fs::read_to_string(entry.path()).unwrap();
                // What the panels draw: the code, not its tests (this table is in one).
                let code = text.split("#[cfg(test)]\nmod tests").next().unwrap_or_default().to_owned();
                out.push((name, code));
            }
        }
        assert!(out.len() > 30, "the chess sources: {}", out.len());
        out
    }

    /// Plan A9-I8 (§2.8.1, finding R4-07): each icon of the table is drawn for its one concept,
    /// in the files that draw that concept, and a long job's Stop is never the closing ×.
    #[test]
    fn one_icon_one_concept() {
        let sources = chess_sources();
        for (icon, concept, files) in ONE_ICON_ONE_CONCEPT {
            let quoted = format!("\"{icon}\"");
            let users: Vec<&str> = sources.iter().filter(|(_, text)| text.contains(&quoted)).map(|(name, _)| name.as_str()).collect();
            assert!(!users.is_empty(), "{icon} ({concept}) is drawn somewhere");
            for user in &users {
                assert!(files.contains(user), "{icon} is {concept}'s, and {user} draws it for something else");
            }
        }
        for (name, text) in &sources {
            for line in text.lines().filter(|l| l.contains("icons::button(") && l.contains("\"x\"")) {
                assert!(!line.contains("Stop"), "{name}: Stop drawn as the closing ×: {line}");
            }
        }
    }

    /// "Read" alone is the Read mode's tab, a verb ("Ler", "Lesen", "Lire"); the diagram state says
    /// the diagram was read ("Lido", "Gelesen", "Lu"), in every language.
    #[test]
    fn the_read_state_is_said_as_done_not_as_the_verb() {
        use crate::i18n::{self, Lang};
        for lang in Lang::all().filter(|l| l.code() != "en") {
            i18n::set_current(lang);
            assert_ne!(DiagramState::Read.name(), i18n::tr(lang, "Read"), "{}", lang.code());
        }
        let pt = Lang::from_code("pt-br").expect("pt-BR registered");
        i18n::set_current(pt);
        assert_eq!((DiagramState::Read.name(), i18n::tr(pt, "Read")), ("Lido", "Ler"));
        i18n::set_current(Lang::EN);
        assert_eq!(DiagramState::Read.name(), "Read");
    }

    #[test]
    fn what_the_user_did_outranks_the_reading() {
        assert_eq!(DiagramState::of(None, None, false), DiagramState::Unread);
        assert_eq!(DiagramState::of(None, Some(Kind::Corrected), false), DiagramState::Corrected);
        assert_eq!(DiagramState::of(None, Some(Kind::Verified), false), DiagramState::Verified);
        // The book's PGN having it never makes a diagram not read, or one the user confirmed, Reference.
        assert_eq!(DiagramState::of(None, None, true), DiagramState::Unread);
        assert_eq!(DiagramState::of(None, Some(Kind::Corrected), true), DiagramState::Corrected);
        // Every state has its own icon but the two the user made, which share the badge.
        let all =
            [DiagramState::Read, DiagramState::Review, DiagramState::Unread, DiagramState::Error, DiagramState::Verified, DiagramState::Reference];
        for (i, a) in all.iter().enumerate() {
            assert!(all.iter().skip(i + 1).all(|b| b.icon() != a.icon() && b.name() != a.name()), "{a:?}");
        }
        assert_ne!(DiagramState::Corrected.name(), DiagramState::Verified.name());
    }
}
