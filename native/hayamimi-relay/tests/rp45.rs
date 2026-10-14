//! RP-45: the labels of E's prompt (`English:`, `Draft Japanese:`,
//! `Rewritten Japanese:`, and the same in the other direction) never reach
//! the subtitle. The text behind the last label is what V sees; an answer
//! that is empty behind its labels is withheld for `form`.
//!
//! A fake llama-server answers T with one text and E with another, over the
//! same HTTP path the relay uses. V is not made stricter: a correct
//! translation with Latin letters or a line break still goes out.

mod support;

use hayamimi_relay::edit_answer::translation_of;
use hayamimi_relay::lang::Lang;
use hayamimi_relay::llm::HttpLlm;
use hayamimi_relay::logging::MemorySink;
use hayamimi_relay::pipeline::{
    LineKind, LineRequest, NoObserver, Pass, Pipeline, PipelineConfig, Status, Translation,
};
use hayamimi_relay::verify::HoldReason;
use support::{FakeServer, Reply, TestResult};

const EN_SRC: &str = "He went inside and examined his letters, but there was nothing from Dana.";
const EN_DRAFT: &str = "彼は中に入って手紙を調べたが、ダナからのものは何もなかった。";
const EN_EDITED: &str = "彼は中に入って手紙を確認したが、ダナからは何も届いていなかった。";

const JA_SRC: &str = "彼は中に入って手紙を確認した。";
const JA_DRAFT: &str = "He went inside and checked his letters.";
const JA_EDITED: &str = "He went in and checked his mail.";

/// Words of the prompt's labels that must not stay in a subtitle.
const LABEL_WORDS: [&str; 5] = ["Draf", "Rewrit", "English:", "Japanese:", "Japaese"];

fn carries_a_label(text: &str) -> bool {
    LABEL_WORDS.iter().any(|word| text.contains(word))
}

/// The settled translation of one line when the server answers T with
/// `draft` and E with `edited`, and the events logged on the way.
fn settle(
    lang: Lang,
    src: &str,
    draft: &str,
    edited: &str,
) -> Result<(Translation, MemorySink), Box<dyn std::error::Error>> {
    let (draft_body, edited_body) = (support::chat_reply(draft), support::chat_reply(edited));
    let server = FakeServer::start(move |n, _| {
        Reply::json(if n == 0 { &draft_body } else { &edited_body })
    })?;
    let mut sink = MemorySink::default();
    let mut pipeline = Pipeline::open(
        HttpLlm::new(&server.base_url()),
        &PipelineConfig::default(),
        &mut sink,
    )?;
    let request = LineRequest {
        id: "rp45-1",
        of: LineKind::Final,
        source: "file",
        src_lang: lang,
        text: src,
        now_epoch: support::epoch(support::DEFAULT_NOW)?,
    };
    let line = pipeline.run(&request, &mut NoObserver, &mut sink);
    Ok((line, sink))
}

/// How many times E's translation was withheld and T's kept.
fn edit_held_count(sink: &MemorySink, reason: &str) -> usize {
    sink.events
        .iter()
        .filter(|event| event.event == "translate.edit.held")
        .filter(|event| event.fields.get("hold_reason") == Some(&serde_json::json!(reason)))
        .count()
}

/// The shapes a model wrote back, from the sections of the prompt.
fn echoed_shapes() -> [String; 6] {
    [
        format!(
            "English:\n{EN_SRC}\n\nDraft Japanese:\n{EN_DRAFT}\n\nRewritten Japanese:\n{EN_EDITED}"
        ),
        format!(
            "{EN_SRC}  \n\nDrafted Japanese:  \n{EN_DRAFT}  \n\nRewritten Japanese:  \n{EN_EDITED}"
        ),
        format!(
            "English:\n{EN_SRC}\n\nDraf Japaese:\n{EN_DRAFT}\n\nRewritten Japaese:\n{EN_EDITED}"
        ),
        format!("Rewritten Japanese:\n{EN_EDITED}"),
        format!("Draft Japanese:\n{EN_DRAFT}\n\nRewritten Japanese: {EN_EDITED}\n"),
        format!("{EN_SRC}\n\nDraft Japanese：\n{EN_DRAFT}\n\nRewritten Japanese：\n\n{EN_EDITED}"),
    ]
}

/// T-RP-45-001: whichever sections the answer writes back, the settled
/// translation is the text behind the last label and carries no label.
#[test]
fn rp45_edit_template_never_reaches_the_subtitle() -> TestResult {
    for answer in echoed_shapes() {
        let (line, _) = settle(Lang::En, EN_SRC, EN_DRAFT, &answer)?;
        assert_eq!(line.status, Status::Ok, "{answer}");
        assert_eq!(line.stage, Pass::E, "{answer}");
        assert_eq!(line.text, EN_EDITED, "{answer}");
        assert!(!carries_a_label(&line.text), "{answer}");
    }
    Ok(())
}

/// T-RP-45-002: into English the labels are `Japanese:`,
/// `Draft English:` and `Rewritten English:`.
#[test]
fn rp45_the_labels_are_stripped_into_english_too() -> TestResult {
    let answers = [
        format!(
            "Japanese:\n{JA_SRC}\n\nDraft English:\n{JA_DRAFT}\n\nRewritten English:\n{JA_EDITED}"
        ),
        format!(
            "{JA_SRC}  \n\nDrafted English:  \n{JA_DRAFT}  \n\nRewritten English:  \n{JA_EDITED}"
        ),
    ];
    for answer in answers {
        let (line, _) = settle(Lang::Ja, JA_SRC, JA_DRAFT, &answer)?;
        assert_eq!(line.status, Status::Ok, "{answer}");
        assert_eq!(line.stage, Pass::E, "{answer}");
        assert_eq!(line.text, JA_EDITED, "{answer}");
    }
    Ok(())
}

/// T-RP-45-003, the control: an answer without labels is the translation as
/// it stands.
#[test]
fn rp45_an_answer_without_labels_is_unchanged() -> TestResult {
    let (line, sink) = settle(Lang::En, EN_SRC, EN_DRAFT, &format!("  {EN_EDITED}\n"))?;
    assert_eq!((line.status, line.stage), (Status::Ok, Pass::E));
    assert_eq!(line.text, EN_EDITED);
    assert_eq!(edit_held_count(&sink, "form"), 0);
    Ok(())
}

/// T-RP-45-004: the model wrote the source language behind the last label.
/// V withholds it for `form`, and T's translation, which passed, stays.
#[test]
fn rp45_the_wrong_language_behind_the_label_is_withheld_for_form() -> TestResult {
    let answer = format!(
        "{EN_SRC}\n\nDraft Japaese:\n{EN_DRAFT}\n\nRewritten Japaese:\n\nHe went in and read his letters."
    );
    let (line, sink) = settle(Lang::En, EN_SRC, EN_DRAFT, &answer)?;
    assert_eq!((line.status, line.stage), (Status::Ok, Pass::T));
    assert_eq!(line.text, EN_DRAFT);
    assert_eq!(edit_held_count(&sink, "form"), 1);
    Ok(())
}

/// T-RP-45-005: labels with nothing behind them are withheld for `form`;
/// when T's translation did not pass either, the line is held.
#[test]
fn rp45_an_answer_empty_behind_its_labels_is_withheld_for_form() -> TestResult {
    let answer =
        format!("English:\n{EN_SRC}\n\nDraft Japanese:\n{EN_DRAFT}\n\nRewritten Japanese:\n");
    let (kept, sink) = settle(Lang::En, EN_SRC, EN_DRAFT, &answer)?;
    assert_eq!((kept.status, kept.stage), (Status::Ok, Pass::T));
    assert_eq!(edit_held_count(&sink, "form"), 1);

    let (held, _) = settle(Lang::En, EN_SRC, "He went in.", &answer)?;
    assert_eq!(held.status, Status::Held);
    assert_eq!(held.hold_reason, Some(HoldReason::Form));
    assert_eq!(held.text, "");
    Ok(())
}

/// T-RP-45-006: a label inside a line cannot be cut off safely, so the
/// answer is withheld for `form` instead of going out with the template.
#[test]
fn rp45_a_label_left_inside_a_line_is_withheld_for_form() -> TestResult {
    let answer = format!("{EN_EDITED} Rewritten Japanese: {EN_EDITED}");
    let (line, sink) = settle(Lang::En, EN_SRC, EN_DRAFT, &answer)?;
    assert_eq!((line.status, line.stage), (Status::Ok, Pass::T));
    assert_eq!(line.text, EN_DRAFT);
    assert_eq!(edit_held_count(&sink, "form"), 1);
    Ok(())
}

/// T-RP-45-007: V is not made stricter. A correct translation with Latin
/// letters, an abbreviation and a line break goes out as E's.
#[test]
fn rp45_latin_letters_and_a_line_break_in_a_correct_translation_still_pass() -> TestResult {
    let src = "He talked to Dana about the NASA plan.";
    let draft = "彼はNASAの計画についてダナと話した。";
    let edited = "彼はダナと\nNASAの計画について話した。";
    let (line, sink) = settle(Lang::En, src, draft, edited)?;
    assert_eq!((line.status, line.stage), (Status::Ok, Pass::E));
    assert_eq!(line.text, edited);
    assert_eq!(edit_held_count(&sink, "form"), 0);
    Ok(())
}

/// T-RP-45-008: a sentence with a colon is not read as a label.
#[test]
fn rp45_a_colon_in_a_sentence_is_not_a_label() {
    for text in [
        "彼は言った。理由は次の通り: 雨だった。",
        "The draft was late: nobody came.",
        "Draft 3: the plan",
    ] {
        assert_eq!(translation_of(text, "English"), text);
    }
}

const RECORD: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/results/r8h_verify/e2e2c.events.jsonl"
);

fn has_japanese(text: &str) -> bool {
    text.chars()
        .any(|c| matches!(c, '\u{3040}'..='\u{30FF}' | '\u{4E00}'..='\u{9FFF}'))
}

/// The `text` of every E translation of a final line, in the record.
fn recorded_edit_texts() -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut texts = Vec::new();
    for line in std::fs::read_to_string(RECORD)?.lines() {
        let row: serde_json::Value = serde_json::from_str(line)?;
        let Some(event) = row.get("event") else {
            continue;
        };
        let is_edit = event.get("type") == Some(&serde_json::json!("translation"))
            && event.get("of") == Some(&serde_json::json!("final"))
            && event.get("stage") == Some(&serde_json::json!("e"));
        let Some(text) = event.get("text").and_then(serde_json::Value::as_str) else {
            continue;
        };
        if is_edit {
            texts.push(text.to_owned());
        }
    }
    Ok(texts)
}

/// T-RP-45-009 (reads the E2E-2 record; run with `--ignored --nocapture`):
/// the E translations of the record before and after the labels are cut.
#[test]
#[ignore = "reads docs/results/r8h_verify, a record of an earlier run"]
fn rp45_recorded_edit_answers_lose_their_labels() -> TestResult {
    let texts = recorded_edit_texts()?;
    let before = texts.iter().filter(|text| carries_a_label(text)).count();
    let stripped: Vec<&str> = texts
        .iter()
        .map(|text| translation_of(text, "English"))
        .collect();
    let after = stripped.iter().filter(|text| carries_a_label(text)).count();
    let empty = stripped
        .iter()
        .zip(&texts)
        .filter(|(text, original)| text.is_empty() && !original.is_empty())
        .count();
    let no_japanese = stripped
        .iter()
        .filter(|text| !text.is_empty() && !has_japanese(text))
        .count();
    println!(
        "e_texts={} with_label_before={before} with_label_after={after} \
         emptied_by_strip={empty} nonempty_without_japanese={no_japanese}",
        texts.len()
    );
    assert_eq!(texts.len(), 35);
    assert_eq!(before, 20);
    assert_eq!(after, 0);
    Ok(())
}
