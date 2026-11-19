//! Chess ▸ Library… (plan A9-N15c, finding A8-08; decisions D22 and D31): the books of the folders
//! a person chooses, read once into the Library, so a position, a move, a player, an event or an
//! opening is found in every one of them without reading a page. The engine's half is
//! `pdfcraft_engine::chess::library` (plans A9-N15a, N15a2 and N15b).
//!
//! - **Where.** A dialog (`widgets::show_titled_modal`), not the chess column: the Library is not
//!   about the document open, works with none open and opens other PDFs; and its folders, its
//!   books and its search need more room than the column has. Two views, **Books** and
//!   **Search** (`widgets::segmented`); the content scrolls inside the dialog, which fits the
//!   window at any interface size.
//! - **The folders** are chosen with the app's folder picker and left out with Remove, in the
//!   Library's own catalog (`chess/library/catalog.json`, `Library::add_folder` and
//!   `Library::remove_folder`): leaving a folder takes its books out of the Library, never a file
//!   of the folder. Each book says where it stands, in words: read (its diagrams, those to
//!   review), not read yet, stopped partway, skipped and why, in the cloud. An encrypted book
//!   (`ask`, D31 A) has its own **Yes**, kept in the catalog: every build reads it after that.
//! - **Only when asked** (D31 B): nothing runs at the app's start or watches the folders. Opening
//!   the dialog reads the catalog and counts the pages a build would read
//!   (`Library::plan`: a book the catalog does not know is opened to count its pages, nothing is
//!   read of it), so the estimate is said before anything starts: the books, their pages and about
//!   how long, at the time a page took when 38 books were measured (plan A9-N15a: 16,944 pages in
//!   3,665 s on four cores), on the cores the scan uses here.
//! - **Build, Stop and Continue.** The build is a job of its own thread ([`Job`], the one kind of
//!   plan A9-I2), never the interface's: each book is opened from its file in a session of the
//!   engine's own, never the open documents'. It says the book and the page it is on; Stop ends it
//!   between two pages, and Continue goes on from there (the engine's catalog and the pages kept on
//!   disk). Closing the dialog leaves it running: its row, with Stop and Open the Library, stays at
//!   the foot of the page area ([`building_card`]). Quitting the app stops it between two pages
//!   ([`PdfCraftApp::quit_library`]); the book it was in is then said as stopped partway.
//! - **The search** (`Library::find`, in a job): a position (a FEN, or the board's own: **From the
//!   board**, and **Look in the Library** in the Board tab's In the book list), a move (its SAN),
//!   a player, an event, an opening (ECO). The list follows the In the book list's rules (plans
//!   A9-N3 and N3b): rows reached by ↑ ↓ Home End, the last answer kept dimmed while the next is
//!   looked for, a click on it doing nothing, and a newer search taking the place of one running.
//! - **A result opens** its PDF (the tab showing it, or the file opened), at its page, the move
//!   outlined as the Text tab's Show in PDF does (the page read first, as a move clicked on the
//!   page is, plan A9-N1) or the diagram opened as the In the book list opens one. A PDF that is
//!   gone, or whose data is in the cloud, is said and never opened (`library::on_this_computer`).
//! - **Desktop only**: the web build has none of this, and its menus leave the item out.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use egui::{RichText, vec2};
use pdfcraft_engine::chess::DiagramAt;
use pdfcraft_engine::chess::library::find::{BookHits, FindQuery, Found, Hit, MAX_TEXT_CHARS};
use pdfcraft_engine::chess::library::index::{IndexedGame, Place};
use pdfcraft_engine::chess::library::plan::Plan;
use pdfcraft_engine::chess::library::walk::Caps;
use pdfcraft_engine::chess::library::{self, BuildError, BuildOptions, Built, Catalog, CatalogBook, ChangeError, Library, Removed, Skip, State};
use pdfcraft_engine::chess::pdfcraft_chess::fen::Fen;
use pdfcraft_engine::chess::pdfcraft_chessvision::doc::MoveAt;

use crate::PdfCraftApp;
use crate::chess_job::Job;
use crate::theme::{self, Tokens};

/// Seconds a page took to read into the Library on [`MEASURED_CORES`] cores (plan A9-N15a: the 38
/// books of the gate's stage 5, 16,944 pages in 3,665 s).
const SECONDS_PER_PAGE: f64 = 3665.0 / 16944.0;
const MEASURED_CORES: f64 = 4.0;

/// Most hits a search lists; all are counted.
pub(crate) const MAX_LISTED: usize = 200;
/// Most keys taken in one pass for the list's rows.
const MAX_KEYS: usize = 16;
/// The height of a book's row in the Books view.
const BOOK_ROW: f32 = 44.0;
/// How long a result waits for its PDF to open, at most.
const OPEN_WAIT: Duration = Duration::from_secs(120);
/// How long a result's move waits for its page to be read, once its page is shown, at most.
const OUTLINE_WAIT: Duration = Duration::from_secs(30);
/// How often the catalog is read again while another build or change holds the Library.
const OTHER_LOOK: Duration = Duration::from_secs(2);

/// The dialog's two views.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum View {
    #[default]
    Books,
    Search,
}

/// What the search's fields hold.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Fields {
    pub(crate) fen: String,
    pub(crate) any_side: bool,
    pub(crate) san: String,
    pub(crate) player: String,
    pub(crate) event: String,
    pub(crate) eco: String,
}

/// A field's text as looked for: trimmed, `None` when empty.
fn text_of(field: &str) -> Option<String> {
    let t = field.trim();
    (!t.is_empty()).then(|| t.chars().take(MAX_TEXT_CHARS).collect())
}

impl Fields {
    /// The query the fields make: `Ok(None)` while they ask for nothing, `Err` with why for a FEN
    /// that is not one.
    pub(crate) fn query(&self) -> Result<Option<FindQuery>, String> {
        let position = match text_of(&self.fen) {
            None => None,
            Some(fen) => Some(Fen::parse(&fen).map_err(|e| e.to_string())?.book_key()),
        };
        let query = FindQuery {
            position,
            any_side: self.any_side,
            san: text_of(&self.san),
            player: text_of(&self.player),
            event: text_of(&self.event),
            eco: text_of(&self.eco),
            ..FindQuery::default()
        };
        Ok((!query.is_empty()).then_some(query))
    }
}

/// What a search asks, of which state of the Library: every change of its catalog and every
/// build's end moves the generation, so an answer from before is the last one, dimmed and inert
/// (plan A9-N15c, the review of #431: a book removed is never clickable).
type Asked = (FindQuery, u64);

/// The search: what it was last asked for, its answer and its job (the rules of plan A9-N3b).
#[derive(Default)]
struct Search {
    now: Option<Asked>,
    answer: Option<(Asked, Result<Arc<Shown>, String>)>,
    asking: Option<(Asked, Job<Found>)>,
    generation: u64,
}

/// An answer, with what its rows say, made once when it came (not at each pass).
#[derive(Debug)]
struct Shown {
    found: Found,
    books: Vec<ShownBook>,
}

/// A book of an answer: its line, the paths in its tooltip, and its rows (the result, its title,
/// the same in words and what follows it).
#[derive(Debug)]
struct ShownBook {
    head: String,
    paths: String,
    rows: Vec<(Item, String, String, String)>,
}

/// Where the search is.
#[derive(Clone, Debug)]
enum Status {
    /// Nothing asked yet.
    Idle,
    /// Being looked for, with the last answer to show meanwhile, dimmed.
    Searching(Option<Arc<Shown>>),
    Found(Arc<Shown>),
    Failed(String),
}

impl Search {
    fn status(&self) -> Status {
        let Some(now) = &self.now else { return Status::Idle };
        match &self.answer {
            Some((q, Ok(f))) if q == now => Status::Found(Arc::clone(f)),
            Some((q, Err(e))) if q == now => Status::Failed(e.clone()),
            Some((_, Ok(f))) => Status::Searching(Some(Arc::clone(f))),
            _ => Status::Searching(None),
        }
    }

    /// Ask for `query` of the Library as it is now: one running for something else is dropped
    /// (which asks it to stop) and `spawn`'s takes its place; one answered or running is not asked
    /// again. Whether a search started.
    fn ask(&mut self, query: FindQuery, spawn: impl FnOnce(FindQuery) -> Job<Found>) -> bool {
        let asked = (query, self.generation);
        self.now = Some(asked.clone());
        let answered = self.answer.as_ref().is_some_and(|(q, _)| *q == asked);
        let running = self.asking.as_ref().is_some_and(|(q, _)| *q == asked);
        if running || answered {
            return false;
        }
        self.asking = Some((asked.clone(), spawn(asked.0)));
        true
    }
}

/// Where a build is, as its job says it.
#[derive(Clone, Debug, Default)]
struct Seen {
    /// The book (from 0) of how many the walk found, its file's name, and its page of how many
    /// (0 of 0 before its scan).
    book: usize,
    books: usize,
    name: String,
    page: usize,
    pages: usize,
    /// The job has said something.
    started: bool,
}

/// What a build is told of each page, for tests: the book, the page and the pages (a test holds
/// the build there).
pub type BuildHook = Arc<dyn Fn(usize, usize, usize) + Send + Sync>;

/// The build running.
struct Building {
    job: Job<Result<Built, BuildError>>,
    /// Stop: looked at by the build in its walk, while a book is read and hashed and between two
    /// pages (plan A9-N15c, the review of #431).
    halt: Arc<AtomicBool>,
    seen: Arc<Mutex<Seen>>,
    /// The book the catalog was last read for: each book's end writes it.
    book_read: usize,
    stop_asked: bool,
}

impl Drop for Building {
    /// A build dropped (the app gone) is asked to stop between two pages.
    fn drop(&mut self) {
        self.halt.store(true, Ordering::Relaxed);
        self.job.cancel();
    }
}

/// A change of the catalog, or a reading of it.
enum Change {
    Read,
    Add(PathBuf),
    Remove(String),
    Allow(String, bool),
}

/// What a change gave: the catalog as it is now, what leaving a folder took out, whether another
/// build or change held the Library when it was read, and whether it was a change (not a read).
struct Changed {
    catalog: Catalog,
    removed: Option<Removed>,
    busy: bool,
    changed: bool,
}

/// What a plan was made for: a digest of the catalog's folders, yeses and time, made once when the
/// catalog was read ([`plan_key`]).
type PlanKey = u64;

/// The key of the plan for `catalog`.
fn plan_key(catalog: &Catalog) -> PlanKey {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (&catalog.dirs, &catalog.allowed, catalog.updated).hash(&mut h);
    h.finish()
}

/// A result asked to be opened, waiting for its PDF and its page.
#[derive(Clone, Debug)]
struct Opening {
    path: String,
    page: Option<usize>,
    place: Option<Place>,
    san: Option<String>,
    /// The page's reading was asked for it.
    read: bool,
    since: Instant,
    /// Its tab was brought to the front and its page shown, once: from then the person's moves
    /// are never undone; moving away gives the outline up (the review of #431).
    shown: bool,
    /// The file changed since the Library read it: only its page, when it has it.
    changed: bool,
}

/// Chess ▸ Library…: the dialog, the catalog, the build, the search and a result being opened.
#[derive(Default)]
pub struct LibraryUi {
    /// The app's chess folder (the Library is its `library`); `None`: no data folder.
    chess_dir: Option<PathBuf>,
    /// The dialog is shown.
    pub(crate) open: bool,
    pub(crate) view: View,
    catalog: Option<Arc<Catalog>>,
    /// The plan's key for `catalog` ([`plan_key`]).
    catalog_key: PlanKey,
    /// Another build or change held the Library when the catalog was last read; looked at again
    /// every [`OTHER_LOOK`] while the dialog is shown, so its end makes Build ready.
    other_build: bool,
    /// When the catalog was last asked to be read.
    read_at: Option<Instant>,
    changing: Option<Job<Result<Changed, ChangeError>>>,
    /// The catalog is to be read again once no change runs.
    stale: bool,
    plan: Option<(PlanKey, Plan)>,
    planning: Option<(PlanKey, Job<Plan>)>,
    building: Option<Building>,
    /// The last build in this session stopped before its end: Continue.
    stopped: bool,
    pub(crate) fields: Fields,
    search: Search,
    /// The list's rows the keyboard reaches, as the last pass drew them, and the ↑ ↓ Home End
    /// pressed on one of them ([`take_keys`]).
    rows: Vec<egui::Id>,
    keys: Vec<egui::Key>,
    /// The FEN field takes the keyboard at the next pass.
    focus_fen: bool,
    opening: Option<Opening>,
    /// A folder chosen, to add once no change of the catalog and no build runs (the review of
    /// #431: a build writes the catalog after each book), and whether that was said.
    adding: Option<PathBuf>,
    adding_said: bool,
    /// A position the Board tab asked to look for (Look in the Library).
    asked_position: Option<String>,
    hook: Option<BuildHook>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl LibraryUi {
    /// Keep the Library in `chess`'s `library` (the app's data folder's chess folder).
    pub(crate) fn use_folder(&mut self, chess: &Path) {
        self.chess_dir = Some(chess.to_path_buf());
    }

    fn library(&self) -> Option<Library> {
        self.chess_dir.as_ref().map(|d| Library::new(d.join(library::KIND)))
    }

    /// Whether the dialog is shown.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Whether a build is running.
    pub fn building(&self) -> bool {
        self.building.is_some()
    }

    /// The catalog as last read.
    pub fn catalog(&self) -> Option<&Catalog> {
        self.catalog.as_deref()
    }

    /// The search's answer for what was last asked of the Library as it is now, once it is in.
    pub fn found(&self) -> Option<&Found> {
        match &self.search.answer {
            Some((q, Ok(f))) if self.search.now.as_ref() == Some(q) => Some(&f.found),
            _ => None,
        }
    }

    /// Ask the Library for `fen` (the Board tab's Look in the Library): the dialog opens on its
    /// search, with the position in its field.
    pub(crate) fn ask_position(&mut self, fen: String) {
        self.asked_position = Some(fen);
    }

    /// A job is running.
    fn busy(&self) -> bool {
        self.changing.is_some()
            || self.planning.is_some()
            || self.building.is_some()
            || self.search.asking.is_some()
            || self.opening.is_some()
            || self.adding.is_some()
            || (self.open && self.other_build)
    }

    /// What the running build says: the book, of how many, its page of how many.
    fn seen(&self) -> Option<Seen> {
        self.building.as_ref().map(|b| lock(&b.seen).clone())
    }
}

/// Take ↑ ↓ Home End out of `events` while a row of the Library's list has the keyboard, before
/// egui would move the keyboard by direction with them: the list moves among its rows with them at
/// its next pass (the In the book list's rule, plan A9-N3b). Only presses, without modifiers.
pub(crate) fn take_keys(lib: &mut LibraryUi, ctx: &egui::Context, events: &mut Vec<egui::Event>) {
    lib.keys.clear();
    if !lib.open || !lib.rows.iter().any(|id| ctx.memory(|m| m.has_focus(*id))) {
        return;
    }
    let keys = &mut lib.keys;
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

/// The cores the scan uses here, and the machine's.
fn cores(app: &PdfCraftApp) -> (usize, usize) {
    let all = std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
    let used = app.chess.scan_workers.unwrap_or_else(pdfcraft_engine::chess::ChessScanJob::default_workers).clamp(1, all.max(1));
    (used, all)
}

/// About how long reading `pages` takes on `cores` cores, in words ("≈ 1 h 2 min").
pub(crate) fn estimate_words(pages: u64, cores: usize) -> String {
    // Pages are counted, so at most 2^64; a float's rounding is all an estimate needs.
    let secs = pages as f64 * SECONDS_PER_PAGE * MEASURED_CORES / (cores.max(1) as f64);
    crate::chess_engine_review::duration_words(secs)
}

/// Why the PDF at `path` is not opened, in words (the review of #431: each reason its own).
fn not_opened_words(path: &str, why: Skip) -> String {
    let template = match why {
        Skip::InCloud => tl!("{path} is in the cloud, not on this computer: it isn't opened."),
        Skip::Gone => tl!("{path} is no longer there: build the Library again to forget it."),
        Skip::Link => tl!("{path} is a link, which the Library never opens."),
        _ => tl!("{path} can't be opened."),
    };
    crate::i18n::fmt(template, &[("path", path)])
}

/// What a build or a change is told while another holds the Library, in the interface's language.
fn busy_words() -> &'static str {
    tl!("Another build of this Library is running (in another window, another PdfCraft or on the command line): wait for it to end.")
}

/// A count as the interface's language writes it.
fn n(v: impl TryInto<u64>) -> String {
    crate::i18n::number(v)
}

/// Why a book was skipped, in words.
fn skip_words(why: Option<Skip>, error: Option<&str>) -> String {
    let with = |text: &str| match error.filter(|e| !e.is_empty()) {
        // The engine's error after the words, as the board's lines put a sign's name after it.
        Some(e) => format!("{text} ({e})"),
        None => text.to_owned(),
    };
    match why {
        Some(Skip::Never) => tl!("Skipped: copying not allowed").to_owned(),
        Some(Skip::Ask) => tl!("Encrypted: read it into the Library?").to_owned(),
        Some(Skip::DoesNotOpen) => with(tl!("Skipped: does not open")),
        Some(Skip::TooLarge) => tl!("Skipped: larger than 1 GB").to_owned(),
        Some(Skip::Unreadable) => with(tl!("Skipped: can't be read")),
        Some(Skip::Failed) => with(tl!("Skipped: reading failed")),
        Some(Skip::InCloud) => tl!("Skipped: now in the cloud").to_owned(),
        Some(Skip::Link) => tl!("Skipped: a link, never followed").to_owned(),
        Some(Skip::Gone) => tl!("Skipped: no longer there").to_owned(),
        Some(Skip::IndexUnreadable) => tl!("Left as it was: its index can't be read now").to_owned(),
        None => with(tl!("Skipped")),
    }
}

/// Where book `b` stands, in words: read and what it holds, not read yet, stopped partway,
/// skipped and why, in the cloud.
pub(crate) fn state_words(b: &CatalogBook, allowed: bool) -> String {
    match b.state {
        State::Indexed => {
            let c = b.counts.unwrap_or_default();
            let mut said = crate::i18n::fmt(
                tl!("Pages read: {pages} · diagrams: {diagrams} · to review: {review} · games: {games}"),
                &[("pages", &n(c.pages_read)), ("diagrams", &n(c.diagrams)), ("review", &n(c.review)), ("games", &n(c.games))],
            );
            if b.no_piece_model {
                said.push_str(" · ");
                said.push_str(tl!("no piece model: picture diagrams not read"));
            }
            said
        }
        State::Partial => tl!("Stopped partway: Continue goes on from there").to_owned(),
        State::Pending => tl!("Not read yet").to_owned(),
        State::Cloud => tl!("In the cloud: never opened").to_owned(),
        State::Full => tl!("Not kept: the Library is full (128 MB)").to_owned(),
        State::Skipped if b.reason == Some(Skip::Ask) && allowed => tl!("Encrypted: you said yes").to_owned(),
        State::Skipped => skip_words(b.reason, b.error.as_deref()),
    }
}

/// The name a book is shown by: its title, else its first file's name.
fn book_name(b: &CatalogBook) -> String {
    b.title.clone().filter(|t| !t.trim().is_empty()).unwrap_or_else(|| {
        b.paths.first().map(|p| Path::new(p).file_name().map_or_else(|| p.clone(), |f| f.to_string_lossy().into_owned())).unwrap_or_default()
    })
}

/// A move as numbered ("12.Bb5", "12…Bb5") in the notation chosen, and in words, as the moves are
/// named ("12. bishop b5").
fn numbered(number: u16, black: bool, san: &str) -> (String, String) {
    let notation = crate::chess_figurines::Notation::current();
    let n = if black { format!("{number}…") } else { format!("{number}.") };
    (format!("{n}{}", notation.san(san)), format!("{n} {}", crate::chess_figurines::spoken(san, "")))
}

/// A SAN without its check, mate and annotation signs, castling with letters.
fn bare_san(san: &str) -> String {
    let s = san.trim().trim_end_matches(['!', '?', '+', '#']);
    match s {
        "0-0" => "O-O".to_owned(),
        "0-0-0" => "O-O-O".to_owned(),
        other => other.to_owned(),
    }
}

/// A game's players and event, as a result says them ("Capablanca – Lasker · Havana").
fn game_words(g: &IndexedGame) -> String {
    // A tag the game leaves unknown ("?", as PGN writes it) says nothing.
    let known = |s: &str| {
        let s = s.trim();
        (!s.is_empty() && s.chars().any(|c| c != '?')).then(|| s.to_owned())
    };
    let players = match (known(&g.white), known(&g.black)) {
        (None, None) => None,
        (Some(w), None) => Some(w),
        (None, Some(b)) => Some(b),
        (Some(w), Some(b)) => Some(format!("{w} – {b}")),
    };
    let mut parts: Vec<String> = Vec::new();
    parts.extend(players);
    parts.extend(known(&g.event));
    parts.extend(g.eco.as_deref().and_then(known));
    parts.join(" · ")
}

/// A hit's row: its title ("p. 12 · 3.Bb5"), the same in words ("p. 12 · 3. bishop b5"), and
/// what follows it (the game, the line).
fn hit_words(b: &BookHits, h: &Hit) -> (String, String, String) {
    let (what, words) = match h.place {
        Place::Diagram { index } => {
            let d = crate::i18n::fmt(tl!("Diagram {n}"), &[("n", &n(index.saturating_add(1)))]);
            (d.clone(), d)
        }
        Place::Move { .. } => h.san.as_deref().map(|s| numbered(h.number, h.black, s)).unwrap_or_default(),
        Place::Pgn { .. } => {
            let (mv, words) = h.san.as_deref().map(|s| numbered(h.number, h.black, s)).unwrap_or_default();
            let pgn = tl!("The book's PGN");
            (format!("{mv} ({pgn})"), format!("{words} ({pgn})"))
        }
    };
    let on_page = |name: &str| match &h.label {
        Some(page) => crate::i18n::fmt(tl!("p. {page} · {name}"), &[("page", page), ("name", name)]),
        None => name.to_owned(),
    };
    let (title, spoken) = (on_page(&what), on_page(&words));
    let mut details: Vec<String> = Vec::new();
    if let Some(g) = h.game.and_then(|g| b.games.iter().find(|(i, _)| *i == g)).map(|(_, g)| game_words(g)).filter(|w| !w.is_empty()) {
        details.push(g);
    }
    if !matches!(h.place, Place::Diagram { .. }) {
        details.push(if h.main_line { tl!("Main line") } else { tl!("Variation") }.to_owned());
    }
    (title, spoken, details.join(" · "))
}

impl PdfCraftApp {
    /// Chess ▸ Library… (plan A9-N15c): the dialog, on the view it was left on.
    pub fn open_chess_library(&mut self) {
        if self.chess.library.chess_dir.is_none() {
            return self.post_error(tl!("The Library needs the app's data folder, and there is none."));
        }
        self.chess.library.open = true;
        self.chess.library.stale = true;
        self.poll_library();
    }

    /// Run `hook` at each page a build reads (tests: the book, the page and the pages; a test holds
    /// the build there).
    pub fn set_library_build_hook(&mut self, hook: BuildHook) {
        self.chess.library.hook = Some(hook);
    }

    /// Start a change of the catalog (or a reading of it), off the interface thread, when none
    /// runs. Whether it started.
    fn change_library(&mut self, change: Change) -> bool {
        let lib = &mut self.chess.library;
        if lib.changing.is_some() {
            return false;
        }
        let Some(library) = lib.library() else { return false };
        lib.changing = Some(Job::spawn("pdfcraft-chess-library-catalog", self.run_inline, move |_| {
            let done = |catalog: Catalog, removed: Option<Removed>| Changed { busy: library.busy(), catalog, removed, changed: true };
            Ok(match change {
                Change::Read => Ok(Changed { catalog: library.catalog(), removed: None, busy: library.busy(), changed: false }),
                Change::Add(dir) => library.add_folder(&dir).map(|catalog| done(catalog, None)),
                Change::Remove(dir) => library.remove_folder(&dir).map(|removed| done(library.catalog(), Some(removed))),
                Change::Allow(hash, yes) => library.allow(&hash, yes).map(|catalog| done(catalog, None)),
            })
        }));
        true
    }

    /// Add a folder: the app's folder picker, then the catalog.
    fn pick_library_folder(&mut self) {
        let dialog = rfd::AsyncFileDialog::new().set_title(tl!("Choose a folder of chess books"));
        self.ask_one(crate::pickers::Ask::Folder(dialog), None, |app, dir| {
            app.chess.library.open = true;
            app.add_library_folder(dir);
        });
    }

    /// Add folder `dir` to the Library (the folder picker's answer): after the change or reading
    /// running, and never while a build runs, which writes the catalog after each book and would
    /// write over it (the review of #431); held until then, and said.
    pub fn add_library_folder(&mut self, dir: PathBuf) {
        let lib = &mut self.chess.library;
        lib.adding = Some(dir);
        lib.adding_said = false;
        self.poll_library();
    }

    /// The catalog changed (a folder added or removed, a yes, a build's end): what the search
    /// answered before is the last answer, dimmed and inert, and it is asked again (the review of
    /// #431: a book removed is never clickable).
    fn library_changed(&mut self) {
        let lib = &mut self.chess.library;
        lib.search.generation = lib.search.generation.wrapping_add(1);
        if let Some((query, _)) = lib.search.now.clone() {
            self.ask_library(query);
        }
    }

    /// Start a build of the folders chosen (Build and Continue): a job of its own thread, never
    /// the interface's; each book opened from its file in the engine's own session. Not while a
    /// change of the catalog runs or waits (it would be written over).
    pub(crate) fn start_library_build(&mut self) {
        let lib = &self.chess.library;
        if lib.building.is_some() || lib.changing.is_some() || lib.adding.is_some() {
            return;
        }
        let Some(dirs) = self.chess.library.catalog.as_ref().map(|c| c.dirs.iter().map(PathBuf::from).collect::<Vec<_>>()) else { return };
        if dirs.is_empty() {
            return;
        }
        let Some(library) = self.chess.library.library() else { return };
        let Some(chess) = self.chess.library.chess_dir.clone() else { return };
        let (workers, _) = cores(self);
        let opts = BuildOptions {
            models_dir: self.chess_scan_settings().models_dir,
            workers,
            corrections: Some(chess.join("corrections")),
            reference: Some(chess.join("reference")),
            app_reading: self.chess.reading_dir.clone(),
            ..BuildOptions::default()
        };
        let halt = Arc::new(AtomicBool::new(false));
        let opts = BuildOptions { stop: Some(Arc::clone(&halt)), ..opts };
        let halted = Arc::clone(&halt);
        let seen: Arc<Mutex<Seen>> = Arc::default();
        let told = Arc::clone(&seen);
        let hook = self.chess.library.hook.clone();
        // Never inline: a build takes hours (R8).
        let job = Job::spawn("pdfcraft-chess-library-build", false, move |ctl| {
            // Its own failures (busy, the folder) are the build's answer, not the job's.
            Ok(library.build(&dirs, &opts, &mut |p| {
                {
                    let mut s = lock(&told);
                    s.book = p.book;
                    s.books = p.books;
                    s.page = p.page;
                    s.pages = p.pages;
                    s.name = p.path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
                    s.started = true;
                }
                ctl.progress(p.book, p.books);
                if let Some(h) = &hook {
                    h(p.book, p.page, p.pages);
                }
                // Stop: the build ends here, between two pages.
                !ctl.cancelled() && !halted.load(Ordering::Relaxed)
            }))
        });
        let lib = &mut self.chess.library;
        lib.stopped = false;
        lib.building = Some(Building { job, halt, seen, book_read: 0, stop_asked: false });
    }

    /// Stop the build running: in its walk, while a book is read and hashed, or between two pages.
    pub fn stop_library_build(&mut self) {
        if let Some(b) = &mut self.chess.library.building {
            b.halt.store(true, Ordering::Relaxed);
            b.job.cancel();
            b.stop_asked = true;
        }
    }

    /// Quit (before the window closes): the build stops between two pages, waited for at most
    /// [`crate::chess_writer::SETTLE`]. What the Library kept stays (each book's index and the
    /// catalog after it, the pages of the book in progress on disk); the book it was in is said as
    /// stopped partway when the Library opens next.
    pub(crate) fn quit_library(&mut self) {
        let Some(b) = self.chess.library.building.take() else { return };
        b.halt.store(true, Ordering::Relaxed);
        b.job.cancel();
        if !b.job.wait(crate::chess_writer::SETTLE) {
            let s = lock(&b.seen).clone();
            log::info!(
                "the Library's build was stopped by quitting, in book {} of {} ({}), page {} of {}",
                s.book.saturating_add(1),
                s.books,
                s.name,
                s.page,
                s.pages
            );
        }
    }

    /// Wait for the Library's jobs to end, at most `timeout` each (tests). Whether none runs.
    pub fn wait_library(&mut self, timeout: Duration) -> bool {
        for _ in 0..6 {
            let lib = &self.chess.library;
            if let Some(j) = &lib.changing {
                j.wait(timeout);
            }
            if let Some((_, j)) = &lib.planning {
                j.wait(timeout);
            }
            if let Some(b) = &lib.building {
                b.job.wait(timeout);
            }
            if let Some((_, j)) = &lib.search.asking {
                j.wait(timeout);
            }
            self.poll_library();
        }
        let lib = &self.chess.library;
        lib.changing.is_none() && lib.planning.is_none() && lib.building.is_none() && lib.search.asking.is_none()
    }

    /// Search the Library for what the fields ask.
    pub(crate) fn search_library(&mut self) {
        let Ok(Some(query)) = self.chess.library.fields.query() else { return };
        self.ask_library(query);
    }

    /// Search the Library for `query`, as it is now (unless answered or asked already).
    fn ask_library(&mut self, query: FindQuery) {
        let Some(library) = self.chess.library.library() else { return };
        let inline = self.run_inline;
        let started =
            self.chess.library.search.ask(query, |q| Job::spawn("pdfcraft-chess-library-find", inline, move |_| Ok(library.find(&q, MAX_LISTED))));
        if started {
            self.poll_library();
        }
    }

    /// Every frame: the jobs that ended picked up (the catalog, the plan, the build, the search),
    /// a position the board asked for, the catalog read again when it changed, the plan made again
    /// for what it holds now, and a result being opened.
    pub(crate) fn poll_library(&mut self) {
        if let Some(fen) = self.chess.library.asked_position.take() {
            let lib = &mut self.chess.library;
            lib.fields = Fields { fen, ..Fields::default() };
            lib.view = View::Search;
            lib.open = true;
            lib.stale = true;
            self.search_library();
        }
        self.poll_library_catalog();
        self.poll_library_build();
        self.poll_library_plan();
        let lib = &mut self.chess.library;
        if let Some((query, job)) = lib.search.asking.as_mut()
            && let Some(out) = job.poll()
        {
            let query = query.clone();
            lib.search.asking = None;
            // Its rows' words made once, here, in the interface's language and notation.
            lib.search.answer = Some((query, out.map(|found| Arc::new(shown_of(found)))));
        }
        self.library_open_tick();
        if self.chess.library.busy()
            && let Some(ctx) = &self.ctx
        {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
    }

    fn poll_library_catalog(&mut self) {
        let lib = &mut self.chess.library;
        // A folder picked while a build runs waits for its end: the build writes the catalog after
        // each book (the review of #431).
        if lib.adding.is_some() && lib.building.is_some() && !std::mem::replace(&mut lib.adding_said, true) {
            self.inform(tl!("The folder will be added when the build ends."));
        }
        let lib = &mut self.chess.library;
        if lib.changing.is_none()
            && lib.building.is_none()
            && let Some(dir) = lib.adding.take()
        {
            self.change_library(Change::Add(dir));
        }
        let lib = &mut self.chess.library;
        if let Some(out) = lib.changing.as_mut().and_then(Job::poll) {
            lib.changing = None;
            match out {
                Ok(Ok(changed)) => {
                    lib.catalog_key = plan_key(&changed.catalog);
                    lib.catalog = Some(Arc::new(changed.catalog));
                    lib.other_build = changed.busy && lib.building.is_none();
                    if let Some(r) = changed.removed.filter(|r| r.books > 0) {
                        self.inform(crate::i18n::fmt(tl!("Books left out of the Library: {n}"), &[("n", &n(r.books))]));
                    }
                    if changed.changed {
                        self.library_changed();
                    }
                }
                Ok(Err(e)) => {
                    let why = match e {
                        ChangeError::Within(dir) => crate::i18n::fmt(tl!("This folder is in the Library already, through {dir}."), &[("dir", &dir)]),
                        ChangeError::TooMany => crate::i18n::fmt(tl!("The Library has {n} folders already."), &[("n", &n(library::MAX_FOLDERS))]),
                        ChangeError::Busy => busy_words().to_owned(),
                        ChangeError::Unreadable(why) => {
                            crate::i18n::fmt(tl!("The Library's catalog can't be read, so it isn't changed: {e}"), &[("e", &why)])
                        }
                        ChangeError::NotAHash | ChangeError::Write(_) => {
                            crate::i18n::fmt(tl!("The Library's folders couldn't be changed: {e}"), &[("e", &e.to_string())])
                        }
                    };
                    self.post_error(why);
                    self.chess.library.stale = true;
                }
                Err(e) => self.post_error(crate::i18n::fmt(tl!("The Library's folders couldn't be changed: {e}"), &[("e", &e)])),
            }
        }
        // Read again when it changed: while it is looked at, a build writes it, or it was read
        // (the build's end changes what the Books view and the search say).
        let lib = &mut self.chess.library;
        if let Some(b) = &mut lib.building {
            let at = lock(&b.seen).book;
            if at != b.book_read {
                b.book_read = at;
                lib.stale = true;
            }
        }
        // Another build or change holds it: looked at again now and then, while it is shown.
        if lib.open && lib.other_build && lib.read_at.is_none_or(|at| at.elapsed() >= OTHER_LOOK) {
            lib.stale = true;
        }
        let wanted = lib.open || lib.building.is_some() || lib.catalog.is_some();
        if lib.stale && lib.changing.is_none() && wanted && self.change_library(Change::Read) {
            let lib = &mut self.chess.library;
            lib.stale = false;
            lib.read_at = Some(Instant::now());
        }
    }

    /// The plan for what the catalog holds now, made again when it changed (never during a build:
    /// the build says where it is instead).
    fn poll_library_plan(&mut self) {
        let lib = &mut self.chess.library;
        if let Some((key, job)) = lib.planning.as_mut()
            && let Some(out) = job.poll()
        {
            let key = *key;
            lib.planning = None;
            if let Ok(plan) = out {
                lib.plan = Some((key, plan));
            }
        }
        let Some(empty) = lib.catalog.as_ref().map(|c| c.dirs.is_empty()) else { return };
        if !lib.open || lib.building.is_some() || lib.view != View::Books {
            return;
        }
        // Keyed by a digest made once per reading of the catalog, never its lists at each pass.
        let key = lib.catalog_key;
        let planned = lib.plan.as_ref().is_some_and(|(k, _)| *k == key) || lib.planning.as_ref().is_some_and(|(k, _)| *k == key);
        if planned || empty {
            return;
        }
        let Some(library) = lib.library() else { return };
        let inline = self.run_inline;
        // A plan for an older catalog is dropped, which asks it to stop.
        lib.planning = Some((
            key,
            Job::spawn("pdfcraft-chess-library-plan", inline, move |ctl| {
                let stop = || ctl.cancelled();
                Ok(library.plan(Caps::default(), &stop, &mut |done, total| ctl.progress(done, total)))
            }),
        ));
    }

    fn poll_library_build(&mut self) {
        let Some(b) = self.chess.library.building.as_mut() else { return };
        let Some(out) = b.job.poll() else { return };
        self.chess.library.building = None;
        let lib = &mut self.chess.library;
        lib.stale = true;
        lib.plan = None;
        match out {
            Ok(Ok(built)) => {
                lib.stopped = built.stopped;
                if built.stopped {
                    self.inform(crate::i18n::fmt(
                        tl!("The Library's build was stopped: books left: {n}. Continue goes on from there."),
                        &[("n", &n(built.left))],
                    ));
                } else {
                    let indexed = built.books.iter().filter(|r| r.book.state == State::Indexed).count();
                    self.inform(crate::i18n::fmt(
                        tl!("The Library is built: books read now: {read}; books in the Library: {indexed}."),
                        &[("read", &n(built.read)), ("indexed", &n(indexed))],
                    ));
                }
                // What the search found may have changed.
                self.library_changed();
            }
            // Another build or change holds the Library (the review of #431): said, nothing done.
            Ok(Err(BuildError::Busy)) => self.post_error(busy_words()),
            Ok(Err(BuildError::Write(e))) => {
                lib.stopped = true;
                self.post_error(crate::i18n::fmt(tl!("The Library couldn't be built: {e}"), &[("e", &e)]));
            }
            Err(e) => {
                lib.stopped = true;
                self.post_error(crate::i18n::fmt(tl!("The Library couldn't be built: {e}"), &[("e", &e)]));
            }
        }
    }

    /// Open hit `hit` of book `book` of the search's answer (or, `game`, one of its games): in the
    /// tab already showing it, whatever became of its file since; else its PDF, when it is on this
    /// computer (each path looked at once, at this click), said when it changed since the Library
    /// read it; at the page. The dialog closes.
    fn open_library_result(&mut self, book: usize, item: Item) {
        let Some(found) = self.chess.library.found().cloned() else { return };
        let Some(b) = found.books.get(book) else { return };
        let (page, place, san) = match item {
            Item::Hit(i) => {
                let Some(h) = b.hits.get(i) else { return };
                (h.page, Some(h.place), h.san.clone())
            }
            Item::Game(i) => {
                let Some((_, g)) = b.games.get(i) else { return };
                (g.page, None, None)
            }
        };
        let page = page.and_then(|p| usize::try_from(p).ok());
        let opening =
            |path: String, changed: bool| Opening { path, page, place, san: san.clone(), read: false, since: Instant::now(), shown: false, changed };
        // A tab showing the book already: there (the review of #431).
        if let Some(path) = b.paths.iter().find(|p| self.tab_showing(p).is_some()).cloned() {
            self.chess.library.open = false;
            self.chess.library.opening = Some(opening(path, false));
            return self.library_open_tick();
        }
        // The first path still a book here; else why none is opened.
        let mut why: Option<(String, Skip)> = None;
        let mut here: Option<(String, (u64, u64))> = None;
        for p in &b.paths {
            match library::on_this_computer(Path::new(p)) {
                Ok(stamp) => {
                    here = Some((p.clone(), stamp));
                    break;
                }
                Err(e) => {
                    if why.as_ref().is_none_or(|(_, w)| *w != Skip::InCloud) {
                        why = Some((p.clone(), e));
                    }
                }
            }
        }
        let Some((path, (size, time))) = here else {
            let (p, e) = why.unwrap_or_default_path();
            return self.post_error(not_opened_words(&p, e));
        };
        // Changed since the Library read it (its size, and its time on the path it was read from):
        // said, and only its page shown, when it has it (the review of #431).
        let changed = self
            .chess
            .library
            .catalog
            .as_ref()
            .and_then(|c| c.books.iter().find(|k| k.hash.as_deref() == Some(b.hash.as_str())))
            .is_some_and(|k| k.size != size || (k.paths.first() == Some(&path) && k.modified != time));
        if changed {
            self.inform(tl!("This book changed since the Library read it: build the Library again."));
        }
        self.chess.library.open = false;
        self.chess.library.opening = Some(opening(path.clone(), changed));
        self.open_path(&path);
        self.library_open_tick();
    }

    /// A result being opened: once its PDF shows, its tab is brought to the front and its page
    /// shown, once; then the diagram opened, or the move outlined (its page read first when the
    /// Text tab has not read it). From then the person's moves are theirs: going to another tab,
    /// or waiting longer than [`OUTLINE_WAIT`], gives the outline up, said (the review of #431).
    /// Dropped when its PDF did not open (that was said).
    fn library_open_tick(&mut self) {
        let Some(mut want) = self.chess.library.opening.take() else { return };
        let Some(i) = self.tab_showing(&want.path) else {
            let waiting = self.pending_opens.iter().any(|p| p.path.as_deref() == Some(want.path.as_str()))
                || self.password_prompt.as_ref().is_some_and(|p| p.path.as_deref() == Some(want.path.as_str()));
            if waiting && want.since.elapsed() < OPEN_WAIT {
                self.chess.library.opening = Some(want);
            }
            return;
        };
        let Some(id) = self.views.get(i).map(|v| v.id) else { return };
        let pages = self.session.get(id).map_or(0, |d| d.info.pages.len());
        let page = want.page.filter(|p| *p < pages);
        if !want.shown {
            want.shown = true;
            want.since = Instant::now();
            self.active = Some(i);
            let Some(page) = page else {
                if want.page.is_none() && matches!(want.place, Some(Place::Pgn { .. })) {
                    self.inform(tl!("This move is in the book's PGN, which gives no page for it."));
                }
                return;
            };
            if let Some(view) = self.views.get_mut(i) {
                view.go_to_page(page);
            }
            // A book that changed: its page only, the move or the diagram there may be another.
            if want.changed {
                return;
            }
            match want.place {
                Some(Place::Diagram { index }) => {
                    return self.pick_after_scan(id, DiagramAt::new(page, usize::try_from(index).unwrap_or(usize::MAX)));
                }
                Some(Place::Move { .. }) => {}
                _ => return,
            }
        } else if self.active != Some(i) || want.since.elapsed() >= OUTLINE_WAIT {
            return self.inform(tl!("The move wasn't outlined: its page wasn't read in time, or you moved away."));
        }
        let (Some(page), Some(Place::Move { block, inline })) = (page, want.place) else { return };
        let at = MoveAt { page: u32::try_from(page).unwrap_or(u32::MAX), block, inline };
        if self.settle_library_move(id, page, at, want.san.as_deref()) {
            return;
        }
        // Another page's reading runs: after it, without touching the view meanwhile.
        if self.chess.reading.is_some() {
            self.chess.library.opening = Some(want);
            return;
        }
        if want.read {
            return self.inform(crate::i18n::fmt(
                tl!("The move couldn't be outlined: page {page} reads differently now."),
                &[("page", &self.page_label_in(id, page))],
            ));
        }
        want.read = true;
        self.chess.library.opening = Some(want.clone());
        self.read_chess_text(page, false);
        // Read inline (tests): the move is outlined now.
        if self.settle_library_move(id, page, at, want.san.as_deref()) {
            self.chess.library.opening = None;
        }
    }

    /// When the Text tab has read page `page` of document `id`: outline the move at `at` (when it is
    /// the move the Library found there, `san`), or say it can't be. Whether the page was read.
    fn settle_library_move(&mut self, id: pdfcraft_engine::DocId, page: usize, at: MoveAt, san: Option<&str>) -> bool {
        let stamp = self.session.chess_stamp(id);
        let Some((read, doc)) = crate::chess_page_moves::read_page(&self.chess, id, stamp) else { return false };
        if read != page {
            return false;
        }
        let rect = at
            .find(doc)
            .filter(|m| san.is_none_or(|s| bare_san(&m.san) == bare_san(s)))
            .and_then(|m| m.source_rect)
            .map(|r| r.page_rect())
            .map(|r| [r.x0, r.y0, r.x1, r.y1])
            .filter(|r| r.iter().all(|v| v.is_finite()));
        self.chess.library.opening = None;
        match rect {
            Some(rect) => self.chess_text_action(crate::chess_text_ui::TextAction::Show(page, rect)),
            None => self.inform(crate::i18n::fmt(
                tl!("The move couldn't be outlined: page {page} reads differently now."),
                &[("page", &self.page_label_in(id, page))],
            )),
        }
        true
    }
}

/// The position the board shows, as a FEN (its own, when it is not legal).
pub(crate) fn board_fen(chess: &crate::chess_ui::ChessState) -> String {
    let state = chess.board();
    match &state.illegal {
        Some((_, fen)) => fen.clone(),
        None => state.game.fen(),
    }
}

/// The first path of a book and why it is not opened, when none was looked at.
trait OrPath {
    fn unwrap_or_default_path(self) -> (String, Skip);
}

impl OrPath for Option<(String, Skip)> {
    fn unwrap_or_default_path(self) -> (String, Skip) {
        self.unwrap_or_else(|| (String::new(), Skip::Gone))
    }
}

/// A result of the list: a position found, or a game.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Item {
    Hit(usize),
    Game(usize),
}

/// What the dialog was asked for.
enum Act {
    AddFolder,
    RemoveFolder(String),
    Allow(String, bool),
    Build,
    Stop,
    Search,
    FromBoard,
    Open(usize, Item),
    Close,
}

/// The dialog's room: its width, and the height its content scrolls in, from the window's.
fn room(ctx: &egui::Context) -> (f32, f32) {
    let screen = ctx.content_rect();
    let width = (screen.width() - 64.0).clamp(240.0, 620.0);
    let height = (screen.height() - 210.0).max(120.0);
    (width, height)
}

/// The dialog, while it is shown; and, while it is not and a build runs, the build's row at the
/// foot of the page area ([`building_card`]).
pub(crate) fn prompt(app: &mut PdfCraftApp, ctx: &egui::Context) {
    if !app.chess.library.open {
        building_card(app, ctx);
        return;
    }
    let t = Tokens::get(ctx);
    let (width, height) = room(ctx);
    let (used, all) = cores(app);
    let mut acts: Vec<Act> = Vec::new();
    let shown = crate::widgets::show_titled_modal(ctx, "chess-library", tl!("Library"), |ui| {
        ui.set_width(width);
        ui.spacing_mut().item_spacing.y = 6.0;
        // The bar beside the content, never over the fields' ends (§2.8.1).
        crate::widgets::solid_scroll_bar(ui);
        let lib = &mut app.chess.library;
        let mut view = lib.view;
        ui.push_id("chess-library-view", |ui| {
            crate::widgets::segmented(ui, tl!("View"), &mut view, &[(View::Books, tl!("Books")), (View::Search, tl!("Search"))])
        });
        lib.view = view;
        // The build running, in both views: where it is, and Stop.
        if let Some(s) = lib.seen() {
            let stopping = lib.building.as_ref().is_some_and(|b| b.stop_asked);
            if build_row(ui, &t, &s, stopping) {
                acts.push(Act::Stop);
            }
        }
        ui.add_space(4.0);
        egui::ScrollArea::vertical().id_salt(("chess-library-body", view)).max_height(height).auto_shrink([false, true]).show(ui, |ui| {
            ui.set_width(ui.available_width());
            match view {
                View::Books => books_view(ui, &t, lib, used, all, &mut acts),
                View::Search => search_view(ui, &t, lib, &mut acts),
            }
        });
        ui.separator();
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Ids of their own: what comes before them changes as the Library does, and a click
            // must find the same control on the next pass.
            if ui.push_id("chess-library-close", |ui| crate::widgets::pill_button(ui, tl!("Close"), false)).inner.clicked() {
                acts.push(Act::Close);
            }
        });
    });
    if shown.should_close() {
        acts.push(Act::Close);
    }
    for act in acts {
        match act {
            Act::AddFolder => app.pick_library_folder(),
            Act::RemoveFolder(dir) => {
                app.change_library(Change::Remove(dir));
            }
            Act::Allow(hash, yes) => {
                app.change_library(Change::Allow(hash, yes));
            }
            Act::Build => app.start_library_build(),
            Act::Stop => app.stop_library_build(),
            Act::Search => app.search_library(),
            Act::FromBoard => {
                let fen = board_fen(&app.chess);
                app.chess.library.fields.fen = fen;
                app.search_library();
            }
            Act::Open(book, item) => app.open_library_result(book, item),
            // A build running goes on: its row shows at the foot of the page area.
            Act::Close => app.chess.library.open = false,
        }
    }
}

/// A section's heading.
fn section(ui: &mut egui::Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(RichText::new(text).font(theme::semibold(13.0)));
}

/// Text that wraps within the room, small and muted.
fn note(ui: &mut egui::Ui, t: &Tokens, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).small().color(t.text_muted)).wrap());
}

/// The build's row: where it is, Stop, and the bar. Whether Stop was clicked.
fn build_row(ui: &mut egui::Ui, t: &Tokens, s: &Seen, stopping: bool) -> bool {
    let said = build_words(s, stopping);
    let mut stop = false;
    // Stop at the row's end, what it says in the room left of it, from the left edge.
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            stop = ui
                .push_id("chess-library-stop", |ui| crate::icons::button(ui, "circle-stop", crate::widgets::MIN_TARGET, false, tl!("Stop")))
                .inner
                .clicked();
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add(egui::Label::new(RichText::new(&said).small().color(t.text_muted)).wrap());
            });
        });
    });
    let books = s.books.max(1);
    // Books done and the pages of the one being read, within 0..=1; the casts only round.
    let book = s.book.min(books) as f32 + if s.pages > 0 { s.page.min(s.pages) as f32 / s.pages as f32 } else { 0.0 };
    crate::widgets::progress(ui, (book / books as f32).clamp(0.0, 1.0), tl_ctx!("progress bar", "Progress"));
    stop
}

/// Where a build is, in words.
fn build_words(s: &Seen, stopping: bool) -> String {
    if stopping {
        return tl!("Stopping after this page…").to_owned();
    }
    if !s.started {
        return tl!("Building the Library: looking at the folders…").to_owned();
    }
    let book = n(s.book.saturating_add(1).min(s.books.max(1)));
    if s.pages > 0 {
        crate::i18n::fmt(
            tl!("Building the Library: book {n} of {m} ({name}), page {p} of {q}"),
            &[("n", &book), ("m", &n(s.books)), ("name", &s.name), ("p", &n(s.page.min(s.pages))), ("q", &n(s.pages))],
        )
    } else {
        crate::i18n::fmt(tl!("Building the Library: book {n} of {m} ({name})"), &[("n", &book), ("m", &n(s.books)), ("name", &s.name)])
    }
}

/// The Books view: the folders, each book's state, and the build.
fn books_view(ui: &mut egui::Ui, t: &Tokens, lib: &mut LibraryUi, used: usize, all: usize, acts: &mut Vec<Act>) {
    note(
        ui,
        t,
        tl!(
            "The books of the folders you choose, read once, so a position, a move or a player is found in all of them. Nothing is read until you build."
        ),
    );
    let building = lib.building.is_some();
    let busy = building || lib.changing.is_some();
    section(ui, tl!("Folders"));
    let catalog = lib.catalog.clone();
    // The catalog's own list, through its `Arc`: nothing copied at each pass.
    let dirs: &[String] = catalog.as_deref().map_or(&[], |c| c.dirs.as_slice());
    if catalog.is_some() && dirs.is_empty() {
        note(ui, t, tl!("No folder yet: add the folders where your chess books are."));
    }
    for (i, dir) in dirs.iter().enumerate() {
        ui.push_id(("chess-library-folder", i), |ui| {
            ui.horizontal(|ui| {
                let remove_w = crate::widgets::text_width(ui, tl!("Remove"), &egui::TextStyle::Button.resolve(ui.style())) + 24.0;
                let w = (ui.available_width() - remove_w - 8.0).max(40.0);
                let font = egui::TextStyle::Body.resolve(ui.style());
                let shown = crate::widgets::fit_line(dir, w, |s| crate::widgets::text_width(ui, s, &font));
                let (rect, r) = ui.allocate_exact_size(vec2(w, ui.spacing().interact_size.y), egui::Sense::hover());
                ui.painter().text(rect.left_center(), egui::Align2::LEFT_CENTER, &shown, font, t.text);
                r.on_hover_text(dir.as_str()).widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, dir.as_str()));
                let remove = ui.add_enabled(!busy, egui::Button::new(tl!("Remove")).min_size(vec2(24.0, 24.0)));
                // Its label, then the folder it removes.
                let name = format!("{} {dir}", tl!("Remove"));
                remove.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, !busy, &name));
                if remove.on_hover_text(tl!("Its books leave the Library; the files stay where they are.")).clicked() {
                    acts.push(Act::RemoveFolder(dir.clone()));
                }
            });
        });
    }
    ui.horizontal_wrapped(|ui| {
        let add = ui
            .push_id("chess-library-add", |ui| {
                ui.add_enabled_ui(!busy, |ui| crate::widgets::pill_button(ui, tl!("Add a folder…"), dirs.is_empty())).inner
            })
            .inner;
        if dirs.is_empty() {
            crate::widgets::dialog_focus(&add);
        }
        if add.clicked() {
            acts.push(Act::AddFolder);
        }
    });
    if building {
        note(ui, t, tl!("The folders can't change while the Library is being built."));
    }
    // Read in a moment (a job).
    let Some(catalog) = catalog.as_deref() else { return };
    // The books, each with its state in words.
    let books = &catalog.books;
    section(ui, &crate::i18n::fmt(tl!("Books in the Library: {n}"), &[("n", &n(books.len()))]));
    if !books.is_empty() {
        let allowed = |b: &CatalogBook| b.hash.as_deref().is_some_and(|h| catalog.allowed.iter().any(|a| a.eq_ignore_ascii_case(h)));
        let rows = books.len();
        egui::ScrollArea::vertical().id_salt("chess-library-books").max_height(BOOK_ROW * 5.0).auto_shrink([false, true]).show_rows(
            ui,
            BOOK_ROW,
            rows,
            |ui, range| {
                for i in range {
                    let Some(b) = books.get(i) else { continue };
                    ui.push_id(("chess-library-book", i), |ui| book_row(ui, t, b, allowed(b), busy, acts));
                }
            },
        );
    }
    let w = &catalog.walk;
    if !w.links.is_empty() || !w.cloud_folders.is_empty() || !w.unlisted.is_empty() || !w.capped.is_empty() {
        let said = crate::i18n::fmt(
            tl!("Not looked in: links {links}, folders in the cloud {cloud}, folders that can't be listed {unlisted}."),
            &[("links", &n(w.links.len())), ("cloud", &n(w.cloud_folders.len())), ("unlisted", &n(w.unlisted.len()))],
        );
        note(ui, t, &said);
        if !w.capped.is_empty() {
            note(ui, t, tl!("The folders hold more than a build looks at."));
        }
    }
    // The build.
    section(ui, tl!("Build"));
    let cores = crate::i18n::fmt(tl!("Reads on {n} of the {all} cores of this computer."), &[("n", &n(used)), ("all", &n(all))]);
    note(ui, t, &cores);
    // Running: its row is above, with Stop.
    if building {
        return;
    }
    // Another build or change holds the Library (the review of #431); else one the catalog says
    // was running and is not: cut short (PdfCraft closed mid-build, or it stopped on an error).
    if lib.other_build {
        crate::notice::callout(ui, crate::notice::Tone::Warning, busy_words());
    } else if catalog.building.is_some() {
        crate::notice::callout(
            ui,
            crate::notice::Tone::Warning,
            tl!("The last build was cut short (PdfCraft closed, or it stopped on an error): Continue goes on from where it was."),
        );
    }
    if !catalog.dirs.is_empty() {
        let key = lib.catalog_key;
        match (&lib.plan, &lib.planning) {
            (Some((k, plan)), _) if *k == key => plan_words(ui, t, plan, used),
            (_, Some((_, job))) => {
                let (done, total) = job.progress();
                let said = crate::i18n::fmt(
                    tl!("Counting the pages: book {n} of {m}…"),
                    &[("n", &n(done.saturating_add(1).min(total.max(1)))), ("m", &n(total))],
                );
                note(ui, t, &said);
            }
            _ => {}
        }
    }
    let partial = books.iter().any(|b| b.state == State::Partial) || catalog.building.is_some();
    let label = if lib.stopped || partial { tl!("Continue") } else { tl!("Build") };
    let ready = !catalog.dirs.is_empty() && lib.changing.is_none() && lib.adding.is_none() && !lib.other_build;
    let build = ui.push_id("chess-library-build", |ui| ui.add_enabled_ui(ready, |ui| crate::widgets::pill_button(ui, label, true)).inner).inner;
    if !dirs.is_empty() {
        crate::widgets::dialog_focus(&build);
    }
    if build.clicked() {
        acts.push(Act::Build);
    }
}

/// What a build would read, said before it starts: the books, their pages and about how long.
fn plan_words(ui: &mut egui::Ui, t: &Tokens, plan: &Plan, cores: usize) {
    if plan.to_read == 0 {
        note(ui, t, tl!("Every book is read: a build only looks for new or changed ones."));
        return;
    }
    let said = crate::i18n::fmt(
        tl!("To read: {books} books, {pages} pages, {time} (an estimate)"),
        &[("books", &n(plan.to_read)), ("pages", &n(plan.pages)), ("time", &estimate_words(plan.pages, cores))],
    );
    ui.add(egui::Label::new(RichText::new(said).strong()).wrap());
    if plan.uncounted > 0 {
        note(ui, t, &crate::i18n::fmt(tl!("Books whose pages could not be counted: {n}"), &[("n", &n(plan.uncounted))]));
    }
}

/// One book's row: its name, and its state in words, with Yes (or Leave out) for an encrypted
/// book.
fn book_row(ui: &mut egui::Ui, t: &Tokens, b: &CatalogBook, allowed: bool, busy: bool, acts: &mut Vec<Act>) {
    ui.allocate_ui(vec2(ui.available_width(), BOOK_ROW), |ui| {
        ui.set_min_height(BOOK_ROW - 4.0);
        ui.spacing_mut().item_spacing.y = 1.0;
        let name = book_name(b);
        let paths = b.paths.join("\n");
        ui.add(egui::Label::new(RichText::new(&name).strong()).truncate()).on_hover_text(&paths);
        let said = state_words(b, allowed);
        let ask = b.state == State::Skipped && b.reason == Some(Skip::Ask);
        ui.horizontal(|ui| {
            let button = match (ask, allowed) {
                (true, false) => Some((tl!("Yes"), true)),
                (true, true) => Some((tl!("Leave out"), false)),
                _ => None,
            };
            let colour = if b.state == State::Skipped && !ask { t.warning_text } else { t.text_muted };
            // The button at the row's end; the state in the room left of it, from the left edge.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let (Some((label, yes)), Some(hash)) = (button, b.hash.as_ref()) {
                    let r = ui.add_enabled(!busy, egui::Button::new(label).min_size(vec2(24.0, 22.0)));
                    let spoken = format!("{label}, {name}, {said}");
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, !busy, &spoken));
                    if r.clicked() {
                        acts.push(Act::Allow(hash.clone(), yes));
                    }
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.add(egui::Label::new(RichText::new(&said).small().color(colour)).truncate()).on_hover_text(&said);
                });
            });
        });
    });
}

/// The Search view: the fields, then the list.
fn search_view(ui: &mut egui::Ui, t: &Tokens, lib: &mut LibraryUi, acts: &mut Vec<Act>) {
    let rows = std::mem::take(&mut lib.rows);
    let ctx = ui.ctx().clone();
    // ↑ ↓ Home End among the rows the last pass drew: taken before egui moves the keyboard by
    // direction ([`take_keys`]).
    let keys = std::mem::take(&mut lib.keys);
    if let Some(at) = rows.iter().position(|id| ctx.memory(|m| m.has_focus(*id))) {
        let last = rows.len().saturating_sub(1);
        let to = keys.iter().fold(at, |to, key| match key {
            egui::Key::ArrowUp => to.saturating_sub(1),
            egui::Key::ArrowDown => to.saturating_add(1).min(last),
            egui::Key::Home => 0,
            egui::Key::End => last,
            _ => to,
        });
        if to != at
            && let Some(id) = rows.get(to)
        {
            ctx.memory_mut(|m| m.request_focus(*id));
        }
    }
    let label_font = egui::TextStyle::Body.resolve(ui.style());
    let labels = [tl!("Position (FEN)"), tl!("Move (SAN)"), tl!("Player"), tl!("Event"), tl!("Opening (ECO)")];
    let label_w = labels.iter().map(|l| crate::widgets::text_width(ui, l, &label_font)).fold(0.0f32, f32::max).min(ui.available_width() * 0.4);
    let mut enter = false;
    let fields = &mut lib.fields;
    let mut field = |ui: &mut egui::Ui, label: &str, text: &mut String, hint: &str, extra: f32, focus: bool| -> egui::Response {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(label_w, ui.spacing().interact_size.y), egui::Sense::hover());
            let shown = crate::widgets::fit_line(label, label_w, |s| crate::widgets::text_width(ui, s, &label_font));
            ui.painter().text(rect.left_center(), egui::Align2::LEFT_CENTER, shown, label_font.clone(), t.text);
            let w = (ui.available_width() - extra).max(60.0);
            // Its own id, by its label: the field keeps its text and the keyboard whatever is
            // shown above it.
            let edit = crate::widgets::line(text).id_salt(("chess-library-field", label)).hint_text(hint);
            let r = ui.add(edit.desired_width(w).char_limit(MAX_TEXT_CHARS.saturating_mul(2)));
            ui.ctx().accesskit_node_builder(r.id, |node| node.set_label(label));
            if focus {
                r.request_focus();
            }
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                enter = true;
            }
            r
        })
        .inner
    };
    let board_w = crate::widgets::text_width(ui, tl!("From the board"), &egui::TextStyle::Button.resolve(ui.style())) + 24.0;
    let focus = std::mem::take(&mut lib.focus_fen);
    ui.horizontal(|ui| {
        let w = ui.available_width();
        ui.allocate_ui(vec2(w - board_w - 8.0, ui.spacing().interact_size.y), |ui| {
            field(ui, labels[0], &mut fields.fen, "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1", 0.0, focus);
        });
        let r = ui
            .push_id("chess-library-from-board", |ui| ui.add(egui::Button::new(tl!("From the board")).min_size(vec2(24.0, 24.0))))
            .inner
            .on_hover_text(tl!("The position the Board tab shows"));
        if r.clicked() {
            acts.push(Act::FromBoard);
        }
    });
    ui.horizontal(|ui| {
        ui.add_space(label_w + ui.spacing().item_spacing.x);
        ui.add(crate::widgets::checkbox(&mut fields.any_side, tl!("Either side to move")));
    });
    field(ui, labels[1], &mut fields.san, "Bxh7+", 0.0, false);
    field(ui, labels[2], &mut fields.player, "Capablanca", 0.0, false);
    field(ui, labels[3], &mut fields.event, "", 0.0, false);
    field(ui, labels[4], &mut fields.eco, "B90", 0.0, false);
    let query = fields.query();
    let ready = matches!(query, Ok(Some(_)));
    ui.horizontal(|ui| {
        let search =
            ui.push_id("chess-library-search", |ui| ui.add_enabled_ui(ready, |ui| crate::widgets::pill_button(ui, tl!("Search"), true)).inner).inner;
        if search.clicked() || (enter && ready) {
            acts.push(Act::Search);
        }
    });
    match &query {
        Err(e) => {
            let said = crate::i18n::fmt(tl!("Not a FEN: {e}"), &[("e", e)]);
            ui.add(egui::Label::new(RichText::new(said).color(t.danger_text)).wrap());
        }
        Ok(None) => note(ui, t, tl!("Write a position (FEN), a move such as Bxh7+, a player, an event or an opening code such as B90.")),
        Ok(Some(_)) => {}
    }
    ui.separator();
    results(ui, t, lib, acts);
}

/// The list of what was found, book by book (the In the book list's rules, plans A9-N3 and N3b).
fn results(ui: &mut egui::Ui, t: &Tokens, lib: &mut LibraryUi, acts: &mut Vec<Act>) {
    let (shown, stale) = match lib.search.status() {
        Status::Idle => return,
        Status::Searching(None) => {
            note(ui, t, tl!("Looking in the Library…"));
            return;
        }
        Status::Failed(why) => {
            crate::notice::callout(ui, crate::notice::Tone::Danger, &crate::i18n::fmt(tl!("The Library couldn't be searched: {e}"), &[("e", &why)]));
            return;
        }
        Status::Searching(Some(f)) => {
            ui.multiply_opacity(0.5);
            (f, true)
        }
        Status::Found(f) => (f, false),
    };
    let found = &shown.found;
    if found.indexes_read == 0 {
        note(ui, t, tl!("No book of the Library is read yet: build it first, in Books."));
        return;
    }
    let counts = if stale { tl!("Looking in the Library…").to_owned() } else { crate::i18n::fmt(tl!("Found: {n}"), &[("n", &n(found.total))]) };
    note(ui, t, &counts);
    if found.not_indexed > 0 {
        note(ui, t, &crate::i18n::fmt(tl!("Books not read yet, not looked in: {n}"), &[("n", &n(found.not_indexed))]));
    }
    if !found.unread.is_empty() {
        note(ui, t, &crate::i18n::fmt(tl!("Books whose index could not be read: {n}"), &[("n", &n(found.unread.len()))]));
    }
    if found.total == 0 {
        if !stale {
            ui.add(egui::Label::new(tl!("Nothing in the Library matches.")).wrap());
        }
        return;
    }
    for (bi, b) in shown.books.iter().enumerate() {
        ui.add_space(4.0);
        ui.add(egui::Label::new(RichText::new(&b.head).font(theme::semibold(12.5))).truncate()).on_hover_text(&b.paths);
        for (item, title, spoken, details) in &b.rows {
            let (clicked, id) = ui
                .push_id(("chess-library-hit", bi, *item), |ui| {
                    let r = ui.add(egui::Button::new(RichText::new(title)).truncate().min_size(vec2(ui.available_width(), 24.0)));
                    let r = r.on_hover_text(title.as_str());
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, spoken));
                    if r.gained_focus() {
                        r.scroll_to_me(None);
                    }
                    if !details.is_empty() {
                        ui.add(egui::Label::new(RichText::new(details).small().color(t.text_muted)).truncate());
                    }
                    (r.clicked(), r.id)
                })
                .inner;
            lib.rows.push(id);
            // A row of the last answer, shown while the next is looked for, does nothing.
            if clicked && !stale {
                acts.push(Act::Open(bi, *item));
            }
        }
    }
    if found.listed < found.total {
        note(ui, t, &crate::i18n::fmt(tl!("The first {n} are listed."), &[("n", &n(found.listed))]));
    }
}

/// An answer with its rows' words, made once when it comes (the review of #431): each book's line
/// and paths, and each result's title, the same in words (its name) and what follows it.
fn shown_of(found: Found) -> Shown {
    let books = found
        .books
        .iter()
        .map(|b| {
            let total = if b.hits.is_empty() { b.games_total } else { b.total };
            let head = format!("{} · {}", b.title, crate::i18n::fmt(tl!("Found: {n}"), &[("n", &n(total))]));
            // Named as Show in PDF, the result in words, the book and what follows.
            let named = |words: &str, details: &str| {
                [tl!("Show in PDF"), words, b.title.as_str(), details].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(", ")
            };
            let rows: Vec<(Item, String, String, String)> = if b.hits.is_empty() {
                b.games
                    .iter()
                    .enumerate()
                    .map(|(i, (_, g))| {
                        let page = g.page.map(|p| crate::i18n::fmt(tl!("from p. {page}"), &[("page", &n(p.saturating_add(1)))])).unwrap_or_default();
                        let words = game_words(g);
                        let spoken = named(&words, &page);
                        (Item::Game(i), words, spoken, page)
                    })
                    .collect()
            } else {
                b.hits
                    .iter()
                    .enumerate()
                    .map(|(i, h)| {
                        let (title, words, details) = hit_words(b, h);
                        let spoken = named(&words, &details);
                        (Item::Hit(i), title, spoken, details)
                    })
                    .collect()
            };
            ShownBook { head, paths: b.paths.join("\n"), rows }
        })
        .collect();
    Shown { found, books }
}

/// While the dialog is closed and a build runs: its row at the foot of the page area, with Stop and
/// Open the Library, so the build is seen and stopped without the dialog.
pub(crate) fn building_card(app: &mut PdfCraftApp, ctx: &egui::Context) {
    let Some(s) = app.chess.library.seen() else { return };
    let stopping = app.chess.library.building.as_ref().is_some_and(|b| b.stop_asked);
    let t = Tokens::get(ctx);
    let area = app.active.and_then(|i| app.views.get(i)).map(|v| v.viewport_rect()).filter(|r| r.is_positive()).unwrap_or_else(|| ctx.content_rect());
    let width = (area.width() - 32.0).clamp(160.0, 360.0);
    let mut stop = false;
    let mut open = false;
    egui::Area::new(egui::Id::new("chess-library-building"))
        .order(egui::Order::Foreground)
        .pivot(egui::Align2::RIGHT_BOTTOM)
        .fixed_pos(area.right_bottom() - vec2(16.0, 16.0))
        .show(ctx, |ui| {
            egui::Frame::NONE
                .fill(t.card)
                .stroke(egui::Stroke::new(1.0, t.border_strong))
                .corner_radius(egui::CornerRadius::same(t.radius))
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.set_width(width);
                    stop = build_row(ui, &t, &s, stopping);
                    ui.horizontal(|ui| {
                        let r =
                            ui.push_id("chess-library-reopen", |ui| ui.add(egui::Button::new(tl!("Open the Library")).min_size(vec2(24.0, 24.0))));
                        if r.inner.clicked() {
                            open = true;
                        }
                    });
                });
        });
    if stop {
        app.stop_library_build();
    }
    if open {
        app.open_chess_library();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The estimate: pages at the time measured, on the cores used, in words; never a panic.
    #[test]
    fn the_estimate_is_pages_at_the_measured_time_on_the_cores_used() {
        // The 38 books measured: about an hour on four cores, four on one.
        assert_eq!(estimate_words(16_944, 4), "≈ 1 h 2 min");
        assert_eq!(estimate_words(16_944, 1), "≈ 4 h 5 min");
        assert_eq!(estimate_words(0, 4), "≈ 1 min");
        assert_eq!(estimate_words(u64::MAX, 0), crate::chess_engine_review::duration_words(f64::INFINITY));
    }

    /// The fields make the query: a FEN that is not one is refused with why; nothing asked is no
    /// query; the texts are trimmed.
    #[test]
    fn the_fields_make_the_query() {
        assert_eq!(Fields::default().query(), Ok(None));
        assert!(Fields { fen: "not a fen".into(), ..Fields::default() }.query().is_err());
        let q = Fields { player: "  Capablanca ".into(), eco: "B9".into(), ..Fields::default() }.query().unwrap().unwrap();
        assert_eq!((q.player.as_deref(), q.eco.as_deref(), q.position), (Some("Capablanca"), Some("B9"), None));
        let start = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
        assert!(Fields { fen: start.into(), ..Fields::default() }.query().unwrap().unwrap().position.is_some());
    }

    /// Plan A9-N3b's rules, for the Library: a newer search takes the place of one running, the
    /// same is not asked twice unless the Library changed, and the last answer stays (dimmed) while
    /// the next is looked for.
    #[test]
    fn the_newest_search_takes_the_place_of_one_running() {
        let a = FindQuery { player: Some("A".into()), ..FindQuery::default() };
        let b = FindQuery { player: Some("B".into()), ..FindQuery::default() };
        let mut gates = Vec::new();
        let mut gated = |_: FindQuery| {
            let (go, gate) = std::sync::mpsc::channel::<()>();
            gates.push(go);
            Job::spawn("library-test", false, move |_| gate.recv().map(|()| Found::default()).map_err(|e| e.to_string()))
        };
        let shown = || Ok(Arc::new(Shown { found: Found::default(), books: Vec::new() }));
        let mut s = Search::default();
        assert!(matches!(s.status(), Status::Idle));
        assert!(s.ask(a.clone(), &mut gated));
        assert!(!s.ask(a.clone(), &mut gated), "running: not asked again");
        assert!(s.ask(b.clone(), &mut gated), "the newest at once");
        assert!(s.asking.as_ref().is_some_and(|(q, _)| q.0 == b));
        s.answer = Some(((a.clone(), 0), shown()));
        assert!(matches!(s.status(), Status::Searching(Some(_))), "the last answer, dimmed");
        s.asking = None;
        s.answer = Some(((b.clone(), 0), shown()));
        assert!(matches!(s.status(), Status::Found(_)));
        assert!(!s.ask(b.clone(), &mut gated), "answered");
        // The Library changed (the review of #431): the answer before is the last one, dimmed and
        // inert, and the same search is asked again.
        s.generation += 1;
        assert!(s.ask(b, &mut gated), "asked again of the Library as it is now");
        assert!(matches!(s.status(), Status::Searching(Some(_))));
        drop(gates);
    }

    /// Every state and every reason a book is skipped for is said in words, never as its name.
    #[test]
    fn every_state_and_reason_is_said_in_words() {
        let mut said = std::collections::BTreeSet::new();
        for state in [State::Indexed, State::Partial, State::Pending, State::Cloud, State::Full] {
            let w = state_words(&CatalogBook { state, ..CatalogBook::default() }, false);
            assert!(w != state.name() && !w.contains('_'), "{w}");
            said.insert(w);
        }
        for reason in [
            Skip::Never,
            Skip::Ask,
            Skip::DoesNotOpen,
            Skip::TooLarge,
            Skip::Unreadable,
            Skip::Failed,
            Skip::InCloud,
            Skip::Link,
            Skip::Gone,
            Skip::IndexUnreadable,
        ] {
            let b = CatalogBook { state: State::Skipped, reason: Some(reason), ..CatalogBook::default() };
            let w = state_words(&b, false);
            // Words, never the reason's name in the tools' replies ("does_not_open").
            assert!(w != reason.name() && !w.contains('_'), "{w}");
            said.insert(w);
        }
        assert_eq!(said.len(), 15, "each its own words: {said:#?}");
        let ask = CatalogBook { state: State::Skipped, reason: Some(Skip::Ask), ..CatalogBook::default() };
        assert_ne!(state_words(&ask, true), state_words(&ask, false), "a yes is said");
        let failed =
            CatalogBook { state: State::Skipped, reason: Some(Skip::DoesNotOpen), error: Some("no trailer".into()), ..CatalogBook::default() };
        assert!(state_words(&failed, false).ends_with("(no trailer)"));
    }

    /// The Library's item is desktop only: the web's menus and palette leave it out.
    #[test]
    fn the_library_is_desktop_only() {
        assert!(crate::commands::DESKTOP_ONLY.contains(&"chess.library"));
    }

    /// A PDF of `n` blank 600 × 800 pt pages.
    fn blank_pages(n: usize) -> Vec<u8> {
        let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 3 + i)).collect();
        let mut objs = vec![
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            format!("<< /Type /Pages /Kids [{}] /Count {n} /MediaBox [0 0 600 800] >>", kids.join(" ")),
        ];
        objs.extend((0..n).map(|_| "<< /Type /Page /Parent 2 0 R >>".to_string()));
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (i, o) in objs.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
        }
        let xref = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
        for o in offsets {
            out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
        out
    }

    /// The review of #431 (7): while another page's reading runs, a result being opened brings its
    /// tab and page to the front once, never again: the person going to another tab gives the
    /// outline up, said, and is left there; so does a wait past [`OUTLINE_WAIT`].
    #[test]
    fn waiting_for_a_reading_never_pins_the_view() {
        let mut app = PdfCraftApp::new();
        app.run_inline = true;
        app.open_bytes("a.pdf", Some("library-pin-a.pdf".into()), blank_pages(3)).unwrap();
        app.open_bytes("b.pdf", None, blank_pages(1)).unwrap();
        let a = app.views[0].id;
        let stamp = app.session.chess_stamp(a).unwrap();
        let opening = || Opening {
            path: "library-pin-a.pdf".into(),
            page: Some(2),
            place: Some(Place::Move { block: 0, inline: 0 }),
            san: None,
            read: false,
            since: Instant::now(),
            shown: false,
            changed: false,
        };
        // Another page's reading runs: the result waits behind it.
        app.chess.reading = Some(crate::chess_text_ui::ChessJob::never_ending_read(a, 0, stamp));
        app.active = Some(1);
        app.chess.library.opening = Some(opening());
        app.library_open_tick();
        assert_eq!(app.active, Some(0), "its tab, once");
        assert!(app.chess.library.opening.as_ref().is_some_and(|o| o.shown), "waiting for the reading");
        app.library_open_tick();
        assert!(app.chess.library.opening.is_some(), "still waiting, the view left alone");
        // The person goes to the other tab: left there, the outline given up and said.
        app.active = Some(1);
        app.library_open_tick();
        assert_eq!(app.active, Some(1), "never pulled back");
        assert!(app.chess.library.opening.is_none(), "given up");
        assert!(app.notice.as_ref().is_some_and(|n| n.text.contains("wasn't outlined")), "said");
        // Past the bounded wait: given up too.
        app.notice = None;
        app.active = Some(0);
        let late = Instant::now().checked_sub(OUTLINE_WAIT + Duration::from_secs(1)).unwrap_or_else(Instant::now);
        app.chess.library.opening = Some(Opening { shown: true, since: late, ..opening() });
        app.library_open_tick();
        assert!(app.chess.library.opening.is_none() && app.notice.as_ref().is_some_and(|n| n.text.contains("wasn't outlined")));
    }

    /// The review of #431 (12): each reason a result's PDF is not opened has its own words.
    #[test]
    fn why_a_result_is_not_opened_is_said_for_each_reason() {
        let said: Vec<String> = [Skip::InCloud, Skip::Gone, Skip::Link, Skip::Unreadable].into_iter().map(|w| not_opened_words("x.pdf", w)).collect();
        assert!(said.iter().all(|s| s.starts_with("x.pdf")));
        assert_eq!(said.iter().collect::<std::collections::BTreeSet<_>>().len(), 4, "{said:?}");
        assert!(said[1].contains("no longer there") && !said[2].contains("no longer there") && !said[3].contains("no longer there"));
    }
}
