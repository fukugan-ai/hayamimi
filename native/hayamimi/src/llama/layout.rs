//! Where the server's files are. Under one root (the user's local data
//! directory, `hayamimi` inside it):
//!
//! ```text
//! llama/<build>/llama-server[.exe]   one directory per build, DLLs beside it
//! llama/manifest.json                the release tag and the assets, as a record
//! models/llm/<file>.gguf             the translation models
//! log/llama-server.log               what the running server prints
//! ```

use std::path::{Path, PathBuf};

use super::plan::Build;

#[cfg(windows)]
const SERVER: &str = "llama-server.exe";
#[cfg(not(windows))]
const SERVER: &str = "llama-server";

/// The directories of one install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    root: PathBuf,
}

impl Layout {
    pub fn new(root: PathBuf) -> Layout {
        Layout { root }
    }

    /// The server executable of a build.
    pub fn server(&self, build: Build) -> PathBuf {
        self.root.join("llama").join(build.name()).join(SERVER)
    }

    /// A model given by file name lives in `models/llm`; a path (a separator
    /// in it, or an absolute one) is taken as it is.
    pub fn model(&self, given: &str) -> PathBuf {
        let path = Path::new(given);
        if path.is_absolute() || path.components().count() > 1 {
            return path.to_path_buf();
        }
        self.root.join("models").join("llm").join(given)
    }

    /// The file the server's output goes to.
    pub fn log(&self) -> PathBuf {
        self.root.join("log").join("llama-server.log")
    }

    /// The release tag the install records, when the manifest can be read.
    /// The tag is information for the log and decides nothing.
    pub fn tag(&self) -> Option<String> {
        let text = std::fs::read_to_string(self.root.join("llama").join("manifest.json")).ok()?;
        let manifest: serde_json::Value = serde_json::from_str(&text).ok()?;
        manifest.get("tag")?.as_str().map(str::to_owned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> Layout {
        Layout::new(PathBuf::from("root"))
    }

    #[test]
    fn each_build_has_its_own_directory() {
        let cuda = layout().server(Build::Cuda);
        let cpu = layout().server(Build::Cpu);
        assert!(cuda.starts_with("root/llama/cuda"));
        assert!(cpu.starts_with("root/llama/cpu"));
        assert!(cuda.ends_with(SERVER) && cpu.ends_with(SERVER));
    }

    #[test]
    fn a_bare_file_name_is_a_model_of_the_install() {
        assert_eq!(
            layout().model("a.gguf"),
            Path::new("root/models/llm/a.gguf")
        );
    }

    #[test]
    fn a_path_is_taken_as_it_is() {
        assert_eq!(layout().model("sub/a.gguf"), Path::new("sub/a.gguf"));
        let absolute = Path::new(env!("CARGO_MANIFEST_DIR")).join("a.gguf");
        let given = absolute.to_string_lossy().into_owned();
        assert_eq!(layout().model(&given), absolute);
    }

    #[test]
    fn the_log_is_in_the_log_directory() {
        assert_eq!(layout().log(), Path::new("root/log/llama-server.log"));
    }

    #[test]
    fn a_missing_manifest_has_no_tag() {
        assert_eq!(layout().tag(), None);
    }

    #[test]
    fn the_tag_is_read_from_the_manifest() -> Result<(), Box<dyn std::error::Error>> {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/unit-tmp");
        let root = base.join(format!("llama-layout-{}", std::process::id()));
        std::fs::create_dir_all(root.join("llama"))?;
        std::fs::write(root.join("llama/manifest.json"), r#"{"tag":"b1"}"#)?;
        let tag = Layout::new(root.clone()).tag();
        std::fs::remove_dir_all(&root)?;
        assert_eq!(tag.as_deref(), Some("b1"));
        Ok(())
    }
}
