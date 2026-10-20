//! The loopback sink of `stream`: plays the planned stream to the default
//! output device and records, for each chunk, when its last sample was handed
//! to the device. The server under test hears the sound through its own
//! loopback capture, so the audio takes the whole input path.
//!
//! The time is taken in the device callback, before the device's own buffer
//! plays the samples, so a measured delay is longer than the heard one by that
//! buffer (tens of milliseconds at most), never shorter.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{ErrorKind, FromSample, SampleFormat, SizedSample, Stream, StreamConfig};

use crate::error::EvalError;
use crate::ingest::{Plan, micros_since};
use crate::log;
use crate::wav::SAMPLE_RATE;

/// How long past the end of the audio the device may take before the run is called stuck.
const SLACK: Duration = Duration::from_secs(10);

/// The whole planned stream at 16 kHz mono: `repeat` copies of the recording,
/// each followed by its silence.
pub fn render(plan: &Plan, audio: &[i16]) -> Vec<f32> {
    let total = plan.chunks.last().map_or(0, |chunk| chunk.end);
    (0..total)
        .map(|position| {
            audio
                .get(position % plan.period)
                .map_or(0.0, |sample| f32::from(*sample) / 32768.0)
        })
        .collect()
}

/// The stream read at the device's rate: linear interpolation, the same value
/// on every channel, silence after the end.
struct Voice {
    source: Vec<f32>,
    rate: u64,
    frame: u64,
}

impl Voice {
    fn new(source: Vec<f32>, rate: u32) -> Voice {
        Voice {
            source,
            rate: u64::from(rate.max(1)),
            frame: 0,
        }
    }

    fn sample_at(&self, frame: u64) -> f32 {
        let scaled = frame * u64::from(SAMPLE_RATE);
        let index = (scaled / self.rate) as usize;
        let fraction = (scaled % self.rate) as f32 / self.rate as f32;
        self.source.get(index).map_or(0.0, |current| {
            let next = self.source.get(index + 1).map_or(*current, |one| *one);
            current + (next - current) * fraction
        })
    }

    fn fill(&mut self, out: &mut [f32], channels: usize) {
        for frame in out.chunks_exact_mut(channels.max(1)) {
            frame.fill(self.sample_at(self.frame));
            self.frame += 1;
        }
    }

    /// Samples of the 16 kHz stream that the device has read.
    fn played(&self) -> usize {
        (self.frame * u64::from(SAMPLE_RATE) / self.rate) as usize
    }
}

/// Sends the time at which each chunk's last sample was read, in order.
struct Stamper {
    ends: Vec<usize>,
    next: usize,
    origin: Instant,
    sent: Sender<i64>,
}

impl Stamper {
    fn note(&mut self, played: usize) {
        while self.ends.get(self.next).is_some_and(|end| *end <= played) {
            // The receiver is gone only when the run has given up; nobody is left to tell.
            let _ = self.sent.send(micros_since(self.origin));
            self.next += 1;
        }
    }
}

/// What the device callback runs.
struct Playback {
    voice: Voice,
    stamper: Stamper,
    channels: usize,
    floats: Vec<f32>,
}

impl Playback {
    fn new(
        (plan, audio): (&Plan, &[i16]),
        (rate, channels): (u32, usize),
        (origin, sent): (Instant, Sender<i64>),
    ) -> Playback {
        Playback {
            voice: Voice::new(render(plan, audio), rate),
            stamper: Stamper {
                ends: plan.chunks.iter().map(|chunk| chunk.end).collect(),
                next: 0,
                origin,
                sent,
            },
            channels,
            floats: Vec::new(),
        }
    }

    /// Fills one buffer of the device's sample type and stamps the chunks it finished.
    fn next_buffer<T: FromSample<f32>>(&mut self, data: &mut [T]) {
        self.floats.resize(data.len(), 0.0);
        self.voice.fill(&mut self.floats, self.channels);
        for (out, value) in data.iter_mut().zip(&self.floats) {
            *out = T::from_sample_(*value);
        }
        self.stamper.note(self.voice.played());
    }
}

fn device_error(error: cpal::Error) -> EvalError {
    if matches!(error.kind(), ErrorKind::DeviceNotAvailable) {
        EvalError::NoOutputDevice
    } else {
        EvalError::Invalid(format!("output device: {error}"))
    }
}

fn stream_of<T>(
    device: &cpal::Device,
    config: StreamConfig,
    mut playback: Playback,
) -> Result<Stream, EvalError>
where
    T: SizedSample + FromSample<f32>,
{
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| playback.next_buffer(data),
            |error| {
                log::warn(
                    "eval.speaker.stream_error",
                    "worker",
                    &error.to_string(),
                    &[],
                );
            },
            None,
        )
        .map_err(device_error)
}

/// Builds the stream for the sample format the device takes.
fn build(
    device: &cpal::Device,
    format: SampleFormat,
    config: StreamConfig,
    playback: Playback,
) -> Result<Stream, EvalError> {
    match format {
        SampleFormat::F32 => stream_of::<f32>(device, config, playback),
        SampleFormat::I16 => stream_of::<i16>(device, config, playback),
        SampleFormat::U16 => stream_of::<u16>(device, config, playback),
        SampleFormat::I32 => stream_of::<i32>(device, config, playback),
        other => Err(EvalError::Invalid(format!(
            "output device: sample format {other:?}"
        ))),
    }
}

/// The longest wait for a stamp before `halt` is looked at again.
const HALT_SLICE: Duration = Duration::from_millis(100);

/// Waits for `count` stamps; a device that stops short of them is an error.
/// Once `halt` is set it returns the stamps it has.
fn collect(
    stamps: &Receiver<i64>,
    count: usize,
    within: Duration,
    halt: &AtomicBool,
) -> Result<Vec<i64>, EvalError> {
    let deadline = Instant::now() + within;
    let mut sent = Vec::with_capacity(count);
    while sent.len() < count && !halt.load(Ordering::Relaxed) {
        let left = deadline.saturating_duration_since(Instant::now());
        match stamps.recv_timeout(left.min(HALT_SLICE)) {
            Ok(stamp) => sent.push(stamp),
            Err(mpsc::RecvTimeoutError::Timeout) if !left.is_zero() => {}
            Err(_) => {
                return Err(EvalError::Invalid(format!(
                    "the output device stopped after {} of {count} chunks",
                    sent.len()
                )));
            }
        }
    }
    Ok(sent)
}

/// Plays `plan` to the default output device and returns, for each chunk, the
/// microseconds since `origin` at which its last sample was handed over. A
/// machine with no output device gives `EvalError::NoOutputDevice` before any
/// sound is made. Once `halt` is set the sound stops and the list ends at the
/// last chunk handed over.
pub fn play(
    plan: &Plan,
    audio: &[i16],
    origin: Instant,
    halt: &AtomicBool,
) -> Result<Vec<i64>, EvalError> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or(EvalError::NoOutputDevice)?;
    let supported = device.default_output_config().map_err(device_error)?;
    let config = supported.config();
    let (sender, stamps) = mpsc::channel();
    let playback = Playback::new(
        (plan, audio),
        (config.sample_rate, usize::from(config.channels)),
        (origin, sender),
    );
    let stream = build(&device, supported.sample_format(), config, playback)?;
    stream.play().map_err(device_error)?;
    let seconds = plan.chunks.last().map_or(0, |chunk| chunk.end) / SAMPLE_RATE as usize;
    let within = Duration::from_secs(seconds as u64) + SLACK;
    let sent = collect(&stamps, plan.chunks.len(), within, halt)?;
    drop(stream);
    Ok(sent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::plan as make_plan;

    #[test]
    fn the_stream_is_the_copies_with_their_silence() {
        let plan = make_plan(3, &[], 2, 2);
        let audio = [16384_i16, -16384, 32767];
        let rendered = render(&plan, &audio);
        assert_eq!(rendered.len(), 10);
        assert_eq!(rendered[..3], [0.5, -0.5, 32767.0 / 32768.0]);
        assert_eq!(rendered[3..5], [0.0, 0.0]);
        assert_eq!(rendered[5..8], rendered[..3]);
    }

    #[test]
    fn an_empty_plan_renders_nothing() {
        assert!(render(&make_plan(3, &[], 0, 2), &[1, 2, 3]).is_empty());
    }

    #[test]
    fn a_device_at_the_stream_rate_reads_it_unchanged() {
        let mut voice = Voice::new(vec![0.1, 0.2, 0.3], SAMPLE_RATE);
        let mut out = [0.0_f32; 6];
        voice.fill(&mut out, 2);
        assert_eq!(out, [0.1, 0.1, 0.2, 0.2, 0.3, 0.3]);
        assert_eq!(voice.played(), 3);
    }

    #[test]
    fn a_faster_device_interpolates_between_samples() {
        let mut voice = Voice::new(vec![0.0, 1.0], SAMPLE_RATE * 2);
        let mut out = [9.0_f32; 3];
        voice.fill(&mut out, 1);
        assert_eq!(out, [0.0, 0.5, 1.0]);
    }

    #[test]
    fn the_end_of_the_stream_is_silence() {
        let mut voice = Voice::new(vec![1.0], SAMPLE_RATE);
        let mut out = [9.0_f32; 3];
        voice.fill(&mut out, 1);
        assert_eq!(out, [1.0, 0.0, 0.0]);
    }

    #[test]
    fn a_device_of_48_khz_plays_three_samples_for_each_stream_sample() {
        let mut voice = Voice::new(vec![0.0; 100], 48_000);
        voice.fill(&mut [0.0_f32; 300], 1);
        assert_eq!(voice.played(), 100);
    }

    #[test]
    fn each_chunk_is_stamped_once_when_the_device_has_read_its_last_sample() -> Result<(), String> {
        let plan = make_plan(10, &[], 1, 0);
        let (sender, stamps) = mpsc::channel();
        let mut playback = Playback::new(
            (&plan, &[0_i16; 10]),
            (SAMPLE_RATE, 1),
            (Instant::now(), sender),
        );
        let mut buffer = [0.0_f32; 4];
        playback.next_buffer(&mut buffer);
        assert!(
            stamps.try_recv().is_err(),
            "the chunk ends at 10, 4 are read"
        );
        playback.next_buffer(&mut [0.0_f32; 8]);
        stamps.try_recv().map_err(|error| error.to_string())?;
        assert!(stamps.try_recv().is_err(), "one stamp for one chunk");
        Ok(())
    }

    #[test]
    fn a_buffer_of_integer_samples_is_filled_from_the_floats() {
        let plan = make_plan(2, &[], 1, 0);
        let (sender, _stamps) = mpsc::channel();
        let mut playback = Playback::new(
            (&plan, &[16384_i16, -16384]),
            (SAMPLE_RATE, 1),
            (Instant::now(), sender),
        );
        let mut buffer = [7_i16; 2];
        playback.next_buffer(&mut buffer);
        assert_eq!(buffer, [16384, -16384]);
    }

    #[test]
    fn a_collect_that_gets_every_stamp_returns_them_in_order() -> Result<(), EvalError> {
        let (sender, stamps) = mpsc::channel();
        for stamp in [5, 6, 7] {
            sender
                .send(stamp)
                .map_err(|error| EvalError::Invalid(error.to_string()))?;
        }
        assert_eq!(
            collect(
                &stamps,
                3,
                Duration::from_millis(50),
                &AtomicBool::new(false)
            )?,
            vec![5, 6, 7]
        );
        Ok(())
    }

    #[test]
    fn a_collect_stops_at_a_halt_with_the_stamps_it_has() -> Result<(), EvalError> {
        let (sender, stamps) = mpsc::channel();
        assert!(sender.send(1).is_ok());
        let halt = AtomicBool::new(true);
        let began = Instant::now();
        let got = collect(&stamps, 2, Duration::from_secs(30), &halt)?;
        assert!(got.is_empty());
        assert!(began.elapsed() < Duration::from_secs(5));
        Ok(())
    }

    #[test]
    fn a_collect_that_runs_out_of_time_names_how_far_it_got() {
        let (sender, stamps) = mpsc::channel();
        assert!(sender.send(1).is_ok());
        let said = collect(
            &stamps,
            2,
            Duration::from_millis(30),
            &AtomicBool::new(false),
        )
        .err()
        .map(|error| error.to_string());
        assert!(
            said.as_deref().is_some_and(|text| text.contains("1 of 2")),
            "{said:?}"
        );
    }

    #[test]
    fn a_missing_device_is_its_own_error() {
        let said = device_error(cpal::Error::from(ErrorKind::DeviceNotAvailable));
        assert!(matches!(said, EvalError::NoOutputDevice));
    }
}
