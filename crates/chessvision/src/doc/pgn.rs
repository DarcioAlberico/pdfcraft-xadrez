//! PGN: write the games of a [`super::ChessDoc`], and read back what is written.
//!
//! [`write_game`] writes PGN's export format: the seven-tag roster, `SetUp` and `FEN` when the game
//! does not start from the initial position, the other tags, then the movetext with move numbers
//! (`12. Nf3`, `12... Nf6` after a comment or a variation), NAGs as `$n`, comments in braces and
//! variations in brackets, wrapped at [`LINE_WIDTH`] columns and ended by the result.
//!
//! [`parse`] reads that back, replaying every move with `cozy-chess`, so `parse(write(g)) == g` for
//! every game the builder makes. It also reads the common variants of the format (no space after
//! a move number, `;` and `%` comments, annotation suffixes such as `!?`), but it is a reader for
//! round trips, not for every PGN in the wild: an illegal move is an error, not a repair.
//!
//! Comments are normalised the same way on both sides (`append_comment`: single spaces, `}`
//! written `)`, at most `MAX_COMMENT_CHARS`), which is what makes the round trip exact.

use cozy_chess::Board;
use pdfcraft_chess::nag::nags_from_suffix;
use pdfcraft_chess::pgn_style::PgnStyle;
use pdfcraft_chess::san::{Lang, San};

use super::games::{GameResult, GameScore, MAX_GAME_NODES, MAX_NAGS, MAX_VARIATION_DEPTH, PlayError, Roster, Tag, append_comment, number_of};

/// Longest movetext line written, in characters (a longer token stands alone).
pub const LINE_WIDTH: usize = 79;
/// Largest PGN text [`parse`] reads.
pub const MAX_PGN_BYTES: usize = 16 << 20;
/// Most games [`parse`] reads.
pub const MAX_PGN_GAMES: usize = 100_000;
/// Most tags a game may have.
pub const MAX_TAGS: usize = 64;
/// Longest tag value, in characters.
pub const MAX_TAG_CHARS: usize = 1024;

/// Why a PGN text could not be read.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PgnError {
    #[error("the PGN is {0} bytes, more than the {MAX_PGN_BYTES} read")]
    TooLarge(usize),
    #[error("the PGN has more than {MAX_PGN_GAMES} games")]
    TooManyGames,
    #[error("line {line}: {what}")]
    Syntax { line: usize, what: String },
    #[error("line {line}: game {game}: '{san}' is not a legal move here")]
    IllegalMove { line: usize, game: usize, san: String },
    #[error("line {line}: game {game}: '{san}' fits more than one legal move")]
    AmbiguousMove { line: usize, game: usize, san: String },
    #[error("line {line}: game {game}: the FEN tag is not a legal position")]
    BadFen { line: usize, game: usize },
    #[error("line {line}: game {game}: more than {MAX_GAME_NODES} moves or {MAX_VARIATION_DEPTH} nested variations")]
    GameTooLarge { line: usize, game: usize },
}

// ---------------------------------------------------------------- writing

/// Every game, one after the other, separated by a blank line.
pub fn write<'a>(games: impl IntoIterator<Item = &'a GameScore>) -> String {
    write_with(games, PgnStyle::FULL)
}

/// [`write`] without what `style` leaves out of the moves (plan A8 V4).
pub fn write_with<'a>(games: impl IntoIterator<Item = &'a GameScore>, style: PgnStyle) -> String {
    let mut out = String::new();
    for game in games {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&write_game_with(game, style));
    }
    out
}

/// One game: tags, a blank line, the movetext, a newline.
pub fn write_game(game: &GameScore) -> String {
    write_game_with(game, PgnStyle::FULL)
}

/// [`write_game`] without what `style` leaves out of the moves: the variations past its depth,
/// the comments (the game's first one too), the glyphs. The tags are all written.
pub fn write_game_with(game: &GameScore, style: PgnStyle) -> String {
    let mut out = String::new();
    let r = &game.roster;
    for (name, value) in [("Event", &r.event), ("Site", &r.site), ("Date", &r.date), ("Round", &r.round), ("White", &r.white), ("Black", &r.black)] {
        tag(&mut out, name, value);
    }
    tag(&mut out, "Result", game.result.as_pgn());
    if let Some(fen) = &game.start_fen {
        tag(&mut out, "SetUp", "1");
        tag(&mut out, "FEN", fen);
    }
    for t in &game.extra_tags {
        if !is_reserved(&t.name) {
            tag(&mut out, &tag_name(&t.name), &t.value);
        }
    }
    out.push('\n');
    let mut tokens: Vec<Tok> = Vec::new();
    if style.comments {
        push_comment(&mut tokens, &game.comment);
    }
    let mut seen = vec![false; game.nodes.len()];
    if let Some((&first, alternatives)) = game.roots.split_first() {
        write_line(game, first, alternatives, 0, style, &mut tokens, &mut seen);
    }
    tokens.push(Tok::Word(game.result.as_pgn().to_owned()));
    wrap(&tokens, &mut out);
    out
}

/// Tags the writer writes from the game's own fields.
fn is_reserved(name: &str) -> bool {
    matches!(name, "Event" | "Site" | "Date" | "Round" | "White" | "Black" | "Result" | "SetUp" | "FEN")
}

/// A tag name as PGN allows it: letters, digits and `_`, starting with a letter.
fn tag_name(name: &str) -> String {
    let mut s: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect();
    if !s.starts_with(|c: char| c.is_ascii_alphabetic()) {
        s.insert(0, 'X');
    }
    s
}

fn tag(out: &mut String, name: &str, value: &str) {
    out.push('[');
    out.push_str(name);
    out.push_str(" \"");
    for c in value.chars().take(MAX_TAG_CHARS) {
        match c {
            '\\' | '"' => {
                out.push('\\');
                out.push(c);
            }
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out.push_str("\"]\n");
}

/// A comment as tokens, one per word, so a long one wraps.
fn push_comment(tokens: &mut Vec<Tok>, text: &str) {
    let mut normalised = String::new();
    append_comment(&mut normalised, text);
    if normalised.is_empty() {
        return;
    }
    let words: Vec<&str> = normalised.split(' ').collect();
    let last = words.len().saturating_sub(1);
    for (i, w) in words.iter().enumerate() {
        let mut t = String::new();
        if i == 0 {
            t.push('{');
        }
        t.push_str(w);
        if i == last {
            t.push('}');
        }
        tokens.push(Tok::Word(t));
    }
}

/// A SAN as a single PGN token, whatever a hand-edited document holds (empty when nothing in it
/// is SAN).
fn san_token(san: &str) -> String {
    san.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '#' | '=' | '-')).collect()
}

/// The move as PGN writes it. A SAN with nothing PGN can write is named again from the positions
/// before and after the move, never written `--` (PGN's null move, another move altogether, plan
/// A8 P8a); `None` when no legal move joins the two positions.
fn move_word(game: &GameScore, node: &super::games::Node) -> Option<String> {
    let t = san_token(&node.san);
    if !t.is_empty() {
        return Some(t);
    }
    let before = game.board_after(node.parent)?;
    let after = Board::from_fen(node.fen.trim(), false).ok()?;
    let key = |b: &Board| b.to_string().split_whitespace().take(4).collect::<Vec<_>>().join(" ");
    let want = key(&after);
    let mut found = None;
    before.generate_moves(|moves| {
        for mv in moves {
            let mut b = before.clone();
            b.play_unchecked(mv);
            if key(&b) == want {
                found = Some(mv);
                return true;
            }
        }
        false
    });
    San::from_move(&before, found?).ok().map(|s| s.to_english())
}

fn write_line(game: &GameScore, start: usize, alternatives: &[usize], depth: usize, style: PgnStyle, tokens: &mut Vec<Tok>, seen: &mut [bool]) {
    let (mut current, mut alternatives) = (start, alternatives);
    let mut number_black = true;
    loop {
        match seen.get_mut(current) {
            Some(s) if !*s => *s = true,
            _ => return,
        }
        let Some(node) = game.nodes.get(current) else { return };
        let Some(word) = move_word(game, node) else {
            // The line cannot go on past a move that cannot be named: say so where it stops.
            if style.comments {
                push_comment(tokens, "(a move that cannot be written: no legal move reaches the position after it)");
            }
            return;
        };
        if style.comments && !node.comment_before.is_empty() {
            push_comment(tokens, &node.comment_before);
            number_black = true;
        }
        let number = number_of(node.ply);
        if !number.black {
            tokens.push(Tok::Word(format!("{}.", number.number)));
        } else if number_black {
            tokens.push(Tok::Word(format!("{}...", number.number)));
        }
        tokens.push(Tok::Word(word));
        if style.nags {
            tokens.extend(node.nags.iter().map(|n| Tok::Word(format!("${n}"))));
        }
        number_black = false;
        if style.comments && !node.comment.is_empty() {
            push_comment(tokens, &node.comment);
            number_black = true;
        }
        if depth < MAX_VARIATION_DEPTH && style.keeps(depth.saturating_add(1)) {
            for &alt in alternatives {
                tokens.push(Tok::Open);
                write_line(game, alt, &[], depth + 1, style, tokens, seen);
                tokens.push(Tok::Close);
                number_black = true;
            }
        }
        let Some((&next, rest)) = node.children.split_first() else { return };
        (current, alternatives) = (next, rest);
    }
}

/// A piece of movetext: a variation's brackets, or a word (move, number, NAG, comment word).
enum Tok {
    Open,
    Close,
    Word(String),
}

/// Join tokens into lines of at most [`LINE_WIDTH`]: one space between tokens, none after a
/// variation's `(` or before its `)`, and never a line that starts with `%` (PGN's escape).
fn wrap(tokens: &[Tok], out: &mut String) {
    let mut line = String::new();
    let mut width = 0usize;
    let mut after_open = false;
    for tok in tokens {
        let t = match tok {
            Tok::Open => "(",
            Tok::Close => ")",
            Tok::Word(w) => w.as_str(),
        };
        let tw = t.chars().count();
        let space = !line.is_empty() && !after_open && !matches!(tok, Tok::Close);
        after_open = matches!(tok, Tok::Open);
        let fits = width.saturating_add(usize::from(space)).saturating_add(tw) <= LINE_WIDTH;
        if !line.is_empty() && !fits && !t.starts_with('%') {
            out.push_str(&line);
            out.push('\n');
            line.clear();
            width = 0;
        } else if space {
            line.push(' ');
            width += 1;
        }
        line.push_str(t);
        width = width.saturating_add(tw);
    }
    if !line.is_empty() {
        out.push_str(&line);
        out.push('\n');
    }
}

// ---------------------------------------------------------------- reading

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Tag(String, String),
    Comment(String),
    Open,
    Close,
    Nag(u16),
    San(String),
    Result(GameResult),
}

struct Lexer<'a> {
    text: &'a str,
    pos: usize,
    line: usize,
}

impl Lexer<'_> {
    fn peek(&self) -> Option<char> {
        self.text.get(self.pos..)?.chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        if c == '\n' {
            self.line += 1;
        }
        Some(c)
    }

    fn at_line_start(&self) -> bool {
        self.pos == 0 || self.text.get(..self.pos).is_some_and(|s| s.ends_with('\n'))
    }

    fn err(&self, what: &str) -> PgnError {
        PgnError::Syntax { line: self.line, what: what.to_owned() }
    }

    fn skip_line(&mut self) {
        while let Some(c) = self.bump() {
            if c == '\n' {
                break;
            }
        }
    }

    fn next_token(&mut self) -> Result<Option<Token>, PgnError> {
        loop {
            if self.peek() == Some('%') && self.at_line_start() {
                self.skip_line();
                continue;
            }
            let Some(c) = self.peek() else { return Ok(None) };
            if c.is_whitespace() {
                self.bump();
                continue;
            }
            return match c {
                ';' => {
                    self.skip_line();
                    continue;
                }
                '[' => self.tag().map(Some),
                '{' => {
                    self.bump();
                    let mut s = String::new();
                    loop {
                        match self.bump() {
                            Some('}') => break,
                            Some(c) => s.push(c),
                            None => return Err(self.err("a comment is not closed")),
                        }
                    }
                    Ok(Some(Token::Comment(s)))
                }
                '(' => {
                    self.bump();
                    Ok(Some(Token::Open))
                }
                ')' => {
                    self.bump();
                    Ok(Some(Token::Close))
                }
                '$' => {
                    self.bump();
                    let digits = self.symbol();
                    digits.parse::<u16>().map(|n| Some(Token::Nag(n))).map_err(|_| self.err("a NAG is not a number"))
                }
                _ => {
                    let sym = self.symbol();
                    if sym.is_empty() {
                        self.bump();
                        return Err(self.err(&format!("unexpected '{c}'")));
                    }
                    match classify(&sym) {
                        Some(t) => Ok(Some(t)),
                        None => continue,
                    }
                }
            };
        }
    }

    /// The characters up to whitespace or a delimiter.
    fn symbol(&mut self) -> String {
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if c.is_whitespace() || matches!(c, '(' | ')' | '{' | '}' | '[' | ']' | ';' | '$' | '"') {
                break;
            }
            s.push(c);
            self.bump();
            if s.len() > 256 {
                break;
            }
        }
        s
    }
}

/// A symbol as a move number (skipped: `None`), a result, or a move with an optional number glued on.
fn classify(sym: &str) -> Option<Token> {
    if let Some(r) = GameResult::from_pgn(sym) {
        return Some(Token::Result(r));
    }
    let rest = sym.trim_start_matches(|c: char| c.is_ascii_digit());
    let after_dots = rest.trim_start_matches('.');
    if rest.len() < sym.len() && after_dots.len() < rest.len() {
        // `12.` or `12...`, maybe with the move glued on.
        return (!after_dots.is_empty()).then(|| Token::San(after_dots.to_owned()));
    }
    if sym.chars().all(|c| c == '.') {
        return None;
    }
    Some(Token::San(sym.to_owned()))
}

impl Lexer<'_> {
    fn tag(&mut self) -> Result<Token, PgnError> {
        self.bump();
        while self.peek().is_some_and(char::is_whitespace) {
            self.bump();
        }
        let mut name = String::new();
        while let Some(c) = self.peek().filter(|c| c.is_ascii_alphanumeric() || *c == '_') {
            name.push(c);
            self.bump();
            if name.len() > 256 {
                return Err(self.err("a tag name is too long"));
            }
        }
        while self.peek().is_some_and(char::is_whitespace) {
            self.bump();
        }
        if name.is_empty() || self.bump() != Some('"') {
            return Err(self.err("a tag is not `[Name \"value\"]`"));
        }
        let mut value = String::new();
        let mut chars = 0usize;
        loop {
            let c = match self.bump() {
                Some('"') => break,
                Some('\\') => self.bump().ok_or_else(|| self.err("a tag value is not closed"))?,
                Some(c) => c,
                None => return Err(self.err("a tag value is not closed")),
            };
            chars += 1;
            if chars > MAX_TAG_CHARS {
                return Err(self.err("a tag value is too long"));
            }
            value.push(c);
        }
        while self.peek().is_some_and(char::is_whitespace) {
            self.bump();
        }
        if self.bump() != Some(']') {
            return Err(self.err("a tag is not closed"));
        }
        Ok(Token::Tag(name, value))
    }
}

/// One line being read: the last move in it, and a comment waiting for its first move.
#[derive(Default)]
struct PLine {
    cursor: Option<usize>,
    last: Option<usize>,
    pending: String,
}

/// A game being read.
struct Reading {
    score: GameScore,
    lines: Vec<PLine>,
    result_tag: Option<GameResult>,
    tags: usize,
    movetext: bool,
}

impl Reading {
    fn new() -> Reading {
        Reading { score: GameScore::new(None), lines: vec![PLine::default()], result_tag: None, tags: 0, movetext: false }
    }

    fn comment_target(&mut self) -> &mut String {
        let depth = self.lines.len().saturating_sub(1);
        let last = self.lines.last().and_then(|l| l.last);
        match last {
            Some(n) if n < self.score.nodes.len() => match self.score.nodes.get_mut(n) {
                Some(node) => &mut node.comment,
                None => &mut self.score.comment,
            },
            _ if depth == 0 => &mut self.score.comment,
            _ => match self.lines.last_mut() {
                Some(line) => &mut line.pending,
                None => &mut self.score.comment,
            },
        }
    }

    fn close_variation(&mut self) {
        if let Some(line) = self.lines.pop()
            && !line.pending.is_empty()
        {
            append_comment(self.comment_target(), &format!("({})", line.pending));
        }
    }
}

/// Read every game. Illegal moves, unclosed variations and malformed tags are errors.
pub fn parse(text: &str) -> Result<Vec<GameScore>, PgnError> {
    if text.len() > MAX_PGN_BYTES {
        return Err(PgnError::TooLarge(text.len()));
    }
    let mut lexer = Lexer { text, pos: 0, line: 1 };
    let mut games: Vec<GameScore> = Vec::new();
    let mut cur: Option<Reading> = None;
    while let Some(token) = lexer.next_token()? {
        let line = lexer.line;
        if matches!(token, Token::Tag(..)) && cur.as_ref().is_some_and(|g| g.movetext) {
            let done = cur.take().map(|g| finish(g, line));
            push_game(&mut games, done.transpose()?)?;
        }
        let game = cur.get_or_insert_with(Reading::new);
        let index = games.len();
        match token {
            Token::Tag(name, value) => {
                game.tags += 1;
                if game.tags > MAX_TAGS {
                    return Err(PgnError::Syntax { line, what: format!("more than {MAX_TAGS} tags") });
                }
                apply_tag(game, name, value, line, index)?;
            }
            Token::Result(result) => {
                if game.lines.len() > 1 {
                    return Err(PgnError::Syntax { line, what: "a variation is not closed".to_owned() });
                }
                game.score.result = result;
                game.result_tag = Some(result);
                let done = cur.take().map(|g| finish(g, line));
                push_game(&mut games, done.transpose()?)?;
            }
            Token::Comment(c) => {
                game.movetext = true;
                append_comment(game.comment_target(), &c);
            }
            Token::Nag(code) => {
                game.movetext = true;
                let last = game.lines.last().and_then(|l| l.last);
                let node =
                    last.and_then(|n| game.score.nodes.get_mut(n)).ok_or(PgnError::Syntax { line, what: "a NAG before any move".to_owned() })?;
                if !node.nags.contains(&code) {
                    if node.nags.len() >= MAX_NAGS {
                        return Err(PgnError::Syntax { line, what: format!("more than {MAX_NAGS} NAGs on one move") });
                    }
                    node.nags.push(code);
                }
            }
            Token::Open => {
                game.movetext = true;
                if game.lines.len() > MAX_VARIATION_DEPTH {
                    return Err(PgnError::GameTooLarge { line, game: index });
                }
                let last = game.lines.last().and_then(|l| l.last).ok_or(PgnError::Syntax { line, what: "a variation before any move".to_owned() })?;
                let cursor = game.score.nodes.get(last).and_then(|n| n.parent);
                game.lines.push(PLine { cursor, last: None, pending: String::new() });
            }
            Token::Close => {
                game.movetext = true;
                if game.lines.len() <= 1 {
                    return Err(PgnError::Syntax { line, what: "a `)` with no variation open".to_owned() });
                }
                game.close_variation();
            }
            Token::San(sym) => {
                game.movetext = true;
                play(game, &sym, line, index)?;
            }
        }
    }
    if let Some(game) = cur.filter(|g| g.movetext || g.tags > 0) {
        let line = lexer.line;
        if game.lines.len() > 1 {
            return Err(PgnError::Syntax { line, what: "a variation is not closed".to_owned() });
        }
        push_game(&mut games, Some(finish(game, line)?))?;
    }
    Ok(games)
}

fn push_game(games: &mut Vec<GameScore>, game: Option<GameScore>) -> Result<(), PgnError> {
    if let Some(g) = game {
        if games.len() >= MAX_PGN_GAMES {
            return Err(PgnError::TooManyGames);
        }
        games.push(g);
    }
    Ok(())
}

fn finish(mut game: Reading, line: usize) -> Result<GameScore, PgnError> {
    if game.lines.len() > 1 {
        return Err(PgnError::Syntax { line, what: "a variation is not closed".to_owned() });
    }
    if let Some(r) = game.result_tag {
        game.score.result = r;
    }
    Ok(game.score)
}

fn apply_tag(game: &mut Reading, name: String, value: String, line: usize, index: usize) -> Result<(), PgnError> {
    let r: &mut Roster = &mut game.score.roster;
    match name.as_str() {
        "Event" => r.event = value,
        "Site" => r.site = value,
        "Date" => r.date = value,
        "Round" => r.round = value,
        "White" => r.white = value,
        "Black" => r.black = value,
        "Result" => game.result_tag = GameResult::from_pgn(value.trim()),
        "SetUp" => {}
        "FEN" => {
            if game.movetext || Board::from_fen(value.trim(), false).is_err() {
                return Err(PgnError::BadFen { line, game: index });
            }
            game.score.start_fen = Some(value);
        }
        _ => game.score.extra_tags.push(Tag { name, value }),
    }
    Ok(())
}

fn play(game: &mut Reading, sym: &str, line: usize, index: usize) -> Result<(), PgnError> {
    let mut san = San::read(sym, Lang::En).map_err(|_| PgnError::Syntax { line, what: format!("'{sym}' is not a move") })?;
    let annotations: String = san.suffix.chars().filter(|c| !matches!(c, '+' | '#')).collect();
    san.suffix.retain(|c| matches!(c, '+' | '#'));
    let cursor = game.lines.last().map(|l| l.cursor).unwrap_or_default();
    let white_moved = game.score.board_after(cursor).is_some_and(|b| b.side_to_move() == cozy_chess::Color::White);
    let node = match game.score.play(cursor, &san, false) {
        Ok(n) => n,
        Err(PlayError::Ambiguous(_)) => return Err(PgnError::AmbiguousMove { line, game: index, san: sym.to_owned() }),
        Err(PlayError::TooLarge) => return Err(PgnError::GameTooLarge { line, game: index }),
        Err(PlayError::Illegal(_) | PlayError::NoPosition) => return Err(PgnError::IllegalMove { line, game: index, san: sym.to_owned() }),
    };
    if let Some(l) = game.lines.last_mut() {
        let pending = std::mem::take(&mut l.pending);
        l.cursor = Some(node);
        l.last = Some(node);
        if !pending.is_empty() {
            game.score.comment_before(node, &pending);
        }
    }
    if let Some(n) = game.score.nodes.get_mut(node) {
        for code in nags_from_suffix(&annotations, white_moved) {
            if !n.nags.contains(&code) && n.nags.len() < MAX_NAGS {
                n.nags.push(code);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
