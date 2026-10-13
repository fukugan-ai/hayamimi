//! Evaluation-set manifests: pinning a Python-made manifest to the bytes of
//! its wav files, and joining clips into one long recording for latency runs.
//!
//! A manifest is a JSON array of rows. Every row names a wav by a path
//! relative to the directory the tool is run from, and carries that wav's
//! SHA-256. Keys are written in sorted order so that pinning the same input
//! twice gives the same bytes.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::error::EvalError;
use crate::sha::{sha256_file, sha256_hex};
use crate::wav::{SAMPLE_RATE, read_wav, write_wav};

pub type Row = Map<String, Value>;

/// Samples per millisecond at the evaluation rate.
const SAMPLES_PER_MS: usize = (SAMPLE_RATE / 1000) as usize;

/// Samples per frame of the speech-end detector: 20 ms.
const END_FRAME: usize = 320;

/// How far from the clip's quiet level to its loud level a frame must reach
/// to count as speech.
const SPEECH_FRACTION: f32 = 0.35;

/// Speech level is never more than this many dB below the clip's loud level,
/// so a stretch of digital silence cannot pull it down into the room noise.
const SPEECH_RANGE_DB: f32 = 25.0;

/// How many frames in a row must reach speech level to count as speech: 100 ms.
/// A click in the noise after the voice is shorter than that.
const SPEECH_RUN: usize = 5;

/// The level of a frame in dB relative to full scale, floored at -120 dB.
fn frame_level_db(frame: &[i16]) -> f32 {
    let energy: f32 = frame.iter().map(|s| f32::from(*s) * f32::from(*s)).sum();
    let mean_square = energy / frame.len() as f32;
    (10.0 * (mean_square / (32768.0 * 32768.0)).log10()).max(-120.0)
}

/// The sample just past the last run of `SPEECH_RUN` frames that all reach
/// speech level, where speech level lies `SPEECH_FRACTION` of the way from
/// the 10th to the 95th percentile of the clip's frame levels. A recorded
/// clip can end in seconds of room noise that a voice detector reads as
/// silence; the delay of a `final` is measured from the end of the speech,
/// not from the end of that noise. `None` for a clip without frames or
/// without such a run.
pub fn speech_end(samples: &[i16]) -> Option<usize> {
    let levels: Vec<f32> = samples.chunks(END_FRAME).map(frame_level_db).collect();
    let mut sorted = levels.clone();
    sorted.sort_by(f32::total_cmp);
    let at = |percent: usize| {
        sorted
            .get(sorted.len().checked_sub(1)? * percent / 100)
            .copied()
    };
    let (quiet, loud) = (at(10)?, at(95)?);
    let threshold = (quiet + SPEECH_FRACTION * (loud - quiet)).max(loud - SPEECH_RANGE_DB);
    let loud_frames: Vec<bool> = levels.iter().map(|level| *level >= threshold).collect();
    let run = SPEECH_RUN.min(loud_frames.len());
    let first = loud_frames
        .windows(run)
        .rposition(|window| window.iter().all(|loud| *loud))?;
    Some(((first + run) * END_FRAME).min(samples.len()))
}

/// A path as the manifests spell it: forward slashes on every platform.
pub fn slash_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Reads a manifest: a JSON array of objects.
pub fn read_rows(path: &Path) -> Result<Vec<Row>, EvalError> {
    let text = std::fs::read_to_string(path).map_err(|e| EvalError::io(path, e))?;
    let value: Value = serde_json::from_str(&text).map_err(|e| EvalError::json(path, e))?;
    let Value::Array(items) = value else {
        return Err(EvalError::Manifest(format!(
            "{} is not a JSON array",
            path.display()
        )));
    };
    items
        .into_iter()
        .enumerate()
        .map(|(index, item)| match item {
            Value::Object(row) => Ok(row),
            _ => Err(EvalError::Manifest(format!(
                "{}: row {index} is not an object",
                path.display()
            ))),
        })
        .collect()
}

/// Writes rows as indented JSON with a trailing newline, making the
/// directory first.
pub fn write_rows(path: &Path, rows: &[Row]) -> Result<(), EvalError> {
    let mut text = serde_json::to_string_pretty(rows).map_err(|e| EvalError::json(path, e))?;
    text.push('\n');
    write_text(path, &text)
}

/// Writes `text` to `path`, making its directory first.
pub fn write_text(path: &Path, text: &str) -> Result<(), EvalError> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| EvalError::io(parent, e))?;
    }
    std::fs::write(path, text).map_err(|e| EvalError::io(path, e))
}

/// The string field `key` of a row, or an error naming the row.
fn str_field<'a>(
    row: &'a Row,
    key: &str,
    origin: &str,
    index: usize,
) -> Result<&'a str, EvalError> {
    row.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| EvalError::Manifest(format!("{origin}: row {index} has no string `{key}`")))
}

/// Pins a manifest: each row's `wav` becomes the path from the run
/// directory and gains the `sha256` of the file. Returns the row count.
pub fn pin(input: &Path, out: &Path) -> Result<usize, EvalError> {
    let rows = read_rows(input)?;
    let origin = input.display().to_string();
    let directory = input.parent().filter(|p| !p.as_os_str().is_empty());
    let mut pinned = Vec::with_capacity(rows.len());
    for (index, mut row) in rows.into_iter().enumerate() {
        let name = str_field(&row, "wav", &origin, index)?.to_owned();
        let wav_path: PathBuf = match directory {
            Some(dir) => dir.join(&name),
            None => PathBuf::from(&name),
        };
        let digest = sha256_file(&wav_path)?;
        row.insert(String::from("wav"), Value::String(slash_path(&wav_path)));
        row.insert(String::from("sha256"), Value::String(digest));
        pinned.push(row);
    }
    write_rows(out, &pinned)?;
    Ok(pinned.len())
}

/// Where the clips of one part of a joined recording come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartKind {
    /// A `golden.json`: an object with a `clips` array, wavs beside it.
    Golden,
    /// A pinned manifest.
    Manifest,
}

/// One `--part kind:path:lang` argument.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Part {
    pub kind: PartKind,
    pub path: PathBuf,
    pub lang: String,
}

/// Reads `kind:path:lang`. The path may itself contain colons (a drive
/// letter): the kind is up to the first colon and the language after the
/// last.
pub fn parse_part(spec: &str) -> Result<Part, EvalError> {
    let bad = || EvalError::Invalid(format!("expected kind:path:lang, found {spec:?}"));
    let (kind_text, rest) = spec.split_once(':').ok_or_else(bad)?;
    let (path_text, lang) = rest.rsplit_once(':').ok_or_else(bad)?;
    let kind = match kind_text {
        "golden" => PartKind::Golden,
        "manifest" => PartKind::Manifest,
        _ => {
            return Err(EvalError::Invalid(format!(
                "the kind of {spec:?} is neither golden nor manifest"
            )));
        }
    };
    if path_text.is_empty() || lang.is_empty() {
        return Err(bad());
    }
    Ok(Part {
        kind,
        path: PathBuf::from(path_text),
        lang: lang.to_owned(),
    })
}

/// A clip chosen for a joined recording.
struct Clip {
    wav: PathBuf,
    row: Row,
}

fn golden_clips(part: &Part) -> Result<Vec<Clip>, EvalError> {
    let text = std::fs::read_to_string(&part.path).map_err(|e| EvalError::io(&part.path, e))?;
    let value: Value = serde_json::from_str(&text).map_err(|e| EvalError::json(&part.path, e))?;
    let origin = part.path.display().to_string();
    let Some(Value::Array(clips)) = value.get("clips") else {
        return Err(EvalError::Manifest(format!(
            "{origin} has no `clips` array"
        )));
    };
    let directory = part.path.parent().filter(|p| !p.as_os_str().is_empty());
    let mut chosen = Vec::with_capacity(clips.len());
    for (index, clip) in clips.iter().enumerate() {
        let Value::Object(fields) = clip else {
            return Err(EvalError::Manifest(format!(
                "{origin}: clip {index} is not an object"
            )));
        };
        let name = str_field(fields, "wav", &origin, index)?;
        let wav = match directory {
            Some(dir) => dir.join(name),
            None => PathBuf::from(name),
        };
        let mut row = Row::new();
        row.insert(
            String::from("ref"),
            Value::String(str_field(fields, "reference", &origin, index)?.to_owned()),
        );
        row.insert(
            String::from("sha256"),
            Value::String(str_field(fields, "sha256", &origin, index)?.to_owned()),
        );
        if let Some(id) = fields.get("fleurs_id") {
            row.insert(String::from("fleurs_id"), id.clone());
        }
        chosen.push(Clip { wav, row });
    }
    Ok(chosen)
}

fn manifest_clips(part: &Part) -> Result<Vec<Clip>, EvalError> {
    let origin = part.path.display().to_string();
    let mut chosen = Vec::new();
    for (index, row) in read_rows(&part.path)?.into_iter().enumerate() {
        if str_field(&row, "lang", &origin, index)? != part.lang {
            continue;
        }
        let wav = PathBuf::from(str_field(&row, "wav", &origin, index)?);
        str_field(&row, "ref", &origin, index)?;
        str_field(&row, "sha256", &origin, index)?;
        chosen.push(Clip { wav, row });
    }
    Ok(chosen)
}

/// What a join produced.
#[derive(Debug, PartialEq, Eq)]
pub struct Joined {
    pub utterances: usize,
    pub samples: usize,
}

/// Joins the clips of `parts` into one wav with `gap_ms` of silence between
/// them, and writes a manifest whose rows give each clip's first sample and
/// the sample just past the end of its speech (see [`speech_end`]). No
/// silence follows the last clip.
pub fn concat(
    parts: &[Part],
    gap_ms: u32,
    out_manifest: &Path,
    out_wav: &Path,
) -> Result<Joined, EvalError> {
    let gap = gap_ms as usize * SAMPLES_PER_MS;
    let mut clips = Vec::new();
    for part in parts {
        let found = match part.kind {
            PartKind::Golden => golden_clips(part)?,
            PartKind::Manifest => manifest_clips(part)?,
        };
        for mut clip in found {
            clip.row
                .insert(String::from("lang"), Value::String(part.lang.clone()));
            clip.row.insert(
                String::from("source"),
                Value::String(String::from(match part.kind {
                    PartKind::Golden => "golden",
                    PartKind::Manifest => "set",
                })),
            );
            clips.push(clip);
        }
    }
    if clips.is_empty() {
        return Err(EvalError::Manifest(String::from("the parts hold no clips")));
    }
    let mut samples: Vec<i16> = Vec::new();
    let mut rows = Vec::with_capacity(clips.len());
    for mut clip in clips {
        let digest = sha256_file(&clip.wav)?;
        if clip.row.get("sha256").and_then(Value::as_str) != Some(digest.as_str()) {
            return Err(EvalError::Manifest(format!(
                "{} does not match its recorded sha256",
                clip.wav.display()
            )));
        }
        let wav = read_wav(&clip.wav)?;
        if wav.sample_rate != SAMPLE_RATE {
            return Err(EvalError::Wav {
                path: clip.wav.display().to_string(),
                message: format!("sample rate {} is not {SAMPLE_RATE}", wav.sample_rate),
            });
        }
        if !samples.is_empty() {
            samples.resize(samples.len() + gap, 0);
        }
        let start = samples.len();
        samples.extend_from_slice(&wav.samples);
        clip.row
            .insert(String::from("wav"), Value::String(slash_path(&clip.wav)));
        clip.row
            .insert(String::from("start_sample"), Value::from(start));
        let Some(speech) = speech_end(&wav.samples) else {
            return Err(EvalError::Manifest(format!(
                "{} has no audio frames",
                clip.wav.display()
            )));
        };
        clip.row
            .insert(String::from("end_sample"), Value::from(start + speech));
        rows.push(clip.row);
    }
    if let Some(parent) = out_wav.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| EvalError::io(parent, e))?;
    }
    write_wav(out_wav, SAMPLE_RATE, &samples)?;
    write_rows(out_manifest, &rows)?;
    Ok(Joined {
        utterances: rows.len(),
        samples: samples.len(),
    })
}

/// The SHA-256 of a manifest's bytes, for the run record.
pub fn manifest_digest(path: &Path) -> Result<String, EvalError> {
    let bytes = std::fs::read(path).map_err(|e| EvalError::io(path, e))?;
    Ok(sha256_hex(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::scratch;

    fn write(path: &Path, text: &str) -> Result<(), EvalError> {
        write_text(path, text)
    }

    fn make_wav(path: &Path, samples: &[i16]) -> Result<(), EvalError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| EvalError::io(parent, e))?;
        }
        write_wav(path, SAMPLE_RATE, samples)
    }

    #[test]
    fn pinning_adds_the_sha256_and_the_run_relative_path() -> Result<(), EvalError> {
        let dir = scratch("pin-basic");
        make_wav(&dir.join("set/a.wav"), &[1, 2, 3])?;
        write(
            &dir.join("set/manifest.json"),
            r#"[{"wav":"a.wav","lang":"ja","ref":"あ"}]"#,
        )?;
        let out = dir.join("out/set.json");
        assert_eq!(pin(&dir.join("set/manifest.json"), &out)?, 1);
        let rows = read_rows(&out)?;
        assert_eq!(rows[0]["lang"], "ja");
        assert_eq!(
            rows[0]["sha256"],
            Value::String(sha256_file(&dir.join("set/a.wav"))?)
        );
        assert!(
            rows[0]["wav"]
                .as_str()
                .is_some_and(|w| w.ends_with("set/a.wav") && !w.contains('\\'))
        );
        Ok(())
    }

    #[test]
    fn pinning_twice_gives_the_same_bytes() -> Result<(), EvalError> {
        let dir = scratch("pin-twice");
        make_wav(&dir.join("s/a.wav"), &[1])?;
        write(&dir.join("s/m.json"), r#"[{"wav":"a.wav","z":1,"a":2}]"#)?;
        pin(&dir.join("s/m.json"), &dir.join("one.json"))?;
        pin(&dir.join("s/m.json"), &dir.join("two.json"))?;
        let first = std::fs::read(dir.join("one.json")).map_err(|e| EvalError::io(&dir, e))?;
        let second = std::fs::read(dir.join("two.json")).map_err(|e| EvalError::io(&dir, e))?;
        assert_eq!(first, second);
        Ok(())
    }

    #[test]
    fn a_missing_wav_is_named() -> Result<(), EvalError> {
        let dir = scratch("pin-missing");
        make_wav(&dir.join("s/a.wav"), &[1])?;
        write(
            &dir.join("s/m.json"),
            r#"[{"wav":"a.wav"},{"wav":"b.wav"}]"#,
        )?;
        let said = pin(&dir.join("s/m.json"), &dir.join("o.json"))
            .err()
            .map(|e| e.to_string());
        assert!(
            said.as_deref().is_some_and(|t| t.contains("b.wav")),
            "{said:?}"
        );
        Ok(())
    }

    #[test]
    fn a_row_without_a_wav_is_named() -> Result<(), EvalError> {
        let dir = scratch("pin-nowav");
        write(&dir.join("m.json"), r#"[{"lang":"ja"}]"#)?;
        let said = pin(&dir.join("m.json"), &dir.join("o.json"))
            .err()
            .map(|e| e.to_string());
        assert!(
            said.as_deref().is_some_and(|t| t.contains("row 0")),
            "{said:?}"
        );
        Ok(())
    }

    #[test]
    fn a_manifest_that_is_not_an_array_is_refused() -> Result<(), EvalError> {
        let dir = scratch("pin-object");
        write(&dir.join("m.json"), r#"{"wav":"a.wav"}"#)?;
        assert!(matches!(
            read_rows(&dir.join("m.json")),
            Err(EvalError::Manifest(_))
        ));
        Ok(())
    }

    #[test]
    fn a_part_is_read_with_a_drive_letter_in_its_path() -> Result<(), EvalError> {
        let part = parse_part(r"golden:C:\repo\tests\golden\ja\golden.json:ja")?;
        assert_eq!(part.kind, PartKind::Golden);
        assert_eq!(part.lang, "ja");
        assert_eq!(
            part.path,
            PathBuf::from(r"C:\repo\tests\golden\ja\golden.json")
        );
        Ok(())
    }

    #[test]
    fn a_part_with_an_unknown_kind_is_refused() {
        assert!(matches!(
            parse_part("other:x.json:ja"),
            Err(EvalError::Invalid(_))
        ));
        assert!(matches!(
            parse_part("manifest:x.json"),
            Err(EvalError::Invalid(_))
        ));
        assert!(matches!(
            parse_part("manifest::ja"),
            Err(EvalError::Invalid(_))
        ));
    }

    fn golden_json(dir: &Path, clips: &[(&str, &str)]) -> Result<PathBuf, EvalError> {
        let mut entries = Vec::new();
        for (name, reference) in clips {
            let digest = sha256_file(&dir.join(name))?;
            entries.push(serde_json::json!({
                "wav": name, "sha256": digest, "reference": reference, "fleurs_id": 7
            }));
        }
        let path = dir.join("golden.json");
        write(&path, &serde_json::json!({ "clips": entries }).to_string())?;
        Ok(path)
    }

    #[test]
    fn a_join_gives_each_clip_its_span_and_no_trailing_silence() -> Result<(), EvalError> {
        let dir = scratch("join-spans");
        make_wav(&dir.join("g/a.wav"), &[1; 100])?;
        make_wav(&dir.join("g/b.wav"), &[2; 50])?;
        let golden = golden_json(&dir.join("g"), &[("a.wav", "あ"), ("b.wav", "い")])?;
        let part = Part {
            kind: PartKind::Golden,
            path: golden,
            lang: String::from("ja"),
        };
        let joined = concat(&[part], 10, &dir.join("m.json"), &dir.join("lat.wav"))?;
        assert_eq!(
            joined,
            Joined {
                utterances: 2,
                samples: 100 + 160 + 50
            }
        );
        let rows = read_rows(&dir.join("m.json"))?;
        assert_eq!(
            (
                rows[0]["start_sample"].as_u64(),
                rows[0]["end_sample"].as_u64()
            ),
            (Some(0), Some(100))
        );
        assert_eq!(
            (
                rows[1]["start_sample"].as_u64(),
                rows[1]["end_sample"].as_u64()
            ),
            (Some(260), Some(310))
        );
        assert_eq!(rows[1]["fleurs_id"], 7);
        assert_eq!(read_wav(&dir.join("lat.wav"))?.samples.len(), 310);
        Ok(())
    }

    #[test]
    fn a_join_keeps_only_the_clips_of_the_asked_language() -> Result<(), EvalError> {
        let dir = scratch("join-lang");
        make_wav(&dir.join("s/a.wav"), &[1; 10])?;
        make_wav(&dir.join("s/b.wav"), &[2; 10])?;
        write(
            &dir.join("s/m.json"),
            r#"[{"wav":"a.wav","lang":"ja","ref":"あ"},{"wav":"b.wav","lang":"en","ref":"b"}]"#,
        )?;
        pin(&dir.join("s/m.json"), &dir.join("pinned.json"))?;
        let part = Part {
            kind: PartKind::Manifest,
            path: dir.join("pinned.json"),
            lang: String::from("en"),
        };
        let joined = concat(&[part], 0, &dir.join("m.json"), &dir.join("lat.wav"))?;
        assert_eq!(joined.utterances, 1);
        Ok(())
    }

    #[test]
    fn a_clip_that_no_longer_matches_its_sha256_is_refused() -> Result<(), EvalError> {
        let dir = scratch("join-changed");
        make_wav(&dir.join("g/a.wav"), &[1; 10])?;
        let golden = golden_json(&dir.join("g"), &[("a.wav", "あ")])?;
        make_wav(&dir.join("g/a.wav"), &[9; 10])?;
        let part = Part {
            kind: PartKind::Golden,
            path: golden,
            lang: String::from("ja"),
        };
        let said = concat(&[part], 0, &dir.join("m.json"), &dir.join("lat.wav"))
            .err()
            .map(|e| e.to_string());
        assert!(
            said.as_deref().is_some_and(|t| t.contains("sha256")),
            "{said:?}"
        );
        Ok(())
    }

    #[test]
    fn parts_with_no_clips_are_refused() -> Result<(), EvalError> {
        let dir = scratch("join-none");
        write(&dir.join("m.json"), "[]")?;
        let part = Part {
            kind: PartKind::Manifest,
            path: dir.join("m.json"),
            lang: String::from("ja"),
        };
        assert!(matches!(
            concat(&[part], 0, &dir.join("o.json"), &dir.join("o.wav")),
            Err(EvalError::Manifest(_))
        ));
        Ok(())
    }

    #[test]
    fn the_digest_of_a_manifest_is_the_digest_of_its_bytes() -> Result<(), EvalError> {
        let dir = scratch("digest");
        write(&dir.join("m.json"), "abc")?;
        assert_eq!(
            manifest_digest(&dir.join("m.json"))?,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        Ok(())
    }

    /// Alternating samples of the given amplitude.
    fn tone(len: usize, amplitude: i16) -> Vec<i16> {
        (0..len)
            .map(|i| if i % 2 == 0 { amplitude } else { -amplitude })
            .collect()
    }

    #[test]
    fn speech_ends_where_the_loud_part_ends_not_where_the_noise_ends() {
        let mut clip = tone(16_000, 8_000);
        clip.extend(tone(32_000, 30));
        assert_eq!(speech_end(&clip), Some(16_000));
    }

    #[test]
    fn digital_silence_in_the_clip_does_not_turn_the_room_noise_into_speech() {
        let mut clip = vec![0; 16_000];
        clip.extend(tone(16_000, 8_000));
        clip.extend(tone(32_000, 30));
        assert_eq!(speech_end(&clip), Some(32_000));
    }

    #[test]
    fn a_click_in_the_noise_after_the_voice_is_not_speech() {
        let mut clip = tone(16_000, 8_000);
        clip.extend(tone(6_400, 30));
        clip.extend(tone(320, 8_000));
        clip.extend(tone(6_400, 30));
        assert_eq!(speech_end(&clip), Some(16_000));
    }

    #[test]
    fn a_steady_clip_is_speech_to_its_last_sample() {
        assert_eq!(speech_end(&tone(1_000, 5_000)), Some(1_000));
    }

    #[test]
    fn a_clip_without_frames_has_no_speech_end() {
        assert_eq!(speech_end(&[]), None);
    }

    #[test]
    fn slash_paths_use_forward_slashes() {
        assert_eq!(slash_path(Path::new(r"a\b\c.wav")), "a/b/c.wav");
    }
}
