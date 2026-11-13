//! Pages of move text for the fuzzer (plan A9-Q3a): the capture seed and a page of PGN as lines
//! of runs, each run a font and its text, and a mutator that edits them word by word from a
//! chess vocabulary and writes the page again. Mutating a PDF's bytes almost never makes new move
//! text, so the walker, the capture and the PGN readers saw the same few lines every run (A7-08).

use super::Rng;

/// A run of text in one font: 1 bold (a main line), 2 roman (analysis, prose), 3 a figurine font
/// that draws K Q R B N as pieces (SemFig), 4 a private-use figurine font (FigurineCB).
pub type Run = (u8, String);
pub type Line = Vec<Run>;

/// The words the mutator puts in: moves and move numbers glued or not, results, PGN's brackets,
/// comments and escapes, glyphs, the walker's separators and labels, the prose it decides on
/// (captions, exercises, stipulations, squares and pieces in sentences), and PGN tags and
/// commands. Non-ASCII words are written in WinAnsi where it has them (`…`, `½`, `±`).
const VOCAB: &[&str] = &[
    "1.",
    "2...",
    "12.",
    "12...",
    "18.",
    "e4",
    "e5",
    "d4",
    "d5",
    "c4",
    "Nf3",
    "Nc6",
    "Bb5",
    "a6",
    "Bxc6",
    "dxc6",
    "O-O",
    "0-0",
    "O-O-O",
    "0-0-0",
    "Qxe7+",
    "Rxd3#",
    "e8=Q",
    "e8Q",
    "exd6",
    "e.p.",
    "--",
    "Kh7",
    "Raxe1",
    "12.e4",
    "12...Nf6",
    "1-0",
    "0-1",
    "1/2-1/2",
    "*",
    "1:0",
    "½-½",
    "(",
    ")",
    "[",
    "]",
    "{",
    "}",
    ";",
    "%",
    "((",
    "))",
    "$14",
    "$1",
    "!",
    "?",
    "!?",
    "?!",
    "!!",
    "+-",
    "-+",
    "=",
    "+=",
    "±",
    "…",
    "...",
    "a)",
    "b)",
    "A)",
    "(B2)",
    "B2.1)",
    "1)",
    "-",
    "===",
    "Position after",
    "Exercise 4",
    "White to move",
    "Black plays",
    "Mate in 2",
    "#2",
    "h#2",
    "on h5",
    "to d3",
    "the knight",
    "does not come to",
    "and",
    "with",
    "after",
    "Or",
    "[Event \"A\"]",
    "[Site \"?\"]",
    "[FEN \"8/8/8/8/8/8/8/K6k w - - 0 1\"]",
    "[SetUp \"1\"]",
    "[%clk 0:10]",
    "[%cal Ge2e4]",
];

/// Most lines on a page: the page holds 33 at 14 pt.
const MAX_LINES: usize = 32;

/// Most words in a run.
const MAX_WORDS: usize = 64;

/// The capture seed (PGN toolbar, T16): the main line in a bold face, analysis in a regular
/// one with brackets, `;` and `a)` separators, glyphs as signs and as `$14`, a null move, an
/// unbalanced bracket and results, and figurines in a figurine font (SemFig draws its K Q R B N as
/// pieces) and in a private-use one (FigurineCB). `check-one --chess` plans each page's text as a
/// selection in every capture mode and applies it.
pub fn capture() -> Vec<Line> {
    let run = |font: u8, text: &str| (font, text.to_owned());
    vec![
        vec![run(1, "1.e4 e5 2.Nf3 "), run(3, "N"), run(1, "c6 3."), run(3, "B"), run(1, "b5 a6")],
        vec![run(2, "Or 2...d6 3.d4 !? (3."), run(4, "Q"), run(2, "c4 $14; a) 3...Nf6 b) 3...exd4 +=) 3...Nd7 ~ 1:0")],
        vec![run(1, "18.Nd5! Qxe1 19.Nxe7+ Kh7 20.Raxe1")],
        vec![run(2, "After 18...Qd8 19.Bxf6! Bxf6 20.fxg6 (less good is 20.Nxf6+ exf6 21.Bxc4 Rxc4) 20...fxg6")],
        vec![run(2, "21.Qg3 +- 21...Kh7 22.e5!! -- 22...Rf7 (() 0-0-0 23.Qh4# 1-0 (1/2)")],
    ]
}

/// A page of PGN, three games, a brace in a `;` comment and in a `%` line: before plan A9-F2,
/// `split_pgn` merged every game after either (A1-05), and `check-one --chess` says so.
pub fn pgn() -> Vec<Line> {
    [
        "[Event \"A\"]",
        "[White \"x\"]",
        "1. e4 e5 ; White plays {the king's pawn",
        "2. Nf3 *",
        "[Event \"B\"]",
        "% exported {by a tool",
        "1. d4 d5 {a comment [%clk 0:10] over} 2. c4 *",
        "[Event \"C\"]",
        "1. c4 (1. Nf3) 1... e5 1/2-1/2",
    ]
    .iter()
    .map(|l| vec![(2, (*l).to_owned())])
    .collect()
}

/// A page of 2 to 10 lines of words from the vocabulary.
pub fn random_page(rng: &mut Rng) -> Vec<Line> {
    (0..2 + rng.below(9)).map(|_| random_line(rng)).collect()
}

fn random_line(rng: &mut Rng) -> Line {
    let mut line = Vec::new();
    for _ in 0..1 + rng.below(4) {
        if rng.below(5) == 0 {
            line.push((3 + rng.below(2) as u8, ["K", "Q", "R", "B", "N"][rng.below(5)].to_owned()));
        }
        let words: Vec<&str> = (0..1 + rng.below(8)).map(|_| word(rng)).collect();
        line.push((1 + rng.below(2) as u8, words.join(" ")));
    }
    line
}

fn word(rng: &mut Rng) -> &'static str {
    VOCAB[rng.below(VOCAB.len())]
}

/// `page` with 1 to 6 edits: a word replaced, put in, taken out or glued to the next; a line
/// put in, doubled, taken out, split or joined to the next; a run's face changed or a figurine
/// put before it.
pub fn mutate(rng: &mut Rng, mut page: Vec<Line>) -> Vec<Line> {
    for _ in 0..1 + rng.below(6) {
        if page.is_empty() {
            page.push(random_line(rng));
        }
        let at = rng.below(page.len());
        match rng.below(10) {
            0..=3 => {
                let Some(line) = page.get_mut(at) else { continue };
                let r = rng.below(line.len().max(1));
                let Some((_, text)) = line.get_mut(r) else { continue };
                let mut words: Vec<String> = text.split(' ').map(str::to_owned).collect();
                let w = rng.below(words.len().max(1));
                match rng.below(4) {
                    0 if w < words.len() => words[w] = word(rng).to_owned(),
                    1 if words.len() < MAX_WORDS => words.insert(w.min(words.len()), word(rng).to_owned()),
                    2 if w < words.len() => {
                        words.remove(w);
                    }
                    _ if w + 1 < words.len() => {
                        let next = words.remove(w + 1);
                        words[w].push_str(&next);
                    }
                    _ => {}
                }
                *text = words.join(" ");
            }
            4 if page.len() < MAX_LINES => {
                let line = random_line(rng);
                page.insert(at, line);
            }
            5 if page.len() < MAX_LINES => {
                let line = page[at].clone();
                page.insert(at, line);
            }
            6 => {
                page.remove(at);
            }
            7 if page.len() < MAX_LINES => {
                let line = &mut page[at];
                let r = rng.below(line.len().max(1));
                let rest = line.split_off(r.min(line.len()));
                if !rest.is_empty() {
                    page.insert(at + 1, rest);
                }
            }
            8 if at + 1 < page.len() => {
                let next = page.remove(at + 1);
                page[at].extend(next);
            }
            _ => {
                let line = &mut page[at];
                let r = rng.below(line.len().max(1));
                if rng.below(2) == 0 {
                    if let Some(run) = line.get_mut(r) {
                        run.0 = 1 + rng.below(2) as u8;
                    }
                } else {
                    line.insert(r.min(line.len()), (3 + rng.below(2) as u8, ["K", "Q", "R", "B", "N"][rng.below(5)].to_owned()));
                }
            }
        }
    }
    page.truncate(MAX_LINES);
    page
}

/// `page` as a PDF: one page, a line every 14 pt from the top, the four fonts.
pub fn pdf(page: &[Line]) -> Vec<u8> {
    let mut content = String::new();
    for (i, line) in page.iter().take(MAX_LINES).enumerate() {
        content.push_str("BT ");
        for (j, (font, text)) in line.iter().enumerate() {
            content.push_str(&format!("/F{font} 10 Tf "));
            if j == 0 {
                content.push_str(&format!("20 {} Td ", 470 - 14 * i));
            }
            content.push('(');
            content.push_str(&pdf_string(text));
            content.push_str(") Tj ");
        }
        content.push_str("ET\n");
    }
    let stream = format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len());
    super::pdf(
        &[
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 400 500] >>",
            "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /Font << /F1 5 0 R /F2 6 0 R /F3 7 0 R /F4 8 0 R >> >> >>",
            &stream,
            "<< /Type /Font /Subtype /Type1 /BaseFont /Times-Bold /Encoding /WinAnsiEncoding >>",
            "<< /Type /Font /Subtype /Type1 /BaseFont /Times-Roman /Encoding /WinAnsiEncoding >>",
            "<< /Type /Font /Subtype /Type1 /BaseFont /SemFig >>",
            "<< /Type /Font /Subtype /Type1 /BaseFont /FigurineCB-Time >>",
        ],
        "/Root 1 0 R",
    )
}

/// `text` as the inside of a PDF literal string in WinAnsi: brackets and backslashes escaped, the
/// characters WinAnsi has beyond ASCII as octal escapes, and the others left out.
fn pdf_string(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        match c {
            '(' | ')' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            ' '..='~' => out.push(c),
            _ => {
                let code: Option<u8> = match c {
                    '…' => Some(0x85),
                    '–' => Some(0x96),
                    '—' => Some(0x97),
                    '±' => Some(0xb1),
                    '½' => Some(0xbd),
                    '×' => Some(0xd7),
                    _ => None,
                };
                if let Some(code) = code {
                    out.push_str(&format!("\\{code:03o}"));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_of_move_text_are_written_and_mutated() {
        let bytes = pdf(&pgn());
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("(1. e4 e5 ; White plays {the king's pawn) Tj"), "{text}");
        assert!(pdf_string("a) (b\\ …½∞") == "a\\) \\(b\\\\ \\205\\275", "{}", pdf_string("a) (b\\ …½∞"));
        let mut rng = Rng(7);
        for _ in 0..500 {
            let page = mutate(&mut rng, capture());
            assert!(!page.is_empty() && page.len() <= MAX_LINES);
            let page = random_page(&mut rng);
            let page = mutate(&mut rng, page);
            assert!(pdf(&page).starts_with(b"%PDF-1.7"));
        }
    }
}
