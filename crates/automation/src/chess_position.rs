//! `chess_position_find`: "this position in the book" without the interface (plan A9-N3, finding
//! A8-07; §2.9). The engine half; the app's is the Board tab's "In the book (n)" (`ui-egui`,
//! `chess_in_book`).
//!
//! A position (a FEN, or moves from the start or from the FEN) is looked for in what is already
//! read of the book, and nowhere else:
//! - the book's PGN, `reference_pgn` or the one kept for the document (`reference_store`), through
//!   its compact index, indexed once a session (plan A9-I3b): every move that leads to the
//!   position, its game, whether it is on the main line, and whether it was reached by another
//!   sequence of moves (a transposition);
//! - the diagrams read in this session (G11) and those kept on disk (N5, `cache`: the app's
//!   folder by default, as `chess_cache_status` reads it): page and diagram.
//!
//! It never reads a page: a book nothing of which is read says so (`read.nothing`), and is never
//! answered "not in the book". Paths are confined to `--root`; the disk is only read.

use pdfcraft_engine::chess::pdfcraft_chess::fen::Fen;
use pdfcraft_engine::chess::pdfcraft_chess::game::Game;
use pdfcraft_engine::chess::pdfcraft_chess::placement::Color;
use pdfcraft_engine::chess::pdfcraft_chessvision::doc::reference::Occurrences;
use serde_json::{Value, json};

use crate::{Args, Automation, Result, ToolError};

/// Occurrences listed when `max` is not given, of each kind.
const DEFAULT_MAX: usize = 100;
/// The most `max` takes.
const MAX_MAX: usize = 1000;
/// The most moves `moves` takes.
const MAX_MOVES: usize = 2000;

impl Automation {
    pub(crate) fn chess_position_find(&mut self, a: &Args) -> Result<Value> {
        // The paths first: one that leaves the root is refused before anything is read.
        if let Some(p) = a.opt_str("reference_pgn")? {
            self.resolve(p, false)?;
        }
        let disk = self.reading_store(a, false)?;
        let id = self.doc(a)?.id;
        let max = match a.opt_int("max")? {
            None => DEFAULT_MAX,
            Some(n) => {
                usize::try_from(n).ok().filter(|n| (1..=MAX_MAX).contains(n)).ok_or_else(|| Args::wrong("max", &format!("from 1 to {MAX_MAX}")))?
            }
        };
        let fen = position_arg(a)?;
        let any_side = a.opt_bool("any_side")?.unwrap_or(false);
        let key = fen.book_key();
        // The book's PGN.
        let (book, kept) = self.book_pgn(a, id)?;
        let pgn = book.as_ref().map(|b| {
            let found = b.index.occurrences(key, any_side, max);
            pgn_reply(&b.shown, &b.index, &found)
        });
        // The diagrams read.
        let settings = self.scan_settings(a)?;
        let store = disk.as_ref().map(|(s, _)| s);
        let diagrams = self.session.chess_position_diagrams(id, key, any_side, store.map(|s| (s, &settings)), max).map_err(crate::failed)?;
        let pages = self.doc(a)?.info.pages.len();
        let nothing = book.is_none() && diagrams.pages_read == 0;
        let total = pgn.as_ref().and_then(|p| p["total"].as_u64()).unwrap_or(0).saturating_add(diagrams.total as u64);
        Ok(json!({
            "fen": fen.to_string(),
            "side": if any_side { "any" } else if key.side == Color::Black { "black" } else { "white" },
            "occurrences": total,
            "pgn": pgn,
            "reference_kept": kept,
            "diagrams": {
                "found": diagrams.found.iter().map(|d| json!({
                    "page": d.page.saturating_add(1),
                    "index": d.index.saturating_add(1),
                    "side_assumed": d.side_assumed,
                    "from_disk": d.from_disk,
                })).collect::<Vec<_>>(),
                "total": diagrams.total,
            },
            "read": {
                "pgn": book.is_some(),
                "pages": pages,
                "pages_read": diagrams.pages_read,
                "from_disk": diagrams.from_disk,
                "cache": disk.as_ref().map(|(_, shown)| shown.clone()),
                "cache_outside_root": disk.is_none(),
                "nothing": nothing,
            },
            "why": nothing.then_some("nothing of the book is read yet: no PGN is kept for it and no page was read in this session or kept on disk. Read it (chess_read, chess_document) or keep its PGN (chess_reference_set) to look for a position in it"),
        }))
    }
}

/// The position `a` asks for: `fen` (the start position without it, as `chess_game` takes it),
/// then `moves` (SAN or UCI) played from there; a move that cannot be played is refused, and says
/// which.
pub(crate) fn position_arg(a: &Args) -> Result<Fen> {
    let mut game = match a.opt_str("fen")? {
        Some(fen) => Game::from_fen(fen).map_err(|e| ToolError::InvalidArgs(format!("fen: {e}")))?,
        None => Game::standard(),
    };
    let moves = if a.get("moves").is_some_and(|v| !v.is_null()) { a.strs("moves")? } else { Vec::new() };
    if moves.len() > MAX_MOVES {
        return Err(Args::wrong("moves", &format!("at most {MAX_MOVES} moves")));
    }
    game.go_to(usize::MAX);
    for (k, mv) in moves.iter().enumerate() {
        // UCI first: `e2e4` is not SAN, and no SAN reads as UCI.
        if game.play_uci(mv).is_err() && game.play_san(mv).is_err() {
            return Err(ToolError::InvalidArgs(format!("moves: move {} ({mv}) cannot be played there", k + 1)));
        }
    }
    Fen::parse(&game.fen()).map_err(|e| ToolError::Failed(format!("the position: {e}")))
}

/// The PGN's occurrences for a reply.
fn pgn_reply(shown: &str, index: &pdfcraft_engine::chess::pdfcraft_chessvision::doc::reference::ReferenceIndex, found: &Occurrences) -> Value {
    let occurrences: Vec<Value> = found
        .found
        .iter()
        .map(|o| {
            let san = index.san(o.at);
            let mv = san.map(|s| if o.black { format!("{}...{s}", o.number) } else { format!("{}.{s}", o.number) });
            json!({
                "game": o.at.game.saturating_add(1),
                "name": index.games().get(o.at.game).map(|g| g.name.clone()),
                "node": o.at.node,
                "move": mv,
                "san": san,
                "number": san.map(|_| o.number),
                "black": san.map(|_| o.black),
                "main_line": o.main_line,
                "transposition": o.transposed,
            })
        })
        .collect();
    json!({
        "pgn": shown,
        "found": occurrences,
        "total": found.total,
        "lines": found.lines,
        "transpositions": found.lines.saturating_sub(1),
        "cut": index.cut().map(crate::chess::cut_name),
    })
}
