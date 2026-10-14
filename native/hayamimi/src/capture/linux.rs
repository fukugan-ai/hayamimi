//! Linux capture through cpal: the microphone. What the machine plays is not
//! captured here (that input is for Windows).

use std::sync::atomic::Ordering;
use std::sync::mpsc::Sender;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{ErrorKind, FromSample, SampleFormat, SizedSample, Stream, StreamConfig};

use super::align::Packet;
use super::{CaptureError, Clock, Control, Feeder, Running, SourceKind, name_matches};

pub fn open(
    kind: SourceKind,
    device: Option<&str>,
    clock: Clock,
    out: Sender<Packet>,
) -> Result<Running, CaptureError> {
    if kind == SourceKind::Speaker {
        return Err(CaptureError::Unsupported);
    }
    let wanted = device.map(str::to_owned);
    super::spawn_source(kind.name(), move |control| {
        capture(wanted.as_deref(), clock, out, control)
    })
}

fn device_error(error: cpal::Error) -> CaptureError {
    if matches!(error.kind(), ErrorKind::DeviceNotAvailable) {
        CaptureError::NoDevice
    } else {
        CaptureError::Device(error.to_string())
    }
}

/// The microphone whose name holds `wanted`, or the system's default.
fn pick(wanted: Option<&str>) -> Result<cpal::Device, CaptureError> {
    let host = cpal::default_host();
    let Some(part) = wanted else {
        return host.default_input_device().ok_or(CaptureError::NoDevice);
    };
    host.input_devices()
        .map_err(device_error)?
        .find(|device| {
            device
                .description()
                .is_ok_and(|found| name_matches(found.name(), Some(part)))
        })
        .ok_or(CaptureError::NoDevice)
}

/// A stream of samples of type `T` into `feeder`.
fn stream_of<T>(
    device: &cpal::Device,
    config: StreamConfig,
    mut feeder: Feeder,
    control: &Control,
) -> Result<Stream, CaptureError>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let lost = control.lost_flag();
    let mut floats: Vec<f32> = Vec::new();
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                floats.clear();
                floats.extend(data.iter().map(|sample| sample.to_sample::<f32>()));
                feeder.push(&floats);
            },
            move |_error| lost.store(true, Ordering::Relaxed),
            None,
        )
        .map_err(device_error)
}

/// Builds the stream for the sample format the device delivers.
fn build(
    device: &cpal::Device,
    format: SampleFormat,
    config: StreamConfig,
    feeder: Feeder,
    control: &Control,
) -> Result<Stream, CaptureError> {
    match format {
        SampleFormat::F32 => stream_of::<f32>(device, config, feeder, control),
        SampleFormat::I16 => stream_of::<i16>(device, config, feeder, control),
        SampleFormat::U16 => stream_of::<u16>(device, config, feeder, control),
        SampleFormat::I32 => stream_of::<i32>(device, config, feeder, control),
        other => Err(CaptureError::Device(format!("sample format {other:?}"))),
    }
}

fn capture(
    wanted: Option<&str>,
    clock: Clock,
    out: Sender<Packet>,
    control: &mut Control,
) -> Result<(), CaptureError> {
    let device = pick(wanted)?;
    let name = device
        .description()
        .map_err(device_error)?
        .name()
        .to_owned();
    let supported = device.default_input_config().map_err(device_error)?;
    let config = supported.config();
    let feeder = Feeder::new(config.sample_rate, usize::from(config.channels), clock, out);
    let stream = build(&device, supported.sample_format(), config, feeder, control)?;
    stream.play().map_err(device_error)?;
    control.opened(&name);
    control.hold();
    drop(stream);
    Ok(())
}
