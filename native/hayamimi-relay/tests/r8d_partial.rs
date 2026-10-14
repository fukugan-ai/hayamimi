//! R8d: the relay translates the text of an utterance in progress.
//!
//! A recognizer's `partial` event carries neither a source nor an `id`. The
//! relay reads them off the stream (the source of the last `final`, and the
//! `id` after that `final`'s), translates the text with T and V, publishes it
//! as a `translation` of `of: "partial"` that is never settled, drops the
//! request when a newer text or the `final` makes it useless, and never hands
//! it to an outside destination.

mod support;

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::sleep;
use std::time::{Duration, Instant};

use hayamimi_relay::llm::{Completion, Llm, LlmError};
use hayamimi_relay::logging::LogSink;
use hayamimi_relay::partial::PartialTranslation;
use hayamimi_relay::pipeline::{Pipeline, PipelineConfig, sha256_hex};
use hayamimi_relay::relay::{Lanes, MakePipeline};
use serde_json::{Value, json};
use support::TestResult;
use support::events::{
    FakeLlm, FakeRecognizer, Recording, Running, Sent, SharedLlm, english_of, outputs,
    run_relay_partials,
};

const WAIT: Duration = Duration::from_secs(10);

/// Answers like the shared fake after `latency`, and gives the request up at
/// once if its answer stops being wanted while it waits.
struct Patient {
    inner: SharedLlm,
    latency: Duration,
    dropped: Arc<AtomicUsize>,
}

impl Llm for Patient {
    fn complete(&self, request: &Completion<'_>) -> Result<String, LlmError> {
        let until = Instant::now() + self.latency;
        while Instant::now() < until {
            if request.wanted.is_some_and(|wanted| !wanted.still()) {
                self.dropped.fetch_add(1, Ordering::SeqCst);
                return Err(LlmError::Aborted);
            }
            sleep(Duration::from_millis(5));
        }
        self.inner.complete(request)
    }
}

/// A relay at work, with what the tests read off the translator.
struct Setup {
    running: Running,
    llm: Arc<FakeLlm>,
    dropped: Arc<AtomicUsize>,
    recording: Recording,
}

fn setup(
    recognizers: &[&FakeRecognizer],
    latency: Duration,
    partials: PartialTranslation,
) -> Result<Setup, io::Error> {
    let llm = Arc::new(FakeLlm::new(|_| Duration::ZERO));
    let dropped = Arc::new(AtomicUsize::new(0));
    let (made_llm, made_dropped) = (Arc::clone(&llm), Arc::clone(&dropped));
    let make: MakePipeline<Patient> = Arc::new(move |sink: &mut dyn LogSink| {
        let patient = Patient {
            inner: SharedLlm(Arc::clone(&made_llm)),
            latency,
            dropped: Arc::clone(&made_dropped),
        };
        Pipeline::open(patient, &PipelineConfig::default(), sink)
    });
    let recording = Recording::new(true);
    let destinations: Vec<Box<dyn hayamimi_relay::deliver::Destination>> =
        vec![Box::new(recording.clone())];
    let running = run_relay_partials(
        recognizers.iter().map(|found| found.url()).collect(),
        Lanes::PerSource,
        make,
        outputs(destinations, None)?,
        partials,
    )?;
    Ok(Setup {
        running,
        llm,
        dropped,
        recording,
    })
}

fn partial_event(text: &str) -> String {
    json!({"type": "partial", "text": text}).to_string()
}

/// The translations of the text in progress among what arrived.
fn in_progress(arrived: &[(Instant, Value)]) -> Vec<&Value> {
    arrived
        .iter()
        .map(|(_, value)| value)
        .filter(|value| value["type"] == "translation" && value["of"] == "partial")
        .collect()
}

fn settled(arrived: &[(Instant, Value)]) -> usize {
    arrived
        .iter()
        .filter(|(_, value)| value["of"] == "final" && value["settled"] == true)
        .count()
}

/// Sends the first line of the microphone and waits until it is settled, so
/// that the relay knows what the next line will be called.
fn first_line_settled(setup: &Setup, mic: &FakeRecognizer) {
    Sent::mic(1, "final").send_through(mic);
    setup.running.translations.wait_for(1, WAIT);
}

#[test]
fn a_text_in_progress_is_translated_under_the_next_id() -> TestResult {
    let mic = FakeRecognizer::start()?;
    let setup = setup(&[&mic], Duration::ZERO, PartialTranslation::On)?;
    first_line_settled(&setup, &mic);
    mic.send(&partial_event("あいう"));
    let arrived = setup
        .running
        .translations
        .wait_until(|got| !in_progress(got).is_empty(), WAIT);
    let shown = in_progress(&arrived);
    assert_eq!(shown.len(), 1, "{shown:?}");
    let found = shown[0];
    assert_eq!(found["id"], "mic-2");
    assert_eq!(found["source"], "mic");
    assert_eq!(
        (found["src_lang"].as_str(), found["lang"].as_str()),
        (Some("ja"), Some("en"))
    );
    assert_eq!(
        (found["status"].as_str(), found["stage"].as_str()),
        (Some("ok"), Some("t"))
    );
    assert_eq!(found["settled"], false);
    assert_eq!(found["text"], english_of("あいう"));
    assert_eq!(found["src_sha256"], sha256_hex("あいう"));
    assert_eq!(found["annotations"], json!([]));
    Ok(())
}

#[test]
fn nothing_is_translated_before_the_first_final() -> TestResult {
    let mic = FakeRecognizer::start()?;
    let setup = setup(&[&mic], Duration::ZERO, PartialTranslation::On)?;
    mic.send(&partial_event("あいう"));
    sleep(Duration::from_millis(400));
    assert!(in_progress(&setup.running.translations.snapshot()).is_empty());
    assert_eq!(setup.llm.calls.load(Ordering::SeqCst), 0);
    let skipped = setup.running.logs.named("translate.partial.skipped");
    assert!(!skipped.is_empty());
    assert_eq!(skipped[0].fields["reason"], "no_source");
    Ok(())
}

#[test]
fn a_newer_text_takes_the_place_of_the_older_one() -> TestResult {
    let mic = FakeRecognizer::start()?;
    let setup = setup(&[&mic], Duration::from_millis(400), PartialTranslation::On)?;
    first_line_settled(&setup, &mic);
    mic.send(&partial_event("あい"));
    sleep(Duration::from_millis(100));
    mic.send(&partial_event("あいう"));
    setup
        .running
        .translations
        .wait_until(|got| !in_progress(got).is_empty(), WAIT);
    sleep(Duration::from_millis(600));
    let arrived = setup.running.translations.snapshot();
    let shown = in_progress(&arrived);
    assert_eq!(shown.len(), 1, "{shown:?}");
    assert_eq!(shown[0]["text"], english_of("あいう"));
    assert!(setup.dropped.load(Ordering::SeqCst) >= 1);
    assert!(
        !setup
            .running
            .logs
            .named("translate.partial.aborted")
            .is_empty()
    );
    Ok(())
}

#[test]
fn the_final_of_the_utterance_drops_the_text_in_progress() -> TestResult {
    let mic = FakeRecognizer::start()?;
    let setup = setup(&[&mic], Duration::from_millis(300), PartialTranslation::On)?;
    first_line_settled(&setup, &mic);
    mic.send(&partial_event("あい"));
    sleep(Duration::from_millis(100));
    Sent::mic(2, "final").send_through(&mic);
    let arrived = setup
        .running
        .translations
        .wait_until(|got| settled(got) >= 2, WAIT);
    assert_eq!(settled(&arrived), 2);
    assert!(
        in_progress(&arrived).is_empty(),
        "{:?}",
        in_progress(&arrived)
    );
    assert!(setup.dropped.load(Ordering::SeqCst) >= 1);
    Ok(())
}

#[test]
fn two_sources_do_not_stop_each_other() -> TestResult {
    let (mic, loopback) = (FakeRecognizer::start()?, FakeRecognizer::start()?);
    let setup = setup(
        &[&mic, &loopback],
        Duration::from_millis(100),
        PartialTranslation::On,
    )?;
    Sent::mic(1, "final").send_through(&mic);
    Sent::loopback(1, "final").send_through(&loopback);
    setup.running.translations.wait_for(2, WAIT);
    mic.send(&partial_event("あいう"));
    loopback.send(&partial_event("aiu"));
    let arrived = setup
        .running
        .translations
        .wait_until(|got| in_progress(got).len() >= 2, WAIT);
    let mut ids: Vec<&str> = in_progress(&arrived)
        .iter()
        .filter_map(|found| found["id"].as_str())
        .collect();
    ids.sort_unstable();
    assert_eq!(ids, ["loopback-2", "mic-2"]);
    Ok(())
}

#[test]
fn a_language_the_source_does_not_translate_is_left_alone() -> TestResult {
    let mic = FakeRecognizer::start()?;
    let setup = setup(&[&mic], Duration::ZERO, PartialTranslation::On)?;
    first_line_settled(&setup, &mic);
    mic.send(&partial_event("hello wor"));
    sleep(Duration::from_millis(400));
    assert!(in_progress(&setup.running.translations.snapshot()).is_empty());
    let skipped = setup.running.logs.named("translate.partial.skipped");
    assert_eq!(skipped[0].fields["reason"], "language");
    Ok(())
}

#[test]
fn a_translation_in_progress_is_never_sent_outside() -> TestResult {
    let mic = FakeRecognizer::start()?;
    let setup = setup(&[&mic], Duration::ZERO, PartialTranslation::On)?;
    first_line_settled(&setup, &mic);
    mic.send(&partial_event("あいう"));
    setup
        .running
        .translations
        .wait_until(|got| !in_progress(got).is_empty(), WAIT);
    Sent::mic(2, "final").send_through(&mic);
    setup.running.translations.wait_for(2, WAIT);
    let deadline = Instant::now() + WAIT;
    while setup.recording.sent().len() < 2 && Instant::now() < deadline {
        sleep(Duration::from_millis(10));
    }
    let ids: Vec<String> = setup.recording.sent().into_iter().map(|c| c.id).collect();
    assert_eq!(ids, ["mic-1", "mic-2"]);
    Ok(())
}

/// The negative control of RP-5: with the translation of the text in
/// progress off, none is made, and the translator is not asked for one.
#[test]
fn with_the_translation_off_no_text_in_progress_is_translated() -> TestResult {
    let mic = FakeRecognizer::start()?;
    let setup = setup(&[&mic], Duration::ZERO, PartialTranslation::Off)?;
    first_line_settled(&setup, &mic);
    let asked = setup.llm.calls.load(Ordering::SeqCst);
    mic.send(&partial_event("あいう"));
    sleep(Duration::from_millis(500));
    assert!(in_progress(&setup.running.translations.snapshot()).is_empty());
    assert_eq!(setup.llm.calls.load(Ordering::SeqCst), asked);
    Ok(())
}

/// The same run with the translation on makes one, so the control above
/// would fail if the switch did nothing.
#[test]
fn with_the_translation_on_the_same_run_makes_one() -> TestResult {
    let mic = FakeRecognizer::start()?;
    let setup = setup(&[&mic], Duration::ZERO, PartialTranslation::On)?;
    first_line_settled(&setup, &mic);
    mic.send(&partial_event("あいう"));
    let arrived = setup
        .running
        .translations
        .wait_until(|got| !in_progress(got).is_empty(), WAIT);
    assert_eq!(in_progress(&arrived).len(), 1);
    Ok(())
}

#[test]
fn a_new_session_makes_the_relay_forget_the_numbering() -> TestResult {
    let mic = FakeRecognizer::start()?;
    let setup = setup(&[&mic], Duration::ZERO, PartialTranslation::On)?;
    first_line_settled(&setup, &mic);
    mic.send(r#"{"type":"session_reset"}"#);
    mic.send(&partial_event("あいう"));
    sleep(Duration::from_millis(400));
    assert!(in_progress(&setup.running.translations.snapshot()).is_empty());
    Ok(())
}
