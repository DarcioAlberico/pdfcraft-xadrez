//! Chess ▸ one kind of job (plan A9 §2.7, step I2; findings A5-03, A5-12, A5-15): work off the
//! interface thread whose result always comes back, which can be asked to stop and says how far
//! it is. The page read, the scan, the long jobs (Export, Link the whole book), the capture's
//! plan and the OCR area all run as one.
//!
//! **The result always comes back.** The worker's first act is to set a guard ([`Guard`]) that,
//! dropped last when the work returns or unwinds, marks the slot when the work left nothing
//! there: a panic in the work never leaves the app waiting for ever (A5-03). The guard never
//! panics and allocates nothing: the lock is taken even poisoned, and nothing of the work's data
//! is formatted. [`Job::poll`] then joins the thread, so a panic is said with its message. A
//! thread that can't be started gives its error at once.
//!
//! **Stop and progress.** The work is given a [`JobCtl`]: it looks at [`JobCtl::cancelled`]
//! between its steps and reports [`JobCtl::progress`]. Dropping a job asks it to stop; its thread
//! finishes on its own and what it left is dropped.
//!
//! With `inline` (tests, and the web, which has no threads) the work runs on the caller's thread,
//! where the guard protects nothing: a panic there reaches the caller. The guard's test is a unit
//! test with a real thread. There is no `panic = "abort"` in the workspace; with one, the guard
//! would never run.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// How the work ended.
enum Outcome<T> {
    /// It returned this.
    Done(Result<T, String>),
    /// It ended without a result: it panicked.
    Lost,
}

/// Where a job's outcome waits.
type Slot<T> = Arc<Mutex<Option<Outcome<T>>>>;

/// The lock, even after a worker panicked while holding it (it holds plain data).
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What a job's work is given: whether it was asked to stop, and where it says how far it is.
#[derive(Clone, Debug, Default)]
pub(crate) struct JobCtl {
    cancel: Arc<AtomicBool>,
    done: Arc<AtomicUsize>,
    total: Arc<AtomicUsize>,
}

impl JobCtl {
    /// Whether the job was asked to stop (Stop, or the job dropped): the work stops at its next
    /// step.
    pub(crate) fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// `done` of `total` steps are done.
    pub(crate) fn progress(&self, done: usize, total: usize) {
        self.total.store(total, Ordering::Relaxed);
        self.done.store(done.min(total), Ordering::Relaxed);
    }
}

/// Marks the slot, when it is dropped and the work left nothing there, as ended without a result.
/// Set first in the worker, so it is dropped last.
struct Guard<T> {
    slot: Slot<T>,
}

impl<T> Drop for Guard<T> {
    fn drop(&mut self) {
        let mut slot = lock(&self.slot);
        if slot.is_none() {
            *slot = Some(Outcome::Lost);
        }
    }
}

/// Work off the interface thread ([`Job::spawn`]), its result taken by [`Job::poll`].
pub(crate) struct Job<T> {
    slot: Slot<T>,
    ctl: JobCtl,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl<T: Send + 'static> Job<T> {
    /// Run `work` on a thread of its own named `name`, or on this one when `inline` (tests; the
    /// web always, which has no threads).
    pub(crate) fn spawn(name: &str, inline: bool, work: impl FnOnce(&JobCtl) -> Result<T, String> + Send + 'static) -> Job<T> {
        let mut job = Job { slot: Arc::default(), ctl: JobCtl::default(), handle: None };
        let (slot, ctl) = (job.slot.clone(), job.ctl.clone());
        let run = move || {
            // First, so it is dropped last, the result already written when there is one.
            let _guard = Guard { slot: slot.clone() };
            let out = work(&ctl);
            *lock(&slot) = Some(Outcome::Done(out));
        };
        if inline || cfg!(target_arch = "wasm32") {
            run();
        } else {
            match std::thread::Builder::new().name(name.to_owned()).spawn(run) {
                Ok(handle) => job.handle = Some(handle),
                Err(e) => *lock(&job.slot) = Some(Outcome::Done(Err(e.to_string()))),
            }
        }
        job
    }
}

impl<T> Job<T> {
    /// What the work gave, once it is done (taken: `None` before, and after). A panic is said
    /// with its message.
    pub(crate) fn poll(&mut self) -> Option<Result<T, String>> {
        let out = lock(&self.slot).take()?;
        // The worker is past its result: its thread ends at once, and gives a panic's message.
        let message = self
            .handle
            .take()
            .and_then(|h| h.join().err())
            .and_then(|payload| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()).or_else(|| payload.downcast_ref::<String>().cloned()));
        Some(match out {
            Outcome::Done(result) => result,
            Outcome::Lost => Err(match message {
                Some(why) => crate::i18n::fmt(tl!("The work stopped on an internal error: {why}"), &[("why", &why)]),
                None => tl!("The work stopped on an internal error.").to_owned(),
            }),
        })
    }

    /// Ask the work to stop at its next step.
    pub(crate) fn cancel(&self) {
        self.ctl.cancel.store(true, Ordering::Relaxed);
    }

    /// How far the work says it is: steps done, of how many (0 of 0 until it says).
    pub(crate) fn progress(&self) -> (usize, usize) {
        (self.ctl.done.load(Ordering::Relaxed), self.ctl.total.load(Ordering::Relaxed))
    }

    /// A job that never ends (the interface's tests, which draw a job running).
    #[cfg(test)]
    pub(crate) fn waiting() -> Job<T> {
        Job { slot: Arc::default(), ctl: JobCtl::default(), handle: None }
    }
}

/// Dropping a job asks its work to stop; its thread finishes on its own.
impl<T> Drop for Job<T> {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait<T: Send + 'static>(job: &mut Job<T>) -> Result<T, String> {
        for _ in 0..12_000 {
            if let Some(out) = job.poll() {
                return out;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("the job never came back");
    }

    /// A job on a real thread: its result comes back once; a panic in the work comes back as an
    /// error with its message (finding A5-03: the Text tab's job had no guard, and a panic in it
    /// left the tab busy for ever).
    #[test]
    fn a_panicking_job_comes_back_with_its_message() {
        let mut ok = Job::spawn("job-test", false, |_| Ok(41 + 1));
        assert_eq!(wait(&mut ok), Ok(42));
        assert_eq!(ok.poll(), None, "taken once");
        let mut boom: Job<u8> = Job::spawn("job-test", false, |_| panic!("the page could not be read"));
        let err = wait(&mut boom).unwrap_err();
        assert!(err.contains("internal error") && err.contains("the page could not be read"), "{err}");
        let mut failed: Job<u8> = Job::spawn("job-test", false, |_| Err("no such page".into()));
        assert_eq!(wait(&mut failed), Err("no such page".into()), "an error the work gives is its own");
    }

    /// Inline (tests, the web): the work is done when `spawn` returns.
    #[test]
    fn an_inline_job_is_done_at_once() {
        let mut job = Job::spawn("job-test", true, |ctl| {
            ctl.progress(3, 3);
            Ok("read")
        });
        assert_eq!(job.progress(), (3, 3));
        assert_eq!(job.poll(), Some(Ok("read")));
    }

    /// Dropping a job asks its work to stop; the work sees it between its steps, and what it
    /// says of how far it is was seen while it ran.
    #[test]
    fn dropping_a_job_cancels_it_and_progress_is_seen() {
        let (tx, rx) = std::sync::mpsc::channel();
        let (started_tx, started) = std::sync::mpsc::channel();
        let job: Job<usize> = Job::spawn("job-test", false, move |ctl| {
            let mut steps = 0;
            ctl.progress(0, 10);
            let _ = started_tx.send(());
            while !ctl.cancelled() && steps < 1_000_000 {
                steps += 1;
                ctl.progress(steps.min(10), 10);
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            let _ = tx.send(ctl.cancelled());
            Ok(steps)
        });
        assert_eq!(started.recv_timeout(std::time::Duration::from_secs(60)), Ok(()));
        assert_eq!(job.progress().1, 10);
        drop(job);
        assert_eq!(rx.recv_timeout(std::time::Duration::from_secs(60)), Ok(true), "the work saw the stop");
    }

    /// The guard marks a slot the work left empty, and never touches a result written.
    #[test]
    fn the_guard_marks_what_left_no_result() {
        let slot: Slot<u8> = Arc::default();
        drop(Guard { slot: slot.clone() });
        assert!(matches!(lock(&slot).take(), Some(Outcome::Lost)));
        *lock(&slot) = Some(Outcome::Done(Ok(1)));
        drop(Guard { slot: slot.clone() });
        assert!(matches!(lock(&slot).take(), Some(Outcome::Done(Ok(1)))), "a result written is kept");
    }
}
