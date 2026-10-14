//! RP-21: `hayamimi models fetch` gets the model files with no Python. The cases
//! here run the command: T-RP-21-001, -005, -006, -007, -012 and -013. The cases
//! that read the manifest as a fixture (-002 to -004 and -008 to -011) are in
//! `hayamimi-eval/tests/rp21.rs`.
//!
//! The cases that fetch from the network are `#[ignore]`: run them with
//! `cargo test --release -p hayamimi --test rp21 -- --ignored`. -006 needs no
//! network: it breaks both sources on purpose.

mod support;

use std::collections::BTreeMap;
use std::fs;
use std::net::{Ipv4Addr, TcpListener};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

use hayamimi_eval::descendants::{check_descendants, watch};
use hayamimi_eval::error::EvalError;
use hayamimi_eval::sha::sha256_hex;
use serde_json::Value;
use support::{Failure, Output, TestResult, collect, events, named, path_text, run, spawn};

/// The address of a source that is broken on purpose: a server that closes every connection at once.
/// (A closed port is not used: some hosts drop the connect and make it wait out the timeout.)
fn broken_source() -> Result<&'static str, Failure> {
    static ADDRESS: OnceLock<Result<String, Failure>> = OnceLock::new();
    ADDRESS
        .get_or_init(|| {
            let listener =
                TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|e| e.to_string())?;
            let port = listener.local_addr().map_err(|e| e.to_string())?.port();
            std::thread::spawn(move || {
                for connection in listener.incoming() {
                    drop(connection);
                }
            });
            Ok(format!("http://127.0.0.1:{port}"))
        })
        .as_deref()
        .map_err(Clone::clone)
}

const RECOGNIZER_DIR: &str = "sherpa-onnx-zipformer-ja-en-reazonspeech-2025-01-17";
const LANGUAGE_ID_DIR: &str = "sherpa-onnx-whisper-tiny";
const PUNCTUATION_DIR: &str = "mojicast-punct-onnx";

/// The four groups of `--minimal`, as the stages the manifest names them.
const MINIMAL_STAGES: [&str; 4] = ["language_id", "punctuation", "recognizer", "vad"];

fn scratch(name: &str) -> Result<PathBuf, Failure> {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("rp21-{name}"));
    if root.exists() {
        fs::remove_dir_all(&root).map_err(|e| format!("{}: {e}", root.display()))?;
    }
    fs::create_dir_all(&root).map_err(|e| format!("{}: {e}", root.display()))?;
    Ok(root)
}

fn fetch_args<'a>(dir: &'a str, extra: &[&'a str]) -> Vec<&'a str> {
    let mut args = vec!["models", "fetch", "--minimal", "--dir", dir];
    args.extend_from_slice(extra);
    args
}

/// Every file under `dir`: its path relative to `dir`, bytes and modification time.
type Files = BTreeMap<String, (Vec<u8>, SystemTime)>;

fn read_tree(root: &Path, dir: &Path, found: &mut Files) -> Result<(), Failure> {
    for entry in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            read_tree(root, &path, found)?;
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        let modified = fs::metadata(&path)
            .and_then(|meta| meta.modified())
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let bytes = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        found.insert(relative, (bytes, modified));
    }
    Ok(())
}

fn tree(dir: &Path) -> Result<Files, Failure> {
    let mut found = Files::new();
    read_tree(dir, dir, &mut found)?;
    Ok(found)
}

/// The entries of `manifest.json` in `dir`.
fn manifest(dir: &Path) -> Result<Vec<Value>, Failure> {
    let path = dir.join("manifest.json");
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let parsed: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    parsed
        .get("files")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| String::from("the manifest has no `files` list"))
}

fn text_of<'a>(entry: &'a Value, key: &str) -> Result<&'a str, Failure> {
    entry
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("an entry has no `{key}`: {entry}"))
}

/// Runs `models fetch` to its end.
fn fetch(dir: &Path, extra: &[&str]) -> Result<Output, Failure> {
    let dir = path_text(dir)?;
    run(&fetch_args(&dir, extra), None, &[])
}

/// A fetch that was watched while it ran: its output, and whether any process below it was Python.
struct Watched {
    output: Output,
    samples: usize,
    tree: Result<(), String>,
}

fn watched_fetch(dir: &Path) -> Result<Watched, Failure> {
    let dir = path_text(dir)?;
    let mut child = spawn(&fetch_args(&dir, &[]), None, &[])?;
    let out = child.stdout.take().map(collect);
    let err = child.stderr.take().map(collect);
    let root = child.id();
    let seen = watch(root, Duration::from_millis(100), || {
        child
            .try_wait()
            .map(|status| status.is_none())
            .map_err(|source| EvalError::Io {
                path: String::from("child"),
                source,
            })
    })
    .map_err(|e| e.to_string())?;
    let status = child.wait().map_err(|e| e.to_string())?;
    let joined = |handle: Option<std::thread::JoinHandle<String>>| match handle {
        Some(handle) => handle.join().map_err(|_| String::from("a reader panicked")),
        None => Err(String::from("the child had no pipe")),
    };
    Ok(Watched {
        output: Output {
            code: status.code(),
            stdout: joined(out)?,
            stderr: joined(err)?,
        },
        samples: seen.samples,
        tree: check_descendants(&seen.seen, root).map_err(|e| e.to_string()),
    })
}

/// The first fetch into an empty directory, shared by the cases that read its result.
struct First {
    dir: PathBuf,
    watched: Watched,
}

fn first() -> Result<&'static First, Failure> {
    static FIRST: OnceLock<Result<First, Failure>> = OnceLock::new();
    FIRST
        .get_or_init(|| {
            let dir = scratch("first")?;
            let watched = watched_fetch(&dir)?;
            Ok(First { dir, watched })
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// A second fetch into the directory of the first, with both sources healthy.
struct Second {
    before: Files,
    after: Files,
    output: Output,
}

fn second() -> Result<&'static Second, Failure> {
    static SECOND: OnceLock<Result<Second, Failure>> = OnceLock::new();
    SECOND
        .get_or_init(|| {
            let first = first()?;
            let before = tree(&first.dir)?;
            let output = fetch(&first.dir, &[])?;
            let after = tree(&first.dir)?;
            Ok(Second {
                before,
                after,
                output,
            })
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// T-RP-21-001: the four groups arrive with a manifest, and each group is of the precision its stage reads.
#[test]
#[ignore = "fetches the minimal model set from the network"]
fn minimal_fetch_gets_the_four_groups_and_a_manifest() -> TestResult {
    let first = first()?;
    assert_eq!(
        first.watched.output.code,
        Some(0),
        "{}",
        first.watched.output.stderr
    );
    let entries = manifest(&first.dir)?;
    let mut stages: Vec<&str> = Vec::new();
    for entry in &entries {
        let stage = text_of(entry, "stage")?;
        if !stages.contains(&stage) {
            stages.push(stage);
        }
    }
    stages.sort_unstable();
    assert_eq!(stages, MINIMAL_STAGES);
    for entry in &entries {
        let (stage, file) = (text_of(entry, "stage")?, text_of(entry, "file")?);
        assert!(first.dir.join(file).is_file(), "{file}");
        let int8 = file.ends_with(".int8.onnx");
        match stage {
            "recognizer" if file.contains("encoder") => assert!(int8, "{file} must be int8"),
            "language_id" | "punctuation" => assert!(!int8, "{file} runs on the GPU"),
            _ => {}
        }
    }
    let encoder = entries
        .iter()
        .any(|e| matches!((text_of(e, "stage"), text_of(e, "file")), (Ok("recognizer"), Ok(f)) if f.contains("encoder")));
    assert!(encoder, "the recognizer's encoder is listed");
    Ok(())
}

/// T-RP-21-005: a second fetch of the same directory leaves every file as it was.
#[test]
#[ignore = "fetches the minimal model set from the network"]
fn second_fetch_does_not_refetch() -> TestResult {
    let second = second()?;
    assert_eq!(second.output.code, Some(0), "{}", second.output.stderr);
    assert!(!second.before.is_empty(), "the first fetch left files");
    let moved: Vec<&String> = second
        .after
        .iter()
        .filter(|(name, (_, time))| second.before.get(*name).map(|(_, was)| was) != Some(time))
        .map(|(name, _)| name)
        .collect();
    assert!(
        moved.is_empty(),
        "files touched by the second fetch: {moved:?}"
    );
    assert_eq!(
        second.before.keys().collect::<Vec<_>>(),
        second.after.keys().collect::<Vec<_>>()
    );
    let log = events(&second.output.stdout)?;
    assert_eq!(named(&log, "models.group.present").len(), 4);
    assert!(named(&log, "models.group.fetched").is_empty());
    Ok(())
}

/// The directory a previous install left, short of some files: one file in three groups, none of the VAD.
fn previous_layout(name: &str) -> Result<PathBuf, Failure> {
    let dir = scratch(name)?;
    for relative in [
        format!("{RECOGNIZER_DIR}/tokens.txt"),
        format!("{LANGUAGE_ID_DIR}/tiny-tokens.txt"),
        format!("{PUNCTUATION_DIR}/vocab.txt"),
    ] {
        let path = dir.join(&relative);
        let parent = path.parent().ok_or("no parent")?;
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        fs::write(&path, format!("previous {relative}")).map_err(|e| e.to_string())?;
    }
    Ok(dir)
}

/// T-RP-21-006: both sources broken, the second fetch keeps what is there, adds no file, says so once, and exits 0.
#[test]
fn broken_sources_keep_the_previous_files_and_say_so() -> TestResult {
    let dir = previous_layout("previous")?;
    let before = tree(&dir)?;
    let broken = broken_source()?;
    let output = fetch(&dir, &["--release-url", broken, "--hf-url", broken])?;
    assert_eq!(output.code, Some(0), "{}", output.stderr);
    let after = tree(&dir)?;
    let bytes = |files: &Files| -> BTreeMap<String, Vec<u8>> {
        files
            .iter()
            .map(|(k, (b, _))| (k.clone(), b.clone()))
            .collect()
    };
    assert_eq!(
        bytes(&after),
        bytes(&before),
        "the files are as they were and none was added"
    );
    let log = events(&output.stdout)?;
    let previous = named(&log, "models.fallback.previous");
    assert_eq!(previous.len(), 1, "{}", output.stdout);
    assert_eq!(previous[0]["error_code"], "HAYAMIMI_MODELS_FALLBACK");
    assert_eq!(previous[0]["groups"], 4);
    assert!(named(&log, "models.group.fetched").is_empty());
    assert!(named(&log, "models.source.failed").len() >= 4);
    fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(())
}

/// T-RP-21-007: no process below the command is Python while it fetches.
#[test]
#[ignore = "fetches the minimal model set from the network"]
fn fetch_runs_without_python() -> TestResult {
    let first = first()?;
    assert!(first.watched.samples >= 1);
    assert_eq!(
        first.watched.tree,
        Ok(()),
        "a process below the command was Python"
    );
    Ok(())
}

/// T-RP-21-012: with the release broken, the other source gives all four groups and the manifest agrees with the files.
#[test]
#[ignore = "fetches the minimal model set from the network"]
fn one_broken_source_falls_back_to_the_other_source() -> TestResult {
    let dir = scratch("release-broken")?;
    let output = fetch(&dir, &["--release-url", broken_source()?])?;
    assert_eq!(output.code, Some(0), "{}", output.stderr);
    let entries = manifest(&dir)?;
    let mut stages: Vec<&str> = entries
        .iter()
        .map(|e| text_of(e, "stage"))
        .collect::<Result<_, _>>()?;
    stages.sort_unstable();
    stages.dedup();
    assert_eq!(stages, MINIMAL_STAGES);
    for entry in &entries {
        let file = text_of(entry, "file")?;
        let bytes = fs::read(dir.join(file)).map_err(|e| format!("{file}: {e}"))?;
        assert_eq!(sha256_hex(&bytes), text_of(entry, "sha256")?, "{file}");
        assert_eq!(entry["bytes"].as_u64(), Some(bytes.len() as u64), "{file}");
    }
    let log = events(&output.stdout)?;
    assert!(named(&log, "models.fallback.previous").is_empty());
    assert!(
        !named(&log, "models.source.failed").is_empty(),
        "the broken source is told"
    );
    Ok(())
}

/// T-RP-21-013: with both sources healthy, a second fetch says nothing about previous files and exits 0.
#[test]
#[ignore = "fetches the minimal model set from the network"]
fn healthy_sources_emit_no_previous_files_event() -> TestResult {
    let second = second()?;
    assert_eq!(second.output.code, Some(0), "{}", second.output.stderr);
    let log = events(&second.output.stdout)?;
    assert!(named(&log, "models.fallback.previous").is_empty());
    assert!(!log.is_empty(), "the control: the run did print events");
    Ok(())
}
