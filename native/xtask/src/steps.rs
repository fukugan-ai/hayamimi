//! The `sherpa-gpu` task: fetch, build and install the CUDA flavour of the
//! sherpa-onnx C library against a CUDA 13 ONNX Runtime package, put the CUDA
//! and cuDNN runtime libraries beside it, and record what was built.
//!
//! On Windows, run it from a shell that has the MSVC tools on `PATH` (an "x64
//! Native Tools" prompt, or `Launch-VsDevShell.ps1`), together with `cmake`,
//! `ninja`, `git`, `curl`, `tar` and `dumpbin`. On Linux (WSL), it needs `gcc`,
//! `g++`, `cmake`, `ninja`, `git`, `curl`, `tar` and `readelf`.
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

/// What differs between the operating systems the task builds on.
struct Platform {
    /// Name of the directory under the output directory, and the `os` of `build.json`.
    name: &'static str,
    /// The operating system part of the ONNX Runtime package name.
    ort_os: &'static str,
    /// The archive extension of the ONNX Runtime package.
    ort_extension: &'static str,
    /// Files of the unpacked ONNX Runtime package that must exist.
    ort_files: &'static [&'static str],
    /// The import library to put beside the C API library, where linking needs one.
    import_library: Option<&'static str>,
    /// The CUDA provider library of ONNX Runtime.
    provider: &'static str,
    /// Where the CUDA toolkit keeps its runtime libraries, best first.
    cuda_directories: &'static [&'static str],
    /// Where cuDNN keeps its runtime libraries, best first.
    cudnn_directories: &'static [&'static str],
    /// Lower-case name prefixes of the CUDA libraries the provider loads at run time.
    cuda_prefixes: &'static [&'static str],
    /// Lower-case name prefixes of the cuDNN libraries; all of them are staged.
    cudnn_prefixes: &'static [&'static str],
    /// One library of each of these must have been staged, or the provider cannot start.
    /// Each pair is the lower-case prefix and the name shown when it is missing.
    required: &'static [(&'static str, &'static str)],
    /// Whether a file name is a shared library of this operating system.
    is_library: fn(&str) -> bool,
}

fn is_dll(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".dll")
}

fn is_shared_object(name: &str) -> bool {
    name.ends_with(".so") || name.contains(".so.")
}

const WINDOWS: Platform = Platform {
    name: "windows",
    ort_os: "win-x64",
    ort_extension: "zip",
    ort_files: &[
        "include/onnxruntime_c_api.h",
        "lib/onnxruntime.dll",
        "lib/onnxruntime.lib",
        "VERSION_NUMBER",
    ],
    import_library: Some("onnxruntime.lib"),
    provider: "onnxruntime_providers_cuda.dll",
    cuda_directories: &["bin/x64", "bin"],
    cudnn_directories: &["bin/x64", "bin"],
    cuda_prefixes: &[
        "cudart64_",
        "cublas64_",
        "cublaslt64_",
        "cufft64_",
        "nvrtc",
        "nvjitlink",
    ],
    cudnn_prefixes: &["cudnn"],
    required: &[
        ("cudart64_", "cudart64_"),
        ("cublas64_", "cublas64_"),
        ("cublaslt64_", "cublasLt64_"),
        ("cudnn64_", "cudnn64_"),
    ],
    is_library: is_dll,
};

const LINUX: Platform = Platform {
    name: "linux",
    ort_os: "linux-x64",
    ort_extension: "tgz",
    ort_files: &[
        "include/onnxruntime_c_api.h",
        "lib/libonnxruntime.so",
        "VERSION_NUMBER",
    ],
    import_library: None,
    provider: "libonnxruntime_providers_cuda.so",
    cuda_directories: &["lib64", "lib"],
    cudnn_directories: &["lib64", "lib"],
    cuda_prefixes: &[
        "libcudart.so",
        "libcublas.so",
        "libcublaslt.so",
        "libcufft.so",
        "libcurand.so",
        "libnvrtc",
        "libnvjitlink",
    ],
    cudnn_prefixes: &["libcudnn"],
    required: &[
        ("libcudart.so", "libcudart.so"),
        ("libcublas.so", "libcublas.so"),
        ("libcublaslt.so", "libcublasLt.so"),
        ("libcudnn.so", "libcudnn.so"),
    ],
    is_library: is_shared_object,
};

/// The platform this build of the task runs on.
const PLATFORM: &Platform = if cfg!(windows) { &WINDOWS } else { &LINUX };

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
    fn new(out_dir: &Path, sherpa_tag: &str, platform: &Platform) -> Layout {
        let directory = out_dir.join(platform.name);
        Layout {
            downloads: out_dir.join("downloads"),
            ort_staging: out_dir.join("ort.part"),
            ort: out_dir.join("ort"),
            source: out_dir.join(format!("src-{sherpa_tag}")),
            build: directory.join("build"),
            install: directory.join("install"),
            build_json: directory.join("build.json"),
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
    archive_sha256: Option<String>,
}

impl OrtPackage {
    fn open(
        root: PathBuf,
        archive_sha256: Option<String>,
        platform: &Platform,
    ) -> Result<OrtPackage, Error> {
        for relative in platform.ort_files {
            let path = root.join(relative);
            if !path.is_file() {
                return Err(Error::Missing {
                    what: "a file of the ONNX Runtime package",
                    path,
                });
            }
        }
        Ok(OrtPackage {
            root,
            archive_sha256,
        })
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

/// Whether a file name is a runtime library of `platform` whose lower-case name starts with one of `prefixes`.
fn is_runtime_library(platform: &Platform, file_name: &str, prefixes: &[&str]) -> bool {
    let lower = file_name.to_ascii_lowercase();
    (platform.is_library)(file_name) && prefixes.iter().any(|prefix| lower.starts_with(prefix))
}

/// Fails with the first required prefix that no staged library satisfies.
fn ensure_staged(platform: &Platform, staged: &[String]) -> Result<(), Error> {
    for &(lower_prefix, shown) in platform.required {
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

/// Unpacks `archive` so that `<ort>/<stem>` appears whole or not at all.
fn extract(archive: &Path, stem: &str, layout: &Layout) -> Result<(), Error> {
    if layout.ort_staging.exists() {
        fs::remove_dir_all(&layout.ort_staging).map_err(Error::io("clear", &layout.ort_staging))?;
    }
    fs::create_dir_all(&layout.ort_staging).map_err(Error::io("create", &layout.ort_staging))?;
    tools::run(
        Command::new("tar")
            .arg("-xf")
            .arg(archive)
            .arg("-C")
            .arg(&layout.ort_staging),
    )?;
    let unpacked = layout.ort_staging.join(stem);
    if !unpacked.is_dir() {
        return Err(Error::Missing {
            what: "the directory inside the ONNX Runtime package (is `tar` able to read the archive?)",
            path: unpacked,
        });
    }
    fs::create_dir_all(&layout.ort).map_err(Error::io("create", &layout.ort))?;
    let target = layout.ort.join(stem);
    fs::rename(&unpacked, &target).map_err(Error::io("rename", &target))?;
    fs::remove_dir(&layout.ort_staging).map_err(Error::io("remove", &layout.ort_staging))
}

/// The package name without extension, for example `onnxruntime-win-x64-gpu_cuda13-1.31.0`.
fn ort_stem(platform: &Platform, ort_version: &str) -> String {
    format!("onnxruntime-{}-gpu_cuda13-{ort_version}", platform.ort_os)
}

fn ensure_ort(
    options: &Options,
    layout: &Layout,
    platform: &Platform,
) -> Result<OrtPackage, Error> {
    if let Some(root) = &options.ort_dir {
        return OrtPackage::open(root.clone(), None, platform);
    }
    let stem = ort_stem(platform, &options.ort_version);
    let file = format!("{stem}.{}", platform.ort_extension);
    let archive = layout.downloads.join(&file);
    if !archive.is_file() {
        progress("downloading the ONNX Runtime CUDA 13 package");
        let url = format!("{ORT_RELEASES}/v{}/{file}", options.ort_version);
        download(&url, &archive)?;
    }
    let root = layout.ort.join(&stem);
    if !root.join("lib").is_dir() {
        progress("unpacking the ONNX Runtime package");
        extract(&archive, &stem, layout)?;
    }
    OrtPackage::open(root, Some(record::sha256_file(&archive)?), platform)
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

/// The first of `candidates` under `root` that is a directory.
fn runtime_directory(root: &Path, candidates: &[&str]) -> Result<PathBuf, Error> {
    candidates
        .iter()
        .map(|relative| root.join(relative))
        .find(|directory| directory.is_dir())
        .ok_or_else(|| Error::Missing {
            what: "the runtime library directory",
            path: root.to_path_buf(),
        })
}

/// Puts `from` at `to`: a copy on Windows, a link to the real file elsewhere
/// (the CUDA libraries are large and several names point at one file).
#[cfg(unix)]
fn place(from: &Path, to: &Path) -> Result<(), Error> {
    let real = fs::canonicalize(from).map_err(Error::io("resolve", from))?;
    if to.symlink_metadata().is_ok() {
        fs::remove_file(to).map_err(Error::io("remove", to))?;
    }
    std::os::unix::fs::symlink(&real, to).map_err(Error::io("link", to))
}

#[cfg(not(unix))]
fn place(from: &Path, to: &Path) -> Result<(), Error> {
    fs::copy(from, to)
        .map(drop)
        .map_err(Error::io("copy", from))
}

/// Places the libraries of `source_dir` whose names start with one of `prefixes` into `destination`.
fn stage_from(
    platform: &Platform,
    source_dir: &Path,
    prefixes: &[&str],
    destination: &Path,
) -> Result<Vec<String>, Error> {
    let mut staged = Vec::new();
    for entry in fs::read_dir(source_dir).map_err(Error::io("list", source_dir))? {
        let entry = entry.map_err(Error::io("list", source_dir))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_runtime_library(platform, &name, prefixes) {
            continue;
        }
        place(&entry.path(), &destination.join(&name))?;
        staged.push(name);
    }
    Ok(staged)
}

/// Puts the CUDA and cuDNN libraries into the directory the Rust crate links
/// against; the crate's build script copies that whole directory beside the executable.
fn stage_runtime(
    options: &Options,
    layout: &Layout,
    ort: &OrtPackage,
    platform: &Platform,
) -> Result<(), Error> {
    progress("staging the import library and the CUDA and cuDNN runtime libraries");
    let destination = layout.install_lib();
    if let Some(import_library) = platform.import_library {
        // The crate's build script links the import library from the same directory as the C API library.
        let from = ort.root.join("lib").join(import_library);
        fs::copy(&from, destination.join(import_library)).map_err(Error::io("copy", &from))?;
    }
    let mut staged = stage_from(
        platform,
        &runtime_directory(&options.cuda_dir, platform.cuda_directories)?,
        platform.cuda_prefixes,
        &destination,
    )?;
    staged.extend(stage_from(
        platform,
        &runtime_directory(&options.cudnn_dir, platform.cudnn_directories)?,
        platform.cudnn_prefixes,
        &destination,
    )?);
    ensure_staged(platform, &staged)
}

fn collect_record(
    options: &Options,
    layout: &Layout,
    ort: &OrtPackage,
    sherpa_commit: String,
    platform: &Platform,
) -> Result<BuildRecord, Error> {
    progress("recording what was built");
    let lib = layout.install_lib();
    let provider = lib.join(platform.provider);
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
        os: platform.name,
        built_at_epoch: record::epoch_now()?,
        sherpa_onnx_tag: options.sherpa_tag.clone(),
        sherpa_onnx_commit: sherpa_commit,
        ort_version: ort.version()?,
        ort_package,
        ort_zip_sha256: ort.archive_sha256.clone(),
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
        runtime_files: record::runtime_files(&lib, platform.is_library)?,
    })
}

pub fn sherpa_gpu(options: &Options) -> Result<(), Error> {
    let platform = PLATFORM;
    let layout = Layout::new(&options.out_dir, &options.sherpa_tag, platform);
    for directory in [&layout.downloads, &layout.build] {
        fs::create_dir_all(directory).map_err(Error::io("create", directory))?;
    }
    let sherpa_commit = ensure_source(options, &layout)?;
    let ort = ensure_ort(options, &layout, platform)?;
    cmake_build(&layout, &ort)?;
    stage_runtime(options, &layout, &ort, platform)?;
    let record = collect_record(options, &layout, &ort, sherpa_commit, platform)?;
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
    fn the_package_names_follow_the_release_assets() {
        assert_eq!(
            ort_stem(&WINDOWS, "1.31.0"),
            "onnxruntime-win-x64-gpu_cuda13-1.31.0"
        );
        assert_eq!(
            ort_stem(&LINUX, "1.31.0"),
            "onnxruntime-linux-x64-gpu_cuda13-1.31.0"
        );
        assert_eq!((WINDOWS.ort_extension, LINUX.ort_extension), ("zip", "tgz"));
    }

    #[test]
    fn windows_runtime_libraries_are_chosen_by_lower_cased_prefix() {
        let cuda = WINDOWS.cuda_prefixes;
        assert!(is_runtime_library(&WINDOWS, "cudart64_13.dll", cuda));
        assert!(is_runtime_library(&WINDOWS, "cublasLt64_13.dll", cuda));
        assert!(is_runtime_library(
            &WINDOWS,
            "nvrtc-builtins64_134.dll",
            cuda
        ));
        assert!(!is_runtime_library(&WINDOWS, "cudart64_13.lib", cuda));
        assert!(!is_runtime_library(&WINDOWS, "npps64_13.dll", cuda));
        assert!(!is_runtime_library(&WINDOWS, "", cuda));
    }

    #[test]
    fn linux_runtime_libraries_are_chosen_by_prefix_and_shared_object_name() {
        let cuda = LINUX.cuda_prefixes;
        assert!(is_runtime_library(&LINUX, "libcudart.so.13", cuda));
        assert!(is_runtime_library(&LINUX, "libcublasLt.so.13.7.0.27", cuda));
        assert!(is_runtime_library(&LINUX, "libcurand.so", cuda));
        assert!(!is_runtime_library(&LINUX, "libcudart_static.a", cuda));
        assert!(!is_runtime_library(&LINUX, "libcufftw.so.12", cuda));
        assert!(!is_runtime_library(&LINUX, "libnpps.so.13", cuda));
        assert!(is_runtime_library(
            &LINUX,
            "libcudnn.so.9",
            LINUX.cudnn_prefixes
        ));
    }

    #[test]
    fn staging_that_lacks_a_required_library_names_it() {
        let staged: Vec<String> = ["cudart64_13.dll", "cublas64_13.dll", "cublasLt64_13.dll"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        let said = ensure_staged(&WINDOWS, &staged);
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
        ensure_staged(&WINDOWS, &staged)?;
        let linux: Vec<String> = [
            "libcudart.so.13",
            "libcublas.so.13",
            "libcublasLt.so.13",
            "libcudnn.so.9",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        ensure_staged(&LINUX, &linux)
    }

    #[test]
    fn nothing_staged_misses_the_cuda_runtime_first() {
        let said = ensure_staged(&WINDOWS, &[]);
        assert!(
            matches!(said, Err(Error::NotStaged("cudart64_"))),
            "{said:?}"
        );
    }

    #[test]
    fn the_layout_keeps_one_platform_directory_for_build_and_install() {
        let layout = Layout::new(Path::new("out"), "v1", &WINDOWS);
        assert_eq!(layout.source, PathBuf::from("out/src-v1"));
        assert_eq!(
            layout.install_lib(),
            PathBuf::from("out/windows/install/lib")
        );
        assert_eq!(layout.build_json, PathBuf::from("out/windows/build.json"));
        let linux = Layout::new(Path::new("out"), "v1", &LINUX);
        assert_eq!(linux.build_json, PathBuf::from("out/linux/build.json"));
    }

    #[test]
    fn the_runtime_directory_is_the_first_candidate_that_exists() -> Result<(), Error> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        assert_eq!(
            runtime_directory(root, &["no-such", "src"])?,
            root.join("src")
        );
        let said = runtime_directory(root, &["no-such"]);
        assert!(matches!(said, Err(Error::Missing { .. })), "{said:?}");
        Ok(())
    }
}
