//! IP-3: when the language identifier is unsure, a `--mode pair` route still
//! reads the segment as `ja` or `en` and does not stop.
//!
//! The identifier and the recognizers are scripted, so a test says what each
//! one answers and then reads which language was chosen, which step chose it
//! and which recognizers were asked. Nothing here opens a model.
//!
//! The cases that read the session's own state (`session.pending`) are in
//! `src/route/tests/pair.rs`, because that state is private to the crate.

use std::cell::Cell;

use hayamimi_asr::AsrError;
use hayamimi_asr::lid::{Identify, script_corrected_lang};
use hayamimi_asr::route::{
    Decode, DecodeError, Decoded, FallbackStep, LangFallback, Policy, Request, RouteError, Router,
    Tier, Transcript,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Audio the fakes ignore.
static SAMPLES: [f32; 4800] = [0.0; 4800];

/// What the identifier answers when it is asked.
#[derive(Clone, Copy)]
enum Say {
    Lang(&'static str),
    Fail,
}

/// An identifier that answers from a list, again from its start when the list
/// is used up, and counts how often it was asked.
struct Heard {
    says: Vec<Say>,
    asked: Cell<usize>,
}

impl Heard {
    fn saying(says: &[Say]) -> Heard {
        Heard {
            says: says.to_vec(),
            asked: Cell::new(0),
        }
    }

    fn lang(code: &'static str) -> Heard {
        Heard::saying(&[Say::Lang(code)])
    }
}

impl Identify for Heard {
    fn identify(&self, _samples: &[f32]) -> Result<String, AsrError> {
        let at = self.asked.get();
        self.asked.set(at + 1);
        let Some(say) = self.says.iter().cycle().nth(at) else {
            return Err(AsrError::Route(String::from(
                "the identifier has no answer",
            )));
        };
        match say {
            Say::Lang(code) => Ok((*code).to_owned()),
            Say::Fail => Err(AsrError::NoResult),
        }
    }
}

/// What a recognizer does when it is asked.
#[derive(Clone, Copy)]
enum Reply {
    /// The text and the language tag the model puts on it.
    Text(&'static str, &'static str),
    Unavailable,
    Failed,
}

/// Recognizers that answer from a list per tier (the last answer repeats), and
/// remember what they were asked. A tier with no list reads a plain sentence of
/// its own language, except SenseVoice, which is then not installed.
struct Models {
    script: Vec<(Tier, Vec<Reply>)>,
    calls: Vec<Tier>,
}

impl Models {
    fn new() -> Models {
        Models {
            script: Vec::new(),
            calls: Vec::new(),
        }
    }

    fn with(mut self, tier: Tier, replies: &[Reply]) -> Models {
        self.script.push((tier, replies.to_vec()));
        self
    }

    fn asked(&self, tier: Tier) -> usize {
        self.calls.iter().filter(|called| **called == tier).count()
    }

    fn reply(&self, tier: Tier, nth: usize) -> Reply {
        let Some((_, replies)) = self.script.iter().find(|(known, _)| *known == tier) else {
            return plain(tier);
        };
        let Some(reply) = replies.get(nth).or(replies.last()) else {
            return Reply::Unavailable;
        };
        *reply
    }
}

/// What a tier with no list reads.
fn plain(tier: Tier) -> Reply {
    match tier {
        Tier::Rz | Tier::Pja => Reply::Text("はい", ""),
        Tier::Pz => Reply::Text("你好", ""),
        Tier::V3 | Tier::V2 => Reply::Text("yes", ""),
        Tier::Omni => Reply::Text("ok", ""),
        Tier::Sv => Reply::Unavailable,
    }
}

impl Decode for Models {
    fn decode(&mut self, tier: Tier, _samples: &[f32]) -> Result<Decoded, DecodeError> {
        let reply = self.reply(tier, self.asked(tier));
        self.calls.push(tier);
        match reply {
            Reply::Text(text, tag) => Ok(Decoded {
                text: text.to_owned(),
                tag: tag.to_owned(),
            }),
            Reply::Unavailable => Err(DecodeError::Unavailable),
            Reply::Failed => Err(DecodeError::Failed(AsrError::NoResult)),
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

type Fake = Router<Models, Heard>;

fn router(models: Models, lid: Option<Heard>, policy: Policy) -> Fake {
    Router::new(models, lid, policy)
}

fn read_live(router: &mut Fake) -> Result<Transcript, RouteError> {
    router.transcribe(&Request::live(&SAMPLES))
}

fn read_with_speech(router: &mut Fake, speech_s: f64) -> Result<Transcript, RouteError> {
    router.transcribe(&Request {
        speech_s: Some(speech_s),
        ..Request::live(&SAMPLES)
    })
}

fn fell(step: FallbackStep, lid: Option<&str>) -> Option<LangFallback> {
    Some(LangFallback {
        step,
        lid: lid.map(str::to_owned),
    })
}

/// What the identifier says when it is unsure, with the `lid` the record keeps.
const UNSURE: [(Say, Option<&str>); 4] = [
    (Say::Lang("zh"), Some("zh")),
    (Say::Lang("ko"), Some("ko")),
    (Say::Lang(""), None),
    (Say::Fail, None),
];

/// What SenseVoice says, with the language and the step it comes to.
const SENSE_VOICE: [(Reply, &str, FallbackStep); 4] = [
    (Reply::Text("yes", "<|en|>"), "en", FallbackStep::SvTag),
    (
        Reply::Text("thank you", "<|zh|>"),
        "en",
        FallbackStep::Script,
    ),
    (
        Reply::Text("ありがとう", "<|zh|>"),
        "ja",
        FallbackStep::Script,
    ),
    (Reply::Text("", "<|zh|>"), "ja", FallbackStep::Default),
];

/// T-IP-03-001
#[test]
fn ip03_an_unsure_identifier_falls_to_ja_or_en() -> TestResult {
    for (say, lid) in UNSURE {
        for (sense_voice, lang, step) in SENSE_VOICE {
            let models = Models::new().with(Tier::Sv, &[sense_voice]);
            let mut routed = router(models, Some(Heard::saying(&[say])), Policy::pair());
            let got = read_live(&mut routed)?;
            assert_eq!(got.lang, lang, "{step:?} after {lid:?}");
            assert_eq!(got.fallback, fell(step, lid), "after {lid:?}");
            assert_eq!(routed.decoder().asked(Tier::Sv), 1, "{step:?}");
        }
    }
    Ok(())
}

/// T-IP-03-002
#[test]
fn a_router_with_no_identifier_falls_to_ja_or_en() -> TestResult {
    for (sense_voice, lang, step) in SENSE_VOICE {
        let models = Models::new().with(Tier::Sv, &[sense_voice]);
        let mut routed = router(models, None, Policy::pair());
        let got = read_live(&mut routed)?;
        assert_eq!(got.lang, lang, "{step:?}");
        assert_eq!(got.fallback, fell(step, None), "{step:?}");
    }
    Ok(())
}

/// T-IP-03-003
#[test]
fn an_identifier_that_says_ja_or_en_never_asks_sensevoice() -> TestResult {
    for code in ["ja", "en"] {
        let models = Models::new().with(Tier::Sv, &[Reply::Text("yes", "<|en|>")]);
        let mut routed = router(models, Some(Heard::lang(code)), Policy::pair());
        let got = read_live(&mut routed)?;
        assert_eq!(got.lang, code);
        assert_eq!(got.fallback, None);
        assert_eq!(routed.decoder().asked(Tier::Sv), 0, "{code}");
    }
    Ok(())
}

/// T-IP-03-004
#[test]
fn fast_mode_follows_an_identifier_that_says_zh() -> TestResult {
    let sense_voice = Reply::Text("yes", "<|en|>");
    let models = Models::new().with(Tier::Sv, &[sense_voice]);
    let mut fast = router(models, Some(Heard::lang("zh")), Policy::fast());
    assert_eq!(read_live(&mut fast)?.lang, "zh");
    let models = Models::new().with(Tier::Sv, &[sense_voice]);
    let mut pair = router(models, Some(Heard::lang("zh")), Policy::pair());
    assert_eq!(read_live(&mut pair)?.lang, "en");
    Ok(())
}

/// T-IP-03-005
#[test]
fn balanced_mode_holds_the_session_language_against_a_sensevoice_that_disagrees() -> TestResult {
    let says = [Say::Lang("ja"), Say::Lang("en")];
    let langs_of = |policy: Policy| -> Result<Vec<String>, RouteError> {
        let models = Models::new().with(Tier::Sv, &[Reply::Text("はい", "<|ja|>")]);
        let mut routed = router(models, Some(Heard::saying(&says)), policy);
        let mut langs = Vec::new();
        for _ in 0..2 {
            langs.push(read_with_speech(&mut routed, 3.0)?.lang);
        }
        Ok(langs)
    };
    assert_eq!(langs_of(Policy::balanced())?, ["ja", "ja"]);
    assert_eq!(langs_of(Policy::pair())?, ["ja", "en"]);
    Ok(())
}

/// One reading of an identifier that says `zh`, with SenseVoice reading `text`
/// under a tag that is not `ja` or `en`.
fn script_decides(text: &'static str, tag: &'static str) -> Result<Transcript, RouteError> {
    let models = Models::new().with(Tier::Sv, &[Reply::Text(text, tag)]);
    let mut routed = router(models, Some(Heard::lang("zh")), Policy::pair());
    read_live(&mut routed)
}

fn assert_by_script(got: &Transcript, lang: &str) {
    assert_eq!(got.lang, lang);
    assert_eq!(
        got.fallback.as_ref().map(|fallback| fallback.step),
        Some(FallbackStep::Script)
    );
}

/// T-IP-03-006
#[test]
fn a_text_with_more_than_three_tenths_kana_and_kanji_is_ja() -> TestResult {
    assert_by_script(&script_decides("日本語だabcdef", "<|zh|>")?, "ja");
    Ok(())
}

/// T-IP-03-007
#[test]
fn a_text_of_latin_letters_only_is_en() -> TestResult {
    assert_by_script(&script_decides("Hello there", "<|zh|>")?, "en");
    Ok(())
}

fn assert_by_default(got: &Transcript) {
    assert_eq!(got.lang, "ja");
    assert_eq!(
        got.fallback.as_ref().map(|fallback| fallback.step),
        Some(FallbackStep::Default)
    );
}

/// T-IP-03-008
#[test]
fn a_sensevoice_reading_that_is_empty_falls_to_ja() -> TestResult {
    assert_by_default(&script_decides("", "<|zh|>")?);
    Ok(())
}

/// T-IP-03-009
#[test]
fn a_sensevoice_reading_of_symbols_only_falls_to_ja() -> TestResult {
    assert_by_default(&script_decides("。、！", "<|zh|>")?);
    Ok(())
}

/// T-IP-03-010
#[test]
fn an_unavailable_sensevoice_falls_to_ja() -> TestResult {
    let models = Models::new().with(Tier::Sv, &[Reply::Unavailable]);
    let mut routed = router(models, Some(Heard::lang("zh")), Policy::pair());
    assert_by_default(&read_live(&mut routed)?);
    Ok(())
}

/// T-IP-03-011
#[test]
fn a_sensevoice_tag_of_ko_is_outside_the_pair_and_the_text_decides() -> TestResult {
    let models = Models::new().with(Tier::Sv, &[Reply::Text("thank you very much", "<|ko|>")]);
    let mut routed = router(models, Some(Heard::lang("ko")), Policy::pair());
    assert_by_script(&read_live(&mut routed)?, "en");
    Ok(())
}

/// T-IP-03-012
#[test]
fn a_sensevoice_tag_of_yue_is_outside_the_pair_and_the_text_decides() -> TestResult {
    assert_by_script(&script_decides("ありがとう", "<|yue|>")?, "ja");
    Ok(())
}

/// T-IP-03-013
#[test]
fn a_japanese_reading_in_hangul_stays_ja_in_pair_mode() -> TestResult {
    assert_eq!(script_corrected_lang("ja", "안녕하세요"), "ko");
    let models = Models::new().with(Tier::Rz, &[Reply::Text("안녕하세요", "")]);
    let mut routed = router(models, Some(Heard::lang("ja")), Policy::pair());
    assert_eq!(read_live(&mut routed)?.lang, "ja");
    Ok(())
}

/// T-IP-03-014
#[test]
fn a_japanese_reading_in_latin_letters_is_read_again_as_en_in_pair_mode() -> TestResult {
    assert_eq!(script_corrected_lang("ja", "thank you very much"), "en");
    let models = Models::new()
        .with(Tier::Rz, &[Reply::Text("thank you very much", "")])
        .with(Tier::V3, &[Reply::Text("thank you very much", "")]);
    let mut routed = router(models, Some(Heard::lang("ja")), Policy::pair());
    let got = read_live(&mut routed)?;
    assert_eq!(got.lang, "en");
    assert_eq!(got.text, "thank you very much");
    Ok(())
}

/// T-IP-03-015
#[test]
fn pair_mode_follows_each_detection_even_when_the_utterance_is_short() -> TestResult {
    let says = [
        Say::Lang("ja"),
        Say::Lang("en"),
        Say::Lang("ja"),
        Say::Lang("en"),
    ];
    let langs_of = |policy: Policy| -> Result<Vec<String>, RouteError> {
        let models = Models::new().with(Tier::Sv, &[Reply::Text("はい", "<|ja|>")]);
        let mut routed = router(models, Some(Heard::saying(&says)), policy);
        let mut langs = Vec::new();
        for _ in 0..4 {
            langs.push(read_with_speech(&mut routed, 0.3)?.lang);
        }
        Ok(langs)
    };
    assert_eq!(langs_of(Policy::pair())?, ["ja", "en", "ja", "en"]);
    assert_eq!(langs_of(Policy::balanced())?[1], "ja");
    Ok(())
}

/// The three inputs of T-IP-03-025: an identifier that is unsure, one that
/// fails, and none.
fn without_a_usable_identifier() -> [(Option<Heard>, Option<&'static str>); 3] {
    [
        (Some(Heard::lang("zh")), Some("zh")),
        (Some(Heard::saying(&[Say::Fail])), None),
        (None, None),
    ]
}

/// T-IP-03-025
#[test]
fn a_failed_sensevoice_falls_to_ja_with_the_sv_failed_step() -> TestResult {
    for (lid, said) in without_a_usable_identifier() {
        let models = Models::new().with(Tier::Sv, &[Reply::Failed]);
        let mut routed = router(models, lid, Policy::pair());
        let got = read_live(&mut routed)?;
        assert_eq!(got.lang, "ja");
        assert_eq!(got.fallback, fell(FallbackStep::SvFailed, said));
        assert_eq!(got.text, "はい");
    }
    Ok(())
}

/// T-IP-03-026
#[test]
fn a_failed_recognizer_other_than_the_sensevoice_probe_is_still_an_error_in_pair_mode() {
    let models = Models::new().with(Tier::Rz, &[Reply::Failed]);
    let mut routed = router(models, Some(Heard::lang("ja")), Policy::pair());
    assert!(matches!(read_live(&mut routed), Err(RouteError::Decode(_))));
    let models = Models::new()
        .with(Tier::Sv, &[Reply::Text("yes", "<|en|>")])
        .with(Tier::V3, &[Reply::Failed]);
    let mut routed = router(models, Some(Heard::lang("zh")), Policy::pair());
    assert!(matches!(read_live(&mut routed), Err(RouteError::Decode(_))));
}

/// T-IP-03-027
#[test]
fn a_failed_sensevoice_is_asked_again_for_the_next_utterance_and_a_missing_one_is_not() -> TestResult
{
    let second = Reply::Text("yes", "<|en|>");
    let models = Models::new().with(Tier::Sv, &[Reply::Failed, second]);
    let mut routed = router(models, Some(Heard::lang("zh")), Policy::pair());
    let first = read_live(&mut routed)?;
    assert_eq!(first.lang, "ja");
    assert_eq!(
        first.fallback.as_ref().map(|fallback| fallback.step),
        Some(FallbackStep::SvFailed)
    );
    let next = read_live(&mut routed)?;
    assert_eq!(next.lang, "en");
    assert_eq!(
        next.fallback.as_ref().map(|fallback| fallback.step),
        Some(FallbackStep::SvTag)
    );
    assert_eq!(routed.decoder().asked(Tier::Sv), 2);

    let models = Models::new().with(Tier::Sv, &[Reply::Unavailable, second]);
    let mut gone = router(models, Some(Heard::lang("zh")), Policy::pair());
    assert_by_default(&read_live(&mut gone)?);
    assert_by_default(&read_live(&mut gone)?);
    assert_eq!(gone.decoder().asked(Tier::Sv), 1);
    Ok(())
}

/// T-IP-03-028
#[test]
fn a_han_only_reading_is_read_again_as_ja_without_asking_sensevoice_in_pair_mode() -> TestResult {
    let models = || {
        Models::new()
            .with(Tier::V3, &[Reply::Text("東京大学", "")])
            .with(Tier::Sv, &[Reply::Text("东京大学", "<|zh|>")])
            .with(Tier::Rz, &[Reply::Text("東京大学です", "")])
            .with(Tier::Pz, &[Reply::Text("东京大学", "")])
    };
    let mut pair = router(models(), Some(Heard::lang("en")), Policy::pair());
    let got = read_live(&mut pair)?;
    assert_eq!(got.lang, "ja");
    assert_eq!(got.text, "東京大学です");
    assert_eq!(got.fallback, None);
    assert_eq!(pair.decoder().asked(Tier::Sv), 0);
    let mut fast = router(models(), Some(Heard::lang("en")), Policy::fast());
    assert_eq!(read_live(&mut fast)?.lang, "zh");
    assert_eq!(fast.decoder().asked(Tier::Sv), 1);
    Ok(())
}

/// T-IP-03-029
#[test]
fn three_tenths_exactly_is_en_and_the_denominator_counts_letters_only() -> TestResult {
    assert_by_script(&script_decides("日本語abcdefg", "<|zh|>")?, "en");
    assert_by_script(&script_decides("日本語だ abc, def 123", "<|zh|>")?, "ja");
    Ok(())
}

/// T-IP-03-030
#[test]
fn a_text_of_hangul_only_or_cyrillic_only_is_en() -> TestResult {
    assert_by_script(&script_decides("안녕하세요", "<|zh|>")?, "en");
    assert_by_script(&script_decides("привет", "<|zh|>")?, "en");
    Ok(())
}

/// T-IP-03-031
#[test]
fn a_text_mixing_scripts_is_decided_by_the_share_of_kana_and_kanji_alone() -> TestResult {
    assert_by_script(&script_decides("hello 안녕하세요", "<|zh|>")?, "en");
    assert_by_script(&script_decides("привет hello", "<|zh|>")?, "en");
    assert_by_script(&script_decides("ありがとう 안녕", "<|zh|>")?, "ja");
    Ok(())
}

/// T-IP-03-032
#[test]
fn a_router_with_no_identifier_follows_sensevoice_and_not_the_session_language_in_pair_mode()
-> TestResult {
    let models = || {
        Models::new().with(
            Tier::Sv,
            &[Reply::Text("yes", "<|en|>"), Reply::Text("はい", "<|ja|>")],
        )
    };
    let mut pair = router(models(), None, Policy::pair());
    let langs = [read_live(&mut pair)?, read_live(&mut pair)?];
    assert_eq!(langs.each_ref().map(|got| got.lang.as_str()), ["en", "ja"]);
    assert!(langs.iter().all(
        |got| got.fallback.as_ref().map(|fallback| fallback.step) == Some(FallbackStep::SvTag)
    ));
    assert_eq!(pair.decoder().asked(Tier::Sv), 2);

    let mut fast = router(models(), None, Policy::fast());
    assert_eq!(read_live(&mut fast)?.lang, "ja");
    assert_eq!(read_live(&mut fast)?.lang, "ja");
    assert_eq!(fast.decoder().asked(Tier::Sv), 0);
    Ok(())
}

/// T-IP-03-033
#[test]
fn a_draft_in_pair_mode_is_read_exactly_as_in_fast_mode() -> TestResult {
    let models = || {
        Models::new()
            .with(Tier::Pz, &[Reply::Text("ni hao", "")])
            .with(Tier::Rz, &[Reply::Text("こんにちは", "")])
            .with(Tier::Sv, &[Reply::Text("はい", "<|ja|>")])
    };
    for hint in [Some("zh"), None] {
        let mut drafts = Vec::new();
        for policy in [Policy::pair(), Policy::fast()] {
            let mut routed = router(models(), Some(Heard::lang("ja")), policy);
            read_live(&mut routed)?;
            let before = routed.decoder().calls.len();
            let text = routed.partial(&SAMPLES, hint)?;
            drafts.push((text, routed.decoder().calls[before..].to_vec()));
        }
        assert_eq!(drafts[0], drafts[1], "{hint:?}");
    }
    let mut pair = router(models(), Some(Heard::lang("ja")), Policy::pair());
    assert_eq!(pair.partial(&SAMPLES, Some("zh"))?, "ni hao");
    assert_eq!(pair.decoder().calls, [Tier::Pz]);
    Ok(())
}
