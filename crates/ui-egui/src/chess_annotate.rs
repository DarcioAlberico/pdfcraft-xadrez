//! Chess ▸ PGN toolbar (T6): the annotation glyphs on the rail, glyphs typed on the keyboard, the
//! comment before and after a move, and the basic glyph palette.
//!
//! **Glyphs** (spec §4.4) are the entries of `pdfcraft_chess::nag::NAGS`: every code of the PGN
//! standard and of the ChessPad extension. The rail has the 28 common ones in three groups (the
//! move's quality, the evaluation, and the others); the palette has them all. A glyph goes on the
//! move shown with the code
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
//! its close button, the rail's button and G close it. It holds the figurines, `½ – × † ‡`, the
//! rail's three groups of glyphs (the move row with ChessPad's `==` too), a row of the positional
//! signs ([`POSITIONAL_GLYPHS`]), and, folded under "More assessments", the assessments that have
//! no printed sign ("slight space advantage"), by name, in sections by subject or narrowed by a
//! filter field ([`more_assessments`]), scrolled in the room left above the bottom of the window
//! the palette floats in: the palette never runs past it, and moves up when the list opens too
//! low to have room. A glyph clicked goes on the move shown, or comes off it,
//! as the rail's do (one undo step); the other characters go at the end of the move's comment (one
//! undo step). While a text field had the keyboard when it was clicked (the comment fields, FEN,
//! PGN, a capture's correction field), any character, a glyph's sign too, goes into that field at
//! its cursor instead; an assessment without a sign always goes on the move (as text it would only
//! be `$n`). Figurines
//! are inserted as the Unicode characters; their buttons show the piece letters where the
//! interface fonts lack the figurines. A click in the palette puts the chess key scope on, as a
//! click in the panel does. The correction field of a capture's preview (T10) has the same
//! buttons inline, with the ellipsis, and every one of them types there ([`palette_buttons`]).
//!
//! **A move's context menu** offers the six move-quality glyphs, the evaluations and "Remove all
//! glyphs" ([`glyph_menu`]): the same changes as the rail's glyphs, on the move clicked, without
//! going to it (as with the glyph brush). Only these common ones: the rest would make the menu a
//! wall, and the palette has them.
//!
//! **Signs no font draws.** The interface's fonts (Inter, JetBrains Mono and egui's own) lack a
//! few chess signs (measured with egui's own font lookup: `every_glyph_is_drawn_by_a_bundled_font_or_painted`):
//! `⌓`, ChessPad's diagonal `⇗`, pawn signs `⯺ ⯻ ⯼ ⯽`, with `∟` and without `⯾`; and the three bishop signs
//! have no Unicode character at all (their glyphs are stand-ins, `⧉ □■ ■■`). Those are painted ([`GlyphIcon`]) in the palette, on the rail
//! and in the notation, so none shows as `$n`; PGN and copied text keep the character or `$n`. An
//! assessment without a sign shows in the notation as its name, small and muted after the move,
//! with its `$n` as the tooltip: a reader learns nothing from `$24`, and these are rare enough in a
//! game for the words not to crowd it.

use egui::Id;
use pdfcraft_engine::chess::pdfcraft_chess::game::NodeId;
use pdfcraft_engine::chess::pdfcraft_chess::nag::{self, NagGroup};
use pdfcraft_engine::chess::pdfcraft_chess::placement::{Color, Piece, PieceKind};

use crate::chess_board_ui::BoardState;
use crate::chess_history::ChessEdit;
use crate::chess_ui::ChessState;

/// The rail's glyph groups (spec §4.2), by PGN code (White's, for the side-dependent ones), as the
/// palette shows them; the palette's move row has ChessPad's `==` (`$144`) after the rail's eight.
/// With [`POSITIONAL_GLYPHS`] and the assessments without a sign they hold `NAGS` once each.
pub(crate) const MOVE_GLYPHS: [u16; 9] = [1, 2, 3, 4, 5, 6, 7, 146, 144];
pub(crate) const EVALUATION_GLYPHS: [u16; 9] = [10, 13, 14, 15, 16, 17, 18, 19, 44];
pub(crate) const OTHER_GLYPHS: [u16; 11] = [32, 36, 40, 132, 140, 141, 22, 138, 142, 143, 145];

/// The palette's positional signs (not on the rail): the standard's ring for space, ChessPad's
/// signs for space, where the play is, a weak point and the ending, its pawn and bishop signs, and
/// "with" and "without".
pub(crate) const POSITIONAL_GLYPHS: [u16; 18] = [26, 238, 239, 240, 241, 242, 243, 244, 245, 249, 250, 251, 252, 246, 247, 248, 254, 255];

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
/// in the table. One arm a name, so the catalogues' checks see every one.
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
        144 => tl!("Equivalent move"),
        8 => tl!("Singular move"),
        9 => tl!("Worst move"),
        10 => tl!("Equal position"),
        13 => tl!("Unclear position"),
        14 => tl!("White is slightly better"),
        15 => tl!("Black is slightly better"),
        16 => tl!("White is clearly better"),
        17 => tl!("Black is clearly better"),
        18 => tl!("White is winning"),
        19 => tl!("Black is winning"),
        44 => tl!("Compensation for the material"),
        11 => tl!("Equal chances, quiet position"),
        12 => tl!("Equal chances, active position"),
        20 => tl!("White has a crushing advantage"),
        21 => tl!("Black has a crushing advantage"),
        42 => tl!("Insufficient compensation for the material"),
        46 => tl!("More than enough compensation for the material"),
        22 => tl!("Zugzwang"),
        32 => tl!("Development advantage"),
        36 => tl!("With the initiative"),
        40 => tl!("With an attack"),
        132 => tl!("With counterplay"),
        138 => tl!("Time trouble"),
        30 => tl!("Slight development advantage"),
        34 => tl!("Decisive development advantage"),
        38 => tl!("Lasting initiative"),
        130 => tl!("Slight counterplay"),
        134 => tl!("Decisive counterplay"),
        136 => tl!("Moderate time pressure"),
        140 => tl!("With the idea"),
        141 => tl!("Aimed against"),
        142 => tl!("Better is"),
        143 => tl!("Worse is"),
        145 => tl!("Editorial comment"),
        254 => tl!("With"),
        255 => tl!("Without"),
        26 => tl!("Moderate space advantage"),
        238 => tl!("Space advantage"),
        239 => tl!("Play along the file"),
        240 => tl!("Play along the diagonal"),
        241 => tl!("Play in the center"),
        242 => tl!("Play on the kingside"),
        243 => tl!("Play on the queenside"),
        244 => tl!("Weak point"),
        245 => tl!("Endgame"),
        24 => tl!("Slight space advantage"),
        28 => tl!("Decisive space advantage"),
        48 => tl!("Slight control of the center"),
        50 => tl!("Moderate control of the center"),
        52 => tl!("Decisive control of the center"),
        54 => tl!("Slight control of the kingside"),
        56 => tl!("Moderate control of the kingside"),
        58 => tl!("Decisive control of the kingside"),
        60 => tl!("Slight control of the queenside"),
        62 => tl!("Moderate control of the queenside"),
        64 => tl!("Decisive control of the queenside"),
        66 => tl!("Vulnerable first rank"),
        68 => tl!("Well protected first rank"),
        70 => tl!("Poorly protected king"),
        72 => tl!("Well protected king"),
        74 => tl!("Poorly placed king"),
        76 => tl!("Well placed king"),
        249 => tl!("Connected pawns"),
        250 => tl!("Isolated pawns"),
        251 => tl!("Doubled pawns"),
        252 => tl!("Passed pawn"),
        253 => tl!("Pawn majority"),
        78 => tl!("Very weak pawn structure"),
        80 => tl!("Moderately weak pawn structure"),
        82 => tl!("Moderately strong pawn structure"),
        84 => tl!("Very strong pawn structure"),
        246 => tl!("Pair of bishops"),
        247 => tl!("Bishops of opposite colors"),
        248 => tl!("Bishops of the same color"),
        86 => tl!("Poor knight placement"),
        88 => tl!("Good knight placement"),
        90 => tl!("Poor bishop placement"),
        92 => tl!("Good bishop placement"),
        94 => tl!("Poor rook placement"),
        96 => tl!("Good rook placement"),
        98 => tl!("Poor queen placement"),
        100 => tl!("Good queen placement"),
        102 => tl!("Poor piece coordination"),
        104 => tl!("Good piece coordination"),
        106 => tl!("Very poorly played opening"),
        108 => tl!("Poorly played opening"),
        110 => tl!("Well played opening"),
        112 => tl!("Very well played opening"),
        114 => tl!("Very poorly played middlegame"),
        116 => tl!("Poorly played middlegame"),
        118 => tl!("Well played middlegame"),
        120 => tl!("Very well played middlegame"),
        122 => tl!("Very poorly played endgame"),
        124 => tl!("Poorly played endgame"),
        126 => tl!("Well played endgame"),
        128 => tl!("Very well played endgame"),
        _ => return None,
    })
}

/// How glyph `code` is shown, where `drawable` says whether the interface fonts have a text's
/// characters: its sign; else a sign books use for it that they draw (`+=`, `=+`, `⊕`, `⇄`);
/// else `$code` (an assessment without a sign too: [`seen`] says how to show those).
pub(crate) fn face(code: u16, mut drawable: impl FnMut(&str) -> bool) -> String {
    let Some(n) = nag::by_code(code) else { return format!("${code}") };
    let others: &[&str] = match n.code {
        14 => &["+="],
        15 => &["=+"],
        132 => &["⇄", "↔"],
        138 => &["⊕"],
        _ => &[],
    };
    n.glyph.into_iter().chain(others.iter().copied()).find(|s| drawable(s)).map_or_else(|| format!("${code}"), str::to_owned)
}

/// How glyph `code` is seen in the interface: as text ([`face`]), painted ([`GlyphIcon`]), or,
/// for an assessment without a sign, by its name in the interface language.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Seen {
    Text(String),
    Icon(GlyphIcon),
    Name(&'static str),
}

/// [`Seen`] for `code`, with the fonts `drawable` describes. A code not in the table is text
/// (`$code`).
pub(crate) fn seen(code: u16, drawable: impl FnMut(&str) -> bool) -> Seen {
    if let Some(icon) = glyph_icon(code) {
        return Seen::Icon(icon);
    }
    match nag::by_code(code) {
        Some(n) if n.glyph.is_none() => Seen::Name(nag_name(code).unwrap_or(n.name)),
        _ => Seen::Text(face(code, drawable)),
    }
}

/// The signs painted because no interface font draws them (or no character exists).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GlyphIcon {
    /// `⌓` better is: a circle's segment.
    BetterIs,
    /// `⯺` connected pawns: two rings joined.
    ConnectedPawns,
    /// `⯻` isolated pawns: two rings kept apart by a bar.
    IsolatedPawns,
    /// `⯼` doubled pawns: two rings one above the other.
    DoubledPawns,
    /// `⯽` passed pawn: a ring with an arrow up.
    PassedPawn,
    /// `⯾` without: a right angle open to the top left.
    Without,
    /// `∟` with: a right angle open to the top right.
    With,
    /// `⇗` play along the diagonal: a double arrow up to the right.
    Diagonal,
    /// The pair of bishops: two bishops in outline.
    BishopPair,
    /// Bishops of opposite colors: one in outline, one solid.
    OppositeBishops,
    /// Bishops of the same color: two solid.
    SameBishops,
}

/// The painted sign of glyph `code` (either side's), if it has one.
pub(crate) fn glyph_icon(code: u16) -> Option<GlyphIcon> {
    Some(match nag::by_code(code)?.code {
        142 => GlyphIcon::BetterIs,
        249 => GlyphIcon::ConnectedPawns,
        250 => GlyphIcon::IsolatedPawns,
        251 => GlyphIcon::DoubledPawns,
        252 => GlyphIcon::PassedPawn,
        255 => GlyphIcon::Without,
        254 => GlyphIcon::With,
        240 => GlyphIcon::Diagonal,
        246 => GlyphIcon::BishopPair,
        247 => GlyphIcon::OppositeBishops,
        248 => GlyphIcon::SameBishops,
        _ => return None,
    })
}

/// Paint `icon` in `rect` (it takes the largest square in the middle) in `color`.
pub(crate) fn paint_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32, icon: GlyphIcon) {
    use egui::{Pos2, Shape, Stroke, pos2, vec2};
    let side = rect.width().min(rect.height());
    if !side.is_finite() || side <= 0.0 {
        return;
    }
    let sq = egui::Rect::from_center_size(rect.center(), vec2(side, side));
    // A point by its place in the square, 0–1 from the top left.
    let at = |x: f32, y: f32| pos2(sq.left() + x * side, sq.top() + y * side);
    let stroke = Stroke::new((side / 11.0).clamp(1.0, 2.0), color);
    let ring = |c: Pos2, r: f32| Shape::circle_stroke(c, r * side, stroke);
    let line = |a: Pos2, b: Pos2| Shape::line_segment([a, b], stroke);
    // A bishop: a body narrowing to the top, a head, a base; solid or in outline.
    let bishop = |x: f32, solid: bool| -> Vec<Shape> {
        let body = vec![at(x - 0.17, 0.82), at(x + 0.17, 0.82), at(x + 0.06, 0.38), at(x - 0.06, 0.38)];
        let base = vec![at(x - 0.22, 0.92), at(x + 0.22, 0.92), at(x + 0.22, 0.84), at(x - 0.22, 0.84)];
        let head = at(x, 0.27);
        if solid {
            vec![
                Shape::convex_polygon(body, color, Stroke::NONE),
                Shape::convex_polygon(base, color, Stroke::NONE),
                Shape::circle_filled(head, 0.1 * side, color),
            ]
        } else {
            vec![Shape::closed_line(body, stroke), Shape::closed_line(base, stroke), Shape::circle_stroke(head, 0.1 * side, stroke)]
        }
    };
    let shapes: Vec<Shape> = match icon {
        GlyphIcon::BetterIs => {
            // The arc over its chord.
            let (c, r) = (at(0.5, 0.7), 0.38 * side);
            let arc: Vec<Pos2> = (0..=16)
                .map(|i| c + r * vec2((std::f32::consts::PI * i as f32 / 16.0).cos(), -(std::f32::consts::PI * i as f32 / 16.0).sin()))
                .collect();
            vec![Shape::line(arc, stroke), line(at(0.1, 0.7), at(0.9, 0.7))]
        }
        GlyphIcon::ConnectedPawns => vec![ring(at(0.25, 0.5), 0.15), ring(at(0.75, 0.5), 0.15), line(at(0.4, 0.5), at(0.6, 0.5))],
        GlyphIcon::IsolatedPawns => vec![ring(at(0.2, 0.5), 0.14), ring(at(0.8, 0.5), 0.14), line(at(0.5, 0.25), at(0.5, 0.75))],
        GlyphIcon::DoubledPawns => vec![ring(at(0.5, 0.27), 0.15), ring(at(0.5, 0.73), 0.15)],
        GlyphIcon::PassedPawn => {
            vec![ring(at(0.5, 0.75), 0.15), line(at(0.5, 0.58), at(0.5, 0.1)), line(at(0.5, 0.1), at(0.33, 0.27)), line(at(0.5, 0.1), at(0.67, 0.27))]
        }
        GlyphIcon::Without => vec![line(at(0.75, 0.15), at(0.75, 0.8)), line(at(0.2, 0.8), at(0.75, 0.8))],
        GlyphIcon::With => vec![line(at(0.25, 0.15), at(0.25, 0.8)), line(at(0.25, 0.8), at(0.8, 0.8))],
        GlyphIcon::Diagonal => {
            // Two strokes side by side, up to the right, and the arrowhead over both.
            let (a, b) = (vec2(-0.06, 0.06) * side, vec2(0.06, -0.06) * side);
            let (from, to) = (at(0.2, 0.8), at(0.75, 0.25));
            vec![line(from + a, to + a), line(from + b, to + b), line(at(0.8, 0.2), at(0.45, 0.2)), line(at(0.8, 0.2), at(0.8, 0.55))]
        }
        GlyphIcon::BishopPair => [bishop(0.28, false), bishop(0.72, false)].concat(),
        GlyphIcon::OppositeBishops => [bishop(0.28, false), bishop(0.72, true)].concat(),
        GlyphIcon::SameBishops => [bishop(0.28, true), bishop(0.72, true)].concat(),
    };
    painter.extend(shapes);
}

/// A painted sign inline (`size` points square), named `name` for assistive technology and in
/// its tooltip.
pub(crate) fn icon_label(ui: &mut egui::Ui, icon: GlyphIcon, size: f32, color: egui::Color32, name: &str) -> egui::Response {
    let (rect, r) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    paint_icon(ui.painter(), rect, color, icon);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, name));
    r.on_hover_text(name)
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
    /// The correction field of a capture's preview (T10).
    Correction,
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

/// What glyph `code`, chosen in the palette or on the rail, types into the text field that had
/// the keyboard: its sign, when a field had it and the glyph has one. `None`: the glyph goes on
/// the move shown (an assessment without a sign, which as text would only be `$n`, always does).
pub(crate) fn glyph_typed(board: &BoardState, code: u16) -> Option<String> {
    board.typing.last?;
    nag::by_code(code).filter(|n| n.glyph.is_some()).map(|n| n.insert_text())
}

/// Glyph `code` chosen in the palette or on the rail: its sign goes into the text field that had
/// the keyboard ([`glyph_typed`], at its cursor), or else the glyph goes on the move shown or
/// comes off it (one undo step).
pub(crate) fn pick_glyph(board: &mut BoardState, code: u16) {
    match glyph_typed(board, code) {
        Some(text) => palette_insert(board, &text),
        None => toggle_glyph(board, code),
    }
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

/// What a palette button puts in: `text`, and for a glyph its code (White's), which goes where
/// [`pick_glyph`] says.
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
    let on: Vec<u16> = nag::NAGS.iter().map(|n| n.code).filter(|&c| has_glyph(board, c)).collect();
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
    // Never past the bottom of the window it floats in: its list takes the room left there
    // ([`more_assessments`]), and when that was short of [`MIN_LIST`] last frame (the list just
    // opened low in the window) the palette moves up by what was missing, as far as the top. A
    // palette that still ended below the window moves up too. Once the list has its room it stays.
    let viewport = ctx.content_rect();
    let shortfall = ctx.data_mut(|d| d.remove_temp::<f32>(Id::new(SHORTFALL_ID))).unwrap_or(0.0);
    if let Some(last) = ctx.memory(|m| m.area_rect(Id::new(PALETTE_ID)))
        && last.is_finite()
    {
        let up = shortfall.max(last.bottom() - viewport.bottom());
        if up.is_finite() && up > 0.0 && last.top() > viewport.top() {
            let top = (last.top() - up).max(viewport.top());
            window = window.current_pos(egui::pos2(last.left(), top));
        }
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
        if let Some(code) = more_assessments(ui, |c| on.contains(&c)) {
            pick = Some(Pick { text: format!("${code}"), glyph: Some(code) });
        }
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
        Some(code) => pick_glyph(board, code),
        None => palette_insert(board, &pick.text),
    }
}

/// The palette's buttons, one per character and each named for what it is: the figurines and
/// `½ – × † ‡`, then the glyphs in the rail's three groups and the positional signs, those no
/// font draws painted ([`GlyphIcon`]). For the correction field of a capture (T10,
/// `correction`) also the ellipsis Black's moves are printed with, no headings, and only the signs
/// that are characters. `on` says which glyphs show as ticked. Returns what the one clicked puts
/// in.
pub(crate) fn palette_buttons(ui: &mut egui::Ui, correction: bool, on: impl Fn(u16) -> bool) -> Option<Pick> {
    let ctx = ui.ctx().clone();
    let muted = crate::theme::Tokens::get(&ctx).text_muted;
    let mut chosen = None;
    let mut button = |ui: &mut egui::Ui, shown: Seen, pick: Pick, name: &str, selected: bool| {
        let text = match &shown {
            Seen::Text(s) => s.as_str(),
            Seen::Icon(_) | Seen::Name(_) => "",
        };
        let r = ui.add(egui::Button::new(egui::RichText::new(text).monospace()).min_size(egui::vec2(26.0, 24.0)).selected(selected));
        if let Seen::Icon(icon) = shown {
            let color = ui.style().interact_selectable(&r, selected).fg_stroke.color;
            paint_icon(ui.painter(), egui::Rect::from_center_size(r.rect.center(), egui::vec2(14.0, 14.0)), color, icon);
        }
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
            button(ui, Seen::Text(if figurines { figurine } else { letter }.to_owned()), text(figurine), name, false);
        }
        for (mark, name) in marks() {
            button(ui, Seen::Text(mark.to_owned()), text(mark), name, false);
        }
        if correction {
            button(ui, Seen::Text("\u{2026}".to_owned()), text("\u{2026}"), tl!("Ellipsis"), false);
        }
    });
    let groups: [(&str, &[u16]); 4] = [
        (tl!("Move glyphs"), &MOVE_GLYPHS),
        (tl!("Evaluation glyphs"), &EVALUATION_GLYPHS),
        (tl!("Other glyphs"), &OTHER_GLYPHS),
        (tl!("Positional glyphs"), &POSITIONAL_GLYPHS),
    ];
    for (title, codes) in groups {
        heading(ui, title);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(2.0, 2.0);
            for &code in codes {
                let Some(n) = nag::by_code(code) else { continue };
                // A correction is text: a sign without a character has nothing to type.
                if correction && n.glyph.is_none() {
                    continue;
                }
                let pick = Pick { text: n.insert_text(), glyph: Some(code) };
                button(ui, seen(code, |s| drawable(&ctx, s)), pick, nag_name(code).unwrap_or(n.name), on(code));
            }
        });
    }
    chosen
}

/// The sections of the assessments without a sign, in the palette's list.
fn assessment_sections() -> [(&'static str, NagGroup); 8] {
    [
        (tl!("Move quality"), NagGroup::Move),
        (tl!("Evaluation"), NagGroup::Evaluation),
        (tl!("Initiative and time"), NagGroup::Dynamics),
        (tl!("Space and control"), NagGroup::Space),
        (tl!("King safety"), NagGroup::King),
        (tl!("Pawns"), NagGroup::Pawns),
        (tl!("Pieces"), NagGroup::Pieces),
        (tl!("Phases of the game"), NagGroup::Phases),
    ]
}

/// The assessments without a sign and without a painted one: the palette's list.
pub(crate) fn assessments() -> impl Iterator<Item = &'static nag::Nag> {
    nag::NAGS.iter().filter(|n| n.glyph.is_none() && glyph_icon(n.code).is_none())
}

/// Whether assessment `n` matches the filter `filter` (lower case, trimmed): by its name in the
/// interface language or in English, or by its code (`24`, `$25`).
fn matches_filter(n: &nag::Nag, filter: &str) -> bool {
    let code = filter.strip_prefix('$').unwrap_or(filter);
    let by_code = !code.is_empty()
        && code.chars().all(|c| c.is_ascii_digit())
        && (n.code.to_string() == code || n.black_code.is_some_and(|b| b.to_string() == code));
    by_code || nag_name(n.code).unwrap_or(n.name).to_lowercase().contains(filter) || n.name.to_lowercase().contains(filter)
}

/// The list of assessments' height in the palette, in points: the palette moves up to give it at
/// least [`MIN_LIST`], it is never taller than [`MAX_LIST`], and never shorter than
/// [`FLOOR_LIST`] even where the window is too short for the palette.
const MIN_LIST: f32 = 120.0;
const MAX_LIST: f32 = 320.0;
const FLOOR_LIST: f32 = 24.0;

/// Where the list leaves, for the next frame, how many points it lacks of [`MIN_LIST`].
const SHORTFALL_ID: &str = "chess-glyph-palette-shortfall";

/// Room kept under the list for the palette's own bottom edge, in points: more than the window's
/// inner margin and frame, so a list as tall as the room leaves the palette inside the window.
const BOTTOM_MARGIN: f32 = 24.0;

/// The most characters of the list's filter kept.
const MAX_FILTER: usize = 64;

/// "More assessments" in the palette: the assessments that have no printed sign, folded away,
/// in sections by subject, or narrowed to those whose name (or `$n`) has the filter's text. Each
/// is its name, ticked while the move shown has it (`on`, White's code). Returns the one clicked
/// (White's code).
pub(crate) fn more_assessments(ui: &mut egui::Ui, on: impl Fn(u16) -> bool) -> Option<u16> {
    let muted = crate::theme::Tokens::get(ui.ctx()).text_muted;
    let mut chosen = None;
    let mut row = |ui: &mut egui::Ui, n: &nag::Nag| {
        let name = nag_name(n.code).unwrap_or(n.name);
        let codes = match n.black_code {
            Some(black) => format!("${} / ${black}", n.code),
            None => format!("${}", n.code),
        };
        let selected = on(n.code);
        let r = ui.selectable_label(selected, name);
        // Ticked for assistive technology as the palette's buttons are.
        r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, name));
        if r.on_hover_text(codes).clicked() {
            chosen = Some(n.code);
        }
    };
    egui::CollapsingHeader::new(tl!("More assessments")).id_salt("chess-palette-more").show(ui, |ui| {
        ui.add(
            egui::Label::new(egui::RichText::new(tl!("These have no printed sign: PGN writes them as a number ($n).")).small().color(muted))
                .selectable(false),
        );
        let id = ui.make_persistent_id("chess-palette-filter");
        let mut filter: String = ui.data_mut(|d| d.get_temp(id)).unwrap_or_default();
        let hint = tl!("Filter assessments");
        let r = ui.add(egui::TextEdit::singleline(&mut filter).hint_text(hint).desired_width(f32::INFINITY).char_limit(MAX_FILTER));
        labeled(&r, hint);
        let wanted = filter.trim().to_lowercase();
        ui.data_mut(|d| d.insert_temp(id, filter));
        // The list scrolls in the room left between here and the bottom of the window the palette
        // floats in, so the palette never runs past it; with less than [`MIN_LIST`] of room the
        // palette moves up next frame by what is missing (`palette_window`).
        let room = ui.ctx().content_rect().bottom() - ui.cursor().top() - BOTTOM_MARGIN;
        let room = if room.is_finite() { room } else { MIN_LIST };
        let height = room.clamp(FLOOR_LIST, MAX_LIST);
        ui.data_mut(|d| d.insert_temp(Id::new(SHORTFALL_ID), (MIN_LIST - room).max(0.0)));
        // As tall as that when it has more (a window that does not resize would otherwise keep it at its
        // first height), shorter when it has less.
        let area = egui::ScrollArea::vertical().id_salt("chess-palette-more-list");
        area.max_height(height).min_scrolled_height(height).show(ui, |ui| {
            if wanted.is_empty() {
                for (title, group) in assessment_sections() {
                    egui::CollapsingHeader::new(title).id_salt(("chess-palette-section", title)).show(ui, |ui| {
                        for n in assessments().filter(|n| n.group == group) {
                            row(ui, n);
                        }
                    });
                }
            } else {
                let mut any = false;
                for n in assessments().filter(|n| matches_filter(n, &wanted)) {
                    any = true;
                    row(ui, n);
                }
                if !any {
                    ui.label(egui::RichText::new(tl!("No assessment matches the filter.")).small().color(muted));
                }
            }
        });
    });
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
    fn the_palette_holds_every_glyph_once_with_its_name() {
        let mut codes: Vec<u16> = MOVE_GLYPHS.iter().chain(&EVALUATION_GLYPHS).chain(&OTHER_GLYPHS).chain(&POSITIONAL_GLYPHS).copied().collect();
        let rows = codes.len();
        codes.extend(assessments().map(|n| n.code));
        let all = codes.len();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), all, "no glyph twice");
        let mut table: Vec<u16> = nag::NAGS.iter().map(|n| n.code).collect();
        table.sort_unstable();
        assert_eq!(codes, table, "every entry of NAGS");
        assert_eq!(rows, 47, "the rows have the signs; the list the assessments without one");
        assert!(assessments().all(|n| assessment_sections().iter().any(|(_, g)| *g == n.group)), "every assessment in a section");
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
    fn the_filter_finds_assessments_by_name_and_code() {
        let names = |filter: &str| assessments().filter(|n| matches_filter(n, filter)).map(|n| n.code).collect::<Vec<_>>();
        assert_eq!(names("space"), [24, 28]);
        assert_eq!(names("$25"), [24], "Black's code finds the pair");
        assert_eq!(names("24"), [24]);
        assert_eq!(names("$"), [] as [u16; 0], "no code: no name has a dollar");
        assert!(names("opening").len() == 4);
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
        assert_eq!(face(25, all), "$25", "no sign");
        assert_eq!(seen(142, ascii), Seen::Icon(GlyphIcon::BetterIs));
        assert_eq!(seen(247, all), Seen::Icon(GlyphIcon::OppositeBishops), "no character at all");
        assert_eq!(seen(27, all), Seen::Text("○".to_owned()));
        let previous = crate::i18n::current();
        crate::i18n::set_current(crate::i18n::Lang::EN);
        assert_eq!(seen(25, all), Seen::Name("Slight space advantage"));
        crate::i18n::set_current(previous);
        assert_eq!(seen(999, all), Seen::Text("$999".to_owned()));
    }

    /// Every glyph in the table is seen as something a reader can tell: a sign the interface's own
    /// fonts draw (or the way books set it in plain text), a painted sign, or its name. Never
    /// `$n`, never a character the fonts lack.
    #[test]
    fn every_glyph_is_drawn_by_a_bundled_font_or_painted() {
        let ctx = egui::Context::default();
        ctx.set_fonts(crate::theme::font_definitions());
        // The fonts load with the first pass; its texture upload is not needed here.
        ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
        let mut painted = Vec::new();
        for n in nag::NAGS {
            for code in [Some(n.code), n.black_code].into_iter().flatten() {
                match seen(code, |s| drawable(&ctx, s)) {
                    Seen::Text(t) => assert!(!t.starts_with('$') && drawable(&ctx, &t), "${code} shows as {t}"),
                    Seen::Icon(icon) => painted.push((n.code, icon)),
                    Seen::Name(name) => assert!(n.glyph.is_none() && !name.is_empty(), "${code}"),
                }
            }
        }
        painted.dedup();
        let codes: Vec<u16> = painted.iter().map(|(c, _)| *c).collect();
        assert_eq!(codes, [142, 254, 255, 240, 249, 250, 251, 252, 246, 247, 248]);
        // A painted sign is one no bundled font draws, or a bishop sign, whose glyph is only a
        // stand-in (no character exists).
        for code in codes.into_iter().filter(|c| !(246..=248).contains(c)) {
            let n = nag::by_code(code).unwrap();
            assert!(n.glyph.is_none_or(|g| !drawable(&ctx, g)), "${code} could be text");
        }
        // Painting never panics, whatever the box.
        let painter = ctx.layer_painter(egui::LayerId::background());
        for (_, icon) in painted {
            for rect in [egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(14.0, 14.0)), egui::Rect::NOTHING, egui::Rect::EVERYTHING] {
                paint_icon(&painter, rect, egui::Color32::WHITE, icon);
            }
        }
    }
}
