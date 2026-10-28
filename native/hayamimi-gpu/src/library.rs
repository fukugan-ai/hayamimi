//! The release of the sherpa-onnx C library that these crates were written against.
//!
//! `sherpa-onnx` and `sherpa-onnx-sys` are pinned to `=2.0.1`, and the structs and functions they declare
//! follow the layout of that release: `SherpaOnnxSpeechSegment.start` is `int64_t` in 2.0.1 and `int32_t`
//! in 1.13.8, so the pointer after it sits at another offset. A C library of another release, left in
//! `SHERPA_ONNX_LIB_DIR` or found through a stale `LD_LIBRARY_PATH` or `PATH`, reads those structs with other
//! widths and could return a wrong value or follow a wrong pointer. Every place that creates a sherpa-onnx
//! object calls [`require`] first; it reads the version string of the library that was loaded, once per
//! process, and refuses any release but [`EXPECTED`].

use std::sync::OnceLock;

use crate::error::GpuError;

/// The release of the C library: the `=` pin of `sherpa-onnx` and `sherpa-onnx-sys` in every `Cargo.toml`.
pub const EXPECTED: &str = "2.0.1";

/// `Ok` when the loaded C library is release [`EXPECTED`].
pub fn require() -> Result<(), GpuError> {
    static JUDGED: OnceLock<Result<(), String>> = OnceLock::new();
    JUDGED
        .get_or_init(|| judge(sherpa_onnx::version()))
        .clone()
        .map_err(|found| GpuError::Library {
            found,
            expected: EXPECTED,
        })
}

/// What the library says it is, against [`EXPECTED`]; the `Err` holds the release the library gave.
pub fn judge(found: &str) -> Result<(), String> {
    if found == EXPECTED {
        Ok(())
    } else {
        Err(found.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::*;

    /// The calls that make a sherpa-onnx object.
    const CREATORS: [&str; 8] = [
        "OfflineRecognizer::create(",
        "OnlineRecognizer::create(",
        "VoiceActivityDetector::create(",
        "SpokenLanguageIdentification::create(",
        "SpeakerEmbeddingExtractor::create(",
        "OfflineSpeakerDiarization::create(",
        "CircularBuffer::new(",
        "LinearResampler::create(",
    ];

    /// The calls that ask for the release: `require` itself, and the helper of `hayamimi-eval`.
    const CHECKS: [&str; 2] = ["library::require()?", "library_ok()?"];

    fn is_fn_head(line: &str) -> bool {
        let line = line.trim_start();
        ["fn ", "pub fn ", "pub(crate) fn ", "pub(super) fn "]
            .iter()
            .any(|head| line.starts_with(head))
    }

    /// How many creations `source` holds before its test module, and the head line of every function whose
    /// first creation has no check ahead of it.
    fn creations_and_unchecked(source: &str) -> (usize, Vec<String>) {
        let source = source.replace("\r\n", "\n");
        let body = match source.split_once("#[cfg(test)]\nmod tests") {
            Some((before, _)) => before,
            None => source.as_str(),
        };
        let mut chunks: Vec<(String, String)> = Vec::new();
        for line in body
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
        {
            if is_fn_head(line) || chunks.is_empty() {
                chunks.push((line.trim().to_owned(), String::new()));
            }
            if let Some((_, text)) = chunks.last_mut() {
                text.push_str(line);
                text.push('\n');
            }
        }
        let mut seen = 0;
        let mut unchecked = Vec::new();
        for (head, text) in chunks {
            let Some(creation) = CREATORS.iter().filter_map(|call| text.find(call)).min() else {
                continue;
            };
            seen += CREATORS
                .iter()
                .map(|call| text.matches(call).count())
                .sum::<usize>();
            let check = CHECKS.iter().filter_map(|call| text.find(call)).min();
            if check.is_none_or(|at| at > creation) {
                unchecked.push(head);
            }
        }
        (seen, unchecked)
    }

    /// The `native` directory of the workspace.
    fn native() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
    }

    /// A source of a crate: `<crate>/src/...`, with no `tests` or `target` directory on the way. `rel` is the
    /// path under `native`; its components are read as they are, so the separator of the system does not matter.
    fn is_crate_source(rel: &Path) -> bool {
        let parts: Vec<String> = rel
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect();
        parts.get(1).is_some_and(|dir| dir == "src")
            && !parts.iter().any(|part| part == "tests" || part == "target")
    }

    fn rust_sources(dir: &Path, found: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                rust_sources(&path, found);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                found.push(path);
            }
        }
    }

    #[test]
    fn the_release_the_library_names_is_accepted() {
        assert_eq!(judge("2.0.1"), Ok(()));
        assert_eq!(judge(EXPECTED), Ok(()));
    }

    #[test]
    fn another_release_is_refused_with_the_version_it_gave() {
        for other in ["1.13.8", "2.0.0", "2.0.10", "2.0.1\n", "v2.0.1", "2.0", ""] {
            assert_eq!(judge(other), Err(other.to_owned()), "{other:?}");
        }
    }

    #[test]
    fn the_linked_library_is_the_expected_release() {
        let said = require().err().map(|why| why.to_string());
        assert_eq!(
            said, None,
            "the library this test loaded is not release {EXPECTED}"
        );
    }

    #[test]
    fn the_expected_release_is_the_pin_of_every_crate() {
        let pinned = format!("version = \"={EXPECTED}\"");
        let mut pins = 0;
        for entry in fs::read_dir(native()).into_iter().flatten().flatten() {
            let manifest = entry.path().join("Cargo.toml");
            let Ok(text) = fs::read_to_string(&manifest) else {
                continue;
            };
            for line in text.lines().filter(|line| line.starts_with("sherpa-onnx")) {
                assert!(line.contains(&pinned), "{}: {line}", manifest.display());
                pins += 1;
            }
        }
        assert!(
            pins >= 4,
            "found {pins} pins of sherpa-onnx; the scan has broken"
        );
    }

    #[test]
    fn the_scan_wants_a_check_ahead_of_the_first_creation_of_each_function() {
        let bare = "fn a() { VoiceActivityDetector::create(&c, 1.0) }\n";
        let checked = "fn a() { library::require()?; VoiceActivityDetector::create(&c, 1.0) }\n";
        let helper = "fn a() { library_ok()?; OfflineRecognizer::create(&c) }\n";
        let too_late = "fn a() { OfflineRecognizer::create(&c); library_ok()?; }\n";
        let two = "fn a() { library_ok()?; LinearResampler::create(1, 2) }\nfn b() { CircularBuffer::new(1) }\n";
        let in_tests =
            "fn a() {}\n#[cfg(test)]\nmod tests {\n fn t() { OfflineRecognizer::create(&c); }\n}\n";
        let in_comment = "fn a() {\n    // OfflineRecognizer::create(&c)\n}\n";
        assert_eq!(creations_and_unchecked(bare).1.len(), 1);
        assert_eq!(creations_and_unchecked(checked), (1, vec![]));
        assert_eq!(creations_and_unchecked(helper), (1, vec![]));
        assert_eq!(creations_and_unchecked(too_late).1.len(), 1);
        assert_eq!(
            creations_and_unchecked(two),
            (2, vec![String::from("fn b() { CircularBuffer::new(1) }")])
        );
        assert_eq!(creations_and_unchecked(in_tests), (0, vec![]));
        assert_eq!(creations_and_unchecked(in_comment), (0, vec![]));
    }

    #[test]
    fn only_the_sources_of_a_crate_are_scanned() {
        for counted in ["hayamimi-asr/src/vad.rs", "hayamimi-eval/src/route/pair.rs"] {
            assert!(is_crate_source(Path::new(counted)), "{counted}");
        }
        for skipped in [
            "hayamimi-asr/tests/br04.rs",
            "hayamimi-asr/src/route/tests/mod.rs",
            "hayamimi-asr/examples/probe.rs",
            "target/debug/build/x/src/lib.rs",
            "xtask/src/target/x.rs",
            "vad.rs",
        ] {
            assert!(!is_crate_source(Path::new(skipped)), "{skipped}");
        }
    }

    #[test]
    fn every_function_that_creates_a_sherpa_object_asks_first() {
        let mut files = Vec::new();
        rust_sources(&native(), &mut files);
        files.retain(|path| path.strip_prefix(native()).is_ok_and(is_crate_source));
        let mut total = 0;
        let mut bare = Vec::new();
        for path in &files {
            let Ok(source) = fs::read_to_string(path) else {
                continue;
            };
            let (seen, unchecked) = creations_and_unchecked(&source);
            total += seen;
            bare.extend(
                unchecked
                    .into_iter()
                    .map(|head| format!("{}: {head}", path.display())),
            );
        }
        assert!(
            total >= 15,
            "the scan found {total} creations in {} files; it has broken",
            files.len()
        );
        assert!(
            bare.is_empty(),
            "functions that create with no check ahead:\n{}",
            bare.join("\n")
        );
    }
}
