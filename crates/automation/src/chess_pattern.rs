//! `chess_pattern_find`: positions of the book by material and pawn structure, without the
//! interface (plan A9-N16a, finding A8-09). The engine half: the filter in the Board's In the book
//! list and the Chess menu's item are lane B's (N16b); across books it is the library's (N15b).
//!
//! It looks where `chess_position_find` looks, and only there: the book's PGN (`reference_pgn`, or
//! the one kept for the document), each of its games played again once, every line, a stretch of
//! matching positions in a row being one find; and the diagrams already read, in this session or
//! kept on disk (`cache`). It never reads a page, and says what it looked at (`read`): nothing
//! read is not "not in the book".

use pdfcraft_engine::chess::pdfcraft_chess::pattern::{Material, PawnQuery, Query};
use pdfcraft_engine::chess::pdfcraft_chess::placement::Color;
use pdfcraft_engine::chess::pdfcraft_chessvision::doc::PageLabel;
use pdfcraft_engine::chess::pdfcraft_chessvision::doc::reference::{ReferenceIndex, Stretches};
use serde_json::{Value, json};

use crate::{Args, Automation, Result, ToolError};

/// Finds listed when `max` is not given, of each kind.
const DEFAULT_MAX: usize = 100;
/// The most `max` takes.
const MAX_MAX: usize = 1000;

/// The query `a` asks for: `material`, `either_side` and `pawns`; at least one of them.
fn query_arg(a: &Args) -> Result<Query> {
    let material = match a.opt_str("material")? {
        None => None,
        Some(text) => Some(Material::parse(text).map_err(|e| ToolError::InvalidArgs(format!("material: {e}")))?),
    };
    let mut pawns = PawnQuery::default();
    if let Some(p) = a.get("pawns") {
        let o = p.as_object().ok_or_else(|| Args::wrong("pawns", "an object"))?;
        let flag = |name: &str| -> Result<bool> {
            match o.get(name) {
                None | Some(Value::Null) => Ok(false),
                Some(Value::Bool(b)) => Ok(*b),
                Some(_) => Err(Args::wrong("pawns", "true or false for doubled, isolated and passed")),
            }
        };
        pawns.doubled = flag("doubled")?;
        pawns.isolated = flag("isolated")?;
        pawns.passed = flag("passed")?;
        pawns.side = match o.get("side").and_then(Value::as_str) {
            None | Some("either") => None,
            Some("white") => Some(Color::White),
            Some("black") => Some(Color::Black),
            Some(_) => return Err(Args::wrong("pawns", "side white, black or either")),
        };
    }
    let query = Query { material, either_side: a.opt_bool("either_side")?.unwrap_or(false), pawns };
    if query.is_empty() {
        return Err(ToolError::InvalidArgs("give material, pawns or both: what to look for".into()));
    }
    Ok(query)
}

fn query_reply(q: &Query) -> Value {
    json!({
        "material": q.material.map(|m| m.to_string()),
        "either_side": q.either_side,
        "pawns": {
            "doubled": q.pawns.doubled,
            "isolated": q.pawns.isolated,
            "passed": q.pawns.passed,
            "side": match q.pawns.side { None => "either", Some(Color::White) => "white", Some(Color::Black) => "black" },
        },
    })
}

/// The PGN's stretches for a reply.
fn pgn_reply(shown: &str, index: &ReferenceIndex, found: &Stretches) -> Value {
    let name = |k: usize| index.games().get(k).map(|g| g.name.clone());
    json!({
        "pgn": shown,
        "found": found.found.iter().map(|s| json!({
            "game": s.game.saturating_add(1),
            "name": name(s.game),
            "first": s.first.to_string(),
            "last": s.last.to_string(),
            "main_line": s.main_line,
            "positions": s.positions,
        })).collect::<Vec<_>>(),
        "total": found.total,
        "games": found.per_game.iter().map(|(k, n)| json!({ "game": k.saturating_add(1), "name": name(*k), "stretches": n })).collect::<Vec<_>>(),
        "played": found.played,
        "unreadable": found.unreadable,
        "capped": found.capped,
        "cut": index.cut().map(crate::chess::cut_name),
    })
}

impl Automation {
    pub(crate) fn chess_pattern_find(&mut self, a: &Args) -> Result<Value> {
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
        let query = query_arg(a)?;
        let (book, kept) = self.book_pgn(a, id)?;
        let settings = self.scan_settings(a)?;
        let store = disk.as_ref().map(|(s, _)| s);
        let found = self
            .session
            .chess_pattern_find(id, &query, book.as_ref().map(|b| &*b.index), store.map(|s| (s, &settings)), max)
            .map_err(crate::failed)?;
        let pgn = book.as_ref().zip(found.pgn.as_ref()).map(|(b, s)| pgn_reply(&b.shown, &b.index, s));
        let labels: Vec<String> = self.doc(a)?.info.pages.iter().map(|p| p.label.clone()).collect();
        let pages = labels.len();
        let diagrams = &found.diagrams;
        let nothing = book.is_none() && diagrams.pages_read == 0;
        let total = found.pgn.as_ref().map_or(0, |s| s.total).saturating_add(diagrams.total);
        Ok(json!({
            "query": query_reply(&query),
            "found": total,
            "pgn": pgn,
            "reference_kept": kept,
            "diagrams": {
                "found": diagrams.found.iter().map(|d| json!({
                    "page": d.page.saturating_add(1),
                    "page_label": PageLabel::of(&labels, d.page).to_string(),
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
            "why": nothing.then_some("nothing of the book is read yet: no PGN is kept for it and no page was read in this session or kept on disk. Read it (chess_read, chess_document) or keep its PGN (chess_reference_set) to look for positions in it"),
        }))
    }
}
