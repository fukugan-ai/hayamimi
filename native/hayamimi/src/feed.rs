//! The line from the audio inputs to the recognition loop.
//!
//! Every input (a wav file, a microphone, the loopback, a network client)
//! cuts its audio into windows of the size the detector reads and puts them
//! on one queue. The loop reads the queue and nothing else, so it does not
//! know which input it is serving.

use std::cell::Cell;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use hayamimi_asr::audio::{SAMPLE_RATE, WINDOW, windows};

/// Windows the queue holds before a live input starts dropping new ones:
/// about four and a half minutes of audio, 16 MiB.
pub const CAPACITY: usize = 8192;

/// What an input hands the loop.
#[derive(Debug, Clone, PartialEq)]
pub enum AudioItem {
    /// One window of `WINDOW` samples at 16 kHz, mono.
    Window(Vec<f32>),
    /// The source went away in the middle of speech: cut what the detector holds.
    Flush,
    /// A finite input has played to its end.
    End,
}

/// An item on the queue, stamped with the moment it was put there.
struct Stamped {
    item: AudioItem,
    queued_at: Instant,
}

/// The sending end. Clones share the queue.
#[derive(Clone)]
pub struct AudioTx {
    sender: Sender<Stamped>,
    queued: Arc<AtomicUsize>,
    /// Windows the queue has taken since the start, from every input.
    windows: Arc<AtomicU64>,
}

/// The receiving end, read by the loop.
pub struct AudioRx {
    receiver: Receiver<Stamped>,
    queued: Arc<AtomicUsize>,
    /// How long the item last returned had waited on the queue.
    waited: Cell<Duration>,
}

/// The queue between the inputs and the loop.
pub fn channel() -> (AudioTx, AudioRx) {
    let (sender, receiver) = mpsc::channel();
    let queued = Arc::new(AtomicUsize::new(0));
    (
        AudioTx {
            sender,
            queued: Arc::clone(&queued),
            windows: Arc::new(AtomicU64::new(0)),
        },
        AudioRx {
            receiver,
            queued,
            waited: Cell::new(Duration::ZERO),
        },
    )
}

impl AudioTx {
    /// Puts an item on the queue. A live input cannot wait for the loop, so
    /// a window that finds the queue full is dropped and the answer is
    /// `false`; so it is when the loop has gone.
    pub fn send(&self, item: AudioItem) -> bool {
        if self.queued.load(Ordering::Relaxed) >= CAPACITY && matches!(item, AudioItem::Window(_)) {
            return false;
        }
        self.queued.fetch_add(1, Ordering::Relaxed);
        let is_window = matches!(item, AudioItem::Window(_));
        let stamped = Stamped {
            item,
            queued_at: Instant::now(),
        };
        if self.sender.send(stamped).is_err() {
            self.queued.fetch_sub(1, Ordering::Relaxed);
            return false;
        }
        if is_window {
            self.windows.fetch_add(1, Ordering::Relaxed);
        }
        true
    }

    /// Where the recognition loop's stream clock will stand when it has read
    /// everything queued so far, in microseconds: the clock of the `t0_s` and
    /// `t1_s` of a `final` counts the windows read, and the windows that were
    /// dropped on a full queue were never read.
    pub fn stream_us(&self) -> u64 {
        self.windows.load(Ordering::Relaxed) * (WINDOW as u64) * 1_000_000 / u64::from(SAMPLE_RATE)
    }

    /// Puts an item on the queue, waiting while it is full. For an input
    /// that can wait, a file that is read faster than it is recognised.
    pub fn send_waiting(&self, item: AudioItem) -> bool {
        while self.queued.load(Ordering::Relaxed) >= CAPACITY {
            std::thread::sleep(Duration::from_millis(5));
        }
        self.send(item)
    }
}

impl AudioRx {
    /// The next item, waiting at most `wait`.
    pub fn recv_timeout(&self, wait: Duration) -> Result<AudioItem, RecvTimeoutError> {
        let stamped = self.receiver.recv_timeout(wait)?;
        self.queued.fetch_sub(1, Ordering::Relaxed);
        self.waited.set(stamped.queued_at.elapsed());
        Ok(stamped.item)
    }

    /// How long the item `recv_timeout` returned last had waited on the queue:
    /// the time from an input putting a window there to the loop taking it.
    pub fn last_wait(&self) -> Duration {
        self.waited.get()
    }

    /// How many items wait: the loop is behind real time by this many windows.
    pub fn backlog(&self) -> usize {
        self.queued.load(Ordering::Relaxed)
    }
}

/// When window number `index` of a file is due, counted from the start of the playback.
pub fn due_after(index: usize) -> Duration {
    Duration::from_secs_f64((index * WINDOW) as f64 / f64::from(SAMPLE_RATE))
}

/// Plays a recording onto the queue from a thread of its own, window by
/// window, at the speed of the audio when `realtime` and as fast as the loop
/// takes it otherwise, then says that the input has ended.
pub fn play_wav(samples: Vec<f32>, realtime: bool, tx: AudioTx) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let start = Instant::now();
        for (index, window) in windows(&samples).enumerate() {
            if realtime {
                // Absolute deadlines: a sleep per window drifts by the timer's granularity.
                if let Some(wait) =
                    (start + due_after(index)).checked_duration_since(Instant::now())
                {
                    std::thread::sleep(wait);
                }
            }
            if !tx.send_waiting(AudioItem::Window(window)) {
                return;
            }
        }
        tx.send_waiting(AudioItem::End);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TICK: Duration = Duration::from_millis(200);

    #[test]
    fn items_come_out_in_the_order_they_went_in() {
        let (tx, rx) = channel();
        assert!(tx.send(AudioItem::Window(vec![1.0])));
        assert!(tx.send(AudioItem::Flush));
        assert_eq!(rx.recv_timeout(TICK), Ok(AudioItem::Window(vec![1.0])));
        assert_eq!(rx.recv_timeout(TICK), Ok(AudioItem::Flush));
    }

    #[test]
    fn the_last_wait_is_the_time_the_item_spent_on_the_queue() {
        let (tx, rx) = channel();
        assert_eq!(rx.last_wait(), Duration::ZERO);
        tx.send(AudioItem::Flush);
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(rx.recv_timeout(TICK), Ok(AudioItem::Flush));
        let waited = rx.last_wait();
        assert!(waited >= Duration::from_millis(30), "{waited:?}");
        assert!(waited < Duration::from_secs(5), "{waited:?}");
    }

    #[test]
    fn the_backlog_counts_what_waits() {
        let (tx, rx) = channel();
        assert_eq!(rx.backlog(), 0);
        tx.send(AudioItem::Window(vec![0.0]));
        tx.send(AudioItem::Window(vec![0.0]));
        assert_eq!(rx.backlog(), 2);
        let _ = rx.recv_timeout(TICK);
        assert_eq!(rx.backlog(), 1);
    }

    #[test]
    fn an_empty_queue_times_out_and_a_closed_one_says_so() {
        let (tx, rx) = channel();
        assert_eq!(
            rx.recv_timeout(Duration::from_millis(5)),
            Err(RecvTimeoutError::Timeout)
        );
        drop(tx);
        assert_eq!(
            rx.recv_timeout(Duration::from_millis(5)),
            Err(RecvTimeoutError::Disconnected)
        );
    }

    #[test]
    fn a_full_queue_drops_windows_but_not_the_notes() {
        let (tx, rx) = channel();
        for _ in 0..CAPACITY {
            assert!(tx.send(AudioItem::Window(Vec::new())));
        }
        assert!(!tx.send(AudioItem::Window(Vec::new())), "dropped");
        assert!(tx.send(AudioItem::Flush), "a note still goes in");
        assert_eq!(rx.backlog(), CAPACITY + 1);
    }

    #[test]
    fn sending_to_a_loop_that_has_gone_is_false_and_leaves_no_backlog() {
        let (tx, rx) = channel();
        let queued = Arc::clone(&tx.queued);
        drop(rx);
        assert!(!tx.send(AudioItem::Window(vec![0.0])));
        assert_eq!(queued.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn the_stream_position_counts_the_windows_the_queue_took_and_nothing_else() {
        let (tx, _rx) = channel();
        assert_eq!(tx.stream_us(), 0);
        assert!(tx.send(AudioItem::Window(vec![0.0])));
        assert!(tx.send(AudioItem::Window(vec![0.0])));
        assert!(tx.send(AudioItem::Flush));
        assert!(tx.send(AudioItem::End));
        // A window is 512 samples, 32 ms at 16 kHz.
        assert_eq!(tx.stream_us(), 64_000);
        assert_eq!(tx.clone().stream_us(), 64_000, "clones share the count");
    }

    #[test]
    fn a_window_dropped_on_a_full_queue_or_sent_to_a_gone_loop_is_not_on_the_stream() {
        let (tx, _rx) = channel();
        for _ in 0..CAPACITY {
            assert!(tx.send(AudioItem::Window(Vec::new())));
        }
        let full = tx.stream_us();
        assert!(!tx.send(AudioItem::Window(Vec::new())));
        assert_eq!(tx.stream_us(), full);
        let (gone, loop_gone) = channel();
        drop(loop_gone);
        assert!(!gone.send(AudioItem::Window(vec![0.0])));
        assert_eq!(gone.stream_us(), 0);
    }

    #[test]
    fn a_window_is_due_when_its_audio_has_gone_by() {
        assert_eq!(due_after(0), Duration::ZERO);
        // 31.25 windows make a second.
        assert_eq!(due_after(125), Duration::from_secs(4));
    }

    #[test]
    fn a_recording_plays_as_whole_windows_then_ends() {
        let (tx, rx) = channel();
        let samples = vec![0.5_f32; WINDOW + 3];
        let player = play_wav(samples, false, tx);
        let mut windows_seen = 0;
        loop {
            match rx.recv_timeout(Duration::from_secs(5)) {
                Ok(AudioItem::Window(window)) => {
                    assert_eq!(window.len(), WINDOW);
                    windows_seen += 1;
                }
                Ok(AudioItem::End) => break,
                other => panic!("unexpected {other:?}"),
            }
        }
        assert_eq!(windows_seen, 2);
        assert!(player.join().is_ok());
    }

    #[test]
    fn an_empty_recording_only_ends() {
        let (tx, rx) = channel();
        let player = play_wav(Vec::new(), false, tx);
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)), Ok(AudioItem::End));
        assert!(player.join().is_ok());
    }
}
