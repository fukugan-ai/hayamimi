//! RP-18: the punctuation of the Rust route is the same as the reference
//! implementation's, case by case: the same ids go into the model (the
//! tokenizer, which needs no model) and the same strings come out (the model
//! on the GPU, and the rules after it).

mod support;

use std::path::Path;
use std::sync::{Mutex, OnceLock};

use hayamimi_asr::ort_runtime::{Device, OrtRuntime};
use hayamimi_asr::punct::Punctuator;
use hayamimi_asr::punct::tokenizer::Tokenizer;
use support::{Failure, PARITY, TestResult, models_dir, read, read_json};

const VOCAB: &str = "mojicast-punct-onnx/vocab.txt";

struct Case {
    name: String,
    input: String,
    input_ids: Vec<i64>,
    expected: String,
}

fn text_of(value: &serde_json::Value, key: &str) -> Result<String, Failure> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("a case has no string `{key}`"))
}

fn cases() -> Result<Vec<Case>, Failure> {
    let json = read_json(Path::new(PARITY))?;
    let Some(list) = json.get("cases").and_then(serde_json::Value::as_array) else {
        return Err(String::from("the fixture has no `cases` array"));
    };
    list.iter()
        .map(|case| {
            let ids = case
                .get("input_ids")
                .and_then(serde_json::Value::as_array)
                .ok_or_else(|| String::from("a case has no `input_ids` array"))?
                .iter()
                .map(|id| {
                    id.as_i64()
                        .ok_or_else(|| String::from("an id is not an integer"))
                })
                .collect::<Result<Vec<i64>, Failure>>()?;
            Ok(Case {
                name: text_of(case, "name")?,
                input: text_of(case, "input")?,
                input_ids: ids,
                expected: text_of(case, "expected")?,
            })
        })
        .collect()
}

fn tokenizer() -> Result<Tokenizer, Failure> {
    let vocab = read(&models_dir()?.join(VOCAB))?;
    Tokenizer::from_vocab(&vocab).map_err(|error| error.to_string())
}

/// The names of the cases whose ids differ from the fixture's.
fn mismatching_ids(cases: &[Case], ids_of: impl Fn(&str) -> Vec<i64>) -> Vec<&str> {
    cases
        .iter()
        .filter(|case| ids_of(&case.input) != case.input_ids)
        .map(|case| case.name.as_str())
        .collect()
}

/// Every case's ids equal the fixture's; the error names the cases that differ.
fn check_input_ids(cases: &[Case], ids_of: impl Fn(&str) -> Vec<i64>) -> Result<(), Failure> {
    let differing = mismatching_ids(cases, ids_of);
    if differing.is_empty() {
        Ok(())
    } else {
        Err(format!("input_ids differ in: {}", differing.join(", ")))
    }
}

/// Every case's restored string equals the fixture's; the error names the
/// first case that differs and both strings.
fn check_expected(
    cases: &[Case],
    mut restore: impl FnMut(&str) -> Result<String, Failure>,
) -> Result<(), Failure> {
    for case in cases {
        let got = restore(&case.input)?;
        if got != case.expected {
            return Err(format!(
                "case {}: got {got:?}, want {:?}",
                case.name, case.expected
            ));
        }
    }
    Ok(())
}

/// The punctuation model on the GPU, opened once for the whole test binary.
fn gpu_punctuator() -> Result<&'static Mutex<Punctuator>, Failure> {
    static SHARED: OnceLock<Result<Mutex<Punctuator>, Failure>> = OnceLock::new();
    SHARED
        .get_or_init(|| {
            let runtime = OrtRuntime::shared().map_err(|error| error.to_string())?;
            let models = models_dir()?;
            Punctuator::open(runtime, &models, Device::Cuda, false)
                .map(Mutex::new)
                .map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(Clone::clone)
}

fn restore_on_gpu(text: &str) -> Result<String, Failure> {
    let mut punctuator = gpu_punctuator()?
        .lock()
        .map_err(|_| String::from("the punctuator is poisoned"))?;
    punctuator.restore(text).map_err(|error| error.to_string())
}

/// The punctuation model on the CPU, as a stage that fell back builds it.
fn cpu_punctuator() -> Result<&'static Mutex<Punctuator>, Failure> {
    static SHARED: OnceLock<Result<Mutex<Punctuator>, Failure>> = OnceLock::new();
    SHARED
        .get_or_init(|| {
            let runtime = OrtRuntime::shared().map_err(|error| error.to_string())?;
            let models = models_dir()?;
            Punctuator::open(runtime, &models, Device::Cpu, false)
                .map(Mutex::new)
                .map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(Clone::clone)
}

fn restore_on_cpu(text: &str) -> Result<String, Failure> {
    let mut punctuator = cpu_punctuator()?
        .lock()
        .map_err(|_| String::from("the punctuator is poisoned"))?;
    punctuator.restore(text).map_err(|error| error.to_string())
}

/// How many cases `restore` answers as the fixture expects.
fn matching_count(
    cases: &[Case],
    mut restore: impl FnMut(&str) -> Result<String, Failure>,
) -> Result<usize, Failure> {
    let mut matching = 0;
    for case in cases {
        if restore(&case.input)? == case.expected {
            matching += 1;
        }
    }
    Ok(matching)
}

/// Design H33: the cases the CPU provider answers as the fixture expects,
/// recorded (run with `--nocapture` to read the count). Not a pass line: the
/// count is a measurement of how far fp32 on the CPU differs from the GPU.
#[test]
fn cpu_punctuation_match_count_is_recorded() -> TestResult {
    let all = cases()?;
    let matching = matching_count(&all, restore_on_cpu)?;
    eprintln!(
        "CPU punctuation: {matching} of {} cases equal `expected`",
        all.len()
    );
    assert!(matching > 0, "no case matched on the CPU");
    Ok(())
}

/// T-RP-18-001
#[test]
fn parity_fixture_has_51_cases() -> TestResult {
    assert_eq!(cases()?.len(), 51);
    Ok(())
}

/// T-RP-18-002
#[test]
fn input_ids_match_for_every_case() -> TestResult {
    let tokenizer = tokenizer()?;
    check_input_ids(&cases()?, |text| tokenizer.encode(text).ids)?;
    Ok(())
}

/// T-RP-18-003
#[test]
fn first_case_input_ids_match() -> TestResult {
    let tokenizer = tokenizer()?;
    let all = cases()?;
    let Some(first) = all.first() else {
        return Err("the fixture has no case".into());
    };
    assert_eq!(tokenizer.encode(&first.input).ids, first.input_ids);
    Ok(())
}

/// T-RP-18-004: the same check with the normalisation left out finds a
/// difference, so the check can see a tokenizer that skips NFKC.
#[test]
fn tokenizer_without_nfkc_differs_on_some_case() -> TestResult {
    let tokenizer = tokenizer()?;
    let all = cases()?;
    let skipping = |text: &str| {
        let chars: Vec<char> = text.chars().filter(|c| !c.is_whitespace()).collect();
        tokenizer.ids_for(&chars)
    };
    let differing = mismatching_ids(&all, skipping);
    assert!(!differing.is_empty(), "no case notices a missing NFKC");
    assert!(check_input_ids(&all, skipping).is_err());
    Ok(())
}

/// T-RP-18-005
#[test]
fn gpu_punctuation_matches_expected_for_every_case() -> TestResult {
    check_expected(&cases()?, restore_on_gpu)?;
    Ok(())
}

/// The acceptance test of RP-18: ids and strings, all cases.
#[test]
fn rp18_punctuation_matches_parity_fixture() -> TestResult {
    let tokenizer = tokenizer()?;
    let all = cases()?;
    check_input_ids(&all, |text| tokenizer.encode(text).ids)?;
    check_expected(&all, restore_on_gpu)?;
    Ok(())
}

/// A string that differs is reported with the case, so the check can fail.
#[test]
fn a_restored_string_that_differs_is_named() {
    let all = vec![Case {
        name: String::from("only"),
        input: String::from("x"),
        input_ids: vec![],
        expected: String::from("x。"),
    }];
    let said = check_expected(&all, |_| Ok(String::from("x")));
    assert!(
        said.as_ref().is_err_and(|text| text.contains("only")),
        "{said:?}"
    );
}
