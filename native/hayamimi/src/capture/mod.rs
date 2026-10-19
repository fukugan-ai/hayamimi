//! Audio capture from the devices of this machine.
//!
//! A source (the microphone, or what the machine plays) runs on a thread of
//! its own, converts what the device delivers to 16 kHz mono windows and
//! sends them stamped with the moment they were captured. A combiner passes
//! one source on, or adds the two, to the recognition loop. A supervisor
//! opens the sources, notices one that was lost and opens it again, and says
//! so when the set of working sources changes.

pub mod align;
pub mod combine;
pub mod convert;
pub mod supervise;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(not(any(target_os = "linux", windows)))]
mod other;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(not(any(target_os = "linux", windows)))]
use other as platform;
#[cfg(windows)]
use windows as platform;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use hayamimi_asr::audio::{SAMPLE_RATE, WINDOW};

use align::Packet;
use convert::Chain;

/// How long a source gets to open its device.
const OPEN_TIMEOUT: Duration = Duration::from_secs(15);
/// How often a source thread looks at its stop flag.
pub const STOP_POLL: Duration = Duration::from_millis(50);
/// The length of one window.
const WINDOW_NS: u64 = WINDOW as u64 * 1_000_000_000 / SAMPLE_RATE as u64;

/// Which audio a source takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    /// A microphone.
    Mic,
    /// What an output device plays.
    Speaker,
}

impl SourceKind {
    /// The name in log events.
    pub fn name(self) -> &'static str {
        match self {
            SourceKind::Mic => "mic",
            SourceKind::Speaker => "speaker",
        }
    }
}

/// Why a source did not open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureError {
    /// This platform cannot capture this kind of audio.
    Unsupported,
    /// No device matches, or there is no device at all.
    NoDevice,
    /// The device is there and did not open or stopped.
    Device(String),
}

/// The run's time zero, so that windows of different sources can be compared.
#[derive(Debug, Clone, Copy)]
pub struct Clock(Instant);

impl Clock {
    pub fn start() -> Clock {
        Clock(Instant::now())
    }

    pub fn now_ns(&self) -> u64 {
        self.0.elapsed().as_nanos() as u64
    }
}

/// Turns the buffers of one device into stamped windows.
pub struct Feeder {
    chain: Chain,
    clock: Clock,
    out: Sender<Packet>,
}

impl Feeder {
    pub fn new(rate: u32, channels: usize, clock: Clock, out: Sender<Packet>) -> Feeder {
        Feeder {
            chain: Chain::new(rate, channels, SAMPLE_RATE),
            clock,
            out,
        }
    }

    /// Takes one buffer of interleaved samples. Returns whether anyone still
    /// listens. The last window of the buffer is stamped now; the ones before
    /// it, one window earlier each.
    pub fn push(&mut self, interleaved: &[f32]) -> bool {
        let windows = self.chain.push(interleaved);
        let now = self.clock.now_ns();
        let count = windows.len() as u64;
        for (index, samples) in windows.into_iter().enumerate() {
            let behind = count - 1 - index as u64;
            let end_ns = now.saturating_sub(behind * WINDOW_NS);
            if self.out.send(Packet { end_ns, samples }).is_err() {
                return false;
            }
        }
        true
    }
}

/// What a source thread uses to report and to learn when to stop.
pub struct Control {
    ready: Sender<Result<String, CaptureError>>,
    stop: Arc<AtomicBool>,
    lost: Arc<AtomicBool>,
    opened: bool,
}

impl Control {
    /// Says that the device is open and delivering, and which one it is.
    pub fn opened(&mut self, device: &str) {
        self.opened = true;
        // The opener gave up waiting when this fails, and the stop flag ends the thread.
        let _ = self.ready.send(Ok(device.to_owned()));
    }

    pub fn should_stop(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    /// A handle for a stream callback to say that the device went away.
    pub fn lost_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.lost)
    }

    pub fn is_lost(&self) -> bool {
        self.lost.load(Ordering::Relaxed)
    }

    /// Waits until the run stops this source or the device is lost.
    pub fn hold(&self) {
        while !self.should_stop() && !self.is_lost() {
            std::thread::sleep(STOP_POLL);
        }
    }
}

/// A source that is capturing. Dropping it stops the thread.
pub struct Running {
    pub device: String,
    stop: Arc<AtomicBool>,
    lost: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Running {
    /// Whether the device went away or the thread ended on an error.
    pub fn is_lost(&self) -> bool {
        self.lost.load(Ordering::Relaxed)
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            // A source thread that panicked has nothing left to release.
            let _ = thread.join();
        }
    }
}

/// Runs `body` on a thread of its own and waits until it reports the device
/// open, or fails before that.
fn spawn_source<F>(name: &str, body: F) -> Result<Running, CaptureError>
where
    F: FnOnce(&mut Control) -> Result<(), CaptureError> + Send + 'static,
{
    let (ready, ready_rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let lost = Arc::new(AtomicBool::new(false));
    let mut control = Control {
        ready,
        stop: Arc::clone(&stop),
        lost: Arc::clone(&lost),
        opened: false,
    };
    let thread = std::thread::Builder::new()
        .name(format!("capture-{name}"))
        .spawn(move || finish(body(&mut control), &control))
        .map_err(|error| CaptureError::Device(error.to_string()))?;
    let device = match ready_rx.recv_timeout(OPEN_TIMEOUT) {
        Ok(Ok(device)) => device,
        Ok(Err(error)) => return Err(error),
        Err(_) => return Err(CaptureError::Device(String::from("open timed out"))),
    };
    Ok(Running {
        device,
        stop,
        lost,
        thread: Some(thread),
    })
}

/// The end of a source thread: an error before the device opened goes to the
/// opener; one after it marks the device lost.
fn finish(result: Result<(), CaptureError>, control: &Control) {
    match result {
        Err(error) if !control.opened => {
            // The opener may have stopped waiting; then nobody needs the answer.
            let _ = control.ready.send(Err(error));
        }
        Err(_) => control.lost.store(true, Ordering::Relaxed),
        Ok(()) if !control.opened => {
            let _ = control.ready.send(Err(CaptureError::NoDevice));
        }
        Ok(()) => {}
    }
}

/// Opens one source on this platform, delivering windows to `out`.
pub fn open(
    kind: SourceKind,
    device: Option<&str>,
    clock: Clock,
    out: Sender<Packet>,
) -> Result<Running, CaptureError> {
    platform::open(kind, device, clock, out)
}

/// Whether `name` matches the `wanted` part of a device name, ignoring case.
/// No `wanted` matches every device.
pub fn name_matches(name: &str, wanted: Option<&str>) -> bool {
    wanted.is_none_or(|part| name.to_lowercase().contains(&part.to_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_part_of_the_name_matches_without_regard_to_case() {
        assert!(name_matches("USB Microphone (Blue)", Some("blue")));
        assert!(name_matches("USB Microphone (Blue)", None));
        assert!(!name_matches("USB Microphone (Blue)", Some("realtek")));
    }

    #[test]
    fn windows_of_one_buffer_are_stamped_a_window_apart_ending_now() {
        let (out, packets) = mpsc::channel();
        let clock = Clock::start();
        // The run must be older than the two windows stamped before "now".
        std::thread::sleep(Duration::from_millis(80));
        let mut feeder = Feeder::new(16_000, 1, clock, out);
        assert!(feeder.push(&vec![0.0; WINDOW * 3]));
        let stamps: Vec<u64> = packets.try_iter().map(|packet| packet.end_ns).collect();
        assert_eq!(stamps.len(), 3);
        assert_eq!(stamps[1] - stamps[0], WINDOW_NS);
        assert_eq!(stamps[2] - stamps[1], WINDOW_NS);
    }

    #[cfg(not(windows))]
    #[test]
    fn speaker_capture_rejects_non_windows_before_import_or_com() {
        let (out, packets) = mpsc::channel();
        let result = open(SourceKind::Speaker, None, Clock::start(), out);
        assert!(matches!(result, Err(CaptureError::Unsupported)));
        // No thread was started, so no window was ever sent.
        assert!(packets.try_recv().is_err());
    }

    #[test]
    fn mic_timestamp_is_taken_in_the_capture_callback() -> Result<(), Box<dyn std::error::Error>> {
        let (out, packets) = mpsc::channel();
        let clock = Clock::start();
        let mut feeder = Feeder::new(16_000, 1, clock, out);
        let before = clock.now_ns();
        // The callback runs on the device's own thread.
        let callback = std::thread::spawn(move || feeder.push(&[0.25_f32; WINDOW]));
        assert!(callback.join().map_err(|_| "callback panicked")?);
        let after = clock.now_ns();
        // The reader comes late; the stamp must not move with it.
        std::thread::sleep(Duration::from_millis(60));
        let packet = packets.try_recv()?;
        assert!(
            (before..=after).contains(&packet.end_ns),
            "{} not in {before}..={after}",
            packet.end_ns
        );
        assert!(packet.samples.iter().all(|s| (s - 0.25).abs() < 1e-6));
        Ok(())
    }

    #[test]
    fn capture_timestamps_keep_audio_spacing_when_backend_returns_a_burst() {
        let (out, packets) = mpsc::channel();
        let clock = Clock::start();
        // The run must be older than the two windows stamped before "now".
        std::thread::sleep(Duration::from_millis(80));
        let mut feeder = Feeder::new(16_000, 1, clock, out);
        // A burst: three windows in one delivery.
        assert!(feeder.push(&[0.0_f32; WINDOW * 3]));
        let burst: Vec<u64> = packets.try_iter().map(|p| p.end_ns).collect();
        assert_eq!(burst.len(), 3);
        assert_eq!(burst[1], burst[0] + WINDOW_NS);
        assert_eq!(burst[2], burst[1] + WINDOW_NS);
        // A delivery after a real pause is stamped when it happened, not packed behind the burst.
        std::thread::sleep(Duration::from_millis(100));
        assert!(feeder.push(&[0.0_f32; WINDOW]));
        let later: Vec<u64> = packets.try_iter().map(|p| p.end_ns).collect();
        assert_eq!(later.len(), 1);
        assert!(later[0] >= burst[2] + 100_000_000, "{later:?} {burst:?}");
    }

    #[test]
    fn a_buffer_shorter_than_a_window_sends_nothing_and_a_closed_listener_is_said() {
        let (out, packets) = mpsc::channel();
        let mut feeder = Feeder::new(16_000, 1, Clock::start(), out);
        assert!(feeder.push(&[0.0; 10]));
        assert!(packets.try_recv().is_err());
        drop(packets);
        assert!(!feeder.push(&vec![0.0; WINDOW]));
    }

    #[test]
    fn a_source_that_fails_before_opening_hands_the_error_to_the_opener() {
        let result = spawn_source("test", |_| Err(CaptureError::NoDevice));
        assert!(matches!(result, Err(CaptureError::NoDevice)));
    }

    #[test]
    fn a_source_that_ends_without_opening_is_a_missing_device() {
        let result = spawn_source("test", |_| Ok(()));
        assert!(matches!(result, Err(CaptureError::NoDevice)));
    }

    #[test]
    fn an_open_source_reports_its_device_and_stops_when_dropped() -> Result<(), CaptureError> {
        let running = spawn_source("test", |control| {
            control.opened("fake device");
            control.hold();
            Ok(())
        })?;
        assert_eq!(running.device, "fake device");
        assert!(!running.is_lost());
        drop(running);
        Ok(())
    }

    #[test]
    fn a_source_that_fails_after_opening_is_marked_lost() -> Result<(), CaptureError> {
        let running = spawn_source("test", |control| {
            control.opened("fake device");
            Err(CaptureError::Device(String::from("unplugged")))
        })?;
        let deadline = Instant::now() + Duration::from_secs(5);
        while !running.is_lost() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(running.is_lost());
        Ok(())
    }
}
