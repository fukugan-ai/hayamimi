//! The link arguments every executable that links `hayamimi-gpu` needs on
//! Windows, and the check that each crate of the workspace passes them to
//! both its binaries and its integration tests.
//!
//! The two arguments make `sherpa-onnx-c-api.dll` load lazily, so that the
//! ONNX Runtime beside the executable is loaded first (see `runtime`). A build
//! script's link arguments reach only the targets of the crate that owns the
//! script, so every crate that depends on `hayamimi-gpu` repeats them in its
//! own build script, for `bins` and for `tests` separately.

use std::fmt;
use std::path::Path;

use serde::Deserialize;

/// The library crate whose dependents are checked.
pub const LIBRARY_CRATE: &str = "hayamimi-gpu";

/// The two link arguments, in the order a build script emits them.
pub const REQUIRED_ARGS: [&str; 2] = ["/DELAYLOAD:sherpa-onnx-c-api.dll", "delayimp.lib"];

/// The link arguments one crate's build script passes to its binaries and to its tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrateArgs {
    pub name: String,
    pub bins: Vec<String>,
    pub tests: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LinkArgError {
    /// No crate that links the library was found; zero crates is not a pass.
    NoCrates,
    /// A crate does not pass one of the two arguments to one kind of target.
    Missing {
        krate: String,
        target: &'static str,
        arg: &'static str,
    },
    /// `cargo metadata` output or a build script could not be read.
    Unreadable(String),
}

impl fmt::Display for LinkArgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LinkArgError::NoCrates => f.write_str("no crate that links hayamimi-gpu was found"),
            LinkArgError::Missing { krate, target, arg } => {
                write!(f, "crate `{krate}` does not pass `{arg}` to its {target}")
            }
            LinkArgError::Unreadable(why) => write!(f, "cannot read the workspace: {why}"),
        }
    }
}

impl std::error::Error for LinkArgError {}

/// The text between `key` and the next double quote, for every `key` on the line.
fn values_after<'a>(line: &'a str, key: &str) -> Vec<&'a str> {
    line.match_indices(key)
        .filter_map(|(at, _)| {
            let rest = &line[at + key.len()..];
            rest.split_once('"').map(|(value, _)| value)
        })
        .collect()
}

/// The link arguments a build script emits, as `(bins, tests)`. Only literal
/// `cargo::rustc-link-arg*=` lines are read; comment lines are skipped.
pub fn args_in_build_script(text: &str) -> (Vec<String>, Vec<String>) {
    let mut bins = Vec::new();
    let mut tests = Vec::new();
    for line in text
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
    {
        for value in values_after(line, "rustc-link-arg-bins=") {
            bins.push(value.to_owned());
        }
        for value in values_after(line, "rustc-link-arg-tests=") {
            tests.push(value.to_owned());
        }
        for value in values_after(line, "rustc-link-arg=") {
            bins.push(value.to_owned());
            tests.push(value.to_owned());
        }
    }
    (bins, tests)
}

/// Fails with the first crate, target kind and argument that is missing.
pub fn check(crates: &[CrateArgs]) -> Result<(), LinkArgError> {
    if crates.is_empty() {
        return Err(LinkArgError::NoCrates);
    }
    for krate in crates {
        for (target, passed) in [("bins", &krate.bins), ("tests", &krate.tests)] {
            for arg in REQUIRED_ARGS {
                if !passed.iter().any(|given| given == arg) {
                    return Err(LinkArgError::Missing {
                        krate: krate.name.clone(),
                        target,
                        arg,
                    });
                }
            }
        }
    }
    Ok(())
}

/// The part of `cargo metadata` output that the check reads.
#[derive(Debug, Deserialize)]
pub struct Metadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct Package {
    name: String,
    id: String,
    dependencies: Vec<Dependency>,
    targets: Vec<Target>,
}

#[derive(Debug, Deserialize)]
struct Dependency {
    name: String,
}

#[derive(Debug, Deserialize)]
struct Target {
    kind: Vec<String>,
    src_path: String,
}

impl Metadata {
    pub fn parse(json: &str) -> Result<Metadata, LinkArgError> {
        serde_json::from_str(json).map_err(|error| LinkArgError::Unreadable(error.to_string()))
    }

    fn members(&self) -> impl Iterator<Item = &Package> {
        self.packages
            .iter()
            .filter(|package| self.workspace_members.contains(&package.id))
    }

    /// The names of the crates the check counts: the library crate itself and
    /// the workspace crates that depend on it.
    pub fn counted_crates(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .linking_crates()
            .into_iter()
            .map(|package| package.name.clone())
            .collect();
        names.sort();
        names
    }

    /// The workspace crates whose executables link the library: the library
    /// crate itself, which has its own binaries and tests, and its dependents.
    fn linking_crates(&self) -> Vec<&Package> {
        self.members()
            .filter(|package| {
                package.name == LIBRARY_CRATE
                    || package
                        .dependencies
                        .iter()
                        .any(|dependency| dependency.name == LIBRARY_CRATE)
            })
            .collect()
    }
}

impl Package {
    /// The link arguments of this crate's build script; none when it has no build script.
    fn build_script_args(&self) -> Result<CrateArgs, LinkArgError> {
        let script = self
            .targets
            .iter()
            .find(|target| target.kind.iter().any(|kind| kind == "custom-build"));
        let (bins, tests) = match script {
            Some(target) => {
                let text = std::fs::read_to_string(Path::new(&target.src_path))
                    .map_err(|error| LinkArgError::Unreadable(error.to_string()))?;
                args_in_build_script(&text)
            }
            None => (Vec::new(), Vec::new()),
        };
        Ok(CrateArgs {
            name: self.name.clone(),
            bins,
            tests,
        })
    }
}

/// Reads the build script of every crate that links the library and checks it.
pub fn check_workspace(metadata: &Metadata) -> Result<(), LinkArgError> {
    let crates = metadata
        .linking_crates()
        .into_iter()
        .map(Package::build_script_args)
        .collect::<Result<Vec<_>, _>>()?;
    check(&crates)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn passing(name: &str) -> CrateArgs {
        let both: Vec<String> = REQUIRED_ARGS.iter().map(|arg| (*arg).to_owned()).collect();
        CrateArgs {
            name: name.to_owned(),
            bins: both.clone(),
            tests: both,
        }
    }

    #[test]
    fn all_dependent_crates_have_both_link_args_in_bins_and_tests() {
        let crates = [passing("a"), passing("b"), passing("c")];
        assert_eq!(check(&crates), Ok(()));
    }

    #[test]
    fn dependent_crate_missing_one_of_the_two_link_args_is_named() {
        let mut broken = passing("b");
        broken.tests.retain(|arg| arg != "delayimp.lib");
        let crates = [passing("a"), broken, passing("c")];
        assert_eq!(
            check(&crates),
            Err(LinkArgError::Missing {
                krate: String::from("b"),
                target: "tests",
                arg: "delayimp.lib"
            })
        );
    }

    #[test]
    fn link_args_for_bins_do_not_cover_tests() {
        let only_bins = CrateArgs {
            tests: Vec::new(),
            ..passing("b")
        };
        assert_eq!(
            check(&[only_bins]),
            Err(LinkArgError::Missing {
                krate: String::from("b"),
                target: "tests",
                arg: "/DELAYLOAD:sherpa-onnx-c-api.dll"
            })
        );
    }

    #[test]
    fn no_counted_crates_is_an_error_not_a_pass() -> Result<(), LinkArgError> {
        assert_eq!(check(&[]), Err(LinkArgError::NoCrates));
        let unrelated = Metadata::parse(
            r#"{"packages":[{"name":"other","id":"other 0.1.0","dependencies":[],"targets":[]}],
                "workspace_members":["other 0.1.0"]}"#,
        )?;
        assert_eq!(check_workspace(&unrelated), Err(LinkArgError::NoCrates));
        Ok(())
    }

    #[test]
    fn a_build_script_is_read_by_its_literal_link_arg_lines() {
        let script = r#"
            // println!("cargo::rustc-link-arg-bins=commented.lib");
            println!("cargo::rustc-link-arg-bins=/DELAYLOAD:sherpa-onnx-c-api.dll");
            println!("cargo::rustc-link-arg-tests=delayimp.lib");
            println!("cargo::rustc-link-arg=common.lib");
        "#;
        let (bins, tests) = args_in_build_script(script);
        assert_eq!(bins, ["/DELAYLOAD:sherpa-onnx-c-api.dll", "common.lib"]);
        assert_eq!(tests, ["delayimp.lib", "common.lib"]);
        assert_eq!(args_in_build_script(""), (Vec::new(), Vec::new()));
    }

    #[test]
    fn counted_crates_are_the_library_crate_and_its_workspace_dependents()
    -> Result<(), LinkArgError> {
        let metadata = Metadata::parse(
            r#"{"packages":[
                {"name":"hayamimi-gpu","id":"g","dependencies":[],"targets":[]},
                {"name":"asr","id":"a","dependencies":[{"name":"hayamimi-gpu"}],"targets":[]},
                {"name":"relay","id":"r","dependencies":[{"name":"serde"}],"targets":[]},
                {"name":"outsider","id":"o","dependencies":[{"name":"hayamimi-gpu"}],"targets":[]}],
                "workspace_members":["g","a","r"]}"#,
        )?;
        assert_eq!(metadata.counted_crates(), ["asr", "hayamimi-gpu"]);
        Ok(())
    }
}
