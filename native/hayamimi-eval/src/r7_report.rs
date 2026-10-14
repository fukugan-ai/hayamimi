//! The result document of R7 (`docs/results/r7_recognition.md`): the
//! recognizer on the GPU and on the CPU with a second source decoding beside
//! it, the host stages under the same contention, and what the recognizer's
//! search costs apart from its encoder.
//!
//! The inputs are the run files of `stages`. A run with `peers` above 0 had
//! that many other processes decoding for its whole length.

use std::collections::HashSet;

use crate::error::EvalError;
use crate::mdtable::render;
use crate::percentile::nearest_rank;
use crate::stage_runs::{ENCODER_ONLY, HOST};
use crate::stage_search::{DECODER_TOTAL, FRAMES_COUNT, JOINER_TOTAL, TOKENS_COUNT};
use crate::stages::{Aggregate, RunFile, Sample, aggregate};

/// The stages of the recognizer a run times with sherpa-onnx.
const RECOGNITION: [&str; 3] = ["recognize_total", "decode", "fbank"];
/// The stages a CPU run times for the sources, with the variant each is recorded under: the
/// host stages that have no GPU implementation, and the two models the design leaves on the CPU.
const HOST_STAGES: [(&str, &str); 7] = [
    ("resample", HOST),
    ("ring", HOST),
    ("vad", "fp32"),
    ("judge", HOST),
    ("speaker_embed", "fp32"),
    ("itn", HOST),
    ("punct_rules", HOST),
];
/// How the tables are to be read: what a peer is, and what the search bracket is not.
const READING: &str = "\n## 読み方\n\n\
- peers が 1 の run は、別 process の 2 つ目の音源が run の間ずっと decode していた。peers が 0 の run は音源 1 つだけ。\n\
- 同じ peers の cuda と cpu を比べる。run を取った時の機械の負荷（他の席の GPU 利用と CPU 負荷）は run file に載らない。比べるときは同じ時間帯に続けて取った run を使い、負荷は測定時に別に記録する。\n\
- 探索の見積りは ort で joiner と decoder を同じ形の入力で呼んだ時間であって、sherpa-onnx の中の時間ではない。残りが見積りの上を超えるとき、差の原因は未特定である。\n";
/// The variant the search is compared on: the encoder is timed alone in fp32 only.
const COMPARED: &str = "fp32";

fn invalid(message: impl Into<String>) -> EvalError {
    EvalError::Invalid(message.into())
}

/// Where the recognizer sits after the second source is counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// The CPU is no slower than the GPU: the CPU placement stands.
    CpuKept,
    /// The CPU is slower than the GPU: the placement goes back to the council.
    ReturnToCouncil,
}

/// The placement verdict from the p50 of the recognition on each side.
pub fn placement_verdict(gpu_p50_us: u64, cpu_p50_us: u64) -> Placement {
    if cpu_p50_us <= gpu_p50_us {
        Placement::CpuKept
    } else {
        Placement::ReturnToCouncil
    }
}

fn verdict_words(placement: Placement) -> &'static str {
    match placement {
        Placement::CpuKept => "CPU は GPU 以下。認識の置き場は CPU のまま",
        Placement::ReturnToCouncil => "CPU が GPU より遅い。認識の置き場を評議へ戻す",
    }
}

/// The search of one utterance is the joiner for every frame plus a decoder call for each token
/// at the least and for each frame at the most. Returns that bracket in microseconds.
pub fn search_bracket_us(frames: f64, tokens: f64, joiner_us: f64, decoder_us: f64) -> (f64, f64) {
    (
        frames * joiner_us + tokens * decoder_us,
        frames * joiner_us + frames * decoder_us,
    )
}

/// Where a measured remainder falls against the bracket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Remainder {
    Inside,
    Above,
    Below,
}

pub fn remainder_against(low_us: f64, high_us: f64, remainder_us: f64) -> Remainder {
    if remainder_us > high_us {
        Remainder::Above
    } else if remainder_us < low_us {
        Remainder::Below
    } else {
        Remainder::Inside
    }
}

fn remainder_words(found: Remainder) -> &'static str {
    match found {
        Remainder::Inside => "探索の見積りの範囲内",
        Remainder::Above => "見積りより大きい。差は ort の外（原因は未特定）",
        Remainder::Below => "見積りより小さい。見積りが過大（原因は未特定）",
    }
}

fn find<'a>(rows: &'a [Aggregate], stage: &str, variant: &str) -> Option<&'a Aggregate> {
    rows.iter()
        .find(|row| row.stage == stage && row.variant == variant)
}

fn variants_of(rows: &[Aggregate], stage: &str) -> Vec<String> {
    rows.iter()
        .filter(|row| row.stage == stage)
        .map(|row| row.variant.clone())
        .collect()
}

fn cells(row: Option<&Aggregate>) -> [String; 2] {
    match row {
        Some(row) => [row.p50_us().to_string(), row.p95_us().to_string()],
        None => [String::from("-"), String::from("-")],
    }
}

fn run_rows(runs: &[RunFile]) -> Vec<Vec<String>> {
    runs.iter()
        .map(|run| {
            vec![
                run.meta.provider.clone(),
                run.meta.peers.to_string(),
                run.meta.peer_decodes.to_string(),
                run.meta.repeat.to_string(),
                run.meta.utterances.to_string(),
                run.meta.os.clone(),
            ]
        })
        .collect()
}

fn runs_section(runs: &[RunFile]) -> String {
    let header = [
        "provider",
        "peers",
        "peer が終えた decode",
        "repeat",
        "発話数",
        "os",
    ];
    format!("## run\n\n{}", render(&header, &run_rows(runs)))
}

fn recognition_rows(runs: &[RunFile]) -> Result<Vec<Vec<String>>, EvalError> {
    let mut rows = Vec::new();
    for run in runs {
        let found = aggregate(run)?;
        for variant in variants_of(&found, "recognize_total") {
            let mut row = vec![
                run.meta.provider.clone(),
                variant.clone(),
                run.meta.peers.to_string(),
            ];
            for stage in RECOGNITION {
                row.extend(cells(find(&found, stage, &variant)));
            }
            rows.push(row);
        }
    }
    Ok(rows)
}

fn recognition_section(runs: &[RunFile]) -> Result<String, EvalError> {
    let header = [
        "provider",
        "variant",
        "peers",
        "認識全体 p50",
        "p95",
        "decode p50",
        "p95",
        "fbank p50",
        "p95",
    ];
    Ok(format!(
        "## 認識の時間\n\n単位は µs。認識全体は create_stream から get_result まで、decode は探索を含む。\n\n{}",
        render(&header, &recognition_rows(runs)?)
    ))
}

/// The run of `provider` that had exactly `peers` other sources; two such runs are ambiguous.
fn run_of<'a>(
    runs: &'a [RunFile],
    provider: &str,
    peers: u32,
) -> Result<Option<&'a RunFile>, EvalError> {
    let mut found = runs
        .iter()
        .filter(|run| run.meta.provider == provider && run.meta.peers == peers);
    let first = found.next();
    if found.next().is_some() {
        return Err(invalid(format!(
            "two runs on {provider} with {peers} peers: the table cannot tell which to compare"
        )));
    }
    Ok(first)
}

fn p50_of(run: &RunFile, stage: &str, variant: &str) -> Result<Option<u64>, EvalError> {
    Ok(find(&aggregate(run)?, stage, variant).map(Aggregate::p50_us))
}

/// One comparison row: the recognition of `gpu_variant` on the GPU against `cpu_variant` on the CPU.
fn placement_row(
    gpu: &RunFile,
    cpu: &RunFile,
    gpu_variant: &str,
    cpu_variant: &str,
) -> Result<Option<Vec<String>>, EvalError> {
    let stage = "recognize_total";
    let (Some(on_gpu), Some(on_cpu)) = (
        p50_of(gpu, stage, gpu_variant)?,
        p50_of(cpu, stage, cpu_variant)?,
    ) else {
        return Ok(None);
    };
    Ok(Some(vec![
        format!("GPU {gpu_variant} 対 CPU {cpu_variant}"),
        on_gpu.to_string(),
        on_cpu.to_string(),
        verdict_words(placement_verdict(on_gpu, on_cpu)).to_owned(),
    ]))
}

fn placement_section(runs: &[RunFile]) -> Result<String, EvalError> {
    let Some(peers) = runs.iter().map(|run| run.meta.peers).max() else {
        return Err(invalid("no run file was given"));
    };
    let heading = "## 認識の置き場";
    let (Some(gpu), Some(cpu)) = (run_of(runs, "cuda", peers)?, run_of(runs, "cpu", peers)?) else {
        return Ok(format!(
            "{heading}\n\n未測定。peers が {peers} の cuda の run と cpu の run が揃っていない。\n"
        ));
    };
    let mut rows = Vec::new();
    for (gpu_variant, cpu_variant) in [("fp32", "fp32"), ("fp32", "int8")] {
        rows.extend(placement_row(gpu, cpu, gpu_variant, cpu_variant)?);
    }
    let header = ["比べる対", "GPU p50", "CPU p50", "判定"];
    Ok(format!(
        "{heading}\n\npeers が {peers} の run の認識全体の p50（µs）。\n\n{}",
        render(&header, &rows)
    ))
}

fn host_rows(runs: &[RunFile]) -> Result<Vec<Vec<String>>, EvalError> {
    let cpu_runs: Vec<&RunFile> = runs.iter().filter(|r| r.meta.provider == "cpu").collect();
    let mut rows = Vec::new();
    for run in cpu_runs {
        let found = aggregate(run)?;
        for (stage, variant) in HOST_STAGES {
            if let Some(row) = find(&found, stage, variant) {
                let [p50, p95] = cells(Some(row));
                rows.push(vec![stage.to_owned(), run.meta.peers.to_string(), p50, p95]);
            }
        }
    }
    rows.sort_by(|a, b| (a[0].as_str(), a[1].as_str()).cmp(&(b[0].as_str(), b[1].as_str())));
    Ok(rows)
}

fn host_section(runs: &[RunFile]) -> Result<String, EvalError> {
    let header = ["段", "peers", "p50", "p95"];
    Ok(format!(
        "## CPU の段\n\n単位は µs。CPU の run の、GPU の経路が無い段。\n\n{}",
        render(&header, &host_rows(runs)?)
    ))
}

/// The values of `stage` and `variant` keyed by the call they belong to.
fn by_call(samples: &[Sample], stage: &str, variant: &str) -> Vec<((u32, u32), u64)> {
    samples
        .iter()
        .filter(|s| s.stage == stage && s.variant == variant)
        .map(|s| ((s.repeat, s.utt), s.ns))
        .collect()
}

fn median(values: &[i64]) -> Result<Option<i64>, EvalError> {
    if values.is_empty() {
        return Ok(None);
    }
    Ok(Some(nearest_rank(values, 50)?))
}

/// The time of one call in nanoseconds, `total_ns / frames`, for each call that has both.
fn per_call_ns(totals: &[((u32, u32), u64)], frames: &[((u32, u32), u64)]) -> Vec<i64> {
    totals
        .iter()
        .filter_map(|(call, total)| {
            let (_, count) = frames.iter().find(|(other, _)| other == call)?;
            let per_call = total.checked_div(*count)?;
            i64::try_from(per_call).ok()
        })
        .collect()
}

fn count_median(counts: &[((u32, u32), u64)]) -> Result<Option<i64>, EvalError> {
    let values: Vec<i64> = counts
        .iter()
        .filter_map(|(_, n)| i64::try_from(*n).ok())
        .collect();
    median(&values)
}

/// What a run measured of the search for one variant.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchRow {
    pub provider: String,
    pub variant: String,
    pub frames: i64,
    pub tokens: i64,
    pub joiner_us: f64,
    pub decoder_us: f64,
    pub bracket_us: (f64, f64),
    /// The recognition's decode, less the encoder alone and the fbank, in the same run.
    pub remainder_us: Option<f64>,
}

fn micros(ns: i64) -> f64 {
    ns as f64 / 1000.0
}

/// The decode less the encoder alone and the fbank, in microseconds, when the run timed all three.
fn remainder_us(run: &RunFile, variant: &str) -> Result<Option<f64>, EvalError> {
    if variant != COMPARED {
        return Ok(None);
    }
    let found = aggregate(run)?;
    let (Some(decode), Some(encoder), Some(fbank)) = (
        find(&found, "decode", variant),
        find(&found, "encoder_only", ENCODER_ONLY),
        find(&found, "fbank", variant),
    ) else {
        return Ok(None);
    };
    Ok(Some(
        decode.p50_us() as f64 - encoder.p50_us() as f64 - fbank.p50_us() as f64,
    ))
}

fn search_row(run: &RunFile, variant: &str) -> Result<Option<SearchRow>, EvalError> {
    let samples = &run.samples;
    let frames = by_call(samples, FRAMES_COUNT, variant);
    let tokens = by_call(samples, TOKENS_COUNT, variant);
    let joiner = per_call_ns(&by_call(samples, JOINER_TOTAL, variant), &frames);
    let decoder = per_call_ns(&by_call(samples, DECODER_TOTAL, variant), &frames);
    let (Some(frames), Some(tokens), Some(joiner), Some(decoder)) = (
        count_median(&frames)?,
        count_median(&tokens)?,
        median(&joiner)?,
        median(&decoder)?,
    ) else {
        return Ok(None);
    };
    let (joiner_us, decoder_us) = (micros(joiner), micros(decoder));
    Ok(Some(SearchRow {
        provider: run.meta.provider.clone(),
        variant: variant.to_owned(),
        frames,
        tokens,
        joiner_us,
        decoder_us,
        bracket_us: search_bracket_us(frames as f64, tokens as f64, joiner_us, decoder_us),
        remainder_us: remainder_us(run, variant)?,
    }))
}

fn search_rows(runs: &[RunFile]) -> Result<Vec<SearchRow>, EvalError> {
    let mut rows = Vec::new();
    for run in runs.iter().filter(|run| run.meta.peers == 0) {
        for variant in variants_of(&aggregate(run)?, JOINER_TOTAL) {
            rows.extend(search_row(run, &variant)?);
        }
    }
    Ok(rows)
}

fn search_cells(row: &SearchRow) -> Vec<String> {
    let (low, high) = row.bracket_us;
    let (remainder, verdict) = match row.remainder_us {
        Some(remainder) => (
            format!("{remainder:.0}"),
            remainder_words(remainder_against(low, high, remainder)).to_owned(),
        ),
        None => (
            String::from("-"),
            String::from("encoder を単独で測っていない"),
        ),
    };
    vec![
        row.provider.clone(),
        row.variant.clone(),
        row.frames.to_string(),
        row.tokens.to_string(),
        format!("{:.0}", row.joiner_us),
        format!("{:.0}", row.decoder_us),
        format!("{low:.0}"),
        format!("{high:.0}"),
        remainder,
        verdict,
    ]
}

fn search_section(runs: &[RunFile]) -> Result<String, EvalError> {
    let header = [
        "provider",
        "variant",
        "frame 数 p50",
        "token 数 p50",
        "joiner 1 回 p50",
        "decoder 1 回 p50",
        "探索の見積り 下",
        "上",
        "残り（decode − encoder − fbank）",
        "残りと見積り",
    ];
    let rows: Vec<Vec<String>> = search_rows(runs)?.iter().map(search_cells).collect();
    Ok(format!(
        "## 探索の内訳\n\n単位は µs。見積りの下は joiner を frame ごと、decoder を token ごとに呼ぶ時間、上は decoder も frame ごとに呼ぶ時間。\n\n{}",
        render(&header, &rows)
    ))
}

/// Adds the samples of `next` to `into`; a stage and variant that both hold is an error.
fn absorb(into: &mut RunFile, next: &RunFile) -> Result<(), EvalError> {
    let held: HashSet<(&str, &str)> = into
        .samples
        .iter()
        .map(|s| (s.stage.as_str(), s.variant.as_str()))
        .collect();
    if let Some(dup) = next
        .samples
        .iter()
        .find(|s| held.contains(&(s.stage.as_str(), s.variant.as_str())))
    {
        return Err(invalid(format!(
            "{} {} is in two run files of {} with {} peers",
            dup.stage, dup.variant, next.meta.provider, next.meta.peers
        )));
    }
    into.samples.extend(next.samples.iter().cloned());
    into.meta.peer_decodes += next.meta.peer_decodes;
    Ok(())
}

/// Joins the run files of one provider and peer count into one run.
///
/// Some stages cannot share a process on CUDA (the recognizer and the encoder
/// alone), so their runs arrive as separate files.
fn merge_runs(runs: &[RunFile]) -> Result<Vec<RunFile>, EvalError> {
    let mut merged: Vec<RunFile> = Vec::new();
    for run in runs {
        let same = merged
            .iter_mut()
            .find(|m| m.meta.provider == run.meta.provider && m.meta.peers == run.meta.peers);
        match same {
            Some(into) => absorb(into, run)?,
            None => merged.push(run.clone()),
        }
    }
    Ok(merged)
}

/// The whole document.
pub fn render_document(runs: &[RunFile]) -> Result<String, EvalError> {
    if runs.is_empty() {
        return Err(invalid("no run file was given"));
    }
    let runs = merge_runs(runs)?;
    let runs = runs.as_slice();
    let sections = [
        String::from("# R7 認識の置き場と 2 音源同時の遅れ\n"),
        runs_section(runs),
        recognition_section(runs)?,
        placement_section(runs)?,
        host_section(runs)?,
        search_section(runs)?,
        String::from(READING),
    ];
    Ok(sections.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stages::Meta;

    fn meta(provider: &str, peers: u32) -> Meta {
        Meta {
            provider: provider.to_owned(),
            load: false,
            cache: String::from("warm"),
            run: 1,
            repeat: 1,
            utterances: 1,
            os: String::from("linux"),
            peers,
            peer_decodes: u64::from(peers) * 10,
        }
    }

    fn sample(stage: &str, variant: &str, ns: u64) -> Sample {
        Sample {
            stage: stage.to_owned(),
            variant: variant.to_owned(),
            utt: 0,
            repeat: 0,
            ns,
        }
    }

    fn run(provider: &str, peers: u32, samples: Vec<Sample>) -> RunFile {
        RunFile {
            meta: meta(provider, peers),
            samples,
        }
    }

    fn recognition(variant: &str, total_us: u64) -> Vec<Sample> {
        vec![sample("recognize_total", variant, total_us * 1000)]
    }

    #[test]
    fn a_cpu_no_slower_than_the_gpu_keeps_the_placement() {
        assert_eq!(placement_verdict(1000, 1000), Placement::CpuKept);
        assert_eq!(placement_verdict(1000, 999), Placement::CpuKept);
    }

    #[test]
    fn a_cpu_one_microsecond_slower_returns_the_placement_to_the_council() {
        assert_eq!(placement_verdict(1000, 1001), Placement::ReturnToCouncil);
    }

    #[test]
    fn the_bracket_runs_from_a_decoder_call_per_token_to_one_per_frame() {
        let (low, high) = search_bracket_us(200.0, 50.0, 300.0, 100.0);
        assert_eq!((low, high), (65_000.0, 80_000.0));
    }

    #[test]
    fn a_remainder_is_placed_against_the_bracket() {
        assert_eq!(remainder_against(10.0, 20.0, 10.0), Remainder::Inside);
        assert_eq!(remainder_against(10.0, 20.0, 20.0), Remainder::Inside);
        assert_eq!(remainder_against(10.0, 20.0, 20.5), Remainder::Above);
        assert_eq!(remainder_against(10.0, 20.0, 9.5), Remainder::Below);
    }

    #[test]
    fn the_verdict_of_the_largest_peer_count_is_written() -> Result<(), EvalError> {
        let runs = [
            run("cuda", 1, recognition("fp32", 300_000)),
            run(
                "cpu",
                1,
                [recognition("int8", 250_000), recognition("fp32", 320_000)].concat(),
            ),
        ];
        let text = placement_section(&runs)?;
        assert!(text.contains("GPU fp32 対 CPU int8 | 300000 | 250000 | CPU は GPU 以下"));
        assert!(text.contains("GPU fp32 対 CPU fp32 | 300000 | 320000 | CPU が GPU より遅い"));
        Ok(())
    }

    #[test]
    fn without_both_providers_the_verdict_is_not_measured() -> Result<(), EvalError> {
        let runs = [run("cuda", 1, recognition("fp32", 300_000))];
        assert!(placement_section(&runs)?.contains("未測定"));
        assert!(placement_section(&[]).is_err());
        Ok(())
    }

    #[test]
    fn two_runs_of_one_kind_are_ambiguous() {
        let runs = [
            run("cuda", 1, recognition("fp32", 1)),
            run("cuda", 1, recognition("fp32", 2)),
        ];
        assert!(run_of(&runs, "cuda", 1).is_err());
    }

    /// 10 frames, 4 tokens; a joiner call of 1000 ns and a decoder call of 500 ns.
    #[test]
    fn runs_of_one_provider_and_peer_count_are_joined() -> Result<(), EvalError> {
        let runs = [
            run("cuda", 1, recognition("fp32", 100)),
            run("cuda", 1, vec![sample("encoder_only", "fp32", 5)]),
            run("cuda", 0, recognition("fp32", 90)),
        ];
        let merged = merge_runs(&runs)?;
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].samples.len(), 2);
        assert_eq!(merged[0].meta.peer_decodes, 20);
        Ok(())
    }

    #[test]
    fn a_stage_in_two_files_of_one_kind_is_refused() {
        let runs = [
            run("cuda", 1, recognition("fp32", 100)),
            run("cuda", 1, recognition("fp32", 110)),
        ];
        assert!(merge_runs(&runs).is_err());
    }

    fn search_samples(variant: &str) -> Vec<Sample> {
        vec![
            sample(FRAMES_COUNT, variant, 10),
            sample(TOKENS_COUNT, variant, 4),
            sample(JOINER_TOTAL, variant, 10_000),
            sample(DECODER_TOTAL, variant, 5_000),
        ]
    }

    #[test]
    fn a_replay_gives_the_per_call_times_and_the_bracket() -> Result<(), EvalError> {
        let found = run("cpu", 0, search_samples("fp32"));
        let Some(row) = search_row(&found, "fp32")? else {
            return Err(invalid("no search row"));
        };
        assert_eq!((row.frames, row.tokens), (10, 4));
        assert_eq!((row.joiner_us, row.decoder_us), (1.0, 0.5));
        assert_eq!(row.bracket_us, (12.0, 15.0));
        assert_eq!(row.remainder_us, None);
        Ok(())
    }

    #[test]
    fn the_remainder_is_the_decode_less_the_encoder_and_the_fbank() -> Result<(), EvalError> {
        let mut samples = search_samples("fp32");
        samples.push(sample("decode", "fp32", 171_000_000));
        samples.push(sample("encoder_only", ENCODER_ONLY, 66_000_000));
        samples.push(sample("fbank", "fp32", 6_000_000));
        let found = run("cuda", 0, samples);
        assert_eq!(remainder_us(&found, "fp32")?, Some(99_000.0));
        assert_eq!(remainder_us(&found, "int8")?, None);
        Ok(())
    }

    #[test]
    fn a_document_names_every_section() -> Result<(), EvalError> {
        let runs = [run("cpu", 0, recognition("int8", 100_000))];
        let text = render_document(&runs)?;
        for heading in [
            "## run",
            "## 認識の時間",
            "## 認識の置き場",
            "## CPU の段",
            "## 探索の内訳",
        ] {
            assert!(text.contains(heading), "{heading}");
        }
        assert!(render_document(&[]).is_err());
        Ok(())
    }
}
