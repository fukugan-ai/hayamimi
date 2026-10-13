//! Opens one wave file on the CUDA provider and reports what the process
//! loaded to do it.
//!
//! The report states facts for the people who judge them: how many CUDA
//! devices exist, which provider library is loaded after the session ran,
//! which ONNX Runtime copies are loaded, and which CUDA runtime, cuDNN and
//! ONNX Runtime libraries sit in the process. It decides nothing.

use std::path::PathBuf;
use std::time::Instant;

use serde::Serialize;
use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig, Wave};

use crate::cuda;
use crate::error::GpuError;
use crate::modules::{self, LoadedModule};
use crate::runtime;

/// The provider the recognizer is asked to run on.
const REQUESTED_PROVIDER: &str = "cuda";
/// File name of the CUDA provider library of ONNX Runtime.
#[cfg(windows)]
const CUDA_PROVIDER_MODULE: &str = "onnxruntime_providers_cuda.dll";
#[cfg(not(windows))]
const CUDA_PROVIDER_MODULE: &str = "libonnxruntime_providers_cuda.so";
/// File name of the ONNX Runtime library that sherpa-onnx links.
#[cfg(windows)]
const ONNXRUNTIME_MODULE: &str = "onnxruntime.dll";
#[cfg(not(windows))]
const ONNXRUNTIME_MODULE: &str = "libonnxruntime.so";
/// The name the ONNX Runtime library is opened by to ask its version: the file
/// name on Windows, the SONAME on Linux. A differently named copy of the file
/// would be loaded as a second library.
#[cfg(windows)]
const ONNXRUNTIME_OPEN_NAME: &str = "onnxruntime.dll";
#[cfg(not(windows))]
const ONNXRUNTIME_OPEN_NAME: &str = "libonnxruntime.so.1";
/// Lower-case name prefixes of the modules worth reporting, as Windows and Linux spell them.
const REPORTED_PREFIXES: [&str; 8] = [
    "onnxruntime",
    "sherpa",
    "cu",
    "nv",
    "libonnxruntime",
    "libsherpa",
    "libcu",
    "libnv",
];

const FLAGS: [&str; 5] = ["--encoder", "--decoder", "--joiner", "--tokens", "--wav"];

/// The model files and the wave file to open.
#[derive(Debug, PartialEq, Eq)]
pub struct ProbeArgs {
    pub encoder: PathBuf,
    pub decoder: PathBuf,
    pub joiner: PathBuf,
    pub tokens: PathBuf,
    pub wav: PathBuf,
}

impl ProbeArgs {
    pub fn parse(args: &[String]) -> Result<ProbeArgs, GpuError> {
        let mut found: Vec<(&'static str, PathBuf)> = Vec::new();
        let mut rest = args.iter();
        while let Some(given) = rest.next() {
            let Some(name) = FLAGS.iter().copied().find(|known| *known == given.as_str()) else {
                return Err(GpuError::Usage(format!("unknown flag `{given}`")));
            };
            let Some(value) = rest.next() else {
                return Err(GpuError::Usage(format!("`{name}` needs a value")));
            };
            if found.iter().any(|(seen, _)| *seen == name) {
                return Err(GpuError::Usage(format!("`{name}` was given twice")));
            }
            found.push((name, PathBuf::from(value)));
        }
        let mut take = |name: &'static str| -> Result<PathBuf, GpuError> {
            let at = found.iter().position(|(seen, _)| *seen == name);
            match at {
                Some(at) => Ok(found.swap_remove(at).1),
                None => Err(GpuError::Usage(format!("`{name}` is required"))),
            }
        };
        Ok(ProbeArgs {
            encoder: take("--encoder")?,
            decoder: take("--decoder")?,
            joiner: take("--joiner")?,
            tokens: take("--tokens")?,
            wav: take("--wav")?,
        })
    }
}

/// What one run observed.
#[derive(Debug, Serialize)]
pub struct Probe {
    pub os: &'static str,
    pub requested_provider: &'static str,
    pub cuda_devices: u32,
    /// `cuda` when the CUDA provider library is loaded after the session ran, otherwise `cpu`.
    pub loaded_provider: &'static str,
    pub ort_version: String,
    /// How many copies of the ONNX Runtime library are loaded.
    pub onnxruntime_libraries: u32,
    pub audio_ms: u64,
    pub decode_ms: u64,
    pub decoded_chars: u64,
    /// Loaded modules whose names start with a reported prefix.
    pub modules: Vec<LoadedModule>,
}

/// `cuda` when the CUDA provider library is among the loaded modules.
pub fn provider_of(modules: &[LoadedModule]) -> &'static str {
    if modules
        .iter()
        .any(|row| modules::is_named(&row.name, CUDA_PROVIDER_MODULE))
    {
        "cuda"
    } else {
        "cpu"
    }
}

/// How many loaded modules are the ONNX Runtime library itself (not a provider).
pub fn count_onnxruntime(modules: &[LoadedModule]) -> Result<u32, GpuError> {
    let count = modules
        .iter()
        .filter(|row| modules::is_named(&row.name, ONNXRUNTIME_MODULE))
        .count();
    u32::try_from(count).map_err(|_| GpuError::Overflow("the ONNX Runtime copy count"))
}

/// The modules whose names start with one of the reported prefixes.
pub fn reported(modules: Vec<LoadedModule>) -> Vec<LoadedModule> {
    modules
        .into_iter()
        .filter(|row| {
            REPORTED_PREFIXES
                .iter()
                .any(|prefix| row.name.starts_with(prefix))
        })
        .collect()
}

fn utf8(path: &std::path::Path) -> Result<String, GpuError> {
    match path.to_str() {
        Some(text) => Ok(text.to_owned()),
        None => Err(GpuError::PathNotUtf8),
    }
}

/// What opening the wave file cost and produced, and the modules loaded while
/// the recognizer was still alive (ONNX Runtime unloads its provider libraries
/// when the last session is released, so a later snapshot would miss them).
struct Opened {
    audio_ms: u64,
    decode_ms: u64,
    decoded_chars: u64,
    modules: Vec<LoadedModule>,
}

/// Creates a recognizer on the CUDA provider and decodes the wave file once.
fn open_audio(args: &ProbeArgs) -> Result<Opened, GpuError> {
    let mut config = OfflineRecognizerConfig::default();
    config.model_config.transducer = OfflineTransducerModelConfig {
        encoder: Some(utf8(&args.encoder)?),
        decoder: Some(utf8(&args.decoder)?),
        joiner: Some(utf8(&args.joiner)?),
    };
    config.model_config.tokens = Some(utf8(&args.tokens)?);
    config.model_config.provider = Some(String::from(REQUESTED_PROVIDER));
    config.model_config.num_threads = 2;
    config.decoding_method = Some(String::from("greedy_search"));

    let Some(wave) = Wave::read(&utf8(&args.wav)?) else {
        return Err(GpuError::Wave);
    };
    let rate = i64::from(wave.sample_rate());
    if rate <= 0 {
        return Err(GpuError::SampleRate(wave.sample_rate()));
    }
    let audio_ms = u64::try_from(i64::from(wave.num_samples()) * 1000 / rate)
        .map_err(|_| GpuError::Overflow("the audio length"))?;

    let Some(recognizer) = OfflineRecognizer::create(&config) else {
        return Err(GpuError::Recognizer);
    };
    let started = Instant::now();
    let stream = recognizer.create_stream();
    stream.accept_waveform(wave.sample_rate(), wave.samples());
    recognizer.decode(&stream);
    let Some(result) = stream.get_result() else {
        return Err(GpuError::NoResult);
    };
    let decode_ms = u64::try_from(started.elapsed().as_millis())
        .map_err(|_| GpuError::Overflow("the decode time"))?;
    let decoded_chars = u64::try_from(result.text.chars().count())
        .map_err(|_| GpuError::Overflow("the decoded length"))?;
    Ok(Opened {
        audio_ms,
        decode_ms,
        decoded_chars,
        modules: modules::loaded_modules()?,
    })
}

/// Opens the audio on the CUDA provider and reads the process module list
/// while the recognizer is alive.
pub fn run(args: &ProbeArgs) -> Result<Probe, GpuError> {
    // Only Windows needs it: the C library there can bind `onnxruntime.dll` to `System32`.
    // On Linux the C library names `libonnxruntime.so.1`, found beside the executable.
    #[cfg(windows)]
    runtime::preload_beside_executable(ONNXRUNTIME_MODULE)?;
    let cuda_devices = cuda::device_count()?;
    let opened = open_audio(args)?;
    let all_modules = opened.modules;
    Ok(Probe {
        os: std::env::consts::OS,
        requested_provider: REQUESTED_PROVIDER,
        cuda_devices,
        loaded_provider: provider_of(&all_modules),
        ort_version: runtime::onnxruntime_version(ONNXRUNTIME_OPEN_NAME)?,
        onnxruntime_libraries: count_onnxruntime(&all_modules)?,
        audio_ms: opened.audio_ms,
        decode_ms: opened.decode_ms,
        decoded_chars: opened.decoded_chars,
        modules: reported(all_modules),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::Origin;

    fn row(name: &str) -> LoadedModule {
        LoadedModule {
            name: name.to_owned(),
            origin: Origin::ExeDirectory,
        }
    }

    fn words(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    #[test]
    fn the_cuda_provider_module_makes_the_provider_cuda() {
        let loaded = [row(ONNXRUNTIME_MODULE), row(CUDA_PROVIDER_MODULE)];
        assert_eq!(provider_of(&loaded), "cuda");
    }

    #[test]
    fn without_the_cuda_provider_module_the_provider_is_cpu() {
        assert_eq!(provider_of(&[row(ONNXRUNTIME_MODULE)]), "cpu");
        assert_eq!(provider_of(&[]), "cpu");
    }

    #[test]
    fn only_the_library_itself_counts_as_an_onnxruntime_copy() -> Result<(), GpuError> {
        let loaded = [
            row(ONNXRUNTIME_MODULE),
            row(CUDA_PROVIDER_MODULE),
            row("libonnxruntime_providers_shared.so"),
            row("onnxruntime_providers_shared.dll"),
        ];
        assert_eq!(count_onnxruntime(&loaded)?, 1);
        assert_eq!(count_onnxruntime(&[])?, 0);
        Ok(())
    }

    #[test]
    fn modules_are_reported_by_name_prefix() {
        let kept = reported(vec![
            row("kernel32.dll"),
            row("cudart64_13.dll"),
            row("nvcuda.dll"),
            row("sherpa-onnx-c-api.dll"),
            row("onnxruntime.dll"),
        ]);
        let names: Vec<&str> = kept.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "cudart64_13.dll",
                "nvcuda.dll",
                "sherpa-onnx-c-api.dll",
                "onnxruntime.dll"
            ]
        );
    }

    #[test]
    fn all_five_flags_are_read() -> Result<(), GpuError> {
        let parsed = ProbeArgs::parse(&words(&[
            "--encoder",
            "e",
            "--decoder",
            "d",
            "--joiner",
            "j",
            "--tokens",
            "t",
            "--wav",
            "w",
        ]))?;
        assert_eq!(parsed.encoder, PathBuf::from("e"));
        assert_eq!(parsed.wav, PathBuf::from("w"));
        Ok(())
    }

    #[test]
    fn a_missing_flag_is_named() {
        let said = ProbeArgs::parse(&words(&["--encoder", "e"]));
        assert!(matches!(said, Err(GpuError::Usage(why)) if why.contains("--decoder")));
    }

    #[test]
    fn an_unknown_repeated_or_valueless_flag_is_refused() {
        for args in [
            words(&["--gpu", "1"]),
            words(&["--encoder", "a", "--encoder", "b"]),
            words(&["--encoder"]),
        ] {
            let said = ProbeArgs::parse(&args);
            assert!(matches!(said, Err(GpuError::Usage(_))), "{args:?} {said:?}");
        }
    }

    #[test]
    fn no_arguments_miss_the_encoder_first() {
        let said = ProbeArgs::parse(&[]);
        assert!(matches!(said, Err(GpuError::Usage(why)) if why.contains("--encoder")));
    }
}
