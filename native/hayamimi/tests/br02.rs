//! BR-2: the page stage `L_pg` is timed on the page's own clock (design 10.13.3).
//!
//! The real run (T-BR-02-001 to 003) starts `hayamimi` on a websocket input, the
//! relay, a stand-in translation server and `chrome-headless-shell`, opens the
//! dashboard with `?relay=`, puts the measuring script into the page over the
//! DevTools protocol, and plays rows 7 to 10 of `lat-dialog`. A second page
//! without `?relay=` is opened beside it. They are `#[ignore]`d because they
//! need the models, `testdata/` (not in git), the page bundle next to the
//! `hayamimi` binary and a browser.
//!
//! The rows 004 to 007 test the tools that turn the page's two times into a
//! stage and its percentiles, on stamps written by hand. The row 008 reads the
//! product's page code.

mod support;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use support::dialog::{PageStamp, page_percentiles, page_stage_text, page_waits};
use support::dialog_run::{
    PagedRun, Playback, Translator, dialog_window, page_record_of, run_paged,
};
use support::{Failure, TestResult, find_above};

/// The first of the four rows of `lat-dialog` the run plays, counted from 1.
const FIRST_ROW: usize = 7;
/// How many utterances the run plays: ja, en, ja, en.
const UTTERANCES: usize = 4;

fn scratch() -> Result<PathBuf, Failure> {
    let work = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("br02");
    std::fs::create_dir_all(&work).map_err(|error| format!("{}: {error}", work.display()))?;
    Ok(work)
}

fn run_page() -> Result<PagedRun, Failure> {
    let work = scratch()?;
    let (wav, manifest) = dialog_window(&work, FIRST_ROW, UTTERANCES)?;
    run_paged(
        &Translator::Fake,
        &Playback {
            wav: &wav,
            manifest: Some(&manifest),
            repeat: 1,
        },
        &work,
        true,
    )
}

/// The run is made once; the three rows that judge it share what it saw.
fn observed() -> Result<&'static PagedRun, Failure> {
    static RUN: OnceLock<Result<PagedRun, Failure>> = OnceLock::new();
    RUN.get_or_init(run_page).as_ref().map_err(Clone::clone)
}

/// T-BR-02-001
#[test]
#[ignore = "starts the hayamimi binary, the relay, a stand-in translation server and chrome-headless-shell"]
fn br02_the_page_stage_is_timed_on_the_page_clock() -> TestResult {
    let run = observed()?;
    let page = &run.with_relay;
    let stamps = &page.stamps;
    // A line the relay withholds (row 9 of `lat-dialog` says a number) draws a
    // mark and no translation: every row is a measured translation or a mark.
    assert!(!stamps.is_empty(), "the page drew no translation");
    assert_eq!(
        stamps.len() + page.marks_in_rows,
        page.rows,
        "{} translations and {} marks for {} rows",
        stamps.len(),
        page.marks_in_rows,
        page.rows
    );
    assert!(
        page.rows >= UTTERANCES,
        "{} rows for {UTTERANCES} utterances",
        page.rows
    );
    let waits = page_waits(stamps)?;
    assert_eq!(waits.len(), stamps.len());
    let text = page_stage_text(&waits)?;
    assert!(text.contains("p50") && text.contains("p95"), "{text}");
    let record = scratch()?.join("record.txt");
    std::fs::write(&record, format!("{text}\n"))
        .map_err(|error| format!("{}: {error}", record.display()))?;
    Ok(())
}

/// T-BR-02-002
#[test]
#[ignore = "starts the hayamimi binary, the relay, a stand-in translation server and chrome-headless-shell"]
fn the_measuring_script_sees_as_many_draws_as_the_page_has_translation_rows() -> TestResult {
    let page = &observed()?.with_relay;
    let draws = page
        .stamps
        .iter()
        .filter(|stamp| stamp.p_draw.is_some())
        .count();
    assert!(page.trs_in_rows > 0, "{} tr in rows", page.trs_in_rows);
    assert_eq!(draws, page.trs_in_rows, "draws the script saw, tr in rows");
    Ok(())
}

/// T-BR-02-003
#[test]
#[ignore = "starts the hayamimi binary, the relay, a stand-in translation server and chrome-headless-shell"]
fn a_page_without_relay_in_its_address_gives_no_draw_and_an_error() -> TestResult {
    let page = observed()?
        .without_relay
        .as_ref()
        .ok_or("the run opened no page without a relay")?;
    assert!(page.rows >= UTTERANCES, "the page drew {} rows", page.rows);
    assert_eq!(page.trs_in_rows, 0);
    assert_eq!(page.marks_in_rows, 0);
    let draws = page
        .stamps
        .iter()
        .filter(|stamp| stamp.p_draw.is_some())
        .count();
    assert_eq!(draws, 0);
    assert!(page_waits(&page.stamps).is_err());
    Ok(())
}

fn stamp(id: &str, arrive: Option<f64>, draw: Option<f64>) -> PageStamp {
    PageStamp {
        id: id.to_owned(),
        p_arrive: arrive,
        p_draw: draw,
        tool_us: None,
    }
}

/// T-BR-02-004
#[test]
fn a_draw_before_its_arrival_on_the_page_clock_is_an_error() -> TestResult {
    assert_eq!(
        page_waits(&[stamp("file-1", Some(100.0), Some(140.0))])?,
        [40.0]
    );
    assert_eq!(
        page_waits(&[stamp("file-2", Some(200.0), Some(200.0))])?,
        [0.0]
    );
    let early = page_waits(&[stamp("file-3", Some(300.0), Some(299.0))]);
    let said = early.err();
    assert!(
        said.as_deref()
            .is_some_and(|text| text.contains("file-3") && text.contains("-1")),
        "{said:?}"
    );
    Ok(())
}

/// T-BR-02-005
#[test]
fn a_translation_without_a_draw_record_is_named_and_is_an_error() {
    let stamps = [
        stamp("file-1", Some(10.0), Some(20.0)),
        stamp("file-2", Some(30.0), Some(40.0)),
        stamp("file-3", Some(50.0), None),
        stamp("file-4", Some(70.0), Some(80.0)),
    ];
    let said = page_waits(&stamps).err();
    assert!(
        said.as_deref()
            .is_some_and(|text| text.contains("file-3") && !text.contains("file-4")),
        "{said:?}"
    );
}

#[test]
fn a_translation_without_an_arrival_record_is_named_and_is_an_error() {
    let said = page_waits(&[stamp("file-9", None, Some(5.0))]).err();
    assert!(
        said.as_deref().is_some_and(|text| text.contains("file-9")),
        "{said:?}"
    );
}

#[test]
fn a_page_with_no_translation_gives_no_stage() {
    let said = page_waits(&[]).err();
    assert!(said.is_some(), "an empty record gave a stage");
    let undrawn = page_waits(&[
        stamp("file-1", Some(1.0), None),
        stamp("file-2", Some(2.0), None),
    ]);
    assert!(undrawn.is_err());
}

/// T-BR-02-006
#[test]
fn the_page_stage_percentiles_are_nearest_rank() -> TestResult {
    let (p50, p95) = page_percentiles(&[40.0, 10.0, 30.0, 20.0])?;
    assert_eq!((p50, p95), (20.0, 40.0));
    let said = page_percentiles(&[]).err();
    assert!(said.is_some(), "an empty list gave percentiles");
    Ok(())
}

/// T-BR-02-007
#[test]
fn the_page_stage_does_not_depend_on_the_tool_clock() -> TestResult {
    let pairs = [(10.0, 20.0), (100.0, 120.0), (500.0, 530.0), (900.0, 940.0)];
    let with_tool_clock = |shift_us: i64| -> Vec<PageStamp> {
        pairs
            .iter()
            .enumerate()
            .map(|(at, (arrive, draw))| PageStamp {
                id: format!("file-{at}"),
                p_arrive: Some(*arrive),
                p_draw: Some(*draw),
                tool_us: Some(1_000_000 * (at as i64 + 1) + shift_us),
            })
            .collect()
    };
    let first = page_waits(&with_tool_clock(0))?;
    let second = page_waits(&with_tool_clock(1_000_000))?;
    assert_eq!(first, second);
    assert_eq!(page_percentiles(&first)?, page_percentiles(&second)?);
    Ok(())
}

#[test]
fn a_page_report_is_read_into_stamps_by_the_order_of_the_lines() -> TestResult {
    let report = r#"{"finals":["file-1","file-2","file-3"],"arrive":{"file-1":10.5,"file-3":30.0},"draw":[12.0,null,33.5],"rows":3,"trs_in_rows":2,"marks_in_rows":1}"#;
    let record = page_record_of(report)?;
    assert_eq!(
        (record.rows, record.trs_in_rows, record.marks_in_rows),
        (3, 2, 1)
    );
    let ids: Vec<&str> = record
        .stamps
        .iter()
        .map(|stamp| stamp.id.as_str())
        .collect();
    assert_eq!(ids, ["file-1", "file-3"], "file-2 has neither time");
    assert_eq!(
        record
            .stamps
            .iter()
            .map(|stamp| stamp.p_draw)
            .collect::<Vec<_>>(),
        [Some(12.0), Some(33.5)]
    );
    Ok(())
}

#[test]
fn a_page_report_that_is_not_the_script_report_is_an_error() {
    assert!(page_record_of("{").is_err());
    assert!(page_record_of(r#"{"finals":[]}"#).is_err());
}

/// The text of every file under `directory`, as `(path, text)`. A directory
/// that cannot be read is an error and not an empty list.
fn files_under(directory: &Path) -> Result<Vec<(PathBuf, String)>, Failure> {
    let mut found = Vec::new();
    let entries = std::fs::read_dir(directory)
        .map_err(|error| format!("{}: {error}", directory.display()))?;
    for entry in entries {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.is_dir() {
            found.extend(files_under(&path)?);
        } else {
            let text = std::fs::read_to_string(&path)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            found.push((path, text));
        }
    }
    Ok(found)
}

/// The files whose text names a measuring point of the page.
fn files_with_the_hook(files: &[(PathBuf, String)]) -> Vec<&Path> {
    files
        .iter()
        .filter(|(_, text)| text.contains("p_arrive") || text.contains("p_draw"))
        .map(|(path, _)| path.as_path())
        .collect()
}

/// T-BR-02-008
#[test]
fn no_product_page_code_carries_the_measuring_hook() -> TestResult {
    let root = find_above("native/hayamimi-ui/src")?;
    let native = root.ancestors().nth(2).ok_or("no native directory")?;
    let mut product = files_under(&root)?;
    product.extend(files_under(&native.join("hayamimi/data/pages"))?);
    assert!(
        product.len() >= 10,
        "only {} product files were read",
        product.len()
    );
    let carrying = files_with_the_hook(&product);
    assert!(carrying.is_empty(), "{carrying:?}");
    // The control: the same walk finds the hook in this test.
    let control = files_under(&native.join("hayamimi/tests"))?;
    let here: Vec<&Path> = files_with_the_hook(&control)
        .into_iter()
        .filter(|path| path.ends_with("br02.rs"))
        .collect();
    assert_eq!(here.len(), 1, "{here:?}");
    Ok(())
}
