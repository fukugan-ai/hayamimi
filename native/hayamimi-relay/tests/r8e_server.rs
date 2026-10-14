//! What the relay relies on from a llama-server with four slots and the
//! translation model on it, measured against the real server:
//!
//! * H6: when the client closes a streamed request after its first token, the
//!   server frees that slot within one second. The control is the same request
//!   left open, whose slot stays busy while the answer is still being made.
//! * `cache_prompt`: the same request sent twice reuses the prompt the second
//!   time (`timings.cache_n` above zero).
//!
//! The tests that ask the server need it on the design's port 8080 with
//! `-np 4` (or the default, which is four slots), so they run with `--ignored`.
//! The reading of `timings` needs no server.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use hayamimi_relay::lang::Lang;
use hayamimi_relay::llm::{Completion, request_body};
use hayamimi_relay::pipeline::translate_prompt;
use serde_json::{Value, json};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const HOST: &str = "127.0.0.1:8080";
const BASE: &str = "http://127.0.0.1:8080";
/// The slot has to be free this soon after the client hung up.
const FREED_WITHIN: Duration = Duration::from_secs(1);
/// How often `/slots` is read while waiting for a slot to change.
const POLL: Duration = Duration::from_millis(20);
/// An answer this long keeps a slot busy for seconds at the model's speed.
const LONG_ANSWER_TOKENS: u32 = 400;

/// The `cache_n` of the `timings` of a chat answer.
fn cache_n_of(answer: &str) -> Result<u64, String> {
    let value: Value = serde_json::from_str(answer).map_err(|error| error.to_string())?;
    value
        .get("timings")
        .and_then(|timings| timings.get("cache_n"))
        .and_then(Value::as_u64)
        .ok_or_else(|| String::from("the answer has no timings.cache_n"))
}

/// A source long enough that its translation takes seconds.
fn long_source() -> String {
    [
        "今日は朝から天気が良かったので、友達と一緒に近くの公園まで散歩に出かけました。",
        "公園の池にはたくさんの鴨がいて、子供たちがパンを細かくちぎって投げていました。",
        "昼になると私たちは木の下に座り、家から持ってきたおにぎりと卵焼きを食べました。",
        "午後には急に風が強くなり、空が暗くなってきたので、私たちは急いで駅へ向かいました。",
        "駅に着いたときにはもう雨が降り始めていて、電車は少し遅れて到着しました。",
        "帰りの電車の中で、私は今日の出来事を日記に書こうと決めて、携帯電話にメモを取りました。",
    ]
    .concat()
}

fn ask(path: &str, body: Option<String>) -> Result<String, Box<dyn std::error::Error>> {
    let agent = ureq::Agent::new_with_defaults();
    let url = format!("{BASE}{path}");
    let mut response = match body {
        Some(body) => agent
            .post(&url)
            .header("Content-Type", "application/json")
            .send(body)?,
        None => agent.get(&url).call()?,
    };
    Ok(response.body_mut().read_to_string()?)
}

/// For each slot, whether it is working on a request.
fn busy_slots() -> Result<Vec<bool>, Box<dyn std::error::Error>> {
    let slots: Value = serde_json::from_str(&ask("/slots", None)?)?;
    let list = slots.as_array().ok_or("/slots is not a list")?;
    list.iter()
        .map(|slot| {
            slot.get("is_processing")
                .and_then(Value::as_bool)
                .ok_or_else(|| "a slot has no is_processing".into())
        })
        .collect()
}

/// Waits until `wanted(busy_count)` holds; the time it took, or `None` when
/// `limit` passed first.
fn wait_for_busy(
    limit: Duration,
    wanted: impl Fn(usize) -> bool,
) -> Result<Option<Duration>, Box<dyn std::error::Error>> {
    let started = Instant::now();
    loop {
        let busy = busy_slots()?.iter().filter(|slot| **slot).count();
        if wanted(busy) {
            return Ok(Some(started.elapsed()));
        }
        if started.elapsed() >= limit {
            return Ok(None);
        }
        std::thread::sleep(POLL);
    }
}

/// A streamed request, sent by hand so that the test owns the connection.
fn open_stream() -> Result<BufReader<TcpStream>, Box<dyn std::error::Error>> {
    let prompt = translate_prompt(Lang::Ja, &long_source());
    let body = json!({
        "messages": [{"role": "user", "content": prompt}],
        "temperature": 0.0, "max_tokens": LONG_ANSWER_TOKENS, "stream": true,
        "cache_prompt": true,
    })
    .to_string();
    let mut stream = TcpStream::connect(HOST)?;
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    write!(
        stream,
        "POST /v1/chat/completions HTTP/1.1\r\nHost: {HOST}\r\nContent-Type: application/json\r\n\
         Connection: close\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )?;
    Ok(BufReader::new(stream))
}

/// Reads until a piece with text arrives: the first token.
fn read_first_token(reader: &mut BufReader<TcpStream>) -> Result<(), Box<dyn std::error::Error>> {
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Err("the stream ended before a token".into());
        }
        let Some(data) = line.trim().strip_prefix("data:") else {
            continue;
        };
        let chunk: Value = serde_json::from_str(data.trim())?;
        let text = chunk["choices"][0]["delta"]["content"].as_str();
        if text.is_some_and(|piece| !piece.is_empty()) {
            return Ok(());
        }
    }
}

fn require_idle_server() -> TestResult {
    let freed = wait_for_busy(Duration::from_secs(10), |busy| busy == 0)?;
    freed.ok_or("the server's slots were busy for 10 s before the test began")?;
    Ok(())
}

/// H6, the control: left open, the request keeps its slot busy while the
/// answer is made, and the slot is free once the answer is complete.
#[test]
#[ignore = "needs a llama-server with CAT-Translate and four slots on port 8080"]
fn an_open_stream_keeps_its_slot_busy_until_the_answer_ends() -> TestResult {
    require_idle_server()?;
    let mut reader = open_stream()?;
    read_first_token(&mut reader)?;
    std::thread::sleep(Duration::from_millis(300));
    let busy = busy_slots()?.iter().filter(|slot| **slot).count();
    assert_eq!(
        busy, 1,
        "one slot should be working 300 ms after the first token"
    );
    let mut rest = String::new();
    reader.read_to_string(&mut rest)?;
    assert!(rest.contains("[DONE]"), "the stream did not reach its end");
    let freed = wait_for_busy(FREED_WITHIN, |busy| busy == 0)?;
    assert!(
        freed.is_some(),
        "the slot stayed busy after the answer ended"
    );
    Ok(())
}

/// H6: closed after the first token, the slot is free within a second.
#[test]
#[ignore = "needs a llama-server with CAT-Translate and four slots on port 8080"]
fn closing_the_stream_after_the_first_token_frees_the_slot_within_a_second() -> TestResult {
    require_idle_server()?;
    let mut reader = open_stream()?;
    read_first_token(&mut reader)?;
    let busy = busy_slots()?.iter().filter(|slot| **slot).count();
    assert_eq!(
        busy, 1,
        "the request should hold one slot at its first token"
    );
    let closed = Instant::now();
    reader.get_ref().shutdown(std::net::Shutdown::Both)?;
    drop(reader);
    let freed = wait_for_busy(FREED_WITHIN, |busy| busy == 0)?;
    eprintln!(
        "h6: slot freed {:?} after the close",
        freed.map(|_| closed.elapsed())
    );
    assert!(
        freed.is_some(),
        "the slot was still busy 1 s after the close"
    );
    Ok(())
}

/// The second of two identical requests reuses the prompt the first computed.
#[test]
#[ignore = "needs a llama-server with CAT-Translate on port 8080"]
fn the_same_request_twice_reuses_the_prompt() -> TestResult {
    let prompt = translate_prompt(
        Lang::Ja,
        "今日は会議が三時からあります。資料を準備してください。",
    );
    let body = request_body(&Completion::new(&prompt, 64))?;
    ask("/v1/chat/completions", Some(body.clone()))?;
    let second = ask("/v1/chat/completions", Some(body))?;
    let cached = cache_n_of(&second)?;
    eprintln!("cache_prompt: second request cache_n = {cached}");
    assert!(cached > 0, "cache_n was {cached}");
    Ok(())
}

#[test]
fn the_cache_count_is_read_from_the_timings() {
    let answer = r#"{"choices":[],"timings":{"cache_n":52,"prompt_n":1}}"#;
    assert_eq!(cache_n_of(answer), Ok(52));
    assert_eq!(cache_n_of(r#"{"timings":{"cache_n":0}}"#), Ok(0));
}

/// The negative control: an answer without the count is an error, not zero.
#[test]
fn an_answer_without_timings_is_an_error() {
    assert!(cache_n_of(r#"{"choices":[]}"#).is_err());
    assert!(cache_n_of("not json").is_err());
}
