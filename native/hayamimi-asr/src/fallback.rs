//! What the route gave up, stage by stage, and why.
//!
//! A stage that cannot run as designed does not stop the route: it runs on
//! the CPU instead, or it is left out. Each such step is one [`Fallback`], so
//! that the caller can say it in the log, on the screen and to the user.

/// A stage that can fall back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// The punctuation model.
    Punct,
    /// The speech recognizer.
    Recognizer,
    /// The voice detector.
    Vad,
}

impl Stage {
    /// The stage name used in events.
    pub fn name(self) -> &'static str {
        match self {
            Stage::Punct => "punct",
            Stage::Recognizer => "recognizer",
            Stage::Vad => "vad",
        }
    }
}

/// Why a stage fell back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// The machine has no CUDA device.
    NoDevice,
    /// The driver or the CUDA runtime could not be loaded.
    DriverUnreadable,
    /// The CUDA session of the stage could not be built.
    SessionFailed,
    /// A model file is not there.
    Missing,
    /// A model file is there and cannot be read as a model.
    Unreadable,
}

impl Reason {
    /// The reason name used in events.
    pub fn name(self) -> &'static str {
        match self {
            Reason::NoDevice => "no_device",
            Reason::DriverUnreadable => "driver_unreadable",
            Reason::SessionFailed => "session_failed",
            Reason::Missing => "missing",
            Reason::Unreadable => "unreadable",
        }
    }
}

/// Where a stage went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum To {
    /// The same model on the CPU.
    Cpu,
    /// The stage is left out.
    Disabled,
}

impl To {
    /// The destination name used in events.
    pub fn name(self) -> &'static str {
        match self {
            To::Cpu => "cpu",
            To::Disabled => "disabled",
        }
    }
}

/// One stage that did not run as designed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fallback {
    pub stage: Stage,
    pub reason: Reason,
    pub to: To,
}

/// What the start found about the GPU, before any session is built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gpu {
    /// A device was counted: stages placed on the GPU try CUDA first.
    Try,
    /// No usable device: stages placed on the GPU are built on the CPU.
    Absent(Reason),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_names_are_the_words_of_the_events() {
        assert_eq!(Stage::Punct.name(), "punct");
        assert_eq!(Reason::DriverUnreadable.name(), "driver_unreadable");
        assert_eq!(Reason::SessionFailed.name(), "session_failed");
        assert_eq!(To::Disabled.name(), "disabled");
        assert_eq!(To::Cpu.name(), "cpu");
    }
}
