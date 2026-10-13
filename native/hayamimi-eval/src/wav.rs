//! 16-bit PCM mono wav files, the only audio form the evaluation sets use.

use std::path::Path;

use hound::{SampleFormat, WavReader, WavSpec, WavWriter};

use crate::error::EvalError;

/// The sample rate every evaluation set is stored at.
pub const SAMPLE_RATE: u32 = 16_000;

/// A mono recording.
#[derive(Debug, PartialEq, Eq)]
pub struct Wav {
    pub sample_rate: u32,
    pub samples: Vec<i16>,
}

fn wav_error(path: &Path, message: impl Into<String>) -> EvalError {
    EvalError::Wav {
        path: path.display().to_string(),
        message: message.into(),
    }
}

/// Reads a 16-bit PCM mono wav. Anything else is refused: a stereo or
/// float file would be streamed as garbage rather than as an error.
pub fn read_wav(path: &Path) -> Result<Wav, EvalError> {
    let mut reader = WavReader::open(path).map_err(|e| wav_error(path, e.to_string()))?;
    let spec = reader.spec();
    if spec.channels != 1 || spec.bits_per_sample != 16 || spec.sample_format != SampleFormat::Int {
        return Err(wav_error(
            path,
            format!(
                "expected 16-bit PCM mono, found {} channel(s), {} bit, {:?}",
                spec.channels, spec.bits_per_sample, spec.sample_format
            ),
        ));
    }
    let samples = reader
        .samples::<i16>()
        .collect::<Result<Vec<i16>, _>>()
        .map_err(|e| wav_error(path, e.to_string()))?;
    Ok(Wav {
        sample_rate: spec.sample_rate,
        samples,
    })
}

/// Writes `samples` as a 16-bit PCM mono wav.
pub fn write_wav(path: &Path, sample_rate: u32, samples: &[i16]) -> Result<(), EvalError> {
    let spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec).map_err(|e| wav_error(path, e.to_string()))?;
    for sample in samples {
        writer
            .write_sample(*sample)
            .map_err(|e| wav_error(path, e.to_string()))?;
    }
    writer
        .finalize()
        .map_err(|e| wav_error(path, e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::scratch;

    #[test]
    fn a_written_wav_reads_back_identically() -> Result<(), EvalError> {
        let path = scratch("roundtrip.wav");
        write_wav(&path, SAMPLE_RATE, &[0, 1, -1, i16::MAX, i16::MIN])?;
        let back = read_wav(&path)?;
        let _ = std::fs::remove_file(&path);
        assert_eq!(
            back,
            Wav {
                sample_rate: SAMPLE_RATE,
                samples: vec![0, 1, -1, i16::MAX, i16::MIN]
            }
        );
        Ok(())
    }

    #[test]
    fn an_empty_wav_reads_back_empty() -> Result<(), EvalError> {
        let path = scratch("empty.wav");
        write_wav(&path, SAMPLE_RATE, &[])?;
        let back = read_wav(&path)?;
        let _ = std::fs::remove_file(&path);
        assert!(back.samples.is_empty());
        Ok(())
    }

    #[test]
    fn a_missing_file_names_its_path() {
        let said = read_wav(Path::new("missing/x.wav"))
            .err()
            .map(|e| e.to_string());
        assert!(
            said.as_deref()
                .is_some_and(|t| t.starts_with("missing/x.wav")),
            "{said:?}"
        );
    }

    #[test]
    fn a_stereo_wav_is_refused() -> Result<(), EvalError> {
        let path = scratch("stereo.wav");
        let spec = WavSpec {
            channels: 2,
            sample_rate: SAMPLE_RATE,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut writer =
            WavWriter::create(&path, spec).map_err(|e| wav_error(&path, e.to_string()))?;
        writer
            .write_sample(0_i16)
            .map_err(|e| wav_error(&path, e.to_string()))?;
        writer
            .write_sample(0_i16)
            .map_err(|e| wav_error(&path, e.to_string()))?;
        writer
            .finalize()
            .map_err(|e| wav_error(&path, e.to_string()))?;
        let said = read_wav(&path).err().map(|e| e.to_string());
        let _ = std::fs::remove_file(&path);
        assert!(
            said.as_deref().is_some_and(|t| t.contains("2 channel")),
            "{said:?}"
        );
        Ok(())
    }
}
