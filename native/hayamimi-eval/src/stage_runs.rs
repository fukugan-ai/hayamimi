//! The timed calls of every stage of the Rust port that can be timed today.
//!
//! One run is one process on one provider. Each stage is called once for every
//! utterance of the recording, `repeat` times over, and every call is a sample.
//! A stage that has no GPU implementation is timed on the host in a `cpu` run;
//! in a `cuda` run it gets the floor of any GPU placement instead: the time to
//! copy its input to the device and its output back, which no kernel can undercut.

use std::hint::black_box;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use hayamimi_asr::itn::{self, Overrides};
use hayamimi_asr::punct::{rules, tokenizer};
use hayamimi_gpu::transfer::Transfer;
use hayamimi_gpu::{modules, probe};
use hayamimi_relay::annotate::{AnnotateRequest, Annotator, AnnotatorConfig};
use hayamimi_relay::lang::Lang;
use hayamimi_relay::logging::{Level, NullSink};
use hayamimi_relay::verify::Verifier;
use serde_json::Value;
use sherpa_onnx::{CircularBuffer, LinearResampler};

use crate::error::EvalError;
use crate::log;
use crate::manifest::read_rows;
use crate::ort_stages::{self, Device, OrtRuntime};
use crate::stage_load::Load;
use crate::stage_models::{self as models, LID_SECONDS, Precision, SAMPLE_RATE, VAD_WINDOW};
use crate::stages::{Meta, RunFile, Sample};
use crate::stream::{Utterances, utterances_of};
use crate::translate_call::{ChatClient, translate_body};
use crate::wav::read_wav;

/// The stage identifiers, in the order a full run measures them.
pub const STAGES: [&str; 16] = [
    "resample",
    "ring",
    "vad",
    "lid",
    "recognize",
    "judge",
    "speaker_embed",
    "speaker_assign",
    "rediarize",
    "translate",
    "prompt_build",
    "verify_convert",
    "punct_bert",
    "encoder_only",
    "itn",
    "punct_rules",
];

/// The variant label of the encoder timed alone through `ort`.
pub const ENCODER_ONLY: &str = "ort-encoder fp32";
/// Threads of the punctuation BERT session.
const BERT_THREADS: usize = 4;

/// The variant label of a stage timed on the host.
pub const HOST: &str = "cpu";
/// The variant label of the floor of a GPU placement.
pub const FLOOR: &str = "transfer-floor";
/// The variant label of the translation model.
pub const TRANSLATION_MODEL: &str = "CAT-Translate-3.3b-Q8_0";

/// Utterances at least this long (4 s) are the ones whose language is identified again.
const LONG_UTTERANCE: usize = 4 * SAMPLE_RATE as usize;
/// The capture rate the host stages start from.
const CAPTURE_RATE: i32 = 48_000;
/// Seconds of audio before an utterance that the ring buffer hands on.
const PREROLL: usize = SAMPLE_RATE as usize;
/// Silence after an utterance that closes the VAD segment: longer than the VAD's minimum silence.
const VAD_TAIL: usize = SAMPLE_RATE as usize / 2;
/// Speakers the assignment stage compares against.
const CENTROIDS: usize = 8;
/// The size of what a stage that exchanges text copies, in bytes.
const TEXT_BYTES: usize = 1024;
/// The translation request ends after this long.
const TRANSLATE_TIMEOUT: Duration = Duration::from_secs(120);
/// Characters per second below which a decode is suspected of having dropped its head.
const SUSPECT_CHARS_PER_SECOND: f32 = 3.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Cuda,
    Cpu,
}

impl Provider {
    pub fn name(self) -> &'static str {
        match self {
            Provider::Cuda => "cuda",
            Provider::Cpu => "cpu",
        }
    }
}

/// What one run does.
#[derive(Debug, Clone)]
pub struct Options {
    pub provider: Provider,
    pub wav: PathBuf,
    pub manifest: PathBuf,
    pub models: PathBuf,
    pub repeat: u32,
    /// The llama-server that answers the translation stage of this run.
    pub llama_url: Option<String>,
    /// The llama-server the load clients keep busy; no load without it.
    pub load_url: Option<String>,
    pub load_clients: u32,
    /// `empty` or `warm`; a label, the cache itself is the environment the process was started in.
    pub cache: String,
    pub run: u32,
    /// The stages to measure; all of them when empty.
    pub only: Vec<String>,
    /// Also load the int8 recognizer on CUDA, to count the copies it adds.
    pub int8_on_cuda: bool,
    /// Open a profiler capture range around every decode.
    pub profile_ranges: bool,
    /// `punct_ja_parity.json`, whose `input_ids` the punctuation BERT is run on.
    pub punct_cases: Option<PathBuf>,
}

struct Clip {
    samples: Vec<f32>,
    reference: String,
}

struct Run<'a> {
    options: &'a Options,
    clips: Vec<Clip>,
    out: Vec<Sample>,
    transfer: Option<Transfer>,
    texts: Vec<String>,
    translations: Vec<String>,
    embeddings: Vec<Vec<f32>>,
    ort: Option<OrtRuntime>,
}

/// The token ids of the first `count` cases of the punctuation fixture.
fn punct_ids(path: &std::path::Path, count: usize) -> Result<Vec<Vec<i64>>, EvalError> {
    let text = std::fs::read_to_string(path).map_err(|e| EvalError::io(path, e))?;
    let value: Value = serde_json::from_str(&text).map_err(|e| EvalError::json(path, e))?;
    let cases = value
        .get("cases")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid(format!("{} has no `cases` array", path.display())))?;
    if cases.len() < count {
        return Err(invalid(format!(
            "{} has {} cases, {count} are needed",
            path.display(),
            cases.len()
        )));
    }
    cases
        .iter()
        .take(count)
        .map(|case| {
            case.get("input_ids")
                .and_then(Value::as_array)
                .ok_or_else(|| invalid("a case has no `input_ids` array"))?
                .iter()
                .map(|id| {
                    id.as_i64()
                        .ok_or_else(|| invalid("an input id is not an integer"))
                })
                .collect()
        })
        .collect()
}

fn invalid(message: impl Into<String>) -> EvalError {
    EvalError::Invalid(message.into())
}

fn nanos(duration: Duration) -> Result<u64, EvalError> {
    u64::try_from(duration.as_nanos()).map_err(|_| invalid("a duration does not fit u64"))
}

/// Runs `call` and returns what it returned with the time it took in nanoseconds.
fn timed<T>(call: impl FnOnce() -> T) -> Result<(T, u64), EvalError> {
    let started = Instant::now();
    let value = call();
    Ok((value, nanos(started.elapsed())?))
}

fn to_pcm(samples: &[f32]) -> Vec<i16> {
    samples
        .iter()
        .map(|s| (s * 32768.0).clamp(-32768.0, 32767.0) as i16)
        .collect()
}

fn clips_of(pcm: &[i16], utterances: &Utterances) -> Vec<Clip> {
    utterances
        .spans
        .iter()
        .zip(&utterances.references)
        .map(|((start, end), reference)| {
            let slice = &pcm[*start..*end];
            Clip {
                samples: slice.iter().map(|s| f32::from(*s) / 32768.0).collect(),
                reference: reference.clone(),
            }
        })
        .collect()
}

/// The model variants of a stage on a provider: what runs there today, and
/// what the other side needs for a fair comparison.
fn precisions(provider: Provider, stage: &str, int8_on_cuda: bool) -> Vec<Precision> {
    match (provider, stage) {
        (Provider::Cuda, "recognize") if int8_on_cuda => {
            vec![Precision::Fp32, Precision::Fp16, Precision::Int8]
        }
        (Provider::Cuda, "recognize") => vec![Precision::Fp32, Precision::Fp16],
        (Provider::Cuda, _) => vec![Precision::Fp32],
        (Provider::Cpu, _) => vec![Precision::Int8, Precision::Fp32],
    }
}

impl Run<'_> {
    fn wants(&self, stage: &str) -> bool {
        if self.options.only.is_empty() {
            // The BERT needs its fixture; a full run without one leaves that stage out.
            return stage != "punct_bert" || self.options.punct_cases.is_some();
        }
        self.options.only.iter().any(|only| only == stage)
    }

    /// The ONNX Runtime of `ort`, opened on first use.
    fn ort(&mut self) -> Result<&OrtRuntime, EvalError> {
        if self.ort.is_none() {
            let library = ort_stages::library_beside_executable()?;
            self.ort = Some(OrtRuntime::open(&library)?);
        }
        self.ort
            .as_ref()
            .ok_or_else(|| invalid("the ONNX Runtime is not open"))
    }

    fn device(&self) -> Device {
        match self.options.provider {
            Provider::Cuda => Device::Cuda,
            Provider::Cpu => Device::Cpu,
        }
    }

    fn provider(&self) -> &'static str {
        self.options.provider.name()
    }

    /// A `cuda` run must have the CUDA provider library loaded while a model is alive; sherpa-onnx
    /// falls back to the CPU without a word, and the run would measure the wrong thing.
    fn assert_provider(&self) -> Result<(), EvalError> {
        if self.options.provider != Provider::Cuda {
            return Ok(());
        }
        let loaded = modules::loaded_modules().map_err(|e| invalid(e.to_string()))?;
        if probe::provider_of(&loaded) == "cuda" {
            return Ok(());
        }
        Err(invalid(
            "the CUDA provider library is not loaded: this run would have measured the CPU",
        ))
    }

    fn calls(&self) -> impl Iterator<Item = (u32, usize)> + use<> {
        let clips = self.clips.len();
        (0..self.options.repeat).flat_map(move |repeat| (0..clips).map(move |utt| (repeat, utt)))
    }

    fn record(
        &mut self,
        stage: &str,
        variant: &str,
        call: (u32, usize),
        ns: u64,
    ) -> Result<(), EvalError> {
        let utt =
            u32::try_from(call.1).map_err(|_| invalid("an utterance index does not fit u32"))?;
        self.out.push(Sample {
            stage: stage.to_owned(),
            variant: variant.to_owned(),
            utt,
            repeat: call.0,
            ns,
        });
        Ok(())
    }

    fn transfer(&mut self) -> Result<&Transfer, EvalError> {
        if self.transfer.is_none() {
            let opened = Transfer::open().map_err(|e| invalid(e.to_string()))?;
            self.transfer = Some(opened);
        }
        self.transfer
            .as_ref()
            .ok_or_else(|| invalid("the CUDA transfer is not open"))
    }

    /// The floor of a GPU placement of `stage`: copy `up` bytes to the device and `down` bytes back.
    fn floor(&mut self, stage: &str, sizes: &[(usize, usize)]) -> Result<(), EvalError> {
        let calls: Vec<(u32, usize)> = self.calls().collect();
        let mut buffers: Vec<(Vec<u8>, Vec<u8>)> = sizes
            .iter()
            .map(|(up, down)| (vec![1_u8; *up], vec![0_u8; *down]))
            .collect();
        // One untimed pass: the first copy of a process pays for the context and the staging buffers.
        for (up, down) in &mut buffers {
            self.transfer()?
                .round_trip(up, down)
                .map_err(|e| invalid(e.to_string()))?;
        }
        for call in calls {
            let (up, down) = &mut buffers[call.1];
            let trip = self
                .transfer()?
                .round_trip(up, down)
                .map_err(|e| invalid(e.to_string()))?;
            self.record(stage, FLOOR, call, nanos(trip.to_device + trip.to_host)?)?;
        }
        Ok(())
    }

    /// A stage with no GPU implementation: timed on the host in a `cpu` run, floored in a `cuda` run.
    fn host_stage(
        &mut self,
        stage: &str,
        sizes: impl Fn(&Clip) -> (usize, usize),
        mut call: impl FnMut(usize, &Clip) -> Result<(), EvalError>,
    ) -> Result<(), EvalError> {
        if self.options.provider == Provider::Cuda {
            let sizes: Vec<(usize, usize)> = self.clips.iter().map(sizes).collect();
            return self.floor(stage, &sizes);
        }
        for (utt, clip) in self.clips.iter().enumerate() {
            call(utt, clip)?;
        }
        let calls: Vec<(u32, usize)> = self.calls().collect();
        let mut times = Vec::with_capacity(calls.len());
        for (repeat, utt) in &calls {
            let clip = &self.clips[*utt];
            let (outcome, ns) = timed(|| call(*utt, clip))?;
            outcome?;
            times.push(((*repeat, *utt), ns));
        }
        for (call, ns) in times {
            self.record(stage, HOST, call, ns)?;
        }
        Ok(())
    }

    fn measure_resample(&mut self) -> Result<(), EvalError> {
        let up = LinearResampler::create(SAMPLE_RATE, CAPTURE_RATE)
            .ok_or_else(|| invalid("cannot create the up-sampler"))?;
        let down = LinearResampler::create(CAPTURE_RATE, SAMPLE_RATE)
            .ok_or_else(|| invalid("cannot create the down-sampler"))?;
        let captured: Vec<Vec<i16>> = self
            .clips
            .iter()
            .map(|clip| {
                up.reset();
                to_pcm(&up.resample(&clip.samples, true))
            })
            .collect();
        self.host_stage(
            "resample",
            // Device bound: the 48 kHz capture as 16-bit samples; host bound: the 16 kHz result as floats.
            |clip| (clip.samples.len() * 3 * 2, clip.samples.len() * 4),
            |utt, _| {
                let floats: Vec<f32> = captured[utt]
                    .iter()
                    .map(|s| f32::from(*s) / 32768.0)
                    .collect();
                down.reset();
                black_box(down.resample(&floats, true));
                Ok(())
            },
        )
    }

    fn measure_ring(&mut self) -> Result<(), EvalError> {
        let longest = self
            .clips
            .iter()
            .map(|c| c.samples.len())
            .max()
            .ok_or_else(|| invalid("no utterances"))?;
        let capacity = i32::try_from(longest + PREROLL)
            .map_err(|_| invalid("the ring capacity does not fit i32"))?;
        let ring = CircularBuffer::new(capacity)
            .ok_or_else(|| invalid("cannot create the ring buffer"))?;
        self.host_stage(
            "ring",
            |_| (PREROLL * 4, PREROLL * 4),
            |_, clip| {
                ring.reset();
                ring.push(&clip.samples);
                let length = i32::try_from(clip.samples.len())
                    .map_err(|_| invalid("an utterance does not fit i32"))?;
                let wanted = length.min(
                    i32::try_from(PREROLL).map_err(|_| invalid("the preroll does not fit i32"))?,
                );
                black_box(ring.get(ring.head(), wanted));
                ring.pop(length);
                Ok(())
            },
        )
    }

    fn measure_vad(&mut self) -> Result<(), EvalError> {
        let provider = self.provider();
        let detector = models::vad(&self.options.models, provider)?;
        self.assert_provider()?;
        let tail = vec![0.0_f32; VAD_TAIL];
        let feed = |samples: &[f32]| {
            for window in samples.chunks(VAD_WINDOW) {
                detector.accept_waveform(window);
            }
        };
        // One untimed pass warms the provider.
        feed(&self.clips[0].samples);
        feed(&tail);
        detector.reset();
        let calls: Vec<(u32, usize)> = self.calls().collect();
        let mut times = Vec::with_capacity(calls.len());
        for call in calls {
            detector.reset();
            let clip = &self.clips[call.1];
            let (segments, ns) = timed(|| {
                feed(&clip.samples);
                feed(&tail);
                let mut segments = 0_u32;
                while !detector.is_empty() {
                    black_box(detector.front());
                    detector.pop();
                    segments += 1;
                }
                segments
            })?;
            black_box(segments);
            times.push((call, ns));
        }
        for (call, ns) in times {
            self.record("vad", Precision::Fp32.name(), call, ns)?;
        }
        Ok(())
    }

    fn measure_lid(&mut self) -> Result<(), EvalError> {
        let provider = self.provider();
        for precision in precisions(self.options.provider, "lid", false) {
            let identifier = models::language_id(&self.options.models, provider, precision)?;
            self.assert_provider()?;
            let identify = |samples: &[f32]| {
                let stream = identifier.create_stream();
                stream.accept_waveform(SAMPLE_RATE, samples);
                identifier.compute(&stream)
            };
            let head = |clip: &Clip| clip.samples.len().min(LID_SECONDS * SAMPLE_RATE as usize);
            black_box(identify(&self.clips[0].samples[..head(&self.clips[0])]));
            let calls: Vec<(u32, usize)> = self.calls().collect();
            let mut times = Vec::with_capacity(calls.len());
            for call in calls {
                let clip = &self.clips[call.1];
                let (found, ns) = timed(|| identify(&clip.samples[..head(clip)]))?;
                if found.is_none() {
                    return Err(invalid("the language identifier gave no answer"));
                }
                times.push((call, ns, clip.samples.len() >= LONG_UTTERANCE));
            }
            for (call, ns, long) in times {
                self.record("lid", precision.name(), call, ns)?;
                if long {
                    self.record("lid_long", precision.name(), call, ns)?;
                }
            }
        }
        Ok(())
    }

    fn measure_recognize(&mut self) -> Result<(), EvalError> {
        let provider = self.provider();
        let profile = self.options.profile_ranges;
        if profile {
            self.transfer()?;
        }
        let variants = precisions(
            self.options.provider,
            "recognize",
            self.options.int8_on_cuda,
        );
        for (index, precision) in variants.into_iter().enumerate() {
            let (recognizer, created) =
                timed(|| models::recognizer(&self.options.models, provider, precision))?;
            let recognizer = recognizer?;
            self.assert_provider()?;
            self.record("recognizer_create", precision.name(), (0, 0), created)?;
            let first = &self.clips[0];
            let (_, first_ns) = timed(|| {
                let stream = recognizer.create_stream();
                stream.accept_waveform(SAMPLE_RATE, &first.samples);
                recognizer.decode(&stream);
                stream.get_result()
            })?;
            self.record("decode_first", precision.name(), (0, 0), first_ns)?;
            let calls: Vec<(u32, usize)> = self.calls().collect();
            let mut times = Vec::with_capacity(calls.len());
            let mut texts = vec![String::new(); self.clips.len()];
            for call in calls {
                let clip = &self.clips[call.1];
                let begun = Instant::now();
                let stream = recognizer.create_stream();
                let (_, fbank) = timed(|| stream.accept_waveform(SAMPLE_RATE, &clip.samples))?;
                if profile {
                    self.transfer()?
                        .profiler_start()
                        .map_err(|e| invalid(e.to_string()))?;
                }
                let (_, decode) = timed(|| recognizer.decode(&stream))?;
                if profile {
                    self.transfer()?
                        .profiler_stop()
                        .map_err(|e| invalid(e.to_string()))?;
                }
                let result = stream
                    .get_result()
                    .ok_or_else(|| invalid("the recognizer gave no result"))?;
                let total = nanos(begun.elapsed())?;
                texts[call.1] = result.text;
                times.push((call, fbank, decode, total));
            }
            for (call, fbank, decode, total) in times {
                self.record("fbank", precision.name(), call, fbank)?;
                self.record("decode", precision.name(), call, decode)?;
                self.record("recognize_total", precision.name(), call, total)?;
            }
            if index == 0 {
                self.texts = texts;
            }
        }
        Ok(())
    }

    fn measure_judge(&mut self) -> Result<(), EvalError> {
        let texts: Vec<String> = if self.texts.len() == self.clips.len() {
            self.texts.clone()
        } else {
            self.clips.iter().map(|c| c.reference.clone()).collect()
        };
        self.host_stage(
            "judge",
            |_| (TEXT_BYTES, 1),
            |utt, clip| {
                let seconds = clip.samples.len() as f32 / SAMPLE_RATE as f32;
                let characters = texts[utt].chars().filter(|c| c.is_alphanumeric()).count();
                black_box(characters as f32 / seconds < SUSPECT_CHARS_PER_SECOND);
                Ok(())
            },
        )
    }

    fn measure_speaker_embed(&mut self) -> Result<(), EvalError> {
        let provider = self.provider();
        let extractor = models::speaker_extractor(&self.options.models, provider)?;
        self.assert_provider()?;
        let embed = |samples: &[f32]| -> Result<Vec<f32>, EvalError> {
            let stream = extractor
                .create_stream()
                .ok_or_else(|| invalid("cannot create the embedding stream"))?;
            stream.accept_waveform(SAMPLE_RATE, samples);
            stream.input_finished();
            if !extractor.is_ready(&stream) {
                return Err(invalid("the embedding stream is not ready"));
            }
            extractor
                .compute(&stream)
                .ok_or_else(|| invalid("the extractor gave no embedding"))
        };
        embed(&self.clips[0].samples)?;
        let calls: Vec<(u32, usize)> = self.calls().collect();
        let mut times = Vec::with_capacity(calls.len());
        let mut embeddings = vec![Vec::new(); self.clips.len()];
        for call in calls {
            let clip = &self.clips[call.1];
            let (vector, ns) = timed(|| embed(&clip.samples))?;
            embeddings[call.1] = vector?;
            times.push((call, ns));
        }
        for (call, ns) in times {
            self.record("speaker_embed", Precision::Fp32.name(), call, ns)?;
        }
        self.embeddings = embeddings;
        Ok(())
    }

    fn measure_speaker_assign(&mut self) -> Result<(), EvalError> {
        if self.embeddings.len() != self.clips.len() {
            return Err(invalid(
                "speaker_assign needs the embeddings of speaker_embed in the same run",
            ));
        }
        let embeddings = std::mem::take(&mut self.embeddings);
        let dim = embeddings[0].len();
        let centroids: Vec<Vec<f32>> = embeddings.iter().take(CENTROIDS).cloned().collect();
        let mut scratch = vec![0.0_f32; dim];
        let outcome = self.host_stage(
            "speaker_assign",
            |_| (dim * 4, 4),
            |utt, _| {
                let vector = &embeddings[utt];
                let norm = vector.iter().map(|x| x * x).sum::<f32>().sqrt();
                let mut best = (0_usize, f32::MIN);
                for (index, centroid) in centroids.iter().enumerate() {
                    let dot: f32 = vector.iter().zip(centroid).map(|(a, b)| a * b).sum();
                    let length = centroid.iter().map(|x| x * x).sum::<f32>().sqrt();
                    let cosine = dot / (norm * length);
                    if cosine > best.1 {
                        best = (index, cosine);
                    }
                }
                for ((slot, old), new) in scratch.iter_mut().zip(&centroids[best.0]).zip(vector) {
                    *slot = (old + new) * 0.5;
                }
                black_box(&scratch);
                Ok(())
            },
        );
        self.embeddings = embeddings;
        outcome
    }

    fn measure_rediarize(&mut self) -> Result<(), EvalError> {
        let provider = self.provider();
        for precision in precisions(self.options.provider, "rediarize", false) {
            let diarizer = models::diarizer(&self.options.models, provider, precision)?;
            self.assert_provider()?;
            let run = |samples: &[f32]| diarizer.process(samples).map(|r| r.num_segments());
            black_box(run(&self.clips[0].samples));
            let calls: Vec<(u32, usize)> = self.calls().collect();
            let mut times = Vec::with_capacity(calls.len());
            for call in calls {
                let clip = &self.clips[call.1];
                let (found, ns) = timed(|| run(&clip.samples))?;
                if found.is_none() {
                    return Err(invalid("the diarizer gave no result"));
                }
                times.push((call, ns));
            }
            for (call, ns) in times {
                self.record("rediarize", precision.name(), call, ns)?;
            }
        }
        Ok(())
    }

    fn measure_translate(&mut self) -> Result<(), EvalError> {
        let Some(url) = &self.options.llama_url else {
            return Err(invalid("the translate stage needs --llama-url"));
        };
        let client = ChatClient::new(url, TRANSLATE_TIMEOUT);
        let bodies: Vec<String> = self
            .clips
            .iter()
            .map(|clip| translate_body(&clip.reference))
            .collect::<Result<_, _>>()?;
        client.complete(&bodies[0])?;
        let calls: Vec<(u32, usize)> = self.calls().collect();
        let mut times = Vec::with_capacity(calls.len());
        let mut translations = vec![String::new(); self.clips.len()];
        for call in calls {
            let (answer, ns) = timed(|| client.complete(&bodies[call.1]))?;
            translations[call.1] = answer?.trim().to_owned();
            times.push((call, ns));
        }
        for (call, ns) in times {
            self.record("translate", TRANSLATION_MODEL, call, ns)?;
        }
        self.translations = translations;
        Ok(())
    }

    fn measure_prompt_build(&mut self) -> Result<(), EvalError> {
        self.host_stage(
            "prompt_build",
            |clip| (clip.reference.len(), TEXT_BYTES),
            |_, clip| {
                black_box(translate_body(&clip.reference)?);
                Ok(())
            },
        )
    }

    fn measure_verify_convert(&mut self) -> Result<(), EvalError> {
        if self.translations.len() != self.clips.len() {
            return Err(invalid(
                "verify_convert needs the translations of the translate stage in the same run",
            ));
        }
        let translations = std::mem::take(&mut self.translations);
        let mut verifier = Verifier::open(None, &mut NullSink).map_err(invalid)?;
        let mut annotator =
            Annotator::open(&AnnotatorConfig::default(), &mut NullSink).map_err(invalid)?;
        let now_epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| invalid(e.to_string()))
            .and_then(|d| {
                i64::try_from(d.as_secs()).map_err(|_| invalid("the epoch does not fit i64"))
            })?;
        let outcome = self.host_stage(
            "verify_convert",
            |clip| (clip.reference.len(), TEXT_BYTES),
            |utt, clip| {
                let translation = &translations[utt];
                black_box(verifier.verify(&clip.reference, translation, Lang::Ja, &mut NullSink));
                black_box(annotator.annotate(
                    &AnnotateRequest {
                        id: "stage",
                        src: &clip.reference,
                        translation,
                        src_lang: Lang::Ja,
                        now_epoch,
                    },
                    &mut NullSink,
                ));
                Ok(())
            },
        );
        self.translations = translations;
        outcome
    }

    /// The inverse text normalization of the reference text of each utterance, with the default exclusions.
    fn measure_itn(&mut self) -> Result<(), EvalError> {
        let overrides = Overrides::default();
        self.host_stage(
            "itn",
            |clip| (clip.reference.len(), clip.reference.len()),
            |_, clip| {
                black_box(itn::convert(&clip.reference, "ja", &overrides));
                Ok(())
            },
        )
    }

    /// The string rules after the punctuation BERT: where the marks go and which sentence asks.
    /// The scores are a fixed pattern of the right shape, because the rules read numbers only.
    fn measure_punct_rules(&mut self) -> Result<(), EvalError> {
        let inputs: Vec<(Vec<char>, Vec<f32>)> = self
            .clips
            .iter()
            .map(|clip| {
                let mut chars = tokenizer::normalized_chars(&clip.reference);
                chars.truncate(rules::MAX_CHARS);
                let logits = (0..(chars.len() + 2) * 2)
                    .map(|i| if i % 11 == 3 { 2.0 } else { -2.0 })
                    .collect();
                (chars, logits)
            })
            .collect();
        self.host_stage(
            "punct_rules",
            |clip| (clip.reference.len(), clip.reference.len()),
            |utt, _| {
                let (chars, logits) = &inputs[utt];
                let text = rules::punctuate(chars, logits).map_err(|e| invalid(e.to_string()))?;
                black_box(rules::apply_question_marks(&text));
                Ok(())
            },
        )
    }

    /// The punctuation BERT's forward alone, on the `input_ids` of the fixture, one case per utterance.
    fn measure_punct_bert(&mut self) -> Result<(), EvalError> {
        let cases = self
            .options
            .punct_cases
            .clone()
            .ok_or_else(|| invalid("--punct-cases is needed for the punct_bert stage"))?;
        let ids = punct_ids(&cases, self.clips.len())?;
        let model = models::punct_bert_file(&self.options.models)?;
        let device = self.device();
        let mut session = self.ort()?.session(&model, device, BERT_THREADS, false)?;
        self.assert_provider()?;
        // One untimed pass: the first run on CUDA builds its plans and warms the kernels.
        for case in &ids {
            ort_stages::forward_bert(&mut session, case)?;
        }
        let calls: Vec<(u32, usize)> = self.calls().collect();
        let mut times = Vec::with_capacity(calls.len());
        for call in calls {
            times.push((call, ort_stages::forward_bert(&mut session, &ids[call.1])?));
        }
        for (call, ns) in times {
            self.record("punct_bert", Precision::Fp32.name(), call, ns)?;
        }
        Ok(())
    }

    /// The recognizer's encoder model alone (fp32), on a log-mel of each utterance, through `ort`.
    fn measure_encoder_only(&mut self) -> Result<(), EvalError> {
        let model = models::encoder_file(&self.options.models, Precision::Fp32)?;
        let device = self.device();
        let threads = usize::try_from(models::RECOGNIZER_THREADS)
            .map_err(|_| invalid("the thread count does not fit usize"))?;
        let mut session = self.ort()?.session(&model, device, threads, false)?;
        self.assert_provider()?;
        let features: Vec<(Vec<f32>, usize)> = self
            .clips
            .iter()
            .map(|clip| ort_stages::log_mel(&clip.samples))
            .collect();
        for (values, frames) in &features {
            ort_stages::forward_encoder(&mut session, values, *frames)?;
        }
        let calls: Vec<(u32, usize)> = self.calls().collect();
        let mut times = Vec::with_capacity(calls.len());
        for call in calls {
            let (values, frames) = &features[call.1];
            times.push((
                call,
                ort_stages::forward_encoder(&mut session, values, *frames)?,
            ));
        }
        for (call, ns) in times {
            self.record("encoder_only", ENCODER_ONLY, call, ns)?;
        }
        Ok(())
    }

    fn measure(&mut self, stage: &str) -> Result<(), EvalError> {
        log::info(
            "eval.stage.started",
            "worker",
            "stage started",
            &[("stage", Value::from(stage))],
        );
        let outcome = match stage {
            "resample" => self.measure_resample(),
            "ring" => self.measure_ring(),
            "vad" => self.measure_vad(),
            "lid" => self.measure_lid(),
            "recognize" => self.measure_recognize(),
            "judge" => self.measure_judge(),
            "speaker_embed" => self.measure_speaker_embed(),
            "speaker_assign" => self.measure_speaker_assign(),
            "rediarize" => self.measure_rediarize(),
            "translate" => self.measure_translate(),
            "prompt_build" => self.measure_prompt_build(),
            "verify_convert" => self.measure_verify_convert(),
            "punct_bert" => self.measure_punct_bert(),
            "encoder_only" => self.measure_encoder_only(),
            "itn" => self.measure_itn(),
            "punct_rules" => self.measure_punct_rules(),
            other => Err(invalid(format!("unknown stage `{other}`"))),
        };
        let level = if outcome.is_ok() {
            Level::Info
        } else {
            Level::Error
        };
        log::emit(
            level,
            "eval.stage.finished",
            "worker",
            "stage finished",
            &[
                ("stage", Value::from(stage)),
                ("samples", Value::from(self.out.len())),
            ],
        );
        outcome
    }
}

/// Runs every wanted stage and returns the samples.
pub fn run(options: &Options) -> Result<RunFile, EvalError> {
    if options.repeat == 0 {
        return Err(invalid("--repeat must be at least 1"));
    }
    if let Some(unknown) = options
        .only
        .iter()
        .find(|only| !STAGES.contains(&only.as_str()))
    {
        return Err(invalid(format!("unknown stage `{unknown}`")));
    }
    let wav = read_wav(&options.wav)?;
    let rows = read_rows(&options.manifest)?;
    let utterances = utterances_of(&rows, wav.samples.len())?;
    let clips = clips_of(&wav.samples, &utterances);
    let mut state = Run {
        options,
        clips,
        out: Vec::new(),
        transfer: None,
        texts: Vec::new(),
        translations: Vec::new(),
        embeddings: Vec::new(),
        ort: None,
    };
    let load = match &options.load_url {
        Some(url) => {
            let sentences: Vec<String> = state.clips.iter().map(|c| c.reference.clone()).collect();
            Some(Load::start(url, &sentences, options.load_clients)?)
        }
        None => None,
    };
    let mut outcome = Ok(());
    for stage in STAGES {
        if state.wants(stage) {
            outcome = state.measure(stage);
            if outcome.is_err() {
                break;
            }
        }
    }
    if let Some(load) = load {
        let count = load.stop()?;
        log::info(
            "eval.load.finished",
            "worker",
            "load stopped",
            &[
                ("done", Value::from(count.done)),
                ("failed", Value::from(count.failed)),
            ],
        );
    }
    outcome?;
    let meta = Meta {
        provider: options.provider.name().to_owned(),
        load: options.load_url.is_some(),
        cache: options.cache.clone(),
        run: options.run,
        repeat: options.repeat,
        utterances: state.clips.len(),
        os: std::env::consts::OS.to_owned(),
    };
    Ok(RunFile {
        meta,
        samples: state.out,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cuda_recognition_loads_fp32_and_fp16_and_int8_only_when_asked() {
        assert_eq!(
            precisions(Provider::Cuda, "recognize", false),
            [Precision::Fp32, Precision::Fp16]
        );
        assert_eq!(
            precisions(Provider::Cuda, "recognize", true),
            [Precision::Fp32, Precision::Fp16, Precision::Int8]
        );
    }

    #[test]
    fn the_cpu_runs_the_int8_model_first_and_fp32_for_comparison() {
        assert_eq!(
            precisions(Provider::Cpu, "lid", false),
            [Precision::Int8, Precision::Fp32]
        );
    }

    #[test]
    fn other_cuda_stages_use_fp32_only() {
        assert_eq!(precisions(Provider::Cuda, "vad", false), [Precision::Fp32]);
    }

    #[test]
    fn pcm_round_trips_through_float_within_one_step() {
        let pcm = [-32768_i16, -1, 0, 1, 32767];
        let floats: Vec<f32> = pcm.iter().map(|s| f32::from(*s) / 32768.0).collect();
        assert_eq!(to_pcm(&floats), pcm);
    }

    #[test]
    fn every_listed_stage_has_a_distinct_name() {
        let mut names = STAGES.to_vec();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), STAGES.len());
    }

    fn options(repeat: u32, only: &[&str]) -> Options {
        Options {
            provider: Provider::Cpu,
            wav: PathBuf::from("x.wav"),
            manifest: PathBuf::from("x.json"),
            models: PathBuf::from("m"),
            repeat,
            llama_url: None,
            load_url: None,
            load_clients: 2,
            cache: String::from("warm"),
            run: 1,
            only: only.iter().map(|s| (*s).to_owned()).collect(),
            int8_on_cuda: false,
            profile_ranges: false,
            punct_cases: None,
        }
    }

    #[test]
    fn a_run_with_zero_repeats_is_refused() {
        assert!(matches!(run(&options(0, &[])), Err(EvalError::Invalid(_))));
    }

    #[test]
    fn an_unknown_stage_is_refused_before_anything_is_read() {
        let said = run(&options(1, &["nope"])).err().map(|e| e.to_string());
        assert!(
            said.as_deref()
                .is_some_and(|t| t.contains("unknown stage `nope`")),
            "{said:?}"
        );
    }
}
