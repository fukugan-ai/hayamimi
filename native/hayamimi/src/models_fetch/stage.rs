//! The staging directory a group is written into before it is installed.
//!
//! Nothing of a group reaches its own directory until every file of it has
//! arrived: the files are written under a hidden directory beside the models,
//! and each one is renamed into place afterwards. A fetch that fails half way
//! leaves the models directory as it was, and the staging directory is removed
//! when the `Stage` goes out of use, however that happens.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use super::error::FetchError;

/// Block size of a copy from the network to a file.
const COPY_BLOCK: usize = 1 << 20;

pub struct Stage {
    dir: PathBuf,
}

impl Stage {
    /// A new, empty staging directory `<models>/.<label>.part`; one left by a run that was killed is cleared.
    pub fn create(models: &Path, label: &str) -> Result<Stage, FetchError> {
        let dir = models.join(format!(".{label}.part"));
        if dir.exists() {
            fs::remove_dir_all(&dir).map_err(|e| FetchError::io(&dir, e))?;
        }
        fs::create_dir_all(&dir).map_err(|e| FetchError::io(&dir, e))?;
        Ok(Stage { dir })
    }

    /// Writes `reader` to `name` inside the staging directory; returns the bytes written.
    pub fn write(&self, name: &str, reader: &mut dyn Read) -> Result<u64, FetchError> {
        let path = self.dir.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| FetchError::io(parent, e))?;
        }
        let mut file = fs::File::create(&path).map_err(|e| FetchError::io(&path, e))?;
        copy_blocks(reader, &mut file, &path)
    }

    /// Moves `names` into `dest`, replacing a file of the same name, and removes the staging directory.
    pub fn install(self, dest: &Path, names: &[String]) -> Result<(), FetchError> {
        for name in names {
            let target = dest.join(name);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|e| FetchError::io(parent, e))?;
            }
            fs::rename(self.dir.join(name), &target).map_err(|e| FetchError::io(&target, e))?;
        }
        Ok(())
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        // A directory that cannot be removed now is cleared by the next run's `create`.
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn copy_blocks(reader: &mut dyn Read, file: &mut fs::File, path: &Path) -> Result<u64, FetchError> {
    use std::io::Write;
    let mut block = vec![0_u8; COPY_BLOCK];
    let mut total = 0_u64;
    loop {
        let read = reader
            .read(&mut block)
            .map_err(|e| FetchError::io(path, e))?;
        if read == 0 {
            return Ok(total);
        }
        file.write_all(&block[..read])
            .map_err(|e| FetchError::io(path, e))?;
        // A read count is at most the block size, which fits in 64 bits on every target.
        total += read as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn models(name: &str) -> Result<PathBuf, std::io::Error> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../target/unit-tmp")
            .join(format!("stage-{name}-{}", std::process::id()));
        if root.exists() {
            fs::remove_dir_all(&root)?;
        }
        fs::create_dir_all(&root)?;
        Ok(root)
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn written_files_reach_the_destination_only_at_install()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = models("install")?;
        let dest = root.join("group");
        let stage = Stage::create(&root, "group")?;
        let bytes = stage.write("a.bin", &mut &b"abc"[..])?;
        assert_eq!(bytes, 3);
        assert!(!dest.join("a.bin").exists());
        stage.install(&dest, &names(&["a.bin"]))?;
        assert_eq!(fs::read(dest.join("a.bin"))?, b"abc");
        assert!(!root.join(".group.part").exists());
        fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn a_stage_dropped_without_install_leaves_nothing_behind()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = models("dropped")?;
        {
            let stage = Stage::create(&root, "group")?;
            stage.write("a.bin", &mut &b"abc"[..])?;
            assert!(root.join(".group.part").is_dir());
        }
        assert_eq!(fs::read_dir(&root)?.count(), 0);
        fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn a_leftover_staging_directory_is_cleared_when_a_new_one_is_made()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = models("leftover")?;
        fs::create_dir_all(root.join(".group.part"))?;
        fs::write(root.join(".group.part/old.bin"), b"old")?;
        let stage = Stage::create(&root, "group")?;
        assert_eq!(fs::read_dir(&stage.dir)?.count(), 0);
        drop(stage);
        fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn an_install_replaces_a_file_of_the_same_name_and_makes_sub_directories()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = models("replace")?;
        let dest = root.join("group");
        fs::create_dir_all(&dest)?;
        fs::write(dest.join("a.bin"), b"old")?;
        let stage = Stage::create(&root, "group")?;
        stage.write("a.bin", &mut &b"new"[..])?;
        stage.write("hf/b.json", &mut &b"{}"[..])?;
        stage.install(&dest, &names(&["a.bin", "hf/b.json"]))?;
        assert_eq!(fs::read(dest.join("a.bin"))?, b"new");
        assert_eq!(fs::read(dest.join("hf/b.json"))?, b"{}");
        fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn an_empty_stream_writes_an_empty_file_and_counts_zero()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = models("empty")?;
        let stage = Stage::create(&root, "group")?;
        assert_eq!(stage.write("a.bin", &mut &b""[..])?, 0);
        drop(stage);
        fs::remove_dir_all(&root)?;
        Ok(())
    }
}
