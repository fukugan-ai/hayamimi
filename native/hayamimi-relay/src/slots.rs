//! The slots of the llama-server the live pair is translated on.
//!
//! The server takes [`LLAMA_SLOTS`] requests at once and queues the rest in
//! front of the model, and a request waiting in that queue spends its time
//! limit (`llm::REQUEST_TIMEOUT`). The relay therefore keeps a request back
//! until a slot is free, instead of sending it to wait in the server. A
//! request holds its slot from the moment it is made to the moment its answer
//! has been judged.

use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// How many requests the server takes at once (`-np 4`): the microphone's
/// line, the loopback's line, the text in progress, and an edit.
pub const LLAMA_SLOTS: usize = 4;

/// Counts the free slots of one server.
pub struct ServerSlots {
    free: Mutex<usize>,
    freed: Condvar,
}

/// A slot in use. It is given back when this is dropped.
pub struct Held<'a> {
    slots: &'a ServerSlots,
}

fn locked(free: &Mutex<usize>) -> MutexGuard<'_, usize> {
    // A count is whole whichever thread panicked while it held the lock.
    match free.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

impl ServerSlots {
    /// A server with `count` slots, all free. `count` is at least 1.
    pub fn new(count: usize) -> ServerSlots {
        ServerSlots {
            free: Mutex::new(count),
            freed: Condvar::new(),
        }
    }

    /// Waits for a free slot and takes it. Returns the slot and how long the
    /// wait took.
    pub fn take(&self) -> (Held<'_>, Duration) {
        let started = Instant::now();
        let waited = self.freed.wait_while(locked(&self.free), |free| *free == 0);
        let mut free = match waited {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        *free -= 1;
        (Held { slots: self }, started.elapsed())
    }

    /// How many slots are free now.
    #[cfg(test)]
    fn free(&self) -> usize {
        *locked(&self.free)
    }
}

impl Drop for Held<'_> {
    fn drop(&mut self) {
        *locked(&self.slots.free) += 1;
        self.slots.freed.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn a_slot_taken_is_not_free_until_it_is_dropped() {
        let slots = ServerSlots::new(2);
        let (first, _) = slots.take();
        let (second, _) = slots.take();
        assert_eq!(slots.free(), 0);
        drop(first);
        assert_eq!(slots.free(), 1);
        drop(second);
        assert_eq!(slots.free(), 2);
    }

    #[test]
    fn a_free_slot_is_taken_without_waiting() {
        let slots = ServerSlots::new(1);
        let (_held, waited) = slots.take();
        assert!(waited < Duration::from_millis(500), "{waited:?}");
    }

    #[test]
    fn a_request_past_the_last_slot_waits_until_one_is_given_back() {
        let slots = Arc::new(ServerSlots::new(1));
        let (held, _) = slots.take();
        let got_it = Arc::new(AtomicBool::new(false));
        let waiter = {
            let (slots, got_it) = (Arc::clone(&slots), Arc::clone(&got_it));
            std::thread::spawn(move || {
                let (_held, waited) = slots.take();
                got_it.store(true, Ordering::SeqCst);
                waited
            })
        };
        std::thread::sleep(Duration::from_millis(100));
        assert!(
            !got_it.load(Ordering::SeqCst),
            "took a slot that was not free"
        );
        drop(held);
        let waited = waiter.join();
        assert!(matches!(waited, Ok(waited) if waited >= Duration::from_millis(50)));
        assert!(got_it.load(Ordering::SeqCst));
    }
}
