//! Descriptive notation (plan A9-L7, R6-07): `P-K4`, `N-KB3`, `PxP` in English, `P4R`, `C3AR`,
//! `C3BR` in Spanish and Portuguese, where a square is named by the piece whose file it is and by
//! the rank counted from the side that moves. Older books are printed in it. The reader does not
//! convert it (decision D27): it only knows it, so that a page of it says so instead of giving no
//! game and no word ([`descriptive_words`]).

use super::{ChessPageText, ChessRun};

/// Fewest words of descriptive notation that make a page one printed in it.
pub const MIN_DESCRIPTIVE_WORDS: usize = 3;

/// Most words of it kept as examples ([`Descriptive::examples`]).
pub const MAX_EXAMPLES: usize = 3;

/// What a page holds of descriptive notation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Descriptive {
    /// How many words of it.
    pub words: usize,
    /// The first few, as printed.
    pub examples: Vec<String>,
}

/// The descriptive notation in the prose of `text` (its text runs: the reader read none of it as
/// a move).
pub fn descriptive_words(text: &ChessPageText) -> Descriptive {
    let mut out = Descriptive::default();
    for span in text.lines.iter().flat_map(|l| &l.spans) {
        let ChessRun::Text(t) = &span.run else { continue };
        for word in t.split_whitespace() {
            let w = trimmed(word);
            if is_descriptive(w) {
                out.words = out.words.saturating_add(1);
                if out.examples.len() < MAX_EXAMPLES {
                    out.examples.push(w.to_owned());
                }
            }
        }
    }
    out
}

/// `word` without a move number before it (`12.`, `12...`), brackets, and the punctuation, check
/// (`+`, `ch`) and assessment marks after it.
fn trimmed(word: &str) -> &str {
    let w = word.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == '(' || c == '[' || c == '…');
    let w = w.trim_end_matches([',', '.', ';', ':', ')', ']', '!', '?', '+', '#']);
    w.strip_suffix("ch").filter(|r| !r.is_empty()).unwrap_or(w)
}

/// Whether `word` is a move in descriptive notation, English (`P-K4`, `N-KB3`, `QxP`, `KtxB`) or
/// Spanish and Portuguese (`P4R`, `C3AR`, `C3BR`, `PxP`). Algebraic moves never are: a capture
/// there takes a square, not a piece, and a square there is a small letter and a digit.
pub fn is_descriptive(word: &str) -> bool {
    english(word) || iberian(word)
}

/// An English piece, maybe on one side (`KN`, `QB`, `QRP`): what moves or is taken.
fn english_piece(s: &str) -> Option<&str> {
    let s = s.strip_prefix(['K', 'Q']).filter(|r| r.starts_with(['R', 'N', 'B']) || r.starts_with("Kt")).unwrap_or(s);
    let s = ["Kt", "N", "B", "R", "Q", "K", "P"].iter().find_map(|p| s.strip_prefix(p))?;
    // A pawn named by its file (`QRP`).
    Some(s.strip_prefix('P').unwrap_or(s))
}

/// An English square: a side (`K`, `Q`), the file's piece (`R`, `N`, `Kt`, `B`), a rank (`KB3`,
/// `Q4`, `R5`, `QR1`).
fn english_square(s: &str) -> bool {
    let s = s.strip_prefix(['K', 'Q']).unwrap_or(s);
    let s = ["Kt", "N", "B", "R"].iter().find_map(|p| s.strip_prefix(p)).unwrap_or(s);
    s.len() == 1 && s.chars().all(|c| ('1'..='8').contains(&c))
}

fn english(word: &str) -> bool {
    let Some(rest) = english_piece(word) else { return false };
    if let Some(to) = rest.strip_prefix('-') {
        return english_square(to);
    }
    // A capture takes a piece: `PxP`, `BxN`, `QxKBP`.
    rest.strip_prefix('x').and_then(english_piece).is_some_and(str::is_empty)
}

/// The pieces in Spanish and Portuguese: pawn, knight (`C`), bishop (`A`, `B`), rook (`T`),
/// queen (`D`), king (`R`).
const IBERIAN_PIECES: [char; 7] = ['P', 'C', 'A', 'B', 'T', 'D', 'R'];

fn iberian(word: &str) -> bool {
    let mut chars = word.chars();
    if !chars.next().is_some_and(|c| IBERIAN_PIECES.contains(&c)) {
        return false;
    }
    let rest: Vec<char> = chars.collect();
    match rest.as_slice() {
        // A capture: `PxP`, `AxC`.
        ['x', taken] => IBERIAN_PIECES.contains(taken),
        // A move: a rank, the file's piece, maybe its side (`4R`, `3AR`, `5C`).
        [rank, file, side @ ..] => {
            ('1'..='8').contains(rank)
                && ['T', 'C', 'A', 'B', 'D', 'R'].contains(file)
                && match side {
                    [] => true,
                    [s] => *s == 'R' || *s == 'D',
                    _ => false,
                }
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptive_moves_are_known_and_algebraic_ones_are_not() {
        for w in ["P-K4", "N-KB3", "PxP", "BxN", "Q-R5", "KKt-B3", "QR-Q1", "QxKBP", "B-QN5", "P4R", "C3AR", "C3BR", "A5C", "PxP", "AxC", "D1D"] {
            assert!(is_descriptive(w), "{w}");
        }
        for w in [
            "Nf3", "e4", "Rxd5", "Bxe5", "O-O", "e2-e4", "Ng1-f3", "Cf3", "Axf7", "Dxd8", "K", "P", "PK4", "N-K9", "P-", "1-0", "R-B", "Kxe2", "KB",
            "PDF", "ABC",
        ] {
            assert!(!is_descriptive(w), "{w}");
        }
        assert_eq!(trimmed("12.P-K4!"), "P-K4");
        assert_eq!(trimmed("NxPch,"), "NxP");
        assert_eq!(trimmed("(B-N5+)"), "B-N5");
    }
}
