//! The rule that ended each utterance of a Rust run (RP-32), and the silence
//! line that rule implies for the validity of a sample.
//!
//! `hayamimi` writes one `asr.endpoint.decided` line per `final`: `id` is the
//! number in the `final` event's `<source>-<number>` id, `rule` the rule that
//! ended the utterance. A sample is closed by its last `final`, the one that
//! fixes `t_final_us`, so the line of a sample is the line of that `final`'s
//! rule. A `final` with no decided line is refused by run and id; the rule is
//! never chosen for it.

use std::collections::BTreeMap;

use hayamimi_asr::endpoint::Rule;
use hayamimi_asr::vad::EARLY_MIN_SILENCE_MS;
use hayamimi_eval::latency::Sample;
use serde_json::Value;

use super::Failure;

/// The `event` name of the decided line.
const DECIDED: &str = "asr.endpoint.decided";

/// Every rule the server can log.
pub const RULES: [Rule; 4] = [Rule::Early, Rule::Silence, Rule::MaxSpeech, Rule::Flush];

/// The silence (ms) a sample closed by `rule` must have waited: the early
/// detector's for the early rule, the run's `min_silence` for the others.
pub fn silence_line_ms(rule: Rule, min_silence_ms: i64) -> i64 {
    match rule {
        Rule::Early => i64::from(EARLY_MIN_SILENCE_MS),
        Rule::Silence | Rule::MaxSpeech | Rule::Flush => min_silence_ms,
    }
}

/// The rule the log calls `name`.
pub fn rule_named(name: &str) -> Option<Rule> {
    RULES.into_iter().find(|rule| rule.name() == name)
}

/// `(id, rule)` of a decided line; `None` for any other line.
fn decided_line(line: &str) -> Result<Option<(u64, Rule)>, Failure> {
    if !line.contains(DECIDED) {
        return Ok(None);
    }
    let row: Value = serde_json::from_str(line)
        .map_err(|why| format!("a line naming {DECIDED} is not JSON: {why}"))?;
    if row.get("event").and_then(Value::as_str) != Some(DECIDED) {
        return Ok(None);
    }
    let Some(id) = row.get("id").and_then(Value::as_u64) else {
        return Err(format!("a {DECIDED} line has no integer id: {line}"));
    };
    let name = row.get("rule").and_then(Value::as_str);
    let Some(rule) = name.and_then(rule_named) else {
        return Err(format!("{DECIDED} id {id}: {name:?} is not a rule"));
    };
    Ok(Some((id, rule)))
}

/// The rule of every decided line of a server's stdout, by id. An id that
/// stands twice is refused: which rule closed it cannot be told.
pub fn decided_rules(log: &str) -> Result<BTreeMap<u64, Rule>, Failure> {
    let mut found = BTreeMap::new();
    for line in log.lines() {
        let Some((id, rule)) = decided_line(line)? else {
            continue;
        };
        if found.insert(id, rule).is_some() {
            return Err(format!("{DECIDED} stands twice for id {id}"));
        }
    }
    Ok(found)
}

/// Every `final` of an events file: its id and the time the tool received it.
fn finals_of(events: &str) -> Result<Vec<(String, i64)>, Failure> {
    let mut finals = Vec::new();
    for line in events.lines().filter(|line| !line.trim().is_empty()) {
        let row: Value = serde_json::from_str(line)
            .map_err(|why| format!("an events line is not JSON: {why}"))?;
        let event = row.get("event");
        if event.and_then(|e| e.get("type")).and_then(Value::as_str) != Some("final") {
            continue;
        }
        let id = event.and_then(|e| e.get("id")).and_then(Value::as_str);
        let t_us = row.get("t_us").and_then(Value::as_i64);
        let (Some(id), Some(t_us)) = (id, t_us) else {
            return Err(format!("a final event has no id or no t_us: {line}"));
        };
        finals.push((id.to_owned(), t_us));
    }
    Ok(finals)
}

/// The number of a `<source>-<number>` id.
fn number_of(id: &str) -> Option<u64> {
    id.rsplit_once('-')
        .and_then(|(_, number)| number.parse().ok())
}

/// The rule of each `final` as `(t_us, rule)`. Every `final` that has no
/// decided line is named with the run, all of them and not the first only.
fn joined_finals(
    run: &str,
    finals: &[(String, i64)],
    decided: &BTreeMap<u64, Rule>,
) -> Result<Vec<(i64, Rule)>, Failure> {
    let mut joined = Vec::new();
    let mut unjoined = Vec::new();
    for (id, t_us) in finals {
        match number_of(id).and_then(|number| decided.get(&number)) {
            Some(rule) => joined.push((*t_us, *rule)),
            None => unjoined.push(id.as_str()),
        }
    }
    if unjoined.is_empty() {
        return Ok(joined);
    }
    Err(format!(
        "{run}: the final {} has no {DECIDED} line in {run}.hayamimi.stdout.log",
        unjoined.join(", ")
    ))
}

/// The rule of the `final` the tool received at `t_final_us`. Two `final`s at
/// that moment with different rules cannot be told apart and are refused.
fn closing_rule(
    run: &str,
    sample: &Sample,
    t_final_us: i64,
    finals: &[(i64, Rule)],
) -> Result<Rule, Failure> {
    let place = format!("{run}: repeat {} utt {}", sample.repeat, sample.utt);
    let mut here = finals
        .iter()
        .filter(|(t_us, _)| *t_us == t_final_us)
        .map(|(_, rule)| *rule);
    let Some(first) = here.next() else {
        return Err(format!(
            "{place}: no final arrived at t_final_us {t_final_us}"
        ));
    };
    if here.any(|rule| rule != first) {
        return Err(format!(
            "{place}: finals with different rules arrived at t_final_us {t_final_us}"
        ));
    }
    Ok(first)
}

/// The rule that closed each sample of the Rust run `run` that has a `final`,
/// by `(repeat, utt)`. `events` is `<run>.events.jsonl`, `log` is
/// `<run>.hayamimi.stdout.log`.
pub fn closing_rules(
    run: &str,
    samples: &[Sample],
    events: &str,
    log: &str,
) -> Result<BTreeMap<(u32, u32), Rule>, Failure> {
    let decided = decided_rules(log).map_err(|why| format!("{run}.hayamimi.stdout.log: {why}"))?;
    let finals = finals_of(events).map_err(|why| format!("{run}.events.jsonl: {why}"))?;
    let finals = joined_finals(run, &finals, &decided)?;
    let mut rules = BTreeMap::new();
    for sample in samples {
        let Some(t_final_us) = sample.t_final_us else {
            continue;
        };
        let rule = closing_rule(run, sample, t_final_us, &finals)?;
        rules.insert((sample.repeat, sample.utt), rule);
    }
    Ok(rules)
}
