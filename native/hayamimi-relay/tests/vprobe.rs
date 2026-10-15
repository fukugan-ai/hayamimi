//! `vprobe`: which references V withholds as they stand and after each fixed
//! break, and what T alone writes with and without V.
//!
//! The pairs are five English sentences chosen so that each finding has a
//! known answer: id 50 is withheld as it stands (a preface), id 20 holds a
//! digit, id 30 a negation and id 40 a hedge.

mod support;

use std::fs;
use std::time::Duration;

use hayamimi_relay::llm::HttpLlm;
use hayamimi_relay::vprobe::{self, Finding, Row};
use serde_json::Value;
use support::{FakeServer, Reply, TempDir, TestResult, chat_reply, closed_port_url, request_body};

const IDS: &str = "10\n20\n30\n40\n50\n";
const JA: &str = "猫が寝ています。\n会議は3時に始まります。\n雨は降っていません。\nたぶん雨が降るでしょう。\n犬が走っています。\n";
const REFERENCES: &str = "The cat is sleeping.\nThe meeting starts at 3.\nIt is not raining.\nIt will probably rain.\nSure, the dog is running.\n";

/// What T answers to each source: ids 20 and 30 come back wrong.
const ANSWERS: [(&str, &str); 5] = [
    ("猫が寝ています。", "The cat is sleeping."),
    ("会議は3時に始まります。", "The meeting starts at 4."),
    ("雨は降っていません。", "It is raining."),
    ("たぶん雨が降るでしょう。", "It will probably rain."),
    ("犬が走っています。", "The dog is running."),
];

fn pairs_dir() -> std::io::Result<TempDir> {
    let dir = TempDir::new("vprobe-pairs")?;
    for (suffix, text) in [("ids", IDS), ("ja", JA), ("ref", REFERENCES)] {
        fs::write(dir.join(&format!("en.{suffix}.txt")), text)?;
    }
    Ok(dir)
}

fn scripted_server() -> std::io::Result<FakeServer> {
    FakeServer::start(|_, raw| {
        let message = serde_json::from_slice::<Value>(request_body(raw))
            .ok()
            .and_then(|body| body["messages"][0]["content"].as_str().map(str::to_owned));
        let answer = message.and_then(|message| {
            ANSWERS
                .iter()
                .find(|(source, _)| message.contains(source))
                .map(|(_, answer)| *answer)
        });
        match answer {
            Some(text) => Reply::json(&chat_reply(text)),
            None => Reply::status(400),
        }
    })
}

fn finding<'a>(found: &'a [Finding], name: &str) -> Result<&'a Finding, String> {
    found
        .iter()
        .find(|one| one.name == name)
        .ok_or_else(|| format!("no finding named {name}"))
}

fn ids(finding: &Finding) -> Vec<&str> {
    finding.held.iter().map(String::as_str).collect()
}

/// (a): the reference with a preface is withheld as it stands, the others pass.
#[test]
fn the_references_v_withholds_as_they_stand_are_named() -> TestResult {
    let dir = pairs_dir()?;
    let found = vprobe::probe(&vprobe::read_rows(dir.path(), "en")?, "en")?;
    let standing = finding(&found, "reference")?;
    assert_eq!(standing.rows, 5);
    assert_eq!(ids(standing), ["50"]);
    Ok(())
}

/// (b): each rule breaks only the rows it can, and V withholds each of them.
#[test]
fn each_break_is_counted_and_withheld_by_its_own_rows() -> TestResult {
    let dir = pairs_dir()?;
    let found = vprobe::probe(&vprobe::read_rows(dir.path(), "en")?, "en")?;
    for (name, id) in [("break:number", "20"), ("break:negation", "30")] {
        let one = finding(&found, name)?;
        assert_eq!(one.rows, 1, "{name}");
        assert_eq!(ids(one), [id], "{name}");
    }
    // Into English a lost hedge is recorded, not withheld.
    let hedge = finding(&found, "break:uncertainty")?;
    assert_eq!(hedge.rows, 1);
    assert!(ids(hedge).is_empty(), "{:?}", ids(hedge));
    Ok(())
}

/// (c): T alone, as written and as V lets it through.
#[test]
fn t_alone_is_written_whole_and_gated_with_withheld_lines_empty() -> TestResult {
    let dir = pairs_dir()?;
    let server = scripted_server()?;
    let llm = HttpLlm::with_timeout(&server.base_url(), Duration::from_secs(5));
    let done = vprobe::t_only(&llm, &vprobe::read_rows(dir.path(), "en")?, "en")?;
    assert_eq!(done.held, ["20", "30"]);
    assert_eq!(done.raw.len(), 5);
    assert_eq!(done.raw[1], "The meeting starts at 4.");
    assert_eq!(done.gated[1], "");
    assert_eq!(done.gated[2], "");
    assert_eq!(done.gated[0], "The cat is sleeping.");
    Ok(())
}

/// The command writes `noV/` and `V/` directories a scorer can read.
#[test]
fn the_command_writes_the_two_hypothesis_directories() -> TestResult {
    let dir = pairs_dir()?;
    let out = TempDir::new("vprobe-out")?;
    let server = scripted_server()?;
    let args: Vec<String> = [
        "--pairs",
        dir.path().to_str().ok_or("non-utf8 path")?,
        "--lang",
        "en",
        "--out",
        out.path().to_str().ok_or("non-utf8 path")?,
        "--llm",
        &server.base_url(),
    ]
    .iter()
    .map(|arg| (*arg).to_owned())
    .collect();
    assert_eq!(vprobe::run(&args), 0);
    let raw = fs::read_to_string(out.join("noV").join("en.hyp.txt"))?;
    let gated = fs::read_to_string(out.join("V").join("en.hyp.txt"))?;
    assert_eq!(raw.lines().count(), 5);
    assert_eq!(gated.lines().count(), 5);
    assert_eq!(gated.lines().filter(|line| line.is_empty()).count(), 2);
    Ok(())
}

/// A translator that cannot be reached stops T alone, naming the language;
/// the command writes only after T has answered every row.
#[test]
fn an_unreachable_translator_stops_t_alone_and_names_the_language() -> TestResult {
    let dir = pairs_dir()?;
    let llm = HttpLlm::with_timeout(&closed_port_url()?, Duration::from_secs(2));
    let stopped = vprobe::t_only(&llm, &vprobe::read_rows(dir.path(), "en")?, "en");
    assert!(matches!(&stopped, Err(why) if why.starts_with("en: ")));
    Ok(())
}

/// Files of different lengths are refused, naming the three counts.
#[test]
fn files_of_different_lengths_are_refused() -> TestResult {
    let dir = pairs_dir()?;
    fs::write(dir.join("en.ref.txt"), "only one\n")?;
    let refused: Result<Vec<Row>, String> = vprobe::read_rows(dir.path(), "en");
    assert!(matches!(&refused, Err(why) if why.contains("5, 5 and 1")));
    Ok(())
}
