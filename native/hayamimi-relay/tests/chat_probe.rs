//! The relay asks T the way the model card does, through the server's chat
//! endpoint: the 40 sentences of the probe run through the relay's T give the
//! same translation as the same request sent with `curl`, line for line.
//!
//! The negative control sends the same sentences to the old `/completion`
//! endpoint with the raw prompt; some of those answers differ from `curl`.
//!
//! The tests that ask a server need a llama-server with a CAT-Translate model
//! on the design's port 8080 (or the URL in `testdata/chat_probe_server.txt`)
//! and the probe files, so they run with `--ignored`. One slot is enough.
//! The test of the request body needs neither.

mod support;

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

use hayamimi_relay::lang::Lang;
use hayamimi_relay::llm::{Completion, HttpLlm, Llm, request_body};
use hayamimi_relay::pipeline::translate_prompt;
use serde_json::{Value, json};
use support::TestResult;

/// The design's address of the translation server (`rust_port.ja.md` 3.6).
const DEFAULT_SERVER: &str = "http://127.0.0.1:8080";
const MAX_TOKENS: u32 = 256;

/// The server to ask: the URL in `testdata/chat_probe_server.txt` when that
/// file is there (a server started on another port), the design's address
/// when it is not.
fn server() -> Result<String, Box<dyn std::error::Error>> {
    let file = testdata()?.join("chat_probe_server.txt");
    if file.is_file() {
        Ok(std::fs::read_to_string(file)?.trim().to_owned())
    } else {
        Ok(String::from(DEFAULT_SERVER))
    }
}

/// The body `curl` sends for the request of the model card.
fn curl_body(prompt: &str) -> String {
    json!({
        "messages": [{"role": "user", "content": prompt}],
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

/// What `curl` gets back for the request of the model card.
fn curl_answer(prompt: &str) -> Result<String, Box<dyn std::error::Error>> {
    let body = curl_body(prompt);
    let out = Command::new("curl")
        .args([
            "-s",
            "--max-time",
            "30",
            "-H",
            "Content-Type: application/json",
        ])
        .args(["-d", &body, &format!("{}/v1/chat/completions", server()?)])
        .output()?;
    let value: Value = serde_json::from_slice(&out.stdout)?;
    value["choices"][0]["message"]["content"]
        .as_str()
        .map(|text| text.trim().to_owned())
        .ok_or_else(|| format!("curl got {value}").into())
}

/// What the old form gets: the raw prompt on `/completion`.
fn completion_answer(prompt: &str) -> Result<String, Box<dyn std::error::Error>> {
    let body = json!({
        "prompt": prompt, "n_predict": MAX_TOKENS, "temperature": 0, "stop": ["</s>"],
    })
    .to_string();
    let out = Command::new("curl")
        .args([
            "-s",
            "--max-time",
            "30",
            "-H",
            "Content-Type: application/json",
        ])
        .args(["-d", &body, &format!("{}/completion", server()?)])
        .output()?;
    let value: Value = serde_json::from_slice(&out.stdout)?;
    value["content"]
        .as_str()
        .map(|text| text.trim().to_owned())
        .ok_or_else(|| format!("completion got {value}").into())
}

/// The ids whose answer differs between two ways of asking.
fn differing(
    sentences: &[Sentence],
    ask: impl Fn(&str) -> Result<String, Box<dyn std::error::Error>>,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut ids = Vec::new();
    for sentence in sentences {
        let prompt = translate_prompt(sentence.lang, &sentence.text);
        // The server's answer to a prompt it has not seen differs in the last
        // digits from its answer to one it has cached, and that can change a
        // word. One request first, so that both sides ask about a cached prompt.
        curl_answer(&prompt)?;
        if ask(&prompt)? != curl_answer(&prompt)? {
            ids.push(sentence.id.clone());
        }
    }
    Ok(ids)
}

#[test]
#[ignore = "needs a llama-server with CAT-Translate on port 8080"]
fn chat_t_matches_curl_on_every_probe_sentence() -> TestResult {
    let all = sentences()?;
    assert_eq!(all.len(), 40);
    let relay = HttpLlm::new(&server()?);
    let ask = |prompt: &str| {
        relay
            .complete(&Completion::new(prompt, MAX_TOKENS))
            .map(|text| text.trim().to_owned())
            .map_err(Box::from)
    };
    let different = differing(&all, ask)?;
    eprintln!(
        "chat: {} of {} differ from curl",
        different.len(),
        all.len()
    );
    assert!(different.is_empty(), "{different:?}");
    Ok(())
}

#[test]
#[ignore = "needs a llama-server with CAT-Translate on port 8080"]
fn the_old_completion_form_differs_from_curl_on_some_probe_sentence() -> TestResult {
    let all = sentences()?;
    let different = differing(&all, completion_answer)?;
    eprintln!(
        "completion: {} of {} differ from curl",
        different.len(),
        all.len()
    );
    assert!(!different.is_empty());
    Ok(())
}

/// The answers of the probe log, kept as a record: how many of the 40 the
/// relay's T reproduces. Nothing is asserted about the count.
#[test]
#[ignore = "needs a llama-server with CAT-Translate on port 8080"]
fn record_agreement_with_the_probe_log() -> TestResult {
    let all = sentences()?;
    let log = std::fs::read_to_string(testdata()?.join("probe1_cat-translate-3.3b.log"))?;
    let relay = HttpLlm::new(&server()?);
    let mut same = 0;
    for sentence in &all {
        let wanted = log
            .lines()
            .filter_map(|line| line.strip_prefix(sentence.id.as_str()))
            .filter_map(|rest| rest.strip_prefix('\t'))
            .filter_map(|rest| rest.split('\t').nth(3))
            .map(|text| text.trim_end_matches("</s>").trim())
            .next();
        let got = relay.complete(&Completion::new(
            &translate_prompt(sentence.lang, &sentence.text),
            MAX_TOKENS,
        ))?;
        if wanted == Some(got.trim()) {
            same += 1;
        }
    }
    // Written to the stderr of the process, which the test harness does not
    // capture, so that the count is in the output of a passing run too.
    writeln!(
        std::io::stderr(),
        "probe1 log: {same} of {} equal",
        all.len()
    )?;
    Ok(())
}

/// No server: the body the relay sends is the body `curl` sends.
#[test]
fn the_relay_sends_the_body_curl_sends() -> TestResult {
    let sentences = [
        translate_prompt(Lang::Ja, "こんにちは。"),
        translate_prompt(Lang::En, "I paid $100."),
    ];
    for prompt in &sentences {
        let relay: Value =
            serde_json::from_str(&request_body(&Completion::new(prompt, MAX_TOKENS))?)?;
        let curl: Value = serde_json::from_str(&curl_body(prompt))?;
        assert_eq!(relay, curl, "{prompt}");
    }
    Ok(())
}

/// The negative control of the test above: a body that differs in one item
/// is not the body `curl` sends.
#[test]
fn a_body_with_another_temperature_is_not_the_body_curl_sends() -> TestResult {
    let prompt = translate_prompt(Lang::Ja, "こんにちは。");
    let mut relay: Value =
        serde_json::from_str(&request_body(&Completion::new(&prompt, MAX_TOKENS))?)?;
    relay["temperature"] = json!(0.7);
    let curl: Value = serde_json::from_str(&curl_body(&prompt))?;
    assert_ne!(relay, curl);
    Ok(())
}
