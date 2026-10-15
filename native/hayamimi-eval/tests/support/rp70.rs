//! RP-70: translating a sentence without its closing mark keeps the quality.
//!
//! A `final` that is the last partial plus one `。` reuses the partial's T
//! answer. The reuse is allowed only if the translations of the sentences
//! without the mark are as good as those of the sentences with it, so the
//! same server translates a set `a` (the text as it is) and a set `b` (the
//! text without its final `。`) alternately, twice each.
//!
//! Everything here is pure: the runs arrive as numbers and rows, and the
//! judgement and the construction of the two sets are checked on fixtures.

use std::path::Path;

use super::{Failure, read};

/// One chrF is carried in millionths, so that the comparison is an integer one.
const MICRO: f64 = 1_000_000.0;

/// What one run left: the corpus chrF of its translations (millionths), the
/// number of lines whose T and E both failed V, and, for a `b` run, the number
/// of lines whose translation lost the end mark that the `a` translation has.
#[derive(Clone, Copy, Debug)]
pub struct MarkRun {
    pub chrf_micro: i64,
    pub held: usize,
    pub lost_marks: usize,
}

/// The values the judgement of one pair of pairs of runs read.
#[derive(Debug)]
pub struct MarkReport {
    pub a_chrf_sum: i64,
    pub b_chrf_sum: i64,
    pub a_held: usize,
    pub b_held: usize,
    pub lost_marks: usize,
    pub a_chrf_gap: u64,
    pub a_held_gap: usize,
}

fn chrf_sum(runs: &[MarkRun; 2]) -> i64 {
    runs.iter().map(|run| run.chrf_micro).sum()
}

fn held_sum(runs: &[MarkRun; 2]) -> usize {
    runs.iter().map(|run| run.held).sum()
}

/// The pooled chrF of two runs of equal size, for a message.
fn pooled(runs: &[MarkRun; 2]) -> String {
    format!("{:.6}", chrf_sum(runs) as f64 / (2.0 * MICRO))
}

fn report_of(a: &[MarkRun; 2], b: &[MarkRun; 2]) -> MarkReport {
    MarkReport {
        a_chrf_sum: chrf_sum(a),
        b_chrf_sum: chrf_sum(b),
        a_held: held_sum(a),
        b_held: held_sum(b),
        lost_marks: b.iter().map(|run| run.lost_marks).sum(),
        a_chrf_gap: a[0].chrf_micro.abs_diff(a[1].chrf_micro),
        a_held_gap: a[0].held.abs_diff(a[1].held),
    }
}

/// Judges `b` (the two runs without the mark) against `a` (the two runs with
/// it). The pooled chrF of `b` must not be below that of `a`, and the lines
/// withheld by both T and E in `b` must not outnumber those in `a`. Both are
/// compared as sums of the two runs, which is the comparison of the pooled
/// values without a division. The lines that lost their end mark are recorded
/// and not judged.
pub fn check_closing_mark(a: &[MarkRun; 2], b: &[MarkRun; 2]) -> Result<MarkReport, Failure> {
    let report = report_of(a, b);
    let mut broken = Vec::new();
    if report.b_chrf_sum < report.a_chrf_sum {
        broken.push(format!(
            "chrF: b pooled {} is below a pooled {}",
            pooled(b),
            pooled(a)
        ));
    }
    if report.b_held > report.a_held {
        broken.push(format!(
            "held rows: b {} is above a {}",
            report.b_held, report.a_held
        ));
    }
    if broken.is_empty() {
        Ok(report)
    } else {
        Err(broken.join("; "))
    }
}

pub const COLUMN_LOST_MARKS: &str = "rows that lost the final mark";
pub const COLUMN_A_CHRF_GAP: &str = "a chrF gap";
pub const COLUMN_A_HELD_GAP: &str = "a held gap";

/// The record of the runs, one `name: value` line per column. The chrF gap is
/// written in chrF points.
pub fn render_report(report: &MarkReport) -> String {
    format!(
        "{COLUMN_LOST_MARKS}: {}\n{COLUMN_A_CHRF_GAP}: {:.6}\n{COLUMN_A_HELD_GAP}: {}\n",
        report.lost_marks,
        report.a_chrf_gap as f64 / MICRO,
        report.a_held_gap
    )
}

/// Refuses a record that leaves out one of the columns, naming it.
pub fn check_columns_written(text: &str) -> Result<(), Failure> {
    for column in [COLUMN_LOST_MARKS, COLUMN_A_CHRF_GAP, COLUMN_A_HELD_GAP] {
        let prefix = format!("{column}: ");
        if !text.lines().any(|line| line.starts_with(&prefix)) {
            return Err(format!("the column {column:?} is not written"));
        }
    }
    Ok(())
}

/// How many lines have an end mark (`.`, `?` or `!`) in the translation of
/// `with_mark` and none in that of `without_mark`. The two are line-aligned.
pub fn lost_final_marks(with_mark: &[&str], without_mark: &[&str]) -> usize {
    let ends = |text: &str| text.trim_end().ends_with(['.', '?', '!']);
    with_mark
        .iter()
        .zip(without_mark)
        .filter(|(a, b)| ends(a) && !ends(b))
        .count()
}

/// The order in which the four runs must be sent.
const RUN_ORDER: [&str; 4] = ["a1", "b1", "a2", "b2"];

/// Refuses runs that were not sent as `a1`, `b1`, `a2`, `b2`, by their
/// start times. Two runs that started at the same time have no order.
pub fn check_run_order(started: &[(&str, i64)]) -> Result<(), Failure> {
    let mut by_time = started.to_vec();
    by_time.sort_by_key(|(_, at)| *at);
    let names: Vec<&str> = by_time.iter().map(|(name, _)| *name).collect();
    if names != RUN_ORDER {
        return Err(format!("runs were sent as {names:?}, not as {RUN_ORDER:?}"));
    }
    if by_time.windows(2).any(|pair| pair[0].1 == pair[1].1) {
        return Err(format!("runs {RUN_ORDER:?} share a start time"));
    }
    Ok(())
}

/// One line of a pairs directory.
#[derive(Clone, Debug)]
pub struct Row {
    pub id: String,
    pub ja: String,
    pub reference: String,
}

/// The rows of `<lang>.ids.txt`, `<lang>.ja.txt` and `<lang>.ref.txt` of
/// `dir`, which must have the same number of lines.
pub fn read_rows(dir: &Path, lang: &str) -> Result<Vec<Row>, Failure> {
    let ids = read(&dir.join(format!("{lang}.ids.txt")))?;
    let ja = read(&dir.join(format!("{lang}.ja.txt")))?;
    let references = read(&dir.join(format!("{lang}.ref.txt")))?;
    let counts = [
        ids.lines().count(),
        ja.lines().count(),
        references.lines().count(),
    ];
    if counts[0] != counts[1] || counts[0] != counts[2] {
        return Err(format!(
            "{} has {counts:?} lines in ids, ja and ref of {lang}",
            dir.display()
        ));
    }
    Ok(ids
        .lines()
        .zip(ja.lines())
        .zip(references.lines())
        .map(|((id, ja), reference)| Row {
            id: id.to_owned(),
            ja: ja.to_owned(),
            reference: reference.to_owned(),
        })
        .collect())
}

/// The rows of `rows` whose text ends with `。`: the set `a`.
pub fn ending_mark_rows(rows: &[Row]) -> Vec<Row> {
    rows.iter()
        .filter(|row| row.ja.ends_with('。'))
        .cloned()
        .collect()
}

/// The rows of `a` without their final `。`: the set `b`. A row that does not
/// end with `。` is left out.
pub fn without_mark_rows(a: &[Row]) -> Vec<Row> {
    a.iter()
        .filter_map(|row| {
            let text = row.ja.strip_suffix('。')?;
            Some(Row {
                ja: text.to_owned(),
                ..row.clone()
            })
        })
        .collect()
}

fn check_row_pair(a: &Row, b: &Row) -> Result<(), Failure> {
    if a.id != b.id {
        return Err(format!("b has the id {} where a has {}", b.id, a.id));
    }
    if a.reference != b.reference {
        return Err(format!("{}: the reference differs between a and b", a.id));
    }
    if format!("{}。", b.ja) != a.ja {
        return Err(format!("{}: b is not a without its final mark", a.id));
    }
    Ok(())
}

/// Checks that `b` is `a` minus the final `。` of each row (same ids, same
/// references), and returns the number of rows. `seed` names the set in a
/// refusal. An empty set is refused, and so is a row of `a` that does not end
/// with `。`.
pub fn check_pair_sets(seed: &str, a: &[Row], b: &[Row]) -> Result<usize, Failure> {
    if a.is_empty() {
        return Err(format!("{seed}: no row ends with the closing mark"));
    }
    if let Some(row) = a.iter().find(|row| !row.ja.ends_with('。')) {
        return Err(format!("{seed}: {} does not end with the mark", row.id));
    }
    if a.len() != b.len() {
        return Err(format!("{seed}: a has {} rows, b has {}", a.len(), b.len()));
    }
    for (row_a, row_b) in a.iter().zip(b) {
        check_row_pair(row_a, row_b)?;
    }
    Ok(a.len())
}
