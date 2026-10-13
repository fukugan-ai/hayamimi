//! `stages-report`: the run files of a measurement campaign, written as the
//! result document `docs/results/gpu_stages.md`.
//!
//! The document holds the machine and the build, a p50 and p95 for every stage
//! under every provider and load, the copies between host and device, the first
//! recognition by state of the JIT cache, a placement proposed for each stage,
//! the cost of repeating the language identification (H25) and the delay `L`
//! as far as the measured stages can bound it.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::error::EvalError;
use crate::gpu_results::{
    CELL_HEADER, ENVIRONMENT, ENVIRONMENT_HEADER, JIT, JIT_HEADER, PROPOSAL_HEADER, PROPOSALS,
    PROVIDERS, STAGES, TRANSFER_HEADER, TRANSFERS, TransferRow, UNMEASURED,
};
use crate::mdtable::render;
use crate::ort_stages::PlacedRow;
use crate::percentile::nearest_rank;
use crate::sha::sha256_hex;
use crate::stage_runs::{ENCODER_ONLY, FLOOR};
use crate::stage_table::{StageRow, parse_stage_table};
use crate::stages::{Aggregate, RunFile, aggregate};

/// What the document is built from.
pub struct Inputs<'a> {
    pub runs: &'a [RunFile],
    pub design: &'a str,
    pub build_json: &'a str,
    pub cpu: &'a str,
    pub transfers: &'a [TransferRow],
    pub placed: &'a [PlacedRow],
    pub python: &'a [PythonSample],
    pub min_silence_ms: f64,
}

/// One utterance of one Python run: the times the baseline measured.
#[derive(Debug, Clone, PartialEq)]
pub struct PythonSample {
    pub utt: u32,
    /// `t_final - t_end` in milliseconds, when a `final` came.
    pub l_asr_ms: Option<f64>,
    /// The `latency_ms` the Python `final` reports.
    pub decode_ms: Option<f64>,
    /// `t_tr - t_end` in milliseconds, when the translation came.
    pub l_ms: Option<f64>,
}

/// The samples of a Python run written by `stream`: one JSON object per line.
pub fn read_python_baseline(path: &std::path::Path) -> Result<Vec<PythonSample>, EvalError> {
    let text = std::fs::read_to_string(path).map_err(|e| EvalError::io(path, e))?;
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| python_sample(line, path))
        .collect()
}

fn python_sample(line: &str, path: &std::path::Path) -> Result<PythonSample, EvalError> {
    let row: Value = serde_json::from_str(line).map_err(|e| EvalError::json(path, e))?;
    let utt = row["utt"]
        .as_u64()
        .and_then(|n| n.checked_sub(1))
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| invalid(format!("{}: a row has no `utt`", path.display())))?;
    let end = row["t_end_us"]
        .as_f64()
        .ok_or_else(|| invalid(format!("{}: a row has no `t_end_us`", path.display())))?;
    let since_end = |key: &str| row[key].as_f64().map(|t| (t - end) / 1000.0);
    Ok(PythonSample {
        utt,
        l_asr_ms: since_end("t_final_us"),
        decode_ms: row["final_latency_ms"].as_f64(),
        l_ms: since_end("t_tr_us"),
    })
}

/// Utterances (0-based) whose `L` the Python baseline cannot give: 5, 10, 18 and 19 of the recording.
const NO_L: [u32; 4] = [4, 9, 17, 18];
/// Utterances (0-based) whose `L_asr` the Python baseline cannot give: 10 and 19.
const NO_L_ASR: [u32; 2] = [9, 18];

/// How a stage of the design table is measured.
struct StageMap {
    /// The start of the stage's name in the design table.
    prefix: &'static str,
    /// The measured stages that stand for it; none when it is not measured.
    ids: &'static [&'static str],
    /// Why it is not measured, or what to know when reading it.
    note: &'static str,
    /// The stage runs on the host whatever the provider is.
    host_inside: bool,
}

const fn map(
    prefix: &'static str,
    ids: &'static [&'static str],
    note: &'static str,
    host_inside: bool,
) -> StageMap {
    StageMap {
        prefix,
        ids,
        note,
        host_inside,
    }
}

/// The 19 rows of the design table, in order.
const STAGE_MAP: [StageMap; 19] = [
    map(
        "音声の取り込み",
        &[],
        "Rust の取り込み（WASAPI）がまだ無く、測れない",
        false,
    ),
    map(
        "int16 → float32",
        &["resample"],
        "入力は 16 kHz を 48 kHz にした物。mono",
        false,
    ),
    map(
        "音声の ring buffer",
        &["ring"],
        "preroll 1.0 秒の取り出しを含む",
        false,
    ),
    map(
        "VAD",
        &["vad"],
        "発話の後ろに 0.5 秒の無音を足して区切りを出すまで",
        false,
    ),
    map("言語判定", &["lid"], "先頭 4 秒を渡す", false),
    map(
        "認識の encoder",
        &["decode", "encoder_only"],
        "variant が fp32 などの行は sherpa-onnx の認識全体（encoder と探索）。ort-encoder fp32 の行は encoder の model だけを ort で同じ入力に掛けた物（入力は log-mel で、認識器の front end とは bit まで同じではない）",
        false,
    ),
    map(
        "fbank と探索",
        &["fbank"],
        "fbank だけ。探索は encoder と分けられず上の行に含む",
        true,
    ),
    map(
        "冒頭欠落の疑い判定",
        &["judge"],
        "判定の比較 1 回。解き直しは認識の encoder の行と同じ呼び出し",
        false,
    ),
    map("ITN", &[], "Rust の ITN がまだ無く、測れない", false),
    map(
        "句読点（BERT",
        &["punct_bert"],
        "forward だけ。入力は punct_ja_parity.json の最初の 23 case の input_ids で、lat-ja の発話の文字列ではない。token id の変換は含まない",
        false,
    ),
    map(
        "句読点の後処理",
        &[],
        "Rust の後処理がまだ無く、測れない",
        false,
    ),
    map("話者の埋め込み", &["speaker_embed"], "", false),
    map(
        "話者の速報の割り当て",
        &["speaker_assign"],
        "重心 8 個との cosine と更新 1 回",
        false,
    ),
    map(
        "清書の再分離",
        &["rediarize"],
        "分割、埋め込み、clustering を含む 1 呼び出し",
        false,
    ),
    map(
        "清書の clustering",
        &[],
        "sherpa-onnx の中で再分離と同じ呼び出しに含まれ、分けて測る口が無い",
        false,
    ),
    map(
        "訳 T と E",
        &["translate"],
        "T の 1 回。E は測っていない。測った環境: WSL の llama-server（CUDA 12.0 の build）で、本番の Windows、CUDA 13 の形ではない",
        false,
    ),
    map(
        "認識の文字列 → 訳の入力",
        &["prompt_build"],
        "依頼の本文（JSON）を作るまで",
        false,
    ),
    map(
        "照合 V と単位の換算",
        &["verify_convert"],
        "T の実際の出力に V と換算を掛けた 1 回",
        false,
    ),
    map(
        "字幕の配信",
        &[],
        "Rust の字幕 server がまだ無く、測れない",
        false,
    ),
];

fn invalid(message: impl Into<String>) -> EvalError {
    EvalError::Invalid(message.into())
}

fn load_label(load: bool) -> &'static str {
    if load { "負荷" } else { "無負荷" }
}

/// The aggregates of every warm run (the empty-cache runs only feed the JIT table).
fn warm_aggregates(runs: &[RunFile]) -> Result<Vec<Aggregate>, EvalError> {
    let mut all = Vec::new();
    for run in runs.iter().filter(|run| run.meta.cache != "empty") {
        all.extend(aggregate(run)?);
    }
    Ok(all)
}

/// The fastest variant of the stages in `ids` under a provider and load, by the mean p50 over the runs.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Best {
    variant: String,
    p50_us: u64,
    p95_us: u64,
}

fn best(aggs: &[Aggregate], provider: &str, load: bool, ids: &[&str]) -> Option<Best> {
    let mut sums: BTreeMap<&str, (u64, u64, u64)> = BTreeMap::new();
    for a in aggs
        .iter()
        .filter(|a| a.provider == provider && a.load == load && ids.contains(&a.stage.as_str()))
    {
        let entry = sums.entry(a.variant.as_str()).or_insert((0, 0, 0));
        entry.0 += a.p50_us();
        entry.1 += a.p95_us();
        entry.2 += 1;
    }
    sums.into_iter()
        .map(|(variant, (p50, p95, runs))| Best {
            variant: variant.to_owned(),
            p50_us: p50 / runs,
            p95_us: p95 / runs,
        })
        .min_by_key(|b| b.p50_us)
}

/// The mean over the runs of one stage's p50 and p95 for one variant, provider and load.
fn variant_mean(
    aggs: &[Aggregate],
    provider: &str,
    load: bool,
    stage: &str,
    variant: &str,
) -> Option<Best> {
    let rows: Vec<&Aggregate> = aggs
        .iter()
        .filter(|a| {
            a.provider == provider && a.load == load && a.stage == stage && a.variant == variant
        })
        .collect();
    let runs = u64::try_from(rows.len()).ok().filter(|n| *n > 0)?;
    Some(Best {
        variant: variant.to_owned(),
        p50_us: rows.iter().map(|a| a.p50_us()).sum::<u64>() / runs,
        p95_us: rows.iter().map(|a| a.p95_us()).sum::<u64>() / runs,
    })
}

/// `GPU 170000 µs / CPU 150000 µs` for one stage and variant under one load, or why it is missing.
fn pair_text(aggs: &[Aggregate], load: bool, stage: &str, variant: &str) -> String {
    match (
        variant_mean(aggs, "cuda", load, stage, variant),
        variant_mean(aggs, "cpu", load, stage, variant),
    ) {
        (Some(g), Some(c)) => format!("GPU {} µs / CPU {} µs", g.p50_us, c.p50_us),
        _ => String::from("片方が測れていない"),
    }
}

/// The recognition row: same-precision pairs only, and no placement. The deciding is left to the council.
fn recognition_proposal(aggs: &[Aggregate]) -> (String, String) {
    let mut parts = Vec::new();
    for (label, stage, variant) in [
        ("sherpa-onnx の認識全体 fp32", "decode", "fp32"),
        ("ort で encoder だけ fp32", "encoder_only", ENCODER_ONLY),
    ] {
        for load in [false, true] {
            parts.push(format!(
                "{label}、{}の p50: {}",
                load_label(load),
                pair_text(aggs, load, stage, variant)
            ));
        }
    }
    let side = |provider: &str, variant: &str| {
        variant_mean(aggs, provider, false, "decode", variant).map_or_else(
            || String::from("測れていない"),
            |b| format!("{} µs", b.p50_us),
        )
    };
    parts.push(format!(
        "参考（同じ精度の対ではない）: CPU の int8 は {}（int8 は CUDA に無い）、GPU の fp16 は {}（fp32 の GPU は {}）",
        side("cpu", "int8"),
        side("cuda", "fp16"),
        side("cuda", "fp32")
    ));
    (String::from("未決（評議）"), parts.join("。"))
}

fn note_for(map: &StageMap, variant: &str) -> String {
    let floor = if variant == FLOOR {
        "下限: 転送の往復だけで kernel を含まない。"
    } else {
        ""
    };
    format!("{floor}{}", map.note)
}

fn cell_rows(table: &[StageRow], aggs: &[Aggregate]) -> Result<Vec<Vec<String>>, EvalError> {
    let mut rows = Vec::new();
    for (stage, map) in table.iter().zip(&STAGE_MAP) {
        if !stage.name.starts_with(map.prefix) {
            return Err(invalid(format!(
                "design row `{}` does not start with `{}`: the stage map is out of date",
                stage.name, map.prefix
            )));
        }
        for provider in PROVIDERS {
            for load in [false, true] {
                let found: Vec<&Aggregate> = aggs
                    .iter()
                    .filter(|a| {
                        a.provider == provider
                            && a.load == load
                            && map.ids.contains(&a.stage.as_str())
                    })
                    .collect();
                if found.is_empty() {
                    rows.push(vec![
                        stage.name.clone(),
                        provider.to_owned(),
                        String::from("-"),
                        load_label(load).to_owned(),
                        String::from("0"),
                        String::from("0"),
                        String::from(UNMEASURED),
                        String::from(UNMEASURED),
                        if map.ids.is_empty() {
                            map.note.to_owned()
                        } else {
                            String::from("この組は測っていない")
                        },
                    ]);
                    continue;
                }
                for a in found {
                    rows.push(vec![
                        stage.name.clone(),
                        provider.to_owned(),
                        a.variant.clone(),
                        load_label(load).to_owned(),
                        a.run.to_string(),
                        a.samples.to_string(),
                        a.p50_us().to_string(),
                        a.p95_us().to_string(),
                        note_for(map, &a.variant),
                    ]);
                }
            }
        }
    }
    Ok(rows)
}

fn field(build: &Value, key: &str) -> Result<String, EvalError> {
    build[key]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| invalid(format!("build.json has no string `{key}`")))
}

fn environment_rows(build_json: &str, cpu: &str, os: &str) -> Result<Vec<Vec<String>>, EvalError> {
    let build: Value = serde_json::from_str(build_json).map_err(|e| invalid(e.to_string()))?;
    let row = |key: &str, value: String| vec![key.to_owned(), value];
    Ok(vec![
        row("os", os.to_owned()),
        row("cpu", cpu.to_owned()),
        row("gpu", field(&build, "gpu_name")?),
        row("driver", field(&build, "gpu_driver")?),
        row(
            "sherpa-onnx",
            format!(
                "{} ({})",
                field(&build, "sherpa_onnx_tag")?,
                field(&build, "sherpa_onnx_commit")?
            ),
        ),
        row("onnxruntime", field(&build, "ort_version")?),
        row("cuda", field(&build, "cuda_version")?),
        row("cudnn", field(&build, "cudnn_version")?),
        row("build.json sha256", sha256_hex(build_json.as_bytes())),
    ])
}

fn jit_rows(runs: &[RunFile]) -> Result<Vec<Vec<String>>, EvalError> {
    let mut rows = Vec::new();
    for run in runs
        .iter()
        .filter(|run| run.meta.provider == "cuda" && !run.meta.load)
    {
        let aggs = aggregate(run)?;
        let find = |stage: &str| {
            aggs.iter()
                .find(|a| a.stage == stage && a.variant == "fp32")
        };
        let (Some(first), Some(decode)) = (find("decode_first"), find("decode")) else {
            continue;
        };
        rows.push(vec![
            run.meta.cache.clone(),
            run.meta.run.to_string(),
            ((first.p50_ns + 500_000) / 1_000_000).to_string(),
            decode.p50_us().to_string(),
        ]);
    }
    Ok(rows)
}

fn transfer_rows(transfers: &[TransferRow]) -> Vec<Vec<String>> {
    transfers
        .iter()
        .map(|t| {
            vec![
                t.direction.clone(),
                t.variant.clone(),
                t.count_p50.to_string(),
                t.count_p95.to_string(),
                t.bytes_p50.to_string(),
                t.bytes_p95.to_string(),
                t.time_us_p50.to_string(),
                t.time_us_p95.to_string(),
            ]
        })
        .collect()
}

fn describe(label: &str, b: &Best) -> String {
    format!("{label} {} µs（{}）", b.p50_us, b.variant)
}

/// The environment the translation stage was measured in.
const LLAMA_ENV: &str =
    "WSL の llama-server（CUDA 12.0 の build）で、本番の Windows、CUDA 13 の形ではない";

/// The section that lists what the numbers do not establish.
const UNVERIFIED: &str = "\n## 未検証\n\n- 訳 T は WSL の llama-server（CUDA 12.0 の build）で測った。設計の 3.12 は CUDA 13.4 だけを指していて食い違う。Windows の native、CUDA 13 の llama-server で測り直すまで、訳 T の行は本番の値ではない。\n- 負荷の列は、WSL の llama-server と Windows の ONNX Runtime が GPU を分けた値で、本番の分け方ではない（NOT VERIFIED）。\n- encoder だけの行の入力は、発話から作った log-mel で、認識器の front end とは値が同じではない。句読点の BERT の行の入力は、punct_ja_parity.json の最初の 23 case で、lat-ja の発話の文字列ではない。どちらも時間は入力の値ではなく長さに従うと見て使ったが、確かめていない。\n";

/// The line about the fp16 slowdown, from the counted placements; empty without both encoders counted.
fn fp16_placement_line(placed: &[PlacedRow]) -> String {
    let encoder = |fp16: bool| {
        placed
            .iter()
            .find(|p| p.model.starts_with("encoder") && p.model.contains(".fp16.") == fp16)
            .filter(|p| p.device == "cuda" && !p.model.contains(".int8."))
    };
    let (Some(fp32), Some(fp16)) = (encoder(false), encoder(true)) else {
        return String::new();
    };
    let verdict = if fp32.cpu_nodes.abs_diff(fp16.cpu_nodes) * 10 < fp32.cpu_nodes {
        "差が 1 割に満たないので、CPU に置かれた node の数では fp16 の遅さを説明できない。原因は未特定である"
    } else {
        "CPU に置かれた node の数に差があり、fp16 の遅さの原因の候補である（確かめていない）"
    };
    format!(
        "- CUDA で CPU に置かれた encoder の node は、fp32 が {}、fp16 が {}。{verdict}。\n",
        fp32.cpu_nodes, fp16.cpu_nodes
    )
}

/// Where ONNX Runtime put the nodes of the models it was asked to count; empty when none was counted.
fn placed_section(placed: &[PlacedRow]) -> String {
    if placed.is_empty() {
        return String::new();
    }
    let rows: Vec<Vec<String>> = placed
        .iter()
        .map(|p| {
            vec![
                p.model.clone(),
                p.device.clone(),
                p.cuda_nodes.to_string(),
                p.cpu_nodes.to_string(),
            ]
        })
        .collect();
    format!(
        "\n## node の置き場（ort）\n\n`ort` が session を作るときに ONNX Runtime が出す \"Node(s) placed on\" の行を、provider ごとに足した数。device は session を作った provider。\n\n{}",
        render(&["model", "device", "CUDA の node", "CPU の node"], &rows)
    )
}

/// The placement proposed for one stage and the numbers behind it.
fn proposal(map: &StageMap, aggs: &[Aggregate]) -> (String, String) {
    if map.ids.is_empty() {
        return (String::from("未決"), map.note.to_owned());
    }
    if map.ids.contains(&"encoder_only") {
        return recognition_proposal(aggs);
    }
    let gpu = best(aggs, "cuda", false, map.ids);
    let cpu = best(aggs, "cpu", false, map.ids);
    let (Some(gpu), Some(cpu)) = (gpu, cpu) else {
        return (
            String::from("未決"),
            String::from("無負荷の GPU か CPU の片方が測れていない"),
        );
    };
    let loaded = match (
        best(aggs, "cuda", true, map.ids),
        best(aggs, "cpu", true, map.ids),
    ) {
        (Some(g), Some(c)) => format!(
            "。負荷下の p50: {} / {}",
            describe("GPU", &g),
            describe("CPU", &c)
        ),
        _ => String::new(),
    };
    let numbers = format!(
        "無負荷の p50: {} / {}{loaded}",
        describe("GPU", &gpu),
        describe("CPU", &cpu)
    );
    if map.host_inside {
        return (
            String::from("CPU（sherpa-onnx の中。provider に依らず host で走る）"),
            numbers,
        );
    }
    if gpu.variant == FLOOR {
        return if cpu.p50_us < gpu.p50_us {
            (
                String::from("CPU"),
                format!("CPU の p50 が GPU の転送だけの下限より小さい。{numbers}"),
            )
        } else {
            (
                String::from("未決"),
                format!(
                    "CPU の p50 が GPU の転送の下限以上で、GPU の kernel の時間が要る。{numbers}"
                ),
            )
        };
    }
    let placement = if gpu.p50_us < cpu.p50_us {
        "GPU"
    } else {
        "CPU"
    };
    if map.ids.contains(&"translate") {
        return (
            placement.to_owned(),
            format!("{numbers}。測った環境: {LLAMA_ENV}"),
        );
    }
    (placement.to_owned(), numbers)
}

fn proposal_rows(table: &[StageRow], aggs: &[Aggregate]) -> Vec<Vec<String>> {
    table
        .iter()
        .zip(&STAGE_MAP)
        .map(|(stage, map)| {
            let (placement, evidence) = proposal(map, aggs);
            vec![stage.name.clone(), placement, evidence]
        })
        .collect()
}

fn percentile_pair(values: &[u64]) -> Result<(u64, u64), EvalError> {
    let signed: Vec<i64> = values
        .iter()
        .map(|v| i64::try_from(*v).map_err(|_| invalid("a value does not fit i64")))
        .collect::<Result<_, _>>()?;
    let at = |p: usize| -> Result<u64, EvalError> {
        u64::try_from(nearest_rank(&signed, p)?).map_err(|_| invalid("a percentile is negative"))
    };
    Ok((at(50)?, at(95)?))
}

fn h25_section(runs: &[RunFile]) -> Result<String, EvalError> {
    let mut rows = Vec::new();
    for provider in PROVIDERS {
        for load in [false, true] {
            let mut by_variant: BTreeMap<String, Vec<u64>> = BTreeMap::new();
            for run in runs.iter().filter(|r| {
                r.meta.provider == provider && r.meta.load == load && r.meta.cache != "empty"
            }) {
                for s in run.samples.iter().filter(|s| s.stage == "lid_long") {
                    by_variant.entry(s.variant.clone()).or_default().push(s.ns);
                }
            }
            for (variant, values) in by_variant {
                let (p50, p95) = percentile_pair(&values)?;
                rows.push(vec![
                    provider.to_owned(),
                    variant,
                    load_label(load).to_owned(),
                    values.len().to_string(),
                    p50.div_ceil(1000).to_string(),
                    p95.div_ceil(1000).to_string(),
                ]);
            }
        }
    }
    Ok(format!(
        "## H25 言語判定のやり直し\n\n先頭 4 秒を渡す言語判定を、4 秒以上の発話で確定のたびにやり直したときの 1 回の時間（µs）。標本は 4 秒以上の発話だけ。\n\n{}",
        render(
            &["provider", "variant", "負荷", "標本数", "p50_us", "p95_us"],
            &rows
        )
    ))
}

/// Medians of the Python run that the Rust numbers are set beside.
struct PythonStats {
    l: (u64, u64, usize),
    decode: (u64, u64, usize),
    non_decode_p50_ms: u64,
}

fn to_ms_pair(values: &[f64]) -> Result<(u64, u64, usize), EvalError> {
    let whole: Vec<u64> = values.iter().map(|v| v.max(0.0).round() as u64).collect();
    let (p50, p95) = percentile_pair(&whole)?;
    Ok((p50, p95, whole.len()))
}

fn python_stats(samples: &[PythonSample], min_silence_ms: f64) -> Result<PythonStats, EvalError> {
    let valid: Vec<&PythonSample> = samples
        .iter()
        .filter(|s| !NO_L_ASR.contains(&s.utt))
        .filter(|s| match (s.l_asr_ms, s.decode_ms) {
            (Some(l_asr), Some(decode)) => l_asr - decode >= min_silence_ms,
            _ => false,
        })
        .collect();
    let decode: Vec<f64> = valid.iter().filter_map(|s| s.decode_ms).collect();
    let non_decode: Vec<f64> = valid
        .iter()
        .filter_map(|s| Some(s.l_asr_ms? - s.decode_ms?))
        .collect();
    let l: Vec<f64> = valid
        .iter()
        .filter(|s| !NO_L.contains(&s.utt))
        .filter_map(|s| s.l_ms)
        .collect();
    Ok(PythonStats {
        l: to_ms_pair(&l)?,
        decode: to_ms_pair(&decode)?,
        non_decode_p50_ms: to_ms_pair(&non_decode)?.0,
    })
}

/// The translation times (ns) of the warm runs of one provider and load, by (repeat, utterance).
/// A later run overwrites an earlier one.
fn translations(runs: &[RunFile], provider: &str, load: bool) -> BTreeMap<(u32, u32), u64> {
    let mut found = BTreeMap::new();
    for run in runs
        .iter()
        .filter(|r| r.meta.provider == provider && r.meta.load == load && r.meta.cache != "empty")
    {
        for s in run.samples.iter().filter(|s| s.stage == "translate") {
            found.insert((s.repeat, s.utt), s.ns);
        }
    }
    found
}

/// Recognition, and recognition plus translation, in microseconds per (repeat, utterance)
/// of the warm runs of one provider and load. The translation comes from the runs of
/// `translate_provider`: the llama-server is on the GPU whatever the recognition runs on.
fn rust_delays(
    runs: &[RunFile],
    provider: &str,
    translate_provider: &str,
    load: bool,
) -> Option<(Vec<u64>, Vec<u64>)> {
    let mut decode_only = Vec::new();
    let mut with_translation = Vec::new();
    let pooled = translations(runs, translate_provider, load);
    for run in runs
        .iter()
        .filter(|r| r.meta.provider == provider && r.meta.load == load && r.meta.cache != "empty")
    {
        let Some(first_variant) = run
            .samples
            .iter()
            .find(|s| s.stage == "recognize_total")
            .map(|s| s.variant.as_str())
        else {
            continue;
        };
        let mut recognized: BTreeMap<(u32, u32), u64> = BTreeMap::new();
        for s in &run.samples {
            if s.stage == "recognize_total" && s.variant == first_variant {
                recognized.insert((s.repeat, s.utt), s.ns);
            }
        }
        let own;
        let translated = if translate_provider == provider {
            own = translations(std::slice::from_ref(run), provider, load);
            &own
        } else {
            &pooled
        };
        for ((repeat, utt), ns) in recognized {
            if NO_L_ASR.contains(&utt) {
                continue;
            }
            decode_only.push(ns / 1000);
            if let Some(t) = translated
                .get(&(repeat, utt))
                .filter(|_| !NO_L.contains(&utt))
            {
                with_translation.push((ns + t) / 1000);
            }
        }
    }
    if decode_only.is_empty() {
        return None;
    }
    Some((decode_only, with_translation))
}

fn delay_section(inputs: &Inputs<'_>) -> Result<String, EvalError> {
    let python = python_stats(inputs.python, inputs.min_silence_ms)?;
    let mut rows = vec![
        vec![
            String::from("Python（記録）"),
            String::from("final の latency_ms（L_dec）"),
            python.decode.2.to_string(),
            python.decode.0.to_string(),
            python.decode.1.to_string(),
        ],
        vec![
            String::from("Python（記録）"),
            String::from("L（発話の終わりから訳）"),
            python.l.2.to_string(),
            python.l.0.to_string(),
            python.l.1.to_string(),
        ],
    ];
    let silence_ms = inputs.min_silence_ms.round() as u64;
    // (recognition on, translation on): the llama-server stays on the GPU when only the recognition moves.
    for (provider, translate_provider) in [("cuda", "cuda"), ("cpu", "cpu"), ("cpu", "cuda")] {
        for load in [false, true] {
            let Some((decode, with_translation)) =
                rust_delays(inputs.runs, provider, translate_provider, load)
            else {
                continue;
            };
            let label = format!(
                "Rust 認識 {provider} / 訳 {translate_provider} {}",
                load_label(load)
            );
            let (d50, d95) = percentile_pair(&decode)?;
            if provider == translate_provider {
                rows.push(vec![
                    label.clone(),
                    String::from("認識（create_stream から get_result）"),
                    decode.len().to_string(),
                    (d50 / 1000).to_string(),
                    (d95 / 1000).to_string(),
                ]);
            }
            let overhead_us = python.non_decode_p50_ms * 1000;
            let estimate: Vec<u64> = with_translation.iter().map(|us| us + overhead_us).collect();
            if estimate.is_empty() {
                continue;
            }
            let (e50, e95) = percentile_pair(&estimate)?;
            rows.push(vec![
                label,
                format!(
                    "L の見積り = Python の認識以外の遅れの p50 {} ms + 認識 + 訳 T",
                    python.non_decode_p50_ms
                ),
                estimate.len().to_string(),
                (e50 / 1000).to_string(),
                (e95 / 1000).to_string(),
            ]);
        }
    }
    Ok(format!(
        "## 遅れ L の見積り\n\nRust の実行系（取り込みから字幕まで）がまだ無いので、`hayamimi-eval stream` で `L` は測れない。\n次の表は、測れた段の和である。単位は ms。Python の行は `lat-ja` を `--min-silence` {silence_ms} ms で流した記録で、有効な標本だけ、`L` は 19 発話（発話 5、10、18、19 を除く）、認識は 20 発話（発話 10、19 を除く）。\n「L の見積り」の足し算は、VAD の区切りと配信の遅れを Python と同じと仮定した値（ASSUMPTION）で、句読点、ITN、字幕の配信を含まない。\n「Rust 認識 X / 訳 Y」は、認識を X の run、訳 T を Y の run から取った。llama-server は本番でも GPU に置くので、認識だけを CPU へ置く形は「認識 cpu / 訳 cuda」の行である。\n\n{}",
        render(&["実装", "指標", "標本数", "p50", "p95"], &rows)
    ))
}

/// The result document for `inputs`.
pub fn render_document(inputs: &Inputs<'_>) -> Result<String, EvalError> {
    let table = parse_stage_table(inputs.design)?;
    if table.len() != STAGE_MAP.len() {
        return Err(invalid(format!(
            "the design table has {} rows, the stage map {}",
            table.len(),
            STAGE_MAP.len()
        )));
    }
    let aggs = warm_aggregates(inputs.runs)?;
    let os = inputs
        .runs
        .first()
        .map(|run| run.meta.os.clone())
        .ok_or_else(|| invalid("no run files"))?;
    let mut text = String::from("# 段ごとの GPU と CPU の時間\n\n");
    text.push_str(&format!("{ENVIRONMENT}\n\n"));
    text.push_str(&render(
        &ENVIRONMENT_HEADER,
        &environment_rows(inputs.build_json, inputs.cpu, &os)?,
    ));
    text.push_str(&format!(
        "\n{STAGES}\n\n単位は µs（1 µs 未満は 1 に切り上げる）。標本は発話数 × 繰り返し。負荷は、GPU の llama-server へ訳の依頼を送り続ける 2 本の client。\n\n"
    ));
    text.push_str(&render(&CELL_HEADER, &cell_rows(&table, &aggs)?));
    text.push_str(&format!(
        "\n{TRANSFERS}\n\n`nsys profile` の capture range を認識の 1 呼び出し（decode）ごとに開き、1 発話あたりの値の p50 と p95 を取った。\n\n"
    ));
    text.push_str(&render(&TRANSFER_HEADER, &transfer_rows(inputs.transfers)));
    text.push_str(&format!(
        "\n{JIT}\n\ncache が空の process と、同じ cache で続けた process の、認識の最初の 1 回（fp32、create の後）と p50。\n\n"
    ));
    text.push_str(&render(&JIT_HEADER, &jit_rows(inputs.runs)?));
    text.push_str(&placed_section(inputs.placed));
    text.push_str(&format!("\n{PROPOSALS}\n\n"));
    text.push_str(&render(&PROPOSAL_HEADER, &proposal_rows(&table, &aggs)));
    text.push('\n');
    text.push_str(&h25_section(inputs.runs)?);
    text.push('\n');
    text.push_str(&delay_section(inputs)?);
    text.push_str(UNVERIFIED);
    text.push_str(&fp16_placement_line(inputs.placed));
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stages::{Meta, Sample};

    fn sample(stage: &str, variant: &str, utt: u32, ns: u64) -> Sample {
        Sample {
            stage: stage.to_owned(),
            variant: variant.to_owned(),
            utt,
            repeat: 0,
            ns,
        }
    }

    fn run(provider: &str, load: bool, cache: &str, samples: Vec<Sample>) -> RunFile {
        RunFile {
            meta: Meta {
                provider: provider.to_owned(),
                load,
                cache: cache.to_owned(),
                run: 1,
                repeat: 1,
                utterances: 1,
                os: String::from("windows"),
            },
            samples,
        }
    }

    #[test]
    fn the_stage_map_has_one_entry_per_design_row_and_distinct_prefixes() {
        let mut prefixes: Vec<&str> = STAGE_MAP.iter().map(|m| m.prefix).collect();
        prefixes.sort_unstable();
        prefixes.dedup();
        assert_eq!(prefixes.len(), 19);
    }

    #[test]
    fn the_empty_cache_runs_are_left_out_of_the_stage_cells() -> Result<(), EvalError> {
        let runs = [
            run(
                "cuda",
                false,
                "empty",
                vec![sample("vad", "fp32", 0, 9_000_000)],
            ),
            run(
                "cuda",
                false,
                "warm",
                vec![sample("vad", "fp32", 0, 1_000_000)],
            ),
        ];
        let aggs = warm_aggregates(&runs)?;
        assert_eq!(aggs.len(), 1);
        assert_eq!(aggs[0].p50_us(), 1000);
        Ok(())
    }

    #[test]
    fn the_best_variant_is_the_one_with_the_smaller_p50() -> Result<(), EvalError> {
        let runs = [run(
            "cuda",
            false,
            "warm",
            vec![
                sample("decode", "fp32", 0, 5_000_000),
                sample("decode", "fp16", 0, 3_000_000),
            ],
        )];
        let aggs = warm_aggregates(&runs)?;
        let found = best(&aggs, "cuda", false, &["decode"]);
        assert_eq!(
            found.map(|b| (b.variant, b.p50_us)),
            Some((String::from("fp16"), 3000))
        );
        assert_eq!(best(&aggs, "cpu", false, &["decode"]), None);
        Ok(())
    }

    fn map_of(ids: &'static [&'static str], host_inside: bool) -> StageMap {
        map("x", ids, "n", host_inside)
    }

    #[test]
    fn a_stage_that_is_not_measured_stays_undecided_with_its_reason() {
        let (placement, evidence) = proposal(&map_of(&[], false), &[]);
        assert_eq!((placement.as_str(), evidence.as_str()), ("未決", "n"));
    }

    #[test]
    fn a_cpu_that_beats_the_transfer_floor_is_proposed() -> Result<(), EvalError> {
        let runs = [
            run(
                "cuda",
                false,
                "warm",
                vec![sample("judge", FLOOR, 0, 50_000)],
            ),
            run("cpu", false, "warm", vec![sample("judge", "cpu", 0, 300)]),
        ];
        let aggs = warm_aggregates(&runs)?;
        let (placement, _) = proposal(&map_of(&["judge"], false), &aggs);
        assert_eq!(placement, "CPU");
        Ok(())
    }

    #[test]
    fn a_cpu_slower_than_the_transfer_floor_leaves_the_stage_undecided() -> Result<(), EvalError> {
        let runs = [
            run("cuda", false, "warm", vec![sample("x", FLOOR, 0, 50_000)]),
            run("cpu", false, "warm", vec![sample("x", "cpu", 0, 900_000)]),
        ];
        let aggs = warm_aggregates(&runs)?;
        let (placement, _) = proposal(&map_of(&["x"], false), &aggs);
        assert_eq!(placement, "未決");
        Ok(())
    }

    #[test]
    fn two_real_implementations_are_compared_by_p50() -> Result<(), EvalError> {
        let runs = [
            run(
                "cuda",
                false,
                "warm",
                vec![sample("vad", "fp32", 0, 2_000_000)],
            ),
            run(
                "cpu",
                false,
                "warm",
                vec![sample("vad", "int8", 0, 1_000_000)],
            ),
        ];
        let aggs = warm_aggregates(&runs)?;
        let (placement, _) = proposal(&map_of(&["vad"], false), &aggs);
        assert_eq!(placement, "CPU");
        Ok(())
    }

    #[test]
    fn a_missing_side_is_undecided() -> Result<(), EvalError> {
        let runs = [run(
            "cuda",
            false,
            "warm",
            vec![sample("vad", "fp32", 0, 2_000_000)],
        )];
        let aggs = warm_aggregates(&runs)?;
        let (placement, _) = proposal(&map_of(&["vad"], false), &aggs);
        assert_eq!(placement, "未決");
        Ok(())
    }

    #[test]
    fn jit_rows_come_from_the_idle_cuda_runs_with_their_cache_state() -> Result<(), EvalError> {
        let first = vec![
            sample("decode_first", "fp32", 0, 900_000_000),
            sample("decode", "fp32", 0, 700_000_000),
        ];
        let runs = [
            run("cuda", false, "empty", first.clone()),
            run("cuda", false, "warm", first),
            run("cuda", true, "warm", vec![]),
            run("cpu", false, "warm", vec![]),
        ];
        let rows = jit_rows(&runs)?;
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], ["empty", "1", "900", "700000"]);
        assert_eq!(rows[1][0], "warm");
        Ok(())
    }

    fn python(utt: u32, l_asr: f64, decode: f64, l: f64) -> PythonSample {
        PythonSample {
            utt,
            l_asr_ms: Some(l_asr),
            decode_ms: Some(decode),
            l_ms: Some(l),
        }
    }

    #[test]
    fn the_python_baseline_keeps_valid_samples_of_the_compare_set() -> Result<(), EvalError> {
        let samples = [
            python(0, 1000.0, 500.0, 1500.0),
            python(1, 700.0, 500.0, 900.0),
            python(9, 2000.0, 100.0, 2500.0),
            python(4, 1500.0, 100.0, 1900.0),
        ];
        let stats = python_stats(&samples, 350.0)?;
        // Utterance 1 is not valid (200 ms after the decode), utterance 9 is outside the set.
        assert_eq!(stats.decode.2, 2);
        // Utterance 4 has an L_asr but no L in the compare set.
        assert_eq!(stats.l.2, 1);
        assert_eq!(stats.l.0, 1500);
        Ok(())
    }

    #[test]
    fn environment_rows_state_every_required_key() -> Result<(), EvalError> {
        let build = r#"{"gpu_name":"G","gpu_driver":"1","sherpa_onnx_tag":"v1","sherpa_onnx_commit":"abc","ort_version":"1.31.0","cuda_version":"13.4.92","cudnn_version":"9.27.0"}"#;
        let rows = environment_rows(build, "CPU", "windows")?;
        for key in crate::gpu_results::ENVIRONMENT_KEYS {
            assert!(
                rows.iter().any(|r| r[0] == key && !r[1].is_empty()),
                "{key}"
            );
        }
        Ok(())
    }

    #[test]
    fn a_python_row_becomes_times_since_the_end_of_speech() -> Result<(), EvalError> {
        let row = r#"{"repeat":1,"utt":3,"t_end_us":1000000,"t_final_us":2500000,"t_tr_us":null,"final_latency_ms":600.5}"#;
        let got = python_sample(row, std::path::Path::new("p.jsonl"))?;
        assert_eq!(got.utt, 2);
        assert_eq!(got.l_asr_ms, Some(1500.0));
        assert_eq!(got.l_ms, None);
        assert_eq!(got.decode_ms, Some(600.5));
        Ok(())
    }

    #[test]
    fn a_python_row_without_an_utterance_number_is_an_error() {
        let row = r#"{"t_end_us":1}"#;
        assert!(python_sample(row, std::path::Path::new("p.jsonl")).is_err());
    }

    #[test]
    fn recognition_on_the_cpu_takes_the_translation_of_the_gpu_run() -> Result<(), EvalError> {
        let runs = [
            run(
                "cpu",
                false,
                "warm",
                vec![
                    sample("recognize_total", "int8", 0, 100_000_000),
                    sample("translate", "llm", 0, 9_000_000_000),
                ],
            ),
            run(
                "cuda",
                false,
                "warm",
                vec![
                    sample("recognize_total", "fp32", 0, 300_000_000),
                    sample("translate", "llm", 0, 200_000_000),
                ],
            ),
        ];
        let Some((_, mixed)) = rust_delays(&runs, "cpu", "cuda", false) else {
            return Err(invalid("no delays for the cpu recognition"));
        };
        assert_eq!(mixed, vec![300_000]);
        let Some((_, own)) = rust_delays(&runs, "cpu", "cpu", false) else {
            return Err(invalid("no delays for the cpu recognition"));
        };
        assert_eq!(own, vec![9_100_000]);
        Ok(())
    }

    #[test]
    fn the_recognition_row_pairs_the_same_precision_and_decides_nothing() -> Result<(), EvalError> {
        let runs = [
            run(
                "cuda",
                false,
                "warm",
                vec![
                    sample("decode", "fp32", 0, 170_000_000),
                    sample("decode", "fp16", 0, 468_000_000),
                    sample("encoder_only", ENCODER_ONLY, 0, 58_000_000),
                ],
            ),
            run(
                "cpu",
                false,
                "warm",
                vec![
                    sample("decode", "fp32", 0, 150_000_000),
                    sample("decode", "int8", 0, 125_000_000),
                    sample("encoder_only", ENCODER_ONLY, 0, 99_000_000),
                ],
            ),
        ];
        let (placement, evidence) = recognition_proposal(&warm_aggregates(&runs)?);
        assert!(placement.starts_with("未決"), "{placement}");
        assert!(
            evidence.contains("無負荷の p50: GPU 170000 µs / CPU 150000 µs"),
            "{evidence}"
        );
        assert!(
            evidence.contains("無負荷の p50: GPU 58000 µs / CPU 99000 µs"),
            "{evidence}"
        );
        assert!(evidence.contains("CPU の int8 は 125000 µs"), "{evidence}");
        assert!(evidence.contains("GPU の fp16 は 468000 µs"), "{evidence}");
        Ok(())
    }

    #[test]
    fn a_side_that_was_not_measured_is_said_not_filled() -> Result<(), EvalError> {
        let runs = [run(
            "cuda",
            false,
            "warm",
            vec![sample("decode", "fp32", 0, 170_000_000)],
        )];
        let (_, evidence) = recognition_proposal(&warm_aggregates(&runs)?);
        assert!(evidence.contains("片方が測れていない"), "{evidence}");
        assert!(
            evidence.contains("CPU の int8 は 測れていない"),
            "{evidence}"
        );
        Ok(())
    }

    fn placed_on(model: &str, cpu_nodes: u64) -> PlacedRow {
        PlacedRow {
            model: model.to_owned(),
            device: String::from("cuda"),
            cuda_nodes: 3500,
            cpu_nodes,
        }
    }

    #[test]
    fn the_fp16_line_says_unexplained_when_the_cpu_nodes_barely_differ() {
        let line = fp16_placement_line(&[
            placed_on("encoder-epoch-35-avg-1.onnx", 1599),
            placed_on("encoder-epoch-35-avg-1.fp16.onnx", 1611),
        ]);
        assert!(line.contains("fp32 が 1599、fp16 が 1611"), "{line}");
        assert!(line.contains("説明できない"), "{line}");
    }

    #[test]
    fn the_fp16_line_names_a_large_difference_as_a_candidate() {
        let line = fp16_placement_line(&[
            placed_on("encoder-epoch-35-avg-1.onnx", 100),
            placed_on("encoder-epoch-35-avg-1.fp16.onnx", 900),
        ]);
        assert!(line.contains("原因の候補"), "{line}");
    }

    #[test]
    fn without_both_encoders_there_is_no_fp16_line() {
        assert_eq!(fp16_placement_line(&[]), "");
        assert_eq!(
            fp16_placement_line(&[placed_on("encoder-epoch-35-avg-1.onnx", 1)]),
            ""
        );
    }

    #[test]
    fn the_node_placement_section_is_left_out_when_nothing_was_counted() {
        assert_eq!(placed_section(&[]), "");
    }

    #[test]
    fn a_build_json_without_the_gpu_name_is_an_error() {
        assert!(environment_rows("{}", "CPU", "windows").is_err());
    }
}
