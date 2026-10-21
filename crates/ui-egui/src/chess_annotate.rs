//! Chess ▸ PGN toolbar (T6): the annotation glyphs on the rail, glyphs typed on the keyboard, the
//! comment before and after a move, and the basic glyph palette.
//!
//! **Glyphs** (spec §4.4) are the 28 of `pdfcraft_chess::nag::NAGS`, in three rail groups: the
//! move's quality, the evaluation, and the others. A glyph goes on the move shown with the code
//! for the side that made it (zugzwang: the side to move after it), and a click on a glyph the
//! move has takes it off. A quality glyph replaces the move's other one, and an evaluation the
//! other evaluation (`Game::set_nag`); the others add up. Each change is one undo step. Their
//! names, read by assistive technology, are the glyphs' names in the interface language, never
//! the signs; their faces are the signs, or the way books set them in plain text (`+=`, `=+`)
//! where the interface fonts lack them.
//!
//! **The glyph brush** (T15, a mode of the rail, B): while it is armed, a click on a move in the
//! notation puts the brush's glyph on that move (its side's code) or takes it off, one undo step
//! per click, and does not go to the move. A glyph chosen in a group becomes the brush's; Esc
//! disarms it.
//!
//! **Typed glyphs.** With the chess key scope on and no text field taking the keyboard, the signs
//! books use can be typed: `!` `?` `!!` `??` `!?` `?!` `=` `+=` `=+` `+/-` `-/+` `+-` `-+` and `~`
//! (unclear), and `0` takes the move's glyphs off. They arrive as text (so any keyboard layout
//! works) and wait in [`Typed`] for at most [`TYPED_WAIT`] for the next character of a longer
//! sign: a sign that no longer one starts with goes on at once, and the longest sign typed goes on
//! when the wait is over or a character that does not continue it comes.
//!
//! **Comments.** Under the notation the move shown has its comment (after the move), and the
//! comment editor (the rail's Move comment, or Enter) adds the comment before it. PGN only has a
//! place for a comment before a move that starts a variation (`( {…} 12... Nf6`); before a move
//! that continues its line the comment is written in one `{…}` with the comment after the move
//! before, and the editor says so.
//!
//! **The glyph palette** (the rail's Glyph palette, or G; U1) is a small floating window, kept
//! within the window it is drawn in (the main one or the chess board's own), that the user can
//! drag about: egui keeps where it was put for the session, and the rail whether it is open. Esc,
//! its close button, the rail's button and G close it. It holds the figurines, `½ – × † ‡` and the
//! 28 glyphs in the rail's three groups. A glyph clicked goes on the move shown, or comes off it,
//! as the rail's do (one undo step); the other characters go at the end of the move's comment (one
//! undo step). While a text field had the keyboard when it was clicked (the comment fields, FEN,
//! PGN), any character, a glyph's sign too, goes into that field at its cursor instead. Figurines
//! are inserted as the Unicode characters; their buttons show the piece letters where the
//! interface fonts lack the figurines. A click in the palette puts the chess key scope on, as a
//! click in the panel does. The correction field of a capture's preview (T10) has the same
//! buttons inline, with the ellipsis, and every one of them types there ([`palette_buttons`]).
//!
//! **A move's context menu** offers the six move-quality glyphs, the evaluations and "Remove all
//! glyphs" ([`glyph_menu`]): the same changes as the rail's glyphs, on the move clicked, without
//! going to it (as with the glyph brush).

use egui::Id;
use pdfcraft_engine::chess::pdfcraft_chess::game::NodeId;
use pdfcraft_engine::chess::pdfcraft_chess::nag;
use pdfcraft_engine::chess::pdfcraft_chess::placement::{Color, Piece, PieceKind};

use crate::chess_board_ui::BoardState;
use crate::chess_history::ChessEdit;
use crate::chess_ui::ChessState;

/// The rail's glyph groups (spec §4.2), by PGN code (White's, for the side-dependent ones). The
/// three hold the 28 glyphs of `NAGS` once each.
pub(crate) const MOVE_GLYPHS: [u16; 8] = [1, 2, 3, 4, 5, 6, 7, 146];
pub(crate) const EVALUATION_GLYPHS: [u16; 9] = [10, 13, 14, 15, 16, 17, 18, 19, 44];
pub(crate) const OTHER_GLYPHS: [u16; 11] = [32, 36, 40, 132, 140, 141, 22, 138, 142, 143, 145];

/// How long a typed sign waits for the next character of a longer one, in seconds.
pub(crate) const TYPED_WAIT: f64 = 0.6;

/// The signs that can be typed, and their glyphs (`0`: take the move's glyphs off).
const TYPED: [(&str, u16); 15] = [
    ("!", 1),
    ("?", 2),
    ("!!", 3),
    ("??", 4),
    ("!?", 5),
    ("?!", 6),
    ("=", 10),
    ("~", 13),
    ("+=", 14),
    ("=+", 15),
    ("+/-", 16),
    ("-/+", 17),
    ("+-", 18),
    ("-+", 19),
    ("0", CLEAR),
];

/// The typed sign that takes a move's glyphs off.
pub(crate) const CLEAR: u16 = 0;

/// Most characters kept from one frame's typing.
const MAX_TYPED: usize = 64;

/// Glyph `code`'s name in the interface language (either side's code), or `None` for a code not
/// in the table.
pub(crate) fn nag_name(code: u16) -> Option<&'static str> {
    Some(match nag::by_code(code)?.code {
        1 => tl!("Good move"),
        2 => tl!("Poor move"),
        3 => tl!("Brilliant move"),
        4 => tl!("Blunder"),
        5 => tl!("Interesting move"),
        6 => tl!("Dubious move"),
        7 => tl!("Only move"),
        146 => tl!("Novelty"),
        10 => tl!("Equal position"),
        13 => tl!("Unclear position"),
        14 => tl!("White is slightly better"),
        15 => tl!("Black is slightly better"),
        16 => tl!("White is clearly better"),
        17 => tl!("Black is clearly better"),
        18 => tl!("White is winning"),
        19 => tl!("Black is winning"),
        44 => tl!("Compensation for the material"),
        22 => tl!("Zugzwang"),
        32 => tl!("Development advantage"),
        36 => tl!("With the initiative"),
        40 => tl!("With an attack"),
        132 => tl!("With counterplay"),
        138 => tl!("Time trouble"),
        140 => tl!("With the idea"),
        141 => tl!("Aimed against"),
        142 => tl!("Better is"),
        143 => tl!("Worse is"),
        145 => tl!("Editorial comment"),
        _ => return None,
    })
}

/// How glyph `code` is shown, where `drawable` says whether the interface fonts have a text's
/// characters: its sign; else a sign books use for it that they draw (`+=`, `=+`, `⊕`, `⇄`);
/// else `$code`.
pub(crate) fn face(code: u16, mut drawable: impl FnMut(&str) -> bool) -> String {
    let Some(n) = nag::by_code(code) else { return format!("${code}") };
    let others: &[&str] = match n.code {
        14 => &["+="],
        15 => &["=+"],
        132 => &["⇄", "↔"],
        138 => &["⊕"],
        _ => &[],
    };
    std::iter::once(n.glyph).chain(others.iter().copied()).find(|s| drawable(s)).map_or_else(|| format!("${}", n.code), str::to_owned)
}

/// Whether the interface's monospace fonts (the glyphs' face) draw every character of `text`.
pub(crate) fn drawable(ctx: &egui::Context, text: &str) -> bool {
    ctx.fonts_mut(|f| f.has_glyphs(&egui::FontId::monospace(13.0), text))
}

/// The code glyph `code` (White's) takes on the move shown: the one for the side that made the
/// move, or for zugzwang the side to move after it. `None` for a code not in the table.
fn code_here(board: &BoardState, code: u16) -> Option<u16> {
    code_at(board, board.game.current(), code)
}

/// [`code_here`] for move `node`, whatever move is shown.
fn code_at(board: &BoardState, node: NodeId, code: u16) -> Option<u16> {
    let n = nag::by_code(code)?;
    // An even absolute half-move after the move: White is to move.
    let white_to_move = board.game.abs_ply(node)? % 2 == 0;
    Some(if n.side_is_mover { n.code_for(!white_to_move) } else { n.code_for(white_to_move) })
}

/// Whether the move shown carries glyph `code` (White's code; the side's is looked for).
pub(crate) fn has_glyph(board: &BoardState, code: u16) -> bool {
    board.illegal.is_none() && code_here(board, code).is_some_and(|c| board.game.nags(board.game.current()).contains(&c))
}

/// Whether there is a move shown to annotate.
pub(crate) fn has_move(board: &BoardState) -> bool {
    board.illegal.is_none() && board.game.current() != 0
}

/// A rail glyph clicked: put glyph `code` on the move shown, or take it off when it is there.
pub(crate) fn toggle_glyph(board: &mut BoardState, code: u16) {
    glyph_edit(board, code, true);
}

/// A typed sign: put glyph `code` on the move shown ([`CLEAR`]: take its glyphs off).
pub(crate) fn typed_glyph(board: &mut BoardState, code: u16) {
    if code == CLEAR {
        clear_glyphs(board);
    } else {
        glyph_edit(board, code, false);
    }
}

/// The glyph brush (T15) on move `node`, clicked in the notation, or a glyph chosen in the move's
/// context menu (U1): glyph `code` (White's) goes on the move with its side's code, or comes off
/// when the move has it; one undo step. The move shown stays.
pub(crate) fn brush_glyph(board: &mut BoardState, node: NodeId, code: u16) {
    if board.illegal.is_some() || node == 0 {
        return;
    }
    glyph_edit_at(board, node, code, true);
}

fn glyph_edit(board: &mut BoardState, code: u16, toggle: bool) {
    if !has_move(board) {
        board.message = Some(tl!("Choose a move first: glyphs go on a move.").to_owned());
        return;
    }
    glyph_edit_at(board, board.game.current(), code, toggle);
}

fn glyph_edit_at(board: &mut BoardState, node: NodeId, code: u16, toggle: bool) {
    let Some(code) = code_at(board, node, code) else { return };
    board.selected = None;
    board.promotion = None;
    let mut full = false;
    board.edit(|s| {
        if s.game.nags(node).contains(&code) {
            if !toggle {
                return None;
            }
            s.game.toggle_nag(node, code);
        } else if !s.game.set_nag(node, code) {
            full = true;
            return None;
        }
        Some(ChessEdit::Glyph(crate::chess_board_ui::nag_text(code)))
    });
    board.message = full.then(|| tl!("Not added: this move has as many glyphs as it can take.").to_owned());
}

/// Take every glyph off the move shown (one undo step; none when it has none).
fn clear_glyphs(board: &mut BoardState) {
    if !has_move(board) {
        return;
    }
    clear_glyphs_at(board, board.game.current());
}

/// Take every glyph off move `node` (one undo step; none when it has none). The move shown stays.
pub(crate) fn clear_glyphs_at(board: &mut BoardState, node: NodeId) {
    if board.illegal.is_some() || node == 0 || board.game.nags(node).is_empty() {
        return;
    }
    board.selected = None;
    board.promotion = None;
    board.edit(|s| {
        for code in s.game.nags(node).to_vec() {
            s.game.toggle_nag(node, code);
        }
        Some(ChessEdit::ClearGlyphs)
    });
}

/// Whether `text` (one event's text) is made only of characters a typed sign uses.
pub(crate) fn is_glyph_text(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| "!?=+-/~0".contains(c))
}

/// Signs typed and not yet put on the move: see the module's notes.
#[derive(Debug, Default)]
pub(crate) struct Typed {
    /// The characters waiting: always the start of some sign.
    buffer: String,
    /// When the last of them came (egui's input time).
    at: f64,
    /// Characters taken from this frame's input, for the panel.
    pub(crate) incoming: Vec<char>,
}

/// The glyph of sign `s`, if it is one.
fn sign(s: &str) -> Option<u16> {
    TYPED.iter().find(|(t, _)| *t == s).map(|(_, c)| *c)
}

/// Whether a sign starts with `s` (or is `s`).
fn starts_sign(s: &str) -> bool {
    TYPED.iter().any(|(t, _)| t.starts_with(s))
}

/// Whether a sign longer than `s` starts with it.
fn longer_sign(s: &str) -> bool {
    TYPED.iter().any(|(t, _)| t.len() > s.len() && t.starts_with(s))
}

impl Typed {
    /// Keep `c`, typed at `now`, for the panel (at most [`MAX_TYPED`] a frame).
    pub(crate) fn take(&mut self, c: char) {
        if self.incoming.len() < MAX_TYPED {
            self.incoming.push(c);
        }
    }

    /// Character `c` typed at `now`: the glyphs (codes, or [`CLEAR`]) it completes, in order.
    pub(crate) fn push(&mut self, c: char, now: f64) -> Vec<u16> {
        let mut out = Vec::new();
        let mut candidate = self.buffer.clone();
        candidate.push(c);
        if !starts_sign(&candidate) {
            // It does not continue the sign waiting: that one goes on, and `c` starts afresh.
            out.extend(self.flush());
            candidate = c.to_string();
            if !starts_sign(&candidate) {
                return out;
            }
        }
        self.buffer = candidate;
        self.at = now;
        if !longer_sign(&self.buffer) {
            out.extend(self.flush());
        }
        out
    }

    /// The glyph of the sign waiting, once [`TYPED_WAIT`] passed since its last character.
    pub(crate) fn tick(&mut self, now: f64) -> Vec<u16> {
        if !self.buffer.is_empty() && (now - self.at >= TYPED_WAIT || !now.is_finite()) { self.flush() } else { Vec::new() }
    }

    /// Whether a sign waits for its next character.
    pub(crate) fn waiting(&self) -> bool {
        !self.buffer.is_empty()
    }

    /// The longest sign the buffer starts with (`+` alone is none), and empty it.
    fn flush(&mut self) -> Vec<u16> {
        let buffer = std::mem::take(&mut self.buffer);
        let mut end = buffer.len();
        while end > 0 {
            if let Some(code) = buffer.get(..end).and_then(sign) {
                return vec![code];
            }
            end = buffer.get(..end).and_then(|s| s.char_indices().last()).map_or(0, |(i, _)| i);
        }
        Vec::new()
    }
}

/// The text fields the glyph palette can type into.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Field {
    CommentAfter,
    CommentBefore,
    Fen,
    Pgn,
}

/// Where the glyph palette types: the text field with the keyboard when it was clicked, and what
/// is waiting to go into it. Kept in [`BoardState`].
#[derive(Debug, Default)]
pub(crate) struct Typing {
    /// The fields drawn in the last frames, by their egui id.
    fields: Vec<(Field, Id)>,
    /// The field that had the keyboard as the pointer last went down (or has it now).
    pub(crate) last: Option<Id>,
    /// Text to put into a field at its cursor when it is drawn next.
    pending: Option<(Id, String)>,
}

impl Typing {
    /// Remember field `field` is drawn as `id`.
    fn seen(&mut self, field: Field, id: Id) {
        if !self.fields.iter().any(|(_, i)| *i == id) {
            self.fields.retain(|(f, _)| *f != field);
            self.fields.push((field, id));
        }
    }

    /// Called once a frame before the fields are drawn: note the field that has the keyboard (as
    /// the last frame left it), and forget it once the pointer went elsewhere and was let go.
    pub(crate) fn track(&mut self, ui: &egui::Ui) {
        let focused = ui.memory(|m| m.focused());
        let (down, released) = ui.input(|i| (i.pointer.any_down() || i.pointer.any_pressed(), i.pointer.any_released()));
        match focused.filter(|id| self.fields.iter().any(|(_, i)| i == id)) {
            Some(id) => self.last = Some(id),
            // A click elsewhere ended: no field takes the palette's text any more.
            None if !down && !released => self.last = None,
            None => {}
        }
    }
}

/// Before drawing text field `id` (as `field`) holding `text`: put in what the palette left for
/// it, at its cursor (over its selection), move the cursor after it and give the field the
/// keyboard back. Returns whether `text` changed.
pub(crate) fn insert_pending(ui: &egui::Ui, typing: &mut Typing, field: Field, id: Id, text: &mut String) -> bool {
    typing.seen(field, id);
    let Some((_, insert)) = typing.pending.take_if(|(target, _)| *target == id) else { return false };
    insert_at_cursor(ui, id, text, &insert)
}

/// Put `insert` into text field `id` holding `text`, at its cursor (over its selection; at the
/// end when it has none), move the cursor after it and give the field the keyboard. Returns
/// whether `text` changed.
pub(crate) fn insert_at_cursor(ui: &egui::Ui, id: Id, text: &mut String, insert: &str) -> bool {
    let mut state = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
    let chars = text.chars().count();
    let (start, end) = state.cursor.char_range().map_or((chars, chars), |r| {
        let range = r.as_sorted_char_range();
        (range.start.0.min(chars), range.end.0.min(chars))
    });
    let byte = |ci: usize| text.char_indices().nth(ci).map_or(text.len(), |(b, _)| b);
    let (from, to) = (byte(start), byte(end));
    if from > to || !text.is_char_boundary(from) || !text.is_char_boundary(to) {
        return false;
    }
    text.replace_range(from..to, insert);
    let after = start.saturating_add(insert.chars().count());
    state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(after))));
    state.store(ui.ctx(), id);
    ui.memory_mut(|m| m.request_focus(id));
    true
}

/// A palette character clicked: `text` goes into the field that had the keyboard, next time it
/// is drawn, or else at the end of the comment after the move shown (one undo step).
pub(crate) fn palette_insert(board: &mut BoardState, text: &str) {
    if let Some(id) = board.typing.last {
        board.typing.pending = Some((id, text.to_owned()));
        return;
    }
    if board.illegal.is_some() {
        return;
    }
    let node = board.game.current();
    let joined = match board.game.comment(node) {
        "" => text.to_owned(),
        c => format!("{c} {text}"),
    };
    board.edit(|s| {
        s.game.set_comment(node, &joined);
        Some(ChessEdit::Comment)
    });
}

/// The basic palette's characters: the figurines (inserted as themselves, shown as `face` where
/// the fonts lack them), the glyphs, then `½ – × † ‡`.
const FIGURINES: [(&str, &str, Color, PieceKind); 12] = [
    ("♔", "K", Color::White, PieceKind::King),
    ("♕", "Q", Color::White, PieceKind::Queen),
    ("♖", "R", Color::White, PieceKind::Rook),
    ("♗", "B", Color::White, PieceKind::Bishop),
    ("♘", "N", Color::White, PieceKind::Knight),
    ("♙", "P", Color::White, PieceKind::Pawn),
    ("♚", "k", Color::Black, PieceKind::King),
    ("♛", "q", Color::Black, PieceKind::Queen),
    ("♜", "r", Color::Black, PieceKind::Rook),
    ("♝", "b", Color::Black, PieceKind::Bishop),
    ("♞", "n", Color::Black, PieceKind::Knight),
    ("♟", "p", Color::Black, PieceKind::Pawn),
];

/// `½ – × † ‡`, with their names for assistive technology.
fn marks() -> [(&'static str, &'static str); 5] {
    [("½", tl!("One half")), ("–", tl!("Dash")), ("×", tl!("Capture sign")), ("†", tl!("Check sign")), ("‡", tl!("Double check sign"))]
}

/// The glyph palette window's id: egui keeps where it was put under it, for the session.
const PALETTE_ID: &str = "chess-glyph-palette";

/// The palette window's content width, in points: the figurines and signs in one row.
const PALETTE_WIDTH: f32 = 340.0;

/// What a palette button puts in: `text`, and for a glyph its code (White's).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Pick {
    pub(crate) text: String,
    pub(crate) glyph: Option<u16>,
}

/// The glyph palette (U1), when the rail has it open: a floating window that can be dragged
/// about, kept within the window it is drawn in, at first at `anchor` (beside the rail's
/// button). A glyph clicked goes on the move shown or comes off it, any other character at the
/// end of the move's comment; while a text field had the keyboard, any character goes into it at
/// its cursor ([`palette_insert`]). A click in it puts the chess key scope on. Drawn at most once
/// a pass of each window, so two rails in one window draw one palette.
pub(crate) fn palette_window(ctx: &egui::Context, chess: &mut ChessState, anchor: Option<egui::Pos2>) {
    if !chess.tools.palette_open {
        return;
    }
    let pass = (ctx.viewport_id(), ctx.cumulative_pass_nr());
    if chess.tools.palette_drawn == Some(pass) {
        return;
    }
    chess.tools.palette_drawn = Some(pass);
    let t = crate::theme::Tokens::get(ctx);
    let board = chess.board();
    // The glyphs the move shown has, ticked in the palette.
    let on: Vec<u16> = MOVE_GLYPHS.iter().chain(&EVALUATION_GLYPHS).chain(&OTHER_GLYPHS).copied().filter(|&c| has_glyph(board, c)).collect();
    let mut window = egui::Window::new(tl!("Glyph palette"))
        .id(Id::new(PALETTE_ID))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        // Its own width from the first frame: egui places a window it hasn't measured yet as if it
        // were wide, and would push it away from the rail to keep it in the window.
        .default_width(PALETTE_WIDTH)
        .constrain(true)
        .order(egui::Order::Foreground);
    if let Some(at) = anchor.filter(|p| p.is_finite()) {
        window = window.default_pos(at);
    }
    let mut closed = false;
    let mut pick = None;
    let shown = window.show(ctx, |ui| {
        // Which field had the keyboard, whether or not the panel with the fields is drawn.
        chess.board_mut().typing.track(ui);
        ui.set_width(PALETTE_WIDTH);
        ui.horizontal(|ui| {
            ui.add(egui::Label::new(egui::RichText::new(tl!("Glyph palette")).strong()).selectable(false));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if crate::icons::button(ui, "x", 20.0, false, tl!("Close the glyph palette")).clicked() {
                    closed = true;
                }
            });
        });
        let hint = tl!("Glyphs go on the move shown and the other characters at the end of its comment, or all into the field you were typing in.");
        ui.add(egui::Label::new(egui::RichText::new(hint).small().color(t.text_muted)).selectable(false));
        pick = palette_buttons(ui, false, |c| on.contains(&c));
    });
    let pressed = shown.is_some_and(|w| w.response.contains_pointer()) && ctx.input(|i| i.pointer.any_pressed());
    if pressed || pick.is_some() {
        // A click in the palette is a click in the chess panels (D12): Undo and the keys are the game's.
        chess.enter_scope();
    }
    if closed {
        chess.tools.palette_open = false;
    }
    let Some(pick) = pick else { return };
    let board = chess.board_mut();
    match pick.glyph {
        // No field had the keyboard: a glyph is the move's.
        Some(code) if board.typing.last.is_none() => toggle_glyph(board, code),
        _ => palette_insert(board, &pick.text),
    }
}

/// The palette's buttons, one per character and each named for what it is: the figurines and
/// `½ – × † ‡`, then the glyphs in the rail's three groups. For the correction field of a
/// capture (T10, `correction`) also the ellipsis Black's moves are printed with, and no headings.
/// `on` says which glyphs show as ticked. Returns what the one clicked puts in.
pub(crate) fn palette_buttons(ui: &mut egui::Ui, correction: bool, on: impl Fn(u16) -> bool) -> Option<Pick> {
    let ctx = ui.ctx().clone();
    let muted = crate::theme::Tokens::get(&ctx).text_muted;
    let mut chosen = None;
    let mut button = |ui: &mut egui::Ui, shown: &str, pick: Pick, name: &str, selected: bool| {
        let r = ui.add(egui::Button::new(egui::RichText::new(shown).monospace()).min_size(egui::vec2(26.0, 24.0)).selected(selected));
        r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, name));
        if r.on_hover_text(name).clicked() {
            chosen = Some(pick);
        }
    };
    let heading = |ui: &mut egui::Ui, text: &str| {
        if !correction {
            ui.add(egui::Label::new(egui::RichText::new(text).small().color(muted)).selectable(false));
        }
    };
    let text = |s: &str| Pick { text: s.to_owned(), glyph: None };
    heading(ui, tl!("Figurines and signs"));
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(2.0, 2.0);
        let figurines = drawable(&ctx, "♔♕♖♗♘♙♚♛♜♝♞♟");
        for (figurine, letter, color, kind) in FIGURINES {
            let name = crate::chess_editor::piece_name(Piece { color, kind });
            button(ui, if figurines { figurine } else { letter }, text(figurine), name, false);
        }
        for (mark, name) in marks() {
            button(ui, mark, text(mark), name, false);
        }
        if correction {
            button(ui, "\u{2026}", text("\u{2026}"), tl!("Ellipsis"), false);
        }
    });
    let groups: [(&str, &[u16]); 3] =
        [(tl!("Move glyphs"), &MOVE_GLYPHS), (tl!("Evaluation glyphs"), &EVALUATION_GLYPHS), (tl!("Other glyphs"), &OTHER_GLYPHS)];
    for (title, codes) in groups {
        heading(ui, title);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(2.0, 2.0);
            for &code in codes {
                let Some(n) = nag::by_code(code) else { continue };
                let shown = face(code, |s| drawable(&ctx, s));
                button(ui, &shown, Pick { text: n.insert_text(), glyph: Some(code) }, nag_name(code).unwrap_or(n.name), on(code));
            }
        });
    }
    chosen
}

/// The glyphs in a move's context menu (U1), for move `node`: the six move-quality glyphs and
/// the evaluations, each ticked while the move has it, and "Remove all glyphs". Returns the one
/// chosen: a code (White's) to put on the move or take off ([`brush_glyph`]), or [`CLEAR`] to
/// take them all off ([`clear_glyphs_at`]).
pub(crate) fn glyph_menu(ui: &mut egui::Ui, board: &BoardState, node: NodeId) -> Option<u16> {
    if board.illegal.is_some() || node == 0 {
        return None;
    }
    let ctx = ui.ctx().clone();
    let muted = crate::theme::Tokens::get(&ctx).text_muted;
    let has = |code: u16| code_at(board, node, code).is_some_and(|c| board.game.nags(node).contains(&c));
    let mut chosen = None;
    // The move-quality glyphs are the group's first six (Only move and Novelty are left to the rail).
    let quality: &[u16] = MOVE_GLYPHS.get(..6).unwrap_or(&MOVE_GLYPHS);
    for (title, codes) in [(tl!("Move glyphs"), quality), (tl!("Evaluation glyphs"), &EVALUATION_GLYPHS[..])] {
        ui.add(egui::Label::new(egui::RichText::new(title).small().color(muted)).selectable(false));
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(2.0, 2.0);
            for &code in codes {
                let Some(name) = nag_name(code) else { continue };
                let selected = has(code);
                let shown = face(code, |s| drawable(&ctx, s));
                let r = ui.add(egui::Button::new(egui::RichText::new(shown).monospace()).min_size(egui::vec2(26.0, 22.0)).selected(selected));
                r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, name));
                if r.on_hover_text(name).clicked() {
                    chosen = Some(code);
                }
            }
        });
    }
    if ui.add_enabled(!board.game.nags(node).is_empty(), egui::Button::new(tl!("Remove all glyphs"))).clicked() {
        chosen = Some(CLEAR);
    }
    chosen
}

/// The comment fields of the move shown: the one before it while the comment editor is open
/// (with the warning PGN needs), then the one after it. `focus` gives the latter the keyboard.
pub(crate) fn comments(ui: &mut egui::Ui, t: &crate::theme::Tokens, board: &mut BoardState, editor: bool, focus: bool) {
    let node = board.game.current();
    if editor && node != 0 {
        let id = ui.make_persistent_id("chess-comment-before");
        let key = format!("before {node}");
        let (mut text, mut recorded) = board.drafted(&key, board.game.comment_before(node));
        let inserted = insert_pending(ui, &mut board.typing, Field::CommentBefore, id, &mut text);
        let hint = crate::i18n::fmt(tl!("Comment before {move}"), &[("move", &crate::chess_tools::move_name(&board.game, node))]);
        let r = ui.add(egui::TextEdit::multiline(&mut text).id(id).hint_text(hint.as_str()).desired_rows(1).desired_width(f32::INFINITY));
        labeled(&r, &hint);
        if r.changed() || inserted {
            recorded = board.type_in(recorded, ChessEdit::CommentBefore, |game| game.set_comment_before(node, &text));
        }
        if (!text.trim().is_empty() || r.has_focus())
            && let Some(why) = before_note(board, node)
        {
            ui.label(egui::RichText::new(format!("⚠ {why}")).small().color(t.text_muted));
        }
        board.keep(&key, &r, text, recorded);
    }
    let id = ui.make_persistent_id("chess-comment-after");
    let key = format!("comment {node}");
    let (mut text, mut recorded) = board.drafted(&key, board.game.comment(node));
    let inserted = insert_pending(ui, &mut board.typing, Field::CommentAfter, id, &mut text);
    let hint = tl!("Comment on this move");
    let r = ui.add(egui::TextEdit::multiline(&mut text).id(id).hint_text(hint).desired_rows(1).desired_width(f32::INFINITY));
    labeled(&r, hint);
    if focus {
        r.request_focus();
    }
    if r.changed() || inserted {
        recorded = board.type_in(recorded, ChessEdit::Comment, |game| game.set_comment(node, &text));
    }
    board.keep(&key, &r, text, recorded);
}

/// Name a comment field for assistive technology by what it holds (its hint); the field keeps
/// the role and value its widget gave it.
fn labeled(r: &egui::Response, name: &str) {
    r.ctx.accesskit_node_builder(r.id, |node| node.set_label(name));
}

/// What PGN does with a comment before move `node` that does not start a variation, or `None`
/// when it does (PGN has its place: `( {…} 12... Nf6`).
fn before_note(board: &BoardState, node: NodeId) -> Option<&'static str> {
    let path = board.game.path_of(node)?;
    let indices = path.indices();
    if indices.last().copied() != Some(0) {
        return None;
    }
    Some(if indices.len() == 1 {
        tl!("In PGN this comment joins the comment before the game's first move.")
    } else {
        tl!("In PGN this comment is written after the previous move.")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(keys: &[(char, f64)], end: f64) -> Vec<u16> {
        let mut t = Typed::default();
        let mut out = Vec::new();
        for (c, at) in keys {
            out.extend(t.tick(*at));
            out.extend(t.push(*c, *at));
        }
        out.extend(t.tick(end));
        out
    }

    #[test]
    fn typed_signs_join_within_the_wait_and_the_longest_wins() {
        assert_eq!(typed(&[('!', 0.0), ('?', 0.1)], 0.2), [5], "!? within the wait");
        assert_eq!(typed(&[('!', 0.0)], 0.5), [] as [u16; 0], "still waiting for a second sign");
        assert_eq!(typed(&[('!', 0.0)], 0.7), [1], "the wait is over");
        assert_eq!(typed(&[('!', 0.0), ('?', 0.8)], 1.6), [1, 2], "too late to join");
        assert_eq!(typed(&[('+', 0.0), ('/', 0.1), ('-', 0.2)], 0.3), [16]);
        assert_eq!(typed(&[('-', 0.0), ('+', 0.1)], 0.2), [19]);
        assert_eq!(typed(&[('=', 0.0), ('!', 0.1)], 1.0), [10, 1], "! does not continue =: = goes on");
        assert_eq!(typed(&[('+', 0.0)], 1.0), [] as [u16; 0], "+ alone is no sign");
        assert_eq!(typed(&[('+', 0.0), ('/', 0.1)], 1.0), [] as [u16; 0], "+/ is none either");
        assert_eq!(typed(&[('0', 0.0)], 0.0), [CLEAR], "0 at once");
        assert_eq!(typed(&[('~', 0.0), ('?', 0.1), ('?', 0.2)], 0.3), [13, 4]);
        assert!(is_glyph_text("+/-") && is_glyph_text("0") && !is_glyph_text("a!") && !is_glyph_text(""));
        assert_eq!(Typed::default().tick(f64::NAN), [] as [u16; 0]);
    }

    #[test]
    fn the_three_groups_hold_every_glyph_once_with_its_name() {
        let mut codes: Vec<u16> = MOVE_GLYPHS.iter().chain(&EVALUATION_GLYPHS).chain(&OTHER_GLYPHS).copied().collect();
        let all = codes.len();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), all, "no glyph twice");
        let mut table: Vec<u16> = nag::NAGS.iter().map(|n| n.code).collect();
        table.sort_unstable();
        assert_eq!(codes, table, "the 28 of NAGS");
        let previous = crate::i18n::current();
        crate::i18n::set_current(crate::i18n::Lang::EN);
        for n in nag::NAGS {
            assert_eq!(nag_name(n.code), Some(n.name), "{}", n.code);
            if let Some(black) = n.black_code {
                assert_eq!(nag_name(black), Some(n.name));
            }
        }
        crate::i18n::set_current(previous);
        assert_eq!(nag_name(999), None);
    }

    #[test]
    fn faces_fall_back_to_what_the_fonts_draw() {
        let all = |_: &str| true;
        let ascii = |s: &str| s.is_ascii();
        assert_eq!(face(14, all), "⩲");
        assert_eq!(face(14, ascii), "+=");
        assert_eq!(face(15, ascii), "=+");
        assert_eq!(face(37, all), "↑", "Black's code, the same sign");
        assert_eq!(face(142, ascii), "$142");
        assert_eq!(face(138, |s| s != "⨁"), "⊕");
        assert_eq!(face(133, |s| s == "↔"), "↔", "Black's counterplay, in the sign the fonts have");
        assert_eq!(face(999, all), "$999");
    }
}
