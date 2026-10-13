//! The `sherpa-gpu` task: fetch, build and install the CUDA flavour of the
//! sherpa-onnx C library against a CUDA 13 ONNX Runtime package, put the CUDA
//! and cuDNN runtime libraries beside it, and record what was built.
//!
//! Run it from a shell that has the MSVC tools on `PATH` (an "x64 Native
//! Tools" prompt, or `Enter-VsDevShell`), together with `cmake`, `ninja`,
//! `git`, `curl`, `tar` and `dumpbin`.
//!
//! Revert: delete the directory given by `--out-dir` (default
//! `native/target/sherpa-gpu`); nothing outside it is written.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::Error;
use crate::options::Options;
use crate::record::{self, BuildRecord};
use crate::tools;

const SHERPA_REPOSITORY: &str = "https://github.com/k2-fsa/sherpa-onnx";
const ORT_RELEASES: &str = "https://github.com/microsoft/onnxruntime/releases/download";
/// Parallelism of the C++ build; the host caps build parallelism at two.
const BUILD_JOBS: &str = "2";
const PROVIDER_LIBRARY: &str = "onnxruntime_providers_cuda.dll";

/// Lower-case name prefixes of the CUDA libraries the ONNX Runtime CUDA provider loads at run time.
const CUDA_RUNTIME_PREFIXES: [&str; 6] = [
    "cudart64_",
    "cublas64_",
    "cublaslt64_",
    "cufft64_",
    "nvrtc",
    "nvjitlink",
];
/// Lower-case name prefix of the cuDNN libraries; all of them are staged.
const CUDNN_PREFIXES: [&str; 1] = ["cudnn"];
/// One library of each of these must have been staged, or the provider cannot start.
const REQUIRED_PREFIXES: [(&str, &str); 4] = [
    ("cudart64_", "cudart64_"),
    ("cublas64_", "cublas64_"),
    ("cublaslt64_", "cublasLt64_"),
    ("cudnn64_", "cudnn64_"),
];

/// Where one run keeps everything.
struct Layout {
    downloads: PathBuf,
    ort_staging: PathBuf,
    ort: PathBuf,
    source: PathBuf,
    build: PathBuf,
    install: PathBuf,
    build_json: PathBuf,
}

impl Layout {
    fn new(out_dir: &Path, sherpa_tag: &str) -> Layout {
        let platform = out_dir.join("windows");
        Layout {
            downloads: out_dir.join("downloads"),
            ort_staging: out_dir.join("ort.part"),
            ort: out_dir.join("ort"),
            source: out_dir.join(format!("src-{sherpa_tag}")),
            build: platform.join("build"),
            install: platform.join("install"),
            build_json: platform.join("build.json"),
        }
    }

    /// The directory the Rust crate links against (`SHERPA_ONNX_LIB_DIR`).
    fn install_lib(&self) -> PathBuf {
        self.install.join("lib")
    }
}

/// An unpacked ONNX Runtime package: `include/` and `lib/` under `root`.
struct OrtPackage {
    root: PathBuf,
    zip_sha256: Option<String>,
}

impl OrtPackage {
    fn open(root: PathBuf, zip_sha256: Option<String>) -> Result<OrtPackage, Error> {
        for relative in [
            "include/onnxruntime_c_api.h",
            "lib/onnxruntime.dll",
            "lib/onnxruntime.lib",
            "VERSION_NUMBER",
        ] {
            let path = root.join(relative);
            if !path.is_file() {
                return Err(Error::Missing {
                    what: "a file of the ONNX Runtime package",
                    path,
                });
            }
        }
        Ok(OrtPackage { root, zip_sha256 })
    }

    fn version(&self) -> Result<String, Error> {
        let path = self.root.join("VERSION_NUMBER");
        let text = fs::read_to_string(&path).map_err(Error::io("read", &path))?;
        Ok(text.trim().to_owned())
    }
}

fn progress(step: &str) {
    eprintln!("xtask: {step}");
}

/// A path as text with forward slashes, which cmake accepts in every position
/// and which survives being written into its generated scripts.
fn forward(path: &Path) -> Result<String, Error> {
    let Some(text) = path.to_str() else {
        return Err(Error::Unreadable {
            what: "a path that is not UTF-8",
        });
    };
    Ok(text.replace('\\', "/"))
}

/// `<path>.part`, the sibling a file is written to before it is renamed into place.
fn sibling_part(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".part");
    PathBuf::from(name)
}

/// The cmake configure arguments: shared libraries, GPU on, and nothing that
/// would pull in audio devices, servers or speech synthesis.
fn configure_args(source: &str, build: &str, install: &str) -> Vec<String> {
    [
        "-G",
        "Ninja",
        "-DCMAKE_BUILD_TYPE=Release",
        "-DBUILD_SHARED_LIBS=ON",
        "-DSHERPA_ONNX_ENABLE_GPU=ON",
        "-DSHERPA_ONNX_ENABLE_TTS=OFF",
        "-DSHERPA_ONNX_ENABLE_PORTAUDIO=OFF",
        "-DSHERPA_ONNX_ENABLE_WEBSOCKET=OFF",
        "-DSHERPA_ONNX_ENABLE_BINARY=OFF",
        "-DSHERPA_ONNX_BUILD_C_API_EXAMPLES=OFF",
    ]
    .into_iter()
    .map(str::to_owned)
    .chain([
        String::from("-S"),
        source.to_owned(),
        String::from("-B"),
        build.to_owned(),
        format!("-DCMAKE_INSTALL_PREFIX={install}"),
    ])
    .collect()
}

/// The two variables sherpa-onnx's cmake reads to use an ONNX Runtime package
/// that is already unpacked instead of downloading its CUDA 12 default.
fn ort_environment(include: &str, lib: &str) -> [(&'static str, String); 2] {
    [
        ("SHERPA_ONNXRUNTIME_INCLUDE_DIR", include.to_owned()),
        ("SHERPA_ONNXRUNTIME_LIB_DIR", lib.to_owned()),
    ]
}

/// Whether a file name is a runtime library whose lower-case name starts with one of `prefixes`.
fn is_runtime_library(file_name: &str, prefixes: &[&str]) -> bool {
    let lower = file_name.to_ascii_lowercase();
    lower.ends_with(".dll") && prefixes.iter().any(|prefix| lower.starts_with(prefix))
}

/// Fails with the first required prefix that no staged library satisfies.
fn ensure_staged(staged: &[String]) -> Result<(), Error> {
    for (lower_prefix, shown) in REQUIRED_PREFIXES {
        let found = staged
            .iter()
            .any(|name| name.to_ascii_lowercase().starts_with(lower_prefix));
        if !found {
            return Err(Error::NotStaged(shown));
        }
    }
    Ok(())
}

fn ensure_source(options: &Options, layout: &Layout) -> Result<String, Error> {
    if !layout.source.join("CMakeLists.txt").is_file() {
        progress("cloning sherpa-onnx");
        tools::run(
            Command::new("git")
                .args(["clone", "--depth", "1", "--branch"])
                .arg(&options.sherpa_tag)
                .arg(SHERPA_REPOSITORY)
                .arg(&layout.source),
        )?;
    }
    let commit = tools::capture(
        Command::new("git")
            .arg("-C")
            .arg(&layout.source)
            .args(["rev-parse", "HEAD"]),
    )?;
    Ok(commit.trim().to_owned())
}

fn download(url: &str, destination: &Path) -> Result<(), Error> {
    let part = sibling_part(destination);
    tools::run(
        Command::new("curl")
            .args(["-L", "--fail", "-sS", "-o"])
            .arg(&part)
            .arg(url),
    )?;
    fs::rename(&part, destination).map_err(Error::io("rename", destination))
}

/// Unpacks `zip` so that `<ort>/<stem>` appears whole or not at all.
fn extract(zip: &Path, stem: &str, layout: &Layout) -> Result<(), Error> {
    if layout.ort_staging.exists() {
        fs::remove_dir_all(&layout.ort_staging).map_err(Error::io("clear", &layout.ort_staging))?;
    }
    fs::create_dir_all(&layout.ort_staging).map_err(Error::io("create", &layout.ort_staging))?;
    tools::run(
        Command::new("tar")
            .arg("-xf")
            .arg(zip)
            .arg("-C")
            .arg(&layout.ort_staging),
    )?;
    let unpacked = layout.ort_staging.join(stem);
    if !unpacked.is_dir() {
        return Err(Error::Missing {
            what: "the directory inside the ONNX Runtime package (is `tar` a zip-capable bsdtar?)",
            path: unpacked,
        });
    }
    fs::create_dir_all(&layout.ort).map_err(Error::io("create", &layout.ort))?;
    let target = layout.ort.join(stem);
    fs::rename(&unpacked, &target).map_err(Error::io("rename", &target))?;
    fs::remove_dir(&layout.ort_staging).map_err(Error::io("remove", &layout.ort_staging))
}

fn ensure_ort(options: &Options, layout: &Layout) -> Result<OrtPackage, Error> {
    if let Some(root) = &options.ort_dir {
        return OrtPackage::open(root.clone(), None);
    }
    let stem = format!("onnxruntime-win-x64-gpu_cuda13-{}", options.ort_version);
    let zip = layout.downloads.join(format!("{stem}.zip"));
    if !zip.is_file() {
        progress("downloading the ONNX Runtime CUDA 13 package");
        let url = format!("{ORT_RELEASES}/v{}/{stem}.zip", options.ort_version);
        download(&url, &zip)?;
    }
    let root = layout.ort.join(&stem);
    if !root.join("lib").is_dir() {
        progress("unpacking the ONNX Runtime package");
        extract(&zip, &stem, layout)?;
    }
    OrtPackage::open(root, Some(record::sha256_file(&zip)?))
}

fn cmake_build(layout: &Layout, ort: &OrtPackage) -> Result<(), Error> {
    let include = forward(&ort.root.join("include"))?;
    let lib = forward(&ort.root.join("lib"))?;
    let mut configure = Command::new("cmake");
    configure.args(configure_args(
        &forward(&layout.source)?,
        &forward(&layout.build)?,
        &forward(&layout.install)?,
    ));
    for (name, value) in ort_environment(&include, &lib) {
        configure.env(name, value);
    }
    progress("configuring");
    tools::run(&mut configure)?;
    progress("building");
    tools::run(
        Command::new("cmake")
            .arg("--build")
            .arg(&layout.build)
            .args(["--parallel", BUILD_JOBS]),
    )?;
    progress("installing");
    tools::run(Command::new("cmake").arg("--install").arg(&layout.build))
}

/// The runtime library directory of a CUDA or cuDNN tree: `bin/x64`, else `bin`.
fn runtime_directory(root: &Path) -> Result<PathBuf, Error> {
    [root.join("bin").join("x64"), root.join("bin")]
        .into_iter()
        .find(|directory| directory.is_dir())
        .ok_or_else(|| Error::Missing {
            what: "the runtime library directory",
            path: root.join("bin"),
        })
}

/// Copies the libraries of `source_dir` whose names start with one of `prefixes` into `destination`.
fn stage_from(
    source_dir: &Path,
    prefixes: &[&str],
    destination: &Path,
) -> Result<Vec<String>, Error> {
    let mut staged = Vec::new();
    for entry in fs::read_dir(source_dir).map_err(Error::io("list", source_dir))? {
        let entry = entry.map_err(Error::io("list", source_dir))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_runtime_library(&name, prefixes) {
            continue;
        }
        let from = entry.path();
        fs::copy(&from, destination.join(&name)).map_err(Error::io("copy", &from))?;
        staged.push(name);
    }
    Ok(staged)
}

/// Puts the CUDA and cuDNN libraries into the directory the Rust crate links
/// against; the crate's build script copies that whole directory beside the executable.
fn stage_runtime(options: &Options, layout: &Layout, ort: &OrtPackage) -> Result<(), Error> {
    progress("staging the import library and the CUDA and cuDNN runtime libraries");
    let destination = layout.install_lib();
    // The crate's build script links `onnxruntime.lib` from the same directory as the C API library.
    let import_library = ort.root.join("lib").join("onnxruntime.lib");
    fs::copy(&import_library, destination.join("onnxruntime.lib"))
        .map_err(Error::io("copy", &import_library))?;
    let mut staged = stage_from(
        &runtime_directory(&options.cuda_dir)?,
        &CUDA_RUNTIME_PREFIXES,
        &destination,
    )?;
    staged.extend(stage_from(
        &runtime_directory(&options.cudnn_dir)?,
        &CUDNN_PREFIXES,
        &destination,
    )?);
    ensure_staged(&staged)
}

fn collect_record(
    options: &Options,
    layout: &Layout,
    ort: &OrtPackage,
    sherpa_commit: String,
) -> Result<BuildRecord, Error> {
    progress("recording what was built");
    let lib = layout.install_lib();
    let provider = lib.join(PROVIDER_LIBRARY);
    let cuda = record::cuda_release(&options.cuda_dir)?;
    let (cudnn_version, cudnn_major) = record::cudnn_version(&options.cudnn_dir)?;
    let (provider_cubin, provider_ptx) = record::provider_arches(&options.cuda_dir, &provider)?;
    let gpu = record::gpu_info()?;
    let ort_package = ort
        .root
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or(Error::Unreadable {
            what: "the ONNX Runtime package name",
        })?;
    Ok(BuildRecord {
        schema_version: record::SCHEMA_VERSION,
        os: "windows",
        built_at_epoch: record::epoch_now()?,
        sherpa_onnx_tag: options.sherpa_tag.clone(),
        sherpa_onnx_commit: sherpa_commit,
        ort_version: ort.version()?,
        ort_package,
        ort_zip_sha256: ort.zip_sha256.clone(),
        cuda_version: cuda.version,
        cuda_major: cuda.major,
        nvcc_build: cuda.build,
        cudnn_version,
        cudnn_major,
        gpu_name: gpu.name,
        gpu_driver: gpu.driver,
        gpu_compute_capability: gpu.compute_capability,
        provider_cubin,
        provider_ptx,
        provider_dependents: record::provider_dependents(&provider)?,
        runtime_files: record::runtime_files(&lib)?,
    })
}

pub fn sherpa_gpu(options: &Options) -> Result<(), Error> {
    if !cfg!(windows) {
        return Err(Error::UnsupportedOs);
    }
    let layout = Layout::new(&options.out_dir, &options.sherpa_tag);
    for directory in [&layout.downloads, &layout.build] {
        fs::create_dir_all(directory).map_err(Error::io("create", directory))?;
    }
    let sherpa_commit = ensure_source(options, &layout)?;
    let ort = ensure_ort(options, &layout)?;
    cmake_build(&layout, &ort)?;
    stage_runtime(options, &layout, &ort)?;
    let record = collect_record(options, &layout, &ort, sherpa_commit)?;
    record::write(&layout.build_json, &record)?;
    progress(&format!("wrote {}", layout.build_json.display()));
    progress(&format!(
        "link the Rust crate with SHERPA_ONNX_LIB_DIR={}",
        layout.install_lib().display()
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backslashes_become_forward_slashes() -> Result<(), Error> {
        assert_eq!(forward(Path::new(r"C:\a\b"))?, "C:/a/b");
        Ok(())
    }

    #[test]
    fn the_part_sibling_keeps_the_directory_and_adds_an_extension() {
        assert_eq!(
            sibling_part(Path::new("d/x.zip")),
            PathBuf::from("d/x.zip.part")
        );
    }

    #[test]
    fn the_configure_arguments_turn_the_gpu_on_and_the_extras_off() {
        let args = configure_args("S", "B", "I");
        for wanted in [
            "-DSHERPA_ONNX_ENABLE_GPU=ON",
            "-DBUILD_SHARED_LIBS=ON",
            "-DSHERPA_ONNX_ENABLE_TTS=OFF",
            "-DCMAKE_INSTALL_PREFIX=I",
        ] {
            assert!(args.iter().any(|arg| arg == wanted), "{wanted} in {args:?}");
        }
        let at = args.iter().position(|arg| arg == "-S");
        assert_eq!(at.and_then(|at| args.get(at + 1)), Some(&String::from("S")));
    }

    #[test]
    fn the_ort_environment_names_both_variables() {
        let environment = ort_environment("inc", "lib");
        assert_eq!(
            environment[0],
            ("SHERPA_ONNXRUNTIME_INCLUDE_DIR", String::from("inc"))
        );
        assert_eq!(
            environment[1],
            ("SHERPA_ONNXRUNTIME_LIB_DIR", String::from("lib"))
        );
    }

    #[test]
    fn runtime_libraries_are_chosen_by_lower_cased_prefix() {
        let cuda = CUDA_RUNTIME_PREFIXES;
        assert!(is_runtime_library("cudart64_13.dll", &cuda));
        assert!(is_runtime_library("cublasLt64_13.dll", &cuda));
        assert!(is_runtime_library("nvrtc-builtins64_134.dll", &cuda));
        assert!(!is_runtime_library("cudart64_13.lib", &cuda));
        assert!(!is_runtime_library("npps64_13.dll", &cuda));
        assert!(!is_runtime_library("", &cuda));
    }

    #[test]
    fn staging_that_lacks_a_required_library_names_it() {
        let staged: Vec<String> = ["cudart64_13.dll", "cublas64_13.dll", "cublasLt64_13.dll"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        let said = ensure_staged(&staged);
        assert!(
            matches!(said, Err(Error::NotStaged("cudnn64_"))),
            "{said:?}"
        );
    }

    #[test]
    fn staging_that_has_every_required_library_passes() -> Result<(), Error> {
        let staged: Vec<String> = [
            "cudart64_13.dll",
            "cublas64_13.dll",
            "cublasLt64_13.dll",
            "cudnn64_9.dll",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        ensure_staged(&staged)
    }

    #[test]
    fn nothing_staged_misses_the_cuda_runtime_first() {
        let said = ensure_staged(&[]);
        assert!(
            matches!(said, Err(Error::NotStaged("cudart64_"))),
            "{said:?}"
        );
    }

    #[test]
    fn the_layout_keeps_one_platform_directory_for_build_and_install() {
        let layout = Layout::new(Path::new("out"), "v1");
        assert_eq!(layout.source, PathBuf::from("out/src-v1"));
        assert_eq!(
            layout.install_lib(),
            PathBuf::from("out/windows/install/lib")
        );
        assert_eq!(layout.build_json, PathBuf::from("out/windows/build.json"));
    }
}
