//! Chess ▸ Check with the engine… (plan A9-N17b, finding A8-10): the book says × the calculation
//! says, for a whole book, in a batch an engine the app runs works through. The engine's half (the
//! judgement, the batch, the report) is `pdfcraft_engine::chess::review` (A9-N17a).
//!
//! - **The dialog** asks which pages and how long each position is searched: to a depth, or for a
//!   number of seconds. **Count the positions** reads those pages off the interface thread (a job:
//!   the diagrams a block of pages at a time, then the text whole, linked to the book's PGN) and
//!   says, before anything starts, how many positions the engine is to be asked and about how long
//!   that takes ([`estimate_words`]), leaving out what the evaluations already hold at that depth
//!   ([`review::Batch`]). A book with more positions than the evaluations keep for a document
//!   ([`MAX_EVALS`]) is said too.
//! - **The batch** is a job ([`crate::chess_job::Job`]) on a thread of its own, never inline,
//!   driving an engine started here with the engine Preferences ([`crate::chess_engine_prefs`])
//!   through [`review::run`]. Its row in the Text tab says how far it is and stops it. Stopped, or
//!   ended by an engine that died or hung, it keeps the positions it has not asked, and **Continue
//!   the check** asks only those; a new count leaves out what the store holds.
//! - **What it finds is kept in batches**, never at each position (the store writes the whole
//!   file): every [`WRITE_POSITIONS`] positions or [`WRITE_EVERY`], and at the end, by a writer
//!   thread ([`crate::chess_writer::StoreWriter`], the last request wins) that merges them into the
//!   document's `evals` file. A document whose chess may not be written ([`crate::chess_keep`])
//!   keeps them for the session only, as the board's own evaluations are not kept for it.
//! - **The divergences** ([`review::divergences`]) and the evaluation beside each move of the Text
//!   tab come from the store and what the batch found. The list takes the page text's place in
//!   the Text tab ("‹ Text" and Esc go back), each row with **Show in PDF**.
//! - **Desktop only:** the engine is a process; the web build has none of this.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use egui::RichText;
use pdfcraft_engine::DocId;
use pdfcraft_engine::chess::analysis::{Analyser, AnalysisError, AnalysisOptions, MAX_DEPTH};
use pdfcraft_engine::chess::document::OcrOptions;
use pdfcraft_engine::chess::evals::{Eval, EvalFile, Evals as EvalsKind, MAX_EVALS};
use pdfcraft_engine::chess::pdfcraft_chess::judge::{Limits, Value};
use pdfcraft_engine::chess::pdfcraft_chessvision::doc::reference::{self, ReferenceIndex};
use pdfcraft_engine::chess::pdfcraft_chessvision::doc::{Block, ChessDoc};
use pdfcraft_engine::chess::review::{self, Batch, Budget, Computed, Divergence, Failure, Found, Place, Ran, Review, Said, Source};
use pdfcraft_engine::chess::{DocKeys, ScanStamp};

use crate::PdfCraftApp;
use crate::chess_export_ui::Pages;
use crate::chess_job::Job;
use crate::chess_keep::Keeping;
use crate::chess_store_ui::in_store;
use crate::chess_text_ui::{CheckView, LongRow, TextAction};
use crate::theme::{self, Tokens};

/// The depth asked by default: the least the report judges at (D29, [`Limits::min_depth`]).
pub(crate) const DEFAULT_DEPTH: u32 = 18;

/// The seconds a position is searched by default (A8-10: 12,000 positions in about an hour).
const DEFAULT_SECONDS: f32 = 0.3;

/// The fewest and the most seconds a position may be given.
const MIN_SECONDS: f32 = 0.1;
const MAX_SECONDS: f32 = 600.0;

/// The longest a position searched to a depth may take: past it the engine is stopped and what it
/// found is kept (shallower: a later check asks it again).
const DEPTH_BUDGET: Duration = Duration::from_secs(120);

/// How many positions found are kept at a time, at least ([`WRITE_EVERY`] too): the store writes
/// the document's whole file each time.
const WRITE_POSITIONS: usize = 64;

/// How long found positions wait to be kept, at most, while the batch runs.
const WRITE_EVERY: Duration = Duration::from_secs(30);

/// Pages whose diagrams the count scans at a time, so it says how far it is and stops between two.
const BLOCK: usize = 16;

/// The milliseconds a position takes at depth 18, about (A8-10: 0.3 s), for the estimate of a
/// check to a depth; each two plies more about double it (an estimate, said with "≈").
const MS_AT_18: f64 = 300.0;

/// How each position is searched: to a depth, or for a number of seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Limit {
    Depth(u32),
    Seconds(f32),
}

/// `s` seconds as a duration within [`MIN_SECONDS`]..=[`MAX_SECONDS`]; the default for a number
/// that is not one.
fn seconds(s: f32) -> Duration {
    let s = if s.is_finite() { s.clamp(MIN_SECONDS, MAX_SECONDS) } else { DEFAULT_SECONDS };
    Duration::from_secs_f32(s)
}

impl Limit {
    /// What [`review::run`] may spend: a depth with [`DEPTH_BUDGET`] a position, or the seconds
    /// with no depth to stop at. The evaluations come back one position at a time (the bar moves
    /// with each); they are kept in batches here.
    fn budget(self) -> Budget {
        match self {
            Limit::Depth(d) => Budget { depth: d.clamp(1, MAX_DEPTH), per_position: DEPTH_BUDGET, chunk: 1 },
            Limit::Seconds(s) => Budget { depth: MAX_DEPTH, per_position: seconds(s), chunk: 1 },
        }
    }

    /// The depth a position held in the evaluations is left out at: the one asked; searched for a
    /// time, any (an exact evaluation, however deep the time let it go: asking it again for the
    /// same time would reach the same depth). A check to a depth asks again what is shallower
    /// than it; the dialog says so ([`time_note`]).
    pub(crate) fn kept_depth(self) -> u32 {
        match self {
            Limit::Depth(d) => d.clamp(1, MAX_DEPTH),
            Limit::Seconds(_) => 1,
        }
    }

    /// About how many milliseconds a position takes.
    fn per_position_ms(self) -> u64 {
        match self {
            // At most 0.3 s × 2^40.5 (depth 99): a float a u64 holds, the cast exact to the unit.
            Limit::Depth(d) => (MS_AT_18 * 2f64.powf((f64::from(d.clamp(1, MAX_DEPTH)) - 18.0) / 2.0)).round() as u64,
            Limit::Seconds(s) => u64::try_from(seconds(s).as_millis()).unwrap_or(u64::MAX),
        }
    }
}

/// About how long `positions` take at `limit`, in words: "≈ 1 h 5 min", "≈ 40 min" (a minute at
/// least).
pub(crate) fn estimate_words(positions: usize, limit: Limit) -> String {
    let ms = limit.per_position_ms().saturating_mul(u64::try_from(positions).unwrap_or(u64::MAX));
    minutes_words(ms.div_ceil(60_000))
}

/// `secs` seconds in words, rounded up to the minute ([`minutes_words`]).
pub(crate) fn duration_words(secs: f64) -> String {
    // Within 1..=1e12 (NaN as 1): the cast of a clamped whole float is exact.
    let minutes = if secs.is_nan() { 1 } else { (secs / 60.0).ceil().clamp(1.0, 1e12) as u64 };
    minutes_words(minutes)
}

/// `minutes` (at least one) in words: "≈ 2 h", "≈ 1 h 5 min", "≈ 40 min".
fn minutes_words(minutes: u64) -> String {
    let minutes = minutes.max(1);
    let (h, m) = (minutes / 60, minutes % 60);
    let (h, m) = (crate::i18n::number(h), m.to_string());
    match (h.as_str(), m.as_str()) {
        ("0", _) => crate::i18n::fmt(tl!("≈ {m} min"), &[("m", &m)]),
        (_, "0") => crate::i18n::fmt(tl!("≈ {h} h"), &[("h", &h)]),
        _ => crate::i18n::fmt(tl!("≈ {h} h {m} min"), &[("h", &h), ("m", &m)]),
    }
}

/// What a count read: the pages chosen, linked to the book's PGN, the evaluations the store holds
/// (with what was found and not written yet), the batch they leave and the report they give.
struct Prepared {
    doc: ChessDoc,
    refs: Option<Arc<ReferenceIndex>>,
    evals: EvalFile,
    batch: Batch,
    review: Review,
    cipher: pdfcraft_engine::chess::pdfcraft_chessvision::cipher::BookCipher,
    cached: pdfcraft_engine::chess::ChessCached,
    /// Why the evaluations could not be read, when they could not (none are left out then).
    unread: Option<String>,
}

/// What a count is for: the pages (0-based, empty for all), the depth what is held is left out at,
/// and the document's state.
#[derive(Clone, Debug, PartialEq)]
struct CountKey {
    pages: Vec<usize>,
    depth: u32,
    stamp: Option<ScanStamp>,
}

/// The dialog, while shown.
pub(crate) struct Dialog {
    doc: DocId,
    page_count: usize,
    pages: Pages,
    /// Searched to a depth (else for seconds), and the two fields.
    by_depth: bool,
    depth: u32,
    seconds: f32,
    counting: Option<(CountKey, Job<Prepared>)>,
    counted: Option<(CountKey, Box<Prepared>)>,
    /// Why the last count failed.
    failed: Option<String>,
}

impl Dialog {
    fn limit(&self) -> Limit {
        if self.by_depth { Limit::Depth(self.depth) } else { Limit::Seconds(self.seconds) }
    }

    fn key(&self, stamp: Option<ScanStamp>) -> CountKey {
        CountKey { pages: self.pages.pages(self.page_count), depth: self.limit().kept_depth(), stamp }
    }
}

/// A batch stopped before its end: what Continue asks.
struct Stopped {
    batch: Batch,
    limit: Limit,
    /// The batch's size when it first started, and how many of it were asked.
    all: usize,
    asked: usize,
}

/// What the check found of one document, for the session.
pub(crate) struct Checked {
    /// The pages read, linked to the book's PGN.
    doc: Arc<ChessDoc>,
    refs: Option<Arc<ReferenceIndex>>,
    /// The store's evaluations and what the batches found since (beside the moves, and for the
    /// report).
    evals: Arc<EvalFile>,
    /// Bumped whenever `evals` changes: the Text tab lays its paragraphs out again.
    generation: u64,
    review: Review,
    /// Each page as the book numbers it, by index.
    labels: Vec<String>,
    stopped: Option<Stopped>,
    /// The Text tab shows the Divergences list in the page text's place.
    list: bool,
}

/// What a running batch ended with.
struct Ended {
    ran: Ran,
    review: Review,
}

/// The batch running.
struct Running {
    doc: DocId,
    /// The document's name, to say it once it is closed.
    name: String,
    job: Job<Ended>,
    /// The evaluations the engine handed back and the interface has not taken yet.
    found: Arc<Mutex<Vec<(String, Eval)>>>,
    /// The batch, which the job holds locked while it runs and leaves here however it ends (a
    /// panic in it included): the interface takes it once the job is done, for Continue.
    batch: Arc<Mutex<Batch>>,
    limit: Limit,
    /// The batch's size when it first started, and how many of it were asked before this run.
    all: usize,
    before: usize,
    started: Instant,
    /// The flag [`review::run`] stops at: set between two positions once Stop was asked (the
    /// position being searched is finished and kept, never asked again), or at once when it takes
    /// longer than [`STOP_WAIT`], the document was closed or the app quits.
    ///
    /// "Between two positions" is the run's progress, which comes after a position that gave an
    /// evaluation: a position given up on (no line within its budget, or only a bound) gives
    /// none, so Stop is seen at the next evaluated position or, past [`STOP_WAIT`], cuts the
    /// search short. A position cut short was not written; it stays in the batch, and the next
    /// run asks it again. A position written is never asked again.
    halt: Arc<AtomicBool>,
    /// When Stop was asked.
    stop_asked: Option<Instant>,
    /// The document was closed: the batch was told to stop.
    closed: bool,
}

/// The longest Stop waits for the position being searched to be finished (and kept) before the
/// search is cut short (and the position asked again by the next run).
const STOP_WAIT: Duration = Duration::from_secs(2);

impl Drop for Running {
    /// The app closes with the batch running: it stops at once, and its engine is closed before
    /// the app is gone (at most [`crate::chess_writer::SETTLE`]).
    fn drop(&mut self) {
        self.halt.store(true, Ordering::Relaxed);
        self.job.cancel();
        self.job.wait(crate::chess_writer::SETTLE);
    }
}

/// Evaluations handed on together: every write that asks for them shares them, none copies them.
type Chunk = Arc<[(String, Eval)]>;

/// A write of a document's evaluations, for the writer thread: every chunk not known written.
struct EvalsJob {
    dir: PathBuf,
    keys: DocKeys,
    pages: usize,
    evals: Vec<Chunk>,
}

/// Add `chunk` to the chunks not known written (`unwritten`, oldest first), holding at most `cap`
/// evaluations in all: past them the oldest chunks go (the store keeps no more for a document,
/// [`MAX_EVALS`]; the session still shows them). The newest chunk always stays. How many went.
fn hold_unwritten(unwritten: &mut Vec<Chunk>, chunk: Chunk, cap: usize) -> usize {
    unwritten.push(chunk);
    let mut total = unwritten.iter().fold(0usize, |n, c| n.saturating_add(c.len()));
    let mut gone = 0usize;
    while total > cap && unwritten.len() > 1 {
        let oldest = unwritten.remove(0);
        total = total.saturating_sub(oldest.len());
        gone = gone.saturating_add(1);
    }
    gone
}

type EvalsWriter = crate::chess_writer::StoreWriter<EvalsJob, Result<usize, String>>;

/// What is waiting to be kept of one document.
#[derive(Default)]
struct Writes {
    keys: Option<(DocKeys, usize)>,
    /// What may be kept of the document, as of the batch's start and each write while it is
    /// open; `None` (never seen) keeps nothing. A closed document is held to it (plan A9-N17b).
    keep: Option<Keeping>,
    /// Found since the last time they were handed on.
    new: Vec<(String, Eval)>,
    /// Handed to the writer and not known written yet, in the chunks they were handed on in,
    /// oldest first, at most [`MAX_EVALS`] evaluations ([`hold_unwritten`]): each write asks for
    /// all of them (the last request wins, and holds what the ones it replaced held).
    unwritten: Vec<Chunk>,
    /// The writes asked for: their generation, and how many chunks of `unwritten` each held.
    asked: Vec<(u64, usize)>,
    /// When they were last handed on.
    last: Option<Instant>,
    /// A write failed: said once, until one lands.
    failed: bool,
}

/// Chess ▸ Check with the engine…: the dialog, the batch running, what each document's check
/// found and the evaluations waiting to be kept.
#[derive(Default)]
pub struct EngineReview {
    pub(crate) dialog: Option<Dialog>,
    running: Option<Running>,
    docs: HashMap<DocId, Checked>,
    writes: HashMap<DocId, Writes>,
    writer: Option<EvalsWriter>,
    /// The writer's files, and whose.
    files: HashMap<String, DocId>,
    /// The last choice of depth or seconds, for the next dialog.
    last: Option<Limit>,
    /// The engine the next batch drives instead of starting one (tests; a host that starts it).
    engine: Option<Analyser>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl EngineReview {
    /// Drive `engine`, already running, in the next batch instead of starting one (tests; hosts
    /// that start the engine themselves).
    pub fn use_engine(&mut self, engine: Analyser) {
        self.engine = Some(engine);
    }

    /// Whether a batch is running.
    pub fn running(&self) -> bool {
        self.running.is_some()
    }

    /// The divergences of document `doc` found so far, when it was checked in this session.
    pub fn divergences(&self, doc: DocId) -> Option<&[Divergence]> {
        self.docs.get(&doc).map(|c| c.review.divergences.as_slice())
    }

    /// The positions left to ask of `doc`'s stopped batch, when one stopped.
    pub fn left(&self, doc: DocId) -> Option<usize> {
        self.docs.get(&doc).and_then(|c| c.stopped.as_ref()).map(|s| s.batch.len())
    }

    /// The evaluations shown for `doc` (the store's and what the batches found).
    pub fn evals(&self, doc: DocId) -> Option<&EvalFile> {
        self.docs.get(&doc).map(|c| &*c.evals)
    }

    /// Whether the Text tab of `doc` shows the Divergences list.
    pub(crate) fn list_shown(&self, doc: DocId) -> bool {
        self.docs.get(&doc).is_some_and(|c| c.list)
    }

    /// What the Text tab of `doc` shows of the check (plan A9-N17b).
    pub(crate) fn view(&self, doc: DocId) -> CheckView {
        let checked = self.docs.get(&doc);
        CheckView {
            available: true,
            evals: checked.map(|c| Arc::clone(&c.evals)),
            generation: checked.map_or(0, |c| c.generation),
            divergences: checked.map_or(0, |c| c.review.divergences.len().saturating_add(c.review.over)),
            running: self.running.as_ref().filter(|r| r.doc == doc).map(Running::row),
            left: if self.running.is_some() { None } else { checked.and_then(|c| c.stopped.as_ref()).map(|s| s.batch.len()) },
        }
    }
}

impl Running {
    /// The Text tab's row: which position, of how many, and about how long is left.
    fn row(&self) -> LongRow {
        let (done, _) = self.job.progress();
        let done = self.before.saturating_add(done).min(self.all);
        let total = self.all;
        // The time left at the pace so far in this run, once a position is done; else the estimate.
        let this_run = done.saturating_sub(self.before);
        let left = total.saturating_sub(done);
        let secs = if this_run > 0 {
            self.started.elapsed().as_secs_f64() / this_run as f64 * left as f64
        } else {
            self.limit.per_position_ms() as f64 / 1000.0 * left as f64
        };
        let on = done.saturating_add(1).min(total);
        let said = crate::i18n::fmt(
            tl!("Checking with the engine: position {done} of {of} ({time} left)"),
            &[("done", &crate::i18n::number(on)), ("of", &crate::i18n::number(total)), ("time", &duration_words(secs))],
        );
        LongRow { said, done, total }
    }
}

/// The page labels of document `id`, by index.
fn labels_of(app: &PdfCraftApp, id: DocId) -> Vec<String> {
    let pages = app.session.get(id).map_or(0, |d| d.info.pages.len());
    (0..pages).map(|p| app.page_label_in(id, p)).collect()
}

impl PdfCraftApp {
    /// Chess ▸ Check with the engine… (plan A9-N17b): the dialog, for the active document.
    pub fn open_engine_review(&mut self) {
        let Some((_, id)) = self.active_ids() else { return };
        if self.chess.engine_review.running.is_some() {
            return self.inform(tl!("A check with the engine is already running: stop it first.").to_owned());
        }
        if !self.may_copy(id) {
            return;
        }
        let page_count = self.session.get(id).map_or(0, |d| d.info.pages.len());
        let last = self.chess.engine_review.last;
        let (by_depth, depth, seconds) = match last {
            Some(Limit::Seconds(s)) => (false, DEFAULT_DEPTH, s),
            Some(Limit::Depth(d)) => (true, d, DEFAULT_SECONDS),
            None => (true, DEFAULT_DEPTH, DEFAULT_SECONDS),
        };
        self.chess.engine_review.dialog =
            Some(Dialog { doc: id, page_count, pages: Pages::default(), by_depth, depth, seconds, counting: None, counted: None, failed: None });
    }

    /// Count the positions the dialog's choice asks: read its pages off the interface thread (the
    /// diagrams a block of pages at a time, then the text whole), link them to the book's PGN, and
    /// make the batch from the evaluations held, with those found and not written yet.
    fn count_check(&mut self, d: &mut Dialog) {
        let id = d.doc;
        if !self.may_copy(id) {
            return;
        }
        let Some(pages_doc) = self.session.get(id).map(|doc| (doc.doc_keys(), doc.info.pages.len())) else { return };
        let (keys, count) = pages_doc;
        let key = d.key(self.session.chess_stamp(id));
        let pages: Vec<usize> = if key.pages.is_empty() { (0..count).collect() } else { key.pages.clone() };
        let mode = self.chess_origin(id).ocr_mode();
        let book = self.chess_cipher(id);
        let settings = self.chess_scan_settings();
        let refs = self.book_pgn_index(id);
        let depth = key.depth;
        // The evaluations held: read by the job from the folder, or taken here from memory.
        let folder = self.chess.evals.folder();
        let in_memory = match &folder {
            Some(_) => None,
            None => Some(in_store!(self.chess.evals.store.reading(&keys), s => s.load(&keys, count)).map(|l| l.file).map_err(|e| e.to_string())),
        };
        // What the batches found in this session (kept or not, written or on its way), and what
        // was found since it was last handed on: never asked again.
        let session = self.chess.engine_review.docs.get(&id).map(|c| Arc::clone(&c.evals));
        let waiting: Vec<(String, Eval)> = self.chess.engine_review.writes.get(&id).map(|w| w.new.clone()).unwrap_or_default();
        let Some(source) = self.chess_job_source(id) else { return };
        let job = Job::spawn("pdfcraft-chess-engine-count", self.run_inline, move |ctl| {
            let (mut s, sid) = source.open()?;
            ctl.progress(0, pages.len());
            let mut done: usize = 0;
            for block in pages.chunks(BLOCK) {
                // Stopped: the dialog was closed, and what it says now is never seen.
                if ctl.cancelled() {
                    return Err(String::new());
                }
                s.chess_scan(sid, block, &settings, true)?;
                done = done.saturating_add(block.len());
                ctl.progress(done, pages.len());
            }
            let options = OcrOptions::new(mode).with_book(book.as_ref());
            let mut read = s.chess_document_with(sid, &pages, &settings, None, &options)?;
            if let Some(index) = refs.as_deref() {
                reference::link(&mut read.doc, index);
            }
            let (mut evals, unread) = match (folder, in_memory) {
                (Some(dir), _) => match pdfcraft_engine::chess::evals::DirStore::new(&dir).load(&keys, count) {
                    Ok(l) => (l.file, None),
                    Err(e) => (EvalFile::empty(pdfcraft_engine::chess::corrections::DocRecord::new(&keys, count)), Some(e.to_string())),
                },
                (None, Some(Ok(file))) => (file, None),
                (None, Some(Err(e))) => (EvalFile::empty(pdfcraft_engine::chess::corrections::DocRecord::new(&keys, count)), Some(e)),
                (None, None) => (EvalFile::empty(pdfcraft_engine::chess::corrections::DocRecord::new(&keys, count)), None),
            };
            if let Some(found) = session {
                evals.merge(&found);
            }
            for (fen, e) in waiting {
                evals.keep(&fen, e);
            }
            let linked: Vec<&ReferenceIndex> = refs.iter().map(|r| &**r).collect();
            let batch = Batch::new(&read.doc, &linked, &evals, depth);
            let review = review::divergences(&read.doc, &linked, &evals, &Limits::default());
            let cached = s.chess_cached(sid, &[]);
            Ok(Prepared { doc: read.doc, refs, evals, batch, review, cipher: read.cipher, cached, unread })
        });
        d.failed = None;
        d.counted = None;
        d.counting = Some((key, job));
    }

    /// Take a count that ended: what it read is this session's too, and what it found of the
    /// document is the Text tab's (the evaluations beside the moves, the divergences known). A
    /// count of a state of the document that is gone (it was edited meanwhile) is dropped: the
    /// dialog offers to count again. While a batch of the document runs, its check is left as it
    /// is (the dialog is modal and refuses to open then, so this is a guard).
    fn take_count(&mut self, d: &mut Dialog) {
        let Some(out) = d.counting.as_mut().and_then(|(_, job)| job.poll()) else {
            if d.counting.is_some()
                && let Some(ctx) = &self.ctx
            {
                ctx.request_repaint_after(Duration::from_millis(200));
            }
            return;
        };
        let Some((key, _)) = d.counting.take() else { return };
        match out {
            Ok(p) => {
                if key.stamp.is_none() || self.session.chess_stamp(d.doc) != key.stamp {
                    return;
                }
                self.session.prime_chess_cache(d.doc, p.cached.clone());
                self.keep_chess_cipher(d.doc, p.cipher.clone());
                let running = self.chess.engine_review.running.as_ref().is_some_and(|r| r.doc == d.doc);
                if !running {
                    let labels = labels_of(self, d.doc);
                    // A batch that stopped stays, for Continue.
                    let (generation, list, stopped) = match self.chess.engine_review.docs.remove(&d.doc) {
                        Some(c) => (c.generation.wrapping_add(1), c.list, c.stopped),
                        None => (1, false, None),
                    };
                    self.chess.engine_review.docs.insert(
                        d.doc,
                        Checked {
                            doc: Arc::new(p.doc.clone()),
                            refs: p.refs.clone(),
                            evals: Arc::new(p.evals.clone()),
                            generation,
                            review: p.review.clone(),
                            labels,
                            stopped,
                            list,
                        },
                    );
                }
                d.counted = Some((key, Box::new(p)));
            }
            // Stopped (the dialog closed): nothing to say.
            Err(why) if why.is_empty() => {}
            Err(why) => d.failed = Some(why),
        }
    }

    /// Start `batch` (the dialog counted it, or it is document `id`'s that stopped). When it can't
    /// start (a batch runs already, the document's check is gone, no engine, or the engine does
    /// not start), why is said and the batch comes back, for the caller to keep: Start and
    /// Continue lose nothing (plan A9-N17b, the review of #420).
    fn start_check(&mut self, id: DocId, batch: Batch, limit: Limit, all: usize, before: usize) -> Result<(), Batch> {
        if self.chess.engine_review.running.is_some() {
            self.inform(tl!("A check with the engine is already running: stop it first.").to_owned());
            return Err(batch);
        }
        let Some(checked) = self.chess.engine_review.docs.get(&id) else { return Err(batch) };
        let (doc, refs, evals) = (Arc::clone(&checked.doc), checked.refs.clone(), (*checked.evals).clone());
        let analyser = match self.chess.engine_review.engine.take() {
            Some(a) => a,
            None => {
                let install = pdfcraft_platform::app_dirs::AppDirs::current().install();
                let Some(path) = self.chess.study.engine_path.clone().or_else(|| pdfcraft_engine::chess::analysis::find_engine(install)) else {
                    // The board's assistant says where to get one and lets it be chosen.
                    self.chess.study.assistant = true;
                    return Err(batch);
                };
                let options = AnalysisOptions { lines: 1, depth: limit.budget().depth };
                match Analyser::start(&path, options, Arc::new(|| {})) {
                    Ok(a) => a,
                    Err(e) => {
                        self.post_error(engine_error(&e));
                        return Err(batch);
                    }
                }
            }
        };
        analyser.set_settings(self.chess.study.settings.clone());
        if let Some(c) = self.chess.engine_review.docs.get_mut(&id) {
            c.stopped = None;
        }
        let keys = self.session.get(id).map(|d| (d.doc_keys(), d.info.pages.len()));
        let keep = self.chess_keeping(id);
        let w = self.chess.engine_review.writes.entry(id).or_default();
        w.keys = keys;
        w.keep = Some(keep);
        w.last = Some(Instant::now());
        let name = self.session.get(id).map(|d| d.name.clone()).unwrap_or_default();
        let found: Arc<Mutex<Vec<(String, Eval)>>> = Arc::default();
        let handed = Arc::clone(&found);
        let budget = limit.budget();
        let halt = Arc::new(AtomicBool::new(false));
        let halted = Arc::clone(&halt);
        // Never inline: the batch takes as long as the engine does (plan A9-N17b).
        let shared = Arc::new(Mutex::new(batch));
        let held = Arc::clone(&shared);
        let job = Job::spawn("pdfcraft-chess-engine-check", false, move |ctl| {
            let mut evals = evals;
            let mut batch = lock(&held);
            ctl.progress(0, batch.len());
            let ran = review::run(&analyser, &mut batch, &budget, &halted, |p| {
                ctl.progress(p.done, p.total);
                for (fen, e) in &p.evals {
                    evals.keep(fen, e.clone());
                }
                lock(&handed).extend(p.evals);
                // Stop asked: the batch ends here, between two positions.
                if ctl.cancelled() {
                    halted.store(true, Ordering::Relaxed);
                }
            });
            drop(batch);
            // The engine is closed before the report is made.
            drop(analyser);
            let linked: Vec<&ReferenceIndex> = refs.iter().map(|r| &**r).collect();
            let review = review::divergences(&doc, &linked, &evals, &Limits::default());
            Ok(Ended { ran, review })
        });
        self.chess.engine_review.last = Some(limit);
        self.chess.engine_review.running = Some(Running {
            doc: id,
            name,
            job,
            found,
            batch: shared,
            limit,
            all,
            before,
            started: Instant::now(),
            halt,
            stop_asked: None,
            closed: false,
        });
        Ok(())
    }

    /// Continue document `id`'s stopped batch: only the positions it had not asked. When it can't
    /// start, the batch stays, and Continue with it.
    pub(crate) fn continue_check(&mut self, id: DocId) {
        let Some(s) = self.chess.engine_review.docs.get_mut(&id).and_then(|c| c.stopped.take()) else { return };
        let Stopped { batch, limit, all, asked } = s;
        if let Err(batch) = self.start_check(id, batch, limit, all, asked)
            && let Some(c) = self.chess.engine_review.docs.get_mut(&id)
        {
            c.stopped = Some(Stopped { batch, limit, all, asked });
        }
    }

    /// Stop the batch running: it ends once the position being searched is finished, kept (at
    /// most [`STOP_WAIT`]; past it the search is cut short).
    pub(crate) fn stop_check(&mut self) {
        if let Some(r) = &mut self.chess.engine_review.running {
            r.job.cancel();
            r.stop_asked.get_or_insert_with(Instant::now);
        }
    }

    /// Each frame: the dialog's count, the batch's progress and end, and the writes.
    pub(crate) fn poll_engine_review(&mut self) {
        if let Some(mut d) = self.chess.engine_review.dialog.take() {
            self.take_count(&mut d);
            // Put back unless the dialog was answered meanwhile.
            if self.chess.engine_review.dialog.is_none() {
                self.chess.engine_review.dialog = Some(d);
            }
        }
        self.poll_running();
        self.poll_writes();
        // A closed document's check goes once nothing of it waits to be written.
        let session = &self.session;
        let review = &mut self.chess.engine_review;
        let running = review.running.as_ref().map(|r| r.doc);
        review.docs.retain(|id, _| session.get(*id).is_some() || running == Some(*id));
        review.writes.retain(|id, w| session.get(*id).is_some() || running == Some(*id) || !(w.unwritten.is_empty() && w.new.is_empty()));
    }

    fn poll_running(&mut self) {
        let Some(run) = self.chess.engine_review.running.as_mut() else { return };
        let doc = run.doc;
        if !run.closed && self.session.get(doc).is_none() {
            run.closed = true;
            run.job.cancel();
            run.halt.store(true, Ordering::Relaxed);
        }
        if run.stop_asked.is_some_and(|t| t.elapsed() >= STOP_WAIT) {
            run.halt.store(true, Ordering::Relaxed);
        }
        let found = std::mem::take(&mut *lock(&run.found));
        let out = run.job.poll();
        self.take_found(doc, found);
        let Some(out) = out else {
            self.flush_check(doc, false);
            if let Some(ctx) = &self.ctx {
                ctx.request_repaint_after(Duration::from_millis(200));
            }
            return;
        };
        let Some(run) = self.chess.engine_review.running.take() else { return };
        let rest = std::mem::take(&mut *lock(&run.found));
        // The job is done: the batch is free, as the job left it (a panic in it too).
        let batch = std::mem::take(&mut *lock(&run.batch));
        let (name, limit, all, closed) = (run.name.clone(), run.limit, run.all, run.closed);
        // Ended: dropping it waits for nothing.
        drop(run);
        self.take_found(doc, rest);
        self.flush_check(doc, true);
        let done = all.saturating_sub(batch.len());
        let left = batch.len();
        let ended = match out {
            Ok(e) => e,
            // The job panicked: what it had not asked stays, for Continue.
            Err(why) => {
                if let Some(c) = self.chess.engine_review.docs.get_mut(&doc) {
                    c.stopped = (left > 0).then_some(Stopped { batch, limit, all, asked: done });
                }
                return self.post_error(why);
            }
        };
        if let Some(c) = self.chess.engine_review.docs.get_mut(&doc) {
            c.review = ended.review;
            c.generation = c.generation.wrapping_add(1);
            c.stopped = (left > 0).then_some(Stopped { batch, limit, all, asked: done });
        }
        let n = |v: usize| crate::i18n::number(v);
        let divergences = self.chess.engine_review.docs.get(&doc).map_or(0, |c| c.review.divergences.len());
        if closed {
            return self.inform(crate::i18n::fmt(
                tl!("The check of {name} stopped: the document was closed. What the engine found is kept."),
                &[("name", &name)],
            ));
        }
        match ended.ran {
            Ran::Done { skipped } => {
                let mut said =
                    crate::i18n::fmt(tl!("The check is done. Positions checked: {n}. Divergences: {d}."), &[("n", &n(done)), ("d", &n(divergences))]);
                if skipped > 0 {
                    said.push(' ');
                    said.push_str(&crate::i18n::fmt(tl!("Positions the engine gave no answer for: {n}"), &[("n", &n(skipped))]));
                }
                self.inform(said);
            }
            Ran::Stopped => self.inform(crate::i18n::fmt(
                tl!("The check was stopped: {done} of {of} positions checked. What was found is kept."),
                &[("done", &n(done)), ("of", &n(all))],
            )),
            Ran::Failed(Failure::Engine(e)) => self.post_error(crate::i18n::fmt(
                tl!("{why} What the engine found is kept: Continue the check goes on from there."),
                &[("why", &engine_error(&e))],
            )),
            Ran::Failed(Failure::Silent { positions }) => self.post_error(crate::i18n::fmt(
                tl!("The engine gave no answer for {n} positions in a row, so the check stopped. What it found is kept."),
                &[("n", &n(positions))],
            )),
        }
    }

    /// Evaluations the batch found for document `id`: waiting to be kept, in batches.
    fn take_found(&mut self, id: DocId, found: Vec<(String, Eval)>) {
        if found.is_empty() {
            return;
        }
        self.chess.engine_review.writes.entry(id).or_default().new.extend(found);
    }

    /// Hand document `id`'s evaluations found to the store when [`WRITE_POSITIONS`] of them wait
    /// or [`WRITE_EVERY`] passed since the last time, or `now`: into the Text tab's evaluations,
    /// and to the writer thread (a folder) or the session's memory. A document whose chess may not
    /// be written keeps them for the session only.
    fn flush_check(&mut self, id: DocId, now: bool) {
        let Some(w) = self.chess.engine_review.writes.get_mut(&id) else { return };
        let due = now || w.new.len() >= WRITE_POSITIONS || w.last.is_none_or(|t| t.elapsed() >= WRITE_EVERY);
        if !due || w.new.is_empty() {
            return;
        }
        w.last = Some(Instant::now());
        let new = std::mem::take(&mut w.new);
        if let Some(c) = self.chess.engine_review.docs.get_mut(&id) {
            let evals = Arc::make_mut(&mut c.evals);
            for (fen, e) in &new {
                evals.keep(fen, e.clone());
            }
            c.generation = c.generation.wrapping_add(1);
        }
        // Never asked about for an evaluation, as the board's (plan A9-N8). While the document is
        // open its rule now holds (an answer given meanwhile counts); once it is closed, the last
        // one seen: a document whose chess was not kept never has it written after it closes.
        let keeping = if self.session.get(id).is_some() { Some(self.chess_keeping(id)) } else { None };
        // The document's keys, under which its evaluations are written: an edit does not change
        // them (its first /ID, its path, its name and the size of the file as last opened or
        // saved), a save does (the size; Save As the path and the name). Taken again at each write
        // while it is open, so a save mid-batch writes under the keys it has now; once it is
        // closed, the last ones.
        let now = self.session.get(id).map(|d| (d.doc_keys(), d.info.pages.len()));
        let Some(w) = self.chess.engine_review.writes.get_mut(&id) else { return };
        if keeping.is_some() {
            w.keep = keeping;
        }
        if now.is_some() {
            w.keys = now;
        }
        if w.keep != Some(Keeping::Yes) {
            return;
        }
        let Some((keys, pages)) = w.keys.clone() else { return };
        match self.chess.evals.folder() {
            Some(dir) => {
                let review = &mut self.chess.engine_review;
                let Some(w) = review.writes.get_mut(&id) else { return };
                let gone = hold_unwritten(&mut w.unwritten, Arc::from(new), MAX_EVALS);
                for (_, n) in &mut w.asked {
                    *n = n.saturating_sub(gone);
                }
                let Some(file) = pdfcraft_engine::chess::store::file_name(&keys, <EvalsKind as pdfcraft_engine::chess::store::StoreKind>::KIND)
                else {
                    return;
                };
                let job = EvalsJob { dir, keys, pages, evals: w.unwritten.clone() };
                let held = w.unwritten.len();
                let writer = review.writer.get_or_insert_with(|| {
                    crate::chess_writer::StoreWriter::new(
                        "chess-evals-writer",
                        |_file: &str, job: EvalsJob| {
                            // One list, made here on the writer's thread.
                            let evals: Vec<(String, Eval)> = job.evals.iter().flat_map(|c| c.iter().cloned()).collect();
                            pdfcraft_engine::chess::evals::DirStore::new(&job.dir).keep(&job.keys, job.pages, &evals).map_err(|e| e.to_string())
                        },
                        |panic| Err(format!("the writer stopped: {panic}")),
                    )
                });
                let generation = writer.submit(&file, job);
                review.files.insert(file, id);
                if let Some(w) = review.writes.get_mut(&id) {
                    w.asked.push((generation, held));
                }
            }
            // Memory (a session with no data folder): kept at once.
            None => {
                let kept = self
                    .chess
                    .evals
                    .store
                    .change(true, &keys, pages, |place| in_store!(place, s => s.keep(&keys, pages, &new)).map_err(|e| e.to_string()));
                if let Err(why) = kept {
                    self.say_evals_not_kept(id, &why);
                }
            }
        }
    }

    /// What became of the writes: a write that landed takes what it held off the ones waiting; one
    /// that failed is said (once, until one lands), and what it held is asked again with the next.
    fn poll_writes(&mut self) {
        let Some(writer) = &self.chess.engine_review.writer else { return };
        for done in writer.take_done() {
            let Some(id) = self.chess.engine_review.files.get(&done.file).copied() else { continue };
            let Some(w) = self.chess.engine_review.writes.get_mut(&id) else { continue };
            let held = w.asked.iter().find(|(g, _)| *g == done.generation).map(|(_, n)| *n);
            w.asked.retain(|(g, _)| *g > done.generation);
            match done.result {
                Ok(_) => {
                    let held = held.unwrap_or(0).min(w.unwritten.len());
                    w.unwritten.drain(..held);
                    for (_, n) in &mut w.asked {
                        *n = n.saturating_sub(held);
                    }
                    w.failed = false;
                }
                Err(why) => self.say_evals_not_kept(id, &why),
            }
        }
    }

    fn say_evals_not_kept(&mut self, id: DocId, why: &str) {
        let Some(w) = self.chess.engine_review.writes.get_mut(&id) else { return };
        if std::mem::replace(&mut w.failed, true) {
            return;
        }
        self.post_error(crate::i18n::fmt(tl!("The engine's evaluations could not be kept: {why}"), &[("why", why)]));
    }

    /// Quit, before the writes are waited for (`shutdown_recovery`): the batch running stops now,
    /// its search cut short, its job (and so its engine) waited for at most
    /// [`crate::chess_writer::SETTLE`], and what it found handed to the store, so that the writes
    /// waited for next hold all of it. Halt, wait, take, flush; the settle follows.
    pub(crate) fn quit_engine_review(&mut self) {
        if let Some(run) = &self.chess.engine_review.running {
            run.halt.store(true, Ordering::Relaxed);
            run.job.cancel();
            run.job.wait(crate::chess_writer::SETTLE);
        }
        // Ended: taken as any end is (what it found written now, its stopped batch kept).
        self.poll_running();
        // Still running past the wait: what it found so far.
        if let Some(run) = &self.chess.engine_review.running {
            let (doc, found) = (run.doc, std::mem::take(&mut *lock(&run.found)));
            self.take_found(doc, found);
        }
        let ids: Vec<DocId> = self.chess.engine_review.writes.keys().copied().collect();
        for id in ids {
            self.flush_check(id, true);
        }
    }

    /// Wait at most `timeout` for the evaluations being written (Close and Quit).
    pub(crate) fn settle_engine_review(&mut self, timeout: Duration) {
        if let Some(run) = &self.chess.engine_review.running {
            let (doc, found) = (run.doc, std::mem::take(&mut *lock(&run.found)));
            self.take_found(doc, found);
        }
        let ids: Vec<DocId> = self.chess.engine_review.writes.keys().copied().collect();
        for id in ids {
            self.flush_check(id, true);
        }
        if let Some(w) = &self.chess.engine_review.writer {
            w.settle(timeout);
        }
        self.poll_writes();
    }

    /// The Text tab's Show in PDF on divergence `i` of the active document: its page, the move or
    /// the diagram outlined.
    pub(crate) fn show_divergence(&mut self, i: usize) {
        let Some((vi, id)) = self.active_ids() else { return };
        let Some(c) = self.chess.engine_review.docs.get(&id) else { return };
        let Some(d) = c.review.divergences.get(i) else { return };
        let Ok(page) = usize::try_from(d.page) else { return };
        let rect = match d.at {
            Place::Move(at) => at.find(&c.doc).and_then(|m| m.source_rect),
            Place::Diagram(at) => c.doc.pages.iter().filter(|p| p.index == at.page_index()).flat_map(|p| &p.blocks).find_map(|b| match b {
                Block::Diagram(dg) if dg.number == at.index => dg.source_rect,
                _ => None,
            }),
        };
        let rect = rect.map(|r| r.page_rect()).map(|r| [r.x0, r.y0, r.x1, r.y1]).filter(|r| r.iter().all(|v| v.is_finite()));
        if let Some(view) = self.views.get_mut(vi) {
            view.go_to_page(page);
            if let Some(rect) = rect {
                view.flash = Some((page, rect, 0.0));
            }
        }
    }

    /// The Text tab shows (`true`) or leaves the Divergences list of the active document.
    pub(crate) fn show_divergences(&mut self, shown: bool) {
        let Some((_, id)) = self.active_ids() else { return };
        match self.chess.engine_review.docs.get_mut(&id) {
            Some(c) => c.list = shown,
            None if shown => {
                // Nothing checked yet in this session: the list says so, with the way to check.
                self.chess.engine_review.docs.insert(
                    id,
                    Checked {
                        doc: Arc::new(ChessDoc {
                            format: pdfcraft_engine::chess::pdfcraft_chessvision::doc::FORMAT_VERSION,
                            pages: Vec::new(),
                            games: Vec::new(),
                        }),
                        refs: None,
                        evals: Arc::new(EvalFile::empty(pdfcraft_engine::chess::corrections::DocRecord::default())),
                        generation: 0,
                        review: Review::default(),
                        labels: Vec::new(),
                        stopped: None,
                        list: true,
                    },
                );
            }
            None => {}
        }
    }
}

/// An engine's failure in words, as the board says it.
fn engine_error(e: &AnalysisError) -> String {
    match e {
        AnalysisError::Start(why) => crate::i18n::fmt(tl!("The engine couldn't be started: {e}"), &[("e", why)]),
        AnalysisError::NotUci => tl!("That program didn't answer as a UCI chess engine.").to_owned(),
        AnalysisError::Stopped => tl!("The engine stopped.").to_owned(),
    }
}

/// What the book says, as a reader of chess writes it: the sign, or the goal's (`+−` to win, `=`
/// to draw, `#2` to mate in two; `h#`, `s#` for help- and selfmates).
fn book_said(d: &Divergence) -> String {
    use pdfcraft_engine::chess::pdfcraft_chess::placement::Color;
    use pdfcraft_engine::chess::pdfcraft_chess::stipulation::Goal;
    match d.book {
        Said::Sign(nag) => {
            let sign = pdfcraft_engine::chess::pdfcraft_chess::nag::shown(nag);
            match crate::chess_annotate::nag_name(nag) {
                Some(name) => format!("{sign} ({name})"),
                None => sign,
            }
        }
        Said::Goal { goal, side } => match goal {
            Goal::Win if side == Color::White => "+\u{2212}".to_owned(),
            Goal::Win => "\u{2212}+".to_owned(),
            Goal::Draw => "=".to_owned(),
            Goal::Mate(n) => format!("#{n}"),
            Goal::HelpMate(n) => format!("h#{n}"),
            Goal::SelfMate(n) => format!("s#{n}"),
        },
    }
}

/// Pawns as a book writes them: `2.00`.
fn pawns(cp: i32) -> String {
    format!("{:.2}", f64::from(cp.unsigned_abs()) / 100.0)
}

/// What the calculation says, in the interface's language: the engine's value for White and what
/// it found, its best move or second line, the depth and the engine; or the solver's word.
fn computed_said(d: &Divergence) -> String {
    let v = |value: Value| review::value_text(value);
    match &d.computed {
        Computed::Solver(s) => {
            let n = s.n.to_string();
            match (d.found, &s.best_try, &s.refutation) {
                (Found::ManyKeys, _, _) => crate::i18n::fmt(
                    tl!("Mate in {n}: {keys} ({k} keys)"),
                    &[("n", &n), ("keys", &s.keys.join(", ")), ("k", &s.keys.len().to_string())],
                ),
                (_, Some(best), Some(reply)) => crate::i18n::fmt(
                    tl!("Mate in {n}: none found; best try {best}, refuted by {reply}"),
                    &[("n", &n), ("best", best), ("reply", reply)],
                ),
                _ => crate::i18n::fmt(tl!("Mate in {n}: none found"), &[("n", &n)]),
            }
        }
        Computed::Engine(e) => {
            let value = v(e.value);
            let best = e.best.as_ref().map(|(san, value)| format!("{san} {}", v(*value)));
            let mut out = match (d.found, best) {
                (Found::Refuted { loss: None }, _) => crate::i18n::fmt(tl!("{value}: lets a mate go"), &[("value", &value)]),
                (Found::Refuted { loss: Some(l) }, Some(best)) => {
                    crate::i18n::fmt(tl!("{value}: loses {pawns} pawns to {best}"), &[("value", &value), ("pawns", &pawns(l)), ("best", &best)])
                }
                (Found::Refuted { loss: Some(l) }, None) => {
                    crate::i18n::fmt(tl!("{value}: loses {pawns} pawns"), &[("value", &value), ("pawns", &pawns(l))])
                }
                (Found::Questioned { loss }, _) if loss > 0 => {
                    crate::i18n::fmt(tl!("{value}: loses only {pawns} pawns"), &[("value", &value), ("pawns", &pawns(loss))])
                }
                (Found::Questioned { .. }, _) => crate::i18n::fmt(tl!("{value}: the engine's best move"), &[("value", &value)]),
                (Found::Contradicted { band } | Found::Goal { band }, _) => format!("{value} ({})", band.sign()),
                (Found::OtherSolution, _) => match &e.second {
                    Some((san, second)) => crate::i18n::fmt(tl!("another solution: {move} {value}"), &[("move", san), ("value", &v(*second))]),
                    None => value,
                },
                _ => value,
            };
            out.push_str(" · ");
            out.push_str(&crate::i18n::fmt(tl!("depth {n}"), &[("n", &e.depth.to_string())]));
            if let Some(name) = &e.engine {
                out.push_str(" · ");
                out.push_str(name);
            }
            out
        }
    }
}

/// Who computed it, in a word.
fn source_word(s: Source) -> &'static str {
    match s {
        Source::Solver => tl!("Solver"),
        Source::Engine | Source::ChessBase => tl!("Engine"),
    }
}

/// The height of one row of the Divergences list: three lines and its margins.
const ROW: f32 = 66.0;

/// The Divergences list, in the Text tab's page text's place (plan A9-N17b): "‹ Text" (and Esc)
/// back, the batch's row with Stop while it runs, what was not judged, then a row per divergence
/// (the page as the book numbers it and the move or diagram, what the book says, what the
/// calculation says and who computed it) with Show in PDF, which opens the page and outlines it.
pub(crate) fn list(ui: &mut egui::Ui, t: &Tokens, review: &EngineReview, id: DocId, running: Option<&LongRow>) -> Option<TextAction> {
    let mut action = None;
    if let Some(row) = running
        && crate::chess_panel_head::long_job(ui, t, row)
    {
        action = Some(TextAction::StopCheck);
    }
    let checked = review.docs.get(&id);
    let count = checked.map_or(0, |c| c.review.divergences.len().saturating_add(c.review.over));
    let area = ui.max_rect();
    ui.horizontal(|ui| {
        if ui.button(tl!("‹ Text")).on_hover_text(tl!("Back to the page's text (Esc)")).clicked() {
            action = Some(TextAction::Divergences(false));
        }
        let title = crate::i18n::fmt(tl!("Divergences ({n})"), &[("n", &crate::i18n::number(count))]);
        ui.add(egui::Label::new(RichText::new(title).font(theme::semibold(13.0))).truncate());
    });
    // Esc goes back while the keyboard is in the list or the pointer is over it; never ahead of
    // a menu's or a dialog's own Esc.
    let ctx = ui.ctx().clone();
    let focus_here = crate::widgets::focused_response(&ctx).is_some_and(|r| area.contains_rect(r.rect));
    let pointer_here = ctx.pointer_hover_pos().is_some_and(|p| area.contains(p));
    let covered = egui::Popup::is_any_open(&ctx) || ctx.memory(|m| m.top_modal_layer().is_some());
    if (focus_here || pointer_here) && !covered && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        action = Some(TextAction::Divergences(false));
    }
    let Some(c) = checked.filter(|c| !c.labels.is_empty() || !c.review.divergences.is_empty()) else {
        ui.add_space(6.0);
        ui.label(RichText::new(tl!("No divergence in what was checked.")).color(t.text_muted));
        if ui.button(tl!("Check with the engine…")).clicked() {
            action = Some(TextAction::CheckWithEngine);
        }
        return action;
    };
    let r = &c.review;
    if r.no_eval > 0 || r.shallow > 0 {
        let said = crate::i18n::fmt(
            tl!("Not judged: {none} with no evaluation, {shallow} searched too shallow."),
            &[("none", &crate::i18n::number(r.no_eval)), ("shallow", &crate::i18n::number(r.shallow))],
        );
        ui.label(RichText::new(said).small().color(t.text_muted));
    }
    if r.divergences.is_empty() {
        ui.add_space(6.0);
        ui.label(RichText::new(tl!("No divergence in what was checked.")).color(t.text_muted));
        return action;
    }
    ui.separator();
    egui::ScrollArea::vertical().id_salt("chess-divergences").auto_shrink([false, false]).show_rows(ui, ROW, r.divergences.len(), |ui, rows| {
        for i in rows {
            let Some(d) = r.divergences.get(i) else { continue };
            if let Some(a) = row(ui, t, c, d, i) {
                action = Some(a);
            }
        }
    });
    action
}

/// One row of the list: its three lines, its source and Show in PDF.
fn row(ui: &mut egui::Ui, t: &Tokens, c: &Checked, d: &Divergence, i: usize) -> Option<TextAction> {
    let mut action = None;
    let page =
        c.labels.get(usize::try_from(d.page).unwrap_or(usize::MAX)).cloned().unwrap_or_else(|| u64::from(d.page).saturating_add(1).to_string());
    let printed = match d.at {
        Place::Move(_) => d.printed.clone(),
        Place::Diagram(at) => crate::i18n::fmt(tl!("Diagram {n}"), &[("n", &at.index.saturating_add(1).to_string())]),
    };
    let head = crate::i18n::fmt(tl!("p. {page} · {name}"), &[("page", &page), ("name", &printed)]);
    let book = crate::i18n::fmt(tl!("The book says: {what}"), &[("what", &book_said(d))]);
    let computed = crate::i18n::fmt(tl!("The calculation says: {what}"), &[("what", &computed_said(d))]);
    let source = source_word(d.source);
    ui.push_id(("chess-divergence", i), |ui| {
        ui.allocate_ui(egui::vec2(ui.available_width(), ROW), |ui| {
            ui.set_min_height(ROW - 4.0);
            ui.spacing_mut().item_spacing.y = 2.0;
            // The first line: Show in PDF at its end, the page and the move, and the source; the
            // book's and the calculation's words under it, the column's whole width each.
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let show = crate::widgets::pill_button(ui, tl!("Show in PDF"), false);
                    let name = format!("{}, {head}, {book}, {computed}, {source}", tl!("Show in PDF"));
                    show.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
                    if show.clicked() {
                        action = Some(TextAction::ShowDivergence(i));
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add(egui::Label::new(RichText::new(&head).strong()).truncate());
                        ui.label(RichText::new(source).small().color(t.text_muted));
                    });
                });
            });
            ui.add(egui::Label::new(RichText::new(&book).small()).truncate()).on_hover_text(&book);
            ui.add(egui::Label::new(RichText::new(&computed).small()).truncate()).on_hover_text(&computed);
        });
    });
    action
}

/// The dialog, while it is shown: the pages, how each position is searched, the count and its
/// estimate, Start the check, Continue and Cancel.
pub(crate) fn prompt(app: &mut PdfCraftApp, ctx: &egui::Context) {
    let Some(mut d) = app.chess.engine_review.dialog.take() else { return };
    // Its document closed: the dialog goes with it (a count running stops).
    if app.session.get(d.doc).is_none() {
        return;
    }
    let t = Tokens::get(ctx);
    let stamp = app.session.chess_stamp(d.doc);
    let left = app.chess.engine_review.left(d.doc);
    let mut answer: Option<Answer> = None;
    let shown = crate::widgets::show_titled_modal(ctx, "chess-engine-check", tl!("Check with the engine"), |ui| {
        ui.set_max_width(520.0);
        ui.spacing_mut().item_spacing.y = 8.0;
        fields(ui, &mut d);
        if !d.by_depth {
            ui.label(RichText::new(time_note()).small().color(t.text_muted));
        }
        ui.separator();
        let key = d.key(stamp);
        let counted = d.counted.as_ref().filter(|(k, _)| *k == key).map(|(_, p)| p);
        match (&d.counting, counted) {
            (Some((_, job)), _) => {
                let (done, total) = job.progress();
                let on = done.saturating_add(1).min(total);
                let said = crate::i18n::fmt(
                    tl!("Reading the pages: page {done} of {of}"),
                    &[("done", &crate::i18n::number(on)), ("of", &crate::i18n::number(total))],
                );
                ui.label(RichText::new(said).small().color(t.text_muted));
                let fraction = if total == 0 { 0.0 } else { (done.min(total) as f32 / total as f32).clamp(0.0, 1.0) };
                crate::widgets::progress(ui, fraction, tl_ctx!("progress bar", "Progress"));
            }
            (None, Some(p)) => summary(ui, &t, p, d.limit()),
            (None, None) => {
                let count = crate::widgets::pill_button(ui, tl!("Count the positions"), true);
                crate::widgets::dialog_focus(&count);
                if count.clicked() {
                    answer = Some(Answer::Count);
                }
            }
        }
        if let Some(why) = &d.failed {
            crate::notice::callout(ui, crate::notice::Tone::Warning, why);
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let ready = counted.is_some_and(|p| !p.batch.is_empty());
            let start = ui.add_enabled_ui(ready, |ui| crate::widgets::pill_button(ui, tl!("Start the check"), true)).inner;
            if ready {
                crate::widgets::dialog_focus(&start);
            }
            if start.clicked() {
                answer = Some(Answer::Start);
            }
            if let Some(n) = left {
                let label = crate::i18n::fmt(tl!("Continue the check ({n} left)"), &[("n", &crate::i18n::number(n))]);
                if crate::widgets::pill_button(ui, &label, false).clicked() {
                    answer = Some(Answer::Continue);
                }
            }
            if crate::widgets::pill_button(ui, tl!("Cancel"), false).clicked() {
                answer = Some(Answer::Cancel);
            }
        });
    });
    if answer.is_none() && shown.should_close() {
        answer = Some(Answer::Cancel);
    }
    let doc = d.doc;
    match answer {
        None => app.chess.engine_review.dialog = Some(d),
        Some(Answer::Count) => {
            app.count_check(&mut d);
            app.chess.engine_review.dialog = Some(d);
        }
        // Closed: a count running is dropped, which stops it.
        Some(Answer::Cancel) => {}
        Some(Answer::Continue) => app.continue_check(doc),
        // When it can't start, the dialog stays with its count, to start again.
        Some(Answer::Start) => {
            let limit = d.limit();
            if let Some((key, mut p)) = d.counted.take() {
                let batch = std::mem::take(&mut p.batch);
                let all = batch.len();
                if let Err(batch) = app.start_check(doc, batch, limit, all, 0) {
                    p.batch = batch;
                    d.counted = Some((key, p));
                    app.chess.engine_review.dialog = Some(d);
                }
            }
        }
    }
}

/// What the dialog was answered with.
enum Answer {
    Count,
    Start,
    Continue,
    Cancel,
}

/// The dialog's fields: the pages, and how each position is searched.
fn fields(ui: &mut egui::Ui, d: &mut Dialog) {
    egui::Grid::new("chess-engine-check-fields").num_columns(2).spacing(egui::vec2(12.0, 8.0)).show(ui, |ui| {
        ui.label(tl!("Pages"));
        ui.scope(|ui| {
            crate::widgets::radio_group(ui, tl!("Pages"));
            ui.horizontal(|ui| {
                crate::widgets::radio_value(ui, &mut d.pages.all, true, tl!("All pages"));
                let from = crate::widgets::radio_value(ui, &mut d.pages.all, false, tl!("Pages from"));
                let last = d.page_count.max(1);
                d.pages.from = d.pages.from.clamp(1, last);
                d.pages.to = d.pages.to.clamp(d.pages.from, last);
                ui.add_enabled(!d.pages.all, egui::DragValue::new(&mut d.pages.from).range(1..=last)).labelled_by(from.id);
                let to = ui.label(tl!("to"));
                ui.add_enabled(!d.pages.all, egui::DragValue::new(&mut d.pages.to).range(1..=last)).labelled_by(to.id);
                d.pages.to = d.pages.to.max(d.pages.from);
            });
        });
        ui.end_row();
        ui.label(tl!("Search each position"));
        ui.scope(|ui| {
            crate::widgets::radio_group(ui, tl!("Search each position"));
            ui.horizontal(|ui| {
                let depth = crate::widgets::radio_value(ui, &mut d.by_depth, true, tl!("To depth"));
                ui.add_enabled(d.by_depth, egui::DragValue::new(&mut d.depth).range(1..=MAX_DEPTH)).labelled_by(depth.id);
                ui.add_space(8.0);
                let secs = crate::widgets::radio_value(ui, &mut d.by_depth, false, tl!("Seconds each"));
                ui.add_enabled(!d.by_depth, egui::DragValue::new(&mut d.seconds).range(MIN_SECONDS..=MAX_SECONDS).speed(0.1).max_decimals(1))
                    .labelled_by(secs.id);
            });
        });
        ui.end_row();
    });
}

/// What searching for a time means for the report: a position may stay shallower than the depth
/// it judges at ([`Limits::min_depth`]), and is then asked again only by a check to a depth.
fn time_note() -> String {
    let d = Limits::default().min_depth.to_string();
    crate::i18n::fmt(
        tl!(
            "Searched for a time, a position may stay shallower than depth {d}, which the report does not judge; a check to depth {d} asks it again."
        ),
        &[("d", &d)],
    )
}

/// What the count found: the positions to ask and about how long, those held left out, and the
/// cap of the evaluations when the book passes it.
fn summary(ui: &mut egui::Ui, t: &Tokens, p: &Prepared, limit: Limit) {
    let n = p.batch.len();
    if n == 0 {
        ui.label(tl!("Nothing to check: the evaluations hold every position at this depth."));
    } else {
        let said = crate::i18n::fmt(tl!("Positions to check: {n} ({time})"), &[("n", &crate::i18n::number(n)), ("time", &estimate_words(n, limit))]);
        ui.label(RichText::new(said).strong());
    }
    if p.batch.kept > 0 {
        let said = crate::i18n::fmt(tl!("Checked before at this depth, left out: {n}"), &[("n", &crate::i18n::number(p.batch.kept))]);
        ui.label(RichText::new(said).small().color(t.text_muted));
    }
    if p.batch.capped {
        let said = crate::i18n::fmt(
            tl!(
                "The pages hold more positions than the {max} the evaluations keep for a document: only the first {max} are checked, and older evaluations make room."
            ),
            &[("max", &crate::i18n::number(MAX_EVALS))],
        );
        crate::notice::callout(ui, crate::notice::Tone::Warning, &said);
    }
    if let Some(why) = &p.unread {
        crate::notice::callout(
            ui,
            crate::notice::Tone::Warning,
            &crate::i18n::fmt(tl!("The engine's evaluations could not be kept: {why}"), &[("why", why)]),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plan A9-N17b (the review of #420): what waits to be written holds at most its cap of
    /// evaluations, the oldest chunks going first and the newest always staying; the chunks are
    /// shared, not copied, by the writes that ask for them.
    #[test]
    fn what_waits_to_be_written_is_capped_and_shared() {
        let chunk = |n: usize| -> Chunk {
            use pdfcraft_engine::chess::pdfcraft_chess::uci::{Bound, Score};
            let e = Eval::new(20, Score::Centipawns(10), Bound::Exact, &[], None, 1).unwrap();
            (0..n).map(|i| (format!("p{i}"), e.clone())).collect::<Vec<_>>().into()
        };
        let mut unwritten: Vec<Chunk> = Vec::new();
        assert_eq!(hold_unwritten(&mut unwritten, chunk(4), 10), 0);
        assert_eq!(hold_unwritten(&mut unwritten, chunk(4), 10), 0);
        let newest = chunk(4);
        assert_eq!(hold_unwritten(&mut unwritten, Arc::clone(&newest), 10), 1, "the oldest went");
        assert_eq!(unwritten.iter().map(|c| c.len()).sum::<usize>(), 8);
        assert!(unwritten.last().is_some_and(|c| Arc::ptr_eq(c, &newest)), "shared, not copied");
        // A chunk bigger than the cap stays alone.
        assert_eq!(hold_unwritten(&mut unwritten, chunk(12), 10), 2);
        assert_eq!(unwritten.len(), 1);
    }

    /// A one-page book whose line is `moves`, in Helvetica.
    fn book(moves: &str) -> Vec<u8> {
        let content = format!("BT /F1 11 Tf 60 730 Td ({moves}) Tj ET");
        let objs = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 600 800] >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
            format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_string(),
        ];
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

    /// Plan A9-N17b (the review of #420): a batch whose job panics keeps what it had not asked,
    /// so Continue is offered; the panic is said.
    #[test]
    fn a_batch_whose_job_panics_keeps_its_positions_for_continue() {
        let mut s = pdfcraft_engine::Session::new();
        let id = s.open("book.pdf", None, Arc::new(book("1.e4 e5 2.Nf3! Nc6")), None).unwrap();
        let read = s.chess_document(id, &[], &pdfcraft_engine::chess::ChessScanSettings::default(), None).unwrap();
        let empty = EvalFile::empty(pdfcraft_engine::chess::corrections::DocRecord::default());
        let batch = Batch::new(&read.doc, &[], &empty, DEFAULT_DEPTH);
        let all = batch.len();
        assert!(all > 0, "a batch to keep");
        let mut app = PdfCraftApp::default();
        let doc = DocId(7);
        app.chess.engine_review.docs.insert(
            doc,
            Checked {
                doc: Arc::new(read.doc),
                refs: None,
                evals: Arc::new(empty),
                generation: 0,
                review: Review::default(),
                labels: Vec::new(),
                stopped: None,
                list: false,
            },
        );
        let shared = Arc::new(Mutex::new(batch));
        let held = Arc::clone(&shared);
        let job: Job<Ended> = Job::spawn("test-check", false, move |_| {
            let _batch = lock(&held);
            panic!("a bug in the batch");
        });
        assert!(job.wait(Duration::from_secs(60)), "the job ends");
        app.chess.engine_review.running = Some(Running {
            doc,
            name: "book.pdf".into(),
            job,
            found: Arc::default(),
            batch: shared,
            limit: Limit::Depth(DEFAULT_DEPTH),
            all,
            before: 0,
            started: Instant::now(),
            halt: Arc::default(),
            stop_asked: None,
            closed: false,
        });
        app.poll_running();
        assert!(!app.chess.engine_review.running(), "ended");
        assert_eq!(app.chess.engine_review.left(doc), Some(all), "Continue has every position it had not asked");
    }

    /// The estimate is said in hours and minutes, rounded up, never "0 min".
    #[test]
    fn the_estimate_is_said_in_hours_and_minutes() {
        assert_eq!(estimate_words(12_000, Limit::Seconds(0.3)), "≈ 1 h");
        assert_eq!(estimate_words(10, Limit::Seconds(0.3)), "≈ 1 min");
        assert_eq!(estimate_words(0, Limit::Seconds(0.3)), "≈ 1 min");
        assert_eq!(estimate_words(13_000, Limit::Seconds(0.3)), "≈ 1 h 5 min");
        // Two plies deeper take about twice as long.
        assert_eq!(estimate_words(12_000, Limit::Depth(18)), "≈ 1 h");
        assert_eq!(estimate_words(12_000, Limit::Depth(20)), "≈ 2 h");
        // Never a panic on a field out of bounds.
        assert_eq!(estimate_words(1, Limit::Seconds(f32::NAN)), "≈ 1 min");
        assert_eq!(estimate_words(usize::MAX, Limit::Depth(u32::MAX)), minutes_words(u64::MAX.div_ceil(60_000)));
        assert_eq!(duration_words(f64::INFINITY), minutes_words(1_000_000_000_000));
        assert_eq!(Limit::Seconds(-3.0).budget().per_position, Duration::from_secs_f32(MIN_SECONDS));
        assert_eq!(Limit::Depth(0).budget().depth, 1);
        assert_eq!(Limit::Seconds(1.0).kept_depth(), 1, "searched for a time: whatever is held is left out");
    }
}
