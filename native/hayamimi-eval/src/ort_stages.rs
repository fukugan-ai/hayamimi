//! Forward passes of single models through `ort`, on the ONNX Runtime the
//! sherpa-onnx build ships, for the two stages the 3.5 table leaves to a
//! measurement: the punctuation BERT and the recognizer's encoder alone.
//!
//! The recognizer is timed through sherpa-onnx everywhere else; the encoder
//! alone cannot be timed there, so its model is opened here and run on the
//! same input on the CUDA and the CPU provider. `OrtRuntime::take_placements`
//! returns the node placement ONNX Runtime logs when it builds a session.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ort::environment::Environment;
use ort::ep;
use ort::logging::LogLevel;
use ort::session::Session;
use ort::value::Tensor;
use serde::{Deserialize, Serialize};

use crate::error::EvalError;

/// The mel bins of the recognizer's front end.
pub const MEL_BINS: usize = 80;
/// Samples per analysis frame (25 ms at 16 kHz).
const FRAME: usize = 400;
/// Samples between two frames (10 ms).
const SHIFT: usize = 160;
/// The size of the Fourier transform; the next power of two above the frame.
const FFT: usize = 512;
const SAMPLE_RATE: f32 = 16_000.0;
const MEL_LOW_HZ: f32 = 20.0;
const MEL_HIGH_HZ: f32 = 8_000.0;
const PREEMPHASIS: f32 = 0.97;
/// Added to a filter-bank energy before its logarithm.
const ENERGY_FLOOR: f32 = 1.192_092_9e-7;

fn invalid(message: impl Into<String>) -> EvalError {
    EvalError::Invalid(message.into())
}

fn ort_error(error: impl std::fmt::Display) -> EvalError {
    invalid(format!("ort: {error}"))
}

/// The provider a session is built for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Device {
    Cuda,
    Cpu,
}

/// How many nodes ONNX Runtime put on one execution provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placement {
    pub provider: String,
    pub nodes: u64,
}

/// Where one model's nodes went when it was built for `device`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacedRow {
    pub model: String,
    pub device: String,
    pub cuda_nodes: u64,
    pub cpu_nodes: u64,
}

/// The nodes of `placements` on the CUDA provider and on the CPU provider.
/// A provider named by neither is an error: its nodes would be counted nowhere.
pub fn placed_row(
    model: &str,
    device: &str,
    placements: &[Placement],
) -> Result<PlacedRow, EvalError> {
    if placements.is_empty() {
        return Err(invalid(
            "ONNX Runtime logged no node placement; the session was not built at verbose level",
        ));
    }
    let (mut cuda_nodes, mut cpu_nodes) = (0_u64, 0_u64);
    for placement in placements {
        if placement.provider.starts_with("CUDA") {
            cuda_nodes += placement.nodes;
        } else if placement.provider.starts_with("CPU") {
            cpu_nodes += placement.nodes;
        } else {
            return Err(invalid(format!(
                "nodes were placed on the unexpected provider `{}`",
                placement.provider
            )));
        }
    }
    Ok(PlacedRow {
        model: model.to_owned(),
        device: device.to_owned(),
        cuda_nodes,
        cpu_nodes,
    })
}

/// An ONNX Runtime loaded by path, with the lines it logs about node placement kept.
pub struct OrtRuntime {
    environment: Environment,
    placements: Arc<Mutex<Vec<String>>>,
}

/// The library name of the ONNX Runtime beside the executable.
pub fn library_beside_executable() -> Result<PathBuf, EvalError> {
    let exe = std::env::current_exe().map_err(|e| invalid(format!("current_exe: {e}")))?;
    let dir = exe
        .parent()
        .ok_or_else(|| invalid("the executable has no directory"))?;
    let name = if cfg!(windows) {
        "onnxruntime.dll"
    } else {
        "libonnxruntime.so"
    };
    Ok(dir.join(name))
}

/// The nodes-per-provider figures in one ONNX Runtime log line, if it has any.
///
/// The line reads `Node(s) placed on [CPUExecutionProvider]. Number of nodes: 12`.
pub fn parse_placement(message: &str) -> Option<Placement> {
    let (_, after) = message.split_once("placed on [")?;
    let (provider, rest) = after.split_once(']')?;
    let (_, count) = rest.split_once("Number of nodes:")?;
    let nodes = count.split_whitespace().next()?.parse().ok()?;
    Some(Placement {
        provider: provider.to_owned(),
        nodes,
    })
}

impl OrtRuntime {
    /// Loads the runtime at `library`. Only one runtime exists per process.
    pub fn open(library: &Path) -> Result<Self, EvalError> {
        let placements: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&placements);
        let environment = ort::init_from(library)
            .map_err(ort_error)?
            .with_logger(Arc::new(
                move |_: LogLevel, _: &str, _: &str, _: &str, message: &str| {
                    if message.contains("placed on")
                        && let Ok(mut lines) = sink.lock()
                    {
                        lines.push(message.to_owned());
                    }
                },
            ))
            .build()
            .map_err(ort_error)?;
        environment.set_log_level(LogLevel::Warning);
        Ok(Self {
            environment,
            placements,
        })
    }

    /// A session on `model`. A CUDA session fails to build when the CUDA provider cannot be used.
    pub fn session(
        &self,
        model: &Path,
        device: Device,
        threads: usize,
        verbose: bool,
    ) -> Result<Session, EvalError> {
        let mut builder = Session::builder(&self.environment).map_err(ort_error)?;
        if device == Device::Cuda {
            builder = builder
                .with_execution_providers([ep::CUDA::default().build().error_on_failure()])
                .map_err(ort_error)?;
        }
        if verbose {
            self.environment.set_log_level(LogLevel::Verbose);
            builder = builder
                .with_log_level(LogLevel::Verbose)
                .map_err(ort_error)?;
        }
        let session = builder
            .with_intra_threads(threads)
            .map_err(ort_error)?
            .commit_from_file(model)
            .map_err(ort_error);
        self.environment.set_log_level(LogLevel::Warning);
        session
    }

    /// The placement lines logged since the last call.
    pub fn take_placements(&self) -> Result<Vec<Placement>, EvalError> {
        let mut lines = self
            .placements
            .lock()
            .map_err(|_| invalid("the placement log is poisoned"))?;
        let found = lines.iter().filter_map(|l| parse_placement(l)).collect();
        lines.clear();
        Ok(found)
    }
}

/// One BERT forward: `input_ids` and an all-ones `attention_mask`, `[1, len]`. Returns the nanoseconds.
pub fn forward_bert(session: &mut Session, ids: &[i64]) -> Result<u64, EvalError> {
    let len = ids.len();
    let input_ids = Tensor::from_array(([1_usize, len], ids.to_vec())).map_err(ort_error)?;
    let mask = Tensor::from_array(([1_usize, len], vec![1_i64; len])).map_err(ort_error)?;
    let started = std::time::Instant::now();
    let outputs = session
        .run(ort::inputs!["input_ids" => input_ids, "attention_mask" => mask])
        .map_err(ort_error)?;
    let elapsed = started.elapsed();
    std::hint::black_box(&outputs);
    u64::try_from(elapsed.as_nanos()).map_err(|_| invalid("a duration does not fit u64"))
}

/// One encoder forward on `frames` frames of `features` (`frames * MEL_BINS` floats). Returns the nanoseconds.
pub fn forward_encoder(
    session: &mut Session,
    features: &[f32],
    frames: usize,
) -> Result<u64, EvalError> {
    let x =
        Tensor::from_array(([1_usize, frames, MEL_BINS], features.to_vec())).map_err(ort_error)?;
    let lens = Tensor::from_array(([1_usize], vec![i64::try_from(frames).map_err(ort_error)?]))
        .map_err(ort_error)?;
    let started = std::time::Instant::now();
    let outputs = session
        .run(ort::inputs!["x" => x, "x_lens" => lens])
        .map_err(ort_error)?;
    let elapsed = started.elapsed();
    std::hint::black_box(&outputs);
    u64::try_from(elapsed.as_nanos()).map_err(|_| invalid("a duration does not fit u64"))
}

/// In-place radix-2 Fourier transform of `re` + i `im`; the length is a power of two.
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut size = 2;
    while size <= n {
        let angle = -2.0 * std::f32::consts::PI / size as f32;
        for start in (0..n).step_by(size) {
            for k in 0..size / 2 {
                let (sin, cos) = (angle * k as f32).sin_cos();
                let (a, b) = (start + k, start + k + size / 2);
                let tr = re[b] * cos - im[b] * sin;
                let ti = re[b] * sin + im[b] * cos;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
            }
        }
        size <<= 1;
    }
}

fn hz_to_mel(hz: f32) -> f32 {
    1127.0 * (1.0 + hz / 700.0).ln()
}

/// Triangular mel filters over the `FFT / 2 + 1` power-spectrum bins.
fn mel_filters() -> Vec<Vec<f32>> {
    let bins = FFT / 2 + 1;
    let (low, high) = (hz_to_mel(MEL_LOW_HZ), hz_to_mel(MEL_HIGH_HZ));
    let step = (high - low) / (MEL_BINS + 1) as f32;
    let hz_per_bin = SAMPLE_RATE / FFT as f32;
    (0..MEL_BINS)
        .map(|m| {
            let (left, center, right) = (
                low + step * m as f32,
                low + step * (m + 1) as f32,
                low + step * (m + 2) as f32,
            );
            (0..bins)
                .map(|b| {
                    let mel = hz_to_mel(b as f32 * hz_per_bin);
                    if mel <= left || mel >= right {
                        0.0
                    } else if mel <= center {
                        (mel - left) / (center - left)
                    } else {
                        (right - mel) / (right - center)
                    }
                })
                .collect()
        })
        .collect()
}

/// A log-mel front end of the recognizer's shape: 25 ms frames every 10 ms, 80 bins.
/// Returns the features row by row and the number of frames. The encoder's time does not
/// depend on the values, only on the number of frames, so this is not bit-exact with the
/// recognizer's own front end.
pub fn log_mel(samples: &[f32]) -> (Vec<f32>, usize) {
    if samples.len() < FRAME {
        return (Vec::new(), 0);
    }
    let frames = 1 + (samples.len() - FRAME) / SHIFT;
    let filters = mel_filters();
    let window: Vec<f32> = (0..FRAME)
        .map(|i| {
            (0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (FRAME - 1) as f32).cos())
                .powf(0.85)
        })
        .collect();
    let mut features = Vec::with_capacity(frames * MEL_BINS);
    let mut re = vec![0.0_f32; FFT];
    let mut im = vec![0.0_f32; FFT];
    for frame in 0..frames {
        let chunk = &samples[frame * SHIFT..frame * SHIFT + FRAME];
        let mean = chunk.iter().sum::<f32>() / FRAME as f32;
        re.fill(0.0);
        im.fill(0.0);
        let mut previous = chunk[0] - mean;
        for i in 0..FRAME {
            let current = chunk[i] - mean;
            let emphasized = current - PREEMPHASIS * if i == 0 { current } else { previous };
            previous = current;
            re[i] = emphasized * window[i];
        }
        fft(&mut re, &mut im);
        let power: Vec<f32> = (0..=FFT / 2)
            .map(|b| re[b] * re[b] + im[b] * im[b])
            .collect();
        for filter in &filters {
            let energy: f32 = filter.iter().zip(&power).map(|(w, p)| w * p).sum();
            features.push((energy.max(ENERGY_FLOOR)).ln());
        }
    }
    (features, frames)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_placement_line_gives_the_provider_and_the_count() {
        let found =
            parse_placement(" Node(s) placed on [CPUExecutionProvider]. Number of nodes: 12");
        assert_eq!(
            found,
            Some(Placement {
                provider: String::from("CPUExecutionProvider"),
                nodes: 12
            })
        );
    }

    #[test]
    fn a_line_without_a_count_gives_nothing() {
        assert_eq!(
            parse_placement("All nodes placed on [CUDAExecutionProvider]."),
            None
        );
        assert_eq!(parse_placement("Node placements"), None);
    }

    fn on(provider: &str, nodes: u64) -> Placement {
        Placement {
            provider: provider.to_owned(),
            nodes,
        }
    }

    #[test]
    fn placements_are_summed_per_provider() -> Result<(), EvalError> {
        let row = placed_row(
            "encoder",
            "cuda",
            &[
                on("CUDAExecutionProvider", 900),
                on("CPUExecutionProvider", 40),
                on("CPUExecutionProvider", 2),
            ],
        )?;
        assert_eq!((row.cuda_nodes, row.cpu_nodes), (900, 42));
        Ok(())
    }

    #[test]
    fn no_placement_line_is_an_error_not_zero_cpu_nodes() {
        assert!(placed_row("encoder", "cuda", &[]).is_err());
    }

    #[test]
    fn an_unexpected_provider_is_an_error() {
        assert!(placed_row("encoder", "cuda", &[on("TensorrtExecutionProvider", 3)]).is_err());
    }

    #[test]
    fn a_count_that_is_not_a_number_gives_nothing() {
        assert_eq!(
            parse_placement("Node(s) placed on [CPUExecutionProvider]. Number of nodes: many"),
            None
        );
    }

    #[test]
    fn the_transform_of_a_constant_is_a_single_bin() {
        let mut re = vec![1.0_f32; 8];
        let mut im = vec![0.0_f32; 8];
        fft(&mut re, &mut im);
        assert!((re[0] - 8.0).abs() < 1e-5);
        assert!(re[1..].iter().all(|v| v.abs() < 1e-5));
    }

    #[test]
    fn one_second_of_audio_gives_98_frames_of_80_bins() {
        let samples: Vec<f32> = (0..16_000)
            .map(|i| ((i as f32) * 0.01).sin() * 0.1)
            .collect();
        let (features, frames) = log_mel(&samples);
        assert_eq!(frames, 98);
        assert_eq!(features.len(), 98 * MEL_BINS);
        assert!(features.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn audio_shorter_than_a_frame_gives_no_frames() {
        let (features, frames) = log_mel(&[0.0; 399]);
        assert_eq!(frames, 0);
        assert!(features.is_empty());
    }
}
