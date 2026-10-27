//! The llama-server client: one request, one answer, a fixed time limit.
//!
//! A request that carries a [`Wanted`] is streamed: the answer is read piece
//! by piece, and the connection is closed as soon as the answer is no longer
//! wanted, so that a newer request does not queue behind a useless one.

use std::io::{self, BufRead, BufReader};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde::ser::{SerializeSeq, Serializer};
use serde::{Deserialize, Serialize};

use crate::logging::{Level, LogEvent, LogSink, SharedSink};
use crate::verify::EOS_MARKER;

/// A request that has not been answered in this long counts as unavailable.
pub const REQUEST_TIMEOUT: Duration = Duration::from_millis(1500);

/// Why the translator gave no text.
#[derive(Debug)]
pub enum LlmError {
    /// The server could not be reached, or the exchange failed or timed out.
    /// The text opens with a word of [`CAUSES`] when the exchange with the
    /// server failed.
    Transport(String),
    /// The server answered with a status other than 2xx.
    Status(u16),
    /// The answer was not the JSON the relay expects.
    Body(String),
    /// The caller no longer wanted the answer, so the request was dropped.
    Aborted,
}

impl std::fmt::Display for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LlmError::Transport(why) => write!(f, "transport: {why}"),
            LlmError::Status(code) => write!(f, "status {code}"),
            LlmError::Body(why) => write!(f, "body: {why}"),
            LlmError::Aborted => write!(f, "aborted"),
        }
    }
}

impl std::error::Error for LlmError {}

/// The words a failed exchange is called in the log, so that no text of the
/// error (an address, a path) reaches a line. The last stands for any other
/// failure. The list is the table in `rust_port.ja.md` 3.23.
pub const CAUSES: [&str; 10] = [
    "unexpected_eof",
    "connection_reset",
    "connection_aborted",
    "broken_pipe",
    "connection_refused",
    "timed_out",
    "io_other",
    "timeout",
    "host_not_found",
    "other",
];

/// The `io::ErrorKind`s that say the server had closed the connection a
/// request was sent on.
const CLOSED_CONNECTION: [io::ErrorKind; 4] = [
    io::ErrorKind::UnexpectedEof,
    io::ErrorKind::ConnectionReset,
    io::ErrorKind::ConnectionAborted,
    io::ErrorKind::BrokenPipe,
];

/// The word of [`CAUSES`] for a failed exchange.
pub fn cause_of(error: &ureq::Error) -> &'static str {
    match error {
        ureq::Error::Io(failure) => match failure.kind() {
            io::ErrorKind::UnexpectedEof => "unexpected_eof",
            io::ErrorKind::ConnectionReset => "connection_reset",
            io::ErrorKind::ConnectionAborted => "connection_aborted",
            io::ErrorKind::BrokenPipe => "broken_pipe",
            io::ErrorKind::ConnectionRefused => "connection_refused",
            io::ErrorKind::TimedOut => "timed_out",
            _ => "io_other",
        },
        ureq::Error::Timeout(_) => "timeout",
        ureq::Error::HostNotFound => "host_not_found",
        _ => "other",
    }
}

/// Whether a failed exchange says the server had closed the connection the
/// request was sent on, so that the same request is worth sending once more.
pub fn asks_again(error: &ureq::Error) -> bool {
    matches!(error, ureq::Error::Io(failure) if CLOSED_CONNECTION.contains(&failure.kind()))
}

/// The word of [`CAUSES`] that a failure's text (`LlmError`'s `Display`)
/// carries: the word after `transport: `. A transport text that opens with
/// none of them is `other`, as the list says. A failure that is not a
/// transport failure has none.
pub fn cause_in(display: &str) -> Option<&'static str> {
    let why = display.strip_prefix("transport: ")?;
    let word = why.split(':').next()?;
    CAUSES
        .iter()
        .copied()
        .find(|known| *known == word)
        .or(Some("other"))
}

/// Tells a request in flight whether its answer is still wanted: it is until
/// the counter it was made from moves on, or for as long as a line has claimed
/// the answer (`pinned`).
#[derive(Clone, Debug)]
pub struct Wanted {
    latest: Arc<AtomicU64>,
    mine: u64,
    pinned: Option<Arc<AtomicBool>>,
}

impl Wanted {
    /// A request made when `latest` read `mine`.
    pub fn new(latest: Arc<AtomicU64>, mine: u64) -> Wanted {
        Wanted {
            latest,
            mine,
            pinned: None,
        }
    }

    /// A request like [`Wanted::new`] whose answer stays wanted once `pin` is
    /// set, whatever the counter reads.
    pub fn pinned(latest: Arc<AtomicU64>, mine: u64, pin: Arc<AtomicBool>) -> Wanted {
        Wanted {
            latest,
            mine,
            pinned: Some(pin),
        }
    }

    /// Whether the request is to go on: it is the newest of its source, or its
    /// answer was claimed.
    pub fn still(&self) -> bool {
        self.newest()
            || self
                .pinned
                .as_ref()
                .is_some_and(|pin| pin.load(Ordering::Acquire))
    }

    /// Whether the request is the newest of its source, claimed or not.
    pub fn newest(&self) -> bool {
        self.latest.load(Ordering::Acquire) == self.mine
    }

    /// The value the counter read when the request was made.
    pub fn generation(&self) -> u64 {
        self.mine
    }
}

/// One request to the translator: the text of the user message, an optional
/// system message before it, how many tokens the answer may take, and the
/// sampling the relay always uses.
#[derive(Debug)]
pub struct Completion<'a> {
    /// The text of the user message. The server applies the model's chat
    /// template to it.
    pub prompt: &'a str,
    /// The text of a system message that goes before the user message. T's
    /// request has none; E's carries the draft and what to keep in it.
    pub system: Option<&'a str>,
    /// The source and target language codes of a model whose chat template
    /// takes them as template variables (TranslateGemma). `prompt` is then
    /// the bare text to translate.
    pub codes: Option<(&'a str, &'a str)>,
    pub n_predict: u32,
    pub temperature: f32,
    /// Always the end-of-sequence marker, so it never reaches a subtitle.
    pub stop: [&'static str; 1],
    /// Set on a request that may be dropped while it runs.
    pub wanted: Option<&'a Wanted>,
}

/// The body of a llama-server `/v1/chat/completions` request.
#[derive(Serialize)]
struct ChatBody<'a> {
    messages: Messages<'a>,
    max_tokens: u32,
    temperature: f32,
    stop: [&'static str; 1],
    /// Reuses the computed part of a prompt that begins as the last one did.
    cache_prompt: bool,
    /// The variables the model's chat template reads besides the messages.
    #[serde(skip_serializing_if = "Option::is_none")]
    chat_template_kwargs: Option<TemplateCodes<'a>>,
    /// Asks for the answer piece by piece; left out when the answer is read whole.
    #[serde(skip_serializing_if = "is_false")]
    stream: bool,
}

fn is_false(flag: &bool) -> bool {
    !*flag
}

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'static str,
    content: &'a str,
}

/// The `messages` array of a request: the system message when there is one,
/// then the user message. Written without building a list.
struct Messages<'a> {
    system: Option<&'a str>,
    user: &'a str,
}

impl Serialize for Messages<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let length = 1 + usize::from(self.system.is_some());
        let mut list = serializer.serialize_seq(Some(length))?;
        if let Some(content) = self.system {
            list.serialize_element(&ChatMessage {
                role: "system",
                content,
            })?;
        }
        list.serialize_element(&ChatMessage {
            role: "user",
            content: self.user,
        })?;
        list.end()
    }
}

/// The language codes the chat template of TranslateGemma reads. The server
/// takes them only from here; written inside the message they are dropped
/// and the template falls back to en-GB on both sides.
#[derive(Serialize)]
struct TemplateCodes<'a> {
    source_lang_code: &'a str,
    target_lang_code: &'a str,
}

impl<'a> From<&Completion<'a>> for ChatBody<'a> {
    fn from(request: &Completion<'a>) -> ChatBody<'a> {
        ChatBody {
            messages: Messages {
                system: request.system,
                user: request.prompt,
            },
            max_tokens: request.n_predict,
            temperature: request.temperature,
            stop: request.stop,
            cache_prompt: true,
            chat_template_kwargs: request.codes.map(|(source, target)| TemplateCodes {
                source_lang_code: source,
                target_lang_code: target,
            }),
            stream: request.wanted.is_some(),
        }
    }
}

impl<'a> Completion<'a> {
    /// A greedy request that stops at the end-of-sequence marker.
    pub fn new(prompt: &'a str, n_predict: u32) -> Completion<'a> {
        Completion {
            prompt,
            system: None,
            codes: None,
            n_predict,
            temperature: 0.0,
            stop: [EOS_MARKER],
            wanted: None,
        }
    }

    /// Like [`Completion::new`] for a model whose chat template reads the
    /// language codes `from` and `to` as template variables.
    pub fn coded(text: &'a str, from: &'a str, to: &'a str, n_predict: u32) -> Completion<'a> {
        Completion {
            codes: Some((from, to)),
            ..Completion::new(text, n_predict)
        }
    }

    /// The same request with a system message before the user message.
    pub fn with_system(mut self, system: &'a str) -> Completion<'a> {
        self.system = Some(system);
        self
    }

    /// The same request, dropped as soon as `wanted` says its answer is not
    /// needed any more.
    pub fn wanted(mut self, wanted: &'a Wanted) -> Completion<'a> {
        self.wanted = Some(wanted);
        self
    }
}

#[derive(Deserialize)]
struct ChatAnswer {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatReply,
}

#[derive(Deserialize)]
struct ChatReply {
    content: String,
}

/// One `data:` line of a streamed answer.
#[derive(Deserialize)]
struct StreamChunk {
    choices: Vec<StreamChoice>,
}

#[derive(Deserialize)]
struct StreamChoice {
    delta: StreamDelta,
}

/// The first piece carries `"content": null` and the last none at all.
#[derive(Deserialize)]
struct StreamDelta {
    content: Option<String>,
}

/// The end of a streamed answer.
const STREAM_DONE: &str = "[DONE]";

/// The text of a streamed answer, read to its end. The reader is given up on
/// the first line that arrives after the answer stopped being wanted; a
/// caller that drops the reader closes the connection.
fn read_stream(reader: impl BufRead, wanted: &Wanted) -> Result<String, LlmError> {
    let mut text = String::new();
    for line in reader.lines() {
        if !wanted.still() {
            return Err(LlmError::Aborted);
        }
        let line = line.map_err(|error| LlmError::Transport(error.to_string()))?;
        let Some(data) = line.strip_prefix("data:").map(str::trim) else {
            continue;
        };
        if data == STREAM_DONE {
            return Ok(text);
        }
        let chunk: StreamChunk =
            serde_json::from_str(data).map_err(|error| LlmError::Body(error.to_string()))?;
        if let Some(piece) = chunk
            .choices
            .first()
            .and_then(|choice| choice.delta.content.as_deref())
        {
            text.push_str(piece);
        }
    }
    Err(LlmError::Transport(String::from(
        "the stream ended before its last line",
    )))
}

/// The JSON body that goes to the server for `request`.
pub fn request_body(request: &Completion<'_>) -> Result<String, serde_json::Error> {
    serde_json::to_string(&ChatBody::from(request))
}

/// Something that completes a prompt.
pub trait Llm {
    fn complete(&self, request: &Completion<'_>) -> Result<String, LlmError>;
}

#[derive(Deserialize)]
struct HealthAnswer {
    status: String,
}

/// What `GET /health` says once the model is loaded and the server takes requests.
const HEALTH_READY: &str = "ok";

fn from_ureq(error: ureq::Error) -> LlmError {
    match error {
        ureq::Error::StatusCode(code) => LlmError::Status(code),
        other => LlmError::Transport(format!("{}: {other}", cause_of(&other))),
    }
}

/// A llama-server reached over HTTP.
pub struct HttpLlm {
    endpoint: String,
    health: String,
    agent: ureq::Agent,
    /// Where a request sent again is said; none says nothing.
    log: Option<SharedSink>,
}

impl HttpLlm {
    /// `base_url` is the server root, e.g. `http://127.0.0.1:8099`.
    pub fn new(base_url: &str) -> HttpLlm {
        HttpLlm::with_timeout(base_url, REQUEST_TIMEOUT)
    }

    /// Like [`HttpLlm::new`] with another time limit for one request: a
    /// measurement waits for a slow answer that a live subtitle does not.
    pub fn with_timeout(base_url: &str, timeout: Duration) -> HttpLlm {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .build();
        let root = base_url.trim_end_matches('/');
        HttpLlm {
            endpoint: format!("{root}/v1/chat/completions"),
            health: format!("{root}/health"),
            agent: ureq::Agent::new_with_config(config),
            log: None,
        }
    }

    /// Says each request it sends again on `sink`, as `translate.request.retried`.
    pub fn logging_to(mut self, sink: SharedSink) -> HttpLlm {
        self.log = Some(sink);
        self
    }

    /// Whether the server answers `GET /health` with `{"status":"ok"}`. A
    /// server that is still loading its model answers 503, which is
    /// [`LlmError::Status`].
    pub fn health(&self) -> Result<(), LlmError> {
        let mut response = self.agent.get(&self.health).call().map_err(from_ureq)?;
        let text = response
            .body_mut()
            .read_to_string()
            .map_err(|error| LlmError::Transport(error.to_string()))?;
        let answer: HealthAnswer =
            serde_json::from_str(&text).map_err(|error| LlmError::Body(error.to_string()))?;
        if answer.status == HEALTH_READY {
            Ok(())
        } else {
            Err(LlmError::Body(format!("health status {}", answer.status)))
        }
    }
}

/// The text of a whole (not streamed) answer.
fn read_whole(response: &mut ureq::http::Response<ureq::Body>) -> Result<String, LlmError> {
    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|error| LlmError::Transport(error.to_string()))?;
    let answer: ChatAnswer =
        serde_json::from_str(&text).map_err(|error| LlmError::Body(error.to_string()))?;
    let choice = answer
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| LlmError::Body(String::from("no choices")))?;
    Ok(choice.message.content)
}

impl HttpLlm {
    /// Sends the body of a request once.
    fn send(&self, body: &str) -> Result<ureq::http::Response<ureq::Body>, ureq::Error> {
        self.agent
            .post(&self.endpoint)
            .header("Content-Type", "application/json")
            .send(body)
    }

    /// Says that the request is sent again after `error` ended its first try.
    fn say_retried(&self, error: &ureq::Error, first_try: Duration) {
        let Some(sink) = &self.log else {
            return;
        };
        let event = LogEvent::new(Level::Warn, "translate.request.retried", "service")
            .with("dependency", "llama-server")
            .with("duration_ms", u64::try_from(first_try.as_millis()).ok())
            .with("cause", cause_of(error));
        sink.clone().emit(event);
    }

    /// The request, sent again once when the server had closed the connection
    /// it went out on (`rust_port.ja.md` 3.23). A request whose answer is no
    /// longer wanted is not sent again.
    fn post(&self, request: &Completion<'_>) -> Result<ureq::http::Response<ureq::Body>, LlmError> {
        let body = request_body(request).map_err(|error| LlmError::Body(error.to_string()))?;
        let started = Instant::now();
        match self.send(&body) {
            Err(error) if asks_again(&error) => {
                if request.wanted.is_some_and(|wanted| !wanted.still()) {
                    return Err(LlmError::Aborted);
                }
                self.say_retried(&error, started.elapsed());
                self.send(&body).map_err(from_ureq)
            }
            other => other.map_err(from_ureq),
        }
    }
}

impl Llm for HttpLlm {
    fn complete(&self, request: &Completion<'_>) -> Result<String, LlmError> {
        let mut response = self.post(request)?;
        match request.wanted {
            Some(wanted) => read_stream(BufReader::new(response.body_mut().as_reader()), wanted),
            None => read_whole(&mut response),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_is_one_user_message_with_the_stop_marker() -> Result<(), serde_json::Error> {
        let body = serde_json::to_value(ChatBody::from(&Completion::new("hello", 64)))?;
        assert_eq!(body["stop"], serde_json::json!(["</s>"]));
        assert_eq!(
            body["messages"],
            serde_json::json!([{"role": "user", "content": "hello"}])
        );
        assert_eq!(body["max_tokens"], 64);
        assert_eq!(body["temperature"], 0.0);
        assert_eq!(body["cache_prompt"], true);
        Ok(())
    }

    #[test]
    fn a_coded_request_carries_the_codes_as_template_variables() -> Result<(), serde_json::Error> {
        let request = Completion::coded("こんにちは。", "ja", "ko", 64);
        let body = serde_json::to_value(ChatBody::from(&request))?;
        assert_eq!(
            body["messages"],
            serde_json::json!([{"role": "user", "content": "こんにちは。"}])
        );
        assert_eq!(
            body["chat_template_kwargs"],
            serde_json::json!({"source_lang_code": "ja", "target_lang_code": "ko"})
        );
        assert_eq!(body["stop"], serde_json::json!(["</s>"]));
        Ok(())
    }

    #[test]
    fn a_system_message_goes_before_the_user_message() -> Result<(), serde_json::Error> {
        let request = Completion::new("user text", 64).with_system("system text");
        let body = serde_json::to_value(ChatBody::from(&request))?;
        assert_eq!(
            body["messages"],
            serde_json::json!([
                {"role": "system", "content": "system text"},
                {"role": "user", "content": "user text"}
            ])
        );
        let wire: serde_json::Value = serde_json::from_str(&request_body(&request)?)?;
        assert_eq!(wire["messages"], body["messages"]);
        Ok(())
    }

    #[test]
    fn a_plain_request_has_no_template_variables() -> Result<(), serde_json::Error> {
        let body = serde_json::to_value(ChatBody::from(&Completion::new("hello", 64)))?;
        assert!(body.get("chat_template_kwargs").is_none());
        Ok(())
    }

    #[test]
    fn the_endpoint_joins_the_base_without_a_double_slash() {
        let llm = HttpLlm::new("http://127.0.0.1:8099/");
        assert_eq!(llm.endpoint, "http://127.0.0.1:8099/v1/chat/completions");
    }

    #[test]
    fn the_answer_is_the_content_of_the_first_choice() -> Result<(), serde_json::Error> {
        let answer: ChatAnswer = serde_json::from_str(
            r#"{"choices":[{"message":{"role":"assistant","content":"Hello."}}]}"#,
        )?;
        assert_eq!(answer.choices[0].message.content, "Hello.");
        Ok(())
    }

    #[test]
    fn an_unreachable_server_is_a_transport_error() {
        // Port 1 on the loopback address has no listener.
        let llm = HttpLlm::new("http://127.0.0.1:1");
        let answer = llm.complete(&Completion::new("x", 1));
        assert!(matches!(answer, Err(LlmError::Transport(_))));
    }

    /// Answers one request on the loopback address with `status_line` and
    /// `body`, and returns the base URL of that server.
    fn serve_once(status_line: &'static str, body: &'static str) -> std::io::Result<String> {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
        let base = format!("http://{}", listener.local_addr()?);
        std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut request = [0_u8; 1024];
            // A client that sent nothing leaves nothing to answer.
            if stream.read(&mut request).is_err() {
                return;
            }
            let answer = format!(
                "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            // A client that hung up has no use for the answer.
            let _ = stream.write_all(answer.as_bytes());
        });
        Ok(base)
    }

    #[test]
    fn the_health_url_joins_the_base_without_a_double_slash() {
        let llm = HttpLlm::new("http://127.0.0.1:8099/");
        assert_eq!(llm.health, "http://127.0.0.1:8099/health");
    }

    #[test]
    fn a_server_that_says_ok_is_healthy() -> Result<(), Box<dyn std::error::Error>> {
        let base = serve_once("200 OK", r#"{"status":"ok"}"#)?;
        HttpLlm::new(&base).health()?;
        Ok(())
    }

    #[test]
    fn a_server_that_is_loading_its_model_is_a_status_error()
    -> Result<(), Box<dyn std::error::Error>> {
        let base = serve_once(
            "503 Service Unavailable",
            r#"{"error":{"code":503,"message":"Loading model"}}"#,
        )?;
        assert!(matches!(
            HttpLlm::new(&base).health(),
            Err(LlmError::Status(503))
        ));
        Ok(())
    }

    #[test]
    fn a_status_other_than_ok_in_a_200_answer_is_not_healthy()
    -> Result<(), Box<dyn std::error::Error>> {
        let base = serve_once("200 OK", r#"{"status":"loading"}"#)?;
        assert!(matches!(
            HttpLlm::new(&base).health(),
            Err(LlmError::Body(_))
        ));
        Ok(())
    }

    #[test]
    fn a_health_check_of_an_unreachable_server_is_a_transport_error() {
        assert!(matches!(
            HttpLlm::new("http://127.0.0.1:1").health(),
            Err(LlmError::Transport(_))
        ));
    }

    // ---- a request that may be dropped ------------------------------------

    fn counter_at(value: u64) -> Arc<AtomicU64> {
        Arc::new(AtomicU64::new(value))
    }

    fn data_line(content: Option<&str>) -> String {
        let delta = match content {
            Some(piece) => serde_json::json!({ "content": piece }),
            None => serde_json::json!({}),
        };
        format!(
            "data: {}\n\n",
            serde_json::json!({ "choices": [{ "delta": delta }] })
        )
    }

    /// Reads the head and the whole body of one request, so that nothing is
    /// left unread when the server answers and closes (an unread byte turns
    /// the close into a reset that the client reads as a failure).
    fn read_request(stream: &mut std::net::TcpStream) -> std::io::Result<()> {
        use std::io::Read;
        let mut received: Vec<u8> = Vec::new();
        let mut chunk = [0_u8; 4096];
        loop {
            let got = stream.read(&mut chunk)?;
            if got == 0 {
                return Ok(());
            }
            received.extend_from_slice(&chunk[..got]);
            let text = String::from_utf8_lossy(&received);
            let Some((head, body)) = text.split_once("\r\n\r\n") else {
                continue;
            };
            let length = head
                .lines()
                .filter_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(str::to_owned)
                })
                .find_map(|value| value.trim().parse::<usize>().ok());
            if length.is_some_and(|length| body.len() >= length) {
                return Ok(());
            }
        }
    }

    /// Answers one request with a stream: each `(wait, text)` is written after
    /// its wait. Returns the base URL and the number of pieces the server
    /// wrote before a write failed (the client had hung up) or all were written.
    fn serve_stream(pieces: Vec<(Duration, String)>) -> std::io::Result<(String, Arc<AtomicU64>)> {
        use std::io::Write;
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
        let base = format!("http://{}", listener.local_addr()?);
        let written = counter_at(0);
        let count = Arc::clone(&written);
        std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            if read_request(&mut stream).is_err() {
                return;
            }
            let head =
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";
            if stream.write_all(head.as_bytes()).is_err() {
                return;
            }
            for (wait, text) in pieces {
                std::thread::sleep(wait);
                if stream
                    .write_all(text.as_bytes())
                    .and_then(|()| stream.flush())
                    .is_err()
                {
                    return;
                }
                count.fetch_add(1, Ordering::SeqCst);
            }
        });
        Ok((base, written))
    }

    #[test]
    fn a_request_that_may_be_dropped_asks_for_a_stream() -> Result<(), serde_json::Error> {
        let latest = counter_at(3);
        let wanted = Wanted::new(latest, 3);
        let request = Completion::new("hello", 64).wanted(&wanted);
        let body = serde_json::to_value(ChatBody::from(&request))?;
        assert_eq!(body["stream"], true);
        assert_eq!(body["cache_prompt"], true);
        let plain = serde_json::to_value(ChatBody::from(&Completion::new("hello", 64)))?;
        assert!(plain.get("stream").is_none());
        Ok(())
    }

    #[test]
    fn an_answer_is_wanted_until_its_counter_moves_on() {
        let latest = counter_at(5);
        let wanted = Wanted::new(Arc::clone(&latest), 5);
        assert!(wanted.still());
        latest.store(6, Ordering::Release);
        assert!(!wanted.still());
        assert!(Wanted::new(latest, 6).still());
    }

    #[test]
    fn a_pinned_answer_stays_wanted_after_its_counter_moves_on() {
        let latest = counter_at(5);
        let pin = Arc::new(AtomicBool::new(false));
        let wanted = Wanted::pinned(Arc::clone(&latest), 5, Arc::clone(&pin));
        latest.store(6, Ordering::Release);
        assert!(!wanted.still());
        pin.store(true, Ordering::Release);
        assert!(wanted.still());
        assert!(!wanted.newest());
        assert_eq!(wanted.generation(), 5);
    }

    #[test]
    fn a_stream_is_read_to_its_last_line() -> Result<(), Box<dyn std::error::Error>> {
        let text = format!(
            "{}{}{}{}data: [DONE]\n\n",
            "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":null}}]}\n\n",
            data_line(Some("Hel")),
            data_line(Some("lo.")),
            data_line(None),
        );
        let wanted = Wanted::new(counter_at(1), 1);
        let said = read_stream(std::io::Cursor::new(text), &wanted)?;
        assert_eq!(said, "Hello.");
        Ok(())
    }

    #[test]
    fn a_stream_with_no_choices_adds_nothing() -> Result<(), Box<dyn std::error::Error>> {
        let text = "data: {\"choices\":[]}\n\ndata: [DONE]\n\n";
        let wanted = Wanted::new(counter_at(1), 1);
        assert_eq!(read_stream(std::io::Cursor::new(text), &wanted)?, "");
        Ok(())
    }

    #[test]
    fn a_stream_that_ends_before_its_last_line_is_a_transport_error() {
        let text = data_line(Some("Hel"));
        let wanted = Wanted::new(counter_at(1), 1);
        let said = read_stream(std::io::Cursor::new(text), &wanted);
        assert!(matches!(said, Err(LlmError::Transport(_))), "{said:?}");
    }

    #[test]
    fn a_stream_line_that_is_not_json_is_a_body_error() {
        let wanted = Wanted::new(counter_at(1), 1);
        let said = read_stream(std::io::Cursor::new("data: {oops\n\n"), &wanted);
        assert!(matches!(said, Err(LlmError::Body(_))), "{said:?}");
    }

    #[test]
    fn a_stream_is_given_up_when_its_answer_is_not_wanted() {
        let latest = counter_at(1);
        let wanted = Wanted::new(Arc::clone(&latest), 1);
        latest.store(2, Ordering::Release);
        let text = format!("{}data: [DONE]\n\n", data_line(Some("Hi")));
        let said = read_stream(std::io::Cursor::new(text), &wanted);
        assert!(matches!(said, Err(LlmError::Aborted)), "{said:?}");
    }

    #[test]
    fn a_streamed_answer_comes_through_the_server() -> Result<(), Box<dyn std::error::Error>> {
        let pieces = vec![
            (Duration::ZERO, data_line(Some("Hello"))),
            (Duration::ZERO, data_line(Some(", world."))),
            (Duration::ZERO, String::from("data: [DONE]\n\n")),
        ];
        let (base, _) = serve_stream(pieces)?;
        let wanted = Wanted::new(counter_at(1), 1);
        let said = HttpLlm::new(&base).complete(&Completion::new("x", 8).wanted(&wanted))?;
        assert_eq!(said, "Hello, world.");
        Ok(())
    }

    /// The connection is closed as soon as the answer is not wanted: the
    /// server's later writes fail, so it stops short of the end.
    #[test]
    fn a_request_not_wanted_any_more_closes_the_connection()
    -> Result<(), Box<dyn std::error::Error>> {
        const PIECES: u64 = 40;
        let pieces: Vec<(Duration, String)> = (0..PIECES)
            .map(|_| (Duration::from_millis(20), data_line(Some("a"))))
            .collect();
        let (base, written) = serve_stream(pieces)?;
        let latest = counter_at(1);
        let wanted = Wanted::new(Arc::clone(&latest), 1);
        let turn = Arc::clone(&latest);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            turn.store(2, Ordering::Release);
        });
        let started = std::time::Instant::now();
        let said = HttpLlm::new(&base).complete(&Completion::new("x", 8).wanted(&wanted));
        assert!(matches!(said, Err(LlmError::Aborted)), "{said:?}");
        assert!(started.elapsed() < Duration::from_millis(600));
        // Let the server try to write after the hang-up.
        std::thread::sleep(Duration::from_millis(500));
        assert!(
            written.load(Ordering::SeqCst) < PIECES,
            "the server wrote all {PIECES} pieces"
        );
        Ok(())
    }

    #[test]
    fn an_aborted_request_is_worded() {
        assert_eq!(LlmError::Aborted.to_string(), "aborted");
    }

    // ---- a request sent again when the connection had been closed ---------

    use crate::reach::{Change, Reach};

    /// What the fake server does with one connection.
    #[derive(Clone, Copy)]
    enum Turn {
        /// Reads the whole request, then closes without a word.
        HangUp,
        /// Reads the whole request, then answers with the status line and body.
        Answer(&'static str, &'static str),
    }

    const CHAT_ANSWER: &str =
        r#"{"choices":[{"message":{"role":"assistant","content":"Hello."}}]}"#;

    /// A server on the loopback address that gives its connections their turns
    /// in order. Returns the base URL and the number of connections it has taken.
    fn serve_turns(turns: Vec<Turn>) -> std::io::Result<(String, Arc<AtomicU64>)> {
        use std::io::Write;
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
        let base = format!("http://{}", listener.local_addr()?);
        let taken = counter_at(0);
        let count = Arc::clone(&taken);
        std::thread::spawn(move || {
            for turn in turns {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                count.fetch_add(1, Ordering::SeqCst);
                if read_request(&mut stream).is_err() {
                    return;
                }
                if let Turn::Answer(status_line, body) = turn {
                    let answer = format!(
                        "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    // A client that hung up has no use for the answer.
                    let _ = stream.write_all(answer.as_bytes());
                }
            }
        });
        Ok((base, taken))
    }

    /// The events a test reads back from the sink it gave a client.
    #[derive(Clone, Default)]
    struct Collected(Arc<std::sync::Mutex<Vec<LogEvent>>>);

    impl LogSink for Collected {
        fn emit(&mut self, event: LogEvent) {
            match self.0.lock() {
                Ok(mut events) => events.push(event),
                Err(poisoned) => poisoned.into_inner().push(event),
            }
        }
    }

    impl Collected {
        fn named(&self, name: &str) -> Vec<LogEvent> {
            let events = match self.0.lock() {
                Ok(events) => events,
                Err(poisoned) => poisoned.into_inner(),
            };
            events
                .iter()
                .filter(|event| event.event == name)
                .cloned()
                .collect()
        }
    }

    /// A client that says on `events` each request it sends again.
    fn logging_client(base: &str) -> (HttpLlm, Collected) {
        let events = Collected::default();
        let llm = HttpLlm::new(base).logging_to(SharedSink::new(events.clone()));
        (llm, events)
    }

    /// T-RP-96-004
    #[test]
    fn a_connection_closed_before_the_answer_is_asked_again_once()
    -> Result<(), Box<dyn std::error::Error>> {
        let (base, taken) = serve_turns(vec![Turn::HangUp, Turn::Answer("200 OK", CHAT_ANSWER)])?;
        let (llm, events) = logging_client(&base);
        let said = llm.complete(&Completion::new("x", 8))?;
        assert_eq!(said, "Hello.");
        assert_eq!(taken.load(Ordering::SeqCst), 2);
        let retried = events.named("translate.request.retried");
        assert_eq!(retried.len(), 1, "{retried:?}");
        assert_eq!(
            retried[0].fields.get("cause"),
            Some(&serde_json::json!("unexpected_eof"))
        );
        Ok(())
    }

    /// T-RP-96-005
    #[test]
    fn a_failure_is_asked_again_once_and_not_more() -> Result<(), Box<dyn std::error::Error>> {
        let (base, taken) = serve_turns(vec![Turn::HangUp; 4])?;
        let (llm, events) = logging_client(&base);
        let said = llm.complete(&Completion::new("x", 8));
        assert!(matches!(said, Err(LlmError::Transport(_))), "{said:?}");
        assert_eq!(taken.load(Ordering::SeqCst), 2);
        assert_eq!(events.named("translate.request.retried").len(), 1);
        let text = said.as_ref().err().map(|error| error.to_string());
        assert_eq!(
            text.as_deref().and_then(cause_in),
            Some("unexpected_eof"),
            "{text:?}"
        );
        Ok(())
    }

    /// T-RP-96-006
    #[test]
    fn an_answered_error_status_is_not_asked_again() -> Result<(), Box<dyn std::error::Error>> {
        let (base, taken) = serve_turns(vec![Turn::Answer("500 Internal Server Error", "{}")])?;
        let (llm, events) = logging_client(&base);
        let said = llm.complete(&Completion::new("x", 8));
        assert!(matches!(said, Err(LlmError::Status(500))), "{said:?}");
        assert_eq!(taken.load(Ordering::SeqCst), 1);
        assert!(events.named("translate.request.retried").is_empty());
        Ok(())
    }

    /// One failed exchange of each kind the list of causes names, with its
    /// word, in the order of the list.
    fn failures() -> Vec<(ureq::Error, &'static str)> {
        let io_error = |kind: io::ErrorKind| ureq::Error::Io(io::Error::from(kind));
        vec![
            (io_error(io::ErrorKind::UnexpectedEof), "unexpected_eof"),
            (io_error(io::ErrorKind::ConnectionReset), "connection_reset"),
            (
                io_error(io::ErrorKind::ConnectionAborted),
                "connection_aborted",
            ),
            (io_error(io::ErrorKind::BrokenPipe), "broken_pipe"),
            (
                io_error(io::ErrorKind::ConnectionRefused),
                "connection_refused",
            ),
            (io_error(io::ErrorKind::TimedOut), "timed_out"),
            (io_error(io::ErrorKind::Other), "io_other"),
            (ureq::Error::Timeout(ureq::Timeout::Global), "timeout"),
            (ureq::Error::HostNotFound, "host_not_found"),
            (ureq::Error::BadUri(String::from("x")), "other"),
        ]
    }

    /// T-RP-96-007
    #[test]
    fn each_failure_kind_has_its_own_cause() {
        let failures = failures();
        for (error, word) in &failures {
            assert_eq!(cause_of(error), *word, "{error}");
        }
        let words: Vec<&str> = failures.iter().map(|(_, word)| *word).collect();
        assert_eq!(words, CAUSES, "the list of causes and the kinds differ");
    }

    /// T-RP-96-008
    #[test]
    fn only_the_four_closed_connection_kinds_are_asked_again() {
        let answers: Vec<bool> = failures()
            .iter()
            .map(|(error, _)| asks_again(error))
            .collect();
        assert_eq!(
            answers,
            [
                true, true, true, true, false, false, false, false, false, false
            ]
        );
    }

    /// T-RP-96-009
    #[test]
    fn a_request_not_wanted_any_more_is_not_asked_again() -> Result<(), Box<dyn std::error::Error>>
    {
        let (base, taken) = serve_turns(vec![Turn::HangUp, Turn::Answer("200 OK", CHAT_ANSWER)])?;
        let (llm, events) = logging_client(&base);
        let wanted = Wanted::new(counter_at(2), 1);
        let said = llm.complete(&Completion::new("x", 8).wanted(&wanted));
        assert!(matches!(said, Err(LlmError::Aborted)), "{said:?}");
        assert_eq!(taken.load(Ordering::SeqCst), 1);
        assert!(events.named("translate.request.retried").is_empty());
        Ok(())
    }

    /// T-RP-96-010
    #[test]
    fn a_retry_that_succeeds_does_not_hold_the_translator() -> Result<(), Box<dyn std::error::Error>>
    {
        let (base, _) = serve_turns(vec![Turn::HangUp, Turn::Answer("200 OK", CHAT_ANSWER)])?;
        let (llm, watch) = Reach::new(HttpLlm::new(&base));
        assert_eq!(llm.complete(&Completion::new("x", 8))?, "Hello.");
        assert!(watch.drain().is_empty());
        // The same watch does see a translator that stays closed.
        let (base, _) = serve_turns(vec![Turn::HangUp; 4])?;
        let (llm, watch) = Reach::new(HttpLlm::new(&base));
        assert!(llm.complete(&Completion::new("x", 8)).is_err());
        assert_eq!(watch.drain(), [Change::Held]);
        Ok(())
    }

    /// T-RP-96-011
    #[test]
    fn the_retried_event_names_the_dependency_and_the_cause_and_no_address()
    -> Result<(), Box<dyn std::error::Error>> {
        let (base, _) = serve_turns(vec![Turn::HangUp, Turn::Answer("200 OK", CHAT_ANSWER)])?;
        let (llm, events) = logging_client(&base);
        llm.complete(&Completion::new("x", 8))?;
        let retried = events.named("translate.request.retried");
        let Some(event) = retried.first() else {
            return Err("no translate.request.retried event".into());
        };
        let line = event.to_json_line();
        let value: serde_json::Value = serde_json::from_str(&line)?;
        assert_eq!(value["level"], "warn");
        assert_eq!(value["dependency"], "llama-server");
        assert!(value["duration_ms"].is_u64(), "{line}");
        assert_eq!(value["cause"], "unexpected_eof");
        assert!(value["error_code"].is_null(), "{line}");
        assert!(!line.contains("127.0.0.1"), "{line}");
        Ok(())
    }

    /// T-RP-96-014
    #[test]
    fn a_failure_text_names_its_cause_only_when_it_is_a_transport_failure() {
        let cases = [
            (
                "transport: connection_reset: io: Connection reset by peer (os error 104)",
                Some("connection_reset"),
            ),
            (
                "transport: the stream ended before its last line",
                Some("other"),
            ),
            ("transport: no scripted answer", Some("other")),
            ("transport: ", Some("other")),
            ("transport: connection_reset", Some("connection_reset")),
            ("status 503", None),
            ("body: no choices", None),
            ("aborted", None),
        ];
        for (text, word) in cases {
            assert_eq!(cause_in(text), word, "{text}");
        }
    }
}
