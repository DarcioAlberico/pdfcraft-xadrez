//! Chess ▸ Board ▸ Analysis: a UCI chess engine the user installed (Stockfish or any
//! other), run as a child process, analysing the position the board shows.
//!
//! None ships with PdfCraft and none is fetched. The engine is `$PDFCRAFT_CHESS_ENGINE`, the
//! program in an `engine` folder beside the executable (in the install folder the frontend passes,
//! plan A9-T3), or one the user picks ([`find_engine`]).
//! It is a separate process speaking UCI over its standard input and output, so its licence does
//! not reach PdfCraft. Desktop only: the web build cannot start processes.
//!
//! The engine is untrusted, like a file. Its lines are read bounded ([`pdfcraft_chess::uci`]), a
//! line about a position it is no longer analysing is dropped, an engine that never finishes the
//! handshake is reported after [`HANDSHAKE_TIMEOUT`], and dropping the [`Analyser`] ends the
//! process. Commands go through a writer thread, so an engine that stops reading cannot hold up
//! the interface.
//!
//! The lines found for the last [`MAX_KEPT`] positions are kept for the session: back on one of
//! them (or on the position it was stopped on), the analyser shows them at once and searches on,
//! showing them until the new search goes deeper; a position searched to the depth asked for is
//! not searched again. A position is what the engine was given: the position and the moves before
//! it ([`Analyser::analyse_game`]).
//!
//! **What the engine is told** (plan A9-N8, A8-11, R6-09). An option is sent only when the engine
//! announced it in its handshake, under the name it announced, a number within the bounds it
//! announced, and text only without a line break or another control character (one would be a
//! second command); the engine's own default stays otherwise, and [`Snapshot::refused`] says why
//! a setting was not sent ([`EngineSettings`]). At most [`MAX_OPTIONS`] options are read from the
//! handshake. A game's position goes with the moves since its last capture or pawn move, at most
//! [`MAX_HISTORY`] (`position fen <start> moves …`), so the engine sees a repetition coming.
//!
//! Only the interface starts an engine. No automation tool does: a tool that runs a program
//! named in its arguments would let any MCP client run any program.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use pdfcraft_chess::game::Game;
use pdfcraft_chess::placement::Color;
use pdfcraft_chess::uci::{self, Bound, EngineOption, MAX_LINE_LEN, MAX_MULTIPV, Message, OptionKind, Score};

/// How long an engine has to answer `uci` with `uciok`.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

/// How deep a search goes before the engine stops by itself, so an open board does not keep a
/// processor busy for ever. Deeper than any analysis board shows by default.
pub const DEFAULT_DEPTH: u32 = 30;

/// The deepest search asked for.
pub const MAX_DEPTH: u32 = 99;

/// Most entries looked at while searching the `engine` folder, over all its folders together.
const MAX_DIR_ENTRIES: usize = 256;

/// How far below the `engine` folder a program is looked for. An engine is usually unpacked as
/// it was downloaded, so it sits in a folder of its own (`engine/stockfish/stockfish.exe`), or
/// one deeper (`engine/stockfish-17/stockfish/stockfish.exe`).
const MAX_FOLDER_DEPTH: usize = 3;

/// How long a closed engine has to quit before it is killed.
const QUIT_GRACE: Duration = Duration::from_millis(400);

/// Moves of a line turned into SAN (the rest stay in UCI).
const MAX_SAN: usize = 24;

/// Positions whose lines are kept for when the board comes back to them.
pub const MAX_KEPT: usize = 64;

/// Most options read from an engine's handshake; the rest are not known to it here.
pub const MAX_OPTIONS: usize = 256;

/// Most moves given before a game's position: the fifty-move rule's 100 half-moves.
pub const MAX_HISTORY: usize = 100;

/// Longest text sent as an option's value, in bytes (a list of folders).
pub const MAX_OPTION_TEXT: usize = 4096;

/// A real UCI engine for the tests that run one, from `PDFCRAFT_CHESS_TEST_ENGINE`; `Ok(None)` skips
/// and `PDFCRAFT_REQUIRE_MODELS=1` makes a missing one an error (see
/// [`pdfcraft_chessvision::models::test_models`]).
pub fn test_engine() -> Result<Option<PathBuf>, String> {
    pdfcraft_chessvision::models::test_resource("PDFCRAFT_CHESS_TEST_ENGINE", "a UCI engine program")
}

/// The engine program to use: `$PDFCRAFT_CHESS_ENGINE` when it names a file, else the program
/// in the `engine` folder of `install` (the folder the app is installed in: the executable's,
/// which the frontend resolves) or in a folder inside it. `None` without either.
pub fn find_engine(install: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("PDFCRAFT_CHESS_ENGINE").map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(path);
    }
    program_in(&install?.join("engine"))
}

/// The first program in `dir`, by name: an `.exe` on Windows, a file anyone may execute
/// elsewhere. A program directly in `dir` wins; with none there, the folders inside it are
/// looked through in the same way, by name, down to [`MAX_FOLDER_DEPTH`], because an engine is
/// usually left in the folder it was unpacked into.
///
/// The search is bounded: [`MAX_DIR_ENTRIES`] entries in all and that depth, so a folder of
/// thousands of files, or links that lead round in a circle, cannot hold it up.
pub fn program_in(dir: &Path) -> Option<PathBuf> {
    let mut budget = MAX_DIR_ENTRIES;
    let mut folders = vec![dir.to_path_buf()];
    for _ in 0..MAX_FOLDER_DEPTH {
        let mut below = Vec::new();
        for folder in &folders {
            let (programs, inside) = entries_of(folder, &mut budget);
            if let Some(program) = programs.into_iter().next() {
                return Some(program);
            }
            below.extend(inside);
        }
        if below.is_empty() || budget == 0 {
            break;
        }
        below.sort();
        folders = below;
    }
    None
}

/// The programs and the folders directly in `dir`, each sorted by name, taking no more than
/// `budget` entries and counting what it took off it. An unreadable folder gives neither.
fn entries_of(dir: &Path, budget: &mut usize) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let (mut programs, mut folders) = (Vec::new(), Vec::new());
    let Ok(entries) = std::fs::read_dir(dir) else { return (programs, folders) };
    for entry in entries.take(*budget).filter_map(Result::ok) {
        *budget = budget.saturating_sub(1);
        let path = entry.path();
        if is_program(&path) {
            programs.push(path);
        } else if path.is_dir() {
            folders.push(path);
        }
    }
    programs.sort();
    folders.sort();
    (programs, folders)
}

#[cfg(windows)]
fn is_program(path: &Path) -> bool {
    path.is_file() && path.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe"))
}

#[cfg(unix)]
fn is_program(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(any(windows, unix)))]
fn is_program(_: &Path) -> bool {
    false
}

/// Why there is no analysis.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnalysisError {
    /// The program could not be started (the system's reason).
    Start(String),
    /// It did not answer the UCI handshake in time.
    NotUci,
    /// It ended, or closed its output.
    Stopped,
}

/// One analysis line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    /// From 1, the engine's best line first.
    pub multipv: u32,
    pub depth: u32,
    /// For White.
    pub score: Score,
    pub bound: Bound,
    /// The moves in UCI notation, and as many of them as [`MAX_SAN`] in SAN.
    pub uci: Vec<String>,
    pub san: Vec<String>,
}

/// What the engine has found so far.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// The engine's name, once it said it.
    pub name: Option<String>,
    /// The handshake is done.
    pub ready: bool,
    /// The position analysed (as the engine was given it).
    pub fen: Option<String>,
    /// The lines found for it, best first.
    pub lines: Vec<Line>,
    /// The engine is still searching it.
    pub searching: bool,
    pub nodes: Option<u64>,
    pub nps: Option<u64>,
    pub error: Option<AnalysisError>,
    /// The settings not sent since the handshake, and why (plan A9-N8).
    pub refused: Vec<Refused>,
}

/// How the engine is set up beyond what the analysis asks (plan A9-N8, A8-11): each `None` leaves
/// the engine's default, and each is sent only when the engine announced the option.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EngineSettings {
    /// Search threads (`Threads`).
    pub threads: Option<u32>,
    /// Hash table size in MB (`Hash`).
    pub hash_mb: Option<u32>,
    /// The folders of Syzygy endgame tables, as the engine takes them (`SyzygyPath`: separated
    /// by `;` on Windows, `:` elsewhere). Picked by the user; never fetched.
    pub syzygy_path: Option<String>,
}

/// A setting the engine was not given.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refused {
    /// The option's name as this board asks for it (`Threads`, `Hash`, `SyzygyPath`, `MultiPV`).
    pub option: &'static str,
    pub why: Refusal,
}

/// Why a setting was not sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The engine did not announce the option (or announced more than [`MAX_OPTIONS`] before it).
    NotAnnounced,
    /// It announced it as another kind (a number where text is sent, or the other way round).
    OtherKind,
    /// The text has a line break or another control character, or is longer than
    /// [`MAX_OPTION_TEXT`]: it would reach the engine as more than one command.
    BadText,
}

/// What the analysis asks of the engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnalysisOptions {
    /// Lines, 1..=[`MAX_MULTIPV`].
    pub lines: u32,
    /// Search depth, 1..=[`MAX_DEPTH`].
    pub depth: u32,
}

impl Default for AnalysisOptions {
    fn default() -> AnalysisOptions {
        AnalysisOptions { lines: 3, depth: DEFAULT_DEPTH }
    }
}

impl AnalysisOptions {
    fn sanitized(self) -> AnalysisOptions {
        AnalysisOptions { lines: self.lines.clamp(1, MAX_MULTIPV), depth: self.depth.clamp(1, MAX_DEPTH) }
    }
}

/// Called after the engine said something new (the interface repaints).
pub type Notify = Arc<dyn Fn() + Send + Sync>;

/// What the engine is given to analyse: a position, as the rules write it, and how it was reached:
/// the moves from `start` that lead to it (none when only the position is known).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Asked {
    fen: String,
    start: String,
    moves: Vec<String>,
}

impl Asked {
    /// The position alone.
    fn position(fen: String) -> Asked {
        Asked { start: fen.clone(), fen, moves: Vec::new() }
    }

    /// The `position` command: `position fen <fen>`, or `position fen <start> moves …`.
    fn command(&self) -> String {
        if self.moves.is_empty() {
            format!("position fen {}", self.fen)
        } else {
            format!("position fen {} moves {}", self.start, self.moves.join(" "))
        }
    }
}

/// The lines found for a position the analyser left.
struct Kept {
    asked: Asked,
    lines: Vec<Line>,
    /// The search reached the depth asked for (it was not stopped).
    done: bool,
}

struct Shared {
    name: Option<String>,
    ready: bool,
    options: AnalysisOptions,
    settings: EngineSettings,
    /// The options the engine announced, at most [`MAX_OPTIONS`].
    announced: Vec<EngineOption>,
    refused: Vec<Refused>,
    /// The position being analysed (and how it was reached), and its side to move.
    asked: Option<Asked>,
    side: Color,
    /// A position asked for before the handshake finished.
    pending: Option<Asked>,
    /// The lines of the search running (or last run) on the position.
    lines: Vec<Line>,
    /// Lines kept from an earlier search of the position, shown instead of [`Shared::lines`]
    /// until the search's best line is deeper.
    seed: Vec<Line>,
    searching: bool,
    /// The search was told to stop, to keep what it found (not to start another).
    stopping: bool,
    /// The last search of the position reached the depth asked for.
    done: bool,
    /// The lines of positions left, the newest last, at most [`MAX_KEPT`].
    kept: VecDeque<Kept>,
    /// Searches stopped whose `bestmove` has not arrived yet: until it does, the engine's lines
    /// are about an earlier position.
    stale: u32,
    nodes: Option<u64>,
    nps: Option<u64>,
    error: Option<AnalysisError>,
    /// The process is being closed: its end is not an error.
    quitting: bool,
}

/// A running engine. Dropping it ends the process.
pub struct Analyser {
    shared: Arc<Mutex<Shared>>,
    commands: Sender<String>,
    child: Option<Child>,
    started: Instant,
}

fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Analyser {
    /// Start the engine at `path`. It runs from its own folder, where engines look for their
    /// network files.
    pub fn start(path: &Path, options: AnalysisOptions, notify: Notify) -> Result<Analyser, AnalysisError> {
        let mut command = Command::new(path);
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            command.current_dir(dir);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW: a console engine opens no console window.
            command.creation_flags(0x0800_0000);
        }
        let mut child = command.spawn().map_err(|e| AnalysisError::Start(e.to_string()))?;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AnalysisError::Start("no standard input or output".into()));
        };
        let mut analyser = Analyser::with_streams(stdout, stdin, options, notify);
        analyser.child = Some(child);
        Ok(analyser)
    }

    /// An engine already connected through `output` (what it prints) and `input` (what it reads).
    pub fn with_streams(
        output: impl Read + Send + 'static,
        input: impl Write + Send + 'static,
        options: AnalysisOptions,
        notify: Notify,
    ) -> Analyser {
        let shared = Arc::new(Mutex::new(Shared {
            name: None,
            ready: false,
            options: options.sanitized(),
            settings: EngineSettings::default(),
            announced: Vec::new(),
            refused: Vec::new(),
            asked: None,
            side: Color::White,
            pending: None,
            lines: Vec::new(),
            seed: Vec::new(),
            searching: false,
            stopping: false,
            done: false,
            kept: VecDeque::new(),
            stale: 0,
            nodes: None,
            nps: None,
            error: None,
            quitting: false,
        }));
        let (commands, received) = channel::<String>();
        let mut input = input;
        std::thread::spawn(move || {
            for line in received {
                if input.write_all(line.as_bytes()).and_then(|()| input.write_all(b"\n")).and_then(|()| input.flush()).is_err() {
                    break;
                }
            }
        });
        let reader_shared = Arc::clone(&shared);
        let reader_commands = commands.clone();
        std::thread::spawn(move || read_engine(output, &reader_shared, &reader_commands, &notify));
        let _ = commands.send("uci".to_owned());
        Analyser { shared, commands, child: None, started: Instant::now() }
    }

    /// Analyse `fen`; what was found for the position before is kept for when it comes back. A
    /// position searched before shows the lines found then at once, and is searched on unless
    /// that search reached the depth asked for. A position with no legal move (mate, stalemate)
    /// or that cannot be played is not sent: its lines are empty.
    pub fn analyse(&self, fen: &str) {
        // The engine is given the FEN as the rules wrote it back, never the text as received: a
        // line break in it would be a second command.
        let fen = Game::from_fen(fen).ok().filter(|g| !g.legal_moves().is_empty()).map(|g| g.fen());
        self.ask(fen.map(Asked::position));
    }

    /// [`Analyser::analyse`] the position `game` shows, with the moves since its last capture or
    /// pawn move (at most [`MAX_HISTORY`], [`Game::reversible_tail`]), so the engine sees a
    /// repetition or a perpetual check coming (plan A9-N8, R6-09). Another line to the same
    /// position is another position here: its lines are kept apart.
    pub fn analyse_game(&self, game: &Game) {
        if game.legal_moves().is_empty() {
            self.ask(None);
            return;
        }
        let (from, nodes) = game.reversible_tail(MAX_HISTORY);
        let moves: Option<Vec<String>> = nodes.iter().map(|n| game.uci(*n).filter(|m| uci::is_move(m))).collect();
        let start = game.fen_at(from).map(|f| f.to_string());
        let asked = match (moves, start) {
            (Some(moves), Some(start)) => Asked { fen: game.fen(), start, moves },
            // Not a line this game holds: the position alone.
            _ => Asked::position(game.fen()),
        };
        self.ask(Some(asked));
    }

    /// Analyse `asked`; `None`: there is nothing to analyse (no legal move, no position).
    fn ask(&self, asked: Option<Asked>) {
        let mut s = lock(&self.shared);
        let Some(asked) = asked else {
            stop_search(&mut s, &self.commands, true);
            remember(&mut s);
            s.asked = None;
            return;
        };
        // Already searching it, or searched to the depth asked for.
        if s.asked.as_ref() == Some(&asked) && ((s.searching && !s.stopping) || (!s.searching && s.done)) {
            return;
        }
        if s.ready {
            start_search(&mut s, &self.commands, asked);
        } else {
            s.pending = Some(asked);
        }
    }

    /// Set the engine up with `settings` (plan A9-N8), starting the position again when they
    /// changed: what was found before is not shown (it was found with the old ones). Each is sent
    /// only as the engine announced it ([`Snapshot::refused`] says what was not).
    pub fn set_settings(&self, settings: EngineSettings) {
        let mut s = lock(&self.shared);
        if s.settings == settings {
            return;
        }
        s.settings = settings;
        s.kept.clear();
        s.seed.clear();
        s.done = false;
        if !s.ready {
            return;
        }
        let again = s.asked.clone();
        // UCI allows setoption only between searches.
        stop_search(&mut s, &self.commands, true);
        s.lines.clear();
        configure(&mut s, &self.commands);
        if let Some(asked) = again {
            start_search(&mut s, &self.commands, asked);
        }
    }

    /// Stop searching; what was found stays.
    pub fn stop(&self) {
        let mut s = lock(&self.shared);
        s.pending = None;
        stop_search(&mut s, &self.commands, false);
    }

    /// Change the options, starting the position again when they changed. The lines kept are
    /// dropped: they were found with the old ones.
    pub fn set_options(&self, options: AnalysisOptions) {
        let mut s = lock(&self.shared);
        let options = options.sanitized();
        if s.options == options {
            return;
        }
        s.options = options;
        s.kept.clear();
        s.seed.clear();
        s.done = false;
        if !s.ready {
            return;
        }
        let again = s.asked.clone();
        // UCI allows setoption only between searches.
        stop_search(&mut s, &self.commands, true);
        s.lines.clear();
        send_lines(&mut s, &self.commands);
        if let Some(asked) = again {
            start_search(&mut s, &self.commands, asked);
        }
    }

    /// What the engine has found so far.
    pub fn snapshot(&self) -> Snapshot {
        let s = lock(&self.shared);
        let error = s.error.clone().or_else(|| (!s.ready && self.started.elapsed() > HANDSHAKE_TIMEOUT).then_some(AnalysisError::NotUci));
        Snapshot {
            name: s.name.clone(),
            ready: s.ready,
            fen: s.asked.as_ref().map(|a| a.fen.clone()),
            lines: shown(&s).to_vec(),
            searching: s.searching,
            nodes: s.nodes,
            nps: s.nps,
            error,
            refused: s.refused.clone(),
        }
    }
}

impl Drop for Analyser {
    fn drop(&mut self) {
        lock(&self.shared).quitting = true;
        let _ = self.commands.send("stop".to_owned());
        let _ = self.commands.send("quit".to_owned());
        // Killed in the background if it does not quit, so closing never waits on the engine.
        if let Some(mut child) = self.child.take() {
            std::thread::spawn(move || {
                let until = Instant::now() + QUIT_GRACE;
                while Instant::now() < until {
                    if child.try_wait().is_ok_and(|s| s.is_some()) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                let _ = child.kill();
                let _ = child.wait();
            });
        }
    }
}

/// Stop the search, if one runs. `drop_result`: the `bestmove` it ends with belongs to no
/// search the board waits for (a new one follows at once).
fn stop_search(s: &mut Shared, commands: &Sender<String>, drop_result: bool) {
    if s.searching {
        let _ = commands.send("stop".to_owned());
        if drop_result {
            s.stale = s.stale.saturating_add(1);
            s.searching = false;
        } else {
            s.stopping = true;
        }
    }
}

/// The lines shown: those kept from an earlier search while there are any, else the search's.
fn shown(s: &Shared) -> &[Line] {
    if s.seed.is_empty() { &s.lines } else { &s.seed }
}

/// Keep the lines shown for the position analysed, for when it comes back; the position has none
/// shown after.
fn remember(s: &mut Shared) {
    let lines = if s.seed.is_empty() { std::mem::take(&mut s.lines) } else { std::mem::take(&mut s.seed) };
    s.lines.clear();
    let Some(asked) = s.asked.clone() else { return };
    if lines.is_empty() {
        return;
    }
    s.kept.retain(|k| k.asked != asked);
    s.kept.push_back(Kept { asked, lines, done: s.done });
    while s.kept.len() > MAX_KEPT {
        s.kept.pop_front();
    }
}

/// Analyse `asked` (its positions written back by the rules): what was found for the position
/// before is kept, and what was kept for this one is shown while the engine searches on, or
/// instead of searching when that search reached the depth asked for.
fn start_search(s: &mut Shared, commands: &Sender<String>, asked: Asked) {
    stop_search(s, commands, true);
    remember(s);
    let kept = s.kept.iter().position(|k| k.asked == asked).and_then(|at| s.kept.remove(at));
    s.side = if asked.fen.split_ascii_whitespace().nth(1) == Some("b") { Color::Black } else { Color::White };
    s.pending = None;
    s.nodes = None;
    s.nps = None;
    s.stopping = false;
    match kept {
        Some(k) if k.done => {
            s.asked = Some(asked);
            s.lines = k.lines;
            s.done = true;
            s.searching = false;
        }
        kept => {
            let _ = commands.send(asked.command());
            let _ = commands.send(format!("go depth {}", s.options.depth));
            s.asked = Some(asked);
            s.seed = kept.map(|k| k.lines).unwrap_or_default();
            s.done = false;
            s.searching = true;
        }
    }
}

/// The option the engine announced as `name` (its spelling may differ in case: UCI's names are
/// not case sensitive).
fn announced<'a>(s: &'a Shared, name: &str) -> Option<&'a EngineOption> {
    s.announced.iter().find(|o| o.name.eq_ignore_ascii_case(name))
}

/// `setoption` for the number `value` of the option the engine announced as `name`, within its
/// bounds; or why not.
fn number_option(s: &Shared, name: &'static str, value: u32) -> Result<String, Refused> {
    let refused = |why| Refused { option: name, why };
    let option = announced(s, name).ok_or(refused(Refusal::NotAnnounced))?;
    match option.kind {
        // max(min).min(max), not clamp: bounds the wrong way round (an option not read from a handshake) never panic.
        OptionKind::Spin { min, max } => Ok(format!("setoption name {} value {}", option.name, i64::from(value).max(min).min(max))),
        _ => Err(refused(Refusal::OtherKind)),
    }
}

/// `setoption` for the text `value` of the option the engine announced as `name`; or why not. A
/// line break or another control character would end the command and start another (`\nquit`),
/// so such a text is never sent.
fn text_option(s: &Shared, name: &'static str, value: &str) -> Result<String, Refused> {
    let refused = |why| Refused { option: name, why };
    if value.len() > MAX_OPTION_TEXT || value.chars().any(char::is_control) {
        return Err(refused(Refusal::BadText));
    }
    let option = announced(s, name).ok_or(refused(Refusal::NotAnnounced))?;
    match option.kind {
        OptionKind::Text => Ok(format!("setoption name {} value {}", option.name, value)),
        _ => Err(refused(Refusal::OtherKind)),
    }
}

/// Tell the engine how many lines to search for ([`AnalysisOptions::lines`]), when it announced
/// `MultiPV`.
fn send_lines(s: &mut Shared, commands: &Sender<String>) {
    s.refused.retain(|r| r.option != "MultiPV");
    match number_option(s, "MultiPV", s.options.lines) {
        Ok(command) => {
            let _ = commands.send(command);
        }
        Err(refused) => s.refused.push(refused),
    }
}

/// Tell the engine its lines and [`EngineSettings`], each as it announced it; what was not sent
/// goes to [`Shared::refused`].
fn configure(s: &mut Shared, commands: &Sender<String>) {
    s.refused.clear();
    send_lines(s, commands);
    let wanted = [
        s.settings.threads.map(|n| number_option(s, "Threads", n)),
        s.settings.hash_mb.map(|n| number_option(s, "Hash", n)),
        s.settings.syzygy_path.as_deref().map(|p| text_option(s, "SyzygyPath", p)),
    ];
    for result in wanted.into_iter().flatten() {
        match result {
            Ok(command) => {
                let _ = commands.send(command);
            }
            Err(refused) => s.refused.push(refused),
        }
    }
}

/// The reader thread: the engine's lines into the shared state, until its output closes.
fn read_engine(output: impl Read, shared: &Mutex<Shared>, commands: &Sender<String>, notify: &Notify) {
    let mut reader = BufReader::new(output);
    let mut line = Vec::new();
    while let Ok(true) = read_bounded_line(&mut reader, &mut line) {
        let text = String::from_utf8_lossy(&line);
        let changed = take_message(&mut lock(shared), commands, uci::parse(&text));
        if changed {
            notify();
        }
    }
    let mut s = lock(shared);
    s.searching = false;
    if !s.quitting {
        s.error = Some(AnalysisError::Stopped);
    }
    drop(s);
    notify();
}

/// Read one line into `line` (without its end), keeping at most [`MAX_LINE_LEN`] bytes of it:
/// a longer line is read to its end and left empty. `Ok(false)` at the end of the output.
fn read_bounded_line(reader: &mut impl BufRead, line: &mut Vec<u8>) -> std::io::Result<bool> {
    line.clear();
    let mut too_long = false;
    let mut any = false;
    loop {
        let buf = reader.fill_buf()?;
        if buf.is_empty() {
            return Ok(any);
        }
        any = true;
        let (chunk, done) = match buf.iter().position(|b| *b == b'\n') {
            Some(i) => (buf.get(..i).unwrap_or_default(), Some(i.saturating_add(1))),
            None => (buf, None),
        };
        if !too_long {
            if line.len().saturating_add(chunk.len()) > MAX_LINE_LEN {
                too_long = true;
                line.clear();
            } else {
                line.extend_from_slice(chunk);
            }
        }
        let used = done.unwrap_or(buf.len());
        reader.consume(used);
        if done.is_some() {
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            return Ok(true);
        }
    }
}

/// Apply one engine message. Whether anything shown changed.
fn take_message(s: &mut Shared, commands: &Sender<String>, message: Message) -> bool {
    match message {
        Message::Name(name) => s.name = Some(name),
        Message::UciOk => {
            if s.ready {
                return false;
            }
            s.ready = true;
            configure(s, commands);
            if let Some(asked) = s.pending.take() {
                start_search(s, commands, asked);
            }
        }
        Message::Option(option) => {
            // Only the handshake announces options; one announced again keeps its first word.
            if !s.ready && s.announced.len() < MAX_OPTIONS && !s.announced.iter().any(|o| o.name.eq_ignore_ascii_case(&option.name)) {
                s.announced.push(option);
            }
            return false;
        }
        Message::Info(info) => {
            if s.stale > 0 || !s.searching {
                return false;
            }
            s.nodes = info.nodes.or(s.nodes);
            s.nps = info.nps.or(s.nps);
            let (Some(depth), Some(score), Some(fen)) = (info.depth, info.score, s.asked.as_ref().map(|a| a.fen.as_str())) else {
                return info.nodes.is_some();
            };
            if info.pv.is_empty() || info.multipv > s.options.lines {
                return info.nodes.is_some();
            }
            let san = uci::pv_san(fen, info.pv.get(..info.pv.len().min(MAX_SAN)).unwrap_or_default());
            // A line whose first move is not legal here belongs to another position.
            if san.is_empty() {
                return false;
            }
            let line = Line { multipv: info.multipv, depth, score: score.for_white(s.side), bound: info.bound, uci: info.pv, san };
            match s.lines.iter_mut().find(|l| l.multipv == line.multipv) {
                Some(old) => *old = line,
                None => s.lines.push(line),
            }
            s.lines.sort_by_key(|l| l.multipv);
            // The lines kept from before give way once the search's best line is deeper.
            let best = |lines: &[Line]| lines.iter().find(|l| l.multipv == 1).map_or(0, |l| l.depth);
            if !s.seed.is_empty() && best(&s.lines) > best(&s.seed) {
                s.seed.clear();
            }
        }
        Message::BestMove(_) => {
            if s.stale > 0 {
                s.stale -= 1;
                return false;
            }
            s.searching = false;
            s.done = !std::mem::take(&mut s.stopping);
        }
        Message::ReadyOk | Message::Other => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{PipeReader, PipeWriter};

    const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
    const AFTER_E4: &str = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1";
    const QUIET: &str = "4k3/8/8/8/8/8/4P3/4K3 w - - 0 1";

    /// A pretend engine on the other end of two pipes. For each `go depth N` it reports depths
    /// 1..=min(N, top) with a line legal in the position it was given, `top` being 3 the first
    /// time it searches the position and two more each time after; asked deeper, it then waits
    /// for `stop` and sends one more (late) line before its `bestmove`, as a real engine
    /// interrupted mid-search does. On a position without castling rights it reports nothing.
    /// It records every command.
    fn fake_engine(commands: PipeReader, out: PipeWriter, log: Arc<Mutex<Vec<String>>>) {
        fake_engine_with(commands, out, log, &["option name MultiPV type spin default 1 min 1 max 5"]);
    }

    /// [`fake_engine`], announcing `options` in its handshake.
    fn fake_engine_with(commands: PipeReader, mut out: PipeWriter, log: Arc<Mutex<Vec<String>>>, options: &[&str]) {
        let mut position = String::new();
        let mut waiting = false;
        let mut searched: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
        let mut top = 3;
        for line in BufReader::new(commands).lines() {
            let Ok(line) = line else { break };
            log.lock().unwrap().push(line.clone());
            let mv = if position.contains(" b ") { "e7e5" } else { "e2e4" };
            let mut say = |text: &str| {
                let _ = writeln!(out, "{text}");
            };
            match line.split_whitespace().next() {
                Some("uci") => {
                    say("id name Fake Engine 1");
                    for option in options {
                        say(option);
                    }
                    say("uciok");
                }
                Some("position") => position = line.clone(),
                Some("go") => {
                    let depth: u32 = line.split_whitespace().last().and_then(|d| d.parse().ok()).unwrap_or(1);
                    let silent = !position.contains("KQkq");
                    let times = searched.entry(position.clone()).or_insert(0);
                    top = 3 + 2 * *times;
                    *times += 1;
                    for d in (1..=depth.min(top)).filter(|_| !silent) {
                        say(&format!("info depth {d} multipv 1 score cp {} nodes {} nps 1000 pv {mv}", 10 * d, 100 * d));
                        say(&format!("info depth {d} multipv 2 score cp -5 pv {mv}"));
                    }
                    if depth <= top || silent {
                        say(&format!("bestmove {mv}"));
                    } else {
                        waiting = true;
                    }
                }
                Some("stop") if waiting => {
                    waiting = false;
                    say(&format!("info depth {} multipv 1 score cp 999 pv {mv}", top + 1));
                    say(&format!("bestmove {mv}"));
                }
                Some("quit") => break,
                _ => {}
            }
        }
    }

    fn connect(options: AnalysisOptions) -> (Analyser, Arc<Mutex<Vec<String>>>) {
        let (engine_in, our_out) = std::io::pipe().unwrap();
        let (our_in, engine_out) = std::io::pipe().unwrap();
        let log = Arc::new(Mutex::new(Vec::new()));
        let l = Arc::clone(&log);
        std::thread::spawn(move || fake_engine(engine_in, engine_out, l));
        (Analyser::with_streams(our_in, our_out, options, Arc::new(|| {})), log)
    }

    /// [`connect`] to an engine announcing `announced`, set up with `settings` before its handshake.
    fn connect_announcing(announced: &'static [&'static str], settings: EngineSettings) -> (Analyser, Arc<Mutex<Vec<String>>>) {
        let (engine_in, our_out) = std::io::pipe().unwrap();
        let (our_in, engine_out) = std::io::pipe().unwrap();
        let log = Arc::new(Mutex::new(Vec::new()));
        let l = Arc::clone(&log);
        std::thread::spawn(move || fake_engine_with(engine_in, engine_out, l, announced));
        let a = Analyser::with_streams(our_in, our_out, AnalysisOptions { lines: 2, depth: 2 }, Arc::new(|| {}));
        a.set_settings(settings);
        (a, log)
    }

    fn setoptions(log: &Mutex<Vec<String>>) -> Vec<String> {
        log.lock().unwrap().iter().filter(|c| c.starts_with("setoption")).cloned().collect()
    }

    /// Plan A9-N8 (A8-11): Threads, Hash and SyzygyPath go to the engine only when it announced
    /// them, under its own spelling and within its bounds; otherwise they are refused and said.
    #[test]
    fn settings_are_sent_only_as_the_engine_announced_them() {
        let settings = || EngineSettings { threads: Some(64), hash_mb: Some(0), syzygy_path: Some("C:/tables/syzygy".into()) };
        // An engine announcing none of them: none is sent (only the lines, which it announced).
        let (a, log) = connect_announcing(&["option name MultiPV type spin default 1 min 1 max 5"], settings());
        a.analyse(START);
        let s = wait(&a, |s| s.ready && !s.searching && !s.lines.is_empty());
        assert_eq!(setoptions(&log), ["setoption name MultiPV value 2"]);
        let refused: Vec<(&str, Refusal)> = s.refused.iter().map(|r| (r.option, r.why)).collect();
        assert_eq!(refused, [("Threads", Refusal::NotAnnounced), ("Hash", Refusal::NotAnnounced), ("SyzygyPath", Refusal::NotAnnounced)]);
        // An engine announcing them: its spelling, its bounds.
        const ALL: &[&str] = &[
            "option name threads type spin default 1 min 1 max 8",
            "option name Hash type spin default 16 min 1 max 1024",
            "option name SyzygyPath type string default <empty>",
        ];
        let (a, log) = connect_announcing(ALL, settings());
        a.analyse(START);
        let s = wait(&a, |s| s.ready && !s.searching && !s.lines.is_empty());
        assert_eq!(
            setoptions(&log),
            ["setoption name threads value 8", "setoption name Hash value 1", "setoption name SyzygyPath value C:/tables/syzygy"]
        );
        assert_eq!(s.refused, [Refused { option: "MultiPV", why: Refusal::NotAnnounced }], "MultiPV was not announced");
        // Changed settings start the position again, after telling the engine.
        a.set_settings(EngineSettings { threads: Some(2), ..settings() });
        let s = wait(&a, |s| !s.searching && !s.lines.is_empty() && searches(&log) == 2);
        assert!(log.lock().unwrap().iter().any(|c| c == "setoption name threads value 2"), "{:?}", log.lock().unwrap());
        assert_eq!(s.fen.as_deref(), Some(START));
    }

    /// Plan A9-N8: a text with a line break would reach the engine as a second command
    /// (`SyzygyPath` `C:/tb` then `quit`): it is refused, and the engine goes on.
    #[test]
    fn a_setting_with_a_line_break_is_refused() {
        const ALL: &[&str] = &["option name SyzygyPath type string default <empty>", "option name Threads type spin default 1 min 1 max 8"];
        for path in ["C:/tb\nquit", "C:/tb\rquit", "C:/tb\u{0}", "C:/tb\u{1b}[2J"] {
            let (a, log) = connect_announcing(ALL, EngineSettings { syzygy_path: Some(path.into()), threads: Some(3), hash_mb: None });
            a.analyse(START);
            let s = wait(&a, |s| s.ready && !s.searching && !s.lines.is_empty());
            assert!(!s.lines.is_empty(), "the engine goes on: {path:?}");
            let log = log.lock().unwrap().clone();
            assert!(!log.iter().any(|c| c == "quit" || c.contains("SyzygyPath")), "{path:?}: {log:?}");
            assert!(log.iter().any(|c| c == "setoption name Threads value 3"), "the others are sent: {log:?}");
            assert!(s.refused.contains(&Refused { option: "SyzygyPath", why: Refusal::BadText }), "{:?}", s.refused);
        }
        // A number option announced as text, and the other way round: refused, not sent.
        const SWAPPED: &[&str] = &["option name SyzygyPath type spin default 0 min 0 max 9", "option name Threads type string default 1"];
        let (a, log) = connect_announcing(SWAPPED, EngineSettings { syzygy_path: Some("C:/tb".into()), threads: Some(3), hash_mb: None });
        a.analyse(START);
        let s = wait(&a, |s| s.ready && !s.searching && !s.lines.is_empty());
        assert!(!log.lock().unwrap().iter().any(|c| c.starts_with("setoption name SyzygyPath") || c.starts_with("setoption name Threads")));
        assert!(s.refused.contains(&Refused { option: "SyzygyPath", why: Refusal::OtherKind }), "{:?}", s.refused);
        assert!(s.refused.contains(&Refused { option: "Threads", why: Refusal::OtherKind }), "{:?}", s.refused);
    }

    /// Plan A9-N8 (R6-09): a game's position goes with the moves since its last capture or pawn
    /// move, so the engine sees a repetition; reached another way, it is another position.
    #[test]
    fn a_games_position_goes_with_its_moves_since_the_last_pawn_move() {
        let (a, log) = connect(AnalysisOptions { lines: 1, depth: 2 });
        let mut game = Game::standard();
        for mv in ["e2e4", "e7e5", "g1f3", "b8c6", "f3g1", "c6b8"] {
            game.play_uci(mv).unwrap();
        }
        a.analyse_game(&game);
        let after_e5 = "rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq e6 0 2";
        let s = wait(&a, |s| s.ready && !s.searching && searches(&log) == 1);
        assert_eq!(s.fen.as_deref(), Some(game.fen().as_str()), "the position shown is the game's");
        let positions: Vec<String> = log.lock().unwrap().iter().filter(|c| c.starts_with("position")).cloned().collect();
        assert_eq!(positions, [format!("position fen {after_e5} moves g1f3 b8c6 f3g1 c6b8")]);
        // The same position given alone is searched as another one.
        a.analyse(&game.fen());
        wait(&a, |s| !s.searching && searches(&log) == 2);
        assert_eq!(log.lock().unwrap().iter().rfind(|c| c.starts_with("position")), Some(&format!("position fen {}", game.fen())));
        // A pawn move: the position alone again.
        game.play_uci("d2d4").unwrap();
        a.analyse_game(&game);
        wait(&a, |s| !s.searching && searches(&log) == 3);
        assert_eq!(log.lock().unwrap().iter().rfind(|c| c.starts_with("position")), Some(&format!("position fen {}", game.fen())));
    }

    /// Wait (up to five seconds) until `done` holds for the snapshot.
    fn wait(a: &Analyser, done: impl Fn(&Snapshot) -> bool) -> Snapshot {
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            let s = a.snapshot();
            if done(&s) || Instant::now() > until {
                return s;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn analyses_a_position_after_the_handshake() {
        let (a, log) = connect(AnalysisOptions { lines: 2, depth: 3 });
        // Asked before the handshake finished: sent once it has.
        a.analyse(START);
        let s = wait(&a, |s| s.ready && !s.searching && s.lines.len() == 2);
        assert_eq!(s.name.as_deref(), Some("Fake Engine 1"));
        assert_eq!(s.fen.as_deref(), Some(START));
        let best = &s.lines[0];
        assert_eq!((best.multipv, best.depth, best.score), (1, 3, Score::Centipawns(30)));
        assert_eq!(best.san, ["e4"]);
        assert_eq!(s.lines[1].score, Score::Centipawns(-5));
        assert_eq!(s.nodes, Some(300));
        let log = log.lock().unwrap().clone();
        assert_eq!(log, ["uci", "setoption name MultiPV value 2", &format!("position fen {START}"), "go depth 3"]);
    }

    #[test]
    fn scores_are_for_white() {
        let (a, _log) = connect(AnalysisOptions { lines: 1, depth: 2 });
        a.analyse(AFTER_E4);
        let s = wait(&a, |s| !s.searching && !s.lines.is_empty());
        let line = &s.lines[0];
        assert_eq!(line.san, ["e5"]);
        // The engine said +20 for Black, the side to move: that is -0.20 for White.
        assert_eq!(line.score, Score::Centipawns(-20));
    }

    #[test]
    fn late_lines_about_the_position_before_are_dropped() {
        let (a, log) = connect(AnalysisOptions { lines: 1, depth: 20 });
        a.analyse(START);
        wait(&a, |s| s.lines.first().is_some_and(|l| l.depth == 3));
        // A new position while the engine is still searching the first. The engine says nothing
        // about it (the fake stays silent on a position without castling rights), so any line
        // shown would be the one it sends after `stop`, about the start position: e2e4 is legal
        // in both, so only knowing that the stopped search had not ended tells them apart.
        a.analyse(QUIET);
        let s = wait(&a, |s| s.fen.as_deref() == Some(QUIET) && !s.searching);
        assert!(!s.searching);
        assert_eq!(s.lines, [], "a line of the stopped search was shown for the new position");
        // New options start the position again, after telling the engine.
        a.set_options(AnalysisOptions { lines: 2, depth: 2 });
        a.analyse(START);
        let s = wait(&a, |s| s.fen.as_deref() == Some(START) && !s.searching && s.lines.len() == 2);
        assert_eq!(s.lines.len(), 2);
        assert!(log.lock().unwrap().iter().any(|c| c == "setoption name MultiPV value 2"));
    }

    /// The number of searches started.
    fn searches(log: &Mutex<Vec<String>>) -> usize {
        log.lock().unwrap().iter().filter(|c| c.starts_with("go ")).count()
    }

    /// Back on a position it had searched, the analyser shows the lines it had at once and
    /// searches on, showing them until the new search goes deeper; so too on the position it was
    /// stopped on (the board hidden, then shown again).
    #[test]
    fn a_position_searched_before_shows_its_lines_at_once_and_is_searched_deeper() {
        let (a, log) = connect(AnalysisOptions { lines: 1, depth: 20 });
        a.analyse(START);
        wait(&a, |s| s.lines.first().is_some_and(|l| l.depth == 3));
        a.analyse(AFTER_E4);
        let s = wait(&a, |s| s.fen.as_deref() == Some(AFTER_E4) && s.lines.first().is_some_and(|l| l.depth == 3));
        assert_eq!(s.lines[0].san, ["e5"]);
        a.analyse(START);
        let s = a.snapshot();
        assert_eq!(s.fen.as_deref(), Some(START));
        let first = s.lines.first().expect("the lines it had, at once");
        assert!(first.depth >= 3 && first.san == ["e4"], "{first:?}");
        assert!(s.searching, "and it searches on");
        let s = wait(&a, |s| s.lines.first().is_some_and(|l| l.depth == 5));
        assert_eq!((s.lines[0].depth, searches(&log)), (5, 3), "the new search's lines once deeper");
        // Stopped (the board hidden), then asked again: searched on from the lines it had.
        a.stop();
        let s = wait(&a, |s| !s.searching);
        assert_eq!(s.lines[0].depth, 6, "the line the engine sent as it stopped");
        a.analyse(START);
        let s = a.snapshot();
        assert_eq!(s.lines.first().map(|l| l.depth), Some(6), "shown at once");
        let s = wait(&a, |s| s.lines.first().is_some_and(|l| l.depth == 7));
        assert_eq!((s.lines[0].depth, searches(&log)), (7, 4));
    }

    /// A position searched to the depth asked for is not searched again: its lines come back.
    #[test]
    fn a_position_searched_to_the_depth_asked_is_not_searched_again() {
        let (a, log) = connect(AnalysisOptions { lines: 2, depth: 3 });
        a.analyse(START);
        wait(&a, |s| !s.searching && s.lines.len() == 2);
        a.analyse(AFTER_E4);
        wait(&a, |s| s.fen.as_deref() == Some(AFTER_E4) && !s.searching && s.lines.len() == 2);
        a.analyse(START);
        let s = a.snapshot();
        assert_eq!((s.fen.as_deref(), s.searching, s.lines.len()), (Some(START), false, 2));
        assert_eq!(s.lines[0].depth, 3);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(searches(&log), 2, "{:?}", log.lock().unwrap());
        // Other options: the position shown is searched again, and what was found with the old
        // ones is not shown.
        a.set_options(AnalysisOptions { lines: 1, depth: 3 });
        let s = wait(&a, |s| !s.searching && s.lines.len() == 1);
        assert_eq!((s.fen.as_deref(), searches(&log)), (Some(START), 3));
        a.analyse(AFTER_E4);
        let s = wait(&a, |s| s.fen.as_deref() == Some(AFTER_E4) && !s.searching && !s.lines.is_empty());
        assert_eq!(s.lines.len(), 1);
        assert_eq!(searches(&log), 4);
    }

    #[test]
    fn positions_without_moves_and_bad_text_are_not_sent() {
        let (a, log) = connect(AnalysisOptions { lines: 1, depth: 2 });
        wait(&a, |s| s.ready);
        // Mate, and a FEN that cannot be read: nothing to send.
        a.analyse("7k/6Q1/6K1/8/8/8/8/8 b - - 0 1");
        a.analyse("not a fen");
        std::thread::sleep(Duration::from_millis(50));
        assert!(log.lock().unwrap().iter().all(|c| !c.starts_with("position") && !c.starts_with("go")));
        assert!(a.snapshot().lines.is_empty());
        // A line break smuggling a command: the engine gets the FEN as the rules write it.
        a.analyse(&format!("{START}\nquit"));
        wait(&a, |s| !s.lines.is_empty());
        let log = log.lock().unwrap().clone();
        assert!(log.iter().filter(|c| c.starts_with("position")).all(|c| *c == format!("position fen {START}")), "{log:?}");
        assert!(!log.iter().any(|c| c == "quit"), "{log:?}");
    }

    #[test]
    fn an_engine_that_never_answers_or_ends_is_reported() {
        let (our_in, engine_out) = std::io::pipe().unwrap();
        let (_engine_in, our_out) = std::io::pipe().unwrap();
        let mut a = Analyser::with_streams(our_in, our_out, AnalysisOptions::default(), Arc::new(|| {}));
        assert_eq!(a.snapshot().error, None, "it has a few seconds to answer");
        a.started = Instant::now().checked_sub(HANDSHAKE_TIMEOUT * 2).unwrap();
        assert_eq!(a.snapshot().error, Some(AnalysisError::NotUci));
        drop(engine_out);
        let s = wait(&a, |s| s.error == Some(AnalysisError::Stopped));
        assert_eq!(s.error, Some(AnalysisError::Stopped));
    }

    #[test]
    fn long_lines_are_read_to_their_end_and_dropped() {
        let text = format!("{}\nuciok\r\nlast", "x".repeat(MAX_LINE_LEN + 10));
        let mut reader = BufReader::with_capacity(64, text.as_bytes());
        let mut line = Vec::new();
        assert!(read_bounded_line(&mut reader, &mut line).unwrap());
        assert!(line.is_empty());
        assert!(read_bounded_line(&mut reader, &mut line).unwrap());
        assert_eq!(line, b"uciok");
        assert!(read_bounded_line(&mut reader, &mut line).unwrap());
        assert_eq!(line, b"last");
        assert!(!read_bounded_line(&mut reader, &mut line).unwrap());
    }

    #[test]
    fn the_program_is_found_in_its_folder() {
        let dir = std::env::temp_dir().join(format!("pdfcraft-engine-dir-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(program_in(&dir), None);
        std::fs::write(dir.join("readme.txt"), "x").unwrap();
        let name = if cfg!(windows) { "stockfish.exe" } else { "stockfish" };
        std::fs::write(dir.join(name), "x").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir.join(name), std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        assert_eq!(program_in(&dir), Some(dir.join(name)));
        assert_eq!(program_in(&dir.join("missing")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A program written at `path`, runnable where the system asks for it.
    fn write_program(path: &Path) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, "x").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    /// An engine left in the folder it was unpacked into is found as well (the app's own `engine`
    /// folder keeps Stockfish in `engine/stockfish/`), down to [`MAX_FOLDER_DEPTH`] and no
    /// further. A program directly in the folder still wins.
    #[test]
    fn a_program_in_a_folder_inside_the_engine_folder_is_found() {
        let dir = std::env::temp_dir().join(format!("pdfcraft-engine-deep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let name = if cfg!(windows) { "stockfish.exe" } else { "stockfish" };
        // Too deep: the folder itself, its folders and theirs are looked at, no deeper.
        let too_deep = dir.join("a").join("b").join("c").join(name);
        write_program(&too_deep);
        assert_eq!(program_in(&dir), None, "four folders down is too deep");
        // One folder down, as an unpacked engine sits.
        let unpacked = dir.join("stockfish").join(name);
        write_program(&unpacked);
        std::fs::create_dir_all(dir.join("stockfish").join("scripts")).unwrap();
        assert_eq!(program_in(&dir), Some(unpacked));
        // A program directly in the folder wins over one inside a folder.
        let beside = dir.join(name);
        write_program(&beside);
        assert_eq!(program_in(&dir), Some(beside));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_program_is_a_start_error() {
        let r = Analyser::start(Path::new("this-engine-does-not-exist-anywhere"), AnalysisOptions::default(), Arc::new(|| {}));
        assert!(matches!(r, Err(AnalysisError::Start(_))));
    }
}
