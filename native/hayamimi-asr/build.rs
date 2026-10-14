//! Link arguments of this package's tests, the same ones `hayamimi-gpu` passes
//! to its own (see its `build.rs`): a library crate cannot set them for the
//! crates that depend on it, and this package's integration tests run the
//! recognizer themselves.
//!
//! The package has no binary, and Cargo refuses `rustc-link-arg-bins` for a
//! package without one, so the plain `rustc-link-arg` form is used: it reaches
//! every linked target, the tests included.
//! `hayamimi_gpu::link_args::check_workspace` reads the Windows lines below as
//! plain text and counts that form for both kinds of target.

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    if cfg!(windows) {
        println!("cargo::rustc-link-arg=/DELAYLOAD:sherpa-onnx-c-api.dll");
        println!("cargo::rustc-link-arg=delayimp.lib");
    } else if cfg!(target_os = "linux") {
        println!("cargo::rustc-link-arg=-Wl,--disable-new-dtags,-rpath,$ORIGIN:$ORIGIN/..");
    }
}
