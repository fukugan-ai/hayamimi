//! A directory on a file system that keeps file modes, for the tests of who
//! may read a file. The work tree can sit on a file system that keeps none
//! (a Windows drive seen from WSL), where every file reads as open to all, so
//! the directory is made in the user's cache directory.

#![allow(dead_code)]

use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A directory that is removed when dropped.
pub struct PrivateDir {
    path: PathBuf,
}

impl PrivateDir {
    pub fn new(label: &str) -> io::Result<PrivateDir> {
        let number = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = dirs::cache_dir()
            .ok_or_else(|| io::Error::other("no cache directory"))?
            .join("hayamimi")
            .join("test-tmp")
            .join(format!("{label}-{}-{number}", std::process::id()));
        fs::create_dir_all(&path)?;
        Ok(PrivateDir { path })
    }

    pub fn join(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }

    /// Writes `text` to `name` and gives the file `mode`. Stops with an error
    /// when the file system did not keep the mode, because a test of modes
    /// there says nothing.
    pub fn write(&self, name: &str, text: &str, mode: u32) -> io::Result<PathBuf> {
        let path = self.join(name);
        fs::write(&path, text)?;
        self.set_mode(name, mode)?;
        Ok(path)
    }

    pub fn set_mode(&self, name: &str, mode: u32) -> io::Result<()> {
        let path = self.join(name);
        fs::set_permissions(&path, fs::Permissions::from_mode(mode))?;
        let kept = fs::metadata(&path)?.permissions().mode() & 0o777;
        if kept == mode {
            Ok(())
        } else {
            Err(io::Error::other(
                "this file system does not keep file modes",
            ))
        }
    }
}

impl Drop for PrivateDir {
    fn drop(&mut self) {
        // Leftover scratch files are harmless; there is no one to tell.
        let _ = fs::remove_dir_all(&self.path);
    }
}
