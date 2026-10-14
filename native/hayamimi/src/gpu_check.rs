//! The first of the two GPU checks, run once at the start: is there a CUDA
//! device? The second check is the building of each stage's CUDA session,
//! which `hayamimi-asr` does. A machine without a device is not stopped: the
//! stages placed on the GPU are built on the CPU instead.

use hayamimi_asr::Gpu;
use hayamimi_gpu::error::GpuError;

#[cfg(test)]
use hayamimi_asr::Reason;

/// Maps the driver's answer to the state the stages are built under; the
/// library holds the mapping so that its own stages can ask again.
pub fn classify(count: Result<u32, GpuError>) -> Gpu {
    Gpu::from_count(count)
}

/// Counts the CUDA devices through the driver API.
pub fn preflight() -> Gpu {
    classify(hayamimi_gpu::cuda::device_count())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_device_is_enough() {
        assert_eq!(classify(Ok(1)), Gpu::Try);
        assert_eq!(classify(Ok(4)), Gpu::Try);
    }

    #[test]
    fn no_device_is_the_no_device_state() {
        assert_eq!(classify(Ok(0)), Gpu::Absent(Reason::NoDevice));
    }

    #[test]
    fn a_driver_that_cannot_be_asked_is_the_driver_unreadable_state() {
        let driver_error = GpuError::Cuda {
            call: "cuInit",
            status: 999,
        };
        assert_eq!(
            classify(Err(driver_error)),
            Gpu::Absent(Reason::DriverUnreadable)
        );
    }

    #[test]
    fn a_driver_that_sees_no_device_is_the_no_device_state() {
        let hidden = GpuError::Cuda {
            call: "cuInit",
            status: 100,
        };
        assert_eq!(classify(Err(hidden)), Gpu::Absent(Reason::NoDevice));
    }

    #[test]
    fn a_driver_library_that_does_not_load_is_the_driver_unreadable_state() {
        assert_eq!(
            classify(Err(GpuError::Overflow("the CUDA device count"))),
            Gpu::Absent(Reason::DriverUnreadable)
        );
    }
}
