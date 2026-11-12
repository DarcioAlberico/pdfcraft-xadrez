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
//! - a glyph written `$n` is set as written; one read from a glyph by the side that moved.

use cozy_chess::Color as CozyColor;

use super::graft::{Frame, GraftStopReason};
use super::pgn_scan::{ScanKind, brace_body, scan, tag_parts, tag_value};
use super::{Game, GameError, MAX_COMMENT_LEN, MAX_DEPTH, MAX_PGN_LEN, MAX_PGN_WARNINGS, MAX_TAG_LEN, MAX_TAGS, NodeId, PgnReport, PgnStopReason};
use super::{PgnWarning, PgnWarningKind, cut};
use crate::fragment::{FragmentItem, NagCode};
use crate::nag;
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
    /// The first word no reading makes a move: the index of the item it stood before, and the
    /// word. Its line ends there.
    pub(super) unreadable: Option<(usize, String)>,
    /// The result after the moves, as PGN writes it.
    pub(super) result: Option<String>,
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
        if self.text.len() > MAX_COMMENT_LEN || piece.is_empty() {
            return;
        }
        if !(self.text.ends_with('(') || piece == ")") {
            self.text.push(' ');
        }
        self.text.push_str(piece);
    }
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

    fn tag(&mut self, raw: &str) {
        let Some((name, value)) = tag_parts(raw) else { return };
        if self.out.tags.len() >= MAX_TAGS {
            return;
        }
        let mut value = tag_value(value);
        if name == "Result"
            && let Some(r) = normal_result(&value)
        {
            value = r.to_owned();
        }
        self.out.tags.push((cut(name, MAX_TAG_LEN).to_owned(), cut(&value, MAX_TAG_LEN).to_owned()));
    }

    fn movetext_token(&mut self, kind: ScanKind, raw: &str) -> bool {
        if let Some(dead) = self.dead {
            match kind {
                ScanKind::Open => self.lines.push(true),
                // The `)` of the variation the line was in: the variation still ends there.
                ScanKind::Close if self.lines.len() > 1 => {
                    self.lines.pop();
                    if self.level() < dead {
                        self.dead = None;
                        self.push(FragmentItem::Close);
                    }
                }
                ScanKind::Result => return self.result(raw),
                _ => {}
            }
            return true;
        }
        match kind {
            ScanKind::Open => self.open(),
            ScanKind::Close => self.close(),
            ScanKind::BraceComment { .. } => {
                self.push(FragmentItem::Comment { text: cut(brace_body(raw), MAX_COMMENT_LEN.saturating_mul(2)).to_owned(), before: false })
            }
            ScanKind::LineComment => {
                self.push(FragmentItem::Comment { text: cut(raw.get(1..).unwrap_or_default(), MAX_COMMENT_LEN).to_owned(), before: false })
            }
            ScanKind::Nag => {
                if let Ok(code) = raw.get(1..).unwrap_or_default().parse::<u16>() {
                    self.push(FragmentItem::Nag(NagCode::Written(code)));
                }
            }
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

    /// A word no reading makes a move: its line ends before item `index`.
    fn unreadable(&mut self, index: usize, word: String) {
        self.out.unreadable.get_or_insert((index, word));
        self.dead = Some(self.level());
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
    let mut game = match fen {
        Some(fen) => Game::from_fen(fen)?,
        None => Game::standard(),
    };
    for (name, value) in &parsed.tags {
        // The placeholders PGN writes for unknown values are no values. Bad names are skipped,
        // not fatal: the moves are what matters.
        if !matches!(value.as_str(), "?" | "????.??.??" | "*") {
            let _ = game.set_tag(name, value);
        }
    }
    let mut b = Builder::new(game, parsed.items.len());
    for (index, item) in parsed.items.iter().enumerate() {
        b.item(index, item);
    }
    Ok(b.finish(parsed))
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
        }
    }

    fn item(&mut self, index: usize, item: &FragmentItem) {
        match item {
            FragmentItem::Open => self.open(index),
            FragmentItem::Close => self.close(),
            _ if self.dead.is_some() => {}
            FragmentItem::Move { san, text, .. } => self.play(index, san, text),
            FragmentItem::Nag(code) => {
                let to = self.last.unwrap_or(self.cur);
                self.mark(&[index], to);
                if let Some(last) = self.last {
                    self.game.set_nag_code(last, *code);
                }
            }
            FragmentItem::Comment { text, .. } => self.comment(index, text),
            FragmentItem::Result => {}
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

    /// Add the comment `text` of items `indices` after move `node`.
    fn comment_after(&mut self, node: NodeId, (text, indices): (String, Vec<usize>)) {
        self.mark(&indices, node);
        append_comment(&mut self.game, node, text);
    }

    fn open(&mut self, index: usize) {
        if self.dead.is_none() {
            self.flush_after_close();
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
        let Some(frame) = self.stack.pop() else { return };
        if self.dead.is_none() {
            self.flush_after_close();
            // A variation holding only a comment: the comment stays on the move it was an
            // alternative to.
            if let Some(pending) = self.pending.take() {
                self.comment_after(frame.last.unwrap_or(frame.cur), pending);
            }
        }
        (self.cur, self.last) = (frame.cur, frame.last);
        if self.dead.is_some_and(|d| self.stack.len() < d) {
            self.dead = None;
        }
        self.after_close = self.dead.is_none();
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
        let opening = self.last.is_none() && !self.stack.is_empty();
        if self.after_close || opening {
            let (mut joined, mut indices) = self.pending.take().unwrap_or_default();
            if !joined.is_empty() {
                joined.push(' ');
            }
            joined.push_str(text);
            indices.push(index);
            self.pending = Some((joined, indices));
        } else {
            self.comment_after(self.last.unwrap_or(self.cur), (text.to_owned(), vec![index]));
        }
    }

    /// Play a move in the line read, after the one it is at.
    fn play(&mut self, index: usize, san: &San, text: &str) {
        let level = self.stack.len();
        let white = self.game.node(self.cur).board.side_to_move() == CozyColor::White;
        let node = match self.game.step_for(self.cur, san).and_then(|step| self.place(step, level)) {
            Ok(node) => node,
            Err(reason) => return self.stopped(index, text, reason),
        };
        match self.born.get_mut(node) {
            Some(born) => *born = (*born).min(level),
            None => self.born.resize(node.saturating_add(1), level),
        }
        self.game.set_suffix_nags(node, &san.suffix, white);
        if let Some((text, indices)) = self.pending.take() {
            self.mark(&indices, node);
            let joined = match self.game.comment_before(node) {
                "" => text,
                before => format!("{before} {text}"),
            };
            self.game.set_comment_before(node, &joined);
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

    /// A move that could not be played ends its line; the first is the report's.
    fn stopped(&mut self, index: usize, text: &str, reason: GraftStopReason) {
        if self.stop.is_none() {
            self.stop = Some((index, text.to_owned(), PgnStopReason::from_graft(reason)));
        }
        // A comment waiting for the move stays where the line stopped.
        self.after_close = false;
        if let Some(pending) = self.pending.take() {
            self.comment_after(self.last.unwrap_or(self.cur), pending);
        }
        self.dead = Some(self.stack.len());
    }

    fn finish(mut self, parsed: Parsed) -> (Game, PgnReport) {
        // Variations never closed close at the end of the text.
        while !self.stack.is_empty() {
            self.close();
        }
        self.flush_after_close();
        if let Some(result) = parsed.result.as_deref()
            && self.game.tag("Result").is_none()
            && result != "*"
        {
            let _ = self.game.set_tag("Result", result);
        }
        let mut notes = parsed.notes;
        notes.sort_by_key(|(index, _)| *index);
        let warnings: Vec<PgnWarning> = notes
            .into_iter()
            .filter_map(|(index, kind)| Some(PgnWarning { kind, at: self.at.get(index).copied().flatten()? }))
            .take(MAX_PGN_WARNINGS)
            .collect();
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
        game.current = 0;
        while game.forward() {}
        (game, PgnReport { stopped, stop_reason, warnings })
    }
}

/// Add `text` to the comment after `node`, after what it already says.
fn append_comment(game: &mut Game, node: NodeId, text: String) {
    let joined = match game.comment(node) {
        "" => text,
        before => format!("{before} {text}"),
    };
    game.set_comment(node, &joined);
}

#[cfg(test)]
mod tests;
