//! The Library searched (plan A9-N15b, finding A8-08): a position (its key, the side to move
//! compared unless asked not to), a move (its SAN), a game (its players, its event, its opening's
//! code) and, with plan A9-N16a, a material balance and a pawn structure, looked for in every
//! book's index, never in a page.
//!
//! **Each index read once, one at a time** ([`Library::find`]): the books indexed, in the
//! catalog's order, each index inflated as it is read and dropped before the next; what it found
//! is kept only once the whole file checked out (an index that does not is skipped and said, never
//! served). Memory: the hits listed, at most `max` ([`MAX_RESULTS`] at most), and one book's while
//! its index is read.
//!
//! **What a hit is.** Asked for a position, a move or a pattern, a hit is a position of a book: the
//! page as the book numbers it, the diagram or the move (as numbered, with its SAN) or the move of
//! the book's PGN, and its game. Asked only for a player, an event or an opening, a hit is a game.
//! A position's game must match the game asked for too.

use std::collections::HashSet;

use pdfcraft_chess::fen::BookKey;
use pdfcraft_chess::pattern::{PawnQuery, Query};
use pdfcraft_chess::placement::Color;

use super::index::{self, IndexMeta, IndexedGame, IndexedPosition, Place, ReadError};
use super::{Library, State};

/// The most hits one search lists.
pub const MAX_RESULTS: usize = 1000;
/// Longest text looked for (a player, an event, an opening's code, a SAN), in characters.
pub const MAX_TEXT_CHARS: usize = 120;

/// What [`Library::find`] looks for. Each part given must hold; none given finds nothing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FindQuery {
    /// The position: its pieces, and its side to move unless `any_side` (or a diagram's side was
    /// assumed: [`BookKey::matches`]).
    pub position: Option<BookKey>,
    pub any_side: bool,
    /// The move that reached a position, by its SAN (`Bxh7+`): the same move, castling with
    /// letters or digits, a check or mate mark held to when asked for, annotation signs left out.
    pub san: Option<String>,
    /// A player of the game (White or Black), its event, and its opening's code from the start
    /// (`B9` finds `B90`): each a part of the text, in any case.
    pub player: Option<String>,
    pub event: Option<String>,
    pub eco: Option<String>,
    /// The material and the pawns (plan A9-N16a).
    pub pattern: Query,
}

impl FindQuery {
    /// Whether it asks for a position (a key, a move or a pattern), not only for games.
    pub fn asks_positions(&self) -> bool {
        self.position.is_some() || self.san.is_some() || !self.pattern.is_empty()
    }

    /// Whether it asks for a game.
    pub fn asks_games(&self) -> bool {
        self.player.is_some() || self.event.is_some() || self.eco.is_some()
    }

    /// Whether it asks for anything.
    pub fn is_empty(&self) -> bool {
        !self.asks_positions() && !self.asks_games()
    }

    /// Whether game `g` is the game asked for (every game when none is asked).
    fn game_matches(&self, g: &IndexedGame) -> bool {
        let has = |text: &str, part: &str| text.to_lowercase().contains(&part.to_lowercase());
        self.player.as_deref().is_none_or(|p| has(&g.white, p) || has(&g.black, p))
            && self.event.as_deref().is_none_or(|e| has(&g.event, e))
            && self.eco.as_deref().is_none_or(|e| g.eco.as_deref().is_some_and(|c| c.trim().to_uppercase().starts_with(&e.trim().to_uppercase())))
    }

    /// Whether position `p` holds the pattern asked for (as [`Query::matches`] has it, on the
    /// material and the marks the index keeps).
    pub(super) fn pattern_matches(&self, p: &IndexedPosition) -> bool {
        let q = &self.pattern;
        let pawns = |pq: &PawnQuery| {
            if pq.is_empty() {
                return true;
            }
            let held = |shift: u8| {
                let bits = p.pawns >> shift;
                (!pq.doubled || bits & 1 != 0) && (!pq.isolated || bits & 2 != 0) && (!pq.passed || bits & 4 != 0)
            };
            match pq.side {
                Some(Color::White) => held(0),
                Some(Color::Black) => held(3),
                None => held(0) || held(3),
            }
        };
        let Some(m) = q.material else { return pawns(&q.pawns) };
        let swapped = PawnQuery { side: q.pawns.side.map(Color::opponent), ..q.pawns };
        (p.has_material(&m, false) && pawns(&q.pawns)) || (q.either_side && p.has_material(&m.mirrored(), false) && pawns(&swapped))
    }
}

/// A SAN as looked for: without annotation signs, castling with letters, and its check or mate
/// mark apart.
fn plain_san(san: &str) -> (String, Option<char>) {
    let san = san.trim().trim_end_matches(['!', '?']);
    let mark = san.chars().last().filter(|c| *c == '+' || *c == '#');
    let bare = san.trim_end_matches(['+', '#']);
    let bare = match bare {
        "0-0" | "o-o" => "O-O",
        "0-0-0" | "o-o-o" => "O-O-O",
        other => other,
    };
    (bare.to_owned(), mark)
}

/// A position found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    /// The page, from 0, and as the book numbers it; `None` for a move of the book's PGN that says
    /// none.
    pub page: Option<u32>,
    pub label: Option<String>,
    pub place: Place,
    /// The move that reached it, as numbered (`12.`, `12...`) with its SAN; none for a diagram.
    pub number: u16,
    pub black: bool,
    pub san: Option<String>,
    /// Its game, in [`BookHits::games`].
    pub game: Option<u32>,
    pub side_assumed: bool,
    pub main_line: bool,
}

/// What one book holds of what was asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BookHits {
    pub hash: String,
    pub title: String,
    pub paths: Vec<String>,
    /// The positions found, the first ones in the book's order, and how many there are.
    pub hits: Vec<Hit>,
    pub total: usize,
    /// The games: those found when only games are asked for (in the book's order), else those
    /// the hits listed are in; by their number in the index.
    pub games: Vec<(u32, IndexedGame)>,
    /// When only games are asked for: how many there are.
    pub games_total: usize,
}

/// What [`Library::find`] found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Found {
    /// The books with anything found, in the catalog's order.
    pub books: Vec<BookHits>,
    /// Hits (positions, or games when only games are asked for) in all, and those listed.
    pub total: usize,
    pub listed: usize,
    /// Indexes read, each once.
    pub indexes_read: usize,
    /// Books the catalog says are indexed whose index was not there or did not check out: their
    /// hashes, and why.
    pub unread: Vec<(String, String)>,
    /// Books of the catalog not indexed (not read yet, skipped, in the cloud): not looked in.
    pub not_indexed: usize,
}

impl Library {
    /// The books' positions or games `query` asks for, at most `max` listed (1 to
    /// [`MAX_RESULTS`]), all counted; each index read once, one at a time (see the module
    /// documentation). Nothing is found for an empty query.
    pub fn find(&self, query: &FindQuery, max: usize) -> Found {
        let max = max.clamp(1, MAX_RESULTS);
        let mut out = Found::default();
        if query.is_empty() {
            return out;
        }
        let wanted_san = query.san.as_deref().map(plain_san);
        for book in self.catalog().books {
            let Some(hash) = book.hash.clone().filter(|h| super::is_hash(h)) else {
                out.not_indexed = out.not_indexed.saturating_add(1);
                continue;
            };
            if book.state != State::Indexed {
                out.not_indexed = out.not_indexed.saturating_add(1);
                continue;
            }
            let room = max.saturating_sub(out.listed);
            let mut hits: Vec<Hit> = Vec::new();
            let mut total = 0usize;
            // Worked out once per book, from its JSON part: which of its SANs and games are asked.
            let mut sans: Option<Vec<bool>> = None;
            let mut games: Option<Vec<bool>> = None;
            let mut each = |meta: &IndexMeta, p: &IndexedPosition| {
                if !query.asks_positions() {
                    return;
                }
                let sans = sans.get_or_insert_with(|| {
                    meta.sans
                        .iter()
                        .map(|s| {
                            wanted_san.as_ref().is_none_or(|(bare, mark)| {
                                let (b, m) = plain_san(s);
                                b == *bare && (mark.is_none() || m == *mark)
                            })
                        })
                        .collect()
                });
                let games = games.get_or_insert_with(|| meta.games.iter().map(|g| query.game_matches(g)).collect());
                if query.position.is_some_and(|k| !p.key().matches(k, query.any_side || p.side_assumed)) {
                    return;
                }
                if wanted_san.is_some() && !p.san.and_then(|s| sans.get(usize::try_from(s).ok()?).copied()).unwrap_or(false) {
                    return;
                }
                if query.asks_games() && !p.game.and_then(|g| games.get(usize::try_from(g).ok()?).copied()).unwrap_or(false) {
                    return;
                }
                if !query.pattern.is_empty() && !query.pattern_matches(p) {
                    return;
                }
                total = total.saturating_add(1);
                if hits.len() < room {
                    hits.push(Hit {
                        page: p.page,
                        label: p.page.map(|page| meta.label(page)),
                        place: p.place,
                        number: p.number,
                        black: p.black,
                        san: p.san.and_then(|s| meta.sans.get(usize::try_from(s).ok()?).cloned()),
                        game: p.game,
                        side_assumed: p.side_assumed,
                        main_line: p.main_line,
                    });
                }
            };
            out.indexes_read = out.indexes_read.saturating_add(1);
            let meta = match index::read(&self.index_path(&hash), None, &mut each) {
                Ok(meta) => meta,
                Err(e) => {
                    let why = match e {
                        ReadError::Missing => "its index is not there".to_owned(),
                        other => other.to_string(),
                    };
                    out.unread.push((hash, why));
                    continue;
                }
            };
            let mut found_games: Vec<(u32, IndexedGame)> = Vec::new();
            let mut games_total = 0usize;
            if query.asks_positions() {
                // The games the hits listed are in, each once.
                let mut seen: HashSet<u32> = HashSet::new();
                for g in hits.iter().filter_map(|h| h.game) {
                    if seen.insert(g)
                        && let Some(info) = usize::try_from(g).ok().and_then(|i| meta.games.get(i))
                    {
                        found_games.push((g, info.clone()));
                    }
                }
                out.total = out.total.saturating_add(total);
                out.listed = out.listed.saturating_add(hits.len());
            } else {
                for (i, g) in meta.games.iter().enumerate() {
                    if !query.game_matches(g) {
                        continue;
                    }
                    games_total = games_total.saturating_add(1);
                    if found_games.len() < room
                        && let Ok(i) = u32::try_from(i)
                    {
                        found_games.push((i, g.clone()));
                    }
                }
                out.total = out.total.saturating_add(games_total);
                out.listed = out.listed.saturating_add(found_games.len());
            }
            if total > 0 || games_total > 0 {
                out.books.push(BookHits { hash, title: meta.title, paths: book.paths, hits, total, games: found_games, games_total });
            }
        }
        out
    }
}
