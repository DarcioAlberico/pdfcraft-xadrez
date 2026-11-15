//! Lichess analysis links for a position.
//!
//! This only builds URLs. Nothing in PdfCraft fetches them: they go into link annotations and
//! exported documents, and the user opens them.
//!
//! The URL shown in the interface and the one written into the PDF must be the same string, so
//! this module is the single place that builds it, and [`link`] the single place that says which
//! positions get one (plan A9-G9): a legal position the decoder did not repair, from the side the
//! diagram is printed from. The reader's readings, the diagrams of an export, the lichess labels
//! written into the PDF and a position a person confirmed all go through it.

use crate::fen::{Position, format_placement};
use crate::placement::Color;

/// Why a position gets no lichess link ([`link`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NoLink {
    /// The text is not a FEN; the parser's reason.
    NotFen(String),
    /// The decoder repaired the reading: a guess is not published as the book's position.
    Repaired,
    /// The position is not legal with its side to move, or no board can be set up from it; the
    /// first rule it breaks, when one is named.
    NotLegal(Option<String>),
}

impl std::fmt::Display for NoLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NoLink::NotFen(e) => write!(f, "the position is not a FEN: {e}"),
            NoLink::Repaired => f.write_str("the reading was repaired, so it needs review first"),
            NoLink::NotLegal(Some(v)) => write!(f, "the position is not legal: {v}"),
            NoLink::NotLegal(None) => f.write_str("the position is not legal"),
        }
    }
}

/// The lichess analysis link of the position `fen`, seen from `orientation` (the side the diagram
/// is printed from), or why it gets none: it must be a FEN, legal with its side to move (White
/// when it says none) and a board a game can start from, and not `repaired`.
pub fn link(fen: &str, orientation: Color, repaired: bool) -> Result<String, NoLink> {
    let position = crate::fen::parse(fen).map_err(|e| NoLink::NotFen(e.to_string()))?;
    if repaired {
        return Err(NoLink::Repaired);
    }
    let side = position.side_to_move.unwrap_or(Color::White);
    let legal = crate::rules::check_position(&position.placement).allows(side) && crate::fen::board_from_fen(fen.trim()).is_ok();
    if !legal {
        let why = crate::rules::position_violations(&position, Color::White).first().map(ToString::to_string);
        return Err(NoLink::NotLegal(why));
    }
    Ok(analysis_url(&position, orientation))
}

/// The analysis board, with no position.
pub const ANALYSIS_BASE: &str = "https://lichess.org/analysis";

/// The lichess analysis URL for `position`, seen from `orientation`.
///
/// The placement goes into the path as is (its `/` separate ranks there too). When the side to
/// move is known, the rest of the FEN follows, percent-encoded. When it is not, the link carries
/// the placement alone and lichess picks the side. Black's orientation adds `?color=black`, for
/// diagrams printed from Black's side.
pub fn analysis_url(position: &Position, orientation: Color) -> String {
    let mut url = format!("{ANALYSIS_BASE}/{}", format_placement(&position.placement));
    if let Some(tail) = position.fen_tail() {
        url.push_str(&percent_encode(&format!(" {tail}")));
    }
    if orientation == Color::Black {
        url.push_str("?color=black");
    }
    url
}

/// Percent-encode everything except the unreserved characters of RFC 3986 §2.3.
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            out.push('%');
            out.push(hex_digit(b >> 4));
            out.push(hex_digit(b & 0x0f));
        }
    }
    out
}

fn hex_digit(n: u8) -> char {
    char::from(if n < 10 { b'0' + n } else { b'A' + (n - 10) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fen::parse;

    #[test]
    fn placement_only_when_the_side_is_unknown() {
        let pos = parse("8/8/8/4k3/8/8/8/4K3").unwrap();
        assert_eq!(analysis_url(&pos, Color::White), "https://lichess.org/analysis/8/8/8/4k3/8/8/8/4K3");
    }

    #[test]
    fn the_rest_of_the_fen_is_percent_encoded() {
        let pos = parse("8/8/8/4k3/8/8/8/4K3 b - - 0 1").unwrap();
        assert_eq!(analysis_url(&pos, Color::White), "https://lichess.org/analysis/8/8/8/4k3/8/8/8/4K3%20b%20-%20-%200%201");
        let start = parse("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        assert_eq!(
            analysis_url(&start, Color::White),
            "https://lichess.org/analysis/rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR%20w%20KQkq%20-%200%201"
        );
    }

    #[test]
    fn black_orientation_is_a_query_parameter() {
        let pos = parse("8/8/8/4k3/8/8/8/4K3").unwrap();
        assert_eq!(analysis_url(&pos, Color::Black), "https://lichess.org/analysis/8/8/8/4k3/8/8/8/4K3?color=black");
    }

    /// Plan A9-G9: one rule for every link: legal and not repaired, from the side it is printed
    /// from; the rest say why.
    #[test]
    fn a_link_is_for_a_legal_position_not_repaired_from_its_side() {
        let start = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
        let url = "https://lichess.org/analysis/rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR%20w%20KQkq%20-%200%201";
        assert_eq!(link(start, Color::White, false).as_deref(), Ok(url));
        assert_eq!(link(start, Color::Black, false), Ok(format!("{url}?color=black")));
        assert_eq!(link(start, Color::White, true), Err(NoLink::Repaired));
        // No kings; a side to move giving check; not a FEN.
        let no_kings = link("8/8/8/8/8/8/8/8 w - - 0 1", Color::White, false).unwrap_err();
        assert!(matches!(no_kings, NoLink::NotLegal(_)) && no_kings.to_string().starts_with("the position is not legal"), "{no_kings}");
        assert!(
            matches!(link("4k2R/8/8/8/8/8/8/4K3 w - - 0 1", Color::White, false), Err(NoLink::NotLegal(_))),
            "White to move while Black is in check"
        );
        assert!(link("4k2R/8/8/8/8/8/8/4K3 b - - 0 1", Color::White, false).is_ok(), "Black to move, in check");
        assert!(matches!(link("not a fen", Color::White, false), Err(NoLink::NotFen(_))));
        assert_eq!(NoLink::Repaired.to_string(), "the reading was repaired, so it needs review first");
    }

    #[test]
    fn encoding_keeps_only_unreserved_bytes() {
        assert_eq!(percent_encode("a-._~ /?#é"), "a-._~%20%2F%3F%23%C3%A9");
    }
}
