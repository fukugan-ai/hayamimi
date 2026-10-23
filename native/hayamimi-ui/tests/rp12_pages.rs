//! RP-12, the rows of the three pages: the pages read the wire the server
//! gives them.
//!
//! The wire is the recording of the Python server (`<base>/wire/`), the shapes
//! `hayamimi` and the relay add to it, and a stream read from a server over a
//! socket. The server here is a fake that sends the messages framed as
//! server-sent events; the real one replaces it by giving `read_events` its
//! address.

mod support;

use std::collections::BTreeMap;

use hayamimi_ui::model::{Conn, Model};
use hayamimi_ui::overlay::Show;
use hayamimi_ui::route::Page;
use hayamimi_ui::strings::Locale;
use hayamimi_ui::wire::{self, UiEvent};
use support::{
    Failure, FakeEvents, count_in, event, model_after, read_events, recorded_sse_datas, render,
};

fn type_of(data: &str) -> Result<String, Failure> {
    let value: serde_json::Value = serde_json::from_str(data)?;
    match value.get("type").and_then(serde_json::Value::as_str) {
        Some(name) => Ok(name.to_owned()),
        None => Err(format!("a message without a type: {data}").into()),
    }
}

fn overlay(model: &Model) -> String {
    render(model, Page::Overlay(Show::Both), Locale::En)
}

fn dashboard(model: &Model) -> String {
    render(model, Page::Dashboard, Locale::En)
}

fn transcript(model: &Model) -> String {
    render(model, Page::Transcript, Locale::En)
}

#[test]
fn rp12_pages_read_every_message_of_the_recording() -> Result<(), Failure> {
    let datas = recorded_sse_datas()?;
    let mut by_type: BTreeMap<String, usize> = BTreeMap::new();
    for data in &datas {
        *by_type.entry(type_of(data)?).or_default() += 1;
        wire::parse(data)?;
    }
    // The control: the recording holds the types the pages draw, and a type
    // the pages do not draw.
    for drawn in ["partial", "final", "refine", "translation"] {
        assert!(
            by_type.get(drawn).is_some_and(|count| *count > 0),
            "{drawn} is not in {by_type:?}"
        );
    }
    assert!(by_type.contains_key("session_summary"), "{by_type:?}");
    Ok(())
}

#[test]
fn rp12_a_message_with_a_key_dropped_is_refused_and_one_with_a_key_added_is_not()
-> Result<(), Failure> {
    let datas = recorded_sse_datas()?;
    let Some(final_data) = datas
        .iter()
        .find(|data| type_of(data).is_ok_and(|name| name == "final"))
    else {
        return Err("the recording has no final".into());
    };
    let mut added: serde_json::Value = serde_json::from_str(final_data)?;
    let mut dropped = added.clone();
    assert!(wire::parse(&added.to_string()).is_ok());
    let (Some(plus), Some(minus)) = (added.as_object_mut(), dropped.as_object_mut()) else {
        return Err("a final is not an object".into());
    };
    plus.insert(
        String::from("a_key_the_server_may_add_later"),
        serde_json::json!([1, 2]),
    );
    minus.remove("text");
    assert!(wire::parse(&added.to_string()).is_ok());
    assert!(wire::parse(&dropped.to_string()).is_err());
    Ok(())
}

#[test]
fn rp12_the_pages_draw_the_recorded_session() -> Result<(), Failure> {
    let datas = recorded_sse_datas()?;
    let mut model = Model::default();
    for (at, data) in datas.iter().enumerate() {
        model.apply(event(data)?, at as u64);
    }
    let finals = datas
        .iter()
        .filter(|data| type_of(data).is_ok_and(|name| name == "final"))
        .count();
    let refines = datas
        .iter()
        .filter(|data| type_of(data).is_ok_and(|name| name == "refine"))
        .count();
    assert_eq!(model.finals.len(), finals);
    assert_eq!(model.refines.len(), refines);
    assert!(finals > 0 && refines > 0);

    let spoken = model.finals[0].text.clone();
    let dash = dashboard(&model);
    assert_eq!(
        count_in(&dash, &format!("<div class=\"row-text\">{spoken}</div>")),
        finals
    );
    assert!(dash.contains(&format!(
        "<div class=\"refine-text\">{}</div>",
        model.refines[0].text
    )));
    assert!(transcript(&model).contains(&model.refines[0].text));
    assert!(overlay(&model).contains(&format!("<span id=\"final-line\">{spoken}</span>")));

    // The recorded `translation` has no `of` and no `id`: it is not stuck to
    // the line before it.
    let recorded_translation = datas
        .iter()
        .find(|data| type_of(data).is_ok_and(|name| name == "translation"))
        .ok_or("the recording has no translation")?;
    let UiEvent::Translation(found) = event(recorded_translation)? else {
        return Err("a translation did not parse as one".into());
    };
    assert!(
        !dash.contains(&found.text),
        "the translation of no line was drawn under a line"
    );
    Ok(())
}

const FALLBACKS: [&str; 2] = [
    r#"{"type":"fallback","stage":"translate","reason":"unreachable","to":"cpu"}"#,
    r#"{"type":"fallback","stage":"punct","reason":"no_device","to":"cpu"}"#,
];

/// What `hayamimi` sends for one utterance, with the relay's translations.
fn a_session() -> Vec<String> {
    let mut messages: Vec<String> = FALLBACKS.iter().map(|data| (*data).to_owned()).collect();
    messages.extend(
        [
            r#"{"type":"session_start"}"#,
            r#"{"type":"partial","text":"予算は五百万"}"#,
            r#"{"type":"translation","of":"partial","lang":"en","text":"The budget is five million","status":"ok","settled":false}"#,
            r#"{"type":"partial","text":"予算は五百万円を想定"}"#,
            r#"{"type":"translation","of":"partial","lang":"en","text":"The budget is five million yen expected","status":"ok","settled":false}"#,
            r#"{"type":"final","id":"mic-1","source":"mic","t0_s":1.5,"t1_s":4.0,"end_wall_ms":1790000000000,"text":"予算は五百万円を想定しています。","lang":"ja","speaker":"","latency_ms":412.6,"tier":"rz","audio_s":2.5,"lid_ms":12.0,"decode_ms":180.0,"switched":false}"#,
            r#"{"type":"translation","of":"final","id":"mic-1","source":"mic","src_lang":"ja","lang":"en","src_sha256":"00","status":"ok","hold_reason":null,"stage":"t","settled":false,"text":"We expect a budget of 5M yen.","annotations":[],"skipped":[]}"#,
            r#"{"type":"speaker","id":"mic-1","speaker":"S2"}"#,
            r#"{"type":"translation","of":"final","id":"mic-1","source":"mic","src_lang":"ja","lang":"en","src_sha256":"00","status":"ok","hold_reason":null,"stage":"e","settled":true,"text":"We are planning on a budget of 5M yen.","annotations":[{"start":29,"end":36,"text":" (about $33,000)"}],"skipped":[]}"#,
            r#"{"type":"refine","id":"mic-1","source":"mic","t0_s":1.5,"t1_s":4.0,"end_wall_ms":1790000000500,"text":"予算は五百万円を想定しています。","lang":"ja","speaker":"S2","audio_s":2.5}"#,
            r#"{"type":"translation","of":"refine","id":"mic-1","lang":"en","status":"ok","settled":true,"text":"The refined paragraph in English.","annotations":[]}"#,
            r#"{"type":"fallback","stage":"punct","to":null}"#,
        ]
        .map(String::from),
    );
    messages
}

#[test]
fn rp12_a_stream_read_over_a_socket_reaches_the_three_pages() -> Result<(), Failure> {
    let server = FakeEvents::serve(a_session())?;
    let (status, datas) = read_events(server.address, "/events")?;
    server.finish()?;
    assert_eq!(status, 200);
    assert_eq!(
        datas,
        a_session(),
        "the keep-alive comment must not become a message"
    );

    let mut model = Model::default();
    for (at, data) in datas.iter().enumerate() {
        model.apply(event(data)?, at as u64);
    }
    // The translation of the line is one row, decided by the second event.
    assert_eq!(model.finals.len(), 1);
    assert_eq!(model.finals[0].speaker, "S2");
    assert_eq!(model.finals[0].trs.len(), 1);
    assert_eq!(
        model.finals[0].trs[0].text,
        "We are planning on a budget of 5M yen."
    );
    assert_eq!(model.fallbacks.len(), 1);

    let dash = dashboard(&model);
    assert!(dash.contains("<span class=\"spk\">S2</span>"), "{dash}");
    assert!(
        dash.contains("413ms") || dash.contains("412ms") || dash.contains("<div class=\"lat\">"),
        "{dash}"
    );
    assert!(
        dash.contains("<span class=\"tr-text confirmed\">"),
        "{dash}"
    );
    assert!(
        dash.contains("<span class=\"tr-note\"> (about $33,000)</span>"),
        "{dash}"
    );
    assert!(dash.contains("The refined paragraph in English."));
    assert!(
        dash.contains("<span class=\"fb-chip\">translate → cpu (unreachable)</span>"),
        "{dash}"
    );
    assert!(
        !dash.contains("punct → cpu"),
        "the ended fallback is still drawn"
    );

    let board = transcript(&model);
    assert!(board.contains("The refined paragraph in English."));
    assert!(
        !board.contains("We are planning"),
        "a line of the feed is on the transcript"
    );
    assert!(!board.contains("fb-chip") && !board.contains("fallback"));
    Ok(())
}

#[test]
fn rp12_a_client_that_connects_again_gets_the_kept_fallbacks_once() -> Result<(), Failure> {
    let mut model = Model::default();
    for _ in 0..2 {
        let server = FakeEvents::serve(FALLBACKS.iter().map(|data| (*data).to_owned()).collect())?;
        let (_, datas) = read_events(server.address, "/events")?;
        server.finish()?;
        for data in &datas {
            model.apply(event(data)?, 0);
        }
        model.set_conn(Conn::Reconnecting);
    }
    assert_eq!(model.fallbacks.len(), 2);
    let html = overlay(&model);
    assert_eq!(count_in(&html, "id=\"fallback-badge\""), 1);
    assert!(html.contains("Running on fallback 2"), "{html}");
    let dash = dashboard(&model);
    assert_eq!(count_in(&dash, "class=\"fb-chip\""), 2);
    assert!(dash.contains("Reconnecting…"), "{dash}");
    Ok(())
}

#[test]
fn rp12_the_server_answers_another_path_with_a_status_the_client_can_read() -> Result<(), Failure> {
    let server = FakeEvents::serve(Vec::new())?;
    let (status, datas) = read_events(server.address, "/nowhere")?;
    server.finish()?;
    assert_eq!(status, 404);
    assert!(datas.is_empty());
    Ok(())
}

/// Each message type, with a marker text that appears nowhere else, and the
/// pages that show it: overlay, dashboard, transcript (board, section 9).
#[test]
fn rp12_each_message_type_reaches_the_pages_the_board_names() -> Result<(), Failure> {
    let line = r#"{"type":"final","id":"a-1","text":"MARK_FINAL","lang":"en","speaker":"","latency_ms":100.0}"#;
    let refine = r#"{"type":"refine","id":"a-1","text":"MARK_REFINE","lang":"en","speaker":""}"#;
    let table: [(&str, &[&str], [bool; 3]); 8] = [
        (
            "MARK_PARTIAL",
            &[r#"{"type":"partial","text":"MARK_PARTIAL"}"#],
            [true, true, false],
        ),
        ("MARK_FINAL", &[line], [true, true, false]),
        (
            "MARK_PARTIAL_TR",
            &[
                r#"{"type":"translation","of":"partial","lang":"ja","text":"MARK_PARTIAL_TR","status":"ok"}"#,
            ],
            [true, true, false],
        ),
        (
            "MARK_FINAL_TR",
            &[
                line,
                r#"{"type":"translation","of":"final","id":"a-1","lang":"ja","text":"MARK_FINAL_TR","settled":true}"#,
            ],
            [true, true, false],
        ),
        (
            "MARK_REFINE_TR",
            &[
                refine,
                r#"{"type":"translation","of":"refine","id":"a-1","lang":"ja","text":"MARK_REFINE_TR","settled":true}"#,
            ],
            [false, true, true],
        ),
        ("MARK_REFINE", &[refine], [false, true, true]),
        (
            "S9",
            &[line, r#"{"type":"speaker","id":"a-1","speaker":"S9"}"#],
            [false, true, false],
        ),
        (
            "MARK_STAGE",
            &[r#"{"type":"fallback","stage":"MARK_STAGE","to":"cpu"}"#],
            // The overlay shows that a fallback is in effect, not which.
            [false, true, false],
        ),
    ];
    for (marker, datas, shown_on) in table {
        let model = model_after(datas, 1)?;
        for (page, html, expected) in [
            ("overlay", overlay(&model), shown_on[0]),
            ("dashboard", dashboard(&model), shown_on[1]),
            ("transcript", transcript(&model), shown_on[2]),
        ] {
            assert_eq!(html.contains(marker), expected, "{marker} on the {page}");
        }
    }
    // A message that only arrives draws nothing.
    let quiet = model_after(
        &[
            r#"{"type":"session_start"}"#,
            r#"{"type":"ready","sr":16000}"#,
        ],
        1,
    )?;
    assert_eq!(overlay(&quiet), overlay(&Model::default()));
    assert_eq!(transcript(&quiet), transcript(&Model::default()));
    Ok(())
}

#[test]
fn rp12_a_withheld_translation_is_marked_on_the_dashboard_and_empty_on_the_overlay()
-> Result<(), Failure> {
    let line = r#"{"type":"final","id":"a-1","text":"原文","lang":"ja"}"#;
    for (held, mark, reason) in [
        (
            r#"{"type":"translation","of":"final","id":"a-1","lang":"en","status":"held","hold_reason":"number","text":"","settled":true}"#,
            "Translation withheld",
            Some("Numbers differ"),
        ),
        (
            r#"{"type":"translation","of":"final","id":"a-1","lang":"en","status":"held","hold_reason":"unavailable","text":"","settled":true}"#,
            "No translation",
            None,
        ),
    ] {
        let model = model_after(&[line, held], 1)?;
        let dash = dashboard(&model);
        assert!(
            dash.contains(&format!("<span class=\"tr-mark\">{mark}</span>")),
            "{dash}"
        );
        assert_eq!(
            dash.contains("<span class=\"tr-reason\">"),
            reason.is_some(),
            "{dash}"
        );
        if let Some(word) = reason {
            assert!(dash.contains(word));
        }
        let over = overlay(&model);
        assert!(over.contains("<span id=\"final-tr\"></span>"), "{over}");
        assert!(!over.contains(mark));
    }
    Ok(())
}

#[test]
fn rp12_a_translation_of_a_partial_is_split_into_the_settled_and_the_faint() -> Result<(), Failure>
{
    let model = model_after(
        &[
            r#"{"type":"translation","of":"partial","lang":"ja","text":"予算は五百万","status":"ok"}"#,
            r#"{"type":"translation","of":"partial","lang":"ja","text":"予算は五百万円を想定","status":"ok"}"#,
        ],
        1,
    )?;
    let dash = dashboard(&model);
    assert!(
        dash.contains("<span>予算は五百万</span><span class=\"tr-rest\">円を想定</span>"),
        "{dash}"
    );
    let over = overlay(&model);
    assert!(
        over.contains("<span>予算は五百万</span><span class=\"tr-faint\">円を想定</span>"),
        "{over}"
    );
    // The first translation has nothing before it to share, so all of it is faint.
    let first = model_after(
        &[r#"{"type":"translation","of":"partial","lang":"ja","text":"予算は","status":"ok"}"#],
        1,
    )?;
    assert!(dashboard(&first).contains("<span class=\"tr-rest\">予算は</span>"));
    Ok(())
}

#[test]
fn rp12_an_early_translation_is_replaced_in_the_same_row_by_the_decided_one() -> Result<(), Failure>
{
    let line = r#"{"type":"final","id":"a-1","text":"原文","lang":"ja"}"#;
    let early = r#"{"type":"translation","of":"final","id":"a-1","lang":"en","text":"early","settled":false}"#;
    let decided = r#"{"type":"translation","of":"final","id":"a-1","lang":"en","text":"decided","settled":true}"#;
    let first = dashboard(&model_after(&[line, early], 1)?);
    assert!(
        first.contains(
            "<span class=\"tr-text\"><span>early</span></span><span class=\"tr-mark\">Early</span>"
        ),
        "{first}"
    );
    let second = dashboard(&model_after(&[line, early, decided], 1)?);
    assert!(
        second.contains("<span class=\"tr-text confirmed\"><span>decided</span></span>"),
        "{second}"
    );
    assert_eq!(count_in(&second, "class=\"tr c-en\""), 1);
    assert!(!second.contains("Early") && !second.contains("early"));
    // A draft that arrives after the decision changes nothing.
    let late = dashboard(&model_after(&[line, decided, early], 1)?);
    assert_eq!(late, second);
    Ok(())
}

const PRODUCT_NAME: &str = "Realtime Translator";

#[test]
fn the_dashboard_header_names_the_product_in_one_text_in_both_locales() {
    for locale in [Locale::Ja, Locale::En] {
        let html = render(&Model::default(), Page::Dashboard, locale);
        let name = format!("<div class=\"logotype\">{PRODUCT_NAME}</div>");
        assert_eq!(count_in(&html, &name), 1, "{locale:?}: {html}");
        // The old two-part mark and its romanisation are gone from the header.
        for gone in ["hayamimi", "早", "耳", "class=\"romaji\""] {
            assert!(!html.contains(gone), "{locale:?} still has {gone}: {html}");
        }
    }
}

#[test]
fn the_tab_title_of_the_shell_page_is_the_product_name() {
    const SHELL: &str = include_str!("../web/index.html");
    assert_eq!(
        count_in(SHELL, &format!("<title>{PRODUCT_NAME}</title>")),
        1
    );
    assert_eq!(count_in(SHELL, "<title>"), 1);
}
