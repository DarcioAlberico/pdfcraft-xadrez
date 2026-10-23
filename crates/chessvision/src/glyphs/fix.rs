//! Corrections after classification (the trunk's `empilhados.corrigir`, `caixa_alta.decidir`,
//! `numero.corrigir`), the word rule, and the move-number join. A 32 × 32 cell has lost the
//! glyph's size and proportion; these put back what geometry and context still know.

use super::classify::{Classes, Read};
use super::notation::{fold, has_move, is_figurine};
use super::seg::{Bx, SHORT_BOX};
use super::{GlyphWord, union_rect};

/// Letters whose capital is the small letter drawn bigger.
const CASE_BY_HEIGHT: &str = "cosuvwxzCOSUVWXZ";
/// A case-ambiguous glyph this many times the line's median height is a capital.
const CAPITAL_HEIGHT: f64 = 1.25;
/// A box read as a dash but narrower than this many heights is a dot.
const SQUARE_DOT_ASPECT: f64 = 1.5;
/// A box ending this close above the baseline, against the median height, sits on it.
const BASELINE_SLACK: f64 = 0.12;
/// A gap wider than this share of the median glyph width is a space.
const WORD_GAP: f64 = 0.42;
/// Marks that always stick to the previous word.
const STICKY_MARKS: &str = ".!?+#,;:";
/// Most digits in a move number the join makes.
const MAX_MOVE_NUMBER_DIGITS: usize = 3;

/// The single character a read stands for, if its class is one character.
fn single(classes: &Classes, r: &Read) -> Option<char> {
    let mut cs = classes.text(r.class).chars();
    match (cs.next(), cs.next()) {
        (Some(c), None) => Some(c),
        _ => None,
    }
}

/// `empilhados.corrigir`: a stacked glyph wider than tall is `=`, else `:`.
pub(crate) fn stacked(reads: &mut [Read], boxes: &[Bx], classes: &Classes) {
    let (Some(eq), Some(colon)) = (classes.of("="), classes.of(":")) else { return };
    for (r, b) in reads.iter_mut().zip(boxes) {
        if !matches!(classes.text(r.class), "=" | ":" | ";") {
            continue;
        }
        let wide = b.w() as f64 / (b.h().max(1) as f64) >= 1.0;
        if wide && r.class != eq {
            r.set(eq);
        } else if !wide && r.class == eq {
            r.set(colon);
        }
    }
}

/// A dash floats at mid height and is wider than tall; a box read as a dash that sits on the
/// baseline and is not wide is a square dot stretched to the 32 × 32 cell (DejaVu's `...` read
/// `. —.`), so it is `.`. Some books' hyphens are as square, which is why the baseline decides.
pub(crate) fn square_dots(reads: &mut [Read], boxes: &[Bx], classes: &Classes) {
    let Some(dot) = classes.of(".") else { return };
    let mut hs: Vec<i64> = boxes.iter().map(Bx::h).collect();
    hs.sort_unstable();
    let med = hs.get(hs.len() / 2).copied().unwrap_or(0) as f64;
    let mut bottoms: Vec<i64> = boxes.iter().filter(|b| b.h() as f64 >= SHORT_BOX * med).map(|b| b.y1).collect();
    bottoms.sort_unstable();
    let Some(&baseline) = bottoms.get(bottoms.len() / 2) else { return };
    for (r, b) in reads.iter_mut().zip(boxes) {
        let on_baseline = (baseline - b.y1) as f64 <= BASELINE_SLACK * med;
        if matches!(classes.text(r.class), "-" | "–" | "—") && on_baseline && (b.w() as f64) < SQUARE_DOT_ASPECT * b.h() as f64 {
            r.set(dot);
        }
    }
}

/// `caixa_alta.decidir`: the case of `c o s u v w x z` from the box height against the line's
/// median height.
pub(crate) fn case_by_height(reads: &mut [Read], boxes: &[Bx], classes: &Classes) {
    let mut hs: Vec<f64> = boxes.iter().map(|b| b.h() as f64).collect();
    hs.sort_by(f64::total_cmp);
    let n = hs.len();
    let xh = match (hs.get(n / 2), n.checked_sub(1).and_then(|m| hs.get(m / 2))) {
        (Some(hi), Some(lo)) if n.is_multiple_of(2) => (hi + lo) / 2.0,
        (Some(hi), _) => *hi,
        _ => return,
    };
    if xh <= 0.0 {
        return;
    }
    for (r, b) in reads.iter_mut().zip(boxes) {
        let Some(c) = single(classes, r) else { continue };
        if !CASE_BY_HEIGHT.contains(c) {
            continue;
        }
        let tall = b.h() as f64 / xh >= CAPITAL_HEIGHT;
        if c.is_uppercase() != tall {
            let other: String = if tall { c.to_uppercase().collect() } else { c.to_lowercase().collect() };
            if let Some(i) = classes.of(&other) {
                r.set(i);
            }
        }
    }
}

/// The Caïssa `words_from_glyphs` rule: a gap wider than 0.42 of the median glyph width is a
/// space, but a lone mark (`.!?+#,;:`) always sticks to the word before it. Ranges into `boxes`.
pub(crate) fn words(boxes: &[Bx], reads: &[Read], classes: &Classes) -> Vec<(usize, usize)> {
    if boxes.is_empty() {
        return Vec::new();
    }
    let mut ws: Vec<i64> = boxes.iter().map(Bx::w).collect();
    ws.sort_unstable();
    let med = ws.get(ws.len() / 2).copied().unwrap_or(0) as f64;
    let limit = WORD_GAP * if med == 0.0 { 1.0 } else { med };
    let mut out = Vec::new();
    let mut start = 0;
    for (i, pair) in boxes.windows(2).enumerate() {
        let [prev, cur] = pair else { continue };
        let t = reads.get(i + 1).map_or("", |r| classes.text(r.class));
        let lone_mark = !t.is_empty() && t.chars().all(|c| STICKY_MARKS.contains(c));
        if (cur.x0 - prev.x1) as f64 > limit && !lone_mark {
            out.push((start, i + 1));
            start = i + 1;
        }
    }
    out.push((start, boxes.len()));
    out
}

/// `numero.corrigir`: an oval inside a number is `0`, and `O.O` / `o-o-o` is castling.
pub(crate) fn numbers(reads: &mut [Read], words: &[(usize, usize)], classes: &Classes) {
    let Some(zero) = classes.of("0") else { return };
    let dash = classes.of("-");
    for &(s, e) in words {
        let Some(piece) = reads.get_mut(s..e) else { continue };
        let tok: Vec<char> = piece.iter().filter_map(|r| single(classes, r)).collect();
        if tok.len() != piece.len() {
            continue;
        }
        let spans = number_spans(&tok);
        if !spans.is_empty() {
            for (a, b) in spans {
                for (r, c) in piece.iter_mut().zip(&tok).take(b).skip(a) {
                    if matches!(c, 'o' | 'O') {
                        r.set(zero);
                    }
                }
            }
            continue;
        }
        if let Some((a, b)) = castle_core(&tok) {
            for (r, c) in piece.iter_mut().zip(&tok).take(b).skip(a) {
                if matches!(c, 'o' | 'O' | '0') {
                    r.set(zero);
                } else if let Some(d) = dash {
                    r.set(d);
                }
            }
        }
    }
}

/// Runs of digits and ovals with at least one of each and no letter on either side.
fn number_spans(t: &[char]) -> Vec<(usize, usize)> {
    let part = |c: &char| c.is_ascii_digit() || *c == 'o' || *c == 'O';
    let mut out = Vec::new();
    let mut i = 0;
    while i < t.len() {
        if !t.get(i).is_some_and(part) {
            i += 1;
            continue;
        }
        let mut j = i;
        while t.get(j).is_some_and(part) {
            j += 1;
        }
        let before = i > 0 && t.get(i - 1).is_some_and(|c| c.is_alphabetic());
        let after = t.get(j).is_some_and(|c| c.is_alphabetic());
        let seg = t.get(i..j).unwrap_or(&[]);
        if !before && !after && seg.iter().any(char::is_ascii_digit) && seg.iter().any(|&c| c == 'o' || c == 'O') {
            out.push((i, j));
        }
        i = j;
    }
    out
}

/// The castling core of a token (without surrounding punctuation): oval, separator, oval, and
/// optionally separator, oval.
fn castle_core(t: &[char]) -> Option<(usize, usize)> {
    let punct = ".,;:!?()[]";
    let (mut a, mut b) = (0, t.len());
    while a < b && t.get(a).is_some_and(|c| punct.contains(*c)) {
        a += 1;
    }
    while b > a && t.get(b - 1).is_some_and(|c| punct.contains(*c)) {
        b -= 1;
    }
    let core = t.get(a..b)?;
    let oval = |c: &char| matches!(c, 'O' | 'o' | '0');
    let sep = |c: &char| matches!(c, '.' | '-' | '–' | '—' | '_');
    let ok = match core {
        [o1, s1, o2] => oval(o1) && sep(s1) && oval(o2),
        [o1, s1, o2, s2, o3] => oval(o1) && sep(s1) && oval(o2) && sep(s2) && oval(o3),
        _ => false,
    };
    ok.then_some((a, b))
}

/// The trunk's `juntar_numero_de_lance`, as the spike simplified it: on a line that carries
/// notation, a lone digit followed by a word that starts with digits (at most three in all) and
/// goes on with nothing or dots is one move number the segmentation split (`1 9...` → `19...`).
/// The word after the digit may start with an oval instead, a `0` read as a letter once it stood
/// apart (`1 O...♕e7` → `10...♕e7`, where a scan cut the `0` and a book's wide figures set it
/// off): joined to the digit, it is the digit it is.
pub(crate) fn join_move_numbers(words: &mut Vec<GlyphWord>) {
    let line: String = words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
    if !line.chars().any(is_figurine) && !has_move(&fold(&line)) {
        return;
    }
    let mut i = 0;
    while i + 1 < words.len() {
        let (Some(a), Some(b)) = (words.get(i), words.get(i + 1)) else { break };
        let lead = b.text.chars().take_while(|c| c.is_ascii_digit() || *c == 'O' || *c == 'o').count();
        // `lead` ASCII digits and ovals are `lead` bytes, so this is a char boundary.
        let rest = b.text.get(lead..).unwrap_or("").to_owned();
        let ok = a.text.chars().count() == 1
            && a.text.chars().all(|c| c.is_ascii_digit())
            && lead > 0
            && lead < MAX_MOVE_NUMBER_DIGITS
            && (rest.is_empty() || rest.starts_with('.') || rest.starts_with('…'));
        if ok {
            let mut b = words.remove(i + 1);
            let zero = |t: &str| t.chars().map(|c| if c == 'O' || c == 'o' { '0' } else { c }).collect::<String>();
            b.text = format!("{}{rest}", zero(b.text.get(..lead).unwrap_or("")));
            for g in b.glyphs.iter_mut().take(lead) {
                g.text = zero(&g.text);
            }
            if let Some(a) = words.get_mut(i) {
                a.text.push_str(&b.text);
                a.rect = union_rect(a.rect, b.rect);
                a.glyphs.extend(b.glyphs);
            }
        } else {
            i += 1;
        }
    }
}
