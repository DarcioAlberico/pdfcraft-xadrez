//! Chess ▸ the Board tab's **In the book (n)** (plan A9-N3, finding A8-07; §2.9 and §2.8.0
//! sketch (d)): where the position the board shows stands in the book open, and what the book
//! plays from it. The item sits in the Board tab's bar ([`bar_item`]) and opens the list in the
//! moves' place ([`list`], `MovesView::Book`; "‹ Moves" and Esc go back).
//!
//! **Never a reading** (the plan's budget rule). The search looks only at what is already read:
//! the book's PGN through its compact index (plan A9-I3b), the diagrams this session read (G11)
//! and those kept on disk (N5). It never starts reading the book, which takes minutes (R8). Off
//! the interface thread, in two [`Job`]s, one of each at a time, the newest asked for when the
//! running one ends:
//! - the diagrams read are listed once for each state of what is read (the document's state, what
//!   the session's cache holds of it, the folder of readings), with their positions' keys
//!   ([`Session::chess_diagram_keys`], renewed when [`Session::chess_cache_revision`] moves);
//! - each position the board shows is looked for in that list and in the PGN's index, and the
//!   book's replies there are played again on the board's own position: one the board cannot
//!   play (castling lost, no en passant) is listed disabled, with why.
//!
//! A diagram the user corrected is found by the position the user gave it. The main line's
//! replies are set in semibold, a variation's in the body's weight, each with the author's sign
//! and how many of the book's games play it; the count is secondary, after the places.
//!
//! [`Session::chess_diagram_keys`]: pdfcraft_engine::Session::chess_diagram_keys
//! [`Session::chess_cache_revision`]: pdfcraft_engine::Session::chess_cache_revision

use std::collections::BTreeMap;
use std::sync::Arc;

use egui::vec2;
use pdfcraft_engine::DocId;
use pdfcraft_engine::chess::ScanStamp;
use pdfcraft_engine::chess::pdfcraft_chess::fen::{BookKey, Fen};
use pdfcraft_engine::chess::pdfcraft_chess::game::Game;
use pdfcraft_engine::chess::pdfcraft_chess::pattern::{Material, MaterialError, PawnQuery, Query as PatternQuery};
use pdfcraft_engine::chess::pdfcraft_chess::placement::{Color, PieceKind, Square};
use pdfcraft_engine::chess::pdfcraft_chessvision::doc::reference::{ReferenceIndex, Stretches};
use pdfcraft_engine::chess::position_find::DiagramKeys;

use crate::PdfCraftApp;
use crate::chess_job::{Job, JobCtl};
use crate::chess_ui::ChessState;
use crate::theme::{self, Tokens};

/// Most places of each kind listed (the PGN's moves, the diagrams); every one is counted.
pub(crate) const MAX_LISTED: usize = 100;
/// Most of the PGN's moves to the position looked at for what the book plays from it.
const MAX_OCCURRENCES: usize = 2_000;
/// Most of the PGN's games read again for the replies they play (and their signs): a position
/// every game reaches, the start, is counted in the first ones.
pub(crate) const MAX_GAMES_READ: usize = 200;
/// Most characters the material field takes: a signature of 16 pieces a side and a dash.
const MAX_MATERIAL: usize = 40;

/// What the list of the diagrams read was made for: the document, its state, what this session's
/// cache held of it, and the folder of readings kept on disk.
#[derive(Clone, Debug, PartialEq)]
struct ReadKey {
    doc: DocId,
    stamp: Option<ScanStamp>,
    revision: u64,
    disk: Option<std::path::PathBuf>,
}

/// The diagrams read of a book, with their positions' keys, or why they could not be listed.
struct ReadList {
    key: ReadKey,
    keys: Result<DiagramKeys, String>,
}

/// What is looked for in the book.
#[derive(Clone, Debug, PartialEq)]
enum Look {
    /// The board's position, all of it (castling and en passant decide what can be played there),
    /// or the position that is not legal it shows.
    Position { fen: String, legal: bool },
    /// Positions by material and pawns (plan A9-N16b).
    Pattern(PatternQuery),
}

/// A search in the book open: the board's position, or positions by material and pawns.
#[derive(Clone)]
struct Query {
    doc: DocId,
    look: Look,
    /// The book's PGN, indexed, when it has one.
    index: Option<Arc<ReferenceIndex>>,
    /// The diagrams read; `None` while the first list is being made.
    read: Option<Arc<ReadList>>,
    /// The diagrams the user corrected: page, index (0-based) and the position given.
    corrected: Vec<(usize, usize, String)>,
    /// The book's pages.
    pages: usize,
}

impl PartialEq for Query {
    fn eq(&self, other: &Self) -> bool {
        let same = |a: &Option<Arc<ReferenceIndex>>, b: &Option<Arc<ReferenceIndex>>| match (a, b) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        let same_read = match (&self.read, &other.read) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        self.doc == other.doc
            && self.look == other.look
            && same(&self.index, &other.index)
            && same_read
            && self.corrected == other.corrected
            && self.pages == other.pages
    }
}

/// A move of the book's PGN that leads to the position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PgnHit {
    /// Its game (0-based) and node in it.
    pub(crate) game: usize,
    pub(crate) node: usize,
    /// The game's name (its Event, Site, White and Black that say something).
    pub(crate) name: String,
    /// The move as numbered ("2...Nc6", English letters); `None` for a game's start.
    pub(crate) number: u32,
    pub(crate) black: bool,
    pub(crate) san: Option<String>,
    pub(crate) main_line: bool,
    /// Reached by another sequence of moves than the first one found.
    pub(crate) transposed: bool,
}

/// What the book's PGN says of the position.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct PgnFound {
    /// The first ones, at most [`MAX_LISTED`].
    pub(crate) found: Vec<PgnHit>,
    pub(crate) total: usize,
    /// The different sequences of moves that lead to it: one more than its transpositions.
    pub(crate) lines: usize,
}

/// A diagram read as the position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DiagramHit {
    /// Page and index on it, 0-based.
    pub(crate) page: usize,
    pub(crate) index: usize,
    /// Its side to move was not printed: only the pieces were compared.
    pub(crate) side_assumed: bool,
    /// The page as the book numbers it (filled once the answer is in).
    pub(crate) label: String,
}

/// A reply played on the board's position: the squares clicked and the piece a pawn becomes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Played {
    pub(crate) from: Square,
    pub(crate) to: Square,
    pub(crate) promotion: Option<PieceKind>,
}

/// Why the board cannot play a reply the book plays from the same pieces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refused {
    /// The book castles; the board's king or rook moved (or its position says no castling).
    Castling,
    /// The book takes en passant; on the board the pawn did not just move two squares.
    EnPassant,
    /// Anything else (the board's position is not legal).
    Other,
}

/// What the book plays from the position: one reply, however many games play it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Next {
    /// As the book's PGN writes it, English letters ("Bb5+").
    pub(crate) san: String,
    /// The author's signs on it, every game's, each once, in the order met.
    pub(crate) nags: Vec<u16>,
    /// On its game's main line in some game (else in variations only).
    pub(crate) main_line: bool,
    /// How many of the games read play it.
    pub(crate) count: usize,
    /// The first game that plays it (0-based).
    pub(crate) game: usize,
    /// Played on the board's position, or why it cannot be.
    pub(crate) play: Result<Played, Refused>,
}

/// What looking for one position found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Answer {
    /// `None`: the book has no PGN.
    pub(crate) pgn: Option<PgnFound>,
    /// The first diagrams, at most [`MAX_LISTED`], page by page.
    pub(crate) diagrams: Vec<DiagramHit>,
    pub(crate) diagrams_total: usize,
    /// The pages whose diagrams were looked at, and the book's.
    pub(crate) pages_read: usize,
    pub(crate) pages: usize,
    /// What the book plays from it, the main line's first, then the most played.
    pub(crate) next: Vec<Next>,
    /// The number the board's next move has, and whether Black plays it.
    pub(crate) number: u32,
    pub(crate) black: bool,
    /// The PGN's games read for the replies, when not every game that reaches the position was.
    pub(crate) games_cut: Option<usize>,
    /// By material and pawns (plan A9-N16b): the stretches of the book's PGN, when it has one (in
    /// place of [`Answer::pgn`]).
    pub(crate) stretches: Option<Stretches>,
    /// Why the diagrams read could not be looked at (a document that does not allow copying, a
    /// reopening that failed): the PGN still answers (plan A9-N3b).
    pub(crate) diagrams_error: Option<String>,
}

impl Answer {
    /// The places found: the PGN's moves to the position (or its stretches) and the diagrams.
    pub(crate) fn count(&self) -> usize {
        let pgn = self.pgn.as_ref().map_or(0, |p| p.total).saturating_add(self.stretches.as_ref().map_or(0, |s| s.total));
        pgn.saturating_add(self.diagrams_total)
    }

    /// Nothing of the book is read: no PGN, no page read, nothing corrected. Never "not in the
    /// book".
    pub(crate) fn nothing_read(&self) -> bool {
        self.pgn.is_none() && self.stretches.is_none() && self.pages_read == 0 && self.diagrams_total == 0
    }
}

/// What the list in the moves' place looks for: the position the board shows, or positions by
/// material and pawns (plan A9-N16b).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Mode {
    #[default]
    Position,
    Pattern,
}

/// The search by material and pawns: what its filter holds (plan A9-N16b).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Filter {
    /// The material's signature, "KRP-KR".
    pub(crate) material: String,
    /// The signature with the colours swapped is taken too.
    pub(crate) either_side: bool,
    /// A side with doubled, isolated or passed pawns (each one asked for, the same side).
    pub(crate) doubled: bool,
    pub(crate) isolated: bool,
    pub(crate) passed: bool,
    /// The material field takes the keyboard at the next frame (Find in the book by material…).
    pub(crate) focus: bool,
}

impl Filter {
    /// The query the filter makes; `Ok(None)` while it asks for nothing, `Err` for a material that
    /// is not a signature.
    pub(crate) fn query(&self) -> Result<Option<PatternQuery>, MaterialError> {
        let text = self.material.trim();
        let material = if text.is_empty() { None } else { Some(Material::parse(text)?) };
        let pawns = PawnQuery { doubled: self.doubled, isolated: self.isolated, passed: self.passed, side: None };
        let query = PatternQuery { material, either_side: self.either_side, pawns };
        Ok((!query.is_empty()).then_some(query))
    }
}

/// One search's state: what it is for now, its answer and its job.
#[derive(Default)]
struct Slot {
    /// What it was last asked for: `None` before a tick, `Some(None)` with nothing to look for
    /// (no book open).
    now: Option<Option<Query>>,
    answer: Option<(Query, Result<Arc<Answer>, String>)>,
    asking: Option<(Query, Job<Answer>)>,
}

impl Slot {
    /// Where the search is for what it was last asked for. While a new answer is looked for, the
    /// last one found in the same book stays (plan A9-N3b), to be shown dimmed: the list keeps its
    /// rows, the keyboard and its scroll.
    fn status(&self) -> Status {
        let now = match &self.now {
            None => return Status::Searching(None),
            Some(None) => return Status::NothingRead,
            Some(Some(q)) => q,
        };
        match &self.answer {
            Some((q, Ok(a))) if q == now => {
                if !a.nothing_read() {
                    Status::Found(a.clone())
                } else if let Some(why) = &a.diagrams_error {
                    // Nothing else to show: the diagrams' failure is the answer.
                    Status::Failed(why.clone())
                } else {
                    Status::NothingRead
                }
            }
            Some((q, Err(e))) if q == now => Status::Failed(e.clone()),
            Some((q, Ok(a))) if q.doc == now.doc && !a.nothing_read() => Status::Searching(Some(a.clone())),
            _ => Status::Searching(None),
        }
    }

    /// Ask for `query`: the search running for another one is dropped (which asks it to stop) and
    /// `spawn`'s takes its place (plan A9-N3b); one already answered or running is not asked again.
    /// Whether a search was started.
    fn ask(&mut self, query: Query, spawn: impl FnOnce(Query) -> Job<Answer>) -> bool {
        self.now = Some(Some(query.clone()));
        let answered = self.answer.as_ref().is_some_and(|(q, _)| *q == query);
        let running = self.asking.as_ref().is_some_and(|(q, _)| *q == query);
        if query.read.is_none() || answered || running {
            return false;
        }
        self.asking = Some((query.clone(), spawn(query)));
        true
    }

    /// The document it looks in.
    fn doc(&self) -> Option<DocId> {
        self.now.as_ref().and_then(|q| q.as_ref()).map(|q| q.doc)
    }
}

/// What a click in the list asked of the app, done at its next tick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum BookAction {
    /// Put the book's PGN's game `game` on the board at node `node`.
    OpenGame { doc: DocId, game: usize, node: usize },
    /// Open diagram `index` of `page` (0-based) on its page and in the Diagrams tab, its page
    /// read first when this session has not (one page, from the disk when kept there).
    ShowDiagram { doc: DocId, page: usize, index: usize },
}

/// The search's state, in the chess state.
#[derive(Default)]
pub(crate) struct InBook {
    /// The Board tab's bar was drawn in the last pass: the search runs only while it shows.
    pub(crate) shown: bool,
    /// The list looked by material and pawns in the last pass (plan A9-N16b): that search runs
    /// only while it shows.
    pub(crate) pattern_shown: bool,
    read: Option<Arc<ReadList>>,
    reading: Option<(ReadKey, Job<DiagramKeys>)>,
    /// The position the board shows (the bar's count, and the list in [`Mode::Position`]).
    position: Slot,
    /// By material and pawns.
    pattern: Slot,
    /// What the list looks for, and the filter by material and pawns.
    pub(crate) mode: Mode,
    pub(crate) filter: Filter,
    /// What a click in the list asked for.
    pub(crate) asked: Option<BookAction>,
    /// The list's rows the keyboard reaches, in order, as the last pass drew them, and the ↑ ↓
    /// Home End pressed on one of them (plan A9-N3b), taken before the chess keys.
    rows: Vec<egui::Id>,
    keys: Vec<egui::Key>,
}

/// Most keys taken from one frame for the list's rows.
const MAX_KEYS: usize = 16;

/// Where the search is for the position the board shows.
#[derive(Clone, Debug)]
pub(crate) enum Status {
    /// Being looked for (or the diagrams read being listed), with the last answer found in the
    /// same book, if any, to show meanwhile.
    Searching(Option<Arc<Answer>>),
    /// Nothing of the book is read, or no book is open.
    NothingRead,
    /// Looked for: found or not.
    Found(Arc<Answer>),
    /// It could not be looked for.
    Failed(String),
}

impl InBook {
    /// Where the search is for the position the board showed at the last tick.
    pub(crate) fn status(&self) -> Status {
        self.position.status()
    }

    /// Where the search by material and pawns is (plan A9-N16b).
    pub(crate) fn pattern_status(&self) -> Status {
        self.pattern.status()
    }

    /// The document of the position looked for.
    fn doc(&self) -> Option<DocId> {
        self.position.doc()
    }

    /// A job is running.
    fn busy(&self) -> bool {
        self.reading.is_some() || self.position.asking.is_some() || self.pattern.asking.is_some()
    }
}

/// The diagrams the user corrected in `results`: page, index and the position given.
fn corrected_of(results: &crate::chess_ui::ChessResults) -> Vec<(usize, usize, String)> {
    use pdfcraft_engine::chess::corrections::Kind;
    let mut out: Vec<(usize, usize, String)> =
        results.kinds.iter().filter(|(_, k)| k.kind == Kind::Corrected).map(|(at, k)| (at.page, at.index, k.fen.clone())).collect();
    out.sort_unstable();
    out
}

/// The piece a pawn becomes in a UCI move's fifth letter.
fn promotion_of(uci: &str) -> Option<PieceKind> {
    match uci.chars().nth(4)?.to_ascii_lowercase() {
        'q' => Some(PieceKind::Queen),
        'r' => Some(PieceKind::Rook),
        'b' => Some(PieceKind::Bishop),
        'n' => Some(PieceKind::Knight),
        _ => None,
    }
}

/// Reply `san`, which the book plays at node `parent` of `book`, played on `board` (the board's
/// position), or why it cannot be.
fn play_on(board: Option<&Game>, book: &mut Game, parent: usize, reply: usize, san: &str) -> Result<Played, Refused> {
    let Some(board) = board else { return Err(Refused::Other) };
    let mut g = board.clone();
    if g.play_san(san).is_ok() {
        let node = g.current();
        if let Some((from, to)) = g.move_at(node) {
            return Ok(Played { from, to, promotion: g.uci(node).as_deref().and_then(promotion_of) });
        }
        return Err(Refused::Other);
    }
    if san.starts_with("O-O") || san.starts_with("0-0") {
        return Err(Refused::Castling);
    }
    // En passant in the book: a pawn takes onto a square nothing stands on.
    if let Some((from, to)) = book.move_at(reply) {
        book.go_to_node(parent);
        let pawn = book.piece_on(from).is_some_and(|p| p.kind == PieceKind::Pawn);
        if pawn && from.file() != to.file() && book.piece_on(to).is_none() {
            return Err(Refused::EnPassant);
        }
    }
    Err(Refused::Other)
}

/// Look for `q`'s position (on a worker): in the PGN's index, in the diagrams read (the user's
/// corrections in place of the readings they correct), and what the book plays from it, played
/// again on the board's position. Never reads a page.
fn look(q: &Query, ctl: &JobCtl) -> Result<Answer, String> {
    match &q.look {
        Look::Position { fen, legal } => look_position(q, fen, *legal, ctl),
        Look::Pattern(query) => look_pattern(q, query),
    }
}

/// The diagrams read (the user's corrections in place of the readings they correct) that `wanted`
/// takes, given a diagram's key and whether its side to move was assumed: into `out`, page by
/// page, the first [`MAX_LISTED`] listed and all counted. When they could not be listed, why is
/// said in [`Answer::diagrams_error`] and the rest of the search goes on (plan A9-N3b).
fn diagrams_where(q: &Query, out: &mut Answer, wanted: impl Fn(BookKey, bool) -> bool) {
    let read = match q.read.as_deref().map(|r| &r.keys) {
        Some(Ok(keys)) => Some(keys),
        Some(Err(e)) => {
            out.diagrams_error = Some(e.clone());
            None
        }
        None => None,
    };
    let mut corrected: BTreeMap<(usize, usize), BookKey> = BTreeMap::new();
    for (page, index, fen) in &q.corrected {
        if let Ok(f) = Fen::parse(fen) {
            corrected.insert((*page, *index), f.book_key());
        }
    }
    let mut hits: Vec<(usize, usize, bool)> = Vec::new();
    let mut seen: std::collections::BTreeSet<(usize, usize)> = std::collections::BTreeSet::new();
    if let Some(keys) = read {
        out.pages_read = keys.pages_read;
        for (d, k) in &keys.diagrams {
            let at = (d.page as usize, usize::from(d.index));
            seen.insert(at);
            // A correction is the user's position, its side said.
            let (k, assumed) = match corrected.get(&at) {
                Some(c) => (*c, false),
                None => (*k, d.side_assumed),
            };
            if wanted(k, assumed) {
                hits.push((at.0, at.1, assumed));
            }
        }
    }
    // A diagram corrected though not read (no reading on its page).
    for (at, k) in &corrected {
        if !seen.contains(at) && wanted(*k, false) {
            hits.push((at.0, at.1, false));
        }
    }
    hits.sort_unstable();
    out.diagrams_total = hits.len();
    out.diagrams = hits
        .into_iter()
        .take(MAX_LISTED)
        .map(|(page, index, side_assumed)| DiagramHit { page, index, side_assumed, label: page.saturating_add(1).to_string() })
        .collect();
}

/// Positions by material and pawns (plan A9-N16b): the stretches of the book's PGN, each game
/// played again once ([`ReferenceIndex::stretches`]), and the diagrams read whose pieces match.
fn look_pattern(q: &Query, query: &PatternQuery) -> Result<Answer, String> {
    let mut out = Answer { pages: q.pages, ..Answer::default() };
    diagrams_where(q, &mut out, |k, _| query.matches(&k.placement.placement()));
    out.stretches = q.index.as_deref().map(|index| index.stretches(query, MAX_LISTED));
    Ok(out)
}

/// The board's position `fen` (`legal`: the board can play there).
fn look_position(q: &Query, fen: &str, legal: bool, ctl: &JobCtl) -> Result<Answer, String> {
    let parsed = Fen::parse(fen).map_err(|e| e.to_string())?;
    let key = parsed.book_key();
    // A position whose side to move is not known (one that is not legal can lack it): its pieces.
    let any_side = parsed.side().is_none();
    let mut out = Answer { pages: q.pages, number: parsed.fullmove_number(), black: parsed.side() == Some(Color::Black), ..Answer::default() };
    diagrams_where(q, &mut out, |k, assumed| k.matches(key, any_side || assumed));
    // The book's PGN.
    let Some(index) = q.index.as_deref() else { return Ok(out) };
    let occ = index.occurrences(key, any_side, MAX_OCCURRENCES);
    let mut pgn = PgnFound { total: occ.total, lines: occ.lines, found: Vec::new() };
    for o in occ.found.iter().take(MAX_LISTED) {
        pgn.found.push(PgnHit {
            game: o.at.game,
            node: o.at.node,
            name: index.games().get(o.at.game).map(|g| g.name.clone()).unwrap_or_default(),
            number: o.number,
            black: o.black,
            san: index.san(o.at).map(str::to_owned),
            main_line: o.main_line,
            transposed: o.transposed,
        });
    }
    out.pgn = Some(pgn);
    // What the book plays from it: each game that reaches it read again, once.
    let board = if legal { Game::from_fen(fen).ok() } else { None };
    let mut by_game: BTreeMap<usize, Vec<(usize, bool)>> = BTreeMap::new();
    for o in &occ.found {
        by_game.entry(o.at.game).or_default().push((o.at.node, o.main_line));
    }
    let games = by_game.len();
    let mut next: Vec<Next> = Vec::new();
    for (&g, nodes) in by_game.iter().take(MAX_GAMES_READ) {
        if ctl.cancelled() {
            return Err(String::new());
        }
        let Some(mut book) = index.game(g) else { continue };
        for &(node, main) in nodes {
            let replies = book.replies(node).to_vec();
            for (k, reply) in replies.into_iter().enumerate() {
                let san = book.san(reply).to_owned();
                if san.is_empty() {
                    continue;
                }
                let nags: Vec<u16> =
                    book.nags(reply).iter().copied().filter(|c| !pdfcraft_engine::chess::pdfcraft_chess::nag::is_diagram(*c)).collect();
                let main_line = main && k == 0;
                match next.iter_mut().find(|n| n.san == san) {
                    Some(n) => {
                        n.count = n.count.saturating_add(1);
                        n.main_line |= main_line;
                        for c in nags {
                            if !n.nags.contains(&c) {
                                n.nags.push(c);
                            }
                        }
                    }
                    None => {
                        let play = play_on(board.as_ref(), &mut book, node, reply, &san);
                        next.push(Next { san, nags, main_line, count: 1, game: g, play });
                    }
                }
            }
        }
    }
    // The main line's first, then the most played, then as the book has them.
    next.sort_by(|a, b| b.main_line.cmp(&a.main_line).then(b.count.cmp(&a.count)));
    out.next = next;
    let read_all = occ.total <= occ.found.len() && games <= MAX_GAMES_READ;
    out.games_cut = (!read_all).then_some(games.min(MAX_GAMES_READ));
    Ok(out)
}

/// Whether the diagrams read are listed again: not while the list is for what is read now
/// (`listed`), and, while a scan of the document runs (`scanning`), only when there is no list of
/// it at all (`have`): every batch the scan reads moves the cache's revision, and each new list
/// opens and parses the whole PDF on a worker; the list is made once the scan ends (plan A9-N3b).
fn list_again(listed: bool, have: bool, scanning: bool) -> bool {
    !listed && !(scanning && have)
}

impl PdfCraftApp {
    /// Every frame: what a click in the In the book list asked for, the jobs that ended picked
    /// up, and, while the Board tab's bar shows, the diagrams read listed and the position the
    /// board shows looked for, each when what it was made for changed (plan A9-N3).
    pub(crate) fn chess_in_book_tick(&mut self) {
        match self.chess.in_book.asked.take() {
            Some(BookAction::OpenGame { doc, game, node }) => self.put_book_pgn_game(doc, game, node),
            // Only in the book it was found in, still the one shown.
            Some(BookAction::ShowDiagram { doc, page, index }) if self.active_ids().is_some_and(|(_, id)| id == doc) => {
                self.pick_after_scan(doc, pdfcraft_engine::chess::DiagramAt::new(page, index));
            }
            Some(BookAction::ShowDiagram { .. }) | None => {}
        }
        self.poll_in_book();
        let pattern_shown = std::mem::take(&mut self.chess.in_book.pattern_shown);
        if !std::mem::take(&mut self.chess.in_book.shown) {
            return;
        }
        let Some((_, doc)) = self.active_ids() else {
            self.chess.in_book.position.now = Some(None);
            self.chess.in_book.pattern.now = Some(None);
            return;
        };
        // The diagrams read, listed once per state of what is read.
        let key = ReadKey {
            doc,
            stamp: self.session.chess_stamp(doc),
            revision: self.session.chess_cache_revision(doc),
            disk: self.chess.reading_dir.clone(),
        };
        let read = self.chess.in_book.read.as_ref();
        let (listed, have) = (read.is_some_and(|r| r.key == key), read.is_some_and(|r| r.key.doc == doc));
        let scanning = self.chess.run.as_ref().is_some_and(|r| r.doc == doc);
        if list_again(listed, have, scanning) && self.chess.in_book.reading.is_none() {
            self.list_read_diagrams(key);
        }
        // The position.
        let (fen, legal) = {
            let state = self.chess.board();
            match &state.illegal {
                Some((_, fen)) => (fen.clone(), false),
                None => (state.game.fen(), true),
            }
        };
        let query = Query {
            doc,
            look: Look::Position { fen, legal },
            index: self.session.chess_book_pgn(doc).map(|b| b.index.clone()),
            read: self.chess.in_book.read.clone().filter(|r| r.key.doc == doc),
            corrected: self.chess.results_of(doc).map(corrected_of).unwrap_or_default(),
            pages: self.session.get(doc).map_or(0, |d| d.info.pages.len()),
        };
        // By material and pawns, while the list looks so and its filter asks for something.
        let pattern = self.chess.in_book.filter.query().ok().flatten().filter(|_| pattern_shown);
        if let Some(p) = pattern {
            self.ask_in_book(true, Query { look: Look::Pattern(p), ..query.clone() });
        }
        self.ask_in_book(false, query);
    }

    /// Ask the position's search (or, `pattern`, the one by material and pawns) for `query`: one
    /// job at a time, a running one for an older query dropped, which asks it to stop (plan
    /// A9-N3b); none before the diagrams read are listed.
    fn ask_in_book(&mut self, pattern: bool, query: Query) {
        let inline = self.run_inline;
        let name = if pattern { "pdfcraft-chess-in-book-pattern" } else { "pdfcraft-chess-in-book" };
        let slot = if pattern { &mut self.chess.in_book.pattern } else { &mut self.chess.in_book.position };
        let started = slot.ask(query, |asked| Job::spawn(name, inline, move |ctl| look(&asked, ctl)));
        // Inline (tests, the web) it has ended already.
        if started {
            self.poll_in_book();
        }
    }

    /// List the diagrams read of document `key.doc` off the interface thread, in a session of its
    /// own given what this session read, from the disk too: nothing is scanned.
    fn list_read_diagrams(&mut self, key: ReadKey) {
        let Some(source) = self.chess_job_source(key.doc) else { return };
        let settings = self.chess_scan_settings();
        let job = Job::spawn("pdfcraft-chess-in-book-read", self.run_inline, move |_| {
            let (mut s, id) = source.open()?;
            let store = settings.cache.as_ref().map(|c| pdfcraft_engine::chess::reading::store_for(&c.dir));
            s.chess_diagram_keys(id, store.as_deref().map(|st| (st, &settings)))
        });
        self.chess.in_book.reading = Some((key, job));
        self.poll_in_book();
    }

    /// Pick up the jobs that ended: the diagrams read, and the answers, their pages said as the
    /// book numbers them.
    fn poll_in_book(&mut self) {
        if let Some((key, job)) = self.chess.in_book.reading.as_mut()
            && let Some(out) = job.poll()
        {
            let key = key.clone();
            self.chess.in_book.reading = None;
            self.chess.in_book.read = Some(Arc::new(ReadList { key, keys: out }));
        }
        for pattern in [false, true] {
            let slot = if pattern { &mut self.chess.in_book.pattern } else { &mut self.chess.in_book.position };
            let Some((query, job)) = slot.asking.as_mut() else { continue };
            let Some(out) = job.poll() else { continue };
            let query = query.clone();
            slot.asking = None;
            let out = out.map(|mut a| {
                for d in &mut a.diagrams {
                    d.label = self.page_label_in(query.doc, d.page);
                }
                Arc::new(a)
            });
            let slot = if pattern { &mut self.chess.in_book.pattern } else { &mut self.chess.in_book.position };
            slot.answer = Some((query, out));
        }
        if self.chess.in_book.busy()
            && let Some(ctx) = &self.ctx
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    /// Wait for the In the book search's jobs to end, at most `timeout` each (tests that run them
    /// on workers). Whether none is running. Desktop only: on the web the jobs run inline.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn wait_in_book(&mut self, timeout: std::time::Duration) -> bool {
        for _ in 0..4 {
            if let Some((_, job)) = &self.chess.in_book.reading {
                job.wait(timeout);
            }
            for slot in [&self.chess.in_book.position, &self.chess.in_book.pattern] {
                if let Some((_, job)) = &slot.asking {
                    job.wait(timeout);
                }
            }
            self.poll_in_book();
        }
        !self.chess.in_book.busy()
    }

    /// Chess ▸ Find in the book by material… (plan A9-N16b): the Board tab, the In the book list
    /// in the moves' place, looking by material and pawns, the keyboard in its material field.
    pub fn find_chess_material(&mut self) {
        self.show_chess_board();
        self.chess.board_view = crate::chess_board_tab::MovesView::Book;
        self.chess.in_book.mode = Mode::Pattern;
        self.chess.in_book.filter.focus = true;
    }
}

/// The bar's label for `status`: "In the book (3)" once looked for (the last count while the next
/// is looked for), "In the book" before and with nothing read.
pub(crate) fn bar_label(status: &Status) -> String {
    match status {
        Status::Found(a) | Status::Searching(Some(a)) => crate::i18n::fmt(tl!("In the book ({n})"), &[("n", &count_text(a.count()))]),
        _ => tl!("In the book").to_owned(),
    }
}

/// A search that failed, said in the interface's language with the reason after it (plan A9-N3b).
pub(crate) fn failure_text(why: &str) -> String {
    crate::i18n::fmt(tl!("The book couldn't be searched: {e}"), &[("e", why)])
}

/// Move the keyboard among the list's `rows` (as the last pass drew them) by `keys`: ↑ and ↓ to
/// the row before and after, Home and End to the first and the last (plan A9-N3b). Nothing when
/// no row has it.
fn move_among(ctx: &egui::Context, rows: &[egui::Id], keys: &[egui::Key]) {
    let Some(mut at) = rows.iter().position(|id| ctx.memory(|m| m.has_focus(*id))) else { return };
    let last = rows.len().saturating_sub(1);
    for key in keys {
        at = match key {
            egui::Key::ArrowUp => at.saturating_sub(1),
            egui::Key::ArrowDown => at.saturating_add(1).min(last),
            egui::Key::Home => 0,
            egui::Key::End => last,
            _ => at,
        };
    }
    if let Some(id) = rows.get(at) {
        ctx.memory_mut(|m| m.request_focus(*id));
    }
}

/// Take ↑ ↓ Home End out of `events` while a row of the list has the keyboard (plan A9-N3b), before
/// the chess keys would step through the game with them: the list moves among its rows with them
/// at its next pass. Only presses are taken, as for the Diagrams list.
pub(crate) fn take_keys(in_book: &mut InBook, ctx: &egui::Context, events: &mut Vec<egui::Event>, free: bool) {
    in_book.keys.clear();
    if !free || !in_book.rows.iter().any(|id| ctx.memory(|m| m.has_focus(*id))) {
        return;
    }
    let keys = &mut in_book.keys;
    events.retain(|event| match event {
        egui::Event::Key {
            key: key @ (egui::Key::ArrowUp | egui::Key::ArrowDown | egui::Key::Home | egui::Key::End),
            pressed: true,
            modifiers,
            ..
        } if modifiers.is_none() => {
            if keys.len() < MAX_KEYS {
                keys.push(*key);
            }
            false
        }
        _ => true,
    });
}

/// A count as the interface's language writes it ("1.034" in Portuguese; plan A9-I14).
fn count_text(n: usize) -> String {
    crate::i18n::number(n)
}

/// The bar's item: In the book (n) (`label`, [`bar_label`]), cut with "…" past `room` points of
/// text and whole in its tooltip and name, selected while its list shows. Whether it was clicked.
pub(crate) fn bar_item(ui: &mut egui::Ui, chess: &mut ChessState, label: &str, room: f32, height: f32) -> bool {
    chess.in_book.shown = true;
    let open = chess.board_view == crate::chess_board_tab::MovesView::Book;
    let font = egui::TextStyle::Button.resolve(ui.style());
    let width = |s: &str| crate::widgets::text_width(ui, s, &font);
    // Cut, the words give way and the count stays: "In the b… (12)".
    let shown = match chess.in_book.status() {
        Status::Found(a) | Status::Searching(Some(a)) if width(label) > room => {
            let count = format!("({})", count_text(a.count()));
            let words = crate::widgets::fit_line(tl!("In the book"), (room - width(&format!(" {count}"))).max(12.0), width);
            format!("{words} {count}")
        }
        _ => crate::widgets::fit_line(label, room.max(12.0), width),
    };
    let tip = tl!("Where the position shown is in the book, and what the book plays from it");
    let hover = if shown == label { tip.to_owned() } else { format!("{label}\n{tip}") };
    let r = ui.add(egui::Button::new(shown.as_str()).selected(open).min_size(vec2(24.0, height))).on_hover_text(hover);
    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, open, label));
    r.clicked()
}

/// A section's heading in the list, painted small and muted.
fn section(ui: &mut egui::Ui, t: &Tokens, text: &str) {
    ui.add_space(6.0);
    ui.label(egui::RichText::new(text).font(theme::semibold(12.0)).color(t.text_muted));
}

/// The signs after a move: `!` and the like glued on (two games' different ones parted by a
/// slash, "!/?!"), the others after a space.
fn signs(nags: &[u16]) -> String {
    let mut out = String::new();
    let mut assessed = false;
    for &code in nags {
        if (1..=6).contains(&code) {
            if assessed {
                out.push('/');
            }
            assessed = true;
        } else {
            out.push(' ');
        }
        out.push_str(&crate::chess_board_ui::nag_text(code));
    }
    out
}

/// The signs' names, for assistive technology.
fn sign_names(nags: &[u16]) -> Vec<String> {
    nags.iter().filter_map(|&c| crate::chess_annotate::nag_name(c)).map(str::to_owned).collect()
}

/// Why the board cannot play a reply, in words.
pub(crate) fn refused_text(why: Refused) -> &'static str {
    match why {
        Refused::Castling => tl!("Castling isn't allowed on this board."),
        Refused::EnPassant => tl!("En passant isn't possible on this board."),
        Refused::Other => tl!("Not legal on this board."),
    }
}

/// The game's first page the book discusses, from the alignment to the book's PGN.
fn game_page(chess: &ChessState, doc: DocId, game: usize) -> Option<String> {
    let link = chess.doc(doc)?.book_link.as_ref()?;
    let g = link.games.iter().find(|g| g.game == game)?;
    g.first_label.clone().or_else(|| g.first_page.map(|p| p.saturating_add(1).to_string()))
}

/// The list in the moves' place (`MovesView::Book`): where the position is in the book (or why
/// not), what the book plays from it, and where. A reply is played on the board; a game of the
/// PGN opens on the board at its move; a diagram opens on its page, in the Diagrams tab (asked of
/// the app, [`BookAction`]).
pub(crate) fn list(ui: &mut egui::Ui, t: &Tokens, chess: &mut ChessState) {
    // ↑ ↓ Home End among the rows the last pass drew; this pass names them again.
    let (rows, keys) = (std::mem::take(&mut chess.in_book.rows), std::mem::take(&mut chess.in_book.keys));
    move_among(ui.ctx(), &rows, &keys);
    ui.label(egui::RichText::new(tl!("In the book")).strong());
    // What it looks for: the board's position, or positions by material and pawns (plan A9-N16b).
    let mut mode = chess.in_book.mode;
    crate::widgets::segmented(ui, tl!("Look for"), &mut mode, &[(Mode::Position, tl!("This position")), (Mode::Pattern, tl!("By material"))]);
    chess.in_book.mode = mode;
    ui.add_space(4.0);
    match mode {
        Mode::Position => position_list(ui, t, chess),
        Mode::Pattern => pattern_list(ui, t, chess),
    }
    // Past this book: the position in every book of the Library (plan A9-N15c; desktop only),
    // after the list, so the keyboard meets the book's own places first.
    #[cfg(not(target_arch = "wasm32"))]
    if mode == Mode::Position {
        ui.add_space(8.0);
        let r = ui.add(egui::Button::new(tl!("Look in the Library")).min_size(vec2(24.0, 24.0)));
        if r.on_hover_text(tl!("Find this position in every book of the Library")).clicked() {
            let fen = crate::chess_library::board_fen(chess);
            chess.library.ask_position(fen);
        }
    }
}

/// What was looked at: the book's PGN, and the pages read of the book's.
fn looked_at(answer: &Answer) -> String {
    let mut looked = Vec::new();
    if answer.pgn.is_some() || answer.stretches.is_some() {
        looked.push(tl!("The book's PGN").to_owned());
    }
    looked.push(crate::i18n::fmt(tl!("Pages read: {r} of {p}"), &[("r", &count_text(answer.pages_read)), ("p", &count_text(answer.pages))]));
    looked.join(" · ")
}

/// The diagrams of `answer`, each opening on its page and in the Diagrams tab, and how many more
/// there are than listed (`listed` of the other places listed too).
fn diagram_rows(ui: &mut egui::Ui, t: &Tokens, chess: &mut ChessState, doc: Option<DocId>, answer: &Answer, listed: usize, stale: bool) {
    for (k, d) in answer.diagrams.iter().enumerate() {
        let title = crate::i18n::fmt(tl!("Diagram {n}, p. {page}"), &[("n", &count_text(d.index.saturating_add(1))), ("page", &d.label)]);
        let (clicked, id) = ui
            .push_id(("chess-in-book-diagram", k), |ui| {
                let r = ui.add(egui::Button::new(title.as_str()).min_size(vec2(24.0, 24.0)));
                let mut hover = tl!("Show this diagram on the page").to_owned();
                if d.side_assumed {
                    hover = format!("{hover}\n{}", tl!("Only the pieces were compared: the side to move isn't printed."));
                }
                let r = r.on_hover_text(hover);
                r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &title));
                (r.clicked(), r.id)
            })
            .inner;
        chess.in_book.rows.push(id);
        // A row of the last answer, shown while the next is looked for, does nothing.
        if clicked
            && !stale
            && let Some(doc) = doc
        {
            chess.in_book.asked = Some(BookAction::ShowDiagram { doc, page: d.page, index: d.index });
        }
    }
    let listed = listed.saturating_add(answer.diagrams.len());
    if listed < answer.count() {
        let note = crate::i18n::fmt(tl!("The first {n} are listed."), &[("n", &count_text(listed))]);
        ui.add(egui::Label::new(egui::RichText::new(note).small().color(t.text_faint)).wrap());
    }
}

/// The list's state for `status`: drawn and `None` while there is nothing to list (looking for the
/// first answer, nothing read, a failure), else the answer to list and whether it is the last one,
/// shown dimmed while the next is looked for (plan A9-N3b): the rows, the keyboard on one of them
/// and the scroll stay.
fn answer_of(ui: &mut egui::Ui, t: &Tokens, status: Status, looking: &str) -> Option<(Arc<Answer>, bool)> {
    match status {
        Status::Searching(None) => {
            ui.add(egui::Label::new(egui::RichText::new(looking).color(t.text_muted)).wrap());
            None
        }
        Status::NothingRead => {
            ui.add(egui::Label::new(egui::RichText::new(tl!("Read the book (Detect diagrams or Text) to search.")).color(t.text_muted)).wrap());
            None
        }
        Status::Failed(why) => {
            crate::notice::callout(ui, crate::notice::Tone::Danger, &failure_text(&why));
            None
        }
        Status::Searching(Some(a)) => {
            ui.multiply_opacity(0.5);
            Some((a, true))
        }
        Status::Found(a) => Some((a, false)),
    }
}

/// The diagrams' failure, when the rest answered (plan A9-N3b).
fn diagrams_failure(ui: &mut egui::Ui, answer: &Answer) {
    if let Some(why) = &answer.diagrams_error {
        let said = crate::i18n::fmt(tl!("The diagrams read couldn't be looked at: {e}"), &[("e", why)]);
        crate::notice::callout(ui, crate::notice::Tone::Warning, &said);
    }
}

/// A move of the book numbered as the list writes it ("34.Rd1", "34…Kf7") and in words.
fn numbered(notation: &crate::chess_figurines::Notation, number: u32, black: bool, san: &str) -> (String, String) {
    let n = if black { format!("{number}…") } else { format!("{number}.") };
    (format!("{n}{}", notation.san(san)), format!("{n} {}", crate::chess_figurines::spoken(san, "")))
}

/// The list by material and pawns (plan A9-N16b): the filter (the material's signature, either
/// side, and doubled, isolated or passed pawns), then the stretches of the book's PGN — the
/// positions in a row of a line that match, a game opening on the board at the first — and the
/// diagrams read that match.
fn pattern_list(ui: &mut egui::Ui, t: &Tokens, chess: &mut ChessState) {
    chess.in_book.pattern_shown = true;
    let filter = &mut chess.in_book.filter;
    ui.horizontal(|ui| {
        // Painted: the field carries the name for assistive technology.
        let name = tl!("Material");
        let font = egui::TextStyle::Body.resolve(ui.style());
        let w = crate::widgets::text_width(ui, name, &font);
        let (rect, _) = ui.allocate_exact_size(vec2(w, ui.spacing().interact_size.y), egui::Sense::hover());
        ui.painter().text(rect.left_center(), egui::Align2::LEFT_CENTER, name, font, t.text);
        let r = ui.add(crate::widgets::line(&mut filter.material).hint_text("KRP-KR").desired_width(120.0).char_limit(MAX_MATERIAL));
        // Named, as every focusable control is (plan A9-I11a): the hint is no name.
        ui.ctx().accesskit_node_builder(r.id, |node| node.set_label(name));
        if std::mem::take(&mut filter.focus) {
            r.request_focus();
        }
    });
    ui.add(crate::widgets::checkbox(&mut filter.either_side, tl!("Either side")))
        .on_hover_text(tl!("The material with the colours swapped is found too."));
    ui.horizontal_wrapped(|ui| {
        ui.label(tl!("Pawns:"));
        ui.add(crate::widgets::checkbox(&mut filter.doubled, tl!("Doubled")));
        ui.add(crate::widgets::checkbox(&mut filter.isolated, tl!("Isolated")));
        ui.add(crate::widgets::checkbox(&mut filter.passed, tl!("Passed")));
    });
    match filter.query() {
        Err(_) => {
            let why = tl!("Not a material signature: each side's pieces in the order K Q R B N P, White's first, as KRP-KR.");
            ui.add(egui::Label::new(egui::RichText::new(why).color(t.danger_text)).wrap());
            return;
        }
        Ok(None) => {
            let hint = tl!("Write a material, such as KRP-KR, or choose a kind of pawn.");
            ui.add(egui::Label::new(egui::RichText::new(hint).color(t.text_muted)).wrap());
            return;
        }
        Ok(Some(_)) => {}
    }
    let looking = tl!("Looking in the book…");
    let Some((answer, stale)) = answer_of(ui, t, chess.in_book.pattern_status(), looking) else { return };
    let doc = chess.in_book.pattern.doc();
    if answer.count() == 0 {
        let none = if stale { looking } else { tl!("Nothing in the book as read has this material and these pawns.") };
        ui.add(egui::Label::new(none).wrap());
        ui.add(egui::Label::new(egui::RichText::new(looked_at(&answer)).small().color(t.text_muted)).wrap());
        diagrams_failure(ui, &answer);
        return;
    }
    let found = if stale { looking.to_owned() } else { crate::i18n::fmt(tl!("Found: {n}"), &[("n", &count_text(answer.count()))]) };
    ui.add(egui::Label::new(egui::RichText::new(found).small().color(t.text_muted)).wrap());
    ui.add(egui::Label::new(egui::RichText::new(looked_at(&answer)).small().color(t.text_muted)).wrap());
    diagrams_failure(ui, &answer);
    section(ui, t, tl!("Where in the book"));
    let notation = crate::chess_figurines::Notation::current();
    let stretches = answer.stretches.as_ref().map_or(&[][..], |s| s.found.as_slice());
    for (k, s) in stretches.iter().enumerate() {
        let game = crate::i18n::fmt(tl!("Game {n}"), &[("n", &count_text(s.game.saturating_add(1)))]);
        let name = answer_game_name(chess, s.game);
        let named = if name.is_empty() { game } else { format!("{game} · {name}") };
        let (first, first_words) = numbered(&notation, s.first.number, s.first.black, &s.first.san);
        let (last, last_words) = numbered(&notation, s.last.number, s.last.black, &s.last.san);
        let title = if s.positions > 1 { format!("{named}: {first} – {last}") } else { format!("{named}: {first}") };
        let words = if s.positions > 1 {
            format!("{named}: {}", crate::i18n::fmt(tl!("{first} to {last}"), &[("first", &first_words), ("last", &last_words)]))
        } else {
            format!("{named}: {first_words}")
        };
        let mut details = vec![if s.main_line { tl!("Main line") } else { tl!("Variation") }.to_owned()];
        details.push(crate::i18n::fmt(tl!("Positions: {n}"), &[("n", &count_text(s.positions))]));
        if let Some(page) = doc.and_then(|d| game_page(chess, d, s.game)) {
            details.push(crate::i18n::fmt(tl!("from p. {page}"), &[("page", &page)]));
        }
        let (clicked, id) = ui
            .push_id(("chess-in-book-stretch", k), |ui| {
                let r = ui.add(egui::Button::new(egui::RichText::new(&title)).truncate().min_size(vec2(24.0, 24.0)));
                let open =
                    crate::i18n::fmt(tl!("Open game {n} on the board at {move}"), &[("n", &count_text(s.game.saturating_add(1))), ("move", &first)]);
                let r = r.on_hover_text(format!("{title}\n{open}"));
                let spoken = format!("{words}, {}", details.join(", "));
                r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &spoken));
                ui.add(egui::Label::new(egui::RichText::new(details.join(" · ")).small().color(t.text_muted)).truncate());
                (r.clicked(), r.id)
            })
            .inner;
        chess.in_book.rows.push(id);
        if clicked
            && !stale
            && let Some(doc) = doc
        {
            chess.in_book.asked = Some(BookAction::OpenGame { doc, game: s.game, node: s.node });
        }
    }
    diagram_rows(ui, t, chess, doc, &answer, stretches.len(), stale);
}

/// The name of the book's PGN's game `game`, as the answer's index keeps it.
fn answer_game_name(chess: &ChessState, game: usize) -> String {
    chess
        .in_book
        .pattern
        .now
        .as_ref()
        .and_then(|q| q.as_ref())
        .and_then(|q| q.index.as_deref())
        .and_then(|i| i.games().get(game))
        .map(|g| g.name.clone())
        .unwrap_or_default()
}

/// The list for the position the board shows (plan A9-N3).
fn position_list(ui: &mut egui::Ui, t: &Tokens, chess: &mut ChessState) {
    let looking = tl!("Looking for the position in the book…");
    let Some((answer, stale)) = answer_of(ui, t, chess.in_book.status(), looking) else { return };
    let doc = chess.in_book.doc();
    // What was looked at, and how often the position is there: secondary.
    let looked = looked_at(&answer);
    if answer.count() == 0 {
        let none = if stale { looking } else { tl!("This position does not appear in the book as read.") };
        ui.add(egui::Label::new(none).wrap());
        ui.add(egui::Label::new(egui::RichText::new(looked).small().color(t.text_muted)).wrap());
        diagrams_failure(ui, &answer);
        return;
    }
    let mut counts = vec![crate::i18n::fmt(tl!("Found: {n}"), &[("n", &count_text(answer.count()))])];
    if let Some(p) = answer.pgn.as_ref().filter(|p| p.lines > 1) {
        counts.push(crate::i18n::fmt(tl!("transpositions: {n}"), &[("n", &count_text(p.lines.saturating_sub(1)))]));
    }
    let counts = if stale { looking.to_owned() } else { counts.join(" · ") };
    ui.add(egui::Label::new(egui::RichText::new(counts).small().color(t.text_muted)).wrap());
    ui.add(egui::Label::new(egui::RichText::new(looked).small().color(t.text_muted)).wrap());
    diagrams_failure(ui, &answer);
    // What the book plays here.
    if !answer.next.is_empty() {
        section(ui, t, tl!("The book plays here"));
        let notation = crate::chess_figurines::Notation::current();
        let number = if answer.black { format!("{}…", answer.number) } else { format!("{}.", answer.number) };
        for (k, n) in answer.next.iter().enumerate() {
            let shown = format!("{number}{}{}", notation.san(&n.san), signs(&n.nags));
            let font = if n.main_line { theme::semibold(13.0) } else { egui::FontId::proportional(13.0) };
            let mut details = vec![if n.main_line { tl!("Main line") } else { tl!("Variation") }.to_owned()];
            if n.count > 1 {
                details.push(crate::i18n::fmt(tl!("Games: {n}"), &[("n", &count_text(n.count))]));
            }
            if let Some(page) = doc.and_then(|d| game_page(chess, d, n.game)) {
                details.push(crate::i18n::fmt(tl!("from p. {page}"), &[("page", &page)]));
            }
            // Named in words, as the moves are: "Play 3. bishop b5 on the board, Good move, Main
            // line, Games: 2"; one that can't be played says why instead.
            let words = format!("{number} {}", crate::chess_figurines::spoken(&n.san, ""));
            let mut rest = sign_names(&n.nags);
            let row = ui.push_id(("chess-in-book-next", k), |ui| {
                ui.horizontal(|ui| {
                    let button = egui::Button::new(egui::RichText::new(&shown).font(font)).min_size(vec2(48.0, 24.0));
                    let r = ui.add_enabled(n.play.is_ok(), button);
                    let (head, aside) = match n.play {
                        Ok(_) => {
                            rest.extend(details.iter().cloned());
                            (crate::i18n::fmt(tl!("Play {move} on the board"), &[("move", &words)]), details.join(" · "))
                        }
                        Err(why) => {
                            rest.push(refused_text(why).to_owned());
                            (words.clone(), refused_text(why).to_owned())
                        }
                    };
                    let name = std::iter::once(head).chain(rest.iter().cloned()).collect::<Vec<_>>().join(", ");
                    let r = match n.play {
                        Ok(_) => r.on_hover_text(&name),
                        Err(why) => r.on_disabled_hover_text(refused_text(why)),
                    };
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, n.play.is_ok(), &name));
                    ui.add(egui::Label::new(egui::RichText::new(aside).small().color(t.text_muted)).truncate());
                    (r.clicked(), r.id)
                })
                .inner
            });
            let (clicked, id) = row.inner;
            // Only a reply the board can play is reached by the keyboard.
            if n.play.is_ok() {
                chess.in_book.rows.push(id);
            }
            if clicked
                && !stale
                && let Ok(p) = n.play
            {
                chess.board_mut().play_typed(p.from, p.to, p.promotion);
            }
        }
        if let Some(read) = answer.games_cut {
            let note = crate::i18n::fmt(tl!("Counted in the first {n} games that reach it."), &[("n", &count_text(read))]);
            ui.add(egui::Label::new(egui::RichText::new(note).small().color(t.text_faint)).wrap());
        }
    }
    // Where: the PGN's moves to it, then the diagrams.
    section(ui, t, tl!("Where in the book"));
    if let Some(pgn) = &answer.pgn {
        let notation = crate::chess_figurines::Notation::current();
        for (k, h) in pgn.found.iter().enumerate() {
            let game = crate::i18n::fmt(tl!("Game {n}"), &[("n", &count_text(h.game.saturating_add(1)))]);
            // The move as the list writes it, and in words for its name, as the moves are named.
            let (mv, mv_words) = h.san.as_deref().map(|s| numbered(&notation, h.number, h.black, s)).unzip();
            let named = if h.name.is_empty() { game.clone() } else { format!("{game} · {}", h.name) };
            let with = |mv: &Option<String>| match mv {
                Some(mv) => format!("{named}: {mv}"),
                None => named.clone(),
            };
            let (title, title_words) = (with(&mv), with(&mv_words));
            let mut details = vec![if h.main_line { tl!("Main line") } else { tl!("Variation") }.to_owned()];
            if h.transposed {
                details.push(tl!("transposition").to_owned());
            }
            if let Some(page) = doc.and_then(|d| game_page(chess, d, h.game)) {
                details.push(crate::i18n::fmt(tl!("from p. {page}"), &[("page", &page)]));
            }
            let (clicked, id) = ui
                .push_id(("chess-in-book-pgn", k), |ui| {
                    let r = ui.add(egui::Button::new(egui::RichText::new(&title)).truncate().min_size(vec2(24.0, 24.0)));
                    let at = mv.clone().unwrap_or_else(|| tl!("Start of the game").to_owned());
                    let name =
                        crate::i18n::fmt(tl!("Open game {n} on the board at {move}"), &[("n", &count_text(h.game.saturating_add(1))), ("move", &at)]);
                    let r = r.on_hover_text(format!("{title}\n{name}"));
                    let spoken = format!("{title_words}, {}", details.join(", "));
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &spoken));
                    ui.add(egui::Label::new(egui::RichText::new(details.join(" · ")).small().color(t.text_muted)).truncate());
                    (r.clicked(), r.id)
                })
                .inner;
            chess.in_book.rows.push(id);
            if clicked
                && !stale
                && let Some(doc) = doc
            {
                chess.in_book.asked = Some(BookAction::OpenGame { doc, game: h.game, node: h.node });
            }
        }
    }
    let listed = answer.pgn.as_ref().map_or(0, |p| p.found.len());
    diagram_rows(ui, t, chess, doc, &answer, listed, stale);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two games reaching the same position by two move orders (plan A9-N3), and a third that
    /// castles from it.
    const BOOK: &str = "[Event \"Open\"]\n\n1.e4 e5 2.Nf3 Nc6 3.Bb5 $1 a6 *\n\n[Event \"Reti\"]\n\n1.Nf3 Nc6 2.e4 e5 3.Bc4 (3.Bb5 $6) *\n";

    fn query(fen: &str, pgn: &str) -> Query {
        Query {
            doc: DocId(1),
            look: Look::Position { fen: fen.to_owned(), legal: true },
            index: Some(Arc::new(ReferenceIndex::from_pgn(pgn).unwrap())),
            read: None,
            corrected: Vec::new(),
            pages: 3,
        }
    }

    fn fen_after(moves: &[&str]) -> String {
        let mut g = Game::standard();
        for m in moves {
            g.play_san(m).unwrap();
        }
        g.fen()
    }

    /// The position two move orders reach: found twice, the transposition counted, and what the
    /// book plays from it — Bb5 on the main line of the first game and in a variation of the
    /// second, with each game's sign; Bc4 on the second's main line.
    #[test]
    fn two_move_orders_are_found_and_the_replies_merged() {
        let a = look(&query(&fen_after(&["e4", "e5", "Nf3", "Nc6"]), BOOK), &JobCtl::default()).unwrap();
        let pgn = a.pgn.as_ref().unwrap();
        assert_eq!((pgn.total, pgn.lines, a.count()), (2, 2, 2));
        assert_eq!(
            pgn.found.iter().map(|h| (h.game, h.san.as_deref(), h.transposed)).collect::<Vec<_>>(),
            [(0, Some("Nc6"), false), (1, Some("e5"), true)]
        );
        let next: Vec<(&str, bool, usize, &[u16])> = a.next.iter().map(|n| (n.san.as_str(), n.main_line, n.count, n.nags.as_slice())).collect();
        assert_eq!(next, [("Bb5", true, 2, [1u16, 6].as_slice()), ("Bc4", true, 1, [].as_slice())]);
        assert!(a.next.iter().all(|n| n.play.is_ok()));
        assert_eq!((a.number, a.black), (3, false));
        assert_eq!(a.games_cut, None);
    }

    /// The same pieces with castling lost on the board: the book's O-O is listed and refused,
    /// saying why; a position the book does not reach is found nowhere, with the PGN looked at.
    #[test]
    fn a_reply_the_board_cannot_play_says_why() {
        let book = "[Event \"Castles\"]\n\n1.e4 e5 2.Nf3 Nc6 3.Bc4 Bc5 4.O-O Nf6 *\n";
        let lost = fen_after(&["e4", "e5", "Nf3", "Nc6", "Bc4", "Bc5", "Kf1", "Nf6", "Ke1", "Ng8"]);
        let a = look(&query(&lost, book), &JobCtl::default()).unwrap();
        assert_eq!(a.next.first().map(|n| (n.san.as_str(), n.play)), Some(("O-O", Err(Refused::Castling))));
        let kept = look(&query(&fen_after(&["e4", "e5", "Nf3", "Nc6", "Bc4", "Bc5"]), book), &JobCtl::default()).unwrap();
        assert!(kept.next.first().is_some_and(|n| n.play.is_ok()), "{:?}", kept.next);
        let none = look(&query(&fen_after(&["d4"]), book), &JobCtl::default()).unwrap();
        assert_eq!((none.count(), none.nothing_read()), (0, false));
    }

    /// En passant in the book, none on the board: refused as such.
    #[test]
    fn an_en_passant_the_board_cannot_take_says_why() {
        let book = "[Event \"Ep\"]\n\n1.e4 Nf6 2.e5 d5 3.exd6 *\n";
        // The same pieces and White to move, the pawn on d5 come there in two steps.
        let late = fen_after(&["e3", "Nf6", "e4", "d6", "e5", "d5"]);
        let a = look(&query(&late, book), &JobCtl::default()).unwrap();
        assert_eq!(a.pgn.as_ref().map(|p| p.total), Some(1), "{a:?}");
        assert_eq!(a.next.first().map(|n| (n.san.as_str(), n.play)), Some(("exd6", Err(Refused::EnPassant))));
    }

    /// The diagrams read: found by their key, the side compared unless assumed; a corrected one
    /// by the position the user gave it, not by its reading; no PGN and no page read is nothing
    /// read, never "not in the book".
    #[test]
    fn diagrams_are_found_by_their_reading_or_their_correction() {
        use pdfcraft_engine::chess::position_find::DiagramFound;
        let start = Fen::parse(&Game::standard().fen()).unwrap().book_key();
        let found = |page, index, side_assumed| (DiagramFound { page, index, side_assumed, from_disk: false }, start);
        let keys = DiagramKeys { diagrams: vec![found(0, 0, true), found(4, 1, false), found(6, 0, false)], pages_read: 7, from_disk: 0 };
        let read = Arc::new(ReadList { key: ReadKey { doc: DocId(1), stamp: None, revision: 0, disk: None }, keys: Ok(keys) });
        let mut q = Query { index: None, read: Some(read), ..query(&Game::standard().fen(), BOOK) };
        // Page 7's diagram corrected to the position after 1.e4.
        q.corrected = vec![(6, 0, fen_after(&["e4"]))];
        let a = look(&q, &JobCtl::default()).unwrap();
        assert_eq!(a.diagrams.iter().map(|d| (d.page, d.index, d.side_assumed)).collect::<Vec<_>>(), [(0, 0, true), (4, 1, false)]);
        assert_eq!((a.pages_read, a.pgn.is_none()), (7, true));
        let after = look(&Query { look: Look::Position { fen: fen_after(&["e4"]), legal: true }, ..q.clone() }, &JobCtl::default()).unwrap();
        assert_eq!(after.diagrams.iter().map(|d| (d.page, d.index)).collect::<Vec<_>>(), [(6, 0)]);
        // Black to move with the start's pieces: only the diagram whose side is assumed.
        let black = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR b KQkq - 0 1";
        let a = look(&Query { look: Look::Position { fen: black.to_owned(), legal: true }, ..q.clone() }, &JobCtl::default()).unwrap();
        assert_eq!(a.diagrams.iter().map(|d| d.page).collect::<Vec<_>>(), [0]);
        // Nothing read.
        let empty = DiagramKeys::default();
        let read = Arc::new(ReadList { key: ReadKey { doc: DocId(1), stamp: None, revision: 0, disk: None }, keys: Ok(empty) });
        let a = look(&Query { read: Some(read), corrected: Vec::new(), ..q }, &JobCtl::default()).unwrap();
        assert!(a.nothing_read());
    }

    /// Never a panic: a FEN that is not one, a list that could not be made, and a book's PGN
    /// whose position the board cannot reach.
    #[test]
    fn bad_input_is_an_error_not_a_panic() {
        assert!(look(&query("not a fen", BOOK), &JobCtl::default()).is_err());
        let illegal =
            Query { look: Look::Position { fen: "8/8/8/8/8/8/8/8 w - - 0 1".to_owned(), legal: false }, ..query("8/8/8/8/8/8/8/8 w - - 0 1", BOOK) };
        assert_eq!(look(&illegal, &JobCtl::default()).unwrap().count(), 0);
        assert_eq!(promotion_of("e7e8q"), Some(PieceKind::Queen));
        assert_eq!(promotion_of("e2e4"), None);
        assert_eq!(promotion_of("é"), None);
    }

    /// Plan A9-N3b: the diagrams read that could not be listed (a document that does not allow
    /// copying, a reopening that failed) never hide the book's PGN: it answers, and the diagrams'
    /// failure is said beside it. With no PGN either, the failure is the answer.
    #[test]
    fn a_list_of_diagrams_that_failed_leaves_the_pgn_answering() {
        let failed =
            || Some(Arc::new(ReadList { key: ReadKey { doc: DocId(1), stamp: None, revision: 0, disk: None }, keys: Err("no copying".into()) }));
        let q = Query { read: failed(), ..query(&fen_after(&["e4", "e5", "Nf3", "Nc6"]), BOOK) };
        let a = look(&q, &JobCtl::default()).unwrap();
        assert_eq!((a.pgn.as_ref().map(|p| p.total), a.diagrams_error.as_deref()), (Some(2), Some("no copying")));
        let mut slot = Slot { now: Some(Some(q.clone())), answer: Some((q, Ok(Arc::new(a)))), asking: None };
        assert!(matches!(slot.status(), Status::Found(_)), "the PGN answers");
        let alone = Query { index: None, read: failed(), ..query(&Game::standard().fen(), BOOK) };
        let a = look(&alone, &JobCtl::default()).unwrap();
        slot = Slot { now: Some(Some(alone.clone())), answer: Some((alone, Ok(Arc::new(a)))), asking: None };
        assert!(matches!(slot.status(), Status::Failed(why) if why == "no copying"), "nothing else to show");
    }

    /// Plan A9-N3b: a search for a position no longer shown is dropped (which asks it to stop) and
    /// the newest takes its place at once; the same one is never asked twice. While it runs, the
    /// last answer in the same book is what the list shows, dimmed.
    #[test]
    fn the_newest_search_takes_the_place_of_one_running() {
        let read =
            || Some(Arc::new(ReadList { key: ReadKey { doc: DocId(1), stamp: None, revision: 0, disk: None }, keys: Ok(DiagramKeys::default()) }));
        let (a, b) = (Query { read: read(), ..query(&fen_after(&["e4"]), BOOK) }, Query { read: read(), ..query(&fen_after(&["d4"]), BOOK) });
        // Jobs that never end: each waits for a message no one sends.
        let mut senders = Vec::new();
        let mut gated = |_: Query| {
            let (go, gate) = std::sync::mpsc::channel::<()>();
            senders.push(go);
            Job::spawn("in-book-test", false, move |_| gate.recv().map(|()| Answer::default()).map_err(|e| e.to_string()))
        };
        let mut slot = Slot::default();
        assert!(slot.ask(a.clone(), &mut gated));
        assert!(!slot.ask(a.clone(), &mut gated), "the same one, running: not asked again");
        assert!(slot.ask(b.clone(), &mut gated), "the newest at once");
        assert!(slot.asking.as_ref().is_some_and(|(q, _)| *q == b));
        // The last answer stays while the next is looked for, in the same book only.
        let found = Arc::new(Answer { pgn: Some(PgnFound { total: 1, lines: 1, found: Vec::new() }), ..Answer::default() });
        slot.answer = Some((a, Ok(found)));
        assert!(matches!(slot.status(), Status::Searching(Some(_))));
        slot.now = Some(Some(Query { doc: DocId(2), ..b }));
        assert!(matches!(slot.status(), Status::Searching(None)), "another book's answer is not shown");
        drop(senders);
    }

    /// Plan A9-N3b: while a scan of the document runs, its batches move the cache's revision; the
    /// diagrams read are listed then only when there is no list of the document at all, and again
    /// once it ends.
    #[test]
    fn the_diagrams_are_listed_again_after_a_scan_not_at_each_batch() {
        assert!(list_again(false, false, false) && list_again(false, true, false), "a new state of what is read");
        assert!(!list_again(true, true, false) && !list_again(true, true, true), "the list is for it");
        assert!(!list_again(false, true, true), "a scan running: the list it has");
        assert!(list_again(false, false, true), "a scan running and no list yet: one now");
    }

    /// Plan A9-N3b: a search that failed is said in the interface's language, the reason after it.
    #[test]
    fn a_failure_is_said_in_the_interfaces_language() {
        use crate::i18n::{self, Lang};
        let pt = Lang::from_code("pt-br").expect("pt-BR registered");
        i18n::set_current(pt);
        let said = failure_text("no copying");
        i18n::set_current(Lang::EN);
        assert_eq!(said, "Não foi possível procurar no livro: no copying");
        assert_eq!(failure_text("x"), "The book couldn't be searched: x");
    }

    /// The signs of two games on one reply: both, the second parted by a slash; an assessment of
    /// the position after a space.
    #[test]
    fn two_games_signs_are_both_written() {
        assert_eq!(signs(&[1, 6]), "!/?!");
        assert_eq!(signs(&[3, 16]), format!("!! {}", crate::chess_board_ui::nag_text(16)));
        assert_eq!(signs(&[]), "");
    }

    /// Plan A9-N16b: by material, the book's PGN's stretches (the colours swapped too with Either
    /// side) and the diagrams read whose pieces have it, held by their keys; a filter asking for
    /// nothing, or a material that is not a signature, makes no query.
    #[test]
    fn by_material_the_stretches_and_the_diagrams_are_found() {
        use pdfcraft_engine::chess::position_find::DiagramFound;
        let pgn = "[Event \"Rook ending\"]\n[SetUp \"1\"]\n[FEN \"6k1/8/8/3P4/8/8/r7/4K2R w - - 0 1\"]\n\n1.d6 Ra8 2.d7 Rd8 *\n\n\
            [Event \"Twin\"]\n[SetUp \"1\"]\n[FEN \"4k2r/R7/8/8/3p4/8/8/6K1 b - - 0 1\"]\n\n1...d3 2.Ra1 d2 *\n\n\
            [Event \"Queen\"]\n[SetUp \"1\"]\n[FEN \"6k1/5ppp/8/8/8/8/5PPP/3Q2K1 w - - 0 1\"]\n\n1.Qd8# 1-0\n";
        let rook = Fen::parse("6k1/8/8/3P4/8/8/r7/4K2R w - - 0 1").unwrap().book_key();
        let start = Fen::parse(&Game::standard().fen()).unwrap().book_key();
        let found = |page, key| (DiagramFound { page, index: 0, side_assumed: true, from_disk: false }, key);
        let keys = DiagramKeys { diagrams: vec![found(3, rook), found(5, start)], pages_read: 6, from_disk: 0 };
        let read = Arc::new(ReadList { key: ReadKey { doc: DocId(1), stamp: None, revision: 0, disk: None }, keys: Ok(keys) });
        let mut filter = Filter { material: " KRP-KR ".into(), ..Filter::default() };
        let base = Query { read: Some(read), ..query(&Game::standard().fen(), pgn) };
        let ask = |filter: &Filter| {
            let wanted = filter.query().unwrap().unwrap();
            look(&Query { look: Look::Pattern(wanted), ..base.clone() }, &JobCtl::default()).unwrap()
        };
        let a = ask(&filter);
        let games: Vec<usize> = a.stretches.as_ref().unwrap().found.iter().map(|s| s.game).collect();
        assert_eq!((games, a.diagrams.iter().map(|d| d.page).collect::<Vec<_>>(), a.count()), (vec![0], vec![3], 2));
        filter.either_side = true;
        let a = ask(&filter);
        assert_eq!(a.stretches.as_ref().unwrap().found.iter().map(|s| (s.game, s.positions)).collect::<Vec<_>>(), [(0, 4), (1, 3)]);
        // Passed pawns alone: every position with one (the rook endings').
        let passed = Filter { passed: true, ..Filter::default() };
        assert_eq!(ask(&passed).stretches.map(|s| s.total), Some(2));
        assert!(Filter { material: "KRP".into(), ..Filter::default() }.query().is_err());
        assert_eq!(Filter::default().query(), Ok(None));
        assert_eq!(Filter { material: "   ".into(), ..Filter::default() }.query(), Ok(None));
    }
}
