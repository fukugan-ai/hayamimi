//! The recording behind the two-speaker rows of the diarization tests: a
//! stretch of an AMI meeting in which two people speak and one of them speaks
//! once.
//!
//! [`pick_window`] reads the RTTM files of a directory in the order of their
//! names and returns the first window that fits the rule below.
//! [`write_clip`] cuts that window out of the meeting's wav and writes it with
//! a manifest.
//!
//! A window is four consecutive turns of one meeting, the turns ordered by
//! their start. It fits when
//!
//! - exactly two speakers take part, and one of them has one turn only, that
//!   turn lasting 2.000 to 4.000 seconds (both ends included);
//! - no two of the four turns share a millisecond (turns that only touch do
//!   not share one, whoever speaks them);
//! - no turn outside the window reaches into it.
//!
//! The window starts where its first turn starts and ends where the last of
//! its turns ends. Turns of no length are dropped before windows are looked
//! for. Every comparison is made on whole milliseconds: an RTTM has three
//! decimals, and a sum of two such seconds as `f64` can land a millisecond's
//! fraction on the wrong side of a boundary. Nothing bounds the length of a
//! window or the silence between its turns.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::der::parse_rttm;
use crate::error::EvalError;
use crate::sha::sha256_file;
use crate::wav::{SAMPLE_RATE, Wav, read_wav, write_wav};

/// Turns in a window.
const WINDOW_TURNS: usize = 4;
/// The shortest turn the lone speaker may have, in milliseconds.
const LONE_TURN_MIN_MS: i64 = 2_000;
/// The longest turn the lone speaker may have, in milliseconds.
const LONE_TURN_MAX_MS: i64 = 4_000;
/// Samples in a millisecond of the recording.
const SAMPLES_PER_MS: i64 = (SAMPLE_RATE / 1000) as i64;

/// The wav that [`write_clip`] writes.
pub const CLIP_FILE: &str = "two_speakers.wav";
/// The manifest that [`write_clip`] writes beside the wav.
pub const MANIFEST_FILE: &str = "two_speakers.json";
/// Manifest key: the meeting the window was cut from.
pub const KEY_MEETING: &str = "meeting";
/// Manifest key: where the window starts, in seconds.
pub const KEY_START_S: &str = "start_s";
/// Manifest key: where the window ends, in seconds.
pub const KEY_END_S: &str = "end_s";
/// Manifest key: the SHA-256 of the written wav, as lowercase hex. A record only; nothing compares it.
pub const KEY_SHA256: &str = "sha256";

/// Manifest key: the licence of the audio and of the reference turns.
pub const KEY_LICENSE: &str = "license";
/// Manifest key: where the licence text is.
pub const KEY_LICENSE_URL: &str = "license_url";
/// The attribution every manifest carries, as (key, value): the same facts as
/// the README of `testdata/eval_diar`. The window's seconds are on the meeting's
/// wav, which starts `start_offset_s` into the recording (see `time_base`).
const ATTRIBUTION: [(&str, &str); 5] = [
    (KEY_LICENSE, "AMI Meeting Corpus: CC BY 4.0"),
    (
        KEY_LICENSE_URL,
        "https://groups.inf.ed.ac.uk/ami/corpus/license.shtml",
    ),
    (
        "citation",
        "Carletta et al., The AMI Meeting Corpus: A Pre-Announcement, MLMI 2005",
    ),
    (
        "rttm_source",
        "https://github.com/BUTSpeechFIT/AMI-diarization-setup (MIT)",
    ),
    (
        "time_base",
        "seconds on the meeting's wav in eval_diar; that wav starts start_offset_s (eval_diar/manifest.json) into the recording",
    ),
];

/// What the error says when no meeting holds a window.
const NO_WINDOW: &str = "no window of four turns fits the rule";
/// What the error says when the directory has no RTTM file.
const NO_RTTM: &str = "no .rttm file";
/// What the error says when the window runs past the end of the audio.
const PAST_END: &str = "ends past the audio";

/// The part of a meeting that [`write_clip`] cuts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    /// The name of the RTTM file without its extension, which is also the name of the wav.
    pub meeting: String,
    pub start_ms: i64,
    pub end_ms: i64,
}

/// One speech turn on whole milliseconds.
struct MsTurn {
    speaker: String,
    start: i64,
    end: i64,
}

/// The milliseconds `[a_start, a_end)` and `[b_start, b_end)` share; 0 when they only touch.
fn shared_ms(a_start: i64, a_end: i64, b_start: i64, b_end: i64) -> i64 {
    (a_end.min(b_end) - a_start.max(b_start)).max(0)
}

fn to_ms(seconds: f64) -> i64 {
    (seconds * 1000.0).round() as i64
}

fn seconds(ms: i64) -> f64 {
    ms as f64 / 1000.0
}

/// The RTTM files of `dir`, in the order of their names without the extension.
fn rttm_files(dir: &Path) -> Result<Vec<PathBuf>, EvalError> {
    let entries = std::fs::read_dir(dir).map_err(|error| EvalError::io(dir, error))?;
    let mut files = Vec::new();
    for entry in entries {
        let path = entry.map_err(|error| EvalError::io(dir, error))?.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "rttm")
        {
            files.push(path);
        }
    }
    files.sort_by(|a, b| a.file_stem().cmp(&b.file_stem()));
    Ok(files)
}

fn meeting_name(path: &Path) -> Result<String, EvalError> {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::to_owned)
        .ok_or_else(|| {
            EvalError::Invalid(format!("{}: the file name is not UTF-8", path.display()))
        })
}

/// The turns of one RTTM file that last at least a millisecond, ordered by start.
fn read_turns(path: &Path) -> Result<Vec<MsTurn>, EvalError> {
    let text = std::fs::read_to_string(path).map_err(|error| EvalError::io(path, error))?;
    let mut turns: Vec<MsTurn> = parse_rttm(&text, &path.display().to_string())?
        .into_iter()
        .map(|turn| MsTurn {
            start: to_ms(turn.start),
            end: to_ms(turn.end),
            speaker: turn.speaker,
        })
        .filter(|turn| turn.end > turn.start)
        .collect();
    turns.sort_by_key(|turn| (turn.start, turn.end));
    Ok(turns)
}

/// The turn of the speaker who has one turn in `window`, when exactly two
/// speakers take part and the other has the remaining three.
fn lone_speaker_turn(window: &[MsTurn]) -> Option<&MsTurn> {
    let first = window.first()?;
    let second = window.iter().find(|turn| turn.speaker != first.speaker)?;
    if window
        .iter()
        .any(|turn| turn.speaker != first.speaker && turn.speaker != second.speaker)
    {
        return None;
    }
    match window
        .iter()
        .filter(|turn| turn.speaker == first.speaker)
        .count()
    {
        1 => Some(first),
        3 => Some(second),
        _ => None,
    }
}

/// Whether two turns of `window` share a millisecond.
fn has_overlap(window: &[MsTurn]) -> bool {
    window.iter().enumerate().any(|(index, a)| {
        window
            .iter()
            .skip(index + 1)
            .any(|b| shared_ms(a.start, a.end, b.start, b.end) > 0)
    })
}

/// The window of the four turns that begin at `at`, if they fit the rule.
fn window_at(turns: &[MsTurn], at: usize) -> Option<(i64, i64)> {
    let window = turns.get(at..at + WINDOW_TURNS)?;
    let lone = lone_speaker_turn(window)?;
    if !(LONE_TURN_MIN_MS..=LONE_TURN_MAX_MS).contains(&(lone.end - lone.start))
        || has_overlap(window)
    {
        return None;
    }
    let start = window.first()?.start;
    let end = window.iter().map(|turn| turn.end).max()?;
    let reached_into = turns
        .iter()
        .enumerate()
        .filter(|(index, _)| !(at..at + WINDOW_TURNS).contains(index))
        .any(|(_, turn)| shared_ms(turn.start, turn.end, start, end) > 0);
    (!reached_into).then_some((start, end))
}

/// The earliest window of one meeting's turns.
fn first_window(turns: &[MsTurn]) -> Option<(i64, i64)> {
    let last = turns.len().checked_sub(WINDOW_TURNS)?;
    (0..=last).find_map(|at| window_at(turns, at))
}

/// The first window that fits the rule, looking at the meetings under `rttm_dir`
/// in the order of their names. `Err` when the directory holds no RTTM file, an
/// RTTM line is malformed, or no meeting holds a window.
pub fn pick_window(rttm_dir: &Path) -> Result<Window, EvalError> {
    let files = rttm_files(rttm_dir)?;
    if files.is_empty() {
        return Err(EvalError::Invalid(format!(
            "{}: {NO_RTTM}",
            rttm_dir.display()
        )));
    }
    for path in &files {
        if let Some((start_ms, end_ms)) = first_window(&read_turns(path)?) {
            return Ok(Window {
                meeting: meeting_name(path)?,
                start_ms,
                end_ms,
            });
        }
    }
    Err(EvalError::Invalid(format!(
        "{NO_WINDOW} in the {} meeting(s) under {}",
        files.len(),
        rttm_dir.display()
    )))
}

/// The index of the sample at `ms`.
fn sample_index(ms: i64) -> Option<usize> {
    usize::try_from(ms * SAMPLES_PER_MS).ok()
}

/// The samples of `window` in `wav`, which was read from `source`.
fn cut_samples<'a>(wav: &'a Wav, window: &Window, source: &Path) -> Result<&'a [i16], EvalError> {
    let named = |message: String| EvalError::Wav {
        path: source.display().to_string(),
        message,
    };
    if wav.sample_rate != SAMPLE_RATE {
        return Err(named(format!(
            "expected {SAMPLE_RATE} Hz, found {} Hz",
            wav.sample_rate
        )));
    }
    let range = sample_index(window.start_ms).zip(sample_index(window.end_ms));
    let clip = range.and_then(|(start, end)| wav.samples.get(start..end));
    clip.ok_or_else(|| {
        named(format!(
            "the window {}..{} ms {PAST_END} ({} samples)",
            window.start_ms,
            window.end_ms,
            wav.samples.len()
        ))
    })
}

fn write_manifest(window: &Window, sha256: &str, path: &Path) -> Result<(), EvalError> {
    let mut row = Map::new();
    row.insert(KEY_MEETING.to_owned(), Value::from(window.meeting.as_str()));
    row.insert(
        KEY_START_S.to_owned(),
        Value::from(seconds(window.start_ms)),
    );
    row.insert(KEY_END_S.to_owned(), Value::from(seconds(window.end_ms)));
    row.insert(KEY_SHA256.to_owned(), Value::from(sha256));
    for (key, value) in ATTRIBUTION {
        row.insert(key.to_owned(), Value::from(value));
    }
    let mut text =
        serde_json::to_string_pretty(&row).map_err(|error| EvalError::json(path, error))?;
    text.push('\n');
    std::fs::write(path, text).map_err(|error| EvalError::io(path, error))
}

/// Cuts `window` out of `<source_dir>/<meeting>.wav` into `out_dir`: the wav
/// as [`CLIP_FILE`] and the manifest as [`MANIFEST_FILE`]. The same input gives
/// the same bytes. Nothing is written when the window runs past the audio.
pub fn write_clip(window: &Window, source_dir: &Path, out_dir: &Path) -> Result<(), EvalError> {
    let source = source_dir.join(format!("{}.wav", window.meeting));
    let wav = read_wav(&source)?;
    let clip = cut_samples(&wav, window, &source)?;
    std::fs::create_dir_all(out_dir).map_err(|error| EvalError::io(out_dir, error))?;
    let clip_path = out_dir.join(CLIP_FILE);
    write_wav(&clip_path, SAMPLE_RATE, clip)?;
    write_manifest(
        window,
        &sha256_file(&clip_path)?,
        &out_dir.join(MANIFEST_FILE),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sha::sha256_hex;
    use crate::testutil::scratch;

    type TestResult = Result<(), Box<dyn std::error::Error>>;
    /// Speaker, start and length in seconds, as an RTTM line writes them.
    type Row = (&'static str, &'static str, &'static str);

    const MA: &str = "mA";
    const F60V: [Row; 4] = [
        ("A", "10.000", "5.000"),
        ("B", "16.000", "3.000"),
        ("A", "20.000", "4.000"),
        ("A", "25.000", "2.500"),
    ];
    const F60LO: [Row; 4] = [
        ("B", "0.010", "2.000"),
        ("A", "3.000", "1.000"),
        ("A", "5.000", "1.000"),
        ("A", "7.000", "1.000"),
    ];
    const F60HI: [Row; 4] = [
        ("B", "4.001", "4.000"),
        ("A", "9.000", "1.000"),
        ("A", "11.000", "1.000"),
        ("A", "13.000", "1.000"),
    ];
    const F60TO: [Row; 4] = [
        ("B", "0.131", "3.000"),
        ("A", "3.131", "1.000"),
        ("A", "6.000", "1.000"),
        ("A", "8.000", "1.000"),
    ];
    const F60OV: [Row; 4] = [
        ("A", "10.000", "5.000"),
        ("B", "14.000", "3.000"),
        ("A", "20.000", "4.000"),
        ("A", "25.000", "2.500"),
    ];
    /// F60V moved 90 seconds later.
    const F60V_LATER: [Row; 4] = [
        ("A", "100.000", "5.000"),
        ("B", "106.000", "3.000"),
        ("A", "110.000", "4.000"),
        ("A", "115.000", "2.500"),
    ];

    /// A scratch directory that is removed when it goes out of scope.
    struct Dir(PathBuf);

    impl Dir {
        fn new(name: &str) -> Result<Dir, std::io::Error> {
            let path = scratch(name);
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path)?;
            Ok(Dir(path))
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// RTTM text with one SPEAKER line for each row, in the order given.
    fn rttm(meeting: &str, rows: &[Row]) -> String {
        rows.iter()
            .map(|(speaker, start, length)| {
                format!("SPEAKER {meeting} 1 {start} {length} <NA> <NA> {speaker} <NA> <NA>\n")
            })
            .collect()
    }

    /// A directory with the given RTTM files, written in the order given.
    fn meetings(name: &str, files: &[(&str, String)]) -> Result<Dir, std::io::Error> {
        let dir = Dir::new(name)?;
        for (meeting, text) in files {
            std::fs::write(dir.path().join(format!("{meeting}.rttm")), text)?;
        }
        Ok(dir)
    }

    fn one_meeting(name: &str, rows: &[Row]) -> Result<Dir, std::io::Error> {
        meetings(name, &[(MA, rttm(MA, rows))])
    }

    fn window(meeting: &str, start_ms: i64, end_ms: i64) -> Window {
        Window {
            meeting: meeting.to_owned(),
            start_ms,
            end_ms,
        }
    }

    /// `rows` as the only meeting must give `expected`.
    fn picks(name: &str, rows: &[Row], expected: Window) -> TestResult {
        let dir = one_meeting(name, rows)?;
        assert_eq!(pick_window(dir.path())?, expected);
        Ok(())
    }

    /// `rows` as the only meeting must give an `Err` that says no window fits.
    fn finds_none(name: &str, rows: &[Row]) -> TestResult {
        let dir = one_meeting(name, rows)?;
        let said = pick_window(dir.path()).err().map(|error| error.to_string());
        assert!(
            said.as_deref().is_some_and(|text| text.contains(NO_WINDOW)),
            "{said:?}"
        );
        Ok(())
    }

    /// `rows` with the length of the row at `at` replaced.
    fn with_length(rows: [Row; 4], at: usize, length: &'static str) -> [Row; 4] {
        let mut changed = rows;
        changed[at].2 = length;
        changed
    }

    /// `rows` with the start of the row at `at` replaced.
    fn with_start(rows: [Row; 4], at: usize, start: &'static str) -> [Row; 4] {
        let mut changed = rows;
        changed[at].1 = start;
        changed
    }

    #[test]
    fn two_speaker_clip_picks_the_window_of_a_hand_placed_rttm() -> TestResult {
        picks("t001", &F60V, window(MA, 10_000, 27_500))
    }

    #[test]
    fn two_speaker_clip_accepts_a_minority_turn_of_exactly_2_seconds() -> TestResult {
        picks("t002", &F60LO, window(MA, 10, 8_000))
    }

    #[test]
    fn two_speaker_clip_rejects_a_minority_turn_of_1_999_seconds() -> TestResult {
        finds_none("t003", &with_length(F60LO, 0, "1.999"))
    }

    #[test]
    fn two_speaker_clip_accepts_a_minority_turn_of_exactly_4_seconds() -> TestResult {
        picks("t004", &F60HI, window(MA, 4_001, 14_000))
    }

    #[test]
    fn two_speaker_clip_rejects_a_minority_turn_of_4_001_seconds() -> TestResult {
        finds_none("t005", &with_length(F60HI, 0, "4.001"))
    }

    #[test]
    fn two_speaker_clip_rejects_three_speakers() -> TestResult {
        let rows = [
            ("A", "10.000", "5.000"),
            ("B", "16.000", "3.000"),
            ("C", "20.000", "4.000"),
            ("A", "25.000", "2.500"),
        ];
        finds_none("t006", &rows)
    }

    #[test]
    fn two_speaker_clip_rejects_one_speaker() -> TestResult {
        let rows = [
            ("A", "10.000", "5.000"),
            ("A", "16.000", "3.000"),
            ("A", "20.000", "4.000"),
            ("A", "25.000", "2.500"),
        ];
        finds_none("t007", &rows)
    }

    #[test]
    fn two_speaker_clip_rejects_two_turns_for_each_speaker() -> TestResult {
        let rows = [
            ("A", "10.000", "5.000"),
            ("B", "16.000", "3.000"),
            ("A", "20.000", "4.000"),
            ("B", "25.000", "3.000"),
        ];
        finds_none("t008", &rows)
    }

    #[test]
    fn two_speaker_clip_accepts_the_minority_turn_at_the_end() -> TestResult {
        let rows = [
            ("A", "10.000", "5.000"),
            ("A", "16.000", "3.000"),
            ("A", "20.000", "4.000"),
            ("B", "25.000", "3.000"),
        ];
        picks("t009", &rows, window(MA, 10_000, 28_000))
    }

    #[test]
    fn two_speaker_clip_rejects_three_turns() -> TestResult {
        finds_none("t010", &F60V[..3])
    }

    #[test]
    fn two_speaker_clip_accepts_turns_that_touch_with_no_overlap() -> TestResult {
        picks("t011", &F60TO, window(MA, 131, 9_000))
    }

    #[test]
    fn two_speaker_clip_rejects_an_overlap_of_1_ms() -> TestResult {
        finds_none("t012", &with_start(F60TO, 1, "3.130"))
    }

    #[test]
    fn two_speaker_clip_names_the_missing_window_when_only_overlapping_windows_exist() -> TestResult
    {
        finds_none("t013", &F60OV)
    }

    #[test]
    fn two_speaker_clip_reads_the_meetings_in_name_order() -> TestResult {
        let dir = meetings(
            "t014",
            &[("mB", rttm("mB", &F60V)), (MA, rttm(MA, &F60V_LATER))],
        )?;
        assert_eq!(pick_window(dir.path())?, window(MA, 100_000, 117_500));
        Ok(())
    }

    #[test]
    fn two_speaker_clip_skips_a_meeting_with_no_window() -> TestResult {
        let dir = meetings("t015", &[(MA, rttm(MA, &F60OV)), ("mB", rttm("mB", &F60V))])?;
        assert_eq!(pick_window(dir.path())?, window("mB", 10_000, 27_500));
        Ok(())
    }

    #[test]
    fn two_speaker_clip_takes_the_earliest_window_of_a_meeting() -> TestResult {
        let later_first: Vec<Row> = F60V_LATER
            .iter()
            .chain(F60V.iter().rev())
            .copied()
            .collect();
        picks("t016", &later_first, window(MA, 10_000, 27_500))
    }

    #[test]
    fn two_speaker_clip_rejects_a_directory_with_no_rttm() -> TestResult {
        let dir = Dir::new("t017")?;
        let said = pick_window(dir.path()).err().map(|error| error.to_string());
        let named = dir.path().display().to_string();
        assert!(
            said.as_deref().is_some_and(|text| text.contains(&named)
                && text.contains(NO_RTTM)
                && !text.contains(NO_WINDOW)),
            "{said:?}"
        );
        Ok(())
    }

    #[test]
    fn two_speaker_clip_rejects_an_rttm_with_no_turns() -> TestResult {
        let dir = meetings("t018", &[(MA, String::new())])?;
        let said = pick_window(dir.path()).err().map(|error| error.to_string());
        assert!(
            said.as_deref().is_some_and(|text| text.contains(NO_WINDOW)),
            "{said:?}"
        );
        Ok(())
    }

    #[test]
    fn two_speaker_clip_reports_a_malformed_rttm_line_not_a_missing_window() -> TestResult {
        let text = format!("{}SPEAKER mA 1 12.0 1.0\n", rttm(MA, &F60V[..1]));
        let dir = meetings("t019", &[(MA, text)])?;
        let said = pick_window(dir.path()).err().map(|error| error.to_string());
        assert!(
            said.as_deref()
                .is_some_and(|text| text.contains("mA.rttm:2") && !text.contains(NO_WINDOW)),
            "{said:?}"
        );
        Ok(())
    }

    /// 30 seconds at 16 kHz in which sample `i` is the low 16 bits of `i`, read as signed.
    fn s60() -> Vec<i16> {
        (0..480_000_u32)
            .map(|i| (i & 0xFFFF) as u16 as i16)
            .collect()
    }

    /// A directory holding `mA.wav` made of `samples`.
    fn source_with(name: &str, samples: &[i16]) -> Result<Dir, Box<dyn std::error::Error>> {
        let dir = Dir::new(name)?;
        write_wav(&dir.path().join("mA.wav"), SAMPLE_RATE, samples)?;
        Ok(dir)
    }

    /// Cuts `window` out of `samples` and returns the directory it was written to.
    fn cut(
        name: &str,
        samples: &[i16],
        window: &Window,
    ) -> Result<Dir, Box<dyn std::error::Error>> {
        let source = source_with(&format!("{name}-src"), samples)?;
        let out = Dir::new(name)?;
        write_clip(window, source.path(), out.path())?;
        Ok(out)
    }

    fn clip_of(out: &Dir) -> Result<Wav, EvalError> {
        read_wav(&out.path().join(CLIP_FILE))
    }

    fn manifest_of(out: &Dir) -> Result<Value, Box<dyn std::error::Error>> {
        let text = std::fs::read_to_string(out.path().join(MANIFEST_FILE))?;
        Ok(serde_json::from_str(&text)?)
    }

    fn manifest_text(manifest: &Value, key: &str) -> Result<String, String> {
        manifest
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| format!("the manifest has no string {key}"))
    }

    /// A manifest's seconds, read as whole milliseconds.
    fn manifest_ms(manifest: &Value, key: &str) -> Result<i64, String> {
        manifest
            .get(key)
            .and_then(Value::as_f64)
            .map(to_ms)
            .ok_or_else(|| format!("the manifest has no number {key}"))
    }

    #[test]
    fn two_speaker_clip_writes_the_samples_of_the_window() -> TestResult {
        let out = cut("t020", &s60(), &window(MA, 10_000, 27_500))?;
        let clip = clip_of(&out)?;
        assert_eq!(clip.sample_rate, 16_000);
        assert_eq!(clip.samples.len(), 280_000);
        assert_eq!(clip.samples[..2], [28_928, 28_929]);
        assert_eq!(clip.samples[279_999], -18_753);
        Ok(())
    }

    #[test]
    fn two_speaker_clip_accepts_a_window_ending_at_the_end_of_the_audio() -> TestResult {
        let out = cut("t021", &s60(), &window(MA, 25_000, 30_000))?;
        let clip = clip_of(&out)?;
        assert_eq!(clip.samples.len(), 80_000);
        assert_eq!(clip.samples[0], 6_784);
        assert_eq!(clip.samples[79_999], 21_247);
        Ok(())
    }

    #[test]
    fn two_speaker_clip_rejects_a_window_ending_past_the_audio() -> TestResult {
        let source = source_with("t022-src", &s60())?;
        let out = Dir::new("t022")?;
        let said = write_clip(&window(MA, 25_000, 30_001), source.path(), out.path())
            .err()
            .map(|error| error.to_string());
        assert!(
            said.as_deref()
                .is_some_and(|text| text.contains("mA.wav") && text.contains(PAST_END)),
            "{said:?}"
        );
        assert!(!out.path().join(CLIP_FILE).exists());
        Ok(())
    }

    #[test]
    fn two_speaker_clip_manifest_names_the_meeting_and_the_window() -> TestResult {
        let out = cut("t023", &s60(), &window(MA, 10_000, 27_500))?;
        let manifest = manifest_of(&out)?;
        assert_eq!(manifest_text(&manifest, KEY_MEETING)?, "mA");
        assert_eq!(manifest_ms(&manifest, KEY_START_S)?, 10_000);
        assert_eq!(manifest_ms(&manifest, KEY_END_S)?, 27_500);
        Ok(())
    }

    #[test]
    fn two_speaker_clip_manifest_sha256_is_the_digest_of_the_written_wav() -> TestResult {
        let out = cut("t024", &s60(), &window(MA, 10_000, 27_500))?;
        let recorded = manifest_text(&manifest_of(&out)?, KEY_SHA256)?;
        assert_eq!(recorded.len(), 64);
        assert!(
            recorded
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        );
        assert_eq!(
            recorded,
            sha256_hex(&std::fs::read(out.path().join(CLIP_FILE))?)
        );
        Ok(())
    }

    #[test]
    fn two_speaker_clip_sha256_changes_with_a_sample_inside_the_window_only() -> TestResult {
        let cut_window = window(MA, 10_000, 27_500);
        let base = manifest_text(
            &manifest_of(&cut("t025-base", &s60(), &cut_window)?)?,
            KEY_SHA256,
        )?;
        // The window holds samples 160000 up to, not including, 440000.
        for (index, inside) in [
            (159_999, false),
            (160_000, true),
            (439_999, true),
            (440_000, false),
        ] {
            let mut samples = s60();
            samples[index] = samples[index].wrapping_add(1);
            let name = format!("t025-{index}");
            let changed = manifest_text(
                &manifest_of(&cut(&name, &samples, &cut_window)?)?,
                KEY_SHA256,
            )?;
            assert_eq!(changed != base, inside, "sample {index}");
        }
        Ok(())
    }

    #[test]
    fn two_speaker_clip_is_byte_identical_when_cut_twice() -> TestResult {
        let cut_window = window(MA, 10_000, 27_500);
        let first = cut("t026-a", &s60(), &cut_window)?;
        let second = cut("t026-b", &s60(), &cut_window)?;
        for file in [CLIP_FILE, MANIFEST_FILE] {
            let (a, b) = (
                std::fs::read(first.path().join(file))?,
                std::fs::read(second.path().join(file))?,
            );
            assert_eq!(a, b, "{file}");
        }
        Ok(())
    }

    /// The directory `testdata/eval_diar` at or above this crate: untracked data is not in a worktree, so it is read from the main checkout.
    fn eval_diar_dir() -> Result<PathBuf, String> {
        let start = env!("CARGO_MANIFEST_DIR");
        Path::new(start)
            .ancestors()
            .map(|directory| directory.join("testdata").join("eval_diar"))
            .find(|data| data.is_dir())
            .ok_or_else(|| format!("no testdata/eval_diar at or above {start}"))
    }

    #[test]
    #[ignore = "reads testdata/eval_diar, which is not in git"]
    fn two_speaker_clip_is_cut_by_the_rule_from_ami() -> TestResult {
        let data = eval_diar_dir()?;
        let found = pick_window(&data)?;
        assert_eq!(found, window("IS1008a", 457_220, 491_880));
        let out = Dir::new("t027")?;
        write_clip(&found, &data, out.path())?;
        let clip = clip_of(&out)?;
        assert_eq!(clip.samples.len(), 554_560);
        assert_eq!(clip.samples[..4], [-542, -572, -621, -683]);
        assert_eq!(clip.samples[clip.samples.len() - 2..], [265, 284]);
        let manifest = manifest_of(&out)?;
        assert_eq!(manifest_text(&manifest, KEY_MEETING)?, "IS1008a");
        assert_eq!(manifest_ms(&manifest, KEY_START_S)?, 457_220);
        assert_eq!(manifest_ms(&manifest, KEY_END_S)?, 491_880);
        let digest = sha256_hex(&std::fs::read(out.path().join(CLIP_FILE))?);
        assert_eq!(manifest_text(&manifest, KEY_SHA256)?, digest);
        println!(
            "{}",
            std::fs::read_to_string(out.path().join(MANIFEST_FILE))?
        );
        Ok(())
    }

    #[test]
    fn two_speaker_clip_rejects_a_minority_turn_of_1_9_seconds() -> TestResult {
        finds_none("t028", &with_length(F60LO, 0, "1.900"))
    }

    #[test]
    fn two_speaker_clip_rejects_a_minority_turn_of_4_1_seconds() -> TestResult {
        finds_none("t029", &with_length(F60HI, 0, "4.100"))
    }

    #[test]
    fn two_speaker_clip_accepts_same_speaker_turns_that_touch() -> TestResult {
        let rows = [
            ("A", "0.131", "3.000"),
            ("A", "3.131", "1.000"),
            ("B", "5.000", "3.000"),
            ("A", "9.000", "1.000"),
        ];
        picks("t030", &rows, window(MA, 131, 10_000))
    }

    #[test]
    fn two_speaker_clip_rejects_a_same_speaker_overlap_of_1_ms() -> TestResult {
        let rows = [
            ("A", "0.131", "3.000"),
            ("A", "3.130", "1.000"),
            ("B", "5.000", "3.000"),
            ("A", "9.000", "1.000"),
        ];
        finds_none("t031", &rows)
    }

    #[test]
    fn two_speaker_clip_drops_a_zero_length_turn_before_choosing() -> TestResult {
        let mut rows = F60V.to_vec();
        rows.push(("C", "21.000", "0.000"));
        picks("t032", &rows, window(MA, 10_000, 27_500))
    }

    #[test]
    fn two_speaker_clip_does_not_count_a_zero_length_turn_as_the_fourth() -> TestResult {
        finds_none("t033", &with_length(F60V, 3, "0.000"))
    }

    #[test]
    fn two_speaker_clip_rejects_a_window_an_outside_turn_reaches_into() -> TestResult {
        let mut rows = F60V.to_vec();
        rows.push(("A", "9.000", "2.000"));
        finds_none("t034", &rows)
    }

    #[test]
    fn two_speaker_clip_accepts_a_window_an_outside_turn_only_touches() -> TestResult {
        let mut rows = F60V.to_vec();
        rows.push(("C", "27.500", "1.000"));
        picks("t035", &rows, window(MA, 10_000, 27_500))
    }
}
