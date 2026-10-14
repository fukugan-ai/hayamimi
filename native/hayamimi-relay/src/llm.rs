//! The llama-server client: one request, one answer, a fixed time limit.

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
}

impl std::fmt::Display for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LlmError::Transport(why) => write!(f, "transport: {why}"),
            LlmError::Status(code) => write!(f, "status {code}"),
            LlmError::Body(why) => write!(f, "body: {why}"),
        }
    }
}

impl std::error::Error for LlmError {}

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
        }
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

impl Llm for HttpLlm {
    fn complete(&self, request: &Completion<'_>) -> Result<String, LlmError> {
        let body = serde_json::to_string(&ChatBody::from(request))
            .map_err(|error| LlmError::Body(error.to_string()))?;
        let mut response = self
            .agent
            .post(&self.endpoint)
            .header("Content-Type", "application/json")
            .send(body)
            .map_err(from_ureq)?;
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
}
