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

/// The body of a llama-server `/completion` request.
#[derive(Debug, Serialize)]
pub struct Completion<'a> {
    pub prompt: &'a str,
    pub n_predict: u32,
    pub temperature: f32,
    /// Always the end-of-sequence marker, so it never reaches a subtitle.
    pub stop: [&'static str; 1],
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
struct CompletionAnswer {
    content: String,
}

/// Something that completes a prompt.
pub trait Llm {
    fn complete(&self, request: &Completion<'_>) -> Result<String, LlmError>;
}

/// A llama-server reached over HTTP.
pub struct HttpLlm {
    endpoint: String,
    agent: ureq::Agent,
}

impl HttpLlm {
    /// `base_url` is the server root, e.g. `http://127.0.0.1:8099`.
    pub fn new(base_url: &str) -> HttpLlm {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(REQUEST_TIMEOUT))
            .build();
        HttpLlm {
            endpoint: format!("{}/completion", base_url.trim_end_matches('/')),
            agent: ureq::Agent::new_with_config(config),
        }
    }
}

impl Llm for HttpLlm {
    fn complete(&self, request: &Completion<'_>) -> Result<String, LlmError> {
        let body =
            serde_json::to_string(request).map_err(|error| LlmError::Body(error.to_string()))?;
        let mut response = self
            .agent
            .post(&self.endpoint)
            .header("Content-Type", "application/json")
            .send(body)
            .map_err(|error| match error {
                ureq::Error::StatusCode(code) => LlmError::Status(code),
                other => LlmError::Transport(other.to_string()),
            })?;
        let text = response
            .body_mut()
            .read_to_string()
            .map_err(|error| LlmError::Transport(error.to_string()))?;
        let answer: CompletionAnswer =
            serde_json::from_str(&text).map_err(|error| LlmError::Body(error.to_string()))?;
        Ok(answer.content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_always_carries_the_stop_marker() -> Result<(), serde_json::Error> {
        let body = serde_json::to_value(Completion::new("hello", 64))?;
        assert_eq!(body["stop"], serde_json::json!(["</s>"]));
        assert_eq!(body["prompt"], "hello");
        assert_eq!(body["n_predict"], 64);
        Ok(())
    }

    #[test]
    fn the_endpoint_joins_the_base_without_a_double_slash() {
        let llm = HttpLlm::new("http://127.0.0.1:8099/");
        assert_eq!(llm.endpoint, "http://127.0.0.1:8099/completion");
    }

    #[test]
    fn an_unreachable_server_is_a_transport_error() {
        // Port 1 on the loopback address has no listener.
        let llm = HttpLlm::new("http://127.0.0.1:1");
        let answer = llm.complete(&Completion::new("x", 1));
        assert!(matches!(answer, Err(LlmError::Transport(_))));
    }
}
