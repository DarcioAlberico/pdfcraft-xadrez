//! Lichess analysis links for a position.
//!
//! This only builds URLs. Nothing in PdfCraft fetches them: they go into link annotations and
//! exported documents, and the user opens them.
//!
//! The URL shown in the interface and the one written into the PDF must be the same string, so
//! this module is the single place that builds it.

use crate::fen::{Position, format_placement};
use crate::placement::Color;

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

    #[test]
    fn encoding_keeps_only_unreserved_bytes() {
        assert_eq!(percent_encode("a-._~ /?#é"), "a-._~%20%2F%3F%23%C3%A9");
    }
}
