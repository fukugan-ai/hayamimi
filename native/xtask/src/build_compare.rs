//! RP-24: whether two `build.json` files describe the same build.
//!
//! The build is the same when five items agree: the sherpa-onnx commit, the
//! ONNX Runtime version, the CUDA major version, the cuDNN major version, and
//! the `sm_120` family verdict. The CUDA minor version is recorded and not
//! compared. The `sm_120` family verdict holds when the CUDA provider library
//! has machine code for `sm_120` or `sm_120a`, or has `sm_120` PTX and a
//! `rp17_passed.json` that names the SHA-256 of this very `build.json` (the
//! RP-17 run that proved the driver translated the PTX).

use std::fmt;
use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::error::Error;
use crate::record;

/// The items of `build.json` that must agree, besides the `sm_120` family verdict.
const COMPARED: [&str; 4] = [
    "sherpa_onnx_commit",
    "ort_version",
    "cuda_major",
    "cudnn_major",
];

/// One build: its `build.json`, the digest of that file, and the RP-17 record if there is one.
#[derive(Debug)]
pub struct BuildFacts {
    pub build: Value,
    pub build_json_sha256: String,
    pub rp17_passed: Option<Value>,
}

impl BuildFacts {
    /// Reads `build.json` and, when present, `rp17_passed.json` from `platform_dir`.
    pub fn read(platform_dir: &Path) -> Result<BuildFacts, Error> {
        let build_path = platform_dir.join("build.json");
        let text = fs::read_to_string(&build_path).map_err(Error::io("read", &build_path))?;
        let record_path = platform_dir.join("rp17_passed.json");
        let rp17_passed = if record_path.is_file() {
            let text = fs::read_to_string(&record_path).map_err(Error::io("read", &record_path))?;
            Some(serde_json::from_str(&text).map_err(Error::Json)?)
        } else {
            None
        };
        Ok(BuildFacts {
            build: serde_json::from_str(&text).map_err(Error::Json)?,
            build_json_sha256: record::sha256_file(&build_path)?,
            rp17_passed,
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Mismatch {
    /// A compared item differs between the two builds.
    Field {
        field: &'static str,
        left: String,
        right: String,
    },
    /// A compared item is missing from a `build.json`.
    MissingField(&'static str),
    /// No `sm_120` or `sm_120a` machine code, and no `sm_120` PTX to translate.
    NoSm120Ptx,
    /// PTX only, and no RP-17 record.
    NoRp17Record,
    /// PTX only, and the RP-17 record belongs to another `build.json`.
    Rp17RecordForAnotherBuild {
        recorded: Option<String>,
        build: String,
    },
}

impl fmt::Display for Mismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Mismatch::Field { field, left, right } => {
                write!(f, "`{field}` differs: {left} and {right}")
            }
            Mismatch::MissingField(field) => write!(f, "`{field}` is missing from a build.json"),
            Mismatch::NoSm120Ptx => {
                f.write_str("no sm_120 or sm_120a machine code and no sm_120 PTX")
            }
            Mismatch::NoRp17Record => {
                f.write_str("sm_120 PTX only, and no rp17_passed.json to show it ran")
            }
            Mismatch::Rp17RecordForAnotherBuild { recorded, build } => write!(
                f,
                "rp17_passed.json names build.json {recorded:?}, this build.json is {build}"
            ),
        }
    }
}

impl std::error::Error for Mismatch {}

/// Whether `build.json` counts `architecture` kernels in `section` (`provider_cubin` or `provider_ptx`).
fn has_kernels(build: &Value, section: &str, architecture: &str) -> bool {
    build
        .get(section)
        .and_then(|counts| counts.get(architecture))
        .and_then(Value::as_u64)
        .is_some_and(|count| count > 0)
}

/// Checks the `sm_120` family rule for one build.
pub fn sm_120_family(facts: &BuildFacts) -> Result<(), Mismatch> {
    if ["sm_120", "sm_120a"]
        .iter()
        .any(|architecture| has_kernels(&facts.build, "provider_cubin", architecture))
    {
        return Ok(());
    }
    if !has_kernels(&facts.build, "provider_ptx", "sm_120") {
        return Err(Mismatch::NoSm120Ptx);
    }
    let Some(record) = &facts.rp17_passed else {
        return Err(Mismatch::NoRp17Record);
    };
    let recorded = record.get("build_json_sha256").and_then(Value::as_str);
    if recorded != Some(facts.build_json_sha256.as_str()) {
        return Err(Mismatch::Rp17RecordForAnotherBuild {
            recorded: recorded.map(str::to_owned),
            build: facts.build_json_sha256.clone(),
        });
    }
    Ok(())
}

fn field_text(build: &Value, field: &'static str) -> Result<String, Mismatch> {
    build
        .get(field)
        .map(Value::to_string)
        .ok_or(Mismatch::MissingField(field))
}

/// Compares the five items of two builds.
pub fn compare_build_json(left: &BuildFacts, right: &BuildFacts) -> Result<(), Mismatch> {
    for field in COMPARED {
        let (a, b) = (
            field_text(&left.build, field)?,
            field_text(&right.build, field)?,
        );
        if a != b {
            return Err(Mismatch::Field {
                field,
                left: a,
                right: b,
            });
        }
    }
    let (a, b) = (sm_120_family(left).is_ok(), sm_120_family(right).is_ok());
    if a != b {
        return Err(Mismatch::Field {
            field: "sm_120_family",
            left: a.to_string(),
            right: b.to_string(),
        });
    }
    Ok(())
}
