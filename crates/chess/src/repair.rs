//! Misread moves repaired by legality, and the figurine cipher of a book proved by replay
//! (blueprint S13c).
//!
//! An OCR engine that meets a figurine guesses the nearest Latin shape, and it guesses
//! **consistently**: on one book `♕` comes back as `W` every time. That is a substitution cipher,
//! and a cipher can be inverted, but only on evidence. Two kinds are used here, and both need a
//! position to start from (a diagram's FEN before the moves, or the initial position after `1.`):
//!
//! - **The cipher** ([`infer`], [`prove`]). [`infer`] lists the symbols that stand where a piece
//!   letter belongs, ranked by support. [`prove`] replays the moves from the position under every
//!   assignment of the most frequent symbols to pieces, and accepts one only when it replays
//!   **strictly further** than reading the text as written, and every assignment that replays as
//!   far reads the same moves. Correct notation can never be "proved" a cipher: its as-written
//!   reading already replays as far as any decoding of it.
//! - **The repair** ([`repair`]). Each token is tried as written (decoded by the cipher first, when
//!   it has a symbol), then with one substitution among its alternatives (the glyph reader's top
//!   readings, the other reader's word, or the OCR confusion table for plain text), then with two.
//!   The first tier that reaches a legal move decides: one legal move is a repair, two or more is
//!   an ambiguity that is **flagged and not applied**. Every repair says what it changed, why, and
//!   how sure it is, for the review queue (S17). A token nothing repairs stops the replay, because
//!   every later move would be judged in the wrong position.
//!
//! [`repair`] takes a [`GameContext`]: without a position there is nothing to judge legality
//! against, so there is no repair.
//!
//! Ported from Caïssa (relicensed by its author, `docs/chess/provenance.md`), re-derived on
//! `cozy-chess` where the Python used python-chess: `ocr/notation/cipher.py` (symbols, the
//! ciphered-page guard), `ocr/notation/validate.py` (`_best_assignment`) and
//! `notation/legality_repair.py` (tiers, confusion table, confidences).

use std::collections::BTreeMap;
use std::ops::Range;

use cozy_chess::{Board, Move};

use crate::fen::Position;
use crate::placement::{Color, PieceKind};
use crate::san::{FigurineSet, Lang, MAX_SAN_LEN, San, SanBody, SanError, figurine, figurine_piece};

#[cfg(test)]
mod tests;

/// Most tokens one replay reads.
pub const MAX_TOKENS: usize = 512;
/// Most slots a token may have to be read as a move (`MAX_WORD_CHARS` of `chess_text`).
pub const MAX_TOKEN_SLOTS: usize = 64;
/// Most alternatives tried per slot (the glyph reader keeps three).
pub const MAX_ALTERNATIVES: usize = 3;
/// Most substitutions combined in one token: "two is the measured sweet spot"
/// (`legality_repair.py:552-556`, `max_substitutions = 2`).
pub const MAX_SUBSTITUTIONS: usize = 2;
/// Most variants of one token tried in one tier.
pub const MAX_VARIANTS: usize = 256;
/// Longest cipher symbol: "the look-alike clusters of a damaged text layer run to five
/// characters" (`cipher.py:140-142`, `_MAX_PREFIX = 5`).
pub const MAX_SYMBOL_CHARS: usize = 5;
/// A symbol seen once is a smudge, twice a pattern (`cipher.py:255-262`, `min_support = 2`).
pub const MIN_SYMBOL_SUPPORT: u32 = 2;
/// Piece moves a page needs before it can be called ciphered (`cipher.py:206`).
pub const MIN_JUDGED_MOVES: u32 = 12;
/// Symbols assigned by replay at once: five pieces over three symbols are 60 hypotheses
/// (`validate.py:510-513`, `_MAX_FREE_SYMBOLS = 3`).
pub const MAX_FREE_SYMBOLS: usize = 3;
/// Legal moves offered for an unresolved token (`legality_repair.py:1019`).
pub const MAX_NEAR_MISSES: usize = 3;
/// Distinct symbols counted on one page.
const MAX_SYMBOLS: usize = 256;

/// Confidence of a repair by one confusion swap, and by two (`legality_repair.py:944`).
pub const CONFIDENCE_ONE_SWAP: f32 = 0.92;
pub const CONFIDENCE_TWO_SWAPS: f32 = 0.80;
/// Confidence of a cipher decoding: a piece slot resolved by legality (`legality_repair.py:820`).
pub const CONFIDENCE_CIPHER: f32 = 0.90;
/// Confidence of taking the other reader's word. Not in Caïssa, which had one reader: set between
/// one and two swaps, since the other reader is independent but reads notation worse.
pub const CONFIDENCE_OTHER_READER: f32 = 0.85;

/// Punctuation that opens a word without being part of it.
const LEADING: &[char] = &['(', '[', '{', '"', '\'', '«', '“', '‘'];
/// Capture marks between the from and to squares.
const CAPTURES: &[char] = &['x', ':', '×'];

/// Symbol → piece: what a book's figurine look-alikes stand for.
pub type CipherMap = BTreeMap<String, PieceKind>;

/// Another reading of a slot and its probability.
#[derive(Clone, Debug, PartialEq)]
pub struct Alternative {
    pub text: String,
    pub p: f32,
}

/// One recognized glyph (or one character of plain text): its reading and the others.
#[derive(Clone, Debug, PartialEq)]
pub struct Slot {
    pub text: String,
    /// The reader's other readings, most likely first. Empty for plain text, where the OCR
    /// confusion table supplies them.
    pub alternatives: Vec<Alternative>,
}

/// A recognized word, as slots in reading order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReadToken {
    pub slots: Vec<Slot>,
    /// What another reader read over the same place, if anything.
    pub other: Option<String>,
}

impl ReadToken {
    /// Plain text: one slot per character, no alternatives. Longer than [`MAX_TOKEN_SLOTS`] is
    /// cut there (it is not a move anyway).
    pub fn plain(text: &str) -> ReadToken {
        let slots = text.chars().take(MAX_TOKEN_SLOTS + 1).map(|c| Slot { text: c.to_string(), alternatives: Vec::new() }).collect();
        ReadToken { slots, other: None }
    }

    /// The slots' texts joined.
    pub fn text(&self) -> String {
        self.slots.iter().map(|s| s.text.as_str()).collect()
    }
}

/// Where the move is in a token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shape {
    /// The slots of the move proper: after wrapping punctuation and a move number, before
    /// check marks, annotations and closing punctuation.
    pub core: Range<usize>,
    /// A move number glued in front: `(12, false)` for `12.`, `(12, true)` for `12...`.
    pub number: Option<(u16, bool)>,
    /// How many core slots hold the piece (0 for a pawn move or castling).
    pub prefix: usize,
    /// Whether the last core slot is a promotion piece.
    pub promotion: bool,
    pub castle: bool,
    pub capture: bool,
}

impl Shape {
    /// Notation on its own: a number, a piece, a capture, castling or a promotion. A bare pawn
    /// push (`f3`) is a move only next to other notation.
    pub fn is_strong(&self) -> bool {
        self.number.is_some() || self.prefix > 0 || self.capture || self.castle || self.promotion
    }
}

fn texts(token: &ReadToken) -> Vec<&str> {
    token.slots.iter().map(|s| s.text.as_str()).collect()
}

fn concat(texts: &[impl AsRef<str>]) -> String {
    texts.iter().map(AsRef::as_ref).collect()
}

fn is_dash(c: char) -> bool {
    matches!(c, '-' | '‐' | '‑' | '‒' | '–' | '—' | '―' | '−')
}

/// `O-O`, `0-0-0`, `o–o`, and the misreads the confusion table brings back to them (`0-Q`).
fn is_castling(text: &str) -> bool {
    let parts: Vec<&str> = text.split(is_dash).collect();
    (parts.len() == 2 || parts.len() == 3) && parts.iter().all(|p| matches!(*p, "O" | "0" | "o" | "Q" | "D" | "О" | "о"))
}

/// The move in `token`, or `None` when it is not shaped like one.
pub fn shape(token: &ReadToken) -> Option<Shape> {
    let t = texts(token);
    let bytes = t.iter().fold(0usize, |n, s| n.saturating_add(s.len()));
    if t.len() > MAX_TOKEN_SLOTS || bytes > MAX_TOKEN_SLOTS * 4 {
        return None;
    }
    let lead = t.iter().take_while(|s| !s.is_empty() && s.chars().all(|c| LEADING.contains(&c))).count();
    // A move number: one to three digits, then dots or an ellipsis.
    let mut start = lead;
    let mut number = None;
    let digits: String = t.iter().skip(lead).take_while(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())).copied().collect();
    let digit_slots = t.iter().skip(lead).take_while(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())).count();
    let dots: String = t.iter().skip(lead + digit_slots).take_while(|s| !s.is_empty() && s.chars().all(|c| c == '.' || c == '…')).copied().collect();
    if (1..=3).contains(&digits.len()) && !dots.is_empty() {
        let n: u16 = digits.parse().ok()?;
        let dot_slots = t.iter().skip(lead + digit_slots).take_while(|s| !s.is_empty() && s.chars().all(|c| c == '.' || c == '…')).count();
        number = Some((n, dots.contains('…') || dots.chars().count() >= 2));
        start = lead + digit_slots + dot_slots;
    }
    // Check marks, annotations and punctuation trail the move: nothing of a move is without a
    // letter, a digit or a figurine.
    let mut end = t.len();
    while end > start && t.get(end - 1).is_some_and(|s| !s.chars().any(|c| c.is_alphanumeric() || figurine_piece(c).is_some())) {
        end -= 1;
    }
    let core_slots = t.get(start..end)?;
    let core = concat(core_slots);
    let chars: Vec<char> = core.chars().collect();
    if chars.is_empty() || chars.len() > MAX_SAN_LEN {
        return None;
    }
    if is_castling(&core) {
        return Some(Shape { core: start..end, number, prefix: 0, promotion: false, castle: true, capture: false });
    }
    // From the right: a promotion piece, the target square, a capture mark, a rank and a file.
    let n = chars.len();
    let file = |c: char| ('a'..='h').contains(&c);
    let rank = |c: char| ('1'..='8').contains(&c);
    let promo = n >= 3
        && (chars.get(n - 2) == Some(&'=')
            || chars.get(n - 2).is_some_and(|c| *c == '1' || *c == '8') && !chars.get(n - 1).is_some_and(char::is_ascii_digit));
    let m = if promo { n - if chars.get(n - 2) == Some(&'=') { 2 } else { 1 } } else { n };
    if m < 2 || !chars.get(m - 2).copied().is_some_and(file) || !chars.get(m - 1).copied().is_some_and(rank) {
        return None;
    }
    let mut j = m - 2;
    let capture = j > 0 && chars.get(j - 1).is_some_and(|c| CAPTURES.contains(c));
    if capture {
        j -= 1;
    }
    if j > 0 && chars.get(j - 1).copied().is_some_and(rank) {
        j -= 1;
    }
    if j > 0 && chars.get(j - 1).copied().is_some_and(file) {
        j -= 1;
    }
    if j > MAX_SYMBOL_CHARS {
        return None;
    }
    // The piece prefix must end on a slot boundary.
    let mut seen = 0;
    let mut prefix = None;
    for (i, s) in core_slots.iter().enumerate() {
        if seen == j {
            prefix = Some(i);
            break;
        }
        seen += s.chars().count();
    }
    let prefix = prefix?;
    let promotion = promo && core_slots.last().is_some_and(|s| s.chars().count() == 1) && core_slots.len() > prefix + 2;
    Some(Shape { core: start..end, number, prefix, promotion, castle: false, capture })
}

/// What a piece prefix is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Prefix {
    Pawn,
    /// An English piece letter or a figurine.
    Piece,
    /// One character, or a short cluster with a non-alphanumeric character in it, standing where
    /// a piece belongs: a cipher symbol (`cipher.py:234-236`, `294-300`).
    Symbol,
    /// A run of letters glued to a square: a word, or a glyph that did not survive as one symbol.
    Other,
}

fn classify(prefix: &str) -> Prefix {
    let n = prefix.chars().count();
    if n == 0 {
        Prefix::Pawn
    } else if n == 1 && prefix.chars().all(|c| "KQRBN".contains(c) || figurine_piece(c).is_some()) {
        Prefix::Piece
    } else if n == 1 || (n <= MAX_SYMBOL_CHARS && !prefix.chars().all(char::is_alphanumeric)) {
        Prefix::Symbol
    } else {
        Prefix::Other
    }
}

/// The prefix and promotion symbols of a shaped token.
fn symbols_of<'a>(t: &[&'a str], s: &Shape) -> (String, Option<&'a str>) {
    let prefix = concat(t.get(s.core.start..s.core.start + s.prefix).unwrap_or(&[]));
    let promotion = if s.promotion { s.core.end.checked_sub(1).and_then(|i| t.get(i)).copied() } else { None };
    (prefix, promotion)
}

/// What a page says about its own cipher.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CipherReport {
    /// Symbols seen at least [`MIN_SYMBOL_SUPPORT`] times, most supported first.
    pub symbols: Vec<(String, u32)>,
    /// Piece moves whose piece is an English letter or a figurine.
    pub clean: u32,
    /// Piece moves whose piece is anything else.
    pub ciphered: u32,
    pub pawns: u32,
}

impl CipherReport {
    pub fn judged(&self) -> u32 {
        self.clean.saturating_add(self.ciphered)
    }

    /// Most piece moves carry a symbol, on a page with enough of them to tell
    /// (`cipher.py:202-206`: at least 12 judged and a ciphered majority).
    pub fn is_ciphered(&self) -> bool {
        self.judged() >= MIN_JUDGED_MOVES && u64::from(self.ciphered) * 2 > u64::from(self.judged())
    }
}

/// List the symbols `tokens` use where a piece belongs, with their support.
///
/// Symbols are judged against English letters and figurines, the alphabet a decoding writes:
/// `S` in a German book is a symbol here, and it is [`prove`] that refuses to call it a cipher,
/// because reading it as German already replays as well.
pub fn infer(tokens: &[ReadToken]) -> CipherReport {
    let mut counts: BTreeMap<String, u32> = BTreeMap::new();
    let mut report = CipherReport::default();
    let count = |sym: &str, counts: &mut BTreeMap<String, u32>| {
        if counts.len() < MAX_SYMBOLS || counts.contains_key(sym) {
            let n = counts.entry(sym.to_owned()).or_insert(0);
            *n = n.saturating_add(1);
        }
    };
    for token in tokens.iter().take(MAX_TOKENS) {
        let Some(s) = shape(token) else { continue };
        if s.castle {
            continue;
        }
        let t = texts(token);
        let (prefix, promotion) = symbols_of(&t, &s);
        if let Some(p) = promotion
            && classify(p) == Prefix::Symbol
        {
            count(p, &mut counts);
        }
        match classify(&prefix) {
            Prefix::Pawn => report.pawns = report.pawns.saturating_add(1),
            Prefix::Piece => report.clean = report.clean.saturating_add(1),
            Prefix::Symbol => {
                count(&prefix, &mut counts);
                report.ciphered = report.ciphered.saturating_add(1);
            }
            Prefix::Other => report.ciphered = report.ciphered.saturating_add(1),
        }
    }
    let mut symbols: Vec<(String, u32)> = counts.into_iter().filter(|(_, n)| *n >= MIN_SYMBOL_SUPPORT).collect();
    symbols.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    report.symbols = symbols;
    report
}

/// A symbol and the piece a decoding read it as.
type Use = (String, PieceKind);

/// The token's slot texts with `cipher`'s symbols replaced by figurines, and the symbols used.
/// A cluster symbol's figurine goes in its first slot; the others become empty.
fn decode(token: &ReadToken, s: &Shape, cipher: &CipherMap) -> Option<(Vec<String>, Vec<Use>)> {
    if s.castle || cipher.is_empty() {
        return None;
    }
    let t = texts(token);
    let (prefix, promotion) = symbols_of(&t, s);
    let mut out: Vec<String> = t.iter().map(|x| (*x).to_owned()).collect();
    let mut used = Vec::new();
    if let Some(&piece) = cipher.get(&prefix) {
        for (k, slot) in out.iter_mut().enumerate().skip(s.core.start).take(s.prefix) {
            *slot = if k == s.core.start { figurine(piece, FigurineSet::Solid).to_string() } else { String::new() };
        }
        used.push((prefix, piece));
    }
    if let Some(p) = promotion
        && let Some(&piece) = cipher.get(p)
        && let Some(slot) = s.core.end.checked_sub(1).and_then(|i| out.get_mut(i))
    {
        *slot = figurine(piece, FigurineSet::Solid).to_string();
        used.push((p.to_owned(), piece));
    }
    (!used.is_empty()).then_some((out, used))
}

/// A position moves are replayed from: a diagram's FEN, or the initial position of a game.
#[derive(Clone, Debug)]
pub struct GameContext {
    board: Board,
}

impl GameContext {
    /// The initial position, for moves that start at `1.`.
    pub fn game_start() -> GameContext {
        GameContext { board: Board::default() }
    }

    /// A diagram's position, with `side` to move when the position does not say. `None` when
    /// the position is not legal.
    pub fn from_position(position: &Position, side: Color) -> Option<GameContext> {
        position.to_board(side).ok().map(|board| GameContext { board })
    }

    /// A full FEN. `None` when it is not a legal position.
    pub fn from_fen(fen: &str) -> Option<GameContext> {
        Board::from_fen(fen.trim(), false).ok().map(|board| GameContext { board })
    }

    pub fn fen(&self) -> String {
        format!("{}", self.board)
    }
}

/// What reading one text gave on a board.
enum Reading {
    Legal(Move),
    /// Several legal moves fit (a missing disambiguation or promotion piece).
    Ambiguous(Vec<Move>),
    None,
}

fn legal_moves(board: &Board) -> Vec<Move> {
    let mut v = Vec::new();
    board.generate_moves(|moves| {
        v.extend(moves);
        false
    });
    v
}

/// The legal moves `san` could mean, its disambiguation and promotion allowed to be missing.
fn fitting(board: &Board, san: &San) -> Vec<Move> {
    let SanBody::Move { piece, from_file, from_rank, to, promotion, .. } = san.body else { return Vec::new() };
    let want = piece.map(PieceKind::letter);
    legal_moves(board)
        .into_iter()
        .filter(|&mv| {
            // Cheap checks first: naming a move generates the moves again.
            let moving = board.piece_on(mv.from).map(|p| crate::san::kind_from_cozy(p).letter());
            if mv.to != crate::san::square_to_cozy(to).unwrap_or(mv.from) || moving != Some(want.unwrap_or('P')) {
                return false;
            }
            let Ok(named) = San::from_move(board, mv) else { return false };
            let SanBody::Move { piece: p, to: t, promotion: pr, .. } = named.body else { return false };
            p == piece
                && t == to
                && (promotion.is_none() || pr == promotion)
                && from_file.is_none_or(|f| f == mv.from.file() as u8)
                && from_rank.is_none_or(|r| r == mv.from.rank() as u8)
        })
        .collect()
}

fn read(board: &Board, text: &str, lang: Lang) -> Reading {
    let Ok(san) = San::read(text, lang) else { return Reading::None };
    // A pawn reaching the last rank with no promotion piece is missing one, not illegal.
    let unpromoted = matches!(san.body, SanBody::Move { piece: None, to, promotion: None, .. } if to.rank() == 0 || to.rank() == 7);
    match san.to_move(board) {
        Ok(mv) => Reading::Legal(mv),
        Err(SanError::NoSuchMove(_)) if unpromoted => ambiguous(fitting(board, &san)),
        Err(SanError::Ambiguous(_)) => ambiguous(fitting(board, &san)),
        Err(_) => Reading::None,
    }
}

fn ambiguous(fit: Vec<Move>) -> Reading {
    if fit.len() > 1 { Reading::Ambiguous(fit) } else { Reading::None }
}

fn san_of(board: &Board, mv: Move) -> Option<San> {
    San::from_move(board, mv).ok()
}

/// One replay of the tokens, reading each as written or decoded by `cipher` and nothing else.
struct Strict {
    sans: Vec<String>,
    used: Vec<(Use, String, String)>,
    complete: bool,
}

fn replay_strict(ctx: &GameContext, tokens: &[(&ReadToken, Option<Shape>)], lang: Lang, cipher: &CipherMap) -> Strict {
    let mut board = ctx.board.clone();
    let mut out = Strict { sans: Vec::new(), used: Vec::new(), complete: true };
    for (token, s) in tokens {
        let Some(s) = s else {
            out.complete = false;
            break;
        };
        let decoded = decode(token, s, cipher);
        let core = match &decoded {
            Some((t, _)) => concat(t.get(s.core.clone()).unwrap_or(&[])),
            None => concat(texts(token).get(s.core.clone()).unwrap_or(&[])),
        };
        let Reading::Legal(mv) = read(&board, &core, lang) else {
            out.complete = false;
            break;
        };
        let Some(san) = san_of(&board, mv) else {
            out.complete = false;
            break;
        };
        for u in decoded.map(|(_, used)| used).unwrap_or_default() {
            out.used.push((u, token.text(), san.to_english()));
        }
        if board.try_play(mv).is_err() {
            out.complete = false;
            break;
        }
        out.sans.push(san.to_english());
    }
    out
}

/// One symbol a proof read as a piece, in one legal move.
#[derive(Clone, Debug, PartialEq)]
pub struct Observation {
    pub symbol: String,
    pub piece: PieceKind,
    /// The token as read.
    pub raw: String,
    /// The legal move it was, in English SAN.
    pub san: String,
}

/// What a replay said about a cipher.
#[derive(Clone, Debug, PartialEq)]
pub enum Proof {
    /// No symbol is supported enough to try.
    NothingToProve,
    /// One assignment replays strictly further than the text as written, and every assignment
    /// that replays as far reads the same moves.
    Proven {
        /// The symbols the replayed moves used, and their pieces.
        assignment: CipherMap,
        /// One per use of a symbol in a replayed move: the evidence a book collects.
        observations: Vec<Observation>,
        /// Plies replayed reading the text as written.
        plain_plies: usize,
        plies: usize,
        /// Whether every token replayed.
        complete: bool,
    },
    /// No assignment replays further than the text as written: decoding would not help, or
    /// would make things worse.
    NoGain { plain_plies: usize, best_plies: usize },
    /// Assignments that replay equally far read different moves: the page does not say which.
    Disputed { plies: usize },
}

/// Every injective assignment of `symbols` to the five pieces.
fn assignments(symbols: &[String]) -> Vec<CipherMap> {
    const PIECES: [PieceKind; 5] = [PieceKind::King, PieceKind::Queen, PieceKind::Rook, PieceKind::Bishop, PieceKind::Knight];
    let mut out = vec![CipherMap::new()];
    for sym in symbols {
        let mut next = Vec::new();
        for a in &out {
            for p in PIECES {
                if !a.values().any(|v| *v == p) {
                    let mut b = a.clone();
                    b.insert(sym.clone(), p);
                    next.push(b);
                }
            }
        }
        out = next;
    }
    out
}

/// Prove which pieces `report`'s most supported symbols stand for, by replaying `tokens` (move
/// tokens in reading order) from `ctx` in `lang` under each assignment (`validate.py:537-566`).
///
/// The replay reads each token as written or decoded, nothing else, so a ply counts only when
/// the symbol read literally is legal there. Accepted only when the best assignment replays
/// strictly further than the plain reading (the Python's `best_key > baseline`), and refused when
/// two assignments that replay as far read different moves (`legality_repair.py:682-685`).
pub fn prove(ctx: &GameContext, tokens: &[ReadToken], lang: Lang, report: &CipherReport) -> Proof {
    let free: Vec<String> = report.symbols.iter().take(MAX_FREE_SYMBOLS).map(|(s, _)| s.clone()).collect();
    if free.is_empty() {
        return Proof::NothingToProve;
    }
    let shaped: Vec<(&ReadToken, Option<Shape>)> = tokens.iter().take(MAX_TOKENS).map(|t| (t, shape(t))).collect();
    let key = |r: &Strict| (r.sans.len(), r.complete);
    let plain = replay_strict(ctx, &shaped, lang, &CipherMap::new());
    let runs: Vec<Strict> = assignments(&free).iter().map(|a| replay_strict(ctx, &shaped, lang, a)).collect();
    let Some(best) = runs.iter().max_by_key(|r| key(r)) else { return Proof::NothingToProve };
    if key(best) <= key(&plain) {
        return Proof::NoGain { plain_plies: plain.sans.len(), best_plies: best.sans.len() };
    }
    if runs.iter().any(|r| key(r) == key(best) && r.sans != best.sans) {
        return Proof::Disputed { plies: best.sans.len() };
    }
    let mut assignment = CipherMap::new();
    let mut observations = Vec::new();
    for ((symbol, piece), raw, san) in &best.used {
        assignment.insert(symbol.clone(), *piece);
        observations.push(Observation { symbol: symbol.clone(), piece: *piece, raw: raw.clone(), san: san.clone() });
    }
    if observations.is_empty() {
        // Cannot happen: a replay goes further only by decoding a symbol. Refuse rather than
        // claim a proof without evidence.
        return Proof::NoGain { plain_plies: plain.sans.len(), best_plies: best.sans.len() };
    }
    Proof::Proven { assignment, observations, plain_plies: plain.sans.len(), plies: best.sans.len(), complete: best.complete }
}

/// How a token was repaired.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepairKind {
    /// A cipher symbol decoded to its piece.
    Cipher,
    /// A slot replaced by one of the reader's alternatives.
    Alternative,
    /// A slot replaced through the OCR confusion table (plain text).
    Confusion,
    /// The other reader's word taken.
    OtherReader,
}

/// One repaired token.
#[derive(Clone, Debug, PartialEq)]
pub struct Repair {
    /// The token as read.
    pub from: String,
    /// The token repaired: the same punctuation, number and marks around a new move.
    pub to: String,
    /// The new text of each slot (empty for a slot merged into its neighbour), or `None` when the
    /// whole word was replaced and its slots no longer line up.
    pub slots: Option<Vec<String>>,
    /// The legal move, in canonical English SAN with the position's check mark.
    pub san: San,
    pub kind: RepairKind,
    /// Why, for the reviewer.
    pub why: String,
    pub confidence: f32,
}

/// What became of one token.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// Legal as read.
    Kept(San),
    Repaired(Repair),
    /// Several legal moves are equally close: flagged, not applied. The replay stops.
    Ambiguous {
        candidates: Vec<San>,
    },
    /// No legal reading within the allowed edits. The replay stops; `near` are the closest legal
    /// moves, for the reviewer.
    Unresolved {
        near: Vec<San>,
    },
    /// After the replay stopped: not judged.
    NotReached,
}

/// A replay with repairs.
#[derive(Clone, Debug, PartialEq)]
pub struct Replay {
    /// One per token, in order.
    pub outcomes: Vec<Outcome>,
    pub plies: usize,
    /// Whether every token became a legal move.
    pub complete: bool,
    /// The position after the last legal move.
    pub fen: String,
}

/// Glyphs an OCR engine mixes up: each key lists what it might have been
/// (`legality_repair.py:97-144`, `CONFUSIONS`; Cyrillic homoglyphs from the same table).
fn confusions(c: &str) -> &'static [&'static str] {
    match c {
        "l" => &["1", "I", "i", "|"],
        "1" => &["l", "I", "i", "|"],
        "I" => &["1", "l", "i"],
        "i" => &["1", "l", "I"],
        "|" => &["1", "l"],
        "5" => &["S", "s"],
        "S" | "s" => &["5"],
        "8" => &["B", "3"],
        "B" => &["8"],
        "0" => &["O", "o", "Q", "D"],
        "O" => &["0", "Q", "D"],
        "o" => &["0", "O"],
        "Q" => &["O", "0"],
        "6" => &["b", "G", "5"],
        "b" | "G" => &["6"],
        "2" => &["Z", "z"],
        "Z" => &["2"],
        "9" => &["g", "q"],
        "g" | "q" => &["9"],
        "4" => &["A"],
        "A" => &["4"],
        "7" => &["T", "/"],
        "T" => &["7"],
        "3" => &["8"],
        "c" => &["e"],
        "e" => &["c"],
        "D" => &["0", "O"],
        "R" => &["P"],
        "К" => &["K"],
        "K" => &["К"],
        "С" => &["C"],
        "C" => &["С"],
        "Р" => &["P", "R"],
        "P" => &["Р"],
        "х" => &["x"],
        "x" => &["х", ":", "×"],
        ":" | "×" => &["x"],
        "о" => &["o", "0", "O"],
        "е" => &["e"],
        "а" => &["a"],
        "р" => &["p", "r"],
        _ => &[],
    }
}

/// A variant of a token's slot texts to try.
struct Variant {
    texts: Vec<String>,
    /// `None`: the other reader's core, replacing the whole core.
    whole: Option<String>,
    confidence: f32,
    kind: RepairKind,
    why: String,
}

/// A substitute text and the reader's probability for it (None: from the confusion table).
type Substitute = (String, Option<f32>);

/// The substitutions a slot allows: its reader's alternatives, or the confusion table.
fn substitutes(slot: &Slot) -> Vec<Substitute> {
    if slot.alternatives.is_empty() {
        confusions(&slot.text).iter().map(|s| ((*s).to_owned(), None)).collect()
    } else {
        slot.alternatives
            .iter()
            .filter(|a| a.text != slot.text && a.p.is_finite())
            .take(MAX_ALTERNATIVES)
            .map(|a| (a.text.clone(), Some(a.p)))
            .collect()
    }
}

/// Variants with `depth` substitutions, over the core slots in `positions`.
fn variants(token: &ReadToken, base: &[String], positions: &[usize], depth: usize) -> Vec<Variant> {
    let subs: Vec<(usize, Vec<Substitute>)> =
        positions.iter().filter_map(|&i| token.slots.get(i).map(|s| (i, substitutes(s)))).filter(|(_, s)| !s.is_empty()).collect();
    let mut out = Vec::new();
    let emit = |changes: &[(usize, &String, Option<f32>)], out: &mut Vec<Variant>| {
        if out.len() >= MAX_VARIANTS {
            return;
        }
        let mut texts = base.to_vec();
        let mut why = Vec::new();
        for (i, new, _) in changes {
            if let Some(slot) = texts.get_mut(*i) {
                why.push(format!("'{slot}' taken as '{new}'"));
                (*new).clone_into(slot);
            }
        }
        let model = changes.iter().any(|c| c.2.is_some());
        let p: f32 = changes.iter().map(|c| c.2.unwrap_or(CONFIDENCE_ONE_SWAP)).product();
        let cap = if changes.len() > 1 { CONFIDENCE_TWO_SWAPS } else { CONFIDENCE_ONE_SWAP };
        let (confidence, kind) = if model { (p.clamp(0.0, cap), RepairKind::Alternative) } else { (cap, RepairKind::Confusion) };
        out.push(Variant { texts, whole: None, confidence, kind, why: why.join(", ") });
    };
    for (a, (i, subs_i)) in subs.iter().enumerate() {
        for (text_i, p_i) in subs_i {
            if depth == 1 {
                emit(&[(*i, text_i, *p_i)], &mut out);
                continue;
            }
            for (j, subs_j) in subs.iter().skip(a + 1) {
                for (text_j, p_j) in subs_j {
                    emit(&[(*i, text_i, *p_i), (*j, text_j, *p_j)], &mut out);
                }
            }
        }
    }
    out
}

/// The legal moves a tier's variants reach: one is a repair, more is an ambiguity.
fn settle(board: &Board, token: &ReadToken, s: &Shape, lang: Lang, tier: Vec<Variant>) -> Option<Outcome> {
    let mut found: Vec<(Move, Option<Variant>)> = Vec::new();
    for v in tier {
        let core = v.whole.clone().unwrap_or_else(|| concat(v.texts.get(s.core.clone()).unwrap_or(&[])));
        match read(board, &core, lang) {
            Reading::Legal(mv) => match found.iter_mut().find(|f| f.0 == mv) {
                Some(f) => {
                    if f.1.as_ref().is_some_and(|old| v.confidence > old.confidence) {
                        f.1 = Some(v);
                    }
                }
                None => found.push((mv, Some(v))),
            },
            Reading::Ambiguous(moves) => {
                for mv in moves {
                    if !found.iter().any(|f| f.0 == mv) {
                        found.push((mv, None));
                    }
                }
            }
            Reading::None => {}
        }
    }
    match found.as_slice() {
        [] => None,
        [(mv, Some(v))] => {
            let san = san_of(board, *mv)?;
            Some(Outcome::Repaired(repair_of(token, s, v, san)))
        }
        _ => Some(Outcome::Ambiguous { candidates: found.iter().filter_map(|(mv, _)| san_of(board, *mv)).collect() }),
    }
}

fn repair_of(token: &ReadToken, s: &Shape, v: &Variant, san: San) -> Repair {
    let t = texts(token);
    let (to, slots) = match &v.whole {
        Some(core) => {
            let before = concat(t.get(..s.core.start).unwrap_or(&[]));
            let after = concat(t.get(s.core.end..).unwrap_or(&[]));
            (format!("{before}{core}{after}"), None)
        }
        None => (concat(&v.texts), Some(v.texts.clone())),
    };
    Repair { from: token.text(), to, slots, why: format!("{}: the only legal reading is {san}", v.why), san, kind: v.kind, confidence: v.confidence }
}

/// Levenshtein distance over characters, for near misses.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row.first().copied().unwrap_or(0);
        if let Some(first) = row.first_mut() {
            *first = i + 1;
        }
        for (j, cb) in b.iter().enumerate() {
            let cur = row.get(j + 1).copied().unwrap_or(0);
            let left = row.get(j).copied().unwrap_or(0);
            let best = (prev + usize::from(ca != *cb)).min(cur + 1).min(left + 1);
            if let Some(cell) = row.get_mut(j + 1) {
                *cell = best;
            }
            prev = cur;
        }
    }
    row.last().copied().unwrap_or(0)
}

fn near_misses(board: &Board, core: &str, lang: Lang) -> Vec<San> {
    let read = San::read(core, lang).map(|s| s.to_english()).unwrap_or_else(|_| core.to_owned());
    let mut scored: Vec<(usize, String, San)> = legal_moves(board)
        .into_iter()
        .filter_map(|mv| san_of(board, mv))
        .map(|s| {
            let e = s.to_english();
            (distance(read.trim_end_matches(['+', '#']), e.trim_end_matches(['+', '#'])), e, s)
        })
        .collect();
    scored.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    scored.into_iter().take(MAX_NEAR_MISSES).map(|(_, _, s)| s).collect()
}

/// Read one token on `board`: as written (decoded first), then one substitution, then two.
fn resolve(board: &Board, token: &ReadToken, lang: Lang, cipher: &CipherMap) -> Outcome {
    let Some(s) = shape(token) else { return Outcome::Unresolved { near: Vec::new() } };
    let written: Vec<String> = texts(token).iter().map(|x| (*x).to_owned()).collect();
    let core_of = |t: &[String]| concat(t.get(s.core.clone()).unwrap_or(&[]));
    let decoded = decode(token, &s, cipher);
    if let Some((t, used)) = &decoded {
        match read(board, &core_of(t), lang) {
            Reading::Legal(mv) => {
                let Some(san) = san_of(board, mv) else { return Outcome::Unresolved { near: Vec::new() } };
                let why =
                    used.iter().map(|(sym, p)| format!("'{sym}' is the book's {}", figurine(*p, FigurineSet::Solid))).collect::<Vec<_>>().join(", ");
                let v = Variant { texts: t.clone(), whole: None, confidence: CONFIDENCE_CIPHER, kind: RepairKind::Cipher, why };
                return Outcome::Repaired(repair_of(token, &s, &v, san));
            }
            Reading::Ambiguous(moves) => return Outcome::Ambiguous { candidates: moves.into_iter().filter_map(|mv| san_of(board, mv)).collect() },
            Reading::None => {}
        }
    }
    match read(board, &core_of(&written), lang) {
        Reading::Legal(mv) => return san_of(board, mv).map_or(Outcome::Unresolved { near: Vec::new() }, Outcome::Kept),
        Reading::Ambiguous(moves) => return Outcome::Ambiguous { candidates: moves.into_iter().filter_map(|mv| san_of(board, mv)).collect() },
        Reading::None => {}
    }
    // Substitutions build on the decoded reading, and never touch a slot the cipher decoded.
    let base = decoded.as_ref().map_or_else(|| written.clone(), |(t, _)| t.clone());
    let positions: Vec<usize> = s.core.clone().filter(|&i| base.get(i) == written.get(i)).collect();
    let mut one = variants(token, &base, &positions, 1);
    if let Some(other) = token.other.as_deref().filter(|o| o.split_whitespace().count() == 1) {
        let o = ReadToken::plain(other.trim());
        if let Some(os) = shape(&o) {
            let core = concat(texts(&o).get(os.core).unwrap_or(&[]));
            one.push(Variant {
                texts: base.clone(),
                whole: Some(core),
                confidence: CONFIDENCE_OTHER_READER,
                kind: RepairKind::OtherReader,
                why: format!("the other reader read '{other}'"),
            });
        }
    }
    if let Some(outcome) = settle(board, token, &s, lang, one) {
        return outcome;
    }
    if MAX_SUBSTITUTIONS >= 2
        && let Some(outcome) = settle(board, token, &s, lang, variants(token, &base, &positions, 2))
    {
        return outcome;
    }
    Outcome::Unresolved { near: near_misses(board, &core_of(&base), lang) }
}

/// Replay `tokens` (move tokens in reading order) from `ctx`, read in `lang` with `cipher`,
/// repairing what legality allows. Tokens past [`MAX_TOKENS`] and past the first token that
/// cannot be read are [`Outcome::NotReached`].
pub fn repair(ctx: &GameContext, tokens: &[ReadToken], lang: Lang, cipher: &CipherMap) -> Replay {
    let mut board = ctx.board.clone();
    let mut outcomes = Vec::with_capacity(tokens.len().min(MAX_TOKENS));
    let mut stopped = false;
    let mut plies = 0;
    for (i, token) in tokens.iter().enumerate() {
        if stopped || i >= MAX_TOKENS {
            stopped = true;
            outcomes.push(Outcome::NotReached);
            continue;
        }
        let outcome = resolve(&board, token, lang, cipher);
        let san = match &outcome {
            Outcome::Kept(san) => Some(san),
            Outcome::Repaired(r) => Some(&r.san),
            _ => None,
        };
        // The SAN was named from a legal move on this board, so it names that move again.
        match san.and_then(|s| s.to_move(&board).ok()).map(|mv| board.try_play(mv)) {
            Some(Ok(())) => plies += 1,
            _ => stopped = true,
        }
        outcomes.push(outcome);
    }
    Replay { outcomes, plies, complete: !stopped, fen: format!("{board}") }
}
