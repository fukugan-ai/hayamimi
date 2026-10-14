//! RP-27: the string stages that have no GPU path (ITN and the punctuation
//! rules) do not call ONNX Runtime or CUDA. The check is a search run in a
//! child process; a search that failed is never read as "no hits".

mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use support::{Failure, REPO, TARGET, TestResult};

/// The files that must stay free of both words, relative to the repository.
const STRING_STAGES: [&str; 2] = [
    "native/hayamimi-asr/src/itn.rs",
    "native/hayamimi-asr/src/punct/rules.rs",
];
/// The words: the runtime as a whole word, and CUDA anywhere, any case.
const PATTERN: &str = r"\bort\b|cuda";

/// What the search answered: its exit code and the files that matched.
#[derive(Debug)]
struct Count {
    /// 0: hits, 1: none, 2: the search itself failed.
    code: i32,
    hits: Vec<(String, usize)>,
    stderr: String,
}

/// Runs `rg -c -i` over `files` in a child process.
fn count_gpu_imports(files: &[PathBuf]) -> Result<Count, Failure> {
    let output = Command::new("rg")
        .args(["--no-config", "--with-filename", "-c", "-i", PATTERN])
        .args(files)
        .output()
        .map_err(|error| format!("cannot run rg: {error}"))?;
    let code = output
        .status
        .code()
        .ok_or_else(|| String::from("rg was ended by a signal"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let hits = stdout
        .lines()
        .filter_map(|line| line.rsplit_once(':'))
        .filter_map(|(file, count)| Some((file.to_owned(), count.parse::<usize>().ok()?)))
        .collect();
    Ok(Count {
        code,
        hits,
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// Passes only when the search ran and found nothing. The error names the
/// files that matched, or says that the search failed.
fn check_string_stages(files: &[PathBuf]) -> Result<(), Failure> {
    let count = count_gpu_imports(files)?;
    match count.code {
        1 => Ok(()),
        0 => {
            let names: Vec<&str> = count.hits.iter().map(|(file, _)| file.as_str()).collect();
            Err(format!("ORT or CUDA appears in: {}", names.join(", ")))
        }
        2 => Err(format!("the search failed: {}", count.stderr.trim())),
        other => Err(format!("the search ended with code {other}")),
    }
}

/// A directory made for one test under the workspace's `target`, removed when dropped.
struct Tree {
    root: PathBuf,
}

impl Tree {
    fn new(name: &str) -> Result<Tree, Failure> {
        let root = Path::new(TARGET)
            .join("rp27")
            .join(format!("{name}-{}", std::process::id()));
        fs::create_dir_all(root.join("native/hayamimi-asr/src/punct"))
            .map_err(|error| format!("cannot make {}: {error}", root.display()))?;
        Ok(Tree { root })
    }

    fn write(&self, relative: &str, text: &str) -> Result<(), Failure> {
        let path = self.root.join(relative);
        fs::write(&path, text).map_err(|error| format!("cannot write {}: {error}", path.display()))
    }

    /// The two string-stage files with clean contents.
    fn clean(name: &str) -> Result<Tree, Failure> {
        let tree = Tree::new(name)?;
        tree.write(STRING_STAGES[0], "pub fn convert() {}\n")?;
        tree.write(STRING_STAGES[1], "pub fn punctuate() {}\n")?;
        Ok(tree)
    }

    fn files(&self) -> Vec<PathBuf> {
        STRING_STAGES
            .iter()
            .map(|file| self.root.join(file))
            .collect()
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        // A tree that cannot be removed is only left behind under `target`.
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn repo_files() -> Vec<PathBuf> {
    STRING_STAGES
        .iter()
        .map(|file| Path::new(REPO).join(file))
        .collect()
}

/// T-RP-27-001
#[test]
fn clean_string_stage_files_have_no_hits() -> TestResult {
    let tree = Tree::clean("clean")?;
    let count = count_gpu_imports(&tree.files())?;
    assert_eq!(count.code, 1);
    assert!(count.hits.is_empty());
    Ok(())
}

/// T-RP-27-002
#[test]
fn ort_import_in_itn_is_named() -> TestResult {
    let tree = Tree::clean("ort-in-itn")?;
    tree.write(STRING_STAGES[0], "use ort::session::Session;\n")?;
    let count = count_gpu_imports(&tree.files())?;
    assert_eq!(count.code, 0);
    assert_eq!(count.hits.len(), 1);
    let said = check_string_stages(&tree.files());
    assert!(
        said.as_ref().is_err_and(|text| text.contains("itn.rs")),
        "{said:?}"
    );
    Ok(())
}

/// T-RP-27-003
#[test]
fn uppercase_cuda_in_punct_rules_is_named() -> TestResult {
    let tree = Tree::clean("cuda-in-rules")?;
    tree.write(STRING_STAGES[1], "// runs on CUDA\n")?;
    let count = count_gpu_imports(&tree.files())?;
    assert_eq!(count.code, 0);
    assert_eq!(count.hits.len(), 1);
    let said = check_string_stages(&tree.files());
    assert!(
        said.as_ref().is_err_and(|text| text.contains("rules.rs")),
        "{said:?}"
    );
    Ok(())
}

/// T-RP-27-004
#[test]
fn missing_string_stage_file_is_an_error_not_zero_hits() -> TestResult {
    let tree = Tree::new("missing-rules")?;
    tree.write(STRING_STAGES[0], "pub fn convert() {}\n")?;
    let count = count_gpu_imports(&tree.files())?;
    assert_eq!(count.code, 2);
    assert!(check_string_stages(&tree.files()).is_err());
    Ok(())
}

/// T-RP-27-005: the same expression finds the words where they are.
#[test]
fn import_counter_finds_ort_and_cuda_in_the_gpu_crate() -> TestResult {
    let gpu = Path::new(REPO).join("native/hayamimi-gpu/src");
    let count = count_gpu_imports(&[gpu])?;
    assert_eq!(count.code, 0);
    assert!(!count.hits.is_empty());
    Ok(())
}

/// T-RP-27-006
#[test]
fn string_stage_modules_import_neither_ort_nor_cuda() -> TestResult {
    for file in repo_files() {
        assert!(file.is_file(), "{} does not exist", file.display());
    }
    let count = count_gpu_imports(&repo_files())?;
    assert_eq!(count.code, 1, "{count:?}");
    assert!(count.hits.is_empty());
    Ok(())
}

/// The acceptance test of RP-27: the two modules exist and are clean, and the
/// same search finds the words in the GPU crate.
#[test]
fn rp27_string_stages_do_not_import_ort_or_cuda() -> TestResult {
    check_string_stages(&repo_files())?;
    let gpu = Path::new(REPO).join("native/hayamimi-gpu/src");
    assert_eq!(count_gpu_imports(&[gpu])?.code, 0);
    Ok(())
}
