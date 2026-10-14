//! RP-39: the number check of V reads the scale words that follow digits in
//! Chinese, Korean and Spanish, so a translation that keeps the amount goes
//! out and one that changes it is held for `number`.
//!
//! The reader is called directly for the values, and a fake llama-server
//! answers every request with one text to run lines through V.

mod support;

use hayamimi_relay::lang::Lang;
use hayamimi_relay::llm::HttpLlm;
use hayamimi_relay::logging::MemorySink;
use hayamimi_relay::numbers::{MICRO, Tongue, compared_values};
use hayamimi_relay::pipeline::{
    LineKind, LineRequest, Pipeline, PipelineConfig, Status, Translation,
};
use hayamimi_relay::target::Target;
use hayamimi_relay::verify::HoldReason;
use support::{FakeServer, Reply, TestResult};

const BUDGET: &str = "予算は五百万円です。";

/// Per language: a text with digits and scale words, and the value it names.
const READINGS: [(&str, &str, u128); 12] = [
    ("zh", "预算是5百万日元。", 5_000_000),
    ("zh", "共3千亿。", 300_000_000_000),
    ("zh", "约1.5亿人。", 150_000_000),
    ("zh", "共500万人。", 5_000_000),
    ("ko", "예산은 5백만 엔이에요.", 5_000_000),
    ("ko", "예산은 500만 엔이에요.", 5_000_000),
    ("ko", "3억 명이에요.", 300_000_000),
    ("ko", "2천 명이에요.", 2_000),
    ("es", "El presupuesto es de 5 millones de yenes.", 5_000_000),
    ("es", "Hay 1 millón de personas.", 1_000_000),
    ("es", "Hay 2 mil personas.", 2_000),
    ("es", "Hay 3 mil millones de personas.", 3_000_000_000),
];

/// The one value of 2 or more that `text` names, in whole units.
fn value_of(code: &str, text: &str) -> Vec<u128> {
    compared_values(text, Tongue::of_code(code))
        .into_iter()
        .map(|micro| micro / MICRO)
        .collect()
}

fn run_line(code: &str, said: &str) -> Result<Translation, Box<dyn std::error::Error>> {
    run_source(BUDGET, code, said)
}

/// The Japanese line `src` into `code` against a server that answers `said`.
fn run_source(
    src: &str,
    code: &str,
    said: &str,
) -> Result<Translation, Box<dyn std::error::Error>> {
    let Some(target) = Target::from_code(code) else {
        return Err(format!("no target {code}").into());
    };
    let body = support::chat_reply(said);
    let server = FakeServer::start(move |_, _| Reply::json(&body))?;
    let mut sink = MemorySink::default();
    let mut pipeline = Pipeline::open(
        HttpLlm::new(&server.base_url()),
        &PipelineConfig::default(),
        &mut sink,
    )?;
    let request = LineRequest {
        id: "rp39-1",
        of: LineKind::Final,
        source: "file",
        src_lang: Lang::Ja,
        text: src,
        now_epoch: support::epoch(support::DEFAULT_NOW)?,
    };
    Ok(pipeline.run_into(&request, &target, &mut |_| {}, &mut sink))
}

/// T-RP-39-001: the examples of the design read as the numbers they name,
/// and the three translations of the budget go out.
#[test]
fn rp39_number_scales_read_in_zh_ko_es() -> TestResult {
    for (code, text, whole) in READINGS {
        assert_eq!(value_of(code, text), [whole], "{code}: {text}");
    }
    let kept = [
        ("zh", "预算是5百万日元。"),
        ("ko", "예산은 5백만 엔이에요."),
        ("es", "El presupuesto es de 5 millones de yenes."),
    ];
    for (code, said) in kept {
        let line = run_line(code, said)?;
        assert_eq!(
            line.status,
            Status::Ok,
            "{code}: held for {:?}",
            line.hold_reason
        );
    }
    Ok(())
}

/// T-RP-39-002: a changed amount is held for `number`.
#[test]
fn rp39_a_changed_amount_is_held_for_number() -> TestResult {
    let changed = [
        ("zh", "预算是7百万日元。"),
        ("ko", "예산은 7백만 엔이에요."),
        ("es", "El presupuesto es de 7 millones de yenes."),
    ];
    for (code, said) in changed {
        let line = run_line(code, said)?;
        assert_eq!(line.status, Status::Held, "{code}: status");
        assert_eq!(line.hold_reason, Some(HoldReason::Number), "{code}: reason");
    }
    Ok(())
}

/// T-RP-39-003, the control: Japanese and English keep their reading.
#[test]
fn rp39_japanese_and_english_keep_their_reading() {
    assert_eq!(value_of("ja", "500万円です。"), [5_000_000]);
    assert_eq!(value_of("ja", BUDGET), [5_000_000]);
    assert_eq!(value_of("en", "It costs 5 million yen."), [5_000_000]);
    assert_eq!(value_of("en", "It costs 5 thousand yen."), [5_000]);
}

/// T-RP-39-005: a Japanese source with digits and a han unit and its Chinese
/// translation name the same amount, so the line goes out.
#[test]
fn rp39_japanese_digits_with_a_han_unit_match_the_chinese_translation() -> TestResult {
    assert_eq!(value_of("ja", "予算は5千円です。"), [5_000]);
    assert_eq!(value_of("ja", "3百円です。"), [300]);
    let rows = [
        ("予算は5千円です。", "预算是5千日元。"),
        ("3百円です。", "3百日元。"),
    ];
    for (src, said) in rows {
        let line = run_source(src, "zh", said)?;
        assert_eq!(
            line.status,
            Status::Ok,
            "{src}: held for {:?}",
            line.hold_reason
        );
    }
    let line = run_source("予算は5千円です。", "zh", "预算是6千日元。")?;
    assert_eq!(line.hold_reason, Some(HoldReason::Number));
    Ok(())
}

/// T-RP-39-006: the 百 of `百分点` is not a factor of the digit before it.
#[test]
fn rp39_the_hundred_of_a_percentage_word_is_not_a_factor() {
    assert_eq!(value_of("zh", "上升5百分点。"), [5, 100]);
    assert_eq!(value_of("ja", "5百分点上がった。"), [5, 100]);
}

/// T-RP-39-004: a word that only begins like a scale word is not one.
#[test]
fn rp39_words_that_only_look_like_scales_are_not_read() {
    assert_eq!(value_of("es", "Hay 5 milímetros."), [5]);
    assert_eq!(value_of("es", "Hay 5 millas."), [5]);
}
