//! The llama-server client: one request, one answer, a fixed time limit.
//!
//! A request that carries a [`Wanted`] is streamed: the answer is read piece
//! by piece, and the connection is closed as soon as the answer is no longer
//! wanted, so that a newer request does not queue behind a useless one.

use std::io::{BufRead, BufReader};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::verify::EOS_MARKER;

/// A request that has not been answered in this long counts as unavailable.
pub const REQUEST_TIMEOUT: Duration = Duration::from_millis(1500);

/// Why the translator gave no text.
#[derive(Debug)]
pub enum LlmError {
    /// The server could not be reached, or the exchange failed or timed out.
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

/// Tells a request in flight whether its answer is still wanted: it is until
/// the counter it was made from moves on.
#[derive(Clone, Debug)]
pub struct Wanted {
    latest: Arc<AtomicU64>,
    mine: u64,
}

impl Wanted {
    /// A request made when `latest` read `mine`.
    pub fn new(latest: Arc<AtomicU64>, mine: u64) -> Wanted {
        Wanted { latest, mine }
    }

    pub fn still(&self) -> bool {
        self.latest.load(Ordering::Acquire) == self.mine
    }
}

/// One request to the translator: the text of the single user message, how
/// many tokens the answer may take, and the sampling the relay always uses.
#[derive(Debug)]
pub struct Completion<'a> {
    /// The text of the one user message. The server applies the model's chat
    /// template to it.
    pub prompt: &'a str,
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
    messages: [ChatMessage<'a>; 1],
    max_tokens: u32,
    temperature: f32,
    stop: [&'static str; 1],
    /// Reuses the computed part of a prompt that begins as the last one did.
    cache_prompt: bool,
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

impl<'a> From<&Completion<'a>> for ChatBody<'a> {
    fn from(request: &Completion<'a>) -> ChatBody<'a> {
        ChatBody {
            messages: [ChatMessage {
                role: "user",
                content: request.prompt,
            }],
            max_tokens: request.n_predict,
            temperature: request.temperature,
            stop: request.stop,
            cache_prompt: true,
            stream: request.wanted.is_some(),
        }
    }
}

impl<'a> Completion<'a> {
    /// A greedy request that stops at the end-of-sequence marker.
    pub fn new(prompt: &'a str, n_predict: u32) -> Completion<'a> {
        Completion {
            prompt,
            n_predict,
            temperature: 0.0,
            stop: [EOS_MARKER],
            wanted: None,
        }
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
        other => LlmError::Transport(other.to_string()),
    }
}

/// A llama-server reached over HTTP.
pub struct HttpLlm {
    endpoint: String,
    health: String,
    agent: ureq::Agent,
}

impl HttpLlm {
    /// `base_url` is the server root, e.g. `http://127.0.0.1:8099`.
    pub fn new(base_url: &str) -> HttpLlm {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(REQUEST_TIMEOUT))
            .build();
        let root = base_url.trim_end_matches('/');
        HttpLlm {
            endpoint: format!("{root}/v1/chat/completions"),
            health: format!("{root}/health"),
            agent: ureq::Agent::new_with_config(config),
        }
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
    fn post(&self, request: &Completion<'_>) -> Result<ureq::http::Response<ureq::Body>, LlmError> {
        let body = request_body(request).map_err(|error| LlmError::Body(error.to_string()))?;
        self.agent
            .post(&self.endpoint)
            .header("Content-Type", "application/json")
            .send(body)
            .map_err(from_ureq)
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
}
