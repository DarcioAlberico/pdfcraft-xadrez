//! Chess ▸ PGN toolbar ▸ Correcting a capture (T10): a move of the preview that did not play is
//! corrected there, and the whole text is read again (PGN toolbar spec §4.3.4, §4.6).
//!
//! A ✗ chip of the preview (a move the reader could not play, or the one the graft stopped at)
//! opens a small panel under the chips with three ways to correct it:
//!
//! - **The OCR's readings.** When the text came from an OCR area of a scan (T9), the glyphs of
//!   the move are listed, a doubtful one (its top two readings within [`LOW_MARGIN`]) underlined;
//!   a click on one lists the reader's top three readings of it, and picking one puts it in place
//!   of the glyph.
//! - **The legal moves near it** (`engine::chess::capture::stopped_moves`): what the legality
//!   repair makes of the move in the position it was to be played in, at most three, in the
//!   language the text is read in. They are **never applied without a click**: guessing a move
//!   goes wrong (the lesson of 2026-10-19).
//! - **A correction field**, holding the move as printed, with the full glyph palette inserting
//!   at its cursor; Enter (or Read again) takes it.
//!
//! Every correction rewrites the move in the captured text itself (the glyphs of a page or of an
//! OCR area, keeping where they were printed; or the pasted text) and plans the capture again, so
//! the preview shows what the corrected text reads as. Nothing goes into the game until Apply.
//! A correction that fits nowhere is refused and the text is left as it was.

use std::sync::Arc;

use egui::{Key, RichText};
use pdfcraft_engine::chess::capture::{StoppedMove, move_word};
use pdfcraft_engine::chess::pdfcraft_chess::san::{Lang, Style};
use pdfcraft_engine::chess::pdfcraft_chessvision::coords::{PageGeometry, PageRect};
use pdfcraft_engine::chess::text_ocr::AreaRead;
use pdfcraft_render::text::{PageText, TextGlyph};

use crate::chess_capture::{Pending, Request, Text};
use crate::chess_ui::ChessState;
use crate::theme::Tokens;

/// A glyph whose top two readings are this close (`1 − p2 / p1` below it: the first less than
/// twice as likely as the second) is doubtful, and underlined.
pub(crate) const LOW_MARGIN: f32 = 0.5;

/// Longest correction typed, in characters (a move is a dozen at most).
const MAX_FIELD_CHARS: usize = 64;

/// The correction panel of one stopped move, kept with the capture waiting.
#[derive(Clone, Debug, Default)]
pub struct Fix {
    /// The stopped move, by its place in [`Pending::stopped`].
    pub stop: usize,
    /// The correction field.
    pub field: String,
    /// The OCR glyph whose readings are listed (an index into `AreaRead::alternatives`).
    pub glyph: Option<usize>,
    /// The glyph palette is shown under the field.
    pub palette: bool,
}

/// A correction asked for in the panel.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Change {
    /// The stopped move `stop` is `text` instead.
    Word { stop: usize, text: String },
    /// OCR glyph `alt` (an index into `AreaRead::alternatives`) is `text`.
    Glyph { alt: usize, text: String },
}

impl Fix {
    /// The panel for stopped move `stop` (its index), the field holding the move as printed.
    pub(crate) fn open(stop: usize, s: &StoppedMove) -> Fix {
        Fix { stop, field: s.word.clone(), glyph: None, palette: false }
    }
}

/// `rect` (user space) in view space, by `geometry`.
fn to_view(geometry: Option<&PageGeometry>, rect: Option<[f32; 4]>) -> Option<[f32; 4]> {
    let [x0, y0, x1, y1] = rect?;
    let v = geometry?.user_to_view(PageRect::new(x0, y0, x1, y1))?;
    Some([v.x0.min(v.x1), v.y0.min(v.y1), v.x0.max(v.x1), v.y0.max(v.y1)])
}

/// Whether the centre of `r` is inside `area` (grown by half a point).
fn centred_in(r: [f32; 4], area: [f32; 4]) -> bool {
    let (cx, cy) = ((r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0);
    cx >= area[0] - 0.5 && cx <= area[2] + 0.5 && cy >= area[1] - 0.5 && cy <= area[3] + 0.5
}

/// The OCR glyphs of stopped move `s` (indices into `read.alternatives`), left to right.
pub(crate) fn move_glyphs(read: &AreaRead, geometry: Option<&PageGeometry>, s: &StoppedMove) -> Vec<usize> {
    let Some(area) = to_view(geometry, s.rect) else { return Vec::new() };
    let mut out: Vec<usize> = read.alternatives.iter().enumerate().filter(|(_, a)| centred_in(a.rect, area)).map(|(i, _)| i).collect();
    out.sort_by(|a, b| {
        let x = |i: &usize| read.alternatives.get(*i).map_or(0.0, |a| a.rect[0]);
        x(a).total_cmp(&x(b))
    });
    out
}

// ─── The panel ───────────────────────────────────────────────────────────────────────────────

/// The panel of `fix` for stopped move `s`, of a capture read in `lang`; `read` is what OCR read
/// when the text came from an area of a scan, with the page's geometry. Returns the correction
/// asked for.
pub(crate) fn panel(
    ui: &mut egui::Ui,
    t: &Tokens,
    fix: &mut Fix,
    s: &StoppedMove,
    lang: Lang,
    read: Option<(&AreaRead, Option<&PageGeometry>)>,
) -> Option<Change> {
    let mut change = None;
    ui.separator();
    ui.label(RichText::new(crate::i18n::fmt(tl!("Correct {move}"), &[("move", &s.text)])).strong());
    // (a) The OCR's readings of the move's glyphs.
    if let Some((read, geometry)) = read {
        let glyphs = move_glyphs(read, geometry, s);
        if !glyphs.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(tl!("Characters read (underlined: doubtful)")).small().color(t.text_muted));
                for &i in &glyphs {
                    let Some(a) = read.alternatives.get(i) else { continue };
                    let doubtful = a.margin < LOW_MARGIN;
                    let mut text = RichText::new(&a.text).monospace();
                    if doubtful {
                        text = text.underline();
                    }
                    let r = ui.selectable_label(fix.glyph == Some(i), text);
                    let name = if doubtful {
                        crate::i18n::fmt(tl!("Character {glyph}, doubtful"), &[("glyph", &a.text)])
                    } else {
                        crate::i18n::fmt(tl!("Character {glyph}"), &[("glyph", &a.text)])
                    };
                    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, fix.glyph == Some(i), &name));
                    if r.clicked() {
                        fix.glyph = if fix.glyph == Some(i) { None } else { Some(i) };
                    }
                }
            });
            if let Some(a) = fix.glyph.filter(|g| glyphs.contains(g)).and_then(|g| read.alternatives.get(g).map(|a| (g, a))) {
                let (alt, a) = a;
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(tl!("Read it as")).small().color(t.text_muted));
                    for (text, p) in a.choices.iter().take(3) {
                        let percent = if p.is_finite() { (p.clamp(0.0, 1.0) * 100.0).round() as u32 } else { 0 };
                        let r = ui.button(RichText::new(format!("{text} · {percent}%")).monospace());
                        let name = crate::i18n::fmt(tl!("Read the character as {glyph}"), &[("glyph", text)]);
                        r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
                        if r.clicked() && *text != a.text {
                            change = Some(Change::Glyph { alt, text: text.clone() });
                        }
                    }
                });
            }
        }
    }
    // (b) The legal moves near it: offered, never applied without a click.
    if s.near.is_empty() {
        let why = if s.fen.is_none() { tl!("Not reached: correct the move that stopped before it first.") } else { tl!("No legal move is near it.") };
        ui.label(RichText::new(why).small().color(t.text_muted));
    } else {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(tl!("Legal moves near it")).small().color(t.text_muted));
            for san in &s.near {
                let text = san.write(lang, Style::Letters);
                let r = ui.button(RichText::new(&text).monospace());
                let name = crate::i18n::fmt(tl!("Read as {move}"), &[("move", &text)]);
                r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
                if r.on_hover_text(&name).clicked() {
                    change = Some(Change::Word { stop: fix.stop, text: text.clone() });
                }
            }
        });
    }
    // (c) The correction field, with the glyph palette.
    let id = ui.make_persistent_id("chess-capture-correction");
    ui.horizontal(|ui| {
        let label = ui.label(RichText::new(tl!("Correction")).small().color(t.text_muted));
        let r = ui
            .add(egui::TextEdit::singleline(&mut fix.field).id(id).char_limit(MAX_FIELD_CHARS).desired_width(140.0).font(egui::TextStyle::Monospace));
        r.clone().labelled_by(label.id);
        let entered = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        let again = ui.button(tl!("Read again")).on_hover_text(tl!("Read the text again with the move corrected"));
        if (entered || again.clicked()) && !fix.field.trim().is_empty() {
            change = Some(Change::Word { stop: fix.stop, text: fix.field.trim().to_owned() });
        }
        let r = ui.selectable_label(fix.palette, tl!("Glyph palette"));
        r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, fix.palette, tl!("Glyph palette")));
        if r.clicked() {
            fix.palette = !fix.palette;
        }
    });
    if fix.palette
        && let Some(insert) = crate::chess_annotate::palette_buttons(ui, true)
        && fix.field.chars().count().saturating_add(insert.chars().count()) <= MAX_FIELD_CHARS
    {
        crate::chess_annotate::insert_at_cursor(ui, id, &mut fix.field, &insert);
    }
    change
}

// ─── Rewriting the text ──────────────────────────────────────────────────────────────────────

/// Plan the capture waiting again with `change` made to its text. Refused (with a notice, the
/// capture left as it was) when the move can't be found in the text or the corrected text fits
/// nowhere.
pub(crate) fn correct(chess: &mut ChessState, change: Change) {
    let Some(p) = chess.capture.pending.as_ref() else { return };
    let Some(request) = rewrite(p, &change) else {
        chess.capture.note(tl!("The correction couldn't be made: the move wasn't found in the text.").to_owned());
        return;
    };
    let plan = crate::chess_capture::plan_for(&chess.board().game, &request);
    if plan.chosen.is_none() {
        chess.capture.note(tl!("The corrected text fits nowhere: nothing was changed.").to_owned());
        return;
    }
    chess.capture.pending = Some(Pending::new(request, plan));
}

/// The request of `p` with `change` made to its text; `None` when it can't be made.
fn rewrite(p: &Pending, change: &Change) -> Option<Request> {
    let mut request = p.request.clone();
    match change {
        Change::Word { stop, text } => {
            let s = p.stopped.get(*stop)?;
            request.text = rewrite_word(&p.request.text, s, text)?;
        }
        Change::Glyph { alt, text } => {
            let Text::Page { page, geometry, rects, read: Some(read), .. } = &p.request.text else { return None };
            let a = read.alternatives.get(*alt)?;
            let mut read = (**read).clone();
            let at = read.text.glyphs.iter().position(|g| same_box(g.rect, a.rect))?;
            read.text.glyphs.get_mut(at)?.text.clone_from(text);
            read.alternatives.get_mut(*alt)?.text.clone_from(text);
            request.text = page_text(*page, *geometry, rects.clone(), read);
        }
    }
    Some(request)
}

/// Two boxes that are the same glyph's.
fn same_box(a: [f32; 4], b: [f32; 4]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.01)
}

/// The text of a page read by OCR, after `read` changed.
fn page_text(page: usize, geometry: Option<PageGeometry>, rects: Vec<[f32; 4]>, read: AreaRead) -> Text {
    Text::Page { page, glyphs: Arc::new(read.text.glyphs.clone()), geometry, rects, read: Some(Arc::new(read)) }
}

/// `text` with stopped move `s` written `word` instead.
fn rewrite_word(text: &Text, s: &StoppedMove, word: &str) -> Option<Text> {
    let word = word.trim();
    if word.is_empty() {
        return None;
    }
    match text {
        Text::Plain(plain) => Some(Text::Plain(Arc::from(replace_in_plain(plain, s, word)?))),
        Text::Page { page, glyphs, geometry, rects, read: None } => {
            let area = to_view(geometry.as_ref(), s.rect)?;
            let range = word_range(glyphs, area)?;
            let new = splice(glyphs, range, word)?;
            Some(Text::Page { page: *page, glyphs: Arc::new(new), geometry: *geometry, rects: rects.clone(), read: None })
        }
        Text::Page { page, geometry, rects, read: Some(read), .. } => {
            let area = to_view(geometry.as_ref(), s.rect)?;
            let range = word_range(&read.text.glyphs, area)?;
            let old: Vec<[f32; 4]> = read.text.glyphs.get(range.clone())?.iter().map(|g| g.rect).collect();
            let mut new = (**read).clone();
            new.text = splice_page(&read.text, range, word)?;
            // A glyph of the move that kept its box keeps its readings, with its new text; the
            // others no longer have a glyph of their own.
            let AreaRead { text: layer, alternatives, .. } = &mut new;
            alternatives.retain_mut(|a| {
                if !old.iter().any(|r| same_box(*r, a.rect)) {
                    return true;
                }
                match layer.glyphs.iter().find(|g| same_box(g.rect, a.rect)) {
                    Some(g) => {
                        a.text.clone_from(&g.text);
                        true
                    }
                    None => false,
                }
            });
            Some(page_text(*page, *geometry, rects.clone(), new))
        }
    }
}

/// The glyphs of the move printed in `area` (view space): those whose centre is in it, without
/// the move number in front (`19.` stays as printed). `None` when there are none.
fn word_range(glyphs: &[TextGlyph], area: [f32; 4]) -> Option<std::ops::Range<usize>> {
    let inside: Vec<usize> = glyphs.iter().enumerate().filter(|(_, g)| centred_in(g.rect, area)).map(|(i, _)| i).collect();
    let (&first, &last) = (inside.first()?, inside.last()?);
    // The longest run of glyphs from the first that reads as a move number alone.
    let mut start = first;
    let mut acc = String::new();
    for i in first..=last {
        acc.push_str(&glyphs.get(i)?.text);
        if acc.starts_with(|c: char| c.is_ascii_digit()) && move_word(&acc).is_empty() {
            start = i.saturating_add(1);
        }
    }
    (start <= last).then(|| start..last.saturating_add(1))
}

/// `glyphs` with those in `range` replaced by `word`, a glyph per character sharing their box
/// along the line, in the font of the last one replaced (the squares are set in the text font,
/// never in a figurine font, so the weight is kept and nothing is read through a figurine
/// catalogue).
fn splice(glyphs: &[TextGlyph], range: std::ops::Range<usize>, word: &str) -> Option<Vec<TextGlyph>> {
    let old = glyphs.get(range.clone())?;
    let new = word_glyphs(old, word)?;
    let mut out = Vec::with_capacity(glyphs.len().saturating_add(new.len()));
    out.extend_from_slice(glyphs.get(..range.start)?);
    out.extend(new);
    out.extend_from_slice(glyphs.get(range.end..)?);
    Some(out)
}

/// [`splice`] in a page's text, the new glyphs on the line of the first replaced, with the word
/// gap before it kept.
fn splice_page(text: &PageText, range: std::ops::Range<usize>, word: &str) -> Option<PageText> {
    let old = text.glyphs.get(range.clone())?;
    let new = word_glyphs(old, word)?;
    let line = *text.line_of.get(range.start)?;
    let gap = *text.space_before.get(range.start)?;
    let n = new.len();
    let mut glyphs = Vec::with_capacity(text.glyphs.len().saturating_add(n));
    let mut line_of = Vec::with_capacity(glyphs.capacity());
    let mut space_before = Vec::with_capacity(glyphs.capacity());
    glyphs.extend_from_slice(text.glyphs.get(..range.start)?);
    line_of.extend_from_slice(text.line_of.get(..range.start)?);
    space_before.extend_from_slice(text.space_before.get(..range.start)?);
    glyphs.extend(new);
    line_of.extend(std::iter::repeat_n(line, n));
    space_before.extend((0..n).map(|k| k == 0 && gap));
    glyphs.extend_from_slice(text.glyphs.get(range.end..)?);
    line_of.extend_from_slice(text.line_of.get(range.end..)?);
    space_before.extend_from_slice(text.space_before.get(range.end..)?);
    Some(PageText { glyphs, line_of, space_before })
}

/// The glyphs of `word` in place of `old`: one per character. With as many characters as glyphs,
/// each takes the box of the glyph it replaces; otherwise they share the boxes' span evenly.
fn word_glyphs(old: &[TextGlyph], word: &str) -> Option<Vec<TextGlyph>> {
    let last = old.last()?;
    let chars: Vec<char> = word.chars().take(MAX_FIELD_CHARS).collect();
    if chars.is_empty() {
        return None;
    }
    let font = last.font.clone();
    let glyph = |c: char, rect: [f32; 4], template: &TextGlyph| {
        let mut g = TextGlyph::new(c.to_string(), rect, quad(rect), template.direction).with_source(font.clone(), None);
        g.invisible = template.invisible;
        g
    };
    if chars.len() == old.len() {
        return Some(chars.iter().zip(old).map(|(&c, o)| if o.text == c.to_string() { o.clone() } else { glyph(c, o.rect, last) }).collect());
    }
    let x0 = old.iter().map(|g| g.rect[0]).fold(f32::INFINITY, f32::min);
    let x1 = old.iter().map(|g| g.rect[2]).fold(f32::NEG_INFINITY, f32::max);
    let y0 = old.iter().map(|g| g.rect[1]).fold(f32::INFINITY, f32::min);
    let y1 = old.iter().map(|g| g.rect[3]).fold(f32::NEG_INFINITY, f32::max);
    if ![x0, x1, y0, y1].iter().all(|v| v.is_finite()) {
        return None;
    }
    // At most 64 characters: the count is exact in f32.
    let step = (x1 - x0).max(0.0) / chars.len() as f32;
    Some(chars.iter().enumerate().map(|(k, &c)| glyph(c, [x0 + step * k as f32, y0, x0 + step * (k as f32 + 1.0), y1], last)).collect())
}

/// The em box of an upright glyph drawn in `rect` (view space, y down): bottom edge left to
/// right, then the top edge right to left.
fn quad(r: [f32; 4]) -> [[f32; 2]; 4] {
    [[r[0], r[3]], [r[2], r[3]], [r[2], r[1]], [r[0], r[1]]]
}

/// `plain` with stopped move `s` written `word` instead: the first time the move is printed after
/// its number (`19.Nxe6+`, `19. Nxe6+`), else the first time it stands as a word of its own.
fn replace_in_plain(plain: &str, s: &StoppedMove, word: &str) -> Option<String> {
    let old = s.word.trim();
    if old.is_empty() {
        return None;
    }
    let number = s.text.trim().strip_suffix(old).map(str::trim).unwrap_or_default();
    let boundary = |c: Option<char>| c.is_none_or(|c| !c.is_alphanumeric());
    let mut fallback = None;
    for (at, _) in plain.match_indices(old) {
        let end = at.saturating_add(old.len());
        let (before, after) = (plain.get(..at)?, plain.get(end..)?);
        // A move is a word of its own: nothing alphanumeric glued to it (its number is not).
        let glued_number = !number.is_empty() && before.trim_end().ends_with(number);
        if !boundary(after.chars().next()) || !(boundary(before.chars().next_back()) || glued_number) {
            continue;
        }
        if glued_number {
            return Some(format!("{before}{word}{after}"));
        }
        fallback.get_or_insert(at);
    }
    let at = fallback?;
    let (before, after) = (plain.get(..at)?, plain.get(at.saturating_add(old.len())..)?);
    Some(format!("{before}{word}{after}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stop(text: &str) -> StoppedMove {
        StoppedMove { text: text.into(), word: move_word(text).into(), rect: None, fen: None, near: Vec::new(), graft: false }
    }

    #[test]
    fn pasted_text_is_corrected_where_the_move_was_printed() {
        let text = "18.Nd5 Qxe1 19.Nxe6+ Kh7 (or 19.Nxe6+ at once)";
        assert_eq!(replace_in_plain(text, &stop("19.Nxe6+"), "Nxe7+").as_deref(), Some("18.Nd5 Qxe1 19.Nxe7+ Kh7 (or 19.Nxe6+ at once)"));
        assert_eq!(replace_in_plain("19. Nxe6+ Kh7", &stop("19.Nxe6+"), "Nxe7+").as_deref(), Some("19. Nxe7+ Kh7"));
        // A move without a number: the first time it stands alone ("Kh7" in "Kh7x" is not it).
        assert_eq!(replace_in_plain("Kh7x then Kh7 and", &stop("Kh7"), "Kh8").as_deref(), Some("Kh7x then Kh8 and"));
        assert_eq!(replace_in_plain("nothing here", &stop("Kh7"), "Kh8"), None);
        assert_eq!(replace_in_plain("19.", &stop("19."), "Kh8"), None);
    }

    fn glyphs(text: &str) -> Vec<TextGlyph> {
        text.chars()
            .enumerate()
            .filter(|(_, c)| *c != ' ')
            .map(|(i, c)| {
                let x = i as f32 * 6.0;
                let r = [x, 0.0, x + 6.0, 10.0];
                TextGlyph::new(c.to_string(), r, quad(r), [1.0, 0.0])
            })
            .collect()
    }

    fn joined(g: &[TextGlyph]) -> String {
        g.iter().map(|g| g.text.as_str()).collect()
    }

    #[test]
    fn a_page_move_is_rewritten_in_its_box_keeping_its_number() {
        // "19.Nxe6+ Kh7": the move's box covers its number too.
        let g = glyphs("19.Nxe6+ Kh7");
        let range = word_range(&g, [0.0, 0.0, 48.0, 10.0]).unwrap();
        assert_eq!(joined(&g[range.clone()]), "Nxe6+");
        let out = splice(&g, range.clone(), "Nxe7+").unwrap();
        assert_eq!(joined(&out), "19.Nxe7+Kh7");
        assert_eq!(out[6].rect, g[6].rect, "same count: each keeps its box");
        let out = splice(&g, range, "Nf6").unwrap();
        assert_eq!(joined(&out), "19.Nf6Kh7");
        // Castling with zeros is no number.
        let g = glyphs("0-0");
        assert_eq!(word_range(&g, [0.0, 0.0, 18.0, 10.0]), Some(0..3));
        // Nothing in the box, or only a number.
        assert_eq!(word_range(&g, [100.0, 0.0, 120.0, 10.0]), None);
        assert_eq!(word_range(&glyphs("19."), [0.0, 0.0, 18.0, 10.0]), None);
        assert!(splice(&glyphs("ab"), 5..9, "x").is_none());
        assert!(word_glyphs(&[], "x").is_none());
    }
}
