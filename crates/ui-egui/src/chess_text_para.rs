//! Chess ▸ Text tab: one paragraph, one galley (plan A9-I9, §2.8.1 "Aba Texto — dois eixos";
//! findings A6-06, A6-07, A6-23, R5-12).
//!
//! - **One galley per paragraph:** the prose and the moves are sections of one [`LayoutJob`] in
//!   one font, so its lines are as tall as each other and wrap as text does (a `Label` per inline
//!   made the rows uneven, A6-23).
//! - **One focus per move** (and per diagram, drawn by the tab): the prose is read, never a stop
//!   on the way with the keyboard (A6-07: 74 focusables on a page, 50 of them prose).
//! - **The move's state on two axes, never by colour alone** (WCAG 1.4.1, A6-06): the walk's
//!   state ([`MoveStatus`]) is the text's colour, a token that reads at 4.5:1 in both themes
//!   (WCAG 1.4.3; the unchecked grey was 2.61:1, the caption blue 4.06:1, R5-12), **and** a shape
//!   painted before the move ([`marker`]); the link to the book's PGN is the solid underline (read
//!   from the PGN, the PGN's move chosen) or italics (in conflict, not decided). Both are said in
//!   the move's name too ([`move_name`]): "6. bishop b5, check, not checked, read from the book's
//!   PGN".

use std::ops::Range;

use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontId, Stroke};
use pdfcraft_engine::chess::pdfcraft_chessvision::doc::{Inline, MoveStatus};

use crate::theme::Tokens;

/// The colour of a move's text for its walk state: a token, never a fixed colour.
pub(crate) fn status_colour(s: MoveStatus, t: &Tokens) -> Color32 {
    match s {
        MoveStatus::Valid => t.text,
        MoveStatus::Illegal | MoveStatus::Ambiguous => t.danger_text,
        MoveStatus::OutOfSequence | MoveStatus::NotReached => t.warning_text,
        MoveStatus::Unchecked => t.text_muted,
        MoveStatus::Caption => t.accent_text,
    }
}

/// The shape painted before a move for its walk state: none for a valid one.
pub(crate) fn marker(s: MoveStatus) -> &'static str {
    match s {
        MoveStatus::Valid => "",
        MoveStatus::Illegal => "×",
        MoveStatus::Ambiguous => "×?",
        MoveStatus::OutOfSequence | MoveStatus::NotReached => "?",
        MoveStatus::Unchecked => "·",
        MoveStatus::Caption => "▸",
    }
}

/// What the walk found of a move, in words: the Text tab's tooltip and the capture's preview say
/// it so (plan A9-I6, findings A5-19 and A7-19: they had a copy each), and the exports say a
/// review's reason in them (`pdfcraft_chess::spoken`).
pub(crate) fn move_status_words(s: MoveStatus) -> &'static str {
    match s {
        MoveStatus::Valid => tl!("Valid move"),
        MoveStatus::Illegal => tl!("Illegal move"),
        MoveStatus::Ambiguous => tl!("Ambiguous move"),
        MoveStatus::OutOfSequence => tl!("Move out of sequence"),
        MoveStatus::NotReached => tl!("Not reached: an earlier move failed"),
        MoveStatus::Unchecked => tl!("Unchecked: no position to play it from"),
        MoveStatus::Caption => tl!("Caption: the move that led to the diagram, which gives it its side to move and move number"),
    }
}

/// The walk state in a few words, for the move's name: none for a valid one.
pub(crate) fn status_words(s: MoveStatus) -> Option<&'static str> {
    match s {
        MoveStatus::Valid => None,
        MoveStatus::Illegal => Some(tl!("illegal")),
        MoveStatus::Ambiguous => Some(tl!("ambiguous")),
        MoveStatus::OutOfSequence => Some(tl!("out of sequence")),
        MoveStatus::NotReached => Some(tl!("not reached")),
        MoveStatus::Unchecked => Some(tl!("not checked")),
        MoveStatus::Caption => Some(tl!("caption")),
    }
}

/// A move's name for assistive technology: its number, the move in words, its state and how it
/// is linked to the book's PGN (`linked`), each after a comma.
pub(crate) fn move_name(number: Option<&str>, san: &str, status: MoveStatus, linked: Option<&str>) -> String {
    let mut name = crate::chess_figurines::spoken(san, "");
    if let Some(n) = number.filter(|n| !n.is_empty()) {
        name = format!("{n} {name}");
    }
    for said in [status_words(status), linked].into_iter().flatten() {
        name = format!("{name}, {said}");
    }
    name
}

/// How a move is drawn in its paragraph.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Look {
    pub(crate) colour: Color32,
    /// Its state's shape, painted before it.
    pub(crate) marker: &'static str,
    /// Read from the book's PGN, or the PGN's move chosen: underlined.
    pub(crate) underline: bool,
    /// In conflict with the book's PGN, not decided: in italics.
    pub(crate) italics: bool,
}

/// A paragraph laid out as one job: its text, and each move's place in it.
pub(crate) struct Paragraph {
    pub(crate) job: LayoutJob,
    /// Each move: its inline's index, and the characters it takes in the job's text (its marker
    /// with it), as galley cursors count them.
    pub(crate) moves: Vec<(usize, Range<usize>)>,
    /// Each inline's characters, for a click on the prose.
    pub(crate) inlines: Vec<Range<usize>>,
}

/// `inlines` as one job in `font`, `wrap` wide: each inline's text (`text_of`), the moves drawn
/// as `look_of` says and the rest in `prose`, with what `after_of` gives after an inline (the
/// engine's evaluation after a move and its signs, plan A9-N17b) a little smaller in its colour,
/// part of that inline. Spaced as the exports space them: none before a sign or closing
/// punctuation, nor after an opening bracket.
pub(crate) fn paragraph(
    inlines: &[Inline],
    font: &FontId,
    prose: Color32,
    wrap: f32,
    text_of: impl Fn(&Inline) -> String,
    look_of: impl Fn(usize, &Inline) -> Option<Look>,
    after_of: impl Fn(usize) -> Option<(String, Color32)>,
) -> Paragraph {
    let mut job = LayoutJob { wrap: egui::text::TextWrapping::wrap_at_width(wrap.max(1.0)), ..Default::default() };
    let mut chars = 0usize;
    let mut moves = Vec::new();
    let mut spans = Vec::with_capacity(inlines.len());
    let plain = TextFormat { font_id: font.clone(), color: prose, ..Default::default() };
    for (k, inline) in inlines.iter().enumerate() {
        let text = text_of(inline);
        let glued = matches!(inline, Inline::Nag { .. }) || text.starts_with([')', ']', ',', '.', ';', ':', '!', '?']);
        if !job.text.is_empty() && !glued && !job.text.ends_with(['(', '[', ' ']) {
            job.append(" ", 0.0, plain.clone());
            chars = chars.saturating_add(1);
        }
        let start = chars;
        match look_of(k, inline) {
            Some(look) => {
                // A move with no text would take no place, and no stop (plan A9-I9b).
                let text = if text.is_empty() { "…".to_owned() } else { text };
                let format = TextFormat {
                    font_id: font.clone(),
                    color: look.colour,
                    italics: look.italics,
                    underline: if look.underline { Stroke::new(1.0, look.colour) } else { Stroke::NONE },
                    ..Default::default()
                };
                if !look.marker.is_empty() {
                    // A thin space between the shape and the move, in the move's colour.
                    let shape = format!("{}\u{2009}", look.marker);
                    chars = chars.saturating_add(shape.chars().count());
                    job.append(&shape, 0.0, TextFormat { underline: Stroke::NONE, italics: false, ..format.clone() });
                }
                chars = chars.saturating_add(text.chars().count());
                job.append(&text, 0.0, format);
                moves.push((k, start..chars));
            }
            None => {
                chars = chars.saturating_add(text.chars().count());
                job.append(&text, 0.0, plain.clone());
            }
        }
        if let Some((text, colour)) = after_of(k) {
            // A thin space, then the text, 85 % of the font's size.
            let after = format!("\u{2009}{text}");
            chars = chars.saturating_add(after.chars().count());
            let small = FontId::new(font.size * 0.85, font.family.clone());
            job.append(&after, 0.0, TextFormat { font_id: small, color: colour, ..Default::default() });
        }
        spans.push(start..chars);
    }
    Paragraph { job, moves, inlines: spans }
}

/// The marks of a move's rows (`parts`: each row's rectangle, whether the pointer is on it and
/// whether it has the keyboard; only the first row takes the keyboard): every row is pointed at
/// when one is, and shows the keyboard's ring when the move has it (plan A9-I9b).
pub(crate) fn marks(parts: &[(egui::Rect, bool, bool)]) -> Vec<(egui::Rect, bool, bool)> {
    let hovered = parts.iter().any(|(_, h, _)| *h);
    let focused = parts.first().is_some_and(|(_, _, f)| *f);
    parts.iter().map(|(r, _, _)| (*r, hovered, focused)).collect()
}

/// The rectangles characters `range` of `galley` take, one per row they are on, in the galley's
/// own coordinates.
pub(crate) fn rects(galley: &egui::Galley, range: &Range<usize>) -> Vec<egui::Rect> {
    use egui::text::CCursor;
    let mut out: Vec<egui::Rect> = Vec::new();
    for c in range.clone() {
        let a = galley.pos_from_cursor(CCursor::new(c));
        let b = galley.pos_from_cursor(CCursor::new(c.saturating_add(1)));
        // The next character on another row: this one ends its row.
        let right = if (b.min.y - a.min.y).abs() < 0.5 {
            b.min.x
        } else {
            galley.rows.iter().find(|r| r.rect().contains(a.center())).map_or(a.max.x, |r| r.rect().max.x)
        };
        let glyph = egui::Rect::from_min_max(a.min, egui::pos2(right.max(a.min.x), a.max.y));
        match out.last_mut() {
            Some(last) if (last.min.y - glyph.min.y).abs() < 0.5 => *last = last.union(glyph),
            _ => out.push(glyph),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{ThemeKind, Tokens};

    const ALL: [MoveStatus; 7] = [
        MoveStatus::Valid,
        MoveStatus::Illegal,
        MoveStatus::Ambiguous,
        MoveStatus::OutOfSequence,
        MoveStatus::NotReached,
        MoveStatus::Unchecked,
        MoveStatus::Caption,
    ];

    /// Plan A9-I6 (findings A5-19, A7-19): one function says a move's state in words, for the Text
    /// tab's tooltip and the capture's preview; its words are those the two copies it replaced
    /// gave (run before the change), each its own.
    #[test]
    fn a_moves_state_in_words_is_as_before() {
        let words: Vec<&str> = ALL.iter().map(|s| move_status_words(*s)).collect();
        assert_eq!(
            words,
            [
                "Valid move",
                "Illegal move",
                "Ambiguous move",
                "Move out of sequence",
                "Not reached: an earlier move failed",
                "Unchecked: no position to play it from",
                "Caption: the move that led to the diagram, which gives it its side to move and move number",
            ]
        );
    }

    /// WCAG 2 contrast ratio between two opaque colours.
    fn contrast(a: Color32, b: Color32) -> f32 {
        let lum = |c: Color32| {
            let lin = |v: u8| {
                let s = f32::from(v) / 255.0;
                if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
            };
            0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
        };
        let (x, y) = (lum(a) + 0.05, lum(b) + 0.05);
        x.max(y) / x.min(y)
    }

    /// Plan A9-I9 (A6-06, R5-12, WCAG 1.4.3): every walk state's text reads at 4.5:1 on the
    /// panel and on a card, in both themes; and every state but a valid move has a shape too
    /// (WCAG 1.4.1), each its own but for the two warnings.
    #[test]
    fn every_move_state_reads_and_has_a_shape() {
        for kind in [ThemeKind::Light, ThemeKind::Dark] {
            let t = Tokens::for_kind(kind);
            for s in ALL {
                // A new state needs a token here: the match in `status_colour` is exhaustive.
                for (surface, ground) in [("panel", t.panel), ("card", t.card)] {
                    let c = contrast(status_colour(s, &t), ground);
                    assert!(c >= 4.5, "{kind:?} {s:?} on the {surface}: {c:.2}:1");
                }
            }
        }
        for s in ALL {
            assert_eq!(marker(s).is_empty(), s == MoveStatus::Valid, "{s:?}");
            assert_eq!(status_words(s).is_none(), s == MoveStatus::Valid, "{s:?}");
        }
        assert_eq!(marker(MoveStatus::Illegal), "×");
    }

    /// Plan A9-I9b: a move's state reads at 4.5:1 on the fill of the move pointed at too, in both
    /// themes.
    #[test]
    fn every_move_state_reads_on_the_pointed_fill() {
        for kind in [ThemeKind::Light, ThemeKind::Dark] {
            let t = Tokens::for_kind(kind);
            for s in ALL {
                let c = contrast(status_colour(s, &t), t.hover);
                assert!(c >= 4.5, "{kind:?} {s:?} on the hover fill: {c:.2}:1");
            }
        }
    }

    fn a_move(text: &str) -> Inline {
        let json = serde_json::json!({ "type": "move", "text": text, "san": "e4", "status": "valid" });
        serde_json::from_value(json).expect("a move inline")
    }

    fn look() -> Look {
        Look { colour: Color32::BLACK, marker: "", underline: false, italics: false }
    }

    /// Plan A9-I9b (A6-23): the rows of a paragraph are as tall as each other, its moves (with
    /// their shapes, underline and italics) and its prose alike.
    #[test]
    fn a_paragraphs_rows_are_as_tall_as_each_other() {
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let ctx = ui.ctx().clone();
            let inlines: Vec<Inline> = (0..40usize).map(|i| if i.is_multiple_of(3) { a_move("Nf3") } else { a_move("e4") }).collect();
            let looks = |k: usize| Look {
                marker: if k.is_multiple_of(2) { "×?" } else { "" },
                underline: k.is_multiple_of(3),
                italics: k.is_multiple_of(5),
                ..look()
            };
            let para = paragraph(
                &inlines,
                &FontId::proportional(13.0),
                Color32::BLACK,
                120.0,
                |i| if let Inline::Move(m) = i { m.text.clone() } else { String::new() },
                |k, _| Some(looks(k)),
                // An evaluation after every fourth move: the rows stay as tall as each other.
                |k| k.is_multiple_of(4).then(|| ("+0.50".to_owned(), Color32::GRAY)),
            );
            let galley = ctx.fonts_mut(|f| f.layout_job(para.job));
            let heights: Vec<f32> = galley.rows.iter().map(|r| r.rect().height()).collect();
            assert!(heights.len() > 3, "it wraps: {heights:?}");
            assert!(heights.iter().all(|h| (h - heights[0]).abs() < 0.01), "{heights:?}");
        });
        // The font atlas the frame made is not uploaded anywhere here.
        out.textures_delta.clear();
    }

    /// Plan A9-I9b: a move with no text still takes a place, so it keeps its stop.
    #[test]
    fn a_move_with_no_text_keeps_a_place() {
        let para = paragraph(&[a_move("")], &FontId::proportional(13.0), Color32::BLACK, 200.0, |_| String::new(), |_, _| Some(look()), |_| None);
        assert!(para.moves.first().is_some_and(|(_, r)| !r.is_empty()), "{:?}", para.moves);
    }

    /// Plan A9-I9b: a move wrapped onto two rows is pointed at, and has the keyboard's ring, on both.
    #[test]
    fn a_wrapped_moves_rows_are_marked_together() {
        let (a, b) = (
            egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(10.0, 10.0)),
            egui::Rect::from_min_size(egui::pos2(0.0, 12.0), egui::vec2(10.0, 10.0)),
        );
        assert_eq!(marks(&[(a, false, true), (b, true, false)]), [(a, true, true), (b, true, true)]);
        assert_eq!(marks(&[(a, false, false), (b, false, false)]), [(a, false, false), (b, false, false)]);
        assert!(marks(&[]).is_empty());
    }

    /// Plan A9-I9: the move's name says its number, the move in words, its state and its link.
    #[test]
    fn a_moves_name_says_its_state_and_its_link() {
        assert_eq!(
            move_name(Some("6."), "Bb5+", MoveStatus::Unchecked, Some("read from the book's PGN")),
            "6. bishop b5, check, not checked, read from the book's PGN"
        );
        assert_eq!(move_name(None, "e4", MoveStatus::Valid, None), "e4");
    }
}
