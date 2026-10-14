//! W7 (board 6.4.1): each column keeps the newest 500 rows. The 501st row
//! drops the oldest from the page, the column says how many were dropped, and
//! the header counts every line.

mod support;

use hayamimi_ui::model::{Model, ROW_LIMIT};
use hayamimi_ui::route::Page;
use hayamimi_ui::strings::Locale;
use support::{Failure, count_in, event, render};

const ROW: &str = "class=\"row\"";
const REFINED: &str = "class=\"refine\"";
const NOTE_JA: &str = "表示から外しました";

fn model_with(finals: usize, refines: usize) -> Result<Model, Failure> {
    let mut model = Model::default();
    for number in 0..finals {
        let data = format!(
            r#"{{"type":"final","id":"f{number}","text":"確定 {number} 行目","lang":"ja","speaker":"","latency_ms":300.0}}"#
        );
        model.apply(event(&data)?, number as u64);
    }
    for number in 0..refines {
        let data = format!(
            r#"{{"type":"refine","id":"r{number}","text":"清書 {number} 段落目","lang":"ja","speaker":"S1"}}"#
        );
        model.apply(event(&data)?, number as u64);
    }
    Ok(model)
}

#[test]
fn the_501st_confirmed_line_keeps_500_rows_and_says_one_was_dropped() -> Result<(), Failure> {
    let html = render(&model_with(ROW_LIMIT + 1, 0)?, Page::Dashboard, Locale::Ja);
    assert_eq!(count_in(&html, ROW), ROW_LIMIT);
    assert_eq!(count_in(&html, "古い 1 行は表示から外しました"), 1);
    // The oldest line is gone and the newest is there.
    assert_eq!(count_in(&html, ">確定 0 行目<"), 0);
    assert_eq!(count_in(&html, ">確定 500 行目<"), 1);
    // The header counts the dropped line too.
    assert_eq!(count_in(&html, "<b>501</b>"), 1);
    Ok(())
}

#[test]
fn the_500th_confirmed_line_shows_all_500_and_no_note() -> Result<(), Failure> {
    let html = render(&model_with(ROW_LIMIT, 0)?, Page::Dashboard, Locale::Ja);
    assert_eq!(count_in(&html, ROW), ROW_LIMIT);
    assert_eq!(count_in(&html, NOTE_JA), 0);
    Ok(())
}

#[test]
fn the_note_counts_every_dropped_line_and_speaks_the_language_of_the_page() -> Result<(), Failure> {
    let model = model_with(ROW_LIMIT + 37, 0)?;
    let ja = render(&model, Page::Dashboard, Locale::Ja);
    assert_eq!(count_in(&ja, "古い 37 行は表示から外しました"), 1);
    let en = render(&model, Page::Dashboard, Locale::En);
    assert_eq!(count_in(&en, "37 older lines are no longer shown"), 1);
    Ok(())
}

#[test]
fn the_501st_refined_paragraph_is_capped_on_the_dashboard_and_the_transcript() -> Result<(), Failure>
{
    let model = model_with(3, ROW_LIMIT + 1)?;
    let dashboard = render(&model, Page::Dashboard, Locale::Ja);
    assert_eq!(count_in(&dashboard, REFINED), ROW_LIMIT);
    assert_eq!(count_in(&dashboard, NOTE_JA), 1);
    // The three confirmed lines are untouched: nothing moved to their column.
    assert_eq!(count_in(&dashboard, ROW), 3);
    let transcript = render(&model, Page::Transcript, Locale::Ja);
    assert_eq!(count_in(&transcript, REFINED), ROW_LIMIT);
    assert_eq!(count_in(&transcript, "古い 1 行は表示から外しました"), 1);
    assert_eq!(count_in(&transcript, ">清書 0 段落目<"), 0);
    Ok(())
}

#[test]
fn a_page_with_few_rows_has_no_note_to_find() -> Result<(), Failure> {
    // The control for the counts above: the counter finds the note when it is there.
    let html = render(&model_with(2, 2)?, Page::Dashboard, Locale::Ja);
    assert_eq!(count_in(&html, NOTE_JA), 0);
    assert_eq!(count_in(&html, ROW), 2);
    assert_eq!(count_in(&html, REFINED), 2);
    Ok(())
}
