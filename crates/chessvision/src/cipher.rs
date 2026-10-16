//! The figurine cipher of one book, and legality repair applied to recognized lines (blueprint
//! S13c).
//!
//! `pdfcraft_chess::repair` proves a cipher and repairs moves on one block of tokens. This module
//! keeps what a book proved across its pages ([`BookCipher`], serializable so the engine can keep
//! it in the document's preferences) and applies proofs and repairs to the glyph reader's fused
//! lines ([`repair_lines`]), keeping every box. It works on [`FusedLine`]s, before
//! `fuse::to_chess_text`, because `ChessPageText` joins prose words into one run and a
//! ciphered move (`Wd6+`) is prose until it is decoded.
//!
//! A book's table settles a symbol only on enough evidence and no contradiction
//! (`book_cipher.py`, rules in [`CipherEntry::settled`]). It is applied without a position only
//! to a region whose piece moves mostly carry its symbols (`validate.py:288-296`), so correct
//! notation stays byte for byte. With a position, every block is proved again, and the repair
//! judges each token by legality.

use std::collections::BTreeMap;

use pdfcraft_chess::placement::PieceKind;
use pdfcraft_chess::repair::{self, Alternative, CipherMap, GameContext, MAX_SYMBOL_CHARS, Outcome, Proof, ReadToken, RepairKind, Shape, Slot};
use pdfcraft_chess::san::{Lang, San};
use serde::{Deserialize, Serialize};

use crate::coords::PixelRect;
use crate::glyphs::fuse::{FusedLine, FusedWord};
use crate::glyphs::{Glyph, union_rect};

#[cfg(test)]
mod tests;

/// Legality proofs that settle a symbol, with no contradiction: "five independent positions, no
/// contradiction" (`book_cipher.py:73-74`, `DEFAULT_MIN_SUPPORT`).
pub const MIN_LEGAL_SUPPORT: u32 = 5;
/// Observations of any kind that settle a symbol within the glyph reader's error rate
/// (`book_cipher.py:75-80`, `DEFAULT_MIN_VISUAL_SUPPORT`).
pub const MIN_VISUAL_SUPPORT: u32 = 10;
/// Contradictions such a row may carry, as a share of its support (`book_cipher.py:81-83`).
pub const VISUAL_CONTRADICTION_SHARE: f64 = 0.02;
/// Examples kept per symbol for the reviewer (`book_cipher.py:84-85`, `_MAX_EXAMPLES`).
pub const MAX_EXAMPLES: usize = 6;
/// Most symbols a book's table keeps.
pub const MAX_ENTRIES: usize = 64;
/// Longest stored example text, in characters.
const MAX_EXAMPLE_CHARS: usize = 64;
/// Largest table read back, in bytes.
pub const MAX_JSON_BYTES: usize = 1 << 20;
/// Ciphered moves a region needs before a book's table rewrites it without a position
/// (`validate.py:294-296`: two, and a majority of its piece moves).
pub const MIN_REGION_CIPHERED: usize = 2;

/// A piece a symbol can stand for, as stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Officer {
    K,
    Q,
    R,
    B,
    N,
}

impl Officer {
    pub fn from_kind(kind: PieceKind) -> Option<Officer> {
        Some(match kind {
            PieceKind::King => Officer::K,
            PieceKind::Queen => Officer::Q,
            PieceKind::Rook => Officer::R,
            PieceKind::Bishop => Officer::B,
            PieceKind::Knight => Officer::N,
            PieceKind::Pawn => return None,
        })
    }

    pub fn kind(self) -> PieceKind {
        match self {
            Officer::K => PieceKind::King,
            Officer::Q => PieceKind::Queen,
            Officer::R => PieceKind::Rook,
            Officer::B => PieceKind::Bishop,
            Officer::N => PieceKind::Knight,
        }
    }
}

/// Where an observation came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// A replay from a known position read the symbol as the piece ([`Proof::Proven`]).
    Legality,
    /// The glyph reader swapped the look-alike for a figurine.
    Glyph,
}

/// Observations of one symbol as one piece, by source.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Counts {
    pub legality: u32,
    pub glyph: u32,
}

impl Counts {
    pub fn total(&self) -> u32 {
        self.legality.saturating_add(self.glyph)
    }
}

/// A move that proved a symbol, for the reviewer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Example {
    pub page: u32,
    /// The token as read.
    pub raw: String,
    /// The legal move it was, in English SAN.
    pub san: String,
}

/// What the book's evidence says about one symbol.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CipherEntry {
    pub evidence: BTreeMap<Officer, Counts>,
    pub examples: Vec<Example>,
}

impl CipherEntry {
    /// The majority piece, its support and the observations against it.
    pub fn majority(&self) -> Option<(Officer, u32, u32)> {
        let (piece, counts) = self.evidence.iter().max_by_key(|(p, c)| (c.total(), **p))?;
        let all = self.evidence.values().fold(0u32, |n, c| n.saturating_add(c.total()));
        let support = counts.total();
        Some((*piece, support, all.saturating_sub(support)))
    }

    /// The piece the evidence settles (`book_cipher.py:118-145`): the majority piece with
    /// [`MIN_LEGAL_SUPPORT`] legality proofs and no contradiction, or with
    /// [`MIN_VISUAL_SUPPORT`] observations and contradictions within
    /// [`VISUAL_CONTRADICTION_SHARE`] of them.
    pub fn settled(&self) -> Option<Officer> {
        let (piece, support, against) = self.majority()?;
        let legal = self.evidence.get(&piece).map_or(0, |c| c.legality);
        let legal_rule = against == 0 && legal >= MIN_LEGAL_SUPPORT;
        let visual_rule = support >= MIN_VISUAL_SUPPORT && f64::from(against) <= f64::from(support) * VISUAL_CONTRADICTION_SHARE;
        (support > 0 && (legal_rule || visual_rule)).then_some(piece)
    }
}

/// Why a table could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum CipherError {
    #[error("the cipher table is {0} bytes, more than the {MAX_JSON_BYTES} read")]
    TooLarge(usize),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

/// The symbol → piece table of one book, with the evidence for each row.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BookCipher {
    entries: BTreeMap<String, CipherEntry>,
}

/// A symbol a table can hold: like the ones `repair::infer` reports.
fn valid_symbol(symbol: &str) -> bool {
    let n = symbol.chars().count();
    (1..=MAX_SYMBOL_CHARS).contains(&n) && !symbol.chars().any(char::is_whitespace)
}

fn clip(text: &str) -> String {
    text.chars().take(MAX_EXAMPLE_CHARS).collect()
}

impl BookCipher {
    pub fn entries(&self) -> &BTreeMap<String, CipherEntry> {
        &self.entries
    }

    /// One observation: `symbol` stood for `piece`. Ignored for a symbol a table cannot hold, a
    /// pawn, or a new symbol once the table has [`MAX_ENTRIES`].
    pub fn observe(&mut self, symbol: &str, piece: PieceKind, source: Source, example: Option<Example>) {
        let Some(officer) = Officer::from_kind(piece) else { return };
        if !valid_symbol(symbol) || (self.entries.len() >= MAX_ENTRIES && !self.entries.contains_key(symbol)) {
            return;
        }
        let entry = self.entries.entry(symbol.to_owned()).or_default();
        let counts = entry.evidence.entry(officer).or_default();
        match source {
            Source::Legality => counts.legality = counts.legality.saturating_add(1),
            Source::Glyph => counts.glyph = counts.glyph.saturating_add(1),
        }
        if let Some(e) = example
            && entry.examples.len() < MAX_EXAMPLES
            && entry.majority().is_some_and(|(p, _, _)| p == officer)
        {
            entry.examples.push(Example { page: e.page, raw: clip(&e.raw), san: clip(&e.san) });
        }
    }

    /// Every observation of a proof, as legality evidence from `page`. Anything but
    /// [`Proof::Proven`] adds nothing.
    pub fn record(&mut self, proof: &Proof, page: u32) {
        if let Proof::Proven { observations, .. } = proof {
            for o in observations {
                self.observe(&o.symbol, o.piece, Source::Legality, Some(Example { page, raw: o.raw.clone(), san: o.san.clone() }));
            }
        }
    }

    /// Symbol → piece for the rows the evidence settled.
    pub fn proven(&self) -> CipherMap {
        self.entries.iter().filter_map(|(s, e)| e.settled().map(|p| (s.clone(), p.kind()))).collect()
    }

    /// Read a table back (from the document's preferences), dropping what a table cannot hold.
    pub fn from_json(text: &str) -> Result<BookCipher, CipherError> {
        if text.len() > MAX_JSON_BYTES {
            return Err(CipherError::TooLarge(text.len()));
        }
        let read: BookCipher = serde_json::from_str(text)?;
        let mut entries = BTreeMap::new();
        for (symbol, mut entry) in read.entries.into_iter().filter(|(s, _)| valid_symbol(s)).take(MAX_ENTRIES) {
            entry.examples.truncate(MAX_EXAMPLES);
            for e in &mut entry.examples {
                e.raw = clip(&e.raw);
                e.san = clip(&e.san);
            }
            entries.insert(symbol, entry);
        }
        Ok(BookCipher { entries })
    }

    pub fn to_json(&self) -> Result<String, CipherError> {
        Ok(serde_json::to_string(self)?)
    }
}

/// Where the replay's position came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextUsed {
    /// None: nothing was judged by legality.
    None,
    /// The caller's (a diagram's FEN before the moves).
    Given,
    /// The initial position, because the moves start at `1.`.
    GameStart,
}

/// A word the cipher or the repair rewrote.
#[derive(Clone, Debug, PartialEq)]
pub struct WordEdit {
    pub line: usize,
    pub word: usize,
    /// The word's box, unchanged.
    pub rect: PixelRect,
    pub from: String,
    pub to: String,
    pub kind: RepairKind,
    pub why: String,
    pub confidence: f32,
    /// The legal move, when a replay judged it (`None` for a book's table applied without a
    /// position).
    pub san: Option<San>,
}

/// A word the replay could not settle, left as read for the reviewer.
#[derive(Clone, Debug, PartialEq)]
pub struct WordFlag {
    pub line: usize,
    pub word: usize,
    pub rect: PixelRect,
    pub text: String,
    /// Several legal moves were equally close (`true`), or none was close enough (`false`).
    pub ambiguous: bool,
    /// The legal moves in question, or the nearest ones.
    pub candidates: Vec<San>,
}

/// Lines after the cipher and the repair, with every change recorded.
#[derive(Clone, Debug, PartialEq)]
pub struct LinesRepair {
    pub lines: Vec<FusedLine>,
    pub edits: Vec<WordEdit>,
    pub flags: Vec<WordFlag>,
    /// The replay's verdict on this block's cipher, for [`BookCipher::record`]; `None` without
    /// a position.
    pub proof: Option<Proof>,
    pub context: ContextUsed,
}

/// The word as a token: its glyphs with their alternatives when they spell the word, else its
/// characters.
fn token_of(word: &FusedWord) -> (ReadToken, bool) {
    let glyph_backed = !word.glyphs.is_empty() && word.glyphs.iter().map(|g| g.text.as_str()).collect::<String>() == word.text;
    let mut token = if glyph_backed {
        let slots = word
            .glyphs
            .iter()
            .map(|g| Slot {
                text: g.text.clone(),
                alternatives: g.alternatives.iter().map(|a| Alternative { text: a.text.clone(), p: a.p }).collect(),
            })
            .collect();
        ReadToken { slots, other: None }
    } else {
        ReadToken::plain(&word.text)
    };
    token.other.clone_from(&word.other);
    (token, glyph_backed)
}

/// A move-shaped word, in reading order.
struct Candidate {
    line: usize,
    word: usize,
    token: ReadToken,
    glyph_backed: bool,
    shape: Shape,
}

/// The words that are moves of the main line: shaped like a move, outside parentheses (a
/// variation starts from another position), and, for a bare pawn push, next to other notation.
fn candidates(lines: &[FusedLine]) -> Vec<Candidate> {
    let mut all: Vec<(usize, usize, ReadToken, bool, Option<Shape>)> = Vec::new();
    let mut depth = 0usize;
    for (li, line) in lines.iter().enumerate() {
        for (wi, word) in line.words.iter().enumerate() {
            let opens = word.text.chars().filter(|c| *c == '(').count();
            let closes = word.text.chars().filter(|c| *c == ')').count();
            let inside = depth > 0 || opens > 0;
            depth = depth.saturating_add(opens).saturating_sub(closes);
            let (token, glyph_backed) = token_of(word);
            let shape = if inside { None } else { repair::shape(&token) };
            all.push((li, wi, token, glyph_backed, shape));
        }
    }
    let strong: Vec<bool> = all.iter().map(|w| w.4.as_ref().is_some_and(Shape::is_strong)).collect();
    let mut out = Vec::new();
    for (i, (line, word, token, glyph_backed, shape)) in all.into_iter().enumerate() {
        let Some(shape) = shape else { continue };
        let neighbour = i.checked_sub(1).and_then(|p| strong.get(p)).copied().unwrap_or(false) || strong.get(i + 1).copied().unwrap_or(false);
        if (shape.is_strong() || neighbour) && out.len() < repair::MAX_TOKENS {
            out.push(Candidate { line, word, token, glyph_backed, shape });
        }
    }
    out
}

/// The piece symbol of a candidate (empty for a pawn move or castling).
fn piece_symbol(c: &Candidate) -> String {
    c.token.slots.iter().skip(c.shape.core.start).take(c.shape.prefix).map(|s| s.text.as_str()).collect()
}

/// Rewrite a word with new slot texts (a merged slot's box joins its neighbour's), or with a new
/// text when the slots no longer line up.
fn rewrite(word: &mut FusedWord, glyph_backed: bool, slots: Option<&[String]>, to: &str) {
    match slots {
        Some(new) if glyph_backed && new.len() == word.glyphs.len() => {
            let mut kept: Vec<Glyph> = Vec::with_capacity(word.glyphs.len());
            for (mut g, text) in std::mem::take(&mut word.glyphs).into_iter().zip(new) {
                if text.is_empty() {
                    if let Some(last) = kept.last_mut() {
                        last.rect = union_rect(last.rect, g.rect);
                    }
                } else {
                    text.clone_into(&mut g.text);
                    kept.push(g);
                }
            }
            word.glyphs = kept;
        }
        // The glyphs would no longer spell the word: its box alone stays.
        _ => word.glyphs.clear(),
    }
    to.clone_into(&mut word.text);
}

fn rect_of(lines: &[FusedLine], c: &Candidate) -> PixelRect {
    lines.get(c.line).and_then(|l| l.words.get(c.word)).map(|w| w.rect).unwrap_or_default()
}

/// Apply `cipher` (and a repair, given a position) to `lines`, the page's recognized lines in
/// reading order, read in `lang`. `book` is the book's proven table ([`BookCipher::proven`]).
///
/// With `context` (or, without it, when the first move is numbered `1.`, the initial position),
/// the block's cipher is proved again ([`repair::prove`]) and every move token is judged by
/// legality ([`repair::repair`]): repairs are applied and recorded, ambiguous and unreadable
/// tokens are flagged and left alone. Without a position nothing is repaired; only `book` is
/// applied, and only to a region whose piece moves mostly carry its symbols. Tokens after the
/// replay stopped are treated the same way.
pub fn repair_lines(lines: &[FusedLine], context: Option<&GameContext>, lang: Lang, book: &CipherMap) -> LinesRepair {
    let cands = candidates(lines);
    let mut out = LinesRepair { lines: lines.to_vec(), edits: Vec::new(), flags: Vec::new(), proof: None, context: ContextUsed::None };
    let starts_game = cands.first().is_some_and(|c| c.shape.number == Some((1, false)));
    let (ctx, used) = match context {
        Some(ctx) => (Some(ctx.clone()), ContextUsed::Given),
        None if starts_game => (Some(GameContext::game_start()), ContextUsed::GameStart),
        None => (None, ContextUsed::None),
    };
    out.context = used;
    let mut cipher = book.clone();
    let mut pending: Vec<&Candidate> = Vec::new();
    if let Some(ctx) = &ctx {
        let tokens: Vec<ReadToken> = cands.iter().map(|c| c.token.clone()).collect();
        let proof = repair::prove(ctx, &tokens, lang, &repair::infer(&tokens));
        if let Proof::Proven { assignment, .. } = &proof {
            cipher.extend(assignment.iter().map(|(s, p)| (s.clone(), *p)));
        }
        out.proof = Some(proof);
        let replay = repair::repair(ctx, &tokens, lang, &cipher);
        for (c, outcome) in cands.iter().zip(replay.outcomes) {
            let flag = |ambiguous: bool, candidates: Vec<San>| WordFlag {
                line: c.line,
                word: c.word,
                rect: rect_of(lines, c),
                text: c.token.text(),
                ambiguous,
                candidates,
            };
            match outcome {
                Outcome::Kept(_) => {}
                Outcome::Repaired(r) => {
                    if let Some(word) = out.lines.get_mut(c.line).and_then(|l| l.words.get_mut(c.word)) {
                        rewrite(word, c.glyph_backed, r.slots.as_deref(), &r.to);
                    }
                    let (line, word, rect) = (c.line, c.word, rect_of(lines, c));
                    out.edits.push(WordEdit {
                        line,
                        word,
                        rect,
                        from: r.from,
                        to: r.to,
                        kind: r.kind,
                        why: r.why,
                        confidence: r.confidence,
                        san: Some(r.san),
                    });
                }
                Outcome::Ambiguous { candidates } => out.flags.push(flag(true, candidates)),
                Outcome::Unresolved { near } => out.flags.push(flag(false, near)),
                Outcome::NotReached => pending.push(c),
            }
        }
    } else {
        pending = cands.iter().collect();
    }
    // The book's table without a position: only where most piece moves carry its symbols.
    let judged = cands.iter().filter(|c| c.shape.prefix > 0).count();
    let ciphered = cands.iter().filter(|c| c.shape.prefix > 0 && cipher.contains_key(&piece_symbol(c))).count();
    if ciphered < MIN_REGION_CIPHERED || ciphered.saturating_mul(2) <= judged {
        return out;
    }
    for c in pending {
        let Some(slots) = repair::decipher(&c.token, &cipher) else { continue };
        let to: String = slots.concat();
        if let Some(word) = out.lines.get_mut(c.line).and_then(|l| l.words.get_mut(c.word)) {
            rewrite(word, c.glyph_backed, Some(&slots), &to);
        }
        out.edits.push(WordEdit {
            line: c.line,
            word: c.word,
            rect: rect_of(lines, c),
            from: c.token.text(),
            to,
            kind: RepairKind::Cipher,
            why: "decoded by the book's proven table (no position to replay)".to_owned(),
            confidence: repair::CONFIDENCE_CIPHER,
            san: None,
        });
    }
    out
}
