//! Command-line options of the `sherpa-gpu` task. Everything the build needs
//! arrives as an argument; the task reads no environment variable.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::error::Error;

/// sherpa-onnx release that the Rust crate `sherpa-onnx` 1.13.8 is built against.
pub const DEFAULT_SHERPA_TAG: &str = "v1.13.8";
/// ONNX Runtime release that sherpa-onnx v1.13.8 pairs with; the CUDA 13 package of it is used.
pub const DEFAULT_ORT_VERSION: &str = "1.28.2";

const FLAGS: [&str; 6] = [
    "--sherpa-tag",
    "--ort-version",
    "--ort-dir",
    "--out-dir",
    "--cuda-dir",
    "--cudnn-dir",
];

#[derive(Debug, PartialEq, Eq)]
pub struct Options {
    pub sherpa_tag: String,
    pub ort_version: String,
    /// An unpacked ONNX Runtime package; when absent the package is downloaded.
    pub ort_dir: Option<PathBuf>,
    pub out_dir: PathBuf,
    pub cuda_dir: PathBuf,
    pub cudnn_dir: PathBuf,
}

impl Options {
    pub fn parse(args: &[String]) -> Result<Options, Error> {
        let mut values = read_flags(args)?;
        let sherpa_tag = match values.remove("--sherpa-tag") {
            Some(tag) => tag,
            // DEFAULT: docs/design/rust_port.ja.md 3.12 fixes sherpa-onnx v1.13.8 as the argument default.
            None => DEFAULT_SHERPA_TAG.to_owned(),
        };
        let ort_version = match values.remove("--ort-version") {
            Some(version) => version,
            // DEFAULT: docs/design/rust_port.ja.md 3.12 fixes ONNX Runtime 1.28.2 as the argument default.
            None => DEFAULT_ORT_VERSION.to_owned(),
        };
        let out_dir = match values.remove("--out-dir") {
            Some(dir) => PathBuf::from(dir),
            // DEFAULT: docs/design/rust_port.ja.md 3.12 keeps the build under native/target/sherpa-gpu.
            None => default_out_dir()?,
        };
        Ok(Options {
            sherpa_tag,
            ort_version,
            ort_dir: values.remove("--ort-dir").map(PathBuf::from),
            out_dir,
            cuda_dir: required(&mut values, "--cuda-dir")?,
            cudnn_dir: required(&mut values, "--cudnn-dir")?,
        })
    }
}

/// Flag names mapped to their values; an unknown, repeated or valueless flag is an error.
fn read_flags(args: &[String]) -> Result<BTreeMap<&'static str, String>, Error> {
    let mut values = BTreeMap::new();
    let mut rest = args.iter();
    while let Some(given) = rest.next() {
        let Some(name) = FLAGS.iter().copied().find(|known| *known == given.as_str()) else {
            return Err(Error::UnknownFlag(given.clone()));
        };
        let Some(value) = rest.next() else {
            return Err(Error::MissingValue(name));
        };
        if values.insert(name, value.clone()).is_some() {
            return Err(Error::DuplicateFlag(name));
        }
    }
    Ok(values)
}

fn required(
    values: &mut BTreeMap<&'static str, String>,
    name: &'static str,
) -> Result<PathBuf, Error> {
    values
        .remove(name)
        .map(PathBuf::from)
        .ok_or(Error::MissingFlag(name))
}

/// `native/target/sherpa-gpu`, next to this crate's workspace.
fn default_out_dir() -> Result<PathBuf, Error> {
    let Some(native) = Path::new(env!("CARGO_MANIFEST_DIR")).parent() else {
        return Err(Error::Unreadable {
            what: "the workspace directory",
        });
    };
    Ok(native.join("target").join("sherpa-gpu"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    #[test]
    fn the_required_flags_alone_take_the_pinned_versions() -> Result<(), Error> {
        let parsed = Options::parse(&words(&["--cuda-dir", "c", "--cudnn-dir", "d"]))?;
        assert_eq!(parsed.sherpa_tag, DEFAULT_SHERPA_TAG);
        assert_eq!(parsed.ort_version, DEFAULT_ORT_VERSION);
        assert_eq!(parsed.ort_dir, None);
        assert!(parsed.out_dir.ends_with("target/sherpa-gpu"));
        Ok(())
    }

    #[test]
    fn every_flag_overrides_its_default() -> Result<(), Error> {
        let parsed = Options::parse(&words(&[
            "--sherpa-tag",
            "v9",
            "--ort-version",
            "2.0",
            "--ort-dir",
            "o",
            "--out-dir",
            "out",
            "--cuda-dir",
            "c",
            "--cudnn-dir",
            "d",
        ]))?;
        assert_eq!(parsed.sherpa_tag, "v9");
        assert_eq!(parsed.ort_version, "2.0");
        assert_eq!(parsed.ort_dir, Some(PathBuf::from("o")));
        assert_eq!(parsed.out_dir, PathBuf::from("out"));
        Ok(())
    }

    #[test]
    fn a_missing_required_flag_is_named() {
        let said = Options::parse(&words(&["--cuda-dir", "c"]));
        assert!(
            matches!(said, Err(Error::MissingFlag("--cudnn-dir"))),
            "{said:?}"
        );
    }

    #[test]
    fn an_unknown_flag_is_refused() {
        let said = Options::parse(&words(&["--gpu", "1"]));
        assert!(matches!(said, Err(Error::UnknownFlag(_))), "{said:?}");
    }

    #[test]
    fn a_flag_without_a_value_is_refused() {
        let said = Options::parse(&words(&["--cuda-dir"]));
        assert!(
            matches!(said, Err(Error::MissingValue("--cuda-dir"))),
            "{said:?}"
        );
    }

    #[test]
    fn a_repeated_flag_is_refused() {
        let said = Options::parse(&words(&["--cuda-dir", "a", "--cuda-dir", "b"]));
        assert!(
            matches!(said, Err(Error::DuplicateFlag("--cuda-dir"))),
            "{said:?}"
        );
    }

    #[test]
    fn no_arguments_at_all_miss_the_cuda_dir_first() {
        let said = Options::parse(&[]);
        assert!(
            matches!(said, Err(Error::MissingFlag("--cuda-dir"))),
            "{said:?}"
        );
    }
}
