//! Platforms without a capture backend: every source reports that it cannot
//! open, so the run falls back to `/ingest` and the wav file.

use std::sync::mpsc::Sender;

use super::align::Packet;
use super::{CaptureError, Clock, Running, SourceKind};

pub fn open(
    _kind: SourceKind,
    _device: Option<&str>,
    _clock: Clock,
    _out: Sender<Packet>,
) -> Result<Running, CaptureError> {
    Err(CaptureError::Unsupported)
}
