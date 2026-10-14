//! One model session is built at a time in a process.
//!
//! Building a CUDA session through sherpa-onnx and another through `ort` at
//! the same moment can throw a C++ exception across the Rust boundary, which
//! ends the process (`Rust cannot catch foreign exceptions`). Every place that
//! builds a session takes this lock first and holds it only while the library
//! builds; inference runs outside it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

static BUILDING: Mutex<()> = Mutex::new(());
static ENABLED: AtomicBool = AtomicBool::new(true);

/// Held while a session is being built; the next builder waits for its drop.
pub struct SessionLock {
    _held: Option<MutexGuard<'static, ()>>,
}

/// Waits for the session being built, if any, and takes its place.
pub fn lock_for_session() -> SessionLock {
    if !ENABLED.load(Ordering::Relaxed) {
        return SessionLock { _held: None };
    }
    // A builder that panicked left nothing to repair: the lock only orders builds.
    let held = match BUILDING.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    SessionLock { _held: Some(held) }
}

/// Turns the ordering off or on for the whole process. It exists so that a
/// test can show what the lock prevents; a program has no use for it.
#[doc(hidden)]
pub fn set_enabled_for_test(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    static INSIDE: AtomicUsize = AtomicUsize::new(0);
    static MOST: AtomicUsize = AtomicUsize::new(0);

    fn build_slowly() {
        let _lock = lock_for_session();
        let now = INSIDE.fetch_add(1, Ordering::SeqCst) + 1;
        MOST.fetch_max(now, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(20));
        INSIDE.fetch_sub(1, Ordering::SeqCst);
    }

    #[test]
    fn builders_in_many_threads_never_overlap() {
        let threads: Vec<_> = (0..6).map(|_| std::thread::spawn(build_slowly)).collect();
        for thread in threads {
            assert!(thread.join().is_ok());
        }
        assert_eq!(MOST.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_lock_is_free_again_after_it_drops() {
        drop(lock_for_session());
        drop(lock_for_session());
    }
}
