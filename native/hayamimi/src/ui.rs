//! The files of the Dioxus pages. `web/index.html` of `hayamimi-ui` is
//! compiled in and answers `/`, `/dashboard` and `/transcript`; the output of
//! wasm-bindgen is read from the `ui` directory next to the executable and
//! answers `/ui/*`.

use std::path::{Component, Path, PathBuf};

use axum::extract::Path as UrlPath;
use axum::http::StatusCode;
use axum::http::header::CONTENT_TYPE;
use axum::response::{IntoResponse, Response};

/// The shell of the three pages.
pub const INDEX: &str = include_str!("../../hayamimi-ui/web/index.html");

/// The name of the directory, next to the executable, that holds the bundle.
const BUNDLE_DIR: &str = "ui";

/// The shell page; the page that is drawn is chosen by the path in the browser.
pub async fn index() -> Response {
    ([(CONTENT_TYPE, "text/html; charset=utf-8")], INDEX).into_response()
}

/// A file of the bundle, or a 404 when the name is not a file inside it.
pub async fn file(UrlPath(requested): UrlPath<String>) -> Response {
    let Some(path) = bundle_dir().and_then(|dir| resolve(&dir, &requested)) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match std::fs::read(&path) {
        Ok(bytes) => ([(CONTENT_TYPE, content_type(&path))], bytes).into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

/// The directory of the bundle, next to the executable.
pub fn bundle_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.join(BUNDLE_DIR))
}

/// The file `requested` names inside `dir`: every part of the name must be a
/// plain name (no `..`, no root, no drive), and the result must be a file.
fn resolve(dir: &Path, requested: &str) -> Option<PathBuf> {
    let relative = Path::new(requested);
    let plain = relative
        .components()
        .all(|part| matches!(part, Component::Normal(_)));
    let path = dir.join(relative);
    (plain && path.is_file()).then_some(path)
}

/// The type the browser needs: a wasm file must be `application/wasm` to be
/// compiled while it downloads.
fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("js") => "text/javascript",
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CRATE_DIR: &str = env!("CARGO_MANIFEST_DIR");

    #[test]
    fn a_file_inside_the_directory_is_found() {
        assert!(resolve(Path::new(CRATE_DIR), "Cargo.toml").is_some());
        assert!(resolve(Path::new(CRATE_DIR), "src/ui.rs").is_some());
    }

    #[test]
    fn a_missing_name_a_directory_and_an_empty_name_are_not_files() {
        for name in ["no-such-file.js", "src", ""] {
            assert_eq!(resolve(Path::new(CRATE_DIR), name), None, "{name:?}");
        }
    }

    #[test]
    fn a_name_that_leaves_the_directory_is_refused_though_the_file_exists() {
        // The workspace manifest is one level up and /etc/hosts is absolute: both exist.
        assert!(Path::new(CRATE_DIR).join("../Cargo.toml").is_file());
        for name in ["../Cargo.toml", "src/../Cargo.toml", "./Cargo.toml"] {
            assert_eq!(resolve(Path::new(CRATE_DIR), name), None, "{name:?}");
        }
        #[cfg(unix)]
        {
            assert!(Path::new("/etc/hosts").is_file());
            assert_eq!(resolve(Path::new(CRATE_DIR), "/etc/hosts"), None);
        }
    }

    #[test]
    fn the_types_of_the_bundle_files_are_the_ones_a_browser_needs() {
        assert_eq!(content_type(Path::new("a.wasm")), "application/wasm");
        assert_eq!(content_type(Path::new("s/a.js")), "text/javascript");
        assert_eq!(content_type(Path::new("a")), "application/octet-stream");
    }

    #[test]
    fn the_index_loads_the_bundle_from_ui() {
        assert!(INDEX.contains("\"/ui/hayamimi-ui.js\""));
        assert!(INDEX.contains("\"/ui/hayamimi-ui_bg.wasm\""));
    }
}
