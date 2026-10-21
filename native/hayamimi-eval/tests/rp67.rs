//! RP-67: the p95 of `L` (speech end to the translation of the closing
//! `final`) is within 1600 ms, for E2E-1 (ja to en) and E2E-2 (en to ja).
//!
//! The pass line is 1600 ms. 1200 ms is a target: the number of samples over it
//! and whether the p95 meets it are written beside the verdict and never judged.
//!
//! A sample is valid when it has a `final` and `L_asr - L_dec` is at least
//! 0.12 s (the early detector's silence), whichever rule closed it. `L` is
//! counted over the valid samples that got a translation; the valid samples
//! without one are counted apart, so a slow translation cannot leave the
//! sample set. The p95 is the nearest rank on microseconds and is compared
//! to the line without rounding. The same run is also counted at 0.30 s and
//! written beside, with the samples that are valid at 0.12 s alone.
//!
//! Every Rust run goes through the machine gate of RP-83
//! (`support::machine::check_runs`), so a p95 inside the line is not a pass
//! on a busy machine.
//!
//! The live row reads `docs/results/rp67/e2e1.jsonl` and `e2e2.jsonl` (and the
//! machine files beside them) and is run by hand:
//!
//!     cargo test --release -p hayamimi-eval --test rp67 -- --ignored --nocapture
//!
//! Every other row runs on fixtures built by a formula or on the records of
//! `docs/results/e2e/`, which stand in the repository.

mod support;

use std::path::{Path, PathBuf};

use hayamimi_eval::compare::read_samples;
use hayamimi_eval::latency::Sample;
use hayamimi_eval::machine::FILE_SUFFIX;
use serde_json::json;
use support::machine::{GateReport, RunInput, check_runs, f83_rows, f83_text, text_of};
use support::rp67::{
    Lang, OLD_VALID_MS, Report, VALID_MS, p95_failure, report_of, sample, valid_at,
};
use support::{FIXTURES, Failure, REPO, RESULTS, TestResult};

/// One run: where its samples stand, the samples, and the text of its machine file.
struct Run {
    lang: Lang,
    dir: PathBuf,
    samples: Vec<Sample>,
    machine: Option<String>,
}

/// The text of `<stem>.machine.jsonl` in `dir`; `None` when the file does not stand.
fn read_machine(dir: &Path, stem: &str) -> Result<Option<String>, Failure> {
    let path = dir.join(format!("{stem}.{FILE_SUFFIX}"));
    if !path.is_file() {
        return Ok(None);
    }
    std::fs::read_to_string(&path)
        .map(Some)
        .map_err(|why| format!("{}: {why}", path.display()))
}

/// The run `lang` as `dir` holds it.
fn load_run(lang: Lang, dir: &Path) -> Result<Run, Failure> {
    let samples = dir.join(format!("{}.jsonl", lang.stem()));
    if !samples.is_file() {
        return Err(format!(
            "no {}: the E2E seat copies the run's samples and machine file into {}",
            samples.display(),
            dir.display()
        ));
    }
    Ok(Run {
        lang,
        dir: dir.to_path_buf(),
        samples: read_samples(&samples).map_err(|why| why.to_string())?,
        machine: read_machine(dir, lang.stem())?,
    })
}

/// A run the tests build by a formula, with the clean machine record F83.
fn fixture_run(lang: Lang, samples: Vec<Sample>) -> Run {
    Run {
        lang,
        dir: PathBuf::from(FIXTURES),
        samples,
        machine: Some(f83_text()),
    }
}

/// The two runs of the main build kept in the repository, measured before the gate.
fn main_runs() -> Result<[Run; 2], Failure> {
    let e2e = Path::new(RESULTS).join("e2e");
    Ok([
        load_run(Lang::Ja, &e2e.join("e2e1"))?,
        load_run(Lang::En, &e2e.join("e2e2"))?,
    ])
}

/// The reports of the runs, the machine gate, and every reason the acceptance fails.
struct Verdict {
    reports: Vec<(Lang, Report)>,
    gate: Result<GateReport, Failure>,
    failures: Vec<String>,
}

/// RP-67 over the runs: the machine gate of RP-83 first, then the line, run by run.
fn judge(runs: &[Run]) -> Result<Verdict, Failure> {
    let inputs: Vec<RunInput> = runs
        .iter()
        .map(|run| RunInput::rust(&run.dir, run.lang.stem(), run.machine.clone()))
        .collect();
    let gate = check_runs(Path::new(REPO), &inputs);
    let mut failures: Vec<String> = gate.as_ref().err().cloned().into_iter().collect();
    let mut reports = Vec::new();
    for run in runs {
        let report = report_of(&run.samples)?;
        failures.extend(p95_failure(run.lang, &report));
        reports.push((run.lang, report));
    }
    Ok(Verdict {
        reports,
        gate,
        failures,
    })
}

impl Verdict {
    fn result(&self) -> Result<(), Failure> {
        if self.failures.is_empty() {
            Ok(())
        } else {
            Err(self.failures.join("; "))
        }
    }

    fn report(&self, lang: Lang) -> Result<&Report, Failure> {
        self.reports
            .iter()
            .find(|(known, _)| *known == lang)
            .map(|(_, report)| report)
            .ok_or_else(|| format!("the verdict holds no report for {}", lang.label()))
    }

    fn render(&self) -> String {
        let mut lines: Vec<String> = Vec::new();
        for (lang, report) in &self.reports {
            lines.extend(report.render(lang.label()));
        }
        lines.push(match &self.gate {
            Ok(gate) => gate.render(),
            Err(why) => format!("RP-83: FAIL: {why}"),
        });
        lines.push(if self.failures.is_empty() {
            String::from("RP-67: PASS")
        } else {
            format!("RP-67: FAIL: {}", self.failures.join("; "))
        });
        lines.join("\n")
    }
}

/// The valid samples of a main run counted at 0.12 s and at 0.30 s must agree,
/// or the `L` verdict of the main build depends on which line is used.
fn check_same_validity(name: &str, samples: &[Sample]) -> Result<usize, Failure> {
    let at_valid_line = valid_at(samples, VALID_MS).len();
    let at_old_line = valid_at(samples, OLD_VALID_MS).len();
    if at_valid_line == at_old_line {
        return Ok(at_valid_line);
    }
    Err(format!(
        "{name}: {at_valid_line} valid samples at 0.12 s but {at_old_line} at 0.30 s: the main run's count does not agree"
    ))
}

// ---- fixtures built by a formula (T 4.65) ----

/// F67: one sample per `L` (ms), each with a wait of 500 ms and a decode of 150 ms.
fn samples_of(l_ms: &[i64]) -> Vec<Sample> {
    (1_u32..)
        .zip(l_ms)
        .map(|(utt, l)| sample(utt, 500, 150, Some(l - 650)))
        .collect()
}

/// `n` values of 900 ms, the last `over` of them replaced by `l_ms`.
fn with_over(n: usize, over: usize, l_ms: i64) -> Vec<i64> {
    let mut values = vec![900; n.saturating_sub(over)];
    values.extend(std::iter::repeat_n(l_ms, over));
    values
}

fn verdict_of(ja: Vec<Sample>, en: Vec<Sample>) -> Result<Verdict, Failure> {
    judge(&[fixture_run(Lang::Ja, ja), fixture_run(Lang::En, en)])
}

/// The failure text of a verdict that must fail; a pass is an error of the test.
fn rejection(verdict: &Verdict) -> Result<String, Failure> {
    match verdict.result() {
        Ok(()) => Err(format!(
            "the run passed, but it must be rejected:\n{}",
            verdict.render()
        )),
        Err(said) => Ok(said),
    }
}

/// F83 with the `load15` of the first row replaced.
fn machine_with_first_load15(load15: &str) -> String {
    let mut rows = f83_rows();
    rows[0]["load15"] = json!(load15);
    text_of(&rows)
}

// ---- the rows ----

#[test]
fn all_samples_at_900_ms_pass_and_the_columns_are_written() -> TestResult {
    let verdict = verdict_of(samples_of(&[900; 93]), samples_of(&[900; 70]))?;
    verdict.result()?;
    let text = verdict.render();
    for needle in [
        "E2E-1 (ja): L p50 900.0 ms, p95 900.0 ms (n 93)",
        "E2E-1 (ja): d p50 500.0 ms, p95 500.0 ms (n 93)",
        "E2E-1 (ja): decode p50 150.0 ms, p95 150.0 ms (n 93)",
        "E2E-1 (ja): translation p50 250.0 ms, p95 250.0 ms (n 93)",
        "E2E-2 (en): L p50 900.0 ms, p95 900.0 ms (n 70)",
        "E2E-2 (en): d p50 500.0 ms, p95 500.0 ms (n 70)",
    ] {
        assert!(text.contains(needle), "{needle}\n{text}");
    }
    Ok(())
}

#[test]
fn l_of_exactly_1600_ms_passes() -> TestResult {
    let verdict = verdict_of(samples_of(&[1600; 93]), samples_of(&[900; 70]))?;
    verdict.result()?;
    let report = verdict.report(Lang::Ja)?;
    assert_eq!((report.over_line, report.over_target), (0, 93));
    let text = verdict.render();
    assert!(
        text.contains("target 1200 ms (recorded, not judged): not met, 93 of 93 samples over"),
        "{text}"
    );
    Ok(())
}

#[test]
fn four_over_in_93_samples_still_pass() -> TestResult {
    let ja = samples_of(&with_over(93, 4, 1601));
    verdict_of(ja, samples_of(&[900; 70]))?.result()?;
    Ok(())
}

#[test]
fn five_over_in_93_samples_are_rejected() -> TestResult {
    let ja = samples_of(&with_over(93, 5, 1601));
    let said = rejection(&verdict_of(ja, samples_of(&[900; 70]))?)?;
    assert!(said.contains("E2E-1 (ja): L p95 1601.0 ms"), "{said}");
    assert!(said.contains("5 of 93 samples are over"), "{said}");
    Ok(())
}

#[test]
fn three_over_in_70_samples_still_pass() -> TestResult {
    let en = samples_of(&with_over(70, 3, 1601));
    verdict_of(samples_of(&[900; 93]), en)?.result()?;
    Ok(())
}

#[test]
fn four_over_in_70_samples_are_rejected() -> TestResult {
    let en = samples_of(&with_over(70, 4, 1601));
    let said = rejection(&verdict_of(samples_of(&[900; 93]), en)?)?;
    assert!(said.contains("E2E-2 (en): L p95 1601.0 ms"), "{said}");
    assert!(said.contains("4 of 70 samples are over"), "{said}");
    Ok(())
}

/// (n, samples that may be over 1600 ms): the rank is (95 * n + 99) / 100 and
/// the allowed number is n minus the rank.
const ALLOWED_JA: [(usize, usize); 5] = [(92, 4), (93, 4), (94, 4), (95, 4), (96, 4)];
const ALLOWED_EN: [(usize, usize); 3] = [(69, 3), (70, 3), (71, 3)];

#[test]
fn the_allowed_count_follows_n() -> TestResult {
    for (n, allowed) in ALLOWED_JA {
        let edge = samples_of(&with_over(n, allowed, 1601));
        verdict_of(edge, samples_of(&[900; 70]))?.result()?;
        let past = samples_of(&with_over(n, allowed + 1, 1601));
        let said = rejection(&verdict_of(past, samples_of(&[900; 70]))?)?;
        assert!(said.contains("E2E-1 (ja)"), "n {n}: {said}");
    }
    for (n, allowed) in ALLOWED_EN {
        let edge = samples_of(&with_over(n, allowed, 1601));
        verdict_of(samples_of(&[900; 93]), edge)?.result()?;
        let past = samples_of(&with_over(n, allowed + 1, 1601));
        let said = rejection(&verdict_of(samples_of(&[900; 93]), past)?)?;
        assert!(said.contains("E2E-2 (en)"), "n {n}: {said}");
    }
    Ok(())
}

/// The shape of the recorded en run: three over the line (4681, 1658 and 1603
/// ms), nine between 1200 and 1600 ms, the rest at 900 ms. `fourth` replaces
/// the last of the nine.
fn recorded_en_shape(fourth: Option<i64>) -> Vec<i64> {
    let mut values = vec![900; 58];
    values.extend([4681, 1658, 1603]);
    values.extend((0..9).map(|step| 1201 + step * 44));
    if let Some(l_ms) = fourth {
        values.pop();
        values.push(l_ms);
    }
    values
}

#[test]
fn a_run_shaped_like_the_recorded_en_run_passes_the_1600_ms_line() -> TestResult {
    let verdict = verdict_of(samples_of(&[900; 93]), samples_of(&recorded_en_shape(None)))?;
    verdict.result()?;
    let report = verdict.report(Lang::En)?;
    assert_eq!((report.over_line, report.over_target), (3, 12));
    Ok(())
}

#[test]
fn a_fourth_sample_over_1600_ms_is_rejected_in_70_samples() -> TestResult {
    let en = samples_of(&recorded_en_shape(Some(1601)));
    let said = rejection(&verdict_of(samples_of(&[900; 93]), en)?)?;
    assert!(said.contains("E2E-2 (en): L p95 1601.0 ms"), "{said}");
    Ok(())
}

/// F67 samples with `untranslated` valid samples that never got a translation.
fn with_untranslated(l_ms: &[i64], untranslated: u32) -> Vec<Sample> {
    let mut samples = samples_of(l_ms);
    samples.extend((1000..1000 + untranslated).map(|utt| sample(utt, 500, 150, None)));
    samples
}

#[test]
fn samples_without_a_translation_are_not_counted_as_fast() -> TestResult {
    let ja = with_untranslated(&with_over(93, 5, 1601), 7);
    let verdict = verdict_of(ja, samples_of(&[900; 70]))?;
    let said = rejection(&verdict)?;
    assert!(said.contains("E2E-1 (ja): L p95 1601.0 ms"), "{said}");
    let report = verdict.report(Lang::Ja)?;
    assert_eq!((report.valid, report.translated), (100, 93));
    Ok(())
}

#[test]
fn a_run_with_no_translated_sample_is_not_a_pass() -> TestResult {
    for ja in [
        (1000..1005)
            .map(|utt| sample(utt, 500, 150, None))
            .collect(),
        Vec::new(),
    ] {
        let said = rejection(&verdict_of(ja, samples_of(&[900; 70]))?)?;
        assert!(
            said.contains("E2E-1 (ja): no valid sample has a translation"),
            "{said}"
        );
        assert!(!said.contains("E2E-2"), "{said}");
    }
    Ok(())
}

#[test]
fn a_failing_ja_run_is_named_alone() -> TestResult {
    let verdict = verdict_of(samples_of(&[1601; 93]), samples_of(&[900; 70]))?;
    let said = rejection(&verdict)?;
    assert!(said.contains("E2E-1 (ja)"), "{said}");
    assert!(!said.contains("E2E-2") && !said.contains("e2e2"), "{said}");
    Ok(())
}

#[test]
fn a_failing_en_run_is_named_alone() -> TestResult {
    let verdict = verdict_of(samples_of(&[900; 93]), samples_of(&[1601; 70]))?;
    let said = rejection(&verdict)?;
    assert!(said.contains("E2E-2 (en)"), "{said}");
    assert!(!said.contains("E2E-1") && !said.contains("e2e1"), "{said}");
    Ok(())
}

#[test]
fn the_recorded_main_ja_run_is_over_1600_ms_and_the_en_run_is_inside() -> TestResult {
    let verdict = judge(&main_runs()?)?;
    let text = verdict.render();
    assert_eq!(verdict.failures.len(), 1, "{text}");
    let said = rejection(&verdict)?;
    assert!(said.contains("E2E-1 (ja): L p95 1871.7 ms"), "{said}");
    assert!(!said.contains("E2E-2"), "{said}");
    for needle in [
        "E2E-1 (ja): L p50 1415.2 ms, p95 1871.7 ms (n 93)",
        "E2E-1 (ja): d p50 881.5 ms, p95 1355.1 ms",
        "E2E-1 (ja): target 1200 ms (recorded, not judged): not met, 65 of 93 samples over",
        "E2E-2 (en): L p50 970.3 ms, p95 1303.2 ms (n 70)",
        "E2E-2 (en): d p50 443.6 ms, p95 671.9 ms",
        "E2E-2 (en): target 1200 ms (recorded, not judged): not met, 12 of 70 samples over",
        "RP-83: pre-gate records",
    ] {
        assert!(text.contains(needle), "{needle}\n{text}");
    }
    let (ja, en) = (verdict.report(Lang::Ja)?, verdict.report(Lang::En)?);
    assert_eq!((ja.over_line, en.over_line), (16, 3));
    Ok(())
}

#[test]
fn main_runs_count_the_same_valid_samples_at_120_and_300_ms_for_the_l_verdict() -> TestResult {
    let [mut ja, en] = main_runs()?;
    assert_eq!(check_same_validity("e2e1", &ja.samples)?, 100);
    assert_eq!(check_same_validity("e2e2", &en.samples)?, 75);
    // A wait of 250 ms is valid at 0.12 s and not at 0.30 s.
    let Some(sample) = ja.samples.iter_mut().find(|s| s.t_final_us.is_some()) else {
        return Err("the ja record holds no sample with a final".into());
    };
    let Some(l_asr_us) = sample.l_asr_us() else {
        return Err("the sample has no L_asr".into());
    };
    sample.final_latency_ms = Some(l_asr_us as f64 / 1000.0 - 250.0);
    let said = check_same_validity("e2e1", &ja.samples).err();
    assert!(
        said.as_deref().is_some_and(|text| text.contains("e2e1")
            && text.contains("100 valid samples at 0.12 s but 99 at 0.30 s")),
        "{said:?}"
    );
    Ok(())
}

#[test]
fn a_slow_sample_valid_only_at_120_ms_counts_and_both_p95_values_are_written() -> TestResult {
    let mut ja = samples_of(&with_over(92, 4, 1601));
    // A wait of 250 ms and an `L` of 1900 ms: valid at 0.12 s, not at 0.30 s.
    ja.push(sample(2000, 250, 150, Some(1500)));
    let verdict = verdict_of(ja, samples_of(&[900; 70]))?;
    let said = rejection(&verdict)?;
    assert!(said.contains("E2E-1 (ja): L p95 1601.0 ms"), "{said}");
    let report = verdict.report(Lang::Ja)?;
    assert_eq!(report.only_at_valid_line_us, [1_900_000]);
    let at_valid_line = report.l.map(|l| (l.n, l.p95_us));
    let at_old_line = report.l_at_old_line.map(|l| (l.n, l.p95_us));
    assert_eq!(at_valid_line, Some((93, 1_601_000)));
    assert_eq!(at_old_line, Some((92, 900_000)));
    let text = verdict.render();
    assert!(
        text.contains(
            "valid 92, L p50 900.0 ms, p95 900.0 ms (n 92); valid only at 0.12 s: 1900.0 ms"
        ),
        "{text}"
    );
    Ok(())
}

#[test]
fn valid_samples_without_a_translation_are_counted_apart() -> TestResult {
    let ja = with_untranslated(&[900; 93], 7);
    let verdict = verdict_of(ja, samples_of(&[900; 70]))?;
    verdict.result()?;
    let report = verdict.report(Lang::Ja)?;
    assert_eq!(report.valid - report.translated, 7);
    assert_eq!(report.l.map(|l| l.n), Some(93));
    let text = verdict.render();
    assert!(
        text.contains("valid 100, translated 93, valid without a translation 7"),
        "{text}"
    );
    Ok(())
}

#[test]
fn a_rust_run_with_a_first_row_load15_of_2_50_is_rejected_by_the_gate() -> TestResult {
    let runs = |load15: &str| {
        let mut ja = fixture_run(Lang::Ja, samples_of(&[900; 93]));
        ja.machine = Some(machine_with_first_load15(load15));
        [ja, fixture_run(Lang::En, samples_of(&[900; 70]))]
    };
    // The inside of the line is the positive control.
    judge(&runs("2.49"))?.result()?;
    let said = rejection(&judge(&runs("2.50"))?)?;
    for needle in ["e2e1", "load15", "2.50"] {
        assert!(said.contains(needle), "{needle}: {said}");
    }
    assert!(!said.contains("e2e2") && !said.contains("E2E-2"), "{said}");
    Ok(())
}

#[test]
#[ignore = "reads the records in docs/results/rp67/"]
fn rp67_speech_end_to_translation_p95_is_within_1600_ms() -> TestResult {
    let dir = Path::new(RESULTS).join("rp67");
    let runs = [load_run(Lang::Ja, &dir)?, load_run(Lang::En, &dir)?];
    let verdict = judge(&runs)?;
    println!("{}", verdict.render());
    assert!(verdict.failures.is_empty(), "{}", verdict.render());
    Ok(())
}
