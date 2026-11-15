//! Chess: the book says × the engine says, in a batch (plan A9-N17a, finding A8-10; R6
//! "Recomendado" 1 and 8).
//!
//! **The report** ([`divergences`]) is one list for the solver and the engine: a `!` the engine
//! refutes, a `?` it finds best, a `+−` it calls `=`, a caption's "White to play and win" with the
//! engine at −3 (a misread square, most often), an exercise with another winning line, and the
//! mate the page asks that the solver found none of (plan A9-N4: [`ReviewFlag::NoMate`], and
//! [`ReviewFlag::ManyKeys`]). It reads only what is kept: the reading's flags, and the evaluations
//! of the evaluations store ([`super::evals`]), judged by [`pdfcraft_chess::judge`] with the
//! caller's [`Limits`]. A move the book's PGN plays is judged where the PGN plays it (its
//! references, [`pdfcraft_chessvision::doc::reference::link_all`]): a move the reader could not play,
//! or played from the wrong position, has the PGN's.
//!
//! **The batch** ([`Batch`], [`run`]) asks the engine for what the report needs, each position once
//! (by the store's key), in this order: the games' main lines, the positions around a move with a
//! judged sign, the diagrams. What the store holds at the depth asked is left out; an exercise's
//! position (a numbered diagram) is asked for two lines. [`run`] drives an [`Analyser`] the caller
//! started (the interface's, with the engine's settings) position by position, and hands the
//! evaluations back in chunks for the caller to keep; stopped, the batch keeps what it has not
//! asked yet, so running it again asks only that. No tool starts an engine
//! ([`super::analysis`]): only the interface runs a batch.

use std::collections::{HashMap, VecDeque};
use std::fmt::Write as _;

use pdfcraft_chess::fen::Fen;
use pdfcraft_chess::game::Game as Played;
use pdfcraft_chess::judge::{self, Band, Judged, Limits, Outcome, Value, Verdict};
use pdfcraft_chess::placement::Color;
use pdfcraft_chess::stipulation::Goal;
use pdfcraft_chessvision::doc::games::GameScore;
use pdfcraft_chessvision::doc::reference::{LinkKind, RefNode, ReferenceIndex};
use pdfcraft_chessvision::doc::{Block, ChessDoc, DiagramAt, Inline, MoveAt, MoveInline, ReviewFlag};

use super::evals::{EvalFile, MAX_EVALS, position_key};

/// The start position, for a game that names none.
const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

/// Most moves before a position given with it: the engine's ([`super::analysis::MAX_HISTORY`]).
const MAX_HISTORY: usize = 100;

/// Most divergences one report holds; the rest are counted ([`Review::over`]).
pub const MAX_DIVERGENCES: usize = 10_000;

/// Where a divergence is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Place {
    Move(MoveAt),
    Diagram(DiagramAt),
}

/// What the book says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Said {
    /// A sign printed on the move (its PGN code).
    Sign(u16),
    /// A goal its caption prints for the side to play.
    Goal { goal: Goal, side: Color },
}

/// What the computation found against it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Found {
    /// A `!` the engine refutes: the centipawns it loses to the best move (`None`: a mate lost).
    Refuted { loss: Option<i32> },
    /// A `?` the engine finds best, or nearly: what it loses.
    Questioned { loss: i32 },
    /// An evaluation sign two bands or more from the engine's ([`Band`], for White).
    Contradicted { band: Band },
    /// A goal the engine does not bear out ([`Band`], for White).
    Goal { band: Band },
    /// An exercise whose second line wins too.
    OtherSolution,
    /// The solver found no mate in the moves the page asks (plan A9-N4).
    NoMate,
    /// The solver found more than one key.
    ManyKeys,
}

impl Found {
    /// The finding's name in the tools' replies.
    pub fn name(self) -> &'static str {
        match self {
            Found::Refuted { .. } => "refuted",
            Found::Questioned { .. } => "questioned",
            Found::Contradicted { .. } => "contradicted",
            Found::Goal { .. } => "goal_contradicted",
            Found::OtherSolution => "other_solution",
            Found::NoMate => "no_mate",
            Found::ManyKeys => "many_keys",
        }
    }

    /// Every finding's name, in the order a report counts them.
    pub const NAMES: [&'static str; 7] = ["refuted", "questioned", "contradicted", "goal_contradicted", "other_solution", "no_mate", "many_keys"];
}

/// Who computed it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    /// The exact mate solver of plan A9-N4.
    Solver,
    /// An engine the app ran (the evaluations store).
    Engine,
    /// A PGN ChessBase analysed (plan A9-N17c; not read yet).
    ChessBase,
}

impl Source {
    pub fn name(self) -> &'static str {
        match self {
            Source::Solver => "solver",
            Source::Engine => "engine",
            Source::ChessBase => "chessbase",
        }
    }
}

/// What the engine said, for a divergence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineSaid {
    /// After the move (a move's), or in the diagram's position, for White.
    pub value: Value,
    /// The shallowest depth the verdict rests on.
    pub depth: u32,
    /// The engine's best move instead (SAN, numbered as the book numbers it) and its value for
    /// White; `None` when the move was the best, or for a diagram.
    pub best: Option<(String, Value)>,
    /// An exercise's second line: its first move (SAN) and its value for White.
    pub second: Option<(String, Value)>,
    /// The engine's name, as it gave it.
    pub engine: Option<String>,
}

/// What the solver said, for a divergence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SolverSaid {
    pub n: u8,
    /// With no key: the first move the fewest defences refute, and one of them (SAN).
    pub best_try: Option<String>,
    pub refutation: Option<String>,
    /// With more than one: the keys (SAN).
    pub keys: Vec<String>,
}

/// What the computation says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Computed {
    Engine(EngineSaid),
    Solver(SolverSaid),
}

/// One place where the book and the computation disagree (plan A9-N17a): the page (0-based; the
/// tools say it as the book numbers it), the move or the diagram, what the book says, what was
/// found and what computed it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Divergence {
    pub page: u32,
    pub at: Place,
    /// The move as printed with its number (`12.Nf3!`), or `diagram n` (from 1).
    pub printed: String,
    pub book: Said,
    pub found: Found,
    pub computed: Computed,
    pub source: Source,
    /// The diagram's side to play was only assumed (White): the verdict may be the wrong side's.
    pub side_assumed: bool,
}

/// The report: the divergences in reading order, and what could not be judged.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Review {
    pub divergences: Vec<Divergence>,
    /// Signs and goals judged (agreeing or not).
    pub judged: usize,
    /// Signs and goals with an evaluation missing.
    pub no_eval: usize,
    /// Signs and goals whose evaluation is shallower than the limit.
    pub shallow: usize,
    /// Divergences past [`MAX_DIVERGENCES`], counted, not listed.
    pub over: usize,
}

impl Review {
    fn push(&mut self, d: Divergence) {
        if self.divergences.len() < MAX_DIVERGENCES {
            self.divergences.push(d);
        } else {
            self.over = self.over.saturating_add(1);
        }
    }

    fn count(&mut self, outcome: &Outcome) {
        match outcome {
            Outcome::Unsigned => {}
            Outcome::NoEval => self.no_eval = self.no_eval.saturating_add(1),
            Outcome::Shallow { .. } => self.shallow = self.shallow.saturating_add(1),
            Outcome::Judged(_) => self.judged = self.judged.saturating_add(1),
        }
    }
}

/// A position and how a game reached it: its start and the moves after it (SAN, at most
/// [`MAX_HISTORY`]), so the engine sees a repetition coming.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Reached {
    fen: String,
    start: String,
    moves: Vec<String>,
}

impl Reached {
    /// A position alone.
    fn alone(fen: String) -> Reached {
        Reached { start: fen.clone(), fen, moves: Vec::new() }
    }
}

/// A printed move with a judged sign.
struct MoveSite<'a> {
    at: MoveAt,
    m: &'a MoveInline,
    nags: Vec<u16>,
    before: Reached,
    after: Reached,
    /// The move in SAN, as the game (or the book's PGN) plays it.
    san: String,
}

/// The position after node `at` of `score` (`None`: its start), with the moves that led there.
/// `None` when the game does not have the node.
fn reached(score: &GameScore, at: Option<usize>) -> Option<Reached> {
    let start = || score.start_fen.clone().unwrap_or_else(|| START.to_owned());
    let fen = match at {
        None => start(),
        Some(n) => score.nodes.get(n)?.fen.clone(),
    };
    let mut moves = Vec::new();
    let mut back = at;
    // The parents are untrusted (a document read back): never more steps than the history.
    while let Some(n) = back {
        if moves.len() >= MAX_HISTORY {
            break;
        }
        let node = score.nodes.get(n)?;
        moves.push(node.san.clone());
        back = node.parent;
    }
    let from = match back {
        None => start(),
        Some(n) => score.nodes.get(n)?.fen.clone(),
    };
    moves.reverse();
    Some(Reached { fen, start: from, moves })
}

/// The signs printed right after the inline at `i`, as PGN codes.
fn nags_after(inlines: &[Inline], i: usize) -> Vec<u16> {
    inlines
        .iter()
        .skip(i.saturating_add(1))
        .map_while(|x| match x {
            Inline::Nag { code, .. } => Some(*code),
            _ => None,
        })
        .take(pdfcraft_chessvision::doc::games::MAX_NAGS)
        .collect()
}

/// Where the book's PGN plays move `m`, when it was linked to one of `refs` and the reader could
/// not play it or played it from another position: the position before and after it there, and its
/// SAN.
fn by_reference(m: &MoveInline, refs: &[&ReferenceIndex]) -> Option<(Reached, Reached, String)> {
    let r = m.reference.as_ref().filter(|r| matches!(r.kind, LinkKind::Recovered | LinkKind::Conflict))?;
    let at = RefNode { game: r.game?, node: r.node? };
    let index = refs.get(r.source)?;
    let before = index.fen_before(at)?.to_string();
    let after = index.fen(at)?.to_string();
    let san = index.san(at)?.to_owned();
    Some((Reached::alone(before), Reached::alone(after), san))
}

/// The moves of `doc` with a sign [`judge`] judges, in reading order, each where it was played.
fn move_sites<'a>(doc: &'a ChessDoc, refs: &[&ReferenceIndex]) -> Vec<MoveSite<'a>> {
    let mut out = Vec::new();
    for page in &doc.pages {
        for (b, block) in page.book_blocks() {
            let inlines = block.inlines();
            for (i, inline) in inlines.iter().enumerate() {
                let Inline::Move(m) = inline else { continue };
                let nags = nags_after(inlines, i);
                if !nags.iter().any(|n| judge::judged_sign(*n)) {
                    continue;
                }
                let played = by_reference(m, refs).or_else(|| {
                    let r = m.game?;
                    let score = &doc.games.get(r.game)?.score;
                    let node = score.nodes.get(r.node)?;
                    Some((reached(score, node.parent)?, reached(score, Some(r.node))?, node.san.clone()))
                });
                if let Some((before, after, san)) = played {
                    out.push(MoveSite { at: MoveAt::new(page.index, b, i), m, nags, before, after, san });
                }
            }
        }
    }
    out
}

/// A diagram of `doc` and the position it shows.
struct DiagramSite<'a> {
    at: DiagramAt,
    d: &'a pdfcraft_chessvision::doc::DiagramBlock,
    fen: String,
    side_assumed: bool,
}

fn diagram_sites(doc: &ChessDoc) -> Vec<DiagramSite<'_>> {
    let mut out = Vec::new();
    for page in &doc.pages {
        for block in &page.blocks {
            let Block::Diagram(d) = block else { continue };
            let Some(shown) = d.shown() else { continue };
            out.push(DiagramSite { at: DiagramAt::on(page.index, d.number), d, fen: shown.fen.to_owned(), side_assumed: !shown.side_known });
        }
    }
    out
}

/// The side to move of `fen`; White when it does not say.
fn side_of(fen: &str) -> Color {
    Fen::parse(fen).ok().and_then(|f| f.side()).unwrap_or(Color::White)
}

/// The value for White of a kept evaluation of a position with `side` to move.
fn value_of(cp: Option<i32>, mate: Option<i32>, side: Color) -> Option<Value> {
    match (cp, mate) {
        (Some(cp), _) => Some(Value::Cp(cp)),
        (None, Some(n)) if n > 0 => Some(Value::Mates(n.unsigned_abs())),
        (None, Some(n)) if n < 0 => Some(Value::Mated(n.unsigned_abs())),
        // Mate 0: the side to move is mated.
        (None, Some(_)) => Some(if side == Color::White { Value::Mated(0) } else { Value::Mates(0) }),
        (None, None) => None,
    }
}

/// The value for White of a position known without a search (no legal move: mated or
/// stalemated); `None` when it has a move, or cannot be played.
fn exact(fen: &str) -> Option<Value> {
    let game = Played::from_fen(fen).ok()?;
    if !game.legal_moves().is_empty() {
        return None;
    }
    Some(match (game.in_check(), game.side_to_move()) {
        (true, Color::White) => Value::Mated(0),
        (true, Color::Black) => Value::Mates(0),
        (false, _) => Value::Cp(0),
    })
}

/// What the store says of the position `fen`, for White: the evaluation, its best move (UCI) and
/// the engine; a position with no move is known exactly.
struct Kept<'a> {
    eval: judge::Eval,
    best: Option<&'a str>,
    second: Option<(&'a str, Value)>,
    engine: Option<&'a str>,
}

fn kept<'a>(evals: &'a EvalFile, fen: &str) -> Option<Kept<'a>> {
    if let Some(value) = exact(fen) {
        return Some(Kept { eval: judge::Eval::new(value, judge::Eval::EXACT), best: None, second: None, engine: None });
    }
    let side = side_of(fen);
    let e = evals.get(fen)?;
    let value = value_of(e.cp, e.mate, side)?;
    let second = e.second.as_ref().and_then(|s| Some((s.pv.first()?.as_str(), value_of(s.cp, s.mate, side)?)));
    Some(Kept { eval: judge::Eval::new(value, e.depth), best: e.pv.first().map(String::as_str), second, engine: e.engine.as_deref() })
}

/// `eval` (for White) for `side`.
fn for_side(eval: judge::Eval, side: Color) -> judge::Eval {
    if side == Color::White { eval } else { eval.other() }
}

/// Move `uci` played from `fen`, in SAN numbered as a book prints it (`12.Bd3`, `12...Nf6`).
fn numbered_san(fen: &str, uci: &str) -> Option<String> {
    let san = pdfcraft_chess::uci::pv_san(fen, &[uci.to_owned()]).into_iter().next()?;
    let parsed = Fen::parse(fen).ok()?;
    let number = parsed.fullmove_number().max(1);
    Some(match parsed.side() {
        Some(Color::Black) => format!("{number}...{san}"),
        _ => format!("{number}.{san}"),
    })
}

/// Move `san` played from `fen`, in UCI.
fn uci_of(fen: &str, san: &str) -> Option<String> {
    let mut game = Played::from_fen(fen).ok()?;
    game.play_san(san).ok()?;
    game.uci(game.current())
}

/// The divergences of `doc` (read, and linked to `refs` when there are references: the book's
/// PGN, the user's games), from the reading's flags and the evaluations `evals` keeps, by
/// `limits`; in reading order, a page's moves before its diagrams.
pub fn divergences(doc: &ChessDoc, refs: &[&ReferenceIndex], evals: &EvalFile, limits: &Limits) -> Review {
    let mut review = Review::default();
    // Both lists are in the pages' order: each page's moves, then its diagrams.
    let mut moves = move_sites(doc, refs).into_iter().peekable();
    let mut diagrams = diagram_sites(doc).into_iter().peekable();
    for page in doc.pages.iter().map(|p| p.index) {
        while let Some(site) = moves.next_if(|s| s.at.page == page) {
            judge_move(&site, evals, limits, &mut review);
        }
        while let Some(site) = diagrams.next_if(|s| s.at.page_index() == page) {
            judge_diagram(&site, evals, limits, &mut review);
        }
    }
    review
}

fn judge_move(site: &MoveSite<'_>, evals: &EvalFile, limits: &Limits, review: &mut Review) {
    let mover = side_of(&site.before.fen);
    let Some(played) = uci_of(&site.before.fen, &site.san) else {
        review.no_eval = review.no_eval.saturating_add(1);
        return;
    };
    let before = kept(evals, &site.before.fen);
    let after = kept(evals, &site.after.fen);
    let judged = Judged {
        nags: &site.nags,
        mover,
        played: &played,
        before: before.as_ref().and_then(|k| Some((for_side(k.eval, mover), k.best?))),
        after: after.as_ref().map(|k| for_side(k.eval, mover)),
    };
    let outcome = judge::judge(&judged, limits);
    review.count(&outcome);
    let Outcome::Judged(verdicts) = outcome else { return };
    let printed = format!("{}{}", site.m.number.map(|n| n.text()).unwrap_or_default(), site.m.text);
    let printed = site.nags.iter().fold(printed, |mut p, n| {
        p.push_str(&pdfcraft_chess::nag::shown(*n));
        p
    });
    for verdict in verdicts {
        let (book, found) = match verdict {
            Verdict::Refuted { nag, loss } => (Said::Sign(nag), Found::Refuted { loss }),
            Verdict::Questioned { nag, loss } => (Said::Sign(nag), Found::Questioned { loss }),
            Verdict::Contradicted { nag, found } => (Said::Sign(nag), Found::Contradicted { band: found }),
            Verdict::Goal { .. } => continue,
        };
        // What the engine said: the position after the move, its best move instead when another.
        let value = after.as_ref().map_or_else(|| before.as_ref().map_or(Value::Cp(0), |k| k.eval.value), |k| k.eval.value);
        let depth = searched([before.as_ref(), after.as_ref()].into_iter().flatten().map(|k| k.eval.depth));
        let best =
            before.as_ref().and_then(|k| k.best.filter(|b| *b != played).and_then(|b| Some((numbered_san(&site.before.fen, b)?, k.eval.value))));
        let engine = before.as_ref().and_then(|k| k.engine).or_else(|| after.as_ref().and_then(|k| k.engine)).map(str::to_owned);
        review.push(Divergence {
            page: site.at.page,
            at: Place::Move(site.at),
            printed: printed.clone(),
            book,
            found,
            computed: Computed::Engine(EngineSaid { value, depth, best, second: None, engine }),
            source: Source::Engine,
            side_assumed: false,
        });
    }
}

fn judge_diagram(site: &DiagramSite<'_>, evals: &EvalFile, limits: &Limits, review: &mut Review) {
    let printed = format!("diagram {}", site.d.number.saturating_add(1));
    let side = side_of(&site.fen);
    let solver_n = match site.d.goal {
        Some(Goal::Mate(n)) => Some(n),
        _ => None,
    };
    // The solver's word first: it is exact (plan A9-N4).
    for flag in site.d.open_review() {
        let (found, said) = match flag {
            ReviewFlag::NoMate { n, best_try, refutation } => {
                (Found::NoMate, SolverSaid { n: *n, best_try: best_try.clone(), refutation: refutation.clone(), keys: Vec::new() })
            }
            ReviewFlag::ManyKeys { n, keys } => (Found::ManyKeys, SolverSaid { n: *n, best_try: None, refutation: None, keys: keys.clone() }),
            _ => continue,
        };
        review.judged = review.judged.saturating_add(1);
        review.push(Divergence {
            page: site.at.page_index(),
            at: Place::Diagram(site.at),
            printed: printed.clone(),
            book: Said::Goal { goal: Goal::Mate(said.n), side },
            found,
            computed: Computed::Solver(said),
            source: Source::Solver,
            side_assumed: site.side_assumed,
        });
    }
    let position = kept(evals, &site.fen);
    let engine_said = |k: &Kept<'_>, second: Option<(String, Value)>| EngineSaid {
        value: k.eval.value,
        depth: searched([k.eval.depth]),
        best: None,
        second,
        engine: k.engine.map(str::to_owned),
    };
    // A goal the solver did not settle (a mate it could not solve: more moves than it takes).
    if let Some(goal) = site.d.goal.filter(|_| solver_n.is_none_or(|n| n > pdfcraft_chess::mate::MAX_MATE_MOVES)) {
        let outcome = judge::judge_goal(goal, side, position.as_ref().map(|k| for_side(k.eval, side)), limits);
        review.count(&outcome);
        if let (Outcome::Judged(verdicts), Some(k)) = (&outcome, position.as_ref()) {
            for verdict in verdicts {
                if let Verdict::Goal { goal, found } = verdict {
                    review.push(Divergence {
                        page: site.at.page_index(),
                        at: Place::Diagram(site.at),
                        printed: printed.clone(),
                        book: Said::Goal { goal: *goal, side },
                        found: Found::Goal { band: *found },
                        computed: Computed::Engine(engine_said(k, None)),
                        source: Source::Engine,
                        side_assumed: site.side_assumed,
                    });
                }
            }
        }
    }
    // An exercise with a second winning line (not a draw's, nor a mate the solver judged).
    let exercise = site.d.puzzle.is_some() && !matches!(site.d.goal, Some(Goal::Draw | Goal::HelpMate(_) | Goal::SelfMate(_)));
    if exercise
        && solver_n.is_none_or(|n| n > pdfcraft_chess::mate::MAX_MATE_MOVES)
        && let Some(k) = position.as_ref().filter(|k| k.eval.depth >= limits.min_depth)
        && let Some((first, second)) = k.second
        && judge::other_solution(for_side(k.eval, side).value, for_white_to_side(second, side), limits)
    {
        let said = numbered_san(&site.fen, first).map(|s| (s, second));
        review.push(Divergence {
            page: site.at.page_index(),
            at: Place::Diagram(site.at),
            printed,
            book: Said::Goal { goal: site.d.goal.unwrap_or(Goal::Win), side },
            found: Found::OtherSolution,
            computed: Computed::Engine(engine_said(k, said)),
            source: Source::Engine,
            side_assumed: site.side_assumed,
        });
    }
}

/// The shallowest of the searches' `depths`: a position known without one does not count (0 when
/// none was searched).
fn searched(depths: impl IntoIterator<Item = u32>) -> u32 {
    depths.into_iter().filter(|d| *d != judge::Eval::EXACT).min().unwrap_or(0)
}

/// A value for White, for `side`.
fn for_white_to_side(value: Value, side: Color) -> Value {
    if side == Color::White { value } else { value.other() }
}

/// A value for White as a book reads it: `+0.50`, `−1.20`, `#3` (White mates), `#−3`.
pub fn value_text(value: Value) -> String {
    match value {
        Value::Cp(cp) => {
            let pawns = f64::from(cp) / 100.0;
            if cp < 0 { format!("\u{2212}{:.2}", -pawns) } else { format!("+{pawns:.2}") }
        }
        Value::Mates(n) => format!("#{n}"),
        Value::Mated(n) => format!("#\u{2212}{n}"),
    }
}

/// A goal in words, for the side to play.
fn goal_words(goal: Goal, side: Color) -> String {
    let who = if side == Color::White { "White" } else { "Black" };
    match goal {
        Goal::Win => format!("{who} to play and win"),
        Goal::Draw => format!("{who} to play and draw"),
        Goal::Mate(n) => format!("{who} to play, mate in {n}"),
        Goal::HelpMate(n) => format!("helpmate in {n}"),
        Goal::SelfMate(n) => format!("selfmate in {n}"),
    }
}

impl Divergence {
    /// What the book says, in words (English, as the engine's notes are).
    pub fn book_words(&self) -> String {
        match self.book {
            Said::Sign(nag) => pdfcraft_chess::nag::shown(nag),
            Said::Goal { goal, side } => goal_words(goal, side),
        }
    }

    /// What the computation says, in words (English).
    pub fn computed_words(&self) -> String {
        match &self.computed {
            Computed::Solver(s) => match self.found {
                Found::ManyKeys => format!("the solver finds {} keys: {}", s.keys.len(), s.keys.join(", ")),
                _ => {
                    let mut out = format!("the solver finds no mate in {}", s.n);
                    if let (Some(t), Some(r)) = (&s.best_try, &s.refutation) {
                        let _ = write!(out, "; best try {t}, refuted by {r}");
                    }
                    out
                }
            },
            Computed::Engine(e) => {
                let mut out = match self.found {
                    Found::Refuted { loss: None } => format!("it lets a mate go: {}", value_text(e.value)),
                    Found::Refuted { loss: Some(l) } => format!("it loses {:.2} pawns: {}", f64::from(l) / 100.0, value_text(e.value)),
                    Found::Questioned { loss: 0 } => format!("it is the engine's best move: {}", value_text(e.value)),
                    Found::Questioned { loss } => format!("it loses only {:.2} pawns: {}", f64::from(loss) / 100.0, value_text(e.value)),
                    Found::Contradicted { band } | Found::Goal { band } => format!("{} ({})", value_text(e.value), band.sign()),
                    Found::OtherSolution => format!("best line {}", value_text(e.value)),
                    Found::NoMate | Found::ManyKeys => value_text(e.value),
                };
                if let Some((san, v)) = &e.best {
                    let _ = write!(out, "; best {san} {}", value_text(*v));
                }
                if let Some((san, v)) = &e.second {
                    let _ = write!(out, "; also {san} {}", value_text(*v));
                }
                let _ = write!(out, " (depth {}", e.depth);
                if let Some(name) = &e.engine {
                    let _ = write!(out, ", {name}");
                }
                out.push(')');
                out
            }
        }
    }
}

/// What a batch asks of a position.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Part {
    /// A game's main line.
    MainLine,
    /// Before or after a move with a judged sign.
    Signed,
    /// A diagram's position.
    Diagram,
}

/// A position a batch asks the engine for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asked {
    /// The full FEN.
    pub fen: String,
    /// The store's key of it ([`position_key`]).
    pub key: String,
    /// Lines asked for: 2 for an exercise's position, else 1.
    pub lines: u32,
    pub part: Part,
    /// How a game reached it, for the engine ([`super::analysis::Analyser::analyse_game`]); only
    /// a desktop build runs one.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    start: String,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    moves: Vec<String>,
}

/// The positions a review needs and the store does not hold deep enough (plan A9-N17a), in the
/// order they are asked: main lines, signed moves, diagrams. [`run`] takes them off the front.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Batch {
    queue: VecDeque<Asked>,
    /// Positions wanted that the store already holds at the depth asked (left out).
    pub kept: usize,
    /// More positions are wanted than the store keeps for a document ([`MAX_EVALS`]): the rest
    /// were not queued, and the caller says so before starting.
    pub capped: bool,
    /// The depth each position is asked to.
    pub depth: u32,
}

impl Batch {
    /// The batch for `doc` (read, and linked to `refs`), leaving out what `evals` holds at `depth`
    /// or deeper (with a second line where one is asked). Positions with no legal move are known
    /// without asking.
    pub fn new(doc: &ChessDoc, refs: &[&ReferenceIndex], evals: &EvalFile, depth: u32) -> Batch {
        let mut order: Vec<String> = Vec::new();
        let mut wanted: HashMap<String, (Reached, u32, Part)> = HashMap::new();
        let mut capped = false;
        let mut want = |reached: Reached, lines: u32, part: Part| {
            let Some(key) = position_key(&reached.fen) else { return };
            if let Some(w) = wanted.get_mut(&key) {
                w.1 = w.1.max(lines);
                return;
            }
            if order.len() >= MAX_EVALS {
                capped = true;
                return;
            }
            order.push(key.clone());
            wanted.insert(key, (reached, lines, part));
        };
        for game in &doc.games {
            let score = &game.score;
            if let Some(r) = reached(score, None) {
                want(r, 1, Part::MainLine);
            }
            for n in score.mainline() {
                if let Some(r) = reached(score, Some(n)) {
                    want(r, 1, Part::MainLine);
                }
            }
        }
        for site in move_sites(doc, refs) {
            want(site.before, 1, Part::Signed);
            want(site.after, 1, Part::Signed);
        }
        for site in diagram_sites(doc) {
            let lines = if site.d.puzzle.is_some() { 2 } else { 1 };
            want(Reached::alone(site.fen), lines, Part::Diagram);
        }
        let mut batch = Batch { capped, depth, ..Batch::default() };
        for key in order {
            let Some((reached, lines, part)) = wanted.remove(&key) else { continue };
            if exact(&reached.fen).is_some() || Played::from_fen(&reached.fen).is_err() {
                continue;
            }
            if evals.get(&reached.fen).is_some_and(|e| e.depth >= depth && (lines < 2 || e.second.is_some())) {
                batch.kept = batch.kept.saturating_add(1);
                continue;
            }
            batch.queue.push_back(Asked { fen: reached.fen, key, lines, part, start: reached.start, moves: reached.moves });
        }
        batch
    }

    /// The positions still to ask, in order.
    pub fn positions(&self) -> impl Iterator<Item = &Asked> {
        self.queue.iter()
    }

    /// How many positions are still to ask.
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

impl Asked {
    /// The game that reached it, for the engine: its start and the moves after it; the position
    /// alone when they do not play.
    #[cfg(not(target_arch = "wasm32"))]
    fn game(&self) -> Option<Played> {
        let mut game = Played::from_fen(&self.start).ok()?;
        for m in &self.moves {
            if game.play_san(m).is_err() {
                return Played::from_fen(&self.fen).ok();
            }
        }
        let same = |a: &str, b: &str| position_key(a).is_some() && position_key(a) == position_key(b);
        if same(&game.fen(), &self.fen) { Some(game) } else { Played::from_fen(&self.fen).ok() }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use running::{Budget, Progress, Ran, run};

/// Running a batch: desktop only (the engine is a process).
#[cfg(not(target_arch = "wasm32"))]
mod running {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    use super::super::analysis::{Analyser, AnalysisError, AnalysisOptions, Line, MAX_DEPTH};
    use super::super::evals::{Eval, SecondLine};
    use super::Batch;

    /// How often the engine's progress is looked at.
    const POLL: Duration = Duration::from_millis(5);

    /// How long an engine told to stop has to say its last line.
    const STOP_GRACE: Duration = Duration::from_secs(2);

    /// What a batch may spend.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Budget {
        /// The depth asked for each position, 1..=[`MAX_DEPTH`].
        pub depth: u32,
        /// The longest a position is searched: past it the engine is stopped and what it found
        /// is kept (shallower: a later batch asks it again).
        pub per_position: Duration,
        /// How many positions' evaluations are handed back at a time (at least 1).
        pub chunk: usize,
    }

    /// Evaluations handed back ([`run`]): each a full FEN and its evaluation, for the store
    /// (`DocStore<Evals>::keep`).
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Progress {
        /// Positions asked so far in this run, and how many the batch held when it started.
        pub done: usize,
        pub total: usize,
        pub evals: Vec<(String, Eval)>,
    }

    /// How a run ended.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum Ran {
        /// Every position was asked.
        Done,
        /// `stop` was set: the positions not asked stay in the batch.
        Stopped,
        /// The engine stopped working: the position it was on stays in the batch.
        Failed(AnalysisError),
    }

    /// The evaluation in `lines` (the engine's, best first) found by `name`, at `at`.
    fn eval_of(lines: &[Line], name: Option<&str>, at: u64) -> Option<Eval> {
        let best = lines.iter().find(|l| l.multipv == 1)?;
        let eval = Eval::new(best.depth, best.score, best.bound, &best.uci, name, at)?;
        let second = lines.iter().find(|l| l.multipv == 2).and_then(|l| SecondLine::new(l.score, &l.uci));
        Some(eval.with_second(second))
    }

    fn now() -> u64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
    }

    /// `wait` from now, or as far as the clock goes (a budget of years never overflows it).
    fn after(wait: Duration) -> Instant {
        let now = Instant::now();
        now.checked_add(wait).or_else(|| now.checked_add(Duration::from_secs(1 << 30))).unwrap_or(now)
    }

    /// Ask `analyser` (started by the caller, its settings set) for `batch`'s positions, one at a
    /// time, to `budget.depth` and for no longer than `budget.per_position` each, handing the
    /// evaluations to `progress` every `budget.chunk` positions and at the end. Each position
    /// asked is taken off the batch; `stop` (looked at between and during positions) leaves the
    /// rest in it, so running it again asks only those. The analyser's lines and depth are set for
    /// each position (1 line, 2 for an exercise's); the caller sets its own back after.
    pub fn run(analyser: &Analyser, batch: &mut Batch, budget: &Budget, stop: &AtomicBool, mut progress: impl FnMut(Progress)) -> Ran {
        let depth = budget.depth.clamp(1, MAX_DEPTH);
        let chunk = budget.chunk.max(1);
        let total = batch.len();
        let mut done = 0usize;
        let mut found: Vec<(String, Eval)> = Vec::new();
        let mut set: Option<AnalysisOptions> = None;
        let ran = loop {
            if stop.load(Ordering::Relaxed) {
                break Ran::Stopped;
            }
            let Some(asked) = batch.queue.front().cloned() else { break Ran::Done };
            let options = AnalysisOptions { lines: asked.lines, depth };
            if set != Some(options) {
                analyser.set_options(options);
                set = Some(options);
            }
            let Some(game) = asked.game() else {
                batch.queue.pop_front();
                continue;
            };
            let fen = game.fen();
            analyser.analyse_game(&game);
            let mut deadline = after(budget.per_position);
            let mut told_to_stop = false;
            // The lines found for the position and the engine's name; `None`: none came in time.
            let lines = loop {
                if stop.load(Ordering::Relaxed) {
                    analyser.stop();
                    progress(Progress { done, total, evals: std::mem::take(&mut found) });
                    return Ran::Stopped;
                }
                let s = analyser.snapshot();
                if let Some(e) = s.error {
                    progress(Progress { done, total, evals: std::mem::take(&mut found) });
                    return Ran::Failed(e);
                }
                let this = s.fen.as_deref() == Some(fen.as_str());
                if s.ready && this && !s.searching {
                    break Some((s.lines, s.name));
                }
                if Instant::now() >= deadline {
                    if told_to_stop {
                        // It never said its last line: what it has for this position, if anything.
                        break this.then_some((s.lines, s.name));
                    }
                    analyser.stop();
                    told_to_stop = true;
                    deadline = after(STOP_GRACE);
                }
                std::thread::sleep(POLL);
            };
            // Asked, found or not: a position the engine says nothing of within the budget is not
            // asked again in this run.
            batch.queue.pop_front();
            done = done.saturating_add(1);
            if let Some(eval) = lines.and_then(|(lines, name)| eval_of(&lines, name.as_deref(), now())) {
                found.push((asked.fen.clone(), eval));
            }
            if found.len() >= chunk {
                progress(Progress { done, total, evals: std::mem::take(&mut found) });
            }
        };
        progress(Progress { done, total, evals: found });
        ran
    }
}

/// The comment the PGN export gives a move the engine disagrees with (plan A9-N17a, decision C:
/// `{Motor: …}`; the `[%eval]` command comes with F7, which owns that dialect): the label in the
/// book's language (`text_lang`), then the engine's values as a ChessBase reader reads them, value
/// and depth (`−1.50/22`), its best move instead and the engine's name.
pub fn engine_comment(d: &Divergence, text_lang: &str) -> Option<String> {
    let Computed::Engine(e) = &d.computed else { return None };
    let label = match text_lang.split(['-', '_']).next().unwrap_or_default().to_ascii_lowercase().as_str() {
        "pt" | "es" => "Motor",
        "fr" => "Moteur",
        "it" => "Motore",
        _ => "Engine",
    };
    let mut out = format!("{label}: {}/{}", value_text(e.value), e.depth);
    if let Some((san, v)) = &e.best {
        let _ = write!(out, "; {san} {}/{}", value_text(*v), e.depth);
    }
    if let Some((san, v)) = &e.second {
        let _ = write!(out, "; {san} {}/{}", value_text(*v), e.depth);
    }
    if let Some(name) = &e.engine {
        let _ = write!(out, " ({name})");
    }
    Some(out)
}

/// The evaluations kept for a document and the limits to hold its signs to: what the PGN export
/// needs to add the engine's comments ([`annotate`]).
#[derive(Clone, Debug, PartialEq)]
pub struct EngineReview {
    pub evals: EvalFile,
    pub limits: Limits,
}

/// The engine's comments ([`engine_comment`], labelled in the book's language `lang`) added to the
/// moves of `doc`'s games that `review` says the engine disagrees with, after the book's own
/// comment: what the PGN export writes. A move the reader did not play (the book's PGN's) has no
/// node, and gets none. How many.
pub fn annotate(doc: &mut ChessDoc, review: &Review, lang: &str) -> usize {
    let mut added = 0usize;
    for d in &review.divergences {
        let Place::Move(at) = d.at else { continue };
        let Some(node) = at.find(doc).and_then(|m| m.game) else { continue };
        let Some(comment) = engine_comment(d, lang) else { continue };
        let Some(n) = doc.games.get_mut(node.game).and_then(|g| g.score.nodes.get_mut(node.node)) else { continue };
        if !n.comment.contains(&comment) {
            pdfcraft_chessvision::doc::games::append_comment(&mut n.comment, &comment);
            added = added.saturating_add(1);
        }
    }
    added
}

/// A field of the CSV: as the errata's (RFC 4180, a formula-looking start begun with `'`).
fn csv_field(text: &str) -> String {
    pdfcraft_chessvision::doc::errata::csv_field(text)
}

/// The report as CSV (a header, then a row each, lines ended by CR LF): `page` (from 1),
/// `page_label` (`labels` by page index, as the book numbers it), `at` (`move` or `diagram`),
/// `printed`, `finding` ([`Found::name`]), `book` and `computed` (in words), `eval` (for White),
/// `depth`, `best`, `engine` and `source` ([`Source::name`]).
pub fn to_csv(review: &Review, labels: &[String]) -> String {
    let mut out = String::from("page,page_label,at,printed,finding,book,computed,eval,depth,best,engine,source\r\n");
    for d in &review.divergences {
        let fields = row(d, labels);
        let row: Vec<String> = fields.iter().map(|f| csv_field(f)).collect();
        out.push_str(&row.join(","));
        out.push_str("\r\n");
    }
    out
}

/// The report as a Markdown document titled `title`: a table of [`to_csv`]'s columns.
pub fn to_markdown(review: &Review, labels: &[String], title: &str) -> String {
    let esc = pdfcraft_chessvision::doc::markdown::esc;
    let mut out = format!("# The book and the engine: {}\n\n", esc(title));
    if review.divergences.is_empty() {
        out.push_str("No divergences found.\n");
        return out;
    }
    out.push_str("| Page | Printed page | At | Printed | Finding | The book says | The computation says | Eval | Depth | Best | Engine | Source |\n");
    out.push_str("|---:|---|---|---|---|---|---|---|---:|---|---|---|\n");
    for d in &review.divergences {
        let cells: Vec<String> = row(d, labels).iter().map(|c| esc(c)).collect();
        let _ = writeln!(out, "| {} |", cells.join(" | "));
    }
    out
}

/// A divergence's cells, as [`to_csv`] lists them.
fn row(d: &Divergence, labels: &[String]) -> [String; 12] {
    let (eval, depth, best, engine) = match &d.computed {
        Computed::Engine(e) => (
            value_text(e.value),
            e.depth.to_string(),
            e.best.as_ref().map(|(s, _)| s.clone()).unwrap_or_default(),
            e.engine.clone().unwrap_or_default(),
        ),
        Computed::Solver(_) => (String::new(), String::new(), String::new(), String::new()),
    };
    [
        u64::from(d.page).saturating_add(1).to_string(),
        pdfcraft_chessvision::doc::PageLabel::of(labels, d.page).to_string(),
        (if matches!(d.at, Place::Move(_)) { "move" } else { "diagram" }).to_owned(),
        d.printed.clone(),
        d.found.name().to_owned(),
        d.book_words(),
        d.computed_words(),
        eval,
        depth,
        best,
        engine,
        d.source.name().to_owned(),
    ]
}
