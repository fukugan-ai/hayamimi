//! CUDA device count through the driver API, loaded at run time so that a
//! machine without the NVIDIA driver reports an error instead of failing to start.

use std::ffi::c_uint;

use libloading::{Library, Symbol};

use crate::error::GpuError;

#[cfg(windows)]
const DRIVER_LIBRARY: &str = "nvcuda.dll";
#[cfg(not(windows))]
const DRIVER_LIBRARY: &str = "libcuda.so.1";

type CuInit = unsafe extern "C" fn(c_uint) -> i32;
type CuDeviceGetCount = unsafe extern "C" fn(*mut i32) -> i32;

fn check(call: &'static str, status: i32) -> Result<(), GpuError> {
    if status == 0 {
        return Ok(());
    }
    Err(GpuError::Cuda { call, status })
}

/// The number of CUDA devices the driver reports.
#[allow(unsafe_code)]
pub fn device_count() -> Result<u32, GpuError> {
    // SAFETY: opening the NVIDIA driver library only runs its own initialisers, which every CUDA program does.
    let library = unsafe { Library::new(DRIVER_LIBRARY) }.map_err(GpuError::DriverLibrary)?;
    // SAFETY: `cuInit` has the C signature `CUresult cuInit(unsigned int flags)` in the CUDA driver API.
    let init: Symbol<CuInit> =
        unsafe { library.get(b"cuInit\0") }.map_err(|source| GpuError::Symbol {
            name: "cuInit",
            source,
        })?;
    // SAFETY: `cuDeviceGetCount` has the C signature `CUresult cuDeviceGetCount(int *count)`.
    let get_count: Symbol<CuDeviceGetCount> = unsafe { library.get(b"cuDeviceGetCount\0") }
        .map_err(|source| GpuError::Symbol {
            name: "cuDeviceGetCount",
            source,
        })?;
    // SAFETY: flags 0 is the only value the driver API accepts for `cuInit`.
    check("cuInit", unsafe { init(0) })?;
    let mut count: i32 = 0;
    // SAFETY: `count` is a live, writable `int` for the duration of the call.
    check("cuDeviceGetCount", unsafe { get_count(&mut count) })?;
    u32::try_from(count).map_err(|_| GpuError::Overflow("the CUDA device count"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zero_status_is_success() {
        assert!(check("cuInit", 0).is_ok());
    }

    #[test]
    fn a_nonzero_status_names_the_call() {
        let said = check("cuInit", 3);
        assert!(
            matches!(
                said,
                Err(GpuError::Cuda {
                    call: "cuInit",
                    status: 3
                })
            ),
            "{said:?}"
        );
    }
}
