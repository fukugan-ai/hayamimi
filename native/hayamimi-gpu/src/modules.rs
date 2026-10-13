//! The modules (DLLs) that are loaded into this process right now.
//!
//! Which CUDA runtime, cuDNN and ONNX Runtime libraries a session pulled in is
//! only visible here: the import tables of the libraries name just the ones
//! they link at build time, and the ONNX Runtime CUDA provider loads the rest
//! by name while it starts.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::GpuError;

/// Where a module was loaded from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// The directory of the running executable.
    ExeDirectory,
    /// The Windows system directory (`System32` or `SysWOW64`).
    System,
    /// Any other directory, for example one on `PATH`.
    Elsewhere,
}

/// One loaded module: its lower-case file name and where it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoadedModule {
    pub name: String,
    pub origin: Origin,
}

/// Whether a module name is `library`, or `library` followed by a version
/// suffix (`libcublas.so.13` is the mapped file `libcublas.so.13.4.1.1`),
/// ignoring ASCII case.
pub fn is_named(name: &str, library: &str) -> bool {
    let Some(head) = name.get(..library.len()) else {
        return false;
    };
    head.eq_ignore_ascii_case(library)
        && name[library.len()..]
            .chars()
            .next()
            .is_none_or(|next| next == '.')
}

/// The file name of a path, whichever separator it uses.
pub fn file_name_of(path: &str) -> &str {
    path.rsplit_once(['\\', '/']).map_or(path, |(_, name)| name)
}

/// The directory part of a path; none when the path has no separator.
pub fn directory_of(path: &str) -> Option<&str> {
    path.rsplit_once(['\\', '/'])
        .map(|(directory, _)| directory)
}

fn normalise(text: &str) -> String {
    text.replace('/', "\\").to_ascii_lowercase()
}

/// Whether `path` lives directly in `exe_directory`, ignoring case and separator style.
pub fn is_beside(path: &str, exe_directory: &str) -> bool {
    directory_of(path).is_some_and(|directory| normalise(directory) == normalise(exe_directory))
}

/// Whether `path` lives directly in a Windows system directory.
pub fn is_system(path: &str) -> bool {
    directory_of(path).is_some_and(|directory| {
        let directory = normalise(directory);
        directory.ends_with("\\windows\\system32") || directory.ends_with("\\windows\\syswow64")
    })
}

fn origin_of(path: &str, exe_directory: &str) -> Origin {
    if is_beside(path, exe_directory) {
        Origin::ExeDirectory
    } else if is_system(path) {
        Origin::System
    } else {
        Origin::Elsewhere
    }
}

fn module_row(path: &str, exe_directory: &str) -> LoadedModule {
    LoadedModule {
        name: file_name_of(path).to_ascii_lowercase(),
        origin: origin_of(path, exe_directory),
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn module_paths() -> Result<Vec<String>, GpuError> {
    use std::mem::size_of;

    use windows_sys::Win32::Foundation::HMODULE;
    use windows_sys::Win32::System::LibraryLoader::GetModuleFileNameW;
    use windows_sys::Win32::System::ProcessStatus::K32EnumProcessModules;
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    const CAPACITY: usize = 4096;
    const PATH_UNITS: usize = 1024;
    let too_wide = || GpuError::Overflow("the module table size");
    let mut handles: Vec<HMODULE> = vec![std::ptr::null_mut(); CAPACITY];
    let bytes = u32::try_from(CAPACITY * size_of::<HMODULE>()).map_err(|_| too_wide())?;
    let mut needed: u32 = 0;
    // SAFETY: `GetCurrentProcess` takes no argument, returns a pseudo handle and cannot fail.
    let process = unsafe { GetCurrentProcess() };
    // SAFETY: `handles` owns `bytes` writable bytes and `needed` is a live `u32` for the call.
    let listed =
        unsafe { K32EnumProcessModules(process, handles.as_mut_ptr(), bytes, &mut needed) };
    if listed == 0 {
        return Err(GpuError::Modules(String::from("EnumProcessModules failed")));
    }
    let count = usize::try_from(needed).map_err(|_| too_wide())? / size_of::<HMODULE>();
    if count > CAPACITY {
        return Err(GpuError::Modules(format!(
            "{count} modules do not fit a table of {CAPACITY}"
        )));
    }
    handles.truncate(count);
    let mut paths = Vec::with_capacity(count);
    for handle in handles {
        let mut buffer = [0_u16; PATH_UNITS];
        let units = u32::try_from(PATH_UNITS).map_err(|_| too_wide())?;
        // SAFETY: `buffer` has `units` writable UTF-16 units; the call writes at most that many.
        let written = unsafe { GetModuleFileNameW(handle, buffer.as_mut_ptr(), units) };
        let length = usize::try_from(written).map_err(|_| too_wide())?;
        if length == 0 || length >= PATH_UNITS {
            return Err(GpuError::Modules(String::from(
                "a module path is empty or longer than the buffer",
            )));
        }
        paths.push(String::from_utf16_lossy(&buffer[..length]));
    }
    Ok(paths)
}

/// The file paths mapped into the process, from the text of `/proc/self/maps`.
#[cfg(any(not(windows), test))]
fn paths_in_maps(maps: &str) -> Vec<String> {
    let mut paths: Vec<String> = maps
        .lines()
        .filter_map(|line| line.find('/').map(|at| line[at..].trim_end().to_owned()))
        .collect();
    paths.sort();
    paths.dedup();
    paths
}

#[cfg(not(windows))]
fn module_paths() -> Result<Vec<String>, GpuError> {
    let maps = std::fs::read_to_string("/proc/self/maps").map_err(GpuError::Io)?;
    Ok(paths_in_maps(&maps))
}

/// Every module loaded into this process, sorted by name.
pub fn loaded_modules() -> Result<Vec<LoadedModule>, GpuError> {
    let exe = std::env::current_exe().map_err(GpuError::Io)?;
    let Some(exe_directory) = exe.parent().and_then(Path::to_str) else {
        return Err(GpuError::PathNotUtf8);
    };
    let mut modules: Vec<LoadedModule> = module_paths()?
        .iter()
        .map(|path| module_row(path, exe_directory))
        .collect();
    modules.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(modules)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_matches_a_library_with_or_without_a_version_suffix() {
        assert!(is_named("libcublas.so.13", "libcublas.so.13"));
        assert!(is_named("libcublas.so.13.4.1.1", "libcublas.so.13"));
        assert!(!is_named("libcublaslt.so.13", "libcublas.so.13"));
        assert!(!is_named("libcublas.so.130", "libcublas.so.13"));
        assert!(!is_named(
            "onnxruntime_providers_cuda.dll",
            "onnxruntime.dll"
        ));
        assert!(!is_named("", "libcublas.so.13"));
        assert!(is_named("CUBLAS64_13.DLL", "cublas64_13.dll"));
    }

    #[test]
    fn mapped_files_are_listed_once_and_anonymous_maps_are_skipped() {
        let maps = "\
7f00-7f10 r-xp 00000000 08:01 11   /usr/lib/libcublas.so.13.4.1.1
7f10-7f20 r--p 00010000 08:01 11   /usr/lib/libcublas.so.13.4.1.1
7f20-7f30 rw-p 00000000 00:00 0
7ffd-7fff rw-p 00000000 00:00 0    [stack]
";
        assert_eq!(
            paths_in_maps(maps),
            vec![String::from("/usr/lib/libcublas.so.13.4.1.1")]
        );
        assert!(paths_in_maps("").is_empty());
    }

    #[test]
    fn the_file_name_follows_the_last_separator_of_either_style() {
        assert_eq!(file_name_of(r"C:\a\b\X.dll"), "X.dll");
        assert_eq!(file_name_of("/a/b/x.so"), "x.so");
        assert_eq!(file_name_of("bare.dll"), "bare.dll");
        assert_eq!(file_name_of(""), "");
    }

    #[test]
    fn a_path_without_a_separator_has_no_directory() {
        assert_eq!(directory_of("bare.dll"), None);
        assert_eq!(directory_of(r"C:\a\x.dll"), Some(r"C:\a"));
    }

    #[test]
    fn beside_ignores_case_and_separator_style() {
        assert!(is_beside(r"C:\App\bin\x.dll", "c:/app/bin"));
        assert!(!is_beside(r"C:\App\bin\sub\x.dll", r"C:\App\bin"));
        assert!(!is_beside("x.dll", r"C:\App\bin"));
    }

    #[test]
    fn a_module_row_is_lower_cased_and_marked() {
        let row = module_row(r"C:\App\bin\CudaRT64_13.dll", r"C:\App\bin");
        assert_eq!(
            row,
            LoadedModule {
                name: String::from("cudart64_13.dll"),
                origin: Origin::ExeDirectory
            }
        );
    }

    #[test]
    fn the_origin_tells_exe_directory_from_system_from_elsewhere() {
        let exe = r"C:\App\bin";
        assert_eq!(origin_of(r"C:\App\bin\a.dll", exe), Origin::ExeDirectory);
        assert_eq!(origin_of(r"C:\Windows\System32\a.dll", exe), Origin::System);
        assert_eq!(origin_of(r"c:\windows\syswow64\a.dll", exe), Origin::System);
        assert_eq!(
            origin_of(r"C:\Tools\cuda\bin\a.dll", exe),
            Origin::Elsewhere
        );
        assert_eq!(
            origin_of(r"C:\Windows\System32\drivers\a.dll", exe),
            Origin::Elsewhere
        );
    }

    #[cfg(windows)]
    #[test]
    fn the_running_process_has_kernel32_loaded() -> Result<(), GpuError> {
        let modules = loaded_modules()?;
        assert!(modules.iter().any(|row| row.name == "kernel32.dll"));
        assert!(modules.iter().all(|row| row.name != "no-such-module.dll"));
        Ok(())
    }
}
