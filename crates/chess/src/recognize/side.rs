//! Who is to move, and how that is known.
//!
//! Few books say whose move a diagram is, but the position itself often does, because of one law
//! of chess: the side not to move cannot be in check. A diagram that is illegal with White to move
//! and legal with Black to move is not a misreading: it is Black's move.
//!
//! The cascade, with the source always recorded so that "Black to move" can be checked rather
//! than just believed:
//!
//! 1. what the text says (a caption, a move number such as `22...`), when the position allows it;
//! 2. what legality proves;
//! 3. the default, White, marked as assumed.
//!
//! When the text and the position disagree, the position wins (a FEN known to be illegal helps
//! nobody) and the conflict is flagged for review: either a piece was misread or the caption
//! belongs to the neighbouring diagram.

use crate::fen::{Castling, Position};
use crate::placement::{Color, Piece, PieceKind, Placement, Square};
use crate::rules::{Legality, check_position};

/// Where the side to move came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SideSource {
    /// Declared in the PDF's text near the diagram.
    Text,
    /// Read by OCR from the caption.
    Ocr,
    /// Read by the chess glyph classifier from the caption.
    Glyph,
    /// Declared in the page heading, for every diagram on the page.
    TextPageScope,
    OcrPageScope,
    GlyphPageScope,
    /// From the number of the first move under the diagram (`22...`).
    MoveNumber,
    /// From a caption such as "after 22.Nf3".
    CaptionAfter,
    /// Proved by the position: only one side to move is legal.
    Legality,
    /// From the game a database matched.
    Database,
    /// Set by hand.
    Manual,
    /// Nothing said: White, assumed.
    Default,
}

/// The side to move, where it came from, and whether the sources disagreed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SideToMove {
    pub color: Color,
    pub source: SideSource,
    /// The text said one thing and the position proved the other; the position won.
    pub conflicting: bool,
}

impl SideToMove {
    pub fn is_assumed(&self) -> bool {
        self.source == SideSource::Default
    }
}

/// The side to move the position forces, when only one is legal.
pub fn proven_by_legality(placement: &Placement) -> Option<Color> {
    match check_position(placement) {
        Legality::OnlyWithSideToMove { side, .. } => Some(side),
        Legality::Legal | Legality::Illegal(_) => None,
    }
}

/// Decide the side to move from what a source `declared` (if anything) and the position.
pub fn infer_side_to_move(placement: &Placement, declared: Option<(Color, SideSource)>) -> SideToMove {
    let proven = proven_by_legality(placement);
    match (declared, proven) {
        (Some((color, source)), None) => SideToMove { color, source, conflicting: false },
        (Some((color, source)), Some(p)) if p == color => SideToMove { color, source, conflicting: false },
        (Some(_), Some(p)) => SideToMove { color: p, source: SideSource::Legality, conflicting: true },
        (None, Some(p)) => SideToMove { color: p, source: SideSource::Legality, conflicting: false },
        (None, None) => SideToMove { color: Color::White, source: SideSource::Default, conflicting: false },
    }
}

/// Castling rights compatible with where the pieces stand: a king on its home square and a rook
/// in the corner. Conservative by nature (the position cannot say whether the king has moved) and
/// recorded as inferred, not read.
pub fn infer_castling(placement: &Placement) -> Castling {
    let at = |name: &str, color, kind| Square::parse(name).and_then(|sq| placement.get(sq)) == Some(Piece::new(color, kind));
    let white_king = at("e1", Color::White, PieceKind::King);
    let black_king = at("e8", Color::Black, PieceKind::King);
    Castling {
        white_king_side: white_king && at("h1", Color::White, PieceKind::Rook),
        white_queen_side: white_king && at("a1", Color::White, PieceKind::Rook),
        black_king_side: black_king && at("h8", Color::Black, PieceKind::Rook),
        black_queen_side: black_king && at("a8", Color::Black, PieceKind::Rook),
    }
}

/// A full position from a placement and the decided side to move. Move number 0 becomes 1: FEN
/// has no move zero, and refusing the export over a mistyped field would be out of proportion.
pub fn compose_position(placement: Placement, side: Color, castling: Option<Castling>, fullmove: u32) -> Position {
    let castling = castling.unwrap_or_else(|| infer_castling(&placement));
    Position { placement, side_to_move: Some(side), castling, en_passant: None, halfmove_clock: 0, fullmove_number: fullmove.max(1) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fen::parse_placement;

    fn p(fen: &str) -> Placement {
        parse_placement(fen).unwrap()
    }

    // The black king on e8 is attacked by the rook on e1: only Black can be to move.
    const BLACK_MUST_MOVE: &str = "4k3/8/8/8/8/8/8/K3R3";

    #[test]
    fn legality_proves_the_side() {
        let s = infer_side_to_move(&p(BLACK_MUST_MOVE), None);
        assert_eq!(s, SideToMove { color: Color::Black, source: SideSource::Legality, conflicting: false });
    }

    #[test]
    fn the_text_answers_when_both_sides_are_legal() {
        let s = infer_side_to_move(&p("4k3/8/8/8/8/8/8/4K3"), Some((Color::Black, SideSource::MoveNumber)));
        assert_eq!(s, SideToMove { color: Color::Black, source: SideSource::MoveNumber, conflicting: false });
    }

    #[test]
    fn the_position_overrules_the_text_and_says_so() {
        let s = infer_side_to_move(&p(BLACK_MUST_MOVE), Some((Color::White, SideSource::Text)));
        assert_eq!(s, SideToMove { color: Color::Black, source: SideSource::Legality, conflicting: true });
        let agree = infer_side_to_move(&p(BLACK_MUST_MOVE), Some((Color::Black, SideSource::Text)));
        assert_eq!(agree.source, SideSource::Text);
    }

    #[test]
    fn nothing_known_is_white_assumed() {
        let s = infer_side_to_move(&p("4k3/8/8/8/8/8/8/4K3"), None);
        assert!(s.is_assumed());
        assert_eq!(s.color, Color::White);
        // An impossible position proves nothing either.
        assert!(infer_side_to_move(&p("8/8/8/8/8/8/8/K6K"), None).is_assumed());
    }

    #[test]
    fn castling_follows_kings_and_rooks_at_home() {
        let c = infer_castling(&Placement::standard());
        assert_eq!(c.fen(), "KQkq");
        assert_eq!(infer_castling(&p("r3k3/8/8/8/8/8/8/4K2R")).fen(), "Kq");
        assert_eq!(infer_castling(&p("r3k2r/8/8/8/8/8/8/3K3R")).fen(), "kq");
    }

    #[test]
    fn composed_positions_are_complete() {
        let pos = compose_position(Placement::standard(), Color::White, None, 0);
        assert_eq!(pos.fen(), "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1");
        assert!(pos.to_board(Color::White).is_ok());
    }
}
