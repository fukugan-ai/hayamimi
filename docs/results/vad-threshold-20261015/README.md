# VAD cutoff probe: 0.5, 0.6, 0.7

The Silero speech-probability cutoff stays at **0.5** (`THRESHOLD` in `native/hayamimi-asr/src/vad.rs`). 0.6 and 0.7 are not adopted.

The probe is `native/hayamimi-asr/tests/vad_threshold_probe.rs` (`#[ignore]`, an offline probe):

    cargo test --release -p hayamimi-asr --test vad_threshold_probe -- --ignored --nocapture

It cuts `testdata/lat-ja.wav` (23 utterances) and `testdata/lat-en.wav` (15) with the production detector settings (`VadSettings::default()`: min_silence 0.30 s, max_speech 12 s). Only the cutoff varies. Each segment is compared with the utterance windows of `testdata/manifests/lat-{ja,en}.json`.

- end difference: end of the last segment overlapping the utterance minus the manifest `end_sample` (ms)
- splits: segments overlapping the utterance minus one
- start shift: start of the first segment minus the manifest `start_sample`, judged against the same utterance at 0.5

## Result

| lang | cutoff | end diff p50 | end diff p95 | splits (sum) | start shift vs 0.5 (max) | judgement |
|---|---|---|---|---|---|---|
| ja | 0.5 | 524 ms | 877 ms | 4 | baseline | baseline |
| ja | 0.6 | 288 ms | 781 ms | 6 | +256 ms (utt 9, 20) | rejected |
| ja | 0.7 | 288 ms | 685 ms | 8 | +3360 ms (utt 11) | rejected |
| en | 0.5 | 78 ms | 250 ms | 4 | baseline | baseline |
| en | 0.6 | 78 ms | 250 ms | 4 | +32 ms | not rejected |
| en | 0.7 | 76 ms | 250 ms | 4 | +32 ms | not rejected |

## Why 0.6 and 0.7 are not adopted

The council's stop rule: a cutoff is rejected when the splits inside utterances grow, or a start is later than 100 ms against 0.5.

- ja splits grow: 4 at 0.5, 6 at 0.6 (utt 2 and 20 split more), 8 at 0.7 (utt 2, 11, 20, 21 split more).
- ja starts move later: utt 9 and 20 by 256 ms at 0.6; utt 11 by 3360 ms, utt 13 by 928 ms, utt 20 and 9 by 256 ms, utt 6 by 224 ms at 0.7.
- The ja gain that was looked for is real but is not enough: the end difference p50 falls from 524 to 288 ms at 0.6 and 0.7.
- en does not move (splits 4, start shift 32 ms), and its end difference does not shrink, so there is nothing to gain there either.

The end difference also goes below zero at a higher cutoff in some utterances (ja utt 13: +326 ms at 0.5, +38 ms at 0.6, -729 ms at 0.7), which can mean the segment ends before the voice does. This probe does not check that against the audio.

## Limits of the count

- A split is counted only inside the manifest window of an utterance: segments that overlap the window, minus one. A split outside every window is not counted.
- A segment is assigned to an utterance by overlap. The manifest windows are 1.5 s apart, so a segment overlaps at most one window in this audio; another audio may not.
- ja utt 10 has no segment at any cutoff (missed 1 at 0.5, 0.6 and 0.7) and is outside every statistic.
- ja utt 19 ends 1818 ms before the manifest end at 0.5. Design 3.1 says a manifest end can lie after the voice when music follows; this was not checked here. Such rows stay in the percentiles.
- The absolute start shift (up to 3547 ms at 0.5) is not a VAD error: `start_sample` is the start of the clip window and the clip begins with silence. Only the shift against 0.5 is judged.
- The end difference is the end of the segment, not the time the final is emitted; a segment is emitted after min_silence.
- One run per cutoff; the detector is deterministic here, and the numbers were identical on a second run.

## 採らない手段 (not taken)

- Changing the default `THRESHOLD` in `vad.rs`.
- Changing the acceptance binary at the same time as the cutoff.
- Moving the definition of `t_end`.

## Per-utterance rows (ja, the utterances that decide the judgement)

`end_diff_ms / segments / start_diff_ms`; the full rows are not kept in git.

| utt | 0.5 | 0.6 | 0.7 |
|---|---|---|---|
| 2 | 628 / 2 / 2826 | 564 / 3 / 2826 | 532 / 3 / 2826 |
| 9 | 544 / 1 / 894 | 288 / 1 / 1150 | 288 / 1 / 1150 |
| 10 | none | none | none |
| 11 | 370 / 1 / 937 | 242 / 1 / 937 | 146 / 2 / 4297 |
| 13 | 326 / 1 / 575 | 38 / 1 / 575 | -729 / 1 / 1503 |
| 14 | 395 / 3 / 188 | 267 / 3 / 188 | 235 / 3 / 188 |
| 18 | -595 / 2 / 2766 | -691 / 2 / 2766 | -755 / 2 / 2766 |
| 19 | -1818 / 1 / -8 | -2010 / 1 / -8 | -2202 / 1 / 23 |
| 20 | 524 / 1 / 848 | 428 / 2 / 1104 | 300 / 2 / 1104 |
| 21 | 665 / 1 / 1186 | 537 / 1 / 1186 | 441 / 2 / 1186 |
