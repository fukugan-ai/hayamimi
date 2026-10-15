# R8v step 1: offline probe of the early end-of-speech detector (second run)

The probe is `native/hayamimi-asr/tests/r8v_probe.rs` (`#[ignore]`, offline). It changes no product code and no default:

    cargo test --release -p hayamimi-asr --test r8v_probe -- --ignored --nocapture r8v_latency_set_probe

The held-out sets are in `heldout.md`.

## Conditions

- Input: `testdata/lat-ja.wav` (23 utterances) and `testdata/lat-en.wav` (15), with the manifests under `testdata/manifests/`.
- Current detector A: threshold 0.5, min_silence 0.30 s, max_speech 12 s, shortest speech 0.25 s (the production settings).
- Early detector B: threshold 0.65 (its end line is 0.50), min_silence 0.12 s, max_speech 12 s, shortest speech 0.25 s.
- A and B are fed the same 512-sample windows in the same order. A notice of B is a **candidate** only when A is inside speech at that window. The candidate sample is the end of that window.
- The early path is replayed on each segment of A. A candidate is solved from the **line** to the candidate sample through `Engine::transcribe` (recognition, retry, ITN, punctuation, dictionary; speaker stage off; a new engine per candidate with `Policy::single(lang)`; a reused engine is not used after `finish`). The line starts at the voice start of A minus the 1.0 s pre-roll. A complete text confirms the candidate and moves the line to it. What is left between the last line and the end of the segment is solved once more when it is at least 0.5 s long, and it counts as a final when it holds text.
- A candidate is **inside an utterance** when voice goes on after it inside the same segment of A: B reads the start of another stretch of voice after the candidate's end and before the segment's end. The manifest `end_sample` is not used for this.
- The completeness table is the one of the first run (`JA_FINAL`, `JA_CONNECTIVE`, `EN_CONTINUATION` in the probe) and did not change.
- **Split.** A **split by A** is a segment of A beyond the first that touches one utterance. A **split by B alone** is a final beyond the first that the early path makes out of one segment of A (confirmed candidates plus a remainder with text).
- Delay is the sample at which a final is made minus the manifest `end_sample`. "First complete or A" takes the first confirmed candidate of the utterance, and the detection of A for an utterance that has none; it is the figure of the limit formula of 3.17 (count 5). "Last final" takes the sample of the last final the early path makes. "A alone" is the detection of A.
- Percentiles are nearest rank. vad_ms is the time of `accept_waveform` plus the segment take per window.

## Result

| item | lat-ja | lat-en |
|---|---|---|
| utterances | 23 | 15 |
| utterances with no segment of A | 1 | 0 |
| segments of A | 26 | 19 |
| candidates (B notices inside A's speech) | 35 | 22 |
| candidates with voice going on after them | 9 | 3 |
| candidates judged complete | 22 | 18 |
| complete candidates with voice going on | 3 | 2 |
| splits by A | 4 | 4 |
| **splits by B alone** | **4** | **2** |
| splits in total with B | 8 | 6 |
| delay, first complete or A, p50 / p95 | 286 / 880 ms | 166 / 363 ms |
| delay, last final, p50 / p95 | 508 / 880 ms | 186 / 550 ms |
| delay, A alone, p50 / p95 | 824 / 1177 ms | 378 / 550 ms |
| vad_ms per window, A alone, p50 / p95 | 0.116 / 0.208 ms | 0.140 / 0.239 ms |
| vad_ms per window, A plus B, p50 / p95 | 0.238 / 0.410 ms | 0.278 / 0.462 ms |

The delay rows have n = 22 for ja (the utterance with no segment of A has no detection time) and n = 15 for en.

Utterances where B alone adds a final (ja: 4, 12, 19, 20 with one segment of A and two finals; en: 0 and 14 likewise), with the segments of A for the others, are the `UTT` lines of the run. The complete candidates with voice going on:

| lang | text |
|---|---|
| ja | アピヤはサモアの首都です。 |
| ja | 言ったろう。 |
| ja | 高校生200人に聞きました。 |
| en | He was in a fevered state of mind. |
| en | His first impulse was to write but four words in reply. |

## The pass line

The line: splits by B alone are 0, and the splits in total with B are not more than the splits by A.

- lat-ja: **not passed.** Splits by B alone are 4; the total with B is 8 against 4 by A.
- lat-en: **not passed.** Splits by B alone are 2; the total with B is 6 against 4 by A.
- vad_ms p95 of A plus B is 0.410 ms (ja) and 0.462 ms (en) against the limit of 2 ms: passed.

Step 2 does not start on this result; it goes to the council.

## The ja limit of RP-66

With the formula of 3.17 (count 5): P_B is the p95 of "first complete or A" (880 ms), P_A is the p95 of "A alone" (1177 ms), the p95 of `d` of the current main is 1341 ms. The limit is 880 + (1341 - 1177) = 1044 ms, rounded up to 10 ms: **1050 ms**. It is below 1341 ms, so the stop for the limit does not apply. 1050 ms is the value the RP-66 row already holds, so no row changes on account of the limit. The figure rests on 22 of 23 utterances (see below) and on the fitted thresholds, and it has no held-out check yet.

## 手番 2 の前に要る物

- **No held-out set for the thresholds.** 0.65 and 0.12 were chosen on the same `lat-ja` and `lat-en` that this probe measures. The held-out run is in `heldout.md`.
- **`Final.speaker_audio` comes from the segment.** It is taken from `segment.samples` in `process` (`native/hayamimi-asr/src/pipeline.rs:680`). If the early path discards the segment, the source of that audio is lost and the speaker stage gets nothing for that final. Step 2 takes it from the history, from the voice start of A to the line (3.17, count 4).
- **One utterance of lat-ja has no segment of A.** The delay figures leave it out (n = 22). Which utterance it is, and whether A misses it or merges it into a neighbour, is not shown by this run (NOT VERIFIED).
- **The solved slice approximates `process`.** The probe feeds each slice to a fresh engine, whose own detector may cut the slice again. A production early path hands one segment to `process`.
- **Remainder rule.** The remainder after the last line counts as a final when its text is not empty and it is at least 0.5 s long. A shorter remainder is not solved, so splits by B alone can be under-counted by it.

## 採らない手段

- Changing `THRESHOLD`, `min_silence` or `max_speech` of the production detector: not done; the probe builds its own detectors.
- Judging "inside an utterance" by the manifest `end_sample`: replaced by the voice that goes on inside the segment of A.
- Dropping the utterances with no complete candidate from the delay: they take the detection of A.
- Reusing one engine for all candidates: the detector of a reused engine aborts after `finish` until RP-68 is fixed.
- Touching `vad.rs`, `pipeline.rs`, `args.rs` or `session.rs`: not touched.
