//! The thread that turns the windows of the working sources into the one
//! stream the recognition loop reads.
//!
//! With the microphone working, its windows are the clock and, when the
//! speaker side works too, the speaker window of the same moment is added.
//! With only the speaker side working, its windows pass on as they are.
//! Which sources work is read from flags the supervisor sets, so a source
//! that is lost or comes back changes the output at the next window.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::time::Duration;

use super::align::{Aligner, Packet, TOLERANCE_NS, mix};
use super::{Clock, SourceKind};
use crate::feed::{AudioItem, AudioTx};

/// How long to wait for a window before looking at the flags again.
const POLL: Duration = Duration::from_millis(50);
/// How long to sleep when no source works.
const IDLE: Duration = Duration::from_millis(20);

/// Which sources work now.
#[derive(Debug, Default)]
pub struct Alive {
    mic: AtomicBool,
    speaker: AtomicBool,
}

impl Alive {
    fn flag(&self, kind: SourceKind) -> &AtomicBool {
        match kind {
            SourceKind::Mic => &self.mic,
            SourceKind::Speaker => &self.speaker,
        }
    }

    pub fn set(&self, kind: SourceKind, working: bool) {
        self.flag(kind).store(working, Ordering::Relaxed);
    }

    pub fn is(&self, kind: SourceKind) -> bool {
        self.flag(kind).load(Ordering::Relaxed)
    }
}

/// The windows of each source.
pub struct Inputs {
    pub mic: Receiver<Packet>,
    pub speaker: Receiver<Packet>,
}

/// Gathers the speaker windows that can still be paired with `target`,
/// waiting until the end of the tolerance for the one that is not here yet.
fn collect_speaker(aligner: &mut Aligner, speaker: &Receiver<Packet>, target: u64, clock: Clock) {
    for packet in speaker.try_iter() {
        aligner.absorb(packet);
    }
    let upper = target.saturating_add(TOLERANCE_NS);
    while !aligner.has_candidate_from(target) && !aligner.has_later_than(target) {
        let Some(wait) = upper.checked_sub(clock.now_ns()) else {
            return;
        };
        match speaker.recv_timeout(Duration::from_nanos(wait)) {
            Ok(packet) => aligner.absorb(packet),
            Err(_) => return,
        }
    }
}

/// Passes on one microphone window, with the speaker window of its moment added when that side works.
fn step_with_mic(
    inputs: &Inputs,
    alive: &Alive,
    aligner: &mut Aligner,
    clock: Clock,
    tx: &AudioTx,
) {
    let Ok(window) = inputs.mic.recv_timeout(POLL) else {
        return;
    };
    let paired = if alive.is(SourceKind::Speaker) {
        collect_speaker(aligner, &inputs.speaker, window.end_ns, clock);
        aligner.take_closest(window.end_ns)
    } else {
        inputs.speaker.try_iter().for_each(drop);
        None
    };
    // A refused window is dropped on purpose: see `AudioTx::send`.
    tx.send(AudioItem::Window(mix(&window.samples, paired.as_ref())));
}

/// Passes on one speaker window as it is.
fn step_speaker_only(inputs: &Inputs, tx: &AudioTx) {
    let Ok(window) = inputs.speaker.recv_timeout(POLL) else {
        return;
    };
    tx.send(AudioItem::Window(window.samples));
}

/// Runs until `stop` is set or the recognition loop has gone. A window the
/// full queue refused is dropped, which keeps a live input near live time.
pub fn run(inputs: &Inputs, alive: &Alive, clock: Clock, tx: &AudioTx, stop: &AtomicBool) {
    let mut aligner = Aligner::default();
    while !stop.load(Ordering::Relaxed) {
        if alive.is(SourceKind::Mic) {
            step_with_mic(inputs, alive, &mut aligner, clock, tx);
        } else if alive.is(SourceKind::Speaker) {
            inputs.mic.try_iter().for_each(drop);
            step_speaker_only(inputs, tx);
        } else {
            inputs.mic.try_iter().for_each(drop);
            inputs.speaker.try_iter().for_each(drop);
            std::thread::sleep(IDLE);
        }
    }
}

/// [`run`] on a thread of its own.
pub fn spawn(
    inputs: Inputs,
    alive: Arc<Alive>,
    clock: Clock,
    tx: AudioTx,
    stop: Arc<AtomicBool>,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new()
        .name(String::from("capture-combine"))
        .spawn(move || run(&inputs, &alive, clock, &tx, &stop))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feed;
    use std::sync::mpsc;

    const WAIT: Duration = Duration::from_secs(5);

    struct Rig {
        mic: mpsc::Sender<Packet>,
        speaker: mpsc::Sender<Packet>,
        alive: Arc<Alive>,
        stop: Arc<AtomicBool>,
        out: feed::AudioRx,
        thread: std::thread::JoinHandle<()>,
    }

    fn rig() -> std::io::Result<Rig> {
        let (mic, mic_rx) = mpsc::channel();
        let (speaker, speaker_rx) = mpsc::channel();
        let (tx, out) = feed::channel();
        let alive = Arc::new(Alive::default());
        let stop = Arc::new(AtomicBool::new(false));
        let thread = spawn(
            Inputs {
                mic: mic_rx,
                speaker: speaker_rx,
            },
            Arc::clone(&alive),
            Clock::start(),
            tx,
            Arc::clone(&stop),
        )?;
        Ok(Rig {
            mic,
            speaker,
            alive,
            stop,
            out,
            thread,
        })
    }

    impl Rig {
        fn finish(self) {
            self.stop.store(true, Ordering::Relaxed);
            assert!(self.thread.join().is_ok());
        }
    }

    fn packet(end_ns: u64, value: f32) -> Packet {
        Packet {
            end_ns,
            samples: vec![value; 4],
        }
    }

    #[test]
    fn the_microphone_alone_passes_on_unchanged() -> Result<(), Box<dyn std::error::Error>> {
        let rig = rig()?;
        rig.alive.set(SourceKind::Mic, true);
        rig.mic.send(packet(1_000, 0.25))?;
        assert_eq!(
            rig.out.recv_timeout(WAIT),
            Ok(AudioItem::Window(vec![0.25; 4]))
        );
        rig.finish();
        Ok(())
    }

    #[test]
    fn the_speaker_window_of_the_same_moment_is_added() -> Result<(), Box<dyn std::error::Error>> {
        let rig = rig()?;
        rig.alive.set(SourceKind::Mic, true);
        rig.alive.set(SourceKind::Speaker, true);
        rig.speaker.send(packet(1_000_000, 0.5))?;
        rig.mic.send(packet(1_000_000, 0.25))?;
        assert_eq!(
            rig.out.recv_timeout(WAIT),
            Ok(AudioItem::Window(vec![0.75; 4]))
        );
        rig.finish();
        Ok(())
    }

    #[test]
    fn a_speaker_window_that_never_comes_leaves_the_microphone_alone()
    -> Result<(), Box<dyn std::error::Error>> {
        let rig = rig()?;
        rig.alive.set(SourceKind::Mic, true);
        rig.alive.set(SourceKind::Speaker, true);
        // The run is a few milliseconds old, so this window is due within the tolerance.
        rig.mic.send(packet(0, 0.25))?;
        assert_eq!(
            rig.out.recv_timeout(WAIT),
            Ok(AudioItem::Window(vec![0.25; 4]))
        );
        rig.finish();
        Ok(())
    }

    #[test]
    fn the_speaker_alone_passes_on_when_the_microphone_is_lost()
    -> Result<(), Box<dyn std::error::Error>> {
        let rig = rig()?;
        rig.alive.set(SourceKind::Speaker, true);
        rig.speaker.send(packet(5, 0.5))?;
        assert_eq!(
            rig.out.recv_timeout(WAIT),
            Ok(AudioItem::Window(vec![0.5; 4]))
        );
        rig.finish();
        Ok(())
    }

    #[test]
    fn with_no_working_source_nothing_passes_and_the_thread_still_stops()
    -> Result<(), Box<dyn std::error::Error>> {
        let rig = rig()?;
        rig.mic.send(packet(1, 0.1))?;
        assert!(rig.out.recv_timeout(Duration::from_millis(150)).is_err());
        rig.finish();
        Ok(())
    }
}
