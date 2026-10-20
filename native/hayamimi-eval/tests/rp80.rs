//! RP-80: the CI workflow runs cargo, runs no Python, and prints the number of
//! unported rows on every run.
//!
//! `check_workflow` reads the text of `.github/workflows/test.yml`, the text of
//! `docs/results/r11/ci_probe.md` and the test binaries under
//! `native/<crate>/tests/`. It reads lines and does not parse YAML: a step is
//! the lines from one `- ` to the next, and the workflow lists its steps with
//! no other `- ` line.
//!
//! The Python words are searched line by line, comment lines included. The name
//! of the test that counts the unported rows (`rp19_python_cases_are_ported`)
//! holds one of the words and is taken out of a line before the search.

mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use support::{Failure, REPO, TestResult, read};

const WORKFLOW_BASE: &str = include_str!("fixtures/rp80_base/test.yml");
const PROBE_BASE: &str = include_str!("fixtures/rp80_base/ci_probe.md");
const WORKFLOW_BEFORE_R11: &str = include_str!("fixtures/rp80_before_r11/test.yml");

/// The words the design searches for, as a plain substring search.
const PYTHON_WORDS: [&str; 4] = ["python", "pip ", "pytest", "setup-python"];
/// The one name that holds a Python word and is allowed in the workflow.
const COUNT_TEST_NAME: &str = "rp19_python_cases_are_ported";
/// The count steps: the test binary and the one test it runs.
const RP19_COUNT: (&str, &str) = ("rp19", COUNT_TEST_NAME);
const RP81_COUNT: (&str, &str) = ("rp81", "rp81_owned_exceptions_are_gone");
/// What a reason in `ci_probe.md` has to say for a binary left out of the CI.
const REASON_WORDS: [&str; 2] = ["testdata/", "models/"];
const CLONE_FROM_HOME: &str = ".cache/hayamimi-r11/ci-probe";

/// A test binary: the crate that owns it and its target name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Binary {
    krate: String,
    name: String,
}

fn binary(krate: &str, name: &str) -> Binary {
    Binary {
        krate: krate.to_owned(),
        name: name.to_owned(),
    }
}

/// One step of the workflow.
struct Step<'a> {
    run: Option<&'a str>,
    continue_on_error: bool,
}

fn parse_steps(workflow: &str) -> Vec<Step<'_>> {
    let mut steps: Vec<Step<'_>> = Vec::new();
    for line in workflow.lines() {
        let trimmed = line.trim();
        let content = match trimmed.strip_prefix("- ") {
            Some(rest) => {
                steps.push(Step {
                    run: None,
                    continue_on_error: false,
                });
                rest
            }
            None => trimmed,
        };
        let Some(step) = steps.last_mut() else {
            continue;
        };
        if let Some(command) = content.strip_prefix("run:") {
            step.run = Some(command.trim());
        } else if content == "continue-on-error: true" {
            step.continue_on_error = true;
        }
    }
    steps
}

/// The words that follow each occurrence of `flag` in a command.
fn values_after<'a>(run: &'a str, flag: &str) -> BTreeSet<&'a str> {
    let words: Vec<&str> = run.split_whitespace().collect();
    words
        .windows(2)
        .filter(|pair| pair[0] == flag)
        .map(|pair| pair[1])
        .collect()
}

/// The lines that hold a Python word, as `line N: text`.
fn python_lines(workflow: &str) -> Vec<String> {
    workflow
        .lines()
        .enumerate()
        .filter(|(_, line)| {
            let scrubbed = line.replace(COUNT_TEST_NAME, "");
            PYTHON_WORDS.iter().any(|word| scrubbed.contains(word))
        })
        .map(|(index, line)| format!("line {}: {}", index + 1, line.trim()))
        .collect()
}

/// The steps that run `--test <test>` with `--exact`.
fn count_steps<'a>(steps: &'a [Step<'a>], test: &str) -> Vec<&'a Step<'a>> {
    steps
        .iter()
        .filter(|step| {
            step.run.is_some_and(|run| {
                run.contains("--exact") && values_after(run, "--test").contains(test)
            })
        })
        .collect()
}

fn check_count_step(steps: &[Step<'_>], (test, exact): (&str, &str)) -> Result<(), Failure> {
    let candidates = count_steps(steps, test);
    if candidates.is_empty() {
        return Err(format!("no step runs `--test {test} -- --exact {exact}`"));
    }
    let Some(step) = candidates.iter().find(|step| {
        step.run
            .is_some_and(|run| values_after(run, "--exact").contains(exact))
    }) else {
        return Err(format!(
            "the step for `{test}` runs another test; `--exact {exact}` is the one that prints the count"
        ));
    };
    if !step.continue_on_error {
        return Err(format!(
            "the `{test}` count step has no `continue-on-error: true`"
        ));
    }
    Ok(())
}

/// The binaries that the test steps run: every binary when a step names no
/// `--test`, the named ones otherwise, less the crates a step excludes.
fn covered(binaries: &[Binary], steps: &[Step<'_>]) -> BTreeSet<Binary> {
    let mut covered = BTreeSet::new();
    let runs = steps
        .iter()
        .filter_map(|step| step.run)
        .filter(|run| run.starts_with("cargo test") && !run.contains("--exact"));
    for run in runs {
        let named = values_after(run, "--test");
        let excluded = values_after(run, "--exclude");
        for candidate in binaries {
            let selected = named.is_empty() || named.contains(candidate.name.as_str());
            if selected && !excluded.contains(candidate.krate.as_str()) {
                covered.insert(candidate.clone());
            }
        }
    }
    covered
}

/// The rows of `ci_probe.md`: the first cell names a binary, the second gives
/// the reason, and a row with one cell has none.
fn probe_rows(note: &str) -> BTreeMap<&str, Option<&str>> {
    note.lines()
        .map(str::trim)
        .filter(|line| line.starts_with('|'))
        .filter_map(|line| {
            let mut cells = line.trim_matches('|').split('|').map(str::trim);
            let name = cells.next()?.trim_matches('`');
            Some((name, cells.next()))
        })
        .collect()
}

fn excluded_problem(left_out: &Binary, rows: &BTreeMap<&str, Option<&str>>) -> Option<String> {
    let name = &left_out.name;
    let Some(reason) = rows.get(name.as_str()) else {
        return Some(format!(
            "`{name}` is left out of the test step and has no row in ci_probe.md"
        ));
    };
    match reason {
        Some(text) if !text.is_empty() => {
            if REASON_WORDS.iter().any(|word| text.contains(word)) {
                None
            } else {
                Some(format!(
                    "`{name}` is left out for a reason that does not say it reads testdata/ or models/"
                ))
            }
        }
        _ => Some(format!(
            "`{name}` has a row in ci_probe.md without a reason"
        )),
    }
}

fn check_excluded(steps: &[Step<'_>], note: &str, binaries: &[Binary]) -> Result<(), Failure> {
    let covered = covered(binaries, steps);
    let rows = probe_rows(note);
    let problems: Vec<String> = binaries
        .iter()
        .filter(|candidate| !covered.contains(*candidate))
        .filter_map(|left_out| excluded_problem(left_out, &rows))
        .collect();
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("\n"))
    }
}

fn check_workflow(workflow: &str, note: &str, binaries: &[Binary]) -> Result<(), Failure> {
    let python = python_lines(workflow);
    if !python.is_empty() {
        return Err(format!("the workflow names Python:\n{}", python.join("\n")));
    }
    let steps = parse_steps(workflow);
    check_count_step(&steps, RP19_COUNT)?;
    check_count_step(&steps, RP81_COUNT)?;
    if !binaries
        .iter()
        .any(|candidate| candidate.name == RP19_COUNT.0)
    {
        return Err(format!(
            "`{}` has no test file, so its count step runs nothing",
            RP19_COUNT.0
        ));
    }
    check_excluded(&steps, note, binaries)
}

fn dir_entries(dir: &Path) -> Result<Vec<PathBuf>, Failure> {
    let listing = std::fs::read_dir(dir)
        .map_err(|error| format!("cannot read {}: {error}", dir.display()))?;
    listing
        .map(|entry| {
            entry
                .map(|found| found.path())
                .map_err(|error| format!("cannot list {}: {error}", dir.display()))
        })
        .collect()
}

/// The name of the test target a path under `tests/` makes, if it makes one:
/// `<name>.rs`, or `<name>/main.rs`.
fn target_name(entry: &Path) -> Option<String> {
    let is_source = entry.is_file() && entry.extension().is_some_and(|extension| extension == "rs");
    let is_directory_target = entry.is_dir() && entry.join("main.rs").is_file();
    if !is_source && !is_directory_target {
        return None;
    }
    let stem = if is_source {
        entry.file_stem()
    } else {
        entry.file_name()
    };
    stem.and_then(|name| name.to_str()).map(str::to_owned)
}

/// Every test binary under `native/<crate>/tests/`.
fn repo_binaries(native: &Path) -> Result<Vec<Binary>, Failure> {
    let mut found = Vec::new();
    for krate in dir_entries(native)? {
        let tests = krate.join("tests");
        let Some(krate_name) = krate.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !tests.is_dir() {
            continue;
        }
        for entry in dir_entries(&tests)? {
            if let Some(name) = target_name(&entry) {
                found.push(binary(krate_name, &name));
            }
        }
    }
    found.sort();
    Ok(found)
}

// ---------------------------------------------------------------------------
// The base fixture (F80) and the edits the cases make to it.
// ---------------------------------------------------------------------------

const COUNT_STEP_19: &str = "      - name: RP-19 rows not yet ported\n        continue-on-error: true\n        run: cargo test --release --jobs 2 -p hayamimi-eval --test rp19 -- --exact rp19_python_cases_are_ported\n";
const COUNT_STEP_81: &str = "      - name: RP-81 owned exceptions\n        continue-on-error: true\n        run: cargo test --release --jobs 2 -p hayamimi-eval --test rp81 -- --exact rp81_owned_exceptions_are_gone\n";
const TEST_STEP_TAIL: &str = "--test rp80 --test rp81\n";
const PROBE_ROW: &str = "| `rp03` | reads `testdata/` and fails without it |\n";

fn f80_binaries() -> Vec<Binary> {
    ["rp02", "rp03", "rp19", "rp80"]
        .iter()
        .map(|name| binary("hayamimi-eval", name))
        .collect()
}

/// `base` with its first `from` replaced by `to`; a `from` that is not there is
/// an error, so a case cannot pass by editing nothing.
fn edit(base: &str, from: &str, to: &str) -> Result<String, Failure> {
    if !base.contains(from) {
        return Err(format!("the base fixture does not hold {from:?}"));
    }
    Ok(base.replacen(from, to, 1))
}

fn judge_workflow(workflow: &str) -> Result<(), Failure> {
    check_workflow(workflow, PROBE_BASE, &f80_binaries())
}

fn judge_probe(note: &str) -> Result<(), Failure> {
    check_workflow(WORKFLOW_BASE, note, &f80_binaries())
}

fn accepted(verdict: Result<(), Failure>) -> TestResult {
    verdict.map_err(Into::into)
}

/// The verdict is an `Err` that names every needle.
fn rejected(verdict: Result<(), Failure>, needles: &[&str]) -> TestResult {
    let Err(said) = verdict else {
        return Err("the check accepted a fixture it should reject".into());
    };
    match needles.iter().find(|needle| !said.contains(*needle)) {
        Some(missing) => Err(format!("{missing:?} is not named in: {said}").into()),
        None => Ok(()),
    }
}

#[test]
fn a_workflow_that_runs_cargo_and_counts_the_unported_rows_passes() -> TestResult {
    accepted(judge_workflow(WORKFLOW_BASE))
}

#[test]
fn the_workflow_before_r11_has_four_python_lines_and_is_rejected() -> TestResult {
    let verdict = judge_workflow(WORKFLOW_BEFORE_R11);
    rejected(verdict, &["line 14", "line 16", "line 17", "line 18"])
}

#[test]
fn a_workflow_that_keeps_a_pytest_step_is_rejected() -> TestResult {
    let workflow = format!("{WORKFLOW_BASE}      - run: python -m pytest tests -q\n");
    rejected(judge_workflow(&workflow), &["python -m pytest tests -q"])
}

#[test]
fn each_python_word_alone_is_rejected() -> TestResult {
    for added in [
        "- run: pytest -q",
        "- run: pip install x",
        "- run: python3 x.py",
    ] {
        let workflow = format!("{WORKFLOW_BASE}      {added}\n");
        rejected(judge_workflow(&workflow), &[added])?;
    }
    Ok(())
}

#[test]
fn a_word_that_only_starts_with_pip_is_not_python() -> TestResult {
    let workflow = format!("{WORKFLOW_BASE}      shell: bash -o pipefail {{0}}\n");
    accepted(judge_workflow(&workflow))
}

#[test]
fn a_comment_that_names_python_is_rejected_like_any_other_line() -> TestResult {
    let workflow = format!("{WORKFLOW_BASE}# no python here\n");
    rejected(judge_workflow(&workflow), &["# no python here"])
}

#[test]
fn a_workflow_without_the_count_step_is_rejected() -> TestResult {
    let workflow = edit(WORKFLOW_BASE, COUNT_STEP_19, "")?;
    rejected(judge_workflow(&workflow), &["rp19", "--exact"])
}

#[test]
fn a_skip_without_the_count_step_is_rejected() -> TestResult {
    let without = edit(WORKFLOW_BASE, COUNT_STEP_19, "")?;
    let skipped = TEST_STEP_TAIL.replace('\n', " -- --skip rp19_python_cases_are_ported\n");
    let workflow = edit(&without, TEST_STEP_TAIL, &skipped)?;
    rejected(judge_workflow(&workflow), &["rp19", "--exact"])
}

#[test]
fn a_count_step_that_stops_the_job_is_rejected() -> TestResult {
    let stopping = COUNT_STEP_19.replace("        continue-on-error: true\n", "");
    let workflow = edit(WORKFLOW_BASE, COUNT_STEP_19, &stopping)?;
    rejected(judge_workflow(&workflow), &["continue-on-error"])
}

#[test]
fn a_count_step_that_runs_another_test_is_rejected() -> TestResult {
    let other = COUNT_STEP_19.replace(
        "rp19_python_cases_are_ported",
        "rp19_ported_rows_pass_and_the_not_yet_list_only_shrinks",
    );
    let workflow = edit(WORKFLOW_BASE, COUNT_STEP_19, &other)?;
    let needles = ["runs another test", "rp19_python_cases_are_ported"];
    rejected(judge_workflow(&workflow), &needles)
}

#[test]
fn an_excluded_binary_missing_from_the_probe_note_is_named() -> TestResult {
    let note = edit(PROBE_BASE, PROBE_ROW, "")?;
    rejected(judge_probe(&note), &["rp03"])
}

#[test]
fn an_excluded_binary_without_a_reason_is_named() -> TestResult {
    let note = edit(PROBE_BASE, PROBE_ROW, "| `rp03` | |\n")?;
    rejected(judge_probe(&note), &["rp03", "without a reason"])
}

#[test]
fn an_excluded_binary_with_another_reason_is_named() -> TestResult {
    let note = edit(PROBE_BASE, PROBE_ROW, "| `rp03` | too slow for the CI |\n")?;
    rejected(judge_probe(&note), &["rp03", "testdata/ or models/"])
}

#[test]
fn an_excluded_binary_with_a_testdata_or_models_reason_is_accepted() -> TestResult {
    for reason in ["reads `testdata/`", "reads `models/`"] {
        let note = edit(PROBE_BASE, PROBE_ROW, &format!("| `rp03` | {reason} |\n"))?;
        accepted(judge_probe(&note))?;
    }
    Ok(())
}

#[test]
fn an_excluded_crate_hides_no_binary_from_the_probe_note() -> TestResult {
    let by_crate = "cargo test --release --jobs 2 --workspace --exclude hayamimi-eval\n";
    let test_run = "cargo test --release --jobs 2 -p hayamimi-eval --test rp02 --test rp19 --test rp80 --test rp81\n";
    let workflow = edit(WORKFLOW_BASE, test_run, by_crate)?;
    let Err(said) = judge_workflow(&workflow) else {
        return Err("a crate left out as a whole was accepted".into());
    };
    for named in ["rp02", "rp19", "rp80"] {
        if !said.contains(named) {
            return Err(format!("{named} is not named in: {said}").into());
        }
    }
    if said.contains("rp03") {
        return Err(format!("rp03 has a row and must not be named: {said}").into());
    }
    Ok(())
}

#[test]
fn a_count_step_without_its_test_file_is_rejected() -> TestResult {
    let binaries: Vec<Binary> = f80_binaries()
        .into_iter()
        .filter(|candidate| candidate.name != "rp19")
        .collect();
    let verdict = check_workflow(WORKFLOW_BASE, PROBE_BASE, &binaries);
    rejected(verdict, &["rp19", "no test file"])
}

#[test]
fn rp80_ci_runs_cargo_and_no_python() -> TestResult {
    let root = Path::new(REPO);
    let workflow = read(&root.join(".github/workflows/test.yml"))?;
    let note = read(&root.join("docs/results/r11/ci_probe.md"))?;
    let binaries = repo_binaries(&root.join("native"))?;
    accepted(check_workflow(&workflow, &note, &binaries))
}

#[test]
fn a_workflow_without_the_owned_exceptions_step_is_rejected() -> TestResult {
    let workflow = edit(WORKFLOW_BASE, COUNT_STEP_81, "")?;
    rejected(judge_workflow(&workflow), &["rp81", "--exact"])
}

#[test]
fn an_owned_exceptions_step_in_a_wrong_form_is_rejected() -> TestResult {
    let stopping = COUNT_STEP_81.replace("        continue-on-error: true\n", "");
    let without_flag = edit(WORKFLOW_BASE, COUNT_STEP_81, &stopping)?;
    rejected(
        judge_workflow(&without_flag),
        &["rp81", "continue-on-error"],
    )?;
    let other = COUNT_STEP_81.replace(
        "rp81_owned_exceptions_are_gone",
        "rp81_no_document_names_a_removed_python_file",
    );
    let wrong_name = edit(WORKFLOW_BASE, COUNT_STEP_81, &other)?;
    rejected(judge_workflow(&wrong_name), &["rp81"])
}

// ---------------------------------------------------------------------------
// The real run in a clone without `testdata/` and `models/`. Run by hand with
// `-- --ignored`: it builds a clone from scratch.
// ---------------------------------------------------------------------------

/// `~/.cache/hayamimi-r11/ci-probe`. The home directory is the one value read
/// from the environment, and this project gives it no meaning.
fn clone_dir() -> Result<PathBuf, Failure> {
    let Some(home) = std::env::home_dir() else {
        return Err("the home directory is unknown".to_owned());
    };
    Ok(home.join(CLONE_FROM_HOME))
}

fn ensure_clone(clone: &Path) -> Result<(), Failure> {
    if clone.join(".git").exists() {
        return Ok(());
    }
    let status = Command::new("git")
        .args(["clone", "--no-local", "--quiet", REPO])
        .arg(clone)
        .status()
        .map_err(|error| format!("cannot run git: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("git clone into {} failed", clone.display()))
    }
}

/// The variables that earlier steps wrote to the file named by `GITHUB_ENV`.
type StepEnv = Vec<(String, String)>;

fn read_step_env(file: &Path) -> Result<StepEnv, Failure> {
    let text = read(file)?;
    Ok(text
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect())
}

fn run_line(native: &Path, line: &str, env_file: &Path) -> Result<bool, Failure> {
    let step_env = read_step_env(env_file)?;
    Command::new("sh")
        .args(["-c", line])
        .current_dir(native)
        .env("GITHUB_ENV", env_file)
        .envs(step_env)
        .status()
        .map(|status| status.success())
        .map_err(|error| format!("cannot run `{line}`: {error}"))
}

#[test]
#[ignore = "builds a clone from scratch"]
fn the_run_lines_pass_in_a_clone_without_testdata_or_models() -> TestResult {
    let clone = clone_dir()?;
    ensure_clone(&clone)?;
    for name in ["testdata", "models"] {
        if clone.join(name).exists() {
            return Err(format!("the clone holds {name}/").into());
        }
    }
    let workflow = read(&clone.join(".github/workflows/test.yml"))?;
    let env_file = clone.join(".git/github_env");
    std::fs::write(&env_file, "").map_err(|error| format!("cannot reset the env file: {error}"))?;
    for step in parse_steps(&workflow) {
        let Some(line) = step.run else {
            continue;
        };
        // A package install needs the administrator and is not part of a clone's build.
        if line.starts_with("sudo ") || line.starts_with("choco ") {
            continue;
        }
        let passed = run_line(&clone.join("native"), line, &env_file)?;
        if !passed && !step.continue_on_error {
            return Err(format!("`{line}` failed in the clone").into());
        }
    }
    Ok(())
}

#[test]
#[ignore = "needs the clone that the test above makes"]
fn an_excluded_binary_really_fails_in_a_clone_without_testdata_or_models() -> TestResult {
    let clone = clone_dir()?;
    let note = read(&clone.join("docs/results/r11/ci_probe.md"))?;
    let binaries = repo_binaries(&clone.join("native"))?;
    let rows = probe_rows(&note);
    let Some(target) = binaries
        .iter()
        .find(|candidate| rows.contains_key(candidate.name.as_str()))
    else {
        return Err("ci_probe.md lists no binary".into());
    };
    let output = Command::new("cargo")
        .args([
            "test",
            "--release",
            "--jobs",
            "2",
            "-p",
            target.krate.as_str(),
            "--test",
            target.name.as_str(),
        ])
        .current_dir(clone.join("native"))
        .output()?;
    if output.status.success() {
        return Err(format!(
            "`{}` passes in the clone, so nothing is left out",
            target.name
        )
        .into());
    }
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if REASON_WORDS
        .iter()
        .any(|word| said.contains(word.trim_end_matches('/')))
    {
        Ok(())
    } else {
        Err(format!("`{}` failed without naming testdata or models", target.name).into())
    }
}
