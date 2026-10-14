//! The eight Japanese golden clips through the language identification the
//! way the route uses it. whisper-tiny alone misnames two of them; SenseVoice
//! names all eight Japanese, and the route reads a session's first segment in
//! SenseVoice's language, so the language that is left is Japanese for all
//! eight.
//!
//! The labels are what the models answered for each clip on the CPU, with the
//! clip trimmed and capped as the route does, measured once with a probe over
//! the two whisper-tiny builds (fp32, which the route uses, and int8, which
//! the reference implementation's model files are) and SenseVoice's own tag
//! (`lang` in the library's JSON result, which the safe Rust binding drops).

use hayamimi_asr::AsrError;
use hayamimi_asr::lid::Identify;
use hayamimi_asr::route::{
    Decode, DecodeError, Decoded, Policy, Request, RouteError, Router, Tier,
};

/// One clip: its name, the two whisper-tiny labels, and SenseVoice's tag.
struct Row {
    clip: &'static str,
    whisper_fp32: &'static str,
    whisper_int8: &'static str,
    sense_voice: &'static str,
}

const fn row(
    clip: &'static str,
    whisper_fp32: &'static str,
    whisper_int8: &'static str,
    sense_voice: &'static str,
) -> Row {
    Row {
        clip,
        whisper_fp32,
        whisper_int8,
        sense_voice,
    }
}

const GOLDEN: [Row; 8] = [
    row("ja_014", "ko", "ko", "<|ja|>"),
    row("ja_024", "ja", "ja", "<|ja|>"),
    row("ja_033", "ja", "ja", "<|ja|>"),
    row("ja_036", "ja", "ja", "<|ja|>"),
    row("ja_044", "ja", "ja", "<|ja|>"),
    row("ja_079", "ja", "ja", "<|ja|>"),
    row("ja_085", "zh", "zh", "<|ja|>"),
    row("ja_090", "ja", "ja", "<|ja|>"),
];

struct Named(&'static str);

impl Identify for Named {
    fn identify(&self, _samples: &[f32]) -> Result<String, AsrError> {
        Ok(self.0.to_owned())
    }
}

/// SenseVoice answers with its tag; the Japanese model answers with Japanese text.
struct Models {
    tag: &'static str,
}

impl Decode for Models {
    fn decode(&mut self, tier: Tier, _samples: &[f32]) -> Result<Decoded, DecodeError> {
        match tier {
            Tier::Sv => Ok(Decoded {
                text: String::from("はい"),
                tag: self.tag.to_owned(),
            }),
            Tier::Rz => Ok(Decoded {
                text: String::from("はい"),
                tag: String::new(),
            }),
            _ => Err(DecodeError::Unavailable),
        }
    }

    fn split_retry(
        &mut self,
        text: &str,
        _samples: &[f32],
        _lang: &str,
        _tier: Tier,
    ) -> Result<String, DecodeError> {
        Ok(text.to_owned())
    }

    fn prefetch(&mut self, _tier: Tier) {}

    fn close(&mut self) {}
}

fn count_named_ja(label: impl Fn(&Row) -> &'static str) -> usize {
    GOLDEN.iter().filter(|row| label(row) == "ja").count()
}

#[test]
fn whisper_alone_names_six_of_the_eight_clips_japanese_in_both_builds() {
    assert_eq!(count_named_ja(|row| row.whisper_fp32), 6);
    assert_eq!(count_named_ja(|row| row.whisper_int8), 6);
    for row in &GOLDEN {
        assert_eq!(row.whisper_fp32, row.whisper_int8, "{}", row.clip);
    }
}

#[test]
fn sense_voice_names_all_eight_clips_japanese() {
    assert!(GOLDEN.iter().all(|row| row.sense_voice == "<|ja|>"));
}

#[test]
fn the_language_left_after_the_dual_confirmation_is_japanese_for_all_eight()
-> Result<(), RouteError> {
    let samples = vec![0.0_f32; 16_000 * 8];
    for row in &GOLDEN {
        let mut routed = Router::new(
            Models {
                tag: row.sense_voice,
            },
            Some(Named(row.whisper_fp32)),
            Policy::balanced(),
        );
        let request = Request {
            speech_s: Some(8.0),
            ..Request::live(&samples)
        };
        let got = routed.transcribe(&request)?;
        assert_eq!(got.lang, "ja", "{}", row.clip);
        assert_eq!(routed.last_lang(), Some("ja"), "{}", row.clip);
    }
    Ok(())
}

#[test]
fn a_misnamed_clip_is_not_confirmed_as_a_switch_away_from_japanese() -> Result<(), RouteError> {
    // The two clips whisper misnames, met after a Japanese segment: the two
    // identifiers disagree, so the session stays on Japanese.
    let samples = vec![0.0_f32; 16_000 * 8];
    for wrong in ["ko", "zh"] {
        let mut routed = Router::new(
            Models { tag: "<|ja|>" },
            Some(Named(wrong)),
            Policy::balanced(),
        );
        routed.transcribe(&Request::live(&samples))?;
        let second = routed.transcribe(&Request::live(&samples))?;
        assert_eq!(second.lang, "ja", "{wrong}");
    }
    Ok(())
}
