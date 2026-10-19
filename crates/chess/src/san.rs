//! Standard Algebraic Notation in 18 languages and in figurines.
//!
//! Two layers:
//!
//! - **Lexical.** [`San::read`] reads a move as a book prints it, in any of 18 languages or with a
//!   figurine, and [`San::write`] prints it back. The canonical form is English SAN (`Nf3`,
//!   `O-O`, `exd5`, `e8=Q+`); every other spelling is a projection of it, so
//!   `San::read(&san.write(lang, style), lang) == san` for every move and language.
//! - **Semantic.** [`San::from_move`] names a legal move on a board, with the disambiguation and
//!   check marks SAN requires, and [`San::to_move`] finds the one legal move a SAN names.
//!
//! Reading is generous, because books and OCR are: castling as `O-O`, `0-0` or `o-o` with any
//! dash; captures as `x`, `:` or `×`; check as `+` or `†`, mate as `#`, `++` or `‡`; promotion as
//! `=Q`, `(Q)` or a bare `Q`; Cyrillic letters that look like Latin file letters. Writing is
//! strict.

use std::fmt;

use cozy_chess::{Board, GameStatus, Move};

use crate::placement::{Color, PieceKind, Square};

/// Longest move token accepted. Real ones are under 12 characters.
pub const MAX_SAN_LEN: usize = 32;

/// A language with its own piece letters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lang {
    En,
    Pt,
    De,
    Es,
    Fr,
    It,
    Ru,
    Nl,
    Pl,
    Cs,
    Hu,
    Da,
    Sv,
    No,
    Fi,
    Ro,
    Is,
    Ca,
}

/// The letters a language uses for the pieces, and how it writes castling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Letters {
    pub king: &'static str,
    pub queen: &'static str,
    pub rook: &'static str,
    pub bishop: &'static str,
    pub knight: &'static str,
    /// Informational only: SAN never writes a pawn letter. Empty where the language has none
    /// distinct from another piece (Dutch `P` is the knight).
    pub pawn: &'static str,
    /// Castling written with the digit zero (`0-0`), the German and Russian convention.
    pub zero_castling: bool,
}

impl Lang {
    pub const ALL: [Lang; 18] = [
        Lang::En,
        Lang::Pt,
        Lang::De,
        Lang::Es,
        Lang::Fr,
        Lang::It,
        Lang::Ru,
        Lang::Nl,
        Lang::Pl,
        Lang::Cs,
        Lang::Hu,
        Lang::Da,
        Lang::Sv,
        Lang::No,
        Lang::Fi,
        Lang::Ro,
        Lang::Is,
        Lang::Ca,
    ];

    /// The BCP 47 primary subtag.
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Pt => "pt",
            Lang::De => "de",
            Lang::Es => "es",
            Lang::Fr => "fr",
            Lang::It => "it",
            Lang::Ru => "ru",
            Lang::Nl => "nl",
            Lang::Pl => "pl",
            Lang::Cs => "cs",
            Lang::Hu => "hu",
            Lang::Da => "da",
            Lang::Sv => "sv",
            Lang::No => "no",
            Lang::Fi => "fi",
            Lang::Ro => "ro",
            Lang::Is => "is",
            Lang::Ca => "ca",
        }
    }

    /// The language for a BCP 47 tag; only the primary subtag counts (`pt-BR` is `pt`).
    pub fn from_tag(tag: &str) -> Option<Lang> {
        let primary = tag.trim().split(['-', '_']).next()?.to_ascii_lowercase();
        let primary = if primary == "nb" || primary == "nn" { "no".to_owned() } else { primary };
        Lang::ALL.into_iter().find(|l| l.code() == primary)
    }

    pub fn letters(self) -> Letters {
        // King, queen, rook, bishop, knight, pawn, zero castling.
        let (king, queen, rook, bishop, knight, pawn, zero_castling) = match self {
            Lang::En => ("K", "Q", "R", "B", "N", "P", false),
            Lang::Pt => ("R", "D", "T", "B", "C", "P", false),
            Lang::De => ("K", "D", "T", "L", "S", "B", true),
            Lang::Es => ("R", "D", "T", "A", "C", "P", false),
            Lang::Fr => ("R", "D", "T", "F", "C", "P", false),
            Lang::It => ("R", "D", "T", "A", "C", "P", false),
            Lang::Ru => ("Кр", "Ф", "Л", "С", "К", "п", true),
            Lang::Nl => ("K", "D", "T", "L", "P", "", false),
            Lang::Pl => ("K", "H", "W", "G", "S", "p", false),
            Lang::Cs => ("K", "D", "V", "S", "J", "p", false),
            Lang::Hu => ("K", "V", "B", "F", "H", "g", false),
            Lang::Da => ("K", "D", "T", "L", "S", "B", false),
            Lang::Sv => ("K", "D", "T", "L", "S", "B", false),
            Lang::No => ("K", "D", "T", "L", "S", "B", false),
            Lang::Fi => ("K", "D", "T", "L", "R", "s", false),
            Lang::Ro => ("R", "D", "T", "N", "C", "p", false),
            Lang::Is => ("K", "D", "H", "B", "R", "p", false),
            Lang::Ca => ("R", "D", "T", "A", "C", "P", false),
        };
        Letters { king, queen, rook, bishop, knight, pawn, zero_castling }
    }

    /// This language's letter for a piece (empty for the pawn in some languages).
    pub fn letter(self, kind: PieceKind) -> &'static str {
        let l = self.letters();
        match kind {
            PieceKind::King => l.king,
            PieceKind::Queen => l.queen,
            PieceKind::Rook => l.rook,
            PieceKind::Bishop => l.bishop,
            PieceKind::Knight => l.knight,
            PieceKind::Pawn => l.pawn,
        }
    }

    /// The letters SAN can write (every piece but the pawn), longest first so that the Russian
    /// king `Кр` matches before the knight `К`.
    fn officer_letters(self) -> [(&'static str, PieceKind); 5] {
        let l = self.letters();
        let mut pairs = [
            (l.king, PieceKind::King),
            (l.queen, PieceKind::Queen),
            (l.rook, PieceKind::Rook),
            (l.bishop, PieceKind::Bishop),
            (l.knight, PieceKind::Knight),
        ];
        pairs.sort_by_key(|(s, _)| std::cmp::Reverse(s.chars().count()));
        pairs
    }

    fn piece_for_letter(self, token: &str) -> Option<PieceKind> {
        if let Some(kind) = single_char(token).and_then(figurine_piece) {
            return Some(kind);
        }
        self.officer_letters().into_iter().find(|(l, _)| *l == token).map(|(_, k)| k)
    }
}

/// Outline (♔) or solid (♚) chess glyphs. Solid is the print convention for figurine moves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FigurineSet {
    Outline,
    #[default]
    Solid,
}

/// The Unicode glyph for a piece.
pub fn figurine(kind: PieceKind, set: FigurineSet) -> char {
    match (set, kind) {
        (FigurineSet::Outline, PieceKind::King) => '♔',
        (FigurineSet::Outline, PieceKind::Queen) => '♕',
        (FigurineSet::Outline, PieceKind::Rook) => '♖',
        (FigurineSet::Outline, PieceKind::Bishop) => '♗',
        (FigurineSet::Outline, PieceKind::Knight) => '♘',
        (FigurineSet::Outline, PieceKind::Pawn) => '♙',
        (FigurineSet::Solid, PieceKind::King) => '♚',
        (FigurineSet::Solid, PieceKind::Queen) => '♛',
        (FigurineSet::Solid, PieceKind::Rook) => '♜',
        (FigurineSet::Solid, PieceKind::Bishop) => '♝',
        (FigurineSet::Solid, PieceKind::Knight) => '♞',
        (FigurineSet::Solid, PieceKind::Pawn) => '♟',
    }
}

/// The piece a chess glyph (either set) shows.
pub fn figurine_piece(c: char) -> Option<PieceKind> {
    Some(match c {
        '♔' | '♚' => PieceKind::King,
        '♕' | '♛' => PieceKind::Queen,
        '♖' | '♜' => PieceKind::Rook,
        '♗' | '♝' => PieceKind::Bishop,
        '♘' | '♞' => PieceKind::Knight,
        '♙' | '♟' => PieceKind::Pawn,
        _ => return None,
    })
}

/// How a move is printed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Style {
    /// The language's piece letters: `Cf3`.
    Letters,
    /// A glyph for the piece: `♞f3`. Castling and pawn moves have none.
    Figurine(FigurineSet),
    /// Glyph, then the letters in parentheses: `♞f3 (Cf3)`, as bilingual editions print.
    Both(FigurineSet),
}

/// Castling side.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Castle {
    Short,
    Long,
}

/// What a move token says, apart from its check and annotation marks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SanBody {
    Castle(Castle),
    Move {
        /// `None` for a pawn.
        piece: Option<PieceKind>,
        from_file: Option<u8>,
        from_rank: Option<u8>,
        capture: bool,
        to: Square,
        promotion: Option<PieceKind>,
    },
    /// A null move (`--`, `Z0`, `0000`): a pass, used in analysis.
    Null,
}

/// A move as SAN writes it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct San {
    pub body: SanBody,
    /// Check, mate and annotation marks after the move, canonical: `+`, `#`, `!?`, `#!`.
    pub suffix: String,
}

/// Why a move token was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SanError {
    Empty,
    TooLong,
    /// Not a move in the given language.
    Unreadable(String),
    /// A valid move, but not written the canonical English way.
    NotCanonical(String),
    /// No legal move on the board matches.
    NoSuchMove(String),
    /// More than one legal move matches; SAN needed a disambiguation the text does not have.
    Ambiguous(String),
    /// The move given to [`San::from_move`] is not legal on the board.
    IllegalMove,
}

impl fmt::Display for SanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SanError::Empty => write!(f, "the move is empty"),
            SanError::TooLong => write!(f, "the move is longer than {MAX_SAN_LEN} characters"),
            SanError::Unreadable(s) => write!(f, "'{s}' is not a move"),
            SanError::NotCanonical(s) => write!(f, "'{s}' is not canonical English SAN"),
            SanError::NoSuchMove(s) => write!(f, "no legal move matches '{s}'"),
            SanError::Ambiguous(s) => write!(f, "'{s}' matches more than one legal move"),
            SanError::IllegalMove => write!(f, "the move is not legal in this position"),
        }
    }
}

impl std::error::Error for SanError {}

const NULL_MOVES: [&str; 5] = ["--", "Z0", "z0", "0000", "(null)"];

impl San {
    /// Read a move as printed in `lang` (figurines are accepted in every language).
    pub fn read(text: &str, lang: Lang) -> Result<San, SanError> {
        let raw = text.trim();
        if raw.is_empty() {
            return Err(SanError::Empty);
        }
        if raw.chars().count() > MAX_SAN_LEN {
            return Err(SanError::TooLong);
        }
        if NULL_MOVES.contains(&raw) {
            return Ok(San { body: SanBody::Null, suffix: String::new() });
        }
        let unreadable = || SanError::Unreadable(raw.to_owned());
        let normalized = normalize_symbols(raw);
        let (work, suffix) = split_suffix(&normalized);
        if let Some(castle) = read_castle(&work) {
            return Ok(San { body: SanBody::Castle(castle), suffix });
        }
        let (piece, rest) = take_leading_piece(&work, lang);
        let (rest, promotion) = take_promotion(rest, lang);
        let (from_file, from_rank, capture, to) = read_squares(rest).ok_or_else(unreadable)?;
        let piece = match piece {
            Some(PieceKind::Pawn) | None => None,
            Some(kind) => Some(kind),
        };
        if promotion.is_some_and(|p| p == PieceKind::King || p == PieceKind::Pawn) {
            return Err(unreadable());
        }
        Ok(San { body: SanBody::Move { piece, from_file, from_rank, capture, to, promotion }, suffix })
    }

    /// Read canonical English SAN only, exactly as [`San::to_english`] writes it.
    pub fn parse(text: &str) -> Result<San, SanError> {
        let san = San::read(text, Lang::En)?;
        if san.to_english() != text.trim() {
            return Err(SanError::NotCanonical(text.trim().to_owned()));
        }
        Ok(san)
    }

    /// Canonical English SAN.
    pub fn to_english(&self) -> String {
        self.write(Lang::En, Style::Letters)
    }

    /// Print the move in `lang` with `style`.
    pub fn write(&self, lang: Lang, style: Style) -> String {
        match style {
            Style::Letters => self.write_with(|k| lang.letter(k).to_owned(), lang.letters().zero_castling),
            Style::Figurine(set) => self.write_with(|k| figurine(k, set).to_string(), false),
            Style::Both(set) => {
                let fig = self.write(lang, Style::Figurine(set));
                let letters = self.write(lang, Style::Letters);
                if fig == letters { fig } else { format!("{fig} ({letters})") }
            }
        }
    }

    fn write_with(&self, piece: impl Fn(PieceKind) -> String, zero_castling: bool) -> String {
        let mut s = String::new();
        match self.body {
            SanBody::Null => s.push_str("--"),
            SanBody::Castle(side) => {
                s.push_str(match (side, zero_castling) {
                    (Castle::Short, false) => "O-O",
                    (Castle::Long, false) => "O-O-O",
                    (Castle::Short, true) => "0-0",
                    (Castle::Long, true) => "0-0-0",
                });
            }
            SanBody::Move { piece: kind, from_file, from_rank, capture, to, promotion } => {
                if let Some(kind) = kind {
                    s.push_str(&piece(kind));
                }
                if let Some(f) = from_file {
                    s.push(char::from(b'a' + f));
                }
                if let Some(r) = from_rank {
                    s.push(char::from(b'1' + r));
                }
                if capture {
                    s.push('x');
                }
                s.push_str(&to.to_string());
                if let Some(p) = promotion {
                    s.push('=');
                    s.push_str(&piece(p));
                }
            }
        }
        s.push_str(&self.suffix);
        s
    }

    /// Name a legal move on `board`, with the disambiguation and check or mate mark it needs.
    pub fn from_move(board: &Board, mv: Move) -> Result<San, SanError> {
        if !board.is_legal(mv) {
            return Err(SanError::IllegalMove);
        }
        let us = board.side_to_move();
        let moving = board.piece_on(mv.from).ok_or(SanError::IllegalMove)?;
        let from = square_from_cozy(mv.from);
        let to = square_from_cozy(mv.to);
        let body = if moving == cozy_chess::Piece::King && board.color_on(mv.to) == Some(us) {
            // cozy-chess writes castling as the king taking its own rook.
            SanBody::Castle(if to.file() > from.file() { Castle::Short } else { Castle::Long })
        } else {
            let kind = kind_from_cozy(moving);
            let is_pawn = kind == PieceKind::Pawn;
            let capture = board.piece_on(mv.to).is_some() || (is_pawn && from.file() != to.file());
            let (from_file, from_rank) = if is_pawn { (capture.then_some(from.file()), None) } else { disambiguation(board, moving, mv) };
            SanBody::Move { piece: (!is_pawn).then_some(kind), from_file, from_rank, capture, to, promotion: mv.promotion.map(kind_from_cozy) }
        };
        let mut after = board.clone();
        after.try_play(mv).map_err(|_| SanError::IllegalMove)?;
        let suffix = if after.checkers().is_empty() {
            ""
        } else if after.status() == GameStatus::Won {
            "#"
        } else {
            "+"
        };
        Ok(San { body, suffix: suffix.to_owned() })
    }

    /// The one legal move on `board` this SAN names. Capture and check marks are not required to
    /// be right: books drop them, and the squares already say which move is meant.
    pub fn to_move(&self, board: &Board) -> Result<Move, SanError> {
        let text = || self.to_english();
        match self.body {
            SanBody::Null => Err(SanError::NoSuchMove(text())),
            SanBody::Castle(side) => {
                let us = board.side_to_move();
                let king = board.king(us);
                let rights = board.castle_rights(us);
                let rook_file = match side {
                    Castle::Short => rights.short,
                    Castle::Long => rights.long,
                }
                .ok_or_else(|| SanError::NoSuchMove(text()))?;
                let mv = Move { from: king, to: cozy_chess::Square::new(rook_file, king.rank()), promotion: None };
                if board.is_legal(mv) { Ok(mv) } else { Err(SanError::NoSuchMove(text())) }
            }
            SanBody::Move { piece, from_file, from_rank, to, promotion, .. } => {
                let want = to_cozy_kind(piece.unwrap_or(PieceKind::Pawn));
                let want_promotion = promotion.map(to_cozy_kind);
                let to = square_to_cozy(to).ok_or_else(|| SanError::NoSuchMove(text()))?;
                let us = board.side_to_move();
                let mut found: Option<Move> = None;
                let mut count = 0usize;
                board.generate_moves(|moves| {
                    if moves.piece != want {
                        return false;
                    }
                    for mv in moves {
                        let from = square_from_cozy(mv.from);
                        let castling = want == cozy_chess::Piece::King && board.color_on(mv.to) == Some(us);
                        if mv.to == to
                            && !castling
                            && mv.promotion == want_promotion
                            && from_file.is_none_or(|f| f == from.file())
                            && from_rank.is_none_or(|r| r == from.rank())
                        {
                            found = Some(mv);
                            count += 1;
                        }
                    }
                    false
                });
                match (found, count) {
                    (Some(mv), 1) => Ok(mv),
                    (None, _) => Err(SanError::NoSuchMove(text())),
                    _ => Err(SanError::Ambiguous(text())),
                }
            }
        }
    }
}

impl fmt::Display for San {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_english())
    }
}

/// The least SAN needs to tell `mv` from the other legal moves of the same kind of piece to the
/// same square: nothing, the file, the rank, or both.
fn disambiguation(board: &Board, piece: cozy_chess::Piece, mv: Move) -> (Option<u8>, Option<u8>) {
    let from = square_from_cozy(mv.from);
    let mut others: Vec<Square> = Vec::new();
    board.generate_moves(|moves| {
        if moves.piece == piece && moves.from != mv.from && moves.to.has(mv.to) {
            others.push(square_from_cozy(moves.from));
        }
        false
    });
    if others.is_empty() {
        (None, None)
    } else if others.iter().all(|o| o.file() != from.file()) {
        (Some(from.file()), None)
    } else if others.iter().all(|o| o.rank() != from.rank()) {
        (None, Some(from.rank()))
    } else {
        (Some(from.file()), Some(from.rank()))
    }
}

fn single_char(s: &str) -> Option<char> {
    let mut chars = s.chars();
    let c = chars.next()?;
    chars.next().is_none().then_some(c)
}

/// Fold the punctuation variants printed notation uses.
fn normalize_symbols(text: &str) -> String {
    let chars: Vec<char> = text
        .chars()
        .map(|c| match c {
            '‐' | '‑' | '‒' | '–' | '—' | '―' | '−' => '-',
            // Lower-case Cyrillic twins of Latin file letters, which OCR returns on Russian books.
            // Upper case is left alone: `С` is the Russian bishop.
            'а' => 'a',
            'с' => 'c',
            'е' => 'e',
            'х' => 'x',
            'ѕ' => 's',
            'і' => 'i',
            'ј' => 'j',
            '×' => 'x',
            '†' => '+',
            '‡' | '≠' => '#',
            other => other,
        })
        .collect();
    let mut out = String::with_capacity(text.len());
    for (i, &c) in chars.iter().enumerate() {
        // A colon between a move character and a file letter is the old capture mark.
        let prev = i.checked_sub(1).and_then(|p| chars.get(p));
        let next = chars.get(i + 1);
        let is_capture_colon =
            c == ':' && prev.is_some_and(|p| p.is_alphanumeric() || figurine_piece(*p).is_some()) && next.is_some_and(|n| ('a'..='h').contains(n));
        out.push(if is_capture_colon { 'x' } else { c });
    }
    out
}

/// Split trailing check, mate and annotation marks off a token; `++` becomes `#`.
fn split_suffix(text: &str) -> (String, String) {
    let body = text.trim_end_matches(['+', '#', '!', '?']);
    let tail = text.get(body.len()..).unwrap_or("");
    let suffix = match tail.strip_prefix("++") {
        Some(rest) => format!("#{rest}"),
        None => tail.to_owned(),
    };
    (body.to_owned(), suffix)
}

fn read_castle(body: &str) -> Option<Castle> {
    let parts: Vec<&str> = body.split('-').collect();
    if !parts.iter().all(|p| matches!(*p, "O" | "0" | "o")) {
        return None;
    }
    match parts.len() {
        2 => Some(Castle::Short),
        3 => Some(Castle::Long),
        _ => None,
    }
}

fn take_leading_piece(text: &str, lang: Lang) -> (Option<PieceKind>, &str) {
    let mut chars = text.chars();
    if let Some(kind) = chars.next().and_then(figurine_piece) {
        return (Some(kind), chars.as_str());
    }
    for (letter, kind) in lang.officer_letters() {
        if !letter.is_empty()
            && let Some(rest) = text.strip_prefix(letter)
        {
            return (Some(kind), rest);
        }
    }
    (None, text)
}

fn take_promotion(text: &str, lang: Lang) -> (&str, Option<PieceKind>) {
    if let Some(inner) = text.strip_suffix(')')
        && let Some((head, tail)) = inner.rsplit_once('(')
    {
        return match lang.piece_for_letter(tail.trim()) {
            Some(kind) => (head, Some(kind)),
            None => (text, None),
        };
    }
    if let Some((head, tail)) = text.rsplit_once('=') {
        return match lang.piece_for_letter(tail.trim()) {
            Some(kind) => (head, Some(kind)),
            None => (text, None),
        };
    }
    // A bare promotion letter straight after the rank: `e8Q`.
    let mut candidates: Vec<(String, PieceKind)> =
        lang.officer_letters().into_iter().filter(|(l, _)| !l.is_empty()).map(|(l, k)| (l.to_owned(), k)).collect();
    for kind in [PieceKind::Queen, PieceKind::Rook, PieceKind::Bishop, PieceKind::Knight] {
        for set in [FigurineSet::Outline, FigurineSet::Solid] {
            candidates.push((figurine(kind, set).to_string(), kind));
        }
    }
    candidates.sort_by_key(|(l, _)| std::cmp::Reverse(l.chars().count()));
    for (letter, kind) in candidates {
        if let Some(head) = text.strip_suffix(letter.as_str())
            && head.chars().last().is_some_and(|c| c.is_ascii_digit())
        {
            return (head, Some(kind));
        }
    }
    (text, None)
}

/// `[a-h]?[1-8]?x?[a-h][1-8]`: disambiguation, capture and target square.
fn read_squares(text: &str) -> Option<(Option<u8>, Option<u8>, bool, Square)> {
    let chars: Vec<char> = text.chars().collect();
    let (rank_c, rest) = chars.split_last()?;
    let (file_c, rest) = rest.split_last()?;
    let to = Square::parse(&format!("{file_c}{rank_c}"))?;
    // Long algebraic, as German books print it: `e2-e4`, `Sg1–f3` (dashes are normalised to `-`),
    // and `:` for a capture (`e5:d6`, `Lb5:c6`) as well as `x`. A `-` separates a full from-square.
    let (capture, rest) = match rest.split_last() {
        Some(('x' | ':', rest)) => (true, rest),
        Some(('-', rest)) if rest.len() == 2 => (false, rest),
        Some(('-', _)) => return None,
        _ => (false, rest),
    };
    let file = |c: &char| ('a'..='h').contains(c).then(|| *c as u8 - b'a');
    let rank = |c: &char| ('1'..='8').contains(c).then(|| *c as u8 - b'1');
    match rest {
        [] => Some((None, None, capture, to)),
        [c] => match (file(c), rank(c)) {
            (Some(f), _) => Some((Some(f), None, capture, to)),
            (_, Some(r)) => Some((None, Some(r), capture, to)),
            _ => None,
        },
        [f, r] => Some((Some(file(f)?), Some(rank(r)?), capture, to)),
        _ => None,
    }
}

pub(crate) fn square_from_cozy(sq: cozy_chess::Square) -> Square {
    Square::wrapping(sq.file() as u8, sq.rank() as u8)
}

pub(crate) fn square_to_cozy(sq: Square) -> Option<cozy_chess::Square> {
    Some(cozy_chess::Square::new(cozy_chess::File::try_index(usize::from(sq.file()))?, cozy_chess::Rank::try_index(usize::from(sq.rank()))?))
}

pub(crate) fn kind_from_cozy(p: cozy_chess::Piece) -> PieceKind {
    match p {
        cozy_chess::Piece::Pawn => PieceKind::Pawn,
        cozy_chess::Piece::Knight => PieceKind::Knight,
        cozy_chess::Piece::Bishop => PieceKind::Bishop,
        cozy_chess::Piece::Rook => PieceKind::Rook,
        cozy_chess::Piece::Queen => PieceKind::Queen,
        cozy_chess::Piece::King => PieceKind::King,
    }
}

pub(crate) fn to_cozy_kind(k: PieceKind) -> cozy_chess::Piece {
    match k {
        PieceKind::Pawn => cozy_chess::Piece::Pawn,
        PieceKind::Knight => cozy_chess::Piece::Knight,
        PieceKind::Bishop => cozy_chess::Piece::Bishop,
        PieceKind::Rook => cozy_chess::Piece::Rook,
        PieceKind::Queen => cozy_chess::Piece::Queen,
        PieceKind::King => cozy_chess::Piece::King,
    }
}

/// The side a `cozy-chess` colour stands for.
pub fn color_from_cozy(c: cozy_chess::Color) -> Color {
    match c {
        cozy_chess::Color::White => Color::White,
        cozy_chess::Color::Black => Color::Black,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn en(s: &str) -> San {
        San::parse(s).unwrap()
    }

    #[test]
    fn translations_of_one_move() {
        let nf3 = en("Nf3");
        assert_eq!(nf3.write(Lang::Pt, Style::Letters), "Cf3");
        assert_eq!(nf3.write(Lang::De, Style::Letters), "Sf3");
        assert_eq!(nf3.write(Lang::Ru, Style::Letters), "Кf3");
        assert_eq!(nf3.write(Lang::En, Style::Figurine(FigurineSet::Solid)), "♞f3");
        assert_eq!(nf3.write(Lang::En, Style::Figurine(FigurineSet::Outline)), "♘f3");
        assert_eq!(nf3.write(Lang::Pt, Style::Both(FigurineSet::Solid)), "♞f3 (Cf3)");
        assert_eq!(en("O-O").write(Lang::De, Style::Letters), "0-0");
        assert_eq!(en("O-O-O+").write(Lang::Ru, Style::Letters), "0-0-0+");
        assert_eq!(en("e8=Q+").write(Lang::Pt, Style::Letters), "e8=D+");
        assert_eq!(en("Kxe2").write(Lang::Ru, Style::Letters), "Крxe2");
        assert_eq!(en("exd5").write(Lang::Pt, Style::Both(FigurineSet::Solid)), "exd5");
    }

    #[test]
    fn generous_reading() {
        let read = |s: &str, lang| San::read(s, lang).unwrap().to_english();
        assert_eq!(read("0-0", Lang::En), "O-O");
        assert_eq!(read("o–o–o", Lang::En), "O-O-O");
        assert_eq!(read("e:d5", Lang::En), "exd5");
        assert_eq!(read("Nf3†", Lang::En), "Nf3+");
        assert_eq!(read("Qh7++", Lang::En), "Qh7#");
        assert_eq!(read("Dh7‡", Lang::Pt), "Qh7#");
        assert_eq!(read("e8(Q)", Lang::En), "e8=Q");
        assert_eq!(read("e8Q", Lang::En), "e8=Q");
        assert_eq!(read("e8D", Lang::De), "e8=Q");
        assert_eq!(read("e8♛", Lang::En), "e8=Q");
        assert_eq!(read("♘xe5!?", Lang::Pt), "Nxe5!?");
        assert_eq!(read("Сxе4", Lang::Ru), "Bxe4");
        assert_eq!(read("Крg1", Lang::Ru), "Kg1");
        assert_eq!(read("Кg1", Lang::Ru), "Ng1");
        assert_eq!(read("Nbd2", Lang::En), "Nbd2");
        assert_eq!(read("R1a3", Lang::En), "R1a3");
        assert_eq!(read("Qh4xe1", Lang::En), "Qh4xe1");
        assert_eq!(read(" Pf3 ", Lang::Nl), "Nf3");
        assert_eq!(read("--", Lang::En), "--");
    }

    #[test]
    fn unreadable_tokens_are_errors() {
        for s in ["", "  ", "Xf3", "Nf9", "Ni3", "e8=K", "N", "Nbcd2", "O-O-O-O", "e4e5e6"] {
            assert!(San::read(s, Lang::En).is_err(), "{s}");
        }
        assert_eq!(San::read(&"N".repeat(40), Lang::En), Err(SanError::TooLong));
        assert_eq!(San::parse("0-0"), Err(SanError::NotCanonical("0-0".into())));
    }

    /// Long algebraic, as German books print it: the from-square, `-` (or a dash) for a move and
    /// `:` (or `x`) for a capture. It plays as the move it names.
    #[test]
    fn long_algebraic_reads_and_plays() {
        let read = |s: &str, lang: Lang| San::read(s, lang).map(|m| m.to_english()).unwrap_or_else(|e| format!("error: {e}"));
        assert_eq!(read("e2-e4", Lang::En), "e2e4");
        assert_eq!(read("Sg1–f3", Lang::De), "Ng1f3");
        assert_eq!(read("Lf1—b5", Lang::De), "Bf1b5");
        assert_eq!(read("e5:d6", Lang::De), "e5xd6");
        assert_eq!(read("Lb5:c6+", Lang::De), "Bb5xc6+");
        for bad in ["e-4", "-e4", "Sg-f3", "e2--e4", "e2-"] {
            assert!(San::read(bad, Lang::De).is_err(), "{bad}");
        }
        let start = Board::default();
        let mv = San::read("e2-e4", Lang::En).unwrap().to_move(&start).unwrap();
        assert_eq!(mv.to_string(), "e2e4");
        let mv = San::read("Sg1-f3", Lang::De).unwrap().to_move(&start).unwrap();
        assert_eq!(mv.to_string(), "g1f3");
        // A from-square with nothing of its colour on it is not a move here.
        assert!(San::read("Sg2-f3", Lang::De).unwrap().to_move(&start).is_err());
    }

    #[test]
    fn language_tags() {
        assert_eq!(Lang::from_tag("pt-BR"), Some(Lang::Pt));
        assert_eq!(Lang::from_tag("DE_at"), Some(Lang::De));
        assert_eq!(Lang::from_tag("nb"), Some(Lang::No));
        assert_eq!(Lang::from_tag("xx"), None);
        assert_eq!(Lang::from_tag(""), None);
    }

    fn san_of(fen: &str, uci: &str) -> String {
        let board = Board::from_fen(fen, false).unwrap();
        let mv: Move = uci.parse().unwrap();
        San::from_move(&board, mv).unwrap().to_english()
    }

    #[test]
    fn naming_legal_moves() {
        let start = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
        assert_eq!(san_of(start, "g1f3"), "Nf3");
        assert_eq!(san_of(start, "e2e4"), "e4");
        // Knights on b1 and f3 can both reach d2: the file tells them apart.
        assert_eq!(san_of("4k3/8/8/8/8/5N2/8/1N2K3 w - - 0 1", "b1d2"), "Nbd2");
        // Rooks on a1 and a5 share a file: the rank tells them apart.
        assert_eq!(san_of("4k3/8/8/R7/8/8/8/R3K3 w - - 0 1", "a1a3"), "R1a3");
        // Three queens: h4, e4 and h1 all reach e1; h1 shares the file and e4 the rank, so only
        // file and rank together name h4.
        assert_eq!(san_of("8/k7/8/8/4Q2Q/8/8/K6Q w - - 0 1", "h4e1"), "Qh4e1");
        // Castling, written by cozy-chess as the king taking its rook.
        assert_eq!(san_of("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1", "e1h1"), "O-O");
        assert_eq!(san_of("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1", "e1a1"), "O-O-O");
        // En passant is a capture although d6 is empty.
        assert_eq!(san_of("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1", "e5d6"), "exd6");
        assert_eq!(san_of("4k3/1P6/8/8/8/8/8/4K3 w - - 0 1", "b7b8q"), "b8=Q+");
        // Fool's mate.
        assert_eq!(san_of("rnbqkbnr/pppp1ppp/8/4p3/6P1/5P2/PPPPP2P/RNBQKBNR b KQkq - 0 2", "d8h4"), "Qh4#");
    }

    #[test]
    fn finding_the_move_a_san_names() {
        let board = Board::from_fen("4k3/8/8/8/8/5N2/8/1N2K3 w - - 0 1", false).unwrap();
        assert_eq!(San::read("Nd2", Lang::En).unwrap().to_move(&board), Err(SanError::Ambiguous("Nd2".into())));
        assert_eq!(San::read("Nbd2", Lang::En).unwrap().to_move(&board), Ok("b1d2".parse().unwrap()));
        assert_eq!(San::read("Nd4", Lang::En).unwrap().to_move(&board), Ok("f3d4".parse().unwrap()));
        // A dropped capture mark still finds the move; a wrong square does not.
        assert!(San::read("Ne5", Lang::En).unwrap().to_move(&board).is_ok());
        assert_eq!(San::read("Ne6", Lang::En).unwrap().to_move(&board), Err(SanError::NoSuchMove("Ne6".into())));
        assert!(San::read("O-O", Lang::En).unwrap().to_move(&board).is_err());
        assert!(San::read("--", Lang::En).unwrap().to_move(&board).is_err());
    }

    /// A small deterministic generator so the test needs no random-number crate.
    struct XorShift(u64);
    impl XorShift {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
    }

    fn legal_moves(board: &Board) -> Vec<Move> {
        let mut v = Vec::new();
        board.generate_moves(|moves| {
            v.extend(moves);
            false
        });
        v
    }

    #[test]
    fn every_legal_move_round_trips_in_every_language() {
        let mut rng = XorShift(0x9E37_79B9_7F4A_7C15);
        let mut positions = 0;
        while positions < 1000 {
            let mut board = Board::default();
            let plies = rng.next() % 80;
            for _ in 0..plies {
                let moves = legal_moves(&board);
                if moves.is_empty() {
                    break;
                }
                let mv = moves[(rng.next() % moves.len() as u64) as usize];
                board.play(mv);
            }
            positions += 1;
            for mv in legal_moves(&board) {
                let san = San::from_move(&board, mv).unwrap();
                assert_eq!(San::parse(&san.to_english()).as_ref(), Ok(&san));
                for lang in Lang::ALL {
                    for style in [Style::Letters, Style::Figurine(FigurineSet::Solid), Style::Figurine(FigurineSet::Outline)] {
                        let text = san.write(lang, style);
                        let back = San::read(&text, lang).unwrap_or_else(|e| panic!("{text} in {lang:?}: {e}"));
                        assert_eq!(back, san, "{text} in {lang:?}");
                        assert_eq!(back.to_move(&board), Ok(mv), "{text} in {lang:?}");
                    }
                }
            }
        }
    }

    proptest::proptest! {
        #[test]
        fn reading_never_panics(s in "\\PC{0,40}", lang in 0usize..18) {
            let _ = San::read(&s, Lang::ALL[lang]);
        }
    }
}
