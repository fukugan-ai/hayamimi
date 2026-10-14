//! Who may read a file that holds a credential.
//!
//! A file is private when nobody but its owner (and, on Windows, the system
//! and the administrators) is named in its access list. On Windows the list
//! is read from `icacls`; elsewhere the mode bits are read.

use std::io;
use std::path::Path;

/// Principals besides the user the program runs as that may hold an entry in
/// the access list of a credential file.
const ALLOWED: [&str; 2] = ["NT AUTHORITY\\SYSTEM", "BUILTIN\\Administrators"];

/// Whether `name` is the user or one of the [`ALLOWED`] principals.
fn may_hold(name: &str, user: &str) -> bool {
    ALLOWED
        .iter()
        .chain(std::iter::once(&user))
        .any(|allowed| allowed.eq_ignore_ascii_case(name))
}

/// The principals an `icacls` listing of `path` names. The first line starts
/// with the path as it was given; every entry reads `<principal>:(<rights>)`.
fn principals<'a>(listing: &'a str, path: &str) -> Vec<&'a str> {
    let body = match listing.strip_prefix(path) {
        Some(rest) => rest,
        None => listing,
    };
    body.lines()
        .filter_map(|line| line.split_once(":("))
        .map(|(name, _)| name.trim())
        .collect()
}

/// Whether an `icacls` listing of `path` names only the user, the system and
/// the administrators. A listing that names nobody is not private: it is
/// what an error message looks like.
pub fn acl_is_private(listing: &str, path: &str, user: &str) -> bool {
    let names = principals(listing, path);
    !names.is_empty() && names.iter().all(|name| may_hold(name, user))
}

/// Whether only the owner may read the file at `path`. Any error means the
/// answer is not known, and the caller treats the file as not private.
pub fn is_private(path: &Path) -> io::Result<bool> {
    platform_is_private(path)
}

#[cfg(unix)]
fn platform_is_private(path: &Path) -> io::Result<bool> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(path)?.permissions().mode();
    Ok(mode & 0o077 == 0)
}

/// The standard output of a program that exited with success.
#[cfg(windows)]
fn capture(program: &str, args: &[&std::ffi::OsStr]) -> io::Result<String> {
    let output = std::process::Command::new(program).args(args).output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!("{program} failed")));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(windows)]
fn platform_is_private(path: &Path) -> io::Result<bool> {
    let listing = capture("icacls", &[path.as_os_str()])?;
    let user = capture("whoami", &[])?;
    Ok(acl_is_private(
        &listing,
        &path.to_string_lossy(),
        user.trim(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATH: &str = "C:\\Users\\u\\AppData\\Roaming\\hayamimi\\send.toml";
    const USER: &str = "DESKTOP\\u";

    fn listing(entries: &[&str]) -> String {
        let mut text = String::new();
        for (index, entry) in entries.iter().enumerate() {
            if index == 0 {
                text.push_str(PATH);
                text.push(' ');
            } else {
                text.push_str("                                            ");
            }
            text.push_str(entry);
            text.push('\n');
        }
        text.push_str("\nSuccessfully processed 1 files; Failed processing 0 files\n");
        text
    }

    #[test]
    fn the_user_the_system_and_the_administrators_are_private() {
        let text = listing(&[
            "DESKTOP\\u:(I)(F)",
            "NT AUTHORITY\\SYSTEM:(I)(F)",
            "BUILTIN\\Administrators:(I)(F)",
        ]);
        assert!(acl_is_private(&text, PATH, USER));
    }

    #[test]
    fn a_names_case_does_not_matter() {
        let text = listing(&["desktop\\U:(F)", "nt authority\\system:(F)"]);
        assert!(acl_is_private(&text, PATH, USER));
    }

    #[test]
    fn users_added_to_the_list_is_too_open() {
        let text = listing(&[
            "DESKTOP\\u:(I)(F)",
            "BUILTIN\\Users:(I)(RX)",
            "NT AUTHORITY\\SYSTEM:(I)(F)",
        ]);
        assert!(!acl_is_private(&text, PATH, USER));
    }

    #[test]
    fn another_user_or_everyone_is_too_open() {
        for other in ["DESKTOP\\other:(R)", "Everyone:(R)"] {
            let text = listing(&["DESKTOP\\u:(F)", other]);
            assert!(!acl_is_private(&text, PATH, USER), "{other}");
        }
    }

    #[test]
    fn a_listing_that_names_nobody_is_not_private() {
        assert!(!acl_is_private("", PATH, USER));
        assert!(!acl_is_private(
            "send.toml: Access is denied.\n",
            PATH,
            USER
        ));
    }

    #[test]
    fn a_path_with_spaces_does_not_hide_the_first_entry() {
        let path = "C:\\Users\\a b\\send.toml";
        let text = format!("{path} BUILTIN\\Users:(R)\n");
        assert!(!acl_is_private(&text, path, USER));
    }

    #[cfg(unix)]
    mod unix {
        use super::*;
        use std::os::unix::fs::PermissionsExt;

        /// A file that is removed when it goes out of scope.
        struct Made(std::path::PathBuf);

        impl Drop for Made {
            fn drop(&mut self) {
                // A file the test never made has nothing to remove.
                let _ = std::fs::remove_file(&self.0);
            }
        }

        /// A file with `mode`, in the cache directory: the work tree may be
        /// on a file system that keeps no modes, and a test of modes there
        /// says nothing, so it stops with that as its error.
        fn file_with_mode(name: &str, mode: u32) -> io::Result<Made> {
            let dir = dirs::cache_dir()
                .ok_or_else(|| io::Error::other("no cache directory"))?
                .join("hayamimi")
                .join("unit-tmp");
            std::fs::create_dir_all(&dir)?;
            let made = Made(dir.join(format!("access-{}-{name}", std::process::id())));
            std::fs::write(&made.0, b"x")?;
            std::fs::set_permissions(&made.0, std::fs::Permissions::from_mode(mode))?;
            let kept = std::fs::metadata(&made.0)?.permissions().mode() & 0o777;
            if kept != mode {
                return Err(io::Error::other(
                    "this file system does not keep file modes",
                ));
            }
            Ok(made)
        }

        #[test]
        fn a_file_only_the_owner_reads_is_private() -> io::Result<()> {
            let made = file_with_mode("private", 0o600)?;
            assert!(is_private(&made.0)?);
            Ok(())
        }

        #[test]
        fn a_file_the_group_or_others_read_is_not() -> io::Result<()> {
            for (name, mode) in [("group", 0o640), ("others", 0o604), ("all", 0o644)] {
                let made = file_with_mode(name, mode)?;
                assert!(!is_private(&made.0)?, "{name}");
            }
            Ok(())
        }

        #[test]
        fn a_file_that_is_not_there_is_an_error() {
            assert!(is_private(Path::new("/no/such/send.toml")).is_err());
        }
    }

    /// These run `icacls` and `whoami` on a Windows machine; on any other
    /// target they are compiled out, and `cargo check --target
    /// x86_64-pc-windows-gnu --tests` is what proves they build.
    #[cfg(windows)]
    mod windows {
        use super::*;
        use std::process::Command;

        /// The well-known id of the group `BUILTIN\Users`, the same on every
        /// language of Windows.
        const USERS: &str = "*S-1-5-32-545";

        struct Made(std::path::PathBuf);

        impl Drop for Made {
            fn drop(&mut self) {
                // A file the test never made has nothing to remove.
                let _ = std::fs::remove_file(&self.0);
            }
        }

        /// A file in the cache directory, which carries the ACL of the user's
        /// profile: the user, SYSTEM and Administrators only.
        fn new_file(name: &str) -> io::Result<Made> {
            let dir = dirs::cache_dir()
                .ok_or_else(|| io::Error::other("no cache directory"))?
                .join("hayamimi")
                .join("unit-tmp");
            std::fs::create_dir_all(&dir)?;
            let made = Made(dir.join(format!("access-{}-{name}", std::process::id())));
            std::fs::write(&made.0, b"x")?;
            Ok(made)
        }

        fn grant_users_read(path: &Path) -> io::Result<()> {
            let status = Command::new("icacls")
                .arg(path)
                .args(["/grant", &format!("{USERS}:R")])
                .output()?
                .status;
            if status.success() {
                Ok(())
            } else {
                Err(io::Error::other("icacls /grant failed"))
            }
        }

        #[test]
        fn a_file_in_the_profile_is_private() -> io::Result<()> {
            let made = new_file("private")?;
            assert!(platform_is_private(&made.0)?);
            Ok(())
        }

        #[test]
        fn a_file_users_were_added_to_is_not() -> io::Result<()> {
            let made = new_file("users")?;
            grant_users_read(&made.0)?;
            assert!(!platform_is_private(&made.0)?);
            Ok(())
        }

        #[test]
        fn a_file_that_is_not_there_is_an_error() {
            assert!(platform_is_private(Path::new("C:\\no\\such\\send.toml")).is_err());
        }
    }
}
