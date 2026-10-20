//! Chess ▸ Study board ▸ Analysis: a UCI chess engine the user installed (Stockfish or any
//! other), run as a child process, analysing the position the board shows.
//!
//! None ships with PdfCraft and none is fetched. The engine is `$PDFCRAFT_CHESS_ENGINE`, the
//! program in an `engine` folder beside the executable, or one the user picks ([`find_engine`]).
//! It is a separate process speaking UCI over its standard input and output, so its licence does
//! not reach PdfCraft. Desktop only: the web build cannot start processes.
//!
//! The engine is untrusted, like a file. Its lines are read bounded ([`pdfcraft_chess::uci`]), a
//! line about a position it is no longer analysing is dropped, an engine that never finishes the
//! handshake is reported after [`HANDSHAKE_TIMEOUT`], and dropping the [`Analyser`] ends the
//! process. Commands go through a writer thread, so an engine that stops reading cannot hold up
//! the interface.
//!
//! Only the interface starts an engine. No automation tool does: a tool that runs a program
//! named in its arguments would let any MCP client run any program.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use pdfcraft_chess::game::Game;
use pdfcraft_chess::placement::Color;
use pdfcraft_chess::uci::{self, Bound, MAX_LINE_LEN, MAX_MULTIPV, Message, Score};

/// How long an engine has to answer `uci` with `uciok`.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

/// How deep a search goes before the engine stops by itself, so an open board does not keep a
/// processor busy for ever. Deeper than any analysis board shows by default.
pub const DEFAULT_DEPTH: u32 = 30;

/// The deepest search asked for.
pub const MAX_DEPTH: u32 = 99;

/// Most entries of the `engine` folder looked at.
const MAX_DIR_ENTRIES: usize = 256;

/// How long a closed engine has to quit before it is killed.
const QUIT_GRACE: Duration = Duration::from_millis(400);

/// Moves of a line turned into SAN (the rest stay in UCI).
const MAX_SAN: usize = 24;

/// The engine program to use: `$PDFCRAFT_CHESS_ENGINE` when it names a file, else the program
/// in an `engine` folder beside the executable.
pub fn find_engine() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("PDFCRAFT_CHESS_ENGINE").map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(path);
    }
    let exe = std::env::current_exe().ok()?;
    program_in(&exe.parent()?.join("engine"))
}

/// The first program in `dir`, by name: an `.exe` on Windows, a file anyone may execute
/// elsewhere.
pub fn program_in(dir: &Path) -> Option<PathBuf> {
    let mut programs: Vec<PathBuf> =
        std::fs::read_dir(dir).ok()?.take(MAX_DIR_ENTRIES).filter_map(Result::ok).map(|e| e.path()).filter(|p| is_program(p)).collect();
    programs.sort();
    programs.into_iter().next()
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

struct Shared {
    name: Option<String>,
    ready: bool,
    options: AnalysisOptions,
    /// The position being analysed and its side to move.
    fen: Option<String>,
    side: Color,
    /// A position asked for before the handshake finished.
    pending: Option<String>,
    lines: Vec<Line>,
    searching: bool,
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
            fen: None,
            side: Color::White,
            pending: None,
            lines: Vec::new(),
            searching: false,
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

    /// Analyse `fen`, dropping what was found for the position before. A position with no legal
    /// move (mate, stalemate) or that cannot be played is not sent: its lines are empty.
    pub fn analyse(&self, fen: &str) {
        let mut s = lock(&self.shared);
        // The engine is given the FEN as the rules wrote it back, never the text as received: a
        // line break in it would be a second command.
        let fen = Game::from_fen(fen).ok().filter(|g| !g.legal_moves().is_empty()).map(|g| g.fen());
        let Some(fen) = fen else {
            stop_search(&mut s, &self.commands, true);
            s.fen = None;
            s.lines.clear();
            return;
        };
        if s.fen.as_deref() == Some(fen.as_str()) && (s.searching || !s.lines.is_empty()) {
            return;
        }
        if s.ready {
            start_search(&mut s, &self.commands, fen);
        } else {
            s.pending = Some(fen);
        }
    }

    /// Stop searching; what was found stays.
    pub fn stop(&self) {
        let mut s = lock(&self.shared);
        s.pending = None;
        stop_search(&mut s, &self.commands, false);
    }

    /// Change the options, starting the position again when they changed.
    pub fn set_options(&self, options: AnalysisOptions) {
        let mut s = lock(&self.shared);
        let options = options.sanitized();
        if s.options == options {
            return;
        }
        s.options = options;
        if !s.ready {
            return;
        }
        let again = s.fen.clone().filter(|_| s.searching);
        // UCI allows setoption only between searches.
        stop_search(&mut s, &self.commands, true);
        let _ = self.commands.send(format!("setoption name MultiPV value {}", options.lines));
        if let Some(fen) = again {
            start_search(&mut s, &self.commands, fen);
        }
    }

    /// What the engine has found so far.
    pub fn snapshot(&self) -> Snapshot {
        let s = lock(&self.shared);
        let error = s.error.clone().or_else(|| (!s.ready && self.started.elapsed() > HANDSHAKE_TIMEOUT).then_some(AnalysisError::NotUci));
        Snapshot {
            name: s.name.clone(),
            ready: s.ready,
            fen: s.fen.clone(),
            lines: s.lines.clone(),
            searching: s.searching,
            nodes: s.nodes,
            nps: s.nps,
            error,
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
        }
    }
}

fn start_search(s: &mut Shared, commands: &Sender<String>, fen: String) {
    stop_search(s, commands, true);
    let _ = commands.send(format!("position fen {fen}"));
    let _ = commands.send(format!("go depth {}", s.options.depth));
    s.side = if fen.split_ascii_whitespace().nth(1) == Some("b") { Color::Black } else { Color::White };
    s.fen = Some(fen);
    s.pending = None;
    s.lines.clear();
    s.nodes = None;
    s.nps = None;
    s.searching = true;
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
            let _ = commands.send(format!("setoption name MultiPV value {}", s.options.lines));
            if let Some(fen) = s.pending.take() {
                start_search(s, commands, fen);
            }
        }
        Message::Info(info) => {
            if s.stale > 0 || !s.searching {
                return false;
            }
            s.nodes = info.nodes.or(s.nodes);
            s.nps = info.nps.or(s.nps);
            let (Some(depth), Some(score), Some(fen)) = (info.depth, info.score, s.fen.as_deref()) else { return info.nodes.is_some() };
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
        }
        Message::BestMove(_) => {
            if s.stale > 0 {
                s.stale -= 1;
                return false;
            }
            s.searching = false;
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
    /// 1..=min(N, 3) with a line legal in the position it was given; asked deeper, it then waits
    /// for `stop` and sends one more (late) line before its `bestmove`, as a real engine
    /// interrupted mid-search does. On a position without castling rights it reports nothing.
    /// It records every command.
    fn fake_engine(commands: PipeReader, mut out: PipeWriter, log: Arc<Mutex<Vec<String>>>) {
        let mut position = String::new();
        let mut waiting = false;
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
                    say("option name MultiPV type spin default 1 min 1 max 5");
                    say("uciok");
                }
                Some("position") => position = line.clone(),
                Some("go") => {
                    let depth: u32 = line.split_whitespace().last().and_then(|d| d.parse().ok()).unwrap_or(1);
                    let silent = !position.contains("KQkq");
                    for d in (1..=depth.min(3)).filter(|_| !silent) {
                        say(&format!("info depth {d} multipv 1 score cp {} nodes {} nps 1000 pv {mv}", 10 * d, 100 * d));
                        say(&format!("info depth {d} multipv 2 score cp -5 pv {mv}"));
                    }
                    if depth <= 3 || silent {
                        say(&format!("bestmove {mv}"));
                    } else {
                        waiting = true;
                    }
                }
                Some("stop") if waiting => {
                    waiting = false;
                    say(&format!("info depth 4 multipv 1 score cp 999 pv {mv}"));
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

    #[test]
    fn a_missing_program_is_a_start_error() {
        let r = Analyser::start(Path::new("this-engine-does-not-exist-anywhere"), AnalysisOptions::default(), Arc::new(|| {}));
        assert!(matches!(r, Err(AnalysisError::Start(_))));
    }
}
