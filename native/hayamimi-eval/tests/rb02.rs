//! RB-2: the evaluation sets, the two joined latency recordings and their
//! manifests agree with each other and with the wav files on disk.

mod support;

use std::path::{Path, PathBuf};

use serde_json::Value;
use support::{
    FIXTURES, Failure, GOLDEN, RESULTS, TestResult, base, read, read_bytes, read_json, section,
    sha256_hex, testdata,
};

const SETS: [&str; 6] = [
    "eval_real",
    "eval_real_zhko",
    "eval_real_yue",
    "eval_diar",
    "lat-ja",
    "lat-en",
];
const SOURCE_SETS: [&str; 4] = ["eval_real", "eval_real_zhko", "eval_real_yue", "eval_diar"];
const LAT_SETS: [&str; 2] = ["lat-ja", "lat-en"];

/// Row keys that give the position of a row in its source.
const SOURCE_ROW_KEYS: [&str; 3] = ["fleurs_id", "row", "source_row"];

fn manifest_path(set: &str) -> Result<PathBuf, Failure> {
    Ok(testdata()?.join("manifests").join(format!("{set}.json")))
}

fn rows_of(text: &str) -> Result<Vec<Value>, Failure> {
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Array(rows)) => Ok(rows),
        Ok(_) => Err(String::from("a manifest is not a list of rows")),
        Err(error) => Err(format!("a manifest is not JSON: {error}")),
    }
}

fn manifest_rows(set: &str) -> Result<Vec<Value>, Failure> {
    rows_of(&read(&manifest_path(set)?)?)
}

fn text_of<'a>(row: &'a Value, key: &str) -> Result<&'a str, Failure> {
    match row.get(key).and_then(Value::as_str) {
        Some(text) => Ok(text),
        None => Err(format!("a row has no text {key}")),
    }
}

/// Every row's wav exists under `root`; the error names the first that does not.
fn check_wavs_present(rows: &[Value], root: &Path) -> Result<(), Failure> {
    for row in rows {
        let wav = text_of(row, "wav")?;
        if !root.join(wav).is_file() {
            return Err(format!("{wav} is missing"));
        }
    }
    Ok(())
}

/// Every row's sha256 is that of its wav; the error names the wav.
fn check_row_sha256(rows: &[Value], root: &Path) -> Result<(), Failure> {
    for row in rows {
        let wav = text_of(row, "wav")?;
        let got = sha256_hex(&read_bytes(&root.join(wav))?);
        let want = text_of(row, "sha256")?;
        if got != want {
            return Err(format!("{wav}: sha256 is {got}, the manifest says {want}"));
        }
    }
    Ok(())
}

fn check_row_count(rows: &[Value], want: usize) -> Result<(), Failure> {
    if rows.len() == want {
        Ok(())
    } else {
        Err(format!("{} utterances, want {want}", rows.len()))
    }
}

fn check_lat_ja(rows: &[Value]) -> Result<(), Failure> {
    check_row_count(rows, 23)
}

/// The end positions grow strictly and the last fits in `total` samples.
fn check_end_positions(ends: &[i64], total: i64) -> Result<(), Failure> {
    let Some(last) = ends.last() else {
        return Err(String::from("no end positions"));
    };
    if ends.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(String::from("end positions do not strictly increase"));
    }
    if *last > total {
        return Err(format!("the last end position {last} is past {total}"));
    }
    Ok(())
}

fn ends_of(rows: &[Value]) -> Result<Vec<i64>, Failure> {
    rows.iter()
        .map(|row| match row.get("end_sample").and_then(Value::as_i64) {
            Some(end) => Ok(end),
            None => Err(String::from("a row has no end_sample")),
        })
        .collect()
}

/// The sample count of a wav file.
fn total_samples(path: &Path) -> Result<i64, Failure> {
    let reader = hound::WavReader::open(path)
        .map_err(|error| format!("cannot open {}: {error}", path.display()))?;
    Ok(i64::from(reader.duration()))
}

/// The golden clips among the lat-ja rows carry the recorded sha256.
fn check_golden_part(lat_rows: &[Value], golden: &Value) -> Result<(), Failure> {
    let Some(clips) = golden.get("clips").and_then(Value::as_array) else {
        return Err(String::from("golden.json has no clips"));
    };
    let mut seen = 0;
    for row in lat_rows
        .iter()
        .filter(|row| row.get("source").and_then(Value::as_str) == Some("golden"))
    {
        let name = Path::new(text_of(row, "wav")?)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned());
        let recorded = clips
            .iter()
            .find(|clip| clip.get("wav").and_then(Value::as_str) == name.as_deref())
            .and_then(|clip| clip.get("sha256"))
            .and_then(Value::as_str);
        if recorded != Some(text_of(row, "sha256")?) {
            return Err(format!("{name:?} differs from golden.json"));
        }
        seen += 1;
    }
    if seen == clips.len() {
        Ok(())
    } else {
        Err(format!(
            "{seen} golden rows, golden.json has {}",
            clips.len()
        ))
    }
}

fn lacks_source_row(rows: &[Value]) -> bool {
    rows.iter()
        .any(|row| !SOURCE_ROW_KEYS.iter().any(|key| row.get(key).is_some()))
}

/// A set with a row that has no source row number is named in the summary.
fn check_provenance_recorded(set: &str, rows: &[Value], summary: &str) -> Result<(), Failure> {
    let named = summary
        .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-'))
        .any(|word| word == set);
    if lacks_source_row(rows) && !named {
        return Err(format!(
            "{set} has rows without a source row number and the summary does not name it"
        ));
    }
    Ok(())
}

fn summary_rb2() -> Result<String, Failure> {
    let summary = read(&Path::new(RESULTS).join("baseline_python.md"))?;
    Ok(section(&summary, "RB-2")?.to_owned())
}

#[test]
fn rb02_eval_sets_match_their_manifests_six_manifests_exist() -> TestResult {
    for set in SETS {
        assert!(manifest_path(set)?.is_file(), "{set}");
    }
    Ok(())
}

#[test]
fn rb02_eval_sets_match_their_manifests_row_count_equals_wav_count_in_each_set() -> TestResult {
    for set in SOURCE_SETS {
        let wavs = std::fs::read_dir(testdata()?.join(set))?
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "wav"))
            .count();
        let rows = manifest_rows(set)?.len();
        assert!(rows >= 1, "{set} has no rows");
        assert_eq!(rows, wavs, "{set}");
    }
    Ok(())
}

#[test]
fn rb02_eval_sets_match_their_manifests_lat_sets_have_23_and_15_utterances() -> TestResult {
    check_lat_ja(&manifest_rows("lat-ja")?)?;
    check_row_count(&manifest_rows("lat-en")?, 15)?;
    Ok(())
}

#[test]
fn rb02_eval_sets_match_their_manifests_lat_ja_with_22_utterances_is_rejected() -> TestResult {
    let rows = rows_of(include_str!("fixtures/rb02_lat_ja_22_rows.json"))?;
    assert!(check_lat_ja(&rows).is_err());
    Ok(())
}

#[test]
fn rb02_eval_sets_match_their_manifests_lat_ja_with_24_utterances_is_rejected() -> TestResult {
    let rows = rows_of(include_str!("fixtures/rb02_lat_ja_24_rows.json"))?;
    assert!(check_lat_ja(&rows).is_err());
    Ok(())
}

#[test]
fn rb02_eval_sets_match_their_manifests_golden_part_of_lat_ja_has_the_recorded_sha256() -> TestResult
{
    let golden = read_json(&Path::new(GOLDEN).join("golden.json"))?;
    check_golden_part(&manifest_rows("lat-ja")?, &golden)?;
    Ok(())
}

#[test]
fn rb02_eval_sets_match_their_manifests_sha256_of_abc_matches_the_standard_vector() {
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn rb02_eval_sets_match_their_manifests_every_row_sha256_matches_its_wav() -> TestResult {
    for set in SETS {
        let rows = manifest_rows(set)?;
        assert!(!rows.is_empty(), "{set} has no rows");
        // The rows name their wav files from the checkout that holds testdata/.
        let data = testdata()?;
        let Some(root) = data.parent() else {
            return Err(format!("{} has no parent", data.display()).into());
        };
        check_row_sha256(&rows, root)?;
    }
    Ok(())
}

#[test]
fn rb02_eval_sets_match_their_manifests_lat_end_positions_increase_and_fit_in_the_wav() -> TestResult
{
    for set in LAT_SETS {
        let total = total_samples(&testdata()?.join(format!("{set}.wav")))?;
        check_end_positions(&ends_of(&manifest_rows(set)?)?, total)?;
    }
    Ok(())
}

fn ends_fixture(text: &str) -> Result<(Vec<i64>, i64), Failure> {
    let value: Value = serde_json::from_str(text).map_err(|e| format!("not JSON: {e}"))?;
    let ends = value
        .get("ends")
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(Value::as_i64).collect::<Vec<_>>());
    match (ends, value.get("total").and_then(Value::as_i64)) {
        (Some(ends), Some(total)) => Ok((ends, total)),
        _ => Err(String::from("the fixture has no ends or total")),
    }
}

#[test]
fn rb02_eval_sets_match_their_manifests_end_position_equal_to_total_samples_is_accepted()
-> TestResult {
    let (ends, total) = ends_fixture(include_str!("fixtures/rb02_end_equal_total.json"))?;
    check_end_positions(&ends, total)?;
    Ok(())
}

#[test]
fn rb02_eval_sets_match_their_manifests_end_position_one_past_total_samples_is_rejected()
-> TestResult {
    let (ends, total) = ends_fixture(include_str!("fixtures/rb02_end_past_total.json"))?;
    assert!(check_end_positions(&ends, total).is_err());
    Ok(())
}

#[test]
fn rb02_eval_sets_match_their_manifests_equal_consecutive_end_positions_are_rejected() -> TestResult
{
    let (ends, total) = ends_fixture(include_str!("fixtures/rb02_end_equal_neighbours.json"))?;
    assert!(check_end_positions(&ends, total).is_err());
    Ok(())
}

#[test]
fn rb02_eval_sets_match_their_manifests_second_pin_run_is_byte_identical() -> TestResult {
    for set in SOURCE_SETS {
        let second = read_bytes(&base()?.join("pin_second").join(format!("{set}.json")))?;
        let first = read_bytes(&manifest_path(set)?)?;
        assert!(second == first, "{set}: the second --pin run differs");
    }
    Ok(())
}

#[test]
fn rb02_eval_sets_match_their_manifests_missing_wav_is_named_in_the_error() -> TestResult {
    let rows = rows_of(include_str!("fixtures/rb02_missing_wav/manifest.json"))?;
    let root = Path::new(FIXTURES).join("rb02_missing_wav");
    let said = check_wavs_present(&rows, &root);
    assert!(matches!(&said, Err(text) if text.contains("b.wav") && !text.contains("a.wav")));
    Ok(())
}

#[test]
fn rb02_eval_sets_match_their_manifests_wrong_sha256_row_is_named() -> TestResult {
    let root = Path::new(FIXTURES);
    check_row_sha256(
        &rows_of(include_str!("fixtures/rb02_right_sha.json"))?,
        root,
    )?;
    let wrong = rows_of(include_str!("fixtures/rb02_wrong_sha.json"))?;
    let said = check_row_sha256(&wrong, root);
    assert!(matches!(&said, Err(text) if text.contains("rb02_missing_wav/a.wav")));
    Ok(())
}

#[test]
fn rb02_eval_sets_match_their_manifests_sets_without_source_row_are_named_in_baseline_summary()
-> TestResult {
    let summary = summary_rb2()?;
    let mut lacking = 0;
    for set in SETS {
        let rows = manifest_rows(set)?;
        lacking += usize::from(lacks_source_row(&rows));
        check_provenance_recorded(set, &rows, &summary)?;
    }
    if lacking == 0 {
        assert!(summary.contains("出所の行の番号が無い組：なし"));
    }
    Ok(())
}

#[test]
fn rb02_eval_sets_match_their_manifests_set_missing_from_baseline_summary_is_named() -> TestResult {
    let rows = rows_of(include_str!("fixtures/rb02_set_without_source_row.json"))?;
    let summary = include_str!("fixtures/rb02_summary_without_set_name.md");
    let said = check_provenance_recorded("eval_real", &rows, summary);
    assert!(matches!(&said, Err(text) if text.contains("eval_real")));
    Ok(())
}

#[test]
fn rb02_eval_sets_match_their_manifests_missing_manifest_file_names_its_path() {
    let absent = Path::new(FIXTURES).join("absent").join("lat-ja.json");
    assert!(matches!(read(&absent), Err(path) if path.ends_with("lat-ja.json")));
}
