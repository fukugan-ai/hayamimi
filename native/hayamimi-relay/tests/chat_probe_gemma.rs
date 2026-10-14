//! The relay asks the second translator the way its chat template reads: the
//! 40 sentences of the TranslateGemma probe, sent through the relay's coded
//! request, give the same translation as the same request sent with `curl`,
//! line for line, and `curl` gives the answers of the probe log.
//!
//! The negative control sends the same sentences with the prompt of the live
//! pair's model and no language codes. The template then writes English (en-GB)
//! to English (en-GB) on both sides, and some answers differ from `curl`.
//!
//! The tests that ask a server need a llama-server with TranslateGemma on the
//! design's second port (18081), or at the URL in
//! `testdata/rp13_other_server.txt`, and the probe files, so they run with
//! `--ignored`. One slot is enough. The test of the request body needs neither.

mod support;

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

use hayamimi_relay::lang::Lang;
use hayamimi_relay::llm::{Completion, HttpLlm, Llm, request_body};
use hayamimi_relay::pipeline::translate_prompt;
use serde_json::{Value, json};
use support::TestResult;

/// The design's address of the second translator (`rust_port.ja.md` 3.6).
const DEFAULT_SERVER: &str = "http://127.0.0.1:18081";
const MAX_TOKENS: u32 = 256;

/// The server to ask: the URL in `testdata/rp13_other_server.txt` when that
/// file is there (a server started on another port), the design's address
/// when it is not.
fn server() -> Result<String, Box<dyn std::error::Error>> {
    let file = testdata()?.join("rp13_other_server.txt");
    if file.is_file() {
        Ok(std::fs::read_to_string(file)?.trim().to_owned())
    } else {
        Ok(String::from(DEFAULT_SERVER))
    }
}

/// The body `curl` sends: the text as the one user message, and the language
/// codes as template variables.
fn curl_body(text: &str, from: Lang) -> String {
    json!({
        "messages": [{"role": "user", "content": text}],
        "chat_template_kwargs": {
            "source_lang_code": from.code(),
            "target_lang_code": from.other().code(),
        },
        "temperature": 0.0, "max_tokens": MAX_TOKENS, "stop": ["</s>"], "cache_prompt": true,
    })
    .to_string()
}

/// One sentence of the probe: its id, language and text.
struct Sentence {
    id: String,
    lang: Lang,
    text: String,
}

/// The repository's `testdata/`: in the main tree, or from a worktree under
/// `.claude/worktrees/`.
fn testdata() -> Result<PathBuf, String> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    [
        manifest.join("../../testdata"),
        manifest.join("../../../../../testdata"),
    ]
    .into_iter()
    .find(|path| path.join("conv20.tsv").is_file())
    .ok_or_else(|| String::from("testdata/conv20.tsv was not found"))
}

fn sentences() -> Result<Vec<Sentence>, Box<dyn std::error::Error>> {
    let text = std::fs::read_to_string(testdata()?.join("conv20.tsv"))?;
    let mut out = Vec::new();
    for line in text.lines().skip(1) {
        let mut columns = line.split('\t');
        let (Some(id), Some(code), Some(source)) = (columns.next(), columns.next(), columns.next())
        else {
            return Err(format!("a row with fewer than three columns: {line}").into());
        };
        let lang = Lang::from_code(code).ok_or_else(|| format!("language {code}"))?;
        out.push(Sentence {
            id: id.to_owned(),
            lang,
            text: source.to_owned(),
        });
    }
    Ok(out)
}

/// What `curl` gets back for a chat body.
fn curl_post(body: &str) -> Result<String, Box<dyn std::error::Error>> {
    let out = Command::new("curl")
        .args([
            "-s",
            "--max-time",
            "30",
            "-H",
            "Content-Type: application/json",
        ])
        .args(["-d", body, &format!("{}/v1/chat/completions", server()?)])
        .output()?;
    let value: Value = serde_json::from_slice(&out.stdout)?;
    value["choices"][0]["message"]["content"]
        .as_str()
        .map(|text| text.trim().to_owned())
        .ok_or_else(|| format!("curl got {value}").into())
}

fn curl_answer(sentence: &Sentence) -> Result<String, Box<dyn std::error::Error>> {
    curl_post(&curl_body(&sentence.text, sentence.lang))
}

/// What the form without language codes gets: the live pair's prompt as the
/// one user message and no template variables.
fn uncoded_answer(sentence: &Sentence) -> Result<String, Box<dyn std::error::Error>> {
    let prompt = translate_prompt(sentence.lang, &sentence.text);
    let body = json!({
        "messages": [{"role": "user", "content": prompt}],
        "temperature": 0.0, "max_tokens": MAX_TOKENS, "stop": ["</s>"], "cache_prompt": true,
    })
    .to_string();
    curl_post(&body)
}

/// The ids whose answer differs between two ways of asking.
fn differing(
    sentences: &[Sentence],
    ask: impl Fn(&Sentence) -> Result<String, Box<dyn std::error::Error>>,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut ids = Vec::new();
    for sentence in sentences {
        // The server's answer to a prompt it has not seen differs in the last
        // digits from its answer to one it has cached, and that can change a
        // word. One request first, so that both sides ask about a cached prompt.
        curl_answer(sentence)?;
        if ask(sentence)? != curl_answer(sentence)? {
            ids.push(sentence.id.clone());
        }
    }
    Ok(ids)
}

/// The answer of the probe log for `id`, without the end marker.
fn logged_answer<'a>(log: &'a str, id: &str) -> Option<&'a str> {
    log.lines()
        .filter_map(|line| line.strip_prefix(id))
        .filter_map(|rest| rest.strip_prefix('\t'))
        .filter_map(|rest| rest.split('\t').nth(3))
        .map(|text| text.trim_end_matches("</s>").trim())
        .next()
}

#[test]
#[ignore = "needs a llama-server with TranslateGemma on the second port"]
fn coded_t_matches_curl_on_every_probe_sentence() -> TestResult {
    let all = sentences()?;
    assert_eq!(all.len(), 40);
    let relay = HttpLlm::new(&server()?);
    let ask = |sentence: &Sentence| {
        relay
            .complete(&Completion::coded(
                &sentence.text,
                sentence.lang.code(),
                sentence.lang.other().code(),
                MAX_TOKENS,
            ))
            .map(|text| text.trim().to_owned())
            .map_err(Box::from)
    };
    let different = differing(&all, ask)?;
    writeln!(
        std::io::stderr(),
        "coded: {} of {} differ from curl",
        different.len(),
        all.len()
    )?;
    assert!(different.is_empty(), "{different:?}");
    Ok(())
}

/// The positive control: `curl` gives the answers the probe log recorded.
/// A line that differs means the llama.cpp build is not the probe's; the count
/// is written, and the verdict is the relay's agreement with `curl`.
#[test]
#[ignore = "needs a llama-server with TranslateGemma on the second port"]
fn curl_matches_the_probe_log() -> TestResult {
    let all = sentences()?;
    let log = std::fs::read_to_string(testdata()?.join("probe1_translategemma-4b.log"))?;
    let mut apart = Vec::new();
    for sentence in &all {
        curl_answer(sentence)?;
        let got = curl_answer(sentence)?;
        if logged_answer(&log, &sentence.id) != Some(got.as_str()) {
            apart.push(sentence.id.clone());
        }
    }
    writeln!(
        std::io::stderr(),
        "probe1 log: {} of {} equal to curl, apart: {apart:?}",
        all.len() - apart.len(),
        all.len()
    )?;
    assert_eq!(logged_answer(&log, "j01"), Some("Hello."));
    Ok(())
}

#[test]
#[ignore = "needs a llama-server with TranslateGemma on the second port"]
fn the_form_without_codes_differs_from_curl_on_some_probe_sentence() -> TestResult {
    let all = sentences()?;
    let different = differing(&all, uncoded_answer)?;
    writeln!(
        std::io::stderr(),
        "uncoded: {} of {} differ from curl",
        different.len(),
        all.len()
    )?;
    assert!(!different.is_empty());
    Ok(())
}

/// No server: the body the relay sends is the body `curl` sends.
#[test]
fn the_relay_sends_the_body_curl_sends() -> TestResult {
    for (text, lang) in [("こんにちは。", Lang::Ja), ("I paid $100.", Lang::En)] {
        let request = Completion::coded(text, lang.code(), lang.other().code(), MAX_TOKENS);
        let relay: Value = serde_json::from_str(&request_body(&request)?)?;
        let curl: Value = serde_json::from_str(&curl_body(text, lang))?;
        assert_eq!(relay, curl, "{text}");
    }
    Ok(())
}

/// The negative control of the test above: codes written for the other
/// direction are not the body `curl` sends.
#[test]
fn a_body_with_the_codes_swapped_is_not_the_body_curl_sends() -> TestResult {
    let request = Completion::coded("こんにちは。", "en", "ja", MAX_TOKENS);
    let relay: Value = serde_json::from_str(&request_body(&request)?)?;
    let curl: Value = serde_json::from_str(&curl_body("こんにちは。", Lang::Ja))?;
    assert_ne!(relay, curl);
    Ok(())
}
