//! CS-4a: verification V stops swaps and passes the tuning references.

mod support;

use hayamimi_relay::lang::Lang;
use hayamimi_relay::logging::NullSink;
use hayamimi_relay::verify::{HoldReason, Verdict, Verifier};
use support::{Pair, TestResult, cat_output, conv20};

fn verifier() -> Result<Verifier, String> {
    Verifier::open(None, &mut NullSink)
}

/// The number of pairs of one direction and the ids whose reference is held.
fn held_references(lang: Lang) -> Result<(usize, Vec<String>), String> {
    let mut verifier = verifier()?;
    let pairs: Vec<Pair> = conv20()?
        .into_iter()
        .filter(|pair| pair.lang == lang)
        .collect();
    let held = pairs
        .iter()
        .filter(|pair| {
            verifier.verify(&pair.src, &pair.reference, pair.lang, &mut NullSink) != Verdict::Ok
        })
        .map(|pair| pair.id.clone())
        .collect();
    Ok((pairs.len(), held))
}

/// The failure text of the suite: ids and a count, never a subtitle.
fn failure_report(ids: &[String]) -> String {
    format!("{} line(s) held: {}", ids.len(), ids.join(", "))
}

/// The verdict for one tuning line against the model's output for it.
fn verdict_for_cat(id: &str) -> Result<Verdict, String> {
    let pair = conv20()?
        .into_iter()
        .find(|pair| pair.id == id)
        .ok_or_else(|| format!("no tuning pair {id}"))?;
    let output = cat_output(id)?;
    Ok(verifier()?.verify(&pair.src, &output, pair.lang, &mut NullSink))
}

fn expect_held(id: &str, reason: HoldReason) -> TestResult {
    let verdict = verdict_for_cat(id)?;
    if verdict != Verdict::Held(reason) {
        return Err(format!("{id}: expected held for {reason:?}, got {verdict:?}").into());
    }
    Ok(())
}

fn expect_not_number(id: &str) -> TestResult {
    let verdict = verdict_for_cat(id)?;
    if verdict == Verdict::Held(HoldReason::Number) {
        return Err(format!("{id}: a value of 1 must not be compared").into());
    }
    Ok(())
}

/// T-CS-4a-001
#[test]
fn cs04a_verify_passes_tuning_refs_and_stops_known_drift_ja_en_refs_all_pass() -> TestResult {
    let (count, held) = held_references(Lang::Ja)?;
    assert_eq!(count, 20, "T-CS-4a-001: ja rows");
    assert!(held.is_empty(), "T-CS-4a-001: {}", failure_report(&held));
    Ok(())
}

/// T-CS-4a-002
#[test]
fn cs04a_verify_passes_tuning_refs_and_stops_known_drift_en_ja_refs_all_pass() -> TestResult {
    let (count, held) = held_references(Lang::En)?;
    assert_eq!(count, 20, "T-CS-4a-002: en rows");
    assert!(held.is_empty(), "T-CS-4a-002: {}", failure_report(&held));
    Ok(())
}

/// T-CS-4a-003
#[test]
fn cs04a_verify_passes_tuning_refs_and_stops_known_drift_no_number_pair_passes() -> TestResult {
    let verdict = verdict_for_cat("j01")?;
    assert_eq!(verdict, Verdict::Ok, "T-CS-4a-003");
    Ok(())
}

/// T-CS-4a-004
#[test]
fn cs04a_verify_passes_tuning_refs_and_stops_known_drift_j08_is_held_for_number() -> TestResult {
    expect_held("j08", HoldReason::Number)
}

/// T-CS-4a-005
#[test]
fn cs04a_verify_passes_tuning_refs_and_stops_known_drift_j12_is_held_for_number() -> TestResult {
    expect_held("j12", HoldReason::Number)
}

/// T-CS-4a-006
#[test]
fn cs04a_verify_passes_tuning_refs_and_stops_known_drift_e01_is_held_for_number() -> TestResult {
    expect_held("e01", HoldReason::Number)
}

/// T-CS-4a-007
#[test]
fn cs04a_verify_passes_tuning_refs_and_stops_known_drift_e20_is_held_for_number() -> TestResult {
    expect_held("e20", HoldReason::Number)
}

/// T-CS-4a-008
#[test]
fn cs04a_verify_passes_tuning_refs_and_stops_known_drift_e09_value_one_is_not_compared()
-> TestResult {
    expect_not_number("e09")
}

/// T-CS-4a-009
#[test]
fn cs04a_verify_passes_tuning_refs_and_stops_known_drift_e16_value_one_is_not_compared()
-> TestResult {
    expect_not_number("e16")
}

/// T-CS-4a-010
#[test]
fn cs04a_verify_passes_tuning_refs_and_stops_known_drift_failure_report_names_ids_and_count_only()
-> TestResult {
    let report = failure_report(&[String::from("j08"), String::from("e01")]);
    for needle in ["j08", "e01", "2"] {
        assert!(
            report.contains(needle),
            "T-CS-4a-010: {report} lacks {needle}"
        );
    }
    for leaked in ["五百万円", "Talk to you", "budget", "Hello"] {
        assert!(
            !report.contains(leaked),
            "T-CS-4a-010: {report} leaks {leaked}"
        );
    }
    Ok(())
}
