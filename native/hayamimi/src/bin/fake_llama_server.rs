//! A stand-in for llama.cpp's `llama-server`, for the tests of
//! `hayamimi llama start` and for the page test of IP-5. It is not part of the
//! product.
//!
//! It takes the arguments the real server takes and reads `--port` and `-m`.
//! What it does depends on the name of the model file, which the test names:
//!
//! - `exits.gguf`: ends at once with code 3, as a server that cannot start does;
//! - `silent.gguf`: runs and never listens, as a server that never finishes loading does;
//! - `translates.gguf`: answers `POST /v1/chat/completions` with one fixed
//!   sentence, English when the first line of the user message asks for a
//!   translation `into English` and Japanese when it asks `into Japanese`,
//!   as the whole answer or, when the request has `"stream": true`, as a
//!   stream of `data:` lines ending in `[DONE]`. Every other request, and a
//!   chat request that names neither language, gets `{"status":"ok"}`, which
//!   the relay does not read as an answer;
//! - anything else: answers every request with `{"status":"ok"}` until it is stopped.
//!
//! The fixed sentences carry no digit, negation, hedge or name, so the
//! verifier of the relay has nothing of theirs to hold; a source line with
//! one still holds its translation.
//!
//! It prints its arguments on the first line, so the log the start gives it
//! shows how it was started.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::process::ExitCode;
use std::time::Duration;

use serde_json::{Value, json};

const ANSWER: &str = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 15\r\nConnection: close\r\n\r\n{\"status\":\"ok\"}";

/// What `translates.gguf` says for a translation into English.
const ENGLISH: &str = "That is a very good question.";
/// What `translates.gguf` says for a translation into Japanese.
const JAPANESE: &str = "それはとても良い質問です。";

const HEAD_END: &[u8] = b"\r\n\r\n";
/// How long a client may take to send its request.
const READ_WAIT: Duration = Duration::from_secs(10);

/// The value that follows `name` in the arguments.
fn value_after<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    let at = args.iter().position(|arg| arg == name)?;
    args.get(at + 1).map(String::as_str)
}

/// Reads the request and answers it.
fn answer(mut stream: TcpStream) {
    let mut request = [0_u8; 2048];
    // A client that sent nothing, or hung up, gets no answer.
    if stream.read(&mut request).is_err() {
        return;
    }
    // A client that hung up has no use for the answer.
    let _ = stream.write_all(ANSWER.as_bytes());
}

/// The length of the body a request head announces.
fn content_length(head: &str) -> usize {
    let announced = head.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if name.eq_ignore_ascii_case("content-length") {
            value.trim().parse::<usize>().ok()
        } else {
            None
        }
    });
    // DEFAULT: a request with no Content-Length has no body (RFC 9112, 6.3).
    announced.unwrap_or_default()
}

/// The head (up to the blank line) and the body of the request that arrives
/// on `stream`; `None` when the client hangs up or stalls before it is whole.
fn read_request(stream: &mut TcpStream) -> Option<(String, Vec<u8>)> {
    let mut received = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        if let Some(end) = received.windows(HEAD_END.len()).position(|w| w == HEAD_END) {
            let head = String::from_utf8_lossy(&received[..end]).into_owned();
            let start = end + HEAD_END.len();
            let wanted = start + content_length(&head);
            if received.len() >= wanted {
                return Some((head, received[start..wanted].to_vec()));
            }
        }
        let got = stream.read(&mut chunk).ok()?;
        if got == 0 {
            return None;
        }
        received.extend_from_slice(&chunk[..got]);
    }
}

/// A chat request: the text of its last user message and whether the answer
/// is asked piece by piece.
struct Chat {
    user: String,
    stream: bool,
}

/// The chat request that `head` and `body` make, or `None` for anything else.
fn parse_chat(head: &str, body: &[u8]) -> Option<Chat> {
    if !head.starts_with("POST /v1/chat/completions") {
        return None;
    }
    let json: Value = serde_json::from_slice(body).ok()?;
    let user = json
        .get("messages")?
        .as_array()?
        .iter()
        .rev()
        .find(|message| message.get("role").and_then(Value::as_str) == Some("user"))?
        .get("content")?
        .as_str()?
        .to_owned();
    let stream = json.get("stream").and_then(Value::as_bool) == Some(true);
    Some(Chat { user, stream })
}

/// The fixed sentence for the language the first line of `user` asks for.
fn sentence_for(user: &str) -> Option<&'static str> {
    let ask = user.lines().next()?;
    if ask.contains("into English") {
        Some(ENGLISH)
    } else if ask.contains("into Japanese") {
        Some(JAPANESE)
    } else {
        None
    }
}

/// The whole answer of a chat request.
fn whole_answer(sentence: &str) -> String {
    let body =
        json!({"choices": [{"message": {"role": "assistant", "content": sentence}}]}).to_string();
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

/// The answer of a chat request that asked for a stream: the first piece has
/// no content, the second all of it, the third none, then the end mark.
fn stream_answer(sentence: &str) -> String {
    let piece = |delta: Value| format!("data: {}\n\n", json!({"choices": [{"delta": delta}]}));
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n{}{}{}data: [DONE]\n\n",
        piece(json!({"role": "assistant", "content": null})),
        piece(json!({"content": sentence})),
        piece(json!({})),
    )
}

/// The HTTP answer to one request of `translates.gguf`.
fn translation_reply(head: &str, body: &[u8]) -> String {
    let asked = parse_chat(head, body)
        .and_then(|chat| sentence_for(&chat.user).map(|sentence| (chat.stream, sentence)));
    match asked {
        Some((true, sentence)) => stream_answer(sentence),
        Some((false, sentence)) => whole_answer(sentence),
        None => ANSWER.to_owned(),
    }
}

/// Reads the request and answers it as `translates.gguf` does.
fn answer_translating(mut stream: TcpStream) {
    if stream.set_read_timeout(Some(READ_WAIT)).is_err() {
        return;
    }
    let Some((head, body)) = read_request(&mut stream) else {
        return;
    };
    // A client that hung up has no use for the answer.
    let _ = stream.write_all(translation_reply(&head, &body).as_bytes());
}

fn serve(port: u16, translates: bool) -> ExitCode {
    let Ok(listener) = TcpListener::bind((Ipv4Addr::LOCALHOST, port)) else {
        return ExitCode::from(1);
    };
    for stream in listener.incoming().flatten() {
        if translates {
            // One thread per client: the relay asks for several lines at once.
            std::thread::spawn(move || answer_translating(stream));
        } else {
            answer(stream);
        }
    }
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    println!("fake-llama-server {}", args.join(" "));
    let model = value_after(&args, "-m");
    if model.is_some_and(|name| name.ends_with("exits.gguf")) {
        return ExitCode::from(3);
    }
    let Some(port) = value_after(&args, "--port").and_then(|text| text.parse::<u16>().ok()) else {
        return ExitCode::from(2);
    };
    if model.is_some_and(|name| name.ends_with("silent.gguf")) {
        loop {
            std::thread::sleep(Duration::from_secs(1));
        }
    }
    serve(
        port,
        model.is_some_and(|name| name.ends_with("translates.gguf")),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    type Check = Result<(), Box<dyn std::error::Error>>;

    const CHAT: &str = "POST /v1/chat/completions HTTP/1.1\r\nContent-Length: 0";

    fn request(user: &str, stream: bool) -> Value {
        json!({
            "messages": [
                {"role": "system", "content": "A draft Japanese translation of the user's English text is: x"},
                {"role": "user", "content": user},
            ],
            "stream": stream,
        })
    }

    fn reply_to(user: &str, stream: bool) -> String {
        translation_reply(CHAT, request(user, stream).to_string().as_bytes())
    }

    #[test]
    fn a_request_into_english_gets_the_english_sentence() {
        assert_eq!(
            sentence_for("Translate the following Japanese text into English.\n\n こんにちは。"),
            Some(ENGLISH)
        );
    }

    #[test]
    fn a_request_into_japanese_gets_the_japanese_sentence() {
        assert_eq!(
            sentence_for("Translate the following English text into Japanese.\n\n Hello."),
            Some(JAPANESE)
        );
    }

    #[test]
    fn only_the_first_line_names_the_language() {
        assert_eq!(sentence_for("Say hi.\n\n into English"), None);
        assert_eq!(sentence_for(""), None);
    }

    #[test]
    fn the_whole_answer_has_its_length_and_the_sentence() -> Check {
        let answer = reply_to("Translate the following English text into Japanese.", false);
        let (head, body) = answer.split_once("\r\n\r\n").ok_or("no blank line")?;
        assert!(
            head.contains(&format!("Content-Length: {}", body.len())),
            "{head}"
        );
        let json: Value = serde_json::from_str(body)?;
        assert_eq!(json["choices"][0]["message"]["content"], JAPANESE);
        Ok(())
    }

    #[test]
    fn the_stream_answer_carries_the_sentence_and_ends_with_the_end_mark() -> Check {
        let answer = reply_to("Translate the following Japanese text into English.", true);
        let (_, body) = answer.split_once("\r\n\r\n").ok_or("no blank line")?;
        let data: Vec<&str> = body
            .lines()
            .filter_map(|line| line.strip_prefix("data: "))
            .collect();
        assert_eq!(data.last(), Some(&"[DONE]"));
        let text: String = data
            .iter()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter_map(|chunk| {
                chunk["choices"][0]["delta"]["content"]
                    .as_str()
                    .map(str::to_owned)
            })
            .collect();
        assert_eq!(text, ENGLISH);
        Ok(())
    }

    #[test]
    fn a_chat_request_that_names_no_language_gets_the_status_answer() {
        assert_eq!(reply_to("hello", false), ANSWER);
    }

    #[test]
    fn a_request_that_is_not_a_chat_gets_the_status_answer() {
        assert_eq!(translation_reply("GET /health HTTP/1.1", b""), ANSWER);
        assert_eq!(translation_reply(CHAT, b"{oops"), ANSWER);
    }

    #[test]
    fn the_last_user_message_is_the_one_read() -> Check {
        let body = request("Translate the following Japanese text into English.", true);
        let chat = parse_chat(CHAT, body.to_string().as_bytes()).ok_or("not a chat")?;
        assert_eq!(
            chat.user,
            "Translate the following Japanese text into English."
        );
        assert!(chat.stream);
        Ok(())
    }

    #[test]
    fn the_length_of_the_body_is_read_from_the_head() {
        assert_eq!(content_length("POST / HTTP/1.1\r\ncontent-length: 12"), 12);
        assert_eq!(content_length("GET /health HTTP/1.1"), 0);
        assert_eq!(content_length("POST / HTTP/1.1\r\nContent-Length: x"), 0);
    }

    #[test]
    fn a_request_sent_in_two_writes_is_read_whole() -> Check {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let address = listener.local_addr()?;
        let sender = std::thread::spawn(move || -> std::io::Result<()> {
            let mut client = TcpStream::connect(address)?;
            client
                .write_all(b"POST /v1/chat/completions HTTP/1.1\r\nContent-Length: 5\r\n\r\nab")?;
            client.flush()?;
            std::thread::sleep(Duration::from_millis(50));
            client.write_all(b"cde")
        });
        let (mut served, _) = listener.accept()?;
        let (head, body) = read_request(&mut served).ok_or("no request")?;
        sender.join().map_err(|_| "the sender panicked")??;
        assert!(head.starts_with("POST /v1/chat/completions"));
        assert_eq!(body, b"abcde");
        Ok(())
    }
}
