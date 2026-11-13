//! Reading one PGN game in three pure steps (plan A9-F2b): [`scan`] → [`parse`] → [`build`].
//!
//! **Parse** turns the scanner's tokens into the tag section and a flat list of [`FragmentItem`]s,
//! the items a fragment read from a page is made of. Nothing nests: `((((…` is a counter, never a
//! stack of boxes, so no text can make a value deep enough to overflow the stack when dropped.
//! Here the text's spelling is settled:
//!
//! - move numbers (`12.`, `12...`, `12…`, glued or not) are left out;
//! - a glyph standing alone or glued to its move is read by the glyph table (`±`, `+-`, `∞`,
//!   `Nf3±`), `e.p.` after a capture is dropped, and a move PGN would spell otherwise (`e8Q`,
//!   `a8=q`) is read as books print it ([`San::read`]); each of these is noted
//!   ([`PgnWarningKind::NonStandard`]), the traditional `!`/`?` glyphs and castling with zeros
//!   excepted (A1-09);
//! - a variation with no move before it in its line, or nested deeper than [`MAX_DEPTH`], becomes
//!   its text as a comment, noted ([`PgnWarningKind::OrphanVariation`], [`PgnWarningKind::TooDeep`]);
//! - a word no reading makes a move ends its line, as a move that can't be played does;
//! - the result after the moves is the game's, `1/2`, `½-½` and `0.5-0.5` written `1/2-1/2`
//!   (in the `Result` tag too), also when no move came before it (A1-21).
//!
//! **Build** plays the items into a new game with the graft's own steps
//! ([`Game::step_for`], [`Game::reply_with`], [`Game::add_reply`], [`Game::make_first`],
//! [`Game::set_nag_code`]), the PGN's rules on top:
//!
//! - a move that can't be played ends its own line (the variation, or the main line), and the
//!   reading goes on after it; the first one is the report's `stopped`, with why
//!   ([`PgnStopReason`]: a limit is not an illegal move, A1-15);
//! - **a variation never takes the line of a shallower one** (A1-03): each move remembers the
//!   nesting it was first played at, and a line playing where the first reply was first played
//!   deeper puts its own move first (`2. Nf3 (2. Nf3 Nc6 3. Bc4) 2... Nc6 3. Bb5` keeps
//!   `3. Bb5` the main line, `3. Bc4` its variation);
//! - a comment between `)` and the next move is that move's comment-before; one before `(`, the
//!   result or the end goes after the move before the variations (A1-06). A variation holding
//!   only a comment leaves it on the move it was an alternative to. A comment opening a variation
//!   is its first move's comment-before; one before the first move is the game's.
//! - a glyph written `$n` is set as written; one read from a glyph by the side that moved; the
//!   glyphs of a move are kept side by side, whatever their group (the group is the palette's
//!   rule, not the PGN's);
//! - **nothing is cut or dropped unsaid** (plan A9-F3, A1-02): a comment past [`MAX_COMMENT_LEN`]
//!   bytes, a tag past [`MAX_TAG_LEN`] bytes or past [`MAX_TAGS`] tags, a glyph with no move,
//!   past [`super::MAX_NAGS`] on a move or with a code PGN has none for, a result the tag disagrees with
//!   or a tag that is no result: each is a warning with the move it is on; a FEN another program
//!   got slightly wrong (castling rights the pieces don't allow, an en-passant square no double
//!   step left, the clock past 100, move 0 or past [`MAX_MOVE_NUMBER`]) is made legal and said
//!   (A1-08, A1-20), instead of refusing the game;
//! - **the rest of a line after a move that can't be played becomes a comment** where the line
//!   stopped (A4-05), so writing the game back loses none of its text.

use cozy_chess::Color as CozyColor;

use super::graft::{Frame, GraftStopReason};
use super::pgn_scan::{ScanKind, brace_body, scan, tag_parts, tag_value};
use super::{Game, GameError, MAX_COMMENT_LEN, MAX_DEPTH, MAX_MOVE_NUMBER, MAX_PGN_LEN, MAX_PGN_WARNINGS, MAX_TAG_LEN, MAX_TAGS};
use super::{NodeId, PgnReport, PgnStopReason, PgnWarning, PgnWarningKind, cut};
use crate::fen::Position;
use crate::fragment::{FragmentItem, NagCode, items_text};
use crate::nag;
use crate::placement::Color;
use crate::rules::{Violation, position_violations};
use crate::san::{Lang, San, SanBody};

/// Read one game's text (the first game, when it holds more).
pub(super) fn read(text: &str) -> Result<(Game, PgnReport), GameError> {
    if text.len() > MAX_PGN_LEN {
        return Err(GameError::PgnTooLong);
    }
    build(parse(crate::pgn::without_bom(text)))
}

/// What [`parse`] makes of one game's text.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Parsed {
    pub(super) tags: Vec<(String, String)>,
    pub(super) items: Vec<FragmentItem>,
    /// What reading an item noted, by the item's index.
    pub(super) notes: Vec<(usize, PgnWarningKind)>,
    /// What reading the tags noted (on the start).
    pub(super) tag_notes: Vec<PgnWarningKind>,
    /// The first word no reading makes a move: the index of the item it stood before, and the
    /// word. Its line ends there.
    pub(super) unreadable: Option<(usize, String)>,
    /// The result after the moves, as PGN writes it.
    pub(super) result: Option<String>,
    /// A word no reading makes a move ended the main line.
    pub(super) main_stopped: bool,
}

/// Step two: the tokens of one game's text as its tags and its items. Reading stops at the next
/// game's first tag and after the result that ends the moves.
pub(super) fn parse(text: &str) -> Parsed {
    let mut p = Parser { lines: vec![false], ..Parser::default() };
    for token in scan(text) {
        let raw = text.get(token.range).unwrap_or_default();
        if !p.token(token.kind, raw) {
            break;
        }
    }
    if let Some(kept) = p.kept.take() {
        // A variation never closed: kept to the end of the text.
        p.keep_done(kept);
    }
    p.rest_done();
    p.out
}

/// A variation [`parse`] keeps as text: how many brackets are open in it, why, and its text so
/// far (at most [`MAX_COMMENT_LEN`] bytes: the rest would be cut from the comment).
#[derive(Debug)]
struct Kept {
    open: usize,
    kind: PgnWarningKind,
    text: String,
}

impl Kept {
    /// Add a piece of the variation, spaced as PGN writes it.
    fn push(&mut self, piece: &str) {
        push_piece(&mut self.text, piece);
    }
}

/// Add `piece` to text kept from the PGN, spaced as PGN writes it; past [`MAX_COMMENT_LEN`] bytes
/// nothing more is added (the comment it becomes is cut there, and says so).
fn push_piece(text: &mut String, piece: &str) {
    if text.len() > MAX_COMMENT_LEN || piece.is_empty() {
        return;
    }
    if !(text.is_empty() || text.ends_with('(') || piece == ")") {
        text.push(' ');
    }
    text.push_str(piece);
}

#[derive(Debug, Default)]
struct Parser {
    out: Parsed,
    /// The moves have begun: a tag now starts the next game.
    movetext: bool,
    /// A move was read in the main line or a variation's line: a result now ends the moves.
    moved: bool,
    /// For each open line (the main line first), whether a move was read in it.
    lines: Vec<bool>,
    kept: Option<Kept>,
    /// The nesting whose line an unreadable word ended: skipped until it closes.
    dead: Option<usize>,
    /// The text of that line from the word on, which becomes a comment where it stopped.
    rest: Option<String>,
    /// The last item is a move (for an `e.p.` after it).
    after_move: bool,
}

impl Parser {
    /// The nesting of the line being read (0: the main line).
    fn level(&self) -> usize {
        self.lines.len().saturating_sub(1)
    }

    /// Take one token; `false` when this game's text ends before it.
    fn token(&mut self, kind: ScanKind, raw: &str) -> bool {
        if self.kept.is_some() {
            return self.keep_token(kind, raw);
        }
        match kind {
            ScanKind::Tag if self.movetext => return false,
            ScanKind::Tag => self.tag(raw),
            ScanKind::Escape => {}
            _ => {
                self.movetext = true;
                return self.movetext_token(kind, raw);
            }
        }
        true
    }

    /// A tag: kept, cut to [`MAX_TAG_LEN`] bytes, or (past [`MAX_TAGS`], or with no value)
    /// dropped; each cut or drop noted.
    fn tag(&mut self, raw: &str) {
        let Some((name, value)) = tag_parts(raw).filter(|_| self.out.tags.len() < MAX_TAGS) else {
            self.out.tag_notes.push(PgnWarningKind::TagsDropped);
            return;
        };
        let mut value = tag_value(value);
        if name == "Result"
            && let Some(r) = normal_result(&value)
        {
            value = r.to_owned();
        }
        let (name_kept, value_kept) = (cut(name, MAX_TAG_LEN), cut(&value, MAX_TAG_LEN));
        if name_kept.len() < name.len() || value_kept.len() < value.len() {
            self.out.tag_notes.push(PgnWarningKind::TagCut);
        }
        self.out.tags.push((name_kept.to_owned(), value_kept.to_owned()));
    }

    fn movetext_token(&mut self, kind: ScanKind, raw: &str) -> bool {
        if let Some(dead) = self.dead {
            return self.dead_token(dead, kind, raw);
        }
        match kind {
            ScanKind::Open => self.open(),
            ScanKind::Close => self.close(),
            // Whole: the file is capped, and a comment past the cap is cut where it is set, and said.
            ScanKind::BraceComment { .. } => self.push(FragmentItem::Comment { text: brace_body(raw).to_owned(), before: false }),
            ScanKind::LineComment => self.push(FragmentItem::Comment { text: raw.get(1..).unwrap_or_default().to_owned(), before: false }),
            // A code past any PGN has (`$70000`) is code 0, none: dropped where it is set, and said.
            ScanKind::Nag => self.push(FragmentItem::Nag(NagCode::Written(raw.get(1..).unwrap_or_default().parse::<u16>().unwrap_or(0)))),
            ScanKind::Result => return self.result(raw),
            ScanKind::Word => self.word(raw),
            ScanKind::Tag | ScanKind::Escape => {}
        }
        true
    }

    fn push(&mut self, item: FragmentItem) {
        self.after_move = matches!(item, FragmentItem::Move { .. });
        self.out.items.push(item);
    }

    fn note(&mut self, item: usize, kind: PgnWarningKind) {
        self.out.notes.push((item, kind));
    }

    /// A variation: read, or kept as text when no move comes before it in its line or it would
    /// nest deeper than [`MAX_DEPTH`].
    fn open(&mut self) {
        let has_move = self.lines.last().copied().unwrap_or(false);
        if has_move && self.level() < MAX_DEPTH {
            self.push(FragmentItem::Open);
            self.lines.push(false);
        } else {
            let kind = if has_move { PgnWarningKind::TooDeep } else { PgnWarningKind::OrphanVariation };
            self.kept = Some(Kept { open: 1, kind, text: "(".to_owned() });
        }
    }

    /// The end of a variation; a `)` with no `(` is skipped.
    fn close(&mut self) {
        if self.lines.len() > 1 {
            self.lines.pop();
            self.push(FragmentItem::Close);
        }
    }

    /// A result: the game's when it ends the main line's moves; one inside a variation is skipped.
    fn result(&mut self, raw: &str) -> bool {
        if self.level() > 0 {
            return true;
        }
        self.out.result = Some(normal_result(raw).unwrap_or(raw).to_owned());
        // A result before any move (a position given as won) does not end the text.
        !self.moved
    }

    fn word(&mut self, raw: &str) {
        let index = self.out.items.len();
        let read = read_word(raw);
        // A move number written with the ellipsis books print (`12…`): noted on what follows.
        if raw.contains('…') && !matches!(read, WordRead::Move { standard: false, .. }) {
            self.note(index, PgnWarningKind::NonStandard);
        }
        match read {
            WordRead::Skip => {}
            WordRead::Glyphs(codes, standard) => {
                if !standard {
                    self.note(index, PgnWarningKind::NonStandard);
                }
                for code in codes {
                    self.push(FragmentItem::Nag(NagCode::WhiteRelative(code)));
                }
            }
            // The item before it is the capture (or a glyph on it): the note goes to that move.
            WordRead::EnPassant if self.after_move => self.note(index.saturating_sub(1), PgnWarningKind::NonStandard),
            WordRead::Move { san, text, glyphs, standard } => {
                if !standard {
                    self.note(index, PgnWarningKind::NonStandard);
                }
                self.push(FragmentItem::Move { ply: None, san, text, bold: None, rect: None });
                for code in glyphs {
                    self.out.items.push(FragmentItem::Nag(NagCode::WhiteRelative(code)));
                }
                self.moved = true;
                if let Some(line) = self.lines.last_mut() {
                    *line = true;
                }
            }
            WordRead::EnPassant => self.unreadable(index, raw.to_owned()),
            WordRead::Unreadable(word) => self.unreadable(index, word),
        }
    }

    /// A word no reading makes a move: its line ends before item `index`, and its text from the
    /// word on is kept for a comment.
    fn unreadable(&mut self, index: usize, word: String) {
        let mut rest = String::new();
        push_piece(&mut rest, &word);
        self.out.unreadable.get_or_insert((index, word));
        self.out.main_stopped |= self.level() == 0;
        self.dead = Some(self.level());
        self.rest = Some(rest);
    }

    /// A token of a line an unreadable word ended: its text goes to the rest, until the line's
    /// own `)`, which still ends the variation; `false` when this game's text ends before it.
    fn dead_token(&mut self, dead: usize, kind: ScanKind, raw: &str) -> bool {
        let piece = match kind {
            ScanKind::Open => {
                self.lines.push(true);
                "(".to_owned()
            }
            ScanKind::Close if self.lines.len() > 1 => {
                self.lines.pop();
                if self.level() < dead {
                    self.dead = None;
                    self.rest_done();
                    self.push(FragmentItem::Close);
                    return true;
                }
                ")".to_owned()
            }
            ScanKind::Result if self.level() == 0 => return self.result(raw),
            ScanKind::BraceComment { .. } => brace_body(raw).to_owned(),
            ScanKind::LineComment => raw.get(1..).unwrap_or_default().to_owned(),
            ScanKind::Word => without_number(raw),
            ScanKind::Nag | ScanKind::Result => raw.to_owned(),
            ScanKind::Close | ScanKind::Tag | ScanKind::Escape => String::new(),
        };
        if let Some(rest) = self.rest.as_mut() {
            push_piece(rest, piece.trim());
        }
        true
    }

    /// The rest of a line an unreadable word ended, as a comment where it stopped.
    fn rest_done(&mut self) {
        if let Some(text) = self.rest.take() {
            self.push(FragmentItem::Comment { text, before: false });
        }
    }

    /// A token of a variation kept as text; `false` when this game's text ends before it.
    fn keep_token(&mut self, kind: ScanKind, raw: &str) -> bool {
        let Some(mut kept) = self.kept.take() else { return true };
        match kind {
            ScanKind::Open => {
                kept.open = kept.open.saturating_add(1);
                kept.push("(");
            }
            ScanKind::Close => {
                kept.push(")");
                kept.open = kept.open.saturating_sub(1);
                if kept.open == 0 {
                    self.keep_done(kept);
                    return true;
                }
            }
            // The next game: the variation was never closed.
            ScanKind::Tag => {
                self.keep_done(kept);
                return self.token(kind, raw);
            }
            ScanKind::Escape => {}
            ScanKind::BraceComment { .. } => kept.push(brace_body(raw)),
            ScanKind::LineComment => kept.push(raw.get(1..).unwrap_or_default()),
            ScanKind::Nag | ScanKind::Result => kept.push(raw),
            ScanKind::Word => kept.push(&without_number(raw)),
        }
        self.kept = Some(kept);
        true
    }

    /// A variation kept as text, done: a comment where it stood, noted.
    fn keep_done(&mut self, kept: Kept) {
        let index = self.out.items.len();
        self.note(index, kept.kind);
        self.push(FragmentItem::Comment { text: kept.text, before: false });
    }
}

/// `1/2`, `½-½` and `0.5-0.5` as PGN writes a draw, and the standard results as they are.
fn normal_result(text: &str) -> Option<&'static str> {
    match text.trim() {
        "1-0" => Some("1-0"),
        "0-1" => Some("0-1"),
        "1/2-1/2" | "1/2" | "½-½" | "½" | "0.5-0.5" => Some("1/2-1/2"),
        "*" => Some("*"),
        _ => None,
    }
}

/// What a word of the move text is.
#[derive(Debug, PartialEq)]
enum WordRead {
    /// Nothing to read: a move number alone (`12.`, `12...`), or a `$` with no glyph code.
    Skip,
    /// Glyphs alone (`±`, `!?`), by White's codes; `standard`: only the traditional `!` and `?`.
    Glyphs(Vec<u16>, bool),
    /// `e.p.`, `ep`: the capture before it was en passant.
    EnPassant,
    /// A move, the glyphs glued to it beyond its own suffix (White's codes), and whether PGN
    /// spells it so.
    Move { san: San, text: String, glyphs: Vec<u16>, standard: bool },
    /// Not a move, as written (the move number taken off).
    Unreadable(String),
}

/// The move numbers' dots: `.` and the ellipsis books print.
fn is_dot(c: char) -> bool {
    c == '.' || c == '…'
}

/// A word without the move number glued before it (`12...Nf6`, `12…Nf6`, `...Nf6`) and without
/// its other dots (`Nf3.`); empty for a move number alone.
fn without_number(raw: &str) -> String {
    let digits = raw.find(|c: char| !c.is_ascii_digit()).unwrap_or(raw.len());
    let rest = raw.get(digits..).unwrap_or_default();
    let rest = if digits > 0 && !rest.starts_with(is_dot) { raw } else { rest.trim_start_matches(is_dot) };
    if rest.chars().all(|c| c.is_ascii_digit()) {
        return String::new();
    }
    rest.chars().filter(|&c| !is_dot(c)).take(64).collect()
}

/// What `raw` reads as: the canonical SAN first, then glyphs, glyphs glued after a move, `e.p.`,
/// and last the spellings books print.
fn read_word(raw: &str) -> WordRead {
    if matches!(raw.to_ascii_lowercase().as_str(), "e.p." | "e.p" | "ep" | "e. p.") {
        return WordRead::EnPassant;
    }
    let word = without_number(raw);
    // A `$` not followed by a code PGN has (the scanner takes `$` and digits): no glyph.
    if word.is_empty() || word.starts_with('$') {
        return WordRead::Skip;
    }
    if let Ok(san) = San::parse(&word) {
        return WordRead::Move { san, text: word, glyphs: Vec::new(), standard: !raw.contains('…') };
    }
    if let Some(codes) = nag::glyph_codes(&word, true) {
        let standard = word.chars().all(|c| matches!(c, '!' | '?'));
        return WordRead::Glyphs(codes, standard);
    }
    if let Some(read) = glued_glyphs(&word) {
        return read;
    }
    let read = San::read(&word, Lang::En).or_else(|e| promotion_uppercased(&word).ok_or(e).and_then(|w| San::read(&w, Lang::En)));
    match read {
        // Castling with zeros or small o's, and the null moves, are what PGN tools write.
        Ok(san) => {
            let standard = matches!(san.body, SanBody::Castle(_) | SanBody::Null);
            WordRead::Move { san, text: word, glyphs: Vec::new(), standard }
        }
        Err(_) => WordRead::Unreadable(word),
    }
}

/// `a8=q`, `e1n`: a promotion with its piece in a small letter, as some programs write it, with
/// the letter made a capital; `None` for any other word.
fn promotion_uppercased(word: &str) -> Option<String> {
    let body = word.trim_end_matches(['+', '#', '!', '?']);
    let suffix = word.get(body.len()..)?;
    let mut chars: Vec<char> = body.chars().collect();
    let before = chars.len().checked_sub(2).and_then(|i| chars.get(i)).copied()?;
    let last = chars.last_mut()?;
    if !(matches!(*last, 'q' | 'r' | 'b' | 'n') && matches!(before, '=' | '1' | '8')) {
        return None;
    }
    *last = last.to_ascii_uppercase();
    Some(chars.into_iter().collect::<String>() + suffix)
}

/// A move with glyphs glued after it that are not its own `!`/`?` (`Nf3±`, `Nf3+-`): the longest
/// head that reads as a move and a tail made only of glyphs.
fn glued_glyphs(word: &str) -> Option<WordRead> {
    let bounds: Vec<usize> = word.char_indices().map(|(i, _)| i).filter(|&i| i > 0).collect();
    bounds.into_iter().rev().find_map(|at| {
        let (head, tail) = (word.get(..at)?, word.get(at..)?);
        let glyphs = nag::glyph_codes(tail, true)?;
        let san = San::parse(head).or_else(|_| San::read(head, Lang::En)).ok()?;
        Some(WordRead::Move { san, text: word.to_owned(), glyphs, standard: false })
    })
}

/// Step three: the items played into a new game.
pub(super) fn build(parsed: Parsed) -> Result<(Game, PgnReport), GameError> {
    let fen = parsed.tags.iter().find(|(n, _)| n == "FEN").map(|(_, v)| v.as_str());
    let (mut game, repaired) = match fen {
        Some(fen) => start_from(fen)?,
        None => (Game::standard(), false),
    };
    let mut notes: Vec<PgnWarningKind> = parsed.tag_notes.clone();
    if repaired {
        notes.push(PgnWarningKind::FenRepaired);
    }
    for (name, value) in &parsed.tags {
        // The placeholders PGN writes for unknown values are no values. A name PGN does not allow
        // drops its tag, said: the moves are what matters.
        if !matches!(value.as_str(), "?" | "????.??.??" | "*") && game.set_tag(name, value).is_err() {
            notes.push(PgnWarningKind::TagsDropped);
        }
    }
    let mut b = Builder::new(game, parsed.items.len());
    b.notes.extend(notes.into_iter().map(|kind| (0, kind, 0)));
    for (index, item) in parsed.items.iter().enumerate() {
        b.item(index, item);
    }
    Ok(b.finish(parsed))
}

/// The game a `FEN` tag starts, and whether the FEN had to be repaired to be legal: castling
/// rights the pieces do not allow taken off, an en-passant square no double step left dropped, the
/// clock kept to 100 and the move number to 1..=[`MAX_MOVE_NUMBER`] (A1-08, A1-20). A FEN that
/// can't be read, or whose position stays illegal, still refuses the game, with why.
fn start_from(fen: &str) -> Result<(Game, bool), GameError> {
    let mut position = crate::fen::parse(fen.trim()).map_err(|e| GameError::BadFen(e.to_string()))?;
    let repaired = repair(&mut position);
    Ok((Game::new(&position, Color::White)?, repaired))
}

/// Make `position`'s castling rights, en-passant square and counters ones a legal game can
/// have; whether anything changed.
fn repair(position: &mut Position) -> bool {
    let before = position.clone();
    position.castling = position.castling.fitted(&position.placement);
    if position.en_passant.is_some() && position_violations(position, Color::White).contains(&Violation::BadEnPassant) {
        position.en_passant = None;
    }
    position.halfmove_clock = position.halfmove_clock.min(100);
    position.fullmove_number = position.fullmove_number.clamp(1, MAX_MOVE_NUMBER);
    *position != before
}

/// Where [`build`] stands.
struct Builder {
    game: Game,
    /// The move the next one is played after, and the last one played in the line read.
    cur: NodeId,
    last: Option<NodeId>,
    stack: Vec<Frame>,
    /// The nesting whose line a move that could not be played ended: skipped until it closes.
    dead: Option<usize>,
    /// A comment waiting for the next move of the line, with its items: one opening a variation,
    /// or one after a `)` (`after_close`).
    pending: Option<(String, Vec<usize>)>,
    after_close: bool,
    /// The nesting each move was first played at, by node: a line never yields to a deeper one.
    born: Vec<usize>,
    /// The move each item went to, for the notes: a move's own node, a comment's (the move whose
    /// comment or comment-before holds it), a glyph's.
    at: Vec<Option<NodeId>>,
    /// The first move that could not be played: its item, its text and why.
    stop: Option<(usize, String, PgnStopReason)>,
    /// A move that could not be played ended the main line.
    main_stopped: bool,
    /// The line that move ended, from it on (with the comment that waited for it): the items
    /// that become a comment where the line stopped, and the items whose notes go there.
    rest: Option<(Vec<usize>, Vec<FragmentItem>)>,
    /// What building noted: the item, the kind, the move it is on.
    notes: Vec<(usize, PgnWarningKind, NodeId)>,
}

impl Builder {
    fn new(game: Game, items: usize) -> Builder {
        Builder {
            game,
            cur: 0,
            last: None,
            stack: Vec::new(),
            dead: None,
            pending: None,
            after_close: false,
            born: vec![0],
            at: vec![None; items],
            stop: None,
            main_stopped: false,
            rest: None,
            notes: Vec::new(),
        }
    }

    fn item(&mut self, index: usize, item: &FragmentItem) {
        match item {
            FragmentItem::Open => self.open(index),
            FragmentItem::Close => self.close(),
            _ if self.dead.is_some() => self.rest_push(item),
            FragmentItem::Move { san, .. } => self.play(index, san, item),
            FragmentItem::Nag(code) => self.nag(index, *code),
            FragmentItem::Comment { text, .. } => self.comment(index, text),
            FragmentItem::Result => {}
        }
    }

    /// Note `kind` on item `index`, on move `node`.
    fn note(&mut self, index: usize, kind: PgnWarningKind, node: NodeId) {
        self.notes.push((index, kind, node));
    }

    /// A glyph on the last move, next to the glyphs there whatever their group; one with no move
    /// before it, with a code PGN has none for, or past [`super::MAX_NAGS`] is dropped, and said.
    fn nag(&mut self, index: usize, code: NagCode) {
        let to = self.last.unwrap_or(self.cur);
        self.mark(&[index], to);
        let added = self.last.is_some_and(|last| {
            let code = self.game.nag_code_for(last, code);
            self.game.add_nag(last, code)
        });
        if !added {
            self.note(index, PgnWarningKind::NagDropped, to);
        }
    }

    /// An item of a line a move could not be played in: part of the rest.
    fn rest_push(&mut self, item: &FragmentItem) {
        if let Some((_, items)) = self.rest.as_mut() {
            items.push(item.clone());
        }
    }

    /// The rest of the line that died, as a comment where it stopped: after its last move, or,
    /// when its first move could not be played, as a variation holding only a comment is.
    fn place_rest(&mut self) {
        if let Some((indices, items)) = self.rest.take() {
            self.after_close = false;
            self.comment_of(indices, items_text(&items));
        }
    }

    /// Items `indices` went to move `node`.
    fn mark(&mut self, indices: &[usize], node: NodeId) {
        for &index in indices {
            if let Some(slot) = self.at.get_mut(index) {
                *slot = Some(node);
            }
        }
    }

    /// Add the comment `text` of items `indices` after move `node`; cut past
    /// [`MAX_COMMENT_LEN`], and said.
    fn comment_after(&mut self, node: NodeId, (text, indices): (String, Vec<usize>)) {
        self.mark(&indices, node);
        let joined = match self.game.comment(node) {
            "" => text,
            before => format!("{before} {text}"),
        };
        if self.game.set_comment(node, &joined).is_err() {
            self.note(indices.first().copied().unwrap_or(0), PgnWarningKind::CommentCut, node);
        }
    }

    fn open(&mut self, index: usize) {
        if self.dead.is_none() {
            self.flush_after_close();
        } else {
            self.rest_push(&FragmentItem::Open);
        }
        // A variation is an alternative to the move before it; parse leaves none without one.
        let from = match (self.dead, self.last) {
            (None, Some(last)) => self.game.node(last).parent,
            _ => self.cur,
        };
        self.stack.push(Frame { cur: self.cur, last: self.last, open: index });
        (self.cur, self.last) = (from, None);
    }

    fn close(&mut self) {
        if self.stack.is_empty() {
            return;
        }
        match self.dead {
            // The `)` of the line that died: what it did not play goes where it stopped first.
            Some(d) if self.stack.len() <= d => {
                self.dead = None;
                self.place_rest();
            }
            Some(_) => {
                self.rest_push(&FragmentItem::Close);
                if let Some(frame) = self.stack.pop() {
                    (self.cur, self.last) = (frame.cur, frame.last);
                }
                return;
            }
            None => {}
        }
        let Some(frame) = self.stack.pop() else { return };
        self.flush_after_close();
        // A variation holding only a comment: the comment stays on the move it was an
        // alternative to.
        if let Some(pending) = self.pending.take() {
            self.comment_after(frame.last.unwrap_or(frame.cur), pending);
        }
        (self.cur, self.last) = (frame.cur, frame.last);
        self.after_close = true;
    }

    /// A comment after a `)` that no move followed goes after the move before the variations.
    fn flush_after_close(&mut self) {
        if std::mem::take(&mut self.after_close)
            && let Some(pending) = self.pending.take()
        {
            self.comment_after(self.last.unwrap_or(self.cur), pending);
        }
    }

    fn comment(&mut self, index: usize, text: &str) {
        self.comment_of(vec![index], text.to_owned());
    }

    /// The comment `text` of items `indices`, where the line is: waiting for the next move when
    /// it opens a variation or follows a `)`, else after the last move (or on the start).
    fn comment_of(&mut self, indices: Vec<usize>, text: String) {
        let opening = self.last.is_none() && !self.stack.is_empty();
        if self.after_close || opening {
            let (mut joined, mut waiting) = self.pending.take().unwrap_or_default();
            if !joined.is_empty() {
                joined.push(' ');
            }
            joined.push_str(&text);
            waiting.extend(indices);
            self.pending = Some((joined, waiting));
        } else {
            self.comment_after(self.last.unwrap_or(self.cur), (text, indices));
        }
    }

    /// Play a move (`item`) in the line read, after the one it is at.
    fn play(&mut self, index: usize, san: &San, item: &FragmentItem) {
        let level = self.stack.len();
        let white = self.game.node(self.cur).board.side_to_move() == CozyColor::White;
        let node = match self.game.step_for(self.cur, san).and_then(|step| self.place(step, level)) {
            Ok(node) => node,
            Err(reason) => return self.stopped(index, item, reason),
        };
        match self.born.get_mut(node) {
            Some(born) => *born = (*born).min(level),
            None => self.born.resize(node.saturating_add(1), level),
        }
        for code in nag::nags_from_suffix(&san.suffix, white) {
            if !self.game.add_nag(node, code) {
                self.note(index, PgnWarningKind::NagDropped, node);
            }
        }
        if let Some((text, indices)) = self.pending.take() {
            self.mark(&indices, node);
            let joined = match self.game.comment_before(node) {
                "" => text,
                before => format!("{before} {text}"),
            };
            if self.game.set_comment_before(node, &joined).is_err() {
                self.note(indices.first().copied().unwrap_or(index), PgnWarningKind::CommentCut, node);
            }
        }
        (self.cur, self.last, self.after_close) = (node, Some(node), false);
        self.mark(&[index], node);
    }

    /// The reply `step` is after the move the line is at: followed when there, else added; first
    /// when the first reply there was first played deeper than this line (A1-03).
    fn place(&mut self, step: Option<cozy_chess::Move>, level: usize) -> Result<NodeId, GraftStopReason> {
        let first = self.game.node(self.cur).children.first().copied();
        let existing = self.game.reply_with(self.cur, step);
        let yields = first.is_some_and(|f| Some(f) != existing && self.born.get(f).copied().unwrap_or(0) > level);
        match existing {
            Some(node) => {
                // Too deep to reorder: the line stays a variation there (nothing is lost).
                if yields {
                    self.game.make_first(self.cur, node);
                }
                Ok(node)
            }
            None => match self.game.add_reply(self.cur, step, yields) {
                Err(GraftStopReason::TooDeep) if yields => self.game.add_reply(self.cur, step, false),
                added => added,
            },
        }
    }

    /// A move (`item`) that could not be played ends its line, and the line from it on becomes
    /// the rest; the first such move is the report's.
    fn stopped(&mut self, index: usize, item: &FragmentItem, reason: GraftStopReason) {
        if self.stop.is_none() {
            self.stop = Some((index, item.text(), PgnStopReason::from_graft(reason)));
        }
        self.main_stopped |= self.stack.is_empty();
        // A comment waiting for the move goes with the rest, before it.
        self.after_close = false;
        let mut indices = vec![index];
        let mut items = Vec::new();
        if let Some((text, waiting)) = self.pending.take() {
            indices.extend(waiting);
            items.push(FragmentItem::Comment { text, before: false });
        }
        items.push(item.clone());
        self.rest = Some((indices, items));
        self.dead = Some(self.stack.len());
    }

    /// The result after the moves: the game's when no tag says one; one that disagrees with the
    /// tag, or a tag that is no result, is said (A1-21).
    fn result(&mut self, read: Option<&str>) {
        let tag = self.game.tag("Result").map(str::to_owned);
        match (tag.as_deref(), read) {
            (Some(t), _) if normal_result(t).is_none() => self.note(0, PgnWarningKind::ResultIgnored, 0),
            (Some(t), Some(r)) if r != "*" && r != t => self.note(0, PgnWarningKind::ResultIgnored, 0),
            (None, Some(r)) if r != "*" => {
                // A result PGN writes is a value set_tag takes.
                let _ = self.game.set_tag("Result", r);
            }
            _ => {}
        }
    }

    fn finish(mut self, parsed: Parsed) -> (Game, PgnReport) {
        // Variations never closed close at the end of the text; a main line that died leaves its
        // rest at its end.
        while !self.stack.is_empty() {
            self.close();
        }
        self.dead = None;
        self.place_rest();
        self.flush_after_close();
        self.result(parsed.result.as_deref());
        let mut notes: Vec<(usize, PgnWarningKind, NodeId)> =
            parsed.notes.iter().filter_map(|&(index, kind)| Some((index, kind, self.at.get(index).copied().flatten()?))).collect();
        notes.append(&mut self.notes);
        notes.sort_by_key(|(index, ..)| *index);
        let warnings: Vec<PgnWarning> = notes.into_iter().map(|(_, kind, at)| PgnWarning { kind, at }).take(MAX_PGN_WARNINGS).collect();
        // The first of the two: a word no reading makes a move, or a move that could not be played.
        let unreadable_first = match (&parsed.unreadable, &self.stop) {
            (Some((i, _)), Some((j, ..))) => i <= j,
            (unreadable, _) => unreadable.is_some(),
        };
        let first = if unreadable_first {
            parsed.unreadable.map(|(_, word)| (word, PgnStopReason::Illegal))
        } else {
            self.stop.map(|(_, text, reason)| (text, reason))
        };
        let (stopped, stop_reason) = first.unzip();
        let mut game = self.game;
        // The tags the moves decide describe the moves read, unless the main line stopped short
        // of the text's (plan A9-F4).
        game.settle_move_tags(!(self.main_stopped || parsed.main_stopped));
        game.current = 0;
        while game.forward() {}
        (game, PgnReport { stopped, stop_reason, warnings })
    }
}

#[cfg(test)]
mod tests;
