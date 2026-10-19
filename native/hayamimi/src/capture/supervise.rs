//! Opens the capture sources the input asked for, notices one that is lost,
//! opens it again, and reports when the set of working sources changes.
//!
//! What it reports follows the design for a missing resource: a source that
//! is not there never stops the run. With two sources asked for and one
//! working the run goes on with it (`Single`); with none working it goes on
//! with `/ingest` and the wav file (`NoSource`); when the missing sources
//! work again the stage is cleared.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::combine::{self, Alive, Inputs};
use super::{CaptureError, Clock, Running, STOP_POLL, SourceKind};
use crate::feed::AudioTx;

/// How long to wait before trying a missing source again.
const RETRY: Duration = Duration::from_secs(2);

/// A source the input asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    pub kind: SourceKind,
    /// Part of the device name; `None` takes the system's default.
    pub device: Option<String>,
}

/// How many of the asked-for sources work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// All of them.
    Full,
    /// Some, not all.
    Single,
    /// None.
    NoSource,
}

impl Level {
    pub fn of(wanted: usize, working: usize) -> Level {
        if working >= wanted {
            Level::Full
        } else if working == 0 {
            Level::NoSource
        } else {
            Level::Single
        }
    }
}

/// What the supervisor says happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A source opened this device.
    Opened { kind: SourceKind, device: String },
    /// Some of the asked-for sources work, not all.
    Single,
    /// None of the asked-for sources works.
    NoSource,
    /// Every asked-for source works again.
    Cleared,
}

/// The event for a move from one level to another, if there is one.
pub fn transition(before: Level, after: Level) -> Option<Event> {
    match (before == after, after) {
        (true, _) => None,
        (false, Level::Full) => Some(Event::Cleared),
        (false, Level::Single) => Some(Event::Single),
        (false, Level::NoSource) => Some(Event::NoSource),
    }
}

/// Tries to open one source.
pub type Opener = Box<dyn FnMut(SourceKind, Option<&str>) -> Result<Running, CaptureError> + Send>;

/// The state of the sources of one input.
pub struct Supervisor {
    specs: Vec<Spec>,
    running: Vec<Option<Running>>,
    level: Level,
    next_retry: Instant,
    alive: Arc<Alive>,
    events: Sender<Event>,
    open: Opener,
}

impl Supervisor {
    pub fn new(specs: Vec<Spec>, alive: Arc<Alive>, events: Sender<Event>, open: Opener) -> Self {
        let running = specs.iter().map(|_| None).collect();
        Supervisor {
            specs,
            running,
            // The first pass reports a fall only when a source is missing.
            level: Level::Full,
            next_retry: Instant::now(),
            alive,
            events,
            open,
        }
    }

    fn say(&self, event: Event) {
        // The reporter ends with the run; an event nobody reads is not an error.
        let _ = self.events.send(event);
    }

    /// Lets go of the sources that were lost.
    fn reap_lost(&mut self) {
        for (spec, slot) in self.specs.iter().zip(&mut self.running) {
            if slot.as_ref().is_some_and(Running::is_lost) {
                *slot = None;
                self.alive.set(spec.kind, false);
            }
        }
    }

    /// Tries to open the sources that are not working.
    fn reopen_missing(&mut self) {
        for (spec, slot) in self.specs.iter().zip(&mut self.running) {
            if slot.is_some() {
                continue;
            }
            let Ok(running) = (self.open)(spec.kind, spec.device.as_deref()) else {
                continue;
            };
            self.alive.set(spec.kind, true);
            let _ = self.events.send(Event::Opened {
                kind: spec.kind,
                device: running.device.clone(),
            });
            *slot = Some(running);
        }
    }

    fn working(&self) -> usize {
        self.running.iter().flatten().count()
    }

    /// One look at the sources: lets go of lost ones, tries the missing ones
    /// when the retry wait is over, and reports a change of level.
    pub fn pass(&mut self, now: Instant) {
        self.reap_lost();
        if now >= self.next_retry {
            self.reopen_missing();
            self.next_retry = now + RETRY;
        }
        let level = Level::of(self.specs.len(), self.working());
        if let Some(event) = transition(self.level, level) {
            self.say(event);
        }
        self.level = level;
    }
}

/// The running capture: its threads end when this is dropped.
pub struct Capture {
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for thread in self.threads.drain(..) {
            // A thread that panicked has nothing left to release.
            let _ = thread.join();
        }
    }
}

/// The opener of this platform, sending each kind's windows to its own channel.
fn platform_opener(
    clock: Clock,
    mic: Sender<super::align::Packet>,
    speaker: Sender<super::align::Packet>,
) -> Opener {
    Box::new(move |kind, device| {
        let out = match kind {
            SourceKind::Mic => mic.clone(),
            SourceKind::Speaker => speaker.clone(),
        };
        super::open(kind, device, clock, out)
    })
}

/// Looks at the sources until `stop`, every [`STOP_POLL`].
fn watch(mut supervisor: Supervisor, stop: &AtomicBool) {
    while !stop.load(Ordering::Relaxed) {
        supervisor.pass(Instant::now());
        std::thread::sleep(STOP_POLL);
    }
}

/// Starts capturing `specs` into `tx`. The first look at the sources is made
/// before this returns, so that the events of the start come first.
pub fn start(specs: Vec<Spec>, tx: AudioTx, events: Sender<Event>) -> std::io::Result<Capture> {
    let clock = Clock::start();
    let (mic, mic_rx) = mpsc::channel();
    let (speaker, speaker_rx) = mpsc::channel();
    let alive = Arc::new(Alive::default());
    let stop = Arc::new(AtomicBool::new(false));
    let mut supervisor = Supervisor::new(
        specs,
        Arc::clone(&alive),
        events,
        platform_opener(clock, mic, speaker),
    );
    supervisor.pass(Instant::now());
    let combiner = combine::spawn(
        Inputs {
            mic: mic_rx,
            speaker: speaker_rx,
        },
        alive,
        clock,
        tx,
        Arc::clone(&stop),
    )?;
    let watcher_stop = Arc::clone(&stop);
    let watcher = std::thread::Builder::new()
        .name(String::from("capture-supervise"))
        .spawn(move || watch(supervisor, &watcher_stop))?;
    Ok(Capture {
        stop,
        threads: vec![combiner, watcher],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::sync::mpsc::Receiver;

    fn fake_source(name: &'static str) -> Result<Running, CaptureError> {
        super::super::spawn_source("fake", move |control| {
            control.opened(name);
            control.hold();
            Ok(())
        })
    }

    /// An opener that opens the kinds in `present` and refuses the others.
    fn opener(present: Arc<Mutex<Vec<SourceKind>>>) -> Opener {
        Box::new(move |kind, _| {
            let here = present.lock().is_ok_and(|kinds| kinds.contains(&kind));
            if here {
                fake_source(kind.name())
            } else {
                Err(CaptureError::NoDevice)
            }
        })
    }

    fn spec(kind: SourceKind) -> Spec {
        Spec { kind, device: None }
    }

    fn rig(
        wanted: &[SourceKind],
        present: &[SourceKind],
    ) -> (Supervisor, Arc<Mutex<Vec<SourceKind>>>, Receiver<Event>) {
        let (events, rx) = mpsc::channel();
        let present = Arc::new(Mutex::new(present.to_vec()));
        let supervisor = Supervisor::new(
            wanted.iter().copied().map(spec).collect(),
            Arc::new(Alive::default()),
            events,
            opener(Arc::clone(&present)),
        );
        (supervisor, present, rx)
    }

    fn drained(rx: &Receiver<Event>) -> Vec<Event> {
        rx.try_iter().collect()
    }

    #[test]
    fn the_levels_follow_how_many_work() {
        assert_eq!(Level::of(2, 2), Level::Full);
        assert_eq!(Level::of(2, 1), Level::Single);
        assert_eq!(Level::of(2, 0), Level::NoSource);
        assert_eq!(Level::of(1, 1), Level::Full);
        assert_eq!(Level::of(1, 0), Level::NoSource);
    }

    #[test]
    fn a_move_between_levels_is_one_event_and_no_move_is_none() {
        assert_eq!(transition(Level::Full, Level::Full), None);
        assert_eq!(transition(Level::Full, Level::Single), Some(Event::Single));
        assert_eq!(
            transition(Level::Single, Level::NoSource),
            Some(Event::NoSource)
        );
        assert_eq!(
            transition(Level::NoSource, Level::Full),
            Some(Event::Cleared)
        );
        assert_eq!(transition(Level::NoSource, Level::NoSource), None);
    }

    #[test]
    fn both_devices_present_open_and_say_nothing_fell() {
        let both = [SourceKind::Mic, SourceKind::Speaker];
        let (mut supervisor, _present, rx) = rig(&both, &both);
        supervisor.pass(Instant::now());
        let events = drained(&rx);
        assert_eq!(events.len(), 2);
        assert!(
            events
                .iter()
                .all(|event| matches!(event, Event::Opened { .. }))
        );
    }

    #[test]
    fn a_missing_speaker_falls_to_single_once_and_the_microphone_still_opens() {
        let both = [SourceKind::Mic, SourceKind::Speaker];
        let (mut supervisor, _present, rx) = rig(&both, &[SourceKind::Mic]);
        let start = Instant::now();
        supervisor.pass(start);
        supervisor.pass(start + Duration::from_millis(50));
        let events = drained(&rx);
        assert_eq!(
            events,
            vec![
                Event::Opened {
                    kind: SourceKind::Mic,
                    device: String::from("mic"),
                },
                Event::Single,
            ]
        );
    }

    #[test]
    fn no_device_at_all_falls_to_no_source() {
        let both = [SourceKind::Mic, SourceKind::Speaker];
        let (mut supervisor, _present, rx) = rig(&both, &[]);
        supervisor.pass(Instant::now());
        assert_eq!(drained(&rx), vec![Event::NoSource]);
    }

    #[test]
    fn a_single_source_input_with_no_device_falls_to_no_source() {
        let (mut supervisor, _present, rx) = rig(&[SourceKind::Mic], &[]);
        supervisor.pass(Instant::now());
        assert_eq!(drained(&rx), vec![Event::NoSource]);
    }

    #[test]
    fn a_device_that_comes_back_clears_the_fall_after_the_retry_wait() -> Result<(), String> {
        let (mut supervisor, present, rx) = rig(&[SourceKind::Mic], &[]);
        let start = Instant::now();
        supervisor.pass(start);
        assert_eq!(drained(&rx), vec![Event::NoSource]);
        present
            .lock()
            .map_err(|_| String::from("poisoned"))?
            .push(SourceKind::Mic);
        // Before the wait is over the device is not tried again.
        supervisor.pass(start + Duration::from_millis(500));
        assert!(drained(&rx).is_empty());
        supervisor.pass(start + RETRY + Duration::from_millis(1));
        let events = drained(&rx);
        assert_eq!(events.len(), 2);
        assert_eq!(events[1], Event::Cleared);
        Ok(())
    }

    #[test]
    fn mix_speaker_failure_falls_back_once_to_mic_only() -> Result<(), Box<dyn std::error::Error>> {
        use super::super::align::Packet;
        use crate::feed::{self, AudioItem};

        let (events, rx) = mpsc::channel();
        let alive = Arc::new(Alive::default());
        let mut supervisor = Supervisor::new(
            vec![spec(SourceKind::Mic), spec(SourceKind::Speaker)],
            Arc::clone(&alive),
            events,
            Box::new(|kind, _| match kind {
                SourceKind::Mic => fake_source("mic"),
                SourceKind::Speaker => super::super::spawn_source("fake", |control| {
                    control.opened("speaker");
                    Err(CaptureError::Device(String::from("loopback lost")))
                }),
            }),
        );
        let start = Instant::now();
        supervisor.pass(start);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !supervisor.running.iter().flatten().any(Running::is_lost)
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        // Passes before the retry wait is over must not say it again.
        supervisor.pass(start + Duration::from_millis(50));
        supervisor.pass(start + Duration::from_millis(100));
        assert!(alive.is(SourceKind::Mic));
        assert!(!alive.is(SourceKind::Speaker));
        let said = drained(&rx);
        assert_eq!(
            said.iter().filter(|event| **event == Event::Single).count(),
            1
        );

        // From here the combiner passes the microphone windows on alone.
        let (mic_tx, mic_rx) = mpsc::channel();
        let (_speaker_tx, speaker_rx) = mpsc::channel();
        let (tx, out) = feed::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let combiner = combine::spawn(
            Inputs {
                mic: mic_rx,
                speaker: speaker_rx,
            },
            Arc::clone(&alive),
            Clock::start(),
            tx,
            Arc::clone(&stop),
        )?;
        for (end_ns, value) in [(1_000, 0.25_f32), (2_000, 0.5)] {
            mic_tx.send(Packet {
                end_ns,
                samples: vec![value; 4],
            })?;
        }
        let wait = Duration::from_secs(5);
        assert_eq!(out.recv_timeout(wait)?, AudioItem::Window(vec![0.25; 4]));
        assert_eq!(out.recv_timeout(wait)?, AudioItem::Window(vec![0.5; 4]));
        stop.store(true, Ordering::Relaxed);
        assert!(combiner.join().is_ok());
        Ok(())
    }

    #[test]
    fn a_lost_source_is_noticed_and_the_level_falls() -> Result<(), Box<dyn std::error::Error>> {
        let (events, rx) = mpsc::channel();
        let alive = Arc::new(Alive::default());
        let mut supervisor = Supervisor::new(
            vec![spec(SourceKind::Mic)],
            Arc::clone(&alive),
            events,
            Box::new(|_, _| {
                super::super::spawn_source("fake", |control| {
                    control.opened("fake");
                    Err(CaptureError::Device(String::from("unplugged")))
                })
            }),
        );
        let start = Instant::now();
        supervisor.pass(start);
        assert!(alive.is(SourceKind::Mic));
        let deadline = Instant::now() + Duration::from_secs(5);
        while supervisor.running.iter().flatten().any(|r| !r.is_lost()) && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        supervisor.pass(start + Duration::from_millis(50));
        assert!(!alive.is(SourceKind::Mic));
        assert_eq!(rx.try_iter().last(), Some(Event::NoSource));
        Ok(())
    }
}
