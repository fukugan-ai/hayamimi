//! GPU memory in use, read through NVML, the library `nvidia-smi` reads. The
//! figure is the `memory.used` of the whole device, every process included.
//!
//! The library is opened at run time so that a machine without the NVIDIA
//! management library reports an error instead of failing to start.

use std::ffi::{c_int, c_uint, c_void};

use libloading::{Library, Symbol};

use crate::error::GpuError;

#[cfg(windows)]
const NVML_LIBRARY: &str = "nvml.dll";
#[cfg(not(windows))]
const NVML_LIBRARY: &str = "libnvidia-ml.so.1";

/// The device whose memory is read; the design has one GPU.
const DEVICE_INDEX: c_uint = 0;
const BYTES_PER_MIB: u64 = 1024 * 1024;

type Init = unsafe extern "C" fn() -> c_int;
type Shutdown = unsafe extern "C" fn() -> c_int;
type HandleByIndex = unsafe extern "C" fn(c_uint, *mut *mut c_void) -> c_int;
type GetMemoryInfo = unsafe extern "C" fn(*mut c_void, *mut NvmlMemory) -> c_int;

/// `nvmlMemory_t` of the NVML header, in bytes.
#[repr(C)]
#[derive(Default)]
struct NvmlMemory {
    total: u64,
    free: u64,
    used: u64,
}

fn check(call: &'static str, status: c_int) -> Result<(), GpuError> {
    if status == 0 {
        return Ok(());
    }
    Err(GpuError::Cuda { call, status })
}

/// A reader of the GPU's memory in use. Dropping it shuts NVML down.
pub struct MemoryMeter {
    library: Library,
    /// The `nvmlDevice_t` handle, which is an opaque pointer.
    device: usize,
}

/// Whole MiB in a byte count.
pub fn mib_of(bytes: u64) -> u64 {
    bytes / BYTES_PER_MIB
}

#[allow(unsafe_code)]
fn symbol<'a, T>(library: &'a Library, name: &'static str) -> Result<Symbol<'a, T>, GpuError> {
    let mut terminated = name.as_bytes().to_vec();
    terminated.push(0);
    // SAFETY: the caller names the C signature of `name` in `T`; NVML exports every name used here with it.
    unsafe { library.get(terminated.as_slice()) }
        .map_err(|source| GpuError::Symbol { name, source })
}

impl MemoryMeter {
    /// Opens NVML and takes the handle of device 0.
    #[allow(unsafe_code)]
    pub fn open() -> Result<MemoryMeter, GpuError> {
        // SAFETY: opening the NVIDIA management library only runs its own initialisers, which every NVML client does.
        let library = unsafe { Library::new(NVML_LIBRARY) }.map_err(GpuError::DriverLibrary)?;
        let init: Symbol<Init> = symbol(&library, "nvmlInit_v2")?;
        let handle: Symbol<HandleByIndex> = symbol(&library, "nvmlDeviceGetHandleByIndex_v2")?;
        // SAFETY: `nvmlInit_v2` takes no argument and may be called more than once; each call is paired with a shutdown.
        check("nvmlInit_v2", unsafe { init() })?;
        let mut device: *mut c_void = std::ptr::null_mut();
        // SAFETY: `device` is a live, writable pointer slot for the duration of the call.
        let found = unsafe { handle(DEVICE_INDEX, &mut device) };
        if let Err(error) = check("nvmlDeviceGetHandleByIndex_v2", found) {
            Self::shut_down(&library);
            return Err(error);
        }
        Ok(MemoryMeter {
            library,
            device: device as usize,
        })
    }

    #[allow(unsafe_code)]
    fn shut_down(library: &Library) {
        if let Ok(shutdown) = symbol::<Shutdown>(library, "nvmlShutdown") {
            // SAFETY: `nvmlShutdown` takes no argument and balances one `nvmlInit_v2`; the handle is not used after it.
            let _ = unsafe { shutdown() };
        }
    }

    /// The memory in use on the device, in MiB.
    #[allow(unsafe_code)]
    pub fn used_mib(&self) -> Result<u64, GpuError> {
        let info: Symbol<GetMemoryInfo> = symbol(&self.library, "nvmlDeviceGetMemoryInfo")?;
        let mut memory = NvmlMemory::default();
        // SAFETY: the handle came from `nvmlDeviceGetHandleByIndex_v2` and NVML is still initialised; `memory` is a live, writable `nvmlMemory_t`.
        check("nvmlDeviceGetMemoryInfo", unsafe {
            info(self.device as *mut c_void, &mut memory)
        })?;
        Ok(mib_of(memory.used))
    }
}

impl Drop for MemoryMeter {
    fn drop(&mut self) {
        Self::shut_down(&self.library);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_become_whole_mebibytes() {
        assert_eq!(mib_of(0), 0);
        assert_eq!(mib_of(1024 * 1024 - 1), 0);
        assert_eq!(mib_of(1024 * 1024), 1);
        assert_eq!(mib_of(1199 * 1024 * 1024 + 5), 1199);
    }

    #[test]
    fn a_nonzero_status_names_the_call() {
        let said = check("nvmlInit_v2", 9);
        assert!(
            matches!(
                said,
                Err(GpuError::Cuda {
                    call: "nvmlInit_v2",
                    status: 9
                })
            ),
            "{said:?}"
        );
        assert!(check("nvmlInit_v2", 0).is_ok());
    }

    #[test]
    fn the_memory_struct_is_three_u64_as_in_the_header() {
        assert_eq!(std::mem::size_of::<NvmlMemory>(), 24);
    }

    /// On a machine with the NVIDIA driver the reading is a plausible figure;
    /// on one without it the open is refused, which is the other road.
    #[test]
    fn a_reading_is_a_figure_or_the_open_is_refused() {
        match MemoryMeter::open() {
            Ok(meter) => {
                let used = meter.used_mib();
                assert!(matches!(used, Ok(figure) if figure < 1_000_000), "{used:?}");
            }
            Err(error) => {
                assert!(matches!(
                    error,
                    GpuError::DriverLibrary(_) | GpuError::Cuda { .. } | GpuError::Symbol { .. }
                ));
            }
        }
    }
}
