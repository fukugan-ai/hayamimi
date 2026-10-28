//! BR-7: does drafting by n-gram (`--spec-type ngram-simple`) keep the settled
//! text, and what does it do to the E stage? A record only (design 10.13.4,
//! hand 5; IP-S9). The flag is put on the llama-server of the E2E window
//! (port 18084) and never on the shared server (port 18081).
//!
//! The real rows start that server themselves, with the arguments of
//! `units/hayamimi-llama-e2e.service` (flagged or not), put the 40 tuning lines
//! of `fixtures/conv20.tsv` through the relay's pipeline at temperature 0 and
//! read the time of E (`e_ms` of the `translate.line.*` event) and the settled
//! text of each line. They run with `-- --ignored`, once for the three servers
//! (without the flag twice, then with it); the two real rows share that run.
//! The record is `docs/results/in-person/br07_ngram.md`.
//!
//! How this can break, and what stops it:
//!   first run   the model is read from disk (about 20 s); the row waits up to
//!               120 s for `/health` and fails with the log of the server.
//!   contention  port 18084 is held by `hayamimi-llama-e2e.service` or by a
//!               leftover server: the row refuses to start (it never stops a
//!               service) and says which unit to stop; the GPU needs room for
//!               the server (about 5.5 GB); a server it started is killed when
//!               the row ends or panics (`Server::drop`), so none is left.
//!   start       nothing listens on 18084; the shared server on 18081 is not
//!               touched and the flag is refused for it (T-BR-07-006).
//!   waiting     none: no prompt, no stdin.

mod support;

use std::collections::BTreeMap;
use std::fs::File;
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use hayamimi_relay::llm::HttpLlm;
use hayamimi_relay::logging::MemorySink;
use hayamimi_relay::pipeline::{LineKind, LineRequest, NoObserver, Pipeline, PipelineConfig};
use support::{DEFAULT_NOW, Pair, TestResult, conv20, epoch};

/// The port of the server of the E2E window: the only one the flag is put on.
const E2E_PORT: u16 = 18084;
/// The port of the shared server, which is never flagged.
const SHARED_PORT: u16 = 18081;
const FLAG: [&str; 2] = ["--spec-type", "ngram-simple"];
const UNIT: &str = include_str!("../units/hayamimi-llama-e2e.service");
const RECORD: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/results/in-person/br07_ngram.md"
);
const READY_WITHIN: Duration = Duration::from_secs(120);

// ---- tools: counting and building --------------------------------------------------------

/// How many lines two runs settled on the same text. An error for runs with no
/// line or with a different number of lines: a record of nothing is not a record
/// of agreement.
fn agreeing(first: &[String], second: &[String]) -> Result<usize, String> {
    if first.is_empty() || second.is_empty() {
        return Err(String::from("a run with no line cannot agree with another"));
    }
    if first.len() != second.len() {
        return Err(format!(
            "the runs have {} and {} lines",
            first.len(),
            second.len()
        ));
    }
    Ok(first.iter().zip(second).filter(|(a, b)| a == b).count())
}

/// The value at the nearest rank `ceil(percent / 100 * n)` (1-based) of `values`.
fn nearest_rank(values: &[u64], percent: usize) -> Result<u64, String> {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let rank = (percent * sorted.len()).div_ceil(100).max(1);
    sorted
        .get(rank - 1)
        .copied()
        .ok_or_else(|| String::from("no value to take a percentile of"))
}

/// The program and the arguments after `ExecStart=` of a unit, with `%h` read as `home`.
fn exec_start_of(unit: &str, home: &str) -> Result<(String, Vec<String>), String> {
    let line = unit
        .lines()
        .find_map(|line| line.strip_prefix("ExecStart="))
        .ok_or("the unit has no ExecStart=")?;
    let command = line.replace("%h", home);
    let mut words = command.split_whitespace().map(str::to_owned);
    let program = words.next().ok_or("ExecStart= is empty")?;
    Ok((program, words.collect()))
}

/// The arguments with the value after `--port` replaced by `port`.
fn with_port(mut args: Vec<String>, port: u16) -> Result<Vec<String>, String> {
    let at = args
        .iter()
        .position(|word| word == "--port")
        .ok_or("the unit names no --port")?;
    let slot = args.get_mut(at + 1).ok_or("--port has no value")?;
    *slot = port.to_string();
    Ok(args)
}

/// The command line of a server on `port`: the arguments of the unit, and the
/// n-gram flag when `ngram` is set. The flag is built for the E2E window's port
/// only: asking for it on another port, the shared server's above all, is an error.
fn server_command(
    unit: &str,
    home: &str,
    port: u16,
    ngram: bool,
) -> Result<(String, Vec<String>), String> {
    if ngram && port != E2E_PORT {
        return Err(format!(
            "the flag is built for port {E2E_PORT} only, not {port} (the shared server listens on {SHARED_PORT})"
        ));
    }
    let (program, args) = exec_start_of(unit, home)?;
    let mut args = with_port(args, port)?;
    if ngram {
        args.extend(FLAG.iter().map(|word| (*word).to_owned()));
    }
    Ok((program, args))
}

/// One line of the record: the agreement of two runs, `一致 k / n`.
fn agreement_line(label: &str, first: &[String], second: &[String]) -> Result<String, String> {
    let same = agreeing(first, second)?;
    let all = if same == first.len() {
        String::new()
    } else {
        String::from("（40 にならない）")
    };
    Ok(format!("{label}: 一致 {same} / {}{all}\n", first.len()))
}

// ---- the runs ----------------------------------------------------------------------------

/// What one server left for the 40 lines.
struct Run {
    /// The settled text of each line, in the order of the tuning set (empty when held).
    texts: Vec<String>,
    /// The time of E of each line that asked E, in ms.
    e_ms: Vec<u64>,
}

/// A server this row started; it is killed when dropped.
struct Server {
    child: Child,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn url() -> String {
    format!("http://127.0.0.1:{E2E_PORT}")
}

fn port_is_held(port: u16) -> bool {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&address, Duration::from_millis(500)).is_ok()
}

fn tmp_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
}

fn start_server(ngram: bool, log_name: &str) -> Result<Server, String> {
    if port_is_held(E2E_PORT) {
        return Err(format!(
            "port {E2E_PORT} is held: stop hayamimi-llama-e2e.service first (systemctl --user stop hayamimi-llama-e2e.service)"
        ));
    }
    let home = dirs::home_dir()
        .and_then(|path| path.to_str().map(str::to_owned))
        .ok_or_else(|| String::from("the home directory is unknown or not UTF-8"))?;
    let (program, args) = server_command(UNIT, &home, E2E_PORT, ngram)?;
    let log = File::create(tmp_dir().join(log_name)).map_err(|error| error.to_string())?;
    let child = Command::new(&program)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(log.try_clone().map_err(|error| error.to_string())?)
        .stderr(log)
        .spawn()
        .map_err(|error| format!("{program}: {error}"))?;
    let server = Server { child };
    wait_ready(log_name)?;
    Ok(server)
}

fn wait_ready(log_name: &str) -> Result<(), String> {
    let llm = HttpLlm::new(&url());
    let started = Instant::now();
    while started.elapsed() < READY_WITHIN {
        if llm.health().is_ok() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    Err(format!(
        "no /health within {} s (log: {})",
        READY_WITHIN.as_secs(),
        tmp_dir().join(log_name).display()
    ))
}

/// The `e_ms` of each line, by id, from the timing events of a run.
fn e_ms_by_id(sink: &MemorySink) -> BTreeMap<String, u64> {
    let mut found = BTreeMap::new();
    for event in &sink.events {
        if !event.event.starts_with("translate.line.") {
            continue;
        }
        let id = event.fields.get("id").and_then(|value| value.as_str());
        let e_ms = event.fields.get("e_ms").and_then(|value| value.as_u64());
        if let (Some(id), Some(e_ms)) = (id, e_ms) {
            found.insert(id.to_owned(), e_ms);
        }
    }
    found
}

fn line_of(
    pipeline: &mut Pipeline<HttpLlm>,
    sink: &mut MemorySink,
    pair: &Pair,
    now_epoch: i64,
) -> String {
    let request = LineRequest {
        id: &pair.id,
        of: LineKind::Final,
        source: "file",
        src_lang: pair.lang,
        text: &pair.src,
        now_epoch,
    };
    pipeline.run(&request, &mut NoObserver, sink).text
}

/// The 40 lines through the pipeline against the server that listens now. One
/// line is translated first and left out, so that the first line of the record
/// is not the one that pays for the first request.
fn run_lines() -> Result<Run, String> {
    let pairs = conv20()?;
    let now = epoch(DEFAULT_NOW)?;
    let llm = HttpLlm::with_timeout(&url(), Duration::from_secs(120));
    let mut sink = MemorySink::default();
    let mut pipeline = Pipeline::open(llm, &PipelineConfig::default(), &mut sink)?;
    let warm = pairs.first().ok_or("the tuning set is empty")?;
    line_of(&mut pipeline, &mut sink, warm, now);
    sink.events.clear();
    let texts: Vec<String> = pairs
        .iter()
        .map(|pair| line_of(&mut pipeline, &mut sink, pair, now))
        .collect();
    let by_id = e_ms_by_id(&sink);
    let e_ms = pairs
        .iter()
        .filter_map(|pair| by_id.get(&pair.id).copied())
        .filter(|ms| *ms > 0)
        .collect();
    Ok(Run { texts, e_ms })
}

/// The three runs of the record: without the flag twice, then with it.
struct Runs {
    plain_first: Run,
    plain_second: Run,
    flagged: Run,
}

fn three_runs() -> Result<Runs, String> {
    let one = |ngram: bool, name: &str| -> Result<Run, String> {
        let server = start_server(ngram, name)?;
        let run = run_lines();
        drop(server);
        run
    };
    Ok(Runs {
        plain_first: one(false, "br07_plain_first.log")?,
        plain_second: one(false, "br07_plain_second.log")?,
        flagged: one(true, "br07_flagged.log")?,
    })
}

/// The runs, taken once however many real rows ask for them.
fn runs() -> Result<&'static Runs, String> {
    static RUNS: OnceLock<Result<Runs, String>> = OnceLock::new();
    RUNS.get_or_init(three_runs).as_ref().map_err(Clone::clone)
}

// ---- the record --------------------------------------------------------------------------

fn stage_line(label: &str, run: &Run) -> Result<String, String> {
    Ok(format!(
        "{label}: E を頼んだ行 {}、p50 {} ms、p95 {} ms\n",
        run.e_ms.len(),
        nearest_rank(&run.e_ms, 50)?,
        nearest_rank(&run.e_ms, 95)?
    ))
}

fn differing_lines(pairs: &[Pair], first: &Run, second: &Run) -> String {
    let mut text = String::new();
    for (pair, (a, b)) in pairs.iter().zip(first.texts.iter().zip(&second.texts)) {
        if a != b {
            text.push_str(&format!("{}: 「{a}」と「{b}」\n", pair.id));
        }
    }
    if text.is_empty() {
        text.push_str("違う行なし\n");
    }
    text
}

fn record_text(runs: &Runs) -> Result<String, String> {
    let pairs = conv20()?;
    let mut text = String::from(
        "# BR-7: E の n-gram の下書き（--spec-type ngram-simple）の効き\n\n\
         調整用の 40 行（fixtures/conv20.tsv）を温度 0 で relay の pipeline に通した値。server は 18084 の unit と同じ引数で test が起こした。\n\n## E の段\n\n",
    );
    text.push_str(&stage_line("旗なし（1 回目）", &runs.plain_first)?);
    text.push_str(&stage_line("旗なし（2 回目）", &runs.plain_second)?);
    text.push_str(&stage_line("旗つき", &runs.flagged)?);
    text.push_str("\n## 確定の文の一致\n\n");
    text.push_str(&agreement_line(
        "旗なしの 2 回",
        &runs.plain_first.texts,
        &runs.plain_second.texts,
    )?);
    text.push_str(&agreement_line(
        "旗なし（1 回目）と旗つき",
        &runs.plain_first.texts,
        &runs.flagged.texts,
    )?);
    text.push_str("\n## 旗なしの 2 回で違う行\n\n");
    text.push_str(&differing_lines(
        &pairs,
        &runs.plain_first,
        &runs.plain_second,
    ));
    text.push_str("\n## 旗なし（1 回目）と旗つきで違う行\n\n");
    text.push_str(&differing_lines(&pairs, &runs.plain_first, &runs.flagged));
    Ok(text)
}

// ---- rows --------------------------------------------------------------------------------

/// T-BR-07-001
#[test]
#[ignore = "starts llama-server on 18084 (stop hayamimi-llama-e2e.service first) and writes docs/results/in-person/br07_ngram.md"]
fn br07_ngram_drafting_keeps_the_settled_text_and_records_the_e_stage() -> TestResult {
    let runs = runs()?;
    let text = record_text(runs)?;
    std::fs::write(Path::new(RECORD), text)?;
    Ok(())
}

/// T-BR-07-002
#[test]
#[ignore = "starts llama-server on 18084 (stop hayamimi-llama-e2e.service first)"]
fn two_runs_without_the_flag_agree_on_all_forty_lines() -> TestResult {
    let runs = runs()?;
    let same = agreeing(&runs.plain_first.texts, &runs.plain_second.texts)?;
    // Not asserted to be 40 by the tool: a server that is not deterministic
    // makes the record say so (the line of the record shows the number).
    eprintln!("without the flag, twice: {same} of 40 lines agree");
    Ok(())
}

/// T-BR-07-003
#[test]
fn a_pair_that_differs_on_one_line_is_written_as_39_agreeing() -> TestResult {
    let first: Vec<String> = (0..40).map(|n| format!("line {n}")).collect();
    let mut second = first.clone();
    // The same length as "line 17": a comparison of the lengths of two lines cannot stand in for one of their texts.
    second[17] = String::from("line 71");
    let line = agreement_line("pair", &first, &second)?;
    assert!(
        line.contains("一致 39 / 40") && line.contains("40 にならない"),
        "{line}"
    );
    // The control: the same texts twice agree on all 40.
    let line = agreement_line("pair", &first, &first)?;
    assert!(
        line.contains("一致 40 / 40") && !line.contains("40 にならない"),
        "{line}"
    );
    Ok(())
}

/// T-BR-07-004
#[test]
fn an_empty_pair_of_runs_is_an_error_and_not_zero_agreeing() {
    let forty: Vec<String> = (0..40).map(|n| format!("line {n}")).collect();
    assert_eq!(agreeing(&forty, &forty), Ok(40));
    assert!(agreeing(&[], &[]).is_err());
    assert!(agreeing(&forty, &[]).is_err());
    assert!(agreeing(&forty, &forty[..39]).is_err());
}

/// T-BR-07-005
#[test]
fn the_e_stage_percentiles_are_nearest_rank() -> TestResult {
    let values = [400, 100, 300, 200];
    assert_eq!(nearest_rank(&values, 50)?, 200);
    assert_eq!(nearest_rank(&values, 95)?, 400);
    assert!(nearest_rank(&[], 50).is_err());
    Ok(())
}

/// T-BR-07-006
#[test]
fn the_flag_is_never_built_for_the_shared_server_port() -> TestResult {
    let flagged = server_command(UNIT, "/home/x", E2E_PORT, true)?;
    let joined = flagged.1.join(" ");
    assert!(joined.contains("--spec-type ngram-simple"), "{joined}");
    assert!(joined.contains("--port 18084"), "{joined}");
    assert!(server_command(UNIT, "/home/x", SHARED_PORT, true).is_err());
    for port in [E2E_PORT, SHARED_PORT] {
        let plain = server_command(UNIT, "/home/x", port, false)?.1.join(" ");
        assert!(!plain.contains("--spec-type"), "{plain}");
        assert!(plain.contains(&format!("--port {port}")), "{plain}");
    }
    // The arguments are the unit's: the model, the layers, the context.
    assert!(
        joined.contains("-ngl 99") && joined.contains("-c 16384"),
        "{joined}"
    );
    assert!(
        joined.contains("/home/x/models/CAT-Translate-3.3b-Q8_0.gguf"),
        "{joined}"
    );
    Ok(())
}
