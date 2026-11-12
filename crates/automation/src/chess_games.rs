//! `chess_games`: Chess ▸ Games in this document without the interface (PGN toolbar wave 10, U2).
//!
//! A collection of games comes in as a PGN (inline in `pgn`, or the `.pgn` file `path`), with the
//! game shown (`active`), and goes back as a PGN in list order, with the list; with `out` it is
//! also written to that file (atomically, and only when every operation was applied). A list of
//! operations is applied in order, as the games list of the app does them: `list`, `set_active`,
//! `rename` (the White, Black, Event, Site, Date and Round tags), `duplicate`, `delete`, `move` (to any place),
//! `new_from` (a game from a move's position, after its game) and `export` (one game's PGN, or the
//! whole collection's).
//!
//! The order, the game shown after each change, the blank game the last delete leaves, the names
//! and the PGN split are the engine's ([`pdfcraft_engine::chess::collection`]), which the app's
//! list goes through too, so the two never differ. The app's own collections (autosaved in its
//! data folder, under its instance lock) are not touched: this tool works on PGN text and files.
//!
//! Both paths are confined to `--root` and resolved first, before anything is read. Each operation
//! adds an entry to the report; one that is refused adds an error entry, changes nothing and stops
//! the list there (and `out` is then not written).
//!
//! **Nothing is cut in silence** (plan A9, G7a; A4-05). A game is read up to the first move of its
//! PGN that cannot be played (`pgn_stopped`); since plan A9-F3 what follows it is kept as a comment
//! where the reading stopped, so writing the collection loses no text, but those moves would be
//! written as text, no longer moves. With such a game in the list, `out` is refused unless the
//! call says `allow_truncate: true`. `out` may be `path` (editing the file in place is what the tool is for),
//! never the file of an open document unless the call says `overwrite: true`.

use pdfcraft_engine::chess::collection::{self, GameList, GameNames, ListedGame, MAX_GAMES};
use pdfcraft_engine::chess::pdfcraft_chess::game::{Game, PgnStopReason};
use serde_json::{Map, Value, json};

use crate::{Args, Automation, Result, ToolError, chess_pgn, write_atomic};

/// Most operations in one call.
pub(crate) const MAX_OPS: usize = 1000;

/// Largest PGN read, inline or from a file (the app's Open PGN… reads as much).
pub(crate) const MAX_PGN_BYTES: usize = 32 << 20;

/// One game of the collection: the game, and the first move of its PGN that could not be played
/// (its line ends there) with why, said in the list.
struct Entry {
    game: Game,
    stopped: Option<String>,
    stop_reason: Option<PgnStopReason>,
}

impl ListedGame for Entry {
    fn blank() -> Entry {
        Entry { game: Game::standard(), stopped: None, stop_reason: None }
    }

    fn is_blank(&self) -> bool {
        self.stopped.is_none() && self.game.is_blank()
    }
}

/// Why one operation was refused: said in its report entry.
type Refusal = String;

impl Automation {
    pub(crate) fn chess_games(&self, a: &Args) -> Result<Value> {
        let inline = a.opt_str("pgn")?;
        if inline.is_some() && a.opt_str("path")?.is_some() {
            return Err(ToolError::InvalidArgs("give pgn or path, not both".into()));
        }
        // The paths next: one that leaves the root is refused before anything is read.
        let path = a.opt_str("path")?.map(|p| self.resolve(p, false).map(|r| (p, r))).transpose()?;
        let out = a.opt_str("out")?.map(|p| self.resolve(p, true)).transpose()?;
        let mut encoding = None;
        let text = match (inline, path) {
            (Some(_), Some(_)) => return Err(ToolError::InvalidArgs("give pgn or path, not both".into())),
            (Some(text), None) => {
                if text.len() > MAX_PGN_BYTES {
                    return Err(Args::wrong("pgn", &format!("at most {MAX_PGN_BYTES} bytes")));
                }
                text.to_owned()
            }
            (None, Some((given, file))) => {
                // Resolved above, and refused above the cap (a file that grew past it since too).
                let bytes = crate::chess_files::read_capped(&file, given, MAX_PGN_BYTES)?;
                let (text, read_as) = pdfcraft_engine::chess::pdfcraft_chess::pgn::decode(&bytes);
                encoding = Some(read_as.name());
                text
            }
            (None, None) => String::new(),
        };
        if let Some(target) = &out {
            self.check_outputs(a, &[("out", target.as_path())], &[])?;
        }
        let allow_truncate = a.opt_bool("allow_truncate")?.unwrap_or(false);
        let mut list = read_games(&text)?;
        if let Some(active) = a.opt_int("active")? {
            let shown = usize::try_from(active).ok().filter(|&i| list.switch_to(i));
            shown.ok_or_else(|| Args::wrong("active", &format!("a game of the list, 0 to {}", list.len().saturating_sub(1))))?;
        }
        let ops = match a.get("ops") {
            None => &[][..],
            Some(v) => v.as_array().map(Vec::as_slice).ok_or_else(|| Args::wrong("ops", "an array of operations"))?,
        };
        if ops.len() > MAX_OPS {
            return Err(Args::wrong("ops", &format!("at most {MAX_OPS} operations")));
        }
        let mut report = Vec::with_capacity(ops.len());
        let mut refused = false;
        for (index, op) in ops.iter().enumerate() {
            let name = op.get("op").and_then(Value::as_str).unwrap_or_default().to_owned();
            match apply(&mut list, op) {
                Ok(mut entry) => {
                    entry.insert("index".into(), json!(index));
                    entry.insert("op".into(), json!(name));
                    entry.insert("ok".into(), json!(true));
                    entry.insert("active".into(), json!(list.active_index()));
                    report.push(Value::Object(entry));
                }
                Err(error) => {
                    report.push(json!({ "index": index, "op": name, "ok": false, "error": error }));
                    refused = true;
                    break;
                }
            }
        }
        let pgn = export(&list, None);
        let written = match (&out, refused) {
            (Some(target), false) => {
                let stopped: Vec<String> =
                    list.iter().enumerate().filter_map(|(i, e)| e.stopped.as_ref().map(|mv| format!("game {i} at {mv}"))).collect();
                if !stopped.is_empty() && !allow_truncate {
                    return Err(ToolError::InvalidArgs(format!(
                        "out not written: {} read only up to a move that could not be played, and the rest of {} would be written \
                         as a comment there, no longer moves; correct the move, delete the game, or pass allow_truncate: true \
                         to write it so",
                        stopped.join(", "),
                        if stopped.len() == 1 { "it" } else { "them" },
                    )));
                }
                write_atomic(target, pgn.as_bytes())?;
                Some(self.shown_path(target).0)
            }
            _ => None,
        };
        let applied = report.iter().filter(|e| e["ok"] == true).count();
        Ok(json!({
            "games": list_json(&list),
            "active": list.active_index(),
            "count": list.len(),
            "encoding": encoding,
            "pgn": pgn,
            "report": report,
            "applied": applied,
            "written": written,
        }))
    }
}

/// The games of `text`, the first shown; one blank game for none. Refused when a game cannot be
/// read at all (rather than leave it out of what is written back) or there are more than
/// [`MAX_GAMES`].
fn read_games(text: &str) -> Result<GameList<Entry>> {
    let chunks = collection::split_pgn(text, MAX_GAMES);
    if chunks.len() > MAX_GAMES {
        return Err(Args::wrong("pgn", &format!("at most {MAX_GAMES} games")));
    }
    let mut games = Vec::with_capacity(chunks.len());
    for (i, chunk) in chunks.iter().enumerate() {
        let (game, report) = Game::from_pgn_report(chunk).map_err(|e| ToolError::InvalidArgs(format!("game {i}: {e}")))?;
        games.push(Entry { game, stopped: report.stopped, stop_reason: report.stop_reason });
    }
    Ok(GameList::from_games(games, 0))
}

/// The PGN of game `only`, or of every game in list order, a blank line between two.
fn export(list: &GameList<Entry>, only: Option<usize>) -> String {
    // Each game's PGN ends with a line break: joined by another, a blank line parts them.
    let texts: Vec<String> = list.iter().enumerate().filter(|(i, _)| only.is_none_or(|o| o == *i)).map(|(_, e)| e.game.pgn()).collect();
    texts.join("\n")
}

/// The list as the reply gives it.
fn list_json(list: &GameList<Entry>) -> Value {
    let shown = list.active_index();
    Value::Array(
        list.iter()
            .enumerate()
            .map(|(i, e)| {
                let names = GameNames::of(&e.game);
                json!({
                    "index": i,
                    "name": names.display(),
                    "white": names.white,
                    "black": names.black,
                    "event": names.event,
                    "site": names.site,
                    "date": names.date,
                    "round": names.round,
                    "moves": collection::scoresheet_moves(&e.game),
                    "start_fen": e.game.start_fen(),
                    "shown": i == shown,
                    "pgn_stopped": e.stopped,
                    "pgn_stop_reason": e.stop_reason.map(|r| r.name()),
                })
            })
            .collect(),
    )
}

/// The game an operation names (`game`, 0-based), the game shown when it names none.
fn game_of(list: &GameList<Entry>, obj: &Map<String, Value>) -> std::result::Result<usize, Refusal> {
    match obj.get("game") {
        None | Some(Value::Null) => Ok(list.active_index()),
        Some(v) => v
            .as_u64()
            .and_then(|g| usize::try_from(g).ok())
            .filter(|&g| g < list.len())
            .ok_or_else(|| format!("game {v}: the list has games 0 to {}", list.len().saturating_sub(1))),
    }
}

/// An optional string field of an operation.
fn opt_text<'a>(obj: &'a Map<String, Value>, key: &str) -> std::result::Result<Option<&'a str>, Refusal> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_str().map(Some).ok_or_else(|| format!("{key} is a string")),
    }
}

/// Apply one operation; the list is unchanged when it is refused.
fn apply(list: &mut GameList<Entry>, op: &Value) -> std::result::Result<Map<String, Value>, Refusal> {
    let obj = op.as_object().ok_or("an operation is an object")?;
    let name = obj.get("op").and_then(Value::as_str).ok_or("op is missing")?;
    let mut entry = Map::new();
    match name {
        "list" => {
            entry.insert("games".into(), list_json(list));
        }
        "set_active" => {
            let g = game_of(list, obj)?;
            list.switch_to(g);
        }
        "rename" => {
            let g = game_of(list, obj)?;
            let game = &mut list.get_mut(g).ok_or("no such game")?.game;
            let mut names = GameNames::of(game);
            for (key, field) in [
                ("white", &mut names.white),
                ("black", &mut names.black),
                ("event", &mut names.event),
                ("site", &mut names.site),
                ("date", &mut names.date),
                ("round", &mut names.round),
            ] {
                if let Some(value) = opt_text(obj, key)? {
                    *field = value.to_owned();
                }
            }
            // A date given must be one PGN reads; one the game had is kept as it was.
            if let Some(date) = opt_text(obj, "date")?
                && !collection::pgn_date_ok(date)
            {
                return Err(format!("date {:?} is not a PGN date: YYYY.MM.DD, ? for each unknown digit (1992.??.??)", date.trim()));
            }
            let renamed = collection::rename(game, &names);
            entry.insert("changed".into(), json!(renamed.changed));
            // Plan A9-F3 (A4-22): a value cut to the tag's cap, or a tag a full game refused, is said.
            entry.insert("cut".into(), json!(renamed.cut));
            entry.insert("refused".into(), json!(renamed.refused));
            entry.insert("name".into(), json!(GameNames::of(game).display()));
        }
        "duplicate" => {
            let g = game_of(list, obj)?;
            let copy =
                list.get(g).map(|e| Entry { game: e.game.clone(), stopped: e.stopped.clone(), stop_reason: e.stop_reason }).ok_or("no such game")?;
            entry.insert("game".into(), json!(list.insert_after(g, copy)));
        }
        "delete" => {
            let g = game_of(list, obj)?;
            let gone = list.delete(g).ok_or("no such game")?;
            entry.insert("deleted".into(), json!(GameNames::of(&gone.game).display()));
        }
        "move" => {
            let g = game_of(list, obj)?;
            let to = obj
                .get("to")
                .and_then(Value::as_u64)
                .and_then(|t| usize::try_from(t).ok())
                .filter(|&t| t < list.len())
                .ok_or_else(|| format!("to is the game's place once moved, 0 to {}", list.len().saturating_sub(1)))?;
            entry.insert("moved".into(), json!(list.move_to(g, to)));
            entry.insert("game".into(), json!(to));
        }
        "new_from" => {
            let g = game_of(list, obj)?;
            let from = &list.get(g).ok_or("no such game")?.game;
            // The main line's last move by default: a game read from a PGN has no move shown.
            let node = match opt_text(obj, "at")? {
                Some(at) => Some(chess_pgn::resolve(from, at)?),
                None => from.main_line().last().copied(),
            };
            let game = collection::game_from(from, node).map_err(|e| e.to_string())?;
            entry.insert("game".into(), json!(list.insert_after(g, Entry { game, stopped: None, stop_reason: None })));
        }
        "new_at" => {
            let g = game_of(list, obj)?;
            let fen = opt_text(obj, "fen")?.ok_or("fen is missing: the position the new game starts at")?;
            let game = Game::from_fen(fen).map_err(|e| format!("fen: {e}"))?;
            entry.insert("game".into(), json!(list.insert_after(g, Entry { game, stopped: None, stop_reason: None })));
        }
        "export" => {
            let only = match obj.get("game") {
                None | Some(Value::Null) => None,
                Some(_) => Some(game_of(list, obj)?),
            };
            entry.insert("pgn".into(), json!(export(list, only)));
        }
        other => return Err(format!("op {other:?} is list, set_active, rename, duplicate, delete, move, new_from, new_at or export")),
    }
    Ok(entry)
}
