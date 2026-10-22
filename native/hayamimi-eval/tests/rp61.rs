//! RP-61: the refine pass added to the live run does not delay the subtitle.
//!
//! The run is taken three ways on the same load: the refiner on (`refine`),
//! the same build told `--no-refine` (`norefine`), and Python with its refiner
//! left at the default (`python`). Each arm has an E2E-1 (ja to en, `e2e1`) run
//! and an E2E-2 (en to ja, `e2e2`) run, and the two languages are judged apart.
//! A language passes when:
//!
//! - `L` of the refine run is at most `L` of the Python run at p50 and at p95;
//! - `L` of the refine run is at most 31 ms over `L` of the no-refine run at
//!   p50 and at most 100 ms over it at p95 (the band is fixed beforehand and
//!   is never widened after a run is seen);
//! - the refine run got a translation for at least 20 of every 23 valid
//!   samples;
//! - the refine run holds at least one `refine` event, and for E2E-1 at least
//!   one `refine` event whose `lang` is `ja` holds a punctuation mark (`、`,
//!   `。` or `？`), which shows the marks reached the refined text.
//!
//! `L` is counted as in RP-67: a sample is valid when `L_asr - L_dec` is at
//! least 0.12 s, `L` is taken over the valid samples that got a translation,
//! and the percentile is the nearest rank on microseconds, compared to its
//! limit without rounding. Two runs of the same arm give a gap of 0.
//!
//! Every Rust run (`refine` and `norefine`) goes through the machine gate of
//! RP-83 (`support::machine::check_runs`); a Python run is never judged by it.
//!
//! The live row reads `docs/results/rp61/`. Each stem `<arm>.<run>` (`refine`,
//! `norefine`, `python`, then `e2e1`, `e2e2`) has `<stem>.jsonl` (the samples),
//! `<stem>.machine.jsonl` (the machine record) and `<stem>.events.jsonl` (the
//! events received; the two `refine` stems are read for their `refine` events).
//! It is run by hand:
//!
//!     cargo test --release -p hayamimi-eval --test rp61 -- --ignored --nocapture
//!
//! Every other row runs on fixtures built by a formula, and on a run recorded
//! in `docs/results/r8x/` to show the files are read as they are written.

mod support;

use std::path::{Path, PathBuf};

use hayamimi_eval::compare::{is_valid_l_asr, read_samples, translated_share};
use hayamimi_eval::latency::Sample;
use hayamimi_eval::machine::FILE_SUFFIX;
use hayamimi_eval::percentile::nearest_rank;
use serde_json::{Value, json};
use support::machine::{GateReport, RunInput, check_runs, f83_rows, f83_text, text_of};
use support::{FIXTURES, Failure, REPO, RESULTS, TestResult};

/// The silence a sample must have waited for to be valid, in milliseconds.
const VALID_MS: i64 = 120;
/// How far the refine run's `L` p50 may stand over the no-refine run's, in microseconds.
const P50_BAND_US: i64 = 31_000;
/// How far the refine run's `L` p95 may stand over the no-refine run's, in microseconds.
const P95_BAND_US: i64 = 100_000;
/// The marks a refined sentence of a ja run carries after the punctuation pass.
const MARKS: [char; 3] = ['、', '。', '？'];

/// The two runs of an arm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Lang {
    Ja,
    En,
}

impl Lang {
    const ALL: [Lang; 2] = [Lang::Ja, Lang::En];

    /// The run's part of a record stem (`<arm>.<this>`).
    fn stem(self) -> &'static str {
        match self {
            Lang::Ja => "e2e1",
            Lang::En => "e2e2",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Lang::Ja => "E2E-1 (ja)",
            Lang::En => "E2E-2 (en)",
        }
    }

    /// The language the run's speech is in, as a `refine` event names it.
    fn spoken(self) -> &'static str {
        match self {
            Lang::Ja => "ja",
            Lang::En => "en",
        }
    }
}

/// The three ways the run is taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Arm {
    Refine,
    NoRefine,
    Python,
}

impl Arm {
    const ALL: [Arm; 3] = [Arm::Refine, Arm::NoRefine, Arm::Python];

    /// The arm's part of a record stem (`<this>.<run>`).
    fn name(self) -> &'static str {
        match self {
            Arm::Refine => "refine",
            Arm::NoRefine => "norefine",
            Arm::Python => "python",
        }
    }
}

fn stem_of(arm: Arm, lang: Lang) -> String {
    format!("{}.{}", arm.name(), lang.stem())
}

/// Every (arm, run) pair of the acceptance, arm by arm.
fn pairs() -> impl Iterator<Item = (Arm, Lang)> {
    Arm::ALL
        .into_iter()
        .flat_map(|arm| Lang::ALL.into_iter().map(move |lang| (arm, lang)))
}

/// One run: where its files stand, its samples, and the text of the files beside them.
struct Run {
    arm: Arm,
    lang: Lang,
    dir: PathBuf,
    stem: String,
    samples: Vec<Sample>,
    machine: Option<String>,
    events: Option<String>,
}

impl Run {
    fn label(&self) -> String {
        format!("{} {}", self.lang.label(), self.arm.name())
    }

    /// The run as the machine gate takes it: a Python run is not judged.
    fn gate_input(&self) -> RunInput {
        match self.arm {
            Arm::Python => RunInput::python(&self.dir, &self.stem, self.machine.clone()),
            Arm::Refine | Arm::NoRefine => {
                RunInput::rust(&self.dir, &self.stem, self.machine.clone())
            }
        }
    }
}

// ---- reading the records ----

/// The text of `file` in `dir`; `None` when the file does not stand.
fn read_optional(dir: &Path, file: &str) -> Result<Option<String>, Failure> {
    let path = dir.join(file);
    if !path.is_file() {
        return Ok(None);
    }
    std::fs::read_to_string(&path)
        .map(Some)
        .map_err(|why| format!("{}: {why}", path.display()))
}

/// The run whose files are named `<stem>.*` in `dir`.
fn load_stem(dir: &Path, stem: &str, arm: Arm, lang: Lang) -> Result<Run, Failure> {
    let samples =
        read_samples(&dir.join(format!("{stem}.jsonl"))).map_err(|why| why.to_string())?;
    Ok(Run {
        arm,
        lang,
        dir: dir.to_path_buf(),
        stem: stem.to_owned(),
        samples,
        machine: read_optional(dir, &format!("{stem}.{FILE_SUFFIX}"))?,
        events: read_optional(dir, &format!("{stem}.events.jsonl"))?,
    })
}

/// The sample files of the six runs that do not stand in `dir`.
fn missing_samples(dir: &Path) -> Vec<String> {
    pairs()
        .map(|(arm, lang)| format!("{}.jsonl", stem_of(arm, lang)))
        .filter(|file| !dir.join(file).is_file())
        .collect()
}

/// The six runs of `dir`. A run with no sample file is not a pass: it is named.
fn load_runs(dir: &Path) -> Result<Vec<Run>, Failure> {
    let missing = missing_samples(dir);
    if !missing.is_empty() {
        return Err(format!(
            "no record: {} of the 6 runs have no sample file in {}: {}; the E2E window copies each run's samples, machine and events files there",
            missing.len(),
            dir.display(),
            missing.join(", ")
        ));
    }
    pairs()
        .map(|(arm, lang)| load_stem(dir, &stem_of(arm, lang), arm, lang))
        .collect()
}

/// The run of `arm` and `lang` among `runs`.
fn run_of(runs: &[Run], arm: Arm, lang: Lang) -> Result<&Run, Failure> {
    runs.iter()
        .find(|run| run.arm == arm && run.lang == lang)
        .ok_or_else(|| format!("no {} run of {}", arm.name(), lang.label()))
}

// ---- the numbers ----

/// `p50` and `p95` of `L` over `n` samples, in microseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Column {
    n: usize,
    p50_us: i64,
    p95_us: i64,
}

/// `L` of the valid samples that got a translation, in microseconds.
fn translated_l_us(samples: &[Sample]) -> Vec<i64> {
    samples
        .iter()
        .filter(|sample| is_valid_l_asr(sample, VALID_MS))
        .filter_map(Sample::l_us)
        .collect()
}

/// `None` when no valid sample got a translation.
fn column(samples: &[Sample]) -> Result<Option<Column>, Failure> {
    let values = translated_l_us(samples);
    if values.is_empty() {
        return Ok(None);
    }
    let pick = |p| nearest_rank(&values, p).map_err(|why| why.to_string());
    Ok(Some(Column {
        n: values.len(),
        p50_us: pick(50)?,
        p95_us: pick(95)?,
    }))
}

/// How far the refine run stands over another run, in microseconds (negative: under).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Gap {
    p50_us: i64,
    p95_us: i64,
}

fn gap_between(refine: Column, other: Column) -> Gap {
    Gap {
        p50_us: refine.p50_us - other.p50_us,
        p95_us: refine.p95_us - other.p95_us,
    }
}

/// Microseconds as milliseconds with one decimal.
fn ms1(us: i64) -> String {
    format!("{:.1}", us as f64 / 1000.0)
}

fn signed_ms1(us: i64) -> String {
    format!("{:+.1}", us as f64 / 1000.0)
}

fn show(column: Column) -> String {
    format!(
        "p50 {} ms, p95 {} ms (n {})",
        ms1(column.p50_us),
        ms1(column.p95_us),
        column.n
    )
}

fn column_lines(label: &str, columns: [Column; 3], gap: Gap) -> Vec<String> {
    let [refine, no_refine, python] = columns;
    vec![
        format!("{label}: refine L {}", show(refine)),
        format!("{label}: no-refine L {}", show(no_refine)),
        format!("{label}: python L {}", show(python)),
        format!(
            "{label}: gap to no-refine p50 {} ms (band {} ms), p95 {} ms (band {} ms)",
            signed_ms1(gap.p50_us),
            P50_BAND_US / 1000,
            signed_ms1(gap.p95_us),
            P95_BAND_US / 1000
        ),
    ]
}

/// The percentiles of the refine run that stand over Python's.
fn python_failures(label: &str, refine: Column, python: Column) -> Vec<String> {
    [
        ("p50", refine.p50_us, python.p50_us),
        ("p95", refine.p95_us, python.p95_us),
    ]
    .into_iter()
    .filter(|(_, ours, theirs)| ours > theirs)
    .map(|(name, ours, theirs)| {
        format!(
            "{label}: L {name} {} ms with the refiner is over python {} ms",
            ms1(ours),
            ms1(theirs)
        )
    })
    .collect()
}

/// The percentiles of the refine run that stand over the no-refine run's by more than the band.
fn band_failures(label: &str, gap: Gap) -> Vec<String> {
    [
        ("p50", gap.p50_us, P50_BAND_US),
        ("p95", gap.p95_us, P95_BAND_US),
    ]
    .into_iter()
    .filter(|(_, over, band)| over > band)
    .map(|(name, over, band)| {
        format!(
            "{label}: L {name} with the refiner is {} ms over the run with --no-refine, the band is {} ms",
            ms1(over),
            band / 1000
        )
    })
    .collect()
}

// ---- the refine events ----

/// A `refine` event of an events file: the language it names and its text.
struct RefineEvent {
    lang: String,
    text: String,
}

fn text_at(value: &Value, pointer: &str, number: usize) -> Result<String, Failure> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("events line {number}: a refine event has no {pointer}"))
}

/// The `refine` events of the text of `<stem>.events.jsonl`, in the order they stand.
fn refine_events(text: &str) -> Result<Vec<RefineEvent>, Failure> {
    let mut found = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let number = index + 1;
        let value: Value = serde_json::from_str(line)
            .map_err(|why| format!("events line {number} is not JSON: {why}"))?;
        if value.pointer("/event/type").and_then(Value::as_str) != Some("refine") {
            continue;
        }
        found.push(RefineEvent {
            lang: text_at(&value, "/event/lang", number)?,
            text: text_at(&value, "/event/text", number)?,
        });
    }
    Ok(found)
}

/// How many `refine` events name `lang`, and how many of those hold a mark.
fn marked_count(events: &[RefineEvent], lang: &str) -> (usize, usize) {
    let named = events.iter().filter(|event| event.lang == lang);
    (
        named.clone().count(),
        named.filter(|event| event.text.contains(MARKS)).count(),
    )
}

// ---- the verdict ----

/// What one language came to: the lines written, every reason it fails, and the gap.
#[derive(Default)]
struct LangVerdict {
    lines: Vec<String>,
    failures: Vec<String>,
    gap: Option<Gap>,
}

impl LangVerdict {
    fn absorb(&mut self, other: LangVerdict) {
        self.lines.extend(other.lines);
        self.failures.extend(other.failures);
    }
}

/// The column of a run, or the failure that none of its valid samples got a translation.
fn column_of(run: &Run, verdict: &mut LangVerdict) -> Result<Option<Column>, Failure> {
    let found = column(&run.samples)?;
    if found.is_none() {
        verdict.failures.push(format!(
            "{}: no valid sample has a translation",
            run.label()
        ));
    }
    Ok(found)
}

/// The refine run got a translation for at least 20 of every 23 valid samples.
fn share_check(refine: &Run) -> LangVerdict {
    let label = refine.lang.label();
    let share = translated_share(&refine.samples, VALID_MS);
    let mut verdict = LangVerdict::default();
    verdict.lines.push(format!(
        "{label}: translated {} of {} valid samples (20 / 23 needed)",
        share.translated, share.valid
    ));
    if !share.is_enough() {
        verdict.failures.push(format!(
            "{label}: translated {} of {} valid samples, 20 / 23 are needed",
            share.translated, share.valid
        ));
    }
    verdict
}

/// An E2E-1 run needs a `ja` refine event that holds a mark.
fn mark_check(label: &str, events: &[RefineEvent], spoken: &str) -> LangVerdict {
    let (named, marked) = marked_count(events, spoken);
    let mut verdict = LangVerdict::default();
    verdict.lines.push(format!(
        "{label}: {marked} of {named} {spoken} refine events hold a mark"
    ));
    if marked == 0 {
        verdict.failures.push(format!(
            "{label}: {marked} of {named} {spoken} refine events hold a mark ({}), one is needed",
            MARKS.iter().collect::<String>()
        ));
    }
    verdict
}

/// The refine run holds a `refine` event, and an E2E-1 run holds marks in them.
fn events_check(refine: &Run) -> Result<LangVerdict, Failure> {
    let label = refine.lang.label();
    let mut verdict = LangVerdict::default();
    let Some(text) = refine.events.as_deref() else {
        verdict.failures.push(format!(
            "{label}: no record of the refine events, {}.events.jsonl is missing",
            refine.stem
        ));
        return Ok(verdict);
    };
    let events = refine_events(text)?;
    verdict
        .lines
        .push(format!("{label}: refine events {}", events.len()));
    if events.is_empty() {
        verdict
            .failures
            .push(format!("{label}: the run holds no refine event"));
        return Ok(verdict);
    }
    if refine.lang == Lang::Ja {
        verdict.absorb(mark_check(label, &events, refine.lang.spoken()));
    }
    Ok(verdict)
}

/// One language: the three runs' columns, the gaps, the share and the events.
fn judge_lang(refine: &Run, no_refine: &Run, python: &Run) -> Result<LangVerdict, Failure> {
    let mut verdict = LangVerdict::default();
    let label = refine.lang.label();
    let ours = column_of(refine, &mut verdict)?;
    let base = column_of(no_refine, &mut verdict)?;
    let theirs = column_of(python, &mut verdict)?;
    if let (Some(ours), Some(base), Some(theirs)) = (ours, base, theirs) {
        let gap = gap_between(ours, base);
        verdict
            .lines
            .extend(column_lines(label, [ours, base, theirs], gap));
        verdict
            .failures
            .extend(python_failures(label, ours, theirs));
        verdict.failures.extend(band_failures(label, gap));
        verdict.gap = Some(gap);
    }
    verdict.absorb(share_check(refine));
    verdict.absorb(events_check(refine)?);
    Ok(verdict)
}

/// The machine gate of RP-83 and the verdict of each language.
struct Verdict {
    langs: Vec<(Lang, LangVerdict)>,
    gate: Result<GateReport, Failure>,
}

/// RP-61 over the six runs: the machine gate first, then each language.
fn judge(runs: &[Run]) -> Result<Verdict, Failure> {
    let inputs: Vec<RunInput> = runs.iter().map(Run::gate_input).collect();
    let gate = check_runs(Path::new(REPO), &inputs);
    let mut langs = Vec::new();
    for lang in Lang::ALL {
        let verdict = judge_lang(
            run_of(runs, Arm::Refine, lang)?,
            run_of(runs, Arm::NoRefine, lang)?,
            run_of(runs, Arm::Python, lang)?,
        )?;
        langs.push((lang, verdict));
    }
    Ok(Verdict { langs, gate })
}

impl Verdict {
    fn failures(&self) -> Vec<String> {
        let mut found: Vec<String> = self.gate.as_ref().err().cloned().into_iter().collect();
        for (_, lang) in &self.langs {
            found.extend(lang.failures.iter().cloned());
        }
        found
    }

    fn result(&self) -> Result<(), Failure> {
        let failures = self.failures();
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("; "))
        }
    }

    fn lang(&self, wanted: Lang) -> Result<&LangVerdict, Failure> {
        self.langs
            .iter()
            .find(|(lang, _)| *lang == wanted)
            .map(|(_, verdict)| verdict)
            .ok_or_else(|| format!("the verdict holds no {}", wanted.label()))
    }

    fn render(&self) -> String {
        let mut lines: Vec<String> = Vec::new();
        for (_, lang) in &self.langs {
            lines.extend(lang.lines.iter().cloned());
        }
        lines.push(match &self.gate {
            Ok(gate) => gate.render(),
            Err(why) => format!("RP-83: FAIL: {why}"),
        });
        let failures = self.failures();
        lines.push(if failures.is_empty() {
            String::from("RP-61: PASS")
        } else {
            format!("RP-61: FAIL: {}", failures.join("; "))
        });
        lines.join("\n")
    }
}

// ---- fixtures built by a formula ----

/// Valid samples of every arm.
const N: usize = 93;

/// One sample: the speech ends at 1 s, the `final` comes `wait_ms + decode_ms`
/// later, and the translation `translation_ms` after the `final` (none: no translation).
fn sample(utt: u32, wait_ms: i64, decode_ms: i64, translation_ms: Option<i64>) -> Sample {
    let t_end_us = 1_000_000;
    let t_final_us = t_end_us + (wait_ms + decode_ms) * 1000;
    Sample {
        repeat: 1,
        utt,
        t_start_us: 0,
        t_end_us,
        t_partial_us: None,
        t_partial_tr_us: None,
        t_final_us: Some(t_final_us),
        t_tr_us: translation_ms.map(|ms| t_final_us + ms * 1000),
        finals: 1,
        text: String::new(),
        translation: None,
        final_latency_ms: Some(decode_ms as f64),
        cer_dist: 0,
        cer_denom: 0,
    }
}

/// One sample per `L` (ms), each with a wait of 500 ms and a decode of 150 ms.
fn samples_of(l_ms: &[i64]) -> Vec<Sample> {
    (1_u32..)
        .zip(l_ms)
        .map(|(utt, l)| sample(utt, 500, 150, Some(l - 650)))
        .collect()
}

/// `n` values of `base_ms`, the last `raised` of them raised by `extra_ms`.
fn tail(n: usize, base_ms: i64, raised: usize, extra_ms: i64) -> Vec<i64> {
    let mut values = vec![base_ms; n.saturating_sub(raised)];
    values.extend(std::iter::repeat_n(base_ms + extra_ms, raised));
    values
}

fn event_line(spoken: &str, text: &str) -> String {
    json!({
        "event": {
            "type": "refine",
            "id": "mic-1",
            "source": "mic",
            "t0_s": 1.5,
            "t1_s": 4.0,
            "end_wall_ms": 1_790_000_000_500_u64,
            "text": text,
            "lang": spoken,
            "speaker": "S1",
            "audio_s": 2.5,
        },
        "t_us": 9_000_000,
    })
    .to_string()
}

fn partial_line() -> String {
    json!({"event": {"type": "partial", "text": "予算は"}, "t_us": 2_000_000}).to_string()
}

fn events_text(lines: &[String]) -> String {
    lines.iter().map(|line| format!("{line}\n")).collect()
}

/// An events file with `marked` refine sentences that hold a mark and `plain` that do not.
fn events_of(lang: Lang, marked: usize, plain: usize) -> String {
    let (with, without) = match lang {
        Lang::Ja => ("予算は五百万円です。", "予算は五百万円です"),
        Lang::En => (
            "The budget is five million yen.",
            "The budget is five million yen",
        ),
    };
    let mut lines = vec![partial_line()];
    lines.extend(std::iter::repeat_n(event_line(lang.spoken(), with), marked));
    lines.extend(std::iter::repeat_n(
        event_line(lang.spoken(), without),
        plain,
    ));
    events_text(&lines)
}

/// `L` (ms) of the three arms of one language, the refine run's valid samples that
/// got no translation, and its events file (`None`: the file does not stand).
struct Lengths {
    refine: Vec<i64>,
    no_refine: Vec<i64>,
    python: Vec<i64>,
    untranslated: u32,
    events: Option<String>,
}

/// Every arm at 900 ms (Python well over), and three marked refine sentences.
fn calm(lang: Lang) -> Lengths {
    Lengths {
        refine: vec![900; N],
        no_refine: vec![900; N],
        python: vec![1200; N],
        untranslated: 0,
        events: Some(events_of(lang, 3, 0)),
    }
}

fn fixture_run(arm: Arm, lang: Lang, samples: Vec<Sample>, events: Option<String>) -> Run {
    Run {
        arm,
        lang,
        dir: PathBuf::from(FIXTURES),
        stem: stem_of(arm, lang),
        samples,
        machine: Some(f83_text()),
        events,
    }
}

fn fixture_runs(lang: Lang, lengths: &Lengths) -> Vec<Run> {
    let mut refine = samples_of(&lengths.refine);
    refine.extend((1000..1000 + lengths.untranslated).map(|utt| sample(utt, 500, 150, None)));
    vec![
        fixture_run(Arm::Refine, lang, refine, lengths.events.clone()),
        fixture_run(Arm::NoRefine, lang, samples_of(&lengths.no_refine), None),
        fixture_run(Arm::Python, lang, samples_of(&lengths.python), None),
    ]
}

fn fixture_set(ja: &Lengths, en: &Lengths) -> Vec<Run> {
    let mut runs = fixture_runs(Lang::Ja, ja);
    runs.extend(fixture_runs(Lang::En, en));
    runs
}

fn verdict_of(ja: &Lengths, en: &Lengths) -> Result<Verdict, Failure> {
    judge(&fixture_set(ja, en))
}

/// The ja run as given and the en run calm.
fn ja_verdict(ja: Lengths) -> Result<Verdict, Failure> {
    verdict_of(&ja, &calm(Lang::En))
}

/// The en run as given and the ja run calm.
fn en_verdict(en: Lengths) -> Result<Verdict, Failure> {
    verdict_of(&calm(Lang::Ja), &en)
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

/// F83 with the `load15` of its first row replaced.
fn machine_with_first_load15(load15: &str) -> String {
    let mut rows = f83_rows();
    rows[0]["load15"] = json!(load15);
    text_of(&rows)
}

/// The machine record of one run of `runs` replaced.
fn set_machine(runs: &mut [Run], arm: Arm, lang: Lang, machine: String) -> Result<(), Failure> {
    let found = runs
        .iter_mut()
        .find(|run| run.arm == arm && run.lang == lang)
        .ok_or_else(|| format!("no {} run of {}", arm.name(), lang.label()))?;
    found.machine = Some(machine);
    Ok(())
}

// ---- the rows ----

#[test]
fn calm_runs_pass_and_the_columns_are_written() -> TestResult {
    let verdict = verdict_of(&calm(Lang::Ja), &calm(Lang::En))?;
    verdict.result()?;
    let text = verdict.render();
    for needle in [
        "E2E-1 (ja): refine L p50 900.0 ms, p95 900.0 ms (n 93)",
        "E2E-1 (ja): no-refine L p50 900.0 ms, p95 900.0 ms (n 93)",
        "E2E-1 (ja): python L p50 1200.0 ms, p95 1200.0 ms (n 93)",
        "E2E-1 (ja): gap to no-refine p50 +0.0 ms (band 31 ms), p95 +0.0 ms (band 100 ms)",
        "E2E-1 (ja): translated 93 of 93 valid samples (20 / 23 needed)",
        "E2E-1 (ja): refine events 3",
        "E2E-1 (ja): 3 of 3 ja refine events hold a mark",
        "E2E-2 (en): refine L p50 900.0 ms, p95 900.0 ms (n 93)",
        "E2E-2 (en): refine events 3",
        "RP-61: PASS",
    ] {
        assert!(text.contains(needle), "{needle}\n{text}");
    }
    Ok(())
}

#[test]
fn a_gap_of_exactly_31_ms_at_p50_passes() -> TestResult {
    let ja = Lengths {
        refine: vec![931; N],
        ..calm(Lang::Ja)
    };
    let verdict = ja_verdict(ja)?;
    verdict.result()?;
    assert_eq!(
        verdict.lang(Lang::Ja)?.gap,
        Some(Gap {
            p50_us: 31_000,
            p95_us: 31_000
        })
    );
    Ok(())
}

#[test]
fn a_gap_of_exactly_100_ms_at_p95_passes() -> TestResult {
    let ja = Lengths {
        refine: tail(N, 900, 10, 100),
        ..calm(Lang::Ja)
    };
    let verdict = ja_verdict(ja)?;
    verdict.result()?;
    assert_eq!(
        verdict.lang(Lang::Ja)?.gap,
        Some(Gap {
            p50_us: 0,
            p95_us: 100_000
        })
    );
    Ok(())
}

#[test]
fn a_gap_over_31_ms_at_p50_is_rejected_and_p95_is_not_blamed() -> TestResult {
    let ja = Lengths {
        refine: vec![932; N],
        ..calm(Lang::Ja)
    };
    let said = rejection(&ja_verdict(ja)?)?;
    assert!(
        said.contains("E2E-1 (ja): L p50 with the refiner is 32.0 ms over the run with --no-refine, the band is 31 ms"),
        "{said}"
    );
    assert!(!said.contains("L p95"), "{said}");
    Ok(())
}

#[test]
fn a_gap_over_100_ms_at_p95_is_rejected_and_p50_is_not_blamed() -> TestResult {
    let ja = Lengths {
        refine: tail(N, 900, 10, 101),
        ..calm(Lang::Ja)
    };
    let said = rejection(&ja_verdict(ja)?)?;
    assert!(
        said.contains("E2E-1 (ja): L p95 with the refiner is 101.0 ms over the run with --no-refine, the band is 100 ms"),
        "{said}"
    );
    assert!(!said.contains("L p50"), "{said}");
    Ok(())
}

#[test]
fn a_refine_run_faster_than_the_no_refine_run_passes() -> TestResult {
    let ja = Lengths {
        refine: vec![800; N],
        ..calm(Lang::Ja)
    };
    let verdict = ja_verdict(ja)?;
    verdict.result()?;
    let gap = verdict.lang(Lang::Ja)?.gap;
    assert_eq!(
        gap,
        Some(Gap {
            p50_us: -100_000,
            p95_us: -100_000
        })
    );
    Ok(())
}

#[test]
fn a_refine_run_slower_than_python_is_rejected_at_both_percentiles() -> TestResult {
    let ja = Lengths {
        refine: vec![931; N],
        python: vec![900; N],
        ..calm(Lang::Ja)
    };
    let said = rejection(&ja_verdict(ja)?)?;
    for needle in [
        "E2E-1 (ja): L p50 931.0 ms with the refiner is over python 900.0 ms",
        "E2E-1 (ja): L p95 931.0 ms with the refiner is over python 900.0 ms",
    ] {
        assert!(said.contains(needle), "{needle}\n{said}");
    }
    Ok(())
}

#[test]
fn a_refine_run_equal_to_python_passes() -> TestResult {
    let ja = Lengths {
        python: vec![900; N],
        ..calm(Lang::Ja)
    };
    ja_verdict(ja)?.result()?;
    Ok(())
}

#[test]
fn a_refine_tail_over_python_is_rejected_at_p95_alone() -> TestResult {
    let ja = Lengths {
        refine: tail(N, 900, 10, 50),
        python: vec![900; N],
        ..calm(Lang::Ja)
    };
    let said = rejection(&ja_verdict(ja)?)?;
    assert!(
        said.contains("E2E-1 (ja): L p95 950.0 ms with the refiner is over python 900.0 ms"),
        "{said}"
    );
    assert!(!said.contains("L p50"), "{said}");
    Ok(())
}

#[test]
fn a_translated_share_of_20_in_23_passes_and_19_in_23_is_rejected() -> TestResult {
    let edge = Lengths {
        refine: vec![900; 20],
        untranslated: 3,
        ..calm(Lang::Ja)
    };
    let verdict = ja_verdict(edge)?;
    verdict.result()?;
    assert!(
        verdict
            .render()
            .contains("translated 20 of 23 valid samples")
    );
    let past = Lengths {
        refine: vec![900; 19],
        untranslated: 4,
        ..calm(Lang::Ja)
    };
    let said = rejection(&ja_verdict(past)?)?;
    assert!(
        said.contains("E2E-1 (ja): translated 19 of 23 valid samples, 20 / 23 are needed"),
        "{said}"
    );
    Ok(())
}

#[test]
fn a_refine_run_with_no_refine_event_is_rejected() -> TestResult {
    let ja = Lengths {
        events: Some(events_text(&[partial_line()])),
        ..calm(Lang::Ja)
    };
    let said = rejection(&ja_verdict(ja)?)?;
    assert!(
        said.contains("E2E-1 (ja): the run holds no refine event"),
        "{said}"
    );
    assert!(!said.contains("hold a mark"), "{said}");
    Ok(())
}

#[test]
fn an_en_refine_run_with_no_refine_event_is_rejected_alone() -> TestResult {
    let en = Lengths {
        events: Some(String::new()),
        ..calm(Lang::En)
    };
    let said = rejection(&en_verdict(en)?)?;
    assert!(
        said.contains("E2E-2 (en): the run holds no refine event"),
        "{said}"
    );
    assert!(!said.contains("E2E-1"), "{said}");
    Ok(())
}

#[test]
fn a_refine_run_without_an_events_file_is_rejected_and_names_the_file() -> TestResult {
    let ja = Lengths {
        events: None,
        ..calm(Lang::Ja)
    };
    let said = rejection(&ja_verdict(ja)?)?;
    assert!(
        said.contains("refine.e2e1.events.jsonl is missing"),
        "{said}"
    );
    Ok(())
}

#[test]
fn ja_refine_sentences_with_no_mark_are_rejected() -> TestResult {
    let ja = Lengths {
        events: Some(events_of(Lang::Ja, 0, 5)),
        ..calm(Lang::Ja)
    };
    let said = rejection(&ja_verdict(ja)?)?;
    assert!(
        said.contains("E2E-1 (ja): 0 of 5 ja refine events hold a mark"),
        "{said}"
    );
    assert!(!said.contains("no refine event"), "{said}");
    Ok(())
}

#[test]
fn one_marked_sentence_among_five_is_enough() -> TestResult {
    let ja = Lengths {
        events: Some(events_of(Lang::Ja, 1, 4)),
        ..calm(Lang::Ja)
    };
    let verdict = ja_verdict(ja)?;
    verdict.result()?;
    assert!(
        verdict
            .render()
            .contains("E2E-1 (ja): 1 of 5 ja refine events hold a mark")
    );
    Ok(())
}

#[test]
fn each_of_the_three_marks_counts_alone() -> TestResult {
    for mark in MARKS {
        let line = event_line("ja", &format!("予算は五百万円です{mark}"));
        let ja = Lengths {
            events: Some(events_text(&[line])),
            ..calm(Lang::Ja)
        };
        ja_verdict(ja)?.result()?;
    }
    Ok(())
}

#[test]
fn a_mark_in_a_refine_event_of_another_language_does_not_count_for_the_ja_run() -> TestResult {
    let ja = Lengths {
        events: Some(events_of(Lang::En, 4, 0)),
        ..calm(Lang::Ja)
    };
    let said = rejection(&ja_verdict(ja)?)?;
    assert!(
        said.contains("E2E-1 (ja): 0 of 0 ja refine events hold a mark"),
        "{said}"
    );
    assert!(!said.contains("no refine event"), "{said}");
    Ok(())
}

#[test]
fn the_en_run_is_not_asked_for_marks() -> TestResult {
    let en = Lengths {
        events: Some(events_of(Lang::En, 0, 5)),
        ..calm(Lang::En)
    };
    en_verdict(en)?.result()?;
    Ok(())
}

#[test]
fn an_events_line_that_is_not_json_is_an_error_that_names_the_line() {
    let ja = Lengths {
        events: Some(format!("{}\nnot json\n", partial_line())),
        ..calm(Lang::Ja)
    };
    let said = ja_verdict(ja).err();
    assert!(
        said.as_deref()
            .is_some_and(|text| text.contains("events line 2 is not JSON")),
        "{said:?}"
    );
}

#[test]
fn a_refine_event_with_no_text_is_an_error_that_names_the_key() {
    let line = json!({"event": {"type": "refine", "lang": "ja"}, "t_us": 1}).to_string();
    let ja = Lengths {
        events: Some(events_text(&[line])),
        ..calm(Lang::Ja)
    };
    let said = ja_verdict(ja).err();
    assert!(
        said.as_deref()
            .is_some_and(|text| text.contains("events line 1") && text.contains("/event/text")),
        "{said:?}"
    );
}

/// Two runs taken without the refiner give a gap of 0, and nothing else is blamed on the gap.
#[test]
fn two_runs_without_the_refiner_give_a_gap_of_zero() -> TestResult {
    let same = tail(N, 900, 10, 60);
    let ja = Lengths {
        refine: same.clone(),
        no_refine: same,
        events: Some(events_text(&[partial_line()])),
        ..calm(Lang::Ja)
    };
    let verdict = ja_verdict(ja)?;
    assert_eq!(
        verdict.lang(Lang::Ja)?.gap,
        Some(Gap {
            p50_us: 0,
            p95_us: 0
        })
    );
    let said = rejection(&verdict)?;
    assert!(said.contains("the run holds no refine event"), "{said}");
    assert!(!said.contains("over the run with --no-refine"), "{said}");
    Ok(())
}

#[test]
fn a_failing_en_run_is_named_alone() -> TestResult {
    let en = Lengths {
        refine: vec![932; N],
        ..calm(Lang::En)
    };
    let said = rejection(&en_verdict(en)?)?;
    assert!(said.contains("E2E-2 (en)"), "{said}");
    assert!(!said.contains("E2E-1"), "{said}");
    Ok(())
}

#[test]
fn an_arm_with_no_translated_sample_is_named() -> TestResult {
    let ja = Lengths {
        refine: Vec::new(),
        untranslated: 5,
        ..calm(Lang::Ja)
    };
    let said = rejection(&ja_verdict(ja)?)?;
    assert!(
        said.contains("E2E-1 (ja) refine: no valid sample has a translation"),
        "{said}"
    );
    let ja = Lengths {
        python: Vec::new(),
        ..calm(Lang::Ja)
    };
    let said = rejection(&ja_verdict(ja)?)?;
    assert!(
        said.contains("E2E-1 (ja) python: no valid sample has a translation"),
        "{said}"
    );
    assert!(!said.contains("E2E-2"), "{said}");
    Ok(())
}

#[test]
fn a_rust_run_with_a_first_row_load15_of_2_50_is_rejected_by_the_gate() -> TestResult {
    let runs = |load15: &str| -> Result<Vec<Run>, Failure> {
        let mut runs = fixture_set(&calm(Lang::Ja), &calm(Lang::En));
        set_machine(
            &mut runs,
            Arm::Refine,
            Lang::Ja,
            machine_with_first_load15(load15),
        )?;
        Ok(runs)
    };
    // The inside of the line is the positive control.
    judge(&runs("2.49")?)?.result()?;
    let said = rejection(&judge(&runs("2.50")?)?)?;
    for needle in ["refine.e2e1", "load15", "2.50"] {
        assert!(said.contains(needle), "{needle}: {said}");
    }
    assert!(
        !said.contains("norefine") && !said.contains("e2e2"),
        "{said}"
    );
    Ok(())
}

#[test]
fn a_no_refine_run_is_judged_by_the_gate_and_a_python_run_is_not() -> TestResult {
    let mut runs = fixture_set(&calm(Lang::Ja), &calm(Lang::En));
    let busy = machine_with_first_load15("2.50");
    set_machine(&mut runs, Arm::Python, Lang::Ja, busy.clone())?;
    judge(&runs)?.result()?;
    set_machine(&mut runs, Arm::NoRefine, Lang::En, busy)?;
    let said = rejection(&judge(&runs)?)?;
    assert!(said.contains("norefine.e2e2"), "{said}");
    assert!(!said.contains("python"), "{said}");
    Ok(())
}

#[test]
fn the_record_names_follow_the_layout() {
    let stems: Vec<String> = pairs().map(|(arm, lang)| stem_of(arm, lang)).collect();
    assert_eq!(
        stems,
        [
            "refine.e2e1",
            "refine.e2e2",
            "norefine.e2e1",
            "norefine.e2e2",
            "python.e2e1",
            "python.e2e2"
        ]
    );
}

#[test]
fn a_directory_with_no_record_is_named_and_not_passed() {
    let dir = Path::new(FIXTURES).join("no-such-rp61-records");
    let said = load_runs(&dir).err();
    assert!(
        said.as_deref()
            .is_some_and(|text| text.starts_with("no record: 6 of the 6 runs")
                && text.contains("refine.e2e1.jsonl")
                && text.contains("python.e2e2.jsonl")),
        "{said:?}"
    );
}

/// A run recorded before the refiner is read as the files stand: samples, the
/// machine record and the events, with no `refine` event among them.
#[test]
fn a_recorded_run_is_read_whole_and_holds_no_refine_event() -> TestResult {
    let dir = Path::new(RESULTS).join("r8x");
    let run = load_stem(&dir, "e3p1.e2e1", Arm::NoRefine, Lang::Ja)?;
    let Some(events) = run.events.as_deref() else {
        return Err("the recorded run has no events file".into());
    };
    assert!(run.machine.is_some());
    assert!(refine_events(events)?.is_empty());
    let Some(found) = column(&run.samples)? else {
        return Err("the recorded run has no translated valid sample".into());
    };
    assert!(found.n > 0 && found.p50_us <= found.p95_us);
    assert_eq!(
        gap_between(found, found),
        Gap {
            p50_us: 0,
            p95_us: 0
        }
    );
    Ok(())
}

#[test]
fn a_refine_event_is_read_with_the_keys_the_wire_gives_it() -> TestResult {
    let text = events_text(&[partial_line(), event_line("ja", "予算は五百万円です。")]);
    let events = refine_events(&text)?;
    assert_eq!(events.len(), 1);
    assert_eq!(
        (events[0].lang.as_str(), events[0].text.as_str()),
        ("ja", "予算は五百万円です。")
    );
    assert_eq!(marked_count(&events, "ja"), (1, 1));
    assert_eq!(marked_count(&events, "en"), (0, 0));
    Ok(())
}

// ---- fixed samples, written out ----
//
// The rows above build their samples from the constants they judge (`MARKS`,
// `VALID_MS`) or from a formula. A constant changed together with its fixture
// would still pass them, so the rows below spell every sentence, wait and rank
// out.

/// A calm ja run whose only refine event holds `sentence`.
fn ja_with_sentence(sentence: &str) -> Lengths {
    Lengths {
        events: Some(events_text(&[event_line("ja", sentence)])),
        ..calm(Lang::Ja)
    }
}

#[test]
fn the_comma_and_the_question_mark_each_pass_in_a_sentence_written_out() -> TestResult {
    // Each sentence holds one of the three marks and neither of the other two.
    for sentence in ["予算は五百万円です、", "本当ですか？"] {
        let verdict = ja_verdict(ja_with_sentence(sentence))?;
        verdict.result()?;
        let text = verdict.render();
        assert!(
            text.contains("E2E-1 (ja): 1 of 1 ja refine events hold a mark"),
            "{sentence}\n{text}"
        );
    }
    Ok(())
}

#[test]
fn a_sentence_with_none_of_the_three_marks_is_rejected() -> TestResult {
    let said = rejection(&ja_verdict(ja_with_sentence("予算は五百万円です"))?)?;
    assert!(
        said.contains("E2E-1 (ja): 0 of 1 ja refine events hold a mark"),
        "{said}"
    );
    Ok(())
}

#[test]
fn a_sample_that_waited_119_ms_is_not_valid_and_one_of_120_ms_is() -> TestResult {
    // Waits of 119, 120 and 121 ms; each decodes for 150 ms and is translated 100 ms
    // after its final, so `L` is 369, 370 and 371 ms.
    let samples = vec![
        sample(1, 119, 150, Some(100)),
        sample(2, 120, 150, Some(100)),
        sample(3, 121, 150, Some(100)),
    ];
    assert_eq!(translated_l_us(&samples), [370_000, 371_000]);
    let n = column(&samples)?.map(|found| found.n);
    assert_eq!(n, Some(2));
    Ok(())
}

#[test]
fn a_run_whose_samples_are_all_under_120_ms_has_no_valid_sample() -> TestResult {
    let samples = vec![sample(1, 119, 150, Some(100))];
    assert!(translated_l_us(&samples).is_empty());
    assert_eq!(column(&samples)?, None);
    let mut runs = fixture_set(&calm(Lang::Ja), &calm(Lang::En));
    runs.retain(|run| !(run.arm == Arm::Refine && run.lang == Lang::Ja));
    runs.push(fixture_run(
        Arm::Refine,
        Lang::Ja,
        samples,
        calm(Lang::Ja).events,
    ));
    let said = rejection(&judge(&runs)?)?;
    assert!(
        said.contains("E2E-1 (ja) refine: no valid sample has a translation"),
        "{said}"
    );
    Ok(())
}

/// The column of valid, translated samples whose `L` are `l_ms`.
fn column_of_l(l_ms: &[i64]) -> Result<Column, Failure> {
    column(&samples_of(l_ms))?.ok_or_else(|| String::from("no valid sample has a translation"))
}

#[test]
fn the_p95_is_the_nearest_rank_value_and_a_rank_off_by_one_is_not() -> TestResult {
    // n = 20: the 19th of 20 (190 ms), with 180 ms and 200 ms on either side of it.
    let twenty = column_of_l(&[
        10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160, 170, 180, 190, 200,
    ])?;
    assert_eq!((twenty.n, twenty.p95_us), (20, 190_000));
    // n = 21: the 20th of 21 (200 ms), with 190 ms and 210 ms on either side of it.
    // Rounding 0.95 * 21 down would take the 19th (190 ms).
    let twenty_one = column_of_l(&[
        10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160, 170, 180, 190, 200,
        210,
    ])?;
    assert_eq!((twenty_one.n, twenty_one.p95_us), (21, 200_000));
    Ok(())
}

#[test]
fn a_directory_missing_one_runs_samples_names_that_file_alone() -> TestResult {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("rp61-one-run-missing");
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    std::fs::create_dir_all(&dir)?;
    // Five of the six sample files stand; `python.e2e1.jsonl` is the one that does not.
    const PRESENT: [&str; 5] = [
        "refine.e2e1.jsonl",
        "refine.e2e2.jsonl",
        "norefine.e2e1.jsonl",
        "norefine.e2e2.jsonl",
        "python.e2e2.jsonl",
    ];
    for file in PRESENT {
        std::fs::write(dir.join(file), "")?;
    }
    let Err(said) = load_runs(&dir) else {
        return Err("a directory with a missing sample file was loaded".into());
    };
    // The directory is part of the sentence; its spelling is not what is read here.
    let place = dir.display().to_string();
    assert!(said.contains(&place), "{said}");
    let said = said.replace(&place, "");
    assert!(
        said.contains("1 of the 6 runs") && said.contains("python.e2e1.jsonl"),
        "{said}"
    );
    for file in PRESENT {
        assert!(
            !said.contains(file),
            "{file} stands, yet it is named: {said}"
        );
    }
    Ok(())
}

#[test]
#[ignore = "reads the records in docs/results/rp61/"]
fn rp61_live_refine_does_not_delay_the_subtitle() -> TestResult {
    let runs = load_runs(&Path::new(RESULTS).join("rp61"))?;
    let verdict = judge(&runs)?;
    println!("{}", verdict.render());
    assert!(verdict.failures().is_empty(), "{}", verdict.render());
    Ok(())
}
