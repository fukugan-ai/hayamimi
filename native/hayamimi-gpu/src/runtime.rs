//! The version of the ONNX Runtime library that is loaded into this process.

use std::ffi::{CStr, c_char, c_void};
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

/// `GetVersionString()` of the library `name` (for example `onnxruntime.dll`).
///
/// A library that is already loaded under that name is reused, so the answer
/// is the version of the copy the process is running.
#[allow(unsafe_code)]
pub fn onnxruntime_version(name: &str) -> Result<String, GpuError> {
    // SAFETY: opening a shared library runs its initialisers, which is the intent of loading ONNX Runtime.
    let library = unsafe { Library::new(name) }.map_err(GpuError::OnnxRuntime)?;
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
    fn a_library_that_does_not_exist_is_an_open_error() {
        let said = onnxruntime_version("hayamimi-no-such-onnxruntime.dll");
        assert!(matches!(said, Err(GpuError::OnnxRuntime(_))), "{said:?}");
    }
}
