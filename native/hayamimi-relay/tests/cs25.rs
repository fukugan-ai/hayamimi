//! CS-25: the three stages run in the order T, E, V, each translation is
//! verified once, and the conversion note is added after V.
//!
//! A fake llama-server (a TCP server on the loopback address) records every
//! request and answers with the scripted text. V checks each of the two
//! translations once (design 3.4): the translation of T decides whether it is
//! shown early, the translation of E decides the settled line. The draft goes
//! out before E is asked.

mod support;

use hayamimi_relay::lang::Lang;
use hayamimi_relay::llm::HttpLlm;
use hayamimi_relay::logging::MemorySink;
use hayamimi_relay::pipeline::{
    LineKind, LineRequest, Observer, Pass, Pipeline, PipelineConfig, Stage, Status, Translation,
    translate_prompt,
};
use serde_json::Value;
use support::{FakeServer, Reply, TestResult, dir_with_rates, epoch, rates_a, request_body};

const SOURCE: &str = "I paid $100.";
const DRAFT: &str = "私は100ドルを払いました。";
const EDITED: &str = "私は100ドル払いました。";
const NOTE_PIECES: [&str; 5] = ["（", "(", "約", "15,900", "10/12"];

/// What the pipeline reported while it ran.
#[derive(Default)]
struct Trace {
    stages: Vec<Stage>,
    draft_inputs: Vec<String>,
    edit_inputs: Vec<String>,
}

impl Observer for Trace {
    fn stage(&mut self, stage: Stage) {
        self.stages.push(stage);
    }

    fn draft_verify_input(&mut self, translation: &str) {
        self.draft_inputs.push(translation.to_owned());
    }

    fn verify_input(&mut self, translation: &str) {
        self.edit_inputs.push(translation.to_owned());
    }
}

/// What a check of the recorded requests refuses.
#[derive(Debug, PartialEq, Eq)]
enum Fault {
    Count(usize),
    FirstIsNotT,
    SecondIsNotE,
    EditLacksTheDraft,
}

/// The prompts of the two requests must be T's and then E's, and E's must
/// carry the draft T answered.
fn check_requests(prompts: &[String], draft: &str) -> Result<(), Fault> {
    let [first, second] = prompts else {
        return Err(Fault::Count(prompts.len()));
    };
    if first != &translate_prompt(Lang::En, SOURCE) {
        return Err(Fault::FirstIsNotT);
    }
    if !second.starts_with("Below is a English sentence") {
        return Err(Fault::SecondIsNotE);
    }
    if !second.contains(&format!("Draft Japanese:\n{draft}\n\nRewritten Japanese:")) {
        return Err(Fault::EditLacksTheDraft);
    }
    Ok(())
}

fn prompt_of(raw: &[u8]) -> Result<String, Box<dyn std::error::Error>> {
    let body: Value = serde_json::from_slice(request_body(raw))?;
    body["prompt"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| "a request without a prompt".into())
}

/// Everything one run left behind.
struct Run {
    prompts: Vec<String>,
    trace: Trace,
    drafts: Vec<Translation>,
    requests_when_drafted: Vec<usize>,
    settled: Translation,
}

fn run() -> Result<Run, Box<dyn std::error::Error>> {
    let script =
        [DRAFT, EDITED].map(|content| serde_json::json!({ "content": content }).to_string());
    let server = FakeServer::start(move |index, _| match script.get(index) {
        Some(body) => Reply::json(body),
        None => Reply::status(500),
    })?;
    let (_dir, rates) = dir_with_rates(&rates_a())?;
    let config = PipelineConfig {
        rates_path: Some(rates),
        tables_dir: None,
    };
    let mut sink = MemorySink::default();
    let mut pipeline = Pipeline::open(HttpLlm::new(&server.base_url()), &config, &mut sink)?;
    let request = LineRequest {
        id: "cs25",
        of: LineKind::Final,
        source: "loopback",
        src_lang: Lang::En,
        text: SOURCE,
        now_epoch: epoch(support::DEFAULT_NOW)?,
    };
    let mut trace = Trace::default();
    let (mut drafts, mut requests_when_drafted) = (Vec::new(), Vec::new());
    let settled = pipeline.run_staged(
        &request,
        &mut trace,
        &mut |draft| {
            requests_when_drafted.push(server.requests().len());
            drafts.push(draft);
        },
        &mut sink,
    );
    let prompts = server
        .requests()
        .iter()
        .map(|raw| prompt_of(raw))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Run {
        prompts,
        trace,
        drafts,
        requests_when_drafted,
        settled,
    })
}

#[test]
fn cs25_stages_run_in_order_t_e_v_once() -> TestResult {
    let run = run()?;
    // T then E, one request each, and E's carries T's translation.
    assert_eq!(check_requests(&run.prompts, DRAFT), Ok(()));
    // V ran on each translation once, on exactly what the model answered.
    assert_eq!(run.trace.draft_inputs, [DRAFT]);
    assert_eq!(run.trace.edit_inputs, [EDITED]);
    let order = [
        Stage::Translate,
        Stage::VerifyDraft,
        Stage::Edit,
        Stage::Verify,
        Stage::Annotate,
    ];
    assert_eq!(run.trace.stages, order);
    // The conversion note is added after V and never shown to it.
    for shown in run.trace.draft_inputs.iter().chain(&run.trace.edit_inputs) {
        assert!(
            NOTE_PIECES.iter().all(|piece| !shown.contains(piece)),
            "{shown}"
        );
    }
    assert_eq!(run.settled.annotations.len(), 1);
    assert_eq!(run.settled.text, EDITED);
    assert_eq!(
        (run.settled.stage, run.settled.status),
        (Pass::E, Status::Ok)
    );
    Ok(())
}

#[test]
fn cs25_the_draft_goes_out_after_t_and_before_e_is_asked() -> TestResult {
    let run = run()?;
    assert_eq!(run.requests_when_drafted, [1]);
    let [draft] = &run.drafts[..] else {
        return Err(format!("{} drafts", run.drafts.len()).into());
    };
    assert_eq!(
        (draft.stage, draft.settled, draft.text.as_str()),
        (Pass::T, false, DRAFT)
    );
    assert!(draft.annotations.is_empty());
    Ok(())
}

/// The negative controls: an order or a prompt a wrong build would produce.
#[test]
fn cs25_a_wrong_order_or_an_edit_without_the_draft_is_refused() {
    let t = translate_prompt(Lang::En, SOURCE);
    let e =
        format!("Below is a English sentence\n\nDraft Japanese:\n{DRAFT}\n\nRewritten Japanese:\n");
    assert_eq!(check_requests(&[t.clone(), e.clone()], DRAFT), Ok(()));
    assert_eq!(
        check_requests(&[e.clone(), t.clone()], DRAFT),
        Err(Fault::FirstIsNotT)
    );
    assert_eq!(
        check_requests(std::slice::from_ref(&t), DRAFT),
        Err(Fault::Count(1))
    );
    assert_eq!(
        check_requests(&[t.clone(), t.clone()], DRAFT),
        Err(Fault::SecondIsNotE)
    );
    assert_eq!(
        check_requests(&[t, e], "other"),
        Err(Fault::EditLacksTheDraft)
    );
}
