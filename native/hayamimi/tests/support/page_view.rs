//! What a measured page showed of the refined paragraphs and of the listening
//! strip, and the judgement of it (design 10.13.13, BR-14 and BR-15).
//!
//! The measuring script put into the page (see `dialog_run`) records what reached
//! the page's own event stream and what the page drew. This module holds the
//! shape of that record and decides whether it shows the page doing what the
//! design says. Every function reads values only, so each is tested on values
//! written by hand (`br02.rs`), and the records of the real runs are read back
//! by the tests of `br03.rs`.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::Failure;

/// How many refined paragraphs the page keeps: the `ROW_LIMIT` of the page model.
pub const PAGE_ROW_LIMIT: usize = 500;

/// How long before a `final` a partial must have reached the page for the strip
/// to have drawn it when the `final` arrives, in milliseconds.
pub const DRAW_SETTLE_MS: f64 = 150.0;

/// A refined paragraph that reached the page's event stream.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageArrival {
    pub id: Option<String>,
    pub speaker: Option<String>,
    /// Sent to the page again by the measuring script, not by the stream.
    pub replayed: bool,
}

/// The refined paragraphs: what reached the page and how many rows it holds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageRefines {
    /// The `refine-text` elements the page held before the script replayed any paragraph.
    pub rows_before: usize,
    /// How many paragraphs had reached the page when those rows were read.
    pub arrived_before: usize,
    /// The `refine-text` elements it held when the page was read after the replay.
    pub rows: usize,
    pub arrived: Vec<PageArrival>,
}

/// The strip as it stood when a `final` reached the page, and the newest partial before it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AtFinal {
    pub id: Option<String>,
    /// The text of the strip before the page handled the `final`.
    pub strip: Option<String>,
    /// The newest partial since the previous `final`.
    pub partial: Option<String>,
    /// How long before the `final` that partial reached the page.
    pub gap_ms: Option<f64>,
}

/// The listening strip over a run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PageStrip {
    /// The text of the strip before any speech.
    pub waiting: Option<String>,
    pub at_final: Vec<AtFinal>,
    /// How many partials reached the page after the last `final`.
    pub after_last: usize,
    /// The text of the strip when the page was read.
    pub text: Option<String>,
    /// Whether the strip carried the class `quiet` when the page was read.
    pub quiet: bool,
}

/// Both, as the record of a run keeps them (`<step>.page.json`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PageView {
    pub refines: PageRefines,
    pub strip: PageStrip,
}

/// What the refined paragraphs of a page show.
#[derive(Debug, PartialEq, Eq)]
pub struct RefineCheck {
    /// Refined paragraphs that reached the page, repeats included.
    pub arrived: usize,
    /// The rows the page should hold: one for each distinct id and one for each arrival without an id.
    pub paragraphs: usize,
    /// Arrivals whose id had reached the page before.
    pub repeated: usize,
    /// Arrivals the script replayed on purpose.
    pub replayed: usize,
}

/// How many rows the arrivals should make: a repeated id is one paragraph.
fn paragraphs_of(arrived: &[PageArrival]) -> usize {
    let ids: BTreeSet<&str> = arrived
        .iter()
        .filter_map(|arrival| arrival.id.as_deref())
        .collect();
    ids.len()
        + arrived
            .iter()
            .filter(|arrival| arrival.id.is_none())
            .count()
}

/// The page holds one row for each distinct id that reached it, when the rows
/// were first read and again after the script sent some paragraphs to the page
/// again. A run in which nothing reached the page, or in which the script
/// replayed nothing, shows no overlap and is no pass.
pub fn judge_refines(view: &PageRefines) -> Result<RefineCheck, Failure> {
    if view.arrived.is_empty() {
        return Err(String::from("no refined paragraph reached the page"));
    }
    let Some(before) = view.arrived.get(..view.arrived_before) else {
        return Err(format!(
            "the page report counts {} arrivals before the replay and lists {}",
            view.arrived_before,
            view.arrived.len()
        ));
    };
    let replayed = view
        .arrived
        .iter()
        .filter(|arrival| arrival.replayed)
        .count();
    if replayed == 0 {
        return Err(String::from(
            "the script replayed no refined paragraph, so no id reached the page twice",
        ));
    }
    let paragraphs = paragraphs_of(&view.arrived);
    if paragraphs > PAGE_ROW_LIMIT {
        return Err(format!(
            "{paragraphs} paragraphs reached the page, more than the {PAGE_ROW_LIMIT} it keeps"
        ));
    }
    let paragraphs_before = paragraphs_of(before);
    if view.rows_before != paragraphs_before || view.rows != paragraphs {
        return Err(format!(
            "the page held {} refine-text elements before the replay (for {paragraphs_before} \
             paragraphs) and {} after it (for {paragraphs} paragraphs), with {} arrivals, \
             {replayed} replayed",
            view.rows_before,
            view.rows,
            view.arrived.len()
        ));
    }
    Ok(RefineCheck {
        arrived: view.arrived.len(),
        paragraphs,
        repeated: view.arrived.len() - paragraphs,
        replayed,
    })
}

/// What the strip showed before the finals of a run.
#[derive(Debug, PartialEq, Eq)]
pub struct BeforeCheck {
    /// Finals whose strip was set beside the partial before them.
    pub compared: usize,
    /// Finals not compared: no partial before them, or one that came too late to have been drawn.
    pub left_out: usize,
}

/// The newest partial before a `final`, when it reached the page early enough
/// for the strip to have drawn it.
fn settled_partial(found: &AtFinal) -> Option<&str> {
    let gap = found.gap_ms?;
    let partial = found.partial.as_deref()?;
    (gap >= DRAW_SETTLE_MS && !partial.is_empty()).then_some(partial)
}

/// The strip held the newest partial when a `final` arrived, for every `final`
/// that had one with time to be drawn. A run that compares no `final` is no
/// pass: a strip that never showed anything would otherwise pass.
pub fn judge_before_final(finals: &[AtFinal]) -> Result<BeforeCheck, Failure> {
    let mut compared = 0;
    let mut left_out = 0;
    for found in finals {
        match settled_partial(found) {
            None => left_out += 1,
            Some(partial) if found.strip.as_deref() == Some(partial) => compared += 1,
            Some(partial) => {
                return Err(format!(
                    "before the final {:?} the strip read {:?} and the newest partial said {partial:?}",
                    found.id, found.strip
                ));
            }
        }
    }
    if compared == 0 {
        return Err(format!(
            "none of the {} finals had a partial that was drawn before it",
            finals.len()
        ));
    }
    Ok(BeforeCheck { compared, left_out })
}

/// Whether the strip after the `final`s was judged.
#[derive(Debug, PartialEq, Eq)]
pub enum AfterCheck {
    /// The strip read the waiting word of the page at `emptied` finals that followed a final
    /// with a partial and had none of their own, and after the last final it read the
    /// waiting word and carried `quiet`.
    Waiting { emptied: usize },
    /// Partials reached the page after the last `final`, so the strip may show one.
    NotJudged { partials: usize, emptied: usize },
}

/// How many finals found the strip at the waiting word although the final before
/// them had a partial: no partial came in between, so the strip had nothing to
/// show. A strip that kept the partial of the final before reads it again at
/// the next final, and that is an error.
fn emptied_strips(finals: &[AtFinal], waiting: &str) -> Result<usize, Failure> {
    let mut emptied = 0;
    for pair in finals.windows(2) {
        let [before, found] = pair else {
            continue;
        };
        let covered = before
            .partial
            .as_deref()
            .is_some_and(|text| !text.is_empty());
        if !covered || found.partial.is_some() {
            continue;
        }
        if found.strip.as_deref() != Some(waiting) {
            return Err(format!(
                "the final {:?} found the strip reading {:?} with no partial since the final {:?} had covered one",
                found.id, found.strip, before.id
            ));
        }
        emptied += 1;
    }
    Ok(emptied)
}

/// A final empties the strip: the next final, when no partial came in between,
/// finds it at the waiting word. After the last `final` the strip reads the
/// waiting word and carries `quiet`, unless a partial came after it. The word
/// before any speech must be one the page can show, and a run in which no final
/// follows a final with a partial and has none of its own cannot tell a strip
/// that dropped a partial from one that never held it, so it is no pass.
pub fn judge_after_last(view: &PageStrip, waiting_words: &[String]) -> Result<AfterCheck, Failure> {
    let waiting = view
        .waiting
        .as_deref()
        .ok_or("the strip was not on the page when the script was put in")?;
    if !waiting_words.iter().any(|word| word == waiting) {
        return Err(format!(
            "the strip read {waiting:?} before any speech, which is no waiting word of the page"
        ));
    }
    if view.at_final.is_empty() {
        return Err(String::from("no final reached the page"));
    }
    let emptied = emptied_strips(&view.at_final, waiting)?;
    if emptied == 0 {
        return Err(String::from(
            "no final followed a final with a partial without a partial of its own, so nothing shows whether a final empties the strip",
        ));
    }
    if view.after_last > 0 {
        return Ok(AfterCheck::NotJudged {
            partials: view.after_last,
            emptied,
        });
    }
    if view.text.as_deref() != Some(waiting) {
        return Err(format!(
            "after the last final the strip read {:?}, not {waiting:?}",
            view.text
        ));
    }
    if !view.quiet {
        return Err(String::from(
            "after the last final the strip did not carry the class quiet",
        ));
    }
    Ok(AfterCheck::Waiting { emptied })
}

/// The three judgements of a run's page, each kept whole so that the record can
/// say which one failed.
pub struct PageJudgement {
    pub refines: Result<RefineCheck, Failure>,
    pub before: Result<BeforeCheck, Failure>,
    pub after: Result<AfterCheck, Failure>,
}

pub fn judge_page(view: &PageView, waiting_words: &[String]) -> PageJudgement {
    PageJudgement {
        refines: judge_refines(&view.refines),
        before: judge_before_final(&view.strip.at_final),
        after: judge_after_last(&view.strip, waiting_words),
    }
}

impl PageJudgement {
    /// The first judgement that failed, if one did.
    pub fn first_failure(&self) -> Option<&Failure> {
        self.refines
            .as_ref()
            .err()
            .or(self.before.as_ref().err())
            .or(self.after.as_ref().err())
    }

    /// The section of a run's record that says what the page showed.
    pub fn lines(&self) -> String {
        format!(
            "\n## 頁の清書の欄と聞き取りの欄（BR-14、BR-15）\n\n{}\n{}\n{}\n",
            refine_line(&self.refines),
            before_line(&self.before),
            after_line(&self.after)
        )
    }
}

fn refine_line(judged: &Result<RefineCheck, Failure>) -> String {
    match judged {
        Ok(check) => format!(
            "清書の行: 頁の行は {} 行で、清書の数 {} と同じ。届いた {} 件のうち、同じ id が重ねて届いた件は {}、うち script が流し直した件は {}",
            check.paragraphs, check.paragraphs, check.arrived, check.repeated, check.replayed
        ),
        Err(said) => format!("清書の行: 不可 {said}"),
    }
}

fn before_line(judged: &Result<BeforeCheck, Failure>) -> String {
    match judged {
        Ok(check) => format!(
            "聞き取りの欄の確定の前: 欄の文が直前の速報と同じだった確定は {} 件、速報が無いか {DRAW_SETTLE_MS} ms に満たず比べなかった確定は {} 件",
            check.compared, check.left_out
        ),
        Err(said) => format!("聞き取りの欄の確定の前: 不可 {said}"),
    }
}

fn after_line(judged: &Result<AfterCheck, Failure>) -> String {
    match judged {
        Ok(AfterCheck::Waiting { emptied }) => format!(
            "聞き取りの欄の確定の後: 速報の後の確定の次の確定で、間に速報が無く欄が待ちの文言だった確定は {emptied} 件。最後の確定の後は、待ちの文言で、quiet の class を持つ"
        ),
        Ok(AfterCheck::NotJudged { partials, emptied }) => format!(
            "聞き取りの欄の確定の後: 速報の後の確定の次の確定で、間に速報が無く欄が待ちの文言だった確定は {emptied} 件。最後の確定の後に速報が {partials} 件着いたので、その後の欄は判定しない"
        ),
        Err(said) => format!("聞き取りの欄の確定の後: 不可 {said}"),
    }
}
