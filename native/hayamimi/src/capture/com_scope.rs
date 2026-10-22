//! The COM initialisation of a capture thread, apart from the Windows code so
//! that the rule for each result of `CoInitializeEx` runs on every platform.
//!
//! Only two results are a success: `S_OK` (this call initialised COM) and
//! `S_FALSE` (COM was already initialised on the thread in the same mode, and
//! the call is counted). Each of them is paired with one `CoUninitialize`.
//! `RPC_E_CHANGED_MODE` means the thread already lives in another apartment
//! mode: the capture runs in it and nothing is undone, because this call
//! initialised nothing. Every other value is a failure that names the
//! endpoint and the value.

use super::CaptureError;

/// COM was initialised by this call.
const S_OK: i32 = 0;
/// COM was already initialised on this thread in the same mode.
const S_FALSE: i32 = 1;
/// The thread was initialised in a different apartment mode (0x80010106).
const RPC_E_CHANGED_MODE: i32 = i32::from_ne_bytes(0x8001_0106_u32.to_ne_bytes());

/// What a call of `CoInitializeEx` left the thread with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Apartment {
    /// The call counts as an initialisation: one `CoUninitialize` is owed.
    Owned,
    /// The thread was already initialised in another mode: nothing is owed.
    Existing,
}

/// The apartment a result of `CoInitializeEx` leaves, or the failure it is.
/// `endpoint` names the source whose thread was initialised.
pub fn classify(hresult: i32, endpoint: &str) -> Result<Apartment, CaptureError> {
    match hresult {
        S_OK | S_FALSE => Ok(Apartment::Owned),
        RPC_E_CHANGED_MODE => Ok(Apartment::Existing),
        other => Err(CaptureError::ComInit {
            endpoint: endpoint.to_owned(),
            hresult: other,
        }),
    }
}

/// Holds the COM initialisation of the current thread. Dropping it runs the
/// cleanup once when one is owed, and never when the thread was already
/// initialised in another mode.
#[must_use = "dropping the scope ends the COM initialisation it holds"]
pub struct ComScope<F: FnMut()> {
    cleanup: F,
    owed: bool,
}

impl<F: FnMut()> ComScope<F> {
    /// Takes the result of `CoInitializeEx` of this thread. `cleanup` is the
    /// call that pairs it (`CoUninitialize`). A failure is returned before any
    /// cleanup is held, so the caller does not go on without COM.
    pub fn enter(hresult: i32, endpoint: &str, cleanup: F) -> Result<Self, CaptureError> {
        let apartment = classify(hresult, endpoint)?;
        Ok(ComScope {
            cleanup,
            owed: apartment == Apartment::Owned,
        })
    }
}

impl<F: FnMut()> Drop for ComScope<F> {
    fn drop(&mut self) {
        if self.owed {
            self.owed = false;
            (self.cleanup)();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// An HRESULT from the bits Microsoft's documentation prints.
    fn hr(bits: u32) -> i32 {
        i32::from_ne_bytes(bits.to_ne_bytes())
    }

    /// The text of the error of a classification, if it is one.
    fn said_by(result: Result<Apartment, CaptureError>) -> Option<String> {
        result.err().map(|error| error.to_string())
    }

    /// The failures the scope reports, as the endpoint and the value they carry.
    fn failure(result: Result<Apartment, CaptureError>) -> Option<(String, i32)> {
        match result {
            Err(CaptureError::ComInit { endpoint, hresult }) => Some((endpoint, hresult)),
            _ => None,
        }
    }

    /// Enters a scope on `hresult`, reads the cleanup count while it is held
    /// and after it is dropped.
    fn counts_for(hresult: i32) -> Result<(u32, u32), CaptureError> {
        let calls = Cell::new(0_u32);
        let scope = ComScope::enter(hresult, "mic-probe", || calls.set(calls.get() + 1))?;
        let while_held = calls.get();
        drop(scope);
        Ok((while_held, calls.get()))
    }

    /// Enters a scope on `hresult` and runs a body that only says it ran.
    fn body_ran_in_scope(hresult: i32, calls: &Cell<u32>) -> Result<bool, CaptureError> {
        let _scope = ComScope::enter(hresult, "mic-probe", || calls.set(calls.get() + 1))?;
        Ok(true)
    }

    fn failing_body(calls: &Cell<u32>) -> Result<(), CaptureError> {
        let _scope = ComScope::enter(hr(0), "mic-probe", || calls.set(calls.get() + 1))?;
        Err(CaptureError::Device(String::from("body failed")))
    }

    #[test]
    fn windows_com_scope_classifies_s_ok_as_needing_cleanup() {
        assert_eq!(classify(hr(0), "mic-probe"), Ok(Apartment::Owned));
    }

    #[test]
    fn windows_com_scope_classifies_s_false_as_needing_cleanup() {
        assert_eq!(classify(hr(1), "mic-probe"), Ok(Apartment::Owned));
    }

    #[test]
    fn windows_com_scope_classifies_changed_mode_as_an_existing_apartment() {
        assert_eq!(
            classify(hr(0x8001_0106), "mic-probe"),
            Ok(Apartment::Existing)
        );
    }

    #[test]
    fn windows_com_scope_classifies_other_failures_as_errors() {
        for bits in [0x8000_4005, 0x8007_0057, 0x8007_000E, 0x8000_FFFF] {
            let found = failure(classify(hr(bits), "mic-probe"));
            assert_eq!(
                found,
                Some((String::from("mic-probe"), hr(bits))),
                "{bits:#x}"
            );
        }
    }

    #[test]
    fn windows_com_scope_values_next_to_the_named_ones_are_errors() {
        for value in [2, -1, hr(0x8001_0105), hr(0x8001_0107)] {
            assert!(
                failure(classify(value, "mic-probe")).is_some(),
                "{value} was not an error"
            );
        }
    }

    #[test]
    fn windows_com_scope_error_carries_the_endpoint_name_it_was_given() {
        // (value, name, the name not given, its hex, the hex not given)
        let cases = [
            (
                hr(0x8000_4005),
                "mic-probe",
                "speaker-probe",
                "0x80004005",
                "0x8007000E",
            ),
            (
                hr(0x8000_4005),
                "speaker-probe",
                "mic-probe",
                "0x80004005",
                "0x8007000E",
            ),
            (
                hr(0x8007_000E),
                "mic-probe",
                "speaker-probe",
                "0x8007000E",
                "0x80004005",
            ),
        ];
        for (value, name, other_name, hex, other_hex) in cases {
            let said = said_by(classify(value, name));
            let carries = said.as_deref().is_some_and(|text| {
                text.contains(name)
                    && text.contains(hex)
                    && !text.contains(other_name)
                    && !text.contains(other_hex)
            });
            assert!(carries, "{said:?}");
        }
    }

    #[test]
    fn windows_com_scope_balances_every_success_result() -> Result<(), CaptureError> {
        assert_eq!(counts_for(hr(0))?, (0, 1));
        assert_eq!(counts_for(hr(1))?, (0, 1));
        Ok(())
    }

    #[test]
    fn windows_com_scope_uses_existing_changed_mode_apartment() -> Result<(), CaptureError> {
        let calls = Cell::new(0_u32);
        assert!(body_ran_in_scope(hr(0x8001_0106), &calls)?);
        assert_eq!(calls.get(), 0);
        Ok(())
    }

    #[test]
    fn windows_com_scope_rejects_other_hresult_failures() {
        let calls = Cell::new(0_u32);
        // The body never runs: the scope is refused before it.
        let ran = body_ran_in_scope(hr(0x8000_4005), &calls);
        assert!(matches!(ran, Err(CaptureError::ComInit { .. })));
        assert_eq!(calls.get(), 0);
    }

    #[test]
    fn windows_com_scope_cleans_up_once_when_the_body_fails() {
        let calls = Cell::new(0_u32);
        let result = failing_body(&calls);
        assert_eq!(
            result,
            Err(CaptureError::Device(String::from("body failed")))
        );
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn speaker_capture_failure_is_reported_with_endpoint_context() {
        let said = said_by(classify(hr(0x8000_4005), "speaker"));
        let carries = said
            .as_deref()
            .is_some_and(|text| text.contains("speaker") && text.contains("0x80004005"));
        assert!(carries, "{said:?}");
    }
}
