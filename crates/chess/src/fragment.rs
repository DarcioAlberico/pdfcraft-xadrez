//! A stretch of move text read from a page, as items a game can take.
//!
//! A reader (the book walker in `pdfcraft-chessvision`, or a test) turns a selection such as
//! `18.Nd5 Qxe1 19.Nxe7+ (19.Bxf6!?) Kh7` into a flat list of [`FragmentItem`]s, and
//! [`Game::graft`](crate::game::Game::graft) plays them into a game. The items carry only what
//! this crate understands: the printed move number and the result's value stay with the reader.

use crate::nag;
use crate::san::San;

/// Most items one fragment holds. A selection is at most 64 KiB of text, which a real one never
/// turns into more items than this; the graft stops at the cap, it never truncates silently.
pub const MAX_FRAGMENT_ITEMS: usize = 4096;

/// One piece of a fragment, in reading order.
#[derive(Clone, Debug, PartialEq)]
pub enum FragmentItem {
    /// A move.
    Move {
        /// The absolute half-move of the printed number ([`ply_of`]); `None` when it had none.
        ply: Option<u32>,
        /// The move, already read in the language chosen.
        san: San,
        /// The move as printed, for the preview and the report.
        text: String,
        /// Whether it was printed in bold; `None` when the source does not say (OCR, pasted
        /// text). A bold chain is the game's main line. The graft itself does not use it.
        bold: Option<bool>,
        /// Where on the page it was printed, `[x0, y0, x1, y1]` in user space.
        rect: Option<[f32; 4]>,
    },
    /// An annotation glyph, by White's code: the graft applies the code for the side it speaks of
    /// (`↑` is `$36` after a White move and `$37` after a Black one).
    Nag(u16),
    /// A comment, already normalized. `before`: it comes before the next move rather than after
    /// the last one.
    Comment { text: String, before: bool },
    /// `(` or `[`: the moves that follow are an alternative to the last one.
    Open,
    /// `)` or `]`: back to the line the matching [`FragmentItem::Open`] left.
    Close,
    /// A result: the fragment ends here. Its value stays with the reader.
    Result,
}

impl FragmentItem {
    /// The item as text, for a leftover or a comment: a move as printed (its English SAN when
    /// nothing was printed), a glyph as printed (`$n` when it has no safe glyph), a comment as
    /// written and `(`/`)` for the parentheses. A result has no text here.
    pub fn text(&self) -> String {
        match self {
            FragmentItem::Move { san, text, .. } if text.trim().is_empty() => san.to_english(),
            FragmentItem::Move { text, .. } => text.trim().to_owned(),
            FragmentItem::Nag(code) => nag::printed(*code),
            FragmentItem::Comment { text, .. } => text.trim().to_owned(),
            FragmentItem::Open => "(".to_owned(),
            FragmentItem::Close => ")".to_owned(),
            FragmentItem::Result => String::new(),
        }
    }
}

/// The absolute half-move of the move numbered `number` played by Black (`black`) or White:
/// `2·(number−1)`, plus one for Black. `18… Qd8` is half-move 35. Saturates for numbers no
/// game reaches; number 0 counts as 1.
pub fn ply_of(number: u32, black: bool) -> u32 {
    number.saturating_sub(1).saturating_mul(2).saturating_add(u32::from(black))
}

/// `items` as one line of text: the pieces joined by spaces, with none after `(` or before `)`.
/// Results are left out.
pub(crate) fn items_text<'a>(items: impl IntoIterator<Item = &'a FragmentItem>) -> String {
    let mut out = String::new();
    let mut glue = true;
    for item in items {
        let piece = item.text();
        if piece.is_empty() {
            continue;
        }
        if !(glue || matches!(item, FragmentItem::Close)) {
            out.push(' ');
        }
        out.push_str(&piece);
        glue = matches!(item, FragmentItem::Open);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mv(text: &str) -> FragmentItem {
        FragmentItem::Move { ply: None, san: San::parse(text).unwrap(), text: text.to_owned(), bold: None, rect: None }
    }

    #[test]
    fn half_moves_count_from_move_one() {
        assert_eq!(ply_of(1, false), 0);
        assert_eq!(ply_of(1, true), 1);
        assert_eq!(ply_of(18, true), 35);
        assert_eq!(ply_of(0, true), 1);
        assert_eq!(ply_of(u32::MAX, true), u32::MAX);
    }

    #[test]
    fn items_read_back_as_text() {
        let items = vec![
            mv("Nd5"),
            FragmentItem::Nag(1),
            FragmentItem::Open,
            mv("Bxf6"),
            FragmentItem::Nag(146),
            FragmentItem::Comment { text: " an idea ".to_owned(), before: false },
            FragmentItem::Close,
            FragmentItem::Move { ply: Some(36), san: San::parse("Qxe1").unwrap(), text: String::new(), bold: Some(true), rect: None },
            FragmentItem::Result,
        ];
        assert_eq!(items_text(&items), "Nd5 ! (Bxf6 $146 an idea) Qxe1");
    }
}
