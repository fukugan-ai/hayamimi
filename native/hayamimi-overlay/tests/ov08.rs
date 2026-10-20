//! OV-8: the Linux build of `hayamimi-overlay` does not pull in a window crate.
//!
//! `native/` is built and tested on Linux in WSL, where the desktop feature of
//! Dioxus would need webkit2gtk. The window crates are therefore declared under
//! `[target.'cfg(windows)'.dependencies]`, and `cargo tree` for the Linux
//! target must list none of them.
//!
//! The count is of whole package names (the first word of a line of
//! `cargo tree --prefix none`), so `windows-sys` is not a `windows`.
//!
//! Positive control: the same command for the Windows target lists `tao` and
//! `wry`, so the count can see them. Negative control: a manifest that
//! declares the same dependencies under `[dependencies]` does list `tao` for
//! the Linux target, so a leak would be caught.
//!
//! This test runs on both hosts and needs the crates' sources, which cargo
//! fetches when it does not have them.

use std::path::Path;
use std::process::Command;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const CARGO: &str = env!("CARGO");
const MANIFEST_DIR: &str = env!("CARGO_MANIFEST_DIR");
const LINUX: &str = "x86_64-unknown-linux-gnu";
const WINDOWS: &str = "x86_64-pc-windows-msvc";
/// The crates of the window: the toolkit, the webview, its Windows binding and the Win32 binding.
const WINDOW_CRATES: [&str; 4] = ["tao", "wry", "webview2-com", "windows"];

/// The output of `cargo tree` with one package per line and no drawing characters.
fn tree(manifest: Option<&Path>, target: &str) -> Result<String, String> {
    let mut command = Command::new(CARGO);
    command
        .arg("tree")
        .args(["--target", target, "-e", "normal", "--prefix", "none"]);
    match manifest {
        Some(path) => command.arg("--manifest-path").arg(path),
        None => command
            .args(["-p", "hayamimi-overlay"])
            .current_dir(MANIFEST_DIR),
    };
    let output = command
        .output()
        .map_err(|error| format!("could not run cargo tree: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo tree for {target} failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// How many lines of `tree` are the package `name` (the first word, not a prefix of it).
fn count(tree: &str, name: &str) -> usize {
    tree.lines()
        .filter(|line| line.split_whitespace().next() == Some(name))
        .count()
}

/// A manifest whose only dependencies are those the overlay declares for
/// Windows, put under `[dependencies]` so that every target sees them.
fn leaking_manifest(dir: &Path) -> Result<std::path::PathBuf, String> {
    let real = std::fs::read_to_string(Path::new(MANIFEST_DIR).join("Cargo.toml"))
        .map_err(|error| error.to_string())?;
    let real: toml::Table = real
        .parse()
        .map_err(|error: toml::de::Error| error.to_string())?;
    let Some(gated) = real
        .get("target")
        .and_then(|targets| targets.get("cfg(windows)"))
        .and_then(|windows| windows.get("dependencies"))
        .and_then(toml::Value::as_table)
    else {
        return Err(String::from(
            "Cargo.toml declares no [target.'cfg(windows)'.dependencies] to move",
        ));
    };
    let mut package = toml::Table::new();
    package.insert(
        String::from("name"),
        toml::Value::String(String::from("ov08-leak")),
    );
    package.insert(
        String::from("version"),
        toml::Value::String(String::from("0.0.0")),
    );
    package.insert(
        String::from("edition"),
        toml::Value::String(String::from("2024")),
    );
    let mut root = toml::Table::new();
    root.insert(String::from("package"), toml::Value::Table(package));
    root.insert(
        String::from("dependencies"),
        toml::Value::Table(gated.clone()),
    );
    root.insert(
        String::from("workspace"),
        toml::Value::Table(toml::Table::new()),
    );
    let source = dir.join("src");
    std::fs::create_dir_all(&source).map_err(|error| error.to_string())?;
    std::fs::write(source.join("lib.rs"), "").map_err(|error| error.to_string())?;
    let manifest = dir.join("Cargo.toml");
    std::fs::write(&manifest, root.to_string()).map_err(|error| error.to_string())?;
    Ok(manifest)
}

#[test]
fn ov08_linux_dependency_tree_has_no_window_crates() -> TestResult {
    let linux = tree(None, LINUX)?;
    let found: Vec<(&str, usize)> = WINDOW_CRATES
        .iter()
        .map(|name| (*name, count(&linux, name)))
        .filter(|(_, listed)| *listed > 0)
        .collect();

    // Positive control: the Windows target lists the window crates, so the count can see them.
    let windows = tree(None, WINDOWS)?;
    for name in ["tao", "wry"] {
        assert!(
            count(&windows, name) >= 1,
            "{name} is missing from the tree for {WINDOWS}; the count cannot see a window crate"
        );
    }

    // Negative control: the same dependencies under [dependencies] are listed for Linux.
    let dir = tempfile::tempdir_in(concat!(env!("CARGO_MANIFEST_DIR"), "/../target"))?;
    let leaking = tree(Some(&leaking_manifest(dir.path())?), LINUX)?;
    assert!(
        count(&leaking, "tao") >= 1,
        "tao is missing from the tree of a manifest that declares it for every target"
    );

    assert!(
        found.is_empty(),
        "window crates in the tree for {LINUX}: {found:?}"
    );
    Ok(())
}
