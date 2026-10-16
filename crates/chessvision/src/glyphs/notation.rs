//! What counts as chess notation in a recognised word: the routing rule of the fusion and the
//! guard of the move-number join.
//!
//! The move-token shape is Caïssa's `MOVE_TOKEN` (the S13a spike's scorer port: castling, or an
//! optional piece, optional file, rank and capture, then a square, a promotion and a check
//! mark, with the same look-around), without its NFKC step, which a recogniser's output does not
//! need: the folds that matter here (dashes, quotes, the ellipsis, `×`) are applied directly.

/// Folded to ASCII before deciding (Caïssa's `normalise`).
const FOLD: &[(char, &str)] = &[
    ('\u{ad}', "-"),
    ('‐', "-"),
    ('‑', "-"),
    ('‒', "-"),
    ('–', "-"),
    ('—', "-"),
    ('−', "-"),
    ('‘', "'"),
    ('’', "'"),
    ('‚', "'"),
    ('“', "\""),
    ('”', "\""),
    ('„', "\""),
    ('…', "..."),
    ('×', "x"),
];

/// Letters that end a word but may follow a move (`♘f3²`).
const WORDLIKE_SUFFIX: &[char] = &['²', '³', 'µ', 'μ', 'ʘ', 'ƒ', 'Δ', '¹'];

/// Marks that make a word notation on their own (`!?`, `+-`, `=`).
const MARKS: &str = "!?+-=#";

pub(crate) fn is_figurine(c: char) -> bool {
    ('♔'..='♟').contains(&c)
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn is_piece(c: char) -> bool {
    "KQRBNPDTCSAFLГЛСКФП".contains(c) || is_figurine(c)
}

fn is_promo(c: char) -> bool {
    "QRBNDTCSAFL".contains(c) || is_figurine(c)
}

/// `text` with the folds applied, `0-0` read as castling and whitespace collapsed.
pub(crate) fn fold(text: &str) -> Vec<char> {
    let mut out: Vec<char> = Vec::with_capacity(text.len());
    for c in text.chars() {
        match FOLD.iter().find(|(f, _)| *f == c) {
            Some((_, to)) => out.extend(to.chars()),
            None => out.push(c),
        }
    }
    let out = replace_castle(&out, "0-0-0", "O-O-O");
    let out = replace_castle(&out, "0-0", "O-O");
    let mut collapsed = Vec::with_capacity(out.len());
    for c in out {
        if c.is_whitespace() {
            if collapsed.last().is_some_and(|l: &char| *l != ' ') {
                collapsed.push(' ');
            }
        } else {
            collapsed.push(c);
        }
    }
    if collapsed.last() == Some(&' ') {
        collapsed.pop();
    }
    collapsed
}

fn replace_castle(chars: &[char], pat: &str, rep: &str) -> Vec<char> {
    let p: Vec<char> = pat.chars().collect();
    let mut out = Vec::with_capacity(chars.len());
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        let fits = chars.get(i..i + p.len()).is_some_and(|s| s == p.as_slice());
        let before_ok = i == 0 || chars.get(i - 1).is_none_or(|&b| !is_word(b));
        let after_ok = chars.get(i + p.len()).is_none_or(|&a| !is_word(a));
        if fits && before_ok && after_ok {
            out.extend(rep.chars());
            i += p.len();
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

fn tail_ok(c: &[char], end: usize) -> bool {
    if let Some(&n) = c.get(end)
        && !WORDLIKE_SUFFIX.contains(&n)
        && is_word(n)
    {
        return false;
    }
    let two = |i: usize| c.get(i).is_some_and(char::is_ascii_alphabetic) && c.get(i + 1).is_some_and(char::is_ascii_alphabetic);
    !(two(end) || (c.get(end) == Some(&'-') && two(end + 1)))
}

fn file(c: Option<&char>) -> bool {
    c.is_some_and(|c| ('a'..='h').contains(c))
}

fn rank(c: Option<&char>) -> bool {
    c.is_some_and(|c| ('1'..='8').contains(c))
}

/// The end of a move token starting at `i`, trying the pattern's alternatives in its greedy
/// order.
fn match_at(c: &[char], i: usize) -> Option<usize> {
    let lit = |at: usize, s: &str| -> Option<usize> {
        let p: Vec<char> = s.chars().collect();
        (c.get(at..at + p.len()) == Some(p.as_slice())).then_some(at + p.len())
    };
    for castle in ["O-O-O", "O-O"] {
        if let Some(e) = lit(i, castle)
            && tail_ok(c, e)
        {
            return Some(e);
        }
    }
    for piece in [true, false] {
        if piece && !c.get(i).is_some_and(|&x| is_piece(x)) {
            continue;
        }
        let p = i + usize::from(piece);
        for f_opt in [true, false] {
            if f_opt && !file(c.get(p)) {
                continue;
            }
            let q = p + usize::from(f_opt);
            for r_opt in [true, false] {
                if r_opt && !rank(c.get(q)) {
                    continue;
                }
                let r = q + usize::from(r_opt);
                for x_opt in [true, false] {
                    if x_opt && c.get(r) != Some(&'x') {
                        continue;
                    }
                    let s = r + usize::from(x_opt);
                    if !(file(c.get(s)) && rank(c.get(s + 1))) {
                        continue;
                    }
                    let sq = s + 2;
                    let mut promos = Vec::with_capacity(3);
                    if c.get(sq) == Some(&'=') && c.get(sq + 1).is_some_and(|&x| is_promo(x)) {
                        promos.push(sq + 2);
                    }
                    if c.get(sq).is_some_and(|&x| is_promo(x)) {
                        promos.push(sq + 1);
                    }
                    promos.push(sq);
                    for pe in promos {
                        let check = c.get(pe).is_some_and(|&x| x == '+' || x == '#');
                        for e in [pe + 1, pe].into_iter().skip(usize::from(!check)) {
                            if tail_ok(c, e) {
                                return Some(e);
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

/// Whether folded text holds a move token (`♖b7+`, `exd5`, `O-O`, `e8=Q`).
pub(crate) fn has_move(c: &[char]) -> bool {
    (0..c.len()).any(|i| (i == 0 || c.get(i - 1).is_none_or(|&p| !is_word(p))) && match_at(c, i).is_some())
}

/// Whether a glyph-reader word is chess notation, and so the glyph reader's to decide: a
/// figurine or a chess symbol (anything not ASCII and not a letter), a move token, a move number
/// (`24`, `24...`, `24)`) or marks only (`!?`, `+-`).
pub(crate) fn is_notation(word: &str) -> bool {
    let n = fold(word);
    if n.iter().any(|c| !c.is_ascii() && !c.is_alphabetic()) || has_move(&n) {
        return true;
    }
    let core_end = n.iter().rposition(|c| !".,;:)(".contains(*c)).map_or(0, |i| i + 1);
    let core = n.get(..core_end).unwrap_or(&[]);
    let digits_dots = !core.is_empty() && core.iter().all(char::is_ascii_digit) && n.iter().all(|c| c.is_ascii_digit() || ".,;:()".contains(*c));
    let marks = !n.is_empty() && n.iter().all(|c| MARKS.contains(*c));
    digits_dots || marks
}
