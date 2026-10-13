//! Delay-loads the sherpa-onnx C library in this package's executables
//! (Windows only), so that `runtime::preload_beside_executable` can load
//! the ONNX Runtime copy beside the executable before the C library's own
//! import of `onnxruntime.dll` is resolved.
//!
//! Every other executable that links `hayamimi-gpu` needs the same two link
//! arguments, because a library crate cannot set them for its dependents.

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    if cfg!(windows) {
        println!("cargo::rustc-link-arg-bins=/DELAYLOAD:sherpa-onnx-c-api.dll");
        println!("cargo::rustc-link-arg-bins=delayimp.lib");
    }
}
