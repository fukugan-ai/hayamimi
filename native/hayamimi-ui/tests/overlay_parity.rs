//! The overlay is the same page as the mobile server's `overlayHtml` while no
//! translation and no fallback has been seen: the same element tree, the same
//! rules for the same selectors, and the same behaviour for the same messages.
//!
//! `overlay_html_matches_mobile_port` is the test the Python
//! `test_overlay_html_matches_mobile_port` becomes (RP-19). The page is drawn
//! to a string and compared with the Dart file; the script of the Dart page is
//! checked for the lines the Rust page's behaviour was written from, and the
//! behaviour is compared with a transcription of that script.

mod support;

use std::collections::BTreeMap;
use std::path::Path;

use hayamimi_ui::css;
use hayamimi_ui::model::{Model, OVERLAY_HOLD_MS};
use hayamimi_ui::overlay::Show;
use hayamimi_ui::route::{Page, page_for};
use hayamimi_ui::strings::Locale;
use hayamimi_ui::wire::UiEvent;
use support::{Failure, event, model_after, render};

type Sheet = BTreeMap<String, BTreeMap<String, String>>;

fn dart_page() -> Result<String, Failure> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../mobile/hayamimi_core/lib/server/overlay_html.dart");
    let source =
        std::fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let (_, rest) = source
        .split_once("const String overlayHtml = '''\n")
        .ok_or("no overlayHtml in the Dart file")?;
    let (page, _) = rest.split_once("''';").ok_or("overlayHtml is not closed")?;
    Ok(page.to_owned())
}

fn between<'a>(text: &'a str, open: &str, close: &str) -> Result<&'a str, Failure> {
    let (_, rest) = text.split_once(open).ok_or_else(|| format!("no {open}"))?;
    let (inside, _) = rest
        .split_once(close)
        .ok_or_else(|| format!("no {close} after {open}"))?;
    Ok(inside)
}

fn squeeze(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The rules of a sheet that has no at-rules, by selector, merged when a
/// selector is written twice.
fn rules(sheet: &str) -> Sheet {
    let mut found = Sheet::new();
    for chunk in sheet.split('}') {
        let Some((selector, body)) = chunk.split_once('{') else {
            continue;
        };
        let mut names: Vec<String> = selector.split(',').map(squeeze).collect();
        names.sort();
        let entry = found.entry(names.join(", ")).or_default();
        for declaration in body.split(';').filter(|piece| !piece.trim().is_empty()) {
            if let Some((property, value)) = declaration.split_once(':') {
                entry.insert(squeeze(property), squeeze(value));
            }
        }
    }
    found
}

#[test]
fn overlay_html_matches_mobile_port() -> Result<(), Failure> {
    let dart = dart_page()?;
    let ours = render(&Model::default(), Page::Overlay(Show::Both), Locale::Ja);

    // P1: the element tree is `div#box` holding the two spans and nothing else.
    let dart_box = format!(
        "<div id=\"box\">{}</div>",
        between(&dart, "<div id=\"box\">", "</div>")?
    );
    let our_box = format!(
        "<div id=\"box\">{}</div>",
        between(&ours, "<div id=\"box\">", "</div>")?
    );
    assert_eq!(our_box, dart_box);
    assert_eq!(
        dart_box,
        "<div id=\"box\"><span id=\"final-line\"></span><span id=\"partial-line\"></span></div>"
    );

    // P2, P3, P4: every rule of the Dart sheet is ours, declaration for declaration.
    let theirs = rules(between(&dart, "<style>", "</style>")?);
    let mine = rules(&css::overlay());
    assert_eq!(theirs.len(), 4, "{theirs:?}");
    for (selector, declarations) in &theirs {
        assert_eq!(mine.get(selector), Some(declarations), "{selector}");
    }

    // P7: until a message arrives there is nothing but the two empty lines.
    for absent in [
        "id=\"fallback-badge\"",
        "id=\"final-tr\"",
        "id=\"partial-tr\"",
        "<img",
        "<svg",
    ] {
        assert!(
            !ours.contains(absent),
            "{absent} is drawn before any message"
        );
    }
    Ok(())
}

#[test]
fn a_change_of_one_declaration_is_told_from_the_dart_sheet() -> Result<(), Failure> {
    // The negative control of BD-1: the size of the text moved by a tenth.
    let dart = dart_page()?;
    let theirs = rules(between(&dart, "<style>", "</style>")?);
    let moved = css::overlay().replace("font-size: 5vh", "font-size: 5.1vh");
    let mine = rules(&moved);
    assert_ne!(mine.get("#box"), theirs.get("#box"));
    Ok(())
}

#[test]
fn the_overlay_page_leaves_the_ground_to_the_page_behind_it() {
    let sheet = css::overlay();
    assert_eq!(rules(&sheet)["body, html"]["background"], "transparent");
    assert!(!sheet.contains("var(--ink"));
}

/// The script of the Dart page, line by line, as the Rust page was written
/// from it. A change to the Dart script that is not made here fails this test.
#[test]
fn the_dart_script_still_says_what_the_rust_page_does() -> Result<(), Failure> {
    let dart = dart_page()?;
    let script = between(&dart, "<script>", "</script>")?;
    let lines: Vec<&str> = script.lines().map(str::trim).collect();
    for expected in [
        r#"const mode = new URLSearchParams(location.search).get("show") || "both";"#,
        r#"if (mode === "final") par.style.display = "none";"#,
        r#"if (mode === "partial") fin.style.display = "none";"#,
        r#"const es = new EventSource("/events");"#,
        "if (clearTimer) { clearTimeout(clearTimer); clearTimer = null; }",
        r#"if (ev.type === "partial") {"#,
        "par.textContent = ev.text;",
        r#"} else if (ev.type === "final") {"#,
        "fin.textContent = ev.text;",
        r#"par.textContent = "";"#,
        r#"clearTimer = setTimeout(() => { fin.textContent = ""; }, 6000);"#,
    ] {
        assert!(
            lines.contains(&expected),
            "the Dart script lost: {expected}"
        );
    }
    assert_eq!(OVERLAY_HOLD_MS, 6000);
    Ok(())
}

/// What the two lines hold at an instant.
type Lines = (String, String);

/// The Dart script, transcribed: it holds two strings and one timer that every
/// message sets again for `hold_ms`, and that empties only the confirmed one.
struct Reference {
    hold_ms: u64,
    fin: String,
    par: String,
    clear_at: Option<u64>,
}

impl Reference {
    fn message(&mut self, ev: &UiEvent, now: u64) {
        match ev {
            UiEvent::Partial { text } => self.par.clone_from(text),
            UiEvent::Final(found) => {
                self.fin.clone_from(&found.text);
                self.par.clear();
            }
            _ => {}
        }
        self.clear_at = Some(now + self.hold_ms);
    }

    fn time(&mut self, now: u64) {
        if self.clear_at.is_some_and(|at| at <= now) {
            self.fin.clear();
            self.clear_at = None;
        }
    }
}

const FINAL_LINE: &str = r#"{"type":"final","id":"mic-1","text":"確定の文","lang":"ja","speaker":"","latency_ms":300.0}"#;

/// The sequence of BD-2: a partial, a final, quiet for nearly six seconds,
/// partials, a message that is neither, a wait, and a last final.
const SCRIPT: [(u64, &str); 10] = [
    (0, r#"{"type":"partial","text":"こん"}"#),
    (400, FINAL_LINE),
    (5900, r#"{"type":"partial","text":"次"}"#),
    (6500, r#"{"type":"partial","text":"次の"}"#),
    (7000, r#"{"type":"session_start"}"#),
    (9000, r#"{"type":"partial","text":"次の文"}"#),
    (9500, r#"{"type":"partial","text":"次の文を"}"#),
    (9800, r#"{"type":"partial","text":"次の文を話"}"#),
    (17000, r#"{"type":"fallback","stage":"punct","to":"cpu"}"#),
    (
        24000,
        r#"{"type":"final","text":"二つ目","lang":"ja","latency_ms":null}"#,
    ),
];

/// The instants a page is looked at: just before each message, at it, just
/// after it, and well past the last.
fn instants() -> Vec<u64> {
    let mut at: Vec<u64> = SCRIPT
        .iter()
        .flat_map(|(time, _)| [time.saturating_sub(1), *time, time + 1])
        .collect();
    at.extend([11_000, 16_000, 29_999, 30_000, 40_000]);
    at.sort_unstable();
    at.dedup();
    at
}

fn trace_of_the_rust_page() -> Result<Vec<Lines>, Failure> {
    let mut page = Model::default();
    let mut seen = Vec::new();
    for now in instants() {
        for (time, data) in SCRIPT.iter().filter(|(time, _)| *time == now) {
            page.expire(*time);
            page.apply(event(data)?, *time);
        }
        page.expire(now);
        seen.push((
            page.overlay.final_text.clone(),
            page.overlay.partial_text.clone(),
        ));
    }
    Ok(seen)
}

fn trace_of_the_script(hold_ms: u64) -> Result<Vec<Lines>, Failure> {
    let mut page = Reference {
        hold_ms,
        fin: String::new(),
        par: String::new(),
        clear_at: None,
    };
    let mut seen = Vec::new();
    for now in instants() {
        for (time, data) in SCRIPT.iter().filter(|(time, _)| *time == now) {
            page.time(*time);
            page.message(&event(data)?, *time);
        }
        page.time(now);
        seen.push((page.fin.clone(), page.par.clone()));
    }
    Ok(seen)
}

#[test]
fn the_rust_page_holds_what_the_dart_page_holds_at_every_instant() -> Result<(), Failure> {
    let mine = trace_of_the_rust_page()?;
    let theirs = trace_of_the_script(6000)?;
    assert!(mine.len() > 30, "{}", mine.len());
    assert_eq!(mine, theirs);
    // The trace shows a confirmed line, an emptied one and a partial line.
    assert!(mine.iter().any(|(fin, _)| fin == "確定の文"));
    assert!(
        mine.iter()
            .any(|(fin, par)| fin.is_empty() && par == "次の文を話")
    );
    Ok(())
}

#[test]
fn a_timer_of_five_or_seven_seconds_is_told_from_the_dart_script() -> Result<(), Failure> {
    // The negative control of BD-2.
    let mine = trace_of_the_rust_page()?;
    assert_ne!(mine, trace_of_the_script(5000)?);
    assert_ne!(mine, trace_of_the_script(7000)?);
    Ok(())
}

#[test]
fn show_keeps_one_line_and_every_other_value_keeps_both() -> Result<(), Failure> {
    let model = model_after(
        &[
            r#"{"type":"partial","text":"聞き取り中"}"#,
            FINAL_LINE,
            r#"{"type":"partial","text":"次"}"#,
        ],
        1,
    )?;
    let hidden = "style=\"display: none\"";
    for (query, hides_partial, hides_final) in [
        ("?show=final", true, false),
        ("?show=partial", false, true),
        ("?show=x", false, false),
        ("", false, false),
    ] {
        let html = render(&model, page_for("/", query), Locale::Ja);
        assert_eq!(
            html.contains(&format!("<span id=\"partial-line\" {hidden}")),
            hides_partial,
            "{query}"
        );
        assert_eq!(
            html.contains(&format!("<span id=\"final-line\" {hidden}")),
            hides_final,
            "{query}"
        );
    }
    Ok(())
}

#[test]
fn the_translation_lines_and_the_mark_appear_only_after_their_first_event() -> Result<(), Failure> {
    let before = model_after(&[FINAL_LINE], 10)?;
    let html = render(&before, Page::Overlay(Show::Both), Locale::En);
    assert!(!html.contains("id=\"final-tr\"") && !html.contains("id=\"fallback-badge\""));
    let after = model_after(
        &[
            FINAL_LINE,
            r#"{"type":"translation","of":"final","id":"mic-1","lang":"en","text":"The decided line","settled":true}"#,
            r#"{"type":"fallback","stage":"punct","reason":"no_device","to":"cpu"}"#,
        ],
        10,
    )?;
    let html = render(&after, Page::Overlay(Show::Both), Locale::En);
    assert!(html.contains("<span id=\"final-tr\">"), "{html}");
    assert!(html.contains("<span id=\"partial-tr\">"), "{html}");
    assert!(html.contains("The decided line"), "{html}");
    assert!(html.contains("Running on fallback"), "{html}");
    // `?show=partial` keeps the mark away and hides the confirmed translation.
    let partial = render(&after, Page::Overlay(Show::Partial), Locale::En);
    assert!(!partial.contains("id=\"fallback-badge\""));
    assert!(
        partial.contains("<span id=\"final-tr\" style=\"display: none\">"),
        "{partial}"
    );
    Ok(())
}
