//! How the move list writes the pieces (plan HC8, decision D15): as figurines drawn with the
//! board's own piece set (a small picture the height of the text), as letters in the
//! interface's language (pt-BR `C`, German `S`…), or as English letters, the way SAN is stored.
//! The game and its PGN never change; only the list shows the moves so. Every move is named for
//! assistive technology in words ("knight f3"), whatever is shown.

use std::sync::Arc;

use egui::{Color32, TextureHandle};
use pdfcraft_engine::chess::pdfcraft_chess::draw::PieceSet;
use pdfcraft_engine::chess::pdfcraft_chess::placement::{Color, Piece, PieceKind};
use pdfcraft_engine::chess::pdfcraft_chess::san::Lang;

/// How the list writes the pieces.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MovePieces {
    /// The board's piece set, as small pictures.
    #[default]
    Figurines,
    /// Letters in the interface's language.
    Letters,
    /// English letters (K Q R B N), as SAN writes them.
    English,
}

impl MovePieces {
    pub fn token(self) -> &'static str {
        match self {
            MovePieces::Figurines => "figurines",
            MovePieces::Letters => "letters",
            MovePieces::English => "english",
        }
    }

    pub fn from_token(token: &str) -> Option<MovePieces> {
        [MovePieces::Figurines, MovePieces::Letters, MovePieces::English].into_iter().find(|m| m.token() == token)
    }
}

fn mode_key() -> egui::Id {
    egui::Id::new("chess-move-pieces")
}

fn pieces_key() -> egui::Id {
    egui::Id::new("chess-move-piece-set")
}

fn atlas_key() -> egui::Id {
    egui::Id::new("chess-move-figurine-atlas")
}

pub(crate) fn current(ctx: &egui::Context) -> MovePieces {
    ctx.data(|d| d.get_temp::<MovePieces>(mode_key())).unwrap_or_default()
}

/// The mode and the board's pieces for this frame.
pub(crate) fn set(ctx: &egui::Context, mode: MovePieces, pieces: Option<Arc<PieceSet>>) {
    ctx.data_mut(|d| {
        d.insert_temp(mode_key(), mode);
        match pieces {
            Some(p) => {
                d.insert_temp(pieces_key(), p);
            }
            None => d.remove::<Arc<PieceSet>>(pieces_key()),
        }
    });
}

/// The figurines' picture (the board's twelve pieces, cut as the drag ghost's), made once per
/// piece set.
fn atlas(ctx: &egui::Context) -> Option<TextureHandle> {
    let pieces = ctx.data(|d| d.get_temp::<Arc<PieceSet>>(pieces_key()))?;
    let stamp = (Arc::as_ptr(&pieces) as usize, crate::chess_editor::pieces_generation());
    if let Some((s, tex)) = ctx.data(|d| d.get_temp::<((usize, u64), TextureHandle)>(atlas_key()))
        && s == stamp
    {
        return Some(tex);
    }
    let tex = crate::chess_board_ui::cut_sprites(ctx, &pieces, "chess-move-figurines")?;
    ctx.data_mut(|d| d.insert_temp(atlas_key(), (stamp, tex.clone())));
    Some(tex)
}

/// The piece a SAN move starts with, and the rest (`Nf3` → knight, `f3`); pawn moves and
/// castling have none.
pub(crate) fn split(san: &str) -> (Option<PieceKind>, &str) {
    let kind = match san.chars().next() {
        Some('K') => PieceKind::King,
        Some('Q') => PieceKind::Queen,
        Some('R') => PieceKind::Rook,
        Some('B') => PieceKind::Bishop,
        Some('N') => PieceKind::Knight,
        _ => return (None, san),
    };
    (Some(kind), san.get(1..).unwrap_or_default())
}

fn letter(kind: PieceKind, lang: Lang) -> &'static str {
    let l = lang.letters();
    match kind {
        PieceKind::King => l.king,
        PieceKind::Queen => l.queen,
        PieceKind::Rook => l.rook,
        PieceKind::Bishop => l.bishop,
        PieceKind::Knight => l.knight,
        PieceKind::Pawn => l.pawn,
    }
}

/// `san` with `lang`'s piece letters: the moving piece and a promotion's (`e8=Q` → `e8=D`), and
/// castling with zeros where the language writes them so.
pub(crate) fn in_letters(san: &str, lang: Lang) -> String {
    if san.starts_with("O-O") {
        return if lang.letters().zero_castling { san.replace('O', "0") } else { san.to_owned() };
    }
    let (kind, rest) = split(san);
    let mut out = kind.map(|k| letter(k, lang).to_owned()).unwrap_or_default();
    match rest.split_once('=') {
        Some((before, after)) => {
            let (promoted, tail) = split(after);
            out.push_str(before);
            out.push('=');
            match promoted {
                Some(k) => out.push_str(letter(k, lang)),
                None => out.push_str(after.get(..1).unwrap_or_default()),
            }
            out.push_str(if promoted.is_some() { tail } else { after.get(1..).unwrap_or_default() });
        }
        None => out.push_str(rest),
    }
    out
}

/// The interface's piece-letter language: English when it has none of its own.
fn interface_lang() -> Lang {
    Lang::from_tag(crate::i18n::current().code()).unwrap_or(Lang::En)
}

pub(crate) fn piece_word(kind: PieceKind) -> &'static str {
    match kind {
        PieceKind::King => tl!("king"),
        PieceKind::Queen => tl!("queen"),
        PieceKind::Rook => tl!("rook"),
        PieceKind::Bishop => tl!("bishop"),
        PieceKind::Knight => tl!("knight"),
        PieceKind::Pawn => tl!("pawn"),
    }
}

/// A move in words for assistive technology: "knight f3", "knight takes e5", "e8 promotes to
/// queen, check", "castles short"; `glyphs` (`!`, `?`…) follow as written.
pub(crate) fn spoken(san: &str, glyphs: &str) -> String {
    let (core, check) = match san.strip_suffix('#') {
        Some(c) => (c, Some(tl!("checkmate"))),
        None => match san.strip_suffix('+') {
            Some(c) => (c, Some(tl!("check"))),
            None => (san, None),
        },
    };
    let mut words = match core {
        "O-O" => tl!("castles short").to_owned(),
        "O-O-O" => tl!("castles long").to_owned(),
        _ => {
            let (kind, rest) = split(core);
            let (rest, promotion) = match rest.split_once('=') {
                Some((before, after)) => (before, split(after).0),
                None => (rest, None),
            };
            let rest = rest.replace('x', &format!(" {} ", tl!("takes")));
            let mut w = match kind {
                Some(k) => format!("{} {}", piece_word(k), rest.trim()),
                None => rest.trim().to_owned(),
            };
            if let Some(p) = promotion {
                w = crate::i18n::fmt(tl!("{move} promotes to {piece}"), &[("move", &w), ("piece", piece_word(p))]);
            }
            w
        }
    };
    if let Some(c) = check {
        words = format!("{words}, {c}");
    }
    format!("{words}{glyphs}")
}

/// A move of the list: `san` and its `glyphs` shown as the mode says, selected when it is the
/// move shown; `small` and `muted` for variations. Named in words.
#[allow(clippy::too_many_arguments)]
pub(crate) fn move_button(
    ui: &mut egui::Ui,
    san: &str,
    glyphs: &str,
    spoken_glyphs: &str,
    selected: bool,
    small: bool,
    muted: Color32,
    tint: Option<Color32>,
) -> egui::Response {
    let style = |text: String| {
        let mut rich = egui::RichText::new(text).monospace();
        if small {
            rich = rich.small().color(muted);
        }
        if let Some(t) = tint {
            rich = rich.background_color(t);
        }
        rich
    };
    let mode = current(ui.ctx());
    let (kind, rest) = split(san);
    let response = match (mode, kind) {
        (MovePieces::Figurines, Some(kind)) if let Some(tex) = atlas(ui.ctx()) => {
            let font = if small { egui::TextStyle::Small } else { egui::TextStyle::Monospace };
            let side = ui.text_style_height(&font);
            // The board's white pieces: the list reads as a book printed in black on white.
            let piece = Piece { color: Color::White, kind };
            let image = egui::Image::new(&tex).uv(crate::chess_board_ui::sprite_uv(piece)).fit_to_exact_size(egui::vec2(side, side));
            ui.add(egui::Button::selectable(selected, (image, style(format!("{rest}{glyphs}")))))
        }
        (MovePieces::Letters, _) => ui.selectable_label(selected, style(format!("{}{glyphs}", in_letters(san, interface_lang())))),
        _ => ui.selectable_label(selected, style(format!("{san}{glyphs}"))),
    };
    let name = spoken(san, spoken_glyphs);
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, &name));
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_are_written_in_a_languages_letters() {
        assert_eq!(in_letters("Nf3", Lang::Pt), "Cf3");
        assert_eq!(in_letters("Qxd5+", Lang::De), "Dxd5+");
        assert_eq!(in_letters("e8=Q#", Lang::Pt), "e8=D#");
        assert_eq!(in_letters("exd8=N", Lang::Fr), "exd8=C");
        assert_eq!(in_letters("O-O-O", Lang::De), "0-0-0");
        assert_eq!(in_letters("O-O", Lang::Pt), "O-O");
        assert_eq!(in_letters("Kc2", Lang::Ru), "Крc2");
        assert_eq!(in_letters("e4", Lang::Pt), "e4");
        // Nothing a SAN token could hold is cut where it should not be.
        for odd in ["", "N", "=", "e8=", "e8=é", "Né"] {
            let _ = in_letters(odd, Lang::Pt);
            let _ = spoken(odd, "");
        }
    }

    #[test]
    fn moves_are_said_in_words() {
        assert_eq!(spoken("Nf3", ""), "knight f3");
        assert_eq!(spoken("Nxe5", "!"), "knight takes e5!");
        assert_eq!(spoken("exd8=Q+", ""), "e takes d8 promotes to queen, check");
        assert_eq!(spoken("O-O-O#", "?!"), "castles long, checkmate?!");
        assert_eq!(spoken("e4", ""), "e4");
    }

    #[test]
    fn tokens_read_back() {
        for m in [MovePieces::Figurines, MovePieces::Letters, MovePieces::English] {
            assert_eq!(MovePieces::from_token(m.token()), Some(m));
        }
        assert_eq!(MovePieces::from_token("pictures"), None);
    }
}
