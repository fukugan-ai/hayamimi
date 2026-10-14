//! Shared by the integration tests: drawing a page to a string, finding the
//! recorded wire, and a server that sends the events a server would.

// Each test file uses the part of this module it needs.
#![allow(dead_code)]

use std::error::Error;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::thread::JoinHandle;
use std::time::Duration;

use dioxus::prelude::*;
use hayamimi_ui::model::Model;
use hayamimi_ui::parts::Words;
use hayamimi_ui::route::Page;
use hayamimi_ui::strings::{Locale, Strings};
use hayamimi_ui::wire::{self, UiEvent};
use hayamimi_ui::{dashboard::DashboardPage, overlay::OverlayPage, transcript::TranscriptPage};

pub type Failure = Box<dyn Error>;

#[derive(Props, Clone, PartialEq)]
struct Root {
    model: Model,
    page: Page,
    words: Strings,
}

fn root(props: Root) -> Element {
    let words = props.words.clone();
    use_context_provider(|| -> Words { Rc::new(words) });
    let model = use_signal(|| props.model.clone());
    match props.page {
        Page::Overlay(show) => rsx! { OverlayPage { model, show } },
        Page::Dashboard => rsx! { DashboardPage { model } },
        Page::Transcript => rsx! { TranscriptPage { model } },
    }
}

/// The page as the browser would first draw it from `model`, as HTML.
///
/// A table that cannot be read comes back as the text of the error, which no
/// test expects to find.
pub fn render(model: &Model, page: Page, locale: Locale) -> String {
    let words = match Strings::embedded(locale) {
        Ok(words) => words,
        Err(error) => return format!("the table of words is unreadable: {error}"),
    };
    let mut dom = VirtualDom::new_with_props(
        root,
        Root {
            model: model.clone(),
            page,
            words,
        },
    );
    dom.rebuild_in_place();
    dioxus_ssr::render(&dom)
}

/// One event of the stream, from its `data`.
pub fn event(data: &str) -> Result<UiEvent, Failure> {
    Ok(wire::parse(data)?)
}

/// The model after `datas` arrived, one every `step_ms` from time 0.
pub fn model_after(datas: &[&str], step_ms: u64) -> Result<Model, Failure> {
    let mut model = Model::default();
    for (at, data) in datas.iter().enumerate() {
        model.apply(event(data)?, at as u64 * step_ms);
    }
    Ok(model)
}

/// The directory of the recorded wire (`testdata/baseline/wire`).
///
/// `testdata/` is not in git. It is looked for two directories above this
/// crate, and from a linked worktree in the main working tree that owns it.
pub fn recorded_wire() -> Result<PathBuf, Failure> {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let near = here.join("../../testdata/baseline/wire");
    if near.is_dir() {
        return Ok(near);
    }
    let asked = Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(here)
        .output()?;
    let common = String::from_utf8(asked.stdout)?;
    let Some(root) = Path::new(common.trim()).parent() else {
        return Err(format!(
            "no testdata at {} and git named no main tree",
            near.display()
        )
        .into());
    };
    let far = root.join("testdata/baseline/wire");
    if far.is_dir() {
        return Ok(far);
    }
    Err(format!(
        "no recorded wire at {} or at {}",
        near.display(),
        far.display()
    )
    .into())
}

/// The `data` of the messages a client read from the server, in order.
pub fn recorded_sse_datas() -> Result<Vec<String>, Failure> {
    let text = std::fs::read_to_string(recorded_wire()?.join("events.jsonl"))?;
    let mut datas = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let row: serde_json::Value = serde_json::from_str(line)?;
        if row.get("source").and_then(serde_json::Value::as_str) == Some("sse") {
            let Some(inner) = row.get("event") else {
                return Err(format!("a row without an event: {line}").into());
            };
            datas.push(inner.to_string());
        }
    }
    Ok(datas)
}

/// A server that answers `GET /events` with the given messages, framed as
/// server-sent events, and then closes. Anything else gets a 404.
pub struct FakeEvents {
    pub address: SocketAddr,
    handle: JoinHandle<std::io::Result<()>>,
}

impl FakeEvents {
    pub fn serve(datas: Vec<String>) -> Result<FakeEvents, Failure> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let address = listener.local_addr()?;
        let handle = std::thread::spawn(move || answer(&listener, &datas));
        Ok(FakeEvents { address, handle })
    }

    /// Waits for the server to finish its one answer.
    pub fn finish(self) -> Result<(), Failure> {
        match self.handle.join() {
            Ok(done) => Ok(done?),
            Err(_) => Err("the fake server panicked".into()),
        }
    }
}

fn answer(listener: &TcpListener, datas: &[String]) -> std::io::Result<()> {
    let (mut stream, _) = listener.accept()?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut head = Vec::new();
    let mut byte = [0_u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if stream.read(&mut byte)? == 0 {
            break;
        }
        head.push(byte[0]);
    }
    if !head.starts_with(b"GET /events ") {
        return stream.write_all(
            b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
        );
    }
    stream.write_all(b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncache-control: no-cache\r\nconnection: close\r\n\r\n")?;
    stream.write_all(b": connected\n\n")?;
    for data in datas {
        stream.write_all(format!("data: {data}\n\n").as_bytes())?;
    }
    stream.flush()
}

/// Reads `GET /events` from `address` to its end and returns the `data` of
/// each message, as a browser's `EventSource` would give them.
pub fn read_events(address: SocketAddr, path: &str) -> Result<(u16, Vec<String>), Failure> {
    let mut stream = TcpStream::connect(address)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.write_all(format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes())?;
    let mut reply = Vec::new();
    stream.read_to_end(&mut reply)?;
    let text = String::from_utf8(reply)?;
    let Some((head, body)) = text.split_once("\r\n\r\n") else {
        return Err(format!("no end of head in {text:?}").into());
    };
    let status = head
        .split_whitespace()
        .nth(1)
        .ok_or("no status in the head")?
        .parse::<u16>()?;
    Ok((status, data_of_messages(body)))
}

/// The `data` field of each message of a server-sent event body. A message is
/// the lines up to a blank line; lines that start with `:` are comments, and a
/// message with several `data:` lines joins them with a newline.
pub fn data_of_messages(body: &str) -> Vec<String> {
    body.split("\n\n")
        .filter_map(|message| {
            let lines: Vec<&str> = message
                .lines()
                .filter_map(|line| line.strip_prefix("data:"))
                .map(|rest| rest.strip_prefix(' ').map_or(rest, |trimmed| trimmed))
                .collect();
            (!lines.is_empty()).then(|| lines.join("\n"))
        })
        .collect()
}

/// `text` appears in `html` this many times.
pub fn count_in(html: &str, text: &str) -> usize {
    html.matches(text).count()
}
