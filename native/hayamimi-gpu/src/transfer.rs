//! Timed copies between host and device memory through the CUDA driver API.
//!
//! A stage that runs on the GPU pays at least these copies for its input and
//! its output, whatever its kernel costs. The measurement uses ordinary
//! (pageable) host memory and synchronous copies, which is how a Rust caller
//! hands a `Vec<f32>` to a library that owns the device buffer.

use std::ffi::{c_int, c_uint, c_void};
use std::time::{Duration, Instant};

use libloading::{Library, Symbol};

use crate::error::GpuError;

#[cfg(windows)]
const DRIVER_LIBRARY: &str = "nvcuda.dll";
#[cfg(not(windows))]
const DRIVER_LIBRARY: &str = "libcuda.so.1";

type CuInit = unsafe extern "C" fn(c_uint) -> i32;
type CuDeviceGet = unsafe extern "C" fn(*mut c_int, c_int) -> i32;
type CuPrimaryCtxRetain = unsafe extern "C" fn(*mut *mut c_void, c_int) -> i32;
type CuCtxSetCurrent = unsafe extern "C" fn(*mut c_void) -> i32;
type CuMemAlloc = unsafe extern "C" fn(*mut u64, usize) -> i32;
type CuMemFree = unsafe extern "C" fn(u64) -> i32;
type CuMemcpyHtoD = unsafe extern "C" fn(u64, *const c_void, usize) -> i32;
type CuMemcpyDtoH = unsafe extern "C" fn(*mut c_void, u64, usize) -> i32;
type CuProfiler = unsafe extern "C" fn() -> i32;

fn check(call: &'static str, status: i32) -> Result<(), GpuError> {
    if status == 0 {
        return Ok(());
    }
    Err(GpuError::Cuda { call, status })
}

/// The time one buffer took to go to the device and the time it took to come back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoundTrip {
    pub to_device: Duration,
    pub to_host: Duration,
}

/// The driver library with the primary context of device 0 current on the calling thread.
pub struct Transfer {
    // Keeps the function pointers below valid.
    _library: Library,
    mem_alloc: CuMemAlloc,
    mem_free: CuMemFree,
    memcpy_htod: CuMemcpyHtoD,
    memcpy_dtoh: CuMemcpyDtoH,
    profiler_start: CuProfiler,
    profiler_stop: CuProfiler,
}

/// A function of the driver API, copied out of the symbol table.
#[allow(unsafe_code)]
fn load<T: Copy>(library: &Library, name: &'static str) -> Result<T, GpuError> {
    // SAFETY: every caller names the function by the C signature of its `T` in the CUDA driver API.
    let symbol: Symbol<T> =
        unsafe { library.get(name) }.map_err(|source| GpuError::Symbol { name, source })?;
    Ok(*symbol)
}

impl Transfer {
    /// Opens the driver and makes the primary context of device 0 current.
    #[allow(unsafe_code)]
    pub fn open() -> Result<Transfer, GpuError> {
        // SAFETY: opening the NVIDIA driver library only runs its own initialisers, which every CUDA program does.
        let library = unsafe { Library::new(DRIVER_LIBRARY) }.map_err(GpuError::DriverLibrary)?;
        let init: CuInit = load(&library, "cuInit")?;
        let device_get: CuDeviceGet = load(&library, "cuDeviceGet")?;
        let retain: CuPrimaryCtxRetain = load(&library, "cuDevicePrimaryCtxRetain")?;
        let set_current: CuCtxSetCurrent = load(&library, "cuCtxSetCurrent")?;
        let transfer = Transfer {
            mem_alloc: load(&library, "cuMemAlloc_v2")?,
            mem_free: load(&library, "cuMemFree_v2")?,
            memcpy_htod: load(&library, "cuMemcpyHtoD_v2")?,
            memcpy_dtoh: load(&library, "cuMemcpyDtoH_v2")?,
            profiler_start: load(&library, "cuProfilerStart")?,
            profiler_stop: load(&library, "cuProfilerStop")?,
            _library: library,
        };
        let mut device: c_int = 0;
        let mut context: *mut c_void = std::ptr::null_mut();
        // SAFETY: flags 0 is the only value `cuInit` accepts.
        check("cuInit", unsafe { init(0) })?;
        // SAFETY: `device` is a live, writable `int` for the duration of the call.
        check("cuDeviceGet", unsafe { device_get(&mut device, 0) })?;
        // SAFETY: `context` is a live, writable pointer slot and `device` came from `cuDeviceGet`.
        check("cuDevicePrimaryCtxRetain", unsafe {
            retain(&mut context, device)
        })?;
        // SAFETY: `context` was just returned by `cuDevicePrimaryCtxRetain` and stays retained for the process.
        check("cuCtxSetCurrent", unsafe { set_current(context) })?;
        Ok(transfer)
    }

    /// Copies `up` to the device and `down.len()` bytes back, timing each copy.
    /// The device buffer is allocated and freed outside the timed regions.
    #[allow(unsafe_code)]
    pub fn round_trip(&self, up: &[u8], down: &mut [u8]) -> Result<RoundTrip, GpuError> {
        let size = up.len().max(down.len());
        let mut buffer: u64 = 0;
        // SAFETY: `buffer` is a live, writable device-pointer slot.
        check("cuMemAlloc", unsafe { (self.mem_alloc)(&mut buffer, size) })?;
        let outcome = self.copy_both_ways(buffer, up, down);
        // SAFETY: `buffer` was allocated above and is not used after this call.
        let freed = check("cuMemFree", unsafe { (self.mem_free)(buffer) });
        let trip = outcome?;
        freed?;
        Ok(trip)
    }

    #[allow(unsafe_code)]
    fn copy_both_ways(
        &self,
        buffer: u64,
        up: &[u8],
        down: &mut [u8],
    ) -> Result<RoundTrip, GpuError> {
        let started = Instant::now();
        // SAFETY: `buffer` holds at least `up.len()` bytes and `up` is a live slice of that length.
        check("cuMemcpyHtoD", unsafe {
            (self.memcpy_htod)(buffer, up.as_ptr().cast(), up.len())
        })?;
        let to_device = started.elapsed();
        let started = Instant::now();
        // SAFETY: `buffer` holds at least `down.len()` bytes and `down` is a live, writable slice of that length.
        check("cuMemcpyDtoH", unsafe {
            (self.memcpy_dtoh)(down.as_mut_ptr().cast(), buffer, down.len())
        })?;
        Ok(RoundTrip {
            to_device,
            to_host: started.elapsed(),
        })
    }

    /// Opens a capture range for a profiler that waits for `cuProfilerStart`.
    #[allow(unsafe_code)]
    pub fn profiler_start(&self) -> Result<(), GpuError> {
        // SAFETY: `cuProfilerStart` takes no arguments.
        check("cuProfilerStart", unsafe { (self.profiler_start)() })
    }

    /// Closes the capture range opened by [`Transfer::profiler_start`].
    #[allow(unsafe_code)]
    pub fn profiler_stop(&self) -> Result<(), GpuError> {
        // SAFETY: `cuProfilerStop` takes no arguments.
        check("cuProfilerStop", unsafe { (self.profiler_stop)() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zero_status_is_success() {
        assert!(check("cuMemAlloc", 0).is_ok());
    }

    #[test]
    fn a_nonzero_status_names_the_call() {
        let said = check("cuMemcpyHtoD", 1);
        assert!(
            matches!(
                said,
                Err(GpuError::Cuda {
                    call: "cuMemcpyHtoD",
                    status: 1
                })
            ),
            "{said:?}"
        );
    }

    /// Runs only where a driver exists; a machine without one reports the open error.
    #[test]
    fn a_round_trip_returns_the_bytes_it_sent() -> Result<(), GpuError> {
        let transfer = match Transfer::open() {
            Ok(transfer) => transfer,
            Err(GpuError::DriverLibrary(_)) => return Ok(()),
            Err(other) => return Err(other),
        };
        let up: Vec<u8> = (0..4096u32).map(|n| (n % 251) as u8).collect();
        let mut down = vec![0u8; up.len()];
        transfer.round_trip(&up, &mut down)?;
        assert_eq!(up, down);
        Ok(())
    }

    #[test]
    fn an_empty_buffer_is_a_driver_error_or_a_noop_not_a_panic() {
        let Ok(transfer) = Transfer::open() else {
            return;
        };
        let mut down: [u8; 0] = [];
        let outcome = transfer.round_trip(&[], &mut down);
        assert!(outcome.is_ok() || matches!(outcome, Err(GpuError::Cuda { .. })));
    }
}
