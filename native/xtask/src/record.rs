//! What was built: facts read back from the tools and written as `build.json`.
//!
//! The record states versions and counts for later comparison. Nothing here
//! decides whether a build is acceptable; the RP-17 and RP-24 tests do.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::error::Error;
use crate::tools;

/// Version of the `build.json` layout.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Serialize)]
pub struct RuntimeFile {
    pub name: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Serialize)]
pub struct BuildRecord {
    pub schema_version: u32,
    pub os: &'static str,
    pub built_at_epoch: u64,
    pub sherpa_onnx_tag: String,
    pub sherpa_onnx_commit: String,
    pub ort_version: String,
    pub ort_package: String,
    pub ort_zip_sha256: Option<String>,
    pub cuda_version: String,
    pub cuda_major: u32,
    pub nvcc_build: String,
    pub cudnn_version: String,
    pub cudnn_major: u32,
    pub gpu_name: String,
    pub gpu_driver: String,
    pub gpu_compute_capability: String,
    /// Machine-code kernels in the CUDA provider library, per target architecture.
    pub provider_cubin: BTreeMap<String, u32>,
    /// PTX kernels in the CUDA provider library, per target architecture.
    pub provider_ptx: BTreeMap<String, u32>,
    /// Libraries the CUDA provider library imports (`dumpbin /dependents`).
    pub provider_dependents: Vec<String>,
    pub runtime_files: Vec<RuntimeFile>,
}

/// The CUDA toolkit release reported by `nvcc --version`.
#[derive(Debug, PartialEq, Eq)]
pub struct CudaRelease {
    pub version: String,
    pub major: u32,
    pub build: String,
}

/// The first GPU reported by `nvidia-smi`.
#[derive(Debug, PartialEq, Eq)]
pub struct GpuInfo {
    pub name: String,
    pub driver: String,
    pub compute_capability: String,
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(char::from(DIGITS[usize::from(byte >> 4)]));
        text.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    text
}

fn sha256_reader(mut reader: impl io::Read) -> io::Result<String> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1 << 16];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

pub fn sha256_file(path: &Path) -> Result<String, Error> {
    let file = fs::File::open(path).map_err(Error::io("open", path))?;
    sha256_reader(file).map_err(Error::io("hash", path))
}

pub fn epoch_now() -> Result<u64, Error> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .map_err(|_| Error::Unreadable {
            what: "the system clock",
        })
}

/// Every file in `dir` that `is_library` accepts, with its size and digest
/// (links are followed), sorted by name.
pub fn runtime_files(dir: &Path, is_library: fn(&str) -> bool) -> Result<Vec<RuntimeFile>, Error> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir).map_err(Error::io("list", dir))? {
        let entry = entry.map_err(Error::io("list", dir))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_library(&name) {
            continue;
        }
        let bytes = fs::metadata(&path).map_err(Error::io("stat", &path))?.len();
        files.push(RuntimeFile {
            name,
            bytes,
            sha256: sha256_file(&path)?,
        });
    }
    files.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(files)
}

/// Atomically writes the record: a sibling file first, then a rename.
pub fn write(path: &Path, record: &BuildRecord) -> Result<(), Error> {
    let text = serde_json::to_string_pretty(record).map_err(Error::Json)?;
    let mut part = path.as_os_str().to_owned();
    part.push(".part");
    let part = Path::new(&part);
    fs::write(part, text).map_err(Error::io("write", part))?;
    fs::rename(part, path).map_err(Error::io("rename", path))
}

/// `Cuda compilation tools, release 13.4, V13.4.59` into 13.4.59 and its major.
pub fn parse_cuda_release(nvcc_output: &str) -> Result<CudaRelease, Error> {
    let unreadable = Error::Unreadable {
        what: "the nvcc version",
    };
    let Some(version) = nvcc_output
        .lines()
        .find_map(|line| line.split_once(", V").map(|(_, rest)| rest.trim()))
    else {
        return Err(unreadable);
    };
    let Some(major) = version
        .split('.')
        .next()
        .and_then(|digits| digits.parse::<u32>().ok())
    else {
        return Err(unreadable);
    };
    let Some(build) = nvcc_output
        .lines()
        .find_map(|line| line.trim().strip_prefix("Build "))
    else {
        return Err(unreadable);
    };
    Ok(CudaRelease {
        version: version.to_owned(),
        major,
        build: build.to_owned(),
    })
}

/// How many entries of a `cuobjdump --list-elf` or `--list-ptx` listing target each architecture.
///
/// An entry looks like `ELF file    7: name.7.sm_90a.cubin`; only entries whose
/// extension is `extension` are counted.
pub fn count_by_arch(listing: &str, extension: &str) -> BTreeMap<String, u32> {
    let mut counts = BTreeMap::new();
    for line in listing.lines() {
        let Some((_, tail)) = line.rsplit_once(".sm_") else {
            continue;
        };
        let Some((arch, ending)) = tail.split_once('.') else {
            continue;
        };
        if ending.trim() == extension {
            *counts.entry(format!("sm_{arch}")).or_insert(0) += 1;
        }
    }
    counts
}

/// The library names under `Image has the following dependencies:` in `dumpbin /dependents`.
pub fn parse_dependents(dumpbin_output: &str) -> Vec<String> {
    dumpbin_output
        .lines()
        .skip_while(|line| !line.contains("Image has the following dependencies"))
        .skip(1)
        .map(str::trim)
        .skip_while(|line| line.is_empty())
        .take_while(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

/// `CUDNN_MAJOR`, `CUDNN_MINOR` and `CUDNN_PATCHLEVEL` of `cudnn_version.h` into `9.27.0` and 9.
pub fn parse_cudnn_version(header: &str) -> Result<(String, u32), Error> {
    let define = |name: &str| -> Option<u32> {
        header.lines().find_map(|line| {
            let mut words = line.split_whitespace();
            (words.next() == Some("#define") && words.next() == Some(name))
                .then(|| words.next().and_then(|value| value.parse::<u32>().ok()))
                .flatten()
        })
    };
    let (Some(major), Some(minor), Some(patch)) = (
        define("CUDNN_MAJOR"),
        define("CUDNN_MINOR"),
        define("CUDNN_PATCHLEVEL"),
    ) else {
        return Err(Error::Unreadable {
            what: "the cuDNN version header",
        });
    };
    Ok((format!("{major}.{minor}.{patch}"), major))
}

/// The first line of `nvidia-smi --query-gpu=name,driver_version,compute_cap --format=csv,noheader`.
pub fn parse_gpu(csv: &str) -> Result<GpuInfo, Error> {
    let Some(line) = csv.lines().find(|line| !line.trim().is_empty()) else {
        return Err(Error::Unreadable {
            what: "the nvidia-smi output",
        });
    };
    let fields: Vec<&str> = line.split(',').map(str::trim).collect();
    let [name, driver, compute_capability] = fields[..] else {
        return Err(Error::Unreadable {
            what: "the nvidia-smi output",
        });
    };
    Ok(GpuInfo {
        name: name.to_owned(),
        driver: driver.to_owned(),
        compute_capability: compute_capability.to_owned(),
    })
}

/// Suffix of an executable file name on this operating system.
#[cfg(windows)]
const EXE_SUFFIX: &str = ".exe";
#[cfg(not(windows))]
const EXE_SUFFIX: &str = "";

pub fn cuda_release(cuda_dir: &Path) -> Result<CudaRelease, Error> {
    let nvcc = cuda_dir.join("bin").join(format!("nvcc{EXE_SUFFIX}"));
    parse_cuda_release(&tools::capture(Command::new(nvcc).arg("--version"))?)
}

pub fn cudnn_version(cudnn_dir: &Path) -> Result<(String, u32), Error> {
    let header = cudnn_dir.join("include").join("cudnn_version.h");
    let text = fs::read_to_string(&header).map_err(Error::io("read", &header))?;
    parse_cudnn_version(&text)
}

/// Counts per architecture name.
type ArchCounts = BTreeMap<String, u32>;

/// Machine-code and PTX counts per architecture in the provider library.
pub fn provider_arches(
    cuda_dir: &Path,
    provider: &Path,
) -> Result<(ArchCounts, ArchCounts), Error> {
    let cuobjdump = cuda_dir.join("bin").join(format!("cuobjdump{EXE_SUFFIX}"));
    let elf = tools::capture(Command::new(&cuobjdump).arg("--list-elf").arg(provider))?;
    let ptx = tools::capture(Command::new(&cuobjdump).arg("--list-ptx").arg(provider))?;
    Ok((count_by_arch(&elf, "cubin"), count_by_arch(&ptx, "ptx")))
}

/// The library names under `Dynamic section` of `readelf -d` that are `(NEEDED)`.
pub fn parse_needed(readelf_output: &str) -> Vec<String> {
    readelf_output
        .lines()
        .filter(|line| line.contains("(NEEDED)"))
        .filter_map(|line| {
            let (_, tail) = line.split_once('[')?;
            tail.split_once(']').map(|(name, _)| name.to_owned())
        })
        .collect()
}

/// The libraries the provider library imports: `dumpbin /dependents` on Windows.
#[cfg(windows)]
pub fn provider_dependents(provider: &Path) -> Result<Vec<String>, Error> {
    let report = tools::capture(Command::new("dumpbin").arg("/dependents").arg(provider))?;
    Ok(parse_dependents(&report))
}

/// The libraries the provider library needs: `readelf -d` on Linux.
#[cfg(not(windows))]
pub fn provider_dependents(provider: &Path) -> Result<Vec<String>, Error> {
    let report = tools::capture(Command::new("readelf").arg("-d").arg(provider))?;
    Ok(parse_needed(&report))
}

pub fn gpu_info() -> Result<GpuInfo, Error> {
    let csv = tools::capture(Command::new("nvidia-smi").args([
        "--query-gpu=name,driver_version,compute_cap",
        "--format=csv,noheader",
    ]))?;
    parse_gpu(&csv)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NVCC: &str = "nvcc: NVIDIA (R) Cuda compiler driver\n\
        Copyright (c) 2005-2026 NVIDIA Corporation\n\
        Built on Sun_Aug_03_13:24:32_Pacific_Standard_Time_2026\n\
        Cuda compilation tools, release 13.4, V13.4.59\n\
        Build cuda_13.4.r13.4/compiler.38657139_0\n";

    #[test]
    fn needed_libraries_are_read_from_readelf_lines() {
        let output = " 0x0000000000000001 (NEEDED)             Shared library: [libcublasLt.so.13]\n\
             0x000000000000000e (SONAME)            Library soname: [libprovider.so]\n\
             0x0000000000000001 (NEEDED)             Shared library: [libc.so.6]\n";
        assert_eq!(parse_needed(output), ["libcublasLt.so.13", "libc.so.6"]);
        assert!(parse_needed("").is_empty());
    }

    #[test]
    fn the_sha256_of_abc_is_the_standard_vector() -> io::Result<()> {
        let digest = sha256_reader(&b"abc"[..])?;
        assert_eq!(
            digest,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        Ok(())
    }

    #[test]
    fn the_sha256_of_nothing_is_the_empty_vector() -> io::Result<()> {
        let digest = sha256_reader(&b""[..])?;
        assert_eq!(
            digest,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        Ok(())
    }

    #[test]
    fn the_cuda_release_is_read_from_the_nvcc_banner() -> Result<(), Error> {
        let release = parse_cuda_release(NVCC)?;
        assert_eq!(release.version, "13.4.59");
        assert_eq!(release.major, 13);
        assert_eq!(release.build, "cuda_13.4.r13.4/compiler.38657139_0");
        Ok(())
    }

    #[test]
    fn a_banner_without_a_release_line_is_unreadable() {
        let said = parse_cuda_release("nvcc: NVIDIA (R) Cuda compiler driver\n");
        assert!(matches!(said, Err(Error::Unreadable { .. })), "{said:?}");
    }

    #[test]
    fn a_non_numeric_cuda_major_is_unreadable() {
        let said = parse_cuda_release("Cuda compilation tools, release x, Vx.4.59\nBuild b\n");
        assert!(matches!(said, Err(Error::Unreadable { .. })), "{said:?}");
    }

    const ELF_LISTING: &str = "ELF file    1: onnxruntime_providers_cuda.1.sm_75.cubin\n\
        ELF file    2: onnxruntime_providers_cuda.2.sm_120a.cubin\n\
        ELF file    3: onnxruntime_providers_cuda.3.sm_120a.cubin\n\
        ELF file    4: onnxruntime_providers_cuda.4.sm_120.ptx\n";

    #[test]
    fn architectures_are_counted_per_extension() {
        let cubin = count_by_arch(ELF_LISTING, "cubin");
        assert_eq!(cubin.get("sm_75"), Some(&1));
        assert_eq!(cubin.get("sm_120a"), Some(&2));
        assert_eq!(cubin.get("sm_120"), None);
        let ptx = count_by_arch(ELF_LISTING, "ptx");
        assert_eq!(ptx.get("sm_120"), Some(&1));
    }

    #[test]
    fn an_empty_listing_has_no_architectures() {
        assert!(count_by_arch("", "cubin").is_empty());
    }

    const DUMPBIN: &str = "Dump of file onnxruntime_providers_cuda.dll\n\n\
        File Type: DLL\n\n  Image has the following dependencies:\n\n    \
        cublasLt64_13.dll\n    cublas64_13.dll\n    KERNEL32.dll\n\n  Summary\n\n        \
        1000 .data\n";

    #[test]
    fn dependents_stop_at_the_blank_line_after_the_list() {
        assert_eq!(
            parse_dependents(DUMPBIN),
            ["cublasLt64_13.dll", "cublas64_13.dll", "KERNEL32.dll"]
        );
    }

    #[test]
    fn a_report_without_a_dependency_list_has_none() {
        assert!(parse_dependents("Dump of file x.dll\n\nFile Type: DLL\n").is_empty());
    }

    #[test]
    fn the_cudnn_version_is_read_from_the_header_defines() -> Result<(), Error> {
        let header = "#define CUDNN_MAJOR 9\n#define CUDNN_MINOR 27\n#define CUDNN_PATCHLEVEL 0\n";
        assert_eq!(parse_cudnn_version(header)?, (String::from("9.27.0"), 9));
        Ok(())
    }

    #[test]
    fn a_header_without_the_patch_level_is_unreadable() {
        let said = parse_cudnn_version("#define CUDNN_MAJOR 9\n#define CUDNN_MINOR 27\n");
        assert!(matches!(said, Err(Error::Unreadable { .. })), "{said:?}");
    }

    #[test]
    fn the_gpu_is_the_first_csv_line() -> Result<(), Error> {
        let gpu = parse_gpu("NVIDIA GeForce RTX 5060 Ti, 610.88, 12.0\n")?;
        assert_eq!(gpu.name, "NVIDIA GeForce RTX 5060 Ti");
        assert_eq!(gpu.driver, "610.88");
        assert_eq!(gpu.compute_capability, "12.0");
        Ok(())
    }

    #[test]
    fn a_gpu_line_with_two_fields_is_unreadable() {
        let said = parse_gpu("NVIDIA, 610.88\n");
        assert!(matches!(said, Err(Error::Unreadable { .. })), "{said:?}");
    }

    #[test]
    fn no_gpu_line_is_unreadable() {
        let said = parse_gpu("\n");
        assert!(matches!(said, Err(Error::Unreadable { .. })), "{said:?}");
    }
}
