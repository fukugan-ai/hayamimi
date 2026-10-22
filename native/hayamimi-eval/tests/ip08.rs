//! IP-8: `stream --own-pid` accepts the translation `llama-server` that
//! listens on the port `--own-llama-port` names, and no other.
//!
//! Every case starts the `hayamimi-eval stream` process against a fake
//! `/ingest` (a port the operating system picks) and a `/proc` of its own
//! (`support::s83`), whose three processes are pids 4242, 4243 and 4244. No
//! case opens 8080, 8833, 8835, 18081 or 18083.
//!
//!     cd native && cargo test --release --jobs 2 -p hayamimi-eval --test ip08

mod support;

use std::error::Error;
use std::path::Path;

use serde_json::{Value, json};

use support::s83::{Ended, FakeIngest, OWN_PIDS, OWN_TICKS, Setup, WINDOW_PORT, own_pid_args, run};
use support::{REPO, TestResult};

type Outcome<T> = Result<T, Box<dyn Error>>;

const REFUSED: &str = "stream.machine.refused";
const ABORTED: &str = "stream.machine.aborted";
const FLAG: &str = "--own-llama-port";
const THE_SERVER: u32 = 4244;

/// What one `stream` run left: how it ended, the audio bytes the fake
/// `/ingest` received and the machine record (absent when none was written).
struct Seen {
    ended: Ended,
    audio: usize,
    machine: Option<String>,
}

/// `stream` over a one second recording, with the server of pid 4244 started
/// with `words`, and these arguments after the standard ones.
fn stream(name: &str, words: &[&str], args: &[String]) -> Outcome<Seen> {
    let setup = Setup::new_own(&format!("ip08-{name}"), 1)?;
    setup.proc_root.set_cmdline(THE_SERVER, words)?;
    let ingest = FakeIngest::start()?;
    let arguments: Vec<&str> = args.iter().map(String::as_str).collect();
    let ended = run(&setup, ingest.port, 1, &arguments)?;
    Ok(Seen {
        ended,
        audio: ingest.finish()?,
        machine: setup.machine_text(),
    })
}

/// The three `--own-pid` of the run, then these words.
fn named(after: &[&str]) -> Vec<String> {
    let mut args = own_pid_args(&OWN_PIDS);
    args.extend(after.iter().map(|word| (*word).to_owned()));
    args
}

/// The server's words with `port` after `--port`.
fn server_on(port: &'static str) -> [&'static str; 5] {
    ["llama-server", "-m", "model.gguf", "--port", port]
}

/// A run with the server on `port` that names `named_port`.
fn stream_naming(name: &str, port: &'static str, named_port: &str) -> Outcome<Seen> {
    stream(name, &server_on(port), &named(&[FLAG, named_port]))
}

fn machine_rows(seen: &Seen) -> Outcome<Vec<Value>> {
    let text = seen
        .machine
        .as_deref()
        .ok_or("the run left no machine record")?;
    let rows = text
        .lines()
        .map(serde_json::from_str::<Value>)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// The run started: no refusal, no abort, audio sent, the three processes
/// recorded in the order given, and their ticks counted in every row.
fn assert_accepted(seen: &Seen) -> TestResult {
    let said = &seen.ended.stderr;
    assert!(!matches!(seen.ended.code, Some(2 | 3)), "{said}");
    assert!(seen.ended.events(REFUSED).is_empty(), "{said}");
    assert!(seen.ended.events(ABORTED).is_empty(), "{said}");
    assert!(seen.audio >= 1, "no audio was sent: {said}");
    let rows = machine_rows(seen)?;
    let own = json!([
        { "pid": 4242, "comm": "hayamimi" },
        { "pid": 4243, "comm": "hayamimi-relay" },
        { "pid": 4244, "comm": "llama-server" },
    ]);
    assert_eq!(rows[0]["own"], own);
    for row in &rows {
        assert_eq!(row["own_cpu_ticks"], json!(OWN_TICKS), "{row}");
    }
    Ok(())
}

/// The run was refused for the port of pid 4244, before any audio.
fn assert_refused_on_port(seen: &Seen) {
    let said = &seen.ended.stderr;
    let events = seen.ended.events(REFUSED);
    assert_eq!(events.len(), 1, "{said}");
    assert_eq!(events[0]["level"], "warn");
    assert_eq!(events[0]["reason"], "own_pid");
    assert_eq!(events[0]["own_fault"], "port");
    assert_eq!(events[0]["own_pid"], THE_SERVER);
    assert_eq!(seen.ended.code, Some(2), "{said}");
    assert_eq!(seen.audio, 0, "a refused run sent audio");
    assert!(seen.ended.events(ABORTED).is_empty());
}

/// The arguments were refused by the parser, naming `flag`: no event, no record, no audio.
fn assert_usage_error(seen: &Seen, flag: &str) {
    let said = &seen.ended.stderr;
    assert_eq!(seen.ended.code, Some(2), "{said}");
    assert!(said.contains(flag), "{flag} is not named: {said}");
    assert!(seen.ended.events(REFUSED).is_empty(), "{said}");
    assert!(seen.machine.is_none(), "a machine record was written");
    assert_eq!(seen.audio, 0, "a refused run sent audio");
}

#[test]
fn ip08_the_own_llama_server_is_checked_on_the_port_the_run_names() -> TestResult {
    let seen = stream_naming("t001", "18083", "18083")?;
    assert_accepted(&seen)
}

#[test]
fn a_server_on_18084_is_accepted_when_the_run_names_18084() -> TestResult {
    let seen = stream_naming("t002", "18084", &WINDOW_PORT.to_string())?;
    assert_accepted(&seen)
}

#[test]
fn a_server_on_another_port_than_the_one_named_stops_the_stream_before_any_audio() -> TestResult {
    let seen = stream_naming("t003", "8080", "18083")?;
    assert_refused_on_port(&seen);
    Ok(())
}

#[test]
fn own_pid_without_own_llama_port_is_a_usage_error_and_writes_no_record() -> TestResult {
    let all_three = stream("t004a", &server_on("18083"), &named(&[]))?;
    assert_usage_error(&all_three, FLAG);
    let one = stream("t004b", &server_on("18083"), &own_pid_args(&[THE_SERVER]))?;
    assert_usage_error(&one, FLAG);
    Ok(())
}

#[test]
fn own_llama_port_without_own_pid_is_a_usage_error_and_writes_no_record() -> TestResult {
    let args = [String::from(FLAG), String::from("18083")];
    let seen = stream("t005", &server_on("18083"), &args)?;
    assert_usage_error(&seen, "--own-pid");
    Ok(())
}

#[test]
fn the_port_in_the_command_line_must_equal_the_port_named_and_not_a_fixed_one() -> TestResult {
    assert_refused_on_port(&stream_naming("t006a", "18084", "18083")?);
    assert_refused_on_port(&stream_naming("t006b", "18083", "18084")?);
    Ok(())
}

#[test]
fn a_port_that_only_contains_the_named_port_stops_the_stream() -> TestResult {
    assert_refused_on_port(&stream_naming("t007a", "180830", "18083")?);
    assert_refused_on_port(&stream_naming("t007b", "118083", "18083")?);
    Ok(())
}

#[test]
fn a_port_flag_joined_with_equals_stops_the_stream_on_the_named_port() -> TestResult {
    let words = ["llama-server", "--port=18083"];
    let seen = stream("t008", &words, &named(&[FLAG, "18083"]))?;
    assert_refused_on_port(&seen);
    Ok(())
}

#[test]
fn two_port_flags_stop_the_stream_on_the_named_port() -> TestResult {
    let cases: [(&str, [&str; 4]); 3] = [
        ("t009a", ["--port", "18083", "--port", "18083"]),
        ("t009b", ["--port", "8080", "--port", "18083"]),
        ("t009c", ["--port", "18083", "--port", "8080"]),
    ];
    for (name, tail) in cases {
        let mut words = vec!["llama-server"];
        words.extend(tail);
        assert_refused_on_port(&stream(name, &words, &named(&[FLAG, "18083"]))?);
    }
    Ok(())
}

#[test]
fn a_port_flag_with_no_value_stops_the_stream_on_the_named_port() -> TestResult {
    let words = ["llama-server", "-m", "model.gguf", "--port"];
    let seen = stream("t010", &words, &named(&[FLAG, "18083"]))?;
    assert_refused_on_port(&seen);
    Ok(())
}

#[test]
fn a_named_port_that_is_not_the_value_of_port_stops_the_stream() -> TestResult {
    let after_another_flag = ["llama-server", "--port", "8080", "--parallel", "18083"];
    let before_the_flag = ["llama-server", "18083", "--port", "8080"];
    for (name, words) in [
        ("t011a", &after_another_flag[..]),
        ("t011b", &before_the_flag[..]),
    ] {
        assert_refused_on_port(&stream(name, words, &named(&[FLAG, "18083"]))?);
    }
    Ok(())
}

#[test]
fn a_port_written_with_a_leading_zero_does_not_match_the_named_port() -> TestResult {
    assert_refused_on_port(&stream_naming("t012", "018083", "18083")?);
    Ok(())
}

#[test]
fn a_non_numeric_own_llama_port_is_a_usage_error_and_not_a_refusal() -> TestResult {
    assert_usage_error(&stream_naming("t013", "18083", "abc")?, FLAG);
    Ok(())
}

/// A line that gives 18084 as the port of the check and does not say that the run names it.
fn names_the_fixed_port(line: &str) -> bool {
    line.contains("18084") && !line.contains(FLAG)
}

fn is_a_doc_line(line: &str, markers: &[&str]) -> bool {
    let line = line.trim_start();
    markers.iter().any(|marker| line.starts_with(marker))
}

/// The lines of the file `relative` (under the repository) that `chosen` picks and
/// `names_the_fixed_port` flags, with how many lines `chosen` picked.
fn flagged_in(relative: &str, chosen: impl Fn(&str) -> bool) -> Outcome<(usize, Vec<String>)> {
    let text = std::fs::read_to_string(Path::new(REPO).join(relative))?;
    let picked: Vec<&str> = text.lines().filter(|line| chosen(line)).collect();
    let flagged = picked
        .iter()
        .filter(|line| names_the_fixed_port(line))
        .map(|line| (*line).to_owned())
        .collect();
    Ok((picked.len(), flagged))
}

#[test]
fn no_comment_or_readme_line_names_18084_as_the_fixed_port_of_the_check() -> TestResult {
    // The expression finds a line that fixes the port and passes one that names the flag.
    assert!(names_the_fixed_port("/// on port 18084"));
    assert!(!names_the_fixed_port("/// --own-llama-port 18084"));
    let machine = "native/hayamimi-eval/src/machine.rs";
    let cli = "native/hayamimi-eval/src/cli.rs";
    let readme = "docs/results/e2e/README.md";
    let docs = |line: &str| is_a_doc_line(line, &["//!", "///"]);
    let own_pid_lines = |line: &str| line.contains("--own-pid");
    for (relative, (picked, flagged)) in [
        (machine, flagged_in(machine, docs)?),
        (cli, flagged_in(cli, docs)?),
        (readme, flagged_in(readme, own_pid_lines)?),
    ] {
        assert!(picked > 0, "no line of {relative} was read");
        assert!(flagged.is_empty(), "{relative}: {flagged:?}");
    }
    Ok(())
}

#[test]
fn an_own_llama_port_of_zero_is_read_and_stops_the_stream_at_the_port_check() -> TestResult {
    assert_refused_on_port(&stream_naming("t015a", "18083", "0")?);
    assert_refused_on_port(&stream_naming("t015b", "8080", "0")?);
    Ok(())
}

#[test]
fn the_largest_u16_port_is_accepted_when_the_server_listens_on_it() -> TestResult {
    assert_accepted(&stream_naming("t016", "65535", "65535")?)
}

#[test]
fn an_own_llama_port_one_above_u16_is_a_usage_error_and_not_a_refusal() -> TestResult {
    assert_usage_error(&stream_naming("t017", "18083", "65536")?, FLAG);
    Ok(())
}

#[test]
fn a_negative_own_llama_port_is_a_usage_error_in_both_spellings() -> TestResult {
    let two_words = stream("t018a", &server_on("18083"), &named(&[FLAG, "-1"]))?;
    assert_usage_error(&two_words, FLAG);
    let one_word = stream(
        "t018b",
        &server_on("18083"),
        &named(&["--own-llama-port=-1"]),
    )?;
    assert_usage_error(&one_word, FLAG);
    Ok(())
}

#[test]
fn an_empty_own_llama_port_is_a_usage_error() -> TestResult {
    assert_usage_error(&stream_naming("t019", "18083", "")?, FLAG);
    Ok(())
}

#[test]
fn an_own_llama_port_given_twice_is_a_usage_error_and_neither_value_is_taken() -> TestResult {
    let different = named(&[FLAG, "18083", FLAG, "18084"]);
    assert_usage_error(&stream("t020a", &server_on("18083"), &different)?, FLAG);
    let same = named(&[FLAG, "18083", FLAG, "18083"]);
    assert_usage_error(&stream("t020b", &server_on("18083"), &same)?, FLAG);
    Ok(())
}

#[test]
fn an_own_llama_port_written_with_a_leading_zero_is_read_as_the_number() -> TestResult {
    assert_accepted(&stream_naming("t021", "18083", "018083")?)
}

#[test]
fn a_command_line_port_written_with_a_leading_zero_does_not_match_even_the_same_spelling()
-> TestResult {
    assert_refused_on_port(&stream_naming("t022", "018083", "018083")?);
    Ok(())
}
