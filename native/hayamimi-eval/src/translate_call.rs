//! One translation request to a llama-server, in the form the model card of
//! the translation model asks for: a single user message that holds the
//! instruction and the text, sent to the chat endpoint.

use std::time::Duration;

use hayamimi_relay::lang::Lang;
use hayamimi_relay::pipeline::translate_prompt;
use serde_json::{Value, json};

use crate::error::EvalError;

/// The end-of-sequence marker the model writes after its answer.
const STOP: &str = "</s>";
/// The answer is at most this many tokens.
const MAX_TOKENS: u32 = 256;

/// The JSON body that asks for a Japanese to English translation of `text`.
///
/// The prompt cache is off, so that the same sentence sent again is computed
/// again and a repeated run is not faster than the first.
pub fn translate_body(text: &str) -> Result<String, EvalError> {
    let body = json!({
        "messages": [{ "role": "user", "content": translate_prompt(Lang::Ja, text) }],
        "temperature": 0,
        "max_tokens": MAX_TOKENS,
        "stop": [STOP],
        "cache_prompt": false,
    });
    serde_json::to_string(&body).map_err(|e| EvalError::Invalid(e.to_string()))
}

/// A llama-server reached over HTTP.
pub struct ChatClient {
    endpoint: String,
    agent: ureq::Agent,
}

impl ChatClient {
    /// `base_url` is the server root, for example `http://127.0.0.1:18091`.
    pub fn new(base_url: &str, timeout: Duration) -> ChatClient {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .build();
        ChatClient {
            endpoint: format!("{}/v1/chat/completions", base_url.trim_end_matches('/')),
            agent: ureq::Agent::new_with_config(config),
        }
    }

    /// Sends `body` and returns the text of the answer.
    pub fn complete(&self, body: &str) -> Result<String, EvalError> {
        let http_error = |message: String| EvalError::Http {
            url: self.endpoint.clone(),
            message,
        };
        let mut response = self
            .agent
            .post(&self.endpoint)
            .header("Content-Type", "application/json")
            .send(body)
            .map_err(|e| http_error(e.to_string()))?;
        let text = response
            .body_mut()
            .read_to_string()
            .map_err(|e| http_error(e.to_string()))?;
        let value: Value = serde_json::from_str(&text).map_err(|e| http_error(e.to_string()))?;
        value["choices"][0]["message"]["content"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| http_error(String::from("the answer has no choices[0].message.content")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_body_carries_the_instruction_the_stop_marker_and_no_prompt_cache()
    -> Result<(), EvalError> {
        let body: Value = serde_json::from_str(&translate_body("こんにちは。")?)
            .map_err(|e| EvalError::Invalid(e.to_string()))?;
        let content = body["messages"][0]["content"].as_str();
        assert!(
            content.is_some_and(
                |c| c.starts_with("Translate the following Japanese text into English.")
            )
        );
        assert!(content.is_some_and(|c| c.ends_with("こんにちは。")));
        assert_eq!(body["stop"], json!(["</s>"]));
        assert_eq!(body["cache_prompt"], json!(false));
        Ok(())
    }

    #[test]
    fn an_unreachable_server_is_an_http_error() {
        let client = ChatClient::new("http://127.0.0.1:1", Duration::from_millis(300));
        assert!(matches!(client.complete("{}"), Err(EvalError::Http { .. })));
    }

    #[test]
    fn the_endpoint_joins_the_base_without_a_double_slash() {
        let client = ChatClient::new("http://127.0.0.1:8099/", Duration::from_secs(1));
        assert_eq!(client.endpoint, "http://127.0.0.1:8099/v1/chat/completions");
    }
}
