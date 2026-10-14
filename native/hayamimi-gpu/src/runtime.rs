//! The version of the ONNX Runtime library that is loaded into this process.

use std::ffi::{CStr, c_char, c_void};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use libloading::{Library, Symbol};

use crate::error::GpuError;

/// The leading fields of `OrtApiBase` in `onnxruntime_c_api.h`.
#[repr(C)]
struct OrtApiBase {
    _get_api: unsafe extern "C" fn(u32) -> *const c_void,
    get_version_string: unsafe extern "C" fn() -> *const c_char,
}

type OrtGetApiBase = unsafe extern "C" fn() -> *const OrtApiBase;

/// The ONNX Runtime copy loaded by [`preload_beside_executable`]; it stays loaded for the whole process.
static PRELOADED: OnceLock<Library> = OnceLock::new();

/// Loads `name` from the directory of the running executable, by full path.
///
/// Windows ships its own `onnxruntime.dll` (a 1.17 build for Windows ML) in
/// `System32`, and an import of the bare name `onnxruntime.dll` can bind to it
/// instead of the copy beside the executable. A module that is already loaded
/// answers every later import of its name, so this must run before the first
/// call into the sherpa-onnx C library, which the binaries delay-load for that
/// purpose (see `build.rs`).
#[allow(unsafe_code)]
pub fn preload_beside_executable(name: &str) -> Result<(), GpuError> {
    let exe = std::env::current_exe().map_err(GpuError::Io)?;
    let Some(directory) = exe.parent() else {
        return Err(GpuError::NoExecutableDirectory);
    };
    // SAFETY: opening a shared library runs its initialisers, which is the intent of loading ONNX Runtime.
    let library = unsafe { Library::new(directory.join(name)) }.map_err(GpuError::OnnxRuntime)?;
    // A second call finds the slot taken; the extra handle is released by dropping it.
    let _ = PRELOADED.set(library);
    Ok(())
}

/// The directory that holds `name`: the executable's own directory, or the
/// one above it. A test executable runs from `target/<profile>/deps/`, one
/// level below the libraries the build copies into `target/<profile>/`.
fn directory_with(exe: &Path, name: &str) -> Result<PathBuf, GpuError> {
    let Some(directory) = exe.parent() else {
        return Err(GpuError::NoExecutableDirectory);
    };
    if directory.join(name).is_file() {
        return Ok(directory.to_path_buf());
    }
    match directory.parent() {
        Some(above) if above.join(name).is_file() => Ok(above.to_path_buf()),
        _ => Err(GpuError::NoExecutableDirectory),
    }
}

/// Whether `name` is beside the executable (or in the directory above it),
/// without loading it. A delay-loaded library that is absent ends the process
/// at its first call, so a caller that can go on without it asks first.
pub fn require_near_executable(name: &str) -> Result<(), GpuError> {
    let exe = std::env::current_exe().map_err(GpuError::Io)?;
    directory_with(&exe, name).map(|_| ())
}

/// Like [`preload_beside_executable`], for a process that may be a test
/// executable: the library is taken from the executable's directory, or from
/// the directory above it when it is not there.
#[allow(unsafe_code)]
pub fn preload_near_executable(name: &str) -> Result<(), GpuError> {
    let exe = std::env::current_exe().map_err(GpuError::Io)?;
    let directory = directory_with(&exe, name)?;
    // SAFETY: opening a shared library runs its initialisers, which is the intent of loading ONNX Runtime.
    let library = unsafe { Library::new(directory.join(name)) }.map_err(GpuError::OnnxRuntime)?;
    // A second call finds the slot taken; the extra handle is released by dropping it.
    let _ = PRELOADED.set(library);
    Ok(())
}

/// `GetVersionString()` of the library `name` (for example `onnxruntime.dll`).
///
/// The copy loaded by [`preload_beside_executable`] answers when there is one.
/// Otherwise a library that is already loaded under that name is reused, so
/// the answer is the version of the copy the process is running.
#[allow(unsafe_code)]
pub fn onnxruntime_version(name: &str) -> Result<String, GpuError> {
    if let Some(library) = PRELOADED.get() {
        return version_of(library);
    }
    // SAFETY: opening a shared library runs its initialisers, which is the intent of loading ONNX Runtime.
    let library = unsafe { Library::new(name) }.map_err(GpuError::OnnxRuntime)?;
    version_of(&library)
}

#[allow(unsafe_code)]
fn version_of(library: &Library) -> Result<String, GpuError> {
    // SAFETY: `OrtGetApiBase` has the C signature `const OrtApiBase* OrtGetApiBase(void)`.
    let get_base: Symbol<OrtGetApiBase> =
        unsafe { library.get(b"OrtGetApiBase\0") }.map_err(|source| GpuError::Symbol {
            name: "OrtGetApiBase",
            source,
        })?;
    // SAFETY: the call takes no argument and returns a pointer to a static table.
    let base = unsafe { get_base() };
    if base.is_null() {
        return Err(GpuError::VersionString);
    }
    // SAFETY: `base` is non-null and points to a static `OrtApiBase` whose two leading fields have this layout.
    let get_version = unsafe { (*base).get_version_string };
    // SAFETY: `GetVersionString` takes no argument and returns a NUL-terminated static string.
    let text = unsafe { get_version() };
    if text.is_null() {
        return Err(GpuError::VersionString);
    }
    // SAFETY: `text` is non-null and NUL-terminated, and stays valid while the library is loaded.
    let version = unsafe { CStr::from_ptr(text) }
        .to_str()
        .map_err(|_| GpuError::VersionString)?
        .to_owned();
    Ok(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preloading_a_library_that_is_not_beside_the_executable_is_an_open_error() {
        let said = preload_beside_executable("hayamimi-no-such-onnxruntime.dll");
        assert!(matches!(said, Err(GpuError::OnnxRuntime(_))), "{said:?}");
    }

    #[test]
    fn a_library_beside_neither_the_executable_nor_the_directory_above_is_refused() {
        let said = directory_with(Path::new("/no/such/dir/exe"), "hayamimi-no-such.dll");
        assert!(
            matches!(said, Err(GpuError::NoExecutableDirectory)),
            "{said:?}"
        );
    }

    /// This test executable's own file name and directory stand for the library and the directory
    /// that holds it.
    fn own_file_and_directory() -> Result<(PathBuf, String, PathBuf), GpuError> {
        let exe = std::env::current_exe().map_err(GpuError::Io)?;
        let Some(here) = exe.parent().map(Path::to_path_buf) else {
            return Err(GpuError::NoExecutableDirectory);
        };
        let Some(own) = exe.file_name().and_then(|name| name.to_str()) else {
            return Err(GpuError::NoExecutableDirectory);
        };
        Ok((exe.clone(), own.to_owned(), here))
    }

    #[test]
    fn a_library_beside_the_executable_is_found_in_its_directory() -> Result<(), GpuError> {
        let (exe, own, here) = own_file_and_directory()?;
        assert_eq!(directory_with(&exe, &own)?, here);
        Ok(())
    }

    #[test]
    fn a_test_executable_finds_the_library_one_directory_up() -> Result<(), GpuError> {
        let (_, own, here) = own_file_and_directory()?;
        // An executable in a subdirectory that holds no such file reaches the directory above.
        let below = here.join("no-such-subdirectory").join("exe");
        assert_eq!(directory_with(&below, &own)?, here);
        Ok(())
    }

    #[test]
    fn requiring_a_library_that_is_not_there_is_an_error_and_loads_nothing() {
        let said = require_near_executable("hayamimi-no-such.dll");
        assert!(
            matches!(said, Err(GpuError::NoExecutableDirectory)),
            "{said:?}"
        );
    }

    #[test]
    fn requiring_a_file_beside_the_executable_succeeds() -> Result<(), GpuError> {
        let (_, own, _) = own_file_and_directory()?;
        require_near_executable(&own)
    }

    #[test]
    fn a_library_that_does_not_exist_is_an_open_error() {
        let said = onnxruntime_version("hayamimi-no-such-onnxruntime.dll");
        assert!(matches!(said, Err(GpuError::OnnxRuntime(_))), "{said:?}");
    }
}
