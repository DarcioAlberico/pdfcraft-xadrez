//! `chess_pgn_edit`: reshape a game's moves without the interface, inline only.
//!
//! The game comes in the arguments (a PGN or a FEN) and goes back in the reply; no file and no
//! document is read or written, so the tool takes no path. A PGN may hold several games: `game`
//! (0-based, the first by default) picks the one edited, cut as `chess_games` cuts them, and the
//! reply says how many there are (`games`); only that game comes back. A list of operations is applied in
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
//! applies the chosen place unless told otherwise (`apply: renumber` takes the Renumber choice);
//! its entry lists every candidate place.
//!
//! Each operation adds an entry to the report. One that is refused (a bad address, an illegal
//! move, a structure change the tree does not allow) adds an error entry, leaves the game as it
//! was before it, and stops the list there.

use pdfcraft_engine::chess::capture::{self, Candidate, CaptureMode, CapturePlan, CaptureText};
use pdfcraft_engine::chess::collection::{self, MAX_GAMES};
use pdfcraft_engine::chess::pdfcraft_chess::fragment::{FragmentItem, NagCode};
use pdfcraft_engine::chess::pdfcraft_chess::game::{
    Game, GraftMode, GraftReport, GraftStopReason, MAX_COMMENT_LEN, MAX_NODES, NodeId, NodePath, PgnReport, Shift,
};
use pdfcraft_engine::chess::pdfcraft_chess::pgn_style::PgnStyle;
use pdfcraft_engine::chess::pdfcraft_chess::san::{Lang, San, Style};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::{Args, Automation, Result, ToolError};

/// Most operations in one call (for a game of up to [`MAX_WORK`] / [`MAX_OPS`] moves).
pub(crate) const MAX_OPS: usize = 2000;

/// The work one call may do: operations times the moves of the game (its own and the ones the
/// operations play). Every operation may visit the whole game (an address on the main line, the
/// reading order, a graft's copy of it), so the operations allowed shrink as the game grows:
/// [`MAX_OPS`] up to 1,000 moves, 200 at 10,000 (plan A9-F6, R8-08).
pub(crate) const MAX_WORK: usize = 2_000_000;

/// The operations one call may hold for a game of `moves` moves ([`MAX_WORK`]).
pub(crate) fn max_ops(moves: usize) -> usize {
    (MAX_WORK / moves.max(1)).min(MAX_OPS)
}

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
        let chosen = a.opt_int("game")?;
        if chosen.is_some() && pgn.is_none() {
            return Err(ToolError::InvalidArgs("game picks a game of pgn: give pgn".into()));
        }
        // How many games the PGN holds, said in the reply (none without one).
        let mut games = 0usize;
        let style = pgn_style(a)?;
        // The first move of the PGN that could not be played (its line ends there), and the
        // variations kept as comments, said in the reply.
        let (mut game, read) = match (fen, pgn) {
            (Some(_), Some(_)) => return Err(ToolError::InvalidArgs("give fen or pgn, not both".into())),
            (Some(fen), None) => (Game::from_fen(fen).map_err(|e| ToolError::InvalidArgs(format!("fen: {e}")))?, PgnReport::default()),
            (None, Some(pgn)) => {
                let (text, count) = pick_game(pgn, chosen)?;
                games = count;
                Game::from_pgn_report(text).map_err(|e| ToolError::InvalidArgs(format!("pgn: {e}")))?
            }
            (None, None) => (Game::standard(), PgnReport::default()),
        };
        // Addresses as the PGN read was: the ops may move its moves after.
        let warnings: Vec<Value> = read.warnings.iter().map(|w| json!({ "kind": w.kind.name(), "at": address_of(&game, w.at) })).collect();
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
        let limit = max_ops(game.move_count().saturating_add(steps).min(MAX_NODES));
        if ops.len() > limit {
            let adding = if steps > 0 { format!(" and {steps} moves and items to add") } else { String::new() };
            return Err(Args::wrong("ops", &format!("at most {limit} operations for a game of {} moves{adding}", game.move_count())));
        }
        let mut report = Vec::with_capacity(ops.len());
        for (index, op) in ops.iter().enumerate() {
            let name = op.get("op").and_then(Value::as_str).unwrap_or_default().to_owned();
            match PgnOp::read(op).and_then(|op| op.apply(&mut game)) {
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
            "pgn": game.pgn_with(style),
            "movetext": game.movetext_with(style),
            "report": report,
            "applied": applied,
            "pgn_stopped": read.stopped,
            "pgn_stop_reason": read.stop_reason.map(|r| r.name()),
            "pgn_warnings": warnings,
            "current_path": address_of(&game, game.current()),
            "games": games,
            "game": chosen.unwrap_or(0),
        }))
    }
}

/// The PGN writers' options (plan A8 V4): `variations` (`all`, `main`, `depth:N`), `comments`
/// and `nags` (`all`, `none`); everything by default.
pub(crate) fn pgn_style(a: &Args) -> Result<PgnStyle> {
    let mut style = PgnStyle::FULL;
    if let Some(name) = a.opt_str("variations")? {
        style.variations = PgnStyle::variations_from_name(name).ok_or_else(|| Args::wrong("variations", "all, main or depth:N"))?;
    }
    if let Some(name) = a.opt_str("comments")? {
        style.comments = PgnStyle::switch_from_name(name).ok_or_else(|| Args::wrong("comments", "all or none"))?;
    }
    if let Some(name) = a.opt_str("nags")? {
        style.nags = PgnStyle::switch_from_name(name).ok_or_else(|| Args::wrong("nags", "all or none"))?;
    }
    Ok(style)
}

/// Game `chosen` (0-based, the first when `None`) of a PGN that may hold several, cut as the app's
/// Open PGN… and `chess_games` cut it ([`collection::split_pgn`]), and how many games it holds. A
/// PGN with no game in it (empty, or only blank lines) is one blank game. Refused past the last
/// game, and for more than [`MAX_GAMES`] games (only that many and one more are cut).
fn pick_game(pgn: &str, chosen: Option<i64>) -> Result<(&str, usize)> {
    let chunks = collection::split_pgn(pgn, MAX_GAMES);
    if chunks.len() > MAX_GAMES {
        return Err(Args::wrong("pgn", &format!("at most {MAX_GAMES} games")));
    }
    let count = chunks.len().max(1);
    let index = chosen.unwrap_or(0);
    let text = match usize::try_from(index) {
        Ok(0) if chunks.is_empty() => pgn,
        Ok(i) => chunks.get(i).copied().ok_or_else(|| {
            ToolError::InvalidArgs(format!(
                "game {index}: the PGN holds {count} game{}, 0 to {}",
                if count == 1 { "" } else { "s" },
                count.saturating_sub(1)
            ))
        })?,
        Err(_) => return Err(Args::wrong("game", "a game of the PGN, 0 or more")),
    };
    Ok((text, count))
}

/// The operations of `chess_pgn_edit` (plan A9-F6, A7-12), read from JSON by their `op` into
/// fields of types that are valid once they exist: a glyph's code is 1–255, a count of steps
/// 1–[`MAX_STEPS`], a direction up or down, a capture's language, mode, bold ranges and choice. An
/// address stays text until its operation runs: the operations before it change the tree. A field
/// an operation needs and does not have is said when it runs. Fields an operation does not know
/// are accepted, as they always were; an `op` this list does not have is refused as
/// `unknown op "<x>"`.
#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum PgnOp {
    Play {
        at: Option<Address>,
        moves: Option<Moves>,
    },
    Comment {
        at: Option<Address>,
        text: Option<Text>,
    },
    CommentBefore {
        at: Option<Address>,
        text: Option<Text>,
    },
    Nag {
        at: Option<Address>,
        code: Option<GlyphCode>,
    },
    PlayNull {
        at: Option<Address>,
    },
    MoveVariation {
        at: Option<Address>,
        to: Option<Way>,
    },
    Diagram {
        at: Option<Address>,
        on: Option<Switch>,
    },
    PromoteOne {
        at: Option<Address>,
    },
    MakeMain {
        at: Option<Address>,
    },
    Demote {
        at: Option<Address>,
    },
    DeleteVariation {
        at: Option<Address>,
    },
    Cut {
        at: Option<Address>,
    },
    Back {
        at: Option<Address>,
        n: Option<Steps>,
    },
    Forward {
        at: Option<Address>,
        n: Option<Steps>,
    },
    Choose {
        at: Option<Address>,
        index: Option<ReplyIndex>,
    },
    NextVariation {
        at: Option<Address>,
    },
    PrevVariation {
        at: Option<Address>,
    },
    Graft {
        at: Option<Address>,
        mode: Option<GraftModeName>,
        items: Option<Items>,
    },
    Capture {
        at: Option<Address>,
        text: Option<Text>,
        lang: Option<LangTag>,
        mode: Option<CaptureModeName>,
        bold_runs: Option<BoldRuns>,
        apply: Option<ApplyChoice>,
    },
}

/// Every operation's name, in the order the tool's schema lists them.
pub(crate) const OP_NAMES: [&str; 19] = [
    "play",
    "comment",
    "comment_before",
    "nag",
    "play_null",
    "move_variation",
    "diagram",
    "promote_one",
    "make_main",
    "demote",
    "delete_variation",
    "cut",
    "back",
    "forward",
    "choose",
    "next_variation",
    "prev_variation",
    "graft",
    "capture",
];

/// A field read from JSON by `TryFrom<Value>`; its refusal is what the report says.
macro_rules! field {
    ($(#[$doc:meta])* $name:ident($inner:ty), |$v:ident| $read:expr) => {
        $(#[$doc])*
        #[derive(Debug, Deserialize)]
        #[serde(try_from = "Value")]
        struct $name($inner);

        impl TryFrom<Value> for $name {
            type Error = String;
            fn try_from($v: Value) -> std::result::Result<$name, String> {
                $read.map($name)
            }
        }
    };
}

field!(
    /// An address, `main:<half-move>` or `path:<i.j.k>`, read when its operation runs.
    Address(String),
    |v| v.as_str().map(str::to_owned).ok_or_else(|| "at is main:<half-move> or path:<i.j.k>".to_owned())
);
field!(
    /// Moves in SAN or UCI, played in order.
    Moves(Vec<String>),
    |v| v
        .as_array()
        .ok_or_else(|| "moves is an array of moves in SAN or UCI".to_owned())?
        .iter()
        .map(|m| m.as_str().map(str::to_owned).ok_or_else(|| "moves are strings".to_owned()))
        .collect::<std::result::Result<Vec<String>, String>>()
);
field!(
    /// A comment's text, or a capture's.
    Text(String),
    |v| v.as_str().map(str::to_owned).ok_or_else(|| "text is a string".to_owned())
);
field!(
    /// A graft's fragment items, read when it runs, at most [`MAX_ITEMS`].
    Items(Vec<Value>),
    |v| match v {
        Value::Array(items) if items.len() > MAX_ITEMS => Err(format!("at most {MAX_ITEMS} items")),
        Value::Array(items) => Ok(items),
        _ => Err("items is an array of fragment items".to_owned()),
    }
);
field!(
    /// A glyph's PGN code, 1–255.
    GlyphCode(u16),
    |v| v.as_u64().and_then(|c| u16::try_from(c).ok()).filter(|c| (1..=255).contains(c)).ok_or_else(|| {
        format!("code is a glyph's PGN code, 1-255 ({v} cannot be put on a move)")
    })
);
field!(
    /// Up or down among the variations at a fork.
    Way(Shift),
    |v| match v.as_str() {
        Some("up") => Ok(Shift::Up),
        Some("down") => Ok(Shift::Down),
        _ => Err("to is up or down".to_owned()),
    }
);
field!(
    /// A switch.
    Switch(bool),
    |v| v.as_bool().ok_or_else(|| "on is true or false".to_owned())
);
field!(
    /// A count of moves to step, 1–[`MAX_STEPS`].
    Steps(usize),
    |v| v.as_u64().and_then(|n| usize::try_from(n).ok()).filter(|n| (1..=MAX_STEPS).contains(n)).ok_or_else(|| format!("n is a number of moves, 1-{MAX_STEPS}"))
);
field!(
    /// A reply to step into: 0 the line, 1 its first variation…
    ReplyIndex(u64),
    |v| v.as_u64().ok_or_else(|| "index is the reply to step into (0: the line, 1: its first variation…)".to_owned())
);
field!(
    /// How a graft places its items.
    GraftModeName(GraftMode),
    |v| match v.as_str() {
        Some("follow") => Ok(GraftMode::Follow),
        Some("line") => Ok(GraftMode::Line),
        Some("comment") => Ok(GraftMode::Comment),
        _ => Err(format!("mode {v} is follow, line or comment")),
    }
);
field!(
    /// The language a capture's piece letters are in.
    LangTag(Lang),
    |v| v.as_str().and_then(Lang::from_tag).ok_or_else(|| "lang is a language tag such as en or pt".to_owned())
);
field!(
    /// Where a capture's text goes.
    CaptureModeName(CaptureMode),
    |v| match v.as_str() {
        Some(name) => capture_mode(Some(name)),
        None => Err(format!("mode {v} is auto, line, variation, subvariation, comment or comment_before")),
    }
);
field!(
    /// The characters of a capture's text set in bold, as `[start, end)` ranges.
    BoldRuns(Vec<std::ops::Range<usize>>),
    |v| bold_runs(&v)
);
field!(
    /// Which of a capture's places is applied.
    ApplyChoice(String),
    |v| match v.as_str() {
        Some(name @ ("chosen" | "auto" | "never" | "renumber")) => Ok(name.to_owned()),
        _ => Err(format!("apply {v} is chosen, auto, never or renumber")),
    }
);

/// A capture's `bold_runs`: at most [`MAX_BOLD_RUNS`] `[start, end]` pairs of character indices.
fn bold_runs(v: &Value) -> std::result::Result<Vec<std::ops::Range<usize>>, String> {
    let runs = v.as_array().ok_or("bold_runs is an array of [start, end] character ranges")?;
    if runs.len() > MAX_BOLD_RUNS {
        return Err(format!("at most {MAX_BOLD_RUNS} bold_runs"));
    }
    let bound = |v: &Value| v.as_u64().map(|n| usize::try_from(n).unwrap_or(usize::MAX));
    runs.iter()
        .map(|r| match r.as_array().map(Vec::as_slice) {
            Some([a, b]) => bound(a).zip(bound(b)).map(|(start, end)| start..end),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| "bold_runs are [start, end] pairs of character indices".to_owned())
}

impl PgnOp {
    /// Read one operation: its name first (one this list does not have is refused by name),
    /// then its fields, each into its type.
    fn read(op: &Value) -> std::result::Result<PgnOp, Refusal> {
        let name = op.as_object().ok_or("an operation is an object")?.get("op").and_then(Value::as_str).ok_or("op is missing")?;
        if !OP_NAMES.contains(&name) {
            return Err(format!("unknown op {name:?}"));
        }
        PgnOp::deserialize(op).map_err(|e| e.to_string())
    }

    /// Where the operation runs: its address, or for a step through the game the move shown.
    fn at(&self) -> Option<&str> {
        match self {
            PgnOp::Play { at, .. }
            | PgnOp::Comment { at, .. }
            | PgnOp::CommentBefore { at, .. }
            | PgnOp::Nag { at, .. }
            | PgnOp::PlayNull { at }
            | PgnOp::MoveVariation { at, .. }
            | PgnOp::Diagram { at, .. }
            | PgnOp::PromoteOne { at }
            | PgnOp::MakeMain { at }
            | PgnOp::Demote { at }
            | PgnOp::DeleteVariation { at }
            | PgnOp::Cut { at }
            | PgnOp::Back { at, .. }
            | PgnOp::Forward { at, .. }
            | PgnOp::Choose { at, .. }
            | PgnOp::NextVariation { at }
            | PgnOp::PrevVariation { at }
            | PgnOp::Graft { at, .. }
            | PgnOp::Capture { at, .. } => at.as_ref().map(|a| a.0.as_str()),
        }
    }

    /// Whether the operation steps through the game, from the move shown when it has no address.
    fn steps(&self) -> bool {
        matches!(self, PgnOp::Back { .. } | PgnOp::Forward { .. } | PgnOp::Choose { .. } | PgnOp::NextVariation { .. } | PgnOp::PrevVariation { .. })
    }

    /// Apply the operation; the game is unchanged when it is refused.
    fn apply(self, game: &mut Game) -> std::result::Result<Map<String, Value>, Refusal> {
        let op = self;
        let at = match op.at() {
            Some(at_text) => resolve(game, at_text)?,
            None if op.steps() => game.current(),
            None => return Err("at is missing (main:<half-move> or path:<i.j.k>)".into()),
        };
        let mut entry = Map::new();
        match op {
            PgnOp::Play { moves, .. } => {
                let Moves(moves) = moves.ok_or("moves is missing")?;
                // Played on the game itself; a move that cannot be played takes back the ones
                // before it (plan A9-F6: a copy of the whole game was kept for each list).
                let mark = game.mark();
                game.go_to_node(at);
                for mv in &moves {
                    // UCI first: `e2e4` is not SAN, and no SAN reads as UCI.
                    if game.play_uci(mv).is_err() && game.play_san(mv).is_err() {
                        game.roll_back(mark);
                        return Err(format!("{mv} cannot be played there"));
                    }
                }
            }
            PgnOp::Comment { text, .. } => set_comment(game, at, &text.ok_or("text is missing")?.0, false)?,
            PgnOp::CommentBefore { text, .. } => set_comment(game, at, &text.ok_or("text is missing")?.0, true)?,
            PgnOp::Nag { code, .. } => {
                let GlyphCode(code) = code.ok_or("code is a glyph's PGN code, 1-255")?;
                if !game.set_nag(at, code) {
                    return Err(format!("${code} cannot be put on this move (the start, a code outside 1-255, or too many glyphs)"));
                }
                game.go_to_node(at);
            }
            PgnOp::PlayNull { .. } => {
                let mark = game.mark();
                game.go_to_node(at);
                if game.play_null().is_err() {
                    game.roll_back(mark);
                    return Err("a null move cannot be played there: the side to move is in check, or the game is full".into());
                }
            }
            PgnOp::MoveVariation { to, .. } => {
                let Way(shift) = to.ok_or("to is up or down")?;
                game.move_variation(at, shift).map_err(|e| e.to_string())?;
            }
            PgnOp::Diagram { on, .. } => {
                let on = on.is_none_or(|Switch(on)| on);
                if on != game.has_diagram(at) && !game.toggle_diagram(at) {
                    return Err("a diagram mark cannot go on this move (the start, or too many glyphs)".into());
                }
                game.go_to_node(at);
            }
            PgnOp::PromoteOne { .. } => game.promote_one(at).map_err(|e| e.to_string())?,
            PgnOp::Demote { .. } => game.demote(at).map_err(|e| e.to_string())?,
            PgnOp::DeleteVariation { .. } => game.delete_variation(at).map_err(|e| e.to_string())?,
            PgnOp::MakeMain { .. } => make_main(game, at)?,
            PgnOp::Back { n, .. } => {
                entry.insert("moved".into(), json!(step(game, at, n, Game::back)));
            }
            PgnOp::Forward { n, .. } => {
                entry.insert("moved".into(), json!(step(game, at, n, Game::forward)));
            }
            PgnOp::NextVariation { .. } => {
                let to = game.next_variation(at);
                entry.insert("moved".into(), json!(to.is_some()));
                game.go_to_node(to.unwrap_or(at));
            }
            PgnOp::PrevVariation { .. } => {
                let to = game.previous_variation(at);
                entry.insert("moved".into(), json!(to.is_some()));
                game.go_to_node(to.unwrap_or(at));
            }
            PgnOp::Choose { index, .. } => {
                let ReplyIndex(index) = index.ok_or("index is the reply to step into (0: the line, 1: its first variation…)")?;
                game.go_to_node(at);
                let replies = game.replies(at).len();
                if !usize::try_from(index).is_ok_and(|i| game.forward_to(i)) {
                    return Err(format!("the move has {replies} replies, so there is no reply {index}"));
                }
            }
            PgnOp::Cut { .. } => {
                game.go_to_node(at);
                game.truncate();
            }
            PgnOp::Graft { mode, items, .. } => {
                let mode = mode.map_or(GraftMode::Follow, |GraftModeName(m)| m);
                let Items(raw) = items.ok_or("items is missing")?;
                let items = raw
                    .iter()
                    .enumerate()
                    .map(|(i, v)| fragment_item(v).map_err(|e| format!("items[{i}]: {e}")))
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                let (grafted, graft) = game.graft(at, mode, &items, None);
                *game = grafted;
                entry.insert("graft".into(), graft_json(&graft));
            }
            PgnOp::Capture { text, lang, mode, bold_runs, apply, .. } => {
                let Text(text) = text.ok_or("text is missing")?;
                let lang = lang.map_or(Lang::En, |LangTag(l)| l);
                let mode = mode.map_or(CaptureMode::Auto, |CaptureModeName(m)| m);
                let bold = bold_runs.map(|BoldRuns(b)| b);
                let captured = match &bold {
                    Some(bold) => CaptureText::Styled { text: &text, bold },
                    None => CaptureText::Plain(&text),
                };
                let plan = capture::plan(game, at, mode, captured, lang);
                let (apply, choice) = match apply.as_ref().map_or("chosen", |ApplyChoice(a)| a.as_str()) {
                    "auto" => (plan.chosen.is_some() && !plan.needs_preview, None),
                    "never" => (false, None),
                    // The Renumber choice, when the plan has one.
                    "renumber" => (plan.renumber.is_some(), plan.renumber),
                    _ => (plan.chosen.is_some(), None),
                };
                // Planned on this very game, so only a missing candidate can refuse it.
                let applied = apply && plan.apply(game, choice).is_ok();
                if !applied {
                    game.go_to_node(at);
                }
                entry.insert("capture".into(), capture_json(&plan, applied, lang));
            }
        }
        entry.insert("path".into(), json!(address_of(game, game.current())));
        Ok(entry)
    }
}

/// Set the comment after move `at`, or before it, at most [`MAX_COMMENT_LEN`] bytes, and show
/// the move.
fn set_comment(game: &mut Game, at: NodeId, text: &str, before: bool) -> std::result::Result<(), Refusal> {
    if text.len() > MAX_COMMENT_LEN {
        return Err(format!("text is longer than {MAX_COMMENT_LEN} bytes"));
    }
    let set = if !before {
        game.set_comment(at, text)
    } else if at == 0 {
        return Err("the start has no comment before it (use comment on it)".into());
    } else {
        game.set_comment_before(at, text)
    };
    // Refused above the cap, so a cut can't happen; were it to, it is said.
    set.map_err(|c| format!("the comment was cut to {} bytes", c.kept))?;
    game.go_to_node(at);
    Ok(())
}

/// Step `n` moves (one by default) from `at` with `one` (back or forward); how many it moved.
fn step(game: &mut Game, at: NodeId, n: Option<Steps>, one: fn(&mut Game) -> bool) -> usize {
    let n = n.map_or(1, |Steps(n)| n);
    game.go_to_node(at);
    let mut moved = 0usize;
    while moved < n && one(game) {
        moved += 1;
    }
    moved
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
        "renumber": plan.renumber,
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
        "shifted": c.shifted,
        "renumbering": c.renumbering().map(|(printed, played)| json!({ "printed": printed, "played": played })),
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
/// old main line deeper than the cap), and show the move there.
fn make_main(game: &mut Game, at: NodeId) -> std::result::Result<(), Refusal> {
    let depth = game.path_of(at).map_or(0, |p| p.indices().len());
    game.promote(at).map_err(|e| e.to_string())?;
    // The moves now lead from the start along first replies.
    let now = game.node_at(&NodePath::new(vec![0; depth])).ok_or("there is no such move")?;
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
        "nag" => FragmentItem::Nag(NagCode::WhiteRelative(
            v.get("code").and_then(Value::as_u64).and_then(|c| u16::try_from(c).ok()).ok_or("code is a glyph's PGN code")?,
        )),
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
        "already_there": r.already_there,
        "leftover_text": r.leftover_text,
        "last": r.last.as_ref().map(path_text),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    /// Plan A9-F6 (A7-12): the tool's schema lists the operations the enum reads, and the fields
    /// they read, no more and no fewer. Both are asked of the enum itself, not of a list kept
    /// beside it: its names from serde's refusal of a name it does not have, and the fields an
    /// operation reads from whether it refuses an object put in one (every field's type does; a
    /// field it does not read is let through).
    #[test]
    fn the_schema_lists_the_operations_the_enum_reads() {
        let def = crate::tools::find("chess_pgn_edit").unwrap();
        let item = &def.input_schema["properties"]["ops"]["items"]["properties"];
        let names: Vec<&str> = item["op"]["enum"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        assert_eq!(names, OP_NAMES);
        let refusal = PgnOp::deserialize(&json!({ "op": "?" })).unwrap_err().to_string();
        let (_, listed) = refusal.split_once("expected one of ").unwrap();
        let read: Vec<&str> = listed.split(", ").map(|n| n.trim_matches('`')).collect();
        assert_eq!(read, OP_NAMES, "{refusal}");
        let schema_fields: BTreeSet<&str> = item.as_object().unwrap().keys().map(String::as_str).filter(|&k| k != "op").collect();
        let mut read_fields = BTreeSet::new();
        for name in OP_NAMES {
            let read = PgnOp::read(&json!({ "op": name }));
            assert!(read.is_ok(), "{name}: {read:?}");
            for &field in &schema_fields {
                if PgnOp::read(&json!({ "op": name, field: { "?": 0 } })).is_err() {
                    read_fields.insert(field);
                }
            }
        }
        assert_eq!(read_fields, schema_fields);
        assert_eq!(PgnOp::read(&json!({ "op": "rotate" })).err().as_deref(), Some("unknown op \"rotate\""));
        assert_eq!(PgnOp::read(&json!("cut")).err().as_deref(), Some("an operation is an object"));
        assert_eq!(PgnOp::read(&json!({ "at": "path:" })).err().as_deref(), Some("op is missing"));
        assert_eq!((max_ops(1), max_ops(1000), max_ops(10_000)), (MAX_OPS, MAX_OPS, 200));
    }
}
