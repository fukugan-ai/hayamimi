//! Link arguments of this package's executables, the same ones `hayamimi-gpu`
//! passes to its own (see its `build.rs`): a library crate cannot set them for
//! the crates that depend on it. `hayamimi_gpu::link_args::check_workspace`
//! reads the Windows lines below as plain text, so they stay literal.

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
