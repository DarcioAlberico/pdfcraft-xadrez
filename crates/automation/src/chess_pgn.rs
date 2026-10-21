//! `chess_pgn_edit`: reshape a game's moves without the interface, inline only.
//!
//! The game comes in the arguments (a PGN or a FEN) and goes back in the reply; no file and no
//! document is read or written, so the tool takes no path. A list of operations is applied in
//! order, each at an address: `main:<n>` is the main-line move after which the absolute
//! half-move is `n` (the start's when nothing has been played: `main:0` from the standard start,
//! `main:35` from a FEN `… b - - 0 18`), and `path:<i.j.k>` the move reached by those reply
//! indices from the start (`0` the line, `1` the first variation; `path:` is the start).
//! Addresses are read when their operation runs, after the ones before it changed the tree.
//!
//! `play_null` plays a null move (`--`, a threat shown by passing), `move_variation` moves a
//! variation one place up or down among the others at its fork, and `diagram` marks a move for a
//! diagram (`$220`) or takes the mark off.
//!
//! `capture` plans where a selection of move text goes (`pdfcraft_engine::chess::capture`) and
//! applies the chosen place unless told otherwise; its entry lists every candidate place.
//!
//! Each operation adds an entry to the report. One that is refused (a bad address, an illegal
//! move, a structure change the tree does not allow) adds an error entry, leaves the game as it
//! was before it, and stops the list there.

use pdfcraft_engine::chess::capture::{self, Candidate, CaptureMode, CapturePlan, CaptureText};
use pdfcraft_engine::chess::pdfcraft_chess::fragment::FragmentItem;
use pdfcraft_engine::chess::pdfcraft_chess::game::{Game, GraftMode, GraftReport, GraftStopReason, MAX_COMMENT_LEN, NodeId, NodePath, Shift};
use pdfcraft_engine::chess::pdfcraft_chess::san::{Lang, San, Style};
use serde_json::{Map, Value, json};

use crate::{Args, Automation, Result, ToolError};

/// Most operations in one call.
pub(crate) const MAX_OPS: usize = 2000;

/// Most moves and fragment items over all the operations of one call.
const MAX_STEPS: usize = 20_000;

/// Most items in one `graft`.
const MAX_ITEMS: usize = 4096;

/// Longest address read (`path:` with every index of a deep line).
const MAX_ADDRESS_LEN: usize = 65_536;

/// Most bold ranges in one `capture`.
pub(crate) const MAX_BOLD_RUNS: usize = 4096;

/// Why one operation was refused: said in its report entry.
type Refusal = String;

impl Automation {
    pub(crate) fn chess_pgn_edit(&self, a: &Args) -> Result<Value> {
        let (fen, pgn) = (a.opt_str("fen")?, a.opt_str("pgn")?);
        // The first move of the PGN that could not be played (its line ends there), said in the reply.
        let (mut game, stopped) = match (fen, pgn) {
            (Some(_), Some(_)) => return Err(ToolError::InvalidArgs("give fen or pgn, not both".into())),
            (Some(fen), None) => (Game::from_fen(fen).map_err(|e| ToolError::InvalidArgs(format!("fen: {e}")))?, None),
            (None, Some(pgn)) => Game::from_pgn(pgn).map_err(|e| ToolError::InvalidArgs(format!("pgn: {e}")))?,
            (None, None) => (Game::standard(), None),
        };
        let ops = a.get("ops").and_then(Value::as_array).ok_or_else(|| Args::wrong("ops", "an array of operations"))?;
        if ops.len() > MAX_OPS {
            return Err(Args::wrong("ops", &format!("at most {MAX_OPS} operations")));
        }
        let steps = ops
            .iter()
            .map(|op| ["moves", "items"].iter().filter_map(|k| op.get(k).and_then(Value::as_array)).map(Vec::len).sum::<usize>())
            .sum::<usize>();
        if steps > MAX_STEPS {
            return Err(Args::wrong("ops", &format!("at most {MAX_STEPS} moves and items in all")));
        }
        let mut report = Vec::with_capacity(ops.len());
        for (index, op) in ops.iter().enumerate() {
            let name = op.get("op").and_then(Value::as_str).unwrap_or_default().to_owned();
            match apply(&mut game, op) {
                Ok(mut entry) => {
                    entry.insert("index".into(), json!(index));
                    entry.insert("op".into(), json!(name));
                    entry.insert("ok".into(), json!(true));
                    report.push(Value::Object(entry));
                }
                Err(error) => {
                    report.push(json!({ "index": index, "op": name, "ok": false, "error": error }));
                    break;
                }
            }
        }
        let applied = report.iter().filter(|e| e["ok"] == true).count();
        Ok(json!({
            "pgn": game.pgn(),
            "movetext": game.movetext(),
            "report": report,
            "applied": applied,
            "pgn_stopped": stopped,
            "current_path": address_of(&game, game.current()),
        }))
    }
}

/// Apply one operation; the game is unchanged when it is refused.
fn apply(game: &mut Game, op: &Value) -> std::result::Result<Map<String, Value>, Refusal> {
    let obj = op.as_object().ok_or("an operation is an object")?;
    let name = obj.get("op").and_then(Value::as_str).ok_or("op is missing")?;
    let at_text = obj.get("at").and_then(Value::as_str).ok_or("at is missing (main:<half-move> or path:<i.j.k>)")?;
    let at = resolve(game, at_text)?;
    let text = || obj.get("text").and_then(Value::as_str).ok_or("text is missing");
    let mut entry = Map::new();
    match name {
        "play" => {
            let moves = obj.get("moves").and_then(Value::as_array).ok_or("moves is missing")?;
            let mut played = game.clone();
            played.go_to_node(at);
            for mv in moves {
                let mv = mv.as_str().ok_or("moves are strings")?;
                // UCI first: `e2e4` is not SAN, and no SAN reads as UCI.
                if played.play_uci(mv).is_err() && played.play_san(mv).is_err() {
                    return Err(format!("{mv} cannot be played there"));
                }
            }
            *game = played;
        }
        "comment" | "comment_before" => {
            let text = text()?;
            if text.len() > MAX_COMMENT_LEN {
                return Err(format!("text is longer than {MAX_COMMENT_LEN} bytes"));
            }
            if name == "comment" {
                game.set_comment(at, text);
            } else if at == 0 {
                return Err("the start has no comment before it (use comment on it)".into());
            } else {
                game.set_comment_before(at, text);
            }
            game.go_to_node(at);
        }
        "nag" => {
            let code = obj.get("code").and_then(Value::as_u64).and_then(|c| u16::try_from(c).ok()).ok_or("code is a glyph's PGN code, 1-255")?;
            if !game.set_nag(at, code) {
                return Err(format!("${code} cannot be put on this move (the start, a code outside 1-255, or too many glyphs)"));
            }
            game.go_to_node(at);
        }
        "play_null" => {
            let mut played = game.clone();
            played.go_to_node(at);
            played.play_null().map_err(|_| "a null move cannot be played there: the side to move is in check, or the game is full".to_owned())?;
            *game = played;
        }
        "move_variation" => {
            let shift = match obj.get("to").and_then(Value::as_str) {
                Some("up") => Shift::Up,
                Some("down") => Shift::Down,
                _ => return Err("to is up or down".into()),
            };
            game.move_variation(at, shift).map_err(|e| e.to_string())?;
        }
        "diagram" => {
            let on = match obj.get("on") {
                None => true,
                Some(v) => v.as_bool().ok_or("on is true or false")?,
            };
            if on != game.has_diagram(at) && !game.toggle_diagram(at) {
                return Err("a diagram mark cannot go on this move (the start, or too many glyphs)".into());
            }
            game.go_to_node(at);
        }
        "promote_one" => game.promote_one(at).map_err(|e| e.to_string())?,
        "demote" => game.demote(at).map_err(|e| e.to_string())?,
        "delete_variation" => game.delete_variation(at).map_err(|e| e.to_string())?,
        "make_main" => make_main(game, at)?,
        "cut" => {
            game.go_to_node(at);
            game.truncate();
        }
        "graft" => {
            let mode = match obj.get("mode").and_then(Value::as_str) {
                None | Some("follow") => GraftMode::Follow,
                Some("line") => GraftMode::Line,
                Some("comment") => GraftMode::Comment,
                Some(other) => return Err(format!("mode {other:?} is follow, line or comment")),
            };
            let raw = obj.get("items").and_then(Value::as_array).ok_or("items is missing")?;
            if raw.len() > MAX_ITEMS {
                return Err(format!("at most {MAX_ITEMS} items"));
            }
            let items = raw
                .iter()
                .enumerate()
                .map(|(i, v)| fragment_item(v).map_err(|e| format!("items[{i}]: {e}")))
                .collect::<std::result::Result<Vec<_>, _>>()?;
            let (grafted, graft) = game.graft(at, mode, &items, None);
            *game = grafted;
            entry.insert("graft".into(), graft_json(&graft));
        }
        "capture" => {
            let (plan, lang) = capture_plan(game, at, obj)?;
            let apply = match obj.get("apply").and_then(Value::as_str) {
                None | Some("chosen") => plan.chosen.is_some(),
                Some("auto") => plan.chosen.is_some() && !plan.needs_preview,
                Some("never") => false,
                Some(other) => return Err(format!("apply {other:?} is chosen, auto or never")),
            };
            // Planned on this very game, so only a missing candidate can refuse it.
            let applied = apply && plan.apply(game, None).is_ok();
            if !applied {
                game.go_to_node(at);
            }
            entry.insert("capture".into(), capture_json(&plan, applied, lang));
        }
        other => return Err(format!("unknown op {other:?}")),
    }
    entry.insert("path".into(), json!(address_of(game, game.current())));
    Ok(entry)
}

/// The plan of a `capture` op: `text` read in `lang` (English by default) after `at` in `mode`
/// (auto by default), the characters in `bold_runs` (`[start, end]` pairs) set in bold; with the
/// language.
fn capture_plan(game: &Game, at: NodeId, obj: &Map<String, Value>) -> std::result::Result<(CapturePlan, Lang), Refusal> {
    let text = obj.get("text").and_then(Value::as_str).ok_or("text is missing")?;
    let lang = match obj.get("lang").filter(|l| !l.is_null()) {
        None => Lang::En,
        Some(l) => l.as_str().and_then(Lang::from_tag).ok_or("lang is a language tag such as en or pt")?,
    };
    let mode = capture_mode(obj.get("mode").and_then(Value::as_str))?;
    let bold = match obj.get("bold_runs").filter(|b| !b.is_null()) {
        None => None,
        Some(runs) => {
            let runs = runs.as_array().ok_or("bold_runs is an array of [start, end] character ranges")?;
            if runs.len() > MAX_BOLD_RUNS {
                return Err(format!("at most {MAX_BOLD_RUNS} bold_runs"));
            }
            let bound = |v: Option<&Value>| v.and_then(Value::as_u64).map(|n| usize::try_from(n).unwrap_or(usize::MAX));
            let ranges = runs
                .iter()
                .map(|r| match r.as_array().map(Vec::as_slice) {
                    Some([a, b]) => bound(Some(a)).zip(bound(Some(b))).map(|(start, end)| start..end),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()
                .ok_or("bold_runs are [start, end] pairs of character indices")?;
            Some(ranges)
        }
    };
    let text = match &bold {
        Some(bold) => CaptureText::Styled { text, bold },
        None => CaptureText::Plain(text),
    };
    Ok((capture::plan(game, at, mode, text, lang), lang))
}

/// A capture mode by its name (`auto` when none is given).
pub(crate) fn capture_mode(name: Option<&str>) -> std::result::Result<CaptureMode, Refusal> {
    Ok(match name {
        None | Some("auto") => CaptureMode::Auto,
        Some("line") => CaptureMode::Line,
        Some("variation") => CaptureMode::Variation,
        Some("subvariation") => CaptureMode::SubVariation,
        Some("comment") => CaptureMode::Comment { before: false },
        Some("comment_before") => CaptureMode::Comment { before: true },
        Some(other) => return Err(format!("mode {other:?} is auto, line, variation, subvariation, comment or comment_before")),
    })
}

/// A capture plan as the reply gives it: every candidate, the chosen one and whether it was applied,
/// and the moves of the chosen one that did not play with the legal moves near each (T10), read in
/// `lang`: offered, never applied.
pub(crate) fn capture_json(plan: &CapturePlan, applied: bool, lang: Lang) -> Value {
    let stopped = plan.chosen().map(|c| capture::stopped_moves(c, lang)).unwrap_or_default();
    json!({
        "stopped": stopped.iter().map(|s| json!({
            "text": s.text,
            "word": s.word,
            "fen": s.fen,
            "near": s.near.iter().map(San::to_english).collect::<Vec<_>>(),
            "near_as_printed": s.near.iter().map(|n| n.write(lang, Style::Letters)).collect::<Vec<_>>(),
            "graft": s.graft,
        })).collect::<Vec<_>>(),
        "applied": applied,
        "needs_preview": plan.needs_preview,
        "reasons": plan.reasons.iter().map(|r| r.name()).collect::<Vec<_>>(),
        "chosen": plan.chosen,
        "base_revision": plan.base_revision,
        "candidates": plan.candidates.iter().map(candidate_json).collect::<Vec<_>>(),
    })
}

fn candidate_json(c: &Candidate) -> Value {
    json!({
        "anchor": path_text(&c.anchor),
        "line": c.line,
        "mode": match c.mode_used {
            GraftMode::Follow => "follow",
            GraftMode::Line => "line",
            GraftMode::Comment => "comment",
        },
        "renumbered": c.renumbered,
        "start_fen": c.renumbered.map(|_| c.game.start_fen()),
        "played": c.played,
        "stops": c.stops,
        "reasons": c.reasons.iter().map(|r| r.name()).collect::<Vec<_>>(),
        "movetext": c.game.movetext(),
        "graft": graft_json(&c.report),
        "read": {
            "played": c.read.played,
            "stops": c.read.stops.iter().map(|s| json!({ "text": s.text, "status": serde_json::to_value(s.status).unwrap_or(Value::Null) })).collect::<Vec<_>>(),
            "flags": c.read.flags.iter().map(|f| serde_json::to_value(f).unwrap_or(Value::Null)).collect::<Vec<_>>(),
            "rest": c.read.rest,
        },
    })
}

/// Make the line through `at` the main line, saying so when it could not be (it would nest the
/// old main line deeper than the cap).
fn make_main(game: &mut Game, at: NodeId) -> std::result::Result<(), Refusal> {
    let sans: Vec<String> =
        game.line_of(at).iter().skip(1).take(game.path_of(at).map_or(0, |p| p.indices().len())).map(|&n| game.san(n).to_owned()).collect();
    let depth = sans.len();
    game.promote(at);
    // The moves now lead from the start along first replies.
    let first_replies = NodePath::new(vec![0; depth]);
    let now = game.node_at(&first_replies).ok_or("variations would nest too deep")?;
    let line: Vec<String> = game.line_of(now).iter().skip(1).take(depth).map(|&n| game.san(n).to_owned()).collect();
    if line != sans {
        return Err("variations would nest too deep".into());
    }
    game.go_to_node(now);
    Ok(())
}

/// The move at an address, or why there is none.
pub(crate) fn resolve(game: &Game, at: &str) -> std::result::Result<NodeId, Refusal> {
    if at.len() > MAX_ADDRESS_LEN {
        return Err("at is too long".into());
    }
    if let Some(n) = at.strip_prefix("main:") {
        let ply: u32 = n.trim().parse().map_err(|_| format!("{at}: main: takes a half-move number"))?;
        return game
            .main_line()
            .into_iter()
            .find(|&id| game.abs_ply(id) == Some(ply))
            .ok_or_else(|| format!("{at}: the main line has no move there"));
    }
    if let Some(p) = at.strip_prefix("path:") {
        let p = p.trim();
        let indices = if p.is_empty() {
            Vec::new()
        } else {
            p.split('.')
                .map(|i| i.trim().parse::<u16>().map_err(|_| format!("{at}: path: takes reply indices such as 0.1.0")))
                .collect::<std::result::Result<Vec<_>, _>>()?
        };
        return game.node_at(&NodePath::new(indices)).ok_or_else(|| format!("{at}: there is no such move"));
    }
    Err(format!("{at}: an address is main:<half-move> or path:<i.j.k>"))
}

/// `path:<i.j.k>` for a move (`path:` for the start).
pub(crate) fn address_of(game: &Game, node: NodeId) -> String {
    path_text(&game.path_of(node).unwrap_or_default())
}

/// A path as an address: `path:<i.j.k>`.
fn path_text(path: &NodePath) -> String {
    let indices: Vec<String> = path.indices().iter().map(u16::to_string).collect();
    format!("path:{}", indices.join("."))
}

/// One fragment item from its JSON: `{"item": "move", "san": "Nd5", "ply"?, "bold"?}`,
/// `{"item": "nag", "code": 36}`, `{"item": "comment", "text": "…", "before"?}`, or
/// `{"item": "open" | "close" | "result"}`.
fn fragment_item(v: &Value) -> std::result::Result<FragmentItem, Refusal> {
    let kind = v.get("item").and_then(Value::as_str).ok_or("item is move, nag, comment, open, close or result")?;
    Ok(match kind {
        "move" => {
            let text = v.get("san").and_then(Value::as_str).ok_or("san is missing")?;
            let san = San::read(text, Lang::En).map_err(|e| e.to_string())?;
            let ply = match v.get("ply").filter(|p| !p.is_null()) {
                None => None,
                Some(p) => Some(p.as_u64().and_then(|p| u32::try_from(p).ok()).ok_or("ply is a half-move number")?),
            };
            let bold = match v.get("bold").filter(|b| !b.is_null()) {
                None => None,
                Some(b) => Some(b.as_bool().ok_or("bold is true or false")?),
            };
            FragmentItem::Move { ply, san, text: text.to_owned(), bold, rect: None }
        }
        "nag" => FragmentItem::Nag(v.get("code").and_then(Value::as_u64).and_then(|c| u16::try_from(c).ok()).ok_or("code is a glyph's PGN code")?),
        "comment" => {
            let text = v.get("text").and_then(Value::as_str).ok_or("text is missing")?;
            let before = match v.get("before").filter(|b| !b.is_null()) {
                None => false,
                Some(b) => b.as_bool().ok_or("before is true or false")?,
            };
            FragmentItem::Comment { text: text.to_owned(), before }
        }
        "open" => FragmentItem::Open,
        "close" => FragmentItem::Close,
        "result" => FragmentItem::Result,
        other => return Err(format!("unknown item {other:?}")),
    })
}

/// What a graft did, as the reply gives it (paths as addresses).
fn graft_json(r: &GraftReport) -> Value {
    json!({
        "played": r.played,
        "stop": r.stop.as_ref().map(|s| json!({
            "index": s.index,
            "reason": match s.reason {
                GraftStopReason::NoAnchor => "no_anchor",
                GraftStopReason::Illegal => "illegal",
                GraftStopReason::Ambiguous => "ambiguous",
                GraftStopReason::Unreadable => "unreadable",
                GraftStopReason::TooLong => "too_long",
                GraftStopReason::TooDeep => "too_deep",
                GraftStopReason::NullMove => "null_move",
            },
            "text": s.text,
            "maybe_en_passant": s.maybe_en_passant,
        })),
        "anchor": path_text(&r.anchor),
        "new_nodes": r.new_nodes.iter().map(path_text).collect::<Vec<_>>(),
        "demoted": r.demoted,
        "extended_tip": r.extended_tip,
        "unbalanced": r.unbalanced,
        "ply_mismatches": r.ply_mismatches,
        "unplaced": r.unplaced,
        "rest_from": r.rest_from,
        "leftover_text": r.leftover_text,
        "last": r.last.as_ref().map(path_text),
    })
}
