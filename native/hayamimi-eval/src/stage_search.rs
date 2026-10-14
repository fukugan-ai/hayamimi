//! The search of the recognizer, replayed on its own.
//!
//! sherpa-onnx runs the encoder once for an utterance and then walks the
//! encoder's frames: one joiner call per frame, and a decoder call whenever a
//! hypothesis takes a new token, each a small ONNX Runtime call whose inputs
//! cross to the device and whose outputs cross back. Those calls cannot be
//! timed from inside sherpa-onnx, so the same call pattern is run here
//! through `ort`, on the provider under test, with inputs of the beam's shape.
//! The time of one call does not depend on the values, only on the shapes.
//!
//! The decoder is timed for as many calls as there are frames, the most the
//! search can make; the report brackets the search between the joiner alone
//! plus one decoder call per token and the joiner plus a decoder call per frame.

use std::hint::black_box;
use std::path::Path;
use std::time::{Duration, Instant};

use ort::session::Session;
use ort::value::{Outlet, Tensor};

use crate::error::EvalError;
use crate::ort_stages::{Device, OrtRuntime};
use crate::stage_models::{self as models, Precision};

/// Hypotheses the search keeps per frame: the default of `modified_beam_search`.
pub const BEAM: usize = 4;
/// A token id inside the vocabulary; the time of a call does not depend on it.
const TOKEN: i64 = 1;
/// A joiner input value; the time of a call does not depend on it.
const ACTIVATION: f32 = 0.1;

/// The sample names a replay writes. Two of them count, and their `ns` field
/// is a number of frames or tokens, not a duration.
pub const JOINER_TOTAL: &str = "search_joiner";
pub const DECODER_TOTAL: &str = "search_decoder";
pub const FRAMES_COUNT: &str = "search_frames_count";
pub const TOKENS_COUNT: &str = "search_tokens_count";

fn invalid(message: impl Into<String>) -> EvalError {
    EvalError::Invalid(message.into())
}

fn ort_error(error: impl std::fmt::Display) -> EvalError {
    invalid(format!("ort: {error}"))
}

fn nanos(total: Duration) -> Result<u64, EvalError> {
    u64::try_from(total.as_nanos()).map_err(|_| invalid("a duration does not fit u64"))
}

/// The width (second axis) of a static `[batch, width]` input.
fn input_width(outlet: &Outlet) -> Result<usize, EvalError> {
    let name = outlet.name();
    let Some(shape) = outlet.dtype().tensor_shape() else {
        return Err(invalid(format!("input `{name}` is not a tensor")));
    };
    let Some(width) = shape.get(1) else {
        return Err(invalid(format!("input `{name}` has no second axis")));
    };
    match usize::try_from(*width) {
        Ok(width) if width > 0 => Ok(width),
        _ => Err(invalid(format!("input `{name}` has a dynamic width"))),
    }
}

/// The names of the inputs of a model that must have exactly `count` of them.
fn input_names(session: &Session, model: &str, count: usize) -> Result<Vec<String>, EvalError> {
    let names: Vec<String> = session
        .inputs()
        .iter()
        .map(|outlet| outlet.name().to_owned())
        .collect();
    if names.len() != count {
        return Err(invalid(format!(
            "the {model} has {} inputs, the replay needs {count}",
            names.len()
        )));
    }
    Ok(names)
}

/// The decoder and the joiner of the recognizer's transducer, opened on one provider.
pub struct SearchModels {
    decoder: Session,
    joiner: Session,
    decoder_input: String,
    joiner_inputs: [String; 2],
    context: usize,
    width: usize,
}

/// What replaying the search of one utterance cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Replay {
    pub frames: usize,
    pub tokens: usize,
    pub joiner_ns: u64,
    /// `frames` decoder calls: the most the search makes.
    pub decoder_ns: u64,
}

impl SearchModels {
    pub fn open(
        runtime: &OrtRuntime,
        models_dir: &Path,
        precision: Precision,
        device: Device,
        threads: usize,
    ) -> Result<Self, EvalError> {
        let decoder = runtime.session(
            &models::decoder_file(models_dir, precision)?,
            device,
            threads,
            false,
        )?;
        let joiner = runtime.session(
            &models::joiner_file(models_dir, precision)?,
            device,
            threads,
            false,
        )?;
        let mut decoder_names = input_names(&decoder, "decoder", 1)?;
        let mut joiner_names = input_names(&joiner, "joiner", 2)?;
        let context = input_width(&decoder.inputs()[0])?;
        let width = input_width(&joiner.inputs()[0])?;
        let (Some(decoder_input), Some(second), Some(first)) =
            (decoder_names.pop(), joiner_names.pop(), joiner_names.pop())
        else {
            return Err(invalid("a model lost its inputs while they were counted"));
        };
        Ok(Self {
            decoder,
            joiner,
            decoder_input,
            joiner_inputs: [first, second],
            context,
            width,
        })
    }

    fn decoder_once(&mut self) -> Result<Duration, EvalError> {
        let ids = Tensor::from_array(([BEAM, self.context], vec![TOKEN; BEAM * self.context]))
            .map_err(ort_error)?;
        let started = Instant::now();
        let outputs = self
            .decoder
            .run(ort::inputs![self.decoder_input.as_str() => ids])
            .map_err(ort_error)?;
        let elapsed = started.elapsed();
        black_box(&outputs);
        Ok(elapsed)
    }

    fn joiner_once(&mut self) -> Result<Duration, EvalError> {
        let make = |width: usize| {
            Tensor::from_array(([BEAM, width], vec![ACTIVATION; BEAM * width])).map_err(ort_error)
        };
        let (encoder_frame, decoder_out) = (make(self.width)?, make(self.width)?);
        let [first, second] = &self.joiner_inputs;
        let started = Instant::now();
        let outputs = self
            .joiner
            .run(ort::inputs![first.as_str() => encoder_frame, second.as_str() => decoder_out])
            .map_err(ort_error)?;
        let elapsed = started.elapsed();
        black_box(&outputs);
        Ok(elapsed)
    }

    /// The time of `count` decoder calls, in nanoseconds.
    pub fn decoder_calls(&mut self, count: usize) -> Result<u64, EvalError> {
        let mut total = Duration::ZERO;
        for _ in 0..count {
            total += self.decoder_once()?;
        }
        nanos(total)
    }

    /// The time of `count` joiner calls, in nanoseconds.
    pub fn joiner_calls(&mut self, count: usize) -> Result<u64, EvalError> {
        let mut total = Duration::ZERO;
        for _ in 0..count {
            total += self.joiner_once()?;
        }
        nanos(total)
    }

    /// Replays the search of an utterance that gave `frames` encoder frames and `tokens` tokens.
    pub fn replay(&mut self, frames: usize, tokens: usize) -> Result<Replay, EvalError> {
        Ok(Replay {
            frames,
            tokens,
            joiner_ns: self.joiner_calls(frames)?,
            decoder_ns: self.decoder_calls(frames)?,
        })
    }
}
