//! RB-1: the scoring and evaluation packages are installed. The input is
//! the saved output of `pip show` for those packages.

mod support;

use std::path::Path;

use support::{FIXTURES, Failure, TestResult, base, read};

const EXPECTED: [(&str, &str); 5] = [
    ("sacrebleu", "2.6.0"),
    ("jiwer", "4.0.0"),
    ("simpleder", "0.0.5"),
    ("OpenCC", "1.4.2"),
    ("pyarrow", "25.0.1"),
];

/// pip's normalised project name: lower case, with `_` and `.` read as `-`.
fn normal(name: &str) -> String {
    name.trim().to_ascii_lowercase().replace(['_', '.'], "-")
}

/// The (name, version) pair of each block of a `pip show` output.
fn packages(text: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut name: Option<String> = None;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("Name:") {
            name = Some(normal(value));
        } else if let Some(value) = line.strip_prefix("Version:")
            && let Some(owner) = name.take()
        {
            found.push((owner, value.trim().to_owned()));
        }
    }
    found
}

fn version_of(text: &str, package: &str) -> Option<String> {
    let wanted = normal(package);
    packages(text)
        .into_iter()
        .find(|(name, _)| *name == wanted)
        .map(|(_, version)| version)
}

/// Every expected package is present with exactly the expected version.
/// The error names each package that is missing or has another version.
fn check_pip_show_versions(text: &str) -> Result<(), Failure> {
    if text.trim().is_empty() {
        return Err(String::from("pip show output is empty"));
    }
    let mut wrong = Vec::new();
    for (name, wanted) in EXPECTED {
        match version_of(text, name) {
            None => wrong.push(format!("{name} is missing")),
            Some(found) if found != wanted => {
                wrong.push(format!("{name} is {found}, want {wanted}"));
            }
            Some(_) => {}
        }
    }
    if wrong.is_empty() {
        Ok(())
    } else {
        Err(wrong.join("; "))
    }
}

fn pip_show() -> Result<String, Failure> {
    read(&base()?.join("pip_show.txt"))
}

#[test]
fn rb01_scoring_and_eval_packages_are_installed_all_five_versions_match() -> TestResult {
    check_pip_show_versions(&pip_show()?)?;
    Ok(())
}

#[test]
fn rb01_scoring_and_eval_packages_are_installed_sherpa_onnx_control_reads_1_13_6() -> TestResult {
    let found = version_of(&pip_show()?, "sherpa-onnx");
    assert_eq!(found.as_deref(), Some("1.13.6"));
    Ok(())
}

#[test]
fn rb01_scoring_and_eval_packages_are_installed_one_wrong_version_names_only_that_package()
-> TestResult {
    let wrong = check_pip_show_versions(include_str!("fixtures/rb01_sacrebleu_2_5_9.txt"));
    assert!(matches!(&wrong, Err(said) if said.contains("sacrebleu")
        && !said.contains("jiwer")
        && !said.contains("simpleder")
        && !said.contains("OpenCC")
        && !said.contains("pyarrow")));
    Ok(())
}

#[test]
fn rb01_scoring_and_eval_packages_are_installed_version_is_compared_whole_not_by_prefix()
-> TestResult {
    let longer = check_pip_show_versions(include_str!("fixtures/rb01_sacrebleu_2_6_01.txt"));
    assert!(matches!(&longer, Err(said) if said.contains("sacrebleu")));
    Ok(())
}

#[test]
fn rb01_scoring_and_eval_packages_are_installed_shorter_version_is_rejected() -> TestResult {
    let shorter = check_pip_show_versions(include_str!("fixtures/rb01_sacrebleu_2_6.txt"));
    assert!(matches!(&shorter, Err(said) if said.contains("sacrebleu")));
    Ok(())
}

#[test]
fn rb01_scoring_and_eval_packages_are_installed_venv_before_install_names_all_five_as_missing()
-> TestResult {
    let before = check_pip_show_versions(include_str!("fixtures/rb01_not_found.txt"));
    assert!(matches!(&before, Err(said) if EXPECTED
        .iter()
        .all(|(name, _)| said.contains(&format!("{name} is missing")))));
    Ok(())
}

#[test]
fn rb01_scoring_and_eval_packages_are_installed_empty_pip_show_is_an_error() -> TestResult {
    let empty = check_pip_show_versions(include_str!("fixtures/rb01_empty.txt"));
    assert!(empty.is_err());
    Ok(())
}

#[test]
fn rb01_scoring_and_eval_packages_are_installed_package_name_match_ignores_case() -> TestResult {
    // All five versions are right; only `OpenCC` is written in lower case.
    let text = include_str!("fixtures/rb01_lowercase_name.txt");
    check_pip_show_versions(text)?;
    Ok(())
}

#[test]
fn rb01_scoring_and_eval_packages_are_installed_missing_pip_show_file_names_its_path() {
    let absent = Path::new(FIXTURES).join("absent").join("pip_show.txt");
    let said = read(&absent);
    assert!(matches!(&said, Err(path) if path.ends_with("pip_show.txt")));
}
