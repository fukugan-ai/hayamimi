//! IP-6: `fetch-set --concat --interleave` takes one clip from each `--part`
//! in turn, so a recording of two languages alternates between them.
//!
//! Every case runs the `hayamimi-eval` binary on pinned manifests that the test
//! builds under this crate's build tree: constant-level wav files of a few
//! hundred samples, so that the end of each clip's speech is the clip's end.
//! The one `#[ignore]` case reads `lat-dialog`, which the command of design
//! 10.7 makes under `testdata/`, and runs with `-- --ignored`.

mod support;

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};

use support::rp32::scratch_dir;
use support::{TestResult, sha256_hex, testdata};

type Outcome<T> = Result<T, Box<dyn Error>>;

/// Samples of a 10 ms gap at 16 kHz.
const GAP_10_MS: usize = 160;
/// The level of every sample of a fixture clip.
const LEVEL: i16 = 3000;
/// The gap of `lat-dialog`: 800 ms at 16 kHz.
const GAP_800_MS: u64 = 12_800;

/// One clip of a fixture: the file stem (also its `ref`), its language and its length.
struct Clip {
    stem: String,
    lang: &'static str,
    len: usize,
}

fn clip(stem: &str, lang: &'static str, len: usize) -> Clip {
    Clip {
        stem: stem.to_owned(),
        lang,
        len,
    }
}

/// Three Japanese clips, then three English ones: 450 samples in all.
fn six() -> Vec<Clip> {
    vec![
        clip("a1", "ja", 100),
        clip("a2", "ja", 60),
        clip("a3", "ja", 80),
        clip("b1", "en", 50),
        clip("b2", "en", 70),
        clip("b3", "en", 90),
    ]
}

fn without(clips: Vec<Clip>, stem: &str) -> Vec<Clip> {
    clips.into_iter().filter(|c| c.stem != stem).collect()
}

fn utf8(path: &Path) -> Outcome<&str> {
    path.to_str()
        .ok_or_else(|| Box::<dyn Error>::from(format!("{} is not UTF-8", path.display())))
}

fn write_constant_wav(path: &Path, samples: usize, level: i16) -> Outcome<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for _ in 0..samples {
        writer.write_sample(level)?;
    }
    writer.finalize()?;
    Ok(())
}

fn read_samples(path: &Path) -> Outcome<Vec<i16>> {
    let mut reader = hound::WavReader::open(path)?;
    Ok(reader.samples::<i16>().collect::<Result<Vec<_>, _>>()?)
}

fn tool(args: &[&str]) -> Outcome<Output> {
    Ok(Command::new(env!("CARGO_BIN_EXE_hayamimi-eval"))
        .args(args)
        .output()?)
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// A directory with the wav files of some clips and the manifest `fetch-set --pin` made of them.
struct Material {
    dir: PathBuf,
    pinned: PathBuf,
}

fn material(name: &str, clips: &[Clip]) -> Outcome<Material> {
    let dir = scratch_dir("ip06", name)?;
    let mut rows = Vec::with_capacity(clips.len());
    for c in clips {
        write_constant_wav(&dir.join(format!("{}.wav", c.stem)), c.len, LEVEL)?;
        rows.push(json!({ "wav": format!("{}.wav", c.stem), "lang": c.lang, "ref": c.stem }));
    }
    let raw = dir.join("raw.json");
    std::fs::write(&raw, serde_json::to_string(&rows)?)?;
    let pinned = dir.join("pinned.json");
    let pin = tool(&["fetch-set", "--pin", utf8(&raw)?, "--out", utf8(&pinned)?])?;
    if !pin.status.success() {
        return Err(format!("pinning the fixture failed: {}", stderr_of(&pin)).into());
    }
    Ok(Material { dir, pinned })
}

/// One `fetch-set` run that joins, and the two files it was asked to write.
struct Join {
    output: Output,
    manifest: PathBuf,
    wav: PathBuf,
}

impl Join {
    fn rows(&self) -> Outcome<Vec<Value>> {
        read_rows(&self.manifest)
    }

    fn samples(&self) -> Outcome<Vec<i16>> {
        read_samples(&self.wav)
    }

    fn succeeded(&self) -> bool {
        self.output.status.success()
    }

    fn wrote_nothing(&self) -> bool {
        !self.manifest.exists() && !self.wav.exists()
    }
}

fn read_rows(path: &Path) -> Outcome<Vec<Value>> {
    Ok(serde_json::from_slice::<Vec<Value>>(&std::fs::read(path)?)?)
}

/// `fetch-set <flags>` over one `--part` for each language of `parts`, in that order.
fn join(
    material: &Material,
    tag: &str,
    flags: &[&str],
    parts: &[&str],
    gap_ms: u32,
) -> Outcome<Join> {
    let manifest = material.dir.join(format!("{tag}.json"));
    let wav = material.dir.join(format!("{tag}.wav"));
    let pinned = utf8(&material.pinned)?;
    let specs: Vec<String> = parts
        .iter()
        .map(|lang| format!("manifest:{pinned}:{lang}"))
        .collect();
    let gap = gap_ms.to_string();
    let mut args = vec!["fetch-set"];
    args.extend_from_slice(flags);
    for spec in &specs {
        args.extend(["--part", spec]);
    }
    args.extend([
        "--gap-ms",
        &gap,
        "--out",
        utf8(&manifest)?,
        "--wav-out",
        utf8(&wav)?,
    ]);
    let output = tool(&args)?;
    Ok(Join {
        output,
        manifest,
        wav,
    })
}

fn interleaved(material: &Material, tag: &str, parts: &[&str], gap_ms: u32) -> Outcome<Join> {
    join(material, tag, &["--concat", "--interleave"], parts, gap_ms)
}

fn text<'a>(row: &'a Value, key: &str) -> Outcome<&'a str> {
    row[key]
        .as_str()
        .ok_or_else(|| Box::<dyn Error>::from(format!("a row has no string `{key}`")))
}

fn number(row: &Value, key: &str) -> Outcome<u64> {
    row[key]
        .as_u64()
        .ok_or_else(|| Box::<dyn Error>::from(format!("a row has no number `{key}`")))
}

fn strings<'a>(rows: &'a [Value], key: &str) -> Outcome<Vec<&'a str>> {
    rows.iter().map(|row| text(row, key)).collect()
}

fn numbers(rows: &[Value], key: &str) -> Outcome<Vec<u64>> {
    rows.iter().map(|row| number(row, key)).collect()
}

/// Names the first two neighbouring rows that have the same language.
fn check_alternates(langs: &[&str]) -> Result<(), String> {
    match langs.windows(2).position(|pair| pair[0] == pair[1]) {
        Some(at) => Err(format!("rows {at} and {} are both {}", at + 1, langs[at])),
        None => Ok(()),
    }
}

/// The join of `six()` with a 10 ms gap, which most cases read.
fn six_joined(name: &str) -> Outcome<Join> {
    let material = material(name, &six())?;
    let joined = interleaved(&material, "join", &["ja", "en"], 10)?;
    assert!(joined.succeeded(), "{}", stderr_of(&joined.output));
    Ok(joined)
}

#[test]
fn ip06_interleaved_join_alternates_languages() -> TestResult {
    let joined = six_joined("t001")?;
    let rows = joined.rows()?;
    let langs = strings(&rows, "lang")?;
    assert_eq!(langs, ["ja", "en", "ja", "en", "ja", "en"]);
    check_alternates(&langs)?;
    assert_eq!(strings(&rows, "ref")?, ["a1", "b1", "a2", "b2", "a3", "b3"]);
    assert_eq!(
        numbers(&rows, "start_sample")?,
        [0, 260, 470, 690, 920, 1160]
    );
    assert_eq!(
        numbers(&rows, "end_sample")?,
        [100, 310, 530, 760, 1000, 1250]
    );
    assert_eq!(joined.samples()?.len(), 1250);
    Ok(())
}

#[test]
fn an_interleaved_join_puts_exactly_the_gap_between_clips_and_none_at_the_ends() -> TestResult {
    let joined = six_joined("t002")?;
    let samples = joined.samples()?;
    let gaps = [(100, 260), (310, 470), (530, 690), (760, 920), (1000, 1160)];
    for (at, sample) in samples.iter().enumerate() {
        let inside_a_gap = gaps.iter().any(|(from, to)| (*from..*to).contains(&at));
        assert_eq!(*sample == 0, inside_a_gap, "sample {at}");
    }
    assert_ne!(samples[0], 0);
    assert_ne!(samples[samples.len() - 1], 0);
    let rows = joined.rows()?;
    let (starts, ends) = (
        numbers(&rows, "start_sample")?,
        numbers(&rows, "end_sample")?,
    );
    for at in 1..rows.len() {
        assert_eq!(
            starts[at] - ends[at - 1],
            160,
            "between rows {} and {at}",
            at - 1
        );
    }
    Ok(())
}

#[test]
fn a_join_with_three_ja_and_two_en_ends_with_ja() -> TestResult {
    let material = material("t003", &without(six(), "b3"))?;
    let joined = interleaved(&material, "join", &["ja", "en"], 10)?;
    assert!(joined.succeeded(), "{}", stderr_of(&joined.output));
    let rows = joined.rows()?;
    assert_eq!(strings(&rows, "lang")?, ["ja", "en", "ja", "en", "ja"]);
    assert_eq!(numbers(&rows, "start_sample")?, [0, 260, 470, 690, 920]);
    assert_eq!(joined.samples()?.len(), 1000);
    Ok(())
}

#[test]
fn a_join_with_two_ja_and_three_en_ends_with_two_en() -> TestResult {
    let material = material("t004", &without(six(), "a3"))?;
    let joined = interleaved(&material, "join", &["ja", "en"], 10)?;
    assert!(joined.succeeded(), "{}", stderr_of(&joined.output));
    let rows = joined.rows()?;
    assert_eq!(strings(&rows, "lang")?, ["ja", "en", "ja", "en", "en"]);
    assert_eq!(numbers(&rows, "start_sample")?, [0, 260, 470, 690, 920]);
    assert_eq!(numbers(&rows, "end_sample")?[4], 1010);
    assert_eq!(joined.samples()?.len(), 1010);
    Ok(())
}

#[test]
fn a_join_without_interleave_does_not_alternate() -> TestResult {
    let material = material("t005", &six())?;
    let blocks = join(&material, "blocks", &["--concat"], &["ja", "en"], 10)?;
    assert!(blocks.succeeded(), "{}", stderr_of(&blocks.output));
    let rows = blocks.rows()?;
    let langs = strings(&rows, "lang")?;
    assert_eq!(langs, ["ja", "ja", "ja", "en", "en", "en"]);
    let said = check_alternates(&langs).err();
    assert_eq!(said.as_deref(), Some("rows 0 and 1 are both ja"));
    let turns = interleaved(&material, "turns", &["ja", "en"], 10)?;
    assert!(check_alternates(&strings(&turns.rows()?, "lang")?).is_ok());
    Ok(())
}

/// What taking one clip from each language in turn gives, written down apart
/// from the code under test: the `ref` of each row and its language.
fn round_robin_reference(ja: usize, en: usize) -> Vec<(&'static str, String)> {
    let mut order = Vec::with_capacity(ja + en);
    for turn in 0..ja.max(en) {
        if turn < ja {
            order.push(("ja", format!("j{turn}")));
        }
        if turn < en {
            order.push(("en", format!("e{turn}")));
        }
    }
    order
}

/// `ja` Japanese and `en` English clips, every length different.
fn clips_of(ja: usize, en: usize) -> Vec<Clip> {
    let japanese = (0..ja).map(|i| clip(&format!("j{i}"), "ja", 100 + 10 * i));
    let english = (0..en).map(|i| clip(&format!("e{i}"), "en", 205 + 10 * i));
    japanese.chain(english).collect()
}

#[test]
fn interleave_is_round_robin_and_keeps_each_languages_order() -> TestResult {
    for ja in 0..=4 {
        for en in 0..=4 {
            if ja + en == 0 {
                continue;
            }
            let clips = clips_of(ja, en);
            let total: usize = clips.iter().map(|c| c.len).sum();
            let material = material(&format!("t006-{ja}-{en}"), &clips)?;
            let joined = interleaved(&material, "join", &["ja", "en"], 10)?;
            assert!(
                joined.succeeded(),
                "{ja}/{en}: {}",
                stderr_of(&joined.output)
            );
            let rows = joined.rows()?;
            let (want_langs, want_refs): (Vec<_>, Vec<_>) =
                round_robin_reference(ja, en).into_iter().unzip();
            assert_eq!(strings(&rows, "lang")?, want_langs, "{ja}/{en}");
            assert_eq!(strings(&rows, "ref")?, want_refs, "{ja}/{en}");
            let want_len = total + (ja + en - 1) * GAP_10_MS;
            assert_eq!(joined.samples()?.len(), want_len, "{ja}/{en}");
        }
    }
    Ok(())
}

#[test]
fn interleave_with_one_part_gives_the_same_join_as_without_it() -> TestResult {
    let material = material("t007", &six())?;
    let turns = interleaved(&material, "turns", &["ja"], 10)?;
    let blocks = join(&material, "blocks", &["--concat"], &["ja"], 10)?;
    assert!(turns.succeeded() && blocks.succeeded());
    assert_eq!(turns.rows()?.len(), 3);
    assert_eq!(
        std::fs::read(&turns.manifest)?,
        std::fs::read(&blocks.manifest)?
    );
    assert_eq!(std::fs::read(&turns.wav)?, std::fs::read(&blocks.wav)?);
    Ok(())
}

#[test]
fn three_parts_are_taken_in_turn_and_an_exhausted_part_is_skipped() -> TestResult {
    let clips = vec![
        clip("a1", "ja", 100),
        clip("a2", "ja", 60),
        clip("b1", "en", 50),
        clip("b2", "en", 70),
        clip("c1", "zh", 80),
    ];
    let material = material("t008", &clips)?;
    let joined = interleaved(&material, "join", &["ja", "en", "zh"], 10)?;
    assert!(joined.succeeded(), "{}", stderr_of(&joined.output));
    let rows = joined.rows()?;
    assert_eq!(strings(&rows, "lang")?, ["ja", "en", "zh", "ja", "en"]);
    assert_eq!(strings(&rows, "ref")?, ["a1", "b1", "c1", "a2", "b2"]);
    Ok(())
}

#[test]
fn every_row_of_an_interleaved_manifest_carries_lang_span_and_sha256() -> TestResult {
    let joined = six_joined("t009")?;
    let rows = joined.rows()?;
    assert_eq!(rows.len(), 6);
    for row in &rows {
        for key in ["lang", "start_sample", "end_sample", "sha256", "wav", "ref"] {
            assert!(row.get(key).is_some(), "{row} has no `{key}`");
        }
        assert!(number(row, "start_sample")? < number(row, "end_sample")?);
        let bytes = std::fs::read(text(row, "wav")?)?;
        assert_eq!(text(row, "sha256")?, sha256_hex(&bytes));
    }
    Ok(())
}

#[test]
fn a_clip_that_no_longer_matches_its_sha256_is_refused_under_interleave() -> TestResult {
    let material = material("t010", &six())?;
    write_constant_wav(&material.dir.join("a2.wav"), 60, LEVEL + 1)?;
    let joined = interleaved(&material, "join", &["ja", "en"], 10)?;
    assert!(!joined.succeeded());
    assert!(stderr_of(&joined.output).contains("sha256"));
    assert!(joined.wrote_nothing());
    Ok(())
}

#[test]
fn a_part_with_no_clips_of_its_language_is_skipped() -> TestResult {
    let clips = vec![
        clip("b1", "en", 50),
        clip("b2", "en", 70),
        clip("b3", "en", 90),
    ];
    let material = material("t011", &clips)?;
    let joined = interleaved(&material, "join", &["ja", "en"], 10)?;
    assert!(joined.succeeded(), "{}", stderr_of(&joined.output));
    assert_eq!(strings(&joined.rows()?, "lang")?, ["en", "en", "en"]);
    Ok(())
}

#[test]
fn parts_that_hold_no_clips_are_refused_under_interleave() -> TestResult {
    let material = material("t012", &[])?;
    let joined = interleaved(&material, "join", &["ja", "en"], 10)?;
    assert_eq!(joined.output.status.code(), Some(1));
    assert!(stderr_of(&joined.output).contains("no clips"));
    assert!(joined.wrote_nothing());
    Ok(())
}

#[test]
fn a_zero_gap_puts_clips_back_to_back() -> TestResult {
    let material = material("t013", &six())?;
    let joined = interleaved(&material, "join", &["ja", "en"], 0)?;
    assert!(joined.succeeded(), "{}", stderr_of(&joined.output));
    let rows = joined.rows()?;
    assert_eq!(
        numbers(&rows, "start_sample")?,
        [0, 100, 150, 210, 280, 360]
    );
    assert_eq!(
        numbers(&rows, "end_sample")?,
        [100, 150, 210, 280, 360, 450]
    );
    assert_eq!(joined.samples()?.len(), 450);
    Ok(())
}

#[test]
#[ignore = "reads testdata or run records, which are not in git"]
fn lat_dialog_alternates_ja_and_en_with_an_800_ms_gap() -> TestResult {
    let data = testdata()?;
    let root = data.parent().ok_or("testdata has no parent directory")?;
    let rows = read_rows(&data.join("manifests/lat-dialog.json"))?;
    let langs = strings(&rows, "lang")?;
    assert_eq!(rows.len(), 30);
    assert_eq!(langs[0], "ja");
    assert_eq!(langs.iter().filter(|lang| **lang == "ja").count(), 15);
    assert_eq!(langs.iter().filter(|lang| **lang == "en").count(), 15);
    check_alternates(&langs)?;
    let starts = numbers(&rows, "start_sample")?;
    assert_eq!(starts[0], 0);
    let mut lengths = Vec::with_capacity(rows.len());
    for row in &rows {
        let bytes = std::fs::read(root.join(text(row, "wav")?))?;
        assert_eq!(text(row, "sha256")?, sha256_hex(&bytes));
        lengths.push(read_samples(&root.join(text(row, "wav")?))?.len() as u64);
    }
    for at in 1..rows.len() {
        assert_eq!(
            starts[at] - (starts[at - 1] + lengths[at - 1]),
            GAP_800_MS,
            "row {at}"
        );
    }
    let total: u64 = lengths.iter().sum::<u64>() + 29 * GAP_800_MS;
    assert_eq!(
        read_samples(&data.join("lat-dialog.wav"))?.len() as u64,
        total
    );
    Ok(())
}

#[test]
fn interleave_without_concat_is_a_usage_error_and_writes_nothing() -> TestResult {
    let material = material("t015", &six())?;
    let without_concat = join(&material, "refused", &["--interleave"], &["ja", "en"], 10)?;
    assert_eq!(without_concat.output.status.code(), Some(2));
    assert!(stderr_of(&without_concat.output).contains("--concat"));
    assert!(without_concat.wrote_nothing());
    let with_concat = interleaved(&material, "joined", &["ja", "en"], 10)?;
    assert_eq!(with_concat.output.status.code(), Some(0));
    assert!(with_concat.manifest.exists() && with_concat.wav.exists());
    Ok(())
}

#[test]
fn interleave_beside_pin_is_a_usage_error_and_never_a_silent_pin() -> TestResult {
    let material = material("t016", &six())?;
    let pinned = utf8(&material.pinned)?;
    let out_a = material.dir.join("a.json");
    let a = tool(&[
        "fetch-set",
        "--pin",
        pinned,
        "--interleave",
        "--out",
        utf8(&out_a)?,
    ])?;
    assert_eq!(a.status.code(), Some(2));
    assert!(stderr_of(&a).contains("--concat"));
    assert!(!out_a.exists());
    let out_b = material.dir.join("b.json");
    let wav_b = material.dir.join("b.wav");
    let part_ja = format!("manifest:{pinned}:ja");
    let part_en = format!("manifest:{pinned}:en");
    let b = tool(&[
        "fetch-set",
        "--pin",
        pinned,
        "--concat",
        "--interleave",
        "--part",
        &part_ja,
        "--part",
        &part_en,
        "--wav-out",
        utf8(&wav_b)?,
        "--out",
        utf8(&out_b)?,
    ])?;
    assert_eq!(b.status.code(), Some(2));
    let said = stderr_of(&b);
    assert!(
        said.contains("--pin") && said.contains("--concat"),
        "{said}"
    );
    assert!(!out_b.exists());
    Ok(())
}
