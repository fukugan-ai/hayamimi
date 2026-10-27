//! BR-5: a settled line is not queued behind the translation of the text in
//! progress.
//!
//! The relay follows fake recognizers and translates through the fake
//! translator of `support/events.rs`; there is no audio and no model. The
//! translator keeps a log of every request it receives, with the stage and the
//! text, and answers after a time that depends on what the text is: a line's T
//! takes 300 ms and its E 400 ms (or none and 400 ms), a text in progress
//! 100 ms. The lanes are one for each source.
//!
//! While a `final` of a source is being translated, T or E, the text in
//! progress of that source is let go and no request is made for it. The lane of
//! another source, and the text of the same source once the `final` is settled,
//! are not held. With the yield off the same texts are requested while the
//! `final` waits, which is the control of the first case.
//!
//!     cd native && cargo test --release --jobs 2 -p hayamimi-relay --test br05

mod support;

use std::io;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::sleep;
use std::time::{Duration, Instant};

use hayamimi_relay::logging::LogSink;
use hayamimi_relay::partial::PartialTranslation;
use hayamimi_relay::pipeline::{Pipeline, PipelineConfig};
use hayamimi_relay::relay::{Lanes, MakePipeline};
use serde_json::{Value, json};
use support::TestResult;
use support::events::{
    Call, FakeLlm, FakeRecognizer, Running, Sent, SharedLlm, Stage, english_of, outputs,
    run_relay_partials, translation_of, translations_of,
};

const WAIT: Duration = Duration::from_secs(10);

/// T and E of a line that is slow in both.
const SLOW_T: Duration = Duration::from_millis(300);
const SLOW_E: Duration = Duration::from_millis(400);
/// The translation of a text in progress.
const PARTIAL_T: Duration = Duration::from_millis(100);
/// An E, and a translation of a text in progress, long enough that a slow
/// machine still sends what the case sends while they are pending.
const LONG_E: Duration = Duration::from_millis(600);
const LONG_PARTIAL_T: Duration = Duration::from_millis(400);

/// How long each kind of request takes at the translator.
#[derive(Clone, Copy)]
struct Pace {
    translate: Duration,
    edit: Duration,
    in_progress: Duration,
}

const USUAL: Pace = Pace {
    translate: SLOW_T,
    edit: SLOW_E,
    in_progress: PARTIAL_T,
};

/// The texts in progress of the next utterance: three, each longer than the
/// one before, none a text any line says.
const FIRST: &str = "あい";
const SECOND: &str = "あいう";
const THIRD: &str = "あいうえ";
const IN_PROGRESS: [&str; 3] = [FIRST, SECOND, THIRD];

// ---- what the translator received ---------------------------------------------

/// One request the translator received.
#[derive(Clone, Debug)]
struct Asked {
    at: Instant,
    stage: Stage,
    text: String,
}

#[derive(Clone, Default)]
struct Requests(Arc<Mutex<Vec<Asked>>>);

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

impl Requests {
    fn note(&self, call: &Call<'_>) {
        locked(&self.0).push(Asked {
            at: Instant::now(),
            stage: call.stage,
            text: call.text.to_owned(),
        });
    }

    fn all(&self) -> Vec<Asked> {
        locked(&self.0).clone()
    }

    /// The requests made for `text`, whichever the stage.
    fn for_text(&self, text: &str) -> Vec<Asked> {
        let all = self.all();
        all.into_iter().filter(|asked| asked.text == text).collect()
    }

    /// The requests made for any of the texts in progress.
    fn for_the_texts_in_progress(&self) -> Vec<Asked> {
        let all = self.all();
        all.into_iter()
            .filter(|asked| IN_PROGRESS.contains(&asked.text.as_str()))
            .collect()
    }

    /// Waits until the translator has received a request of `stage` for `text`.
    fn wait_for(&self, stage: Stage, text: &str) -> Result<Asked, String> {
        let deadline = Instant::now() + WAIT;
        while Instant::now() < deadline {
            let all = self.all();
            if let Some(found) = all
                .into_iter()
                .find(|asked| asked.stage == stage && asked.text == text)
            {
                return Ok(found);
            }
            sleep(Duration::from_millis(2));
        }
        Err(format!("no {stage:?} request for {text} within {WAIT:?}"))
    }
}

/// A translator that logs what it receives into `requests` and answers at the
/// `pace`.
fn translator(requests: &Requests, pace: Pace) -> Arc<FakeLlm> {
    let requests = requests.clone();
    Arc::new(FakeLlm::new(move |call: &Call<'_>| {
        requests.note(call);
        if IN_PROGRESS.contains(&call.text) {
            return pace.in_progress;
        }
        match call.stage {
            Stage::Translate => pace.translate,
            Stage::Edit => pace.edit,
        }
    }))
}

// ---- running the relay ---------------------------------------------------------

/// A relay that follows `recognizers`, translates with `llm`, and translates
/// the text in progress as `partials` says.
fn start(
    recognizers: &[&FakeRecognizer],
    llm: Arc<FakeLlm>,
    partials: PartialTranslation,
) -> Result<Running, io::Error> {
    let make: MakePipeline<SharedLlm> = Arc::new(move |sink: &mut dyn LogSink| {
        Pipeline::open(
            SharedLlm(Arc::clone(&llm)),
            &PipelineConfig::default(),
            sink,
        )
    });
    run_relay_partials(
        recognizers.iter().map(|found| found.url()).collect(),
        Lanes::PerSource,
        make,
        outputs(Vec::new(), None)?,
        partials,
    )
}

fn partial_event(text: &str) -> String {
    json!({"type": "partial", "text": text}).to_string()
}

/// Sends the first line of `source` and waits until `settled` lines are
/// settled, so that the relay knows what the next line of the source will be
/// called.
fn first_line(
    running: &Running,
    recognizer: &FakeRecognizer,
    source: &'static str,
    settled: usize,
) {
    Sent::spoken(source, 1, "final", "ja").send_through(recognizer);
    running.translations.wait_for(settled, WAIT);
}

/// The texts in progress, one every 50 ms, beginning 50 ms from now.
fn send_texts_in_progress(recognizer: &FakeRecognizer) {
    for text in IN_PROGRESS {
        sleep(Duration::from_millis(50));
        recognizer.send(&partial_event(text));
    }
}

/// The translations of the text in progress among what arrived.
fn in_progress(arrived: &[(Instant, Value)]) -> Vec<&Value> {
    arrived
        .iter()
        .map(|(_, value)| value)
        .filter(|value| value["type"] == "translation" && value["of"] == "partial")
        .collect()
}

/// The one settled translation of `line` among what arrived.
fn check_settled_once(line: &Sent, arrived: &[(Instant, Value)]) -> Result<(), String> {
    let translations: Vec<&Value> = translations_of(arrived)
        .into_iter()
        .map(|(_, value)| value)
        .collect();
    translation_of(line, &translations).map(|_| ())
}

// ---- the cases ---------------------------------------------------------------

/// The line `file-2`, sent when the relay knows the stream is at `file-1`.
fn second_line() -> Sent {
    Sent::spoken("file", 2, "final", "ja")
}

/// What a run of the second line left behind: what the translator received,
/// when T of the line began, and what the relay published.
struct Run {
    requests: Requests,
    began: Asked,
    arrived: Vec<(Instant, Value)>,
}

/// Sends the second line, whose T and E are slow, and the three texts in
/// progress while it waits for T, and waits until the line is settled.
fn final_with_texts_in_progress(
    partials: PartialTranslation,
) -> Result<Run, Box<dyn std::error::Error>> {
    let requests = Requests::default();
    let recognizer = FakeRecognizer::start()?;
    let running = start(&[&recognizer], translator(&requests, USUAL), partials)?;
    first_line(&running, &recognizer, "file", 1);
    let line = second_line();
    line.clone().send_through(&recognizer);
    let began = requests.wait_for(Stage::Translate, &line.text)?;
    send_texts_in_progress(&recognizer);
    let arrived = running.translations.wait_for(2, WAIT);
    check_settled_once(&line, &arrived)?;
    Ok(Run {
        requests,
        began,
        arrived,
    })
}

#[test]
fn br05_a_final_is_not_queued_behind_partial_translations() -> TestResult {
    let run = final_with_texts_in_progress(PartialTranslation::Yielding)?;
    let asked = run.requests.for_the_texts_in_progress();
    assert!(asked.is_empty(), "{asked:?}");
    assert!(in_progress(&run.arrived).is_empty());
    Ok(())
}

#[test]
fn a_partial_arriving_while_the_settled_translation_is_pending_is_not_requested() -> TestResult {
    let requests = Requests::default();
    let recognizer = FakeRecognizer::start()?;
    let pace = Pace {
        translate: Duration::ZERO,
        edit: LONG_E,
        ..USUAL
    };
    let running = start(
        &[&recognizer],
        translator(&requests, pace),
        PartialTranslation::Yielding,
    )?;
    first_line(&running, &recognizer, "file", 1);
    let line = second_line();
    line.clone().send_through(&recognizer);
    requests.wait_for(Stage::Edit, &line.text)?;
    sleep(Duration::from_millis(100));
    recognizer.send(&partial_event(FIRST));
    let arrived = running.translations.wait_for(2, WAIT);
    check_settled_once(&line, &arrived)?;
    let asked = requests.for_text(FIRST);
    assert!(asked.is_empty(), "{asked:?}");
    Ok(())
}

#[test]
fn an_unfinished_partial_translation_is_dropped_when_the_final_arrives() -> TestResult {
    let requests = Requests::default();
    let recognizer = FakeRecognizer::start()?;
    let pace = Pace {
        in_progress: LONG_PARTIAL_T,
        ..USUAL
    };
    let running = start(
        &[&recognizer],
        translator(&requests, pace),
        PartialTranslation::Yielding,
    )?;
    first_line(&running, &recognizer, "file", 1);
    recognizer.send(&partial_event(FIRST));
    requests.wait_for(Stage::Translate, FIRST)?;
    sleep(Duration::from_millis(30));
    let line = second_line();
    line.clone().send_through(&recognizer);
    let arrived = running.translations.wait_for(2, WAIT);
    check_settled_once(&line, &arrived)?;
    let shown = in_progress(&arrived);
    assert!(shown.is_empty(), "{shown:?}");
    Ok(())
}

#[test]
fn a_lone_partial_is_still_translated() -> TestResult {
    let requests = Requests::default();
    let recognizer = FakeRecognizer::start()?;
    let running = start(
        &[&recognizer],
        translator(&requests, USUAL),
        PartialTranslation::Yielding,
    )?;
    first_line(&running, &recognizer, "file", 1);
    recognizer.send(&partial_event(FIRST));
    let arrived = running
        .translations
        .wait_until(|got| !in_progress(got).is_empty(), WAIT);
    let shown = in_progress(&arrived);
    assert_eq!(shown.len(), 1, "{shown:?}");
    assert_eq!(shown[0]["text"], english_of(FIRST));
    assert_eq!(shown[0]["id"], "file-2");
    Ok(())
}

#[test]
fn with_partial_yield_off_a_partial_is_requested_while_the_final_waits() -> TestResult {
    let run = final_with_texts_in_progress(PartialTranslation::On)?;
    let asked = run.requests.for_the_texts_in_progress();
    let while_t_waits = asked
        .iter()
        .filter(|found| found.at < run.began.at + SLOW_T)
        .count();
    assert!(while_t_waits >= 1, "{asked:?}");
    Ok(())
}

#[test]
fn a_partial_after_the_final_is_settled_is_requested_again() -> TestResult {
    let requests = Requests::default();
    let recognizer = FakeRecognizer::start()?;
    let running = start(
        &[&recognizer],
        translator(&requests, USUAL),
        PartialTranslation::Yielding,
    )?;
    first_line(&running, &recognizer, "file", 1);
    let line = second_line();
    line.clone().send_through(&recognizer);
    let settled = running.translations.wait_for(2, WAIT);
    check_settled_once(&line, &settled)?;
    recognizer.send(&partial_event(FIRST));
    requests.wait_for(Stage::Translate, FIRST)?;
    assert_eq!(requests.for_text(FIRST).len(), 1);
    let arrived = running
        .translations
        .wait_until(|got| !in_progress(got).is_empty(), WAIT);
    assert_eq!(in_progress(&arrived).len(), 1);
    Ok(())
}

#[test]
fn a_partial_in_another_lane_is_not_held_by_the_final_of_this_lane() -> TestResult {
    let requests = Requests::default();
    let (file, mic) = (FakeRecognizer::start()?, FakeRecognizer::start()?);
    let running = start(
        &[&file, &mic],
        translator(&requests, USUAL),
        PartialTranslation::Yielding,
    )?;
    first_line(&running, &file, "file", 1);
    first_line(&running, &mic, "mic", 2);
    let line = second_line();
    line.clone().send_through(&file);
    let began = requests.wait_for(Stage::Translate, &line.text)?;
    sleep(Duration::from_millis(50));
    mic.send(&partial_event(FIRST));
    let asked = requests.wait_for(Stage::Translate, FIRST)?;
    assert!(asked.at < began.at + SLOW_T + SLOW_E, "{asked:?}");
    let arrived = running.translations.wait_for(3, WAIT);
    check_settled_once(&line, &arrived)?;
    assert_eq!(requests.for_text(FIRST).len(), 1);
    Ok(())
}

/// Each text in progress that is let go is said so in the log, with the reason
/// `yielded`; nothing else is skipped in this run.
#[test]
fn the_texts_in_progress_that_are_let_go_are_said_so_in_the_log() -> TestResult {
    let requests = Requests::default();
    let recognizer = FakeRecognizer::start()?;
    let running = start(
        &[&recognizer],
        translator(&requests, USUAL),
        PartialTranslation::Yielding,
    )?;
    first_line(&running, &recognizer, "file", 1);
    let line = second_line();
    line.clone().send_through(&recognizer);
    requests.wait_for(Stage::Translate, &line.text)?;
    send_texts_in_progress(&recognizer);
    running.translations.wait_for(2, WAIT);
    let skipped = running.logs.named("translate.partial.skipped");
    let reasons: Vec<&str> = skipped
        .iter()
        .filter_map(|event| event.fields["reason"].as_str())
        .collect();
    assert_eq!(reasons, ["yielded"; 3]);
    Ok(())
}
