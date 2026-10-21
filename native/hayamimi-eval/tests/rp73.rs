//! RP-73: the verdict on `L` (RP-67) does not hang on where the start of `L`
//! was placed.
//!
//! `L` is counted from `end_sample` of the manifest, the end of the voice that
//! `manifest::speech_end` finds. Of the utterances, the voice end of some is
//! certain, of some it is cut by the clip, of the rest it is not known
//! (`manifest::EndGroup`). The p95 of `L` is judged by RP-67's own counting
//! (`support::rp67`) on all utterances and on the certain ones alone, at both
//! low lines (`manifest::LOW_LINES_DB`). The verdicts agree, or the language is
//! named as split. An agreement on a failure is an agreement: RP-73 does not
//! pass RP-67 and RP-67 does not pass RP-73.
//!
//! The groups of a language are a table (utterance `k` is entry `k - 1`) made
//! from the clips a manifest names, or written out for the fixtures. The
//! manifest is also read back: the end of each clip found again must be its
//! `end_sample` minus its `start_sample`.
//!
//! The rows that read `testdata/` and `docs/results/rp67/` are run by hand:
//!
//!     cargo test --release -p hayamimi-eval --test rp73 -- --ignored --nocapture
//!
//! Every other row runs on fixtures built by a formula or on the records kept
//! in `docs/results/`.

mod support;

use std::error::Error;
use std::path::{Path, PathBuf};

use hayamimi_eval::compare::read_samples;
use hayamimi_eval::latency::Sample;
use hayamimi_eval::manifest::{
    EndGroup, EndReading, LOW_LINES_DB, Row, read_end, read_rows, speech_end, write_rows,
};
use hayamimi_eval::wav::{SAMPLE_RATE, read_wav, write_wav};
use serde_json::json;
use support::rp32::scratch_dir;
use support::rp67::{Column, LINE_US, Lang, Report, p95_failure, report_of, sample, show};
use support::{FIXTURES, Failure, RESULTS, TestResult, find_up};

/// How many low lines are counted side by side.
const LINES: usize = LOW_LINES_DB.len();

const JA_UTTERANCES: u32 = 23;
const EN_UTTERANCES: u32 = 15;
const REPEATS: u32 = 5;

const JA_MANIFEST: &str = "testdata/manifests/lat-ja.json";
const EN_MANIFEST: &str = "testdata/manifests/lat-en.json";

/// The groups of lat-ja: certain at the 6 dB line, cut, and certain at the 10 dB line.
const JA_CERTAIN_6: [u32; 4] = [1, 6, 8, 16];
const JA_CUT: [u32; 8] = [10, 11, 13, 14, 19, 20, 21, 22];
const JA_CERTAIN_10: [u32; 3] = [1, 6, 8];
/// The three lat-en utterances the fixtures take as not certain at the 10 dB line.
const EN_UNCERTAIN_10: [u32; 3] = [2, 9, 14];

// ---- the groups of a language ----

/// The group of each utterance of a language at every low line. Utterance
/// `k` is entry `k - 1`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct GroupTable {
    by_line: [Vec<EndGroup>; LINES],
}

impl GroupTable {
    /// Utterances `1..=count`: those in `cut` are cut, those in `certain[line]`
    /// are certain at that line, the rest are not certain.
    fn from_sets(count: u32, cut: &[u32], certain: [&[u32]; LINES]) -> Self {
        let group_of = |number: u32, set: &[u32]| {
            if cut.contains(&number) {
                EndGroup::Cut
            } else if set.contains(&number) {
                EndGroup::Certain
            } else {
                EndGroup::Uncertain
            }
        };
        Self {
            by_line: certain.map(|set| (1..=count).map(|number| group_of(number, set)).collect()),
        }
    }

    fn from_utterances(utterances: &[Utterance]) -> Self {
        Self {
            by_line: LOW_LINES_DB.map(|db| {
                utterances
                    .iter()
                    .map(|utterance| utterance.reading.group(db))
                    .collect()
            }),
        }
    }

    /// The numbers of the utterances in `group` at low line number `line`.
    fn utterances(&self, line: usize, group: EndGroup) -> Vec<u32> {
        self.by_line[line]
            .iter()
            .zip(1_u32..)
            .filter(|(found, _)| **found == group)
            .map(|(_, number)| number)
            .collect()
    }
}

fn ja_table() -> GroupTable {
    GroupTable::from_sets(JA_UTTERANCES, &JA_CUT, [&JA_CERTAIN_6, &JA_CERTAIN_10])
}

/// lat-en: every utterance is certain at the 6 dB line, twelve at the 10 dB line.
fn en_table() -> GroupTable {
    let all: Vec<u32> = (1..=EN_UTTERANCES).collect();
    let twelve: Vec<u32> = all
        .iter()
        .copied()
        .filter(|number| !EN_UNCERTAIN_10.contains(number))
        .collect();
    GroupTable::from_sets(EN_UTTERANCES, &[], [&all, &twelve])
}

/// A table that holds every utterance of a language as certain at every line.
fn all_certain(count: u32) -> GroupTable {
    let all: Vec<u32> = (1..=count).collect();
    GroupTable::from_sets(count, &[], [&all, &all])
}

// ---- the judgement ----

/// One language's samples and the groups their utterances fall in.
struct LangRun {
    lang: Lang,
    samples: Vec<Sample>,
    table: GroupTable,
}

fn ja_run(samples: Vec<Sample>) -> LangRun {
    LangRun {
        lang: Lang::Ja,
        samples,
        table: ja_table(),
    }
}

fn en_run(samples: Vec<Sample>) -> LangRun {
    LangRun {
        lang: Lang::En,
        samples,
        table: en_table(),
    }
}

/// The samples of the utterances in `utterances`.
fn pick(samples: &[Sample], utterances: &[u32]) -> Vec<Sample> {
    samples
        .iter()
        .filter(|sample| utterances.contains(&sample.utt))
        .cloned()
        .collect()
}

/// Which side of the pass line of RP-67 the p95 of a set falls on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    Inside,
    Outside,
}

/// The side of a set, by the failure RP-67 gives it; `None` when the set has
/// no valid sample with a translation, which is no verdict at all.
fn side_of(lang: Lang, report: &Report) -> Option<Side> {
    report.l?;
    Some(if p95_failure(lang, report).is_some() {
        Side::Outside
    } else {
        Side::Inside
    })
}

fn side_text(side: Option<Side>) -> String {
    let line = LINE_US / 1000;
    side.map_or_else(
        || format!("no sample for the {line} ms line"),
        |side| match side {
            Side::Inside => format!("within the {line} ms line"),
            Side::Outside => format!("over the {line} ms line"),
        },
    )
}

/// What one low line makes of a language: the certain utterances counted by
/// RP-67, and the `L` of the other two groups, written and not judged.
struct LineCount {
    certain: Report,
    cut: Option<Column>,
    uncertain: Option<Column>,
}

struct LangJudgement {
    lang: Lang,
    all: Report,
    lines: Vec<LineCount>,
}

fn count_line(run: &LangRun, line: usize) -> Result<LineCount, Failure> {
    let of_group = |group: EndGroup| -> Result<Report, Failure> {
        report_of(&pick(&run.samples, &run.table.utterances(line, group)))
    };
    Ok(LineCount {
        certain: of_group(EndGroup::Certain)?,
        cut: of_group(EndGroup::Cut)?.l,
        uncertain: of_group(EndGroup::Uncertain)?.l,
    })
}

fn judge_lang(run: &LangRun) -> Result<LangJudgement, Failure> {
    Ok(LangJudgement {
        lang: run.lang,
        all: report_of(&run.samples)?,
        lines: (0..LINES)
            .map(|line| count_line(run, line))
            .collect::<Result<_, _>>()?,
    })
}

impl LangJudgement {
    /// Why the verdicts of this language do not agree, one text per reason.
    fn faults(&self) -> Vec<String> {
        let label = self.lang.label();
        let Some(all_side) = side_of(self.lang, &self.all) else {
            return vec![format!("{label}: no valid sample has a translation")];
        };
        let mut faults = Vec::new();
        for (db, count) in LOW_LINES_DB.iter().zip(&self.lines) {
            match side_of(self.lang, &count.certain) {
                None => faults.push(format!(
                    "{label}: no valid sample with a translation among the utterances certain at {db} dB (an empty set is not an agreement)"
                )),
                Some(side) if side != all_side => faults.push(format!(
                    "{label}: 組で割れた at {db} dB: the certain utterances are {} ({}), all utterances are {} ({}); the start of L or too few samples can each split it, and this does not say which",
                    side_text(Some(side)),
                    show(count.certain.l),
                    side_text(Some(all_side)),
                    show(self.all.l),
                )),
                Some(_) => {}
            }
        }
        faults
    }

    fn render(&self) -> Vec<String> {
        let label = self.lang.label();
        let mut lines = vec![format!(
            "{label}: all utterances: L {}: {}",
            show(self.all.l),
            side_text(side_of(self.lang, &self.all))
        )];
        for (db, count) in LOW_LINES_DB.iter().zip(&self.lines) {
            lines.push(format!(
                "{label}: certain at {db} dB: L {}: {}",
                show(count.certain.l),
                side_text(side_of(self.lang, &count.certain))
            ));
            lines.push(format!(
                "{label}: groups at {db} dB: certain: {}; cut: {}; uncertain: {}",
                show(count.certain.l),
                show(count.cut),
                show(count.uncertain)
            ));
        }
        lines
    }
}

/// RP-73 over the languages: every language agrees with itself, or it is named.
struct Judgement {
    langs: Vec<LangJudgement>,
}

fn judge(runs: &[LangRun]) -> Result<Judgement, Failure> {
    Ok(Judgement {
        langs: runs.iter().map(judge_lang).collect::<Result<_, _>>()?,
    })
}

impl Judgement {
    fn result(&self) -> Result<(), Failure> {
        let faults: Vec<String> = self.langs.iter().flat_map(LangJudgement::faults).collect();
        if faults.is_empty() {
            Ok(())
        } else {
            Err(faults.join("; "))
        }
    }

    fn lang(&self, lang: Lang) -> Result<&LangJudgement, Failure> {
        self.langs
            .iter()
            .find(|judged| judged.lang == lang)
            .ok_or_else(|| format!("the judgement holds no {}", lang.label()))
    }

    fn render(&self) -> String {
        let mut lines: Vec<String> = self.langs.iter().flat_map(LangJudgement::render).collect();
        lines.push(match self.result() {
            Ok(()) => String::from(
                "RP-73: the verdicts agree on every set (an agreement is not a pass of RP-67)",
            ),
            Err(why) => format!("RP-73: FAIL: {why}"),
        });
        lines.join("\n")
    }
}

/// The failure text of a judgement that must fail; a pass is an error of the test.
fn rejection(judgement: &Judgement) -> Result<String, Failure> {
    match judgement.result() {
        Ok(()) => Err(format!(
            "the verdicts agreed, but they must split:\n{}",
            judgement.render()
        )),
        Err(said) => Ok(said),
    }
}

// ---- samples built by a formula ----

/// A sample of utterance `utt` whose `L` is `l_ms`: a wait of 500 ms, a
/// decode of 150 ms and a translation of `l_ms - 650` ms.
fn timed(utt: u32, repeat: u32, l_ms: i64) -> Sample {
    Sample {
        repeat,
        ..sample(utt, 500, 150, Some(l_ms - 650))
    }
}

/// `REPEATS` samples of each utterance `1..=count`, made by `make(utt, repeat)`.
fn samples_by(count: u32, make: impl Fn(u32, u32) -> Sample) -> Vec<Sample> {
    (1..=count)
        .flat_map(|utt| (1..=REPEATS).map(move |repeat| (utt, repeat)))
        .map(|(utt, repeat)| make(utt, repeat))
        .collect()
}

/// Every sample at `l_ms`.
fn flat(count: u32, l_ms: i64) -> Vec<Sample> {
    samples_by(count, |utt, repeat| timed(utt, repeat, l_ms))
}

/// The samples of the utterances in `slow` at `slow_ms`, the others at 900 ms.
fn slow_utterances(count: u32, slow: &[u32], slow_ms: i64) -> Vec<Sample> {
    samples_by(count, |utt, repeat| {
        timed(utt, repeat, if slow.contains(&utt) { slow_ms } else { 900 })
    })
}

/// The samples of the utterances in `fast` at 900 ms, the others at `other_ms`.
fn fast_utterances(count: u32, fast: &[u32], other_ms: i64) -> Vec<Sample> {
    samples_by(count, |utt, repeat| {
        timed(
            utt,
            repeat,
            if fast.contains(&utt) { 900 } else { other_ms },
        )
    })
}

/// The samples whose (utterance, repeat) is in `slow` at 1,700 ms, the others at 900 ms.
fn slow_samples(count: u32, slow: &[(u32, u32)]) -> Vec<Sample> {
    samples_by(count, |utt, repeat| {
        timed(
            utt,
            repeat,
            if slow.contains(&(utt, repeat)) {
                1700
            } else {
                900
            },
        )
    })
}

/// `(n, p95)` of a column, in microseconds.
fn n_p95(column: Option<Column>) -> Option<(usize, i64)> {
    column.map(|c| (c.n, c.p95_us))
}

/// `(n, p50, p95)` of a column, in microseconds.
fn triple(column: Option<Column>) -> Option<(usize, i64, i64)> {
    column.map(|c| (c.n, c.p50_us, c.p95_us))
}

// ---- records ----

/// The samples of the E2E-1 and E2E-2 runs a directory holds, ja then en.
fn load_records(dir: &Path) -> Result<(Vec<Sample>, Vec<Sample>), Failure> {
    let names: Vec<String> = [Lang::Ja, Lang::En]
        .iter()
        .map(|lang| format!("{}.jsonl", lang.stem()))
        .collect();
    let missing: Vec<&str> = names
        .iter()
        .map(String::as_str)
        .filter(|name| !dir.join(name).is_file())
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "no record: {} of the 2 runs are missing: {} in {}",
            missing.len(),
            missing.join(", "),
            dir.display()
        ));
    }
    let read = |name: &str| read_samples(&dir.join(name)).map_err(|why| why.to_string());
    Ok((read(&names[0])?, read(&names[1])?))
}

fn rs_samples() -> Result<Vec<Sample>, Failure> {
    let mut all = Vec::new();
    for number in 1..=3 {
        let path = Path::new(RESULTS)
            .join("rp32")
            .join(format!("rs{number}.jsonl"));
        all.extend(read_samples(&path).map_err(|why| why.to_string())?);
    }
    Ok(all)
}

fn main_run(lang: Lang) -> Result<Vec<Sample>, Failure> {
    let path = Path::new(RESULTS)
        .join("e2e")
        .join(lang.stem())
        .join(format!("{}.jsonl", lang.stem()));
    read_samples(&path).map_err(|why| why.to_string())
}

fn write_jsonl(path: &Path, samples: &[Sample]) -> Result<(), Box<dyn Error>> {
    let lines: Result<Vec<String>, _> = samples.iter().map(serde_json::to_string).collect();
    std::fs::write(path, format!("{}\n", lines?.join("\n")))?;
    Ok(())
}

// ---- a manifest read back against its clips ----

/// One utterance of a manifest: where it says the voice ends, and what its clip says.
struct Utterance {
    number: u32,
    start_sample: usize,
    end_sample: usize,
    /// `speech_end` of the clip.
    rule_end: usize,
    reading: EndReading,
}

fn count_field(row: &Row, key: &str, number: u32) -> Result<usize, Failure> {
    let Some(value) = row.get(key).and_then(serde_json::Value::as_u64) else {
        return Err(format!("utterance {number}: no integer `{key}`"));
    };
    usize::try_from(value).map_err(|_| format!("utterance {number}: `{key}` {value} is too large"))
}

fn utterance_of(
    row: &Row,
    number: u32,
    resolve: &dyn Fn(&str) -> Result<PathBuf, Failure>,
) -> Result<Utterance, Failure> {
    let Some(name) = row.get("wav").and_then(serde_json::Value::as_str) else {
        return Err(format!("utterance {number}: no string `wav`"));
    };
    let clip = read_wav(&resolve(name)?).map_err(|why| format!("utterance {number}: {why}"))?;
    let (Some(rule_end), Some(reading)) = (speech_end(&clip.samples), read_end(&clip.samples))
    else {
        return Err(format!("utterance {number}: the clip has no run of speech"));
    };
    Ok(Utterance {
        number,
        start_sample: count_field(row, "start_sample", number)?,
        end_sample: count_field(row, "end_sample", number)?,
        rule_end,
        reading,
    })
}

/// The utterances of a manifest, in row order: utterance `k` is row `k - 1`.
fn utterances_of(
    rows: &[Row],
    resolve: &dyn Fn(&str) -> Result<PathBuf, Failure>,
) -> Result<Vec<Utterance>, Failure> {
    rows.iter()
        .zip(1_u32..)
        .map(|(row, number)| utterance_of(row, number, resolve))
        .collect()
}

/// The utterances of a manifest of `testdata/`, their clips found where the rows say.
fn real_utterances(manifest: &str) -> Result<Vec<Utterance>, Failure> {
    let rows = read_rows(&find_up(manifest)?).map_err(|why| why.to_string())?;
    utterances_of(&rows, &|name| find_up(name))
}

/// Every utterance whose `end_sample` is not its `start_sample` plus the end
/// the rule gives the clip again, named; no tolerance.
fn check_ends(utterances: &[Utterance]) -> Result<(), Failure> {
    let wrong: Vec<String> = utterances
        .iter()
        .filter_map(|utterance| {
            let again = utterance.start_sample + utterance.rule_end;
            (again != utterance.end_sample).then(|| {
                format!(
                    "utterance {}: the manifest has end_sample {} and the rule gives {again} again",
                    utterance.number, utterance.end_sample
                )
            })
        })
        .collect();
    if wrong.is_empty() {
        Ok(())
    } else {
        Err(wrong.join("; "))
    }
}

/// Alternating samples: `amplitude` frames of 320 samples are `frames`.
fn tone(len: usize, amplitude: i16) -> Vec<i16> {
    (0..len)
        .map(|at| if at % 2 == 0 { amplitude } else { -amplitude })
        .collect()
}

fn frames(n: usize, amplitude: i16) -> Vec<i16> {
    tone(n * 320, amplitude)
}

/// A voice that stops dead (certain).
fn clip_001() -> Vec<i16> {
    [frames(20, 164), frames(30, 10362), frames(50, 164)].concat()
}

/// A voice that runs to the last frame (cut).
fn clip_008() -> Vec<i16> {
    [frames(20, 164), frames(40, 10362)].concat()
}

/// A voice with a tail beyond the window (uncertain).
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

/// The three clips beside a manifest that joins them with 24,000 samples of
/// silence between: starts 0, 56,000 and 99,200, and ends 16,000, 75,200 and
/// 115,200. The directory is `name` of this test.
fn three_clip_dir(name: &str) -> Result<PathBuf, Box<dyn Error>> {
    let dir = scratch_dir("rp73", name)?;
    let clips = [
        ("a.wav", clip_001(), 0_u64, 16_000_u64),
        ("b.wav", clip_008(), 56_000, 75_200),
        ("c.wav", clip_007(), 99_200, 115_200),
    ];
    let mut rows = Vec::new();
    for (file, samples, start, end) in &clips {
        write_wav(&dir.join(file), SAMPLE_RATE, samples)?;
        let mut row = Row::new();
        row.insert(String::from("wav"), json!(file));
        row.insert(String::from("start_sample"), json!(start));
        row.insert(String::from("end_sample"), json!(end));
        rows.push(row);
    }
    write_rows(&dir.join("manifest.json"), &rows)?;
    Ok(dir)
}

/// The utterances of the manifest in `dir`, with the `end_sample` of the given
/// (row index, value) pairs replaced.
fn fixture_utterances(dir: &Path, replaced: &[(usize, u64)]) -> Result<Vec<Utterance>, Failure> {
    let mut rows = read_rows(&dir.join("manifest.json")).map_err(|why| why.to_string())?;
    for (index, end) in replaced {
        let Some(row) = rows.get_mut(*index) else {
            return Err(format!("the manifest has no row {index}"));
        };
        row.insert(String::from("end_sample"), json!(end));
    }
    utterances_of(&rows, &|name| Ok(dir.join(name)))
}

// ---- the rows that run on fixtures ----

#[test]
fn a_verdict_that_splits_on_the_certain_utterances_names_the_language() -> TestResult {
    let ja = fast_utterances(JA_UTTERANCES, &JA_CERTAIN_6, 1700);
    let judgement = judge(&[ja_run(ja), en_run(flat(EN_UTTERANCES, 900))])?;
    let said = rejection(&judgement)?;
    for needle in ["組で割れた", "E2E-1 (ja)", "6 dB", "10 dB"] {
        assert!(said.contains(needle), "{needle}: {said}");
    }
    assert!(!said.contains("E2E-2") && !said.contains("(en)"), "{said}");
    let ja = judgement.lang(Lang::Ja)?;
    assert_eq!(n_p95(ja.all.l), Some((115, 1_700_000)));
    assert_eq!(n_p95(ja.lines[0].certain.l), Some((20, 900_000)));
    assert_eq!(n_p95(ja.lines[1].certain.l), Some((15, 900_000)));
    Ok(())
}

#[test]
fn an_agreed_failure_is_not_a_split() -> TestResult {
    let ja = slow_utterances(JA_UTTERANCES, &JA_CERTAIN_6, 1700);
    let judgement = judge(&[ja_run(ja), en_run(flat(EN_UTTERANCES, 900))])?;
    judgement.result()?;
    let ja = judgement.lang(Lang::Ja)?;
    assert_eq!(n_p95(ja.all.l), Some((115, 1_700_000)));
    assert_eq!(n_p95(ja.lines[0].certain.l), Some((20, 1_700_000)));
    assert_eq!(n_p95(ja.lines[1].certain.l), Some((15, 1_700_000)));
    let text = judgement.render();
    let outside: Vec<&str> = text
        .lines()
        .filter(|line| line.contains("E2E-1 (ja)") && line.contains("over the 1600 ms line"))
        .collect();
    assert_eq!(outside.len(), 3, "{text}");
    assert!(
        text.contains("an agreement is not a pass of RP-67"),
        "{text}"
    );
    Ok(())
}

#[test]
fn all_samples_at_900_ms_agree_and_the_groups_are_written() -> TestResult {
    let judgement = judge(&[
        ja_run(flat(JA_UTTERANCES, 900)),
        en_run(flat(EN_UTTERANCES, 900)),
    ])?;
    judgement.result()?;
    let text = judgement.render();
    for needle in [
        "E2E-1 (ja): groups at 6 dB: certain: p50 900.0 ms, p95 900.0 ms (n 20); cut: p50 900.0 ms, p95 900.0 ms (n 40); uncertain: p50 900.0 ms, p95 900.0 ms (n 55)",
        "E2E-1 (ja): groups at 10 dB: certain: p50 900.0 ms, p95 900.0 ms (n 15); cut: p50 900.0 ms, p95 900.0 ms (n 40); uncertain: p50 900.0 ms, p95 900.0 ms (n 60)",
        "E2E-2 (en): groups at 6 dB: certain: p50 900.0 ms, p95 900.0 ms (n 75)",
    ] {
        assert!(text.contains(needle), "{needle}\n{text}");
    }
    Ok(())
}

#[test]
fn a_group_p95_over_the_line_is_written_and_does_not_decide() -> TestResult {
    let ja = slow_utterances(JA_UTTERANCES, &[10], 1700);
    let judgement = judge(&[ja_run(ja), en_run(flat(EN_UTTERANCES, 900))])?;
    judgement.result()?;
    let ja = judgement.lang(Lang::Ja)?;
    assert_eq!(n_p95(ja.all.l), Some((115, 900_000)));
    assert_eq!(n_p95(ja.lines[0].certain.l), Some((20, 900_000)));
    assert_eq!(n_p95(ja.lines[1].certain.l), Some((15, 900_000)));
    assert_eq!(triple(ja.lines[0].cut), Some((40, 900_000, 1_700_000)));
    Ok(())
}

#[test]
fn one_slow_sample_splits_the_10_db_set_alone() -> TestResult {
    let ja = slow_samples(JA_UTTERANCES, &[(1, 1)]);
    let judgement = judge(&[ja_run(ja), en_run(flat(EN_UTTERANCES, 900))])?;
    let said = rejection(&judgement)?;
    assert!(
        said.contains("E2E-1 (ja)") && said.contains("10 dB"),
        "{said}"
    );
    assert!(!said.contains("6 dB"), "{said}");
    let ja = judgement.lang(Lang::Ja)?;
    assert_eq!(n_p95(ja.all.l), Some((115, 900_000)));
    assert_eq!(n_p95(ja.lines[0].certain.l), Some((20, 900_000)));
    assert_eq!(n_p95(ja.lines[1].certain.l), Some((15, 1_700_000)));
    Ok(())
}

#[test]
fn the_6_db_set_of_20_samples_allows_one_over() -> TestResult {
    let en = en_run(flat(EN_UTTERANCES, 900));
    let one = slow_samples(JA_UTTERANCES, &[(16, 1)]);
    judge(&[ja_run(one), en])?.result()?;
    let en = en_run(flat(EN_UTTERANCES, 900));
    let two = slow_samples(JA_UTTERANCES, &[(16, 1), (16, 2)]);
    let judgement = judge(&[ja_run(two), en])?;
    let said = rejection(&judgement)?;
    assert!(
        said.contains("E2E-1 (ja)") && said.contains("6 dB"),
        "{said}"
    );
    assert!(!said.contains("10 dB"), "{said}");
    let ja = judgement.lang(Lang::Ja)?;
    assert_eq!(n_p95(ja.all.l), Some((115, 900_000)));
    assert_eq!(n_p95(ja.lines[0].certain.l), Some((20, 1_700_000)));
    Ok(())
}

/// The slow lat-en utterances of 030: those the fixtures take as not certain at 10 dB.
fn slow_en() -> Vec<Sample> {
    slow_utterances(EN_UTTERANCES, &EN_UNCERTAIN_10, 1700)
}

#[test]
fn a_split_in_en_is_named_alone() -> TestResult {
    let judgement = judge(&[ja_run(flat(JA_UTTERANCES, 900)), en_run(slow_en())])?;
    let said = rejection(&judgement)?;
    assert!(
        said.contains("E2E-2 (en)") && said.contains("10 dB"),
        "{said}"
    );
    assert!(!said.contains("E2E-1") && !said.contains("(ja)"), "{said}");
    assert!(!said.contains("6 dB"), "{said}");
    let en = judgement.lang(Lang::En)?;
    assert_eq!(n_p95(en.all.l), Some((75, 1_700_000)));
    assert_eq!(n_p95(en.lines[0].certain.l), Some((75, 1_700_000)));
    assert_eq!(n_p95(en.lines[1].certain.l), Some((60, 900_000)));
    Ok(())
}

#[test]
fn both_languages_are_named_when_both_split() -> TestResult {
    let ja = slow_samples(JA_UTTERANCES, &[(1, 1)]);
    let said = rejection(&judge(&[ja_run(ja), en_run(slow_en())])?)?;
    for needle in ["E2E-1 (ja)", "E2E-2 (en)", "10 dB"] {
        assert!(said.contains(needle), "{needle}: {said}");
    }
    assert!(!said.contains("6 dB"), "{said}");
    Ok(())
}

#[test]
fn a_certain_set_without_a_translated_valid_sample_is_an_error() -> TestResult {
    let untranslated = samples_by(JA_UTTERANCES, |utt, repeat| {
        if JA_CERTAIN_6.contains(&utt) {
            Sample {
                repeat,
                ..sample(utt, 500, 150, None)
            }
        } else {
            timed(utt, repeat, 900)
        }
    });
    let too_short = samples_by(JA_UTTERANCES, |utt, repeat| {
        if JA_CERTAIN_6.contains(&utt) {
            Sample {
                repeat,
                ..sample(utt, 100, 150, Some(250))
            }
        } else {
            timed(utt, repeat, 900)
        }
    });
    for ja in [untranslated, too_short] {
        let said = rejection(&judge(&[ja_run(ja), en_run(flat(EN_UTTERANCES, 900))])?)?;
        for needle in [
            "E2E-1 (ja)",
            "6 dB",
            "10 dB",
            "no valid sample with a translation",
        ] {
            assert!(said.contains(needle), "{needle}: {said}");
        }
        assert!(!said.contains("E2E-2"), "{said}");
    }
    Ok(())
}

#[test]
fn an_empty_10_db_set_names_the_10_db_line() -> TestResult {
    let ja = samples_by(JA_UTTERANCES, |utt, repeat| {
        if JA_CERTAIN_10.contains(&utt) {
            Sample {
                repeat,
                ..sample(utt, 500, 150, None)
            }
        } else {
            timed(utt, repeat, 900)
        }
    });
    let judgement = judge(&[ja_run(ja), en_run(flat(EN_UTTERANCES, 900))])?;
    let said = rejection(&judgement)?;
    assert!(
        said.contains("E2E-1 (ja)") && said.contains("10 dB"),
        "{said}"
    );
    assert!(
        said.contains("no valid sample with a translation"),
        "{said}"
    );
    assert!(!said.contains("6 dB"), "{said}");
    let ja = judgement.lang(Lang::Ja)?;
    assert_eq!(n_p95(ja.lines[0].certain.l), Some((5, 900_000)));
    Ok(())
}

#[test]
fn the_split_message_carries_the_counts_and_names_no_cause() -> TestResult {
    let ja = fast_utterances(JA_UTTERANCES, &JA_CERTAIN_6, 1700);
    let said = rejection(&judge(&[ja_run(ja), en_run(flat(EN_UTTERANCES, 900))])?)?;
    for needle in [
        "組で割れた",
        "(ja)",
        "6 dB",
        "10 dB",
        "p95 1700.0 ms (n 115)",
        "p95 900.0 ms (n 20)",
        "p95 900.0 ms (n 15)",
    ] {
        assert!(said.contains(needle), "{needle}: {said}");
    }
    assert!(!said.contains("誤り"), "{said}");
    Ok(())
}

/// Samples of the three utterances of the fixture manifest, utterance 2 slow.
fn slow_second_of_three() -> Vec<Sample> {
    samples_by(3, |utt, repeat| {
        timed(utt, repeat, if utt == 2 { 1700 } else { 900 })
    })
}

#[test]
fn utterance_k_is_the_manifest_row_k_minus_1() -> TestResult {
    let dir = three_clip_dir("t035")?;
    let utterances = fixture_utterances(&dir, &[])?;
    let table = GroupTable::from_utterances(&utterances);
    let expected = vec![EndGroup::Certain, EndGroup::Cut, EndGroup::Uncertain];
    assert_eq!(table.by_line, [expected.clone(), expected]);
    let run = LangRun {
        lang: Lang::Ja,
        samples: slow_second_of_three(),
        table,
    };
    let judgement = judge(&[run])?;
    let said = rejection(&judgement)?;
    assert!(
        said.contains("E2E-1 (ja)") && said.contains("6 dB") && said.contains("10 dB"),
        "{said}"
    );
    let ja = judgement.lang(Lang::Ja)?;
    assert_eq!(n_p95(ja.all.l), Some((15, 1_700_000)));
    assert_eq!(n_p95(ja.lines[0].certain.l), Some((5, 900_000)));
    Ok(())
}

#[test]
fn a_manifest_made_by_the_join_matches_the_rule_again() -> TestResult {
    let dir = three_clip_dir("t036")?;
    check_ends(&fixture_utterances(&dir, &[])?)?;
    Ok(())
}

#[test]
fn a_manifest_end_moved_300_ms_earlier_is_named_by_its_utterance() -> TestResult {
    let dir = three_clip_dir("t037")?;
    let said = check_ends(&fixture_utterances(&dir, &[(0, 11_200)])?).err();
    let Some(said) = said else {
        return Err("a manifest end moved 300 ms earlier was accepted".into());
    };
    assert!(said.contains("utterance 1:"), "{said}");
    assert!(said.contains("11200") && said.contains("16000"), "{said}");
    assert!(
        !said.contains("utterance 2") && !said.contains("utterance 3"),
        "{said}"
    );
    Ok(())
}

#[test]
fn a_manifest_end_one_sample_off_is_named() -> TestResult {
    let dir = three_clip_dir("t038")?;
    let said = check_ends(&fixture_utterances(&dir, &[(1, 75_201)])?).err();
    let Some(said) = said else {
        return Err("a manifest end one sample off was accepted".into());
    };
    assert!(said.contains("utterance 2:"), "{said}");
    assert!(
        !said.contains("utterance 1") && !said.contains("utterance 3"),
        "{said}"
    );
    Ok(())
}

#[test]
fn every_utterance_whose_end_differs_is_named() -> TestResult {
    let dir = three_clip_dir("t039")?;
    let said = check_ends(&fixture_utterances(&dir, &[(0, 15_680), (2, 114_880)])?).err();
    let Some(said) = said else {
        return Err("two manifest ends moved were accepted".into());
    };
    assert!(
        said.contains("utterance 1:") && said.contains("utterance 3:"),
        "{said}"
    );
    assert!(!said.contains("utterance 2"), "{said}");
    Ok(())
}

#[test]
fn the_recorded_rp32_runs_do_not_split_on_ja() -> TestResult {
    let judgement = judge(&[ja_run(rs_samples()?)])?;
    println!("{}", judgement.render());
    judgement.result()?;
    let ja = judgement.lang(Lang::Ja)?;
    assert_eq!(n_p95(ja.all.l), Some((291, 1_460_791)));
    assert_eq!(n_p95(ja.lines[0].certain.l), Some((60, 1_491_084)));
    assert_eq!(n_p95(ja.lines[1].certain.l), Some((45, 1_222_845)));
    assert_eq!(
        triple(ja.lines[0].certain.l),
        Some((60, 739_306, 1_491_084))
    );
    assert_eq!(triple(ja.lines[0].cut), Some((84, 748_865, 1_370_740)));
    assert_eq!(
        triple(ja.lines[0].uncertain),
        Some((147, 908_165, 1_425_901))
    );
    Ok(())
}

#[test]
fn the_recorded_main_ja_run_fails_alike_on_every_set() -> TestResult {
    let judgement = judge(&[ja_run(main_run(Lang::Ja)?)])?;
    println!("{}", judgement.render());
    judgement.result()?;
    let ja = judgement.lang(Lang::Ja)?;
    assert_eq!(n_p95(ja.all.l), Some((93, 1_871_715)));
    assert_eq!(n_p95(ja.lines[0].certain.l), Some((20, 1_777_552)));
    assert_eq!(n_p95(ja.lines[1].certain.l), Some((15, 1_813_440)));
    for report in [&ja.all, &ja.lines[0].certain, &ja.lines[1].certain] {
        assert_eq!(side_of(Lang::Ja, report), Some(Side::Outside));
    }
    assert_eq!(triple(ja.lines[0].cut), Some((24, 1_277_621, 1_458_034)));
    assert_eq!(
        triple(ja.lines[0].uncertain),
        Some((49, 1_444_076, 1_925_482))
    );
    assert_eq!(
        ja.lines[0].certain.l.map(|c| (c.n, c.p50_us)),
        Some((20, 1_476_481))
    );
    let text = judgement.render();
    assert_eq!(text.matches("over the 1600 ms line").count(), 3, "{text}");
    Ok(())
}

#[test]
fn the_recorded_main_en_run_is_the_same_on_all_and_on_the_certain_set() -> TestResult {
    // The 10 dB line needs the groups of the real clips; the ignored row of
    // the 12 utterances reads them.
    let run = LangRun {
        lang: Lang::En,
        samples: main_run(Lang::En)?,
        table: all_certain(EN_UTTERANCES),
    };
    let judgement = judge(&[run])?;
    judgement.result()?;
    let en = judgement.lang(Lang::En)?;
    assert_eq!(triple(en.all.l), Some((70, 970_333, 1_303_167)));
    assert_eq!(en.lines[0].certain.l, en.all.l);
    Ok(())
}

#[test]
fn a_directory_with_no_rp73_record_is_named_and_not_passed() -> TestResult {
    let said = load_records(&Path::new(FIXTURES).join("no-such-rp73-records")).err();
    assert!(
        said.as_deref()
            .is_some_and(|text| text.starts_with("no record")
                && text.contains("e2e1.jsonl")
                && text.contains("e2e2.jsonl")),
        "{said:?}"
    );
    let one = scratch_dir("rp73", "t043-one")?;
    write_jsonl(&one.join("e2e1.jsonl"), &flat(JA_UTTERANCES, 900))?;
    let said = load_records(&one).err();
    assert!(
        said.as_deref()
            .is_some_and(|text| text.starts_with("no record")
                && text.contains("e2e2.jsonl")
                && !text.contains("e2e1.jsonl")),
        "{said:?}"
    );
    // The control: a directory that holds both is read.
    let both = scratch_dir("rp73", "t043-both")?;
    write_jsonl(&both.join("e2e1.jsonl"), &flat(JA_UTTERANCES, 900))?;
    write_jsonl(&both.join("e2e2.jsonl"), &flat(EN_UTTERANCES, 900))?;
    let (ja, en) = load_records(&both)?;
    assert_eq!((ja.len(), en.len()), (115, 75));
    Ok(())
}

// ---- the rows that run on the clips and the records of a run ----

#[test]
#[ignore = "reads testdata/, which git does not carry"]
fn the_lat_ja_and_lat_en_clips_fall_in_the_recorded_groups() -> TestResult {
    let ja = GroupTable::from_utterances(&real_utterances(JA_MANIFEST)?);
    let en = GroupTable::from_utterances(&real_utterances(EN_MANIFEST)?);
    assert_eq!(ja.utterances(0, EndGroup::Certain), JA_CERTAIN_6);
    assert_eq!(ja.utterances(0, EndGroup::Cut), JA_CUT);
    assert_eq!(
        ja.utterances(0, EndGroup::Uncertain),
        [2, 3, 4, 5, 7, 9, 12, 15, 17, 18, 23]
    );
    assert_eq!(ja.utterances(1, EndGroup::Certain), JA_CERTAIN_10);
    assert_eq!(ja.utterances(1, EndGroup::Cut), JA_CUT);
    assert_eq!(ja.utterances(1, EndGroup::Uncertain).len(), 12);
    assert_eq!(en.utterances(0, EndGroup::Certain).len(), 15);
    assert_eq!(en.utterances(1, EndGroup::Certain).len(), 12);
    println!(
        "lat-en: not certain at 10 dB: {:?}",
        en.utterances(1, EndGroup::Uncertain)
    );
    Ok(())
}

#[test]
#[ignore = "reads testdata/, which git does not carry"]
fn every_manifest_end_sample_is_the_end_the_rule_gives_again() -> TestResult {
    for manifest in [JA_MANIFEST, EN_MANIFEST] {
        let utterances = real_utterances(manifest)?;
        println!("{manifest}: {} utterances", utterances.len());
        check_ends(&utterances)?;
    }
    Ok(())
}

#[test]
#[ignore = "reads testdata/, which git does not carry"]
fn a_real_manifest_end_moved_300_ms_earlier_is_named() -> TestResult {
    let mut rows = read_rows(&find_up(JA_MANIFEST)?)?;
    let Some(first) = rows.first_mut() else {
        return Err("the lat-ja manifest holds no row".into());
    };
    let Some(end) = first.get("end_sample").and_then(serde_json::Value::as_u64) else {
        return Err("the first row has no end_sample".into());
    };
    first.insert(String::from("end_sample"), json!(end - 4_800));
    let utterances = utterances_of(&rows, &|name| find_up(name))?;
    let Some(said) = check_ends(&utterances).err() else {
        return Err("a manifest end moved 300 ms earlier was accepted".into());
    };
    assert!(said.contains("utterance 1:"), "{said}");
    assert_eq!(said.matches("utterance ").count(), 1, "{said}");
    Ok(())
}

#[test]
#[ignore = "reads testdata/, which git does not carry"]
fn the_recorded_en_run_is_the_same_on_the_12_utterances_certain_at_10_db() -> TestResult {
    let run = LangRun {
        lang: Lang::En,
        samples: main_run(Lang::En)?,
        table: GroupTable::from_utterances(&real_utterances(EN_MANIFEST)?),
    };
    let judgement = judge(&[run])?;
    println!("{}", judgement.render());
    judgement.result()?;
    let en = judgement.lang(Lang::En)?;
    assert_eq!(n_p95(en.all.l), Some((70, 1_303_167)));
    let Some((n, p95)) = n_p95(en.lines[1].certain.l) else {
        return Err("the 12 utterances have no valid sample with a translation".into());
    };
    assert_eq!(n, 60);
    assert!((1_270_000..=1_272_000).contains(&p95), "p95 {p95} us");
    Ok(())
}

#[test]
#[ignore = "reads docs/results/rp67/, where the records of the E4 window are not placed yet"]
fn rp73_the_l_verdict_is_the_same_on_utterances_whose_end_is_certain() -> TestResult {
    let (ja, en) = load_records(&Path::new(RESULTS).join("rp67"))?;
    let runs = [
        LangRun {
            lang: Lang::Ja,
            samples: ja,
            table: GroupTable::from_utterances(&real_utterances(JA_MANIFEST)?),
        },
        LangRun {
            lang: Lang::En,
            samples: en,
            table: GroupTable::from_utterances(&real_utterances(EN_MANIFEST)?),
        },
    ];
    let judgement = judge(&runs)?;
    println!("{}", judgement.render());
    judgement.result()?;
    Ok(())
}
