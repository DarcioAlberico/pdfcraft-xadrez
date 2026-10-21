//! Capture: where a selection of move text goes in a game (PGN toolbar spec §4.3.3, step T3).
//!
//! The user selects move text on a page (or pastes it) and the game being written gets it: as the
//! line, a variation, a variation inside one, or a comment. [`plan`] decides where, on copies, and
//! never changes the game it is given; the caller shows the plan or applies it
//! ([`CapturePlan::apply`]), one undo step either way.
//!
//! **Reading.** The selection is read by the book walker in fragment mode
//! (`pdfcraft_chessvision::doc::fragment`), from the position of each place it could go: a
//! numbered move goes back to its line, prose becomes comments, brackets variations. Glyphs keep
//! their weight, so a bold chain is the main line; pasted text and text read by chess OCR in an
//! area of a scan ([`CaptureText::Read`]) have none.
//!
//! **Anchoring.** Half-moves are absolute ([`Game::abs_ply`]: `2·(n−1)`, plus one for Black,
//! from the start FEN's move number; the start has one too). The selection's first move decides:
//!
//! 1. Its printed number gives the half-move it is played at. Its weight, in
//!    [`CaptureMode::Auto`], whether it is the main line.
//! 2. The candidate lines are `L0`, the line through the current move, `L1`, the line `L0`
//!    branched from, and so on to `Lk`, the main line. [`CaptureMode::Line`] and
//!    [`CaptureMode::SubVariation`] try them all, innermost first; [`CaptureMode::Variation`]
//!    only `Lk`; [`CaptureMode::Auto`] `Lk` as the line when the first move is bold, else all of
//!    them as a sub-variation.
//! 3. On each line the place is the move after which the absolute half-move is the printed one
//!    (the start included). A move without a number goes after the current move as the line;
//!    otherwise as an alternative to the move after the current one, then as an alternative to
//!    the current one.
//! 4. As the line ([`GraftMode::Line`]) a new move after a move that has replies becomes the
//!    first reply and the old first reply a variation; as a variation ([`GraftMode::Follow`]) it
//!    goes after the replies there. A reply already there is followed either way.
//! 5. Each place is scored by reading the selection from its position and grafting it on a copy:
//!    the moves played. Fewer moves that did not play break a tie, then the inner line. Two
//!    places that play as many moves (with as many stops) are ambiguous.
//! 6. A game with no moves whose start says move 1 (the number under the diagram was not read)
//!    gets one more candidate when the first move's number is another one for the same side: the
//!    same start renumbered to it. It is never applied without the preview.
//!
//! **When to ask (decision D6 b).** A plan is applied without the preview only when the reading
//! is complete, the place is unique and on `L0`, nothing outer is extended, no move is demoted
//! and the start is not renumbered. Every other case says why in [`CapturePlan::reasons`].
//!
//! **Comment.** [`CaptureMode::Comment`] reads no moves: the selection becomes the current
//! move's comment (or comment-before), moves in English SAN (decision D7).
//!
//! **Limits.** At most [`MAX_CAPTURE_TEXT`] bytes of text ([`PreviewReason::TooLong`] past it) and
//! one candidate per line (variations nest at most `MAX_DEPTH` deep), plus two. Nothing here
//! panics on any text or game.

use std::fmt;
use std::ops::Range;
use std::sync::Arc;

use pdfcraft_chess::fen;
use pdfcraft_chess::fragment::{FragmentItem, ply_of};
use pdfcraft_chess::game::{Game, GraftMode, GraftReport, GraftStop, GraftStopReason, MAX_COMMENT_LEN, MAX_DEPTH, NodeId, NodePath};
use pdfcraft_chess::nag;
use pdfcraft_chess::placement::Color;
use pdfcraft_chess::repair::{self, CipherMap, GameContext, MAX_NEAR_MISSES, Outcome, ReadToken};
use pdfcraft_chess::san::{Lang, San, figurine_piece};
use pdfcraft_chessvision::chess_text::{ChessPageText, ChessRun};
use pdfcraft_chessvision::coords::PageGeometry;
use pdfcraft_chessvision::doc::ReviewFlag;
use pdfcraft_chessvision::doc::fragment::{self, FragmentOptions, FragmentRead};
use pdfcraft_render::text::{PageText, TextGlyph};

/// Most text one capture reads, in bytes (PGN toolbar spec §2).
pub const MAX_CAPTURE_TEXT: usize = 1 << 16;

/// How a capture goes into the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CaptureMode {
    /// Decided by the text: a bold first move is the main line's continuation, anything else a
    /// variation on the line nearest the current move that takes it.
    Auto,
    /// As the line: it continues the line (or follows it), and a different move already there
    /// becomes a variation.
    Line,
    /// As a variation of the main line.
    Variation,
    /// As a variation of the line the current move is on, or of an outer one.
    SubVariation,
    /// As text on the current move: its comment, or with `before` its comment-before.
    Comment { before: bool },
}

/// The text captured.
#[derive(Clone, Copy, Debug)]
pub enum CaptureText<'a> {
    /// Glyphs from a page's text layer, in content order: their fonts give figurines and weight.
    /// With the page's `geometry`, each move keeps where it was printed on page `page`.
    Glyphs { glyphs: &'a [TextGlyph], geometry: Option<PageGeometry>, page: u32 },
    /// What chess OCR read in an area of a scanned page (`text_ocr::AreaRead::text`), its lines
    /// and word gaps as the reader cut them: read as [`Self::Glyphs`] are, but without weight, as
    /// a scan has none (the moves are placed by their numbers only). With the page's `geometry`,
    /// each move keeps where it was printed.
    Read { text: &'a PageText, geometry: Option<PageGeometry>, page: u32 },
    /// Pasted text: no weight, no place on a page.
    Plain(&'a str),
    /// Text with the character ranges (`start..end`, in characters) set in bold: pasted rich text,
    /// or a test's page. No place on a page.
    Styled { text: &'a str, bold: &'a [Range<usize>] },
}

impl CaptureText<'_> {
    /// The text's size in bytes.
    fn len(&self) -> usize {
        match self {
            CaptureText::Glyphs { glyphs, .. } => glyphs.iter().fold(0usize, |n, g| n.saturating_add(g.text.len())),
            CaptureText::Read { text, .. } => text.glyphs.iter().fold(0usize, |n, g| n.saturating_add(g.text.len())),
            CaptureText::Plain(text) | CaptureText::Styled { text, .. } => text.len(),
        }
    }
}

/// Why a plan, or one of its candidates, is shown before it is applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PreviewReason {
    /// Another place plays as many moves.
    Ambiguous,
    /// Part of the text was not read: a move that did not play, a bracket that did not balance,
    /// text after a result, a glyph or comment with no place, or a comment too long to add.
    Incomplete,
    /// The place is on a line outside the current move's (`L1` or beyond).
    OuterLine,
    /// Taken as a variation, it found no move to be an alternative to and extended a line's end
    /// (not the start of a game with no moves, which has no line yet).
    TipExtension,
    /// The start position is renumbered to the text's first move.
    Renumber,
    /// A closing bracket the text lacks was assumed.
    ImpliedClose,
    /// A move already there became a variation.
    Demotes,
    /// No place plays a move (or, as a comment, there is no text).
    NoFit,
    /// The text is longer than [`MAX_CAPTURE_TEXT`]; nothing was read.
    TooLong,
}

impl PreviewReason {
    /// A name for reports and automation.
    pub fn name(self) -> &'static str {
        match self {
            PreviewReason::Ambiguous => "ambiguous",
            PreviewReason::Incomplete => "incomplete",
            PreviewReason::OuterLine => "outer_line",
            PreviewReason::TipExtension => "tip_extension",
            PreviewReason::Renumber => "renumber",
            PreviewReason::ImpliedClose => "implied_close",
            PreviewReason::Demotes => "demotes",
            PreviewReason::NoFit => "no_fit",
            PreviewReason::TooLong => "too_long",
        }
    }
}

/// One place the text could go, with what grafting it there made.
#[derive(Clone, Debug)]
pub struct Candidate {
    /// The move the text goes after, in the game planned on (the start for a renumbered one).
    pub anchor: NodePath,
    /// The candidate line it is on: 0 for `L0` (the current move's), 1 for `L1`, …
    pub line: usize,
    /// How it was grafted.
    pub mode_used: GraftMode,
    /// The move number the start was renumbered to, for a renumbered start.
    pub renumbered: Option<u32>,
    /// The game with the text grafted: applying the candidate is taking this game.
    pub game: Game,
    /// What the graft did.
    pub report: GraftReport,
    /// How the text read from the place's position.
    pub read: FragmentRead,
    /// Moves played (the score).
    pub played: usize,
    /// Moves that did not play: the reading's stops, and the graft's.
    pub stops: usize,
    /// Why this candidate would be previewed.
    pub reasons: Vec<PreviewReason>,
}

/// Where a capture goes, worked out on copies.
#[derive(Clone, Debug)]
pub struct CapturePlan {
    /// The game's [`Game::revision`] when planned: applying checks it is still the same.
    pub base_revision: u64,
    /// Every place tried, innermost line first.
    pub candidates: Vec<Candidate>,
    /// The best candidate, when one plays a move (or, as a comment, has text).
    pub chosen: Option<usize>,
    /// Show the preview rather than apply (`reasons` is not empty).
    pub needs_preview: bool,
    /// Why: the chosen candidate's reasons, and the plan's own.
    pub reasons: Vec<PreviewReason>,
}

/// Why a plan was not applied. The game is unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureError {
    /// The game changed since the plan was made: plan again.
    Stale,
    /// The plan has no such candidate (or none was chosen).
    NoCandidate,
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CaptureError::Stale => write!(f, "the game changed since the capture was planned"),
            CaptureError::NoCandidate => write!(f, "the capture has no such place"),
        }
    }
}

impl std::error::Error for CaptureError {}

impl CapturePlan {
    /// The chosen candidate.
    pub fn chosen(&self) -> Option<&Candidate> {
        self.chosen.and_then(|i| self.candidates.get(i))
    }

    /// Replace `game` with candidate `index`'s game (the chosen one when `None`), after checking
    /// that `game` is still the one planned on (spec §4.3.3 step 10). The move shown becomes the
    /// last one grafted.
    pub fn apply(&self, game: &mut Game, index: Option<usize>) -> Result<(), CaptureError> {
        if game.revision() != self.base_revision {
            return Err(CaptureError::Stale);
        }
        let candidate = index.or(self.chosen).and_then(|i| self.candidates.get(i)).ok_or(CaptureError::NoCandidate)?;
        *game = candidate.game.clone();
        Ok(())
    }

    fn empty(game: &Game, reason: PreviewReason) -> CapturePlan {
        CapturePlan { base_revision: game.revision(), candidates: Vec::new(), chosen: None, needs_preview: true, reasons: vec![reason] }
    }
}

/// Plan capturing `text`, read in `lang` (the document's language), after move `current` of
/// `game` in `mode` (see the [module](self)). `game` is not changed.
pub fn plan(game: &Game, current: NodeId, mode: CaptureMode, text: CaptureText<'_>, lang: Lang) -> CapturePlan {
    if text.len() > MAX_CAPTURE_TEXT {
        return CapturePlan::empty(game, PreviewReason::TooLong);
    }
    if game.path_of(current).is_none() {
        return CapturePlan::empty(game, PreviewReason::NoFit);
    }
    let (chess, opts, page) = chess_text(text, lang);
    let mut out = CapturePlan { base_revision: game.revision(), candidates: Vec::new(), chosen: None, needs_preview: true, reasons: Vec::new() };
    if let CaptureMode::Comment { before } = mode {
        let candidate = comment_candidate(game, current, &chess, before);
        out.chosen = (!candidate.reasons.contains(&PreviewReason::NoFit)).then_some(0);
        out.candidates.push(candidate);
        return finish(out);
    }
    let start_ply = game.abs_ply(0).unwrap_or(0);
    let Some(first) = first_move(&chess, start_ply) else {
        return finish(out);
    };
    // As the line or as a variation, and on which lines.
    let (graft_mode, main_only, variation) = match mode {
        CaptureMode::Line => (GraftMode::Line, false, false),
        CaptureMode::Variation => (GraftMode::Follow, true, true),
        CaptureMode::SubVariation => (GraftMode::Follow, false, true),
        CaptureMode::Auto if first.bold => (GraftMode::Line, true, false),
        CaptureMode::Auto | CaptureMode::Comment { .. } => (GraftMode::Follow, false, true),
    };
    let ctx = Context { chess: &chess, opts, page, graft_mode, variation };
    let mut points: Vec<(NodeId, usize)> = Vec::new();
    match first.ply {
        Some(ply) => {
            let lines = candidate_lines(game, current);
            let last = lines.len().saturating_sub(1);
            for (index, line) in lines.iter().enumerate() {
                if main_only && index != last {
                    continue;
                }
                if let Some(&at) = line.iter().find(|&&n| game.abs_ply(n) == Some(ply))
                    && !points.iter().any(|&(p, _)| p == at)
                {
                    points.push((at, index));
                }
            }
        }
        None => {
            points.push((current, 0));
            if variation && let Some(parent) = parent_of(game, current) {
                points.push((parent, 0));
            }
        }
    }
    for (at, line) in points {
        if let Some(candidate) = ctx.try_at(game, at, line, None) {
            out.candidates.push(candidate);
        }
    }
    if let Some(candidate) = renumbered(game, first, &ctx) {
        out.candidates.push(candidate);
    }
    out.chosen = best(&out.candidates);
    if let (Some(chosen), Some(c)) = (out.chosen, out.chosen.and_then(|i| out.candidates.get(i)))
        && out.candidates.iter().enumerate().any(|(i, o)| i != chosen && o.played == c.played && o.stops == c.stops)
    {
        out.reasons.push(PreviewReason::Ambiguous);
    }
    finish(out)
}

/// The plan's reasons (the chosen candidate's, its own, or that nothing fits), sorted, and
/// whether to preview.
fn finish(mut plan: CapturePlan) -> CapturePlan {
    match plan.chosen() {
        Some(c) => {
            let own = c.reasons.clone();
            plan.reasons.extend(own);
        }
        None => plan.reasons.push(PreviewReason::NoFit),
    }
    plan.reasons.sort_unstable();
    plan.reasons.dedup();
    plan.needs_preview = !plan.reasons.is_empty();
    plan
}

/// The best candidate: the most moves played, then the fewest stops, then the innermost line,
/// then the first tried. `None` when none plays a move.
fn best(candidates: &[Candidate]) -> Option<usize> {
    candidates
        .iter()
        .enumerate()
        .filter(|(_, c)| c.played > 0)
        .max_by(|(i, a), (j, b)| a.played.cmp(&b.played).then(b.stops.cmp(&a.stops)).then(b.line.cmp(&a.line)).then(j.cmp(i)))
        .map(|(i, _)| i)
}

/// What every candidate of one plan is read and grafted with.
struct Context<'a> {
    chess: &'a ChessPageText,
    opts: FragmentOptions,
    page: Option<u32>,
    graft_mode: GraftMode,
    /// Taken as a variation (so extending a line's end is worth a preview).
    variation: bool,
}

impl Context<'_> {
    /// Read the text from the position after `at` and graft it there; `renumbered`: `game` is
    /// the renumbered start. `None` when `game` has no move `at`.
    fn try_at(&self, game: &Game, at: NodeId, line: usize, renumbered: Option<u32>) -> Option<Candidate> {
        let fen = game.fen_at(at)?;
        let read = fragment::read(self.chess, &fen, &self.opts);
        let (grafted, report) = game.graft(at, self.graft_mode, &read.items, self.page);
        let stops = read.stops.len().saturating_add(usize::from(report.stop.is_some()));
        let mut reasons = Vec::new();
        let incomplete =
            !read.start_ok || stops > 0 || !report.unbalanced.is_empty() || !report.leftover_text.trim().is_empty() || !read.rest.trim().is_empty();
        if incomplete {
            reasons.push(PreviewReason::Incomplete);
        }
        if line > 0 {
            reasons.push(PreviewReason::OuterLine);
        }
        // A game with no moves has no line for a variation to be of: its first moves are the line.
        if self.variation && report.extended_tip && !game.is_empty() {
            reasons.push(PreviewReason::TipExtension);
        }
        if renumbered.is_some() {
            reasons.push(PreviewReason::Renumber);
        }
        if read.flags.iter().any(|f| matches!(f, ReviewFlag::ImpliedClose { .. })) {
            reasons.push(PreviewReason::ImpliedClose);
        }
        if report.demoted {
            reasons.push(PreviewReason::Demotes);
        }
        Some(Candidate {
            anchor: game.path_of(at)?,
            line,
            mode_used: self.graft_mode,
            renumbered,
            played: report.played,
            stops,
            game: grafted,
            report,
            read,
            reasons,
        })
    }
}

/// The selection's first move, as the walker will take it: its printed half-move and weight.
#[derive(Clone, Copy, Debug, PartialEq)]
struct FirstMove {
    ply: Option<u32>,
    bold: bool,
}

/// The first move of `text` that the walker plays rather than leaves in prose: one with a number
/// (not before `start_ply`, the game's start, which the walker reads as prose), one before any
/// prose word, or a bold one. `None` when there is none.
fn first_move(text: &ChessPageText, start_ply: u32) -> Option<FirstMove> {
    let mut number: Option<(u16, bool)> = None;
    let mut prose = false;
    for span in text.lines.iter().flat_map(|l| &l.spans) {
        match &span.run {
            ChessRun::MoveNumber { number: n, black } => number = Some((*n, *black)),
            ChessRun::Move { .. } => {
                match number.take() {
                    Some((n, black)) => {
                        let ply = ply_of(u32::from(n), black);
                        if ply >= start_ply {
                            return Some(FirstMove { ply: Some(ply), bold: span.bold });
                        }
                        // Numbered before the game starts: quoted in prose.
                        prose = true;
                    }
                    None if !prose || span.bold => return Some(FirstMove { ply: None, bold: span.bold }),
                    // A move after prose words, without a number: the walker leaves it there.
                    None => {}
                }
            }
            ChessRun::Text(t) => {
                if t.chars().any(char::is_alphanumeric) {
                    prose = true;
                    number = None;
                }
            }
            ChessRun::Nag(_) | ChessRun::Figurine(_) => {}
        }
    }
    None
}

/// The candidate lines from move `current`: `L0`, the line through it, then each line the one
/// before branched from, to the main line. At most `MAX_DEPTH + 1`.
fn candidate_lines(game: &Game, current: NodeId) -> Vec<Vec<NodeId>> {
    let mut out = Vec::new();
    let Some(path) = game.path_of(current) else { return out };
    let mut indices = path.indices().to_vec();
    let mut at = current;
    loop {
        out.push(game.line_of(at));
        if out.len() > MAX_DEPTH {
            break;
        }
        // The nearest move on the way that is not a first reply: the line branched at its parent.
        let Some(head) = indices.iter().rposition(|&i| i != 0) else { break };
        indices.truncate(head);
        match game.node_at(&NodePath::new(indices.clone())) {
            Some(parent) => at = parent,
            None => break,
        }
    }
    out
}

/// The move before `node`; `None` for the start or a move not in the game.
fn parent_of(game: &Game, node: NodeId) -> Option<NodeId> {
    let path = game.path_of(node)?;
    let (_, up) = path.indices().split_last()?;
    game.node_at(&NodePath::new(up.to_vec()))
}

/// The renumbered-start candidate: `game` has no moves and starts at move 1, and the first move
/// is numbered otherwise for the side to move.
fn renumbered(game: &Game, first: FirstMove, ctx: &Context<'_>) -> Option<Candidate> {
    let ply = first.ply?;
    let start = game.abs_ply(0)?;
    if !game.is_empty() || start > 1 || ply == start || ply % 2 != start % 2 {
        return None;
    }
    let number = ply / 2 + 1;
    let mut position = fen::parse(&game.start_fen()).ok()?;
    position.fullmove_number = number;
    let side = if start % 2 == 1 { Color::Black } else { Color::White };
    let mut again = Game::new(&position, side).ok()?;
    for (name, value) in game.tags() {
        // The tags were valid in `game`.
        let _ = again.set_tag(name, value);
    }
    again.set_comment(0, game.comment(0));
    if again.abs_ply(0) != Some(ply) {
        return None;
    }
    let mut candidate = ctx.try_at(&again, 0, 0, Some(number))?;
    // The renumbered game is a new one; its revision must not look like the planned game's.
    candidate.game.bump_revision(game.revision());
    Some(candidate)
}

/// The comment candidate: the text, moves in English SAN, on `current`'s comment (or
/// comment-before), unless it already says it.
fn comment_candidate(game: &Game, current: NodeId, chess: &ChessPageText, before: bool) -> Candidate {
    let text = comment_text(chess);
    let mut g = game.clone();
    let mut report = GraftReport { anchor: game.path_of(current).unwrap_or_default(), ..GraftReport::default() };
    let mut reasons = Vec::new();
    // The start has no comment-before: its comment comes before the first move.
    let before = before && current != 0;
    if text.is_empty() {
        reasons.push(PreviewReason::NoFit);
    } else {
        let existing = if before { g.comment_before(current) } else { g.comment(current) };
        if !existing.contains(text.as_str()) {
            let joined = if existing.is_empty() { text.clone() } else { format!("{existing} {text}") };
            if joined.len() > MAX_COMMENT_LEN {
                report.stop = Some(GraftStop { index: 0, reason: GraftStopReason::TooLong, text: text.clone(), maybe_en_passant: false, fen: None });
                report.rest_from = Some(0);
                report.leftover_text = text;
                reasons.push(PreviewReason::Incomplete);
            } else if before {
                g.set_comment_before(current, &joined);
            } else {
                g.set_comment(current, &joined);
            }
        }
    }
    g.go_to_node(current);
    Candidate {
        anchor: report.anchor.clone(),
        line: 0,
        mode_used: GraftMode::Comment,
        renumbered: None,
        game: g,
        stops: usize::from(report.stop.is_some()),
        report,
        read: FragmentRead { start_ok: true, ..FragmentRead::default() },
        played: 0,
        reasons,
    }
}

/// The text as a comment: moves in English SAN glued to their numbers and glyphs, figurines as
/// English letters, the words as written, lines joined by a space.
fn comment_text(chess: &ChessPageText) -> String {
    let mut out = String::new();
    // Glue the next piece to the last one (after a move number).
    let mut glue = false;
    for span in chess.lines.iter().flat_map(|l| &l.spans) {
        let (piece, glued) = match &span.run {
            ChessRun::MoveNumber { number, black } => (if *black { format!("{number}...") } else { format!("{number}.") }, false),
            ChessRun::Move { san, .. } => (san.to_english(), glue),
            ChessRun::Nag(code) => (glyph_text(*code), true),
            ChessRun::Figurine(kind) => (kind.letter().to_string(), glue),
            ChessRun::Text(t) => (t.trim().chars().map(|c| figurine_piece(c).map_or(c, |k| k.letter())).collect::<String>(), false),
        };
        glue = matches!(span.run, ChessRun::MoveNumber { .. });
        if piece.is_empty() {
            continue;
        }
        if !(glued || out.is_empty()) {
            out.push(' ');
        }
        out.push_str(&piece);
    }
    out
}

/// A glyph as printed when that is safe alone, else `$code`.
fn glyph_text(code: u16) -> String {
    match nag::by_code(code) {
        Some(n) if n.safe_glyph => n.glyph.to_owned(),
        _ => format!("${code}"),
    }
}

// ─── Correcting a move that did not play (PGN toolbar T10) ──────────────────────────────────

/// Most moves of one candidate that [`stopped_moves`] looks at: each is a replay on one board.
pub const MAX_STOPPED: usize = 64;

/// A move of a candidate's text that did not play, with the legal moves near it.
#[derive(Clone, Debug, PartialEq)]
pub struct StoppedMove {
    /// The move as printed, with its number (`19.Nxe6+`).
    pub text: String,
    /// The move alone, as printed (`Nxe6+`): what a correction replaces.
    pub word: String,
    /// Where it was printed, in user space, when the text was on a page.
    pub rect: Option<[f32; 4]>,
    /// The position it was to be played in, as a FEN; `None` when it was never reached (a move
    /// out of sequence, or after one that did not play).
    pub fen: Option<String>,
    /// The legal moves near it in that position, at most [`MAX_NEAR_MISSES`]: the one reading
    /// the legality repair settles on, the moves an ambiguous reading fits, or the closest legal
    /// moves. Offered for a click, **never applied** here (the lesson of 2026-10-19: guessing a
    /// move goes wrong).
    pub near: Vec<San>,
    /// The graft stopped at it (else the reader did not play it).
    pub graft: bool,
}

/// The moves of candidate `c` that did not play, read in `lang`: the reader's, in reading order,
/// then the one the graft stopped at. At most [`MAX_STOPPED`].
pub fn stopped_moves(c: &Candidate, lang: Lang) -> Vec<StoppedMove> {
    let mut out: Vec<StoppedMove> = c
        .read
        .stops
        .iter()
        .take(MAX_STOPPED)
        .map(|s| {
            let word = move_word(&s.text).to_owned();
            let near = s.fen.as_deref().map(|fen| nearby_moves(fen, &word, lang)).unwrap_or_default();
            StoppedMove { text: s.text.clone(), word, rect: s.rect, fen: s.fen.clone(), near, graft: false }
        })
        .collect();
    if let Some(stop) = c.report.stop.as_ref()
        && out.len() < MAX_STOPPED
        && let Some(FragmentItem::Move { text, rect, .. }) = c.read.items.get(stop.index)
    {
        let word = move_word(text).to_owned();
        let near = stop.fen.as_deref().map(|fen| nearby_moves(fen, &word, lang)).unwrap_or_default();
        out.push(StoppedMove { text: text.clone(), word, rect: *rect, fen: stop.fen.clone(), near, graft: true });
    }
    out
}

/// The legal moves near `word` (a move as printed, read in `lang`) in the position `fen`: what
/// the legality repair (`pdfcraft_chess::repair`, with the OCR confusion table) makes of it
/// alone: the one move it settles on, the moves an ambiguous reading fits, or the closest legal
/// moves when nothing settles; the move itself when it is legal there. At most
/// [`MAX_NEAR_MISSES`], each once; none for a position that is not legal.
pub fn nearby_moves(fen: &str, word: &str, lang: Lang) -> Vec<San> {
    let Some(ctx) = GameContext::from_fen(fen) else { return Vec::new() };
    let replay = repair::repair(&ctx, &[ReadToken::plain(word.trim())], lang, &CipherMap::new());
    let found = match replay.outcomes.into_iter().next() {
        Some(Outcome::Kept(san)) => vec![san],
        Some(Outcome::Repaired(r)) => vec![r.san],
        Some(Outcome::Ambiguous { candidates }) => candidates,
        Some(Outcome::Unresolved { near }) => near,
        Some(Outcome::NotReached) | None => Vec::new(),
    };
    let mut out: Vec<San> = Vec::new();
    for san in found {
        if out.len() >= MAX_NEAR_MISSES {
            break;
        }
        if !out.iter().any(|o| o.to_english() == san.to_english()) {
            out.push(san);
        }
    }
    out
}

/// `text` without the move number it starts with (`19.`, `19...`, `19…`, `19. `): the move as
/// printed. A castling written with zeros (`0-0`) has no number.
pub fn move_word(text: &str) -> &str {
    let trimmed = text.trim();
    let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
    // Digits are ASCII, so `digits` is a char boundary.
    let Some(after) = trimmed.get(digits..) else { return trimmed };
    let marks: Vec<char> = after.chars().take_while(|c| matches!(c, '.' | '\u{2026}') || c.is_whitespace()).collect();
    if digits == 0 || !marks.iter().any(|c| matches!(c, '.' | '\u{2026}')) {
        return trimmed;
    }
    after.get(marks.iter().map(|c| c.len_utf8()).sum::<usize>()..).unwrap_or(trimmed)
}

/// The text as chess text, with how to read it and the page its moves were printed on.
fn chess_text(text: CaptureText<'_>, lang: Lang) -> (ChessPageText, FragmentOptions, Option<u32>) {
    match text {
        CaptureText::Glyphs { glyphs, geometry, page } => {
            (fragment::text_of_glyphs(glyphs, Some(lang)), FragmentOptions { geometry, weight: true }, Some(page))
        }
        CaptureText::Read { text, geometry, page } => {
            (fragment::text_of_page(text, Some(lang)), FragmentOptions { geometry, weight: false }, Some(page))
        }
        CaptureText::Plain(text) => {
            (fragment::text_of_glyphs(&glyphs_of_text(text, &[]), Some(lang)), FragmentOptions { geometry: None, weight: false }, None)
        }
        CaptureText::Styled { text, bold } => {
            (fragment::text_of_glyphs(&glyphs_of_text(text, bold), Some(lang)), FragmentOptions { geometry: None, weight: true }, None)
        }
    }
}

/// Glyph advance and line pitch of text set out as glyphs, in points.
const ADVANCE: f32 = 5.0;
const PITCH: f32 = 14.0;
const HEIGHT: f32 = 10.0;

/// `text` set out as glyphs, one line per line of text, so the page's text reader reads it: a
/// character a glyph, a run of white space one word gap (never wide enough to look like a column
/// gutter), the characters in `bold` (character ranges) in a bold font.
fn glyphs_of_text(text: &str, bold: &[Range<usize>]) -> Vec<TextGlyph> {
    let count = text.chars().count();
    // Which characters are bold: +1 where a range starts, −1 where it ends, summed.
    let mut steps = vec![0i32; count.saturating_add(1)];
    for r in bold {
        let (start, end) = (r.start.min(count), r.end.min(count));
        if start < end {
            if let Some(s) = steps.get_mut(start) {
                *s = s.saturating_add(1);
            }
            if let Some(e) = steps.get_mut(end) {
                *e = e.saturating_sub(1);
            }
        }
    }
    let regular: Arc<str> = Arc::from("Capture-Regular");
    let heavy: Arc<str> = Arc::from("Capture-Bold");
    let mut out = Vec::with_capacity(count);
    let (mut row, mut x, mut depth, mut gap) = (0u32, 0.0f32, 0i32, false);
    for (i, c) in text.chars().enumerate() {
        depth = depth.saturating_add(steps.get(i).copied().unwrap_or(0));
        if c == '\n' {
            row = row.saturating_add(1);
            (x, gap) = (0.0, false);
            continue;
        }
        if c.is_whitespace() || c.is_control() {
            gap = true;
            continue;
        }
        if gap && x > 0.0 {
            x += ADVANCE;
        }
        gap = false;
        // Rows and columns stay far below f32's exact integers for a 64 KiB text.
        let y = 100.0 + row as f32 * PITCH;
        let rect = [40.0 + x, y, 40.0 + x + ADVANCE, y + HEIGHT];
        let quad = [[rect[0], rect[3]], [rect[2], rect[3]], [rect[2], rect[1]], [rect[0], rect[1]]];
        let font = if depth > 0 { heavy.clone() } else { regular.clone() };
        let code = u32::from(c);
        out.push(TextGlyph::new(c.to_string(), rect, quad, [1.0, 0.0]).with_source(Some(font), (code <= 0xFF).then_some(code)));
        x += ADVANCE;
    }
    out
}
