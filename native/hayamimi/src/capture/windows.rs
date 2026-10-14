//! Windows capture through WASAPI: the microphone, and the loopback of an
//! output device (what the machine plays).
//!
//! The stream is read by polling, which loopback capture supports on every
//! Windows version, in shared mode with the system's own format conversion.
//! A loopback device delivers nothing while nothing plays, so the gap is
//! filled with silence: the detector ends an utterance only on audio it is fed.

use std::collections::VecDeque;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use wasapi::{
    AudioCaptureClient, Device, DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat,
    deinitialize, initialize_mta,
};

use super::align::Packet;
use super::convert::le_f32;
use super::{CaptureError, Clock, Control, Feeder, Running, SourceKind, name_matches};

/// How often the device is read.
const POLL: Duration = Duration::from_millis(10);
/// A delivery gap longer than this is filled with silence.
const GAP: Duration = Duration::from_millis(100);
/// Bytes of one sample: the stream is 32-bit float.
const SAMPLE_BYTES: usize = 4;

pub fn open(
    kind: SourceKind,
    device: Option<&str>,
    clock: Clock,
    out: Sender<Packet>,
) -> Result<Running, CaptureError> {
    let wanted = device.map(str::to_owned);
    super::spawn_source(kind.name(), move |control| {
        run(kind, wanted.as_deref(), clock, out, control)
    })
}

fn device_error(error: impl std::fmt::Display) -> CaptureError {
    CaptureError::Device(error.to_string())
}

/// COM is initialised for the thread around the whole capture.
fn run(
    kind: SourceKind,
    wanted: Option<&str>,
    clock: Clock,
    out: Sender<Packet>,
    control: &mut Control,
) -> Result<(), CaptureError> {
    initialize_mta().ok().map_err(device_error)?;
    let result = capture(kind, wanted, clock, out, control);
    deinitialize();
    result
}

/// The device of `kind` whose name holds `wanted`, or the system's default.
/// A microphone is a capture device; the loopback of an output device is
/// taken from a render device.
fn pick(kind: SourceKind, wanted: Option<&str>) -> Result<Device, CaptureError> {
    let direction = match kind {
        SourceKind::Mic => Direction::Capture,
        SourceKind::Speaker => Direction::Render,
    };
    let enumerator = DeviceEnumerator::new().map_err(device_error)?;
    let Some(part) = wanted else {
        return enumerator
            .get_default_device(&direction)
            .map_err(|_| CaptureError::NoDevice);
    };
    let devices = enumerator
        .get_device_collection(&direction)
        .map_err(device_error)?;
    let count = devices.get_nbr_devices().map_err(device_error)?;
    for index in 0..count {
        let device = devices.get_device_at_index(index).map_err(device_error)?;
        if device
            .get_friendlyname()
            .is_ok_and(|name| name_matches(&name, Some(part)))
        {
            return Ok(device);
        }
    }
    Err(CaptureError::NoDevice)
}

fn capture(
    kind: SourceKind,
    wanted: Option<&str>,
    clock: Clock,
    out: Sender<Packet>,
    control: &mut Control,
) -> Result<(), CaptureError> {
    let device = pick(kind, wanted)?;
    let name = device.get_friendlyname().map_err(device_error)?;
    let mut client = device.get_iaudioclient().map_err(device_error)?;
    let mix = client.get_mixformat().map_err(device_error)?;
    let rate = mix.get_samplespersec();
    let channels = usize::from(mix.get_nchannels());
    let format = WaveFormat::new(32, 32, &SampleType::Float, rate as usize, channels, None);
    let (period, _) = client.get_device_period().map_err(device_error)?;
    let mode = StreamMode::PollingShared {
        autoconvert: true,
        buffer_duration_hns: period,
    };
    // A render device is read as loopback by asking for a capture stream.
    client
        .initialize_client(&format, &Direction::Capture, &mode)
        .map_err(device_error)?;
    let reader = client.get_audiocaptureclient().map_err(device_error)?;
    client.start_stream().map_err(device_error)?;
    control.opened(&name);
    let mut feeder = Feeder::new(rate, channels, clock, out);
    let result = pump(&reader, &mut feeder, rate, channels, control);
    // Stopping a stream that is going away anyway has nothing to report.
    let _ = client.stop_stream();
    result
}

/// Appends every packet the device has ready to `bytes`.
fn drain_device(reader: &AudioCaptureClient, bytes: &mut VecDeque<u8>) -> Result<(), CaptureError> {
    while reader
        .get_next_packet_size()
        .map_err(device_error)?
        .is_some_and(|frames| frames > 0)
    {
        reader
            .read_from_device_to_deque(bytes)
            .map_err(device_error)?;
    }
    Ok(())
}

/// The whole frames waiting in `bytes`, as samples.
fn take_frames(bytes: &mut VecDeque<u8>, frame_bytes: usize) -> Vec<f32> {
    let whole = bytes.len() / frame_bytes * frame_bytes;
    let raw: Vec<u8> = bytes.drain(..whole).collect();
    le_f32(&raw)
}

/// Delivers what the device has until the run stops. Returns `Ok` when the
/// run stops or nobody listens, and an error when the device fails.
fn pump(
    reader: &AudioCaptureClient,
    feeder: &mut Feeder,
    rate: u32,
    channels: usize,
    control: &Control,
) -> Result<(), CaptureError> {
    let mut bytes = VecDeque::new();
    let started = Instant::now();
    let mut fed_frames = 0_u64;
    while !control.should_stop() {
        std::thread::sleep(POLL);
        drain_device(reader, &mut bytes)?;
        let samples = take_frames(&mut bytes, channels * SAMPLE_BYTES);
        fed_frames += (samples.len() / channels) as u64;
        if !samples.is_empty() && !feeder.push(&samples) {
            return Ok(());
        }
        let due = (started.elapsed().as_secs_f64() * f64::from(rate)) as u64;
        let missing = due.saturating_sub(fed_frames);
        if missing as f64 > GAP.as_secs_f64() * f64::from(rate) {
            fed_frames += missing;
            if !feeder.push(&vec![0.0; missing as usize * channels]) {
                return Ok(());
            }
        }
    }
    Ok(())
}
