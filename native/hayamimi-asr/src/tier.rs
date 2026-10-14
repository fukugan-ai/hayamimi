//! The recognizer tiers: which model directory each one reads, which of its
//! files say that it is installed, and the order a missing tier gives way in.
//!
//! The names are the ones the wire format carries in a final event's `tier`.

use std::path::{Path, PathBuf};

use crate::error::AsrError;

/// One recognizer of the routed engine. All of them run on the CPU (int8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tier {
    /// ReazonSpeech zipformer transducer: Japanese. Always resident.
    Rz,
    /// Paraformer: Mandarin.
    Pz,
    /// SenseVoice: Korean and Cantonese, and the confirmation probe.
    Sv,
    /// Parakeet TDT v3: English and the other European languages.
    V3,
    /// Parakeet TDT v2: English only, opt-in.
    V2,
    /// Omnilingual CTC: the generalist.
    Omni,
    /// Parakeet TDT-CTC ja: the second opinion for Japanese.
    Pja,
}

/// The tiers a missing tier gives way to, after its own default, in this order.
const GENERIC_CHAIN: [Tier; 5] = [Tier::Rz, Tier::Sv, Tier::V3, Tier::Pz, Tier::Omni];

impl Tier {
    /// Every tier.
    pub const ALL: [Tier; 7] = [
        Tier::Rz,
        Tier::Pz,
        Tier::Sv,
        Tier::V3,
        Tier::V2,
        Tier::Omni,
        Tier::Pja,
    ];

    /// The name used in events.
    pub fn name(self) -> &'static str {
        match self {
            Tier::Rz => "rz",
            Tier::Pz => "pz",
            Tier::Sv => "sv",
            Tier::V3 => "v3",
            Tier::V2 => "v2",
            Tier::Omni => "omni",
            Tier::Pja => "pja",
        }
    }

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

    /// The tiers to try for a request of this tier, in order, each once. An
    /// opt-in tier gives way to the default it is an alternative to first
    /// (v2 to v3, never to the unpunctuated upper-case output of rz).
    pub fn chain(self) -> Vec<Tier> {
        let own_default: &[Tier] = match self {
            Tier::V2 => &[Tier::V3],
            _ => &[],
        };
        let mut chain = vec![self];
        for tier in own_default.iter().chain(GENERIC_CHAIN.iter()) {
            if !chain.contains(tier) {
                chain.push(*tier);
            }
        }
        chain
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
    fn a_chain_never_repeats_a_tier() {
        for tier in Tier::ALL {
            let chain = tier.chain();
            for (index, one) in chain.iter().enumerate() {
                assert!(!chain[index + 1..].contains(one), "{tier:?}");
            }
        }
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
