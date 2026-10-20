//! Offline probe of the Silero speech probability, one value per 512-sample
//! window, streamed the way the detector is fed.
//!
//! The model is opened through `ort` (the sherpa-onnx C API returns no
//! probability). Its input and output names are read and logged, and the
//! streaming layout is chosen from them. The layout this probe streams takes
//! one window per call (`x` of `[1, 512]`) and carries the LSTM state (`h` and
//! `c`, each `[2, 1, 64]`) across windows; both start at zero.
//!
//! `lat-ja.wav` and `lat-en.wav` are streamed in two ways, and a stream of
//! zeros is the positive control:
//!
//! - `orig`: the file as it is.
//! - `noise60`: every run of exact zeros (at least one window long) inside the
//!   gap after an utterance is replaced by white noise at -60 dBFS RMS.
//! - `zero_only`: zeros from the first sample; the probability must stay under
//!   the 0.5 cutoff, or the probe fails.
//!
//! Every line is `KEY field=value ...` on stdout:
//!
//! - `MODEL`: the tensor names, types and the layout in use.
//! - `GAP`: per utterance, the gap length, its exact-zero total and the zeros
//!   that follow the manifest end without a break.
//! - `NOISE`: the samples replaced and the RMS of the noise written.
//! - `TRACE`: one window: `utt` (the last utterance that started at or before
//!   the window), `zone` (`speech` before its manifest end, `gap` after,
//!   `before` ahead of the first utterance), `window_index`, `t_ms` (the
//!   window's first sample) and `prob`.
//! - `FALL`: per utterance, the first window that starts at or after the
//!   manifest end and before the next utterance, whose probability is under
//!   0.5 and under 0.35, as milliseconds after the manifest end.
//! - `CONTROL`: the maximum probability of the zero stream.
//!
//!     cargo test -j 2 --release -p hayamimi-asr --test vad_trace_probe -- --ignored --nocapture

mod support;

use std::ops::Range;
use std::path::Path;

use hayamimi_asr::audio::{WINDOW, read_wav};
use hayamimi_asr::ort_runtime::{Device, OrtRuntime};
use ort::session::Session;
use ort::value::Tensor;
use serde_json::Value;
use support::{Failure, TestResult, models_dir, read_json, testdata_dir};

const MODEL_FILE: &str = "silero_vad.onnx";
/// Elements of one LSTM state tensor, `[2, 1, 64]`.
const STATE_LEN: usize = 2 * 64;
/// Samples per millisecond at 16 kHz.
const SAMPLES_PER_MS: usize = 16;
const SAMPLES_PER_SECOND: usize = 16_000;
/// The production speech cutoff and the line below which speech is ended.
const CUTS: [f32; 2] = [0.5, 0.35];
/// Level of the noise that replaces the zeros of a gap, as RMS in dBFS.
const NOISE_DBFS: f32 = -60.0;
const NOISE_SEED: u32 = 0x9E37_79B9;
const CONTROL_SECONDS: usize = 20;

/// A stretch of the input in samples: a manifest utterance.
#[derive(Clone, Copy, Debug)]
struct Span {
    start: usize,
    end: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Layout {
    /// `x`, `h`, `c` in; `prob`, `new_h`, `new_c` out.
    Lstm,
    /// `input`, `state`, `sr` in; `output`, `stateN` out.
    StateOnly,
    /// `input`, `sr`, `h`, `c` in; `output`, `hn`, `cn` out.
    LstmWithRate,
}

/// The layout the tensor names show, in the order the model lists them.
fn layout_of(inputs: &[&str], outputs: &[&str]) -> Option<Layout> {
    match (inputs, outputs) {
        (["x", "h", "c"], ["prob", "new_h", "new_c"]) => Some(Layout::Lstm),
        (["input", "state", "sr"], ["output", "stateN"]) => Some(Layout::StateOnly),
        (["input", "sr", "h", "c"], ["output", "hn", "cn"]) => Some(Layout::LstmWithRate),
        _ => None,
    }
}

/// The Silero model with the LSTM state it carries from one window to the next.
struct Silero {
    session: Session,
    h: Vec<f32>,
    c: Vec<f32>,
}

/// Logs the tensors of `session`; fails unless they show the layout streamed here.
fn announce(session: &Session, model: &Path) -> Result<(), Failure> {
    let inputs: Vec<&str> = session.inputs().iter().map(|o| o.name()).collect();
    let outputs: Vec<&str> = session.outputs().iter().map(|o| o.name()).collect();
    let layout = layout_of(&inputs, &outputs);
    println!(
        "MODEL file={} layout={layout:?} inputs={inputs:?} outputs={outputs:?}",
        model.display()
    );
    for outlet in session.inputs().iter().chain(session.outputs()) {
        println!("MODEL tensor={} type={:?}", outlet.name(), outlet.dtype());
    }
    match layout {
        Some(Layout::Lstm) => Ok(()),
        Some(other) => Err(format!(
            "the model has the {other:?} layout; this probe streams the Lstm layout only"
        )),
        None => Err(format!(
            "unknown tensor names: inputs {inputs:?}, outputs {outputs:?}"
        )),
    }
}

/// The elements of a state tensor the model returned, checked for their count.
fn next_state(value: &ort::value::DynValue) -> Result<Vec<f32>, Failure> {
    let (_, data) = value
        .try_extract_tensor::<f32>()
        .map_err(|why| why.to_string())?;
    if data.len() != STATE_LEN {
        return Err(format!(
            "the state has {} elements, want {STATE_LEN}",
            data.len()
        ));
    }
    Ok(data.to_vec())
}

impl Silero {
    fn open(model: &Path) -> Result<Self, Failure> {
        let runtime = OrtRuntime::shared().map_err(|why| why.to_string())?;
        let session = runtime
            .session(model, Device::Cpu, 1, false)
            .map_err(|why| why.to_string())?;
        announce(&session, model)?;
        Ok(Self {
            session,
            h: vec![0.0; STATE_LEN],
            c: vec![0.0; STATE_LEN],
        })
    }

    /// Back to the start of a stream: zero state.
    fn reset(&mut self) {
        self.h.fill(0.0);
        self.c.fill(0.0);
    }

    /// The speech probability of one `WINDOW`-sample window, advancing the state.
    fn probability(&mut self, window: &[f32]) -> Result<f32, Failure> {
        let x = Tensor::from_array(([1_usize, WINDOW], window.to_vec()))
            .map_err(|why| why.to_string())?;
        let h = Tensor::from_array(([2_usize, 1, 64], std::mem::take(&mut self.h)))
            .map_err(|why| why.to_string())?;
        let c = Tensor::from_array(([2_usize, 1, 64], std::mem::take(&mut self.c)))
            .map_err(|why| why.to_string())?;
        let outputs = self
            .session
            .run(ort::inputs!["x" => x, "h" => h, "c" => c])
            .map_err(|why| why.to_string())?;
        let (_, probability) = outputs["prob"]
            .try_extract_tensor::<f32>()
            .map_err(|why| why.to_string())?;
        let Some(&probability) = probability.first() else {
            return Err(String::from("the model returned an empty probability"));
        };
        self.h = next_state(&outputs["new_h"])?;
        self.c = next_state(&outputs["new_c"])?;
        Ok(probability)
    }

    /// The probability of every whole window of `samples`, from a fresh state.
    fn trace(&mut self, samples: &[f32]) -> Result<Vec<f32>, Failure> {
        self.reset();
        let (windows, _tail) = samples.as_chunks::<WINDOW>();
        windows
            .iter()
            .map(|window| self.probability(window.as_slice()))
            .collect()
    }
}

/// White noise from a fixed seed, uniform in `[-peak, peak]`, with the RMS of
/// what it has written kept.
struct Noise {
    seed: u32,
    peak: f32,
    sum_squares: f64,
    written: usize,
}

impl Noise {
    fn new(rms_dbfs: f32) -> Self {
        // A uniform variable on [-a, a] has an RMS of a / sqrt(3).
        let peak = 3.0_f32.sqrt() * 10.0_f32.powf(rms_dbfs / 20.0);
        Self {
            seed: NOISE_SEED,
            peak,
            sum_squares: 0.0,
            written: 0,
        }
    }

    fn sample(&mut self) -> f32 {
        let mut x = self.seed;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.seed = x;
        let value = (x as f32 / u32::MAX as f32 * 2.0 - 1.0) * self.peak;
        self.sum_squares += f64::from(value) * f64::from(value);
        self.written += 1;
        value
    }

    fn rms_dbfs(&self) -> f64 {
        10.0 * (self.sum_squares / self.written as f64).log10()
    }
}

fn sample_field(entry: &Value, key: &str) -> Result<usize, Failure> {
    let number = entry
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("a manifest entry has no integer `{key}`"))?;
    usize::try_from(number).map_err(|why| format!("{key}: {why}"))
}

/// The utterance windows of a manifest, in order.
fn manifest_spans(path: &Path) -> Result<Vec<Span>, Failure> {
    let Some(entries) = read_json(path)?.as_array().cloned() else {
        return Err(format!("{} is not an array", path.display()));
    };
    entries
        .iter()
        .map(|entry| {
            Ok(Span {
                start: sample_field(entry, "start_sample")?,
                end: sample_field(entry, "end_sample")?,
            })
        })
        .collect()
}

/// The spans are in order, do not overlap and lie inside `len` samples.
fn check_spans(spans: &[Span], len: usize) -> Result<(), Failure> {
    let mut floor = 0;
    for (index, span) in spans.iter().enumerate() {
        if span.start < floor || span.end <= span.start || span.end > len {
            return Err(format!(
                "utterance {} ({}..{}) is out of order or outside the {len} samples",
                index + 1,
                span.start,
                span.end
            ));
        }
        floor = span.end;
    }
    Ok(())
}

/// The gap after utterance `index`: from its end to the next start, or to the end of the input.
fn gap_after(spans: &[Span], index: usize, len: usize) -> Range<usize> {
    let end = spans[index].end;
    let next = spans.get(index + 1).map_or(len, |span| span.start);
    end..next
}

/// The runs of exact zeros, at least `min_len` long, as ranges of `samples`.
fn zero_runs(samples: &[f32], min_len: usize) -> Vec<Range<usize>> {
    let mut runs = Vec::new();
    let mut at = 0;
    for run in samples.chunk_by(|a, b| (*a == 0.0) == (*b == 0.0)) {
        if run[0] == 0.0 && run.len() >= min_len {
            runs.push(at..at + run.len());
        }
        at += run.len();
    }
    runs
}

/// Replaces the zero runs of every gap with noise.
fn fill_zero_gaps(samples: &mut [f32], spans: &[Span], noise: &mut Noise) {
    for index in 0..spans.len() {
        let gap = gap_after(spans, index, samples.len());
        for run in zero_runs(&samples[gap.clone()], WINDOW) {
            for sample in &mut samples[gap.start + run.start..gap.start + run.end] {
                *sample = noise.sample();
            }
        }
    }
}

fn ms_of(samples: usize) -> usize {
    samples / SAMPLES_PER_MS
}

fn ms_between(later: usize, earlier: usize) -> i64 {
    (later as i64 - earlier as i64) / SAMPLES_PER_MS as i64
}

fn print_gaps(lang: &str, samples: &[f32], spans: &[Span]) {
    for index in 0..spans.len() {
        let gap = gap_after(spans, index, samples.len());
        let body = &samples[gap];
        let zeros = body.iter().filter(|s| **s == 0.0).count();
        let leading = body.iter().take_while(|s| **s == 0.0).count();
        println!(
            "GAP lang={lang} utt={} gap_ms={} zero_ms={} lead_zero_ms={}",
            index + 1,
            ms_of(body.len()),
            ms_of(zeros),
            ms_of(leading)
        );
    }
}

/// Where a window sits against the utterance that started last.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Zone {
    Before,
    Speech,
    Gap,
}

impl Zone {
    fn name(self) -> &'static str {
        match self {
            Zone::Before => "before",
            Zone::Speech => "speech",
            Zone::Gap => "gap",
        }
    }
}

/// The zone of a window starting at `first`, inside utterance `utt` (1-based,
/// 0 ahead of the first one).
fn zone_of(spans: &[Span], utt: usize, first: usize) -> Zone {
    let Some(span) = utt.checked_sub(1).map(|at| spans[at]) else {
        return Zone::Before;
    };
    if first < span.end {
        Zone::Speech
    } else {
        Zone::Gap
    }
}

/// One `TRACE` line per window.
fn print_trace(variant: &str, lang: &str, probs: &[f32], spans: &[Span]) {
    for (index, probability) in probs.iter().enumerate() {
        let first = index * WINDOW;
        let utt = spans.partition_point(|span| span.start <= first);
        println!(
            "TRACE variant={variant} lang={lang} utt={utt} zone={} window_index={index} \
             t_ms={} prob={probability:.4}",
            zone_of(spans, utt, first).name(),
            ms_of(first)
        );
    }
}

/// The first of `gap` (windows from `from`) whose probability is under `cut`,
/// as milliseconds after `end`.
fn fall_after(gap: &[f32], from: usize, end: usize, cut: f32) -> Option<i64> {
    let at = gap.iter().position(|probability| *probability < cut)?;
    Some(ms_between((from + at) * WINDOW, end))
}

/// One `FALL` line per utterance.
fn print_falls(variant: &str, lang: &str, probs: &[f32], spans: &[Span]) -> Result<(), Failure> {
    for (index, span) in spans.iter().enumerate() {
        let limit = gap_after(spans, index, probs.len() * WINDOW).end;
        let from = span.end.div_ceil(WINDOW);
        let to = limit.div_ceil(WINDOW).min(probs.len());
        let Some(gap) = probs.get(from..to) else {
            return Err(format!("utterance {}: no window after its end", index + 1));
        };
        let peak = gap.iter().copied().fold(0.0_f32, f32::max);
        println!(
            "FALL variant={variant} lang={lang} utt={} end_ms={} gap_ms={} \
             below_{}_rel_ms={:?} below_{}_rel_ms={:?} first_prob={:?} peak_prob={peak:.4}",
            index + 1,
            ms_of(span.end),
            ms_of(limit - span.end),
            CUTS[0],
            fall_after(gap, from, span.end, CUTS[0]),
            CUTS[1],
            fall_after(gap, from, span.end, CUTS[1]),
            gap.first().copied(),
        );
    }
    Ok(())
}

fn run_variant(
    silero: &mut Silero,
    variant: &str,
    lang: &str,
    samples: &[f32],
    spans: &[Span],
) -> Result<(), Failure> {
    let probs = silero.trace(samples)?;
    print_trace(variant, lang, &probs, spans);
    print_falls(variant, lang, &probs, spans)
}

fn report_language(
    silero: &mut Silero,
    lang: &str,
    wav: &Path,
    manifest: &Path,
) -> Result<(), Failure> {
    let samples = read_wav(wav).map_err(|why| why.to_string())?;
    let spans = manifest_spans(manifest)?;
    check_spans(&spans, samples.len())?;
    print_gaps(lang, &samples, &spans);
    run_variant(silero, "orig", lang, &samples, &spans)?;
    let mut noisy = samples.clone();
    let mut noise = Noise::new(NOISE_DBFS);
    fill_zero_gaps(&mut noisy, &spans, &mut noise);
    println!(
        "NOISE lang={lang} replaced_samples={} rms_dbfs={:.2}",
        noise.written,
        noise.rms_dbfs()
    );
    run_variant(silero, "noise60", lang, &noisy, &spans)
}

/// The positive control: a stream of zeros never reaches the cutoff.
fn zero_control(silero: &mut Silero) -> Result<(), Failure> {
    let probs = silero.trace(&vec![0.0_f32; CONTROL_SECONDS * SAMPLES_PER_SECOND])?;
    print_trace("zero_only", "none", &probs, &[]);
    let peak = probs.iter().copied().fold(0.0_f32, f32::max);
    let above = |cut: f32| probs.iter().filter(|p| **p >= cut).count();
    println!(
        "CONTROL variant=zero_only windows={} max_prob={peak:.4} at_or_above_{}={} at_or_above_{}={}",
        probs.len(),
        CUTS[0],
        above(CUTS[0]),
        CUTS[1],
        above(CUTS[1])
    );
    if above(CUTS[0]) > 0 {
        return Err(String::from("the zero stream reached the speech cutoff"));
    }
    Ok(())
}

#[test]
#[ignore = "an offline probe: reads testdata/ and the VAD model"]
fn vad_window_probabilities() -> TestResult {
    let model = models_dir()?.join(MODEL_FILE);
    let testdata = testdata_dir()?;
    let mut silero = Silero::open(&model)?;
    zero_control(&mut silero)?;
    for lang in ["ja", "en"] {
        let wav = testdata.join(format!("lat-{lang}.wav"));
        let manifest = testdata.join("manifests").join(format!("lat-{lang}.json"));
        report_language(&mut silero, lang, &wav, &manifest)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lstm_names_give_the_lstm_layout() {
        let found = layout_of(&["x", "h", "c"], &["prob", "new_h", "new_c"]);
        assert_eq!(found, Some(Layout::Lstm));
    }

    #[test]
    fn the_state_only_names_give_the_state_only_layout() {
        let found = layout_of(&["input", "state", "sr"], &["output", "stateN"]);
        assert_eq!(found, Some(Layout::StateOnly));
    }

    #[test]
    fn the_rate_and_lstm_names_give_their_own_layout() {
        let found = layout_of(&["input", "sr", "h", "c"], &["output", "hn", "cn"]);
        assert_eq!(found, Some(Layout::LstmWithRate));
    }

    #[test]
    fn other_names_give_no_layout() {
        assert_eq!(layout_of(&["x"], &["y"]), None);
        assert_eq!(layout_of(&[], &[]), None);
    }

    #[test]
    fn zero_runs_shorter_than_the_minimum_are_left_alone() {
        let samples = [0.5, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.0];
        assert_eq!(zero_runs(&samples, 3), vec![4..8]);
        assert_eq!(zero_runs(&samples, 2), vec![1..3, 4..8]);
        assert!(zero_runs(&samples, 5).is_empty());
        assert!(zero_runs(&[], 1).is_empty());
    }

    #[test]
    fn noise_has_the_requested_level() {
        let mut noise = Noise::new(NOISE_DBFS);
        for _ in 0..100_000 {
            noise.sample();
        }
        assert!((noise.rms_dbfs() - f64::from(NOISE_DBFS)).abs() < 0.2);
    }

    #[test]
    fn only_long_zero_runs_inside_a_gap_become_noise() {
        let mut samples = vec![0.0_f32; 20 + 600 + 1 + 100];
        samples[10] = 0.25;
        samples[620] = 0.25;
        let before = samples.clone();
        let spans = [Span { start: 0, end: 20 }];
        fill_zero_gaps(&mut samples, &spans, &mut Noise::new(NOISE_DBFS));
        assert_eq!(samples[..20], before[..20]);
        assert!(samples[20..620].iter().all(|s| *s != 0.0));
        assert_eq!(samples[620..], before[620..]);
    }

    #[test]
    fn spans_out_of_order_or_outside_the_input_are_refused() {
        let ok = [Span { start: 0, end: 10 }, Span { start: 12, end: 20 }];
        assert!(check_spans(&ok, 20).is_ok());
        assert!(check_spans(&ok, 19).is_err());
        let overlapping = [Span { start: 0, end: 10 }, Span { start: 9, end: 20 }];
        assert!(check_spans(&overlapping, 30).is_err());
        assert!(check_spans(&[Span { start: 5, end: 5 }], 30).is_err());
    }

    #[test]
    fn a_fall_is_counted_from_the_manifest_end() {
        // The gap starts at window 3; the third of its windows (window 5) is the first under 0.5.
        let gap = [0.9, 0.7, 0.4, 0.2];
        let end = 3 * WINDOW - 160;
        assert_eq!(fall_after(&gap, 3, end, 0.5), Some(74));
        assert_eq!(fall_after(&gap, 3, end, 0.1), None);
    }

    #[test]
    fn a_window_is_placed_by_the_utterance_that_started_last() {
        let spans = [
            Span {
                start: 100,
                end: 200,
            },
            Span {
                start: 300,
                end: 400,
            },
        ];
        assert_eq!(zone_of(&spans, 0, 50), Zone::Before);
        assert_eq!(zone_of(&spans, 1, 150), Zone::Speech);
        assert_eq!(zone_of(&spans, 1, 200), Zone::Gap);
        assert_eq!(zone_of(&spans, 2, 300), Zone::Speech);
    }
}
