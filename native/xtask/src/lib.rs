//! Developer tasks for the hayamimi workspace.
//!
//! `cargo run -p xtask -- sherpa-gpu --cuda-dir <dir> --cudnn-dir <dir>` builds
//! the CUDA flavour of the sherpa-onnx C library that `hayamimi-gpu` links
//! against. The steps, the versions and the way back are in
//! `docs/guide/build_gpu.ja.md`; the reasons are in `docs/design/rust_port.ja.md` 3.12.
//!
//! The library also holds the rule that compares two `build.json` files
//! ([`build_compare`], RP-24), so that its tests can reach it.

pub mod build_compare;
pub mod error;
pub mod options;
pub mod record;
pub mod steps;
pub mod tools;

use error::Error;
use options::Options;

/// Runs the task named by the first argument.
pub fn run(args: &[String]) -> Result<(), Error> {
    let Some((task, rest)) = args.split_first() else {
        return Err(Error::Usage);
    };
    match task.as_str() {
        "sherpa-gpu" => steps::sherpa_gpu(&Options::parse(rest)?),
        other => Err(Error::UnknownTask(other.to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_task_is_a_usage_error() {
        assert!(matches!(run(&[]), Err(Error::Usage)));
    }

    #[test]
    fn an_unknown_task_is_named() {
        let said = run(&[String::from("frobnicate")]);
        assert!(matches!(said, Err(Error::UnknownTask(task)) if task == "frobnicate"));
    }

    #[test]
    fn a_task_without_its_required_flags_stops_before_building() {
        let said = run(&[String::from("sherpa-gpu")]);
        assert!(
            matches!(said, Err(Error::MissingFlag("--cuda-dir"))),
            "{said:?}"
        );
    }
}
