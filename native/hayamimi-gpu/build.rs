//! Delay-loads the sherpa-onnx C library in this package's executables
//! (Windows only), so that `runtime::preload_beside_executable` can load
//! the ONNX Runtime copy beside the executable before the C library's own
//! import of `onnxruntime.dll` is resolved.
//!
//! The arguments go to this package's binaries and integration tests, so a
//! test process that runs recognition cannot bind to the `onnxruntime.dll` in
//! `System32` either. Every other crate that links `hayamimi-gpu` needs the
//! same two arguments in its own build script, because a library crate cannot
//! set them for its dependents. `link_args::check_workspace` reads the lines
//! below as plain text, so they stay literal.

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    if cfg!(windows) {
        println!("cargo::rustc-link-arg-bins=/DELAYLOAD:sherpa-onnx-c-api.dll");
        println!("cargo::rustc-link-arg-bins=delayimp.lib");
        println!("cargo::rustc-link-arg-tests=/DELAYLOAD:sherpa-onnx-c-api.dll");
        println!("cargo::rustc-link-arg-tests=delayimp.lib");
    }
}
