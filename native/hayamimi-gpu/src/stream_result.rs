//! The language tag SenseVoice puts on a result.
//!
//! The safe `sherpa-onnx` crate parses a result into its text, tokens and
//! timestamps and drops the rest, and keeps the pointer of the stream private.
//! The C library hands the whole result back as JSON, `lang` included, so this
//! module asks for that JSON through the raw bindings.
#![allow(unsafe_code)]

use std::ffi::CStr;

use serde_json::Value;
use sherpa_onnx::OfflineStream;
use sherpa_onnx_sys as sys;

use crate::error::GpuError;

type RawStream = *const sys::OfflineStream;

// The safe stream is one raw pointer and nothing else of any size, so the
// pointer is at offset 0 of it: a type of the pointer's size and alignment
// holding a pointer has room for nothing else. A release of the crate that
// adds a field breaks this build instead of reading the wrong bytes.
const _: () = assert!(
    size_of::<OfflineStream>() == size_of::<RawStream>()
        && align_of::<OfflineStream>() == align_of::<RawStream>()
);

/// The pointer the safe stream wraps.
fn raw_pointer(stream: &OfflineStream) -> RawStream {
    let wrapped: *const OfflineStream = stream;
    // SAFETY: `OfflineStream` is exactly one `*const sys::OfflineStream` (the const assertion
    // above), so reading one pointer from its address reads that field; `stream` is borrowed
    // and so valid and aligned for the read.
    unsafe { *wrapped.cast::<RawStream>() }
}

/// The result of the last decode on `stream`, as the JSON the C library writes.
fn result_json(stream: &OfflineStream) -> Result<String, GpuError> {
    let raw = raw_pointer(stream);
    // SAFETY: `raw` is the live stream owned by the borrowed `stream`. The call returns a
    // NUL-terminated string that this function owns until the paired destroy call, or null.
    // The text is copied out before the destroy and not used after it.
    unsafe {
        let text = sys::SherpaOnnxGetOfflineStreamResultAsJson(raw);
        if text.is_null() {
            return Err(GpuError::NoResult);
        }
        let owned = CStr::from_ptr(text).to_string_lossy().into_owned();
        sys::SherpaOnnxDestroyOfflineStreamResultJson(text);
        Ok(owned)
    }
}

/// The `lang` field of a result's JSON: `None` when the model does not
/// identify languages and so wrote none.
pub fn lang_of(json: &str) -> Result<Option<String>, GpuError> {
    let value: Value = serde_json::from_str(json).map_err(GpuError::ResultNotJson)?;
    Ok(value.get("lang").and_then(Value::as_str).map(str::to_owned))
}

/// The language tag of the result of the last decode on `stream`, as the model
/// wrote it (SenseVoice writes `<|ja|>`).
pub fn result_lang(stream: &OfflineStream) -> Result<Option<String>, GpuError> {
    lang_of(&result_json(stream)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_result_with_a_lang_gives_it() -> Result<(), GpuError> {
        let json = r#"{"text":"x","tokens":[],"lang":"<|ja|>"}"#;
        assert_eq!(lang_of(json)?, Some(String::from("<|ja|>")));
        Ok(())
    }

    #[test]
    fn a_result_without_a_lang_gives_none() -> Result<(), GpuError> {
        assert_eq!(lang_of(r#"{"text":"x"}"#)?, None);
        Ok(())
    }

    #[test]
    fn a_lang_that_is_not_text_gives_none() -> Result<(), GpuError> {
        assert_eq!(lang_of(r#"{"lang":3}"#)?, None);
        Ok(())
    }

    #[test]
    fn a_result_that_is_not_json_is_an_error() {
        assert!(matches!(lang_of("{"), Err(GpuError::ResultNotJson(_))));
    }
}
