//! pdfcraft-chess — L0 chess knowledge, with no PDF in it.
//!
//! Everything here works on chess values (placements, moves, probabilities, shapes), never on
//! documents or files, so it is standalone, builds for wasm32 and can be tested without a page.
//! `pdfcraft-chessvision` turns pages into these values and the engine turns them back into edits.
//!
//! Diagrams read from books can be illegal (two white kings after a misread square), so positions
//! are held in this crate's own [`placement`] type. `cozy-chess` (MIT) is used only once a
//! position is known to be legal.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod catalog;
pub mod describe;
pub mod draw;
pub mod fen;
pub mod fragment;
pub mod game;
pub mod hash;
pub mod judge;
pub mod lichess;
pub mod mate;
pub mod nag;
pub mod origin;
pub mod pattern;
pub mod pgn;
pub mod pgn_style;
pub mod placement;
pub mod recognize;
pub mod repair;
pub mod rules;
pub mod san;
pub mod spoken;
pub mod stipulation;
pub mod svg;
pub mod texture;
pub mod theme;
pub mod uci;

/// The legal moves on `board`, in the order `cozy-chess` generates them (plan A9-I6, finding
/// A7-19: the repairer, the mate search and the walker each had their own copy).
pub fn legal_moves(board: &cozy_chess::Board) -> Vec<cozy_chess::Move> {
    let mut out = Vec::new();
    board.generate_moves(|moves| {
        out.extend(moves);
        false
    });
    out
}

#[cfg(test)]
mod tests {
    use cozy_chess::Board;

    /// Plan A9-I6: the one list of legal moves gives what the three copies it replaced gave (the
    /// repairer's, the mate search's and the walker's, run before the change), in the same order.
    #[test]
    fn the_legal_moves_are_those_the_copies_gave() {
        let uci = |fen: &str| super::legal_moves(&fen.parse::<Board>().unwrap()).iter().map(|m| m.to_string()).collect::<Vec<_>>().join(" ");
        assert_eq!(
            uci("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"),
            "a2a3 a2a4 b2b3 b2b4 c2c3 c2c4 d2d3 d2d4 e2e3 e2e4 f2f3 f2f4 g2g3 g2g4 h2h3 h2h4 b1a3 b1c3 g1f3 g1h3"
        );
        assert_eq!(uci("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1").split(' ').count(), 48);
        assert!(super::legal_moves(&"7k/5Q2/6K1/8/8/8/8/8 b - - 0 1".parse::<Board>().unwrap()).is_empty(), "stalemate: none");
    }
}
