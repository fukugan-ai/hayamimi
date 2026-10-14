//! Where each recognizer tier of the catalogue (`route::Tier`) lives on disk,
//! which of its files say that it is installed, and which tier keeps its
//! place in memory whatever the cap.

use std::path::{Path, PathBuf};

use crate::error::AsrError;
use crate::route::{Tier, fallback_chain};

impl Tier {
    /// The tier a name stands for.
    pub fn from_name(name: &str) -> Option<Tier> {
        Tier::ALL.into_iter().find(|tier| tier.name() == name)
    }

    /// The model directory under the models directory.
    pub fn directory(self) -> &'static str {
        match self {
            Tier::Rz => "sherpa-onnx-zipformer-ja-en-reazonspeech-2025-01-17",
            Tier::Pz => "sherpa-onnx-paraformer-zh-int8-2025-10-07",
            Tier::Sv => "sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2024-07-17",
            Tier::V3 => "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8",
            Tier::V2 => "sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8",
            Tier::Omni => "omnilingual-300m-ctc-int8",
            Tier::Pja => "sherpa-onnx-nemo-parakeet-tdt_ctc-0.6b-ja-35000-int8",
        }
    }

    /// The file name prefix of the model file that says the tier is installed
    /// (the same prefixes the Python engine globs with).
    pub fn key_prefix(self) -> &'static str {
        match self {
            Tier::Rz | Tier::V3 | Tier::V2 => "encoder",
            Tier::Pz | Tier::Sv | Tier::Omni | Tier::Pja => "model",
        }
    }

    /// Whether the tier keeps its place however many others are resident.
    pub fn is_always_resident(self) -> bool {
        self == Tier::Rz
    }

    /// The tiers to try for a request of this tier, in order, each once.
    pub fn chain(self) -> Vec<Tier> {
        fallback_chain(self)
    }

    /// The directory of the tier under `models`.
    pub fn path(self, models: &Path) -> PathBuf {
        models.join(self.directory())
    }
}

/// The first file of `directory` whose name starts with `prefix` and ends
/// with `.onnx`, in name order: the lookup the Python engine does with a glob.
pub fn find_model(directory: &Path, prefix: &str) -> Result<PathBuf, AsrError> {
    let missing = || AsrError::MissingModel(directory.join(format!("{prefix}*.onnx")));
    let entries = std::fs::read_dir(directory).map_err(|_| missing())?;
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.starts_with(prefix) && name.ends_with(".onnx"))
        .collect();
    names.sort();
    let Some(first) = names.first() else {
        return Err(missing());
    };
    Ok(directory.join(first))
}

/// Whether the key model file of the tier is under `models`.
pub fn is_installed(tier: Tier, models: &Path) -> bool {
    find_model(&tier.path(models), tier.key_prefix()).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(tiers: &[Tier]) -> Vec<&'static str> {
        tiers.iter().map(|tier| tier.name()).collect()
    }

    #[test]
    fn every_tier_has_its_own_name_and_directory() {
        let mut seen_names = Vec::new();
        let mut seen_directories = Vec::new();
        for tier in Tier::ALL {
            assert!(!seen_names.contains(&tier.name()));
            assert!(!seen_directories.contains(&tier.directory()));
            seen_names.push(tier.name());
            seen_directories.push(tier.directory());
            assert_eq!(Tier::from_name(tier.name()), Some(tier));
        }
        assert_eq!(Tier::from_name("no-such-tier"), None);
    }

    #[test]
    fn only_the_japanese_tier_is_always_resident() {
        let always: Vec<Tier> = Tier::ALL
            .into_iter()
            .filter(|tier| tier.is_always_resident())
            .collect();
        assert_eq!(always, [Tier::Rz]);
    }

    #[test]
    fn a_missing_tier_gives_way_in_the_order_of_the_python_engine() {
        assert_eq!(
            names(&Tier::Pz.chain()),
            ["pz", "rz", "sv", "v3", "omni"],
            "pz asks for itself and then the generic order, without asking twice"
        );
        assert_eq!(names(&Tier::Rz.chain()), ["rz", "sv", "v3", "pz", "omni"]);
        assert_eq!(names(&Tier::Omni.chain()), ["omni", "rz", "sv", "v3", "pz"]);
    }

    #[test]
    fn the_opt_in_english_tier_gives_way_to_its_default_before_anything_else() {
        assert_eq!(
            names(&Tier::V2.chain()),
            ["v2", "v3", "rz", "sv", "pz", "omni"]
        );
    }

    #[test]
    fn a_directory_that_is_not_there_names_the_pattern_it_lacked() {
        let said = find_model(Path::new("no-such-models/x"), "model").err();
        assert!(
            matches!(&said, Some(AsrError::MissingModel(path)) if path.ends_with("model*.onnx")),
            "{said:?}"
        );
    }

    #[test]
    fn no_models_means_no_tier_is_installed() {
        for tier in Tier::ALL {
            assert!(!is_installed(tier, Path::new("no-such-models")), "{tier:?}");
        }
    }

    #[test]
    fn the_first_file_by_name_is_the_one_found() -> Result<(), Box<dyn std::error::Error>> {
        let scratch = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../target/unit-tmp"))
            .join(format!("tier-{}", std::process::id()));
        std::fs::create_dir_all(&scratch)?;
        std::fs::write(scratch.join("model.int8.onnx"), b"")?;
        std::fs::write(scratch.join("model.onnx"), b"")?;
        std::fs::write(scratch.join("tokens.txt"), b"")?;
        let found = find_model(&scratch, "model")?;
        std::fs::remove_dir_all(&scratch)?;
        assert!(found.ends_with("model.int8.onnx"), "{found:?}");
        Ok(())
    }
}
