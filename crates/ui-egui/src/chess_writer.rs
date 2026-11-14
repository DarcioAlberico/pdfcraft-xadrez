//! Chess ▸ a writer thread for a store (plan A9-I3b, §2.3: "a UI nunca espera"): the interface
//! thread hands it what a file should hold, and goes on.
//!
//! - **One thread per store**, made with the store's first write, so the files of one store are
//!   written one at a time, in the order asked.
//! - **The last request wins.** Each file has one slot: a request for a file whose last request
//!   has not been taken yet replaces it. Every request gets a generation, increasing across the
//!   store, so an older write never lands after a newer one, and two writes of one file are never
//!   in flight.
//! - **What became of each write** comes back to the interface thread ([`StoreWriter::take_done`])
//!   with its file and generation; a write that panicked comes back as the store's error for it,
//!   and the thread goes on.
//! - **Waiting** happens only where the app must know the writes landed, Close and Quit
//!   ([`StoreWriter::settle`], at most [`SETTLE`]); past that, what is still pending is treated as
//!   not kept, and the app asks, as it does for unsaved documents. A store dropped (the app gone)
//!   waits as long for what was asked, then leaves the thread to it.

use std::collections::BTreeMap;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// How long Close and Quit wait for a pending write before treating it as not kept.
pub(crate) const SETTLE: Duration = Duration::from_secs(2);

/// What became of a write.
#[derive(Debug)]
pub(crate) struct Done<R> {
    /// The file it was for.
    pub file: String,
    /// The generation it was asked with ([`StoreWriter::submit`]).
    pub generation: u64,
    pub result: R,
}

/// The writes asked for and not yet taken back.
struct State<J, R> {
    /// The last request of each file not yet started, with its generation.
    slots: BTreeMap<String, (u64, J)>,
    /// The file being written, and its generation.
    busy: Option<(String, u64)>,
    /// What became of the writes, until taken.
    done: Vec<Done<R>>,
    /// The store is gone: the thread writes what was asked and stops.
    stop: bool,
}

struct Shared<J, R> {
    state: Mutex<State<J, R>>,
    /// A request came, or a write ended.
    changed: Condvar,
}

impl<J, R> Shared<J, R> {
    /// The state, whatever another thread's panic left behind (every change to it is one step).
    fn state(&self) -> MutexGuard<'_, State<J, R>> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// How a store writes when it has no thread: the write itself.
type InlineWrite<J, R> = Box<dyn FnMut(&str, J) -> R + Send>;

/// A store's writer thread ([`StoreWriter::submit`]), with the requests' generations.
pub(crate) struct StoreWriter<J: Send + 'static, R: Send + 'static> {
    shared: Arc<Shared<J, R>>,
    /// The next request's generation.
    next: u64,
    /// The thread could not be started: requests are written on the caller's thread.
    inline: Option<InlineWrite<J, R>>,
}

impl<J: Send + 'static, R: Send + 'static> StoreWriter<J, R> {
    /// A writer named `name` (the thread's name), writing each request with `write`. A `write` that
    /// panics gives `failed(message)` for that request. When no thread can be started, requests
    /// are written by [`Self::submit`] itself, as they were before plan A9-I3b.
    pub(crate) fn new(name: &str, write: impl FnMut(&str, J) -> R + Send + Clone + 'static, failed: fn(String) -> R) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::new(State { slots: BTreeMap::new(), busy: None, done: Vec::new(), stop: false }),
            changed: Condvar::new(),
        });
        let worker = shared.clone();
        let mut on_thread = write.clone();
        let spawned = std::thread::Builder::new().name(name.to_owned()).spawn(move || run(&worker, &mut on_thread, failed));
        let inline: Option<InlineWrite<J, R>> = match spawned {
            Ok(_) => None,
            Err(e) => {
                log::warn!("the {name} writer thread couldn't start ({e}); writing on the interface thread");
                Some(Box::new(write))
            }
        };
        StoreWriter { shared, next: 1, inline }
    }

    /// Ask for `file` to be written by `job`: it replaces the file's last request when that one
    /// has not been started. The request's generation, greater than every earlier one's.
    pub(crate) fn submit(&mut self, file: &str, job: J) -> u64 {
        let generation = self.next;
        self.next = self.next.saturating_add(1);
        if let Some(write) = self.inline.as_mut() {
            let result = write(file, job);
            self.shared.state().done.push(Done { file: file.to_owned(), generation, result });
            return generation;
        }
        let mut state = self.shared.state();
        state.slots.insert(file.to_owned(), (generation, job));
        drop(state);
        self.shared.changed.notify_all();
        generation
    }

    /// What became of the writes that ended since the last call, in the order they ended.
    pub(crate) fn take_done(&self) -> Vec<Done<R>> {
        std::mem::take(&mut self.shared.state().done)
    }

    /// Whether a write is asked for or in flight.
    #[cfg(test)]
    fn pending(&self) -> bool {
        let state = self.shared.state();
        state.busy.is_some() || !state.slots.is_empty()
    }

    /// Wait until every write asked for has ended, at most `timeout`. Whether they all have.
    pub(crate) fn settle(&self, timeout: Duration) -> bool {
        let deadline = Instant::now().checked_add(timeout);
        let mut state = self.shared.state();
        loop {
            if state.busy.is_none() && state.slots.is_empty() {
                return true;
            }
            let left = match deadline {
                Some(d) => d.saturating_duration_since(Instant::now()),
                None => timeout,
            };
            if left.is_zero() {
                return false;
            }
            state = match self.shared.changed.wait_timeout(state, left) {
                Ok((s, _)) => s,
                Err(poisoned) => poisoned.into_inner().0,
            };
        }
    }
}

impl<J: Send + 'static, R: Send + 'static> Drop for StoreWriter<J, R> {
    /// The store is gone (the app closes): the thread writes what was asked and stops, waited for
    /// at most [`SETTLE`], so what was asked last is on disk when the app is gone.
    fn drop(&mut self) {
        self.shared.state().stop = true;
        self.shared.changed.notify_all();
        self.settle(SETTLE);
    }
}

/// The writer thread: the oldest request first, until the store is gone and nothing is left.
fn run<J, R>(shared: &Shared<J, R>, write: &mut impl FnMut(&str, J) -> R, failed: fn(String) -> R) {
    loop {
        let mut state = shared.state();
        let next = loop {
            // The oldest request: files are written in the order they were asked.
            let oldest = state.slots.iter().min_by_key(|(_, (generation, _))| *generation).map(|(file, _)| file.clone());
            if let Some((file, (generation, job))) = oldest.and_then(|f| state.slots.remove_entry(&f)) {
                state.busy = Some((file.clone(), generation));
                break Some((file, generation, job));
            }
            if state.stop {
                break None;
            }
            state = match shared.changed.wait(state) {
                Ok(s) => s,
                Err(poisoned) => poisoned.into_inner(),
            };
        };
        drop(state);
        let Some((file, generation, job)) = next else { return };
        let result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| write(&file, job))) {
            Ok(r) => r,
            Err(p) => failed(p.downcast_ref::<&str>().map(|s| (*s).to_owned()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_default()),
        };
        let mut state = shared.state();
        state.busy = None;
        state.done.push(Done { file, generation, result });
        drop(state);
        shared.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A slow disk: each write takes 10 ms and lands in `disk` (file → what was written, in the
    /// order written).
    type Disk = Arc<Mutex<Vec<(String, u32)>>>;

    fn slow(disk: &Disk, ms: u64) -> StoreWriter<u32, Result<(), String>> {
        let disk = disk.clone();
        StoreWriter::new(
            "test-writer",
            move |file: &str, value: u32| {
                std::thread::sleep(Duration::from_millis(ms));
                if value == 666 {
                    panic!("the disk broke");
                }
                disk.lock().unwrap().push((file.to_owned(), value));
                Ok(())
            },
            Err,
        )
    }

    /// Plan A9-I3b's check: on a slow store, changes 1, 2 and 3 of one file asked for within 10
    /// ms end with 3 on disk, in at most two writes, and an older one never lands after a newer.
    #[test]
    fn the_last_request_wins_and_an_older_write_never_lands_after_a_newer() {
        let disk: Disk = Arc::default();
        let mut w = slow(&disk, 30);
        let g1 = w.submit("a.games", 1);
        std::thread::sleep(Duration::from_millis(5));
        let g2 = w.submit("a.games", 2);
        let g3 = w.submit("a.games", 3);
        assert!(g1 < g2 && g2 < g3);
        assert!(w.settle(SETTLE));
        let written = disk.lock().unwrap().clone();
        assert_eq!(written.last(), Some(&("a.games".to_owned(), 3)), "{written:?}");
        assert!(written.len() <= 2, "at most two writes: {written:?}");
        let values: Vec<u32> = written.iter().map(|(_, v)| *v).collect();
        assert!(values.windows(2).all(|p| p[0] < p[1]), "never an older after a newer: {values:?}");
        let done: Vec<u64> = w.take_done().iter().map(|d| d.generation).collect();
        assert_eq!(done.last(), Some(&g3));
        assert!(!w.pending());
    }

    /// Two files each keep their last request; one never takes the other's place.
    #[test]
    fn each_file_keeps_its_own_last_request() {
        let disk: Disk = Arc::default();
        let mut w = slow(&disk, 5);
        for v in 1..=4 {
            w.submit("a", v);
            w.submit("b", v + 10);
        }
        assert!(w.settle(SETTLE));
        let written = disk.lock().unwrap().clone();
        let last = |f: &str| written.iter().rev().find(|(n, _)| n == f).map(|(_, v)| *v);
        assert_eq!((last("a"), last("b")), (Some(4), Some(14)), "{written:?}");
    }

    /// A write that panics comes back as the store's error for it, and the thread goes on.
    #[test]
    fn a_write_that_panics_is_an_error_and_the_thread_goes_on() {
        let disk: Disk = Arc::default();
        let mut w = slow(&disk, 1);
        let broken = w.submit("a", 666);
        assert!(w.settle(SETTLE));
        let ok = w.submit("a", 7);
        assert!(w.settle(SETTLE));
        let done = w.take_done();
        assert!(done.iter().any(|d| d.generation == broken && d.result.as_ref().is_err_and(|e| e.contains("the disk broke"))), "{done:?}");
        assert!(done.iter().any(|d| d.generation == ok && d.result.is_ok()), "{done:?}");
        assert_eq!(disk.lock().unwrap().as_slice(), [("a".to_owned(), 7)]);
    }

    /// Waiting is bounded: a write slower than the wait leaves it pending, and says so.
    #[test]
    fn settling_waits_at_most_its_time() {
        let disk: Disk = Arc::default();
        let mut w = slow(&disk, 400);
        w.submit("a", 1);
        let started = Instant::now();
        assert!(!w.settle(Duration::from_millis(50)), "still writing");
        assert!(started.elapsed() < Duration::from_millis(350), "{:?}", started.elapsed());
        assert!(w.pending());
        assert!(w.settle(SETTLE));
        assert_eq!(disk.lock().unwrap().len(), 1);
    }

    /// The store gone, what was asked is written before the drop returns.
    #[test]
    fn what_was_asked_is_written_when_the_store_is_dropped() {
        let disk: Disk = Arc::default();
        {
            let mut w = slow(&disk, 20);
            w.submit("a", 1);
            w.submit("b", 2);
        }
        assert_eq!(disk.lock().unwrap().len(), 2);
    }
}
