//! The book's PGN with the book's text as its comments (plan "Carregar PGN do livro", R4).
//!
//! [`annotated_pgn`] writes every game of the reference, with all its lines and glyphs, and puts
//! the text the book prints between one linked move and the next in the comment after the first
//! ([`align`] links them); text read before the first move of another game (its heading, an
//! introduction) goes before that game's first move. The PGN is the source of the moves: a move
//! the reader played elsewhere (a conflict) is the PGN's (decision D1). In the comments, figurines
//! are letters (the one PGN writer writes them so, plan A9-F4); a diagram on a linked position is
//! `$220` on that move, and the "Position after:" cue of its caption is left out (the `$220` says
//! it), the caption's other words kept; line labels (`A)`, `B1)`) stay as printed. The page's
//! furniture, its printed number, its running heads and a diagram's own text, is no comment (plan
//! A9-L1b, A2-04).
//!
//! **Lines only the book has** (decision D2): a move the reader played that the PGN does not
//! have goes in as a variation from the position it was played from, when that position is one
//! of the last [`RECENT_ANCHORS`] linked, its first move commented "only in the book" in the book's
//! language. A move that can be placed nowhere stays text in the comment.
//!
//! Each game the book discusses gets the origin's tags from the first page that discusses it
//! (`PdfPage`, `PdfPageLabel`, `SourceTitle`); the games it never discusses are written as they
//! are, so no move of the PGN is lost. After them, the diagrams [`PgnPositions`] asks a game of
//! their own for (plan A9-F4: with a reference too), in the order of the book: by default those
//! linked to no move of the PGN.

use std::collections::{BTreeMap, HashMap, HashSet};

use pdfcraft_chess::game::Game as PgnGame;
use pdfcraft_chess::origin::PgnOrigin;
use pdfcraft_chess::pgn::WriteOptions;
use pdfcraft_chess::pgn_style::PgnStyle;
use pdfcraft_chess::san::Lang;

use super::{RECENT_ANCHORS, RefNode, ReferenceIndex, align, move_key};
use crate::doc::html::{Figurines, plain_text_with};
use crate::doc::markdown::{PgnBook, PgnPositions, SkippedPosition, position_game_of};
use crate::doc::pgn::GameWarning;
use crate::doc::{Block, ChessDoc, DiagramAt, Inline, MoveAt, MoveInline, MoveStatus};

/// What [`annotated_pgn`] wrote.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AnnotatedPgn {
    pub pgn: String,
    /// Games written: every game of the reference that could be read.
    pub games: usize,
    /// Moves given a comment from the book.
    pub commented: usize,
    /// Moves only the book has, added as variations.
    pub book_only: usize,
    /// Moves marked as shown in a diagram (`$220`).
    pub diagrams: usize,
    /// Comments the book's text made longer than a comment may be (`MAX_COMMENT_LEN` bytes), cut
    /// there (plan A9-F3).
    pub comments_cut: usize,
    /// Games with no moves added for diagrams ([`PgnPositions`]), after the reference's games.
    pub positions: usize,
    /// The diagrams [`PgnPositions`] asked a game for that got none, in the order of the book.
    pub skipped: Vec<SkippedPosition>,
    /// What writing left out of the diagrams' games, by game (counted from 0: the reference's
    /// games first, then the diagrams' in the order of the book).
    pub warnings: Vec<GameWarning>,
}

/// Text waiting for the move it follows.
#[derive(Default)]
struct Pending {
    text: String,
    /// It starts with a heading: what it introduces is what follows.
    heading: bool,
}

impl Pending {
    /// Take off a "Position after:" cue that ends the text: the caption of a diagram, which `$220`
    /// says. The word before the cue goes too (`Position`, `Posição`, `Stellung`), unless it ends
    /// a sentence.
    fn drop_caption_cue(&mut self) {
        if !super::super::games::after_cue(&self.text) {
            return;
        }
        let mut words: Vec<&str> = self.text.split_whitespace().collect();
        // The cue is one word, or two (`depois de`, `después de`).
        let one = words.last().is_some_and(|w| super::super::games::after_cue(w));
        words.truncate(words.len().saturating_sub(if one { 1 } else { 2 }));
        if words.last().is_some_and(|w| !w.ends_with(['.', '!', '?', ')', ']', ':', ';'])) {
            words.pop();
        }
        self.text = words.join(" ");
    }

    fn add(&mut self, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        if !self.text.is_empty() {
            self.text.push(' ');
        }
        self.text.push_str(text);
    }
}

/// Where the text read before move `next` goes: in the comment of the move read before it,
/// `current`, unless that is in another game and the text starts with a heading (or nothing was
/// read before): then before the next game's first move.
fn before(current: Option<RefNode>, next: RefNode, pending: &Pending) -> Option<RefNode> {
    match current {
        Some(c) if c.game == next.game || !pending.heading => Some(c),
        _ => Some(RefNode { game: next.game, node: 0 }),
    }
}

/// Put `pending`'s text in the comment of move `at`, after what it has.
fn flush(pending: &mut Pending, at: Option<RefNode>, notes: &mut BTreeMap<RefNode, String>) {
    pending.heading = false;
    let text = std::mem::take(&mut pending.text);
    let Some(at) = at.filter(|_| !text.is_empty()) else { return };
    let note = notes.entry(at).or_default();
    if !note.is_empty() {
        note.push(' ');
    }
    note.push_str(&text);
}

/// The reference's games written with the book's text, then the diagrams' games `positions` asks
/// for (see the module documentation).
pub fn annotated_pgn(doc: &ChessDoc, index: &ReferenceIndex, book: &PgnBook, positions: PgnPositions, style: PgnStyle) -> AnnotatedPgn {
    let alignment = align(doc, index);
    let links: HashMap<MoveAt, RefNode> = alignment.moves.iter().filter_map(|l| Some((l.at, l.node?))).collect();
    let shown: HashMap<DiagramAt, RefNode> = alignment.diagrams.iter().filter_map(|d| Some((d.at, d.node?))).collect();
    let page_game: HashMap<u32, usize> = alignment.pages.iter().map(|p| (p.page, p.game)).collect();
    // Each game read again from its text: the index keeps none (plan A9-I3b).
    let mut games: Vec<Option<PgnGame>> = (0..index.games().len()).map(|k| index.game(k)).collect();
    let mut notes: BTreeMap<RefNode, String> = BTreeMap::new();
    let mut marked: HashSet<RefNode> = HashSet::new();
    // Each move only the book has, and the move it was played from.
    let mut book_only: HashMap<RefNode, RefNode> = HashMap::new();
    let mut first_page: HashMap<usize, (u32, Option<Lang>)> = HashMap::new();
    let mut recent: Vec<RefNode> = Vec::new();
    let mut current: Option<RefNode> = None;
    let mut pending = Pending::default();
    let mut made = AnnotatedPgn::default();

    for page in &doc.pages {
        let lang = page.lang.lang();
        // Text before any linked move of the page's game goes before its first move.
        if current.is_none() {
            current = page_game.get(&page.index).map(|&game| RefNode { game, node: 0 });
        }
        // The book's text: its number, running heads and a diagram's own text are no comment
        // (plan A9-L1b, A2-04).
        for (b, block) in page.book_blocks() {
            match block {
                Block::Diagram(d) => {
                    if let Some(&at) = shown.get(&DiagramAt::on(page.index, d.number)) {
                        let to = before(current, at, &pending);
                        flush(&mut pending, to, &mut notes);
                        marked.insert(at);
                        current = Some(at);
                    }
                }
                // A heading starts what follows: the text before it stays with the move it follows.
                Block::Heading { inlines, .. } => {
                    flush(&mut pending, current, &mut notes);
                    pending.heading = true;
                    pending.add(&plain_text_with(inlines, Figurines::Letters(Lang::En)));
                }
                Block::Paragraph { inlines, .. } => {
                    let mut from = 0usize;
                    for (i, inline) in inlines.iter().enumerate() {
                        let Inline::Move(mv) = inline else { continue };
                        let at = MoveAt::new(page.index, b, i);
                        let node = match links.get(&at) {
                            Some(&node) => Some(node),
                            None => book_line(&mut games, &recent, current, mv).map(|(parent, n, new)| {
                                if new {
                                    book_only.insert(n, parent);
                                }
                                n
                            }),
                        };
                        let Some(node) = node else { continue };
                        if let Some(text) = inlines.get(from..i) {
                            pending.add(&plain_text_with(text, Figurines::Letters(Lang::En)));
                        }
                        from = i.saturating_add(1);
                        // A move repeated just after itself or its diagram is a caption naming it.
                        if current == Some(node) || marked.contains(&node) {
                            pending.drop_caption_cue();
                        }
                        let to = before(current, node, &pending);
                        flush(&mut pending, to, &mut notes);
                        current = Some(node);
                        first_page.entry(node.game).or_insert((page.index, lang));
                        recent.retain(|r| *r != node);
                        recent.insert(0, node);
                        recent.truncate(RECENT_ANCHORS);
                    }
                    if let Some(text) = inlines.get(from..) {
                        pending.add(&plain_text_with(text, Figurines::Letters(Lang::En)));
                    }
                }
            }
        }
    }
    flush(&mut pending, current, &mut notes);
    // The first move of each line only the book has says so.
    for (&at, from) in &book_only {
        if book_only.contains_key(from) {
            continue;
        }
        let lang = first_page.get(&at.game).and_then(|(_, l)| *l).unwrap_or(Lang::En);
        let note = notes.entry(at).or_default();
        let word = pdfcraft_chess::origin::only_in_the_book(lang);
        *note = if note.is_empty() { word.to_owned() } else { format!("{word}. {note}") };
    }
    made.book_only = book_only.len();
    for (at, note) in &notes {
        let Some(game) = games.get_mut(at.game).and_then(Option::as_mut) else { continue };
        let old = game.comment(at.node).trim().to_owned();
        let text = if old.is_empty() { note.clone() } else { format!("{old} {note}") };
        if game.set_comment(at.node, &text).is_err() {
            made.comments_cut = made.comments_cut.saturating_add(1);
        }
        made.commented = made.commented.saturating_add(usize::from(at.node > 0));
    }
    for at in &marked {
        let Some(game) = games.get_mut(at.game).and_then(Option::as_mut) else { continue };
        if at.node > 0 && !game.nags(at.node).iter().any(|&c| pdfcraft_chess::nag::is_diagram(c)) {
            game.toggle_nag(at.node, pdfcraft_chess::nag::DIAGRAM);
            made.diagrams = made.diagrams.saturating_add(1);
        }
    }
    let options = WriteOptions { style, letters: true };
    let mut out = Vec::new();
    for (g, game) in games.iter_mut().enumerate() {
        let Some(game) = game.as_mut() else { continue };
        if let Some(&(page, lang)) = first_page.get(&g) {
            let origin = PgnOrigin {
                book_title: book.title.clone(),
                page_index: page,
                page_label: usize::try_from(page).ok().and_then(|i| book.labels.get(i)).cloned(),
                diagram: None,
                of: None,
                puzzle: None,
                lang,
            };
            for (name, value) in origin.tags() {
                // A tag the game cannot take (too many, a bad name) is left out; the moves are what matter.
                let _ = game.set_tag(&name, &value);
            }
        }
        out.push(pdfcraft_chess::pgn::write(game, options));
        made.games = made.games.saturating_add(1);
    }
    // The diagrams' own games: by default those no move of the PGN is linked to.
    let mut position_games = Vec::new();
    for page in &doc.pages {
        for block in &page.blocks {
            let Block::Diagram(d) = block else { continue };
            let wanted = match positions {
                PgnPositions::All => true,
                PgnPositions::Unused => !shown.contains_key(&DiagramAt::on(page.index, d.number)),
                PgnPositions::None => false,
            };
            if wanted {
                match position_game_of(doc, book, page.index, d) {
                    Ok(game) => position_games.push(game),
                    Err(skipped) => made.skipped.push(skipped),
                }
            }
        }
    }
    let written = crate::doc::pgn::write_report(position_games.iter(), style);
    let left_out = written.warnings.iter().filter(|w| w.warning.kind == pdfcraft_chess::game::PgnWarningKind::GameLeftOut).count();
    made.positions = position_games.len().saturating_sub(left_out);
    made.warnings = written.warnings.into_iter().map(|w| GameWarning { game: w.game.saturating_add(made.games), ..w }).collect();
    if !written.pgn.is_empty() {
        out.push(written.pgn);
    }
    made.pgn = out.join("\n");
    made
}

/// Place move `mv`, which [`align`] did not link, from the position the reader played it from: the
/// current move's, else one of the `recent` linked moves'. A move the game has there already is
/// taken; another is added as a variation. Returns the move it was played from, the move, and
/// whether it was added. `None` when the reader did not play it or no such position is found.
fn book_line(games: &mut [Option<PgnGame>], recent: &[RefNode], current: Option<RefNode>, mv: &MoveInline) -> Option<(RefNode, RefNode, bool)> {
    if mv.status != MoveStatus::Valid {
        return None;
    }
    let want = move_key(mv.fen.as_deref())?;
    for from in current.into_iter().chain(recent.iter().copied()) {
        let Some(game) = games.get_mut(from.game).and_then(Option::as_mut) else { continue };
        let Some(fen) = game.fen_at(from.node) else { continue };
        let Ok(mut trial) = PgnGame::from_fen(&fen.to_string()) else { continue };
        if trial.play_san(&mv.san).is_err() || trial.fen_at(trial.current()).map(|f| f.book_key()) != Some(want) {
            continue;
        }
        let before = game.replies(from.node).len();
        game.go_to_node(from.node);
        if game.play_san(&mv.san).is_err() {
            return None;
        }
        let added = game.replies(from.node).len() > before;
        return Some((from, RefNode { game: from.game, node: game.current() }, added));
    }
    None
}

#[cfg(test)]
mod tests;
