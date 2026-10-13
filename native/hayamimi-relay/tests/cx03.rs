//! CX-3: the annotation adds under five milliseconds (p95) to a line.
//!
//! p95 is the sample at 1-based rank ceil(0.95 * n) of the sorted samples.

mod support;

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use hayamimi_relay::annotate::{AnnotateRequest, Annotator, AnnotatorConfig};
use hayamimi_relay::logging::NullSink;
use hayamimi_relay::verify::Verifier;
use support::{DEFAULT_NOW, Pair, TestResult, backdate, conv20, dir_with_rates, epoch, rates_a};

const ROUNDS: usize = 100;
const LIMIT: Duration = Duration::from_millis(5);

/// What the p95 check refuses.
#[derive(Debug)]
enum LatencyFault {
    NoSamples,
    TooSlow(Duration),
}

/// 1-based nearest rank of the 95th percentile.
fn p95_rank(count: usize) -> usize {
    (count * 95).div_ceil(100)
}

fn p95(samples: &[Duration]) -> Option<Duration> {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    sorted.get(p95_rank(sorted.len()).checked_sub(1)?).copied()
}

fn check_p95_within(samples: &[Duration], limit: Duration) -> Result<Duration, LatencyFault> {
    let found = p95(samples).ok_or(LatencyFault::NoSamples)?;
    if found <= limit {
        Ok(found)
    } else {
        Err(LatencyFault::TooSlow(found))
    }
}

fn repeated(value: Duration, count: usize) -> Vec<Duration> {
    vec![value; count]
}

fn annotator(path: std::path::PathBuf) -> Result<Annotator, String> {
    Annotator::open(
        &AnnotatorConfig {
            rates_path: Some(path),
            tables_dir: None,
        },
        &mut NullSink,
    )
}

/// One call per pair per round; returns the sample times and, for the first
/// round, the annotation texts per id.
struct Measured {
    samples: Vec<Duration>,
    first_round: Vec<(String, Vec<String>)>,
}

fn measure_annotation(
    pairs: &[Pair],
    path: std::path::PathBuf,
) -> Result<Measured, Box<dyn std::error::Error>> {
    let mut annotator = annotator(path)?;
    let now_epoch = epoch(DEFAULT_NOW)?;
    let mut samples = Vec::with_capacity(pairs.len() * ROUNDS);
    let mut first_round = Vec::new();
    for round in 0..ROUNDS {
        for pair in pairs {
            let request = AnnotateRequest {
                id: &pair.id,
                src: &pair.src,
                translation: &pair.reference,
                src_lang: pair.lang,
                now_epoch,
            };
            let started = Instant::now();
            let result = annotator.annotate(&request, &mut NullSink);
            samples.push(started.elapsed());
            if round == 0 {
                first_round.push((
                    pair.id.clone(),
                    result
                        .annotations
                        .iter()
                        .map(|note| note.text.trim().to_owned())
                        .collect(),
                ));
            }
        }
    }
    Ok(Measured {
        samples,
        first_round,
    })
}

fn measure_verify(pairs: &[Pair]) -> Result<Vec<Duration>, String> {
    let mut verifier = Verifier::open(None, &mut NullSink)?;
    let mut samples = Vec::with_capacity(pairs.len() * ROUNDS);
    for _ in 0..ROUNDS {
        for pair in pairs {
            let started = Instant::now();
            let verdict = verifier.verify(&pair.src, &pair.reference, pair.lang, &mut NullSink);
            samples.push(started.elapsed());
            std::hint::black_box(verdict);
        }
    }
    Ok(samples)
}

const FIGURE_WORDS: [&str; 22] = [
    "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "twenty",
    "thirty", "forty", "fifty", "hundred", "thousand", "million", "一", "二", "三", "五", "万",
];
const UNIT_WORDS: [&str; 14] = [
    "円",
    "ドル",
    "dollar",
    "yen",
    "mile",
    "pound",
    "lb",
    "kg",
    "km",
    "マイル",
    "ポンド",
    "キロ",
    "インチ",
    "inch",
];

fn mentions(text: &str, words: &[&str]) -> bool {
    let lowered = text.to_lowercase();
    words.iter().any(|word| lowered.contains(word))
}

fn has_figure(text: &str) -> bool {
    text.chars()
        .any(|c| c.is_ascii_digit() || ('０'..='９').contains(&c))
        || mentions(text, &FIGURE_WORDS)
}

/// Rows whose source or reference holds both a figure and a unit word.
fn rows_with_figure_and_unit(pairs: &[Pair]) -> BTreeSet<String> {
    pairs
        .iter()
        .filter(|pair| {
            [pair.src.as_str(), pair.reference.as_str()]
                .iter()
                .any(|text| has_figure(text) && mentions(text, &UNIT_WORDS))
        })
        .map(|pair| pair.id.clone())
        .collect()
}

/// T-CX-3-001
#[test]
fn cx03_annotation_adds_under_five_ms_p95_of_4000_annotation_calls_is_under_5_ms() -> TestResult {
    let pairs = conv20()?;
    let (_dir, path) = dir_with_rates(&rates_a())?;
    // The file was fetched this morning; a file written moments ago is
    // re-read on every line until it is two seconds old.
    backdate(&path, 3600)?;
    let measured = measure_annotation(&pairs, path)?;
    assert_eq!(p95_rank(measured.samples.len()), 3800, "T-CX-3-001: rank");
    let cold = p95(&measured.samples[..pairs.len()]);
    let warm = p95(&measured.samples[pairs.len()..]);
    eprintln!("T-CX-3-001: first round p95 {cold:?}, remaining p95 {warm:?}");
    let found = check_p95_within(&measured.samples, LIMIT);
    assert!(found.is_ok(), "T-CX-3-001: {found:?}");
    Ok(())
}

/// T-CX-3-002
#[test]
fn cx03_annotation_adds_under_five_ms_sample_count_and_conversions_are_real() -> TestResult {
    let pairs = conv20()?;
    let (_dir, path) = dir_with_rates(&rates_a())?;
    backdate(&path, 3600)?;
    let measured = measure_annotation(&pairs, path)?;
    assert_eq!(measured.samples.len(), 4000, "T-CX-3-002: samples");
    let annotated: Vec<&(String, Vec<String>)> = measured
        .first_round
        .iter()
        .filter(|(_, notes)| !notes.is_empty())
        .collect();
    let ids: Vec<&str> = annotated.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, ["j08", "e08"], "T-CX-3-002: annotated lines");
    assert_eq!(
        annotated[0].1,
        ["(about $31,500, rate of Oct 12)"],
        "T-CX-3-002: j08"
    );
    assert_eq!(
        annotated[1].1,
        ["（約 7,930,000 円、10/12 の値）"],
        "T-CX-3-002: e08"
    );
    let scanned = rows_with_figure_and_unit(&pairs);
    let expected: BTreeSet<String> = ["j08", "e08"].iter().map(|id| (*id).to_owned()).collect();
    assert_eq!(
        scanned, expected,
        "T-CX-3-002: rows with a figure and a unit word"
    );
    Ok(())
}

/// T-CX-3-003
#[test]
fn cx03_annotation_adds_under_five_ms_verify_p95_is_reported_with_the_same_method() -> TestResult {
    let pairs = conv20()?;
    let samples = measure_verify(&pairs)?;
    assert_eq!(samples.len(), 4000, "T-CX-3-003: samples");
    let found = p95(&samples).ok_or("no V samples")?;
    eprintln!("T-CX-3-003: V p95 {found:?}");
    assert!(found > Duration::ZERO, "T-CX-3-003: V p95 is zero");
    Ok(())
}

/// T-CX-3-004
#[test]
fn cx03_annotation_adds_under_five_ms_p95_boundary_at_rank_3800_passes() {
    let mut samples = repeated(Duration::from_millis(1), 3800);
    samples.extend(repeated(Duration::from_millis(50), 200));
    let verdict = check_p95_within(&samples, LIMIT);
    assert!(
        matches!(verdict, Ok(found) if found == Duration::from_millis(1)),
        "T-CX-3-004: {verdict:?}"
    );
}

/// T-CX-3-005
#[test]
fn cx03_annotation_adds_under_five_ms_p95_boundary_at_rank_3799_fails() {
    let mut samples = repeated(Duration::from_millis(1), 3799);
    samples.extend(repeated(Duration::from_millis(50), 201));
    let verdict = check_p95_within(&samples, LIMIT);
    assert!(
        matches!(verdict, Err(LatencyFault::TooSlow(found)) if found == Duration::from_millis(50)),
        "T-CX-3-005: {verdict:?}"
    );
}

/// T-CX-3-006
#[test]
fn cx03_annotation_adds_under_five_ms_fifty_ms_wait_is_rejected_by_the_check() {
    let samples: Vec<Duration> = (0..40)
        .map(|_| {
            let started = Instant::now();
            std::thread::sleep(Duration::from_millis(50));
            started.elapsed()
        })
        .collect();
    assert_eq!(p95_rank(samples.len()), 38, "T-CX-3-006: rank");
    let verdict = check_p95_within(&samples, LIMIT);
    assert!(
        matches!(verdict, Err(LatencyFault::TooSlow(found)) if found >= Duration::from_millis(50)),
        "T-CX-3-006: {verdict:?}"
    );
    assert!(matches!(
        check_p95_within(&[], LIMIT),
        Err(LatencyFault::NoSamples)
    ));
}
