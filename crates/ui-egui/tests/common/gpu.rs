//! One wgpu device compiling shaders at a time, for every test that renders the window (pixels
//! read, or a picture saved): Microsoft's WARP, wgpu's fallback without a GPU, crashes with an
//! access violation when two devices compile at once, as on GitHub's Windows 11 ARM64 runner
//! (#170). One lock in one place (plan A9-Q4b, finding A7-16): the five files that read pixels
//! each had their own, and the chess tests' pictures took none.
//!
//! Each test binary is a process of its own, so the lock orders the tests of one binary; cargo
//! runs the binaries one after the other. Included with `#[path = "common/gpu.rs"] mod gpu;`,
//! not through `support`: a binary that includes a module and uses only part of it gets a
//! dead-code warning, which `-D warnings` makes an error.

use std::cell::Cell;
use std::sync::{Mutex, MutexGuard, PoisonError};

static GPU: Mutex<()> = Mutex::new(());

thread_local! {
    /// This test's thread holds the lock: a picture taken inside a test that reads pixels takes
    /// it again without waiting for itself.
    static HELD: Cell<bool> = const { Cell::new(false) };
}

/// The lock, held until this is dropped.
pub struct Gpu(Option<MutexGuard<'static, ()>>);

impl Drop for Gpu {
    fn drop(&mut self) {
        if self.0.is_some() {
            HELD.with(|held| held.set(false));
        }
    }
}

/// The lock. A test that reads pixels takes it before it builds its harness and holds it to the
/// end (declared first, it is released after the harness and its device are dropped); a picture
/// taken for the eyes holds it while it renders. Taken again on the thread that holds it, it is
/// already held.
pub fn gpu() -> Gpu {
    if HELD.with(Cell::get) {
        return Gpu(None);
    }
    // A test that panicked while holding it leaves nothing to clean up.
    let guard = GPU.lock().unwrap_or_else(PoisonError::into_inner);
    HELD.with(|held| held.set(true));
    Gpu(Some(guard))
}
