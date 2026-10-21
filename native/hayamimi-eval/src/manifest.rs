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

/// The percentile of the frame levels taken as the quiet level.
const QUIET_PERCENT: usize = 10;

/// The percentile of the frame levels taken as the loud level.
const LOUD_PERCENT: usize = 95;

/// The heights of the speech line, as fractions of the way from the quiet to
/// the loud level. The first is the rule of `speech_end`; the lower ones show
/// how far the end moves when the line is lowered.
pub const LINE_FRACTIONS: [f32; 4] = [SPEECH_FRACTION, 0.25, 0.15, 0.10];

/// The end of the voice is stable when no lower line moves it by more than
/// this many samples: 40 ms.
const STABLE_MOVE: usize = 40 * SAMPLES_PER_MS;

/// How many frames after the voice are averaged to tell how quiet it is: 300 ms.
const TAIL_FRAMES: usize = 15;

/// How far below the rule's line the mean level after the voice must lie, in
/// dB. Both are counted side by side, so a verdict can be shown not to depend
/// on which one is used.
pub const LOW_LINES_DB: [f32; 2] = [6.0, 10.0];

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
    let levels = Levels::of(samples)?;
    Some((levels.end_frame(SPEECH_FRACTION)? * END_FRAME).min(samples.len()))
}

/// The frame levels of a clip and the quiet and loud levels they span.
struct Levels {
    frames: Vec<f32>,
    quiet: f32,
    loud: f32,
}

impl Levels {
    /// `None` for a clip without frames.
    fn of(samples: &[i16]) -> Option<Self> {
        let frames: Vec<f32> = samples.chunks(END_FRAME).map(frame_level_db).collect();
        let mut sorted = frames.clone();
        sorted.sort_by(f32::total_cmp);
        let last = sorted.len().checked_sub(1)?;
        let at = |percent: usize| sorted.get(last * percent / 100).copied();
        let (quiet, loud) = (at(QUIET_PERCENT)?, at(LOUD_PERCENT)?);
        Some(Self {
            frames,
            quiet,
            loud,
        })
    }

    /// The level a frame must reach to count as speech when the line stands
    /// `fraction` of the way from the quiet to the loud level.
    fn line(&self, fraction: f32) -> f32 {
        (self.quiet + fraction * (self.loud - self.quiet)).max(self.loud - SPEECH_RANGE_DB)
    }

    /// The index of the frame just past the last run of `SPEECH_RUN` frames
    /// that all reach the line at `fraction`.
    fn end_frame(&self, fraction: f32) -> Option<usize> {
        let line = self.line(fraction);
        let run = SPEECH_RUN.min(self.frames.len());
        let first = self
            .frames
            .windows(run)
            .rposition(|window| window.iter().all(|level| *level >= line))?;
        Some(first + run)
    }

    /// How far below the rule's line the mean level of the `TAIL_FRAMES`
    /// frames from `end_frame` on lies, in dB. The mean is of the dB values,
    /// not of the power. `None` when no frame follows.
    fn tail_low_db(&self, end_frame: usize) -> Option<f32> {
        let after = self.frames.get(end_frame..)?;
        let tail = &after[..after.len().min(TAIL_FRAMES)];
        if tail.is_empty() {
            return None;
        }
        let sum: f64 = tail.iter().map(|level| f64::from(*level)).sum();
        let mean = sum / tail.len() as f64;
        Some((f64::from(self.line(SPEECH_FRACTION)) - mean) as f32)
    }
}

/// How certain it is that the voice of a clip ends where `speech_end` says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndGroup {
    /// Lowering the line barely moves the end, and what follows is quiet.
    Certain,
    /// The voice runs to the last frame, so the clip is cut inside it.
    Cut,
    /// Neither: a tail of the voice and the background do not part by level.
    Uncertain,
}

/// Where the voice of a clip ends at each height of the line, and how quiet
/// the clip is after the rule's end.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EndReading {
    /// The end in samples at each of `LINE_FRACTIONS`, cut at the clip's
    /// length. The first is `speech_end`.
    pub ends: [usize; LINE_FRACTIONS.len()],
    /// How far below the rule's line the mean level after the first end lies,
    /// in dB; `None` when the clip ends with the voice.
    pub tail_low_db: Option<f32>,
    cut: bool,
}

impl EndReading {
    /// The group of the clip when the level after the voice must lie
    /// `low_line_db` below the rule's line to be quiet.
    pub fn group(&self, low_line_db: f32) -> EndGroup {
        if self.cut {
            return EndGroup::Cut;
        }
        let stable = self
            .ends
            .iter()
            .all(|end| end.abs_diff(self.ends[0]) <= STABLE_MOVE);
        let quiet = self.tail_low_db.is_some_and(|low| low >= low_line_db);
        if stable && quiet {
            EndGroup::Certain
        } else {
            EndGroup::Uncertain
        }
    }
}

/// Reads the end of the voice of a clip at every height of the line. `None`
/// when `speech_end` finds no voice, so the clip has no group.
pub fn read_end(samples: &[i16]) -> Option<EndReading> {
    let levels = Levels::of(samples)?;
    let mut end_frames = [0_usize; LINE_FRACTIONS.len()];
    for (slot, fraction) in end_frames.iter_mut().zip(LINE_FRACTIONS) {
        *slot = levels.end_frame(fraction)?;
    }
    let ends = end_frames.map(|frame| (frame * END_FRAME).min(samples.len()));
    Some(EndReading {
        ends,
        tail_low_db: levels.tail_low_db(end_frames[0]),
        // The end lies inside the last 20 ms of the clip, whether or not the
        // clip's length is a whole number of frames.
        cut: ends[0] + END_FRAME > samples.len(),
    })
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

    // ---- the groups of a clip (RP-73) ----
    //
    // A clip is written as frames of 320 samples of one amplitude, so the
    // level of each frame is 10 * log10(amplitude^2 / 32768^2). Three rooms
    // are used: quiet 164 and voice 10362 (-46.01 and -10.00 dB), quiet 328 and
    // voice 6540 (-39.99 and -14.00 dB), and quiet 1000 with a voice chosen
    // per clip.

    /// `n` frames of 320 samples at `amplitude`.
    fn frames(n: usize, amplitude: i16) -> Vec<i16> {
        tone(n * END_FRAME, amplitude)
    }

    fn read(clip: &[i16]) -> Result<EndReading, String> {
        read_end(clip).ok_or_else(|| String::from("the clip has no run of speech"))
    }

    /// The groups of a clip at the 6 dB line and at the 10 dB line.
    fn groups_of(clip: &[i16]) -> Result<(EndGroup, EndGroup), String> {
        let reading = read(clip)?;
        Ok((
            reading.group(LOW_LINES_DB[0]),
            reading.group(LOW_LINES_DB[1]),
        ))
    }

    /// The low level after the voice is `expected` dB, within `tolerance`.
    fn assert_low(reading: &EndReading, expected: f32, tolerance: f32) -> Result<(), String> {
        let low = reading
            .tail_low_db
            .ok_or_else(|| String::from("no frame follows the voice"))?;
        if (low - expected).abs() <= tolerance {
            Ok(())
        } else {
            Err(format!("low {low} dB is not {expected} dB +- {tolerance}"))
        }
    }

    fn clip_001() -> Vec<i16> {
        [frames(20, 164), frames(30, 10362), frames(50, 164)].concat()
    }

    fn clip_002() -> Vec<i16> {
        [
            frames(20, 164),
            frames(30, 10362),
            frames(2, 639),
            frames(48, 164),
        ]
        .concat()
    }

    fn clip_003() -> Vec<i16> {
        [
            frames(20, 164),
            frames(30, 10362),
            frames(3, 639),
            frames(47, 164),
        ]
        .concat()
    }

    fn clip_004() -> Vec<i16> {
        [
            frames(20, 328),
            frames(30, 6540),
            frames(3, 474),
            frames(47, 328),
        ]
        .concat()
    }

    fn clip_005() -> Vec<i16> {
        [
            frames(20, 328),
            frames(30, 6540),
            frames(3, 583),
            frames(47, 328),
        ]
        .concat()
    }

    fn clip_006() -> Vec<i16> {
        [
            frames(20, 328),
            frames(30, 6540),
            frames(3, 823),
            frames(47, 328),
        ]
        .concat()
    }

    fn clip_007() -> Vec<i16> {
        [
            frames(20, 328),
            frames(30, 6540),
            frames(15, 328),
            frames(20, 596),
            frames(15, 328),
        ]
        .concat()
    }

    fn clip_008() -> Vec<i16> {
        [frames(20, 164), frames(40, 10362)].concat()
    }

    fn clip_009() -> Vec<i16> {
        [frames(20, 164), frames(39, 10362), frames(1, 164)].concat()
    }

    fn clip_010() -> Vec<i16> {
        [frames(20, 164), frames(39, 10362), tone(100, 10362)].concat()
    }

    fn clip_011() -> Vec<i16> {
        [frames(20, 164), frames(30, 10362), frames(5, 164)].concat()
    }

    fn clip_012() -> Vec<i16> {
        [frames(20, 1000), frames(30, 7173), frames(5, 1000)].concat()
    }

    fn clip_013() -> Vec<i16> {
        [frames(20, 1000), frames(30, 7197), frames(50, 1000)].concat()
    }

    fn clip_014() -> Vec<i16> {
        [frames(20, 1000), frames(30, 7173), frames(50, 1000)].concat()
    }

    fn clip_015() -> Vec<i16> {
        [frames(20, 1000), frames(30, 26828), frames(50, 1000)].concat()
    }

    fn clip_016() -> Vec<i16> {
        [frames(20, 1000), frames(30, 26738), frames(50, 1000)].concat()
    }

    /// The quiet frame and the isolated frame just under the rule's line,
    /// five times, then a quiet room: 10 quiet and 5 isolated frames follow
    /// the voice inside the window.
    fn clip_017() -> Vec<i16> {
        let mut parts = vec![frames(20, 164), frames(30, 10362)];
        for _ in 0..5 {
            parts.push(frames(1, 164));
            parts.push(frames(1, 661));
        }
        parts.push(frames(40, 164));
        parts.concat()
    }

    fn clip_018() -> Vec<i16> {
        [
            frames(20, 328),
            frames(30, 6540),
            frames(6, 583),
            frames(44, 328),
        ]
        .concat()
    }

    fn clip_019() -> Vec<i16> {
        [
            frames(20, 164),
            frames(30, 10362),
            frames(6, 519),
            frames(44, 164),
        ]
        .concat()
    }

    fn clip_020() -> Vec<i16> {
        [
            frames(20, 164),
            frames(6, 10362),
            frames(25, 519),
            frames(52, 164),
        ]
        .concat()
    }

    fn clip_021() -> Vec<i16> {
        [
            frames(10, 0),
            frames(14, 164),
            frames(30, 3277),
            frames(6, 328),
            frames(40, 164),
        ]
        .concat()
    }

    /// Voice frames that never make a run of five.
    fn clip_022_isolated() -> Vec<i16> {
        let mut parts = Vec::new();
        for _ in 0..4 {
            parts.push(frames(10, 164));
            parts.push(frames(1, 10362));
        }
        parts.push(frames(10, 164));
        parts.concat()
    }

    /// Every clip above that has a voice, for the properties of all of them.
    fn clips_with_a_voice() -> Vec<Vec<i16>> {
        vec![
            clip_001(),
            clip_002(),
            clip_003(),
            clip_004(),
            clip_005(),
            clip_006(),
            clip_007(),
            clip_008(),
            clip_009(),
            clip_010(),
            clip_011(),
            clip_012(),
            clip_013(),
            clip_014(),
            clip_015(),
            clip_016(),
            clip_017(),
            clip_018(),
            clip_019(),
            clip_020(),
            clip_021(),
        ]
    }

    const BOTH_CERTAIN: (EndGroup, EndGroup) = (EndGroup::Certain, EndGroup::Certain);
    const BOTH_UNCERTAIN: (EndGroup, EndGroup) = (EndGroup::Uncertain, EndGroup::Uncertain);
    const BOTH_CUT: (EndGroup, EndGroup) = (EndGroup::Cut, EndGroup::Cut);

    #[test]
    fn a_clip_whose_voice_stops_dead_is_certain_at_both_lines() -> Result<(), String> {
        let reading = read(&clip_001())?;
        assert_eq!(reading.ends, [16_000; 4]);
        assert_low(&reading, 12.60, 0.05)?;
        assert_eq!(groups_of(&clip_001())?, BOTH_CERTAIN);
        Ok(())
    }

    #[test]
    fn a_movement_of_exactly_40_ms_is_still_certain() -> Result<(), String> {
        let reading = read(&clip_002())?;
        assert_eq!(reading.ends, [16_000, 16_640, 16_640, 16_640]);
        assert_low(&reading, 11.03, 0.05)?;
        assert_eq!(groups_of(&clip_002())?, BOTH_CERTAIN);
        Ok(())
    }

    #[test]
    fn a_movement_of_60_ms_is_not_certain_though_the_tail_is_low() -> Result<(), String> {
        let reading = read(&clip_003())?;
        assert_eq!(reading.ends, [16_000, 16_960, 16_960, 16_960]);
        assert_low(&reading, 10.24, 0.05)?;
        assert_eq!(groups_of(&clip_003())?, BOTH_UNCERTAIN);
        Ok(())
    }

    #[test]
    fn only_the_lowest_line_moving_60_ms_makes_the_clip_uncertain() -> Result<(), String> {
        let reading = read(&clip_004())?;
        assert_eq!(reading.ends, [16_000, 16_000, 16_000, 16_960]);
        assert_low(&reading, 8.46, 0.05)?;
        assert_eq!(groups_of(&clip_004())?, BOTH_UNCERTAIN);
        Ok(())
    }

    #[test]
    fn the_two_lowest_lines_moving_60_ms_make_the_clip_uncertain() -> Result<(), String> {
        let reading = read(&clip_005())?;
        assert_eq!(reading.ends, [16_000, 16_000, 16_960, 16_960]);
        assert_low(&reading, 8.10, 0.05)?;
        assert_eq!(groups_of(&clip_005())?, BOTH_UNCERTAIN);
        Ok(())
    }

    #[test]
    fn the_three_lower_lines_moving_60_ms_make_the_clip_uncertain() -> Result<(), String> {
        let reading = read(&clip_006())?;
        assert_eq!(reading.ends, [16_000, 16_960, 16_960, 16_960]);
        assert_low(&reading, 7.50, 0.05)?;
        assert_eq!(groups_of(&clip_006())?, BOTH_UNCERTAIN);
        Ok(())
    }

    #[test]
    fn a_tail_beyond_the_window_makes_the_clip_uncertain() -> Result<(), String> {
        let reading = read(&clip_007())?;
        assert_eq!(reading.ends, [16_000, 16_000, 27_200, 27_200]);
        assert_low(&reading, 9.10, 0.05)?;
        assert_eq!(groups_of(&clip_007())?, BOTH_UNCERTAIN);
        Ok(())
    }

    #[test]
    fn a_voice_running_to_the_last_frame_is_cut() -> Result<(), String> {
        let reading = read(&clip_008())?;
        assert_eq!(reading.ends, [19_200; 4]);
        assert_eq!(reading.tail_low_db, None);
        assert_eq!(groups_of(&clip_008())?, BOTH_CUT);
        Ok(())
    }

    #[test]
    fn a_voice_ending_one_frame_before_the_end_is_not_cut() -> Result<(), String> {
        let reading = read(&clip_009())?;
        assert_eq!(reading.ends, [18_880; 4]);
        assert_low(&reading, 12.60, 0.05)?;
        assert_eq!(groups_of(&clip_009())?, BOTH_CERTAIN);
        Ok(())
    }

    #[test]
    fn a_last_frame_shorter_than_320_samples_ends_the_voice_at_the_clip_length()
    -> Result<(), String> {
        let reading = read(&clip_010())?;
        assert_eq!(reading.ends, [18_980; 4]);
        assert_eq!(groups_of(&clip_010())?, BOTH_CUT);
        Ok(())
    }

    #[test]
    fn a_voice_ending_where_a_short_last_frame_begins_is_cut() -> Result<(), String> {
        let clip = [frames(20, 164), frames(30, 10362), tone(185, 164)].concat();
        let reading = read(&clip)?;
        assert_eq!(reading.ends, [16_000; 4]);
        assert_eq!(groups_of(&clip)?, BOTH_CUT);
        Ok(())
    }

    #[test]
    fn a_window_shorter_than_300_ms_is_averaged_over_its_own_frames() -> Result<(), String> {
        let reading = read(&clip_011())?;
        assert_eq!(reading.ends, [16_000; 4]);
        assert_low(&reading, 12.60, 0.05)?;
        assert_eq!(groups_of(&clip_011())?, BOTH_CERTAIN);
        Ok(())
    }

    #[test]
    fn a_short_window_that_is_not_low_enough_is_not_padded_with_silence() -> Result<(), String> {
        let reading = read(&clip_012())?;
        assert_eq!(reading.ends, [16_000; 4]);
        assert_low(&reading, 5.98991, 0.001)?;
        assert_eq!(groups_of(&clip_012())?, BOTH_UNCERTAIN);
        Ok(())
    }

    #[test]
    fn a_tail_6_db_below_the_line_is_certain_at_the_6_db_line_only() -> Result<(), String> {
        let reading = read(&clip_013())?;
        assert_eq!(reading.ends, [16_000; 4]);
        assert_low(&reading, 6.00006, 0.001)?;
        assert_eq!(
            groups_of(&clip_013())?,
            (EndGroup::Certain, EndGroup::Uncertain)
        );
        Ok(())
    }

    #[test]
    fn a_tail_exactly_on_the_low_line_is_certain() {
        let on_the_line = EndReading {
            ends: [16_000; LINE_FRACTIONS.len()],
            tail_low_db: Some(LOW_LINES_DB[0]),
            cut: false,
        };
        assert_eq!(on_the_line.group(LOW_LINES_DB[0]), EndGroup::Certain);
        assert_eq!(on_the_line.group(LOW_LINES_DB[1]), EndGroup::Uncertain);
    }

    #[test]
    fn a_tail_0_01_db_short_of_6_db_is_not_certain() -> Result<(), String> {
        let reading = read(&clip_014())?;
        assert_low(&reading, 5.98991, 0.001)?;
        assert_eq!(groups_of(&clip_014())?, BOTH_UNCERTAIN);
        Ok(())
    }

    #[test]
    fn a_tail_10_db_below_the_line_is_certain_at_both_lines() -> Result<(), String> {
        let reading = read(&clip_015())?;
        assert_eq!(reading.ends, [16_000; 4]);
        assert_low(&reading, 10.00012, 0.001)?;
        assert_eq!(groups_of(&clip_015())?, BOTH_CERTAIN);
        Ok(())
    }

    #[test]
    fn a_tail_0_01_db_short_of_10_db_is_certain_only_at_the_6_db_line() -> Result<(), String> {
        let reading = read(&clip_016())?;
        assert_low(&reading, 9.98990, 0.001)?;
        assert_eq!(
            groups_of(&clip_016())?,
            (EndGroup::Certain, EndGroup::Uncertain)
        );
        Ok(())
    }

    #[test]
    fn the_tail_is_averaged_in_decibels_not_in_power() -> Result<(), String> {
        let reading = read(&clip_017())?;
        assert_eq!(reading.ends, [16_000; 4]);
        assert_low(&reading, 8.57, 0.05)?;
        assert_eq!(
            groups_of(&clip_017())?,
            (EndGroup::Certain, EndGroup::Uncertain)
        );
        Ok(())
    }

    #[test]
    fn the_percentile_line_decides_when_it_is_above_the_25_db_floor() -> Result<(), String> {
        let reading = read(&clip_018())?;
        assert_eq!(reading.ends, [16_000, 16_000, 17_920, 17_920]);
        assert_eq!(groups_of(&clip_018())?, BOTH_UNCERTAIN);
        Ok(())
    }

    #[test]
    fn the_25_db_floor_decides_when_the_percentile_line_is_lower() -> Result<(), String> {
        let reading = read(&clip_019())?;
        assert_eq!(reading.ends, [16_000; 4]);
        assert_low(&reading, 8.60, 0.05)?;
        assert_eq!(
            groups_of(&clip_019())?,
            (EndGroup::Certain, EndGroup::Uncertain)
        );
        Ok(())
    }

    #[test]
    fn the_loud_level_is_the_95th_percentile_by_integer_rank() -> Result<(), String> {
        assert_eq!(read(&clip_020())?.ends, [16_320; 4]);
        Ok(())
    }

    #[test]
    fn the_quiet_level_is_the_10th_percentile_by_integer_rank() -> Result<(), String> {
        assert_eq!(read(&clip_021())?.ends, [19_200; 4]);
        Ok(())
    }

    #[test]
    fn a_clip_without_a_speech_run_has_no_group() {
        assert_eq!(read_end(&[]), None);
        assert_eq!(read_end(&clip_022_isolated()), None);
    }

    #[test]
    fn the_ends_never_move_earlier_as_the_line_goes_down() -> Result<(), String> {
        for (index, clip) in clips_with_a_voice().iter().enumerate() {
            let reading = read(clip)?;
            assert!(
                reading.ends.windows(2).all(|pair| pair[0] <= pair[1]),
                "clip {index}: {:?}",
                reading.ends
            );
        }
        Ok(())
    }
}
