//! The model groups `hayamimi models fetch` knows, the files each one needs and
//! the places each one can be fetched from, in the order they are tried.
//!
//! A group is the files one stage reads. Which precision is fetched follows the
//! stage table of the design: the recognizers are CPU stages and read int8
//! files, the stages on the GPU read fp32 files (CUDA has no op for the int8
//! ones), and the stages with no model of their own have none.

use std::path::{Path, PathBuf};

use super::error::FetchError;
use super::options::Options;

/// Where a group can be fetched from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// A `.tar.bz2` of the sherpa-onnx release `tag`; only the group's files are kept.
    Tar {
        tag: &'static str,
        file: &'static str,
    },
    /// One file of the sherpa-onnx release `tag`, kept under the group's single file name.
    Asset {
        tag: &'static str,
        file: &'static str,
    },
    /// The files of a Hugging Face repository, under the names the group uses.
    Hf { repo: &'static str },
}

impl Source {
    /// The token the log event carries as `source`.
    pub fn name(self) -> &'static str {
        match self {
            Source::Tar { .. } | Source::Asset { .. } => "release",
            Source::Hf { .. } => "huggingface",
        }
    }
}

/// The files a group consists of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Files {
    /// These files, by name inside the group's directory.
    Named(&'static [&'static str]),
    /// Every file of a repository; the group is there once `marker` is.
    Snapshot { marker: &'static str },
}

/// One group of model files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Item {
    /// The stage that reads it, as the manifest and the log name it.
    pub stage: &'static str,
    /// Its directory under the models directory; empty for files at the top.
    pub dir: &'static str,
    pub files: Files,
    pub sources: &'static [Source],
}

const ASR: &str = "asr-models";
const SPEAKER: &str = "speaker-recongition-models";
const SEGMENTATION: &str = "speaker-segmentation-models";

const RECOGNIZER_DIR: &str = "sherpa-onnx-zipformer-ja-en-reazonspeech-2025-01-17";
const LANGUAGE_ID_DIR: &str = "sherpa-onnx-whisper-tiny";
const PUNCTUATION_DIR: &str = "mojicast-punct-onnx";

/// The core of Japanese and English: what `--minimal` fetches.
pub static MINIMAL: [Item; 4] = [
    Item {
        stage: "recognizer",
        dir: RECOGNIZER_DIR,
        files: Files::Named(&[
            "encoder-epoch-35-avg-1.int8.onnx",
            "decoder-epoch-35-avg-1.int8.onnx",
            "joiner-epoch-35-avg-1.int8.onnx",
            "tokens.txt",
        ]),
        sources: &[
            Source::Tar {
                tag: ASR,
                file: "sherpa-onnx-zipformer-ja-en-reazonspeech-2025-01-17.tar.bz2",
            },
            Source::Hf {
                repo: "csukuangfj/reazonspeech-k2-v2-ja-en",
            },
        ],
    },
    Item {
        stage: "language_id",
        dir: LANGUAGE_ID_DIR,
        files: Files::Named(&["tiny-encoder.onnx", "tiny-decoder.onnx", "tiny-tokens.txt"]),
        sources: &[
            Source::Tar {
                tag: ASR,
                file: "sherpa-onnx-whisper-tiny.tar.bz2",
            },
            Source::Hf {
                repo: "csukuangfj/sherpa-onnx-whisper-tiny",
            },
        ],
    },
    Item {
        stage: "vad",
        dir: "",
        files: Files::Named(&["silero_vad.onnx"]),
        sources: &[
            Source::Asset {
                tag: ASR,
                file: "silero_vad.onnx",
            },
            Source::Hf {
                repo: "csukuangfj/vad",
            },
        ],
    },
    Item {
        stage: "punctuation",
        dir: PUNCTUATION_DIR,
        files: Files::Named(&["punct_bert.onnx", "vocab.txt"]),
        sources: &[Source::Hf {
            repo: "ishiki-emo/mojicast-punct-onnx",
        }],
    },
];

/// What the default run fetches besides the core: the other languages' recognizers and the speakers.
pub static LANGUAGES: [Item; 6] = [
    Item {
        stage: "recognizer_zh",
        dir: "sherpa-onnx-paraformer-zh-int8-2025-10-07",
        files: Files::Named(&["model.int8.onnx", "tokens.txt"]),
        sources: &[Source::Tar {
            tag: ASR,
            file: "sherpa-onnx-paraformer-zh-int8-2025-10-07.tar.bz2",
        }],
    },
    Item {
        stage: "recognizer_ko_yue",
        dir: "sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2024-07-17",
        files: Files::Named(&["model.int8.onnx", "tokens.txt"]),
        sources: &[Source::Tar {
            tag: ASR,
            file: "sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2024-07-17.tar.bz2",
        }],
    },
    Item {
        stage: "recognizer_en",
        dir: "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8",
        files: Files::Named(&[
            "encoder.int8.onnx",
            "decoder.int8.onnx",
            "joiner.int8.onnx",
            "tokens.txt",
        ]),
        sources: &[Source::Tar {
            tag: ASR,
            file: "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8.tar.bz2",
        }],
    },
    Item {
        stage: "recognizer_any",
        dir: "omnilingual-300m-ctc-int8",
        files: Files::Named(&["model.int8.onnx", "tokens.txt"]),
        sources: &[Source::Tar {
            tag: ASR,
            file: "sherpa-onnx-omnilingual-asr-1600-languages-300M-ctc-int8-2025-11-12.tar.bz2",
        }],
    },
    Item {
        stage: "speaker_embedding",
        dir: "",
        files: Files::Named(&["campplus_sv.onnx"]),
        sources: &[Source::Asset {
            tag: SPEAKER,
            file: "3dspeaker_speech_campplus_sv_zh_en_16k-common_advanced.onnx",
        }],
    },
    Item {
        stage: "speaker_segmentation",
        dir: "sherpa-onnx-pyannote-segmentation-3-0",
        files: Files::Named(&["model.onnx"]),
        sources: &[Source::Tar {
            tag: SEGMENTATION,
            file: "sherpa-onnx-pyannote-segmentation-3-0.tar.bz2",
        }],
    },
];

/// The English-only recognizer that `--en-parakeet-v2` adds.
pub static PARAKEET_V2: Item = Item {
    stage: "recognizer_en_v2",
    dir: "sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8",
    files: Files::Named(&[
        "encoder.int8.onnx",
        "decoder.int8.onnx",
        "joiner.int8.onnx",
        "tokens.txt",
    ]),
    sources: &[Source::Tar {
        tag: ASR,
        file: "sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8.tar.bz2",
    }],
};

/// The four-class punctuation model that `--punct-4class` adds: a whole repository.
pub static PUNCT_4CLASS: Item = Item {
    stage: "punctuation_4class",
    dir: "punct-ja-4class-permissive",
    files: Files::Snapshot {
        marker: "punct_4class.onnx",
    },
    sources: &[Source::Hf {
        repo: "oboroge0/hayamimi-punct-ja-4class",
    }],
};

/// The groups a run fetches, in the order they are fetched: the core, the
/// other languages unless `--minimal`, and the opt-in groups.
pub fn selected(options: &Options) -> Vec<&'static Item> {
    let mut chosen: Vec<&'static Item> = MINIMAL.iter().collect();
    if !options.minimal {
        chosen.extend(LANGUAGES.iter());
    }
    if options.en_parakeet_v2 {
        chosen.push(&PARAKEET_V2);
    }
    if options.punct_4class {
        chosen.push(&PUNCT_4CLASS);
    }
    chosen
}

/// Every file under `dir` as `prefix` plus its path, with `/`.
fn walk(dir: &Path, prefix: &str, found: &mut Vec<String>) -> Result<(), FetchError> {
    for entry in std::fs::read_dir(dir).map_err(|e| FetchError::io(dir, e))? {
        let entry = entry.map_err(|e| FetchError::io(dir, e))?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let kind = entry.file_type().map_err(|e| FetchError::io(dir, e))?;
        if kind.is_dir() {
            walk(&entry.path(), &format!("{prefix}{name}/"), found)?;
        } else if kind.is_file() {
            found.push(format!("{prefix}{name}"));
        }
    }
    Ok(())
}

impl Item {
    /// The directory the group's files are installed into.
    pub fn dest(&self, models: &Path) -> PathBuf {
        if self.dir.is_empty() {
            models.to_path_buf()
        } else {
            models.join(self.dir)
        }
    }

    /// The names whose presence says the group is there.
    pub fn marker_names(&self) -> &'static [&'static str] {
        match self.files {
            Files::Named(names) => names,
            Files::Snapshot { .. } => &[],
        }
    }

    /// The path of `name` inside the group, relative to the models directory, with `/`.
    pub fn relative(&self, name: &str) -> String {
        if self.dir.is_empty() {
            name.to_owned()
        } else {
            format!("{}/{name}", self.dir)
        }
    }

    /// The names of the group's files that are in its directory now, with `/`.
    pub fn names_on_disk(&self, models: &Path) -> Result<Vec<String>, FetchError> {
        match self.files {
            Files::Named(names) => Ok(names.iter().map(|name| (*name).to_owned()).collect()),
            Files::Snapshot { .. } => {
                let mut found = Vec::new();
                walk(&self.dest(models), "", &mut found)?;
                found.sort();
                Ok(found)
            }
        }
    }

    /// Whether any file of the group is there, complete or not.
    pub fn has_any_file(&self, models: &Path) -> bool {
        let dest = self.dest(models);
        match self.files {
            Files::Named(names) => names.iter().any(|name| dest.join(name).is_file()),
            Files::Snapshot { marker } => dest.join(marker).is_file(),
        }
    }

    /// Whether the group is already there: every named file, or the marker of a snapshot.
    pub fn is_present(&self, models: &Path) -> bool {
        let dest = self.dest(models);
        let there = |name: &str| {
            std::fs::metadata(dest.join(name)).is_ok_and(|meta| meta.is_file() && meta.len() > 0)
        };
        match self.files {
            Files::Named(names) => names.iter().all(|name| there(name)),
            Files::Snapshot { marker } => there(marker),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models_fetch::options::Options;

    fn options(minimal: bool, v2: bool, class4: bool) -> Options {
        Options {
            minimal,
            en_parakeet_v2: v2,
            punct_4class: class4,
            ..Options::new()
        }
    }

    fn stages(chosen: &[&Item]) -> Vec<&'static str> {
        chosen.iter().map(|item| item.stage).collect()
    }

    #[test]
    fn minimal_is_the_four_groups_of_the_core() {
        let chosen = selected(&options(true, false, false));
        assert_eq!(
            stages(&chosen),
            ["recognizer", "language_id", "vad", "punctuation"]
        );
    }

    #[test]
    fn the_default_adds_the_other_languages_and_the_speakers() {
        let chosen = selected(&options(false, false, false));
        assert_eq!(chosen.len(), MINIMAL.len() + LANGUAGES.len());
        assert!(stages(&chosen).contains(&"recognizer_zh"));
        assert!(stages(&chosen).contains(&"speaker_segmentation"));
    }

    #[test]
    fn an_opt_in_is_added_after_the_rest() {
        let chosen = selected(&options(true, true, true));
        assert_eq!(
            stages(&chosen)[4..],
            ["recognizer_en_v2", "punctuation_4class"]
        );
    }

    #[test]
    fn every_group_has_a_stage_a_source_and_files_no_name_repeats() {
        let all = MINIMAL
            .iter()
            .chain(LANGUAGES.iter())
            .chain([&PARAKEET_V2, &PUNCT_4CLASS]);
        let mut stage_names = Vec::new();
        for item in all {
            assert!(!item.sources.is_empty(), "{}", item.stage);
            assert!(!stage_names.contains(&item.stage), "{}", item.stage);
            stage_names.push(item.stage);
            if let Files::Named(names) = item.files {
                assert!(!names.is_empty(), "{}", item.stage);
                for (at, name) in names.iter().enumerate() {
                    assert!(!names[..at].contains(name), "{}: {name}", item.stage);
                }
            }
        }
    }

    #[test]
    fn a_snapshot_is_only_ever_fetched_from_a_repository() {
        assert!(matches!(PUNCT_4CLASS.files, Files::Snapshot { .. }));
        assert!(
            PUNCT_4CLASS
                .sources
                .iter()
                .all(|source| matches!(source, Source::Hf { .. }))
        );
    }

    #[test]
    fn an_asset_group_keeps_one_file_and_a_tar_group_names_the_files_it_keeps() {
        for item in MINIMAL.iter().chain(LANGUAGES.iter()) {
            let Files::Named(names) = item.files else {
                continue;
            };
            for source in item.sources {
                if matches!(source, Source::Asset { .. }) {
                    assert_eq!(names.len(), 1, "{}", item.stage);
                }
            }
        }
    }

    #[test]
    fn the_core_reads_int8_for_the_recognizer_and_never_for_a_gpu_stage() {
        let core = |stage: &str| {
            MINIMAL
                .iter()
                .find(|item| item.stage == stage)
                .map(Item::marker_names)
        };
        let int8_count = |names: &[&str]| {
            names
                .iter()
                .filter(|name| name.ends_with(".int8.onnx"))
                .count()
        };
        assert_eq!(core("recognizer").map(int8_count), Some(3));
        for stage in ["language_id", "punctuation"] {
            assert_eq!(core(stage).map(int8_count), Some(0), "{stage}");
        }
    }

    #[test]
    fn a_group_is_present_only_when_every_file_is_there_and_not_empty() -> std::io::Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../target/unit-tmp")
            .join(format!("items-present-{}", std::process::id()));
        let item = &MINIMAL[1];
        let dest = item.dest(&root);
        std::fs::create_dir_all(&dest)?;
        assert!(!item.is_present(&root));
        for name in item.marker_names() {
            std::fs::write(dest.join(name), b"x")?;
        }
        assert!(item.is_present(&root));
        std::fs::write(dest.join("tiny-tokens.txt"), b"")?;
        assert!(!item.is_present(&root));
        std::fs::remove_dir_all(&root)
    }

    #[test]
    fn a_relative_name_holds_the_directory_only_when_there_is_one() {
        assert_eq!(MINIMAL[2].relative("silero_vad.onnx"), "silero_vad.onnx");
        assert_eq!(
            MINIMAL[1].relative("tiny-tokens.txt"),
            "sherpa-onnx-whisper-tiny/tiny-tokens.txt"
        );
    }
}
