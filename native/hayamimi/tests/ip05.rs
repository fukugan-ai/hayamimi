//! IP-5: one page shows both directions of translation.
//!
//! The real run (T-IP-05-001 to 004) starts `hayamimi` on a websocket input,
//! the relay, a stand-in translation server and `chrome-headless-shell`, opens
//! the dashboard with `?relay=` before the first line is played, plays rows 7
//! to 10 of `lat-dialog` (ja, en, ja, en) and reads the html of the page over
//! the DevTools protocol once the relay has settled a translation for every
//! `final` of the run. It is run once and the four rows judge what it saw, one
//! manifest row (one utterance) at a time (design 10.9.3). They are `#[ignore]`d
//! because they need the models, `testdata/` (not in git), the page bundle next
//! to the `hayamimi` binary and a browser.
//!
//! An utterance may be read as several `final`s, so no count of `final`s or of
//! translations is fixed. The page does not write the id of a line into its
//! html, so the `row` of a `final` is found by its place: the page draws the
//! `final`s in the order the stream received them, and a row is taken for the
//! `final` only when its text and language badge are the `final`'s too.
//!
//! The rows 005, 006 and 017 to 019 test the tools that read a dump of the
//! page, on html written by hand, and so do the tests of the row judgement
//! that follow them. The rows 007 to 016 read the `?relay=` of an address and
//! are in `hayamimi-ui/src/route.rs`. The record of the run is
//! `docs/results/in-person/ip05.md`.

mod support;

use std::fs::File;
use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use hayamimi_eval::manifest::{Row as ManifestRow, read_rows, write_rows};
use hayamimi_eval::wav::{SAMPLE_RATE, read_wav, write_wav};
use serde_json::Value;
use support::dialog::{
    Given, Placed, append_line, events_of, given_of, langs_of, session_start_us,
};
use support::dialog_run::sibling_binary;
use support::{BINARY, Failure, TestResult, data_events, find_above, models_dir, path_text};

/// What `fake_llama_server -m translates.gguf` says for a translation into
/// English and into Japanese.
const ENGLISH: &str = "That is a very good question.";
const JAPANESE: &str = "それはとても良い質問です。";

const FAKE_SERVER: &str = env!("CARGO_BIN_EXE_fake_llama_server");
const RECORD: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/results/in-person/ip05.md"
);
/// The utterances the run plays: four rows of `lat-dialog`, ja, en, ja, en.
const UTTERANCES: usize = 4;
/// The first of those rows, counted from 1: the first four rows whose
/// utterance is read in its language and has a segment in the IP-2 record
/// (design 10.9.3, 1). Whoever moves the IP-2 record moves this number.
const FIRST_ROW: usize = 7;
/// How long the stack is given to open its ports: the models are read first.
const START_WAIT: Duration = Duration::from_secs(300);
/// How long a browser is given to start and to draw the page.
const BROWSER_WAIT: Duration = Duration::from_secs(60);
/// How long a page that is drawn is given to connect its streams, before the
/// first line is played.
const CONNECT_GRACE: Duration = Duration::from_secs(3);
/// How long the last translation is given after the stream has ended.
const TRANSLATE_WAIT: Duration = Duration::from_secs(90);
/// How long a page is given after its last translation, for the pages that
/// should show none to show one.
const SHOW_GRACE: Duration = Duration::from_secs(2);

// ---- a dump of the page ---------------------------------------------------

/// One element of a dumped page: its tag, its class tokens, its parent, and
/// the text inside it, its descendants' included.
struct Element {
    tag: String,
    classes: Vec<String>,
    parent: Option<usize>,
    text: String,
}

/// Elements whose content is not markup.
const RAW_TEXT: [&str; 2] = ["script", "style"];
/// Elements that have no end tag.
const VOID: [&str; 14] = [
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// The end of `from`: what is left once all of it has been read.
fn consumed(from: &str) -> &str {
    &from[from.len()..]
}

/// The index of the `>` that ends the tag at the start of `from`, skipping a
/// `>` inside a quoted value.
fn tag_end(from: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (at, c) in from.char_indices() {
        match (quote, c) {
            (Some(open), c) if c == open => quote = None,
            (None, '"' | '\'') => quote = Some(c),
            (None, '>') => return Some(at),
            _ => {}
        }
    }
    None
}

/// The value of the `class` attribute in the text after a tag's name.
fn class_attribute(attributes: &str) -> Option<&str> {
    let (at, _) = attributes
        .match_indices("class=")
        .find(|(at, _)| attributes[..*at].ends_with(char::is_whitespace))?;
    let value = &attributes[at + "class=".len()..];
    match value.chars().next()? {
        quote @ ('"' | '\'') => value[1..].split(quote).next(),
        _ => value.split(char::is_whitespace).next(),
    }
}

fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// Adds `text` to every element that is open.
fn add_text(found: &mut [Element], open: &[usize], text: &str) {
    if text.is_empty() {
        return;
    }
    let text = unescape(text);
    for at in open {
        found[*at].text.push_str(&text);
    }
}

/// Closes the innermost open element called `name`, and what is open inside it.
fn close(found: &[Element], open: &mut Vec<usize>, name: &str) {
    if let Some(depth) = open.iter().rposition(|at| found[*at].tag == name) {
        open.truncate(depth);
    }
}

/// Opens the element a start tag makes; `inner` is the tag without `<` and `>`.
fn open_element(inner: &str, found: &mut Vec<Element>, open: &mut Vec<usize>) -> String {
    let tag: String = inner
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect::<String>()
        .to_ascii_lowercase();
    let classes = class_attribute(&inner[tag.len()..])
        .into_iter()
        .flat_map(str::split_whitespace)
        .map(str::to_owned)
        .collect();
    found.push(Element {
        tag: tag.clone(),
        classes,
        parent: open.last().copied(),
        text: String::new(),
    });
    if !inner.ends_with('/') && !VOID.contains(&tag.as_str()) {
        open.push(found.len() - 1);
    }
    tag
}

/// Reads the markup at the start of `from` (it begins with `<`) and returns
/// what follows it.
fn read_markup<'a>(from: &'a str, found: &mut Vec<Element>, open: &mut Vec<usize>) -> &'a str {
    if let Some(rest) = from.strip_prefix("<!--") {
        return match rest.find("-->") {
            Some(end) => &rest[end + 3..],
            None => consumed(rest),
        };
    }
    let Some(end) = tag_end(from) else {
        return consumed(from);
    };
    let (inner, after) = (&from[1..end], &from[end + 1..]);
    if inner.starts_with(['!', '?']) {
        return after;
    }
    if let Some(name) = inner.strip_prefix('/') {
        close(found, open, name.trim().to_ascii_lowercase().as_str());
        return after;
    }
    let tag = open_element(inner, found, open);
    if !RAW_TEXT.contains(&tag.as_str()) {
        return after;
    }
    let closing = format!("</{tag}");
    match after.to_ascii_lowercase().find(&closing) {
        Some(at) => &after[at..],
        None => consumed(after),
    }
}

fn parse_elements(html: &str) -> Vec<Element> {
    let mut found = Vec::new();
    let mut open = Vec::new();
    let mut rest = html;
    while !rest.is_empty() {
        let Some(at) = rest.find('<') else {
            add_text(&mut found, &open, rest);
            break;
        };
        add_text(&mut found, &open, &rest[..at]);
        rest = read_markup(&rest[at..], &mut found, &mut open);
    }
    found
}

/// What the page drew for one translation.
#[derive(Debug)]
struct Tr {
    arrow: Option<String>,
    text: Option<String>,
    mark: Option<String>,
    reason: Option<String>,
}

/// What the page drew for one confirmed line.
#[derive(Debug)]
struct Row {
    badge: Option<String>,
    text: Option<String>,
    trs: Vec<Tr>,
}

/// The rows of a dumped dashboard, and how many `tr` elements the page holds
/// anywhere. Only the `tr` inside a row are judged: the refined paragraphs
/// hold some outside any row.
#[derive(Debug)]
struct Dump {
    rows: Vec<Row>,
    trs_total: usize,
}

fn has_class(element: &Element, class: &str) -> bool {
    element.classes.iter().any(|token| token == class)
}

/// Whether element `at` is `ancestor` or inside it.
fn inside(found: &[Element], at: usize, ancestor: usize) -> bool {
    std::iter::successors(Some(at), |current| found[*current].parent)
        .any(|current| current == ancestor)
}

/// The text of the first element of class `class` inside element `within`.
fn text_of(found: &[Element], within: usize, class: &str) -> Option<String> {
    found
        .iter()
        .enumerate()
        .find(|(at, element)| has_class(element, class) && inside(found, *at, within))
        .map(|(_, element)| element.text.clone())
}

fn tr_at(found: &[Element], at: usize) -> Tr {
    Tr {
        arrow: text_of(found, at, "tr-arrow"),
        text: text_of(found, at, "tr-text"),
        mark: text_of(found, at, "tr-mark"),
        reason: text_of(found, at, "tr-reason"),
    }
}

fn row_at(found: &[Element], at: usize) -> Row {
    let trs = found
        .iter()
        .enumerate()
        .filter(|(inner, element)| has_class(element, "tr") && inside(found, *inner, at))
        .map(|(inner, _)| tr_at(found, inner))
        .collect();
    Row {
        badge: text_of(found, at, "lang-badge"),
        text: text_of(found, at, "row-text"),
        trs,
    }
}

/// Reads a dump of the dashboard. A dump with no element of class `dash` is
/// of a page that was not drawn, and is an error rather than zero rows.
fn read_dump(html: &str) -> Result<Dump, Failure> {
    let found = parse_elements(html);
    if !found.iter().any(|element| has_class(element, "dash")) {
        return Err(String::from(
            "the dump has no element of class `dash`: the page was not drawn",
        ));
    }
    let rows = found
        .iter()
        .enumerate()
        .filter(|(_, element)| has_class(element, "row"))
        .map(|(at, _)| row_at(&found, at))
        .collect();
    let trs_total = found
        .iter()
        .filter(|element| has_class(element, "tr"))
        .count();
    Ok(Dump { rows, trs_total })
}

/// The dump has `rows` rows and `per_row` translations inside each of them.
/// A `tr` outside a row is not counted.
fn check_shape(dump: &Dump, rows: usize, per_row: usize) -> Result<(), Failure> {
    if dump.rows.len() != rows {
        return Err(format!("{} rows, not {rows}", dump.rows.len()));
    }
    for (at, row) in dump.rows.iter().enumerate() {
        if row.trs.len() != per_row {
            return Err(format!("row {at} has {} tr, not {per_row}", row.trs.len()));
        }
    }
    Ok(())
}

// ---- what the translations should look like -------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Ok,
    Held,
}

/// How many translations of each status were judged, and whether each judgement
/// had anything to judge.
#[derive(Debug, PartialEq, Eq)]
struct Judgement {
    ok: usize,
    held: usize,
    ok_judged: bool,
    held_judged: bool,
}

/// The words a translation row is judged against.
struct Words<'a> {
    english: &'a str,
    japanese: &'a str,
    /// The mark of a withheld translation in each language of the page.
    held_marks: &'a [String],
}

fn check_ok_tr(at: usize, tr: &Tr, target: &str, words: &Words<'_>) -> Result<(), Failure> {
    let expected = match target {
        "en" => words.english,
        "ja" => words.japanese,
        other => return Err(format!("row {at}: no sentence is fixed for `{other}`")),
    };
    if tr.text.as_deref() == Some(expected) {
        Ok(())
    } else {
        Err(format!(
            "row {at}: `tr-text` is {:?}, not {expected:?}",
            tr.text
        ))
    }
}

fn check_held_tr(at: usize, tr: &Tr, held_marks: &[String]) -> Result<(), Failure> {
    if tr.text.is_some() {
        return Err(format!(
            "row {at}: a withheld translation shows `tr-text` {:?}",
            tr.text
        ));
    }
    match tr.mark.as_deref() {
        None => return Err(format!("row {at}: a withheld translation has no `tr-mark`")),
        Some(mark) if !held_marks.iter().any(|held| held == mark) => {
            return Err(format!(
                "row {at}: `tr-mark` is {mark:?}, not one of {held_marks:?}"
            ));
        }
        Some(_) => {}
    }
    if tr
        .reason
        .as_deref()
        .is_some_and(|reason| reason.trim().is_empty())
    {
        return Err(format!("row {at}: `tr-reason` is empty"));
    }
    Ok(())
}

/// Judges the translation of each row of the dump against the status and the
/// target language the relay settled it with. A judgement with no row of its
/// status to judge is marked as not made, and is not a pass.
fn judge(dump: &Dump, lines: &[(Status, &str)], words: &Words<'_>) -> Result<Judgement, Failure> {
    check_shape(dump, lines.len(), 1)?;
    let (mut ok, mut held) = (0, 0);
    for (at, (row, (status, target))) in dump.rows.iter().zip(lines).enumerate() {
        let [tr] = row.trs.as_slice() else {
            return Err(format!("row {at} has {} tr", row.trs.len()));
        };
        if tr.arrow.as_deref() != Some(format!("→{target}").as_str()) {
            return Err(format!(
                "row {at}: `tr-arrow` is {:?}, not →{target}",
                tr.arrow
            ));
        }
        match status {
            Status::Ok => {
                check_ok_tr(at, tr, target, words)?;
                ok += 1;
            }
            Status::Held => {
                check_held_tr(at, tr, words.held_marks)?;
                held += 1;
            }
        }
    }
    Ok(Judgement {
        ok,
        held,
        ok_judged: ok > 0,
        held_judged: held > 0,
    })
}

/// The mark of a withheld translation in the strings of each language.
fn held_marks() -> Result<Vec<String>, Failure> {
    ["en", "ja"]
        .into_iter()
        .map(|lang| {
            let path = format!(
                "{}/../hayamimi-ui/data/strings.{lang}.toml",
                env!("CARGO_MANIFEST_DIR")
            );
            let table = toml::from_str::<toml::Table>(&support::read(Path::new(&path))?)
                .map_err(|error| format!("{path}: {error}"))?;
            table
                .get("ui")
                .and_then(|ui| ui.get("tr"))
                .and_then(|tr| tr.get("held"))
                .and_then(toml::Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| format!("{path} has no ui.tr.held"))
        })
        .collect()
}

// ---- the events a run produced --------------------------------------------

fn is_settled(event: &Value) -> bool {
    event.get("settled").and_then(Value::as_bool) == Some(true)
}

fn text_field<'a>(event: &'a Value, key: &str) -> Result<&'a str, Failure> {
    event
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("an event has no string `{key}`: {event}"))
}

/// The status a settled translation was published with. A translation with no
/// `status` is `ok`, as the page reads it.
fn status_of(translation: &Value) -> Result<Status, Failure> {
    match translation.get("status").and_then(Value::as_str) {
        Some("ok") => Ok(Status::Ok),
        Some("held") => Ok(Status::Held),
        // DEFAULT: the wire of the page (`hayamimi-ui/src/wire.rs`, `Status`) reads a translation with no `status` as `ok`.
        None => Ok(Status::Ok),
        Some(other) => Err(format!("a translation has the status `{other}`")),
    }
}

/// The language a line in `lang` is translated into.
fn opposite(lang: &str) -> Result<&'static str, Failure> {
    match lang {
        "ja" => Ok("en"),
        "en" => Ok("ja"),
        other => Err(format!(
            "`{other}` is neither ja nor en: it has no opposite"
        )),
    }
}

/// One `final` the stream received, as the page draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Seen {
    id: String,
    lang: String,
    text: String,
}

/// The `final`s of the events file of a run, in the order they were received.
fn finals_seen(events_text: &str) -> Result<Vec<Seen>, Failure> {
    events_of(events_text)?
        .iter()
        .filter(|line| line.body.get("type").and_then(Value::as_str) == Some("final"))
        .map(|line| {
            Ok(Seen {
                id: text_field(&line.body, "id")?.to_owned(),
                lang: text_field(&line.body, "lang")?.to_owned(),
                text: text_field(&line.body, "text")?.to_owned(),
            })
        })
        .collect()
}

/// For each row, the last `final` it was given in the language of its
/// manifest row (design 10.9.3, 2 (a)); its id is the one the translation is
/// looked for by. A row with none means the identifier missed and not the
/// page: every such row is named, with the languages it was given, and the run
/// is not judged.
fn last_own_finals<'a>(langs: &[String], given: &'a [Given]) -> Result<Vec<&'a Placed>, Failure> {
    if langs.len() != given.len() {
        return Err(format!(
            "the manifest has {} rows and the run {} utterances",
            langs.len(),
            given.len()
        ));
    }
    let mut last = Vec::new();
    let mut lacking = Vec::new();
    for (at, (want, own)) in langs.iter().zip(given).enumerate() {
        match own.finals.iter().rev().find(|done| done.lang == *want) {
            Some(done) => last.push(done),
            None => {
                let heard: Vec<&str> = own.finals.iter().map(|done| done.lang.as_str()).collect();
                lacking.push(format!(
                    "row {} ({want}) was given {heard:?}",
                    FIRST_ROW + at
                ));
            }
        }
    }
    if lacking.is_empty() {
        Ok(last)
    } else {
        Err(format!(
            "no final in the language of its row: {}: IP-5 is not judged",
            lacking.join("; ")
        ))
    }
}

/// The last settled `of: final` translation the relay published for line `id`.
/// A translation of a refined paragraph (`of: refine`) is not one.
fn translation_of<'a>(relay: &'a [Value], id: &str) -> Option<&'a Value> {
    relay.iter().rev().find(|event| {
        event.get("type").and_then(Value::as_str) == Some("translation")
            && event.get("of").and_then(Value::as_str) == Some("final")
            && is_settled(event)
            && event.get("id").and_then(Value::as_str) == Some(id)
    })
}

/// Manifest row `row` has a settled translation for its last `final` in its
/// own language `want`, into the opposite language (design 10.9.3, 2 (b)).
fn check_translated(row: usize, want: &str, id: &str, relay: &[Value]) -> Result<(), Failure> {
    let target = opposite(want)?;
    let Some(found) = translation_of(relay, id) else {
        return Err(format!(
            "row {row}: the relay published no settled `of: final` translation for {id}"
        ));
    };
    let lang = text_field(found, "lang")?;
    if lang == target {
        Ok(())
    } else {
        Err(format!(
            "row {row}: the translation of {id} is into `{lang}`, not `{target}`"
        ))
    }
}

/// Every row has a settled translation for its last `final` in its language.
fn check_rows_translated(
    langs: &[String],
    last: &[&Placed],
    relay: &[Value],
) -> Result<(), Failure> {
    for (at, (want, done)) in langs.iter().zip(last).enumerate() {
        let row = FIRST_ROW + at;
        let id = done
            .id
            .as_deref()
            .ok_or_else(|| format!("row {row}: its last final has no id"))?;
        check_translated(row, want, id, relay)?;
    }
    Ok(())
}

/// The ids of the `final`s that the relay has published no settled
/// `of: final` translation for yet.
fn missing_translations<'a>(relay: &[Value], finals: &'a [Seen]) -> Vec<&'a str> {
    finals
        .iter()
        .filter(|done| translation_of(relay, &done.id).is_none())
        .map(|done| done.id.as_str())
        .collect()
}

/// The page draws a `row` for each `final` the stream received, in the order
/// it received them. A row is taken for a `final` when its text and its
/// language badge are the `final`'s, and it holds exactly one `tr`
/// (design 10.9.3, 2 (c)).
fn check_page_rows(dump: &Dump, finals: &[Seen]) -> Result<(), Failure> {
    if dump.rows.len() != finals.len() {
        return Err(format!(
            "the page has {} rows for {} finals",
            dump.rows.len(),
            finals.len()
        ));
    }
    for (row, done) in dump.rows.iter().zip(finals) {
        let same_line = row.badge.as_deref() == Some(done.lang.as_str())
            && row.text.as_deref().map(str::trim) == Some(done.text.trim());
        if !same_line {
            return Err(format!(
                "the row at the place of {} is {:?} {:?}, not {:?} {:?}",
                done.id, row.badge, row.text, done.lang, done.text
            ));
        }
        if row.trs.len() != 1 {
            return Err(format!(
                "the row of {} has {} tr, not 1",
                done.id,
                row.trs.len()
            ));
        }
    }
    Ok(())
}

/// The arrow of each `tr` points away from the badge of its row
/// (design 10.9.3, 5).
fn check_arrows_oppose_badges(dump: &Dump) -> Result<(), Failure> {
    for (at, row) in dump.rows.iter().enumerate() {
        let badge = row
            .badge
            .as_deref()
            .ok_or_else(|| format!("row {at} has no language badge"))?;
        let want = format!("→{}", opposite(badge)?);
        for tr in &row.trs {
            if tr.arrow.as_deref() != Some(want.as_str()) {
                return Err(format!(
                    "row {at}: the badge is {badge}, the arrow {:?}, not {want}",
                    tr.arrow
                ));
            }
        }
    }
    Ok(())
}

/// The status and the target language of the settled translation of each
/// `final`, in the order of the rows of the page.
fn lines_for<'a>(finals: &[Seen], relay: &'a [Value]) -> Result<Vec<(Status, &'a str)>, Failure> {
    finals
        .iter()
        .map(|done| {
            let found = translation_of(relay, &done.id).ok_or_else(|| {
                format!(
                    "the relay published no settled `of: final` translation for {}",
                    done.id
                )
            })?;
            Ok((status_of(found)?, text_field(found, "lang")?))
        })
        .collect()
}

// ---- the stack and the pages ----------------------------------------------

/// A process that is stopped and waited for when it goes out of scope, its
/// output in a log file.
struct Proc(Child);

impl Proc {
    fn start(binary: &str, args: &[&str], log: &Path) -> Result<Proc, Failure> {
        let out = File::create(log).map_err(|error| format!("{}: {error}", log.display()))?;
        let err = out
            .try_clone()
            .map_err(|error| format!("{}: {error}", log.display()))?;
        Command::new(binary)
            .args(args)
            .stdin(Stdio::null())
            .stdout(out)
            .stderr(err)
            .spawn()
            .map(Proc)
            .map_err(|error| format!("cannot start {binary}: {error}"))
    }
}

impl Drop for Proc {
    fn drop(&mut self) {
        // A process that already ended has nothing to kill; the wait reaps it either way.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait_for_port(port: u16, within: Duration) -> Result<(), Failure> {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        if TcpStream::connect_timeout(&address, Duration::from_millis(200)).is_ok() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Err(format!("port {port} did not open within {within:?}"))
}

/// The ports of the run, which the system chooses and which differ.
struct Ports {
    events: u16,
    ingest: u16,
    llm: u16,
    relay: u16,
    /// The debugging port of each of the three browsers.
    browsers: [u16; 3],
}

fn free_ports() -> Result<Ports, Failure> {
    // All are held at once, so that no two are the same port.
    let held = (0..7)
        .map(|_| TcpListener::bind((Ipv4Addr::LOCALHOST, 0)))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let numbers = held
        .iter()
        .map(|listener| listener.local_addr().map(|address| address.port()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let [events, ingest, llm, relay, first, second, third] = numbers[..] else {
        return Err(String::from("seven ports were not made"));
    };
    Ok(Ports {
        events,
        ingest,
        llm,
        relay,
        browsers: [first, second, third],
    })
}

/// The programs and files of the run, found before anything is started.
struct Tools {
    relay: String,
    eval: PathBuf,
    shell: PathBuf,
    models: String,
}

/// The newest `chrome-headless-shell` in the cache of the browser installer.
fn headless_shell() -> Result<PathBuf, Failure> {
    let cache = dirs::home_dir()
        .ok_or("no home directory")?
        .join(".cache/ms-playwright");
    let entries =
        std::fs::read_dir(&cache).map_err(|error| format!("{}: {error}", cache.display()))?;
    let mut shells: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("chromium_headless_shell-")
        })
        .map(|entry| {
            entry
                .path()
                .join("chrome-headless-shell-linux64/chrome-headless-shell")
        })
        .filter(|path| path.is_file())
        .collect();
    shells.sort();
    shells
        .pop()
        .ok_or_else(|| format!("no chrome-headless-shell under {}", cache.display()))
}

/// The page bundle the `hayamimi` binary serves stands beside the binary.
fn check_bundle() -> Result<(), Failure> {
    let bundle = Path::new(BINARY)
        .parent()
        .ok_or("the binary has no directory")?
        .join("ui/hayamimi-ui.js");
    if bundle.is_file() {
        Ok(())
    } else {
        Err(format!(
            "{} is missing; build it: `cargo build --release --target wasm32-unknown-unknown -p hayamimi-ui --features web` and `wasm-bindgen --target web --no-typescript --out-dir target/release/ui --out-name hayamimi-ui target/wasm32-unknown-unknown/release/hayamimi-ui.wasm`",
            bundle.display()
        ))
    }
}

impl Tools {
    fn find() -> Result<Tools, Failure> {
        check_bundle()?;
        Ok(Tools {
            relay: path_text(&sibling_binary("hayamimi-relay")?)?,
            eval: sibling_binary("hayamimi-eval")?,
            shell: headless_shell()?,
            models: path_text(&models_dir()?)?,
        })
    }
}

/// `hayamimi`, the stand-in translation server and the relay in front of it.
struct Stack {
    _fake: Proc,
    _hayamimi: Proc,
    _relay: Proc,
}

impl Stack {
    fn start(tools: &Tools, ports: &Ports, work: &Path) -> Result<Stack, Failure> {
        let llm = ports.llm.to_string();
        let fake = Proc::start(
            FAKE_SERVER,
            &["-m", "translates.gguf", "--port", &llm],
            &work.join("fake.log"),
        )?;
        wait_for_port(ports.llm, START_WAIT)?;
        let (events, ingest) = (ports.events.to_string(), ports.ingest.to_string());
        let hayamimi = Proc::start(
            BINARY,
            &[
                "--input",
                "ws",
                "--ws-port",
                &ingest,
                "--mode",
                "pair",
                "--source",
                "file",
                "--models",
                &tools.models,
                "--serve",
                &events,
            ],
            &work.join("hayamimi.log"),
        )?;
        wait_for_port(ports.events, START_WAIT)?;
        wait_for_port(ports.ingest, START_WAIT)?;
        let source = format!("http://127.0.0.1:{}/events", ports.events);
        let translator = format!("http://127.0.0.1:{}", ports.llm);
        let serve = ports.relay.to_string();
        let relay = Proc::start(
            &tools.relay,
            &[
                "live",
                "--events",
                &source,
                "--llm",
                &translator,
                "--serve",
                &serve,
            ],
            &work.join("relay.log"),
        )?;
        wait_for_port(ports.relay, START_WAIT)?;
        Ok(Stack {
            _fake: fake,
            _hayamimi: hayamimi,
            _relay: relay,
        })
    }
}

/// A client of an `/events` stream. It is connected before the lines are
/// played in, because a client that connects later is not sent what came
/// before.
struct Events {
    stream: TcpStream,
    received: Vec<u8>,
}

impl Events {
    fn open(port: u16) -> Result<Events, Failure> {
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port))
            .map_err(|error| format!("cannot connect to port {port}: {error}"))?;
        stream
            .write_all(b"GET /events HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .map_err(|error| error.to_string())?;
        stream
            .set_read_timeout(Some(Duration::from_millis(500)))
            .map_err(|error| error.to_string())?;
        Ok(Events {
            stream,
            received: Vec::new(),
        })
    }

    /// The events received so far.
    fn events(&self) -> Vec<Value> {
        data_events(&String::from_utf8_lossy(&self.received))
    }

    /// Reads until `done` holds of the events so far, or `within` passes.
    fn read_until(
        &mut self,
        within: Duration,
        done: impl Fn(&[Value]) -> bool,
    ) -> Result<Vec<Value>, Failure> {
        let deadline = Instant::now() + within;
        let mut chunk = [0_u8; 8192];
        loop {
            let events = self.events();
            if done(&events) {
                return Ok(events);
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "only {} events came within {within:?}",
                    events.len()
                ));
            }
            match self.stream.read(&mut chunk) {
                Ok(0) => return Err(String::from("the stream closed")),
                Ok(got) => self.received.extend_from_slice(&chunk[..got]),
                Err(error)
                    if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                Err(error) => return Err(error.to_string()),
            }
        }
    }
}

/// The samples of a row of the manifest at `key`.
fn sample_of(row: &ManifestRow, key: &str) -> Result<usize, Failure> {
    let at = row
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("a row has no integer `{key}`"))?;
    usize::try_from(at).map_err(|error| error.to_string())
}

/// `row` with its samples counted from sample `from` of the recording.
fn rebased(row: &ManifestRow, from: usize) -> Result<ManifestRow, Failure> {
    let mut moved = row.clone();
    for key in ["start_sample", "end_sample"] {
        let at = sample_of(row, key)?
            .checked_sub(from)
            .ok_or_else(|| format!("`{key}` of a row lies before the cut"))?;
        moved.insert(String::from(key), Value::from(at));
    }
    Ok(moved)
}

/// The part of `lat-dialog` the run plays: the samples from the end of the row
/// before the window to the end of its last row, and the rows of the window
/// with their samples counted from the start of the cut (design 10.9.3, 1).
#[derive(Debug)]
struct Cut {
    from: usize,
    to: usize,
    rows: Vec<ManifestRow>,
}

fn cut_of(rows: &[ManifestRow]) -> Result<Cut, Failure> {
    let first = FIRST_ROW - 1;
    let last = first + UTTERANCES;
    let window = rows
        .get(first..last)
        .ok_or_else(|| format!("lat-dialog has fewer than {last} utterances"))?;
    let langs = langs_of(window)?;
    if langs != ["ja", "en", "ja", "en"] {
        return Err(format!(
            "rows {FIRST_ROW} to {last} of lat-dialog are {langs:?}, not ja, en, ja, en"
        ));
    }
    let before = rows
        .get(first - 1)
        .ok_or("the window has no row before it")?;
    let tail = window.last().ok_or("the window has no row")?;
    let from = sample_of(before, "end_sample")?;
    Ok(Cut {
        from,
        to: sample_of(tail, "end_sample")?,
        rows: window
            .iter()
            .map(|row| rebased(row, from))
            .collect::<Result<_, _>>()?,
    })
}

/// The window of `lat-dialog` as a wav and its four-row manifest, and the
/// language of each row.
struct Clip {
    wav: PathBuf,
    manifest: PathBuf,
    langs: Vec<String>,
}

fn dialog_window(work: &Path) -> Result<Clip, Failure> {
    let rows = read_rows(&find_above("testdata/manifests/lat-dialog.json")?)
        .map_err(|error| error.to_string())?;
    let cut = cut_of(&rows)?;
    let whole =
        read_wav(&find_above("testdata/lat-dialog.wav")?).map_err(|error| error.to_string())?;
    let samples = whole
        .samples
        .get(cut.from..cut.to)
        .ok_or("lat-dialog.wav is shorter than its manifest")?;
    let clip = Clip {
        wav: work.join("window.wav"),
        manifest: work.join("window.json"),
        langs: langs_of(&cut.rows)?,
    };
    write_wav(&clip.wav, SAMPLE_RATE, samples).map_err(|error| error.to_string())?;
    write_rows(&clip.manifest, &cut.rows).map_err(|error| error.to_string())?;
    Ok(clip)
}

/// Plays the clip into `/ingest` with `hayamimi-eval stream`; returns the
/// samples file of the run, which has its events file beside it.
fn play(tools: &Tools, clip: &Clip, ingest: u16, work: &Path) -> Result<PathBuf, Failure> {
    let out = work.join("played.jsonl");
    let status = Command::new(&tools.eval)
        .args(["stream", "--wav", &path_text(&clip.wav)?])
        .args(["--manifest", &path_text(&clip.manifest)?])
        .args(["--repeat", "1", "--port", &ingest.to_string()])
        .args(["--out", &path_text(&out)?])
        .status()
        .map_err(|error| format!("cannot run hayamimi-eval stream: {error}"))?;
    if status.success() {
        Ok(out)
    } else {
        Err(format!("hayamimi-eval stream ended with {status}"))
    }
}

// The browser is driven over the DevTools protocol and not asked for a dump
// of the page as it starts (`--dump-dom`): that dump is taken when the page
// has loaded, before the app has drawn anything, and with a virtual-time
// budget it is never taken while the page holds an `/events` stream open.

/// The key of the websocket handshake; the browser does not look at its value.
const WS_KEY: &str = "dGhlIHNhbXBsZSBub25jZQ==";
/// The mask of the frames this client sends; any four bytes will do.
const WS_MASK: [u8; 4] = [0x12, 0x34, 0x56, 0x78];
/// The longest frame this client reads: a dump of the page is far below it.
const WS_LONGEST: u64 = 64 * 1024 * 1024;

/// One text frame of a websocket, as a client sends it: masked.
fn text_frame(payload: &[u8]) -> Result<Vec<u8>, Failure> {
    let length = u64::try_from(payload.len()).map_err(|error| error.to_string())?;
    let bytes = length.to_be_bytes();
    let mut frame = vec![0x81];
    if length < 126 {
        frame.push(0x80 | bytes[7]);
    } else if length < 65_536 {
        frame.extend([0x80 | 126, bytes[6], bytes[7]]);
    } else {
        frame.push(0x80 | 127);
        frame.extend(bytes);
    }
    frame.extend(WS_MASK);
    frame.extend(
        payload
            .iter()
            .zip(WS_MASK.iter().cycle())
            .map(|(byte, mask)| byte ^ mask),
    );
    Ok(frame)
}

/// One frame from the server: whether it ends a message, its opcode and its payload.
fn read_frame(reader: &mut impl Read) -> Result<(bool, u8, Vec<u8>), Failure> {
    let mut head = [0_u8; 2];
    reader
        .read_exact(&mut head)
        .map_err(|error| format!("websocket: {error}"))?;
    let length = match head[1] & 0x7f {
        126 => {
            let mut size = [0_u8; 2];
            reader
                .read_exact(&mut size)
                .map_err(|error| format!("websocket: {error}"))?;
            u64::from(u16::from_be_bytes(size))
        }
        127 => {
            let mut size = [0_u8; 8];
            reader
                .read_exact(&mut size)
                .map_err(|error| format!("websocket: {error}"))?;
            u64::from_be_bytes(size)
        }
        small => u64::from(small),
    };
    if length > WS_LONGEST {
        return Err(format!("websocket: a frame of {length} bytes"));
    }
    let size = usize::try_from(length).map_err(|error| error.to_string())?;
    let mut payload = vec![0_u8; size];
    reader
        .read_exact(&mut payload)
        .map_err(|error| format!("websocket: {error}"))?;
    Ok((head[0] & 0x80 != 0, head[0] & 0x0f, payload))
}

/// The body of `GET path` from the HTTP server at `port`.
fn http_body(port: u16, path: &str) -> Result<String, Failure> {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port))
        .map_err(|error| format!("cannot connect to port {port}: {error}"))?;
    let request =
        format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(BROWSER_WAIT))
        .map_err(|error| error.to_string())?;
    let head = read_head(&mut stream).map_err(|error| format!("{path}: {error}"))?;
    let length = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())?
        })
        .ok_or_else(|| format!("{path}: the answer has no Content-Length"))?;
    let mut body = vec![0_u8; length];
    stream
        .read_exact(&mut body)
        .map_err(|error| format!("{path}: {error}"))?;
    String::from_utf8(body).map_err(|error| error.to_string())
}

/// The head of an HTTP message, up to and including the blank line, read a
/// byte at a time so that nothing of the body is taken.
fn read_head(stream: &mut TcpStream) -> Result<String, Failure> {
    let mut head = Vec::new();
    let mut byte = [0_u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        stream
            .read_exact(&mut byte)
            .map_err(|error| error.to_string())?;
        head.push(byte[0]);
    }
    String::from_utf8(head).map_err(|error| error.to_string())
}

/// The websocket path of the first page of the browser at `port`.
fn page_socket_path(port: u16) -> Result<String, Failure> {
    let targets: Value = serde_json::from_str(&http_body(port, "/json/list")?)
        .map_err(|error| format!("/json/list: {error}"))?;
    let url = targets
        .as_array()
        .into_iter()
        .flatten()
        .find(|target| target.get("type").and_then(Value::as_str) == Some("page"))
        .and_then(|target| target.get("webSocketDebuggerUrl"))
        .and_then(Value::as_str)
        .ok_or("the browser lists no page")?;
    let at = url
        .find("/devtools/")
        .ok_or("the page socket has no path")?;
    Ok(url[at..].to_owned())
}

/// A connection to one page of a browser.
struct DevTools {
    stream: TcpStream,
    next_id: u64,
}

impl DevTools {
    fn connect(port: u16, path: &str) -> Result<DevTools, Failure> {
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port))
            .map_err(|error| format!("cannot connect to port {port}: {error}"))?;
        let request = format!(
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {WS_KEY}\r\nSec-WebSocket-Version: 13\r\n\r\n"
        );
        stream
            .write_all(request.as_bytes())
            .map_err(|error| error.to_string())?;
        stream
            .set_read_timeout(Some(BROWSER_WAIT))
            .map_err(|error| error.to_string())?;
        let head =
            read_head(&mut stream).map_err(|error| format!("websocket handshake: {error}"))?;
        if !head.starts_with("HTTP/1.1 101") {
            return Err(format!("websocket handshake: {head}"));
        }
        Ok(DevTools { stream, next_id: 1 })
    }

    /// The next message of the browser, its fragments joined.
    fn read_message(&mut self) -> Result<String, Failure> {
        let mut message = Vec::new();
        loop {
            let (last, opcode, payload) = read_frame(&mut self.stream)?;
            match opcode {
                0x0 | 0x1 => message.extend(payload),
                0x8 => return Err(String::from("the browser closed the websocket")),
                _ => continue,
            }
            if last {
                return String::from_utf8(message).map_err(|error| error.to_string());
            }
        }
    }

    /// Calls `method` and returns its `result`.
    fn call(&mut self, method: &str, params: Value) -> Result<Value, Failure> {
        let id = self.next_id;
        self.next_id += 1;
        let request = serde_json::json!({"id": id, "method": method, "params": params});
        self.stream
            .write_all(&text_frame(request.to_string().as_bytes())?)
            .map_err(|error| error.to_string())?;
        loop {
            let answer: Value = serde_json::from_str(&self.read_message()?)
                .map_err(|error| format!("{method}: {error}"))?;
            if answer.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = answer.get("error") {
                return Err(format!("{method}: {error}"));
            }
            return answer
                .get("result")
                .cloned()
                .ok_or_else(|| format!("{method}: the answer has no result"));
        }
    }

    /// The html of the page as it stands now.
    fn html(&mut self) -> Result<String, Failure> {
        let result = self.call(
            "Runtime.evaluate",
            serde_json::json!({
                "expression": "document.documentElement.outerHTML",
                "returnByValue": true
            }),
        )?;
        result
            .get("result")
            .and_then(|value| value.get("value"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| format!("the page has no html: {result}"))
    }
}

/// A browser with one page open, which is closed when this goes out of scope.
struct Page {
    _browser: Proc,
    devtools: DevTools,
}

impl Page {
    fn open(shell: &Path, url: &str, port: u16, work: &Path, name: &str) -> Result<Page, Failure> {
        let debugging = format!("--remote-debugging-port={port}");
        let profile = format!(
            "--user-data-dir={}",
            path_text(&work.join(format!("{name}-profile")))?
        );
        let browser = Proc::start(
            &path_text(shell)?,
            &["--disable-gpu", &debugging, &profile, url],
            &work.join(format!("{name}.log")),
        )?;
        wait_for_port(port, BROWSER_WAIT)?;
        let path = page_socket_path(port)?;
        Ok(Page {
            _browser: browser,
            devtools: DevTools::connect(port, &path)?,
        })
    }

    /// The html of the page once `ready` holds of it, or as it stands when
    /// `within` has passed.
    fn html_when(
        &mut self,
        within: Duration,
        ready: impl Fn(&str) -> bool,
    ) -> Result<String, Failure> {
        let deadline = Instant::now() + within;
        loop {
            let html = self.devtools.html()?;
            if ready(&html) || Instant::now() >= deadline {
                return Ok(html);
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }
}

/// Whether the dashboard has been drawn.
fn is_drawn(html: &str) -> bool {
    read_dump(html).is_ok()
}

/// Whether the dashboard has `rows` confirmed lines and a translation inside
/// each of them.
fn shows_a_translation_in_each_of(rows: usize, html: &str) -> bool {
    read_dump(html).is_ok_and(|dump| {
        dump.rows.len() == rows && dump.rows.iter().all(|row| !row.trs.is_empty())
    })
}

/// Three dashboards opened before the first line: with the relay, with none,
/// and with a `javascript:` value that is no relay.
struct Pages {
    with_relay: Page,
    without_relay: Page,
    bad_relay: Page,
}

impl Pages {
    fn open(tools: &Tools, ports: &Ports, work: &Path) -> Result<Pages, Failure> {
        let page = format!("http://127.0.0.1:{}/dashboard", ports.events);
        let relay = format!("?relay=http://127.0.0.1:{}/events", ports.relay);
        let [first, second, third] = ports.browsers;
        let open = |query: &str, port: u16, name: &str| {
            Page::open(&tools.shell, &format!("{page}{query}"), port, work, name)
        };
        Ok(Pages {
            with_relay: open(&relay, first, "with-relay")?,
            without_relay: open("", second, "without-relay")?,
            bad_relay: open("?relay=javascript:x", third, "bad-relay")?,
        })
    }

    /// Waits until every page is drawn, which is after it opened its streams,
    /// and a little longer for them to connect.
    fn wait_until_drawn(&mut self) -> Result<(), Failure> {
        for page in [
            &mut self.with_relay,
            &mut self.without_relay,
            &mut self.bad_relay,
        ] {
            if !is_drawn(&page.html_when(BROWSER_WAIT, is_drawn)?) {
                return Err(format!("a page was not drawn within {BROWSER_WAIT:?}"));
            }
        }
        std::thread::sleep(CONNECT_GRACE);
        Ok(())
    }

    /// The html of the three pages: with the relay once it shows a translation
    /// inside each of `rows` lines, then the others a little later.
    fn dumps(mut self, rows: usize) -> Result<[String; 3], Failure> {
        // What the page shows by then is judged, whether or not it is all there.
        self.with_relay.html_when(TRANSLATE_WAIT, |html| {
            shows_a_translation_in_each_of(rows, html)
        })?;
        std::thread::sleep(SHOW_GRACE);
        Ok([
            self.with_relay.devtools.html()?,
            self.without_relay.devtools.html()?,
            self.bad_relay.devtools.html()?,
        ])
    }
}

/// What the run saw.
struct Observed {
    /// The language of each manifest row of the window.
    langs: Vec<String>,
    /// The `final`s each row was given, by the rule of `build_samples`.
    given: Vec<Given>,
    /// Every `final` the stream received, in that order.
    finals: Vec<Seen>,
    /// Every event the relay's `/events` published after the pages were drawn.
    relay: Vec<Value>,
    with_relay: String,
    without_relay: String,
    bad_relay: String,
}

fn scratch() -> Result<PathBuf, Failure> {
    let work = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("ip05");
    std::fs::create_dir_all(&work).map_err(|error| format!("{}: {error}", work.display()))?;
    Ok(work)
}

/// Writes the finals each row of the window was given to the record, with the
/// id of the last one in its language.
fn record_rows(langs: &[String], given: &[Given], last: &[&Placed]) -> Result<(), Failure> {
    for (at, ((want, own), done)) in langs.iter().zip(given).zip(last).enumerate() {
        let heard: Vec<String> = own
            .finals
            .iter()
            .map(|placed| format!("{} {:?}", placed.lang, placed.id))
            .collect();
        append_line(
            Path::new(RECORD),
            &format!(
                "row {} ({want}): finals {heard:?}, last in its language {:?}",
                FIRST_ROW + at,
                done.id
            ),
        )?;
    }
    Ok(())
}

/// The finals each row was given and the finals the stream received, read
/// from the files `stream --out` left.
fn read_run(played: &Path) -> Result<(Vec<Given>, Vec<Seen>), Failure> {
    let samples = support::read(played)?;
    let events = support::read(&played.with_extension("events.jsonl"))?;
    // Without it the finals cannot be placed: their clock is the server's.
    session_start_us(&events_of(&events)?).ok_or("the events file has no session_start event")?;
    Ok((given_of(&samples, &events)?, finals_seen(&events)?))
}

/// Waits until the relay has settled a translation of every `final`; the ones
/// still missing when the wait ends are named.
fn wait_for_translations(relay: &mut Events, finals: &[Seen]) -> Result<Vec<Value>, Failure> {
    relay
        .read_until(TRANSLATE_WAIT, |events| {
            missing_translations(events, finals).is_empty()
        })
        .map_err(|error| {
            let missing = missing_translations(&relay.events(), finals);
            format!("{error}; no settled `of: final` translation came for {missing:?}")
        })
}

fn run_dialog_page() -> Result<Observed, Failure> {
    let tools = Tools::find()?;
    let work = scratch()?;
    std::fs::write(RECORD, "# IP-5 record\n").map_err(|error| format!("{RECORD}: {error}"))?;
    let clip = dialog_window(&work)?;
    let ports = free_ports()?;
    let _stack = Stack::start(&tools, &ports, &work)?;
    let mut relay = Events::open(ports.relay)?;
    let mut pages = Pages::open(&tools, &ports, &work)?;
    pages.wait_until_drawn()?;
    let played = play(&tools, &clip, ports.ingest, &work)?;
    let (given, finals) = read_run(&played)?;
    let last = last_own_finals(&clip.langs, &given).inspect_err(|said| {
        // The record names the rows; the error itself is returned either way.
        let _ = append_line(Path::new(RECORD), said);
    })?;
    record_rows(&clip.langs, &given, &last)?;
    let events = wait_for_translations(&mut relay, &finals)?;
    let [with_relay, without_relay, bad_relay] = pages.dumps(finals.len())?;
    Ok(Observed {
        langs: clip.langs,
        given,
        finals,
        relay: events,
        with_relay,
        without_relay,
        bad_relay,
    })
}

/// The run is made once; the four rows that judge it share what it saw.
fn observed() -> Result<&'static Observed, Failure> {
    static RUN: OnceLock<Result<Observed, Failure>> = OnceLock::new();
    RUN.get_or_init(run_dialog_page)
        .as_ref()
        .map_err(Clone::clone)
}

fn yes_no(flag: bool) -> &'static str {
    if flag { "yes" } else { "no (nothing to judge)" }
}

/// Writes the breakdown by status to the record, and which judgements had
/// something to judge.
fn record_judgement(judgement: &Judgement) -> Result<(), Failure> {
    let record = Path::new(RECORD);
    append_line(
        record,
        &format!(
            "status of the translations on the page: ok {}, held {}",
            judgement.ok, judgement.held
        ),
    )?;
    append_line(
        record,
        &format!(
            "sentences of ok translations judged: {}",
            yes_no(judgement.ok_judged)
        ),
    )?;
    append_line(
        record,
        &format!(
            "marks of held translations judged: {}",
            yes_no(judgement.held_judged)
        ),
    )
}

/// T-IP-05-001: each row was given a `final` in its own language (the run
/// stops before this when one was not), and the page draws a `row` for every
/// `final` with exactly one `tr` inside it.
#[test]
#[ignore = "starts the hayamimi binary, the relay, a stand-in translation server and chrome-headless-shell"]
fn ip05_one_page_shows_both_directions() -> TestResult {
    let run = observed()?;
    last_own_finals(&run.langs, &run.given)?;
    check_page_rows(&read_dump(&run.with_relay)?, &run.finals)?;
    Ok(())
}

/// T-IP-05-002
#[test]
#[ignore = "starts the hayamimi binary, the relay, a stand-in translation server and chrome-headless-shell"]
fn each_direction_shows_its_arrow_and_the_translation_the_server_made() -> TestResult {
    let run = observed()?;
    let marks = held_marks()?;
    let words = Words {
        english: ENGLISH,
        japanese: JAPANESE,
        held_marks: &marks,
    };
    let dump = read_dump(&run.with_relay)?;
    check_page_rows(&dump, &run.finals)?;
    check_arrows_oppose_badges(&dump)?;
    let judgement = judge(&dump, &lines_for(&run.finals, &run.relay)?, &words)?;
    record_judgement(&judgement)?;
    Ok(())
}

/// T-IP-05-003: the pages without a relay draw the same rows, with no `tr`
/// inside any of them.
#[test]
#[ignore = "starts the hayamimi binary, the relay, a stand-in translation server and chrome-headless-shell"]
fn a_page_without_relay_in_its_address_shows_no_translation() -> TestResult {
    let run = observed()?;
    let rows = read_dump(&run.with_relay)?.rows.len();
    assert!(rows >= 1, "the page with the relay drew no row");
    for html in [&run.without_relay, &run.bad_relay] {
        check_shape(&read_dump(html)?, rows, 0)?;
    }
    Ok(())
}

/// T-IP-05-004: the relay's own `/events`, read apart from the page, holds a
/// settled `of: final` translation into the opposite language for the last
/// `final` in the language of each row.
#[test]
#[ignore = "starts the hayamimi binary, the relay, a stand-in translation server and chrome-headless-shell"]
fn the_relay_publishes_four_settled_translations_for_the_four_lines() -> TestResult {
    let run = observed()?;
    let last = last_own_finals(&run.langs, &run.given)?;
    check_rows_translated(&run.langs, &last, &run.relay)?;
    Ok(())
}

const OK_ROW: &str = r#"<div class="row"><div class="row-text">x</div><div class="tr lang-en"><span class="tr-arrow">→en</span><span class="tr-text">y</span></div></div>"#;
const HELD_ROW: &str = r#"<div class="row"><div class="row-text">z</div><div class="tr lang-ja"><span class="tr-arrow">→ja</span><span class="tr-mark">訳を控えました</span><span class="tr-reason">w</span></div></div>"#;

fn page(rows: &[&str]) -> String {
    format!(r#"<div class="dash">{}</div>"#, rows.concat())
}

fn marks() -> Vec<String> {
    vec![
        String::from("Translation withheld"),
        String::from("訳を控えました"),
    ]
}

/// Judges `rows` of hand-written html, whose ok rows say "y" into English and
/// "j" into Japanese.
fn judge_rows(rows: &[&str], lines: &[(Status, &str)]) -> Result<Judgement, Failure> {
    let marks = marks();
    let words = Words {
        english: "y",
        japanese: "j",
        held_marks: &marks,
    };
    judge(&read_dump(&page(rows))?, lines, &words)
}

/// T-IP-05-005
#[test]
fn the_page_counter_counts_whole_class_names_only() -> TestResult {
    let dump = read_dump(&page(&[
        r#"<div class="row"><div class="row-text">x</div><div class="tr lang-ja"><span class="tr-arrow">→ja</span></div></div>"#,
    ]))?;
    assert_eq!((dump.rows.len(), dump.trs_total), (1, 1));
    let row = dump.rows.first().ok_or("no row")?;
    assert_eq!(row.trs.len(), 1);
    assert_eq!(
        row.trs.first().ok_or("no tr")?.arrow.as_deref(),
        Some("→ja")
    );
    let without = read_dump(r#"<div class="dash"><div class="row-text">x</div></div>"#)?;
    assert_eq!((without.rows.len(), without.trs_total), (0, 0));
    Ok(())
}

/// T-IP-05-006
#[test]
fn a_dump_of_an_unrendered_page_is_an_error_and_not_zero_counts() -> TestResult {
    for html in [
        r#"<div class="x"></div>"#,
        "<html><head></head><body></body></html>",
        "",
    ] {
        let said = read_dump(html);
        assert!(
            matches!(&said, Err(message) if message.contains("dash")),
            "{html:?}: {said:?}"
        );
    }
    read_dump(&page(&[]))?;
    Ok(())
}

/// The dump of a real page: the shell around the app, a comment, a script with
/// a `<` in it, void elements, an attribute with a `>` in it and an entity.
#[test]
fn a_dump_with_the_shell_around_the_page_is_read() -> TestResult {
    let html = format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>hayamimi</title></head><body><!-- x --><div id=\"main\" title=\"a>b\">{}</div><script type=\"module\">if (a < b) {{ init(); }}</script></body></html>",
        page(&[
            r#"<div class="row"><span class="lang-badge ja">ja</span><div class="row-text">a &amp; b<br></div></div>"#
        ])
    );
    let dump = read_dump(&html)?;
    assert_eq!(dump.rows.len(), 1);
    let row = dump.rows.first().ok_or("no row")?;
    assert_eq!(row.badge.as_deref(), Some("ja"));
    Ok(())
}

/// The marks of a withheld translation are the strings of the two languages.
#[test]
fn the_held_marks_are_read_from_both_string_tables() -> TestResult {
    assert_eq!(held_marks()?, marks());
    Ok(())
}

/// A client frame is masked, and its length field has the size the payload needs.
#[test]
fn a_client_frame_is_masked_and_its_length_has_the_size_the_payload_needs() -> TestResult {
    let short = text_frame(b"hi")?;
    assert_eq!(&short[..2], [0x81, 0x82]);
    assert_eq!(&short[2..6], WS_MASK);
    assert_eq!(&short[6..], [b'h' ^ WS_MASK[0], b'i' ^ WS_MASK[1]]);
    let medium = text_frame(&[b'a'; 126])?;
    assert_eq!(&medium[..4], [0x81, 0x80 | 126, 0, 126]);
    assert_eq!(medium.len(), 4 + 4 + 126);
    let long = text_frame(&vec![b'a'; 65_536])?;
    assert_eq!(&long[..10], [0x81, 0x80 | 127, 0, 0, 0, 0, 0, 1, 0, 0]);
    Ok(())
}

/// A server frame is not masked; a payload of 126 bytes or more has a longer
/// length field, and a frame beyond the limit is refused before it is read.
#[test]
fn a_server_frame_is_read_with_its_length_field_and_its_end_flag() -> TestResult {
    let mut small: &[u8] = &[0x81, 0x02, b'o', b'k'];
    assert_eq!(read_frame(&mut small)?, (true, 0x1, b"ok".to_vec()));
    let mut medium = vec![0x01, 126, 0, 200];
    medium.extend([b'x'; 200]);
    let (last, opcode, payload) = read_frame(&mut medium.as_slice())?;
    assert_eq!((last, opcode, payload.len()), (false, 0x1, 200));
    let mut huge: &[u8] = &[0x81, 127, 0xff, 0, 0, 0, 0, 0, 0, 0];
    let said = read_frame(&mut huge);
    assert!(
        matches!(&said, Err(message) if message.contains("bytes")),
        "{said:?}"
    );
    Ok(())
}

/// T-IP-05-017
#[test]
fn a_held_translation_with_a_mark_and_no_text_passes_the_page_check() -> TestResult {
    let judgement = judge_rows(
        &[OK_ROW, HELD_ROW],
        &[(Status::Ok, "en"), (Status::Held, "ja")],
    )?;
    assert_eq!(
        judgement,
        Judgement {
            ok: 1,
            held: 1,
            ok_judged: true,
            held_judged: true
        }
    );
    Ok(())
}

/// T-IP-05-018
#[test]
fn a_held_translation_that_shows_text_or_has_no_mark_fails_the_page_check() {
    let with_text = HELD_ROW.replacen(
        "</div></div>",
        r#"<span class="tr-text">y</span></div></div>"#,
        1,
    );
    let no_mark = HELD_ROW.replace(r#"<span class="tr-mark">訳を控えました</span>"#, "");
    let wrong_mark = HELD_ROW.replace("訳を控えました", "x");
    let cases = [
        (with_text, "tr-text"),
        (no_mark, "no `tr-mark`"),
        (wrong_mark, "`tr-mark` is \"x\""),
    ];
    for (held, reason) in cases {
        let said = judge_rows(
            &[OK_ROW, &held],
            &[(Status::Ok, "en"), (Status::Held, "ja")],
        );
        assert!(
            matches!(&said, Err(message) if message.contains("row 1") && message.contains(reason)),
            "{reason}: {said:?}"
        );
    }
}

/// T-IP-05-019
#[test]
fn a_judgement_with_no_row_to_judge_is_not_issued_and_the_counts_are_recorded() -> TestResult {
    let all_ok = judge_rows(&[OK_ROW, OK_ROW], &[(Status::Ok, "en"), (Status::Ok, "en")])?;
    assert_eq!(
        all_ok,
        Judgement {
            ok: 2,
            held: 0,
            ok_judged: true,
            held_judged: false
        }
    );
    let all_held = judge_rows(
        &[HELD_ROW, HELD_ROW],
        &[(Status::Held, "ja"), (Status::Held, "ja")],
    )?;
    assert_eq!(
        all_held,
        Judgement {
            ok: 0,
            held: 2,
            ok_judged: false,
            held_judged: true
        }
    );
    Ok(())
}

// ---- the judgement of one manifest row (design 10.9.3) --------------------

fn placed(lang: &str, id: &str) -> Placed {
    Placed {
        lang: lang.to_owned(),
        id: Some(id.to_owned()),
    }
}

fn given_for(utt: u32, finals: Vec<Placed>) -> Given {
    Given {
        repeat: 1,
        utt,
        finals,
    }
}

fn langs4() -> Vec<String> {
    ["ja", "en", "ja", "en"].map(String::from).to_vec()
}

fn seen(id: &str, lang: &str, text: &str) -> Seen {
    Seen {
        id: id.to_owned(),
        lang: lang.to_owned(),
        text: text.to_owned(),
    }
}

/// A translation event of line `id` into `lang`.
fn translation(id: &str, lang: &str, of: &str, settled: bool) -> Value {
    serde_json::json!({
        "type": "translation", "id": id, "of": of, "settled": settled,
        "lang": lang, "text": "t", "status": "ok"
    })
}

/// One row of a dumped page: the badge, the text and one `tr` for each arrow.
fn drawn(text: &str, badge: &str, arrows: &[&str]) -> String {
    let trs: String = arrows
        .iter()
        .map(|lang| {
            format!(r#"<div class="tr lang-{lang}"><span class="tr-arrow">→{lang}</span></div>"#)
        })
        .collect();
    format!(
        r#"<div class="row"><span class="lang-badge {badge}">{badge}</span><div class="row-text">{text}</div>{trs}</div>"#
    )
}

fn dump_of(rows: &[String]) -> Result<Dump, Failure> {
    let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
    read_dump(&page(&rows))
}

/// The row a language is translated from is opposed by the other language of
/// the pair; a third language has none.
#[test]
fn the_language_opposite_to_ja_is_en_and_the_other_way_round_and_no_other_has_one() -> TestResult {
    assert_eq!((opposite("ja")?, opposite("en")?), ("en", "ja"));
    let said = opposite("zh");
    assert!(
        matches!(&said, Err(message) if message.contains("zh")),
        "{said:?}"
    );
    Ok(())
}

/// An utterance split into several `final`s is judged by the last of them
/// that is in the language of its manifest row (10.9.3, 2 (b)).
#[test]
fn a_row_split_into_several_finals_is_judged_by_its_last_final_in_its_own_language() -> TestResult {
    let given = [
        given_for(
            1,
            vec![
                placed("ja", "file-1"),
                placed("en", "file-2"),
                placed("ja", "file-3"),
            ],
        ),
        given_for(2, vec![placed("en", "file-4")]),
        given_for(3, vec![placed("ja", "file-5")]),
        given_for(4, vec![placed("en", "file-6"), placed("ja", "file-7")]),
    ];
    let last = last_own_finals(&langs4(), &given)?;
    let ids: Vec<Option<&str>> = last.iter().map(|done| done.id.as_deref()).collect();
    assert_eq!(
        ids,
        [
            Some("file-3"),
            Some("file-4"),
            Some("file-5"),
            Some("file-6")
        ]
    );
    Ok(())
}

/// A row with no `final` in its language means the identifier missed, not the
/// page: nothing is judged, and every such row is named with what it was given.
#[test]
fn a_run_with_a_row_that_has_no_final_in_its_language_is_not_judged_and_every_such_row_is_named()
-> TestResult {
    let given = [
        given_for(1, vec![placed("ja", "file-1")]),
        given_for(2, vec![placed("ja", "file-2")]),
        given_for(3, vec![]),
        given_for(4, vec![placed("en", "file-3")]),
    ];
    let said = last_own_finals(&langs4(), &given);
    assert!(
        matches!(&said, Err(message) if message.contains("row 8 (en)")
            && message.contains("row 9 (ja)")
            && !message.contains("row 7")
            && message.contains("not judged")),
        "{said:?}"
    );
    let short = last_own_finals(&langs4(), &given[..3]);
    assert!(short.is_err(), "{short:?}");
    Ok(())
}

/// Only a settled translation of the `final` itself counts, into the language
/// opposite to the row's (10.9.3, 2 (b) and 4).
#[test]
fn a_row_is_translated_only_by_a_settled_of_final_translation_into_the_opposite_language()
-> TestResult {
    let ok = [translation("file-1", "en", "final", true)];
    check_translated(7, "ja", "file-1", &ok)?;
    let refined_only = [translation("file-1", "en", "refine", true)];
    let unsettled = [translation("file-1", "en", "final", false)];
    let wrong_way = [translation("file-1", "ja", "final", true)];
    let other_line = [translation("file-2", "en", "final", true)];
    for (events, why) in [
        (&refined_only, "refine"),
        (&unsettled, "unsettled"),
        (&wrong_way, "wrong way"),
        (&other_line, "other line"),
    ] {
        let said = check_translated(7, "ja", "file-1", events);
        assert!(
            matches!(&said, Err(message) if message.contains("row 7")),
            "{why}: {said:?}"
        );
    }
    let refined_later = [
        translation("file-1", "en", "final", true),
        translation("file-1", "en", "refine", true),
    ];
    check_translated(7, "ja", "file-1", &refined_later)?;
    Ok(())
}

/// The finals with no settled translation yet are the ones the run still waits
/// for, and are named when the wait ends.
#[test]
fn the_finals_with_no_settled_translation_are_named() {
    let finals = [seen("file-1", "ja", "a"), seen("file-2", "en", "b")];
    let relay = [translation("file-1", "en", "final", true)];
    assert_eq!(missing_translations(&relay, &finals), ["file-2"]);
    assert!(missing_translations(&[], &finals).len() == 2);
    let both = [relay[0].clone(), translation("file-2", "ja", "final", true)];
    assert!(missing_translations(&both, &finals).is_empty());
}

/// The page draws a `row` for each `final` in the order they came; a row is
/// taken for a `final` only when its text and badge are the `final`'s, and it
/// holds exactly one `tr` (10.9.3, 2 (c)).
#[test]
fn each_final_has_the_row_of_its_place_with_its_text_and_badge_and_exactly_one_tr() -> TestResult {
    let finals = [seen("file-1", "ja", "あ"), seen("file-2", "en", "b")];
    let good = [drawn("あ", "ja", &["en"]), drawn("b", "en", &["ja"])];
    check_page_rows(&dump_of(&good)?, &finals)?;
    let cases = [
        (
            vec![drawn("あ", "ja", &["en"])],
            "1 rows for 2 finals",
            "fewer rows",
        ),
        (
            vec![drawn("あ", "ja", &["en"]), drawn("c", "en", &["ja"])],
            "file-2",
            "other text",
        ),
        (
            vec![drawn("あ", "ja", &["en"]), drawn("b", "ja", &["en"])],
            "file-2",
            "other badge",
        ),
        (
            vec![drawn("あ", "ja", &["en", "en"]), drawn("b", "en", &["ja"])],
            "file-1",
            "two tr",
        ),
        (
            vec![drawn("あ", "ja", &[]), drawn("b", "en", &["ja"])],
            "file-1",
            "no tr",
        ),
    ];
    for (rows, named, why) in cases {
        let said = check_page_rows(&dump_of(&rows)?, &finals);
        assert!(
            matches!(&said, Err(message) if message.contains(named)),
            "{why}: {said:?}"
        );
    }
    Ok(())
}

/// The arrow of a `tr` points away from the badge of its row (10.9.3, 5).
#[test]
fn an_arrow_that_does_not_point_away_from_the_badge_of_its_row_fails() -> TestResult {
    check_arrows_oppose_badges(&dump_of(&[
        drawn("a", "ja", &["en"]),
        drawn("b", "en", &["ja"]),
    ])?)?;
    for rows in [[drawn("a", "ja", &["ja"])], [drawn("a", "en", &["en"])]] {
        let said = check_arrows_oppose_badges(&dump_of(&rows)?);
        assert!(
            matches!(&said, Err(message) if message.contains("row 0")),
            "{said:?}"
        );
    }
    Ok(())
}

/// A `tr` outside every row (a refined paragraph) is not counted, and a page
/// without the relay has rows with none inside (10.9.3, 4).
#[test]
fn a_tr_outside_every_row_is_not_counted() -> TestResult {
    let html = format!(
        r#"<div class="dash">{}<div class="refine"><div class="tr lang-en"><span class="tr-arrow">→en</span></div></div></div>"#,
        drawn("a", "ja", &[])
    );
    let dump = read_dump(&html)?;
    assert_eq!((dump.rows.len(), dump.trs_total), (1, 1));
    check_shape(&dump, 1, 0)?;
    let said = check_shape(&dump_of(&[drawn("a", "ja", &["en"])])?, 1, 0);
    assert!(said.is_err(), "{said:?}");
    Ok(())
}

/// The `final`s of an events file, in the order they came, with their id,
/// language and text; other events are not read.
#[test]
fn the_finals_of_an_events_file_are_read_in_the_order_they_came() -> TestResult {
    let line =
        |t_us: i64, event: Value| serde_json::json!({ "t_us": t_us, "event": event }).to_string();
    let text = [
        line(1, serde_json::json!({"type": "partial", "text": "x"})),
        line(
            2,
            serde_json::json!({"type": "final", "id": "file-2", "lang": "en", "text": "b"}),
        ),
        line(
            3,
            serde_json::json!({"type": "translation", "id": "file-2"}),
        ),
        line(
            4,
            serde_json::json!({"type": "final", "id": "file-3", "lang": "ja", "text": "あ"}),
        ),
    ]
    .join("\n");
    assert_eq!(
        finals_seen(&text)?,
        [seen("file-2", "en", "b"), seen("file-3", "ja", "あ")]
    );
    let no_id = line(
        1,
        serde_json::json!({"type": "final", "lang": "en", "text": "b"}),
    );
    let said = finals_seen(&no_id);
    assert!(
        matches!(&said, Err(message) if message.contains("id")),
        "{said:?}"
    );
    Ok(())
}

/// The samples file of two utterances that start 10 s and 16 s after the tool
/// began.
fn two_samples() -> Result<String, Failure> {
    let sample = |utt: u32, start: i64| {
        serde_json::json!({
            "repeat": 1, "utt": utt, "t_start_us": start, "t_end_us": start + 4_000_000,
            "t_partial_us": null, "t_partial_tr_us": null, "t_final_us": null,
            "t_tr_us": null, "finals": 0, "text": "", "translation": null,
            "final_latency_ms": null, "cer_dist": 0, "cer_denom": 0
        })
        .to_string()
    };
    Ok([sample(1, 10_000_000), sample(2, 16_000_000)].join("\n"))
}

/// An events file whose server clock began `began_us` after the tool's; the
/// finals end 2 s and 8 s after it began.
fn events_began(began_us: Option<i64>) -> String {
    let line = |t_us: i64, event: Value| serde_json::json!({ "t_us": t_us, "event": event });
    let final_at = |t_us: i64, id: &str, t1_s: f64| {
        line(
            t_us,
            serde_json::json!({"type": "final", "id": id, "lang": "ja", "text": "x", "t1_s": t1_s}),
        )
    };
    let mut lines = Vec::new();
    if let Some(began) = began_us {
        lines.push(line(began, serde_json::json!({"type": "session_start"})));
    }
    lines.push(final_at(12_500_000, "file-1", 2.0));
    lines.push(final_at(18_500_000, "file-2", 8.0));
    lines
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

/// The ids each utterance was given.
fn ids_given(samples: &str, events: &str) -> Result<Vec<Vec<Option<String>>>, Failure> {
    Ok(given_of(samples, events)?
        .into_iter()
        .map(|given| given.finals.into_iter().map(|done| done.id).collect())
        .collect())
}

/// A `final`'s `t1_s` counts from the `session_start` event, the window from
/// the start of the tool: with the server 9 s into the tool's clock, the
/// finals at 2 s and 8 s belong to the windows at 10 s and 16 s. Read on one
/// clock they come before both windows and belong to none.
#[test]
fn finals_are_placed_on_the_clock_of_the_windows_by_the_session_start_event() -> TestResult {
    let samples = two_samples()?;
    let aligned = ids_given(&samples, &events_began(Some(9_000_000)))?;
    assert_eq!(
        aligned,
        [
            vec![Some(String::from("file-1"))],
            vec![Some(String::from("file-2"))]
        ]
    );
    let unaligned = ids_given(&samples, &events_began(None))?;
    assert_eq!(unaligned, [vec![], vec![]]);
    Ok(())
}

/// A row of `lat-dialog` with its language and its place in the recording.
fn manifest_row(lang: &str, start: u64, end: u64) -> ManifestRow {
    let mut row = ManifestRow::new();
    row.insert(String::from("lang"), Value::from(lang));
    row.insert(String::from("start_sample"), Value::from(start));
    row.insert(String::from("end_sample"), Value::from(end));
    row
}

/// Ten rows that alternate ja, en, from sample 1000, each 100 long and 50 apart.
fn ten_rows() -> Vec<ManifestRow> {
    (0..10_u64)
        .map(|at| {
            let start = 1000 + at * 150;
            manifest_row(if at % 2 == 0 { "ja" } else { "en" }, start, start + 100)
        })
        .collect()
}

/// The cut runs from the end of row 6 to the end of row 10, and the four rows
/// of the window are counted from the start of the cut.
#[test]
fn the_cut_runs_from_the_end_of_row_six_to_the_end_of_row_ten_and_the_rows_start_from_it()
-> TestResult {
    let rows = ten_rows();
    let cut = cut_of(&rows)?;
    let from = 1000 + 5 * 150 + 100;
    assert_eq!((cut.from, cut.to), (from, 1000 + 9 * 150 + 100));
    let first = cut.rows.first().ok_or("no row")?;
    assert_eq!(first.get("start_sample"), Some(&Value::from(50_u64)));
    assert_eq!(first.get("end_sample"), Some(&Value::from(150_u64)));
    assert_eq!(langs_of(&cut.rows)?, langs4());
    Ok(())
}

/// A manifest too short for the window, and one whose window does not begin
/// with ja and alternate, are refused.
#[test]
fn a_manifest_that_is_too_short_or_does_not_alternate_in_the_window_is_refused() {
    assert!(cut_of(&ten_rows()[..9]).is_err());
    let mut blocks = ten_rows();
    blocks[7] = manifest_row("ja", 2000, 2100);
    let said = cut_of(&blocks);
    assert!(
        matches!(&said, Err(message) if message.contains("ja, en, ja, en")),
        "{said:?}"
    );
}
