//! Chess ▸ the Board tab's Move field (plan A9-N7a, finding R5-03; WCAG 2.1.1): a move, or a
//! question about the board, typed instead of clicked. It stands in the navigation row after
//! « ‹ › » ⇅ (§2.8.0 left its place open; the navigation row, the coordinator's choice of
//! 2026-11-16), and M takes the keyboard there from the chess scope (a one-key shortcut, off with
//! the others: WCAG 2.1.4).
//!
//! What it takes, as lichess's blind mode does:
//! - a move in SAN, in the interface's letters or in English (`Cf3`, `Nf3`, `e4`, `O-O`), or in
//!   UCI (`g1f3`, `e7e8q`), played as a click plays it (one undo step);
//! - `p` the pieces, `u` the last move, `s` whose move it is, `f e` the pieces on a file, `r 4` on
//!   a rank;
//! - `arrow e2e4 red` and `circle d5 green` (in the interface's words too; green when no colour is
//!   given) draw on the position, or take the drawing off when it is there.
//!
//! The answer is said to screen readers ([`crate::notice::announce`]): a move played by the move
//! shown's own announcement (plan A9-I17), the rest at once; what can't be done is an alert, with
//! why. Nothing typed reaches the engine: only a move the board found legal goes into the game.

use egui::Key;
use pdfcraft_engine::chess::pdfcraft_chess::describe::{self, Naming};
use pdfcraft_engine::chess::pdfcraft_chess::game::{Game, GameError};
use pdfcraft_engine::chess::pdfcraft_chess::placement::{Piece, PieceKind, Placement, Square};
use pdfcraft_engine::chess::pdfcraft_chess::san::{Lang, San, SanBody, Style};

use crate::chess_board_ui::{BoardState, Mark, MarkColor};
use crate::notice::Level;

/// Most characters the field keeps.
pub(crate) const MAX_TYPED: usize = 64;

/// The field's narrowest width: it shrinks to it before the row's end loses anything.
pub(crate) const MIN_WIDTH: f32 = 72.0;

/// Its widest.
pub(crate) const MAX_WIDTH: f32 = 200.0;

/// The space on each side of the field in its row.
pub(crate) const GAP: f32 = 4.0;

/// The field's width in a row with `room` points left after the arrows, which keeps `end` points at
/// its end for the engine's state or the material: the end gives up its room first, then the field
/// shrinks to [`MIN_WIDTH`]; `None` when even that does not fit (the arrows never give up theirs).
pub(crate) fn width_in(room: f32, end: f32) -> Option<f32> {
    if !room.is_finite() || room < MIN_WIDTH + GAP {
        return None;
    }
    Some((room - end - 2.0 * GAP).clamp(MIN_WIDTH, MAX_WIDTH).min(room - GAP))
}

/// The field: what is typed, and whether it takes the keyboard when it is drawn next (M).
#[derive(Debug, Default)]
pub struct MoveField {
    pub text: String,
    /// M asked for the keyboard here: taken when the field is drawn, dropped when the panel was
    /// drawn without it (plan A9-N7a2, `chess_tools::with_rail`).
    pub(crate) focus: bool,
    /// Why the last line could not be done, until the line is changed.
    pub(crate) error: Option<String>,
}

/// The field's id: M gives it the keyboard.
pub(crate) fn field_id() -> egui::Id {
    egui::Id::new("pdfcraft-chess-move-field")
}

/// What a line typed in the field asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Typed {
    Pieces,
    LastMove,
    SideToMove,
    /// The pieces on a file (0 = a).
    File(u8),
    /// The pieces on a rank (0 = the first).
    Rank(u8),
    /// An arrow, or a square circled (`from == to`).
    Draw(Mark),
    /// A move, as typed.
    Move(String),
    /// A command whose squares or colour can't be read.
    Unclear,
}

/// Whether `word` is `english` or the interface's word for it (any case).
fn is_word(word: &str, english: &str, translated: &str) -> bool {
    let word = word.to_lowercase();
    word == english || word == translated.to_lowercase()
}

/// A colour named in English or in the interface's words.
fn color_word(word: &str) -> Option<MarkColor> {
    [
        ("green", tl!("Green"), MarkColor::Green),
        ("red", tl!("Red"), MarkColor::Red),
        ("blue", tl!("Blue"), MarkColor::Blue),
        ("yellow", tl!("Yellow"), MarkColor::Yellow),
    ]
    .into_iter()
    .find(|(english, translated, _)| is_word(word, english, translated))
    .map(|(_, _, color)| color)
}

/// A colour's name in the interface's words, in lower case (it ends a sentence).
fn color_name(color: MarkColor) -> String {
    match color {
        MarkColor::Red => tl!("Red"),
        MarkColor::Blue => tl!("Blue"),
        MarkColor::Yellow => tl!("Yellow"),
        MarkColor::Green | MarkColor::Engine => tl!("Green"),
    }
    .to_lowercase()
}

/// What `line` asks for; `None` when nothing is typed.
pub(crate) fn parse(line: &str) -> Option<Typed> {
    let words: Vec<&str> = line.split_whitespace().collect();
    let one = |w: &str, c: &str| w.eq_ignore_ascii_case(c);
    let draw = |from: Square, to: Square, color: Option<&&str>| match color.map(|c| color_word(c)) {
        None => Typed::Draw(Mark { from, to, color: MarkColor::Green }),
        Some(Some(color)) => Typed::Draw(Mark { from, to, color }),
        Some(None) => Typed::Unclear,
    };
    Some(match words.as_slice() {
        [] => return None,
        [w] if one(w, "p") => Typed::Pieces,
        [w] if one(w, "u") => Typed::LastMove,
        [w] if one(w, "s") => Typed::SideToMove,
        [w, file] if one(w, "f") => match file.as_bytes() {
            [c @ b'a'..=b'h'] | [c @ b'A'..=b'H'] => Typed::File(c.to_ascii_lowercase() - b'a'),
            _ => Typed::Unclear,
        },
        [w, rank] if one(w, "r") => match rank.as_bytes() {
            [c @ b'1'..=b'8'] => Typed::Rank(c - b'1'),
            _ => Typed::Unclear,
        },
        [w, squares, rest @ ..] if is_word(w, "arrow", tl!("Arrow")) && rest.len() <= 1 => {
            match (squares.get(0..2).and_then(Square::parse), squares.get(2..).and_then(Square::parse)) {
                (Some(from), Some(to)) if from != to => draw(from, to, rest.first()),
                _ => Typed::Unclear,
            }
        }
        [w, square, rest @ ..] if is_word(w, "circle", tl!("Circle")) && rest.len() <= 1 => match Square::parse(square) {
            Some(on) => draw(on, on, rest.first()),
            None => Typed::Unclear,
        },
        [w, ..] if is_word(w, "arrow", tl!("Arrow")) || is_word(w, "circle", tl!("Circle")) => Typed::Unclear,
        _ => Typed::Move(line.trim().to_owned()),
    })
}

/// Whether `text` is a move in UCI (`e2e4`, `e7e8q`).
fn is_uci(text: &str) -> bool {
    let b = text.as_bytes();
    matches!(b, [f1, r1, f2, r2, rest @ ..]
        if (b'a'..=b'h').contains(f1) && (b'1'..=b'8').contains(r1) && (b'a'..=b'h').contains(f2) && (b'1'..=b'8').contains(r2)
            && matches!(rest, [] | [b'q' | b'r' | b'b' | b'n' | b'Q' | b'R' | b'B' | b'N']))
}

/// The pieces on the squares `on` keeps, in words, or `None` when there are none.
fn pieces_on(placement: &Placement, on: impl Fn(Square) -> bool) -> Option<String> {
    let mut some = Placement::empty();
    for (sq, piece) in placement.pieces().filter(|(sq, _)| on(*sq)) {
        some.set(sq, Some(piece));
    }
    (!some.is_empty()).then(|| describe::pieces_named(&some, crate::chess_figurines::interface_lang(), Naming::Words))
}

/// The promotion a SAN in English letters asks for (`e8=Q`).
fn promotion_of(english: &str) -> Option<PieceKind> {
    let (_, after) = english.split_once('=')?;
    match after.chars().next()? {
        'Q' => Some(PieceKind::Queen),
        'R' => Some(PieceKind::Rook),
        'B' => Some(PieceKind::Bishop),
        'N' => Some(PieceKind::Knight),
        _ => None,
    }
}

/// Do what `typed` asks on `state`'s board: what to say (empty when the move shown's own
/// announcement says it), or why it can't be done.
pub(crate) fn run(state: &mut BoardState, typed: Typed) -> Result<String, String> {
    let not_legal = || crate::chess_board_ui::illegal_reason().to_owned();
    let lang = crate::chess_figurines::interface_lang();
    match typed {
        Typed::Pieces => Ok(pieces_on(&state.placement(), |_| true).unwrap_or_else(|| tl!("The board is empty.").to_owned())),
        Typed::File(file) => {
            let name = char::from(b'a'.saturating_add(file)).to_string();
            Ok(match pieces_on(&state.placement(), |sq| sq.file() == file) {
                Some(pieces) => crate::i18n::fmt(tl!("{file} file: {pieces}"), &[("file", &name), ("pieces", &pieces)]),
                None => crate::i18n::fmt(tl!("{file} file: empty"), &[("file", &name)]),
            })
        }
        Typed::Rank(rank) => {
            let name = (u32::from(rank) + 1).to_string();
            Ok(match pieces_on(&state.placement(), |sq| sq.rank() == rank) {
                Some(pieces) => crate::i18n::fmt(tl!("Rank {rank}: {pieces}"), &[("rank", &name), ("pieces", &pieces)]),
                None => crate::i18n::fmt(tl!("Rank {rank}: empty"), &[("rank", &name)]),
            })
        }
        Typed::Draw(mark) => Ok(draw(state, mark)),
        _ if state.is_illegal() => Err(not_legal()),
        Typed::SideToMove => {
            let side = describe::to_move(state.game().side_to_move(), lang);
            let mut chars = side.chars();
            Ok(chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default())
        }
        Typed::LastMove => {
            let node = state.game().current();
            Ok(if state.game().ply() == 0 {
                tl!("No move yet: the start of the game.").to_owned()
            } else {
                crate::chess_tools::move_spoken(state.game(), node)
            })
        }
        Typed::Unclear => Err(tl!("Not understood. Type a move (Nf3, e2e4), or p, u, s, f e, r 4, arrow e2e4 red, circle d5.").to_owned()),
        Typed::Move(text) => play(state, &text, lang),
    }
}

/// Draw `mark` on the position shown, or take it off when it is there in that colour: what to say.
pub(crate) fn draw(state: &mut BoardState, mark: Mark) -> String {
    let there = state.marks().iter().any(|m| m.from == mark.from && m.to == mark.to && m.color == mark.color);
    state.toggle_mark(mark);
    let (from, to, color) = (mark.from.to_string(), mark.to.to_string(), color_name(mark.color));
    let args = [("from", from.as_str()), ("to", to.as_str()), ("color", color.as_str())];
    match (mark.from == mark.to, there) {
        (false, false) => crate::i18n::fmt(tl!("Arrow {from} to {to}, {color}"), &args),
        (false, true) => crate::i18n::fmt(tl!("Arrow {from} to {to} taken off"), &args),
        (true, false) => crate::i18n::fmt(tl!("Circle on {from}, {color}"), &args),
        (true, true) => crate::i18n::fmt(tl!("Circle on {from} taken off"), &args),
    }
}

/// Play the move `text` (SAN in `lang` or English, or UCI) on the move shown, as a click plays it.
/// A move refused says why: an ambiguous one names the moves it could be, and a pawn reaching the
/// last rank with no piece asks for one (plan A9-N7a2).
fn play(state: &mut BoardState, text: &str, lang: Lang) -> Result<String, String> {
    let illegal = || crate::i18n::fmt(tl!("Illegal move: {move} can't be played here."), &[("move", text)]);
    let needs_piece = |example: &str| {
        crate::i18n::fmt(tl!("Illegal move: {move} needs the piece the pawn becomes, as in {example}."), &[("move", text), ("example", example)])
    };
    let mut probe = state.game().clone();
    if is_uci(text) {
        match probe.play_uci(text) {
            Ok(()) => {}
            Err(GameError::NeedsPromotion) => return Err(needs_piece(&format!("{}q", text.trim()))),
            Err(_) => return Err(illegal()),
        }
    } else {
        let (san, letters) = match San::read(text, lang) {
            Ok(san) => (san, lang),
            Err(_) => (San::read(text, Lang::En).map_err(|_| crate::i18n::fmt(tl!("{move} is not a move."), &[("move", text)]))?, Lang::En),
        };
        if probe.play_san(&san.write(Lang::En, Style::Letters)).is_err() {
            return Err(match refused(state.game(), &san, letters) {
                Some(Refused::Ambiguous(moves)) => {
                    crate::i18n::fmt(tl!("Illegal move: {move} is ambiguous, it could be {moves}."), &[("move", text), ("moves", &moves.join(", "))])
                }
                Some(Refused::NeedsPiece(example)) => needs_piece(&example),
                None => illegal(),
            });
        }
    }
    let at = probe.current();
    let Some((from, to)) = probe.move_at(at) else { return Err(illegal()) };
    let promotion = promotion_of(probe.san(at));
    state.play_typed(from, to, promotion);
    Ok(String::new())
}

/// Why a move typed in SAN can't be played, when it is more than "not legal here".
#[derive(Debug, PartialEq, Eq)]
enum Refused {
    /// More than one piece can make it: the moves it could be, in `letters`.
    Ambiguous(Vec<String>),
    /// A pawn reaching the last rank with no piece named: the move with a queen, in `letters`.
    NeedsPiece(String),
}

/// Why `san` can't be played on `game`'s move shown, written in `letters`: the pieces of the side
/// to move that could make it, as typed (its file or rank when given), are more than one, or one
/// pawn promoting with no piece named. `None` for any other reason.
fn refused(game: &Game, san: &San, letters: Lang) -> Option<Refused> {
    let SanBody::Move { piece, from_file, from_rank, capture, to, promotion } = san.body else { return None };
    let want = Piece::new(game.side_to_move(), piece.unwrap_or(PieceKind::Pawn));
    let from: Vec<Square> = game
        .placement()
        .pieces()
        .filter(|(sq, p)| *p == want && from_file.is_none_or(|f| f == sq.file()) && from_rank.is_none_or(|r| r == sq.rank()))
        .map(|(sq, _)| sq)
        .filter(|sq| game.targets(*sq).contains(&to))
        .collect();
    let written = |english: &str| San::read(english, Lang::En).map(|s| s.write(letters, Style::Letters)).unwrap_or_else(|_| english.to_owned());
    match from.as_slice() {
        [one] if piece.is_none() && promotion.is_none() && game.is_promotion(*one, to) => {
            let queen =
                San { body: SanBody::Move { piece, from_file, from_rank, capture, to, promotion: Some(PieceKind::Queen) }, suffix: String::new() };
            Some(Refused::NeedsPiece(queen.write(letters, Style::Letters)))
        }
        [_, _, ..] => {
            let moves = from
                .iter()
                .filter_map(|&sq| {
                    let mut each = game.clone();
                    each.play(sq, to, promotion).ok()?;
                    Some(written(each.san(each.current())))
                })
                .collect();
            Some(Refused::Ambiguous(moves))
        }
        _ => None,
    }
}

/// The button the field waits behind while the engine's state has the navigation row's end: it
/// opens the field, as M does.
pub(crate) fn open_button(ui: &mut egui::Ui) -> egui::Response {
    crate::icons::button(ui, "text-cursor-input", 28.0, false, tl!("Type a move"))
}

/// The field, `width` points wide, on `state`'s board: Enter does what the line says, and the
/// keyboard stays in the field for the next one. What failed is said as an alert and shown as the
/// field's tooltip, with a danger edge, until the line is changed.
pub(crate) fn field(ui: &mut egui::Ui, state: &mut BoardState, field: &mut MoveField, width: f32) {
    let t = crate::theme::Tokens::get(ui.ctx());
    let edit = egui::TextEdit::singleline(&mut field.text).id(field_id()).hint_text(tl!("Move")).desired_width(width).char_limit(MAX_TYPED);
    let response = ui.add(edit);
    // Named, as every focusable control is (plan A9-I11a): the hint is no name.
    ui.ctx().accesskit_node_builder(response.id, |node| node.set_label(tl!("Move")));
    if response.changed() {
        field.error = None;
    }
    if let Some(why) = &field.error {
        ui.painter().rect_stroke(response.rect, t.radius, egui::Stroke::new(1.5, t.danger), egui::StrokeKind::Outside);
        response.clone().on_hover_text(why.as_str());
    } else {
        response.clone().on_hover_text(tl!(
            "Type a move (Nf3, e4, e2e4) and Enter, or: p the pieces, u the last move, s whose move it is, f e a file, r 4 a rank, arrow e2e4 red, circle d5. M brings you here."
        ));
    }
    if std::mem::take(&mut field.focus) {
        response.request_focus();
    }
    if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
        if let Some(typed) = parse(&field.text) {
            match run(state, typed) {
                Ok(said) => {
                    field.text.clear();
                    field.error = None;
                    if !said.is_empty() {
                        crate::notice::announce(ui.ctx(), Level::Info, said);
                    }
                }
                Err(why) => {
                    crate::notice::announce(ui.ctx(), Level::Error, why.clone());
                    field.error = Some(why);
                }
            }
        }
        response.request_focus();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a line asks for: the lichess letters, the drawings in English and in the interface's
    /// words, and everything else a move.
    #[test]
    fn lines_are_read_as_commands_or_moves() {
        let sq = |n: &str| Square::parse(n).unwrap();
        assert_eq!(parse("  "), None);
        assert_eq!(parse("p"), Some(Typed::Pieces));
        assert_eq!(parse("U"), Some(Typed::LastMove));
        assert_eq!(parse("s"), Some(Typed::SideToMove));
        assert_eq!(parse("f e"), Some(Typed::File(4)));
        assert_eq!(parse("r 8"), Some(Typed::Rank(7)));
        assert_eq!(parse("f k"), Some(Typed::Unclear));
        assert_eq!(parse("r 9"), Some(Typed::Unclear));
        assert_eq!(parse("arrow e2e4 red"), Some(Typed::Draw(Mark { from: sq("e2"), to: sq("e4"), color: MarkColor::Red })));
        assert_eq!(parse("Arrow g1f3"), Some(Typed::Draw(Mark { from: sq("g1"), to: sq("f3"), color: MarkColor::Green })));
        assert_eq!(parse("circle d5 blue"), Some(Typed::Draw(Mark { from: sq("d5"), to: sq("d5"), color: MarkColor::Blue })));
        assert_eq!(parse("arrow e2e2"), Some(Typed::Unclear), "an arrow goes somewhere");
        assert_eq!(parse("arrow e2e4 pink"), Some(Typed::Unclear));
        assert_eq!(parse("circle"), Some(Typed::Unclear));
        assert_eq!(parse(" Nf3 "), Some(Typed::Move("Nf3".into())));
        assert!(is_uci("e7e8q") && is_uci("g1f3") && !is_uci("e7e8k") && !is_uci("Nf3") && !is_uci("e2e9"));
        assert_eq!(promotion_of("exd8=N+"), Some(PieceKind::Knight));
        assert_eq!(promotion_of("Nf3"), None);
    }

    /// `line` typed on a board at `fen`: what is said (or why not), and the move shown after.
    fn typed(fen: &str, line: &str) -> (Result<String, String>, String) {
        let mut state = BoardState::default();
        state.load_fen(fen, false, None);
        let said = parse(line).map_or_else(|| Err("nothing typed".to_owned()), |t| run(&mut state, t));
        (said, state.game().san(state.game().current()).to_owned())
    }

    const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

    /// Plan A9-N7a2 (review of #405): a promotion typed with "=", without it and in UCI, and castling
    /// in letters, in zeros and as the king's two steps, are played.
    #[test]
    fn promotions_and_castling_typed_are_played() {
        let promote = "k7/4P3/8/8/8/8/8/4K3 w - - 0 1";
        for line in ["e8=Q", "e8Q", "e7e8q", "e7e8Q"] {
            assert_eq!(typed(promote, line), (Ok(String::new()), "e8=Q+".into()), "{line}");
        }
        assert_eq!(typed(promote, "e8=N"), (Ok(String::new()), "e8=N".into()));
        let castle = "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1";
        for line in ["O-O", "0-0", "e1g1", "o-o"] {
            assert_eq!(typed(castle, line), (Ok(String::new()), "O-O".into()), "{line}");
        }
        assert_eq!(typed(castle, "O-O-O"), (Ok(String::new()), "O-O-O".into()));
        assert_eq!(typed(castle, "e1c1"), (Ok(String::new()), "O-O-O".into()));
    }

    /// Plan A9-N7a2 (review of #405; the N7a card: "lance ilegal" with the reason): an ambiguous move
    /// names the moves it could be, and a pawn reaching the last rank with no piece asks for one;
    /// with the piece's file the move is played. Before, both were "can't be played here".
    #[test]
    fn an_ambiguous_move_or_a_promotion_without_a_piece_says_why() {
        let knights = "4k3/8/8/8/8/5N2/8/1N2K3 w - - 0 1";
        assert_eq!(typed(knights, "Nd2"), (Err("Illegal move: Nd2 is ambiguous, it could be Nbd2, Nfd2.".into()), String::new()));
        assert_eq!(typed(knights, "Nbd2"), (Ok(String::new()), "Nbd2".into()));
        assert_eq!(typed(knights, "Nfd2"), (Ok(String::new()), "Nfd2".into()));
        // Two knights on one file: the file is not enough, the rank is.
        let file = "4k3/8/8/8/8/1N6/8/1N2K3 w - - 0 1";
        assert_eq!(typed(file, "Nbd2"), (Err("Illegal move: Nbd2 is ambiguous, it could be N1d2, N3d2.".into()), String::new()));
        assert_eq!(typed(file, "N3d2"), (Ok(String::new()), "N3d2".into()));
        let promote = "k2r4/4P3/8/8/8/8/8/4K3 w - - 0 1";
        assert_eq!(typed(promote, "e8"), (Err("Illegal move: e8 needs the piece the pawn becomes, as in e8=Q.".into()), String::new()));
        assert_eq!(typed(promote, "exd8"), (Err("Illegal move: exd8 needs the piece the pawn becomes, as in exd8=Q.".into()), String::new()));
        assert_eq!(typed(promote, "e7e8"), (Err("Illegal move: e7e8 needs the piece the pawn becomes, as in e7e8q.".into()), String::new()));
        // Any other refusal is the plain one.
        assert_eq!(typed(START, "Nf4"), (Err("Illegal move: Nf4 can't be played here.".into()), String::new()));
        assert_eq!(typed(START, "e2e5"), (Err("Illegal move: e2e5 can't be played here.".into()), String::new()));
    }

    /// Plan A9-N7a2 (review of #405): what is not ASCII is refused, never a panic: a Cyrillic letter
    /// that looks like a piece's, an emoji, wide digits, and characters cut by the commands' squares.
    /// A figurine is a piece in every language ([`San::read`]), so `♘f3` is a move and `♘f4` is not.
    #[test]
    fn what_is_not_ascii_is_refused_without_a_panic() {
        assert_eq!(typed(START, "Сf3"), (Err("Сf3 is not a move.".into()), String::new()));
        assert_eq!(typed(START, "🐴f3"), (Err("🐴f3 is not a move.".into()), String::new()));
        assert_eq!(typed(START, "♘f3"), (Ok(String::new()), "Nf3".into()));
        assert_eq!(typed(START, "♘f4"), (Err("Illegal move: ♘f4 can't be played here.".into()), String::new()));
        for line in ["😀", "e2é4", "ｅ２ｅ４", "f ｅ", "r ９", "arrow 😀😀", "arrow é2e4", "circle 😀", "e7e8😀"] {
            let (said, shown) = typed(START, line);
            assert!(said.is_err() && shown.is_empty(), "{line}: {said:?} {shown}");
        }
    }

    /// The field's width in the navigation row (plan A9-N7a): never under its narrowest nor over
    /// its widest, never past the row; the row's end gives up its room first. From the 360-pt Chess
    /// column (about 188 pt after the arrows) to the chess window at its 320-pt least.
    #[test]
    fn the_field_takes_the_room_after_the_arrows() {
        for room in (0..=600).map(|r| r as f32) {
            match width_in(room, 80.0) {
                Some(w) => assert!((MIN_WIDTH..=MAX_WIDTH).contains(&w) && w + GAP <= room, "{room}: {w}"),
                None => assert!(room < MIN_WIDTH + GAP, "{room}"),
            }
        }
        assert_eq!(width_in(188.0, 80.0), Some(100.0), "the end keeps its 80 pt");
        assert_eq!(width_in(120.0, 80.0), Some(MIN_WIDTH), "the end gives up its room first");
        assert_eq!(width_in(400.0, 80.0), Some(MAX_WIDTH));
        assert_eq!(width_in(f32::NAN, 80.0), None);
    }
}
