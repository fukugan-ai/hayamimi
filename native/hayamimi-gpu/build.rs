//! Link arguments of this package's executables.
//!
//! Windows: delay-loads the sherpa-onnx C library, so that
//! `runtime::preload_beside_executable` can load the ONNX Runtime copy beside
//! the executable before the C library's own import of `onnxruntime.dll` is
//! resolved. The arguments go to binaries and integration tests, so a test
//! process that runs recognition cannot bind to the `onnxruntime.dll` in
//! `System32` either.
//!
//! Linux: a run path of `$ORIGIN`, so that the shared libraries beside the
//! executable are found without `LD_LIBRARY_PATH`. It is written as `DT_RPATH`
//! (`--disable-new-dtags`), not `DT_RUNPATH`: the ONNX Runtime CUDA provider
//! opens `libcudnn.so` and its cuBLAS by name, and a `DT_RUNPATH` of the
//! executable does not reach libraries that other libraries load, so they came
//! from the system CUDA 13.3 or were not found. An integration test runs from
//! `target/<profile>/deps/`, one level below the libraries, so its run path also
//! has `$ORIGIN/..`.
//!
//! Every other crate that links `hayamimi-gpu` needs the same arguments in its
//! own build script, because a library crate cannot set them for its
//! dependents. `link_args::check_workspace` reads the Windows lines below as
//! plain text, so they stay literal.

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    if cfg!(windows) {
        println!("cargo::rustc-link-arg-bins=/DELAYLOAD:sherpa-onnx-c-api.dll");
        println!("cargo::rustc-link-arg-bins=delayimp.lib");
        println!("cargo::rustc-link-arg-tests=/DELAYLOAD:sherpa-onnx-c-api.dll");
        println!("cargo::rustc-link-arg-tests=delayimp.lib");
    } else if cfg!(target_os = "linux") {
        println!("cargo::rustc-link-arg-bins=-Wl,--disable-new-dtags,-rpath,$ORIGIN");
        println!("cargo::rustc-link-arg-tests=-Wl,--disable-new-dtags,-rpath,$ORIGIN:$ORIGIN/..");
    }
}
