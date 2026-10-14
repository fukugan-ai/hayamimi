//! The GPU memory budget. The memory in use on the whole GPU, every process
//! included, is read once a second. When it reaches the budget, one GPU stage
//! moves to the CPU, and a second later, if the budget is still reached, the
//! next one does. The run is never stopped for want of memory.

use std::time::{Duration, Instant};

use hayamimi_gpu::error::GpuError;
use hayamimi_gpu::memory::MemoryMeter;

/// How often the memory is read.
pub const READ_EVERY: Duration = Duration::from_secs(1);

/// Whether a reading has reached the budget.
pub fn is_reached(used_mib: u64, budget_mib: u64) -> bool {
    used_mib >= budget_mib
}

/// What a source of readings can be asked.
pub trait Meter {
    /// The memory in use, in MiB.
    fn used_mib(&self) -> Result<u64, GpuError>;
}

impl Meter for MemoryMeter {
    fn used_mib(&self) -> Result<u64, GpuError> {
        MemoryMeter::used_mib(self)
    }
}

/// Reads the memory on its schedule and says when the budget is reached.
pub struct MemoryGuard<M: Meter> {
    meter: M,
    budget_mib: u64,
    next_read: Instant,
    /// Set when a read failed: the run goes on without a budget rather than guessing.
    blind: bool,
}

/// What one look at the guard found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    /// It is not time to read, or the budget is not reached.
    Quiet,
    /// The budget is reached at this reading, in MiB.
    Reached { used_mib: u64 },
    /// The memory can no longer be read.
    Blind,
}

impl<M: Meter> MemoryGuard<M> {
    pub fn new(meter: M, budget_mib: u64, now: Instant) -> MemoryGuard<M> {
        MemoryGuard {
            meter,
            budget_mib,
            next_read: now,
            blind: false,
        }
    }

    /// Reads the memory if a second has passed since the last read.
    pub fn look(&mut self, now: Instant) -> Look {
        if self.blind || now < self.next_read {
            return Look::Quiet;
        }
        self.next_read = now + READ_EVERY;
        match self.meter.used_mib() {
            Ok(used_mib) if is_reached(used_mib, self.budget_mib) => Look::Reached { used_mib },
            Ok(_) => Look::Quiet,
            Err(_) => {
                self.blind = true;
                Look::Blind
            }
        }
    }
}

impl MemoryGuard<MemoryMeter> {
    /// Opens NVML for a budget of `budget_mib`.
    pub fn open(budget_mib: u64, now: Instant) -> Result<MemoryGuard<MemoryMeter>, GpuError> {
        Ok(MemoryGuard::new(MemoryMeter::open()?, budget_mib, now))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Readings handed out in order; once they are gone the meter fails.
    struct Script(RefCell<Vec<u64>>);

    impl Meter for Script {
        fn used_mib(&self) -> Result<u64, GpuError> {
            let mut readings = self.0.borrow_mut();
            if readings.is_empty() {
                return Err(GpuError::Overflow("the script"));
            }
            Ok(readings.remove(0))
        }
    }

    fn script(readings: &[u64]) -> Script {
        Script(RefCell::new(readings.to_vec()))
    }

    #[test]
    fn the_budget_is_reached_at_the_budget_and_not_below_it() {
        assert!(is_reached(14_000, 14_000));
        assert!(is_reached(14_001, 14_000));
        assert!(!is_reached(13_999, 14_000));
    }

    #[test]
    fn a_reading_below_the_budget_is_quiet() {
        let start = Instant::now();
        let mut guard = MemoryGuard::new(script(&[1_199]), 14_000, start);
        assert_eq!(guard.look(start), Look::Quiet);
    }

    #[test]
    fn a_reading_at_the_budget_is_reached() {
        let start = Instant::now();
        let mut guard = MemoryGuard::new(script(&[14_000]), 14_000, start);
        assert_eq!(guard.look(start), Look::Reached { used_mib: 14_000 });
    }

    #[test]
    fn the_memory_is_read_once_a_second() {
        let start = Instant::now();
        let mut guard = MemoryGuard::new(script(&[14_100, 14_200]), 14_000, start);
        assert_eq!(guard.look(start), Look::Reached { used_mib: 14_100 });
        // Half a second later nothing is read, so the second figure is still there.
        assert_eq!(guard.look(start + Duration::from_millis(500)), Look::Quiet);
        assert_eq!(
            guard.look(start + READ_EVERY),
            Look::Reached { used_mib: 14_200 }
        );
    }

    #[test]
    fn a_meter_that_fails_blinds_the_guard_once_and_for_good() {
        let start = Instant::now();
        let mut guard = MemoryGuard::new(script(&[]), 14_000, start);
        assert_eq!(guard.look(start), Look::Blind);
        assert_eq!(guard.look(start + READ_EVERY * 2), Look::Quiet);
    }
}
