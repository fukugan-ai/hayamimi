//! BR-1: the wait from the end of speech to the translation is written stage by
//! stage on one clock (design 10.13.3).
//!
//! The records are built by hand, in the two files a run leaves: the samples
//! file (one [`Sample`] a line) and the events file (`{"t_us", "event"}` a
//! line). `session_start` stands at 9 s, so a `final` whose `t1_s` is `T - 9`
//! seconds is placed at `T` on the tool's clock.
//!
//! Record A is three utterances (`ja`, `en`, `ja`) and record B seven: the
//! first two are the cold samples of their languages and the other five carry
//! the stage values `(d, L_dec, L_tr)` of the budget lines.

mod support;

use hayamimi_eval::latency::{Stage, StageReport, stage_report};
use hayamimi_eval::sha::sha256_hex;
use serde_json::json;
use support::{Failure, TestResult, event_line, samples_text, window_sample};

/// Where `session_start` stands on the tool's clock.
const SESSION_START_US: i64 = 9_000_000;

/// One utterance of a hand-made run, in microseconds on the tool's clock.
#[derive(Clone)]
struct Utt {
    repeat: u32,
    utt: u32,
    lang: &'static str,
    t_start_us: i64,
    t_end_us: i64,
    final_at_us: i64,
    latency_ms: Option<f64>,
    draft_at_us: Option<i64>,
    settled_at_us: Option<i64>,
    settled_status: &'static str,
}

impl Utt {
    /// An utterance that ends at `end_ms`, whose closing `final` comes
    /// `d + dec` later, whose draft translation comes `tr` after that and
    /// whose settled translation 500 ms after the draft.
    fn staged(utt: u32, lang: &'static str, end_ms: i64, (d, dec, tr): (i64, i64, i64)) -> Utt {
        let end = end_ms * 1000;
        let final_at = end + (d + dec) * 1000;
        Utt {
            repeat: 1,
            utt,
            lang,
            t_start_us: end - 2_000_000,
            t_end_us: end,
            final_at_us: final_at,
            latency_ms: Some(dec as f64),
            draft_at_us: Some(final_at + tr * 1000),
            settled_at_us: Some(final_at + (tr + 500) * 1000),
            settled_status: "ok",
        }
    }

    fn id(&self) -> String {
        format!("r{}-{}", self.repeat, self.utt)
    }

    fn text(&self) -> String {
        format!("text of {}", self.id())
    }

    fn t1_s(&self) -> f64 {
        (self.t_end_us - SESSION_START_US) as f64 / 1e6
    }

    fn final_event(&self) -> (i64, serde_json::Value) {
        let mut body = json!({"type": "final", "id": self.id(), "lang": self.lang,
                              "text": self.text(), "t1_s": self.t1_s()});
        if let Some(latency) = self.latency_ms {
            body["latency_ms"] = json!(latency);
        }
        (self.final_at_us, body)
    }

    fn translation_event(
        &self,
        at_us: i64,
        settled: bool,
        status: &str,
    ) -> (i64, serde_json::Value) {
        let text = if status == "ok" { "translated" } else { "" };
        (
            at_us,
            json!({"type": "translation", "id": self.id(), "of": "final", "settled": settled,
                   "status": status, "text": text, "lang": "xx",
                   "src_sha256": sha256_hex(self.text().as_bytes())}),
        )
    }

    fn events(&self) -> Vec<(i64, serde_json::Value)> {
        let mut found = vec![self.final_event()];
        if let Some(at) = self.draft_at_us {
            found.push(self.translation_event(at, false, "ok"));
        }
        if let Some(at) = self.settled_at_us {
            found.push(self.translation_event(at, true, self.settled_status));
        }
        found
    }
}

/// The two files of a run made of `utts`.
fn files(utts: &[Utt]) -> Result<(String, String), Failure> {
    let samples: Vec<_> = utts
        .iter()
        .map(|u| window_sample(u.repeat, u.utt, u.t_start_us, u.t_end_us))
        .collect();
    let mut events = vec![(SESSION_START_US, json!({"type": "session_start"}))];
    events.extend(utts.iter().flat_map(Utt::events));
    events.sort_by_key(|(at, _)| *at);
    let lines: Vec<String> = events
        .into_iter()
        .map(|(at, body)| event_line(at, body))
        .collect();
    Ok((samples_text(&samples)?, lines.join("\n") + "\n"))
}

fn report_of(utts: &[Utt]) -> Result<StageReport, Failure> {
    let (samples, events) = files(utts)?;
    stage_report(&samples, &events).map_err(|error| error.to_string())
}

/// Record A.
fn record_a() -> Vec<Utt> {
    let first = Utt {
        t_start_us: 9_500_000,
        final_at_us: 12_745_000,
        latency_ms: Some(268.0),
        draft_at_us: Some(12_978_000),
        settled_at_us: Some(13_700_000),
        ..Utt::staged(1, "ja", 12_000, (0, 0, 0))
    };
    let second = Utt {
        t_start_us: 14_000_000,
        final_at_us: 16_300_000,
        latency_ms: Some(200.0),
        draft_at_us: Some(16_350_000),
        settled_at_us: Some(16_900_000),
        ..Utt::staged(2, "en", 16_000, (0, 0, 0))
    };
    let third = Utt {
        t_start_us: 17_000_000,
        final_at_us: 19_550_000,
        latency_ms: Some(250.0),
        draft_at_us: Some(19_650_000),
        settled_at_us: Some(20_150_000),
        ..Utt::staged(3, "ja", 19_000, (0, 0, 0))
    };
    vec![first, second, third]
}

/// The stage values `(d, L_dec, L_tr)` in ms of the five warm utterances of record B.
const WARM: [(i64, i64, i64); 5] = [
    (250, 200, 50),
    (280, 250, 60),
    (300, 270, 100),
    (450, 300, 150),
    (600, 480, 300),
];

/// Record B: the cold utterances of record A, then the five warm ones.
fn record_b() -> Vec<Utt> {
    let mut utts: Vec<Utt> = record_a().into_iter().take(2).collect();
    let langs = ["ja", "en", "ja", "en", "ja"];
    for (at, (stages, lang)) in WARM.iter().zip(langs).enumerate() {
        let utt = 3 + at as u32;
        utts.push(Utt::staged(utt, lang, 19_000 + 5_000 * at as i64, *stages));
    }
    utts
}

fn with_stage(mut utts: Vec<Utt>, utt: u32, set: impl Fn(&mut Utt)) -> Vec<Utt> {
    if let Some(found) = utts.iter_mut().find(|u| u.utt == utt) {
        set(found);
    }
    utts
}

/// `(p50, p95)` in ms of a stage, and its verdicts, from the warm utterances.
fn percentiles(report: &StageReport, stage: Stage) -> Result<(i64, i64), Failure> {
    let stat = report
        .stat(stage)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("no value for {stage:?}"))?;
    Ok((stat.p50_ms, stat.p95_ms))
}

/// Whether `(p50, p95)` of a stage stand inside their lines.
fn inside(report: &StageReport, stage: Stage) -> Result<(bool, bool), Failure> {
    let verdicts = report.verdicts().map_err(|error| error.to_string())?;
    verdicts
        .iter()
        .find(|v| v.stage == stage)
        .map(|v| (v.p50_inside, v.p95_inside))
        .ok_or_else(|| format!("no verdict for {stage:?}"))
}

fn all_inside(report: &StageReport) -> Result<bool, Failure> {
    let verdicts = report.verdicts().map_err(|error| error.to_string())?;
    Ok(verdicts.iter().all(|v| v.p50_inside && v.p95_inside))
}

/// The `(d, L_dec, L_tr, L_e)` of a row in ms.
type Stages = (Option<i64>, Option<i64>, Option<i64>, Option<i64>);

fn stages_of(report: &StageReport, utt: u32) -> Result<Stages, Failure> {
    let row = report
        .cold
        .iter()
        .chain(&report.warm)
        .find(|row| row.utt == utt)
        .ok_or_else(|| format!("no row for utterance {utt}"))?;
    Ok((row.d_ms(), row.dec_ms(), row.tr_ms(), row.settle_ms()))
}

fn utts_of(rows: &[hayamimi_eval::latency::StageRow]) -> Vec<u32> {
    rows.iter().map(|row| row.utt).collect()
}

/// T-BR-01-001
#[test]
fn br01_each_stage_of_the_wait_is_recorded_on_one_clock() -> TestResult {
    let report = report_of(&record_a())?;
    assert_eq!(
        stages_of(&report, 1)?,
        (Some(477), Some(268), Some(233), Some(722))
    );
    assert_eq!(
        stages_of(&report, 2)?,
        (Some(100), Some(200), Some(50), Some(550))
    );
    assert_eq!(
        stages_of(&report, 3)?,
        (Some(300), Some(250), Some(100), Some(500))
    );
    assert_eq!(utts_of(&report.cold), [1, 2]);
    assert_eq!(utts_of(&report.warm), [3]);
    let waits: Vec<Option<i64>> = [1, 2, 3]
        .iter()
        .map(|utt| {
            report
                .cold
                .iter()
                .chain(&report.warm)
                .find(|row| row.utt == *utt)
                .and_then(|row| row.l_us)
        })
        .collect();
    assert_eq!(waits, [Some(978_000), Some(350_000), Some(650_000)]);
    for utt in 1..=3 {
        let row = report
            .cold
            .iter()
            .chain(&report.warm)
            .find(|row| row.utt == utt)
            .ok_or("a row is missing")?;
        let sum =
            row.d_us.ok_or("no d")? + row.dec_us.ok_or("no dec")? + row.tr_us.ok_or("no tr")?;
        assert_eq!(Some(sum), row.l_us, "utterance {utt}");
    }
    assert_eq!(percentiles(&report, Stage::Wait)?, (300, 300));
    assert_eq!(percentiles(&report, Stage::Decode)?, (250, 250));
    assert_eq!(percentiles(&report, Stage::Draft)?, (100, 100));
    assert!(all_inside(&report)?);
    Ok(())
}

/// T-BR-01-002
#[test]
fn a_run_whose_stages_stand_on_their_lines_is_written_inside_the_lines() -> TestResult {
    let report = report_of(&record_b())?;
    assert_eq!((report.warm.len(), report.cold.len()), (5, 2));
    assert_eq!(percentiles(&report, Stage::Wait)?, (300, 600));
    assert_eq!(percentiles(&report, Stage::Decode)?, (270, 480));
    assert_eq!(percentiles(&report, Stage::Draft)?, (100, 300));
    assert_eq!(report.verdicts()?.len(), 3);
    assert!(all_inside(&report)?);
    Ok(())
}

/// T-BR-01-003
#[test]
fn a_d_p50_of_301_ms_is_outside_its_line_while_p95_stays_inside() -> TestResult {
    let utts = with_stage(record_b(), 5, |u| {
        *u = Utt::staged(5, "ja", 29_000, (301, 270, 100))
    });
    let report = report_of(&utts)?;
    assert_eq!(percentiles(&report, Stage::Wait)?, (301, 600));
    assert_eq!(inside(&report, Stage::Wait)?, (false, true));
    assert_eq!(inside(&report, Stage::Decode)?, (true, true));
    assert_eq!(inside(&report, Stage::Draft)?, (true, true));
    Ok(())
}

/// T-BR-01-004
#[test]
fn a_d_p95_of_601_ms_is_outside_its_line_while_p50_stays_inside() -> TestResult {
    let utts = with_stage(record_b(), 7, |u| {
        *u = Utt::staged(7, "ja", 39_000, (601, 480, 300))
    });
    let report = report_of(&utts)?;
    assert_eq!(inside(&report, Stage::Wait)?, (true, false));
    Ok(())
}

/// T-BR-01-005
#[test]
fn a_d_p50_of_299_ms_and_p95_of_599_ms_are_inside_their_lines() -> TestResult {
    let utts = with_stage(record_b(), 5, |u| {
        *u = Utt::staged(5, "ja", 29_000, (299, 270, 100))
    });
    let utts = with_stage(utts, 7, |u| {
        *u = Utt::staged(7, "ja", 39_000, (599, 480, 300))
    });
    let report = report_of(&utts)?;
    assert_eq!(percentiles(&report, Stage::Wait)?, (299, 599));
    assert_eq!(inside(&report, Stage::Wait)?, (true, true));
    Ok(())
}

/// T-BR-01-006
#[test]
fn a_decode_p50_of_271_ms_is_outside_its_line() -> TestResult {
    let utts = with_stage(record_b(), 5, |u| {
        *u = Utt::staged(5, "ja", 29_000, (300, 271, 100))
    });
    let report = report_of(&utts)?;
    assert_eq!(inside(&report, Stage::Decode)?, (false, true));
    Ok(())
}

/// T-BR-01-007
#[test]
fn a_decode_p95_of_481_ms_is_outside_its_line() -> TestResult {
    let utts = with_stage(record_b(), 7, |u| {
        *u = Utt::staged(7, "ja", 39_000, (600, 481, 300))
    });
    let report = report_of(&utts)?;
    assert_eq!(inside(&report, Stage::Decode)?, (true, false));
    Ok(())
}

/// T-BR-01-008
#[test]
fn a_draft_translation_p50_of_101_ms_is_outside_its_line() -> TestResult {
    let utts = with_stage(record_b(), 5, |u| {
        *u = Utt::staged(5, "ja", 29_000, (300, 270, 101))
    });
    let report = report_of(&utts)?;
    assert_eq!(inside(&report, Stage::Draft)?, (false, true));
    Ok(())
}

/// T-BR-01-009
#[test]
fn a_draft_translation_p95_of_301_ms_is_outside_its_line() -> TestResult {
    let utts = with_stage(record_b(), 7, |u| {
        *u = Utt::staged(7, "ja", 39_000, (600, 480, 301))
    });
    let report = report_of(&utts)?;
    assert_eq!(inside(&report, Stage::Draft)?, (true, false));
    Ok(())
}

/// T-BR-01-010
#[test]
fn an_utterance_without_a_closing_final_has_no_stage_values_and_is_not_counted() -> TestResult {
    let mut utts = record_b();
    let early = Utt {
        utt: 8,
        t_start_us: 40_000_000,
        t_end_us: 42_000_000,
        final_at_us: 41_000_000,
        draft_at_us: None,
        settled_at_us: None,
        ..Utt::staged(8, "ja", 42_000, (0, 0, 0))
    };
    utts.push(early);
    let report = report_of(&utts)?;
    assert_eq!((report.warm.len(), report.cold.len()), (5, 2));
    assert!(
        stages_of(&report, 8).is_err(),
        "a row without a closing final is not kept"
    );
    assert_eq!(percentiles(&report, Stage::Wait)?, (300, 600));
    assert_eq!(percentiles(&report, Stage::Decode)?, (270, 480));
    assert_eq!(percentiles(&report, Stage::Draft)?, (100, 300));
    Ok(())
}

/// T-BR-01-011
#[test]
fn the_three_stages_add_up_to_the_wait_of_3_1_at_every_grid_point() -> TestResult {
    let axis = [0_i64, 1, 499, 500, 501, 2000];
    for d in axis {
        for dec in axis {
            for tr in axis {
                let utt = Utt::staged(1, "ja", 12_000, (d, dec, tr));
                let wait_us = utt.draft_at_us.ok_or("no draft")? - utt.t_end_us;
                let report = report_of(&[utt])?;
                let row = report.cold.first().ok_or("no row")?;
                let sum = row.d_ms().ok_or("no d")?
                    + row.dec_ms().ok_or("no dec")?
                    + row.tr_ms().ok_or("no tr")?;
                assert_eq!(sum, wait_us / 1000, "d {d}, dec {dec}, tr {tr}");
            }
        }
    }
    Ok(())
}

/// T-BR-01-012
#[test]
fn a_draft_translation_that_reused_a_partial_keeps_its_negative_wait() -> TestResult {
    let utts = with_stage(record_b(), 3, |u| {
        u.draft_at_us = Some(u.final_at_us - 40_000);
    });
    let report = report_of(&utts)?;
    assert_eq!(stages_of(&report, 3)?.2, Some(-40));
    let row = report
        .warm
        .iter()
        .find(|row| row.utt == 3)
        .ok_or("no row")?;
    assert_eq!(
        row.d_ms().ok_or("no d")? + row.dec_ms().ok_or("no dec")? + row.tr_ms().ok_or("no tr")?,
        410
    );
    assert_eq!(percentiles(&report, Stage::Draft)?, (100, 300));
    Ok(())
}

/// T-BR-01-013
#[test]
fn the_cold_sample_of_a_language_is_the_first_utterance_that_has_a_closing_final() -> TestResult {
    let utts = with_stage(record_b(), 1, |u| u.final_at_us = u.t_end_us - 500_000);
    let report = report_of(&utts)?;
    assert_eq!(utts_of(&report.cold), [2, 3]);
    assert_eq!(utts_of(&report.warm), [4, 5, 6, 7]);
    Ok(())
}

/// T-BR-01-020
#[test]
fn a_language_outside_ja_and_en_also_gets_its_own_cold_sample() -> TestResult {
    let utts = with_stage(record_b(), 4, |u| u.lang = "zh");
    let report = report_of(&utts)?;
    assert_eq!(utts_of(&report.cold), [1, 2, 4]);
    assert_eq!(utts_of(&report.warm), [3, 5, 6, 7]);
    Ok(())
}

/// T-BR-01-014
#[test]
fn a_second_repeat_in_the_same_process_has_no_cold_sample() -> TestResult {
    let mut utts = record_b();
    let again: Vec<Utt> = record_b()
        .into_iter()
        .map(|u| Utt {
            repeat: 2,
            t_start_us: u.t_start_us + 100_000_000,
            t_end_us: u.t_end_us + 100_000_000,
            final_at_us: u.final_at_us + 100_000_000,
            draft_at_us: u.draft_at_us.map(|at| at + 100_000_000),
            settled_at_us: u.settled_at_us.map(|at| at + 100_000_000),
            ..u
        })
        .collect();
    utts.extend(again);
    let report = report_of(&utts)?;
    assert_eq!(report.cold.len(), 2);
    assert!(
        report
            .cold
            .iter()
            .all(|row| row.repeat == 1 && row.utt <= 2)
    );
    assert_eq!(report.warm.len(), 12);
    Ok(())
}

/// T-BR-01-015
#[test]
fn a_closing_final_without_latency_ms_has_no_d_and_no_decode_but_keeps_its_translation_wait()
-> TestResult {
    let utts = with_stage(record_b(), 4, |u| u.latency_ms = None);
    let report = report_of(&utts)?;
    let row = report
        .warm
        .iter()
        .find(|row| row.utt == 4)
        .ok_or("no row")?;
    assert_eq!(
        (row.d_ms(), row.dec_ms(), row.tr_ms()),
        (None, None, Some(60))
    );
    let stat = |stage| report.stat(stage).map(|found| found.map(|s| s.n));
    assert_eq!(stat(Stage::Wait)?, Some(4));
    assert_eq!(stat(Stage::Decode)?, Some(4));
    assert_eq!(stat(Stage::Draft)?, Some(5));
    Ok(())
}

/// T-BR-01-016
#[test]
fn a_run_with_no_warm_sample_is_not_written_as_inside_the_lines() -> TestResult {
    let report = report_of(&record_a().into_iter().take(2).collect::<Vec<_>>())?;
    assert_eq!(report.cold.len(), 2);
    assert!(report.warm.is_empty());
    assert!(report.verdicts().is_err());
    let written = report.render();
    assert!(written.contains("温まった標本 0"), "{written}");
    assert!(
        !written.contains("線の内") && !written.contains("線の外"),
        "{written}"
    );
    Ok(())
}

/// T-BR-01-017
#[test]
fn the_settle_wait_is_recorded_without_a_verdict_and_needs_an_ok_settled_translation() -> TestResult
{
    let ok = report_of(&record_b())?;
    assert_eq!(stages_of(&ok, 3)?.3, Some(500));
    let missing = report_of(&with_stage(record_b(), 3, |u| u.settled_at_us = None))?;
    assert_eq!(stages_of(&missing, 3)?.3, None);
    let held = report_of(&with_stage(record_b(), 3, |u| u.settled_status = "held"))?;
    assert_eq!(stages_of(&held, 3)?.3, None);
    let stat = ok.stat(Stage::Settle)?.ok_or("no L_e stat")?;
    assert_eq!((stat.p50_ms, stat.p95_ms), (500, 500));
    assert_eq!(ok.verdicts()?.len(), 3, "L_e carries no verdict");
    let written = ok.render();
    let line = written
        .lines()
        .find(|line| line.contains("L_e"))
        .ok_or("no L_e line")?;
    assert!(!line.contains("線の"), "{line}");
    Ok(())
}

/// T-BR-01-018
#[test]
fn a_record_with_no_line_is_an_error() {
    assert!(stage_report("", "").is_err());
}

/// T-BR-01-019
#[test]
fn stage_values_are_floored_to_milliseconds_and_negative_ones_floor_away_from_zero() -> TestResult {
    let base = Utt::staged(1, "ja", 12_000, (0, 0, 0));
    let a = Utt {
        final_at_us: 12_745_000,
        latency_ms: Some(268.6),
        draft_at_us: Some(12_745_000 + 233_000),
        settled_at_us: None,
        ..base.clone()
    };
    let report = report_of(&[a])?;
    let row = report.cold.first().ok_or("no row")?;
    assert_eq!(row.dec_us, Some(268_600));
    assert_eq!(row.d_us, Some(476_400));
    assert_eq!(
        (row.d_ms(), row.dec_ms(), row.tr_ms()),
        (Some(476), Some(268), Some(233))
    );
    assert_eq!(row.l_us, Some(978_000));
    assert_eq!(Some(476_400 + 268_600 + 233_000), row.l_us);
    let b = Utt {
        final_at_us: 12_745_000,
        latency_ms: Some(268.0),
        draft_at_us: Some(12_745_000 - 400),
        settled_at_us: None,
        ..base.clone()
    };
    let report = report_of(&[b])?;
    assert_eq!(report.cold.first().ok_or("no row")?.tr_ms(), Some(-1));
    let c = Utt {
        final_at_us: 12_745_800,
        latency_ms: Some(268.9),
        draft_at_us: Some(12_745_800 + 233_900),
        settled_at_us: None,
        ..base
    };
    let report = report_of(&[c])?;
    let row = report.cold.first().ok_or("no row")?;
    assert_eq!(
        (row.d_ms(), row.dec_ms(), row.tr_ms()),
        (Some(476), Some(268), Some(233))
    );
    assert_eq!(row.l_us, Some(979_700));
    let sum = row.d_us.ok_or("no d")? + row.dec_us.ok_or("no dec")? + row.tr_us.ok_or("no tr")?;
    assert_eq!(Some(sum), row.l_us);
    Ok(())
}
